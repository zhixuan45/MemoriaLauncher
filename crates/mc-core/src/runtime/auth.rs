use crate::{LauncherError, LauncherResult};
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 由正式认证 Provider 返回；不要将凭据存入实例、计划、CLI 参数或日志。
pub struct AuthSession {
    pub username: String,
    pub uuid: String,
    pub access_token: String,
    pub xuid: String,
    pub client_id: String,
    pub expires_at: DateTime<Utc>,
    pub is_offline: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginStart {
    pub authorization_url: String,
    pub state: String,
    pub code_verifier: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DeviceCodeStart {
    pub user_code: String,
    pub device_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
    pub message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DevicePollResult {
    Pending,
    SlowDown,
    Success {
        access_token: String,
        refresh_token: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AuthAccount {
    pub id: String,
    pub username: String,
    pub uuid: String,
}

impl AuthAccount {
    /// 构造标准离线账户
    pub fn offline(username: impl Into<String>) -> Self {
        let username = username.into();
        let uuid = offline_uuid(&username);
        Self {
            id: format!("offline:{username}"),
            username,
            uuid,
        }
    }

    /// 判断是否为离线测试账户
    pub fn is_offline(&self) -> bool {
        self.id.starts_with("offline:")
    }
}

/// 基于玩家名称生成符合 Minecraft 官方标准的离线 UUIDv3
pub fn offline_uuid(username: &str) -> String {
    uuid::Uuid::new_v3(&uuid::Uuid::nil(), format!("OfflinePlayer:{username}").as_bytes()).to_string()
}

#[derive(Clone)]
pub struct MicrosoftAuthConfig {
    pub client_id: String,
    pub redirect_uri: String,
    device_code_endpoint: Option<String>,
    token_endpoint: Option<String>,
}

impl MicrosoftAuthConfig {
    pub fn new(
        client_id: impl Into<String>,
        redirect_uri: impl Into<String>,
    ) -> LauncherResult<Self> {
        let config = Self {
            client_id: client_id.into(),
            redirect_uri: redirect_uri.into(),
            device_code_endpoint: None,
            token_endpoint: None,
        };
        if config.client_id.trim().is_empty() || config.redirect_uri.trim().is_empty() {
            return Err(LauncherError::AuthUnavailable(
                "Microsoft 认证缺少 client_id 或 redirect_uri".into(),
            ));
        }
        Ok(config)
    }

    /// 离线模式占位配置，仅用于离线会话解析
    pub fn offline_dummy() -> Self {
        Self {
            client_id: "00000000-0000-0000-0000-000000000000".into(),
            redirect_uri: "http://localhost:53682/oauth/callback".into(),
            device_code_endpoint: None,
            token_endpoint: None,
        }
    }

    pub fn from_env() -> LauncherResult<Self> {
        let client_id = std::env::var("MC_LAUNCHER_CLIENT_ID").map_err(|_| {
            LauncherError::AuthUnavailable(
                "未配置 MC_LAUNCHER_CLIENT_ID；请使用项目自己的 Microsoft 应用注册信息".into(),
            )
        })?;
        let redirect_uri = std::env::var("MC_LAUNCHER_REDIRECT_URI")
            .unwrap_or_else(|_| "http://localhost:53682/oauth/callback".into());
        Self::new(client_id, redirect_uri)
    }

    /// 覆盖设备码与令牌端点，主要用于测试
    #[doc(hidden)]
    pub fn with_custom_endpoints(
        mut self,
        device_code_endpoint: impl Into<String>,
        token_endpoint: impl Into<String>,
    ) -> Self {
        self.device_code_endpoint = Some(device_code_endpoint.into());
        self.token_endpoint = Some(token_endpoint.into());
        self
    }

    pub fn device_code_endpoint(&self) -> &str {
        self.device_code_endpoint
            .as_deref()
            .unwrap_or("https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode")
    }

    pub fn token_endpoint(&self) -> &str {
        self.token_endpoint
            .as_deref()
            .unwrap_or("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
    }
}

#[derive(Clone)]
pub struct StoredCredential {
    pub account_id: String,
    pub refresh_token: String,
}

impl std::fmt::Debug for StoredCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredCredential")
            .field("account_id", &self.account_id)
            .field("refresh_token", &"[REDACTED]")
            .finish()
    }
}

#[async_trait]
pub trait CredentialStore: Send + Sync {
    async fn load(&self, account_id: &str) -> LauncherResult<Option<StoredCredential>>;
    async fn save(&self, credential: StoredCredential) -> LauncherResult<()>;
    async fn delete(&self, account_id: &str) -> LauncherResult<()>;
}

/// 测试与临时桌面会话使用；生产环境应替换为 Windows Credential Manager 等系统安全存储。
#[derive(Clone, Default)]
pub struct MemoryCredentialStore {
    entries: Arc<RwLock<HashMap<String, StoredCredential>>>,
}

#[async_trait]
impl CredentialStore for MemoryCredentialStore {
    async fn load(&self, account_id: &str) -> LauncherResult<Option<StoredCredential>> {
        Ok(self.entries.read().await.get(account_id).cloned())
    }

    async fn save(&self, credential: StoredCredential) -> LauncherResult<()> {
        if credential.account_id.trim().is_empty() || credential.refresh_token.trim().is_empty() {
            return Err(LauncherError::AuthUnavailable(
                "不能保存空的账户 ID 或 refresh token".into(),
            ));
        }
        self.entries
            .write()
            .await
            .insert(credential.account_id.clone(), credential);
        Ok(())
    }

    async fn delete(&self, account_id: &str) -> LauncherResult<()> {
        self.entries.write().await.remove(account_id);
        Ok(())
    }
}

impl std::fmt::Debug for AuthSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthSession")
            .field("username", &self.username)
            .field("access_token", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

impl AuthSession {
    /// 构造免认证的本地离线测试会话
    pub fn offline(username: impl Into<String>) -> Self {
        let username = username.into();
        let uuid = offline_uuid(&username);
        Self {
            username,
            uuid,
            access_token: "0".into(),
            xuid: "0".into(),
            client_id: "00000000-0000-0000-0000-000000000000".into(),
            expires_at: Utc::now() + Duration::days(36500),
            is_offline: true,
        }
    }

    pub(crate) fn validate(&self) -> LauncherResult<()> {
        if self.username.trim().is_empty() || uuid::Uuid::parse_str(&self.uuid).is_err() {
            return Err(LauncherError::AuthUnavailable(
                "无效的 Minecraft 账户用户名或 UUID".into(),
            ));
        }
        if self.is_offline {
            return Ok(());
        }
        if self.access_token.trim().is_empty()
            || self.expires_at <= Utc::now()
            || self.xuid.trim().is_empty()
            || self.client_id.trim().is_empty()
        {
            return Err(LauncherError::AuthUnavailable(
                "缺少有效的 Minecraft 正版会话，或会话已过期".into(),
            ));
        }
        Ok(())
    }
}

#[async_trait]
pub trait AuthProvider: Send + Sync {
    /// 实现方须验证游戏拥有权与会话资格，不能仅信任界面中的账号标签。
    async fn minecraft_session(&self, account_id: &str) -> LauncherResult<AuthSession>;
}

pub struct MicrosoftAuthProvider<S> {
    config: MicrosoftAuthConfig,
    store: Arc<S>,
}

impl<S> Clone for MicrosoftAuthProvider<S> {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            store: self.store.clone(),
        }
    }
}

impl<S> MicrosoftAuthProvider<S>
where
    S: CredentialStore + 'static,
{
    pub fn new(config: MicrosoftAuthConfig, store: Arc<S>) -> Self {
        Self { config, store }
    }

    pub fn begin_login(&self) -> LoginStart {
        let (authorization_url, state, code_verifier) =
            mc_launcher_core::auth::microsoft_account::get_secure_login_data(
                &self.config.client_id,
                &self.config.redirect_uri,
                None,
            );
        LoginStart {
            authorization_url,
            state,
            code_verifier,
        }
    }

    pub async fn complete_login(
        &self,
        login: &LoginStart,
        redirect_url: &str,
    ) -> LauncherResult<AuthAccount> {
        // Reject lookalike callbacks before parsing the authorization code or making network calls.
        if !redirect_url.strip_prefix(self.config.redirect_uri.as_str())
            .is_some_and(|suffix| suffix.starts_with('?')) {
            return Err(LauncherError::AuthUnavailable(
                "OAuth 回调地址与当前应用配置不一致".into(),
            ));
        }
        let auth_code = mc_launcher_core::auth::microsoft_account::parse_auth_code_url(
            redirect_url,
            Some(login.state.clone()),
        )
        .map_err(|e| LauncherError::AuthUnavailable(format!("登录回调无效: {e}")))?;
        let config = self.config.clone();
        let verifier = login.code_verifier.clone();
        let result = tokio::task::spawn_blocking(move || {
            exchange_authorization_code(&config, &auth_code, &verifier)
        })
        .await
        .map_err(|e| LauncherError::Internal(e.to_string()))??;
        self.store
            .save(StoredCredential {
                account_id: result.account.id.clone(),
                refresh_token: result.refresh_token,
            })
            .await?;
        Ok(result.account)
    }

    pub async fn logout(&self, account_id: &str) -> LauncherResult<()> {
        self.store.delete(account_id).await
    }

    /// 发起微软设备代码流授权请求
    pub async fn begin_device_login(&self) -> LauncherResult<DeviceCodeStart> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| LauncherError::AuthUnavailable(format!("初始化 HTTP 客户端失败: {e}")))?;
        let params = [
            ("client_id", self.config.client_id.as_str()),
            ("scope", "XboxLive.signin offline_access"),
        ];
        let response = client
            .post(self.config.device_code_endpoint())
            .form(&params)
            .send()
            .await
            .map_err(|e| LauncherError::AuthUnavailable(format!("请求微软设备代码失败: {e}")))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let desc = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| {
                    v.get("error_description")
                        .and_then(|d| d.as_str())
                        .map(|s| s.to_string())
                        .or_else(|| v.get("error").and_then(|e| e.as_str()).map(|s| s.to_string()))
                })
                .unwrap_or(body);
            return Err(LauncherError::AuthUnavailable(format!(
                "获取设备代码失败 ({status}): {desc}"
            )));
        }

