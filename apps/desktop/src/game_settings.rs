use crate::app::{Launcher, Message as AppMessage};
use crate::assets::{icon, Icon};
use crate::java_environment::{self, JavaChoice, JavaDiscovery};
use crate::local::{self, Environment};
use crate::motion_renderer::Element;
use crate::runtime_memory::{gib, MemoryMode, MemorySnapshot};
use crate::theme::{self, bold, label, GREEN, MUTED};
use iced::widget::{
    button, column, container, pick_list, radio, row, scrollable, slider, text_input, tooltip,
    Space,
};
use iced::{Alignment, Color, Fill, Length, Task};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub enum Message {
    Folder(String),
    Add(bool),
    BrowseFolder,
    FolderPicked(Option<PathBuf>),
    Prepared(Result<(PathBuf, bool), String>),
    Select(usize),
    Remove(usize),
    Java(String),
    BrowseJava,
    JavaPicked(Option<PathBuf>),
    CheckJava,
    JavaChecked(Result<mc_core::JavaRuntime, String>),
    JavaSelected(JavaChoice),
    ScanJava,
    JavaScanned(Result<JavaDiscovery, String>),
    MemoryMode(MemoryMode),
    MemoryPercent(u8),
    RefreshMemory,
    MemoryRead(Result<MemorySnapshot, String>),
    Ignore,
    Save,
    Saved(Result<Environment, String>),
}

pub struct State {
    pub draft: Environment,
    pub folder: String,
    pub java: String,
    pub min_memory: String,
    pub max_memory: String,
    pub busy: bool,
    pub notice: String,
    pub java_notice: String,
    pub java_choices: Vec<JavaChoice>,
    pub java_scanning: bool,
    java_loaded: bool,
    pub memory: MemorySnapshot,
    memory_refreshing: bool,
}

impl State {
    pub fn new(environment: &Environment) -> Self {
        let mut draft = environment.clone();
        draft.reconcile_roots();
        let memory = MemorySnapshot::read();
        let mut java_choices = vec![JavaChoice::configured(draft.java.clone())];
        for path in &draft.java_paths {
            if !java_choices.iter().any(|choice| {
                choice.path().is_some_and(|other| {
                    java_environment::key(other) == java_environment::key(path)
                })
            }) {
                java_choices.push(JavaChoice::configured(path.clone()));
            }
        }
        java_choices.push(JavaChoice::Import);
        Self {
            folder: suggested_folder(&draft).to_string_lossy().into_owned(),
            java: draft.java.to_string_lossy().into_owned(),
            min_memory: draft.min_memory_mb.to_string(),
            max_memory: memory
                .allocation(draft.memory_mode, draft.memory_percent, draft.max_memory_mb)
                .unwrap_or(draft.max_memory_mb)
                .to_string(),
            java_choices,
            java_scanning: false,
            java_loaded: false,
            memory,
            memory_refreshing: false,
            draft,
            busy: false,
            notice: "修改后点击保存并应用；当前实例集中的版本将显示在主页。".into(),
            java_notice: "可填写 Java 可执行文件的完整路径，或使用 PATH 中的 java。".into(),
        }
    }

    fn configuration(&self) -> Result<Environment, String> {
        let mut config = self.draft.clone();
        if self.java.trim().is_empty() {
            return Err("Java 路径不能为空。".into());
        }
        config.java = self.java.trim().into();
        config.min_memory_mb = self
            .min_memory
            .trim()
            .parse()
            .map_err(|_| "初始内存请输入整数，单位为 MiB。")?;
        if config.memory_mode == MemoryMode::Fixed {
            config.max_memory_mb = self
                .max_memory
                .trim()
                .parse()
                .map_err(|_| "最大内存请输入整数，单位为 MiB。")?;
        } else {
            if !(1..=90).contains(&config.memory_percent) {
                return Err("自定义内存比例应在 1%–90% 之间。".into());
            }
            config.max_memory_mb = self
                .memory
                .allocation(
                    config.memory_mode,
                    config.memory_percent,
                    config.max_memory_mb,
                )
                .unwrap_or(config.max_memory_mb);
            config.min_memory_mb = config.min_memory_mb.min(config.max_memory_mb);
        }
        if config.min_memory_mb < 128
            || config.min_memory_mb > config.max_memory_mb
            || config.max_memory_mb > 1024 * 1024
        {
            return Err("内存应满足 128 MiB ≤ 初始内存 ≤ 最大内存 ≤ 1048576 MiB。".into());
        }
        Ok(config)
    }

