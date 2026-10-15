#!/usr/bin/env bash
# Fetch jyviko's exact-ground-truth surface/surface intersection corpus
# (https://github.com/jyviko/ssi-corpus, MIT OR Apache-2.0) at a pinned commit into
# corpus/ssi/ (gitignored; never commit case data). Idempotent: re-running leaves an
# existing checkout at the pinned commit.
#
# The corpus is data only: difftest --ssi-corpus reads its cases/data/*.json and never
# executes anything from it.
#
# Usage: scripts/fetch-ssi-corpus.sh [dest]      (default <repo>/corpus/ssi)
# Used by: cargo run --profile difftest -p difftest --features cpp -- --ssi-corpus
set -euo pipefail

URL="https://github.com/jyviko/ssi-corpus.git"
# Full SHA (b04c8a6): a short prefix could later be matched by a crafted commit.
COMMIT="b04c8a6093a8b3505d865d1dd874ec3879ccb1f9"

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
DEST="${1:-$ROOT/corpus/ssi}"

if [ -d "$DEST/.git" ]; then
  origin="$(git -C "$DEST" remote get-url origin 2>/dev/null || true)"
  if [ "$origin" != "$URL" ]; then
    echo "fetch-ssi-corpus: $DEST has origin '$origin', expected $URL; remove it and re-run" >&2
    exit 1
  fi
  if [ -n "$(git -C "$DEST" status --porcelain --untracked-files=all)" ]; then
    echo "fetch-ssi-corpus: $DEST has local changes; remove it and re-run" >&2
    exit 1
  fi
  if [ "$(git -C "$DEST" rev-parse HEAD)" = "$COMMIT" ]; then
    echo "ssi-corpus already at $COMMIT in $DEST"
    exit 0
  fi
else
  if [ -e "$DEST" ]; then
    echo "fetch-ssi-corpus: $DEST exists and is not a git checkout; remove it and re-run" >&2
    exit 1
  fi
  mkdir -p "$(dirname "$DEST")"
  git clone --quiet --no-checkout --filter=blob:none "$URL" "$DEST"
fi
git -C "$DEST" fetch --quiet origin "$COMMIT"
git -C "$DEST" -c advice.detachedHead=false checkout --quiet --detach "$COMMIT"
[ "$(git -C "$DEST" rev-parse HEAD)" = "$COMMIT" ] || { echo "fetch-ssi-corpus: pin mismatch" >&2; exit 1; }
echo "ssi-corpus at $COMMIT in $DEST"
