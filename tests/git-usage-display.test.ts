import test from "node:test";
import assert from "node:assert/strict";
import {
  buildGitUsageChartRows,
  formatCompactLines,
  gitUsageSummaryMetrics,
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

test("buildGitUsageChartRows scales added and deleted series against max line bucket", () => {
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

test("mock getGitUsage returns a complete visible report outside Tauri", async () => {
  resetMockTauriStateForTests();

  const mock = await getGitUsage("thisWeek");

  assert.equal(mock.range, "thisWeek");
  assert.ok(mock.repository_count > 0);
  assert.ok(mock.totals.added_lines > 0);
  assert.ok(mock.totals.deleted_lines > 0);
  assert.ok(mock.totals.changed_files > 0);
  assert.ok(mock.buckets.length > 0);
});
