//! AI workspace UI. No request is sent until a real provider is connected.
use crate::assets::{icon, Icon, ART};
use crate::theme::{self, bold, label, GREEN, MUTED, PURPLE, WHITE};
use iced::widget::{
    button, column, container, pick_list, progress_bar, row, scrollable, text_input, toggler, Space,
};
use iced::{Alignment, Border, Color, ContentFit, Element, Fill, Length, Task};

const VERSIONS: [&str; 4] = ["1.21", "1.20.6", "1.20.1", "1.19.2"];
const LANGUAGES: [&str; 3] = ["简体中文", "繁體中文", "English"];
const PROMPTS: [[&str; 8]; 2] = [
    [
        "帮我做一个 1.21 的冒险整合包",
        "推荐适合生存的 Mod",
        "帮我配置一个光影包",
        "汉化这个整合包",
        "优化当前游戏配置",
        "写一个数据包指令",
        "推荐材质包",
        "游戏问题解答",
    ],
    [
        "安装基础优化 Mod",
        "检查 Mod 依赖冲突",
        "汉化未翻译的 Mod",
        "为低配电脑推荐光影",
        "制作科技自动化整合包",
        "分析游戏崩溃日志",
        "整理整合包语言文件",
        "推荐多人联机配置",
    ],
];
const EXAMPLES: [(&str, &str); 4] = [
    ("RPG 冒险整合包", "按主题规划 Mod 列表与汉化任务"),
    ("科技生存整合包", "组织自动化、科技与生存玩法"),
    ("写实光影配置", "选择适合硬件的光影与画质设置"),
    ("自定义玩法", "生成数据包与命令需求"),
];

