import test from "node:test";
import assert from "node:assert/strict";
import {
  buildTokenUsageChartRows,
  formatCompactTokens,
  modelUsageRows,
} from "../src/lib/token-usage-display.ts";
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
