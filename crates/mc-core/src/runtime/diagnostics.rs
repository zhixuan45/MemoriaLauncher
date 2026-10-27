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
    pub rule_id: String,
    pub source_lines: Vec<usize>,
    pub context: std::collections::BTreeMap<String, String>,
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
    pub rule_limit_reached: bool,
    pub rules_applied: usize,
    pub inspected_lines: usize,
    pub truncated: bool,
    pub redacted_lines: usize,
    pub exit_code: Option<i32>,
}

pub fn analyze_text(text: &str, exit_code: Option<i32>) -> Analysis {
    analyze_with_rules(text, exit_code, super::diagnostic_rules::RuleSet::builtin())
}

pub fn analyze_with_rules(
    text: &str,
    exit_code: Option<i32>,
    rules: &super::diagnostic_rules::RuleSet,
) -> Analysis {
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
    let (mut findings, rule_limit_reached) = rules.apply(&text);
    if let Some(code) = exit_code.filter(|code| *code != 0) {
        findings.push(Finding {
            rule_id: "process_status".into(),
            source_lines: vec![],
            context: Default::default(),
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
            rule_id: "unmatched".into(),
            source_lines: vec![],
            context: Default::default(),
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
        rule_limit_reached,
        rules_applied: rules.len(),
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
    analyze_instance_with_rules(
        layout,
        instance,
        path,
        exit_code,
        super::diagnostic_rules::RuleSet::builtin(),
    )
    .await
}
pub async fn analyze_instance_with_rules(
    layout: GameLayout,
    instance: String,
    path: String,
    exit_code: Option<i32>,
    rules: &super::diagnostic_rules::RuleSet,
) -> LauncherResult<Analysis> {
    let log = logs::read_instance_log(layout, instance, path, logs::MAX_LOG_BYTES).await?;
    let mut analysis = analyze_with_rules(&log.text, exit_code, rules);
    analysis.truncated |= log.truncated;
    analysis.redacted_lines += log.redacted_lines;
    Ok(analysis)
}

pub fn analyze_file(path: &Path) -> LauncherResult<Analysis> {
    analyze_file_with_rules(path, super::diagnostic_rules::RuleSet::builtin())
}
pub fn analyze_file_with_rules(
    path: &Path,
    rules: &super::diagnostic_rules::RuleSet,
) -> LauncherResult<Analysis> {
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
    let mut analysis = analyze_with_rules(&String::from_utf8_lossy(bytes), None, rules);
    analysis.truncated |= start > 0;
    Ok(analysis)
}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;