#[derive(Debug, Clone)]
pub enum Message {
    Prompt(String),
    UsePrompt(String),
    NextPrompts,
    Version(&'static str),
    Language(&'static str),
    Web(bool),
    Attach,
    Submit,
    ApiEndpoint(String),
    ApiModel(String),
    ApiKey(String),
    ApiReply(Result<String, String>),
    Pause(u64),
    Remove(u64),
}
pub struct TaskPreview {
    pub id: u64,
    title: &'static str,
    detail: &'static str,
    progress: f32,
    steps: &'static [&'static str],
    completed: usize,
    pub paused: bool,
    tint: Color,
}
pub struct State {
    pub prompt: String,
    batch: usize,
    version: &'static str,
    language: &'static str,
    web: bool,
    pub tasks: Vec<TaskPreview>,
    pub notice: String,
    pub endpoint: String,
    pub model: String,
    pub api_key: String,
    pub reply: Option<String>,
    pub busy: bool,
}
impl Default for State {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            batch: 0,
            version: VERSIONS[0],
            language: LANGUAGES[0],
            web: false,
            notice: "可配置 OpenAI 兼容 API 进行咨询；外部 MCP 工具需在 AI 客户端连接".into(),
            endpoint: "https://api.openai.com/v1".into(),
            model: String::new(),
            api_key: String::new(),
            reply: None,
            busy: false,
            tasks: vec![
                TaskPreview {
                    id: 1,
                    title: "汉化整合包：All the Mods 9",
                    detail: "中文语言文件 · 142 / 198",
                    progress: 72.,
                    steps: &["分析 Mod 列表", "下载语言文件", "翻译语言文件", "打包输出"],
                    completed: 2,
                    paused: false,
                    tint: PURPLE,
                },
                TaskPreview {
                    id: 2,
                    title: "下载 Mod：Sodium",
                    detail: "Sodium 0.5.11",
                    progress: 56.,
                    steps: &[],
                    completed: 0,
                    paused: false,
                    tint: GREEN,
                },
                TaskPreview {
                    id: 3,
                    title: "生成光影配置",
                    detail: "显卡与游戏设置",
                    progress: 60.,
                    steps: &[
                        "检测硬件配置",
                        "推荐光影包",
                        "优化参数",
                        "生成配置文件",
                        "完成",
                    ],
                    completed: 2,
                    paused: false,
                    tint: Color::from_rgb(0.2, 0.7, 0.95),
                },
            ],
        }
    }
}
impl State {
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ApiEndpoint(v) => self.endpoint = v,
            Message::ApiModel(v) => self.model = v,
            Message::ApiKey(v) => self.api_key = v,
            Message::ApiReply(result) => {
                self.busy = false;
                match result {
                    Ok(reply) => {
                        self.reply = Some(reply);
                        self.notice = "模型已回复；没有执行任何本地更改".into();
                    }
                    Err(err) => {
                        self.notice = format!("模型请求失败：{err}");
                    }
                }
            }
            Message::Prompt(v) | Message::UsePrompt(v) => self.prompt = v,
            Message::NextPrompts => self.batch = (self.batch + 1) % PROMPTS.len(),
            Message::Version(v) => self.version = v,
            Message::Language(v) => self.language = v,
            Message::Web(v) => self.web = v,
            Message::Attach => {
                self.notice = "整合包 / Mod 文件导入尚未接入，未读取或上传文件。".into()
            }
            Message::Submit if self.prompt.trim().is_empty() => {
                self.notice = "请先输入需求，或选择下方示例。".into()
            }
            Message::Submit if self.busy => {}
            Message::Submit if self.model.trim().is_empty() => {
                self.notice = "请先填写 API 模型名称；密钥仅保存在当前运行内存中".into();
            }
            Message::Submit => {
                self.busy = true;
                self.reply = None;
                self.notice = "正在请求模型（只读咨询，不执行工具）…".into();
                let endpoint = self.endpoint.clone();
                let model = self.model.clone();
                let key = self.api_key.clone();
                let prompt = self.prompt.trim().to_owned();
                let version = self.version.to_owned();
                let language = self.language.to_owned();
                return Task::perform(
                    async move { crate::api_client::chat(endpoint, model, key, prompt, version, language).await },
                    Message::ApiReply,
                );
            }
            Message::Pause(id) => {
                if let Some(task) = self.tasks.iter_mut().find(|t| t.id == id) {
                    task.paused = !task.paused;
                }
            }
            Message::Remove(id) => self.tasks.retain(|t| t.id != id),
        }
        Task::none()
    }
}

