import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  AccountQuotaStatus,
  AppUpdateDownloadEvent,
  AppUpdateInfo,
  AppSettings,
  AppStatus,
  ClaudeApiFormat,
  ClaudeAuthField,
  ClaudeProxyCapability,
  ClaudeProxyConfig,
  ClaudeProxyProfileInput,
  ClaudeProxyProfileSummary,
  ConnectedAccount,
  ConnectionTestResult,
  CopilotAuthStatus,
  GitHubDeviceCodeResponse,
  GitUsageBucket,
  GitUsageReport,
  LocalProxySettingsState,
  LocalProxyStatus,
  ManagedAuthAccount,
  GitUsageRepository,
  GitUsageTotals,
  LocalTokenUsageDay,
  LocalTokenUsageModel,
  LocalTokenUsageRange,
  LocalTokenUsageReport,
  LocalTokenUsageTotals,
  LocalTokenUsageTool,
  OAuthStatus,
  PresetUsageRange,
  PrKpiMetric,
  PrKpiReport,
  SaveLocalProxySettingsInput,
  SaveReverseProxySettingsInput,
  SaveSettingsInput,
  ReverseProxySettingsState,
  ReverseProxyStatus,
  UsageRangeSelection,
} from "./types";

const isTauriRuntime = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
const mockGitUsageRoot = "/Users/test/project";
const defaultMockClaudeProxyConfig: ClaudeProxyConfig = {
  listen_address: "127.0.0.1",
  listen_port: 16555,
  routes: [],
};
const defaultMockReverseProxyConfig = {
  enabled: false,
  default_openai_account_id: null,
  default_copilot_account_id: null,
} as const;
const PR_KPI_OUTPUT_RATIO_TOKEN_UNIT = 1_000;
const PR_KPI_CACHE_READ_TOKEN_DIVISOR = 10;
let mockOAuthSequence = 0;
let mockOAuthCompleteSequence = 0;
let mockPendingOAuthAccountId: string | null = null;

type OAuthProviderKey = "openai" | "anthropic";
type MockProviderKey = OAuthProviderKey | "kimi" | "glm" | "minimax" | "copilot";

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
    notify_on_low_quota: false,
    notify_on_reset: false,
    reset_notify_lead_minutes: 15,
    git_usage_root: mockGitUsageRoot,
    launch_at_login: false,
    claude_proxy: defaultMockClaudeProxyConfig,
    claude_proxy_profiles: {},
    reverse_proxy: defaultMockReverseProxyConfig,
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
    notify_on_low_quota: false,
    notify_on_reset: false,
    reset_notify_lead_minutes: 15,
    git_usage_root: mockGitUsageRoot,
    launch_at_login: false,
    claude_proxy: defaultMockClaudeProxyConfig,
    claude_proxy_profiles: {},
    reverse_proxy: defaultMockReverseProxyConfig,
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
    fetched_at: localIsoString(new Date()),
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
      fetched_at: localIsoString(new Date()),
      source: "probe_headers",
      last_error: null,
    },
  ],
  refresh_status: "ok",
  last_error: null,
  last_refreshed_at: localIsoString(new Date()),
};

let mockLocalProxySettingsState: LocalProxySettingsState = {
  config: defaultMockClaudeProxyConfig,
  capabilities: [],
};

let mockLocalProxyStatus: LocalProxyStatus = {
  running: false,
  address: "127.0.0.1",
  port: 16555,
  active_connections: 0,
  total_requests: 0,
  successful_requests: 0,
  failed_requests: 0,
  success_rate: 0,
  uptime_seconds: 0,
  last_error: null,
};

let mockReverseProxySettingsState: ReverseProxySettingsState = {
  enabled: false,
  copilot_accounts: [],
  openai_accounts: [],
  default_copilot_account_id: null,
  default_openai_account_id: null,
};

let mockReverseProxyStatus: ReverseProxyStatus = {
  enabled: false,
  copilot_ready: false,
  openai_ready: false,
  available_copilot_accounts: 0,
  available_openai_accounts: 0,
};

export function resetMockTauriStateForTests(): void {
  mockOAuthSequence = 0;
  mockOAuthCompleteSequence = 0;
  mockPendingOAuthAccountId = null;
  mockLocalProxySettingsState = {
    config: defaultMockClaudeProxyConfig,
    capabilities: [],
  };
  mockLocalProxyStatus = {
    running: false,
    address: "127.0.0.1",
    port: 16555,
    active_connections: 0,
    total_requests: 0,
    successful_requests: 0,
    failed_requests: 0,
    success_rate: 0,
    uptime_seconds: 0,
    last_error: null,
  };
  mockReverseProxySettingsState = {
    enabled: false,
    copilot_accounts: [],
    openai_accounts: [],
    default_copilot_account_id: null,
    default_openai_account_id: null,
  };
  mockReverseProxyStatus = {
    enabled: false,
    copilot_ready: false,
    openai_ready: false,
    available_copilot_accounts: 0,
    available_openai_accounts: 0,
  };
  mockSettings = emptyMockSettings();
  mockStatus = {
    snapshot: null,
    accounts: [],
    refresh_status: "idle",
    last_error: null,
    last_refreshed_at: null,
  };
  syncMockReverseProxyState();
  syncMockLocalProxyState();
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
      notify_on_reset: false,
      secret_configured: mockSettings.secret_configured || Boolean(authSecret),
    };
    syncMockLocalProxyState();
    return mockSettings;
  }
  return invoke("save_settings", { input });
}

