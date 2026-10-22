//! Microsoft account login orchestration for the iced desktop UI.
//! Refresh tokens are kept in memory only until an OS credential vault is wired up.
use iced::Task;
use mc_core::{
    AuthAccount, MemoryCredentialStore, MicrosoftAuthConfig, MicrosoftAuthProvider,
};
use std::{collections::HashMap, process::{Command, Stdio}, sync::Arc, time::Duration};
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::TcpListener, time::timeout};

#[derive(Debug, Clone)]
pub enum Message {
    ClientIdChanged(String),
    RedirectChanged(String),
    BeginLogin,
    LoginFinished(Result<(AuthAccount, String, String), String>),
    Select(String),
    Logout(String),
    LogoutFinished(String, Result<(), String>),
}

pub struct State {
    pub client_id: String,
    pub redirect_uri: String,
    pub accounts: Vec<AuthAccount>,
    pub selected: Option<String>,
    pub busy: bool,
    pub notice: String,
    credentials: Arc<MemoryCredentialStore>,
    configurations: HashMap<String, MicrosoftAuthConfig>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            client_id: std::env::var("MC_LAUNCHER_CLIENT_ID").unwrap_or_default(),
            redirect_uri: std::env::var("MC_LAUNCHER_REDIRECT_URI")
                .unwrap_or_else(|_| "http://localhost:53682/oauth/callback".into()),
            accounts: Vec::new(),
            selected: None,
            busy: false,
            notice: "尚未登录 Microsoft 正版账号。登录状态仅在本次启动器运行期间保留。".into(),
            credentials: Arc::new(MemoryCredentialStore::default()),
            configurations: HashMap::new(),
        }
    }
}

impl State {
    pub fn active(&self) -> Option<&AuthAccount> {
        self.selected.as_ref().and_then(|id| self.accounts.iter().find(|a| &a.id == id))
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ClientIdChanged(value) if !self.busy => self.client_id = value,
            Message::RedirectChanged(value) if !self.busy => self.redirect_uri = value,
            Message::BeginLogin if !self.busy => {
                let config = match MicrosoftAuthConfig::new(
                    self.client_id.trim(), self.redirect_uri.trim(),
                ) {
                    Ok(config) => config,
                    Err(e) => {
                        self.notice = e.to_string();
                        return Task::none();
                    }
                };
                let callback = match Callback::parse(&config.redirect_uri) {
                    Ok(callback) => callback,
                    Err(e) => {
                        self.notice = e;
                        return Task::none();
                    }
                };
                self.busy = true;
                self.notice = "正在等待浏览器授权；请使用拥有 Minecraft Java 版的 Microsoft 账户登录。".into();
                let provider = MicrosoftAuthProvider::new(config.clone(), self.credentials.clone());
                let login = provider.begin_login();
                return Task::perform(
                    async move {
                        let result = browser_login(&provider, &login, &callback).await
                            .map(|account| (account, config.client_id, config.redirect_uri));
                        Message::LoginFinished(result)
                    },
                    |message| message,
                );
            }
            Message::LoginFinished(result) => {
                self.busy = false;
                match result {
                    Ok((account, client_id, redirect_uri)) => {
                        self.configurations.insert(
                            account.id.clone(),
                            MicrosoftAuthConfig::new(client_id, redirect_uri)
                                .expect("configuration was validated at login start"),
                        );
                        self.accounts.retain(|a| a.id != account.id);
                        self.selected = Some(account.id.clone());
                        self.notice = format!("已验证 Minecraft Java 版正版档案：{}", account.username);
                        self.accounts.push(account);
                    }
                    Err(message) => self.notice = message,
                }
            }
            Message::Select(id) if !self.busy => {
                if self.accounts.iter().any(|a| a.id == id) {
                    self.selected = Some(id);
                    self.notice = "已切换当前游戏账户。".into();
                }
            }
            Message::Logout(id) if !self.busy => {
                if let Some(config) = self.configurations.get(&id).cloned() {
                    self.busy = true;
                    let provider = MicrosoftAuthProvider::new(config, self.credentials.clone());
                    return Task::perform(
                        async move { provider.logout(&id).await.map_err(|e| e.to_string()) },
                        move |result| Message::LogoutFinished(id.clone(), result),
                    );
                }
            }
            Message::LogoutFinished(id, result) => {
                self.busy = false;
                match result {
                    Ok(()) => {
                        self.configurations.remove(&id);
                        self.accounts.retain(|a| a.id != id);
                        if self.selected.as_deref() == Some(id.as_str()) {
                            self.selected = self.accounts.first().map(|a| a.id.clone());
                        }
                        self.notice = "已退出该账户并清除会话中的 refresh token。".into();
                    }
                    Err(e) => self.notice = format!("退出账号失败：{e}"),
                }
            }
            _ => {}
        }
        Task::none()
    }
}

