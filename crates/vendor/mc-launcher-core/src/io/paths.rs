//! Path-safety helpers.

use std::{
    fs::{self, OpenOptions},
    io::{self, Read},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

static NEXT_TEMP_FILE: AtomicU64 = AtomicU64::new(0);


use crate::{LauncherError, Result};

/// Joins a relative path to a base directory without allowing traversal.
///
/// # Errors
///
/// Returns [`LauncherError::UnsafePath`] when `relative` is absolute or contains
/// parent-directory, root, or platform-prefix components.
pub fn safe_join(base: impl AsRef<Path>, relative: impl AsRef<Path>) -> Result<PathBuf> {
    let base = base.as_ref();
    let relative = relative.as_ref();

    if relative.is_absolute() {
        return Err(LauncherError::UnsafePath {
            base: base.to_path_buf(),
            path: relative.to_path_buf(),
        });
    }

    for component in relative.components() {
        match component {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(LauncherError::UnsafePath {
                    base: base.to_path_buf(),
                    path: relative.to_path_buf(),
                });
            }
            Component::CurDir | Component::Normal(_) => {}
        }
    }

    // A lexically safe path can still escape through a pre-existing symlink
    // (or a Windows junction / reparse point). Never follow these.
    // The base may not exist yet for callers constructing installation paths.
    // Resolve its nearest existing ancestor, then inspect every existing child.
    let mut ancestor = base;
    while !ancestor.exists() {
        ancestor = ancestor.parent().ok_or_else(|| LauncherError::UnsafePath {
            base: base.to_path_buf(),
            path: relative.to_path_buf(),
        })?;
    }
    let boundary = fs::canonicalize(ancestor)?;
    let mut candidate = base.to_path_buf();
    for component in relative.components() {
        candidate.push(component.as_os_str());
        match fs::symlink_metadata(&candidate) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink()
                    || !candidate.canonicalize()?.starts_with(&boundary)
                {
                    return Err(LauncherError::UnsafePath {
                        base: base.to_path_buf(),
                        path: candidate,
                    });
                }
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }

    Ok(candidate)
}

/// Stage bytes in a newly created sibling and rename the file into place.
/// This prevents following a symlink at the final path, and avoids leaving
/// a partially written library when an archive is malformed.
pub fn write_archive_entry<R: Read>(output: &Path, input: &mut R) -> io::Result<()> {
    let parent = output.parent().ok_or_else(|| io::Error::new(
        io::ErrorKind::InvalidInput,
        "archive entry has no parent directory",
    ))?;
    let basename = output.file_name().ok_or_else(|| io::Error::new(
        io::ErrorKind::InvalidInput,
        "archive entry has no filename",
    ))?.to_string_lossy();

    for _ in 0..32 {
        let counter = NEXT_TEMP_FILE.fetch_add(1, Ordering::Relaxed);
        let staging = parent.join(format!(".{basename}.{}.{counter}.part", std::process::id()));
        let mut staged = match OpenOptions::new().write(true).create_new(true).open(&staging) {
            Ok(file) => file,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        };
        let result = (|| {
            io::copy(input, &mut staged)?;
            staged.sync_all()?;
            drop(staged);
            // Windows rename does not overwrite. Remove the directory entry,
            // not the symlink's target, before installing the staged file.
            #[cfg(windows)]
            if fs::symlink_metadata(output).is_ok() {
                fs::remove_file(output)?;
            }
            fs::rename(&staging, output)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&staging);
        }
        return result;
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not reserve a temporary archive entry",
    ))
}


/// Verifies that a path is inside a base directory.
///
/// # Errors
///
/// Returns [`LauncherError::UnsafePath`] when `path` does not start with `base`.
pub fn ensure_inside_base(base: impl AsRef<Path>, path: impl AsRef<Path>) -> Result<()> {
    let base = base.as_ref();
    let path = path.as_ref();
    if !path.starts_with(base) {
        return Err(LauncherError::UnsafePath {
            base: base.to_path_buf(),
            path: path.to_path_buf(),
        });
    }
    Ok(())
}


#[cfg(test)]
mod security_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sandbox() -> PathBuf {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("memoria-zip-security-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(unix)]
    #[test]
    fn safe_join_rejects_symlinked_intermediate_directory() {
        let root = sandbox();
        let inside = root.join("extract");
        let outside = root.join("outside");
        fs::create_dir_all(&inside).unwrap();
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, inside.join("escape")).unwrap();
        assert!(safe_join(&inside, "escape/target.txt").is_err());
        assert!(!outside.join("target.txt").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn safe_join_rejects_symlinked_leaf() {
        let root = sandbox();
        let inside = root.join("extract");
        fs::create_dir_all(&inside).unwrap();
        let target = root.join("outside.txt");
        fs::write(&target, b"original").unwrap();
        std::os::unix::fs::symlink(&target, inside.join("file.txt")).unwrap();
        assert!(safe_join(&inside, "file.txt").is_err());
        assert_eq!(fs::read(target).unwrap(), b"original");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn staged_entry_replaces_regular_file_without_partial_write() {
        let root = sandbox();
        let file = root.join("lib.so");
        fs::write(&file, b"old").unwrap();
        write_archive_entry(&file, &mut &b"new"[..]).unwrap();
        assert_eq!(fs::read(&file).unwrap(), b"new");
        fs::remove_dir_all(root).unwrap();
    }
}
