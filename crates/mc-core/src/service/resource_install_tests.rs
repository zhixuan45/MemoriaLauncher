use super::*;
use std::io::Write;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
#[ignore = "requires live Modrinth metadata and CDN downloads"]
async fn live_resource_installation_uses_modrinth_files() {
    let root = fixture();
    let catalog = ResourceCatalog::new().unwrap();
    let project = catalog.metadata("/project/fabric-api").await.unwrap();
    let versions = catalog
        .versions(project["id"].as_str().unwrap(), "1.21.1", "fabric")
        .await
        .unwrap();
    let selected = versions
        .iter()
        .find(|v| v.version_type == "release")
        .or(versions.first())
        .unwrap();
    let mut request = request(root.clone(), ResourceKind::Mod);
    request.project = project["id"].as_str().unwrap().into();
    request.version = selected.id.clone();
    let preview = prepare_with(request, client(), catalog).await.unwrap();
    println!(
        "Live source: {} / {} ; {} files, {} bytes",
        project["id"],
        selected.id,
        preview.files.len(),
        preview.bytes
    );
    install(preview, client(), progress()).await.unwrap();
    let snapshot = super::super::desktop_instances::snapshot(&root, "versions/test").unwrap();
    assert!(!snapshot.mods.is_empty());
    assert!(snapshot.mods.iter().all(|m| m.enabled));
    fs::remove_dir_all(root).unwrap();
}
fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn jar(id: &str) -> Vec<u8> {
    archive(&[(
        "fabric.mod.json",
        format!(r#"{{"schemaVersion":1,"id":"{id}","version":"1.0"}}"#).as_bytes(),
    )])
}
fn fixture() -> PathBuf {
    let root = std::env::temp_dir().join(format!("memoria-install-{}", uuid::Uuid::new_v4()));
    let base = root.join("versions/test");
    fs::create_dir_all(base.join("mods")).unwrap();
    fs::write(base.join("test.json"),br#"{"id":"test","clientVersion":"1.21.1","mainClass":"net.minecraft.client.main.Main","assets":"1.21","libraries":[{"name":"net.fabricmc:fabric-loader:0.16.0"}]}"#).unwrap();
    fs::write(base.join("test.jar"), b"fixture").unwrap();
    root
}
struct Server {
    base: String,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn server(main: Vec<u8>, dep: Vec<u8>, kind: ResourceKind) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let origin = base.clone();
    let task = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = vec![];
            while !request.windows(4).any(|v| v == b"\r\n\r\n") {
                let mut buffer = [0; 4096];
                let n = socket.read(&mut buffer).await.unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..n]);
            }
            let header = String::from_utf8(request).unwrap();
            let path = header
                .split_whitespace()
                .nth(1)
                .unwrap()
                .split('?')
                .next()
                .unwrap();
            let version = |id: &str, project: &str, bytes: &[u8], deps: Value| json!({"id":id,"project_id":project,"name":project,"game_versions":["1.21.1"],"loaders":["fabric"],"files":[{"filename":if kind==ResourceKind::Modpack{"sample.mrpack"}else if kind==ResourceKind::Mod{if project=="main"{"main.jar"}else{"dep.jar"}}else{"sample.zip"},"url":format!("{origin}/file/{project}"),"size":bytes.len(),"hashes":{"sha1":format!("{:x}",Sha1::digest(bytes)),"sha512":format!("{:x}",Sha512::digest(bytes))},"primary":true}],"dependencies":deps});
            let body=match path{
            "/version/mainV"=>serde_json::to_vec(&version("mainV","main",&main,if kind==ResourceKind::Mod{json!([{"project_id":"dep","version_id":"depV","dependency_type":"required"}])}else{json!([])})).unwrap(),
            "/version/depV"=>serde_json::to_vec(&version("depV","dep",&dep,json!([]))).unwrap(),
            "/project/main"=>serde_json::to_vec(&json!({"id":"main","title":"Main","project_type":kind.as_str()})).unwrap(),
            "/project/dep"=>serde_json::to_vec(&json!({"id":"dep","title":"Dependency","project_type":"mod"})).unwrap(),
            "/file/main"=>main.clone(),"/file/dep"=>dep.clone(),_=>panic!("unexpected route {path}"),
        };
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            socket.write_all(&body).await.unwrap();
        }
    });
    Server { base, task }
}
fn request(root: PathBuf, kind: ResourceKind) -> Request {
    Request {
        root,
        instance: "versions/test".into(),
        kind,
        project: "main".into(),
        version: "mainV".into(),
        optional: false,
    }
}
fn client() -> DownloadClient {
    DownloadClient::new(Default::default()).unwrap()
}
fn progress() -> ProgressCallback {
    Arc::new(|_| {})
}
#[tokio::test]
async fn required_dependency_is_previewed_and_installed_with_verified_source_receipt() {
    let root = fixture();
    let main = jar("main");
    let dep = jar("dep");
    let server = server(main.clone(), dep.clone(), ResourceKind::Mod).await;
    let preview = prepare_with(
        request(root.clone(), ResourceKind::Mod),
        client(),
        ResourceCatalog::with_base(server.base.clone()),
    )
    .await
    .unwrap();
    assert_eq!(preview.files.len(), 3);
    assert!(!root.join("versions/test/mods/main.jar").exists());
    let result = install(preview, client(), progress()).await.unwrap();
    assert!(result.contains("安装完成"));
    assert_eq!(
        fs::read(root.join("versions/test/mods/main.jar")).unwrap(),
        main
    );
    assert_eq!(
        fs::read(root.join("versions/test/mods/dep.jar")).unwrap(),
        dep
    );
    let record: Value = serde_json::from_slice(
        &fs::read(root.join("versions/test/.memoria/resource-install.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(record["resources"][0]["version"], "mainV");
    assert!(record["resources"][0]["sha256"].as_str().unwrap().len() == 64);
    fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn stale_preview_hash_failure_and_commit_failure_leave_existing_content_intact() {
    let root = fixture();
    let base = root.join("versions/test");
    let main = jar("main");
    let server = server(main.clone(), jar("dep"), ResourceKind::Mod).await;
    let plan = || {
        prepare_with(
            request(root.clone(), ResourceKind::Mod),
            client(),
            ResourceCatalog::with_base(server.base.clone()),
        )
    };
    let preview = plan().await.unwrap();
    fs::write(base.join("mods/external.jar"), b"external").unwrap();
    assert!(install(preview, client(), progress())
        .await
        .unwrap_err()
        .contains("变化"));
    assert!(!base.join("mods/main.jar").exists());
    fs::write(base.join("mods/main.jar"), b"old user file").unwrap();
    let mut preview = plan().await.unwrap();
    preview.fail_commit_at = Some(1);
    assert!(install(preview, client(), progress())
        .await
        .unwrap_err()
        .contains("回滚"));
    assert_eq!(
        fs::read(base.join("mods/main.jar")).unwrap(),
        b"old user file"
    );
    assert!(!base.join("mods/dep.jar").exists());
    let mut preview = plan().await.unwrap();
    preview.files[0].sha1 = "0".repeat(40);
    assert!(install(preview, client(), progress())
        .await
        .unwrap_err()
        .contains("校验失败"));
    assert_eq!(
        fs::read(base.join("mods/main.jar")).unwrap(),
        b"old user file"
    );
    fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn resourcepacks_and_shaders_use_the_correct_instance_directories() {
    for (kind, folder) in [
        (ResourceKind::Resourcepack, "resourcepacks"),
        (ResourceKind::Shader, "shaderpacks"),
    ] {
        let root = fixture();
        let bytes = archive(&[("pack.mcmeta", b"{}")]);
        let server = server(bytes.clone(), vec![], kind).await;
        let preview = prepare_with(
            request(root.clone(), kind),
            client(),
            ResourceCatalog::with_base(server.base.clone()),
        )
        .await
        .unwrap();
        install(preview, client(), progress()).await.unwrap();
        assert_eq!(
            fs::read(root.join(format!("versions/test/{folder}/sample.zip"))).unwrap(),
            bytes
        );
        fs::remove_dir_all(root).unwrap();
    }
}
#[test]
fn mrpack_uses_client_overrides_and_checks_paths_runtime_and_optional_files() {
    let index = json!({"formatVersion":1,"game":"minecraft","dependencies":{"minecraft":"1.21.1","fabric-loader":"0.16.0"},"files":[{"path":"mods/optional.jar","fileSize":1,"hashes":{"sha1":"0".repeat(40)},"downloads":["https://cdn.modrinth.com/optional.jar"],"env":{"client":"optional"}},{"path":"mods/server.jar","env":{"client":"unsupported"}}]});
    let data = serde_json::to_vec(&index).unwrap();
    let bytes = archive(&[
        ("modrinth.index.json", &data),
        ("overrides/config/test.json", b"shared"),
        ("client-overrides/config/test.json", b"client"),
        ("server-overrides/config/test.json", b"server"),
    ]);
    let (files, warnings) = unpack(&bytes, "1.21.1", "fabric", "0.16.0", false).unwrap();
    assert_eq!(files.len(), 1);
    assert_eq!(files[0].inline.as_deref(), Some(b"client".as_slice()));
    assert!(!warnings.is_empty());
    assert_eq!(
        unpack(&bytes, "1.21.1", "fabric", "0.16.0", true)
            .unwrap()
            .0
            .len(),
        2
    );
    assert!(unpack(&bytes, "1.21.1", "fabric", "0.15.0", false).is_err());
    let unsafe_pack = archive(&[
        ("modrinth.index.json", &data),
        ("overrides/../outside", b"bad"),
    ]);
    assert!(unpack(&unsafe_pack, "1.21.1", "fabric", "0.16.0", false).is_err());
    let protected = archive(&[
        ("modrinth.index.json", &data),
        ("overrides/.memoria/instance-settings.json", b"bad"),
    ]);
    assert!(unpack(&protected, "1.21.1", "fabric", "0.16.0", false).is_err());
}
#[tokio::test]
async fn mrpack_installs_overrides_into_a_matching_target() {
    let root = fixture();
    let data=serde_json::to_vec(&json!({"formatVersion":1,"game":"minecraft","dependencies":{"minecraft":"1.21.1","fabric-loader":"0.16.0"},"files":[]})).unwrap();
    let bytes = archive(&[
        ("modrinth.index.json", &data),
        ("overrides/config/test.json", b"{\"enabled\":true}"),
    ]);
    let server = server(bytes, vec![], ResourceKind::Modpack).await;
    let preview = prepare_with(
        request(root.clone(), ResourceKind::Modpack),
        client(),
        ResourceCatalog::with_base(server.base.clone()),
    )
    .await
    .unwrap();
    install(preview, client(), progress()).await.unwrap();
    assert_eq!(
        fs::read(root.join("versions/test/config/test.json")).unwrap(),
        b"{\"enabled\":true}"
    );
    fs::remove_dir_all(root).unwrap();
}
