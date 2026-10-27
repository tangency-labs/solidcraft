#!/usr/bin/env bash
# Fetches SMLib for solidcraft-smlib (feature `native`), at the commit in crates/smlib/SMLIB_REV.
#
#   scripts/fetch-smlib.sh [TARGET...]     the prebuilt C API library for each Rust target (default:
#                                          the host's), checked against crates/smlib/SMLIB_PREBUILT,
#                                          into target/smlib-<rev12>-prebuilt/<target>; the sources
#                                          for a target with no prebuilt library
#   scripts/fetch-smlib.sh --source        the sources (source/SMLib, source/SM_API, capi), into
#                                          target/smlib-<rev12> or SOLIDCRAFT_SMLIB_DIR, to compile
#   scripts/fetch-smlib.sh --pin           writes SMLIB_PREBUILT from the release of SMLIB_REV
#
# The libraries are the release prebuilt-<rev> of tangency-labs/smlib (its prebuilt.yml).
# OpenCASCADE is not part of the fork.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
rev="$(tr -d '[:space:]' < "$root/crates/smlib/SMLIB_REV")"
pins="$root/crates/smlib/SMLIB_PREBUILT"
release="https://github.com/tangency-labs/smlib/releases/download/prebuilt-$rev"

sha256() {
  if command -v sha256sum >/dev/null; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}

fetch_source() {
  local dest="${SOLIDCRAFT_SMLIB_DIR:-$root/target/smlib-${rev:0:12}}"
  if [ -f "$dest/capi/include/smlib.h" ] && [ "$(git -C "$dest" rev-parse HEAD 2>/dev/null)" = "$rev" ]; then
    echo "SMLib sources $rev already at $dest"
    return
  fi
  rm -rf "$dest"
  mkdir -p "$dest"
  git -C "$dest" init -q
  git -C "$dest" remote add origin https://github.com/tangency-labs/smlib.git
  git -C "$dest" sparse-checkout set --no-cone /source/SMLib/ /source/SM_API/ /capi/ /LICENSE
  git -C "$dest" fetch -q --depth 1 --filter=blob:none origin "$rev"
  git -C "$dest" checkout -q FETCH_HEAD
  echo "SMLib sources $rev at $dest"
}

fetch_prebuilt() {
  local target="$1" want dest tmp
  want="$(awk -v t="$target" '$1 == t { print $2 }' "$pins")"
  if [ -z "$want" ]; then
    echo "no prebuilt SMLib for $target: fetching the sources to compile"
    fetch_source
    return
  fi
  dest="$root/target/smlib-${rev:0:12}-prebuilt/$target"
  if [ "$(cat "$dest/.sha256" 2>/dev/null)" = "$want" ]; then
    echo "SMLib $rev for $target already at $dest"
    return
  fi
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' RETURN
  curl -fsSL --retry 3 -o "$tmp/smlib.tar.gz" "$release/smlib-$target.tar.gz"
  if [ "$(sha256 "$tmp/smlib.tar.gz")" != "$want" ]; then
    echo "smlib-$target.tar.gz does not match its SHA-256 in crates/smlib/SMLIB_PREBUILT" >&2
    exit 1
  fi
  tar -xzf "$tmp/smlib.tar.gz" -C "$tmp"
  if [ "$(tr -d '[:space:]' < "$tmp/smlib-$target/REV")" != "$rev" ]; then
    echo "smlib-$target.tar.gz was built from another commit than SMLIB_REV" >&2
    exit 1
  fi
  rm -rf "$dest"
  mkdir -p "$(dirname "$dest")"
  mv "$tmp/smlib-$target" "$dest"
  echo "$want" > "$dest/.sha256"
  echo "SMLib $rev for $target at $dest"
}

case "${1:-}" in
  --source)
    fetch_source
    ;;
  --pin)
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    curl -fsSL --retry 3 -o "$tmp/SHA256SUMS" "$release/SHA256SUMS"
    {
      echo "# Rust target, SHA-256 of smlib-<target>.tar.gz in the release prebuilt-<SMLIB_REV> of"
      echo "# tangency-labs/smlib. Written by scripts/fetch-smlib.sh --pin; never edited by hand."
      sed -E 's/^([0-9a-f]{64}) +smlib-(.*)\.tar\.gz$/\2 \1/' "$tmp/SHA256SUMS" | sort
    } > "$pins"
    echo "pinned $(grep -vc '^#' "$pins") prebuilt targets of $rev"
    ;;
  *)
    if [ $# -eq 0 ]; then
      set -- "$(rustc -vV | sed -n 's/^host: //p')"
    fi
    for target in "$@"; do
      fetch_prebuilt "$target"
    done
    ;;
esac
