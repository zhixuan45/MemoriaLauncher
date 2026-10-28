//! Previewed desktop resource installation, isolated from MCP permissions.
use super::{
    desktop_instances::{directory, error, Result},
    instance_write::{digest, files},
    resources::{ResourceCatalog, ResourceKind},
};
use crate::{DownloadClient, GameLayout, LoaderType, LocalClientService};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use sha2::Sha512;
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    fs,
    io::Read,
    path::PathBuf,
    sync::Arc,
};

const MANIFEST: &str = ".memoria/resource-install.json";
const LIMIT: usize = 64 * 1024 * 1024;
#[path = "resource_dependencies.rs"]
mod dependencies;
#[derive(Debug, Clone)]
pub struct Request {
    pub root: PathBuf,
    pub instance: String,
    pub kind: ResourceKind,
    pub project: String,
    pub version: String,
    pub optional: bool,
}

#[cfg(test)]
#[path = "resource_install_tests.rs"]
mod tests;
#[derive(Debug, Clone)]
pub struct InstallFile {
    pub path: String,
    pub size: u64,
    pub source: String,
    urls: Vec<String>,
    sha1: String,
    sha512: Option<String>,
    inline: Option<Vec<u8>>,
    remove: bool,
}
#[derive(Debug, Clone)]
pub struct Preview {
    pub request: Request,
    pub files: Vec<InstallFile>,
    pub warnings: Vec<String>,
    pub title: String,
    pub target: String,
    pub bytes: u64,
    watches: Vec<files::Watch>,
    environment_digest: String,
    #[cfg(test)]
    fail_commit_at: Option<usize>,
}
#[derive(Debug, Clone)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
    pub message: String,
}
pub type ProgressCallback = Arc<dyn Fn(Progress) + Send + Sync>;
fn identifier(v: &str) -> Result<()> {
    if v.is_empty()
        || v.len() > 100
        || !v
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
    {
        return Err("资源 ID 无效".into());
    }
    Ok(())
}
fn string<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .ok_or_else(|| format!("资源元数据缺少 {key}"))
}
fn contains(v: &Value, text: &str) -> bool {
    v.as_array().is_some_and(|a| a.iter().any(|v| v == text))
}
fn loader(value: Option<&crate::LoaderInfo>) -> (&str, &str) {
    value
        .map(|l| {
            (
                match l.loader_type {
                    LoaderType::Fabric => "fabric",
                    LoaderType::Quilt => "quilt",
                    LoaderType::Forge => "forge",
                    LoaderType::NeoForge => "neoforge",
                    _ => "minecraft",
                },
                l.version.as_str(),
            )
        })
        .unwrap_or(("minecraft", ""))
}
fn allowed_path(path: &str) -> Result<()> {
    files::safe_join(std::path::Path::new("."), path).map_err(error)?;
    let first = path
        .split('/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ![
        "mods",
        "resourcepacks",
        "shaderpacks",
        "config",
        "defaultconfigs",
        "kubejs",
        "scripts",
        "options.txt",
        "servers.dat",
    ]
    .contains(&first.as_str())
    {
        return Err(format!("整合包路径不属于游戏内容目录：{path}"));
    }
    Ok(())
}
fn file(v: &Value, path: String, source: String) -> Result<InstallFile> {
    let sha1 = v["hashes"]["sha1"].as_str().unwrap_or_default().to_owned();
    let size = v["size"]
        .as_u64()
        .or(v["fileSize"].as_u64())
        .ok_or("文件大小缺失")?;
    if sha1.len() != 40
        || !sha1.bytes().all(|c| c.is_ascii_hexdigit())
        || size == 0
        || size > LIMIT as u64
    {
        return Err("文件哈希或大小无效（单文件上限 64 MiB）".into());
    }
    let urls = if let Some(url) = v["url"].as_str() {
        vec![url.into()]
    } else {
        v["downloads"]
            .as_array()
            .ok_or("缺少下载地址")?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(String::from)
                    .ok_or("下载地址无效".to_string())
            })
            .collect::<Result<Vec<_>>>()?
    };
    if urls.is_empty() {
        return Err("缺少下载地址".into());
    }
    for url in &urls {
        check_url(url)?;
    }
    let sha512 = v["hashes"]["sha512"].as_str().map(String::from);
    if sha512
        .as_ref()
        .is_some_and(|s| s.len() != 128 || !s.bytes().all(|v| v.is_ascii_hexdigit()))
    {
        return Err("SHA-512 无效".into());
    }
    Ok(InstallFile {
        path,
        size,
        source,
        urls,
        sha1,
        sha512,
        inline: None,
        remove: false,
    })
}
fn check_url(value: &str) -> Result<()> {
    let url = url::Url::parse(value).map_err(error)?;
    let host = url.host_str().unwrap_or_default();
    let fixture = cfg!(test) && host == "127.0.0.1" && url.scheme() == "http";
    if !(fixture
        || (url.scheme() == "https"
            && [
                "cdn.modrinth.com",
                "github.com",
                "raw.githubusercontent.com",
                "gitlab.com",
            ]
            .contains(&host)))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err("资源下载地址不在支持的源站中".into());
    }
    Ok(())
}
async fn download(client: &DownloadClient, file: &InstallFile) -> Result<Vec<u8>> {
    if let Some(bytes) = &file.inline {
        if !file.sha1.is_empty()
            && (bytes.len() as u64 != file.size
                || !format!("{:x}", Sha1::digest(bytes)).eq_ignore_ascii_case(&file.sha1)
                || file.sha512.as_ref().is_some_and(|hash| {
                    !format!("{:x}", Sha512::digest(bytes)).eq_ignore_ascii_case(hash)
                }))
        {
            return Err(format!("文件校验失败：{}", file.path));
        }
        return Ok(bytes.clone());
    }
    let mut last = "下载失败".to_string();
    for url in &file.urls {
        match client.bytes(url, LIMIT).await.map_err(error) {
            Ok(bytes) => {
                if bytes.len() as u64 != file.size
                    || !format!("{:x}", Sha1::digest(&bytes)).eq_ignore_ascii_case(&file.sha1)
                    || file.sha512.as_ref().is_some_and(|hash| {
                        !format!("{:x}", Sha512::digest(&bytes)).eq_ignore_ascii_case(hash)
                    })
                {
                    last = format!("文件校验失败：{}", file.path);
                    continue;
                }
                return Ok(bytes);
            }
            Err(e) => last = e,
        }
    }
    Err(last)
}
pub async fn prepare(request: Request, client: DownloadClient) -> Result<Preview> {
    prepare_with(request, client, ResourceCatalog::new().map_err(error)?).await
}
async fn prepare_with(
    request: Request,
    client: DownloadClient,
    catalog: ResourceCatalog,
) -> Result<Preview> {
    identifier(&request.project)?;
    identifier(&request.version)?;
    let base = directory(&request.root, &request.instance)?;
    let report = LocalClientService::new(GameLayout::new(&request.root).map_err(error)?)
        .scan()
        .await
        .map_err(error)?;
    let target = report
        .clients
        .iter()
        .find(|v| v.instance.id == request.instance)
        .ok_or("目标实例元数据无效")?;
    let (loader, loader_version) = loader(target.instance.loader.as_ref());
    let game = &target.instance.mc_version;
    let environment_digest =
        super::mod_resolver::inventory::environment_digest(&request.root, &request.instance)?;
    let mut watches = vec![
        files::Watch::file(&base, MANIFEST).map_err(error)?,
        files::Watch::directory(&base, "mods").map_err(error)?,
    ];
    let id = request.instance.split('/').nth(1).ok_or("实例 ID 无效")?;
    watches.push(files::Watch::file(&base, &format!("{id}.json")).map_err(error)?);
    let manifest_path = files::safe_join(&base, MANIFEST).map_err(error)?;
    let mut previous = if manifest_path.exists() {
        serde_json::from_slice::<Value>(
            &files::read_bounded(&manifest_path, 1024 * 1024).map_err(error)?,
        )
        .map_err(error)?
    } else {
        json!({"version":1, "resources":[]})
    };
    let records = previous["resources"]
        .as_array_mut()
        .ok_or("已安装资源清单损坏")?;
    let (mut queue, mut solver_warnings) =
        dependencies::resolve_remote(&request, &catalog, &base, records, game, loader).await?;
    let mut resolved = HashMap::<String, String>::new();
    let mut output = Vec::new();
    let mut warnings = Vec::new();
    warnings.append(&mut solver_warnings);
    let mut incompatible = Vec::new();
    let mut title = String::new();
    while let Some((project, version)) = queue.pop_front() {
        if resolved.len() > 128 {
            return Err("依赖数量超过 128".into());
        }
        identifier(&project)?;
        identifier(&version)?;
        if let Some(old) = resolved.get(&project) {
            if old != &version {
                return Err(format!("项目 {project} 要求互相冲突的版本"));
            }
            continue;
        }
        let metadata = catalog
            .metadata(&format!("/version/{version}"))
            .await
            .map_err(error)?;
        if metadata["id"] != version || metadata["project_id"] != project {
            return Err("资源项目或版本身份不匹配".into());
        }
        let project_meta = catalog
            .metadata(&format!("/project/{project}"))
            .await
            .map_err(error)?;
        if project_meta["client_side"] == "unsupported"
            || metadata["environment"] == "dedicated_server_only"
        {
            return Err("此资源仅支持服务端，不能安装到客户端实例".into());
        }
        let root = project == request.project;
        let kind = if root {
            request.kind
        } else {
            match project_meta["project_type"].as_str() {
                Some("mod") => ResourceKind::Mod,
                Some("resourcepack") => ResourceKind::Resourcepack,
                Some("shader") => ResourceKind::Shader,
                _ => return Err("不支持的资源依赖类别".into()),
            }
        };
        if project_meta["project_type"] != kind.as_str() {
            return Err("所选版本的资源类别不匹配".into());
        }
        if title.is_empty() {
            title = project_meta["title"].as_str().unwrap_or(&project).into();
        }
        if !contains(&metadata["game_versions"], game)
            || (kind == ResourceKind::Mod && !contains(&metadata["loaders"], loader))
        {
            return Err(format!(
                "{} 不兼容目标 Minecraft {game} / {loader}",
                metadata["name"]
            ));
        }
        resolved.insert(project.clone(), version.clone());
        let list = metadata["files"].as_array().ok_or("缺少资源文件列表")?;
        let primary = list
            .iter()
            .find(|v| v["primary"] == true)
            .or(list.first())
            .ok_or("资源文件列表为空")?;
        let filename = string(primary, "filename")?;
        let folder = match kind {
            ResourceKind::Mod => "mods",
            ResourceKind::Resourcepack => "resourcepacks",
            ResourceKind::Shader => "shaderpacks",
            ResourceKind::Modpack => "",
        };
        let extension = match kind {
            ResourceKind::Mod => ".jar",
            ResourceKind::Modpack => ".mrpack",
            _ => ".zip",
        };
        files::basename(filename, extension).map_err(error)?;
        let descriptor = file(
            primary,
            format!("{folder}/{filename}"),
            metadata["name"].as_str().unwrap_or(filename).into(),
        )?;
        if kind == ResourceKind::Modpack {
            let bytes = download(&client, &descriptor).await?;
            let (pack_files, pack_warnings) =
                unpack(&bytes, game, loader, loader_version, request.optional)?;
            output.extend(pack_files);
            warnings.extend(pack_warnings);
        } else {
            if kind == ResourceKind::Mod
                && files::safe_join(&base, &format!("mods/{filename}.disabled"))
                    .map_err(error)?
                    .exists()
            {
                return Err(format!("{filename} 已禁用，请在 Mod 管理中处理后重试"));
            }
            if let Some(old) = records.iter().find(|r| r["project"] == project) {
                if let Some(path) = old["path"].as_str() {
                    if path != descriptor.path {
                        allowed_path(path)?;
                        let old_file = files::safe_join(&base, path).map_err(error)?;
                        if let Some(hash) = files::file_hash(&old_file).map_err(error)? {
                            if old["sha256"] != hash {
                                return Err("旧资源已被外部修改，请先在实例中处理".into());
                            }
                            output.push(InstallFile {
                                path: path.into(),
                                size: 0,
                                source: "移除旧版本（保留备份）".into(),
                                urls: vec![],
                                sha1: String::new(),
                                sha512: None,
                                inline: None,
                                remove: true,
                            });
                        }
                    }
                }
            }
            records.retain(|r| r["project"] != project);
            records.push(json!({"project":project,"version":version,"path":descriptor.path}));
            output.push(descriptor);
        }
        if let Some(deps) = metadata["dependencies"]
            .as_array()
            .filter(|_| request.kind != ResourceKind::Mod)
        {
            for dep in deps {
                match dep["dependency_type"].as_str() {
                    Some("required") if kind != ResourceKind::Modpack => {
                        let mut dep_project =
                            dep["project_id"].as_str().unwrap_or_default().to_string();
                        let dep_version = if let Some(id) = dep["version_id"].as_str() {
                            identifier(id)?;
                            let value = catalog
                                .metadata(&format!("/version/{id}"))
                                .await
                                .map_err(error)?;
                            if dep_project.is_empty() {
                                dep_project = string(&value, "project_id")?.into();
                            }
                            id.into()
                        } else if let Some(version) = resolved.get(&dep_project) {
                            version.clone()
                        } else {
                            identifier(&dep_project)?;
                            let versions = catalog
                                .versions(
                                    &dep_project,
                                    game,
                                    if kind == ResourceKind::Mod {
                                        loader
                                    } else {
                                        ""
                                    },
                                )
                                .await
                                .map_err(error)?;
                            versions
                                .iter()
                                .find(|v| v.version_type == "release")
                                .or(versions.first())
                                .ok_or("找不到必需依赖的兼容版本")?
                                .id
                                .clone()
                        };
                        queue.push_back((dep_project, dep_version));
                    }
                    Some("incompatible") => incompatible.push((
                        dep["project_id"].as_str().map(String::from),
                        dep["version_id"].as_str().map(String::from),
                    )),
                    Some("optional") => {
                        warnings.push("项目包含可选依赖，未自动添加；可从资源目录另行安装。".into())
                    }
                    _ => {}
                }
            }
        }
    }
    for (project, version) in incompatible {
        if records.iter().any(|r| {
            (project.as_ref().is_some_and(|p| r["project"] == *p)
                || (project.is_none() && version.as_ref().is_some_and(|v| r["version"] == *v)))
                && version.as_ref().is_none_or(|v| r["version"] == *v)
        }) {
            return Err("所选资源与已安装或本次计划中的资源不兼容".into());
        }
    }
    if output.iter().map(|file| file.size).sum::<u64>() > 1024 * 1024 * 1024 {
        return Err("单次计划超过 1 GiB".into());
    }
    dependencies::audit_preview(&request, &client, &mut output, &mut watches, &mut warnings)
        .await?;
    let mut names = std::collections::HashSet::new();
    let mut total = 0;
    for entry in &output {
        allowed_path(&entry.path)?;
        if !names.insert(entry.path.to_ascii_lowercase()) {
            return Err(format!("安装清单包含重复路径：{}", entry.path));
        }
        watches.push(files::Watch::file(&base, &entry.path).map_err(error)?);
        total += entry.size;
    }
    if total > 1024 * 1024 * 1024 {
        return Err("单次计划超过 1 GiB".into());
    }
    // Record content hashes after downloads, immediately before committing the manifest.
    let data = serde_json::to_vec_pretty(&previous).map_err(error)?;
    output.push(InstallFile {
        path: MANIFEST.into(),
        size: data.len() as u64,
        source: "来源记录".into(),
        urls: vec![],
        sha1: String::new(),
        sha512: None,
        inline: Some(data),
        remove: false,
    });
    warnings.push(
        "已有同名文件会在确认后替换并保留备份；未知来源的 Mod 仍需自行检查运行兼容性。".into(),
    );
    warnings.sort();
    warnings.dedup();
    Ok(Preview {
        request,
        files: output,
        warnings,
        title,
        target: format!(
            "{} · {game} · {loader} {loader_version}",
            target.instance.name
        ),
        bytes: total,
        watches,
        environment_digest,
        #[cfg(test)]
        fail_commit_at: None,
    })
}

