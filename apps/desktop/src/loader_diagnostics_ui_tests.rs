use super::*;
use crate::{diagnostics::Message as LogMessage, loader_install::Message as LoaderMessage};
use iced::futures::StreamExt;
use std::path::PathBuf;

async fn fixture(size: Size) -> (Launcher, PathBuf) {
    let root = std::env::temp_dir().join(format!("memoria-loader-ui-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("versions/test/logs")).unwrap();
    std::fs::write(root.join("versions/test/test.json"),br#"{"id":"test","mainClass":"net.minecraft.client.main.Main","assets":"test","libraries":[]}"#).unwrap();
    std::fs::write(root.join("versions/test/test.jar"), b"fixture").unwrap();
    std::fs::write(
        root.join("versions/test/logs/latest.log"),
        "OutOfMemoryError: Java heap space\naccess_token=private\n",
    )
    .unwrap();
    let (mut app, _) = Launcher::new();
    app.viewport = size;
    app.settings = Default::default();
    app.environment.game_root = root.clone();
    app.loading = false;
    let data = crate::local::scan(root.clone()).await.unwrap();
    let _ = app.update(Message::Loaded(Ok(data)));
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::PAGE_DURATION,
    ));
    (app, root)
}
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
fn settle(app: &mut Launcher) {
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::OPEN_DURATION,
    ));
}

#[tokio::test]
async fn loader_controls_work_at_both_sizes_and_ignore_stale_queries_and_block_concurrent_writes() {
    use mc_core::runtime::loaders::{Kind, Version};
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        let (mut app, root) = fixture(size).await;
        for message in click(&mut app, "安装加载器") {
            let _ = app.update(message);
        }
        settle(&mut app);
        assert_eq!(app.modal, Some(crate::app::Modal::LoaderInstall));
        assert_eq!(app.loader_install.bases.len(), 1);
        let messages = click(&mut app, "查询兼容版本");
        assert!(messages
            .iter()
            .any(|m| matches!(m, Message::LoaderInstall(LoaderMessage::Query))));
        for message in messages {
            let _ = app.update(message);
        }
        let old = app.loader_install.generation;
        let _ = app.update(Message::LoaderInstall(LoaderMessage::Kind(Kind::Quilt)));
        let versions = vec![Version {
            version: "0.26.0".into(),
            stable: true,
        }];
        let _ = app.update(Message::LoaderInstall(LoaderMessage::Versions(
            old,
            Ok(versions.clone()),
        )));
        assert!(app.loader_install.versions.is_empty());
        let generation = app.loader_install.generation;
        let _ = app.update(Message::LoaderInstall(LoaderMessage::Versions(
            generation,
            Ok(versions),
        )));
        let messages = click(&mut app, "生成加载器安装预览");
        assert!(messages
            .iter()
            .any(|m| matches!(m, Message::LoaderInstall(LoaderMessage::Preview))));
        for message in messages {
            let _ = app.update(message);
        }
        let generation = app.loader_install.generation;
        let _ = app.update(Message::LoaderInstall(LoaderMessage::Prepared(
            generation,
            Err("official endpoint unavailable".into()),
        )));
        assert!(!app.loader_install.busy);
        app.loader_install.installing = true;
        assert!(app.instance_writing());
        assert_eq!(app.update(Message::Launch).units(), 0);
        let old_root = app.environment.game_root.clone();
        let _ = app.update(Message::GameSettings(crate::game_settings::Message::Save));
        assert_eq!(app.environment.game_root, old_root);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn logs_parse_locally_through_actual_tasks_and_an_old_reply_cannot_cross_instances() {
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        let (mut app, root) = fixture(size).await;
        let messages = click(&mut app, "日志诊断");
        for message in messages {
            let task = app.update(message);
            complete(&mut app, task).await;
        }
        settle(&mut app);
        assert_eq!(app.modal, Some(crate::app::Modal::Diagnostics));
        let analysis = app.diagnostics.analysis.clone().unwrap();
        assert_eq!(analysis.findings[0].category, "out_of_memory");
        assert!(!serde_json::to_string(&analysis)
            .unwrap()
            .contains("private"));
        simulator(&app).find("内存不足或分配失败").unwrap();
        assert!(click(&mut app, "导入文本日志")
            .iter()
            .any(|m| matches!(m, Message::Diagnostics(LogMessage::Import))));
        let generation = app.diagnostics.generation;
        let task = app
            .diagnostics
            .open(root.clone(), "versions/other".into(), None, None);
        drop(task);
        let _ = app.update(Message::Diagnostics(LogMessage::Analyzed(
            generation,
            Ok(analysis),
        )));
        assert!(app.diagnostics.analysis.is_none());
        let input = root.join("imported.log");
        std::fs::write(
            &input,
            "UnsupportedClassVersionError: class file version 65.0\n",
        )
        .unwrap();
        let generation = app.diagnostics.generation;
        let task = app.update(Message::Diagnostics(LogMessage::Picked(
            generation,
            Some(input),
        )));
        complete(&mut app, task).await;
        assert_eq!(
            app.diagnostics.analysis.as_ref().unwrap().findings[0].category,
            "java_version"
        );
        assert!(!app.ai.busy);
        assert!(app.ai.crash_log.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn game_exit_automatically_analyzes_the_exited_instances_log_without_sending_it() {
    let (mut app, root) = fixture(Size::new(1024., 680.)).await;
    let dir = root.join("versions/test/.launcher/logs");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("run.log"), "MixinApplyError: demo.mixin\n").unwrap();
    let exit = crate::local::GameExit {
        game_root: root.clone(),
        instance_id: "versions/test".into(),
        exit: mc_core::ProcessExit {
            success: false,
            code: Some(1),
            log_path: dir.join("run.log"),
            log_error: None,
        },
    };
    let task = app.update(Message::GameFinished(Ok(exit)));
    complete(&mut app, task).await;
    settle(&mut app);
    assert_eq!(app.modal, Some(crate::app::Modal::Crash));
    let findings = &app.diagnostics.analysis.as_ref().unwrap().findings;
    assert!(findings.iter().any(|f| f.category == "mixin_conflict"));
    assert!(findings.iter().any(|f| f.category == "nonzero_exit"));
    assert!(!app.ai.busy);
    assert!(app.ai.crash_log.is_none());
    assert!(click(&mut app, "启用 AI 分析")
        .iter()
        .any(|m| matches!(m, Message::AnalyzeCrash)));
    assert!(click(&mut app, "暂不分析")
        .iter()
        .any(|m| matches!(m, Message::Dismiss)));
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
#[ignore = "writes loader and diagnostics screenshots; set MEMORIA_UI_CAPTURE_DIR"]
async fn capture_loader_and_diagnostics() {
    let directory = std::env::var("MEMORIA_UI_CAPTURE_DIR").unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        let (mut app, root) = fixture(size).await;
        let _ = app.update(Message::OpenLoader);
        settle(&mut app);
        simulator(&app)
            .snapshot(&app.theme())
            .unwrap()
            .matches_image(format!("{directory}/loader-{}.png", size.width))
            .unwrap();
        let task = app.update(Message::OpenDiagnostics);
        complete(&mut app, task).await;
        settle(&mut app);
        simulator(&app)
            .snapshot(&app.theme())
            .unwrap()
            .matches_image(format!("{directory}/diagnostics-{}.png", size.width))
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