        let start: DeviceCodeStart = response
            .json()
            .await
            .map_err(|e| LauncherError::AuthUnavailable(format!("解析设备代码响应失败: {e}")))?;
        Ok(start)
    }

    /// 轮询微软设备授权令牌状态
    pub async fn poll_device_token(&self, device_code: &str) -> LauncherResult<DevicePollResult> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| LauncherError::AuthUnavailable(format!("初始化 HTTP 客户端失败: {e}")))?;
        let params = [
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", self.config.client_id.as_str()),
            ("device_code", device_code),
        ];
        let response = client
            .post(self.config.token_endpoint())
            .form(&params)
            .send()
            .await
            .map_err(|e| LauncherError::AuthUnavailable(format!("轮询设备授权令牌失败: {e}")))?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| LauncherError::AuthUnavailable(format!("读取令牌响应失败: {e}")))?;

        if status.is_success() {
            let value: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| LauncherError::AuthUnavailable(format!("解析令牌响应失败: {e}")))?;
            let access_token = value
                .get("access_token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LauncherError::AuthUnavailable("微软令牌响应缺少 access_token".into()))?
                .to_string();
            let refresh_token = value
                .get("refresh_token")
                .and_then(|v| v.as_str())
                .ok_or_else(|| LauncherError::AuthUnavailable("微软令牌响应缺少 refresh_token".into()))?
                .to_string();
            return Ok(DevicePollResult::Success {
                access_token,
                refresh_token,
            });
        }

        if status == reqwest::StatusCode::BAD_REQUEST {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&body) {
                if let Some(error) = value.get("error").and_then(|v| v.as_str()) {
                    return match error {
                        "authorization_pending" => Ok(DevicePollResult::Pending),
                        "slow_down" => Ok(DevicePollResult::SlowDown),
                        "expired_token" => {
                            Err(LauncherError::AuthUnavailable("设备代码已过期，请重新发起登录".into()))
                        }
                        other => {
                            let desc = value
                                .get("error_description")
                                .and_then(|v| v.as_str())
                                .unwrap_or(other);
                            Err(LauncherError::AuthUnavailable(format!("微软设备认证失败: {desc}")))
                        }
                    };
                }
            }
        }

        Err(LauncherError::AuthUnavailable(format!(
            "轮询设备授权失败 ({status}): {body}"
        )))
    }

    /// 完成设备代码登录后的 Minecraft 凭据置换并持久化
    pub async fn complete_device_login(
        &self,
        access_token: String,
        refresh_token: String,
    ) -> LauncherResult<AuthAccount> {
        let config = self.config.clone();
        let result = tokio::task::spawn_blocking(move || {
            exchange_minecraft(&config, access_token, refresh_token)
        })
        .await
        .map_err(|e| LauncherError::Internal(e.to_string()))??;
        self.store
            .save(StoredCredential {
                account_id: result.account.id.clone(),
                refresh_token: result.refresh_token,
            })
            .await?;
        Ok(result.account)
    }
}

