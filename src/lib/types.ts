export type RefreshStatus = "idle" | "refreshing" | "ok" | "error";

export type AuthMode = "apiKey" | "sessionToken" | "cookie";

export interface QuotaWindow {
  used_percent: number;
  remaining_percent: number;
  reset_at: string | null;
  window_minutes: number | null;
}

export interface QuotaSnapshot {
  account_name: string;
  five_hour: QuotaWindow | null;
  seven_day: QuotaWindow | null;
  fetched_at: string;
  source: "probe_headers";
}

export interface AppSettings {
  account_name: string;
  auth_mode: AuthMode;
  base_url_override: string | null;
  chatgpt_account_id: string | null;
  refresh_interval_minutes: number;
  low_quota_threshold_percent: number;
  notify_on_low_quota: boolean;
  notify_on_reset: boolean;
  reset_notify_lead_minutes: number;
  secret_configured: boolean;
}

export interface SaveSettingsInput {
  account_name: string;
  auth_mode: AuthMode;
  base_url_override: string | null;
  chatgpt_account_id: string | null;
  refresh_interval_minutes: number;
  low_quota_threshold_percent: number;
  notify_on_low_quota: boolean;
  notify_on_reset: boolean;
  reset_notify_lead_minutes: number;
  auth_secret?: string | null;
}

export interface AppStatus {
  snapshot: QuotaSnapshot | null;
  refresh_status: RefreshStatus;
  last_error: string | null;
  last_refreshed_at: string | null;
}

export interface ConnectionTestResult {
  success: boolean;
  message: string;
}