export async function getLocalProxySettings(): Promise<LocalProxySettingsState> {
  if (!isTauriRuntime) {
    syncMockLocalProxyState();
    return mockLocalProxySettingsState;
  }
  return invoke("get_local_proxy_settings");
}

export async function saveLocalProxySettings(input: SaveLocalProxySettingsInput): Promise<LocalProxySettingsState> {
  if (!isTauriRuntime) {
    mockSettings = {
      ...mockSettings,
      claude_proxy: input.config,
    };
    syncMockLocalProxyState();
    return mockLocalProxySettingsState;
  }
  return invoke("save_local_proxy_settings", { input });
}

export async function saveClaudeProxyProfile(input: ClaudeProxyProfileInput): Promise<LocalProxySettingsState> {
  if (!isTauriRuntime) {
    const { account_id, api_key_or_token, ...profile } = input;
    mockSettings = {
      ...mockSettings,
      claude_proxy_profiles: {
        ...mockSettings.claude_proxy_profiles,
        [account_id]: {
          base_url: profile.base_url,
          api_format: profile.api_format,
          auth_field: profile.auth_field,
          secret_configured:
            mockSettings.claude_proxy_profiles[account_id]?.secret_configured || Boolean(api_key_or_token?.trim()),
        },
      },
    };
    syncMockLocalProxyState();
    return mockLocalProxySettingsState;
  }
  return invoke("save_claude_proxy_profile", { input });
}

export async function getLocalProxyStatus(): Promise<LocalProxyStatus> {
  if (!isTauriRuntime) {
    return mockLocalProxyStatus;
  }
  return invoke("get_local_proxy_status");
}

export async function startLocalProxy(): Promise<LocalProxyStatus> {
  if (!isTauriRuntime) {
    syncMockLocalProxyState();
    mockLocalProxyStatus = {
      ...mockLocalProxyStatus,
      running: true,
      address: mockSettings.claude_proxy.listen_address,
      port: mockSettings.claude_proxy.listen_port,
      uptime_seconds: 1,
      last_error: null,
    };
    return mockLocalProxyStatus;
  }
  const status = await invoke<LocalProxyStatus>("start_local_proxy");
  await syncTrayMenu();
  return status;
}

export async function stopLocalProxy(): Promise<LocalProxyStatus> {
  if (!isTauriRuntime) {
    mockLocalProxyStatus = {
      ...mockLocalProxyStatus,
      running: false,
      uptime_seconds: 0,
    };
    return mockLocalProxyStatus;
  }
  const status = await invoke<LocalProxyStatus>("stop_local_proxy");
  await syncTrayMenu();
  return status;
}

export async function getReverseProxySettings(): Promise<ReverseProxySettingsState> {
  if (!isTauriRuntime) {
    syncMockReverseProxyState();
    return mockReverseProxySettingsState;
  }
  return invoke("get_reverse_proxy_settings");
}

export async function saveReverseProxySettings(
  input: SaveReverseProxySettingsInput,
): Promise<ReverseProxySettingsState> {
  if (!isTauriRuntime) {
    mockSettings = {
      ...mockSettings,
      reverse_proxy: {
        enabled: input.enabled,
        default_openai_account_id: input.default_openai_account_id ?? mockSettings.reverse_proxy.default_openai_account_id,
        default_copilot_account_id:
          input.default_copilot_account_id ?? mockSettings.reverse_proxy.default_copilot_account_id,
      },
    };
    syncMockReverseProxyState();
    syncMockLocalProxyState();
    return mockReverseProxySettingsState;
  }
  const settings = await invoke<ReverseProxySettingsState>("save_reverse_proxy_settings", { input });
  await syncTrayMenu();
  return settings;
}

export async function getReverseProxyStatus(): Promise<ReverseProxyStatus> {
  if (!isTauriRuntime) {
    syncMockReverseProxyState();
    return mockReverseProxyStatus;
  }
  return invoke("get_reverse_proxy_status");
}

export async function copilotStartDeviceFlow(): Promise<GitHubDeviceCodeResponse> {
  if (!isTauriRuntime) {
    return {
      device_code: `preview-device-${Date.now()}`,
      user_code: "MOCK-CODE",
      verification_uri: "https://github.com/login/device",
      expires_in: 900,
      interval: 5,
    };
  }
  return invoke("copilot_start_device_flow");
}

export async function copilotPollForAccount(deviceCode: string): Promise<ManagedAuthAccount | null> {
  if (!isTauriRuntime) {
    if (!deviceCode) {
      return null;
    }
    const account: ManagedAuthAccount = {
      id: `copilot-${Date.now()}`,
      login: `github-${Date.now().toString(36)}`,
      avatar_url: null,
      authenticated_at: Math.floor(Date.now() / 1000),
      domain: "github.com",
    };
    mockReverseProxySettingsState = {
      ...mockReverseProxySettingsState,
      copilot_accounts: [account, ...mockReverseProxySettingsState.copilot_accounts],
      default_copilot_account_id:
        mockReverseProxySettingsState.default_copilot_account_id ?? account.id,
    };
    mockSettings = {
      ...mockSettings,
      reverse_proxy: {
        ...mockSettings.reverse_proxy,
        default_copilot_account_id:
          mockSettings.reverse_proxy.default_copilot_account_id ?? account.id,
      },
    };
    syncMockReverseProxyState();
    syncMockLocalProxyState();
    return account;
  }
  return invoke("copilot_poll_for_account", { deviceCode });
}

