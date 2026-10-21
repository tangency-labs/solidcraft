#!/usr/bin/env python3
"""Split light dependencies out of `depends_on` into `soft_depends_on` (plan/units/*.toml).

A dependency U -> D is *soft* when U's functions call at most MAX_CALLS distinct functions
of D and D owns no methods of a class U's own classes are or derive from (so U needs no
type from D). A soft dependency does not block claiming U: the calls into D are stubbed
with `UNPORTED(<D>)` and the cases that reach them are deferred, exactly like a back-edge
(AGENTS.md section 4a). Hard dependencies stay in `depends_on`.

Needs the call graph: `harness/callgraph/run.sh` (writes bin/callgraph/graph.json; set
CALLGRAPH=<path> to use another copy, e.g. from a worktree).
Idempotent: soft deps already split out are re-evaluated together with depends_on.

    python3 harness/plan/soften.py [--max-calls 2]
"""
import collections
import json
import os
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
MAX_CALLS = int(sys.argv[sys.argv.index("--max-calls") + 1]) if "--max-calls" in sys.argv else 2

g = json.loads(pathlib.Path(os.environ.get("CALLGRAPH", ROOT / "bin/callgraph/graph.json")).read_text())
edges, funcs, classes = g["edges"], g["funcs"], g["classes"]
units_json = json.loads((ROOT / "harness/callgraph/units.json").read_text())
byname = {v["name"]: k for k, v in classes.items()}

owner = {f["usr"]: uid for uid, d in units_json.items() for f in d["functions"]}
calls = collections.defaultdict(set)
for usr, uid in owner.items():
    for t in edges.get(usr, []):
        d = owner.get(t)
        if d and d != uid:
            calls[(uid, d)].add(t)


def cls_id(c):
    return c if c in classes else byname.get(c, c)


def ancestors(c, seen):
    for b in classes.get(c, {}).get("bases", []):
        b = cls_id(b)
        if b not in seen:
            seen.add(b)
            ancestors(b, seen)
    return seen


own_classes = collections.defaultdict(set)
for usr, uid in owner.items():
    c = funcs.get(usr, {}).get("class", "")
    if c:
        own_classes[uid].add(cls_id(c))


def needs_type(u, d):
    mine = set()
    for c in own_classes[u]:
        mine |= {c} | ancestors(c, set())
    return bool(mine & own_classes[d])


sys.path.insert(0, str(ROOT / "harness/plan"))
import units as plan_units  # noqa: E402

moved, total = 0, 0
for u in plan_units.load_units(ROOT):
    uid = u["id"]
    deps = sorted(set(u.get("depends_on", [])) | set(u.get("soft_depends_on", [])))
    if uid not in units_json:
        # Not a port unit (e.g. audit.recursion): its ordering is deliberate, keep it hard.
        hard = deps
    else:
        hard = [d for d in deps if len(calls.get((uid, d), ())) > MAX_CALLS or needs_type(uid, d)]
    soft = [d for d in deps if d not in hard]
    total += len(deps)
    moved += len(soft)
    new = {}
    for k, v in u.items():  # soft_depends_on goes right after depends_on
        if k == "soft_depends_on":
            continue
        new[k] = v
        if k == "depends_on":
            new[k], new["soft_depends_on"] = hard, soft
    plan_units.write_unit(new, ROOT)
print(f"soft_depends_on: {moved} of {total} dependencies (max {MAX_CALLS} calls, no type dependency)",
      file=sys.stderr)
