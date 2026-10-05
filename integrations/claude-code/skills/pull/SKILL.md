---
name: pull
description: Route a captured Context Drop packet into THIS Claude Code session and investigate it in an isolated subagent. Use when the user runs /context-drop:pull (or /cd) with an instruction like "investigate this", "原因を調べて直して", or "このUIをレビューして", after collecting clipboard items (screenshots, logs, JSON, files) in the Context Drop desktop app. The raw packet contents (text, logs, images, JSON, files) must NEVER be read into this main conversation — only packet metadata and the subagent's compact result may enter the main context.
allowed-tools:
  - Bash(sh "${CLAUDE_SKILL_DIR}/claim.sh" *)
  - Bash(powershell -NoProfile -ExecutionPolicy Bypass -File "${CLAUDE_SKILL_DIR}/claim.ps1" *)
  - PowerShell(powershell -NoProfile -ExecutionPolicy Bypass -File "${CLAUDE_SKILL_DIR}/claim.ps1" *)
---

# Context Drop — Pull

You are routing a **Context Drop packet** into the current Claude Code session and delegating the raw material to an **isolated subagent**. The user has already captured clipboard items (images, logs, JSON, files) into a packet.

## The user's instruction is about the PACKET

User instruction: $ARGUMENTS

This instruction is **always about the captured packet** (its screenshots, logs, JSON, files). It is **never** a standalone task. Even when it reads like a self-contained question ("これを調べて", "investigate X"), the packet is the subject — do **not** start researching, searching the web, or reading the repository on your own before the packet has been handed to the subagent in Step 3. If the instruction is empty, treat it as "investigate this". Your **first tool call is always the claim in Step 1**.

## Hard rule (do not violate)

The **main agent (you) MUST NOT read** any packet content:

- packet text, logs, JSON, images, or copied files
- the item files under the packet's `items/` directory

Only these may enter the main context:

- packet **metadata** returned by the CLI (ids, paths, counts, routing info)
- the **compact result** returned by the `context-investigator` subagent

Never `cat`, `Read`, open, or paste the manifest's item files or the manifest body into this conversation. You pass the manifest **path** to the subagent; the subagent reads the content in its own isolated context.

## Step 1 — Claim the packet FIRST (your very first tool call)

Before anything else — before reading code, searching, or thinking about the
instruction — run the bundled claim script for **this** session. It binds the
packet to this session (session id + cwd + git project root), marks it
PROCESSING, and clears the desktop app's Current Packet. Run exactly one of:

```bash
# macOS / Linux / Git Bash
sh "${CLAUDE_SKILL_DIR}/claim.sh" "${CLAUDE_SESSION_ID}"
# Windows (PowerShell, cmd, or Git Bash)
powershell -NoProfile -ExecutionPolicy Bypass -File "${CLAUDE_SKILL_DIR}/claim.ps1" "${CLAUDE_SESSION_ID}"
```

Its output is metadata only. Read it as:

- `CONTEXT_DROP_BIN=<path>` — the resolved CLI. Use this exact path for `consume` in Step 4.
- `CLAIM_EXIT=<code>` followed by the claim JSON, e.g.
  `{ "ok": true, "packetId": "…", "claimId": "…", "manifestPath": "…", "itemCount": 7, "projectRoot": "…", … }`.
  Keep `packetId`, `claimId`, `manifestPath`, `projectRoot`.

Handle non-success by exit code / `error`:

- `CLAIM_EXIT=3` / `NO_PACKET`: tell the user, verbatim: **"No Context Drop packet is ready. Start Capture and copy the materials first."** Stop.
- `CLAIM_EXIT=4` / `MISSING_SESSION_ID`: report that no Claude Code session id was available, so routing was refused (Context Drop never routes by project alone). Stop.
- `CLAIM_EXIT=127` / `NOT_INSTALLED`: tell the user to install Context Drop (desktop app, then "Install Claude Code integration" — or run `context-drop install-claude`). Stop.
- any other error: report the message and stop.
- `CLAIM_EXIT=0` but `PROCESSING_EXIT=` is nonzero: the packet is claimed but not
  protected from TTL cleanup. Report that marking it PROCESSING failed, do **not**
  delegate, and tell the user to run `/context-drop:undo` and retry. Stop.

## Step 1b — Label this dispatch with the instruction

Right after a successful claim, record the user's instruction on it so the
desktop app's dispatch history shows what this session was asked to do (and
two sessions on the same project can be told apart). Put the instruction in
single quotes, writing any `'` inside it as `'\''`; use `investigate this`
when the instruction is empty:

```bash
# macOS / Linux / Git Bash
"<CONTEXT_DROP_BIN>" note --claim-id <claimId> -- '<user instruction>'
# Windows: use PowerShell (a quoted path needs the call operator &; write ' inside as '')
& "<CONTEXT_DROP_BIN>" note --claim-id <claimId> -- '<user instruction>'
```

This is a label only (metadata, never packet content). If it fails, continue —
do not retry or stop.

## Step 2 — Decide the task mode from the user's instruction

- **ANALYZE** — read-only investigation. Triggers: "原因だけ調べて", "調査してください", "調べて", "まだ修正しないで", "investigate", "what's causing…". The subagent must NOT edit source code.
- **FIX** — investigate, then modify the repository and run tests. Triggers: "原因を調べて直して", "修正までして", "fix it", "直して", "必要ならテストも追加して".
- **REVIEW** — read-only comparison/review unless the user explicitly authorizes edits. Triggers: "レビューして", "画像と実装を比較して", "仕様との差を確認して", "review this UI".

When ambiguous, default to **ANALYZE** (the safest, read-only mode).

## Step 3 — Delegate to the isolated subagent

Use the **Agent/Task** tool to launch the `context-investigator` agent (subagent_type `context-drop:context-investigator`, or `context-investigator` if that is how it is listed). Pass it a prompt containing:

- the **manifest path** from Step 1 (`manifestPath`)
- the **task mode** (ANALYZE / FIX / REVIEW)
- the user's **original instruction** (verbatim)
- the **projectRoot** so it can read repository source if needed

Example prompt to the subagent:

> Task mode: ANALYZE. Context Drop manifest: `<manifestPath>`. Project root: `<projectRoot>`.
> User request: "<verbatim user instruction>".
> Read the manifest and the packet items yourself (including any screenshots). Investigate. Return a compact, evidence-based result only.

Do **not** read the manifest or items yourself before or after delegating.

## Step 4 — Relay the compact result and finish

Relay the subagent's compact result to the user (Status / Confidence / Findings / Evidence references / Changed files / Verification / Remaining unknowns). Do not expand it with raw quotes from the packet.

Then mark the packet consumed so it is not re-processed (pass the `claimId` from
Step 1 so only this exact claim is consumed):

```bash
# macOS / Linux / Git Bash / cmd
"<CONTEXT_DROP_BIN>" consume <packetId> --claim-id <claimId>
# PowerShell (a quoted path needs the call operator &)
& "<CONTEXT_DROP_BIN>" consume <packetId> --claim-id <claimId>
```

If the user wants to undo the routing (within ~5 minutes), point them at `/context-drop:undo` (undo affects routing only, never code changes already made).
