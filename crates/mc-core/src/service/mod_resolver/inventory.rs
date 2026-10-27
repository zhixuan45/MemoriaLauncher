use super::*;
use crate::service::instance_write::files;
use crate::{GameLayout, LocalClientService};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::Path};
pub fn environment_digest(root: &Path, instance: &str) -> Result<String, String> {
    let layout = GameLayout::new(root).map_err(|e| e.to_string())?;
    let mut key = instance.to_owned();
    let mut values = vec![];
    for _ in 0..32 {
        let base = files::instance_dir(root, &key).map_err(|e| e.to_string())?;
        let id = key.split_once('/').ok_or("实例 ID 无效")?.1;
        let bytes = files::read_bounded(
            &files::safe_join(&base, &format!("{id}.json")).map_err(|e| e.to_string())?,
            16 * 1024 * 1024,
        )
        .map_err(|e| e.to_string())?;
        values.push(crate::service::instance_write::digest(&bytes));
        let metadata: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if let Some(parent) = metadata["inheritsFrom"].as_str() {
            key = layout.find_version(parent).map_err(|e| e.to_string())?;
        } else {
            return Ok(crate::service::instance_write::digest(
                values.join("\n").as_bytes(),
            ));
        }
    }
    Err("实例继承链过深或包含循环".into())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inventory {
    pub instance: String,
    pub minecraft: String,
    pub loader: String,
    pub loader_version: String,
    pub candidates: Vec<Candidate>,
    pub roots: Vec<Root>,
    pub warnings: Vec<Issue>,
    pub issues: Vec<Issue>,
    pub files: Vec<(String, String)>,
}
pub async fn inspect_instance(root: &Path, instance: &str) -> Result<Inventory, String> {
    let report = LocalClientService::new(GameLayout::new(root).map_err(|e| e.to_string())?)
        .scan()
        .await
        .map_err(|e| e.to_string())?;
    let client = report
        .clients
        .into_iter()
        .find(|entry| entry.instance.id == instance)
        .ok_or("实例不存在或元数据无效")?;
    let loader = client
        .instance
        .loader
        .as_ref()
        .map(|value| match value.loader_type {
            crate::LoaderType::Fabric => "fabric",
            crate::LoaderType::Quilt => "quilt",
            crate::LoaderType::Forge => "forge",
            crate::LoaderType::NeoForge => "neoforge",
            _ => "vanilla",
        })
        .unwrap_or("vanilla");
    let base = files::instance_dir(root, instance).map_err(|e| e.to_string())?;
    let loader_version = client
        .instance
        .loader
        .as_ref()
        .map(|value| value.version.clone())
        .unwrap_or_default();
    let mut inventory = Inventory {
        instance: instance.into(),
        minecraft: client.instance.mc_version,
        loader: loader.into(),
        loader_version,
        candidates: vec![],
        roots: vec![],
        warnings: vec![],
        issues: vec![],
        files: vec![],
    };
    let folder = files::safe_join(&base, "mods").map_err(|e| e.to_string())?;
    match std::fs::read_dir(folder) {
        Ok(entries) => {
            let mut names = vec![];
            for entry in entries {
                let name = entry
                    .map_err(|e| e.to_string())?
                    .file_name()
                    .to_string_lossy()
                    .into_owned();
                if name.to_ascii_lowercase().ends_with(".jar") {
                    names.push(name);
                }
                if names.len() > 1024 {
                    return Err("Mod 文件超过 1024".into());
                }
            }
            names.sort();
            let mut bytes_total = 0usize;
            for name in names {
                let bytes = files::read_bounded(
                    &files::safe_join(&base, &format!("mods/{name}")).map_err(|e| e.to_string())?,
                    files::MAX_MOD_BYTES,
                )
                .map_err(|e| e.to_string())?;
                bytes_total += bytes.len();
                if bytes_total > 512 * 1024 * 1024 {
                    return Err("Mod 扫描超过 512 MiB 预算".into());
                }
                inventory
                    .files
                    .push((name.clone(), crate::service::instance_write::digest(&bytes)));
                inventory.add_jar(&bytes, &name, true);
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
    }
    let id = instance.split_once('/').ok_or("实例标识无效")?.1;
    let profile = files::read_bounded(
        &files::safe_join(&base, &format!("{id}.json")).map_err(|e| e.to_string())?,
        16 * 1024 * 1024,
    )
    .map_err(|e| e.to_string())?;
    let profile: serde_json::Value = serde_json::from_slice(&profile).map_err(|e| e.to_string())?;
    inventory.fact("minecraft", &inventory.minecraft.clone(), true);
    inventory.fact(
        "java",
        &client
            .java_major
            .map(|n| n.to_string())
            .unwrap_or_else(|| "unknown".into()),
        client.java_major.is_some(),
    );
    let version = if loader == "forge" {
        inventory
            .loader_version
            .strip_prefix(&format!("{}-", inventory.minecraft))
            .unwrap_or(&inventory.loader_version)
            .to_owned()
    } else {
        inventory.loader_version.clone()
    };
    match loader {
        "fabric" => inventory.fact("fabricloader", &version, true),
        "quilt" => {
            inventory.fact("quilt_loader", &version, true);
            inventory.fact("fabricloader", "unknown", false);
        }
        "forge" => {
            inventory.fact("forge", &version, true);
            inventory.fact("javafml", &version, true);
        }
        "neoforge" => {
            inventory.fact("neoforge", &version, true);
            let fml = profile["libraries"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|lib| lib["name"].as_str())
                .find_map(|name| name.strip_prefix("net.neoforged.fancymodloader:loader:"))
                .unwrap_or("unknown");
            inventory.fact("javafml", fml, fml != "unknown");
        }
        _ => {}
    }
    Ok(inventory)
}
impl Inventory {
    fn fact(&mut self, id: &str, version: &str, known: bool) {
        self.candidates.push(Candidate {
            id: id.into(),
            version: version.into(),
            source: "environment".into(),
            provides: vec![],
            provided_versions: Default::default(),
            origin: Default::default(),
            dependencies: vec![],
            environment: Environment::Any,
            installed: true,
            metadata_complete: known,
        });
    }
    pub fn add_jar(&mut self, bytes: &[u8], source: &str, installed: bool) {
        match metadata::parse_jar(bytes, source, &self.loader) {
            Ok(mut parsed) => {
                if parsed.candidates.iter().any(|c| {
                    [
                        "minecraft",
                        "java",
                        "forge",
                        "neoforge",
                        "fabricloader",
                        "quilt_loader",
                        "javafml",
                    ]
                    .iter()
                    .any(|id| c.supplies(id))
                }) {
                    self.issues.push(issue(
                        "reserved_mod_id",
                        source,
                        "Mod 不能替代游戏、Java 或加载器的内置身份",
                        vec![],
                    ));
                }
                if parsed.wrong_loader {
                    self.issues.push(issue(
                        "loader_mismatch",
                        source,
                        format!("文件声明的加载器不适用于 {}", self.loader),
                        vec![],
                    ));
                }
                for candidate in &mut parsed.candidates {
                    candidate.installed = installed;
                }
                if installed {
                    self.roots.extend(parsed.roots);
                }
                self.candidates.extend(parsed.candidates);
                self.warnings.extend(parsed.warnings);
            }
            Err(error) => self
                .warnings
                .push(issue("invalid_jar_metadata", source, error, vec![])),
        }
    }
    pub fn audit(&self) -> Report {
        self.resolve(&[])
    }
    pub fn resolve(&self, extra_roots: &[Root]) -> Report {
        let mut roots: Vec<_> = self
            .roots
            .iter()
            .filter(|root| !extra_roots.iter().any(|requested| requested.id == root.id))
            .cloned()
            .collect();
        roots.extend_from_slice(extra_roots);
        let mut report = solve(&self.candidates, &roots, Environment::Client);
        let mut duplicates = HashMap::<String, Vec<String>>::new();
        for root in &self.roots {
            duplicates
                .entry(root.id.clone())
                .or_default()
                .push(root.reason.clone());
        }
        for (id, sources) in duplicates {
            if sources.len() > 1 && !extra_roots.iter().any(|root| root.id == id) {
                report.issues.push(issue(
                    "duplicate_mod",
                    &id,
                    "多个启用的顶层 JAR 声明了同一 Mod ID",
                    sources,
                ));
            }
        }
        report.issues.extend(self.issues.clone());
        report.warnings.extend(self.warnings.clone());
        if self.candidates.iter().any(|candidate| {
            !candidate.origin.is_empty()
                && candidate.source != candidate.origin
                && !report.selected.iter().any(|selected| {
                    selected.id == candidate.id && selected.source == candidate.source
                })
        }) {
            report.warnings.push(issue(
                "nested_activation_unknown",
                "",
                "存在未被当前约束选中的嵌套 Mod，实际加载选择仍需加载器验证",
                vec![],
            ));
            if report.status == Status::Resolved {
                report.status = Status::Unknown;
            }
        }
        if !report.issues.is_empty() && report.status != Status::LimitReached {
            let hard_issue = report.issues.iter().any(|i| {
                !matches!(
                    i.code.as_str(),
                    "invalid_metadata" | "missing_or_incompatible"
                )
            });
            report.status = if !hard_issue
                && (report.status == Status::Unknown
                    || (report
                        .issues
                        .iter()
                        .all(|i| i.code == "missing_or_incompatible")
                        && !self.warnings.is_empty()))
            {
                Status::Unknown
            } else {
                Status::Conflict
            };
        } else if !self.warnings.is_empty() && report.status == Status::Resolved {
            report.status = Status::Unknown;
        }
        for change in &mut report.changes {
            if change.action == "add"
                && self
                    .candidates
                    .iter()
                    .any(|old| old.installed && old.id == change.id && old.source != change.source)
            {
                change.action = "replace_review".into();
                change.reason =
                    "只读建议：核对旧文件、备份和其他前置后，再手动替换；此检查不应用变更。".into();
            }
        }
        if report.status == Status::Conflict || report.status == Status::LimitReached {
            report.changes.clear();
        }
        report
    }
}
pub async fn solve_candidate_files(
    root: &Path,
    instance: &str,
    paths: Vec<std::path::PathBuf>,
) -> Result<Report, String> {
    if paths.is_empty() || paths.len() > 16 {
        return Err("候选 JAR 需要 1–16 个文件".into());
    }
    let mut inventory = inspect_instance(root, instance).await?;
    let original: std::collections::HashSet<_> =
        inventory.roots.iter().map(|r| r.id.clone()).collect();
    let mut requested = vec![];
    let mut requested_ids = std::collections::HashSet::new();
    let mut total = 0;
    for path in paths {
        let bytes = files::read_bounded(&path, files::MAX_MOD_BYTES).map_err(|e| e.to_string())?;
        total += bytes.len();
        if total > 128 * 1024 * 1024 {
            return Err("候选文件合计超过 128 MiB".into());
        }
        let source = format!(
            "候选 {} #{}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            &crate::service::instance_write::digest(&bytes)[..12]
        );
        let parsed = metadata::parse_jar(&bytes, &source, &inventory.loader)?;
        let ids: std::collections::HashSet<_> = parsed.roots.iter().map(|r| r.id.clone()).collect();
        for id in &ids {
            if !requested_ids.insert(id.clone()) {
                inventory.issues.push(issue(
                    "duplicate_candidate",
                    id,
                    "多个候选文件声明同一 Mod ID，不能同时安装",
                    vec![source.clone()],
                ));
            }
        }
        let replaced: std::collections::HashSet<_> = inventory
            .candidates
            .iter()
            .filter(|c| c.installed && ids.iter().any(|id| c.supplies(id)))
            .map(|c| {
                if c.origin.is_empty() {
                    c.source.clone()
                } else {
                    c.origin.clone()
                }
            })
            .collect();
        inventory.candidates.retain(|c| {
            !c.installed
                || !replaced.contains(if c.origin.is_empty() {
                    &c.source
                } else {
                    &c.origin
                })
        });
        inventory
            .issues
            .retain(|issue| !replaced.contains(&issue.subject));
        inventory
            .warnings
            .retain(|issue| !replaced.contains(&issue.subject));
        requested.extend(parsed.roots.into_iter().map(|mut root| {
            root.reason = format!("用户选择的候选：{source}");
            root
        }));
        inventory.add_jar(&bytes, &source, false);
    }
    let mut report = inventory.resolve(&requested);
    for change in &mut report.changes {
        if change.action == "add" && original.contains(&change.id) {
            change.action = "replace_review".into();
            change.reason = "只读替换建议，须核对并备份旧 JAR 后另行确认安装。".into();
        }
    }
    Ok(report)
}
