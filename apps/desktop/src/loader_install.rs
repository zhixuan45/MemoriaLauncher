use crate::{
    app::Message as AppMessage,
    models::InstanceEntry,
    motion_renderer::Element,
    theme::{self, bold, label, MUTED},
};
use iced::{
    widget::{button, column, pick_list, row, scrollable, text_input},
    Fill, Task,
};
use mc_core::{
    runtime::loaders::{self, Catalog, Kind, Plan, Version},
    DownloadClient, DownloadProgress,
};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base {
    pub id: String,
    pub name: String,
    pub minecraft: String,
}
impl std::fmt::Display for Base {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} · {}", self.name, self.minecraft)
    }
}
#[derive(Debug, Clone)]
pub enum Message {
    Base(Base),
    Kind(Kind),
    Version(Version),
    Java(String),
    Query,
    Versions(u64, Result<Vec<Version>, String>),
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
    pub bases: Vec<Base>,
    pub base: Option<Base>,
    pub kind: Kind,
    pub versions: Vec<Version>,
    pub version: Option<Version>,
    pub plan: Option<Plan>,
    pub java: String,
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
            bases: vec![],
            base: None,
            kind: Kind::Fabric,
            versions: vec![],
            version: None,
            plan: None,
            java: String::new(),
            notice: String::new(),
            progress: None,
        }
    }
}
impl State {
    pub fn open(
        &mut self,
        root: PathBuf,
        instances: &[InstanceEntry],
        active: &str,
        java: PathBuf,
    ) {
        let bases: Vec<_> = instances
            .iter()
            .filter(|entry| {
                entry.installed
                    && entry
                        .instance
                        .loader
                        .as_ref()
                        .is_none_or(|loader| loader.loader_type == mc_core::LoaderType::Vanilla)
            })
            .map(|entry| Base {
                id: entry.instance.id.clone(),
                name: entry.instance.name.clone(),
                minecraft: entry.instance.mc_version.clone(),
            })
            .collect();
        let base = bases
            .iter()
            .find(|base| base.id == active)
            .or_else(|| bases.first())
            .cloned();
        *self = Self {
            root,
            generation: self.generation + 1,
            base,
            bases,
            java: java.to_string_lossy().into_owned(),
            notice: "选择已安装的原版实例和加载器，查询可用版本。安装会创建新的独立实例。".into(),
            ..Default::default()
        };
    }
    fn invalidate(&mut self) {
        self.generation += 1;
        self.busy = false;
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
                            .or_else(|| versions.first())
                            .cloned();
                        self.versions = versions;
                        self.notice = if self.versions.is_empty() {
                            "官方目录未提供兼容版本。"
                        } else {
                            "选择具体版本后生成安装预览。"
                        }
                        .into();
                    }
                    Err(e) => self.notice = e,
                }
            }
            Message::Prepared(g, result) if g == self.generation => {
                self.busy = false;
                match result {
                    Ok(plan) => {
                        self.plan = Some(plan);
                        self.notice = "请核对新实例、库文件及安装方式后确认。".into();
                    }
                    Err(e) => self.notice = e,
                }
            }
            Message::Progress(g, value) if g == self.generation => self.progress = Some(value),
            Message::Finished(g, result) if g == self.generation => {
                self.busy = false;
                self.installing = false;
                self.plan = None;
                self.notice = match result {
                    Ok(id) => format!("加载器安装完成：{id}"),
                    Err(e) => format!("安装失败：{e}"),
                };
            }
            Message::Versions(..)
            | Message::Prepared(..)
            | Message::Progress(..)
            | Message::Finished(..) => {}
            _ if blocked || self.installing => return Task::none(),
            Message::Base(base) => {
                if self.bases.contains(&base) {
                    self.invalidate();
                    self.base = Some(base);
                    self.versions.clear();
                    self.version = None;
                }
            }
            Message::Kind(kind) => {
                self.invalidate();
                self.kind = kind;
                self.versions.clear();
                self.version = None;
            }
            Message::Version(version) => {
                if self.versions.contains(&version) {
                    self.invalidate();
                    self.version = Some(version);
                }
            }
            Message::Java(value) => {
                self.invalidate();
                self.java = value;
            }
            Message::Query => {
                let Some(base) = self.base.clone() else {
                    self.notice = "请先下载并安装原版 Minecraft。".into();
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
                self.versions.clear();
                self.version = None;
                self.busy = true;
                self.notice = "正在查询官方加载器目录…".into();
                let kind = self.kind;
                let g = self.generation;
                return Task::perform(
                    async move {
                        Catalog::new(client)
                            .versions(kind, &base.minecraft)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    move |r| Message::Versions(g, r),
                );
            }
            Message::Preview => {
                let (Some(base), Some(version)) = (self.base.clone(), self.version.clone()) else {
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
                self.notice = "正在验证游戏兼容性、固定版本与文件哈希…".into();
                let root = self.root.clone();
                let kind = self.kind;
                let g = self.generation;
                return Task::perform(
                    async move {
                        Catalog::new(client)
                            .preview(root, base.id, kind, version.version)
                            .await
                            .map_err(|e| e.to_string())
                    },
                    move |r| Message::Prepared(g, r),
                );
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
                self.notice = if plan.kind().requires_installer() {
                    "正在校验文件并运行官方安装器；可在暂存记录中检查安装日志。"
                } else {
                    "正在下载并校验加载器库文件…"
                }
                .into();
                let java = PathBuf::from(self.java.trim());
                let g = self.generation;
                use iced::futures::SinkExt;
                return Task::run(
                    iced::stream::channel(
                        64,
                        move |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
                            let sender = std::sync::Arc::new(std::sync::Mutex::new(output.clone()));
                            let progress = std::sync::Arc::new(move |value| {
                                if let Ok(mut sender) = sender.lock() {
                                    let _ = sender.try_send(Message::Progress(g, value));
                                }
                            });
                            let result = loaders::install(client, plan, java, progress)
                                .await
                                .map_err(|e| e.to_string());
                            let _ = output.send(Message::Finished(g, result)).await;
                        },
                    ),
                    |message| message,
                );
            }
        }
        Task::none()
    }
}
fn dispatch(message: Message) -> AppMessage {
    AppMessage::LoaderInstall(message)
}
pub fn view(state: &State, blocked: bool) -> Element<'_, AppMessage> {
    let enabled = !blocked && !state.busy;
    let mut body = column![
        label("基础原版实例（不覆盖；安装后新增独立实例）", 12.),
        pick_list(state.bases.clone(), state.base.clone(), |v| dispatch(
            Message::Base(v)
        ))
        .width(Fill),
        row![
            pick_list(Kind::ALL, Some(state.kind), |v| dispatch(Message::Kind(v))),
            button(label("查询兼容版本", 12.))
                .padding(10)
                .style(theme::quiet)
                .on_press_maybe(enabled.then_some(dispatch(Message::Query)))
        ]
        .spacing(12),
        pick_list(state.versions.clone(), state.version.clone(), |v| dispatch(
            Message::Version(v)
        ))
        .width(Fill),
        label("安装与启动使用的 Java 路径", 12.),
        text_input("java", &state.java)
            .padding(10)
            .on_input(|v| dispatch(Message::Java(v))),
        button(label("生成加载器安装预览", 12.))
            .padding(10)
            .style(theme::quiet)
            .on_press_maybe(
                (enabled && state.version.is_some()).then_some(dispatch(Message::Preview))
            ),
    ]
    .spacing(10);
    if state.bases.is_empty() {
        body = body
            .push(label("没有已安装的原版基础实例，请先在游戏下载中安装原版。", 12.).color(MUTED));
    }
    if let Some(plan) = &state.plan {
        body = body
            .push(bold(format!("新实例：{}", plan.target()), 15.))
            .push(label(
                format!(
                    "{} {} · Minecraft {} · 要求 Java {}",
                    plan.kind(),
                    plan.version(),
                    plan.minecraft(),
                    plan.required_java()
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "未声明".into())
                ),
                12.,
            ));
        if let Some(installer) = plan.installer() {
            body=body.push(label("确认后将运行官方 Java 安装器，在暂存目录生成加载器文件。此过程可能继续联网下载依赖。",12.))
                .push(label(format!("{}\nSHA-1：{}",installer.url,installer.sha1.as_deref().unwrap_or_default()),10.).color(MUTED));
        }
        for file in plan.files() {
            body = body.push(
                label(
                    format!(
                        "{} · SHA-1 {}",
                        file.path,
                        file.sha1
                            .as_deref()
                            .unwrap_or("安装器产物，完成后记录 SHA-256")
                    ),
                    10.,
                )
                .color(MUTED),
            );
        }
    }
    if let Some(progress) = &state.progress {
        body = body.push(label(
            format!(
                "已校验 {} / {} 个下载文件，复用 {} 个",
                progress.completed, progress.total, progress.reused
            ),
            11.,
        ));
    }
    column![
        scrollable(body)
            .direction(theme::scroll_direction())
            .height(Fill),
        label(&state.notice, 12.).color(MUTED),
        button(label("确认安装加载器", 13.))
            .padding(12)
            .style(theme::action)
            .on_press_maybe(
                (enabled && state.plan.is_some() && !state.java.trim().is_empty())
                    .then_some(dispatch(Message::Confirm))
            )
    ]
    .spacing(12)
    .height(420)
    .into()
}
