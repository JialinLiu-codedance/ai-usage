import { listen } from "@tauri-apps/api/event";
import { ArrowLeft, Copy, Inbox, Info, KeyRound, Link2, Plus, RefreshCw, Settings } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { Switch } from "@/components/ui/switch";
import anthropicIcon from "../icons/extracted/anthropic.svg";
import kimiIcon from "../icons/extracted/kimi.svg";
import minimaxIcon from "../icons/extracted/minimax.svg";
import openaiIcon from "../icons/extracted/openai.svg";
import openrouterIcon from "../icons/extracted/openrouter.svg";
import qwenIcon from "../icons/extracted/qwen.svg";
import zhipuIcon from "../icons/extracted/zhipu.svg";
import aiUsageLogo from "../icons/ai-usage-logo.svg";
import {
  deleteOpenAIAccount,
  getCurrentQuota,
  getSettings,
  completeOpenAIOAuth,
  resizePanel,
  refreshQuota,
  saveSettings,
  startOpenAIOAuth,
} from "./lib/tauri";
import {
  hasGeneratedOpenAIAuthLink,
  shouldApplyOAuthStartResult,
  shouldResetOpenAIAuthDraft,
} from "./lib/oauth-auth-state";
import { remainingQuotaProgressValue } from "./lib/quota-display";
import type {
  AccountQuotaStatus,
  AppSettings,
  AppStatus,
  ConnectedAccount,
  QuotaWindow,
  SaveSettingsInput,
} from "./lib/types";

type PanelView = "overview" | "settings" | "add-account" | "openai-auth";
type AddAccountBackView = Extract<PanelView, "overview" | "settings">;
type Tone = "success" | "warning" | "danger" | "muted";
const isTauriRuntime = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
const PANEL_WIDTH = 420;

const emptyStatus: AppStatus = {
  snapshot: null,
  accounts: [],
  refresh_status: "idle",
  last_error: null,
  last_refreshed_at: null,
};

function quotaTone(value: number, threshold: number): Tone {
  if (value <= threshold) {
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

function accountLabel(settings: AppSettings, status: AppStatus): string {
  return status.snapshot?.account_name || settings.account_name || "OpenAI Account";
}

function hasConnectedAccount(settings: AppSettings, status: AppStatus): boolean {
  return settings.accounts.some((account) => account.secret_configured) || settings.secret_configured || Boolean(status.snapshot);
}

function accountSubtitle(settings: AppSettings, status: AppStatus): string {
  const label = accountLabel(settings, status);
  return label.trim() || "OpenAI Account";
}

function connectedAccounts(settings: AppSettings, status: AppStatus): ConnectedAccount[] {
  const configuredAccounts = settings.accounts.filter((account) => account.secret_configured);
  if (configuredAccounts.length > 0) {
    return configuredAccounts;
  }
  if (!hasConnectedAccount(settings, status)) {
    return [];
  }
  return [
    {
      account_id: settings.account_id,
      account_name: accountSubtitle(settings, status),
      provider: "openai",
      auth_mode: settings.auth_mode,
      chatgpt_account_id: settings.chatgpt_account_id,
      secret_configured: settings.secret_configured,
    },
  ];
}

function connectedAccountSubtitle(account: ConnectedAccount): string {
  return account.account_name.trim() || "OpenAI Account";
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
      five_hour: snapshot?.five_hour ?? null,
      seven_day: snapshot?.seven_day ?? null,
      fetched_at: snapshot?.fetched_at ?? null,
      source: snapshot?.source ?? null,
    };
  });
}

function quotaAccountSubtitle(account: AccountQuotaStatus): string {
  return account.account_name.trim() || "OpenAI Account";
}

