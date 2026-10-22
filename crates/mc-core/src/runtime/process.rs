use super::LaunchPlan;
use crate::{LauncherError, LauncherResult};
use std::{path::PathBuf, process::Stdio};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
    sync::mpsc,
    task::JoinHandle,
};

#[derive(Debug, Clone)]
pub struct ProcessExit {
    pub code: Option<i32>,
    pub success: bool,
    pub log_path: PathBuf,
    pub log_error: Option<String>,
}

pub struct GameProcess {
    child: Child,
    log_path: PathBuf,
    pumps: Vec<JoinHandle<std::io::Result<()>>>,
    logger: JoinHandle<std::io::Result<()>>,
}

impl GameProcess {
    pub fn id(&self) -> Option<u32> {
        self.child.id()
    }
    pub fn log_path(&self) -> &std::path::Path {
        &self.log_path
    }
    pub async fn kill(&mut self) -> LauncherResult<()> {
        Ok(self.child.kill().await?)
    }

    /// 消费 stdout/stderr 的任务与 wait 并行运行，避免管道写满导致 Java 卡死。
    pub async fn wait(mut self) -> LauncherResult<ProcessExit> {
        let status = self.child.wait().await?;
        let mut log_errors = Vec::new();
        for pump in self.pumps {
            match pump.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => log_errors.push(error.to_string()),
                Err(error) => log_errors.push(error.to_string()),
            }
        }
        match self.logger.await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => log_errors.push(error.to_string()),
            Err(error) => log_errors.push(error.to_string()),
        }
        Ok(ProcessExit {
            code: status.code(),
            success: status.success(),
            log_path: self.log_path,
            log_error: (!log_errors.is_empty()).then(|| log_errors.join("; ")),
        })
    }
}

pub(crate) async fn spawn(plan: LaunchPlan) -> LauncherResult<GameProcess> {
    // 再检查已有父目录，避免 prepare 与执行之间目录被替换为越界链接。
    super::layout::checked_join(&plan.game_directory, std::path::Path::new(".launcher"))?;
    super::layout::checked_join(
        &plan.game_directory,
        &std::path::PathBuf::from(format!(
            ".launcher/natives/{}",
            plan.natives_directory
                .file_name()
                .unwrap()
                .to_string_lossy()
        )),
    )?;
    super::layout::checked_join(
        &plan.game_directory,
        &std::path::PathBuf::from(format!(
            ".launcher/logs/{}",
            plan.log_path.file_name().unwrap().to_string_lossy()
        )),
    )?;
    let native_path = plan.natives_directory.clone();
    let archives = plan.natives;
    tokio::task::spawn_blocking(move || super::natives::extract(&archives, &native_path))
        .await
        .map_err(|e| LauncherError::Internal(e.to_string()))??;
    tokio::fs::create_dir_all(plan.log_path.parent().unwrap()).await?;
    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&plan.log_path)
        .await?;
    let mut command = Command::new(&plan.executable);
    command
        .args(&plan.arguments)
        .current_dir(&plan.game_directory)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // 禁止用户环境变量偷偷注入 JVM agent/选项；Java 的其余运行环境正常继承。
    for name in [
        "JAVA_TOOL_OPTIONS",
        "_JAVA_OPTIONS",
        "JDK_JAVA_OPTIONS",
        "CLASSPATH",
    ] {
        command.env_remove(name);
    }
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command.spawn()?;
    let (sender, mut receiver) = mpsc::channel::<Vec<u8>>(32);
    let logger = tokio::spawn(async move {
        while let Some(bytes) = receiver.recv().await {
            file.write_all(&bytes).await?;
        }
        file.flush().await
    });
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let a = tokio::spawn(pump(stdout, sender.clone(), plan.secrets.clone()));
    let b = tokio::spawn(pump(stderr, sender, plan.secrets));
    Ok(GameProcess {
        child,
        log_path: plan.log_path,
        pumps: vec![a, b],
        logger,
    })
}

