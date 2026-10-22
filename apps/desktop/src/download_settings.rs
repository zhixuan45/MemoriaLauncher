use crate::{
    app::{Launcher, Message as AppMessage},
    assets::{icon, Icon},
    motion_renderer::Element,
    theme::{self, bold, label, GREEN, MUTED},
};
use iced::{
    widget::{button, column, container, pick_list, row, scrollable, text_input, Space},
    Alignment, Fill, Task,
};
use mc_core::{DownloadClient, DownloadSettings, DownloadSource, MojangCatalogProvider};
use std::path::Path;

#[derive(Debug, Clone)]
pub enum Message {
    Source(DownloadSource),
    CustomUrl(String),
    Parallel(String),
    Save,
    Saved(Result<DownloadSettings, String>),
    Check,
    Checked(Result<String, String>),
}

pub struct State {
    pub saved: DownloadSettings,
    pub source: DownloadSource,
    pub custom_url: String,
    pub parallel: String,
    pub busy: bool,
    pub notice: String,
    pub check_notice: String,
}

fn read_config(path: &Path) -> Result<DownloadSettings, String> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice::<DownloadSettings>(&bytes)
            .map_err(|e| format!("下载配置读取失败：{e}"))?
            .normalized()
            .map_err(|e| e.to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(DownloadSettings::default()),
        Err(e) => Err(format!("下载配置读取失败：{e}")),
    }
}

async fn save_config(path: &Path, config: &DownloadSettings) -> Result<(), String> {
    let config = config.normalized().map_err(|e| e.to_string())?;
    tokio::fs::create_dir_all(path.parent().ok_or("配置路径无效")?)
        .await
        .map_err(|e| e.to_string())?;
    let temporary = path.with_extension("json.tmp");
    tokio::fs::write(
        &temporary,
        serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?,
    )
    .await
    .map_err(|e| e.to_string())?;
    tokio::fs::rename(&temporary, path)
        .await
        .map_err(|e| e.to_string())
}

impl State {
    pub fn load() -> Self {
        match read_config(&crate::accounts::data_dir().join("downloads.json")) {
            Ok(config) => Self::new(config),
            Err(error) => {
                let mut state = Self::new(DownloadSettings::default());
                state.notice = format!("{error}；当前使用默认值，可修改后重新保存。");
                state
            }
        }
    }

    pub fn new(config: DownloadSettings) -> Self {
        Self {
            source: config.source,
            custom_url: config.custom_url.clone(),
            parallel: config.parallel_downloads.to_string(),
            saved: config,
            busy: false,
            notice: "修改后点击保存并应用，重启后仍会保留。".into(),
            check_notice: "可检测当前填写的下载源是否能读取版本清单。".into(),
        }
    }

