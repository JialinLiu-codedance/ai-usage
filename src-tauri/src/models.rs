use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const PROVIDER_OPENAI: &str = "openai";
pub const PROVIDER_ANTHROPIC: &str = "anthropic";
pub const PROVIDER_KIMI: &str = "kimi";
pub const PROVIDER_GLM: &str = "glm";
pub const PROVIDER_MINIMAX: &str = "minimax";
pub const PROVIDER_COPILOT: &str = "copilot";
pub const CUSTOM_USAGE_WINDOW_DAYS: i64 = 90;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
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
    #[serde(default = "default_git_usage_root")]
    pub git_usage_root: String,
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
            git_usage_root: default_git_usage_root(),
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
    #[serde(default = "default_git_usage_root")]
    pub git_usage_root: String,
    pub auth_secret: Option<String>,
}

pub fn default_account_id() -> String {
    "default".into()
}

pub fn default_git_usage_root() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    let project = PathBuf::from(&home).join("project");
    if project.is_dir() {
        project.to_string_lossy().to_string()
    } else {
        home
    }
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
    Custom,
}

impl Default for LocalTokenUsageRange {
    fn default() -> Self {
        Self::ThisMonth
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UsageRangeRequest {
    #[serde(rename_all = "camelCase")]
    Preset { range: LocalTokenUsageRange },
    #[serde(rename_all = "camelCase")]
    Custom {
        start_date: String,
        end_date: String,
    },
}

impl Default for UsageRangeRequest {
    fn default() -> Self {
        Self::Preset {
            range: LocalTokenUsageRange::ThisMonth,
        }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
    #[serde(default)]
    pub pending: bool,
    pub totals: LocalTokenUsageTotals,
    pub days: Vec<LocalTokenUsageDay>,
    pub models: Vec<LocalTokenUsageModel>,
    pub tools: Vec<LocalTokenUsageTool>,
    pub missing_sources: Vec<String>,
    pub warnings: Vec<String>,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GitUsageTotals {
    pub added_lines: u64,
    pub deleted_lines: u64,
    pub changed_files: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitUsageBucket {
    pub date: String,
    pub added_lines: u64,
    pub deleted_lines: u64,
    pub changed_files: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitUsageRepository {
    pub name: String,
    pub path: String,
    pub added_lines: u64,
    pub deleted_lines: u64,
    pub changed_files: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitUsageReport {
    pub range: LocalTokenUsageRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
    #[serde(default)]
    pub pending: bool,
    pub totals: GitUsageTotals,
    pub buckets: Vec<GitUsageBucket>,
    #[serde(default)]
    pub repositories: Vec<GitUsageRepository>,
    pub repository_count: usize,
    pub missing_sources: Vec<String>,
    pub warnings: Vec<String>,
    pub generated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrKpiMetricKey {
    CycleTimeAi,
    MergedAiPrsPerWeek,
    ReviewCommentsPerPr,
    TestAddedRatio,
    #[serde(rename = "7d_rework_rate")]
    SevenDayReworkRate,
    #[serde(rename = "7d_retention_rate")]
    SevenDayRetentionRate,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrKpiOverview {
    pub token_total: u64,
    pub code_lines: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_ratio: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrKpiMetric {
    pub key: PrKpiMetricKey,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_value: Option<f64>,
    pub display_value: String,
    #[serde(default)]
    pub is_missing: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrKpiReport {
    pub range: LocalTokenUsageRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub end_date: Option<String>,
    #[serde(default)]
    pub pending: bool,
    pub overview: PrKpiOverview,
    pub metrics: Vec<PrKpiMetric>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overall_score: Option<f64>,
    pub missing_sources: Vec<String>,
    pub warnings: Vec<String>,
    pub generated_at: DateTime<Utc>,
}
