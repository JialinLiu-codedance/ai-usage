import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

test("desktop app wires the updater plugins and capabilities", async () => {
  const [cargoToml, tauriConfig, mainSource, capabilitiesSource] = await Promise.all([
    readFile(new URL("../src-tauri/Cargo.toml", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/tauri.conf.json", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/src/main.rs", import.meta.url), "utf8"),
    readFile(new URL("../src-tauri/capabilities/default.json", import.meta.url), "utf8"),
  ]);

  assert.match(cargoToml, /tauri-plugin-updater/);
  assert.match(cargoToml, /tauri-plugin-process/);
  assert.match(mainSource, /tauri_plugin_process::init\(\)/);
  assert.match(mainSource, /tauri_plugin_updater::Builder::new\(\)\.build\(\)/);
  assert.match(capabilitiesSource, /"process:default"/);
  assert.match(capabilitiesSource, /"updater:default"/);
  assert.match(capabilitiesSource, /"notification:allow-notify"/);

  const parsedConfig = JSON.parse(tauriConfig);
  assert.equal(parsedConfig.bundle.createUpdaterArtifacts, true);
  assert.deepEqual(parsedConfig.plugins.updater.endpoints, [
    "https://github.com/JialinLiu-codedance/ai-usage/releases/latest/download/latest.json",
  ]);
  assert.equal(typeof parsedConfig.plugins.updater.pubkey, "string");
  assert.notEqual(parsedConfig.plugins.updater.pubkey.trim(), "");
});

test("app automatically checks for updates and exposes an install action", async () => {
  const [appSource, tauriBridgeSource] = await Promise.all([
    readFile(new URL("../src/App.tsx", import.meta.url), "utf8"),
    readFile(new URL("../src/lib/tauri.ts", import.meta.url), "utf8"),
  ]);

  assert.match(tauriBridgeSource, /export async function checkForAppUpdate\(\): Promise<AppUpdateInfo \| null>/);
  assert.match(tauriBridgeSource, /export async function installAppUpdate\(/);
  assert.match(tauriBridgeSource, /export async function relaunchApp\(\): Promise<void>/);

  assert.match(appSource, /checkForAppUpdate/);
  assert.match(appSource, /installAppUpdate/);
  assert.match(appSource, /UPDATE_CHECK_INTERVAL_MS/);
  assert.match(appSource, /发现新版本/);
  assert.match(appSource, /立即更新/);
  assert.match(appSource, /检查更新/);
});
