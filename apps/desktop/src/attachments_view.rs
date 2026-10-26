use crate::{
    ai::{Message, State},
    assets::{icon, Icon},
    attachments::{Content, Message as AttachmentMessage},
    motion_renderer::Element,
    theme::{self, bold, label, GREEN, MUTED},
};
use iced::{
    widget::{button, column, container, image, row, scrollable, Space},
    Alignment, Fill,
};

fn dispatch(message: AttachmentMessage) -> Message {
    Message::Attachments(message)
}
pub fn view(state: &State) -> Element<'_, Message> {
    let attachments = &state.attachments;
    let enabled = !state.busy && !state.loading_log;
    if !attachments.menu_open
        && attachments.files.is_empty()
        && !attachments.log_enabled
        && !attachments.loading()
        && attachments.notice.is_empty()
    {
        return Space::new().height(0).into();
    }
    let mut body = column![].spacing(8).width(Fill);
    if attachments.menu_open {
        body = body.push(
            container(
                column![
                    button(
                        row![
                            icon(
                                Icon::Info,
                                if attachments.log_enabled {
                                    GREEN
                                } else {
                                    MUTED
                                },
                                16.
                            ),
                            label(
                                if attachments.log_enabled {
                                    "关闭日志上下文"
                                } else {
                                    "启用日志上下文"
                                },
                                12.
                            )
                        ]
                        .spacing(10)
                    )
                    .padding(10)
                    .width(Fill)
                    .style(theme::quiet)
                    .on_press_maybe(
                        enabled
                            .then_some(dispatch(AttachmentMessage::Logs(!attachments.log_enabled)))
                    ),
                    button(row![icon(Icon::Edit, MUTED, 16.), label("添加图片", 12.)].spacing(10))
                        .padding(10)
                        .width(Fill)
                        .style(theme::quiet)
                        .on_press_maybe(
                            (enabled && !attachments.loading_files)
                                .then_some(dispatch(AttachmentMessage::Pick(true)))
                        ),
                    button(
                        row![icon(Icon::Folder, MUTED, 16.), label("添加文件", 12.)].spacing(10)
                    )
                    .padding(10)
                    .width(Fill)
                    .style(theme::quiet)
                    .on_press_maybe(
                        (enabled && !attachments.loading_files)
                            .then_some(dispatch(AttachmentMessage::Pick(false)))
                    ),
                    label(
                        "支持图片、PDF、UTF-8 文本及配置文件；选择后仍需点击发送。",
                        10.
                    )
                    .color(MUTED),
                ]
                .spacing(3),
            )
            .padding(8)
            .width(Fill)
            .style(theme::panel),
        );
    }
    for files in attachments.files.chunks(2) {
        let mut line = row![].spacing(8);
        for file in files {
            let chip = row![
                button(
                    row![
                        icon(
                            if matches!(&file.content, Content::Image { .. }) {
                                Icon::Edit
                            } else {
                                Icon::Folder
                            },
                            GREEN,
                            15.
                        ),
                        label(file.name.chars().take(25).collect::<String>(), 11.),
                        label(format!("{} KiB", file.size.div_ceil(1024)), 10.).color(MUTED)
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center)
                )
                .padding(8)
                .width(Fill)
                .style(theme::quiet)
                .on_press(dispatch(AttachmentMessage::Preview(file.id))),
                button(icon(Icon::Close, MUTED, 13.))
                    .padding(8)
                    .style(theme::quiet)
                    .on_press_maybe(
                        enabled.then_some(dispatch(AttachmentMessage::Remove(file.id)))
                    ),
            ]
            .align_y(Alignment::Center);
            line = line.push(container(chip).width(Fill).style(theme::surface));
        }
        body = body.push(line);
    }
    if let Some(file) = attachments
        .files
        .iter()
        .find(|file| Some(file.id) == attachments.preview)
    {
        let mut preview = column![bold(&file.name, 12.)].spacing(8);
        match &file.content {
            Content::Text(text) => {
                preview = preview.push(
                    scrollable(label(text.chars().take(4000).collect::<String>(), 11.)).height(100),
                );
            }
            Content::Image { mime, bytes } if *mime == "image/png" && safe_preview(bytes) => {
                preview = preview.push(
                    image(image::Handle::from_bytes(bytes.as_ref().clone()))
                        .height(110)
                        .content_fit(iced::ContentFit::Contain),
                );
            }
            Content::Image { mime, .. } => {
                preview = preview.push(
                    label(format!("图片格式：{mime} · 发送时提供图片内容。"), 11.).color(MUTED),
                );
            }
            Content::Pdf(_) => {
                preview = preview
                    .push(label("PDF 将作为文件发送；所选模型需支持 PDF 输入。", 11.).color(MUTED));
            }
        }
        body = body.push(
            container(preview)
                .padding(12)
                .width(Fill)
                .style(theme::surface),
        );
    }
    if attachments.log_enabled {
        body = body.push(
            row![
                button(
                    row![
                        icon(Icon::Info, GREEN, 14.),
                        label(
                            if attachments.loading_log {
                                "正在读取实例日志…"
                            } else {
                                "日志上下文已启用 · 点击预览"
                            },
                            11.
                        )
                    ]
                    .spacing(7)
                )
                .padding(6)
                .style(theme::quiet)
                .on_press(dispatch(AttachmentMessage::PreviewLog)),
                Space::new().width(Fill),
                button(icon(Icon::Close, MUTED, 13.))
                    .padding(6)
                    .style(theme::quiet)
                    .on_press_maybe(enabled.then_some(dispatch(AttachmentMessage::Logs(false)))),
            ]
            .align_y(Alignment::Center),
        );
    }
    if attachments.show_log {
        if let Some(log) = &attachments.log {
            body = body.push(
                container(
                    column![
                        label(
                            format!(
                                "{} · {} · 已遮蔽 {} 行",
                                log.instance_id, log.path, log.redacted_lines
                            ),
                            10.
                        )
                        .color(MUTED),
                        scrollable(label(log.text.chars().take(4000).collect::<String>(), 11.))
                            .height(90),
                    ]
                    .spacing(6),
                )
                .padding(10)
                .style(theme::surface),
            );
        }
    }
    if attachments.loading_files {
        body = body.push(label("正在读取所选文件…", 11.).color(MUTED));
    }
    if !attachments.notice.is_empty() {
        body = body.push(label(&attachments.notice, 10.).color(MUTED));
    }
    body.into()
}
fn safe_preview(bytes: &[u8]) -> bool {
    let Some(header) = bytes.get(16..24) else {
        return false;
    };
    let width = u32::from_be_bytes(header[0..4].try_into().unwrap()) as u64;
    let height = u32::from_be_bytes(header[4..8].try_into().unwrap()) as u64;
    width > 0 && height > 0 && width * height <= 16 * 1024 * 1024
}