fn unpack(
    bytes: &[u8],
    game: &str,
    loader: &str,
    loader_version: &str,
    optional: bool,
) -> Result<(Vec<InstallFile>, Vec<String>)> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(error)?;
    if zip.len() > 10000 {
        return Err("整合包条目超过 10000".into());
    }
    let index: Value = {
        let mut entry = zip.by_name("modrinth.index.json").map_err(error)?;
        if entry.size() > 4 * 1024 * 1024 {
            return Err("整合包清单过大".into());
        }
        let mut data = Vec::new();
        entry
            .by_ref()
            .take(4 * 1024 * 1024 + 1)
            .read_to_end(&mut data)
            .map_err(error)?;
        serde_json::from_slice(&data).map_err(error)?
    };
    if index["formatVersion"] != 1
        || index["game"] != "minecraft"
        || index["dependencies"]["minecraft"] != game
    {
        return Err("整合包格式或目标游戏版本不匹配".into());
    }
    for (name, version) in index["dependencies"].as_object().ok_or("整合包依赖缺失")? {
        let expected = match name.as_str() {
            "minecraft" => continue,
            "fabric-loader" => "fabric",
            "quilt-loader" => "quilt",
            "forge" => "forge",
            "neoforge" => "neoforge",
            _ => return Err(format!("未知整合包运行依赖：{name}")),
        };
        if expected != loader || version.as_str() != Some(loader_version) {
            return Err(format!(
                "请先安装并选择 Minecraft {game} / {expected} {version} 实例"
            ));
        }
    }
    let mut output = BTreeMap::new();
    let mut warnings = vec![];
    for entry in index["files"].as_array().ok_or("整合包文件清单缺失")? {
        if entry["env"]["client"] == "unsupported" {
            continue;
        }
        if entry["env"]["client"] == "optional" && !optional {
            warnings.push("已跳过整合包可选客户端文件；可勾选包含可选文件后重新预览。".into());
            continue;
        }
        let path = string(entry, "path")?;
        allowed_path(path)?;
        if output
            .insert(
                path.to_string(),
                file(entry, path.into(), "整合包文件".into())?,
            )
            .is_some()
        {
            return Err("整合包存在重复文件路径".into());
        }
    }
    // Client overrides are applied after shared overrides, as required by mrpack.
    for prefix in ["overrides/", "client-overrides/"] {
        let mut seen = std::collections::HashSet::new();
        let mut total = 0u64;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(error)?;
            if entry.is_dir() {
                continue;
            }
            let Some(path) = entry.name().strip_prefix(prefix).map(String::from) else {
                continue;
            };
            allowed_path(&path)?;
            if entry.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
                || !seen.insert(path.to_ascii_lowercase())
            {
                return Err("整合包含链接或重复覆盖路径".into());
            }
            total += entry.size();
            if entry.size() > LIMIT as u64 || total > 128 * 1024 * 1024 {
                return Err("整合包覆盖文件过大".into());
            }
            let mut data = vec![];
            entry
                .by_ref()
                .take(LIMIT as u64 + 1)
                .read_to_end(&mut data)
                .map_err(error)?;
            if data.len() > LIMIT {
                return Err("覆盖文件超出限制".into());
            }
            output.insert(
                path.clone(),
                InstallFile {
                    path,
                    size: data.len() as u64,
                    source: "整合包覆盖文件".into(),
                    urls: vec![],
                    sha1: String::new(),
                    sha512: None,
                    inline: Some(data),
                    remove: false,
                },
            );
        }
    }
    if output.len() > 4096 {
        return Err("整合包文件超过 4096".into());
    }
    Ok((output.into_values().collect(), warnings))
}

