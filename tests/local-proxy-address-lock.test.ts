import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

test("local proxy listen address is rendered as a read-only field", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");

  assert.match(appSource, /<span>地址<\/span>[\s\S]*?<input[\s\S]*?readOnly[\s\S]*?\/>/);
  assert.doesNotMatch(appSource, /listen_address:\s*event\.target\.value/);
  assert.match(appSource, /地址当前不可修改，修改端口后需要重启代理服务才能生效/);
});
