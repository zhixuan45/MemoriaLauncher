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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginStart {
    pub authorization_url: String,
    pub state: String,
    pub code_verifier: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AuthAccount {
    pub id: String,
    pub username: String,
    pub uuid: String,
}

#[derive(Clone)]
pub struct MicrosoftAuthConfig {
    pub client_id: String,
    pub redirect_uri: String,
}

impl MicrosoftAuthConfig {
    pub fn new(
        client_id: impl Into<String>,
        redirect_uri: impl Into<String>,
    ) -> LauncherResult<Self> {
        let config = Self {
            client_id: client_id.into(),
            redirect_uri: redirect_uri.into(),
        };
        if config.client_id.trim().is_empty() || config.redirect_uri.trim().is_empty() {
            return Err(LauncherError::AuthUnavailable(
                "Microsoft 认证缺少 client_id 或 redirect_uri".into(),
            ));
        }
        Ok(config)
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
    pub(crate) fn validate(&self) -> LauncherResult<()> {
        if self.access_token.trim().is_empty()
            || self.expires_at <= Utc::now()
            || self.username.trim().is_empty()
            || self.xuid.trim().is_empty()
            || self.client_id.trim().is_empty()
            || uuid::Uuid::parse_str(&self.uuid).is_err()
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
    async fn minecraft_session(&self, _account_id: &str) -> LauncherResult<AuthSession> {
        Err(LauncherError::AuthUnavailable(
            "当前桌面配置未启用 Microsoft/Minecraft 正版认证 Provider；请先完成应用注册并注入认证 Provider".into(),
        ))
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
}
