use super::*;
use std::{collections::HashMap, sync::Mutex};
fn mod_jar(id: &str, version: &str, dependencies: Value, breaks: Value) -> Vec<u8> {
    archive(&[(
        "fabric.mod.json",
        json!({"id":id,"version":version,"depends":dependencies,"breaks":breaks})
            .to_string()
            .as_bytes(),
    )])
}
async fn resolver_server() -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let mut routes = HashMap::<String, Vec<u8>>::new();
    let mut versions = HashMap::<String, Value>::new();
    for (project, id, bytes, deps) in [
        (
            "main",
            "mainV",
            mod_jar("main", "1", json!({"dep":"*"}), json!({})),
            json!([{"project_id":"dep","dependency_type":"required"}]),
        ),
        (
            "dep",
            "d2",
            mod_jar("dep", "2", json!({"shared":">=2"}), json!({})),
            json!([{"project_id":"shared","version_id":"b2","dependency_type":"required"}]),
        ),
        (
            "dep",
            "d1",
            mod_jar("dep", "1", json!({"shared":"<2"}), json!({})),
            json!([{"project_id":"shared","version_id":"b1","dependency_type":"required"}]),
        ),
        (
            "shared",
            "b2",
            mod_jar("shared", "2", json!({}), json!({})),
            json!([]),
        ),
        (
            "shared",
            "b1",
            mod_jar("shared", "1", json!({}), json!({})),
            json!([]),
        ),
        (
            "installed",
            "installedV",
            mod_jar("installed", "1", json!({"shared":"<2"}), json!({})),
            json!([{"project_id":"shared","version_id":"b1","dependency_type":"required"}]),
        ),
    ] {
        let value = json!({"project_id":project,"id":id,"name":id,"version_type":"release","date_published":if id.ends_with('2'){"2026-02-01"}else{"2026-01-01"},"game_versions":["1.21.1"],"loaders":["fabric"],"dependencies":deps,
            "files":[{"filename":format!("{id}.jar"),"url":format!("{base}/file/{id}"),"size":bytes.len(),"hashes":{"sha1":format!("{:x}",Sha1::digest(&bytes))},"primary":true}]});
        routes.insert(
            format!("/version/{id}"),
            serde_json::to_vec(&value).unwrap(),
        );
        routes.insert(format!("/file/{id}"), bytes);
        routes.insert(
            format!("/project/{project}"),
            json!({"id":project,"title":project,"project_type":"mod","client_side":"required"})
                .to_string()
                .into_bytes(),
        );
        versions.insert(id.into(), value);
    }
    routes.insert(
        "/project/dep/version".into(),
        serde_json::to_vec(&json!([versions["d2"], versions["d1"]])).unwrap(),
    );
    let routes = Arc::new(Mutex::new(routes));
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let routes = routes.clone();
            tokio::spawn(async move {
                let mut request = vec![];
                while !request.ends_with(b"\r\n\r\n") {
                    let Ok(byte) = stream.read_u8().await else {
                        return;
                    };
                    request.push(byte);
                    if request.len() > 8192 {
                        return;
                    }
                }
                let header = String::from_utf8_lossy(&request);
                let path = header
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .split('?')
                    .next()
                    .unwrap();
                let data = routes.lock().unwrap().get(path).cloned();
                let status = if data.is_some() { 200 } else { 404 };
                let data = data.unwrap_or_default();
                let _=stream.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",data.len()).as_bytes()).await;
                let _ = stream.write_all(&data).await;
            });
        }
    });
    Server { base, task }
}
fn installed(root: &std::path::Path, breaks: Value) -> Vec<u8> {
    let base = root.join("versions/test");
    let bytes = mod_jar("installed", "1", json!({"shared":"<2"}), breaks);
    fs::write(base.join("mods/installed.jar"), &bytes).unwrap();
    fs::create_dir_all(base.join(".memoria")).unwrap();
    fs::write(base.join(MANIFEST),json!({"version":1,"resources":[{"project":"installed","version":"installedV","path":"mods/installed.jar","sha256":digest(&bytes)}]}).to_string()).unwrap();
    bytes
}

