use async_trait::async_trait;
use chrono::{TimeZone, Utc};

use crate::error::LauncherResult;
use crate::model::{GameVersion, GameVersionType, ModProject};

/// 游戏与内容目录提供者接口（对接 Mojang Piston Meta / Modrinth，当前由 MockCatalogProvider 提供模拟数据）
#[async_trait]
pub trait CatalogProvider: Send + Sync {
    /// 获取支持的官方 Minecraft 版本清单
    async fn list_game_versions(&self) -> LauncherResult<Vec<GameVersion>>;

    /// 根据版本 ID 获取单个版本的完整元数据详情
    async fn get_game_version(&self, version_id: &str) -> LauncherResult<Option<GameVersion>> {
        let list = self.list_game_versions().await?;
        Ok(list.into_iter().find(|v| v.id == version_id))
    }

    /// 检索第三方 Mod 项目（如 Modrinth）
    async fn search_mods(
        &self,
        query: &str,
        mc_version: Option<&str>,
    ) -> LauncherResult<Vec<ModProject>>;
}

/// Official catalogue. Never substitutes example data when the request fails.
pub struct MojangCatalogProvider;

impl MojangCatalogProvider {
    pub async fn list_with_client(
        client: &crate::DownloadClient,
    ) -> LauncherResult<Vec<GameVersion>> {
        let manifest = client.json(super::downloads::VERSION_MANIFEST).await?;
        Ok(manifest_versions(manifest))
    }
}

#[derive(serde::Deserialize)]
struct Manifest {
    versions: Vec<ManifestVersion>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestVersion {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    release_time: chrono::DateTime<Utc>,
    url: String,
}

fn manifest_versions(manifest: Manifest) -> Vec<GameVersion> {
    manifest
        .versions
        .into_iter()
        .filter_map(|entry| {
            let version_type = match entry.kind.as_str() {
                "release" => GameVersionType::Release,
                "snapshot" => GameVersionType::Snapshot,
                _ => return None,
            };
            Some(GameVersion {
                title: format!("Minecraft {}", entry.id),
                summary: format!("Mojang 官方版本清单：{}（{}）", entry.id, entry.kind),
                id: entry.id,
                version_type,
                release_date: entry.release_time.format("%Y-%m-%d").to_string(),
                release_time: entry.release_time,
                java_version: "Java 要求待查询".into(),
                supported_loaders: vec!["原版".into()],
                highlights: vec![],
                version_files: vec![format!("官方版本元数据：{}", entry.url)],
                system_requirements: vec!["官方清单未提供配置要求，请查阅该版本元数据。".into()],
                related_resources: vec![],
                changelog_url: String::new(),
                url: entry.url,
            })
        })
        .collect()
}

#[async_trait]
impl CatalogProvider for MojangCatalogProvider {
    async fn list_game_versions(&self) -> LauncherResult<Vec<GameVersion>> {
        let client = crate::DownloadClient::new(crate::DownloadSettings::default())?;
        Self::list_with_client(&client).await
    }

    async fn search_mods(&self, _: &str, _: Option<&str>) -> LauncherResult<Vec<ModProject>> {
        Err(crate::LauncherError::Unsupported("Mod 目录尚未接入".into()))
    }
}

#[cfg(test)]
mod official_tests {
    use super::*;

    #[test]
    fn official_manifest_preserves_source_metadata_without_inventing_loader_support() {
        let manifest = serde_json::from_str(r#"{"versions":[{"id":"test-release","type":"release","releaseTime":"2026-10-20T12:00:00Z","url":"https://example.test/release.json"},{"id":"test-snapshot","type":"snapshot","releaseTime":"2026-10-21T12:00:00Z","url":"https://example.test/snapshot.json"},{"id":"classic","type":"old_alpha","releaseTime":"2010-01-01T00:00:00Z","url":"https://example.test/old.json"}]}"#).unwrap();
        let versions = manifest_versions(manifest);
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].id, "test-release");
        assert_eq!(versions[0].release_date, "2026-10-20");
        assert_eq!(versions[0].url, "https://example.test/release.json");
        assert_eq!(versions[0].supported_loaders, ["原版"]);
        assert!(versions[0].highlights.is_empty());
        assert_eq!(versions[1].version_type, GameVersionType::Snapshot);
    }

