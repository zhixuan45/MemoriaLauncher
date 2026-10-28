use super::*;
use crate::{ai, workspace};

#[tokio::test]
async fn ai_project_controls_keep_home_selection_and_target_the_ai_project() {
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        let mut app=populated().await;
        let home=app.active.clone();
        let selected=app.instances.iter().find(|i|i.instance.id!=home).unwrap().instance.id.clone();
        for instance in &mut app.instances {instance.installed=true;}
        let _=app.update(Message::Workspace(workspace::Message::OpenProject(selected.clone())));
        assert_eq!(app.active,home);
        app.viewport=size;
        let _=app.update(Message::Animate(app.animation_time+crate::motion::PAGE_DURATION));
        let launches=click(&mut app,"启动实例");
        assert!(launches.iter().any(|message|matches!(message,Message::LaunchInstance(_,id) if id==&selected)));
        for message in click(&mut app,"管理实例") {let _=app.update(message);}
        assert_eq!(app.instance_settings.instance,selected);
        assert_eq!(app.active,home);
        let _=app.update(Message::Dismiss);
        let _=app.update(Message::Navigate(Page::Home));
        assert_eq!(app.current().unwrap().instance.id,home);
    }
}

#[tokio::test]
async fn project_launch_buttons_target_their_own_instance_and_disable_during_a_game() {
    let mut app = populated().await;
    app.accounts.busy = false;
    let id = app
        .instances
        .iter()
        .find(|i| i.instance.id != app.active)
        .unwrap()
        .instance
        .id
        .clone();
    app.instances
        .iter_mut()
        .find(|i| i.instance.id == id)
        .unwrap()
        .installed = true;
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        app.viewport = size;
        let _ = app.update(Message::Animate(
            app.animation_time + crate::motion::PAGE_DURATION,
        ));
        let mut ui = simulator(&app);
        ui.click(iced_test::selector::id(format!("project-launch-{id}")))
            .unwrap();
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(messages
            .iter()
            .any(|m| matches!(m, Message::LaunchInstance(_, target) if target == &id)));
        for message in messages {
            let _ = app.update(message);
        }
        assert_eq!(app.active, id);
        assert_eq!(
            app.modal,
            Some(crate::app::Modal::Profile),
            "same account validation as the home launch"
        );
        let _ = app.update(Message::Dismiss);
        let _ = app.update(Message::Animate(
            app.animation_time + crate::motion::CLOSE_DURATION,
        ));
        app.launching = true;
        let mut ui = simulator(&app);
        ui.click(iced_test::selector::id(format!("project-launch-{id}")))
            .unwrap();
        assert!(!ui
            .into_messages()
            .any(|m| matches!(m, Message::LaunchInstance(..))));
        app.launching = false;
    }
}

#[tokio::test]
async fn home_new_chat_project_selection_and_conversation_search_are_real_controls() {
    let mut app = populated().await;
    app.viewport = Size::new(1024., 680.);
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    assert!(simulator(&app).find("实例项目").is_ok());
    for message in click(&mut app, "新对话") {
        let _ = app.update(message);
    }
    assert_eq!(app.page, Page::Ai);
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    assert!(simulator(&app).find("今天想做些什么？").is_ok());
    let first = app.workspace.current.id;
    app.ai.connection.config.model = "fixture-model".into();
    let _ = app.update(Message::Ai(ai::Message::Prompt("优化我的生存世界".into())));
    for message in click(&mut app, "发送") {
        let _ = app.update(message);
    }
    assert!(app.ai.busy);
    for message in click(&mut app, "新对话") {
        let _ = app.update(message);
    }
    assert_ne!(app.workspace.current.id, first);
    let _ = app.update(Message::ChatReply(
        first,
        ai::Message::ApiReply(Ok("保存在原对话的回复".into())),
    ));
    assert!(app.ai.messages.is_empty());
    for message in click(&mut app, "搜索对话") {
        let _ = app.update(message);
    }
    assert!(app.workspace.search_open);
    let _ = app.update(Message::Workspace(workspace::Message::Query(
        "优化我的生存世界".into(),
    )));
    let mut ui = simulator(&app);
    ui.click(iced_test::selector::id(format!(
        "conversation-project-{first}"
    )))
    .unwrap();
    for message in ui.into_messages() {
        let _ = app.update(message);
    }
    assert_eq!(app.workspace.current.id, first);
    assert_eq!(app.ai.messages[1].content, "保存在原对话的回复");
    for message in click(&mut app, "主页") {
        let _ = app.update(message);
    }
    assert_eq!(app.page, Page::Home);
}

