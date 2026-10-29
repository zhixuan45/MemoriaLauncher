//! Download navigation backed by Mojang's official version catalogue.
use crate::assets::{icon, picture, Icon, ART};
use crate::motion_renderer::Element;
use crate::theme::{self, bold, label, GREEN, MUTED, WHITE};
use iced::widget::{
    button, column, container, pick_list, progress_bar, row, scrollable, text_input,
};
use iced::{Alignment, Fill, Length, Task};
use mc_core::{
    DownloadClient, DownloadProgress, DownloadSettings, GameVersion, GameVersionType,
    MojangCatalogProvider,
};
use std::path::PathBuf;

pub const CATEGORIES: [&str; 5] = ["游戏版本", "Mod", "整合包", "资源包", "光影包"];
const TYPES: [&str; 3] = ["全部版本", "正式版", "快照版"];
const LOADERS: [&str; 5] = ["全部加载器", "原版", "Fabric", "Forge", "NeoForge"];
const SORT: [&str; 2] = ["最新发布", "最早发布"];
const TABS: [&str; 4] = ["版本介绍", "版本文件", "配置要求", "相关资源"];

#[derive(Debug, Clone)]
pub enum Message {
    NewInstance,
    Category(usize),
    Resources(crate::resources::Message),
    Search(String),
    VersionType(&'static str),
    Loader(&'static str),
    Sort(&'static str),
    Select(String),
    Tab(usize),
    Install(String),
    Loaded(Result<Vec<GameVersion>, String>),
    CatalogLoaded(u64, Result<Vec<GameVersion>, String>),
    InstallProgress(DownloadProgress),
    Installed(String, PathBuf, Result<(), String>),
    Retry,
}

pub struct State {
    pub category: usize,
    pub resources: crate::resources::State,
    pub query: String,
    pub versions: Vec<GameVersion>,
    pub selected: Option<String>,
    pub tab: usize,
    pub loading: bool,
    loaded: bool,
    version_type: &'static str,
    loader: &'static str,
    sort: &'static str,
    pub notice: String,
    pub client: Result<DownloadClient, String>,
    pub installing: Option<String>,
    pub progress: Option<DownloadProgress>,
    pub catalog_generation: u64,
}
impl Default for State {
    fn default() -> Self {
        Self::new(DownloadSettings::default())
    }
}
impl State {
    pub fn new(config: DownloadSettings) -> Self {
        Self {
            category: 0,
            resources: crate::resources::State::default(),
            query: String::new(),
            versions: vec![],
            selected: None,
            tab: 0,
            loading: false,
            loaded: false,
            version_type: TYPES[1],
            loader: LOADERS[0],
            sort: SORT[0],
            notice: format!(
                "下载源：{} · {} 个并行下载",
                config.source, config.parallel_downloads
            ),
            client: DownloadClient::new(config).map_err(|e| e.to_string()),
            installing: None,
            progress: None,
            catalog_generation: 0,
        }
    }

    pub fn configure(&mut self, config: DownloadSettings) {
        self.client = DownloadClient::new(config.clone()).map_err(|e| e.to_string());
        self.catalog_generation += 1;
        self.loading = false;
        self.loaded = false;
        self.versions.clear();
        self.selected = None;
        if self.installing.is_none() {
            self.notice = format!(
                "下载源：{} · {} 个并行下载",
                config.source, config.parallel_downloads
            );
        }
    }

    pub fn ensure_loaded(&mut self) -> Task<Message> {
        if self.category != 0 {
            return self.resources.ensure_loaded().map(Message::Resources);
        }
        if self.loading || self.loaded {
            return Task::none();
        }
        self.loading = true;
        let client = self.client.clone();
        let generation = self.catalog_generation;
        Task::perform(
            async move {
                let client = client?;
                MojangCatalogProvider::list_with_client(&client)
                    .await
                    .map_err(|e| e.to_string())
            },
            move |result| Message::CatalogLoaded(generation, result),
        )
    }
    pub fn filtered(&self) -> Vec<&GameVersion> {
        let query = self.query.trim().to_lowercase();
        let mut versions: Vec<_> = self
            .versions
            .iter()
            .filter(|v| {
                (query.is_empty()
                    || v.title.to_lowercase().contains(&query)
                    || v.id.contains(&query))
                    && (self.version_type == TYPES[0]
                        || (self.version_type == TYPES[1]
                            && v.version_type == GameVersionType::Release)
                        || (self.version_type == TYPES[2]
                            && v.version_type == GameVersionType::Snapshot))
                    && (self.loader == LOADERS[0]
                        || v.supported_loaders.iter().any(|l| l == self.loader))
            })
            .collect();
        versions.sort_by_key(|v| v.release_time);
        if self.sort == SORT[0] {
            versions.reverse();
        }
        versions
    }
    fn reconcile_selection(&mut self) {
        let filtered = self.filtered();
        if !filtered
            .iter()
            .any(|v| Some(&v.id) == self.selected.as_ref())
        {
            self.selected = filtered.first().map(|v| v.id.clone());
        }
    }
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::NewInstance=>{},
            Message::Resources(message) => {
                return self.resources.update(message).map(Message::Resources)
            }
            Message::Category(i) => {
                self.category = i.min(CATEGORIES.len() - 1);
                if self.category != 0 {
                    return self
                        .resources
                        .category(self.category)
                        .map(Message::Resources);
                }
                return self.ensure_loaded();
            }
            Message::Search(v) => {
                if self.category != 0 {
                    return self
                        .resources
                        .update(crate::resources::Message::Query(v))
                        .map(Message::Resources);
                }
                self.query = v;
                self.reconcile_selection();
            }
            Message::VersionType(v) => {
                self.version_type = v;
                self.reconcile_selection();
            }
            Message::Loader(v) => {
                self.loader = v;
                self.reconcile_selection();
            }
            Message::Sort(v) => self.sort = v,
            Message::Select(id) => {
                if self.versions.iter().any(|v| v.id == id) {
                    self.selected = Some(id);
                }
            }
            Message::Tab(i) => self.tab = i.min(TABS.len() - 1),
            Message::Install(id) => {
                self.installing = Some(id.clone());
                self.progress = None;
                self.notice = format!("{id}：正在读取版本信息并准备下载…");
            }
            Message::InstallProgress(progress) => {
                self.notice = format!(
                    "{} · 文件 {}/{} · 已复用 {} 个文件",
                    self.installing.as_deref().unwrap_or("安装中"),
                    progress.completed,
                    progress.total,
                    progress.reused
                );
                self.progress = Some(progress);
            }
            Message::Installed(id, _, result) => {
                self.installing = None;
                self.progress = None;
                self.notice = match result {
                    Ok(()) => format!("{id} 安装完成，可在主页选择此实例。"),
                    Err(error) => format!("{id} 安装失败：{error}"),
                };
            }
            Message::CatalogLoaded(generation, result) => {
                if generation == self.catalog_generation {
                    return self.update(Message::Loaded(result));
                }
            }
            Message::Loaded(result) => {
                self.loading = false;
                match result {
                    Ok(versions) => {
                        self.versions = versions;
                        self.loaded = true;
                        self.reconcile_selection();
                        if self.installing.is_none() {
                            self.notice = format!(
                                "已读取 {} 个版本 · {}",
                                self.versions.len(),
                                self.client
                                    .as_ref()
                                    .map(|client| format!(
                                        "下载源：{} · {} 个并行下载",
                                        client.settings().source,
                                        client.settings().parallel_downloads
                                    ))
                                    .unwrap_or_default()
                            );
                        }
                    }
                    Err(e) => self.notice = format!("读取目录失败：{e}"),
                }
            }
            Message::Retry => {
                if self.loading {
                    return Task::none();
                }
                self.loaded = false;
                return self.ensure_loaded();
            }
        }
        Task::none()
    }
}

pub fn install(client: DownloadClient, root: PathBuf, id: String) -> Task<Message> {
    use iced::futures::SinkExt;
    use std::sync::{Arc, Mutex};
    Task::run(
        iced::stream::channel(
            8,
            move |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
                let sender = Arc::new(Mutex::new(output.clone()));
                let progress = Arc::new(move |progress| {
                    if let Ok(mut sender) = sender.lock() {
                        let _ = sender.try_send(Message::InstallProgress(progress));
                    }
                });
                let result = async {
                    let layout = mc_core::GameLayout::new(&root)?;
                    mc_core::install_vanilla(client, layout, id.clone(), progress).await
                }
                .await
                .map_err(|e: mc_core::LauncherError| e.to_string());
                let _ = output.send(Message::Installed(id, root, result)).await;
            },
        ),
        |message| message,
    )
}

