use crate::app::{Launcher, Message as AppMessage};
use crate::assets::{icon, Icon};
use crate::motion::{self, Toggle};
use crate::motion_renderer::Element;
use crate::theme::{self, bold, label, GREEN, MUTED, PURPLE, WHITE};
use iced::widget::{button, column, container, pick_list, row, scrollable, text_input, Space};
use iced::{Alignment, Color, Fill, Length};
use std::time::Instant;

pub const LOGIN_TAB: usize = 7;
const TABS: [&str; 8] = [
    "常规",
    "游戏设置",
    "下载安装",
    "环境配置",
    "AI 设置",
    "界面外观",
    "高级设置",
    "账户登录",
];
const LANGUAGES: [&str; 3] = [
    "简体中文 (Chinese)",
    "繁體中文 (Traditional)",
    "English (US)",
];
const REGIONS: [&str; 3] = ["中国大陆", "中国香港 / 台湾", "Global"];
const DATES: [&str; 3] = ["YYYY-MM-DD", "DD/MM/YYYY", "MM/DD/YYYY"];

#[derive(Debug, Clone)]
pub enum Message {
    Tab(usize),
    Search(String),
    Toggle(usize, bool),
    Language(&'static str),
    Region(&'static str),
    Date(&'static str),
    Reset,
    Notice(&'static str),
}

pub struct State {
    tab: usize,
    query: String,
    switches: [Toggle; 5],
    language: &'static str,
    region: &'static str,
    date: &'static str,
    notice: &'static str,
}

impl Default for State {
    fn default() -> Self {
        Self {
            tab: 0,
            query: String::new(),
            switches: [false, true, true, false, false].map(Toggle::new),
            language: LANGUAGES[0],
            region: REGIONS[0],
            date: DATES[0],
            notice: "设置预览：开关与选项仅在本次会话生效，尚未保存到磁盘。",
        }
    }
}

impl State {
    pub fn animating(&self, now: Instant) -> bool {
        self.switches.iter().any(|toggle| toggle.active(now))
    }

    pub fn update(&mut self, message: Message, now: Instant) {
        match message {
            Message::Tab(tab) => {
                self.tab = tab.min(TABS.len() - 1);
                self.query.clear();
            }
            Message::Search(query) => self.query = query,
            Message::Toggle(index, value) => {
                if let Some(setting) = self.switches.get_mut(index) {
                    setting.set(value, now);
                }
            }
            Message::Language(value) => self.language = value,
            Message::Region(value) => self.region = value,
            Message::Date(value) => self.date = value,
            Message::Reset => {
                for (toggle, value) in self
                    .switches
                    .iter_mut()
                    .zip([false, true, true, false, false])
                {
                    toggle.set(value, now);
                }
                let mut defaults = Self::default();
                std::mem::swap(&mut defaults.switches, &mut self.switches);
                *self = defaults;
            }
            Message::Notice(value) => self.notice = value,
        }
    }
}

fn dispatch(message: Message) -> AppMessage {
    AppMessage::Settings(message)
}

// Both rows allocate their card heights once. Account content scrolls inside its
// cell and cannot push its bottom edge into the quick-actions row.
pub fn view(app: &Launcher, cards: f32) -> Element<'_, AppMessage> {
    let state = &app.settings;
    let mut tabs = row![].spacing(4);
    for (index, title) in TABS.iter().enumerate() {
        tabs = tabs.push(
            button(label(*title, 12.))
                .padding([11, 8])
                .width(Fill)
                .style(if state.tab == index {
                    theme::selected
                } else {
                    theme::quiet
                })
                .on_press(dispatch(Message::Tab(index))),
        );
    }
    let body: Element<'_, AppMessage> = if !state.query.trim().is_empty() {
        let query = state.query.trim().to_lowercase();
        let entries = [
            (
                "常规 / 启动器行为 / 语言与地区 / 账户与数据 / 更新 / 快捷操作",
                0,
            ),
            ("游戏设置 / 内存 / 分辨率", 1),
            ("下载安装 / 下载源 / 并行任务", 2),
            ("环境配置 / Java / 运行环境", 3),
            ("AI 设置 / 模型 / API / MCP", 4),
            ("界面外观 / 壁纸 / 主题 / 缩放", 5),
            ("高级设置 / 日志 / 诊断", 6),
            (
                "账户登录 / Microsoft / 微软 / 客户端 ID / Client ID / 回调地址",
                LOGIN_TAB,
            ),
        ];
        let mut results = column![].spacing(10);
        let mut found = false;
        for (title, tab) in entries {
            if title.to_lowercase().contains(&query) {
                found = true;
                results = results.push(
                    button(label(title, 14.))
                        .padding(16)
                        .width(Fill)
                        .style(theme::quiet)
                        .on_press(dispatch(Message::Tab(tab))),
                );
            }
        }
        if !found {
            results = results.push(label("没有匹配的设置项", 14.).color(MUTED));
        }
        motion::pop(
            container(scrollable(results).direction(theme::scroll_direction()))
                .padding(20)
                .width(Fill)
                .height(Fill)
                .style(theme::panel),
            cards,
            true,
        )
    } else if state.tab == 0 {
        let top_height = if app.viewport.height < 740. {
            278.
        } else {
            292.
        };
        column![
            row![
                panel(
                    Icon::Play,
                    GREEN,
                    "启动器行为",
                    behavior(state, app.animation_time),
                    cards
                ),
                panel(
                    Icon::Info,
                    Color::from_rgb(0.2, 0.75, 1.),
                    "语言与地区",
                    locale(state),
                    cards
                ),
                panel(
                    Icon::Gamepad,
                    Color::from_rgb(0.2, 0.75, 1.),
                    "账户与数据",
                    account(app),
                    cards
                ),
            ]
            .spacing(12)
            .height(top_height),
            row![
                panel(
                    Icon::Download,
                    Color::from_rgb(0.2, 0.75, 1.),
                    "启动器更新",
                    updates(state, app.animation_time),
                    cards
                ),
                panel(
                    Icon::Spark,
                    Color::from_rgb(1., 0.8, 0.2),
                    "快捷操作",
                    quick_actions(),
                    cards
                ),
            ]
            .spacing(12)
            .height(Length::Fill),
        ]
        .spacing(12)
        .height(Fill)
        .into()
    } else if state.tab == LOGIN_TAB {
        motion::pop(login_settings(app), cards, true)
    } else if state.tab == 3 {
        motion::pop(
            container(
                column![
                    bold("游戏与 Java 环境", 24.),
                    label("游戏根目录（包含 versions、libraries、assets）", 13.).color(MUTED),
                    text_input("游戏根目录", &app.game_root_input)
                        .on_input(AppMessage::GameRoot)
                        .padding(12),
                    label("Java 可执行文件路径，或 PATH 中的 java", 13.).color(MUTED),
                    text_input("java", &app.java_input)
                        .on_input(AppMessage::JavaPath)
                        .padding(12),
                    button(label("保存并重新扫描", 14.))
                        .padding(12)
                        .style(theme::action)
                        .on_press_maybe(
                            (!app.loading && !app.environment_saving && !app.launching)
                                .then_some(AppMessage::SaveEnvironment)
                        ),
                    label(&app.status, 12.).color(MUTED),
                ]
                .spacing(16),
            )
            .padding(24)
            .width(Fill)
            .height(Fill)
            .style(theme::panel),
            cards,
            true,
        )
    } else {
        motion::pop(
            container(
                column![
                    icon(Icon::Settings, PURPLE, 36.),
                    bold(TABS[state.tab], 24.),
                    label(
                        [
                            "",
                            "内存分配、分辨率与游戏启动参数",
                            "下载源、并行任务与安装路径",
                            "Java 运行环境与路径检测",
                            "模型提供商、API 连接与 MCP",
                            "壁纸、主题与界面缩放",
                            "日志、诊断与实验功能"
                        ][state.tab],
                        14.
                    )
                    .color(MUTED),
                    label("此模块的配置服务尚未接入。", 12.).color(MUTED),
                    button(label("返回常规设置", 13.))
                        .on_press(dispatch(Message::Tab(0)))
                        .style(theme::action)
                        .padding(12),
                ]
                .spacing(16),
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
            row![
                bold("设置", 30.),
                label("自定义你的 Minecraft 启动器", 12.).color(MUTED),
                Space::new().width(Fill),
                text_input("搜索设置项…", &state.query)
                    .on_input(|q| dispatch(Message::Search(q)))
                    .padding(10)
                    .size(12)
                    .width(220)
            ]
            .spacing(16)
            .align_y(Alignment::End),
            tabs,
            label(
                if state.tab == LOGIN_TAB {
                    "Microsoft 登录配置保存在本机，保存后立即生效。"
                } else {
                    state.notice
                },
                10.
            )
            .color(MUTED),
            body,
        ]
        .spacing(12)
        .height(Fill),
    )
    .padding(20)
    .width(Fill)
    .height(Fill)
    .into()
}

fn login_settings(app: &Launcher) -> Element<'_, AppMessage> {
    use crate::accounts::Message as AccountMessage;
    let accounts = &app.accounts;
    let content = column![
        bold("Microsoft 登录应用", 24.),
        label("填写应用概述页的应用程序（客户端）ID，保存一次即可。", 13.).color(MUTED),
        label("客户端 ID", 14.),
        text_input(
            "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx",
            &accounts.login_settings.client_id
        )
        .on_input_maybe(
            (!accounts.busy && !app.launching)
                .then_some(|value| AppMessage::Accounts(AccountMessage::ClientId(value)))
        )
        .padding(12),
        label("回调地址（与 Microsoft 应用注册保持一致）", 14.),
        text_input(
            "http://localhost:53682/oauth/callback",
            &accounts.login_settings.redirect_uri
        )
        .on_input_maybe(
            (!accounts.busy && !app.launching)
                .then_some(|value| AppMessage::Accounts(AccountMessage::RedirectUri(value)))
        )
        .padding(12),
        row![
            button(label(
                if accounts.settings_saving {
                    "正在保存…"
                } else {
                    "保存登录配置"
                },
                14.
            ))
            .on_press_maybe(
                (!accounts.busy && !app.launching)
                    .then_some(AppMessage::Accounts(AccountMessage::SaveLoginSettings))
            )
            .padding(12)
            .style(theme::action),
            button(label("管理账户", 14.))
                .on_press(AppMessage::Profile)
                .padding(12)
                .style(theme::quiet),
        ]
        .spacing(12),
        label(&accounts.settings_notice, 13.).color(MUTED),
    ]
    .spacing(16);
    container(scrollable(content).direction(theme::scroll_direction()))
        .padding(24)
        .width(Fill)
        .height(Fill)
        .style(theme::panel)
        .into()
}

fn panel<'a>(
    kind: Icon,
    tint: Color,
    title: &'static str,
    content: Element<'a, AppMessage>,
    cards: f32,
) -> Element<'a, AppMessage> {
    motion::pop(
        container(
            column![
                row![icon(kind, tint, 20.), bold(title, 15.)]
                    .spacing(10)
                    .align_y(Alignment::Center),
                scrollable(content)
                    .direction(theme::scroll_direction())
                    .height(Fill),
            ]
            .spacing(14),
        )
        .padding(14)
        .height(Fill)
        .width(Fill)
        .style(theme::panel),
        cards,
        true,
    )
}

