#!/usr/bin/env python3
"""Cluster the in-scope closure into port units and emit the plan.

Inputs  (bin/callgraph/, produced by extract.py + build_graph.py):
  graph.json, closure.json, resolve.json, overriders.json, files.json, globals.json
Outputs:
  plan/units/<id>.toml            one file per unit, per plan/modules.schema.md
  plan/order.md                   parallel layers + critical path
  plan/closure.md                 closure report (files, unused code, hazards, threading)
  harness/callgraph/units.json    unit -> upstream functions (USR, name, file, lines)

Unit model
  * The authoritative closure is CHA (every override of a reachable virtual counts).
  * "Idiom" functions (NLib malloc/free/error stacks, SmTArray-style containers, string helpers,
    memory managers) map to Rust std/Result and are not port units; edges to them are ignored.
  * Functions are grouped by (rust module, home file); headers fold into the same-named .cpp.
    Groups larger than MAX_UNIT lines are split along the intra-file call order (callees first)
    without splitting intra-file recursion (function SCCs). Groups below MIN_UNIT are merged into
    the same-module unit they call most.
  * Unit dependency = any static call or virtual dispatch from one unit's function into another's.
    Unit-level cycles (the curve/surface/topology core is mutually recursive) are broken with a
    weighted greedy feedback-arc-set (Eades-Lin-Smyth); dropped back-edges are listed per unit.
"""
import collections
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", ".."))
D = os.path.join(ROOT, "bin", "callgraph")
UP = os.path.join(ROOT, "upstream")
sys.path.insert(0, HERE)
from entrypoints import ENTRY_GROUPS, OUT_OF_SCOPE_ENTRY  # noqa: E402
from pp import inactive_lines  # noqa: E402

MAX_UNIT = 3000
TARGET_CHUNK = 2200
MIN_UNIT = 300


def L(f):
    return f["end"] - f["line"] + 1


# ----------------------------------------------------------------------------------------------
# load
g = json.load(open(os.path.join(D, "graph.json")))
cl = json.load(open(os.path.join(D, "closure.json")))
RES = json.load(open(os.path.join(D, "resolve.json")))
OV = json.load(open(os.path.join(D, "overriders.json")))
FILES = json.load(open(os.path.join(D, "files.json")))
GLOBS = json.load(open(os.path.join(D, "globals.json")))
F, E = g["funcs"], g["edges"]
CHA, RTA = set(cl["cha"]), set(cl["rta"])
ROOTS = cl["roots"]
GROUP_OF = cl["group_of"]
R = {u for u in CHA if F[u]["file"].startswith("source/")}

# ----------------------------------------------------------------------------------------------
# idioms: replaced by Rust std / Result, not ported as units
IDIOM_FILES = {
    "SmTArray.h", "SmTArray.cpp", "SmTList.h", "SmTList.cpp", "SmSArray.h", "SmSArray.cpp",
    "SmMemory.h", "SmMemory.cpp", "SmMemBlockMgr.h", "SmMemBlockMgr.cpp", "SmString.h",
    "SmTypes.h", "SmTypes.cpp", "SmMapPtrToPtr.h", "SmMapPtrToPtr.cpp", "SmMapTypeToType.h",
    "SmMapTypeToType.cpp", "SmBucketSort.h", "SmBucketSort.cpp", "SmDllMain.cpp", "StdAfx.cpp",
    "SmCodeCount.cpp",
}
IDIOM_NL = re.compile(r"^(N_Malloc|N_Calloc|N_Realloc|N_Free\w*|N_Alloc\w*|N_ErrSet|N_ErrGet\w*|"
                      r"N_InitNurbs|N_EndNurbs|N_Stack\w*|N_Push\w*|N_Pop\w*)$")


def is_idiom(u):
    f = F[u]
    base = f["file"].split("/")[-1]
    if base in IDIOM_FILES:
        return True
    nm = f["name"].split("::")[-1]
    if base.startswith("NL_") and IDIOM_NL.match(nm):
        return True
    if nm.startswith("smos_"):
        return True
    return False


IDIOM = {u for u in R if is_idiom(u)}
PORT = R - IDIOM

