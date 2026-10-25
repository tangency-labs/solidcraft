#!/usr/bin/env bash
# Fetch the STEP Tools sample STEP files (AP203 and AP214 sets,
# https://steptools.com/docs/stpfiles/) into corpus/steptools/ (gitignored).
#
# The files are "All Rights Reserved" by STEP Tools, Inc.; the site permits only
# informational, non-commercial copying. NEVER commit them, and never copy them into
# fixtures/. This script only downloads them for local corpus runs.
#
# Every file is pinned by SHA-256 in scripts/steptools-corpus.sha256; a download whose
# hash differs fails the script (nothing is kept). Idempotent: files already present
# with the pinned hash are not downloaded again; a present file with another hash is an
# error (remove it and re-run).
#
# The corpus is data only: difftest --step-corpus parses it as untrusted input and never
# executes anything from it.
#
# Usage: scripts/fetch-steptools-corpus.sh [dest]      (default <repo>/corpus/steptools)
# Used by: cargo run --profile difftest -p difftest --features cpp -- --step-corpus
set -euo pipefail

BASE="https://steptools.com/docs/stpfiles"

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
MANIFEST="$HERE/steptools-corpus.sha256"
DEST="${1:-$ROOT/corpus/steptools}"

command -v sha256sum >/dev/null || { echo "fetch-steptools-corpus: sha256sum not found" >&2; exit 1; }
command -v curl >/dev/null || { echo "fetch-steptools-corpus: curl not found" >&2; exit 1; }

hash_of() { sha256sum "$1" | cut -d' ' -f1; }

mkdir -p "$DEST"
fetched=0
present=0
while read -r sha path; do
  case "$sha" in ''|'#'*) continue ;; esac
  # Paths are fixed in the manifest; refuse anything that could leave DEST.
  case "$path" in
    */../*|../*|/*|*/..) echo "fetch-steptools-corpus: bad path '$path'" >&2; exit 1 ;;
  esac
  out="$DEST/$path"
  if [ -e "$out" ]; then
    if [ "$(hash_of "$out")" = "$sha" ]; then
      present=$((present + 1))
      continue
    fi
    echo "fetch-steptools-corpus: $out exists with another hash; remove it and re-run" >&2
    exit 1
  fi
  mkdir -p "$(dirname "$out")"
  tmp="$out.part"
  rm -f "$tmp"
  curl --fail --silent --show-error --location --max-time 300 -o "$tmp" "$BASE/$path"
  got="$(hash_of "$tmp")"
  if [ "$got" != "$sha" ]; then
    rm -f "$tmp"
    echo "fetch-steptools-corpus: $path: sha256 $got, pinned $sha" >&2
    exit 1
  fi
  mv "$tmp" "$out"
  fetched=$((fetched + 1))
done < "$MANIFEST"
echo "steptools corpus in $DEST: $fetched fetched, $present already present"
