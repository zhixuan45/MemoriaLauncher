use crate::{
    app::{Launcher, Message as AppMessage, Modal},
    workspace::Project,
};
use iced::{Point, Rectangle, Task, Vector};

#[derive(Debug, Clone)]
pub enum Message {
    Ignore,
    Open(String),
    Positioned(String, Option<Rectangle>),
    Close,
    Pin,
    Edit,
    Sections,
    Section(String),
    SectionName(String),
    CreateSection,
    OpenDirectory,
    DirectoryOpened(Result<(), String>),
    Archive,
    Hide,
    ToggleHidden,
    Restore(String),
    ToggleArchived,
    RestoreChat(uuid::Uuid),
}
pub struct Menu {
    pub project: Project,
    pub position: Point,
    pub sections: bool,
    pub section_name: String,
}
#[derive(Default)]
pub struct State {
    pub preferences: crate::project_preferences::State,
    pub menu: Option<Menu>,
    pub hidden_open: bool,
    pub archived_open: bool,
}
impl State {
    pub fn load() -> Self {
        Self {
            preferences: crate::project_preferences::State::load(),
            ..Default::default()
        }
    }
}
fn dispatch(message: Message) -> AppMessage {
    AppMessage::ProjectMenu(message)
}

pub fn update(app: &mut Launcher, message: Message) -> Task<AppMessage> {
    match message {
        Message::Ignore => {}
        Message::Close => app.project_menu.menu = None,
        Message::ToggleHidden => app.project_menu.hidden_open = !app.project_menu.hidden_open,
        Message::ToggleArchived => app.project_menu.archived_open = !app.project_menu.archived_open,
        Message::Restore(id) => {
            let result =
                app.project_menu
                    .preferences
                    .modify(&app.environment.game_root, &id, |e| e.hidden = false);
            app.status = result
                .map(|_| "项目已恢复到侧栏。".into())
                .unwrap_or_else(|e| e);
        }
        Message::RestoreChat(id) => {
            app.workspace.restore_chat(id);
            return app.update(AppMessage::Workspace(crate::workspace::Message::OpenChat(
                id,
            )));
        }
        Message::Open(id) => {
            if app.modal.is_some() {
                return Task::none();
            }
            if app
                .project_menu
                .menu
                .as_ref()
                .is_some_and(|menu| menu.project.instance_id == id)
            {
                app.project_menu.menu = None;
                return Task::none();
            }
            let Some(project) = crate::workspace::project(app, &id) else {
                return Task::none();
            };
            app.project_menu.menu = Some(Menu {
                project,
                position: Point::new(140., 200.),
                sections: false,
                section_name: String::new(),
            });
            return iced::advanced::widget::operate(FindAnchor::new(format!("project-more-{id}")))
                .map(move |bounds| dispatch(Message::Positioned(id.clone(), bounds)));
        }
        Message::Positioned(id, bounds) => {
            if let Some(menu) = &mut app.project_menu.menu {
                if menu.project.instance_id == id {
                    if let Some(bounds) = bounds {
                        menu.position = Point::new(bounds.x, bounds.y + bounds.height + 4.);
                    }
                }
            }
        }
        Message::DirectoryOpened(result) => {
            app.status = result
                .map(|_| "已在文件管理器中打开实例目录。".into())
                .unwrap_or_else(|e| format!("打开失败：{e}"));
        }
        action => {
            let Some(menu) = app.project_menu.menu.as_mut() else {
                return Task::none();
            };
            match action {
                Message::Sections => {
                    menu.sections = !menu.sections;
                    return Task::none();
                }
                Message::SectionName(name) => {
                    menu.section_name = name;
                    return Task::none();
                }
                _ => {}
            }
            let project = menu.project.clone();
            if !crate::local::same_root(&project.root, &app.environment.game_root)
                || !app
                    .instances
                    .iter()
                    .any(|i| i.instance.id == project.instance_id)
            {
                app.project_menu.menu = None;
                return Task::none();
            }
            let section_name = menu.section_name.trim().to_string();
            let hiding = matches!(action, Message::Hide);
            let result =
                match action {
                    Message::Pin => app.project_menu.preferences.modify(
                        &project.root,
                        &project.instance_id,
                        |e| e.pinned = !e.pinned,
                    ),
                    Message::Section(section) => app.project_menu.preferences.modify(
                        &project.root,
                        &project.instance_id,
                        |e| e.section = section,
                    ),
                    Message::CreateSection => {
                        if section_name.is_empty() {
                            return Task::none();
                        }
                        app.project_menu.preferences.modify(
                            &project.root,
                            &project.instance_id,
                            |e| e.section = section_name,
                        )
                    }
                    Message::Hide => app.project_menu.preferences.modify(
                        &project.root,
                        &project.instance_id,
                        |e| e.hidden = true,
                    ),
                    Message::Edit => {
                        if app.instance_writing()
                            || app.launching
                            || app.loading
                            || app.environment_saving
                        {
                            return Task::none();
                        }
                        app.project_menu.menu = None;
                        app.open_modal(Modal::Instance);
                        return app
                            .instance_settings
                            .open(project.root, project.instance_id)
                            .map(AppMessage::InstanceSettings);
                    }
                    Message::OpenDirectory => {
                        app.project_menu.menu = None;
                        return Task::perform(
                            async move {
                                tokio::task::spawn_blocking(move || {
                                    let path = mc_core::service::desktop_instances::directory(
                                        &project.root,
                                        &project.instance_id,
                                    )?;
                                    crate::general::open_directory(&path)
                                })
                                .await
                                .map_err(|e| e.to_string())?
                            },
                            |result| dispatch(Message::DirectoryOpened(result)),
                        );
                    }
                    Message::Archive => {
                        match app.workspace.archive_project(&mut app.ai, &project) {
                            Ok(count) => {
                                app.status =
                                    format!("已归档 {count} 个对话，可在侧栏“已归档对话”恢复。");
                                app.project_menu.menu = None;
                            }
                            Err(error) => app.status = error,
                        }
                        return Task::none();
                    }
                    _ => return Task::none(),
                };
            match result {
                Ok(()) => {
                    app.status = if hiding {
                        "已从侧栏移除项目；游戏文件和对话保留，可在“已移除项目”恢复。".into()
                    } else {
                        "侧栏项目设置已保存。".into()
                    };
                    app.project_menu.menu = None;
                }
                Err(error) => app.status = error,
            }
        }
    }
    Task::none()
}

