use crate::app::{Launcher, Message as AppMessage};
use crate::assets::{icon, Icon};
use crate::theme::{self, bold, label, GREEN, MUTED, PURPLE, WHITE};
use iced::widget::{
    button, column, container, pick_list, row, scrollable, text_input, toggler, Space,
};
use iced::{Alignment, Color, Element, Fill, Length};

const TABS: [&str; 7] = [
    "常规",
    "游戏设置",
    "下载安装",
    "环境配置",
    "AI 设置",
    "界面外观",
    "高级设置",
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
    switches: [bool; 5],
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
            switches: [false, true, true, false, false],
            language: LANGUAGES[0],
            region: REGIONS[0],
            date: DATES[0],
            notice: "设置预览：开关与选项仅在本次会话生效，尚未保存到磁盘。",
        }
    }
}

impl State {
    pub fn update(&mut self, message: Message) {
        match message {
            Message::Tab(tab) => {
                self.tab = tab.min(TABS.len() - 1);
                self.query.clear();
            }
            Message::Search(query) => self.query = query,
            Message::Toggle(index, value) => {
                if let Some(setting) = self.switches.get_mut(index) {
                    *setting = value;
                }
            }
            Message::Language(value) => self.language = value,
            Message::Region(value) => self.region = value,
            Message::Date(value) => self.date = value,
            Message::Reset => *self = Self::default(),
            Message::Notice(value) => self.notice = value,
        }
    }
}

fn dispatch(message: Message) -> AppMessage {
    AppMessage::Settings(message)
}

// Both rows allocate their card heights once. Account content scrolls inside its
// cell and cannot push its bottom edge into the quick-actions row.
pub fn view(app: &Launcher) -> Element<'_, AppMessage> {
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
        container(scrollable(results).direction(theme::scroll_direction()))
            .padding(20)
            .width(Fill)
            .height(Fill)
            .style(theme::panel)
            .into()
    } else if state.tab == 0 {
        let top_height = if app.viewport.height < 740. {
            278.
        } else {
            292.
        };
        column![
            row![
                panel(Icon::Play, GREEN, "启动器行为", behavior(state)).width(Fill),
                panel(
                    Icon::Info,
                    Color::from_rgb(0.2, 0.75, 1.),
                    "语言与地区",
                    locale(state)
                )
                .width(Fill),
                panel(
                    Icon::Gamepad,
                    Color::from_rgb(0.2, 0.75, 1.),
                    "账户与数据",
                    account(app)
                )
                .width(Fill),
            ]
            .spacing(12)
            .height(top_height),
            row![
                panel(
                    Icon::Download,
                    Color::from_rgb(0.2, 0.75, 1.),
                    "启动器更新",
                    updates(state)
                )
                .width(Fill),
                panel(
                    Icon::Spark,
                    Color::from_rgb(1., 0.8, 0.2),
                    "快捷操作",
                    quick_actions()
                )
                .width(Fill),
            ]
            .spacing(12)
            .height(Length::Fill),
        ]
        .spacing(12)
        .height(Fill)
        .into()
    } else {
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
        .style(theme::panel)
        .into()
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
            label(state.notice, 10.).color(MUTED),
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

fn panel<'a>(
    kind: Icon,
    tint: Color,
    title: &'static str,
    content: Element<'a, AppMessage>,
) -> iced::widget::Container<'a, AppMessage> {
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
    .style(theme::panel)
}

fn behavior(state: &State) -> Element<'_, AppMessage> {
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
                toggler(state.switches[index])
                    .size(26)
                    .on_toggle(move |value| dispatch(Message::Toggle(index, value))),
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
                bold(
                    if app.profile == 0 {
                        "Steve_AI"
                    } else {
                        "Alex_Builder"
                    },
                    13.
                ),
                label("离线示例档案", 9.).color(MUTED),
            ]
            .spacing(4)
            .width(Fill),
            button(label("更换账户", 10.))
                .padding(7)
                .style(theme::quiet)
                .on_press(AppMessage::CycleProfile),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
        label("数据管理", 10.).color(MUTED),
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

fn updates(state: &State) -> Element<'_, AppMessage> {
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
            toggler(state.switches[4])
                .size(26)
                .on_toggle(|v| dispatch(Message::Toggle(4, v))),
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
