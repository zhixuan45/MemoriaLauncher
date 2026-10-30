//! Sidebar organization is independent of game files and home/AI selection.
use crate::local::same_root;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    root: PathBuf,
    id: String,
    pub pinned: bool,
    pub hidden: bool,
    pub section: String,
}

#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Document {
    entries: Vec<Entry>,
    sections: Vec<String>,
}

#[derive(Default)]
pub struct State {
    document: Document,
    path: Option<PathBuf>,
    pub notice: Option<String>,
}

impl State {
    pub fn load() -> Self {
        if cfg!(test) {
            return Self::default();
        }
        Self::load_from(crate::accounts::data_dir().join("project-sidebar.json"))
    }
    pub fn load_from(path: PathBuf) -> Self {
        let result = (|| {
            let bytes = match fs::read(&path) {
                Ok(bytes) => bytes,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    return Ok(Document::default())
                }
                Err(error) => return Err(error.to_string()),
            };
            if bytes.len() > 256 * 1024 {
                return Err("侧栏配置过大".into());
            }
            let document: Document = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if document.entries.len() > 1024
                || document.sections.len() > 32
                || document.sections.iter().any(|s| !valid_section(s))
                || document.entries.iter().any(|e| {
                    e.id.len() > 240
                        || e.id.chars().any(char::is_control)
                        || e.root.as_os_str().is_empty()
                        || (!e.section.is_empty() && !valid_section(&e.section))
                })
            {
                return Err("侧栏配置无效".into());
            }
            Ok(document)
        })();
        match result {
            Ok(document) => Self {
                document,
                path: Some(path),
                notice: None,
            },
            Err(error) => Self {
                notice: Some(format!("侧栏配置读取失败：{error}；原文件保留。")),
                ..Default::default()
            },
        }
    }
    pub fn entry(&self, root: &Path, id: &str) -> Option<&Entry> {
        self.document
            .entries
            .iter()
            .find(|e| e.id == id && same_root(&e.root, root))
    }
    pub fn hidden(&self, root: &Path, id: &str) -> bool {
        self.entry(root, id).is_some_and(|e| e.hidden)
    }
    pub fn pinned(&self, root: &Path, id: &str) -> bool {
        self.entry(root, id).is_some_and(|e| e.pinned)
    }
    pub fn section(&self, root: &Path, id: &str) -> &str {
        self.entry(root, id)
            .map(|e| e.section.as_str())
            .unwrap_or("")
    }
    pub fn sections(&self) -> &[String] {
        &self.document.sections
    }
    pub fn modify(
        &mut self,
        root: &Path,
        id: &str,
        change: impl FnOnce(&mut Entry),
    ) -> Result<(), String> {
        if let Some(notice) = &self.notice {
            return Err(notice.clone());
        }
        let previous = self.document.clone();
        let index = self
            .document
            .entries
            .iter()
            .position(|e| e.id == id && same_root(&e.root, root));
        let index = match index {
            Some(index) => index,
            None => {
                if self.document.entries.len() >= 1024 {
                    return Err("侧栏项目记录达到上限".into());
                }
                self.document.entries.push(Entry {
                    root: root.into(),
                    id: id.into(),
                    ..Default::default()
                });
                self.document.entries.len() - 1
            }
        };
        change(&mut self.document.entries[index]);
        let section = self.document.entries[index].section.clone();
        if !section.is_empty() && !self.document.sections.contains(&section) {
            if !valid_section(&section) || self.document.sections.len() >= 32 {
                self.document = previous;
                return Err("分区名称需为 1–24 字，最多 32 个分区".into());
            }
            self.document.sections.push(section);
        }
        if let Err(error) = self.save() {
            self.document = previous;
            return Err(error);
        }
        Ok(())
    }
    fn save(&self) -> Result<(), String> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        let parent = path.parent().ok_or("侧栏配置目录无效")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let temporary = parent.join(format!("sidebar-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            use std::io::Write;
            let bytes = serde_json::to_vec_pretty(&self.document).map_err(|e| e.to_string())?;
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
                .map_err(|e| e.to_string())?;
            file.write_all(&bytes).map_err(|e| e.to_string())?;
            file.sync_all().map_err(|e| e.to_string())?;
            drop(file);
            fs::rename(&temporary, path).map_err(|e| e.to_string())
        })();
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}
fn valid_section(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().count() <= 24 && !value.chars().any(char::is_control)
}
