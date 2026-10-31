use crate::app::{Launcher, Message, Modal, Page};
use crate::assets::{icon, picture, Icon, ART};
use crate::models::{CHANGELOG, FEATURE_DESCRIPTIONS, FEATURE_TITLES};
use crate::motion_renderer::Element;
use crate::theme::{self, bold, label, GREEN, MUTED, WHITE};
use iced::widget::{button, column, container, mouse_area, row, scrollable, stack, text_input, Space};
use iced::{Alignment, Background, Color, ContentFit, Fill};

pub fn icon_button(kind: Icon, message: Message) -> Element<'static, Message> {
    button(icon(kind, WHITE, 16.))
        .padding(8)
        .on_press(message)
        .style(theme::quiet)
        .into()
}

pub fn view(app: &Launcher) -> Element<'_, Message> {
    let progress = app.page_motion.value(app.animation_time);
    // Keep the destination in a stable tree slot so its input/scroll state survives
    // removal of the outgoing page at the end of the transition.
    let outgoing = app
        .outgoing_page
        .map(|page| crate::motion::fade(page_view(app, page, 1.0), 1.0 - progress, false));
    let page = stack![
        crate::motion::fade(
            page_view(app, app.page, app.card_motion.value(app.animation_time)),
            progress,
            true
        ),
        outgoing.unwrap_or_else(|| Space::new().into())
    ];
    let main = column![workspace_toolbar(app), page,
        container(label(&app.status, 10.).color(MUTED)).padding([4, 24]).height(26)
    ].width(Fill).height(Fill);
    let shell: Element<'_, Message> = container(column![
        titlebar(app), row![crate::sidebar::view(app), main].height(Fill)
    ].height(Fill)).style(|theme: &iced::Theme| container::Style {
        background: Some(Color { a: if app.appearance.saved.wallpaper.is_some() { app.appearance.saved.dim as f32/100. } else {1.}, ..theme.palette().background }.into()), text_color: Some(WHITE), ..Default::default()
    }).into();
    // Preserve the shell tree (focus and scroll positions) when a modal comes/goes.
    let overlay = app
        .modal
        .map(|modal| modal_view(app, modal))
        .unwrap_or_else(|| Space::new().into());
    stack![
        app.appearance.saved.wallpaper.as_ref().map(|path| -> Element<'_,Message> {iced::widget::image(iced::widget::image::Handle::from_path(path)).width(Fill).height(Fill).content_fit(ContentFit::Cover).into()}).unwrap_or_else(||Space::new().width(Fill).height(Fill).into()),
        crate::motion::fade(shell, 1.0, app.modal.is_none()),
        overlay,
        crate::project_menu::overlay(app)
    ]
    .into()
}

fn page_view(app: &Launcher, page: Page, cards: f32) -> Element<'_, Message> {
    match page {
        Page::Home => crate::home::view(app, cards),
        Page::Settings => crate::settings::view(app, cards),
        Page::Download => crate::download::view(&app.download, cards).map(Message::Download),
        Page::Ai => crate::chat_view::view(&app.ai, &app.workspace.current, cards).map(Message::Ai),
    }
}

fn titlebar(app: &Launcher) -> Element<'_, Message> {
    let brand = mouse_area(container(row![
        picture(&ART.grass, 22.), bold("Memoria", 14.),
        label("/", 14.).color(MUTED), label("Minecraft 工作空间", 11.).color(MUTED),
    ].spacing(12).align_y(Alignment::Center)).padding([0, 16]).width(Fill).height(42).align_y(Alignment::Center))
        .on_press(Message::DragWindow).on_double_click(Message::Maximize);
    let mut controls = row![brand].spacing(4).align_y(Alignment::Center);
    if app.crash.is_some() {
        controls = controls.push(button(label("异常退出记录", 10.)).padding(8).style(theme::quiet).on_press(Message::ShowCrash));
    }
    controls = controls.push(icon_button(Icon::Minimize, Message::Minimize))
        .push(icon_button(Icon::Maximize, Message::Maximize)).push(icon_button(Icon::Close, Message::Close));
    container(controls).padding([0, 8]).height(42).width(Fill).style(|theme: &iced::Theme| container::Style {
        background: Some(theme::shade(theme, -0.02).into()), border: iced::Border { color: theme::shade(theme, 0.10), width: 1., ..Default::default() }, ..Default::default()
    }).into()
}

