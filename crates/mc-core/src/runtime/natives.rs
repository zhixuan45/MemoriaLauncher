use super::layout::checked_join;
use crate::{LauncherError, LauncherResult};
use std::{
    fs::File,
    path::{Path, PathBuf},
};

pub(crate) struct NativeArchive {
    pub path: PathBuf,
    pub exclude: Vec<String>,
}

pub(crate) fn extract(archives: &[NativeArchive], target: &Path) -> LauncherResult<()> {
    std::fs::create_dir_all(target)?;
    let mut total = 0_u64;
    for archive in archives {
        let mut zip = zip::ZipArchive::new(File::open(&archive.path)?)
            .map_err(|e| LauncherError::InvalidMetadata(format!("native ZIP: {e}")))?;
        for i in 0..zip.len() {
            let mut file = zip
                .by_index(i)
                .map_err(|e| LauncherError::InvalidMetadata(e.to_string()))?;
            let name = file.name().to_string();
            if file.is_dir()
                || name.starts_with("META-INF/")
                || archive.exclude.iter().any(|e| name.starts_with(e))
            {
                continue;
            }
            if file.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000) {
                return Err(LauncherError::UnsafePath(name));
            }
            let path = checked_join(target, Path::new(&name))?;
            total = total
                .checked_add(file.size())
                .ok_or_else(|| LauncherError::InvalidMetadata("native 解压体积溢出".into()))?;
            if total > 512 * 1024 * 1024 {
                return Err(LauncherError::InvalidMetadata(
                    "native 解压体积超过 512 MiB".into(),
                ));
            }
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut output = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?;
            std::io::copy(&mut file, &mut output)?;
        }
    }
    Ok(())
}