# ----------------------------------------------------------------------------------------------
# module assignment
MODULE_RULES = [
    (r"^NL_", "nurbs"),
    (r"^SmB(Spline|ezier)(Curve|Surface)(NLib)?\.", "nurbs"),
    (r"^SmNurbs(Crv|Srf|Vol)\.", "nurbs"),
    (r"^SmBSplineVolume(NLib)?\.", "surface"),
    (r"^Sm(Vector\dd|VectorNd|Matrix|Transform|Extent\dd|ExtentNd|PeriodicExtent1d|Math|Polynomial|"
     r"Axis2Placement|Gauss|LocalSolve1d|LocalSolveNd|GlobalSolver|SolutionArray|Tree|Grid|"
     r"PseudoBox|PolarBox|HashTable|PointSet|Histogram|Shape)\.", "geom"),
    (r"^Sm(Object|AObject|SAGObject|Context|Tol|Messages|Attribute|ThreadLocalStorage|CacheMgr\w*|"
     r"ObjProps|Relation|Config|CoreTypes|Tools|AssertArray|Register\w*)\.", "geom"),
    (r"^Sm(Fillet\w*|CurveFilletExecutive|BlendSurface|HCR)\.", "blend"),
    (r"^Sm(Offset(Executive|GeometryCreation))\.", "offset"),
    (r"^Sm(Merge|TopologyIntersector|Cutter|BrepCutting)\.", "boolean"),
    (r"^Sm(HealData|HealProbArray|Stitch|EdgeProps|VertexProps|FaceProps|LoopProps|Gap)\.", "heal"),
    (r"^Sm(TopologySweep|RotationalSweepGeometry|TransSweepGeometry|SweepGeometryCreation|"
     r"FeatureExecutive)\.", "sweep"),
    (r"^SmPrimitiveCreation\.", "primitive"),
    (r"^SmIntegrator\.", "mass"),
    (r"^Sm(SurfaceIntersector|AdvSurfaceIntersector|SurfaceTracer|RayTracer|PointClass|CurveClass|"
     r"LineSegClass|SurfaceDropLib|SurfaceDropCurve|SurfaceSilhouette|DirectedSilhouette|TopologySolver|"
     r"SLC|SpaceBend|SpaceUnbend)\.", "intersect"),
    (r"^Sm(Curve|CurveCache|Line|Circle|Ellipse|Conic|Parabola|Hyperbola|CompositeCurve|HermiteCurve|"
     r"OffsetCurve|IsoCurve|ProjectedCurve|CrvOnSurf|CrvInVolume|TangentField|Spiral)\.", "curve"),
    (r"^Sm(Surface|SurfaceCache|SurfacePointSolve|Plane|Sphere|Cone|Cylinder|Torus|SurfOfRevolution|"
     r"SurfOfExtrusion|OffsetSurface|CubicBezierSurface|STEPSurface|CurveBoundedSurface|TrimSrfCache|"
     r"SrfInVolume|Volume|BendVolume|UnbendVolume|TwistVolume)\.", "surface"),
    (r"^Sm(Brep|BrepData|BrepCache|Face|Edge|Vertex|Shell|Region|Loop|Loopuse|Edgeuse|Vertexuse|Faceuse|"
     r"Topology|OwningTopology|ProtoTopology|CEdge|CFace|TopologyTraverser|TrackTopologyChanges|"
     r"CopyBrepMap|TrimmingTools|DatabaseIO|Assembly|AssemblyInstance|FaceGrid|SMB\w*)\.", "topology"),
    (r"^Sm(Poly\w*|Tess|Graphics\w*|Draw\w*)\.", "query"),
]
TOPO_TOPIC = [
    (re.compile(r"(Mass|Volume|Area|Centroid|Inertia|Moment|Integra|Properties)"), "mass"),
    (re.compile(r"(ClosestPoint|Distance|Classify|Containment|Contains|PointIn|IsInside|BoundingBox|"
                r"Relationship)"), "query"),
]
API_MODULE = {"fixture_io": "topology", "context": "geom", "primitive": "primitive", "boolean": "boolean", "fillet": "blend",
              "offset": "offset", "sweep": "sweep", "heal": "heal", "mass": "mass", "query": "query",
              "copy_transform": "topology"}


def home_file(u):
    f = F[u]["file"]
    if f.endswith(".h"):
        cpp = f.replace("/inc/", "/src/")[:-2] + ".cpp"
        if cpp in FILES:
            return cpp
    return f


def file_module(path):
    base = path.split("/")[-1]
    for pat, m in MODULE_RULES:
        if re.match(pat, base):
            return m
    return "geom"


# SM_API helpers: module of their (first) in-scope API caller group
api_group = {}
for r in ROOTS:
    api_group[r] = GROUP_OF[r]


