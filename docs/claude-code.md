# Claude Code Integration

Context Drop ships a Claude Code **plugin** that turns a temporary Context Packet into
a compact, model-ready result — **without dumping the raw material into your main
Claude conversation**.

The whole point of the integration is this flow:

```
raw context -> Context Drop Packet -> isolated Claude Code subagent -> compact result -> main Claude
```

The forbidden flow — the one this integration exists to prevent — is:

```
raw context -> main Claude -> subagent
```

The main agent never reads the packet's text, logs, images, JSON, or raw files. It works
only from **metadata**, hands a manifest **path** to an isolated subagent, and relays back
the subagent's compact summary.

---

## How the desktop app and Claude Code coordinate

There is **no daemon, no HTTP/websocket/DB server, and no network listener**. The desktop
app, the `context-drop` CLI, and every plugin instance across every Claude Code account
coordinate **only** through the shared SQLite database and the filesystem under the global
data directory.

```
Clipboard -> Packet -> Claim -> Claude session binding -> Isolated subagent -> compact result
```

- The desktop app captures clipboard material into a **Context Packet** (capture is OFF by
  default and only monitors the clipboard while Capture is ON).
- A Claude Code session **claims** that packet by running the CLI — the destination is
  decided at claim time, not at capture time.
- The isolated **`context-investigator`** subagent processes the packet and returns a
  compact result.

---

## Plugin structure

The plugin lives in the repository under `integrations/claude-code/`:

| Path | Purpose |
| --- | --- |
| `.claude-plugin/plugin.json` | Plugin manifest. |
| `skills/pull/SKILL.md` | The `/context-drop:pull` skill. |
| `skills/undo/SKILL.md` | The `/context-drop:undo` skill. |
| `skills/status/SKILL.md` | The `/context-drop:status` skill. |
| `agents/context-investigator.md` | The isolated subagent definition. |
| `alias/cd/SKILL.md` | The optional user-level short alias `/cd`. |

When installed, this is laid down as a **self-contained local marketplace** under:

```
<config-dir>/plugins/marketplaces/context-drop/
```

Installation is **idempotent and additive**: it never overwrites unrelated settings and
never wholesale-overwrites `settings.json`.

---

## Installing the plugin

The plugin is installed by the `context-drop` CLI, which **ships with the desktop app**.
End users need **no Node.js/npm** at runtime (pnpm is dev-only).

```bash
context-drop install-claude \
  [--config-dir <path>] \
  [--from <path>] \
  [--short-alias] \
  [--force]
```

- `--config-dir <path>` — install into a single, explicit config root.
- `--from <path>` — source location to install from.
- `--short-alias` — also install the user-level `/cd` alias (see below).
- `--force` — allow overwriting an existing `/cd` alias.

Global option: `--data-dir <path>` selects the Context Drop data directory.

### Enable it in a Claude Code session

After `install-claude` runs, it prints a marketplace path. Enable the plugin from within a
Claude Code session with:

```
/plugin marketplace add <printed marketplace path>
/plugin install context-drop@context-drop
```

---

## The three skills

### `/context-drop:pull`

Routes the currently claimable packet to **this** Claude Code session and delegates its
processing to the isolated subagent. This is the primary command.

Behavior — the main agent **MUST NOT** read the packet's text, logs, images, JSON, or raw
files. Instead it:

1. Runs `context-drop claim --json` and uses **only** the returned metadata.
2. Infers a **task mode** from the user's argument (see [Task modes](#task-modes)).
3. Delegates the manifest **PATH** (not its contents) to the isolated
   `context-investigator` subagent.
4. Relays the subagent's **compact result** back into the conversation.
5. Runs `context-drop consume <packetId>` to mark the packet done.

### `/context-drop:undo`

Undoes **only the most recent eligible claim** for the **current** session. Eligibility
means: same session id, claimed within roughly the last 5 minutes, not fully `CONSUMED`,
and safe to return to `READY`. Undo returns the packet to `READY`.

> **Undo is routing-only.** It never rolls back source-code changes that a `FIX` run may
> already have made to your repository. It only un-routes the packet so it can be claimed
> again.

### `/context-drop:status`

Reports the current Context Drop state (what packet, if any, is ready to be claimed and its
metadata) so you can see what `pull` would pick up.

---

## The `context-investigator` subagent

`context-investigator` is the **isolated subagent** that actually reads the packet. Because
it runs in its own context, the raw material (screenshots, console output, server/debug
logs, JSON responses, stack traces, copied text and images, Finder/Explorer files, etc.)
is read **only** by the subagent — never by your main conversation.

The main agent gives the subagent a **manifest path**, and the subagent returns a compact,
structured result in this format:

- **Status**
- **Confidence** — one of `CONFIRMED`, `HIGH`, `PROBABLE`, `UNKNOWN`
- **Root cause** or **Findings**
- **Evidence references** — packet item path, source `file:line`, test names
- **Changed files**
- **Tests / Verification**
- **Remaining unknowns**

