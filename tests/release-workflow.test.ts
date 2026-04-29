import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

test("release workflow publishes Tauri bundles from main and uploads updater metadata", async () => {
  const workflow = await readFile(new URL("../.github/workflows/release.yml", import.meta.url), "utf8");

  assert.match(workflow, /push:\s*[\s\S]*branches:\s*[\s\S]*-\s*main/);
  assert.match(workflow, /workflow_dispatch:/);
  assert.match(workflow, /tauri-apps\/tauri-action@v0\.6\.2/);
  assert.match(workflow, /uploadUpdaterJson:\s*true/);
  assert.match(workflow, /TAURI_SIGNING_PRIVATE_KEY:\s*\$\{\{\s*secrets\.TAURI_SIGNING_PRIVATE_KEY\s*\}\}/);
  assert.match(workflow, /tagName:\s*v__VERSION__/);
});

test("ci workflow verifies the desktop app before merge", async () => {
  const workflow = await readFile(new URL("../.github/workflows/ci.yml", import.meta.url), "utf8");

  assert.match(workflow, /pull_request:/);
  assert.match(workflow, /npm ci/);
  assert.match(workflow, /node --test tests\/\*\.test\.ts/);
  assert.match(workflow, /cargo test --manifest-path src-tauri\/Cargo.toml/);
  assert.match(workflow, /npm run build/);
});
