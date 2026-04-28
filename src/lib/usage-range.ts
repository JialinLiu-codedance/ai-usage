import type { PresetUsageRange, UsageRangeSelection } from "./types";

export type UsageRangeOption = PresetUsageRange | "custom";
export type CustomUsageRangeDraft = Extract<UsageRangeSelection, { kind: "custom" }>;

export interface UsageRangeUiState {
  selectedOption: UsageRangeOption;
  appliedSelection: UsageRangeSelection;
  customDraft: CustomUsageRangeDraft;
}

export interface PendingLikeReport {
  pending?: boolean;
}

export interface VisibleReportState {
  visibleKey: string | null;
  showingFallback: boolean;
}

export const CUSTOM_USAGE_WINDOW_DAYS = 90;

export function createUsageRangeUiState(now = new Date()): UsageRangeUiState {
  return {
    selectedOption: "thisMonth",
    appliedSelection: { kind: "preset", range: "thisMonth" },
    customDraft: defaultCustomUsageRangeDraft(now),
  };
}

export function selectUsageRangeOption(state: UsageRangeUiState, option: UsageRangeOption): UsageRangeUiState {
  if (option === "custom") {
    return { ...state, selectedOption: option };
  }
  return {
    ...state,
    selectedOption: option,
    appliedSelection: { kind: "preset", range: option },
  };
}

export function updateCustomRangeDraft(
  state: UsageRangeUiState,
  field: "startDate" | "endDate",
  value: string,
): UsageRangeUiState {
  return {
    ...state,
    customDraft: {
      ...state.customDraft,
      [field]: value,
    },
  };
}

export function applyCustomRangeDraft(state: UsageRangeUiState): UsageRangeUiState {
  return {
    ...state,
    selectedOption: "custom",
    appliedSelection: {
      kind: "custom",
      startDate: state.customDraft.startDate,
      endDate: state.customDraft.endDate,
    },
  };
}

export function defaultCustomUsageRangeDraft(now = new Date()): CustomUsageRangeDraft {
  const maxDate = startOfLocalDay(now);
  const suggestedStart = addLocalDays(maxDate, -7);
  const { minDate } = customUsageWindowBounds(now);
  const min = parseDateInput(minDate) ?? suggestedStart;
  const start = suggestedStart < min ? min : suggestedStart;
  return {
    kind: "custom",
    startDate: localDateInputValue(start),
    endDate: localDateInputValue(maxDate),
  };
}

export function customUsageWindowBounds(now = new Date()): { minDate: string; maxDate: string } {
  const max = startOfLocalDay(now);
  const min = addLocalDays(max, -(CUSTOM_USAGE_WINDOW_DAYS - 1));
  return {
    minDate: localDateInputValue(min),
    maxDate: localDateInputValue(max),
  };
}

export function usageRangeSelectionKey(selection: UsageRangeSelection): string {
  if (selection.kind === "custom") {
    return `custom:${selection.startDate}:${selection.endDate}`;
  }
  return selection.range;
}

export function validateCustomUsageRangeSelection(
  selection: CustomUsageRangeDraft,
  now = new Date(),
): string | null {
  const start = parseDateInput(selection.startDate);
  if (!start) {
    return "自定义开始日期格式无效，请使用 YYYY-MM-DD";
  }
  const end = parseDateInput(selection.endDate);
  if (!end) {
    return "自定义结束日期格式无效，请使用 YYYY-MM-DD";
  }
  if (start.getTime() > end.getTime()) {
    return "自定义开始日期不能晚于结束日期";
  }

  const { minDate, maxDate } = customUsageWindowBounds(now);
  const min = parseDateInput(minDate);
  const max = parseDateInput(maxDate);
  if (min && start.getTime() < min.getTime()) {
    return `自定义开始日期不能早于 ${minDate}`;
  }
  if (max && end.getTime() > max.getTime()) {
    return "自定义结束日期不能晚于今天";
  }
  return null;
}

export function resolveVisibleReportState(
  requestedKey: string,
  requestedReport: PendingLikeReport | null,
  lastReadyKey: string | null,
  lastReadyReport: PendingLikeReport | null,
): VisibleReportState {
  if (requestedReport && !requestedReport.pending) {
    return {
      visibleKey: requestedKey,
      showingFallback: false,
    };
  }
  if (lastReadyKey && lastReadyReport && lastReadyKey !== requestedKey) {
    return {
      visibleKey: lastReadyKey,
      showingFallback: true,
    };
  }
  if (requestedReport) {
    return {
      visibleKey: requestedKey,
      showingFallback: false,
    };
  }
  if (lastReadyKey && lastReadyReport) {
    return {
      visibleKey: lastReadyKey,
      showingFallback: false,
    };
  }
  return {
    visibleKey: null,
    showingFallback: false,
  };
}

export function parseDateInput(value: string): Date | null {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) {
    return null;
  }
  const [year, month, day] = value.split("-").map((part) => Number.parseInt(part, 10));
  const parsed = new Date(year, month - 1, day);
  if (parsed.getFullYear() !== year || parsed.getMonth() !== month - 1 || parsed.getDate() !== day) {
    return null;
  }
  return parsed;
}

export function addLocalDays(date: Date, days: number): Date {
  const next = new Date(date);
  next.setDate(next.getDate() + days);
  return next;
}

export function startOfLocalDay(date: Date): Date {
  const next = new Date(date);
  next.setHours(0, 0, 0, 0);
  return next;
}

export function localDateInputValue(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
}