export default function App() {
  const panelRootRef = useRef<HTMLElement | null>(null);
  const lastPanelSizeRef = useRef<{ width: number; height: number } | null>(null);
  const currentViewRef = useRef<PanelView>("overview");
  const oauthRequestIdRef = useRef(0);
  const [view, setView] = useState<PanelView>("overview");
  const [addAccountBackView, setAddAccountBackView] = useState<AddAccountBackView>("settings");
  const [status, setStatus] = useState<AppStatus>(emptyStatus);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [loading, setLoading] = useState(true);
  const [oauthStarting, setOauthStarting] = useState(false);
  const [oauthTargetAccountId, setOAuthTargetAccountId] = useState<string | null>(null);
  const [authUrl, setAuthUrl] = useState<string | null>(null);
  const [authCode, setAuthCode] = useState("");
  const [settingsMessage, setSettingsMessage] = useState<string | null>(null);
  const [form, setForm] = useState<SaveSettingsInput>({
    account_id: "default",
    account_name: "OpenAI Account",
    auth_mode: "apiKey",
    base_url_override: null,
    chatgpt_account_id: null,
    refresh_interval_minutes: 15,
    low_quota_threshold_percent: 10,
    notify_on_low_quota: true,
    notify_on_reset: false,
    reset_notify_lead_minutes: 15,
    auth_secret: "",
  });

  useEffect(() => {
    async function bootstrap() {
      const [nextStatus, nextSettings] = await Promise.all([getCurrentQuota(), getSettings()]);
      setStatus(nextStatus);
      applySettings(nextSettings);
      setLoading(false);
    }

    void bootstrap();
    if (!isTauriRuntime) {
      return undefined;
    }
    const unlistenPanel = listen("show-main-panel", () => navigateToView("overview"));
    return () => {
      void unlistenPanel.then((dispose) => dispose());
    };
  }, []);

  useEffect(() => {
    if (!isTauriRuntime || !panelRootRef.current) {
      return undefined;
    }

    const panelRoot = panelRootRef.current;
    let frameId: number | null = null;

    const scheduleResize = () => {
      if (frameId !== null) {
        cancelAnimationFrame(frameId);
      }

      frameId = requestAnimationFrame(() => {
        frameId = null;
        const measuredElement = panelRoot.firstElementChild instanceof HTMLElement ? panelRoot.firstElementChild : panelRoot;
        const nextHeight = Math.ceil(measuredElement.scrollHeight);
        const lastPanelSize = lastPanelSizeRef.current;
        if (!nextHeight || (lastPanelSize?.width === PANEL_WIDTH && lastPanelSize.height === nextHeight)) {
          return;
        }

        lastPanelSizeRef.current = { width: PANEL_WIDTH, height: nextHeight };
        void resizePanel(PANEL_WIDTH, nextHeight);
      });
    };

    const observer = new ResizeObserver(scheduleResize);
    observer.observe(panelRoot);
    scheduleResize();

    return () => {
      observer.disconnect();
      if (frameId !== null) {
        cancelAnimationFrame(frameId);
      }
    };
  }, [view, loading]);

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
      auth_secret: "",
    }));
  }

  async function handleRefresh() {
    setStatus((current) => ({ ...current, refresh_status: "refreshing", last_error: null }));
    try {
      setStatus(await refreshQuota());
    } catch (error) {
      setStatus((current) => ({
        ...current,
        refresh_status: "error",
        last_error: error instanceof Error ? error.message : "刷新失败",
      }));
    }
  }

  async function updateSettings(nextForm: SaveSettingsInput) {
    setForm(nextForm);
    try {
      applySettings(await saveSettings(nextForm));
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
      const nextAuthUrl = await startOpenAIOAuth(oauthTargetAccountId);
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
    setOAuthTargetAccountId(null);
  }

  function navigateToView(nextView: PanelView) {
    const previousView = currentViewRef.current;
    currentViewRef.current = nextView;
    if (shouldResetOpenAIAuthDraft(previousView, nextView)) {
      resetOAuthDraft();
    }
    setView(nextView);
  }

  async function handleCompleteOAuth() {
    setSettingsMessage(null);
    if (!hasGeneratedOpenAIAuthLink(authUrl)) {
      setSettingsMessage("请先重新生成授权链接");
      return;
    }
    if (!authCode.trim()) {
      setSettingsMessage("请先输入授权链接或 Code");
      return;
    }

    try {
      const result = await completeOpenAIOAuth(authCode.trim());
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

  function openOpenAIAuth(accountId: string | null) {
    setSettingsMessage(null);
    setOAuthTargetAccountId(accountId);
    navigateToView("openai-auth");
  }

  async function handleDeleteOpenAIAccount(accountId: string) {
    setSettingsMessage(null);
    try {
      const nextSettings = await deleteOpenAIAccount(accountId);
      applySettings(nextSettings);
      setStatus(await getCurrentQuota());
    } catch (error) {
      setSettingsMessage(error instanceof Error ? error.message : "删除账号失败");
    }
  }

  if (loading || !settings) {
    return (
      <main ref={panelRootRef} className="panel-root panel-root-overview">
        <Card className="overview-panel loading-panel">加载中...</Card>
      </main>
    );
  }

  return (
    <main ref={panelRootRef} className={`panel-root panel-root-${view}`}>
      {view === "overview" ? (
        <OverviewPanel
          status={status}
          settings={settings}
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
          onChange={(nextForm) => void updateSettings(nextForm)}
          onBack={() => navigateToView("overview")}
          onAddAccount={() => openAddAccount("settings")}
          onReauthorize={(accountId) => openOpenAIAuth(accountId)}
          onDeleteAccount={(accountId) => void handleDeleteOpenAIAccount(accountId)}
        />
      ) : null}

      {view === "add-account" ? (
        <AddAccountPanel
          message={settingsMessage}
          onBack={() => navigateToView(addAccountBackView)}
          onNext={(provider) => {
            setSettingsMessage(null);
            if (provider === "openai") {
              openOpenAIAuth(null);
              return;
            }
            setSettingsMessage("该平台的接入流程尚未实现");
          }}
        />
      ) : null}

      {view === "openai-auth" ? (
        <OpenAIAuthPanel
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
    </main>
  );
}

function OverviewPanel({
  status,
  settings,
  onRefresh,
  onSettings,
  onAddAccount,
}: {
  status: AppStatus;
  settings: AppSettings;
  onRefresh: () => void;
  onSettings: () => void;
  onAddAccount: () => void;
}) {
  const threshold = settings.low_quota_threshold_percent;
  const stale = status.refresh_status === "error" && Boolean(status.snapshot);
  const accounts = quotaAccounts(settings, status);
  const hasAccount = accounts.length > 0 || hasConnectedAccount(settings, status);

  if (!hasAccount) {
    return (
      <Card className="overview-panel">
        <PanelHeader
          isRefreshing={status.refresh_status === "refreshing"}
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
        onRefresh={onRefresh}
        onSettings={onSettings}
      />
      {accounts.map((account) => (
        <QuotaAccountCard
          key={account.account_id}
          account={account}
          threshold={threshold}
          muted={status.refresh_status === "error"}
          stale={stale}
          error={status.last_error}
        />
      ))}
    </Card>
  );
}

function QuotaAccountCard({
  account,
  threshold,
  muted,
  stale,
  error,
}: {
  account: AccountQuotaStatus;
  threshold: number;
  muted: boolean;
  stale: boolean;
  error: string | null;
}) {
  return (
    <Card className={`quota-card ${muted ? "quota-card-error" : ""}`}>
      <div className="overview-account-row">
        <img className="overview-provider-logo" src={openaiIcon} alt="" aria-hidden="true" />
        <span className="account-subtitle">{quotaAccountSubtitle(account)}</span>
      </div>

      {error ? <div className="inline-error">{error}</div> : null}
      {stale ? <div className="stale-text">数据来自缓存</div> : null}

      <QuotaRow label="5H" window={account.five_hour} threshold={threshold} muted={muted || !account.five_hour} />
      <QuotaRow label="7D" window={account.seven_day} threshold={threshold} muted={muted || !account.seven_day} />
    </Card>
  );
}

function PanelHeader({
  isRefreshing,
  onRefresh,
  onSettings,
}: {
  isRefreshing: boolean;
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
          <Settings data-icon="inline-start" />
        </Button>
      </div>
    </div>
  );
}

function QuotaRow({
  label,
  window,
  threshold,
  muted,
}: {
  label: string;
  window: QuotaWindow | null;
  threshold: number;
  muted: boolean;
}) {
  const remaining = remainingQuotaProgressValue(window);
  const tone = muted || !window ? "muted" : quotaTone(remaining, threshold);
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
  onChange,
  onBack,
  onAddAccount,
  onReauthorize,
  onDeleteAccount,
}: {
  form: SaveSettingsInput;
  settings: AppSettings;
  status: AppStatus;
  message: string | null;
  onChange: (nextForm: SaveSettingsInput) => void;
  onBack: () => void;
  onAddAccount: () => void;
  onReauthorize: (accountId: string) => void;
  onDeleteAccount: (accountId: string) => void;
}) {
  const [thresholdDraft, setThresholdDraft] = useState(String(form.low_quota_threshold_percent));

  useEffect(() => {
    setThresholdDraft(String(form.low_quota_threshold_percent));
  }, [form.low_quota_threshold_percent]);

  function commitThreshold(value: string) {
    const parsed = Number.parseInt(value, 10);
    const nextValue = Number.isFinite(parsed) ? Math.max(1, Math.min(100, parsed)) : 1;
    setThresholdDraft(String(nextValue));
    if (nextValue !== form.low_quota_threshold_percent) {
      onChange({ ...form, low_quota_threshold_percent: nextValue });
    }
  }
  const accounts = connectedAccounts(settings, status);

  return (
    <Card className="settings-panel">
      <div className="add-header">
        <Button variant="ghost" size="icon-sm" className="icon-button back-button" onClick={onBack} aria-label="返回">
          <ArrowLeft data-icon="inline-start" />
        </Button>
        <h1>设置</h1>
      </div>

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
                <span className="account-title">OpenAI</span>
                <span className="account-status">已授权</span>
              </div>
              <div className="account-subtitle">{connectedAccountSubtitle(account)}</div>
              <div className="account-actions">
                <Button
                  variant="secondary"
                  className="account-action-button"
                  onClick={() => onReauthorize(account.account_id)}
                >
                  重新授权
                </Button>
                <Button className="account-delete-button" onClick={() => onDeleteAccount(account.account_id)}>
                  删除
                </Button>
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
        <div className="split-row">
          <span>重置提醒</span>
          <Switch
            aria-label="重置提醒"
            className="settings-switch"
            size="panel"
            checked={form.notify_on_reset}
            onCheckedChange={(checked) => onChange({ ...form, notify_on_reset: checked })}
          />
        </div>
      </section>

      {message ? <div className="settings-message">{message}</div> : null}
    </Card>
  );
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

function OpenAIAuthPanel({
  authUrl,
  authCode,
  oauthStarting,
  message,
  onBack,
  onGenerate,
  onComplete,
  onCodeChange,
}: {
  authUrl: string | null;
  authCode: string;
  oauthStarting: boolean;
  message: string | null;
  onBack: () => void;
  onGenerate: () => void;
  onComplete: () => void;
  onCodeChange: (value: string) => void;
}) {
  return (
    <Card className="settings-panel oauth-panel">
      <div className="add-header">
        <Button variant="ghost" size="icon-sm" className="icon-button back-button" onClick={onBack} aria-label="返回">
          <ArrowLeft data-icon="inline-start" />
        </Button>
        <h1>OpenAI 账户授权</h1>
      </div>

      <p className="auth-subtitle">Authorization Method</p>
      <p className="auth-instruction">请按照以下步骤完成 OpenAI 账户的授权：</p>

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
        <p className="auth-muted">请在新标签页中打开授权链接，登录您的 OpenAI 账户并授权。</p>
        <div className="auth-alert">
          重要提示：授权后页面可能会加载较长时间，请耐心等待。当浏览器地址栏变为 http://localhost... 开头时，表示授权已完成。
        </div>
      </AuthStepCard>

      <AuthStepCard number={3} title="输入授权链接或 Code">
        <p className="auth-muted">授权完成后，当页面地址变为 http://localhost:xxx/auth/callback?code=... 时：</p>
        <label className="auth-input-label" htmlFor="oauth-code">
          <KeyRound />
          授权链接或 Code
        </label>
        <textarea
          id="oauth-code"
          className="auth-code-input"
          value={authCode}
          onChange={(event) => onCodeChange(event.target.value)}
          placeholder={"方式1：复制完整的链接（http://localhost:xxx/auth/callback?code=...）\n方式2：仅复制 code 参数的值"}
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

type ProviderKey = "openai" | "anthropic" | "kimi" | "glm" | "minimax" | "qwen" | "xiaomi" | "custom";
type ProviderIconMode = "img" | "mask" | "xiaomi";

const refreshIntervalOptions = [
  { value: 5, label: "5 分钟" },
  { value: 15, label: "15 分钟" },
  { value: 30, label: "30 分钟" },
  { value: 60, label: "1 小时" },
];

const providers: Array<{ key: ProviderKey; name: string; method: "OAuth" | "API Key"; icon: string; iconMode?: ProviderIconMode }> = [
  { key: "openai", name: "OpenAI", method: "OAuth", icon: openaiIcon },
  { key: "anthropic", name: "Anthropic", method: "OAuth", icon: anthropicIcon },
  { key: "kimi", name: "Kimi", method: "API Key", icon: kimiIcon },
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
