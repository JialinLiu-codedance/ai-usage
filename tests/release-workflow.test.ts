import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

test("release workflow publishes Tauri bundles from main and uploads updater metadata", async () => {
  const workflow = await readFile(new URL("../.github/workflows/release.yml", import.meta.url), "utf8");

  assert.match(workflow, /workflow_dispatch:/);
  assert.match(workflow, /inputs:\s*[\s\S]*bump:/);
  assert.match(workflow, /default:\s*patch/);
  assert.match(workflow, /options:\s*[\s\S]*-\s*patch[\s\S]*-\s*minor[\s\S]*-\s*major/);
  assert.match(workflow, /tauri-apps\/tauri-action@v0\.6\.2/);
  assert.match(workflow, /uploadUpdaterJson:\s*true/);
  assert.match(workflow, /node scripts\/release\/bump-version\.mjs "\$\{BUMP_KIND\}"/);
  assert.match(workflow, /Ensure release version is new[\s\S]*APP_VERSION: \$\{\{\s*steps\.bump_version\.outputs\.version\s*\}\}/);
  assert.match(workflow, /persist-credentials:\s*false/);
  assert.match(workflow, /RELEASE_PUSH_TOKEN: \$\{\{\s*secrets\.RELEASE_PUSH_TOKEN\s*\}\}/);
  assert.match(workflow, /git commit -m "core:bump version to \$\{VERSION\}"/);
  assert.match(workflow, /git remote set-url origin "https:\/\/x-access-token:\$\{RELEASE_PUSH_TOKEN\}@github\.com\/\$\{GITHUB_REPOSITORY\}\.git"/);
  assert.match(workflow, /git push origin HEAD:main/);
  assert.match(workflow, /release_sha=\$\(git rev-parse HEAD\)/);
  assert.match(workflow, /ref:\s*\$\{\{\s*needs\.preflight\.outputs\.release_sha\s*\}\}/);
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
