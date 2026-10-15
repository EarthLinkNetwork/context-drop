#!/usr/bin/env bash
# Prepare and verify the in-app update files for one release, BEFORE anything
# is published (called by release.yml; runnable by hand for testing).
#
#   prepare-updater.sh <version> <tag> <bundle .app.tar.gz> <owner/repo> <out-dir>
#
# Writes into <out-dir>:
#   Context-Drop-<tag>-macos-arm64.app.tar.gz      the bundler's tarball, renamed (never re-packed)
#   Context-Drop-<tag>-macos-arm64.app.tar.gz.sig  its signature
#   latest.json                                    what the app's updater reads
#
# and fails unless:
#   1. the signature verifies with the public key built into the app
#      (tauri.conf.json plugins.updater.pubkey), and its signed trusted comment
#      carries `version:<version>` — the app has `requireSignedVersion` on, so a
#      release without it would be refused by every installed copy;
#   2. the tarball holds `Context Drop.app` at its root, and that app passes
#      codesign and Gatekeeper (skipped with SKIP_APP_CHECK=1, for tests);
#   3. latest.json names this version (no leading v), the uploaded file's URL,
#      and exactly this signature.
#
# Needs: jq, minisign, base64, tar (and codesign/spctl unless SKIP_APP_CHECK=1).
set -euo pipefail

if [ $# -ne 5 ]; then
  echo "usage: $0 <version> <tag> <bundle .app.tar.gz> <owner/repo> <out-dir>" >&2
  exit 2
fi
version=$1 tag=$2 src=$3 repo=$4 out=$5
conf="$(cd "$(dirname "$0")/../.." && pwd)/apps/desktop/src-tauri/tauri.conf.json"

fail() {
  echo "::error::$*" >&2
  exit 1
}

[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || fail "not a version: $version"
[ -f "$src" ] || fail "missing $src"
[ -f "$src.sig" ] || fail "missing $src.sig (was TAURI_SIGNING_PRIVATE_KEY set for the build?)"

mkdir -p "$out"
name="Context-Drop-${tag}-macos-arm64.app.tar.gz"
cp "$src" "$out/$name"
# The .sig is one base64 line; drop any trailing newline.
tr -d '\n' < "$src.sig" > "$out/$name.sig"

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# 1. Signature, with the app's own public key, and the version it was signed for.
jq -er '.plugins.updater.pubkey' "$conf" | base64 -d > "$tmp/updater.pub"
base64 -d < "$out/$name.sig" > "$tmp/updater.minisig"
verified=$(minisign -V -p "$tmp/updater.pub" -m "$out/$name" -x "$tmp/updater.minisig") ||
  fail "the update signature does not verify with the public key in tauri.conf.json"
echo "$verified"
comment=$(printf '%s\n' "$verified" | sed -n 's/^Trusted comment: //p')
printf '%s' "$comment" | tr '\t' '\n' | grep -qx "version:${version}" ||
  fail "the signature's trusted comment lacks version:${version} (got: ${comment})"

# 2. The tarball: `Context Drop.app` at its root (the updater drops the first
#    path component), signed and notarized.
first=$(tar -tzf "$out/$name" | head -n 1)
case "$first" in
  "Context Drop.app/"*) ;;
  *) fail "the tarball's root is not 'Context Drop.app/' (got: $first)" ;;
esac
if [ "${SKIP_APP_CHECK:-0}" != 1 ]; then
  mkdir "$tmp/x"
  tar -xzf "$out/$name" -C "$tmp/x"
  codesign --verify --deep --strict --verbose=2 "$tmp/x/Context Drop.app"
  spctl -a -vvv -t exec "$tmp/x/Context Drop.app"
fi

# 3. latest.json, then check it says exactly what was verified above.
url="https://github.com/${repo}/releases/download/${tag}/${name}"
jq -n \
  --arg version "$version" \
  --arg notes "See https://github.com/${repo}/releases/tag/${tag}" \
  --arg date "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
  --arg url "$url" \
  --rawfile signature "$out/$name.sig" \
  '{version: $version, notes: $notes, pub_date: $date,
    platforms: {"darwin-aarch64": {signature: $signature, url: $url}}}' \
  > "$out/latest.json"
jq -e \
  --arg version "$version" \
  --arg url "$url" \
  --rawfile signature "$out/$name.sig" \
  '.version == $version
   and (.pub_date | test("^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$"))
   and .platforms["darwin-aarch64"].url == $url
   and .platforms["darwin-aarch64"].signature == $signature' \
  "$out/latest.json" > /dev/null || fail "latest.json does not match the verified files"

echo "prepared: $out/$name, $out/$name.sig, $out/latest.json"
