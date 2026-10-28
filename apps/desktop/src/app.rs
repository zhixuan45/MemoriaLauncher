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
    #[cfg(test)]
    pub const ALL: [Self; 4] = [Self::Home, Self::Download, Self::Ai, Self::Settings];
    pub fn title(self) -> &'static str {
        match self {
            Self::Home => "主页",
            Self::Download => "游戏下载",
            Self::Ai => "AI 对话",
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
    GameSettings(crate::game_settings::Message),
    DownloadSettings(crate::download_settings::Message),
    AiSettings(crate::ai_settings::Message),
    McpSettings(crate::mcp_settings::Message),
    InstanceSettings(crate::instance_settings::Message),
    Appearance(crate::appearance::Message),
    ResourceInstall(crate::resource_install::Message),
    OpenLoader,
    LoaderInstall(crate::loader_install::Message),
    OpenDiagnostics,
    Diagnostics(crate::diagnostics::Message),
    GameStarted(String),
    GameFinished(Result<crate::local::GameExit, String>),
    AnalyzeCrash,
    ShowCrash,
    Navigate(Page),
    SelectInstance(String),
    LaunchInstance(std::path::PathBuf, String),
    Workspace(crate::workspace::Message),
    ChatReply(uuid::Uuid, crate::ai::Message),
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
    Quit,
    Profile,
    LoginSettings,
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
    pub game_settings: crate::game_settings::State,
    pub download_settings: crate::download_settings::State,
    pub environment_saving: bool,
    pub launching: bool,
    pub crash: Option<crate::local::GameExit>,
    pub settings: crate::settings::State,
    pub download: crate::download::State,
    pub ai: crate::ai::State,
    pub workspace: crate::workspace::State,
    pub selections: crate::selections::State,
    pub ai_settings: crate::ai_settings::State,
    pub mcp_settings: crate::mcp_settings::State,
    pub instance_settings: crate::instance_settings::State,
    pub appearance: crate::appearance::State,
    pub resource_install: crate::resource_install::State,
    pub loader_install: crate::loader_install::State,
    pub diagnostics: crate::diagnostics::State,
    pub verification: verification::Verification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Modal {
    Instance,
    Changelog,
    Feature,
    Profile,
    Crash,
    ResourceInstall,
    LoaderInstall,
    Diagnostics,
}

impl Launcher {
    pub fn new() -> (Self, Task<Message>) {
        let (environment, environment_error) = crate::local::Environment::load();
        let download_settings = crate::download_settings::State::load();
        let download = crate::download::State::new(download_settings.saved.clone());
        let ai_settings = crate::ai_settings::State::load();
        let mcp_settings = crate::mcp_settings::State::load(&environment.game_root);
        let mut ai = crate::ai::State::default();
        ai.connection = ai_settings.saved.clone();
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
            game_settings: crate::game_settings::State::new(&environment),
            download_settings,
            environment,
            environment_saving: false,
            launching: false,
            crash: None,
            settings: crate::settings::State::load(),
            download,
            ai,
            workspace: crate::workspace::State::default(),
            selections: crate::selections::State::load(),
            ai_settings,
            mcp_settings,
            instance_settings: Default::default(),
            appearance: crate::appearance::State::load(),
            resource_install: Default::default(),
            loader_install: Default::default(),
            diagnostics: Default::default(),
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
    pub fn page_instance(&self) -> Option<&InstanceEntry> {
        if self.page == Page::Ai {
            self.workspace.current.project.as_ref()
                .filter(|p|crate::local::same_root(&p.root,&self.environment.game_root))
                .and_then(|p|self.instances.iter().find(|i|i.instance.id==p.instance_id))
        } else {self.current()}
    }
    pub fn theme(&self) -> iced::Theme { self.appearance.theme() }
    pub fn scale_factor(&self) -> f32 { self.appearance.saved.scale as f32 / 100. }
    pub fn instance_writing(&self) -> bool {
        self.instance_settings.busy || self.resource_install.busy || self.loader_install.installing
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::OpenLoader => {
                if self.instance_writing()
                    || self.launching
                    || self.loading
                    || self.environment_saving
                    || self.download.installing.is_some()
                {
                    return Task::none();
                }
                self.loader_install.open(
                    self.environment.game_root.clone(),
                    &self.instances,
                    &self.active,
                    self.environment.java.clone(),
                );
                self.open_modal(Modal::LoaderInstall);
            }
            Message::LoaderInstall(message) => {
                let refresh = matches!(&message, crate::loader_install::Message::Finished(g, Ok(_)) if *g == self.loader_install.generation);
                let blocked = self.launching
                    || self.loading
                    || self.environment_saving
                    || self.download.installing.is_some()
                    || self.instance_settings.busy
                    || self.resource_install.busy
                    || !crate::local::same_root(&self.loader_install.root, &self.environment.game_root);
                let task = self.loader_install
                    .update(message, self.download.client.clone(), blocked)
                    .map(Message::LoaderInstall);
                self.status = self.loader_install.notice.clone();
                if refresh && crate::local::same_root(&self.loader_install.root, &self.environment.game_root) {
                    return Task::batch([task, self.load()]);
                }
                return task;
            }
            Message::OpenDiagnostics => {
                self.open_modal(Modal::Diagnostics);
                let instance = self.page_instance().map(|entry| entry.instance.id.clone()).unwrap_or_default();
                return self.diagnostics.open(
                    self.environment.game_root.clone(), instance, None, None,
                ).map(Message::Diagnostics);
            }
            Message::Diagnostics(message) => return self.diagnostics.update(message).map(Message::Diagnostics),
            Message::Appearance(message) => return self.appearance.update(message).map(Message::Appearance),
            Message::InstanceSettings(message) => {
                let refresh=matches!(&message,crate::instance_settings::Message::Completed(g,Ok(_),notice) if *g==self.instance_settings.generation && notice!="实例已读取");
                let blocked=self.launching||self.loading||self.environment_saving||self.download.installing.is_some()||self.resource_install.busy||self.loader_install.installing;
                let task=if let Some(task)=self.instance_settings.picker(&message){task}else{self.instance_settings.update(message,blocked)}.map(Message::InstanceSettings);
                if refresh && crate::local::same_root(&self.instance_settings.root,&self.environment.game_root){return Task::batch([task,self.load()]);}
                return task;
            }
            Message::ResourceInstall(message) => {
                let refresh=matches!(&message,crate::resource_install::Message::Finished(g,Ok(_)) if *g==self.resource_install.generation);
                let blocked=self.launching||self.loading||self.environment_saving||self.download.installing.is_some()||self.instance_settings.busy||self.loader_install.installing||!self.environment.game_roots.iter().chain(std::iter::once(&self.environment.game_root)).any(|root|crate::local::same_root(root,&self.resource_install.root));
                let task=self.resource_install.update(message,self.download.client.clone(),blocked).map(Message::ResourceInstall);
                self.status=self.resource_install.notice.clone();
                if refresh && crate::local::same_root(&self.resource_install.root,&self.environment.game_root){return Task::batch([task,self.load()]);}
                return task;
            }
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
                        self.instances = instances;
                        self.status = data.notice;
                        crate::selections::restore(self);
                    }
                    Err(e) => self.status = format!("读取失败：{e}"),
                }
                if self.verification.enabled() {
                    return verification::after_load(self);
                }
                return self.settings.startup();
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
                if page == Page::Settings && self.settings.game_settings_visible() {
                    return self
                        .game_settings
                        .ensure_java_loaded()
                        .map(Message::GameSettings);
                }
            }
            Message::SelectInstance(id) => {
                if self.instances.iter().any(|i| i.instance.id == id) {
                    self.active = id;
                    crate::selections::remember(self);
                }
            }
            Message::Workspace(message) => return crate::workspace::update(self, message),
            Message::ChatReply(id, message) => {
                return crate::workspace::update_ai(self, id, message)
            }
            Message::LaunchInstance(root, id) => {
                if self.instance_writing() { return Task::none(); }
                if !crate::local::same_root(&root, &self.environment.game_root) {
                    self.status = "实例集已变化，请从当前项目重新启动。".into();
                    return Task::none();
                }
                if self.launching
                    || self.loading
                    || self.environment_saving
                    || self.download.installing.is_some()
                {
                    return Task::none();
                }
                if !self.instances.iter().any(|i| i.instance.id == id) {
                    return Task::none();
                }
                self.active = id;
                crate::selections::remember(self);
                return self.update(Message::Launch);
            }
            Message::MoveInstance(id, delta) => {
                if let Some(index) = self.instances.iter().position(|i| i.instance.id == id) {
                    let target =
                        (index as i32 + delta).clamp(0, self.instances.len() as i32 - 1) as usize;
                    self.instances.swap(index, target);
                }
            }
            Message::Launch => {
                if self.instance_writing() { return Task::none(); }
                if self.launching
                    || self.loading
                    || self.environment_saving
                    || self.download.installing.is_some()
                {
                    return Task::none();
                }
                let key = match self.current() {
                    Some(i) if !i.installed => {
                        self.status = "客户端 JAR 缺失，请先安装完整游戏。".into();
                        return Task::none();
                    }
                    Some(i) => i.instance.id.clone(),
                    None => {
                        self.status =
                            "暂无可启动的实例，请在设置 → 游戏设置中检查实例集文件夹。".into();
                        return Task::none();
                    }
                };
                if let Err(error) = self.selections.launched(&self.environment.game_root, &key) {
                    self.selections.notice = Some(format!("启动选择保存失败：{error}"));
                }
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
                        if account.is_offline() {
                            crate::accounts::Provider::new(
                                mc_core::MicrosoftAuthConfig::offline_dummy(),
                                std::sync::Arc::new(crate::credentials::SystemCredentialStore),
                            )
                        } else {
                            self.status = e.clone();
                            self.open_modal(Modal::Profile);
                            return Task::none();
                        }
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
            Message::GameStarted(status) => {
                self.status = status;
                if self.settings.saved.minimize_on_launch {
                    return window::latest().and_then(|id| window::minimize(id, true));
                }
            }
            Message::GameFinished(result) => {
                self.launching = false;
                match result {
                    Ok(finished) => {
                        self.status = finished.description();
                        let path = finished.exit.log_path.file_name()
                            .map(|name| format!(".launcher/logs/{}", name.to_string_lossy()));
                        let diagnostics = self.diagnostics.open(
                            finished.game_root.clone(), finished.instance_id.clone(), path, finished.exit.code,
                        ).map(Message::Diagnostics);
                        if !finished.exit.success {
                            self.crash = Some(finished);
                            self.open_modal(Modal::Crash);
                        }
                        return diagnostics;
                    }
                    Err(error) => self.status = format!("游戏启动或退出监测失败：{error}"),
                }
            }
            Message::AnalyzeCrash => {
                if !self.ai.busy && !self.ai.loading_log {
                    if let Some(crash) = self.crash.clone() {
                        let context = crate::workspace::project(self, &crash.instance_id)
                            .filter(|p| crate::local::same_root(&p.root, &crash.game_root));
                        self.workspace.new_chat(&mut self.ai, context);
                        crate::selections::remember(self);
                        let navigation = self.update(Message::Navigate(Page::Ai));
                        let analysis = crate::workspace::update_ai(
                            self,
                            self.workspace.current.id,
                            crate::ai::Message::AnalyzeCrash(crash),
                        );
                        return Task::batch([navigation, analysis]);
                    }
                }
            }
            Message::ShowCrash => {
                if self.crash.is_some() {
                    self.open_modal(Modal::Crash);
                }
            }
            Message::Accounts(message) => {
                if self.launching {
                    return Task::none();
                }
                return self.accounts.update(message).map(Message::Accounts);
            }
            Message::GameSettings(message) => {
                let writing=self.instance_writing();
                let saved = match &message {
                    crate::game_settings::Message::Saved(Ok(config)) => Some(config.clone()),
                    _ => None,
                };
                let task = self
                    .game_settings
                    .update(
                        message,
                        self.loading || self.launching || self.download.installing.is_some() || writing,
                    )
                    .map(Message::GameSettings);
                self.environment_saving = self.game_settings.busy;
                if let Some(config) = saved {
                    if !crate::local::same_root(&self.environment.game_root, &config.game_root) {
                        crate::selections::remember(self);
                        // Version IDs are only unique within one collection.
                        self.active.clear();
                        self.instances.clear();
                        self.selections.restore_pending = true;
                    }
                    self.environment = config;
                    self.loading = true;
                    return Task::batch([task, self.load()]);
                }
                return task;
            }
            Message::DownloadSettings(message) => {
                let saved = match &message {
                    crate::download_settings::Message::Saved(Ok(config)) => Some(config.clone()),
                    _ => None,
                };
                let task = self
                    .download_settings
                    .update(message)
                    .map(Message::DownloadSettings);
                if let Some(config) = saved {
                    self.download.configure(config);
                    if self.page == Page::Download {
                        return Task::batch([
                            task,
                            self.download.ensure_loaded().map(Message::Download),
                        ]);
                    }
                }
                return task;
            }
            Message::EditInstance => {
                if self.instance_writing() {return Task::none();}
                if let Some(instance)=self.page_instance(){
                    let key=instance.instance.id.clone();
                    let task=self.instance_settings.open(self.environment.game_root.clone(),key).map(Message::InstanceSettings);
                    self.open_modal(Modal::Instance); return task;
                }
            }
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
            Message::Event(Event::Window(window::Event::CloseRequested)) => {
                return self.update(Message::Close)
            }
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
            Message::Close => {
                crate::selections::remember(self);
                if self.settings.saved.minimize_on_close {
                    return window::latest().and_then(|id| window::minimize(id, true));
                }
                return iced::exit();
            }
            Message::Quit => {
                crate::selections::remember(self);
                return iced::exit();
            }
            Message::Profile => self.open_modal(Modal::Profile),
            Message::LoginSettings => {
                let _ = self.settings.update(
                    crate::settings::Message::Tab(crate::settings::LOGIN_TAB),
                    self.animation_time,
                );
                return self.update(Message::Navigate(Page::Settings));
            }
            Message::Settings(message) => {
                self.animation_time = Instant::now().max(self.animation_time);
                // Cache maintenance waits until all application writes are idle.
                if matches!(message, crate::settings::Message::ConfirmCache)
                    && (self.launching
                        || self.instance_writing()
                        || self.loading
                        || self.environment_saving
                        || self.download.installing.is_some())
                {
                    self.status = "请等待当前扫描、保存、游戏运行或安装完成后再清理缓存。".into();
                    return Task::none();
                }
                let task = self
                    .settings
                    .update(message, self.animation_time)
                    .map(Message::Settings);
                if self.page == Page::Settings && self.settings.game_settings_visible() {
                    return Task::batch([
                        task,
                        self.game_settings
                            .ensure_java_loaded()
                            .map(Message::GameSettings),
                    ]);
                }
                return task;
            }
            Message::Download(message) => {
                if let crate::download::Message::Resources(crate::resources::Message::Install(id))=&message {
                    if self.instance_writing()||self.launching||self.loading||self.environment_saving||self.download.installing.is_some(){self.status="请等待当前实例操作完成。".into();return Task::none();}
                    if let Some((kind,project,version,selection))=self.download.resources.install_selection(id){
                        self.resource_install.open(self.environment.game_root.clone(),&self.instances,&self.active,kind,project,version,selection);
                        let preferred = self.selections.last_launched().filter(|p| self.environment.game_roots.iter().chain(std::iter::once(&self.environment.game_root)).any(|root|crate::local::same_root(root,&p.root))).cloned();
                        let task = if kind==mc_core::ResourceKind::Mod {self.resource_install.restore_default(preferred)} else {Task::none()};
                        self.open_modal(Modal::ResourceInstall);
                        return task.map(Message::ResourceInstall);
                    }
                    return Task::none();
                }
                if let crate::download::Message::Install(id) = &message {
                    if self.instance_writing() {self.download.notice="请等待当前实例操作完成。".into();return Task::none();}
                    if self.download.installing.is_some()
                        || self.loading
                        || self.environment_saving
                        || self.launching
                    {
                        self.download.notice =
                            "请等待当前扫描、保存、游戏运行或安装任务完成。".into();
                        return Task::none();
                    }
                    if !self
                        .download
                        .versions
                        .iter()
                        .any(|version| &version.id == id)
                    {
                        self.download.notice = "请先从当前版本清单选择要安装的版本。".into();
                        return Task::none();
                    }
                    let client = match &self.download.client {
                        Ok(client) => client.clone(),
                        Err(error) => {
                            self.download.notice = error.clone();
                            return Task::none();
                        }
                    };
                    let id = id.clone();
                    let _ = self.download.update(message);
                    return crate::download::install(
                        client,
                        self.environment.game_root.clone(),
                        id,
                    )
                    .map(Message::Download);
                }
                let refresh = matches!(&message, crate::download::Message::Installed(_, root, Ok(()))
                    if crate::local::same_root(root, &self.environment.game_root));
                let task = self.download.update(message).map(Message::Download);
                if refresh && !self.loading {
                    self.loading = true;
                    return Task::batch([task, self.load()]);
                }
                return task;
            }
            Message::AiSettings(message) => {
                let task = self.ai_settings.update(message).map(Message::AiSettings);
                self.ai.connection = self.ai_settings.saved.clone();
                for conversation in &mut self.workspace.others {
                    conversation.state.connection = self.ai_settings.saved.clone();
                }
                return task;
            }
            Message::McpSettings(message) => {
                return self
                    .mcp_settings
                    .update(message, &self.environment.game_root)
                    .map(Message::McpSettings)
            }
            Message::Ai(crate::ai::Message::OpenSettings) => {
                let _ = self.settings.update(
                    crate::settings::Message::Tab(crate::settings::AI_TAB),
                    self.animation_time,
                );
                return self.update(Message::Navigate(Page::Settings));
            }
            Message::Ai(message) => {
                self.animation_time = Instant::now().max(self.animation_time);
                return crate::workspace::update_ai(self, self.workspace.current.id, message);
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
        if self.appearance.saved.accent_mode==crate::appearance::AccentMode::System {
            subscriptions.push(iced::time::every(std::time::Duration::from_secs(2)).map(|_|Message::Appearance(crate::appearance::Message::RefreshSystem)));
        }
        if self.page == Page::Settings && self.settings.game_settings_visible() {
            subscriptions.push(
                iced::time::every(std::time::Duration::from_secs(3))
                    .map(|_| Message::GameSettings(crate::game_settings::Message::RefreshMemory)),
            );
        }
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
        Event::Window(
            window::Event::Resized(_)
                | window::Event::Unfocused
                | window::Event::CloseRequested
        )
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
