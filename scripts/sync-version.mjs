#!/usr/bin/env node
// Sync the semantic-release-computed version into every place the build reads
// it: package.json, tauri.conf.json, the Cargo workspace version, and the
// workspace members' entries in Cargo.lock (so the committed lock stays
// accurate without needing a Rust toolchain on the version job).
import { readFileSync, writeFileSync } from "node:fs";

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+(-[\w.]+)?$/.test(version ?? "")) {
  console.error(`usage: sync-version.mjs <semver> (got: ${version})`);
  process.exit(1);
}

function edit(path, fn) {
  const before = readFileSync(path, "utf8");
  const after = fn(before);
  if (after === before) {
    console.error(`sync-version: no change applied to ${path}`);
    process.exit(1);
  }
  writeFileSync(path, after);
  console.log(`sync-version: ${path} -> ${version}`);
}

edit("package.json", (s) => {
  const pkg = JSON.parse(s);
  pkg.version = version;
  return JSON.stringify(pkg, null, 2) + "\n";
});

edit("src-tauri/tauri.conf.json", (s) => {
  const conf = JSON.parse(s);
  conf.version = version;
  return JSON.stringify(conf, null, 2) + "\n";
});

edit("src-tauri/Cargo.toml", (s) =>
  s.replace(
    /(\[workspace\.package\][\s\S]*?version\s*=\s*")[^"]+(")/,
    `$1${version}$2`,
  ),
);

// Cargo.lock: only the workspace members inherit workspace.package.version.
edit("src-tauri/Cargo.lock", (s) => {
  let out = s;
  for (const name of ["speakly", "speakly-engine", "speakly-engine-types"]) {
    out = out.replace(
      new RegExp(`(name = "${name}"\\nversion = ")[^"]+(")`),
      `$1${version}$2`,
    );
  }
  return out;
});
