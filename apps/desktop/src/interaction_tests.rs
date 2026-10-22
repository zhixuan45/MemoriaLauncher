//! Real widget hit-testing: dispatching Navigate directly cannot catch blocked clicks.
use crate::app::{Launcher, Message, Page};
use iced::{Settings, Size};
use iced_test::Simulator;
use mc_core::{
    CatalogProvider, InMemoryTaskService, InstanceService, MockCatalogProvider,
    MockInstanceService, TaskService,
};

fn simulator<'a>(
    app: &'a Launcher,
) -> Simulator<'a, Message, iced::Theme, crate::motion_renderer::Renderer> {
    let mut settings = Settings::default();
    if let Ok(path) = std::env::var("MEMORIA_TEST_FONT") {
        settings.fonts.push(std::fs::read(path).unwrap().into());
        settings.default_font = iced::Font::with_name("Noto Sans CJK SC");
    }
    Simulator::with_size(settings, app.viewport, app.view())
}

async fn populated() -> Launcher {
    let (mut app, _) = Launcher::new();
    app.loading = false;
    app.status = "界面预览 · 实例与任务使用示例数据".into();
    app.instances = MockInstanceService::new()
        .with_sample_data()
        .await
        .list_instances()
        .await
        .unwrap()
        .into_iter()
        .map(Into::into)
        .collect();
    app.tasks = InMemoryTaskService::new()
        .with_sample_data()
        .await
        .list_tasks()
        .await
        .unwrap()
        .into_iter()
        .map(Into::into)
        .collect();
    app.active = "inst-fabric-121".into();
    let versions = MockCatalogProvider::new()
        .list_game_versions()
        .await
        .unwrap();
    let _ = app
        .download
        .update(crate::download::Message::Loaded(Ok(versions)));
    app
}

fn click(app: &mut Launcher, target: &str) -> Vec<Message> {
    let mut ui = simulator(app);
    ui.click(target)
        .unwrap_or_else(|e| panic!("Cannot click {target}: {e}"));
    ui.into_messages().collect()
}

#[tokio::test]
async fn account_controls_display_loaded_accounts_and_dispatch_real_actions() {
    use crate::accounts::{AccountIndex, Message as AccountMessage};
    use mc_core::AuthAccount;
    let mut app = populated().await;
    let _ = app.update(Message::Accounts(AccountMessage::Loaded(Ok(
        AccountIndex {
            accounts: vec![
                AuthAccount {
                    id: "one".into(),
                    username: "ActualPlayer".into(),
                    uuid: "one-uuid".into(),
                },
                AuthAccount {
                    id: "two".into(),
                    username: "SecondPlayer".into(),
                    uuid: "two-uuid".into(),
                },
            ],
            active: Some("one".into()),
        },
    ))));
    assert_eq!(app.accounts.name(), "ActualPlayer");
    assert!(click(&mut app, "ActualPlayer")
        .iter()
        .any(|m| matches!(m, Message::Profile)));
    let _ = app.update(Message::Profile);
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::OPEN_DURATION,
    ));
    assert!(click(&mut app, "使用此账户").iter().any(|m| matches!(m,
        Message::Accounts(AccountMessage::Select(id)) if id == "two")));
    assert!(click(&mut app, "退出登录").iter().any(|m| matches!(m,
        Message::Accounts(AccountMessage::Remove(id)) if id == "one")));
}

#[tokio::test]
async fn launch_without_an_account_opens_account_management() {
    let mut app = populated().await;
    app.accounts.busy = false;
    app.instances.iter_mut().for_each(|i| i.installed = true);
    let _ = app.update(Message::Launch);
    assert_eq!(app.modal, Some(crate::app::Modal::Profile));
    assert!(!app.launching);
    assert!(app.status.contains("登录"));
}