#[tokio::test]
async fn collection_changes_never_launch_an_old_conversations_instance_with_the_same_id() {
    let mut app = populated().await;
    let id = app.active.clone();
    let old_root = app.environment.game_root.clone();
    let _ = app.update(Message::Workspace(workspace::Message::OpenProject(
        id.clone(),
    )));
    app.ai.prompt = "old collection draft".into();
    app.environment.game_root = app.environment.game_root.with_file_name("other-collection");
    assert_eq!(app.update(Message::LaunchInstance(old_root, id)).units(), 0);
    assert!(app.status.contains("实例集已变化"));
    app.page = Page::Ai;
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    assert!(simulator(&app).find("启动实例").is_err());
    let _ = app.update(Message::Workspace(workspace::Message::NewChat));
    assert!(app.ai.prompt.is_empty());
    assert_eq!(app.workspace.others.len(), 1);
}

#[tokio::test]
async fn home_metrics_plus_menu_and_mcp_settings_controls_are_visible_at_minimum_size() {
    let mut app = populated().await;
    app.viewport = Size::new(1024., 680.);
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    for title in ["游戏时长", "最近打开时间", "模组数量"] {
        assert!(simulator(&app).find(title).is_ok());
    }
    let _ = app.update(Message::Workspace(workspace::Message::OpenProject(
        app.active.clone(),
    )));
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    let mut ui = simulator(&app);
    ui.click(iced_test::selector::id("ai-attach")).unwrap();
    for message in ui.into_messages() {
        let _ = app.update(message);
    }
    assert!(app.ai.attachments.menu_open);
    for title in ["启用日志上下文", "添加图片", "添加文件"] {
        assert!(simulator(&app).find(title).is_ok());
    }
    for message in click(&mut app, "AI 设置") {
        let _ = app.update(message);
    }
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    for message in click(&mut app, "MCP 服务器") {
        let _ = app.update(message);
    }
    assert!(app.ai_settings.mcp_tab);
    assert!(click(&mut app, "保存 MCP 权限")
        .iter()
        .any(|m| matches!(m, Message::McpSettings(crate::mcp_settings::Message::Save))));
    let messages = click(&mut app, "启用内置 MCP 服务");
    assert!(messages.iter().any(|m| matches!(
        m,
        Message::McpSettings(crate::mcp_settings::Message::Enabled(_))
    )));
    for message in messages {
        let _ = app.update(message);
    }
    assert!(app.mcp_settings.draft.enabled);
    assert!(
        !app.mcp_settings.saved.enabled,
        "draft changes must not grant permissions before save"
    );
}

#[tokio::test]
#[ignore = "writes UI review screenshots"]
async fn capture_workspace_redesign() {
    let mut app = populated().await;
    for instance in &mut app.instances {
        instance.installed = true;
    }
    let dir = std::env::var("MEMORIA_UI_CAPTURE_DIR").expect("Set MEMORIA_UI_CAPTURE_DIR");
    std::fs::create_dir_all(&dir).unwrap();
    let id = app.active.clone();
    let _ = app.update(Message::Workspace(workspace::Message::OpenProject(id)));
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        app.viewport = size;
        for page in Page::ALL {
            app.page = page;
            simulator(&app)
                .snapshot(&crate::theme::launcher())
                .unwrap()
                .matches_image(format!("{dir}/{page:?}-{}.png", size.width))
                .unwrap();
        }
    }
    app.page = Page::Ai;
    app.ai.connection.config.model = "fixture-model".into();
    let _ = app.update(Message::Ai(ai::Message::Prompt(
        "我想优化这个实例的配置。".into(),
    )));
    let _ = app.update(Message::Ai(ai::Message::Submit));
    let _ = app.update(Message::Ai(ai::Message::ApiReply(Ok("可以先检查 Java 版本、内存分配和渲染距离。请告诉我当前的显卡和总内存，我会结合这个实例的版本给出建议。".into()))));
    let _ = app.update(Message::Workspace(workspace::Message::NewChat));
    app.viewport = Size::new(1536., 921.);
    simulator(&app)
        .snapshot(&crate::theme::launcher())
        .unwrap()
        .matches_image(format!("{dir}/projects-and-conversations.png"))
        .unwrap();
}