def callees(u, dispatch=True):
    out = set()
    for x in E.get(u, ()):
        nv = x.startswith("NV:")
        for t in RES.get(x, []):
            if t not in R:
                continue
            out.add(t)
            if dispatch and not nv and F[t]["virtual"]:
                out.update(o for o in OV.get(t, ()) if o in R)
    out.discard(u)
    return out


SUCC = {u: callees(u) for u in R}
SUCC_PORT = {u: {v for v in SUCC[u] if v in PORT} for u in PORT}
PRED = collections.defaultdict(set)
for u, vs in SUCC_PORT.items():
    for v in vs:
        PRED[v].add(u)

# propagate API groups to SM_API helper functions
for u in sorted(PORT):
    if F[u]["file"].startswith("source/SM_API/") and u not in api_group:
        callers = sorted(GROUP_OF[p] for p in PRED[u] if p in GROUP_OF)
        api_group[u] = callers[0] if callers else "query"


def module_of(u):
    f = F[u]
    if f["file"].startswith("source/SM_API/"):
        return API_MODULE[api_group.get(u, "query")]
    m = file_module(home_file(u))
    if m == "topology":
        nm = f["name"].split("::")[-1]
        for rx, topic in TOPO_TOPIC:
            if rx.search(nm):
                return topic
    return m


# ----------------------------------------------------------------------------------------------
# smallest NLib leaf: knot vectors + B-spline basis evaluation
LEAF_SEED = re.compile(r"^(N_KnotVector\w*|N_SetKnotIndex|N_BasisFindSpan\w*|N_BasisGetSpanCount|N_BasisEval|"
                       r"N_BasisDerivs|N_BasisIEval|N_BasisIDerivs|N_BasisDerivsArray)$")
leaf = {u for u in PORT if F[u]["file"].split("/")[-1].startswith("NL_Basis") and LEAF_SEED.match(F[u]["name"])}
changed = True
while changed:  # keep only functions whose non-idiom callees stay inside the leaf
    changed = False
    for u in sorted(leaf):
        if not SUCC_PORT[u] <= leaf:
            leaf.discard(u)
            changed = True

# ----------------------------------------------------------------------------------------------
# function SCCs (for chunking without splitting recursion)


def tarjan(nodes, succ):
    idx, low, on, st, comps, n = {}, {}, set(), [], [], [0]
    for s in nodes:
        if s in idx:
            continue
        work = [(s, iter(sorted(succ(s))))]
        idx[s] = low[s] = n[0]
        n[0] += 1
        st.append(s)
        on.add(s)
        while work:
            v, it = work[-1]
            pushed = False
            for w in it:
                if w not in idx:
                    idx[w] = low[w] = n[0]
                    n[0] += 1
                    st.append(w)
                    on.add(w)
                    work.append((w, iter(sorted(succ(w)))))
                    pushed = True
                    break
                elif w in on:
                    low[v] = min(low[v], idx[w])
            if pushed:
                continue
            work.pop()
            if work:
                low[work[-1][0]] = min(low[work[-1][0]], low[v])
            if low[v] == idx[v]:
                comp = []
                while True:
                    w = st.pop()
                    on.discard(w)
                    comp.append(w)
                    if w == v:
                        break
                comps.append(sorted(comp))
    return comps


# ----------------------------------------------------------------------------------------------
# initial groups
groups = collections.defaultdict(list)
for u in sorted(PORT - leaf):
    groups[(module_of(u), home_file(u))].append(u)

units = []  # list of dict(id?, module, files, funcs)


def snake(s):
    s = re.sub(r"^(SmApi|Sm|NL_)", "", s)
    s = re.sub(r"(?<=[a-z0-9])(?=[A-Z])", "_", s)
    s = re.sub(r"(?<=[A-Z])(?=[A-Z][a-z])", "_", s)
    return re.sub(r"[^a-z0-9_]", "", s.lower()).strip("_")


