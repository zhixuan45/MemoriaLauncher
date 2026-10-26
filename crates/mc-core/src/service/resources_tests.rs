use super::*;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn fixture(body: String, status: u16) -> (ResourceCatalog, tokio::task::JoinHandle<String>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut catalog = ResourceCatalog::new().unwrap();
    catalog.base = format!("http://{}/v2", listener.local_addr().unwrap());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut header = vec![];
        while !header.windows(4).any(|part| part == b"\r\n\r\n") {
            let mut buffer = [0; 4096];
            let count = socket.read(&mut buffer).await.unwrap();
            assert!(count > 0);
            header.extend_from_slice(&buffer[..count]);
        }
        socket.write_all(format!("HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        String::from_utf8(header).unwrap()
    });
    (catalog, server)
}
fn request_url(header: &str) -> url::Url {
    url::Url::parse(&format!(
        "http://localhost{}",
        header.split_whitespace().nth(1).unwrap()
    ))
    .unwrap()
}
fn query(kind: ResourceKind) -> ResourceQuery {
    ResourceQuery {
        kind,
        query: "光影 & shader".into(),
        game_version: "1.21.1".into(),
        loader: "fabric".into(),
        offset: 20,
    }
}

#[tokio::test]
async fn all_categories_encode_facets_and_pagination_on_the_wire() {
    for kind in [
        ResourceKind::Mod,
        ResourceKind::Modpack,
        ResourceKind::Resourcepack,
        ResourceKind::Shader,
    ] {
        let body = json!({"hits": [{"project_id":"id", "slug":"sample", "title":"Sample", "description":"From wire", "author":"author", "downloads":42, "categories":["fabric"], "versions":["1.21.1"]}], "total_hits":41, "offset":20, "limit":20});
        let (catalog, server) = fixture(body.to_string(), 200).await;
        let page = catalog.search(&query(kind)).await.unwrap();
        assert_eq!(page.hits[0].title, "Sample");
        assert_eq!(page.total_hits, 41);
        let header = server.await.unwrap();
        let url = request_url(&header);
        assert_eq!(url.path(), "/v2/search");
        let values: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(values["query"], "光影 & shader");
        assert_eq!(values["offset"], "20");
        let facets: serde_json::Value = serde_json::from_str(&values["facets"]).unwrap();
        assert_eq!(
            facets,
            json!([
                [format!("project_type:{}", kind.as_str())],
                ["versions:1.21.1"],
                ["categories:fabric"]
            ])
        );
    }
}

#[tokio::test]
async fn versions_decode_files_dependencies_and_encode_filters() {
    let body = json!([{"id":"version", "name":"Release", "version_number":"1.0", "version_type":"release", "game_versions":["1.21.1"], "loaders":["fabric"], "files":[{"filename":"sample.jar","size":1234}], "dependencies":[{"project_id":"dependency","version_id":null,"dependency_type":"required"}]}]);
    let (catalog, server) = fixture(body.to_string(), 200).await;
    let versions = catalog
        .versions("sample", "1.21.1", "fabric")
        .await
        .unwrap();
    assert_eq!(versions[0].files[0].filename, "sample.jar");
    assert_eq!(versions[0].dependencies[0].dependency_type, "required");
    let header = server.await.unwrap();
    let url = request_url(&header);
    assert_eq!(url.path(), "/v2/project/sample/version");
    let values: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    assert_eq!(values["game_versions"], "[\"1.21.1\"]");
    assert_eq!(values["loaders"], "[\"fabric\"]");
    for invalid in ["../escape", "id?x=y", "", "a/b"] {
        assert!(catalog.versions(invalid, "", "").await.is_err());
    }
}

#[tokio::test]
async fn errors_and_invalid_pages_are_reported_without_fabricated_results() {
    for (body, status) in [
        ("{}", 429),
        ("not json", 200),
        (r#"{"hits":[],"total_hits":0,"offset":0,"limit":20}"#, 200),
    ] {
        let (catalog, server) = fixture(body.into(), status).await;
        assert!(catalog.search(&query(ResourceKind::Mod)).await.is_err());
        server.await.unwrap();
    }
}

#[tokio::test]
#[ignore = "requires live Modrinth access"]
async fn live_resource_categories_and_versions_are_reachable() {
    let catalog = ResourceCatalog::new().unwrap();
    for kind in [
        ResourceKind::Mod,
        ResourceKind::Modpack,
        ResourceKind::Resourcepack,
        ResourceKind::Shader,
    ] {
        let query = ResourceQuery {
            kind,
            query: String::new(),
            game_version: String::new(),
            loader: String::new(),
            offset: 0,
        };
        let page = catalog.search(&query).await.unwrap();
        assert!(!page.hits.is_empty());
        let versions = catalog
            .versions(&page.hits[0].project_id, "", "")
            .await
            .unwrap();
        assert!(!versions.is_empty());
        assert!(!versions[0].files.is_empty());
    }
}
