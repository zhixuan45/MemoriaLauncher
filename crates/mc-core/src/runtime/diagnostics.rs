//! Deterministic local log analysis. Evidence is data, never an executable repair.
use super::{logs, GameLayout};
use crate::{LauncherError, LauncherResult};
use regex::Regex;
use serde::Serialize;
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::OnceLock,
};

pub const MAX_ANALYSIS_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub category: String,
    pub severity: String,
    pub confidence: String,
    pub title: String,
    pub evidence: Vec<String>,
    pub mod_id: Option<String>,
    pub required_mod_id: Option<String>,
    pub required_version: Option<String>,
    pub suggested_next_action: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Analysis {
    pub findings: Vec<Finding>,
    pub inspected_lines: usize,
    pub truncated: bool,
    pub redacted_lines: usize,
    pub exit_code: Option<i32>,
}

fn rules() -> &'static Vec<(&'static str, &'static str, &'static str, Regex)> {
    static RULES: OnceLock<Vec<(&str, &str, &str, Regex)>> = OnceLock::new();
    RULES.get_or_init(|| [
        ("missing_dependency", "缺少必需依赖", "核对所列 Mod 的必需依赖、游戏版本和加载器后重新生成安装预览。", r"(?i)(requires.*(?:missing|not installed|is absent)|depends on.*(?:missing|not installed)|missing (?:or unsupported )?(?:mandatory )?dependenc|requires .*version .*of .*, which is missing)"),
        ("loader_mismatch", "加载器或 Mod 版本不兼容", "核对 Minecraft、加载器和 Mod 支持的版本，勿混用不同加载器的文件。", r"(?i)(incompatible mod set|incompatible mods found|requires.*(?:fabricloader|fabric loader|quilt_loader|minecraft|neoforge|forge).*but|requires .*version .*of .*but only|mod .*requires .*minecraft|unsupported.*(?:loader|minecraft).*version)"),
        ("java_version", "Java 版本不兼容", "按实例声明的 Java 要求选择运行环境；class file 52/61/65 分别对应 Java 8/17/21。", r"(?i)(UnsupportedClassVersionError|class file version.*(?:recognizes|supported)|requires java|java.*version.*(?:not supported|unsupported)|Unsupported major.minor version)"),
        ("out_of_memory", "内存不足或分配失败", "检查可用内存、Java 位数和实例分配值；不要直接将内存调到系统总量。", r"(?i)(OutOfMemoryError|Could not reserve enough space|Cannot allocate memory|unable to create native thread|Native memory allocation.*failed)"),
        ("hash_mismatch", "文件完整性校验失败", "核对文件来源并重新下载受影响文件，保留旧文件供检查。", r"(?i)(hash mismatch|checksum.*(?:mismatch|failed|invalid)|sha-?(?:1|256|512).*(?:mismatch|does not match)|文件校验哈希不匹配)"),
        ("mixin_conflict", "Mixin 注入失败，需人工确认", "结合完整 Caused by 链及涉及的 Mod 版本定位；不要据此自动删除 Mod。", r"(?i)(MixinApplyError|MixinTransformerError|InvalidMixinException|InjectionError|Critical injection failure|Mixin apply.*failed)"),
        ("class_loading", "类加载失败，需人工确认", "检查缺失类对应的依赖和加载器版本；仅凭类名不能确定应删除哪个 Mod。", r"(?i)(NoClassDefFoundError|ClassNotFoundException|NoSuchMethodError|NoSuchFieldError|LinkageError)"),
        ("graphics_driver", "图形环境初始化失败", "检查显卡驱动、OpenGL 支持及远程桌面环境。", r"(?i)(GLFW error 6554[23]|does not support OpenGL|Failed to create window|Pixel format not accelerated|Could not initialize.*GLFW)"),
        ("network", "网络或证书连接失败", "检查源站可用性、代理和系统时间；保留 TLS 验证。", r"(?i)(UnknownHostException|ConnectException|SocketTimeoutException|SSLHandshakeException|PKIX path building failed|Connection timed out)"),
        ("jvm_crash", "JVM 原生崩溃", "检查 hs_err 日志中的 Problematic frame、驱动及 Java 版本，保留完整崩溃报告。", r"(?i)(A fatal error has been detected by the Java Runtime|EXCEPTION_ACCESS_VIOLATION|SIGSEGV.*(?:0x|pc=)|Problematic frame:)"),
    ].into_iter().map(|(id,title,action,pattern)| (id,title,action,Regex::new(pattern).expect("diagnostic rule"))).collect())
}

