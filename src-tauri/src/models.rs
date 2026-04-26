use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthMode {
    ApiKey,
    OAuth,
    SessionToken,
    Cookie,
}

impl Default for AuthMode {
    fn default() -> Self {
        Self::ApiKey
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaWindow {
    pub used_percent: f64,
    pub remaining_percent: f64,
    pub reset_at: Option<DateTime<Utc>>,
    pub window_minutes: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaSnapshot {
    pub account_id: String,
    pub account_name: String,
    pub five_hour: Option<QuotaWindow>,
    pub seven_day: Option<QuotaWindow>,
    pub fetched_at: DateTime<Utc>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RefreshStatus {
    Idle,
    Refreshing,
    Ok,
    Error,
}

impl Default for RefreshStatus {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectedAccount {
    #[serde(default = "default_account_id")]
    pub account_id: String,
    pub account_name: String,
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default)]
    pub auth_mode: AuthMode,
    pub chatgpt_account_id: Option<String>,
    #[serde(default)]
    pub secret_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountQuotaStatus {
    pub account_id: String,
    pub account_name: String,
    pub five_hour: Option<QuotaWindow>,
    pub seven_day: Option<QuotaWindow>,
    pub fetched_at: Option<DateTime<Utc>>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default = "default_account_id")]
    pub account_id: String,
    pub account_name: String,
    pub auth_mode: AuthMode,
    pub base_url_override: Option<String>,
    pub chatgpt_account_id: Option<String>,
    #[serde(default)]
    pub accounts: Vec<ConnectedAccount>,
    pub refresh_interval_minutes: u32,
    pub low_quota_threshold_percent: f64,
    pub notify_on_low_quota: bool,
    pub notify_on_reset: bool,
    pub reset_notify_lead_minutes: u32,
    pub secret_configured: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            account_id: default_account_id(),
            account_name: "OpenAI Account".into(),
            auth_mode: AuthMode::ApiKey,
            base_url_override: None,
            chatgpt_account_id: None,
            accounts: Vec::new(),
            refresh_interval_minutes: 15,
            low_quota_threshold_percent: 10.0,
            notify_on_low_quota: true,
            notify_on_reset: false,
            reset_notify_lead_minutes: 15,
            secret_configured: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveSettingsInput {
    #[serde(default = "default_account_id")]
    pub account_id: String,
    pub account_name: String,
    pub auth_mode: AuthMode,
    pub base_url_override: Option<String>,
    pub chatgpt_account_id: Option<String>,
    pub refresh_interval_minutes: u32,
    pub low_quota_threshold_percent: f64,
    pub notify_on_low_quota: bool,
    pub notify_on_reset: bool,
    pub reset_notify_lead_minutes: u32,
    pub auth_secret: Option<String>,
}

pub fn default_account_id() -> String {
    "default".into()
}

fn default_provider() -> String {
    "openai".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredOAuthTokens {
    pub account_id: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub email: Option<String>,
    pub chatgpt_account_id: Option<String>,
    pub scope: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStatus {
    pub snapshot: Option<QuotaSnapshot>,
    #[serde(default)]
    pub accounts: Vec<AccountQuotaStatus>,
    pub refresh_status: RefreshStatus,
    pub last_error: Option<String>,
    pub last_refreshed_at: Option<DateTime<Utc>>,
}

impl Default for AppStatus {
    fn default() -> Self {
        Self {
            snapshot: None,
            accounts: Vec::new(),
            refresh_status: RefreshStatus::Idle,
            last_error: None,
            last_refreshed_at: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionTestResult {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OAuthPhase {
    Idle,
    Running,
    Success,
    Error,
}

impl Default for OAuthPhase {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OAuthStatus {
    pub phase: OAuthPhase,
    pub message: Option<String>,
    pub email: Option<String>,
    pub auth_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProbeCredentials {
    pub auth_mode: AuthMode,
    pub secret: String,
    pub chatgpt_account_id: Option<String>,
}
