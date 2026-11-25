#!/usr/bin/env python3
"""Merge per-TU extraction (extract.py) into one call graph and compute the in-scope closure.

Virtual calls are resolved conservatively:
  * CHA  - a call to a virtual method may reach every (transitive) override in any class.
  * RTA  - same, but restricted to classes whose constructor is reachable (reported for info).
The CHA closure is authoritative for planning (plan/closure.md).

Outputs (bin/callgraph/):
  graph.json       merged funcs/edges/classes
  closure.json     {usr: {...func, "reach": "cha"|"rta"}} for reachable upstream functions
  files.json       per-file totals: lines, func lines, reachable func lines
Usage: build_graph.py
"""
import collections
import glob
import json
import os
import re
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
TU = os.path.join(ROOT, "bin", "callgraph", "tu")
OUTD = os.path.join(ROOT, "bin", "callgraph")
UP = os.path.join(ROOT, "upstream")

sys.path.insert(0, os.path.dirname(__file__))
from entrypoints import ENTRY_GROUPS, EXCLUDED_ENTRY, KERNEL_ROOTS  # noqa: E402


def strip_tmpl(name):
    out, depth = [], 0
    for ch in name:
        if ch == "<":
            depth += 1
        elif ch == ">":
            depth = max(0, depth - 1)
        elif depth == 0:
            out.append(ch)
    return "".join(out)


def load():
    funcs, edges, classes, names, globs, tus = {}, collections.defaultdict(set), {}, {}, [], []
    for p in sorted(glob.glob(os.path.join(TU, "*.json"))):
        d = json.load(open(p))
        tus.append(d["tu"])
        for u, f in d["funcs"].items():
            if u not in funcs or (f["end"] - f["line"]) > (funcs[u]["end"] - funcs[u]["line"]):
                funcs[u] = f
        for u, es in d["edges"].items():
            edges[u].update(es)
        for u, c in d["classes"].items():
            classes.setdefault(u, c)
        names.update(d.get("names", {}))
        for g in d["globals"]:
            globs.append(g)
    return funcs, edges, classes, names, globs, tus


