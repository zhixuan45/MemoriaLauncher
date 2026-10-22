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
    app.download_settings =
        crate::download_settings::State::new(mc_core::DownloadSettings::default());
    app.download = crate::download::State::default();
    app.ai_settings = crate::ai_settings::State::new(Default::default());
    app.ai = crate::ai::State::default();
    app.game_settings.memory = crate::runtime_memory::MemorySnapshot {
        total_mb: 32768,
        available_mb: 24576,
    };
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

fn finished_game(success: bool) -> crate::local::GameExit {
    crate::local::GameExit {
        instance_id: "versions/crashed-instance".into(),
        game_root: "unused-fixture".into(),
        exit: mc_core::ProcessExit {
            success,
            code: Some(if success { 0 } else { 1 }),
            log_path: "unused-fixture/versions/crashed-instance/.launcher/logs/run.log".into(),
            log_error: None,
        },
    }
}

#[tokio::test]
async fn abnormal_exit_offers_opt_in_analysis_of_the_exited_instance() {
    for size in [Size::new(1440., 900.), Size::new(1024., 700.)] {
        let mut app = populated().await;
        app.viewport = size;
        app.launching = true;
        let _ = app.update(Message::GameFinished(Ok(finished_game(false))));
        assert!(!app.launching);
        assert_eq!(app.modal, Some(crate::app::Modal::Crash));
        assert!(app.ai.crash_log.is_none());
        assert!(!app.ai.busy);
        let _ = app.update(Message::Animate(
            app.animation_time + crate::motion::OPEN_DURATION,
        ));
        let messages = click(&mut app, "启用 AI 分析");
        assert!(messages
            .iter()
            .any(|message| matches!(message, Message::AnalyzeCrash)));
        for message in messages {
            let _ = app.update(message);
        }
        assert_eq!(app.page, Page::Ai);
        assert!(app.ai.loading_log);
        assert!(
            !app.ai.busy,
            "Reading a local log must not send an API request"
        );
        assert_ne!(app.crash.as_ref().unwrap().instance_id, app.active);
    }
}

#[tokio::test]
async fn normal_exit_and_launch_failure_do_not_offer_crash_analysis() {
    let mut app = populated().await;
    let _ = app.update(Message::GameFinished(Ok(finished_game(true))));
    assert!(app.crash.is_none());
    assert!(app.modal.is_none());
    let _ = app.update(Message::GameFinished(Err("Java missing".into())));
    assert!(app.crash.is_none());
    assert!(app.modal.is_none());
    assert!(app.status.contains("Java missing"));
}

