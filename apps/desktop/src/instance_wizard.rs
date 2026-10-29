//! A single desktop entry for standalone loader instances and local/remote mrpack imports.
use crate::{
    app::{Launcher, Message as AppMessage, Modal},
    motion_renderer::Element,
    theme::{self, bold, label, MUTED},
};
use iced::{
    widget::{button, checkbox, column, pick_list, progress_bar, row, scrollable, text_input},
    Fill, Task,
};
use mc_core::{
    runtime::loaders::{Catalog, Kind, Version},
    service::packs::{
        exchange::{self, Pack},
        install::{self, Plan, Request},
        Loader,
    },
    DownloadClient, DownloadProgress,
};
use std::path::PathBuf;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Vanilla,
    Fabric,
    Quilt,
    Forge,
    NeoForge,
}
impl Choice {
    const ALL: [Self; 5] = [
        Self::Vanilla,
        Self::Fabric,
        Self::Quilt,
        Self::Forge,
        Self::NeoForge,
    ];
    fn kind(self) -> Option<Kind> {
        match self {
            Self::Vanilla => None,
            Self::Fabric => Some(Kind::Fabric),
            Self::Quilt => Some(Kind::Quilt),
            Self::Forge => Some(Kind::Forge),
            Self::NeoForge => Some(Kind::NeoForge),
        }
    }
    fn from_kind(kind: Kind) -> Self {
        match kind {
            Kind::Fabric => Self::Fabric,
            Kind::Quilt => Self::Quilt,
            Kind::Forge => Self::Forge,
            Kind::NeoForge => Self::NeoForge,
        }
    }
}
impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Vanilla => "原版",
            Self::Fabric => "Fabric",
            Self::Quilt => "Quilt",
            Self::Forge => "Forge",
            Self::NeoForge => "NeoForge",
        })
    }
}
#[derive(Debug, Clone)]
pub enum Message {
    Minecraft(String),
    Target(String),
    Java(String),
    Kind(Choice),
    Version(Version),
    Optional(bool),
    ClearPack,
    Query,
    Versions(u64, Result<Vec<Version>, String>),
    PickPack,
    PackPicked(u64, Option<PathBuf>),
    PackLoaded(u64, Result<Pack, String>),
    Preview,
    Prepared(u64, Result<Plan, String>),
    Confirm,
    Progress(u64, DownloadProgress),
    Finished(u64, Result<String, String>),
}
pub struct State {
    pub root: PathBuf,
    pub generation: u64,
    pub busy: bool,
    pub installing: bool,
    pub minecraft: String,
    pub target: String,
    pub java: String,
    pub choice: Choice,
    pub versions: Vec<Version>,
    pub version: Option<Version>,
    pub pack: Option<Pack>,
    pub optional: bool,
    pub plan: Option<Plan>,
    pub notice: String,
    pub progress: Option<DownloadProgress>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            root: PathBuf::new(),
            generation: 0,
            busy: false,
            installing: false,
            minecraft: "1.21.1".into(),
            target: String::new(),
            java: "java".into(),
            choice: Choice::Fabric,
            versions: vec![],
            version: None,
            pack: None,
            optional: false,
            plan: None,
            notice: String::new(),
            progress: None,
        }
    }
}
impl State {
    pub fn open(&mut self, root: PathBuf, minecraft: String, java: PathBuf) {
        *self = Self {
            root,
            generation: self.generation + 1,
            minecraft,
            target: format!(
                "instance-{}",
                &uuid::Uuid::new_v4().simple().to_string()[..8]
            ),
            java: java.to_string_lossy().into_owned(),
            notice: "选择游戏和加载器，或导入 .mrpack；预览后确认会新建独立实例。".into(),
            ..Default::default()
        };
    }
    fn invalidate(&mut self) {
        self.generation += 1;
        self.plan = None;
        self.progress = None;
    }
    pub fn update(
        &mut self,
        message: Message,
        client: Result<DownloadClient, String>,
        blocked: bool,
    ) -> Task<Message> {
        match message {
            Message::Versions(g, result) if g == self.generation => {
                self.busy = false;
                match result {
                    Ok(versions) => {
                        self.version = versions
                            .iter()
                            .find(|v| v.stable)
                            .or(versions.first())
                            .cloned();
                        self.versions = versions;
                        self.notice = "选择加载器版本后生成安装预览。".into();
                    }
                    Err(e) => self.notice = e,
                }
            }
            Message::PackPicked(g, path) if g == self.generation => {
                let Some(path) = path else {
                    self.busy = false;
                    return Task::none();
                };
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || exchange::read_pack(&path))
                            .await
                            .map_err(|e| e.to_string())
                            .and_then(|r| r)
                    },
                    move |result| Message::PackLoaded(g, result),
                );
            }
            Message::PackLoaded(g, result) if g == self.generation => {
                self.busy = false;
                match result {
                    Ok(pack) => {
                        self.minecraft = pack.minecraft.clone();
                        self.choice = pack
                            .loader
                            .as_ref()
                            .map(|l| Choice::from_kind(l.kind))
                            .unwrap_or(Choice::Vanilla);
                        self.version = pack.loader.as_ref().map(|l| Version {
                            version: l.version.clone(),
                            stable: true,
                        });
                        self.pack = Some(pack);
                        self.plan = None;
                        self.notice =
                            "已读取整合包要求；确认后会自动安装匹配的游戏与加载器。".into();
                    }
                    Err(e) => self.notice = e,
                }
            }
            Message::Prepared(g, result) if g == self.generation => {
                self.busy = false;
                match result {
                    Ok(plan) => {
                        self.plan = Some(plan);
                        self.notice = "核对版本、文件和安装方式后确认。".into();
                    }
                    Err(e) => self.notice = format!("预览失败：{e}"),
                }
            }
            Message::Progress(g, value) if g == self.generation => self.progress = Some(value),
            Message::Finished(g, result) if g == self.generation => {
                self.busy = false;
                self.installing = false;
                self.plan = None;
                self.notice = match result {
                    Ok(key) => format!("新实例安装完成：{key}"),
                    Err(e) => format!("安装失败：{e}"),
                };
            }
            Message::Versions(..)
            | Message::PackPicked(..)
            | Message::PackLoaded(..)
            | Message::Prepared(..)
            | Message::Progress(..)
            | Message::Finished(..) => {}
            _ if self.busy || blocked => {}
            Message::Minecraft(value) => {
                if self.pack.is_none() {
                    self.invalidate();
                    self.minecraft = value;
                    self.versions.clear();
                    self.version = None;
                }
            }
            Message::Target(value) => {
                self.invalidate();
                self.target = value;
            }
            Message::Java(value) => {
                self.invalidate();
                self.java = value;
            }
            Message::Kind(value) => {
                if self.pack.is_none() {
                    self.invalidate();
                    self.choice = value;
                    self.versions.clear();
                    self.version = None;
                }
            }
            Message::Version(value) => {
                if self.versions.contains(&value) {
                    self.invalidate();
                    self.version = Some(value);
                }
            }
            Message::Optional(value) => {
                self.invalidate();
                self.optional = value;
            }
            Message::ClearPack => {
                self.invalidate();
                self.pack = None;
                self.version = None;
                self.versions.clear();
            }
            Message::PickPack => {
                self.invalidate();
                self.busy = true;
                let g = self.generation;
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("导入 Modrinth 整合包")
                            .add_filter("Modrinth 整合包", &["mrpack"])
                            .pick_file()
                            .await
                            .map(|f| f.path().to_owned())
                    },
                    move |path| Message::PackPicked(g, path),
                );
            }
            Message::Query => {
                let Some(kind) = self.choice.kind() else {
                    return Task::none();
                };
                let client = match client {
                    Ok(c) => c,
                    Err(e) => {
                        self.notice = e;
                        return Task::none();
                    }
                };
                self.invalidate();
                self.busy = true;
                let g = self.generation;
                let mc = self.minecraft.clone();
                return Task::perform(
                    async move {
                        Catalog::new(client)
                            .versions(kind, &mc)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    move |result| Message::Versions(g, result),
                );
            }
            Message::Preview => {
                let client = match client {
                    Ok(c) => c,
                    Err(e) => {
                        self.notice = e;
                        return Task::none();
                    }
                };
                let loader = if let Some(kind) = self.choice.kind() {
                    let Some(version) = &self.version else {
                        self.notice = "请先查询并选择加载器版本。".into();
                        return Task::none();
                    };
                    Some(Loader {
                        kind,
                        version: version.version.clone(),
                    })
                } else {
                    None
                };
                let request = Request {
                    root: self.root.clone(),
                    target: self.target.trim().into(),
                    minecraft: self.minecraft.trim().into(),
                    loader,
                    java: self.java.trim().into(),
                    pack: self.pack.clone(),
                    optional: self.optional,
                };
                self.invalidate();
                self.busy = true;
                let g = self.generation;
                self.notice = "正在固定游戏、加载器与整合包文件清单…".into();
                return Task::perform(install::preview(request, client), move |result| {
                    Message::Prepared(g, result)
                });
            }
            Message::Confirm => {
                let Some(plan) = self.plan.take() else {
                    return Task::none();
                };
                let client = match client {
                    Ok(c) => c,
                    Err(e) => {
                        self.notice = e;
                        return Task::none();
                    }
                };
                self.busy = true;
                self.installing = true;
                let g = self.generation;
                self.notice = "正在暂存、校验并部署新实例…".into();
                use iced::futures::SinkExt;
                return Task::run(
                    iced::stream::channel(
                        64,
                        move |mut sender: iced::futures::channel::mpsc::Sender<Message>| async move {
                            let output = std::sync::Arc::new(std::sync::Mutex::new(sender.clone()));
                            let callback = std::sync::Arc::new(move |p| {
                                if let Ok(mut output) = output.lock() {
                                    let _ = output.try_send(Message::Progress(g, p));
                                }
                            });
                            let result = install::install(plan, client, callback).await;
                            let _ = sender.send(Message::Finished(g, result)).await;
                        },
                    ),
                    |message| message,
                );
            }
        }
        Task::none()
    }
}
pub fn open_app(app: &mut Launcher) -> Task<AppMessage> {
    if app.instance_writing()
        || app.launching
        || app.loading
        || app.environment_saving
        || app.download.installing.is_some()
    {
        return Task::none();
    }
    let selected = if app.page == crate::app::Page::Download && app.download.category == 0 {
        app.download
            .selected
            .as_ref()
            .filter(|id| app.download.versions.iter().any(|v| &v.id == *id))
            .cloned()
    } else {
        None
    };
    let minecraft = selected
        .or_else(|| app.current().map(|i| i.instance.mc_version.clone()))
        .unwrap_or_else(|| "1.21.1".into());
    app.instance_wizard.open(
        app.environment.game_root.clone(),
        minecraft,
        app.environment.java.clone(),
    );
    app.open_modal(Modal::InstanceWizard);
    Task::none()
}
pub fn update_app(app: &mut Launcher, message: Message) -> Task<AppMessage> {
    let installed = match &message {
        Message::Finished(g, Ok(key)) if *g == app.instance_wizard.generation => Some(key.clone()),
        _ => None,
    };
    let refresh =
        matches!(&message,Message::Finished(g,Ok(_)) if *g==app.instance_wizard.generation);
    let blocked = app.launching
        || app.loading
        || app.environment_saving
        || app.download.installing.is_some()
        || app.instance_settings.busy
        || app.resource_install.busy
        || app.loader_install.installing
        || app.change_plans.busy
        || app.pack_export.busy
        || !crate::local::same_root(&app.instance_wizard.root, &app.environment.game_root);
    let task = app
        .instance_wizard
        .update(message, app.download.client.clone(), blocked)
        .map(AppMessage::InstanceWizard);
    if refresh {
        if crate::local::same_root(&app.instance_wizard.root, &app.environment.game_root) {
            if let Some(key) = installed {
                app.active = key;
                crate::selections::remember(app);
            }
        }
        return Task::batch([task, app.update(AppMessage::Refresh)]);
    }
    task
}
pub fn remote_app(app: &mut Launcher, project: String, version: String) -> Task<AppMessage> {
    if app.instance_writing()
        || app.launching
        || app.loading
        || app.environment_saving
        || app.download.installing.is_some()
    {
        return Task::none();
    }
    let _ = open_app(app);
    let client = match &app.download.client {
        Ok(client) => client.clone(),
        Err(error) => {
            app.instance_wizard.notice = error.clone();
            return Task::none();
        }
    };
    app.instance_wizard.busy = true;
    let g = app.instance_wizard.generation;
    Task::perform(
        mc_core::service::resource_install::fetch_pack(client, project, version),
        move |result| AppMessage::InstanceWizard(Message::PackLoaded(g, result)),
    )
}
fn dispatch(message: Message) -> AppMessage {
    AppMessage::InstanceWizard(message)
}
pub fn view(state: &State, blocked: bool) -> Element<'_, AppMessage> {
    let enabled = !state.busy && !blocked;
    let mut body = column![
        label(format!("实例集：{}", state.root.display()), 11.).color(MUTED),
        row![
            button(label("导入 .mrpack", 12.))
                .style(theme::quiet)
                .padding(9)
                .on_press_maybe(enabled.then_some(dispatch(Message::PickPack))),
            button(label("清除整合包", 12.))
                .style(theme::quiet)
                .padding(9)
                .on_press_maybe(
                    (enabled && state.pack.is_some()).then_some(dispatch(Message::ClearPack))
                )
        ]
        .spacing(10),
        label("新实例 ID", 12.),
        text_input("例如 my-fabric-world", &state.target)
            .on_input_maybe(enabled.then_some(|v| dispatch(Message::Target(v))))
            .padding(9)
    ]
    .spacing(12);
    if let Some(pack) = &state.pack {
        body = body
            .push(bold(format!("{} · {}", pack.name, pack.version), 16.))
            .push(label(
                format!(
                    "Minecraft {} · {} {}",
                    pack.minecraft,
                    state.choice,
                    state
                        .version
                        .as_ref()
                        .map(|v| v.version.as_str())
                        .unwrap_or("")
                ),
                13.,
            ))
            .push(
                checkbox(state.optional)
                    .label("包含可选客户端文件")
                    .on_toggle_maybe(enabled.then_some(|v| dispatch(Message::Optional(v)))),
            );
    } else {
        body = body
            .push(label("Minecraft 版本", 12.))
            .push(
                text_input("例如 1.21.1", &state.minecraft)
                    .on_input_maybe(enabled.then_some(|v| dispatch(Message::Minecraft(v))))
                    .padding(9),
            )
            .push(
                row![
                    pick_list(Choice::ALL, Some(state.choice), |v| dispatch(
                        Message::Kind(v)
                    ))
                    .width(Fill),
                    button(label("查询加载器版本", 12.))
                        .style(theme::quiet)
                        .padding(9)
                        .on_press_maybe(
                            (enabled && state.choice.kind().is_some())
                                .then_some(dispatch(Message::Query))
                        )
                ]
                .spacing(10),
            );
        if state.choice.kind().is_some() {
            body = body.push(
                pick_list(state.versions.clone(), state.version.clone(), |v| {
                    dispatch(Message::Version(v))
                })
                .placeholder("请先查询并选择加载器版本")
                .width(Fill),
            );
        }
    }
    body = body.push(label("Java 路径", 12.)).push(
        text_input("java 或 Java 可执行文件路径", &state.java)
            .on_input_maybe(enabled.then_some(|v| dispatch(Message::Java(v))))
            .padding(9),
    );
    if let Some(plan) = &state.plan {
        body = body
            .push(bold(
                format!("{} → Minecraft {}", plan.target(), plan.minecraft()),
                15.,
            ))
            .push(label(
                format!(
                    "{} 个文件 · 已知 {:.1} MiB · Java {}",
                    plan.file_count(),
                    plan.known_bytes() as f64 / 1048576.,
                    plan.required_java()
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "未声明".into())
                ),
                12.,
            ));
        for warning in plan.warnings() {
            body = body.push(label(warning, 11.).color(MUTED));
        }
    }
    if let Some(p) = &state.progress {
        body = body
            .push(label(
                format!("当前阶段：{} / {} 个文件", p.completed, p.total),
                11.,
            ))
            .push(progress_bar(
                0.0..=p.total.max(1) as f32,
                p.completed as f32,
            ));
    }
    column![
        scrollable(body)
            .direction(theme::scroll_direction())
            .height(Fill),
        row![
            button(label("生成新实例预览", 12.))
                .style(theme::quiet)
                .padding(10)
                .on_press_maybe(enabled.then_some(dispatch(Message::Preview))),
            button(label("确认安装新实例", 12.))
                .style(theme::action)
                .padding(10)
                .on_press_maybe(
                    (enabled && state.plan.is_some()).then_some(dispatch(Message::Confirm))
                )
        ]
        .spacing(10),
        label(&state.notice, 11.).color(MUTED)
    ]
    .spacing(12)
    .height(Fill)
    .into()
}