def main():
    funcs, edges, classes, names, globs, tus = load()
    print("funcs", len(funcs), "classes", len(classes), "tus", len(tus))

    # name index for template/unresolved fallback
    by_name = collections.defaultdict(list)
    for u, f in funcs.items():
        by_name[strip_tmpl(f["name"])].append(u)

    # class hierarchy
    derived = collections.defaultdict(set)
    for cu, c in classes.items():
        for b in c["bases"]:
            derived[b].add(cu)

    # override index: base virtual method -> overriders in (transitively) derived classes.
    # The pip libclang bindings lack clang_getOverriddenCursors, so match on the USR method key
    # (name + parameter signature after "@F@") within the class hierarchy.
    def mkey(u):
        i = u.find("@F@")
        return u[i + 3:] if i >= 0 else None

    methods_by_class = collections.defaultdict(dict)
    for u, f in funcs.items():
        if f["class"] and f["kind"] in ("CXX_METHOD", "DESTRUCTOR", "CONVERSION_FUNCTION"):
            k = "~DTOR" if f["kind"] == "DESTRUCTOR" else mkey(u)
            if k:
                methods_by_class[f["class"]][k] = u
    overriders = collections.defaultdict(set)
    for cu, ms in methods_by_class.items():
        for k, u in ms.items():
            if not funcs[u]["virtual"]:
                continue
            st, seen = [cu], set()
            while st:
                x = st.pop()
                for d in derived.get(x, ()):
                    if d in seen:
                        continue
                    seen.add(d)
                    st.append(d)
                    o = methods_by_class.get(d, {}).get(k)
                    if o:
                        overriders[u].add(o)
    by_cls_name = collections.defaultdict(list)
    for cu, c in classes.items():
        by_cls_name[strip_tmpl(c["name"])].append(cu)

    def all_derived(cu):
        seen, st = set(), [cu]
        while st:
            x = st.pop()
            for d in derived.get(x, ()):
                if d not in seen:
                    seen.add(d)
                    st.append(d)
        return seen

    dtor_of = {cu: c["dtor"] for cu, c in classes.items() if c.get("dtor")}

    def all_overriders(u):
        seen, st = set(), [u]
        while st:
            x = st.pop()
            for d in overriders.get(x, ()):
                if d not in seen:
                    seen.add(d)
                    st.append(d)
        return seen

    def resolve(callee):
        """Map a callee USR to defined function USRs (plus pseudo handling)."""
        if callee.startswith("DTOR:"):
            cu = callee[5:]
            out = set()
            for c in {cu} | all_derived(cu):
                if c in dtor_of and dtor_of[c] in funcs:
                    out.add(dtor_of[c])
            return out, True
        if callee in funcs:
            return {callee}, False
        n = names.get(callee)
        if n:
            return set(by_name.get(strip_tmpl(n), ())), False
        return set(), False

    def class_of(u):
        return funcs[u]["class"] if u in funcs else ""

    def closure(roots, mode):
        """mode: 'cha' or 'rta'."""
        reach = set()
        inst = set()  # instantiated classes (RTA)
        pending_virtual = []  # (base usr) awaiting instantiation in RTA
        wl = list(roots)
        vcalls = set()
        while wl or (mode == "rta" and pending_virtual):
            while wl:
                u = wl.pop()
                if u in reach or u not in funcs:
                    continue
                reach.add(u)
                f = funcs[u]
                if f["kind"] == "CONSTRUCTOR" and f["class"]:
                    if f["class"] not in inst:
                        inst.add(f["class"])
                for c in edges.get(u, ()):
                    # constructor names reveal instantiation even for implicit ctors
                    n = names.get(c[3:] if c.startswith("NV:") else c, "")
                    parts = strip_tmpl(n).split("::")
                    if len(parts) >= 2 and parts[-1] == parts[-2]:
                        for cu in by_cls_name.get("::".join(parts[:-1]), ()):
                            inst.add(cu)
                    nonvirt = c.startswith("NV:")
                    tg, is_dtor = resolve(c[3:] if nonvirt else c)
                    for t in tg:
                        wl.append(t)
                        if funcs[t]["virtual"] and not nonvirt:
                            vcalls.add(t)
            # expand virtual calls
            added = False
            for v in list(vcalls):
                for o in all_overriders(v):
                    if o in reach:
                        continue
                    if mode == "cha" or class_of(o) in inst or class_of(o) == "":
                        wl.append(o)
                        added = True
            if not added:
                break
        return reach, inst

    roots, group_of = [], {}
    sm_api = {u: f for u, f in funcs.items() if f["file"].startswith("source/SM_API/src/")}
    entry_found = collections.defaultdict(list)
    for u, f in sorted(sm_api.items(), key=lambda kv: (kv[1]["file"], kv[1]["line"])):
        nm = f["name"]
        for g, pats in ENTRY_GROUPS.items():
            if any(re.fullmatch(p, nm) for p in pats) and nm not in EXCLUDED_ENTRY:
                roots.append(u)
                group_of[u] = g
                entry_found[g].append(nm)
                break
    for u, f in sorted(funcs.items(), key=lambda kv: (kv[1]["file"], kv[1]["line"])):
        if not f["file"].startswith("source/SMLib/"):
            continue
        for g, pats in KERNEL_ROOTS.items():
            if any(re.fullmatch(p, f["name"]) for p in pats):
                roots.append(u)
                group_of[u] = g
                entry_found[g].append(f["name"])
    print("entry points", len(roots))

    reach_cha, inst_cha = closure(roots, "cha")
    reach_rta, inst_rta = closure(roots, "rta")
    print("reach cha", len(reach_cha), "rta", len(reach_rta))

    # per-group closure (CHA) for unit test mapping
    group_reach = {}
    for g in list(ENTRY_GROUPS) + list(KERNEL_ROOTS):
        rs = [u for u in roots if group_of[u] == g]
        group_reach[g] = closure(rs, "cha")[0]

    # file totals
    files = collections.defaultdict(lambda: {"lines": 0, "func_lines": 0, "cha_lines": 0, "rta_lines": 0,
                                             "funcs": 0, "cha_funcs": 0, "rta_funcs": 0, "groups": set()})
    for p in glob.glob(os.path.join(UP, "source", "SMLib", "src", "*.cpp")) + \
            glob.glob(os.path.join(UP, "source", "SMLib", "inc", "*.h")) + \
            glob.glob(os.path.join(UP, "source", "SM_API", "src", "*.cpp")) + \
            glob.glob(os.path.join(UP, "source", "SM_API", "inc", "*.h")):
        r = os.path.relpath(p, UP)
        with open(p, errors="replace") as fh:
            files[r]["lines"] = sum(1 for _ in fh)
    for u, f in funcs.items():
        if not f["file"].startswith("source/"):
            continue
        n = f["end"] - f["line"] + 1
        e = files[f["file"]]
        e["func_lines"] += n
        e["funcs"] += 1
        if u in reach_cha:
            e["cha_lines"] += n
            e["cha_funcs"] += 1
        if u in reach_rta:
            e["rta_lines"] += n
            e["rta_funcs"] += 1
        for g, rs in group_reach.items():
            if u in rs:
                e["groups"].add(g)
    for e in files.values():
        e["groups"] = sorted(e["groups"])

    json.dump({"funcs": funcs, "edges": {k: sorted(v) for k, v in edges.items()}, "classes": classes,
               "names": names}, open(os.path.join(OUTD, "graph.json"), "w"))
    json.dump({"roots": roots, "group_of": group_of, "entry_found": entry_found,
               "cha": sorted(reach_cha), "rta": sorted(reach_rta),
               "inst_rta": sorted(inst_rta),
               "group_reach": {g: sorted(r) for g, r in group_reach.items()}},
              open(os.path.join(OUTD, "closure.json"), "w"))
    json.dump(files, open(os.path.join(OUTD, "files.json"), "w"), indent=0, sort_keys=True)
    json.dump(globs, open(os.path.join(OUTD, "globals.json"), "w"))
    json.dump({k: sorted(v) for k, v in overriders.items()}, open(os.path.join(OUTD, "overriders.json"), "w"))
    allc = set()
    for es in edges.values():
        allc.update(es)
    json.dump({x: sorted(resolve(x[3:] if x.startswith("NV:") else x)[0]) for x in sorted(allc)}, open(os.path.join(OUTD, "resolve.json"), "w"))
    tot = sum(files[f]["cha_lines"] for f in files)
    print("CHA reachable func lines", tot, "RTA", sum(files[f]["rta_lines"] for f in files))
    print("all func lines", sum(files[f]["func_lines"] for f in files), "all lines", sum(files[f]["lines"] for f in files))


if __name__ == "__main__":
    main()
