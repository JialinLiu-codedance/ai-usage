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
  percent: number;
}

export const tokenUsageRangeLabels: Record<LocalTokenUsageRange, string> = {
  thisMonth: "本月",
  thisWeek: "本周",
  last3Days: "近3天",
  today: "今天",
};

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

export function modelUsageRows(report: LocalTokenUsageReport, limit = 3): ModelUsageDisplayRow[] {
  const maxTotal = Math.max(...report.models.map((model) => model.total_tokens), 0);
  return [...report.models]
    .sort((a, b) => b.total_tokens - a.total_tokens || a.model.localeCompare(b.model))
    .slice(0, limit)
    .map((model) => ({
      ...model,
      displayTotal: formatCompactTokens(model.total_tokens),
      percent: maxTotal > 0 ? Math.max(4, Math.round((model.total_tokens / maxTotal) * 100)) : 0,
    }));
}

export function buildTokenUsageChartRows(report: LocalTokenUsageReport): TokenUsageChartRow[] {
  const days = report.days.length > 0 ? report.days : emptyRecentDays();
  const maxTotal = Math.max(...days.map((day) => day.total_tokens), 0);
  const legend = buildTokenUsageChartLegend(report);
  return days.map((day) => {
    const totalHeight = maxTotal > 0 ? Math.round((day.total_tokens / maxTotal) * 100) : 0;
    const modelTotals = new Map((day.models ?? []).map((model) => [model.model, model.total_tokens]));
    return {
      date: day.date,
      label: formatBucketLabel(day.date, report.range),
      totalHeight,
      segments: legend.map((item) => {
        const totalTokens = modelTotals.get(item.model) ?? 0;
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

export function buildTokenUsageChartLegend(report: LocalTokenUsageReport, limit = 4): TokenUsageChartLegendItem[] {
  const models = report.models.length > 0 ? report.models : uniqueDayModels(report.days);
  return [...models]
    .sort((a, b) => b.total_tokens - a.total_tokens || a.model.localeCompare(b.model))
    .slice(0, limit)
    .map((model, index) => ({
      model: model.model,
      label: model.model,
      colorClass: chartModelColorClass(index),
    }));
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
  return `token-model-color-${index % 4}`;
}

function trimTrailingZero(value: number, integerAt = 10): string {
  const rounded = value >= integerAt ? value.toFixed(0) : value.toFixed(1);
  return rounded.replace(/\.0$/, "");
}
