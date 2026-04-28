import type { QuotaWindow } from "./types";

export interface QuotaDisplayRowsInput {
  five_hour: QuotaWindow | null;
  seven_day: QuotaWindow | null;
}

export interface QuotaAccountCardStateInput extends QuotaDisplayRowsInput {
  fetched_at?: string | null;
  last_error?: string | null;
}

export interface QuotaDisplayRow {
  label: string;
  window: QuotaWindow;
}

export interface QuotaAccountCardState {
  error: string | null;
  muted: boolean;
  stale: boolean;
}

export function remainingQuotaProgressValue(window: QuotaWindow | null): number {
  if (!window) {
    return 0;
  }
  return Math.max(0, Math.min(100, window.remaining_percent));
}

export function quotaAccountCardState(account: QuotaAccountCardStateInput): QuotaAccountCardState {
  const error = account.last_error?.trim() ? account.last_error : null;
  const hasCachedQuota = Boolean(account.fetched_at || account.five_hour || account.seven_day);

  return {
    error,
    muted: Boolean(error),
    stale: Boolean(error && hasCachedQuota),
  };
}

export function quotaDisplayRows(account: QuotaDisplayRowsInput): QuotaDisplayRow[] {
  const rows: QuotaDisplayRow[] = [];
  if (account.five_hour) {
    rows.push({ label: account.five_hour.label?.trim() || "5H", window: account.five_hour });
  }
  if (account.seven_day) {
    rows.push({ label: account.seven_day.label?.trim() || "7D", window: account.seven_day });
  }
  return rows;
}