#[tokio::test]
async fn sidebar_clicks_route_from_every_page_at_both_sizes() {
    let mut app = populated().await;
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        app.viewport = size;
        for from in Page::ALL {
            for to in Page::ALL {
                app.page = from;
                let messages = click(&mut app, to.title());
                assert!(
                    messages
                        .iter()
                        .any(|m| matches!(m, Message::Navigate(p) if *p == to)),
                    "Sidebar click did not reach {to:?} from {from:?} at {size:?}: {messages:?}"
                );
                for message in messages {
                    let _ = app.update(message);
                }
                assert_eq!(app.page, to);
            }
        }
    }
}

#[tokio::test]
async fn download_and_settings_tabs_are_clickable_and_state_survives_navigation() {
    let mut app = populated().await;
    app.page = Page::Download;
    for title in [
        "版本文件",
        "配置要求",
        "相关资源",
        "版本介绍",
        "光影包",
        "游戏版本",
    ] {
        let messages = click(&mut app, title);
        assert!(
            messages.iter().any(|m| matches!(m, Message::Download(_))),
            "{title}"
        );
        for message in messages {
            let _ = app.update(message);
        }
    }
    let _ = app.update(Message::Ai(crate::ai::Message::Prompt(
        "汉化这个整合包".into(),
    )));
    let _ = app.update(Message::Navigate(Page::Settings));
    for title in ["环境配置", "常规"] {
        let messages = click(&mut app, title);
        assert!(messages.iter().any(|m| matches!(m, Message::Settings(_))));
        for message in messages {
            let _ = app.update(message);
        }
    }
    let _ = app.update(Message::Navigate(Page::Ai));
    assert_eq!(app.ai.prompt, "汉化这个整合包");
}

#[tokio::test]
async fn ai_request_reaches_the_runtime_and_retains_local_page_animation() {
    let mut app = populated().await;
    let _ = app.update(Message::Navigate(Page::Ai));
    let _ = app.update(Message::Ai(crate::ai::Message::ApiModel(
        "test-model".into(),
    )));
    let _ = app.update(Message::Ai(crate::ai::Message::Prompt("配置建议".into())));
    let request = app.update(Message::Ai(crate::ai::Message::Submit));
    assert_eq!(request.units(), 1);
    assert!(app.ai.busy);
    assert_eq!(
        app.update(Message::Ai(crate::ai::Message::Submit)).units(),
        0
    );
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    let _ = app.update(Message::Ai(crate::ai::Message::ApiReply(Ok(
        "模型建议".into()
    ))));
    assert_eq!(app.page, Page::Ai);
    assert!(app.outgoing_page.is_none());
    assert_eq!(app.ai.reply.as_deref(), Some("模型建议"));
    assert!(!app.ai.busy);
    assert!(app.ai.tasks.is_empty());
}

#[tokio::test]
async fn modal_blocks_sidebar_until_dismissed() {
    let mut app = populated().await;
    let _ = app.update(Message::EditInstance);
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::OPEN_DURATION,
    ));
    let messages = click(&mut app, "游戏下载");
    assert!(!messages.iter().any(|m| matches!(m, Message::Navigate(_))));
    let _ = app.update(Message::Dismiss);
    assert!(
        app.modal.is_some(),
        "Retain the dialog during the exit fade"
    );
    assert!(!click(&mut app, "游戏下载")
        .iter()
        .any(|m| matches!(m, Message::Navigate(_))));
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::CLOSE_DURATION,
    ));
    assert!(app.modal.is_none());
    assert!(click(&mut app, "游戏下载")
        .iter()
        .any(|m| matches!(m, Message::Navigate(Page::Download))));
}

