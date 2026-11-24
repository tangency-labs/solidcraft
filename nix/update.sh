#!/usr/bin/env bash
# Pin the flake's release channels (nix/sources.json) to the newest upstream releases.
#
#   stable      the release GitHub marks Latest
#   prerelease  the newest published release of any kind (nightly, release candidate or stable)
#
# Each channel records the tag, the commit it names, the source hash, the Cargo dependency hash and
# the Rust versions that commit asks for. A channel whose tag and commit are unchanged keeps its
# hashes. The releases always come from upstream, so a fork pins the same builds:
#   SOLIDCRAFT_NIX_UPSTREAM  owner/repo to read (default tangency-labs/solidcraft)
#   GITHUB_TOKEN             optional, raises the GitHub API rate limit
# Needs curl, jq and nix with flakes; without jq or curl it re-runs itself in a nix shell that
# provides them.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(dirname "$here")"
sources="$here/sources.json"
upstream="${SOLIDCRAFT_NIX_UPSTREAM:-tangency-labs/solidcraft}"

if ! command -v jq >/dev/null || ! command -v curl >/dev/null; then
  if [ -n "${SOLIDCRAFT_NIX_UPDATE_REEXEC:-}" ]; then
    echo "update.sh: jq and curl are still missing" >&2
    exit 1
  fi
  export SOLIDCRAFT_NIX_UPDATE_REEXEC=1
  exec nix shell --inputs-from "$root" nixpkgs#jq nixpkgs#curl --command bash "${BASH_SOURCE[0]}" "$@"
fi

api() {
  local auth=()
  if [ -n "${GITHUB_TOKEN:-}" ]; then auth=(-H "Authorization: Bearer $GITHUB_TOKEN"); fi
  curl -fsSL -H "Accept: application/vnd.github+json" "${auth[@]}" "https://api.github.com/repos/$upstream/$1"
}

raw() { curl -fsSL "https://raw.githubusercontent.com/$upstream/$1/$2"; }

# The commit a tag names, through an annotated tag object when there is one.
tag_commit() {
  local ref type sha
  ref="$(api "git/ref/tags/$1")"
  type="$(jq -r .object.type <<<"$ref")"
  sha="$(jq -r .object.sha <<<"$ref")"
  while [ "$type" = tag ]; do
    ref="$(api "git/tags/$sha")"
    type="$(jq -r .object.type <<<"$ref")"
    sha="$(jq -r .object.sha <<<"$ref")"
  done
  [ "$type" = commit ] || { echo "update.sh: tag $1 does not name a commit" >&2; exit 1; }
  echo "$sha"
}

# The Cargo dependency hash of a source, from the flake's own locked nixpkgs: build the vendor
# directory with a fake hash and read the real one from the mismatch.
cargo_hash() {
  local rev="$1" hash="$2" out got
  out="$(cd "$root" && nix build --no-link --impure --expr "
    let
      lock = (builtins.fromJSON (builtins.readFile ./flake.lock)).nodes.nixpkgs.locked;
      pkgs = import (builtins.fetchTree lock) { };
      parts = pkgs.lib.splitString \"/\" \"$upstream\";
    in
    pkgs.rustPlatform.fetchCargoVendor {
      src = pkgs.fetchFromGitHub {
        owner = builtins.elemAt parts 0;
        repo = builtins.elemAt parts 1;
        rev = \"$rev\";
        hash = \"$hash\";
      };
      hash = pkgs.lib.fakeHash;
    }" 2>&1 || true)"
  got="$(grep -oE 'got: +sha256-[A-Za-z0-9+/=]+' <<<"$out" | grep -oE 'sha256-[A-Za-z0-9+/=]+' | tail -n1 || true)"
  if [ -z "$got" ]; then
    echo "update.sh: could not compute the Cargo hash for $rev:" >&2
    echo "$out" | tail -n 20 >&2
    exit 1
  fi
  echo "$got"
}

current() { [ -f "$sources" ] && jq -c --arg ch "$1" '.[$ch] // empty' "$sources" || true; }

pin() {
  local channel="$1" tag="$2" rev old hash cargo rust toolchain
  rev="$(tag_commit "$tag")"
  old="$(current "$channel")"
  if [ -n "$old" ] && [ "$(jq -r .tag <<<"$old")" = "$tag" ] && [ "$(jq -r .rev <<<"$old")" = "$rev" ] \
    && [ "$(jq -r .upstream "$sources")" = "$upstream" ]; then
    echo "$channel: $tag unchanged" >&2
    echo "$old"
    return
  fi
  echo "$channel: $tag ($rev)" >&2
  hash="$(nix flake prefetch --json "github:$upstream/$rev" | jq -r .hash)"
  rust="$(raw "$rev" Cargo.toml | sed -n 's/^rust-version *= *"\([^"]*\)".*/\1/p' | head -n1)"
  toolchain="$(raw "$rev" rust-toolchain.toml | sed -n 's/^channel *= *"\([^"]*\)".*/\1/p' | head -n1)"
  [ -n "$rust" ] || { echo "update.sh: no rust-version in Cargo.toml at $rev" >&2; exit 1; }
  cargo="$(cargo_hash "$rev" "$hash")"
  jq -nc --arg tag "$tag" --arg version "${tag#v}" --arg rev "$rev" --arg hash "$hash" \
    --arg cargoHash "$cargo" --arg rustVersion "$rust" --arg toolchain "$toolchain" \
    '{tag: $tag, version: $version, rev: $rev, hash: $hash, cargoHash: $cargoHash,
      rustVersion: $rustVersion, toolchain: $toolchain}'
}

stable_tag="$(api releases/latest | jq -r .tag_name)"
prerelease_tag="$(api 'releases?per_page=50' \
  | jq -r '[.[] | select(.draft | not) | select(.published_at != null)] | max_by(.published_at) | .tag_name')"
[ -n "$stable_tag" ] && [ "$stable_tag" != null ] || { echo "update.sh: no latest release on $upstream" >&2; exit 1; }
[ -n "$prerelease_tag" ] && [ "$prerelease_tag" != null ] || { echo "update.sh: no published release on $upstream" >&2; exit 1; }

stable="$(pin stable "$stable_tag")"
prerelease="$(pin prerelease "$prerelease_tag")"

new="$(jq -n --arg upstream "$upstream" --argjson stable "$stable" --argjson prerelease "$prerelease" \
  '{upstream: $upstream, stable: $stable, prerelease: $prerelease}')"
if [ -f "$sources" ] && [ "$new" = "$(cat "$sources")" ]; then
  echo "nix/sources.json is up to date" >&2
else
  printf '%s\n' "$new" >"$sources.tmp"
  mv "$sources.tmp" "$sources"
  echo "wrote nix/sources.json" >&2
fi
