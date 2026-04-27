import type { GitUsageReport, LocalTokenUsageRange } from "./types";

export interface GitUsageChartRow {
  date: string;
  label: string;
  addedLines: number;
  deletedLines: number;
  addedHeight: number;
  deletedHeight: number;
}

export interface GitUsageSummaryMetric {
  label: string;
  value: string;
  tone: "green" | "red" | "blue";
}

export function formatCompactLines(value: number): string {
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

export function gitUsageSummaryMetrics(report: GitUsageReport): GitUsageSummaryMetric[] {
  return [
    { label: "新增行数", value: formatCompactLines(report.totals.added_lines), tone: "green" },
    { label: "删除行数", value: formatCompactLines(report.totals.deleted_lines), tone: "red" },
    { label: "修改文件数", value: formatCompactLines(report.totals.changed_files), tone: "blue" },
  ];
}

export function buildGitUsageChartRows(report: GitUsageReport): GitUsageChartRow[] {
  const maxLines = Math.max(
    ...report.buckets.flatMap((bucket) => [bucket.added_lines, bucket.deleted_lines]),
    0,
  );

  return report.buckets.map((bucket) => ({
    date: bucket.date,
    label: formatBucketLabel(bucket.date, report.range),
    addedLines: bucket.added_lines,
    deletedLines: bucket.deleted_lines,
    addedHeight: scaledHeight(bucket.added_lines, maxLines),
    deletedHeight: scaledHeight(bucket.deleted_lines, maxLines),
  }));
}

function scaledHeight(value: number, maxValue: number): number {
  if (maxValue <= 0 || value <= 0) {
    return 0;
  }
  return Math.round((value / maxValue) * 100);
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

function trimTrailingZero(value: number, integerAt = 10): string {
  const rounded = value >= integerAt ? value.toFixed(0) : value.toFixed(1);
  return rounded.replace(/\.0$/, "");
}
