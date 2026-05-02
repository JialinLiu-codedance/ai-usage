import test from "node:test";
import assert from "node:assert/strict";

import {
  connectedAccounts,
  isManagedCopilotAccountId,
  managedCopilotAccountIdToReverseAccountId,
} from "../src/lib/connected-accounts.ts";
import type { AppSettings, AppStatus } from "../src/lib/types.ts";

function baseSettings(): AppSettings {
  return {
    account_id: "openai-primary",
    account_name: "OpenAI Primary",
    auth_mode: "oauth",
    base_url_override: null,
    chatgpt_account_id: "openai-primary",
    accounts: [
      {
        account_id: "openai-primary",
        account_name: "OpenAI Primary",
        provider: "openai",
        auth_mode: "oauth",
        chatgpt_account_id: "openai-primary",
        secret_configured: true,
      },
    ],
    refresh_interval_minutes: 30,
    low_quota_threshold_percent: 10,
    notify_on_low_quota: false,
    notify_on_reset: false,
    reset_notify_lead_minutes: 15,
    git_usage_root: "/Users/test/project",
    git_default_branch_overrides: {},
    launch_at_login: false,
    claude_proxy: {
      listen_address: "127.0.0.1",
      listen_port: 16555,
      routes: [],
    },
    claude_proxy_profiles: {},
    reverse_proxy: {
      enabled: false,
      default_copilot_account_id: "218696320",
      default_openai_account_id: null,
    },
    secret_configured: true,
  };
}

function baseStatus(): AppStatus {
  return {
    snapshot: null,
    accounts: [
      {
        account_id: "openai-primary",
        account_name: "OpenAI Primary",
        provider: "openai",
        five_hour: null,
        seven_day: null,
        fetched_at: null,
        source: null,
        last_error: null,
      },
      {
        account_id: "managed:copilot:218696320",
        account_name: "JialinLiu-codedance",
        provider: "copilot",
        five_hour: null,
        seven_day: null,
        fetched_at: null,
        source: null,
        last_error: null,
      },
    ],
    refresh_status: "ok",
    last_error: null,
    last_refreshed_at: null,
  };
}

test("managed Copilot quota accounts are included in the connected account list", () => {
  const accounts = connectedAccounts(baseSettings(), baseStatus());

  assert.deepEqual(
    accounts.map((account) => [account.account_id, account.provider, account.account_name, account.auth_mode]),
    [
      ["openai-primary", "openai", "OpenAI Primary", "oauth"],
      ["managed:copilot:218696320", "copilot", "JialinLiu-codedance", "oauth"],
    ],
  );
});

test("managed Copilot ids are recognized and can be unwrapped", () => {
  assert.equal(isManagedCopilotAccountId("managed:copilot:218696320"), true);
  assert.equal(isManagedCopilotAccountId("218696320"), false);
  assert.equal(managedCopilotAccountIdToReverseAccountId("managed:copilot:218696320"), "218696320");
  assert.equal(managedCopilotAccountIdToReverseAccountId("218696320"), null);
});
