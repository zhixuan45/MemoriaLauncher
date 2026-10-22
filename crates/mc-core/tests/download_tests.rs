use mc_core::{
    DownloadClient, DownloadFile, DownloadSettings, DownloadSource, GameLayout, LocalClientService,
};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

struct Server {
    base: String,
    routes: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    requests: Arc<Mutex<Vec<String>>>,
    failures: Arc<Mutex<HashMap<String, usize>>>,
    peak: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Server {
    async fn start(routes: HashMap<String, Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let routes = Arc::new(Mutex::new(routes));
        let requests = Arc::new(Mutex::new(Vec::new()));
        let peak = Arc::new(AtomicUsize::new(0));
        let failures = Arc::new(Mutex::new(HashMap::<String, usize>::new()));
        let active = Arc::new(AtomicUsize::new(0));
        let state = (
            routes.clone(),
            requests.clone(),
            peak.clone(),
            failures.clone(),
        );
        let task = tokio::spawn(async move {
            let mut handlers = tokio::task::JoinSet::new();
            loop {
                tokio::select! {
                    accepted = listener.accept() => {
                        let (mut stream, _) = accepted.unwrap();
                        let (routes, requests, peak, failures) = state.clone();
                        let active = active.clone();
                        handlers.spawn(async move {
                            let mut header = Vec::new();
                            let mut buffer = [0u8; 1024];
                            while !header.windows(4).any(|part| part == b"\r\n\r\n") {
                                let count = stream.read(&mut buffer).await.unwrap();
                                if count == 0 { return; }
                                header.extend_from_slice(&buffer[..count]);
                            }
                            let header = String::from_utf8_lossy(&header);
                            let path = header.split_whitespace().nth(1).unwrap().to_string();
                            requests.lock().unwrap().push(path.clone());
                            let bytes = routes.lock().unwrap().get(&path).cloned();
                            let failed = failures.lock().unwrap().get_mut(&path).is_some_and(|remaining| {
                                if *remaining == 0 { false } else { *remaining -= 1; true }
                            });
                            let status = if failed { "503 Service Unavailable" } else if bytes.is_some() { "200 OK" } else { "404 Not Found" };
                            let bytes = bytes.unwrap_or_default();
                            let count = active.fetch_add(1, Ordering::SeqCst) + 1;
                            peak.fetch_max(count, Ordering::SeqCst);
                            // Headers arrive immediately; concurrency must stay limited while the body waits.
                            stream.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", bytes.len()).as_bytes()).await.unwrap();
                            tokio::time::sleep(std::time::Duration::from_millis(35)).await;
                            active.fetch_sub(1, Ordering::SeqCst);
                            let _ = stream.write_all(&bytes).await;
                            let _ = stream.shutdown().await;
                        });
                    }
                    _ = handlers.join_next(), if !handlers.is_empty() => {}
                }
            }
        });
        Self {
            base,
            routes,
            requests,
            failures,
            peak,
            task,
        }
    }

    fn client(&self, parallel_downloads: usize) -> DownloadClient {
        DownloadClient::new(DownloadSettings {
            source: DownloadSource::Custom,
            custom_url: self.base.clone(),
            parallel_downloads,
        })
        .unwrap()
    }

    fn count(&self, path: &str) -> usize {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| *request == path)
            .count()
    }
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/download-tests")
        .join(uuid::Uuid::new_v4().to_string())
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha1::digest(bytes))
}

fn remote(url: &str, bytes: &[u8]) -> Value {
    json!({ "url": url, "sha1": hash(bytes), "size": bytes.len() })
}

#[tokio::test]
async fn transient_http_failures_retry_with_the_same_source_and_limit() {
    let payload = b"retry succeeds";
    let server = Server::start(HashMap::from([(
        "/maven/retry.jar".into(),
        payload.to_vec(),
    )]))
    .await;
    server
        .failures
        .lock()
        .unwrap()
        .insert("/maven/retry.jar".into(), 2);
    let layout = GameLayout::new(fixture()).unwrap();
    server
        .client(1)
        .download_file(
            &layout,
            DownloadFile {
                url: "https://libraries.minecraft.net/retry.jar".into(),
                path: "libraries/retry.jar".into(),
                sha1: Some(hash(payload)),
                size: Some(payload.len() as u64),
            },
        )
        .await
        .unwrap();
    assert_eq!(server.count("/maven/retry.jar"), 3);
    assert_eq!(server.peak.load(Ordering::SeqCst), 1);
}

