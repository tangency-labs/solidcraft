#!/usr/bin/env bash
# Exact-base artifact retrieval. Event payload, API provenance, digest and complete
# report set must validate before any --baseline argument is emitted.
set -euo pipefail
exec python3 "$(dirname "$0")/difftest_baseline.py" "$@"