struct ExchangeResult {
    account: AuthAccount,
    refresh_token: String,
    session: AuthSession,
}

fn exchange_authorization_code(
    config: &MicrosoftAuthConfig,
    auth_code: &str,
    code_verifier: &str,
) -> LauncherResult<ExchangeResult> {
    let auth = mc_launcher_core::auth::microsoft_account::get_authorization_token(
        &config.client_id,
        None,
        &config.redirect_uri,
        auth_code,
        Some(code_verifier),
    )
    .map_err(|e| auth_error("Microsoft OAuth", e))?;
    if let Some(error) = auth.error.as_deref() {
        return Err(LauncherError::AuthUnavailable(format!(
            "Microsoft OAuth 拒绝授权: {error}"
        )));
    }
    if auth.access_token.is_empty() {
        return Err(LauncherError::AuthUnavailable(
            "Microsoft OAuth 未返回 access token".into(),
        ));
    }
    exchange_minecraft(config, auth.access_token, auth.refresh_token)
}

fn exchange_refresh_token(
    config: &MicrosoftAuthConfig,
    refresh_token: &str,
) -> LauncherResult<ExchangeResult> {
    let auth = mc_launcher_core::auth::microsoft_account::refresh_authorization_token(
        &config.client_id,
        None,
        refresh_token,
    )
    .map_err(|e| auth_error("Microsoft refresh token", e))?;
    if let Some(error) = auth.error.as_deref() {
        return Err(LauncherError::AuthUnavailable(format!(
            "Microsoft refresh token 已失效: {error}"
        )));
    }
    if auth.access_token.is_empty() {
        return Err(LauncherError::AuthUnavailable(
            "Microsoft refresh 未返回 access token".into(),
        ));
    }
    let next_refresh = if auth.refresh_token.is_empty() {
        refresh_token.to_string()
    } else {
        auth.refresh_token
    };
    exchange_minecraft(config, auth.access_token, next_refresh)
}

