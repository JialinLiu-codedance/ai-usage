import test from "node:test";
import assert from "node:assert/strict";
import {
  getCurrentQuota,
  getSettings,
  importMiniMaxAccount,
  resetMockTauriStateForTests,
} from "../src/lib/tauri.ts";

test("mock MiniMax import appends multiple named API key accounts", async () => {
  resetMockTauriStateForTests();

  await importMiniMaxAccount("MiniMax Global", "global-key");
  await importMiniMaxAccount("MiniMax CN", "cn-key");

  const settings = await getSettings();

  assert.deepEqual(
    settings.accounts.map((account) => [
      account.provider,
      account.auth_mode,
      account.account_name,
      account.secret_configured,
    ]),
    [
      ["minimax", "apiKey", "MiniMax Global", true],
      ["minimax", "apiKey", "MiniMax CN", true],
    ],
  );
  assert.equal(settings.account_name, "MiniMax CN");

  const status = await getCurrentQuota();
  assert.deepEqual(
    status.accounts.map((account) => [account.provider, account.account_name]),
    [
      ["minimax", "MiniMax Global"],
      ["minimax", "MiniMax CN"],
    ],
  );
});

test("mock MiniMax import updates the selected account in place", async () => {
  resetMockTauriStateForTests();

  const first = await importMiniMaxAccount("MiniMax Work", "first-key");
  const accountId = first.accounts[0].account_id;
  await importMiniMaxAccount("MiniMax Work Renamed", "replacement-key", accountId);

  const settings = await getSettings();

  assert.equal(settings.accounts.length, 1);
  assert.equal(settings.accounts[0].account_id, accountId);
  assert.equal(settings.accounts[0].provider, "minimax");
  assert.equal(settings.accounts[0].account_name, "MiniMax Work Renamed");
  assert.equal(settings.account_id, accountId);
});
