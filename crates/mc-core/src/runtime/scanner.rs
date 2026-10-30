use super::{metadata::resolve, GameLayout};
use crate::{Instance, LauncherResult, LoaderInfo, LoaderType};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
pub struct LocalClient {
    pub instance: Instance,
    pub java_major: Option<u32>,
    /// 仅表示客户端 JAR 存在；完整依赖校验在启动准备阶段执行。
    pub client_jar_present: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanIssue {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ScanReport {
    pub clients: Vec<LocalClient>,
    pub issues: Vec<ScanIssue>,
}

#[derive(Debug, Clone)]
pub struct LocalClientService {
    layout: GameLayout,
}

impl LocalClientService {
    pub fn new(layout: GameLayout) -> Self {
        Self { layout }
    }
    pub fn layout(&self) -> &GameLayout {
        &self.layout
    }

    pub async fn required_java_major(&self, key: String) -> LauncherResult<Option<u32>> {
        let layout = self.layout.clone();
        tokio::task::spawn_blocking(move || Ok(resolve(&layout, &key)?.required_java_major()))
            .await
            .map_err(|e| crate::LauncherError::Internal(e.to_string()))?
    }

    pub async fn scan(&self) -> LauncherResult<ScanReport> {
        let layout = self.layout.clone();
        tokio::task::spawn_blocking(move || scan(&layout))
            .await
            .map_err(|e| crate::LauncherError::Internal(e.to_string()))?
    }
}

fn scan(layout: &GameLayout) -> LauncherResult<ScanReport> {
    let mut report = ScanReport::default();
    for parent in ["versions", "version"] {
        let parent_path = layout.path(parent)?;
        let entries = match std::fs::read_dir(&parent_path) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                report.issues.push(ScanIssue {
                    path: parent_path,
                    message: e.to_string(),
                });
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    report.issues.push(ScanIssue {
                        path: parent_path.clone(),
                        message: e.to_string(),
                    });
                    continue;
                }
            };
            if !entry.path().is_dir() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().to_string();
            let key = format!("{parent}/{id}");
            let resolved = match resolve(layout, &key) {
                Ok(v) => v,
                Err(e) => {
                    report.issues.push(ScanIssue {
                        path: entry.path(),
                        message: e.to_string(),
                    });
                    continue;
                }
            };
            let mc_version = resolved.mc_version();
            let java_major = resolved.required_java_major();
            let meta = resolved.metadata;
            let loader = detect_loader(&meta);
            let created_at = meta
                .release_time
                .as_deref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
            report.clients.push(LocalClient {
                java_major,
                client_jar_present: resolved.jar_path.is_file(),
                instance: Instance {
                    id: key,
                    name: id,
                    mc_version,
                    loader,
                    game_dir: entry.path(),
                    created_at,
                    last_played_at: None,
                },
            });
        }
    }
    report
        .clients
        .sort_by(|a, b| a.instance.id.cmp(&b.instance.id));
    report.issues.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(report)
}

