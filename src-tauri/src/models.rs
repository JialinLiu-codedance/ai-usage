use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuthMode {
    ApiKey,
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
pub struct AppSettings {
    pub account_name: String,
    pub auth_mode: AuthMode,
    pub base_url_override: Option<String>,
    pub chatgpt_account_id: Option<String>,
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
            account_name: "OpenAI Account".into(),
            auth_mode: AuthMode::ApiKey,
            base_url_override: None,
            chatgpt_account_id: None,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStatus {
    pub snapshot: Option<QuotaSnapshot>,
    pub refresh_status: RefreshStatus,
    pub last_error: Option<String>,
    pub last_refreshed_at: Option<DateTime<Utc>>,
}

impl Default for AppStatus {
    fn default() -> Self {
        Self {
            snapshot: None,
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

#[derive(Debug, Clone)]
pub struct ProbeCredentials {
    pub auth_mode: AuthMode,
    pub secret: String,
    pub chatgpt_account_id: Option<String>,
}
