use mc_core::{Instance, LoaderType, LocalClient, Task, TaskStatus};

#[derive(Debug, Clone)]
pub struct InstanceEntry {
    pub instance: Instance,
    pub loader: String,
    pub java: String,
    pub icon: usize,
    pub installed: bool,
    pub play_time: String,
    pub mods: String,
}

impl From<Instance> for InstanceEntry {
    fn from(instance: Instance) -> Self {
        let loader = instance
            .loader
            .as_ref()
            .map(|l| match l.loader_type {
                LoaderType::Fabric => "Fabric",
                LoaderType::Forge => "Forge",
                LoaderType::NeoForge => "NeoForge",
                LoaderType::Quilt => "Quilt",
                LoaderType::Vanilla => "Vanilla",
            })
            .unwrap_or("Vanilla")
            .to_owned();
        let icon = if matches!(
            instance.loader.as_ref().map(|l| &l.loader_type),
            Some(LoaderType::Forge | LoaderType::NeoForge)
        ) {
            1
        } else {
            0
        };
        Self {
            instance,
            loader,
            java: "Java 要求未知".into(),
            icon,
            installed: false,
            play_time: "未记录".into(),
            mods: "未统计".into(),
        }
    }
}

impl From<LocalClient> for InstanceEntry {
    fn from(client: LocalClient) -> Self {
        let mut entry = Self::from(client.instance);
        entry.installed = client.client_jar_present;
        entry.java = client
            .java_major
            .map(|v| format!("Java {v}"))
            .unwrap_or_else(|| "Java 要求未知".into());
        let mods = entry.instance.game_dir.join("mods");
        entry.mods = match std::fs::read_dir(&mods) {
            Ok(files) => {
                let count = files
                    .map(|file| {
                        file.and_then(|file| {
                            Ok(file.file_type()?.is_file()
                                && file
                                    .path()
                                    .extension()
                                    .is_some_and(|ext| ext.eq_ignore_ascii_case("jar")))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>();
                match count {
                    Ok(files) => format!("{} 个", files.into_iter().filter(|v| *v).count()),
                    Err(_) => "读取失败".into(),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => "0 个".into(),
            Err(_) => "读取失败".into(),
        };
        entry
    }
}

#[derive(Debug, Clone)]
pub struct QueueEntry {
    pub task: Task,
    pub title: String,
    pub detail: String,
    pub speed: String,
    pub sodium: bool,
    pub paused: bool,
}

impl From<Task> for QueueEntry {
    fn from(task: Task) -> Self {
        let parts: Vec<_> = task.progress.step_description.split('|').collect();
        let title = parts.first().unwrap_or(&"后台任务").to_string();
        let detail = parts.get(1).unwrap_or(&"等待中").to_string();
        let speed = parts.get(2).unwrap_or(&"").to_string();
        let sodium = parts.get(3) == Some(&"sodium");
        let paused = task.status == TaskStatus::Cancelled;
        Self {
            task,
            title,
            detail,
            speed,
            sodium,
            paused,
        }
    }
}

pub const CHANGELOG: [(&str, &str, &[&str]); 1] = [(
    "v0.1.0",
    "开发中",
    &[
        "扫描本地 Minecraft 实例与 Mod 数量",
        "Microsoft 账户登录、切换与凭据存储",
        "浏览版本清单并下载原版客户端与依赖",
        "保存下载源与并行下载数量",
    ],
)];

pub const FEATURE_TITLES: [&str; 4] = [
    "你的本地 Minecraft 游戏库",
    "使用 Microsoft 账户登录",
    "浏览官方游戏版本",
    "规划你的 AI 需求",
];
pub const FEATURE_DESCRIPTIONS: [&str; 4] = [
    "在游戏设置中选择实例集文件夹，\n扫描已安装的版本和加载器。",
    "点击侧栏底部管理账户，\n游戏启动时验证正版会话。",
    "在游戏下载页选择原版版本，\n按下载配置安装到当前实例集。",
    "在 AI 对话中咨询配置或排查问题，\n前往设置连接自己的模型服务。",
];
