//! Archive extraction helpers.

use std::{
    fs::{self, File},
    path::Path,
};

use zip::ZipArchive;

use crate::{io::paths::{safe_join, write_archive_entry}, Result};

/// Extracts a ZIP archive while rejecting entries that escape the destination.
///
/// # Errors
///
/// Returns [`crate::LauncherError`] if the archive cannot be read, a file cannot
/// be written, or an entry path is unsafe.
pub fn extract_zip_safely(zip_path: impl AsRef<Path>, destination: impl AsRef<Path>) -> Result<()> {
    let file = File::open(zip_path)?;
    let mut archive = ZipArchive::new(file)?;
    let destination = destination.as_ref();
    fs::create_dir_all(destination)?;

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let Some(enclosed) = entry.enclosed_name().map(|path| path.to_path_buf()) else {
            let unsafe_name = entry.name().to_string();
            safe_join(destination, unsafe_name)?;
            continue;
        };
        let output = safe_join(destination, enclosed)?;

        if entry.is_dir() {
            fs::create_dir_all(&output)?;
            continue;
        }

        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)?;
        }
        // Ignore archive-supplied symlink entries, even when their names are safe.
        if entry.unix_mode().is_some_and(|mode| mode & 0o170000 == 0o120000) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "ZIP symlink entries are forbidden",
            ).into());
        }
        write_archive_entry(&output, &mut entry)?;
    }

    Ok(())
}


#[cfg(all(test, unix))]
mod security_tests {
    use super::*;
    use std::{io::Write, time::{SystemTime, UNIX_EPOCH}};

    #[test]
    fn extracting_through_existing_symlink_does_not_touch_outside() {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("memoria-archive-test-{}-{stamp}", std::process::id()));
        let dest = root.join("dest");
        let outside = root.join("outside");
        fs::create_dir_all(&dest).unwrap();
        fs::create_dir_all(&outside).unwrap();
        std::os::unix::fs::symlink(&outside, dest.join("escape")).unwrap();
        let archive_path = root.join("attack.zip");
        let mut archive = zip::ZipWriter::new(File::create(&archive_path).unwrap());
        archive.start_file("escape/owned.txt", zip::write::SimpleFileOptions::default()).unwrap();
        archive.write_all(b"untrusted").unwrap();
        archive.finish().unwrap();
        assert!(extract_zip_safely(&archive_path, &dest).is_err());
        assert!(!outside.join("owned.txt").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
