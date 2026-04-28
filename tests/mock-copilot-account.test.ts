import test from "node:test";
import assert from "node:assert/strict";
import {
  getCurrentQuota,
  getSettings,
  importCopilotAccount,
  resetMockTauriStateForTests,
} from "../src/lib/tauri.ts";

test("mock Copilot import appends multiple named GitHub token accounts", async () => {
  resetMockTauriStateForTests();

  await importCopilotAccount("Copilot Work", "gho_work");
  await importCopilotAccount("Copilot Personal", "gho_personal");

  const settings = await getSettings();

  assert.deepEqual(
    settings.accounts.map((account) => [
      account.provider,
      account.auth_mode,
      account.account_name,
      account.secret_configured,
    ]),
    [
      ["copilot", "apiKey", "Copilot Work", true],
      ["copilot", "apiKey", "Copilot Personal", true],
    ],
  );
  assert.equal(settings.account_name, "Copilot Personal");

  const status = await getCurrentQuota();
  assert.deepEqual(
    status.accounts.map((account) => [account.provider, account.account_name]),
    [
      ["copilot", "Copilot Work"],
      ["copilot", "Copilot Personal"],
    ],
  );
});

test("mock Copilot import updates the selected account in place", async () => {
  resetMockTauriStateForTests();

  const first = await importCopilotAccount("Copilot Work", "gho_first");
  const accountId = first.accounts[0].account_id;
  await importCopilotAccount("Copilot Work Renamed", "gho_replacement", accountId);

  const settings = await getSettings();

  assert.equal(settings.accounts.length, 1);
  assert.equal(settings.accounts[0].account_id, accountId);
  assert.equal(settings.accounts[0].provider, "copilot");
  assert.equal(settings.accounts[0].account_name, "Copilot Work Renamed");
  assert.equal(settings.account_id, accountId);
});
