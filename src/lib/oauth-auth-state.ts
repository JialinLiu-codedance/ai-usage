const OPENAI_AUTH_VIEW = "openai-auth";

export function shouldResetOpenAIAuthDraft(previousView: string, nextView: string): boolean {
  return previousView === OPENAI_AUTH_VIEW && nextView !== OPENAI_AUTH_VIEW;
}

export function shouldApplyOAuthStartResult(currentView: string, currentRequestId: number, requestId: number): boolean {
  return currentView === OPENAI_AUTH_VIEW && currentRequestId === requestId;
}

export function hasGeneratedOpenAIAuthLink(authUrl: string | null): boolean {
  return Boolean(authUrl?.trim());
}