fn behavior(state: &State, now: Instant) -> Element<'_, AppMessage> {
    let titles = [
        ("开机自动启动", "随系统启动时自动运行启动器"),
        ("启动后最小化到托盘", "启动器启动后自动最小化到托盘"),
        ("启动时自动检查更新", "启动器启动时检查新版本"),
        ("关闭时最小化到托盘", "点击关闭按钮时最小化而非退出"),
    ];
    let mut rows = column![].spacing(13);
    for (index, (title, detail)) in titles.into_iter().enumerate() {
        rows = rows.push(
            row![
                icon(Icon::Settings, GREEN, 17.),
                column![bold(title, 12.), label(detail, 9.).color(MUTED)]
                    .spacing(4)
                    .width(Fill),
                state.switches[index].view(now, 26., move |value| dispatch(Message::Toggle(
                    index, value
                ))),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    }
    rows.into()
}

fn locale(state: &State) -> Element<'_, AppMessage> {
    column![
        label("界面语言", 11.).color(MUTED),
        pick_list(LANGUAGES, Some(state.language), |v| dispatch(
            Message::Language(v)
        ))
        .text_size(11)
        .width(Fill),
        label("地区设置", 11.).color(MUTED),
        pick_list(REGIONS, Some(state.region), |v| dispatch(Message::Region(
            v
        )))
        .text_size(11)
        .width(Fill),
        label("日期与时间格式", 11.).color(MUTED),
        pick_list(DATES, Some(state.date), |v| dispatch(Message::Date(v)))
            .text_size(11)
            .width(Fill),
        label("部分内容可能仍使用原始语言。", 10.).color(MUTED),
    ]
    .spacing(7)
    .into()
}