async fn pump(
    mut input: impl AsyncRead + Unpin,
    sender: mpsc::Sender<Vec<u8>>,
    secrets: Vec<String>,
) -> std::io::Result<()> {
    let secrets: Vec<_> = secrets
        .into_iter()
        .filter(|s| !s.is_empty())
        .map(String::into_bytes)
        .collect();
    let mut pending = Vec::new();
    let mut chunk = [0_u8; 8192];
    loop {
        let count = input.read(&mut chunk).await?;
        pending.extend_from_slice(&chunk[..count]);
        let (output, consumed) = redact_chunk(&pending, &secrets, count == 0);
        pending.drain(..consumed);
        if !output.is_empty() && sender.send(output).await.is_err() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "启动日志写入任务已停止",
            ));
        }
        if count == 0 {
            break;
        }
    }
    Ok(())
}

fn redact_chunk(input: &[u8], secrets: &[Vec<u8>], eof: bool) -> (Vec<u8>, usize) {
    let mut result = Vec::new();
    let mut i = 0;
    while i < input.len() {
        if let Some(secret) = secrets.iter().find(|s| input[i..].starts_with(s)) {
            result.extend_from_slice(b"[REDACTED]");
            i += secret.len();
        } else if !eof && secrets.iter().any(|s| s.starts_with(&input[i..])) {
            break;
        } else {
            result.push(input[i]);
            i += 1;
        }
    }
    (result, i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn real_process_exit_preserves_status_and_flushes_redacted_log() {
        for code in [0, 7] {
            let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/process-exit-tests")
                .join(uuid::Uuid::new_v4().to_string());
            tokio::fs::create_dir_all(&directory).await.unwrap();
            #[cfg(windows)]
            let (executable, arguments) = (
                PathBuf::from("cmd.exe"),
                vec![
                    "/D".into(),
                    "/C".into(),
                    format!("echo secret-token & echo Error: fixture & exit /b {code}"),
                ],
            );
            #[cfg(not(windows))]
            let (executable, arguments) = (
                PathBuf::from("sh"),
                vec![
                    "-c".into(),
                    format!("printf 'secret-token\\nError: fixture\\n'; exit {code}"),
                ],
            );
            let log_path = directory.join(".launcher/logs/run.log");
            let process = spawn(LaunchPlan {
                executable,
                arguments,
                game_directory: directory.clone(),
                natives_directory: directory.join(".launcher/natives/run"),
                log_path: log_path.clone(),
                natives: vec![],
                secrets: vec!["secret-token".into()],
            })
            .await
            .unwrap();
            let exit = tokio::time::timeout(std::time::Duration::from_secs(10), process.wait())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(exit.code, Some(code));
            assert_eq!(exit.success, code == 0);
            assert!(exit.log_error.is_none());
            let text = tokio::fs::read_to_string(log_path).await.unwrap();
            assert!(text.contains("Error: fixture"));
            assert!(text.contains("[REDACTED]"));
            assert!(!text.contains("secret-token"));
        }
    }

    #[tokio::test]
    async fn logging_failure_does_not_hide_abnormal_exit() {
        #[cfg(windows)]
        let child = Command::new("cmd.exe")
            .args(["/D", "/C", "exit /b 9"])
            .creation_flags(0x08000000)
            .spawn()
            .unwrap();
        #[cfg(not(windows))]
        let child = Command::new("sh").args(["-c", "exit 9"]).spawn().unwrap();
        let process = GameProcess {
            child,
            log_path: "fixture.log".into(),
            pumps: vec![],
            logger: tokio::spawn(async { Err(std::io::Error::other("fixture log failure")) }),
        };
        let exit = process.wait().await.unwrap();
        assert_eq!(exit.code, Some(9));
        assert!(!exit.success);
        assert!(exit.log_error.unwrap().contains("fixture log failure"));
    }

    #[tokio::test]
    async fn redacts_secrets_across_chunks_without_waiting_for_newlines() {
        let (mut writer, reader) = tokio::io::duplex(64);
        let (sender, mut receiver) = mpsc::channel(8);
        let reader_task = tokio::spawn(pump(reader, sender, vec!["secret-token".into()]));
        writer.write_all(b"prefix secret-").await.unwrap();
        assert_eq!(receiver.recv().await.unwrap(), b"prefix ");
        writer.write_all(b"token suffix").await.unwrap();
        writer.shutdown().await.unwrap();
        reader_task.await.unwrap().unwrap();
        let mut result = Vec::new();
        while let Some(bytes) = receiver.recv().await {
            result.extend(bytes);
        }
        assert_eq!(result, b"[REDACTED] suffix");
    }
}
