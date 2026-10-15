// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.loopuse`: the SmLoop and SmLoopuse members owned by
// the unit (source/SMLib/src/SmLoop.cpp, source/SMLib/inc/SmLoop.h,
// source/SMLib/src/SmLoopuse.cpp, source/SMLib/inc/SmLoopuse.h) on real SmBrep objects.
//
// One entry point, sc_topology_loopuse_eval(op, path, in, n_in, out, cap, out_len). Every
// op starts with the B-rep block of unit topology.edgeuse_p1 (built and exported by the
// shared sc_edgeuse_graph.h; the export itself is topology.edgeuse_p1's Export op).
// Layouts are documented in crates/solidcore-ffi/src/units/topology_loopuse.rs, which
// validates every input. Outputs start with `0`, or `-1 status` when the B-rep could not
// be built. In an output slot `-2` is "upstream would dereference null (or read memory
// of the wrong kind)" and `-3` "not run": the op would reach a callee that is unported
// on the Rust side (whose cases are deferred), so the shim does not call it.
//
// The guards below (ok_*, has_prim, ok_sector) mirror what upstream dereferences without
// checking; the sector guard is that of the topology.vertexuse shim.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAttribute.h>
#include <SmBrep.h>
#include <SmCurve.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFace.h>
#include <SmFaceuse.h>
#include <SmLoop.h>
#include <SmLoopuse.h>
#include <SmRegion.h>
#include <SmShell.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmTol.h>
#include <SmVertex.h>
#include <SmVertexuse.h>

#include <cstring>
#include <vector>

#include "sc_edgeuse_graph.h"

namespace
{
enum OpLu : int32_t
{
    OP_LU_QUERY = 1,
    OP_LU_LISTS = 2,
    OP_LU_CONNECTED = 3,
    OP_LOOP_QUERY = 4,
    OP_LOOP_CONNECTED = 5,
    OP_CLOSED3D = 6,
    OP_MISSING_SEAM = 7,
    OP_UV_CORNERS = 8,
    OP_FLIP = 9,
    OP_COLLECT = 10,
    OP_SET_EDGEUSE = 11,
    OP_SET_ORIENT = 12,
    OP_SET_LOOP = 13,
    OP_FACE_FU = 14,
};

const double ST_NULL_DEREF = -2.0;
const double ST_NOT_RUN = -3.0;

// The attribute id the LuLists op attaches to its selected objects.
const ULONG ATTR_ID = 777;

const SM_TYPE KIND_TYPES[] = {SmLoopuse_TYPE,        SmLoop_TYPE,    SmTopology_TYPE, SmEdgeuse_TYPE,
                              SmOwningTopology_TYPE, SmAObject_TYPE, SmObject_TYPE,   SmUnknown_TYPE,
                              0};

void put_kinds(std::vector<double>& o, const SmObject* p)
{
    for (const SM_TYPE* t = KIND_TYPES; *t != 0; t++)
        o.push_back(flg(p->IsKindOf(*t)));
}

// A string as its length and character codes.
void put_str(std::vector<double>& o, const TCHAR* s)
{
    std::vector<double> c;
    for (const TCHAR* q = s; q && *q; q++)
        c.push_back((double)*q);
    o.push_back((double)c.size());
    o.insert(o.end(), c.begin(), c.end());
}

struct FcPeek : SmFace
{
    static SmFaceuse* SmFace::*fu() { return &FcPeek::m_pFU; }
};

struct EdTolPeek : SmEdge
{
    static SmZoneTol3d SmEdge::*tol() { return &EdTolPeek::m_sZoneTol3d; }
};

struct EuTypePeek : SmEdgeuse
{
    static SM_TYPE SmEdgeuse::*type() { return &EuTypePeek::m_tEdgeuseType; }
};

// The links the mutating ops change, restored before the B-rep is deleted (its
// destructor walks them; a loop use left pointing at another loop, say, is freed twice).
struct Snapshot
{
    struct Lu
    {
        SM_TYPE type;
        SmTopology* start;
        SmOrientType orient;
        SmLoop* loop;
    };
    std::vector<Lu> lus;
    std::vector<std::pair<SM_TYPE, SmTopology*>> eus;
    std::vector<SmTopology*> vus;

