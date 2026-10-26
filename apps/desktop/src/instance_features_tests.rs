use super::*;
use crate::{appearance, instance_settings, resource_install};
use iced::futures::StreamExt;
use std::path::PathBuf;

#[tokio::test]
#[ignore = "installs a real Modrinth resource into an isolated fixture through GUI controls"]
async fn live_resource_gui_installation() {
    let (mut app, root) = fixture().await;
    let catalog = mc_core::ResourceCatalog::new().unwrap();
    let page = catalog
        .search(&mc_core::ResourceQuery {
            kind: mc_core::ResourceKind::Mod,
            query: "Fabric API".into(),
            game_version: "1.21.1".into(),
            loader: "fabric".into(),
            offset: 0,
        })
        .await
        .unwrap();
    let project = page
        .hits
        .into_iter()
        .find(|p| p.slug == "fabric-api")
        .unwrap();
    let versions = catalog
        .versions(&project.project_id, "1.21.1", "fabric")
        .await
        .unwrap();
    let version = versions
        .iter()
        .find(|v| v.version_type == "release")
        .or(versions.first())
        .unwrap()
        .clone();
    println!("Live GUI install: {} / {}", project.project_id, version.id);
    app.page = Page::Download;
    let _ = app.download.update(crate::download::Message::Category(1));
    app.download.resources.selected = Some(project.project_id.clone());
    app.download.resources.versions = vec![version];
    app.download.resources.loading = false;
    app.download.resources.page = Some(mc_core::ResourcePage {
        hits: vec![project],
        total_hits: 1,
        offset: 0,
        limit: 20,
    });
    app.viewport = Size::new(1024., 680.);
    settle(&mut app);
    for message in click(&mut app, "安装此资源版本") {
        let _ = app.update(message);
    }
    settle(&mut app);
    for message in click(&mut app, "生成安装预览") {
        let task = app.update(message);
        complete(&mut app, task).await;
    }
    assert!(
        app.resource_install.preview.is_some(),
        "{}",
        app.resource_install.notice
    );
    let directory = std::env::var("MEMORIA_UI_CAPTURE_DIR").unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    simulator(&app)
        .snapshot(&app.theme())
        .unwrap()
        .matches_image(format!("{directory}/resource-live-preview.png"))
        .unwrap();
    for message in click(&mut app, "确认安装") {
        let task = app.update(message);
        complete(&mut app, task).await;
    }
    assert!(!app.resource_install.busy);
    assert!(
        app.resource_install.notice.contains("安装完成"),
        "{}",
        app.resource_install.notice
    );
    let snapshot = mc_core::service::desktop_instances::snapshot(&root, "versions/test").unwrap();
    assert!(!snapshot.mods.is_empty());
    simulator(&app)
        .snapshot(&app.theme())
        .unwrap()
        .matches_image(format!("{directory}/resource-live-installed.png"))
        .unwrap();
    println!(
        "Installed {} real Mod files; source receipt recorded under .memoria",
        snapshot.mods.len()
    );
    std::fs::remove_dir_all(root).unwrap();
}