#[tokio::test]
async fn declining_crash_analysis_keeps_logs_unread() {
    let mut app = populated().await;
    let _ = app.update(Message::GameFinished(Ok(finished_game(false))));
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::OPEN_DURATION,
    ));
    for message in click(&mut app, "暂不分析") {
        let _ = app.update(message);
    }
    assert!(!app.ai.loading_log);
    assert!(app.ai.crash_log.is_none());
    assert!(!app.ai.busy);
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
async fn microsoft_settings_are_reachable_from_account_dialog_and_clickable_at_both_sizes() {
    use crate::accounts::{AccountIndex, Message as AccountMessage};
    let mut app = populated().await;
    let _ = app.update(Message::Accounts(AccountMessage::Loaded(Ok(
        AccountIndex::default(),
    ))));
    let _ = app.update(Message::Profile);
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::OPEN_DURATION,
    ));
    for message in click(&mut app, "配置 Microsoft 登录") {
        let _ = app.update(message);
    }
    assert_eq!(app.page, Page::Settings);
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    let _ = app.update(Message::Accounts(AccountMessage::ClientId(
        "12345678-1234-1234-1234-123456789abc".into(),
    )));
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        app.viewport = size;
        assert!(click(&mut app, "账户登录")
            .iter()
            .any(|m| matches!(m, Message::Settings(_))));
        assert!(click(&mut app, "保存登录配置")
            .iter()
            .any(|m| matches!(m, Message::Accounts(AccountMessage::SaveLoginSettings))));
        assert!(click(&mut app, "管理账户")
            .iter()
            .any(|m| matches!(m, Message::Profile)));
    }
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
    app.ai.connection.config.model = "test-model".into();
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
async fn ai_settings_navigation_save_controls_and_application() {
    use crate::ai_settings::{Connection, Message as SettingMessage};
    let mut app = populated().await;
    let _ = app.update(Message::Navigate(Page::Ai));
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    for message in click(&mut app, "AI 设置") {
        let _ = app.update(message);
    }
    assert_eq!(app.page, Page::Settings);
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    for size in [Size::new(1024., 680.), Size::new(1280., 768.)] {
        app.viewport = size;
        assert!(click(&mut app, "保存并应用")
            .iter()
            .any(|m| matches!(m, Message::AiSettings(SettingMessage::Save))));
        assert!(click(&mut app, "测试连接")
            .iter()
            .any(|m| matches!(m, Message::AiSettings(SettingMessage::Check))));
    }
    let _ = app.update(Message::AiSettings(SettingMessage::Model(
        "unsaved-model".into(),
    )));
    assert!(app.ai.connection.config.model.is_empty());
    let mut connection = Connection::default();
    connection.config.model = "saved-model".into();
    let _ = app.update(Message::AiSettings(SettingMessage::Saved(Ok(connection))));
    assert_eq!(app.ai.connection.config.model, "saved-model");
    let _ = app.update(Message::AiSettings(SettingMessage::Saved(Err(
        "disk full".into()
    ))));
    assert_eq!(app.ai.connection.config.model, "saved-model");
}

#[tokio::test]
#[ignore]
async fn capture_ai_settings() {
    let mut app = populated().await;
    let dir = std::env::var("MEMORIA_UI_CAPTURE_DIR").expect("Set MEMORIA_UI_CAPTURE_DIR");
    let _ = app.update(Message::Ai(crate::ai::Message::OpenSettings));
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    for size in [Size::new(1024., 680.), Size::new(1280., 768.)] {
        app.viewport = size;
        simulator(&app)
            .snapshot(&crate::theme::launcher())
            .unwrap()
            .matches_image(format!("{dir}/ai-settings-{}.png", size.width))
            .unwrap();
    }
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

#[tokio::test]
async fn game_settings_controls_work_at_minimum_window_size_and_drafts_survive_navigation() {
    use crate::game_settings::Message as GameMessage;
    let mut app = populated().await;
    app.viewport = Size::new(1024., 680.);
    let _ = app.update(Message::Navigate(Page::Settings));
    for message in click(&mut app, "游戏设置") {
        let _ = app.update(message);
    }
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    let second = app
        .environment
        .game_root
        .with_file_name(".minecraft-modpacks");
    let _ = app.update(Message::GameSettings(GameMessage::Prepared(Ok((
        second.clone(),
        false,
    )))));
    for message in click(&mut app, "设为当前") {
        assert!(matches!(
            message,
            Message::GameSettings(GameMessage::Select(1))
        ));
        let _ = app.update(message);
    }
    assert_eq!(app.game_settings.draft.game_root, second);
    assert_ne!(
        app.environment.game_root, second,
        "draft must not affect launching before save"
    );
    let _ = app.update(Message::GameSettings(GameMessage::MemoryPercent(25)));
    let _ = app.update(Message::Navigate(Page::Home));
    let _ = app.update(Message::Navigate(Page::Settings));
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    assert_eq!(app.game_settings.max_memory, "8192");
    let messages = click(&mut app, "保存并应用");
    assert!(messages
        .iter()
        .any(|message| matches!(message, Message::GameSettings(GameMessage::Save))));

    let mut ui = simulator(&app);
    ui.point_at((800., 500.));
    ui.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
        delta: iced::mouse::ScrollDelta::Pixels { x: 0., y: -350. },
    })]);
    ui.click("检测 Java")
        .expect("Java controls must be accessible after scrolling");
    assert!(ui
        .into_messages()
        .any(|message| matches!(message, Message::GameSettings(GameMessage::CheckJava))));

    let previous = app.environment.game_root.clone();
    let _ = app.update(Message::GameSettings(GameMessage::Saved(Err(
        "read-only".into()
    ))));
    assert_eq!(app.environment.game_root, previous);
    let mut saved = app.game_settings.draft.clone();
    saved.max_memory_mb = 8192;
    let task = app.update(Message::GameSettings(GameMessage::Saved(Ok(saved))));
    assert_eq!(task.units(), 1, "applying a collection must rescan it");
    assert!(app.loading);
    assert!(app.active.is_empty());
    assert!(
        app.instances.is_empty(),
        "version IDs from a previous collection must not survive"
    );
    assert_eq!(app.environment.game_root, second);
    assert_eq!(app.environment.launch_options().max_memory_mb, 8192);
}

