use crate::app::{Launcher, Message as AppMessage};
use crate::assets::{icon, Icon};
use crate::general::{self, Config, DATES};
use crate::motion::{self, Toggle};
use crate::motion_renderer::Element;
use crate::theme::{self, bold, label, GREEN, MUTED, PURPLE, WHITE};
use iced::widget::{button, column, container, pick_list, row, scrollable, text_input, Space};
use iced::Task;
use iced::{Alignment, Color, Fill};
use std::time::Instant;

pub const LOGIN_TAB: usize = 7;
pub const AI_TAB: usize = 4;
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

#[derive(Debug, Clone)]
pub enum Message {
    Tab(usize),
    Search(String),
    Toggle(usize, bool),
    Date(&'static str),
    Reset,
    Revert,
    Save,
    Saved(Result<Config, String>),
    Import,
    Imported(Result<Option<Config>, String>),
    Export,
    Completed(Result<String, String>),
    OpenData,
    ClearCache,
    ConfirmCache,
    CancelCache,
    CheckUpdate,
    Checked(u64, Result<Option<crate::updates::Available>, String>),
    OpenRelease,
}

pub struct State {
    tab: usize,
    query: String,
    switches: [Toggle; 5],
    pub saved: Config,
    draft: Config,
    notice: String,
    busy: bool,
    confirm_cache: bool,
    checking: bool,
    update_generation: u64,
    update_notice: String,
    available: Option<crate::updates::Available>,
    startup_pending: bool,
    directory: std::path::PathBuf,
}

impl Default for State {
    fn default() -> Self {
        Self {
            tab: 0,
            query: String::new(),
            switches: Config::default().switches().map(Toggle::new),
            saved: Config::default(),
            draft: Config::default(),
            notice: "修改后点击保存常规设置，重启后继续生效。".into(),
            busy: false,
            confirm_cache: false,
            checking: false,
            update_generation: 0,
            update_notice: "未检查更新".into(),
            available: None,
            startup_pending: true,
            directory: crate::accounts::data_dir(),
        }
    }
}

#[path = "settings_flow.rs"]
mod flow;

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;

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
            (
                "游戏设置 / 实例集 / 文件夹 / 目录 / 内存 / Java / 运行环境",
                1,
            ),
            ("下载安装 / 下载源 / 镜像 / BMCLAPI / 并行 / 并发 / 线程", 2),
            ("环境配置", 3),
            ("AI 设置 / 模型 / API / MCP / 密钥 / 地址 / 接口", AI_TAB),
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
        scrollable(
            column![
                row![
                    panel(
                        Icon::Play,
                        GREEN,
                        "启动器行为",
                        behavior(state, app.animation_time),
                        cards
                    ),
                    panel(Icon::Info, MUTED, "语言与地区", locale(state), cards)
                ]
                .spacing(16)
                .height(270),
                row![
                    panel(Icon::Gamepad, GREEN, "账户与数据", account(app), cards),
                    panel(
                        Icon::Download,
                        MUTED,
                        "启动器更新",
                        updates(state, app.animation_time),
                        cards
                    )
                ]
                .spacing(16)
                .height(260),
                container(panel(
                    Icon::Sliders,
                    MUTED,
                    "快捷操作",
                    quick_actions(),
                    cards
                ))
                .height(185),
            ]
            .spacing(16),
        )
        .direction(theme::scroll_direction())
        .height(Fill)
        .into()
    } else if state.tab == LOGIN_TAB {
        motion::pop(login_settings(app), cards, true)
    } else if state.tab == 1 || state.tab == 3 {
        motion::pop(crate::game_settings::view(app), cards, true)
    } else if state.tab == 2 {
        motion::pop(crate::download_settings::view(app), cards, true)
    } else if state.tab == AI_TAB {
        motion::pop(crate::ai_settings::view(app), cards, true)
    } else if state.tab == 5 {
        motion::pop(crate::appearance::view(&app.appearance), cards, true)
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
                bold("设置", 26.),
                label("自定义你的 Minecraft 启动器", 12.).color(MUTED),
                Space::new().width(Fill),
                text_input("搜索设置项…", &state.query)
                    .on_input(|q| dispatch(Message::Search(q)))
                    .style(theme::input)
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
                } else if state.tab == 1 || state.tab == 3 {
                    "实例集、内存与 Java 配置保存在本机，点击保存并应用后生效。"
                } else if state.tab == 2 {
                    "下载源与并行下载数量保存在本机，保存后用于新的下载任务。"
                } else if state.tab == AI_TAB {
                    "AI 配置保存在本机，保存后用于后续咨询与日志分析。"
                } else if state.tab == 5 {
                    "外观配置保存在本机，保存后立即应用。"
                } else {
                    &state.notice
                },
                10.
            )
            .color(MUTED),
            body,
            if state.tab == 0 {
                row![
                    button(label("撤销常规更改", 12.))
                        .style(theme::quiet)
                        .padding(10)
                        .on_press_maybe((!state.busy).then_some(dispatch(Message::Revert))),
                    button(label("退出启动器", 12.))
                        .style(theme::quiet)
                        .padding(10)
                        .on_press(AppMessage::Quit),
                    Space::new().width(Fill),
                    button(label(
                        if state.busy {
                            "正在处理…"
                        } else {
                            "保存常规设置"
                        },
                        12.
                    ))
                    .style(theme::action)
                    .padding(10)
                    .on_press_maybe((!state.busy).then_some(dispatch(Message::Save))),
                ]
            } else {
                row![]
            },
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
        ("游戏启动后最小化", "游戏进程启动成功后最小化启动器"),
        ("启动器启动后最小化", "启动器加载完成后最小化到任务栏"),
        ("启动时自动检查更新", "连接 GitHub 检查项目发布记录"),
        ("关闭时最小化到任务栏", "点击关闭或 Alt+F4 时保留任务栏入口"),
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
        label("简体中文；其他语言尚待接入。", 11.),
        label("地区与下载源", 11.).color(MUTED),
        button(label("前往下载安装设置", 11.))
            .style(theme::quiet)
            .on_press(dispatch(Message::Tab(2))),
        label("日期与时间格式", 11.).color(MUTED),
        pick_list(DATES, Some(DATES[state.draft.date_format]), |v| dispatch(
            Message::Date(v)
        ))
        .text_size(11)
        .width(Fill),
        label("保存后用于主页最近打开时间（UTC）。", 10.).color(MUTED),
    ]
    .spacing(7)
    .into()
}

