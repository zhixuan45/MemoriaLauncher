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
            let meta = resolved.metadata;
            let loader = meta.libraries.iter().find_map(|lib| {
                let parts: Vec<_> = lib.name.split(':').collect();
                let loader_type = match (parts.first().copied(), parts.get(1).copied()) {
                    (Some("net.fabricmc"), Some("fabric-loader")) => LoaderType::Fabric,
                    (Some("org.quiltmc"), Some("quilt-loader")) => LoaderType::Quilt,
                    (Some("net.neoforged"), Some("neoforge")) => LoaderType::NeoForge,
                    (Some("net.minecraftforge"), Some("forge")) => LoaderType::Forge,
                    _ => return None,
                };
                Some(LoaderInfo {
                    loader_type,
                    version: parts.get(2).unwrap_or(&"").to_string(),
                })
            });
            let created_at = meta
                .release_time
                .as_deref()
                .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
                .map(|d| d.with_timezone(&Utc))
                .unwrap_or(DateTime::<Utc>::UNIX_EPOCH);
            // 根原版可能有显示用重命名；mc_version 取继承链的游戏 JAR 名称。
            let mc_version = meta.client_version.unwrap_or_else(|| {
                resolved
                    .jar_path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string()
            });
            report.clients.push(LocalClient {
                java_major: meta.java_version.map(|j| j.major_version),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GameLayout;

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