#[tokio::test]
async fn rapid_navigation_and_modal_reopen_settle_without_losing_page_state() {
    let mut app = populated().await;
    let _ = app.update(Message::Ai(crate::ai::Message::Prompt(
        "保留这段输入".into(),
    )));
    for page in [Page::Download, Page::Settings, Page::Ai] {
        let _ = app.update(Message::Navigate(page));
    }
    assert_eq!(app.page, Page::Ai);
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    assert!(!app.animating());
    assert!(app.outgoing_page.is_none());
    assert_eq!(app.ai.prompt, "保留这段输入");
    let _ = app.update(Message::Profile);
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::OPEN_DURATION,
    ));
    let _ = app.update(Message::Dismiss);
    let _ = app.update(Message::Profile);
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::OPEN_DURATION,
    ));
    assert_eq!(app.modal, Some(crate::app::Modal::Profile));
    assert_eq!(app.modal_motion.value(app.animation_time), 1.0);
    assert!(!app.animating());
}

#[test]
fn pop_hit_testing_follows_the_visible_button_and_blocks_exit_input() {
    for interactive in [true, false] {
        let button = iced::widget::button("action")
            .width(200)
            .height(100)
            .on_press(());
        let mut ui = Simulator::with_size(
            Settings::default(),
            Size::new(300., 200.),
            crate::motion::pop(button, 0.5, interactive),
        );
        // At half progress the button occupies x=3..197, y=7.5..104.5.
        ui.point_at((1., 50.));
        ui.simulate(iced_test::simulator::click());
        ui.point_at((100., 103.));
        ui.simulate(iced_test::simulator::click());
        assert_eq!(ui.into_messages().count(), usize::from(interactive));
    }
}

#[test]
fn fading_switch_accepts_one_click_and_reverses_before_the_fade_finishes() {
    use crate::motion::{Toggle, SWITCH_DURATION};
    use std::time::{Duration, Instant};
    let mut toggle = Toggle::new(false);
    let start = Instant::now();
    for (millis, expected) in [(0, true), (20, false), (40, true)] {
        let now = start + Duration::from_millis(millis);
        let mut ui = Simulator::with_size(
            Settings::default(),
            Size::new(150., 40.),
            toggle.view(now, 26., |value| value),
        );
        ui.point_at((20., 13.));
        ui.simulate(iced_test::simulator::click());
        let messages: Vec<_> = ui.into_messages().collect();
        assert_eq!(messages, [expected]);
        toggle.set(messages[0], now);
        assert!(toggle.active(now));
    }
    let now = start + SWITCH_DURATION + Duration::from_millis(40);
    assert!(!toggle.active(now));
    let mut ui = Simulator::with_size(
        Settings::default(),
        Size::new(150., 40.),
        toggle.labeled_view(now, 24., "联网搜索", |value| value),
    );
    ui.click("联网搜索").unwrap();
    assert_eq!(ui.into_messages().collect::<Vec<_>>(), [false]);
}

#[tokio::test]
async fn page_cards_settle_in_50ms_and_settings_reset_animates_switches() {
    use crate::motion::{CARD_DURATION, PAGE_DURATION, SWITCH_DURATION};
    let mut app = populated().await;
    let _ = app.update(Message::Navigate(Page::Settings));
    let start = app.animation_time;
    let _ = app.update(Message::Animate(start + CARD_DURATION));
    assert_eq!(app.card_motion.value(app.animation_time), 1.0);
    assert!(!app.card_motion.active(app.animation_time));
    let _ = app.update(Message::Animate(start + PAGE_DURATION));
    let _ = app.update(Message::Settings(crate::settings::Message::Toggle(0, true)));
    assert!(app.animating());
    let _ = app.update(Message::Animate(app.animation_time + SWITCH_DURATION));
    assert!(!app.animating());
    let _ = app.update(Message::Settings(crate::settings::Message::Reset));
    assert!(app.animating());
    let _ = app.update(Message::Animate(app.animation_time + SWITCH_DURATION));
    assert!(!app.animating());
}

