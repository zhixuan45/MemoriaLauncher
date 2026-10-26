//! The default homepage: real instance projects, recent conversations and download activity.
use crate::{
    app::{Launcher, Message, Page},
    assets::{icon, picture, Icon},
    motion_renderer::Element,
    theme::{self, bold, label, GREEN, MUTED},
    workspace,
};
use iced::{
    widget::{button, column, container, progress_bar, row, scrollable, Space},
    Alignment, Fill,
};

pub fn view(app: &Launcher, cards: f32) -> Element<'_, Message> {
    let heading = row![
        column![
            bold(
                format!(
                    "欢迎回来，{}",
                    if app.accounts.index.current().is_some() {
                        app.accounts.name()
                    } else {
                        "玩家"
                    }
                ),
                28.
            ),
            label("你的实例、对话和下一次冒险，都在这里。", 13.).color(MUTED)
        ]
        .spacing(9)
        .width(Fill),
        button(row![icon(Icon::Plus, GREEN, 15.), label("开始对话", 12.)].spacing(7))
            .padding(10)
            .style(theme::quiet)
            .on_press(Message::Workspace(workspace::Message::NewChat)),
        button(label("刷新实例", 12.))
            .padding(10)
            .style(theme::quiet)
            .on_press_maybe((!app.loading && !app.launching).then_some(Message::Refresh)),
    ]
    .spacing(8)
    .align_y(Alignment::Center);
    let hero: Element<'_, Message> = if let Some(instance) = app.current() {
        container(
            column![
                label("继续你的世界", 11.).color(MUTED),
                row![
                    picture(crate::sidebar::art(instance), 55.),
                    column![
                        bold(&instance.instance.name, 24.),
                        label(
                            format!(
                                "Minecraft {} · {} · {}",
                                instance.instance.mc_version, instance.loader, instance.java
                            ),
                            12.
                        )
                        .color(MUTED)
                    ]
                    .spacing(8)
                    .width(Fill),
                ]
                .spacing(18)
                .align_y(Alignment::Center),
                row![
                    button(
                        row![
                            icon(Icon::Play, iced::Color::BLACK, 15.),
                            bold(
                                if app.launching {
                                    "游戏运行中…"
                                } else {
                                    "开始游戏"
                                },
                                13.
                            )
                        ]
                        .spacing(8)
                    )
                    .padding([12, 22])
                    .style(theme::action)
                    .on_press_maybe(
                        crate::sidebar::can_launch(app, instance).then_some(Message::Launch)
                    ),
                    button(label("打开项目", 12.))
                        .padding(12)
                        .style(theme::quiet)
                        .on_press(Message::Workspace(workspace::Message::OpenProject(
                            instance.instance.id.clone()
                        ))),
                    button(label("编辑实例", 12.))
                        .padding(12)
                        .style(theme::quiet)
                        .on_press(Message::EditInstance),
                    Space::new().width(Fill),
                    column![
                        instance_stat("游戏时长", instance.play_time.clone()),
                        instance_stat(
                            "最近打开时间",
                            instance
                                .instance
                                .last_played_at
                                .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                                .unwrap_or_else(|| "未记录".into())
                        ),
                        instance_stat("模组数量", instance.mods.clone()),
                    ]
                    .spacing(8)
                    .width(210),
                ]
                .spacing(9)
                .align_y(Alignment::Center),
            ]
            .spacing(20),
        )
        .padding(24)
        .width(Fill)
        .style(theme::panel)
        .into()
    } else {
        container(
            column![
                icon(Icon::Cube, GREEN, 32.),
                bold(
                    if app.loading {
                        "正在扫描本地实例"
                    } else {
                        "从第一个实例开始"
                    },
                    22.
                ),
                label("导入已有的 Minecraft 目录，或下载一个新版本。", 13.).color(MUTED),
                row![
                    button(label("管理实例集", 13.))
                        .padding(12)
                        .style(theme::action)
                        .on_press(Message::Workspace(workspace::Message::ManageProjects)),
                    button(label("浏览游戏版本", 13.))
                        .padding(12)
                        .style(theme::quiet)
                        .on_press(Message::Navigate(Page::Download))
                ]
                .spacing(10),
            ]
            .spacing(14),
        )
        .padding(24)
        .width(Fill)
        .style(theme::panel)
        .into()
    };
    let summary = row![
        metric("本地项目", app.instances.len().to_string()),
        metric(
            "对话记录",
            (app.workspace.others.len() + usize::from(app.ai.has_content())).to_string()
        ),
        metric(
            "下载状态",
            if app.download.installing.is_some() {
                "正在安装"
            } else {
                "空闲"
            }
            .into()
        ),
    ]
    .spacing(12);
    let mut projects = column![row![
        bold("实例项目", 16.),
        Space::new().width(Fill),
        button(label("管理实例集", 11.))
            .padding(7)
            .style(theme::quiet)
            .on_press(Message::Workspace(workspace::Message::ManageProjects))
    ]
    .align_y(Alignment::Center)]
    .spacing(8);
    for instance in &app.instances {
        projects = projects.push(
            container(
                row![
                    picture(crate::sidebar::art(instance), 30.),
                    column![
                        bold(&instance.instance.name, 13.),
                        label(
                            format!("{} · {}", instance.instance.mc_version, instance.loader),
                            10.
                        )
                        .color(MUTED),
                    ]
                    .spacing(5)
                    .width(Fill),
                    button(label("对话", 11.))
                        .padding(9)
                        .style(theme::quiet)
                        .on_press(Message::Workspace(workspace::Message::OpenProject(
                            instance.instance.id.clone()
                        ))),
                    button(icon(Icon::Play, GREEN, 14.))
                        .padding(10)
                        .style(theme::quiet)
                        .on_press_maybe(crate::sidebar::can_launch(app, instance).then_some(
                            Message::LaunchInstance(
                                app.environment.game_root.clone(),
                                instance.instance.id.clone()
                            )
                        )),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            )
            .padding(10)
            .style(theme::surface),
        );
    }
    let mut recent = column![bold("最近对话", 16.)].spacing(12);
    let mut count = 0;
    for info in app.workspace.conversations() {
        if info.id == app.workspace.current.id && !app.ai.has_content() {
            continue;
        }
        recent = recent.push(
            button(
                column![
                    label(&info.title, 12.),
                    label(
                        info.project
                            .as_ref()
                            .map(|p| p.name.as_str())
                            .unwrap_or("独立对话"),
                        10.
                    )
                    .color(MUTED)
                ]
                .spacing(6),
            )
            .padding(10)
            .width(Fill)
            .style(theme::quiet)
            .on_press(Message::Workspace(workspace::Message::OpenChat(info.id))),
        );
        count += 1;
        if count == 4 {
            break;
        }
    }
    if count == 0 {
        recent = recent.push(label("还没有对话。选择一个实例项目开始。", 12.).color(MUTED));
    }
    if let Some(progress) = &app.download.progress {
        recent = recent
            .push(bold("当前下载", 14.))
            .push(label(&app.download.notice, 11.).color(MUTED))
            .push(
                progress_bar(
                    0.0..=progress.total.max(1) as f32,
                    progress.completed as f32,
                )
                .girth(4),
            );
    }
    let panels: Element<'_, Message> = if app.viewport.width < 1180. {
        column![
            projects,
            container(recent)
                .padding(18)
                .width(Fill)
                .style(theme::panel)
        ]
        .spacing(20)
        .into()
    } else {
        row![
            container(projects).width(iced::Length::FillPortion(3)),
            container(recent)
                .padding(18)
                .style(theme::panel)
                .width(iced::Length::FillPortion(2))
        ]
        .spacing(20)
        .into()
    };
    let body = column![
        heading,
        crate::motion::pop(hero, cards, true),
        summary,
        panels
    ]
    .spacing(24);
    container(
        scrollable(body)
            .direction(theme::scroll_direction())
            .height(Fill),
    )
    .padding(28)
    .width(Fill)
    .height(Fill)
    .into()
}
fn metric(title: &str, value: String) -> Element<'_, Message> {
    container(column![label(title, 11.).color(MUTED), bold(value, 22.)].spacing(9))
        .padding(18)
        .width(Fill)
        .style(theme::surface)
        .into()
}

fn instance_stat(title: &str, value: String) -> Element<'_, Message> {
    row![
        label(title, 11.).color(MUTED),
        Space::new().width(Fill),
        label(value, 11.)
    ]
    .spacing(10)
    .into()
}
