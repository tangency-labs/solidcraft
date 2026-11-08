#!/usr/bin/env python3
"""Explain why a function is in the closure: print a shortest call path from an entry point.

Usage: why.py <substring of qualified name or file> [--group G] [--max N]
Virtual dispatch hops are marked with '~>' (callee reached as an override of the called method).
"""
import argparse
import collections
import json
import os

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
D = os.path.join(ROOT, "bin", "callgraph")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("target")
    ap.add_argument("--group", default="")
    ap.add_argument("--max", type=int, default=3)
    a = ap.parse_args()
    g = json.load(open(os.path.join(D, "graph.json")))
    c = json.load(open(os.path.join(D, "closure.json")))
    F, E = g["funcs"], g["edges"]
    ov = {k: set(v) for k, v in json.load(open(os.path.join(D, "overriders.json"))).items()}
    res = json.load(open(os.path.join(D, "resolve.json")))
    roots = [r for r in c["roots"] if not a.group or c["group_of"][r] == a.group]
    prev = {r: (None, "") for r in roots}
    q = collections.deque(roots)
    while q:
        u = q.popleft()
        nxt = []
        for x in E.get(u, ()):
            for t in res.get(x, [x] if x in F else []):
                nxt.append((t, "->"))
                if F[t]["virtual"] and not x.startswith("NV:"):
                    for o in ov.get(t, ()):
                        nxt.append((o, "~>"))
        for t, how in nxt:
            if t in F and t not in prev:
                prev[t] = (u, how)
                q.append(t)
    hits = [u for u in prev if a.target in F[u]["name"] or a.target in F[u]["file"]]
    hits.sort(key=lambda u: F[u]["name"])
    for h in hits[: a.max]:
        path, u = [], h
        while u is not None:
            p, how = prev[u]
            path.append((F[u]["name"], F[u]["file"].split("/")[-1], how))
            u = p
        print("==", F[h]["name"])
        for n, f, how in reversed(path):
            print("   %s %s  [%s]" % (how or "  ", n, f))
    if not hits:
        print("not reachable")


if __name__ == "__main__":
    main()