#[tokio::test]
#[ignore]
async fn capture_game_settings() {
    use crate::game_settings::Message as GameMessage;
    let mut app = populated().await;
    let dir = std::env::var("MEMORIA_UI_CAPTURE_DIR").expect("Set MEMORIA_UI_CAPTURE_DIR");
    let _ = app.update(Message::Navigate(Page::Settings));
    let _ = app.update(Message::Settings(crate::settings::Message::Tab(1)));
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    let second = std::path::PathBuf::from("D:/Minecraft/Modpacks/.minecraft");
    let _ = app.update(Message::GameSettings(GameMessage::Prepared(Ok((
        second, false,
    )))));
    for size in [Size::new(1024., 680.), Size::new(1280., 768.)] {
        app.viewport = size;
        simulator(&app)
            .snapshot(&crate::theme::launcher())
            .unwrap()
            .matches_image(format!("{dir}/game-settings-{}.png", size.width))
            .unwrap();
        let mut ui = simulator(&app);
        ui.point_at((800., 500.));
        ui.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: iced::mouse::ScrollDelta::Pixels { x: 0., y: -1200. },
        })]);
        ui.snapshot(&crate::theme::launcher())
            .unwrap()
            .matches_image(format!("{dir}/game-settings-bottom-{}.png", size.width))
            .unwrap();
    }
}

#[tokio::test]
async fn memory_radio_buttons_and_ratio_slider_control_the_saved_launch_policy() {
    use crate::game_settings::Message as GameMessage;
    use crate::runtime_memory::MemoryMode;
    let mut app = populated().await;
    let _ = app.update(Message::Navigate(Page::Settings));
    let _ = app.update(Message::Settings(crate::settings::Message::Tab(1)));
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    let _ = app.update(Message::GameSettings(GameMessage::MemoryMode(
        MemoryMode::Automatic,
    )));
    for size in [Size::new(1024., 680.), Size::new(1280., 768.)] {
        app.viewport = size;
        let _ = app.update(Message::GameSettings(GameMessage::MemoryPercent(25)));
        let _ = app.update(Message::GameSettings(GameMessage::MemoryMode(
            MemoryMode::Automatic,
        )));
        let mut ui = simulator(&app);
        ui.point_at((800., 500.));
        ui.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: iced::mouse::ScrollDelta::Pixels { x: 0., y: -2000. },
        })]);
        ui.click(iced_test::selector::id("memory-custom")).unwrap();
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(messages.iter().any(|message| matches!(
            message,
            Message::GameSettings(GameMessage::MemoryMode(MemoryMode::Custom))
        )));
        for message in messages {
            let _ = app.update(message);
        }
        let mut ui = simulator(&app);
        ui.point_at((800., 500.));
        ui.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: iced::mouse::ScrollDelta::Pixels { x: 0., y: -2000. },
        })]);
        let slider = ui
            .find(iced_test::selector::id("memory-ratio-slider"))
            .unwrap()
            .visible_bounds()
            .unwrap();
        ui.point_at((slider.x + slider.width * 0.6, slider.center_y()));
        ui.simulate(iced_test::simulator::click());
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(messages.iter().any(|message| matches!(message, Message::GameSettings(GameMessage::MemoryPercent(value)) if (50..=58).contains(value))), "{size:?}: slider={slider:?}, messages={messages:?}");
        for message in messages {
            let _ = app.update(message);
        }
        assert_eq!(app.game_settings.draft.memory_mode, MemoryMode::Custom);
        assert!(app.game_settings.draft.memory_percent >= 50);
        assert_ne!(
            app.environment.memory_percent,
            app.game_settings.draft.memory_percent
        );
    }
}

