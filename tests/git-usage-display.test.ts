import test from "node:test";
import assert from "node:assert/strict";
import {
  buildGitUsageChartRows,
  commitDetailGroups,
  formatCompactLines,
  gitUsageSummaryMetrics,
  repositoryUsageRows,
} from "../src/lib/git-usage-display.ts";
import { getGitUsage, resetMockTauriStateForTests } from "../src/lib/tauri.ts";
import type { GitUsageReport } from "../src/lib/types.ts";

const report: GitUsageReport = {
  range: "last3Days",
  generated_at: "2026-04-27T12:00:00Z",
  repository_count: 2,
  missing_sources: [],
  warnings: [],
  totals: {
    added_lines: 1_240,
    deleted_lines: 320,
    changed_files: 8,
  },
  buckets: [
    {
      date: "2026-04-27T00:00:00Z",
      added_lines: 100,
      deleted_lines: 25,
      changed_files: 2,
    },
    {
      date: "2026-04-27T03:00:00Z",
      added_lines: 400,
      deleted_lines: 80,
      changed_files: 3,
    },
  ],
  repositories: [
    {
      name: "docs-site",
      path: "/Users/test/docs-site",
      added_lines: 450,
      deleted_lines: 120,
      changed_files: 4,
    },
    {
      name: "ai-usage",
      path: "/Users/test/ai-usage",
      added_lines: 1_200,
      deleted_lines: 350,
      changed_files: 7,
    },
    {
      name: "backend-api",
      path: "/Users/test/backend-api",
      added_lines: 525,
      deleted_lines: 100,
      changed_files: 5,
    },
    {
      name: "small-tool",
      path: "/Users/test/small-tool",
      added_lines: 25,
      deleted_lines: 5,
      changed_files: 1,
    },
  ],
  commits: [
    {
      commit_hash: "cccccccccccccccccccccccccccccccccccccccc",
      short_hash: "cccccccccc",
      timestamp: "2026-04-27T04:15:00Z",
      author_name: "Local User",
      author_email: "local@example.com",
      committer_name: "GitHub",
      committer_email: "noreply@github.com",
      subject: "feat: add backend endpoint (#88)",
      repository_name: "backend-api",
      repository_path: "/Users/test/backend-api",
      parent_count: 1,
      patch_id: "patch-backend-endpoint",
      duplicate_group_id: "/Users/test/backend-api::patch-backend-endpoint",
      duplicate_group_size: 2,
      is_group_representative: true,
      commit_role: "pr_merge",
      added_lines: 500,
      deleted_lines: 100,
      changed_files: 3,
    },
    {
      commit_hash: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      short_hash: "aaaaaaaaaa",
      timestamp: "2026-04-27T00:15:00Z",
      author_name: "Local User",
      author_email: "local@example.com",
      committer_name: "Local User",
      committer_email: "local@example.com",
      subject: "fix: adjust settings layout",
      repository_name: "ai-usage",
      repository_path: "/Users/test/ai-usage",
      parent_count: 1,
      patch_id: "patch-settings-layout",
      duplicate_group_id: "/Users/test/ai-usage::patch-settings-layout",
      duplicate_group_size: 1,
      is_group_representative: true,
      commit_role: "original",
      added_lines: 40,
      deleted_lines: 12,
      changed_files: 2,
    },
    {
      commit_hash: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
      short_hash: "bbbbbbbbbb",
      timestamp: "2026-04-27T02:15:00Z",
      author_name: "Local User",
      author_email: "local@example.com",
      committer_name: "Local User",
      committer_email: "local@example.com",
      subject: "feat: add backend endpoint",
      repository_name: "backend-api",
      repository_path: "/Users/test/backend-api",
      parent_count: 1,
      patch_id: "patch-backend-endpoint",
      duplicate_group_id: "/Users/test/backend-api::patch-backend-endpoint",
      duplicate_group_size: 2,
      is_group_representative: false,
      commit_role: "original",
      added_lines: 500,
      deleted_lines: 100,
      changed_files: 3,
    },
    {
      commit_hash: "dddddddddddddddddddddddddddddddddddddddd",
      short_hash: "dddddddddd",
      timestamp: "2026-04-27T03:15:00Z",
      author_name: "Local User",
      author_email: "local@example.com",
      committer_name: "Local User",
      committer_email: "local@example.com",
      subject: "",
      repository_name: "backend-api",
      repository_path: "/Users/test/backend-api",
      parent_count: 1,
      patch_id: "patch-backend-docs",
      duplicate_group_id: "/Users/test/backend-api::patch-backend-docs",
      duplicate_group_size: 1,
      is_group_representative: true,
      commit_role: "original",
      added_lines: 25,
      deleted_lines: 0,
      changed_files: 0,
    },
  ],
};