fn detect_loader(meta: &super::metadata::VersionMetadata) -> Option<LoaderInfo> {
    // Modern NeoForge profiles can omit the neoforge artifact entirely.
    // Its explicit version flag is stronger evidence than shared Forge/FML libraries.
    for (flag, loader_type) in [
        ("--fml.neoForgeVersion", LoaderType::NeoForge),
        ("--fml.forgeVersion", LoaderType::Forge),
    ] {
        if let Some(version) = meta.game_argument(flag) {
            return Some(LoaderInfo {
                loader_type,
                version,
            });
        }
    }
    meta.libraries.iter().find_map(|lib| {
        let parts: Vec<_> = lib.name.split(':').collect();
        let loader_type = match (parts.first().copied(), parts.get(1).copied()) {
            (Some("net.fabricmc"), Some("fabric-loader")) => LoaderType::Fabric,
            (Some("org.quiltmc"), Some("quilt-loader")) => LoaderType::Quilt,
            (Some("net.neoforged"), Some("neoforge" | "forge")) => LoaderType::NeoForge,
            (Some("net.minecraftforge"), Some("forge")) => LoaderType::Forge,
            _ => return None,
        };
        Some(LoaderInfo {
            loader_type,
            version: parts.get(2).unwrap_or(&"").to_string(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GameLayout;

    #[test]
    fn detects_explicit_neo_version_without_neoforge_artifact_and_ignores_names() {
        for (arguments, expected_type, expected_version) in [
            (
                serde_json::json!({"game":["--fml.neoForgeVersion","21.1.252","--fml.fmlVersion","4.0.44","--launchTarget","forgeclient"]}),
                LoaderType::NeoForge,
                "21.1.252",
            ),
            (
                serde_json::json!({"game":["--fml.neoForgeVersion=21.1.252"]}),
                LoaderType::NeoForge,
                "21.1.252",
            ),
            (
                serde_json::json!({"game":["--fml.forgeVersion","47.3.0"]}),
                LoaderType::Forge,
                "47.3.0",
            ),
        ] {
            let meta = serde_json::from_value(serde_json::json!({
                "id":"renamed-client", "arguments":arguments,
                "libraries":[{"name":"net.neoforged.fancymodloader:loader:4.0.44"}]
            }))
            .unwrap();
            let loader = detect_loader(&meta).unwrap();
            assert_eq!(loader.loader_type, expected_type);
            assert_eq!(loader.version, expected_version);
        }
        let meta = serde_json::from_value(serde_json::json!({
            "id":"NeoForge-name-only", "libraries":[]
        }))
        .unwrap();
        assert!(detect_loader(&meta).is_none());
        let meta = serde_json::from_value(serde_json::json!({
            "id":"legacy", "minecraftArguments":"--fml.forgeVersion '47.3.0'", "libraries":[]
        }))
        .unwrap();
        assert_eq!(detect_loader(&meta).unwrap().version, "47.3.0");
    }

    #[tokio::test]
    async fn scans_plural_versions_with_inheritance_and_isolated_game_dirs() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/test-fixtures")
            .join(format!("mc-launcher-scan-{}", uuid::Uuid::new_v4()));
        let base = root.join("versions/1.20.1");
        let profile = root.join("versions/fabric-loader-0.15.11-1.20.1");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(&profile).unwrap();
        std::fs::create_dir_all(root.join("libraries")).unwrap();
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(
            base.join("1.20.1.json"),
            r#"{"id":"1.20.1","clientVersion":"1.20.1","mainClass":"net.minecraft.client.main.Main","assets":"1.20","libraries":[]}"#,
        )
        .unwrap();
        std::fs::write(base.join("1.20.1.jar"), b"fixture").unwrap();
        std::fs::write(
            profile.join("fabric-loader-0.15.11-1.20.1.json"),
            r#"{"id":"fabric-loader-0.15.11-1.20.1","inheritsFrom":"1.20.1","libraries":[{"name":"net.fabricmc:fabric-loader:0.15.11"}]}"#,
        )
        .unwrap();

        let layout = GameLayout::new(&root).unwrap();
        let report = LocalClientService::new(layout.clone())
            .scan()
            .await
            .unwrap();

        let client = report
            .clients
            .iter()
            .find(|client| client.instance.id == "versions/fabric-loader-0.15.11-1.20.1")
            .unwrap();
        assert_eq!(client.instance.game_dir, profile);
        assert!(client.client_jar_present);
        assert_eq!(client.instance.mc_version, "1.20.1");
        assert_eq!(
            client.instance.loader.as_ref().unwrap().loader_type,
            LoaderType::Fabric
        );
        assert_eq!(layout.libraries().unwrap(), root.join("libraries"));
        assert_eq!(layout.assets().unwrap(), root.join("assets"));

        std::fs::remove_dir_all(root).unwrap();
    }
}