#[tokio::test]
#[ignore]
async fn capture_java_and_memory_controls() {
    use crate::game_settings::Message as GameMessage;
    use crate::runtime_memory::MemoryMode;
    let mut app = populated().await;
    let dir = std::env::var("MEMORIA_UI_CAPTURE_DIR").expect("Set MEMORIA_UI_CAPTURE_DIR");
    let mut runtimes = Vec::new();
    for (index, (major, version)) in [
        (26, "26.0.2"),
        (21, "21.0.9"),
        (21, "21.0.8"),
        (17, "17.0.17"),
        (17, "17.0.16"),
        (8, "1.8.0_472"),
        (8, "1.8.0_462"),
        (17, "17.0.20"),
    ]
    .into_iter()
    .enumerate()
    {
        runtimes.push(mc_core::JavaRuntime {
            executable: format!("C:/Program Files/Zulu/zulu-{major}-{index}/bin/java.exe").into(),
            major_version: major,
            version: version.into(),
            arch: "amd64".into(),
            os_version: "10.0".into(),
        });
    }
    app.game_settings.java = runtimes[1].executable.to_string_lossy().into_owned();
    app.game_settings.java_choices.clear();
    let _ = app.update(Message::GameSettings(GameMessage::JavaScanned(Ok(
        crate::java_environment::JavaDiscovery {
            runtimes,
            failures: Vec::new(),
        },
    ))));
    let _ = app.update(Message::Navigate(Page::Settings));
    let _ = app.update(Message::Settings(crate::settings::Message::Tab(1)));
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    for size in [Size::new(1024., 680.), Size::new(1280., 768.)] {
        app.viewport = size;
        for (name, mode) in [
            ("auto", MemoryMode::Automatic),
            ("custom", MemoryMode::Custom),
        ] {
            let _ = app.update(Message::GameSettings(GameMessage::MemoryMode(mode)));
            if mode == MemoryMode::Custom {
                let _ = app.update(Message::GameSettings(GameMessage::MemoryPercent(30)));
            }
            let mut ui = simulator(&app);
            ui.point_at((800., 500.));
            ui.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Pixels { x: 0., y: -2000. },
            })]);
            ui.snapshot(&crate::theme::launcher())
                .unwrap()
                .matches_image(format!("{dir}/memory-{name}-{}.png", size.width))
                .unwrap();
        }
        let mut ui = simulator(&app);
        ui.point_at((800., 500.));
        ui.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: iced::mouse::ScrollDelta::Pixels { x: 0., y: -300. },
        })]);
        ui.click(iced_test::selector::id("game-java-picker"))
            .unwrap();
        ui.snapshot(&crate::theme::launcher())
            .unwrap()
            .matches_image(format!("{dir}/java-dropdown-{}.png", size.width))
            .unwrap();
    }
}

