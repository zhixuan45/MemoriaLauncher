use crate::{
    app::Message as AppMessage,
    motion_renderer::Element,
    theme::{self, bold, label, MUTED},
};
use iced::{
    widget::{button, column, row, scrollable, text_input, Space},
    Fill, Task,
};
use mc_core::service::desktop_instances::{
    self as service, InstanceSettings, ModAction, ModEntry, Snapshot,
};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Message {
    Tab(bool),
    Name(String),
    Java(String),
    Memory(String),
    Width(String),
    Height(String),
    Jvm(String),
    JvmLine(usize, String),
    Query(String),
    BrowseJava,
    JavaPicked(u64, Option<PathBuf>),
    Import,
    Picked(u64, Vec<PathBuf>),
    Save,
    Refresh,
    Toggle(ModEntry),
    Remove(ModEntry),
    CancelRemove,
    ConfirmRemove,
    Completed(u64, Result<Snapshot, String>, String),
    CheckDependencies,
    ImportCandidates,
    CandidateFiles(u64, Vec<PathBuf>),
    DependenciesChecked(u64, Result<mc_core::service::mod_resolver::Report, String>),
}
#[derive(Default)]
pub struct State {
    pub busy: bool,
    pub mods_tab: bool,
    pub root: PathBuf,
    pub instance: String,
    pub generation: u64,
    pub snapshot: Option<Snapshot>,
    name: String,
    java: String,
    memory: String,
    width: String,
    height: String,
    jvm: String,
    query: String,
    pub notice: String,
    remove: Option<ModEntry>,
    pub dependency_report: Option<mc_core::service::mod_resolver::Report>,
}
impl State {
    pub fn open(&mut self, root: PathBuf, instance: String) -> Task<Message> {
        let generation = self.generation + 1;
        *self = Self {
            root,
            instance,
            generation,
            ..Default::default()
        };
        self.refresh("实例已读取")
    }
    fn refresh(&mut self, notice: &str) -> Task<Message> {
        self.busy = true;
        let root = self.root.clone();
        let key = self.instance.clone();
        let generation = self.generation;
        let notice = notice.to_string();
        Task::perform(
            async move {
                tokio::task::spawn_blocking(move || service::snapshot(&root, &key))
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r)
            },
            move |value| Message::Completed(generation, value, notice.clone()),
        )
    }
    pub fn update(&mut self, message: Message, blocked: bool) -> Task<Message> {
        match message {
            Message::Completed(generation, result, notice) if generation == self.generation => {
                self.busy = false;
                self.dependency_report = None;
                match result {
                    Ok(snapshot) => {
                        let c = &snapshot.config;
                        self.name = c.name.clone();
                        self.java = c
                            .java
                            .as_ref()
                            .map(|p| p.display().to_string())
                            .unwrap_or_default();
                        self.memory = c.memory_mb.map(|v| v.to_string()).unwrap_or_default();
                        self.width = c.resolution.map(|v| v.0.to_string()).unwrap_or_default();
                        self.height = c.resolution.map(|v| v.1.to_string()).unwrap_or_default();
                        self.jvm = c.jvm_args.join("\n");
                        self.snapshot = Some(snapshot);
                        self.notice = notice;
                    }
                    Err(e) => self.notice = format!("操作失败：{e}"),
                }
            }
            Message::Completed(..) => {}
            Message::DependenciesChecked(g, result) if g == self.generation => {
                self.busy = false;
                match result {
                    Ok(report) => {
                        self.dependency_report = Some(report);
                        self.notice = "前置与冲突求解完成；结果为只读建议。".into();
                    }
                    Err(error) => self.notice = format!("依赖检查失败：{error}"),
                }
            }
            Message::CandidateFiles(g, paths) if g == self.generation => {
                if paths.is_empty() {
                    self.busy = false;
                    self.notice = "已取消候选选择。".into();
                    return Task::none();
                }
                self.busy = true;
                let root = self.root.clone();
                let key = self.instance.clone();
                return Task::perform(
                    async move {
                        mc_core::service::mod_resolver::inventory::solve_candidate_files(
                            &root, &key, paths,
                        )
                        .await
                    },
                    move |result| Message::DependenciesChecked(g, result),
                );
            }
            Message::DependenciesChecked(..) | Message::CandidateFiles(..) => {}
            Message::Tab(value) => self.mods_tab = value,
            Message::Query(value) => self.query = value,
            Message::CancelRemove => self.remove = None,
            _ if self.busy || blocked => {}
            Message::CheckDependencies => {
                self.busy = true;
                self.dependency_report = None;
                self.notice = "正在读取本地元数据并求解前置与冲突…".into();
                let g = self.generation;
                let root = self.root.clone();
                let key = self.instance.clone();
                return Task::perform(
                    async move {
                        mc_core::service::mod_resolver::inventory::inspect_instance(&root, &key)
                            .await
                            .map(|inventory| inventory.audit())
                    },
                    move |result| Message::DependenciesChecked(g, result),
                );
            }
            Message::ImportCandidates => {
                self.busy = true;
                self.dependency_report = None;
                let g = self.generation;
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("选择候选 JAR（仅求解，不导入实例）")
                            .add_filter("候选 Mod JAR", &["jar"])
                            .pick_files()
                            .await
                            .unwrap_or_default()
                            .into_iter()
                            .map(|file| file.path().to_owned())
                            .collect()
                    },
                    move |paths| Message::CandidateFiles(g, paths),
                );
            }
            Message::Name(v) => self.name = v,
            Message::Java(v) => self.java = v,
            Message::Memory(v) => self.memory = v,
            Message::Width(v) => self.width = v,
            Message::Height(v) => self.height = v,
            Message::Jvm(v) => self.jvm = v,
            Message::JvmLine(index, value) => {
                let mut lines: Vec<_> = self.jvm.split('\n').map(String::from).collect();
                if let Some(line) = lines.get_mut(index) {
                    *line = value;
                }
                self.jvm = lines.join("\n");
            }
            Message::Refresh => return self.refresh("列表已刷新"),
            Message::BrowseJava => {
                self.busy = true;
                let generation = self.generation;
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("选择 Java 可执行文件")
                            .pick_file()
                            .await
                            .map(|v| v.path().to_path_buf())
                    },
                    move |path| Message::JavaPicked(generation, path),
                );
            }
            Message::Import => {
                self.busy = true;
                let generation = self.generation;
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .add_filter("Mod JAR", &["jar"])
                            .pick_files()
                            .await
                            .unwrap_or_default()
                            .into_iter()
                            .map(|v| v.path().to_path_buf())
                            .collect()
                    },
                    move |paths| Message::Picked(generation, paths),
                );
            }
            Message::JavaPicked(..) | Message::Picked(..) => {}
            Message::Save => {
                let config = match self.config() {
                    Ok(c) => c,
                    Err(e) => {
                        self.notice = e;
                        return Task::none();
                    }
                };
                let Some(snapshot) = &self.snapshot else {
                    return Task::none();
                };
                let expected = snapshot.config_hash.clone();
                let root = self.root.clone();
                let key = self.instance.clone();
                return self.operation(
                    move || {
                        service::save_settings(&root, &key, &config, &expected)?;
                        service::snapshot(&root, &key)
                    },
                    "实例设置已保存，下次启动生效",
                );
            }
            Message::Remove(entry) => self.remove = Some(entry),
            Message::ConfirmRemove => {
                if let Some(entry) = self.remove.take() {
                    return self.change(ModAction::Remove(entry));
                }
            }
            Message::Toggle(entry) => return self.change(ModAction::Toggle(entry)),
        }
        Task::none()
    }
    // Dialog completions must be accepted while the picker itself holds busy=true.
    pub fn picker(&mut self, message: &Message) -> Option<Task<Message>> {
        match message {
            Message::JavaPicked(generation, path) if *generation == self.generation => {
                self.busy = false;
                if let Some(path) = path {
                    self.java = path.display().to_string();
                }
                Some(Task::none())
            }
            Message::Picked(generation, paths) if *generation == self.generation => {
                self.busy = false;
                Some(if paths.is_empty() {
                    Task::none()
                } else {
                    self.change(ModAction::Import(paths.clone()))
                })
            }
            _ => None,
        }
    }
    fn operation(
        &mut self,
        f: impl FnOnce() -> Result<Snapshot, String> + Send + 'static,
        notice: &'static str,
    ) -> Task<Message> {
        self.busy = true;
        self.notice = "正在处理…".into();
        let generation = self.generation;
        Task::perform(
            async move {
                tokio::task::spawn_blocking(f)
                    .await
                    .map_err(|e| e.to_string())
                    .and_then(|r| r)
            },
            move |result| Message::Completed(generation, result, notice.into()),
        )
    }
    fn change(&mut self, action: ModAction) -> Task<Message> {
        let root = self.root.clone();
        let key = self.instance.clone();
        self.operation(
            move || service::change_mod(&root, &key, action),
            "Mod 列表已更新；移除的文件保留在 .memoria/removed-mods",
        )
    }
    fn config(&self) -> Result<InstanceSettings, String> {
        let memory_mb = if self.memory.trim().is_empty() {
            None
        } else {
            Some(self.memory.trim().parse().map_err(|_| "内存应为整数 MiB")?)
        };
        let resolution = if self.width.trim().is_empty() && self.height.trim().is_empty() {
            None
        } else {
            Some((
                self.width.trim().parse().map_err(|_| "请填写窗口宽度")?,
                self.height.trim().parse().map_err(|_| "请填写窗口高度")?,
            ))
        };
        let config = InstanceSettings {
            name: self.name.trim().into(),
            java: (!self.java.trim().is_empty()).then(|| PathBuf::from(self.java.trim())),
            memory_mb,
            resolution,
            jvm_args: self
                .jvm
                .lines()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(String::from)
                .collect(),
        };
        config.validate()?;
        Ok(config)
    }
}
fn dispatch(message: Message) -> AppMessage {
    AppMessage::InstanceSettings(message)
}
pub fn view(state: &State, blocked: bool) -> Element<'_, AppMessage> {
    let enabled = !state.busy && !blocked && state.snapshot.is_some();
    let tabs = row![
        button(label("实例设置", 13.))
            .style(if state.mods_tab {
                theme::quiet
            } else {
                theme::selected
            })
            .padding(10)
            .on_press(dispatch(Message::Tab(false))),
        button(label("Mod 管理", 13.))
            .style(if state.mods_tab {
                theme::selected
            } else {
                theme::quiet
            })
            .padding(10)
            .on_press(dispatch(Message::Tab(true))),
        button(label("变更计划与恢复",12.)).style(theme::quiet).padding(10)
            .on_press_maybe(enabled.then_some(AppMessage::OpenPlans)),
        button(label("锁定与导出",12.)).style(theme::quiet).padding(10)
            .on_press_maybe(enabled.then_some(AppMessage::OpenPackExport))
    ]
    .spacing(8);
    let mut body = column![].spacing(12);
    if state.mods_tab {
        let mods = state
            .snapshot
            .as_ref()
            .map(|v| v.mods.as_slice())
            .unwrap_or_default();
        body = body.push(
            row![
                bold(
                    format!(
                        "{} 个 Mod · {} 个启用",
                        mods.len(),
                        mods.iter().filter(|v| v.enabled).count()
                    ),
                    16.
                ),
                Space::new().width(Fill),
                button(label("导入 JAR", 12.))
                    .on_press_maybe(enabled.then_some(dispatch(Message::Import)))
                    .padding(10)
                    .style(theme::action),
                button(label("刷新", 12.))
                    .on_press_maybe(enabled.then_some(dispatch(Message::Refresh)))
                    .padding(10)
                    .style(theme::quiet)
            ]
            .spacing(8),
        );
        body = body.push(
            row![
                button(label("检查前置与冲突", 12.))
                    .padding(10)
                    .style(theme::quiet)
                    .on_press_maybe(enabled.then_some(dispatch(Message::CheckDependencies))),
                button(label("选择候选 JAR 求解", 12.))
                    .padding(10)
                    .style(theme::quiet)
                    .on_press_maybe(enabled.then_some(dispatch(Message::ImportCandidates)))
            ]
            .spacing(8),
        );
        if let Some(report) = &state.dependency_report {
            body = body.push(crate::dependency_view::view(report));
        }
        body = body.push(
            text_input("按名称或文件名搜索 Mod", &state.query)
                .padding(10)
                .on_input(|v| dispatch(Message::Query(v)))
                .style(theme::input),
        );
        if let Some(entry) = &state.remove {
            body = body
                .push(label(
                    format!(
                        "移除 {}？文件会保留在 .memoria/removed-mods。",
                        entry.filename
                    ),
                    12.,
                ))
                .push(
                    row![
                        button(label("确认移除", 12.))
                            .padding(10)
                            .on_press_maybe(enabled.then_some(dispatch(Message::ConfirmRemove)))
                            .style(theme::action),
                        button(label("取消", 12.))
                            .padding(10)
                            .on_press(dispatch(Message::CancelRemove))
                            .style(theme::quiet)
                    ]
                    .spacing(8),
                );
        }
        if mods.is_empty() {
            body = body
                .push(label("此实例还没有 Mod，可导入 JAR 或从资源目录安装。", 13.).color(MUTED));
        }
        let query = state.query.to_lowercase();
        for entry in mods.iter().filter(|v| {
            format!("{} {}", v.name, v.filename)
                .to_lowercase()
                .contains(&query)
        }) {
            body = body.push(
                row![
                    column![
                        bold(entry.name.clone(), 13.),
                        label(
                            format!("{} · {} KiB", entry.filename, entry.size.div_ceil(1024)),
                            10.
                        )
                        .color(MUTED)
                    ]
                    .width(Fill)
                    .spacing(4),
                    button(label(if entry.enabled { "禁用" } else { "启用" }, 12.))
                        .padding(8)
                        .on_press_maybe(enabled.then(|| dispatch(Message::Toggle(entry.clone()))))
                        .style(theme::quiet),
                    button(label("移除", 12.))
                        .padding(8)
                        .on_press_maybe(enabled.then(|| dispatch(Message::Remove(entry.clone()))))
                        .style(theme::quiet)
                ]
                .spacing(8),
            );
        }
        body = body.push(
            label(
                "启停通过 .jar / .jar.disabled 切换；正在运行游戏时禁止修改。",
                11.,
            )
            .color(MUTED),
        );
    } else {
        for (title, value, placeholder, kind) in [
            ("显示名称", &state.name, "留空使用版本名称", 0),
            ("Java 路径", &state.java, "留空按实例版本自动选择 Java", 1),
            ("最大内存 MiB", &state.memory, "留空继承全局内存策略", 2),
            ("窗口宽度", &state.width, "留空使用游戏默认", 3),
            ("窗口高度", &state.height, "留空使用游戏默认", 4),
        ] {
            body = body.push(label(title, 12.)).push(
                text_input(placeholder, value)
                    .id(format!("instance-field-{kind}"))
                    .padding(10)
                    .style(theme::input)
                    .on_input_maybe(enabled.then_some(move |v| {
                        dispatch(match kind {
                            0 => Message::Name(v),
                            1 => Message::Java(v),
                            2 => Message::Memory(v),
                            3 => Message::Width(v),
                            _ => Message::Height(v),
                        })
                    })),
            );
            if kind == 1 {
                body = body.push(
                    button(label("选择 Java…", 12.))
                        .on_press_maybe(enabled.then_some(dispatch(Message::BrowseJava)))
                        .padding(8)
                        .style(theme::quiet),
                );
            }
        }
        // Each row is one argv entry; never pass JVM parameters through a shell.
        body = body.push(label("JVM 参数（每行一项，含空格的参数保持在同一行）", 12.));
        for (index, line) in state.jvm.split('\n').enumerate() {
            body = body.push(
                text_input("例如 -XX:+UseG1GC", line)
                    .padding(10)
                    .style(theme::input)
                    .on_input_maybe(
                        enabled.then_some(move |v| dispatch(Message::JvmLine(index, v))),
                    ),
            );
        }
        body = body.push(
            button(label("添加 JVM 参数", 12.))
                .padding(8)
                .style(theme::quiet)
                .on_press_maybe(
                    enabled.then(|| dispatch(Message::Jvm(format!("{}\n", state.jvm)))),
                ),
        );
    }
    let save: Element<'_, AppMessage> = if state.mods_tab {
        Space::new().into()
    } else {
        button(label("保存实例设置", 13.))
            .padding(12)
            .style(theme::action)
            .on_press_maybe(enabled.then_some(dispatch(Message::Save)))
            .into()
    };
    column![
        tabs,
        label(
            format!("{} / {}", state.root.display(), state.instance),
            10.
        )
        .color(MUTED),
        scrollable(body)
            .direction(theme::scroll_direction())
            .height(Fill),
        label(&state.notice, 12.).color(MUTED),
        save
    ]
    .spacing(12)
    .height(440)
    .into()
}
