import { listen } from "@tauri-apps/api/event";
import { ArrowLeft, Copy, FileText, FolderOpen, Inbox, Info, KeyRound, Link2, Pencil, Plus, RefreshCw, Settings, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import anthropicIcon from "../icons/extracted/anthropic.svg";
import githubCopilotIcon from "../icons/extracted/githubcopilot.svg";
import kimiIcon from "../icons/extracted/kimi.svg";
import minimaxIcon from "../icons/extracted/minimax.svg";
import openaiIcon from "../icons/extracted/openai.svg";
import openrouterIcon from "../icons/extracted/openrouter.svg";
import qwenIcon from "../icons/extracted/qwen.svg";
import zhipuIcon from "../icons/extracted/zhipu.svg";
import aiUsageLogo from "../icons/ai-usage-logo.svg";
import {
  checkForAppUpdate,
  copilotGetAuthStatus,
  copilotListAccounts,
  copilotRemoveAccount,
  copilotSetDefaultAccount,
  deleteConnectedAccount,
  ensureNotificationPermission,
  chooseGitUsageRoot,
  getCurrentQuota,
  installAppUpdate,
  getLocalProxySettings,
  getLocalProxyStatus,
  getLocalTokenUsage,
  getReverseProxySettings,
  getReverseProxyStatus,
  getSettings,
  completeAnthropicOAuth,
  completeOpenAIOAuth,
  getGitUsage,
  getPrKpi,
  importCopilotAccount,
  importGlmAccount,
  importKimiAccount,
  importMiniMaxAccount,
  refreshPrKpi,
  refreshQuota,
  refreshGitUsage,
  refreshLocalTokenUsage,
  relaunchApp,
  saveClaudeProxyProfile,
  saveLocalProxySettings,
  saveReverseProxySettings,
  saveSettings,
  sendDesktopNotification,
  startCopilotOAuthDeviceFlow,
  startLocalProxy,
  startAnthropicOAuth,
  stopLocalProxy,
  notificationPermissionGranted,
  pollCopilotOAuthAccount,
  startOpenAIOAuth,
} from "./lib/tauri";
import {
  connectedAccountSubtitle,
  connectedAccounts,
  hasConnectedAccount,
  isManagedCopilotAccountId,
} from "./lib/connected-accounts";
import { copyCopilotDeviceValue } from "./lib/copilot-device-copy";
import {
  startCopilotDevicePolling,
} from "./lib/copilot-device-polling";
import {
  hasGeneratedOAuthAuthLink,
  shouldApplyOAuthStartResult,
  shouldResetOAuthAuthDraft,
} from "./lib/oauth-auth-state";
import { quotaAccountCardState, quotaDisplayRows, remainingQuotaProgressValue } from "./lib/quota-display";
import {
  buildGitUsageChartRows,
  formatCompactLines,
  gitUsageSummaryMetrics,
  repositoryUsageRows,
} from "./lib/git-usage-display";
import {
  buildTokenUsageChartLegend,
  buildTokenUsageChartRows,
  formatCompactTokens,
  modelUsageRows,
  tokenUsageRangeLabels,
  usageToolLabel,
} from "./lib/token-usage-display";
import {
  buildPrKpiRadarModel,
  formatPrKpiOutputRatio,
  formatPrKpiOverviewValue,
  prKpiAxisAnchor,
  prKpiMetricDescriptions,
  prKpiOutputRatioTone,
} from "./lib/pr-kpi-display";
import {
  applyCustomRangeDraft,
  createUsageRangeUiState,
  customUsageWindowBounds,
  resolveVisibleReportState,
  selectUsageRangeOption,
  updateCustomRangeDraft,
  usageRangeSelectionKey,
  validateCustomUsageRangeSelection,
} from "./lib/usage-range";
import type { UsageRangeOption } from "./lib/usage-range";
import type {
  AccountQuotaStatus,
  AppUpdateInfo,
  AppSettings,
  AppStatus,
  ClaudeApiFormat,
  ClaudeAuthField,
  ClaudeProxyCapability,
  ConnectedAccount,
  CopilotAuthStatus,
  GitUsageReport,
  GitHubDeviceCodeResponse,
  LocalProxySettingsState,
  LocalProxyStatus,
  LocalTokenUsageReport,
  LocalTokenUsageTotals,
  ManagedAuthAccount,
  PrKpiReport,
  QuotaWindow,
  ReverseProxySettingsState,
  ReverseProxyStatus,
  SaveReverseProxySettingsInput,
  SaveSettingsInput,
  UsageRangeSelection,
} from "./lib/types";

type PanelView =
  | "overview"
  | "settings"
  | "add-account"
  | "oauth-auth"
  | "kimi-auth"
  | "glm-auth"
  | "minimax-auth"
  | "copilot-auth";
type AddAccountBackView = Extract<PanelView, "overview" | "settings">;
type OAuthProviderKey = "openai" | "anthropic";
type SettingsTab = "quota" | "tokens" | "proxy";
type SettingsUsageTab = "token" | "git" | "kpi";
type ProxySubTab = "local" | "reverse";
type ReverseManagerKind = "copilot" | "openai";
type Tone = "success" | "warning" | "danger" | "muted";
const isTauriRuntime = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
const UPDATE_CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

const emptyStatus: AppStatus = {
  snapshot: null,
  accounts: [],
  refresh_status: "idle",
  last_error: null,
  last_refreshed_at: null,
};

function quotaTone(value: number): Tone {
  if (value <= 10) {
    return "danger";
  }
  if (value <= 50) {
    return "warning";
  }
  return "success";
}

function formatPercent(window: QuotaWindow | null): string {
  if (!window) {
    return "--";
  }
  return `${Math.round(window.remaining_percent)}%`;
}

function errorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === "string" && error.trim()) {
    return error;
  }
  return fallback;
}

function formatUpdatePublishedAt(date: string | null): string | null {
  if (!date) {
    return null;
  }
  const parsed = new Date(date);
  if (Number.isNaN(parsed.getTime())) {
    return null;
  }
  return new Intl.DateTimeFormat("zh-CN", {
    month: "numeric",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  }).format(parsed);
}

function quotaAccounts(settings: AppSettings, status: AppStatus): AccountQuotaStatus[] {
  if (status.accounts.length > 0) {
    return status.accounts;
  }
  return connectedAccounts(settings, status).map((account) => {
    const snapshot = status.snapshot?.account_id === account.account_id ? status.snapshot : null;
    return {
      account_id: account.account_id,
      account_name: connectedAccountSubtitle(account),
      provider: account.provider,
      five_hour: snapshot?.five_hour ?? null,
      seven_day: snapshot?.seven_day ?? null,
      fetched_at: snapshot?.fetched_at ?? null,
      source: snapshot?.source ?? null,
      last_error: null,
    };
  });
}

function quotaAccountSubtitle(account: AccountQuotaStatus): string {
  return account.account_name.trim() || defaultProviderAccountName(account.provider);
}

function defaultProviderAccountName(provider: string): string {
  if (provider === "anthropic") {
    return "Anthropic Account";
  }
  if (provider === "kimi") {
    return "Kimi Account";
  }
  if (provider === "glm") {
    return "GLM Account";
  }
  if (provider === "minimax") {
    return "MiniMax Account";
  }
  if (provider === "copilot") {
    return "Copilot Account";
  }
  if (provider === "qwen") {
    return "Qwen Account";
  }
  if (provider === "xiaomi") {
    return "XiaoMi Account";
  }
  if (provider === "custom") {
    return "Custom Account";
  }
  return "OpenAI Account";
}

function providerDisplayName(provider: string): string {
  if (provider === "anthropic") {
    return "Anthropic";
  }
  if (provider === "kimi") {
    return "Kimi";
  }
  if (provider === "glm") {
    return "GLM";
  }
  if (provider === "minimax") {
    return "MiniMax";
  }
  if (provider === "copilot") {
    return "Copilot";
  }
  if (provider === "qwen") {
    return "Qwen";
  }
  if (provider === "xiaomi") {
    return "XiaoMi";
  }
  if (provider === "custom") {
    return "Custom";
  }
  return "OpenAI";
}

function providerIconConfig(provider: string): { icon: string; iconMode?: ProviderIconMode } {
  const match = providers.find((item) => item.key === provider);
  return { icon: match?.icon ?? openaiIcon, iconMode: match?.iconMode };
}

function isOAuthProvider(provider: ProviderKey): provider is OAuthProviderKey {
  return provider === "openai" || provider === "anthropic";
}

function proxyCapabilityStatus(capability: ClaudeProxyCapability): {
  label: string;
  tone: "ready" | "pending" | "unsupported";
} {
  switch (capability.status) {
    case "direct_ready":
      return { label: "已接入", tone: "ready" };
    case "needs_profile":
      return { label: "待补充", tone: "pending" };
    case "reverse_ready":
      return { label: "反代接入", tone: "ready" };
    case "reverse_pending":
      return { label: "待接入", tone: "pending" };
    default:
      return { label: "当前不支持", tone: "unsupported" };
  }
}

function proxyCapabilityHint(capability: ClaudeProxyCapability): string {
  if (capability.status === "unsupported") {
    return "当前账号类型暂不支持 Claude 调用模式";
  }
  if (capability.status === "reverse_pending") {
    return "开启反向代理并完成认证后可接入";
  }
  if (capability.missing_fields.length === 0) {
    return "当前账号已具备 Claude 代理所需信息";
  }
  return `缺少：${capability.missing_fields
    .map((field) => (field === "api_key_or_token" ? "API Key" : field === "base_url" ? "BASE_URL" : field))
    .join("、")}`;
}

function localProxyUrl(status: LocalProxyStatus | null, settingsState: LocalProxySettingsState | null): string {
  const address =
    status?.running && status.address ? status.address : settingsState?.config.listen_address || "127.0.0.1";
  const port = status?.running && status.port ? status.port : settingsState?.config.listen_port || 16555;
  return `http://${address}:${port}`;
}

function formatProxyUptime(seconds: number): string {
  if (seconds <= 0) {
    return "0s";
  }
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const secs = seconds % 60;
  if (hours > 0) {
    return `${hours}h ${minutes}m`;
  }
  if (minutes > 0) {
    return `${minutes}m ${secs}s`;
  }
  return `${secs}s`;
}

function nextKimiAccountName(settings: AppSettings): string {
  const existingCount = settings.accounts.filter((account) => account.provider === "kimi").length;
  return existingCount === 0 ? "Kimi Account" : `Kimi Account ${existingCount + 1}`;
}

function nextGlmAccountName(settings: AppSettings): string {
  const existingCount = settings.accounts.filter((account) => account.provider === "glm").length;
  return existingCount === 0 ? "GLM Account" : `GLM Account ${existingCount + 1}`;
}

function nextMiniMaxAccountName(settings: AppSettings): string {
  const existingCount = settings.accounts.filter((account) => account.provider === "minimax").length;
  return existingCount === 0 ? "MiniMax Account" : `MiniMax Account ${existingCount + 1}`;
}

function nextCopilotAccountName(settings: AppSettings): string {
  const existingCount = settings.accounts.filter((account) => account.provider === "copilot").length;
  return existingCount === 0 ? "Copilot Account" : `Copilot Account ${existingCount + 1}`;
}

