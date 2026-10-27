//! Read-only release checks. Never downloads or runs an updater.
use serde::Deserialize;
use std::time::Duration;

const ENDPOINT: &str =
    "https://api.github.com/repos/zhixuan45/MemoriaLauncher/releases?per_page=100";
const RELEASE_PREFIX: &str = "https://github.com/zhixuan45/MemoriaLauncher/releases/tag/";
const MAX_BYTES: usize = 1024 * 1024;

fn release_url(value: &str) -> Result<url::Url, String> {
    let url = url::Url::parse(value).map_err(|_| "更新链接无效")?;
    if !value.starts_with(RELEASE_PREFIX)
        || url.host_str() != Some("github.com")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !url
            .path()
            .strip_prefix("/zhixuan45/MemoriaLauncher/releases/tag/")
            .is_some_and(|tag| !tag.is_empty())
    {
        return Err("更新链接不属于项目发布页。".into());
    }
    Ok(url)
}

pub fn open_release(value: &str) -> Result<(), String> {
    let url = release_url(value)?;
    // Pass the URL as a process argument; a release tag must never reach a shell.
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        let mut command = std::process::Command::new("rundll32.exe");
        command
            .arg("url.dll,FileProtocolHandler")
            .creation_flags(0x08000000);
        command
    };
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let mut command = std::process::Command::new("xdg-open");
    let status = command
        .arg(url.as_str())
        .status()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("浏览器启动失败：{status}"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Available {
    pub version: String,
    pub url: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    draft: bool,
    prerelease: bool,
}

pub fn select(bytes: &[u8], current: &str, prerelease: bool) -> Result<Option<Available>, String> {
    let current = semver::Version::parse(current).map_err(|e| e.to_string())?;
    let releases: Vec<Release> =
        serde_json::from_slice(bytes).map_err(|e| format!("更新响应无效：{e}"))?;
    let mut candidates = Vec::new();
    for release in releases {
        let Ok(version) = semver::Version::parse(
            release
                .tag_name
                .strip_prefix('v')
                .unwrap_or(&release.tag_name),
        ) else {
            continue;
        };
        if release.draft || (!prerelease && (release.prerelease || !version.pre.is_empty())) {
            continue;
        }
        if version.cmp_precedence(&current).is_gt() {
            release_url(&release.html_url)?;
            candidates.push((
                version,
                Available {
                    version: release.tag_name,
                    url: release.html_url,
                },
            ));
        }
    }
    Ok(candidates
        .into_iter()
        .max_by(|a, b| a.0.cmp_precedence(&b.0))
        .map(|(_, release)| release))
}

pub async fn check(prerelease: bool) -> Result<Option<Available>, String> {
    check_at(ENDPOINT, env!("CARGO_PKG_VERSION"), prerelease).await
}

async fn check_at(
    endpoint: &str,
    current: &str,
    prerelease: bool,
) -> Result<Option<Available>, String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("MemoriaLauncher/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(20))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let mut response = client
        .get(endpoint)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("无法连接更新服务：{e}"))?;
    if !response.status().is_success() {
        return Err(format!("更新服务返回 HTTP {}", response.status()));
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_BYTES as u64)
    {
        return Err("更新响应过大。".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if bytes.len() + chunk.len() > MAX_BYTES {
            return Err("更新响应过大。".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    select(&bytes, current, prerelease)
}

#[cfg(test)]
#[path = "updates_tests.rs"]
mod tests;
