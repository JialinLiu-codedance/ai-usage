import test from "node:test";
import assert from "node:assert/strict";
import {
  getCurrentQuota,
  getSettings,
  importKimiAccount,
  resetMockTauriStateForTests,
} from "../src/lib/tauri.ts";

test("mock Kimi import appends multiple named Kimi accounts", async () => {
  resetMockTauriStateForTests();

  await importKimiAccount("Kimi Work");
  await importKimiAccount("Kimi Personal");

  const settings = await getSettings();

  assert.deepEqual(
    settings.accounts.map((account) => [account.provider, account.account_name]),
    [
      ["kimi", "Kimi Work"],
      ["kimi", "Kimi Personal"],
    ],
  );
  assert.equal(settings.account_name, "Kimi Personal");

  const status = await getCurrentQuota();
  assert.deepEqual(
    status.accounts.map((account) => [account.provider, account.account_name]),
    [
      ["kimi", "Kimi Work"],
      ["kimi", "Kimi Personal"],
    ],
  );
});

test("mock Kimi import updates the selected Kimi account in place", async () => {
  resetMockTauriStateForTests();

  const first = await importKimiAccount("Kimi Work");
  const accountId = first.accounts[0].account_id;
  await importKimiAccount("Kimi Work Renamed", accountId);

  const settings = await getSettings();

  assert.equal(settings.accounts.length, 1);
  assert.equal(settings.accounts[0].account_id, accountId);
  assert.equal(settings.accounts[0].account_name, "Kimi Work Renamed");
});
