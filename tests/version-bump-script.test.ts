import test from "node:test";
import assert from "node:assert/strict";

import { bumpSemver, replaceProjectVersions } from "../scripts/release/versioning.mjs";

test("bumpSemver increments patch, minor, and major versions", () => {
  assert.equal(bumpSemver("0.1.0", "patch"), "0.1.1");
  assert.equal(bumpSemver("0.1.0", "minor"), "0.2.0");
  assert.equal(bumpSemver("0.1.0", "major"), "1.0.0");
});

test("replaceProjectVersions updates all tracked version files consistently", () => {
  const result = replaceProjectVersions(
    {
      packageJsonSource: JSON.stringify({ name: "ai-usage", version: "0.1.0" }),
      packageLockSource: JSON.stringify({
        name: "ai-usage",
        version: "0.1.0",
        packages: {
          "": {
            name: "ai-usage",
            version: "0.1.0",
          },
        },
      }),
      cargoTomlSource: '[package]\nname = "ai-usage"\nversion = "0.1.0"\n',
      cargoLockSource: '[[package]]\nname = "ai-usage"\nversion = "0.1.0"\n',
      tauriConfigSource: JSON.stringify({ productName: "AI Usage", version: "0.1.0" }),
    },
    "0.1.1",
  );

  assert.equal(JSON.parse(result.packageJsonSource).version, "0.1.1");
  assert.equal(JSON.parse(result.packageLockSource).version, "0.1.1");
  assert.equal(JSON.parse(result.packageLockSource).packages[""].version, "0.1.1");
  assert.match(result.cargoTomlSource, /^version = "0\.1\.1"$/m);
  assert.match(result.cargoLockSource, /name = "ai-usage"\nversion = "0\.1\.1"/);
  assert.equal(JSON.parse(result.tauriConfigSource).version, "0.1.1");
});
