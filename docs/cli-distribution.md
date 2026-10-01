# CLI distribution & discovery (design)

Status: **Designed, cross-model reviewed (codex ×2), and implemented.** The
design review (9 findings) and the implementation review (13 findings) are both
incorporated. This closes the "CLI not shipped with the desktop app / not on
PATH" gap (audit finding 2). End-to-end acceptance runs in CI
(`tests/e2e/install-and-claim.sh`): install the CLI + plugin, then complete the
claim flow using only the canonical `<data-dir>/bin` copy (CLI not on PATH).

Implemented: D1 sidecar (`externalBin` + `scripts/stage-sidecar.mjs`, cross-target
aware), D2 canonical `<data-dir>/bin` copy (same dir as `core::data_dir()`), D3
install-time PATH (Unix `~/.local/bin` symlink with collision rules; Windows copy
+ printed hint), D4 skill Step 0 resolution, D5 startup refresh of the managed
copy. Essential steps (canonical copy, plugin staging) hard-fail; PATH symlink is
best-effort. **Deferred (documented follow-up):** the Windows opt-in
`install-claude --add-to-path` (HKCU registry update) — not yet implemented; the
canonical `<data-dir>/bin` fallback makes it non-blocking for usability, and it
cannot be runtime-verified in the current macOS/CI environment.

## Problem

The Claude Code plugin skill runs the companion CLI from a shell:

```bash
context-drop claim --json
```

If a user installs only the desktop app, `context-drop` is absent and the whole
`/context-drop:pull` flow fails. Two things must hold: the CLI must be **shipped**
with the desktop app (spec §19/§38; no Node.js for end users) and be
**discoverable** from a normal shell. Bundling it inside the `.app`/`.exe` does
**not** put it on `PATH`, so discovery needs an explicit, guaranteed fallback.

## Decisions (revised after codex review)

### D1 — Ship the CLI as a Tauri sidecar (`externalBin`)

- `tauri.conf.json`: `bundle.externalBin = ["binaries/context-drop"]`.
- Stage the binary at `src-tauri/binaries/context-drop-<target-triple>[.exe]`
  (Tauri's required sidecar naming), built for the **app's build target triple**
  (not the build host's). A `beforeBuildCommand` step builds the CLI release
  binary and copies it into `src-tauri/binaries/` with the triple suffix.
- **(codex #9)** `externalBin` is processed on *dev* builds too, so the same
  staging must run before `tauri dev`, otherwise development breaks. The staging
  script runs for both dev and build (or dev uses a checkedstaged helper).
- At runtime Tauri installs the sidecar next to the app executable **without**
  the triple suffix, i.e. `context-drop[.exe]` beside the app binary.

### D2 — One shared CLI location, matching the core data dir

- **(codex #4 — the important fix)** The canonical installed copy lives at
  `<data-dir>/bin/context-drop[.exe]`, where `<data-dir>` is exactly
  `context_drop_core::data_dir()` — i.e. `dirs::data_dir()/com.contextdrop.app`
  and honoring `CONTEXT_DROP_DATA_DIR`. This is the SAME directory the core
  resolver already returns on every OS (macOS `~/Library/Application Support/…`,
  Windows `%APPDATA%\com.contextdrop.app`, Linux `~/.local/share/…`). The design
  must NOT invent a separate location (an earlier draft's `%LOCALAPPDATA%\ContextDrop`
  contradicted the fallback and is rejected).
- A single shared helper resolves this path and is used by the installer, the
  desktop, and the skill's documented fallback.

### D3 — Install onto PATH (best-effort) when the integration is installed

The GUI "Install Claude Code integration" action and `context-drop install-claude`:

- **(codex #3)** Copy the CLI from the sibling `context-drop[.exe]` next to the
  running desktop executable (`current_exe().parent()`), not via the plugin
  resource resolver, preserving the executable bit, into `<data-dir>/bin/` (D2).
- Best-effort PATH:
  - **macOS/Linux (codex #6):** if `~/.local/bin` is missing, create it; if it
    exists but is not a directory, skip PATH setup with a diagnostic. Inspect the
    target with `symlink_metadata` (do not follow); if it is already our managed
    symlink, accept it; if it is an unrelated file/symlink, leave it and print a
    diagnostic; only ever create/replace the symlink we manage. Never copy
    through an existing destination symlink. If `~/.local/bin` is not on `PATH`,
    print the exact `export PATH=…` line (never edit shell rc files).
  - **Windows (codex #7):** default = copy only + print the one-time PATH command.
    Opt-in `--add-to-path` updates the **HKCU** `Environment` `Path`, preserving
    its registry type (`REG_EXPAND_SZ` and `%…%` tokens), de-duplicating entries,
    never writing the merged *process* PATH back, then broadcasts
    `WM_SETTINGCHANGE`. Already-running shells keep their old environment, so the
    D4 fallback still matters.
- Idempotent, additive, no sudo/UAC, never touches system-wide PATH.

### D4 — Skill resolves the CLI without relying on un-shipped docs

**(codex #1, #5)** Discovery is guaranteed by the skill itself (which ships in
the plugin), not by `docs/*` (not shipped) nor a persisted env var. Each skill
(`pull`/`undo`/`status`) resolves the binary once and uses it for **every**
command (`claim`, `consume`, `undo`, `status`):

1. `$CONTEXT_DROP_BIN` if set.
2. `context-drop` on `PATH`.
3. The deterministic canonical fallback (spelled out per-OS in the SKILL.md):
   - macOS: `"$HOME/Library/Application Support/com.contextdrop.app/bin/context-drop"`
   - Linux: `"${XDG_DATA_HOME:-$HOME/.local/share}/com.contextdrop.app/bin/context-drop"`
   - Windows: `"%APPDATA%\com.contextdrop.app\bin\context-drop.exe"`

Because D2 guarantees the canonical copy exists at exactly these paths, step 3
always works even if PATH was never updated.

### D5 — Upgrades and multiple config dirs

- **(codex #8)** On desktop upgrade, refresh `<data-dir>/bin/context-drop` from
  the current bundled sidecar (compare version; overwrite when different),
  serializing the replacement and handling a locked exe on Windows; skip if the
  source and destination are the same file. Resolution order override → managed
  copy → PATH prevents a stale PATH binary from shadowing an upgrade.
- **(codex #8)** The CLI's own `install-claude` (which may target other config
  roots) needs a stable plugin source after the CLI is relocated to
  `<data-dir>/bin`: the installer also stages the plugin tree under
  `<data-dir>/claude-code`, and `integration::resolve_plugin_source` gains that
  as a candidate (in addition to the existing exe-relative and
  `CONTEXT_DROP_PLUGIN_DIR` candidates). The desktop still passes its resource
  dir explicitly.

## Verification plan (implementation step)

- Unit-test copy / symlink / PATH-detection / Windows-registry logic against temp
  dirs; never mutate the real PATH or `~/.local/bin`.
- **(codex #9)** Add a packaged-install acceptance check that runs the skill's
  resolution with PATH deliberately absent and asserts the canonical fallback
  path is used — copy/symlink unit tests alone miss the central failure mode.
- Windows PATH update is opt-in, compiled on `windows-latest` CI, runtime-verified
  manually.
- Re-run a codex design review of the final implementation diff before merge.

## Non-goals

No system-wide install, no privileged installer, no auto-editing of shell rc
files. The canonical `<data-dir>/bin` copy makes discovery work even if PATH is
never updated.
