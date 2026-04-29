import { readFile } from "node:fs/promises";

const tauriConfig = JSON.parse(await readFile(new URL("../../src-tauri/tauri.conf.json", import.meta.url), "utf8"));

if (typeof tauriConfig.version !== "string" || !tauriConfig.version.trim()) {
  throw new Error("src-tauri/tauri.conf.json 缺少合法 version");
}

process.stdout.write(tauriConfig.version.trim());
