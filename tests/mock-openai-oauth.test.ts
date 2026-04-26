import test from "node:test";
import assert from "node:assert/strict";
import { startOpenAIOAuth } from "../src/lib/tauri.ts";

test("non-Tauri OAuth preview generates a fresh authorization URL each time", async () => {
  const firstUrl = await startOpenAIOAuth();
  const secondUrl = await startOpenAIOAuth();

  assert.notEqual(secondUrl, firstUrl);
});
