export function bumpSemver(version, bumpKind) {
  const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(version.trim());
  if (!match) {
    throw new Error(`不支持的语义化版本号: ${version}`);
  }

  const major = Number.parseInt(match[1], 10);
  const minor = Number.parseInt(match[2], 10);
  const patch = Number.parseInt(match[3], 10);

  if (bumpKind === "major") {
    return `${major + 1}.0.0`;
  }
  if (bumpKind === "minor") {
    return `${major}.${minor + 1}.0`;
  }
  if (bumpKind === "patch") {
    return `${major}.${minor}.${patch + 1}`;
  }

  throw new Error(`不支持的版本升级类型: ${bumpKind}`);
}

export function replaceProjectVersions(files, nextVersion) {
  return {
    packageJsonSource: replaceJsonVersion(files.packageJsonSource, nextVersion),
    packageLockSource: replacePackageLockVersion(files.packageLockSource, nextVersion),
    cargoTomlSource: replaceCargoTomlVersion(files.cargoTomlSource, nextVersion),
    cargoLockSource: replaceCargoLockVersion(files.cargoLockSource, nextVersion),
    tauriConfigSource: replaceJsonVersion(files.tauriConfigSource, nextVersion),
  };
}

function replaceJsonVersion(source, nextVersion) {
  const parsed = JSON.parse(source);
  parsed.version = nextVersion;
  return `${JSON.stringify(parsed, null, 2)}\n`;
}

function replacePackageLockVersion(source, nextVersion) {
  const parsed = JSON.parse(source);
  parsed.version = nextVersion;
  if (parsed.packages?.[""]) {
    parsed.packages[""].version = nextVersion;
  }
  return `${JSON.stringify(parsed, null, 2)}\n`;
}

function replaceCargoTomlVersion(source, nextVersion) {
  let replaced = false;
  const output = source.replace(/^version\s*=\s*"([^"]+)"$/m, () => {
    replaced = true;
    return `version = "${nextVersion}"`;
  });

  if (!replaced) {
    throw new Error("src-tauri/Cargo.toml 中未找到版本号");
  }

  return output;
}

function replaceCargoLockVersion(source, nextVersion) {
  const pattern = /(\[\[package\]\]\nname = "ai-usage"\nversion = )"([^"]+)"/;
  if (!pattern.test(source)) {
    throw new Error("src-tauri/Cargo.lock 中未找到 ai-usage 版本号");
  }
  return source.replace(pattern, `$1"${nextVersion}"`);
}