test("formatCompactLines uses compact K and M suffixes", () => {
  assert.equal(formatCompactLines(999), "999");
  assert.equal(formatCompactLines(12_400), "12.4K");
  assert.equal(formatCompactLines(2_400_000), "2.4M");
});

test("gitUsageSummaryMetrics formats added, deleted, and changed file totals", () => {
  assert.deepEqual(gitUsageSummaryMetrics(report), [
    { label: "新增行数", value: "1.2K", tone: "green" },
    { label: "删除行数", value: "320", tone: "red" },
    { label: "修改文件数", value: "8", tone: "blue" },
  ]);
});

test("buildGitUsageChartRows scales added and deleted line series against max bucket", () => {
  assert.deepEqual(
    buildGitUsageChartRows(report).map((row) => ({
      label: row.label,
      addedHeight: row.addedHeight,
      deletedHeight: row.deletedHeight,
    })),
    [
      { label: "00", addedHeight: 25, deletedHeight: 6 },
      { label: "03", addedHeight: 100, deletedHeight: 20 },
    ],
  );
});

test("buildGitUsageChartRows scales only line-count series", () => {
  const lineOnlyReport: GitUsageReport = {
    ...report,
    range: "thisMonth",
    buckets: [
      {
        date: "2026-04-27",
        added_lines: 10,
        deleted_lines: 5,
        changed_files: 500,
      },
      {
        date: "2026-04-28",
        added_lines: 20,
        deleted_lines: 10,
        changed_files: 1,
      },
    ],
  };

  assert.deepEqual(
    buildGitUsageChartRows(lineOnlyReport).map((row) => ({
      label: row.label,
      addedHeight: row.addedHeight,
      deletedHeight: row.deletedHeight,
    })),
    [
      { label: "27", addedHeight: 50, deletedHeight: 25 },
      { label: "28", addedHeight: 100, deletedHeight: 50 },
    ],
  );
});

test("repositoryUsageRows sorts repositories and scales bars against the highest line total", () => {
  assert.deepEqual(
    repositoryUsageRows(report).map((row) => ({
      name: row.name,
      displayAdded: row.displayAdded,
      displayDeleted: row.displayDeleted,
      addedPercent: row.addedPercent,
      deletedPercent: row.deletedPercent,
    })),
    [
      {
        name: "ai-usage",
        displayAdded: "+1.2K",
        displayDeleted: "-350",
        addedPercent: 77,
        deletedPercent: 23,
      },
      {
        name: "backend-api",
        displayAdded: "+525",
        displayDeleted: "-100",
        addedPercent: 34,
        deletedPercent: 6,
      },
      {
        name: "docs-site",
        displayAdded: "+450",
        displayDeleted: "-120",
        addedPercent: 29,
        deletedPercent: 8,
      },
      {
        name: "small-tool",
        displayAdded: "+25",
        displayDeleted: "-5",
        addedPercent: 2,
        deletedPercent: 0,
      },
    ],
  );
});

