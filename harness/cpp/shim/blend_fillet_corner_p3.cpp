// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_corner_p3`, part 1: SmFilletCorner::Create (the corner
// classification, upstream source/SMLib/src/SmFilletCorner.cpp) on a real SmBrep built from
// unit topology.edgeuse_p1's B-rep block (sc_edgeuse_graph.h), with an SmFilletExecutive whose
// edge-use-to-solver map holds one base SmFilletSolver per filleted edge use (tolerances and a
// surface generator). The corner patch and adjustment ops are in
// blend_fillet_corner_p3_static.inc (they need blend_fillet_geom_p1.cpp's pseudo-B-rep Model).
//
// One entry point, sc_blend_fillet_corner_p3_create(path, in, n_in, out, cap, out_len). Layout
// in crates/solidcore-ffi/src/units/blend_fillet_corner_p3.rs, which validates every input.
// Output: `0` then the op record, or `-1 status` when the B-rep could not be built. The record
// starts with `-2` ("upstream would dereference null": the shim refuses to run Create), `-3`
// ("not run": Create would reach a routine whose Rust port is a stub, so the case is deferred
// and upstream is not called), else `0` (no corner) or `1` and the corner's members.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBrep.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmFilletCorner.h>
#include <SmFilletExecutive.h>
#include <SmFilletSolver.h>
#include <SmVertex.h>
#include <SmVertexuse.h>

#include <vector>

#include "sc_edgeuse_graph.h"

