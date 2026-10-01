# Security Policy

Context Drop is a **local-only** desktop utility. It collects raw context (screenshots,
UI images, browser console output, server/debug logs, JSON responses, network results,
stack traces, external docs, and multiple copied text/image fragments or files) into a
temporary **Context Packet** and routes it to a chosen Claude Code session, where an
**isolated subagent** processes it and returns a compact result — without dumping the raw
material into the main Claude conversation.

This document describes the threat model, the privacy guarantees Context Drop makes, where
data is stored and how it is protected, the boundary between Context Drop and your model
provider, how to report a vulnerability, and which versions are supported.

## Supported Versions

| Version | Platforms                         | Supported          |
| ------- | --------------------------------- | ------------------ |
| 1.0.x   | macOS, Windows                    | :white_check_mark: |
| 1.0.x   | Linux (experimental)              | :warning: best-effort, not a release gate |
| < 1.0   | —                                 | :x:                |

v1.0 officially supports macOS and Windows. Linux support is experimental: the
architecture is present, but it is not a blocking target for macOS/Windows releases.
Security fixes are provided for the current 1.0.x line on the officially supported
platforms.

## Threat Model

Context Drop is designed around a single principle: **the raw context never enters the main
Claude conversation.** The intended flow is:

```
raw context -> Context Drop Packet -> isolated Claude Code subagent -> compact result -> main Claude
```

The following flow is explicitly **forbidden** by design:

```
raw context -> main Claude -> subagent
```

### What Context Drop is designed to protect against

- **Uncontrolled context leakage into the main conversation.** During a `/context-drop:pull`
  run, the main agent MUST NOT read packet text, logs, images, JSON, or raw files. It runs
  `context-drop claim --json`, uses only the returned **metadata**, infers a task mode, and
  delegates the manifest **path** to the isolated `context-investigator` subagent. Only the
  subagent's compact result is relayed back.
- **Cross-session / cross-account misrouting.** A packet has destination `NONE` at capture
  time ("capture first, route later"). The destination is decided **only** when
  `/context-drop:pull` runs from the target Claude Code session. Routing identity is
  `session_id + cwd + project_root`, derived from `CLAUDE_CODE_SESSION_ID` and
  `git rev-parse --show-toplevel` (or the canonicalized cwd outside a git repo). Routing is
  **never** performed by most-recently-opened project, foreground app, window title,
  Anthropic account, or the desktop UI selection. The same repo open in multiple tabs is
  distinguished by `session_id`. This supports 20+ parallel sessions and 10+ Claude accounts
  without cross-talk.
- **Concurrent-claim races.** Claims are atomic via SQLite `BEGIN IMMEDIATE` plus a
  conditional `UPDATE`, so exactly one session can ever claim a given packet, even under
  concurrent claims. Claiming a `DRAFT` atomically ends capture (`DRAFT -> CLAIMED`); any
  later append is refused because append re-verifies `state == DRAFT` inside the transaction.
- **Silent, always-on clipboard collection.** Capture is **OFF by default** and monitors the
  clipboard **only while Capture is ON** — it is never an always-on daemon. On app restart
  Capture is OFF and never auto-resumes. When Capture is ON, the tray indicator is visually
  obvious so capture never silently stays on.
- **Oversized inputs / denial of service.** On exceeding the configured limits
  (`maxItemBytes`, default 25 MiB; `maxPacketBytes`, default 200 MiB), the item is rejected
  with a reason and the existing packet is preserved. Context Drop does not crash and does
  not silently truncate.

### Out of scope / assumptions

- **A trusted local OS user account.** Data is stored per OS user under the user's data
  directory and protected by filesystem permissions (see below). An attacker who already has
  read access to that OS user's files or session is outside the tool's threat model.
- **What Claude Code does with the material.** Once an isolated subagent reads packet
  material, transmission of that material to the configured model provider is Claude Code's
  own behavior, not Context Drop's (see [Model-Provider Boundary](#model-provider-boundary)).

## Privacy Guarantees

The following are absolute properties of Context Drop:

- **No telemetry, no analytics.**
- **No cloud backend, no remote storage, no external API** for Context Drop functionality.
- **No clipboard-content network transmission by Context Drop.**
- **No always-on clipboard collection and no hidden monitoring.** Capture is off by default
  and only runs while explicitly ON.
- **No localhost unauthenticated HTTP service**, and no daemon, HTTP/websocket/DB server, or
  network listener. The desktop app and CLI coordinate **only** through the shared SQLite
  database and the filesystem.
