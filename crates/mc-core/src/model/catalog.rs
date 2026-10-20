use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// 游戏版本类别
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GameVersionType {
    Release,
    Snapshot,
}

/// 官方游戏版本条目
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameVersion {
    pub id: String,
    pub version_type: GameVersionType,
    pub release_time: DateTime<Utc>,
    pub url: String,
}

/// 模组项目摘要（对应 Modrinth 等源）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModProject {
    pub id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub loaders: Vec<String>,
    pub game_versions: Vec<String>,
    pub icon_url: Option<String>,
}
