import test from "node:test";
import assert from "node:assert/strict";
import {
  buildGitUsageChartRows,
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
      added_lines: 800,
      deleted_lines: 200,
      changed_files: 5,
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

test("buildGitUsageChartRows scales added, deleted, and changed file series against max bucket", () => {
  assert.deepEqual(
    buildGitUsageChartRows(report).map((row) => ({
      label: row.label,
      addedHeight: row.addedHeight,
      deletedHeight: row.deletedHeight,
      changedHeight: row.changedHeight,
    })),
    [
      { label: "00", addedHeight: 25, deletedHeight: 6, changedHeight: 1 },
      { label: "03", addedHeight: 100, deletedHeight: 20, changedHeight: 1 },
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
        displayAdded: "+800",
        displayDeleted: "-200",
        addedPercent: 52,
        deletedPercent: 13,
      },
      {
        name: "docs-site",
        displayAdded: "+450",
        displayDeleted: "-120",
        addedPercent: 29,
        deletedPercent: 8,
      },
    ],
  );
});

test("mock getGitUsage returns a complete visible report outside Tauri", async () => {
  resetMockTauriStateForTests();

  const mock = await getGitUsage("thisWeek");

  assert.equal(mock.range, "thisWeek");
  assert.ok(mock.repository_count > 0);
  assert.ok(mock.totals.added_lines > 0);
  assert.ok(mock.totals.deleted_lines > 0);
  assert.ok(mock.totals.changed_files > 0);
  assert.ok(mock.buckets.length > 0);
  assert.ok(mock.repositories.length > 0);
});
