use mc_core::{Instance, LoaderType, Task, TaskStatus};

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
        let (icon, installed, time, mods, java) = match instance.id.as_str() {
            "inst-fabric-121" => (0, true, "128 小时", "42 个", "Java 21"),
            "inst-forge-1192" => (1, false, "0 小时", "18 个", "Java 17"),
            "inst-modpack-1201" => (2, true, "215 小时", "164 个", "Java 17"),
            _ => (0, true, "36 小时", "0 个", "Java 21"),
        };
        Self {
            instance,
            loader,
            java: java.into(),
            icon,
            installed,
            play_time: time.into(),
            mods: mods.into(),
        }
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

pub const CHANGELOG: [(&str, &str, &[&str]); 3] = [
    (
        "v0.1.0",
        "2024-12-28",
        &[
            "全新界面与架构设计",
            "支持 AI 一键组包（API / MCP）",
            "优化中文本地化体验",
        ],
    ),
    (
        "v0.0.9",
        "2024-12-10",
        &["优化游戏下载速度", "改进 Mod 管理体验"],
    ),
    (
        "v0.0.8",
        "2024-11-25",
        &["修复整合包导入异常问题", "提升启动稳定性"],
    ),
];

pub const FEATURE_TITLES: [&str; 4] = [
    "MC 启动器 v0.1.0 正式发布",
    "RPG 冒险整合包",
    "探索樱花与山野",
    "创造你的方块世界",
];
pub const FEATURE_DESCRIPTIONS: [&str; 4] = [
    "全新界面，更强大的 Mod 管理体验，\n支持 AI 一键组包，带来更流畅的游戏体验。",
    "沉浸式魔幻世界，\n开启全新的冒险旅程。",
    "发现新的风景，\n让每一次出发都值得期待。",
    "从一块草地开始，\n探索、创造，更多可能。",
];
