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

const DEFAULT_REDIRECT_URI: &str = "http://localhost:53682/oauth/callback";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginSettings {
    pub client_id: String,
    pub redirect_uri: String,
}

impl LoginSettings {
    fn fallback() -> Self {
        Self {
            client_id: configured_client_id(),
            redirect_uri: std::env::var("MC_LAUNCHER_REDIRECT_URI")
                .unwrap_or_else(|_| DEFAULT_REDIRECT_URI.into()),
        }
    }

    fn normalized(&self) -> Self {
        Self {
            client_id: self.client_id.trim().into(),
            redirect_uri: self.redirect_uri.trim().into(),
        }
    }

    fn auth_config(&self) -> Result<MicrosoftAuthConfig, String> {
        if self.client_id.trim().is_empty() {
            return Err("请在设置 → 账户登录中填写 Microsoft 应用的客户端 ID。".into());
        }
        uuid::Uuid::parse_str(&self.client_id)
            .map_err(|_| "客户端 ID 格式无效，请复制应用概述页的应用程序（客户端）ID。")?;
        callback_address(&self.redirect_uri)?;
        MicrosoftAuthConfig::new(&self.client_id, &self.redirect_uri).map_err(|e| e.to_string())
    }
}

fn read_login_settings(path: &Path) -> Result<Option<LoginSettings>, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| format!("登录配置读取失败，请在设置中重新保存：{e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("登录配置读取失败：{e}")),
    }
}

async fn save_login_settings(
    path: PathBuf,
    settings: LoginSettings,
) -> Result<LoginSettings, String> {
    let settings = settings.normalized();
    settings.auth_config()?;
    let bytes = serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?;
    tokio::fs::create_dir_all(path.parent().ok_or("登录配置路径无效")?)
        .await
        .map_err(|e| e.to_string())?;
    let temporary = path.with_extension("json.tmp");
    tokio::fs::write(&temporary, bytes)
        .await
        .map_err(|e| e.to_string())?;
    tokio::fs::rename(&temporary, &path)
        .await
        .map_err(|e| format!("登录配置保存失败：{e}"))?;
    Ok(settings)
}

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
    ClientId(String),
    RedirectUri(String),
    SaveLoginSettings,
    LoginSettingsSaved(Result<LoginSettings, String>),
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
    pub login_settings: LoginSettings,
    pub settings_notice: String,
    pub settings_saving: bool,
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
        Self::from_directory(data_dir())
    }
}

