import test from "node:test";
import assert from "node:assert/strict";
import {
  buildTokenUsageChartLegend,
  buildTokenUsageChartRows,
  formatCompactTokens,
  modelUsageRows,
} from "../src/lib/token-usage-display.ts";
import { getLocalTokenUsage, resetMockTauriStateForTests } from "../src/lib/tauri.ts";
import type { LocalTokenUsageReport } from "../src/lib/types.ts";

const report: LocalTokenUsageReport = {
  range: "thisMonth",
  generated_at: "2026-04-27T12:00:00Z",
  missing_sources: [],
  warnings: [],
  totals: {
    input_tokens: 1_200_000,
    output_tokens: 240_000,
    cache_read_tokens: 800_000,
    cache_creation_tokens: 160_000,
    total_tokens: 2_400_000,
    cache_hit_rate_percent: 40,
  },
  days: [
    {
      date: "2026-04-26",
      input_tokens: 100,
      output_tokens: 50,
      cache_read_tokens: 25,
      cache_creation_tokens: 25,
      total_tokens: 200,
      models: [],
    },
    {
      date: "2026-04-27",
      input_tokens: 300,
      output_tokens: 150,
      cache_read_tokens: 50,
      cache_creation_tokens: 0,
      total_tokens: 500,
      models: [],
    },
  ],
  models: [
    {
      model: "small-model",
      input_tokens: 10,
      output_tokens: 10,
      cache_read_tokens: 0,
      cache_creation_tokens: 0,
      total_tokens: 20,
    },
    {
      model: "large-model",
      input_tokens: 100,
      output_tokens: 50,
      cache_read_tokens: 30,
      cache_creation_tokens: 20,
      total_tokens: 200,
    },
  ],
  tools: [],
};

test("formatCompactTokens uses compact K, M, and B suffixes", () => {
  assert.equal(formatCompactTokens(999), "999");
  assert.equal(formatCompactTokens(12_400), "12.4K");
  assert.equal(formatCompactTokens(2_400_000), "2.4M");
  assert.equal(formatCompactTokens(6_419_000_000), "6.4B");
});

test("modelUsageRows sorts models by total token usage descending", () => {
  assert.deepEqual(
    modelUsageRows(report).map((row) => [row.model, row.displayTotal]),
    [
      ["large-model", "200"],
      ["small-model", "20"],
    ],
  );
});

test("modelUsageRows returns all models by default", () => {
  const modelReport: LocalTokenUsageReport = {
    ...report,
    models: [
      ...report.models,
      {
        model: "medium-model",
        input_tokens: 30,
        output_tokens: 20,
        cache_read_tokens: 10,
        cache_creation_tokens: 0,
        total_tokens: 60,
      },
      {
        model: "tiny-model",
        input_tokens: 1,
        output_tokens: 1,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        total_tokens: 2,
      },
    ],
  };

  assert.deepEqual(
    modelUsageRows(modelReport).map((row) => row.model),
    ["large-model", "medium-model", "small-model", "tiny-model"],
  );
});

test("modelUsageRows formats token component columns", () => {
  const [row] = modelUsageRows(report);

  assert.equal(row.displayInput, "100");
  assert.equal(row.displayOutput, "50");
  assert.equal(row.displayCacheRead, "30");
  assert.equal(row.displayCacheCreation, "20");
});

test("buildTokenUsageChartRows scales daily token bars against max day", () => {
  assert.deepEqual(
    buildTokenUsageChartRows(report).map((row) => ({
      label: row.label,
      totalHeight: row.totalHeight,
      segments: row.segments.map((segment) => [segment.model, segment.height]),
    })),
    [
      { label: "26", totalHeight: 40, segments: [["large-model", 0], ["small-model", 0]] },
      { label: "27", totalHeight: 100, segments: [["large-model", 0], ["small-model", 0]] },
    ],
  );
});

test("buildTokenUsageChartRows stacks usage by model", () => {
  const modelReport: LocalTokenUsageReport = {
    ...report,
    days: [
      {
        date: "2026-04-27",
        input_tokens: 100,
        output_tokens: 50,
        cache_read_tokens: 30,
        cache_creation_tokens: 20,
        total_tokens: 200,
        models: [
          {
            model: "large-model",
            input_tokens: 60,
            output_tokens: 20,
            cache_read_tokens: 10,
            cache_creation_tokens: 10,
            total_tokens: 100,
          },
          {
            model: "small-model",
            input_tokens: 40,
            output_tokens: 30,
            cache_read_tokens: 20,
            cache_creation_tokens: 10,
            total_tokens: 100,
          },
        ],
      },
    ],
  };

  assert.deepEqual(
    buildTokenUsageChartRows(modelReport)[0].segments.map((segment) => ({
      model: segment.model,
      height: segment.height,
    })),
    [
      { model: "large-model", height: 50 },
      { model: "small-model", height: 50 },
    ],
  );
});

