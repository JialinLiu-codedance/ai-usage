import test from "node:test";
import assert from "node:assert/strict";
import { getGitUsage, getLocalTokenUsage, getPrKpi, resetMockTauriStateForTests } from "../src/lib/tauri.ts";
import {
  buildPrKpiRadarModel,
  formatPrKpiOverviewValue,
  formatPrKpiOutputRatio,
  prKpiMetricDescriptions,
} from "../src/lib/pr-kpi-display.ts";
import type { PrKpiReport } from "../src/lib/types.ts";

const report: PrKpiReport = {
  range: "thisMonth",
  generated_at: "2026-04-28T12:00:00Z",
  warnings: [],
  missing_sources: [],
  overview: {
    token_total: 1_240_000,
    code_lines: 8_432,
    output_ratio: 6.8,
  },
  metrics: [
    { key: "cycle_time_ai", label: "合入周期", score: 82, raw_value: 18, display_value: "18h", is_missing: false },
    { key: "merged_ai_prs_per_week", label: "合入频率", score: 74, raw_value: 3.7, display_value: "3.7 / 周", is_missing: false },
    { key: "review_comments_per_pr", label: "评审负担", score: 68, raw_value: 4.1, display_value: "4.1 / PR", is_missing: false },
    { key: "test_added_ratio", label: "测试占比", score: 51, raw_value: 0.18, display_value: "18%", is_missing: false },
    { key: "7d_rework_rate", label: "返工控制", score: null, raw_value: null, display_value: "N/A", is_missing: true },
    { key: "7d_retention_rate", label: "代码保留", score: 88, raw_value: 0.91, display_value: "91%", is_missing: false },
  ],
  overall_score: 72.6,
};

test("buildPrKpiRadarModel returns six axes and excludes missing scores from overall math", () => {
  const radar = buildPrKpiRadarModel(report);

  assert.equal(radar.axes.length, 6);
  assert.equal(radar.missingAxes.length, 1);
  assert.equal(radar.axes[4]?.displayValue, "N/A");
  assert.equal(radar.overallScoreLabel, "73");
});

test("format helpers render KPI overview values with compact units", () => {
  assert.equal(formatPrKpiOverviewValue(1_240_000), "1.24M");
  assert.equal(formatPrKpiOverviewValue(8_432), "8,432");
  assert.equal(formatPrKpiOutputRatio(6.8), "6.8");
  assert.equal(formatPrKpiOutputRatio(null), "N/A");
});

test("mock getPrKpi follows the shared custom date range contract", async () => {
  resetMockTauriStateForTests();

  const mock = await getPrKpi({
    kind: "custom",
    startDate: "2026-04-20",
    endDate: "2026-04-27",
  });

  assert.equal(mock.range, "custom");
  assert.equal(mock.start_date, "2026-04-20");
  assert.equal(mock.end_date, "2026-04-27");
  assert.equal(mock.metrics.length, 6);
  assert.equal(typeof mock.overview.code_lines, "number");
});

test("mock getPrKpi uses total changed code lines for per-thousand output ratio", async () => {
  resetMockTauriStateForTests();

  const selection = {
    kind: "custom" as const,
    startDate: "2026-04-20",
    endDate: "2026-04-27",
  };
  const tokenReport = await getLocalTokenUsage(selection);
  const gitReport = await getGitUsage(selection);
  const mock = await getPrKpi(selection);
  const expectedEffectiveTokens =
    tokenReport.totals.input_tokens +
    tokenReport.totals.output_tokens +
    tokenReport.totals.cache_creation_tokens +
    Math.floor(tokenReport.totals.cache_read_tokens / 10);
  const expectedOutputRatio =
    (gitReport.totals.added_lines + gitReport.totals.deleted_lines) /
    (expectedEffectiveTokens / 1_000);

  assert.equal(mock.overview.code_lines, gitReport.totals.added_lines + gitReport.totals.deleted_lines);
  assert.ok(Math.abs((mock.overview.output_ratio ?? 0) - expectedOutputRatio) < 0.000001);
  assert.ok((mock.overview.output_ratio ?? 0) > 0);
});

test("mock getPrKpi uses effective KPI tokens with cache reads discounted", async () => {
  resetMockTauriStateForTests();

  const selection = {
    kind: "custom" as const,
    startDate: "2026-04-20",
    endDate: "2026-04-27",
  };
  const tokenReport = await getLocalTokenUsage(selection);
  const mock = await getPrKpi(selection);
  const expectedEffectiveTokens =
    tokenReport.totals.input_tokens +
    tokenReport.totals.output_tokens +
    tokenReport.totals.cache_creation_tokens +
    Math.floor(tokenReport.totals.cache_read_tokens / 10);

  assert.equal(mock.overview.token_total, expectedEffectiveTokens);
  assert.notEqual(mock.overview.token_total, tokenReport.totals.total_tokens);
});

test("metric descriptions cover both 7d stability keys", () => {
  assert.equal(prKpiMetricDescriptions["7d_rework_rate"], "合入后 7 天内被删除或重写的代码比例");
  assert.equal(prKpiMetricDescriptions["7d_retention_rate"], "合入后 7 天仍然保留的代码比例");
});
