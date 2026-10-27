//! Saved per-category MCP permissions, always bounded by the instance allowlist.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct McpPermissions {
    pub catalog: bool,
    pub instances: bool,
    pub logs: bool,
    pub write_mod_list: bool,
    pub write_mod_files: bool,
    pub write_configs: bool,
    pub write_quests: bool,
}
impl Default for McpPermissions {
    fn default() -> Self {
        Self {
            catalog: true,
            instances: false,
            logs: false,
            write_mod_list: false,
            write_mod_files: false,
            write_configs: false,
            write_quests: false,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct McpPolicy {
    pub version: u32,
    pub enabled: bool,
    pub game_root: PathBuf,
    pub permissions: McpPermissions,
    pub allowed_instances: Vec<String>,
}
impl Default for McpPolicy {
    fn default() -> Self {
        Self {
            version: 1,
            enabled: false,
            game_root: PathBuf::new(),
            permissions: Default::default(),
            allowed_instances: vec![],
        }
    }
}
impl McpPolicy {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("MCP 配置版本不受支持".into());
        }
        if !self.game_root.is_absolute() {
            return Err("MCP 实例集必须使用绝对路径".into());
        }
        if self.allowed_instances.len() > 512 {
            return Err("MCP 授权实例过多".into());
        }
        let layout = crate::GameLayout::new(&self.game_root).map_err(|e| e.to_string())?;
        for id in &self.allowed_instances {
            layout.version_dir(id).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn allows_tool(&self, name: &str) -> bool {
        self.enabled
            && match name {
                "minecraft_list_versions" | "minecraft_search_mods" | "minecraft_get_mod_info" => {
                    self.permissions.catalog
                }
                "minecraft_list_instances" => self.permissions.instances,
                "minecraft_list_instance_logs" | "minecraft_read_instance_log" | "minecraft_analyze_logs" => {
                    self.permissions.logs
                }
                "minecraft_get_write_formats" => true,
                "minecraft_write_mod_list" => self.permissions.write_mod_list,
                "minecraft_write_mod_file" => self.permissions.write_mod_files,
                "minecraft_write_config" => self.permissions.write_configs,
                "minecraft_upsert_quest_entry"
                | "minecraft_insert_quest_image"
                | "minecraft_read_quest_entries"
                | "minecraft_validate_item_ids" => self.permissions.write_quests,
                "minecraft_apply_instance_write" => {
                    self.permissions.write_mod_list
                        || self.permissions.write_mod_files
                        || self.permissions.write_configs
                        || self.permissions.write_quests
                }
                _ => false,
            }
    }
    pub fn allows_instance(&self, id: &str) -> bool {
        self.enabled && self.allowed_instances.iter().any(|allowed| allowed == id)
    }
    pub fn read(path: &Path) -> Result<Option<Self>, String> {
        use std::io::Read;
        let file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("无法读取 MCP 权限配置：{e}")),
        };
        let mut bytes = vec![];
        file.take(128 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 128 * 1024 {
            return Err("MCP 权限配置过大".into());
        }
        let policy: Self =
            serde_json::from_slice(&bytes).map_err(|e| format!("MCP 权限配置无效：{e}"))?;
        policy.validate()?;
        Ok(Some(policy))
    }
    pub async fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let parent = path.parent().ok_or("MCP 配置路径无效")?;
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| e.to_string())?;
        let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let result: Result<(), String> = async {
            tokio::fs::write(
                &temporary,
                serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
            )
            .await
            .map_err(|e| e.to_string())?;
            tokio::fs::rename(&temporary, path)
                .await
                .map_err(|e| e.to_string())?;
            Ok(())
        }
        .await;
        if result.is_err() {
            let _ = tokio::fs::remove_file(&temporary).await;
        }
        result
    }
}
pub fn policy_path() -> PathBuf {
    let directory = std::env::var_os("MEMORIA_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("MemoriaLauncher"))
        })
        .or_else(|| {
            std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share/memoria-launcher"))
        })
        .unwrap_or_else(|| PathBuf::from(".memoria-launcher"));
    directory.join("mcp.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn policy_round_trip_checks_scope_and_rejects_unknown_or_invalid_authority() {
        let root = std::env::temp_dir().join(format!("memoria-mcp-{}", uuid::Uuid::new_v4()));
        let path = root.join("mcp.json");
        let mut policy = McpPolicy {
            game_root: root.clone(),
            ..Default::default()
        };
        assert!(!policy.allows_tool("minecraft_list_versions"));
        policy.enabled = true;
        policy.permissions.logs = true;
        policy.allowed_instances = vec!["versions/one".into()];
        policy.save(&path).await.unwrap();
        assert_eq!(McpPolicy::read(&path).unwrap(), Some(policy.clone()));
        assert!(policy.allows_instance("versions/one"));
        assert!(!policy.allows_instance("versions/two"));
        assert!(!policy.allows_tool("run_shell"));
        policy
            .allowed_instances
            .push("versions/../../secret".into());
        assert!(policy.save(&path).await.is_err());
        assert_eq!(
            McpPolicy::read(&path)
                .unwrap()
                .unwrap()
                .allowed_instances
                .len(),
            1
        );
        std::fs::write(&path, b"invalid json").unwrap();
        assert!(McpPolicy::read(&path).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
