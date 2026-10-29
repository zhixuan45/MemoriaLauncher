use super::*;
use crate::{
    instance_wizard::{self, Choice},
    pack_export,
};
use iced::futures::StreamExt;
use std::{
    collections::HashMap,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
struct Fixture {
    directory: PathBuf,
    root: PathBuf,
    java: PathBuf,
    server: tokio::task::JoinHandle<()>,
    client: mc_core::DownloadClient,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
#[cfg(unix)]
impl Fixture {
    async fn new() -> Self {
        use std::os::unix::fs::PermissionsExt;
        let directory =
            std::env::temp_dir().join(format!("memoria-pack-ui-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let root = directory.join("game");
        let java = directory.join("java");
        std::fs::write(
            &java,
            b"#!/bin/sh\necho 'java.version = 17.0.1'\necho 'os.arch = amd64'\n",
        )
        .unwrap();
        std::fs::set_permissions(&java, std::fs::Permissions::from_mode(0o700)).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let routes = Arc::new(Mutex::new(HashMap::<String, Vec<u8>>::new()));
        let index = b"{\"objects\":{}}";
        let index_hash = sha1_fixture(index);
        let metadata=serde_json::json!({"id":"1.20.1","type":"release","mainClass":"fixture.Main","javaVersion":{"majorVersion":17},"assets":"test","libraries":[],"downloads":{"client":{"url":"https://piston-data.mojang.com/client.jar","sha1":sha1_fixture(b"vanilla"),"size":7}},"assetIndex":{"id":"test","url":"https://piston-meta.mojang.com/index.json","sha1":index_hash,"size":index.len()}}).to_string();
        routes.lock().unwrap().insert("/mc/game/version_manifest_v2.json".into(),serde_json::json!({"versions":[{"id":"1.20.1","url":"https://piston-meta.mojang.com/version.json","sha1":sha1_fixture(metadata.as_bytes())}]}).to_string().into_bytes());
        routes
            .lock()
            .unwrap()
            .insert("/version.json".into(), metadata.into_bytes());
        routes
            .lock()
            .unwrap()
            .insert("/client.jar".into(), b"vanilla".to_vec());
        routes
            .lock()
            .unwrap()
            .insert("/index.json".into(), index.to_vec());
        let server = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let routes = routes.clone();
                tokio::spawn(async move {
                    let mut header = vec![];
                    while !header.ends_with(b"\r\n\r\n") {
                        let Ok(byte) = stream.read_u8().await else {
                            return;
                        };
                        header.push(byte);
                        if header.len() > 8192 {
                            return;
                        }
                    }
                    let text = String::from_utf8_lossy(&header);
                    let path = text.split_whitespace().nth(1).unwrap();
                    let data = routes.lock().unwrap().get(path).cloned();
                    let status = if data.is_some() { 200 } else { 404 };
                    let data = data.unwrap_or_default();
                    let _=stream.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",data.len()).as_bytes()).await;
                    let _ = stream.write_all(&data).await;
                });
            }
        });
        let client = mc_core::DownloadClient::new(mc_core::DownloadSettings {
            source: mc_core::DownloadSource::Custom,
            custom_url: address,
            ..Default::default()
        })
        .unwrap();
        Self {
            directory,
            root,
            java,
            server,
            client,
        }
    }
    async fn app(&self, size: Size) -> Launcher {
        let (mut app, _) = Launcher::new();
        app.environment.game_root = self.root.clone();
        app.environment.game_roots = vec![self.root.clone()];
        app.environment.java = self.java.clone();
        app.download.client = Ok(self.client.clone());
        app.viewport = size;
        app.settings.saved.check_updates = false;
        let _ = app.update(Message::Loaded(Ok(crate::local::scan(self.root.clone())
            .await
            .unwrap())));
        app
    }
}
fn sha1_fixture(bytes: &[u8]) -> String {
    use sha1::{Digest, Sha1};
    format!("{:x}", Sha1::digest(bytes))
}

fn settle(app: &mut Launcher) {
    let _ = app.update(Message::Animate(
        app.animation_time + crate::motion::OPEN_DURATION + crate::motion::PAGE_DURATION,
    ));
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
async fn action(app: &mut Launcher, message: Message) {
    let task = app.update(message);
    complete(app, task).await;
    settle(app);
}
fn pack() -> Vec<u8> {
    let mut archive = zip::ZipWriter::new(std::io::Cursor::new(vec![]));
    let index = serde_json::json!({"formatVersion":1,"game":"minecraft","name":"UI imported pack","versionId":"1","dependencies":{"minecraft":"1.20.1"},"files":[]});
    for (name, bytes) in [
        ("modrinth.index.json", index.to_string().into_bytes()),
        ("overrides/config/demo.json", b"{\"enabled\":true}".to_vec()),
    ] {
        archive
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&bytes).unwrap();
    }
    archive.finish().unwrap().into_inner()
}
#[cfg(unix)]
#[tokio::test]
async fn new_instance_and_mrpack_import_controls_execute_real_install_tasks_then_export_and_verify()
{
    let fixture = Fixture::new().await;
    let mut app = fixture.app(Size::new(1024., 680.)).await;
    app.page = Page::Download;
    settle(&mut app);
    for message in click(&mut app, "新建实例 / 导入整合包") {
        action(&mut app, message).await;
    }
    assert_eq!(app.modal, Some(crate::app::Modal::InstanceWizard));
    action(
        &mut app,
        Message::InstanceWizard(instance_wizard::Message::Kind(Choice::Vanilla)),
    )
    .await;
    action(
        &mut app,
        Message::InstanceWizard(instance_wizard::Message::Minecraft("1.20.1".into())),
    )
    .await;
    action(
        &mut app,
        Message::InstanceWizard(instance_wizard::Message::Target("standalone".into())),
    )
    .await;
    for message in click(&mut app, "生成新实例预览") {
        action(&mut app, message).await;
    }
    assert!(
        app.instance_wizard.plan.is_some(),
        "{}",
        app.instance_wizard.notice
    );
    assert!(!fixture.root.exists());
    for message in click(&mut app, "确认安装新实例") {
        action(&mut app, message).await;
    }
    assert!(fixture
        .root
        .join("versions/standalone/standalone.json")
        .exists());
    assert!(!app.instance_wizard.installing);
    assert_eq!(app.active, "versions/standalone");
    action(&mut app, Message::OpenInstanceWizard).await;
    let path = fixture.directory.join("input.mrpack");
    std::fs::write(&path, pack()).unwrap();
    // File picker selection is supplied explicitly; parsing and all installation tasks run normally.
    let _ = app.update(Message::InstanceWizard(instance_wizard::Message::PickPack));
    let g = app.instance_wizard.generation;
    action(
        &mut app,
        Message::InstanceWizard(instance_wizard::Message::PackPicked(g, Some(path))),
    )
    .await;
    assert_eq!(app.instance_wizard.choice, Choice::Vanilla);
    action(
        &mut app,
        Message::InstanceWizard(instance_wizard::Message::Target("imported".into())),
    )
    .await;
    for message in click(&mut app, "生成新实例预览") {
        action(&mut app, message).await;
    }
    for message in click(&mut app, "确认安装新实例") {
        action(&mut app, message).await;
    }
    assert!(
        fixture
            .root
            .join("versions/imported/config/demo.json")
            .exists(),
        "{}",
        app.instance_wizard.notice
    );
    assert_eq!(app.active, "versions/imported");
    action(&mut app, Message::Dismiss).await;
    action(&mut app, Message::Navigate(Page::Home)).await;
    action(
        &mut app,
        Message::SelectInstance("versions/imported".into()),
    )
    .await;
    action(&mut app, Message::EditInstance).await;
    for message in click(&mut app, "锁定与导出") {
        action(&mut app, message).await;
    }
    for message in click(&mut app, "生成或刷新锁定文件") {
        action(&mut app, message).await;
    }
    for message in click(&mut app, "验证现有锁定") {
        action(&mut app, message).await;
    }
    assert!(app.pack_export.notice.contains("验证通过"));
    for message in click(&mut app, "包含配置与脚本（请检查其中的私人设置）") {
        action(&mut app, message).await;
    }
    for message in click(&mut app, "生成导出预览") {
        action(&mut app, message).await;
    }
    assert!(
        app.pack_export.export.is_some(),
        "{}",
        app.pack_export.notice
    );
    let output = fixture.directory.join("output.mrpack");
    let _ = app.update(Message::PackExport(pack_export::Message::SaveExport));
    let g = app.pack_export.generation;
    action(
        &mut app,
        Message::PackExport(pack_export::Message::ExportPicked(g, Some(output.clone()))),
    )
    .await;
    let parsed = mc_core::service::packs::exchange::read_pack(&output).unwrap();
    assert_eq!(parsed.minecraft, "1.20.1");
}
#[tokio::test]
async fn standalone_loader_and_pack_export_controls_are_reachable_and_blocked_during_other_writes()
{
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        let mut app = populated().await;
        app.viewport = size;
        action(&mut app, Message::OpenInstanceWizard).await;
        assert!(simulator(&app).find("查询加载器版本").is_ok());
        assert!(simulator(&app).find("导入 .mrpack").is_ok());
        assert_eq!(app.instance_wizard.choice, Choice::Fabric);
        let generation = app.instance_wizard.generation;
        let _ = app.update(Message::InstanceWizard(instance_wizard::Message::Versions(
            generation + 1,
            Ok(vec![]),
        )));
        assert!(app.instance_wizard.version.is_none());
        app.instance_wizard.installing = true;
        assert_eq!(app.update(Message::Launch).units(), 0);
        assert_eq!(app.update(Message::OpenPackExport).units(), 0);
        app.instance_wizard.installing = false;
        action(&mut app, Message::Dismiss).await;
        action(&mut app, Message::OpenPackExport).await;
        assert!(simulator(&app).find("生成导出预览").is_ok());
        assert!(!app.pack_export.configs);
    }
}
#[cfg(unix)]
#[tokio::test]
#[ignore = "writes instance wizard and pack export screenshots"]
async fn capture_pack_workflows() {
    let directory = std::env::var("MEMORIA_UI_CAPTURE_DIR").unwrap();
    std::fs::create_dir_all(&directory).unwrap();
    let fixture = Fixture::new().await;
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        let mut app = fixture.app(size).await;
        action(&mut app, Message::OpenInstanceWizard).await;
        simulator(&app)
            .snapshot(&app.theme())
            .unwrap()
            .matches_image(format!(
                "{directory}/new-loader-instance-{}.png",
                size.width
            ))
            .unwrap();
    }
    let pack = mc_core::service::packs::exchange::parse(&pack()).unwrap();
    let request = mc_core::service::packs::NewInstanceRequest {
        root: fixture.root.clone(),
        target: "export-fixture".into(),
        minecraft: pack.minecraft.clone(),
        loader: pack.loader.clone(),
        java: fixture.java.clone(),
        pack: Some(pack),
        optional: false,
    };
    let plan = mc_core::service::packs::preview_new_instance(request, fixture.client.clone())
        .await
        .unwrap();
    mc_core::service::packs::install_new_instance(plan, fixture.client.clone(), Arc::new(|_| {}))
        .await
        .unwrap();
    for size in [Size::new(1024., 680.), Size::new(1536., 921.)] {
        let mut app = fixture.app(size).await;
        action(
            &mut app,
            Message::SelectInstance("versions/export-fixture".into()),
        )
        .await;
        action(&mut app, Message::OpenPackExport).await;
        action(
            &mut app,
            Message::PackExport(pack_export::Message::Configs(true)),
        )
        .await;
        action(
            &mut app,
            Message::PackExport(pack_export::Message::ExportPreview),
        )
        .await;
        simulator(&app)
            .snapshot(&app.theme())
            .unwrap()
            .matches_image(format!("{directory}/pack-export-{}.png", size.width))
            .unwrap();
    }
}
