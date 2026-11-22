#!/usr/bin/env bash
# SSI_RUNNER adapter so the corpus' own scorer (corpus/ssi/scoring/score.py, jyviko's
# ssi-corpus, MIT OR Apache-2.0) can grade solidcore's engines. score.py calls
#   <runner> <case.json> <mode> <tol> <out.json>
# and this forwards to `goldens --ssi-case <case.json> <tol> <engine> <out.json>` (mode is
# always `gsi`). With SSI_SC_RAW=<dir> it instead copies <dir>/<id>.json, the raw output a
# previous `goldens --ssi-corpus` run kept under bin/ssi-corpus/raw/, so both scorers grade
# byte-identical engine output.
#
#   cargo build --profile goldens -p goldens
#   SSI_RUNNER=$PWD/scripts/ssi-corpus-runner.sh SSI_ENGINE=solidcore-rust \
#     python3 -I corpus/ssi/scoring/score.py gsi 1e-6
#
# Environment: SSI_SC_ENGINE (rust | ssi | solidcore-ssi, default rust), SSI_SC_GOLDENS (goldens binary,
# default bin/goldens/goldens), SSI_SC_RAW (see above).
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
if [ $# -ne 4 ]; then
  echo "usage: $0 <case.json> <mode> <tol> <out.json>" >&2
  exit 2
fi
if [ -n "${SSI_SC_RAW:-}" ]; then
  id="$(basename "$1" .json)"
  exec cp "$SSI_SC_RAW/$id.json" "$4"
fi
exec "${SSI_SC_GOLDENS:-$ROOT/bin/goldens/goldens}" --ssi-case "$1" "$3" "${SSI_SC_ENGINE:-rust}" "$4"
