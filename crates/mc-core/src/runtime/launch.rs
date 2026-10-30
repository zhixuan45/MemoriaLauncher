use super::{
    metadata::{self, Argument, ArgumentValue, Artifact, ResolvedVersion},
    natives::NativeArchive,
    rules::{allowed, Platform},
    AuthProvider, AuthSession, GameLayout, GameProcess, JavaRuntime,
};
use crate::{LauncherError, LauncherResult};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug, Clone)]
pub struct LaunchOptions {
    pub java: PathBuf,
    pub min_memory_mb: u32,
    pub max_memory_mb: u32,
    pub resolution: Option<(u32, u32)>,
    pub verify_hashes: bool,
    pub extra_jvm_args: Vec<String>,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            java: "java".into(),
            min_memory_mb: 512,
            max_memory_mb: 4096,
            resolution: None,
            verify_hashes: true,
            extra_jvm_args: Vec::new(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct LaunchCheck {
    pub instance_id: String,
    pub game_directory: PathBuf,
    pub java_major: u32,
    pub required_java_major: Option<u32>,
    pub classpath_entries: usize,
    pub native_archives: usize,
}

pub struct LaunchPlan {
    pub(crate) executable: PathBuf,
    pub(crate) arguments: Vec<String>,
    pub(crate) game_directory: PathBuf,
    pub(crate) natives_directory: PathBuf,
    pub(crate) log_path: PathBuf,
    pub(crate) natives: Vec<NativeArchive>,
    pub(crate) secrets: Vec<String>,
}

impl LaunchPlan {
    pub fn game_directory(&self) -> &Path {
        &self.game_directory
    }
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }
    pub fn redacted_arguments(&self) -> Vec<String> {
        self.arguments
            .iter()
            .map(|a| {
                self.secrets
                    .iter()
                    .filter(|s| !s.is_empty())
                    .fold(a.clone(), |v, s| v.replace(s, "[REDACTED]"))
            })
            .collect()
    }

    pub async fn spawn(self) -> LauncherResult<GameProcess> {
        super::process::spawn(self).await
    }
}

impl std::fmt::Debug for LaunchPlan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LaunchPlan")
            .field("executable", &self.executable)
            .field("arguments", &self.redacted_arguments())
            .field("game_directory", &self.game_directory)
            .finish()
    }
}

#[derive(Clone)]
pub struct GameLauncher {
    layout: GameLayout,
    auth: Arc<dyn AuthProvider>,
}

impl GameLauncher {
    pub fn new(layout: GameLayout, auth: Arc<dyn AuthProvider>) -> Self {
        Self { layout, auth }
    }

    /// 无凭据、无写入的启动前检查，可用于 GUI/CLI/API 诊断。
    pub async fn check(&self, key: &str, options: LaunchOptions) -> LauncherResult<LaunchCheck> {
        let java = inspect_java(&options).await?;
        let layout = self.layout.clone();
        let key = key.to_string();
        tokio::task::spawn_blocking(move || {
            let version = metadata::resolve(&layout, &key)?;
            check_java(&java, &version)?;
            let files = collect_files(
                &layout,
                &version,
                &platform_for(&java),
                options.verify_hashes,
            )?;
            Ok(LaunchCheck {
                game_directory: layout.version_dir(&key)?,
                instance_id: key,
                java_major: java.major_version,
                required_java_major: version.required_java_major(),
                classpath_entries: files.classpath.len(),
                native_archives: files.natives.len(),
            })
        })
        .await
        .map_err(|e| LauncherError::Internal(e.to_string()))?
    }

    /// 只生成计划；spawn 才创建启动目录并执行 Java，计划不支持序列化。
    pub async fn prepare(
        &self,
        key: &str,
        account_id: &str,
        options: LaunchOptions,
    ) -> LauncherResult<LaunchPlan> {
        let session = self.auth.minecraft_session(account_id).await?;
        session.validate()?;
        let java = inspect_java(&options).await?;
        let layout = self.layout.clone();
        let key = key.to_string();
        tokio::task::spawn_blocking(move || build_plan(&layout, &key, &java, &session, &options))
            .await
            .map_err(|e| LauncherError::Internal(e.to_string()))?
    }