export default function App() {
  const currentViewRef = useRef<PanelView>("overview");
  const oauthRequestIdRef = useRef(0);
  const updateNoticeVersionRef = useRef<string | null>(null);
  const [view, setView] = useState<PanelView>("overview");
  const [addAccountBackView, setAddAccountBackView] = useState<AddAccountBackView>("settings");
  const [settingsEntryProxy, setSettingsEntryProxy] = useState<{
    subTab: ProxySubTab;
    manager: ReverseManagerKind | null;
    nonce: number;
  }>({
    subTab: "local",
    manager: null,
    nonce: 0,
  });
  const [status, setStatus] = useState<AppStatus>(emptyStatus);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [loading, setLoading] = useState(true);
  const [oauthStarting, setOauthStarting] = useState(false);
  const [oauthProvider, setOAuthProvider] = useState<OAuthProviderKey>("openai");
  const [oauthTargetAccountId, setOAuthTargetAccountId] = useState<string | null>(null);
  const [kimiTargetAccountId, setKimiTargetAccountId] = useState<string | null>(null);
  const [kimiAccountName, setKimiAccountName] = useState("Kimi Account");
  const [kimiImporting, setKimiImporting] = useState(false);
  const [glmTargetAccountId, setGlmTargetAccountId] = useState<string | null>(null);
  const [glmAccountName, setGlmAccountName] = useState("GLM Account");
  const [glmApiKey, setGlmApiKey] = useState("");
  const [glmImporting, setGlmImporting] = useState(false);
  const [minimaxTargetAccountId, setMiniMaxTargetAccountId] = useState<string | null>(null);
  const [minimaxAccountName, setMiniMaxAccountName] = useState("MiniMax Account");
  const [minimaxApiKey, setMiniMaxApiKey] = useState("");
  const [minimaxImporting, setMiniMaxImporting] = useState(false);
  const [copilotTargetAccountId, setCopilotTargetAccountId] = useState<string | null>(null);
  const [copilotAccountName, setCopilotAccountName] = useState("Copilot Account");
  const [copilotToken, setCopilotToken] = useState("");
  const [copilotImporting, setCopilotImporting] = useState(false);
  const [authUrl, setAuthUrl] = useState<string | null>(null);
  const [authCode, setAuthCode] = useState("");
  const [settingsMessage, setSettingsMessage] = useState<string | null>(null);
  const [availableUpdate, setAvailableUpdate] = useState<AppUpdateInfo | null>(null);
  const [updateChecking, setUpdateChecking] = useState(false);
  const [updateInstalling, setUpdateInstalling] = useState(false);
  const [updateDownloadPercent, setUpdateDownloadPercent] = useState<number | null>(null);
  const [updateMessage, setUpdateMessage] = useState<string | null>(null);
  const [updateError, setUpdateError] = useState<string | null>(null);
  const [form, setForm] = useState<SaveSettingsInput>({
    account_id: "default",
    account_name: "OpenAI Account",
    auth_mode: "apiKey",
    base_url_override: null,
    chatgpt_account_id: null,
    refresh_interval_minutes: 15,
    low_quota_threshold_percent: 10,
    notify_on_low_quota: false,
    notify_on_reset: false,
    reset_notify_lead_minutes: 15,
    git_usage_root: "",
    launch_at_login: false,
    auth_secret: "",
  });

  async function syncQuotaStatus() {
    try {
      setStatus(await getCurrentQuota());
    } catch (error) {
      setStatus((current) => ({
        ...current,
        refresh_status: "error",
        last_error: errorMessage(error, "读取额度状态失败"),
      }));
    }
  }

  async function syncAvailableUpdate({ quiet = false }: { quiet?: boolean } = {}) {
    if (!isTauriRuntime) {
      return;
    }

    if (!quiet) {
      setUpdateChecking(true);
      setUpdateMessage(null);
      setUpdateError(null);
    }

    try {
      const nextUpdate = await checkForAppUpdate();
      setAvailableUpdate(nextUpdate);
      if (!nextUpdate) {
        if (!quiet) {
          setUpdateMessage("当前已是最新版本");
        }
        return;
      }

      if (!quiet) {
        setUpdateMessage(`发现新版本 ${nextUpdate.version}`);
      }

      if (
        updateNoticeVersionRef.current !== nextUpdate.version &&
        (await notificationPermissionGranted())
      ) {
        updateNoticeVersionRef.current = nextUpdate.version;
        await sendDesktopNotification(
          "AI Usage 有新版本可用",
          `发现 ${nextUpdate.version}，可在应用内立即更新。`,
        );
      } else if (updateNoticeVersionRef.current !== nextUpdate.version) {
        updateNoticeVersionRef.current = nextUpdate.version;
      }
    } catch (error) {
      if (!quiet) {
        setUpdateError(errorMessage(error, "检查更新失败"));
      }
    } finally {
      if (!quiet) {
        setUpdateChecking(false);
      }
    }
  }

  useEffect(() => {
    let disposed = false;

    async function bootstrap() {
      try {
        const [nextStatus, nextSettings] = await Promise.all([getCurrentQuota(), getSettings()]);
        if (disposed) {
          return;
        }
        setStatus(nextStatus);
        applySettings(nextSettings);
        if (isTauriRuntime) {
          void syncAvailableUpdate({ quiet: true });
        }
      } finally {
        if (!disposed) {
          setLoading(false);
        }
      }
    }

    void bootstrap();
    if (!isTauriRuntime) {
      return () => {
        disposed = true;
      };
    }
    const unlistenPanel = listen("show-main-panel", () => {
      void syncQuotaStatus();
      void syncAvailableUpdate({ quiet: true });
      navigateToView("overview");
    });
    return () => {
      disposed = true;
      void unlistenPanel.then((dispose) => dispose());
    };
  }, []);

  useEffect(() => {
    if (loading || status.refresh_status !== "refreshing") {
      return undefined;
    }

    const timer = window.setTimeout(() => {
      void syncQuotaStatus();
    }, 1000);

    return () => {
      window.clearTimeout(timer);
    };
  }, [loading, status]);

  useEffect(() => {
    if (!isTauriRuntime) {
      return undefined;
    }

    const timer = window.setInterval(() => {
      void syncAvailableUpdate({ quiet: true });
    }, UPDATE_CHECK_INTERVAL_MS);

    return () => {
      window.clearInterval(timer);
    };
  }, []);

  function applySettings(nextSettings: AppSettings) {
    setSettings(nextSettings);
    setForm((current) => ({
      ...current,
      account_id: nextSettings.account_id,
      account_name: nextSettings.account_name,
      auth_mode: nextSettings.auth_mode,
      base_url_override: nextSettings.base_url_override,
      chatgpt_account_id: nextSettings.chatgpt_account_id,
      refresh_interval_minutes: nextSettings.refresh_interval_minutes,
      low_quota_threshold_percent: nextSettings.low_quota_threshold_percent,
      notify_on_low_quota: nextSettings.notify_on_low_quota,
      notify_on_reset: nextSettings.notify_on_reset,
      reset_notify_lead_minutes: nextSettings.reset_notify_lead_minutes,
      git_usage_root: nextSettings.git_usage_root,
      launch_at_login: nextSettings.launch_at_login,
      auth_secret: "",
    }));
  }

  async function handleRefresh() {
    setStatus((current) => ({
      ...current,
      accounts: current.accounts.map((account) => ({ ...account, last_error: null })),
      refresh_status: "refreshing",
      last_error: null,
    }));
    try {
      setStatus(await refreshQuota());
    } catch (error) {
      setStatus((current) => ({
        ...current,
        refresh_status: "error",
        last_error: errorMessage(error, "刷新失败"),
      }));
    }
  }

  async function handleCheckForUpdate() {
    await syncAvailableUpdate();
  }

  async function handleInstallUpdate() {
    setUpdateInstalling(true);
    setUpdateDownloadPercent(null);
    setUpdateMessage("正在准备更新…");
    setUpdateError(null);

    let downloaded = 0;
    let contentLength = 0;

    try {
      await installAppUpdate((event) => {
        if (event.event === "Started") {
          contentLength = event.data.contentLength ?? 0;
          setUpdateMessage("正在下载更新…");
          return;
        }
        if (event.event === "Progress") {
          downloaded += event.data.chunkLength ?? 0;
          if (contentLength > 0) {
            const percent = Math.min(100, Math.round((downloaded / contentLength) * 100));
            setUpdateDownloadPercent(percent);
            setUpdateMessage(`正在下载更新 ${percent}%`);
          }
          return;
        }
        setUpdateDownloadPercent(100);
        setUpdateMessage("更新已安装，正在重启…");
      });
      await relaunchApp();
    } catch (error) {
      setUpdateError(errorMessage(error, "安装更新失败"));
    } finally {
      setUpdateInstalling(false);
    }
  }

  async function updateSettings(nextForm: SaveSettingsInput) {
    const normalizedForm = { ...nextForm, notify_on_reset: false };
    if (!form.notify_on_low_quota && normalizedForm.notify_on_low_quota) {
      try {
        if (!(await ensureNotificationPermission())) {
          setForm({ ...normalizedForm, notify_on_low_quota: false });
          setSettingsMessage("需要允许系统通知后才能开启低额度提醒");
          return;
        }
      } catch (error) {
        setForm({ ...normalizedForm, notify_on_low_quota: false });
        setSettingsMessage(error instanceof Error ? error.message : "无法请求系统通知权限");
        return;
      }
    }

    setForm(normalizedForm);
    try {
      applySettings(await saveSettings(normalizedForm));
      setSettingsMessage(null);
    } catch (error) {
      setSettingsMessage(error instanceof Error ? error.message : "设置保存失败");
    }
  }

  async function handleOAuth() {
    const requestId = oauthRequestIdRef.current + 1;
    oauthRequestIdRef.current = requestId;
    setOauthStarting(true);
    setSettingsMessage(null);
    const shouldApplyStartResult = () =>
      shouldApplyOAuthStartResult(currentViewRef.current, oauthRequestIdRef.current, requestId);
    try {
      const nextAuthUrl =
        oauthProvider === "anthropic"
          ? await startAnthropicOAuth(oauthTargetAccountId)
          : await startOpenAIOAuth(oauthTargetAccountId);
      if (shouldApplyStartResult()) {
        setAuthUrl(nextAuthUrl);
      }
    } catch (error) {
      if (shouldApplyStartResult()) {
        setSettingsMessage(error instanceof Error ? error.message : "启动 OAuth 失败");
      }
    } finally {
      if (shouldApplyStartResult()) {
        setOauthStarting(false);
      }
    }
  }

  function resetOAuthDraft() {
    oauthRequestIdRef.current += 1;
    setAuthUrl(null);
    setAuthCode("");
    setOauthStarting(false);
    setOAuthProvider("openai");
    setOAuthTargetAccountId(null);
  }

  function navigateToView(nextView: PanelView) {
    const previousView = currentViewRef.current;
    currentViewRef.current = nextView;
    if (shouldResetOAuthAuthDraft(previousView, nextView)) {
      resetOAuthDraft();
    }
    setView(nextView);
  }

  async function handleCompleteOAuth() {
    setSettingsMessage(null);
    if (!hasGeneratedOAuthAuthLink(authUrl)) {
      setSettingsMessage("请先重新生成授权链接");
      return;
    }
    if (!authCode.trim()) {
      setSettingsMessage("请先输入授权链接或 Code");
      return;
    }

    try {
      const result =
        oauthProvider === "anthropic"
          ? await completeAnthropicOAuth(authCode.trim())
          : await completeOpenAIOAuth(authCode.trim());
      if (result.phase === "success") {
        setSettingsMessage(null);
        navigateToView("settings");
        const [nextStatus, nextSettings] = await Promise.all([getCurrentQuota(), getSettings()]);
        setStatus(nextStatus);
        applySettings(nextSettings);
      } else {
        setSettingsMessage(result.message || "授权处理中");
      }
    } catch (error) {
      setSettingsMessage(error instanceof Error ? error.message : "完成授权失败");
    }
  }

  function openAddAccount(backView: AddAccountBackView) {
    setSettingsMessage(null);
    setAddAccountBackView(backView);
    navigateToView("add-account");
  }

  function openOAuthAuth(provider: OAuthProviderKey, accountId: string | null) {
    setSettingsMessage(null);
    setOAuthProvider(provider);
    setOAuthTargetAccountId(accountId);
    navigateToView("oauth-auth");
  }

  function openKimiImport(accountId: string | null, accountName: string) {
    setSettingsMessage(null);
    setKimiTargetAccountId(accountId);
    setKimiAccountName(accountName.trim() || "Kimi Account");
    setKimiImporting(false);
    navigateToView("kimi-auth");
  }

  function openGlmImport(accountId: string | null, accountName: string) {
    setSettingsMessage(null);
    setGlmTargetAccountId(accountId);
    setGlmAccountName(accountName.trim() || "GLM Account");
    setGlmApiKey("");
    setGlmImporting(false);
    navigateToView("glm-auth");
  }

  function openMiniMaxImport(accountId: string | null, accountName: string) {
    setSettingsMessage(null);
    setMiniMaxTargetAccountId(accountId);
    setMiniMaxAccountName(accountName.trim() || "MiniMax Account");
    setMiniMaxApiKey("");
    setMiniMaxImporting(false);
    navigateToView("minimax-auth");
  }

  function openCopilotImport(accountId: string | null, accountName: string) {
    setSettingsMessage(null);
    setCopilotTargetAccountId(accountId);
    setCopilotAccountName(accountName.trim() || "Copilot Account");
    setCopilotToken("");
    setCopilotImporting(false);
    navigateToView("copilot-auth");
  }

  async function handleImportKimi() {
    const trimmedName = kimiAccountName.trim();
    if (!trimmedName) {
      setSettingsMessage("请输入账号名称");
      return;
    }

    setKimiImporting(true);
    setSettingsMessage(null);
    try {
      const nextSettings = await importKimiAccount(trimmedName, kimiTargetAccountId);
      applySettings(nextSettings);
      navigateToView("settings");
      try {
        setStatus(await refreshQuota());
        setSettingsMessage("Kimi 账号已导入并刷新额度");
      } catch (refreshError) {
        setStatus(await getCurrentQuota());
        setSettingsMessage(refreshError instanceof Error ? refreshError.message : "Kimi 账号已导入，额度刷新失败");
      }
    } catch (error) {
      setSettingsMessage(error instanceof Error ? error.message : "导入 Kimi 账号失败");
    } finally {
      setKimiImporting(false);
    }
  }

  async function handleImportGlm() {
    const trimmedName = glmAccountName.trim();
    const trimmedApiKey = glmApiKey.trim();
    if (!trimmedName) {
      setSettingsMessage("请输入账号名称");
      return;
    }
    if (!trimmedApiKey) {
      setSettingsMessage("请输入 GLM API Key");
      return;
    }

    setGlmImporting(true);
    setSettingsMessage(null);
    try {
      const nextSettings = await importGlmAccount(trimmedName, trimmedApiKey, glmTargetAccountId);
      applySettings(nextSettings);
      navigateToView("settings");
      try {
        setStatus(await refreshQuota());
        setSettingsMessage("GLM 账号已添加并刷新额度");
      } catch (refreshError) {
        setStatus(await getCurrentQuota());
        setSettingsMessage(refreshError instanceof Error ? refreshError.message : "GLM 账号已添加，额度刷新失败");
      }
    } catch (error) {
      setSettingsMessage(error instanceof Error ? error.message : "添加 GLM 账号失败");
    } finally {
      setGlmImporting(false);
    }
  }

  async function handleImportMiniMax() {
    const trimmedName = minimaxAccountName.trim();
    const trimmedApiKey = minimaxApiKey.trim();
    if (!trimmedName) {
      setSettingsMessage("请输入账号名称");
      return;
    }
    if (!trimmedApiKey) {
      setSettingsMessage("请输入 MiniMax API Key");
      return;
    }

    setMiniMaxImporting(true);
    setSettingsMessage(null);
    try {
      const nextSettings = await importMiniMaxAccount(trimmedName, trimmedApiKey, minimaxTargetAccountId);
      applySettings(nextSettings);
      navigateToView("settings");
      try {
        setStatus(await refreshQuota());
        setSettingsMessage("MiniMax 账号已添加并刷新额度");
      } catch (refreshError) {
        setStatus(await getCurrentQuota());
        setSettingsMessage(refreshError instanceof Error ? refreshError.message : "MiniMax 账号已添加，额度刷新失败");
      }
    } catch (error) {
      setSettingsMessage(error instanceof Error ? error.message : "添加 MiniMax 账号失败");
    } finally {
      setMiniMaxImporting(false);
    }
  }

  async function handleImportCopilot() {
    const trimmedName = copilotAccountName.trim();
    const trimmedToken = copilotToken.trim();
    if (!trimmedName) {
      setSettingsMessage("请输入账号名称");
      return;
    }

    setCopilotImporting(true);
    setSettingsMessage(null);
    try {
      const nextSettings = await importCopilotAccount(trimmedName, trimmedToken || null, copilotTargetAccountId);
      applySettings(nextSettings);
      navigateToView("settings");
      try {
        setStatus(await refreshQuota());
        setSettingsMessage("Copilot 账号已添加并刷新额度");
      } catch (refreshError) {
        setStatus(await getCurrentQuota());
        setSettingsMessage(refreshError instanceof Error ? refreshError.message : "Copilot 账号已添加，额度刷新失败");
      }
    } catch (error) {
      setSettingsMessage(error instanceof Error ? error.message : "添加 Copilot 账号失败");
    } finally {
      setCopilotImporting(false);
    }
  }

  async function handleDeleteConnectedAccount(account: ConnectedAccount) {
    if (isManagedCopilotAccountId(account.account_id)) {
      setSettingsEntryProxy((current) => ({
        subTab: "reverse",
        manager: "copilot",
        nonce: current.nonce + 1,
      }));
      navigateToView("settings");
      return;
    }

    setSettingsMessage(null);
    try {
      const nextSettings = await deleteConnectedAccount(account.account_id);
      applySettings(nextSettings);
      setStatus(await getCurrentQuota());
    } catch (error) {
      setSettingsMessage(error instanceof Error ? error.message : "删除账号失败");
    }
  }

  if (loading || !settings) {
    return (
      <main className="panel-root panel-root-overview">
        <Card className="overview-panel loading-panel">加载中...</Card>
      </main>
    );
  }

  return (
    <main className={`panel-root panel-root-${view}`}>
      {view === "overview" ? (
        <OverviewPanel
          status={status}
          settings={settings}
          hasUpdateAvailable={Boolean(availableUpdate)}
          onRefresh={() => void handleRefresh()}
          onSettings={() => navigateToView("settings")}
          onAddAccount={() => openAddAccount("overview")}
        />
      ) : null}

      {view === "settings" ? (
        <SettingsPanel
          form={form}
          settings={settings}
          status={status}
          message={settingsMessage}
          availableUpdate={availableUpdate}
          updateChecking={updateChecking}
          updateInstalling={updateInstalling}
          updateDownloadPercent={updateDownloadPercent}
          updateMessage={updateMessage}
          updateError={updateError}
          entryProxySubTab={settingsEntryProxy.subTab}
          entryReverseManagerOpen={settingsEntryProxy.manager}
          entryProxyNonce={settingsEntryProxy.nonce}
          onChange={(nextForm) => void updateSettings(nextForm)}
          onBack={() => navigateToView("overview")}
          onAddAccount={() => openAddAccount("settings")}
          onCheckForUpdate={() => void handleCheckForUpdate()}
          onInstallUpdate={() => void handleInstallUpdate()}
          onAddReverseOpenAIOAuth={() => openOAuthAuth("openai", null)}
          onManagedCopilotChanged={() => handleRefresh()}
          onReauthorize={(account) => {
            if (isManagedCopilotAccountId(account.account_id)) {
              setSettingsEntryProxy((current) => ({
                subTab: "reverse",
                manager: "copilot",
                nonce: current.nonce + 1,
              }));
              navigateToView("settings");
              return;
            }
            if (account.provider === "kimi") {
              openKimiImport(account.account_id, connectedAccountSubtitle(account));
              return;
            }
            if (account.provider === "glm") {
              openGlmImport(account.account_id, connectedAccountSubtitle(account));
              return;
            }
            if (account.provider === "minimax") {
              openMiniMaxImport(account.account_id, connectedAccountSubtitle(account));
              return;
            }
            if (account.provider === "copilot") {
              openCopilotImport(account.account_id, connectedAccountSubtitle(account));
              return;
            }
            openOAuthAuth(account.provider as OAuthProviderKey, account.account_id);
          }}
          onDeleteAccount={(account) => void handleDeleteConnectedAccount(account)}
        />
      ) : null}

      {view === "add-account" ? (
        <AddAccountPanel
          message={settingsMessage}
          onBack={() => navigateToView(addAccountBackView)}
          onNext={(provider) => {
            setSettingsMessage(null);
            if (isOAuthProvider(provider)) {
              openOAuthAuth(provider, null);
              return;
            }
            if (provider === "kimi") {
              openKimiImport(null, nextKimiAccountName(settings));
              return;
            }
            if (provider === "glm") {
              openGlmImport(null, nextGlmAccountName(settings));
              return;
            }
            if (provider === "minimax") {
              openMiniMaxImport(null, nextMiniMaxAccountName(settings));
              return;
            }
            if (provider === "copilot") {
              setSettingsEntryProxy((current) => ({
                subTab: "reverse",
                manager: "copilot",
                nonce: current.nonce + 1,
              }));
              navigateToView("settings");
              return;
            }
            setSettingsMessage("该平台的接入流程尚未实现");
          }}
        />
      ) : null}

      {view === "oauth-auth" ? (
        <OAuthAuthPanel
          provider={oauthProvider}
          authUrl={authUrl}
          authCode={authCode}
          oauthStarting={oauthStarting}
          message={settingsMessage}
          onBack={() => navigateToView("add-account")}
          onGenerate={() => void handleOAuth()}
          onComplete={() => void handleCompleteOAuth()}
          onCodeChange={setAuthCode}
        />
      ) : null}

      {view === "kimi-auth" ? (
        <KimiImportPanel
          accountName={kimiAccountName}
          importing={kimiImporting}
          message={settingsMessage}
          onBack={() => navigateToView(kimiTargetAccountId ? "settings" : "add-account")}
          onAccountNameChange={setKimiAccountName}
          onImport={() => void handleImportKimi()}
        />
      ) : null}

      {view === "glm-auth" ? (
        <GlmApiKeyPanel
          accountName={glmAccountName}
          apiKey={glmApiKey}
          importing={glmImporting}
          message={settingsMessage}
          onBack={() => navigateToView(glmTargetAccountId ? "settings" : "add-account")}
          onAccountNameChange={setGlmAccountName}
          onApiKeyChange={setGlmApiKey}
          onImport={() => void handleImportGlm()}
        />
      ) : null}

      {view === "minimax-auth" ? (
        <MiniMaxApiKeyPanel
          accountName={minimaxAccountName}
          apiKey={minimaxApiKey}
          importing={minimaxImporting}
          message={settingsMessage}
          onBack={() => navigateToView(minimaxTargetAccountId ? "settings" : "add-account")}
          onAccountNameChange={setMiniMaxAccountName}
          onApiKeyChange={setMiniMaxApiKey}
          onImport={() => void handleImportMiniMax()}
        />
      ) : null}

      {view === "copilot-auth" ? (
        <CopilotTokenPanel
          accountName={copilotAccountName}
          token={copilotToken}
          importing={copilotImporting}
          message={settingsMessage}
          onBack={() => navigateToView(copilotTargetAccountId ? "settings" : "add-account")}
          onAccountNameChange={setCopilotAccountName}
          onTokenChange={setCopilotToken}
          onImport={() => void handleImportCopilot()}
        />
      ) : null}
    </main>
  );
}

function OverviewPanel({
  status,
  settings,
  hasUpdateAvailable,
  onRefresh,
  onSettings,
  onAddAccount,
}: {
  status: AppStatus;
  settings: AppSettings;
  hasUpdateAvailable: boolean;
  onRefresh: () => void;
  onSettings: () => void;
  onAddAccount: () => void;
}) {
  const accounts = quotaAccounts(settings, status);
  const hasAccount = accounts.length > 0 || hasConnectedAccount(settings, status);
  const hasAccountErrors = accounts.some((account) => Boolean(account.last_error));
  const globalError = status.refresh_status === "error" && !hasAccountErrors ? status.last_error : null;

  if (!hasAccount) {
    return (
      <Card className="overview-panel">
        <PanelHeader
          isRefreshing={status.refresh_status === "refreshing"}
          hasUpdateAvailable={hasUpdateAvailable}
          onRefresh={onRefresh}
          onSettings={onSettings}
        />
        <div className="empty-account-state">
          <Inbox className="empty-account-icon" aria-hidden="true" />
          <div className="empty-account-title">暂无账号</div>
          <p>您还没有添加任何账号，点击下方按钮开始添加</p>
          <Button className="empty-account-button" onClick={onAddAccount}>
            <Plus data-icon="inline-start" />
            添加账号
          </Button>
        </div>
      </Card>
    );
  }

  return (
    <Card className="overview-panel">
      <PanelHeader
        isRefreshing={status.refresh_status === "refreshing"}
        hasUpdateAvailable={hasUpdateAvailable}
        onRefresh={onRefresh}
        onSettings={onSettings}
      />
      {globalError ? <div className="inline-error">{globalError}</div> : null}
      {accounts.map((account) => (
        <QuotaAccountCard key={account.account_id} account={account} />
      ))}
    </Card>
  );
}

