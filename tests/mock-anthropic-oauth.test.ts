import test from "node:test";
import assert from "node:assert/strict";
import {
  completeAnthropicOAuth,
  getSettings,
  resetMockTauriStateForTests,
  startAnthropicOAuth,
} from "../src/lib/tauri.ts";

test("non-Tauri Anthropic OAuth preview uses the Claude authorization endpoint", async () => {
  const authUrl = new URL(await startAnthropicOAuth());

  assert.equal(`${authUrl.origin}${authUrl.pathname}`, "https://claude.ai/oauth/authorize");
  assert.equal(authUrl.searchParams.get("client_id"), "9d1c250a-e61b-44d9-88ed-5944d1962f5e");
  assert.equal(authUrl.searchParams.get("response_type"), "code");
  assert.equal(authUrl.searchParams.get("code_challenge_method"), "S256");
});

test("mock Anthropic OAuth completion stores an Anthropic connected account", async () => {
  resetMockTauriStateForTests();

  await completeAnthropicOAuth("https://platform.claude.com/oauth/code/callback?code=claude&email=claude@example.com");

  const settings = await getSettings();

  assert.equal(settings.account_name, "claude@example.com");
  assert.equal(settings.accounts.length, 1);
  assert.equal(settings.accounts[0].provider, "anthropic");
  assert.equal(settings.accounts[0].account_name, "claude@example.com");
});
