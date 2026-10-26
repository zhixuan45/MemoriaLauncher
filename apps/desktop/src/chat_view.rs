use crate::{
    ai::{Message, State},
    api_client::Role,
    assets::{icon, Icon},
    motion_renderer::Element,
    theme::{self, bold, label, GREEN, MUTED, WHITE},
    workspace::ConversationInfo,
};
use iced::{
    widget::{button, column, container, row, scrollable, text_input, tooltip, Space},
    Alignment, Fill,
};

pub fn view<'a>(state: &'a State, info: &'a ConversationInfo, cards: f32) -> Element<'a, Message> {
    let empty = state.messages.is_empty()
        && state.pending.is_none()
        && state.failed.is_none()
        && !state.attachments.has_content()
        && !state.attachments.menu_open
        && state.crash_log.is_none();
    let composer: Element<'_, Message> = container(
        scrollable(composer(state, info))
            .direction(theme::scroll_direction())
            .height(iced::Length::Shrink),
    )
    .max_height(350)
    .width(Fill)
    .into();
    let body: Element<'_, Message> = if empty {
        column![
            Space::new().height(iced::Length::FillPortion(3)),
            container(
                column![
                    bold("今天想做些什么？", 30.),
                    label("围绕你的 Minecraft 实例，开始一次对话", 14.).color(MUTED),
                    Space::new().height(16),
                    composer,
                ]
                .spacing(14)
                .align_x(Alignment::Center)
            )
            .max_width(820)
            .width(Fill),
            Space::new().height(iced::Length::FillPortion(4)),
        ]
        .align_x(Alignment::Center)
        .height(Fill)
        .into()
    } else {
        let mut transcript = column![].spacing(26).width(Fill);
        for message in &state.messages {
            transcript = transcript.push(bubble(&message.content, message.role == Role::User));
        }
        if let Some(prompt) = &state.pending {
            transcript = transcript
                .push(bubble(prompt, true))
                .push(label("正在回复…", 12.).color(MUTED));
        }
        if let Some(prompt) = &state.failed {
            transcript = transcript.push(bubble(prompt, true)).push(
                button(label("重试发送", 12.))
                    .padding(8)
                    .style(theme::quiet)
                    .on_press_maybe((!state.busy && !state.loading_log).then_some(Message::Retry)),
            );
        }
        column![
            scrollable(
                container(transcript)
                    .padding([24, 10])
                    .max_width(820)
                    .width(Fill)
            )
            .id("ai-transcript")
            .direction(theme::scroll_direction())
            .height(Fill)
            .width(Fill),
            container(composer).max_width(820).width(Fill),
        ]
        .spacing(18)
        .align_x(Alignment::Center)
        .height(Fill)
        .into()
    };
    container(crate::motion::pop(body, cards, true))
        .padding([20, 32])
        .width(Fill)
        .height(Fill)
        .into()
}

fn composer<'a>(state: &'a State, info: &'a ConversationInfo) -> Element<'a, Message> {
    let enabled = !state.busy && !state.loading_log && !state.attachments.loading();
    let mut body = column![crate::attachments_view::view(state)]
        .spacing(12)
        .width(Fill);
    if let Some(log) = &state.crash_log {
        let preview: String = log.text.chars().take(4000).collect();
        body = body.push(
            container(
                column![
                    row![
                        bold("待发送的崩溃日志", 12.),
                        Space::new().width(Fill),
                        button(label("移除崩溃日志", 11.))
                            .padding(6)
                            .style(theme::quiet)
                            .on_press_maybe(enabled.then_some(Message::RemoveCrashLog))
                    ],
                    label(
                        format!(
                            "{} · 已遮蔽 {} 行；点击发送才会上传，日志可能包含玩家名和本地路径。",
                            log.path, log.redacted_lines
                        ),
                        10.
                    )
                    .color(MUTED),
                    scrollable(label(preview, 11.)).height(90),
                ]
                .spacing(8),
            )
            .padding(12)
            .style(theme::surface),
        );
    }
    let input = text_input("描述你的需求，或询问这个实例的问题…", &state.prompt)
        .id("ai-prompt")
        .on_input(Message::Prompt)
        .on_submit(Message::Submit)
        .padding(12)
        .size(15)
        .style(|theme, status| {
            let mut style = theme::input(theme, status);
            style.background = iced::Color::TRANSPARENT.into();
            style.border.width = 0.;
            style
        });
    let send = container(tooltip(
        button(icon(
            Icon::ArrowUp,
            if enabled && (!state.prompt.trim().is_empty() || !state.attachments.files.is_empty()) {
                iced::Color::BLACK
            } else {
                MUTED
            },
            19.,
        ))
        .padding(11)
        .style(theme::action)
        .on_press_maybe(
            (enabled && (!state.prompt.trim().is_empty() || !state.attachments.files.is_empty()))
                .then_some(Message::Submit),
        ),
        "发送",
        tooltip::Position::Top,
    ))
    .id("ai-send");
    body = body.push(
        container(
            row![
                container(tooltip(
                    button(icon(Icon::Plus, MUTED, 19.))
                        .padding(10)
                        .style(theme::quiet)
                        .on_press_maybe(
                            (!state.busy && !state.loading_log)
                                .then_some(Message::Attachments(crate::attachments::Message::Menu))
                        ),
                    "添加上下文或文件",
                    tooltip::Position::Top
                ))
                .id("ai-attach"),
                input,
                send
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        )
        .padding([10, 12])
        .width(Fill)
        .style(theme::composer),
    );
    let context = info
        .project
        .as_ref()
        .map(|p| format!("{} · {} · {}", p.name, p.version, p.loader))
        .unwrap_or_else(|| "独立对话 · 在左侧选择实例项目".into());
    body = body.push(
        row![
            icon(Icon::Cube, GREEN, 14.),
            label(context, 10.).color(MUTED),
            Space::new().width(Fill),
            button(
                row![
                    label(
                        if state.connection.config.model.is_empty() {
                            "选择模型"
                        } else {
                            &state.connection.config.model
                        },
                        11.
                    )
                    .color(MUTED),
                    icon(Icon::ChevronDown, MUTED, 12.)
                ]
                .spacing(6)
            )
            .padding(4)
            .style(theme::quiet)
            .on_press(Message::OpenSettings),
        ]
        .spacing(8)
        .align_y(Alignment::Center),
    );
    if !state.notice.is_empty() && !state.busy {
        body = body.push(label(&state.notice, 11.).color(MUTED));
    }
    body = body.push(
        container(label("对话仅在本次运行保留；日志需确认发送后才会上传。", 10.).color(MUTED))
            .width(Fill)
            .center_x(Fill),
    );
    body.into()
}
fn bubble(content: &str, user: bool) -> Element<'_, Message> {
    let text = column![
        bold(if user { "你" } else { "Memoria" }, 11.).color(if user { MUTED } else { GREEN }),
        label(content, 14.).color(WHITE)
    ]
    .spacing(10);
    let bubble = container(text)
        .padding(if user { 16 } else { 4 })
        .max_width(740)
        .style(move |theme| {
            if user {
                theme::surface(theme)
            } else {
                container::Style::default()
            }
        });
    container(bubble)
        .width(Fill)
        .align_x(if user {
            iced::alignment::Horizontal::Right
        } else {
            iced::alignment::Horizontal::Left
        })
        .into()
}
