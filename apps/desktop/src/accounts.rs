use crate::credentials::SystemCredentialStore;
use iced::Task;
use mc_core::{AuthAccount, MicrosoftAuthConfig, MicrosoftAuthProvider};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Notify,
};

pub type Provider = MicrosoftAuthProvider<SystemCredentialStore>;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountIndex {
    pub accounts: Vec<AuthAccount>,
    pub active: Option<String>,
}

impl AccountIndex {
    pub fn current(&self) -> Option<&AuthAccount> {
        self.accounts
            .iter()
            .find(|a| Some(&a.id) == self.active.as_ref())
    }

    fn reconcile(&mut self) {
        if self.current().is_none() {
            self.active = self.accounts.first().map(|a| a.id.clone());
        }
    }

    fn upsert(&mut self, account: AuthAccount) {
        self.accounts.retain(|a| a.id != account.id);
        self.active = Some(account.id.clone());
        self.accounts.push(account);
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    Loaded(Result<AccountIndex, String>),
    Login,
    CancelLogin,
    LoggedIn(Result<AuthAccount, String>),
    Select(String),
    Remove(String),
    Saved(Result<AccountIndex, String>),
}

pub struct State {
    pub index: AccountIndex,
    pub busy: bool,
    pub logging_in: bool,
    pub notice: String,
    pub provider: Result<Provider, String>,
    config: Result<MicrosoftAuthConfig, String>,
    path: PathBuf,
    cancel: Arc<Notify>,
    ready: bool,
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("MEMORIA_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("MemoriaLauncher"))
        })
        .or_else(|| {
            std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share/memoria-launcher"))
        })
        .unwrap_or_else(|| PathBuf::from(".memoria-launcher"))
}

impl Default for State {
    fn default() -> Self {
        let config = configured_auth();
        let provider = config
            .clone()
            .map(|config| Provider::new(config, Arc::new(SystemCredentialStore)));
        Self {
            index: AccountIndex::default(),
            busy: true,
            ready: false,
            logging_in: false,
            notice: "正在读取已保存的账户…".into(),
            provider,
            config,
            path: data_dir().join("accounts.json"),
            cancel: Arc::new(Notify::new()),
        }
    }
}

fn configured_client_id() -> String {
    std::env::var("MC_LAUNCHER_CLIENT_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            option_env!("MEMORIA_MICROSOFT_CLIENT_ID")
                .filter(|value| !value.trim().is_empty())
                .map(str::to_owned)
        })
        .unwrap_or_default()
}

fn configured_auth() -> Result<MicrosoftAuthConfig, String> {
    let client_id = configured_client_id();
    if client_id.trim().is_empty() {
        return Err(
            "当前发行版尚未配置 Microsoft 登录应用，请联系启动器开发者或更新发行版。".into(),
        );
    }
    let redirect_uri = std::env::var("MC_LAUNCHER_REDIRECT_URI")
        .unwrap_or_else(|_| "http://localhost:53682/oauth/callback".into());
    callback_address(&redirect_uri)?;
    MicrosoftAuthConfig::new(client_id.trim(), redirect_uri).map_err(|e| e.to_string())
}

