export type RefreshStatus = "idle" | "refreshing" | "ok" | "error";

export type AuthMode = "apiKey" | "oauth" | "sessionToken" | "cookie";

export interface QuotaWindow {
  label?: string | null;
  used_percent: number;
  remaining_percent: number;
  reset_at: string | null;
  window_minutes: number | null;
}

export interface QuotaSnapshot {
  account_id: string;
  account_name: string;
  five_hour: QuotaWindow | null;
  seven_day: QuotaWindow | null;
  fetched_at: string;
  source: string;
}

export interface AppSettings {
  account_id: string;
  account_name: string;
  auth_mode: AuthMode;
  base_url_override: string | null;
  chatgpt_account_id: string | null;
  accounts: ConnectedAccount[];
  refresh_interval_minutes: number;
  low_quota_threshold_percent: number;
  notify_on_low_quota: boolean;
  notify_on_reset: boolean;
  reset_notify_lead_minutes: number;
  git_usage_root: string;
  launch_at_login: boolean;
  claude_proxy: ClaudeProxyConfig;
  claude_proxy_profiles: Record<string, ClaudeProxyProfileSummary>;
  reverse_proxy: ReverseProxyConfig;
  secret_configured: boolean;
}

export interface SaveSettingsInput {
  account_id: string;
  account_name: string;
  auth_mode: AuthMode;
  base_url_override: string | null;
  chatgpt_account_id: string | null;
  refresh_interval_minutes: number;
  low_quota_threshold_percent: number;
  notify_on_low_quota: boolean;
  notify_on_reset: boolean;
  reset_notify_lead_minutes: number;
  git_usage_root: string;
  launch_at_login: boolean;
  auth_secret?: string | null;
}

export type ClaudeApiFormat = "anthropic" | "openai_chat" | "openai_responses";
export type ClaudeAuthField = "ANTHROPIC_AUTH_TOKEN" | "ANTHROPIC_API_KEY";

export interface ClaudeProxyProfileSummary {
  base_url: string | null;
  api_format: ClaudeApiFormat;
  auth_field: ClaudeAuthField;
  secret_configured: boolean;
}

export interface ClaudeProxyCapability {
  account_id: string;
  kind: ProxyTargetKind;
  provider: string;
  display_name: string;
  is_claude_compatible_provider: boolean;
  can_direct_connect: boolean;
  missing_fields: string[];
  status: ProxyTargetStatus;
  profile: ClaudeProxyProfileSummary;
  resolved_profile: ClaudeProxyProfileSummary | null;
}

export type ProxyTargetKind = "direct_account" | "reverse_copilot" | "reverse_openai";
export type ProxyTargetStatus =
  | "unsupported"
  | "direct_ready"
  | "needs_profile"
  | "reverse_pending"
  | "reverse_ready";

export interface ClaudeModelRoute {
  id: string;
  model_pattern: string;
  account_id: string;
  enabled: boolean;
}

export interface ClaudeProxyConfig {
  listen_address: string;
  listen_port: number;
  routes: ClaudeModelRoute[];
}

export interface LocalProxySettingsState {
  config: ClaudeProxyConfig;
  capabilities: ClaudeProxyCapability[];
}

export interface SaveLocalProxySettingsInput {
  config: ClaudeProxyConfig;
}

export interface ClaudeProxyProfileInput {
  account_id: string;
  base_url: string | null;
  api_format: ClaudeApiFormat;
  auth_field: ClaudeAuthField;
  api_key_or_token?: string | null;
}

export interface LocalProxyStatus {
  running: boolean;
  address: string;
  port: number;
  active_connections: number;
  total_requests: number;
  successful_requests: number;
  failed_requests: number;
  success_rate: number;
  uptime_seconds: number;
  last_error: string | null;
}

export interface ReverseProxyConfig {
  enabled: boolean;
  default_openai_account_id: string | null;
  default_copilot_account_id: string | null;
}

export interface ManagedAuthAccount {
  id: string;
  login: string;
  avatar_url: string | null;
  authenticated_at: number;
  domain: string | null;
}

export interface GitHubDeviceCodeResponse {
  device_code: string;
  user_code: string;
  verification_uri: string;
  expires_in: number;
  interval: number;
}

export interface CopilotAuthStatus {
  accounts: ManagedAuthAccount[];
  default_account_id: string | null;
  authenticated: boolean;
}

export interface ReverseProxySettingsState {
  enabled: boolean;
  copilot_accounts: ManagedAuthAccount[];
  openai_accounts: ManagedAuthAccount[];
  default_copilot_account_id: string | null;
  default_openai_account_id: string | null;
}

