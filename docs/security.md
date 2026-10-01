# Security & Privacy

Context Drop is a local-only desktop utility. It collects raw context (screenshots, UI images, browser console output, server/debug logs, JSON responses, network results, stack traces, external docs, copied text and images, Finder/Explorer files) into a temporary **Context Packet**, then routes it to a chosen Claude Code session where an **isolated subagent** processes it and returns a compact result.

This document describes the security and privacy guarantees that follow from that design. Everything here is a property of Context Drop itself; the one boundary where content can leave the machine is called out explicitly under [The model-provider boundary](#the-model-provider-boundary).

---

## Design principle: raw content never reaches the main conversation

The entire point of Context Drop is to keep raw material out of your main Claude conversation:

```
raw context -> Context Drop Packet -> isolated Claude Code subagent -> compact result -> main Claude
```

The following flow is **forbidden** and does not happen:

```
raw context -> main Claude -> subagent
```

Concretely, when `/context-drop:pull` runs:

- The **main agent MUST NOT** read packet text, logs, images, JSON, or raw files.
- The main agent runs `context-drop claim --json` and uses **only the metadata** it returns.
- It delegates the manifest **path** to the isolated `context-investigator` subagent.
- It relays only the subagent's **compact result** and then runs `context-drop consume <packetId>`.

The subagent's compact result is deliberately small — Status, Confidence (`CONFIRMED` | `HIGH` | `PROBABLE` | `UNKNOWN`), root cause or findings, evidence references (packet item path, source `file:line`, test names), changed files, tests/verification, and remaining unknowns. It **never** returns complete logs, large raw text, image binaries, or large JSON.

This means the bulk raw context is confined to the isolated subagent and never dumped into the main conversation.

---

## Clipboard privacy

Context Drop is **not** an always-on clipboard monitor.

- **Capture is OFF by default.** The clipboard is monitored **only while Capture is ON**. There is no always-on daemon and no hidden background collection.
- **The tray indicator makes capture obvious.** While Capture is ON the tray title shows a filled marker and the live item count — `● Context Drop · <item count>` — versus the inactive `○ Context Drop`. Capture never silently stays ON.
- **Capture is OFF after restart.** On app restart, Capture is **OFF by default and never auto-resumes**. An existing `DRAFT` packet may remain recoverable, but collection is OFF until you explicitly start it again.
- **Explicit start/stop.** Capture is toggled by the user — via the global shortcut (default `CommandOrControl+Shift+9`, user-configurable) or the popover. A claim from `/context-drop:pull` also auto-ends capture (the `DRAFT` transitions to `CLAIMED` and any later append is refused).

---

## Local storage

All packet data stays on the local machine. There is no cloud backend and no remote storage.

### Location

Data lives in a single global per-OS-user directory:

- App identifier / data-dir leaf: `com.contextdrop.app`
- Resolved as `dirs::data_dir()/com.contextdrop.app` (matches Tauri v2 `app_data_dir`)
- Override with the `CONTEXT_DROP_DATA_DIR` environment variable

Layout:

```
<data-dir>/
  context-drop.db          # SQLite metadata authority (WAL mode)
  config.json
  packets/<packet-id>/
    manifest.json          # projection of the DB; references + hashes only
    items/0001.png, 0002.txt, ...
  logs/
  bin/
```

Packet ids are **UUIDv7** (collision-resistant and time-ordered; never timestamp-only).

### Filesystem permissions

- **Unix (macOS / Linux):** directories are created `0700` and files `0600` — owner-only access.
- **Windows:** the app relies on per-user `%APPDATA%` ACLs for isolation.

### No local network service

The desktop app and the `context-drop` CLI coordinate **only via the shared SQLite database and the filesystem**. There is:

- **No daemon**
- **No HTTP / WebSocket / DB server**
- **No network listener**
- **No localhost unauthenticated HTTP service**

SQLite runs in WAL mode with a 5s busy timeout, `foreign_keys` ON, and `synchronous NORMAL`. Claims and other critical writes use `BEGIN IMMEDIATE` transactions so exactly one session can ever claim a given packet, even under concurrent claims — this is a data-integrity property, achieved without any network coordination.

---

## No-network guarantees (absolute)

Context Drop, as a product, makes the following hard guarantees. None of these are configurable "off by default" toggles — they are absent by design:

- **No telemetry**
- **No analytics**
- **No cloud backend**
- **No remote storage**
- **No external API** for Context Drop functionality
- **No clipboard-content network transmission** by Context Drop
- **No always-on clipboard collection**
- **No hidden monitoring**
- **No localhost unauthenticated HTTP service**

Packet data stays local.

---

## The model-provider boundary

This is the single most important distinction to understand.

**Context Drop itself transmits nothing off the machine.** It has no network path for packet content.

The **only** time packet material leaves your machine is when **Claude Code itself** sends that material to its configured model provider — as the isolated `context-investigator` subagent reads the packet items during processing. That is **Claude Code's normal behavior**, exactly as it would be for any file or content you point Claude Code at. It is **not** Context Drop transmitting your data.

In other words:

| Actor | Sends packet content off-machine? |
| --- | --- |
| Context Drop (desktop app, CLI, storage layer) | **No — never.** |
| Claude Code (when its subagent reads the packet) | Yes — to Claude Code's configured model provider, as normal Claude Code operation. |

If you need packet content to never leave the machine at all, that is governed by how you configure and use Claude Code, not by Context Drop.

---

## Logging

App logs are metadata-only. They **never** contain:

- Clipboard raw content
- File contents
- Image content
- Secrets

Logs record **only**:

- Packet id
- Item type
- Item size
- State transitions
- Error class

This mirrors the same discipline applied everywhere else in Context Drop: metadata is authoritative and freely recorded; raw content is referenced by path and hash but never inlined or transmitted by the product.

### Related: the manifest never inlines content

Consistent with logging, the packet manifest (`packets/<id>/manifest.json`, `schemaVersion = 1`) stores **only references and hashes** — each item records `id`, `kind`, `mimeType`, `relativePath`, `byteSize`, `sha256`, and `createdAt`. **Raw file contents are never inlined.** SQLite is the metadata authority; the manifest is a projection of it.

### Related: `claim --json` returns metadata only

`context-drop claim --json` returns metadata only — never raw content, logs, JSON contents, image bytes, or file contents. A success payload looks like:

```json
{
  "ok": true,
  "packetId": "...",
  "manifestPath": "...",
  "itemCount": 7,
  "sessionId": "...",
  "cwd": "...",
  "projectRoot": "...",
  "projectName": "prompt-flow"
}
```

---

## Routing and account privacy

Context Drop's routing identity is `session_id + cwd + project_root`. It uses `CLAUDE_CODE_SESSION_ID` to distinguish sessions, and derives `project_root` from `git rev-parse --show-toplevel` (or the canonicalized cwd outside a git repo).

For privacy specifically:

- The **Anthropic account id is NEVER used for routing**.
- The desktop Settings UI shows the **config path** and its install status (e.g. `~/.claude Installed`, `~/.claude-work Installed`, `~/.claude-client2 Not installed`) — **never Anthropic account identity**.
- Multiple `CLAUDE_CONFIG_DIR` roots are supported, and all plugin instances across all config roots talk to the **same global Context Drop packet store**. Packet data is **never duplicated per account**.

---

## Data lifecycle and cleanup

- **TTL:** default 24h (configurable).
- **Triggers:** cleanup runs on **app startup and new-capture start only**. There is **no scheduler, cron, or background service**.
- **Protected states:** `DRAFT` and `PROCESSING` packets are **never age-deleted**. Stale `READY` / `CLAIMED` / `FAILED` packets are marked `EXPIRED` and then removed together with terminal packets past TTL.

Because cleanup is event-triggered rather than scheduled, Context Drop never runs unattended in the background to manage your data.

---

## Summary

- Raw context is confined to an isolated subagent and its compact result — it never lands in the main Claude conversation.
- The clipboard is watched only while Capture is ON, which is always visible in the tray, off by default, and never auto-resumed after restart.
- All data is stored locally under a per-user directory with owner-only permissions (`0700`/`0600` on Unix, `%APPDATA%` ACLs on Windows) and coordinated through SQLite + the filesystem — no network listener, no localhost HTTP service.
- Context Drop transmits nothing off the machine. The only content that leaves is what Claude Code itself sends to its configured model provider when its subagent reads a packet — normal Claude Code behavior, not Context Drop.
- Logs and manifests record metadata (ids, types, sizes, state transitions, hashes, error classes) and never raw clipboard content, file contents, image content, or secrets.
