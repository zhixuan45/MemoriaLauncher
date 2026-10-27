//! Build a finite Modrinth candidate universe, then solve the whole graph instead of greedily picking one dependency.
use super::*;
use crate::ResourceCatalog;
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub project: String,
    pub version: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub report: Report,
    pub selections: Vec<Pin>,
    pub requests: usize,
}
pub async fn resolve(
    catalog: &ResourceCatalog,
    requested: &[Pin],
    installed: &[Pin],
    game: &str,
    loader: &str,
) -> Result<Plan, String> {
    if requested.is_empty()
        || requested.len() > 16
        || installed.len() > 128
        || requested.iter().chain(installed).any(|p| {
            !platform_id(&p.project)
                || !platform_id(&p.version)
                || p.project.contains(':')
                || p.version.contains(':')
        })
    {
        return Err("Modrinth 求解入参或数量无效".into());
    }
    let requested_projects: HashSet<_> = requested.iter().map(|p| p.project.as_str()).collect();
    let locked: Vec<_> = installed
        .iter()
        .filter(|p| !requested_projects.contains(p.project.as_str()))
        .cloned()
        .collect();
    let mut queue: VecDeque<_> = requested
        .iter()
        .chain(&locked)
        .map(|p| (Some(p.project.clone()), Some(p.version.clone())))
        .collect();
    let mut projects = HashMap::<String, Value>::new();
    let mut versions = BTreeMap::<String, Value>::new();
    let mut visited = HashSet::new();
    let mut requests = 0;
    let mut discovery = vec![];
    let mut truncated = false;
    while let Some((project, version)) = queue.pop_front() {
        if !visited.insert((project.clone(), version.clone())) {
            continue;
        }
        if requests >= 512 || versions.len() > 2048 || projects.len() > 128 {
            truncated = true;
            break;
        }
        let fetched = if let Some(version) = &version {
            if let Some(value) = versions.get(version) {
                vec![value.clone()]
            } else {
                requests += 1;
                match catalog.metadata(&format!("/version/{version}")).await {
                    Ok(value) => vec![value],
                    Err(e) => {
                        discovery.push(issue(
                            "provider_unavailable",
                            version,
                            e.to_string(),
                            vec![],
                        ));
                        continue;
                    }
                }
            }
        } else if let Some(project) = &project {
            if !projects.contains_key(project) {
                requests += 1;
                match catalog.metadata(&format!("/project/{project}")).await {
                    Ok(value) => {
                        projects.insert(project.clone(), value);
                    }
                    Err(e) => {
                        discovery.push(issue(
                            "provider_unavailable",
                            project,
                            e.to_string(),
                            vec![],
                        ));
                        continue;
                    }
                }
            }
            let kind = projects[project]["project_type"].as_str().unwrap_or("mod");
            requests += 1;
            match catalog
                .version_metadata(project, game, if kind == "mod" { loader } else { "" })
                .await
            {
                Ok(mut values) => {
                    values.sort_by_key(|value| value["version_type"] != "release");
                    if values.len() > 32 {
                        values.truncate(32);
                        truncated = true;
                    }
                    values
                }
                Err(e) => {
                    discovery.push(issue(
                        "provider_unavailable",
                        project,
                        e.to_string(),
                        vec![],
                    ));
                    continue;
                }
            }
        } else {
            continue;
        };
        for value in fetched {
            let id = value["id"].as_str().ok_or("源站版本缺少 ID")?.to_owned();
            let owner = value["project_id"]
                .as_str()
                .ok_or("源站版本缺少项目 ID")?
                .to_owned();
            if !platform_id(&id)
                || !platform_id(&owner)
                || id.contains(':')
                || owner.contains(':')
                || project.as_ref().is_some_and(|p| p != &owner)
                || version.as_ref().is_some_and(|v| v != &id)
            {
                return Err("源站项目/版本身份与请求不一致".into());
            }
            if !projects.contains_key(&owner) {
                requests += 1;
                match catalog.metadata(&format!("/project/{owner}")).await {
                    Ok(meta) => {
                        if meta["id"].as_str().is_some_and(|id| id != owner) {
                            return Err("源站项目身份不一致".into());
                        }
                        projects.insert(owner.clone(), meta);
                    }
                    Err(e) => {
                        discovery.push(issue(
                            "provider_unavailable",
                            &owner,
                            e.to_string(),
                            vec![],
                        ));
                        continue;
                    }
                }
            }
            if versions.contains_key(&id) {
                continue;
            }
            for dependency in value["dependencies"].as_array().into_iter().flatten() {
                let p = dependency["project_id"].as_str().map(str::to_owned);
                let v = dependency["version_id"].as_str().map(str::to_owned);
                if p.as_ref()
                    .is_some_and(|p| !platform_id(p) || p.contains(':'))
                    || v.as_ref()
                        .is_some_and(|v| !platform_id(v) || v.contains(':'))
                {
                    return Err("源站依赖标识无效".into());
                }
                if dependency["dependency_type"] == "required" || v.is_some() {
                    queue.push_back((p, v));
                }
            }
            versions.insert(id, value);
        }
    }
    let mut candidates = vec![];
    for (id, value) in &versions {
        let project = value["project_id"].as_str().unwrap();
        let Some(meta) = projects.get(project) else {
            continue;
        };
        let kind = meta["project_type"].as_str().unwrap_or("mod");
        if !["mod", "resourcepack", "shader"].contains(&kind) {
            continue;
        }
        if value["game_versions"]
            .as_array()
            .is_some_and(|values| !values.iter().any(|v| v == game))
            || (kind == "mod"
                && value["loaders"]
                    .as_array()
                    .is_some_and(|values| !values.iter().any(|v| v == loader)))
        {
            continue;
        }
        let mut c = Candidate {
            id: format!("modrinth:{project}"),
            version: id.clone(),
            source: format!("modrinth:{project}:{id}"),
            provides: vec![],
            provided_versions: Default::default(),
            origin: Default::default(),
            dependencies: vec![],
            environment: match meta["client_side"].as_str() {
                Some("unsupported") => Environment::Server,
                Some("required" | "optional") => Environment::Client,
                _ => Environment::Unknown,
            },
            installed: installed
                .iter()
                .any(|pin| pin.project == project && pin.version == *id),
            metadata_complete: meta["project_type"].is_string()
                && value["dependencies"].is_array()
                && value["game_versions"].is_array()
                && (kind != "mod" || value["loaders"].is_array()),
        };
        for dependency in value["dependencies"].as_array().into_iter().flatten() {
            let version = dependency["version_id"].as_str();
            let owner = dependency["project_id"].as_str().or_else(|| {
                version
                    .and_then(|v| versions.get(v))
                    .and_then(|v| v["project_id"].as_str())
            });
            let Some(owner) = owner else {
                c.metadata_complete = false;
                continue;
            };
            if let Some(v) = version.and_then(|v| versions.get(v)) {
                if v["project_id"] != owner {
                    return Err("依赖的项目与固定版本身份不一致".into());
                }
            }
            let relation = match dependency["dependency_type"].as_str() {
                Some("required") => Relation::Required,
                Some("optional") => Relation::Optional,
                Some("incompatible") => Relation::Conflict,
                Some("embedded") => continue,
                _ => {
                    c.metadata_complete = false;
                    continue;
                }
            };
            c.dependencies.push(Requirement {
                id: format!("modrinth:{owner}"),
                constraint: version
                    .map(|v| Constraint::Exact(v.into()))
                    .unwrap_or(Constraint::Any),
                relation,
                reason: format!("{project} / {id} 声明的依赖"),
            });
        }
        candidates.push(c);
    }
    candidates.sort_by(|a, b| {
        let av = &versions[&a.version];
        let bv = &versions[&b.version];
        (av["version_type"] != "release")
            .cmp(&(bv["version_type"] != "release"))
            .then_with(|| {
                bv["date_published"]
                    .as_str()
                    .unwrap_or_default()
                    .cmp(av["date_published"].as_str().unwrap_or_default())
            })
            .then_with(|| a.version.cmp(&b.version))
    });
    let roots: Vec<_> = requested
        .iter()
        .chain(&locked)
        .map(|pin| Root {
            id: format!("modrinth:{}", pin.project),
            constraint: Constraint::Exact(pin.version.clone()),
            reason: if requested_projects.contains(pin.project.as_str()) {
                format!("用户固定版本 {}", pin.version)
            } else {
                format!("保留已安装版本 {}，不隐式升级", pin.version)
            },
        })
        .collect();
    let mut report = solve(&candidates, &roots, Environment::Client);
    if report.status == Status::Conflict && (truncated || !discovery.is_empty()) {
        report.status = if truncated {
            Status::LimitReached
        } else {
            Status::Unknown
        };
    }
    if truncated {
        discovery.push(issue(
            "candidate_limit",
            "",
            "仅检查有限候选，未证明全局最优或无解",
            vec![],
        ));
    }
    report.warnings.extend(discovery);
    for change in &mut report.changes {
        if let Some(old) = installed.iter().find(|pin| {
            format!("modrinth:{}", pin.project) == change.id && pin.version != change.version
        }) {
            change.action = "update".into();
            change.reason = format!("显式替换根版本 {} → {}", old.version, change.version);
        }
    }
    let selections = report
        .selected
        .iter()
        .filter_map(|candidate| {
            candidate.id.strip_prefix("modrinth:").map(|project| Pin {
                project: project.into(),
                version: candidate.version.clone(),
            })
        })
        .collect();
    Ok(Plan {
        report,
        selections,
        requests,
    })
}

fn platform_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_-".contains(&byte))
}
