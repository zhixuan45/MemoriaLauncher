use super::*;
use crate::project_menu::Message as Menu;

fn open(app: &mut Launcher, id: &str) {
    let mut ui = simulator(app);
    ui.click(iced_test::selector::id(format!("project-more-{id}")))
        .unwrap();
    let messages: Vec<_> = ui.into_messages().collect();
    assert!(messages
        .iter()
        .any(|m| matches!(m,Message::ProjectMenu(Menu::Open(target)) if target==id)));
    for message in messages {
        let _ = app.update(message);
    }
    assert_eq!(
        app.project_menu.menu.as_ref().unwrap().project.instance_id,
        id
    );
}
#[tokio::test]
async fn project_menu_controls_target_the_row_and_dismiss_without_clicking_through() {
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        let mut app = populated().await;
        app.viewport = size;
        let _ = app.update(Message::Animate(
            app.animation_time + crate::motion::PAGE_DURATION,
        ));
        let home = app.active.clone();
        let id = app
            .instances
            .iter()
            .find(|i| i.instance.id != home)
            .unwrap()
            .instance
            .id
            .clone();
        open(&mut app, &id);
        assert!(simulator(&app).find("在资源管理器中打开").is_ok());
        for message in click(&mut app, "置顶") {
            let _ = app.update(message);
        }
        assert!(app
            .project_menu
            .preferences
            .pinned(&app.environment.game_root, &id));
        assert!(app.project_menu.menu.is_none());
        assert_eq!(app.active, home);
        open(&mut app, &id);
        let messages = click(&mut app, "主页");
        assert!(messages
            .iter()
            .any(|m| matches!(m, Message::ProjectMenu(Menu::Close))));
        assert!(!messages.iter().any(|m| matches!(m, Message::Navigate(_))));
        for message in messages {
            let _ = app.update(message);
        }
        open(&mut app, &id);
        let mut ui = simulator(&app);
        ui.click(iced_test::selector::id("project-menu-Edit"))
            .unwrap();
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(messages.iter().any(|m|matches!(m,Message::ProjectMenu(Menu::Edit))),"messages: {messages:?}; busy={}, launching={}, loading={}, environment_saving={}, download={}",app.instance_writing(),app.launching,app.loading,app.environment_saving,app.download.installing.is_some());
        for message in messages {
            let _ = app.update(message);
        }
        assert_eq!(app.instance_settings.instance, id);
        assert_eq!(app.active, home);
        assert_eq!(app.modal, Some(crate::app::Modal::Instance));
    }
}

#[tokio::test]
async fn sections_hide_and_archive_are_reversible_and_do_not_delete_instances_or_drafts() {
    let mut app = populated().await;
    app.viewport = Size::new(1536., 921.);
    let id = app.active.clone();
    let total = app.instances.len();
    let project = crate::workspace::project(&app, &id).unwrap();
    app.workspace.new_chat(&mut app.ai, Some(project.clone()));
    app.ai.prompt = "保留这个草稿".into();
    let chat = app.workspace.current.id;
    app.ai.busy = true;
    assert!(app
        .workspace
        .archive_project(&mut app.ai, &project)
        .is_err());
    app.ai.busy = false;
    let _ = app.update(Message::ProjectMenu(Menu::Open(id.clone())));
    for message in click(&mut app, "归档项目对话") {
        let _ = app.update(message);
    }
    assert!(app.workspace.is_archived(chat));
    assert!(app
        .workspace
        .others
        .iter()
        .any(|c| c.info.id == chat && c.state.prompt == "保留这个草稿"));
    let _ = app.update(Message::ProjectMenu(Menu::RestoreChat(chat)));
    assert!(!app.workspace.is_archived(chat));
    assert_eq!(app.ai.prompt, "保留这个草稿");
    let _ = app.update(Message::ProjectMenu(Menu::Open(id.clone())));
    let _ = app.update(Message::ProjectMenu(Menu::Sections));
    let _ = app.update(Message::ProjectMenu(Menu::SectionName("整合包".into())));
    for message in click(&mut app, "新建并移入") {
        let _ = app.update(message);
    }
    assert_eq!(
        app.project_menu
            .preferences
            .section(&app.environment.game_root, &id),
        "整合包"
    );
    let _ = app.update(Message::ProjectMenu(Menu::Open(id.clone())));
    for message in click(&mut app, "移除项目") {
        let _ = app.update(message);
    }
    assert!(app
        .project_menu
        .preferences
        .hidden(&app.environment.game_root, &id));
    assert_eq!(app.instances.len(), total);
    assert_eq!(app.ai.prompt, "保留这个草稿");
    assert!(simulator(&app)
        .find(iced_test::selector::id(format!("project-{id}")))
        .is_err());
    let _ = app.update(Message::ProjectMenu(Menu::Restore(id.clone())));
    assert!(!app
        .project_menu
        .preferences
        .hidden(&app.environment.game_root, &id));
}

#[test]
fn sidebar_organization_survives_reload_and_scopes_identical_ids_by_collection() {
    let root = std::env::temp_dir().join(format!("project-menu-{}", uuid::Uuid::new_v4()));
    let path = root.join("sidebar.json");
    let a = root.join("a");
    let b = root.join("b");
    let mut preferences = crate::project_preferences::State::load_from(path.clone());
    preferences
        .modify(&a, "versions/one", |e| {
            e.pinned = true;
            e.hidden = true;
            e.section = "常用".into()
        })
        .unwrap();
    let reloaded = crate::project_preferences::State::load_from(path.clone());
    assert!(reloaded.pinned(&a, "versions/one"));
    assert!(reloaded.hidden(&a, "versions/one"));
    assert!(!reloaded.hidden(&b, "versions/one"));
    assert_eq!(reloaded.sections(), ["常用"]);
    let original = std::fs::read(&path).unwrap();
    assert!(preferences
        .modify(&a, "versions/one", |e| e.section = "x".repeat(25))
        .is_err());
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[tokio::test]
#[ignore = "Writes project-menu and section screenshots"]
async fn capture_project_menu() {
    let directory = std::env::var("MEMORIA_UI_CAPTURE_DIR").unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        let mut app = populated().await;
        app.viewport = size;
        let _ = app.update(Message::Animate(
            app.animation_time + crate::motion::PAGE_DURATION,
        ));
        let id = app.active.clone();
        let _ = app.update(Message::ProjectMenu(Menu::Open(id.clone())));
        simulator(&app)
            .snapshot(&app.theme())
            .unwrap()
            .matches_image(format!("{directory}/menu-{}.png", size.width))
            .unwrap();
        let _ = app.update(Message::ProjectMenu(Menu::Sections));
        simulator(&app)
            .snapshot(&app.theme())
            .unwrap()
            .matches_image(format!("{directory}/sections-{}.png", size.width))
            .unwrap();
    }
}