#[tokio::test]
async fn download_preferences_apply_to_the_client_and_controls_remain_accessible() {
    use crate::download_settings::Message as SettingMessage;
    use mc_core::{DownloadSettings, DownloadSource};
    let mut app = populated().await;
    let _ = app.update(Message::Navigate(Page::Settings));
    for message in click(&mut app, "下载安装") {
        let _ = app.update(message);
    }
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    for size in [Size::new(1024., 680.), Size::new(1280., 768.)] {
        app.viewport = size;
        for message in click(&mut app, "16") {
            assert!(matches!(
                message,
                Message::DownloadSettings(SettingMessage::Parallel(_))
            ));
            let _ = app.update(message);
        }
        assert_eq!(app.download_settings.parallel, "16");
        assert_eq!(
            app.download
                .client
                .as_ref()
                .unwrap()
                .settings()
                .parallel_downloads,
            8
        );
        assert!(click(&mut app, "保存并应用")
            .iter()
            .any(|message| matches!(message, Message::DownloadSettings(SettingMessage::Save))));
    }
    let _ = app.update(Message::Navigate(Page::Home));
    let _ = app.update(Message::Navigate(Page::Settings));
    assert_eq!(app.download_settings.parallel, "16");
    let config = DownloadSettings {
        source: DownloadSource::Bmclapi,
        parallel_downloads: 16,
        ..Default::default()
    };
    let _ = app.update(Message::DownloadSettings(SettingMessage::Saved(Ok(
        config.clone()
    ))));
    assert_eq!(app.download.client.as_ref().unwrap().settings(), &config);
    assert_eq!(app.download_settings.saved, config);
    let _ = app.update(Message::DownloadSettings(SettingMessage::Saved(Err(
        "disk full".into(),
    ))));
    assert_eq!(
        app.download
            .client
            .as_ref()
            .unwrap()
            .settings()
            .parallel_downloads,
        16
    );
    assert_eq!(
        app.update(Message::Navigate(Page::Download)).units(),
        1,
        "saving a source must cause the catalogue to load through the new client"
    );
}

#[tokio::test]
async fn vanilla_install_uses_current_collection_and_rescans_only_after_success() {
    use crate::download::Message as DownloadMessage;
    let mut app = populated().await;
    let version = app.download.versions[0].id.clone();
    assert_eq!(
        app.update(Message::Download(DownloadMessage::Install(version.clone())))
            .units(),
        1
    );
    assert_eq!(app.download.installing, Some(version.clone()));
    assert_eq!(
        app.update(Message::Download(DownloadMessage::Install(version.clone())))
            .units(),
        0
    );
    let before = app.game_settings.java.clone();
    let _ = app.update(Message::GameSettings(crate::game_settings::Message::Java(
        "different-java".into(),
    )));
    assert_eq!(
        app.game_settings.java, before,
        "collection settings are locked during installation"
    );
    let _ = app.update(Message::Download(DownloadMessage::InstallProgress(
        mc_core::DownloadProgress {
            completed: 2,
            total: 4,
            reused: 1,
        },
    )));
    let _ = app.update(Message::Download(DownloadMessage::Installed(
        version.clone(),
        app.environment.game_root.clone(),
        Err("hash mismatch".into()),
    )));
    assert!(app.download.installing.is_none());
    assert!(!app.loading);
    assert!(app.download.notice.contains("hash mismatch"));
    let task = app.update(Message::Download(DownloadMessage::Installed(
        version,
        app.environment.game_root.clone(),
        Ok(()),
    )));
    assert_eq!(task.units(), 1);
    assert!(app.loading);
}

#[tokio::test]
#[ignore]
async fn capture_download_settings() {
    use crate::download_settings::Message as SettingMessage;
    let mut app = populated().await;
    let dir = std::env::var("MEMORIA_UI_CAPTURE_DIR").expect("Set MEMORIA_UI_CAPTURE_DIR");
    let _ = app.update(Message::Navigate(Page::Settings));
    let _ = app.update(Message::Settings(crate::settings::Message::Tab(2)));
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    for size in [Size::new(1024., 680.), Size::new(1280., 768.)] {
        app.viewport = size;
        let _ = app.update(Message::DownloadSettings(SettingMessage::Source(
            mc_core::DownloadSource::Bmclapi,
        )));
        simulator(&app)
            .snapshot(&crate::theme::launcher())
            .unwrap()
            .matches_image(format!("{dir}/download-settings-{}.png", size.width))
            .unwrap();
        let _ = app.update(Message::DownloadSettings(SettingMessage::Source(
            mc_core::DownloadSource::Custom,
        )));
        let mut ui = simulator(&app);
        ui.point_at((800., 500.));
        ui.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: iced::mouse::ScrollDelta::Pixels { x: 0., y: -1200. },
        })]);
        ui.snapshot(&crate::theme::launcher())
            .unwrap()
            .matches_image(format!("{dir}/download-settings-custom-{}.png", size.width))
            .unwrap();
    }
}

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