test("buildTokenUsageChartRows includes non-primary models in the other segment", () => {
  const overflowReport: LocalTokenUsageReport = {
    ...report,
    models: [
      tokenModel("model-a", 600),
      tokenModel("model-b", 500),
      tokenModel("model-c", 400),
      tokenModel("model-d", 300),
      tokenModel("model-e", 200),
      tokenModel("model-f", 100),
    ],
    days: [
      {
        date: "2026-04-27",
        input_tokens: 2_100,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        total_tokens: 2_100,
        models: [
          tokenModel("model-a", 600),
          tokenModel("model-b", 500),
          tokenModel("model-c", 400),
          tokenModel("model-d", 300),
          tokenModel("model-e", 200),
          tokenModel("model-f", 100),
        ],
      },
    ],
  };

  assert.deepEqual(
    buildTokenUsageChartLegend(overflowReport).map((item) => item.label),
    ["model-a", "model-b", "model-c", "model-d", "model-e", "其他"],
  );
  assert.equal(
    buildTokenUsageChartRows(overflowReport)[0].segments.reduce((sum, segment) => sum + segment.totalTokens, 0),
    2_100,
  );
  assert.deepEqual(
    buildTokenUsageChartRows(overflowReport)[0].segments.at(-1),
    {
      model: "__other__",
      height: 5,
      totalTokens: 100,
      colorClass: "token-model-color-5",
    },
  );
});

test("buildTokenUsageChartRows formats hourly bucket labels", () => {
  const hourlyReport: LocalTokenUsageReport = {
    ...report,
    range: "today",
    days: [
      {
        date: "2026-04-27T00:00:00Z",
        input_tokens: 10,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        total_tokens: 10,
        models: [],
      },
      {
        date: "2026-04-27T01:00:00Z",
        input_tokens: 20,
        output_tokens: 0,
        cache_read_tokens: 0,
        cache_creation_tokens: 0,
        total_tokens: 20,
        models: [],
      },
    ],
  };

  assert.deepEqual(
    buildTokenUsageChartRows(hourlyReport).map((row) => row.label),
    ["00", "01"],
  );
});

test("mock getLocalTokenUsage supports a daily custom date range", async () => {
  resetMockTauriStateForTests();

  const mock = await getLocalTokenUsage({
    kind: "custom",
    startDate: "2026-04-20",
    endDate: "2026-04-22",
  });

  assert.equal(mock.range, "custom");
  assert.equal(mock.start_date, "2026-04-20");
  assert.equal(mock.end_date, "2026-04-22");
  assert.deepEqual(
    mock.days.map((day) => day.date),
    ["2026-04-20", "2026-04-21", "2026-04-22"],
  );
  assert.deepEqual(
    buildTokenUsageChartRows(mock).map((row) => row.label),
    ["04/20", "04/21", "04/22"],
  );
});

test("mock getLocalTokenUsage returns every hour for today's full day", async () => {
  resetMockTauriStateForTests();

  const mock = await getLocalTokenUsage({ kind: "preset", range: "today" });

  assert.equal(mock.days.length, 24);
  assert.deepEqual(
    buildTokenUsageChartRows(mock).map((row) => row.label),
    Array.from({ length: 24 }, (_, hour) => String(hour).padStart(2, "0")),
  );
});

test("mock getLocalTokenUsage returns every day for the selected week and month", async () => {
  resetMockTauriStateForTests();

  const week = await getLocalTokenUsage({ kind: "preset", range: "thisWeek" });
  const month = await getLocalTokenUsage({ kind: "preset", range: "thisMonth" });

  assert.equal(week.days.length, 7);
  assert.equal(month.days.length, daysInMonth(month.days[0].date));
  assert.equal(month.days[0].date.endsWith("-01"), true);
});

function daysInMonth(dateKey: string): number {
  const [year, month] = dateKey.split("-").map((part) => Number.parseInt(part, 10));
  return new Date(year, month, 0).getDate();
}

function tokenModel(model: string, totalTokens: number) {
  return {
    model,
    input_tokens: totalTokens,
    output_tokens: 0,
    cache_read_tokens: 0,
    cache_creation_tokens: 0,
    total_tokens: totalTokens,
  };
}