- **Packet data stays local.**

### Logging

App logs **never** contain clipboard raw content, file contents, image content, or secrets.
Logs record only: packet id, item type, item size, state transition, and error class.

## Data Storage and Permissions

### Location

Data is stored **globally per OS user** under the application data directory:

- App identifier / data-dir leaf: `com.contextdrop.app`
- Resolved as `dirs::data_dir()/com.contextdrop.app` (matching Tauri v2 `app_data_dir`)
- Override with the environment variable `CONTEXT_DROP_DATA_DIR`

### Layout

```
<data-dir>/
  context-drop.db          # SQLite (metadata authority)
  config.json
  packets/
    <packet-id>/
      manifest.json        # projection of the SQLite metadata; schemaVersion = 1
      items/
        0001.png
        0002.txt
        ...
  logs/
  bin/
```

Packet ids are **UUIDv7** (collision-resistant and time-ordered; never timestamp-only).

### What is stored — and what is not

- Item files under `items/` hold the captured material (images are normalized to PNG).
- The **manifest** stores only **references and hashes** — raw file contents are **never**
  inlined. Each item records `id`, `kind`, `mimeType`, `relativePath`, `byteSize`, `sha256`,
  and `createdAt`. The `claim` block (present only once `CLAIMED`) records `sessionId`,
  `cwd`, `projectRoot`, `projectName`, an optional `configDir` (diagnostics only), and
  `claimedAt`.
- **SQLite is the metadata authority; the manifest is a projection of it.** The database uses
  WAL mode, `busy_timeout` 5000ms, `foreign_keys` ON, and `synchronous` NORMAL, with the
  schema versioned via `PRAGMA user_version` (current v1). Tables: `packets`,
  `packet_items`, `claims`, `settings`, `migrations`.
- **The `claim --json` output is metadata only** — never raw content, logs, JSON contents,
  image bytes, or file contents.

### Filesystem permissions

- **Unix (macOS/Linux):** directories are created `0700` and files `0600`.
- **Windows:** protection relies on the per-user `%APPDATA%` ACLs.

### Retention

The default TTL is 24h (configurable). Cleanup runs **only** at app startup and when a new
capture starts — there is no scheduler, cron, or background service. `DRAFT` and `PROCESSING`
packets are never age-deleted; stale `READY`/`CLAIMED`/`FAILED` packets are marked `EXPIRED`
and then removed along with terminal packets past their TTL.

Note that **undo affects routing only.** `/context-drop:undo` (and `context-drop undo`)
returns the most recent eligible claim for the current session back to `READY`; it does
**not** roll back any source-code changes a `FIX` run already made.

## Model-Provider Boundary

This distinction is important: Context Drop itself transmits **nothing** off the machine.

The **only** time packet content leaves the machine is when **Claude Code itself** sends
packet material to its configured model provider as the isolated subagent reads it. That is
Claude Code's normal behavior, **not** Context Drop transmitting your data. Context Drop
stores and routes packets locally; it hands the isolated subagent a manifest **path**, and
any subsequent transmission is governed by Claude Code and your model-provider configuration.

The subagent is also constrained to return a **compact** result — Status, Confidence
(`CONFIRMED` / `HIGH` / `PROBABLE` / `UNKNOWN`), root cause or findings, evidence references,
changed files, tests/verification, and remaining unknowns — and **never** returns complete
logs, large raw text, image binary, or large JSON.

## Reporting a Vulnerability

> **Note:** The disclosure contact and process below are **placeholders and to-be-configured**
> by the project maintainers before public release. Update the contact address, response
> targets, and any coordinated-disclosure details before relying on this section.

If you believe you have found a security vulnerability in Context Drop, please report it
privately so it can be addressed before public disclosure.

- **Contact:** `security@example.invalid` *(TO BE CONFIGURED)*
- **Please do not** open a public GitHub issue for security vulnerabilities.

When reporting, please include:

- A clear description of the issue and its potential impact.
- Steps to reproduce, including affected version and platform (macOS / Windows / Linux).
- Any relevant logs — but **do not** include clipboard contents, secrets, or other sensitive
  material in your report.

**Coordinated disclosure targets** *(TO BE CONFIGURED)*:

- Acknowledgement of your report: within *N* business days.
- Initial assessment / triage: within *N* business days.
- Fix and coordinated disclosure timeline: agreed with the reporter.

We will keep you informed of progress and credit reporters who wish to be acknowledged,
subject to the finalized process above.