/// Only a loopback redirect may be intercepted. Never listen on a public interface.
#[derive(Debug, PartialEq, Eq)]
struct Callback {
    port: u16,
    path: String,
    redirect_uri: String,
}

impl Callback {
    fn parse(uri: &str) -> Result<Self, String> {
        let rest = uri.strip_prefix("http://")
            .ok_or("回调地址必须是 http://localhost 或 http://127.0.0.1。")?;
        let (host_port, remainder) = rest.split_once('/')
            .ok_or("回调地址必须包含路径。")?;
        let (host, port) = host_port.rsplit_once(':')
            .ok_or("回调地址必须指定端口。")?;
        if host != "localhost" && host != "127.0.0.1" {
            return Err("仅允许本机 OAuth 回调，禁止绑定公共网络地址。".into());
        }
        let port = port.parse::<u16>().map_err(|_| "OAuth 回调端口无效。")?;
        if port == 0 || remainder.is_empty()
            || remainder.contains(['?', '#', '\\'])
            || remainder.contains("..")
        {
            return Err("OAuth 回调路径或端口无效。".into());
        }
        Ok(Self { port, path: format!("/{remainder}"), redirect_uri: uri.into() })
    }

    fn accepts(&self, target: &str) -> bool {
        target.split_once('?').is_some_and(|(path, query)| {
            path == self.path && query.split('&').any(|part| {
                part.starts_with("code=") || part.starts_with("error=")
            })
        })
    }
}

async fn browser_login(
    provider: &MicrosoftAuthProvider<MemoryCredentialStore>,
    login: &mc_core::LoginStart,
    callback: &Callback,
) -> Result<AuthAccount, String> {
    let listener = TcpListener::bind(("127.0.0.1", callback.port)).await
        .map_err(|e| format!("无法监听 OAuth 本地回调端口 {}：{e}", callback.port))?;
    open_browser(&login.authorization_url)
        .map_err(|e| format!("无法打开浏览器：{e}。请检查系统默认浏览器设置。"))?;
    let redirect = timeout(Duration::from_secs(180), async {
        loop {
            let (mut stream, _) = listener.accept().await.map_err(|e| e.to_string())?;
            let mut buffer = [0u8; 8192];
            let size = stream.read(&mut buffer).await.map_err(|e| e.to_string())?;
            let request = String::from_utf8_lossy(&buffer[..size]);
            let target = request.lines().next()
                .and_then(|line| line.strip_prefix("GET "))
                .and_then(|line| line.split_once(' ').map(|(target, _)| target))
                .unwrap_or("");
            let accepted = callback.accepts(target);
            let response = if accepted {
                "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n<!doctype html><meta charset=\"utf-8\"><title>Memoria Launcher</title><p>授权回调已收到，可返回 Memoria Launcher 查看登录结果。</p>"
            } else {
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            };
            let _ = stream.write_all(response.as_bytes()).await;
            if accepted {
                if target.split_once('?').is_some_and(|(_, query)| query.split('&').any(|part| part.starts_with("error="))) {
                    return Err("Microsoft 授权被取消或拒绝，请重新登录。".into());
                }
                return Ok(format!("{}{}", callback.redirect_uri, &target[callback.path.len()..]));
            }
        }
    }).await.map_err(|_| "Microsoft 登录超时（3 分钟），请重新操作。".to_owned())??;
    provider.complete_login(login, &redirect).await
        .map_err(|e| format!("正版账号验证失败：{e}"))
}

fn open_browser(url: &str) -> Result<(), std::io::Error> {
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut cmd = Command::new("rundll32.exe");
        cmd.arg("url.dll,FileProtocolHandler");
        cmd
    };
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    let mut command = Command::new("xdg-open");

    command.arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_loopback_callbacks_with_an_explicit_port_are_accepted() {
        let callback = Callback::parse("http://localhost:53682/oauth/callback").unwrap();
        assert_eq!(callback.port, 53682);
        assert!(callback.accepts("/oauth/callback?code=abc&state=xyz"));
        assert!(callback.accepts("/oauth/callback?error=access_denied"));
        assert!(!callback.accepts("/oauth/callback/evil?code=abc"));
        assert!(!callback.accepts("/oauth/callback?state=xyz"));
        assert!(Callback::parse("https://example.org/oauth/callback").is_err());
        assert!(Callback::parse("http://0.0.0.0:53682/oauth/callback").is_err());
        assert!(Callback::parse("http://localhost:0/oauth/callback").is_err());
        assert!(Callback::parse("http://localhost:53682/oauth/../admin").is_err());
    }

    #[test]
    fn starts_without_a_fake_premium_account() {
        let state = State::default();
        assert!(state.active().is_none());
        assert!(state.accounts.is_empty());
    }
}
