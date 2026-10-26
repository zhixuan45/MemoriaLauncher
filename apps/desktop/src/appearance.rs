use crate::{
    app::Message as AppMessage,
    motion_renderer::Element,
    theme::{self, bold, label, MUTED},
};
use iced::{
    widget::{button, column, container, pick_list, row, scrollable, slider, text_input, Space},
    Color, Fill, Task, Theme,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scheme {
    #[default]
    Forest,
    Graphite,
    Midnight,
}
impl std::fmt::Display for Scheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Forest => "森林深灰",
            Self::Graphite => "石墨深灰",
            Self::Midnight => "午夜蓝",
        })
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    pub scheme: Scheme,
    pub scale: u16,
    pub wallpaper: Option<PathBuf>,
    pub dim: u8,
    pub accent_mode: AccentMode,
    pub accent: [u8; 3],
}
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum AccentMode {
    #[default]
    Custom,
    System,
}
impl std::fmt::Display for AccentMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Custom => "自定义主题色",
            Self::System => "跟随系统强调色",
        })
    }
}
impl Default for Config {
    fn default() -> Self {
        Self {
            scheme: Scheme::Forest,
            scale: 100,
            wallpaper: None,
            dim: 75,
            accent_mode: AccentMode::Custom,
            accent: [56, 201, 143],
        }
    }
}
impl Config {
    fn validate(&self) -> Result<(), String> {
        if !(80..=140).contains(&self.scale) || !(30..=95).contains(&self.dim) {
            Err("缩放或背景遮罩超出范围".into())
        } else {
            Ok(())
        }
    }
    pub fn theme(&self) -> Theme {
        let mut palette = theme::launcher().palette();
        match self.scheme {
            Scheme::Forest => {}
            Scheme::Graphite => {
                palette.background = Color::from_rgb(0.075, 0.075, 0.085);
            }
            Scheme::Midnight => {
                palette.background = Color::from_rgb(0.035, 0.065, 0.12);
            }
        }
        palette.primary = Color::from_rgb8(self.accent[0], self.accent[1], self.accent[2]);
        Theme::custom(format!("Memoria {}", self.scheme), palette)
    }
}
#[derive(Debug, Clone)]
pub enum Message {
    Scheme(Scheme),
    Scale(u16),
    Dim(u8),
    Browse,
    Picked(Option<PathBuf>),
    Remove,
    Defaults,
    Revert,
    Save,
    Saved(Result<Config, String>),
    AccentMode(AccentMode),
    AccentHex(String),
    RefreshSystem,
    SystemAccent(Result<[u8; 3], String>),
}
pub struct State {
    pub saved: Config,
    pub draft: Config,
    pub busy: bool,
    pub notice: String,
    pub accent_hex: String,
    pub system_accent: Option<[u8; 3]>,
    pub system_notice: String,
}
impl State {
    pub fn load() -> Self {
        let result = (|| -> Result<Config, String> {
            let path = crate::accounts::data_dir().join("appearance.json");
            if !path.exists() {
                return Ok(Config::default());
            }
            let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
            let value: Config = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            value.validate()?;
            Ok(value)
        })();
        let (saved, notice) = match result {
            Ok(v) => (v, "主题、缩放和背景保存在本机，保存后立即应用。".into()),
            Err(e) => (Config::default(), format!("外观读取失败：{e}")),
        };
        let (system_accent, system_notice) = match system_accent() {
            Ok(v) => (Some(v), String::new()),
            Err(e) => (None, e),
        };
        Self {
            accent_hex: hex(saved.accent),
            system_accent,
            system_notice,
            draft: saved.clone(),
            saved,
            busy: false,
            notice,
        }
    }
    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::RefreshSystem => {
                return Task::perform(
                    async {
                        tokio::task::spawn_blocking(system_accent)
                            .await
                            .map_err(|e| e.to_string())
                            .and_then(|v| v)
                    },
                    Message::SystemAccent,
                )
            }
            Message::SystemAccent(result) => {
                match result {
                    Ok(v) => {
                        self.system_accent = Some(v);
                        self.system_notice.clear();
                    }
                    Err(e) => self.system_notice = e,
                };
            }
            Message::Saved(result) => {
                self.busy = false;
                match result {
                    Ok(config) => {
                        self.saved = config.clone();
                        self.draft = config;
                        self.accent_hex = hex(self.draft.accent);
                        self.notice = "外观已保存并应用。".into();
                    }
                    Err(e) => self.notice = format!("保存失败：{e}"),
                }
            }
            Message::Picked(path) => {
                self.busy = false;
                if let Some(path) = path {
                    match png(&path) {
                        Ok(_) => {
                            self.draft.wallpaper = Some(path);
                            self.notice = "背景已选择，保存后应用。".into();
                        }
                        Err(e) => self.notice = e,
                    }
                }
            }
            _ if self.busy => {}
            Message::Scheme(v) => self.draft.scheme = v,
            Message::Scale(v) => self.draft.scale = v,
            Message::Dim(v) => self.draft.dim = v,
            Message::Remove => self.draft.wallpaper = None,
            Message::Defaults => {
                self.draft = Default::default();
                self.accent_hex = hex(self.draft.accent);
            }
            Message::Revert => {
                self.draft = self.saved.clone();
                self.accent_hex = hex(self.draft.accent);
            }
            Message::AccentMode(mode) => self.draft.accent_mode = mode,
            Message::AccentHex(value) => {
                self.accent_hex = value;
            }
            Message::Browse => {
                self.busy = true;
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .add_filter("PNG 背景", &["png"])
                            .pick_file()
                            .await
                            .map(|v| v.path().to_path_buf())
                    },
                    Message::Picked,
                );
            }
            Message::Save => {
                if self.draft.accent_mode == AccentMode::Custom {
                    match parse_hex(&self.accent_hex) {
                        Ok(v) => self.draft.accent = v,
                        Err(e) => {
                            self.notice = e;
                            return Task::none();
                        }
                    }
                }
                self.busy = true;
                let config = self.draft.clone();
                let root = crate::accounts::data_dir();
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || save(root, config))
                            .await
                            .map_err(|e| e.to_string())
                            .and_then(|v| v)
                    },
                    Message::Saved,
                );
            }
        }
        Task::none()
    }
    pub fn theme(&self) -> Theme {
        let mut config = self.saved.clone();
        if config.accent_mode == AccentMode::System {
            if let Some(color) = self.system_accent {
                config.accent = color;
            }
        }
        config.theme()
    }
}
fn hex(value: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", value[0], value[1], value[2])
}
fn parse_hex(value: &str) -> Result<[u8; 3], String> {
    let value = value.trim().strip_prefix('#').unwrap_or(value.trim());
    if value.len() != 6 || !value.bytes().all(|v| v.is_ascii_hexdigit()) {
        return Err("主题色应为 #RRGGBB，例如 #38C98F".into());
    }
    Ok([
        u8::from_str_radix(&value[0..2], 16).unwrap(),
        u8::from_str_radix(&value[2..4], 16).unwrap(),
        u8::from_str_radix(&value[4..6], 16).unwrap(),
    ])
}
#[cfg(windows)]
fn system_accent() -> Result<[u8; 3], String> {
    let mut color = 0u32;
    let mut opaque = 0;
    // DWM returns AARRGGBB; it does not use Win32 COLORREF's BGR ordering.
    let status = unsafe {
        windows_sys::Win32::Graphics::Dwm::DwmGetColorizationColor(&mut color, &mut opaque)
    };
    if status < 0 {
        return Err(format!(
            "系统强调色读取失败，使用自定义颜色：0x{status:08X}"
        ));
    }
    Ok([
        ((color >> 16) & 255) as u8,
        ((color >> 8) & 255) as u8,
        (color & 255) as u8,
    ])
}
#[cfg(not(windows))]
fn system_accent() -> Result<[u8; 3], String> {
    Err("此平台使用自定义主题色。".into())
}
fn png(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let metadata = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.len() > 16 * 1024 * 1024 {
        return Err("PNG 背景应小于 16 MiB".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(&bytes), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    reader.decode().map_err(|e| format!("PNG 背景无效：{e}"))?;
    Ok(bytes)
}
fn save(root: PathBuf, mut config: Config) -> Result<Config, String> {
    config.validate()?;
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    if let Some(path) = &config.wallpaper {
        let bytes = png(path)?;
        if path.parent() != Some(root.as_path())
            || !path
                .file_name()
                .is_some_and(|v| v.to_string_lossy().starts_with("wallpaper-"))
        {
            let target = root.join(format!("wallpaper-{}.png", uuid::Uuid::new_v4()));
            std::fs::write(&target, bytes).map_err(|e| e.to_string())?;
            config.wallpaper = Some(target);
        }
    }
    let temp = root.join(format!("appearance-{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(
        &temp,
        serde_json::to_vec_pretty(&config).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    std::fs::rename(temp, root.join("appearance.json")).map_err(|e| e.to_string())?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn theme_scale_and_copied_wallpaper_survive_disk_round_trip_and_invalid_saves_preserve_config()
    {
        let root =
            std::env::temp_dir().join(format!("memoria-appearance-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let source = root.join("source.png");
        std::fs::write(&source, include_bytes!("../ui/assets/icon_grass.png")).unwrap();
        let config = Config {
            scheme: Scheme::Midnight,
            scale: 120,
            wallpaper: Some(source.clone()),
            dim: 65,
            accent: [255, 138, 101],
            ..Default::default()
        };
        let saved = save(root.clone(), config).unwrap();
        assert_ne!(saved.wallpaper, Some(source.clone()));
        let bytes = std::fs::read(root.join("appearance.json")).unwrap();
        let loaded: Config = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(saved, loaded);
        std::fs::remove_file(source).unwrap();
        assert!(loaded.wallpaper.as_ref().unwrap().is_file());
        let mut bad = loaded.clone();
        bad.scale = 0;
        assert!(save(root.clone(), bad).is_err());
        assert_eq!(std::fs::read(root.join("appearance.json")).unwrap(), bytes);
        assert_ne!(
            loaded.theme().palette().background,
            theme::launcher().palette().background
        );
        assert_eq!(
            loaded.theme().palette().primary,
            Color::from_rgb8(255, 138, 101)
        );
        save(root.clone(), loaded).unwrap();
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn custom_color_validation_and_system_updates_keep_the_users_choice() {
        let mut state = State::load();
        state.saved.accent_mode = AccentMode::System;
        let _ = state.update(Message::SystemAccent(Ok([10, 80, 200])));
        assert_eq!(
            state.theme().palette().primary,
            Color::from_rgb8(10, 80, 200)
        );
        let _ = state.update(Message::SystemAccent(Ok([220, 90, 30])));
        assert_eq!(
            state.theme().palette().primary,
            Color::from_rgb8(220, 90, 30)
        );
        state.saved.accent_mode = AccentMode::Custom;
        state.saved.accent = [100, 200, 50];
        let _ = state.update(Message::SystemAccent(Ok([0, 0, 0])));
        assert_eq!(
            state.theme().palette().primary,
            Color::from_rgb8(100, 200, 50)
        );
        state.draft.accent_mode = AccentMode::Custom;
        state.accent_hex = "not a color".into();
        assert_eq!(state.update(Message::Save).units(), 0);
        assert!(!state.busy);
        assert_eq!(parse_hex("#11aAcC").unwrap(), [17, 170, 204]);
    }
    #[cfg(windows)]
    #[test]
    fn reads_the_live_windows_accent() {
        let color = system_accent().unwrap();
        println!("Windows accent: {}", hex(color));
    }
}
fn dispatch(message: Message) -> AppMessage {
    AppMessage::Appearance(message)
}
pub fn view(state: &State) -> Element<'_, AppMessage> {
    let enabled = !state.busy;
    let colors = column![
        label("主题", 13.),
        pick_list(
            [Scheme::Forest, Scheme::Graphite, Scheme::Midnight],
            Some(state.draft.scheme),
            |v| dispatch(Message::Scheme(v))
        )
        .text_size(14)
        .width(260),
        label("主题色来源", 13.),
        pick_list(
            [AccentMode::Custom, AccentMode::System],
            Some(state.draft.accent_mode),
            |v| dispatch(Message::AccentMode(v))
        )
        .width(260),
        text_input("#RRGGBB", &state.accent_hex)
            .padding(10)
            .width(220)
            .style(theme::input)
            .on_input_maybe(
                (enabled && state.draft.accent_mode == AccentMode::Custom)
                    .then_some(|v| dispatch(Message::AccentHex(v)))
            ),
        label(
            if state.draft.accent_mode == AccentMode::System {
                state
                    .system_accent
                    .map(|v| format!("Windows 当前强调色：{}", hex(v)))
                    .unwrap_or_else(|| state.system_notice.clone())
            } else {
                "输入十六进制颜色；保存后用于按钮、选中项和强调图标。".into()
            },
            11.
        )
        .color(MUTED),
        row![
            label("界面缩放", 13.),
            pick_list(
                [80u16, 90, 100, 110, 120, 130, 140],
                Some(state.draft.scale),
                |v| dispatch(Message::Scale(v))
            )
            .width(110),
            label(format!("{}%", state.draft.scale), 11.).color(MUTED)
        ]
        .spacing(10)
        .align_y(iced::Alignment::Center),
    ]
    .spacing(8)
    .width(Fill);
    let wallpaper = column![
        label("背景图片（PNG）", 13.),
        label(
            state
                .draft
                .wallpaper
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "使用纯色背景".into()),
            11.
        )
        .color(MUTED),
        row![
            button(label("选择背景图片", 12.))
                .padding(10)
                .style(theme::quiet)
                .on_press_maybe(enabled.then_some(dispatch(Message::Browse))),
            button(label("移除背景", 12.))
                .padding(10)
                .style(theme::quiet)
                .on_press_maybe(enabled.then_some(dispatch(Message::Remove)))
        ]
        .spacing(10),
        label(format!("背景遮罩 {}%", state.draft.dim), 13.),
        slider(30..=95, state.draft.dim, |v| dispatch(Message::Dim(v))),
        label("遮罩越高，图片越暗，文字越容易阅读。", 11.).color(MUTED)
    ]
    .spacing(14)
    .width(Fill);
    let content = column![bold("界面外观", 24.), row![colors, wallpaper].spacing(24)].spacing(12);
    column![
        scrollable(
            container(content)
                .padding(16)
                .width(Fill)
                .style(theme::panel)
        )
        .direction(theme::scroll_direction())
        .height(Fill),
        label(&state.notice, 12.).color(MUTED),
        row![
            button(label("恢复默认外观", 12.))
                .padding(10)
                .style(theme::quiet)
                .on_press_maybe(enabled.then_some(dispatch(Message::Defaults))),
            button(label("撤销更改", 12.))
                .padding(10)
                .style(theme::quiet)
                .on_press_maybe(enabled.then_some(dispatch(Message::Revert))),
            Space::new().width(Fill),
            button(label("保存外观", 13.))
                .padding(12)
                .style(theme::action)
                .on_press_maybe(enabled.then_some(dispatch(Message::Save)))
        ]
        .spacing(8)
    ]
    .spacing(12)
    .height(Fill)
    .into()
}
