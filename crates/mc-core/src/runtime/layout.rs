use std::path::{Component, Path, PathBuf};

use crate::{LauncherError, LauncherResult};

#[derive(Debug, Clone)]
pub struct GameLayout {
    root: PathBuf,
}

impl GameLayout {
    /// root 是包含 version(s)、libraries 和 assets 的游戏根目录。
    /// 不创建目录；扫描空目录时没有写入副作用。
    pub fn new(root: impl AsRef<Path>) -> LauncherResult<Self> {
        Ok(Self {
            root: std::path::absolute(root)?,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn versions(&self) -> LauncherResult<PathBuf> {
        self.path("versions")
    }
    pub fn libraries(&self) -> LauncherResult<PathBuf> {
        self.path("libraries")
    }
    pub fn assets(&self) -> LauncherResult<PathBuf> {
        self.path("assets")
    }

    pub(crate) fn path(&self, relative: impl AsRef<Path>) -> LauncherResult<PathBuf> {
        checked_join(&self.root, relative.as_ref())
    }

    pub(crate) fn version_dir(&self, key: &str) -> LauncherResult<PathBuf> {
        let (parent, id) = key
            .split_once('/')
            .ok_or_else(|| LauncherError::UnsafePath(key.into()))?;
        if !matches!(parent, "version" | "versions") {
            return Err(LauncherError::UnsafePath(key.into()));
        }
        validate_id(id)?;
        self.path(Path::new(parent).join(id))
    }

    pub(crate) fn find_version(&self, id: &str) -> LauncherResult<String> {
        validate_id(id)?;
        for parent in ["versions", "version"] {
            let key = format!("{parent}/{id}");
            if self.version_dir(&key)?.join(format!("{id}.json")).is_file() {
                return Ok(key);
            }
        }
        Err(LauncherError::InvalidMetadata(format!("找不到父版本 {id}")))
    }
}

pub(crate) fn validate_id(id: &str) -> LauncherResult<()> {
    if id.is_empty()
        || matches!(id, "." | "..")
        || id.trim_end_matches([' ', '.']) != id
        || id
            .chars()
            .any(|c| c.is_control() || r#"/\:<>"|?*"#.contains(c))
    {
        return Err(LauncherError::UnsafePath(id.into()));
    }
    Ok(())
}

/// 同时拒绝 Windows 与 Unix 形式的路径穿越；已有符号链接不能越过授权根。
pub(crate) fn checked_join(root: &Path, relative: &Path) -> LauncherResult<PathBuf> {
    let text = relative.to_string_lossy();
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || text.contains(':')
        || (!cfg!(windows) && text.contains('\\'))
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(LauncherError::UnsafePath(text.into_owned()));
    }
    for part in relative.components() {
        validate_id(&part.as_os_str().to_string_lossy())?;
    }
    let mut anchor = root;
    while !anchor.exists() {
        anchor = anchor
            .parent()
            .ok_or_else(|| LauncherError::UnsafePath(root.display().to_string()))?;
    }
    let canonical_anchor = anchor.canonicalize()?;
    let mut path = root.to_path_buf();
    // 根目录本身允许是用户显式指定的符号链接；其下的路径必须留在根内。
    let boundary = if root.exists() {
        root.canonicalize()?
    } else {
        canonical_anchor
    };
    for part in relative.components() {
        path.push(part.as_os_str());
        if let Ok(meta) = std::fs::symlink_metadata(&path) {
            if meta.file_type().is_symlink() || path.exists() {
                let actual = path.canonicalize()?;
                if !actual.starts_with(&boundary) {
                    return Err(LauncherError::UnsafePath(path.display().to_string()));
                }
            }
        }
    }
    Ok(path)
}
