//! Official loader metadata, immutable previews, staged installation and new-profile publication.
mod install;
mod java_installer;
mod profiles;

use crate::service::instance_write::files;
use crate::{DownloadClient, DownloadFile, GameLayout, LauncherError, LauncherResult};
pub use install::install;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}
impl Kind {
    pub const ALL: [Self; 4] = [Self::Fabric, Self::Quilt, Self::Forge, Self::NeoForge];
    pub fn slug(self) -> &'static str {
        match self {
            Self::Fabric => "fabric",
            Self::Quilt => "quilt",
            Self::Forge => "forge",
            Self::NeoForge => "neoforge",
        }
    }
    pub fn requires_installer(self) -> bool {
        matches!(self, Self::Forge | Self::NeoForge)
    }
    fn coordinate(self) -> &'static str {
        match self {
            Self::Fabric => "net.fabricmc:fabric-loader:",
            Self::Quilt => "org.quiltmc:quilt-loader:",
            Self::Forge => "net.minecraftforge:forge:",
            Self::NeoForge => "net.neoforged:neoforge:",
        }
    }
}
impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub version: String,
    pub stable: bool,
}
impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}{}",
            self.version,
            if self.stable { "" } else { "（测试版）" }
        )
    }
}

#[derive(Clone)]
pub struct Catalog {
    client: DownloadClient,
    fabric: String,
    quilt: String,
    forge: String,
    neoforge: String,
}
impl Catalog {
    pub fn new(client: DownloadClient) -> Self {
        Self {
            client,
            fabric: "https://meta.fabricmc.net/v2".into(),
            quilt: "https://meta.quiltmc.org/v3".into(),
            forge: "https://maven.minecraftforge.net".into(),
            neoforge: "https://maven.neoforged.net/releases".into(),
        }
    }

    pub async fn versions(&self, kind: Kind, minecraft: &str) -> LauncherResult<Vec<Version>> {
        component(minecraft)?;
        let mut versions = match kind {
            Kind::Fabric | Kind::Quilt => {
                // This endpoint is already filtered by the exact Minecraft version.
                let base = if kind == Kind::Fabric {
                    &self.fabric
                } else {
                    &self.quilt
                };
                let entries: Vec<Value> = self
                    .client
                    .json(&format!("{base}/versions/loader/{minecraft}"))
                    .await?;
                entries
                    .into_iter()
                    .filter_map(|entry| {
                        let loader = entry.get("loader")?;
                        let version = loader.get("version")?.as_str()?.to_owned();
                        let stable = loader
                            .get("stable")
                            .and_then(Value::as_bool)
                            .unwrap_or_else(|| !version.contains(['-', '+']));
                        Some(Version { version, stable })
                    })
                    .collect::<Vec<_>>()
            }
            Kind::Forge | Kind::NeoForge => {
                let (base, group, artifact) = self.maven(kind);
                let bytes = self
                    .client
                    .bytes(
                        &format!("{base}/{group}/{artifact}/maven-metadata.xml"),
                        4 * 1024 * 1024,
                    )
                    .await?;
                let prefix = if kind == Kind::Forge {
                    format!("{minecraft}-")
                } else {
                    neo_prefix(minecraft)?
                };
                regex::Regex::new(r"<version>\s*([^<]+?)\s*</version>")
                    .unwrap()
                    .captures_iter(&String::from_utf8_lossy(&bytes))
                    .filter_map(|c| {
                        let version = c[1].trim().to_owned();
                        version.starts_with(&prefix).then(|| Version {
                            stable: !["beta", "alpha", "rc"]
                                .iter()
                                .any(|part| version.contains(part)),
                            version,
                        })
                    })
                    .collect()
            }
        };
        versions.retain(|v| component(&v.version).is_ok());
        versions.sort_by(|a, b| {
            numeric(&b.version)
                .cmp(&numeric(&a.version))
                .then(b.version.cmp(&a.version))
        });
        let mut seen = HashSet::new();
        versions.retain(|v| seen.insert(v.version.clone()));
        versions.truncate(1000);
        Ok(versions)
    }

    fn maven(&self, kind: Kind) -> (&str, &str, &str) {
        if kind == Kind::Forge {
            (&self.forge, "net/minecraftforge", "forge")
        } else {
            (&self.neoforge, "net/neoforged", "neoforge")
        }
    }

