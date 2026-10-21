#!/usr/bin/env bash
# Regenerate the call graph and the plan (plan/closure.md, plan/units/*.toml, plan/order.md).
# Requires bin/cpp/compile_commands.json (harness/cpp/build.sh) and libclang (see README.md).
set -euo pipefail
cd "$(dirname "$0")/../.."
VENV=bin/callgraph-venv
if [ ! -x "$VENV/bin/python" ]; then
  python3 -m venv "$VENV"
  "$VENV/bin/pip" install -q libclang
fi
mkdir -p bin/callgraph
nice -n 10 "$VENV/bin/python" harness/callgraph/extract.py --jobs "${JOBS:-6}" > bin/callgraph/extract.log
nice -n 10 python3 harness/callgraph/build_graph.py
nice -n 10 python3 harness/callgraph/plan_units.py > bin/callgraph/plan_units.log
nice -n 10 python3 harness/callgraph/write_closure.py
