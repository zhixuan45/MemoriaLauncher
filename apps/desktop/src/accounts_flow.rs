//! 微软账号认证流程与系统浏览器集成辅助模块
use crate::credentials::SystemCredentialStore;
use mc_core::{AuthAccount, DeviceCodeStart, DevicePollResult, MicrosoftAuthProvider};
use std::{future::Future, sync::Arc, time::Duration};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Notify,
};

pub type Provider = MicrosoftAuthProvider<SystemCredentialStore>;

/// 调用系统默认浏览器打开指定 URL
pub fn open_browser(url: &str) -> Result<(), String> {
    #[cfg(windows)]
    {
        crate::general::shell_open(std::ffi::OsStr::new(url))
            .map_err(|e| format!("无法打开浏览器：{e}"))
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("无法打开浏览器: {e}"))?;
        Ok(())
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map_err(|e| format!("无法打开浏览器: {e}"))?;
        Ok(())
    }
}

/// 校验并解析本地重定向回调地址与端口
pub fn callback_address(redirect: &str) -> Result<(url::Url, u16), String> {
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

/// 接收来自浏览器的 OAuth 重定向回调
pub async fn receive_callback<F, Fut>(
    listener: TcpListener,
    redirect: &url::Url,
    expected_state: &str,
    existing_accounts: &[String],
    complete: F,
) -> Result<AuthAccount, String>
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = Result<AuthAccount, String>>,
{
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
            })
            .await;
            if !matches!(read, Ok(Ok(()))) || !request.ends_with(b"\r\n\r\n") {
                continue;
            }
            let raw = String::from_utf8_lossy(&request);
            let mut line = raw.lines().next().unwrap_or_default().split_whitespace();
            let method = line.next();
            let target = line.next().unwrap_or_default();
            let Ok(url) = redirect.join(target) else {
                continue;
            };
            let state = url
                .query_pairs()
                .find(|(key, _)| key == "state")
                .map(|(_, value)| value.into_owned());
            if method != Some("GET")
                || !target.starts_with('/')
                || url.origin() != redirect.origin()
                || target.split('?').next() != Some(redirect.path())
                || url.path() != redirect.path()
                || state.as_deref() != Some(expected_state)
                || !url.query_pairs().any(|(key, _)| key == "code" || key == "error")
            {
                write_callback_page(&mut stream, CallbackPage::Invalid).await;
                continue;
            }
            if url.query_pairs().any(|(key, _)| key == "error") {
                write_callback_page(&mut stream, CallbackPage::Denied).await;
                return Err("Microsoft 授权被取消或拒绝，请重新登录。".into());
            }
            let result = complete(url.to_string()).await;
            let page = match &result {
                Ok(account) if existing_accounts.contains(&account.id) => CallbackPage::Existing,
                Ok(_) => CallbackPage::Received,
                Err(_) => CallbackPage::Failed,
            };
            write_callback_page(&mut stream, page).await;
            break result;
        }
    })
    .await
    .map_err(|_| "登录超时，请重新尝试。".to_string())?
}

#[derive(Clone, Copy)]
enum CallbackPage {
    Received,
    Existing,
    Denied,
    Failed,
    Invalid,
}

