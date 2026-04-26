import { invoke } from "@tauri-apps/api/core";
import type {
  AccountQuotaStatus,
  AppSettings,
  AppStatus,
  ConnectedAccount,
  ConnectionTestResult,
  OAuthStatus,
  SaveSettingsInput,
} from "./types";

const isTauriRuntime = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
let mockOAuthSequence = 0;
let mockOAuthCompleteSequence = 0;
let mockPendingOAuthAccountId: string | null = null;

type OAuthProviderKey = "openai" | "anthropic";
type MockProviderKey = OAuthProviderKey | "kimi" | "minimax";

const oauthProviderConfig: Record<
  OAuthProviderKey,
  {
    authorizeUrl: string;
    clientId: string;
    redirectUri: string;
    scope: string;
    defaultAccountName: string;
  }
> = {
  openai: {
    authorizeUrl: "https://auth.openai.com/oauth/authorize",
    clientId: "app_EMoamEEZ73f0CkXaXp7hrann",
    redirectUri: "http://localhost:1455/auth/callback",
    scope: "openid profile email offline_access",
    defaultAccountName: "OpenAI Account",
  },
  anthropic: {
    authorizeUrl: "https://claude.ai/oauth/authorize",
    clientId: "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
    redirectUri: "https://platform.claude.com/oauth/code/callback",
    scope: "org:create_api_key user:profile user:inference user:sessions:claude_code user:mcp_servers user:file_upload",
    defaultAccountName: "Anthropic Account",
  },
};

function mockAccount(
  accountId: string,
  accountName: string,
  provider: MockProviderKey = "openai",
  authMode: ConnectedAccount["auth_mode"] = "oauth",
): ConnectedAccount {
  return {
    account_id: accountId,
    account_name: accountName,
    provider,
    auth_mode: authMode,
    chatgpt_account_id: provider === "openai" ? accountId : null,
    secret_configured: true,
  };
}

function initialMockSettings(): AppSettings {
  return {
    account_id: "default",
    account_name: "OpenAI Account",
    auth_mode: "oauth",
    base_url_override: null,
    chatgpt_account_id: "default",
    accounts: [mockAccount("default", "OpenAI Account")],
    refresh_interval_minutes: 15,
    low_quota_threshold_percent: 10,
    notify_on_low_quota: true,
    notify_on_reset: false,
    reset_notify_lead_minutes: 15,
    secret_configured: true,
  };
}

function emptyMockSettings(): AppSettings {
  return {
    account_id: "default",
    account_name: "OpenAI Account",
    auth_mode: "apiKey",
    base_url_override: null,
    chatgpt_account_id: null,
    accounts: [],
    refresh_interval_minutes: 15,
    low_quota_threshold_percent: 10,
    notify_on_low_quota: true,
    notify_on_reset: false,
    reset_notify_lead_minutes: 15,
    secret_configured: false,
  };
}

let mockSettings: AppSettings = initialMockSettings();