    /// Repair only the declared logging resource; game and mod files are never guessed.
    pub async fn repair_logging(
        &self,
        key: &str,
        client: &crate::DownloadClient,
    ) -> LauncherResult<()> {
        let layout = self.layout.clone();
        let key = key.to_string();
        let file = tokio::task::spawn_blocking(move || {
            let version = metadata::resolve(&layout, &key)?;
            let Some(logging) = version.metadata.logging.get("client") else {
                return Ok(None);
            };
            super::layout::validate_id(&logging.file.id)?;
            let relative = format!("assets/log_configs/{}", logging.file.id);
            let path = layout.path(&relative)?;
            let descriptor = Artifact {
                path: None,
                sha1: logging.file.sha1.clone(),
                size: logging.file.size,
            };
            let mut missing = Vec::new();
            verify(&path, Some(&descriptor), true, &mut missing)?;
            if missing.is_empty() {
                return Ok(None);
            }
            let sha1 = logging
                .file
                .sha1
                .as_ref()
                .filter(|hash| hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
                .ok_or_else(|| {
                    LauncherError::InvalidMetadata("日志配置缺少有效 SHA-1，无法自动补全".into())
                })?;
            let size = logging
                .file
                .size
                .filter(|size| *size > 0 && *size <= 1024 * 1024)
                .ok_or_else(|| {
                    LauncherError::InvalidMetadata(
                        "日志配置缺少有效大小（最多 1 MiB），无法自动补全".into(),
                    )
                })?;
            let url = logging.file.url.clone().ok_or_else(|| {
                LauncherError::InvalidMetadata("日志配置缺少下载地址，无法自动补全".into())
            })?;
            Ok::<_, LauncherError>(Some(crate::DownloadFile {
                url,
                path: relative,
                sha1: Some(sha1.clone()),
                size: Some(size),
            }))
        })
        .await
        .map_err(|e| LauncherError::Internal(e.to_string()))??;
        if let Some(file) = file {
            client.download_file(&self.layout, file).await?;
        }
        Ok(())
    }
}

async fn inspect_java(options: &LaunchOptions) -> LauncherResult<JavaRuntime> {
    if options.min_memory_mb < 128
        || options.min_memory_mb > options.max_memory_mb
        || options.max_memory_mb > 1024 * 1024
    {
        return Err(LauncherError::JavaUnavailable(
            "内存范围应满足 128 MiB <= 最小值 <= 最大值 <= 1 TiB".into(),
        ));
    }
    if options.resolution.is_some_and(|(w, h)| w == 0 || h == 0) {
        return Err(LauncherError::InvalidMetadata("窗口尺寸必须大于零".into()));
    }
    JavaRuntime::inspect(&options.java).await
}

fn platform_for(java: &JavaRuntime) -> Platform {
    let mut platform = Platform::current();
    platform.version = java.os_version.clone();
    platform.arch = match java.arch.as_str() {
        "amd64" | "x86_64" => "x86_64",
        "x86" | "i386" => "x86",
        "aarch64" | "arm64" => "aarch64",
        _ => platform.arch,
    };
    platform
}

fn check_java(java: &JavaRuntime, version: &ResolvedVersion) -> LauncherResult<()> {
    if let Some(required) = version.required_java_major() {
        if java.major_version != required {
            return Err(LauncherError::JavaUnavailable(format!(
                "该版本元数据要求 Java {}，已选择 Java {}；请指定匹配的 --java",
                required, java.major_version
            )));
        }
    }
    Ok(())
}

struct Files {
    classpath: Vec<PathBuf>,
    natives: Vec<NativeArchive>,
    index_name: String,
}

fn collect_files(
    layout: &GameLayout,
    version: &ResolvedVersion,
    platform: &Platform,
    hashes: bool,
) -> LauncherResult<Files> {
    let mut missing = Vec::new();
    let mut classpath = Vec::new();
    let mut natives = Vec::new();
    let libraries = layout.libraries()?;
    for library in &version.metadata.libraries {
        if !allowed(&library.rules, platform, &HashMap::new())? {
            continue;
        }
        let downloads = library.downloads.as_ref();
        let artifact = downloads.and_then(|d| d.artifact.as_ref());
        if artifact.is_some() || (downloads.is_none() && library.natives.is_empty()) {
            let path = artifact
                .and_then(|a| a.path.clone())
                .map(Ok)
                .unwrap_or_else(|| metadata::maven_path(&library.name))?;
            let path = super::layout::checked_join(&libraries, Path::new(&path))?;
            verify(&path, artifact, hashes, &mut missing)?;
            classpath.push(path);
        }
        if let Some(classifier) = library.natives.get(platform.os) {
            let bits = if platform.arch == "x86" { "32" } else { "64" };
            let classifier = classifier.replace("${arch}", bits);
            let native = downloads
                .and_then(|d| d.classifiers.get(&classifier))
                .ok_or_else(|| {
                    LauncherError::InvalidMetadata(format!(
                        "{} 缺少 native {classifier}",
                        library.name
                    ))
                })?;
            let relative = native
                .path
                .as_ref()
                .ok_or_else(|| LauncherError::InvalidMetadata("native 缺少 path".into()))?;
            let path = super::layout::checked_join(&libraries, Path::new(relative))?;
            verify(&path, Some(native), hashes, &mut missing)?;
            natives.push(NativeArchive {
                path,
                exclude: library
                    .extract
                    .as_ref()
                    .map(|e| e.exclude.clone())
                    .unwrap_or_default(),
            });
        }
    }
    verify(
        &version.jar_path,
        version.metadata.downloads.get("client"),
        hashes,
        &mut missing,
    )?;
    classpath.push(version.jar_path.clone());
    let index_name = version
        .metadata
        .asset_index
        .as_ref()
        .map(|a| a.id.clone())
        .or_else(|| version.metadata.assets.clone())
        .ok_or_else(|| LauncherError::InvalidMetadata("缺少 assets/assetIndex".into()))?;
    super::layout::validate_id(&index_name)?;
    let index_path = layout.path(format!("assets/indexes/{index_name}.json"))?;
    let descriptor = version.metadata.asset_index.as_ref().map(|a| Artifact {
        path: None,
        sha1: a.sha1.clone(),
        size: a.size,
    });
    verify(&index_path, descriptor.as_ref(), hashes, &mut missing)?;
    if index_path.is_file() {
        #[derive(Deserialize)]
        struct Index {
            objects: HashMap<String, Object>,
            #[serde(default)]
            virtual_: bool,
            #[serde(default)]
            map_to_resources: bool,
        }
        #[derive(Deserialize)]
        struct Object {
            hash: String,
            size: u64,
        }
        // virtual 是旧版字段，单独检查以免 Rust 保留字改名影响解析。
        let raw: serde_json::Value = serde_json::from_reader(File::open(index_path)?)?;
        let index: Index = serde_json::from_value(raw.clone())?;
        if raw
            .get("virtual")
            .and_then(|v| v.as_bool())
            .unwrap_or(index.virtual_)
            || index.map_to_resources
        {
            return Err(LauncherError::Unsupported(
                "旧版 virtual/map_to_resources 资产布局尚未接入".into(),
            ));
        }
        for object in index.objects.values() {
            if object.hash.len() != 40 || !object.hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(LauncherError::InvalidMetadata("非法资源 SHA-1".into()));
            }
            let path = layout.path(format!(
                "assets/objects/{}/{}",
                &object.hash[..2],
                object.hash
            ))?;
            verify(
                &path,
                Some(&Artifact {
                    path: None,
                    sha1: Some(object.hash.clone()),
                    size: Some(object.size),
                }),
                hashes,
                &mut missing,
            )?;
        }
    }
    if let Some(logging) = version.metadata.logging.get("client") {
        super::layout::validate_id(&logging.file.id)?;
        let path = layout.path(format!("assets/log_configs/{}", logging.file.id))?;
        verify(
            &path,
            Some(&Artifact {
                path: None,
                sha1: logging.file.sha1.clone(),
                size: logging.file.size,
            }),
            hashes,
            &mut missing,
        )?;
    }
    if !missing.is_empty() {
        let total = missing.len();
        missing.truncate(12);
        return Err(LauncherError::MissingGameFiles(format!(
            "共 {total} 项异常：{}",
            missing.join("；")
        )));
    }
    Ok(Files {
        classpath,
        natives,
        index_name,
    })
}

