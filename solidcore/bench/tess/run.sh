#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
#
# Mesh every body of LIST with each harness binary in turn, one process per body, setting and
# mesher, strictly one after another, so every mesher meets the same load on a shared machine and
# its timings are clean. Each process's JSON lines are kept in OUT/<label>/<setting>/<body>.jsonl.
#
#   TMO=900   seconds a process may take before it is killed
#
# Usage: bench/tess/run.sh LIST DUMPS OUT LABEL=BIN[:ONLY]...
#   LIST   one body name per line (the body is DUMPS/NAME.nb)
#   ONLY   solidcraft (SolidCraft's mesher, `Body::mesh_plan`) or solidcore-tess (`solidcore_tess::plan`);
#          without it the binary meshes with both
set -uo pipefail
LIST="$1"; DUMPS="$2"; OUT="$3"; shift 3
for mode in display export; do
  while read -r n; do
    [ -n "$n" ] || continue
    for spec in "$@"; do
      label="${spec%%=*}"; rest="${spec#*=}"; bin="${rest%%:*}"; only=""
      [ "$bin" != "$rest" ] && only="${rest#*:}"
      d="$OUT/$label/$mode"; mkdir -p "$d"
      MB_ONLY="$only" /usr/bin/timeout "${TMO:-900}" "$bin" "$DUMPS/$n.nb" --mode "$mode" >"$d/$n.jsonl" 2>"$d/$n.err"
      rc=$?
      if [ $rc -ne 0 ]; then printf '{"kind":"proc","body":"%s","mode":"%s","rc":%d}\n' "$n" "$mode" "$rc" >>"$d/$n.jsonl"; fi
    done
  done <"$LIST"
done
touch "$OUT/done"
