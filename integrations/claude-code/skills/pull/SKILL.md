---
name: pull
description: Route a captured Context Drop packet into THIS Claude Code session and investigate it in an isolated subagent. Use when the user runs /context-drop:pull (or /cd) with an instruction like "investigate this", "原因を調べて直して", or "このUIをレビューして", after collecting clipboard items (screenshots, logs, JSON, files) in the Context Drop desktop app. The raw packet contents (text, logs, images, JSON, files) must NEVER be read into this main conversation — only packet metadata and the subagent's compact result may enter the main context.
---

# Context Drop — Pull

You are routing a **Context Drop packet** into the current Claude Code session and delegating the raw material to an **isolated subagent**. The user has already captured clipboard items (images, logs, JSON, files) into a packet.

## Hard rule (do not violate)

The **main agent (you) MUST NOT read** any packet content:

- packet text, logs, JSON, images, or copied files
- the item files under the packet's `items/` directory

Only these may enter the main context:

- packet **metadata** returned by the CLI (ids, paths, counts, routing info)
- the **compact result** returned by the `context-investigator` subagent

Never `cat`, `Read`, open, or paste the manifest's item files or the manifest body into this conversation. You pass the manifest **path** to the subagent; the subagent reads the content in its own isolated context.

## Step 0 — Resolve the `context-drop` binary (do this once)

Resolve the CLI once and use that exact path for **every** command below
(`processing`, `claim`, `consume`). Resolution order — prefer the **canonical
managed copy over `PATH`**, so a stale/older `context-drop` on `PATH` can never
shadow the up-to-date one, and every Claude account/config dir resolves to the
same global CLI:

1. `$CONTEXT_DROP_BIN` if set.
2. The canonical install path (the desktop app / `install-claude` refreshes it
   here; it is global per OS user and shared by all `CLAUDE_CONFIG_DIR`s). The
   binary is `context-drop` (`context-drop.exe` on Windows). If
   `$CONTEXT_DROP_DATA_DIR` is set, use `<that>/bin/context-drop[.exe]`; else per OS:
   - macOS: `"$HOME/Library/Application Support/com.contextdrop.app/bin/context-drop"`
   - Linux: `"${XDG_DATA_HOME:-$HOME/.local/share}/com.contextdrop.app/bin/context-drop"`
   - Windows: `<APPDATA>\com.contextdrop.app\bin\context-drop.exe` — use your shell's
     env syntax for APPDATA (`$APPDATA` in Git Bash, `$env:APPDATA` in PowerShell).
3. Only if that canonical copy does not exist, fall back to `context-drop` on `PATH`.

If none of these resolve, tell the user to install Context Drop (desktop app, then
"Install Claude Code integration" — or run `context-drop install-claude`) and stop.
Below, `context-drop` means the resolved binary.

## Step 1 — Claim the packet for THIS session

Run the companion CLI (the resolved binary from Step 0):

```bash
context-drop claim --json
```

This atomically binds the highest-priority packet (the current DRAFT if it has items, else the most recent READY) to **this** Claude Code session, using `CLAUDE_CODE_SESSION_ID` + cwd + git project root. Capture ends automatically. The user does **not** choose a project, account, session, directory, packet id, or filename.

Parse the JSON from stdout. On success it looks like:

```json
{ "ok": true, "packetId": "…", "claimId": "…", "manifestPath": "…", "itemCount": 7,
  "sessionId": "…", "projectRoot": "…", "projectName": "…" }
```

Keep the `packetId` **and** `claimId` — you pass both to `consume` in Step 4 so a
re-claim of the same packet is never consumed by mistake.

Handle non-success by exit code / `error`:

- `NO_PACKET` (exit 3): tell the user, verbatim: **"No Context Drop packet is ready. Start Capture and copy the materials first."** Stop.
- `MISSING_SESSION_ID` (exit 4): report that no Claude Code session id was available, so routing was refused (Context Drop never routes by project alone). Suggest running from inside a Claude Code session. Stop.
- any other error: report the message and stop.

Do not print raw stdout if it might contain content — but note the CLI is designed to emit metadata only.

## Step 2 — Decide the task mode from the user's argument

Infer the mode from the user's instruction (the text after the command):

- **ANALYZE** — read-only investigation. Triggers: "原因だけ調べて", "調査してください", "まだ修正しないで", "investigate", "what's causing…". The subagent must NOT edit source code.
- **FIX** — investigate, then modify the repository and run tests. Triggers: "原因を調べて直して", "修正までして", "fix it", "直して", "必要ならテストも追加して".
- **REVIEW** — read-only comparison/review unless the user explicitly authorizes edits. Triggers: "レビューして", "画像と実装を比較して", "仕様との差を確認して", "review this UI".

When ambiguous, default to **ANALYZE** (the safest, read-only mode).

## Step 3 — Mark the packet as PROCESSING

Before delegating, mark the packet PROCESSING so it is protected from TTL
cleanup while the subagent investigates (pass the `claimId` from Step 1):

```bash
context-drop processing <packetId> --claim-id <claimId>
```

## Step 4 — Delegate to the isolated subagent

Use the **Task** tool to launch the `context-investigator` agent (subagent_type: `context-investigator`). Pass it a prompt containing:

- the **manifest path** from Step 1 (e.g. `manifestPath`)
- the **task mode** (ANALYZE / FIX / REVIEW)
- the user's **original instruction** (verbatim)
- the **projectRoot** so it can read repository source if needed

Example prompt to the subagent:

> Task mode: ANALYZE. Context Drop manifest: `<manifestPath>`. Project root: `<projectRoot>`.
> User request: "<verbatim user instruction>".
> Read the manifest and the packet items yourself. Investigate. Return a compact, evidence-based result only.

Do **not** read the manifest or items yourself before or after delegating.

## Step 5 — Relay the compact result and finish

Relay the subagent's compact result to the user (Status / Confidence / Findings / Evidence references / Changed files / Verification / Remaining unknowns). Do not expand it with raw quotes from the packet.

Then mark the packet consumed so it is not re-processed (pass the `claimId` from
Step 1 so only this exact claim is consumed):

```bash
context-drop consume <packetId> --claim-id <claimId>
```

If the user wants to undo the routing (within ~5 minutes), point them at `/context-drop:undo` (undo affects routing only, never code changes already made).
