use mc_core::JavaRuntime;
use std::{
    collections::{HashSet, VecDeque},
    fmt,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JavaChoice {
    Runtime {
        executable: PathBuf,
        major: Option<u32>,
        version: String,
        arch: String,
    },
    Import,
}

impl JavaChoice {
    pub fn configured(path: PathBuf) -> Self {
        Self::Runtime {
            executable: path,
            major: None,
            version: String::new(),
            arch: String::new(),
        }
    }

    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Runtime { executable, .. } => Some(executable),
            Self::Import => None,
        }
    }

    pub fn from_runtime(runtime: JavaRuntime) -> Self {
        Self::Runtime {
            executable: runtime.executable,
            major: Some(runtime.major_version),
            version: runtime.version,
            arch: runtime.arch,
        }
    }
}

impl fmt::Display for JavaChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Import => f.write_str("导入电脑中已有的 Java…"),
            Self::Runtime {
                executable,
                major,
                version,
                ..
            } => {
                let full = executable.to_string_lossy();
                let full = full.strip_prefix(r"\\?\").unwrap_or(&full);
                let chars: Vec<_> = full.chars().collect();
                let shortened = if chars.len() > 68 {
                    format!(
                        "{}…{}",
                        chars[..35].iter().collect::<String>(),
                        chars[chars.len() - 28..].iter().collect::<String>()
                    )
                } else {
                    full.to_string()
                };
                match major {
                    Some(major) => write!(f, "Java {major} ({version})：{shortened}"),
                    None => write!(f, "已配置：{shortened}"),
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct JavaDiscovery {
    pub runtimes: Vec<JavaRuntime>,
    pub failures: Vec<String>,
}

pub fn executable(path: &Path) -> PathBuf {
    let name = if cfg!(windows) { "java.exe" } else { "java" };
    let path = if path.is_dir() {
        if path.join(name).is_file() {
            path.join(name)
        } else {
            path.join("bin").join(name)
        }
    } else if path
        .file_name()
        .is_some_and(|file| file.eq_ignore_ascii_case("javaw.exe"))
    {
        path.with_file_name("java.exe")
    } else if !path.is_absolute() && path.components().count() == 1 {
        std::env::var_os("PATH")
            .and_then(|paths| {
                std::env::split_paths(&paths)
                    .map(|directory| directory.join(path))
                    .flat_map(|candidate| [candidate.clone(), candidate.with_extension("exe")])
                    .find(|candidate| candidate.is_file())
            })
            .unwrap_or_else(|| path.to_path_buf())
    } else {
        path.to_path_buf()
    };
    path.canonicalize().unwrap_or(path)
}

pub fn key(path: &Path) -> String {
    let path = executable(path).to_string_lossy().into_owned();
    if cfg!(windows) {
        path.to_lowercase()
    } else {
        path
    }
}

fn candidates(roots: Vec<PathBuf>, explicit: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut result = Vec::new();
    let mut seen_files = HashSet::new();
    let mut add = |path: PathBuf| {
        let path = executable(&path);
        if path.is_file() && seen_files.insert(key(&path)) {
            result.push(path);
        }
    };
    for path in explicit {
        add(path);
    }
    let mut directories: VecDeque<_> = roots.into_iter().map(|root| (root, 0)).collect();
    let mut seen_directories = HashSet::new();
    let mut visited = 0;
    while let Some((directory, depth)) = directories.pop_front() {
        if visited >= 2048 {
            break;
        }
        let Ok(canonical) = directory.canonicalize() else {
            continue;
        };
        if !seen_directories.insert(canonical) {
            continue;
        }
        visited += 1;
        add(directory.clone());
        if depth >= 5 {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(directory) {
            for entry in entries.flatten() {
                if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    continue;
                }
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if [
                    "lib",
                    "include",
                    "jmods",
                    "legal",
                    "conf",
                    "assets",
                    "libraries",
                    "versions",
                    "mods",
                    "logs",
                    "saves",
                    "target",
                    ".git",
                    "node_modules",
                ]
                .contains(&name.as_str())
                {
                    continue;
                }
                directories.push_back((entry.path(), depth + 1));
            }
        }
    }
    result.truncate(96);
    result
}

fn search_paths(
    game_roots: Vec<PathBuf>,
    mut explicit: Vec<PathBuf>,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut roots = Vec::new();
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives},
            System::WindowsProgramming::DRIVE_FIXED,
        };
        // Only inspect common Minecraft folders on fixed local disks, never network/removable drives.
        let drives = unsafe { GetLogicalDrives() };
        for index in 0..26 {
            if drives & (1 << index) == 0 {
                continue;
            }
            let path = format!("{}:\\", (b'A' + index as u8) as char);
            let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
            if unsafe { GetDriveTypeW(wide.as_ptr()) } == DRIVE_FIXED {
                roots.push(PathBuf::from(&path).join("Minecraft"));
                roots.push(PathBuf::from(path).join("Games/Minecraft"));
            }
        }
    }
    for variable in ["JAVA_HOME", "JDK_HOME"] {
        if let Some(home) = std::env::var_os(variable) {
            explicit.push(PathBuf::from(home));
        }
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&paths) {
            explicit.push(directory.join(if cfg!(windows) { "java.exe" } else { "java" }));
        }
    }
    for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(directory) = std::env::var_os(variable) {
            let directory = PathBuf::from(directory);
            for vendor in [
                "Java",
                "Zulu",
                "Eclipse Adoptium",
                "Eclipse Foundation",
                "AdoptOpenJDK",
                "Microsoft",
                "Amazon Corretto",
                "BellSoft",
                "Semeru",
            ] {
                roots.push(directory.join(vendor));
            }
        }
    }
    if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
        let home = PathBuf::from(home);
        for relative in [
            ".jdks",
            ".jabba/jdk",
            ".sdkman/candidates/java",
            "apps/minecraft",
            ".minecraft/runtime",
        ] {
            roots.push(home.join(relative));
        }
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        roots.push(PathBuf::from(appdata).join(".minecraft/runtime"));
    }
    if let Ok(path) = std::env::current_exe() {
        if let Some(parent) = path.parent() {
            roots.push(parent.join("runtime"));
            roots.push(parent.join("java"));
        }
    }
    for root in game_roots {
        roots.push(root.join("runtime"));
        if let Some(parent) = root.parent() {
            if let Ok(entries) = std::fs::read_dir(parent) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_lowercase();
                    if [
                        "java", "jdk", "jre", "zulu", "temurin", "runtime", "corretto", "graal",
                        "semeru",
                    ]
                    .iter()
                    .any(|prefix| name.starts_with(prefix))
                    {
                        roots.push(entry.path());
                    }
                }
            }
        }
    }
    if !cfg!(windows) {
        for path in [
            "/usr/lib/jvm",
            "/usr/java",
            "/Library/Java/JavaVirtualMachines",
        ] {
            roots.push(path.into());
        }
    }
    (roots, explicit)
}

