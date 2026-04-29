import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

test("main panel refreshes quota state when the tray window is shown", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");

  assert.match(
    appSource,
    /listen\("show-main-panel",\s*\(\)\s*=>\s*\{[\s\S]*void syncQuotaStatus\(\);[\s\S]*navigateToView\("overview"\);[\s\S]*\}\)/,
  );
});

test("overview polls quota state again while a refresh is still in progress", async () => {
  const appSource = await readFile(new URL("../src/App.tsx", import.meta.url), "utf8");

  assert.match(
    appSource,
    /useEffect\(\(\)\s*=>\s*\{[\s\S]*status\.refresh_status !== "refreshing"[\s\S]*setTimeout\(\(\)\s*=>\s*\{[\s\S]*void syncQuotaStatus\(\);[\s\S]*\},\s*1000\)/,
  );
});
