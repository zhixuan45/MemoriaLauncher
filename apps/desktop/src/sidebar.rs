use crate::{
    app::{Launcher, Message, Page},
    assets::{icon, picture, Icon, ART},
    models::InstanceEntry,
    motion_renderer::Element,
    theme::{self, bold, label, GREEN, MUTED},
    workspace,
};
use iced::{
    widget::{button, column, container, row, scrollable, text_input, tooltip, Space},
    Alignment, Fill,
};

pub fn width(viewport: f32) -> f32 {
    if viewport < 1200. {
        232.
    } else {
        268.
    }
}
pub fn can_launch(app: &Launcher, instance: &InstanceEntry) -> bool {
    instance.installed
        && !app.launching
        && !app.loading
        && !app.environment_saving
        && app.download.installing.is_none()
        && !app.instance_writing()
}
pub fn art(instance: &InstanceEntry) -> &'static iced::widget::image::Handle {
    match instance.icon {
        1 => &ART.anvil,
        2 => &ART.modpack,
        3 => &ART.sodium,
        _ => &ART.grass,
    }
}
fn nav<'a>(kind: Icon, title: &'a str, active: bool, message: Message) -> Element<'a, Message> {
    button(
        row![
            icon(kind, if active { GREEN } else { MUTED }, 17.),
            label(title, 13.)
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    )
    .width(Fill)
    .padding([11, 12])
    .style(if active {
        theme::selected
    } else {
        theme::quiet
    })
    .on_press(message)
    .into()
}
fn dispatch(message: workspace::Message) -> Message {
    Message::Workspace(message)
}

