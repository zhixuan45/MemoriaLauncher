use crate::motion::{self, Tween};
use crate::motion_renderer::Element;
use crate::{
    models::{InstanceEntry, QueueEntry},
    shell, verification,
};
use iced::{event, mouse, window, Event, Point, Subscription, Task, Vector};
use std::time::Instant;

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
    Animate(Instant),
    Loaded(Result<crate::local::LocalData, String>),
    Accounts(crate::accounts::Message),
    GameRoot(String),
    JavaPath(String),
    SaveEnvironment,
    EnvironmentSaved(Result<crate::local::Environment, String>),
    GameStarted(String),
    GameFinished(Result<String, String>),
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
    pub outgoing_page: Option<Page>,
    pub page_motion: Tween,
    pub card_motion: Tween,
    pub modal_motion: Tween,
    pub animation_time: Instant,
    pub accounts: crate::accounts::State,
    pub environment: crate::local::Environment,
    pub game_root_input: String,
    pub java_input: String,
    pub environment_saving: bool,
    pub launching: bool,
    pub settings: crate::settings::State,
    pub download: crate::download::State,
    pub ai: crate::ai::State,
    pub verification: verification::Verification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modal {
    Instance,
    Changelog,
    Feature,
    Profile,
}

impl Launcher {
    pub fn new() -> (Self, Task<Message>) {
        let (environment, environment_error) = crate::local::Environment::load();
        let now = Instant::now();
        let mut page_motion = Tween::new(0.0, now);
        page_motion.to(1.0, motion::PAGE_DURATION, now);
        let mut card_motion = Tween::new(0.0, now);
        card_motion.to(1.0, motion::CARD_DURATION, now);
        let state = Self {
            page: Page::Home,
            instances: vec![],
            tasks: vec![],
            active: String::new(),
            status: environment_error.unwrap_or_else(|| "正在读取本地实例…".into()),
            loading: true,
            feature: 0,
            edit_layout: false,
            layouts: [CardLayout::default(); 6],
            cursor: Point::ORIGIN,
            viewport: verification::window_size(),
            drag: None,
            modal: None,
            outgoing_page: None,
            page_motion,
            card_motion,
            modal_motion: Tween::new(0.0, now),
            animation_time: now,
            accounts: crate::accounts::State::default(),
            game_root_input: environment.game_root.to_string_lossy().into_owned(),
            java_input: environment.java.to_string_lossy().into_owned(),
            environment,
            environment_saving: false,
            launching: false,
            settings: crate::settings::State::default(),
            download: crate::download::State::default(),
            ai: crate::ai::State::default(),
            verification: verification::Verification::from_args(),
        };
        let task = Task::batch([state.load(), state.accounts.load().map(Message::Accounts)]);
        (state, task)
    }

    fn load(&self) -> Task<Message> {
        Task::perform(
            crate::local::scan(self.environment.game_root.clone()),
            Message::Loaded,
        )
    }

