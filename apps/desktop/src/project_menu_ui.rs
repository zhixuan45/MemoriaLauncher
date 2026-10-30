use super::{dispatch, Message};
use crate::{
    app::Launcher,
    assets::{icon, Icon},
    motion_renderer::Element,
    theme::{self, bold, label, MUTED, WHITE},
};
use iced::{
    widget::{
        button, column, container, mouse_area, pin, row, rule, scrollable, stack, text_input, Space,
    },
    Alignment, Color, Fill,
};

fn item<'a>(
    kind: Icon,
    title: impl Into<String>,
    message: Message,
    enabled: bool,
) -> Element<'a, crate::app::Message> {
    let id = format!("project-menu-{message:?}");
    container(
        button(
            row![
                icon(kind, if enabled { WHITE } else { MUTED }, 16.),
                label(title.into(), 13.)
            ]
            .spacing(11)
            .align_y(Alignment::Center),
        )
        .width(Fill)
        .padding([10, 12])
        .style(theme::quiet)
        .on_press_maybe(enabled.then_some(dispatch(message))),
    )
    .id(id)
    .into()
}
pub fn overlay(app: &Launcher) -> Element<'_, crate::app::Message> {
    let Some(menu) = &app.project_menu.menu else {
        return Space::new().into();
    };
    let project = &menu.project;
    let preferences = &app.project_menu.preferences;
    let blocked = app.instance_writing()
        || app.launching
        || app.loading
        || app.environment_saving
        || app.download.installing.is_some();
    let mut content = column![
        container(
            column![
                bold(&project.name, 12.),
                label(
                    format!("Minecraft {} · {}", project.version, project.loader),
                    10.
                )
                .color(MUTED)
            ]
            .spacing(4)
        )
        .padding([8, 12]),
        rule::horizontal(1),
        item(
            Icon::Pin,
            if preferences.pinned(&project.root, &project.instance_id) {
                "取消置顶"
            } else {
                "置顶"
            },
            Message::Pin,
            true
        ),
        item(Icon::Settings, "编辑实例", Message::Edit, !blocked),
        rule::horizontal(1),
        item(
            Icon::List,
            if menu.sections {
                "收起分区 ‹"
            } else {
                "分区 ›"
            },
            Message::Sections,
            true
        ),
    ]
    .spacing(2);
    if menu.sections {
        let current = preferences.section(&project.root, &project.instance_id);
        content = content.push(item(
            Icon::Folder,
            if current.is_empty() {
                "✓ 项目"
            } else {
                "项目"
            },
            Message::Section(String::new()),
            true,
        ));
        for section in preferences.sections() {
            content = content.push(item(
                Icon::Folder,
                format!("{}{}", if current == section { "✓ " } else { "" }, section),
                Message::Section(section.clone()),
                true,
            ));
        }
        content = content.push(
            container(
                column![
                    text_input("新分区名称…", &menu.section_name)
                        .id("project-section-name")
                        .padding(9)
                        .size(12)
                        .style(theme::input)
                        .on_input(|v| dispatch(Message::SectionName(v)))
                        .on_submit(dispatch(Message::CreateSection)),
                    button(label("新建并移入", 12.))
                        .style(theme::quiet)
                        .padding(8)
                        .on_press_maybe(
                            (!menu.section_name.trim().is_empty())
                                .then_some(dispatch(Message::CreateSection))
                        ),
                ]
                .spacing(4),
            )
            .padding([4, 10]),
        );
    }
    content = content
        .push(item(
            Icon::Folder,
            "在资源管理器中打开",
            Message::OpenDirectory,
            true,
        ))
        .push(rule::horizontal(1))
        .push(item(Icon::Archive, "归档项目对话", Message::Archive, true))
        .push(rule::horizontal(1))
        .push(item(Icon::Close, "移除项目", Message::Hide, true));
    let width = 250f32.min(app.viewport.width - 24.);
    let x = menu
        .position
        .x
        .clamp(12., (app.viewport.width - width - 12.).max(12.));
    let max_height = (app.viewport.height - 110.).max(160.);
    let estimated = if menu.sections {
        440. + preferences.sections().len() as f32 * 38.
    } else {
        336.
    };
    let y = menu
        .position
        .y
        .min(app.viewport.height - estimated.min(max_height) - 16.)
        .max(54.);
    let panel = container(scrollable(content).direction(theme::scroll_direction()))
        .width(width)
        .max_height(max_height)
        .padding(7)
        .style(|theme: &iced::Theme| container::Style {
            background: Some(theme::shade(theme, 0.075).into()),
            border: iced::Border {
                color: theme::shade(theme, 0.16),
                width: 1.,
                radius: 16.into(),
            },
            shadow: iced::Shadow {
                color: Color::from_rgba(0., 0., 0., 0.35),
                offset: iced::Vector::new(0., 7.),
                blur_radius: 24.,
            },
            ..Default::default()
        });
    stack![
        mouse_area(Space::new().width(Fill).height(Fill))
            .on_press(dispatch(Message::Close))
            .on_right_press(dispatch(Message::Close)),
        pin(mouse_area(panel).on_press(dispatch(Message::Ignore)))
            .x(x)
            .y(y),
    ]
    .width(Fill)
    .height(Fill)
    .into()
}
