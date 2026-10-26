use super::*;
use crate::service::instance_write::tests::jar;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
async fn server<F>(responses: F) -> (Resolver, tokio::task::JoinHandle<Vec<String>>)
where
    F: FnOnce(&str) -> Vec<Vec<u8>>,
{
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let payloads = responses(&base);
    let mut resolver = Resolver::new().unwrap();
    resolver.modrinth = format!("{base}/v2");
    resolver.curseforge = format!("{base}/v1");
    resolver.key = Some("fixture-key".into());
    resolver.test_transport = true;
    let task = tokio::spawn(async move {
        let mut headers = vec![];
        for body in payloads {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = vec![];
            while !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                let mut buffer = [0; 4096];
                let count = socket.read(&mut buffer).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
            }
            headers.push(String::from_utf8(bytes).unwrap());
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
        headers
    });
    (resolver, task)
}
#[tokio::test]
async fn modrinth_pins_compatible_file_and_checks_download_hash() {
    let bytes = jar("demo");
    let sha1 = format!("{:x}", Sha1::digest(&bytes));
    let (resolver,task)=server(|base|vec![json!({"id":"project","project_type":"mod","title":"Demo"}).to_string().into_bytes(),json!({"id":"version","project_id":"project","game_versions":["1.20.1"],"loaders":["fabric"],"files":[{"primary":true,"filename":"demo.jar","url":format!("{base}/demo.jar"),"size":bytes.len(),"hashes":{"sha1":sha1}}]}).to_string().into_bytes(),bytes.clone()]).await;
    let (entry, actual) = resolver
        .resolve(
            &ModReference {
                source: Source::Modrinth,
                project_id: "project".into(),
                file_id: Some("version".into()),
            },
            "1.20.1",
            "fabric",
        )
        .await
        .unwrap();
    assert_eq!(actual, bytes);
    assert_eq!(entry["file_id"], "version");
    assert_eq!(entry["sha256"], digest(&actual));
    let headers = task.await.unwrap();
    assert!(headers[0].starts_with("GET /v2/project/project "));
    assert!(headers[1].starts_with("GET /v2/version/version "));
    assert!(headers
        .iter()
        .all(|h| !h.to_ascii_lowercase().contains("x-api-key")));
}
#[tokio::test]
async fn curseforge_uses_key_only_for_api_and_honors_pinned_ids() {
    let bytes = jar("demo");
    let sha1 = format!("{:x}", Sha1::digest(&bytes));
    let (resolver,task)=server(|base|vec![json!({"data":{"id":123,"gameId":432,"classId":6,"name":"Demo","allowModDistribution":true}}).to_string().into_bytes(),json!({"data":{"id":456,"modId":123,"isAvailable":true,"fileName":"demo.jar","gameVersions":["1.20.1","Fabric"],"hashes":[{"algo":1,"value":sha1}],"downloadUrl":format!("{base}/demo.jar"),"fileLength":bytes.len()}}).to_string().into_bytes(),bytes.clone()]).await;
    let (_, actual) = resolver
        .resolve(
            &ModReference {
                source: Source::Curseforge,
                project_id: "123".into(),
                file_id: Some("456".into()),
            },
            "1.20.1",
            "fabric",
        )
        .await
        .unwrap();
    assert_eq!(actual, bytes);
    let headers = task.await.unwrap();
    assert!(headers[0].starts_with("GET /v1/mods/123 "));
    assert!(headers[1].starts_with("GET /v1/mods/123/files/456 "));
    assert!(headers[0].contains("fixture-key"));
    assert!(headers[1].contains("fixture-key"));
    assert!(!headers[2].contains("fixture-key"));
}
#[tokio::test]
async fn forbidden_distribution_mismatch_and_missing_key_fail_closed() {
    let (resolver, task) = server(|_| {
        vec![
            json!({"data":{"id":123,"gameId":432,"classId":6,"allowModDistribution":false}})
                .to_string()
                .into_bytes(),
        ]
    })
    .await;
    let reference = ModReference {
        source: Source::Curseforge,
        project_id: "123".into(),
        file_id: None,
    };
    assert_eq!(
        resolver
            .resolve(&reference, "1.20.1", "fabric")
            .await
            .unwrap_err()
            .code,
        "DISTRIBUTION_FORBIDDEN"
    );
    task.await.unwrap();
    let mut resolver = Resolver::new().unwrap();
    resolver.key = None;
    assert_eq!(
        resolver
            .resolve(&reference, "1.20.1", "fabric")
            .await
            .unwrap_err()
            .code,
        "CURSEFORGE_KEY_REQUIRED"
    );
    let (resolver,task)=server(|_|vec![json!({"id":"project","project_type":"mod"}).to_string().into_bytes(),json!({"id":"wrong","project_id":"project","game_versions":["1.20.1"],"loaders":["fabric"]}).to_string().into_bytes()]).await;
    assert_eq!(
        resolver
            .resolve(
                &ModReference {
                    source: Source::Modrinth,
                    project_id: "project".into(),
                    file_id: Some("version".into())
                },
                "1.20.1",
                "fabric"
            )
            .await
            .unwrap_err()
            .code,
        "INVALID_SOURCE_METADATA"
    );
    task.await.unwrap();
    assert_eq!(
        resolver
            .file("https://untrusted.example/demo.jar", &"0".repeat(40), 10)
            .await
            .unwrap_err()
            .code,
        "INVALID_SOURCE_METADATA"
    );
    let (resolver, task) = server(|_| vec![jar("demo")]).await;
    assert_eq!(
        resolver
            .file(
                &format!("{}/bad.jar", resolver.modrinth.trim_end_matches("/v2")),
                &"0".repeat(40),
                jar("demo").len() as u64
            )
            .await
            .unwrap_err()
            .code,
        "DOWNLOAD_HASH_MISMATCH"
    );
    task.await.unwrap();
}