async fn complete(app: &mut Launcher, task: iced::Task<Message>) {
    let mut pending = vec![task];
    while let Some(task) = pending.pop() {
        if let Some(mut stream) = iced_test::runtime::task::into_stream(task) {
            while let Some(action) = stream.next().await {
                if let iced_test::runtime::Action::Output(message) = action {
                    pending.push(app.update(message));
                }
            }
        }
    }
}
async fn fixture() -> (Launcher, PathBuf) {
    let root = std::env::temp_dir().join(format!("memoria-ui-instance-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("versions/test/mods")).unwrap();
    std::fs::write(root.join("versions/test/test.json"),br#"{"id":"test","clientVersion":"1.21.1","mainClass":"net.minecraft.client.main.Main","assets":"1.21","libraries":[{"name":"net.fabricmc:fabric-loader:0.16.0"}]}"#).unwrap();
    std::fs::write(root.join("versions/test/test.jar"), b"fixture").unwrap();
    let (mut app, _) = Launcher::new();
    app.loading = false;
    app.environment.game_root = root.clone();
    let data = crate::local::scan(root.clone()).await.unwrap();
    let _ = app.update(Message::Loaded(Ok(data)));
    (app, root)
}
fn settle(app: &mut Launcher) {
    let _ = app.update(Message::Animate(
        app.animation_time + std::time::Duration::from_secs(1),
    ));
}
async fn open(app: &mut Launcher) {
    let task = app.update(Message::EditInstance);
    complete(app, task).await;
    settle(app);
}
fn resource_fixture(app: &mut Launcher) {
    app.page = Page::Download;
    let _ = app.download.update(crate::download::Message::Category(1));
    app.download.resources.page=Some(serde_json::from_value(serde_json::json!({"hits":[{"project_id":"main","slug":"sample","title":"示例资源","description":"测试数据","author":"fixture","downloads":0,"categories":["fabric"],"versions":["1.21.1"]}],"total_hits":1,"offset":0,"limit":20})).unwrap());
    app.download.resources.loading = false;
    app.download.resources.selected = Some("main".into());
    app.download.resources.versions=vec![serde_json::from_value(serde_json::json!({"id":"mainV","name":"示例版本","version_number":"1.0","version_type":"release","game_versions":["1.21.1"],"loaders":["fabric"],"files":[{"filename":"sample.jar","size":2048}],"dependencies":[]})).unwrap()];
    settle(app);
}
#[tokio::test]
async fn instance_settings_and_mod_management_are_reachable_and_saved_at_both_sizes() {
    let (mut app, root) = fixture().await;
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        app.viewport = size;
        open(&mut app).await;
        assert!(simulator(&app).find("保存实例设置").is_ok());
        let _ = app.update(Message::InstanceSettings(instance_settings::Message::Name(
            "我的独立实例".into(),
        )));
        let _ = app.update(Message::InstanceSettings(
            instance_settings::Message::Memory("2048".into()),
        ));
        for message in click(&mut app, "保存实例设置") {
            let task = app.update(message);
            complete(&mut app, task).await;
        }
        assert_eq!(app.current().unwrap().instance.name, "我的独立实例");
        for message in click(&mut app, "Mod 管理") {
            let _ = app.update(message);
        }
        assert!(app.instance_settings.mods_tab);
        assert!(click(&mut app, "导入 JAR").iter().any(|v| matches!(
            v,
            Message::InstanceSettings(instance_settings::Message::Import)
        )));
        assert!(simulator(&app)
            .find("此实例还没有 Mod，可导入 JAR 或从资源目录安装。")
            .is_ok());
        let _ = app.update(Message::Dismiss);
        settle(&mut app);
    }
    assert_eq!(
        mc_core::service::desktop_instances::load_settings(&root, "versions/test")
            .unwrap()
            .memory_mb,
        Some(2048)
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn installation_button_opens_a_pinned_target_and_busy_work_blocks_launch_and_collection_changes(
) {
    let (mut app, root) = fixture().await;
    app.viewport = Size::new(1024., 680.);
    resource_fixture(&mut app);
    for message in click(&mut app, "安装此资源版本") {
        let _ = app.update(message);
    }
    settle(&mut app);
    assert_eq!(app.modal, Some(crate::app::Modal::ResourceInstall));
    assert_eq!(
        app.resource_install.request.as_ref().unwrap().instance,
        "versions/test"
    );
    assert!(click(&mut app, "生成安装预览").iter().any(|m| matches!(
        m,
        Message::ResourceInstall(resource_install::Message::Prepare)
    )));
    assert!(click(&mut app, "确认安装").is_empty());
    let _ = app.update(Message::ResourceInstall(resource_install::Message::Prepare));
    assert!(app.resource_install.busy);
    assert_eq!(app.update(Message::Launch).units(), 0);
    assert!(!app.launching);
    let _ = app.update(Message::GameSettings(
        crate::game_settings::Message::Folder("other".into()),
    ));
    assert_eq!(app.environment.game_root, root);
    let generation = app.resource_install.generation;
    let _ = app.update(Message::ResourceInstall(
        resource_install::Message::Prepared(generation + 1, Err("stale".into())),
    ));
    assert!(app.resource_install.busy);
    let _ = app.update(Message::ResourceInstall(
        resource_install::Message::Prepared(generation, Err("兼容性测试失败".into())),
    ));
    assert!(!app.resource_install.busy);
    assert!(app.resource_install.notice.contains("兼容性测试失败"));
    std::fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn appearance_controls_apply_theme_and_scale_and_are_reachable_at_minimum_size() {
    let (mut app, root) = fixture().await;
    app.viewport = Size::new(1024., 680.);
    let _ = app.update(Message::Navigate(Page::Settings));
    let _ = app.update(Message::Settings(crate::settings::Message::Tab(5)));
    settle(&mut app);
    assert!(click(&mut app, "保存外观")
        .iter()
        .any(|m| matches!(m, Message::Appearance(appearance::Message::Save))));
    assert!(click(&mut app, "选择背景图片")
        .iter()
        .any(|m| matches!(m, Message::Appearance(appearance::Message::Browse))));
    let mut config = app.appearance.saved.clone();
    config.scheme = appearance::Scheme::Midnight;
    config.scale = 120;
    let _ = app.update(Message::Appearance(appearance::Message::Saved(Ok(config))));
    assert_eq!(app.scale_factor(), 1.2);
    assert_ne!(
        app.theme().palette().background,
        crate::theme::launcher().palette().background
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
#[ignore = "writes instance, Mod, appearance and installation screenshots"]
async fn capture_instance_features() {
    let (mut app, root) = fixture().await;
    let directory = std::env::var("MEMORIA_UI_CAPTURE_DIR").unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        app.viewport = size;
        app.page = Page::Home;
        open(&mut app).await;
        simulator(&app)
            .snapshot(&app.theme())
            .unwrap()
            .matches_image(format!("{directory}/instance-settings-{}.png", size.width))
            .unwrap();
        for message in click(&mut app, "Mod 管理") {
            let _ = app.update(message);
        }
        simulator(&app)
            .snapshot(&app.theme())
            .unwrap()
            .matches_image(format!("{directory}/instance-mods-{}.png", size.width))
            .unwrap();
        let _ = app.update(Message::Dismiss);
        settle(&mut app);
        let _ = app.update(Message::Navigate(Page::Settings));
        let _ = app.update(Message::Settings(crate::settings::Message::Tab(5)));
        settle(&mut app);
        simulator(&app)
            .snapshot(&app.theme())
            .unwrap()
            .matches_image(format!("{directory}/appearance-{}.png", size.width))
            .unwrap();
        resource_fixture(&mut app);
        for message in click(&mut app, "安装此资源版本") {
            let _ = app.update(message);
        }
        settle(&mut app);
        simulator(&app)
            .snapshot(&app.theme())
            .unwrap()
            .matches_image(format!("{directory}/resource-install-{}.png", size.width))
            .unwrap();
        let _ = app.update(Message::Dismiss);
        settle(&mut app);
    }
    std::fs::remove_dir_all(root).unwrap();
}
