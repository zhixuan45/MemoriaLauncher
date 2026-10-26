use super::*;
use iced::futures::StreamExt;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn root() -> PathBuf {
    std::env::temp_dir().join(format!("memoria-attachments-{}", Uuid::new_v4()))
}
fn fixture_files(root: &std::path::Path) -> Vec<PathBuf> {
    std::fs::create_dir_all(root).unwrap();
    std::fs::write(root.join("settings.txt"), b"renderDistance=12").unwrap();
    std::fs::write(
        root.join("screen.png"),
        include_bytes!("../ui/assets/icon_grass.png"),
    )
    .unwrap();
    vec![root.join("settings.txt"), root.join("screen.png")]
}
#[test]
fn selected_files_are_bounded_and_binary_content_is_typed_without_debug_disclosure() {
    let directory = root();
    let files = load_files(fixture_files(&directory)).unwrap();
    assert!(matches!(&files[0].content, Content::Text(t) if t == "renderDistance=12"));
    assert_eq!(files[1].part().unwrap()["type"], "image_url");
    assert!(!format!("{files:?}").contains("renderDistance=12"));
    std::fs::write(directory.join("example.pdf"), b"%PDF-1.7\nfixture").unwrap();
    let pdf = load_files(vec![directory.join("example.pdf")]).unwrap();
    assert_eq!(pdf[0].part().unwrap()["file"]["filename"], "example.pdf");
    let large = directory.join("large.txt");
    std::fs::File::create(&large)
        .unwrap()
        .set_len(MAX_FILE_BYTES as u64 + 1)
        .unwrap();
    assert!(load_files(vec![large]).is_err());
    std::fs::write(directory.join("binary.jar"), b"\xff\x00\x01").unwrap();
    assert!(load_files(vec![directory.join("binary.jar")]).is_err());
    assert!(load_files(vec![directory.join("settings.txt"); 5]).is_err());
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn opt_in_log_context_uses_only_the_selected_instance_and_ignores_a_disabled_reply() {
    let directory = root();
    std::fs::create_dir_all(directory.join("versions/one/logs")).unwrap();
    std::fs::write(
        directory.join("versions/one/logs/latest.log"),
        "Authorization: Bearer secret\nMissing dependency\n",
    )
    .unwrap();
    let mut state = State::default();
    state.target = Some(LogTarget {
        root: directory.clone(),
        instance_id: "versions/one".into(),
    });
    assert!(state.text().is_empty());
    let task = state.update(Message::Logs(true), false);
    let mut stream = iced_test::runtime::task::into_stream(task).unwrap();
    while let Some(action) = stream.next().await {
        if let iced_test::runtime::Action::Output(message) = action {
            let _ = state.update(message, false);
        }
    }
    assert!(state.log_enabled);
    assert!(state.text().contains("Missing dependency"));
    assert!(!state.text().contains("Bearer secret"));
    let generation = state.generation;
    let old = state.log.clone().unwrap();
    let _ = state.update(Message::Logs(false), false);
    let _ = state.update(Message::LogLoaded(generation, Ok(old)), false);
    assert!(state.text().is_empty());
    assert!(!state.log_enabled);
    std::fs::remove_dir_all(directory).unwrap();
}

#[tokio::test]
async fn actual_chat_request_sends_image_and_file_text_only_after_submit_and_does_not_keep_raw_files_in_history(
) {
    tokio::time::timeout(std::time::Duration::from_secs(15), async {
        let directory = root();
        let files = load_files(fixture_files(&directory)).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = vec![];
            let (end, length) = loop {
                let mut buffer = [0; 4096];
                let count = socket.read(&mut buffer).await.unwrap(); assert!(count > 0);
                bytes.extend_from_slice(&buffer[..count]);
                if let Some(end) = bytes.windows(4).position(|p| p == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    let length: usize = headers.lines().find_map(|line| line.strip_prefix("content-length:")).unwrap().trim().parse().unwrap();
                    break (end + 4, length);
                }
            };
            while bytes.len() < end + length {
                let mut buffer = [0; 4096]; let count = socket.read(&mut buffer).await.unwrap(); assert!(count > 0); bytes.extend_from_slice(&buffer[..count]);
            }
            let payload: Value = serde_json::from_slice(&bytes[end..end + length]).unwrap();
            assert!(payload["messages"][1]["content"][0]["text"].as_str().unwrap().contains("renderDistance=12"));
            assert!(payload["messages"][1]["content"][1]["image_url"]["url"].as_str().unwrap().starts_with("data:image/png;base64,"));
            let response = r#"{"choices":[{"message":{"content":"attachment reply"}}]}"#;
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await.unwrap();
        });
        let mut state = crate::ai::State::default();
        state.connection.config.model = "fixture-model".into();
        state.connection.config.endpoint = endpoint;
        let _ = state.update(crate::ai::Message::Attachments(Message::Loaded(Ok(files))), std::time::Instant::now());
        assert!(!state.busy, "selecting a file must not send it");
        let task = state.update(crate::ai::Message::Submit, std::time::Instant::now());
        let mut stream = iced_test::runtime::task::into_stream(task).unwrap();
        while let Some(action) = stream.next().await {
            if let iced_test::runtime::Action::Output(message) = action { let _ = state.update(message, std::time::Instant::now()); }
        }
        server.await.unwrap();
        assert_eq!(state.messages[1].content, "attachment reply");
        assert!(state.attachments.files.is_empty());
        assert!(state.messages[0].content.contains("screen.png"));
        assert!(!state.messages[0].content.contains("renderDistance=12"));
        std::fs::remove_dir_all(directory).unwrap();
    }).await.unwrap();
}