fn tab_button<'a>(title: &'a str, active: bool, message: Message) -> Element<'a, Message> {
    button(label(title, 12.))
        .padding([10, 12])
        .width(Fill)
        .style(if active {
            theme::selected
        } else {
            theme::quiet
        })
        .on_press(message)
        .into()
}

pub fn view(state: &State, cards: f32) -> Element<'_, Message> {
    let mut categories = row![].spacing(8);
    for (i, title) in CATEGORIES.iter().enumerate() {
        let kind = [
            Icon::Gamepad,
            Icon::Sliders,
            Icon::Cube,
            Icon::Edit,
            Icon::Spark,
        ][i];
        categories = categories.push(crate::motion::pop(
            button(
                row![
                    icon(kind, if state.category == i { GREEN } else { WHITE }, 25.),
                    column![
                        bold(*title, 14.),
                        label(
                            [
                                "原版 Minecraft",
                                "海量模组",
                                "精选整合包",
                                "材质 / 音效",
                                "光照 / 阴影"
                            ][i],
                            10.
                        )
                        .color(MUTED)
                    ]
                    .spacing(4)
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .padding(12)
            .width(Fill)
            .style(if state.category == i {
                theme::selected
            } else {
                theme::quiet
            })
            .on_press(Message::Category(i)),
            cards,
            true,
        ));
    }
    let body: Element<'_, Message> = if state.category == 0 {
        let mut list = column![].spacing(6);
        for v in state.filtered() {
            let active = state.selected.as_deref() == Some(v.id.as_str());
            list = list.push(
                row![
                    button(
                        row![
                            picture(&ART.grass, 42.),
                            column![
                                bold(&v.title, 15.),
                                label(
                                    format!(
                                        "{} · {} · {}",
                                        v.release_date,
                                        version_kind(v),
                                        v.java_version
                                    ),
                                    10.
                                )
                                .color(MUTED)
                            ]
                            .spacing(6)
                            .width(Fill)
                        ]
                        .spacing(10)
                        .align_y(Alignment::Center)
                    )
                    .width(Fill)
                    .padding(10)
                    .style(if active {
                        theme::selected
                    } else {
                        theme::quiet
                    })
                    .on_press(Message::Select(v.id.clone())),
                    button(label("安装", 12.))
                        .padding([19, 12])
                        .style(theme::action)
                        .on_press_maybe(
                            (state.installing.is_none()).then_some(Message::Install(v.id.clone()))
                        )
                ]
                .spacing(6)
                .align_y(Alignment::Center),
            );
        }
        if state.filtered().is_empty() {
            list = list.push(
                label(
                    if state.loading {
                        "正在读取版本目录…"
                    } else {
                        "没有匹配的版本，请调整搜索或筛选条件。"
                    },
                    13.,
                )
                .color(MUTED),
            );
        }
        let filters = row![
            pick_list(TYPES, Some(state.version_type), Message::VersionType)
                .text_size(11)
                .width(Fill),
            pick_list(LOADERS, Some(state.loader), Message::Loader)
                .text_size(11)
                .width(Fill),
            pick_list(SORT, Some(state.sort), Message::Sort)
                .text_size(11)
                .width(Fill)
        ]
        .spacing(6);
        let left = container(
            column![
                filters,
                scrollable(list)
                    .direction(theme::scroll_direction())
                    .height(Fill),
                button(label("刷新目录", 11.))
                    .on_press(Message::Retry)
                    .style(theme::quiet)
            ]
            .spacing(12),
        )
        .padding(12)
        .height(Fill)
        .width(Length::FillPortion(5))
        .style(theme::panel);
        row![
            crate::motion::pop(left, cards, true),
            crate::motion::pop(
                container(details(state))
                    .padding(16)
                    .height(Fill)
                    .width(Length::FillPortion(6))
                    .style(theme::panel),
                cards,
                true
            )
        ]
        .spacing(12)
        .height(Fill)
        .into()
    } else {
        crate::resources::view(&state.resources).map(Message::Resources)
    };
    let mut content = column![
        row![icon(Icon::Download, GREEN, 24.), bold("游戏下载", 26.),iced::widget::Space::new().width(Fill),button(label("新建实例 / 导入整合包",12.)).padding(10).style(theme::quiet).on_press(Message::NewInstance)]
            .spacing(12)
            .align_y(Alignment::Center),
        label("选择版本、Mod 或整合包，快速开始你的冒险", 14.).color(MUTED),
        text_input(
            "搜索游戏版本 / Mod / 整合包…",
            if state.category == 0 {
                &state.query
            } else {
                &state.resources.query
            }
        )
        .style(theme::input)
        .on_input(Message::Search)
        .on_submit_maybe(
            (state.category != 0).then_some(Message::Resources(crate::resources::Message::Search))
        )
        .padding(12)
        .size(14),
        categories,
    ]
    .spacing(12);
    if state.category == 0 || state.installing.is_some() {
        content = content.push(label(&state.notice, 11.).color(MUTED));
    }
    if let Some(progress) = &state.progress {
        content = content.push(
            progress_bar(
                0.0..=progress.total.max(1) as f32,
                progress.completed as f32,
            )
            .girth(5),
        );
    }
    container(content.push(body).height(Fill))
        .padding(22)
        .width(Fill)
        .height(Fill)
        .into()
}

