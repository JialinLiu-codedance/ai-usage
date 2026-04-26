import test from "node:test";
import assert from "node:assert/strict";
import { quotaDisplayRows, remainingQuotaProgressValue } from "../src/lib/quota-display.ts";

test("quota progress uses remaining quota percent instead of used percent", () => {
  assert.equal(
    remainingQuotaProgressValue({
      used_percent: 4,
      remaining_percent: 96,
      reset_at: null,
      window_minutes: 300,
    }),
    96,
  );
});

test("quota display rows show every available quota window", () => {
  const fiveHour = {
    used_percent: 55,
    remaining_percent: 45,
    reset_at: null,
    window_minutes: 300,
  };
  const sevenDay = {
    used_percent: 11,
    remaining_percent: 89,
    reset_at: null,
    window_minutes: 10080,
  };

  assert.deepEqual(
    quotaDisplayRows({
      five_hour: fiveHour,
      seven_day: sevenDay,
    }),
    [
      { label: "5H", window: fiveHour },
      { label: "7D", window: sevenDay },
    ],
  );
});

test("quota display rows hide only the missing quota window", () => {
  const fiveHour = {
    used_percent: 55,
    remaining_percent: 45,
    reset_at: null,
    window_minutes: 300,
  };
  const sevenDay = {
    used_percent: 11,
    remaining_percent: 89,
    reset_at: null,
    window_minutes: 10080,
  };

  assert.deepEqual(
    quotaDisplayRows({
      five_hour: fiveHour,
      seven_day: null,
    }),
    [{ label: "5H", window: fiveHour }],
  );
  assert.deepEqual(
    quotaDisplayRows({
      five_hour: null,
      seven_day: sevenDay,
    }),
    [{ label: "7D", window: sevenDay }],
  );
});