let mockStatus: AppStatus = {
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
  accounts: [
    {
      account_id: "default",
      account_name: "OpenAI Account",
      provider: "openai",
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
  ],
  refresh_status: "ok",
  last_error: null,
  last_refreshed_at: new Date().toISOString(),
};

export function resetMockTauriStateForTests(): void {
  mockOAuthSequence = 0;
  mockOAuthCompleteSequence = 0;
  mockPendingOAuthAccountId = null;
  mockSettings = emptyMockSettings();
  mockStatus = {
    snapshot: null,
    accounts: [],
    refresh_status: "idle",
    last_error: null,
    last_refreshed_at: null,
  };
}

export async function getCurrentQuota(): Promise<AppStatus> {
  if (!isTauriRuntime) {
    return mockStatusWithAccounts();
  }
  const status = await invoke<AppStatus>("get_current_quota");
  await syncTrayMenu();
  return status;
}

export async function refreshQuota(): Promise<AppStatus> {
  if (!isTauriRuntime) {
    return mockStatusWithAccounts();
  }
  const status = await invoke<AppStatus>("refresh_quota");
  await syncTrayMenu();
  return status;
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

export async function importKimiAccount(accountName?: string | null, accountId?: string | null): Promise<AppSettings> {
  const normalizedName = accountName?.trim() || "Kimi Account";
  if (!isTauriRuntime) {
    const nextAccountId = accountId?.trim() || uniqueMockAccountId("kimi", normalizedName);
    const nextAccount = mockAccount(nextAccountId, normalizedName, "kimi");
    const existingIndex = mockSettings.accounts.findIndex((account) => account.account_id === nextAccountId);
    const accounts =
      existingIndex >= 0
        ? mockSettings.accounts.map((account, index) => (index === existingIndex ? nextAccount : account))
        : [...mockSettings.accounts, nextAccount];

    mockSettings = {
      ...mockSettings,
      account_id: nextAccountId,
      account_name: normalizedName,
      auth_mode: "oauth",
      chatgpt_account_id: null,
      accounts,
      secret_configured: true,
    };
    mockStatus = {
      ...mockStatus,
      accounts: mockAccountStatuses(accounts),
      snapshot: mockStatus.snapshot
        ? { ...mockStatus.snapshot, account_id: nextAccountId, account_name: normalizedName }
        : null,
    };
    return mockSettings;
  }
  return invoke("import_kimi_account", {
    accountName: normalizedName,
    accountId: accountId?.trim() || null,
  });
}

export async function importMiniMaxAccount(
  accountName: string,
  apiKey: string,
  accountId?: string | null,
): Promise<AppSettings> {
  const normalizedName = accountName.trim() || "MiniMax Account";
  const normalizedApiKey = apiKey.trim();
  if (!normalizedApiKey) {
    throw new Error("请填写 MiniMax API Key");
  }
  if (!isTauriRuntime) {
    const nextAccountId = accountId?.trim() || uniqueMockAccountId("minimax", normalizedName);
    const nextAccount = mockAccount(nextAccountId, normalizedName, "minimax", "apiKey");
    const existingIndex = mockSettings.accounts.findIndex((account) => account.account_id === nextAccountId);
    const accounts =
      existingIndex >= 0
        ? mockSettings.accounts.map((account, index) => (index === existingIndex ? nextAccount : account))
        : [...mockSettings.accounts, nextAccount];

    mockSettings = {
      ...mockSettings,
      account_id: nextAccountId,
      account_name: normalizedName,
      auth_mode: "apiKey",
      chatgpt_account_id: null,
      accounts,
      secret_configured: true,
    };
    mockStatus = {
      ...mockStatus,
      accounts: mockAccountStatuses(accounts),
      snapshot: mockStatus.snapshot
        ? { ...mockStatus.snapshot, account_id: nextAccountId, account_name: normalizedName }
        : null,
    };
    return mockSettings;
  }
  return invoke("import_minimax_account", {
    accountName: normalizedName,
    apiKey: normalizedApiKey,
    accountId: accountId?.trim() || null,
  });
}

export async function testConnection(): Promise<ConnectionTestResult> {
  if (!isTauriRuntime) {
    return { success: true, message: "Mock connection succeeded" };
  }
  return invoke("test_connection");
}

export async function startOpenAIOAuth(accountId?: string | null): Promise<string> {
  return startProviderOAuth("openai", accountId);
}

export async function startAnthropicOAuth(accountId?: string | null): Promise<string> {
  return startProviderOAuth("anthropic", accountId);
}

async function startProviderOAuth(provider: OAuthProviderKey, accountId?: string | null): Promise<string> {
  if (!isTauriRuntime) {
    mockPendingOAuthAccountId = accountId?.trim() || null;
    mockOAuthSequence += 1;
    const nonce = `${Date.now().toString(36)}-${mockOAuthSequence.toString(36)}`;
    const config = oauthProviderConfig[provider];
    const params = new URLSearchParams({
      client_id: config.clientId,
      redirect_uri: config.redirectUri,
      scope: config.scope,
      response_type: "code",
      code_challenge: `mocked_for_preview_${nonce}`,
      code_challenge_method: "S256",
      state: `preview_${nonce}`,
    });
    if (provider === "openai") {
      params.set("id_token_add_organizations", "true");
      params.set("codex_cli_simplified_flow", "true");
    } else {
      params.set("code", "true");
    }
    return `${config.authorizeUrl}?${params.toString()}`;
  }
  const command = provider === "anthropic" ? "start_anthropic_oauth" : "start_openai_oauth";
  return invoke(command, { accountId: accountId ?? null });
}

export async function getOAuthStatus(): Promise<OAuthStatus> {
  if (!isTauriRuntime) {
    return { phase: "idle", message: null, email: null, auth_url: null };
  }
  return invoke("get_oauth_status");
}

export async function completeOpenAIOAuth(callbackUrl: string): Promise<OAuthStatus> {
  return completeProviderOAuth("openai", callbackUrl);
}

export async function completeAnthropicOAuth(callbackUrl: string): Promise<OAuthStatus> {
  return completeProviderOAuth("anthropic", callbackUrl);
}

async function completeProviderOAuth(provider: OAuthProviderKey, callbackUrl: string): Promise<OAuthStatus> {
  if (!isTauriRuntime) {
    mockOAuthCompleteSequence += 1;
    const config = oauthProviderConfig[provider];
    const fallbackEmail =
      provider === "anthropic" ? `claude+${mockOAuthCompleteSequence}@example.com` : `john+${mockOAuthCompleteSequence}@example.com`;
    const email = mockEmailFromCallback(callbackUrl) ?? fallbackEmail;
    const accountId = mockPendingOAuthAccountId ?? uniqueMockAccountId(provider, email);
    const nextAccount = mockAccount(accountId, email || config.defaultAccountName, provider);
    const existingIndex = mockSettings.accounts.findIndex((account) => account.account_id === accountId);
    const accounts =
      existingIndex >= 0
        ? mockSettings.accounts.map((account, index) => (index === existingIndex ? nextAccount : account))
        : [...mockSettings.accounts, nextAccount];

    mockSettings = {
      ...mockSettings,
      account_id: accountId,
      account_name: email,
      auth_mode: "oauth",
      chatgpt_account_id: provider === "openai" ? accountId : null,
      accounts,
      secret_configured: true,
    };
    mockStatus = {
      ...mockStatus,
      accounts: mockAccountStatuses(accounts),
      snapshot: mockStatus.snapshot
        ? { ...mockStatus.snapshot, account_id: accountId, account_name: email }
        : null,
    };
    mockPendingOAuthAccountId = null;
    return { phase: "success", message: callbackUrl, email, auth_url: null };
  }
  const command = provider === "anthropic" ? "complete_anthropic_oauth" : "complete_openai_oauth";
  return invoke(command, { callbackUrl });
}

export async function deleteOpenAIAccount(accountId: string): Promise<AppSettings> {
  return deleteConnectedAccount(accountId);
}

export async function deleteConnectedAccount(accountId: string): Promise<AppSettings> {
  if (!isTauriRuntime) {
    const accounts = mockSettings.accounts.filter((account) => account.account_id !== accountId);
    const activeAccount =
      accounts.find((account) => account.account_id === mockSettings.account_id) ?? accounts[0] ?? null;
    mockSettings = activeAccount
      ? {
          ...mockSettings,
          account_id: activeAccount.account_id,
          account_name: activeAccount.account_name,
          auth_mode: activeAccount.auth_mode,
          chatgpt_account_id: activeAccount.chatgpt_account_id,
          accounts,
          secret_configured: true,
        }
      : {
          ...mockSettings,
          account_id: "default",
          account_name: "OpenAI Account",
          auth_mode: "apiKey",
          chatgpt_account_id: null,
          accounts: [],
          secret_configured: false,
        };
    mockStatus = {
      snapshot: activeAccount
        ? mockStatus.snapshot
          ? {
              ...mockStatus.snapshot,
              account_id: activeAccount.account_id,
              account_name: activeAccount.account_name,
            }
          : null
        : null,
      accounts: mockAccountStatuses(accounts),
      refresh_status: activeAccount ? mockStatus.refresh_status : "idle",
      last_error: activeAccount ? mockStatus.last_error : null,
      last_refreshed_at: activeAccount ? mockStatus.last_refreshed_at : null,
    };
    return mockSettings;
  }
  return invoke("delete_connected_account", { accountId });
}

function mockStatusWithAccounts(): AppStatus {
  return {
    ...mockStatus,
    accounts: mockStatus.accounts.length > 0 ? mockStatus.accounts : mockAccountStatuses(mockSettings.accounts),
  };
}

function mockAccountStatuses(accounts: ConnectedAccount[]): AccountQuotaStatus[] {
  return accounts.map((account) => {
    const snapshot = mockStatus.snapshot?.account_id === account.account_id ? mockStatus.snapshot : null;
    return {
      account_id: account.account_id,
      account_name: account.account_name,
      provider: account.provider,
      five_hour: snapshot?.five_hour ?? null,
      seven_day: snapshot?.seven_day ?? null,
      fetched_at: snapshot?.fetched_at ?? null,
      source: snapshot?.source ?? null,
    };
  });
}

function mockEmailFromCallback(input: string): string | null {
  try {
    const parsed = new URL(input);
    return parsed.searchParams.get("email");
  } catch {
    return null;
  }
}

function uniqueMockAccountId(provider: MockProviderKey, email: string): string {
  const base = email
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "");
  const normalizedBase = provider === "openai" ? base || "openai" : `${provider}-${base || "account"}`;
  if (!mockSettings.accounts.some((account) => account.account_id === normalizedBase)) {
    return normalizedBase;
  }
  for (let index = 2; ; index += 1) {
    const candidate = `${normalizedBase}-${index}`;
    if (!mockSettings.accounts.some((account) => account.account_id === candidate)) {
      return candidate;
    }
  }
}

export async function resizePanel(width: number, height: number): Promise<void> {
  if (!isTauriRuntime) {
    return;
  }
  return invoke("resize_main_panel", { width, height });
}

async function syncTrayMenu(): Promise<void> {
  if (!isTauriRuntime) {
    return;
  }
  return invoke("sync_tray_menu");
}
