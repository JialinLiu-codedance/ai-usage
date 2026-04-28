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
  auth_secret?: string | null;
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

export type LocalTokenUsageRange = "today" | "last3Days" | "thisWeek" | "thisMonth";

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
  totals: GitUsageTotals;
  buckets: GitUsageBucket[];
  repositories: GitUsageRepository[];
  repository_count: number;
  missing_sources: string[];
  warnings: string[];
  generated_at: string;
}
