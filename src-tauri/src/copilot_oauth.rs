use crate::models::{CopilotAuthStatus, GitHubDeviceCodeResponse, ManagedAuthAccount};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, io::Write, path::PathBuf, sync::Arc};
use tokio::sync::{Mutex, RwLock};

const GITHUB_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";
const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const OAUTH_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const GITHUB_USER_URL: &str = "https://api.github.com/user";
const COPILOT_TOKEN_URL: &str = "https://api.github.com/copilot_internal/v2/token";
const COPILOT_USAGE_URL: &str = "https://api.github.com/copilot_internal/user";
const USER_AGENT: &str = "ai-usage-copilot-oauth";
const TOKEN_REFRESH_BUFFER_SECONDS: i64 = 60;

#[derive(Debug, thiserror::Error)]
pub enum CopilotOAuthError {
    #[error("设备码流程未启动")]
    DeviceFlowNotStarted,
    #[error("等待用户授权中")]
    AuthorizationPending,
    #[error("用户拒绝授权")]
    AccessDenied,
    #[error("设备码已过期")]
    ExpiredToken,
    #[error("网络错误: {0}")]
    NetworkError(String),
    #[error("解析错误: {0}")]
    ParseError(String),
    #[error("IO 错误: {0}")]
    IoError(String),
    #[error("账号不存在: {0}")]
    AccountNotFound(String),
    #[error("Copilot Token 获取失败: {0}")]
    CopilotTokenFetchFailed(String),
}

impl From<reqwest::Error> for CopilotOAuthError {
    fn from(err: reqwest::Error) -> Self {
        Self::NetworkError(err.to_string())
    }
}

impl From<std::io::Error> for CopilotOAuthError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err.to_string())
    }
}

#[derive(Debug, Deserialize)]
struct DeviceCodeApiResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

