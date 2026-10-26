use crate::app::{Card, Launcher, Message};
use crate::assets::{icon, picture, Icon, ART};
use crate::models::{CHANGELOG, FEATURE_DESCRIPTIONS, FEATURE_TITLES};
use crate::motion_renderer::Element;
use crate::shell::icon_button;
use crate::theme::{self, bold, label, GREEN, MUTED, PURPLE, WHITE};
use iced::widget::{
    button, column, container, mouse_area, pin, progress_bar, row, scrollable, stack, Space,
};
use iced::{Alignment, Border, Color, ContentFit, Fill, Length, Size};

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

pub fn bounds(card: Card, size: Size) -> Rect {
    let sx = size.width / 1086.;
    let sy = size.height / 728.;
    let (x, y, width, height) = match card {
        Card::Launch => (16., 126., 420., 180.),
        Card::Stats => (16., 310., 420., 92.),
        Card::Instances => (452., 126., 326., 286.),
        Card::Featured => (16., 424., 762., 270.),
        Card::Changelog => (794., 14., 276., 322.),
        Card::Queue => (794., 350., 276., 344.),
    };
    Rect {
        x: x * sx,
        y: y * sy,
        width: width * sx,
        height: height * sy,
    }
}

pub fn view(app: &Launcher, cards: f32) -> Element<'_, Message> {
    if !app.edit_layout { return crate::dashboard::view(app, cards); }
    let size = Size::new(
        (app.viewport.width - crate::sidebar::width(app.viewport.width)).max(1.),
        (app.viewport.height - 116.).max(1.),
    );
    let mut layers = vec![Space::new().width(Fill).height(Fill).into()];
    for card in Card::ALL {
        let layout = app.layouts[card.index()];
        if !layout.visible {
            continue;
        }
        let rect = bounds(card, size);
        let min: f32 = if matches!(card, Card::Launch | Card::Featured) {
            280.
        } else {
            200.
        };
        let width = (rect.width + layout.extra.x).clamp(min.min(size.width), size.width);
        let height = (rect.height + layout.extra.y).clamp(90., size.height);
        let x = (rect.x + layout.offset.x).clamp(0., (size.width - width).max(0.));
        let y = (rect.y + layout.offset.y).clamp(0., (size.height - height).max(0.));
        let content = match card {
            Card::Launch => launch(app),
            Card::Stats => stats(app),
            Card::Instances => instances(app),
            Card::Featured => featured(app),
            Card::Changelog => changelog(),
            Card::Queue => queue(app),
        };
        let body: Element<'_, Message> = if app.edit_layout {
            let mut header = row![mouse_area(
                container(
                    row![icon(Icon::Edit, GREEN, 12.), label(card_name(card), 10.)].spacing(6)
                )
                .width(Fill)
                .height(24)
                .align_y(Alignment::Center)
            )
            .on_press(Message::BeginDrag(card, false))
            .interaction(iced::mouse::Interaction::Grab),]
            .align_y(Alignment::Center);
            if !card.protected() {
                header = header.push(
                    button(icon(Icon::Close, MUTED, 12.))
                        .padding(4)
                        .on_press(Message::HideCard(card))
                        .style(theme::quiet),
                );
            }
            let frame = container(column![header, container(content).height(Fill)].spacing(2))
                .padding(4)
                .width(width)
                .height(height)
                .style(|_| container::Style {
                    border: Border {
                        color: GREEN,
                        width: 1.,
                        radius: 9.into(),
                    },
                    ..Default::default()
                });
            stack![
                frame,
                container(
                    mouse_area(
                        container(icon(Icon::Sliders, GREEN, 13.))
                            .padding(4)
                            .style(theme::surface)
                    )
                    .on_press(Message::BeginDrag(card, true))
                    .interaction(iced::mouse::Interaction::ResizingDiagonallyDown)
                )
                .align_right(Fill)
                .align_bottom(Fill)
            ]
            .into()
        } else {
            container(content)
                .width(width)
                .height(height)
                .clip(true)
                .into()
        };
        layers.push(pin(crate::motion::pop(body, cards, true)).x(x).y(y).into());
    }
    stack(layers).width(Fill).height(Fill).into()
}

fn card_name(card: Card) -> &'static str {
    match card {
        Card::Launch => "当前实例",
        Card::Stats => "实例统计",
        Card::Instances => "实例列表",
        Card::Featured => "精选内容",
        Card::Changelog => "更新日志",
        Card::Queue => "下载队列",
    }
}