#[test]
fn source_routes_cover_manifest_client_libraries_assets_and_custom_prefixes() {
    let mut config = DownloadSettings {
        source: DownloadSource::Bmclapi,
        ..Default::default()
    };
    for (original, expected) in [
        (
            "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
            "/mc/game/version_manifest_v2.json",
        ),
        (
            "https://launchermeta.mojang.com/v1/packages/abc/version.json",
            "/v1/packages/abc/version.json",
        ),
        (
            "https://piston-data.mojang.com/v1/objects/abc/client.jar",
            "/v1/objects/abc/client.jar",
        ),
        (
            "https://launcher.mojang.com/v1/objects/abc/client.jar",
            "/v1/objects/abc/client.jar",
        ),
        (
            "https://libraries.minecraft.net/com/example/test.jar",
            "/maven/com/example/test.jar",
        ),
        (
            "http://resources.download.minecraft.net/ab/abcdef",
            "/assets/ab/abcdef",
        ),
    ] {
        assert_eq!(
            config.resolve(original).unwrap(),
            format!("https://bmclapi2.bangbang93.com{expected}")
        );
        config.source = DownloadSource::Custom;
        config.custom_url = " https://mirror.example/api/ ".into();
        assert_eq!(
            config.resolve(original).unwrap(),
            format!("https://mirror.example/api{expected}")
        );
        config.source = DownloadSource::Bmclapi;
    }
    assert_eq!(
        config
            .resolve("https://example.com/piston-meta.mojang.com/file")
            .unwrap(),
        "https://example.com/piston-meta.mojang.com/file"
    );
    assert_eq!(
        config
            .resolve("https://piston-data.mojang.com/v1/objects/abc/client.xml")
            .unwrap(),
        "https://piston-data.mojang.com/v1/objects/abc/client.xml"
    );
    config.source = DownloadSource::Official;
    assert_eq!(
        config
            .resolve("http://resources.download.minecraft.net/ab/abcdef")
            .unwrap(),
        "https://resources.download.minecraft.net/ab/abcdef"
    );
    for parallel in [0, 65, usize::MAX] {
        config.parallel_downloads = parallel;
        assert!(config.normalized().is_err());
    }
    config.parallel_downloads = 8;
    config.source = DownloadSource::Custom;
    for address in [
        "",
        "http://mirror.example",
        "https://user:password@mirror.example",
        "https://mirror.example/?key=value",
        "https://mirror.example/#fragment",
    ] {
        config.custom_url = address.into();
        assert!(config.normalized().is_err(), "{address}");
    }
}

#[tokio::test]
async fn parallel_limit_is_shared_by_batches_and_kept_until_bodies_finish() {
    for limit in [1, 3] {
        let payload = b"verified file contents";
        let routes = (0..12)
            .map(|index| (format!("/maven/test-{index}.jar"), payload.to_vec()))
            .collect();
        let server = Server::start(routes).await;
        let client = server.client(limit);
        let layout = GameLayout::new(fixture()).unwrap();
        let files: Vec<_> = (0..12)
            .map(|index| DownloadFile {
                url: format!("https://libraries.minecraft.net/test-{index}.jar"),
                path: format!("libraries/test-{index}.jar"),
                sha1: Some(hash(payload)),
                size: Some(payload.len() as u64),
            })
            .collect();
        let first = files[..6].to_vec();
        let second = files[6..].to_vec();
        let (a, b) = tokio::join!(
            client.download_files(layout.clone(), first, Arc::new(|_| {})),
            client.download_files(layout.clone(), second, Arc::new(|_| {})),
        );
        a.unwrap();
        b.unwrap();
        assert_eq!(server.peak.load(Ordering::SeqCst), limit);
        assert_eq!(server.requests.lock().unwrap().len(), 12);
        let result = Arc::new(Mutex::new(None));
        let progress_result = result.clone();
        client
            .download_files(
                layout,
                files,
                Arc::new(move |progress| *progress_result.lock().unwrap() = Some(progress)),
            )
            .await
            .unwrap();
        assert_eq!(
            server.requests.lock().unwrap().len(),
            12,
            "verified files must be reused"
        );
        assert_eq!(result.lock().unwrap().as_ref().unwrap().reused, 12);
    }
}

