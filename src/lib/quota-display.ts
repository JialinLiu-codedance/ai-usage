import type { QuotaWindow } from "./types";

export function remainingQuotaProgressValue(window: QuotaWindow | null): number {
  if (!window) {
    return 0;
  }
  return Math.max(0, Math.min(100, window.remaining_percent));
}
