const OAUTH_AUTH_VIEW = "oauth-auth";

export function shouldResetOAuthAuthDraft(previousView: string, nextView: string): boolean {
  return previousView === OAUTH_AUTH_VIEW && nextView !== OAUTH_AUTH_VIEW;
}

export function shouldApplyOAuthStartResult(currentView: string, currentRequestId: number, requestId: number): boolean {
  return currentView === OAUTH_AUTH_VIEW && currentRequestId === requestId;
}

export function hasGeneratedOAuthAuthLink(authUrl: string | null): boolean {
  return Boolean(authUrl?.trim());
}

export const shouldResetOpenAIAuthDraft = shouldResetOAuthAuthDraft;
export const hasGeneratedOpenAIAuthLink = hasGeneratedOAuthAuthLink;
