//! Execute the actual UI Tasks against an HTTP fixture, including conversation context.
use crate::{
    ai::{Message, State},
    api_client::Role,
};
use iced::futures::StreamExt;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn execute(state: &mut State, task: iced::Task<Message>) {
    let mut stream = iced_test::runtime::task::into_stream(task).unwrap();
    while let Some(action) = stream.next().await {
        if let iced_test::runtime::Action::Output(message) = action {
            let _ = state.update(message, Instant::now());
        }
    }
}
#[tokio::test]
async fn actual_chat_tasks_send_previous_turns_and_handle_http_failure() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/custom/v1", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let mut payloads = vec![];
            for turn in 0..3 {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut bytes = vec![];
                let (end, length) = loop {
                    let mut buffer = [0; 4096];
                    let count = socket.read(&mut buffer).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                    if let Some(end) = bytes.windows(4).position(|p| p == b"\r\n\r\n") {
                        let header = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                        assert!(header.starts_with("post /custom/v1/chat/completions http/1.1"));
                        assert!(header.contains("authorization: bearer fixture-key"));
                        let length: usize = header.lines().find_map(|l| l.strip_prefix("content-length:")).unwrap().trim().parse().unwrap();
                        break (end + 4, length);
                    }
                };
                while bytes.len() < end + length {
                    let mut buffer = [0; 4096];
                    let count = socket.read(&mut buffer).await.unwrap();
                    assert!(count > 0);
                    bytes.extend_from_slice(&buffer[..count]);
                }
                payloads.push(serde_json::from_slice::<Value>(&bytes[end..end + length]).unwrap());
                let status = if turn == 2 { "503 Unavailable" } else { "200 OK" };
                let body = json!({"choices":[{"message":{"content":format!("reply {turn}")}}]}).to_string();
                socket.write_all(format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
            }
            payloads
        });
        let mut state = State::default();
        state.connection.config.endpoint = endpoint;
        state.connection.config.model = "fixture-model".into();
        state.connection.key = "fixture-key".to_owned().into();
        for prompt in ["第一条问题", "解释上一条回复", "触发失败"] {
            let _ = state.update(Message::Prompt(prompt.into()), Instant::now());
            let task = state.update(Message::Submit, Instant::now());
            execute(&mut state, task).await;
            assert!(!state.busy);
        }
        assert_eq!(state.messages.len(), 4);
        assert_eq!(state.messages[3].role, Role::Assistant);
        assert_eq!(state.prompt, "触发失败");
        let payloads = server.await.unwrap();
        assert_eq!(payloads[0]["messages"].as_array().unwrap().len(), 2);
        assert_eq!(payloads[1]["messages"].as_array().unwrap().len(), 4);
        assert_eq!(payloads[1]["messages"][1]["content"], "第一条问题");
        assert_eq!(payloads[1]["messages"][2], json!({"role":"assistant", "content":"reply 0"}));
        assert_eq!(payloads[1]["messages"][3]["content"], "解释上一条回复");
        assert_eq!(payloads[1]["model"], "fixture-model");
    }).await.unwrap();
}
