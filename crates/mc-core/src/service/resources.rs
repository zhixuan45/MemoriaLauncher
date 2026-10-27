//! Typed, read-only Modrinth V2 catalogue shared by desktop and other clients.
use crate::{LauncherError, LauncherResult};
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResourceKind {
    Mod,
    Modpack,
    Resourcepack,
    Shader,
}
impl ResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mod => "mod",
            Self::Modpack => "modpack",
            Self::Resourcepack => "resourcepack",
            Self::Shader => "shader",
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceQuery {
    pub kind: ResourceKind,
    pub query: String,
    pub game_version: String,
    pub loader: String,
    pub offset: usize,
}
impl ResourceQuery {
    pub const PAGE_SIZE: usize = 20;
    fn parameters(&self) -> Vec<(&'static str, String)> {
        let mut facets = vec![vec![format!("project_type:{}", self.kind.as_str())]];
        if !self.game_version.trim().is_empty() {
            facets.push(vec![format!("versions:{}", self.game_version.trim())]);
        }
        if !self.loader.is_empty() {
            facets.push(vec![format!("categories:{}", self.loader)]);
        }
        vec![
            ("query", self.query.trim().to_owned()),
            (
                "facets",
                serde_json::to_string(&facets).expect("string facets"),
            ),
            ("index", "relevance".into()),
            ("offset", self.offset.to_string()),
            ("limit", Self::PAGE_SIZE.to_string()),
        ]
    }
}
#[derive(Debug, Clone, Deserialize)]
pub struct ResourceProject {
    pub project_id: String,
    pub slug: String,
    pub title: String,
    pub description: String,
    pub author: String,
    pub downloads: u64,
    pub categories: Vec<String>,
    pub versions: Vec<String>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct ResourcePage {
    pub hits: Vec<ResourceProject>,
    pub total_hits: usize,
    pub offset: usize,
    pub limit: usize,
}
#[derive(Debug, Clone, Deserialize)]
pub struct ResourceVersion {
    pub id: String,
    pub name: String,
    pub version_number: String,
    pub version_type: String,
    pub game_versions: Vec<String>,
    pub loaders: Vec<String>,
    pub files: Vec<ResourceFile>,
    pub dependencies: Vec<ResourceDependency>,
}
#[derive(Debug, Clone, Deserialize)]
pub struct ResourceFile {
    pub filename: String,
    pub size: u64,
}
#[derive(Debug, Clone, Deserialize)]
pub struct ResourceDependency {
    pub project_id: Option<String>,
    pub version_id: Option<String>,
    pub dependency_type: String,
}

#[derive(Clone)]
pub struct ResourceCatalog {
    client: reqwest::Client,
    base: String,
}
impl ResourceCatalog {
    pub(crate) async fn metadata(&self, path: &str) -> LauncherResult<serde_json::Value> {
        self.get(path, &[]).await
    }
    #[cfg(test)]
    pub(crate) fn with_base(base: String) -> Self {
        let mut value = Self::new().unwrap(); value.base = base; value
    }
    pub fn new() -> LauncherResult<Self> {
        super::online_catalog::init_tls_crypto();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("MemoriaLauncher/0.1 (https://github.com/zhixuan45/MemoriaLauncher)")
            .build()
            .map_err(|e| LauncherError::Internal(e.to_string()))?;
        Ok(Self {
            client,
            base: "https://api.modrinth.com/v2".into(),
        })
    }
    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        parameters: &[(&str, String)],
    ) -> LauncherResult<T> {
        let mut response = self
            .client
            .get(format!("{}{path}", self.base))
            .query(parameters)
            .send()
            .await
            .map_err(|e| LauncherError::Internal(format!("Modrinth 连接失败：{e}")))?;
        if !response.status().is_success() {
            return Err(LauncherError::Internal(format!(
                "Modrinth 返回 HTTP {}",
                response.status()
            )));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| LauncherError::Internal(e.to_string()))?
        {
            if bytes.len() + chunk.len() > 4 * 1024 * 1024 {
                return Err(LauncherError::InvalidMetadata("Modrinth 响应过大".into()));
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes)
            .map_err(|e| LauncherError::InvalidMetadata(format!("Modrinth 数据无效：{e}")))
    }
    pub async fn search(&self, query: &ResourceQuery) -> LauncherResult<ResourcePage> {
        let page: ResourcePage = self.get("/search", &query.parameters()).await?;
        if page.offset != query.offset
            || page.limit == 0
            || page.hits.len() > ResourceQuery::PAGE_SIZE
        {
            return Err(LauncherError::InvalidMetadata(
                "Modrinth 分页数据无效".into(),
            ));
        }
        Ok(page)
    }
    pub async fn versions(
        &self,
        project: &str,
        game_version: &str,
        loader: &str,
    ) -> LauncherResult<Vec<ResourceVersion>> {
        if project.is_empty()
            || project.len() > 100
            || !project
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        {
            return Err(LauncherError::InvalidMetadata(
                "Modrinth 项目 ID 无效".into(),
            ));
        }
        let mut parameters = vec![];
        if !game_version.trim().is_empty() {
            parameters.push((
                "game_versions",
                serde_json::to_string(&[game_version.trim()])?,
            ));
        }
        if !loader.is_empty() {
            parameters.push(("loaders", serde_json::to_string(&[loader])?));
        }
        self.get(&format!("/project/{project}/version"), &parameters)
            .await
    }
    pub(crate) async fn version_metadata(&self,project:&str,game:&str,loader:&str)->LauncherResult<Vec<serde_json::Value>> {
        if !super::mod_resolver::identifier(project)||project.contains(':') {return Err(LauncherError::InvalidMetadata("资源项目 ID 无效".into()));}
        let mut parameters=vec![("game_versions",serde_json::to_string(&[game])?)];
        if !loader.is_empty(){parameters.push(("loaders",serde_json::to_string(&[loader])?));}
        self.get(&format!("/project/{project}/version"),&parameters).await
    }
}

#[cfg(test)]
#[path = "resources_tests.rs"]
mod tests;
