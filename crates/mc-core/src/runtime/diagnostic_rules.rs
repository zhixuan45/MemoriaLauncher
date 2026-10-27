//! Validated declarative rule packs. Bounded regex matching never executes log text or repair code.
use super::diagnostics::Finding;
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
    sync::OnceLock,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub category: String,
    pub title: String,
    pub action: String,
    pub severity: String,
    pub confidence: String,
    #[serde(default)]
    pub priority: i16,
    pub any: Vec<String>,
    #[serde(default)]
    pub all: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub context_lines: usize,
    #[serde(default)]
    pub capture_fields: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Document {
    pub version: u32,
    pub rules: Vec<Rule>,
}
#[derive(Debug, Clone)]
struct Compiled {
    rule: Rule,
    any: Vec<Regex>,
    all: Vec<Regex>,
    exclude: Vec<Regex>,
}
#[derive(Debug, Clone)]
pub struct RuleSet {
    rules: Vec<Compiled>,
}
impl RuleSet {
    pub fn builtin() -> &'static Self {
        static RULES: OnceLock<RuleSet> = OnceLock::new();
        RULES.get_or_init(|| {
            Self::compile(serde_json::from_str(include_str!("diagnostic-rules.json")).unwrap())
                .expect("built-in diagnostic rules")
        })
    }
    pub fn custom(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 128 * 1024 {
            return Err("规则文件超过 128 KiB".into());
        }
        let mut document: Document =
            serde_json::from_slice(bytes).map_err(|e| format!("规则 JSON 无效：{e}"))?;
        if document.version != 1 {
            return Err("不支持的诊断规则版本".into());
        }
        let mut builtin: Document =
            serde_json::from_str(include_str!("diagnostic-rules.json")).unwrap();
        builtin.rules.append(&mut document.rules);
        Self::compile(builtin)
    }
    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::builtin().clone()),
            Err(e) => Err(e.to_string()),
            Ok(meta)
                if !meta.is_file() || meta.len() > 128 * 1024 || meta.file_type().is_symlink() =>
            {
                Err("规则路径类型或大小无效".into())
            }
            Ok(_) => {
                use std::io::Read;
                let mut bytes = vec![];
                std::fs::File::open(path)
                    .map_err(|e| e.to_string())?
                    .take(128 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                Self::custom(&bytes)
            }
        }
    }
    fn compile(mut document: Document) -> Result<Self, String> {
        if document.version != 1 || document.rules.len() > 64 {
            return Err("规则版本或数量无效（合计最多 64 条）".into());
        }
        let mut ids = HashSet::new();
        let mut count = 0;
        let mut compiled = vec![];
        document
            .rules
            .sort_by(|a, b| b.priority.cmp(&a.priority).then(a.id.cmp(&b.id)));
        for rule in document.rules {
            if !identifier(&rule.id)
                || !identifier(&rule.category)
                || !ids.insert(rule.id.clone())
                || rule.title.len() > 300
                || rule.action.len() > 2000
                || !(-100..=100).contains(&rule.priority)
                || rule.context_lines > 8
                || rule.any.is_empty()
                || rule.any.len() > 4
                || rule.all.len() > 4
                || rule.exclude.len() > 4
                || !["info", "warning", "error"].contains(&rule.severity.as_str())
                || !["low", "medium", "high"].contains(&rule.confidence.as_str())
            {
                return Err(format!("规则 {} 的标识、字段或数量无效", rule.id));
            }
            let mut build = |patterns: &[String]| -> Result<Vec<Regex>, String> {
                patterns
                    .iter()
                    .map(|pattern| {
                        count += 1;
                        if count > 128 || pattern.len() > 2048 {
                            return Err("规则正则数量或长度超限".into());
                        }
                        RegexBuilder::new(pattern)
                            .size_limit(256 * 1024)
                            .dfa_size_limit(128 * 1024)
                            .nest_limit(32)
                            .build()
                            .map_err(|_| format!("规则 {} 正则无效或超出编译预算", rule.id))
                    })
                    .collect()
            };
            let any = build(&rule.any)?;
            let all = build(&rule.all)?;
            let exclude = build(&rule.exclude)?;
            for name in &rule.capture_fields {
                if ![
                    "mod_id",
                    "required_mod_id",
                    "required_version",
                    "exception",
                    "class_name",
                    "required_java",
                    "java_version",
                    "file",
                ]
                .contains(&name.as_str())
                    || !any
                        .iter()
                        .chain(&all)
                        .any(|pattern| pattern.capture_names().flatten().any(|group| group == name))
                {
                    return Err(format!("规则 {} 提取字段无效：{name}", rule.id));
                }
            }
            compiled.push(Compiled {
                rule,
                any,
                all,
                exclude,
            });
        }
        Ok(Self { rules: compiled })
    }
    pub fn len(&self) -> usize {
        self.rules.len()
    }
    pub(crate) fn apply(&self, text: &str) -> (Vec<Finding>, bool) {
        let mut starts = vec![0];
        starts.extend(text.match_indices('\n').map(|(index, _)| index + 1));
        let mut findings = vec![];
        let mut limited = false;
        for compiled in &self.rules {
            let mut evidence = vec![];
            let mut source_lines = vec![];
            let mut captures = BTreeMap::new();
            let mut seen = HashSet::new();
            let mut count = 0;
            for anchor in &compiled.any {
                for matched in anchor.find_iter(text) {
                    count += 1;
                    if count > 256 {
                        limited = true;
                        break;
                    }
                    let line = starts
                        .partition_point(|start| *start <= matched.start())
                        .saturating_sub(1);
                    let endline = starts
                        .partition_point(|start| *start < matched.end())
                        .saturating_sub(1);
                    let begin = starts[line.saturating_sub(compiled.rule.context_lines)];
                    let end = starts
                        .get(endline + compiled.rule.context_lines + 1)
                        .copied()
                        .unwrap_or(text.len());
                    if end - begin > 16 * 1024 {
                        limited = true;
                        continue;
                    }
                    let snippet = &text[begin..end];
                    if compiled
                        .all
                        .iter()
                        .any(|pattern| !pattern.is_match(snippet))
                        || compiled
                            .exclude
                            .iter()
                            .any(|pattern| pattern.is_match(snippet))
                    {
                        continue;
                    }
                    if !seen.insert(line) {
                        continue;
                    }
                    if evidence.len() < 3 {
                        evidence.push(snippet.trim().chars().take(500).collect());
                        source_lines.push(line + 1);
                    }
                    for pattern in compiled.any.iter().chain(&compiled.all) {
                        if let Some(found) = pattern.captures(snippet) {
                            for name in &compiled.rule.capture_fields {
                                if let Some(value) = found.name(name) {
                                    captures.entry(name.clone()).or_insert_with(|| {
                                        value.as_str().chars().take(200).collect::<String>()
                                    });
                                }
                            }
                        }
                    }
                }
            }
            if evidence.is_empty() {
                continue;
            }
            findings.push(Finding {
                rule_id: compiled.rule.id.clone(),
                category: compiled.rule.category.clone(),
                title: compiled.rule.title.clone(),
                severity: compiled.rule.severity.clone(),
                confidence: compiled.rule.confidence.clone(),
                mod_id: captures.get("mod_id").cloned(),
                required_mod_id: captures.get("required_mod_id").cloned(),
                required_version: captures.get("required_version").cloned(),
                evidence,
                suggested_next_action: compiled.rule.action.clone(),
                context: captures,
                source_lines,
            });
        }
        (findings, limited)
    }
}
pub fn import_custom(source: &Path, destination: &Path) -> Result<String, String> {
    use std::io::{Read, Write};
    let meta = std::fs::symlink_metadata(source).map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 128 * 1024 {
        return Err("请选择 128 KiB 以内的普通规则 JSON".into());
    }
    let mut bytes = vec![];
    std::fs::File::open(source)
        .map_err(|e| e.to_string())?
        .take(128 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    let rules = RuleSet::custom(&bytes)?;
    let parent = destination.parent().ok_or("规则保存路径无效")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temporary = parent.join(format!("diagnostic-rules-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> std::io::Result<()> {
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        output.write_all(&bytes)?;
        output.sync_all()?;
        drop(output);
        std::fs::rename(&temporary, destination)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result.map_err(|e| e.to_string())?;
    Ok(format!("规则已保存，共 {} 条（含内置规则）", rules.len()))
}
pub fn restore_builtin(path: &Path) -> Result<String, String> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.to_string()),
        Ok(_) => {
            let backup = path.with_extension(format!("{}.bak", uuid::Uuid::new_v4()));
            std::fs::rename(path, backup).map_err(|e| e.to_string())?;
        }
    }
    Ok("已恢复内置规则；原自定义规则已保留为备份".into())
}
fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}

#[cfg(test)]
#[path = "diagnostic_rules_tests.rs"]
mod tests;
