use crate::{LauncherError, LauncherResult};
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Deserialize)]
pub struct Rule {
    pub action: String,
    pub os: Option<OsRule>,
    #[serde(default)]
    pub features: HashMap<String, bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OsRule {
    pub name: Option<String>,
    pub arch: Option<String>,
    pub version: Option<String>,
}

pub(crate) struct Platform {
    pub os: &'static str,
    pub arch: &'static str,
    pub version: String,
}

impl Platform {
    pub fn current() -> Self {
        Self {
            os: match std::env::consts::OS {
                "macos" => "osx",
                other => other,
            },
            arch: match std::env::consts::ARCH {
                "x86" => "x86",
                "x86_64" => "x86_64",
                other => other,
            },
            version: String::new(),
        }
    }
}

pub(crate) fn allowed(
    rules: &[Rule],
    platform: &Platform,
    features: &HashMap<String, bool>,
) -> LauncherResult<bool> {
    let mut result = rules.is_empty();
    for rule in rules {
        let action = match rule.action.as_str() {
            "allow" => true,
            "disallow" => false,
            other => {
                return Err(LauncherError::InvalidMetadata(format!(
                    "未知规则动作 {other}"
                )))
            }
        };
        if rule
            .features
            .iter()
            .any(|(key, value)| features.get(key).copied().unwrap_or(false) != *value)
        {
            continue;
        }
        if let Some(os) = &rule.os {
            if os.name.as_ref().is_some_and(|n| n != platform.os) {
                continue;
            }
            let mut matches = true;
            for (pattern, value) in [
                (os.arch.as_deref(), platform.arch),
                (os.version.as_deref(), platform.version.as_str()),
            ] {
                if let Some(pattern) = pattern {
                    let regex = regex::Regex::new(pattern)
                        .map_err(|e| LauncherError::InvalidMetadata(e.to_string()))?;
                    matches &= regex.is_match(value);
                }
            }
            if !matches {
                continue;
            }
        }
        result = action;
    }
    Ok(result)
}