pub fn view(app: &Launcher) -> Element<'_, Message> {
    let mut top = column![
        nav(
            Icon::Home,
            "主页",
            app.page == Page::Home,
            Message::Navigate(Page::Home)
        ),
        nav(
            Icon::Plus,
            "新对话",
            false,
            dispatch(workspace::Message::NewChat)
        ),
        nav(
            Icon::Search,
            "搜索对话",
            app.workspace.search_open,
            dispatch(workspace::Message::Search)
        ),
    ]
    .spacing(3);
    if app.workspace.search_open {
        top = top.push(
            text_input("搜索标题或项目…", &app.workspace.query)
                .id("conversation-search")
                .on_input(|v| dispatch(workspace::Message::Query(v)))
                .padding(10)
                .size(12)
                .style(theme::input),
        );
    }
    let mut list = column![row![
        label("项目", 11.).color(MUTED),
        Space::new().width(Fill),
        tooltip(
            button(icon(Icon::Plus, MUTED, 14.))
                .padding(5)
                .style(theme::quiet)
                .on_press(dispatch(workspace::Message::ManageProjects)),
            "管理实例集",
            tooltip::Position::Right
        ),
    ]
    .align_y(Alignment::Center)]
    .spacing(9);
    if app.instances.is_empty() {
        list = list.push(
            container(
                label(
                    if app.loading {
                        "正在扫描实例…"
                    } else {
                        "还没有实例项目"
                    },
                    12.,
                )
                .color(MUTED),
            )
            .padding([8, 10]),
        );
    }
    for instance in &app.instances {
        let id = &instance.instance.id;
        let expanded = app.workspace.expanded(&app.environment.game_root, id);
        let selected = if app.page == Page::Ai {
            app.workspace.current.project.as_ref()
                .is_some_and(|p|p.matches(&app.environment.game_root,id))
        } else { app.active == *id };
        let title = instance.instance.name.chars().take(17).collect::<String>();
        let select = button(
            row![
                picture(art(instance), 25.),
                column![
                    label(title, 12.),
                    label(
                        format!("{} · {}", instance.instance.mc_version, instance.loader),
                        10.
                    )
                    .color(MUTED),
                ]
                .spacing(4)
                .width(Fill)
            ]
            .spacing(9)
            .align_y(Alignment::Center),
        )
        .padding([8, 4])
        .width(Fill)
        .style(theme::quiet)
        .on_press(dispatch(workspace::Message::OpenProject(id.clone())));
        let launch = container(tooltip(
            button(icon(
                Icon::Play,
                if can_launch(app, instance) {
                    GREEN
                } else {
                    MUTED
                },
                14.,
            ))
            .padding([10, 7])
            .style(theme::quiet)
            .on_press_maybe(can_launch(app, instance).then_some(Message::LaunchInstance(
                app.environment.game_root.clone(),
                id.clone(),
            ))),
            label(format!("启动 {}", instance.instance.name), 12.),
            tooltip::Position::Right,
        ))
        .id(format!("project-launch-{id}"));
        let entry = row![
            button(icon(
                if expanded {
                    Icon::ChevronDown
                } else {
                    Icon::Right
                },
                MUTED,
                10.
            ))
            .padding([12, 3])
            .style(theme::quiet)
            .on_press(dispatch(workspace::Message::ToggleProject(id.clone()))),
            container(select).id(format!("project-{id}")),
            launch,
        ]
        .spacing(1)
        .align_y(Alignment::Center);
        list = list.push(
            container(entry)
                .padding([0, 3])
                .width(Fill)
                .style(move |palette: &iced::Theme| {
                    if selected {
                        container::Style {
                            background: theme::selected(palette,iced::widget::button::Status::Active).background,
                            border: iced::border::rounded(7),
                            ..Default::default()
                        }
                    } else {
                        container::Style::default()
                    }
                }),
        );
        if expanded || !app.workspace.query.is_empty() {
            for info in app.workspace.conversations().filter(|c| {
                c.project
                    .as_ref()
                    .is_some_and(|p| p.matches(&app.environment.game_root, id))
            }) {
                if app.workspace.visible(info)
                    && (info.id != app.workspace.current.id || app.ai.has_content())
                {
                    list =
                        list.push(container(conversation(app, info, "project")).padding([0, 20]));
                }
            }
        }
    }
    list = list.push(
        container(
            label(
                if app.workspace.query.is_empty() {
                    "最近对话"
                } else {
                    "搜索结果"
                },
                11.,
            )
            .color(MUTED),
        )
        .padding([16, 0]),
    );
    let mut count = 0;
    for info in app
        .workspace
        .conversations()
        .filter(|c| app.workspace.visible(c))
    {
        if info.id == app.workspace.current.id && !app.ai.has_content() {
            continue;
        }
        list = list.push(conversation(app, info, "recent"));
        count += 1;
        if count == 12 {
            break;
        }
    }
    if count == 0 {
        list = list.push(
            label(
                if app.workspace.query.is_empty() {
                    "开始一段新的对话"
                } else {
                    "没有匹配的对话"
                },
                11.,
            )
            .color(MUTED),
        );
    }
    let bottom = column![
        nav(
            Icon::Download,
            "游戏下载",
            app.page == Page::Download,
            Message::Navigate(Page::Download)
        ),
        nav(
            Icon::Settings,
            "设置",
            app.page == Page::Settings,
            Message::Navigate(Page::Settings)
        ),
        container(
            button(
                row![
                    icon(Icon::Gamepad, GREEN, 24.),
                    column![
                        bold(app.accounts.name(), 12.),
                        label(app.accounts.description(), 9.).color(MUTED)
                    ]
                    .spacing(4)
                    .width(Fill),
                    icon(Icon::Right, MUTED, 12.)
                ]
                .spacing(10)
                .align_y(Alignment::Center)
            )
            .padding(12)
            .width(Fill)
            .style(theme::quiet)
            .on_press(Message::Profile)
        )
        .padding([10, 0]),
    ]
    .spacing(3);
    container(
        column![
            top,
            scrollable(list)
                .direction(theme::scroll_direction())
                .height(Fill),
            bottom
        ]
        .spacing(18),
    )
    .padding([16, 12])
    .width(width(app.viewport.width))
    .height(Fill)
    .style(|palette: &iced::Theme| container::Style {
        background: Some(theme::shade(palette, -0.02).into()),
        border: iced::Border {
            color: theme::LINE,
            width: 1.,
            ..Default::default()
        },
        ..Default::default()
    })
    .into()
}
fn conversation<'a>(
    app: &'a Launcher,
    info: &'a workspace::ConversationInfo,
    scope: &'static str,
) -> Element<'a, Message> {
    container(
        button(
            row![
                icon(Icon::Chat, MUTED, 14.),
                label(info.title.chars().take(22).collect::<String>(), 11.)
            ]
            .spacing(9)
            .align_y(Alignment::Center),
        )
        .padding([9, 8])
        .width(Fill)
        .style(
            if app.page == Page::Ai && app.workspace.current.id == info.id {
                theme::selected
            } else {
                theme::quiet
            },
        )
        .on_press(dispatch(workspace::Message::OpenChat(info.id))),
    )
    .id(format!("conversation-{scope}-{}", info.id))
    .into()
}
