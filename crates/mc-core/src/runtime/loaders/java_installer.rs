use super::*;
use std::{process::Stdio, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt};

async fn drain(mut stream: impl AsyncRead + Unpin) -> std::io::Result<Vec<u8>> {
    let mut output = vec![];
    let mut buffer = [0; 8192];
    loop {
        let count = stream.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        let remaining = (512 * 1024usize).saturating_sub(output.len());
        output.extend_from_slice(&buffer[..count.min(remaining)]);
    }
    Ok(output)
}

pub(super) async fn run(java: &Path, installer: &Path, work: &Path) -> LauncherResult<()> {
    let mut command = tokio::process::Command::new(java);
    command
        .arg("-jar")
        .arg(installer)
        .arg("--installClient")
        .arg(work)
        .current_dir(work)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let result = tokio::time::timeout(Duration::from_secs(600), async {
        let (status, out, err) = tokio::join!(child.wait(), drain(stdout), drain(stderr));
        Ok::<_, std::io::Error>((status?, out?, err?))
    })
    .await;
    match result {
        Ok(Ok((status, out, err))) => {
            let text = format!(
                "{}\n{}",
                String::from_utf8_lossy(&out),
                String::from_utf8_lossy(&err)
            );
            let (safe, _) = super::super::logs::redact(&text);
            std::fs::write(work.parent().unwrap().join("installer.log"), safe)?;
            if !status.success() {
                return Err(invalid(format!("官方安装器退出失败：{status}")));
            }
            Ok(())
        }
        Ok(Err(error)) => Err(error.into()),
        Err(_) => {
            let _ = child.kill().await;
            Err(invalid("官方安装器运行超过 10 分钟，已终止主进程。"))
        }
    }
}
