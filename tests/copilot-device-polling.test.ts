import test from "node:test";
import assert from "node:assert/strict";

import {
  getCopilotPollIntervalMs,
  startCopilotDevicePolling,
} from "../src/lib/copilot-device-polling.ts";

test("uses a buffered interval instead of GitHub's raw poll interval", () => {
  assert.equal(getCopilotPollIntervalMs(0), 8000);
  assert.equal(getCopilotPollIntervalMs(5), 8000);
  assert.equal(getCopilotPollIntervalMs(10), 13000);
});

test("polls once immediately before scheduling the next interval", () => {
  const calls: string[] = [];
  const scheduled: Array<{ fn: () => void; ms: number }> = [];
  const cleared: unknown[] = [];

  const stop = startCopilotDevicePolling(5, () => {
    calls.push("poll");
  }, {
    setInterval(fn, ms) {
      scheduled.push({ fn, ms });
      return "interval-id";
    },
    clearInterval(intervalId) {
      cleared.push(intervalId);
    },
  });

  assert.deepEqual(calls, ["poll"]);
  assert.equal(scheduled.length, 1);
  assert.equal(scheduled[0]?.ms, 8000);

  scheduled[0]?.fn();
  assert.deepEqual(calls, ["poll", "poll"]);

  stop();
  assert.deepEqual(cleared, ["interval-id"]);
});
