# Troubleshooting

This guide covers the most common issues when using Context Drop and its
`context-drop` companion CLI and Claude Code plugin.

Context Drop is local-only. The desktop app and the CLI coordinate **only**
through the shared SQLite database and the filesystem — there is no daemon, no
HTTP/WebSocket/DB server, and no network listener. If something is not behaving
as expected, the cause is almost always local state (capture is off, no packet
is ready, a session id is missing, or the plugin is not installed for the
config directory you are using).

## First diagnostic: `context-drop doctor`

Before anything else, run:

```sh
context-drop doctor
```

`doctor` is the first-line diagnostic for any problem. If your data lives in a
non-default location, pass the global flag:

```sh
context-drop --data-dir <path> doctor
```

The CLI uses these exit codes, which are useful when scripting or reading
failures:

| Exit code | Meaning |
|-----------|---------|
| `0` | OK |
| `1` | Generic error |
| `3` | `NO_PACKET` |
| `4` | `MISSING_SESSION_ID` |
| `5` | `NOTHING_TO_UNDO` |

---

## "No packet" (`NO_PACKET`, exit 3)

**Symptom.** `/context-drop:pull` or `context-drop claim` reports that nothing
is ready, or the CLI exits with code `3`. The exact message is:

```
No Context Drop packet is ready. Start Capture and copy the materials first.
```

**Cause.** Capture is **OFF by default** and only monitors the clipboard while
Capture is ON — it is never an always-on daemon. On app restart Capture is
always OFF and never auto-resumes. If you have not started Capture and copied
your materials, there is no `DRAFT` to claim and no `READY` packet to route.

**Fix.**

1. Start Capture from the tray/menu-bar popover, or press the global shortcut
   (default `CommandOrControl+Shift+9`). The tray title changes from
   `○ Context Drop` to `● Context Drop · <item count>` while capturing.
2. Copy your materials (screenshots, logs, JSON, files, text, images). Each
   copy is appended to the current packet.
3. Check what the store currently holds:

   ```sh
   context-drop status
   context-drop status --json
   context-drop list --json
   ```

Claim priority when you run `/context-drop:pull` is: (1) the current `DRAFT`
with items > 0, (2) the newest `READY` packet, otherwise (3) `NO_PACKET`. If
`status` shows an empty `DRAFT` (zero items) and no `READY` packet, you will get
`NO_PACKET` — copy at least one item first.

---

## Global shortcut conflicts

**Symptom.** The global shortcut does nothing, or the app tells you that
shortcut registration failed.

**Cause.** The shortcut (default `CommandOrControl+Shift+9`, registered via the
Tauri 2 global-shortcut plugin) is already claimed by another application or by
the OS. When registration fails, the app tells you — it does not fail silently.

**Fix.** Open the tray/menu-bar **Settings** and change the **global shortcut**
to a free combination. The shortcut is user-configurable. Its behavior:

- When Capture is OFF, the shortcut **starts** capture.
- When Capture is ON, the shortcut **stops/finalizes** the packet.

Note that a claim from `/context-drop:pull` also auto-ends capture
(`DRAFT -> CLAIMED`), so you do not always need the shortcut to finalize.

---

## Plugin not found in Claude Code

**Symptom.** `/context-drop:pull`, `/context-drop:undo`, or
`/context-drop:status` is not recognized inside a Claude Code session, or
`/plugin install` cannot find the marketplace.

**Cause.** The plugin has not been installed for the Claude Code config
directory you are currently using, or the marketplace has not been added and the
plugin not enabled in this session.

**Fix.**

1. Install the plugin for the correct config directory. It ships with the
   desktop app, and end users need no Node.js:

   ```sh
   context-drop install-claude
   ```

   `install-claude` lays down a self-contained local marketplace under
   `<config-dir>/plugins/marketplaces/context-drop/`. It is idempotent and
   additive: it never overwrites unrelated settings and never wholesale-
   overwrites `settings.json`. Relevant flags:

   ```
   context-drop install-claude [--config-dir <path>] [--from <path>] \
                               [--short-alias] [--force]
   ```

2. Enable it inside a Claude Code session using the marketplace path the
   installer printed:

   ```
   /plugin marketplace add <printed marketplace path>
   /plugin install context-drop@context-drop
   ```

3. Confirm the CLI and store are healthy:

   ```sh
   context-drop doctor
   ```

The plugin provides the skills `/context-drop:pull`, `/context-drop:undo`, and
`/context-drop:status`, plus the isolated `context-investigator` subagent.

### Optional `/cd` short alias

An optional user-level short alias `/cd` can be installed with:

```sh
context-drop install-claude --short-alias
```

If a `/cd` alias already exists, the installer **refuses to overwrite it**
unless you also pass `--force`.