async fn read_index(path: PathBuf) -> Result<AccountIndex, String> {
    match tokio::fs::read(path).await {
        Ok(bytes) => {
            let mut index: AccountIndex =
                serde_json::from_slice(&bytes).map_err(|e| format!("账户记录损坏：{e}"))?;
            index.reconcile();
            Ok(index)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(AccountIndex::default()),
        Err(e) => Err(format!("读取账户记录失败：{e}")),
    }
}

async fn write_index(path: &Path, index: AccountIndex) -> Result<AccountIndex, String> {
    let bytes = serde_json::to_vec_pretty(&index).map_err(|e| e.to_string())?;
    tokio::fs::create_dir_all(path.parent().ok_or("无效的账户存储路径")?)
        .await
        .map_err(|e| e.to_string())?;
    let temporary = path.with_extension("json.tmp");
    tokio::fs::write(&temporary, bytes)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::rename(&temporary, path)
        .await
        .map_err(|e| format!("保存账户记录失败：{e}"))?;
    Ok(index)
}

impl State {
    pub fn has_microsoft_app(&self) -> bool {
        self.config.is_ok()
    }

    pub fn load(&self) -> Task<Message> {
        Task::perform(read_index(self.path.clone()), Message::Loaded)
    }

    pub fn name(&self) -> &str {
        self.index
            .current()
            .map(|a| a.username.as_str())
            .unwrap_or("未登录")
    }

    pub fn description(&self) -> &str {
        if self.index.current().is_some() {
            "Microsoft · 启动时验证会话"
        } else {
            "添加 Microsoft 账户"
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Loaded(result) => {
                self.busy = false;
                match result {
                    Ok(index) => {
                        self.index = index;
                        self.ready = true;
                        self.notice = self
                            .provider
                            .as_ref()
                            .err()
                            .cloned()
                            .unwrap_or_else(|| "登录后可选择账户启动游戏。".into());
                    }
                    Err(e) => self.notice = e,
                }
            }
            Message::Login if !self.busy && self.ready => {
                let provider = match &self.provider {
                    Ok(provider) => provider.clone(),
                    Err(e) => {
                        self.notice = e.clone();
                        return Task::none();
                    }
                };
                self.busy = true;
                self.logging_in = true;
                self.cancel = Arc::new(Notify::new());
                let cancel = self.cancel.clone();
                let config = match &self.config {
                    Ok(config) => config.clone(),
                    Err(error) => {
                        self.busy = false;
                        self.logging_in = false;
                        self.notice = error.clone();
                        return Task::none();
                    }
                };
                self.notice = "请在浏览器完成 Microsoft 登录；等待回调（最多 3 分钟）。".into();
                return Task::perform(
                    async move {
                        tokio::select! {
                            result = browser_login(provider, config) => result,
                            _ = cancel.notified() => Err("已取消登录".into()),
                        }
                    },
                    Message::LoggedIn,
                );
            }
            Message::CancelLogin => self.cancel.notify_one(),
            Message::LoggedIn(result) => {
                self.logging_in = false;
                match result {
                    Ok(account) => {
                        let mut index = self.index.clone();
                        index.upsert(account);
                        let path = self.path.clone();
                        self.notice = "登录成功，正在保存账户…".into();
                        return Task::perform(
                            async move { write_index(&path, index).await },
                            Message::Saved,
                        );
                    }
                    Err(e) => {
                        self.busy = false;
                        self.notice = e;
                    }
                }
            }
            Message::Select(id) if !self.busy && self.ready => {
                if self.index.accounts.iter().any(|a| a.id == id) {
                    let mut index = self.index.clone();
                    index.active = Some(id);
                    self.busy = true;
                    let path = self.path.clone();
                    return Task::perform(
                        async move { write_index(&path, index).await },
                        Message::Saved,
                    );
                }
            }
            Message::Remove(id) if !self.busy && self.ready => {
                if !self.index.accounts.iter().any(|a| a.id == id) {
                    return Task::none();
                }
                let mut index = self.index.clone();
                index.accounts.retain(|a| a.id != id);
                index.reconcile();
                let path = self.path.clone();
                self.busy = true;
                return Task::perform(
                    async move {
                        use mc_core::CredentialStore;
                        SystemCredentialStore
                            .delete(&id)
                            .await
                            .map_err(|e| e.to_string())?;
                        write_index(&path, index).await
                    },
                    Message::Saved,
                );
            }
            Message::Saved(result) => {
                self.busy = false;
                match result {
                    Ok(index) => {
                        self.index = index;
                        self.notice = "账户记录已保存。".into();
                    }
                    Err(e) => self.notice = e,
                }
            }
            _ => {}
        }
        Task::none()
    }
}

fn callback_address(redirect: &str) -> Result<(url::Url, u16), String> {
    // Validate the raw URI before URL parsing can normalize traversal segments.
    let remainder = redirect
        .strip_prefix("http://")
        .and_then(|rest| rest.split_once('/').map(|(_, path)| path))
        .ok_or("登录回调必须包含 HTTP 路径")?;
    if remainder.is_empty() || remainder.contains("..") || remainder.contains('\\') {
        return Err("登录回调路径无效".into());
    }
    let url = url::Url::parse(redirect).map_err(|_| "登录回调地址无效")?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("localhost" | "127.0.0.1"))
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err("桌面登录需要 http://localhost:端口/路径 或 127.0.0.1 回调地址。".into());
    }
    let port = url
        .port()
        .filter(|port| *port != 0)
        .ok_or("登录回调缺少有效的显式端口")?;
    Ok((url, port))
}

async fn browser_login(
    provider: Provider,
    config: MicrosoftAuthConfig,
) -> Result<AuthAccount, String> {
    let (redirect, port) = callback_address(&config.redirect_uri)?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
        .await
        .map_err(|e| format!("无法监听登录回调端口 {port}：{e}"))?;
    let login = provider.begin_login();
    open_browser(&login.authorization_url)?;
    let callback = receive_callback(listener, &redirect, &login.state).await?;
    provider
        .complete_login(&login, &callback)
        .await
        .map_err(|e| e.to_string())
}

async fn receive_callback(
    listener: TcpListener,
    redirect: &url::Url,
    expected_state: &str,
) -> Result<String, String> {
    tokio::time::timeout(Duration::from_secs(180), async {
        loop {
            let (mut stream, _) = listener.accept().await.map_err(|e| e.to_string())?;
            let mut request = Vec::new();
            let read = tokio::time::timeout(Duration::from_secs(5), async {
                while request.len() < 8192 && !request.ends_with(b"\r\n\r\n") {
                    let byte = stream.read_u8().await?;
                    request.push(byte);
                }
                Ok::<_, std::io::Error>(())
            }).await;
            if !matches!(read, Ok(Ok(()))) || !request.ends_with(b"\r\n\r\n") { continue; }
            let raw = String::from_utf8_lossy(&request);
            let mut line = raw.lines().next().unwrap_or_default().split_whitespace();
            let method = line.next();
            let target = line.next().unwrap_or_default();
            let Ok(url) = redirect.join(target) else { continue; };
            let state = url.query_pairs().find(|(key, _)| key == "state").map(|(_, value)| value.into_owned());
            if method != Some("GET") || !target.starts_with('/') || url.origin() != redirect.origin()
                || target.split('?').next() != Some(redirect.path())
                || url.path() != redirect.path() || state.as_deref() != Some(expected_state)
                || !url.query_pairs().any(|(key, _)| key == "code" || key == "error") {
                let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await;
                continue;
            }
            let body = "Login response received. You can return to Memoria Launcher.";
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
            let _ = stream.write_all(response.as_bytes()).await;
            if url.query_pairs().any(|(key, _)| key == "error") {
                return Err("Microsoft 授权被取消或拒绝，请重新登录。".into());
            }
            break Ok::<_, String>(url.to_string());
        }
    }).await.map_err(|_| "登录超时，请重新尝试。".to_string())?
}