export async function copilotListAccounts(): Promise<ManagedAuthAccount[]> {
  if (!isTauriRuntime) {
    return mockReverseProxySettingsState.copilot_accounts;
  }
  return invoke("copilot_list_accounts");
}

export async function copilotSetDefaultAccount(accountId: string): Promise<void> {
  if (!isTauriRuntime) {
    mockSettings = {
      ...mockSettings,
      reverse_proxy: {
        ...mockSettings.reverse_proxy,
        default_copilot_account_id: accountId,
      },
    };
    syncMockReverseProxyState();
    syncMockLocalProxyState();
    return;
  }
  return invoke("copilot_set_default_account", { accountId });
}

export async function copilotRemoveAccount(accountId: string): Promise<void> {
  if (!isTauriRuntime) {
    const nextAccounts = mockReverseProxySettingsState.copilot_accounts.filter((account) => account.id !== accountId);
    const nextDefault =
      mockSettings.reverse_proxy.default_copilot_account_id === accountId ? nextAccounts[0]?.id ?? null : mockSettings.reverse_proxy.default_copilot_account_id;
    mockSettings = {
      ...mockSettings,
      reverse_proxy: {
        ...mockSettings.reverse_proxy,
        default_copilot_account_id: nextDefault,
      },
    };
    mockReverseProxySettingsState = {
      ...mockReverseProxySettingsState,
      copilot_accounts: nextAccounts,
      default_copilot_account_id: nextDefault,
    };
    syncMockReverseProxyState();
    syncMockLocalProxyState();
    return;
  }
  return invoke("copilot_remove_account", { accountId });
}

export async function copilotGetAuthStatus(): Promise<CopilotAuthStatus> {
  if (!isTauriRuntime) {
    return {
      accounts: mockReverseProxySettingsState.copilot_accounts,
      default_account_id: mockReverseProxySettingsState.default_copilot_account_id,
      authenticated: mockReverseProxySettingsState.copilot_accounts.length > 0,
    };
  }
  return invoke("copilot_get_auth_status");
}

export async function startCopilotOAuthDeviceFlow(): Promise<GitHubDeviceCodeResponse> {
  return copilotStartDeviceFlow();
}

export async function pollCopilotOAuthAccount(deviceCode: string): Promise<ManagedAuthAccount | null> {
  return copilotPollForAccount(deviceCode);
}

export async function ensureNotificationPermission(): Promise<boolean> {
  if (!isTauriRuntime) {
    return true;
  }

  const { isPermissionGranted, requestPermission } = await import("@tauri-apps/plugin-notification");
  if (await isPermissionGranted()) {
    return true;
  }

  return (await requestPermission()) === "granted";
}

export async function notificationPermissionGranted(): Promise<boolean> {
  if (!isTauriRuntime) {
    return false;
  }

  const { isPermissionGranted } = await import("@tauri-apps/plugin-notification");
  return isPermissionGranted();
}

export async function sendDesktopNotification(title: string, body: string): Promise<void> {
  if (!isTauriRuntime) {
    return;
  }

  const { sendNotification } = await import("@tauri-apps/plugin-notification");
  sendNotification({ title, body });
}

export async function checkForAppUpdate(): Promise<AppUpdateInfo | null> {
  if (!isTauriRuntime) {
    return null;
  }

  const { check } = await import("@tauri-apps/plugin-updater");
  const update = await check();
  if (!update) {
    return null;
  }

  return {
    version: update.version,
    currentVersion: update.currentVersion,
    body: typeof update.body === "string" && update.body.trim() ? update.body.trim() : null,
    date: typeof update.date === "string" && update.date.trim() ? update.date : null,
  };
}

export async function installAppUpdate(
  onEvent?: (event: AppUpdateDownloadEvent) => void,
): Promise<void> {
  if (!isTauriRuntime) {
    throw new Error("更新只在桌面应用中可用");
  }

  const { check } = await import("@tauri-apps/plugin-updater");
  const update = await check();
  if (!update) {
    throw new Error("当前已是最新版本");
  }

  await update.downloadAndInstall((event) => {
    onEvent?.(event as AppUpdateDownloadEvent);
  });
}

export async function relaunchApp(): Promise<void> {
  if (!isTauriRuntime) {
    return;
  }

  const { relaunch } = await import("@tauri-apps/plugin-process");
  await relaunch();
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
    syncMockLocalProxyState();
    return mockSettings;
  }
  return invoke("import_kimi_account", {
    accountName: normalizedName,
    accountId: accountId?.trim() || null,
  });
}