fn account(app: &Launcher) -> Element<'_, AppMessage> {
    column![
        label("当前账户", 10.).color(MUTED),
        row![
            icon(Icon::Gamepad, GREEN, 30.),
            column![
                bold(app.accounts.name(), 13.),
                label(app.accounts.description(), 9.).color(MUTED),
            ]
            .spacing(4)
            .width(Fill),
            button(label("更换账户", 10.))
                .padding(7)
                .style(theme::quiet)
                .on_press(AppMessage::Profile),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        label("数据管理", 10.).color(MUTED),
        button(label("Microsoft 登录配置", 11.))
            .style(theme::quiet)
            .on_press(AppMessage::LoginSettings),
        action(
            Icon::Download,
            "备份与恢复",
            "备份游戏、设置、下载记录等",
            Message::Notice("备份与恢复尚未接入。"),
            false
        ),
        action(
            Icon::Folder,
            "打开数据文件夹",
            "打开启动器的存储目录",
            Message::Notice("持久化数据目录尚未接入。"),
            false
        ),
        action(
            Icon::Close,
            "清理缓存",
            "清理临时文件和缓存数据",
            Message::Notice("缓存管理尚未接入，未执行文件删除。"),
            true
        ),
    ]
    .spacing(8)
    .into()
}

fn updates(state: &State, now: Instant) -> Element<'_, AppMessage> {
    column![
        row![
            label("当前版本   v0.1.0", 12.),
            Space::new().width(Fill),
            label("未检查更新", 10.).color(MUTED),
        ]
        .align_y(Alignment::Center),
        button(label("检查更新", 12.))
            .padding(10)
            .width(Fill)
            .style(theme::quiet)
            .on_press(dispatch(Message::Notice(
                "更新服务尚未接入，无法确认是否为最新版本。"
            ))),
        row![
            column![
                bold("启用测试版更新", 12.),
                label("获取最新的测试功能（可能不稳定）", 10.).color(MUTED)
            ]
            .spacing(4)
            .width(Fill),
            state.switches[4].view(now, 26., |v| dispatch(Message::Toggle(4, v))),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    ]
    .spacing(16)
    .into()
}