pub async fn discover(
    game_roots: Vec<PathBuf>,
    explicit: Vec<PathBuf>,
) -> Result<JavaDiscovery, String> {
    let candidates = tokio::task::spawn_blocking(move || {
        let (roots, explicit) = search_paths(game_roots, explicit);
        candidates(roots, explicit)
    })
    .await
    .map_err(|error| error.to_string())?;
    let mut report = JavaDiscovery {
        runtimes: Vec::new(),
        failures: Vec::new(),
    };
    let mut remaining = candidates.into_iter();
    let mut running = tokio::task::JoinSet::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    loop {
        while running.len() < 4 {
            let Some(path) = remaining.next() else {
                break;
            };
            running.spawn(async move {
                JavaRuntime::inspect(&path)
                    .await
                    .map_err(|error| format!("{}：{error}", path.display()))
            });
        }
        match tokio::time::timeout_at(deadline, running.join_next()).await {
            Ok(Some(Ok(Ok(runtime)))) => report.runtimes.push(runtime),
            Ok(Some(Ok(Err(error)))) => report.failures.push(error),
            Ok(Some(Err(error))) => report.failures.push(error.to_string()),
            Ok(None) => break,
            Err(_) => {
                report
                    .failures
                    .push("扫描达到 30 秒上限，已保留检测成功的 Java。".into());
                break;
            }
        }
    }
    report.runtimes.sort_by(|a, b| {
        b.major_version
            .cmp(&a.major_version)
            .then(a.executable.cmp(&b.executable))
    });
    Ok(report)
}

pub async fn inspect(path: PathBuf) -> Result<JavaRuntime, String> {
    let path = tokio::task::spawn_blocking(move || executable(&path))
        .await
        .map_err(|error| error.to_string())?;
    JavaRuntime::inspect(path)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_nested_and_imported_java_without_duplicates_or_scanning_library_trees() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/java-discovery-tests")
            .join(uuid::Uuid::new_v4().to_string());
        let name = if cfg!(windows) { "java.exe" } else { "java" };
        let first = root.join("Zulu/jdk-21/bin");
        let second = root.join("runtime/java-runtime/windows/java-runtime/bin");
        let ignored = root.join("assets/ignored/bin");
        for directory in [&first, &second, &ignored] {
            std::fs::create_dir_all(directory).unwrap();
            std::fs::write(directory.join(name), b"fixture").unwrap();
        }
        let found = candidates(
            vec![root.clone(), root.join("Zulu")],
            vec![first.join(name), first.join(name)],
        );
        assert_eq!(found.len(), 2);
        assert!(!found.iter().any(|path| path.starts_with(&ignored)));
        assert_eq!(
            executable(first.parent().unwrap()),
            executable(&first.join(name))
        );
        if cfg!(windows) {
            assert_eq!(
                executable(&first.join("javaw.exe")),
                executable(&first.join("java.exe"))
            );
        }
    }

    #[tokio::test]
    #[ignore = "Requires installed Java runtimes"]
    async fn detects_real_installed_java_versions_and_system_memory() {
        let report = discover(Vec::new(), vec![PathBuf::from("java")])
            .await
            .unwrap();
        for runtime in &report.runtimes {
            eprintln!(
                "Java {} ({}) · {} · {}",
                runtime.major_version,
                runtime.version,
                runtime.arch,
                runtime.executable.display()
            );
        }
        eprintln!("Discovery issues: {:?}", report.failures);
        assert!(!report.runtimes.is_empty());
        assert!(report
            .runtimes
            .iter()
            .all(|runtime| !runtime.version.is_empty()));
        let memory = crate::runtime_memory::MemorySnapshot::read();
        eprintln!(
            "System memory: total={} MiB, available={} MiB",
            memory.total_mb, memory.available_mb
        );
        assert!(memory.total_mb > 0 && memory.available_mb <= memory.total_mb);
    }
}
