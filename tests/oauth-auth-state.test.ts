import test from "node:test";
import assert from "node:assert/strict";
import {
  hasGeneratedOpenAIAuthLink,
  shouldApplyOAuthStartResult,
  shouldResetOpenAIAuthDraft,
} from "../src/lib/oauth-auth-state.ts";

test("requires a new generated link after leaving the OpenAI auth page", () => {
  assert.equal(shouldResetOpenAIAuthDraft("openai-auth", "add-account"), true);
  assert.equal(shouldResetOpenAIAuthDraft("openai-auth", "settings"), true);
  assert.equal(shouldResetOpenAIAuthDraft("openai-auth", "overview"), true);
  assert.equal(shouldResetOpenAIAuthDraft("openai-auth", "openai-auth"), false);
  assert.equal(shouldResetOpenAIAuthDraft("add-account", "openai-auth"), false);
});

test("ignores generated OAuth URLs that resolve after the user has left", () => {
  assert.equal(shouldApplyOAuthStartResult("openai-auth", 2, 2), true);
  assert.equal(shouldApplyOAuthStartResult("add-account", 2, 2), false);
  assert.equal(shouldApplyOAuthStartResult("openai-auth", 3, 2), false);
});

test("requires a current generated link before completing authorization", () => {
  assert.equal(hasGeneratedOpenAIAuthLink(null), false);
  assert.equal(hasGeneratedOpenAIAuthLink(""), false);
  assert.equal(hasGeneratedOpenAIAuthLink("   "), false);
  assert.equal(hasGeneratedOpenAIAuthLink("https://auth.openai.com/oauth/authorize?state=fresh"), true);
});
