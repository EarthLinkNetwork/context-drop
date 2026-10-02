#!/bin/sh
# Context Drop — deterministic claim, run by the skill loader (`!` injection)
# the moment /context-drop:pull or /cd expands, BEFORE the model sees the
# user's instruction. This guarantees the packet is claimed (and leaves the
# desktop app's Current Packet) even when the model would otherwise treat the
# instruction as a standalone task.
#
# Usage: sh claim.sh <claude-session-id>
# Prints metadata only (never packet content):
#   CONTEXT_DROP_BIN=<resolved binary>
#   CLAIM_EXIT=<claim exit code>
#   <claim --json output>
#   PROCESSING_EXIT=<processing exit code>   (only when the claim succeeded)

session_id="$1"

# Step 0 — resolve the binary: $CONTEXT_DROP_BIN → canonical managed copy → PATH.
bin="${CONTEXT_DROP_BIN:-}"
if [ -z "$bin" ]; then
  for c in \
    "${CONTEXT_DROP_DATA_DIR:+$CONTEXT_DROP_DATA_DIR/bin/context-drop}" \
    "${CONTEXT_DROP_DATA_DIR:+$CONTEXT_DROP_DATA_DIR/bin/context-drop.exe}" \
    "$HOME/Library/Application Support/com.contextdrop.app/bin/context-drop" \
    "${XDG_DATA_HOME:-$HOME/.local/share}/com.contextdrop.app/bin/context-drop" \
    "${APPDATA:+$APPDATA/com.contextdrop.app/bin/context-drop.exe}"; do
    if [ -n "$c" ] && [ -x "$c" ]; then
      bin="$c"
      break
    fi
  done
fi
[ -n "$bin" ] || bin="$(command -v context-drop 2>/dev/null)"
if [ -z "$bin" ]; then
  echo "CONTEXT_DROP_BIN="
  echo "CLAIM_EXIT=127"
  echo '{"ok":false,"error":"NOT_INSTALLED","message":"context-drop CLI not found"}'
  exit 0
fi
printf 'CONTEXT_DROP_BIN=%s\n' "$bin"

if [ -n "$session_id" ]; then
  out="$("$bin" claim --json --session-id "$session_id" 2>&1)"
else
  out="$("$bin" claim --json 2>&1)"
fi
code=$?
echo "CLAIM_EXIT=$code"
printf '%s\n' "$out"

# Mark PROCESSING right away so the claimed packet is protected from TTL
# cleanup while the subagent investigates.
if [ "$code" -eq 0 ]; then
  packet_id="$(printf '%s' "$out" | sed -n 's/.*"packetId"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
  claim_id="$(printf '%s' "$out" | sed -n 's/.*"claimId"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
  if [ -n "$packet_id" ] && [ -n "$claim_id" ]; then
    if [ -n "$session_id" ]; then
      "$bin" processing "$packet_id" --claim-id "$claim_id" --session-id "$session_id" >/dev/null 2>&1
    else
      "$bin" processing "$packet_id" --claim-id "$claim_id" >/dev/null 2>&1
    fi
    echo "PROCESSING_EXIT=$?"
  fi
fi
exit 0