function QuotaAccountCard({
  account,
}: {
  account: AccountQuotaStatus;
}) {
  const icon = providerIconConfig(account.provider);
  const cardState = quotaAccountCardState(account);
  return (
    <Card className={`quota-card ${cardState.muted ? "quota-card-error" : ""}`}>
      <div className="overview-account-row">
        <span className="overview-provider-logo">
          <ProviderIcon icon={icon.icon} iconMode={icon.iconMode} />
        </span>
        <span className="account-subtitle">{quotaAccountSubtitle(account)}</span>
      </div>

      {cardState.error ? <div className="inline-error">{cardState.error}</div> : null}
      {cardState.stale ? <div className="stale-text">数据来自缓存</div> : null}

      {quotaDisplayRows(account).map((row) => (
        <QuotaRow
          key={row.label}
          label={row.label}
          window={row.window}
          muted={cardState.muted}
        />
      ))}
    </Card>
  );
}

function PanelHeader({
  isRefreshing,
  hasUpdateAvailable,
  onRefresh,
  onSettings,
}: {
  isRefreshing: boolean;
  hasUpdateAvailable: boolean;
  onRefresh: () => void;
  onSettings: () => void;
}) {
  return (
    <div className="panel-header">
      <div className="logo-mark">
        <img className="logo-mark-img" src={aiUsageLogo} alt="" aria-hidden="true" />
      </div>
      <div className="icon-row">
        <Button
          variant="ghost"
          size="icon-sm"
          className="icon-button"
          aria-label="刷新"
          aria-busy={isRefreshing}
          onClick={onRefresh}
        >
          <RefreshCw className={isRefreshing ? "refresh-icon-spinning" : undefined} data-icon="inline-start" />
        </Button>
        <Button variant="ghost" size="icon-sm" className="icon-button" aria-label="设置" onClick={onSettings}>
          {hasUpdateAvailable ? <span className="icon-badge" aria-hidden="true" /> : null}
          <Settings data-icon="inline-start" />
        </Button>
      </div>
    </div>
  );
}

function QuotaRow({
  label,
  window,
  muted,
}: {
  label: string;
  window: QuotaWindow | null;
  muted: boolean;
}) {
  const remaining = remainingQuotaProgressValue(window);
  const tone = muted || !window ? "muted" : quotaTone(remaining);
  return (
    <>
      <div className="quota-row-header">
        <span>{label}</span>
        <span className={`quota-value quota-value-${tone}`}>{formatPercent(window)}</span>
      </div>
      <Progress value={remaining} className={`quota-progress quota-progress-${tone}`} />
    </>
  );
}