impl State {
    fn from_directory(directory: PathBuf) -> Self {
        let (login_settings, config) =
            match read_login_settings(&directory.join("microsoft-login.json")) {
                Ok(saved) => {
                    let settings = saved.unwrap_or_else(LoginSettings::fallback).normalized();
                    let config = settings.auth_config();
                    (settings, config)
                }
                Err(error) => (LoginSettings::fallback(), Err(error)),
            };
        let settings_notice = config
            .as_ref()
            .err()
            .cloned()
            .unwrap_or_else(|| "登录配置已加载。保存后立即生效，下次启动自动读取。".into());
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
            login_settings,
            settings_notice,
            settings_saving: false,
            config,
            path: directory.join("accounts.json"),
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
            Message::ClientId(value) if !self.busy => {
                self.login_settings.client_id = value;
                self.settings_notice = "修改尚未保存，请点击保存登录配置。".into();
            }
            Message::RedirectUri(value) if !self.busy => {
                self.login_settings.redirect_uri = value;
                self.settings_notice = "修改尚未保存，请点击保存登录配置。".into();
            }
            Message::SaveLoginSettings if !self.busy => {
                let settings = self.login_settings.normalized();
                if let Err(error) = settings.auth_config() {
                    self.settings_notice = error;
                    return Task::none();
                }
                self.busy = true;
                self.settings_saving = true;
                self.settings_notice = "正在保存登录配置…".into();
                return Task::perform(
                    save_login_settings(self.path.with_file_name("microsoft-login.json"), settings),
                    Message::LoginSettingsSaved,
                );
            }
            Message::LoginSettingsSaved(result) => {
                self.busy = false;
                self.settings_saving = false;
                match result {
                    Ok(settings) => match settings.auth_config() {
                        Ok(config) => {
                            let changed = self
                                .config
                                .as_ref()
                                .is_ok_and(|old| old.client_id != config.client_id);
                            self.provider = Ok(Provider::new(
                                config.clone(),
                                Arc::new(SystemCredentialStore),
                            ));
                            self.config = Ok(config);
                            self.login_settings = settings;
                            self.settings_notice = if changed && !self.index.accounts.is_empty() {
                                "登录配置已保存并生效。客户端 ID 已更改，请重新登录已有账户。"
                            } else {
                                "登录配置已保存并生效，下次启动自动读取。现在可以添加 Microsoft 账户。"
                            }.into();
                            self.notice = self.settings_notice.clone();
                        }
                        Err(error) => self.settings_notice = error,
                    },
                    Err(error) => self.settings_notice = error,
                }
            }
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

    #[tokio::test]
    async fn saved_login_settings_survive_restart_and_immediately_configure_the_provider() {
        let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/test-fixtures")
            .join(format!("login-settings-{}", std::process::id()));
        let path = directory.join("microsoft-login.json");
        let first = LoginSettings {
            client_id: "12345678-1234-1234-1234-123456789abc".into(),
            redirect_uri: DEFAULT_REDIRECT_URI.into(),
        };
        save_login_settings(path.clone(), first.clone())
            .await
            .unwrap();
        let mut state = State::from_directory(directory.clone());
        let _ = state.update(Message::Loaded(Ok(AccountIndex::default())));
        assert_eq!(state.login_settings, first);
        assert!(state.has_microsoft_app());
        let changed = LoginSettings {
            client_id: "abcdefab-1234-1234-1234-123456789abc".into(),
            redirect_uri: "http://localhost:53683/oauth/callback".into(),
        };
        let saved = save_login_settings(path.clone(), changed.clone()).await;
        let _ = state.update(Message::LoginSettingsSaved(saved));
        let login = state.provider.as_ref().unwrap().begin_login();
        let url = url::Url::parse(&login.authorization_url).unwrap();
        assert!(url
            .query_pairs()
            .any(|(k, v)| k == "client_id" && v == changed.client_id));
        assert!(url
            .query_pairs()
            .any(|(k, v)| k == "redirect_uri" && v == changed.redirect_uri));
        assert_eq!(
            State::from_directory(directory.clone()).login_settings,
            changed
        );

        // Invalid edits and disk failures preserve the last usable provider and file.
        for invalid in [
            LoginSettings {
                client_id: "not-an-app-id".into(),
                ..first.clone()
            },
            LoginSettings {
                redirect_uri: "http://example.com:53682/oauth/callback".into(),
                ..first.clone()
            },
        ] {
            assert!(save_login_settings(path.clone(), invalid).await.is_err());
        }
        let _ = state.update(Message::LoginSettingsSaved(Err("write failed".into())));
        assert!(state.has_microsoft_app());
        assert_eq!(state.config.as_ref().unwrap().client_id, changed.client_id);
        assert_eq!(read_login_settings(&path).unwrap().unwrap(), changed);
        assert!(state.settings_notice.contains("write failed"));
        assert!(!state.busy);
        // Corrupt settings must be reported rather than silently overridden by defaults.
        tokio::fs::write(&path, "invalid json").await.unwrap();
        let broken = State::from_directory(directory);
        assert!(!broken.has_microsoft_app());
        assert!(broken.settings_notice.contains("读取失败"));
        tokio::fs::remove_file(path).await.unwrap();
    }

    #[test]
    fn login_settings_cannot_change_during_login_or_save() {
        let mut state = State::default();
        let original = state.login_settings.clone();
        state.busy = true;
        let _ = state.update(Message::ClientId("changed".into()));
        let _ = state.update(Message::RedirectUri(
            "http://localhost:5000/callback".into(),
        ));
        assert_eq!(state.login_settings, original);
        assert_eq!(state.update(Message::SaveLoginSettings).units(), 0);
    }

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