#[tokio::test]
#[ignore = "writes UI review screenshots"]
async fn capture_context_and_mcp_settings() {
    let mut app = populated().await;
    let dir = std::env::var("MEMORIA_UI_CAPTURE_DIR").expect("Set MEMORIA_UI_CAPTURE_DIR");
    std::fs::create_dir_all(&dir).unwrap();
    let id = app.active.clone();
    let _ = app.update(Message::Workspace(workspace::Message::OpenProject(id)));
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    let _ = app.update(Message::Ai(ai::Message::Attachments(
        crate::attachments::Message::Menu,
    )));
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        app.viewport = size;
        simulator(&app)
            .snapshot(&crate::theme::launcher())
            .unwrap()
            .matches_image(format!("{dir}/context-menu-{}.png", size.width))
            .unwrap();
    }
    app.ai.attachments.menu_open = false;
    let file = crate::attachments::Attachment {
        id: uuid::Uuid::new_v4(),
        name: "游戏截图.png".into(),
        size: include_bytes!("../ui/assets/icon_grass.png").len(),
        content: crate::attachments::Content::Image {
            mime: "image/png",
            bytes: std::sync::Arc::new(include_bytes!("../ui/assets/icon_grass.png").to_vec()),
        },
    };
    app.ai.attachments.preview = Some(file.id);
    app.ai.attachments.files.push(file);
    app.ai.prompt = "帮我分析这张游戏截图。".into();
    app.viewport = Size::new(1024., 680.);
    simulator(&app)
        .snapshot(&crate::theme::launcher())
        .unwrap()
        .matches_image(format!("{dir}/image-context-1024.png"))
        .unwrap();
    let _ = app.update(Message::Ai(ai::Message::OpenSettings));
    let _ = app.update(Message::AiSettings(crate::ai_settings::Message::McpTab(
        true,
    )));
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        app.viewport = size;
        simulator(&app)
            .snapshot(&crate::theme::launcher())
            .unwrap()
            .matches_image(format!("{dir}/mcp-settings-{}.png", size.width))
            .unwrap();
    }
}

#[tokio::test]
async fn mcp_write_categories_are_separate_drafts_and_examples_are_reachable() {
    let mut app = populated().await;
    app.viewport = Size::new(1536., 921.);
    let _ = app.update(Message::Ai(ai::Message::OpenSettings));
    let _ = app.update(Message::AiSettings(crate::ai_settings::Message::McpTab(
        true,
    )));
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    assert!(!app.mcp_settings.saved.permissions.write_mod_list);
    for (index, title) in [
        (
            3,
            "间接写入 Mod 列表 · 使用 Modrinth / CurseForge 项目或文件 ID",
        ),
        (4, "直接写入 Mod 列表 · 上传 JAR 文件（需 SHA-256）"),
        (5, "写入配置文件 · config / defaultconfigs（FTB 任务除外）"),
        (6, "写入 FTB 任务 · 条目、物品图标、条件、奖励与图片"),
    ] {
        let messages = click(&mut app, title);
        assert!(messages.iter().any(|message|matches!(message,Message::McpSettings(crate::mcp_settings::Message::Permission(i,true)) if *i==index)));
        for message in messages {
            let _ = app.update(message);
        }
    }
    let draft = &app.mcp_settings.draft.permissions;
    assert!(
        draft.write_mod_list && draft.write_mod_files && draft.write_configs && draft.write_quests
    );
    let saved = &app.mcp_settings.saved.permissions;
    assert!(
        !saved.write_mod_list
            && !saved.write_mod_files
            && !saved.write_configs
            && !saved.write_quests
    );
    assert!(click(&mut app, "复制任务示例").iter().any(|m| matches!(
        m,
        Message::McpSettings(crate::mcp_settings::Message::CopyQuestExample)
    )));
}
