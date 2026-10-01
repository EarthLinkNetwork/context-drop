---
name: cd
description: Short alias for /context-drop:pull. Route a captured Context Drop packet (clipboard screenshots, logs, JSON, files) into THIS Claude Code session and investigate it in an isolated subagent, keeping raw context out of the main conversation. Use when the user types /cd with an instruction like "investigate this bug" or "原因を調べて直して".
---

# /cd — Context Drop short alias

This is a convenience alias for the official **`/context-drop:pull`** plugin skill.

Follow the exact same procedure as `/context-drop:pull`, including its **Step 0**
resolution of the `context-drop` binary (`$CONTEXT_DROP_BIN` → the canonical
`<data-dir>/bin/context-drop` per OS → `PATH`) and its **Step 3** PROCESSING mark:

1. Run `context-drop claim --json` to atomically bind the highest-priority packet to **this** Claude Code session (session id + cwd + git project root). Use metadata only.
2. **Never** read raw packet content (text, logs, images, JSON, files) into this main conversation.
3. Infer the task mode (ANALYZE / FIX / REVIEW) from the user's instruction; default to ANALYZE.
4. Launch the `context-investigator` subagent (Task tool) with the manifest path, task mode, project root, and the user's verbatim instruction. It reads the items in isolation.
5. Relay the subagent's compact result, then run `context-drop consume <packetId> --claim-id <claimId>` (using the `claimId` from the claim JSON in step 1).

Handle CLI errors exactly as `/context-drop:pull` does:

- `NO_PACKET` (exit 3): "No Context Drop packet is ready. Start Capture and copy the materials first."
- `MISSING_SESSION_ID` (exit 4): routing refused; run from inside a Claude Code session.

The official, namespaced command is `/context-drop:pull`; this alias exists only for convenience and does exactly the same thing.
