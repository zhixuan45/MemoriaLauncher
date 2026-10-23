//! 微软账号认证流程与系统浏览器集成辅助模块
use crate::credentials::SystemCredentialStore;
use mc_core::{AuthAccount, DeviceCodeStart, DevicePollResult, MicrosoftAuthProvider};
use std::{sync::Arc, time::Duration};
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
        use std::os::windows::process::CommandExt;
        // 优先使用 cmd /c start "" "<url>" 打开默认浏览器
        let mut cmd = std::process::Command::new("cmd");
        cmd.args(["/c", "start", "", url]);
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        if let Ok(mut child) = cmd.spawn() {
            let _ = child.wait();
            return Ok(());
        }
        // 兜底尝试 rundll32
        let mut fallback = std::process::Command::new("rundll32.exe");
        fallback.args(["url.dll,FileProtocolHandler", url]);
        fallback.creation_flags(0x08000000);
        let _ = fallback.spawn();
        Ok(())
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
pub async fn receive_callback(
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
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                continue;
            }
            let body = "Login response received. You can return to Memoria Launcher.";
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes()).await;
            if url.query_pairs().any(|(key, _)| key == "error") {
                return Err("Microsoft 授权被取消或拒绝，请重新登录。".into());
            }
            break Ok::<_, String>(url.to_string());
        }
    })
    .await
    .map_err(|_| "登录超时，请重新尝试。".to_string())?
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
