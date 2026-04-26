import type { QuotaWindow } from "./types";

export interface QuotaDisplayRowsInput {
  five_hour: QuotaWindow | null;
  seven_day: QuotaWindow | null;
}

export interface QuotaDisplayRow {
  label: string;
  window: QuotaWindow;
}

export function remainingQuotaProgressValue(window: QuotaWindow | null): number {
  if (!window) {
    return 0;
  }
  return Math.max(0, Math.min(100, window.remaining_percent));
}

export function quotaDisplayRows(account: QuotaDisplayRowsInput): QuotaDisplayRow[] {
  const rows: QuotaDisplayRow[] = [];
  if (account.five_hour) {
    rows.push({ label: "5H", window: account.five_hour });
  }
  if (account.seven_day) {
    rows.push({ label: "7D", window: account.seven_day });
  }
  return rows;
}
