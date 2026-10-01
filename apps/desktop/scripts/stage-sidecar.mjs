#!/usr/bin/env node
// Stages the `context-drop` CLI as a Tauri sidecar (externalBin) so it ships
// with the desktop app. Tauri requires the file at
//   src-tauri/binaries/context-drop-<target-triple>[.exe]
// and strips the triple when installing it next to the app executable.
//
// Usage: node scripts/stage-sidecar.mjs [--release]
// Honors TAURI_ENV_TARGET_TRIPLE when set (cross-compilation); otherwise uses
// the host triple from `rustc -vV`.
import { execFileSync } from "node:child_process";
import { mkdirSync, copyFileSync, chmodSync, existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";

const scriptDir = dirname(fileURLToPath(import.meta.url)); // apps/desktop/scripts
const desktopDir = dirname(scriptDir); // apps/desktop
const repoRoot = join(desktopDir, "..", ".."); // repo root
const release = process.argv.includes("--release");

function hostTriple() {
  const out = execFileSync("rustc", ["-vV"], { encoding: "utf8" });
  const line = out.split("\n").find((l) => l.startsWith("host:"));
  if (!line) throw new Error("could not determine host triple from `rustc -vV`");
  return line.slice("host:".length).trim();
}

// Tauri sets TAURI_ENV_TARGET_TRIPLE when building; honor it for cross-compiles.
const requestedTriple = process.env.TAURI_ENV_TARGET_TRIPLE;
const triple = requestedTriple || hostTriple();
const isWindowsTarget = triple.includes("windows");
const exe = isWindowsTarget ? ".exe" : "";
const profileArgs = release ? ["--release"] : [];
const profileDir = release ? "release" : "debug";
// Build FOR the requested target (not the host) so the shipped binary matches.
const targetArgs = requestedTriple ? ["--target", requestedTriple] : [];
// Cargo resolves a relative CARGO_TARGET_DIR from the workspace root, so anchor
// it to repoRoot here rather than to this script's cwd.
const targetRoot = process.env.CARGO_TARGET_DIR
  ? resolve(repoRoot, process.env.CARGO_TARGET_DIR)
  : join(repoRoot, "target");

console.log(`[stage-sidecar] building context-drop CLI (${profileDir}) for ${triple}`);
execFileSync("cargo", ["build", "-p", "context-drop-cli", ...profileArgs, ...targetArgs], {
  cwd: repoRoot,
  stdio: "inherit",
});

const artifactDir = requestedTriple
  ? join(targetRoot, requestedTriple, profileDir)
  : join(targetRoot, profileDir);
const src = join(artifactDir, `context-drop${exe}`);
if (!existsSync(src)) throw new Error(`built CLI not found at ${src}`);

const binariesDir = join(desktopDir, "src-tauri", "binaries");
mkdirSync(binariesDir, { recursive: true });
const dest = join(binariesDir, `context-drop-${triple}${exe}`);
copyFileSync(src, dest);
if (!isWindowsTarget) chmodSync(dest, 0o755);

console.log(`[stage-sidecar] staged sidecar -> ${dest}`);
