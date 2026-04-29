import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

const [packageJsonSource, cargoTomlSource, tauriConfigSource] = await Promise.all([
  readFile(new URL("../../package.json", import.meta.url), "utf8"),
  readFile(new URL("../../src-tauri/Cargo.toml", import.meta.url), "utf8"),
  readFile(new URL("../../src-tauri/tauri.conf.json", import.meta.url), "utf8"),
]);

const packageVersion = JSON.parse(packageJsonSource).version;
const cargoVersion = cargoTomlSource.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
const tauriVersion = JSON.parse(tauriConfigSource).version;

assert.equal(typeof packageVersion, "string", "package.json 缺少 version");
assert.equal(typeof cargoVersion, "string", "src-tauri/Cargo.toml 缺少 version");
assert.equal(typeof tauriVersion, "string", "src-tauri/tauri.conf.json 缺少 version");
assert.equal(packageVersion, cargoVersion, "package.json 与 src-tauri/Cargo.toml 版本不一致");
assert.equal(packageVersion, tauriVersion, "package.json 与 src-tauri/tauri.conf.json 版本不一致");

process.stdout.write(`Version OK: ${packageVersion}\n`);
