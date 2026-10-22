//! Source routing and a shared request limit for catalogue and game file downloads.
use crate::{LauncherError, LauncherResult};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};
use std::{fmt, path::PathBuf, sync::Arc, time::Duration};
use tokio::{io::AsyncWriteExt, sync::Semaphore, task::JoinSet};

pub const VERSION_MANIFEST: &str =
    "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
pub const MAX_PARALLEL_DOWNLOADS: usize = 64;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadSource {
    #[default]
    Official,
    Bmclapi,
    Custom,
}

impl fmt::Display for DownloadSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Official => "Mojang 官方",
            Self::Bmclapi => "BMCLAPI 镜像",
            Self::Custom => "自定义镜像",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DownloadSettings {
    pub source: DownloadSource,
    pub custom_url: String,
    pub parallel_downloads: usize,
}

impl Default for DownloadSettings {
    fn default() -> Self {
        Self {
            source: DownloadSource::Official,
            custom_url: String::new(),
            parallel_downloads: 8,
        }
    }
}

fn internal(error: impl fmt::Display) -> LauncherError {
    LauncherError::Internal(error.to_string())
}

fn checked_url(value: &str) -> LauncherResult<reqwest::Url> {
    let url = reqwest::Url::parse(value).map_err(internal)?;
    let loopback = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host_str().is_none()
    {
        return Err(internal(
            "下载地址须使用 HTTPS（本机调试可用 HTTP），且不能包含账户密码。",
        ));
    }
    Ok(url)
}

impl DownloadSettings {
    pub fn normalized(&self) -> LauncherResult<Self> {
        if !(1..=MAX_PARALLEL_DOWNLOADS).contains(&self.parallel_downloads) {
            return Err(internal(format!(
                "并行下载数量应为 1–{MAX_PARALLEL_DOWNLOADS} 的整数。"
            )));
        }
        let mut config = self.clone();
        config.custom_url = config.custom_url.trim().trim_end_matches('/').into();
        if config.source == DownloadSource::Custom {
            let url = checked_url(&config.custom_url)?;
            if url.query().is_some() || url.fragment().is_some() {
                return Err(internal("镜像根地址不能包含查询参数或片段。"));
            }
        }
        Ok(config)
    }

    pub fn resolve(&self, original: &str) -> LauncherResult<String> {
        let mut url = reqwest::Url::parse(original).map_err(internal)?;
        let prefix = match url.host_str().unwrap_or_default() {
            "piston-meta.mojang.com"
            | "launchermeta.mojang.com"
            | "piston-data.mojang.com"
            | "launcher.mojang.com" => Some(""),
            "resources.download.minecraft.net" => Some("assets"),
            "libraries.minecraft.net" => Some("maven"),
            _ => None,
        };
        if prefix.is_some() && url.scheme() == "http" {
            url.set_scheme("https")
                .map_err(|_| internal("无效下载地址"))?;
        }
        checked_url(url.as_str())?;
        let config = self.normalized()?;
        // BMCLAPI serves client JARs at this path, but its log XML endpoint returns 404.
        // Keep official log configuration downloads on Mojang (under the same limiter).
        if config.source == DownloadSource::Bmclapi
            && prefix == Some("")
            && url.path().ends_with(".xml")
        {
            return Ok(url.into());
        }
        if config.source == DownloadSource::Official || prefix.is_none() {
            return Ok(url.into());
        }
        let base = match config.source {
            DownloadSource::Bmclapi => "https://bmclapi2.bangbang93.com",
            _ => &config.custom_url,
        };
        let prefix = prefix.unwrap();
        let mut target = checked_url(&format!(
            "{base}/{}{}",
            if prefix.is_empty() {
                String::new()
            } else {
                format!("{prefix}/")
            },
            url.path().trim_start_matches('/')
        ))?;
        target.set_query(url.query());
        Ok(target.into())
    }
}

#[derive(Clone)]
pub struct DownloadClient {
    settings: DownloadSettings,
    client: reqwest::Client,
    permits: Arc<Semaphore>,
}