The subagent **never** returns complete logs, large raw text, image binary, or large JSON.

### Task modes

`/context-drop:pull` infers the task mode from the user's argument. The default is
`ANALYZE`.

| Mode | Behavior | Example prompts |
| --- | --- | --- |
| `ANALYZE` *(default)* | Read-only investigation. | "原因だけ調べて", "investigate" |
| `FIX` | Investigate → modify repo → test → report. | "原因を調べて直して" |
| `REVIEW` | Read-only comparison unless explicitly authorized to change anything. | "このUIをレビューして" |

---

## Session routing

Routing is the most important part of the model. Context Drop follows a **"capture first,
route later"** design:

- At capture time a packet's destination is **`NONE`**.
- The destination is decided **only** when `/context-drop:pull` runs from the target Claude
  Code session.

### Routing identity

The routing identity stored for a claim is:

```
session_id + cwd + project_root
```

- **`session_id`** comes from `CLAUDE_CODE_SESSION_ID`, which is verified to be set inside
  Claude Code sessions. This is what distinguishes the **same repository open in multiple
  tabs**.
- **`project_root`** is `git rev-parse --show-toplevel` when inside a git repository,
  otherwise the canonicalized `cwd`. A **monorepo subdirectory is NOT a different project**
  unless its git root differs.

### What routing never uses

Routing is **never** decided by:

- the most-recently-opened project,
- the foreground app,
- the window title,
- the Anthropic account,
- or the desktop UI selection.

The **Anthropic account id is never used for routing.**

### Claim priority

When a session claims, the CLI selects a packet in this order:

1. The current `DRAFT` packet with `items > 0`.
2. Otherwise, the newest `READY` packet.
3. Otherwise, `NO_PACKET`.

Claiming a `DRAFT` **atomically ends capture** (`DRAFT -> CLAIMED`); any later append to
that packet is refused. Claims are atomic (SQLite `BEGIN IMMEDIATE` + conditional
`UPDATE`), so **exactly one session can ever claim a given packet**, even under concurrent
claims.

Context Drop supports **20+ parallel sessions and 10+ Claude accounts**.

---

## Multiple accounts and `CLAUDE_CONFIG_DIR`

Multiple config roots are **supported and required** — this integration is built for users
running many Claude accounts.

### How `install-claude` behaves with multiple config roots

- By default, `install-claude` installs into **`CLAUDE_CONFIG_DIR`** (which may be
  **comma- or semicolon-separated**) **plus `~/.claude`**.
- Passing a single `--config-dir <path>` installs into just that one root.

### One global packet store — no per-account duplication

All plugin instances, across **all** config roots, talk to the **same global Context Drop
packet store**. Packet data is **never duplicated per account**.

This is why routing does not need — and never uses — account identity: the packet lives in
one place, and the session that runs `/context-drop:pull` is the one that claims it.

---

## The `/cd` short alias

`/cd` is an **opt-in**, user-level short alias for the plugin's skills.

- Install it by adding `--short-alias` to `context-drop install-claude`.
- Installation **refuses to overwrite an existing `/cd`** unless you also pass `--force`.

---

## Settings: Claude Code Integrations list

The desktop app's Settings popover shows a **Claude Code Integrations** list. It displays
each config path together with whether the plugin is installed there. It **never** shows
Anthropic account identity.

| Config path | Status |
| --- | --- |
| `~/.claude` | Installed |
| `~/.claude-work` | Installed |
| `~/.claude-client2` | Not installed |

From the same Settings area you can also **Install integration** and **Install `/cd`
alias**.

---

## What the CLI returns to the plugin (metadata only)

`/context-drop:pull` calls `context-drop claim --json`. A successful claim returns
**metadata only** — never raw content, logs, JSON contents, image bytes, or file contents:

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

When there is nothing to claim, the CLI returns `NO_PACKET` with this exact message:

```
No Context Drop packet is ready. Start Capture and copy the materials first.
```

Relevant CLI exit codes:

| Code | Meaning |
| --- | --- |
| `0` | OK |
| `1` | Error |
| `3` | `NO_PACKET` |
| `4` | `MISSING_SESSION_ID` |
| `5` | `NOTHING_TO_UNDO` |

The CLI commands the plugin relies on are `claim`, `consume`, `undo`, `status`, and
`install-claude`; see the CLI reference for full details.

---

## Privacy note specific to the integration

Context Drop itself performs **no telemetry, no analytics, no cloud backend, and no network
transmission of clipboard content**. Packet data stays local.

The **only** content that leaves your machine is when **Claude Code itself** sends packet
material to its configured model provider as the `context-investigator` subagent reads it.
That is Claude Code's normal behavior — **not Context Drop transmitting your data**.
