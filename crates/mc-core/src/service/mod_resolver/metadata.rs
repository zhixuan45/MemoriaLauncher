//! Read metadata only: never load a class, run a JAR or extract archive paths to disk.
use super::*;
use serde_json::Value;
use std::{
    collections::HashSet,
    io::{Cursor, Read},
};

#[derive(Debug, Clone, Default)]
pub struct Parsed {
    pub candidates: Vec<Candidate>,
    pub roots: Vec<Root>,
    pub warnings: Vec<Issue>,
    pub wrong_loader: bool,
}
struct Budget {
    bytes: usize,
    entries: usize,
}
pub fn parse_jar(bytes: &[u8], source: &str, loader: &str) -> Result<Parsed, String> {
    let mut budget = Budget {
        bytes: 0,
        entries: 0,
    };
    let mut parsed = parse(bytes, source, loader, 0, &mut budget)?;
    for candidate in &mut parsed.candidates {
        candidate.origin = source.to_owned();
    }
    Ok(parsed)
}
fn read(
    zip: &mut zip::ZipArchive<Cursor<&[u8]>>,
    name: &str,
    limit: usize,
    budget: &mut Budget,
) -> Result<Option<Vec<u8>>, String> {
    let mut entry = match zip.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    if entry.size() > limit as u64
        || entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
    {
        return Err("归档元数据或嵌套文件类型/大小无效".into());
    }
    let mut bytes = vec![];
    entry
        .by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    budget.bytes += bytes.len();
    budget.entries += 1;
    if bytes.len() > limit || budget.bytes > 64 * 1024 * 1024 || budget.entries > 256 {
        return Err("嵌套 JAR 元数据达到读取预算".into());
    }
    Ok(Some(bytes))
}
fn parse(
    bytes: &[u8],
    source: &str,
    loader: &str,
    depth: usize,
    budget: &mut Budget,
) -> Result<Parsed, String> {
    if depth > 6 {
        return Err("嵌套 JAR 过深".into());
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    if zip.len() > 20000 {
        return Err("JAR 条目数量过多".into());
    }
    let mut names = HashSet::new();
    for name in zip.file_names() {
        if !names.insert(name.to_owned()) {
            return Err("JAR 含重复路径，元数据不明确".into());
        }
    }
    let fabric = names.contains("fabric.mod.json");
    let quilt = names.contains("quilt.mod.json");
    let forge = names.contains("META-INF/mods.toml");
    let neo = names.contains("META-INF/neoforge.mods.toml");
    let mut parsed = Parsed::default();
    let mut nested = vec![];
    if (loader == "quilt" && quilt) || (!fabric && !forge && !neo && quilt) {
        let value: Value =
            serde_json::from_slice(&read(&mut zip, "quilt.mod.json", 512 * 1024, budget)?.unwrap())
                .map_err(|e| e.to_string())?;
        let q = &value["quilt_loader"];
        let mut c = candidate(q, source, "id", "version");
        c.provides = aliases(&q["provides"]);
        c.provided_versions = q["provides"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|value| {
                Some((
                    value["id"].as_str()?.to_owned(),
                    value["version"].as_str()?.to_owned(),
                ))
            })
            .collect();
        c.environment = env(value["minecraft"]["environment"].as_str().unwrap_or("*"));
        for (key, relation) in [
            ("depends", Relation::Required),
            ("breaks", Relation::Conflict),
        ] {
            if let Some(deps) = q[key].as_array() {
                for dep in deps {
                    if let Some(id) = dep["id"].as_str() {
                        let constraint = quilt_constraint(&dep["versions"]);
                        c.dependencies.push(Requirement {
                            id: id.into(),
                            constraint,
                            relation: if dep["optional"] == true || dep.get("unless").is_some() {
                                Relation::Optional
                            } else {
                                relation.clone()
                            },
                            reason: format!("quilt_loader.{key}"),
                        });
                        if dep.get("unless").is_some() {
                            c.metadata_complete = false;
                            parsed.warnings.push(issue(
                                "conditional_dependency",
                                &c.id,
                                "Quilt unless 条件需人工确认，未忽略该前置",
                                vec![source.into()],
                            ));
                        }
                    } else {
                        c.metadata_complete = false;
                    }
                }
            }
        }
        nested.extend(
            q["jars"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v.as_str().map(str::to_owned)),
        );
        parsed.wrong_loader = loader != "quilt";
        add(&mut parsed, c, depth == 0);
    } else if fabric && (matches!(loader, "fabric" | "quilt") || (!forge && !neo)) {
        let value: Value = serde_json::from_slice(
            &read(&mut zip, "fabric.mod.json", 512 * 1024, budget)?.unwrap(),
        )
        .map_err(|e| e.to_string())?;
        let mut c = candidate(&value, source, "id", "version");
        c.provides = aliases(&value["provides"]);
        c.environment = env(value["environment"].as_str().unwrap_or("*"));
        for (key, relation) in [
            ("depends", Relation::Required),
            ("recommends", Relation::Recommended),
            ("suggests", Relation::Optional),
            ("breaks", Relation::Conflict),
            ("conflicts", Relation::Discouraged),
        ] {
            if let Some(deps) = value[key].as_object() {
                for (id, value) in deps {
                    c.dependencies.push(Requirement {
                        id: id.clone(),
                        constraint: json_constraint(value),
                        relation: relation.clone(),
                        reason: format!("fabric.mod.json:{key}"),
                    });
                }
            } else if value.get(key).is_some() {
                c.metadata_complete = false;
            }
        }
        nested.extend(
            value["jars"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|v| v["file"].as_str().map(str::to_owned)),
        );
        parsed.wrong_loader = !matches!(loader, "fabric" | "quilt");
        add(&mut parsed, c, depth == 0);
    } else if forge || neo {
        let use_neo = neo && (loader == "neoforge" || !forge);
        let path = if use_neo {
            "META-INF/neoforge.mods.toml"
        } else {
            "META-INF/mods.toml"
        };
        let data = read(&mut zip, path, 512 * 1024, budget)?.unwrap();
        let text = std::str::from_utf8(&data).map_err(|e| e.to_string())?;
        let doc = text
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| e.to_string())?;
        let manifest =
            read(&mut zip, "META-INF/MANIFEST.MF", 128 * 1024, budget)?.unwrap_or_default();
        let manifest_version = String::from_utf8_lossy(&manifest).lines().find_map(|line| {
            line.strip_prefix("Implementation-Version:")
                .map(|value| value.trim().to_owned())
        });
        if let Some(mods) = doc
            .get("mods")
            .and_then(toml_edit::Item::as_array_of_tables)
        {
            for m in mods {
                let id = m
                    .get("modId")
                    .and_then(toml_edit::Item::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let version = m
                    .get("version")
                    .and_then(toml_edit::Item::as_str)
                    .unwrap_or("unknown");
                let version = if version == "${file.jarVersion}" {
                    manifest_version.clone().unwrap_or_else(|| "unknown".into())
                } else {
                    version.into()
                };
                let mut c = Candidate {
                    id: id.clone(),
                    version,
                    source: source.into(),
                    provides: vec![],
                    provided_versions: Default::default(),
                    origin: Default::default(),
                    dependencies: vec![],
                    environment: Environment::Any,
                    installed: true,
                    metadata_complete: true,
                };
                if c.version.contains("${") || c.version == "unknown" {
                    c.metadata_complete = false;
                }
                if let Some(range) = doc.get("loaderVersion").and_then(toml_edit::Item::as_str) {
                    c.dependencies.push(Requirement {
                        id: if use_neo || loader == "neoforge" {
                            "javafml"
                        } else {
                            "forge"
                        }
                        .into(),
                        constraint: Constraint::maven(range),
                        relation: Relation::Required,
                        reason: "Java FML 语言加载器版本".into(),
                    });
                }
                if let Some(deps) = doc
                    .get("dependencies")
                    .and_then(|item| item.get(id.as_str()))
                    .and_then(toml_edit::Item::as_array_of_tables)
                {
                    for dep in deps {
                        let side = dep
                            .get("side")
                            .and_then(toml_edit::Item::as_str)
                            .unwrap_or("BOTH");
                        if side == "SERVER" {
                            continue;
                        }
                        let relation = match dep.get("type").and_then(toml_edit::Item::as_str) {
                            Some("required") => Relation::Required,
                            Some("optional") => Relation::Optional,
                            Some("incompatible") => Relation::Conflict,
                            Some("discouraged") => Relation::Discouraged,
                            Some(_) => {
                                c.metadata_complete = false;
                                Relation::Required
                            }
                            None => {
                                if dep
                                    .get("mandatory")
                                    .and_then(toml_edit::Item::as_bool)
                                    .unwrap_or(true)
                                {
                                    Relation::Required
                                } else {
                                    Relation::Optional
                                }
                            }
                        };
                        c.dependencies.push(Requirement {
                            id: dep
                                .get("modId")
                                .and_then(toml_edit::Item::as_str)
                                .unwrap_or_default()
                                .into(),
                            constraint: dep
                                .get("versionRange")
                                .and_then(toml_edit::Item::as_str)
                                .map(Constraint::maven)
                                .unwrap_or(Constraint::Any),
                            relation,
                            reason: format!("{path}:dependencies.{id}"),
                        });
                        if !matches!(side, "CLIENT" | "BOTH") {
                            c.metadata_complete = false;
                        }
                    }
                }
                add(&mut parsed, c, depth == 0);
            }
        }
        parsed.wrong_loader = if use_neo {
            loader != "neoforge"
        } else {
            !matches!(loader, "forge" | "neoforge")
        };
        if let Some(data) = read(
            &mut zip,
            "META-INF/jarjar/metadata.json",
            512 * 1024,
            budget,
        )? {
            let value: Value = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
            nested.extend(
                value["jars"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v["path"].as_str().map(str::to_owned)),
            );
        }
    } else if depth == 0 {
        parsed.warnings.push(issue(
            "unknown_jar",
            source,
            "未识别到受支持的 Mod 元数据，不能推断兼容性",
            vec![],
        ));
    }
    if (fabric || quilt || forge || neo) && parsed.candidates.is_empty() {
        parsed.warnings.push(issue(
            "empty_mod_metadata",
            source,
            "存在已知格式文件，但未读到可用 Mod 声明，不能判定兼容",
            vec![],
        ));
    }
    for path in nested {
        super::super::instance_write::files::safe_join(std::path::Path::new("."), &path)
            .map_err(|e| e.to_string())?;
        let data = read(&mut zip, &path, 32 * 1024 * 1024, budget)?
            .ok_or_else(|| format!("声明的嵌套 JAR 不存在：{path}"))?;
        let child = parse(
            &data,
            &format!("{source}!{path}"),
            loader,
            depth + 1,
            budget,
        )?;
        parsed.candidates.extend(child.candidates);
        parsed.warnings.extend(child.warnings);
        parsed.wrong_loader |= child.wrong_loader;
    }
    Ok(parsed)
}
fn add(parsed: &mut Parsed, candidate: Candidate, root: bool) {
    if root {
        parsed.roots.push(Root {
            id: candidate.id.clone(),
            constraint: Constraint::Exact(candidate.version.clone()),
            reason: format!("已启用文件 {}", candidate.source),
        });
    }
    parsed.candidates.push(candidate);
}
fn candidate(value: &Value, source: &str, id: &str, version: &str) -> Candidate {
    Candidate {
        id: value[id].as_str().unwrap_or_default().into(),
        version: value[version].as_str().unwrap_or("unknown").into(),
        source: source.into(),
        provides: vec![],
        provided_versions: Default::default(),
        origin: Default::default(),
        dependencies: vec![],
        environment: Environment::Any,
        installed: true,
        metadata_complete: value[id].is_string() && value[version].is_string(),
    }
}
fn env(value: &str) -> Environment {
    match value {
        "*" => Environment::Any,
        "client" => Environment::Client,
        "server" => Environment::Server,
        _ => Environment::Unknown,
    }
}
fn aliases(value: &Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().or(value["id"].as_str()).map(str::to_owned))
        .collect()
}
pub fn json_constraint(value: &Value) -> Constraint {
    match value {
        Value::String(v) => Constraint::fabric(v),
        Value::Array(v) => Constraint::AnyOf(v.iter().map(json_constraint).collect()),
        _ => Constraint::Unknown("未识别的 Fabric 版本表达式".into()),
    }
}
fn quilt_constraint(value: &Value) -> Constraint {
    match value {
        Value::Null => Constraint::Any,
        Value::Object(v) => {
            if v.len() != 1 {
                Constraint::Unknown("Quilt 复合版本字段不能同时判断".into())
            } else if let Some(any) = v.get("any").and_then(Value::as_array) {
                Constraint::AnyOf(any.iter().map(quilt_constraint).collect())
            } else if let Some(all) = v.get("all").and_then(Value::as_array) {
                Constraint::AllOf(all.iter().map(quilt_constraint).collect())
            } else {
                Constraint::Unknown("Quilt 复合版本表达式不明确".into())
            }
        }
        _ => json_constraint(value),
    }
}
