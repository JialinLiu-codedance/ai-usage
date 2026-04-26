import test from "node:test";
import assert from "node:assert/strict";
import { remainingQuotaProgressValue } from "../src/lib/quota-display.ts";

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
