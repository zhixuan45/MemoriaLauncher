use crate::models::InstanceEntry;
use mc_core::{GameLauncher, GameLayout, LaunchOptions, LocalClientService};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub game_root: PathBuf,
    pub java: PathBuf,
}

impl Default for Environment {
    fn default() -> Self {
        let root = std::env::var_os("APPDATA")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        Self {
            game_root: root.join(".minecraft"),
            java: "java".into(),
        }
    }
}

impl Environment {
    pub fn load() -> (Self, Option<String>) {
        let path = crate::accounts::data_dir().join("environment.json");
        let (mut config, error) = match std::fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<Self>(&bytes) {
                Ok(config) => (config, None),
                Err(e) => (Self::default(), Some(format!("环境配置读取失败：{e}"))),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Self::default(), None),
            Err(e) => (Self::default(), Some(format!("环境配置读取失败：{e}"))),
        };
        if let Some(root) = std::env::var_os("MC_LAUNCHER_GAME_DIR") {
            config.game_root = root.into();
        }
        if let Some(java) = std::env::var_os("MC_LAUNCHER_JAVA") {
            config.java = java.into();
        }
        (config, error)
    }

    pub async fn save(&self) -> Result<(), String> {
        let directory = crate::accounts::data_dir();
        tokio::fs::create_dir_all(&directory)
            .await
            .map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        let temporary = directory.join("environment.json.tmp");
        tokio::fs::write(&temporary, bytes)
            .await
            .map_err(|e| e.to_string())?;
        tokio::fs::rename(temporary, directory.join("environment.json"))
            .await
            .map_err(|e| e.to_string())
    }
}

#[derive(Debug, Clone)]
pub struct LocalData {
    pub instances: Vec<InstanceEntry>,
    pub notice: String,
}

pub async fn scan(root: PathBuf) -> Result<LocalData, String> {
    let layout = GameLayout::new(root).map_err(|e| e.to_string())?;
    let report = LocalClientService::new(layout.clone())
        .scan()
        .await
        .map_err(|e| e.to_string())?;
    let notice = format!(
        "游戏目录：{} · {} 个实例{}",
        layout.root().display(),
        report.clients.len(),
        if report.issues.is_empty() {
            String::new()
        } else {
            format!(
                " · {} 项读取失败：{}",
                report.issues.len(),
                report
                    .issues
                    .iter()
                    .map(|i| format!("{}：{}", i.path.display(), i.message))
                    .collect::<Vec<_>>()
                    .join("；")
            )
        }
    );
    let instances = tokio::task::spawn_blocking(move || {
        report
            .clients
            .into_iter()
            .map(InstanceEntry::from)
            .collect()
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(LocalData { instances, notice })
}

pub fn launch(
    environment: Environment,
    key: String,
    account_id: String,
    provider: crate::accounts::Provider,
) -> iced::Task<crate::app::Message> {
    use crate::app::Message;
    use iced::futures::SinkExt;
    iced::Task::run(
        iced::stream::channel(
            2,
            move |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
                let result = async {
                    let layout = GameLayout::new(environment.game_root)?;
                    let launcher = GameLauncher::new(layout, Arc::new(provider));
                    let options = LaunchOptions {
                        java: environment.java,
                        ..Default::default()
                    };
                    launcher
                        .prepare(&key, &account_id, options)
                        .await?
                        .spawn()
                        .await
                }
                .await;
                match result {
                    Ok(process) => {
                        let description = format!(
                            "游戏已启动（PID {}），日志：{}",
                            process.id().unwrap_or_default(),
                            process.log_path().display()
                        );
                        let _ = output.send(Message::GameStarted(description)).await;
                        let result = process
                            .wait()
                            .await
                            .map(|exit| {
                                format!(
                                    "游戏已退出（退出码 {:?}），日志：{}",
                                    exit.code,
                                    exit.log_path.display()
                                )
                            })
                            .map_err(|e| e.to_string());
                        let _ = output.send(Message::GameFinished(result)).await;
                    }
                    Err(e) => {
                        let _ = output.send(Message::GameFinished(Err(e.to_string()))).await;
                    }
                }
            },
        ),
        |message| message,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn scans_real_files_without_inserting_sample_instances() {
        let root =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/desktop-scan-fixture");
        let game = root.join("versions/test-version");
        tokio::fs::create_dir_all(game.join("mods")).await.unwrap();
        tokio::fs::write(game.join("test-version.json"), r#"{"id":"test-version","mainClass":"example.Main","javaVersion":{"majorVersion":21},"libraries":[]}"#).await.unwrap();
        tokio::fs::write(game.join("test-version.jar"), b"fixture")
            .await
            .unwrap();
        tokio::fs::write(game.join("mods/test.jar"), b"fixture")
            .await
            .unwrap();
        tokio::fs::write(game.join("mods/readme.txt"), b"not a mod")
            .await
            .unwrap();
        let data = scan(root.clone()).await.unwrap();
        assert_eq!(data.instances.len(), 1);
        assert_eq!(data.instances[0].instance.id, "versions/test-version");
        assert!(data.instances[0].installed);
        assert_eq!(data.instances[0].java, "Java 21");
        assert_eq!(data.instances[0].mods, "1 个");
        assert_eq!(data.instances[0].play_time, "未记录");
        tokio::fs::remove_file(game.join("test-version.jar"))
            .await
            .unwrap();
        assert!(!scan(root).await.unwrap().instances[0].installed);
    }

    #[tokio::test]
    async fn missing_game_directory_produces_an_empty_library() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/no-game-installed");
        assert!(scan(root).await.unwrap().instances.is_empty());
    }
}