fn account(app: &Launcher) -> Element<'_, AppMessage> {
    if app.settings.confirm_cache {
        return column![
            bold("清理启动器缓存", 13.),
            label(
                "将启动器 cache 文件夹移至数据目录下的 recovered-cache，可手动恢复。",
                11.
            )
            .color(MUTED),
            label("游戏、日志、配置和安装备份保留。", 11.).color(MUTED),
            row![
                button(label("确认移动缓存", 11.))
                    .style(theme::action)
                    .padding(8)
                    .on_press(dispatch(Message::ConfirmCache)),
                button(label("取消", 11.))
                    .style(theme::quiet)
                    .padding(8)
                    .on_press(dispatch(Message::CancelCache)),
            ]
            .spacing(8),
        ]
        .spacing(12)
        .into();
    }
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
            Icon::Folder,
            "打开数据文件夹",
            "打开启动器的存储目录",
            Message::OpenData,
            false
        ),
        action(
            Icon::Close,
            "清理缓存",
            "将启动器 cache 目录移至恢复目录",
            Message::ClearCache,
            true
        ),
    ]
    .spacing(8)
    .into()
}

fn updates(state: &State, now: Instant) -> Element<'_, AppMessage> {
    column![
        row![
            label(concat!("当前版本   v", env!("CARGO_PKG_VERSION")), 12.),
            Space::new().width(Fill),
        ]
        .align_y(Alignment::Center),
        button(label("检查更新", 12.))
            .padding(10)
            .width(Fill)
            .style(theme::quiet)
            .on_press_maybe(
                (!state.checking && !state.busy).then_some(dispatch(Message::CheckUpdate))
            ),
        label(&state.update_notice, 10.).color(MUTED),
        button(label("打开新版本发布页", 11.))
            .style(theme::quiet)
            .on_press_maybe(
                (state.available.is_some() && !state.busy)
                    .then_some(dispatch(Message::OpenRelease))
            ),
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
                "导出常规设置",
                "仅导出已保存的开关和日期格式",
                Message::Export,
                false
            ),
            action(
                Icon::Folder,
                "导入常规设置",
                "载入 JSON 草稿，核对后保存",
                Message::Import,
                false
            ),
        ]
        .spacing(10),
        row![
            action(
                Icon::Edit,
                "恢复默认设置",
                "恢复常规设置草稿，保存后生效",
                Message::Reset,
                false
            ),
            action(
                Icon::Folder,
                "查看备份目录",
                "在数据目录查看 recovered-cache",
                Message::OpenData,
                false
            ),
        ]
        .spacing(10),
    ]
    .spacing(10)
    .into()
}
