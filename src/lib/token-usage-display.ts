import type {
  LocalTokenUsageDay,
  LocalTokenUsageModel,
  LocalTokenUsageRange,
  LocalTokenUsageReport,
} from "./types";

export interface TokenUsageChartRow {
  date: string;
  label: string;
  totalHeight: number;
  segments: TokenUsageChartSegment[];
}

export interface TokenUsageChartSegment {
  model: string;
  height: number;
  totalTokens: number;
  colorClass: string;
}

export interface TokenUsageChartLegendItem {
  model: string;
  label: string;
  colorClass: string;
}

export interface ModelUsageDisplayRow extends LocalTokenUsageModel {
  displayTotal: string;
  displayInput: string;
  displayOutput: string;
  displayCacheRead: string;
  displayCacheCreation: string;
  percent: number;
}

export const tokenUsageRangeLabels: Record<LocalTokenUsageRange, string> = {
  thisMonth: "本月",
  thisWeek: "本周",
  last3Days: "近3天",
  today: "今天",
  custom: "自定义",
};

const TOKEN_CHART_PRIMARY_MODEL_LIMIT = 5;
const TOKEN_CHART_OTHER_MODEL = "__other__";

export function formatCompactTokens(value: number): string {
  if (value >= 1_000_000_000) {
    return `${trimTrailingZero(value / 1_000_000_000)}B`;
  }
  if (value >= 1_000_000) {
    return `${trimTrailingZero(value / 1_000_000)}M`;
  }
  if (value >= 1_000) {
    return `${trimTrailingZero(value / 1_000, 100)}K`;
  }
  return `${Math.round(value)}`;
}

export function modelUsageRows(report: LocalTokenUsageReport, limit?: number): ModelUsageDisplayRow[] {
  const maxTotal = Math.max(...report.models.map((model) => model.total_tokens), 0);
  const sorted = [...report.models]
    .sort((a, b) => b.total_tokens - a.total_tokens || a.model.localeCompare(b.model));
  const rows = typeof limit === "number" ? sorted.slice(0, limit) : sorted;
  return rows.map((model) => ({
    ...model,
    displayTotal: formatCompactTokens(model.total_tokens),
    displayInput: formatCompactTokens(model.input_tokens),
    displayOutput: formatCompactTokens(model.output_tokens),
    displayCacheRead: formatCompactTokens(model.cache_read_tokens),
    displayCacheCreation: formatCompactTokens(model.cache_creation_tokens),
    percent: maxTotal > 0 ? Math.max(4, Math.round((model.total_tokens / maxTotal) * 100)) : 0,
  }));
}

export function buildTokenUsageChartRows(report: LocalTokenUsageReport): TokenUsageChartRow[] {
  const days = report.days.length > 0 ? report.days : emptyRecentDays();
  const maxTotal = Math.max(...days.map((day) => day.total_tokens), 0);
  const legend = buildTokenUsageChartLegend(report);
  const primaryModels = new Set(
    legend
      .filter((item) => item.model !== TOKEN_CHART_OTHER_MODEL)
      .map((item) => item.model),
  );
  const hasOther = legend.some((item) => item.model === TOKEN_CHART_OTHER_MODEL);
  return days.map((day) => {
    const totalHeight = maxTotal > 0 ? Math.round((day.total_tokens / maxTotal) * 100) : 0;
    const modelTotals = new Map((day.models ?? []).map((model) => [model.model, model.total_tokens]));
    return {
      date: day.date,
      label: formatBucketLabel(day.date, report.range),
      totalHeight,
      segments: legend.map((item) => {
        const totalTokens = item.model === TOKEN_CHART_OTHER_MODEL
          ? otherModelTotal(day.models ?? [], primaryModels, hasOther)
          : modelTotals.get(item.model) ?? 0;
        return {
          model: item.model,
          height: scaledSegment(totalTokens, maxTotal),
          totalTokens,
          colorClass: item.colorClass,
        };
      }),
    };
  });
}

export function buildTokenUsageChartLegend(
  report: LocalTokenUsageReport,
  primaryLimit = TOKEN_CHART_PRIMARY_MODEL_LIMIT,
): TokenUsageChartLegendItem[] {
  const models = report.models.length > 0 ? report.models : uniqueDayModels(report.days);
  const sorted = [...models]
    .sort((a, b) => b.total_tokens - a.total_tokens || a.model.localeCompare(b.model));
  const primaryModels = sorted.slice(0, primaryLimit);
  const items = primaryModels
    .map((model, index) => ({
      model: model.model,
      label: model.model,
      colorClass: chartModelColorClass(index),
    }));
  if (sorted.length > primaryLimit) {
    items.push({
      model: TOKEN_CHART_OTHER_MODEL,
      label: "其他",
      colorClass: chartModelColorClass(primaryModels.length),
    });
  }
  return items;
}

export function usageToolLabel(tool: string): string {
  if (tool === "claude") {
    return "Claude Code";
  }
  if (tool === "codex") {
    return "Codex CLI";
  }
  if (tool === "opencode") {
    return "OpenCode";
  }
  if (tool === "kimi") {
    return "Kimi CLI";
  }
  return tool;
}

function scaledSegment(value: number, maxTotal: number): number {
  if (maxTotal <= 0 || value <= 0) {
    return 0;
  }
  return Math.round((value / maxTotal) * 100);
}

function otherModelTotal(
  models: LocalTokenUsageModel[],
  primaryModels: Set<string>,
  hasOther: boolean,
): number {
  if (!hasOther) {
    return 0;
  }
  return models
    .filter((model) => !primaryModels.has(model.model))
    .reduce((sum, model) => sum + model.total_tokens, 0);
}

function formatBucketLabel(date: string, range: LocalTokenUsageRange): string {
  const hourlyMatch = date.match(/^\d{4}-\d{2}-\d{2}T(\d{2}):/);
  if (hourlyMatch) {
    return hourlyMatch[1];
  }
  const [year, month, day] = date.split("-");
  if (!year || !month || !day) {
    return date;
  }
  if (range === "thisMonth") {
    return String(Number.parseInt(day, 10));
  }
  return `${month}/${day}`;
}

function emptyRecentDays(): LocalTokenUsageDay[] {
  return Array.from({ length: 7 }, (_, index) => {
    const date = new Date();
    date.setDate(date.getDate() - (6 - index));
    return {
      date: date.toISOString().slice(0, 10),
      input_tokens: 0,
      output_tokens: 0,
      cache_read_tokens: 0,
      cache_creation_tokens: 0,
      total_tokens: 0,
      models: [],
    };
  });
}

function uniqueDayModels(days: LocalTokenUsageDay[]): LocalTokenUsageModel[] {
  const totals = new Map<string, LocalTokenUsageModel>();
  for (const day of days) {
    for (const model of day.models ?? []) {
      const current = totals.get(model.model);
      if (!current) {
        totals.set(model.model, { ...model });
        continue;
      }
      current.input_tokens += model.input_tokens;
      current.output_tokens += model.output_tokens;
      current.cache_read_tokens += model.cache_read_tokens;
      current.cache_creation_tokens += model.cache_creation_tokens;
      current.total_tokens += model.total_tokens;
    }
  }
  return [...totals.values()];
}

function chartModelColorClass(index: number): string {
  return `token-model-color-${index % 6}`;
}

function trimTrailingZero(value: number, integerAt = 10): string {
  const rounded = value >= integerAt ? value.toFixed(0) : value.toFixed(1);
  return rounded.replace(/\.0$/, "");
}