#[tokio::test]
async fn preview_and_install_backtrack_the_whole_graph_and_retain_existing_mods() {
    let root = fixture();
    let existing = installed(&root, json!({}));
    let server = resolver_server().await;
    let preview = prepare_with(
        request(root.clone(), ResourceKind::Mod),
        client(),
        ResourceCatalog::with_base(server.base.clone()),
    )
    .await
    .unwrap();
    assert!(preview.files.iter().any(|file| file.path == "mods/d1.jar"));
    assert!(!preview.files.iter().any(|file| file.path == "mods/d2.jar"));
    assert!(preview.files.iter().any(|file| file.path == "mods/b1.jar"));
    install(preview, client(), progress()).await.unwrap();
    assert_eq!(
        fs::read(root.join("versions/test/mods/installed.jar")).unwrap(),
        existing
    );
    let inventory =
        crate::service::mod_resolver::inventory::inspect_instance(&root, "versions/test")
            .await
            .unwrap();
    assert_eq!(
        inventory.audit().status,
        crate::service::mod_resolver::Status::Resolved
    );
    let manifest: Value =
        serde_json::from_slice(&fs::read(root.join("versions/test").join(MANIFEST)).unwrap())
            .unwrap();
    assert!(manifest["resources"]
        .as_array()
        .unwrap()
        .iter()
        .any(|record| record["version"] == "d1"));
    fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn real_jar_conflicts_and_in_place_dependency_changes_are_rejected_before_writes() {
    let root = fixture();
    let existing = installed(&root, json!({"main":"*"}));
    let server = resolver_server().await;
    let error = prepare_with(
        request(root.clone(), ResourceKind::Mod),
        client(),
        ResourceCatalog::with_base(server.base.clone()),
    )
    .await
    .unwrap_err();
    assert!(error.contains("冲突"));
    assert_eq!(
        fs::read(root.join("versions/test/mods/installed.jar")).unwrap(),
        existing
    );
    assert!(!root.join("versions/test/mods/mainV.jar").exists());
    installed(&root, json!({}));
    let preview = prepare_with(
        request(root.clone(), ResourceKind::Mod),
        client(),
        ResourceCatalog::with_base(server.base.clone()),
    )
    .await
    .unwrap();
    fs::write(
        root.join("versions/test/mods/installed.jar"),
        mod_jar("installed", "9", json!({}), json!({})),
    )
    .unwrap();
    assert!(install(preview, client(), progress())
        .await
        .unwrap_err()
        .contains("变化"));
    assert!(!root.join("versions/test/mods/mainV.jar").exists());
    fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn inherited_environment_changes_invalidate_a_prepared_dependency_plan() {
    let root = fixture();
    installed(&root, json!({}));
    let server = resolver_server().await;
    let parent = root.join("versions/base");
    fs::create_dir_all(&parent).unwrap();
    let metadata =
        json!({"id":"base","clientVersion":"1.21.1","mainClass":"net.minecraft.client.main.Main"});
    fs::write(parent.join("base.json"), metadata.to_string()).unwrap();
    fs::write(parent.join("base.jar"), b"base").unwrap();
    fs::write(root.join("versions/test/test.json"),json!({"id":"test","inheritsFrom":"base","libraries":[{"name":"net.fabricmc:fabric-loader:0.16.0"}]}).to_string()).unwrap();
    let preview = prepare_with(
        request(root.clone(), ResourceKind::Mod),
        client(),
        ResourceCatalog::with_base(server.base.clone()),
    )
    .await
    .unwrap();
    let mut changed = metadata;
    changed["mainClass"] = json!("changed.Main");
    fs::write(parent.join("base.json"), changed.to_string()).unwrap();
    assert!(install(preview, client(), progress())
        .await
        .unwrap_err()
        .contains("环境已变化"));
    assert!(!root.join("versions/test/mods/mainV.jar").exists());
    fs::remove_dir_all(root).unwrap();
}
