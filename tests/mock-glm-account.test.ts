import test from "node:test";
import assert from "node:assert/strict";
import {
  getCurrentQuota,
  getSettings,
  importGlmAccount,
  resetMockTauriStateForTests,
} from "../src/lib/tauri.ts";

test("mock GLM import appends multiple named API key accounts", async () => {
  resetMockTauriStateForTests();

  await importGlmAccount("GLM Work", "work-key");
  await importGlmAccount("GLM Personal", "personal-key");

  const settings = await getSettings();

  assert.deepEqual(
    settings.accounts.map((account) => [
      account.provider,
      account.auth_mode,
      account.account_name,
      account.secret_configured,
    ]),
    [
      ["glm", "apiKey", "GLM Work", true],
      ["glm", "apiKey", "GLM Personal", true],
    ],
  );
  assert.equal(settings.account_name, "GLM Personal");

  const status = await getCurrentQuota();
  assert.deepEqual(
    status.accounts.map((account) => [account.provider, account.account_name]),
    [
      ["glm", "GLM Work"],
      ["glm", "GLM Personal"],
    ],
  );
});

test("mock GLM import updates the selected account in place", async () => {
  resetMockTauriStateForTests();

  const first = await importGlmAccount("GLM Work", "first-key");
  const accountId = first.accounts[0].account_id;
  await importGlmAccount("GLM Work Renamed", "replacement-key", accountId);

  const settings = await getSettings();

  assert.equal(settings.accounts.length, 1);
  assert.equal(settings.accounts[0].account_id, accountId);
  assert.equal(settings.accounts[0].provider, "glm");
  assert.equal(settings.accounts[0].account_name, "GLM Work Renamed");
  assert.equal(settings.account_id, accountId);
});
