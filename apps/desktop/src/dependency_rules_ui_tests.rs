use super::*;
use crate::{diagnostics::Message as LogMessage, instance_settings::Message as InstanceMessage};
use iced::futures::StreamExt;
use std::{io::Write, path::PathBuf};
fn jar(json: &str) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(vec![]));
    zip.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(json.as_bytes()).unwrap();
    zip.finish().unwrap().into_inner()
}
async fn fixture(size: Size) -> (Launcher, PathBuf) {
    let root = std::env::temp_dir().join(format!("memoria-dependency-ui-{}", uuid::Uuid::new_v4()));
    let base = root.join("versions/test");
    std::fs::create_dir_all(base.join("mods")).unwrap();
    std::fs::create_dir_all(base.join("logs")).unwrap();
    std::fs::write(base.join("test.json"),br#"{"id":"test","clientVersion":"1.21.1","mainClass":"net.minecraft.client.main.Main","javaVersion":{"majorVersion":21},"libraries":[{"name":"net.fabricmc:fabric-loader:0.16.0"}]}"#).unwrap();
    std::fs::write(base.join("test.jar"), b"fixture").unwrap();
    std::fs::write(
        base.join("mods/main.jar"),
        jar(r#"{"id":"main","version":"1","depends":{"prerequisite":">=2"}}"#),
    )
    .unwrap();
    std::fs::write(
        base.join("logs/latest.log"),
        "java.util.zip.ZipException: zip END header not found\n",
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
async fn mod_checks_and_candidate_solving_work_at_both_sizes_without_changing_instances() {
    use mc_core::service::mod_resolver::Status;
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        let (mut app, root) = fixture(size).await;
        let original = std::fs::read(root.join("versions/test/mods/main.jar")).unwrap();
        let task = app.update(Message::EditInstance);
        complete(&mut app, task).await;
        settle(&mut app);
        for message in click(&mut app, "Mod 管理") {
            let _ = app.update(message);
        }
        let messages = click(&mut app, "检查前置与冲突");
        assert!(messages.iter().any(|message| matches!(
            message,
            Message::InstanceSettings(InstanceMessage::CheckDependencies)
        )));
        for message in messages {
            let task = app.update(message);
            complete(&mut app, task).await;
        }
        assert_eq!(
            app.instance_settings
                .dependency_report
                .as_ref()
                .unwrap()
                .status,
            Status::Conflict
        );
        simulator(&app).find("前置或冲突约束不满足").unwrap();
        assert!(click(&mut app, "选择候选 JAR 求解")
            .iter()
            .any(|message| matches!(
                message,
                Message::InstanceSettings(InstanceMessage::ImportCandidates)
            )));
        let path = root.join("candidate.jar");
        std::fs::write(&path, jar(r#"{"id":"prerequisite","version":"2.1.0"}"#)).unwrap();
        let generation = app.instance_settings.generation;
        let task = app.update(Message::InstanceSettings(InstanceMessage::CandidateFiles(
            generation,
            vec![path],
        )));
        complete(&mut app, task).await;
        let report = app.instance_settings.dependency_report.clone().unwrap();
        assert_eq!(report.status, Status::Resolved);
        assert!(report
            .changes
            .iter()
            .any(|change| change.id == "prerequisite" && change.action == "add"));
        assert_eq!(
            std::fs::read_dir(root.join("versions/test/mods"))
                .unwrap()
                .count(),
            1
        );
        assert_eq!(
            std::fs::read(root.join("versions/test/mods/main.jar")).unwrap(),
            original
        );
        let _ = app
            .instance_settings
            .open(root.clone(), "versions/other".into());
        let _ = app.update(Message::InstanceSettings(
            InstanceMessage::DependenciesChecked(generation, Ok(report)),
        ));
        assert!(app.instance_settings.dependency_report.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
async fn diagnostic_rule_controls_and_new_builtin_matches_are_reachable_at_both_sizes() {
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        let (mut app, root) = fixture(size).await;
        let task = app.update(Message::OpenDiagnostics);
        complete(&mut app, task).await;
        settle(&mut app);
        let report = app.diagnostics.analysis.as_ref().unwrap();
        assert!(report.rules_applied >= 19);
        assert!(report
            .findings
            .iter()
            .any(|finding| finding.category == "archive_corrupt"));
        assert!(click(&mut app, "导入诊断规则")
            .iter()
            .any(|message| matches!(message, Message::Diagnostics(LogMessage::ImportRules))));
        assert!(click(&mut app, "恢复内置规则")
            .iter()
            .any(|message| matches!(message, Message::Diagnostics(LogMessage::RestoreRules))));
        simulator(&app).find("JAR/ZIP 文件损坏").unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[tokio::test]
#[ignore = "writes dependency and rule matching screenshots; set MEMORIA_UI_CAPTURE_DIR"]
async fn capture_dependency_and_rule_matching() {
    let directory = std::env::var("MEMORIA_UI_CAPTURE_DIR").unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        let (mut app, root) = fixture(size).await;
        let task = app.update(Message::EditInstance);
        complete(&mut app, task).await;
        settle(&mut app);
        let _ = app.update(Message::InstanceSettings(InstanceMessage::Tab(true)));
        let task = app.update(Message::InstanceSettings(
            InstanceMessage::CheckDependencies,
        ));
        complete(&mut app, task).await;
        simulator(&app)
            .snapshot(&app.theme())
            .unwrap()
            .matches_image(format!(
                "{directory}/dependency-conflict-{}.png",
                size.width
            ))
            .unwrap();
        let task = app.update(Message::OpenDiagnostics);
        complete(&mut app, task).await;
        settle(&mut app);
        simulator(&app)
            .snapshot(&app.theme())
            .unwrap()
            .matches_image(format!("{directory}/rule-matching-{}.png", size.width))
            .unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