    pub fn current(&self) -> Option<&InstanceEntry> {
        self.instances.iter().find(|i| i.instance.id == self.active)
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Animate(now) => {
                let now = now.max(self.animation_time);
                self.animation_time = now;
                if !self.page_motion.active(now) {
                    self.outgoing_page = None;
                }
                if self.modal_motion.target() == 0.0 && !self.modal_motion.active(now) {
                    self.modal = None;
                }
            }
            Message::Loaded(result) => {
                self.loading = false;
                match result {
                    Ok(data) => {
                        let instances = data.instances;
                        if !instances.iter().any(|i| i.instance.id == self.active) {
                            self.active = instances
                                .first()
                                .map(|i| i.instance.id.clone())
                                .unwrap_or_default();
                        }
                        self.instances = instances;
                        self.status = data.notice;
                    }
                    Err(e) => self.status = format!("读取失败：{e}"),
                }
                if self.verification.enabled() {
                    return verification::after_load(self);
                }
            }
            Message::Navigate(page) => {
                if self.page != page {
                    let now = Instant::now().max(self.animation_time);
                    self.animation_time = now;
                    self.outgoing_page = Some(self.page);
                    self.page_motion = Tween::new(0.0, now);
                    self.page_motion.to(1.0, motion::PAGE_DURATION, now);
                    self.card_motion = Tween::new(0.0, now);
                    self.card_motion.to(1.0, motion::CARD_DURATION, now);
                }
                self.page = page;
                self.dismiss_modal();
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
                if self.launching || self.loading || self.environment_saving {
                    return Task::none();
                }
                let key = match self.current() {
                    Some(i) if !i.installed => {
                        self.status = "客户端 JAR 缺失，请先安装完整游戏。".into();
                        return Task::none();
                    }
                    Some(i) => i.instance.id.clone(),
                    None => {
                        self.status = "暂无可启动的实例，请在环境配置中检查游戏目录。".into();
                        return Task::none();
                    }
                };
                if self.accounts.busy {
                    self.status = "请等待账户操作完成。".into();
                    return Task::none();
                }
                let Some(account) = self.accounts.index.current() else {
                    self.status = "请先登录 Microsoft 账户。".into();
                    self.open_modal(Modal::Profile);
                    return Task::none();
                };
                let provider = match &self.accounts.provider {
                    Ok(provider) => provider.clone(),
                    Err(e) => {
                        self.status = e.clone();
                        self.open_modal(Modal::Profile);
                        return Task::none();
                    }
                };
                self.launching = true;
                self.status = "正在验证账户、Java 和游戏依赖…".into();
                return crate::local::launch(
                    self.environment.clone(),
                    key,
                    account.id.clone(),
                    provider,
                );
            }
            Message::GameStarted(status) => self.status = status,
            Message::GameFinished(result) => {
                self.launching = false;
                self.status = result.unwrap_or_else(|e| format!("游戏启动或运行失败：{e}"));
            }
            Message::Accounts(message) => {
                if self.launching {
                    return Task::none();
                }
                return self.accounts.update(message).map(Message::Accounts);
            }
            Message::GameRoot(value) => self.game_root_input = value,
            Message::JavaPath(value) => self.java_input = value,
            Message::SaveEnvironment => {
                if self.loading || self.environment_saving || self.launching {
                    return Task::none();
                }
                if self.game_root_input.trim().is_empty() || self.java_input.trim().is_empty() {
                    self.status = "游戏目录和 Java 路径不能为空。".into();
                    return Task::none();
                }
                let config = crate::local::Environment {
                    game_root: self.game_root_input.trim().into(),
                    java: self.java_input.trim().into(),
                };
                self.environment_saving = true;
                return Task::perform(
                    async move { config.save().await.map(|_| config) },
                    Message::EnvironmentSaved,
                );
            }
            Message::EnvironmentSaved(result) => {
                self.environment_saving = false;
                match result {
                    Ok(config) => {
                        self.environment = config;
                        self.loading = true;
                        return self.load();
                    }
                    Err(e) => self.status = format!("保存环境配置失败：{e}"),
                }
            }
            Message::EditInstance => self.open_modal(Modal::Instance),
            Message::Dismiss => self.dismiss_modal(),
            Message::Refresh => {
                if !self.loading && !self.environment_saving && !self.launching {
                    self.loading = true;
                    return self.load();
                }
            }
            Message::ToggleTask(id) => {
                let _ = id;
                self.status = "下载服务尚未接入，无法暂停或恢复任务。".into();
            }
            Message::ToggleAllTasks => {
                self.status = "下载服务尚未接入，暂无可操作的下载任务。".into();
            }
            Message::Feature(delta) => {
                self.feature = (self.feature as i32 + delta).rem_euclid(4) as usize
            }
            Message::FeatureDetails => self.open_modal(Modal::Feature),
            Message::Changelogs => self.open_modal(Modal::Changelog),
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
                self.dismiss_modal();
                self.drag = None;
                self.edit_layout = false;
            }
            Message::Event(_) => {}
            Message::DragWindow => return window::latest().and_then(window::drag),
            Message::Minimize => return window::latest().and_then(|id| window::minimize(id, true)),
            Message::Maximize => return window::latest().and_then(window::toggle_maximize),
            Message::Close => return iced::exit(),
            Message::Profile => self.open_modal(Modal::Profile),
            Message::Settings(message) => {
                self.animation_time = Instant::now().max(self.animation_time);
                self.settings.update(message, self.animation_time);
            }
            Message::Download(message) => {
                return self.download.update(message).map(Message::Download)
            }
            Message::Ai(message) => {
                self.animation_time = Instant::now().max(self.animation_time);
                self.ai.update(message, self.animation_time);
            }
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
        let mut subscriptions = vec![window];
        if self.edit_layout && self.page == Page::Home && self.modal.is_none() {
            subscriptions.push(event::listen_with(layout_event));
        }
        if self.animating() {
            subscriptions.push(
                iced::time::every(std::time::Duration::from_millis(16)).map(Message::Animate),
            );
        }
        Subscription::batch(subscriptions)
    }

    pub fn animating(&self) -> bool {
        self.page_motion.active(self.animation_time)
            || self.card_motion.active(self.animation_time)
            || self.modal_motion.active(self.animation_time)
            || self.settings.animating(self.animation_time)
            || self.ai.animating(self.animation_time)
    }

    fn open_modal(&mut self, modal: Modal) {
        let now = Instant::now().max(self.animation_time);
        self.animation_time = now;
        if self.modal != Some(modal) {
            self.modal_motion = Tween::new(0.0, now);
        }
        self.modal = Some(modal);
        self.modal_motion.to(1.0, motion::OPEN_DURATION, now);
    }

    fn dismiss_modal(&mut self) {
        if self.modal.is_some() {
            let now = Instant::now().max(self.animation_time);
            self.animation_time = now;
            self.modal_motion.to(0.0, motion::CLOSE_DURATION, now);
            if !self.modal_motion.active(now) {
                self.modal = None;
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        shell::view(self)
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
