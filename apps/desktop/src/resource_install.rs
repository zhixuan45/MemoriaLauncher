use crate::{
    app::Message as AppMessage,
    models::InstanceEntry,
    motion_renderer::Element,
    theme::{self, bold, label, MUTED},
};
use iced::{
    widget::{button, checkbox, column, pick_list, progress_bar, row, scrollable, Space},
    Fill, Task,
};
use mc_core::{
    service::resource_install::{self as service, Preview, Progress, Request},
    DownloadClient, ResourceKind,
};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub id: String,
    pub label: String,
    pub root: PathBuf,
}
impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}
#[derive(Debug, Clone)]
pub enum Message {
    LastTargetLoaded(
        u64,
        crate::selections::Reference,
        Result<crate::local::LocalData, String>,
    ),
    Target(Target),
    Optional(bool),
    Prepare,
    Prepared(u64, Result<Preview, String>),
    Confirm,
    Progress(u64, Progress),
    Finished(u64, Result<String, String>),
}
#[derive(Default)]
pub struct State {
    pub busy: bool,
    pub root: PathBuf,
    pub request: Option<Request>,
    pub targets: Vec<Target>,
    pub target: Option<Target>,
    pub preview: Option<Preview>,
    pub notice: String,
    pub progress: Option<Progress>,
    pub generation: u64,
    pub selection: String,
}
impl State {
    pub fn open(
        &mut self,
        root: PathBuf,
        instances: &[InstanceEntry],
        active: &str,
        kind: ResourceKind,
        project: String,
        version: String,
        selection: String,
    ) {
        let generation = self.generation + 1;
        let targets: Vec<_> = instances
            .iter()
            .map(|i| Target {
                id: i.instance.id.clone(),
                root: root.clone(),
                label: format!(
                    "{} · {} · {}",
                    i.instance.name, i.instance.mc_version, i.loader
                ),
            })
            .collect();
        let target = targets
            .iter()
            .find(|v| v.id == active)
            .or(targets.first())
            .cloned();
        let key = target.as_ref().map(|v| v.id.clone()).unwrap_or_default();
        *self = Self {
            root: root.clone(),
            request: Some(Request {
                root,
                instance: key,
                kind,
                project,
                version,
                optional: false,
            }),
            targets,
            target,
            generation,
            selection,
            notice: "选择目标实例后生成预览；确认安装才会修改实例。".into(),
            ..Default::default()
        };
    }
    pub fn restore_default(
        &mut self,
        preferred: Option<crate::selections::Reference>,
    ) -> Task<Message> {
        let Some(preferred) = preferred else {
            return Task::none();
        };
        if crate::local::same_root(&preferred.root, &self.root) {
            self.select_remembered(&preferred);
            return Task::none();
        }
        self.busy = true;
        self.notice = "正在读取上次启动实例所在的实例集…".into();
        let generation = self.generation;
        Task::perform(crate::local::scan(preferred.root.clone()), move |result| {
            Message::LastTargetLoaded(generation, preferred.clone(), result)
        })
    }
    fn select_remembered(&mut self, preferred: &crate::selections::Reference) {
        if let Some(target) = self
            .targets
            .iter()
            .find(|t| {
                t.id == preferred.instance_id && crate::local::same_root(&t.root, &preferred.root)
            })
            .cloned()
        {
            self.root = target.root.clone();
            if let Some(request) = &mut self.request {
                request.root = target.root.clone();
                request.instance = target.id.clone();
            }
            self.target = Some(target);
            self.notice = "默认选择上次启动的实例；请核对目标后生成预览。".into();
        } else {
            self.notice = "上次启动的实例已不存在，已回退到主页选择；请核对目标实例。".into();
        }
    }
    pub fn update(
        &mut self,
        message: Message,
        client: Result<DownloadClient, String>,
        blocked: bool,
    ) -> Task<Message> {
        match message {
            Message::LastTargetLoaded(g, preferred, result) if g == self.generation => {
                self.busy = false;
                match result {
                    Ok(data) => {
                        self.targets
                            .extend(data.instances.iter().map(|instance| Target {
                                id: instance.instance.id.clone(),
                                root: preferred.root.clone(),
                                label: format!(
                                    "{} · {} · {} · {}",
                                    instance.instance.name,
                                    instance.instance.mc_version,
                                    instance.loader,
                                    preferred.root.display()
                                ),
                            }));
                        self.select_remembered(&preferred);
                    }
                    Err(error) => {
                        self.notice = format!("无法读取上次启动实例，保留主页目标：{error}")
                    }
                }
            }
            Message::LastTargetLoaded(..) => {}
            Message::Prepared(generation, result) if generation == self.generation => {
                self.busy = false;
                match result {
                    Ok(plan) => {
                        self.notice = "请检查目标实例和文件清单，然后确认安装。".into();
                        self.preview = Some(plan);
                    }
                    Err(e) => self.notice = format!("预览失败：{e}"),
                }
            }
            Message::Progress(generation, progress) if generation == self.generation => {
                self.notice = progress.message.clone();
                self.progress = Some(progress);
            }
            Message::Finished(generation, result) if generation == self.generation => {
                self.busy = false;
                self.notice = match result {
                    Ok(v) => v,
                    Err(e) => format!("安装失败：{e}。可重新生成预览后重试。"),
                };
            }
            Message::Prepared(..) | Message::Progress(..) | Message::Finished(..) => {}
            _ if self.busy || blocked => {}
            Message::Target(target) => {
                if self.targets.contains(&target) {
                    if let Some(request) = &mut self.request {
                        request.instance = target.id.clone();
                        request.root = target.root.clone();
                    }
                    self.root = target.root.clone();
                    self.target = Some(target);
                    self.preview = None;
                    self.progress = None;
                }
            }
            Message::Optional(value) => {
                if let Some(request) = &mut self.request {
                    request.optional = value;
                }
                self.preview = None;
            }
            Message::Prepare => {
                let Some(request) = self.request.clone().filter(|v| !v.instance.is_empty()) else {
                    self.notice = "请先导入或安装一个目标游戏实例。".into();
                    return Task::none();
                };
                let client = match client {
                    Ok(v) => v,
                    Err(e) => {
                        self.notice = e;
                        return Task::none();
                    }
                };
                self.busy = true;
                self.preview = None;
                self.progress = None;
                self.notice = "正在解析兼容性、必需依赖与安装文件…".into();
                let generation = self.generation;
                return Task::perform(service::prepare(request, client), move |result| {
                    Message::Prepared(generation, result)
                });
            }
            Message::Confirm => {
                let client = match client {
                    Ok(v) => v,
                    Err(e) => {
                        self.notice = e;
                        return Task::none();
                    }
                };
                let Some(preview) = self.preview.take() else {
                    return Task::none();
                };
                self.busy = true;
                self.notice = "正在安装…".into();
                let generation = self.generation;
                use iced::futures::SinkExt;
                return Task::run(
                    iced::stream::channel(
                        64,
                        move |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
                            let sender = std::sync::Arc::new(std::sync::Mutex::new(output.clone()));
                            let progress = std::sync::Arc::new(move |value| {
                                if let Ok(mut sender) = sender.lock() {
                                    let _ = sender.try_send(Message::Progress(generation, value));
                                }
                            });
                            let result = service::install(preview, client, progress).await;
                            let _ = output.send(Message::Finished(generation, result)).await;
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
    AppMessage::ResourceInstall(message)
}
pub fn view(state: &State, blocked: bool) -> Element<'_, AppMessage> {
    let enabled = !state.busy && !blocked;
    let mut body = column![
        bold(&state.selection, 18.),
        label("选择目标 → 检查预览 → 确认安装", 11.).color(MUTED),
        label("目标实例", 13.),
        pick_list(state.targets.clone(), state.target.clone(), |v| dispatch(
            Message::Target(v)
        ))
        .width(Fill)
        .text_size(13),
        label(format!("实例集：{}", state.root.display()), 10.).color(MUTED)
    ]
    .spacing(12);
    if state
        .request
        .as_ref()
        .is_some_and(|v| v.kind == ResourceKind::Modpack)
    {
        if let Some(request)=&state.request {body=body.push(button(label("新建整合包实例（自动安装游戏与加载器）",12.)).padding(10).style(theme::quiet).on_press_maybe(enabled.then_some(AppMessage::ImportRemotePack(request.project.clone(),request.version.clone()))));}
        body = body.push(
            label(
                "整合包会部署到所选实例；请使用独立实例，并匹配清单要求的 Minecraft 与加载器版本。",
                12.,
            )
            .color(MUTED),
        );
        body = body.push(
            checkbox(state.request.as_ref().unwrap().optional)
                .label("包含整合包可选客户端文件")
                .on_toggle_maybe(enabled.then_some(|v| dispatch(Message::Optional(v)))),
        );
    }
    if let Some(plan) = &state.preview {
        body = body
            .push(bold(format!("{} → {}", plan.title, plan.target), 16.))
            .push(label(
                format!(
                    "{} 个文件 · {:.1} MiB",
                    plan.files.len(),
                    plan.bytes as f64 / 1048576.
                ),
                12.,
            ));
        for warning in &plan.warnings {
            body = body.push(label(warning, 11.).color(MUTED));
        }
        for file in &plan.files {
            body = body.push(label(
                format!(
                    "{} · {} · {} KiB",
                    file.path,
                    file.source,
                    file.size.div_ceil(1024)
                ),
                11.,
            ));
        }
    }
    if let Some(progress) = &state.progress {
        body = body
            .push(progress_bar(
                0.0..=progress.total.max(1) as f32,
                progress.done as f32,
            ))
            .push(label(format!("{}/{}", progress.done, progress.total), 11.));
    }
    column![
        scrollable(body)
            .direction(theme::scroll_direction())
            .height(Fill),
        label(&state.notice, 12.).color(MUTED),
        row![
            button(label("生成安装预览", 12.))
                .padding(11)
                .style(theme::quiet)
                .on_press_maybe(
                    (enabled && state.target.is_some()).then_some(dispatch(Message::Prepare))
                ),
            Space::new().width(Fill),
            button(label("确认安装", 13.))
                .padding(12)
                .style(theme::action)
                .on_press_maybe(
                    (enabled && state.preview.is_some()).then_some(dispatch(Message::Confirm))
                )
        ]
        .spacing(8)
    ]
    .spacing(12)
    .height(440)
    .into()
}
