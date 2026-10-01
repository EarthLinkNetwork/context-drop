# Contributing to Context Drop

Thanks for your interest in contributing to Context Drop — a local-only desktop
utility that collects raw context (screenshots, logs, console output, JSON
responses, stack traces, copied text/images, files) into a temporary **Context
Packet** and routes it to a chosen Claude Code session, where an **isolated
subagent** processes it and returns a compact result.

Before you touch code, please internalize the one principle the whole project
exists to protect:

```
raw context -> Context Drop Packet -> isolated Claude Code subagent -> compact result -> main Claude
```

The following flow is **forbidden** and any change that enables it will be
rejected:

```
raw context -> main Claude -> subagent
```

The main agent must never read packet text, logs, images, JSON, or raw files. It
works from metadata and delegates the manifest **path** to the isolated
`context-investigator` subagent.

Please also read [`SECURITY.md`](./SECURITY.md) and the documents under
[`docs/`](./docs/) before making changes that touch capture, storage, routing,
or the Claude Code integration.

---

## Prerequisites

- **Rust (stable), installed via [rustup](https://rustup.rs/).** This is a Rust
  workspace and the core is pure Rust.
- **Node + [pnpm](https://pnpm.io/).** pnpm is the package manager and is
  **dev-only** — end users need no Node.js/npm at runtime.
- Platform WebView for desktop development (see below).

Supported platforms for v1.0 are **macOS** and **Windows**. Linux is
**experimental** — the architecture is present, but Linux work must never block
macOS/Windows.

---

## Repository layout

- `crates/context-drop-core` — packet model, SQLite authority, claim routing,
  storage, cleanup, and project detection. Pure Rust; **all critical tests live
  here**.
- `crates/context-drop-clipboard` — platform clipboard abstraction
  (`trait ClipboardProvider`; `macos.rs` / `windows.rs` / `linux.rs`). Text and
  image via `arboard`; native APIs (`objc2` `NSPasteboard` on macOS,
  `clipboard-win` `CF_HDROP` on Windows) for file lists and change detection;
  images are normalized to PNG.
- `crates/context-drop-cli` — the `context-drop` companion CLI.
- `apps/desktop` — the Tauri 2 app (React frontend in `src/`, Rust backend in
  `src-tauri/`). A tray / menu-bar utility.
- `integrations/claude-code` — the Claude Code plugin
  (`.claude-plugin/plugin.json`, `skills/pull|undo|status/SKILL.md`,
  `agents/context-investigator.md`, `alias/cd/SKILL.md`).
- `tests/` — cross-cutting routing / concurrency / lifecycle integration tests.
- `docs/` — project documentation.

---

## Building, testing, and linting

### Rust workspace

```sh
cargo test --workspace
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
```

Clippy runs with `-D warnings`, so warnings are treated as errors. Run
`cargo fmt` (without `--check`) to apply formatting before committing.

### Frontend (Tauri / React / TypeScript / Vite)

```sh
pnpm install
pnpm lint
pnpm typecheck
pnpm test      # Vitest
pnpm build
```

Please make sure all of the above pass locally before opening a PR. CI runs on
**macOS and Windows** (Linux where feasible).

---

## Running the desktop app in dev

From `apps/desktop`, run the Tauri dev command. Desktop development requires the
Rust toolchain plus the platform WebView.

The desktop app is a menu-bar / system-tray utility, not a large main window.
Capture is **OFF by default** and only monitors the clipboard while Capture is
ON — it is never an always-on daemon, and it never auto-resumes on restart.

The desktop app and the CLI coordinate **only** via the shared SQLite database
and the filesystem. There is no daemon, no HTTP/websocket/DB server, and no
network listener. Do not add one.

---

## Coding principles

- **Boring and deterministic.** Prefer straightforward, predictable code.
  Concurrency correctness comes from SQLite (`BEGIN IMMEDIATE` + conditional
  updates), not from ad-hoc coordination.
- **Keep dependencies minimal.** Add a dependency only when it clearly earns its
  place.
- **Do not introduce any of the following:** Electron, a cloud backend, Redis,
  an external database, a background scheduler/cron/service, analytics, or an
  auth server. These are explicit non-goals.
- **Platform code stays out of the core domain.** `context-drop-core` is pure
  Rust. Platform-specific clipboard behavior belongs in
  `context-drop-clipboard` behind `trait ClipboardProvider`
  (`macos.rs` / `windows.rs` / `linux.rs`), never in the core domain.
- **SQLite is the metadata authority; the manifest is a projection.** Raw file
  contents are never inlined into the manifest — only references and hashes.
- **Metadata only, never raw content.** The CLI (`claim --json`, etc.), the
  main agent, and app logs must never surface raw clipboard content, logs, JSON
  bodies, image bytes, file contents, or secrets. App logs may contain only
  packet id, item type, item size, state transition, and error class.
- **Capture-first, route-later.** A packet's destination is `NONE` at capture
  time and is decided only when `/context-drop:pull` runs from the target
  session. Never route by most-recently-opened project, foreground app, window
  title, Anthropic account, or the desktop UI selection. Routing identity is
  `session_id + cwd + project_root` (using `CLAUDE_CODE_SESSION_ID`).

---

## Test expectations

- **The routing and concurrency tests must stay green.** Changes that touch
  claim routing, the packet state machine, or SQLite transactions must keep the
  `tests/` integration suite and the `context-drop-core` tests passing.
- Claims are atomic (SQLite `BEGIN IMMEDIATE` + conditional `UPDATE`) so that
  exactly one session can ever claim a given packet, even under concurrent
  claims. Append verifies `state == DRAFT` inside the transaction so it can
  never win a race against a claim. Do not weaken these guarantees.
- Context Drop is expected to support 20+ parallel sessions and 10+ Claude
  accounts sharing the **same** global packet store; keep tests that exercise
  parallelism and multi-config-dir behavior green.
- The packet state machine
  (`DRAFT`, `READY`, `CLAIMED`, `PROCESSING`, `CONSUMED`, `FAILED`, `EXPIRED`)
  and CLI exit codes (`0` ok, `1` error, `3` `NO_PACKET`, `4`
  `MISSING_SESSION_ID`, `5` `NOTHING_TO_UNDO`) are part of the contract — update
  tests and docs together when they legitimately change.

---

## Pull request guidance

- Keep PRs focused and reasonably small. Describe what changed and why, and note
  which crate(s)/app(s) are affected.
- Ensure the full local check set passes before opening the PR:
  - `cargo test --workspace`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --check`
  - `pnpm install`, `pnpm lint`, `pnpm typecheck`, `pnpm test`, `pnpm build`
- If you change routing, the state machine, storage layout, the manifest schema
  (`schemaVersion`), the database schema (`PRAGMA user_version`), CLI
  flags/exit codes, or the Claude Code integration, update the corresponding
  docs under [`docs/`](./docs/) in the same PR.
- Any change touching capture, storage, routing, logging, or transmission must
  be consistent with [`SECURITY.md`](./SECURITY.md): no telemetry, no analytics,
  no cloud backend, no remote storage, no external API for Context Drop
  functionality, no clipboard-content network transmission by Context Drop, no
  always-on clipboard collection, no hidden monitoring, and no localhost
  unauthenticated HTTP service.
- Do not regress the privacy UX: Capture must be visually obvious while ON,
  must never silently stay ON, and must be OFF by default on restart.

Thank you for helping keep Context Drop small, local, and trustworthy.
