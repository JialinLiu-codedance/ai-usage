import test from "node:test";
import assert from "node:assert/strict";

import { copyCopilotDeviceValue } from "../src/lib/copilot-device-copy.ts";
import type { GitHubDeviceCodeResponse } from "../src/lib/types.ts";

const deviceCode: GitHubDeviceCodeResponse = {
  device_code: "device-code",
  user_code: "9CF5-F920",
  verification_uri: "https://github.com/login/device",
  expires_in: 900,
  interval: 5,
};

test("copies the Copilot device user code", async () => {
  const copied: string[] = [];

  await copyCopilotDeviceValue(
    {
      writeText(text) {
        copied.push(text);
        return Promise.resolve();
      },
    },
    deviceCode,
    "user_code",
  );

  assert.deepEqual(copied, ["9CF5-F920"]);
});

test("copies the Copilot verification url", async () => {
  const copied: string[] = [];

  await copyCopilotDeviceValue(
    {
      writeText(text) {
        copied.push(text);
        return Promise.resolve();
      },
    },
    deviceCode,
    "verification_uri",
  );

  assert.deepEqual(copied, ["https://github.com/login/device"]);
});

test("throws when clipboard support is unavailable", async () => {
  await assert.rejects(
    copyCopilotDeviceValue(null, deviceCode, "user_code"),
    /当前环境不支持复制/,
  );
});
