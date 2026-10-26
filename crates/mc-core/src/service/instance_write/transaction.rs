use super::{
    authorize, digest, error,
    files::{self, Watch},
    ftb, invalid, io_error, mods, Change, PreparedChange, Result, WriteKind,
};
use crate::service::mcp_policy::McpPolicy;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    version: u32,
    id: String,
    root: PathBuf,
    instance: String,
    kind: WriteKind,
    created: i64,
    files: Vec<PlannedFile>,
    watches: Vec<Watch>,
    warnings: Vec<String>,
    details: Value,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PlannedFile {
    path: String,
    old_sha256: Option<String>,
    sha256: String,
    size: usize,
    stage: String,
}
#[derive(Debug, Serialize)]
pub struct PlanPreview {
    pub plan_id: String,
    pub plan_hash: String,
    pub expires_in_seconds: u32,
    pub instance_id: String,
    pub files: Vec<Value>,
    pub warnings: Vec<String>,
    pub details: Value,
}
pub fn decode(data: &str, limit: usize) -> Result<Vec<u8>> {
    if data.len() > limit.div_ceil(3) * 4 {
        return Err(invalid("Base64 文件超过大小限制"));
    }
    let bytes = STANDARD
        .decode(data)
        .map_err(|_| invalid("文件内容不是有效 Base64"))?;
    if bytes.is_empty() || bytes.len() > limit {
        return Err(invalid("文件为空或过大"));
    }
    Ok(bytes)
}
async fn compile(
    policy: &McpPolicy,
    instance: &str,
    base: &Path,
    change: &Change,
) -> Result<PreparedChange> {
    match change {
        Change::Config { path, content } => {
            files::config_path(path)?;
            if content.len() > files::MAX_TEXT_BYTES || content.contains('\0') {
                return Err(invalid("配置内容须为不含空字符、最多 1 MiB 的 UTF-8 文本"));
            }
            match Path::new(path)
                .extension()
                .and_then(|v| v.to_str())
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str()
            {
                "json" => {
                    serde_json::from_str::<Value>(content)
                        .map_err(|e| error("INVALID_CONFIG", e))?;
                }
                "toml" => {
                    content
                        .parse::<toml_edit::DocumentMut>()
                        .map_err(|e| error("INVALID_CONFIG", e))?;
                }
                "snbt" => {
                    super::snbt::parse(content).map_err(|e| error("INVALID_CONFIG", e))?;
                }
                _ => {}
            }
            let mut result = PreparedChange::single(path.clone(), content.as_bytes().to_vec());
            result.warnings.push("只检查 JSON/TOML/SNBT 语法；其他配置仅检查路径、UTF-8 和大小。具体配置键仍需由对应 Mod 校验。".into());
            Ok(result)
        }
        Change::ModFile {
            filename,
            data_base64,
            sha256,
        } => {
            files::basename(filename, ".jar")?;
            let bytes = decode(data_base64, files::MAX_MOD_BYTES)?;
            if sha256.len() != 64
                || !sha256.bytes().all(|b| b.is_ascii_hexdigit())
                || digest(&bytes) != sha256.to_ascii_lowercase()
            {
                return Err(error("FILE_HASH_MISMATCH", "上传 JAR 的 SHA-256 不匹配"));
            }
            super::validate_jar(&bytes)?;
            let mut result = PreparedChange::single(format!("mods/{filename}"), bytes);
            result.warnings.push(
                "已检查 JAR 结构、Mod 元信息与 SHA-256；直接上传不保证加载器、游戏版本或依赖兼容。"
                    .into(),
            );
            Ok(result)
        }
        Change::ModList { mods: references } => {
            mods::prepare(base, &policy.game_root, instance, references).await
        }
        Change::QuestEntry {
            chapter,
            chapter_title,
            entry,
        } => ftb::entry(base, chapter, chapter_title.as_deref(), entry),
        Change::QuestImage {
            chapter,
            filename,
            data_base64,
            position,
            width,
            height,
        } => ftb::image(
            base,
            chapter,
            filename,
            decode(data_base64, 4 * 1024 * 1024)?,
            *position,
            *width,
            *height,
        ),
    }
}
fn scope(kind: WriteKind, path: &str) -> Result<()> {
    let allowed = match kind {
        WriteKind::Config => return files::config_path(path),
        WriteKind::ModFile | WriteKind::ModList => {
            path.strip_prefix("mods/")
                .is_some_and(|name| files::basename(name, ".jar").is_ok())
                || (kind == WriteKind::ModList && path == ".launcher/mod-list.json")
        }
        WriteKind::Quest => {
            path.strip_prefix("config/ftbquests/quests/chapters/")
                .is_some_and(|n| files::basename(n, ".snbt").is_ok())
                || path
                    .strip_prefix("config/ftbquests/quests/images/")
                    .is_some_and(|n| {
                        [".png", ".jpg", ".jpeg"]
                            .iter()
                            .any(|ext| files::basename(n, ext).is_ok())
                    })
        }
    };
    if !allowed {
        return Err(error("PERMISSION_DENIED", "计划文件超出授权类别的路径范围"));
    }
    Ok(())
}
fn plan_path(id: &str) -> Result<String> {
    let parsed = uuid::Uuid::parse_str(id).map_err(|_| invalid("计划 ID 无效"))?;
    if parsed.to_string() != id {
        return Err(invalid("计划 ID 不是标准 UUID"));
    }
    Ok(format!(".launcher/mcp/plans/{id}"))
}
pub async fn prepare(policy: &McpPolicy, instance: &str, change: &Change) -> Result<PlanPreview> {
    authorize(policy, instance, change.kind())?;
    let base = files::instance_dir(&policy.game_root, instance)?;
    let _lock = files::lock(&base)?;
    // Observe instance metadata before resolving remote files.
    let name = base
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(|| invalid("实例名称无效"))?;
    let metadata = Watch::file(&base, &format!("{name}.json"))?;
    let mut result = compile(policy, instance, &base, change).await?;
    metadata.check(&base)?;
    result.watches.push(metadata);
    if result.files.is_empty()
        || result.files.len() > 64
        || result.files.iter().map(|f| f.bytes.len()).sum::<usize>() > files::MAX_PLAN_BYTES
    {
        return Err(invalid("写入计划为空或超出 64 个文件/128 MiB"));
    }
    reserve_cache(&base, result.files.iter().map(|f| f.bytes.len()).sum())?;
    let mut names = std::collections::BTreeSet::new();
    for file in &result.files {
        scope(change.kind(), &file.path)?;
        files::safe_join(&base, &file.path)?;
        if !names.insert(file.path.to_ascii_lowercase()) {
            return Err(invalid("计划包含重复文件"));
        }
    }
    for watch in &result.watches {
        watch.check(&base)?;
    }
    let id = uuid::Uuid::new_v4().to_string();
    let relative = plan_path(&id)?;
    files::create_dirs(&base, &relative)?;
    let mut planned = vec![];
    let mut preview = vec![];
    for (index, file) in result.files.iter().enumerate() {
        let old = files::file_hash(&files::safe_join(&base, &file.path)?)?;
        let hash = digest(&file.bytes);
        let stage = format!("{index}.bin");
        files::write_new(
            &files::safe_join(&base, &format!("{relative}/{stage}"))?,
            &file.bytes,
        )?;
        let text = std::str::from_utf8(&file.bytes)
            .ok()
            .map(|v| v.chars().take(8192).collect::<String>());
        preview.push(json!({"path":file.path,"old_sha256":old,"sha256":hash,"size":file.bytes.len(),"text_preview":text,"preview_truncated":text.as_ref().is_some_and(|t|t.len()<file.bytes.len())}));
        planned.push(PlannedFile {
            path: file.path.clone(),
            old_sha256: old,
            sha256: hash,
            size: file.bytes.len(),
            stage,
        });
    }
    let plan = Plan {
        version: 1,
        id: id.clone(),
        root: fs::canonicalize(&base).map_err(io_error)?,
        instance: instance.into(),
        kind: change.kind(),
        created: chrono::Utc::now().timestamp(),
        files: planned,
        watches: result.watches,
        warnings: result.warnings.clone(),
        details: result.details.clone(),
    };
    let bytes = serde_json::to_vec(&plan).map_err(invalid)?;
    if bytes.len() > files::MAX_TEXT_BYTES {
        return Err(invalid("计划元信息超过 1 MiB，请减少来源条目数量"));
    }
    let hash = digest(&bytes);
    files::write_new(
        &files::safe_join(&base, &format!("{relative}/plan.json"))?,
        &bytes,
    )?;
    Ok(PlanPreview {
        plan_id: id,
        plan_hash: hash,
        expires_in_seconds: 1800,
        instance_id: instance.into(),
        files: preview,
        warnings: result.warnings,
        details: result.details,
    })
}
pub fn apply(policy: &McpPolicy, instance: &str, id: &str, hash: &str) -> Result<Value> {
    if !policy.enabled || !policy.allows_instance(instance) {
        return Err(error("PERMISSION_DENIED", "实例未获授权"));
    }
    let base = files::instance_dir(&policy.game_root, instance)?;
    let _lock = files::lock(&base)?;
    let relative = plan_path(id)?;
    let bytes = files::read_bounded(
        &files::safe_join(&base, &format!("{relative}/plan.json"))?,
        files::MAX_TEXT_BYTES,
    )?;
    if digest(&bytes) != hash {
        return Err(error(
            "PLAN_HASH_MISMATCH",
            "必须提供预览返回的准确计划哈希",
        ));
    }
    let plan: Plan = serde_json::from_slice(&bytes).map_err(invalid)?;
    authorize(policy, instance, plan.kind)?;
    if plan.version != 1
        || plan.id != id
        || plan.instance != instance
        || plan.root != fs::canonicalize(&base).map_err(io_error)?
    {
        return Err(error("PLAN_STALE", "计划不属于当前实例目录"));
    }
    if plan.files.is_empty()
        || plan.files.len() > 64
        || plan.files.iter().map(|f| f.size as u128).sum::<u128>() > files::MAX_PLAN_BYTES as u128
    {
        return Err(invalid("计划大小无效"));
    }
    for file in &plan.files {
        scope(plan.kind, &file.path)?;
        files::safe_join(&base, &file.path)?;
        files::basename(&file.stage, ".bin")?;
    }
    let receipt = files::safe_join(&base, &format!("{relative}/applied.json"))?;
    if receipt.exists() {
        for file in &plan.files {
            if files::file_hash(&files::safe_join(&base, &file.path)?)?.as_deref()
                != Some(&file.sha256)
            {
                return Err(error("PLAN_STALE", "已应用文件之后发生变化"));
            }
        }
        return Ok(json!({"plan_id":id,"status":"already_applied"}));
    }
    if !chrono::Utc::now()
        .timestamp()
        .checked_sub(plan.created)
        .is_some_and(|age| (-60..=1800).contains(&age))
    {
        return Err(error("PLAN_EXPIRED", "计划已过期，请重新预览"));
    }
    if files::safe_join(&base, &format!("{relative}/applying.json"))?.exists() {
        return Err(error(
            "RECOVERY_REQUIRED",
            "上次应用中断，请检查备份后重新生成计划",
        ));
    }
    for watch in &plan.watches {
        watch.check(&base)?;
    }
    let mut data = vec![];
    for file in &plan.files {
        let path = files::safe_join(&base, &file.path)?;
        if files::file_hash(&path)? != file.old_sha256 {
            return Err(error(
                "PLAN_STALE",
                format!("{} 已变化，请重新预览", file.path),
            ));
        }
        let bytes = files::read_bounded(
            &files::safe_join(&base, &format!("{relative}/{}", file.stage))?,
            files::MAX_MOD_BYTES,
        )?;
        if bytes.len() != file.size || digest(&bytes) != file.sha256 {
            return Err(error("PLAN_HASH_MISMATCH", "暂存文件已损坏或被更改"));
        }
        data.push(bytes);
    }
    let backup = format!(".launcher/mcp/backups/{id}");
    for file in &plan.files {
        if file.old_sha256.is_some() {
            let target = format!("{backup}/{}", file.path);
            ensure_parent(&base, &target)?;
            let original =
                files::read_bounded(&files::safe_join(&base, &file.path)?, 64 * 1024 * 1024)?;
            if Some(digest(&original)) != file.old_sha256 {
                return Err(error("PLAN_STALE", "备份时检测到文件变化"));
            }
            files::write_new(&files::safe_join(&base, &target)?, &original)?;
        }
    }
    files::write_new(
        &files::safe_join(&base, &format!("{relative}/applying.json"))?,
        &serde_json::to_vec(
            &json!({"plan_hash":hash,"backup":backup,"created":chrono::Utc::now()}),
        )
        .map_err(invalid)?,
    )?;
    let mut changed = vec![];
    let result = (|| -> Result<()> {
        for (file, bytes) in plan.files.iter().zip(&data) {
            // Last check immediately before replacing each file; the lock serializes Memoria writers.
            if files::file_hash(&files::safe_join(&base, &file.path)?)? != file.old_sha256 {
                return Err(error("PLAN_STALE", "应用时检测到并发修改"));
            }
            replace(&base, &file.path, bytes)?;
            changed.push(file);
        }
        Ok(())
    })();
    if let Err(cause) = result {
        let mut failures = vec![];
        for file in changed.iter().rev() {
            let rollback = if file.old_sha256.is_some() {
                files::read_bounded(
                    &files::safe_join(&base, &format!("{backup}/{}", file.path))?,
                    64 * 1024 * 1024,
                )
                .and_then(|bytes| replace(&base, &file.path, &bytes))
            } else {
                files::safe_join(&base, &file.path)
                    .and_then(|p| fs::remove_file(p).map_err(io_error))
            };
            if let Err(e) = rollback {
                failures.push(e.to_string());
            }
        }
        return Err(error(
            "APPLY_FAILED",
            format!(
                "{cause}；已回滚可恢复文件；备份：{backup}；回滚错误：{}",
                failures.join("; ")
            ),
        ));
    }
    let report = json!({"plan_id":id,"plan_hash":hash,"status":"applied","backup_directory":backup,"files":plan.files.iter().map(|f|json!({"path":f.path,"sha256":f.sha256})).collect::<Vec<_>>(),"applied_at":chrono::Utc::now()});
    files::write_new(
        &receipt,
        &serde_json::to_vec_pretty(&report).map_err(invalid)?,
    )?;
    files::create_dirs(&base, &backup)?;
    files::write_new(
        &files::safe_join(&base, &format!("{backup}/receipt.json"))?,
        &serde_json::to_vec_pretty(&report).map_err(invalid)?,
    )?;
    for file in &plan.files {
        let _ = fs::remove_file(files::safe_join(
            &base,
            &format!("{relative}/{}", file.stage),
        )?);
    }
    Ok(report)
}
// Bound pending uploads, reclaim expired previews, and retain receipts/backups separately.
fn reserve_cache(base: &Path, incoming: usize) -> Result<()> {
    let directory = files::create_dirs(base, ".launcher/mcp/plans")?;
    let mut bytes = 0u64;
    let mut pending = 0;
    for entry in fs::read_dir(&directory).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let id = entry
            .file_name()
            .to_str()
            .ok_or_else(|| invalid("计划缓存名称无效"))?
            .to_owned();
        let relative = plan_path(&id)?;
        let path = files::safe_join(base, &relative)?;
        let applied = files::safe_join(base, &format!("{relative}/applied.json"))?.exists();
        let applying = files::safe_join(base, &format!("{relative}/applying.json"))?.exists();
        let old = entry
            .metadata()
            .map_err(io_error)?
            .modified()
            .map_err(io_error)?
            .elapsed()
            .is_ok_and(|age| age.as_secs() > 1800);
        let clear = applied || (old && !applying);
        let mut children = vec![];
        for child in fs::read_dir(&path).map_err(io_error)? {
            let child = child.map_err(io_error)?;
            let name = child
                .file_name()
                .to_str()
                .ok_or_else(|| invalid("计划缓存文件名无效"))?
                .to_owned();
            let absolute = files::safe_join(base, &format!("{relative}/{name}"))?;
            let meta = fs::metadata(&absolute).map_err(io_error)?;
            if !meta.is_file() {
                return Err(invalid("计划缓存包含异常目录"));
            }
            children.push((name, absolute, meta.len()));
        }
        for (name, absolute, size) in &children {
            if name.ends_with(".bin") && clear {
                fs::remove_file(absolute).map_err(io_error)?;
            } else if name.ends_with(".bin") {
                bytes = bytes.saturating_add(*size);
            }
        }
        if !applied && !(old && !applying) {
            pending += 1;
        }
        if old
            && !applying
            && children
                .iter()
                .all(|(name, _, _)| name == "plan.json" || name.ends_with(".bin"))
        {
            for (name, absolute, _) in children {
                if name == "plan.json" {
                    fs::remove_file(absolute).map_err(io_error)?;
                }
            }
            fs::remove_dir(path).map_err(io_error)?;
        }
    }
    if pending >= 16 || bytes.saturating_add(incoming as u64) > 256 * 1024 * 1024 {
        return Err(error(
            "PLAN_CACHE_FULL",
            "实例待应用计划超过 16 个或 256 MiB；请应用计划或等待预览过期。中断计划请先检查备份",
        ));
    }
    Ok(())
}
fn ensure_parent(base: &Path, path: &str) -> Result<()> {
    if let Some((parent, _)) = path.rsplit_once('/') {
        files::create_dirs(base, parent)?;
    }
    Ok(())
}
fn replace(base: &Path, path: &str, bytes: &[u8]) -> Result<()> {
    ensure_parent(base, path)?;
    let target = files::safe_join(base, path)?;
    let temporary = target.with_extension(format!("{}.mcp-tmp", uuid::Uuid::new_v4()));
    files::write_new(&temporary, bytes)?;
    if let Err(e) = fs::rename(&temporary, &target) {
        let _ = fs::remove_file(&temporary);
        return Err(io_error(e));
    }
    Ok(())
}
