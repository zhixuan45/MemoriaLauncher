use crate::{LauncherError, LauncherResult};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct JavaRuntime {
    pub executable: PathBuf,
    pub major_version: u32,
    pub version: String,
    pub os_version: String,
    pub arch: String,
}

impl JavaRuntime {
    /// 使用 java 而非 javaw 读取版本信息；游戏输出通过管道捕获。
    pub async fn inspect(executable: impl AsRef<Path>) -> LauncherResult<Self> {
        let executable = executable.as_ref().to_path_buf();
        let mut command = Command::new(&executable);
        command
            .args(["-XshowSettings:properties", "-version"])
            .kill_on_drop(true);
        #[cfg(windows)]
        command.creation_flags(0x08000000);
        let output = tokio::time::timeout(Duration::from_secs(10), command.output())
            .await
            .map_err(|_| LauncherError::JavaUnavailable("Java 检测超时".into()))?
            .map_err(|e| LauncherError::JavaUnavailable(e.to_string()))?;
        if !output.status.success() {
            return Err(LauncherError::JavaUnavailable(
                "java -version 执行失败".into(),
            ));
        }
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
        let property = |name: &str| {
            text.lines()
                .filter_map(|l| l.trim().split_once('='))
                .find(|(k, _)| k.trim() == name)
                .map(|(_, v)| v.trim().to_string())
        };
        let version = property("java.version")
            .or_else(|| text.split('"').nth(1).map(str::to_string))
            .ok_or_else(|| LauncherError::JavaUnavailable("无法解析 Java 版本".into()))?;
        let numeric = version.strip_prefix("1.").unwrap_or(&version);
        let major_version = numeric
            .split(['.', '-', '_', '+'])
            .next()
            .unwrap_or_default()
            .parse()
            .map_err(|_| LauncherError::JavaUnavailable(format!("无法解析 Java 版本 {version}")))?;
        Ok(Self {
            executable,
            major_version,
            version,
            os_version: property("os.version").unwrap_or_default(),
            arch: property("os.arch").unwrap_or_else(|| std::env::consts::ARCH.into()),
        })
    }
}