pub fn view(state: &State, available_width: f32) -> Element<'_, Message> {
    let compact = available_width < 1050.;
    let file_options = row![
        button(
            row![
                icon(Icon::Folder, MUTED, 18.),
                label("添加文件 / 整合包 / Mod", 11.)
            ]
            .spacing(8)
        )
        .padding(10)
        .on_press(Message::Attach)
        .style(theme::quiet),
        pick_list(VERSIONS, Some(state.version), Message::Version).text_size(12),
        pick_list(LANGUAGES, Some(state.language), Message::Language).text_size(12),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let send_options = row![
        Space::new().width(Fill),
        toggler(state.web)
            .label("联网搜索（未接入）")
            .text_size(11)
            .size(24),
        button(row![icon(Icon::Right, WHITE, 18.), label("发送", 12.)].spacing(6))
            .on_press(Message::Submit)
            .padding(12)
            .style(purple_button),
    ]
    .spacing(12)
    .align_y(Alignment::Center);
    let controls: Element<'_, Message> = if compact {
        column![file_options, send_options].spacing(10).into()
    } else {
        row![file_options, send_options]
            .spacing(12)
            .align_y(Alignment::Center)
            .into()
    };
    let composer = container(
        column![
            row![
                icon(Icon::Spark, PURPLE, 25.),
                text_input("请输入你的需求…", &state.prompt)
                    .on_input(Message::Prompt)
                    .on_submit(Message::Submit)
                    .padding(16)
                    .size(18)
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            controls,
        ]
        .spacing(20),
    )
    .padding(20)
    .width(Fill)
    .style(|_| {
        let mut style = theme::panel(&iced::Theme::Dark);
        style.border = Border {
            color: PURPLE,
            width: 1.5,
            radius: 14.into(),
        };
        style
    });
    let mut suggestions = column![row![
        bold("你可以这样问", 15.),
        Space::new().width(Fill),
        button(label("换一批", 12.))
            .on_press(Message::NextPrompts)
            .style(theme::quiet)
    ]
    .align_y(Alignment::Center)]
    .spacing(10);
    for chunk in PROMPTS[state.batch].chunks(if compact { 2 } else { 4 }) {
        let mut line = row![].spacing(8);
        for prompt in chunk {
            line = line.push(
                button(
                    row![icon(Icon::Spark, PURPLE, 17.), label(*prompt, 12.)]
                        .spacing(8)
                        .align_y(Alignment::Center),
                )
                .padding(12)
                .height(65)
                .width(Fill)
                .style(theme::quiet)
                .on_press(Message::UsePrompt((*prompt).into())),
            );
        }
        suggestions = suggestions.push(line);
    }
    let mut examples = column![].spacing(10);
    let mut example_row = row![].spacing(10);
    for (i, (title, description)) in EXAMPLES.iter().enumerate() {
        example_row = example_row.push(
            button(
                column![
                    container(
                        iced::widget::image(ART.banners[[1, 0, 2, 0][i]].clone())
                            .width(Fill)
                            .height(92)
                            .content_fit(ContentFit::Cover)
                    )
                    .width(Fill)
                    .height(92)
                    .clip(true),
                    bold(*title, 13.),
                    label(
                        format!(
                            "{} · {}",
                            state.version,
                            if i < 2 {
                                "整合包 / 汉化"
                            } else {
                                "个性化配置"
                            }
                        ),
                        10.
                    )
                    .color(PURPLE),
                    label(*description, 11.).color(MUTED),
                ]
                .spacing(8),
            )
            .padding(10)
            .width(Fill)
            .style(theme::quiet)
            .on_press(Message::UsePrompt(format!(
                "请帮我制作{}，目标版本 {}，输出语言 {}。",
                title, state.version, state.language
            ))),
        );
        if (i + 1) % (if compact { 2 } else { 4 }) == 0 {
            examples = examples.push(example_row);
            example_row = row![].spacing(10);
        }
    }
    let api_settings = container(
        column![
            bold("直连 API 配置", 14.),
            label("支持 OpenAI Chat Completions 兼容端点；密钥仅在内存中使用，关闭应用后失效。自定义服务会收到你的输入与密钥。", 11.).color(MUTED),
            text_input("API 地址，例如 https://api.openai.com/v1", &state.endpoint)
                .on_input(Message::ApiEndpoint).padding(10).size(12),
            row![
                text_input("模型名称（必填）", &state.model)
                    .on_input(Message::ApiModel).padding(10).size(12),
                text_input("API Key（本地服务可留空）", &state.api_key)
                    .secure(true).on_input(Message::ApiKey).padding(10).size(12),
            ].spacing(10),
            label("此模式仅提供问答与建议；外部 Agent 可连接 mc-mcp 使用真实只读工具。", 11.).color(MUTED),
        ].spacing(10)
    ).padding(14).style(theme::panel);
    let mut content = column![
        row![
            icon(Icon::Spark, PURPLE, 40.),
            column![
                bold("AI 功能", 34.),
                label("用 AI 让你的 Minecraft 更简单，更有趣", 14.).color(MUTED)
            ]
            .spacing(8)
        ]
        .spacing(16)
        .align_y(Alignment::Center),
        Space::new().height(12),
        composer,
        label(&state.notice, 11.).color(MUTED),
        api_settings,
        suggestions,
        container(column![bold("实用示例", 15.), examples].spacing(12))
            .padding(14)
            .style(theme::panel),
    ]
    .spacing(18);
    if let Some(reply) = &state.reply {
        content = content.push(
            container(column![
                bold("模型回复（未执行本地操作）", 15.),
                label(reply, 13.).color(WHITE)
            ].spacing(10)).padding(14).style(theme::panel)
        );
    }
    let panel_width = 268.;
    container(
        row![
            scrollable(content)
                .direction(theme::scroll_direction())
                .width(Length::FillPortion(3))
                .height(Fill),
            container(task_panel(state))
                .width(panel_width)
                .height(Fill)
                .padding(14)
                .style(theme::panel)
        ]
        .spacing(16)
        .height(Fill),
    )
    .padding(22)
    .width(Fill)
    .height(Fill)
    .into()
}

