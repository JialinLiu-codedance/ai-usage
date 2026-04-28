import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

test("local proxy settings no longer expose the test-match feature", async () => {
  const [appSource, tauriSource, typesSource, styleSource, commandsSource, mainSource] = await Promise.all([
    readFile(new URL("../src/App.tsx", import.meta.url), "utf8"),
    readFile(new URL("../src/lib/tauri.ts", import.meta.url), "utf8"),
    readFile(new URL("../src/lib/types.ts", import.meta.url), "utf8"),
    readFile(new URL("../src/styles.css", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/commands.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/main.rs", import.meta.url), "utf8"),
  ]);

  assert.equal(appSource.includes("<h2>测试匹配</h2>"), false);
  assert.equal(appSource.includes("proxy-test-card"), false);
  assert.equal(appSource.includes("handleProxyTestMatch"), false);

  assert.equal(tauriSource.includes("testLocalProxyMatch"), false);
  assert.equal(typesSource.includes("interface LocalProxyMatchResult"), false);
  assert.equal(styleSource.includes(".proxy-test-result"), false);
  assert.equal(commandsSource.includes("pub fn test_local_proxy_match"), false);
  assert.equal(mainSource.includes("commands::test_local_proxy_match"), false);
});

test("local proxy direct-connect providers use the connected label", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");

  assert.equal(appSource.includes('label: "可直接接入"'), false);
  assert.equal(appSource.includes('label: "已接入"'), true);
});