#[tokio::test]
async fn failed_hash_and_unsafe_paths_do_not_replace_existing_files() {
    let server = Server::start(HashMap::from([(
        "/maven/file.jar".into(),
        b"new-data".to_vec(),
    )]))
    .await;
    let root = fixture();
    tokio::fs::create_dir_all(root.join("libraries"))
        .await
        .unwrap();
    tokio::fs::write(root.join("libraries/file.jar"), b"old-data")
        .await
        .unwrap();
    let file = DownloadFile {
        url: "https://libraries.minecraft.net/file.jar".into(),
        path: "libraries/file.jar".into(),
        sha1: Some("0".repeat(40)),
        size: Some(8),
    };
    let client = server.client(2);
    let layout = GameLayout::new(&root).unwrap();
    assert!(client.download_file(&layout, file.clone()).await.is_err());
    assert_eq!(
        tokio::fs::read(root.join("libraries/file.jar"))
            .await
            .unwrap(),
        b"old-data"
    );
    assert_eq!(
        std::fs::read_dir(root.join("libraries")).unwrap().count(),
        1,
        "temporary files must be cleaned up"
    );
    let mut unsafe_file = file;
    unsafe_file.path = "../outside.jar".into();
    assert!(client.download_file(&layout, unsafe_file).await.is_err());
    assert_eq!(server.count("/maven/file.jar"), 1);
}

async fn vanilla_server() -> (Server, String) {
    let asset = b"sound asset";
    let asset_hash = hash(asset);
    let index = serde_json::to_vec(
        &json!({ "objects": { "sound.ogg": { "hash": asset_hash, "size": asset.len() } } }),
    )
    .unwrap();
    let jar = b"client jar";
    let library = b"library jar";
    let logging = b"logging xml";
    let version = json!({
        "id": "test-version", "type": "release", "mainClass": "example.Main", "javaVersion": { "majorVersion": 21 },
        "releaseTime": "2026-10-22T00:00:00Z", "arguments": { "jvm": [], "game": [] },
        "downloads": { "client": remote("https://piston-data.mojang.com/client.jar", jar) },
        "assetIndex": { "id": "test-index", "url": "https://piston-meta.mojang.com/index.json", "sha1": hash(&index), "size": index.len() },
        "libraries": [{ "name": "example:lib:1", "downloads": { "artifact": {
            "url": "https://libraries.minecraft.net/example/lib/1/lib-1.jar", "path": "example/lib/1/lib-1.jar", "sha1": hash(library), "size": library.len()
        } } }],
        "logging": { "client": { "argument": "-Dlog4j.configurationFile=${path}", "file": {
            "id": "test.xml", "url": "https://piston-data.mojang.com/test.xml", "sha1": hash(logging), "size": logging.len()
        } } }
    });
    let metadata = serde_json::to_vec(&version).unwrap();
    let manifest = json!({ "versions": [{ "id": "test-version", "type": "release", "releaseTime": "2026-10-22T00:00:00Z",
        "url": "https://piston-meta.mojang.com/test-version.json", "sha1": hash(&metadata) }] });
    let asset_path = format!("/assets/{}/{}", &asset_hash[..2], asset_hash);
    let server = Server::start(HashMap::from([
        (
            "/mc/game/version_manifest_v2.json".into(),
            serde_json::to_vec(&manifest).unwrap(),
        ),
        ("/test-version.json".into(), metadata),
        ("/index.json".into(), index),
        ("/client.jar".into(), jar.to_vec()),
        ("/test.xml".into(), logging.to_vec()),
        ("/maven/example/lib/1/lib-1.jar".into(), library.to_vec()),
        (asset_path.clone(), asset.to_vec()),
    ]))
    .await;
    (server, asset_path)
}