    void take(Graph& g)
    {
        for (SmLoopuse* p : g.loopuses)
            lus.push_back({p->*LuPeek::type(), p->*LuPeek::start(), p->*LuPeek::orient(), p->*LuPeek::loop()});
        for (SmEdgeuse* p : g.edgeuses)
            eus.push_back({p->*EuTypePeek::type(), p->*EuPeek::sorlu()});
        for (SmVertexuse* p : g.vertexuses)
            vus.push_back(p->*VuPeek::owner());
    }
    void restore(Graph& g)
    {
        for (size_t i = 0; i < lus.size(); i++)
        {
            SmLoopuse* p = g.loopuses[i];
            p->*LuPeek::type() = lus[i].type;
            p->*LuPeek::start() = lus[i].start;
            p->*LuPeek::orient() = lus[i].orient;
            p->*LuPeek::loop() = lus[i].loop;
        }
        for (size_t i = 0; i < eus.size(); i++)
        {
            g.edgeuses[i]->*EuTypePeek::type() = eus[i].first;
            g.edgeuses[i]->*EuPeek::sorlu() = eus[i].second;
        }
        for (size_t i = 0; i < vus.size(); i++)
            g.vertexuses[i]->*VuPeek::owner() = vus[i];
    }
};

SmTopology* start_of(SmLoopuse* p) { return p->*LuPeek::start(); }
SM_TYPE type_of(SmLoopuse* p) { return p->*LuPeek::type(); }

// The CCW walk of an edge loop use (the walk of a vertex loop use is empty); `closes`
// false when it neither reaches NULL nor returns to its start within the B-rep's edge
// use count (upstream loops forever).
std::vector<SmEdgeuse*> ccw_walk(SmLoopuse* p, size_t n_eu, bool& closes)
{
    std::vector<SmEdgeuse*> v;
    closes = true;
    if (type_of(p) == SmVertexuse_TYPE)
        return v;
    SmEdgeuse* start = SM_CAST_PTR(SmEdgeuse, start_of(p));
    SmEdgeuse* q = start;
    while (q != nullptr)
    {
        if (v.size() > n_eu)
        {
            closes = false;
            return v;
        }
        v.push_back(q);
        q = q->GetCCWEdgeuse();
        if (q == start)
            break;
    }
    return v;
}

// GetMate needs the edge's primary edge use (loop edge uses).
bool has_prim(SmEdgeuse* e)
{
    return e->GetEdgeuseType() == SmShell_TYPE || (e->GetEdge() && e->GetEdge()->GetPrimaryEdgeuse());
}

// ---- vertex use guards (copy of the topology.vertexuse shim's) -------------------------

SmTopology* owner_of(SmVertexuse* p) { return p->*VuPeek::owner(); }
bool ok_faceuse(SmVertexuse* p) { return !p->IsLoopVertexuse() || owner_of(p) != nullptr; }
bool ok_loopuse(SmVertexuse* p) { return !p->IsEdgeVertexuse() || owner_of(p) != nullptr; }
bool ok_shell(SmVertexuse* p)
{
    if (!p->IsLoopVertexuse() || owner_of(p) == nullptr)
        return true;
    return ((SmLoopuse*)owner_of(p))->GetFaceuse() != nullptr;
}

bool ok_sector(SmVertexuse* p)
{
    if (p->IsShellVertexuse())
        return true;
    if (!ok_loopuse(p) || !ok_faceuse(p))
        return false;
    SmVertex* v = p->GetVertex();
    SmFaceuse* fu = p->GetFaceuse();
    SmFace* f = fu ? fu->GetFace() : nullptr;
    if (v == nullptr || f == nullptr || f->GetSurface() == nullptr)
        return true;
    if (!p->IsEdgeVertexuse())
        return true;
    SmEdgeuse* beg = p->GetEdgeuse();
    if (beg->IsLoopEdgeuse())
    {
        if (beg->GetVertexuse() == nullptr || !has_prim(beg))
            return false;
        if (beg->GetVertexuse()->GetVertex() != v &&
            (beg->GetMate() == nullptr || beg->GetMate()->GetVertexuse() == nullptr))
            return false;
    }
    SmEdgeuse* end = beg->GetCornerMateEdgeuse(v);
    if (end == nullptr || end->GetVertexuse() == nullptr)
        return false;
    SmVertexuse* evu = end->GetVertexuse();
    if (evu->GetVertex() != v)
    {
        if (!has_prim(end) || end->GetMate() == nullptr || end->GetMate()->GetVertexuse() == nullptr)
            return false;
        evu = end->GetMate()->GetVertexuse();
    }
    if (evu->GetVertex() == nullptr || !ok_faceuse(evu))
        return false;
    return p->GetLoopuse() != nullptr || fu->GetOrientation() == SM_OT_OPPOSITE;
}

// SmLoopuse::GetVertices, reached by SmVertex::IsConnectedTo with a loop or loop use
// target: a vertex-use loop use reads its start, an edge loop use the vertex use of
// every edge use of its CCW chain (null = no loop use, dereferenced through SmLoop).
const int64_t MAX_LU_WALK = 100000;
bool ok_lu_vertices(SmLoopuse* lu)
{
    if (lu == nullptr)
        return false;
    SM_TYPE ty = lu->*LuPeek::type();
    SmTopology* start = lu->*LuPeek::start();
    if (ty == SmVertexuse_TYPE)
        return start != nullptr;
    if (ty != SmEdgeuse_TYPE)
        return start == nullptr;
    SmEdgeuse* eu = (SmEdgeuse*)start;
    for (int64_t k = 0; eu != nullptr; k++)
    {
        if (k > MAX_LU_WALK || eu->GetVertexuse() == nullptr)
            return false;
        eu = eu->GetCCWEdgeuse();
        if (eu == (SmEdgeuse*)start)
            return true;
    }
    return true;
}

// SmVertex::IsConnectedTo / SmEdge::IsConnectedTo reached from the loop use `lu` (of
// `self`, a loop or the loop use): the vertex walks the loop use's vertices, the edge
// its edges (through SmLoop::m_pLU for a loop).
bool ok_ve_connected(SmLoopuse* lu, int64_t ty)
{
    if (ty == SmVertex_TYPE)
        return ok_lu_vertices(lu);
    return lu != nullptr;
}

// SmVertexuse::IsConnectedTo of `vu` against a loop or loop use target.
double vu_connected(SmVertexuse* vu, SmTopology* t)
{
    return ok_loopuse(vu) ? flg(vu->IsConnectedTo(t)) : ST_NULL_DEREF;
}

// SmEdgeuse::IsConnectedTo of `eu`: a face target dereferences a loop edge use's face use.
bool ok_eu_connected(SmEdgeuse* eu, int64_t ty)
{
    if (ty != SmFace_TYPE || !eu->IsLoopEdgeuse())
        return true;
    SmLoopuse* lu = eu->GetLoopuse();
    return lu == nullptr || lu->GetFaceuse() != nullptr;
}

// ---- outputs ------------------------------------------------------------------------------

// The loop/edge/vertex use links the mutating ops change, in the pre-op traversal order:
// per loop use `type start orientation loop faceuse`, per edge use `owner_type owner`,
// per vertex use `owner_type owner`.
void put_use_state(std::vector<double>& o, Graph& g)
{
    for (SmLoopuse* p : g.loopuses)
    {
        SM_TYPE ty = type_of(p);
        o.push_back((double)ty);
        SmTopology* s = start_of(p);
        o.push_back((double)(ty == SmEdgeuse_TYPE ? g.find(6, s) : ty == SmVertexuse_TYPE ? g.find(8, s) : -1));
        o.push_back((double)(p->*LuPeek::orient()));
        o.push_back((double)g.find(5, p->*LuPeek::loop()));
        o.push_back((double)g.find(2, p->GetOwner()));
    }
    for (SmEdgeuse* p : g.edgeuses)
    {
        SM_TYPE ty = p->GetEdgeuseType();
        o.push_back((double)ty);
        SmTopology* s = p->*EuPeek::sorlu();
        o.push_back((double)(ty == SmLoopuse_TYPE ? g.find(4, s) : ty == SmShell_TYPE ? g.find(1, s) : -1));
    }
    for (SmVertexuse* p : g.vertexuses)
    {
        SM_TYPE ty = p->GetVertexuseType();
        o.push_back((double)ty);
        SmTopology* s = owner_of(p);
        o.push_back((double)(ty == SmEdgeuse_TYPE    ? g.find(6, s)
                             : ty == SmLoopuse_TYPE ? g.find(4, s)
                             : ty == SmShell_TYPE   ? g.find(1, s)
                                                    : -1));
    }
}

// The four member lists of a loop use (`GetEdges`, `GetEdgeuses`, `GetVertexuses`,
// `GetVertices`), each `-2` where upstream would dereference null or loop forever.
void put_lists(std::vector<double>& o, Graph& g, SmLoopuse* p, const ULONG* attr, SmTopology* conn, int64_t conn_ty)
{
    bool closes = true;
    std::vector<SmEdgeuse*> walk = ccw_walk(p, g.edgeuses.size(), closes);
    bool vertex = type_of(p) == SmVertexuse_TYPE;

    // GetEdges: a null edge is only dereferenced to test the attribute
    bool ok = closes;
    for (SmEdgeuse* q : walk)
        if (attr && q->GetEdge() == nullptr)
            ok = false;
    if (ok)
    {
        SmTArray<SmEdge*> v;
        p->GetEdges(v, (ULONG*)attr);
        std::vector<int64_t> ix;
        for (ULONG k = 0; k < v.GetSize(); k++)
            ix.push_back(g.find(7, v[k]));
        put_list(o, ix);
    }
    else
        o.push_back(ST_NULL_DEREF);

    // GetEdgeuses
    ok = closes;
    for (SmEdgeuse* q : walk)
        if (conn && !ok_eu_connected(q, conn_ty))
            ok = false;
    if (ok)
    {
        SmTArray<SmEdgeuse*> v;
        p->GetEdgeuses(v, (ULONG*)attr, conn);
        std::vector<int64_t> ix;
        for (ULONG k = 0; k < v.GetSize(); k++)
            ix.push_back(g.find(6, v[k]));
        put_list(o, ix);
    }
    else
        o.push_back(ST_NULL_DEREF);

    // GetVertexuses: a null vertex use is only dereferenced to test the attribute
    ok = closes;
    if (vertex)
        ok = !(attr && start_of(p) == nullptr);
    for (SmEdgeuse* q : walk)
        if (attr && q->GetVertexuse() == nullptr)
            ok = false;
    if (ok)
    {
        SmTArray<SmVertexuse*> v;
        p->GetVertexuses(v, (ULONG*)attr);
        std::vector<int64_t> ix;
        for (ULONG k = 0; k < v.GetSize(); k++)
            ix.push_back(g.find(8, v[k]));
        put_list(o, ix);
    }
    else
        o.push_back(ST_NULL_DEREF);

    // GetVertices: the vertex use is always dereferenced, a null vertex only to test
    // the attribute
    ok = closes;
    if (vertex)
    {
        SmVertexuse* vu = (SmVertexuse*)start_of(p);
        ok = vu != nullptr && !(attr && vu->GetVertex() == nullptr);
    }
    for (SmEdgeuse* q : walk)
        if (q->GetVertexuse() == nullptr || (attr && q->GetVertexuse()->GetVertex() == nullptr))
            ok = false;
    if (ok)
    {
        SmTArray<SmVertex*> v;
        p->GetVertices(v, (ULONG*)attr);
        std::vector<int64_t> ix;
        for (ULONG k = 0; k < v.GetSize(); k++)
            ix.push_back(g.find(9, v[k]));
        put_list(o, ix);
    }
    else
        o.push_back(ST_NULL_DEREF);
}

// IsClosed3d's loop with only the checks: whether upstream dereferences a null edge or
// curve before it finishes (`check_all` FALSE stops at the first gap over tolerance).
bool closed3d_safe(SmLoopuse* p, size_t n_eu, bool check_all)
{
    if (type_of(p) == SmVertexuse_TYPE)
        return true;
    bool closes = true;
    std::vector<SmEdgeuse*> eus = ccw_walk(p, n_eu, closes);
    if (!closes)
        return false;
    size_t n = eus.size();
    for (size_t i = 0; i < n; i++)
    {
        SmEdgeuse* a = eus[i];
        SmEdgeuse* b = eus[(i + 1) % n];
        if (a->GetEdge() == nullptr || a->GetEdge()->GetCurve() == nullptr || b->GetEdge() == nullptr ||
            b->GetEdge()->GetCurve() == nullptr)
            return false;
        if (check_all)
            continue;
        SmPoint3d pa(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE), pb = pa;
        SmExtent1d ia = a->GetEdge()->GetInterval(), ib = b->GetEdge()->GetInterval();
        a->GetEdge()->GetCurve()->EvaluatePoint(a->GetOrientation() == SM_OT_SAME ? ia.GetMax() : ia.GetMin(), pa);
        b->GetEdge()->GetCurve()->EvaluatePoint(b->GetOrientation() == SM_OT_SAME ? ib.GetMin() : ib.GetMax(), pb);
        // the same tolerance upstream tests against (SmLoopuse.cpp:860)
        SmXSectTol3d tol = SmTol::GetXSectTol3d(SmTol::GetZoneTol3d(a), SmTol::GetZoneTol3d(b));
        if (pa.DistanceBetweenSquared(pb) > tol * tol)
            return true;
    }
    return true;
}

void put_closed(std::vector<double>& o, Graph& g, SmBoolean r, SmEdgeuse* eu, double gap, double tol)
{
    o.push_back(flg(r));
    o.push_back(eu == (SmEdgeuse*)0x1 ? -5.0 : (double)g.find(6, eu));
    o.push_back(gap);
    o.push_back(tol);
}

// SmLoop::IsConnectedTo for shell/region targets: what it dereferences, in its order.
double loop_connected_shell(SmLoop* l, SmTopology* t, bool region)
{
    SmLoopuse* lu = l->GetLoopuse();
    if (lu == nullptr || lu->GetFaceuse() == nullptr || lu->GetFaceuse()->GetFace() == nullptr)
        return ST_NULL_DEREF;
    SmFaceuse *f1, *f2;
    lu->GetFaceuse()->GetFace()->GetFaceuses(f1, f2);
    if (f1 == nullptr || (region && f1->GetShell() == nullptr))
        return ST_NULL_DEREF;
    bool first = region ? f1->GetShell()->GetRegion() == (SmRegion*)t : f1->GetShell() == (SmShell*)t;
    if (!first && (f2 == nullptr || (region && f2->GetShell() == nullptr)))
        return ST_NULL_DEREF;
    return flg(l->IsConnectedTo(t));
}

SmTopology* read_target(Rd& rd, Graph& g, int64_t& ty)
{
    ty = rd.idx();
    int64_t i = rd.idx();
    return target_of(g, ty, i);
}

void run_graph_op(int32_t op, Rd& rd, Graph& g, size_t n_eu, std::vector<double>& o);

void run_op(int32_t op, Rd& rd, const char* path, std::vector<double>& o)
{
    SmStatus bst;
    Built b;
    b.brep = build(rd, path, bst);
    if (b.brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(bst));
        return;
    }
    o.push_back(0.0);
    Graph g;
    walk(b.brep, g);
    size_t n_eu = g.edgeuses.size();
    Snapshot snap;
    snap.take(g);
    run_graph_op(op, rd, g, n_eu, o);
    snap.restore(g);
}

