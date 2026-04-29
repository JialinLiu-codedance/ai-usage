import test from "node:test";
import assert from "node:assert/strict";
import {
  getSettings,
  resetMockTauriStateForTests,
  saveSettings,
} from "../src/lib/tauri.ts";

test("mock saveSettings persists the launch-at-login preference", async () => {
  resetMockTauriStateForTests();

  const initial = await getSettings();
  assert.equal(initial.launch_at_login, false);

  const updated = await saveSettings({
    ...initial,
    launch_at_login: true,
  });

  assert.equal(updated.launch_at_login, true);
  assert.equal((await getSettings()).launch_at_login, true);
});
