use super::{
    layout::{checked_join, validate_id},
    rules::Rule,
    GameLayout,
};
use crate::{LauncherError, LauncherResult};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VersionMetadata {
    pub id: String,
    pub client_version: Option<String>,
    #[serde(rename = "type")]
    pub version_type: Option<String>,
    pub inherits_from: Option<String>,
    pub jar: Option<String>,
    pub main_class: Option<String>,
    pub minecraft_arguments: Option<String>,
    pub arguments: Option<Arguments>,
    #[serde(default)]
    pub libraries: Vec<Library>,
    pub asset_index: Option<AssetIndex>,
    pub assets: Option<String>,
    pub java_version: Option<JavaVersion>,
    #[serde(default)]
    pub downloads: HashMap<String, Artifact>,
    #[serde(default)]
    pub logging: HashMap<String, Logging>,
    pub release_time: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct Arguments {
    #[serde(default)]
    pub jvm: Vec<Argument>,
    #[serde(default)]
    pub game: Vec<Argument>,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum Argument {
    Plain(String),
    Conditional {
        #[serde(default)]
        rules: Vec<Rule>,
        value: ArgumentValue,
    },
}
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub(crate) enum ArgumentValue {
    One(String),
    Many(Vec<String>),
}
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Library {
    pub name: String,
    #[serde(default)]
    pub rules: Vec<Rule>,
    pub downloads: Option<LibraryDownloads>,
    #[serde(default)]
    pub natives: HashMap<String, String>,
    pub extract: Option<Extract>,
}
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct LibraryDownloads {
    pub artifact: Option<Artifact>,
    #[serde(default)]
    pub classifiers: HashMap<String, Artifact>,
}
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct Artifact {
    pub path: Option<String>,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Extract {
    #[serde(default)]
    pub exclude: Vec<String>,
}
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct AssetIndex {
    pub id: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct JavaVersion {
    #[serde(rename = "majorVersion")]
    pub major_version: u32,
}
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Logging {
    pub argument: String,
    pub file: LogFile,
}
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct LogFile {
    pub id: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

pub(crate) struct ResolvedVersion {
    pub metadata: VersionMetadata,
    pub jar_path: PathBuf,
}

pub(crate) fn resolve(layout: &GameLayout, key: &str) -> LauncherResult<ResolvedVersion> {
    resolve_inner(layout, key, &mut HashSet::new(), 0)
}

fn resolve_inner(
    layout: &GameLayout,
    key: &str,
    seen: &mut HashSet<String>,
    depth: usize,
) -> LauncherResult<ResolvedVersion> {
    if depth > 32 || !seen.insert(key.into()) {
        return Err(LauncherError::InvalidMetadata(format!(
            "版本继承循环或过深: {key}"
        )));
    }
    let dir = layout.version_dir(key)?;
    let id = key.split_once('/').unwrap().1;
    let file = checked_join(&dir, &PathBuf::from(format!("{id}.json")))?;
    let mut child: VersionMetadata = serde_json::from_reader(std::fs::File::open(file)?)?;
    if child.id != id {
        return Err(LauncherError::InvalidMetadata(format!(
            "文件夹 {id} 与元数据 id {} 不一致",
            child.id
        )));
    }
    let explicit_jar = child.jar.clone();
    let mut jar_path = checked_join(&dir, &PathBuf::from(format!("{id}.jar")))?;
    if let Some(parent_id) = child.inherits_from.as_deref() {
        let parent_key = layout.find_version(parent_id)?;
        let parent = resolve_inner(layout, &parent_key, seen, depth + 1)?;
        jar_path = parent.jar_path;
        let base = parent.metadata;
        child.main_class = child.main_class.or(base.main_class);
        child.client_version = child
            .client_version
            .or(base.client_version)
            .or(Some(base.id));
        child.version_type = child.version_type.or(base.version_type);
        child.asset_index = child.asset_index.or(base.asset_index);
        child.assets = child.assets.or(base.assets);
        child.java_version = child.java_version.or(base.java_version);
        // 新旧参数格式不能同时生效；旧式子版本明确替换父级 game 参数。
        if child.minecraft_arguments.is_none() {
            child.minecraft_arguments = base.minecraft_arguments;
        }
        let legacy_override = child.arguments.is_none() && child.minecraft_arguments.is_some();
        match (&mut child.arguments, base.arguments) {
            (Some(args), Some(mut parent_args)) => {
                parent_args.jvm.append(&mut args.jvm);
                parent_args.game.append(&mut args.game);
                *args = parent_args;
            }
            (slot @ None, Some(mut args)) => {
                if legacy_override {
                    args.game.clear();
                }
                *slot = Some(args);
            }
            _ => {}
        }
        let mut libs: BTreeMap<String, Library> = base
            .libraries
            .into_iter()
            .map(|l| (library_key(&l.name), l))
            .collect();
        for library in child.libraries {
            libs.insert(library_key(&library.name), library);
        }
        child.libraries = libs.into_values().collect();
        for (key, value) in base.downloads {
            child.downloads.entry(key).or_insert(value);
        }
        for (key, value) in base.logging {
            child.logging.entry(key).or_insert(value);
        }
    }
    if let Some(jar) = explicit_jar {
        validate_id(&jar)?;
        let jar_key = layout.find_version(&jar)?;
        jar_path = checked_join(
            &layout.version_dir(&jar_key)?,
            &PathBuf::from(format!("{jar}.jar")),
        )?;
    }
    if child
        .main_class
        .as_ref()
        .is_none_or(|s| s.is_empty() || s.chars().any(char::is_whitespace) || s.starts_with('-'))
    {
        return Err(LauncherError::InvalidMetadata(format!(
            "{id} 缺少有效 mainClass"
        )));
    }
    Ok(ResolvedVersion {
        metadata: child,
        jar_path,
    })
}

fn library_key(name: &str) -> String {
    let parts: Vec<_> = name.split(':').collect();
    format!(
        "{}:{}:{}",
        parts.first().unwrap_or(&""),
        parts.get(1).unwrap_or(&""),
        parts.get(3).unwrap_or(&"")
    )
}

pub(crate) fn maven_path(name: &str) -> LauncherResult<String> {
    let (coordinate, extension) = name.split_once('@').unwrap_or((name, "jar"));
    let parts: Vec<_> = coordinate.split(':').collect();
    if !(3..=4).contains(&parts.len()) {
        return Err(LauncherError::InvalidMetadata(format!(
            "无效 Maven 坐标 {name}"
        )));
    }
    for part in &parts {
        validate_id(part)?;
    }
    validate_id(extension)?;
    let classifier = parts.get(3).map(|c| format!("-{c}")).unwrap_or_default();
    Ok(format!(
        "{}/{}/{}/{}-{}{}.{}",
        parts[0].replace('.', "/"),
        parts[1],
        parts[2],
        parts[1],
        parts[2],
        classifier,
        extension
    ))
}
