//! Small, ordered preference writes keep home, AI and last-launch identities independent.
use crate::{app::Launcher, local::same_root};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const LIMIT: usize = 64 * 1024;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub root: PathBuf,
    pub instance_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Collection {
    root: PathBuf,
    home: Option<String>,
    ai: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u32,
    collections: Vec<Collection>,
    last_launched: Option<Reference>,
}
impl Default for Document {
    fn default() -> Self {
        Self {
            version: 1,
            collections: vec![],
            last_launched: None,
        }
    }
}
pub struct State {
    document: Document,
    path: Option<PathBuf>,
    pub restore_pending: bool,
    pub notice: Option<String>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            document: Document::default(),
            path: None,
            restore_pending: true,
            notice: None,
        }
    }
}
impl State {
    pub fn load() -> Self {
        // Test fixtures must never overwrite the real user's remembered selections.
        if cfg!(test) {
            Self::default()
        } else {
            Self::load_from(crate::accounts::data_dir().join("selections.json"))
        }
    }
    pub fn load_from(path: PathBuf) -> Self {
        let result = (|| -> Result<Document, String> {
            let meta = match fs::symlink_metadata(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(Document::default())
                }
                Err(e) => return Err(e.to_string()),
                Ok(meta) => meta,
            };
            if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > LIMIT as u64 {
                return Err("选择记录不是有效的普通配置文件".into());
            }
            let mut bytes = vec![];
            fs::File::open(&path)
                .map_err(|e| e.to_string())?
                .take(LIMIT as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            if bytes.len() > LIMIT {
                return Err("选择记录过大".into());
            }
            let document: Document = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if document.version != 1 || document.collections.len() > 128 {
                return Err("选择记录版本或数量无效".into());
            }
            for collection in &document.collections {
                validate(&collection.root, collection.home.as_deref())?;
                validate(&collection.root, collection.ai.as_deref())?;
            }
            if let Some(last) = &document.last_launched {
                validate(&last.root, Some(&last.instance_id))?;
            }
            Ok(document)
        })();
        match result {
            Ok(document) => Self {
                document,
                path: Some(path),
                ..Default::default()
            },
            Err(error) => Self {
                notice: Some(format!(
                    "无法恢复项目选择：{error}；原记录保留，本次选择暂不写入。"
                )),
                ..Default::default()
            },
        }
    }
    fn collection(&self, root: &Path) -> Option<&Collection> {
        self.document
            .collections
            .iter()
            .find(|c| same_root(&c.root, root))
    }
    pub fn home(&self, root: &Path) -> Option<&str> {
        self.collection(root)?.home.as_deref()
    }
    pub fn ai(&self, root: &Path) -> Option<&str> {
        self.collection(root)?.ai.as_deref()
    }
    pub fn last_launched(&self) -> Option<&Reference> {
        self.document.last_launched.as_ref()
    }
    pub fn remember(&mut self, root: &Path, home: &str, ai: Option<&str>) -> Result<(), String> {
        let normalized = crate::local::normalize_root(root)?;
        let root = normalized.as_path();
        validate(root, Some(home))?;
        validate(root, ai)?;
        let index = match self
            .document
            .collections
            .iter()
            .position(|c| same_root(&c.root, root))
        {
            Some(index) => index,
            None => {
                if self.document.collections.len() >= 128 {
                    return Err("实例集选择记录达到上限".into());
                }
                self.document.collections.push(Collection {
                    root: root.into(),
                    home: None,
                    ai: None,
                });
                self.document.collections.len() - 1
            }
        };
        let collection = &mut self.document.collections[index];
        collection.home = (!home.is_empty()).then(|| home.to_owned());
        collection.ai = ai.map(str::to_owned);
        self.save()
    }
    pub fn launched(&mut self, root: &Path, id: &str) -> Result<(), String> {
        let normalized = crate::local::normalize_root(root)?;
        let root = normalized.as_path();
        validate(root, Some(id))?;
        self.document.last_launched = Some(Reference {
            root: root.into(),
            instance_id: id.into(),
        });
        self.save()
    }
    fn save(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let bytes = serde_json::to_vec_pretty(&self.document).map_err(|e| e.to_string())?;
        if bytes.len() > LIMIT {
            return Err("选择记录超过 64 KiB".into());
        }
        let parent = path.parent().ok_or("选择记录路径无效")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temporary = parent.join(format!("selections-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| -> std::io::Result<()> {
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temporary, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result.map_err(|e| e.to_string())
    }
}
fn validate(root: &Path, id: Option<&str>) -> Result<(), String> {
    if root.as_os_str().is_empty()
        || root.as_os_str().len() > 4096
        || id.is_some_and(|id| id.len() > 240 || id.chars().any(char::is_control))
    {
        Err("实例选择标识无效".into())
    } else {
        Ok(())
    }
}

pub fn remember(app: &mut Launcher) {
    // Closing while the initial/changed collection is still scanning must not erase saved choices.
    if app.selections.restore_pending {
        return;
    }
    let ai = app
        .workspace
        .current
        .project
        .as_ref()
        .filter(|p| same_root(&p.root, &app.environment.game_root))
        .map(|p| p.instance_id.as_str());
    if let Err(error) = app
        .selections
        .remember(&app.environment.game_root, &app.active, ai)
    {
        app.status = format!("项目选择保存失败：{error}");
    }
}
pub fn restore(app: &mut Launcher) {
    if !app.instances.iter().any(|i| i.instance.id == app.active) {
        app.active = app
            .selections
            .home(&app.environment.game_root)
            .filter(|id| app.instances.iter().any(|i| i.instance.id == *id))
            .map(str::to_owned)
            .or_else(|| app.instances.first().map(|i| i.instance.id.clone()))
            .unwrap_or_default();
    }
    if app.selections.restore_pending {
        app.selections.restore_pending = false;
        let project = app
            .selections
            .ai(&app.environment.game_root)
            .and_then(|id| crate::workspace::project(app, id));
        app.workspace.new_chat(&mut app.ai, project);
    }
    if let Some(notice) = &app.selections.notice {
        app.status = format!("{}；{notice}", app.status);
    }
}

#[cfg(test)]
#[path = "selections_tests.rs"]
mod tests;
