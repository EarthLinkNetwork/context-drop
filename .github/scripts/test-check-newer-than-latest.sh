#!/usr/bin/env bash
# Tests for check-newer-than-latest.sh with a fake `gh` on PATH (no network).
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
bin=$(mktemp -d)
trap 'rm -rf "$bin"' EXIT
fails=0

# FAKE_GH=<tag> prints that tag; FAKE_GH=404 / FAKE_GH=net fail like gh does.
cat > "$bin/gh" <<'GH'
#!/usr/bin/env bash
case "$FAKE_GH" in
  404) echo '{"message":"Not Found","status":"404"}gh: Not Found (HTTP 404)' >&2; exit 1 ;;
  net) echo 'error connecting to api.github.com' >&2; exit 1 ;;
  500) echo 'gh: Server Error (HTTP 500)' >&2; exit 1 ;;
  *) echo "$FAKE_GH" ;;
esac
GH
chmod +x "$bin/gh"

expect() { # expect <ok|fail> <tag> <FAKE_GH>
  local want=$1 tag=$2 fake=$3 got
  if FAKE_GH=$fake PATH="$bin:$PATH" bash "$here/check-newer-than-latest.sh" "$tag" o/r >/dev/null 2>&1; then got=ok; else got=fail; fi
  if [ "$got" = "$want" ]; then echo "ok   $tag vs $fake -> $got"; else echo "FAIL $tag vs $fake -> $got (want $want)"; fails=$((fails + 1)); fi
}

expect ok   v0.1.10 v0.1.9    # newer (numeric, not string order)
expect ok   v1.0.0  v0.9.9
expect fail v0.1.9  v0.1.10   # older: would move latest backwards
expect fail v0.1.10 v0.1.10   # same
expect ok   v0.1.0  404       # no release yet
expect fail v0.1.10 net       # network error: fail closed
expect fail v0.1.10 500       # server error: fail closed
expect fail v0.1.10 ""        # empty tag name

[ "$fails" -eq 0 ] || { echo "$fails failed"; exit 1; }
echo "all passed"
