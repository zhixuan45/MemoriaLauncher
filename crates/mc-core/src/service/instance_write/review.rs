//! Desktop review is a per-call local approval; it never changes saved MCP permissions.
use super::*;
#[derive(Debug, Clone, Serialize)]
pub struct LocalPlan {
    pub id: String,
    pub hash: String,
    pub kind: WriteKind,
    pub created: i64,
    pub files: usize,
    pub state: String,
}
#[derive(Debug, Clone, Serialize)]
pub struct LocalReview {
    pub plan: LocalPlan,
    pub preview: PlanPreview,
}
fn read(base: &Path, instance: &str, id: &str) -> Result<(Plan, String)> {
    let relative = plan_path(id)?;
    let bytes = files::read_bounded(
        &files::safe_join(base, &format!("{relative}/plan.json"))?,
        files::MAX_TEXT_BYTES,
    )?;
    let hash = digest(&bytes);
    let plan: Plan = serde_json::from_slice(&bytes).map_err(invalid)?;
    if plan.version != 1
        || plan.id != id
        || plan.instance != instance
        || plan.root != fs::canonicalize(base).map_err(io_error)?
        || plan.files.is_empty()
        || plan.files.len() > 64
        || plan.files.iter().map(|f| f.size as u128).sum::<u128>() > files::MAX_PLAN_BYTES as u128
    {
        return Err(invalid("计划身份或文件大小无效"));
    }
    let mut names = std::collections::HashSet::new();
    for file in &plan.files {
        scope(plan.kind, &file.path)?;
        files::safe_join(base, &file.path)?;
        files::basename(&file.stage, ".bin")?;
        if !names.insert(file.path.to_ascii_lowercase())
            || file.sha256.len() != 64
            || !file.sha256.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(invalid("计划文件路径或哈希无效"));
        }
    }
    Ok((plan, hash))
}
fn status(base: &Path, plan: &Plan) -> Result<&'static str> {
    let relative = plan_path(&plan.id)?;
    if files::safe_join(base, &format!("{relative}/cancelled.json"))?.exists() {
        return Ok("cancelled");
    }
    if files::safe_join(base, &format!("{relative}/applied.json"))?.exists()
        || crate::service::file_transaction::committed(base, &plan.id)?
    {
        return Ok("applied");
    }
    if crate::service::file_transaction::rolled_back(base, &plan.id)? {
        return Ok("rolled_back");
    }
    if files::safe_join(base, &format!("{relative}/applying.json"))?.exists() {
        return Ok("interrupted");
    }
    if !chrono::Utc::now()
        .timestamp()
        .checked_sub(plan.created)
        .is_some_and(|age| (-60..=1800).contains(&age))
    {
        return Ok("expired");
    }
    Ok("pending")
}
pub fn list_local_plans(root: &Path, instance: &str) -> Result<Vec<LocalPlan>> {
    let base = files::instance_dir(root, instance)?;
    let directory = files::safe_join(&base, ".launcher/mcp/plans")?;
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(io_error(e)),
    };
    let mut result = vec![];
    for entry in entries {
        let id = entry
            .map_err(io_error)?
            .file_name()
            .to_str()
            .ok_or_else(|| invalid("计划目录名称无效"))?
            .to_owned();
        let relative = plan_path(&id)?;
        if !files::safe_join(&base, &format!("{relative}/plan.json"))?.exists() {
            continue;
        }
        let (plan, hash) = read(&base, instance, &id)?;
        let state = status(&base, &plan)?.into();
        result.push(LocalPlan {
            id,
            hash,
            kind: plan.kind,
            created: plan.created,
            files: plan.files.len(),
            state,
        });
        if result.len() > 512 {
            return Err(invalid("历史计划超过 512，请先归档旧记录"));
        }
    }
    result.sort_by_key(|p| std::cmp::Reverse(p.created));
    Ok(result)
}
pub fn review_local_plan(root: &Path, instance: &str, id: &str) -> Result<LocalReview> {
    let base = files::instance_dir(root, instance)?;
    let (plan, hash) = read(&base, instance, id)?;
    let mut state = status(&base, &plan)?.to_owned();
    let mut warnings = plan.warnings.clone();
    if state == "pending" {
        for watch in &plan.watches {
            if let Err(error) = watch.check(&base) {
                state = "stale".into();
                warnings.push(error.to_string());
            }
        }
    }
    let relative = plan_path(id)?;
    let mut entries = vec![];
    for file in &plan.files {
        if state == "pending"
            && files::file_hash(&files::safe_join(&base, &file.path)?)? != file.old_sha256
        {
            state = "stale".into();
            warnings.push(format!("{} 已变化，须重新生成", file.path));
        }
        let mut text = None;
        if state == "pending" {
            let bytes = files::read_bounded(
                &files::safe_join(&base, &format!("{relative}/{}", file.stage))?,
                files::MAX_MOD_BYTES,
            )?;
            if bytes.len() != file.size || digest(&bytes) != file.sha256 {
                return Err(error("PLAN_HASH_MISMATCH", "计划暂存内容已损坏"));
            }
            text = std::str::from_utf8(&bytes)
                .ok()
                .map(|v| v.chars().take(8192).collect::<String>());
        }
        entries.push(json!({"path":file.path,"old_sha256":file.old_sha256,"sha256":file.sha256,"size":file.size,"text_preview":text,"preview_truncated":text.as_ref().is_some_and(|t|t.len()<file.size)}));
    }
    Ok(LocalReview {
        plan: LocalPlan {
            id: id.into(),
            hash: hash.clone(),
            kind: plan.kind,
            created: plan.created,
            files: plan.files.len(),
            state,
        },
        preview: PlanPreview {
            plan_id: id.into(),
            plan_hash: hash,
            expires_in_seconds: 1800,
            instance_id: instance.into(),
            files: entries,
            warnings,
            details: plan.details,
        },
    })
}
pub fn apply_local_plan(root: &Path, instance: &str, id: &str, hash: &str) -> Result<Value> {
    let review = review_local_plan(root, instance, id)?;
    if review.plan.hash != hash {
        return Err(error("PLAN_HASH_MISMATCH", "审阅后计划内容已变化"));
    }
    if review.plan.state != "pending" {
        return Err(error(
            "PLAN_STALE",
            format!("计划状态为 {}，不可批准", review.plan.state),
        ));
    }
    let mut policy = McpPolicy {
        enabled: true,
        game_root: fs::canonicalize(root).map_err(io_error)?,
        allowed_instances: vec![instance.into()],
        ..Default::default()
    };
    match review.plan.kind {
        WriteKind::Config => policy.permissions.write_configs = true,
        WriteKind::ModFile => policy.permissions.write_mod_files = true,
        WriteKind::ModList => policy.permissions.write_mod_list = true,
        WriteKind::Quest => policy.permissions.write_quests = true,
    }
    apply(&policy, instance, id, hash)
}
pub fn cancel_local_plan(root: &Path, instance: &str, id: &str, hash: &str) -> Result<()> {
    let base = files::instance_dir(root, instance)?;
    let _lock = files::lock(&base)?;
    let (plan, actual) = read(&base, instance, id)?;
    if hash != actual {
        return Err(error("PLAN_HASH_MISMATCH", "计划内容已变化"));
    }
    if status(&base, &plan)? == "applied" {
        return Err(error("PLAN_STALE", "已提交计划不能通过取消撤销"));
    }
    let path = files::safe_join(&base, &format!("{}/cancelled.json", plan_path(id)?))?;
    if !path.exists() {
        files::write_new(
            &path,
            &serde_json::to_vec(&json!({"id":id,"hash":hash})).map_err(invalid)?,
        )?;
    }
    Ok(())
}
