use super::*;
use crate::service::{
    instance_write::{digest, files},
    mod_resolver::inventory,
};
use crate::{GameLayout, LocalClientService};
use sha1::{Digest, Sha1};
use sha2::Sha512;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};
pub const LOCK_PATH: &str = ".memoria/instance-lock.json";
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LockedFile {
    pub path: String,
    pub size: u64,
    pub sha1: String,
    pub sha256: String,
    pub sha512: String,
    #[serde(default)]
    pub downloads: Vec<String>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lock {
    pub format: String,
    pub version: u32,
    pub minecraft: String,
    pub loader: Option<Loader>,
    pub metadata_digest: String,
    pub include_configs: bool,
    pub files: Vec<LockedFile>,
    pub warnings: Vec<String>,
}
#[derive(Debug, Clone, Default, Serialize)]
pub struct Verification {
    pub matches: bool,
    pub missing: Vec<String>,
    pub changed: Vec<String>,
    pub extra: Vec<String>,
    pub environment_changed: bool,
}
#[derive(Debug, Clone)]
pub(crate) struct Replacement {
    pub path: String,
    pub staged: Option<String>,
}
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
pub(crate) fn content(path: &str, configs: bool) -> bool {
    ["mods/", "resourcepacks/", "shaderpacks/"]
        .iter()
        .any(|prefix| path.starts_with(prefix))
        || configs
            && ["config/", "defaultconfigs/", "kubejs/", "scripts/"]
                .iter()
                .any(|prefix| path.starts_with(prefix))
        || configs && ["options.txt", "servers.dat"].contains(&path)
}
fn walk(
    base: &Path,
    relative: &str,
    output: &mut BTreeMap<String, PathBuf>,
    depth: usize,
) -> Result<(), String> {
    if depth > 16 {
        return Err("游戏内容目录嵌套过深".into());
    }
    let path = files::safe_join(base, relative).map_err(error)?;
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(error(e)),
    };
    for entry in entries {
        let entry = entry.map_err(error)?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "文件名不是有效文本")?;
        let relative = format!("{relative}/{name}");
        let path = files::safe_join(base, &relative).map_err(error)?;
        let meta = fs::symlink_metadata(&path).map_err(error)?;
        if meta.is_dir() {
            walk(base, &relative, output, depth + 1)?;
        } else if meta.is_file() {
            output.insert(relative, path);
        } else {
            return Err("游戏内容包含非普通文件".into());
        }
        if output.len() > 4096 {
            return Err("游戏内容超过 4096 个文件".into());
        }
    }
    Ok(())
}
pub async fn capture(root: &Path, key: &str, include_configs: bool) -> Result<Lock, String> {
    capture_with(root, key, include_configs, &[]).await
}
pub(crate) async fn capture_with(
    root: &Path,
    key: &str,
    include_configs: bool,
    replacements: &[Replacement],
) -> Result<Lock, String> {
    let report = LocalClientService::new(GameLayout::new(root).map_err(error)?)
        .scan()
        .await
        .map_err(error)?;
    let client = report
        .clients
        .into_iter()
        .find(|c| c.instance.id == key)
        .ok_or("实例不存在或元数据无效")?;
    let loader = client.instance.loader.as_ref().and_then(|l| {
        normalized_loader(
            l.loader_type.clone(),
            &l.version,
            &client.instance.mc_version,
        )
    });
    let base = files::instance_dir(root, key).map_err(error)?;
    let mut paths = BTreeMap::new();
    for folder in ["mods", "resourcepacks", "shaderpacks"] {
        walk(&base, folder, &mut paths, 0)?;
    }
    if include_configs {
        for folder in ["config", "defaultconfigs", "kubejs", "scripts"] {
            walk(&base, folder, &mut paths, 0)?;
        }
        for name in ["options.txt", "servers.dat"] {
            let source = files::safe_join(&base, name).map_err(error)?;
            if source.exists() {
                paths.insert(name.into(), source);
            }
        }
    }
    for replacement in replacements
        .iter()
        .filter(|r| content(&r.path, include_configs))
    {
        files::safe_join(&base, &replacement.path).map_err(error)?;
        if let Some(source) = &replacement.staged {
            paths.insert(
                replacement.path.clone(),
                files::safe_join(&base, source).map_err(error)?,
            );
        } else {
            paths.remove(&replacement.path);
        }
    }
    if paths.len() > 4096 {
        return Err("锁定文件超过 4096 项".into());
    }
    let manifest_path = replacements
        .iter()
        .find(|r| r.path == ".memoria/resource-install.json")
        .and_then(|r| r.staged.as_deref())
        .unwrap_or(".memoria/resource-install.json");
    let manifest_path = files::safe_join(&base, manifest_path).map_err(error)?;
    let manifest: serde_json::Value = if manifest_path.exists() {
        serde_json::from_slice(
            &files::read_bounded(&manifest_path, 4 * 1024 * 1024).map_err(error)?,
        )
        .map_err(error)?
    } else {
        serde_json::json!({})
    };
    let mut locked = vec![];
    let mut total = 0u64;
    let mut warnings = vec![];
    for (path, source) in paths {
        let bytes = files::read_bounded(&source, 64 * 1024 * 1024).map_err(error)?;
        total += bytes.len() as u64;
        if total > 1024 * 1024 * 1024 {
            return Err("锁定内容超过 1 GiB".into());
        }
        let sha256 = digest(&bytes);
        let sha1 = format!("{:x}", Sha1::digest(&bytes));
        let sha512 = format!("{:x}", Sha512::digest(&bytes));
        let record = manifest["resources"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|record| {
                record["path"] == path
                    && record["sha256"] == sha256
                    && record["sha1"] == sha1
                    && record["sha512"] == sha512
            });
        let downloads = record
            .and_then(|r| r["downloads"].as_array())
            .into_iter()
            .flatten()
            .filter_map(|url| url.as_str())
            .filter(|url| {
                crate::service::resource_install::check_url(url).is_ok()
                    && url::Url::parse(url).is_ok_and(|url| url.query().is_none())
            })
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if path.starts_with("mods/") && downloads.is_empty() {
            warnings.push(format!(
                "{path} 未绑定可复用的下载来源；导出时将作为本地覆盖文件。"
            ));
        }
        locked.push(LockedFile {
            path,
            size: bytes.len() as u64,
            sha1,
            sha256,
            sha512,
            downloads,
            project: record
                .and_then(|r| r["project"].as_str())
                .map(str::to_owned),
            version: record
                .and_then(|r| r["version"].as_str())
                .map(str::to_owned),
        });
    }
    let lock = Lock {
        format: "memoria-instance-lock".into(),
        version: 1,
        minecraft: client.instance.mc_version,
        loader,
        metadata_digest: inventory::environment_digest(root, key)?,
        include_configs,
        files: locked,
        warnings,
    };
    validate(&lock)?;
    Ok(lock)
}
pub fn validate(lock: &Lock) -> Result<(), String> {
    if lock.format != "memoria-instance-lock" || lock.version != 1 || lock.files.len() > 4096 {
        return Err("不支持的锁定文件格式或大小".into());
    }
    component(&lock.minecraft)?;
    if lock.metadata_digest.len() != 64
        || !lock.metadata_digest.bytes().all(|b| b.is_ascii_hexdigit())
        || lock.warnings.len() > 4096
    {
        return Err("锁定元数据摘要或提示数量无效".into());
    }
    if let Some(loader) = &lock.loader {
        component(&loader.version)?;
    }
    let mut names = HashSet::new();
    let mut total = 0u64;
    for file in &lock.files {
        files::safe_join(Path::new("."), &file.path).map_err(error)?;
        total = total.saturating_add(file.size);
        if !content(&file.path, lock.include_configs)
            || !names.insert(file.path.to_ascii_lowercase())
            || file.size > 64 * 1024 * 1024
            || total > 1024 * 1024 * 1024
            || file.downloads.len() > 8
        {
            return Err("锁定内容路径、数量或大小无效".into());
        }
        for (hash, len) in [(&file.sha1, 40), (&file.sha256, 64), (&file.sha512, 128)] {
            if hash.len() != len || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("锁定内容哈希无效".into());
            }
        }
        for url in &file.downloads {
            crate::service::resource_install::check_url(url)?;
        }
    }
    if serde_json::to_vec(lock).map_err(error)?.len() > 4 * 1024 * 1024 {
        return Err("锁定记录超过 4 MiB".into());
    }
    Ok(())
}
pub fn read(root: &Path, key: &str) -> Result<Lock, String> {
    let base = files::instance_dir(root, key).map_err(error)?;
    let bytes = files::read_bounded(
        &files::safe_join(&base, LOCK_PATH).map_err(error)?,
        4 * 1024 * 1024,
    )
    .map_err(error)?;
    let lock = serde_json::from_slice(&bytes).map_err(error)?;
    validate(&lock)?;
    Ok(lock)
}
pub async fn verify(root: &Path, key: &str, lock: &Lock) -> Result<Verification, String> {
    validate(lock)?;
    let actual = capture(root, key, lock.include_configs).await?;
    let mut report = Verification {
        environment_changed: actual.metadata_digest != lock.metadata_digest
            || actual.minecraft != lock.minecraft
            || actual.loader != lock.loader,
        ..Default::default()
    };
    for file in &lock.files {
        match actual.files.iter().find(|f| f.path == file.path) {
            None => report.missing.push(file.path.clone()),
            Some(current) if current.sha256 != file.sha256 => {
                report.changed.push(file.path.clone())
            }
            _ => {}
        }
    }
    report.extra = actual
        .files
        .iter()
        .filter(|f| !lock.files.iter().any(|old| old.path == f.path))
        .map(|f| f.path.clone())
        .collect();
    report.matches = !report.environment_changed
        && report.missing.is_empty()
        && report.changed.is_empty()
        && report.extra.is_empty();
    Ok(report)
}
pub async fn save(root: &Path, key: &str, lock: &Lock) -> Result<(), String> {
    let base = files::instance_dir(root, key).map_err(error)?;
    let _guard = files::lock(&base).map_err(error)?;
    if !verify(root, key, lock).await?.matches {
        return Err("实例在生成锁定记录后已变化，请重新生成".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    let stage = format!(".memoria/lock-previews/{id}.json");
    files::create_dirs(&base, ".memoria/lock-previews").map_err(error)?;
    files::write_new(
        &files::safe_join(&base, &stage).map_err(error)?,
        &serde_json::to_vec_pretty(lock).map_err(error)?,
    )
    .map_err(error)?;
    crate::service::file_transaction::execute_locked(
        &base,
        &id,
        "instance_lockfile",
        &[crate::service::file_transaction::Operation {
            path: LOCK_PATH.into(),
            old_sha256: files::file_hash(&files::safe_join(&base, LOCK_PATH).map_err(error)?)
                .map_err(error)?,
            staged: Some(stage),
        }],
    )
    .map_err(error)?;
    Ok(())
}
