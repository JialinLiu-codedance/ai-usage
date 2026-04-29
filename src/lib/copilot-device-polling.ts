type PollingScheduler = {
  setInterval: (callback: () => void, ms: number) => unknown;
  clearInterval: (handle: unknown) => void;
};

const DEFAULT_SCHEDULER: PollingScheduler = {
  setInterval: (callback, ms) => globalThis.setInterval(callback, ms),
  clearInterval: (handle) => globalThis.clearInterval(handle as ReturnType<typeof setInterval>),
};

export function getCopilotPollIntervalMs(intervalSeconds: number): number {
  return Math.max(intervalSeconds + 3, 8) * 1000;
}

export function startCopilotDevicePolling(
  intervalSeconds: number,
  pollOnce: () => void,
  scheduler: PollingScheduler = DEFAULT_SCHEDULER,
): () => void {
  pollOnce();
  const intervalId = scheduler.setInterval(pollOnce, getCopilotPollIntervalMs(intervalSeconds));
  return () => scheduler.clearInterval(intervalId);
}
