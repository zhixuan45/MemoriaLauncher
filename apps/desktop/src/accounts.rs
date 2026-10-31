use crate::accounts_flow;
use crate::credentials::SystemCredentialStore;
use iced::Task;
use mc_core::{AuthAccount, DeviceCodeStart, MicrosoftAuthConfig, MicrosoftAuthProvider};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::Notify;

pub type Provider = MicrosoftAuthProvider<SystemCredentialStore>;

const DEFAULT_REDIRECT_URI: &str = "http://localhost:53682/oauth/callback";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoginMode {
    Browser,
    DeviceCode,
}

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
        accounts_flow::callback_address(&self.redirect_uri)?;
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
    // 登录交互增强消息
    SwitchMode(LoginMode),
    DeviceCodeStarted(Result<DeviceCodeStart, String>),
    OpenBrowser(String),
    ChooseSkin(String),
    ResetSkin(String),
    RefreshSkin(String),
    SkinPreview(String, u64, crate::account_skin::Skin),
    SkinLoaded(String, u64, Result<crate::account_skin::Skin, String>),
    SkinChosen(String, u64, Result<Option<crate::account_skin::Skin>, String>),
    CopyText(String, &'static str),
    // 离线测试账户消息
    OfflineInput(String),
    ToggleOfflineInput,
    AddOffline,
}

pub struct State {
    pub index: AccountIndex,
    pub busy: bool,
    pub logging_in: bool,
    pub login_mode: LoginMode,
    pub auth_url: Option<String>,
    pub device_code: Option<DeviceCodeStart>,
    pub copied_feedback: Option<String>,
    pub offline_input: String,
    pub showing_offline_input: bool,
    pub notice: String,
    pub provider: Result<Provider, String>,
    pub login_settings: LoginSettings,
    pub settings_notice: String,
    pub settings_saving: bool,
    skins: std::collections::HashMap<String, crate::account_skin::Skin>,
    skin_requests: std::collections::HashMap<String, u64>,
    skin_generation: u64,
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
    pub(crate) fn from_directory(directory: PathBuf) -> Self {
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
            login_mode: LoginMode::Browser,
            auth_url: None,
            device_code: None,
            copied_feedback: None,
            offline_input: "Steve".into(),
            showing_offline_input: false,
            notice: "正在读取已保存的账户…".into(),
            provider,
            login_settings,
            settings_notice,
            settings_saving: false,
            skins: std::collections::HashMap::new(),
            skin_requests: std::collections::HashMap::new(),
            skin_generation: 0,
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
        .map_err(|e| format!("创建账户目录失败（{}）：{e}", path.display()))?;
    let temporary = path.with_extension("json.tmp");
    tokio::fs::write(&temporary, bytes)
        .await
        .map_err(|e| format!("写入账户临时文件失败（{}）：{e}", temporary.display()))?;
    tokio::fs::rename(&temporary, path)
        .await
        .map_err(|e| format!("保存账户记录失败：{e}"))?;
    Ok(index)
}

impl State {
    pub fn avatar(&self, account: &AuthAccount) -> iced::widget::image::Handle {
        self.skins.get(&account.id).map(|skin| skin.avatar.clone()).unwrap_or_else(|| crate::account_skin::fallback(account))
    }

    pub fn skin_loading(&self, id: &str) -> bool {
        self.skin_requests.contains_key(id)
    }

    pub fn custom_skin(&self, id: &str) -> bool {
        self.skins.get(id).is_some_and(|skin| skin.custom)
    }

    pub fn skin_description(&self, account: &AuthAccount) -> String {
        let skin = self.skins.get(&account.id).cloned().unwrap_or_else(|| crate::account_skin::Skin::default_for(account));
        if self.skin_loading(&account.id) {
            format!("{} · 正在读取皮肤…", skin.description)
        } else if let Some(warning) = skin.warning {
            format!("{} · {warning}", skin.description)
        } else { skin.description }
    }

