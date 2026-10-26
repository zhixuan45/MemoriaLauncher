//! Optional render captures split from the widget interaction checks.
use super::*;

#[tokio::test]
#[ignore = "requires live Modrinth access and a screenshot output directory"]
async fn capture_chat_and_resource_catalogue() {
    use iced::futures::StreamExt;
    async fn complete(app: &mut Launcher, task: iced::Task<Message>) {
        let mut stream = iced_test::runtime::task::into_stream(task).unwrap();
        while let Some(action) = stream.next().await {
            if let iced_test::runtime::Action::Output(message) = action {
                let _ = app.update(message);
            }
        }
    }
    let mut app = populated().await;
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    let dir = std::env::var("MEMORIA_UI_CAPTURE_DIR").expect("Set MEMORIA_UI_CAPTURE_DIR");
    std::fs::create_dir_all(&dir).unwrap();
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        app.viewport = size;
        app.page = Page::Ai;
        simulator(&app)
            .snapshot(&crate::theme::launcher())
            .unwrap()
            .matches_image(format!("{dir}/chat-empty-{}.png", size.width))
            .unwrap();
    }
    app.ai.connection.config.model = "fixture".into();
    let _ = app.update(Message::Ai(crate::ai::Message::Prompt(
        "我在玩 Minecraft 1.21.1，有哪些优化建议？".into(),
    )));
    let _ = app.update(Message::Ai(crate::ai::Message::Submit));
    let _ = app.update(Message::Ai(crate::ai::Message::ApiReply(Ok("可以先检查 Java 版本、分配的内存和渲染距离。请告诉我你的显卡、总内存以及当前使用的模组加载器，我会按你的环境给出具体建议。".into()))));
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        app.viewport = size;
        simulator(&app)
            .snapshot(&crate::theme::launcher())
            .unwrap()
            .matches_image(format!("{dir}/chat-reply-{}.png", size.width))
            .unwrap();
    }
    app.page = Page::Download;
    let task = app.update(Message::Download(crate::download::Message::Category(1)));
    complete(&mut app, task).await;
    let id = app
        .download
        .resources
        .page
        .as_ref()
        .expect("live result")
        .hits[0]
        .project_id
        .clone();
    let task = app.update(Message::Download(crate::download::Message::Resources(
        crate::resources::Message::Select(id),
    )));
    complete(&mut app, task).await;
    assert!(!app.download.resources.versions.is_empty());
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        app.viewport = size;
        simulator(&app)
            .snapshot(&crate::theme::launcher())
            .unwrap()
            .matches_image(format!("{dir}/resources-{}.png", size.width))
            .unwrap();
    }
}

#[tokio::test]
#[ignore = "writes page inventory screenshots; set MEMORIA_UI_CAPTURE_DIR"]
async fn capture_pages() {
    let mut app = populated().await;
    let dir = std::env::var("MEMORIA_UI_CAPTURE_DIR").expect("Set MEMORIA_UI_CAPTURE_DIR");
    std::fs::create_dir_all(&dir).unwrap();
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
        app.page = Page::Settings;
        for (index, title) in [
            "常规",
            "游戏设置",
            "下载安装",
            "环境配置",
            "AI 设置",
            "界面外观",
            "高级设置",
            "账户登录",
        ]
        .into_iter()
        .enumerate()
        {
            let messages = click(&mut app, title);
            assert!(
                messages.iter().any(|message| matches!(
                    message,
                    Message::Settings(crate::settings::Message::Tab(tab)) if *tab == index
                )),
                "{title} at {size:?}"
            );
            for message in messages {
                let _ = app.update(message);
            }
            let _ = app.update(Message::Animate(
                app.animation_time + crate::motion::PAGE_DURATION,
            ));
            simulator(&app)
                .snapshot(&crate::theme::launcher())
                .unwrap()
                .matches_image(format!("{dir}/settings-{index}-{}.png", size.width))
                .unwrap();
        }
        app.page = Page::Download;
        for (index, title) in crate::download::CATEGORIES.into_iter().enumerate() {
            let messages = click(&mut app, title);
            assert!(messages.iter().any(|message| matches!(
                message,
                Message::Download(crate::download::Message::Category(category)) if *category == index
            )), "{title} at {size:?}");
            for message in messages {
                // Catalogue requests stay unpolled: this capture is an offline UI audit.
                let _ = app.update(message);
            }
            if index != 0 {
                let generation = app.download.resources.test_generation();
                let _ = app.update(Message::Download(crate::download::Message::Resources(
                    crate::resources::Message::Loaded(
                        generation,
                        Ok(mc_core::ResourcePage {
                            hits: vec![],
                            total_hits: 0,
                            offset: 0,
                            limit: 20,
                        }),
                    ),
                )));
            }
            simulator(&app)
                .snapshot(&crate::theme::launcher())
                .unwrap()
                .matches_image(format!("{dir}/download-{index}-{}.png", size.width))
                .unwrap();
        }
        for message in click(&mut app, "游戏版本") {
            let _ = app.update(message);
        }
        for (index, title) in ["版本介绍", "版本文件", "配置要求", "相关资源"]
            .into_iter()
            .enumerate()
        {
            let messages = click(&mut app, title);
            assert!(
                messages.iter().any(|message| matches!(
                    message,
                    Message::Download(crate::download::Message::Tab(tab)) if *tab == index
                )),
                "{title} at {size:?}"
            );
            for message in messages {
                let _ = app.update(message);
            }
            simulator(&app)
                .snapshot(&crate::theme::launcher())
                .unwrap()
                .matches_image(format!("{dir}/version-{index}-{}.png", size.width))
                .unwrap();
        }
        // Keep each viewport's top-level captures independent of the previous tabs.
        app.settings = Default::default();
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
