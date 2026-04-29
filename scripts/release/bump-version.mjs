import { readFile, writeFile } from "node:fs/promises";

import { bumpSemver, replaceProjectVersions } from "./versioning.mjs";

const bumpKind = process.argv[2] ?? "patch";
const files = {
  packageJsonPath: new URL("../../package.json", import.meta.url),
  packageLockPath: new URL("../../package-lock.json", import.meta.url),
  cargoTomlPath: new URL("../../src-tauri/Cargo.toml", import.meta.url),
  cargoLockPath: new URL("../../src-tauri/Cargo.lock", import.meta.url),
  tauriConfigPath: new URL("../../src-tauri/tauri.conf.json", import.meta.url),
};

const [packageJsonSource, packageLockSource, cargoTomlSource, cargoLockSource, tauriConfigSource] = await Promise.all([
  readFile(files.packageJsonPath, "utf8"),
  readFile(files.packageLockPath, "utf8"),
  readFile(files.cargoTomlPath, "utf8"),
  readFile(files.cargoLockPath, "utf8"),
  readFile(files.tauriConfigPath, "utf8"),
]);

const currentVersion = JSON.parse(packageJsonSource).version;
if (typeof currentVersion !== "string" || !currentVersion.trim()) {
  throw new Error("package.json 缺少合法 version");
}

const nextVersion = bumpSemver(currentVersion, bumpKind);
const updatedFiles = replaceProjectVersions(
  {
    packageJsonSource,
    packageLockSource,
    cargoTomlSource,
    cargoLockSource,
    tauriConfigSource,
  },
  nextVersion,
);

await Promise.all([
  writeFile(files.packageJsonPath, updatedFiles.packageJsonSource),
  writeFile(files.packageLockPath, updatedFiles.packageLockSource),
  writeFile(files.cargoTomlPath, updatedFiles.cargoTomlSource),
  writeFile(files.cargoLockPath, updatedFiles.cargoLockSource),
  writeFile(files.tauriConfigPath, updatedFiles.tauriConfigSource),
]);

process.stdout.write(`${nextVersion}\n`);
