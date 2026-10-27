//! Portable general preferences; exported files contain no accounts or credentials.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

pub const DATES: [&str; 3] = ["YYYY-MM-DD", "DD/MM/YYYY", "MM/DD/YYYY"];
const MAX_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub minimize_on_launch: bool,
    pub start_minimized: bool,
    pub check_updates: bool,
    pub minimize_on_close: bool,
    pub prerelease: bool,
    pub date_format: usize,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    format: String,
    version: u32,
    general: Config,
}

impl Config {
    pub fn switches(&self) -> [bool; 5] {
        [
            self.minimize_on_launch,
            self.start_minimized,
            self.check_updates,
            self.minimize_on_close,
            self.prerelease,
        ]
    }

    pub fn set_switch(&mut self, index: usize, value: bool) {
        match index {
            0 => self.minimize_on_launch = value,
            1 => self.start_minimized = value,
            2 => self.check_updates = value,
            3 => self.minimize_on_close = value,
            4 => self.prerelease = value,
            _ => {}
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.date_format >= DATES.len() {
            return Err("日期格式无效。".into());
        }
        Ok(())
    }

    pub fn date_pattern(&self) -> &'static str {
        match self.date_format {
            1 => "%d/%m/%Y %H:%M",
            2 => "%m/%d/%Y %H:%M",
            _ => "%Y-%m-%d %H:%M",
        }
    }
}

pub fn read(path: &Path) -> Result<Config, String> {
    let bytes = bounded_read(path)?;
    let document: Document =
        serde_json::from_slice(&bytes).map_err(|e| format!("常规配置无效：{e}"))?;
    if document.format != "memoria-general-settings" || document.version != 1 {
        return Err("不支持的常规配置格式或版本。".into());
    }
    document.general.validate()?;
    Ok(document.general)
}

pub fn load(directory: &Path) -> Result<Config, String> {
    let path = directory.join("general.json");
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        _ => read(&path),
    }
}

fn bounded_read(path: &Path) -> Result<Vec<u8>, String> {
    let meta = fs::metadata(path).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return Err("配置必须是 64 KiB 以内的 JSON 文件。".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("配置文件过大。".into());
    }
    Ok(bytes)
}

pub fn write(path: &Path, config: &Config) -> Result<(), String> {
    config.validate()?;
    let bytes = serde_json::to_vec_pretty(&Document {
        format: "memoria-general-settings".into(),
        version: 1,
        general: config.clone(),
    })
    .map_err(|e| e.to_string())?;
    let parent = path.parent().ok_or("配置路径无效")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!("general-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e| e.to_string())
}

/// Move only launcher-owned cache out of service; never traverse or delete it.
pub fn recover_cache(directory: &Path) -> Result<String, String> {
    let cache = directory.join("cache");
    match fs::symlink_metadata(&cache) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok("没有可清理的启动器缓存。".into())
        }
        Err(e) => return Err(e.to_string()),
        Ok(meta) if !plain_directory(&meta) => {
            return Err("缓存路径不是普通目录，未执行清理。".into())
        }
        Ok(_) => {}
    }
    let recovery = directory.join("recovered-cache");
    match fs::symlink_metadata(&recovery) {
        Ok(meta) if !plain_directory(&meta) => return Err("缓存恢复路径不是普通目录。".into()),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir(&recovery).map_err(|e| e.to_string())?
        }
        Err(e) => return Err(e.to_string()),
    }
    let destination = recovery.join(uuid::Uuid::new_v4().to_string());
    fs::rename(cache, &destination).map_err(|e| e.to_string())?;
    Ok(format!(
        "缓存已移至 {}，需要时可手动恢复。",
        destination.display()
    ))
}

fn plain_directory(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    meta.is_dir() && !meta.file_type().is_symlink()
}

pub fn open_directory(directory: &Path) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    let directory = std::path::absolute(directory).map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        // Explorer can keep running as the file-manager process. Do not wait for its window.
        std::process::Command::new("explorer.exe")
            .arg(&directory)
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let mut command = std::process::Command::new("xdg-open");
    #[cfg(not(windows))]
    command
        .arg(&directory)
        .status()
        .map_err(|e| e.to_string())
        .and_then(|status| {
            if status.success() {
                Ok(())
            } else {
                Err(format!("文件管理器退出：{status}"))
            }
        })
}

#[cfg(test)]
#[path = "general_tests.rs"]
mod tests;