test("repositoryUsageRows returns all counted repositories by default", () => {
  assert.deepEqual(
    repositoryUsageRows(report).map((row) => row.name),
    ["ai-usage", "backend-api", "docs-site", "small-tool"],
  );
});

test("commitDetailGroups groups duplicate patch entries under a representative row", () => {
  assert.deepEqual(
    commitDetailGroups(report).map((group) => ({
      name: group.name,
      totalAdded: group.totalAdded,
      totalDeleted: group.totalDeleted,
      summaryHashes: group.items.map((item) => item.summary.shortHash),
      summaryTitles: group.items.map((item) => item.summary.subject),
      summaryAdded: group.items.map((item) => item.summary.displayAdded),
      nestedRoles: group.items.map((item) => item.members.map((commit) => commit.roleLabel)),
    })),
    [
      {
        name: "ai-usage",
        totalAdded: 1200,
        totalDeleted: 350,
        summaryHashes: ["aaaaaaaaaa"],
        summaryTitles: ["fix: adjust settings layout"],
        summaryAdded: ["+40"],
        nestedRoles: [["原始提交"]],
      },
      {
        name: "backend-api",
        totalAdded: 525,
        totalDeleted: 100,
        summaryHashes: ["cccccccccc", "dddddddddd"],
        summaryTitles: ["feat: add backend endpoint (#88)", "未命名提交"],
        summaryAdded: ["+500", "+25"],
        nestedRoles: [["PR 合并", "原始提交"], ["原始提交"]],
      },
    ],
  );
});

test("mock getGitUsage returns a complete visible report outside Tauri", async () => {
  resetMockTauriStateForTests();

  const mock = await getGitUsage({ kind: "preset", range: "thisWeek" });

  assert.equal(mock.range, "thisWeek");
  assert.ok(mock.repository_count > 0);
  assert.ok(mock.totals.added_lines > 0);
  assert.ok(mock.totals.deleted_lines > 0);
  assert.ok(mock.totals.changed_files > 0);
  assert.ok(mock.buckets.length > 0);
  assert.ok(mock.repositories.length > 0);
  assert.ok(mock.commits.length > 0);
});

test("mock getGitUsage supports a daily custom date range", async () => {
  resetMockTauriStateForTests();

  const mock = await getGitUsage({
    kind: "custom",
    startDate: "2026-04-20",
    endDate: "2026-04-22",
  });

  assert.equal(mock.range, "custom");
  assert.equal(mock.start_date, "2026-04-20");
  assert.equal(mock.end_date, "2026-04-22");
  assert.deepEqual(
    mock.buckets.map((bucket) => bucket.date),
    ["2026-04-20", "2026-04-21", "2026-04-22"],
  );
  assert.deepEqual(
    buildGitUsageChartRows(mock).map((row) => row.label),
    ["04/20", "04/21", "04/22"],
  );
});

test("mock getGitUsage returns every hour for today's full day", async () => {
  resetMockTauriStateForTests();

  const mock = await getGitUsage({ kind: "preset", range: "today" });

  assert.equal(mock.buckets.length, 24);
  assert.deepEqual(
    buildGitUsageChartRows(mock).map((row) => row.label),
    Array.from({ length: 24 }, (_, hour) => String(hour).padStart(2, "0")),
  );
});

test("mock getGitUsage returns every day for the selected week and month", async () => {
  resetMockTauriStateForTests();

  const week = await getGitUsage({ kind: "preset", range: "thisWeek" });
  const month = await getGitUsage({ kind: "preset", range: "thisMonth" });

  assert.equal(week.buckets.length, 7);
  assert.equal(month.buckets.length, daysInMonth(month.buckets[0].date));
  assert.equal(month.buckets[0].date.endsWith("-01"), true);
});

function daysInMonth(dateKey: string): number {
  const [year, month] = dateKey.split("-").map((part) => Number.parseInt(part, 10));
  return new Date(year, month, 0).getDate();
}
