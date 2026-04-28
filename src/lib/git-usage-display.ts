import type { GitUsageReport, GitUsageRepository, LocalTokenUsageRange } from "./types";

export interface GitUsageChartRow {
  date: string;
  label: string;
  addedLines: number;
  deletedLines: number;
  changedFiles: number;
  addedHeight: number;
  deletedHeight: number;
}

export interface GitUsageSummaryMetric {
  label: string;
  value: string;
  tone: "green" | "red" | "blue";
}

export interface RepositoryUsageDisplayRow extends GitUsageRepository {
  displayAdded: string;
  displayDeleted: string;
  addedPercent: number;
  deletedPercent: number;
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
    changedFiles: bucket.changed_files,
    addedHeight: scaledHeight(bucket.added_lines, maxLines),
    deletedHeight: scaledHeight(bucket.deleted_lines, maxLines),
  }));
}

export function repositoryUsageRows(report: GitUsageReport, limit?: number): RepositoryUsageDisplayRow[] {
  const sorted = [...report.repositories]
    .filter((repository) => repository.added_lines + repository.deleted_lines + repository.changed_files > 0)
    .sort((a, b) => {
      const aLineTotal = a.added_lines + a.deleted_lines;
      const bLineTotal = b.added_lines + b.deleted_lines;
      return bLineTotal - aLineTotal || b.changed_files - a.changed_files || a.name.localeCompare(b.name);
    });
  const repositories = typeof limit === "number" ? sorted.slice(0, limit) : sorted;
  const maxLineTotal = Math.max(...repositories.map((repository) => repository.added_lines + repository.deleted_lines), 0);

  return repositories.map((repository) => {
    const lineTotal = repository.added_lines + repository.deleted_lines;
    const totalPercent = scaledHeight(lineTotal, maxLineTotal);
    const addedPercent = scaledHeight(repository.added_lines, maxLineTotal);

    return {
      ...repository,
      displayAdded: `+${formatCompactLines(repository.added_lines)}`,
      displayDeleted: `-${formatCompactLines(repository.deleted_lines)}`,
      addedPercent,
      deletedPercent: Math.max(0, totalPercent - addedPercent),
    };
  });
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
