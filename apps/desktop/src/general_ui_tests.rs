use super::*;
use crate::settings::Message as GeneralMessage;
use iced::futures::StreamExt;

fn scroll_to_row<'a>(
    app: &'a Launcher,
    pixels: f32,
) -> Simulator<'a, Message, iced::Theme, crate::motion_renderer::Renderer> {
    let mut ui = simulator(app);
    // The gutter between cards scrolls the page, independent of card scroll positions.
    ui.point_at((
        (crate::sidebar::width(app.viewport.width) + app.viewport.width) / 2.,
        350.,
    ));
    ui.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
        delta: iced::mouse::ScrollDelta::Pixels { x: 0., y: -pixels },
    })]);
    ui
}

async fn general_page(size: Size) -> Launcher {
    let mut app = populated().await;
    app.settings = crate::settings::State::from_directory(
        std::env::temp_dir().join(format!("memoria-general-ui-{}", uuid::Uuid::new_v4())),
    );
    app.viewport = size;
    let _ = app.update(Message::Navigate(Page::Settings));
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    app
}

#[tokio::test]
async fn general_controls_route_and_saved_preferences_survive_actual_ui_tasks_at_both_sizes() {
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        let mut app = general_page(size).await;
        let _ = app.update(Message::Settings(GeneralMessage::Toggle(0, true)));
        let _ = app.update(Message::Settings(GeneralMessage::Date(
            crate::general::DATES[1],
        )));
        let messages = click(&mut app, "保存常规设置");
        assert!(messages
            .iter()
            .any(|m| matches!(m, Message::Settings(GeneralMessage::Save))));
        let task = app.update(messages.into_iter().next().unwrap());
        let mut stream = iced_test::runtime::task::into_stream(task).unwrap();
        while let Some(action) = stream.next().await {
            if let iced_test::runtime::Action::Output(message) = action {
                let _ = app.update(message);
            }
        }
        assert!(app.settings.saved.minimize_on_launch);
        assert_eq!(app.settings.saved.date_format, 1);
        assert!(click(&mut app, "撤销常规更改")
            .iter()
            .any(|m| matches!(m, Message::Settings(GeneralMessage::Revert))));
        assert!(click(&mut app, "退出启动器")
            .iter()
            .any(|m| matches!(m, Message::Quit)));
        for (target, expected) in [("打开数据文件夹", 0), ("清理缓存", 1), ("检查更新", 2)]
        {
            let mut ui = scroll_to_row(&app, 300.);
            ui.click(target)
                .unwrap_or_else(|e| panic!("{target} at {size:?}: {e}"));
            assert!(ui.into_messages().any(|m| matches!(
                (m, expected),
                (Message::Settings(GeneralMessage::OpenData), 0)
                    | (Message::Settings(GeneralMessage::ClearCache), 1)
                    | (Message::Settings(GeneralMessage::CheckUpdate), 2)
            )));
        }
        let _ = app.update(Message::Settings(GeneralMessage::ClearCache));
        let mut ui = scroll_to_row(&app, 300.);
        ui.click("取消").unwrap();
        assert!(ui
            .into_messages()
            .any(|m| matches!(m, Message::Settings(GeneralMessage::CancelCache))));
        for target in ["导出常规设置", "导入常规设置", "恢复默认设置"] {
            let mut ui = scroll_to_row(&app, 2000.);
            ui.click(target)
                .unwrap_or_else(|e| panic!("{target} at {size:?}: {e}"));
            assert!(ui
                .into_messages()
                .any(|m| matches!(m, Message::Settings(_))));
        }
        let _ = app.update(Message::Settings(GeneralMessage::Toggle(3, true)));
        assert!(
            !app.settings.saved.minimize_on_close,
            "unsaved switch must not alter closing behavior"
        );
        app.settings.saved.minimize_on_close = true;
        assert!(
            app.update(Message::Event(iced::Event::Window(
                iced::window::Event::CloseRequested
            )))
            .units()
                > 0
        );
        let active = app.active.clone();
        app.instances
            .iter_mut()
            .find(|entry| entry.instance.id == active)
            .unwrap()
            .instance
            .last_played_at = Some("2026-10-27T09:15:00Z".parse().unwrap());
        let _ = app.update(Message::Navigate(Page::Home));
        let _ = app.update(Message::Animate(
            app.animation_time + crate::motion::PAGE_DURATION,
        ));
        simulator(&app)
            .find("27/10/2026 09:15")
            .expect("saved format must reach the homepage timestamp");
    }
}

#[tokio::test]
async fn general_cache_confirmation_is_blocked_while_game_or_installer_is_busy() {
    let mut app = general_page(Size::new(1024., 680.)).await;
    let _ = app.update(Message::Settings(GeneralMessage::ClearCache));
    app.launching = true;
    assert_eq!(
        app.update(Message::Settings(GeneralMessage::ConfirmCache))
            .units(),
        0
    );
    assert!(app.status.contains("完成后再清理缓存"));
}

#[tokio::test]
#[ignore = "writes general settings screenshots; set MEMORIA_UI_CAPTURE_DIR"]
async fn capture_general_settings() {
    let dir = std::env::var("MEMORIA_UI_CAPTURE_DIR").expect("Set MEMORIA_UI_CAPTURE_DIR");
    std::fs::create_dir_all(&dir).unwrap();
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        let mut app = general_page(size).await;
        simulator(&app)
            .snapshot(&app.theme())
            .unwrap()
            .matches_image(format!("{dir}/general-top-{}.png", size.width))
            .unwrap();
        scroll_to_row(&app, 2000.)
            .snapshot(&app.theme())
            .unwrap()
            .matches_image(format!("{dir}/general-actions-{}.png", size.width))
            .unwrap();
        let _ = app.update(Message::Settings(GeneralMessage::ClearCache));
        scroll_to_row(&app, 300.)
            .snapshot(&app.theme())
            .unwrap()
            .matches_image(format!("{dir}/general-confirm-{}.png", size.width))
            .unwrap();
    }
}
