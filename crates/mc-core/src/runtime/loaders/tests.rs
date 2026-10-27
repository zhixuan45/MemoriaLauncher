use super::*;
use serde_json::json;
use std::{
    collections::HashMap,
    fs,
    sync::{Arc, Mutex},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Server {
    base: String,
    routes: Arc<Mutex<HashMap<String, Vec<u8>>>>,
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
        let task = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let shared = shared.clone();
                tokio::spawn(async move {
                    let mut header = vec![];
                    while !header.ends_with(b"\r\n\r\n") {
                        let Ok(b) = stream.read_u8().await else {
                            return;
                        };
                        header.push(b);
                        if header.len() > 8192 {
                            return;
                        }
                    }
                    let text = String::from_utf8_lossy(&header);
                    let path = text.split_whitespace().nth(1).unwrap();
                    let body = shared.lock().unwrap().get(path).cloned();
                    let status = if body.is_some() { 200 } else { 404 };
                    let body = body.unwrap_or_default();
                    let _=stream.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",body.len()).as_bytes()).await;
                    let _ = stream.write_all(&body).await;
                });
            }
        });
        Self { base, routes, task }
    }
    fn put(&self, path: &str, body: impl Into<Vec<u8>>) {
        self.routes.lock().unwrap().insert(path.into(), body.into());
    }
    fn catalog(&self) -> Catalog {
        let client = DownloadClient::new(Default::default()).unwrap();
        Catalog {
            client,
            fabric: format!("{}/fabric", self.base),
            quilt: format!("{}/quilt", self.base),
            forge: format!("{}/forge", self.base),
            neoforge: format!("{}/neo", self.base),
        }
    }
    fn direct(&self, kind: Kind) -> Value {
        let group = if kind == Kind::Fabric {
            "net.fabricmc"
        } else {
            "org.quiltmc"
        };
        let artifact = if kind == Kind::Fabric {
            "fabric-loader"
        } else {
            "quilt-loader"
        };
        let profile = json!({"id":"official-profile","inheritsFrom":"1.20.1","mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient",
            "libraries":[{"name":format!("{group}:{artifact}:0.16.10"),"url":format!("{}/repo",self.base)}]});
        let path = format!(
            "/repo/{}/{artifact}/0.16.10/{artifact}-0.16.10.jar",
            group.replace('.', "/")
        );
        self.put(&path, b"verified loader".to_vec());
        self.put(
            &format!("{path}.sha1"),
            format!("{:x}\n", sha1::Sha1::digest(b"verified loader")),
        );
        let api = if kind == Kind::Fabric {
            "fabric"
        } else {
            "quilt"
        };
        self.put(&format!("/{api}/versions/loader/1.20.1"),json!([{"loader":{"version":"0.16.9","stable":true}},{"loader":{"version":"0.16.10","stable":true}}]).to_string());
        self.put(
            &format!("/{api}/versions/loader/1.20.1/0.16.10/profile/json"),
            profile.to_string(),
        );
        profile
    }
}
fn fixture() -> PathBuf {
    let root = std::env::temp_dir().join(format!("memoria-loader-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("versions/1.20.1")).unwrap();
    fs::write(root.join("versions/1.20.1/1.20.1.json"),json!({"id":"1.20.1","mainClass":"net.minecraft.client.main.Main","assets":"1.20","javaVersion":{"component":"java-runtime-gamma","majorVersion":17},"libraries":[],"arguments":{"game":["--username","${auth_player_name}","--version","${version_name}"]}}).to_string()).unwrap();
    fs::write(root.join("versions/1.20.1/1.20.1.jar"), b"vanilla").unwrap();
    fs::create_dir_all(root.join("assets/indexes")).unwrap();
    fs::write(root.join("assets/indexes/1.20.json"), br#"{"objects":{}}"#).unwrap();
    root
}
#[cfg(unix)]
fn java(root: &Path, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = root.join(format!("java-{}", uuid::Uuid::new_v4()));
    fs::write(&path,format!("#!/bin/sh\nif [ \"$1\" = \"-XshowSettings:properties\" ]; then\n echo 'java.version = 17.0.1'\n echo 'os.arch = amd64'\n exit 0\nfi\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn progress() -> crate::ProgressCallback {
    Arc::new(|_| {})
}

#[cfg(unix)]
#[tokio::test]
async fn fabric_and_quilt_use_compatible_metadata_verified_libraries_and_new_inherited_instances() {
    for kind in [Kind::Fabric, Kind::Quilt] {
        let root = fixture();
        let server = Server::new().await;
        server.direct(kind);
        let catalog = server.catalog();
        let versions = catalog.versions(kind, "1.20.1").await.unwrap();
        assert_eq!(versions[0].version, "0.16.10");
        let original = fs::read(root.join("versions/1.20.1/1.20.1.json")).unwrap();
        let plan = catalog
            .preview(
                root.clone(),
                "versions/1.20.1".into(),
                kind,
                "0.16.10".into(),
            )
            .await
            .unwrap();
        assert_eq!(plan.files.len(), 1);
        assert!(plan.files[0].sha1.is_some());
        let target = plan.target.clone();
        let executable = java(&root, "exit 9");
        let id = install(catalog.client.clone(), plan, executable.clone(), progress())
            .await
            .unwrap();
        assert_eq!(id, format!("versions/{target}"));
        let report = crate::LocalClientService::new(GameLayout::new(&root).unwrap())
            .scan()
            .await
            .unwrap();
        let entry = report
            .clients
            .iter()
            .find(|entry| entry.instance.id == id)
            .unwrap();
        assert!(entry.client_jar_present);
        let launcher = crate::GameLauncher::new(
            GameLayout::new(&root).unwrap(),
            Arc::new(crate::OfflineAuthProvider),
        );
        let options = crate::LaunchOptions {
            java: executable.clone(),
            ..Default::default()
        };
        let checked = launcher.check(&id, options.clone()).await.unwrap();
        assert!(checked.classpath_entries >= 2);
        let launch = launcher.prepare(&id, "Fixture", options).await.unwrap();
        assert!(launch
            .redacted_arguments()
            .iter()
            .all(|argument| !argument.contains("${")));

        let settings = crate::service::desktop_instances::load_settings(&root, &id).unwrap();
        assert_eq!(settings.java, Some(executable.clone()));
        let check = crate::GameLauncher::new(
            GameLayout::new(&root).unwrap(),
            Arc::new(crate::UnavailableAuthProvider),
        )
        .check(
            &id,
            crate::LaunchOptions {
                java: executable,
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(check.classpath_entries >= 2);
        assert_eq!(entry.instance.mc_version, "1.20.1");
        assert_eq!(entry.instance.loader.as_ref().unwrap().version, "0.16.10");
        assert_eq!(
            fs::read(root.join("versions/1.20.1/1.20.1.json")).unwrap(),
            original
        );
        assert!(catalog
            .preview(
                root.clone(),
                "versions/1.20.1".into(),
                kind,
                "0.16.10".into()
            )
            .await
            .is_err());
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(unix)]
#[tokio::test]
async fn incompatible_versions_bad_hashes_modified_bases_and_library_conflicts_do_not_publish() {
    let root = fixture();
    let server = Server::new().await;
    let profile = server.direct(Kind::Fabric);
    let catalog = server.catalog();
    assert!(catalog
        .preview(
            root.clone(),
            "versions/1.20.1".into(),
            Kind::Fabric,
            "0.99.0".into()
        )
        .await
        .is_err());
    let plan = catalog
        .preview(
            root.clone(),
            "versions/1.20.1".into(),
            Kind::Fabric,
            "0.16.10".into(),
        )
        .await
        .unwrap();
    let target = plan.target.clone();
    let executable = java(&root, "exit 9");
    let mut wrong_java = plan.clone();
    wrong_java.required_java = Some(21);
    assert!(matches!(
        install(
            catalog.client.clone(),
            wrong_java,
            executable.clone(),
            progress()
        )
        .await,
        Err(LauncherError::JavaUnavailable(_))
    ));
    let base = root.join("versions/1.20.1/1.20.1.jar");
    fs::write(&base, b"changed").unwrap();
    assert!(matches!(
        install(catalog.client.clone(), plan, executable.clone(), progress()).await,
        Err(LauncherError::PlanStale)
    ));
    fs::write(&base, b"vanilla").unwrap();
    let mut bad = profile.clone();
    bad["inheritsFrom"] = json!("1.21.1");
    server.put(
        "/fabric/versions/loader/1.20.1/0.16.10/profile/json",
        bad.to_string(),
    );
    assert!(catalog
        .preview(
            root.clone(),
            "versions/1.20.1".into(),
            Kind::Fabric,
            "0.16.10".into()
        )
        .await
        .is_err());
    server.put(
        "/fabric/versions/loader/1.20.1/0.16.10/profile/json",
        profile.to_string(),
    );
    let plan = catalog
        .preview(
            root.clone(),
            "versions/1.20.1".into(),
            Kind::Fabric,
            "0.16.10".into(),
        )
        .await
        .unwrap();
    server.put(
        "/repo/net/fabricmc/fabric-loader/0.16.10/fabric-loader-0.16.10.jar",
        b"corrupt".to_vec(),
    );
    assert!(
        install(catalog.client.clone(), plan, executable.clone(), progress())
            .await
            .is_err()
    );
    assert!(!root.join(format!("versions/{target}")).exists());
    server.direct(Kind::Fabric);
    let plan = catalog
        .preview(
            root.clone(),
            "versions/1.20.1".into(),
            Kind::Fabric,
            "0.16.10".into(),
        )
        .await
        .unwrap();
    let path = root.join(&plan.files[0].path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"user version").unwrap();
    assert!(
        install(catalog.client.clone(), plan, executable, progress())
            .await
            .is_err()
    );
    assert_eq!(fs::read(path).unwrap(), b"user version");
    assert!(!root.join(format!("versions/{target}")).exists());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn modern_forge_and_neoforge_pin_installers_run_in_staging_and_validate_their_output() {
    use std::io::{Cursor, Write};
    use zip::write::SimpleFileOptions;
    for (kind, version, group, artifact) in [
        (Kind::Forge, "1.20.1-47.0.0", "net/minecraftforge", "forge"),
        (Kind::NeoForge, "20.1.1", "net/neoforged", "neoforge"),
    ] {
        let root = fixture();
        let server = Server::new().await;
        let catalog = server.catalog();
        let maven = if kind == Kind::Forge { "forge" } else { "neo" };
        server.put(
            &format!("/{maven}/{group}/{artifact}/maven-metadata.xml"),
            format!("<metadata><version>{version}</version><version>99.0.1</version></metadata>"),
        );
        let name = format!("{}{}:universal", kind.coordinate(), version);
        let relative = super::super::metadata::maven_path(&name).unwrap();
        let profile = json!({"id":"official-forge","inheritsFrom":"1.20.1","mainClass":"cpw.mods.bootstraplauncher.BootstrapLauncher", "libraries":[{"name":name,"downloads":{"artifact":{"path":relative,"url":"","sha1":"","size":0}}}]});
        let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
        zip.start_file("version.json", SimpleFileOptions::default())
            .unwrap();
        zip.write_all(profile.to_string().as_bytes()).unwrap();
        let jar = zip.finish().unwrap().into_inner();
        let url =
            format!("/{maven}/{group}/{artifact}/{version}/{artifact}-{version}-installer.jar");
        server.put(&url, jar.clone());
        server.put(
            &format!("{url}.sha1"),
            format!("{:x}", sha1::Sha1::digest(&jar)),
        );
        fs::write(root.join("installer-profile.json"), profile.to_string()).unwrap();
        let executable=java(&root,&format!("test \"$1\" = '-jar' || exit 31\ntest \"$3\" = '--installClient' || exit 32\nmkdir -p \"$4/versions/official-forge\" \"$4/libraries/{}\"\ncp '{}/installer-profile.json' \"$4/versions/official-forge/official-forge.json\"\nprintf processed > \"$4/libraries/{relative}\"",Path::new(&relative).parent().unwrap().display(),root.display()));
        let plan = catalog
            .preview(root.clone(), "versions/1.20.1".into(), kind, version.into())
            .await
            .unwrap();
        assert!(plan.installer.is_some());
        let id = install(catalog.client.clone(), plan, executable.clone(), progress())
            .await
            .unwrap();
        let entry = crate::LocalClientService::new(GameLayout::new(&root).unwrap())
            .scan()
            .await
            .unwrap()
            .clients
            .into_iter()
            .find(|entry| entry.instance.id == id)
            .unwrap();
        assert!(entry.client_jar_present);
        let launcher = crate::GameLauncher::new(
            GameLayout::new(&root).unwrap(),
            Arc::new(crate::OfflineAuthProvider),
        );
        let options = crate::LaunchOptions {
            java: executable.clone(),
            ..Default::default()
        };
        let checked = launcher.check(&id, options.clone()).await.unwrap();
        assert!(checked.classpath_entries >= 2);
        let launch = launcher.prepare(&id, "Fixture", options).await.unwrap();
        assert!(launch
            .redacted_arguments()
            .iter()
            .all(|argument| !argument.contains("${")));

        assert_eq!(entry.instance.loader.unwrap().version, version);
        let plan = catalog
            .preview(root.clone(), "versions/1.20.1".into(), kind, version.into())
            .await;
        assert!(plan.is_err());
        fs::remove_dir_all(root).unwrap();
    }
}

#[test]
fn rejects_path_traversal_and_maps_neoforge_versions_without_guessing_snapshots() {
    for value in ["../bad", "bad/version", "bad\\version", "bad:version", ""] {
        assert!(component(value).is_err());
    }
    assert_eq!(neo_prefix("1.21").unwrap(), "21.0.");
    assert_eq!(neo_prefix("26.1.2").unwrap(), "26.1.2.");
    assert!(neo_prefix("24w14a").is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn installer_failure_is_reported_with_redacted_logs_and_never_publishes_a_profile() {
    let root = fixture();
    let stage = root.join("operation");
    let work = stage.join("work");
    fs::create_dir_all(&work).unwrap();
    let executable = java(
        &root,
        "echo 'Authorization: Bearer private-secret'\necho 'processor failure'\nexit 7",
    );
    let error = java_installer::run(&executable, &work.join("installer.jar"), &work)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("失败"));
    let log = fs::read_to_string(stage.join("installer.log")).unwrap();
    assert!(!log.contains("private-secret"));
    assert!(log.contains("processor failure"));
    assert_eq!(fs::read_dir(root.join("versions")).unwrap().count(), 1);
    fs::remove_dir_all(root).unwrap();
}
