//! Bounded, read-only access to known instance logs, shared by desktop and MCP.
use super::{layout::checked_join, GameLayout};
use crate::{LauncherError, LauncherResult};
use regex::Regex;
use serde::Serialize;
use std::{
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::UNIX_EPOCH,
};

pub const MAX_LOG_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct InstanceLog {
    /// Relative to the isolated instance directory; accepted by read_instance_log.
    pub path: String,
    pub kind: String,
    pub bytes: u64,
    pub modified_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InstanceLogContent {
    pub instance_id: String,
    pub path: String,
    pub text: String,
    pub truncated: bool,
    pub redacted_lines: usize,
}

fn log_kind(path: &str) -> Option<&'static str> {
    let parts: Vec<_> = path.split('/').collect();
    match parts.as_slice() {
        [".launcher", "logs", name] if name.ends_with(".log") => Some("launcher"),
        ["logs", "latest.log" | "debug.log"] => Some("game"),
        ["crash-reports", name] if name.starts_with("crash-") && name.ends_with(".txt") => {
            Some("crash_report")
        }
        [name] if name.starts_with("hs_err_pid") && name.ends_with(".log") => Some("jvm_crash"),
        _ => None,
    }
}

fn log_path(layout: &GameLayout, instance: &str, path: &str) -> LauncherResult<PathBuf> {
    if log_kind(path).is_none() || path.contains('\\') {
        return Err(LauncherError::UnsafePath(path.into()));
    }
    let directory = layout.version_dir(instance)?;
    let result = checked_join(&directory, Path::new(path))?;
    // A log name must not alias another file inside the instance, either.
    let mut component = directory;
    for part in path.split('/') {
        component.push(part);
        match fs::symlink_metadata(&component) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(LauncherError::UnsafePath(path.into()));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(result)
}

pub async fn list_instance_logs(
    layout: GameLayout,
    instance: String,
) -> LauncherResult<Vec<InstanceLog>> {
    tokio::task::spawn_blocking(move || {
        let directory = layout.version_dir(&instance)?;
        let mut logs = Vec::new();
        for folder in [".launcher/logs", "logs", "crash-reports", ""] {
            let location = if folder.is_empty() {
                directory.clone()
            } else {
                checked_join(&directory, Path::new(folder))?
            };
            let entries = match fs::read_dir(location) {
                Ok(entries) => entries,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            for entry in entries {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().into_owned();
                let relative = if folder.is_empty() {
                    name
                } else {
                    format!("{folder}/{name}")
                };
                let Some(kind) = log_kind(&relative) else {
                    continue;
                };
                let Ok(path) = log_path(&layout, &instance, &relative) else {
                    continue;
                };
                let meta = fs::metadata(path)?;
                if meta.is_file() {
                    logs.push(InstanceLog {
                        path: relative,
                        kind: kind.into(),
                        bytes: meta.len(),
                        modified_unix_ms: meta
                            .modified()
                            .ok()
                            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                            .map(|time| time.as_millis() as u64),
                    });
                }
            }
        }
        logs.sort_by(|a, b| {
            b.modified_unix_ms
                .cmp(&a.modified_unix_ms)
                .then(a.path.cmp(&b.path))
        });
        logs.truncate(100);
        Ok(logs)
    })
    .await
    .map_err(|error| LauncherError::Internal(error.to_string()))?
}

fn redact(text: &str) -> (String, usize) {
    static CREDENTIAL: OnceLock<Regex> = OnceLock::new();
    let pattern = CREDENTIAL.get_or_init(|| Regex::new(
        r"(?i)(access[_-]?token|refresh[_-]?token|client[_-]?token|client[_-]?secret|api[_-]?key|authorization\s*[:=]|bearer\s+|xbl3\.0\s+x=|--session\b|password\s*[:=])"
    ).expect("valid credential pattern"));
    let mut redacted = 0;
    let lines: Vec<_> = text
        .lines()
        .map(|line| {
            if pattern.is_match(line) {
                redacted += 1;
                "[REDACTED: credential-bearing log line]"
            } else {
                line
            }
        })
        .collect();
    (lines.join("\n"), redacted)
}

pub async fn read_instance_log(
    layout: GameLayout,
    instance: String,
    path: String,
    max_bytes: usize,
) -> LauncherResult<InstanceLogContent> {
    if max_bytes == 0 || max_bytes > MAX_LOG_BYTES {
        return Err(LauncherError::InvalidMetadata(format!(
            "max_bytes 必须在 1..={MAX_LOG_BYTES} 之间"
        )));
    }
    tokio::task::spawn_blocking(move || {
        let location = log_path(&layout, &instance, &path)?;
        if !fs::metadata(&location)?.is_file() {
            return Err(LauncherError::UnsafePath(path));
        }
        let mut file = File::open(location)?;
        let meta = file.metadata()?;
        if !meta.is_file() {
            return Err(LauncherError::UnsafePath(path));
        }
        let start = meta.len().saturating_sub(max_bytes as u64);
        file.seek(SeekFrom::Start(start))?;
        let mut bytes = Vec::new();
        file.take(max_bytes as u64).read_to_end(&mut bytes)?;
        // Discard a partial first line so a cut-off credential prefix cannot bypass redaction.
        let truncated = start > 0;
        let bytes = if truncated {
            bytes
                .iter()
                .position(|byte| *byte == b'\n')
                .map(|i| &bytes[i + 1..])
                .unwrap_or(&[])
        } else {
            &bytes[..]
        };
        let (text, redacted_lines) = redact(&String::from_utf8_lossy(bytes));
        Ok(InstanceLogContent {
            instance_id: instance,
            path,
            text,
            truncated,
            redacted_lines,
        })
    })
    .await
    .map_err(|error| LauncherError::Internal(error.to_string()))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> GameLayout {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/log-tests")
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(root.join("versions/test/.launcher/logs")).unwrap();
        GameLayout::new(root).unwrap()
    }

    #[tokio::test]
    async fn lists_all_supported_log_kinds_and_reports_missing_files() {
        let layout = fixture();
        let instance = layout.version_dir("versions/test").unwrap();
        for path in [
            ".launcher/logs/run.log",
            "logs/latest.log",
            "logs/debug.log",
            "crash-reports/crash-test.txt",
            "hs_err_pid123.log",
        ] {
            let full = instance.join(path);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, "fixture").unwrap();
        }
        let logs = list_instance_logs(layout.clone(), "versions/test".into())
            .await
            .unwrap();
        assert_eq!(logs.len(), 5);
        for kind in ["launcher", "game", "crash_report", "jvm_crash"] {
            assert!(logs.iter().any(|log| log.kind == kind));
        }
        assert!(read_instance_log(
            layout,
            "versions/test".into(),
            ".launcher/logs/missing.log".into(),
            100
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn tail_never_reveals_a_credential_when_its_prefix_is_cut_off() {
        let layout = fixture();
        let instance = layout.version_dir("versions/test").unwrap();
        fs::write(
            instance.join(".launcher/logs/run.log"),
            format!("accessToken={}\nError: fixture", "private".repeat(100)),
        )
        .unwrap();
        let log = read_instance_log(
            layout,
            "versions/test".into(),
            ".launcher/logs/run.log".into(),
            64,
        )
        .await
        .unwrap();
        assert!(log.truncated);
        assert_eq!(log.text, "Error: fixture");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn rejects_windows_junctions_outside_the_instance() {
        let layout = fixture();
        let outside = layout.root().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("latest.log"), "private").unwrap();
        let link = layout.version_dir("versions/test").unwrap().join("logs");
        let result = std::process::Command::new("cmd.exe")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(result.status.success(), "could not create fixture junction");
        assert!(read_instance_log(
            layout,
            "versions/test".into(),
            "logs/latest.log".into(),
            100
        )
        .await
        .is_err());
    }

    #[tokio::test]
    async fn lists_only_logs_and_reads_bounded_redacted_tail() {
        let layout = fixture();
        let instance = layout.version_dir("versions/test").unwrap();
        fs::write(
            instance.join(".launcher/logs/run.log"),
            "prefix\naccessToken=private\nCaused by: missing mod\n",
        )
        .unwrap();
        fs::write(instance.join("accounts.json"), "private").unwrap();
        let logs = list_instance_logs(layout.clone(), "versions/test".into())
            .await
            .unwrap();
        assert_eq!(logs.len(), 1);
        let full = read_instance_log(
            layout.clone(),
            "versions/test".into(),
            logs[0].path.clone(),
            MAX_LOG_BYTES,
        )
        .await
        .unwrap();
        assert!(!full.text.contains("private"));
        assert!(full.text.contains("Caused by: missing mod"));
        assert_eq!(full.redacted_lines, 1);
        let tail = read_instance_log(layout, "versions/test".into(), logs[0].path.clone(), 32)
            .await
            .unwrap();
        assert!(tail.truncated);
        assert!(tail.text.len() <= 32);
        assert!(!tail.text.contains("private"));
    }

    #[tokio::test]
    async fn rejects_traversal_non_logs_and_invalid_limits() {
        let layout = fixture();
        for (key, path) in [
            ("versions/test", "accounts.json"),
            ("versions/test", ".launcher/logs/../../../accounts.log"),
            ("versions/../test", "logs/latest.log"),
            ("versions/test", "C:/private.log"),
            ("versions/test", "logs\\latest.log"),
        ] {
            assert!(
                read_instance_log(layout.clone(), key.into(), path.into(), 1024)
                    .await
                    .is_err()
            );
        }
        for limit in [0, MAX_LOG_BYTES + 1] {
            assert!(read_instance_log(
                layout.clone(),
                "versions/test".into(),
                "logs/latest.log".into(),
                limit
            )
            .await
            .is_err());
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_log_symlinks() {
        let layout = fixture();
        let directory = layout.version_dir("versions/test").unwrap();
        fs::write(directory.join("private"), "private").unwrap();
        std::os::unix::fs::symlink(
            directory.join("private"),
            directory.join(".launcher/logs/link.log"),
        )
        .unwrap();
        assert!(read_instance_log(
            layout,
            "versions/test".into(),
            ".launcher/logs/link.log".into(),
            1024
        )
        .await
        .is_err());
    }
}
