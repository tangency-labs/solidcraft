#!/usr/bin/env bash
# Fetch the NIST MBE PMI STEP files (the 33 AP203 and AP242 files of the FTC / CTC / STC test
# cases, https://www.nist.gov/ctl/smart-connected-systems-division/smart-connected-manufacturing-systems-group/mbe-pmi-0)
# into corpus/nist-pmi/ (gitignored), for the STEP pipeline's corpus runs (#2577).
#
# The files are a US Government work, public domain. They are still never committed: like the
# STEP Tools set (scripts/fetch-steptools-corpus.sh), the corpus is fetched by script and read
# in place as untrusted data. Nothing from it is ever executed.
#
# The archive and every file in it are pinned by SHA-256 in scripts/nist-pmi-corpus.sha256 (the
# archive is the one SolidCraft's tests/external-corpus.toml pins as "nist-pmi"). A download or
# a file whose hash differs fails the script. Idempotent: an archive or file already present
# with its pinned hash is kept; one present with another hash is an error (remove it and
# re-run). The script removes nothing: a failed download stays as `<archive>.part` and is
# overwritten by the next run.
#
# Usage: scripts/fetch-nist-pmi-corpus.sh [dest]      (default <repo>/corpus/nist-pmi)
set -euo pipefail

URL="https://www.nist.gov/document/nist-pmi-step-files"

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
MANIFEST="$HERE/nist-pmi-corpus.sha256"
DEST="${1:-$ROOT/corpus/nist-pmi}"

for tool in sha256sum curl unzip; do
  command -v "$tool" >/dev/null || { echo "fetch-nist-pmi-corpus: $tool not found" >&2; exit 1; }
done

hash_of() { sha256sum "$1" | cut -d' ' -f1; }

read -r tag archive_sha archive_name < <(grep '^archive ' "$MANIFEST")
[ "$tag" = archive ] && [ -n "$archive_sha" ] && [ -n "$archive_name" ] \
  || { echo "fetch-nist-pmi-corpus: no archive line in $MANIFEST" >&2; exit 1; }

mkdir -p "$DEST/.archive"
archive="$DEST/.archive/$archive_name"
if [ -e "$archive" ]; then
  [ "$(hash_of "$archive")" = "$archive_sha" ] \
    || { echo "fetch-nist-pmi-corpus: $archive exists with another hash; remove it and re-run" >&2; exit 1; }
else
  curl --fail --silent --show-error --location --max-time 600 -o "$archive.part" "$URL"
  got="$(hash_of "$archive.part")"
  [ "$got" = "$archive_sha" ] \
    || { echo "fetch-nist-pmi-corpus: archive sha256 $got, pinned $archive_sha (left as $archive.part)" >&2; exit 1; }
  mv "$archive.part" "$archive"
fi

extracted=0
present=0
while read -r sha path; do
  case "$sha" in ''|'#'*|archive) continue ;; esac
  # Paths are fixed in the manifest; refuse anything that could leave DEST.
  case "$path" in
    */../*|../*|/*|*/..) echo "fetch-nist-pmi-corpus: bad path '$path'" >&2; exit 1 ;;
  esac
  out="$DEST/$path"
  if [ -e "$out" ]; then
    [ "$(hash_of "$out")" = "$sha" ] \
      || { echo "fetch-nist-pmi-corpus: $out exists with another hash; remove it and re-run" >&2; exit 1; }
    present=$((present + 1))
    continue
  fi
  # Exactly the pinned member, by its literal name (-n: never overwrite).
  unzip -q -n "$archive" "$path" -d "$DEST"
  got="$(hash_of "$out")"
  [ "$got" = "$sha" ] \
    || { echo "fetch-nist-pmi-corpus: $path: sha256 $got, pinned $sha" >&2; exit 1; }
  extracted=$((extracted + 1))
done < "$MANIFEST"
echo "nist-pmi corpus in $DEST: $extracted extracted, $present already present"