fn exchange_minecraft(
    config: &MicrosoftAuthConfig,
    microsoft_access_token: String,
    refresh_token: String,
) -> LauncherResult<ExchangeResult> {
    let xbl =
        mc_launcher_core::auth::microsoft_account::authenticate_with_xbl(&microsoft_access_token)
            .map_err(|e| auth_error("Xbox Live", e))?;
    let userhash = xbl
        .display_claims
        .xui
        .first()
        .map(|x| x.uhs.clone())
        .ok_or_else(|| LauncherError::AuthUnavailable("Xbox Live 未返回用户标识".into()))?;
    let xsts = mc_launcher_core::auth::microsoft_account::authenticate_with_xsts(&xbl.token)
        .map_err(|e| auth_error("Xbox XSTS", e))?;
    let minecraft = mc_launcher_core::auth::microsoft_account::authenticate_with_minecraft(
        &userhash,
        &xsts.token,
    )
    .map_err(|e| auth_error("Minecraft Services", e))?;
    if minecraft.access_token.is_empty() {
        return Err(LauncherError::AuthUnavailable(
            "Minecraft Services 未返回 access token，账户可能没有游戏权限或应用未获批准".into(),
        ));
    }
    let entitlements =
        mc_launcher_core::auth::microsoft_account::get_store_information(&minecraft.access_token)
            .map_err(|e| auth_error("Minecraft entitlement", e))?;
    if entitlements.items.is_empty() {
        return Err(LauncherError::AuthUnavailable(
            "账户未返回 Minecraft 游戏权益，不能作为正版启动账户".into(),
        ));
    }
    let profile = mc_launcher_core::auth::microsoft_account::get_profile(&minecraft.access_token)
        .map_err(|e| auth_error("Minecraft profile", e))?;
    if profile.error.is_some() || profile.id.is_empty() || profile.name.is_empty() {
        return Err(LauncherError::AuthUnavailable(
            profile
                .error_message
                .unwrap_or_else(|| "无法读取 Minecraft profile，账户可能没有 Java 版权限".into()),
        ));
    }
    let account = AuthAccount {
        id: profile.id.clone(),
        username: profile.name,
        uuid: normalize_uuid(&profile.id),
    };
    let session = AuthSession {
        username: account.username.clone(),
        uuid: account.uuid.clone(),
        access_token: minecraft.access_token,
        xuid: userhash,
        client_id: config.client_id.clone(),
        expires_at: Utc::now() + Duration::seconds(i64::from(minecraft.expires_in.max(60))),
        is_offline: false,
    };
    Ok(ExchangeResult {
        account,
        refresh_token,
        session,
    })
}