    pub fn ensure_java_loaded(&mut self) -> Task<Message> {
        if self.java_loaded || self.java_scanning {
            return Task::none();
        }
        self.update(Message::ScanJava, false)
    }

    fn add_java(&mut self, runtime: mc_core::JavaRuntime) {
        let choice = JavaChoice::from_runtime(runtime);
        let key = java_environment::key(choice.path().unwrap());
        self.java_choices.retain(|item| {
            item.path()
                .is_some_and(|path| java_environment::key(path) != key)
        });
        self.java_choices.push(choice);
        self.java_choices.sort_by(|left, right| {
            let major = |item: &JavaChoice| match item {
                JavaChoice::Runtime { major, .. } => major.unwrap_or(0),
                _ => 0,
            };
            major(right).cmp(&major(left))
        });
        self.java_choices.push(JavaChoice::Import);
    }

    pub fn selected_java(&self) -> Option<JavaChoice> {
        let key = java_environment::key(PathBuf::from(self.java.trim()).as_path());
        self.java_choices
            .iter()
            .find(|choice| {
                choice
                    .path()
                    .is_some_and(|path| java_environment::key(path) == key)
            })
            .cloned()
    }

    pub fn allocation(&self) -> Option<u32> {
        self.memory.allocation(self.draft.memory_mode, self.draft.memory_percent, self.max_memory.parse().unwrap_or(self.draft.max_memory_mb))
            .map(|value| {
                if self.draft.memory_mode == MemoryMode::Automatic && matches!(self.selected_java(), Some(JavaChoice::Runtime { arch, .. }) if matches!(arch.as_str(), "x86" | "i386" | "i686")) {
                    value.min(1536)
                } else { value }
            })
    }