// Optional visual inspection, excluded from ordinary test runs.
#[tokio::test]
#[ignore]
async fn capture_pages() {
    let mut app = populated().await;
    let dir = std::env::var("MEMORIA_UI_CAPTURE_DIR").expect("Set MEMORIA_UI_CAPTURE_DIR");
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        app.viewport = size;
        for page in Page::ALL {
            app.page = page;
            let mut ui = simulator(&app);
            let path = format!("{dir}/{page:?}-{}.png", size.width);
            ui.snapshot(&crate::theme::launcher())
                .unwrap()
                .matches_image(path)
                .unwrap();
        }
    }
}

#[tokio::test]
#[ignore]
async fn capture_motion() {
    let mut app = populated().await;
    let dir = std::env::var("MEMORIA_UI_CAPTURE_DIR").expect("Set MEMORIA_UI_CAPTURE_DIR");
    std::fs::create_dir_all(&dir).unwrap();
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    let _ = app.update(Message::Navigate(Page::Settings));
    let start = app.animation_time;
    for millis in [0, 25, 50, 70, 180] {
        let _ = app.update(Message::Animate(
            start + std::time::Duration::from_millis(millis),
        ));
        simulator(&app)
            .snapshot(&crate::theme::launcher())
            .unwrap()
            .matches_image(format!("{dir}/page-{millis}.png"))
            .unwrap();
    }
    let _ = app.update(Message::Settings(crate::settings::Message::Toggle(0, true)));
    let start = app.animation_time;
    for millis in [0, 40, 120] {
        let _ = app.update(Message::Animate(
            start + std::time::Duration::from_millis(millis),
        ));
        simulator(&app)
            .snapshot(&crate::theme::launcher())
            .unwrap()
            .matches_image(format!("{dir}/switch-{millis}.png"))
            .unwrap();
    }
    let _ = app.update(Message::Profile);
    let start = app.animation_time;
    for millis in [0, 70, 220] {
        let _ = app.update(Message::Animate(
            start + std::time::Duration::from_millis(millis),
        ));
        simulator(&app)
            .snapshot(&crate::theme::launcher())
            .unwrap()
            .matches_image(format!("{dir}/open-{millis}.png"))
            .unwrap();
    }
    let _ = app.update(Message::Dismiss);
    let start = app.animation_time;
    for millis in [0, 60, 150] {
        let _ = app.update(Message::Animate(
            start + std::time::Duration::from_millis(millis),
        ));
        simulator(&app)
            .snapshot(&crate::theme::launcher())
            .unwrap()
            .matches_image(format!("{dir}/close-{millis}.png"))
            .unwrap();
    }
}

#[test]
#[ignore]
fn capture_sliding_switch() {
    use crate::motion::{Toggle, SWITCH_DURATION};
    use iced::widget::{column, container, row, text};
    use std::time::{Duration, Instant};
    let dir = std::env::var("MEMORIA_UI_CAPTURE_DIR").expect("Set MEMORIA_UI_CAPTURE_DIR");
    std::fs::create_dir_all(&dir).unwrap();
    let mut toggle = Toggle::new(false);
    let start = Instant::now();
    toggle.set(true, start);
    let mut stages = column![].spacing(20);
    for millis in [0, 25, 120] {
        stages = stages.push(
            row![
                text(format!("{millis} ms")).width(90),
                toggle.view(start + Duration::from_millis(millis), 26., |v| v),
            ]
            .spacing(12),
        );
    }
    toggle.set(false, start + SWITCH_DURATION);
    stages = stages.push(
        row![
            text("off +25 ms").width(90),
            toggle.view(
                start + SWITCH_DURATION + Duration::from_millis(25),
                26.,
                |v| v
            ),
        ]
        .spacing(12),
    );
    let mut ui = Simulator::with_size(
        Settings::default(),
        Size::new(260., 225.),
        container(stages).padding(20).style(crate::theme::panel),
    );
    ui.snapshot(&crate::theme::launcher())
        .unwrap()
        .matches_image(format!("{dir}/switch-stages.png"))
        .unwrap();
}
