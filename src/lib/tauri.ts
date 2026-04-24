import { invoke } from "@tauri-apps/api/core";
import type {
  AppSettings,
  AppStatus,
  ConnectionTestResult,
  OAuthStatus,
  SaveSettingsInput,
} from "./types";

const isTauriRuntime = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

let mockSettings: AppSettings = {
  account_id: "default",
  account_name: "OpenAI Account",
  auth_mode: "oauth",
  base_url_override: null,
  chatgpt_account_id: null,
  refresh_interval_minutes: 15,
  low_quota_threshold_percent: 10,
  notify_on_low_quota: true,
  notify_on_reset: false,
  reset_notify_lead_minutes: 15,
  secret_configured: true,
};

const mockStatus: AppStatus = {
  snapshot: {
    account_id: "default",
    account_name: "OpenAI Account",
    five_hour: {
      used_percent: 55,
      remaining_percent: 45,
      reset_at: null,
      window_minutes: 300,
    },
    seven_day: {
      used_percent: 11,
      remaining_percent: 89,
      reset_at: null,
      window_minutes: 10080,
    },
    fetched_at: new Date().toISOString(),
    source: "probe_headers",
  },
  refresh_status: "ok",
  last_error: null,
  last_refreshed_at: new Date().toISOString(),
};

export async function getCurrentQuota(): Promise<AppStatus> {
  if (!isTauriRuntime) {
    return mockStatus;
  }
  return invoke("get_current_quota");
}

export async function refreshQuota(): Promise<AppStatus> {
  if (!isTauriRuntime) {
    return mockStatus;
  }
  return invoke("refresh_quota");
}

export async function getSettings(): Promise<AppSettings> {
  if (!isTauriRuntime) {
    return mockSettings;
  }
  return invoke("get_settings");
}

export async function saveSettings(input: SaveSettingsInput): Promise<AppSettings> {
  if (!isTauriRuntime) {
    const { auth_secret: authSecret, ...settingsInput } = input;
    mockSettings = {
      ...mockSettings,
      ...settingsInput,
      secret_configured: mockSettings.secret_configured || Boolean(authSecret),
    };
    return mockSettings;
  }
  return invoke("save_settings", { input });
}

export async function testConnection(): Promise<ConnectionTestResult> {
  if (!isTauriRuntime) {
    return { success: true, message: "Mock connection succeeded" };
  }
  return invoke("test_connection");
}

export async function startOpenAIOAuth(): Promise<string> {
  if (!isTauriRuntime) {
    return "https://auth.openai.com/oauth/authorize?client_id=app_EMoamEEZ73f0CkXaXp7hrann&code_challenge=mocked_for_preview";
  }
  return invoke("start_openai_oauth");
}

export async function getOAuthStatus(): Promise<OAuthStatus> {
  if (!isTauriRuntime) {
    return { phase: "idle", message: null, email: null, auth_url: null };
  }
  return invoke("get_oauth_status");
}

export async function completeOpenAIOAuth(callbackUrl: string): Promise<OAuthStatus> {
  if (!isTauriRuntime) {
    return { phase: "success", message: callbackUrl, email: "john@example.com", auth_url: null };
  }
  return invoke("complete_openai_oauth", { callbackUrl });
}
