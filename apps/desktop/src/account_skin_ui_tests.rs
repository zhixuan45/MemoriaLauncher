use super::*;
use crate::accounts::{AccountIndex, Message as AccountMessage};
use iced::futures::StreamExt;

async fn complete(app: &mut Launcher, task: iced::Task<Message>) {
    let mut tasks = vec![task];
    while let Some(task) = tasks.pop() {
        if let Some(mut stream) = iced_test::runtime::task::into_stream(task) {
            while let Some(action) = stream.next().await {
                if let iced_test::runtime::Action::Output(message) = action {
                    tasks.push(app.update(message));
                }
            }
        }
    }
}

async fn profile(size: Size) -> (Launcher, std::path::PathBuf, mc_core::AuthAccount) {
    let root = std::env::temp_dir().join(format!("memoria-skin-ui-{}", uuid::Uuid::new_v4()));
    let account = mc_core::AuthAccount::offline("OfflinePlayer");
    // Use a real bundled skin as the imported file, including its head overlay.
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("input.png");
    std::fs::write(
        &source,
        include_bytes!("../ui/assets/default-skins/slim-sunny.png"),
    )
    .unwrap();
    crate::account_skin::save_offline(&root, &account, &source).unwrap();
    let mut app = populated().await;
    app.viewport = size;
    app.accounts = crate::accounts::State::from_directory(root.clone());
    let task = app.update(Message::Accounts(AccountMessage::Loaded(Ok(
        AccountIndex {
            accounts: vec![account.clone()],
            active: Some(account.id.clone()),
        },
    ))));
    complete(&mut app, task).await;
    let task = app.update(Message::Profile);
    complete(&mut app, task).await;
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::OPEN_DURATION,
    ));
    (app, root, account)
}

#[tokio::test]
async fn offline_skin_controls_show_persisted_skin_and_reset_at_both_sizes() {
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        let (mut app, root, account) = profile(size).await;
        assert!(simulator(&app).find("本地自定义皮肤").is_ok());
        assert!(click(&mut app, "设置皮肤").iter().any(|m| matches!(m,
            Message::Accounts(AccountMessage::ChooseSkin(id)) if id == &account.id)));
        let expected = crate::account_skin::avatar(include_bytes!(
            "../ui/assets/default-skins/slim-sunny.png"
        ))
        .unwrap();
        let iced::widget::image::Handle::Rgba {
            pixels: expected, ..
        } = expected
        else {
            panic!("pixels")
        };
        let iced::widget::image::Handle::Rgba { pixels: actual, .. } =
            app.accounts.avatar(&account)
        else {
            panic!("pixels")
        };
        assert_eq!(expected, actual);
        let messages = click(&mut app, "恢复默认皮肤");
        assert!(messages.iter().any(
            |m| matches!(m, Message::Accounts(AccountMessage::ResetSkin(id)) if id == &account.id)
        ));
        for message in messages {
            let task = app.update(message);
            complete(&mut app, task).await;
        }
        assert!(!app.accounts.custom_skin(&account.id));
        assert_eq!(
            app.accounts.avatar(&account),
            crate::account_skin::fallback(&account)
        );
        assert!(!crate::account_skin::skin_path(&root, &account)
            .unwrap()
            .exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
#[ignore = "requires a screenshot output directory"]
async fn capture_account_skins() {
    let dir = std::env::var("MEMORIA_UI_CAPTURE_DIR").expect("Set MEMORIA_UI_CAPTURE_DIR");
    std::fs::create_dir_all(&dir).unwrap();
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        let (mut app, root, account) = profile(size).await;
        simulator(&app)
            .snapshot(&crate::theme::launcher())
            .unwrap()
            .matches_image(format!("{dir}/account-custom-{}.png", size.width))
            .unwrap();
        let task = app.update(Message::Accounts(AccountMessage::ResetSkin(account.id)));
        complete(&mut app, task).await;
        simulator(&app)
            .snapshot(&crate::theme::launcher())
            .unwrap()
            .matches_image(format!("{dir}/account-default-{}.png", size.width))
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
