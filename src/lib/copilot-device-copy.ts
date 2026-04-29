import type { GitHubDeviceCodeResponse } from "./types";

export type CopilotDeviceCopyTarget = "user_code" | "verification_uri";

type ClipboardWriter = {
  writeText: (text: string) => Promise<void> | void;
};

export async function copyCopilotDeviceValue(
  clipboard: ClipboardWriter | null | undefined,
  deviceCode: GitHubDeviceCodeResponse,
  target: CopilotDeviceCopyTarget,
): Promise<void> {
  if (!clipboard || typeof clipboard.writeText !== "function") {
    throw new Error("当前环境不支持复制");
  }

  const text = target === "user_code" ? deviceCode.user_code : deviceCode.verification_uri;
  await clipboard.writeText(text);
}
