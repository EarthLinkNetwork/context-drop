---
name: status
description: Show the current Context Drop status — whether capture is in progress, how many items are in the current draft packet, how many packets are READY, and the most recent dispatch. Use when the user runs /context-drop:status (or after /cd) to check what is captured before sending. Metadata only; never reads packet contents.
---

# Context Drop — Status

Report Context Drop's current state using metadata only.

## How

Resolve the `context-drop` binary first (prefer the canonical managed copy over
PATH): use `$CONTEXT_DROP_BIN` if set, else the canonical path
(`"$CONTEXT_DROP_DATA_DIR/bin/context-drop"` if set, else macOS
`"$HOME/Library/Application Support/com.contextdrop.app/bin/context-drop"`,
Linux `"${XDG_DATA_HOME:-$HOME/.local/share}/com.contextdrop.app/bin/context-drop"`,
Windows `<APPDATA>\com.contextdrop.app\bin\context-drop.exe`), else `context-drop`
on `PATH`. Then run:

```bash
context-drop status --json
```

Parse and summarize the JSON for the user:

- `currentDraft`: whether a DRAFT is being captured and how many items it holds (`itemCount`).
- `readyCount`: packets stopped but not yet dispatched.
- `lastDispatch`: the most recent claim — `projectName`, `itemCount`, `state`, `claimedAt`.
- `totalPackets`: total packets in the local store.

Keep the summary short. Do **not** read or display any packet content — only these counts and identifiers.

If the user seems to expect a packet but `currentDraft` is null and `readyCount` is 0, remind them: start Capture (global shortcut) and copy the materials first.
