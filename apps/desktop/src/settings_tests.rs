use super::*;
use iced::futures::StreamExt;

async fn execute(state: &mut State, task: Task<Message>) {
    let mut stream = iced_test::runtime::task::into_stream(task).unwrap();
    while let Some(action) = stream.next().await {
        if let iced_test::runtime::Action::Output(message) = action {
            let _ = state.update(message, Instant::now());
        }
    }
}

fn fixture() -> State {
    State::from_directory(
        std::env::temp_dir().join(format!("memoria-settings-{}", uuid::Uuid::new_v4())),
    )
}

#[tokio::test]
async fn save_task_persists_only_confirmed_preferences_and_survives_restart() {
    let mut state = fixture();
    let now = Instant::now();
    let _ = state.update(Message::Toggle(0, true), now);
    let _ = state.update(Message::Date(DATES[1]), now);
    assert!(!state.saved.minimize_on_launch);
    assert!(!state.directory.exists());
    let task = state.update(Message::Save, now);
    assert!(state.busy);
    let _ = state.update(Message::Reset, now);
    let _ = state.update(Message::Toggle(0, false), now);
    assert!(state.draft.minimize_on_launch);
    execute(&mut state, task).await;
    assert!(!state.busy);
    assert!(state.saved.minimize_on_launch);
    assert_eq!(state.saved.date_format, 1);
    let reloaded = State::from_directory(state.directory.clone());
    assert_eq!(reloaded.saved, state.saved);
    assert_eq!(reloaded.draft, state.saved);
    let original = std::fs::read(state.directory.join("general.json")).unwrap();
    let imported = Config {
        start_minimized: true,
        date_format: 2,
        ..Default::default()
    };
    let _ = state.update(Message::Imported(Ok(Some(imported.clone()))), now);
    assert_eq!(state.draft, imported);
    assert_ne!(state.saved, imported);
    let _ = state.update(Message::Imported(Err("bad file".into())), now);
    assert_eq!(state.draft, imported);
    assert_eq!(
        std::fs::read(state.directory.join("general.json")).unwrap(),
        original
    );
    let _ = state.update(Message::Revert, now);
    assert_eq!(state.draft, state.saved);
    let _ = state.update(Message::Reset, now);
    assert_eq!(state.draft, Config::default());
    assert_ne!(state.saved, state.draft);
    std::fs::remove_dir_all(state.directory).unwrap();
}

#[tokio::test]
async fn failed_save_keeps_saved_preferences_and_editable_draft() {
    let mut state = fixture();
    std::fs::create_dir_all(&state.directory).unwrap();
    std::fs::create_dir(state.directory.join("general.json")).unwrap();
    let now = Instant::now();
    let _ = state.update(Message::Toggle(0, true), now);
    let task = state.update(Message::Save, now);
    execute(&mut state, task).await;
    assert!(!state.busy);
    assert_eq!(state.saved, Config::default());
    assert!(state.draft.minimize_on_launch);
    assert!(state.notice.contains("保存失败"));
    std::fs::remove_dir_all(state.directory).unwrap();
}

#[tokio::test]
async fn cache_requires_confirmation_and_startup_and_update_replies_are_scoped() {
    let mut state = fixture();
    let now = Instant::now();
    assert_eq!(state.update(Message::ConfirmCache, now).units(), 0);
    let _ = state.update(Message::ClearCache, now);
    let _ = state.update(Message::CancelCache, now);
    assert_eq!(state.update(Message::ConfirmCache, now).units(), 0);
    let _ = state.update(Message::ClearCache, now);
    let task = state.update(Message::ConfirmCache, now);
    execute(&mut state, task).await;
    assert!(state.notice.contains("没有"));
    assert!(!state.directory.exists());
    state.saved.start_minimized = true;
    assert!(state.startup().units() > 0);
    assert_eq!(state.startup().units(), 0);
    let _ = state.update(Message::CheckUpdate, now);
    assert!(state.checking);
    assert_eq!(state.update(Message::CheckUpdate, now).units(), 0);
    let _ = state.update(Message::Checked(state.update_generation - 1, Ok(None)), now);
    assert!(state.checking);
    let _ = state.update(
        Message::Saved(Ok(Config {
            prerelease: true,
            ..Default::default()
        })),
        now,
    );
    assert!(!state.checking);
    let _ = state.update(Message::Checked(state.update_generation - 1, Ok(None)), now);
    assert!(state.update_notice.contains("通道已更改"));
    let _ = state.update(
        Message::Checked(state.update_generation, Err("HTTP 403".into())),
        now,
    );
    assert!(state.update_notice.contains("HTTP 403"));
    assert!(state.available.is_none());
}
