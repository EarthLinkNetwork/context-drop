#!/usr/bin/env bash
# Download counts per release, from GitHub's public per-file counters ($0).
#
#   .github/scripts/release-stats.sh [owner/repo]
#
# Columns:
#   dmg      downloads of the DMG (new installs, or reinstalls by hand)
#   update   downloads of the .app.tar.gz (installs done by the in-app updater)
#   checks   downloads of latest.json (update checks; installed apps with
#            automatic checks on fetch it at launch and about once a day, so
#            this is a rough "in use" signal, not a count of people)
# Counts include repeat downloads and bots. Only the newest release's
# latest.json is fetched by apps, so older releases stop gaining checks.
set -euo pipefail
repo=${1:-EarthLinkNetwork/context-drop}
printf '%-10s %8s %8s %8s  %s\n' tag dmg update checks published
gh api --paginate "repos/$repo/releases" --jq '.[] | select(.draft | not) |
  [ .tag_name,
    ([.assets[] | select(.name | endswith(".dmg") or endswith(".zip")) | .download_count] | add // 0),
    ([.assets[] | select(.name | endswith(".app.tar.gz")) | .download_count] | add // 0),
    ([.assets[] | select(.name == "latest.json") | .download_count] | add // 0),
    (.published_at // "" | .[0:10]) ] | @tsv' |
  while IFS=$'\t' read -r tag dmg update checks published; do
    printf '%-10s %8s %8s %8s  %s\n' "$tag" "$dmg" "$update" "$checks" "$published"
  done
