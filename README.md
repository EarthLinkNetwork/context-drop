# Context Drop

**Collect raw context once. Route it to the right Claude Code session. Let an isolated subagent do the reading — so your main conversation stays clean.**

Context Drop is a local-only desktop utility (tray / menu-bar) that gathers lots of raw material — screenshots, UI images, browser console output, server/debug logs, JSON responses, network results, stack traces, external docs, and multiple copied text fragments, images, or Finder/Explorer files — into a temporary **Context Packet**. When you are ready, you route that packet to a specific Claude Code session, where an **isolated subagent** reads the raw material and returns a **compact result** — without dumping the raw bytes into your main Claude conversation.

## Documentation

- [Architecture & data flow](docs/architecture.md)
- [Packet format, storage & database](docs/packet-format.md)
- [Claude Code plugin, routing & multiple accounts](docs/claude-code.md)
- [Privacy & security](docs/security.md)
- [Troubleshooting](docs/troubleshooting.md)

---

## 1. The problem

Debugging and reviews generate a pile of raw context: several screenshots, a console dump, a stack trace, a chunk of JSON, a couple of log files. The natural move is to paste all of it into your main Claude conversation. That has two costs:

- **It floods the main context.** Large logs, image bytes, and raw JSON crowd out the conversation you actually care about.
- **It couples collection to a single chat.** If you gathered the material with the wrong tab focused, or you want a different session to handle it, you are stuck re-pasting.

Context Drop separates **collecting** context from **deciding where it goes**, and keeps the heavy reading off your main thread.

## 2. The concept

Capture material into a Context Packet, then hand the packet — by reference, not by value — to the Claude Code session you choose. That session's main agent never reads the raw material itself. It delegates the packet's manifest **path** to an isolated `context-investigator` subagent, which reads the raw items and returns a short, structured result. Only that compact result reaches your main conversation.

**Core principle — this is the whole point:**

```
raw context → Context Drop Packet → isolated Claude Code subagent → compact result → main Claude
```

**Forbidden flow — Context Drop exists to prevent this:**

```
raw context → main Claude → subagent          ✗ never
```

The main agent must not read packet text, logs, images, JSON, or raw files. It works only from packet **metadata** (item counts, types, sizes, hashes, paths) and relays the subagent's compact output.

## 3. Install