fn auth_error(stage: &str, error: impl std::fmt::Display) -> LauncherError {
    LauncherError::AuthUnavailable(format!("{stage} 请求失败: {error}"))
}

fn normalize_uuid(value: &str) -> String {
    if value.len() == 32 && value.chars().all(|c| c.is_ascii_hexdigit()) {
        format!(
            "{}-{}-{}-{}-{}",
            &value[0..8],
            &value[8..12],
            &value[12..16],
            &value[16..20],
            &value[20..32]
        )
    } else {
        value.to_string()
    }
}

#[async_trait]
impl<S> AuthProvider for MicrosoftAuthProvider<S>
where
    S: CredentialStore + 'static,
{
    async fn minecraft_session(&self, account_id: &str) -> LauncherResult<AuthSession> {
        if account_id.starts_with("offline:") {
            let username = account_id.strip_prefix("offline:").unwrap_or(account_id);
            return Ok(AuthSession::offline(username));
        }
        let credential =
            self.store.load(account_id).await?.ok_or_else(|| {
                LauncherError::AuthUnavailable("账户尚未完成 Microsoft 登录".into())
            })?;
        let config = self.config.clone();
        let refresh_token = credential.refresh_token;
        let result =
            tokio::task::spawn_blocking(move || exchange_refresh_token(&config, &refresh_token))
                .await
                .map_err(|e| LauncherError::Internal(e.to_string()))??;
        if result.account.id != account_id {
            return Err(LauncherError::AuthUnavailable(
                "刷新返回的 Minecraft 档案与所选账户不一致".into(),
            ));
        }
        self.store
            .save(StoredCredential {
                account_id: result.account.id,
                refresh_token: result.refresh_token,
            })
            .await?;
        Ok(result.session)
    }
}

pub struct UnavailableAuthProvider;

