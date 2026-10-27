//! Instance projects and isolated conversations. In-flight replies carry their conversation ID.
use crate::{
    ai,
    app::{Launcher, Message as AppMessage, Page},
};
use iced::Task;
use std::{collections::HashSet, path::PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Project {
    pub root: PathBuf,
    pub instance_id: String,
    pub name: String,
    pub version: String,
    pub loader: String,
}
impl Project {
    pub fn matches(&self, root: &std::path::Path, id: &str) -> bool {
        crate::local::same_root(&self.root, root) && self.instance_id == id
    }
}
#[derive(Clone)]
pub struct ConversationInfo {
    pub id: Uuid,
    pub title: String,
    pub project: Option<Project>,
}
impl ConversationInfo {
    fn new(project: Option<Project>) -> Self {
        Self {
            id: Uuid::new_v4(),
            title: "新对话".into(),
            project,
        }
    }
}
pub struct Conversation {
    pub info: ConversationInfo,
    pub state: ai::State,
}
pub struct State {
    pub current: ConversationInfo,
    pub others: Vec<Conversation>,
    pub search_open: bool,
    pub query: String,
    collapsed: HashSet<(PathBuf, String)>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            current: ConversationInfo::new(None),
            others: vec![],
            search_open: false,
            query: String::new(),
            collapsed: HashSet::new(),
        }
    }
}
#[derive(Debug, Clone)]
pub enum Message {
    NewChat,
    OpenProject(String),
    OpenChat(Uuid),
    ToggleProject(String),
    Search,
    Query(String),
    ManageProjects,
}
impl State {
    pub fn expanded(&self, root: &std::path::Path, id: &str) -> bool {
        !self
            .collapsed
            .contains(&(root.to_path_buf(), id.to_owned()))
    }
    pub fn conversations(&self) -> impl Iterator<Item = &ConversationInfo> {
        std::iter::once(&self.current).chain(self.others.iter().rev().map(|c| &c.info))
    }
    pub fn visible<'a>(&'a self, info: &'a ConversationInfo) -> bool {
        let query = self.query.trim().to_lowercase();
        query.is_empty()
            || info.title.to_lowercase().contains(&query)
            || info
                .project
                .as_ref()
                .is_some_and(|p| p.name.to_lowercase().contains(&query))
    }
    fn stash(&mut self, state: &mut ai::State) {
        if state.has_content() {
            let connection = state.connection.clone();
            let saved = std::mem::take(state);
            self.others.push(Conversation {
                info: self.current.clone(),
                state: saved,
            });
            state.connection = connection;
        } else {
            let connection = state.connection.clone();
            *state = ai::State::default();
            state.connection = connection;
        }
    }
    pub fn new_chat(&mut self, state: &mut ai::State, project: Option<Project>) {
        self.stash(state);
        self.current = ConversationInfo::new(project);
        self.set_context(state);
    }
    pub fn open(&mut self, state: &mut ai::State, id: Uuid) {
        if self.current.id == id {
            return;
        }
        if let Some(index) = self.others.iter().position(|c| c.info.id == id) {
            let selected = self.others.remove(index);
            let connection = state.connection.clone();
            self.stash(state);
            self.current = selected.info;
            *state = selected.state;
            state.connection = connection;
        }
    }
    fn set_context(&self, state: &mut ai::State) {
        state.attachments.target =
            self.current
                .project
                .as_ref()
                .map(|p| crate::attachments::LogTarget {
                    root: p.root.clone(),
                    instance_id: p.instance_id.clone(),
                });
        state.project_context = self.current.project.as_ref().map(|p| {
            format!(
                "项目：{}\nMinecraft 版本：{}\n加载器：{}",
                p.name, p.version, p.loader
            )
        });
    }
}

pub fn project(app: &Launcher, id: &str) -> Option<Project> {
    let instance = app.instances.iter().find(|i| i.instance.id == id)?;
    Some(Project {
        root: app.environment.game_root.clone(),
        instance_id: id.into(),
        name: instance.instance.name.clone(),
        version: instance.instance.mc_version.clone(),
        loader: instance.loader.clone(),
    })
}

pub fn update(app: &mut Launcher, message: Message) -> Task<AppMessage> {
    match message {
        Message::NewChat => {
            let context = project(app, &app.active);
            app.workspace.new_chat(&mut app.ai, context);
        }
        Message::OpenProject(id) => {
            let Some(context) = project(app, &id) else {
                return Task::none();
            };
            app.active = id;
            let existing = app
                .workspace
                .conversations()
                .find(|c| {
                    c.project
                        .as_ref()
                        .is_some_and(|p| p.matches(&context.root, &context.instance_id))
                })
                .map(|c| c.id);
            if let Some(id) = existing {
                app.workspace.open(&mut app.ai, id);
            } else {
                app.workspace.new_chat(&mut app.ai, Some(context));
            }
        }
        Message::OpenChat(id) => {
            app.workspace.open(&mut app.ai, id);
            if let Some(context) = &app.workspace.current.project {
                if project(app, &context.instance_id).is_some()
                    && crate::local::same_root(&context.root, &app.environment.game_root)
                {
                    app.active = context.instance_id.clone();
                }
            }
        }
        Message::ToggleProject(id) => {
            let key = (app.environment.game_root.clone(), id);
            if !app.workspace.collapsed.remove(&key) {
                app.workspace.collapsed.insert(key);
            }
            return Task::none();
        }
        Message::Search => {
            app.workspace.search_open = !app.workspace.search_open;
            if !app.workspace.search_open {
                app.workspace.query.clear();
            }
            return if app.workspace.search_open {
                iced::widget::operation::focus("conversation-search")
            } else {
                Task::none()
            };
        }
        Message::Query(query) => {
            app.workspace.query = query;
            return Task::none();
        }
        Message::ManageProjects => {
            let _ = app
                .settings
                .update(crate::settings::Message::Tab(1), app.animation_time);
            return app.update(AppMessage::Navigate(Page::Settings));
        }
    }
    Task::batch([
        app.update(AppMessage::Navigate(Page::Ai)),
        iced::widget::operation::focus("ai-prompt"),
        iced::widget::operation::snap_to(
            "ai-transcript",
            iced::widget::scrollable::RelativeOffset::END,
        ),
    ])
}

pub fn update_ai(app: &mut Launcher, id: Uuid, message: ai::Message) -> Task<AppMessage> {
    let current = id == app.workspace.current.id;
    let (info, state) = if current {
        (&mut app.workspace.current, &mut app.ai)
    } else if let Some(conversation) = app.workspace.others.iter_mut().find(|c| c.info.id == id) {
        (&mut conversation.info, &mut conversation.state)
    } else {
        return Task::none();
    };
    if matches!(message, ai::Message::Submit)
        && info.title == "新对话"
        && !state.prompt.trim().is_empty()
    {
        info.title = state.prompt.trim().chars().take(28).collect();
    }
    let clear = matches!(message, ai::Message::Clear)
        && !state.busy
        && !state.loading_log
        && !state.attachments.loading();
    let task = state.update(message, app.animation_time);
    if clear {
        info.title = "新对话".into();
    }
    // An inactive reply must not scroll the currently open conversation.
    if current {
        task.map(move |message| AppMessage::ChatReply(id, message))
    } else {
        Task::none()
    }
}

#[cfg(test)]
#[path = "workspace_tests.rs"]
mod tests;
