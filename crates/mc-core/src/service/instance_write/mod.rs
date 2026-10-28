//! Permission-scoped instance writes: prepare a persisted plan, then apply that exact plan.
pub mod files;
pub mod ftb;
pub mod items;
pub mod mods;
pub mod snbt;
mod transaction;

use super::mcp_policy::McpPolicy;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub use transaction::{apply, prepare, PlanPreview};
pub use transaction::{list_local_plans, review_local_plan, apply_local_plan, cancel_local_plan, LocalPlan, LocalReview};
pub type Result<T> = std::result::Result<T, WriteError>;
#[derive(Debug, Clone, Serialize)]
pub struct WriteError {
    pub code: &'static str,
    pub message: String,
}
impl std::fmt::Display for WriteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for WriteError {}
pub fn error(code: &'static str, message: impl ToString) -> WriteError {
    WriteError {
        code,
        message: message.to_string(),
    }
}
pub fn invalid(message: impl ToString) -> WriteError {
    error("INVALID_ARGUMENT", message)
}
pub fn io_error(message: impl ToString) -> WriteError {
    error("WRITE_IO_ERROR", message)
}
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WriteKind {
    ModList,
    ModFile,
    Config,
    Quest,
}
impl WriteKind {
    pub fn allowed(self, policy: &McpPolicy) -> bool {
        match self {
            Self::ModList => policy.permissions.write_mod_list,
            Self::ModFile => policy.permissions.write_mod_files,
            Self::Config => policy.permissions.write_configs,
            Self::Quest => policy.permissions.write_quests,
        }
    }
}
pub fn authorize(policy: &McpPolicy, instance: &str, kind: WriteKind) -> Result<()> {
    if !policy.enabled || !policy.allows_instance(instance) || !kind.allowed(policy) {
        return Err(error(
            "PERMISSION_DENIED",
            "此实例或写入类别未在启动器中授权",
        ));
    }
    policy
        .validate()
        .map_err(|e| error("MCP_POLICY_INVALID", e))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    ModList {
        mods: Vec<mods::ModReference>,
    },
    ModFile {
        filename: String,
        data_base64: String,
        sha256: String,
    },
    Config {
        path: String,
        content: String,
    },
    QuestEntry {
        chapter: String,
        #[serde(default)]
        chapter_title: Option<String>,
        entry: ftb::QuestEntry,
    },
    QuestImage {
        chapter: String,
        filename: String,
        data_base64: String,
        position: ftb::Position,
        #[serde(default = "ftb::image_size")]
        width: f64,
        #[serde(default = "ftb::image_size")]
        height: f64,
    },
}
impl Change {
    pub fn kind(&self) -> WriteKind {
        match self {
            Self::ModList { .. } => WriteKind::ModList,
            Self::ModFile { .. } => WriteKind::ModFile,
            Self::Config { .. } => WriteKind::Config,
            Self::QuestEntry { .. } | Self::QuestImage { .. } => WriteKind::Quest,
        }
    }
}
pub struct PreparedFile {
    pub path: String,
    pub bytes: Vec<u8>,
}
pub struct PreparedChange {
    pub files: Vec<PreparedFile>,
    pub watches: Vec<files::Watch>,
    pub warnings: Vec<String>,
    pub details: serde_json::Value,
}
impl PreparedChange {
    pub fn single(path: String, bytes: Vec<u8>) -> Self {
        Self {
            files: vec![PreparedFile { path, bytes }],
            watches: vec![],
            warnings: vec![],
            details: serde_json::json!({}),
        }
    }
}

pub fn validate_jar(bytes: &[u8]) -> Result<()> {
    if bytes.is_empty() || bytes.len() > files::MAX_MOD_BYTES {
        return Err(invalid("Mod 文件须为 1 字节至 32 MiB 的 JAR"));
    }
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|_| invalid("Mod 文件不是有效的 JAR/ZIP"))?;
    if archive.len() > 50_000 {
        return Err(invalid("JAR 条目过多"));
    }
    let mut metadata = false;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(invalid)?;
        if entry.enclosed_name().is_none() || entry.name().contains('\\') {
            return Err(invalid("JAR 包含不安全的内部路径"));
        }
        metadata |= matches!(
            entry.name(),
            "fabric.mod.json"
                | "quilt.mod.json"
                | "META-INF/mods.toml"
                | "META-INF/neoforge.mods.toml"
                | "mcmod.info"
        );
    }
    if !metadata {
        return Err(invalid("JAR 中没有受支持的 Mod 元信息"));
    }
    items::mod_namespaces(bytes)?;
    Ok(())
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