void run_graph_op(int32_t op, Rd& rd, Graph& g, size_t n_eu, std::vector<double>& o)
{
    switch (op)
    {
    case OP_LU_QUERY:
    {
        SmLoopuse* p = g.loopuses[(size_t)rd.idx()];
        o.push_back((double)g.find(8, p->GetVertexuse()));
        o.push_back((double)g.find(2, p->GetFaceuse()));
        o.push_back((double)g.find(5, p->GetLoop()));
        o.push_back((double)p->GetOrientation());
        o.push_back((double)g.find(4, p->GetOtherLoopuse()));
        SmTopology* s = p->GetEUorVU();
        SM_TYPE ty = p->GetLoopuseType();
        o.push_back(s == nullptr ? -1.0 : (double)ty);
        o.push_back(s == nullptr ? -1.0 : (double)g.typed(ty, s));
        o.push_back((double)ty);
        o.push_back(flg(p->IsVertexLoopuse()));
        o.push_back(flg(p->IsEdgeLoopuse()));
        o.push_back((double)g.find(1, p->GetShell()));
        bool closes = true;
        ccw_walk(p, n_eu, closes);
        o.push_back(closes ? flg(p->IsClosedPtrs()) : ST_NULL_DEREF);
        o.push_back((double)p->GetType());
        put_kinds(o, p);
        put_str(o, p->GetTypeString());
        put_str(o, p->GetClassString());
        put_lists(o, g, p, nullptr, nullptr, 0);
        break;
    }
    case OP_LU_LISTS:
    {
        SmLoopuse* p = g.loopuses[(size_t)rd.idx()];
        bool use_attr = rd.flag();
        int64_t n = rd.idx();
        for (int64_t k = 0; k < n; k++)
        {
            int64_t ty = rd.idx();
            int64_t i = rd.idx();
            SmTopology* t = target_of(g, ty, i);
            if (t != nullptr && t->FindAttribute(ATTR_ID) == nullptr)
                t->AddAttribute(new (the_context()) SmAttribute(ATTR_ID, SM_AB_STANDALONE_COPY));
        }
        int64_t cty = 0;
        SmTopology* conn = read_target(rd, g, cty);
        ULONG id = ATTR_ID;
        put_lists(o, g, p, use_attr ? &id : nullptr, conn, cty);
        break;
    }
    case OP_LU_CONNECTED:
    {
        SmLoopuse* p = g.loopuses[(size_t)rd.idx()];
        int64_t ty = 0;
        SmTopology* t = read_target(rd, g, ty);
        if (t == nullptr)
            o.push_back(flg(p->IsConnectedTo(t)));
        else if (ty == SmVertex_TYPE || ty == SmEdge_TYPE)
            o.push_back(ok_ve_connected(p, ty) ? flg(p->IsConnectedTo(t)) : ST_NULL_DEREF);
        else if (ty == SmVertexuse_TYPE)
            o.push_back(vu_connected((SmVertexuse*)t, p));
        else if (ty == SmEdgeuse_TYPE)
            o.push_back(flg(p->IsConnectedTo(t)));
        else if (ty == SmFace_TYPE && p->GetFaceuse() == nullptr)
            o.push_back(ST_NULL_DEREF);
        else
            o.push_back(flg(p->IsConnectedTo(t)));
        break;
    }
    case OP_LOOP_QUERY:
    {
        SmLoop* l = g.loops[(size_t)rd.idx()];
        SmLoopuse* lu = l->GetLoopuse();
        o.push_back((double)g.find(4, lu));
        bool face_ok = lu != nullptr && lu->GetFaceuse() != nullptr;
        o.push_back(face_ok ? (double)g.find(3, l->GetFace()) : ST_NULL_DEREF);
        if (lu != nullptr)
        {
            SmLoopuse *a, *c;
            l->GetLoopuses(a, c);
            o.push_back((double)g.find(4, a));
            o.push_back((double)g.find(4, c));
        }
        else
        {
            o.push_back(ST_NULL_DEREF);
            o.push_back(ST_NULL_DEREF);
        }
        bool outer_ok = face_ok;
        if (outer_ok)
        {
            SmTArray<SmLoopuse*> lus;
            lu->GetFaceuse()->GetLoopuses(lus);
            outer_ok = lus.GetSize() > 0;
        }
        o.push_back(outer_ok ? flg(l->IsOuterLoop()) : ST_NULL_DEREF);
        bool closes = true;
        if (lu != nullptr)
            ccw_walk(lu, n_eu, closes);
        o.push_back(closes ? flg(l->IsClosedPtrs()) : ST_NULL_DEREF);
        o.push_back((double)l->GetType());
        o.push_back((double)SmLoop::GetClassType());
        put_kinds(o, l);
        put_str(o, l->GetTypeString());
        put_str(o, l->GetClassString());
        if (lu == nullptr)
        {
            for (int k = 0; k < 3; k++)
                o.push_back(ST_NULL_DEREF);
            break;
        }
        // SmLoop::GetVertices / GetEdges / GetEdgeuses (guards of the loop use lists)
        std::vector<double> lists;
        put_lists(lists, g, lu, nullptr, nullptr, 0);
        // put_lists' order is edges, edge uses, vertex uses, vertices: run the loop's
        // own calls where the loop use's are defined
        size_t at = 0;
        auto skip = [&lists, &at]() -> bool {
            if (lists[at] == ST_NULL_DEREF)
            {
                at += 1;
                return false;
            }
            at += 1 + (size_t)lists[at];
            return true;
        };
        bool e_ok = skip();
        bool eu_ok = skip();
        skip();
        bool v_ok = skip();
        if (v_ok)
        {
            SmTArray<SmVertex*> v;
            l->GetVertices(v);
            std::vector<int64_t> ix;
            for (ULONG k = 0; k < v.GetSize(); k++)
                ix.push_back(g.find(9, v[k]));
            put_list(o, ix);
        }
        else
            o.push_back(ST_NULL_DEREF);
        if (e_ok)
        {
            SmTArray<SmEdge*> v;
            l->GetEdges(v);
            std::vector<int64_t> ix;
            for (ULONG k = 0; k < v.GetSize(); k++)
                ix.push_back(g.find(7, v[k]));
            put_list(o, ix);
        }
        else
            o.push_back(ST_NULL_DEREF);
        if (eu_ok)
        {
            SmTArray<SmEdgeuse*> v;
            l->GetEdgeuses(v);
            std::vector<int64_t> ix;
            for (ULONG k = 0; k < v.GetSize(); k++)
                ix.push_back(g.find(6, v[k]));
            put_list(o, ix);
        }
        else
            o.push_back(ST_NULL_DEREF);
        break;
    }
    case OP_LOOP_CONNECTED:
    {
        SmLoop* l = g.loops[(size_t)rd.idx()];
        int64_t ty = 0;
        SmTopology* t = read_target(rd, g, ty);
        SmLoopuse* lu = l->GetLoopuse();
        bool face_ok = lu != nullptr && lu->GetFaceuse() != nullptr;
        if (t == nullptr)
            o.push_back(flg(l->IsConnectedTo(t)));
        else if (ty == SmVertex_TYPE || ty == SmEdge_TYPE)
            o.push_back(ok_ve_connected(lu, ty) ? flg(l->IsConnectedTo(t)) : ST_NULL_DEREF);
        else if (ty == SmVertexuse_TYPE)
            o.push_back(vu_connected((SmVertexuse*)t, l));
        else if ((ty == SmFace_TYPE || ty == SmFaceuse_TYPE) && !face_ok)
            o.push_back(ST_NULL_DEREF);
        else if (ty == SmShell_TYPE || ty == SmRegion_TYPE)
            o.push_back(loop_connected_shell(l, t, ty == SmRegion_TYPE));
        else
            o.push_back(flg(l->IsConnectedTo(t)));
        break;
    }
    case OP_CLOSED3D:
    {
        bool of_loop = rd.flag();
        int64_t i = rd.idx();
        SmBoolean all = rd.bflag();
        bool want_eu = rd.flag();
        bool want_gap = rd.flag();
        bool want_tol = rd.flag();
        bool set_tol = rd.flag();
        double edge_tol = rd.num();
        // tolerance-limit cases: every edge's stored ZoneTol3d (`operator=` keeps fabs)
        if (set_tol)
            for (SmEdge* e : g.edges)
                e->*EdTolPeek::tol() = edge_tol;
        SmLoopuse* p = of_loop ? g.loops[(size_t)i]->GetLoopuse() : g.loopuses[(size_t)i];
        if (p != nullptr && !closed3d_safe(p, n_eu, all != FALSE))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        // the out-params start at sentinels (-5 edge use, -7 gap, 777 tolerance), so an
        // unwritten one is visible
        SmEdgeuse* eu = (SmEdgeuse*)0x1;
        double gap = -7.0;
        SmXSectTol3d tol = 777.0; // a tolerance is stored as fabs
        SmBoolean r = of_loop ? g.loops[(size_t)i]->IsClosed3d(all, want_eu ? &eu : NULL, want_gap ? &gap : NULL,
                                                               want_tol ? &tol : NULL)
                              : p->IsClosed3d(all, want_eu ? &eu : NULL, want_gap ? &gap : NULL, want_tol ? &tol : NULL);
        put_closed(o, g, r, eu, gap, (double)tol);
        break;
    }
    case OP_MISSING_SEAM:
    {
        SmLoopuse* p = g.loopuses[(size_t)rd.idx()];
        SmFaceuse* fu = p->GetFaceuse();
        SmSurface* s = fu && fu->GetFace() ? fu->GetFace()->GetSurface() : nullptr;
        bool closes = true;
        std::vector<SmEdgeuse*> walk_v = ccw_walk(p, n_eu, closes);
        if (s != nullptr && (s->IsClosed(s->GetNaturalUVDomain(), SM_SP_U) ||
                             s->IsClosed(s->GetNaturalUVDomain(), SM_SP_V)))
        {
            if (!closes)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            // the sectors upstream computes, in its order, until one is missing a seam
            std::vector<SmVertexuse*> vus;
            if (type_of(p) == SmVertexuse_TYPE)
                vus.push_back((SmVertexuse*)start_of(p));
            for (SmEdgeuse* q : walk_v)
                vus.push_back(q->GetVertexuse());
            bool safe = true;
            for (SmVertexuse* vu : vus)
            {
                if (vu == nullptr)
                    continue;
                if (!ok_sector(vu))
                {
                    safe = false;
                    break;
                }
                SmUVSectorIO io;
                vu->ComputeUVSector(io, TRUE);
                if (io.m_bSectorMissingSeamU || io.m_bSectorMissingSeamV)
                    break;
            }
            if (!safe)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
        }
        o.push_back(flg(p->IsMissingSeamAtVertices()));
        break;
    }
    case OP_UV_CORNERS:
    {
        SmLoop* l = g.loops[(size_t)rd.idx()];
        SmPoint2d a = rd.uv(), c = rd.uv();
        double tol = rd.num();
        SmLoopuse* lu = l->GetLoopuse();
        if (lu != nullptr)
        {
            bool closes = true;
            std::vector<SmEdgeuse*> w = ccw_walk(lu, n_eu, closes);
            if (!closes)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            if (!w.empty())
            {
                // GetOrCreateUVTrimCurve on every edge use (deferred on the Rust side)
                o.push_back(ST_NOT_RUN);
                break;
            }
        }
        SmExtent2d dom(a.x, a.y, c.x, c.y);
        SmTArray<SmEdge*> edges;
        SmTArray<ULONG> bnd;
        SmTArray<double> prm;
        ULONG r = l->GetUVCornerBoundaries(dom, tol, edges, bnd, prm);
        o.push_back((double)r);
        std::vector<int64_t> ix;
        for (ULONG k = 0; k < edges.GetSize(); k++)
            ix.push_back(g.find(7, edges[k]));
        put_list(o, ix);
        ix.clear();
        for (ULONG k = 0; k < bnd.GetSize(); k++)
            ix.push_back((int64_t)bnd[k]);
        put_list(o, ix);
        o.push_back((double)prm.GetSize());
        for (ULONG k = 0; k < prm.GetSize(); k++)
            o.push_back(prm[k]);
        break;
    }
    case OP_FLIP:
    {
        SmLoop* l = g.loops[(size_t)rd.idx()];
        SmLoopuse* l1 = l->GetLoopuse();
        if (l1 == nullptr)
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmLoopuse* l2 = l1->GetOtherLoopuse();
        if (l2 != nullptr)
        {
            // what the swap reads as the kind of the loop use's own start (the mate's
            // start of the other kind is memory of the wrong class upstream)
            SM_TYPE t1 = type_of(l1), t2 = type_of(l2);
            bool bad = false;
            if (t1 == SmVertexuse_TYPE)
            {
                SmVertexuse* v1 = (SmVertexuse*)start_of(l1);
                SmTopology* s2 = start_of(l2);
                if (v1 != nullptr && s2 != nullptr)
                {
                    if (t2 != SmVertexuse_TYPE)
                        bad = true;
                    else if (!v1->IsLoopVertexuse() || !((SmVertexuse*)s2)->IsLoopVertexuse())
                        bad = true;
                }
            }
            else if (start_of(l1) != nullptr && start_of(l2) != nullptr && t2 == SmVertexuse_TYPE)
                bad = true;
            if (bad)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
        }
        o.push_back(st(l->FlipLoopOrientation()));
        put_use_state(o, g);
        break;
    }
    case OP_COLLECT:
    {
        SmLoopuse* p = g.loopuses[(size_t)rd.idx()];
        int64_t e = rd.idx();
        SmEdgeuse* eu = e >= 0 ? g.edgeuses[(size_t)e] : nullptr;
        o.push_back(st(p->CollectEdgeuses(eu)));
        put_use_state(o, g);
        break;
    }
    case OP_SET_EDGEUSE:
    {
        SmLoopuse* p = g.loopuses[(size_t)rd.idx()];
        int64_t e = rd.idx();
        SmEdgeuse* eu = e >= 0 ? g.edgeuses[(size_t)e] : nullptr;
        o.push_back(st(p->SetEdgeuse(eu)));
        put_use_state(o, g);
        break;
    }
    case OP_SET_ORIENT:
    {
        SmLoopuse* p = g.loopuses[(size_t)rd.idx()];
        p->SetOrientation((SmOrientType)rd.idx());
        o.push_back((double)p->GetOrientation());
        put_use_state(o, g);
        break;
    }
    case OP_SET_LOOP:
    {
        SmLoopuse* p = g.loopuses[(size_t)rd.idx()];
        int64_t i = rd.idx();
        p->SetLoop(i >= 0 ? g.loops[(size_t)i] : nullptr);
        o.push_back((double)g.find(5, p->GetLoop()));
        put_use_state(o, g);
        break;
    }
    case OP_FACE_FU:
    {
        // the export lacks each face's m_pFU (SmFace::GetFaceuses reads it)
        std::vector<int64_t> ix;
        for (SmFace* f : g.faces)
            ix.push_back(g.find(2, f->*FcPeek::fu()));
        put_list(o, ix);
        break;
    }
    default:
        break;
    }
}
} // namespace

SC_EXPORT int32_t sc_topology_loopuse_eval(int32_t op, const char* path, const double* in, int64_t n_in, double* out,
                                           int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    run_op(op, rd, path, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