fn header<'a>(
    kind: Icon,
    tint: Color,
    title: impl Into<String>,
    action: Option<(&'a str, Message)>,
) -> Element<'a, Message> {
    let mut line = row![
        icon(kind, tint, 20.),
        bold(title.into(), 14.),
        Space::new().width(Fill)
    ]
    .spacing(9)
    .align_y(Alignment::Center);
    if let Some((title, msg)) = action {
        line = line.push(
            button(
                row![
                    label(title, 10.).color(Color::from_rgb(0.24, 0.64, 0.80)),
                    icon(Icon::Right, MUTED, 10.)
                ]
                .spacing(3)
                .align_y(Alignment::Center),
            )
            .style(|_, status| button::Style {
                background: if matches!(status, button::Status::Hovered) {
                    Some(Color::from_rgba(0.2, 0.4, 0.5, 0.3).into())
                } else {
                    None
                },
                ..Default::default()
            })
            .padding(2)
            .on_press(msg),
        );
    }
    line.into()
}

fn launch(app: &Launcher) -> Element<'_, Message> {
    let Some(current) = app.current() else {
        return container(label(
            if app.loading {
                "正在读取游戏实例…"
            } else {
                "暂无游戏实例"
            },
            20.,
        ))
        .center_y(Fill)
        .into();
    };
    let image = match current.icon {
        1 => &ART.anvil,
        2 => &ART.modpack,
        _ => &ART.grass,
    };
    let details = column![
        row![
            icon(Icon::Gamepad, MUTED, 13.),
            label("当前游戏实例", 11.).color(MUTED)
        ]
        .spacing(6),
        bold(current.instance.name.clone(), 30.),
        row![
            theme::pill(
                if current.installed {
                    "已安装"
                } else {
                    "未安装"
                },
                current.installed
            ),
            theme::pill(current.loader.clone(), false),
            theme::pill(current.instance.mc_version.clone(), false),
            theme::pill(current.java.clone(), false),
        ]
        .spacing(5)
    ]
    .spacing(7);
    let info = row![
        container(picture(image, 78.))
            .padding(8)
            .style(theme::surface),
        details
    ]
    .spacing(14)
    .align_y(Alignment::Center);
    let start = button(
        row![
            icon(Icon::Play, Color::from_rgb(0., 0.1, 0.08), 21.),
            bold(
                if app.launching {
                    "游戏运行中…"
                } else if app.download.installing.is_some() {
                    "正在安装游戏…"
                } else {
                    "开始游戏"
                },
                18.
            )
            .color(Color::from_rgb(0., 0.1, 0.08))
        ]
        .spacing(10)
        .align_y(Alignment::Center),
    )
    .padding([12, 20])
    .width(Length::FillPortion(6))
    .on_press_maybe(
        (!app.launching
            && !app.loading
            && !app.environment_saving
            && app.download.installing.is_none())
        .then_some(Message::Launch),
    )
    .style(theme::action);
    let edit = button(
        row![icon(Icon::Sliders, WHITE, 21.), bold("编辑实例", 15.)]
            .spacing(9)
            .align_y(Alignment::Center),
    )
    .padding([13, 17])
    .width(Length::FillPortion(5))
    .on_press(Message::EditInstance)
    .style(theme::quiet);
    container(column![info, row![start, edit].spacing(12)].spacing(16))
        .padding(0)
        .into()
}

fn stats(app: &Launcher) -> Element<'_, Message> {
    let Some(current) = app.current() else {
        return Space::new().into();
    };
    fn item(kind: Icon, title: &str, value: String) -> Element<'_, Message> {
        row![
            icon(kind, Color::from_rgb(0.72, 0.79, 0.92), 23.),
            column![label(title.to_owned(), 10.).color(MUTED), label(value, 12.)].spacing(5)
        ]
        .spacing(11)
        .align_y(Alignment::Center)
        .into()
    }
    container(
        row![
            container(item(Icon::Gamepad, "游戏时长", current.play_time.clone()))
                .width(Length::FillPortion(3)),
            container(item(Icon::Cube, "Mod 数量", current.mods.clone()))
                .width(Length::FillPortion(3)),
            container(item(
                Icon::Folder,
                "游戏目录",
                current
                    .instance
                    .game_dir
                    .to_string_lossy()
                    .replace("\\Instances\\", "\\Instances\\\n")
            ))
            .width(Length::FillPortion(4)),
        ]
        .spacing(8),
    )
    .padding(12)
    .width(Fill)
    .height(Fill)
    .style(theme::surface)
    .into()
}