    pub async fn preview(
        &self,
        root: PathBuf,
        base: String,
        kind: Kind,
        version: String,
    ) -> LauncherResult<Plan> {
        component(&version)?;
        let layout = GameLayout::new(&root)?;
        let snapshot = base_snapshot(&layout, &base)?;
        let resolved = super::metadata::resolve(&layout, &base)?;
        let minecraft = resolved
            .metadata
            .client_version
            .clone()
            .unwrap_or_else(|| resolved.metadata.id.clone());
        let versions = self.versions(kind, &minecraft).await?;
        if !versions.iter().any(|entry| entry.version == version) {
            return Err(invalid("加载器版本不支持所选 Minecraft，请重新查询。"));
        }
        let target = format!("memoria-{}-{version}-{minecraft}", kind.slug());
        component(&target)?;
        if files::safe_join(&root, &format!("versions/{target}"))
            .map_err(invalid)?
            .exists()
        {
            return Err(invalid("同名加载器实例已存在，不会覆盖。"));
        }
        let mut plan = Plan {
            root,
            minecraft,
            base,
            kind,
            version,
            target,
            files: vec![],
            profile: None,
            installer: None,
            snapshot,
            required_java: resolved.metadata.java_version.map(|v| v.major_version),
        };
        if kind.requires_installer() {
            let (base, group, artifact) = self.maven(kind);
            let url = format!(
                "{base}/{group}/{artifact}/{}/{}-{}-installer.jar",
                plan.version, artifact, plan.version
            );
            let hash = profiles::sha1(&self.client, &url).await?;
            let bytes = self.client.bytes(&url, 64 * 1024 * 1024).await?;
            crate::service::downloads::verify_sha1(
                &format!("{:x}", sha1::Sha1::digest(&bytes)),
                Some(&hash),
            )?;
            let profile = profiles::installer_profile(&bytes)?;
            profiles::validate(&profile, &plan)?;
            plan.files = profiles::libraries(&self.client, &profile, false).await?;
            plan.profile = Some(profile);
            plan.installer = Some(DownloadFile {
                url,
                path: "installer.jar".into(),
                sha1: Some(hash),
                size: Some(bytes.len() as u64),
            });
        } else {
            let base = if kind == Kind::Fabric {
                &self.fabric
            } else {
                &self.quilt
            };
            let profile: Value = self
                .client
                .json(&format!(
                    "{base}/versions/loader/{}/{}/profile/json",
                    plan.minecraft, plan.version
                ))
                .await?;
            profiles::validate(&profile, &plan)?;
            plan.files = profiles::libraries(&self.client, &profile, true).await?;
            plan.profile = Some(profile);
        }
        if let Some(version) = plan
            .profile
            .as_ref()
            .and_then(|profile| profile.get("javaVersion"))
        {
            let major = version["majorVersion"]
                .as_u64()
                .filter(|major| (1..=100).contains(major))
                .ok_or_else(|| invalid("加载器 Java 要求无效"))?;
            plan.required_java = Some(major as u32);
        }
        Ok(plan)
    }
}

#[derive(Debug, Clone)]
pub struct Plan {
    root: PathBuf,
    minecraft: String,
    base: String,
    kind: Kind,
    version: String,
    target: String,
    files: Vec<DownloadFile>,
    required_java: Option<u32>,
    installer: Option<DownloadFile>,
    profile: Option<Value>,
    snapshot: BaseSnapshot,
}
impl Plan {
    pub fn target(&self) -> &str {
        &self.target
    }
    pub fn minecraft(&self) -> &str {
        &self.minecraft
    }
    pub fn kind(&self) -> Kind {
        self.kind
    }
    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn required_java(&self) -> Option<u32> {
        self.required_java
    }
    pub fn files(&self) -> &[DownloadFile] {
        &self.files
    }
    pub fn installer(&self) -> Option<&DownloadFile> {
        self.installer.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BaseSnapshot {
    metadata: String,
    jar: String,
}
fn base_snapshot(layout: &GameLayout, base: &str) -> LauncherResult<BaseSnapshot> {
    let dir = files::instance_dir(layout.root(), base).map_err(invalid)?;
    let id = base
        .split_once('/')
        .ok_or_else(|| invalid("实例 ID 无效"))?
        .1;
    let bytes = files::read_bounded(
        &files::safe_join(&dir, &format!("{id}.json")).map_err(invalid)?,
        16 * 1024 * 1024,
    )
    .map_err(invalid)?;
    let metadata: Value = serde_json::from_slice(&bytes)?;
    if metadata.get("inheritsFrom").is_some() || metadata["id"] != id {
        return Err(invalid("请选择独立的原版实例作为基础。"));
    }
    let resolved = super::metadata::resolve(layout, base)?;
    if resolved.metadata.libraries.iter().any(|lib| {
        Kind::ALL
            .iter()
            .any(|kind| lib.name.starts_with(kind.coordinate()))
    }) {
        return Err(invalid("基础实例已经安装加载器。"));
    }
    let jar = files::file_hash(&files::safe_join(&dir, &format!("{id}.jar")).map_err(invalid)?)
        .map_err(invalid)?
        .ok_or_else(|| invalid("原版客户端 JAR 缺失，请先安装原版。"))?;
    Ok(BaseSnapshot {
        metadata: format!("{:x}", Sha256::digest(bytes)),
        jar,
    })
}
fn invalid(e: impl ToString) -> LauncherError {
    LauncherError::InvalidMetadata(e.to_string())
}
fn component(value: &str) -> LauncherResult<()> {
    if value.is_empty()
        || value.len() > 180
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-+".contains(&b))
        || value.contains("..")
    {
        return Err(invalid("版本标识无效"));
    }
    Ok(())
}
fn numeric(value: &str) -> Vec<u64> {
    value
        .split(|c: char| !c.is_ascii_digit())
        .filter_map(|p| p.parse().ok())
        .collect()
}
fn neo_prefix(mc: &str) -> LauncherResult<String> {
    let parts: Vec<_> = mc.split('.').collect();
    if parts.iter().any(|p| p.parse::<u32>().is_err()) {
        return Err(invalid("NeoForge 不支持此 Minecraft 版本"));
    }
    match parts.as_slice() {
        ["1", minor] => Ok(format!("{minor}.0.")),
        ["1", minor, patch] => Ok(format!("{minor}.{patch}.")),
        [year, minor] if year.parse::<u32>().unwrap_or(0) >= 26 => Ok(format!("{year}.{minor}.0.")),
        [year, minor, patch] if year.parse::<u32>().unwrap_or(0) >= 26 => {
            Ok(format!("{year}.{minor}.{patch}."))
        }
        _ => Err(invalid("NeoForge 不支持此 Minecraft 版本")),
    }
}

#[cfg(test)]
mod tests;
