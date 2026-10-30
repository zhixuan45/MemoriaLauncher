use crate::models::InstanceEntry;
use mc_core::{GameLauncher, GameLayout, LaunchOptions, LocalClientService};
use serde::{Deserialize, Serialize};
use std::{path::PathBuf, sync::Arc};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Environment {
    pub game_root: PathBuf,
    pub java: PathBuf,
    #[serde(default)]
    pub game_roots: Vec<PathBuf>,
    #[serde(default = "default_min_memory")]
    pub min_memory_mb: u32,
    #[serde(default = "default_max_memory")]
    pub max_memory_mb: u32,
    #[serde(default)]
    pub memory_mode: crate::runtime_memory::MemoryMode,
    #[serde(default = "default_memory_percent")]
    pub memory_percent: u8,
    #[serde(default)]
    pub java_paths: Vec<PathBuf>,
}

fn default_min_memory() -> u32 {
    LaunchOptions::default().min_memory_mb
}
fn default_max_memory() -> u32 {
    LaunchOptions::default().max_memory_mb
}
fn default_memory_percent() -> u8 {
    25
}

impl Default for Environment {
    fn default() -> Self {
        let root = std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        let game_root = root.join(".minecraft");
        Self {
            game_roots: vec![game_root.clone()],
            game_root,
            java: "java".into(),
            min_memory_mb: default_min_memory(),
            max_memory_mb: default_max_memory(),
            memory_mode: crate::runtime_memory::MemoryMode::Automatic,
            memory_percent: default_memory_percent(),
            java_paths: Vec::new(),
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
        config.reconcile_roots();
        (config, error)
    }

    pub fn reconcile_roots(&mut self) {
        if !self
            .game_roots
            .iter()
            .any(|root| same_root(root, &self.game_root))
        {
            self.game_roots.insert(0, self.game_root.clone());
        }
        let mut roots: Vec<PathBuf> = Vec::new();
        for root in self.game_roots.drain(..) {
            if !roots.iter().any(|other| same_root(other, &root)) {
                roots.push(root);
            }
        }
        self.game_roots = roots;
    }

    pub fn launch_options(&self) -> LaunchOptions {
        LaunchOptions {
            java: self.java.clone(),
            min_memory_mb: self.min_memory_mb,
            max_memory_mb: self.max_memory_mb,
            ..Default::default()
        }
    }

    pub fn resolved_launch_options(
        &self,
        memory: crate::runtime_memory::MemorySnapshot,
        arch: &str,
    ) -> Result<LaunchOptions, String> {
        use crate::runtime_memory::{gib, MemoryMode};
        if self.memory_mode == MemoryMode::Custom && !(1..=90).contains(&self.memory_percent) {
            return Err("自定义内存比例应在 1%–90% 之间。".into());
        }
        let mut options = self.launch_options();
        options.max_memory_mb = memory
            .allocation(self.memory_mode, self.memory_percent, self.max_memory_mb)
            .ok_or("无法读取系统内存，请稍后重试。")?;
        let is_32_bit = matches!(arch, "x86" | "i386" | "i486" | "i586" | "i686");
        if is_32_bit && options.max_memory_mb > 1536 {
            if self.memory_mode == MemoryMode::Automatic {
                options.max_memory_mb = 1536;
            } else {
                return Err("当前选择的是 32 位 Java，游戏内存不能超过 1.5 GB；请选择 64 位 Java 或降低分配比例。".into());
            }
        }
        if self.memory_mode != MemoryMode::Fixed {
            if options.max_memory_mb as u64 > memory.available_mb {
                return Err(format!(
                    "当前可用内存 {}，无法分配 {}；请降低比例或关闭其他程序。",
                    gib(memory.available_mb),
                    gib(options.max_memory_mb as u64)
                ));
            }
            options.min_memory_mb = options.min_memory_mb.min(options.max_memory_mb).max(128);
        }
        Ok(options)
    }

    pub async fn save(&self) -> Result<(), String> {
        let directory = crate::accounts::data_dir();
        self.save_to(&directory).await
    }

    async fn save_to(&self, directory: &std::path::Path) -> Result<(), String> {
        tokio::fs::create_dir_all(directory)
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

pub fn normalize_root(path: &std::path::Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() {
        return Err("实例集文件夹路径不能为空。".into());
    }
    let path = std::path::absolute(path).map_err(|e| e.to_string())?;
    match std::fs::metadata(&path) {
        Ok(meta) if !meta.is_dir() => Err("请选择文件夹，而不是文件。".into()),
        Ok(_) => path.canonicalize().map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(path),
        Err(e) => Err(format!("无法访问实例集文件夹：{e}")),
    }
}

pub fn same_root(left: &std::path::Path, right: &std::path::Path) -> bool {
    let left = normalize_root(left).unwrap_or_else(|_| left.to_path_buf());
    let right = normalize_root(right).unwrap_or_else(|_| right.to_path_buf());
    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

/// Existing directories are imported without touching their contents.
pub async fn prepare_collection(path: PathBuf, create: bool) -> Result<(PathBuf, bool), String> {
    let path = normalize_root(&path)?;
    match tokio::fs::metadata(&path).await {
        Ok(_) => Ok((path, false)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && create => {
            for child in ["versions", "libraries", "assets"] {
                tokio::fs::create_dir_all(path.join(child))
                    .await
                    .map_err(|e| format!("创建实例集文件夹失败：{e}"))?;
            }
            Ok((normalize_root(&path)?, true))
        }
        Err(e) => Err(format!("导入失败，请选择已有的实例集文件夹：{e}")),
    }
}

#[derive(Debug, Clone)]
pub struct LocalData {
    pub instances: Vec<InstanceEntry>,
    pub notice: String,
}

#[derive(Debug, Clone)]
pub struct GameExit {
    pub instance_id: String,
    pub game_root: PathBuf,
    pub exit: mc_core::ProcessExit,
}

impl GameExit {
    pub async fn read_log(&self) -> Result<mc_core::InstanceLogContent, String> {
        let filename = self
            .exit
            .log_path
            .file_name()
            .ok_or("本次启动日志路径无效")?;
        mc_core::read_instance_log(
            GameLayout::new(&self.game_root).map_err(|error| error.to_string())?,
            self.instance_id.clone(),
            format!(".launcher/logs/{}", filename.to_string_lossy()),
            mc_core::MAX_LOG_BYTES,
        )
        .await
        .map_err(|error| error.to_string())
    }

    pub fn description(&self) -> String {
        let code = self
            .exit
            .code
            .map(|code| code.to_string())
            .unwrap_or_else(|| "无退出码（信号终止）".into());
        let mut status = format!(
            "实例 {} {}（退出码 {}），日志：{}",
            self.instance_id,
            if self.exit.success {
                "正常退出"
            } else {
                "异常退出"
            },
            code,
            self.exit.log_path.display()
        );
        if let Some(error) = &self.exit.log_error {
            status.push_str(&format!("；日志记录异常：{error}"));
        }
        status
    }
}

pub async fn scan(root: PathBuf) -> Result<LocalData, String> {
    let layout = GameLayout::new(&root).map_err(|e| e.to_string())?;
    let report = LocalClientService::new(layout.clone())
        .scan()
        .await
        .map_err(|e| e.to_string())?;
    let mut notice = format!(
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
    let (instances, recovery) = tokio::task::spawn_blocking(move || {
        let mut recovery=vec![];
        let instances=report
            .clients
            .into_iter()
            .map(|client| {
                match mc_core::service::file_transaction::recover(&root,&client.instance.id) {
                    Ok(report) if !report.rolled_back.is_empty()=>recovery.push(format!("{} 已恢复 {} 个中断事务",client.instance.id,report.rolled_back.len())),
                    Err(error)=>recovery.push(format!("{} 恢复未完成：{error}",client.instance.id)),
                    _=>{}
                }
                let config = mc_core::service::desktop_instances::load_settings(&root, &client.instance.id);
                let mut entry = InstanceEntry::from(client);
                if let Ok(config) = config {
                    if !config.name.is_empty() { entry.instance.name = config.name; }
                }
                entry
            })
            .collect();
        (instances,recovery)
    })
    .await
    .map_err(|e| e.to_string())?;
    if !recovery.is_empty(){notice.push_str(&format!("；{}",recovery.join("；")));}
    Ok(LocalData { instances, notice })
}

pub fn launch(
    environment: Environment,
    downloads: mc_core::DownloadSettings,
    key: String,
    account_id: String,
    provider: crate::accounts::Provider,
) -> iced::Task<crate::app::Message> {
    use crate::app::Message;
    use iced::futures::SinkExt;
    let game_root = environment.game_root.clone();
    iced::Task::run(
        iced::stream::channel(
            2,
            move |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
                let result = async {
                    let config = mc_core::service::desktop_instances::load_settings(&environment.game_root, &key)
                        .map_err(mc_core::LauncherError::InvalidMetadata)?;
                    let layout = GameLayout::new(&environment.game_root)?;
                    let required = LocalClientService::new(layout.clone()).required_java_major(key.clone()).await?;
                    let java = crate::java_environment::select_for_launch(required, config.java.clone(), &environment)
                        .await.map_err(mc_core::LauncherError::JavaUnavailable)?;
                    let memory =
                        tokio::task::spawn_blocking(crate::runtime_memory::MemorySnapshot::read)
                            .await
                            .map_err(|e| mc_core::LauncherError::Internal(e.to_string()))?;
                    let mut effective = environment.clone();
                    effective.java = java.executable.clone();
                    if let Some(value) = config.memory_mb {
                        effective.memory_mode = crate::runtime_memory::MemoryMode::Fixed;
                        effective.max_memory_mb = value;
                        effective.min_memory_mb = effective.min_memory_mb.min(value);
                    }
                    let mut options = effective
                        .resolved_launch_options(memory, &java.arch)
                        .map_err(mc_core::LauncherError::JavaUnavailable)?;
                    config.apply(&mut options).map_err(mc_core::LauncherError::InvalidMetadata)?;
                    options.java = java.executable.clone();
                    let launcher = GameLauncher::new(layout, Arc::new(provider));
                    launcher.repair_logging(&key, &mc_core::DownloadClient::new(downloads)?).await?;
                    let process = launcher
                        .prepare(&key, &account_id, options)
                        .await?
                        .spawn()
                        .await?;
                    Ok::<_, mc_core::LauncherError>((process, java))
                }
                .await;
                match result {
                    Ok((process, java)) => {
                        let description = format!(
                            "游戏已启动（PID {}）· Java {}（{}），日志：{}",
                            process.id().unwrap_or_default(),
                            java.major_version,
                            java.executable.display(),
                            process.log_path().display()
                        );
                        let _ = output.send(Message::GameStarted(description)).await;
                        let result = process
                            .wait()
                            .await
                            .map(|exit| GameExit {
                                instance_id: key,
                                game_root,
                                exit,
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

    fn fixture() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/game-settings-tests")
            .join(uuid::Uuid::new_v4().to_string())
    }

    #[tokio::test]
    async fn legacy_environment_migrates_and_persists_multiple_collections_and_memory() {
        let root = fixture();
        let legacy = serde_json::json!({ "game_root": root.join("old"), "java": "custom-java" });
        let mut config: Environment = serde_json::from_value(legacy).unwrap();
        config.reconcile_roots();
        assert_eq!(config.game_roots, vec![root.join("old")]);
        assert_eq!(config.min_memory_mb, 512);
        assert_eq!(config.max_memory_mb, 4096);
        assert_eq!(config.memory_mode, crate::runtime_memory::MemoryMode::Fixed);
        config.game_roots.push(root.join("second"));
        config.game_root = root.join("second");
        config.min_memory_mb = 1024;
        config.max_memory_mb = 8192;
        config.save_to(&root).await.unwrap();
        // Replacing an existing file must work on Windows as well.
        config.save_to(&root).await.unwrap();
        let reloaded: Environment = serde_json::from_slice(
            &tokio::fs::read(root.join("environment.json"))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(reloaded.game_roots.len(), 2);
        assert_eq!(reloaded.game_root, root.join("second"));
        let options = reloaded.launch_options();
        assert_eq!(options.java, PathBuf::from("custom-java"));
        assert_eq!(options.min_memory_mb, 1024);
        assert_eq!(options.max_memory_mb, 8192);
    }

    #[tokio::test]
    async fn java_imports_and_memory_mode_survive_restart_and_control_launch_options() {
        use crate::runtime_memory::{MemoryMode, MemorySnapshot};
        let root = fixture();
        let mut config = Environment::default();
        config.java = root.join("jdk-21/bin/java.exe");
        config.java_paths = vec![config.java.clone(), root.join("jdk-17/bin/java.exe")];
        config.memory_mode = MemoryMode::Custom;
        config.memory_percent = 30;
        config.save_to(&root).await.unwrap();
        let mut reloaded: Environment = serde_json::from_slice(
            &tokio::fs::read(root.join("environment.json"))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(reloaded.java_paths, config.java_paths);
        assert_eq!(reloaded.memory_mode, MemoryMode::Custom);
        let roomy = MemorySnapshot {
            total_mb: 32768,
            available_mb: 24576,
        };
        let busy = MemorySnapshot {
            total_mb: 32768,
            available_mb: 1024,
        };
        let options = reloaded.resolved_launch_options(roomy, "amd64").unwrap();
        assert_eq!(options.java, config.java);
        assert_eq!(options.max_memory_mb, 9830);
        assert!(reloaded.resolved_launch_options(busy, "amd64").is_err());
        assert!(reloaded.resolved_launch_options(roomy, "x86").is_err());
        reloaded.memory_mode = MemoryMode::Automatic;
        assert_eq!(
            reloaded
                .resolved_launch_options(roomy, "amd64")
                .unwrap()
                .max_memory_mb,
            8192
        );
        assert_eq!(
            reloaded
                .resolved_launch_options(busy, "amd64")
                .unwrap()
                .max_memory_mb,
            512
        );
        assert_eq!(
            reloaded
                .resolved_launch_options(roomy, "x86")
                .unwrap()
                .max_memory_mb,
            1536
        );
    }

    #[tokio::test]
    async fn creates_new_collections_but_imports_existing_directories_without_writing() {
        let root = fixture();
        let path = root.join(".minecraft");
        assert!(prepare_collection(path.clone(), false).await.is_err());
        assert!(!path.exists());
        let (created, is_new) = prepare_collection(path.clone(), true).await.unwrap();
        assert!(is_new);
        for child in ["versions", "libraries", "assets"] {
            assert!(created.join(child).is_dir());
        }
        let existing = root.join("existing");
        tokio::fs::create_dir_all(&existing).await.unwrap();
        tokio::fs::write(existing.join("keep.txt"), b"keep")
            .await
            .unwrap();
        for create in [true, false] {
            let (imported, is_new) = prepare_collection(existing.clone(), create).await.unwrap();
            assert!(!is_new);
            assert!(!imported.join("versions").exists());
            assert_eq!(
                tokio::fs::read(imported.join("keep.txt")).await.unwrap(),
                b"keep"
            );
        }
        assert!(prepare_collection(existing.join("keep.txt"), true)
            .await
            .is_err());
        assert!(same_root(&created, &path.join(".")));
        if cfg!(windows) {
            assert!(same_root(
                &created,
                &PathBuf::from(path.to_string_lossy().to_uppercase())
            ));
        }
    }

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