fn instances(app: &Launcher) -> Element<'_, Message> {
    let mut list = column![].spacing(6);
    if app.instances.is_empty() && !app.loading {
        list = list.push(
            label(
                "未发现本地游戏，请在设置 → 游戏设置中选择实例集文件夹。",
                12.,
            )
            .color(MUTED),
        );
    }
    for item in &app.instances {
        let active = app.active == item.instance.id;
        let image = match item.icon {
            1 => &ART.anvil,
            2 => &ART.modpack,
            _ => &ART.grass,
        };
        let subtitle = if app.viewport.width < 1160. {
            format!("{} · {}", item.loader, item.instance.mc_version)
        } else if item.icon == 2 {
            format!("自定义整合包 · {}", item.instance.mc_version)
        } else {
            format!(
                "{} · {} · {}",
                item.loader, item.instance.mc_version, item.java
            )
        };
        let entry = row![
            picture(image, 38.),
            column![
                bold(item.instance.name.clone(), 13.),
                label(subtitle, 10.).color(MUTED)
            ]
            .spacing(4)
            .width(Fill),
            theme::pill(
                if active {
                    "当前使用"
                } else if item.installed {
                    "已安装"
                } else {
                    "缺少 JAR"
                },
                active || item.installed
            ),
        ]
        .spacing(9)
        .align_y(Alignment::Center);
        let select = button(entry)
            .padding([7, 9])
            .width(Fill)
            .on_press(Message::SelectInstance(item.instance.id.clone()))
            .style(move |t, s| {
                if active {
                    theme::selected(t, s)
                } else {
                    theme::quiet(t, s)
                }
            });
        if app.edit_layout {
            list = list.push(
                row![
                    select,
                    button(icon(Icon::Left, MUTED, 10.))
                        .padding(4)
                        .style(theme::quiet)
                        .on_press(Message::MoveInstance(item.instance.id.clone(), -1)),
                    button(icon(Icon::Right, MUTED, 10.))
                        .padding(4)
                        .style(theme::quiet)
                        .on_press(Message::MoveInstance(item.instance.id.clone(), 1)),
                ]
                .spacing(2),
            );
        } else {
            list = list.push(select);
        }
    }
    container(
        column![
            header(
                Icon::Cube,
                Color::from_rgb(0.72, 0.79, 0.92),
                "实例列表",
                Some((
                    if app.loading { "读取中" } else { "刷新" },
                    Message::Refresh
                ))
            ),
            scrollable(list)
                .direction(theme::scroll_direction())
                .height(Fill),
        ]
        .spacing(10),
    )
    .padding(12)
    .width(Fill)
    .height(Fill)
    .style(theme::panel)
    .into()
}

fn changelog() -> Element<'static, Message> {
    let mut entries = column![].spacing(14);
    for (index, (version, date, lines)) in CHANGELOG.into_iter().enumerate() {
        let tint = if index == 2 {
            Color::from_rgb(1., 0.6, 0.2)
        } else {
            GREEN
        };
        let mut entry = column![row![
            container(Space::new().width(6).height(6)).style(move |_| container::Style {
                background: Some(tint.into()),
                border: iced::border::rounded(4),
                ..Default::default()
            }),
            label(version, 12.),
            label(date, 10.).color(MUTED),
        ]
        .spacing(12)
        .align_y(Alignment::Center)]
        .spacing(5);
        for line in lines {
            entry = entry
                .push(container(label(format!("· {line}"), 11.).color(MUTED)).padding([0, 20]));
        }
        entries = entries.push(entry);
    }
    container(
        column![
            header(
                Icon::Info,
                GREEN,
                "更新日志",
                Some(("查看全部", Message::Changelogs))
            ),
            scrollable(entries)
                .direction(theme::scroll_direction())
                .height(Fill),
        ]
        .spacing(16),
    )
    .padding(14)
    .width(Fill)
    .height(Fill)
    .style(theme::panel)
    .into()
}

