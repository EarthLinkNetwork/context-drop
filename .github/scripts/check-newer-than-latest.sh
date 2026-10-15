#!/usr/bin/env bash
# Refuse to publish <tag> unless it is newer than the repository's current
# latest release (what installed apps update to). Fails closed: only a 404
# ("no release yet") counts as "no latest"; any other error from GitHub
# (network, auth, server) stops the release.
#
#   check-newer-than-latest.sh <tag vX.Y.Z> <owner/repo>
set -euo pipefail
tag=$1 repo=$2
err=$(mktemp)
trap 'rm -f "$err"' EXIT

if current=$(gh api "repos/$repo/releases/latest" --jq .tag_name 2>"$err"); then
  :
elif grep -q "HTTP 404" "$err"; then
  echo "no latest release yet; $tag will be the first"
  exit 0
else
  echo "::error::could not read the latest release; not publishing: $(cat "$err")" >&2
  exit 1
fi
[ -n "$current" ] || { echo "::error::the latest release has no tag name; not publishing" >&2; exit 1; }

newest=$(printf '%s\n%s\n' "${current#v}" "${tag#v}" | sort -V | tail -n 1)
if [ "$current" = "$tag" ] || [ "$newest" != "${tag#v}" ]; then
  echo "::error::$tag is not newer than the latest release $current" >&2
  exit 1
fi
echo "$tag is newer than the latest release $current"