for (mod, hf), fs in sorted(groups.items()):
    total = sum(L(F[u]) for u in fs)
    base = hf.split("/")[-1].rsplit(".", 1)[0]
    if total <= MAX_UNIT:
        units.append({"module": mod, "base": base, "home": hf, "funcs": fs})
        continue
    # split: order by height in intra-group condensation (callees first)
    fset = set(fs)
    comps = tarjan(fs, lambda x: SUCC_PORT[x] & fset)
    comp_of = {u: i for i, cp in enumerate(comps) for u in cp}
    csucc = collections.defaultdict(set)
    for u in fs:
        for v in SUCC_PORT[u] & fset:
            if comp_of[v] != comp_of[u]:
                csucc[comp_of[u]].add(comp_of[v])
    height = {}
    for i in range(len(comps)):  # tarjan emits callee comps first
        height[i] = 1 + max((height[j] for j in csucc[i]), default=-1)
    order = sorted(range(len(comps)), key=lambda i: (height[i], min(F[u]["line"] for u in comps[i])))
    chunk, size = [], 0
    nchunks = max(2, -(-total // TARGET_CHUNK))
    target = total / nchunks
    for i in order:
        cs = sum(L(F[u]) for u in comps[i])
        if chunk and size + cs > target * 1.15 and size >= MIN_UNIT:
            units.append({"module": mod, "base": base, "home": hf, "funcs": chunk})
            chunk, size = [], 0
        chunk = chunk + comps[i]
        size += cs
    if chunk:
        units.append({"module": mod, "base": base, "home": hf, "funcs": chunk})

units.insert(0, {"module": "nurbs", "base": "knots", "home": "source/SMLib/src/NL_BasisBasic.cpp",
                 "funcs": sorted(leaf), "fixed_id": "nlib.knots"})


def usize(un):
    return sum(L(F[u]) for u in un["funcs"])


# ----------------------------------------------------------------------------------------------
# merge small units into the same-module unit they are most connected to
def unit_index():
    return {u: i for i, un in enumerate(units) for u in un["funcs"]}


for _ in range(6):
    uidx = unit_index()
    merged_any = False
    order = sorted(range(len(units)), key=lambda i: usize(units[i]))
    dead = set()
    for i in order:
        un = units[i]
        if i in dead or un.get("fixed_id") or usize(un) >= MIN_UNIT:
            continue
        w = collections.Counter()
        for u in un["funcs"]:
            for v in SUCC_PORT[u]:
                j = uidx[v]
                if j != i and j not in dead:
                    w[j] += 2  # prefer merging into a callee (keeps the DAG shallow)
            for p in PRED[u]:
                j = uidx[p]
                if j != i and j not in dead:
                    w[j] += 1
        cands = [j for j in w if units[j]["module"] == un["module"] and not units[j].get("fixed_id")
                 and usize(units[j]) + usize(un) <= MAX_UNIT]
        if not cands:
            cands = [j for j in range(len(units)) if j != i and j not in dead and units[j]["module"] == un["module"]
                     and not units[j].get("fixed_id") and usize(units[j]) + usize(un) <= MAX_UNIT]
            if not cands:
                continue
            j = min(cands, key=lambda j: (usize(units[j]), j))
        else:
            j = max(cands, key=lambda j: (w[j], -usize(units[j]), -j))
        units[j]["funcs"] = sorted(units[j]["funcs"] + un["funcs"])
        units[j].setdefault("absorbed", []).append(un["home"])
        for u in un["funcs"]:
            uidx[u] = j
        dead.add(i)
        merged_any = True
    units = [un for k, un in enumerate(units) if k not in dead]
    if not merged_any:
        break

# ----------------------------------------------------------------------------------------------
# naming
name_count = collections.Counter((un["module"], snake(un["base"])) for un in units)
seen = collections.Counter()
units.sort(key=lambda un: (un["module"], snake(un["base"]), min(F[u]["line"] for u in un["funcs"])))
for un in units:
    if un.get("fixed_id"):
        un["id"] = un["fixed_id"]
        un["rust_module"] = "nurbs::knots"
        continue
    b = snake(un["base"]) or "misc"
    key = (un["module"], b)
    seen[key] += 1
    prefix = "nlib" if un["home"].split("/")[-1].startswith("NL_") else un["module"]
    if un["home"].startswith("source/SM_API/"):
        b = "api_" + b
        prefix = un["module"]
    if name_count[key] > 1:
        un["id"] = "%s.%s_p%d" % (prefix, b, seen[key])
        un["rust_module"] = "%s::%s::p%d" % (un["module"], b, seen[key])
    else:
        un["id"] = "%s.%s" % (prefix, b)
        un["rust_module"] = "%s::%s" % (un["module"], b)
ids = [un["id"] for un in units]
assert len(ids) == len(set(ids)), [x for x in ids if ids.count(x) > 1]

uidx = unit_index()
UID = {u: units[i]["id"] for u, i in uidx.items()}
BYID = {un["id"]: un for un in units}

# ----------------------------------------------------------------------------------------------
# unit graph, cycle breaking
# Port-order dependencies use *static* callees only: a virtual call depends on the declared
# (interface) method, not on every override -- with concrete geometry enums the dispatch `match`
# grows as variants land, and difftests feed only already-ported variants. Dispatch targets are
# still part of the closure (est_lines, in-scope) and of test reachability.
STATIC_PORT = {u: {v for v in callees(u, dispatch=False) if v in PORT} for u in PORT}
W = collections.defaultdict(collections.Counter)
for u in PORT:
    for v in STATIC_PORT[u]:
        a, b = UID[u], UID[v]
        if a != b:
            W[a][b] += 1
MODULE_RANK = {m: i for i, m in enumerate(
    ["geom", "nurbs", "curve", "surface", "intersect", "topology", "mass", "query", "heal", "primitive",
     "sweep", "boolean", "offset", "blend"])}


def fas_order(cp):
    """Eades-Lin-Smyth greedy feedback-arc-set; returns callees-first order."""
    rem = set(cp)
    s1, s2 = [], []

    def outw(a):
        return sum(w for b, w in W[a].items() if b in rem)

    def inw(a):
        return sum(W[b][a] for b in rem if a in W[b])

    while rem:
        moved = True
        while moved:
            moved = False
            for a in sorted(rem):
                if a in rem and outw(a) == 0:
                    s2.insert(0, a)
                    rem.discard(a)
                    moved = True
            for a in sorted(rem):
                if a in rem and inw(a) == 0:
                    s1.append(a)
                    rem.discard(a)
                    moved = True
        if rem:
            a = max(sorted(rem), key=lambda x: outw(x) - inw(x))
            s1.append(a)
            rem.discard(a)
    return list(reversed(s1 + s2))


ucomps = tarjan(sorted(BYID), lambda a: set(W[a]))
back = collections.defaultdict(dict)
scc_of = {}
for ci_, cp in enumerate(ucomps):
    for a in cp:
        scc_of[a] = ci_
    if len(cp) == 1:
        continue
    # Order by module rank first (math < nurbs < curve < surface < ... < operations); within a rank
    # use an Eades-Lin-Smyth greedy FAS on the weighted subgraph.
    seq = []
    for rk in sorted({MODULE_RANK[BYID[a]["module"]] for a in cp}):
        seq += fas_order([a for a in cp if MODULE_RANK[BYID[a]["module"]] == rk])
    pos = {a: i for i, a in enumerate(seq)}
    for a in cp:
        for b, w in W[a].items():
            if b in pos and pos[b] > pos[a]:  # a would depend on a later unit -> back-edge
                back[a][b] = w

DEPS = {a: sorted(b for b in W[a] if b not in back[a]) for a in BYID}

# layers
layer = {}


def lay(a, stack=()):
    if a in layer:
        return layer[a]
    layer[a] = 1 + max((lay(b) for b in DEPS[a]), default=-1)
    return layer[a]


sys.setrecursionlimit(20000)
for a in sorted(BYID):
    lay(a)
# critical path (weighted by est_lines)
est = {a: usize(BYID[a]) for a in BYID}
cp_cost, cp_next = {}, {}
for a in sorted(BYID, key=lambda x: layer[x]):
    best = max(DEPS[a], key=lambda b: (cp_cost[b], b), default=None)
    cp_cost[a] = est[a] + (cp_cost[best] if best else 0)
    cp_next[a] = best

# ----------------------------------------------------------------------------------------------
# tests: closure of each test function over the whole graph
test_funcs = sorted(u for u, f in F.items() if f["file"].startswith("tests/"))
FULL = {}


def full_succ(u):
    if u in FULL:
        return FULL[u]
    out = set()
    for x in E.get(u, ()):
        nv = x.startswith("NV:")
        for t in RES.get(x, []):
            out.add(t)
            if not nv and F[t]["virtual"]:
                out.update(OV.get(t, ()))
    FULL[u] = out
    return out


src_cache = {}


def src_lines(path):
    if path not in src_cache:
        with open(os.path.join(UP, path), errors="replace") as fh:
            src_cache[path] = fh.read().split("\n")
    return src_cache[path]


FIX_RX = re.compile(r"pt_TestFiles/([A-Za-z0-9_./ -]+\.[A-Za-z0-9]+)")
test_info = {}
for t in test_funcs:
    f = F[t]
    # only test entry points: functions not called by other test functions
    test_info[t] = None
callers_in_tests = set()
for t in test_funcs:
    for x in E.get(t, ()):
        for v in RES.get(x, []):
            if F[v]["file"].startswith("tests/"):
                callers_in_tests.add(v)
TEST_RX = re.compile(r"^(Test\w+|my_(test|local_op|many_prim|boolean_stress|offset|shell|sweep|primsol)\w*|"
                     r"my_\w*(regression|demo|stress|suite_\d+)|ShellBrep|airfoil_offset)$")
entry_tests = [t for t in test_funcs if TEST_RX.match(F[t]["name"])]
unit_tests = collections.defaultdict(list)
for t in entry_tests:
    seen_ = set()
    st = [t]
    while st:
        x = st.pop()
        if x in seen_:
            continue
        seen_.add(x)
        st.extend(full_succ(x))
    us = {UID[x] for x in seen_ if x in UID}
    lines = src_lines(F[t]["file"])[F[t]["line"] - 1:F[t]["end"]]
    # fixtures referenced by the test and the test-local helpers it calls
    fx = set()
    for x in seen_:
        if F[x]["file"].startswith("tests/"):
            for ln in src_lines(F[x]["file"])[F[x]["line"] - 1:F[x]["end"]]:
                for m in FIX_RX.finditer(ln):
                    fx.add(m.group(1))
    tname = "%s:%s" % (F[t]["file"].split("/")[1], F[t]["name"])
    for a in us:
        unit_tests[a].append((len(seen_), tname, sorted(fx)))

# ----------------------------------------------------------------------------------------------
# hazards
HAZ_PATTERNS = [
    ("rng", re.compile(r"\b(s?rand|drand48|random)\s*\(")),
    ("clock", re.compile(r"\b(clock|time|gettimeofday)\s*\(\s*(NULL|nullptr|0)?\s*\)")),
    ("ptr-hash", re.compile(r"(unordered_(map|set)\s*<\s*(const\s+)?\w+\s*\*|SmMapPtrToPtrs?\s*<|"
                            r"std::(map|set|multimap)\s*<\s*(const\s+)?\w+\s*\*)")),
    ("ptr-iter", re.compile(r"\b(GetAllKeys|GetAllValues|GetAllKeyValuePairs|Get1stKeyFor)\s*\(|"
                            r"for\s*\([^;]*:\s*\*?\s*[sm]_?\w*(Set|Map|Edges|Vertices|Edgeuses)\w*\s*\)")),
    ("ptr-order", re.compile(r"\b(qsort|std::sort|bsearch)\s*\(|\(\s*(ULONG_PTR|uintptr_t|intptr_t|"
                             r"size_t|unsigned long long|long long)\s*\)\s*\(?\s*[pP]")),
    ("tbb", re.compile(r"\btbb::|SM_USE_TBB|task_arena|parallel_for")),
    ("thread_local", re.compile(r"\b(thread_local|SM_THREAD_LOCAL|SmThreadLocal\w*)\b")),
]
hazards = collections.defaultdict(list)  # kind -> (unit, func, file, line, text)
for u in sorted(R):
    f = F[u]
    lines = src_lines(f["file"])
    dead_pp = inactive_lines(f["file"])
    for i in range(f["line"] - 1, min(f["end"], len(lines))):
        if (i + 1) in dead_pp:
            continue
        ln = lines[i]
        s = ln.strip()
        if s.startswith("//") or s.startswith("*"):
            continue
        for kind, rx in HAZ_PATTERNS:
            if rx.search(ln):
                hazards[kind].append((UID.get(u, "(idiom)"), f["name"], f["file"], i + 1, s[:110]))
mut_globals = []
for gl in GLOBS:
    if gl["const"] or "const " in gl["type"].split("[")[0][:6]:
        continue
    if gl["kind"] == "static_local":
        if gl["in"] in R:
            mut_globals.append((UID.get(gl["in"], "(idiom)"), F[gl["in"]]["name"], gl))
    elif gl["file"].startswith("source/"):
        hf = gl["file"]
        if FILES.get(hf, {}).get("cha_lines", 0) > 0 or hf.endswith(".h"):
            mut_globals.append(("", "", gl))
dedup = {}
for unit, fn, gl in mut_globals:
    dedup[(gl["file"], gl["line"], gl["name"])] = (unit, fn, gl)
mut_globals = [dedup[k] for k in sorted(dedup)]

# ----------------------------------------------------------------------------------------------
# write units.json
os.makedirs(os.path.join(HERE), exist_ok=True)
out_units = {}
for un in units:
    out_units[un["id"]] = {
        "module": un["module"], "rust_module": un["rust_module"],
        "upstream": sorted({F[u]["file"] for u in un["funcs"]}),
        "functions": [{"name": F[u]["name"], "file": F[u]["file"], "line": F[u]["line"], "lines": L(F[u]),
                       "usr": u} for u in sorted(un["funcs"], key=lambda x: (F[x]["file"], F[x]["line"]))],
    }
json.dump(out_units, open(os.path.join(HERE, "units.json"), "w"), indent=1, sort_keys=True)


# ----------------------------------------------------------------------------------------------
# plan/units/<id>.toml
def unit_tests_for(a, k=6):
    ts = sorted(set((n, t) for n, t, fx in unit_tests.get(a, [])))
    picked = [t for n, t in ts[:k]]
    fx = []
    for n, t, f in sorted(unit_tests.get(a, [])):
        if t in picked:
            for x in f:
                if ("pt_TestFiles/" + x) not in fx:
                    fx.append("pt_TestFiles/" + x)
    return picked + fx[:6], len(ts)


SM_API_GROUP_ROOTS = collections.defaultdict(list)
for r in ROOTS:
    SM_API_GROUP_ROOTS[UID.get(r, "")].append(F[r]["name"])

# One file per unit (plan/units/<id>.toml, harness/plan/units.py). Regenerating resets
# statuses/claims/notes: do it only with care (ids are stable for a given closure).
# est_lines = in-scope (reachable) upstream function lines in the unit.
sys.path.insert(0, os.path.join(ROOT, "harness", "plan"))
import units as plan_units  # noqa: E402

plan_out = {}
for un in sorted(units, key=lambda x: (layer[x["id"]], x["id"])):
    a = un["id"]
    up = sorted({F[u]["file"] for u in un["funcs"]})
    tests, ntests = unit_tests_for(a)
    notes = []
    names = sorted({F[u]["name"] for u in un["funcs"]},
                   key=lambda n: (-max(L(F[u]) for u in un["funcs"] if F[u]["name"] == n), n))
    notes.append("%d fns; largest: %s" % (len(un["funcs"]), ", ".join(names[:4])))
    if SM_API_GROUP_ROOTS.get(a):
        notes.append("entry points: " + ", ".join(sorted(set(SM_API_GROUP_ROOTS[a]))))
    if back[a]:
        notes.append("cycle back-edges (call into later units; stub or co-port): " +
                     ", ".join("%s(%d)" % (b, w) for b, w in sorted(back[a].items(), key=lambda kv: (-kv[1], kv[0]))))
    if len(ucomps[scc_of[a]]) > 1:
        notes.append("unit-SCC #%d (%d units)" % (scc_of[a], len(ucomps[scc_of[a]])))
    rta_l = sum(L(F[u]) for u in un["funcs"] if u in RTA)
    if rta_l < 0.5 * usize(un):
        notes.append("only %d of %d lines survive RTA (reached via virtual dispatch to classes no in-scope "
                     "entry point constructs); low priority / likely deferrable" % (rta_l, usize(un)))
    hz = sorted({k for k, v in hazards.items() for x in v if x[0] == a})
    if hz:
        notes.append("hazards: " + ",".join(hz))
    if ntests > len([t for t in tests if not t.startswith("pt_")]):
        notes.append("%d upstream test entry points reach this unit" % ntests)
    plan_out[a] = {
        "id": a,
        "upstream": up,
        "rust_module": un["rust_module"],
        "depends_on": list(DEPS[a]),
        "est_lines": usize(un),
        "layer": layer[a],
        "tests": tests,
        "status": "todo",
        "claimed_by": "",
        "claimed_at": "",
        "notes": "; ".join(notes),
    }
units_dir = os.path.join(ROOT, "plan", "units")
os.makedirs(units_dir, exist_ok=True)
for f in os.listdir(units_dir):
    if f.endswith(".toml") and f[:-5] not in plan_out:
        os.remove(os.path.join(units_dir, f))
for a, u in plan_out.items():
    open(os.path.join(units_dir, a + ".toml"), "w").write(plan_units.dump_unit(u))

# ----------------------------------------------------------------------------------------------
# order.md
nl = max(layer.values()) + 1
per = collections.defaultdict(list)
for a in BYID:
    per[layer[a]].append(a)
crit_end = max(BYID, key=lambda a: (cp_cost[a], a))
path = []
x = crit_end
while x:
    path.append(x)
    x = cp_next[x]
o = ["# Port order (parallel layers)", "",
     "Generated by `harness/callgraph/plan_units.py` from the CHA call-graph closure. A unit in layer *k*",
     "depends only on units in layers < *k*, so every unit within a layer can be ported in parallel once",
     "the previous layers are green. Dependencies are static calls plus conservative virtual dispatch",
     "(a call to a virtual method depends on every in-scope override).", "",
     "**Cycles.** The kernel's curve/surface/topology core is mutually recursive (e.g. surface",
     "evaluation -> offset/blend surfaces -> curve-on-surface -> surface point solve -> surface",
     "evaluation). Unit-level strongly connected components were linearised with a weighted greedy",
     "feedback-arc-set; the dropped *back-edges* are listed in each unit's `notes` in",
     "`plan/units/<id>.toml`. A unit with back-edges can be ported in its layer, but its difftest can only",
     "cover paths whose callees are already ported; full side-by-side validation of such a unit is",
     "re-run when the back-edge targets turn green (keep `status = \"claimed\"` until then or record",
     "the partial coverage in `plan/findings.md`).", "",
     "| layer | units | est. upstream lines | max unit |", "|---:|---:|---:|---:|"]
tot = 0
for k in range(nl):
    s = sum(est[a] for a in per[k])
    tot += s
    o.append("| %d | %d | %d | %d |" % (k, len(per[k]), s, max(est[a] for a in per[k])))
o.append("| **total** | **%d** | **%d** | |" % (len(BYID), tot))
o += ["", "## Critical path", "",
      "Longest dependency chain weighted by `est_lines` (%d units, %d upstream lines):" % (len(path), cp_cost[crit_end]),
      ""]
for a in reversed(path):
    o.append("1. `%s` (layer %d, %d lines)" % (a, layer[a], est[a]))
o += ["", "## Unit-level cycles", ""]
big = [cp for cp in ucomps if len(cp) > 1]
o.append("%d strongly connected unit groups; sizes: %s. Total back-edges dropped: %d." % (
    len(big), ", ".join(str(len(cp)) for cp in sorted(big, key=len, reverse=True)),
    sum(len(v) for v in back.values())))
for cp in sorted(big, key=len, reverse=True):
    o.append("")
    o.append("- SCC #%d (%d units, %d lines): %s" % (scc_of[cp[0]], len(cp), sum(est[a] for a in cp),
                                                     ", ".join("`%s`" % a for a in sorted(cp, key=lambda a: layer[a]))))
o += ["", "## Layers", ""]
for k in range(nl):
    o.append("### Layer %d (%d units, %d lines)" % (k, len(per[k]), sum(est[a] for a in per[k])))
    o.append("")
    o.append(", ".join("`%s` (%d)" % (a, est[a]) for a in sorted(per[k])))
    o.append("")
open(os.path.join(ROOT, "plan", "order.md"), "w").write("\n".join(o))

# ----------------------------------------------------------------------------------------------
# summary json for closure.md writer
summary = {
    "units": len(BYID), "layers": nl, "inscope_lines": tot,
    "idiom_lines": sum(L(F[u]) for u in IDIOM),
    "leaf": "nlib.knots", "leaf_lines": est["nlib.knots"], "leaf_deps": DEPS["nlib.knots"],
    "leaf_funcs": sorted(F[u]["name"] for u in leaf),
    "critical_path": list(reversed(path)), "critical_lines": cp_cost[crit_end],
    "hazards": {k: v for k, v in hazards.items()},
    "mut_globals": [(a, fn, gl) for a, fn, gl in mut_globals],
    "back_edges": sum(len(v) for v in back.values()),
    "sccs": [sorted(cp) for cp in big],
    "module_lines": dict(collections.Counter({m: 0 for m in []})),
    "entry_tests": len(entry_tests),
}
ml = collections.Counter()
mu = collections.Counter()
for un in units:
    ml[un["module"]] += usize(un)
    mu[un["module"]] += 1
summary["module_lines"] = dict(ml)
summary["module_units"] = dict(mu)
summary["over_max"] = [(un["id"], usize(un)) for un in units if usize(un) > MAX_UNIT]
summary["under_min"] = [(un["id"], usize(un)) for un in units if usize(un) < MIN_UNIT]
json.dump(summary, open(os.path.join(D, "plan_summary.json"), "w"), indent=1)
print(json.dumps({k: summary[k] for k in ("units", "layers", "inscope_lines", "idiom_lines", "leaf_lines",
                                          "leaf_deps", "critical_lines", "back_edges", "module_units",
                                          "module_lines", "over_max", "under_min", "entry_tests")}, indent=0))
print("leaf funcs", summary["leaf_funcs"])
print("sccs", [len(x) for x in summary["sccs"]])
