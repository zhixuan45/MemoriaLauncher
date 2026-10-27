use super::*;
use crate::service::mod_resolver::{self as resolver, remote::Pin, Status};
pub(super) async fn resolve_remote(
    request: &Request,
    catalog: &ResourceCatalog,
    base: &std::path::Path,
    records: &[Value],
    game: &str,
    loader: &str,
) -> Result<(VecDeque<(String, String)>, Vec<String>)> {
    if request.kind != ResourceKind::Mod {
        return Ok((
            VecDeque::from([(request.project.clone(), request.version.clone())]),
            vec![],
        ));
    }
    let mut installed = vec![];
    for record in records {
        let Some(path) = record["path"]
            .as_str()
            .filter(|path| path.starts_with("mods/"))
        else {
            continue;
        };
        let Some(hash) =
            files::file_hash(&files::safe_join(base, path).map_err(error)?).map_err(error)?
        else {
            continue;
        };
        if record["sha256"].as_str() != Some(hash.as_str()) {
            return Err(format!(
                "已有资源 {} 的来源记录与实际内容不一致，请先检查",
                path
            ));
        }
        if let (Some(project), Some(version)) =
            (record["project"].as_str(), record["version"].as_str())
        {
            installed.push(Pin {
                project: project.into(),
                version: version.into(),
            });
        }
    }
    let plan = resolver::remote::resolve(
        catalog,
        &[Pin {
            project: request.project.clone(),
            version: request.version.clone(),
        }],
        &installed,
        game,
        loader,
    )
    .await?;
    if matches!(plan.report.status, Status::Conflict | Status::LimitReached)
        || plan.selections.is_empty()
    {
        return Err(format!(
            "依赖求解 {:?}：{}",
            plan.report.status,
            plan.report
                .issues
                .iter()
                .chain(&plan.report.warnings)
                .map(|issue| format!(
                    "{} {} {}",
                    issue.subject,
                    issue.message,
                    issue.chain.join("；")
                ))
                .take(8)
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    let queue = plan
        .selections
        .into_iter()
        .filter(|pin| {
            pin.project == request.project
                || !installed
                    .iter()
                    .any(|old| old.project == pin.project && old.version == pin.version)
        })
        .map(|pin| (pin.project, pin.version))
        .collect();
    let mut warnings: Vec<_> = plan
        .report
        .warnings
        .iter()
        .map(|issue| format!("{}：{}", issue.subject, issue.message))
        .collect();
    if plan.report.status == Status::Unknown {
        warnings.push("部分远程依赖元数据不明确，需结合下方本地 JAR 检查人工确认。".into());
    }
    if let Ok(entries) = fs::read_dir(base.join("mods")) {
        if entries.filter_map(|entry| entry.ok()).any(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.to_ascii_lowercase().ends_with(".jar")
                && !records
                    .iter()
                    .any(|record| record["path"] == format!("mods/{name}"))
        }) {
            warnings.push("存在未绑定来源项目的本地 JAR：会检查其本地前置声明，但无法据此判断源站项目 ID 之间的冲突。".into());
        }
    }
    Ok((queue, warnings))
}

pub(super) async fn audit_preview(
    request: &Request,
    client: &DownloadClient,
    output: &mut [InstallFile],
    watches: &mut Vec<files::Watch>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    if !output.iter().any(|file| file.path.starts_with("mods/")) {
        return Ok(());
    }
    let mut inventory =
        resolver::inventory::inspect_instance(&request.root, &request.instance).await?;
    for (name, hash) in &inventory.files {
        watches.push(files::Watch {
            path: format!("mods/{name}"),
            directory: false,
            hash: Some(hash.clone()),
        });
    }
    let replaced: std::collections::HashSet<_> = output
        .iter()
        .filter_map(|file| file.path.strip_prefix("mods/"))
        .collect();
    inventory.candidates.retain(|candidate| {
        !replaced.contains(if candidate.origin.is_empty() {
            candidate.source.as_str()
        } else {
            candidate.origin.as_str()
        })
    });
    inventory.roots.retain(|root| {
        inventory.candidates.iter().any(|c| {
            c.id == root.id
                && resolver::version::matches(&c.version, &root.constraint)
                    != resolver::version::Match::No
                && root.reason == format!("已启用文件 {}", c.source)
        })
    });
    inventory
        .issues
        .retain(|issue| !replaced.contains(issue.subject.as_str()));
    inventory
        .warnings
        .retain(|issue| !replaced.contains(issue.subject.as_str()));
    for file in output
        .iter_mut()
        .filter(|file| file.path.starts_with("mods/") && !file.remove)
    {
        let bytes = download(client, file).await?;
        super::super::instance_write::validate_jar(&bytes).map_err(error)?;
        inventory.add_jar(&bytes, file.path.strip_prefix("mods/").unwrap(), true);
        file.inline = Some(bytes);
    }
    let report = inventory.audit();
    if matches!(report.status, Status::Conflict | Status::LimitReached) {
        return Err(format!(
            "安装后的 Mod 前置/冲突检查未通过：{}",
            report
                .issues
                .iter()
                .map(|issue| format!(
                    "{}：{}（{}）",
                    issue.subject,
                    issue.message,
                    issue.chain.join("；")
                ))
                .take(12)
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    warnings.extend(
        report
            .warnings
            .iter()
            .map(|issue| format!("JAR 检查 {}：{}", issue.subject, issue.message)),
    );
    if report.status == Status::Unknown {
        warnings.push("安装后组合存在未知约束或元数据，不能保证运行兼容。".into());
    }
    Ok(())
}
