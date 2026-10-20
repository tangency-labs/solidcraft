#!/usr/bin/env bash
# Print the key that names a prebuilt C++ reference build (harness/cpp/build.sh output).
#
#   v1-<upstream commit, 12 hex>-<16 hex of a sha256 over the harness inputs>
#
# The inputs are exactly what build.sh compiles into bin/cpp/lib: the pinned upstream
# commit plus harness/cpp/{CMakeLists.txt,build.sh,stubs/**,patches/**}. The shims
# (harness/cpp/shim/**) are compiled by crates/solidcore-ffi, not by build.sh, so they
# are not part of the key. CI (actions/cache key), .github/workflows/cpp-reference.yml
# (release tag cpp-ref-<key>) and scripts/fetch-cpp.sh all call this script, so they
# always agree. Bump the "v1" prefix when the packaging itself changes.
#
# UPSTREAM_COMMIT overrides the pinned commit (CI exports the same value).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Must match ATTRIBUTION.md / AGENTS.md / .github/workflows/ci.yml.
PINNED=c9c5979f04cea8c38213004d4f04df0dcfb40d05
COMMIT="${UPSTREAM_COMMIT:-$PINNED}"

if command -v sha256sum >/dev/null 2>&1; then
  sha() { sha256sum | cut -c1-64; }
else
  sha() { shasum -a 256 | cut -c1-64; }   # macOS
fi

cd "$ROOT"
files="$(
  { printf '%s\n' harness/cpp/CMakeLists.txt harness/cpp/build.sh
    for d in harness/cpp/stubs harness/cpp/patches; do
      if [ -d "$d" ]; then find "$d" -type f; fi
    done
  } | LC_ALL=C sort
)"
hash="$(
  printf '%s\n' "$files" | while IFS= read -r f; do
    printf '%s  %s\n' "$(sha < "$f")" "$f"
  done | sha
)"
printf 'v1-%s-%s\n' "$(printf '%s' "$COMMIT" | cut -c1-12)" "$(printf '%s' "$hash" | cut -c1-16)"
