//! Read-only public catalog adapter for MCP and future GUI/API use.
use crate::{LauncherError, LauncherResult};
use serde_json::{json, Value};
use std::time::Duration;

/// Select ring once for the shared reqwest rustls-no-provider transport.
pub fn init_tls_crypto() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[derive(Clone)]
pub struct OnlineCatalog {
    client: reqwest::Client,
}

impl OnlineCatalog {
    pub fn new() -> LauncherResult<Self> {
        init_tls_crypto();
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .user_agent("MemoriaLauncher/0.1 (read-only catalog)")
            .build()
            .map_err(|e| LauncherError::Internal(format!("HTTP client: {e}")))?;
        Ok(Self { client })
    }

    async fn get_json(&self, url: &str, query: &[(&str, &str)]) -> LauncherResult<Value> {
        let response = self.client
            .get(url)
            .query(query)
            .send()
            .await
            .map_err(|e| LauncherError::Internal(format!("Catalog connection failed: {e}")))?
            .error_for_status()
            .map_err(|e| LauncherError::Internal(format!("Catalog returned an error: {e}")))?;
        response.json::<Value>().await
            .map_err(|e| LauncherError::Internal(format!("Invalid catalog JSON: {e}")))
    }

    /// Mojang's real version manifest, newest first. No sample records.
    pub async fn list_versions(&self, limit: usize) -> LauncherResult<Value> {
        let data = self.get_json(
            "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json", &[],
        ).await?;
        let versions = data["versions"].as_array()
            .ok_or_else(|| LauncherError::InvalidMetadata("Missing Mojang versions".into()))?;
        let items: Vec<Value> = versions.iter().take(limit.clamp(1, 100))
            .map(|v| json!({
                "id": v["id"], "type": v["type"],
                "release_time": v["releaseTime"], "metadata_url": v["url"]
            })).collect();
        Ok(json!({"source": "mojang", "versions": items}))
    }

    /// Query Modrinth V2 using encoded query parameters and explicit filters.
    pub async fn search_mods(
        &self, query: &str, version: Option<&str>, loader: Option<&str>, limit: usize,
    ) -> LauncherResult<Value> {
        let mut facets = vec![vec!["project_type:mod".to_string()]];
        if let Some(version) = version {
            facets.push(vec![format!("versions:{version}")]);
        }
        if let Some(loader) = loader {
            facets.push(vec![format!("categories:{loader}")]);
        }
        let facets = serde_json::to_string(&facets)?;
        let limit = limit.clamp(1, 20).to_string();
        let data = self.get_json(
            "https://api.modrinth.com/v2/search",
            &[("query", query), ("facets", facets.as_str()), ("limit", limit.as_str())],
        ).await?;
        let hits = data["hits"].as_array()
            .ok_or_else(|| LauncherError::InvalidMetadata("Missing Modrinth results".into()))?;
        let items: Vec<Value> = hits.iter().map(|p| json!({
            "project_id": p["project_id"], "slug": p["slug"],
            "title": p["title"], "description": p["description"],
            "categories": p["categories"], "versions": p["versions"],
            "client_side": p["client_side"], "server_side": p["server_side"]
        })).collect();
        Ok(json!({"source": "modrinth", "total_hits": data["total_hits"], "projects": items}))
    }

    /// Public metadata only. Versions/dependency resolution are a separate milestone.
    pub async fn get_mod_info(&self, id: &str) -> LauncherResult<Value> {
        if !valid_project_id(id) {
            return Err(LauncherError::InvalidMetadata("Invalid Modrinth project id/slug".into()));
        }
        let url = format!("https://api.modrinth.com/v2/project/{id}");
        let p = self.get_json(&url, &[]).await?;
        Ok(json!({
            "source": "modrinth", "project_id": p["id"], "slug": p["slug"],
            "title": p["title"], "description": p["description"],
            "project_type": p["project_type"], "license": p["license"],
            "categories": p["categories"], "game_versions": p["game_versions"],
            "client_side": p["client_side"], "server_side": p["server_side"],
            "versions": p["versions"], "status": p["status"],
            "url": format!("https://modrinth.com/mod/{}", p["slug"].as_str().unwrap_or(id))
        }))
    }
}

fn valid_project_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 100 &&
        id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_id_must_not_allow_url_path_injection() {
        for invalid in ["", "..", "foo/bar", "https://host", "id?q=all", "%2F"] {
            assert!(!valid_project_id(invalid), "{invalid}");
        }
        assert!(valid_project_id("sodium"));
        assert!(valid_project_id("AANobbMI"));
    }
}