    fn skin_request(&mut self, id: &str) -> u64 {
        self.skin_generation += 1;
        self.skin_requests.insert(id.to_owned(), self.skin_generation);
        self.skin_generation
    }

    fn load_avatar(&mut self, account: AuthAccount) -> Task<Message> {
        let root = self.path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let generation = self.skin_request(&account.id);
        Task::run(iced::stream::channel(2, move |mut output: iced::futures::channel::mpsc::Sender<Message>| async move {
            use iced::futures::SinkExt;
            if !account.is_offline() {
                if let Ok(Some(skin)) = crate::account_skin::cached(&root, &account).await {
                    let _ = output.send(Message::SkinPreview(account.id.clone(), generation, skin)).await;
                }
            }
            let id = account.id.clone();
            let result = crate::account_skin::load(root, account).await;
            let _ = output.send(Message::SkinLoaded(id, generation, result)).await;
        }), |message| message)
    }

    fn load_avatars(&mut self) -> Task<Message> {
        self.skins.retain(|id, _| self.index.accounts.iter().any(|a| &a.id == id));
        self.skin_requests.retain(|id, _| self.index.accounts.iter().any(|a| &a.id == id));
        let accounts: Vec<_> = self.index.accounts.iter()
            .filter(|a| !self.skins.contains_key(&a.id) && !self.skin_loading(&a.id)).cloned().collect();
        Task::batch(accounts.into_iter().map(|account| self.load_avatar(account)).collect::<Vec<_>>())
    }

    pub fn retry_skins(&mut self) -> Task<Message> {
        let accounts: Vec<_> = self.index.accounts.iter().filter(|a| {
            !self.skin_loading(&a.id) && self.skins.get(&a.id).is_none_or(|skin| skin.warning.is_some())
        }).cloned().collect();
        Task::batch(accounts.into_iter().map(|account| self.load_avatar(account)).collect::<Vec<_>>())
    }

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
        if let Some(account) = self.index.current() {
            if account.is_offline() {
                "离线测试账户 · 本地单人/内网"
            } else {
                "Microsoft · 启动时验证会话"
            }
        } else {
            "添加 Microsoft 或离线账户"
        }
    }

