# Changelog

All notable changes to Context Drop are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **Menu-bar / tray: icon only.** The `○ Context Drop` / `● Context Drop · N` title
  text is gone (it wasted menu-bar space and duplicated the icon). A new drop icon
  shows the state: outlined while idle (macOS template image; blue on
  Windows/Linux so it stays visible on dark taskbars), filled green while
  capturing. The live item count moved to the tooltip.

### Fixed

- `/cd <instruction>` sometimes skipped the packet entirely: the model treated the
  instruction as a standalone task, never claimed the packet or looked at the
  screenshots, and the packet stayed in the desktop app's Current Packet until a
  bare `/cd` was run. `/cd` now claims deterministically when it expands (bundled
  `claim.sh` via the skill loader's `!` injection, `allowed-tools` pinned to that
  script's absolute path), and states that the instruction is always about the packet.
- **Windows:** the claim no longer needs `sh`/Git Bash. The skills ship `claim.ps1`
  next to `claim.sh`; on Windows the installer (desktop app or `context-drop
  install-claude`) writes `/cd`'s claim line as `powershell -NoProfile
  -ExecutionPolicy Bypass -File "<skill dir>/claim.ps1"` (valid from Git Bash,
  PowerShell, or cmd) and grants exactly that command. `claim.ps1` decodes the CLI
  output as UTF-8 (non-ASCII paths on CP932 consoles). Both scripts run in CI.
- `/context-drop:pull` (installable straight from GitHub, so never OS-adapted) does
  not use loader injection — a POSIX-only injected command would stop the skill
  loading on Windows without Git Bash. It makes running `claim.sh`/`claim.ps1` the
  model's first tool call instead, with the same "instruction is about the packet"
  rule.
- `/cd` is now self-contained instead of a pointer to the pull skill, and installing
  the short alias upgrades an older Context Drop `/cd` in place (a user's own `/cd`
  is still never overwritten without `--force`).

## [0.1.3] - 2026-10-02

### Fixed

- The in-app **Claude Code Setup** and the README no longer claim that `/cd`
  ships with the plugin. The plugin provides `/context-drop:pull` (plus `:status`
  and `:undo`); `/cd` is an optional short alias installed separately
  (Settings → Claude Code Setup → Optional / offline install → Install /cd short
  alias, or `context-drop install-claude --short-alias`). The README also notes
  that already-open sessions need `/reload-plugins` (or a new session).

## [0.1.2] - 2026-10-01

### Fixed

- **Last Dispatch** no longer ticks up every second — times under a minute now
  show "just now", and the label only changes at minute/hour/day granularity.

## [0.1.1] - 2026-10-01

### Added

- **Step-by-step Claude Code Setup.** A dedicated setup section (separate from
  Settings) walks you through enabling the plugin with copyable
  `/plugin marketplace add EarthLinkNetwork/context-drop` and
  `/plugin install context-drop@context-drop` commands, plus an optional/offline
  path. A "finish setup" banner appears until the plugin is installed.
- **Install-state detection.** The app reads Claude Code's own installed-plugins
  record and shows a **✓ Plugin installed in Claude Code** badge once it's set up.

### Changed

- **Last Dispatch** moved back under Current Packet on the Capture tab, so a
  consume/undo is visible next to the packet it affected.
- This repository is now a public Claude Code marketplace
  (`/plugin marketplace add EarthLinkNetwork/context-drop`).

### Added


- **Drag & drop capture.** Files can be dropped directly onto the Context Drop
  window to add them to the current packet — no need to copy them to the
  clipboard first, and it works even when Capture is OFF (a drop is an explicit
  one-shot capture). The window highlights while a drag hovers it.
- The captured-item list now updates instantly (via a backend event) instead of
  only on the periodic refresh, so items are visibly seen to accumulate as you
  copy or drop.
- **Item previews.** Each captured item now shows a preview so you can tell what
  it is: the first ~100 characters for text/JSON/HTML/URL items, and a small
  thumbnail for images (including dropped image files: png/jpeg/gif/webp/bmp/tiff).
- **Full-content viewer.** Click any item to open a modal showing its full
  content — the whole text (scrollable) or a large image — so you can confirm
  exactly what was captured.
- **Per-item delete.** Each item has a trash button to remove just that item from
  the current packet (only while the packet is still a DRAFT, i.e. not yet sent).
- **Two-tab layout.** The window is split into a **Capture** tab (capture control
  + the current packet's items) and a **Settings** tab (settings, integrations,
  and Last Dispatch), so the main working view is uncluttered. A shortcut-conflict
  warning stays visible above both tabs.

### Changed

- **Start Capture no longer grabs the existing clipboard.** Capture now records
  the clipboard's current state without capturing it, so only what you copy
  *after* pressing Start is collected — a stale item already on the clipboard is
  no longer swept into the packet. (Drag & drop is unaffected; a drop is always an
  explicit capture.)
- **Stop Capture now PAUSES instead of clearing.** Stopping capture keeps the
  collected items as a DRAFT (still visible and still claimable); a later Start
  Capture resumes the same packet. Only the **Clear** button (or sending) empties
  it. Previously, Stop moved the packet to a state the UI didn't show, so it
  looked like everything was discarded.
- **Responsive layout.** The window content now fills (and tracks) the window
  size instead of staying a fixed 360px column, so enlarging the window to view a
  big image no longer leaves the base UI narrow/broken.
- **Last Dispatch** moved out of the main capture view into the Settings tab.
- The Claude Code skill/command was renamed from `/context-drop:send` to
  `/context-drop:pull` (the CLI verb is `claim`; "pull" matches the session-pulls
  semantics). The `/cd` short alias is unchanged.

## [0.1.0] - 2026-09-29

Initial release of Context Drop, a local-only desktop utility that collects raw
context (screenshots, UI images, browser console output, server/debug logs, JSON
responses, network results, stack traces, external docs, and multiple copied text
fragments, images, or Finder/Explorer files) into a temporary "Context Packet" and
routes it to a chosen Claude Code session, where an isolated subagent processes it
and returns a compact result.

The whole point of the tool is this flow:

```
raw context -> Context Drop Packet -> isolated Claude Code subagent -> compact result -> main Claude
```

The raw material is never dumped into the main Claude conversation. The forbidden
flow `raw context -> main Claude -> subagent` is deliberately not supported.

macOS and Windows are officially supported. Linux is experimental: the architecture
is present, but it is not a supported target.

### CLI distribution & hardening (post-audit, cross-reviewed with codex)

- The desktop app now ships the `context-drop` CLI as a Tauri sidecar
  (`externalBin`) and installs it to the canonical `<data-dir>/bin` on
  integration install, refreshing it on startup; the Claude Code skills resolve
  the CLI via `$CONTEXT_DROP_BIN` → PATH → the canonical copy, so the flow works
  without any PATH edits. See `docs/cli-distribution.md`.
- Fixed: Windows clipboard sequence number used a non-existent function
  (`raw::seq_num` now); consume is scoped to a specific claim id + session so an
  undo/re-claim of the same packet can never be consumed by a stale finish;
  `clear_packet` and TTL cleanup now delete atomically under a state guard so a
  concurrently-claimed packet is never removed; cleanup self-heals orphaned
  packet directories; clipboard file dedupe is content-based; the desktop capture
  loop stops on an external claim without needing a new clipboard event; the
  desktop Clear button command name and the size-limit rejection notice are wired
  to the UI.

### Added

#### Core packet model and SQLite authority (`crates/context-drop-core`)
- Packet data model with the state machine: `DRAFT`, `READY`, `CLAIMED`,
  `PROCESSING`, `CONSUMED`, `FAILED`, `EXPIRED`.
  - `DRAFT` = capturing; `READY` = capture stopped, not yet handed to a session;
    `CLAIMED` = bound to a specific Claude session; `PROCESSING` = subagent started;
    `CONSUMED` = done.
  - Main transitions: `DRAFT->READY`, `DRAFT->CLAIMED`, `READY->CLAIMED`,
    `CLAIMED->PROCESSING`, `PROCESSING->CONSUMED`.
  - Recovery transitions: `CLAIMED`/`PROCESSING`->`READY` (release/undo),
    `CLAIMED`/`PROCESSING`->`FAILED`, and `*`->`EXPIRED` (TTL).
- SQLite as the single metadata authority (WAL mode, `busy_timeout` 5000ms,
  `foreign_keys` ON, `synchronous` NORMAL). Tables: `packets`, `packet_items`,
  `claims`, `settings`, `migrations`. Schema versioned via `PRAGMA user_version`
  (current v1).
- Transactional operations using `BEGIN IMMEDIATE` for finalize, claim,
  append-state-check, undo, and release. Append verifies `state == DRAFT` inside the
  transaction, so it can never win a race against a claim.
- Manifest projection at `packets/<id>/manifest.json` (`schemaVersion = 1`) with
  fields `schemaVersion`, `id`, `state`, `createdAt`, `updatedAt` (RFC3339),
  `items[]`, and `claim` (present only once `CLAIMED`).
  - Each item carries `id`, `kind`, `mimeType`, `relativePath`, `byteSize`,
    `sha256`, `createdAt`.
  - `claim` carries `sessionId`, `cwd`, `projectRoot`, `projectName`, `configDir`
    (optional, diagnostics only), and `claimedAt`.
  - Raw file contents are never inlined; the manifest holds references and hashes
    only. SQLite is the authority and the manifest is a projection of it.
- Storage layout, cleanup, and project detection live here, alongside all critical
  Rust tests.

#### Session-centric atomic claim routing
- "Capture first, route later." A packet's destination is `NONE` at capture time.
  The destination is decided only when `/context-drop:pull` runs from the target
  Claude Code session.
- Routing identity is `session_id + cwd + project_root`, using
  `CLAUDE_CODE_SESSION_ID`. `project_root` is `git rev-parse --show-toplevel` when in
  a git repo, otherwise the canonicalized `cwd` (a monorepo subdirectory is not a
  different project unless the git root differs).
- The Anthropic account id is never used for routing. Routing is never by
  most-recently-opened project, foreground app, window title, account, or the desktop
  UI selection. The same repo open in multiple tabs is distinguished by `session_id`.
  Supports 20+ parallel sessions and 10+ Claude accounts.
- Claim priority: (1) current `DRAFT` with items > 0, (2) newest `READY`,
  (3) `NO_PACKET`. Claiming a `DRAFT` atomically ends capture (`DRAFT->CLAIMED`); any
  later append is refused.
- Claims are atomic via SQLite `BEGIN IMMEDIATE` plus a conditional `UPDATE`, so
  exactly one session can ever claim a given packet, even under concurrent claims.

#### Clipboard capture (`crates/context-drop-clipboard`)
- Platform clipboard abstraction via the `ClipboardProvider` trait, with
  `macos.rs`, `windows.rs`, and `linux.rs` implementations.
- Text and image capture via `arboard`; native APIs for file lists and change
  detection (`objc2` `NSPasteboard` on macOS, `clipboard-win` `CF_HDROP` on Windows).
  Images are normalized to PNG.
- Capture is OFF by default and only monitors the clipboard while Capture is ON. It
  is never an always-on daemon.
- Size limits (configurable): `maxItemBytes` default 25 MiB, `maxPacketBytes`
  default 200 MiB. On exceed, Context Drop does not crash and does not silently
  truncate; the item is rejected with a reason and the existing packet is preserved.

#### Storage
- Global, per OS user. App identifier and data-dir leaf `com.contextdrop.app`,
  resolved as `dirs::data_dir()/com.contextdrop.app` (matching Tauri v2
  `app_data_dir`). Overridable via the `CONTEXT_DROP_DATA_DIR` environment variable.
- Layout: `<data-dir>/context-drop.db`, `config.json`,
  `packets/<packet-id>/manifest.json` + `items/0001.png`, `0002.txt`, ...,
  `logs/`, and `bin/`.
- Packet ids are UUIDv7 (collision-resistant and time-ordered; never
  timestamp-only).
- On Unix, directories are `0700` and files are `0600`; on Windows, per-user
  `%APPDATA%` ACLs are relied upon.

#### TTL cleanup
- Default TTL 24h (configurable). Triggered only on app startup and on new-capture
  start; there is no scheduler, cron, or background service.
- `DRAFT` and `PROCESSING` packets are never age-deleted. Stale
  `READY`/`CLAIMED`/`FAILED` packets are marked `EXPIRED` and then removed along with
  terminal packets past TTL.

#### `context-drop` companion CLI (`crates/context-drop-cli`)
- Commands: `status [--json]`, `claim [--json] [--session-id S] [--cwd P]`,
  `consume <packet-id>`, `release <packet-id>`, `undo [--json]`, `list [--json]`,
  `doctor`, and
  `install-claude [--config-dir <path>] [--from <path>] [--short-alias] [--force]`.
  Global flag: `--data-dir <path>`.
- Exit codes: `0` ok, `1` error, `3` `NO_PACKET`, `4` `MISSING_SESSION_ID`,
  `5` `NOTHING_TO_UNDO`.
- `claim --json` returns metadata only (never raw content, logs, JSON contents,
  image bytes, or file contents), for example:

  ```json
  { "ok": true, "packetId": "...", "manifestPath": "...", "itemCount": 7, "sessionId": "...", "cwd": "...", "projectRoot": "...", "projectName": "prompt-flow" }
  ```

- On `NO_PACKET`, the message is exactly:
  `No Context Drop packet is ready. Start Capture and copy the materials first.`

#### Claude Code plugin (`integrations/claude-code`)
- Installed via `context-drop install-claude` (ships with the desktop app; end
  users need no Node.js). It lays down a self-contained local marketplace under
  `<config-dir>/plugins/marketplaces/context-drop/`. The install is idempotent and
  additive: it never overwrites unrelated settings and never wholesale-overwrites
  `settings.json`.
- Enable in a Claude Code session with:

  ```
  /plugin marketplace add <printed marketplace path>
  /plugin install context-drop@context-drop
  ```

- Skills: `/context-drop:pull`, `/context-drop:undo`, `/context-drop:status`.
- Optional user-level short alias `/cd` (install with `--short-alias`); it refuses
  to overwrite an existing `/cd` without `--force`.
- Agent: `context-investigator`, the isolated subagent that processes the packet.
- Multi-`CLAUDE_CONFIG_DIR` install: `CLAUDE_CONFIG_DIR` may be comma- or
  semicolon-separated. By default `install-claude` installs into `CLAUDE_CONFIG_DIR`
  plus `~/.claude`, or into a single `--config-dir`. All plugin instances across all
  config roots talk to the same global Context Drop packet store; packet data is never
  duplicated per account.
- The Settings UI shows the config path (for example `~/.claude Installed`,
  `~/.claude-work Installed`, `~/.claude-client2 Not installed`), never Anthropic
  account identity.

#### `/context-drop:pull` behavior
- The main agent must not read packet text, logs, images, JSON, or raw files. It
  runs `context-drop claim --json`, uses only the metadata, infers a task mode from
  the user's argument, and delegates the manifest path to the isolated
  `context-investigator` subagent. It then relays the compact result and runs
  `context-drop consume <packetId>`.
- Task modes:
  - `ANALYZE` (default) - read-only investigation (e.g. "原因だけ調べて",
    "investigate").
  - `FIX` - investigate, modify repo, test, then report (e.g. "原因を調べて直して").
  - `REVIEW` - read-only comparison unless explicitly authorized (e.g.
    "このUIをレビューして").
- Subagent compact result format: Status / Confidence
  (`CONFIRMED` | `HIGH` | `PROBABLE` | `UNKNOWN`) / Root cause or Findings / Evidence
  references (packet item path, source `file:line`, test names) / Changed files /
  Tests-Verification / Remaining unknowns. It never returns complete logs, large raw
  text, image binary, or large JSON.

#### Undo (routing only)
- `/context-drop:undo` and `context-drop undo` undo only the most recent eligible
  claim for the current session (same session id, claimed within ~5 minutes, not
  fully `CONSUMED`, safe to return to `READY`). Undo returns the packet to `READY`.
- Undo is routing only: it never rolls back source-code changes a `FIX` run already
  made.

#### Tauri desktop tray app (`apps/desktop`)
- Menu-bar / system-tray utility (not a large main window), built with Tauri 2 +
  React + TypeScript + Vite on a Rust backend (`src/` frontend, `src-tauri/`
  backend).
- Tray title: `○ Context Drop` when inactive, `● Context Drop · <item count>` while
  capturing.
- Popover shows Current Packet (item count, recent items), Stop Capture, and Clear
  Packet; Last Dispatch (project name, item count, time, Undo); and Settings (global
  shortcut, TTL, file size limit, packet size limit, Claude integration status/list,
  Install integration, Install `/cd` alias, Open data folder, Privacy info).
- Global shortcut via the Tauri 2 global-shortcut plugin, default
  `CommandOrControl+Shift+9`, user-configurable. OFF -> start capture, ON ->
  stop/finalize; a claim from `/context-drop:pull` also auto-ends capture. If
  registration fails, the user is told.
- Privacy UX: when Capture is ON the tray indicator is visually obvious, and capture
  never silently stays ON. On app restart Capture is OFF by default and never
  auto-resumes; an existing `DRAFT` may remain recoverable, but collection is OFF.

#### Architecture and coordination
- The desktop app and the CLI coordinate only via the shared SQLite database and the
  filesystem. There is no daemon, no HTTP/websocket/DB server, no network listener,
  and no localhost unauthenticated HTTP service.
- Data flow: Clipboard -> Packet -> Claim -> Claude session binding -> isolated
  subagent -> compact result.

#### Security and privacy
- No telemetry, no analytics, no cloud backend, no remote storage, no external API
  for Context Drop functionality, no clipboard-content network transmission by
  Context Drop, no always-on clipboard collection, and no hidden monitoring. Packet
  data stays local.
- The only content that leaves the machine is when Claude Code itself sends packet
  material to its configured model provider as its subagent reads it. That is Claude
  Code's normal behavior, not Context Drop transmitting data.
- App logs never contain clipboard raw content, file contents, image content, or
  secrets; they record only packet id, item type, item size, state transition, and
  error class.

#### Tests and CI
- Cross-cutting routing, concurrency, and lifecycle integration tests under
  `tests/`, plus the critical Rust unit tests in `crates/context-drop-core`.
- Rust workspace tests via `cargo test --workspace`; frontend tests via `pnpm test`
  (Vitest).
- Lint and formatting: `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo fmt --check`, `pnpm lint`, and `pnpm typecheck`.
- CI runs on macOS and Windows (Linux where feasible).

### Platform support
- Officially supported: macOS and Windows.
- Experimental: Linux (architecture present; not a supported target).

[0.1.0]: https://github.com/context-drop/context-drop/releases/tag/v0.1.0
