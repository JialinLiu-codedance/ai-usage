use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

pub const PROVIDER_OPENAI: &str = "openai";
pub const PROVIDER_ANTHROPIC: &str = "anthropic";
pub const PROVIDER_KIMI: &str = "kimi";
pub const PROVIDER_GLM: &str = "glm";
pub const PROVIDER_MINIMAX: &str = "minimax";

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
    #[serde(default = "default_provider")]
    pub provider: String,
    pub five_hour: Option<QuotaWindow>,
    pub seven_day: Option<QuotaWindow>,
    pub fetched_at: Option<DateTime<Utc>>,
    pub source: Option<String>,
    pub last_error: Option<String>,
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
            notify_on_low_quota: false,
            notify_on_reset: false,
            reset_notify_lead_minutes: 15,
            secret_configured: false,
        }
    }
}

impl AppSettings {
    pub fn active_provider(&self) -> &str {
        self.accounts
            .iter()
            .find(|account| account.account_id == self.account_id)
            .map(|account| account.provider.as_str())
            .unwrap_or(PROVIDER_OPENAI)
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
    PROVIDER_OPENAI.into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredOAuthTokens {
    #[serde(default = "default_provider")]
    pub provider: String,
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
    pub provider: String,
    pub auth_mode: AuthMode,
    pub secret: String,
    pub chatgpt_account_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalTokenUsageRange {
    Today,
    Last3Days,
    ThisWeek,
    ThisMonth,
}

impl Default for LocalTokenUsageRange {
    fn default() -> Self {
        Self::ThisMonth
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LocalTokenUsageTotals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_tokens: u64,
    pub cache_hit_rate_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalTokenUsageDay {
    pub date: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_tokens: u64,
    pub models: Vec<LocalTokenUsageModel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalTokenUsageModel {
    pub model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalTokenUsageTool {
    pub tool: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalTokenUsageReport {
    pub range: LocalTokenUsageRange,
    pub totals: LocalTokenUsageTotals,
    pub days: Vec<LocalTokenUsageDay>,
    pub models: Vec<LocalTokenUsageModel>,
    pub tools: Vec<LocalTokenUsageTool>,
    pub missing_sources: Vec<String>,
    pub warnings: Vec<String>,
    pub generated_at: DateTime<Utc>,
}