function SettingsPanel({
  form,
  settings,
  status,
  message,
  availableUpdate,
  updateChecking,
  updateInstalling,
  updateDownloadPercent,
  updateMessage,
  updateError,
  entryProxySubTab,
  entryReverseManagerOpen,
  entryProxyNonce,
  onChange,
  onBack,
  onAddAccount,
  onCheckForUpdate,
  onInstallUpdate,
  onAddReverseOpenAIOAuth,
  onManagedCopilotChanged,
  onReauthorize,
  onDeleteAccount,
}: {
  form: SaveSettingsInput;
  settings: AppSettings;
  status: AppStatus;
  message: string | null;
  availableUpdate: AppUpdateInfo | null;
  updateChecking: boolean;
  updateInstalling: boolean;
  updateDownloadPercent: number | null;
  updateMessage: string | null;
  updateError: string | null;
  entryProxySubTab: ProxySubTab;
  entryReverseManagerOpen: ReverseManagerKind | null;
  entryProxyNonce: number;
  onChange: (nextForm: SaveSettingsInput) => void;
  onBack: () => void;
  onAddAccount: () => void;
  onCheckForUpdate: () => void;
  onInstallUpdate: () => void;
  onAddReverseOpenAIOAuth: () => void;
  onManagedCopilotChanged: () => Promise<void>;
  onReauthorize: (account: ConnectedAccount) => void;
  onDeleteAccount: (account: ConnectedAccount) => void;
}) {
  const [thresholdDraft, setThresholdDraft] = useState(String(form.low_quota_threshold_percent));
  const [activeTab, setActiveTab] = useState<SettingsTab>("quota");
  const [activeUsageTab, setActiveUsageTab] = useState<SettingsUsageTab>("token");
  const [usageRangeUiState, setUsageRangeUiState] = useState(() => createUsageRangeUiState());
  const [tokenReportsByRange, setTokenReportsByRange] = useState<Partial<Record<string, LocalTokenUsageReport>>>({});
  const [gitReportsByRange, setGitReportsByRange] = useState<Partial<Record<string, GitUsageReport>>>({});
  const [kpiReportsByRange, setKpiReportsByRange] = useState<Partial<Record<string, PrKpiReport>>>({});
  const [lastReadyTokenRangeKey, setLastReadyTokenRangeKey] = useState<string | null>("thisMonth");
  const [lastReadyGitRangeKey, setLastReadyGitRangeKey] = useState<string | null>("thisMonth");
  const [lastReadyKpiRangeKey, setLastReadyKpiRangeKey] = useState<string | null>("thisMonth");
  const [tokenLoading, setTokenLoading] = useState(false);
  const [gitLoading, setGitLoading] = useState(false);
  const [kpiLoading, setKpiLoading] = useState(false);
  const [tokenRefreshing, setTokenRefreshing] = useState(false);
  const [gitRefreshing, setGitRefreshing] = useState(false);
  const [kpiRefreshing, setKpiRefreshing] = useState(false);
  const [tokenError, setTokenError] = useState<string | null>(null);
  const [gitError, setGitError] = useState<string | null>(null);
  const [kpiError, setKpiError] = useState<string | null>(null);
  const [gitRootDraft, setGitRootDraft] = useState(form.git_usage_root);
  const [gitRootPicking, setGitRootPicking] = useState(false);
  const [proxySubTab, setProxySubTab] = useState<ProxySubTab>("local");
  const [proxySettingsState, setProxySettingsState] = useState<LocalProxySettingsState | null>(null);
  const [proxyStatus, setProxyStatus] = useState<LocalProxyStatus | null>(null);
  const [proxyLoading, setProxyLoading] = useState(false);
  const [proxySaving, setProxySaving] = useState(false);
  const [proxyActionPending, setProxyActionPending] = useState(false);
  const [proxyProfileSaving, setProxyProfileSaving] = useState(false);
  const [proxyError, setProxyError] = useState<string | null>(null);
  const [reverseSettingsState, setReverseSettingsState] = useState<ReverseProxySettingsState | null>(null);
  const [reverseStatus, setReverseStatus] = useState<ReverseProxyStatus | null>(null);
  const [reverseLoading, setReverseLoading] = useState(false);
  const [reverseSaving, setReverseSaving] = useState(false);
  const [reverseManagerOpen, setReverseManagerOpen] = useState<"copilot" | "openai" | null>(null);
  const [copilotAuthStatus, setCopilotAuthStatus] = useState<CopilotAuthStatus | null>(null);
  const [copilotDeviceCode, setCopilotDeviceCode] = useState<GitHubDeviceCodeResponse | null>(null);
  const [copilotPolling, setCopilotPolling] = useState(false);
  const [proxyEditorAccountId, setProxyEditorAccountId] = useState<string | null>(null);
  const [proxyEditorBaseUrl, setProxyEditorBaseUrl] = useState("");
  const [proxyEditorApiFormat, setProxyEditorApiFormat] = useState<ClaudeApiFormat>("anthropic");
  const [proxyEditorAuthField, setProxyEditorAuthField] = useState<ClaudeAuthField>("ANTHROPIC_AUTH_TOKEN");
  const [proxyEditorApiKey, setProxyEditorApiKey] = useState("");
  const [routeEditorRouteId, setRouteEditorRouteId] = useState<string | null>(null);
  const [routeEditorPattern, setRouteEditorPattern] = useState("");
  const [routeEditorAccountId, setRouteEditorAccountId] = useState("");
  const [routeEditorEnabled, setRouteEditorEnabled] = useState(true);
  const usageRangeSelection = usageRangeUiState.appliedSelection;
  const usageRangeKey = usageRangeSelectionKey(usageRangeSelection);
  const selectedRangeOption = usageRangeUiState.selectedOption;
  const customRangeDraft = usageRangeUiState.customDraft;
  const usageRangeError =
    selectedRangeOption === "custom" ? validateCustomUsageRangeSelection(customRangeDraft) : null;
  const customRangeBounds = customUsageWindowBounds();

  function updateTokenReport(rangeKey: string, report: LocalTokenUsageReport) {
    setTokenReportsByRange((current) => ({ ...current, [rangeKey]: report }));
    if (!report.pending) {
      setLastReadyTokenRangeKey(rangeKey);
    }
  }

  function updateGitReport(rangeKey: string, report: GitUsageReport) {
    setGitReportsByRange((current) => ({ ...current, [rangeKey]: report }));
    if (!report.pending) {
      setLastReadyGitRangeKey(rangeKey);
    }
  }

  function updateKpiReport(rangeKey: string, report: PrKpiReport) {
    setKpiReportsByRange((current) => ({ ...current, [rangeKey]: report }));
    if (!report.pending) {
      setLastReadyKpiRangeKey(rangeKey);
    }
  }

  useEffect(() => {
    setThresholdDraft(String(form.low_quota_threshold_percent));
  }, [form.low_quota_threshold_percent]);

  useEffect(() => {
    setGitRootDraft(form.git_usage_root);
  }, [form.git_usage_root]);

  useEffect(() => {
    setProxySubTab(entryProxySubTab);
    setReverseManagerOpen(entryReverseManagerOpen);
    if (entryProxySubTab === "reverse") {
      setActiveTab("proxy");
    }
  }, [entryProxySubTab, entryReverseManagerOpen, entryProxyNonce]);

  useEffect(() => {
    if (activeTab !== "proxy") {
      return undefined;
    }

    let cancelled = false;
    setProxyLoading(true);
    setReverseLoading(true);
    setProxyError(null);

    Promise.all([
      getLocalProxySettings(),
      getLocalProxyStatus(),
      getReverseProxySettings(),
      getReverseProxyStatus(),
      copilotGetAuthStatus(),
    ])
      .then(([nextSettingsState, nextStatus, nextReverseSettings, nextReverseStatus, nextCopilotAuthStatus]) => {
        if (cancelled) {
          return;
        }
        setProxySettingsState(nextSettingsState);
        setProxyStatus(nextStatus);
        setReverseSettingsState(nextReverseSettings);
        setReverseStatus(nextReverseStatus);
        setCopilotAuthStatus(nextCopilotAuthStatus);
      })
      .catch((error) => {
        if (!cancelled) {
          setProxyError(errorMessage(error, "代理设置读取失败"));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setProxyLoading(false);
          setReverseLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [activeTab, settings.accounts.length]);

  useEffect(() => {
    if (!isTauriRuntime || activeTab !== "proxy" || proxySubTab !== "local" || !proxyStatus?.running) {
      return undefined;
    }

    let cancelled = false;
    const intervalId = window.setInterval(() => {
      getLocalProxyStatus()
        .then((nextStatus) => {
          if (!cancelled) {
            setProxyStatus(nextStatus);
          }
        })
        .catch((error) => {
          if (!cancelled) {
            setProxyError(errorMessage(error, "代理状态刷新失败"));
          }
        });
    }, 1000);

    return () => {
      cancelled = true;
      window.clearInterval(intervalId);
    };
  }, [activeTab, proxySubTab, proxyStatus?.running]);

  useEffect(() => {
    if (!copilotPolling || !copilotDeviceCode) {
      return undefined;
    }

    return startCopilotDevicePolling(copilotDeviceCode.interval, () => {
      void handleCopilotPollOnce(copilotDeviceCode.device_code);
    });
  }, [copilotPolling, copilotDeviceCode]);

  useEffect(() => {
    if (activeTab !== "tokens" || activeUsageTab !== "token") {
      return undefined;
    }

    let cancelled = false;
    const cachedReport = tokenReportsByRange[usageRangeKey] ?? null;
    const lastReadyReport =
      lastReadyTokenRangeKey ? tokenReportsByRange[lastReadyTokenRangeKey] ?? null : null;
    const visibleState = resolveVisibleReportState(
      usageRangeKey,
      cachedReport,
      lastReadyTokenRangeKey,
      lastReadyReport,
    );
    setTokenLoading(!visibleState.visibleKey);
    setTokenError(null);

    getLocalTokenUsage(usageRangeSelection)
      .then((report) => {
        if (!cancelled) {
          updateTokenReport(usageRangeKey, report);
        }
      })
      .catch((error) => {
        if (!cancelled) {
          setTokenError(errorMessage(error, "Token 用量读取失败"));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setTokenLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [activeTab, activeUsageTab, usageRangeKey, usageRangeSelection]);

  useEffect(() => {
    if (activeTab !== "tokens" || activeUsageTab !== "git") {
      return undefined;
    }

    let cancelled = false;
    const cachedReport = gitReportsByRange[usageRangeKey] ?? null;
    const lastReadyReport = lastReadyGitRangeKey ? gitReportsByRange[lastReadyGitRangeKey] ?? null : null;
    const visibleState = resolveVisibleReportState(
      usageRangeKey,
      cachedReport,
      lastReadyGitRangeKey,
      lastReadyReport,
    );
    setGitLoading(!visibleState.visibleKey);
    setGitError(null);

    getGitUsage(usageRangeSelection)
      .then((report) => {
        if (!cancelled) {
          updateGitReport(usageRangeKey, report);
        }
      })
      .catch((error) => {
        if (!cancelled) {
          setGitError(errorMessage(error, "Git 统计读取失败"));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setGitLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [activeTab, activeUsageTab, usageRangeKey, usageRangeSelection, form.git_usage_root]);

  useEffect(() => {
    if (activeTab !== "tokens" || activeUsageTab !== "kpi") {
      return undefined;
    }

    let cancelled = false;
    const cachedReport = kpiReportsByRange[usageRangeKey] ?? null;
    const lastReadyReport = lastReadyKpiRangeKey ? kpiReportsByRange[lastReadyKpiRangeKey] ?? null : null;
    const visibleState = resolveVisibleReportState(
      usageRangeKey,
      cachedReport,
      lastReadyKpiRangeKey,
      lastReadyReport,
    );
    setKpiLoading(!visibleState.visibleKey);
    setKpiError(null);

    getPrKpi(usageRangeSelection)
      .then((report) => {
        if (!cancelled) {
          updateKpiReport(usageRangeKey, report);
        }
      })
      .catch((error) => {
        if (!cancelled) {
          setKpiError(errorMessage(error, "KPI 分析读取失败"));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setKpiLoading(false);
        }
      });

    return () => {
      cancelled = true;
    };
  }, [activeTab, activeUsageTab, usageRangeKey, usageRangeSelection, form.git_usage_root]);

  useEffect(() => {
    if (!isTauriRuntime || activeTab !== "tokens" || activeUsageTab !== "token") {
      return undefined;
    }

    let cancelled = false;
    const unlisten = listen("local-token-usage-cache-updated", () => {
      getLocalTokenUsage(usageRangeSelection)
        .then((report) => {
          if (!cancelled) {
            updateTokenReport(usageRangeKey, report);
          }
        })
        .catch((error) => {
          if (!cancelled) {
            setTokenError(errorMessage(error, "Token 用量读取失败"));
          }
        });
    });

    return () => {
      cancelled = true;
      void unlisten.then((dispose) => dispose());
    };
  }, [activeTab, activeUsageTab, usageRangeKey, usageRangeSelection]);

  useEffect(() => {
    if (!isTauriRuntime || activeTab !== "tokens" || activeUsageTab !== "git") {
      return undefined;
    }

    let cancelled = false;
    const unlisten = listen("git-usage-cache-updated", () => {
      getGitUsage(usageRangeSelection)
        .then((report) => {
          if (!cancelled) {
            updateGitReport(usageRangeKey, report);
          }
        })
        .catch((error) => {
          if (!cancelled) {
            setGitError(errorMessage(error, "Git 统计读取失败"));
          }
        });
    });

    return () => {
      cancelled = true;
      void unlisten.then((dispose) => dispose());
    };
  }, [activeTab, activeUsageTab, usageRangeKey, usageRangeSelection]);

  useEffect(() => {
    if (!isTauriRuntime || activeTab !== "tokens" || activeUsageTab !== "kpi") {
      return undefined;
    }

    let cancelled = false;
    const unlisten = listen("pr-kpi-cache-updated", () => {
      getPrKpi(usageRangeSelection)
        .then((report) => {
          if (!cancelled) {
            updateKpiReport(usageRangeKey, report);
          }
        })
        .catch((error) => {
          if (!cancelled) {
            setKpiError(errorMessage(error, "KPI 分析读取失败"));
          }
        });
    });

    return () => {
      cancelled = true;
      void unlisten.then((dispose) => dispose());
    };
  }, [activeTab, activeUsageTab, usageRangeKey, usageRangeSelection]);

  function commitThreshold(value: string) {
    const parsed = Number.parseInt(value, 10);
    const nextValue = Number.isFinite(parsed) ? Math.max(1, Math.min(100, parsed)) : 1;
    setThresholdDraft(String(nextValue));
    if (nextValue !== form.low_quota_threshold_percent) {
      onChange({ ...form, low_quota_threshold_percent: nextValue });
    }
  }

  function commitGitUsageRoot(value: string) {
    const nextValue = value.trim();
    setGitRootDraft(nextValue);
    if (nextValue && nextValue !== form.git_usage_root) {
      setGitReportsByRange({});
      setKpiReportsByRange({});
      onChange({ ...form, git_usage_root: nextValue });
    }
  }

  async function handlePickGitUsageRoot() {
    setGitRootPicking(true);
    setGitError(null);
    try {
      const selected = await chooseGitUsageRoot(gitRootDraft || form.git_usage_root);
      if (selected) {
        setGitRootDraft(selected);
        setGitReportsByRange({});
        setKpiReportsByRange({});
        onChange({ ...form, git_usage_root: selected });
      }
    } catch (error) {
      setGitError(errorMessage(error, "选择 Git 统计路径失败"));
    } finally {
      setGitRootPicking(false);
    }
  }

  async function handleTokenRefresh() {
    setTokenRefreshing(true);
    setTokenError(null);
    try {
      const report = await refreshLocalTokenUsage(usageRangeSelection);
      updateTokenReport(usageRangeKey, report);
    } catch (error) {
      setTokenError(errorMessage(error, "Token 用量刷新失败"));
    } finally {
      setTokenRefreshing(false);
    }
  }

  async function handleGitRefresh() {
    setGitRefreshing(true);
    setGitError(null);
    try {
      const report = await refreshGitUsage(usageRangeSelection);
      updateGitReport(usageRangeKey, report);
    } catch (error) {
      setGitError(errorMessage(error, "Git 统计刷新失败"));
    } finally {
      setGitRefreshing(false);
    }
  }

  async function handleKpiRefresh() {
    setKpiRefreshing(true);
    setKpiError(null);
    try {
      const report = await refreshPrKpi(usageRangeSelection);
      updateKpiReport(usageRangeKey, report);
    } catch (error) {
      setKpiError(errorMessage(error, "KPI 分析刷新失败"));
    } finally {
      setKpiRefreshing(false);
    }
  }

  async function reloadProxyState() {
    const [nextSettingsState, nextStatus, nextReverseSettings, nextReverseStatus, nextCopilotAuthStatus] =
      await Promise.all([
        getLocalProxySettings(),
        getLocalProxyStatus(),
        getReverseProxySettings(),
        getReverseProxyStatus(),
        copilotGetAuthStatus(),
      ]);
    setProxySettingsState(nextSettingsState);
    setProxyStatus(nextStatus);
    setReverseSettingsState(nextReverseSettings);
    setReverseStatus(nextReverseStatus);
    setCopilotAuthStatus(nextCopilotAuthStatus);
  }

  async function persistProxyConfig(nextConfig: LocalProxySettingsState["config"]) {
    setProxySaving(true);
    setProxyError(null);
    try {
      const nextState = await saveLocalProxySettings({ config: nextConfig });
      setProxySettingsState(nextState);
    } catch (error) {
      setProxyError(errorMessage(error, "代理设置保存失败"));
    } finally {
      setProxySaving(false);
    }
  }

  function updateProxyConfig(nextConfig: LocalProxySettingsState["config"]) {
    setProxySettingsState((current) => (current ? { ...current, config: nextConfig } : current));
  }

  async function handleProxyToggle(checked: boolean) {
    setProxyActionPending(true);
    setProxyError(null);
    try {
      const nextStatus = checked ? await startLocalProxy() : await stopLocalProxy();
      setProxyStatus(nextStatus);
      if (!proxySettingsState) {
        await reloadProxyState();
      }
    } catch (error) {
      setProxyError(errorMessage(error, checked ? "启动代理失败" : "停止代理失败"));
    } finally {
      setProxyActionPending(false);
    }
  }

  async function handleProxyRouteChange(
    routeId: string,
    updater: (route: LocalProxySettingsState["config"]["routes"][number]) => LocalProxySettingsState["config"]["routes"][number],
  ) {
    if (!proxySettingsState) {
      return;
    }
    const nextConfig = {
      ...proxySettingsState.config,
      routes: proxySettingsState.config.routes.map((route) => (route.id === routeId ? updater(route) : route)),
    };
    updateProxyConfig(nextConfig);
    await persistProxyConfig(nextConfig);
  }

  function openProxyRouteEditor(routeId: string | null) {
    if (!proxySettingsState) {
      return;
    }
    if (!routeId) {
      const firstCapability =
        proxySettingsState.capabilities.find((capability) => capability.is_claude_compatible_provider) ??
        proxySettingsState.capabilities[0];
      setRouteEditorRouteId(null);
      setRouteEditorPattern("claude-*");
      setRouteEditorAccountId(firstCapability?.account_id ?? "");
      setRouteEditorEnabled(true);
      return;
    }
    const route = proxySettingsState.config.routes.find((item) => item.id === routeId);
    if (!route) {
      return;
    }
    setRouteEditorRouteId(route.id);
    setRouteEditorPattern(route.model_pattern);
    setRouteEditorAccountId(route.account_id);
    setRouteEditorEnabled(route.enabled);
  }

  async function handleProxySaveRoute() {
    if (!proxySettingsState) {
      return;
    }
    const nextRoute = {
      id: routeEditorRouteId ?? `route-${Date.now()}`,
      model_pattern: routeEditorPattern.trim(),
      account_id: routeEditorAccountId,
      enabled: routeEditorEnabled,
    };
    if (!nextRoute.model_pattern || !nextRoute.account_id) {
      return;
    }
    const nextConfig = {
      ...proxySettingsState.config,
      routes: routeEditorRouteId
        ? proxySettingsState.config.routes.map((route) => (route.id === routeEditorRouteId ? nextRoute : route))
        : [...proxySettingsState.config.routes, nextRoute],
    };
    updateProxyConfig(nextConfig);
    await persistProxyConfig(nextConfig);
    setRouteEditorRouteId(null);
    setRouteEditorPattern("");
    setRouteEditorAccountId("");
    setRouteEditorEnabled(true);
  }

  async function handleProxyDeleteRoute(routeId: string) {
    if (!proxySettingsState) {
      return;
    }
    const nextConfig = {
      ...proxySettingsState.config,
      routes: proxySettingsState.config.routes.filter((route) => route.id !== routeId),
    };
    updateProxyConfig(nextConfig);
    await persistProxyConfig(nextConfig);
  }

  function openProxyProfileEditor(capability: ClaudeProxyCapability) {
    setProxyEditorAccountId(capability.account_id);
    setProxyEditorBaseUrl(capability.profile.base_url ?? "");
    setProxyEditorApiFormat(capability.profile.api_format);
    setProxyEditorAuthField(capability.profile.auth_field);
    setProxyEditorApiKey("");
  }

  async function handleProxyProfileSave() {
    if (!proxyEditorAccountId) {
      return;
    }
    setProxyProfileSaving(true);
    setProxyError(null);
    try {
      const nextState = await saveClaudeProxyProfile({
        account_id: proxyEditorAccountId,
        base_url: proxyEditorBaseUrl.trim() || null,
        api_format: proxyEditorApiFormat,
        auth_field: proxyEditorAuthField,
        api_key_or_token: proxyEditorApiKey.trim() || null,
      });
      setProxySettingsState(nextState);
      setProxyEditorAccountId(null);
      setProxyEditorApiKey("");
    } catch (error) {
      setProxyError(errorMessage(error, "Claude 代理资料保存失败"));
    } finally {
      setProxyProfileSaving(false);
    }
  }

  async function persistReverseProxySettings(nextInput: SaveReverseProxySettingsInput) {
    setReverseSaving(true);
    setProxyError(null);
    try {
      const nextSettings = await saveReverseProxySettings(nextInput);
      setReverseSettingsState(nextSettings);
      setReverseStatus(await getReverseProxyStatus());
      setProxySettingsState(await getLocalProxySettings());
    } catch (error) {
      setProxyError(errorMessage(error, "反向代理设置保存失败"));
    } finally {
      setReverseSaving(false);
    }
  }

  async function handleReverseProxyToggle(checked: boolean) {
    if (!reverseSettingsState) {
      return;
    }
    await persistReverseProxySettings({
      enabled: checked,
      default_copilot_account_id: reverseSettingsState.default_copilot_account_id,
      default_openai_account_id: reverseSettingsState.default_openai_account_id,
    });
  }

  async function handleReverseSetDefault(kind: "copilot" | "openai", accountId: string) {
    if (kind === "copilot") {
      await copilotSetDefaultAccount(accountId);
    }
    await persistReverseProxySettings({
      enabled: reverseSettingsState?.enabled ?? false,
      default_copilot_account_id:
        kind === "copilot" ? accountId : reverseSettingsState?.default_copilot_account_id ?? null,
      default_openai_account_id:
        kind === "openai" ? accountId : reverseSettingsState?.default_openai_account_id ?? null,
    });
    if (kind === "copilot") {
      await onManagedCopilotChanged();
    }
  }

  async function handleCopilotAddAccount() {
    setProxyError(null);
    setCopilotPolling(false);
    try {
      const deviceCode = await startCopilotOAuthDeviceFlow();
      setCopilotDeviceCode(deviceCode);
      setCopilotPolling(true);
    } catch (error) {
      setProxyError(errorMessage(error, "启动 Copilot OAuth 失败"));
    }
  }

  async function handleCopilotPollOnce(deviceCode = copilotDeviceCode?.device_code ?? null) {
    if (!deviceCode) {
      return;
    }
    try {
      const account = await pollCopilotOAuthAccount(deviceCode);
      if (account) {
        setCopilotPolling(false);
        setCopilotDeviceCode(null);
        await reloadProxyState();
        await onManagedCopilotChanged();
      }
    } catch (error) {
      setCopilotPolling(false);
      setProxyError(errorMessage(error, "Copilot OAuth 认证失败"));
    }
  }

  async function handleCopilotRemoveAccount(accountId: string) {
    setProxyError(null);
    try {
      const removingDefault = reverseSettingsState?.default_copilot_account_id === accountId;
      await copilotRemoveAccount(accountId);
      if (removingDefault) {
        const remainingAccounts = await copilotListAccounts();
        await persistReverseProxySettings({
          enabled: reverseSettingsState?.enabled ?? false,
          default_copilot_account_id: remainingAccounts[0]?.id ?? null,
          default_openai_account_id: reverseSettingsState?.default_openai_account_id ?? null,
        });
      } else {
        await reloadProxyState();
      }
      await onManagedCopilotChanged();
    } catch (error) {
      setProxyError(errorMessage(error, "移除 Copilot 账号失败"));
    }
  }

  async function handleCopyCopilotDeviceValue(target: "user_code" | "verification_uri") {
    if (!copilotDeviceCode) {
      return;
    }
    try {
      setProxyError(null);
      await copyCopilotDeviceValue(navigator.clipboard, copilotDeviceCode, target);
    } catch (error) {
      setProxyError(errorMessage(error, "复制失败"));
    }
  }

  function handleUsageRangeChange(option: UsageRangeOption) {
    setUsageRangeUiState((current) => selectUsageRangeOption(current, option));
  }

  function handleCustomRangeChange(field: "startDate" | "endDate", value: string) {
    setUsageRangeUiState((current) => updateCustomRangeDraft(current, field, value));
  }

  function handleCustomRangeApply() {
    if (usageRangeError) {
      return;
    }
    setUsageRangeUiState((current) => applyCustomRangeDraft(current));
  }

  const accounts = connectedAccounts(settings, status);
  const requestedTokenReport = tokenReportsByRange[usageRangeKey] ?? null;
  const requestedGitReport = gitReportsByRange[usageRangeKey] ?? null;
  const requestedKpiReport = kpiReportsByRange[usageRangeKey] ?? null;
  const lastReadyTokenReport = lastReadyTokenRangeKey ? tokenReportsByRange[lastReadyTokenRangeKey] ?? null : null;
  const lastReadyGitReport = lastReadyGitRangeKey ? gitReportsByRange[lastReadyGitRangeKey] ?? null : null;
  const lastReadyKpiReport = lastReadyKpiRangeKey ? kpiReportsByRange[lastReadyKpiRangeKey] ?? null : null;
  const tokenVisibleState = resolveVisibleReportState(
    usageRangeKey,
    requestedTokenReport,
    lastReadyTokenRangeKey,
    lastReadyTokenReport,
  );
  const gitVisibleState = resolveVisibleReportState(
    usageRangeKey,
    requestedGitReport,
    lastReadyGitRangeKey,
    lastReadyGitReport,
  );
  const kpiVisibleState = resolveVisibleReportState(
    usageRangeKey,
    requestedKpiReport,
    lastReadyKpiRangeKey,
    lastReadyKpiReport,
  );
  const tokenReport = tokenVisibleState.visibleKey ? tokenReportsByRange[tokenVisibleState.visibleKey] ?? null : null;
  const gitReport = gitVisibleState.visibleKey ? gitReportsByRange[gitVisibleState.visibleKey] ?? null : null;
  const kpiReport = kpiVisibleState.visibleKey ? kpiReportsByRange[kpiVisibleState.visibleKey] ?? null : null;
  const tokenPreparing = Boolean(requestedTokenReport?.pending) || tokenVisibleState.showingFallback;
  const gitPreparing = Boolean(requestedGitReport?.pending) || gitVisibleState.showingFallback;
  const kpiPreparing = Boolean(requestedKpiReport?.pending) || kpiVisibleState.showingFallback;

  return (
    <Card className="settings-panel">
      <div className="add-header">
        <Button variant="ghost" size="icon-sm" className="icon-button back-button" onClick={onBack} aria-label="返回">
          <ArrowLeft data-icon="inline-start" />
        </Button>
        <h1>设置</h1>
      </div>

      <div className="settings-tabs" role="tablist" aria-label="设置分类">
        <button
          type="button"
          className={`settings-tab ${activeTab === "quota" ? "settings-tab-active" : ""}`}
          role="tab"
          aria-selected={activeTab === "quota"}
          onClick={() => setActiveTab("quota")}
        >
          额度及账号
        </button>
        <button
          type="button"
          className={`settings-tab ${activeTab === "tokens" ? "settings-tab-active" : ""}`}
          role="tab"
          aria-selected={activeTab === "tokens"}
          onClick={() => setActiveTab("tokens")}
        >
          统计
        </button>
        <button
          type="button"
          className={`settings-tab ${activeTab === "proxy" ? "settings-tab-active" : ""}`}
          role="tab"
          aria-selected={activeTab === "proxy"}
          onClick={() => setActiveTab("proxy")}
        >
          代理
        </button>
      </div>

      {activeTab === "quota" ? (
        <>
      <section className="settings-section">
        <div className="section-header">
          <h2>已连接账号</h2>
          <Button className="settings-add-button" onClick={onAddAccount}>
            添加账号
          </Button>
        </div>
        {accounts.length > 0 ? (
          accounts.map((account) => (
            <Card className="account-card settings-account-card" key={account.account_id}>
              <div className="account-line">
                <span className="settings-account-title">
                  <ProviderIcon {...providerIconConfig(account.provider)} />
                  <span className="account-title">{providerDisplayName(account.provider)}</span>
                </span>
                <span className="account-status">已授权</span>
              </div>
              <div className="account-subtitle">{connectedAccountSubtitle(account)}</div>
              <div className="account-actions">
                {isOAuthProvider(account.provider as ProviderKey) ||
                account.provider === "kimi" ||
                account.provider === "glm" ||
                account.provider === "minimax" ||
                account.provider === "copilot" ? (
                  <Button
                    variant="secondary"
                    className="account-action-button"
                    onClick={() => onReauthorize(account)}
                  >
                    {isManagedCopilotAccountId(account.account_id)
                      ? "管理授权"
                      : account.provider === "kimi"
                      ? "重新导入"
                      : account.provider === "copilot"
                        ? "更新 Token"
                        : account.provider === "glm" || account.provider === "minimax"
                        ? "更新 Key"
                        : "重新授权"}
                  </Button>
                ) : null}
                {!isManagedCopilotAccountId(account.account_id) ? (
                  <Button className="account-delete-button" onClick={() => onDeleteAccount(account)}>
                    删除
                  </Button>
                ) : null}
              </div>
            </Card>
          ))
        ) : (
          <div className="settings-empty-account">暂无已连接账号</div>
        )}
      </section>

      <div className="separator" />

      <section className="settings-section compact-section">
        <h2>刷新设置</h2>
        <div className="split-row">
          <span>自动刷新周期</span>
          <select
            className="refresh-select"
            aria-label="自动刷新周期"
            value={form.refresh_interval_minutes}
            onChange={(event) => onChange({ ...form, refresh_interval_minutes: Number(event.target.value) })}
          >
            {refreshIntervalOptions.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </div>
      </section>

      <div className="separator" />

      <section className="settings-section compact-section">
        <h2>启动设置</h2>
        <div className="split-row">
          <span>登录时自动启动</span>
          <Switch
            aria-label="登录时自动启动"
            className="settings-switch"
            size="panel"
            checked={form.launch_at_login}
            onCheckedChange={(checked) => onChange({ ...form, launch_at_login: checked })}
          />
        </div>
      </section>

      <div className="separator" />

      <UpdateSection
        availableUpdate={availableUpdate}
        checking={updateChecking}
        installing={updateInstalling}
        downloadPercent={updateDownloadPercent}
        message={updateMessage}
        error={updateError}
        onCheck={onCheckForUpdate}
        onInstall={onInstallUpdate}
      />

      <div className="separator" />

      <section className="settings-section compact-section">
        <h2>通知设置</h2>
        <div className="split-row">
          <span>低额度提醒</span>
          <Switch
            aria-label="低额度提醒"
            className="settings-switch"
            size="panel"
            checked={form.notify_on_low_quota}
            onCheckedChange={(checked) => onChange({ ...form, notify_on_low_quota: checked })}
          />
        </div>
        <div className="split-row">
          <span>提醒阈值</span>
          <label className="threshold-field">
            <input
              aria-label="提醒阈值"
              className="threshold-input"
              inputMode="numeric"
              pattern="[0-9]*"
              value={thresholdDraft}
              onChange={(event) => {
                const nextValue = event.target.value;
                if (!/^\d{0,3}$/.test(nextValue)) {
                  return;
                }
                setThresholdDraft(nextValue);
                if (nextValue !== "") {
                  commitThreshold(nextValue);
                }
              }}
              onBlur={() => commitThreshold(thresholdDraft)}
            />
            <span>%</span>
          </label>
        </div>
        <div className="split-row split-row-disabled">
          <span>重置提醒</span>
          <Switch
            aria-label="重置提醒"
            className="settings-switch"
            size="panel"
            checked={false}
            disabled
          />
        </div>
      </section>
        </>
      ) : activeTab === "tokens" ? (
        <TokenUsagePanel
          report={tokenReport}
          gitReport={gitReport}
          kpiReport={kpiReport}
          selectedRangeOption={selectedRangeOption}
          customRangeDraft={customRangeDraft}
          customRangeBounds={customRangeBounds}
          rangeError={usageRangeError}
          activeUsageTab={activeUsageTab}
          gitUsageRoot={gitRootDraft}
          gitRootPicking={gitRootPicking}
          loading={tokenLoading}
          gitLoading={gitLoading}
          kpiLoading={kpiLoading}
          preparing={tokenPreparing}
          gitPreparing={gitPreparing}
          kpiPreparing={kpiPreparing}
          refreshing={tokenRefreshing}
          gitRefreshing={gitRefreshing}
          kpiRefreshing={kpiRefreshing}
          error={tokenError}
          gitError={gitError}
          kpiError={kpiError}
          onRangeChange={handleUsageRangeChange}
          onCustomRangeChange={handleCustomRangeChange}
          onCustomRangeApply={handleCustomRangeApply}
          onUsageTabChange={setActiveUsageTab}
          onGitUsageRootDraftChange={setGitRootDraft}
          onGitUsageRootCommit={commitGitUsageRoot}
          onGitUsageRootPick={handlePickGitUsageRoot}
          onRefresh={handleTokenRefresh}
          onGitRefresh={handleGitRefresh}
          onKpiRefresh={handleKpiRefresh}
        />
      ) : (
        <LocalProxyPanel
          proxySubTab={proxySubTab}
          settingsState={proxySettingsState}
          status={proxyStatus}
          reverseSettingsState={reverseSettingsState}
          reverseStatus={reverseStatus}
          loading={proxyLoading}
          reverseLoading={reverseLoading}
          saving={proxySaving}
          reverseSaving={reverseSaving}
          actionPending={proxyActionPending}
          profileSaving={proxyProfileSaving}
          error={proxyError}
          reverseManagerOpen={reverseManagerOpen}
          copilotAuthStatus={copilotAuthStatus}
          copilotDeviceCode={copilotDeviceCode}
          copilotPolling={copilotPolling}
          editingAccountId={proxyEditorAccountId}
          editingBaseUrl={proxyEditorBaseUrl}
          editingApiFormat={proxyEditorApiFormat}
          editingAuthField={proxyEditorAuthField}
          editingApiKey={proxyEditorApiKey}
          routeEditingId={routeEditorRouteId}
          routeEditingPattern={routeEditorPattern}
          routeEditingAccountId={routeEditorAccountId}
          routeEditingEnabled={routeEditorEnabled}
          onSubTabChange={setProxySubTab}
          onConfigChange={updateProxyConfig}
          onConfigCommit={persistProxyConfig}
          onToggle={handleProxyToggle}
          onReverseToggle={handleReverseProxyToggle}
          onAddRoute={() => openProxyRouteEditor(null)}
          onDeleteRoute={handleProxyDeleteRoute}
          onOpenProfileEditor={openProxyProfileEditor}
          onOpenRouteEditor={openProxyRouteEditor}
          onOpenReverseManager={setReverseManagerOpen}
          onCloseReverseManager={() => setReverseManagerOpen(null)}
          onReverseSetDefault={handleReverseSetDefault}
          onCopilotAddAccount={handleCopilotAddAccount}
          onCopilotRemoveAccount={handleCopilotRemoveAccount}
          onCopyCopilotDeviceValue={handleCopyCopilotDeviceValue}
          onOpenAIOAuthAccount={onAddReverseOpenAIOAuth}
          onCloseProfileEditor={() => setProxyEditorAccountId(null)}
          onEditingBaseUrlChange={setProxyEditorBaseUrl}
          onEditingApiFormatChange={setProxyEditorApiFormat}
          onEditingAuthFieldChange={setProxyEditorAuthField}
          onEditingApiKeyChange={setProxyEditorApiKey}
          onProfileSave={handleProxyProfileSave}
          onCloseRouteEditor={() => {
            setRouteEditorRouteId(null);
            setRouteEditorPattern("");
            setRouteEditorAccountId("");
            setRouteEditorEnabled(true);
          }}
          onRouteEditingPatternChange={setRouteEditorPattern}
          onRouteEditingAccountIdChange={setRouteEditorAccountId}
          onRouteEditingEnabledChange={setRouteEditorEnabled}
          onRouteSave={handleProxySaveRoute}
        />
      )}

      {activeTab === "quota" && message ? <div className="settings-message">{message}</div> : null}
    </Card>
  );
}

function UpdateSection({
  availableUpdate,
  checking,
  installing,
  downloadPercent,
  message,
  error,
  onCheck,
  onInstall,
}: {
  availableUpdate: AppUpdateInfo | null;
  checking: boolean;
  installing: boolean;
  downloadPercent: number | null;
  message: string | null;
  error: string | null;
  onCheck: () => void;
  onInstall: () => void;
}) {
  const installLabel = installing
    ? downloadPercent != null
      ? `下载中 ${downloadPercent}%`
      : "正在安装…"
    : "立即更新";
  const publishedAt = formatUpdatePublishedAt(availableUpdate?.date ?? null);

  return (
    <section className={`settings-section compact-section ${availableUpdate ? "update-section-available" : ""}`}>
      <div className="section-header">
        <h2>应用更新</h2>
        <Button
          variant="secondary"
          className="settings-add-button"
          onClick={onCheck}
          disabled={checking || installing}
        >
          {checking ? "检查中..." : "检查更新"}
        </Button>
      </div>

      {availableUpdate ? (
        <>
          <div className="update-summary">
            <span className="update-version-badge">新版本 {availableUpdate.version}</span>
            {publishedAt ? <span className="muted-copy">发布于 {publishedAt}</span> : null}
          </div>
          <p className="update-notes">
            {availableUpdate.body || "检测到可安装的新版本，安装完成后应用会自动重启。"}
          </p>
          <Button onClick={onInstall} disabled={checking || installing}>
            {installLabel}
          </Button>
        </>
      ) : (
        <p className="muted-copy">已启用自动检查更新，启动后和运行中每 6 小时会自动检查一次。</p>
      )}

      {message ? <div className="update-status">{message}</div> : null}
      {error ? <div className="inline-error">{error}</div> : null}
    </section>
  );
}

function TokenUsagePanel({
  report,
  gitReport,
  kpiReport,
  selectedRangeOption,
  customRangeDraft,
  customRangeBounds,
  rangeError,
  activeUsageTab,
  gitUsageRoot,
  gitRootPicking,
  loading,
  gitLoading,
  kpiLoading,
  preparing,
  gitPreparing,
  kpiPreparing,
  refreshing,
  gitRefreshing,
  kpiRefreshing,
  error,
  gitError,
  kpiError,
  onRangeChange,
  onCustomRangeChange,
  onCustomRangeApply,
  onUsageTabChange,
  onGitUsageRootDraftChange,
  onGitUsageRootCommit,
  onGitUsageRootPick,
  onRefresh,
  onGitRefresh,
  onKpiRefresh,
}: {
  report: LocalTokenUsageReport | null;
  gitReport: GitUsageReport | null;
  kpiReport: PrKpiReport | null;
  selectedRangeOption: UsageRangeOption;
  customRangeDraft: Extract<UsageRangeSelection, { kind: "custom" }>;
  customRangeBounds: { minDate: string; maxDate: string };
  rangeError: string | null;
  activeUsageTab: SettingsUsageTab;
  gitUsageRoot: string;
  gitRootPicking: boolean;
  loading: boolean;
  gitLoading: boolean;
  kpiLoading: boolean;
  preparing: boolean;
  gitPreparing: boolean;
  kpiPreparing: boolean;
  refreshing: boolean;
  gitRefreshing: boolean;
  kpiRefreshing: boolean;
  error: string | null;
  gitError: string | null;
  kpiError: string | null;
  onRangeChange: (range: UsageRangeOption) => void;
  onCustomRangeChange: (field: "startDate" | "endDate", value: string) => void;
  onCustomRangeApply: () => void;
  onUsageTabChange: (tab: SettingsUsageTab) => void;
  onGitUsageRootDraftChange: (value: string) => void;
  onGitUsageRootCommit: (value: string) => void;
  onGitUsageRootPick: () => void;
  onRefresh: () => void;
  onGitRefresh: () => void;
  onKpiRefresh: () => void;
}) {
  const chartRows = report ? buildTokenUsageChartRows(report) : [];
  const chartLegend = report ? buildTokenUsageChartLegend(report) : [];
  const modelRows = report ? modelUsageRows(report) : [];
  const tools = report?.tools.filter((tool) => tool.total_tokens > 0) ?? [];
  const notices = [
    ...(report?.warnings ?? []),
    ...(report?.missing_sources.map((source) => `未找到 ${source}`) ?? []),
  ];

  return (
    <section className="token-usage-panel">
      <div className="token-range-selector" aria-label="Token 用量时间范围">
        {tokenUsageRangeOptions.map((option) => (
          <button
            key={option}
            type="button"
            className={`token-range-button ${selectedRangeOption === option ? "token-range-button-active" : ""}`}
            onClick={() => onRangeChange(option)}
          >
            {tokenUsageRangeLabels[option]}
          </button>
        ))}
      </div>

      {selectedRangeOption === "custom" ? (
        <div className="usage-custom-range-row">
          <input
            type="date"
            aria-label="开始日期"
            value={customRangeDraft.startDate}
            min={customRangeBounds.minDate}
            max={customRangeDraft.endDate}
            onChange={(event) => onCustomRangeChange("startDate", event.target.value)}
          />
          <span>至</span>
          <input
            type="date"
            aria-label="结束日期"
            value={customRangeDraft.endDate}
            min={customRangeDraft.startDate}
            max={customRangeBounds.maxDate}
            onChange={(event) => onCustomRangeChange("endDate", event.target.value)}
          />
          <Button
            type="button"
            variant="secondary"
            size="sm"
            className="usage-apply-button"
            disabled={Boolean(rangeError)}
            onClick={onCustomRangeApply}
          >
            查询
          </Button>
        </div>
      ) : null}

      {rangeError ? <div className="inline-error">{rangeError}</div> : null}

      <div className="usage-subtabs" role="tablist" aria-label="统计类型">
        <button
          type="button"
          className={`usage-subtab ${activeUsageTab === "token" ? "usage-subtab-active" : ""}`}
          role="tab"
          aria-selected={activeUsageTab === "token"}
          onClick={() => onUsageTabChange("token")}
        >
          Token 用量统计
        </button>
        <span className="usage-subtab-divider" aria-hidden="true" />
        <button
          type="button"
          className={`usage-subtab ${activeUsageTab === "git" ? "usage-subtab-active" : ""}`}
          role="tab"
          aria-selected={activeUsageTab === "git"}
          onClick={() => onUsageTabChange("git")}
        >
          Git 提交统计
        </button>
        <span className="usage-subtab-divider" aria-hidden="true" />
        <button
          type="button"
          className={`usage-subtab ${activeUsageTab === "kpi" ? "usage-subtab-active" : ""}`}
          role="tab"
          aria-selected={activeUsageTab === "kpi"}
          onClick={() => onUsageTabChange("kpi")}
        >
          KPI 分析
        </button>
      </div>

      {activeUsageTab === "token" ? (
        <>
          {error ? <div className="inline-error">{error}</div> : null}
          {preparing ? <div className="usage-preparing">正在准备所选时间范围，当前先展示最近一次可用结果</div> : null}
          {loading && !report ? <div className="token-loading">读取本地日志...</div> : null}

          {report ? (
            <>
              <TokenUsageSummary totals={report.totals} />
              {report.totals.total_tokens === 0 && !report.pending ? (
                <div className="token-empty">当前时间范围暂无 Token 用量数据</div>
              ) : null}

          <section className="usage-section">
            <div className="token-section-header">
              <h2>Token 用量趋势</h2>
              <div className="token-legend" aria-label="模型图例">
                {chartLegend.map((item) => (
                  <span className={`token-legend-item ${item.colorClass}`} key={item.model}>
                    {item.label}
                  </span>
                ))}
              </div>
            </div>
            <div
              className={`token-chart token-chart-range-${report.range}`}
              style={chartColumnCountStyle(chartRows.length)}
              aria-label="每日 Token 趋势"
            >
              {chartRows.map((row) => (
                <div className="token-chart-column" key={row.date}>
                  <div className="token-chart-bar" title={row.label}>
                    {row.segments.map((segment) => (
                      <span
                        className={`token-chart-segment ${segment.colorClass}`}
                        key={segment.model}
                        style={tokenSegmentStyle(segment.height)}
                        title={`${segment.model} ${formatCompactTokens(segment.totalTokens)}`}
                      />
                    ))}
                  </div>
                  <span>{row.label}</span>
                </div>
              ))}
            </div>
          </section>

          <section className="usage-section">
            <div className="token-section-header">
              <h2>模型用量排行</h2>
            </div>
            {modelRows.length > 0 ? (
              <div className="token-model-list">
                {modelRows.map((model) => (
                  <div className="token-model-row" key={model.model}>
                    <div className="token-model-row-header">
                      <span>{model.model}</span>
                      <strong>{model.displayTotal}</strong>
                    </div>
                    <div className="token-model-progress">
                      <span style={{ width: `${model.percent}%` }} />
                    </div>
                    <div className="token-model-breakdown" aria-label={`${model.model} Token 构成`}>
                      <span>
                        输入
                        <strong>{model.displayInput}</strong>
                      </span>
                      <span>
                        输出
                        <strong>{model.displayOutput}</strong>
                      </span>
                      <span>
                        缓存命中
                        <strong>{model.displayCacheRead}</strong>
                      </span>
                      <span>
                        存储缓存
                        <strong>{model.displayCacheCreation}</strong>
                      </span>
                    </div>
                  </div>
                ))}
              </div>
            ) : (
              <div className="token-empty">暂无模型数据</div>
            )}
          </section>

          {tools.length > 0 ? (
            <section className="token-tool-strip" aria-label="工具来源">
              {tools.map((tool) => (
                <span key={tool.tool}>
                  {usageToolLabel(tool.tool)}
                  <strong>{formatCompactTokens(tool.total_tokens)}</strong>
                </span>
              ))}
            </section>
          ) : null}

          {notices.length > 0 ? (
            <div className="token-warning-list">
              {notices.slice(0, 4).map((notice) => (
                <div key={notice}>{notice}</div>
              ))}
            </div>
          ) : null}

          <div className="token-footer">
            <div className="token-generated-at">更新于 {formatGeneratedAt(report.generated_at)}</div>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              className="token-refresh-button"
              disabled={refreshing}
              onClick={onRefresh}
            >
              <RefreshCw
                data-icon="inline-start"
                className={refreshing ? "refresh-icon-spinning" : undefined}
              />
              {refreshing ? "计算中" : "刷新用量"}
            </Button>
          </div>
            </>
          ) : null}
        </>
      ) : activeUsageTab === "git" ? (
        <GitUsageSection
          report={gitReport}
          gitUsageRoot={gitUsageRoot}
          gitRootPicking={gitRootPicking}
          loading={gitLoading}
          preparing={gitPreparing}
          refreshing={gitRefreshing}
          error={gitError}
          onGitUsageRootDraftChange={onGitUsageRootDraftChange}
          onGitUsageRootCommit={onGitUsageRootCommit}
          onGitUsageRootPick={onGitUsageRootPick}
          onRefresh={onGitRefresh}
        />
      ) : (
        <KpiUsageSection
          report={kpiReport}
          loading={kpiLoading}
          preparing={kpiPreparing}
          refreshing={kpiRefreshing}
          error={kpiError}
          onRefresh={onKpiRefresh}
        />
      )}
    </section>
  );
}

function GitUsageSection({
  report,
  gitUsageRoot,
  gitRootPicking,
  loading,
  preparing,
  refreshing,
  error,
  onGitUsageRootDraftChange,
  onGitUsageRootCommit,
  onGitUsageRootPick,
  onRefresh,
}: {
  report: GitUsageReport | null;
  gitUsageRoot: string;
  gitRootPicking: boolean;
  loading: boolean;
  preparing: boolean;
  refreshing: boolean;
  error: string | null;
  onGitUsageRootDraftChange: (value: string) => void;
  onGitUsageRootCommit: (value: string) => void;
  onGitUsageRootPick: () => void;
  onRefresh: () => void;
}) {
  const chartRows = report ? buildGitUsageChartRows(report) : [];
  const metrics = report ? gitUsageSummaryMetrics(report) : [];
  const repositoryRows = report ? repositoryUsageRows(report) : [];
  const notices = [
    ...(report?.warnings ?? []),
    ...(report?.missing_sources.map((source) => `未找到 ${source}`) ?? []),
  ];

  return (
    <section className="git-usage-section">
      {error ? <div className="inline-error">{error}</div> : null}
      {preparing ? <div className="usage-preparing">正在准备所选时间范围，当前先展示最近一次可用结果</div> : null}
      {loading && !report ? <div className="token-loading">读取 Git 统计缓存...</div> : null}

      {report ? (
        <>
          <section className="token-card git-summary-card">
            <h2>提交概览</h2>
            <div className="git-summary-grid">
              {metrics.map((metric) => (
                <TokenUsageMetric
                  key={metric.label}
                  label={metric.label}
                  value={metric.value}
                  tone={metric.tone}
                />
              ))}
            </div>
          </section>

          {report.totals.added_lines + report.totals.deleted_lines === 0 && !report.pending ? (
            <div className="token-empty">当前时间范围暂无 Git 提交统计</div>
          ) : null}

          <section className="usage-section">
            <div className="token-section-header">
              <h2>代码行数趋势</h2>
              <div className="token-legend" aria-label="代码行数图例">
                <span className="token-legend-item git-added-legend">新增</span>
                <span className="token-legend-item git-deleted-legend">删除</span>
              </div>
            </div>
            <div
              className={`git-chart git-chart-range-${report.range}`}
              style={chartColumnCountStyle(chartRows.length)}
              aria-label="代码行数趋势"
            >
              {chartRows.map((row) => (
                <div className="git-chart-column" key={row.date}>
                  <div className="git-chart-bars" title={row.label}>
                    <span
                      className="git-chart-bar git-chart-added"
                      style={gitBarStyle(row.addedHeight)}
                      title={`新增 ${formatCompactLines(row.addedLines)}`}
                    />
                    <span
                      className="git-chart-bar git-chart-deleted"
                      style={gitBarStyle(row.deletedHeight)}
                      title={`删除 ${formatCompactLines(row.deletedLines)}`}
                    />
                  </div>
                  <span>{row.label}</span>
                </div>
              ))}
            </div>
          </section>

          {repositoryRows.length > 0 ? (
            <section className="usage-section git-repository-section">
              <div className="token-section-header">
                <h2>项目提交量排行</h2>
              </div>
              <div className="git-repository-list">
                {repositoryRows.map((repository) => (
                  <div className="git-repository-row" key={repository.path}>
                    <div className="git-repository-row-header">
                      <span>{repository.name}</span>
                      <strong>
                        <span className="git-repository-added">{repository.displayAdded}</span>
                        <span className="git-repository-deleted">/ {repository.displayDeleted}</span>
                      </strong>
                    </div>
                    <div className="git-repository-progress">
                      <span
                        className="git-repository-progress-added"
                        style={{ width: `${repository.addedPercent}%` }}
                      />
                      <span
                        className="git-repository-progress-deleted"
                        style={{ width: `${repository.deletedPercent}%` }}
                      />
                    </div>
                  </div>
                ))}
              </div>
            </section>
          ) : null}

          {notices.length > 0 ? (
            <div className="token-warning-list">
              {notices.slice(0, 4).map((notice) => (
                <div key={notice}>{notice}</div>
              ))}
            </div>
          ) : null}

          <div className="git-root-field">
            <label htmlFor="git-usage-root">统计路径</label>
            <div className="git-root-control">
              <input
                id="git-usage-root"
                value={gitUsageRoot}
                onChange={(event) => onGitUsageRootDraftChange(event.target.value)}
                onBlur={() => onGitUsageRootCommit(gitUsageRoot)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.currentTarget.blur();
                  }
                }}
              />
              <Button
                type="button"
                variant="secondary"
                size="icon-sm"
                className="git-root-pick-button"
                aria-label="选择 Git 统计路径"
                disabled={gitRootPicking}
                onClick={onGitUsageRootPick}
              >
                <FolderOpen />
              </Button>
            </div>
          </div>

          <div className="token-footer">
            <div className="token-generated-at">更新于 {formatGeneratedAt(report.generated_at)}</div>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              className="token-refresh-button"
              disabled={refreshing}
              onClick={onRefresh}
            >
              <RefreshCw
                data-icon="inline-start"
                className={refreshing ? "refresh-icon-spinning" : undefined}
              />
              {refreshing ? "计算中" : "刷新 Git 统计"}
            </Button>
          </div>
        </>
      ) : null}
    </section>
  );
}

function KpiUsageSection({
  report,
  loading,
  preparing,
  refreshing,
  error,
  onRefresh,
}: {
  report: PrKpiReport | null;
  loading: boolean;
  preparing: boolean;
  refreshing: boolean;
  error: string | null;
  onRefresh: () => void;
}) {
  const radar = report ? buildPrKpiRadarModel(report) : null;
  const notices = [
    ...(report?.warnings ?? []),
    ...(report?.missing_sources.map((source) => `未找到 ${source}`) ?? []),
  ];

  return (
    <section className="kpi-usage-section">
      {error ? <div className="inline-error">{error}</div> : null}
      {preparing ? <div className="usage-preparing">正在准备所选时间范围，当前先展示最近一次可用结果</div> : null}
      {loading && !report ? <div className="token-loading">读取 KPI 分析缓存...</div> : null}

      {report ? (
        <>
          <section className="token-card kpi-summary-card">
            <div className="kpi-summary-heading">
              <h2>效率概览</h2>
              <div className="kpi-summary-help">
                <button
                  type="button"
                  className="kpi-summary-help-trigger"
                  aria-label="查看效率概览说明"
                  aria-describedby="kpi-summary-tooltip"
                >
                  <Info />
                </button>
                <div id="kpi-summary-tooltip" role="tooltip" className="kpi-summary-tooltip">
                  <div>代码行数 = 代码增行数 + 代码减行数</div>
                  <div>Token 总用量 = 输入 + 输出 + 写入缓存 + 缓存命中 / 10</div>
                  <div>产出比 = 代码净增行数 / KPI Token 总用量(千)</div>
                </div>
              </div>
            </div>
            <div className="kpi-summary-grid">
              <TokenUsageMetric
                label="Token 总用量"
                value={formatPrKpiOverviewValue(report.overview.token_total)}
              />
              <TokenUsageMetric
                label="代码行数"
                value={formatPrKpiOverviewValue(report.overview.code_lines)}
              />
              <TokenUsageMetric
                label="产出比"
                value={formatPrKpiOutputRatio(report.overview.output_ratio)}
                tone={prKpiOutputRatioTone(report.overview.output_ratio)}
              />
            </div>
          </section>

          <section className="token-card kpi-radar-card">
            <div className="kpi-radar-heading">
              <h2>PR 质量雷达</h2>
              <div className="kpi-radar-help">
                <button
                  type="button"
                  className="kpi-radar-help-trigger"
                  aria-label="查看 PR 质量雷达指标说明"
                  aria-describedby="kpi-radar-tooltip"
                >
                  <FileText />
                </button>
                <div id="kpi-radar-tooltip" role="tooltip" className="kpi-radar-tooltip">
                  {report.metrics.map((metric) => (
                    <div key={metric.key}>{`• ${metric.label}：${prKpiMetricDescriptions[metric.key]}`}</div>
                  ))}
                </div>
              </div>
            </div>
            {radar ? (
              <div className="kpi-radar-wrap">
                <svg
                  className="kpi-radar-svg"
                  viewBox="0 0 280 280"
                  role="img"
                  aria-label="PR 质量雷达图"
                >
                  {radar.gridPolygons.map((points, index) => (
                    <polygon
                      key={points}
                      points={points}
                      className={`kpi-radar-grid kpi-radar-grid-${index}`}
                    />
                  ))}
                  {radar.axes.map((axis) => (
                    <line
                      key={`axis-${axis.key}`}
                      x1={radar.center}
                      y1={radar.center}
                      x2={radar.center + Math.cos(axis.angle) * radar.radius}
                      y2={radar.center + Math.sin(axis.angle) * radar.radius}
                      className="kpi-radar-axis"
                    />
                  ))}
                  <polygon points={radar.polygonPoints} className="kpi-radar-shape" />
                  {radar.axes.map((axis) => (
                    <circle
                      key={`point-${axis.key}`}
                      cx={axis.pointX}
                      cy={axis.pointY}
                      r={axis.is_missing ? 3 : 4}
                      className={`kpi-radar-point ${axis.is_missing ? "kpi-radar-point-missing" : ""}`}
                    >
                      <title>{`${axis.label}: ${axis.display_value}`}</title>
                    </circle>
                  ))}
                  {radar.axes.map((axis) => (
                    <text
                      key={`label-${axis.key}`}
                      x={axis.labelX}
                      y={axis.labelY}
                      textAnchor={prKpiAxisAnchor(axis.labelX, radar.center)}
                      className="kpi-radar-label"
                    >
                      <tspan x={axis.labelX} dy="0">
                        {axis.label}
                      </tspan>
                      <tspan x={axis.labelX} dy="14" className="kpi-radar-label-subtle">
                        {axis.displayValue}
                      </tspan>
                    </text>
                  ))}
                </svg>
              </div>
            ) : null}
            {radar?.overallScoreLabel ? (
              <div className="kpi-overall-note">综合分仅基于有数据的维度计算：{radar.overallScoreLabel}</div>
            ) : null}
          </section>

          {notices.length > 0 ? (
            <div className="token-warning-list">
              {notices.slice(0, 4).map((notice) => (
                <div key={notice}>{notice}</div>
              ))}
            </div>
          ) : null}

          <div className="token-footer">
            <div className="token-generated-at">更新于 {formatGeneratedAt(report.generated_at)}</div>
            <Button
              type="button"
              variant="secondary"
              size="sm"
              className="token-refresh-button"
              disabled={refreshing}
              onClick={onRefresh}
            >
              <RefreshCw
                data-icon="inline-start"
                className={refreshing ? "refresh-icon-spinning" : undefined}
              />
              {refreshing ? "计算中" : "刷新 KPI"}
            </Button>
          </div>
        </>
      ) : null}
    </section>
  );
}

function LocalProxyPanel({
  proxySubTab,
  settingsState,
  status,
  reverseSettingsState,
  reverseStatus,
  loading,
  reverseLoading,
  saving,
  reverseSaving,
  actionPending,
  profileSaving,
  error,
  reverseManagerOpen,
  copilotAuthStatus,
  copilotDeviceCode,
  copilotPolling,
  editingAccountId,
  editingBaseUrl,
  editingApiFormat,
  editingAuthField,
  editingApiKey,
  routeEditingId,
  routeEditingPattern,
  routeEditingAccountId,
  routeEditingEnabled,
  onSubTabChange,
  onConfigChange,
  onConfigCommit,
  onToggle,
  onReverseToggle,
  onAddRoute,
  onDeleteRoute,
  onOpenProfileEditor,
  onOpenRouteEditor,
  onOpenReverseManager,
  onCloseReverseManager,
  onReverseSetDefault,
  onCopilotAddAccount,
  onCopilotRemoveAccount,
  onCopyCopilotDeviceValue,
  onOpenAIOAuthAccount,
  onCloseProfileEditor,
  onEditingBaseUrlChange,
  onEditingApiFormatChange,
  onEditingAuthFieldChange,
  onEditingApiKeyChange,
  onProfileSave,
  onCloseRouteEditor,
  onRouteEditingPatternChange,
  onRouteEditingAccountIdChange,
  onRouteEditingEnabledChange,
  onRouteSave,
}: {
  proxySubTab: ProxySubTab;
  settingsState: LocalProxySettingsState | null;
  status: LocalProxyStatus | null;
  reverseSettingsState: ReverseProxySettingsState | null;
  reverseStatus: ReverseProxyStatus | null;
  loading: boolean;
  reverseLoading: boolean;
  saving: boolean;
  reverseSaving: boolean;
  actionPending: boolean;
  profileSaving: boolean;
  error: string | null;
  reverseManagerOpen: "copilot" | "openai" | null;
  copilotAuthStatus: CopilotAuthStatus | null;
  copilotDeviceCode: GitHubDeviceCodeResponse | null;
  copilotPolling: boolean;
  editingAccountId: string | null;
  editingBaseUrl: string;
  editingApiFormat: ClaudeApiFormat;
  editingAuthField: ClaudeAuthField;
  editingApiKey: string;
  routeEditingId: string | null;
  routeEditingPattern: string;
  routeEditingAccountId: string;
  routeEditingEnabled: boolean;
  onSubTabChange: (tab: ProxySubTab) => void;
  onConfigChange: (config: LocalProxySettingsState["config"]) => void;
  onConfigCommit: (config: LocalProxySettingsState["config"]) => Promise<void>;
  onToggle: (checked: boolean) => Promise<void>;
  onReverseToggle: (checked: boolean) => Promise<void>;
  onAddRoute: () => void;
  onDeleteRoute: (routeId: string) => Promise<void>;
  onOpenProfileEditor: (capability: ClaudeProxyCapability) => void;
  onOpenRouteEditor: (routeId: string | null) => void;
  onOpenReverseManager: (kind: "copilot" | "openai") => void;
  onCloseReverseManager: () => void;
  onReverseSetDefault: (kind: "copilot" | "openai", accountId: string) => Promise<void>;
  onCopilotAddAccount: () => Promise<void>;
  onCopilotRemoveAccount: (accountId: string) => Promise<void>;
  onCopyCopilotDeviceValue: (target: "user_code" | "verification_uri") => Promise<void>;
  onOpenAIOAuthAccount: () => void;
  onCloseProfileEditor: () => void;
  onEditingBaseUrlChange: (value: string) => void;
  onEditingApiFormatChange: (value: ClaudeApiFormat) => void;
  onEditingAuthFieldChange: (value: ClaudeAuthField) => void;
  onEditingApiKeyChange: (value: string) => void;
  onProfileSave: () => Promise<void>;
  onCloseRouteEditor: () => void;
  onRouteEditingPatternChange: (value: string) => void;
  onRouteEditingAccountIdChange: (value: string) => void;
  onRouteEditingEnabledChange: (value: boolean) => void;
  onRouteSave: () => Promise<void>;
}) {
  const editingCapability =
    editingAccountId && settingsState
      ? settingsState.capabilities.find((capability) => capability.account_id === editingAccountId) ?? null
      : null;
  const reverseManagerAccounts =
    reverseManagerOpen === "copilot"
      ? reverseSettingsState?.copilot_accounts ?? []
      : reverseSettingsState?.openai_accounts ?? [];
  const reverseDefaultAccountId =
    reverseManagerOpen === "copilot"
      ? reverseSettingsState?.default_copilot_account_id ?? null
      : reverseSettingsState?.default_openai_account_id ?? null;

  return (
    <section className="local-proxy-panel">
      <div className="proxy-subtabs" role="tablist" aria-label="代理类型">
        <button
          type="button"
          className={`proxy-subtab ${proxySubTab === "local" ? "proxy-subtab-active" : ""}`}
          onClick={() => onSubTabChange("local")}
        >
          本地代理
        </button>
        <button
          type="button"
          className={`proxy-subtab ${proxySubTab === "reverse" ? "proxy-subtab-active" : ""}`}
          onClick={() => onSubTabChange("reverse")}
        >
          反向代理
        </button>
      </div>

      {error ? <div className="inline-error">{error}</div> : null}
      {loading && !settingsState ? <div className="token-loading">读取代理配置...</div> : null}

      {proxySubTab === "reverse" ? (
        <>
          <section className="token-card proxy-master-card">
            <div className="proxy-master-row">
              <div>
                <h2>反向代理</h2>
                <div className={`proxy-status-text ${reverseStatus?.enabled ? "proxy-status-ready" : "proxy-status-pending"}`}>
                  {reverseStatus?.enabled ? "已启用" : "已停用"}
                </div>
              </div>
              <Switch
                aria-label="反向代理开关"
                className="settings-switch"
                size="panel"
                checked={Boolean(reverseStatus?.enabled)}
                disabled={reverseSaving}
                onCheckedChange={(checked) => void onReverseToggle(checked)}
              />
            </div>
          </section>

          {reverseStatus?.enabled && !status?.running ? (
            <section className="token-card proxy-placeholder-card">
              <h2>本地代理未启动</h2>
              <p>Claude Code 当前访问的是 `127.0.0.1:16555`。仅启用反向代理还不够，还需要在“本地代理”子页把本地代理服务启动起来。</p>
            </section>
          ) : null}

          {reverseLoading && !reverseSettingsState ? <div className="token-loading">读取反向代理状态...</div> : null}

          {reverseSettingsState ? (
            <>
              <section className="token-card proxy-model-config-card">
                <h2>GitHub Copilot</h2>
                <div className="proxy-capability-list">
                  {(reverseSettingsState.copilot_accounts.length > 0
                    ? reverseSettingsState.copilot_accounts
                    : [
                        {
                          id: "empty",
                          login: "暂无已授权账号",
                          avatar_url: null,
                          authenticated_at: 0,
                          domain: "github.com",
                        },
                      ]
                  ).map((account) => (
                    <button
                      key={account.id}
                      type="button"
                      className="proxy-capability-row proxy-capability-row-actionable"
                      onClick={() => onOpenReverseManager("copilot")}
                    >
                      <div className="proxy-capability-copy proxy-capability-copy-compact">
                        <span className="proxy-capability-logo">
                          <ProviderIcon icon={githubCopilotIcon} />
                        </span>
                        <span>{account.login}</span>
                      </div>
                      <span className="proxy-capability-badge proxy-capability-badge-ready">
                        {account.id === "empty" ? "待接入" : "已授权"}
                      </span>
                    </button>
                  ))}
                </div>
              </section>

              <section className="token-card proxy-model-config-card">
                <h2>ChatGPT (Codex OAuth)</h2>
                <div className="proxy-capability-list">
                  {(reverseSettingsState.openai_accounts.length > 0
                    ? reverseSettingsState.openai_accounts
                    : [
                        {
                          id: "empty",
                          login: "暂无已授权账号",
                          avatar_url: null,
                          authenticated_at: 0,
                          domain: null,
                        },
                      ]
                  ).map((account) => (
                    <button
                      key={account.id}
                      type="button"
                      className="proxy-capability-row proxy-capability-row-actionable"
                      onClick={() => onOpenReverseManager("openai")}
                    >
                      <div className="proxy-capability-copy proxy-capability-copy-compact">
                        <span className="proxy-capability-logo">
                          <ProviderIcon {...providerIconConfig("openai")} />
                        </span>
                        <span>{account.login}</span>
                      </div>
                      <span className="proxy-capability-badge proxy-capability-badge-ready">
                        {account.id === "empty" ? "待接入" : "已授权"}
                      </span>
                    </button>
                  ))}
                </div>
              </section>
            </>
          ) : null}
        </>
      ) : null}

      {proxySubTab === "local" && settingsState ? (
        <>
          <section className="token-card proxy-master-card">
            <div className="proxy-master-row">
              <div>
                <h2>本地代理（LLM 聚合）</h2>
                <div className={`proxy-status-text ${status?.running ? "proxy-status-ready" : "proxy-status-pending"}`}>
                  {status?.running ? "运行中" : "已停止"}
                </div>
              </div>
              <Switch
                aria-label="本地代理开关"
                className="settings-switch"
                size="panel"
                checked={Boolean(status?.running)}
                disabled={actionPending}
                onCheckedChange={(checked) => void onToggle(checked)}
              />
            </div>
          </section>

          <section className="token-card proxy-address-card">
            <h2>服务地址</h2>
            {status?.running ? (
              <div className="proxy-address-running">
                <span>{localProxyUrl(status, settingsState)}</span>
                <Button
                  type="button"
                  variant="secondary"
                  size="icon-sm"
                  className="git-root-pick-button"
                  aria-label="复制代理地址"
                  onClick={() => void navigator.clipboard?.writeText(localProxyUrl(status, settingsState))}
                >
                  <Copy />
                </Button>
              </div>
            ) : (
              <div className="proxy-address-inputs">
                <label>
                  <span>地址</span>
                  <input
                    value={settingsState.config.listen_address}
                    readOnly
                    aria-readonly="true"
                  />
                </label>
                <label>
                  <span>端口</span>
                  <input
                    type="number"
                    value={settingsState.config.listen_port}
                    onChange={(event) =>
                      onConfigChange({
                        ...settingsState.config,
                        listen_port: Number(event.target.value || 0),
                      })
                    }
                    onBlur={() => void onConfigCommit(settingsState.config)}
                  />
                </label>
              </div>
            )}
            <p className="auth-muted">地址当前不可修改，修改端口后需要重启代理服务才能生效</p>
            {saving ? <div className="proxy-saving-note">保存中...</div> : null}
          </section>

          <section className="token-card proxy-model-config-card">
            <h2>模型配置</h2>
            <div className="proxy-capability-list">
              {settingsState.capabilities.map((capability) => {
                const state = proxyCapabilityStatus(capability);
                return (
                  <button
                    key={capability.account_id}
                    type="button"
                    className={`proxy-capability-row ${capability.status !== "unsupported" ? "proxy-capability-row-actionable" : ""}`}
                    onClick={() => {
                      if (capability.kind === "reverse_copilot" || capability.kind === "reverse_openai") {
                        if (capability.status === "reverse_pending") {
                          onSubTabChange("reverse");
                        }
                        return;
                      }
                      if (capability.is_claude_compatible_provider) {
                        onOpenProfileEditor(capability);
                      }
                    }}
                  >
                    <div className="proxy-capability-copy proxy-capability-copy-compact">
                      <span className="proxy-capability-logo">
                        <ProviderIcon {...providerIconConfig(capability.provider)} />
                      </span>
                      <span>{capability.display_name}</span>
                    </div>
                    <span className={`proxy-capability-badge proxy-capability-badge-${state.tone}`}>{state.label}</span>
                  </button>
                );
              })}
            </div>
          </section>

          <section className="token-card proxy-routes-card">
            <div className="proxy-card-header">
              <div>
                <h2>模型路由</h2>
                <p className="auth-muted">根据请求中的模型名路由到不同的 Provider</p>
              </div>
            </div>

            <div className="proxy-route-list">
              {settingsState.config.routes.length === 0 ? (
                <div className="token-empty">暂无模型路由</div>
              ) : (
                <>
                  <div className="proxy-route-header">
                    <span>模型模式</span>
                    <span>匹配方式</span>
                    <span>目标 Provider</span>
                    <span>操作</span>
                  </div>
                  {settingsState.config.routes.map((route) => {
                    const capability = settingsState.capabilities.find((item) => item.account_id === route.account_id);
                    return (
                      <div key={route.id} className="proxy-route-summary-row">
                        <span>{route.model_pattern}</span>
                        <span className="proxy-route-match-chip">
                          {route.model_pattern.includes("*") ? "通配符匹配" : "精确匹配"}
                        </span>
                        <span>{capability?.display_name ?? route.account_id}</span>
                        <div className="proxy-route-actions">
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon-sm"
                            className="proxy-route-edit-button"
                            aria-label="编辑路由"
                            onClick={() => onOpenRouteEditor(route.id)}
                          >
                            <Pencil />
                          </Button>
                          <Button
                            type="button"
                            variant="ghost"
                            size="icon-sm"
                            className="proxy-route-delete-button"
                            aria-label="删除路由"
                            onClick={() => void onDeleteRoute(route.id)}
                          >
                            <Trash2 />
                          </Button>
                        </div>
                      </div>
                    );
                  })}
                </>
              )}
              <Button type="button" variant="secondary" size="sm" className="proxy-add-route-button" onClick={onAddRoute}>
                <Plus data-icon="inline-start" />
                添加路由
              </Button>
            </div>
          </section>

          <section className="proxy-stats-grid">
            <div className="token-metric token-metric-default">
              <span>活跃连接</span>
              <strong>{status?.active_connections ?? 0}</strong>
            </div>
            <div className="token-metric token-metric-default">
              <span>总请求数</span>
              <strong>{status?.total_requests ?? 0}</strong>
            </div>
            <div className="token-metric token-metric-default">
              <span>成功率</span>
              <strong>{`${Math.round(status?.success_rate ?? 0)}%`}</strong>
            </div>
            <div className="token-metric token-metric-default">
              <span>运行时间</span>
              <strong>{formatProxyUptime(status?.uptime_seconds ?? 0)}</strong>
            </div>
          </section>
        </>
      ) : null}

      {editingCapability ? (
        <div className="proxy-profile-modal-backdrop" role="presentation" onClick={onCloseProfileEditor}>
          <div className="proxy-profile-modal" role="dialog" aria-modal="true" onClick={(event) => event.stopPropagation()}>
            <div className="proxy-card-header">
              <div>
                <h2>{editingCapability.display_name}</h2>
                <p className="auth-muted">补充 Claude 代理所需信息</p>
              </div>
              <Button type="button" variant="ghost" size="sm" onClick={onCloseProfileEditor}>
                关闭
              </Button>
            </div>

            <label className="proxy-profile-field">
              <span>BASE_URL</span>
              <input value={editingBaseUrl} onChange={(event) => onEditingBaseUrlChange(event.target.value)} />
            </label>

            <label className="proxy-profile-field">
              <span>API 格式</span>
              <Select value={editingApiFormat} onValueChange={(value) => onEditingApiFormatChange(value as ClaudeApiFormat)}>
                <SelectTrigger className="proxy-profile-select">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {claudeApiFormatOptions.map((option) => (
                    <SelectItem key={option.value} value={option.value}>
                      {option.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </label>

            <label className="proxy-profile-field">
              <span>认证字段</span>
              <Select value={editingAuthField} onValueChange={(value) => onEditingAuthFieldChange(value as ClaudeAuthField)}>
                <SelectTrigger className="proxy-profile-select">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {claudeAuthFieldOptions.map((option) => (
                    <SelectItem key={option.value} value={option.value}>
                      {option.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </label>

            <label className="proxy-profile-field">
              <span>API Key</span>
              <input
                type="password"
                autoComplete="off"
                value={editingApiKey}
                onChange={(event) => onEditingApiKeyChange(event.target.value)}
                placeholder={editingCapability.profile.secret_configured ? "留空则保留当前密钥" : "请输入 Claude 代理 API Key"}
              />
            </label>

            <div className="proxy-profile-actions">
              <Button type="button" variant="ghost" onClick={onCloseProfileEditor}>
                取消
              </Button>
              <Button type="button" onClick={() => void onProfileSave()} disabled={profileSaving}>
                {profileSaving ? "保存中" : "保存"}
              </Button>
            </div>
          </div>
        </div>
      ) : null}

      {settingsState && (routeEditingId !== null || routeEditingPattern || routeEditingAccountId) ? (
        <div className="proxy-profile-modal-backdrop" role="presentation" onClick={onCloseRouteEditor}>
          <div className="proxy-profile-modal" role="dialog" aria-modal="true" onClick={(event) => event.stopPropagation()}>
            <div className="proxy-card-header">
              <div>
                <h2>{routeEditingId ? "编辑路由" : "添加路由"}</h2>
                <p className="auth-muted">配置模型模式与目标 Provider</p>
              </div>
              <Button type="button" variant="ghost" size="sm" onClick={onCloseRouteEditor}>
                关闭
              </Button>
            </div>

            <label className="proxy-profile-field">
              <span>模型模式</span>
              <input value={routeEditingPattern} onChange={(event) => onRouteEditingPatternChange(event.target.value)} />
            </label>

            <label className="proxy-profile-field">
              <span>目标 Provider</span>
              <Select value={routeEditingAccountId} onValueChange={onRouteEditingAccountIdChange}>
                <SelectTrigger className="proxy-profile-select">
                  <SelectValue placeholder="选择账号" />
                </SelectTrigger>
                <SelectContent>
                  {settingsState.capabilities
                    .filter(
                      (capability) =>
                        capability.status === "direct_ready" ||
                        (capability.status === "reverse_ready" && capability.account_id.startsWith("reverse:")),
                    )
                    .map((capability) => {
                      const state = proxyCapabilityStatus(capability);
                      return (
                        <SelectItem key={capability.account_id} value={capability.account_id}>
                          {`${capability.display_name} · ${state.label}`}
                        </SelectItem>
                      );
                    })}
                </SelectContent>
              </Select>
            </label>

            <div className="proxy-route-enabled-row">
              <span>启用路由</span>
              <Switch checked={routeEditingEnabled} onCheckedChange={onRouteEditingEnabledChange} />
            </div>

            <div className="proxy-profile-actions">
              <Button type="button" variant="ghost" onClick={onCloseRouteEditor}>
                取消
              </Button>
              <Button type="button" onClick={() => void onRouteSave()}>
                保存
              </Button>
            </div>
          </div>
        </div>
      ) : null}

      {reverseManagerOpen ? (
        <div className="proxy-profile-modal-backdrop" role="presentation" onClick={onCloseReverseManager}>
          <div className="proxy-profile-modal" role="dialog" aria-modal="true" onClick={(event) => event.stopPropagation()}>
            <div className="proxy-card-header">
              <div>
                <h2>{reverseManagerOpen === "copilot" ? "GitHub Copilot" : "ChatGPT (Codex OAuth)"}</h2>
                <p className="auth-muted">管理默认账号与已授权来源</p>
              </div>
              <Button type="button" variant="ghost" size="sm" onClick={onCloseReverseManager}>
                关闭
              </Button>
            </div>

            <div className="proxy-capability-list">
              {reverseManagerAccounts.map((account) => (
                <div key={account.id} className="proxy-capability-row">
                    <div className="proxy-capability-copy proxy-capability-copy-compact">
                      <span className="proxy-capability-logo">
                        {reverseManagerOpen === "copilot" ? (
                          <ProviderIcon icon={githubCopilotIcon} />
                        ) : (
                          <ProviderIcon {...providerIconConfig("openai")} />
                        )}
                      </span>
                      <span>{account.login}</span>
                    </div>
                  <div className="proxy-manager-actions">
                    {reverseDefaultAccountId === account.id ? (
                      <span className="proxy-capability-badge proxy-capability-badge-ready">默认</span>
                    ) : (
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        onClick={() => void onReverseSetDefault(reverseManagerOpen, account.id)}
                      >
                        设为默认
                      </Button>
                    )}
                    {reverseManagerOpen === "copilot" ? (
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-sm"
                        onClick={() => void onCopilotRemoveAccount(account.id)}
                      >
                        <Trash2 />
                      </Button>
                    ) : null}
                  </div>
                </div>
              ))}
            </div>

            {reverseManagerOpen === "copilot" ? (
              <>
                <Button type="button" variant="secondary" onClick={() => void onCopilotAddAccount()}>
                  <Plus data-icon="inline-start" />
                  添加其他账号
                </Button>
                {copilotDeviceCode ? (
                  <div className="proxy-device-flow-card">
                    <div className="proxy-device-flow-row">
                      <div className="proxy-device-code">{copilotDeviceCode.user_code}</div>
                      <Button
                        type="button"
                        variant="outline"
                        size="icon-sm"
                        className="proxy-device-copy-button"
                        aria-label="复制设备码"
                        onClick={() => void onCopyCopilotDeviceValue("user_code")}
                      >
                        <Copy />
                      </Button>
                    </div>
                    <div className="proxy-device-flow-row">
                      <div className="auth-muted proxy-device-flow-text">{copilotDeviceCode.verification_uri}</div>
                      <Button
                        type="button"
                        variant="outline"
                        size="icon-sm"
                        className="proxy-device-copy-button"
                        aria-label="复制授权链接"
                        onClick={() => void onCopyCopilotDeviceValue("verification_uri")}
                      >
                        <Copy />
                      </Button>
                    </div>
                    <div className="auth-muted">{copilotPolling ? "等待授权中..." : "等待下一次轮询"}</div>
                  </div>
                ) : null}
              </>
            ) : (
              <Button type="button" variant="secondary" onClick={onOpenAIOAuthAccount}>
                <Plus data-icon="inline-start" />
                添加其他账号
              </Button>
            )}
          </div>
        </div>
      ) : null}
    </section>
  );
}

function TokenUsageSummary({ totals }: { totals: LocalTokenUsageTotals }) {
  return (
    <section className="token-card token-summary-card">
      <h2>用量概览</h2>
      <div className="token-summary-grid">
        <TokenUsageMetric label="总 Token" value={formatCompactTokens(totals.total_tokens)} />
        <TokenUsageMetric label="输入 Token" value={formatCompactTokens(totals.input_tokens)} />
        <TokenUsageMetric label="输出 Token" value={formatCompactTokens(totals.output_tokens)} />
        <TokenUsageMetric label="存储缓存" value={formatCompactTokens(totals.cache_creation_tokens)} />
        <TokenUsageMetric label="缓存命中" value={formatCompactTokens(totals.cache_read_tokens)} />
        <TokenUsageMetric label="缓存命中率" value={`${Math.round(totals.cache_hit_rate_percent)}%`} />
      </div>
    </section>
  );
}

function TokenUsageMetric({
  label,
  value,
  tone = "default",
}: {
  label: string;
  value: string;
  tone?: "default" | "green" | "purple" | "red" | "blue";
}) {
  return (
    <div className={`token-metric token-metric-${tone}`}>
      <span>{label}</span>
      <strong>{value}</strong>
    </div>
  );
}

function tokenSegmentStyle(height: number): CSSProperties {
  return {
    height: height > 0 ? `${Math.max(height, 3)}%` : 0,
  };
}

function gitBarStyle(height: number): CSSProperties {
  return {
    height: height > 0 ? `${Math.max(height, 3)}%` : 0,
  };
}

function chartColumnCountStyle(count: number): CSSProperties & { "--usage-chart-columns": number } {
  return {
    "--usage-chart-columns": Math.max(count, 1),
  };
}

function formatGeneratedAt(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  return date.toLocaleString("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function AddAccountPanel({
  message,
  onBack,
  onNext,
}: {
  message: string | null;
  onBack: () => void;
  onNext: (provider: ProviderKey) => void;
}) {
  const [selectedProvider, setSelectedProvider] = useState<ProviderKey>("openai");

  return (
    <Card className="settings-panel add-panel">
      <div className="add-header">
        <Button variant="ghost" size="icon-sm" className="icon-button back-button" onClick={onBack} aria-label="返回">
          <ArrowLeft data-icon="inline-start" />
        </Button>
        <h1>添加账号</h1>
      </div>

      <section className="settings-section">
        <h2>选择平台</h2>
        <div className="provider-list">
          {providers.map((provider) => (
            <button
              key={provider.key}
              type="button"
              className={`provider-card ${selectedProvider === provider.key ? "provider-card-selected" : ""}`}
              onClick={() => setSelectedProvider(provider.key)}
            >
              <ProviderIcon icon={provider.icon} iconMode={provider.iconMode} />
              <span className="provider-name">{provider.name}</span>
              <span className="provider-spacer" />
              <span className="provider-method">{provider.method}</span>
            </button>
          ))}
        </div>
      </section>

      <div className="separator" />

      <Button className="next-button" onClick={() => onNext(selectedProvider)}>
        下一步
      </Button>

      {message ? <div className="settings-message">{message}</div> : null}
    </Card>
  );
}

function OAuthAuthPanel({
  provider,
  authUrl,
  authCode,
  oauthStarting,
  message,
  onBack,
  onGenerate,
  onComplete,
  onCodeChange,
}: {
  provider: OAuthProviderKey;
  authUrl: string | null;
  authCode: string;
  oauthStarting: boolean;
  message: string | null;
  onBack: () => void;
  onGenerate: () => void;
  onComplete: () => void;
  onCodeChange: (value: string) => void;
}) {
  const providerName = providerDisplayName(provider);
  const callbackExample =
    provider === "anthropic"
      ? "https://platform.claude.com/oauth/code/callback?code=..."
      : "http://localhost:1455/auth/callback?code=...";
  return (
    <Card className="settings-panel oauth-panel">
      <div className="add-header">
        <Button variant="ghost" size="icon-sm" className="icon-button back-button" onClick={onBack} aria-label="返回">
          <ArrowLeft data-icon="inline-start" />
        </Button>
        <h1>{providerName} 账户授权</h1>
      </div>

      <p className="auth-subtitle">Authorization Method</p>
      <p className="auth-instruction">请按照以下步骤完成 {providerName} 账户的授权：</p>

      <AuthStepCard number={1} title="点击下方按钮生成授权链接">
        {authUrl ? (
          <>
            <div className="auth-link-row">
              <div className="auth-link-box">{authUrl}</div>
              <Button variant="outline" className="copy-button" onClick={() => void navigator.clipboard?.writeText(authUrl)}>
                <Copy data-icon="inline-start" />
              </Button>
            </div>
            <Button variant="ghost" className="regen-button" onClick={onGenerate} disabled={oauthStarting}>
              <RefreshCw data-icon="inline-start" className={oauthStarting ? "refresh-icon-spinning" : undefined} />
              重新生成
            </Button>
          </>
        ) : (
          <Button className="generate-button" onClick={onGenerate} disabled={oauthStarting}>
            <Link2 data-icon="inline-start" />
            {oauthStarting ? "生成中" : "生成授权链接"}
          </Button>
        )}
      </AuthStepCard>

      <AuthStepCard number={2} title="在浏览器中打开链接并完成授权">
        <p className="auth-muted">请在新标签页中打开授权链接，登录您的 {providerName} 账户并授权。</p>
        <div className="auth-alert">
          重要提示：授权后页面可能会加载较长时间，请耐心等待。当浏览器地址栏变为回调地址并包含 code 时，表示授权已完成。
        </div>
      </AuthStepCard>

      <AuthStepCard number={3} title="输入授权链接或 Code">
        <p className="auth-muted">授权完成后，当页面地址变为 {callbackExample} 时：</p>
        <label className="auth-input-label" htmlFor="oauth-code">
          <KeyRound />
          授权链接或 Code
        </label>
        <textarea
          id="oauth-code"
          className="auth-code-input"
          value={authCode}
          onChange={(event) => onCodeChange(event.target.value)}
          placeholder={`方式1：复制完整的链接（${callbackExample}）\n方式2：仅复制 code 参数的值`}
        />
        <div className="auth-hint">
          <Info />
          <span>您可以直接复制整个链接或仅复制 code 参数值，系统会自动识别</span>
        </div>
      </AuthStepCard>

      <Button className="submit-auth-button" onClick={onComplete}>
        完成授权
      </Button>
      {message ? <div className="settings-message">{message}</div> : null}
    </Card>
  );
}

function KimiImportPanel({
  accountName,
  importing,
  message,
  onBack,
  onAccountNameChange,
  onImport,
}: {
  accountName: string;
  importing: boolean;
  message: string | null;
  onBack: () => void;
  onAccountNameChange: (value: string) => void;
  onImport: () => void;
}) {
  return (
    <Card className="settings-panel oauth-panel">
      <div className="add-header">
        <Button variant="ghost" size="icon-sm" className="icon-button back-button" onClick={onBack} aria-label="返回">
          <ArrowLeft data-icon="inline-start" />
        </Button>
        <h1>Kimi 账号导入</h1>
      </div>

      <p className="auth-subtitle">Kimi Code</p>
      <p className="auth-instruction">从当前 Kimi CLI 登录态导入账号。</p>

      <AuthStepCard number={1} title="账号名称">
        <label className="auth-input-label" htmlFor="kimi-account-name">
          <KeyRound />
          账号名称
        </label>
        <input
          id="kimi-account-name"
          className="kimi-account-input"
          value={accountName}
          onChange={(event) => onAccountNameChange(event.target.value)}
          placeholder="Kimi Work"
        />
      </AuthStepCard>

      <AuthStepCard number={2} title="CLI 登录态">
        <p className="auth-muted">使用本机 ~/.kimi/credentials/kimi-code.json。添加多个账号时，先切换 Kimi CLI 登录态，再用不同名称导入。</p>
      </AuthStepCard>

      <Button className="submit-auth-button" onClick={onImport} disabled={importing || !accountName.trim()}>
        {importing ? "导入中" : "导入账号"}
      </Button>
      {message ? <div className="settings-message">{message}</div> : null}
    </Card>
  );
}

function GlmApiKeyPanel({
  accountName,
  apiKey,
  importing,
  message,
  onBack,
  onAccountNameChange,
  onApiKeyChange,
  onImport,
}: {
  accountName: string;
  apiKey: string;
  importing: boolean;
  message: string | null;
  onBack: () => void;
  onAccountNameChange: (value: string) => void;
  onApiKeyChange: (value: string) => void;
  onImport: () => void;
}) {
  return (
    <Card className="settings-panel oauth-panel">
      <div className="add-header">
        <Button variant="ghost" size="icon-sm" className="icon-button back-button" onClick={onBack} aria-label="返回">
          <ArrowLeft data-icon="inline-start" />
        </Button>
        <h1>GLM 账号添加</h1>
      </div>

      <p className="auth-subtitle">Z.ai API Key</p>
      <p className="auth-instruction">使用 GLM / z.ai API Key 读取 Coding Plan 额度。</p>

      <AuthStepCard number={1} title="账号名称">
        <label className="auth-input-label" htmlFor="glm-account-name">
          <KeyRound />
          账号名称
        </label>
        <input
          id="glm-account-name"
          className="kimi-account-input"
          value={accountName}
          onChange={(event) => onAccountNameChange(event.target.value)}
          placeholder="GLM Work"
        />
      </AuthStepCard>

      <AuthStepCard number={2} title="API Key">
        <label className="auth-input-label" htmlFor="glm-api-key">
          <KeyRound />
          GLM API Key
        </label>
        <input
          id="glm-api-key"
          className="kimi-account-input"
          type="password"
          autoComplete="off"
          value={apiKey}
          onChange={(event) => onApiKeyChange(event.target.value)}
          placeholder="输入 GLM / z.ai API Key"
        />
        <p className="auth-muted">可添加多个 GLM 账号，每个账号的 API Key 会按账号单独保存。</p>
      </AuthStepCard>

      <Button className="submit-auth-button" onClick={onImport} disabled={importing || !accountName.trim() || !apiKey.trim()}>
        {importing ? "保存中" : "添加账号"}
      </Button>
      {message ? <div className="settings-message">{message}</div> : null}
    </Card>
  );
}

function MiniMaxApiKeyPanel({
  accountName,
  apiKey,
  importing,
  message,
  onBack,
  onAccountNameChange,
  onApiKeyChange,
  onImport,
}: {
  accountName: string;
  apiKey: string;
  importing: boolean;
  message: string | null;
  onBack: () => void;
  onAccountNameChange: (value: string) => void;
  onApiKeyChange: (value: string) => void;
  onImport: () => void;
}) {
  return (
    <Card className="settings-panel oauth-panel">
      <div className="add-header">
        <Button variant="ghost" size="icon-sm" className="icon-button back-button" onClick={onBack} aria-label="返回">
          <ArrowLeft data-icon="inline-start" />
        </Button>
        <h1>MiniMax 账号添加</h1>
      </div>

      <p className="auth-subtitle">API Key</p>
      <p className="auth-instruction">使用 MiniMax API Key 读取 Coding Plan 额度。</p>

      <AuthStepCard number={1} title="账号名称">
        <label className="auth-input-label" htmlFor="minimax-account-name">
          <KeyRound />
          账号名称
        </label>
        <input
          id="minimax-account-name"
          className="kimi-account-input"
          value={accountName}
          onChange={(event) => onAccountNameChange(event.target.value)}
          placeholder="MiniMax Work"
        />
      </AuthStepCard>

      <AuthStepCard number={2} title="API Key">
        <label className="auth-input-label" htmlFor="minimax-api-key">
          <KeyRound />
          MiniMax API Key
        </label>
        <input
          id="minimax-api-key"
          className="kimi-account-input"
          type="password"
          autoComplete="off"
          value={apiKey}
          onChange={(event) => onApiKeyChange(event.target.value)}
          placeholder="输入 MiniMax API Key"
        />
        <p className="auth-muted">可添加多个 MiniMax 账号，每个账号的 API Key 会按账号单独保存。</p>
      </AuthStepCard>

      <Button className="submit-auth-button" onClick={onImport} disabled={importing || !accountName.trim() || !apiKey.trim()}>
        {importing ? "保存中" : "添加账号"}
      </Button>
      {message ? <div className="settings-message">{message}</div> : null}
    </Card>
  );
}

function CopilotTokenPanel({
  accountName,
  token,
  importing,
  message,
  onBack,
  onAccountNameChange,
  onTokenChange,
  onImport,
}: {
  accountName: string;
  token: string;
  importing: boolean;
  message: string | null;
  onBack: () => void;
  onAccountNameChange: (value: string) => void;
  onTokenChange: (value: string) => void;
  onImport: () => void;
}) {
  return (
    <Card className="settings-panel oauth-panel">
      <div className="add-header">
        <Button variant="ghost" size="icon-sm" className="icon-button back-button" onClick={onBack} aria-label="返回">
          <ArrowLeft data-icon="inline-start" />
        </Button>
        <h1>Copilot 账号添加</h1>
      </div>

      <p className="auth-subtitle">GitHub Token</p>
      <p className="auth-instruction">使用 GitHub CLI 登录态或手动 Token 读取 Copilot 额度。</p>

      <AuthStepCard number={1} title="账号名称">
        <label className="auth-input-label" htmlFor="copilot-account-name">
          <KeyRound />
          账号名称
        </label>
        <input
          id="copilot-account-name"
          className="kimi-account-input"
          value={accountName}
          onChange={(event) => onAccountNameChange(event.target.value)}
          placeholder="Copilot Work"
        />
      </AuthStepCard>

      <AuthStepCard number={2} title="GitHub Token">
        <label className="auth-input-label" htmlFor="copilot-token">
          <KeyRound />
          GitHub Token
        </label>
        <input
          id="copilot-token"
          className="kimi-account-input"
          type="password"
          autoComplete="off"
          value={token}
          onChange={(event) => onTokenChange(event.target.value)}
          placeholder="留空则从 gh CLI Keychain 导入"
        />
        <p className="auth-muted">已运行 gh auth login 的情况下可留空；多账号场景可手动填写不同 GitHub Token。</p>
      </AuthStepCard>

      <Button className="submit-auth-button" onClick={onImport} disabled={importing || !accountName.trim()}>
        {importing ? "保存中" : "添加账号"}
      </Button>
      {message ? <div className="settings-message">{message}</div> : null}
    </Card>
  );
}

function AuthStepCard({
  number,
  title,
  children,
}: {
  number: number;
  title: string;
  children: ReactNode;
}) {
  return (
    <section className="auth-step-card">
      <div className="auth-step-header">
        <span className="step-number">{number}</span>
        <h2>{title}</h2>
      </div>
      {children}
    </section>
  );
}

type ProviderKey = "openai" | "anthropic" | "kimi" | "glm" | "minimax" | "copilot" | "qwen" | "xiaomi" | "custom";
type ProviderIconMode = "img" | "mask" | "xiaomi";

const refreshIntervalOptions = [
  { value: 5, label: "5 分钟" },
  { value: 15, label: "15 分钟" },
  { value: 30, label: "30 分钟" },
  { value: 60, label: "1 小时" },
];

const claudeApiFormatOptions: Array<{ value: ClaudeApiFormat; label: string }> = [
  { value: "anthropic", label: "Anthropic Messages（原生）" },
  { value: "openai_chat", label: "OpenAI Chat Completions" },
  { value: "openai_responses", label: "OpenAI Responses" },
];

const claudeAuthFieldOptions: Array<{ value: ClaudeAuthField; label: string }> = [
  { value: "ANTHROPIC_AUTH_TOKEN", label: "ANTHROPIC_AUTH_TOKEN（默认）" },
  { value: "ANTHROPIC_API_KEY", label: "ANTHROPIC_API_KEY" },
];

const tokenUsageRangeOptions: UsageRangeOption[] = ["thisMonth", "thisWeek", "today", "custom"];

const providers: Array<{
  key: ProviderKey;
  name: string;
  method: "OAuth" | "API Key" | "Kimi CLI" | "GitHub Token";
  icon: string;
  iconMode?: ProviderIconMode;
}> = [
  { key: "openai", name: "OpenAI", method: "OAuth", icon: openaiIcon },
  { key: "anthropic", name: "Anthropic", method: "OAuth", icon: anthropicIcon },
  { key: "copilot", name: "Copilot", method: "OAuth", icon: githubCopilotIcon },
  { key: "kimi", name: "Kimi", method: "Kimi CLI", icon: kimiIcon },
  { key: "glm", name: "GLM", method: "API Key", icon: zhipuIcon, iconMode: "mask" },
  { key: "minimax", name: "MiniMax", method: "API Key", icon: minimaxIcon },
  { key: "qwen", name: "Qwen", method: "API Key", icon: qwenIcon },
  { key: "xiaomi", name: "XiaoMi", method: "API Key", icon: "", iconMode: "xiaomi" },
  { key: "custom", name: "Custom", method: "API Key", icon: openrouterIcon, iconMode: "mask" },
];

function ProviderIcon({ icon, iconMode = "img" }: { icon: string; iconMode?: ProviderIconMode }) {
  if (iconMode === "xiaomi") {
    return (
      <svg className="provider-logo-img" viewBox="0 0 24 24" aria-hidden="true">
        <path
          fill="currentColor"
          d="M0.958 10.93629c0.24605 0.0003 0.4483 0.19418 0.459 0.44l0 2.729c-0.01597 0.24176-0.21672 0.42969-0.459 0.42968-0.24228 0-0.44303-0.18793-0.459-0.42968l0-2.729c0.0107-0.24582 0.21295-0.4397 0.459-0.44m4.814-2.035c0.13537-0.02794 0.27613 0.00646 0.38334 0.09371 0.10721 0.08724 0.16951 0.21807 0.16966 0.35629l0 4.754c0 0.2535-0.2055 0.459-0.459 0.459-0.2535 0-0.459-0.2055-0.459-0.459l0-3.625-1.667 1.722c-0.08428 0.08978-0.20104 0.14203-0.32415 0.14503-0.12311 0.00301-0.24228-0.04347-0.33085-0.12903-0.02448-0.02498-0.04626-0.05247-0.065-0.082l-2.392-2.466c-0.15547-0.18446-0.14161-0.45779 0.03171-0.62559 0.17333-0.16779 0.44696-0.17278 0.62629-0.01141l2.124 2.187 2.127-2.188c0.06366-0.06574 0.14548-0.11101 0.235-0.13l0-0.001z m2.068 0.004c0.24753 0.00095 0.44993 0.1976 0.458 0.445l0 4.755c-0.0011 0.25249-0.20551 0.45691-0.458 0.458-0.25249-0.0011-0.4569-0.20551-0.458-0.458l0-4.755c0.00807-0.2474 0.21047-0.44406 0.458-0.445m1.973 2.014c0.25288 0 0.45835 0.20412 0.46 0.457l0 2.729c-0.00102 0.18527-0.11308 0.35184-0.2843 0.4226-0.17122 0.07076-0.36819 0.0319-0.4997-0.0986-0.08569-0.08607-0.13386-0.20254-0.134-0.324l0-2.729c0.0011-0.25249 0.20551-0.4569 0.458-0.458l0 0.001z m0.002-2.045c0.12626 0.00506 0.24487 0.06184 0.328 0.157l2.127 2.19 2.125-2.19c0.13039-0.13077 0.32644-0.17067 0.49756-0.10126 0.17112 0.06941 0.28399 0.23461 0.28644 0.41926l0 4.756c-0.00108 0.25133-0.20368 0.45527-0.455 0.458-0.25249-0.0011-0.4569-0.20551-0.458-0.458l0-3.625-1.667 1.723c-0.17736 0.18151-0.46822 0.18509-0.65 0.008l-0.005-0.005q0-0.002-0.004-0.003l-2.455-2.534c-0.09247-0.08574-0.14568-0.20569-0.14719-0.33178-0.00151-0.12609 0.04881-0.24728 0.13919-0.33522 0.09048-0.08699 0.21259-0.13324 0.338-0.128m6.797 1.206c0.17506-0.0469 0.36135 0.01336 0.47577 0.15391 0.11442 0.14055 0.13566 0.33518 0.05423 0.49709-0.41938 0.76588-0.28336 1.7166 0.33397 2.33416 0.61732 0.61755 1.56799 0.75394 2.33403 0.33484 0.22184-0.12147 0.50013-0.04107 0.623 0.18 0.12199 0.22233 0.04102 0.50145-0.181 0.624-0.42287 0.23217-0.89759 0.3536-1.38 0.353l-0.142-0.004c-0.99339-0.04542-1.89306-0.60011-2.37982-1.46725-0.48677-0.86715-0.49175-1.92406-0.01318-2.79575 0.06061-0.10292 0.15887-0.17822 0.274-0.21l0.001 0z m0.864-0.931c1.12353-0.61511 2.51814-0.41547 3.42398 0.49014 0.90584 0.90561 1.10584 2.30017 0.49102 3.42386-0.08022 0.14731-0.23427 0.23928-0.402 0.24l-0.057-0.004c-0.05745-0.00866-0.11294-0.02728-0.164-0.055-0.22122-0.12215-0.30248-0.39987-0.182-0.622 0.41909-0.76617 0.28283-1.71692-0.33457-2.33456-0.6174-0.61763-1.5681-0.75424-2.33443-0.33544-0.14431 0.09171-0.32764 0.0956-0.47572 0.01009-0.14807-0.08551-0.23634-0.24623-0.22904-0.41707 0.00729-0.17083 0.10894-0.32345 0.26376-0.39602m-7.886-7.781c1.481 0 1.696 1.202 1.696 1.654l0 2.648-0.917 0 0-0.432c-0.26 0.346-0.792 0.535-1.36 0.535-0.133 0-1.289-0.03-1.384-1.136-0.082-0.932 0.675-1.61 2.053-1.61l0.691 0c0-0.563-0.367-0.886-0.983-0.886-0.44 0.013-0.864 0.174-1.2 0.458l-0.36-0.664c0.484-0.379 1.012-0.567 1.764-0.567m4.427 0.1c1.263 0 2.082 0.97 2.083 2.15 0 1.181-0.824 2.154-2.083 2.154-1.259 0-2.084-0.972-2.084-2.152 0-1.18 0.82-2.153 2.084-2.153l0 0.001z m6.801 0.015c0.68 0 1.202 0.465 1.197 1.548l0 2.642-0.915 0 0-2.383c0-0.312-0.002-0.98-0.63-0.98-0.628 0-0.628 0.667-0.628 0.838l0 2.524-0.89 0 0-2.524c0-0.17-0.001-0.838-0.63-0.838-0.628 0-0.628 0.668-0.628 0.98l0 2.383-0.917 0 0-4.03 0.917 0 0 0.357c0.21838-0.30976 0.56832-0.50043 0.947-0.516 0.398 0 0.76 0.193 0.982 0.686 0.23791-0.43492 0.69945-0.69987 1.195-0.686l0-0.001z m-18.093 0.872l1.457-1.772 1.138 0-2.009 2.487 2.14 2.602-1.211 0-1.515-1.876-1.515 1.876-1.21 0 2.138-2.602-2.008-2.487 1.138 0 1.457 1.772z m4.149 3.317l-0.916 0 0-4.028 0.916 0 0 4.028z m16.99 0l-0.916 0 0-4.028 0.916 0 0 4.028z m-13.939-1.962c-1.055 0-1.359 0.412-1.326 0.742 0.032 0.329 0.324 0.537 0.757 0.537 0.54274 0.00107 0.98989-0.4258 1.014-0.968l0.002-0.31-0.447 0 0-0.001z m4.093-1.41c-0.663 0-1.184 0.487-1.184 1.32 0 0.832 0.52 1.32 1.184 1.32 0.662 0 1.182-0.49 1.182-1.32 0-0.832-0.52-1.32-1.182-1.32m-7.601-2.299c0.1562-0.0047 0.30748 0.05494 0.41845 0.16497 0.11097 0.11003 0.17191 0.26079 0.16855 0.41703-0.013 0.31504-0.27219 0.56376-0.5875 0.56376-0.31531 0-0.5745-0.24871-0.5875-0.56376-0.00365-0.15659 0.05729-0.30778 0.16852-0.41806 0.11123-0.11028 0.26293-0.16993 0.41948-0.16494l0 0.001z m16.991 0c0.15705-0.0061 0.30966 0.05291 0.42174 0.1631 0.11208 0.11019 0.17369 0.26176 0.17026 0.4189-0.01816 0.3109-0.27557 0.55372-0.587 0.55372-0.31143 0-0.56885-0.24282-0.587-0.55372-0.00551-0.23739 0.13672-0.45333 0.357-0.542 0.07174-0.02763 0.14813-0.0412 0.225-0.04"
          transform="translate(0 5)"
        />
      </svg>
    );
  }
  if (iconMode === "mask") {
    return (
      <span
        className="provider-logo-img provider-logo-mask"
        aria-hidden="true"
        style={{ "--provider-icon-url": `url("${icon}")` } as CSSProperties & { "--provider-icon-url": string }}
      />
    );
  }

  return <img src={icon} alt="" className="provider-logo-img" />;
}