export interface SaveReverseProxySettingsInput {
  enabled: boolean;
  default_copilot_account_id?: string | null;
  default_openai_account_id?: string | null;
}

export interface ReverseProxyStatus {
  enabled: boolean;
  copilot_ready: boolean;
  openai_ready: boolean;
  available_copilot_accounts: number;
  available_openai_accounts: number;
}

export interface ConnectedAccount {
  account_id: string;
  account_name: string;
  provider: string;
  auth_mode: AuthMode;
  chatgpt_account_id: string | null;
  secret_configured: boolean;
}

export interface AccountQuotaStatus {
  account_id: string;
  account_name: string;
  provider: string;
  five_hour: QuotaWindow | null;
  seven_day: QuotaWindow | null;
  fetched_at: string | null;
  source: "probe_headers" | string | null;
  last_error: string | null;
}

export interface AppStatus {
  snapshot: QuotaSnapshot | null;
  accounts: AccountQuotaStatus[];
  refresh_status: RefreshStatus;
  last_error: string | null;
  last_refreshed_at: string | null;
}

export interface ConnectionTestResult {
  success: boolean;
  message: string;
}

export interface OAuthStatus {
  phase: "idle" | "running" | "success" | "error";
  message: string | null;
  email: string | null;
  auth_url: string | null;
}

export interface AppUpdateInfo {
  version: string;
  currentVersion: string;
  body: string | null;
  date: string | null;
}

export interface AppUpdateDownloadEvent {
  event: "Started" | "Progress" | "Finished";
  data: {
    contentLength?: number;
    chunkLength?: number;
  };
}

export type PresetUsageRange = "today" | "last3Days" | "thisWeek" | "thisMonth";
export type LocalTokenUsageRange = PresetUsageRange | "last3Days" | "custom";
export type UsageRangeSelection =
  | { kind: "preset"; range: PresetUsageRange }
  | { kind: "custom"; startDate: string; endDate: string };

export interface LocalTokenUsageTotals {
  input_tokens: number;
  output_tokens: number;
  cache_read_tokens: number;
  cache_creation_tokens: number;
  total_tokens: number;
  cache_hit_rate_percent: number;
}

export interface LocalTokenUsageDay extends Omit<LocalTokenUsageTotals, "cache_hit_rate_percent"> {
  date: string;
  models: LocalTokenUsageModel[];
}

export interface LocalTokenUsageModel extends Omit<LocalTokenUsageTotals, "cache_hit_rate_percent"> {
  model: string;
}

export interface LocalTokenUsageTool extends Omit<LocalTokenUsageTotals, "cache_hit_rate_percent"> {
  tool: string;
}

export interface LocalTokenUsageReport {
  range: LocalTokenUsageRange;
  start_date?: string | null;
  end_date?: string | null;
  pending?: boolean;
  totals: LocalTokenUsageTotals;
  days: LocalTokenUsageDay[];
  models: LocalTokenUsageModel[];
  tools: LocalTokenUsageTool[];
  missing_sources: string[];
  warnings: string[];
  generated_at: string;
}

export interface GitUsageTotals {
  added_lines: number;
  deleted_lines: number;
  changed_files: number;
}

export interface GitUsageBucket extends GitUsageTotals {
  date: string;
}

export interface GitUsageRepository extends GitUsageTotals {
  name: string;
  path: string;
}

export interface GitUsageReport {
  range: LocalTokenUsageRange;
  start_date?: string | null;
  end_date?: string | null;
  pending?: boolean;
  totals: GitUsageTotals;
  buckets: GitUsageBucket[];
  repositories: GitUsageRepository[];
  repository_count: number;
  missing_sources: string[];
  warnings: string[];
  generated_at: string;
}

export type PrKpiMetricKey =
  | "cycle_time_ai"
  | "merged_ai_prs_per_week"
  | "review_comments_per_pr"
  | "test_added_ratio"
  | "7d_rework_rate"
  | "7d_retention_rate";

export interface PrKpiOverview {
  token_total: number;
  code_lines: number;
  output_ratio: number | null;
}

export interface PrKpiMetric {
  key: PrKpiMetricKey;
  label: string;
  score: number | null;
  raw_value: number | null;
  display_value: string;
  is_missing: boolean;
}

export interface PrKpiReport {
  range: LocalTokenUsageRange;
  start_date?: string | null;
  end_date?: string | null;
  pending?: boolean;
  overview: PrKpiOverview;
  metrics: PrKpiMetric[];
  overall_score: number | null;
  missing_sources: string[];
  warnings: string[];
  generated_at: string;
}
