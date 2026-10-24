#!/usr/bin/env bash
# All Pages deployments include the book, web app, and published Flatpak repository.
# Shared by docs updates and release publication; no other workflow replaces the site.
set -euo pipefail
trap 'echo "flatpak-site.sh: line $LINENO: \"$BASH_COMMAND\" failed (exit $?)" >&2' ERR

site="${1:?site directory required}"
tag="${RELEASE_TAG:-}"
app_id=ai.storyteller.solidcraft
# Compatibility shim (docs/compatibility.md): releases before 0.8 used this app id. Its refs stay
# in the repository, marked end-of-life with a rebase to $app_id once a release under $app_id
# has been imported. Until then (the latest release is still from before the rename), the
# release's bundles carry $legacy_id and are published as they are.
legacy_id=ai.storyteller.solvecraft
if [ -z "$tag" ]; then
  tag="$(gh api "repos/$GITHUB_REPOSITORY/releases?per_page=100" \
    --jq '[.[] | select(.draft == false and .prerelease == false)][0].tag_name // empty')"
fi
# Before the first publication the docs and browser app can still deploy.
if [ -z "$tag" ]; then exit 0; fi
if ! printf '%s' "$tag" | grep -Eq '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-((0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)(\.(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*))?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$'; then
  echo "invalid release tag: $tag" >&2
  exit 1
fi
published="$(gh api "repos/$GITHUB_REPOSITORY/releases/tags/$tag" \
  --jq 'select(.draft == false and .published_at != null) | .tag_name')"
test "$published" = "$tag"
test -s "$site/index.html"
test -s "$site/app/index.html"

version="${tag#v}"
bundles="$(mktemp -d)"
gh release download "$tag" --repo "$GITHUB_REPOSITORY" --pattern SHA256SUMS.txt --dir "$bundles"
repo_dir="$site/flatpak/repo"
mkdir -p "$repo_dir"
ostree --repo="$repo_dir" init --mode=archive-z2
base_url="${PAGES_BASE_URL%/}"
# Pages reports its base URL as http:// for a custom domain; the repository is always fetched over
# HTTPS (the remote URL is stored by every client that adds it).
base_url="https://${base_url#*://}"
previous_url="$base_url/flatpak/repo"
if curl --fail --silent --location --output /dev/null "$previous_url/summary"; then
  ostree --repo="$repo_dir" remote add --no-gpg-verify previous "$previous_url"
  ostree --repo="$repo_dir" pull --mirror previous
  ostree --repo="$repo_dir" remote delete previous
fi
for arch in x86_64 aarch64; do
  asset="solidcraft-$version-linux-$arch.flatpak"
  gh release download "$tag" --repo "$GITHUB_REPOSITORY" --pattern "$asset" --dir "$bundles"
  (
    cd "$bundles"
    awk -v file="$asset" '$2 == file || $2 == "*" file { print; found=1 } END { exit !found }' \
      SHA256SUMS.txt | sha256sum --check -
  )
  validation_repo="$(mktemp -d)"
  ostree --repo="$validation_repo" init --mode=archive-z2
  flatpak build-import-bundle "$validation_repo" "$bundles/$asset"
  refs="$(ostree --repo="$validation_repo" refs)"
  if printf '%s\n' "$refs" | grep -Fxq "app/$app_id/$arch/stable"; then
    bundle_id=$app_id
  elif printf '%s\n' "$refs" | grep -Fxq "app/$legacy_id/$arch/stable"; then
    bundle_id=$legacy_id
  else
    echo "error: $asset holds neither app/$app_id/$arch/stable nor app/$legacy_id/$arch/stable; refs:" >&2
    printf '%s\n' "$refs" >&2
    exit 1
  fi
  if [ -n "${published_id:-}" ] && [ "$published_id" != "$bundle_id" ]; then
    echo "error: $asset is $bundle_id, but the other architecture's bundle is $published_id" >&2
    exit 1
  fi
  published_id=$bundle_id
  echo "$asset: app/$bundle_id/$arch/stable"
  flatpak build-import-bundle "$repo_dir" "$bundles/$asset"
done
# `flatpak update` then offers existing installs of the old id the move to the new one, and
# Flatpak moves their ~/.var/app data folder along. The mark is a new commit on the old ref (the
# same app, so a skipped update changes nothing); it is made once and mirrored by later runs.
for arch in x86_64 aarch64; do
  legacy_ref="app/$legacy_id/$arch/stable"
  ostree --repo="$repo_dir" rev-parse "$legacy_ref" >/dev/null 2>&1 || continue
  # Only rebase to an app the repository has: a release from before the rename has no new ref.
  if ! ostree --repo="$repo_dir" rev-parse "app/$app_id/$arch/stable" >/dev/null 2>&1; then
    echo "$legacy_ref: no app/$app_id/$arch/stable yet, not marked end-of-life"
    continue
  fi
  if ! ostree --repo="$repo_dir" show --print-metadata-key=ostree.endoflife-rebase "$legacy_ref" >/dev/null 2>&1; then
    flatpak build-commit-from --no-update-summary --end-of-life-rebase="$legacy_id=$app_id" \
      --src-ref="$legacy_ref" "$repo_dir" "$legacy_ref"
  fi
  test "$(ostree --repo="$repo_dir" show --print-metadata-key=ostree.endoflife-rebase "$legacy_ref")" = "'app/$app_id/$arch/stable'"
done
flatpak build-update-repo --title="SolidCraft" --comment='Official SolidCraft releases' \
  --description='Official Flatpak repository for SolidCraft' \
  --homepage="https://github.com/$GITHUB_REPOSITORY" --default-branch=stable "$repo_dir"
test -s "$repo_dir/summary"
cat >"$site/flatpak/solidcraft.flatpakrepo" <<META
[Flatpak Repo]
Title=SolidCraft
Comment=Official SolidCraft releases
Description=Official Flatpak repository for SolidCraft
Url=$base_url/flatpak/repo/
Homepage=https://github.com/$GITHUB_REPOSITORY
META
cat >"$site/flatpak/index.html" <<HTML
<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>SolidCraft Flatpak repository</title></head><body><main><h1>SolidCraft Flatpak repository</h1><p>Latest published release: <strong>$tag</strong></p><p><a href="solidcraft.flatpakrepo">Add the SolidCraft repository</a></p><p>Flathub provides the required Freedesktop runtime. Add both remotes to the same user installation:</p><pre>flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak remote-add --user --if-not-exists solidcraft $base_url/flatpak/solidcraft.flatpakrepo
flatpak install --user solidcraft $published_id
flatpak run $published_id</pre></main></body></html>
HTML
# Compatibility shim (docs/compatibility.md): links and instructions from before the rename point
# at solvecraft.flatpakrepo. Existing remotes keep working either way: they store the repo URL.
cp "$site/flatpak/solidcraft.flatpakrepo" "$site/flatpak/solvecraft.flatpakrepo"
touch "$site/.nojekyll"