pub fn analyze_text(text: &str, exit_code: Option<i32>) -> Analysis {
    let truncated = text.len() > MAX_ANALYSIS_BYTES;
    let text = if truncated {
        let mut start = text.len() - MAX_ANALYSIS_BYTES;
        while !text.is_char_boundary(start) {
            start += 1;
        }
        text[start..]
            .split_once('\n')
            .map(|(_, tail)| tail)
            .unwrap_or("")
    } else {
        text
    };
    let (text, redacted_lines) = logs::redact(text);
    static PATHS: OnceLock<Regex> = OnceLock::new();
    let paths = PATHS.get_or_init(|| Regex::new(r#"(?i)(?:[a-z]:[\\/][^\s"'<>]+|/(?:home|users|workspace|tmp|opt|mnt|media|var)/[^\s"'<>]+)"#).unwrap());
    let text = paths.replace_all(&text, "[LOCAL_PATH]");
    static EXIT: OnceLock<Regex> = OnceLock::new();
    let exit_pattern=EXIT.get_or_init(||Regex::new(r"(?im)(?:process exited with code|game exited with code|exit_code|退出代码|退出码)\s*[:=]?\s*(-?\d+)").unwrap());
    let exit_code = exit_code.or_else(|| {
        exit_pattern
            .captures_iter(&text)
            .filter_map(|capture| capture[1].parse::<i32>().ok())
            .last()
    });
    let lines: Vec<_> = text.lines().collect();
    let mut findings = vec![];
    for (id, title, action, rule) in rules() {
        let matching: Vec<_> = lines
            .iter()
            .filter(|line| rule.is_match(line))
            .take(3)
            .collect();
        if matching.is_empty() {
            continue;
        }
        let evidence = matching
            .iter()
            .map(|line| {
                paths
                    .replace_all(line, "[LOCAL_PATH]")
                    .chars()
                    .take(500)
                    .collect()
            })
            .collect();
        let mut finding = Finding {
            category: (*id).into(),
            severity: "error".into(),
            confidence: if matches!(*id, "mixin_conflict" | "class_loading" | "loader_mismatch") {
                "medium"
            } else {
                "high"
            }
            .into(),
            title: (*title).into(),
            evidence,
            mod_id: None,
            required_mod_id: None,
            required_version: None,
            suggested_next_action: (*action).into(),
        };
        if *id == "missing_dependency" {
            static DEP: OnceLock<Regex> = OnceLock::new();
            let dep = DEP.get_or_init(|| Regex::new(r"(?i)Mod .+? \(([^()\s]+)\).*?requires (.+?) of (?:mod )?'[^']+' \(([^()\s]+)\)").unwrap());
            if let Some(c) = matching.iter().find_map(|line| dep.captures(line)) {
                finding.mod_id = Some(c[1].chars().take(100).collect());
                finding.required_version = Some(c[2].chars().take(200).collect());
                finding.required_mod_id = Some(c[3].chars().take(100).collect());
            }
        }
        findings.push(finding);
    }
    if let Some(code) = exit_code.filter(|code| *code != 0) {
        findings.push(Finding {
            category: "nonzero_exit".into(),
            severity: "error".into(),
            confidence: "high".into(),
            title: format!("进程以非零代码 {code} 退出"),
            evidence: vec![format!("exit_code={code}")],
            mod_id: None,
            required_mod_id: None,
            required_version: None,
            suggested_next_action: "结合上方具体日志证据排查；退出码本身不能确定根因。".into(),
        });
    }
    if findings.is_empty() {
        findings.push(Finding {
            category: "unknown".into(),
            severity: "info".into(),
            confidence: "low".into(),
            title: "未识别到已知异常模式".into(),
            evidence: vec![],
            mod_id: None,
            required_mod_id: None,
            required_version: None,
            suggested_next_action: "这不代表运行正常；可选择其他日志或结合完整崩溃报告人工排查。"
                .into(),
        });
    }
    Analysis {
        findings,
        inspected_lines: lines.len(),
        truncated,
        redacted_lines,
        exit_code,
    }
}

pub async fn analyze_instance_log(
    layout: GameLayout,
    instance: String,
    path: String,
    exit_code: Option<i32>,
) -> LauncherResult<Analysis> {
    let log = logs::read_instance_log(layout, instance, path, logs::MAX_LOG_BYTES).await?;
    let mut analysis = analyze_text(&log.text, exit_code);
    analysis.truncated |= log.truncated;
    analysis.redacted_lines += log.redacted_lines;
    Ok(analysis)
}

pub fn analyze_file(path: &Path) -> LauncherResult<Analysis> {
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(LauncherError::UnsafePath("请选择普通日志文件".into()));
    }
    let mut file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    let start = size.saturating_sub(MAX_ANALYSIS_BYTES as u64);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = vec![];
    file.take(MAX_ANALYSIS_BYTES as u64)
        .read_to_end(&mut bytes)?;
    if bytes.contains(&0) {
        return Err(LauncherError::InvalidMetadata(
            "不支持二进制或压缩日志，请先解压为文本。".into(),
        ));
    }
    let bytes = if start > 0 {
        bytes.splitn(2, |byte| *byte == b'\n').nth(1).unwrap_or(&[])
    } else {
        &bytes
    };
    let mut analysis = analyze_text(&String::from_utf8_lossy(bytes), None);
    analysis.truncated |= start > 0;
    Ok(analysis)
}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;
