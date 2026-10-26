use super::*;
use crate::{ai, api_client::ChatMessage};

fn context(root: &str, id: &str) -> Project {
    Project {
        root: root.into(),
        instance_id: id.into(),
        name: id.into(),
        version: "1.21.1".into(),
        loader: "Fabric".into(),
    }
}
#[test]
fn switching_threads_retains_drafts_history_and_project_context() {
    let mut workspace = State::default();
    let mut ai = ai::State::default();
    workspace.new_chat(&mut ai, Some(context("/a", "one")));
    let first = workspace.current.id;
    ai.prompt = "draft one".into();
    ai.messages = vec![
        ChatMessage::user("question".into()),
        ChatMessage::assistant("reply".into()),
    ];
    workspace.new_chat(&mut ai, Some(context("/a", "two")));
    assert!(ai.prompt.is_empty());
    assert!(ai.messages.is_empty());
    assert!(ai.project_context.as_ref().unwrap().contains("two"));
    ai.prompt = "draft two".into();
    workspace.open(&mut ai, first);
    assert_eq!(ai.prompt, "draft one");
    assert_eq!(ai.messages.len(), 2);
    assert_eq!(workspace.others[0].state.prompt, "draft two");
    assert!(ai.project_context.as_ref().unwrap().contains("one"));
    assert!(!workspace
        .current
        .project
        .as_ref()
        .unwrap()
        .matches(std::path::Path::new("/b"), "one"));
}

#[test]
fn empty_new_threads_do_not_fill_the_sidebar_and_search_matches_project_names() {
    let mut workspace = State::default();
    let mut ai = ai::State::default();
    for _ in 0..20 {
        workspace.new_chat(&mut ai, Some(context("/a", "survival")));
    }
    assert!(workspace.others.is_empty());
    workspace.query = "SURVIVAL".into();
    assert!(workspace.visible(&workspace.current));
    workspace.query = "missing".into();
    assert!(!workspace.visible(&workspace.current));
}

#[tokio::test]
async fn late_reply_is_applied_to_its_original_conversation_and_cannot_scroll_the_new_one() {
    let (mut app, _) = Launcher::new();
    app.ai.connection.config.model = "fixture".into();
    let first = app.workspace.current.id;
    let _ = update_ai(
        &mut app,
        first,
        ai::Message::Prompt("original question".into()),
    );
    let _ = update_ai(&mut app, first, ai::Message::Submit);
    assert!(app.ai.busy);
    app.workspace
        .new_chat(&mut app.ai, Some(context("/a", "two")));
    app.ai.prompt = "new draft".into();
    let task = update_ai(
        &mut app,
        first,
        ai::Message::ApiReply(Ok("original answer".into())),
    );
    assert_eq!(task.units(), 0);
    assert_eq!(app.ai.prompt, "new draft");
    assert!(app.ai.messages.is_empty());
    let original = app
        .workspace
        .others
        .iter()
        .find(|c| c.info.id == first)
        .unwrap();
    assert_eq!(original.state.messages[1].content, "original answer");
    assert!(!original.state.busy);
    assert_eq!(original.info.title, "original question");
    app.workspace.open(&mut app.ai, first);
    assert_eq!(app.ai.messages[1].content, "original answer");
}
