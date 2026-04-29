import type { AppSettings, AppStatus, ConnectedAccount } from "./types";

const MANAGED_COPILOT_ACCOUNT_PREFIX = "managed:copilot:";

function accountLabel(settings: AppSettings, status: AppStatus): string {
  return status.snapshot?.account_name || settings.account_name || "OpenAI Account";
}

function accountSubtitle(settings: AppSettings, status: AppStatus): string {
  const label = accountLabel(settings, status);
  return label.trim() || "OpenAI Account";
}

function managedCopilotStatusToConnectedAccount(accountId: string, accountName: string): ConnectedAccount {
  return {
    account_id: accountId,
    account_name: accountName,
    provider: "copilot",
    auth_mode: "oauth",
    chatgpt_account_id: null,
    secret_configured: true,
  };
}

function defaultProviderAccountName(provider: string): string {
  if (provider === "anthropic") {
    return "Anthropic Account";
  }
  if (provider === "kimi") {
    return "Kimi Account";
  }
  if (provider === "glm") {
    return "GLM Account";
  }
  if (provider === "minimax") {
    return "MiniMax Account";
  }
  if (provider === "copilot") {
    return "Copilot Account";
  }
  return "OpenAI Account";
}

export function isManagedCopilotAccountId(accountId: string): boolean {
  return accountId.startsWith(MANAGED_COPILOT_ACCOUNT_PREFIX);
}

export function managedCopilotAccountIdToReverseAccountId(accountId: string): string | null {
  if (!isManagedCopilotAccountId(accountId)) {
    return null;
  }
  const reverseAccountId = accountId.slice(MANAGED_COPILOT_ACCOUNT_PREFIX.length).trim();
  return reverseAccountId || null;
}

export function hasConnectedAccount(settings: AppSettings, status: AppStatus): boolean {
  return (
    settings.accounts.some((account) => account.secret_configured) ||
    settings.secret_configured ||
    Boolean(status.snapshot) ||
    status.accounts.length > 0
  );
}

export function connectedAccounts(settings: AppSettings, status: AppStatus): ConnectedAccount[] {
  const configuredAccounts = settings.accounts.filter((account) => account.secret_configured);
  const accounts =
    configuredAccounts.length > 0
      ? [...configuredAccounts]
      : hasConnectedAccount(settings, status)
        ? [
            {
              account_id: settings.account_id,
              account_name: accountSubtitle(settings, status),
              provider: "openai",
              auth_mode: settings.auth_mode,
              chatgpt_account_id: settings.chatgpt_account_id,
              secret_configured: settings.secret_configured,
            },
          ]
        : [];

  const existingIds = new Set(accounts.map((account) => account.account_id));
  for (const account of status.accounts) {
    if (account.provider !== "copilot" || !isManagedCopilotAccountId(account.account_id)) {
      continue;
    }
    if (existingIds.has(account.account_id)) {
      continue;
    }
    accounts.push(managedCopilotStatusToConnectedAccount(account.account_id, account.account_name));
    existingIds.add(account.account_id);
  }

  return accounts;
}

export function connectedAccountSubtitle(account: ConnectedAccount): string {
  return account.account_name.trim() || defaultProviderAccountName(account.provider);
}