**1. Download the desktop app** from the [**Releases**](https://github.com/EarthLinkNetwork/context-drop/releases) page. The macOS build is signed with a Developer ID certificate and **notarized by Apple**, so it opens without a Gatekeeper warning. Unzip it, move **Context Drop.app** to `/Applications`, and open it (it lives in the menu bar). The desktop app ships the `context-drop` companion CLI — end users need no Node.js, npm, or Rust.

> Current releases are **macOS (Apple Silicon / arm64)**. Intel-Mac (universal) and Windows builds are planned; building from source works on all three (see CONTRIBUTING).

**2. Launch the app once.** Opening Context Drop installs/refreshes the bundled `context-drop` CLI to its canonical location (no extra step needed).

**3. Install the Claude Code plugin.** This repository **is** a Claude Code marketplace, so you can add it directly from GitHub — no local path to copy:

```
/plugin marketplace add EarthLinkNetwork/context-drop
/plugin install context-drop@context-drop
```

Then open a new session (or just use `/cd`, the auto-loaded short alias).

<details>
<summary>Alternative: install the plugin locally from the app</summary>

Instead of the public marketplace, the desktop app can lay down a self-contained **local** marketplace: click **Settings → Claude Code Integrations → Install** (or run `context-drop install-claude`). It is idempotent and additive — it never overwrites unrelated settings. It installs into `CLAUDE_CONFIG_DIR` (comma/semicolon-separated) plus `~/.claude`; flags: `--config-dir <path>`, `--from <path>`, `--short-alias`, `--force`. It prints the local marketplace path to pass to `/plugin marketplace add`.
</details>

## 4. Basic use

1. **Start Capture** with the global shortcut (default `CommandOrControl+Shift+9`, configurable). The tray indicator turns solid: `● Context Drop · <item count>`.
2. **Add your material.** Either way, each item is appended to the current packet and the item list in the window grows as you go:
   - **Copy it** — multiple images, text fragments, and Finder/Explorer files. Each copy while Capture is ON is appended.
   - **Or drag & drop files** onto the Context Drop window — no need to copy, and it works even when Capture is OFF (a drop is itself an explicit "capture this").
3. **Switch to the intended Claude Code tab** — the exact session that should handle this context.
4. **Run the pull skill** in that session:

   ```
   /context-drop:pull Investigate this issue
   ```

   (or the short alias, if installed: `/cd Investigate this issue`)
5. **Context is routed to that exact session** and processed in an isolated subagent. The claim atomically ends capture; the subagent reads the raw items and returns a compact result to your main conversation.

Capture is **OFF by default** and only monitors the clipboard while Capture is ON — never an always-on daemon. (Drag & drop still works on demand even when Capture is OFF, because a drop is an explicit one-shot action, not background monitoring.)

## 5. Multi-tab routing

Context Drop uses **"capture first, route later."** A packet has destination `NONE` at capture time. The destination is decided **only** when `/context-drop:pull` runs from the target Claude Code session — never by most-recently-opened project, foreground app, window title, account, or the desktop UI selection.

**Example.** Capture builds one packet:

- Screenshot A
- Console log
- Screenshot B

You have three Claude Code tabs open: **Project Alpha**, **Project Beta**, **Project Gamma**. You switch to **Project Beta** and run:

```
/context-drop:pull Investigate this bug
```

**Expected:** the packet is atomically bound to **Project Beta's** Claude session.

- **No project chooser.**
- **No packet ID to type.**
- **No path to enter.**

Routing identity is `session_id` + `cwd` + `project_root`. `session_id` comes from `CLAUDE_CODE_SESSION_ID`, so the same repo open in multiple tabs is distinguished per tab. `project_root` is `git rev-parse --show-toplevel` in a git repo (a monorepo subdirectory is **not** a different project unless the git root differs), otherwise the canonicalized `cwd`. Your Anthropic account id is **never** used for routing. This scales to 20+ parallel sessions and 10+ accounts.

Claims are atomic via SQLite `BEGIN IMMEDIATE` + a conditional `UPDATE`, so **exactly one** session can ever claim a given packet, even under concurrent claims. Claiming a `DRAFT` packet atomically ends capture (`DRAFT → CLAIMED`); any later append is refused.

## 6. Privacy

Context Drop is **local-only**. There is:

- no telemetry, no analytics,
- no cloud backend, no remote storage, no external API for Context Drop functionality,
- no clipboard-content network transmission **by** Context Drop,
- no always-on clipboard collection, and no hidden monitoring.

Packet data stays on your machine. The desktop app and CLI coordinate **only** through the shared SQLite database and filesystem — there is no daemon, no HTTP/WebSocket/DB server, no network listener, and no localhost unauthenticated HTTP service.

**The one boundary to understand clearly:** the only time packet content leaves your machine is when **Claude Code itself** sends packet material to its configured model provider as its subagent reads it. That is Claude Code's normal behavior — **not** Context Drop transmitting your data.

App logs never contain clipboard raw content, file contents, image content, or secrets — only packet id, item type, item size, state transition, and error class. When Capture is ON the tray indicator is visually obvious; capture never silently stays ON, and on app restart Capture is OFF by default (it never auto-resumes, though an existing `DRAFT` may remain recoverable).

## 7. Claude Code integration

The plugin is self-contained and installed by `context-drop install-claude` (no Node.js needed for end users).

- **Skills:** `/context-drop:pull`, `/context-drop:undo`, `/context-drop:status`.
- **Optional short alias:** `/cd` (install with `--short-alias`; it refuses to overwrite an existing `/cd` without `--force`).
- **Agent:** `context-investigator` — the isolated subagent that reads packet items and returns the compact result.
- **Multiple config dirs:** supported and required for users with many accounts. `CLAUDE_CONFIG_DIR` may be comma- or semicolon-separated; `install-claude` also targets `~/.claude` by default.
- **One global store:** every plugin instance across every config root talks to the **same** global Context Drop packet store. Packet data is never duplicated per account. The Settings UI shows the config path and install status (e.g. `~/.claude Installed`, `~/.claude-work Installed`, `~/.claude-client2 Not installed`) — never Anthropic account identity.

**`/context-drop:pull` behavior.** The main agent runs `context-drop claim --json`, uses only the returned metadata, infers a task mode from your argument, delegates the manifest **path** to the `context-investigator` subagent, relays the compact result, then runs `context-drop consume <packetId>`. Task modes:

- **ANALYZE** — read-only investigation (default; e.g. "原因だけ調べて", "investigate").
- **FIX** — investigate → modify repo → test → report (e.g. "原因を調べて直して").
- **REVIEW** — read-only comparison unless explicitly authorized (e.g. "このUIをレビューして").

The subagent returns a compact, structured result — Status / Confidence (`CONFIRMED` | `HIGH` | `PROBABLE` | `UNKNOWN`) / Root cause or Findings / Evidence references / Changed files / Tests-Verification / Remaining unknowns — and never returns complete logs, large raw text, image binary, or large JSON.

**Undo is routing only.** `/context-drop:undo` (and `context-drop undo`) returns the most recent eligible claim for the **current** session back to `READY`. It undoes **routing only** — it never rolls back source-code changes a FIX run already made.

## 8. Development

### Build

```sh
pnpm install          # dev-only; not required for end users at runtime
cargo build           # Rust workspace
```

Desktop development runs from `apps/desktop` via the Tauri dev command, and requires the Rust toolchain plus your platform's WebView.

### Test

```sh
cargo test --workspace   # Rust
pnpm test                # Vitest frontend
```

### Lint

```sh
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --check
pnpm lint
pnpm typecheck
```

CI runs on macOS and Windows (and Linux where feasible).

### Tech stack

Tauri 2 + React + TypeScript + Vite frontend; Rust core; bundled SQLite (WAL mode, `busy_timeout` 5000ms, `foreign_keys` ON, `synchronous` NORMAL). Package manager: pnpm (dev-only). A Rust workspace.

### Repository layout

| Path | Contents |
|------|----------|
| `crates/context-drop-core` | Packet model, SQLite authority, claim routing, storage, cleanup, project detection (pure Rust; all critical tests here). |
| `crates/context-drop-clipboard` | Platform clipboard abstraction (`ClipboardProvider` trait; `macos.rs` / `windows.rs` / `linux.rs`). Text + image via `arboard`; native APIs (objc2 `NSPasteboard` on macOS, `clipboard-win` `CF_HDROP` on Windows) for file lists and change detection; images normalized to PNG. |
| `crates/context-drop-cli` | The `context-drop` companion CLI. |
| `apps/desktop` | Tauri 2 app — React frontend in `src/`, Rust backend in `src-tauri/`. Tray / menu-bar utility. |
| `integrations/claude-code` | The Claude Code plugin (`.claude-plugin/plugin.json`, `skills/pull|undo|status/SKILL.md`, `agents/context-investigator.md`, `alias/cd/SKILL.md`). |
| `tests/` | Cross-cutting routing / concurrency / lifecycle integration tests. |
| `docs/` | This documentation. |
