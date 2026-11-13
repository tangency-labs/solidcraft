#!/usr/bin/env bash
# Release step: give the pending file-format changes their number for good.
#
# For each family of files in crates/doc/build.rs's FAMILIES (designs in crates/doc/src/format/,
# drawings in crates/doc/src/drawing/format/), moves every change in its pending/ folder into its
# v<N>/, where N is the number development builds have been writing for them (the family's last
# folded format + 1). Commit the result with the release's version bump. Nothing to do for a family
# with nothing pending. See crates/doc/src/format/pending/README.md.
set -euo pipefail

root=$(git rev-parse --show-toplevel)
families=$(grep -o 'Family { folder: "[^"]*", base: [0-9]*' "$root/crates/doc/build.rs" | sed 's/.*folder: "\([^"]*\)", base: \([0-9]*\)/\1 \2/' || true)
[ -n "$families" ] || { echo "FAMILIES not found in crates/doc/build.rs" >&2; exit 1; }

shopt -s nullglob
folded=0
while read -r folder last; do
  dir="$root/crates/doc/$folder"
  for d in "$dir"/v*/; do
    n=${d%/}
    n=${n##*/v}
    if [ "$n" -gt "$last" ]; then last=$n; fi
  done
  pending=("$dir"/pending/*.rs)
  if [ ${#pending[@]} -eq 0 ]; then
    echo "No pending format changes in $folder: this release writes format $last."
    continue
  fi
  next=$((last + 1))
  mkdir -p "$dir/v$next"
  git -C "$root" mv "${pending[@]}" "$dir/v$next/"
  echo "Format $next of $folder is now fixed, with ${#pending[@]} change(s):"
  for f in "${pending[@]}"; do echo "  ${f##*/}"; done
  folded=$((folded + 1))
done <<< "$families"
[ "$folded" -eq 0 ] || echo "Commit crates/doc/src/ with the version bump."
