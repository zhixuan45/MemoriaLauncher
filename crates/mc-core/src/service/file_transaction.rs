//! Recoverable file commits shared by desktop installs and scoped MCP writes.
//! Callers hold the instance lock; recovery only rolls back identifiable interrupted writes.
use super::instance_write::{digest, error, files, invalid, io_error, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

const DIRECTORY: &str = ".memoria/transactions";
#[derive(Debug, Clone)]
pub struct Operation {
    pub path: String,
    pub old_sha256: Option<String>,
    /// An already verified, instance-relative staged file; None removes the target.
    pub staged: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    path: String,
    before: Option<String>,
    after: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u32,
    id: String,
    root: PathBuf,
    origin: String,
    created: i64,
    entries: Vec<Entry>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub id: String,
    pub origin: String,
    pub state: String,
    pub files: usize,
    pub created: i64,
}
#[derive(Debug, Default, Clone, Serialize)]
pub struct Recovery {
    pub rolled_back: Vec<String>,
}
fn relative(id: &str) -> Result<String> {
    let uuid = uuid::Uuid::parse_str(id).map_err(invalid)?;
    if uuid.to_string() != id {
        return Err(invalid("事务 ID 无效"));
    }
    Ok(format!("{DIRECTORY}/{id}"))
}
fn content_path(base: &Path, path: &str) -> Result<()> {
    files::safe_join(base, path)?;
    if ![
        "mods/",
        "resourcepacks/",
        "shaderpacks/",
        "config/",
        "defaultconfigs/",
        "kubejs/",
        "scripts/",
    ]
    .iter()
    .any(|prefix| path.starts_with(prefix))
        && ![
            "options.txt",
            "servers.dat",
            ".memoria/resource-install.json",
            ".launcher/mod-list.json",
        ]
        .contains(&path)
    {
        return Err(invalid("事务仅允许游戏内容及资源来源清单"));
    }
    Ok(())
}
fn valid_hash(hash: &Option<String>) -> bool {
    hash.as_ref()
        .is_none_or(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn load(base: &Path, id: &str) -> Result<Journal> {
    let rel = relative(id)?;
    let bytes = files::read_bounded(
        &files::safe_join(base, &format!("{rel}/journal.json"))?,
        2 * 1024 * 1024,
    )?;
    let journal: Journal = serde_json::from_slice(&bytes).map_err(invalid)?;
    if journal.version != 1
        || journal.id != id
        || journal.root != fs::canonicalize(base).map_err(io_error)?
        || journal.entries.is_empty()
        || journal.entries.len() > 4096
        || journal.origin.len() > 240
    {
        return Err(invalid("事务记录身份或大小无效，需人工检查"));
    }
    let mut names = HashSet::new();
    for entry in &journal.entries {
        content_path(base, &entry.path)?;
        if !names.insert(entry.path.to_ascii_lowercase())
            || !valid_hash(&entry.before)
            || !valid_hash(&entry.after)
        {
            return Err(invalid("事务含重复路径或无效哈希"));
        }
    }
    Ok(journal)
}
fn marker(base: &Path, id: &str, name: &str) -> Result<bool> {
    let path = files::safe_join(base, &format!("{}/{name}.json", relative(id)?))?;
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(io_error(e)),
        Ok(_) => {
            let bytes = files::read_bounded(&path, 4096)?;
            let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(invalid)?;
            if value["id"] != id || value["state"] != name {
                return Err(invalid("事务完成标记损坏"));
            }
            Ok(true)
        }
    }
}
fn mark(base: &Path, id: &str, name: &str) -> Result<()> {
    let path = files::safe_join(base, &format!("{}/{name}.json", relative(id)?))?;
    if marker(base, id, name)? {
        return Ok(());
    }
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    files::write_new(
        &temporary,
        &serde_json::to_vec(&serde_json::json!({"id":id,"state":name})).map_err(invalid)?,
    )?;
    fs::rename(&temporary, &path).map_err(io_error)?;
    sync_parent(&path)
}
fn sync_parent(path: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::File::open(path.parent().ok_or_else(|| invalid("事务文件无父目录"))?)
        .and_then(|file| file.sync_all())
        .map_err(io_error)?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
fn parent(base: &Path, path: &str) -> Result<()> {
    if let Some((parent, _)) = path.rsplit_once('/') {
        files::create_dirs(base, parent)?;
    }
    Ok(())
}
fn replace(
    base: &Path,
    path: &str,
    source: Option<&str>,
    expected: &Option<String>,
    id: &str,
) -> Result<()> {
    let target = files::safe_join(base, path)?;
    parent(base, path)?;
    if let Some(source) = source {
        let bytes = files::read_bounded(&files::safe_join(base, source)?, 64 * 1024 * 1024)?;
        let publish = files::create_dirs(base, &format!("{}/publish", relative(id)?))?;
        let temporary = publish.join(uuid::Uuid::new_v4().to_string());
        files::write_new(&temporary, &bytes)?;
        if files::file_hash(&target)? != *expected {
            return Err(error("PLAN_STALE", format!("{path} 已被外部修改")));
        }
        fs::rename(&temporary, &target).map_err(io_error)?;
    } else {
        if files::file_hash(&target)? != *expected {
            return Err(error("PLAN_STALE", format!("{path} 已被外部修改")));
        }
        if expected.is_some() {
            fs::remove_file(&target).map_err(io_error)?;
        }
    }
    sync_parent(&target)
}
pub fn committed(base: &Path, id: &str) -> Result<bool> {
    marker(base, id, "committed")
}
pub fn rolled_back(base: &Path, id: &str) -> Result<bool> {
    marker(base, id, "rolled_back")
}
pub fn execute_locked(
    base: &Path,
    id: &str,
    origin: &str,
    operations: &[Operation],
) -> Result<Summary> {
    execute_with(base, id, origin, operations, None)
}
pub(crate) fn execute_with(
    base: &Path,
    id: &str,
    origin: &str,
    operations: &[Operation],
    fail_at: Option<usize>,
) -> Result<Summary> {
    recover_locked(base)?;
    if operations.is_empty() || operations.len() > 4096 || origin.len() > 240 {
        return Err(invalid("事务数量无效"));
    }
    let rel = relative(id)?;
    if files::safe_join(base, &rel)?.exists() {
        return Err(error("PLAN_STALE", "事务 ID 已使用，请重新生成计划"));
    }
    files::create_dirs(base, &format!("{rel}/before"))?;
    files::create_dirs(base, &format!("{rel}/after"))?;
    let mut entries = vec![];
    let mut names = HashSet::new();
    let mut total = 0usize;
    for (index, op) in operations.iter().enumerate() {
        content_path(base, &op.path)?;
        if !names.insert(op.path.to_ascii_lowercase()) {
            return Err(invalid("事务含重复路径"));
        }
        let actual = files::file_hash(&files::safe_join(base, &op.path)?)?;
        if actual != op.old_sha256 {
            return Err(error("PLAN_STALE", format!("{} 已变化", op.path)));
        }
        if actual.is_some() {
            let bytes = files::read_bounded(&files::safe_join(base, &op.path)?, 64 * 1024 * 1024)?;
            if Some(digest(&bytes)) != actual {
                return Err(error("PLAN_STALE", "备份时内容变化"));
            }
            total = total.saturating_add(bytes.len());
            if total > 2 * 1024 * 1024 * 1024 {
                return Err(invalid("事务新旧文件合计超过 2 GiB"));
            }
            files::write_new(
                &files::safe_join(base, &format!("{rel}/before/{index}"))?,
                &bytes,
            )?;
        }
        let after = if let Some(source) = &op.staged {
            let bytes = files::read_bounded(&files::safe_join(base, source)?, 64 * 1024 * 1024)?;
            total = total.saturating_add(bytes.len());
            if total > 2 * 1024 * 1024 * 1024 {
                return Err(invalid("事务新旧文件合计超过 2 GiB"));
            }
            files::write_new(
                &files::safe_join(base, &format!("{rel}/after/{index}"))?,
                &bytes,
            )?;
            Some(digest(&bytes))
        } else {
            None
        };
        entries.push(Entry {
            path: op.path.clone(),
            before: actual,
            after,
        });
    }
    let journal = Journal {
        version: 1,
        id: id.into(),
        root: fs::canonicalize(base).map_err(io_error)?,
        origin: origin.into(),
        created: chrono::Utc::now().timestamp(),
        entries,
    };
    let path = files::safe_join(base, &format!("{rel}/journal.json"))?;
    // Publish the complete journal atomically; an incomplete staging directory never touched targets.
    let temporary = files::safe_join(base, &format!("{rel}/journal.tmp"))?;
    let journal_bytes = serde_json::to_vec(&journal).map_err(invalid)?;
    if journal_bytes.len() > 2 * 1024 * 1024 {
        return Err(invalid("事务记录超过 2 MiB，请减少目标数量或路径长度"));
    }
    files::write_new(&temporary, &journal_bytes)?;
    fs::rename(temporary, &path).map_err(io_error)?;
    sync_parent(&path)?;
    let result = (|| -> Result<()> {
        for (index, entry) in journal.entries.iter().enumerate() {
            if fail_at == Some(index) {
                return Err(error("WRITE_IO_ERROR", "fixture commit failure"));
            }
            let source = entry.after.as_ref().map(|_| format!("{rel}/after/{index}"));
            replace(base, &entry.path, source.as_deref(), &entry.before, id)?;
            #[cfg(test)]
            tests::crash_point(base, index);
        }
        mark(base, id, "committed")?;
        #[cfg(test)]
        tests::crash_point(base, journal.entries.len());
        Ok(())
    })();
    if let Err(cause) = result {
        if committed(base, id)? {
            return Err(error(
                "WRITE_IO_ERROR",
                format!("事务已提交，完成标记同步失败：{cause}"),
            ));
        }
        return match rollback(base, &journal) {
            Ok(()) => Err(error(
                "APPLY_FAILED",
                format!("{cause}；已回滚；备份：{rel}/before"),
            )),
            Err(restore) => Err(error(
                "RECOVERY_REQUIRED",
                format!("{cause}；{restore}；保留现场：{rel}"),
            )),
        };
    }
    Ok(summary(&journal, "committed"))
}
fn rollback(base: &Path, journal: &Journal) -> Result<()> {
    let rel = relative(&journal.id)?;
    // Validate the entire recovery set before changing any target.
    for (index, entry) in journal.entries.iter().enumerate() {
        if let Some(hash) = &entry.before {
            if files::file_hash(&files::safe_join(base, &format!("{rel}/before/{index}"))?)?
                .as_ref()
                != Some(hash)
            {
                return Err(error(
                    "RECOVERY_REQUIRED",
                    format!("{} 的备份缺失或损坏", entry.path),
                ));
            }
        }
        let current = files::file_hash(&files::safe_join(base, &entry.path)?)?;
        if current != entry.before && current != entry.after {
            return Err(error(
                "RECOVERY_REQUIRED",
                format!("{} 已有外部修改，未覆盖", entry.path),
            ));
        }
    }
    for (index, entry) in journal.entries.iter().enumerate().rev() {
        let current = files::file_hash(&files::safe_join(base, &entry.path)?)?;
        if current == entry.before {
            continue;
        }
        if current != entry.after {
            return Err(error("RECOVERY_REQUIRED", "恢复期间检测到外部修改"));
        }
        let source = entry
            .before
            .as_ref()
            .map(|_| format!("{rel}/before/{index}"));
        replace(
            base,
            &entry.path,
            source.as_deref(),
            &entry.after,
            &journal.id,
        )?;
    }
    mark(base, &journal.id, "rolled_back")
}
fn ids(base: &Path) -> Result<Vec<String>> {
    let directory = files::safe_join(base, DIRECTORY)?;
    let entries = match fs::read_dir(directory) {
        Ok(v) => v,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(io_error(e)),
    };
    let mut ids = vec![];
    for entry in entries {
        let id = entry
            .map_err(io_error)?
            .file_name()
            .to_str()
            .ok_or_else(|| invalid("事务目录名称无效"))?
            .to_owned();
        let rel = relative(&id)?;
        // Incomplete preparation has no published journal and needs no rollback.
        if files::safe_join(base, &format!("{rel}/journal.json"))?.exists() {
            ids.push(id);
        }
        if ids.len() > 512 {
            return Err(error(
                "RECOVERY_REQUIRED",
                "事务记录超过 512，需归档历史记录",
            ));
        }
    }
    ids.sort();
    Ok(ids)
}
pub fn recover_locked(base: &Path) -> Result<Recovery> {
    let mut report = Recovery::default();
    for id in ids(base)? {
        if marker(base, &id, "committed")? || marker(base, &id, "rolled_back")? {
            continue;
        }
        let journal = load(base, &id)?;
        rollback(base, &journal)?;
        report.rolled_back.push(id);
    }
    Ok(report)
}
pub fn recover(root: &Path, instance: &str) -> Result<Recovery> {
    let base = files::instance_dir(root, instance)?;
    if !files::safe_join(&base, DIRECTORY)?.exists() {
        return Ok(Recovery::default());
    }
    let pending = ids(&base)?
        .into_iter()
        .map(|id| Ok(!marker(&base, &id, "committed")? && !marker(&base, &id, "rolled_back")?))
        .collect::<Result<Vec<_>>>()?;
    if !pending.into_iter().any(|pending| pending) {
        return Ok(Recovery::default());
    }
    let _lock = files::lock_raw(&base)?;
    recover_locked(&base)
}
fn summary(journal: &Journal, state: &str) -> Summary {
    Summary {
        id: journal.id.clone(),
        origin: journal.origin.clone(),
        state: state.into(),
        files: journal.entries.len(),
        created: journal.created,
    }
}
pub fn history(root: &Path, instance: &str) -> Result<Vec<Summary>> {
    let base = files::instance_dir(root, instance)?;
    let mut result = vec![];
    for id in ids(&base)? {
        let journal = load(&base, &id)?;
        let state = if marker(&base, &id, "committed")? {
            "committed"
        } else if marker(&base, &id, "rolled_back")? {
            "rolled_back"
        } else {
            "interrupted"
        };
        result.push(summary(&journal, state));
    }
    result.sort_by_key(|value| std::cmp::Reverse(value.created));
    Ok(result)
}

#[cfg(test)]
#[path = "file_transaction_tests.rs"]
mod tests;
