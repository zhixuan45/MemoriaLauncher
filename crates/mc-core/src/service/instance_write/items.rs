//! Item names are checked against a user-imported runtime registry, bound to the installed JARs.
use super::{digest, error, files, invalid, io_error, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeSet, io::Read, path::Path};
const CACHE: &str = ".launcher/mcp/item-registry.json";
#[derive(Serialize, Deserialize)]
struct Registry {
    version: u32,
    fingerprint: String,
    items: BTreeSet<String>,
}
#[derive(Serialize)]
pub struct ItemCheck {
    pub id: String,
    pub valid: bool,
    pub code: String,
}
pub fn resource_id(id: &str) -> bool {
    let Some((namespace, name)) = id.split_once(':') else {
        return false;
    };
    !namespace.is_empty()
        && !name.is_empty()
        && id.len() <= 240
        && namespace
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b))
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_./-".contains(&b))
}
fn jars(base: &Path) -> Result<Vec<String>> {
    let directory = files::safe_join(base, "mods")?;
    if !directory.exists() {
        return Ok(vec![]);
    }
    let mut result = vec![];
    for entry in std::fs::read_dir(directory).map_err(io_error)? {
        let name = entry
            .map_err(io_error)?
            .file_name()
            .to_str()
            .ok_or_else(|| invalid("Mod 文件名不是 UTF-8"))?
            .to_owned();
        if name.to_ascii_lowercase().ends_with(".jar") {
            files::basename(&name, ".jar")?;
            result.push(format!("mods/{name}"));
        }
        if result.len() > 512 {
            return Err(invalid("实例 Mod 超过 512 个"));
        }
    }
    result.sort();
    Ok(result)
}
fn snapshot(base: &Path) -> Result<(String, BTreeSet<String>)> {
    let mut records = vec![];
    let mut namespaces = BTreeSet::from(["minecraft".into()]);
    let id = base
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(|| invalid("实例名称无效"))?;
    let metadata = format!("{id}.json");
    records.push((
        metadata.clone(),
        files::file_hash(&files::safe_join(base, &metadata)?)?,
    ));
    for path in jars(base)? {
        let absolute = files::safe_join(base, &path)?;
        let hash = files::file_hash(&absolute)?;
        let bytes = files::read_bounded(&absolute, files::MAX_MOD_BYTES)?;
        if hash.as_deref() != Some(&digest(&bytes)) {
            return Err(error("PLAN_STALE", "读取物品表关联 Mod 时文件发生变化"));
        }
        records.push((path, hash));
        namespaces.extend(mod_namespaces(&bytes)?);
    }
    Ok((
        digest(&serde_json::to_vec(&records).map_err(invalid)?),
        namespaces,
    ))
}
pub fn mod_namespaces(bytes: &[u8]) -> Result<BTreeSet<String>> {
    let names = collect_namespaces(bytes, 0, &mut (64 * 1024 * 1024))?;
    if names.is_empty() {
        return Err(invalid("Mod 元信息中没有可解析的模组 ID"));
    }
    Ok(names)
}
fn collect_namespaces(bytes: &[u8], depth: usize, budget: &mut usize) -> Result<BTreeSet<String>> {
    if depth > 4 || bytes.len() > *budget {
        return Err(invalid("嵌套 Mod 超出 4 层或 64 MiB"));
    }
    *budget -= bytes.len();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(invalid)?;
    let mut names = BTreeSet::new();
    let mut nested = BTreeSet::new();
    for path in [
        "fabric.mod.json",
        "quilt.mod.json",
        "META-INF/mods.toml",
        "META-INF/neoforge.mods.toml",
        "mcmod.info",
    ] {
        let mut entry = match zip.by_name(path) {
            Ok(entry) => entry,
            Err(zip::result::ZipError::FileNotFound) => continue,
            Err(e) => return Err(invalid(e)),
        };
        if entry.size() > files::MAX_TEXT_BYTES as u64 {
            return Err(invalid("Mod 元信息过大"));
        }
        let mut text = String::new();
        entry
            .by_ref()
            .take(files::MAX_TEXT_BYTES as u64 + 1)
            .read_to_string(&mut text)
            .map_err(invalid)?;
        if text.len() > files::MAX_TEXT_BYTES {
            return Err(invalid("Mod 元信息过大"));
        }
        if path.ends_with(".toml") {
            let doc: toml_edit::DocumentMut = text.parse().map_err(invalid)?;
            if let Some(mods) = doc["mods"].as_array_of_tables() {
                for entry in mods {
                    if let Some(id) = entry.get("modId").and_then(|v| v.as_str()) {
                        names.insert(id.into());
                    }
                }
            }
        } else {
            let doc: Value = serde_json::from_str(&text).map_err(invalid)?;
            if path == "fabric.mod.json" {
                if let Some(jars) = doc["jars"].as_array() {
                    for jar in jars {
                        if let Some(file) = jar["file"].as_str() {
                            nested.insert(file.to_owned());
                        }
                    }
                }
                if let Some(id) = doc["id"].as_str() {
                    names.insert(id.into());
                }
                if let Some(provides) = doc["provides"].as_array() {
                    for id in provides.iter().filter_map(Value::as_str) {
                        names.insert(id.into());
                    }
                }
            } else if path == "quilt.mod.json" {
                if let Some(jars) = doc["quilt_loader"]["jars"].as_array() {
                    for jar in jars {
                        if let Some(file) = jar.as_str() {
                            nested.insert(file.to_owned());
                        }
                    }
                }
                if let Some(id) = doc["quilt_loader"]["id"].as_str() {
                    names.insert(id.into());
                }
                if let Some(provides) = doc["quilt_loader"]["provides"].as_array() {
                    for id in provides {
                        if let Some(id) = id.as_str().or_else(|| id["id"].as_str()) {
                            names.insert(id.into());
                        }
                    }
                }
            } else {
                let list = doc.as_array().or_else(|| doc["modList"].as_array());
                if let Some(list) = list {
                    for entry in list {
                        if let Some(id) = entry["modid"].as_str() {
                            names.insert(id.into());
                        }
                    }
                }
            }
        }
    }
    if let Ok(mut metadata) = zip.by_name("META-INF/jarjar/metadata.json") {
        let mut data = vec![];
        metadata
            .by_ref()
            .take(files::MAX_TEXT_BYTES as u64 + 1)
            .read_to_end(&mut data)
            .map_err(invalid)?;
        if data.len() > files::MAX_TEXT_BYTES {
            return Err(invalid("嵌套 Mod 元信息过大"));
        }
        let doc: Value = serde_json::from_slice(&data).map_err(invalid)?;
        if let Some(jars) = doc["jars"].as_array() {
            for jar in jars {
                if let Some(path) = jar["path"].as_str() {
                    nested.insert(path.into());
                }
            }
        }
    }
    if nested.len() > 128 {
        return Err(invalid("嵌套 Mod 数量过多"));
    }
    for path in nested {
        if path.contains('\\')
            || Path::new(&path)
                .components()
                .any(|p| !matches!(p, std::path::Component::Normal(_)))
        {
            return Err(invalid("嵌套 Mod 路径无效"));
        }
        let mut entry = zip.by_name(&path).map_err(invalid)?;
        if entry.size() > files::MAX_MOD_BYTES as u64 {
            return Err(invalid("嵌套 Mod 过大"));
        }
        let mut data = vec![];
        entry
            .by_ref()
            .take(files::MAX_MOD_BYTES as u64 + 1)
            .read_to_end(&mut data)
            .map_err(invalid)?;
        names.extend(collect_namespaces(&data, depth + 1, budget)?);
    }
    Ok(names)
}
/// Called only by the launcher's explicit file picker, never exposed as an MCP write tool.
pub fn import_registry(root: &Path, instance: &str, source: &Path) -> Result<Value> {
    let base = files::instance_dir(root, instance)?;
    let _lock = files::lock(&base)?;
    let value: Value =
        serde_json::from_slice(&files::read_bounded(source, 16 * 1024 * 1024)?).map_err(invalid)?;
    let items: BTreeSet<String> =
        if let Some(entries) = value["minecraft:item"]["entries"].as_object() {
            entries.keys().cloned().collect()
        } else if let Some(items) = value["items"].as_array() {
            items
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| invalid("items 必须是物品 ID 字符串数组"))
                })
                .collect::<Result<_>>()?
        } else {
            return Err(invalid(
                "请导入真实注册表：reports/registries.json 或 {\"items\":[\"minecraft:stone\"]}",
            ));
        };
    if items.is_empty() || items.len() > 200_000 || items.iter().any(|id| !resource_id(id)) {
        return Err(invalid("物品表为空、超过 200000 项或包含无效 ID"));
    }
    let (fingerprint, _) = snapshot(&base)?;
    let count = items.len();
    let bytes = serde_json::to_vec(&Registry {
        version: 1,
        fingerprint,
        items,
    })
    .map_err(invalid)?;
    let target = files::safe_join(&base, CACHE)?;
    let temp = target.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    files::write_new(&temp, &bytes)?;
    std::fs::rename(&temp, &target).map_err(io_error)?;
    Ok(
        json!({"instance_id":instance,"item_count":count,"notice":"已绑定当前实例元数据与 Mod 文件；增删或更新 Mod 后需重新导入"}),
    )
}
pub fn validate(base: &Path, ids: &[String]) -> Result<Vec<ItemCheck>> {
    if ids.len() > 256 {
        return Err(invalid("每次最多检查 256 个物品 ID"));
    }
    let (fingerprint, namespaces) = snapshot(base)?;
    let path = files::safe_join(base, CACHE)?;
    let registry: Option<Registry> = if path.exists() {
        Some(
            serde_json::from_slice(&files::read_bounded(&path, 16 * 1024 * 1024)?)
                .map_err(|e| error("ITEM_REGISTRY_INVALID", e))?,
        )
    } else {
        None
    };
    Ok(ids
        .iter()
        .map(|id| {
            let code = if !resource_id(id) {
                "INVALID_ITEM_ID"
            } else if !namespaces.contains(id.split(':').next().unwrap_or_default()) {
                "MOD_NOT_INSTALLED"
            } else if registry.is_none() {
                "ITEM_REGISTRY_REQUIRED"
            } else if registry
                .as_ref()
                .is_some_and(|r| r.version != 1 || r.fingerprint != fingerprint)
            {
                "ITEM_REGISTRY_STALE"
            } else if !registry.as_ref().unwrap().items.contains(id) {
                "ITEM_NOT_FOUND"
            } else {
                "OK"
            };
            ItemCheck {
                id: id.clone(),
                valid: code == "OK",
                code: code.into(),
            }
        })
        .collect())
}
pub fn require(base: &Path, ids: &[String]) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    for check in validate(base, ids)? {
        if !check.valid {
            return Err(error(
                match check.code.as_str() {
                    "INVALID_ITEM_ID" => "INVALID_ITEM_ID",
                    "MOD_NOT_INSTALLED" => "MOD_NOT_INSTALLED",
                    "ITEM_REGISTRY_REQUIRED" => "ITEM_REGISTRY_REQUIRED",
                    "ITEM_REGISTRY_STALE" => "ITEM_REGISTRY_STALE",
                    _ => "ITEM_NOT_FOUND",
                },
                format!(
                    "{} 未通过校验（{}）。请在 MCP 设置中导入此实例实际运行环境导出的物品注册表",
                    check.id, check.code
                ),
            ));
        }
    }
    Ok(())
}
pub fn watches(base: &Path) -> Result<Vec<files::Watch>> {
    let mut watches = vec![
        files::Watch::directory(base, "mods")?,
        files::Watch::file(base, CACHE)?,
    ];
    for path in jars(base)? {
        watches.push(files::Watch::file(base, &path)?);
    }
    Ok(watches)
}
