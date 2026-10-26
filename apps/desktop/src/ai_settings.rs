//! Only the credential reference is serialized; secrets live in the OS vault.
use crate::{
    app::{Launcher, Message as AppMessage},
    motion_renderer::Element,
    theme::{self, bold, label, MUTED},
};
use iced::{
    widget::{button, column, container, row, scrollable, text_input, Space},
    Alignment, Fill, Task,
};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Secret(String);
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}
impl From<String> for Secret {
    fn from(value: String) -> Self {
        Self(value)
    }
}
impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub endpoint: String,
    pub model: String,
    credential_id: Option<String>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            endpoint: "https://api.openai.com/v1".into(),
            model: String::new(),
            credential_id: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Connection {
    pub config: Config,
    pub key: Secret,
    pub error: Option<String>,
}
impl Connection {
    pub async fn conversation(
        &self,
        messages: Vec<crate::api_client::ChatMessage>,
        attachments: Vec<crate::attachments::Attachment>,
    ) -> Result<String, String> {
        self.validate()?;
        crate::api_client::conversation_with_attachments(
            self.config.endpoint.clone(),
            self.config.model.clone(),
            self.key.expose().to_owned(),
            messages,
            attachments,
        )
        .await
    }
    pub fn validate(&self) -> Result<(), String> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        crate::api_client::completion_url(&self.config.endpoint)?;
        if self.config.model.trim().is_empty() {
            return Err("请填写模型名称，并在 AI 设置中保存并应用。".into());
        }
        if self.key.expose().chars().any(char::is_control) {
            return Err("API 密钥不能包含换行或控制字符。".into());
        }
        Ok(())
    }
    pub async fn chat(
        &self,
        prompt: String,
        version: String,
        language: String,
    ) -> Result<String, String> {
        self.validate()?;
        crate::api_client::chat(
            self.config.endpoint.clone(),
            self.config.model.clone(),
            self.key.expose().to_owned(),
            prompt,
            version,
            language,
        )
        .await
    }
}

fn read_config(path: &Path) -> Result<Connection, String> {
    let config: Config = match std::fs::read(path) {
        Ok(bytes) => {
            serde_json::from_slice(&bytes).map_err(|_| "AI 配置文件格式无效".to_owned())?
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Connection::default()),
        Err(e) => return Err(format!("无法读取 AI 配置：{e}")),
    };
    // Validate before reading an arbitrary credential reference from disk.
    if let Some(id) = &config.credential_id {
        uuid::Uuid::parse_str(id).map_err(|_| "AI 密钥引用无效")?;
    }
    let key = match &config.credential_id {
        Some(id) => crate::credentials::load_ai_key(id).and_then(|key| {
            key.ok_or_else(|| "系统中找不到已保存的 AI 密钥，请重新填写后保存。".into())
        }),
        None => Ok(String::new()),
    };
    let mut connection = Connection {
        config,
        error: key.as_ref().err().cloned(),
        key: key.unwrap_or_default().into(),
    };
    connection.error = connection.validate().err();
    Ok(connection)
}

