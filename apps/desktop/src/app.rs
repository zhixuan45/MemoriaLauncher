use crate::{
    home,
    models::{InstanceEntry, QueueEntry},
    shell, verification,
};
use iced::{event, mouse, window, Element, Event, Point, Subscription, Task, Vector};
use mc_core::{InMemoryTaskService, InstanceService, MockInstanceService, TaskService};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Home,
    Download,
    Ai,
    Settings,
}

impl Page {
    pub const ALL: [Self; 4] = [Self::Home, Self::Download, Self::Ai, Self::Settings];
    pub fn title(self) -> &'static str {
        match self {
            Self::Home => "主页",
            Self::Download => "游戏下载",
            Self::Ai => "AI 功能",
            Self::Settings => "设置",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Card {
    Launch,
    Stats,
    Instances,
    Featured,
    Changelog,
    Queue,
}
impl Card {
    pub const ALL: [Self; 6] = [
        Self::Launch,
        Self::Stats,
        Self::Instances,
        Self::Featured,
        Self::Changelog,
        Self::Queue,
    ];
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn protected(self) -> bool {
        matches!(self, Self::Launch | Self::Instances)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct CardLayout {
    pub offset: Vector,
    pub extra: Vector,
    pub visible: bool,
}
impl Default for CardLayout {
    fn default() -> Self {
        Self {
            offset: Vector::ZERO,
            extra: Vector::ZERO,
            visible: true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Drag {
    pub card: Card,
    pub start: Point,
    pub original: Vector,
    pub resizing: bool,
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Result<(Vec<InstanceEntry>, Vec<QueueEntry>), String>),
    Navigate(Page),
    SelectInstance(String),
    MoveInstance(String, i32),
    Launch,
    EditInstance,
    Dismiss,
    Refresh,
    ToggleTask(String),
    ToggleAllTasks,
    Feature(i32),
    FeatureDetails,
    Changelogs,
    ToggleLayout,
    ResetLayout,
    HideCard(Card),
    BeginDrag(Card, bool),
    Event(Event),
    DragWindow,
    Minimize,
    Maximize,
    Close,
    Profile,
    CycleProfile,
    Settings(crate::settings::Message),
    Download(crate::download::Message),
    Ai(crate::ai::Message),
    Capture,
    Captured(window::Screenshot),
    VerificationNext,
}

pub struct Launcher {
    pub page: Page,
    pub instances: Vec<InstanceEntry>,
    pub tasks: Vec<QueueEntry>,
    pub active: String,
    pub status: String,
    pub loading: bool,
    pub feature: usize,
    pub edit_layout: bool,
    pub layouts: [CardLayout; 6],
    pub cursor: Point,
    pub viewport: iced::Size,
    pub drag: Option<Drag>,
    pub modal: Option<Modal>,
    pub profile: usize,
    pub settings: crate::settings::State,
    pub download: crate::download::State,
    pub ai: crate::ai::State,
    pub verification: verification::Verification,
}

#[derive(Debug, Clone, Copy)]
pub enum Modal {
    Instance,
    Changelog,
    Feature,
    Profile,
}

impl Launcher {
    pub fn new() -> (Self, Task<Message>) {
        let state = Self {
            page: Page::Home,
            instances: vec![],
            tasks: vec![],
            active: String::new(),
            status: "正在读取实例与任务…".into(),
            loading: true,
            feature: 0,
            edit_layout: false,
            layouts: [CardLayout::default(); 6],
            cursor: Point::ORIGIN,
            viewport: verification::window_size(),
            drag: None,
            modal: None,
            profile: 0,
            settings: crate::settings::State::default(),
            download: crate::download::State::default(),
            ai: crate::ai::State::default(),
            verification: verification::Verification::from_args(),
        };
        (state, Self::load())
    }

    fn load() -> Task<Message> {
        Task::perform(
            async {
                let instances = MockInstanceService::new().with_sample_data().await;
                let tasks = InMemoryTaskService::new().with_sample_data().await;
                let mut instances = instances
                    .list_instances()
                    .await
                    .map_err(|e| e.to_string())?;
                let order = [
                    "inst-fabric-121",
                    "inst-vanilla-121",
                    "inst-forge-1192",
                    "inst-modpack-1201",
                ];
                instances.sort_by_key(|i| {
                    order
                        .iter()
                        .position(|id| *id == i.id)
                        .unwrap_or(usize::MAX)
                });
                let mut tasks = tasks.list_tasks().await.map_err(|e| e.to_string())?;
                tasks.sort_by(|a, b| a.id.cmp(&b.id));
                Ok((
                    instances.into_iter().map(Into::into).collect(),
                    tasks.into_iter().map(Into::into).collect(),
                ))
            },
            Message::Loaded,
        )
    }

    pub fn current(&self) -> Option<&InstanceEntry> {
        self.instances.iter().find(|i| i.instance.id == self.active)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Loaded(result) => {
                self.loading = false;
                match result {
                    Ok((instances, tasks)) => {
                        if !instances.iter().any(|i| i.instance.id == self.active) {
                            self.active = instances
                                .first()
                                .map(|i| i.instance.id.clone())
                                .unwrap_or_default();
                        }
                        self.instances = instances;
                        self.tasks = tasks;
                        self.status = "界面预览 · 实例与任务使用示例数据".into();
                    }
                    Err(e) => self.status = format!("读取失败：{e}"),
                }
                if self.verification.enabled() {
                    return verification::after_load(self);
                }
            }
            Message::Navigate(page) => {
                self.page = page;
                self.modal = None;
                self.drag = None;
                self.edit_layout = false;
                if page == Page::Download {
                    return self.download.ensure_loaded().map(Message::Download);
                }
            }
            Message::SelectInstance(id) => {
                if self.instances.iter().any(|i| i.instance.id == id) {
                    self.active = id;
                }
            }
            Message::MoveInstance(id, delta) => {
                if let Some(index) = self.instances.iter().position(|i| i.instance.id == id) {
                    let target =
                        (index as i32 + delta).clamp(0, self.instances.len() as i32 - 1) as usize;
                    self.instances.swap(index, target);
                }
            }
            Message::Launch => {
                self.status = match self.current() {
                    Some(i) if !i.installed => "此示例实例尚未安装，请先安装游戏。".into(),
                    Some(i) => format!(
                        "{}：当前是示例实例，尚未连接本地启动与账户认证。",
                        i.instance.name
                    ),
                    None => "暂无可启动的实例。".into(),
                };
            }
            Message::EditInstance => self.modal = Some(Modal::Instance),
            Message::Dismiss => self.modal = None,
            Message::Refresh => {
                if !self.loading {
                    self.loading = true;
                    return Self::load();
                }
            }
            Message::ToggleTask(id) => {
                if let Some(task) = self.tasks.iter_mut().find(|t| t.task.id == id) {
                    task.paused = !task.paused;
                }
                self.status = "已切换示例队列状态；真实下载服务尚未接入。".into();
            }
            Message::ToggleAllTasks => {
                let pause = self.tasks.iter().any(|t| !t.paused);
                self.tasks.iter_mut().for_each(|t| t.paused = pause);
                self.status = if pause {
                    "已暂停全部示例任务"
                } else {
                    "已恢复全部示例任务"
                }
                .into();
            }
            Message::Feature(delta) => {
                self.feature = (self.feature as i32 + delta).rem_euclid(4) as usize
            }
            Message::FeatureDetails => self.modal = Some(Modal::Feature),
            Message::Changelogs => self.modal = Some(Modal::Changelog),
            Message::ToggleLayout => {
                self.edit_layout = !self.edit_layout;
                self.drag = None;
            }
            Message::ResetLayout => {
                self.layouts = [CardLayout::default(); 6];
                self.drag = None;
            }
            Message::HideCard(card) if !card.protected() => {
                self.layouts[card.index()].visible = false
            }
            Message::HideCard(_) => {}
            Message::BeginDrag(card, resizing) => {
                if self.edit_layout {
                    let layout = self.layouts[card.index()];
                    self.drag = Some(Drag {
                        card,
                        resizing,
                        start: self.cursor,
                        original: if resizing {
                            layout.extra
                        } else {
                            layout.offset
                        },
                    });
                }
            }
            Message::Event(Event::Mouse(mouse::Event::CursorMoved { position })) => {
                self.cursor = position;
                if let Some(drag) = self.drag {
                    let delta = position - drag.start;
                    let snapped = Vector::new(
                        snap(drag.original.x + delta.x),
                        snap(drag.original.y + delta.y),
                    );
                    let layout = &mut self.layouts[drag.card.index()];
                    if drag.resizing {
                        layout.extra = snapped;
                    } else {
                        layout.offset = snapped;
                    }
                }
            }
            Message::Event(Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))) => {
                self.drag = None
            }
            Message::Event(Event::Window(window::Event::Unfocused)) => self.drag = None,
            Message::Event(Event::Window(window::Event::Resized(size))) => self.viewport = size,
            Message::Event(Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                ..
            })) => {
                self.modal = None;
                self.drag = None;
                self.edit_layout = false;
            }
            Message::Event(_) => {}
            Message::DragWindow => return window::latest().and_then(window::drag),
            Message::Minimize => return window::latest().and_then(|id| window::minimize(id, true)),
            Message::Maximize => return window::latest().and_then(window::toggle_maximize),
            Message::Close => return iced::exit(),
            Message::Profile => self.modal = Some(Modal::Profile),
            Message::CycleProfile => self.profile = (self.profile + 1) % 2,
            Message::Settings(message) => self.settings.update(message),
            Message::Download(message) => {
                return self.download.update(message).map(Message::Download)
            }
            Message::Ai(message) => self.ai.update(message),
            Message::Capture => {
                return window::latest()
                    .and_then(window::screenshot)
                    .map(Message::Captured)
            }
            Message::Captured(screenshot) => return verification::save(self, screenshot),
            Message::VerificationNext => return verification::next(self),
        }
        Task::none()
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let window = event::listen_with(window_event);
        if self.edit_layout && self.page == Page::Home && self.modal.is_none() {
            Subscription::batch([window, event::listen_with(layout_event)])
        } else {
            window
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        shell::view(self)
    }
    pub fn home(&self) -> Element<'_, Message> {
        home::view(self)
    }
}

// Ordinary pointer motion is handled by widgets, without rebuilding application state.
// Keep release events even when captured so a drag cannot remain stuck over a button.
fn layout_event(event: Event, _: event::Status, _: window::Id) -> Option<Message> {
    matches!(
        event,
        Event::Mouse(
            mouse::Event::CursorMoved { .. } | mouse::Event::ButtonReleased(mouse::Button::Left)
        )
    )
    .then_some(Message::Event(event))
}

fn window_event(event: Event, _: event::Status, _: window::Id) -> Option<Message> {
    matches!(
        event,
        Event::Window(window::Event::Resized(_) | window::Event::Unfocused)
            | Event::Keyboard(iced::keyboard::Event::KeyPressed {
                key: iced::keyboard::Key::Named(iced::keyboard::key::Named::Escape),
                ..
            })
    )
    .then_some(Message::Event(event))
}

pub fn snap(value: f32) -> f32 {
    (value / 16.).round() * 16.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subscriptions_ignore_normal_pointer_traffic_but_keep_drag_release() {
        let id = window::Id::unique();
        let motion = Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(80., 160.),
        });
        assert!(window_event(motion.clone(), event::Status::Ignored, id).is_none());
        assert!(layout_event(motion, event::Status::Captured, id).is_some());
        let release = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));
        assert!(layout_event(release, event::Status::Captured, id).is_some());
        let redraw = Event::Window(window::Event::RedrawRequested(std::time::Instant::now()));
        assert!(window_event(redraw, event::Status::Ignored, id).is_none());
    }

    #[test]
    fn grid_and_protected_cards() {
        assert_eq!(snap(23.), 16.);
        assert_eq!(snap(25.), 32.);
        assert_eq!(snap(-25.), -32.);
        let (mut app, _) = Launcher::new();
        let _ = app.update(Message::HideCard(Card::Launch));
        let _ = app.update(Message::HideCard(Card::Instances));
        assert!(app.layouts[Card::Launch.index()].visible);
        assert!(app.layouts[Card::Instances.index()].visible);
        let _ = app.update(Message::HideCard(Card::Stats));
        assert!(!app.layouts[Card::Stats.index()].visible);
        let _ = app.update(Message::ResetLayout);
        assert!(app
            .layouts
            .iter()
            .all(|l| l.visible && l.offset == Vector::ZERO));
    }
}
