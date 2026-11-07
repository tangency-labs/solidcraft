#!/usr/bin/env bash
# Export each fixture with solidcraft-cli and load the URDF in real tools (README.md).
#
#   tools/urdf-check/run.sh [fixture...]
#
# SOLIDCRAFT_CLI picks the binary (default target/release/solidcraft-cli), PYTHON the interpreter.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
exec "${PYTHON:-python3}" -I "$here/check.py" "$@"