    /// 启动所选模式的登录授权流程
    pub fn start_login(&mut self) -> Task<Message> {
        let provider = match &self.provider {
            Ok(provider) => provider.clone(),
            Err(e) => {
                self.notice = e.clone();
                return Task::none();
            }
        };
        let config = match &self.config {
            Ok(config) => config.clone(),
            Err(error) => {
                self.busy = false;
                self.logging_in = false;
                self.notice = error.clone();
                return Task::none();
            }
        };
        self.busy = true;
        self.logging_in = true;
        self.copied_feedback = None;
        self.cancel = Arc::new(Notify::new());
        let cancel = self.cancel.clone();

        match self.login_mode {
            LoginMode::Browser => {
                let login = provider.begin_login();
                let auth_url = login.authorization_url.clone();
                self.auth_url = Some(auth_url.clone());
                self.device_code = None;
                let (redirect, port) = match accounts_flow::callback_address(&config.redirect_uri) {
                    Ok(value) => value,
                    Err(error) => {
                        self.busy = false;
                        self.logging_in = false;
                        self.notice = error;
                        return Task::none();
                    }
                };
                let listener = match std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port))
                    .and_then(|listener| { listener.set_nonblocking(true)?; Ok(listener) }) {
                    Ok(listener) => listener,
                    Err(error) => {
                        self.busy = false;
                        self.logging_in = false;
                        self.auth_url = None;
                        self.notice = format!("无法监听登录回调端口 {port}：{error}");
                        return Task::none();
                    }
                };
                self.notice = match accounts_flow::open_browser(&auth_url) {
                    Ok(()) => "等待浏览器授权完成（最长等待 3 分钟）…".into(),
                    Err(error) => format!("{error}；请复制授权链接手动打开。"),
                };

                let existing_accounts: Vec<String> = self.index.accounts.iter().map(|account| account.id.clone()).collect();
                let expected_state = login.state.clone();
                Task::perform(
                    async move {
                        let listener = tokio::net::TcpListener::from_std(listener)
                            .map_err(|e| format!("无法启用登录回调端口 {port}：{e}"))?;

                        tokio::select! {
                            res = async {
                                accounts_flow::receive_callback(listener, &redirect, &expected_state, &existing_accounts, |callback_url| async move {
                                    provider.complete_login(&login, &callback_url).await.map_err(|e| e.to_string())
                                }).await
                            } => res,
                            _ = cancel.notified() => Err("已取消登录".into()),
                        }
                    },
                    Message::LoggedIn,
                )
            }
            LoginMode::DeviceCode => {
                self.auth_url = None;
                self.device_code = None;
                self.notice = "正在向微软申请设备授权码…".into();
                let provider_clone = provider.clone();
                Task::perform(
                    async move {
                        tokio::select! {
                            res = provider_clone.begin_device_login() => res.map_err(|e| e.to_string()),
                            _ = cancel.notified() => Err("已取消登录".into()),
                        }
                    },
                    Message::DeviceCodeStarted,
                )
            }
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::SwitchMode(mode) => {
                if self.login_mode != mode {
                    self.login_mode = mode;
                    self.copied_feedback = None;
                    if self.logging_in {
                        self.cancel.notify_one();
                        self.cancel = Arc::new(Notify::new());
                        self.auth_url = None;
                        self.device_code = None;
                        return self.start_login();
                    }
                }
            }
            Message::CopyText(content, label) => {
                self.copied_feedback = Some(format!("{label}已复制到剪贴板"));
                return iced::clipboard::write(content);
            }
            Message::OpenBrowser(url) => {
                if let Err(error) = accounts_flow::open_browser(&url) {
                    self.notice = format!("{error}；请复制链接手动打开。");
                }
            }
            Message::ChooseSkin(id) if !self.busy => {
                let Some(account) = self.index.accounts.iter().find(|a| a.id == id && a.is_offline()).cloned() else { return Task::none(); };
                let root = self.path.parent().unwrap_or(Path::new(".")).to_path_buf();
                self.busy = true;
                let generation = self.skin_request(&id);
                return Task::perform(async move {
                    let Some(file) = rfd::AsyncFileDialog::new().add_filter("Minecraft 皮肤 PNG", &["png"]).pick_file().await else { return Ok(None); };
                    tokio::task::spawn_blocking(move || crate::account_skin::save_offline(&root, &account, file.path()).map(Some))
                        .await.map_err(|e| e.to_string())?
                }, move |result| Message::SkinChosen(id, generation, result));
            }
            Message::ResetSkin(id) if !self.busy => {
                let Some(account) = self.index.accounts.iter().find(|a| a.id == id && a.is_offline()).cloned() else { return Task::none(); };
                let root = self.path.parent().unwrap_or(Path::new(".")).to_path_buf();
                self.busy = true;
                let generation = self.skin_request(&id);
                return Task::perform(async move { crate::account_skin::reset_offline(root, account).await.map(Some) },
                    move |result| Message::SkinChosen(id, generation, result));
            }
            Message::RefreshSkin(id) if !self.busy => {
                if let Some(account) = self.index.accounts.iter().find(|a| a.id == id).cloned() {
                    if !self.skin_loading(&id) { return self.load_avatar(account); }
                }
            }
            Message::SkinPreview(id, generation, skin) => {
                if self.skin_requests.get(&id) == Some(&generation) { self.skins.insert(id, skin); }
            }
            Message::SkinLoaded(id, generation, result) => {
                if self.skin_requests.get(&id) != Some(&generation) { return Task::none(); }
                self.skin_requests.remove(&id);
                match result {
                    Ok(skin) => { self.skins.insert(id, skin); },
                    Err(error) => {
                        if let Some(account) = self.index.accounts.iter().find(|a| a.id == id) {
                            let skin = self.skins.entry(id).or_insert_with(|| crate::account_skin::Skin::default_for(account));
                            skin.warning = Some(error);
                        }
                    },
                }
            }
            Message::SkinChosen(id, generation, result) => {
                if self.skin_requests.get(&id) != Some(&generation) { return Task::none(); }
                self.skin_requests.remove(&id);
                self.busy = false;
                match result {
                    Ok(Some(skin)) => {
                        self.notice = if skin.custom { "离线皮肤已保存；启动支持的游戏版本时自动应用。" } else { "已恢复按 UUID 自动选择的默认皮肤。" }.into();
                        self.skins.insert(id, skin);
                    }
                    Ok(None) => {
                        if let Some(account) = self.index.accounts.iter().find(|a| a.id == id).cloned() {
                            return self.load_avatar(account);
                        }
                    }
                    Err(error) => self.notice = format!("设置皮肤失败：{error}"),
                }
            }
            Message::OfflineInput(val) => {
                self.offline_input = val;
            }
            Message::ToggleOfflineInput => {
                self.showing_offline_input = !self.showing_offline_input;
                if self.showing_offline_input && self.offline_input.trim().is_empty() {
                    self.offline_input = "Steve".into();
                }
            }
            Message::AddOffline if !self.busy => {
                let name = self.offline_input.trim();
                if name.is_empty() {
                    self.notice = "离线玩家名称不能为空。".into();
                    return Task::none();
                }
                if name.len() > 16 {
                    self.notice = "离线玩家名称长度不可超过 16 个字符。".into();
                    return Task::none();
                }
                if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    self.notice = "离线玩家名称只能包含英文字母、数字和下划线。".into();
                    return Task::none();
                }
                let account = mc_core::AuthAccount::offline(name);
                let mut index = self.index.clone();
                index.upsert(account);
                self.showing_offline_input = false;
                self.notice = format!("离线测试账户 {name} 添加成功。");
                self.busy = true;
                let path = self.path.clone();
                return Task::perform(async move { write_index(&path, index).await }, Message::Saved);
            }
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
                            }
                            .into();
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
                        return self.load_avatars();
                    }
                    Err(e) => self.notice = e,
                }
            }
            Message::Login if !self.busy && self.ready => {
                if !self.has_microsoft_app() {
                    self.notice = "尚未配置客户端 ID，请先在设置中填写。".into();
                    return Task::none();
                }
                return self.start_login();
            }
            Message::DeviceCodeStarted(result) => {
                if !self.logging_in {
                    return Task::none();
                }
                match result {
                    Ok(start) => {
                        self.device_code = Some(start.clone());
                        self.notice = "请在浏览器打开验证网址并输入设备验证码。".into();
                        // 尝试自动打开验证网址
                        if let Err(error) = accounts_flow::open_browser(&start.verification_uri) {
                            self.notice = format!("{error}；请手动打开验证网址。");
                        }

                        let cancel = self.cancel.clone();
                        let provider = match &self.provider {
                            Ok(p) => p.clone(),
                            Err(e) => {
                                self.busy = false;
                                self.logging_in = false;
                                self.notice = e.clone();
                                return Task::none();
                            }
                        };
                        return Task::perform(
                            accounts_flow::run_device_code_flow(provider, start, cancel),
                            Message::LoggedIn,
                        );
                    }
                    Err(e) => {
                        self.busy = false;
                        self.logging_in = false;
                        self.auth_url = None;
                        self.device_code = None;
                        self.notice = e;
                    }
                }
            }
            Message::CancelLogin => {
                self.cancel.notify_one();
                self.cancel = Arc::new(Notify::new());
                self.logging_in = false;
                self.busy = false;
                self.auth_url = None;
                self.device_code = None;
                self.copied_feedback = None;
                self.notice = "已取消登录。".into();
            }
            Message::LoggedIn(result) => {
                self.logging_in = false;
                self.auth_url = None;
                self.device_code = None;
                self.copied_feedback = None;
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
                        return self.load_avatars();
                    }
                    Err(e) => self.notice = e,
                }
            }
            _ => {}
        }
        Task::none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_skins_can_retry_and_late_loads_cannot_replace_a_chosen_skin() {
        let mut state = State::from_directory(std::env::temp_dir().join(uuid::Uuid::new_v4().to_string()));
        let account = AuthAccount::offline("Player");
        let id = account.id.clone();
        let _ = state.update(Message::Loaded(Ok(AccountIndex { accounts: vec![account.clone()], active: Some(id.clone()) })));
        let first = state.skin_requests[&id];
        let _ = state.update(Message::SkinLoaded(id.clone(), first, Err("read failed".into())));
        assert!(state.skin_description(&account).contains("read failed"));
        assert!(state.retry_skins().units() > 0);
        let previous = state.skin_requests[&id];
        let _ = state.update(Message::ChooseSkin(id.clone()));
        let chosen = state.skin_requests[&id];
        let mut skin = crate::account_skin::Skin::default_for(&account);
        skin.description = "chosen skin".into();
        skin.custom = true;
        let _ = state.update(Message::SkinChosen(id.clone(), chosen, Ok(Some(skin))));
        let _ = state.update(Message::SkinLoaded(id.clone(), previous, Ok(crate::account_skin::Skin::default_for(&account))));
        assert_eq!(state.skin_description(&account), "chosen skin");
        assert!(state.custom_skin(&id));
        assert!(!state.busy);
        let _ = state.update(Message::Loaded(Ok(AccountIndex::default())));
        let _ = state.update(Message::SkinLoaded(id.clone(), previous, Err("late".into())));
        assert!(!state.skins.contains_key(&id));
    }

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
        write_index(&path, index).await.unwrap();
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
    fn starts_without_fake_accounts_and_reports_publisher_configuration() {
        let state = State::default();
        assert!(state.index.current().is_none());
        assert!(state.index.accounts.is_empty());
        assert_eq!(state.has_microsoft_app(), state.provider.is_ok());
    }
    #[test]
    fn mode_switch_and_cancel_reset_state() {
        let mut state = State::default();
        assert_eq!(state.login_mode, LoginMode::Browser);
        
        let _ = state.update(Message::SwitchMode(LoginMode::DeviceCode));
        assert_eq!(state.login_mode, LoginMode::DeviceCode);

        state.auth_url = Some("https://example.com/auth".into());
        state.device_code = Some(DeviceCodeStart {
            user_code: "ABCD".into(),
            device_code: "DC123".into(),
            verification_uri: "https://microsoft.com/link".into(),
            expires_in: 900,
            interval: 5,
            message: None,
        });
        state.copied_feedback = Some("已复制".into());
        state.logging_in = true;

        let _ = state.update(Message::CancelLogin);
        assert!(!state.logging_in);
        assert!(state.auth_url.is_none());
        assert!(state.device_code.is_none());
        assert!(state.copied_feedback.is_none());
    }

    #[tokio::test]
    async fn adding_offline_account_updates_index_and_description() {
        use iced::futures::StreamExt;
        let directory = std::env::temp_dir().join(format!("memoria-account-{}", uuid::Uuid::new_v4()));
        let mut state = State::from_directory(directory.clone());
        let _ = state.update(Message::Loaded(Ok(AccountIndex::default())));
        state.offline_input = "Alex".into();
        let task = state.update(Message::AddOffline);
        let mut stream = iced_test::runtime::task::into_stream(task).unwrap();
        while let Some(action) = stream.next().await {
            if let iced_test::runtime::Action::Output(message) = action {
                let _ = state.update(message);
            }
        }
        assert_eq!(state.name(), "Alex", "{}", state.notice);
        assert!(state.description().contains("离线"));
        assert!(state.index.current().unwrap().is_offline());
        let saved = read_index(directory.join("accounts.json")).await.unwrap();
        assert_eq!(saved.current().unwrap().username, "Alex");
        tokio::fs::remove_dir_all(directory).await.unwrap();
    }
}