#[tokio::test]
async fn vanilla_install_downloads_verifies_reuses_and_exposes_a_real_scanned_instance() {
    let (server, asset_path) = vanilla_server().await;
    let root = fixture();
    let layout = GameLayout::new(&root).unwrap();
    let client = server.client(3);
    let versions = mc_core::MojangCatalogProvider::list_with_client(&client)
        .await
        .unwrap();
    assert_eq!(versions[0].id, "test-version");
    for _ in 0..2 {
        mc_core::install_vanilla(
            client.clone(),
            layout.clone(),
            "test-version".into(),
            Arc::new(|_| {}),
        )
        .await
        .unwrap();
    }
    let report = LocalClientService::new(layout).scan().await.unwrap();
    assert!(report.issues.is_empty());
    assert_eq!(report.clients.len(), 1);
    assert!(report.clients[0].client_jar_present);
    assert_eq!(report.clients[0].java_major, Some(21));
    assert!(root.join("libraries/example/lib/1/lib-1.jar").is_file());
    assert!(root.join("assets/log_configs/test.xml").is_file());
    assert_eq!(server.count(&asset_path), 1);
    assert_eq!(server.count("/client.jar"), 1);
    assert_eq!(server.count("/maven/example/lib/1/lib-1.jar"), 1);
    assert!(server.peak.load(Ordering::SeqCst) <= 3);
}

#[tokio::test]
async fn failed_install_does_not_publish_a_version_or_overwrite_a_modified_configuration() {
    let (server, _) = vanilla_server().await;
    let root = fixture();
    let layout = GameLayout::new(&root).unwrap();
    server
        .routes
        .lock()
        .unwrap()
        .insert("/client.jar".into(), b"wrong data".to_vec());
    assert!(mc_core::install_vanilla(
        server.client(2),
        layout.clone(),
        "test-version".into(),
        Arc::new(|_| {})
    )
    .await
    .is_err());
    assert!(!root
        .join("versions/test-version/test-version.json")
        .exists());
    let metadata = root.join("versions/test-version/test-version.json");
    tokio::fs::create_dir_all(metadata.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(&metadata, b"custom configuration")
        .await
        .unwrap();
    assert!(mc_core::install_vanilla(
        server.client(2),
        layout,
        "test-version".into(),
        Arc::new(|_| {})
    )
    .await
    .is_err());
    assert_eq!(
        tokio::fs::read(metadata).await.unwrap(),
        b"custom configuration"
    );
}

#[tokio::test]
#[ignore = "Requires live access to Mojang and BMCLAPI"]
async fn live_sources_download_real_metadata_with_checksum_verification() {
    for source in [DownloadSource::Official, DownloadSource::Bmclapi] {
        let client = DownloadClient::new(DownloadSettings {
            source,
            ..Default::default()
        })
        .unwrap();
        let manifest: Value = client
            .json(mc_core::service::downloads::VERSION_MANIFEST)
            .await
            .unwrap();
        let version = manifest["versions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|version| version["id"] == "1.21.1")
            .unwrap();
        let bytes = client
            .bytes(version["url"].as_str().unwrap(), 16 * 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(hash(&bytes), version["sha1"].as_str().unwrap());
        let metadata: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(metadata["id"], "1.21.1");
        let file = &metadata["logging"]["client"]["file"];
        let content = client
            .bytes(file["url"].as_str().unwrap(), 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(hash(&content), file["sha1"].as_str().unwrap());
        let library = metadata["libraries"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|library| library.get("downloads")?.get("artifact"))
            .filter(|file| file["size"].as_u64().is_some_and(|size| size > 0))
            .min_by_key(|file| file["size"].as_u64().unwrap())
            .unwrap();
        let library_bytes = client
            .bytes(library["url"].as_str().unwrap(), 8 * 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(hash(&library_bytes), library["sha1"].as_str().unwrap());
        let index_file = &metadata["assetIndex"];
        let index_bytes = client
            .bytes(index_file["url"].as_str().unwrap(), 32 * 1024 * 1024)
            .await
            .unwrap();
        assert_eq!(hash(&index_bytes), index_file["sha1"].as_str().unwrap());
        let index: Value = serde_json::from_slice(&index_bytes).unwrap();
        let asset = index["objects"]
            .as_object()
            .unwrap()
            .values()
            .find(|object| {
                object["size"]
                    .as_u64()
                    .is_some_and(|size| size > 0 && size < 1024)
            })
            .unwrap();
        let asset_hash = asset["hash"].as_str().unwrap();
        let asset_bytes = client
            .bytes(
                &format!(
                    "https://resources.download.minecraft.net/{}/{asset_hash}",
                    &asset_hash[..2]
                ),
                1024,
            )
            .await
            .unwrap();
        assert_eq!(hash(&asset_bytes), asset_hash);
        eprintln!(
            "{source}: verified manifest, version JSON, asset index, {} bytes of library, {} bytes of asset and {} bytes of official log configuration",
            library_bytes.len(), asset_bytes.len(), content.len()
        );
    }
}
