//! Local desktop operations. These do not grant permissions to external MCP clients.
use super::instance_write::{files, validate_jar};
use crate::LaunchOptions;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub type Result<T> = std::result::Result<T, String>;
pub(crate) fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}
const CONFIG: &str = ".memoria/instance-settings.json";
const MAX_LOCAL_MOD_BYTES: usize = 512 * 1024 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct InstanceSettings {
    pub name: String,
    pub java: Option<PathBuf>,
    pub memory_mb: Option<u32>,
    pub resolution: Option<(u32, u32)>,
    pub jvm_args: Vec<String>,
}
impl InstanceSettings {
    pub fn validate(&self) -> Result<()> {
        if self.name.chars().count() > 80 || self.name.chars().any(char::is_control) {
            return Err("实例名称最多 80 个字符，不能含控制字符".into());
        }
        if self.java.as_ref().is_some_and(|p| p.as_os_str().is_empty()) {
            return Err("Java 路径不能为空".into());
        }
        if self.memory_mb.is_some_and(|v| !(512..=262144).contains(&v)) {
            return Err("独立内存应为 512–262144 MiB".into());
        }
        if self
            .resolution
            .is_some_and(|(w, h)| !(320..=16384).contains(&w) || !(240..=16384).contains(&h))
        {
            return Err("窗口尺寸应为 320×240 至 16384×16384".into());
        }
        if self.jvm_args.len() > 64
            || self.jvm_args.iter().any(|v| {
                v.len() > 2048
                    || !v.starts_with('-')
                    || v.chars().any(char::is_control)
                    || [
                        "-Xmx",
                        "-Xms",
                        "-cp",
                        "-classpath",
                        "--class-path",
                        "-jar",
                        "--module-path",
                    ]
                    .iter()
                    .any(|p| v.starts_with(p))
            })
        {
            return Err("JVM 参数每行一项，以 - 开头；内存与类路径由启动器管理".into());
        }
        Ok(())
    }
    pub fn apply(&self, options: &mut LaunchOptions) -> Result<()> {
        self.validate()?;
        if let Some(java) = &self.java {
            options.java = java.clone();
        }
        if let Some(memory) = self.memory_mb {
            options.max_memory_mb = memory;
            options.min_memory_mb = options.min_memory_mb.min(memory);
        }
        options.resolution = self.resolution;
        options.extra_jvm_args = self.jvm_args.clone();
        Ok(())
    }
}
pub fn directory(root: &Path, key: &str) -> Result<PathBuf> {
    files::instance_dir(root, key).map_err(error)
}
pub fn load_settings(root: &Path, key: &str) -> Result<InstanceSettings> {
    let base = directory(root, key)?;
    let path = files::safe_join(&base, CONFIG).map_err(error)?;
    if !path.exists() {
        return Ok(InstanceSettings::default());
    }
    let value: InstanceSettings =
        serde_json::from_slice(&files::read_bounded(&path, 128 * 1024).map_err(error)?)
            .map_err(error)?;
    value.validate()?;
    Ok(value)
}
pub fn save_settings(
    root: &Path,
    key: &str,
    config: &InstanceSettings,
    expected: &Option<String>,
) -> Result<()> {
    config.validate()?;
    let base = directory(root, key)?;
    let _lock = files::lock(&base).map_err(error)?;
    let path = files::safe_join(&base, CONFIG).map_err(error)?;
    if files::file_hash(&path).map_err(error)? != *expected {
        return Err("实例设置已被修改，请重新打开后再保存".into());
    }
    files::create_dirs(&base, ".memoria").map_err(error)?;
    let temporary = files::safe_join(
        &base,
        &format!(".memoria/settings-{}.tmp", uuid::Uuid::new_v4()),
    )
    .map_err(error)?;
    files::write_new(
        &temporary,
        &serde_json::to_vec_pretty(config).map_err(error)?,
    )
    .map_err(error)?;
    if let Err(e) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(temporary);
        return Err(error(e));
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct ModEntry {
    pub filename: String,
    pub name: String,
    pub size: u64,
    pub enabled: bool,
    pub hash: String,
}
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub config: InstanceSettings,
    pub config_hash: Option<String>,
    pub mods: Vec<ModEntry>,
}
pub fn snapshot(root: &Path, key: &str) -> Result<Snapshot> {
    let base = directory(root, key)?;
    let config_hash =
        files::file_hash(&files::safe_join(&base, CONFIG).map_err(error)?).map_err(error)?;
    let config = load_settings(root, key)?;
    let folder = files::safe_join(&base, "mods").map_err(error)?;
    let mut mods = Vec::new();
    if folder.exists() {
        for file in fs::read_dir(folder).map_err(error)? {
            let file = file.map_err(error)?;
            let filename = file.file_name().to_string_lossy().into_owned();
            let lower = filename.to_ascii_lowercase();
            if !lower.ends_with(".jar") && !lower.ends_with(".jar.disabled") {
                continue;
            }
            let path = files::safe_join(&base, &format!("mods/{filename}")).map_err(error)?;
            let hash = files::file_hash_with_limit(&path, MAX_LOCAL_MOD_BYTES).map_err(error)?
                .ok_or_else(|| format!("Mod 文件已消失：{filename}"))?;
            let size = fs::metadata(&path).map_err(error)?.len();
            let name = fs::File::open(&path).ok().and_then(mod_name).unwrap_or_else(|| filename.clone());
            mods.push(ModEntry {
                filename,
                name,
                size,
                enabled: lower.ends_with(".jar"),
                hash,
            });
            if mods.len() > 1024 {
                return Err("Mod 数量超过 1024".into());
            }
        }
    }
    mods.sort_by_key(|m| m.filename.to_lowercase());
    Ok(Snapshot {
        config,
        config_hash,
        mods,
    })
}
fn mod_name(reader: impl std::io::Read + std::io::Seek) -> Option<String> {
    use std::io::Read;
    let mut zip = zip::ZipArchive::new(reader).ok()?;
    let mut entry = zip.by_name("fabric.mod.json").ok()?;
    if entry.size() > 65536 {
        return None;
    }
    let mut value = String::new();
    entry.read_to_string(&mut value).ok()?;
    let value: serde_json::Value = serde_json::from_str(&value).ok()?;
    Some(format!(
        "{} {}",
        value["name"].as_str().or(value["id"].as_str())?,
        value["version"].as_str().unwrap_or_default()
    ))
}
#[derive(Debug, Clone)]
pub enum ModAction {
    Toggle(ModEntry),
    Remove(ModEntry),
    Import(Vec<PathBuf>),
}
pub fn change_mod(root: &Path, key: &str, action: ModAction) -> Result<Snapshot> {
    let base = directory(root, key)?;
    let _lock = files::lock(&base).map_err(error)?;
    match &action {
        ModAction::Toggle(entry) | ModAction::Remove(entry) => {
            let source =
                files::safe_join(&base, &format!("mods/{}", entry.filename)).map_err(error)?;
            if files::file_hash_with_limit(&source, MAX_LOCAL_MOD_BYTES).map_err(error)?.as_deref() != Some(&entry.hash) {
                return Err("Mod 文件已变化，请刷新后操作".into());
            }
            let lower = entry.filename.to_ascii_lowercase();
            if !lower.ends_with(".jar") && !lower.ends_with(".jar.disabled") {
                return Err("只允许管理 JAR 文件".into());
            }
            let relative = if matches!(&action, ModAction::Remove(_)) {
                files::create_dirs(&base, ".memoria/removed-mods").map_err(error)?;
                format!(
                    ".memoria/removed-mods/{}-{}",
                    uuid::Uuid::new_v4(),
                    entry.filename
                )
            } else if entry.enabled {
                format!("mods/{}.disabled", entry.filename)
            } else {
                format!("mods/{}", &entry.filename[..entry.filename.len() - 9])
            };
            let target = files::safe_join(&base, &relative).map_err(error)?;
            // hard_link fails if the target exists, including a concurrent external write.
            fs::hard_link(&source, &target).map_err(error)?;
            fs::remove_file(&source).map_err(error)?;
        }
        ModAction::Import(paths) => {
            if paths.is_empty() || paths.len() > 32 {
                return Err("一次可导入 1–32 个 JAR".into());
            }
            let mut prepared = Vec::new();
            let mut total = 0;
            for path in paths {
                let filename = path
                    .file_name()
                    .and_then(|v| v.to_str())
                    .ok_or("无效文件名")?;
                files::basename(filename, ".jar").map_err(error)?;
                let target = files::safe_join(&base, &format!("mods/{filename}")).map_err(error)?;
                if target.exists()
                    || files::safe_join(&base, &format!("mods/{filename}.disabled"))
                        .map_err(error)?
                        .exists()
                    || prepared.iter().any(|(p, _): &(PathBuf, Vec<u8>)| {
                        p.file_name().unwrap().eq_ignore_ascii_case(filename)
                    })
                {
                    return Err(format!("{filename} 已存在，请先移除旧版本"));
                }
                let bytes = files::read_bounded(path, files::MAX_MOD_BYTES).map_err(error)?;
                validate_jar(&bytes).map_err(error)?;
                total += bytes.len();
                if total > 128 * 1024 * 1024 {
                    return Err("一次导入合计不能超过 128 MiB".into());
                }
                prepared.push((target, bytes));
            }
            files::create_dirs(&base, "mods").map_err(error)?;
            let mut written = Vec::new();
            for (path, bytes) in prepared {
                if let Err(e) = files::write_new(&path, &bytes) {
                    for previous in written {
                        let _ = fs::remove_file(previous);
                    }
                    return Err(error(e));
                }
                written.push(path);
            }
        }
    }
    snapshot(root, key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn fixture() -> PathBuf {
        let root = std::env::temp_dir().join(format!("memoria-instance-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(root.join("versions/test/mods")).unwrap();
        root
    }
    fn jar() -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(br#"{"schemaVersion":1,"id":"sample","version":"1.0","name":"Example"}"#)
            .unwrap();
        zip.finish().unwrap().into_inner()
    }
    #[test]
    fn settings_survive_reload_and_apply_to_real_launch_options_without_touching_version_json() {
        let root = fixture();
        let key = "versions/test";
        fs::write(root.join("versions/test/test.json"), b"original").unwrap();
        let config = InstanceSettings {
            name: "独立实例".into(),
            java: Some("custom-java.exe".into()),
            memory_mb: Some(2048),
            resolution: Some((1280, 720)),
            jvm_args: vec!["-Dsample=value with spaces".into()],
        };
        save_settings(&root, key, &config, &None).unwrap();
        assert!(root
            .join("versions/test/.memoria/instance-settings.json")
            .is_file());
        let loaded = load_settings(&root, key).unwrap();
        assert_eq!(config, loaded);
        let mut options = LaunchOptions::default();
        loaded.apply(&mut options).unwrap();
        assert_eq!(options.java, PathBuf::from("custom-java.exe"));
        assert_eq!(options.max_memory_mb, 2048);
        assert_eq!(options.resolution, Some((1280, 720)));
        assert_eq!(options.extra_jvm_args, config.jvm_args);
        assert_eq!(
            fs::read(root.join("versions/test/test.json")).unwrap(),
            b"original"
        );
        assert!(save_settings(&root, key, &config, &None)
            .unwrap_err()
            .contains("已被修改"));
        let mut invalid = config;
        invalid.jvm_args = vec!["-Xmx64G".into()];
        assert!(invalid.validate().is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn import_toggle_and_remove_preserve_the_original_bytes_and_reject_stale_or_duplicate_files() {
        let root = fixture();
        let key = "versions/test";
        let bytes = jar();
        let source = root.join("sample.jar");
        fs::write(&source, &bytes).unwrap();
        let first = change_mod(&root, key, ModAction::Import(vec![source.clone()])).unwrap();
        assert_eq!(first.mods[0].name, "Example 1.0");
        assert!(first.mods[0].enabled);
        assert!(change_mod(&root, key, ModAction::Import(vec![source])).is_err());
        let disabled = change_mod(&root, key, ModAction::Toggle(first.mods[0].clone())).unwrap();
        assert!(!disabled.mods[0].enabled);
        assert!(change_mod(&root, key, ModAction::Toggle(first.mods[0].clone())).is_err());
        let enabled = change_mod(&root, key, ModAction::Toggle(disabled.mods[0].clone())).unwrap();
        assert!(enabled.mods[0].enabled);
        let removed = change_mod(&root, key, ModAction::Remove(enabled.mods[0].clone())).unwrap();
        assert!(removed.mods.is_empty());
        let backup = fs::read_dir(root.join("versions/test/.memoria/removed-mods"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert_eq!(fs::read(backup).unwrap(), bytes);
        assert!(directory(&root, "versions/../escape").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn large_local_mod_does_not_block_settings_and_can_be_toggled_and_removed() {
        use std::io::{Seek,SeekFrom};
        let root=fixture();
        let path=root.join("versions/test/mods/large.jar");
        let mut file=fs::File::create(&path).unwrap();
        file.set_len(70*1024*1024).unwrap();
        file.seek(SeekFrom::End(0)).unwrap();
        file.write_all(&jar()).unwrap();
        drop(file);
        let initial=snapshot(&root,"versions/test").unwrap();
        assert_eq!(initial.mods.len(),1);
        assert!(initial.mods[0].size>64*1024*1024);
        assert_eq!(initial.mods[0].name,"Example 1.0");
        let config=InstanceSettings{name:"Large Mod Instance".into(),..Default::default()};
        save_settings(&root,"versions/test",&config,&initial.config_hash).unwrap();
        assert_eq!(snapshot(&root,"versions/test").unwrap().config,config);
        let disabled=change_mod(&root,"versions/test",ModAction::Toggle(initial.mods[0].clone())).unwrap();
        assert!(!disabled.mods[0].enabled);
        let removed=change_mod(&root,"versions/test",ModAction::Remove(disabled.mods[0].clone())).unwrap();
        assert!(removed.mods.is_empty());
        assert!(root.join("versions/test/.memoria/removed-mods").is_dir());
        fs::remove_dir_all(root).unwrap();
    }
}
