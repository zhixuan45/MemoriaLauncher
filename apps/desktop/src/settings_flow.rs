//! Asynchronous general-settings operations and draft lifecycle.
use super::*;

impl State {
    pub fn load() -> Self {
        Self::from_directory(crate::accounts::data_dir())
    }

    pub(crate) fn from_directory(directory: std::path::PathBuf) -> Self {
        let mut state = Self::default();
        match general::load(&directory) {
            Ok(config) => {
                state.switches = config.switches().map(Toggle::new);
                state.draft = config.clone();
                state.saved = config;
            }
            Err(error) => state.notice = format!("读取常规设置失败：{error}；当前使用默认值。"),
        }
        state.directory = directory;
        state
    }

    pub fn startup(&mut self) -> Task<AppMessage> {
        if !std::mem::take(&mut self.startup_pending) {
            return Task::none();
        }
        let minimize = if self.saved.start_minimized {
            iced::window::latest().and_then(|id| iced::window::minimize(id, true))
        } else {
            Task::none()
        };
        let check = if self.saved.check_updates {
            self.update(Message::CheckUpdate, Instant::now())
                .map(dispatch)
        } else {
            Task::none()
        };
        Task::batch([minimize, check])
    }

    pub fn game_settings_visible(&self) -> bool {
        matches!(self.tab, 1 | 3) && self.query.trim().is_empty()
    }
    pub fn animating(&self, now: Instant) -> bool {
        self.switches.iter().any(|toggle| toggle.active(now))
    }

    pub fn update(&mut self, message: Message, now: Instant) -> Task<Message> {
        match message {
            Message::Tab(tab) => {
                self.tab = tab.min(TABS.len() - 1);
                self.query.clear();
            }
            Message::Search(query) => self.query = query,
            Message::Saved(result) => {
                self.busy = false;
                match result {
                    Ok(config) => {
                        if self.saved.prerelease != config.prerelease {
                            self.update_generation += 1;
                            self.checking = false;
                            self.available = None;
                            self.update_notice = "更新通道已更改，请重新检查。".into();
                        }
                        self.saved = config;
                        self.notice = "常规设置已保存并应用。".into();
                    }
                    Err(error) => self.notice = format!("保存失败，仍使用原设置：{error}"),
                }
            }
            Message::Imported(result) => {
                self.busy = false;
                match result {
                    Ok(Some(config)) => {
                        self.set_draft(config, now);
                        self.notice = "已读取常规设置，请核对开关和日期格式后保存。".into();
                    }
                    Ok(None) => self.notice = "已取消导入。".into(),
                    Err(error) => self.notice = format!("导入失败：{error}"),
                }
            }
            Message::Completed(result) => {
                self.busy = false;
                self.notice = result.unwrap_or_else(|e| format!("操作失败：{e}"));
            }
            Message::Checked(generation, result) => {
                if generation != self.update_generation {
                    return Task::none();
                }
                self.checking = false;
                match result {
                    Ok(available) => {
                        self.update_notice = available
                            .as_ref()
                            .map(|release| format!("发现新版本 {}", release.version))
                            .unwrap_or_else(|| "最近发布记录中未发现更高版本。".into());
                        self.available = available;
                    }
                    Err(error) => {
                        self.available = None;
                        self.update_notice = format!("检查失败：{error}");
                    }
                }
            }
            _ if self.busy => return Task::none(),
            Message::Toggle(index, value) => {
                if let Some(setting) = self.switches.get_mut(index) {
                    setting.set(value, now);
                    self.draft.set_switch(index, value);
                    self.edited();
                }
            }
            Message::Date(value) => {
                if let Some(index) = DATES.iter().position(|date| *date == value) {
                    self.draft.date_format = index;
                    self.edited();
                }
            }
            Message::Reset => {
                self.set_draft(Config::default(), now);
                self.notice = "已恢复常规设置默认草稿，保存后生效。".into();
            }
            Message::Revert => {
                self.set_draft(self.saved.clone(), now);
                self.notice = "已撤销未保存的常规设置更改。".into();
            }
            Message::Save => {
                let config = self.draft.clone();
                let directory = self.directory.clone();
                self.busy = true;
                self.notice = "正在保存常规设置…".into();
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            general::write(&directory.join("general.json"), &config)?;
                            Ok(config)
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    Message::Saved,
                );
            }
            Message::Import => {
                self.busy = true;
                return Task::perform(
                    async {
                        let Some(file) = rfd::AsyncFileDialog::new()
                            .add_filter("常规设置 JSON", &["json"])
                            .pick_file()
                            .await
                        else {
                            return Ok(None);
                        };
                        tokio::task::spawn_blocking(move || general::read(file.path()).map(Some))
                            .await
                            .map_err(|e| e.to_string())?
                    },
                    Message::Imported,
                );
            }
            Message::Export => {
                let config = self.saved.clone();
                self.busy = true;
                return Task::perform(
                    async move {
                        let Some(file) = rfd::AsyncFileDialog::new()
                            .add_filter("常规设置 JSON", &["json"])
                            .set_file_name("memoria-general-settings.json")
                            .save_file()
                            .await
                        else {
                            return Ok("已取消导出。".into());
                        };
                        tokio::task::spawn_blocking(move || {
                            general::write(file.path(), &config)?;
                            Ok(format!("已导出已保存的常规设置：{}", file.path().display()))
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    Message::Completed,
                );
            }
            Message::OpenData => {
                let directory = self.directory.clone();
                self.busy = true;
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || {
                            general::open_directory(&directory)?;
                            Ok(format!("已打开 {}", directory.display()))
                        })
                        .await
                        .map_err(|e| e.to_string())?
                    },
                    Message::Completed,
                );
            }
            Message::ClearCache => self.confirm_cache = true,
            Message::CancelCache => self.confirm_cache = false,
            Message::ConfirmCache => {
                if !self.confirm_cache {
                    return Task::none();
                }
                self.confirm_cache = false;
                self.busy = true;
                let directory = self.directory.clone();
                return Task::perform(
                    async move {
                        tokio::task::spawn_blocking(move || general::recover_cache(&directory))
                            .await
                            .map_err(|e| e.to_string())?
                    },
                    Message::Completed,
                );
            }
            Message::CheckUpdate => {
                if self.checking {
                    return Task::none();
                }
                self.update_generation += 1;
                let generation = self.update_generation;
                let prerelease = self.saved.prerelease;
                self.checking = true;
                self.available = None;
                self.update_notice = "正在检查项目发布页…".into();
                return Task::perform(crate::updates::check(prerelease), move |result| {
                    Message::Checked(generation, result)
                });
            }
            Message::OpenRelease => {
                if let Some(release) = &self.available {
                    let url = release.url.clone();
                    self.busy = true;
                    return Task::perform(
                        async move {
                            tokio::task::spawn_blocking(move || {
                                crate::updates::open_release(&url)?;
                                Ok("已打开项目发布页，请自行选择版本下载。".into())
                            })
                            .await
                            .map_err(|e| e.to_string())?
                        },
                        Message::Completed,
                    );
                }
            }
        }
        Task::none()
    }

    fn set_draft(&mut self, config: Config, now: Instant) {
        for (toggle, value) in self.switches.iter_mut().zip(config.switches()) {
            toggle.set(value, now);
        }
        self.draft = config;
    }

    fn edited(&mut self) {
        self.notice = "有未保存的常规设置更改。".into();
    }
}