fn version_kind(v: &GameVersion) -> &'static str {
    if v.version_type == GameVersionType::Release {
        "正式版"
    } else {
        "快照版"
    }
}
fn details(state: &State) -> Element<'_, Message> {
    let Some(v) = state
        .versions
        .iter()
        .find(|v| Some(&v.id) == state.selected.as_ref())
    else {
        return container(label("选择左侧版本查看详情", 16.).color(MUTED))
            .center_x(Fill)
            .center_y(Fill)
            .into();
    };
    let mut tabs = row![].spacing(5);
    for (i, title) in TABS.iter().enumerate() {
        tabs = tabs.push(tab_button(title, state.tab == i, Message::Tab(i)));
    }
    let mut content = column![].spacing(12);
    if state.tab == 0 {
        content = content
            .push(label(&v.summary, 13.))
            .push(bold("版本信息", 15.));
        for item in &v.highlights {
            content = content.push(label(format!("· {item}"), 12.).color(MUTED));
        }
    } else {
        let items = match state.tab {
            1 => &v.version_files,
            2 => &v.system_requirements,
            _ => &v.related_resources,
        };
        for item in items {
            content = content.push(
                container(label(item, 13.))
                    .padding(12)
                    .width(Fill)
                    .style(theme::surface),
            );
        }
    }
    column![
        row![
            picture(&ART.grass, 56.),
            column![
                bold(&v.title, 25.),
                label(
                    format!("{} · 原版 · {}", version_kind(v), v.java_version),
                    12.
                )
                .style(theme::accent_text)
            ]
            .spacing(8)
        ]
        .spacing(14)
        .align_y(Alignment::Center),
        button(
            row![
                icon(Icon::Download, iced::Color::BLACK, 20.),
                bold("安装此版本", 16.).style(theme::action_text)
            ]
            .spacing(12)
            .align_y(Alignment::Center)
        )
        .padding(14)
        .width(Fill)
        .style(theme::action)
        .on_press_maybe((state.installing.is_none()).then_some(Message::Install(v.id.clone()))),
        tabs,
        scrollable(content)
            .direction(theme::scroll_direction())
            .height(Fill),
        label(
            format!(
                "发布于 {}    支持 {}",
                v.release_date,
                v.supported_loaders.join(" / ")
            ),
            10.
        )
        .color(MUTED)
    ]
    .spacing(14)
    .height(Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mc_core::{CatalogProvider, MockCatalogProvider};
    #[tokio::test]
    async fn filters_reconcile_selection_and_empty_results() {
        let mut state = State::default();
        let versions = MockCatalogProvider::new()
            .list_game_versions()
            .await
            .unwrap();
        let _ = state.update(Message::Loaded(Ok(versions)));
        assert!(state.selected.is_some());
        let _ = state.update(Message::Search("no-such-version".into()));
        assert!(state.selected.is_none());
        let _ = state.update(Message::Search("1.21".into()));
        assert!(state.filtered().iter().all(|v| v.id.contains("1.21")));
        assert!(state.selected.is_some());
        let _ = state.update(Message::VersionType(TYPES[2]));
        assert!(state
            .filtered()
            .iter()
            .all(|v| v.version_type == GameVersionType::Snapshot));
    }

    #[tokio::test]
    async fn applying_a_source_ignores_previous_inflight_catalogue_results() {
        let mut state = State::default();
        assert_eq!(state.ensure_loaded().units(), 1);
        let previous = state.catalog_generation;
        state.configure(DownloadSettings {
            source: mc_core::DownloadSource::Bmclapi,
            parallel_downloads: 3,
            ..Default::default()
        });
        assert_eq!(state.ensure_loaded().units(), 1);
        let versions = MockCatalogProvider::new()
            .list_game_versions()
            .await
            .unwrap();
        let _ = state.update(Message::CatalogLoaded(previous, Ok(versions.clone())));
        assert!(state.versions.is_empty());
        assert!(
            state.loading,
            "a stale reply must not settle the new request"
        );
        let _ = state.update(Message::CatalogLoaded(
            state.catalog_generation,
            Ok(versions),
        ));
        assert!(!state.loading);
        assert!(!state.versions.is_empty());
        assert_eq!(
            state.client.as_ref().unwrap().settings().parallel_downloads,
            3
        );
    }
}
