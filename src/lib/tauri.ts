import { invoke } from "@tauri-apps/api/core";
import type {
  AccountQuotaStatus,
  AppSettings,
  AppStatus,
  ConnectedAccount,
  ConnectionTestResult,
  LocalTokenUsageDay,
  LocalTokenUsageModel,
  LocalTokenUsageRange,
  LocalTokenUsageReport,
  LocalTokenUsageTotals,
  LocalTokenUsageTool,
  OAuthStatus,
  SaveSettingsInput,
} from "./types";

const isTauriRuntime = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
let mockOAuthSequence = 0;
let mockOAuthCompleteSequence = 0;
let mockPendingOAuthAccountId: string | null = null;

type OAuthProviderKey = "openai" | "anthropic";
type MockProviderKey = OAuthProviderKey | "kimi" | "glm" | "minimax";

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
      last_error: null,
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
      notify_on_reset: false,
      secret_configured: mockSettings.secret_configured || Boolean(authSecret),
    };
    return mockSettings;
  }
  return invoke("save_settings", { input });
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

export async function getLocalTokenUsage(range: LocalTokenUsageRange = "thisMonth"): Promise<LocalTokenUsageReport> {
  if (!isTauriRuntime) {
    return mockLocalTokenUsageReport(range);
  }
  return invoke("get_local_token_usage", { range });
}

export async function refreshLocalTokenUsage(range: LocalTokenUsageRange = "thisMonth"): Promise<LocalTokenUsageReport> {
  if (!isTauriRuntime) {
    return mockLocalTokenUsageReport(range);
  }
  return invoke("refresh_local_token_usage", { range });
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

function mockLocalTokenUsageReport(range: LocalTokenUsageRange): LocalTokenUsageReport {
  const generatedAt = new Date();
  const bucketDates = mockTokenBucketDates(range, generatedAt);
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
    totals,
    days,
    models,
    tools: mockTokenUsageTools(totals),
    missing_sources: ["OpenCode: ~/.local/share/opencode/storage/message"],
    warnings: [],
    generated_at: generatedAt.toISOString(),
  };
}

function mockTokenBucketDates(range: LocalTokenUsageRange, now: Date): Date[] {
  const starts: Date[] = [];
  const start = startOfLocalDay(now);
  let cursor: Date;
  let end: Date;
  let stepHours = 24;

  if (range === "thisMonth") {
    cursor = startOfLocalMonth(now);
    end = start;
  } else if (range === "thisWeek") {
    cursor = startOfLocalWeek(now);
    end = start;
  } else if (range === "last3Days") {
    cursor = addLocalDays(start, -2);
    end = floorLocalHour(now, 3);
    stepHours = 3;
  } else {
    cursor = start;
    end = floorLocalHour(now, 1);
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
    return `${localDateKey(date)}T${String(date.getHours()).padStart(2, "0")}:00:00Z`;
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

function localDateKey(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
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
