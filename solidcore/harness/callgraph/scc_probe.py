#!/usr/bin/env python3
"""Probe strongly connected components of the reachable function graph (static vs dispatch edges)."""
import json, os, sys, collections
sys.setrecursionlimit(100000)
D = os.path.join(os.path.dirname(__file__), "..", "..", "bin", "callgraph")
g = json.load(open(os.path.join(D, "graph.json"))); c = json.load(open(os.path.join(D, "closure.json")))
res = json.load(open(os.path.join(D, "resolve.json"))); ov = json.load(open(os.path.join(D, "overriders.json")))
F, E = g["funcs"], g["edges"]; R = set(c["rta"]); inst = set(c["inst_rta"])

def succ(u, dispatch):
    out = set()
    for x in E.get(u, ()):
        for t in res.get(x, []):
            if t in R:
                out.add(t)
                if dispatch and F[t]["virtual"] and not x.startswith("NV:"):
                    out.update(o for o in ov.get(t, ()) if o in R)
    out.discard(u)
    return out

def tarjan(nodes, sf):
    idx, low, on, st, comps, n = {}, {}, set(), [], [], [0]
    for s in nodes:
        if s in idx: continue
        work = [(s, iter(sf(s)))]; idx[s] = low[s] = n[0]; n[0] += 1; st.append(s); on.add(s)
        while work:
            v, it = work[-1]
            for w in it:
                if w not in idx:
                    idx[w] = low[w] = n[0]; n[0] += 1; st.append(w); on.add(w)
                    work.append((w, iter(sf(w)))); break
                elif w in on:
                    low[v] = min(low[v], idx[w])
            else:
                work.pop()
                if work: low[work[-1][0]] = min(low[work[-1][0]], low[v])
                if low[v] == idx[v]:
                    comp = []
                    while True:
                        w = st.pop(); on.discard(w); comp.append(w)
                        if w == v: break
                    comps.append(comp)
    return comps

for disp in (False, True):
    cache = {u: succ(u, disp) for u in R}
    comps = tarjan(sorted(R), lambda u: cache[u])
    sz = sorted(((sum(F[u]["end"] - F[u]["line"] + 1 for u in cp), len(cp)) for cp in comps), reverse=True)
    print("dispatch" if disp else "static", "comps", len(comps), "largest (lines,funcs):", sz[:8])
    big = max(comps, key=len)
    fc = collections.Counter(F[u]["file"].split("/")[-1] for u in big)
    print("  biggest SCC files:", fc.most_common(15))