fn workspace_toolbar(app: &Launcher) -> Element<'_, Message> {
    let title = match app.page { Page::Ai => app.workspace.current.project.as_ref().map(|p| p.name.as_str()).unwrap_or("Minecraft"), _ => app.page.title() };
    let mut controls = row![icon(if app.page == Page::Ai { Icon::Cube } else { Icon::Home }, MUTED, 16.),
        label(title, 12.), label("/", 12.).color(MUTED),
        label(if app.page == Page::Ai { &app.workspace.current.title } else { "工作空间" }, 11.).color(MUTED),
        Space::new().width(Fill),
    ].spacing(10).align_y(Alignment::Center);
    if app.page == Page::Home {
        controls = controls.push(button(label(if app.edit_layout { "完成布局" } else { "编辑布局" }, 11.))
            .padding(8).style(theme::quiet).on_press(Message::ToggleLayout));
        if app.edit_layout || app.layouts.iter().any(|l| !l.visible) {
            controls = controls.push(button(label("恢复布局", 11.)).on_press(Message::ResetLayout).style(theme::quiet));
        }
    }
    if app.page == Page::Ai {
        controls = controls.push(button(label("清空会话", 11.)).padding(8).style(theme::quiet)
            .on_press_maybe((!app.ai.busy && !app.ai.loading_log && !app.ai.attachments.loading()).then_some(Message::Ai(crate::ai::Message::Clear))))
            .push(button(label("AI 设置", 11.)).padding(8).style(theme::quiet).on_press(Message::Ai(crate::ai::Message::OpenSettings)));
    }
    controls=controls.push(button(label("日志诊断",11.)).padding(8).style(theme::quiet).on_press(Message::OpenDiagnostics));
    if matches!(app.page,Page::Home|Page::Download) {
        controls=controls.push(button(label("安装加载器",11.)).padding(8).style(theme::quiet)
            .on_press_maybe((!app.instance_writing()&&!app.launching).then_some(Message::OpenLoader)));
    }
    let target = app.page_instance();
    if let Some(instance) = target {
        controls=controls.push(button(label("管理实例",11.)).padding(8).style(theme::quiet).on_press_maybe((!app.instance_writing()).then_some(Message::EditInstance)));
        controls = controls.push(button(row![icon(Icon::Play, GREEN, 13.), label("启动实例", 11.)].spacing(7).align_y(Alignment::Center))
            .padding(8).style(theme::quiet).on_press_maybe(crate::sidebar::can_launch(app, instance).then_some(Message::LaunchInstance(app.environment.game_root.clone(), instance.instance.id.clone()))));
    }
    container(controls).padding([0, 24]).height(48).width(Fill).style(|_| container::Style {
        border: iced::Border { color: theme::LINE, width: 1., ..Default::default() }, ..Default::default()
    }).into()
}