#[derive(Debug, Clone)]
pub struct DownloadFile {
    pub url: String,
    pub path: String,
    pub sha1: Option<String>,
    pub size: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct DownloadProgress {
    pub completed: usize,
    pub total: usize,
    pub reused: usize,
}

pub type ProgressCallback = Arc<dyn Fn(DownloadProgress) + Send + Sync>;

impl DownloadClient {
    pub fn new(settings: DownloadSettings) -> LauncherResult<Self> {
        crate::initialize_runtime();
        let settings = settings.normalized()?;
        let client = reqwest::Client::builder()
            .user_agent(concat!("MemoriaLauncher/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(300))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 10 {
                    attempt.error("重定向次数过多")
                } else if checked_url(attempt.url().as_str()).is_err() {
                    attempt.error("不支持的重定向地址")
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .map_err(internal)?;
        Ok(Self {
            permits: Arc::new(Semaphore::new(settings.parallel_downloads)),
            settings,
            client,
        })
    }

    pub fn settings(&self) -> &DownloadSettings {
        &self.settings
    }

    async fn request(&self, url: &str) -> LauncherResult<reqwest::Response> {
        let url = self.settings.resolve(url)?;
        for attempt in 0..3 {
            match self
                .client
                .get(&url)
                .send()
                .await
                .and_then(reqwest::Response::error_for_status)
            {
                Ok(response) => return Ok(response),
                Err(error) => {
                    let retry = error.is_connect()
                        || error.is_timeout()
                        || error.status().is_some_and(|status| {
                            status.is_server_error() || status.as_u16() == 429
                        });
                    if !retry || attempt == 2 {
                        return Err(internal(format!(
                            "下载失败（{}）：{error:?}",
                            self.settings.source
                        )));
                    }
                    tokio::time::sleep(Duration::from_millis(200 * (1 << attempt))).await;
                }
            }
        }
        unreachable!()
    }

    pub async fn bytes(&self, url: &str, limit: usize) -> LauncherResult<Vec<u8>> {
        tokio::time::timeout(Duration::from_secs(30), self.read_bytes(url, limit))
            .await
            .map_err(|_| internal("读取下载元数据超时（30 秒）。"))?
    }

    async fn read_bytes(&self, url: &str, limit: usize) -> LauncherResult<Vec<u8>> {
        let _permit = self.permits.acquire().await.map_err(internal)?;
        let mut response = self.request(url).await?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(internal)? {
            if bytes.len().saturating_add(chunk.len()) > limit {
                return Err(internal("下载内容超出大小限制。"));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    pub async fn json<T: serde::de::DeserializeOwned>(&self, url: &str) -> LauncherResult<T> {
        serde_json::from_slice(&self.bytes(url, 32 * 1024 * 1024).await?)
            .map_err(|e| LauncherError::InvalidMetadata(e.to_string()))
    }

    pub async fn download_file(
        &self,
        layout: &crate::GameLayout,
        file: DownloadFile,
    ) -> LauncherResult<bool> {
        let path = layout.download_path(&file.path)?;
        let expected = file.clone();
        let existing = path.clone();
        if tokio::task::spawn_blocking(move || verified_existing(&existing, &expected))
            .await
            .map_err(internal)?
        {
            return Ok(true);
        }
        let _permit = self.permits.acquire().await.map_err(internal)?;
        let mut response = self.request(&file.url).await?;
        let parent = path.parent().ok_or_else(|| internal("无效下载目标"))?;
        tokio::fs::create_dir_all(parent).await?;
        let path = layout.download_path(&file.path)?;
        let temporary =
            path.with_file_name(format!(".launcher-download-{}.tmp", uuid::Uuid::new_v4()));
        let cleanup = Temporary(temporary.clone());
        let result = async {
            let mut output = tokio::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .await?;
            let mut hasher = Sha1::new();
            let mut received = 0u64;
            while let Some(chunk) = response.chunk().await.map_err(internal)? {
                received += chunk.len() as u64;
                if received > file.size.unwrap_or(1024 * 1024 * 1024) {
                    return Err(internal(format!("文件大小超出声明值：{}", file.path)));
                }
                hasher.update(&chunk);
                output.write_all(&chunk).await?;
            }
            if file.size.is_some_and(|size| size != received) {
                return Err(internal(format!("文件大小不匹配：{}", file.path)));
            }
            verify_sha1(&format!("{:x}", hasher.finalize()), file.sha1.as_deref())?;
            output.flush().await?;
            drop(output);
            tokio::fs::rename(&temporary, &path).await?;
            Ok(false)
        }
        .await;
        drop(cleanup);
        result
    }

    pub async fn download_files(
        &self,
        layout: crate::GameLayout,
        files: Vec<DownloadFile>,
        progress: ProgressCallback,
    ) -> LauncherResult<()> {
        let total = files.len();
        let mut remaining = files.into_iter();
        let mut running = JoinSet::new();
        let mut completed = 0;
        let mut reused = 0;
        progress(DownloadProgress {
            completed,
            total,
            reused,
        });
        loop {
            while running.len() < self.settings.parallel_downloads {
                let Some(file) = remaining.next() else {
                    break;
                };
                let client = self.clone();
                let layout = layout.clone();
                running.spawn(async move { client.download_file(&layout, file).await });
            }
            let Some(result) = running.join_next().await else {
                break;
            };
            if result.map_err(internal)?? {
                reused += 1;
            }
            completed += 1;
            progress(DownloadProgress {
                completed,
                total,
                reused,
            });
        }
        Ok(())
    }
}

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

pub(crate) fn verify_sha1(actual: &str, expected: Option<&str>) -> LauncherResult<()> {
    if let Some(expected) = expected {
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(LauncherError::DownloadHashMismatch {
                expected: expected.into(),
                actual: actual.into(),
            });
        }
    }
    Ok(())
}

fn verified_existing(path: &std::path::Path, file: &DownloadFile) -> bool {
    use std::io::Read;
    let Ok(mut input) = std::fs::File::open(path) else {
        return false;
    };
    if file.size.is_some_and(|size| {
        input
            .metadata()
            .map(|meta| meta.len() != size)
            .unwrap_or(true)
    }) {
        return false;
    }
    let Some(expected) = &file.sha1 else {
        return false;
    };
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        match input.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => hasher.update(&buffer[..count]),
            Err(_) => return false,
        }
    }
    format!("{:x}", hasher.finalize()).eq_ignore_ascii_case(expected)
}