---

## Session id unavailable (`MISSING_SESSION_ID`, exit 4)

**Symptom.** `context-drop claim` exits with code `4` (`MISSING_SESSION_ID`), or
routing does not work.

**Cause.** Routing identity is `session_id + cwd + project_root` and depends on
the `CLAUDE_CODE_SESSION_ID` environment variable, which is set inside Claude
Code sessions. If that variable is not present — for example, when you run the
command from a plain shell rather than from within a Claude Code session — the
CLI cannot establish the session identity.

**Fix.**

- Run `/context-drop:pull` (or `context-drop claim`) **from inside a Claude Code
  session**, where `CLAUDE_CODE_SESSION_ID` is available.
- If you must invoke `claim` manually, you can supply the identity explicitly:

  ```sh
  context-drop claim --session-id <S> --cwd <P>
  ```

Context Drop determines `project_root` as `git rev-parse --show-toplevel` when
inside a git repository, otherwise the canonicalized `cwd`. A monorepo subdir is
**not** a different project unless the git root differs. The same repository open
in multiple tabs is distinguished by `session_id`. The Anthropic account id is
never used for routing.

---

## Multiple Claude config directories

**Symptom.** The plugin works in one Claude account/config root but not another,
or the Settings UI shows a config path as **Not installed**.

**Cause.** The plugin must be installed once per config directory. Context Drop
supports (and expects) multiple config dirs — `CLAUDE_CONFIG_DIR` may be comma-
or semicolon-separated for users with many accounts.

**Fix.** Install into each config root you use. By default `install-claude`
installs into every path in `CLAUDE_CONFIG_DIR` **plus** `~/.claude`; or target a
single root explicitly:

```sh
context-drop install-claude --config-dir <path>
```

**One global store.** All plugin instances across all config roots talk to the
**same** global Context Drop packet store — packet data is never duplicated per
account. The Settings UI shows install status per config path (for example
`~/.claude Installed`, `~/.claude-work Installed`,
`~/.claude-client2 Not installed`) and never shows Anthropic account identity.

---

## Windows clipboard problems

**Symptom.** Copied files from Explorer are not captured, or new clipboard copies
are not detected on Windows.

**Cause / behavior.** On Windows, file lists are read via the native
`CF_HDROP` clipboard format (through `clipboard-win`), and change detection uses
the clipboard sequence number. Text and images go through `arboard`, and images
are normalized to PNG.

**Fix / checks.**

1. Make sure Capture is ON before copying (the tray shows `● Context Drop`).
2. Copy the files from Explorer so they land on the clipboard as a `CF_HDROP`
   file list, then confirm the packet grew:

   ```sh
   context-drop status
   context-drop doctor
   ```

3. If a copy is not detected, copy again — detection is driven by the clipboard
   sequence-number change, so re-copying advances it.

Windows relies on per-user `%APPDATA%` ACLs to protect the data directory (there
are no Unix-style permission bits).

---

## macOS permissions

**Symptom.** Clipboard content (especially file lists) is not captured on macOS.

**Cause / behavior.** On macOS, file lists and clipboard change detection use the
native `NSPasteboard` API (via `objc2`); text and images go through `arboard`
and images are normalized to PNG. If macOS requires a permission (for example a
clipboard or accessibility prompt) for the app to observe the clipboard, capture
will not work until that permission is granted.

**Fix.** If prompted, grant the requested clipboard/accessibility permission to
the Context Drop app in **System Settings**, then start Capture again. Verify
with:

```sh
context-drop doctor
context-drop status
```

---

## Where is my data?

Context Drop stores everything globally per OS user under the app data
directory `com.contextdrop.app`, resolved as
`dirs::data_dir()/com.contextdrop.app` (matching the Tauri v2 `app_data_dir`).
You can override the location with the `CONTEXT_DROP_DATA_DIR` environment
variable, or point any CLI invocation at it with `--data-dir <path>`.

Layout:

```
<data-dir>/context-drop.db
<data-dir>/config.json
<data-dir>/packets/<packet-id>/manifest.json
<data-dir>/packets/<packet-id>/items/0001.png, 0002.txt, ...
<data-dir>/logs/
<data-dir>/bin/
```

You can also open this folder from the tray **Settings** via **Open data
folder**. On Unix, directories are `0700` and files `0600`.

---

## A packet was claimed by the wrong session — undo the routing

If you ran `/context-drop:pull` from the wrong session, undo the routing:

```
/context-drop:undo
```

or from the CLI:

```sh
context-drop undo
context-drop undo --json
```