// Query the actual button bounds, including nested scroll offsets, instead of
// guessing a row number or depending on pointer motion (keyboard activation works too).
struct FindAnchor {
    id: iced::advanced::widget::Id,
    bounds: Option<Rectangle>,
    translation: Vector,
    pending: Vector,
}
impl FindAnchor {
    fn new(id: String) -> Self {
        Self {
            id: id.into(),
            bounds: None,
            translation: Vector::ZERO,
            pending: Vector::ZERO,
        }
    }
}
impl iced::advanced::widget::Operation<Option<Rectangle>> for FindAnchor {
    fn traverse(
        &mut self,
        operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation<Option<Rectangle>>),
    ) {
        let previous = self.translation;
        self.translation += std::mem::replace(&mut self.pending, Vector::ZERO);
        operate(self);
        self.translation = previous;
    }
    fn container(&mut self, id: Option<&iced::advanced::widget::Id>, bounds: Rectangle) {
        if id == Some(&self.id) {
            self.bounds = Some(bounds - self.translation);
        }
    }
    fn scrollable(
        &mut self,
        _: Option<&iced::advanced::widget::Id>,
        _: Rectangle,
        _: Rectangle,
        translation: Vector,
        _: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        self.pending = translation;
    }
    fn finish(&self) -> iced::advanced::widget::operation::Outcome<Option<Rectangle>> {
        iced::advanced::widget::operation::Outcome::Some(self.bounds)
    }
}

#[path = "project_menu_ui.rs"]
mod ui;
pub use ui::overlay;
