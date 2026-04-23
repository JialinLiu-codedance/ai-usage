import { useEffect, useState } from "react";
import {
  getCurrentQuota,
  getSettings,
  refreshQuota,
  saveSettings,
  testConnection,
} from "./lib/tauri";
import type {
  AppSettings,
  AppStatus,
  QuotaSnapshot,
  QuotaWindow,
  SaveSettingsInput,
} from "./lib/types";

const emptyStatus: AppStatus = {
  snapshot: null,
  refresh_status: "idle",
  last_error: null,
  last_refreshed_at: null,
};

function formatResetAt(resetAt: string | null): string {
  if (!resetAt) {
    return "暂无";
  }

  return new Intl.DateTimeFormat("zh-CN", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(resetAt));
}

function formatAgo(value: string | null): string {
  if (!value) {
    return "尚未刷新";
  }

  const diffMinutes = Math.max(
    0,
    Math.round((Date.now() - new Date(value).getTime()) / 60000),
  );

  if (diffMinutes < 1) {
    return "刚刚刷新";
  }

  return `${diffMinutes} 分钟前`;
}

function percentColor(value: number, threshold: number): string {
  if (value <= threshold) {
    return "danger";
  }
  if (value <= Math.max(threshold * 2, 25)) {
    return "warning";
  }
  return "success";
}

function clampPercent(value: number): number {
  return Math.max(0, Math.min(100, value));
}

function windowBarWidth(window: QuotaWindow | null): string {
  if (!window) {
    return "0%";
  }

  return `${clampPercent(window.remaining_percent)}%`;
}

function snapshotTone(snapshot: QuotaSnapshot | null, settings: AppSettings | null, status: AppStatus): "normal" | "low" | "error" {
  if (status.refresh_status === "error") {
    return "error";
  }
  if (!snapshot || !settings) {
    return "normal";
  }

  const lowPoint = Math.min(
    snapshot.five_hour?.remaining_percent ?? 100,
    snapshot.seven_day?.remaining_percent ?? 100,
  );

  if (lowPoint <= settings.low_quota_threshold_percent) {
    return "low";
  }
  return "normal";
}