#[tokio::test]
async fn source_list_preparation_scans_instance_and_persists_canonical_manifest() {
    let root = std::env::temp_dir().join(format!("memoria-mod-list-{}", uuid::Uuid::new_v4()));
    let base = root.join("versions/pack");
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("pack.json"),json!({"id":"pack","clientVersion":"1.20.1","mainClass":"net.fabricmc.loader.impl.launch.knot.KnotClient","libraries":[{"name":"net.fabricmc:fabric-loader:0.15.11"}]}).to_string()).unwrap();
    let bytes = jar("demo");
    let sha1 = format!("{:x}", Sha1::digest(&bytes));
    let (resolver,task)=server(|origin|vec![json!({"id":"canonical","project_type":"mod","title":"Demo"}).to_string().into_bytes(),json!([{"id":"release","project_id":"canonical","version_type":"release","game_versions":["1.20.1"],"loaders":["fabric"],"files":[{"filename":"demo.jar","url":format!("{origin}/demo.jar"),"size":bytes.len(),"hashes":{"sha1":sha1}}]}]).to_string().into_bytes(),bytes.clone()]).await;
    let prepared = prepare_with(
        &resolver,
        &base,
        &root,
        "versions/pack",
        &[ModReference {
            source: Source::Modrinth,
            project_id: "slug".into(),
            file_id: None,
        }],
    )
    .await
    .unwrap();
    assert_eq!(prepared.files[0].path, "mods/demo.jar");
    assert_eq!(prepared.files[0].bytes, bytes);
    assert!(!base.join("mods/demo.jar").exists());
    let manifest: Value = serde_json::from_slice(&prepared.files[1].bytes).unwrap();
    assert_eq!(manifest["mods"][0]["project_id"], "canonical");
    assert_eq!(manifest["mods"][0]["file_id"], "release");
    let requests = task.await.unwrap();
    let path = requests[1].split_whitespace().nth(1).unwrap();
    let parsed = url::Url::parse(&format!("http://localhost{path}")).unwrap();
    let query: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
    assert_eq!(query["game_versions"], "[\"1.20.1\"]");
    assert_eq!(query["loaders"], "[\"fabric\"]");
    std::fs::remove_dir_all(root).unwrap();
}
