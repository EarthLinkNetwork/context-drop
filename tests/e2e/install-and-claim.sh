#!/usr/bin/env bash
# Packaged-install acceptance test (design D-verification): install the CLI +
# plugin the way `install-claude` does, then run the full skill flow using ONLY
# the canonical `<data-dir>/bin` copy — i.e. with the CLI NOT on PATH. Proves
# the flow is usable after a plain install, and that claim output is metadata
# only. Requires: bash, cargo, python3. Uses temp dirs; touches nothing real.
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$repo_root"

echo "== build CLI (with dev-tools for the __seed test helper) =="
cargo build -q -p context-drop-cli --features dev-tools
BIN="$repo_root/target/debug/context-drop"

DATA="$(mktemp -d)"; CFG="$(mktemp -d)"; LOCALBIN="$(mktemp -d)"
cleanup() { rm -rf "$DATA" "$CFG" "$LOCALBIN"; }
trap cleanup EXIT
export CONTEXT_DROP_DATA_DIR="$DATA"
export CONTEXT_DROP_LOCAL_BIN="$LOCALBIN"

echo "== install-claude (plugin + CLI + stage + alias) =="
"$BIN" install-claude --config-dir "$CFG" --from integrations/claude-code --short-alias >/dev/null
[ -f "$CFG/plugins/marketplaces/context-drop/plugins/context-drop/.claude-plugin/plugin.json" ] || { echo "FAIL: plugin not installed"; exit 1; }
[ -x "$DATA/bin/context-drop" ] || { echo "FAIL: CLI not installed"; exit 1; }
[ -f "$DATA/claude-code/.claude-plugin/plugin.json" ] || { echo "FAIL: plugin not staged"; exit 1; }
[ -f "$CFG/skills/cd/SKILL.md" ] || { echo "FAIL: /cd alias missing"; exit 1; }
echo "  ok: plugin + CLI + staged plugin + /cd installed"

# Prove discovery works when the CLI is NOT on PATH (the common case): the skill's
# Step 0 falls back to the canonical <data-dir>/bin copy. Assert it is not on PATH
# and no override is set, then use the canonical fallback path.
unset CONTEXT_DROP_BIN || true
if command -v context-drop >/dev/null 2>&1; then
  echo "FAIL: context-drop is unexpectedly on PATH; test would not exercise the fallback"; exit 1
fi
echo "== CLI is not on PATH; resolving via the canonical fallback =="
CLI="$CONTEXT_DROP_DATA_DIR/bin/context-drop"
[ -x "$CLI" ] || { echo "FAIL: canonical CLI fallback missing at $CLI"; exit 1; }

echo "== seed a packet (simulate capture) with a secret =="
printf 'SECRET-LOG-abc123-do-not-leak\n' | "$CLI" __seed >/dev/null

echo "== claim as a Claude session (metadata only) =="
export CLAUDE_CODE_SESSION_ID=e2e-session
OUT="$("$CLI" claim --json)"
case "$OUT" in *SECRET-LOG*) echo "FAIL: raw content leaked in claim output"; exit 1;; esac
PID="$(printf '%s' "$OUT" | python3 -c 'import sys,json;print(json.load(sys.stdin)["packetId"])')"
CLAIMID="$(printf '%s' "$OUT" | python3 -c 'import sys,json;print(json.load(sys.stdin)["claimId"])')"
MAN="$(printf '%s' "$OUT" | python3 -c 'import sys,json;print(json.load(sys.stdin)["manifestPath"])')"
[ -n "$PID" ] && [ -n "$CLAIMID" ] || { echo "FAIL: missing packetId/claimId"; exit 1; }
grep -q SECRET-LOG "$MAN" && { echo "FAIL: manifest leaked secret"; exit 1; } || true
echo "  ok: metadata-only claim; packet=$PID"

echo "== mark PROCESSING (protects from TTL cleanup during investigation) =="
"$CLI" processing "$PID" --claim-id "$CLAIMID" >/dev/null
echo "  ok: marked processing"

echo "== a wrong claim id is refused (ownership) =="
if "$CLI" consume "$PID" --claim-id wrong-id >/dev/null 2>&1; then
  echo "FAIL: consume accepted a wrong claim id"; exit 1
fi
echo "  ok: wrong claim id refused"

echo "== consume with the correct claim id =="
"$CLI" consume "$PID" --claim-id "$CLAIMID" >/dev/null
echo "  ok: consumed"

echo "E2E OK"
