import test from "node:test";
import assert from "node:assert/strict";
import {
  hasGeneratedOAuthAuthLink,
  shouldApplyOAuthStartResult,
  shouldResetOAuthAuthDraft,
} from "../src/lib/oauth-auth-state.ts";

test("requires a new generated link after leaving the OAuth auth page", () => {
  assert.equal(shouldResetOAuthAuthDraft("oauth-auth", "add-account"), true);
  assert.equal(shouldResetOAuthAuthDraft("oauth-auth", "settings"), true);
  assert.equal(shouldResetOAuthAuthDraft("oauth-auth", "overview"), true);
  assert.equal(shouldResetOAuthAuthDraft("oauth-auth", "oauth-auth"), false);
  assert.equal(shouldResetOAuthAuthDraft("add-account", "oauth-auth"), false);
});

test("ignores generated OAuth URLs that resolve after the user has left", () => {
  assert.equal(shouldApplyOAuthStartResult("oauth-auth", 2, 2), true);
  assert.equal(shouldApplyOAuthStartResult("add-account", 2, 2), false);
  assert.equal(shouldApplyOAuthStartResult("oauth-auth", 3, 2), false);
});

test("requires a current generated link before completing authorization", () => {
  assert.equal(hasGeneratedOAuthAuthLink(null), false);
  assert.equal(hasGeneratedOAuthAuthLink(""), false);
  assert.equal(hasGeneratedOAuthAuthLink("   "), false);
  assert.equal(hasGeneratedOAuthAuthLink("https://auth.openai.com/oauth/authorize?state=fresh"), true);
});