export async function importGlmAccount(
  accountName: string,
  apiKey: string,
  accountId?: string | null,
): Promise<AppSettings> {
  const normalizedName = accountName.trim() || "GLM Account";
  const normalizedApiKey = apiKey.trim();
  if (!normalizedApiKey) {
    throw new Error("请填写 GLM API Key");
  }
  if (!isTauriRuntime) {
    const nextAccountId = accountId?.trim() || uniqueMockAccountId("glm", normalizedName);
    const nextAccount = mockAccount(nextAccountId, normalizedName, "glm", "apiKey");
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
    syncMockLocalProxyState();
    return mockSettings;
  }
  return invoke("import_glm_account", {
    accountName: normalizedName,
    apiKey: normalizedApiKey,
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
    syncMockLocalProxyState();
    return mockSettings;
  }
  return invoke("import_minimax_account", {
    accountName: normalizedName,
    apiKey: normalizedApiKey,
    accountId: accountId?.trim() || null,
  });
}

export async function importCopilotAccount(
  accountName: string,
  githubToken?: string | null,
  accountId?: string | null,
): Promise<AppSettings> {
  const normalizedName = accountName.trim() || "Copilot Account";
  const normalizedToken = githubToken?.trim() || "";
  if (!isTauriRuntime && !normalizedToken) {
    throw new Error("请填写 GitHub Token，或先运行 gh auth login 后再导入");
  }
  if (!isTauriRuntime) {
    const nextAccountId = accountId?.trim() || uniqueMockAccountId("copilot", normalizedName);
    const nextAccount = mockAccount(nextAccountId, normalizedName, "copilot", "apiKey");
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
    syncMockLocalProxyState();
    return mockSettings;
  }
  return invoke("import_copilot_account", {
    accountName: normalizedName,
    githubToken: normalizedToken || null,
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
    syncMockLocalProxyState();
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
    syncMockLocalProxyState();
    return mockSettings;
  }
  return invoke("delete_connected_account", { accountId });
}

type UsageRangeInput = UsageRangeSelection | LocalTokenUsageRange;

const defaultUsageRangeSelection: UsageRangeSelection = { kind: "preset", range: "thisMonth" };

export async function getLocalTokenUsage(
  selection: UsageRangeInput = defaultUsageRangeSelection,
): Promise<LocalTokenUsageReport> {
  const request = normalizeUsageRangeSelection(selection);
  if (!isTauriRuntime) {
    return mockLocalTokenUsageReport(request);
  }
  return invoke("get_local_token_usage", { request });
}

export async function refreshLocalTokenUsage(
  selection: UsageRangeInput = defaultUsageRangeSelection,
): Promise<LocalTokenUsageReport> {
  const request = normalizeUsageRangeSelection(selection);
  if (!isTauriRuntime) {
    return mockLocalTokenUsageReport(request);
  }
  return invoke("refresh_local_token_usage", { request });
}

export async function getGitUsage(selection: UsageRangeInput = defaultUsageRangeSelection): Promise<GitUsageReport> {
  const request = normalizeUsageRangeSelection(selection);
  if (!isTauriRuntime) {
    return mockGitUsageReport(request);
  }
  return invoke("get_git_usage", { request });
}

export async function refreshGitUsage(selection: UsageRangeInput = defaultUsageRangeSelection): Promise<GitUsageReport> {
  const request = normalizeUsageRangeSelection(selection);
  if (!isTauriRuntime) {
    return mockGitUsageReport(request);
  }
  return invoke("refresh_git_usage", { request });
}

export async function getPrKpi(selection: UsageRangeInput = defaultUsageRangeSelection): Promise<PrKpiReport> {
  const request = normalizeUsageRangeSelection(selection);
  if (!isTauriRuntime) {
    return mockPrKpiReport(request);
  }
  return invoke("get_pr_kpi", { request });
}

export async function refreshPrKpi(selection: UsageRangeInput = defaultUsageRangeSelection): Promise<PrKpiReport> {
  const request = normalizeUsageRangeSelection(selection);
  if (!isTauriRuntime) {
    return mockPrKpiReport(request);
  }
  return invoke("refresh_pr_kpi", { request });
}

export async function chooseGitUsageRoot(currentPath?: string | null): Promise<string | null> {
  if (!isTauriRuntime) {
    return currentPath || mockGitUsageRoot;
  }

  const selected = await open({
    directory: true,
    multiple: false,
    defaultPath: currentPath || undefined,
  });

  return typeof selected === "string" ? selected : null;
}

function syncMockLocalProxyState(): void {
  syncMockReverseProxyState();
  const capabilities: ClaudeProxyCapability[] = mockSettings.accounts.map((account) => {
    const compatibleProviders = new Set(["anthropic", "glm", "minimax", "kimi", "qwen", "xiaomi", "custom"]);
    const isCompatible = compatibleProviders.has(account.provider);
    const defaults = mockDefaultClaudeProfile(account.provider);
    const stored = mockSettings.claude_proxy_profiles[account.account_id];
    const profile: ClaudeProxyProfileSummary = {
      base_url: stored?.base_url ?? defaults.base_url,
      api_format: stored?.api_format ?? defaults.api_format,
      auth_field: stored?.auth_field ?? defaults.auth_field,
      secret_configured:
        stored?.secret_configured ?? (account.provider === "glm" || account.provider === "minimax"),
    };
    const missing_fields = isCompatible
      ? [
          ...(profile.base_url ? [] : ["base_url"]),
          ...(profile.secret_configured ? [] : ["api_key_or_token"]),
        ]
      : [];
    return {
      account_id: account.account_id,
      kind: "direct_account",
      provider: account.provider,
      display_name: account.account_name,
      is_claude_compatible_provider: isCompatible,
      can_direct_connect: isCompatible && missing_fields.length === 0,
      missing_fields,
      status: !isCompatible ? "unsupported" : missing_fields.length === 0 ? "direct_ready" : "needs_profile",
      profile,
      resolved_profile: isCompatible && missing_fields.length === 0 ? profile : null,
    } satisfies ClaudeProxyCapability;
  });

  capabilities.push({
    account_id: "reverse:copilot",
    kind: "reverse_copilot",
    provider: "copilot",
    display_name: "GitHub Copilot",
    is_claude_compatible_provider: true,
    can_direct_connect: mockReverseProxyStatus.copilot_ready,
    missing_fields: mockReverseProxyStatus.copilot_ready ? [] : ["reverse_proxy"],
    status: mockReverseProxyStatus.copilot_ready ? "reverse_ready" : "reverse_pending",
    profile: {
      base_url: null,
      api_format: "anthropic",
      auth_field: "ANTHROPIC_AUTH_TOKEN",
      secret_configured: mockReverseProxyStatus.copilot_ready,
    },
    resolved_profile: null,
  });
  capabilities.push({
    account_id: "reverse:openai",
    kind: "reverse_openai",
    provider: "openai",
    display_name: "ChatGPT (Codex OAuth)",
    is_claude_compatible_provider: true,
    can_direct_connect: mockReverseProxyStatus.openai_ready,
    missing_fields: mockReverseProxyStatus.openai_ready ? [] : ["reverse_proxy"],
    status: mockReverseProxyStatus.openai_ready ? "reverse_ready" : "reverse_pending",
    profile: {
      base_url: null,
      api_format: "anthropic",
      auth_field: "ANTHROPIC_AUTH_TOKEN",
      secret_configured: mockReverseProxyStatus.openai_ready,
    },
    resolved_profile: null,
  });

  mockLocalProxySettingsState = {
    config: mockSettings.claude_proxy,
    capabilities,
  };
}

function syncMockReverseProxyState(): void {
  const openaiAccounts = mockSettings.accounts
    .filter((account) => account.provider === "openai" && account.auth_mode === "oauth")
    .map((account) => ({
      id: account.account_id,
      login: account.account_name,
      avatar_url: null,
      authenticated_at: 0,
      domain: null,
    }));

  mockReverseProxySettingsState = {
    enabled: mockSettings.reverse_proxy.enabled,
    copilot_accounts: mockReverseProxySettingsState.copilot_accounts,
    openai_accounts: openaiAccounts,
    default_copilot_account_id: mockSettings.reverse_proxy.default_copilot_account_id,
    default_openai_account_id: mockSettings.reverse_proxy.default_openai_account_id,
  };

  mockReverseProxyStatus = {
    enabled: mockSettings.reverse_proxy.enabled,
    copilot_ready:
      mockSettings.reverse_proxy.enabled &&
      Boolean(
        mockReverseProxySettingsState.default_copilot_account_id &&
          mockReverseProxySettingsState.copilot_accounts.some(
            (account) => account.id === mockReverseProxySettingsState.default_copilot_account_id,
          ),
      ),
    openai_ready:
      mockSettings.reverse_proxy.enabled &&
      Boolean(
        mockSettings.reverse_proxy.default_openai_account_id &&
          openaiAccounts.some((account) => account.id === mockSettings.reverse_proxy.default_openai_account_id),
      ),
    available_copilot_accounts: mockReverseProxySettingsState.copilot_accounts.length,
    available_openai_accounts: openaiAccounts.length,
  };
}

function mockDefaultClaudeProfile(provider: string): ClaudeProxyProfileSummary {
  const base_url =
    provider === "anthropic"
      ? "https://api.anthropic.com"
      : provider === "glm"
        ? "https://open.bigmodel.cn/api/anthropic"
        : provider === "minimax"
          ? "https://api.minimaxi.com/anthropic"
          : null;
  return {
    base_url,
    api_format: "anthropic",
    auth_field: "ANTHROPIC_AUTH_TOKEN",
    secret_configured: false,
  };
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
      last_error: null,
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

function normalizeUsageRangeSelection(selection: UsageRangeInput): UsageRangeSelection {
  if (typeof selection !== "string") {
    return selection;
  }
  if (selection === "custom") {
    return defaultMockCustomRange();
  }
  return { kind: "preset", range: selection as PresetUsageRange };
}

function reportRangeFromSelection(selection: UsageRangeSelection): LocalTokenUsageRange {
  return selection.kind === "custom" ? "custom" : selection.range;
}

function reportDateFields(
  selection: UsageRangeSelection,
): Pick<LocalTokenUsageReport, "start_date" | "end_date"> {
  if (selection.kind !== "custom") {
    return {};
  }
  return {
    start_date: selection.startDate,
    end_date: selection.endDate,
  };
}

function defaultMockCustomRange(): UsageRangeSelection {
  const end = startOfLocalDay(new Date());
  const start = addLocalDays(end, -7);
  return {
    kind: "custom",
    startDate: localDateKey(start),
    endDate: localDateKey(end),
  };
}

function mockLocalTokenUsageReport(selection: UsageRangeSelection): LocalTokenUsageReport {
  const range = reportRangeFromSelection(selection);
  const generatedAt = new Date();
  const bucketDates = mockTokenBucketDates(selection, generatedAt);
  const days = bucketDates.map((date, index) => {
    const models = mockTokenBucketModels(index);
    const totals = sumTokenStats(models);
    return {
      date: mockTokenBucketKey(range, date),
      input_tokens: totals.input_tokens,
      output_tokens: totals.output_tokens,
      cache_read_tokens: totals.cache_read_tokens,
      cache_creation_tokens: totals.cache_creation_tokens,
      total_tokens: totals.total_tokens,
      models,
    };
  });
  const totals = withCacheHitRate(sumTokenStats(days));
  const models = aggregateModelStats(days);

  return {
    range,
    ...reportDateFields(selection),
    totals,
    days,
    models,
    tools: mockTokenUsageTools(totals),
    missing_sources: ["OpenCode: ~/.local/share/opencode/storage/message"],
    warnings: [],
    generated_at: localIsoString(generatedAt),
  };
}

function mockGitUsageReport(selection: UsageRangeSelection): GitUsageReport {
  const range = reportRangeFromSelection(selection);
  const generatedAt = new Date();
  const bucketDates = mockTokenBucketDates(selection, generatedAt);
  const buckets = bucketDates.map((date, index) => {
    const stats = mockGitBucketStats(index);
    return {
      date: mockTokenBucketKey(range, date),
      ...stats,
    };
  });
  const totals = sumGitStats(buckets);
  const repositories = mockGitRepositories(totals);

  return {
    range,
    ...reportDateFields(selection),
    totals,
    buckets,
    repositories,
    repository_count: 8,
    missing_sources: [],
    warnings: [],
    generated_at: localIsoString(generatedAt),
  };
}

function mockPrKpiReport(selection: UsageRangeSelection): PrKpiReport {
  const range = reportRangeFromSelection(selection);
  const generatedAt = localIsoString(new Date());
  const tokenReport = mockLocalTokenUsageReport(selection);
  const gitReport = mockGitUsageReport(selection);
  const netLines = gitReport.totals.added_lines - gitReport.totals.deleted_lines;
  const effectiveTokenTotal = effectivePrKpiTokenTotal(tokenReport.totals);
  const outputRatio =
    effectiveTokenTotal > 0
      ? netLines / (effectiveTokenTotal / PR_KPI_OUTPUT_RATIO_TOKEN_UNIT)
      : null;
  const rangeDays =
    selection.kind === "custom"
      ? Math.max(1, daysBetweenInclusive(selection.startDate, selection.endDate))
      : range === "today"
        ? 1
        : range === "thisWeek"
          ? 7
          : 30;
  const mergedPerWeek = Math.max(0.4, roundOneDecimal(gitReport.repositories.length * 7 / rangeDays));
  const metrics: PrKpiMetric[] = [
    metricRow("cycle_time_ai", "合入周期", 18, "18h", 82),
    metricRow("merged_ai_prs_per_week", "合入频率", mergedPerWeek, `${mergedPerWeek.toFixed(1)} / 周`, 74),
    metricRow("review_comments_per_pr", "评审负担", 4.1, "4.1 / PR", 68),
    metricRow("test_added_ratio", "测试占比", 0.18, "18%", 51),
    metricRow("7d_rework_rate", "返工控制", 0.08, "8%", 91),
    metricRow("7d_retention_rate", "代码保留", 0.92, "92%", 91),
  ];

  return {
    range,
    ...reportDateFields(selection),
    overview: {
      token_total: effectiveTokenTotal,
      code_lines: gitReport.totals.added_lines + gitReport.totals.deleted_lines,
      output_ratio: outputRatio,
    },
    metrics,
    overall_score: metrics.reduce((sum, metric) => sum + (metric.score ?? 0), 0) / metrics.length,
    missing_sources: [],
    warnings: [],
    generated_at: generatedAt,
  };
}

function metricRow(
  key: PrKpiMetric["key"],
  label: string,
  rawValue: number | null,
  displayValue: string,
  score: number | null,
): PrKpiMetric {
  return {
    key,
    label,
    score,
    raw_value: rawValue,
    display_value: displayValue,
    is_missing: rawValue == null,
  };
}

function mockGitRepositories(totals: GitUsageTotals): GitUsageRepository[] {
  if (totals.added_lines + totals.deleted_lines + totals.changed_files === 0) {
    return [];
  }

  const rows = [
    { name: "ai-usage", path: "/Users/local/project/ai-usage", addedRatio: 0.48, deletedRatio: 0.46, filesRatio: 0.38 },
    { name: "backend-api", path: "/Users/local/project/backend-api", addedRatio: 0.32, deletedRatio: 0.31, filesRatio: 0.34 },
    { name: "docs-site", path: "/Users/local/project/docs-site", addedRatio: 0.18, deletedRatio: 0.19, filesRatio: 0.2 },
  ];

  return rows.map((row) => ({
    name: row.name,
    path: row.path,
    added_lines: Math.max(1, Math.round(totals.added_lines * row.addedRatio)),
    deleted_lines: Math.max(1, Math.round(totals.deleted_lines * row.deletedRatio)),
    changed_files: Math.max(1, Math.round(totals.changed_files * row.filesRatio)),
  }));
}

function mockTokenBucketDates(selection: UsageRangeSelection, now: Date): Date[] {
  const starts: Date[] = [];
  const start = startOfLocalDay(now);
  let cursor: Date;
  let end: Date;
  let stepHours = 24;
  const range = reportRangeFromSelection(selection);

  if (selection.kind === "custom") {
    cursor = parseLocalDateKey(selection.startDate);
    end = parseLocalDateKey(selection.endDate);
  } else if (range === "thisMonth") {
    cursor = startOfLocalMonth(now);
    end = endOfLocalMonth(now);
  } else if (range === "thisWeek") {
    cursor = startOfLocalWeek(now);
    end = addLocalDays(cursor, 6);
  } else if (range === "last3Days") {
    cursor = addLocalDays(start, -2);
    end = floorLocalHour(now, 3);
    stepHours = 3;
  } else {
    cursor = start;
    end = addLocalHours(start, 23);
    stepHours = 1;
  }

  while (cursor.getTime() <= end.getTime()) {
    starts.push(new Date(cursor));
    cursor = addLocalHours(cursor, stepHours);
  }

  return starts;
}

function mockTokenBucketKey(range: LocalTokenUsageRange, date: Date): string {
  if (range === "today" || range === "last3Days") {
    return localIsoHourKey(date);
  }
  return localDateKey(date);
}

function mockTokenBucketModels(index: number): LocalTokenUsageModel[] {
  return [
    mockTokenModel("gpt-5.3-codex", index, 14_000, 4_200, 9_000, 0),
    mockTokenModel("claude-sonnet-4-5", index, 11_000, 3_300, 6_400, 2_600),
    mockTokenModel("kimi-cli", index, 6_400, 1_700, 2_800, 500),
    mockTokenModel("opencode/claude-3.5", index, 4_200, 1_200, 2_100, 700),
  ];
}

function mockTokenModel(
  model: string,
  index: number,
  inputBase: number,
  outputBase: number,
  cacheReadBase: number,
  cacheCreationBase: number,
): LocalTokenUsageModel {
  const wave = 1 + (index % 6) * 0.08 + Math.floor(index / 6) * 0.05;
  const input = Math.round(inputBase * wave);
  const output = Math.round(outputBase * wave);
  const cacheRead = Math.round(cacheReadBase * wave);
  const cacheCreation = Math.round(cacheCreationBase * (index % 3 === 0 ? 1.35 : wave));
  return {
    model,
    input_tokens: input,
    output_tokens: output,
    cache_read_tokens: cacheRead,
    cache_creation_tokens: cacheCreation,
    total_tokens: input + output + cacheRead + cacheCreation,
  };
}

function aggregateModelStats(days: LocalTokenUsageDay[]): LocalTokenUsageModel[] {
  const byModel = new Map<string, LocalTokenUsageModel>();
  for (const day of days) {
    for (const model of day.models) {
      const current = byModel.get(model.model);
      if (!current) {
        byModel.set(model.model, { ...model });
        continue;
      }
      current.input_tokens += model.input_tokens;
      current.output_tokens += model.output_tokens;
      current.cache_read_tokens += model.cache_read_tokens;
      current.cache_creation_tokens += model.cache_creation_tokens;
      current.total_tokens += model.total_tokens;
    }
  }
  return [...byModel.values()].sort((a, b) => b.total_tokens - a.total_tokens || a.model.localeCompare(b.model));
}

function sumTokenStats(items: Array<Omit<LocalTokenUsageTotals, "cache_hit_rate_percent">>): Omit<LocalTokenUsageTotals, "cache_hit_rate_percent"> {
  return items.reduce(
    (acc, item) => ({
      input_tokens: acc.input_tokens + item.input_tokens,
      output_tokens: acc.output_tokens + item.output_tokens,
      cache_read_tokens: acc.cache_read_tokens + item.cache_read_tokens,
      cache_creation_tokens: acc.cache_creation_tokens + item.cache_creation_tokens,
      total_tokens: acc.total_tokens + item.total_tokens,
    }),
    {
      input_tokens: 0,
      output_tokens: 0,
      cache_read_tokens: 0,
      cache_creation_tokens: 0,
      total_tokens: 0,
    },
  );
}

function effectivePrKpiTokenTotal(totals: LocalTokenUsageTotals): number {
  return (
    totals.input_tokens +
    totals.output_tokens +
    totals.cache_creation_tokens +
    Math.floor(totals.cache_read_tokens / PR_KPI_CACHE_READ_TOKEN_DIVISOR)
  );
}

function withCacheHitRate(totals: Omit<LocalTokenUsageTotals, "cache_hit_rate_percent">): LocalTokenUsageTotals {
  return {
    ...totals,
    cache_hit_rate_percent:
      totals.input_tokens + totals.cache_read_tokens === 0
        ? 0
        : (totals.cache_read_tokens / (totals.input_tokens + totals.cache_read_tokens)) * 100,
  };
}

function mockTokenUsageTools(totals: LocalTokenUsageTotals): LocalTokenUsageTool[] {
  return [
      {
        tool: "codex",
        input_tokens: Math.round(totals.input_tokens * 0.42),
        output_tokens: Math.round(totals.output_tokens * 0.43),
        cache_read_tokens: Math.round(totals.cache_read_tokens * 0.48),
        cache_creation_tokens: 0,
        total_tokens: Math.round(totals.total_tokens * 0.42),
      },
      {
        tool: "claude",
        input_tokens: Math.round(totals.input_tokens * 0.34),
        output_tokens: Math.round(totals.output_tokens * 0.32),
        cache_read_tokens: Math.round(totals.cache_read_tokens * 0.31),
        cache_creation_tokens: Math.round(totals.cache_creation_tokens * 0.72),
        total_tokens: Math.round(totals.total_tokens * 0.34),
      },
      {
        tool: "kimi",
        input_tokens: Math.round(totals.input_tokens * 0.14),
        output_tokens: Math.round(totals.output_tokens * 0.15),
        cache_read_tokens: Math.round(totals.cache_read_tokens * 0.11),
        cache_creation_tokens: Math.round(totals.cache_creation_tokens * 0.1),
        total_tokens: Math.round(totals.total_tokens * 0.13),
      },
      {
        tool: "opencode",
        input_tokens: Math.round(totals.input_tokens * 0.1),
        output_tokens: Math.round(totals.output_tokens * 0.1),
        cache_read_tokens: Math.round(totals.cache_read_tokens * 0.1),
        cache_creation_tokens: Math.round(totals.cache_creation_tokens * 0.18),
        total_tokens: Math.round(totals.total_tokens * 0.11),
      },
    ];
}

function mockGitBucketStats(index: number): GitUsageTotals {
  const wave = 1 + (index % 7) * 0.16 + Math.floor(index / 7) * 0.04;
  const quiet = index % 9 === 3 ? 0.18 : 1;
  const added = Math.round((160 + (index % 4) * 54) * wave * quiet);
  const deleted = Math.round((42 + (index % 5) * 18) * wave * quiet);
  const changed = Math.max(1, Math.round((3 + (index % 6)) * quiet));
  return {
    added_lines: added,
    deleted_lines: deleted,
    changed_files: changed,
  };
}

function sumGitStats(items: GitUsageBucket[]): GitUsageTotals {
  return items.reduce(
    (acc, item) => ({
      added_lines: acc.added_lines + item.added_lines,
      deleted_lines: acc.deleted_lines + item.deleted_lines,
      changed_files: acc.changed_files + item.changed_files,
    }),
    {
      added_lines: 0,
      deleted_lines: 0,
      changed_files: 0,
    },
  );
}

function startOfLocalDay(date: Date): Date {
  const next = new Date(date);
  next.setHours(0, 0, 0, 0);
  return next;
}

function startOfLocalMonth(date: Date): Date {
  const next = startOfLocalDay(date);
  next.setDate(1);
  return next;
}

function endOfLocalMonth(date: Date): Date {
  const next = startOfLocalMonth(date);
  next.setMonth(next.getMonth() + 1);
  next.setDate(0);
  return next;
}

function startOfLocalWeek(date: Date): Date {
  const next = startOfLocalDay(date);
  const offset = (next.getDay() + 6) % 7;
  next.setDate(next.getDate() - offset);
  return next;
}

function floorLocalHour(date: Date, stepHours: number): Date {
  const next = new Date(date);
  next.setMinutes(0, 0, 0);
  next.setHours(next.getHours() - (next.getHours() % stepHours));
  return next;
}

function addLocalDays(date: Date, days: number): Date {
  const next = new Date(date);
  next.setDate(next.getDate() + days);
  return next;
}

function addLocalHours(date: Date, hours: number): Date {
  const next = new Date(date);
  next.setHours(next.getHours() + hours);
  return next;
}

function daysBetweenInclusive(startDate: string, endDate: string): number {
  const start = parseLocalDateKey(startDate);
  const end = parseLocalDateKey(endDate);
  const ms = startOfLocalDay(end).getTime() - startOfLocalDay(start).getTime();
  return Math.floor(ms / (24 * 60 * 60 * 1000)) + 1;
}

function parseLocalDateKey(value: string): Date {
  const [year, month, day] = value.split("-").map((part) => Number.parseInt(part, 10));
  if (!year || !month || !day) {
    return startOfLocalDay(new Date());
  }
  return new Date(year, month - 1, day);
}

function localDateKey(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}

function localIsoHourKey(date: Date): string {
  return `${localDateKey(date)}T${String(date.getHours()).padStart(2, "0")}:00:00${localOffsetSuffix(date)}`;
}

function localIsoString(date: Date): string {
  return `${localDateKey(date)}T${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}:${String(date.getSeconds()).padStart(2, "0")}${localOffsetSuffix(date)}`;
}

function localOffsetSuffix(date: Date): string {
  const totalMinutes = -date.getTimezoneOffset();
  const sign = totalMinutes >= 0 ? "+" : "-";
  const absoluteMinutes = Math.abs(totalMinutes);
  const hours = Math.floor(absoluteMinutes / 60);
  const minutes = absoluteMinutes % 60;
  return `${sign}${String(hours).padStart(2, "0")}:${String(minutes).padStart(2, "0")}`;
}

function roundOneDecimal(value: number): number {
  return Math.round(value * 10) / 10;
}

async function syncTrayMenu(): Promise<void> {
  if (!isTauriRuntime) {
    return;
  }
  return invoke("sync_tray_menu");
}