pub async fn install(
    preview: Preview,
    client: DownloadClient,
    progress: ProgressCallback,
) -> Result<String> {
    let base = directory(&preview.request.root, &preview.request.instance)?;
    let _lock = files::lock(&base).map_err(error)?;
    if super::mod_resolver::inventory::environment_digest(
        &preview.request.root,
        &preview.request.instance,
    )? != preview.environment_digest
    {
        return Err("实例环境已变化，请重新预览".into());
    }
    for watch in &preview.watches {
        watch.check(&base).map_err(error)?;
    }
    let operation_id = uuid::Uuid::new_v4().to_string();
    let run = format!(".memoria/resource-installs/{operation_id}");
    files::create_dirs(&base, &format!("{run}/stage")).map_err(error)?;
    let journal = json!({"journal_version":2,"transaction":operation_id,"instance":preview.request.instance,"title":preview.title,"files":preview.files.iter().enumerate().map(|(index,file)|json!({"path":file.path,"remove":file.remove,"backup":format!(".memoria/transactions/{operation_id}/before/{index}")})).collect::<Vec<_>>()});
    files::write_new(
        &files::safe_join(&base, &format!("{run}/plan.json")).map_err(error)?,
        &serde_json::to_vec_pretty(&journal).map_err(error)?,
    )
    .map_err(error)?;
    let mut manifest: Value = serde_json::from_slice(
        preview
            .files
            .last()
            .and_then(|v| v.inline.as_deref())
            .ok_or("安装清单缺失")?,
    )
    .map_err(error)?;
    for (index, entry) in preview.files.iter().enumerate() {
        progress(Progress {
            done: index,
            total: preview.files.len(),
            message: format!("下载并校验 {}", entry.path),
        });
        if entry.remove {
            continue;
        }
        let mut bytes = download(&client, entry).await?;
        if entry.path.starts_with("mods/") {
            super::instance_write::validate_jar(&bytes).map_err(error)?;
        }
        if entry.path == MANIFEST {
            bytes = serde_json::to_vec_pretty(&manifest).map_err(error)?;
        } else if let Some(records) = manifest["resources"].as_array_mut() {
            for record in records {
                if record["path"] == entry.path {
                    record["sha256"] = json!(digest(&bytes));
                }
            }
        }
        let stage = files::safe_join(&base, &format!("{run}/stage/{index}")).map_err(error)?;
        files::write_new(&stage, &bytes).map_err(error)?;
    }
    for watch in &preview.watches {
        watch.check(&base).map_err(error)?;
    }
    if super::mod_resolver::inventory::environment_digest(
        &preview.request.root,
        &preview.request.instance,
    )? != preview.environment_digest
    {
        return Err("实例环境已变化，请重新预览".into());
    }
    let operations = preview
        .files
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            let watch = preview
                .watches
                .iter()
                .find(|watch| !watch.directory && watch.path == entry.path)
                .ok_or_else(|| format!("缺少文件快照：{}", entry.path))?;
            Ok(super::file_transaction::Operation {
                path: entry.path.clone(),
                old_sha256: watch.hash.clone(),
                staged: (!entry.remove).then(|| format!("{run}/stage/{index}")),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let id = run.rsplit('/').next().unwrap();
    #[cfg(test)]
    super::file_transaction::execute_with(
        &base,
        id,
        "desktop_resource_install",
        &operations,
        preview.fail_commit_at,
    )
    .map_err(error)?;
    #[cfg(not(test))]
    super::file_transaction::execute_locked(&base, id, "desktop_resource_install", &operations)
        .map_err(error)?;
    progress(Progress {
        done: preview.files.len(),
        total: preview.files.len(),
        message: "安装完成".into(),
    });
    Ok(format!(
        "{} 安装完成；备份：{}",
        preview.title,
        base.join(format!(".memoria/transactions/{id}/before"))
            .display()
    ))
}
