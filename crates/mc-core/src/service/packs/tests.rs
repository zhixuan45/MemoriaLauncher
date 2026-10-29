use super::*;
use crate::{runtime::loaders::Catalog, DownloadClient, DownloadSettings, DownloadSource};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use sha2::Sha512;
use std::{
    collections::HashMap,
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
struct Server {
    base: String,
    routes: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    requests: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Server {
    async fn new() -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let routes = Arc::new(Mutex::new(HashMap::<String, Vec<u8>>::new()));
        let shared = routes.clone();
        let requests = Arc::new(Mutex::new(vec![]));
        let log = requests.clone();
        let task = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let shared = shared.clone();
                let log = log.clone();
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
                    let path = text.split_whitespace().nth(1).unwrap().to_owned();
                    log.lock().unwrap().push(path.clone());
                    let body = shared.lock().unwrap().get(&path).cloned();
                    let status = if body.is_some() { 200 } else { 404 };
                    let body = body.unwrap_or_default();
                    let _=stream.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).as_bytes()).await;
                    let _ = stream.write_all(&body).await;
                });
            }
        });
        Self {
            base,
            routes,
            requests,
            task,
        }
    }
    fn put(&self, path: &str, bytes: impl Into<Vec<u8>>) {
        self.routes
            .lock()
            .unwrap()
            .insert(path.into(), bytes.into());
    }
    fn client(&self) -> DownloadClient {
        DownloadClient::new(DownloadSettings {
            source: DownloadSource::Custom,
            custom_url: self.base.clone(),
            ..Default::default()
        })
        .unwrap()
    }
    fn vanilla(&self) {
        let index = br#"{"objects":{}}"#;
        let metadata=json!({"id":"1.20.1","type":"release","mainClass":"net.minecraft.client.main.Main","javaVersion":{"majorVersion":17},"assets":"test-index","assetIndex":{"id":"test-index","url":"https://piston-meta.mojang.com/index.json","sha1":sha1(index),"size":index.len()},"downloads":{"client":{"url":"https://piston-data.mojang.com/client.jar","sha1":sha1(b"vanilla"),"size":7}},"libraries":[],"arguments":{"game":[],"jvm":[]}}).to_string();
        self.put("/mc/game/version_manifest_v2.json",json!({"versions":[{"id":"1.20.1","url":"https://piston-meta.mojang.com/version.json","sha1":sha1(metadata.as_bytes())}]}).to_string());
        self.put("/version.json", metadata);
        self.put("/index.json", index.to_vec());
        self.put("/client.jar", b"vanilla".to_vec());
    }
    fn loader(&self, kind: Kind) -> (Loader, Value, String) {
        let (group, artifact, version, api) = match kind {
            Kind::Fabric => ("net.fabricmc", "fabric-loader", "0.16.10", "fabric"),
            Kind::Quilt => ("org.quiltmc", "quilt-loader", "0.16.10", "quilt"),
            Kind::Forge => ("net.minecraftforge", "forge", "1.20.1-47.4.0", "forge"),
            Kind::NeoForge => ("net.neoforged", "neoforge", "20.1.1", "neo"),
        };
        let path = format!(
            "{}/{artifact}/{version}/{artifact}-{version}.jar",
            group.replace('.', "/")
        );
        let profile = json!({"id":"official-profile","inheritsFrom":"1.20.1","mainClass":"fixture.Main","libraries":[{"name":format!("{group}:{artifact}:{version}"),"downloads":{"artifact":{"path":path,"url":format!("{}/repo/{path}",self.base),"sha1":sha1(b"loader"),"size":6}}}]});
        self.put(&format!("/repo/{path}"), b"loader".to_vec());
        if matches!(kind, Kind::Fabric | Kind::Quilt) {
            self.put(
                &format!("/{api}/versions/loader/1.20.1"),
                json!([{"loader":{"version":version,"stable":true}}]).to_string(),
            );
            self.put(
                &format!("/{api}/versions/loader/1.20.1/{version}/profile/json"),
                profile.to_string(),
            );
        } else {
            let maven = format!("/{api}/{}/{artifact}", group.replace('.', "/"));
            self.put(&format!("{maven}/maven-metadata.xml"),format!("<metadata><versioning><versions><version>{version}</version></versions></versioning></metadata>"));
            let installer = zip(&[("version.json", profile.to_string().as_bytes())]);
            self.put(
                &format!("{maven}/{version}/{artifact}-{version}-installer.jar.sha1"),
                sha1(&installer),
            );
            self.put(
                &format!("{maven}/{version}/{artifact}-{version}-installer.jar"),
                installer,
            );
        }
        (
            Loader {
                kind,
                version: if kind == Kind::Forge {
                    "47.4.0".into()
                } else {
                    version.into()
                },
            },
            profile,
            path,
        )
    }
    fn catalog(&self) -> Catalog {
        Catalog::new(self.client()).fixture_base(&self.base)
    }
}
fn sha1(bytes: &[u8]) -> String {
    format!("{:x}", Sha1::digest(bytes))
}
fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
    for (path, bytes) in entries {
        zip.start_file(*path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn jar(id: &str, dependencies: Value) -> Vec<u8> {
    zip(&[(
        "fabric.mod.json",
        json!({"schemaVersion":1,"id":id,"version":"1.0","depends":dependencies})
            .to_string()
            .as_bytes(),
    )])
}
struct Fixture {
    directory: PathBuf,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("memoria-pack-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        Self {
            root: directory.join("game"),
            directory,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
#[cfg(unix)]
fn java(fixture: &Fixture, profile: Option<&Value>, library: Option<&str>) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = fixture.directory.join("java");
    let body = if let (Some(profile), Some(library)) = (profile, library) {
        fs::write(fixture.directory.join("profile.json"), profile.to_string()).unwrap();
        fs::write(fixture.directory.join("loader.jar"), b"loader").unwrap();
        format!("mkdir -p \"$4/versions/official-profile\" \"$4/libraries/{}\"\ncp '{}' \"$4/versions/official-profile/official-profile.json\"\ncp '{}' \"$4/libraries/{library}\"\n",Path::new(library).parent().unwrap().display(),fixture.directory.join("profile.json").display(),fixture.directory.join("loader.jar").display())
    } else {
        "exit 0\n".into()
    };
    fs::write(&path,format!("#!/bin/sh\nif [ \"$1\" = \"-XshowSettings:properties\" ]; then\n echo 'java.version = 17.0.1'\n echo 'os.arch = amd64'\n exit 0\nfi\n{body}")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn progress() -> crate::ProgressCallback {
    Arc::new(|_| {})
}
#[cfg(unix)]
#[tokio::test]
async fn standalone_public_workflow_installs_all_four_loaders_from_an_empty_collection() {
    for kind in [Kind::Fabric, Kind::Quilt, Kind::Forge, Kind::NeoForge] {
        let fixture = Fixture::new();
        let server = Server::new().await;
        server.vanilla();
        let (loader, profile, library) = server.loader(kind);
        let java = java(&fixture, Some(&profile), Some(&library));
        let request = install::Request {
            root: fixture.root.clone(),
            target: "new-instance".into(),
            minecraft: "1.20.1".into(),
            loader: Some(loader),
            java: java.clone(),
            pack: None,
            optional: false,
        };
        let plan = install::preview_with(request, server.client(), server.catalog())
            .await
            .unwrap();
        assert!(
            !fixture.root.exists(),
            "previews must not write installation directories"
        );
        let key = install::install(plan, server.client(), progress())
            .await
            .unwrap();
        let scan = crate::LocalClientService::new(crate::GameLayout::new(&fixture.root).unwrap())
            .scan()
            .await
            .unwrap();
        assert!(scan
            .clients
            .iter()
            .any(|c| c.instance.id == key && c.instance.loader.is_some()));
        let lock = lockfile::read(&fixture.root, &key).unwrap();
        assert_eq!(lock.loader.as_ref().unwrap().kind, kind);
        assert!(
            lockfile::verify(&fixture.root, &key, &lock)
                .await
                .unwrap()
                .matches
        );
        let check = crate::GameLauncher::new(
            crate::GameLayout::new(&fixture.root).unwrap(),
            Arc::new(crate::UnavailableAuthProvider),
        )
        .check(
            &key,
            crate::LaunchOptions {
                java,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(check.classpath_entries >= 2);
        assert_eq!(
            server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|path| path.as_str() == "/client.jar")
                .count(),
            1
        );
    }
}
#[cfg(unix)]
#[tokio::test]
async fn mrpack_export_import_roundtrip_preserves_overrides_disabled_mods_sources_and_lock_hashes()
{
    let fixture = Fixture::new();
    let server = Server::new().await;
    server.vanilla();
    let (loader, _, _) = server.loader(Kind::Fabric);
    let java = java(&fixture, None, None);
    let main = jar(
        "main",
        json!({"fabricloader":">=0.16","minecraft":"1.20.x"}),
    );
    let disabled = jar("disabled", json!({"missing":"*"}));
    server.put("/main.jar", main.clone());
    let index = json!({"formatVersion":1,"game":"minecraft","name":"Round trip","versionId":"1","dependencies":{"minecraft":"1.20.1","fabric-loader":"0.16.10"},"files":[{"path":"mods/main.jar","fileSize":main.len(),"hashes":{"sha1":sha1(&main),"sha512":format!("{:x}",Sha512::digest(&main))},"downloads":[format!("{}/main.jar",server.base)]}]});
    let archive = zip(&[
        ("modrinth.index.json", index.to_string().as_bytes()),
        ("overrides/config/demo.json", b"{\"shared\":true}"),
        ("client-overrides/config/demo.json", b"{\"client\":true}"),
        ("overrides/mods/disabled.jar.disabled", &disabled),
        ("overrides/mods/readme.txt", b"local note"),
    ]);
    let pack = exchange::parse(&archive).unwrap();
    let request = install::Request {
        root: fixture.root.clone(),
        target: "original".into(),
        minecraft: "1.20.1".into(),
        loader: Some(loader.clone()),
        java: java.clone(),
        pack: Some(pack),
        optional: false,
    };
    let plan = install::preview_with(request, server.client(), server.catalog())
        .await
        .unwrap();
    let key = install::install(plan, server.client(), progress())
        .await
        .unwrap();
    let base = fixture.root.join(&key);
    assert_eq!(
        fs::read(base.join("config/demo.json")).unwrap(),
        b"{\"client\":true}"
    );
    let original = lockfile::read(&fixture.root, &key).unwrap();
    let plan =
        exchange::preview_export(fixture.root.clone(), key.clone(), "Round trip".into(), true)
            .await
            .unwrap();
    assert_eq!(plan.download_files, 1);
    assert_eq!(plan.embedded_files, 3);
    let path = fixture.directory.join("export.mrpack");
    exchange::export(plan, &path).await.unwrap();
    let pack = exchange::read_pack(&path).unwrap();
    let request = install::Request {
        root: fixture.root.clone(),
        target: "imported".into(),
        minecraft: pack.minecraft.clone(),
        loader: pack.loader.clone(),
        java,
        pack: Some(pack),
        optional: false,
    };
    let plan = install::preview_with(request, server.client(), server.catalog())
        .await
        .unwrap();
    let second = install::install(plan, server.client(), progress())
        .await
        .unwrap();
    let imported = lockfile::read(&fixture.root, &second).unwrap();
    assert_eq!(
        original
            .files
            .iter()
            .map(|f| (&f.path, &f.sha256))
            .collect::<Vec<_>>(),
        imported
            .files
            .iter()
            .map(|f| (&f.path, &f.sha256))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        server
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|p| p.as_str() == "/client.jar")
            .count(),
        1,
        "verified shared vanilla files should be reused"
    );
    assert!(
        lockfile::verify(&fixture.root, &key, &original)
            .await
            .unwrap()
            .matches
    );
}
#[cfg(unix)]
#[tokio::test]
async fn install_hash_errors_existing_targets_and_shared_conflicts_never_replace_user_instances() {
    let fixture = Fixture::new();
    let server = Server::new().await;
    server.vanilla();
    let (loader, _, _) = server.loader(Kind::Fabric);
    let java = java(&fixture, None, None);
    let request = install::Request {
        root: fixture.root.clone(),
        target: "new".into(),
        minecraft: "1.20.1".into(),
        loader: Some(loader),
        java,
        pack: None,
        optional: false,
    };
    let plan = install::preview_with(request.clone(), server.client(), server.catalog())
        .await
        .unwrap();
    server.put("/client.jar", b"changed".to_vec());
    assert!(install::install(plan, server.client(), progress())
        .await
        .is_err());
    assert!(!fixture.root.join("versions/new/new.json").exists());
    server.put("/client.jar", b"vanilla".to_vec());
    fs::create_dir_all(fixture.root.join("versions/1.20.1")).unwrap();
    fs::write(
        fixture.root.join("versions/1.20.1/1.20.1.jar"),
        b"user bytes",
    )
    .unwrap();
    let plan = install::preview_with(request.clone(), server.client(), server.catalog())
        .await
        .unwrap();
    assert!(install::install(plan, server.client(), progress())
        .await
        .unwrap_err()
        .contains("不会覆盖"));
    assert_eq!(
        fs::read(fixture.root.join("versions/1.20.1/1.20.1.jar")).unwrap(),
        b"user bytes"
    );
    fs::create_dir_all(fixture.root.join("versions/new")).unwrap();
    assert!(
        install::preview_with(request, server.client(), server.catalog())
            .await
            .is_err()
    );
}
fn local_fixture() -> (Fixture, String) {
    let fixture = Fixture::new();
    let base = fixture.root.join("versions/local");
    fs::create_dir_all(base.join("mods")).unwrap();
    fs::create_dir_all(base.join("config")).unwrap();
    fs::write(
        base.join("local.json"),
        json!({"id":"local","clientVersion":"1.20.1","mainClass":"fixture.Main","libraries":[]})
            .to_string(),
    )
    .unwrap();
    fs::write(base.join("mods/local.jar"), jar("local", json!({}))).unwrap();
    fs::write(base.join("config/demo.json"), b"{}").unwrap();
    (fixture, "versions/local".into())
}
#[tokio::test]
async fn lock_verification_and_export_snapshots_detect_all_content_drift_without_overwriting() {
    let (fixture, key) = local_fixture();
    let lock = lockfile::capture(&fixture.root, &key, true).await.unwrap();
    lockfile::save(&fixture.root, &key, &lock).await.unwrap();
    assert!(
        lockfile::verify(&fixture.root, &key, &lock)
            .await
            .unwrap()
            .matches
    );
    let plan = exchange::preview_export(fixture.root.clone(), key.clone(), "local".into(), true)
        .await
        .unwrap();
    let base = fixture.root.join(&key);
    fs::write(base.join("config/demo.json"), b"changed").unwrap();
    fs::write(base.join("mods/extra.jar"), b"extra").unwrap();
    fs::remove_file(base.join("mods/local.jar")).unwrap();
    let report = lockfile::verify(&fixture.root, &key, &lock).await.unwrap();
    assert_eq!(report.changed, vec!["config/demo.json"]);
    assert_eq!(report.missing, vec!["mods/local.jar"]);
    assert_eq!(report.extra, vec!["mods/extra.jar"]);
    assert!(lockfile::save(&fixture.root, &key, &lock).await.is_err());
    let target = fixture.directory.join("snapshot.mrpack");
    assert!(exchange::export(plan, &target).await.is_err());
    assert!(!target.exists());
}
#[tokio::test]
async fn config_opt_in_and_no_overwrite_exports_are_enforced() {
    let (fixture, key) = local_fixture();
    let preview =
        exchange::preview_export(fixture.root.clone(), key.clone(), "private".into(), false)
            .await
            .unwrap();
    assert!(preview
        .lock
        .files
        .iter()
        .all(|f| !f.path.starts_with("config/")));
    let destination = fixture.directory.join("exists.mrpack");
    fs::write(&destination, b"existing").unwrap();
    assert!(exchange::export(preview, &destination).await.is_err());
    assert_eq!(fs::read(destination).unwrap(), b"existing");
}
#[test]
fn unsafe_and_inconsistent_mrpack_requirements_are_rejected_before_installation() {
    let base = json!({"formatVersion":1,"game":"minecraft","dependencies":{"minecraft":"1.20.1"},"files":[]});
    let archive = zip(&[
        ("modrinth.index.json", base.to_string().as_bytes()),
        ("overrides/../escape", b"x"),
    ]);
    assert!(exchange::parse(&archive).is_err());
    let mut multiple = base.clone();
    multiple["dependencies"]["forge"] = json!("47.4.0");
    multiple["dependencies"]["fabric-loader"] = json!("0.16.10");
    assert!(exchange::parse(&zip(&[(
        "modrinth.index.json",
        multiple.to_string().as_bytes()
    )]))
    .is_err());
    let duplicate = zip(&[
        ("modrinth.index.json", base.to_string().as_bytes()),
        ("MODRINTH.INDEX.JSON", base.to_string().as_bytes()),
    ]);
    assert!(exchange::parse(&duplicate).is_err());
}