fn action(
    kind: Icon,
    title: &'static str,
    detail: &'static str,
    message: Message,
    danger: bool,
) -> Element<'static, AppMessage> {
    button(
        row![
            icon(
                kind,
                if danger {
                    Color::from_rgb(1., 0.28, 0.35)
                } else {
                    GREEN
                },
                21.
            ),
            column![bold(title, 11.), label(detail, 9.).color(MUTED)]
                .spacing(3)
                .width(Fill),
            icon(Icon::Right, WHITE, 10.),
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding(9)
    .width(Fill)
    .style(theme::quiet)
    .on_press(dispatch(message))
    .into()
}

fn quick_actions() -> Element<'static, AppMessage> {
    column![
        row![
            action(
                Icon::Download,
                "导出启动器设置",
                "备份当前设置到文件",
                Message::Notice("设置导出尚未接入，未写入文件。"),
                false
            ),
            action(
                Icon::Folder,
                "导入启动器设置",
                "从文件恢复设置",
                Message::Notice("设置导入尚未接入。"),
                false
            ),
        ]
        .spacing(10),
        row![
            action(
                Icon::Edit,
                "恢复默认设置",
                "恢复本次会话的默认值",
                Message::Reset,
                false
            ),
            action(
                Icon::Close,
                "完全重置",
                "清除所有数据（不可恢复）",
                Message::Notice("完全重置尚未接入，未执行任何删除。"),
                true
            ),
        ]
        .spacing(10),
    ]
    .spacing(10)
    .into()
}