Undo only affects the **most recent eligible claim for the current session**
(same session id, claimed within roughly the last 5 minutes, not fully
`CONSUMED`, and safe to return to `READY`). It returns the packet to `READY`. If
there is nothing to undo, the CLI exits with code `5` (`NOTHING_TO_UNDO`).

**Undo is routing only.** It never rolls back source-code changes that a FIX run
already made. If a FIX run modified your repository, undoing the claim does not
revert those file changes — use your version control to review or revert code.

To release a packet back to `READY` without the "current session, recent" undo
constraints, or to mark a claimed packet done, use:

```sh
context-drop release <packet-id>
context-drop consume <packet-id>
```

---

## Items are being rejected (size limits)

**Symptom.** A copied item does not appear in the packet and a reason is
reported.

**Cause.** An item or the packet exceeded the configured size limits. Defaults
are `maxItemBytes` = 25 MiB and `maxPacketBytes` = 200 MiB (both configurable in
Settings).

**Behavior.** On exceed, Context Drop does **not** crash and does **not**
silently truncate. The offending item is rejected with a reason, and the
existing packet is preserved. Copy a smaller item, or raise the limits in
Settings if appropriate.

---

## A packet disappeared (TTL cleanup)

**Symptom.** A packet you left around is gone.

**Cause.** Packets have a TTL (default 24h, configurable). Cleanup runs only at
**app startup** and when a **new capture starts** — there is no scheduler, cron,
or background service. Stale `READY`/`CLAIMED`/`FAILED` packets past TTL are
marked `EXPIRED` and removed along with terminal packets past TTL.

**Note.** `DRAFT` and `PROCESSING` packets are **never** age-deleted, so an
in-progress capture or an actively processing packet will not vanish due to TTL.

---

## Reading the packet state

If you are unsure what state a packet is in, `context-drop status` /
`context-drop list` report it. The states are:

| State | Meaning |
|-------|---------|
| `DRAFT` | Capturing (Capture is ON). |
| `READY` | Capture stopped, not yet handed to a session. |
| `CLAIMED` | Bound to a specific Claude session. |
| `PROCESSING` | The subagent has started. |
| `CONSUMED` | Done. |
| `FAILED` | Claim/processing failed; recoverable to `READY`. |
| `EXPIRED` | Removed by TTL cleanup. |

---

## A note on privacy (this is not a bug)

Context Drop performs **no** network transmission of clipboard content itself:
no telemetry, no analytics, no cloud backend, no remote storage, no always-on
clipboard collection. Packet data stays local.

The **only** content that leaves your machine is when **Claude Code itself**
sends packet material to its configured model provider as its subagent reads the
manifest — that is Claude Code's normal behavior, **not** Context Drop
transmitting data. Likewise, the app's own logs never contain clipboard raw
content, file contents, image content, or secrets; they record only packet id,
item type, item size, state transitions, and error class.

## Known limitations

A few narrow, low-impact edges are documented here rather than fully eliminated,
because the practical impact is negligible and self-healing:

- **Manifest projection can briefly lag under a rare interleave.** SQLite is the
  authority; `manifest.json` is a projection re-derived on every state change. If
  a capture append and a session claim publish the manifest at nearly the same
  instant, the file can momentarily show the older state — but it always lists
  the **same items** (only the `state`/claim fields may lag), and the next
  transition (`processing`, which the pull skill runs right after claim, or
  `consume`) re-derives it. The investigator reads the manifest **after** claim
  and processing, so it sees the corrected projection. During capture there is a
  single writer (the one desktop capture thread) and no appends occur after a
  packet is claimed.
- **Undo targets the single most-recent dispatch.** If two dispatches share the
  exact same millisecond timestamp, ordering between them is by insertion; Undo
  still refuses to fall back to an unrelated older dispatch.
- **Transient clipboard "busy" is retried; other transient failures may be
  skipped.** A "clipboard occupied" error is surfaced so capture retries on the
  next poll. Some platform clipboard errors are indistinguishable from "no
  content of this type" and are treated as empty; a rare transient of that kind
  would be picked up on the next copy.
- **Windows opt-in `--add-to-path`** (registry PATH update) is not yet
  implemented; the canonical `<data-dir>/bin` fallback the skills resolve to
  makes PATH optional for functionality (see `docs/cli-distribution.md`).
- **Deleting an item is DB-authoritative; the file is removed best-effort.** When
  you remove a single item from a packet, the database row and the packet's size
  tally are updated atomically, and then the item's file is deleted. If that file
  removal fails (e.g. a permission error, or a crash between the commit and the
  unlink), the file can linger on disk. It is never referenced again — the
  manifest is re-derived from the database, so the removed item never reaches a
  subagent — and it is swept when the whole packet directory is later removed
  (consume / clear / TTL). The effect is bounded disk use, not incorrect content.