#[derive(Debug, Deserialize)]
struct OAuthTokenResponse {
    access_token: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct GitHubUser {
    id: u64,
    login: String,
    avatar_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CopilotAccountData {
    github_token: String,
    user: GitHubUser,
    authenticated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct CopilotAuthStore {
    #[serde(default)]
    version: u32,
    #[serde(default)]
    accounts: HashMap<String, CopilotAccountData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_account_id: Option<String>,
}

#[derive(Debug, Clone)]
struct CopilotToken {
    token: String,
    expires_at: i64,
}

impl CopilotToken {
    fn is_expiring_soon(&self) -> bool {
        let now = chrono::Utc::now().timestamp();
        self.expires_at - now < TOKEN_REFRESH_BUFFER_SECONDS
    }
}

#[derive(Debug, Clone)]
struct PendingDeviceCode {
    user_code: String,
    expires_at: i64,
}

pub struct CopilotOAuthManager {
    accounts: Arc<RwLock<HashMap<String, CopilotAccountData>>>,
    default_account_id: Arc<RwLock<Option<String>>>,
    pending_device_codes: Arc<RwLock<HashMap<String, PendingDeviceCode>>>,
    copilot_tokens: Arc<RwLock<HashMap<String, CopilotToken>>>,
    refresh_locks: Arc<RwLock<HashMap<String, Arc<Mutex<()>>>>>,
    http_client: Client,
    storage_path: PathBuf,
}

impl CopilotOAuthManager {
    pub fn new(data_dir: PathBuf) -> Self {
        let manager = Self {
            accounts: Arc::new(RwLock::new(HashMap::new())),
            default_account_id: Arc::new(RwLock::new(None)),
            pending_device_codes: Arc::new(RwLock::new(HashMap::new())),
            copilot_tokens: Arc::new(RwLock::new(HashMap::new())),
            refresh_locks: Arc::new(RwLock::new(HashMap::new())),
            http_client: Client::new(),
            storage_path: data_dir.join("copilot_oauth_auth.json"),
        };
        let _ = manager.load_from_disk_sync();
        manager
    }

    pub async fn start_device_flow(&self) -> Result<GitHubDeviceCodeResponse, CopilotOAuthError> {
        let response = self
            .http_client
            .post(DEVICE_CODE_URL)
            .header("Accept", "application/json")
            .header("User-Agent", USER_AGENT)
            .form(&[
                ("client_id", GITHUB_CLIENT_ID),
                ("scope", "read:user copilot"),
            ])
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(CopilotOAuthError::NetworkError(format!(
                "设备码请求失败: {}",
                response.status()
            )));
        }

        let payload: DeviceCodeApiResponse = response
            .json()
            .await
            .map_err(|err| CopilotOAuthError::ParseError(err.to_string()))?;
        let expires_at = chrono::Utc::now().timestamp() + payload.expires_in as i64;
        self.pending_device_codes.write().await.insert(
            payload.device_code.clone(),
            PendingDeviceCode {
                user_code: payload.user_code.clone(),
                expires_at,
            },
        );
        Ok(GitHubDeviceCodeResponse {
            device_code: payload.device_code,
            user_code: payload.user_code,
            verification_uri: payload.verification_uri,
            expires_in: payload.expires_in,
            interval: payload.interval.max(1),
        })
    }

    pub async fn poll_for_account(
        &self,
        device_code: &str,
    ) -> Result<Option<ManagedAuthAccount>, CopilotOAuthError> {
        let pending = self
            .pending_device_codes
            .read()
            .await
            .get(device_code)
            .cloned()
            .ok_or(CopilotOAuthError::DeviceFlowNotStarted)?;

        if pending.expires_at <= chrono::Utc::now().timestamp() {
            self.pending_device_codes.write().await.remove(device_code);
            return Err(CopilotOAuthError::ExpiredToken);
        }

        let response = self
            .http_client
            .post(OAUTH_TOKEN_URL)
            .header("Accept", "application/json")
            .header("User-Agent", USER_AGENT)
            .form(&[
                ("client_id", GITHUB_CLIENT_ID),
                ("device_code", device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ])
            .send()
            .await?;

        let payload: OAuthTokenResponse = response
            .json()
            .await
            .map_err(|err| CopilotOAuthError::ParseError(err.to_string()))?;

        match payload.error.as_deref() {
            Some("authorization_pending") | Some("slow_down") => return Ok(None),
            Some("access_denied") => return Err(CopilotOAuthError::AccessDenied),
            Some("expired_token") => return Err(CopilotOAuthError::ExpiredToken),
            Some(other) => return Err(CopilotOAuthError::NetworkError(other.to_string())),
            None => {}
        }

        let github_token = payload
            .access_token
            .ok_or_else(|| CopilotOAuthError::ParseError("响应缺少 access_token".into()))?;

        let user: GitHubUser = self
            .http_client
            .get(GITHUB_USER_URL)
            .header("Accept", "application/json")
            .header("Authorization", format!("token {github_token}"))
            .header("User-Agent", USER_AGENT)
            .send()
            .await?
            .json()
            .await
            .map_err(|err| CopilotOAuthError::ParseError(err.to_string()))?;

        let account_id = user.id.to_string();
        let data = CopilotAccountData {
            github_token,
            user,
            authenticated_at: chrono::Utc::now().timestamp(),
        };
        self.accounts.write().await.insert(account_id.clone(), data);
        if self.default_account_id.read().await.is_none() {
            *self.default_account_id.write().await = Some(account_id.clone());
        }
        self.pending_device_codes.write().await.remove(device_code);
        self.save_to_disk().await?;
        Ok(self.get_account(&account_id).await)
    }

    pub async fn list_accounts(&self) -> Vec<ManagedAuthAccount> {
        let accounts = self.accounts.read().await.clone();
        let default = self.default_account_id().await;
        let mut items: Vec<ManagedAuthAccount> = accounts
            .values()
            .map(|data| ManagedAuthAccount {
                id: data.user.id.to_string(),
                login: data.user.login.clone(),
                avatar_url: data.user.avatar_url.clone(),
                authenticated_at: data.authenticated_at,
                domain: Some("github.com".into()),
            })
            .collect();
        items.sort_by(|a, b| {
            let a_default = default.as_deref() == Some(a.id.as_str());
            let b_default = default.as_deref() == Some(b.id.as_str());
            b_default
                .cmp(&a_default)
                .then_with(|| b.authenticated_at.cmp(&a.authenticated_at))
                .then_with(|| a.login.cmp(&b.login))
        });
        items
    }

    pub async fn get_account(&self, account_id: &str) -> Option<ManagedAuthAccount> {
        let accounts = self.accounts.read().await;
        accounts.get(account_id).map(|data| ManagedAuthAccount {
            id: data.user.id.to_string(),
            login: data.user.login.clone(),
            avatar_url: data.user.avatar_url.clone(),
            authenticated_at: data.authenticated_at,
            domain: Some("github.com".into()),
        })
    }

    pub async fn get_github_token_for_account(
        &self,
        account_id: &str,
    ) -> Result<String, CopilotOAuthError> {
        self.accounts
            .read()
            .await
            .get(account_id)
            .map(|data| data.github_token.clone())
            .ok_or_else(|| CopilotOAuthError::AccountNotFound(account_id.to_string()))
    }

    pub async fn set_default_account(&self, account_id: &str) -> Result<(), CopilotOAuthError> {
        if !self.accounts.read().await.contains_key(account_id) {
            return Err(CopilotOAuthError::AccountNotFound(account_id.to_string()));
        }
        *self.default_account_id.write().await = Some(account_id.to_string());
        self.save_to_disk().await
    }

    pub async fn remove_account(&self, account_id: &str) -> Result<(), CopilotOAuthError> {
        if self.accounts.write().await.remove(account_id).is_none() {
            return Err(CopilotOAuthError::AccountNotFound(account_id.to_string()));
        }
        self.copilot_tokens.write().await.remove(account_id);
        self.refresh_locks.write().await.remove(account_id);
        if self.default_account_id.read().await.as_deref() == Some(account_id) {
            let next_default = self.accounts.read().await.keys().next().cloned();
            *self.default_account_id.write().await = next_default;
        }
        self.save_to_disk().await
    }

    pub async fn get_valid_token(&self) -> Result<String, CopilotOAuthError> {
        let default_id = self.default_account_id().await.ok_or_else(|| {
            CopilotOAuthError::AccountNotFound("无可用的 Copilot OAuth 账号".into())
        })?;
        self.get_valid_token_for_account(&default_id).await
    }

    pub async fn get_valid_token_for_account(
        &self,
        account_id: &str,
    ) -> Result<String, CopilotOAuthError> {
        if let Some(token) = self.copilot_tokens.read().await.get(account_id).cloned() {
            if !token.is_expiring_soon() {
                return Ok(token.token);
            }
        }

        let lock = self.get_refresh_lock(account_id).await;
        let _guard = lock.lock().await;

        if let Some(token) = self.copilot_tokens.read().await.get(account_id).cloned() {
            if !token.is_expiring_soon() {
                return Ok(token.token);
            }
        }

        let github_token = self
            .accounts
            .read()
            .await
            .get(account_id)
            .map(|data| data.github_token.clone())
            .ok_or_else(|| CopilotOAuthError::AccountNotFound(account_id.to_string()))?;

        let response = self
            .http_client
            .get(COPILOT_TOKEN_URL)
            .header("Accept", "application/json")
            .header("Authorization", format!("token {github_token}"))
            .header("User-Agent", USER_AGENT)
            .send()
            .await?;

        if !response.status().is_success() {
            return Err(CopilotOAuthError::CopilotTokenFetchFailed(format!(
                "{}",
                response.status()
            )));
        }

        let payload: serde_json::Value = response
            .json()
            .await
            .map_err(|err| CopilotOAuthError::ParseError(err.to_string()))?;
        let token = payload
            .get("token")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .ok_or_else(|| CopilotOAuthError::ParseError("Copilot token 响应缺少 token".into()))?
            .to_string();
        let expires_at = payload
            .get("expires_at")
            .and_then(|value| value.as_i64())
            .unwrap_or_else(|| chrono::Utc::now().timestamp() + 3600);
        self.copilot_tokens.write().await.insert(
            account_id.to_string(),
            CopilotToken {
                token: token.clone(),
                expires_at,
            },
        );
        Ok(token)
    }

    pub async fn default_account_id(&self) -> Option<String> {
        let stored = self.default_account_id.read().await.clone();
        if stored.is_some() {
            return stored;
        }
        self.accounts.read().await.keys().next().cloned()
    }

    pub async fn get_status(&self) -> CopilotAuthStatus {
        let accounts = self.list_accounts().await;
        let default_account_id = self.default_account_id().await;
        CopilotAuthStatus {
            authenticated: !accounts.is_empty(),
            accounts,
            default_account_id,
        }
    }

    async fn get_refresh_lock(&self, account_id: &str) -> Arc<Mutex<()>> {
        if let Some(lock) = self.refresh_locks.read().await.get(account_id) {
            return Arc::clone(lock);
        }
        let mut locks = self.refresh_locks.write().await;
        Arc::clone(
            locks
                .entry(account_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(()))),
        )
    }

    fn load_from_disk_sync(&self) -> Result<(), CopilotOAuthError> {
        if !self.storage_path.exists() {
            return Ok(());
        }
        let content = fs::read_to_string(&self.storage_path)?;
        let store: CopilotAuthStore = serde_json::from_str(&content)
            .map_err(|err| CopilotOAuthError::ParseError(err.to_string()))?;
        if let Ok(mut accounts) = self.accounts.try_write() {
            *accounts = store.accounts;
        }
        if let Ok(mut default_account_id) = self.default_account_id.try_write() {
            *default_account_id = store.default_account_id;
        }
        Ok(())
    }

    async fn save_to_disk(&self) -> Result<(), CopilotOAuthError> {
        let store = CopilotAuthStore {
            version: 1,
            accounts: self.accounts.read().await.clone(),
            default_account_id: self.default_account_id().await,
        };
        let content = serde_json::to_string_pretty(&store)
            .map_err(|err| CopilotOAuthError::ParseError(err.to_string()))?;
        self.write_store_atomic(&content)
    }

    fn write_store_atomic(&self, content: &str) -> Result<(), CopilotOAuthError> {
        if let Some(parent) = self.storage_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let parent = self
            .storage_path
            .parent()
            .ok_or_else(|| CopilotOAuthError::IoError("无效的存储路径".into()))?;
        let file_name = self
            .storage_path
            .file_name()
            .ok_or_else(|| CopilotOAuthError::IoError("无效的存储文件名".into()))?
            .to_string_lossy()
            .to_string();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let tmp_path = parent.join(format!("{file_name}.tmp.{ts}"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .mode(0o600)
                .open(&tmp_path)?;
            file.write_all(content.as_bytes())?;
            file.flush()?;
            fs::rename(&tmp_path, &self.storage_path)?;
            fs::set_permissions(&self.storage_path, fs::Permissions::from_mode(0o600))?;
        }

        #[cfg(windows)]
        {
            let mut file = fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&tmp_path)?;
            file.write_all(content.as_bytes())?;
            file.flush()?;
            if self.storage_path.exists() {
                let _ = fs::remove_file(&self.storage_path);
            }
            fs::rename(&tmp_path, &self.storage_path)?;
        }

        Ok(())
    }
}

pub async fn fetch_usage_for_token(token: &str) -> Result<serde_json::Value, CopilotOAuthError> {
    let response = Client::new()
        .get(COPILOT_USAGE_URL)
        .header("Accept", "application/json")
        .header("Authorization", format!("Bearer {token}"))
        .header("Editor-Version", "vscode/1.110.1")
        .header("Editor-Plugin-Version", "copilot-chat/0.38.2")
        .header("User-Agent", "GitHubCopilotChat/0.38.2")
        .send()
        .await?;

    if !response.status().is_success() {
        return Err(CopilotOAuthError::NetworkError(format!(
            "Copilot usage 请求失败: {}",
            response.status()
        )));
    }

    response
        .json()
        .await
        .map_err(|err| CopilotOAuthError::ParseError(err.to_string()))
}

pub struct CopilotAuthState(pub Arc<RwLock<CopilotOAuthManager>>);
