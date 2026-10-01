---
name: undo
description: Undo the most recent Context Drop dispatch for THIS Claude Code session, returning the packet to READY. Use when the user runs /context-drop:undo (or after /cd) because they routed a packet to the wrong session or want to re-route it. Undo affects packet routing ONLY — it never rolls back any source-code changes a previous FIX run already made.
---

# Context Drop — Undo

Undo the most recent **eligible** claim for the current Claude Code session and return the packet to `READY` so it can be claimed again (from this or another session).

## What "eligible" means

- Same session id (`CLAUDE_CODE_SESSION_ID`).
- Claimed recently (within ~5 minutes).
- Not already fully CONSUMED.
- Safe to return to READY.

## How

Resolve the `context-drop` binary first (prefer the canonical managed copy over
PATH): use `$CONTEXT_DROP_BIN` if set, else the canonical path
(`"$CONTEXT_DROP_DATA_DIR/bin/context-drop"` if set, else macOS
`"$HOME/Library/Application Support/com.contextdrop.app/bin/context-drop"`,
Linux `"${XDG_DATA_HOME:-$HOME/.local/share}/com.contextdrop.app/bin/context-drop"`,
Windows `<APPDATA>\com.contextdrop.app\bin\context-drop.exe`), else `context-drop`
on `PATH`. Then run:

```bash
context-drop undo --json
```

Parse the JSON:

- On success (`{ "ok": true, "packetId": "…", "projectName": "…", "itemCount": … }`), tell the user the dispatch was undone and the packet returned to READY.
- `NOTHING_TO_UNDO` (exit 5): tell the user there is no recent claim in this session eligible to undo.
- `MISSING_SESSION_ID` (exit 4): report that no session id was available.

## Important boundary (state this to the user)

**Undo means routing only.** If a FIX run already modified files or ran tests, those changes are **not** reverted by undo. Use normal version control (e.g. `git`) to review or revert code changes.

Do not read packet contents into the main conversation at any point.