#[async_trait]
impl AuthProvider for UnavailableAuthProvider {
    async fn minecraft_session(&self, account_id: &str) -> LauncherResult<AuthSession> {
        if account_id.starts_with("offline:") {
            let username = account_id.strip_prefix("offline:").unwrap_or(account_id);
            return Ok(AuthSession::offline(username));
        }
        Err(LauncherError::AuthUnavailable(
            "当前桌面配置未启用 Microsoft/Minecraft 正版认证 Provider；请先完成应用注册并注入认证 Provider".into(),
        ))
    }
}

/// 专用于离线模式的认证 Provider
#[derive(Clone, Default)]
pub struct OfflineAuthProvider;

#[async_trait]
impl AuthProvider for OfflineAuthProvider {
    async fn minecraft_session(&self, account_id: &str) -> LauncherResult<AuthSession> {
        let username = account_id.strip_prefix("offline:").unwrap_or(account_id);
        Ok(AuthSession::offline(username))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn config_rejects_missing_values() {
        assert!(MicrosoftAuthConfig::new("", "http://localhost/callback").is_err());
        assert!(MicrosoftAuthConfig::new("client", "").is_err());
        assert!(MicrosoftAuthConfig::new(" client ", " http://localhost/callback ").is_ok());
    }

    #[test]
    fn uuid_normalization_handles_mojang_and_hyphenated_forms() {
        let compact = "0123456789abcdef0123456789abcdef";
        assert_eq!(
            normalize_uuid(compact),
            "01234567-89ab-cdef-0123-456789abcdef"
        );
        let hyphenated = "01234567-89ab-cdef-0123-456789abcdef";
        assert_eq!(normalize_uuid(hyphenated), hyphenated);
    }

    #[tokio::test]
    async fn memory_store_round_trips_and_deletes_credentials() {
        let store = MemoryCredentialStore::default();
        assert!(store
            .save(StoredCredential {
                account_id: "account".into(),
                refresh_token: "refresh".into(),
            })
            .await
            .is_ok());
        let loaded = store.load("account").await.unwrap().unwrap();
        assert_eq!(loaded.account_id, "account");
        assert_eq!(loaded.refresh_token, "refresh");
        assert!(store.delete("account").await.is_ok());
        assert!(store.load("account").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn memory_store_rejects_empty_credentials() {
        let store = MemoryCredentialStore::default();
        assert!(store
            .save(StoredCredential {
                account_id: "".into(),
                refresh_token: "refresh".into(),
            })
            .await
            .is_err());
        assert!(store
            .save(StoredCredential {
                account_id: "account".into(),
                refresh_token: "".into(),
            })
            .await
            .is_err());
    }

    #[tokio::test]
    async fn wrong_redirect_is_rejected_before_token_exchange() {
        let provider = MicrosoftAuthProvider::new(
            MicrosoftAuthConfig::new("client", "http://localhost:53682/oauth/callback").unwrap(),
            Arc::new(MemoryCredentialStore::default()),
        );
        let login = provider.begin_login();
        let forged = format!("http://localhost:53682/oauth/callback-evil?code=abc&state={}", login.state);
        assert!(provider.complete_login(&login, &forged).await.is_err());
    }

    #[test]
    fn login_start_uses_state_and_verifier_from_vendor_helper() {
        let provider = MicrosoftAuthProvider::new(
            MicrosoftAuthConfig::new(
                "00000000-0000-0000-0000-000000000000",
                "http://localhost:53682/oauth/callback",
            )
            .unwrap(),
            Arc::new(MemoryCredentialStore::default()),
        );
        let login = provider.begin_login();
        assert!(!login.authorization_url.is_empty());
        assert!(!login.state.is_empty());
        assert!(!login.code_verifier.is_empty());
        assert!(login.authorization_url.contains("code_challenge"));
        assert!(login.authorization_url.contains("client_id="));
    }

    // 辅助工具：模拟 OAuth 认证服务器
    async fn spawn_mock_server(status: u16, body: &'static str) -> (String, tokio::task::JoinHandle<()>) {
        crate::initialize_runtime();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("http://{}", addr);
        let handle = tokio::spawn(async move {
            if let Ok((mut stream, _)) = listener.accept().await {
                use tokio::io::{AsyncReadExt, AsyncWriteExt};
                let mut buf = [0u8; 4096];
                let _ = stream.read(&mut buf).await;
                let status_line = match status {
                    200 => "HTTP/1.1 200 OK",
                    400 => "HTTP/1.1 400 Bad Request",
                    500 => "HTTP/1.1 500 Internal Server Error",
                    _ => "HTTP/1.1 404 Not Found",
                };
                let response = format!(
                    "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
            }
        });
        (url, handle)
    }

    #[tokio::test]
    async fn begin_device_login_returns_parsed_start_info() {
        let body = r#"{
            "user_code": "ABCDEFGH",
            "device_code": "dev_code_123",
            "verification_uri": "https://microsoft.com/devicelogin",
            "expires_in": 900,
            "interval": 5,
            "message": "Visit https://microsoft.com/devicelogin"
        }"#;
        let (url, _h) = spawn_mock_server(200, body).await;
        let config = MicrosoftAuthConfig::new("test_client", "http://localhost/callback")
            .unwrap()
            .with_custom_endpoints(&url, &url);
        let provider = MicrosoftAuthProvider::new(config, Arc::new(MemoryCredentialStore::default()));
        let start = provider.begin_device_login().await.unwrap();
        assert_eq!(start.user_code, "ABCDEFGH");
        assert_eq!(start.device_code, "dev_code_123");
        assert_eq!(start.verification_uri, "https://microsoft.com/devicelogin");
        assert_eq!(start.expires_in, 900);
        assert_eq!(start.interval, 5);
        assert_eq!(start.message.as_deref(), Some("Visit https://microsoft.com/devicelogin"));
    }

    #[tokio::test]
    async fn begin_device_login_handles_http_error() {
        let body = r#"{"error":"invalid_client","error_description":"Client ID is not valid."}"#;
        let (url, _h) = spawn_mock_server(400, body).await;
        let config = MicrosoftAuthConfig::new("bad_client", "http://localhost/callback")
            .unwrap()
            .with_custom_endpoints(&url, &url);
        let provider = MicrosoftAuthProvider::new(config, Arc::new(MemoryCredentialStore::default()));
        let err = provider.begin_device_login().await.unwrap_err();
        match err {
            LauncherError::AuthUnavailable(msg) => {
                assert!(msg.contains("Client ID is not valid"));
            }
            other => panic!("预期 AuthUnavailable，实际得到: {:?}", other),
        }
    }

    #[tokio::test]
    async fn poll_device_token_pending_and_slow_down() {
        let (url_pending, _h1) = spawn_mock_server(400, r#"{"error":"authorization_pending"}"#).await;
        let config = MicrosoftAuthConfig::new("client", "http://localhost/callback")
            .unwrap()
            .with_custom_endpoints(&url_pending, &url_pending);
        let provider = MicrosoftAuthProvider::new(config, Arc::new(MemoryCredentialStore::default()));
        let res = provider.poll_device_token("dev_code").await.unwrap();
        assert_eq!(res, DevicePollResult::Pending);

        let (url_slow, _h2) = spawn_mock_server(400, r#"{"error":"slow_down"}"#).await;
        let config = MicrosoftAuthConfig::new("client", "http://localhost/callback")
            .unwrap()
            .with_custom_endpoints(&url_slow, &url_slow);
        let provider = MicrosoftAuthProvider::new(config, Arc::new(MemoryCredentialStore::default()));
        let res = provider.poll_device_token("dev_code").await.unwrap();
        assert_eq!(res, DevicePollResult::SlowDown);
    }

    #[tokio::test]
    async fn poll_device_token_expired_and_errors() {
        let (url_expired, _h1) = spawn_mock_server(400, r#"{"error":"expired_token"}"#).await;
        let config = MicrosoftAuthConfig::new("client", "http://localhost/callback")
            .unwrap()
            .with_custom_endpoints(&url_expired, &url_expired);
        let provider = MicrosoftAuthProvider::new(config, Arc::new(MemoryCredentialStore::default()));
        let err = provider.poll_device_token("dev_code").await.unwrap_err();
        match err {
            LauncherError::AuthUnavailable(msg) => {
                assert!(msg.contains("设备代码已过期，请重新发起登录"));
            }
            other => panic!("预期 AuthUnavailable，实际得到: {:?}", other),
        }

        let (url_declined, _h2) = spawn_mock_server(
            400,
            r#"{"error":"access_denied","error_description":"The user declined consent."}"#,
        ).await;
        let config = MicrosoftAuthConfig::new("client", "http://localhost/callback")
            .unwrap()
            .with_custom_endpoints(&url_declined, &url_declined);
        let provider = MicrosoftAuthProvider::new(config, Arc::new(MemoryCredentialStore::default()));
        let err = provider.poll_device_token("dev_code").await.unwrap_err();
        match err {
            LauncherError::AuthUnavailable(msg) => {
                assert!(msg.contains("The user declined consent."));
            }
            other => panic!("预期 AuthUnavailable，实际得到: {:?}", other),
        }
    }

    #[tokio::test]
    async fn poll_device_token_success() {
        let body = r#"{
            "access_token": "mock_access_token",
            "refresh_token": "mock_refresh_token",
            "token_type": "Bearer",
            "expires_in": 3600
        }"#;
        let (url, _h) = spawn_mock_server(200, body).await;
        let config = MicrosoftAuthConfig::new("client", "http://localhost/callback")
            .unwrap()
            .with_custom_endpoints(&url, &url);
        let provider = MicrosoftAuthProvider::new(config, Arc::new(MemoryCredentialStore::default()));
        let res = provider.poll_device_token("dev_code").await.unwrap();
        assert_eq!(
            res,
            DevicePollResult::Success {
                access_token: "mock_access_token".into(),
                refresh_token: "mock_refresh_token".into(),
            }
        );
    }

    #[tokio::test]
    async fn complete_device_login_handles_invalid_tokens() {
        let config = MicrosoftAuthConfig::new("client", "http://localhost/callback").unwrap();
        let provider = MicrosoftAuthProvider::new(config, Arc::new(MemoryCredentialStore::default()));
        let err = provider
            .complete_device_login("invalid_access".into(), "invalid_refresh".into())
            .await
            .unwrap_err();
        match err {
            LauncherError::AuthUnavailable(msg) => {
                assert!(msg.contains("Xbox Live"));
            }
            other => panic!("预期 AuthUnavailable，实际得到: {:?}", other),
        }
    }

    #[tokio::test]
    async fn offline_account_and_session_generation() {
        let account = AuthAccount::offline("Steve");
        assert_eq!(account.username, "Steve");
        assert!(account.is_offline());
        assert_eq!(account.id, "offline:Steve");
        assert_eq!(account.uuid, offline_uuid("Steve"));

        let session = AuthSession::offline("Steve");
        assert!(session.is_offline);
        assert_eq!(session.username, "Steve");
        assert!(session.validate().is_ok());

        let offline_provider = OfflineAuthProvider;
        let s1 = offline_provider.minecraft_session("offline:Steve").await.unwrap();
        assert_eq!(s1.username, "Steve");

        let unavailable = UnavailableAuthProvider;
        let s2 = unavailable.minecraft_session("offline:Alex").await.unwrap();
        assert_eq!(s2.username, "Alex");

        let config = MicrosoftAuthConfig::new("client", "http://localhost/callback").unwrap();
        let ms_provider = MicrosoftAuthProvider::new(config, Arc::new(MemoryCredentialStore::default()));
        let s3 = ms_provider.minecraft_session("offline:Player").await.unwrap();
        assert_eq!(s3.username, "Player");
    }
}
