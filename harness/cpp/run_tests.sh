#!/usr/bin/env bash
# Run the upstream C++ test apps built by build.sh.
#
# Usage:
#   harness/cpp/run_tests.sh sm_api                 # all SM_API tests
#   harness/cpp/run_tests.sh prog [prog_test args]  # e.g. --list-suites, --suite booleans
#
# The upstream tests open files as "../../TestFiles/pt_TestFiles/..." and
# write to "../prog_test/OutputFiles", relative to tests/<name>/. We mirror
# that layout under bin/cpp/run with a private copy of pt_TestFiles so that
# nothing is ever written into upstream/.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
OUT="$ROOT/bin/cpp"
RUN="$OUT/run"

mkdir -p "$RUN/TestFiles" "$RUN/tests/prog_test" "$RUN/tests/SM_API_test"
# Refresh private copy of the test data (pt_TestFiles only).
rsync -a --delete "$ROOT/upstream/TestFiles/pt_TestFiles/" "$RUN/TestFiles/pt_TestFiles/"

which="${1:-}"; shift || true
case "$which" in
  sm_api) cd "$RUN/tests/SM_API_test"; exec "$OUT/tests/SM_API_test_app" "$@" ;;
  prog)   cd "$RUN/tests/prog_test";   exec "$OUT/tests/prog_test_app" "$@" ;;
  *) echo "usage: $0 sm_api | prog [args]" >&2; exit 2 ;;
esac