async fn save_config(
    path: &Path,
    mut connection: Connection,
    previous_id: Option<String>,
) -> Result<Connection, String> {
    connection.validate()?;
    // Write a fresh vault entry so a failed disk write cannot change the active key.
    let new_id = (!connection.key.expose().is_empty()).then(|| uuid::Uuid::new_v4().to_string());
    if let Some(id) = &new_id {
        crate::credentials::save_ai_key(id, connection.key.expose())?;
    }
    connection.config.credential_id = new_id.clone();
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result: Result<(), String> = async {
        tokio::fs::create_dir_all(path.parent().ok_or("AI 配置路径无效")?)
            .await
            .map_err(|e| e.to_string())?;
        let bytes =
            serde_json::to_vec_pretty(&connection.config).map_err(|_| "AI 配置序列化失败")?;
        tokio::fs::write(&temporary, bytes)
            .await
            .map_err(|e| e.to_string())?;
        tokio::fs::rename(&temporary, path)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    .await;
    if let Err(error) = result {
        let _ = tokio::fs::remove_file(&temporary).await;
        if let Some(id) = new_id {
            let _ = crate::credentials::delete_ai_key(&id);
        }
        return Err(error);
    }
    if let Some(id) = previous_id {
        let _ = crate::credentials::delete_ai_key(&id);
    }
    Ok(connection)
}

#[derive(Debug, Clone)]
pub enum Message {
    McpTab(bool),
    Endpoint(String),
    Model(String),
    Key(Secret),
    Save,
    Saved(Result<Connection, String>),
    Check,
    Checked(Result<String, String>),
}
pub struct State {
    pub mcp_tab: bool,
    pub saved: Connection,
    pub endpoint: String,
    pub model: String,
    pub key: Secret,
    pub busy: bool,
    pub notice: String,
    pub check_notice: String,
}
impl State {
    pub fn new(connection: Connection) -> Self {
        Self {
            mcp_tab: false,
            endpoint: connection.config.endpoint.clone(),
            model: connection.config.model.clone(),
            key: connection.key.clone(),
            notice: connection
                .error
                .clone()
                .unwrap_or_else(|| "修改后点击保存并应用，重启后仍会保留。".into()),
            saved: connection,
            busy: false,
            check_notice: "测试会将一条简短问候发送给当前填写的服务，可能产生 API 费用。".into(),
        }
    }
    pub fn load() -> Self {
        Self::new(
            read_config(&crate::accounts::data_dir().join("ai.json")).unwrap_or_else(|error| {
                Connection {
                    error: Some(error),
                    ..Default::default()
                }
            }),
        )
    }
    pub fn configuration(&self) -> Connection {
        Connection {
            config: Config {
                endpoint: self.endpoint.trim().trim_end_matches('/').into(),
                model: self.model.trim().into(),
                credential_id: None,
            },
            key: self.key.expose().trim().to_owned().into(),
            error: None,
        }
    }
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::McpTab(value) => self.mcp_tab = value,
            Message::Saved(result) => {
                self.busy = false;
                match result {
                    Ok(connection) => {
                        *self = Self::new(connection);
                        self.notice = "AI 配置已保存并应用，后续请求将使用此配置。".into();
                    }
                    Err(error) => self.notice = format!("保存失败，仍使用原配置：{error}"),
                }
            }
            Message::Checked(result) => {
                self.busy = false;
                self.check_notice = result
                    .map(|_| "连接成功，模型已回复。".into())
                    .unwrap_or_else(|e| format!("连接失败：{e}"));
            }
            _ if self.busy => {}
            Message::Endpoint(value) => {
                self.endpoint = value;
                self.edited();
            }
            Message::Model(value) => {
                self.model = value;
                self.edited();
            }
            Message::Key(value) => {
                self.key = value;
                self.edited();
            }
            Message::Save | Message::Check => {
                let connection = self.configuration();
                if let Err(error) = connection.validate() {
                    self.notice = error;
                    return Task::none();
                }
                self.busy = true;
                if matches!(message, Message::Check) {
                    self.check_notice = "正在请求当前填写的模型…".into();
                    return Task::perform(
                        async move {
                            connection
                                .chat(
                                    "请简短回复：连接成功。".into(),
                                    "未指定".into(),
                                    "简体中文".into(),
                                )
                                .await
                        },
                        Message::Checked,
                    );
                }
                self.notice = "正在保存 AI 配置…".into();
                let previous = self.saved.config.credential_id.clone();
                return Task::perform(
                    async move {
                        save_config(
                            &crate::accounts::data_dir().join("ai.json"),
                            connection,
                            previous,
                        )
                        .await
                    },
                    Message::Saved,
                );
            }
        }
        Task::none()
    }
    fn edited(&mut self) {
        self.notice = "有未保存的更改，点击保存并应用后生效。".into();
        self.check_notice =
            "配置已更改。测试会发送简短问候，可能产生 API 费用，不会保存配置。".into();
    }
}
fn dispatch(message: Message) -> AppMessage {
    AppMessage::AiSettings(message)
}
pub fn view(app: &Launcher) -> Element<'_, AppMessage> {
    let mcp = app.ai_settings.mcp_tab;
    column![
        row![
            button(label("模型连接", 12.))
                .padding(10)
                .style(if mcp { theme::quiet } else { theme::selected })
                .on_press(dispatch(Message::McpTab(false))),
            button(label("MCP 服务器", 12.))
                .padding(10)
                .style(if mcp { theme::selected } else { theme::quiet })
                .on_press(dispatch(Message::McpTab(true))),
        ]
        .spacing(8),
        if mcp {
            crate::mcp_settings::view(app)
        } else {
            connection_view(app)
        }
    ]
    .spacing(12)
    .height(Fill)
    .into()
}
fn connection_view(app: &Launcher) -> Element<'_, AppMessage> {
    let state = &app.ai_settings;
    let enabled = !state.busy;
    let content = column![
        bold("AI 服务连接", 24.),
        label(
            "支持 OpenAI Chat Completions 兼容服务，配置用于 AI 咨询与崩溃日志分析。",
            12.
        )
        .color(MUTED),
        label("API 地址", 14.),
        text_input("https://api.openai.com/v1", &state.endpoint)
            .id("ai-endpoint")
            .padding(12)
            .size(14)
            .on_input_maybe(enabled.then_some(|v| dispatch(Message::Endpoint(v)))),
        label(
            "可填写服务根地址、/v1 地址或完整 /chat/completions 地址；本机服务允许 HTTP。",
            11.
        )
        .color(MUTED),
        label("API 密钥", 14.),
        text_input(
            "API Key（本地服务可留空；清空后保存可移除密钥）",
            state.key.expose()
        )
        .id("ai-key")
        .secure(true)
        .padding(12)
        .size(14)
        .on_input_maybe(enabled.then_some(|v: String| dispatch(Message::Key(v.into())))),
        label(
            "密钥保存在 Windows 凭据管理器，本地 JSON 只保存密钥引用。",
            11.
        )
        .color(MUTED),
        label("模型名称", 14.),
        text_input("填写服务提供的模型 ID", &state.model)
            .id("ai-model")
            .padding(12)
            .size(14)
            .on_input_maybe(enabled.then_some(|v| dispatch(Message::Model(v)))),
        label(
            "发送时，所选服务会收到你的提示词、主动附加的日志及 API 密钥。",
            11.
        )
        .color(MUTED),
    ]
    .spacing(12);
    column![
        scrollable(
            container(content)
                .padding(22)
                .width(Fill)
                .style(theme::panel)
        )
        .height(Fill),
        label(&state.check_notice, 11.).color(MUTED).width(Fill),
        row![
            label(&state.notice, 12.).color(MUTED).width(Fill),
            Space::new().width(10),
            button(label("测试连接", 13.))
                .padding(12)
                .style(theme::quiet)
                .on_press_maybe(enabled.then_some(dispatch(Message::Check))),
            button(label(
                if state.busy {
                    "处理中…"
                } else {
                    "保存并应用"
                },
                14.
            ))
            .padding([12, 20])
            .style(theme::action)
            .on_press_maybe(enabled.then_some(dispatch(Message::Save)))
        ]
        .spacing(12)
        .align_y(Alignment::Center)
    ]
    .spacing(14)
    .height(Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture {
        path: std::path::PathBuf,
        ids: Vec<String>,
    }
    impl Fixture {
        fn new() -> Self {
            let dir = std::env::current_dir()
                .unwrap()
                .join("target/ai-settings-tests")
                .join(uuid::Uuid::new_v4().to_string());
            std::fs::create_dir_all(&dir).unwrap();
            Self {
                path: dir.join("ai.json"),
                ids: vec![],
            }
        }
        fn track(&mut self, connection: &Connection) {
            if let Some(id) = &connection.config.credential_id {
                self.ids.push(id.clone());
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            for id in &self.ids {
                let _ = crate::credentials::delete_ai_key(id);
            }
            let dir = self.path.parent().unwrap();
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
            let _ = std::fs::remove_dir(dir);
        }
    }
    fn configured() -> Connection {
        let mut connection = Connection::default();
        connection.config.model = "fixture-model".into();
        connection
    }

    #[test]
    fn validates_drafts_without_applying_them_and_redacts_debug_messages() {
        let mut state = State::new(configured());
        let _ = state.update(Message::Endpoint(" http://127.0.0.1:9999/v1/ ".into()));
        let _ = state.update(Message::Model(" new-model ".into()));
        let message = Message::Key("fixture-secret-never-log".to_owned().into());
        assert!(!format!("{message:?}").contains("fixture-secret-never-log"));
        let _ = state.update(message);
        let draft = state.configuration();
        assert_eq!(draft.config.endpoint, "http://127.0.0.1:9999/v1");
        assert_eq!(draft.config.model, "new-model");
        assert_eq!(state.saved.config.model, "fixture-model");
        let _ = state.update(Message::Saved(Err("disk full".into())));
        assert_eq!(state.saved.config.model, "fixture-model");
        let _ = state.update(Message::Model(" ".into()));
        assert_eq!(state.update(Message::Save).units(), 0);
        let _ = state.update(Message::Model("valid".into()));
        let _ = state.update(Message::Endpoint("http://example.com".into()));
        assert_eq!(state.update(Message::Check).units(), 0);
        let _ = state.update(Message::Endpoint("https://example.com".into()));
        let _ = state.update(Message::Key("invalid\nkey".to_owned().into()));
        assert!(state.configuration().validate().is_err());
    }

    #[tokio::test]
    async fn configuration_round_trip_without_a_key_and_corrupt_file_handling() {
        let fixture = Fixture::new();
        assert!(read_config(&fixture.path).unwrap().config.model.is_empty());
        save_config(&fixture.path, configured(), None)
            .await
            .unwrap();
        let loaded = read_config(&fixture.path).unwrap();
        assert_eq!(loaded.config.model, "fixture-model");
        assert!(loaded.key.expose().is_empty());
        std::fs::write(&fixture.path, b"invalid json").unwrap();
        assert!(read_config(&fixture.path).is_err());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn vault_round_trip_replacement_failure_and_key_removal() {
        let mut fixture = Fixture::new();
        let mut original = configured();
        original.key = "fixture-secret-one".to_owned().into();
        let saved = save_config(&fixture.path, original, None).await.unwrap();
        fixture.track(&saved);
        let original_id = saved.config.credential_id.clone().unwrap();
        let json = std::fs::read_to_string(&fixture.path).unwrap();
        assert!(!json.contains("fixture-secret-one"));
        assert_eq!(
            read_config(&fixture.path).unwrap().key.expose(),
            "fixture-secret-one"
        );
        let mut replacement = saved.clone();
        replacement.key = "fixture-secret-two".to_owned().into();
        let blocked = fixture.path.parent().unwrap().join("blocked.json");
        std::fs::create_dir(&blocked).unwrap();
        assert!(
            save_config(&blocked, replacement.clone(), Some(original_id.clone()))
                .await
                .is_err()
        );
        std::fs::remove_dir(blocked).unwrap();
        assert_eq!(std::fs::read_to_string(&fixture.path).unwrap(), json);
        assert_eq!(
            read_config(&fixture.path).unwrap().key.expose(),
            "fixture-secret-one"
        );
        let saved = save_config(&fixture.path, replacement, Some(original_id.clone()))
            .await
            .unwrap();
        fixture.track(&saved);
        assert!(crate::credentials::load_ai_key(&original_id)
            .unwrap()
            .is_none());
        let new_id = saved.config.credential_id.clone().unwrap();
        assert_eq!(
            read_config(&fixture.path).unwrap().key.expose(),
            "fixture-secret-two"
        );
        crate::credentials::delete_ai_key(&new_id).unwrap();
        let missing = read_config(&fixture.path).unwrap();
        assert_eq!(missing.config.model, "fixture-model");
        assert!(
            missing.validate().is_err(),
            "missing saved key must block requests"
        );
        crate::credentials::save_ai_key(&new_id, "fixture-secret-two").unwrap();
        let saved = save_config(&fixture.path, configured(), Some(new_id.clone()))
            .await
            .unwrap();
        assert!(saved.config.credential_id.is_none());
        assert!(read_config(&fixture.path).unwrap().key.expose().is_empty());
        assert!(crate::credentials::load_ai_key(&new_id).unwrap().is_none());
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn persisted_settings_drive_the_actual_ui_request_and_reply() {
        use iced::futures::StreamExt;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut fixture = Fixture::new();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut connection = configured();
        connection.config.endpoint = format!("http://{}/custom/v1", listener.local_addr().unwrap());
        connection.key = "fixture-wire-secret".to_owned().into();
        let saved = save_config(&fixture.path, connection, None).await.unwrap();
        fixture.track(&saved);
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let (header_end, length) = loop {
                let mut chunk = [0; 4096];
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&chunk[..count]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    break (end + 4, length);
                }
            };
            while bytes.len() < header_end + length {
                let mut chunk = [0; 4096];
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0);
                bytes.extend_from_slice(&chunk[..count]);
            }
            let header = String::from_utf8_lossy(&bytes[..header_end]).to_lowercase();
            assert!(header.starts_with("post /custom/v1/chat/completions http/1.1"));
            assert!(header.contains("authorization: bearer fixture-wire-secret"));
            let body: serde_json::Value =
                serde_json::from_slice(&bytes[header_end..header_end + length]).unwrap();
            assert_eq!(body["model"], "fixture-model");
            assert!(body["messages"][1]["content"]
                .as_str()
                .unwrap()
                .contains("测试持久化请求"));
            let response = r#"{"choices":[{"message":{"content":"fixture-reply"}}]}"#;
            socket.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()).as_bytes()).await.unwrap();
        });
        let (mut app, _) = crate::app::Launcher::new();
        app.ai_settings = State::new(read_config(&fixture.path).unwrap());
        app.ai.connection = app.ai_settings.saved.clone();
        let _ = app.update(AppMessage::AiSettings(Message::Model(
            "unsaved-model".into(),
        )));
        assert_eq!(app.ai.connection.config.model, "fixture-model");
        let _ = app.update(AppMessage::Ai(crate::ai::Message::Prompt(
            "测试持久化请求".into(),
        )));
        let task = app.update(AppMessage::Ai(crate::ai::Message::Submit));
        assert!(app.ai.busy);
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let mut stream = iced_test::runtime::task::into_stream(task).unwrap();
            while let Some(action) = stream.next().await {
                if let iced_test::runtime::Action::Output(message) = action {
                    let _ = app.update(message);
                }
            }
            server.await.unwrap();
        })
        .await
        .unwrap();
        assert!(!app.ai.busy);
        assert_eq!(app.ai.messages.last().unwrap().content, "fixture-reply");
    }
}