    pub fn update(&mut self, message: Message, blocked: bool) -> Task<Message> {
        // Completion messages must settle even while the corresponding task is busy.
        match message {
            Message::Ignore => {}
            Message::RefreshMemory => {
                if !self.memory_refreshing {
                    self.memory_refreshing = true;
                    return Task::perform(
                        async {
                            tokio::task::spawn_blocking(MemorySnapshot::read)
                                .await
                                .map_err(|e| e.to_string())
                        },
                        Message::MemoryRead,
                    );
                }
            }
            Message::MemoryRead(result) => {
                self.memory_refreshing = false;
                if let Ok(memory) = result {
                    self.memory = memory;
                    if self.draft.memory_mode != MemoryMode::Fixed {
                        if let Some(value) = self.allocation() {
                            self.max_memory = value.to_string();
                        }
                    }
                }
            }
            Message::JavaScanned(result) => {
                self.java_scanning = false;
                self.java_loaded = true;
                match result {
                    Ok(report) => {
                        let count = report.runtimes.len();
                        let selected = java_environment::key(PathBuf::from(&self.java).as_path());
                        self.java_choices.retain(|choice| {
                            choice.path().is_some_and(|path| {
                                let key = java_environment::key(path);
                                key == selected
                                    || self
                                        .draft
                                        .java_paths
                                        .iter()
                                        .any(|imported| java_environment::key(imported) == key)
                            })
                        });
                        for runtime in report.runtimes {
                            self.add_java(runtime);
                        }
                        self.java_notice = format!(
                            "已检测到 {count} 个 Java，可从下拉列表选择。{}",
                            report
                                .failures
                                .first()
                                .map(|error| format!("部分路径检测失败：{error}"))
                                .unwrap_or_default()
                        );
                    }
                    Err(error) => {
                        self.java_notice = format!("Java 扫描失败：{error}；可手动导入。")
                    }
                }
            }
            Message::FolderPicked(path) => {
                self.busy = false;
                if let Some(path) = path {
                    self.folder = path.to_string_lossy().into_owned();
                    return self.update(Message::Add(false), blocked);
                }
            }
            Message::JavaPicked(path) => {
                self.busy = false;
                if let Some(path) = path {
                    self.busy = true;
                    self.java_notice = "正在检测导入的 Java…".into();
                    return Task::perform(java_environment::inspect(path), Message::JavaChecked);
                }
            }
            Message::Prepared(result) => {
                self.busy = false;
                match result {
                    Ok((path, created)) => {
                        if self
                            .draft
                            .game_roots
                            .iter()
                            .any(|root| local::same_root(root, &path))
                        {
                            self.notice = if created {
                                "实例集文件夹已创建；点击保存并应用以重新扫描。"
                            } else {
                                "该实例集文件夹已在列表中。"
                            }
                            .into();
                            if created {
                                self.folder =
                                    suggested_folder(&self.draft).to_string_lossy().into_owned();
                            }
                        } else {
                            self.draft.game_roots.push(path);
                            self.folder =
                                suggested_folder(&self.draft).to_string_lossy().into_owned();
                            self.notice = if created {
                                "实例集文件夹已创建；点击保存并应用以保存列表。"
                            } else {
                                "已导入已有文件夹；点击保存并应用以保存列表。"
                            }
                            .into();
                        }
                    }
                    Err(error) => self.notice = error,
                }
            }
            Message::JavaChecked(result) => {
                self.busy = false;
                match result {
                    Ok(runtime) => {
                        self.java = runtime.executable.to_string_lossy().into_owned();
                        if !self.draft.java_paths.iter().any(|path| {
                            java_environment::key(path)
                                == java_environment::key(&runtime.executable)
                        }) {
                            self.draft.java_paths.push(runtime.executable.clone());
                        }
                        self.java_notice = format!(
                            "检测通过 · Java {} ({}) · {}",
                            runtime.major_version, runtime.version, runtime.arch
                        );
                        self.add_java(runtime);
                        self.notice = "Java 已选择，点击保存并应用后生效。".into();
                    }
                    Err(error) => self.java_notice = format!("Java 检测失败：{error}"),
                }
            }
            Message::Saved(result) => {
                self.busy = false;
                match result {
                    Ok(config) => {
                        self.draft = config;
                        self.notice = "游戏设置已保存并应用。".into();
                    }
                    Err(error) => self.notice = format!("保存失败：{error}"),
                }
            }
            _ if self.busy || blocked => return Task::none(),
            Message::Folder(value) => self.folder = value,
            Message::Java(value) => {
                self.java = value;
                self.java_notice = "Java 路径已更改，可点击检测 Java。".into();
                self.notice = "有未保存的更改，点击保存并应用后生效。".into();
            }
            Message::MemoryMode(mode) => {
                if mode == MemoryMode::Custom
                    && self.draft.memory_mode == MemoryMode::Fixed
                    && self.memory.total_mb > 0
                {
                    self.draft.memory_percent = ((self.draft.max_memory_mb as u64 * 100
                        / self.memory.total_mb)
                        .clamp(1, 90)) as u8;
                }
                self.draft.memory_mode = mode;
                if let Some(value) = self.allocation() {
                    self.max_memory = value.to_string();
                }
                self.notice = "有未保存的更改，点击保存并应用后生效。".into();
            }
            Message::MemoryPercent(percent) => {
                self.draft.memory_mode = MemoryMode::Custom;
                self.draft.memory_percent = percent.clamp(1, 90);
                if let Some(value) = self.allocation() {
                    self.max_memory = value.to_string();
                }
                self.notice = "有未保存的更改，点击保存并应用后生效。".into();
            }
            Message::JavaSelected(JavaChoice::Import) => {
                return self.update(Message::BrowseJava, blocked)
            }
            Message::JavaSelected(choice) => {
                if let Some(path) = choice.path() {
                    self.java = path.to_string_lossy().into_owned();
                    self.notice = "Java 已选择，点击保存并应用后生效。".into();
                }
            }
            Message::ScanJava => {
                if self.java_scanning {
                    return Task::none();
                }
                self.java_scanning = true;
                let mut explicit = self.draft.java_paths.clone();
                explicit.insert(0, self.java.trim().into());
                self.java_notice = "正在扫描已安装的 Java…".into();
                return Task::perform(
                    java_environment::discover(self.draft.game_roots.clone(), explicit),
                    Message::JavaScanned,
                );
            }
            Message::Select(index) => {
                if let Some(root) = self.draft.game_roots.get(index) {
                    self.draft.game_root = root.clone();
                    self.notice = "已选择当前实例集，保存并应用后会重新扫描。".into();
                }
            }
            Message::Remove(index) => {
                if self.draft.game_roots.len() > 1 && index < self.draft.game_roots.len() {
                    let removed = self.draft.game_roots.remove(index);
                    if local::same_root(&removed, &self.draft.game_root) {
                        self.draft.game_root = self.draft.game_roots[0].clone();
                    }
                    self.notice = "已从列表移除，文件夹内容保留；保存并应用后生效。".into();
                }
            }
            Message::Add(create) => {
                let path = PathBuf::from(self.folder.trim());
                if path.as_os_str().is_empty() {
                    self.notice = "请先填写实例集文件夹路径。".into();
                    return Task::none();
                }
                if (!create || path.exists())
                    && self
                        .draft
                        .game_roots
                        .iter()
                        .any(|root| local::same_root(root, &path))
                {
                    self.notice = "该实例集文件夹已在列表中。".into();
                    return Task::none();
                }
                self.busy = true;
                self.notice = "正在处理实例集文件夹…".into();
                return Task::perform(local::prepare_collection(path, create), Message::Prepared);
            }
            Message::BrowseFolder => {
                self.busy = true;
                let directory = self
                    .draft
                    .game_root
                    .parent()
                    .unwrap_or(&self.draft.game_root)
                    .to_path_buf();
                return Task::perform(
                    async move {
                        rfd::AsyncFileDialog::new()
                            .set_title("导入实例集文件夹（例如 .minecraft）")
                            .set_directory(directory)
                            .pick_folder()
                            .await
                            .map(|file| file.path().to_path_buf())
                    },
                    Message::FolderPicked,
                );
            }
            Message::BrowseJava => {
                self.busy = true;
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("导入 Java 可执行文件（java.exe）")
                            .pick_file()
                            .await
                            .map(|file| file.path().to_path_buf())
                    },
                    Message::JavaPicked,
                );
            }
            Message::CheckJava => {
                if self.java.trim().is_empty() {
                    self.java_notice = "请先填写 Java 路径。".into();
                    return Task::none();
                }
                self.busy = true;
                self.java_notice = "正在检测 Java…".into();
                let java = PathBuf::from(self.java.trim());
                return Task::perform(
                    async move { java_environment::inspect(java).await },
                    Message::JavaChecked,
                );
            }
            Message::Save => {
                let config = match self.configuration() {
                    Ok(config) => config,
                    Err(error) => {
                        self.notice = error;
                        return Task::none();
                    }
                };
                self.busy = true;
                self.notice = "正在保存游戏设置…".into();
                return Task::perform(
                    async move {
                        let mut config = config;
                        config.game_root = local::normalize_root(&config.game_root)?;
                        config.game_roots = config
                            .game_roots
                            .iter()
                            .map(|root| local::normalize_root(root))
                            .collect::<Result<_, _>>()?;
                        config.reconcile_roots();
                        config.save().await?;
                        Ok(config)
                    },
                    Message::Saved,
                );
            }
        }
        Task::none()
    }
}

