import type { GitUsageCommit, GitUsageReport, GitUsageRepository, LocalTokenUsageRange } from "./types";

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

export interface CommitDetailDisplayRow extends GitUsageCommit {
  shortHash: string;
  subject: string;
  timeLabel: string;
  displayAdded: string;
  displayDeleted: string;
  roleLabel: string;
}

export interface CommitDetailItem {
  duplicateGroupId: string;
  summary: CommitDetailDisplayRow;
  members: CommitDetailDisplayRow[];
}

export interface CommitDetailGroup {
  name: string;
  path: string;
  totalAdded: number;
  totalDeleted: number;
  items: CommitDetailItem[];
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

export function commitDetailGroups(report: GitUsageReport): CommitDetailGroup[] {
  const totalsByPath = new Map(
    report.repositories.map((repository) => [
      repository.path,
      {
        totalAdded: repository.added_lines,
        totalDeleted: repository.deleted_lines,
      },
    ]),
  );
  const groupsByPath = new Map<string, CommitDetailGroup>();

  for (const commit of report.commits) {
    const path = commit.repository_path;
    if (!path) {
      continue;
    }

    const group = groupsByPath.get(path) ?? {
      name: commit.repository_name || "repository",
      path,
      totalAdded: totalsByPath.get(path)?.totalAdded ?? 0,
      totalDeleted: totalsByPath.get(path)?.totalDeleted ?? 0,
      items: [],
    };
    groupsByPath.set(path, group);
  }

  const groups = [...groupsByPath.values()]
    .filter((group) => report.commits.some((commit) => commit.repository_path === group.path))
    .sort((a, b) => {
      const aLineTotal = a.totalAdded + a.totalDeleted;
      const bLineTotal = b.totalAdded + b.totalDeleted;
      return bLineTotal - aLineTotal || a.name.localeCompare(b.name);
    });

  for (const group of groups) {
    const membersByDuplicateGroup = new Map<string, CommitDetailDisplayRow[]>();
    for (const commit of report.commits) {
      if (commit.repository_path !== group.path) {
        continue;
      }
      const duplicateGroupId = commit.duplicate_group_id.trim() || commit.commit_hash;
      const members = membersByDuplicateGroup.get(duplicateGroupId) ?? [];
      members.push(decorateCommitDetail(commit));
      membersByDuplicateGroup.set(duplicateGroupId, members);
    }

    group.items = [...membersByDuplicateGroup.entries()]
      .map(([duplicateGroupId, members]) => {
        members.sort(compareCommitDisplayRows);
        const summary =
          members.find((member) => member.is_group_representative) ??
          members[0];
        return {
          duplicateGroupId,
          summary,
          members,
        };
      })
      .sort((left, right) => compareCommitDisplayRows(left.summary, right.summary));
  }

  return groups;
}

function decorateCommitDetail(commit: GitUsageCommit): CommitDetailDisplayRow {
  return {
    ...commit,
    shortHash: commit.short_hash || commit.commit_hash.slice(0, 10),
    subject: commit.subject.trim() || "未命名提交",
    timeLabel: formatCommitTime(commit.timestamp),
    displayAdded: `+${formatCompactLines(commit.added_lines)}`,
    displayDeleted: `-${formatCompactLines(commit.deleted_lines)}`,
    roleLabel: formatCommitRole(commit.commit_role),
  };
}

function compareCommitDisplayRows(left: CommitDetailDisplayRow, right: CommitDetailDisplayRow): number {
  const leftTime = Date.parse(left.timestamp);
  const rightTime = Date.parse(right.timestamp);
  return rightTime - leftTime || left.commit_hash.localeCompare(right.commit_hash);
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

function formatCommitTime(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) {
    return value;
  }
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  const hours = String(date.getHours()).padStart(2, "0");
  const minutes = String(date.getMinutes()).padStart(2, "0");
  return `${month}/${day} ${hours}:${minutes}`;
}

function formatCommitRole(role: string): string {
  if (role === "pr_merge") {
    return "PR 合并";
  }
  if (role === "duplicate") {
    return "重复提交";
  }
  return "原始提交";
}

function trimTrailingZero(value: number, integerAt = 10): string {
  const rounded = value >= integerAt ? value.toFixed(0) : value.toFixed(1);
  return rounded.replace(/\.0$/, "");
}
