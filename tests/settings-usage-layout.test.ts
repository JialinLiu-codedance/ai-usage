import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

test("Git usage panel does not include the duplicate standalone title", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");

  assert.equal(appSource.includes("Git 提交代码行数统计"), false);
  assert.equal(appSource.includes("提交概览"), true);
});

test("Token usage trend title matches the split statistics design", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");

  assert.equal(appSource.includes("每日 Token 用量趋势"), false);
  assert.equal(appSource.includes("<h2>Token 用量趋势</h2>"), true);
});

test("settings account messages are scoped to the quota tab", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");
  const settingsPanelStart = appSource.indexOf("function SettingsPanel");
  const tokenUsagePanelStart = appSource.indexOf("function TokenUsagePanel");
  const settingsPanelSource = appSource.slice(settingsPanelStart, tokenUsagePanelStart);

  assert.equal(
    settingsPanelSource.includes('{message ? <div className="settings-message">{message}</div> : null}'),
    false,
  );
  assert.match(
    settingsPanelSource,
    /\{activeTab === "quota" && message \? <div className="settings-message">\{message\}<\/div> : null\}/,
  );
});