fn verify(
    path: &Path,
    descriptor: Option<&Artifact>,
    hashes: bool,
    missing: &mut Vec<String>,
) -> LauncherResult<()> {
    let meta = match std::fs::metadata(path) {
        Ok(m) if m.is_file() => m,
        _ => {
            missing.push(path.display().to_string());
            return Ok(());
        }
    };
    if let Some(descriptor) = descriptor {
        if descriptor.size.is_some_and(|size| size != meta.len()) {
            missing.push(format!("{} 大小不匹配", path.display()));
        } else if hashes {
            if let Some(expected) = &descriptor.sha1 {
                let mut file = File::open(path)?;
                let mut digest = Sha1::new();
                let mut buffer = [0_u8; 65536];
                loop {
                    let n = file.read(&mut buffer)?;
                    if n == 0 {
                        break;
                    }
                    digest.update(&buffer[..n]);
                }
                let actual = format!("{:x}", digest.finalize());
                if !actual.eq_ignore_ascii_case(expected) {
                    missing.push(format!("{} SHA-1 不匹配", path.display()));
                }
            }
        }
    }
    Ok(())
}

fn build_plan(
    layout: &GameLayout,
    key: &str,
    java: &JavaRuntime,
    session: &AuthSession,
    options: &LaunchOptions,
) -> LauncherResult<LaunchPlan> {
    // Java libraries may reject Windows extended-length paths produced by canonicalize.
    // Only use the conventional spelling when it resolves to the same existing root.
    let java_layout = java_compatible_layout(layout)?;
    let layout = &java_layout;
    let version = metadata::resolve(layout, key)?;
    check_java(java, &version)?;
    let platform = platform_for(java);
    let files = collect_files(layout, &version, &platform, options.verify_hashes)?;
    let game_directory = layout.version_dir(key)?;
    let run = uuid::Uuid::new_v4().simple().to_string();
    let natives_directory = layout.path(format!("{key}/.launcher/natives/{run}"))?;
    let log_path = layout.path(format!("{key}/.launcher/logs/{run}.log"))?;
    let mut arguments = vec![
        format!("-Xms{}M", options.min_memory_mb),
        format!("-Xmx{}M", options.max_memory_mb),
    ];
    if let Some(logging) = version.metadata.logging.get("client") {
        super::layout::validate_id(&logging.file.id)?;
        let path = layout.path(format!("assets/log_configs/{}", logging.file.id))?;
        arguments.push(logging.argument.replace("${path}", &path.to_string_lossy()));
    }
    arguments.extend(options.extra_jvm_args.iter().cloned());
    let (parent, id) = key
        .split_once('/')
        .ok_or_else(|| LauncherError::UnsafePath(key.into()))?;
    let use_upstream_builder = parent == "versions"
        && version
            .metadata
            .arguments
            .as_ref()
            .is_some_and(|args| !args.game.is_empty());
    let game_arguments = if use_upstream_builder {
        build_upstream_arguments(
            layout,
            id,
            &game_directory,
            &natives_directory,
            session,
            options,
        )?
    } else {
        build_legacy_arguments(
            layout,
            &version,
            &files,
            &game_directory,
            &natives_directory,
            java,
            session,
            options,
        )?
    };
    arguments.extend(game_arguments);
    Ok(LaunchPlan {
        executable: java.executable.clone(),
        arguments,
        game_directory,
        natives_directory,
        log_path,
        natives: files.natives,
        secrets: if session.is_offline {
            Vec::new()
        } else {
            vec![
                session.access_token.clone(),
                session.xuid.clone(),
                session.client_id.clone(),
            ]
        },
    })
}