fn modal_view(app: &Launcher, modal: Modal) -> Element<'_, Message> {
    let mut content = column![row![
        bold(
            match modal {
                Modal::Instance => "实例管理",
                Modal::Changelog => "更新日志",
                Modal::Feature => "精选内容",
                Modal::Profile => "账户管理",
                Modal::Crash => "实例异常退出",
                Modal::ResourceInstall => "安装第三方资源",
                Modal::LoaderInstall => "安装加载器",
                Modal::Diagnostics => "本地日志诊断",
                Modal::ChangePlans => "变更计划与恢复",
                Modal::InstanceWizard => "新建实例与导入整合包",
                Modal::PackExport => "实例锁定与整合包导出",
            },
            22.
        ),
        Space::new().width(Fill),
        icon_button(Icon::Close, Message::Dismiss)
    ]
    .align_y(Alignment::Center)]
    .spacing(18);
    match modal {
        Modal::Crash => {
            if let Some(crash) = &app.crash {
                if app.diagnostics.instance==crash.instance_id && crate::local::same_root(&app.diagnostics.root,&crash.game_root)
                    && crash.exit.log_path.file_name().is_some_and(|name|app.diagnostics.selected==format!(".launcher/logs/{}",name.to_string_lossy())) {
                    if let Some(analysis)=&app.diagnostics.analysis {
                        content=content.push(scrollable(crate::diagnostics::summary(analysis)).direction(theme::scroll_direction()).height(120));
                    }
                }
                content = content
                    .push(label(crash.description(), 13.))
                    .push(label("可以启用 AI 分析，帮助定位 Mod 冲突、Java 错误等可能原因。", 13.))
                    .push(label("启用后会读取本次启动日志末尾并遮蔽常见凭据；在 AI 页点击发送后，日志才会发给你配置的服务。日志仍可能包含玩家名和本地路径。", 12.).color(MUTED))
                    .push(row![
                        button(label("启用 AI 分析", 13.))
                            .on_press_maybe((!app.ai.busy && !app.ai.loading_log).then_some(Message::AnalyzeCrash))
                            .style(theme::quiet).padding(10),
                        button(label("暂不分析", 13.)).on_press(Message::Dismiss)
                            .style(theme::quiet).padding(10),
                    ].spacing(10));
                if app.ai.busy || app.ai.loading_log {
                    content =
                        content.push(label("请等待当前 AI 请求或日志读取完成。", 12.).color(MUTED));
                }
            }
        }
        Modal::Instance => {
            content=content.push(crate::instance_settings::view(&app.instance_settings,app.launching||app.resource_install.busy||app.download.installing.is_some()));
        }
        Modal::LoaderInstall => {
            content=content.push(crate::loader_install::view(&app.loader_install,app.launching||app.resource_install.busy||app.instance_settings.busy||app.download.installing.is_some()));
        }
        Modal::Diagnostics => content=content.push(crate::diagnostics::view(&app.diagnostics)),
        Modal::InstanceWizard => content=content.push(crate::instance_wizard::view(&app.instance_wizard,app.launching||app.loading||app.environment_saving||app.download.installing.is_some()||app.resource_install.busy||app.instance_settings.busy||app.loader_install.installing||!crate::local::same_root(&app.instance_wizard.root,&app.environment.game_root))),
        Modal::PackExport => content=content.push(crate::pack_export::view(&app.pack_export,app.launching||app.loading||app.environment_saving||app.download.installing.is_some()||app.resource_install.busy||app.instance_settings.busy||app.loader_install.installing||app.instance_wizard.installing||!crate::local::same_root(&app.pack_export.root,&app.environment.game_root))),
        Modal::ChangePlans => content=content.push(crate::change_plans::view(&app.change_plans,app.launching||app.instance_settings.busy||app.resource_install.busy||app.loader_install.installing||app.loading||app.environment_saving||app.download.installing.is_some()||!crate::local::same_root(&app.change_plans.root,&app.environment.game_root))),
        Modal::ResourceInstall => {
            content=content.push(crate::resource_install::view(&app.resource_install,app.launching||app.instance_settings.busy||app.download.installing.is_some()||!app.environment.game_roots.iter().chain(std::iter::once(&app.environment.game_root)).any(|root|crate::local::same_root(root,&app.resource_install.root))));
        }
        Modal::Changelog => {
            content = content.push(label("当前开发版本已实现的功能。", 12.).color(MUTED));
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
                .push(label("可从侧栏进入对应功能。", 12.).color(MUTED));
        }
        Modal::Profile => {
            if let Some(feedback) = &app.accounts.copied_feedback {
                content = content.push(
                    container(label(feedback, 12.).style(theme::accent_text))
                        .padding([4, 8])
                        .style(theme::panel),
                );
            }

            content = content.push(label(&app.accounts.notice, 13.).color(MUTED));
            if !app.accounts.has_microsoft_app() {
                content = content.push(
                    container(
                        row![
                            column![
                                bold("尚未配置客户端 ID，请先在设置中填写", 14.)
                                    .color(Color::from_rgb(1.0, 0.45, 0.45)),
                                label("请在设置 → 账户登录中填写 Microsoft 应用的客户端 ID 以启用正版登录。", 12.)
                                    .color(MUTED),
                            ]
                            .spacing(4)
                            .width(Fill),
                            button(label("配置 Microsoft 登录", 12.))
                                .padding([6, 12])
                                .style(theme::action)
                                .on_press(Message::LoginSettings),
                        ]
                        .spacing(8)
                        .align_y(Alignment::Center),
                    )
                    .padding(12)
                    .style(theme::panel),
                );
            } else {
                content = content.push(
                    button(label("配置 Microsoft 登录", 13.))
                        .style(theme::quiet)
                        .on_press(Message::LoginSettings),
                );
            }

            if app.accounts.logging_in {
                let is_browser = app.accounts.login_mode == crate::accounts::LoginMode::Browser;
                let mode_selector = row![
                    label("登录模式：", 12.).color(MUTED),
                    button(label("浏览器登录", 12.))
                        .style(if is_browser { theme::selected } else { theme::quiet })
                        .padding([6, 12])
                        .on_press(Message::Accounts(crate::accounts::Message::SwitchMode(
                            crate::accounts::LoginMode::Browser,
                        ))),
                    button(label("设备代码登录", 12.))
                        .style(if !is_browser { theme::selected } else { theme::quiet })
                        .padding([6, 12])
                        .on_press(Message::Accounts(crate::accounts::Message::SwitchMode(
                            crate::accounts::LoginMode::DeviceCode,
                        ))),
                ]
                .spacing(8)
                .align_y(Alignment::Center);
                content = content.push(mode_selector);

                if is_browser {
                    content = content.push(
                        label("若系统未自动弹出浏览器，请点击下方按钮复制授权链接在浏览器中打开：", 12.)
                            .color(MUTED),
                    );
                    if let Some(auth_url) = &app.accounts.auth_url {
                        content = content.push(
                            container(
                                scrollable(label(auth_url, 11.).color(MUTED))
                                    .direction(theme::scroll_direction()),
                            )
                            .padding(10)
                            .max_height(70)
                            .style(theme::panel),
                        );
                        content = content.push(
                            row![
                                button(label("复制授权链接", 12.))
                                    .padding([8, 14])
                                    .style(theme::action)
                                    .on_press(Message::Accounts(crate::accounts::Message::CopyText(
                                        auth_url.clone(),
                                        "授权链接",
                                    ))),
                                button(label("在浏览器中打开", 12.))
                                    .padding([8, 14])
                                    .style(theme::quiet)
                                    .on_press(Message::Accounts(crate::accounts::Message::OpenBrowser(
                                        auth_url.clone(),
                                    ))),
                                button(label("取消登录", 12.))
                                    .padding([8, 14])
                                    .style(theme::quiet)
                                    .on_press(Message::Accounts(crate::accounts::Message::CancelLogin)),
                            ]
                            .spacing(8),
                        );
                    } else {
                        content = content.push(
                            label("正在生成授权链接…", 12.).color(MUTED),
                        );
                        content = content.push(
                            button(label("取消登录", 12.))
                                .padding([6, 14])
                                .style(theme::quiet)
                                .on_press(Message::Accounts(crate::accounts::Message::CancelLogin)),
                        );
                    }
                } else {
                    content = content.push(
                        label("请在任意设备（电脑/手机）的浏览器中访问验证网址并输入验证码：", 12.)
                            .color(MUTED),
                    );
                    if let Some(start) = &app.accounts.device_code {
                        let verify_url = if start.verification_uri.is_empty() {
                            "https://microsoft.com/link"
                        } else {
                            &start.verification_uri
                        };
                        content = content.push(
                            row![
                                bold("验证网址：", 13.),
                                label(verify_url, 13.).style(theme::accent_text),
                            ]
                            .spacing(6)
                            .align_y(Alignment::Center),
                        );
                        content = content.push(
                            container(
                                row![
                                    bold(&start.user_code, 28.)
                                        .color(Color::from_rgb(0.2, 0.85, 1.0)),
                                ]
                                .align_y(Alignment::Center),
                            )
                            .padding([12, 20])
                            .style(theme::panel),
                        );
                        content = content.push(
                            row![
                                button(label("复制验证码", 12.))
                                    .padding([8, 14])
                                    .style(theme::action)
                                    .on_press(Message::Accounts(crate::accounts::Message::CopyText(
                                        start.user_code.clone(),
                                        "设备验证码",
                                    ))),
                                button(label("打开验证网页", 12.))
                                    .padding([8, 14])
                                    .style(theme::quiet)
                                    .on_press(Message::Accounts(crate::accounts::Message::OpenBrowser(
                                        verify_url.to_string(),
                                    ))),
                                button(label("取消登录", 12.))
                                    .padding([8, 14])
                                    .style(theme::quiet)
                                    .on_press(Message::Accounts(crate::accounts::Message::CancelLogin)),
                            ]
                            .spacing(8),
                        );
                        content = content.push(
                            label("正在等待授权完成…", 11.).color(MUTED),
                        );
                    } else {
                        content = content.push(
                            label("正在申请设备验证码…", 13.).color(MUTED),
                        );
                        content = content.push(
                            button(label("取消登录", 12.))
                                .padding([6, 14])
                                .style(theme::quiet)
                                .on_press(Message::Accounts(crate::accounts::Message::CancelLogin)),
                        );
                    }
                }
            } else {
                for account in &app.accounts.index.accounts {
                    let selected = Some(&account.id) == app.accounts.index.active.as_ref();
                    let is_offline = account.is_offline();
                    let tag_text = if is_offline {
                        "离线测试账户"
                    } else {
                        "Microsoft 正版账户"
                    };
                    let tag_color = if is_offline {
                        GREEN
                    } else {
                        Color::from_rgb(0.2, 0.75, 1.)
                    };
                    content = content.push(
                        column![row![
                            iced::widget::image::<iced::widget::image::Handle>(app.accounts.avatar(account)).width(36).height(36)
                                .filter_method(iced::widget::image::FilterMethod::Nearest),
                            column![
                                bold(&account.username, 18.),
                                label(tag_text, 11.).color(tag_color),
                                label(&account.uuid, 10.).color(MUTED),
                            ]
                            .spacing(4)
                            .width(Fill),
                        ].spacing(12).align_y(Alignment::Center),
                        label(app.accounts.skin_description(account), 11.).color(MUTED),
                        row![
                            button(label(
                                if selected {
                                    "当前账户"
                                } else {
                                    "使用此账户"
                                },
                                12.,
                            ))
                            .on_press_maybe((!app.accounts.busy && !app.launching && !selected).then(
                                || Message::Accounts(crate::accounts::Message::Select(
                                    account.id.clone(),
                                )),
                            ))
                            .style(theme::quiet),
                            (|| -> Element<'_, Message> { if is_offline {
                                row![
                                    button(label("设置皮肤", 12.))
                                        .on_press_maybe((!app.accounts.busy && !app.launching).then(|| Message::Accounts(crate::accounts::Message::ChooseSkin(account.id.clone()))))
                                        .style(theme::quiet),
                                    button(label("恢复默认皮肤", 12.))
                                        .on_press_maybe((!app.accounts.busy && !app.launching && app.accounts.custom_skin(&account.id)).then(|| Message::Accounts(crate::accounts::Message::ResetSkin(account.id.clone()))))
                                        .style(theme::quiet),
                                ].spacing(4).into()
                            } else {
                                button(label("刷新皮肤", 12.))
                                    .on_press_maybe((!app.accounts.busy && !app.launching && !app.accounts.skin_loading(&account.id)).then(|| Message::Accounts(crate::accounts::Message::RefreshSkin(account.id.clone()))))
                                    .style(theme::quiet).into()
                            } })(),
                            button(label("退出登录", 12.))
                                .on_press_maybe((!app.accounts.busy && !app.launching).then(|| {
                                    Message::Accounts(crate::accounts::Message::Remove(
                                        account.id.clone(),
                                    ))
                                }))
                                .style(theme::quiet),
                        ]
                        .spacing(8).align_y(Alignment::Center)]
                        .spacing(8).width(Fill),
                    );
                }
                if app.accounts.index.accounts.is_empty() {
                    content = content.push(label("尚未添加账户", 17.));
                }
                if app.accounts.index.accounts.iter().any(|a| a.is_offline()) {
                    content = content.push(label("离线账户自动按 UUID 选择默认皮肤，也可设置本地 PNG；重启后继续使用。", 11.).color(MUTED));
                    content = content.push(label("游戏内皮肤要求 64×64 PNG，支持 1.20.1 / 1.21.1；相同默认皮肤的玩家也会受影响。", 11.).color(MUTED));
                }

                // 离线测试账户添加区域
                if app.accounts.showing_offline_input {
                    content = content.push(
                        container(
                            column![
                                row![
                                    bold("添加离线测试账户", 13.),
                                    label("（单机/本地局域网快速测试，免微软登录）", 11.).color(MUTED),
                                ]
                                .spacing(6)
                                .align_y(Alignment::Center),
                                row![
                                    text_input("输入玩家名称，如 Steve / Alex", &app.accounts.offline_input)
                                        .on_input(|s| Message::Accounts(crate::accounts::Message::OfflineInput(s)))
                                        .on_submit(Message::Accounts(crate::accounts::Message::AddOffline))
                                        .padding([8, 12])
                                        .size(13)
                                        .width(Fill),
                                    button(label("确认添加", 13.))
                                        .padding([8, 16])
                                        .style(theme::action)
                                        .on_press(Message::Accounts(crate::accounts::Message::AddOffline)),
                                    button(label("取消", 13.))
                                        .padding([8, 12])
                                        .style(theme::quiet)
                                        .on_press(Message::Accounts(crate::accounts::Message::ToggleOfflineInput)),
                                ]
                                .spacing(8)
                                .align_y(Alignment::Center),
                            ]
                            .spacing(8),
                        )
                        .padding(14)
                        .style(theme::panel),
                    );
                } else {
                    content = content.push(
                        button(label("+ 添加离线测试账户 (免网络认证)", 13.))
                            .padding([8, 14])
                            .style(theme::quiet)
                            .on_press(Message::Accounts(crate::accounts::Message::ToggleOfflineInput)),
                    );
                }

                let is_browser = app.accounts.login_mode == crate::accounts::LoginMode::Browser;
                let mode_selector = row![
                    label("微软登录模式：", 12.).color(MUTED),
                    button(label("浏览器登录 (推荐)", 12.))
                        .style(if is_browser { theme::selected } else { theme::quiet })
                        .padding([6, 12])
                        .on_press(Message::Accounts(crate::accounts::Message::SwitchMode(
                            crate::accounts::LoginMode::Browser,
                        ))),
                    button(label("设备代码登录", 12.))
                        .style(if !is_browser { theme::selected } else { theme::quiet })
                        .padding([6, 12])
                        .on_press(Message::Accounts(crate::accounts::Message::SwitchMode(
                            crate::accounts::LoginMode::DeviceCode,
                        ))),
                ]
                .spacing(8)
                .align_y(Alignment::Center);
                content = content.push(mode_selector);

                content = content.push(
                    button(label(
                        if app.accounts.busy {
                            "正在处理账户…"
                        } else {
                            "添加 Microsoft 账户"
                        },
                        14.,
                    ))
                    .padding(12)
                    .style(theme::action)
                    .on_press_maybe(
                        (!app.accounts.busy && !app.launching).then(|| {
                            if app.accounts.has_microsoft_app() {
                                Message::Accounts(crate::accounts::Message::Login)
                            } else {
                                Message::LoginSettings
                            }
                        }),
                    ),
                );
            }
        }
    }
    let dialog = container(scrollable(content).direction(theme::scroll_direction()))
        .padding(24)
        .width(if matches!(modal,Modal::Instance|Modal::ResourceInstall|Modal::LoaderInstall|Modal::Diagnostics|Modal::ChangePlans|Modal::InstanceWizard|Modal::PackExport){760}else{580})
        .max_height((app.viewport.height-60.).min(640.))
        .style(theme::panel);
    let progress = app.modal_motion.value(app.animation_time);
    let interactive = app.modal_motion.target() == 1.0;
    iced::widget::opaque(stack![
        mouse_area(
            container(Space::new())
                .width(Fill)
                .height(Fill)
                .style(move |_| container::Style {
                    background: Some(Background::Color(Color::from_rgba(
                        0.,
                        0.02,
                        0.03,
                        0.78 * progress
                    ))),
                    ..Default::default()
                })
        )
        .on_press(Message::Dismiss),
        container(iced::widget::opaque(crate::motion::pop(
            dialog,
            progress,
            interactive
        )))
        .center_x(Fill)
        .center_y(Fill)
    ])
    .into()
}