fn open_browser(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        let mut command = std::process::Command::new("rundll32.exe");
        command
            .args(["url.dll,FileProtocolHandler", url])
            .creation_flags(0x08000000);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = std::process::Command::new("open");
        command.arg(url);
        command
    };
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let mut command = {
        let mut command = std::process::Command::new("xdg-open");
        command.arg(url);
        command
    };
    command
        .spawn()
        .map_err(|e| format!("无法打开登录浏览器：{e}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_deduplicates_accounts_and_reconciles_removed_selection() {
        let mut index = AccountIndex::default();
        for name in ["Before", "After"] {
            index.upsert(AuthAccount {
                id: "a".into(),
                uuid: "uuid".into(),
                username: name.into(),
            });
        }
        assert_eq!(index.accounts.len(), 1);
        assert_eq!(index.current().unwrap().username, "After");
        index.accounts.clear();
        index.reconcile();
        assert!(index.active.is_none());
    }

    #[tokio::test]
    async fn index_round_trip_and_corruption_are_explicit() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-fixtures")
            .join(format!("memoria-accounts-{}.json", std::process::id()));
        let mut index = AccountIndex::default();
        index.upsert(AuthAccount {
            id: "a".into(),
            username: "Player".into(),
            uuid: "uuid".into(),
        });
        write_index(&path, index.clone()).await.unwrap();
        write_index(&path, index).await.unwrap(); // Replacing an existing index must work on Windows.
        assert_eq!(
            read_index(path.clone())
                .await
                .unwrap()
                .current()
                .unwrap()
                .username,
            "Player"
        );
        let json = tokio::fs::read_to_string(&path).await.unwrap();
        assert!(!json.contains("token"));
        tokio::fs::write(&path, "invalid json").await.unwrap();
        assert!(read_index(path.clone()).await.is_err());
        tokio::fs::remove_file(path).await.unwrap();
    }

    #[test]
    fn callback_only_accepts_loopback_http() {
        assert!(callback_address("http://localhost:53682/oauth/callback").is_ok());
        for url in [
            "https://example.com/callback",
            "http://0.0.0.0/callback",
            "file:///tmp/token",
            "http://localhost/?code=x",
            "http://localhost:0/oauth/callback",
            "http://localhost:53682/oauth/../admin",
            "http://localhost:53682/",
            "http://localhost:53682/oauth/callback#fragment",
            "http://user:pass@localhost:53682/oauth/callback",
        ] {
            assert!(callback_address(url).is_err());
        }
    }

    #[tokio::test]
    async fn loopback_callback_rejects_wrong_state_then_accepts_matching_response() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let redirect = url::Url::parse(&format!("http://{address}/oauth/callback")).unwrap();
        let receiver =
            tokio::spawn(
                async move { receive_callback(listener, &redirect, "expected-state").await },
            );
        for (target, status) in [
            (
                "/oauth/callback?code=test-code&state=wrong-state",
                "400 Bad Request",
            ),
            ("/oauth/callback?state=expected-state", "400 Bad Request"),
            (
                "/oauth/callback/evil?code=test-code&state=expected-state",
                "400 Bad Request",
            ),
            (
                "/oauth/../oauth/callback?code=test-code&state=expected-state",
                "400 Bad Request",
            ),
            (
                "/oauth/callback?code=test-code&state=expected%2Dstate",
                "200 OK",
            ),
        ] {
            let mut client = tokio::net::TcpStream::connect(address).await.unwrap();
            let request = format!("GET {target} HTTP/1.1\r\nHost: {address}\r\n\r\n");
            client.write_all(request.as_bytes()).await.unwrap();
            let mut response = String::new();
            tokio::time::timeout(Duration::from_secs(5), client.read_to_string(&mut response))
                .await
                .unwrap()
                .unwrap();
            assert!(response.starts_with(&format!("HTTP/1.1 {status}")));
        }
        let callback = receiver.await.unwrap().unwrap();
        assert!(callback.contains("code=test-code"));
    }

    #[test]
    fn starts_without_fake_accounts_and_reports_publisher_configuration() {
        let state = State::default();
        assert!(state.index.current().is_none());
        assert!(state.index.accounts.is_empty());
        assert_eq!(state.has_microsoft_app(), state.provider.is_ok());
    }
}