fn java_compatible_layout(layout: &GameLayout) -> LauncherResult<GameLayout> {
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let mut components = layout.root().components();
        let candidate = match components.next() {
            Some(Component::Prefix(prefix)) => match prefix.kind() {
                Prefix::VerbatimDisk(drive) => Some(PathBuf::from(format!("{}:\\", drive as char))),
                Prefix::VerbatimUNC(server, share) => {
                    let mut path = PathBuf::from(r"\\");
                    path.push(server);
                    path.push(share);
                    Some(path)
                }
                _ => None,
            },
            _ => None,
        };
        if let Some(mut candidate) = candidate {
            for component in components {
                if let Component::Normal(part) = component {
                    candidate.push(part);
                }
            }
            if let (Ok(original), Ok(converted)) =
                (layout.root().canonicalize(), candidate.canonicalize())
            {
                if original == converted {
                    return GameLayout::new(candidate);
                }
            }
        }
    }
    Ok(layout.clone())
}

#[cfg(all(test, windows))]
mod windows_path_tests {
    use super::*;

    #[test]
    fn canonical_game_root_is_converted_to_an_equivalent_java_path() {
        let root = std::env::temp_dir().canonicalize().unwrap();
        assert!(root.to_string_lossy().starts_with(r"\\?\"));
        let layout = GameLayout::new(&root).unwrap();
        let converted = java_compatible_layout(&layout).unwrap();
        assert!(!converted.root().to_string_lossy().starts_with(r"\\?\"));
        assert_eq!(converted.root().canonicalize().unwrap(), root);
    }
}

fn build_upstream_arguments(
    layout: &GameLayout,
    id: &str,
    game_directory: &Path,
    natives_directory: &Path,
    session: &AuthSession,
    options: &LaunchOptions,
) -> LauncherResult<Vec<String>> {
    let launcher = mc_launcher_core::launcher::Launcher::new(layout.root());
    let mut version = launcher
        .load_version(id)
        .map_err(|e| LauncherError::InvalidMetadata(e.to_string()))?;
    let uuid = session.uuid.replace('-', "");
    patch_account_placeholders(&mut version, session, &uuid);
    let account = if session.is_offline {
        mc_launcher_core::account::Account::Offline {
            username: session.username.clone(),
            uuid,
        }
    } else {
        mc_launcher_core::account::Account::Microsoft {
            username: session.username.clone(),
            uuid,
            access_token: session.access_token.clone(),
        }
    };
    let command = launcher
        .build_launch_command_from_version(
            &version,
            mc_launcher_core::command::builder::LaunchOptions {
                account,
                java_executable: Some(options.java.clone()),
                game_directory: Some(game_directory.to_path_buf()),
                natives_directory: Some(natives_directory.to_path_buf()),
                launcher_name: "mc-ai-launcher".into(),
                launcher_version: env!("CARGO_PKG_VERSION").into(),
                custom_resolution: options.resolution,
                ..Default::default()
            },
        )
        .map_err(|e| LauncherError::InvalidMetadata(e.to_string()))?;
    Ok(command.args)
}

fn patch_account_placeholders(
    version: &mut mc_launcher_core::core::version::VersionJson,
    session: &AuthSession,
    uuid: &str,
) {
    let replacements = [
        (
            "${auth_session}",
            format!("token:{}:{uuid}", session.access_token),
        ),
        ("${auth_xuid}", session.xuid.clone()),
        ("${clientid}", session.client_id.clone()),
    ];
    fn patch(value: &mut serde_json::Value, replacements: &[(&str, String)]) {
        match value {
            serde_json::Value::String(text) => {
                for (key, replacement) in replacements {
                    *text = text.replace(key, replacement);
                }
            }
            serde_json::Value::Array(values) => {
                for value in values {
                    patch(value, replacements);
                }
            }
            serde_json::Value::Object(values) => {
                for value in values.values_mut() {
                    patch(value, replacements);
                }
            }
            _ => {}
        }
    }
    let Ok(mut json) = serde_json::to_value(&*version) else {
        return;
    };
    if let Some(arguments) = json.get_mut("arguments") {
        patch(arguments, &replacements);
    }
    if let Some(arguments) = json.get_mut("minecraftArguments") {
        patch(arguments, &replacements);
    }
    if let Ok(patched) = serde_json::from_value(json) {
        *version = patched;
    }
}

fn build_legacy_arguments(
    layout: &GameLayout,
    version: &ResolvedVersion,
    files: &Files,
    game_directory: &Path,
    natives_directory: &Path,
    java: &JavaRuntime,
    session: &AuthSession,
    options: &LaunchOptions,
) -> LauncherResult<Vec<String>> {
    let separator = if java.os_version.to_lowercase().contains("windows") {
        ";"
    } else {
        ":"
    };
    let classpath = files
        .classpath
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join(separator);
    let variables = HashMap::from([
        ("auth_player_name", session.username.clone()),
        ("auth_uuid", session.uuid.replace('-', "")),
        ("auth_access_token", session.access_token.clone()),
        (
            "auth_session",
            format!(
                "token:{}:{}",
                session.access_token,
                session.uuid.replace('-', "")
            ),
        ),
        ("auth_xuid", session.xuid.clone()),
        ("clientid", session.client_id.clone()),
        (
            "user_type",
            if session.is_offline {
                "Legacy".into()
            } else {
                "msa".into()
            },
        ),
        ("user_properties", "{}".into()),
        ("version_name", version.metadata.id.clone()),
        (
            "version_type",
            version
                .metadata
                .version_type
                .clone()
                .unwrap_or_else(|| "release".into()),
        ),
        (
            "game_directory",
            game_directory.to_string_lossy().into_owned(),
        ),
        (
            "assets_root",
            layout.assets()?.to_string_lossy().into_owned(),
        ),
        ("assets_index_name", files.index_name.clone()),
        (
            "natives_directory",
            natives_directory.to_string_lossy().into_owned(),
        ),
        (
            "library_directory",
            layout.libraries()?.to_string_lossy().into_owned(),
        ),
        ("classpath", classpath),
        ("classpath_separator", separator.into()),
        ("launcher_name", "mc-ai-launcher".into()),
        ("launcher_version", env!("CARGO_PKG_VERSION").into()),
    ]);
    let platform = platform_for(java);
    let features = HashMap::from([("has_custom_resolution".into(), options.resolution.is_some())]);
    let modern = version.metadata.arguments.as_ref();
    let mut arguments = Vec::new();
    let jvm = modern
        .map(|a| expand(&a.jvm, &platform, &features, &variables))
        .transpose()?
        .unwrap_or_default();
    if jvm.is_empty() {
        arguments.push(format!(
            "-Djava.library.path={}",
            natives_directory.display()
        ));
        arguments.extend(["-cp".into(), variables["classpath"].clone()]);
    } else {
        arguments.extend(jvm);
    }
    arguments.push(version.metadata.main_class.clone().unwrap_or_default());
    if let Some(modern) = modern.filter(|a| !a.game.is_empty()) {
        arguments.extend(expand(&modern.game, &platform, &features, &variables)?);
    } else if let Some(legacy) = &version.metadata.minecraft_arguments {
        for arg in tokenize_legacy(legacy)? {
            arguments.push(substitute(&arg, &variables)?);
        }
    } else {
        return Err(LauncherError::InvalidMetadata("缺少游戏参数".into()));
    }
    if let Some((w, h)) = options.resolution {
        arguments.extend([
            "--width".into(),
            w.to_string(),
            "--height".into(),
            h.to_string(),
        ]);
    }
    Ok(arguments)
}

fn expand(
    args: &[Argument],
    platform: &Platform,
    features: &HashMap<String, bool>,
    vars: &HashMap<&str, String>,
) -> LauncherResult<Vec<String>> {
    let mut result = Vec::new();
    for arg in args {
        match arg {
            Argument::Plain(s) => result.push(substitute(s, vars)?),
            Argument::Conditional { rules, value } if allowed(rules, platform, features)? => {
                match value {
                    ArgumentValue::One(s) => result.push(substitute(s, vars)?),
                    ArgumentValue::Many(values) => {
                        for s in values {
                            result.push(substitute(s, vars)?);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(result)
}

fn substitute(template: &str, vars: &HashMap<&str, String>) -> LauncherResult<String> {
    let mut output = String::new();
    let mut rest = template;
    while let Some(start) = rest.find("${") {
        output.push_str(&rest[..start]);
        let tail = &rest[start + 2..];
        let end = tail
            .find('}')
            .ok_or_else(|| LauncherError::InvalidMetadata("未闭合的参数占位符".into()))?;
        let key = &tail[..end];
        output.push_str(
            vars.get(key)
                .ok_or_else(|| LauncherError::Unsupported(format!("启动参数占位符 {key}")))?,
        );
        rest = &tail[end + 1..];
    }
    output.push_str(rest);
    Ok(output)
}

pub(crate) fn tokenize_legacy(input: &str) -> LauncherResult<Vec<String>> {
    let mut args = Vec::new();
    let mut token = String::new();
    let mut quote = None;
    let mut started = false;
    for c in input.chars() {
        if c == '"' || c == '\'' {
            if quote == Some(c) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(c);
            } else {
                token.push(c);
            }
            started = true;
        } else if c.is_whitespace() && quote.is_none() {
            if started {
                args.push(std::mem::take(&mut token));
                started = false;
            }
        } else {
            token.push(c);
            started = true;
        }
    }
    if quote.is_some() {
        return Err(LauncherError::InvalidMetadata(
            "旧式游戏参数引号未闭合".into(),
        ));
    }
    if started {
        args.push(token);
    }
    Ok(args)
}