namespace
{
const double P3_NULL_DEREF = -2.0;
const double P3_NOT_RUN = -3.0;

// A base SmFilletSolver carrying its tolerances and surface generator.
struct P3Solver : SmFilletSolver
{
    P3Solver(const SmContext& ctx, double tol, double tang) : SmFilletSolver(ctx, tol, SM_EFF_ZERO_RAD, tang)
    {
        m_pEdgeuses[0] = m_pEdgeuses[1] = NULL;
        m_pExecutive = NULL;
        m_dOrientations[0] = m_dOrientations[1] = 1.0;
        m_dSavedOffsets[0] = m_dSavedOffsets[1] = 0.0;
        m_bExtendBefore = m_bExtendAfter = FALSE;
    }
    virtual SmFilletSolverType GetSolverType() { return SM_FS_UNKNOWN; }
    virtual void GetName(TCHAR* pc, size_t n)
    {
        if (n > 0)
            pc[0] = 0;
    }
};

struct ExecPeek : SmFilletExecutive
{
    ExecPeek(const SmContext& c) : SmFilletExecutive(c) {}
    static SmMapPtrToPtr<SmEdgeuse, SmFilletSolver> SmFilletExecutive::*map()
    {
        return &ExecPeek::m_vEdgeuseSolverMap;
    }
};

struct CornerPeek : SmFilletCorner
{
    static SmTArray<SmEdgeuse*>* SmFilletCorner::*all() { return &CornerPeek::m_pAllEUs; }
    static SmTArray<SmFilletVertex*> SmFilletCorner::*verts() { return &CornerPeek::m_vVertices; }
    static SmTArray<SmFilletEdge*> SmFilletCorner::*edges() { return &CornerPeek::m_vEdges; }
    static SmTArray<SmSurface*> SmFilletCorner::*surfs() { return &CornerPeek::m_vSurfaces; }
    static SmBoolean SmFilletCorner::*chamfer() { return &CornerPeek::m_bChamfer; }
    static SmBoolean SmFilletCorner::*blending() { return &CornerPeek::m_bBlending; }
    static SmBoolean SmFilletCorner::*ext() { return &CornerPeek::m_bExtendedSurfacePatch; }
    static double SmFilletCorner::*tantol() { return &CornerPeek::m_dTangencyTolRadians; }
};

SmFilletSurfaceGenerator* make_fsg(const SmContext& ctx, int64_t kind)
{
    switch (kind)
    {
    case 1: return new (ctx) SmLinearCrossSectionFSG();
    case 2: return new (ctx) SmCircularCrossSectionFSG();
    case 3: return new (ctx) SmBlendCurveCrossSectionFSG();
    default: return new (ctx) SmFilletSurfaceGenerator();
    }
}

// GetMate/GetRadial need the edge's primary edgeuse (loop edgeuses); see topology_edgeuse_p2.
bool has_prim(SmEdgeuse* p)
{
    return p->GetEdgeuseType() == SmShell_TYPE || (p->GetEdge() && p->GetEdge()->GetPrimaryEdgeuse());
}

bool binormal_safe(SmEdgeuse* p)
{
    return p != nullptr && p->IsLoopEdgeuse() && p->GetEdge() && p->GetEdge()->GetCurve() && p->GetFaceuse() &&
           p->GetFace() && p->GetFace()->GetSurface() && p->GetVertexuse() && has_prim(p);
}

bool eval_safe(SmEdgeuse* p) { return p != nullptr && (!p->IsLoopEdgeuse() || binormal_safe(p)); }

// IsConvexRadialSector(4) runs without dereferencing null.
bool convex_safe(SmEdgeuse* p)
{
    if (!has_prim(p) || p->GetEdge() == nullptr)
        return false;
    return !p->IsLoopEdgeuse() || (eval_safe(p) && eval_safe(p->GetRadial()));
}

// What Create would do before it calls anything: the walk around the start edge use's vertex
// (upstream's loop, repeated), with its null dereferences. Returns -2 for a dereference, -3 when
// the Rust side stops at a stub, 0 when Create can run.
double precheck(Graph& g, const std::vector<SmEdgeuse*>& solver, size_t limit)
{
    SmEdgeuse* start = solver[0];
    std::vector<SmEdgeuse*> all;
    all.push_back(start);
    bool open_end = false;
    SmEdgeuse* eu = start;
    size_t steps = 0;
    while (true)
    {
        if (++steps > limit + 1)
            return P3_NULL_DEREF; // never closes (the Rust side errors; upstream loops)
        SmEdgeuse* eu0 = eu;
        if (!has_prim(eu0))
            return P3_NULL_DEREF;
        SmEdgeuse* eu1 = eu0->GetCWEdgeuse();
        if (eu1 == nullptr || !has_prim(eu1))
            return P3_NULL_DEREF;
        SmEdgeuse* eu2 = eu1->GetRadial();
        if (eu2 == nullptr)
            return P3_NULL_DEREF;
        eu = eu2;
        if (eu == start)
            break;
        if (eu0->GetEdge() == eu1->GetEdge())
            continue;
        bool found = false;
        for (SmEdgeuse* p : all)
            if (eu->GetEdge() == p->GetEdge())
                found = true;
        if (found)
            continue;
        if (eu->GetEdge() == nullptr)
            return P3_NULL_DEREF;
        if (eu->GetEdge()->IsLamina())
            open_end = true;
        SmEdgeuse* s = nullptr;
        for (SmEdgeuse* p : solver)
            if (eu != p && eu->GetEdge() == p->GetEdge())
            {
                s = p;
                break;
            }
        all.push_back(s ? s : eu);
    }
    (void)open_end;
    size_t total = all.size();
    size_t ns = solver.size();
    if (start->GetEdge() == nullptr)
        return P3_NULL_DEREF;
    switch (ns)
    {
    case 1:
        if (start->GetEdge()->IsClosed())
            return total == 1 ? P3_NOT_RUN : 0.0; // sm_TestG1Continuity
        return P3_NOT_RUN;                        // CheckNx1Degeneracy
    case 2:
        if (total == 2)
            return P3_NOT_RUN; // sm_TestG1Continuity
        if (total == 3)
            return (convex_safe(solver[0]) && convex_safe(solver[1])) ? 0.0 : P3_NULL_DEREF;
        return 0.0;
    default:
        return 0.0; // the NxN concavity test is decided by the chamfer flag (below)
    }
}

void run_create(Rd& rd, const char* path, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
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
    SmVertex* vertex = g.vertices[(size_t)rd.idx()];
    int64_t ns = rd.idx();
    std::vector<SmEdgeuse*> solverEUs;
    std::vector<double> tols, tans;
    std::vector<int64_t> kinds;
    for (int64_t i = 0; i < ns; ++i)
    {
        solverEUs.push_back(g.edgeuses[(size_t)rd.idx()]);
        tols.push_back(rd.num());
        tans.push_back(rd.num());
        kinds.push_back(rd.idx());
    }

    double pre = precheck(g, solverEUs, g.edgeuses.size());
    if (pre == 0.0 && ns >= 3)
    {
        // NxN without chamfer reaches sm_FindConcaveEdgeuses (a stub on the Rust side)
        bool chamfer = true;
        for (int64_t k : kinds)
            if (k != 1)
                chamfer = false;
        // total EUs are recomputed by Create; the 4x3 case and a count mismatch never test
        // concavity, so only defer when the corner could be NxN
        if (!chamfer)
            pre = -4.0; // decided after Create's own classification below
    }
    if (pre == P3_NULL_DEREF || pre == P3_NOT_RUN)
    {
        o.push_back(pre);
        return;
    }

    ExecPeek* exec = new (ctx) ExecPeek(ctx);
    std::vector<P3Solver*> solvers;
    std::vector<SmFilletSurfaceGenerator*> fsgs;
    for (int64_t i = 0; i < ns; ++i)
    {
        P3Solver* s = new (ctx) P3Solver(ctx, tols[(size_t)i], tans[(size_t)i]);
        SmFilletSurfaceGenerator* f = make_fsg(ctx, kinds[(size_t)i]);
        s->SetFilletSurfaceGenerator(f);
        solvers.push_back(s);
        fsgs.push_back(f);
        (exec->*ExecPeek::map()).Insert(solverEUs[(size_t)i], s);
    }

    // The walk's edge-use count decides between 4x3, NxN and "not implemented"; only NxN
    // (count == ns) without chamfer tests concavity.
    bool defer = false;
    if (pre == -4.0)
    {
        // repeat the walk count (precheck already validated it)
        SmEdgeuse* start = solverEUs[0];
        std::vector<SmEdgeuse*> all;
        all.push_back(start);
        SmEdgeuse* eu = start;
        while (true)
        {
            SmEdgeuse* eu0 = eu;
            SmEdgeuse* eu1 = eu0->GetCWEdgeuse();
            eu = eu1->GetRadial();
            if (eu == start)
                break;
            if (eu0->GetEdge() == eu1->GetEdge())
                continue;
            bool found = false;
            for (SmEdgeuse* p : all)
                if (eu->GetEdge() == p->GetEdge())
                    found = true;
            if (found)
                continue;
            all.push_back(eu);
        }
        size_t total = all.size();
        bool is4x3 = ns == 3 && total == 4;
        defer = !is4x3 && total == (size_t)ns;
    }

    if (defer)
    {
        o.push_back(P3_NOT_RUN);
    }
    else
    {
        SmTArray<SmEdgeuse*>* pSolverEUs = new (ctx) SmTArray<SmEdgeuse*>(ctx);
        for (SmEdgeuse* p : solverEUs)
            pSolverEUs->Add(p);
        SmFilletCorner* c = SmFilletCorner::Create(ctx, vertex, pSolverEUs, exec);
        if (c == NULL)
        {
            o.push_back(0.0);
            delete pSolverEUs;
        }
        else
        {
            o.push_back(1.0);
            o.push_back((double)c->GetCornerType());
            SmTArray<SmEdgeuse*>* all = c->*CornerPeek::all();
            o.push_back(all ? (double)all->GetSize() : -1.0);
            if (all)
                for (ULONG i = 0; i < all->GetSize(); ++i)
                    o.push_back((double)g.find(6, (*all)[i]));
            o.push_back(c->GetThisApproxTol3d());
            o.push_back(c->*CornerPeek::tantol());
            o.push_back(flg(c->IsBevelCorner()));
            o.push_back(flg(c->IsBlendingCorner()));
            o.push_back(flg(c->*CornerPeek::chamfer()));
            o.push_back(flg(c->IsDegenerateCorner()));
            o.push_back(flg(c->IsCalcAsConstRad()));
            o.push_back(flg(c->IsTopologyAdjusted()));
            o.push_back(flg(c->*CornerPeek::ext()));
            o.push_back(c->GetSetBackDist());
            o.push_back((double)(c->*CornerPeek::verts()).GetSize());
            o.push_back((double)(c->*CornerPeek::edges()).GetSize());
            o.push_back((double)(c->*CornerPeek::surfs()).GetSize());
            SmFilletNx1ClosedCorner* closed = c->GetCornerType() == SM_FCR_N_x_1_CLOSED ? (SmFilletNx1ClosedCorner*)c : NULL;
            o.push_back(closed ? flg(closed->m_bG1Case) : -1.0);
            // the corner owns pSolverEUs and its pAllEUs
            delete c;
        }
    }

    for (P3Solver* s : solvers)
        delete s;
    for (SmFilletSurfaceGenerator* f : fsgs)
        delete f;
    delete exec;
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_corner_p3_create(const char* path, const double* in, int64_t n_in, double* out,
                                                   int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    run_create(rd, path, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