export default function App() {
  const [status, setStatus] = useState<AppStatus>(emptyStatus);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [testing, setTesting] = useState(false);
  const [settingsMessage, setSettingsMessage] = useState<string | null>(null);
  const [form, setForm] = useState<SaveSettingsInput>({
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
      const [nextStatus, nextSettings] = await Promise.all([
        getCurrentQuota(),
        getSettings(),
      ]);
      setStatus(nextStatus);
      setSettings(nextSettings);
      setForm((current) => ({
        ...current,
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
      setLoading(false);
    }

    void bootstrap();
  }, []);

  async function handleRefresh() {
    setStatus((current) => ({ ...current, refresh_status: "refreshing" }));
    try {
      const nextStatus = await refreshQuota();
      setStatus(nextStatus);
    } catch (error) {
      setStatus((current) => ({
        ...current,
        refresh_status: "error",
        last_error: error instanceof Error ? error.message : "刷新失败",
      }));
    }
  }

  async function handleSave() {
    setSaving(true);
    setSettingsMessage(null);
    try {
      const nextSettings = await saveSettings(form);
      setSettings(nextSettings);
      setForm((current) => ({ ...current, auth_secret: "" }));
      setSettingsMessage("设置已保存");
    } catch (error) {
      setSettingsMessage(error instanceof Error ? error.message : "设置保存失败");
    } finally {
      setSaving(false);
    }
  }

  async function handleTest() {
    setTesting(true);
    setSettingsMessage(null);
    try {
      const result = await testConnection();
      setSettingsMessage(result.message);
    } catch (error) {
      setSettingsMessage(error instanceof Error ? error.message : "连接测试失败");
    } finally {
      setTesting(false);
    }
  }

  const tone = snapshotTone(status.snapshot, settings, status);
  const threshold = settings?.low_quota_threshold_percent ?? 10;

  if (loading || !settings) {
    return <main className="app-shell">加载中...</main>;
  }

  return (
    <main className="app-shell">
      <section className={`panel panel-${tone}`}>
        <header className="panel-header">
          <div className="logo-block">C</div>
          <div className="panel-actions">
            <button className="icon-button" onClick={() => void handleRefresh()}>
              {status.refresh_status === "refreshing" ? "刷新中" : "刷新"}
            </button>
          </div>
        </header>

        <div className="card">
          <div className="row account-row">
            <div>
              <div className="account-title">
                {status.snapshot?.account_name ?? settings.account_name}
              </div>
              <div className="account-subtitle">
                {settings.secret_configured ? "认证已配置" : "尚未配置认证"}
              </div>
            </div>
            <div className="refresh-time">{formatAgo(status.last_refreshed_at)}</div>
          </div>

          {tone === "error" && status.last_error ? (
            <div className="error-box">{status.last_error}</div>
          ) : null}

          <QuotaRow
            label="5H"
            window={status.snapshot?.five_hour ?? null}
            threshold={threshold}
            muted={tone === "error"}
          />

          <QuotaRow
            label="7D"
            window={status.snapshot?.seven_day ?? null}
            threshold={threshold}
            muted={tone === "error"}
          />
        </div>
      </section>

      <section className="settings-panel">
        <h1>设置</h1>

        <div className="settings-group">
          <h2>账号</h2>

          <label className="field">
            <span>账号名称</span>
            <input
              value={form.account_name}
              onChange={(event) =>
                setForm((current) => ({ ...current, account_name: event.target.value }))
              }
            />
          </label>

          <label className="field">
            <span>认证方式</span>
            <select
              value={form.auth_mode}
              onChange={(event) =>
                setForm((current) => ({
                  ...current,
                  auth_mode: event.target.value as SaveSettingsInput["auth_mode"],
                }))
              }
            >
              <option value="apiKey">API Key</option>
              <option value="sessionToken">Session Token</option>
              <option value="cookie">Cookie</option>
            </select>
          </label>

          <label className="field">
            <span>认证值</span>
            <textarea
              rows={3}
              placeholder={settings.secret_configured ? "留空表示保留当前密钥" : "输入认证值"}
              value={form.auth_secret ?? ""}
              onChange={(event) =>
                setForm((current) => ({ ...current, auth_secret: event.target.value }))
              }
            />
          </label>

          <label className="field">
            <span>ChatGPT Account ID</span>
            <input
              placeholder="可选"
              value={form.chatgpt_account_id ?? ""}
              onChange={(event) =>
                setForm((current) => ({
                  ...current,
                  chatgpt_account_id: event.target.value || null,
                }))
              }
            />
          </label>

          <label className="field">
            <span>Base URL 覆盖</span>
            <input
              placeholder="默认使用官方 Codex endpoint"
              value={form.base_url_override ?? ""}
              onChange={(event) =>
                setForm((current) => ({
                  ...current,
                  base_url_override: event.target.value || null,
                }))
              }
            />
          </label>
        </div>

        <div className="settings-group">
          <h2>刷新设置</h2>
          <label className="field">
            <span>自动刷新周期（分钟）</span>
            <input
              type="number"
              min={1}
              value={form.refresh_interval_minutes}
              onChange={(event) =>
                setForm((current) => ({
                  ...current,
                  refresh_interval_minutes: Number(event.target.value),
                }))
              }
            />
          </label>
        </div>

        <div className="settings-group">
          <h2>通知设置</h2>
          <label className="field inline-field">
            <span>低额度提醒</span>
            <input
              type="checkbox"
              checked={form.notify_on_low_quota}
              onChange={(event) =>
                setForm((current) => ({
                  ...current,
                  notify_on_low_quota: event.target.checked,
                }))
              }
            />
          </label>

          <label className="field">
            <span>提醒阈值（%）</span>
            <input
              type="number"
              min={0}
              max={100}
              value={form.low_quota_threshold_percent}
              onChange={(event) =>
                setForm((current) => ({
                  ...current,
                  low_quota_threshold_percent: Number(event.target.value),
                }))
              }
            />
          </label>

          <label className="field inline-field">
            <span>重置提醒</span>
            <input
              type="checkbox"
              checked={form.notify_on_reset}
              onChange={(event) =>
                setForm((current) => ({
                  ...current,
                  notify_on_reset: event.target.checked,
                }))
              }
            />
          </label>

          <label className="field">
            <span>提前提醒（分钟）</span>
            <input
              type="number"
              min={1}
              value={form.reset_notify_lead_minutes}
              onChange={(event) =>
                setForm((current) => ({
                  ...current,
                  reset_notify_lead_minutes: Number(event.target.value),
                }))
              }
            />
          </label>
        </div>

        <div className="button-row">
          <button className="secondary-button" disabled={testing} onClick={() => void handleTest()}>
            {testing ? "测试中..." : "测试连接"}
          </button>
          <button className="primary-button" disabled={saving} onClick={() => void handleSave()}>
            {saving ? "保存中..." : "保存设置"}
          </button>
        </div>

        {settingsMessage ? <div className="settings-message">{settingsMessage}</div> : null}
      </section>
    </main>
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
  const value = window?.remaining_percent ?? 0;
  const toneClass = muted ? "muted" : percentColor(value, threshold);
  return (
    <div className="quota-block">
      <div className="row">
        <span className="quota-label">{label}</span>
        <span className={`quota-value ${toneClass}`}>{Math.round(value)}%</span>
      </div>
      <div className="progress-track">
        <div className={`progress-fill ${toneClass}`} style={{ width: windowBarWidth(window) }} />
      </div>
      <div className="quota-reset">重置时间：{formatResetAt(window?.reset_at ?? null)}</div>
    </div>
  );
}