    pub fn configuration(&self) -> Result<DownloadSettings, String> {
        DownloadSettings {
            source: self.source,
            custom_url: self.custom_url.clone(),
            parallel_downloads: self
                .parallel
                .trim()
                .parse()
                .map_err(|_| "并行下载数量请输入 1–64 的整数。")?,
        }
        .normalized()
        .map_err(|e| e.to_string())
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Saved(result) => {
                self.busy = false;
                match result {
                    Ok(config) => {
                        self.source = config.source;
                        self.custom_url = config.custom_url.clone();
                        self.parallel = config.parallel_downloads.to_string();
                        self.saved = config;
                        self.notice = "下载配置已保存并应用，新的下载任务将使用此配置。".into();
                    }
                    Err(error) => self.notice = format!("保存失败，仍使用原配置：{error}"),
                }
            }
            Message::Checked(result) => {
                self.busy = false;
                self.check_notice = result.unwrap_or_else(|e| format!("连接失败：{e}"));
            }
            _ if self.busy => return Task::none(),
            Message::Source(source) => {
                self.source = source;
                self.edited();
            }
            Message::CustomUrl(value) => {
                self.custom_url = value;
                self.edited();
            }
            Message::Parallel(value) => {
                self.parallel = value;
                self.edited();
            }
            Message::Save | Message::Check => {
                let config = match self.configuration() {
                    Ok(config) => config,
                    Err(error) => {
                        self.notice = error;
                        return Task::none();
                    }
                };
                self.busy = true;
                if matches!(message, Message::Check) {
                    self.check_notice = "正在连接所选下载源…".into();
                    return Task::perform(
                        async move {
                            let started = std::time::Instant::now();
                            let client =
                                DownloadClient::new(config.clone()).map_err(|e| e.to_string())?;
                            let versions = MojangCatalogProvider::list_with_client(&client)
                                .await
                                .map_err(|e| e.to_string())?;
                            Ok(format!(
                                "连接成功 · {} · {} 个版本 · {} ms",
                                config.source,
                                versions.len(),
                                started.elapsed().as_millis()
                            ))
                        },
                        Message::Checked,
                    );
                }
                self.notice = "正在保存下载配置…".into();
                return Task::perform(
                    async move {
                        DownloadClient::new(config.clone()).map_err(|e| e.to_string())?;
                        save_config(&crate::accounts::data_dir().join("downloads.json"), &config)
                            .await?;
                        Ok(config)
                    },
                    Message::Saved,
                );
            }
        }
        Task::none()
    }

    fn edited(&mut self) {
        self.notice = "有未保存的更改，点击保存并应用后生效。".into();
        self.check_notice = "配置已更改，可重新检测连接。".into();
    }
}

fn dispatch(message: Message) -> AppMessage {
    AppMessage::DownloadSettings(message)
}