fn purple_button(_: &iced::Theme, status: button::Status) -> button::Style {
    let mut style = theme::action(&iced::Theme::Dark, status);
    style.background = Some(PURPLE.into());
    style.text_color = WHITE;
    style
}
fn task_panel(state: &State) -> Element<'_, Message> {
    let mut tasks = column![].spacing(12);
    for task in &state.tasks {
        let mut steps = column![].spacing(8);
        for (i, step) in task.steps.iter().enumerate() {
            let tint = if i < task.completed {
                GREEN
            } else if i == task.completed {
                task.tint
            } else {
                MUTED
            };
            steps = steps.push(
                row![
                    icon(
                        if i < task.completed {
                            Icon::Cube
                        } else {
                            Icon::Info
                        },
                        tint,
                        12.
                    ),
                    label(*step, 11.).color(tint)
                ]
                .spacing(8),
            );
        }
        tasks = tasks.push(
            container(
                column![
                    bold(task.title, 13.),
                    label(
                        if task.paused {
                            "已暂停（示例）"
                        } else {
                            task.detail
                        },
                        11.
                    )
                    .color(MUTED),
                    row![
                        progress_bar(0.0..=100., task.progress)
                            .girth(6)
                            .style(move |_| progress_bar::Style {
                                background: Color::from_rgb(0.12, 0.2, 0.25).into(),
                                bar: if task.paused { MUTED } else { task.tint }.into(),
                                border: iced::border::rounded(4),
                            }),
                        label(format!("{}%", task.progress as u8), 10.).color(MUTED)
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                    steps,
                    row![
                        button(icon(
                            if task.paused { Icon::Play } else { Icon::Pause },
                            WHITE,
                            14.
                        ))
                        .padding(8)
                        .style(theme::quiet)
                        .on_press(Message::Pause(task.id)),
                        button(icon(Icon::Close, WHITE, 14.))
                            .padding(8)
                            .style(theme::quiet)
                            .on_press(Message::Remove(task.id))
                    ]
                    .spacing(10),
                ]
                .spacing(12),
            )
            .padding(12)
            .width(Fill)
            .style(theme::surface),
        );
    }
    if state.tasks.is_empty() {
        tasks = tasks.push(label("暂无任务", 13.).color(MUTED));
    }
    column![
        row![
            icon(Icon::Cube, PURPLE, 20.),
            bold(format!("任务列表 ({})", state.tasks.len()), 17.)
        ]
        .spacing(10),
        label("示例预览", 10.).color(MUTED),
        scrollable(tasks)
            .direction(theme::scroll_direction())
            .height(Fill)
    ]
    .spacing(12)
    .height(Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn submission_never_fabricates_work_and_task_ids_are_stable() {
        let mut state = State::default();
        state.update(Message::Prompt("汉化整合包".into()));
        state.update(Message::Submit);
        assert_eq!(state.tasks.len(), 3);
        assert_eq!(state.prompt, "汉化整合包");
        state.update(Message::Remove(1));
        state.update(Message::Pause(3));
        assert!(state.tasks.iter().find(|t| t.id == 3).unwrap().paused);
    }
}