fn suggested_folder(config: &Environment) -> PathBuf {
    if config.game_roots.len() == 1 && !config.game_root.exists() {
        return config.game_root.clone();
    }
    let parent = config.game_root.parent().unwrap_or(&config.game_root);
    for index in 1.. {
        let path = parent.join(if index == 1 {
            ".minecraft".into()
        } else {
            format!(".minecraft-{index}")
        });
        if !path.exists()
            && !config
                .game_roots
                .iter()
                .any(|root| local::same_root(root, &path))
        {
            return path;
        }
    }
    unreachable!()
}

fn dispatch(message: Message) -> AppMessage {
    AppMessage::GameSettings(message)
}

pub fn view(app: &Launcher) -> Element<'_, AppMessage> {
    let state = &app.game_settings;
    let enabled =
        !state.busy && !app.loading && !app.launching && app.download.installing.is_none();
    let mut folders = column![].spacing(8);
    for (index, root) in state.draft.game_roots.iter().enumerate() {
        let selected = local::same_root(root, &state.draft.game_root);
        folders = folders.push(
            container(
                row![
                    icon(Icon::Folder, GREEN, 22.),
                    column![
                        bold(
                            root.file_name()
                                .unwrap_or(root.as_os_str())
                                .to_string_lossy()
                                .into_owned(),
                            14.
                        ),
                        label(root.to_string_lossy().into_owned(), 11.).color(MUTED),
                    ]
                    .spacing(5)
                    .width(Fill),
                    button(label(
                        if selected {
                            "当前实例集"
                        } else {
                            "设为当前"
                        },
                        12.
                    ))
                    .padding(9)
                    .style(if selected {
                        theme::selected
                    } else {
                        theme::quiet
                    })
                    .on_press_maybe(
                        (enabled && !selected).then_some(dispatch(Message::Select(index)))
                    ),
                    button(label("移除", 12.))
                        .padding(9)
                        .style(theme::quiet)
                        .on_press_maybe(
                            (enabled && state.draft.game_roots.len() > 1)
                                .then_some(dispatch(Message::Remove(index)))
                        ),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .padding(12)
            .style(theme::surface),
        );
    }
    let collections = card(
        column![
            row![
                icon(Icon::Folder, GREEN, 21.),
                bold("实例集文件夹", 17.),
                Space::new().width(Fill),
                label(format!("{} 个文件夹", state.draft.game_roots.len()), 12.).color(MUTED)
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            label(
                "每个文件夹（如 .minecraft）包含独立的 versions、libraries 和 assets。",
                12.
            )
            .color(MUTED),
            folders,
            text_input("新建或导入的文件夹完整路径", &state.folder)
                .padding(11)
                .size(13.)
                .on_input_maybe(enabled.then_some(|value| dispatch(Message::Folder(value)))),
            row![
                button(label("新建 / 导入", 12.))
                    .padding(10)
                    .style(theme::action)
                    .on_press_maybe(enabled.then_some(dispatch(Message::Add(true)))),
                button(label("导入已有文件夹", 12.))
                    .padding(10)
                    .style(theme::quiet)
                    .on_press_maybe(enabled.then_some(dispatch(Message::Add(false)))),
                button(label("浏览导入…", 12.))
                    .padding(10)
                    .style(theme::quiet)
                    .on_press_maybe(enabled.then_some(dispatch(Message::BrowseFolder))),
            ]
            .spacing(8),
            label(
                "默认在当前实例集旁新建；路径已存在时直接导入。移除只影响列表，保留文件。",
                11.
            )
            .color(MUTED),
        ]
        .spacing(12)
        .into(),
    );
    let memory = memory_card(state, enabled);
    let count = state
        .java_choices
        .iter()
        .filter(|choice| matches!(choice, JavaChoice::Runtime { major: Some(_), .. }))
        .count();
    let java = card(
        column![
            row![
                bold("Java 环境", 17.),
                Space::new().width(Fill),
                label(format!("共 {count} 个已检测 Java"), 12.).color(MUTED)
            ]
            .align_y(Alignment::Center),
            label("游戏版本所需的 Java 版本可在主页实例信息中查看。", 12.).color(MUTED),
            tooltip(
                container(
                    pick_list(
                        state.java_choices.as_slice(),
                        state.selected_java(),
                        move |choice| dispatch(if enabled {
                            Message::JavaSelected(choice)
                        } else {
                            Message::Ignore
                        })
                    )
                    .placeholder("下拉选择 Java 或导入已有 Java…")
                    .width(Fill)
                    .padding(12)
                    .text_size(13)
                )
                .id("game-java-picker")
                .width(Fill),
                container(label(&state.java, 11.))
                    .padding(8)
                    .style(theme::surface),
                tooltip::Position::Top,
            ),
            row![
                text_input("java 或 Java 可执行文件路径", &state.java)
                    .padding(10)
                    .size(12.)
                    .on_input_maybe(enabled.then_some(|value| dispatch(Message::Java(value)))),
                button(label("检测 Java", 12.))
                    .padding(10)
                    .style(theme::quiet)
                    .on_press_maybe(enabled.then_some(dispatch(Message::CheckJava))),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
            row![
                button(label("导入已有 Java…", 12.))
                    .padding(10)
                    .style(theme::quiet)
                    .on_press_maybe(enabled.then_some(dispatch(Message::BrowseJava))),
                button(label(
                    if state.java_scanning {
                        "正在扫描…"
                    } else {
                        "刷新 Java 列表"
                    },
                    12.
                ))
                .padding(10)
                .style(theme::quiet)
                .on_press_maybe(
                    (enabled && !state.java_scanning).then_some(dispatch(Message::ScanJava))
                ),
            ]
            .spacing(8),
            label(&state.java_notice, 11.).color(MUTED),
        ]
        .spacing(12)
        .into(),
    );
    column![
        scrollable(column![collections, java, memory].spacing(12))
            .direction(theme::scroll_direction())
            .height(Fill),
        row![
            label(&state.notice, 12.).color(MUTED).width(Fill),
            button(label(
                if state.busy {
                    "正在处理…"
                } else {
                    "保存并应用"
                },
                13.
            ))
            .padding([12, 20])
            .style(theme::action)
            .on_press_maybe(enabled.then_some(dispatch(Message::Save))),
        ]
        .spacing(16)
        .align_y(Alignment::Center),
    ]
    .spacing(12)
    .height(Fill)
    .into()
}

fn memory_card(state: &State, enabled: bool) -> Element<'_, AppMessage> {
    let automatic = state.draft.memory_mode == MemoryMode::Automatic;
    let allocation = state.allocation();
    let percent = if automatic && state.memory.total_mb > 0 {
        ((allocation.unwrap_or(0) as u64 * 100 / state.memory.total_mb).clamp(1, 90)) as u8
    } else if state.draft.memory_mode == MemoryMode::Fixed && state.memory.total_mb > 0 {
        (state.draft.max_memory_mb as u64 * 100 / state.memory.total_mb).clamp(1, 90) as u8
    } else {
        state.draft.memory_percent.clamp(1, 90)
    };
    let mut track = row![].height(6);
    let total = state.memory.total_mb.max(1);
    let used = state.memory.used_mb();
    let shown_game = (allocation.unwrap_or(0) as u64).min(state.memory.available_mb);
    for (amount, color) in [
        (used, Color::from_rgb(0.16, 0.53, 0.78)),
        (shown_game, GREEN),
        (
            total.saturating_sub(used + shown_game),
            Color::from_rgb(0.12, 0.23, 0.28),
        ),
    ] {
        if amount > 0 {
            track = track.push(
                container(Space::new().height(6))
                    .width(Length::FillPortion((amount * 1000 / total).max(1) as u16))
                    .style(move |_| container::Style {
                        background: Some(color.into()),
                        ..Default::default()
                    }),
            );
        }
    }
    let explanation = if state.memory.total_mb == 0 {
        "系统内存暂不可用，请刷新后重试。".to_string()
    } else if allocation.is_some_and(|amount| amount as u64 > state.memory.available_mb) {
        "当前可用内存不足，启动前请降低比例或关闭其他程序。".into()
    } else if automatic {
        "根据总内存与当前可用内存自动计算，每次启动前更新。".into()
    } else if state.draft.memory_mode == MemoryMode::Fixed {
        "已保留原有固定内存值；拖动滑块后改为按比例分配。".into()
    } else {
        format!("按系统总内存的 {percent}% 分配，每次启动按实际总内存计算。")
    };
    card(
        column![
            bold("内存分配", 17.),
            container(
                radio("自动配置", true, Some(automatic), move |_| dispatch(
                    if enabled {
                        Message::MemoryMode(MemoryMode::Automatic)
                    } else {
                        Message::Ignore
                    }
                ))
                .size(19)
                .text_size(14)
            )
            .id("memory-auto"),
            row![
                container(
                    radio("自定义", false, Some(automatic), move |_| dispatch(
                        if enabled {
                            Message::MemoryMode(MemoryMode::Custom)
                        } else {
                            Message::Ignore
                        }
                    ))
                    .size(19)
                    .text_size(14)
                    .width(110)
                )
                .id("memory-custom"),
                container(
                    slider(1..=90, percent, move |value| dispatch(
                        if enabled && !automatic {
                            Message::MemoryPercent(value)
                        } else {
                            Message::Ignore
                        }
                    ))
                    .step(1)
                    .height(24)
                    .style(move |theme, status| {
                        let mut style = slider::default(theme, status).with_circular_handle(6);
                        if automatic || !enabled {
                            style.rail.backgrounds =
                                (MUTED.into(), Color::from_rgb(0.12, 0.23, 0.28).into());
                            style.handle.background = MUTED.into();
                        }
                        style
                    })
                )
                .id("memory-ratio-slider")
                .width(Fill),
                label(
                    if automatic {
                        "自动".into()
                    } else {
                        format!("{percent}%")
                    },
                    13.
                )
                .color(MUTED)
                .width(42),
            ]
            .spacing(14)
            .align_y(Alignment::Center),
            label(explanation, 11.).color(MUTED),
            row![
                label("已用内存", 11.).color(MUTED).width(Fill),
                label("游戏分配", 11.).color(MUTED).width(Fill)
            ],
            track,
            row![
                bold(
                    if state.memory.total_mb > 0 {
                        format!("{} / {}", gib(used), gib(total))
                    } else {
                        "读取中…".into()
                    },
                    17.
                )
                .width(Fill),
                bold(
                    allocation
                        .map(|amount| gib(amount as u64))
                        .unwrap_or_else(|| "待计算".into()),
                    17.
                )
                .color(GREEN)
                .width(Fill),
            ],
        ]
        .spacing(14)
        .into(),
    )
}

fn card(content: Element<'_, AppMessage>) -> Element<'_, AppMessage> {
    container(content)
        .padding(18)
        .width(Fill)
        .style(theme::panel)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn first_collection_can_be_created_without_registering_it_twice() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/game-settings-tests")
            .join(uuid::Uuid::new_v4().to_string())
            .join(".minecraft");
        let config = Environment {
            game_root: root.clone(),
            game_roots: vec![root.clone()],
            ..Environment::default()
        };
        let mut state = State::new(&config);
        assert!(local::same_root(&PathBuf::from(&state.folder), &root));
        assert_eq!(state.update(Message::Add(true), false).units(), 1);
        assert!(state.busy);
        let prepared = local::prepare_collection(root.clone(), true).await;
        let _ = state.update(Message::Prepared(prepared), false);
        assert!(!state.busy);
        assert!(root.join("versions").is_dir());
        assert_eq!(state.draft.game_roots.len(), 1);
        assert!(state.notice.contains("已创建"));
        assert!(state.folder.ends_with(".minecraft-2"));
    }

    #[test]
    fn rejects_invalid_memory_and_empty_java_before_saving() {
        let mut state = State::new(&Environment::default());
        state.draft.memory_mode = MemoryMode::Fixed;
        for (min, max) in [
            ("0", "4096"),
            ("4097", "4096"),
            ("512", "1048577"),
            ("oops", "4096"),
            ("512", "4.5"),
            ("512", "-1"),
        ] {
            state.min_memory = min.into();
            state.max_memory = max.into();
            assert_eq!(state.update(Message::Save, false).units(), 0);
            assert!(!state.busy);
            assert!(state.notice.contains("内存"));
        }
        state.min_memory = "1024".into();
        state.max_memory = "8192".into();
        state.java = " ".into();
        assert!(state.configuration().is_err());
        state.java = " java ".into();
        let config = state.configuration().unwrap();
        assert_eq!(config.launch_options().max_memory_mb, 8192);
        assert_eq!(config.java, PathBuf::from("java"));
    }

    #[test]
    fn removing_current_collection_selects_another_and_retains_the_last_collection() {
        let mut state = State::new(&Environment::default());
        let second = state.draft.game_root.with_file_name(".minecraft-second");
        let _ = state.update(Message::Prepared(Ok((second.clone(), false))), false);
        let _ = state.update(Message::Prepared(Ok((second.clone(), false))), false);
        assert_eq!(state.draft.game_roots.len(), 2);
        let _ = state.update(Message::Select(1), false);
        assert_eq!(state.draft.game_root, second);
        let _ = state.update(Message::Remove(1), false);
        assert_eq!(state.draft.game_roots.len(), 1);
        assert_eq!(state.draft.game_root, state.draft.game_roots[0]);
        let _ = state.update(Message::Remove(0), false);
        assert_eq!(state.draft.game_roots.len(), 1);
    }

    #[test]
    fn busy_or_launching_state_prevents_changes_and_save_failure_retains_the_draft() {
        let mut state = State::new(&Environment::default());
        let previous = state.java.clone();
        let _ = state.update(Message::Java("other-java".into()), true);
        assert_eq!(state.java, previous);
        state.busy = true;
        let _ = state.update(Message::Java("other-java".into()), false);
        assert_eq!(state.java, previous);
        state.max_memory = "8192".into();
        let _ = state.update(Message::Saved(Err("disk full".into())), false);
        assert!(!state.busy);
        assert_eq!(state.max_memory, "8192");
        assert!(state.notice.contains("disk full"));
    }

    #[test]
    fn importing_java_updates_dropdown_and_configuration_without_losing_the_selection_on_rescan() {
        let mut state = State::new(&Environment::default());
        let runtime = mc_core::JavaRuntime {
            executable: PathBuf::from("C:/custom-jdk/bin/java.exe"),
            major_version: 21,
            version: "21.0.9".into(),
            arch: "amd64".into(),
            os_version: "10.0".into(),
        };
        let _ = state.update(Message::JavaChecked(Ok(runtime.clone())), false);
        let selected = state.selected_java().unwrap();
        assert!(selected.to_string().contains("Java 21 (21.0.9)"));
        assert_eq!(
            state.configuration().unwrap().java_paths,
            vec![runtime.executable.clone()]
        );
        let _ = state.update(
            Message::JavaScanned(Ok(JavaDiscovery {
                runtimes: vec![runtime.clone()],
                failures: Vec::new(),
            })),
            false,
        );
        assert_eq!(state.selected_java(), Some(selected));
        assert_eq!(
            state
                .java_choices
                .iter()
                .filter(|choice| choice.path().is_some())
                .count(),
            1
        );
        let previous = state.java.clone();
        let _ = state.update(
            Message::JavaChecked(Err("not a Java executable".into())),
            false,
        );
        assert_eq!(state.java, previous);
        assert!(state.java_notice.contains("not a Java executable"));
    }
}