pub fn view(app: &Launcher) -> Element<'_, AppMessage> {
    let state = &app.download_settings;
    let enabled = !state.busy;
    let sources = [
        DownloadSource::Official,
        DownloadSource::Bmclapi,
        DownloadSource::Custom,
    ];
    let mut source = column![
        row![icon(Icon::Download, GREEN, 22.), bold("下载源", 18.)]
            .spacing(10)
            .align_y(Alignment::Center),
        label("用于 Minecraft 版本目录、客户端、依赖库与资源文件。", 12.).color(MUTED),
        pick_list(sources, Some(state.source), |source| dispatch(
            Message::Source(source)
        ))
        .padding(12)
        .width(Fill)
        .text_size(14),
    ]
    .spacing(14);
    if state.source == DownloadSource::Custom {
        source = source
            .push(label("镜像根地址（兼容 BMCLAPI 目录结构）", 12.))
            .push(
                text_input("https://your-mirror.example", &state.custom_url)
                    .padding(12)
                    .size(13.)
                    .on_input_maybe(enabled.then_some(|value| dispatch(Message::CustomUrl(value)))),
            );
    }
    source = source.push(
        label(
            match state.source {
                DownloadSource::Official => "直接连接 Mojang 官方服务器。",
                DownloadSource::Bmclapi => {
                    "镜像下载服务由 BMCLAPI（bangbang93）提供：bmclapi2.bangbang93.com。"
                }
                DownloadSource::Custom => {
                    "填写 HTTPS 根地址；本机调试可使用 localhost 的 HTTP 地址。"
                }
            },
            12.,
        )
        .color(MUTED),
    );
    source = source.push(
        row![
            button(label("检测连接", 12.))
                .padding(11)
                .style(theme::quiet)
                .on_press_maybe(enabled.then_some(dispatch(Message::Check))),
            label(&state.check_notice, 11.).color(MUTED).width(Fill),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
    );
    let mut presets = row![].spacing(8);
    for value in [1, 4, 8, 16, 32] {
        presets = presets.push(
            button(label(format!("{value}"), 12.))
                .padding([9, 16])
                .style(if state.parallel == value.to_string() {
                    theme::selected
                } else {
                    theme::quiet
                })
                .on_press_maybe(enabled.then_some(dispatch(Message::Parallel(value.to_string())))),
        );
    }
    let parallel = column![
        bold("并行下载数量", 18.),
        label("同时传输的文件数量，范围为 1–64，默认 8。", 12.).color(MUTED),
        row![
            text_input("8", &state.parallel)
                .padding(12)
                .width(100)
                .on_input_maybe(enabled.then_some(|value| dispatch(Message::Parallel(value)))),
            label("个文件", 13.).color(MUTED),
            Space::new().width(Fill),
        ]
        .spacing(12)
        .align_y(Alignment::Center),
        presets,
        label(
            "网络不稳定时可降低数量；客户端、依赖库和资源文件共用此上限。",
            11.
        )
        .color(MUTED),
    ]
    .spacing(14);
    column![
        scrollable(column![
            container(source).padding(20).width(Fill).style(theme::panel),
            container(parallel).padding(20).width(Fill).style(theme::panel),
            label(format!("当前已应用：{} · {} 个并行下载", state.saved.source, state.saved.parallel_downloads), 12.).color(MUTED),
            label("保存后用于新任务；进行中的安装继续使用开始时的配置。安装目录跟随“游戏设置”中的当前实例集。", 11.).color(MUTED),
        ].spacing(12)).direction(theme::scroll_direction()).height(Fill),
        row![
            label(&state.notice, 12.).color(MUTED).width(Fill),
            button(label(if state.busy { "正在处理…" } else { "保存并应用" }, 13.))
                .padding([12, 20]).style(theme::action)
                .on_press_maybe(enabled.then_some(dispatch(Message::Save))),
        ].spacing(16).align_y(Alignment::Center),
    ].spacing(12).height(Fill).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/download-settings-tests")
            .join(uuid::Uuid::new_v4().to_string())
            .join("downloads.json")
    }

    #[tokio::test]
    async fn settings_survive_restart_and_invalid_saves_preserve_previous_file() {
        let path = path();
        assert_eq!(read_config(&path).unwrap(), DownloadSettings::default());
        let config = DownloadSettings {
            source: DownloadSource::Custom,
            custom_url: " https://mirror.example/api/ ".into(),
            parallel_downloads: 3,
        };
        save_config(&path, &config).await.unwrap();
        save_config(&path, &config).await.unwrap();
        let state = State::new(read_config(&path).unwrap());
        assert_eq!(state.source, DownloadSource::Custom);
        assert_eq!(state.custom_url, "https://mirror.example/api");
        assert_eq!(state.parallel, "3");
        let previous = std::fs::read(&path).unwrap();
        let mut invalid = config;
        invalid.parallel_downloads = 65;
        assert!(save_config(&path, &invalid).await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), previous);
        std::fs::write(&path, b"{}").unwrap();
        assert_eq!(read_config(&path).unwrap(), DownloadSettings::default());
        std::fs::write(&path, b"corrupted").unwrap();
        assert!(read_config(&path).unwrap_err().contains("读取失败"));
    }

    #[test]
    fn edits_validate_without_changing_active_settings_and_failure_keeps_the_draft() {
        let config = DownloadSettings::default();
        let mut state = State::new(config.clone());
        for value in ["", "0", "65", "-1", "1.5", "many"] {
            let _ = state.update(Message::Parallel(value.into()));
            assert_eq!(state.update(Message::Save).units(), 0);
            assert!(!state.busy);
            assert_eq!(state.saved, config);
        }
        let _ = state.update(Message::Parallel("16".into()));
        let _ = state.update(Message::Source(DownloadSource::Bmclapi));
        assert_eq!(state.configuration().unwrap().parallel_downloads, 16);
        state.busy = true;
        let _ = state.update(Message::Parallel("4".into()));
        assert_eq!(state.parallel, "16");
        let _ = state.update(Message::Saved(Err("disk full".into())));
        assert!(!state.busy);
        assert_eq!(state.parallel, "16");
        assert_eq!(state.saved, config);
        assert!(state.notice.contains("disk full"));
    }
}