    #[tokio::test]
    #[ignore = "Requires live access to Mojang"]
    async fn official_catalog_is_reachable() {
        let versions = MojangCatalogProvider.list_game_versions().await.unwrap();
        assert!(!versions.is_empty());
        assert!(versions.iter().all(|v| v.url.starts_with("https://")));
    }
}

/// 模拟版本与模组目录提供者（提供完整结构化模拟版本数据，后续可直接替换为 MojangPistonCatalogProvider）
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
                id: "1.21".to_string(),
                title: "Minecraft 1.21".to_string(),
                version_type: GameVersionType::Release,
                release_time: Utc.with_ymd_and_hms(2024, 6, 13, 12, 0, 0).unwrap(),
                release_date: "2024-06-13".to_string(),
                java_version: "Java 21".to_string(),
                supported_loaders: vec![
                    "原版".to_string(),
                    "Fabric".to_string(),
                    "Forge".to_string(),
                ],
                summary: "Minecraft 1.21（正式版）带来了全新的冒险内容、建筑方块、生物与机制，在探索与创造方面提供了更多可能性。".to_string(),
                highlights: vec![
                    "新增试炼密室（Trial Chambers）".to_string(),
                    "新增铜傀儡（Copper Golem）".to_string(),
                    "新的工艺与装饰方块".to_string(),
                    "优化了部分游戏机制与性能".to_string(),
                    "更多生物与环境改进".to_string(),
                ],
                version_files: vec![
                    "client.jar · 26.4 MB (SHA1: 4f8a9c...e21b)".to_string(),
                    "assetIndex: 17.json · 442.8 KB (共 3,892 个资源)".to_string(),
                    "libraries · 84 个依赖包 (LWJGL 3.3.3)".to_string(),
                    "client_mappings.txt · 官方混淆映射表 (8.9 MB)".to_string(),
                    "logging: client-1.12.xml · Log4j2 安全配置".to_string(),
                ],
                system_requirements: vec![
                    "运行环境：必须使用 Java 21 (64-bit OpenJDK / Temurin)".to_string(),
                    "推荐内存：原版 2048 MB - 4096 MB / 模组包 6144 MB+".to_string(),
                    "显卡支持：OpenGL 3.2 Core Profile 及以上".to_string(),
                    "磁盘空间：核心文件约 680 MB，完整资源约 1.4 GB".to_string(),
                    "操作系统：Windows 10/11 64-bit, macOS 12+, Linux x64".to_string(),
                ],
                related_resources: vec![
                    "Fabric Loader 0.16.9 (兼容 1.21 正式版)".to_string(),
                    "Fabric API 0.102.0+1.21 (必备前置模组)".to_string(),
                    "Sodium 0.5.11+mc1.21 (高性能帧率优化引擎)".to_string(),
                    "Iris Shaders 1.7.5+mc1.21 (光影加载器)".to_string(),
                    "Complementary Reimagined r5.2 (推荐光影包)".to_string(),
                ],
                changelog_url: "https://zh.minecraft.wiki/w/Java%E7%89%881.21".to_string(),
                url: "https://piston-meta.mojang.com/v1/packages/1.21.json".to_string(),
            },
            GameVersion {
                id: "1.20.6".to_string(),
                title: "Minecraft 1.20.6".to_string(),
                version_type: GameVersionType::Release,
                release_time: Utc.with_ymd_and_hms(2024, 4, 29, 12, 0, 0).unwrap(),
                release_date: "2024-04-29".to_string(),
                java_version: "Java 17".to_string(),
                supported_loaders: vec![
                    "原版".to_string(),
                    "Fabric".to_string(),
                    "NeoForge".to_string(),
                ],
                summary: "Minecraft 1.20.6 修复了狼铠与犰狳机制的多项问题，重构了物品组件存储格式，并进一步提升了数据包与服务器稳定性。".to_string(),
                highlights: vec![
                    "新增犰狳生物与狼铠染色保护机制".to_string(),
                    "重构物品数据组件（Item Components）替代旧版 NBT".to_string(),
                    "修复村民交易与实体同步异常漏洞".to_string(),
                    "优化区块流式加载与网络数据包压缩".to_string(),
                    "增强服务端网络协议校验安全性".to_string(),
                ],
                version_files: vec![
                    "client.jar · 25.8 MB (SHA1: 9b2d1f...71a0)".to_string(),
                    "assetIndex: 16.json · 436.1 KB (共 3,810 个资源)".to_string(),
                    "libraries · 82 个依赖包 (LWJGL 3.3.2)".to_string(),
                    "client_mappings.txt · 官方混淆映射表 (8.7 MB)".to_string(),
                    "logging: client-1.12.xml · Log4j2 安全配置".to_string(),
                ],
                system_requirements: vec![
                    "运行环境：推荐使用 Java 17 或 Java 21 (64-bit)".to_string(),
                    "推荐内存：原版 2048 MB / 模组环境 4096 MB+".to_string(),
                    "显卡支持：OpenGL 3.2 Core Profile 及以上".to_string(),
                    "磁盘空间：核心与资源文件共约 1.3 GB".to_string(),
                    "网络环境：首次下载需验证 Mojang Piston 资源清单".to_string(),
                ],
                related_resources: vec![
                    "Fabric Loader 0.15.11 (支持 1.20.6)".to_string(),
                    "NeoForge 20.6.119-beta (推荐现代模组加载器)".to_string(),
                    "Sodium 0.5.8+mc1.20.6 (帧率优化模组)".to_string(),
                    "Lithium 0.12.3 (服务端与物理逻辑优化)".to_string(),
                    "ModMenu 10.0.0 (模组配置菜单)".to_string(),
                ],
                changelog_url: "https://zh.minecraft.wiki/w/Java%E7%89%881.20.6".to_string(),
                url: "https://piston-meta.mojang.com/v1/packages/1.20.6.json".to_string(),
            },
            GameVersion {
                id: "1.20.4".to_string(),
                title: "Minecraft 1.20.4".to_string(),
                version_type: GameVersionType::Release,
                release_time: Utc.with_ymd_and_hms(2023, 12, 7, 12, 0, 0).unwrap(),
                release_date: "2023-12-07".to_string(),
                java_version: "Java 17".to_string(),
                supported_loaders: vec![
                    "原版".to_string(),
                    "Fabric".to_string(),
                    "Forge".to_string(),
                ],
                summary: "Minecraft 1.20.4（足迹与故事更新稳定版）拥有极高的模组生态兼容性，是 1.20 世代主流模组与大型整合包的首选基础版本。".to_string(),
                highlights: vec![
                    "新增樱花树林（Cherry Grove）生物群系".to_string(),
                    "新增考古系统、可疑的沙子与嗅探兽（Sniffer）".to_string(),
                    "新增竹木套装、雕纹书架与悬挂式告示牌".to_string(),
                    "支持 16 种盔甲纹饰模板与材质锻造".to_string(),
                    "修复饰品罐被投射物破坏时物品丢失漏洞".to_string(),
                ],
                version_files: vec![
                    "client.jar · 24.9 MB (SHA1: 5e1c3a...80f2)".to_string(),
                    "assetIndex: 12.json · 418.4 KB (共 3,695 个资源)".to_string(),
                    "libraries · 79 个依赖包 (LWJGL 3.3.2)".to_string(),
                    "client_mappings.txt · 官方混淆映射表 (8.4 MB)".to_string(),
                    "logging: client-1.12.xml · 标准日志配置".to_string(),
                ],
                system_requirements: vec![
                    "运行环境：Java 17 (64-bit OpenJDK 17.0.8+)".to_string(),
                    "推荐内存：原版 2048 MB / 大型整合包 6144 MB+".to_string(),
                    "显卡支持：OpenGL 3.2 及以上（支持 Iris 光影管线）".to_string(),
                    "磁盘空间：约 1.2 GB 可用 SSD 空间".to_string(),
                    "处理器：4 核 2.8 GHz 及以上处理器".to_string(),
                ],
                related_resources: vec![
                    "Fabric API 0.96.11+1.20.4 (核心 API 库)".to_string(),
                    "Forge 49.0.48 (经典模组加载器)".to_string(),
                    "Sodium 0.5.8+mc1.20.4 (渲染优化)".to_string(),
                    "JEI 17.3.0.49 (物品管理器)".to_string(),
                    "AppleSkin 2.5.1 (饱食度显示)".to_string(),
                ],
                changelog_url: "https://zh.minecraft.wiki/w/Java%E7%89%881.20.4".to_string(),
                url: "https://piston-meta.mojang.com/v1/packages/1.20.4.json".to_string(),
            },
            GameVersion {
                id: "1.19.4".to_string(),
                title: "Minecraft 1.19.4".to_string(),
                version_type: GameVersionType::Release,
                release_time: Utc.with_ymd_and_hms(2023, 3, 14, 12, 0, 0).unwrap(),
                release_date: "2023-03-14".to_string(),
                java_version: "Java 17".to_string(),
                supported_loaders: vec![
                    "原版".to_string(),
                    "Fabric".to_string(),
                    "Forge".to_string(),
                ],
                summary: "Minecraft 1.19.4（荒野更新最终修订版）改进了马匹繁殖机制、药水视觉颜色与命令系统，并正式引入展示实体（Display Entities）。".to_string(),
                highlights: vec![
                    "新增 /ride 与 /damage 原生指令支持".to_string(),
                    "引入文本、方块与物品展示实体（Display Entities）".to_string(),
                    "优化马匹与驴繁殖属性继承算法".to_string(),
                    "重构创造模式物品栏分类与搜索体验".to_string(),
                    "新增高对比度内置无障碍资源包".to_string(),
                ],
                version_files: vec![
                    "client.jar · 23.2 MB (SHA1: 7a9f2d...11c4)".to_string(),
                    "assetIndex: 3.json · 392.0 KB (共 3,520 个资源)".to_string(),
                    "libraries · 75 个依赖包 (LWJGL 3.3.1)".to_string(),
                    "client_mappings.txt · 官方混淆映射表 (7.9 MB)".to_string(),
                    "logging: client-1.12.xml · 标准日志配置".to_string(),
                ],
                system_requirements: vec![
                    "运行环境：Java 17 (64-bit)".to_string(),
                    "推荐内存：2048 MB - 4096 MB".to_string(),
                    "显卡支持：OpenGL 3.2 及以上".to_string(),
                    "磁盘空间：约 1.1 GB 可用空间".to_string(),
                    "操作系统：Windows / macOS / Linux 64 位".to_string(),
                ],
                related_resources: vec![
                    "Fabric API 0.87.2+1.19.4".to_string(),
                    "Forge 45.2.0 (1.19.4 稳定版)".to_string(),
                    "Sodium 0.4.10+build.24".to_string(),
                    "Iris Shaders 1.6.4".to_string(),
                    "Xaero's Minimap 23.9 (小地图模组)".to_string(),
                ],
                changelog_url: "https://zh.minecraft.wiki/w/Java%E7%89%881.19.4".to_string(),
                url: "https://piston-meta.mojang.com/v1/packages/1.19.4.json".to_string(),
            },
            GameVersion {
                id: "1.18.2".to_string(),
                title: "Minecraft 1.18.2".to_string(),
                version_type: GameVersionType::Release,
                release_time: Utc.with_ymd_and_hms(2022, 2, 28, 12, 0, 0).unwrap(),
                release_date: "2022-02-28".to_string(),
                java_version: "Java 17".to_string(),
                supported_loaders: vec![
                    "原版".to_string(),
                    "Fabric".to_string(),
                    "Forge".to_string(),
                ],
                summary: "Minecraft 1.18.2（洞穴与山崖第二部分稳定版）全面重构了主世界高度与 3D 立体地形生成，是机械动力与现代科技整合包的黄金版本。".to_string(),
                highlights: vec![
                    "主世界建筑高度扩展至 -64 至 320 层".to_string(),
                    "新增繁茂洞穴、溶洞与巨型含水层生成".to_string(),
                    "新增 /placefeature 指令与自定义结构数据包".to_string(),
                    "采用全新高度矿脉分布规律".to_string(),
                    "优化跨维度区块异步生成与加载性能".to_string(),
                ],
                version_files: vec![
                    "client.jar · 20.6 MB (SHA1: 2e9a8b...43d9)".to_string(),
                    "assetIndex: 1.18.json · 360.5 KB (共 3,280 个资源)".to_string(),
                    "libraries · 68 个依赖包 (LWJGL 3.2.2)".to_string(),
                    "client_mappings.txt · 官方混淆映射表 (7.1 MB)".to_string(),
                    "logging: client-1.12.xml · 标准日志配置".to_string(),
                ],
                system_requirements: vec![
                    "运行环境：Java 17 (64-bit)".to_string(),
                    "推荐内存：原版 2048 MB / 科技整合包 6144 MB+".to_string(),
                    "显卡支持：OpenGL 3.2 及以上".to_string(),
                    "磁盘空间：约 1.0 GB 可用空间".to_string(),
                    "处理器：支持多线程区块生成的四核以上 CPU".to_string(),
                ],
                related_resources: vec![
                    "Forge 40.2.21 (1.18.2 推荐稳定版)".to_string(),
                    "Create (机械动力) 0.5.1.f".to_string(),
                    "Fabric API 0.77.0+1.18.2".to_string(),
                    "Rubidium / Embeddium (Forge 帧率优化)".to_string(),
                    "Oculus (Forge 光影支持模组)".to_string(),
                ],
                changelog_url: "https://zh.minecraft.wiki/w/Java%E7%89%881.18.2".to_string(),
                url: "https://piston-meta.mojang.com/v1/packages/1.18.2.json".to_string(),
            },
            GameVersion {
                id: "1.16.5".to_string(),
                title: "Minecraft 1.16.5".to_string(),
                version_type: GameVersionType::Release,
                release_time: Utc.with_ymd_and_hms(2021, 1, 15, 12, 0, 0).unwrap(),
                release_date: "2021-01-15".to_string(),
                java_version: "Java 8".to_string(),
                supported_loaders: vec![
                    "原版".to_string(),
                    "Fabric".to_string(),
                    "Forge".to_string(),
                ],
                summary: "Minecraft 1.16.5（下界更新最终稳定版）全面重塑了下界生态，引入下界合金装备体系，拥有极其成熟庞大的 Forge 与 Fabric 模组库。".to_string(),
                highlights: vec![
                    "新增绯红森林、诡异森林、灵魂沙峡谷与玄武岩三角洲".to_string(),
                    "引入远古残骸与下界合金（Netherite）顶级装备".to_string(),
                    "新增猪灵以物易物机制与堡垒遗迹结构".to_string(),
                    "新增重生锚、标靶方块与哭泣的黑曜石".to_string(),
                    "修复两项关键服务端崩溃与区块保存异常".to_string(),
                ],
                version_files: vec![
                    "client.jar · 17.5 MB (SHA1: 1b557e...c62a)".to_string(),
                    "assetIndex: 1.16.json · 295.4 KB (共 2,940 个资源)".to_string(),
                    "libraries · 59 个依赖包 (LWJGL 3.2.2)".to_string(),
                    "client_mappings.txt · 官方混淆映射表 (5.8 MB)".to_string(),
                    "logging: client-1.12.xml · 标准日志配置".to_string(),
                ],
                system_requirements: vec![
                    "运行环境：Java 8 (建议 8u312+) 或 Java 11/17".to_string(),
                    "推荐内存：原版 1536 MB / 模组包 4096 MB - 8192 MB".to_string(),
                    "显卡支持：OpenGL 2.1 / 3.2 (兼容老旧核显)".to_string(),
                    "磁盘空间：约 850 MB 可用空间".to_string(),
                    "启动参数：支持经典 G1GC 与 CMS 垃圾回收调优".to_string(),
                ],
                related_resources: vec![
                    "Forge 36.2.42 (1.16.5 终极稳定版)".to_string(),
                    "OptiFine HD U G8 (高清修复与光影)".to_string(),
                    "Fabric API 0.42.0+1.16".to_string(),
                    "JEI 7.8.0.1009 (物品合成查询)".to_string(),
                    "Twilight Forest (暮色森林 1.16.5)".to_string(),
                ],
                changelog_url: "https://zh.minecraft.wiki/w/Java%E7%89%881.16.5".to_string(),
                url: "https://piston-meta.mojang.com/v1/packages/1.16.5.json".to_string(),
            },
            GameVersion {
                id: "1.12.2".to_string(),
                title: "Minecraft 1.12.2".to_string(),
                version_type: GameVersionType::Release,
                release_time: Utc.with_ymd_and_hms(2017, 9, 18, 12, 0, 0).unwrap(),
                release_date: "2017-09-18".to_string(),
                java_version: "Java 8".to_string(),
                supported_loaders: vec![
                    "原版".to_string(),
                    "Forge".to_string(),
                ],
                summary: "Minecraft 1.12.2（多彩世界更新）是经典 Forge 模组黄金时代的核心版本，拥有海量传奇工业、魔法、拔刀剑与空岛整合包。".to_string(),
                highlights: vec![
                    "新增 16 色混凝土、混凝土粉末与带釉陶瓦方块".to_string(),
                    "引入合成配方书（Recipe Book）与全新进度系统".to_string(),
                    "新增鹦鹉生物、幻术师与教学提示机制".to_string(),
                    "极低内存占用与卓越的老硬件高帧率表现".to_string(),
                    "兼容数万款经典 Forge 1.12.2 世代模组".to_string(),
                ],
                version_files: vec![
                    "client.jar · 10.2 MB (SHA1: 0f275b...e501)".to_string(),
                    "assetIndex: 1.12.json · 184.2 KB (共 1,980 个资源)".to_string(),
                    "libraries · 46 个依赖包 (LWJGL 2.9.4)".to_string(),
                    "legacy_assets · 支持旧版资源目录映射".to_string(),
                    "logging: client-1.12.xml · 标准日志配置".to_string(),
                ],
                system_requirements: vec![
                    "运行环境：必须使用 Java 8 (64-bit Hotspot JRE/JDK 1.8)".to_string(),
                    "推荐内存：原版 1024 MB / 大型整合包 4096 MB - 6144 MB".to_string(),
                    "显卡支持：OpenGL 2.1 及以上".to_string(),
                    "磁盘空间：约 550 MB 可用空间".to_string(),
                    "注意事项：Java 9+ 无法直接引导旧版 Forge 14.23".to_string(),
                ],
                related_resources: vec![
                    "Forge 14.23.5.2860 (1.12.2 官方推荐版)".to_string(),
                    "OptiFine 1.12.2 HD U G5 (高清修复)".to_string(),
                    "FoamFix & CensoredASM (1.12 内存与启动优化)".to_string(),
                    "Just Enough Items (JEI) 4.16.1".to_string(),
                    "Applied Energistics 2 (应用能源 2)".to_string(),
                ],
                changelog_url: "https://zh.minecraft.wiki/w/Java%E7%89%881.12.2".to_string(),
                url: "https://piston-meta.mojang.com/v1/packages/1.12.2.json".to_string(),
            },
            GameVersion {
                id: "24w14a".to_string(),
                title: "Snapshot 24w14a".to_string(),
                version_type: GameVersionType::Snapshot,
                release_time: Utc.with_ymd_and_hms(2024, 4, 3, 12, 0, 0).unwrap(),
                release_date: "2024-04-03".to_string(),
                java_version: "Java 21".to_string(),
                supported_loaders: vec![
                    "原版".to_string(),
                    "Fabric".to_string(),
                ],
                summary: "Snapshot 24w14a 是 Minecraft 1.21 开发周期的重要快照版本，首次引入重锤（Mace）武器、旋风人（Breeze）棒与试炼密室新机制。".to_string(),
                highlights: vec![
                    "新增高空坠落暴击武器「重锤（Mace）」".to_string(),
                    "新增不祥之瓶与不祥试炼事件机制".to_string(),
                    "新增旋风人掉落物旋风棒与风弹合成".to_string(),
                    "新增 3 种重锤专属附魔（致密、破甲、风爆）".to_string(),
                    "升级最低运行环境要求至 Java 21".to_string(),
                ],
                version_files: vec![
                    "client.jar · 26.1 MB (SHA1: 8c3a11...f09e)".to_string(),
                    "assetIndex: 17.json · 440.2 KB (快照实验资源)".to_string(),
                    "libraries · 84 个依赖包 (LWJGL 3.3.3)".to_string(),
                    "client_mappings.txt · 官方混淆映射表".to_string(),
                    "experiment: update_1_21 · 内置实验性数据包".to_string(),
                ],
                system_requirements: vec![
                    "运行环境：必须使用 Java 21 (64-bit)".to_string(),
                    "推荐内存：2048 MB - 4096 MB".to_string(),
                    "显卡支持：OpenGL 3.2 Core Profile".to_string(),
                    "存档提示：快照版本请勿直接打开正式版主力存档".to_string(),
                    "调试支持：默认启用详细崩溃追踪日志".to_string(),
                ],
                related_resources: vec![
                    "Fabric Loader 0.15.10 (支持快照 24w14a)".to_string(),
                    "Fabric API 0.97.0+24w14a".to_string(),
                    "Sodium 24w14a 快照适配版".to_string(),
                    "Minecraft Wiki 24w14a 快照评测".to_string(),
                    "Mojang Bug Tracker (快照问题反馈)".to_string(),
                ],
                changelog_url: "https://zh.minecraft.wiki/w/24w14a".to_string(),
                url: "https://piston-meta.mojang.com/v1/packages/24w14a.json".to_string(),
            },
        ])
    }

    async fn search_mods(
        &self,
        query: &str,
        _mc_version: Option<&str>,
    ) -> LauncherResult<Vec<ModProject>> {
        let sample = vec![
            ModProject {
                id: "fabric-api".to_string(),
                slug: "fabric-api".to_string(),
                title: "Fabric API".to_string(),
                description: "Essential hooks and compatibility layers for Fabric mods."
                    .to_string(),
                loaders: vec!["fabric".to_string()],
                game_versions: vec!["1.21".to_string(), "1.20.4".to_string()],
                icon_url: None,
            },
            ModProject {
                id: "sodium".to_string(),
                slug: "sodium".to_string(),
                title: "Sodium".to_string(),
                description: "Modern rendering engine for Minecraft.".to_string(),
                loaders: vec!["fabric".to_string(), "neoforge".to_string()],
                game_versions: vec![
                    "1.21".to_string(),
                    "1.20.6".to_string(),
                    "1.20.4".to_string(),
                ],
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
