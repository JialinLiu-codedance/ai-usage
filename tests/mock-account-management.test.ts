import test from "node:test";
import assert from "node:assert/strict";
import {
  completeOpenAIOAuth,
  deleteOpenAIAccount,
  getCurrentQuota,
  getSettings,
  resetMockTauriStateForTests,
} from "../src/lib/tauri.ts";

test("mock OAuth completion appends OpenAI accounts instead of replacing the first one", async () => {
  resetMockTauriStateForTests();

  await completeOpenAIOAuth("https://localhost/callback?code=first&email=first@example.com");
  await completeOpenAIOAuth("https://localhost/callback?code=second&email=second@example.com");

  const settings = await getSettings();

  assert.deepEqual(
    settings.accounts.map((account) => account.account_name),
    ["first@example.com", "second@example.com"],
  );
  assert.equal(settings.account_name, "second@example.com");
});

test("mock current quota exposes every connected account for the overview", async () => {
  resetMockTauriStateForTests();

  await completeOpenAIOAuth("https://localhost/callback?code=first&email=first@example.com");
  await completeOpenAIOAuth("https://localhost/callback?code=second&email=second@example.com");

  const status = await getCurrentQuota();

  assert.deepEqual(
    status.accounts.map((account) => account.account_name),
    ["first@example.com", "second@example.com"],
  );
});

test("mock delete removes only the selected OpenAI account", async () => {
  resetMockTauriStateForTests();

  await completeOpenAIOAuth("https://localhost/callback?code=first&email=first@example.com");
  await completeOpenAIOAuth("https://localhost/callback?code=second&email=second@example.com");

  let settings = await getSettings();
  const firstAccountId = settings.accounts[0].account_id;
  const secondAccountId = settings.accounts[1].account_id;

  settings = await deleteOpenAIAccount(secondAccountId);

  assert.deepEqual(
    settings.accounts.map((account) => account.account_id),
    [firstAccountId],
  );
  assert.equal(settings.account_id, firstAccountId);
});
