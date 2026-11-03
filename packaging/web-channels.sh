#!/usr/bin/env bash
# Puts the three web app channels into the Pages site (usage: web-channels.sh <site> <edge-build>).
#
#   <site>/app/       stable: the web build attached to the latest release that is not a pre-release
#   <site>/app-rc/    release candidate: the web build of the newest published v*-rc.* pre-release
#   <site>/app-edge/  edge: <edge-build>, built from main in this run
#
# Stable and the release candidate are not rebuilt: the release workflow already attaches
# solidcraft-web-<version>.zip (packaging/web/package.sh, built by that tag's own workflow), and
# a deploy takes that zip as it is. Rebuilding old tags here would need each tag's own build steps
# (they changed: the kernel moved from SMLib to solidcore). The channels are not nested, because
# the app's service worker controls everything under its own directory (apps/solidcraft-web/sw.js).
#
# No stable release or no web zip on it fails the deploy: Pages replaces the whole site, and an
# empty /app/ would take the web app away. A missing, outdated or too-old release candidate
# leaves a small page at /app-rc/ instead. Needs gh and GH_TOKEN; GITHUB_REPOSITORY names the repository.
set -euo pipefail

site="${1:?site directory required}"
edge="${2:?edge build directory required}"
repo="${GITHUB_REPOSITORY:?}"

# Version order, newest last: 0.31.0-rc.2 sorts after 0.31.0-rc.1 and before 0.31.0.
version_sort() { sed 's/-/~/' | sort -V | sed 's/~/-/'; }

# Unpack the web zip of release $1 into directory $2. Fails when the release has none.
fetch_build() {
  local tag="$1" dest="$2" tmp
  tmp="$(mktemp -d)"
  gh release download "$tag" --repo "$repo" --pattern "solidcraft-web-*.zip" --dir "$tmp"
  gh release download "$tag" --repo "$repo" --pattern SHA256SUMS.txt --dir "$tmp"
  local zip
  zip="$(find "$tmp" -name 'solidcraft-web-*.zip' | head -n1)"
  test -n "$zip"
  (cd "$tmp" && awk -v file="$(basename "$zip")" '$2 == file || $2 == "*" file { print; found=1 } END { exit !found }' SHA256SUMS.txt | sha256sum --check -)
  unzip -q "$zip" -d "$tmp/unpacked"
  mkdir -p "$dest"
  cp -R "$tmp"/unpacked/*/. "$dest"/
  test -s "$dest/index.html"
  test -s "$dest/sw.js"
  rm -rf "$tmp"
}

# A page for a channel that has no build to offer.
placeholder() {
  local dir="$1" why="$2"
  mkdir -p "$dir"
  cat >"$dir/index.html" <<HTML
<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="robots" content="noindex"><title>SolidCraft release candidate</title></head><body style="font-family:sans-serif;max-width:40em;margin:3em auto;padding:0 1em"><h1>No release candidate right now</h1><p>$why</p><p><a href="../app/">Open SolidCraft (the latest release)</a> or <a href="../app-edge/">the edge build</a>.</p></body></html>
HTML
}

stable="$(gh api "repos/$repo/releases?per_page=100" \
  --jq '[.[] | select(.draft == false and .prerelease == false)][0].tag_name // empty')"
if [ -z "$stable" ]; then
  echo "error: no published release, so there is no stable web app to deploy" >&2
  exit 1
fi
echo "stable: $stable"
fetch_build "$stable" "$site/app"

# Edge: the build of this run.
mkdir -p "$site/app-edge"
cp -R "$edge"/. "$site/app-edge"/

# Release candidate: the newest published pre-release v*-rc.N, if it is newer than stable and is
# built with per-channel browser storage (apps/solidcraft-web/src/channel.rs): a build without it
# would share stable's saved designs.
rc="$(gh api "repos/$repo/releases?per_page=100" \
  --jq '.[] | select(.draft == false and .prerelease == true) | .tag_name' \
  | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+-rc\.[0-9]+$' | version_sort | tail -n1 || true)"
if [ -z "$rc" ]; then
  placeholder "$site/app-rc" "There is no release candidate at the moment. One is published shortly before each release."
elif [ "$(printf '%s\n%s\n' "${stable#v}" "${rc#v}" | version_sort | tail -n1)" = "${stable#v}" ]; then
  echo "rc: $rc is not newer than $stable, skipped"
  placeholder "$site/app-rc" "The last release candidate ($rc) is already part of the release. The next one appears before the next release."
else
  echo "rc: $rc"
  fetch_build "$rc" "$site/app-rc"
  if [ ! -f "$site/app-rc/channel-storage.txt" ]; then
    echo "rc: $rc is built without separate browser storage, skipped"
    rm -rf "$site/app-rc"
    placeholder "$site/app-rc" "The last release candidate ($rc) predates separate storage for pre-release builds, so it is not offered here. The next one is."
  fi
fi
