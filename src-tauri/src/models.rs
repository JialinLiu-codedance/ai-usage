use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf};

pub const PROVIDER_OPENAI: &str = "openai";
pub const PROVIDER_ANTHROPIC: &str = "anthropic";
pub const PROVIDER_KIMI: &str = "kimi";
pub const PROVIDER_GLM: &str = "glm";
pub const PROVIDER_MINIMAX: &str = "minimax";
pub const PROVIDER_COPILOT: &str = "copilot";
pub const PROVIDER_QWEN: &str = "qwen";
pub const PROVIDER_XIAOMI: &str = "xiaomi";
pub const PROVIDER_CUSTOM: &str = "custom";
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
    #[serde(with = "crate::app_time::local_datetime_serde::option")]
    pub reset_at: Option<DateTime<Utc>>,
    pub window_minutes: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaSnapshot {
    pub account_id: String,
    pub account_name: String,
    pub five_hour: Option<QuotaWindow>,
    pub seven_day: Option<QuotaWindow>,
    #[serde(with = "crate::app_time::local_datetime_serde")]
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
    #[serde(with = "crate::app_time::local_datetime_serde::option")]
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
    #[serde(default)]
    pub launch_at_login: bool,
    #[serde(default)]
    pub claude_proxy: ClaudeProxyConfig,
    #[serde(default)]
    pub claude_proxy_profiles: HashMap<String, ClaudeProxyProfileSettings>,
    #[serde(default)]
    pub reverse_proxy: ReverseProxyConfig,
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
            launch_at_login: false,
            claude_proxy: ClaudeProxyConfig::default(),
            claude_proxy_profiles: HashMap::new(),
            reverse_proxy: ReverseProxyConfig::default(),
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
    #[serde(default)]
    pub launch_at_login: bool,
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

fn default_true() -> bool {
    true
}

fn default_claude_proxy_listen_address() -> String {
    "127.0.0.1".into()
}

fn default_claude_proxy_listen_port() -> u16 {
    16555
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredOAuthTokens {
    #[serde(default = "default_provider")]
    pub provider: String,
    pub account_id: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    #[serde(with = "crate::app_time::local_datetime_serde")]
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
    #[serde(with = "crate::app_time::local_datetime_serde::option")]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeApiFormat {
    Anthropic,
    OpenaiChat,
    OpenaiResponses,
}

impl Default for ClaudeApiFormat {
    fn default() -> Self {
        Self::Anthropic
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClaudeAuthField {
    #[serde(rename = "ANTHROPIC_AUTH_TOKEN")]
    AnthropicAuthToken,
    #[serde(rename = "ANTHROPIC_API_KEY")]
    AnthropicApiKey,
}

impl Default for ClaudeAuthField {
    fn default() -> Self {
        Self::AnthropicAuthToken
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeProxyProfileSettings {
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_format: ClaudeApiFormat,
    #[serde(default)]
    pub auth_field: ClaudeAuthField,
    #[serde(default)]
    pub secret_configured: bool,
}

impl Default for ClaudeProxyProfileSettings {
    fn default() -> Self {
        Self {
            base_url: None,
            api_format: ClaudeApiFormat::Anthropic,
            auth_field: ClaudeAuthField::AnthropicAuthToken,
            secret_configured: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeProxyProfileSummary {
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_format: ClaudeApiFormat,
    #[serde(default)]
    pub auth_field: ClaudeAuthField,
    #[serde(default)]
    pub secret_configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeProxyCapability {
    pub account_id: String,
    #[serde(default)]
    pub kind: ProxyTargetKind,
    pub provider: String,
    pub display_name: String,
    pub is_claude_compatible_provider: bool,
    pub can_direct_connect: bool,
    pub missing_fields: Vec<String>,
    #[serde(default)]
    pub status: ProxyTargetStatus,
    pub profile: ClaudeProxyProfileSummary,
    pub resolved_profile: Option<ClaudeProxyProfileSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeProxyProfileInput {
    pub account_id: String,
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_format: ClaudeApiFormat,
    #[serde(default)]
    pub auth_field: ClaudeAuthField,
    pub api_key_or_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeModelRoute {
    pub id: String,
    pub model_pattern: String,
    pub account_id: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeProxyConfig {
    #[serde(default = "default_claude_proxy_listen_address")]
    pub listen_address: String,
    #[serde(default = "default_claude_proxy_listen_port")]
    pub listen_port: u16,
    #[serde(default)]
    pub routes: Vec<ClaudeModelRoute>,
}

impl Default for ClaudeProxyConfig {
    fn default() -> Self {
        Self {
            listen_address: default_claude_proxy_listen_address(),
            listen_port: default_claude_proxy_listen_port(),
            routes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalProxySettingsState {
    pub config: ClaudeProxyConfig,
    pub capabilities: Vec<ClaudeProxyCapability>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyTargetKind {
    #[default]
    DirectAccount,
    ReverseCopilot,
    ReverseOpenai,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyTargetStatus {
    #[default]
    Unsupported,
    DirectReady,
    NeedsProfile,
    ReversePending,
    ReverseReady,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReverseProxyConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_openai_account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_copilot_account_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedAuthAccount {
    pub id: String,
    pub login: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    pub authenticated_at: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GitHubDeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CopilotAuthStatus {
    pub accounts: Vec<ManagedAuthAccount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_account_id: Option<String>,
    pub authenticated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReverseProxySettingsState {
    pub enabled: bool,
    pub copilot_accounts: Vec<ManagedAuthAccount>,
    pub openai_accounts: Vec<ManagedAuthAccount>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_copilot_account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_openai_account_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveReverseProxySettingsInput {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_copilot_account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_openai_account_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReverseProxyStatus {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub copilot_ready: bool,
    #[serde(default)]
    pub openai_ready: bool,
    #[serde(default)]
    pub available_copilot_accounts: usize,
    #[serde(default)]
    pub available_openai_accounts: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveLocalProxySettingsInput {
    pub config: ClaudeProxyConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalProxyMatchResult {
    pub matched: bool,
    pub route_id: Option<String>,
    pub model_pattern: Option<String>,
    pub account_id: Option<String>,
    pub display_name: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalProxyStatus {
    #[serde(default)]
    pub running: bool,
    pub address: String,
    pub port: u16,
    #[serde(default)]
    pub active_connections: usize,
    #[serde(default)]
    pub total_requests: u64,
    #[serde(default)]
    pub successful_requests: u64,
    #[serde(default)]
    pub failed_requests: u64,
    #[serde(default)]
    pub success_rate: f64,
    #[serde(default)]
    pub uptime_seconds: u64,
    pub last_error: Option<String>,
}

impl Default for LocalProxyStatus {
    fn default() -> Self {
        Self {
            running: false,
            address: default_claude_proxy_listen_address(),
            port: default_claude_proxy_listen_port(),
            active_connections: 0,
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            success_rate: 0.0,
            uptime_seconds: 0,
            last_error: None,
        }
    }
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
    #[serde(with = "crate::app_time::local_datetime_serde")]
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
pub struct GitUsageCommit {
    pub commit_hash: String,
    pub short_hash: String,
    #[serde(with = "crate::app_time::local_datetime_serde")]
    pub timestamp: DateTime<Utc>,
    pub author_name: String,
    pub author_email: String,
    #[serde(default)]
    pub committer_name: String,
    #[serde(default)]
    pub committer_email: String,
    pub subject: String,
    pub repository_name: String,
    pub repository_path: String,
    #[serde(default)]
    pub parent_count: usize,
    #[serde(default)]
    pub patch_id: String,
    #[serde(default)]
    pub duplicate_group_id: String,
    #[serde(default)]
    pub duplicate_group_size: usize,
    #[serde(default)]
    pub is_group_representative: bool,
    #[serde(default)]
    pub commit_role: String,
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
    #[serde(default)]
    pub commits: Vec<GitUsageCommit>,
    pub repository_count: usize,
    pub missing_sources: Vec<String>,
    pub warnings: Vec<String>,
    #[serde(with = "crate::app_time::local_datetime_serde")]
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
    #[serde(with = "crate::app_time::local_datetime_serde")]
    pub generated_at: DateTime<Utc>,
}
