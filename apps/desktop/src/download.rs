//! Download navigation backed by Mojang's official version catalogue.
use crate::assets::{icon, picture, Icon, ART};
use crate::motion_renderer::Element;
use crate::theme::{self, bold, label, GREEN, MUTED, WHITE};
use iced::widget::{button, column, container, pick_list, row, scrollable, text_input, Space};
use iced::{Alignment, ContentFit, Fill, Length, Task};
use mc_core::{CatalogProvider, GameVersion, GameVersionType, MojangCatalogProvider};

pub const CATEGORIES: [&str; 5] = ["游戏版本", "Mod", "整合包", "资源包", "光影包"];
const TYPES: [&str; 3] = ["全部版本", "正式版", "快照版"];
const LOADERS: [&str; 5] = ["全部加载器", "原版", "Fabric", "Forge", "NeoForge"];
const SORT: [&str; 2] = ["最新发布", "最早发布"];
const TABS: [&str; 4] = ["版本介绍", "版本文件", "配置要求", "相关资源"];

#[derive(Debug, Clone)]
pub enum Message {
    Category(usize),
    Search(String),
    VersionType(&'static str),
    Loader(&'static str),
    Sort(&'static str),
    Select(String),
    Tab(usize),
    Banner(i32),
    Install(String),
    Loaded(Result<Vec<GameVersion>, String>),
    Retry,
}

pub struct State {
    pub category: usize,
    pub query: String,
    pub versions: Vec<GameVersion>,
    pub selected: Option<String>,
    pub tab: usize,
    pub banner: usize,
    pub loading: bool,
    loaded: bool,
    version_type: &'static str,
    loader: &'static str,
    sort: &'static str,
    pub notice: String,
}
impl Default for State {
    fn default() -> Self {
        Self {
            category: 0,
            query: String::new(),
            versions: vec![],
            selected: None,
            tab: 0,
            banner: 0,
            loading: false,
            loaded: false,
            version_type: TYPES[1],
            loader: LOADERS[0],
            sort: SORT[0],
            notice: "Mojang 官方版本清单 · 安装服务尚未接入".into(),
        }
    }
}
impl State {
    pub fn ensure_loaded(&mut self) -> Task<Message> {
        if self.loading || self.loaded {
            return Task::none();
        }
        self.loading = true;
        Task::perform(
            async {
                MojangCatalogProvider
                    .list_game_versions()
                    .await
                    .map_err(|e| e.to_string())
            },
            Message::Loaded,
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
            Message::Category(i) => self.category = i.min(CATEGORIES.len() - 1),
            Message::Search(v) => {
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
            Message::Banner(delta) => {
                self.banner = (self.banner as i32 + delta).rem_euclid(3) as usize
            }
            Message::Install(id) => {
                self.notice = format!("{id}：安装服务尚未接入，未创建下载任务。")
            }
            Message::Loaded(result) => {
                self.loading = false;
                match result {
                    Ok(versions) => {
                        self.versions = versions;
                        self.loaded = true;
                        self.reconcile_selection();
                        self.notice = format!(
                            "已读取 {} 个官方版本 · 安装服务尚未接入",
                            self.versions.len()
                        );
                    }
                    Err(e) => self.notice = format!("读取目录失败：{e}"),
                }
            }
            Message::Retry => {
                self.loaded = false;
                return self.ensure_loaded();
            }
        }
        Task::none()
    }
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
                        .on_press(Message::Install(v.id.clone()))
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
        crate::motion::pop(
            container(
                column![
                    icon(Icon::Download, GREEN, 40.),
                    bold(CATEGORIES[state.category], 24.),
                    label("此资源目录尚未接入，当前可以浏览游戏版本。", 14.).color(MUTED),
                    button(label("浏览游戏版本", 14.))
                        .on_press(Message::Category(0))
                        .style(theme::action)
                        .padding(12)
                ]
                .spacing(20),
            )
            .padding(28)
            .width(Fill)
            .height(Fill)
            .style(theme::panel),
            cards,
            true,
        )
    };
    container(
        column![
            row![icon(Icon::Download, GREEN, 30.), bold("游戏下载", 32.)]
                .spacing(12)
                .align_y(Alignment::Center),
            label("选择版本、Mod 或整合包，快速开始你的冒险", 14.).color(MUTED),
            text_input("搜索游戏版本 / Mod / 整合包…", &state.query)
                .on_input(Message::Search)
                .padding(12)
                .size(14),
            categories,
            label(&state.notice, 11.).color(MUTED),
            body,
        ]
        .spacing(12)
        .height(Fill),
    )
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
            .push(
                container(
                    iced::widget::image(ART.banners[[2, 0, 1][state.banner]].clone())
                        .width(Fill)
                        .height(150)
                        .content_fit(ContentFit::Cover),
                )
                .width(Fill)
                .height(150)
                .clip(true),
            )
            .push(row![
                button(icon(Icon::Left, WHITE, 12.))
                    .on_press(Message::Banner(-1))
                    .style(theme::quiet),
                Space::new().width(Fill),
                button(icon(Icon::Right, WHITE, 12.))
                    .on_press(Message::Banner(1))
                    .style(theme::quiet)
            ])
            .push(label(&v.summary, 13.))
            .push(bold("主要更新内容", 15.));
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
                .color(GREEN)
            ]
            .spacing(8)
        ]
        .spacing(14)
        .align_y(Alignment::Center),
        button(
            row![
                icon(Icon::Download, iced::Color::BLACK, 20.),
                bold("安装此版本", 16.).color(iced::Color::BLACK)
            ]
            .spacing(12)
            .align_y(Alignment::Center)
        )
        .padding(14)
        .width(Fill)
        .style(theme::action)
        .on_press(Message::Install(v.id.clone())),
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
    use mc_core::MockCatalogProvider;
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
}
