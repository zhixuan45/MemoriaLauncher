use crate::app::{Launcher, Message, Modal, Page};
use crate::assets::{icon, picture, Icon, ART};
use crate::models::{CHANGELOG, FEATURE_DESCRIPTIONS, FEATURE_TITLES};
use crate::theme::{self, bold, label, GREEN, MUTED, PURPLE, WHITE};
use iced::widget::{button, column, container, mouse_area, row, scrollable, stack, Space};
use iced::{Alignment, Background, Color, ContentFit, Element, Fill};

pub fn icon_button(kind: Icon, message: Message) -> Element<'static, Message> {
    button(icon(kind, WHITE, 16.))
        .padding(8)
        .on_press(message)
        .style(theme::quiet)
        .into()
}

pub fn view(app: &Launcher) -> Element<'_, Message> {
    let page = match app.page {
        Page::Home => app.home(),
        Page::Settings => crate::settings::view(app),
        Page::Download => crate::download::view(&app.download).map(Message::Download),
        Page::Ai => crate::ai::view(&app.ai, app.viewport.width - 194.).map(Message::Ai),
    };
    let main = stack![
        iced::widget::image(ART.wallpaper.clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Fill),
        column![titlebar(app), page].width(Fill).height(Fill)
    ];
    let shell: Element<'_, Message> = container(row![sidebar(app), main].height(Fill))
        .style(|_| container::Style {
            background: Some(Color::from_rgb(0.02, 0.055, 0.07).into()),
            text_color: Some(WHITE),
            ..Default::default()
        })
        .into();
    if let Some(modal) = app.modal {
        stack![shell, modal_view(app, modal)].into()
    } else {
        shell
    }
}

fn titlebar(app: &Launcher) -> Element<'_, Message> {
    let status = if app.edit_layout && app.page == Page::Home {
        "拖动卡片标题移动 · 右下角缩放 · 16px 网格吸附"
    } else {
        &app.status
    };
    let drag = mouse_area(
        container(label(status, 10.).color(MUTED))
            .width(Fill)
            .height(40)
            .align_y(Alignment::Center),
    )
    .on_press(Message::DragWindow)
    .on_double_click(Message::Maximize);
    let mut controls = row![drag].align_y(Alignment::Center).spacing(6);
    if app.page == Page::Home {
        if app.edit_layout || app.layouts.iter().any(|l| !l.visible) {
            controls = controls.push(
                button(label("恢复布局", 10.))
                    .on_press(Message::ResetLayout)
                    .style(theme::quiet),
            );
        }
        controls = controls.push(
            button(
                row![
                    icon(Icon::Edit, if app.edit_layout { GREEN } else { MUTED }, 12.),
                    label(
                        if app.edit_layout {
                            "完成布局"
                        } else {
                            "编辑布局"
                        },
                        10.
                    )
                ]
                .spacing(5)
                .align_y(Alignment::Center),
            )
            .on_press(Message::ToggleLayout)
            .style(theme::quiet)
            .padding([5, 8]),
        );
    }
    controls = controls
        .push(icon_button(Icon::Bell, Message::Changelogs))
        .push(icon_button(Icon::Minimize, Message::Minimize))
        .push(icon_button(Icon::Maximize, Message::Maximize))
        .push(icon_button(Icon::Close, Message::Close));
    container(controls).padding([0, 10]).height(40).into()
}

