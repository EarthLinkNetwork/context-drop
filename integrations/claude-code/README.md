# Context Drop — Claude Code integration

This directory is the Context Drop Claude Code **plugin**. It provides:

- `skills/pull` → `/context-drop:pull` — claim a packet for the current session and process it in an isolated subagent.
- `skills/undo` → `/context-drop:undo` — undo the most recent dispatch (routing only).
- `skills/status` → `/context-drop:status` — show capture/dispatch status (metadata only).
- `agents/context-investigator.md` — the isolated subagent that reads raw packet material.
- `alias/cd/SKILL.md` — an optional user-level `/cd` short alias (installed separately).

## Installing

The Context Drop desktop app ships the `context-drop` CLI, which installs this plugin:

```bash
# Install into the default config root(s) (CLAUDE_CONFIG_DIR + ~/.claude):
context-drop install-claude

# Install into a specific config root (repeat per Claude account):
context-drop install-claude --config-dir ~/.claude-work

# Also install the optional /cd short alias:
context-drop install-claude --short-alias
```

Then enable it in a Claude Code session started with that config dir:

```
/plugin marketplace add <printed marketplace path>
/plugin install context-drop@context-drop
```

The Context Drop packet store is **global per OS user**; installing the plugin into
multiple config roots never duplicates packet data. See `docs/claude-code.md`.

## The one hard rule

The **main agent never reads raw packet content**. It passes the manifest *path* to the
`context-investigator` subagent, which reads the items in its own isolated context and
returns only a compact, evidence-based result.
