use async_trait::async_trait;
use chrono::Utc;

use crate::error::LauncherResult;
use crate::model::{GameVersion, GameVersionType, ModProject};

/// 游戏与内容目录提供者接口
#[async_trait]
pub trait CatalogProvider: Send + Sync {
    /// 获取支持的官方 Minecraft 版本清单
    async fn list_game_versions(&self) -> LauncherResult<Vec<GameVersion>>;

    /// 检索第三方 Mod 项目（如 Modrinth）
    async fn search_mods(&self, query: &str, mc_version: Option<&str>) -> LauncherResult<Vec<ModProject>>;
}

/// 阶段 0 模拟提供者，用于测试与离线环境
#[derive(Default, Clone)]
pub struct MockCatalogProvider;

impl MockCatalogProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CatalogProvider for MockCatalogProvider {
    async fn list_game_versions(&self) -> LauncherResult<Vec<GameVersion>> {
        Ok(vec![
            GameVersion {
                id: "1.21.1".to_string(),
                version_type: GameVersionType::Release,
                release_time: Utc::now(),
                url: "https://piston-meta.mojang.com/v1/packages/sample-1.21.1.json".to_string(),
            },
            GameVersion {
                id: "1.20.1".to_string(),
                version_type: GameVersionType::Release,
                release_time: Utc::now(),
                url: "https://piston-meta.mojang.com/v1/packages/sample-1.20.1.json".to_string(),
            },
        ])
    }

    async fn search_mods(&self, query: &str, _mc_version: Option<&str>) -> LauncherResult<Vec<ModProject>> {
        let sample = vec![
            ModProject {
                id: "fabric-api".to_string(),
                slug: "fabric-api".to_string(),
                title: "Fabric API".to_string(),
                description: "Essential hooks and compatibility layers for Fabric mods.".to_string(),
                loaders: vec!["fabric".to_string()],
                game_versions: vec!["1.21.1".to_string(), "1.20.1".to_string()],
                icon_url: None,
            },
            ModProject {
                id: "sodium".to_string(),
                slug: "sodium".to_string(),
                title: "Sodium".to_string(),
                description: "Modern rendering engine for Minecraft.".to_string(),
                loaders: vec!["fabric".to_string(), "neoforge".to_string()],
                game_versions: vec!["1.21.1".to_string(), "1.20.1".to_string()],
                icon_url: None,
            },
        ];

        let filtered = sample
            .into_iter()
            .filter(|p| {
                if query.is_empty() {
                    true
                } else {
                    p.title.to_lowercase().contains(&query.to_lowercase())
                        || p.description.to_lowercase().contains(&query.to_lowercase())
                }
            })
            .collect();

        Ok(filtered)
    }
}
