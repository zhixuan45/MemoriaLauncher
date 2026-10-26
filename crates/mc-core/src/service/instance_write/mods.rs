use super::{digest, error, files, invalid, validate_jar, PreparedChange, PreparedFile, Result};
use crate::{GameLayout, LoaderType, LocalClientService};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModReference {
    pub source: Source,
    pub project_id: String,
    #[serde(default)]
    pub file_id: Option<String>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Modrinth,
    Curseforge,
}
#[derive(Clone)]
pub struct Resolver {
    client: reqwest::Client,
    modrinth: String,
    curseforge: String,
    key: Option<String>,
    test_transport: bool,
}
impl Resolver {
    pub fn new() -> Result<Self> {
        crate::initialize_runtime();
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("MemoriaLauncher/0.1 (+https://github.com/zhixuan45/MemoriaLauncher)")
            .build()
            .map_err(|e| error("DOWNLOAD_UNAVAILABLE", e))?;
        Ok(Self {
            client,
            modrinth: "https://api.modrinth.com/v2".into(),
            curseforge: "https://api.curseforge.com/v1".into(),
            key: std::env::var("CURSEFORGE_API_KEY")
                .ok()
                .filter(|key| !key.trim().is_empty()),
            test_transport: false,
        })
    }
    async fn bytes(&self, url: &str, key: bool, limit: usize) -> Result<Vec<u8>> {
        let mut request = self.client.get(url);
        if key {
            let value = self.key.as_deref().ok_or_else(|| {
                error(
                    "CURSEFORGE_KEY_REQUIRED",
                    "请通过外部 MCP 客户端安全注入 CURSEFORGE_API_KEY",
                )
            })?;
            let mut header = reqwest::header::HeaderValue::from_str(value)
                .map_err(|_| invalid("CurseForge API Key 无效"))?;
            header.set_sensitive(true);
            request = request.header("x-api-key", header);
        }
        let mut response = request
            .send()
            .await
            .map_err(|e| error("DOWNLOAD_UNAVAILABLE", e))?;
        if !response.status().is_success() {
            return Err(error(
                "DOWNLOAD_UNAVAILABLE",
                format!("源站返回 HTTP {}", response.status()),
            ));
        }
        let mut data = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| error("DOWNLOAD_UNAVAILABLE", e))?
        {
            if data.len() + chunk.len() > limit {
                return Err(error("DOWNLOAD_UNAVAILABLE", "源站文件超过大小限制"));
            }
            data.extend_from_slice(&chunk);
        }
        Ok(data)
    }
    async fn metadata(&self, url: &str, key: bool) -> Result<Value> {
        serde_json::from_slice(&self.bytes(url, key, 4 * 1024 * 1024).await?)
            .map_err(|e| error("INVALID_SOURCE_METADATA", e))
    }
    async fn file(&self, url: &str, sha1: &str, size: u64) -> Result<Vec<u8>> {
        let parsed = url::Url::parse(url).map_err(invalid)?;
        let host = parsed.host_str().unwrap_or_default();
        let fixture = self.test_transport && matches!(host, "127.0.0.1" | "localhost");
        if !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.fragment().is_some()
            || !(fixture
                || (parsed.scheme() == "https"
                    && matches!(
                        host,
                        "cdn.modrinth.com"
                            | "edge.forgecdn.net"
                            | "mediafilez.forgecdn.net"
                            | "media.forgecdn.net"
                    )))
        {
            return Err(error(
                "INVALID_SOURCE_METADATA",
                "源站返回了不受支持的下载地址",
            ));
        }
        if sha1.len() != 40
            || !sha1.bytes().all(|b| b.is_ascii_hexdigit())
            || size == 0
            || size > files::MAX_MOD_BYTES as u64
        {
            return Err(error(
                "INVALID_SOURCE_METADATA",
                "源站缺少有效的 SHA-1 或文件大小",
            ));
        }
        let bytes = self.bytes(url, false, files::MAX_MOD_BYTES).await?;
        if bytes.len() as u64 != size
            || format!("{:x}", Sha1::digest(&bytes)) != sha1.to_ascii_lowercase()
        {
            return Err(error(
                "DOWNLOAD_HASH_MISMATCH",
                "Mod 文件与源站校验信息不符",
            ));
        }
        validate_jar(&bytes)?;
        Ok(bytes)
    }
    async fn resolve(
        &self,
        reference: &ModReference,
        game: &str,
        loader: &str,
    ) -> Result<(Value, Vec<u8>)> {
        match reference.source {
            Source::Modrinth => {
                identifier(&reference.project_id, false)?;
                let project = self
                    .metadata(
                        &format!("{}/project/{}", self.modrinth, reference.project_id),
                        false,
                    )
                    .await?;
                if project["project_type"] != "mod" {
                    return Err(invalid("来源 ID 不是 Mod 项目"));
                }
                let version = if let Some(id) = &reference.file_id {
                    identifier(id, false)?;
                    self.metadata(&format!("{}/version/{id}", self.modrinth), false)
                        .await?
                } else {
                    let mut url = url::Url::parse(&format!(
                        "{}/project/{}/version",
                        self.modrinth, reference.project_id
                    ))
                    .map_err(invalid)?;
                    url.query_pairs_mut()
                        .append_pair("game_versions", &json!([game]).to_string())
                        .append_pair("loaders", &json!([loader]).to_string());
                    let versions = self.metadata(url.as_str(), false).await?;
                    versions
                        .as_array()
                        .and_then(|versions| {
                            versions
                                .iter()
                                .find(|v| v["version_type"] == "release")
                                .or_else(|| versions.first())
                        })
                        .cloned()
                        .ok_or_else(|| {
                            error("NO_COMPATIBLE_FILE", "没有匹配实例版本和加载器的 Mod 文件")
                        })?
                };
                if reference
                    .file_id
                    .as_deref()
                    .is_some_and(|id| version["id"].as_str() != Some(id))
                {
                    return Err(error(
                        "INVALID_SOURCE_METADATA",
                        "源站返回的版本 ID 与指定值不一致",
                    ));
                }
                if version["project_id"] != project["id"]
                    || !has(&version["game_versions"], game)
                    || !has(&version["loaders"], loader)
                {
                    return Err(error(
                        "INCOMPATIBLE_MOD",
                        "文件不属于此项目，或不兼容实例版本/加载器",
                    ));
                }
                let list = version["files"]
                    .as_array()
                    .ok_or_else(|| invalid("源站文件列表缺失"))?;
                let file = list
                    .iter()
                    .find(|f| f["primary"] == true)
                    .or_else(|| list.first())
                    .ok_or_else(|| invalid("源站文件列表为空"))?;
                let filename = file["filename"]
                    .as_str()
                    .ok_or_else(|| invalid("源站文件名缺失"))?;
                files::basename(filename, ".jar")?;
                let bytes = self
                    .file(
                        file["url"]
                            .as_str()
                            .ok_or_else(|| invalid("源站下载地址缺失"))?,
                        file["hashes"]["sha1"].as_str().unwrap_or_default(),
                        file["size"].as_u64().unwrap_or_default(),
                    )
                    .await?;
                let entry = json!({"source":"modrinth", "project_id":project["id"], "file_id":version["id"], "title":project["title"],
                    "filename":filename, "sha256":digest(&bytes), "sha1":file["hashes"]["sha1"], "dependencies":version["dependencies"]});
                Ok((entry, bytes))
            }
            Source::Curseforge => {
                identifier(&reference.project_id, true)?;
                let project = self
                    .metadata(
                        &format!("{}/mods/{}", self.curseforge, reference.project_id),
                        true,
                    )
                    .await?["data"]
                    .clone();
                if project["id"].as_u64() != reference.project_id.parse::<u64>().ok() {
                    return Err(error("INVALID_SOURCE_METADATA", "源站返回的项目 ID 不一致"));
                }
                if project["gameId"] != 432 || project["classId"] != 6 {
                    return Err(invalid("CurseForge ID 不是 Minecraft Mod 项目"));
                }
                if project["allowModDistribution"] == false {
                    return Err(error("DISTRIBUTION_FORBIDDEN", "项目不允许第三方分发"));
                }
                let file = if let Some(id) = &reference.file_id {
                    identifier(id, true)?;
                    self.metadata(
                        &format!(
                            "{}/mods/{}/files/{id}",
                            self.curseforge, reference.project_id
                        ),
                        true,
                    )
                    .await?["data"]
                        .clone()
                } else {
                    let loader_id = match loader {
                        "forge" => 1,
                        "fabric" => 4,
                        "quilt" => 5,
                        "neoforge" => 6,
                        _ => return Err(invalid("实例加载器不受支持")),
                    };
                    let mut url = url::Url::parse(&format!(
                        "{}/mods/{}/files",
                        self.curseforge, reference.project_id
                    ))
                    .map_err(invalid)?;
                    url.query_pairs_mut()
                        .append_pair("gameVersion", game)
                        .append_pair("modLoaderType", &loader_id.to_string())
                        .append_pair("pageSize", "50");
                    let response = self.metadata(url.as_str(), true).await?;
                    response["data"]
                        .as_array()
                        .and_then(|v| v.first())
                        .cloned()
                        .ok_or_else(|| error("NO_COMPATIBLE_FILE", "没有兼容的 CurseForge 文件"))?
                };
                let id = reference.project_id.parse::<u64>().map_err(invalid)?;
                if reference
                    .file_id
                    .as_deref()
                    .is_some_and(|id| file["id"].as_u64() != id.parse::<u64>().ok())
                {
                    return Err(error("INVALID_SOURCE_METADATA", "源站返回的文件 ID 不一致"));
                }
                if file["modId"].as_u64() != Some(id)
                    || file["isAvailable"] == false
                    || !has(&file["gameVersions"], game)
                    || !file["gameVersions"].as_array().is_some_and(|values| {
                        values
                            .iter()
                            .filter_map(Value::as_str)
                            .any(|v| v.eq_ignore_ascii_case(loader))
                    })
                {
                    return Err(error(
                        "INCOMPATIBLE_MOD",
                        "CurseForge 文件不属于此项目或不兼容实例",
                    ));
                }
                let filename = file["fileName"]
                    .as_str()
                    .ok_or_else(|| invalid("源站文件名缺失"))?;
                files::basename(filename, ".jar")?;
                let sha1 = file["hashes"]
                    .as_array()
                    .and_then(|hashes| hashes.iter().find(|h| h["algo"] == 1))
                    .and_then(|h| h["value"].as_str())
                    .ok_or_else(|| invalid("源站文件缺少 SHA-1"))?;
                let download = file["downloadUrl"].as_str().ok_or_else(|| {
                    error(
                        "DOWNLOAD_UNAVAILABLE",
                        "源站不提供下载地址，不会拼接或绕过分发限制",
                    )
                })?;
                let bytes = self
                    .file(
                        download,
                        sha1,
                        file["fileLength"].as_u64().unwrap_or_default(),
                    )
                    .await?;
                Ok((
                    json!({"source":"curseforge", "project_id":reference.project_id, "file_id":file["id"].as_u64().map(|n| n.to_string()), "title":project["name"],
                    "filename":filename,"sha256":digest(&bytes),"sha1":sha1,"dependencies":file["dependencies"]}),
                    bytes,
                ))
            }
        }
    }
}
fn has(array: &Value, value: &str) -> bool {
    array
        .as_array()
        .is_some_and(|items| items.iter().any(|item| item == value))
}
fn identifier(value: &str, numeric: bool) -> Result<()> {
    if value.is_empty()
        || value.len() > 100
        || !value.bytes().all(|b| {
            if numeric {
                b.is_ascii_digit()
            } else {
                b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
            }
        })
    {
        return Err(invalid("来源项目/文件 ID 无效"));
    }
    Ok(())
}
pub async fn prepare(
    base: &Path,
    root: &Path,
    instance: &str,
    references: &[ModReference],
) -> Result<PreparedChange> {
    prepare_with(&Resolver::new()?, base, root, instance, references).await
}
async fn prepare_with(
    resolver: &Resolver,
    base: &Path,
    root: &Path,
    instance: &str,
    references: &[ModReference],
) -> Result<PreparedChange> {
    if references.is_empty() || references.len() > 32 {
        return Err(invalid("一次提交 1–32 个 Mod 来源条目"));
    }
    let report = LocalClientService::new(GameLayout::new(root).map_err(invalid)?)
        .scan()
        .await
        .map_err(invalid)?;
    let client = report
        .clients
        .iter()
        .find(|client| client.instance.id == instance)
        .ok_or_else(|| error("INSTANCE_NOT_FOUND", "未解析到实例元数据"))?;
    let loader = match client.instance.loader.as_ref().map(|l| &l.loader_type) {
        Some(LoaderType::Fabric) => "fabric",
        Some(LoaderType::Forge) => "forge",
        Some(LoaderType::NeoForge) => "neoforge",
        Some(LoaderType::Quilt) => "quilt",
        _ => return Err(error("INCOMPATIBLE_MOD", "实例尚未安装受支持的 Mod 加载器")),
    };
    let manifest_path = ".launcher/mod-list.json";
    let manifest_file = files::safe_join(base, manifest_path)?;
    let watch = files::Watch::file(base, manifest_path)?;
    let mut previous: Value = if manifest_file.exists() {
        let bytes = files::read_bounded(&manifest_file, files::MAX_TEXT_BYTES)?;
        if watch.hash.as_deref() != Some(&digest(&bytes)) {
            return Err(error("PLAN_STALE", "读取 Mod 来源清单时文件发生变化"));
        }
        serde_json::from_slice(&bytes).map_err(invalid)?
    } else {
        json!({"version":1,"mods":[]})
    };
    let entries = previous["mods"]
        .as_array_mut()
        .ok_or_else(|| invalid("已有 Mod 来源清单无效"))?;
    let mut output = Vec::new();
    let mut details = Vec::new();
    let mut total = 0usize;
    for reference in references {
        let (entry, bytes) = resolver
            .resolve(reference, &client.instance.mc_version, loader)
            .await?;
        let filename = entry["filename"]
            .as_str()
            .ok_or_else(|| invalid("缺少文件名"))?;
        if output
            .iter()
            .any(|file: &PreparedFile| file.path.eq_ignore_ascii_case(&format!("mods/{filename}")))
        {
            return Err(invalid("提交列表包含重复或冲突的 JAR 文件名"));
        }
        if let Some(old) = entries.iter().find(|old| {
            old["source"] == entry["source"] && old["project_id"] == entry["project_id"]
        }) {
            let old_filename = old["filename"]
                .as_str()
                .ok_or_else(|| invalid("已有来源清单缺少文件名"))?;
            files::basename(old_filename, ".jar")?;
            let old_path = files::safe_join(base, &format!("mods/{old_filename}"))?;
            if old["filename"] != entry["filename"] && old_path.exists() {
                return Err(error(
                    "MOD_UPDATE_REQUIRES_REMOVAL",
                    "此来源已有不同文件名的版本；请先检查并移走旧 JAR，避免同时加载两个版本",
                ));
            }
        }
        entries.retain(|old| {
            !(old["source"] == entry["source"] && old["project_id"] == entry["project_id"])
        });
        entries.push(entry.clone());
        details.push(entry.clone());
        total += bytes.len();
        if total > files::MAX_PLAN_BYTES {
            return Err(invalid("计划内 Mod 文件合计超过 128 MiB"));
        }
        output.push(PreparedFile {
            path: format!("mods/{filename}"),
            bytes,
        });
    }
    output.push(PreparedFile {
        path: manifest_path.into(),
        bytes: serde_json::to_vec_pretty(&previous).map_err(invalid)?,
    });
    Ok(PreparedChange { files: output, watches: vec![watch], warnings: vec!["来源已固定到具体文件并检查版本/加载器与校验和；依赖信息见详情，未自动安装未列出的依赖。".into()], details: json!({"mods":details}) })
}

#[cfg(test)]
#[path = "mods_tests.rs"]
mod tests;
