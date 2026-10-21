//! Real widget hit-testing: dispatching Navigate directly cannot catch blocked clicks.
use crate::app::{Launcher, Message, Page};
use iced::{Settings, Size};
use iced_test::Simulator;
use mc_core::{
    CatalogProvider, InMemoryTaskService, InstanceService, MockCatalogProvider,
    MockInstanceService, TaskService,
};

fn simulator<'a>(app: &'a Launcher) -> Simulator<'a, Message> {
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
async fn modal_blocks_sidebar_until_dismissed() {
    let mut app = populated().await;
    let _ = app.update(Message::EditInstance);
    let messages = click(&mut app, "游戏下载");
    assert!(!messages.iter().any(|m| matches!(m, Message::Navigate(_))));
    let _ = app.update(Message::Dismiss);
    assert!(click(&mut app, "游戏下载")
        .iter()
        .any(|m| matches!(m, Message::Navigate(Page::Download))));
}

// Optional visual inspection, excluded from ordinary test runs.
#[tokio::test]
#[ignore]
async fn capture_pages() {
    let mut app = populated().await;
    let dir = std::env::var("MEMORIA_UI_CAPTURE_DIR").expect("Set MEMORIA_UI_CAPTURE_DIR");
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
