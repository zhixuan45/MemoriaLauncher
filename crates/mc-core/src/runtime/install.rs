//! Install vanilla clients into the same shared layout used by the launcher.
use super::{
    layout::validate_id,
    metadata,
    rules::{self, Platform, Rule},
};
use crate::{
    DownloadClient, DownloadFile, GameLayout, LauncherError, LauncherResult, ProgressCallback,
};
use serde::Deserialize;
use sha1::{Digest, Sha1};
use std::collections::{BTreeMap, HashMap};

#[derive(Deserialize)]
struct Manifest {
    versions: Vec<ManifestEntry>,
}

#[derive(Deserialize)]
struct ManifestEntry {
    id: String,
    url: String,
    sha1: String,
}

#[derive(Clone, Deserialize)]
struct RemoteFile {
    url: String,
    sha1: Option<String>,
    size: Option<u64>,
    path: Option<String>,
}

impl RemoteFile {
    fn at(&self, path: String) -> DownloadFile {
        DownloadFile {
            url: self.url.clone(),
            sha1: self.sha1.clone(),
            size: self.size,
            path,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Version {
    id: String,
    inherits_from: Option<String>,
    main_class: Option<String>,
    downloads: HashMap<String, RemoteFile>,
    asset_index: AssetIndex,
    #[serde(default)]
    libraries: Vec<Library>,
    #[serde(default)]
    logging: HashMap<String, Logging>,
}

#[derive(Deserialize)]
struct AssetIndex {
    id: String,
    #[serde(flatten)]
    file: RemoteFile,
}

#[derive(Deserialize)]
struct Library {
    name: String,
    url: Option<String>,
    #[serde(default)]
    rules: Vec<Rule>,
    downloads: Option<LibraryDownloads>,
    #[serde(default)]
    natives: HashMap<String, String>,
}

#[derive(Deserialize)]
struct LibraryDownloads {
    artifact: Option<RemoteFile>,
    #[serde(default)]
    classifiers: HashMap<String, RemoteFile>,
}

#[derive(Deserialize)]
struct Logging {
    file: LoggingFile,
}
#[derive(Deserialize)]
struct LoggingFile {
    id: String,
    #[serde(flatten)]
    file: RemoteFile,
}

#[derive(Deserialize)]
struct Assets {
    objects: HashMap<String, AssetObject>,
    #[serde(rename = "virtual", default)]
    virtual_assets: bool,
    #[serde(default)]
    map_to_resources: bool,
}
#[derive(Deserialize)]
struct AssetObject {
    hash: String,
    size: u64,
}

fn invalid(message: impl Into<String>) -> LauncherError {
    LauncherError::InvalidMetadata(message.into())
}

#[derive(Debug, Clone)]
pub struct VanillaPlan {
    id: String,
    metadata: Vec<u8>,
    metadata_file: DownloadFile,
    files: Vec<DownloadFile>,
}
impl VanillaPlan {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn files(&self) -> &[DownloadFile] {
        &self.files
    }
    pub fn required_java(&self) -> Option<u32> {
        serde_json::from_slice::<serde_json::Value>(&self.metadata).ok()?["javaVersion"]
            ["majorVersion"]
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
    }
}
/// Resolve and pin the official metadata and asset index without writing installation files.
pub async fn preview_vanilla(client: DownloadClient, id: String) -> LauncherResult<VanillaPlan> {
    validate_id(&id)?;
    let manifest: Manifest = client
        .json(crate::service::downloads::VERSION_MANIFEST)
        .await?;
    let entry = manifest
        .versions
        .into_iter()
        .find(|version| version.id == id)
        .ok_or_else(|| invalid(format!("版本清单中没有 {id}")))?;
    let bytes = client.bytes(&entry.url, 16 * 1024 * 1024).await?;
    crate::service::downloads::verify_sha1(
        &format!("{:x}", Sha1::digest(&bytes)),
        Some(&entry.sha1),
    )?;
    let version: Version = serde_json::from_slice(&bytes)?;
    if version.id != id || version.inherits_from.is_some() || version.main_class.is_none() {
        return Err(invalid("当前只支持安装独立的原版客户端。"));
    }
    validate_id(&version.asset_index.id)?;
    let index_file = version
        .asset_index
        .file
        .at(format!("assets/indexes/{}.json", version.asset_index.id));
    if index_file
        .sha1
        .as_ref()
        .is_none_or(|hash| hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(invalid("资源索引缺少有效 SHA-1，无法固定安装计划"));
    }
    let index_bytes = client.bytes(&index_file.url, 64 * 1024 * 1024).await?;
    crate::service::downloads::verify_sha1(
        &format!("{:x}", Sha1::digest(&index_bytes)),
        index_file.sha1.as_deref(),
    )?;
    if index_file
        .size
        .is_some_and(|size| size != index_bytes.len() as u64)
    {
        return Err(invalid("资源索引大小不匹配"));
    }
    let assets: Assets = serde_json::from_slice(&index_bytes)?;
    if assets.virtual_assets || assets.map_to_resources {
        return Err(LauncherError::Unsupported(
            "此旧版使用 virtual/map_to_resources 资源布局，当前尚不支持安装。".into(),
        ));
    }
    let validation = GameLayout::new(std::env::temp_dir().join(format!(
        "memoria-metadata-validation-{}",
        uuid::Uuid::new_v4()
    )))?;
    let mut files = BTreeMap::new();
    let mut declared_bytes = 0u64;
    let mut insert = |file: DownloadFile| -> LauncherResult<()> {
        validation.download_path(&file.path)?;
        if !files.contains_key(&file.path) {
            declared_bytes = declared_bytes
                .checked_add(file.size.unwrap_or(0))
                .ok_or_else(|| invalid("游戏文件大小溢出"))?;
            if files.len() >= 200000
                || declared_bytes > 8 * 1024 * 1024 * 1024
                || file.size.is_some_and(|size| size > 128 * 1024 * 1024)
            {
                return Err(invalid(
                    "游戏文件超过数量或大小预算（20 万项/8 GiB，单文件 128 MiB）",
                ));
            }
        }
        if let Some(previous) = files.insert(file.path.clone(), file.clone()) {
            if previous.sha1 != file.sha1 || previous.url != file.url {
                return Err(invalid(format!("重复下载路径的内容不一致：{}", file.path)));
            }
        }
        Ok(())
    };
    insert(index_file)?;
    let game = version
        .downloads
        .get("client")
        .ok_or_else(|| invalid("版本没有客户端 JAR 下载信息"))?;
    if game
        .sha1
        .as_ref()
        .is_none_or(|hash| hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()))
    {
        return Err(invalid("客户端缺少有效 SHA-1，无法固定安装计划"));
    }
    insert(game.at(format!("versions/{id}/{id}.jar")))?;
    let platform = Platform::current();
    for library in version.libraries {
        // OS-version-dependent libraries are a safe superset until Java is selected.
        let version_dependent = library
            .rules
            .iter()
            .any(|rule| rule.os.as_ref().is_some_and(|os| os.version.is_some()));
        if !version_dependent && !rules::allowed(&library.rules, &platform, &HashMap::new())? {
            continue;
        }
        if let Some(downloads) = library.downloads {
            if let Some(artifact) = downloads.artifact {
                let path = artifact
                    .path
                    .clone()
                    .unwrap_or(metadata::maven_path(&library.name)?);
                insert(artifact.at(format!("libraries/{path}")))?;
            }
            if let Some(classifier) = library.natives.get(platform.os) {
                let classifier =
                    classifier.replace("${arch}", if platform.arch == "x86" { "32" } else { "64" });
                let native = downloads
                    .classifiers
                    .get(&classifier)
                    .ok_or_else(|| invalid(format!("{} 缺少 native {classifier}", library.name)))?;
                let path = native
                    .path
                    .as_ref()
                    .ok_or_else(|| invalid("native 缺少 path"))?;
                insert(native.at(format!("libraries/{path}")))?;
            }
        } else {
            let path = metadata::maven_path(&library.name)?;
            insert(DownloadFile {
                url: format!(
                    "{}/{}",
                    library
                        .url
                        .as_deref()
                        .unwrap_or("https://libraries.minecraft.net")
                        .trim_end_matches('/'),
                    path
                ),
                path: format!("libraries/{path}"),
                sha1: None,
                size: None,
            })?;
        }
    }
    for object in assets.objects.into_values() {
        if object.hash.len() != 40 || !object.hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(invalid("非法资源 SHA-1"));
        }
        let suffix = format!("{}/{}", &object.hash[..2], object.hash);
        insert(DownloadFile {
            url: format!("https://resources.download.minecraft.net/{suffix}"),
            path: format!("assets/objects/{suffix}"),
            sha1: Some(object.hash),
            size: Some(object.size),
        })?;
    }
    if let Some(logging) = version.logging.get("client") {
        validate_id(&logging.file.id)?;
        insert(
            logging
                .file
                .file
                .at(format!("assets/log_configs/{}", logging.file.id)),
        )?;
    }
    Ok(VanillaPlan {
        metadata_file: DownloadFile {
            url: entry.url,
            path: format!("versions/{id}/{id}.json"),
            sha1: Some(entry.sha1),
            size: Some(bytes.len() as u64),
        },
        id,
        metadata: bytes,
        files: files.into_values().collect(),
    })
}
pub async fn install_vanilla_plan(
    client: DownloadClient,
    layout: GameLayout,
    plan: VanillaPlan,
    progress: ProgressCallback,
) -> LauncherResult<()> {
    let existing = layout.download_path(&plan.metadata_file.path)?;
    match tokio::fs::read(&existing).await {
        Ok(bytes) if bytes != plan.metadata => {
            return Err(invalid("已有同名版本且配置不同，不会覆盖。"))
        }
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error.into()),
        _ => {}
    }
    client
        .download_files(layout.clone(), plan.files, progress)
        .await?;
    client.download_file(&layout, plan.metadata_file).await?;
    Ok(())
}
/// A job keeps its source and concurrency snapshot for its entire lifetime.
pub async fn install_vanilla(
    client: DownloadClient,
    layout: GameLayout,
    id: String,
    progress: ProgressCallback,
) -> LauncherResult<()> {
    let plan = preview_vanilla(client.clone(), id).await?;
    install_vanilla_plan(client, layout, plan, progress).await
}