fn sidebar(app: &Launcher) -> Element<'_, Message> {
    let brand = mouse_area(
        container(
            row![
                picture(&ART.grass, 47.),
                column![
                    bold("MC 启动器", 21.),
                    label("探索 · 创造 · 更多可能", 10.).color(MUTED)
                ]
                .spacing(4)
            ]
            .spacing(7)
            .align_y(Alignment::Center),
        )
        .padding([15, 12]),
    )
    .on_press(Message::DragWindow)
    .on_double_click(Message::Maximize);
    let mut nav = column![brand, Space::new().height(4)].spacing(14);
    for page in Page::ALL {
        let kind = match page {
            Page::Home => Icon::Home,
            Page::Download => Icon::Download,
            Page::Ai => Icon::Spark,
            Page::Settings => Icon::Settings,
        };
        let active = app.page == page;
        let tint = if page == Page::Ai {
            PURPLE
        } else if active {
            GREEN
        } else {
            Color::from_rgb(0.72, 0.79, 0.92)
        };
        let item = button(
            row![icon(kind, tint, 27.), bold(page.title(), 16.)]
                .spacing(20)
                .align_y(Alignment::Center),
        )
        .width(Fill)
        .padding([15, 20])
        .on_press(Message::Navigate(page))
        .style(move |theme, status| {
            if active {
                let mut style = theme::selected(theme, status);
                if page == Page::Ai {
                    style.border.color = PURPLE;
                    style.background = Some(Color::from_rgba(0.36, 0.18, 0.53, 0.60).into());
                }
                style
            } else {
                let mut style = theme::quiet(theme, status);
                if !matches!(status, button::Status::Hovered) {
                    style.background = None;
                    style.border.width = 0.;
                }
                style
            }
        });
        nav = nav.push(container(item).padding([0, 10]));
    }
    nav = nav
        .push(Space::new().height(Fill))
        .push(
            container(
                column![
                    label("让方块的世界", 19.),
                    label("拥有更多可能", 19.),
                    container(Space::new().width(70).height(2)).style(|_| container::Style {
                        background: Some(GREEN.into()),
                        ..Default::default()
                    }),
                ]
                .spacing(7),
            )
            .padding([12, 23]),
        )
        .push(
            container(
                button(
                    row![
                        icon(Icon::Gamepad, GREEN, 18.),
                        column![
                            label(
                                if app.profile == 0 {
                                    "Steve_AI"
                                } else {
                                    "Alex_Builder"
                                },
                                12.
                            ),
                            label("离线示例档案", 9.).color(MUTED)
                        ],
                        Space::new().width(Fill),
                        icon(Icon::Right, MUTED, 12.)
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center),
                )
                .on_press(Message::Profile)
                .style(theme::quiet)
                .padding(10)
                .width(Fill),
            )
            .padding([0, 12]),
        )
        .push(container(label("v0.1.0  /  iced", 10.).color(MUTED)).padding([9, 23]));
    container(stack![
        iced::widget::image(ART.sidebar.clone())
            .width(194)
            .height(Fill)
            .content_fit(ContentFit::Fill),
        container(nav)
            .width(Fill)
            .height(Fill)
            .style(|_| container::Style {
                background: Some(Color::from_rgba(0.01, 0.045, 0.07, 0.35).into()),
                ..Default::default()
            })
    ])
    .width(194)
    .height(Fill)
    .into()
}

fn modal_view(app: &Launcher, modal: Modal) -> Element<'_, Message> {
    let mut content = column![row![
        bold(
            match modal {
                Modal::Instance => "实例信息",
                Modal::Changelog => "更新日志",
                Modal::Feature => "精选内容",
                Modal::Profile => "玩家档案",
            },
            22.
        ),
        Space::new().width(Fill),
        icon_button(Icon::Close, Message::Dismiss)
    ]
    .align_y(Alignment::Center)]
    .spacing(18);
    match modal {
        Modal::Instance => {
            if let Some(i) = app.current() {
                content = content
                    .push(bold(i.instance.name.clone(), 26.))
                    .push(label(
                        format!("{} · {} · {}", i.loader, i.instance.mc_version, i.java),
                        14.,
                    ))
                    .push(
                        label(i.instance.game_dir.to_string_lossy().into_owned(), 13.).color(MUTED),
                    )
                    .push(
                        label("此面板目前展示实例信息；实例配置保存尚未接入。", 13.).color(MUTED),
                    );
            }
        }
        Modal::Changelog => {
            content =
                content.push(label("以下为设计稿中的示例日志，未连接更新服务。", 12.).color(MUTED));
            for (version, date, lines) in CHANGELOG {
                content = content.push(bold(format!("{version}    {date}"), 16.));
                for line in lines {
                    content = content.push(label(format!("· {line}"), 13.).color(MUTED));
                }
            }
        }
        Modal::Feature => {
            content = content
                .push(
                    iced::widget::image(ART.banners[app.feature].clone())
                        .width(Fill)
                        .height(170)
                        .content_fit(ContentFit::Cover),
                )
                .push(bold(FEATURE_TITLES[app.feature], 21.))
                .push(label(FEATURE_DESCRIPTIONS[app.feature], 14.))
                .push(label("设计稿内容预览，资源下载与详情链接尚未接入。", 12.).color(MUTED));
        }
        Modal::Profile => {
            content = content
                .push(label("当前为离线示例档案；微软登录尚未接入。", 13.).color(MUTED))
                .push(bold(
                    if app.profile == 0 {
                        "Steve_AI"
                    } else {
                        "Alex_Builder"
                    },
                    25.,
                ))
                .push(
                    button(label("切换示例档案", 14.))
                        .style(theme::quiet)
                        .on_press(Message::CycleProfile),
                );
        }
    }
    let dialog = container(scrollable(content).direction(theme::scroll_direction()))
        .padding(24)
        .width(580)
        .max_height(640)
        .style(theme::panel);
    stack![
        mouse_area(
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color::from_rgba(0., 0.02, 0.03, 0.78))),
                    ..Default::default()
                })
        )
        .on_press(Message::Dismiss),
        container(dialog).center_x(Fill).center_y(Fill)
    ]
    .into()
}