fn queue(app: &Launcher) -> Element<'_, Message> {
    let mut entries = column![].spacing(22);
    if app.tasks.is_empty() && app.download.installing.is_none() {
        entries = entries.push(label("暂无下载任务", 12.).color(MUTED));
    }
    if let Some(id) = &app.download.installing {
        let percentage = app
            .download
            .progress
            .as_ref()
            .map(|progress| progress.completed as f32 / progress.total.max(1) as f32 * 100.)
            .unwrap_or(0.);
        entries = entries.push(
            column![
                bold(format!("Minecraft {id}"), 12.),
                progress_bar(0.0..=100., percentage).girth(5),
                label(&app.download.notice, 10.).color(MUTED),
            ]
            .spacing(8),
        );
    }
    for task in &app.tasks {
        let progress = task.task.progress.percentage.clamp(0., 100.);
        let title = label(task.title.clone(), 11.);
        let track =
            progress_bar(0.0..=100., progress)
                .girth(5)
                .style(move |_| progress_bar::Style {
                    background: Color::from_rgb(0.12, 0.23, 0.28).into(),
                    bar: if task.paused {
                        MUTED.into()
                    } else {
                        GREEN.into()
                    },
                    border: iced::border::rounded(4),
                });
        let detail = column![
            label(
                if task.paused {
                    "已暂停".into()
                } else {
                    task.detail.clone()
                },
                8.
            )
            .color(MUTED),
            label(
                if task.paused {
                    String::new()
                } else {
                    task.speed.clone()
                },
                8.
            )
            .color(MUTED),
        ]
        .spacing(3);
        entries = entries.push(
            row![
                picture(if task.sodium { &ART.sodium } else { &ART.grass }, 32.),
                column![title, track, detail].spacing(8).width(Fill),
                icon_button(
                    if task.paused { Icon::Play } else { Icon::Pause },
                    Message::ToggleTask(task.task.id.clone())
                ),
            ]
            .spacing(10)
            .align_y(Alignment::Center),
        );
    }
    container(
        column![
            header(
                Icon::Download,
                GREEN,
                format!(
                    "下载队列 ({})",
                    app.tasks.len() + usize::from(app.download.installing.is_some())
                ),
                (!app.tasks.is_empty()).then_some((
                    if app.tasks.iter().any(|t| !t.paused) {
                        "全部暂停"
                    } else {
                        "全部继续"
                    },
                    Message::ToggleAllTasks
                ))
            ),
            scrollable(entries)
                .direction(theme::scroll_direction())
                .height(Fill),
        ]
        .spacing(22),
    )
    .padding(14)
    .width(Fill)
    .height(Fill)
    .style(theme::panel)
    .into()
}

fn featured(app: &Launcher) -> Element<'_, Message> {
    let feature = app.feature;
    let next = (feature + 1) % 4;
    let compact = app.viewport.height < 740.;
    let preview = stack![
        iced::widget::image(ART.banners[feature].clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover),
        container(
            column![
                theme::pill("使用指南", false),
                bold(FEATURE_TITLES[feature], if compact { 19. } else { 23. }),
                label(
                    FEATURE_DESCRIPTIONS[feature],
                    if compact { 10. } else { 12. }
                ),
                button(
                    row![
                        bold("了解详情", 12.).color(Color::BLACK),
                        icon(Icon::Right, Color::BLACK, 13.)
                    ]
                    .spacing(8)
                    .align_y(Alignment::Center)
                )
                .padding([6, 16])
                .on_press(Message::FeatureDetails)
                .style(theme::action),
            ]
            .spacing(if compact { 6. } else { 10. })
        )
        .padding(if compact { [8, 16] } else { [20, 24] })
        .align_y(Alignment::Center)
        .height(Fill),
    ];
    let secondary = stack![
        iced::widget::image(ART.banners[next].clone())
            .width(Fill)
            .height(Fill)
            .content_fit(ContentFit::Cover),
        container(
            column![
                theme::pill("功能入口", false),
                bold(FEATURE_TITLES[next], 15.),
                label(FEATURE_DESCRIPTIONS[next], 11.).color(WHITE),
                Space::new().height(Fill),
                icon_button(Icon::Right, Message::Feature(1)),
            ]
            .spacing(9)
        )
        .padding(16)
        .height(Fill),
    ];
    let mut dots = row![].spacing(7).align_y(Alignment::Center);
    for index in 0..4 {
        let dot = container(Space::new().width(7).height(7)).style(move |_| container::Style {
            background: Some(if index == feature {
                GREEN.into()
            } else {
                Color::from_rgb(0.15, 0.25, 0.29).into()
            }),
            border: iced::border::rounded(4),
            ..Default::default()
        });
        dots = dots.push(
            button(dot)
                .padding(1)
                .on_press(Message::Feature(index as i32 - feature as i32))
                .style(|_, _| button::Style {
                    background: None,
                    ..Default::default()
                }),
        );
    }
    container(
        column![
            row![
                header(Icon::Spark, PURPLE, "精选内容", None),
                label(format!("{} / 4", feature + 1), 11.),
                icon_button(Icon::Left, Message::Feature(-1)),
                icon_button(Icon::Right, Message::Feature(1)),
            ]
            .spacing(8)
            .align_y(Alignment::Center),
            row![
                container(preview)
                    .clip(true)
                    .height(Fill)
                    .width(Length::FillPortion(4))
                    .style(theme::surface),
                container(secondary)
                    .clip(true)
                    .height(Fill)
                    .width(Length::FillPortion(1))
                    .style(theme::surface),
            ]
            .spacing(12)
            .height(Fill),
            container(dots).center_x(Fill),
        ]
        .spacing(6),
    )
    .padding(12)
    .width(Fill)
    .height(Fill)
    .style(theme::panel)
    .into()
}
