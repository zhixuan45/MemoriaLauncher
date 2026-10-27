use super::*;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn releases() -> Vec<serde_json::Value> {
    [
        ("v0.2.0-beta.2", false, true),
        ("v0.1.10", false, false),
        ("v0.1.9", false, false),
        ("v9.0.0", true, false),
        ("bad-tag", false, false),
    ]
    .into_iter()
    .map(|(tag, draft, prerelease)| {
        json!({
            "tag_name": tag, "draft": draft, "prerelease": prerelease,
            "html_url": format!("{RELEASE_PREFIX}{tag}"),
        })
    })
    .collect()
}

#[test]
fn selects_semver_channel_and_rejects_untrusted_links() {
    let mut releases = releases();
    let bytes = serde_json::to_vec(&releases).unwrap();
    assert_eq!(
        select(&bytes, "0.1.0", false).unwrap().unwrap().version,
        "v0.1.10"
    );
    assert_eq!(
        select(&bytes, "0.1.0", true).unwrap().unwrap().version,
        "v0.2.0-beta.2"
    );
    assert_eq!(select(&bytes, "1.0.0", true).unwrap(), None);
    assert_eq!(select(b"[]", "0.1.0", false).unwrap(), None);
    assert!(select(b"{}", "0.1.0", false).is_err());
    for invalid in [
        "https://github.com/zhixuan45/MemoriaLauncher/releases/tag/../edit",
        "https://github.com/zhixuan45/MemoriaLauncher/releases/tag/%2e%2e/edit",
        "https://github.com/zhixuan45/MemoriaLauncher/releases/tag/",
        "https://github.com/zhixuan45/MemoriaLauncher/releases/tag/v1.0.0?redirect=external",
    ] {
        assert!(release_url(invalid).is_err());
        assert!(open_release(invalid).is_err());
    }
    releases[1]["html_url"] = json!("https://github.com.evil.example/releases/tag/v0.1.10");
    assert!(select(&serde_json::to_vec(&releases).unwrap(), "0.1.0", false).is_err());
    let build = json!([{"tag_name":"v0.1.0+build2", "draft":false, "prerelease":false,
        "html_url":format!("{RELEASE_PREFIX}v0.1.0+build2")}]);
    assert!(
        select(&serde_json::to_vec(&build).unwrap(), "0.1.0+build1", false)
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn real_http_request_checks_status_response_limits_and_release_data() {
    mc_core::initialize_runtime();
    tokio::time::timeout(Duration::from_secs(15), async {
        for (status, oversized) in [(200, false), (403, false), (302, false), (500, false), (200, true)] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!("http://{}/releases?per_page=100", listener.local_addr().unwrap());
            let server = tokio::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut request = vec![];
                while !request.ends_with(b"\r\n\r\n") {
                    request.push(stream.read_u8().await.unwrap());
                    assert!(request.len() < 8192);
                }
                let request = String::from_utf8(request).unwrap().to_lowercase();
                assert!(request.starts_with("get /releases?per_page=100 http/1.1"));
                assert!(request.contains("user-agent: memorialauncher/"));
                assert!(request.contains("accept: application/vnd.github+json"));
                let body = serde_json::to_vec(&releases()).unwrap();
                let length = if oversized { MAX_BYTES + 1 } else { body.len() };
                stream.write_all(format!("HTTP/1.1 {status} Test\r\nContent-Length: {length}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
                if !oversized { stream.write_all(&body).await.unwrap(); }
            });
            let result = check_at(&endpoint, "0.1.0", false).await;
            if oversized { assert!(result.unwrap_err().contains("响应过大")); }
            else if status == 200 { assert_eq!(result.unwrap().unwrap().version, "v0.1.10"); }
            else { assert!(result.unwrap_err().contains(&status.to_string())); }
            server.await.unwrap();
        }
    }).await.unwrap();
}