fn callback_response(page: CallbackPage) -> String {
    let (status, title, description, hint, icon, tone) = match page {
        CallbackPage::Received => ("200 OK", "授权响应已收到", "请返回启动器查看登录状态", "现在可以关闭此页面", "✓", "success"),
        CallbackPage::Existing => ("200 OK", "该账户已添加", "登录信息已更新，请返回启动器查看保存状态", "现在可以关闭此页面", "✓", "success"),
        CallbackPage::Denied => ("200 OK", "授权未完成", "Microsoft 授权已取消或被拒绝", "请返回启动器重新尝试", "!", "error"),
        CallbackPage::Failed => ("200 OK", "登录未完成", "账户验证或凭据保存时遇到问题", "请返回启动器查看详情并重试", "!", "error"),
        CallbackPage::Invalid => ("400 Bad Request", "回调链接无效", "此登录请求无效或已经过期", "请返回启动器重新发起登录", "!", "error"),
    };
    let wallpaper = STANDARD.encode(include_bytes!("../ui/assets/bg_wallpaper.png"));
    let body = include_str!("callback_page.html")
        .replace("{{WALLPAPER}}", &wallpaper)
        .replace("{{TITLE}}", title)
        .replace("{{DESCRIPTION}}", description)
        .replace("{{HINT}}", hint)
        .replace("{{ICON}}", icon)
        .replace("{{TONE}}", tone);
    format!("HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'none'; img-src data:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
}

async fn write_callback_page(stream: &mut tokio::net::TcpStream, page: CallbackPage) {
    let _ = stream.write_all(callback_response(page).as_bytes()).await;
}

/// 执行设备代码授权流的后台轮询
pub async fn run_device_code_flow(
    provider: Provider,
    start: DeviceCodeStart,
    cancel: Arc<Notify>,
) -> Result<AuthAccount, String> {
    let mut interval = start.interval.max(3);
    let expires_in = start.expires_in.max(60);
    let start_time = tokio::time::Instant::now();
    let deadline = start_time + Duration::from_secs(expires_in);

    loop {
        tokio::select! {
            _ = cancel.notified() => {
                return Err("已取消登录".into());
            }
            _ = tokio::time::sleep(Duration::from_secs(interval)) => {}
        }

        if tokio::time::Instant::now() >= deadline {
            return Err("设备代码授权已超时，请重新发起登录。".into());
        }

        let poll_res = tokio::select! {
            _ = cancel.notified() => {
                return Err("已取消登录".into());
            }
            res = provider.poll_device_token(&start.device_code) => res,
        };

        match poll_res {
            Ok(DevicePollResult::Pending) => {}
            Ok(DevicePollResult::SlowDown) => {
                interval += 5;
            }
            Ok(DevicePollResult::Success {
                access_token,
                refresh_token,
            }) => {
                return tokio::select! {
                    _ = cancel.notified() => Err("已取消登录".into()),
                    res = provider.complete_device_login(access_token, refresh_token) => {
                        res.map_err(|e| e.to_string())
                    }
                };
            }
            Err(e) => {
                return Err(e.to_string());
            }
        }
    }
}

#[cfg(test)]
mod callback_tests {
    use super::*;
    use tokio::net::TcpStream;

    async fn request(port: u16, target: &str) -> String {
        let mut stream = TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)).await.unwrap();
        stream.write_all(format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes()).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    }

    #[tokio::test]
    async fn callback_rejects_invalid_state_then_identifies_existing_account() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let redirect = url::Url::parse(&format!("http://localhost:{port}/oauth/callback")).unwrap();
        let task = tokio::spawn(async move {
            receive_callback(listener, &redirect, "expected", &["existing".into()], |_| async {
                Ok(AuthAccount { id: "existing".into(), uuid: "uuid".into(), username: "player".into() })
            }).await
        });
        let invalid = request(port, "/oauth/callback?state=wrong&code=secret").await;
        assert!(invalid.starts_with("HTTP/1.1 400"));
        assert!(invalid.contains("回调链接无效"));
        assert!(!invalid.contains("secret"));
        let existing = request(port, "/oauth/callback?state=expected&code=secret").await;
        assert!(existing.contains("该账户已添加"));
        assert!(existing.contains("查看保存状态"));
        assert!(!existing.contains("secret"));
        assert_eq!(task.await.unwrap().unwrap().id, "existing");
    }

    #[tokio::test]
    async fn callback_reports_denial_and_completion_failure() {
        for (query, expected) in [("error=access_denied", "授权未完成"), ("code=valid", "登录未完成")] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let port = listener.local_addr().unwrap().port();
            let redirect = url::Url::parse(&format!("http://localhost:{port}/oauth/callback")).unwrap();
            let task = tokio::spawn(async move {
                receive_callback(listener, &redirect, "expected", &[], |_| async { Err("private failure".into()) }).await
            });
            let response = request(port, &format!("/oauth/callback?state=expected&{query}")).await;
            assert!(response.contains(expected));
            assert!(!response.contains("private failure"));
            assert!(task.await.unwrap().is_err());
        }
    }

    #[test]
    fn received_page_is_utf8_and_does_not_claim_saved() {
        let response = callback_response(CallbackPage::Received);
        assert!(response.contains("Content-Type: text/html; charset=utf-8"));
        assert!(response.contains("授权响应已收到"));
        assert!(response.contains("请返回启动器查看登录状态"));
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        assert!(headers.contains(&format!("Content-Length: {}", body.len())));
    }
}
