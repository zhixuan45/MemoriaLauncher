use super::{digest, error, invalid, io_error, Result};
use crate::GameLayout;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

pub const MAX_MOD_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_TEXT_BYTES: usize = 1024 * 1024;
pub const MAX_PLAN_BYTES: usize = 128 * 1024 * 1024;

fn no_link(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            let link = meta.file_type().is_symlink();
            #[cfg(windows)]
            let link = {
                use std::os::windows::fs::MetadataExt;
                link || meta.file_attributes() & 0x400 != 0
            };
            if link {
                return Err(error("UNSAFE_PATH", "写入路径不能经过符号链接或重解析点"));
            }
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_error(e)),
    }
}
pub fn instance_dir(root: &Path, instance: &str) -> Result<PathBuf> {
    no_link(root)?;
    let layout = GameLayout::new(root).map_err(invalid)?;
    let directory = layout
        .version_dir(instance)
        .map_err(|e| error("UNSAFE_PATH", e))?;
    let mut path = layout.root().to_path_buf();
    for component in instance.split('/') {
        path.push(component);
        no_link(&path)?;
    }
    if !directory.is_dir() {
        return Err(error("INSTANCE_NOT_FOUND", "实例目录不存在"));
    }
    Ok(directory)
}
pub fn safe_join(base: &Path, relative: &str) -> Result<PathBuf> {
    let relative_path = Path::new(relative);
    if relative.is_empty()
        || relative.len() > 512
        || relative.contains(['\\', ':', '<', '>', '"', '|', '?', '*'])
        || relative.chars().any(char::is_control)
        || relative_path.is_absolute()
        || relative.split('/').any(|part| {
            part.is_empty() || part == "." || part == ".." || part.ends_with([' ', '.'])
        })
        || relative_path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(error("UNSAFE_PATH", "只能使用实例内的普通相对路径"));
    }
    let mut result = base.to_path_buf();
    for name in relative.split('/') {
        let stem = name
            .split('.')
            .next()
            .unwrap_or_default()
            .to_ascii_uppercase();
        if [
            "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
            "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
        ]
        .contains(&stem.as_str())
        {
            return Err(error("UNSAFE_PATH", "路径含 Windows 保留名称"));
        }
    }
    no_link(base)?;
    for part in relative_path.components() {
        result.push(part);
        no_link(&result)?;
    }
    Ok(result)
}
pub fn basename(name: &str, extension: &str) -> Result<()> {
    safe_join(Path::new("."), name)?;
    if name.contains('/') || !name.to_ascii_lowercase().ends_with(extension) {
        return Err(invalid(format!("文件名必须是单层 {extension} 文件")));
    }
    // Reject DOS device aliases on every platform so a plan is portable to Windows.
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ]
    .contains(&stem.as_str())
    {
        return Err(invalid("文件名为 Windows 保留名称"));
    }
    Ok(())
}
pub fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    no_link(path)?;
    let file = fs::File::open(path).map_err(io_error)?;
    let meta = file.metadata().map_err(io_error)?;
    if !meta.is_file() || meta.len() > limit as u64 {
        return Err(invalid("文件类型或大小超出限制"));
    }
    let mut bytes = vec![];
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > limit {
        return Err(invalid("文件大小超出限制"));
    }
    Ok(bytes)
}
pub fn file_hash(path: &Path) -> Result<Option<String>> {
    file_hash_with_limit(path, 64 * 1024 * 1024)
}
pub(crate) fn file_hash_with_limit(path: &Path, limit: usize) -> Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_error(e)),
        Ok(meta) => {
            no_link(path)?;
            if !meta.is_file() || meta.len() > limit as u64 {
                return Err(invalid("目标文件类型或大小不受支持"));
            }
            let mut reader = fs::File::open(path).map_err(io_error)?;
            let mut hash = Sha256::new();
            let mut buffer = [0; 64 * 1024];
            let mut total = 0usize;
            loop {
                let n = reader.read(&mut buffer).map_err(io_error)?;
                if n == 0 {
                    break;
                }
                total += n;
                if total > limit {
                    return Err(invalid("目标文件过大"));
                }
                hash.update(&buffer[..n]);
            }
            Ok(Some(format!("{:x}", hash.finalize())))
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Watch {
    pub path: String,
    #[serde(default)]
    pub directory: bool,
    pub hash: Option<String>,
}
impl Watch {
    pub fn file(base: &Path, path: &str) -> Result<Self> {
        Ok(Self {
            path: path.into(),
            directory: false,
            hash: file_hash(&safe_join(base, path)?)?,
        })
    }
    pub fn directory(base: &Path, path: &str) -> Result<Self> {
        Ok(Self {
            path: path.into(),
            directory: true,
            hash: directory_hash(&safe_join(base, path)?)?,
        })
    }
    pub fn check(&self, base: &Path) -> Result<()> {
        let path = safe_join(base, &self.path)?;
        let actual = if self.directory {
            directory_hash(&path)?
        } else {
            file_hash(&path)?
        };
        if actual != self.hash {
            return Err(error(
                "PLAN_STALE",
                format!("{} 已发生变化，请重新预览", self.path),
            ));
        }
        Ok(())
    }
}
fn directory_hash(path: &Path) -> Result<Option<String>> {
    no_link(path)?;
    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(io_error(e)),
    };
    let mut items = vec![];
    for entry in entries {
        let entry = entry.map_err(io_error)?;
        no_link(&entry.path())?;
        items.push(entry.file_name().to_string_lossy().into_owned());
        if items.len() > 1024 {
            return Err(invalid("目录条目过多"));
        }
    }
    items.sort();
    Ok(Some(digest(
        serde_json::to_string(&items).map_err(io_error)?.as_bytes(),
    )))
}
pub fn create_dirs(base: &Path, relative: &str) -> Result<PathBuf> {
    let destination = safe_join(base, relative)?;
    let mut current = base.to_path_buf();
    for component in Path::new(relative).components() {
        current.push(component);
        no_link(&current)?;
        if !current.exists() {
            fs::create_dir(&current).map_err(io_error)?;
        }
        no_link(&current)?;
    }
    Ok(destination)
}
pub fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    no_link(path)?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(io_error)?;
    file.write_all(bytes).map_err(io_error)?;
    file.sync_all().map_err(io_error)
}
pub fn lock(base: &Path) -> Result<fs::File> {
    let file=lock_raw(base)?;
    crate::service::file_transaction::recover_locked(base)?;
    Ok(file)
}
pub(crate) fn lock_raw(base: &Path) -> Result<fs::File> {
    create_dirs(base, ".memoria/mcp")?;
    let path = safe_join(base, ".memoria/mcp/write.lock")?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(io_error)?;
    file.try_lock()
        .map_err(|_| error("INSTANCE_BUSY", "实例正在处理另一项写入"))?;
    Ok(file)
}
pub fn config_path(path: &str) -> Result<()> {
    let lower = path.to_ascii_lowercase();
    if !(lower.starts_with("config/") || lower.starts_with("defaultconfigs/"))
        || lower.starts_with("config/ftbquests/")
        || lower.starts_with("defaultconfigs/ftbquests/")
    {
        return Err(error(
            "UNSAFE_PATH",
            "配置权限仅覆盖 config/ 和 defaultconfigs/；FTB 任务须使用任务接口",
        ));
    }
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if ![
        "json",
        "toml",
        "cfg",
        "ini",
        "properties",
        "yaml",
        "yml",
        "txt",
        "snbt",
    ]
    .contains(&ext.as_str())
    {
        return Err(invalid("不支持的配置文件扩展名"));
    }
    safe_join(Path::new("."), path).map(|_| ())
}
