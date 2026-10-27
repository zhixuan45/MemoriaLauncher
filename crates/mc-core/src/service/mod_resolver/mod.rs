//! Bounded, read-only dependency solving. A solution is a reviewable plan, never permission to write.
pub mod inventory;
pub mod metadata;
pub mod remote;
mod solve;
pub mod version;
use serde::{Deserialize, Serialize};
pub use solve::solve;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Environment {
    Any,
    Client,
    Server,
    Unknown,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Dialect {
    Fabric,
    Maven,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Constraint {
    Any,
    Exact(String),
    Range {
        dialect: Dialect,
        expression: String,
    },
    AnyOf(Vec<Constraint>),
    AllOf(Vec<Constraint>),
    Unknown(String),
}
impl Constraint {
    pub fn fabric(expression: impl Into<String>) -> Self {
        Self::Range {
            dialect: Dialect::Fabric,
            expression: expression.into(),
        }
    }
    pub fn maven(expression: impl Into<String>) -> Self {
        Self::Range {
            dialect: Dialect::Maven,
            expression: expression.into(),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    Required,
    Optional,
    Recommended,
    Conflict,
    Discouraged,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Requirement {
    pub id: String,
    pub constraint: Constraint,
    pub relation: Relation,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Candidate {
    pub id: String,
    pub version: String,
    pub source: String,
    #[serde(default)]
    pub origin: String,
    pub provides: Vec<String>,
    #[serde(default)]
    pub provided_versions: std::collections::BTreeMap<String, String>,
    pub dependencies: Vec<Requirement>,
    pub environment: Environment,
    pub installed: bool,
    pub metadata_complete: bool,
}
impl Candidate {
    pub fn version_for(&self, id: &str) -> &str {
        if id == self.id {
            &self.version
        } else {
            self.provided_versions
                .get(id)
                .map(String::as_str)
                .unwrap_or(&self.version)
        }
    }
    pub fn supplies(&self, id: &str) -> bool {
        self.id == id || self.provides.iter().any(|alias| alias == id)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Root {
    pub id: String,
    pub constraint: Constraint,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub code: String,
    pub subject: String,
    pub message: String,
    pub chain: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Resolved,
    Conflict,
    Unknown,
    LimitReached,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub relation: Relation,
    pub constraint: Constraint,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    pub action: String,
    pub id: String,
    pub version: String,
    pub source: String,
    pub reason: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub status: Status,
    pub selected: Vec<Candidate>,
    pub issues: Vec<Issue>,
    pub warnings: Vec<Issue>,
    pub edges: Vec<Edge>,
    pub cycles: Vec<Vec<String>>,
    pub changes: Vec<Change>,
    pub steps: usize,
}
pub(crate) fn issue(
    code: &str,
    subject: &str,
    message: impl Into<String>,
    chain: Vec<String>,
) -> Issue {
    Issue {
        code: code.into(),
        subject: subject.into(),
        message: message.into(),
        chain,
    }
}
pub(crate) fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-:+".contains(&b))
}

#[cfg(test)]
mod tests;
