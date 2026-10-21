// SPDX-License-Identifier: Apache-2.0
// Shared real-B-rep fixture construction and raw graph export.
#pragma once
// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.vertexuse`: the SmVertexuse members owned by the unit
// (source/SMLib/src/SmVertexuse.cpp, source/SMLib/inc/SmVertexuse.h) on real SmBrep
// objects.
//
// One entry point, sc_topology_vertexuse_eval(op, path, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/topology_vertexuse.rs, which
// validates every input. The B-rep construction, traversal and export are those of the
// `topology.edgeuse_p1` shim (harness/cpp/shim/topology_edgeuse_p1.cpp), with each vertex
// use's vertex/edge gap state and UV point, each face's raw members (face use, UV domain,
// rectangular-trim flag, tolerance, whether its surface is owned by it; unit
// `topology.face_p2`) and SmPlane surfaces as planes added to the export. Every B-rep op starts with
// a B-rep block:
//   `0 rebuild validate_only init_gaps`         SmBrep::ReadFromFile(path) (ASCII .smb)
//   `1 curve t0 t1 validate_only init_gaps`     CreateWireEdgeFromCurve(curve, [t0, t1])
//   `2 surface u0 v0 u1 v1 validate_only init_gaps` CreateFaceFromSurface(surface, dom)
//   `3 validate_only init_gaps`                 the wire network of prog_test my_test_topology
//   `5 np (x y z)*np ne (a b has_curve [curve])*ne validate_only init_gaps`
//                                               shell vertices joined by MakeEdgeTopology edges
// `init_gaps` computes the cached vertex/face, vertex/edge, edge/face and edge/CCW-edge
// gaps (C++ only) before the op, so their reset is observable.
//
// The B-rep graph is exported in traversal order (each pointer mapped to its first-seen
// index per kind; -1 is null), reading raw members (protected ones through
// pointer-to-member access), never the functions under test. Ops whose upstream path
// would dereference null output -2 instead of calling it.
#include "sc_shim.h"
#include "support/topology_fixture.h"


#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFace.h>
#include <SmFaceuse.h>
#include <SmGap.h>
#include <SmLocalSolveNd.h>
#include <SmLine.h>
#include <SmLoop.h>
#include <SmLoopuse.h>
#include <SmMatrix.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmRegion.h>
#include <SmShell.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <SmVertex.h>
#include <SmVertexuse.h>
#include <nurbs.h>

#include <map>
#include <vector>


namespace
{
enum Op : int32_t
{
    OP_EXPORT = 0,
    OP_VU_QUERY = 1,
    OP_CONNECTED = 2,
    OP_GAPS = 3,
    OP_NOTIFY = 4,
    OP_UV_POINT = 5,
    OP_UV_SECTOR = 6,
    OP_MISSING_SEAM = 7,
    OP_ON_POLE = 8,
};

// Status of an op the shim refuses because upstream would dereference null.
const double ST_NULL_DEREF = -2.0;

struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    bool flag() { return in[at++] != 0.0; }
    SmBoolean bflag() { return flag() ? TRUE : FALSE; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
    SmPoint2d uv()
    {
        double u = num();
        double v = num();
        return SmPoint2d(u, v);
    }
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

void put_vec(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

const SmContext& the_context()
{
    static SmContext* p = new SmContext();
    return *p;
}

SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 1)
    {
        SmPoint3d o = rd.vec();
        SmVector3d n = rd.vec();
        return new (ctx) SmPlane(o, n, &ctx);
    }
    bool oob = rd.flag();
    SmBSplineSurfaceForm form = (SmBSplineSurfaceForm)rd.idx();
    gw_INDEX n = (gw_INDEX)rd.idx();
    gw_INDEX m = (gw_INDEX)rd.idx();
    gw_DEGREE p = (gw_DEGREE)rd.idx();
    gw_DEGREE q = (gw_DEGREE)rd.idx();
    gw_INDEX r = (gw_INDEX)rd.idx();
    gw_INDEX s = (gw_INDEX)rd.idx();
    gw_SURFACE* sur = sm_AllocateNurbSurface(n, m, p, q, r, s);
    for (gw_INDEX i = 0; i <= n; i++)
        for (gw_INDEX j = 0; j <= m; j++)
        {
            sur->net->Pw[i][j].x = rd.num();
            sur->net->Pw[i][j].y = rd.num();
            sur->net->Pw[i][j].z = rd.num();
            sur->net->Pw[i][j].w = rd.num();
        }
    for (gw_INDEX i = 0; i <= r; i++)
        sur->knu->U[i] = rd.num();
    for (gw_INDEX i = 0; i <= s; i++)
        sur->knv->U[i] = rd.num();
    SmBSplineSurface* b = new (ctx) SmBSplineSurface(sur, FALSE, NULL);
    b->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
    b->SetBSplineSurfaceForm(form);
    return b;
}

SmBSplineCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    ULONG dim = (ULONG)rd.idx();
    gw_INDEX n = (gw_INDEX)rd.idx();
    gw_DEGREE p = (gw_DEGREE)rd.idx();
    gw_INDEX m = (gw_INDEX)rd.idx();
    gw_CURVE* cur = sm_AllocateNurbCurve(n, p, m);
    for (gw_INDEX i = 0; i <= n; i++)
    {
        cur->pol->Pw[i].x = rd.num();
        cur->pol->Pw[i].y = rd.num();
        cur->pol->Pw[i].z = rd.num();
        cur->pol->Pw[i].w = rd.num();
    }
    for (gw_INDEX i = 0; i <= m; i++)
        cur->knt->U[i] = rd.num();
    return new (ctx) SmBSplineCurve(cur, dim, FALSE);
}

// ---- raw member access (pointer-to-member through a derived class) ----------------------

struct EuPeek : SmEdgeuse
{
    static SmTopology* SmEdgeuse::*sorlu() { return &EuPeek::m_pSorLU; }
    static SmBSplineCurve* SmEdgeuse::*uv() { return &EuPeek::m_pUVTrimCurve; }
    static SmDropCurveFail* SmEdgeuse::*fail() { return &EuPeek::m_pDropCurveFail; }
    static SmEdgeFaceGap SmEdgeuse::*face_gap() { return &EuPeek::m_sMaxEdgeFaceGap3d; }
    static SmEdgeEdgeGap SmEdgeuse::*ccw_gap() { return &EuPeek::m_sEdgeCCWEdgeGap3d; }
    static SmVertexuse* SmEdgeuse::*vu() { return &EuPeek::m_pVU; }
};
struct FuPeek : SmFaceuse
{
    static SmFace* SmFaceuse::*face() { return &FuPeek::m_pF; }
    static SmFaceuse* SmFaceuse::*mate() { return &FuPeek::m_pFUMate; }
};
struct LuPeek : SmLoopuse
{
    static SmLoop* SmLoopuse::*loop() { return &LuPeek::m_pL; }
    static SmLoopuse* SmLoopuse::*mate() { return &LuPeek::m_pLUMate; }
    static SM_TYPE SmLoopuse::*type() { return &LuPeek::m_tLoopuseType; }
    static SmTopology* SmLoopuse::*start() { return &LuPeek::m_pEUorVU; }
    static SmOrientType SmLoopuse::*orient() { return &LuPeek::m_eOrientation; }
};
struct VuPeek : SmVertexuse
{
    static SmTopology* SmVertexuse::*owner() { return &VuPeek::m_pSorLUorEU; }
    static SmVertexFaceGap SmVertexuse::*face_gap() { return &VuPeek::m_sVertexFaceGap3d; }
    static SmVertexEdgeGap SmVertexuse::*edge_gap() { return &VuPeek::m_sVertexEdgeGap3d; }
    static SmPoint2d SmVertexuse::*uv() { return &VuPeek::m_sUVPoint; }
};
struct FPeek : SmFace
{
    static SmFaceuse* SmFace::*fu() { return &FPeek::m_pFU; }
    static SmExtent2d SmFace::*dom() { return &FPeek::m_vUVDomain; }
    static SmBoolean SmFace::*rect() { return &FPeek::m_bRectangularTrim; }
    static SmSurface* SmFace::*srf() { return &FPeek::m_pSurface; }
    static SmZoneTol3d SmFace::*tol() { return &FPeek::m_sZoneTol3d; }
};
struct ShPeek : SmShell
{
    static SM_TYPE SmShell::*type() { return &ShPeek::m_tShellType; }
};

// ---- B-rep construction -------------------------------------------------------------------

struct Built
{
    SmBrep* brep = nullptr;
    ~Built() { delete brep; }
};

// The wire network of prog_test my_test_topology (smTopoTest.cpp, its shell vertex, wire
// edge and edge topology steps).
SmStatus build_wire_network(const SmContext& ctx, SmBrep* pBrep)
{
    SmShell* pNewShell;
    SmVertex* pNewVertex;
    SER(pBrep->MakeShellVertex(pBrep->GetInfiniteRegion(), SmPoint3d(0, 0, 0), pNewShell, pNewVertex));
    SmVertex* pNewVertex2;
    SER(pBrep->MakeShellVertex(pBrep->GetInfiniteRegion(), SmPoint3d(1, 1, 1), pNewShell, pNewVertex2));

    SmEdge* pNewEdge;
    SmVertex* pNewVertex3;
    SmBSplineCurve* pLine = NULL;
    SER(SmBSplineCurve::CreateLineSegment(ctx, 3, pNewVertex->GetPoint(), SmPoint3d(1, 0, 0), pLine));
    SER(pBrep->MakeWireEdgeVertex(pBrep->GetInfiniteRegion(), pNewVertex, pLine, pLine->GetNaturalInterval(),
                                  SM_OT_SAME, SmPoint3d(1, 0, 0), pNewEdge, pNewVertex3));

    SER(SmBSplineCurve::CreateLineSegment(ctx, 3, pNewVertex3->GetPoint(), pNewVertex2->GetPoint(), pLine));
    SER(pBrep->MakeWireEdge(pBrep->GetInfiniteRegion(), pNewVertex3, pNewVertex2, pLine, pLine->GetNaturalInterval(),
                            SM_OT_SAME, pNewEdge));

    SER(pBrep->MakeEdgeTopology(pBrep->GetInfiniteRegion(), pNewVertex2, pNewVertex, pNewEdge));
    SER(SmBSplineCurve::CreateLineSegment(ctx, 3, pNewVertex2->GetPoint(), pNewVertex->GetPoint(), pLine));
    pNewEdge->SetCurve(pLine, TRUE);
    pLine->SetOwner(pNewEdge);
    pNewEdge->SetInterval(pLine->GetNaturalInterval());
    pNewEdge->SetOrientation(SM_OT_SAME);

    SmVertex* pNewVertex4;
    SER(pBrep->MakeShellVertexTopology(pBrep->GetInfiniteRegion(), pNewShell, pNewVertex4));
    pNewVertex4->SetPoint(SmPoint3d(0, 1, 0));

    SER(pBrep->MakeEdgeTopology(pBrep->GetInfiniteRegion(), pNewVertex2, pNewVertex4, pNewEdge));
    SER(SmBSplineCurve::CreateLineSegment(ctx, 3, pNewVertex2->GetPoint(), pNewVertex4->GetPoint(), pLine));
    pNewEdge->SetCurve(pLine, TRUE);
    pLine->SetOwner(pNewEdge);
    pNewEdge->SetInterval(pLine->GetNaturalInterval());
    pNewEdge->SetOrientation(SM_OT_SAME);

    SER(pBrep->MakeEdgeTopology(pBrep->GetInfiniteRegion(), pNewVertex4, pNewVertex, pNewEdge));
    SER(SmBSplineCurve::CreateLineSegment(ctx, 3, pNewVertex4->GetPoint(), pNewVertex->GetPoint(), pLine));
    pNewEdge->SetCurve(pLine, TRUE);
    pLine->SetOwner(pNewEdge);
    pNewEdge->SetInterval(pLine->GetNaturalInterval());
    pNewEdge->SetOrientation(SM_OT_SAME);
    return SM_SUCCESS;
}

// ---- traversal-order registry ----------------------------------------------------------

struct Graph
{
    std::vector<SmRegion*> regions;
    std::vector<SmShell*> shells;
    std::vector<SmFaceuse*> faceuses;
    std::vector<SmFace*> faces;
    std::vector<SmLoopuse*> loopuses;
    std::vector<SmLoop*> loops;
    std::vector<SmEdgeuse*> edgeuses;
    std::vector<SmEdge*> edges;
    std::vector<SmVertexuse*> vertexuses;
    std::vector<SmVertex*> vertices;
    std::map<const void*, int64_t> index[10];

    template <class T> int64_t add(int k, std::vector<T*>& v, T* p)
    {
        if (p == nullptr)
            return -1;
        auto it = index[k].find((const void*)p);
        if (it != index[k].end())
            return it->second;
        int64_t i = (int64_t)v.size();
        index[k][(const void*)p] = i;
        v.push_back(p);
        return i;
    }
    template <class T> int64_t find(int k, const T* p) const
    {
        if (p == nullptr)
            return -1;
        auto it = index[k].find((const void*)p);
        return it == index[k].end() ? -3 : it->second;
    }

    int64_t region(SmRegion* p) { return add(0, regions, p); }
    int64_t shell(SmShell* p) { return add(1, shells, p); }
    int64_t faceuse(SmFaceuse* p) { return add(2, faceuses, p); }
    int64_t face(SmFace* p) { return add(3, faces, p); }
    int64_t loopuse(SmLoopuse* p) { return add(4, loopuses, p); }
    int64_t loop(SmLoop* p) { return add(5, loops, p); }
    int64_t edgeuse(SmEdgeuse* p) { return add(6, edgeuses, p); }
    int64_t edge(SmEdge* p) { return add(7, edges, p); }
    int64_t vertexuse(SmVertexuse* p) { return add(8, vertexuses, p); }
    int64_t vertex(SmVertex* p) { return add(9, vertices, p); }

    // A topology pointer of the given type code, registered.
    int64_t typed(SM_TYPE t, SmTopology* p)
    {
        if (p == nullptr)
            return -1;
        switch (t)
        {
        case SmShell_TYPE: return shell((SmShell*)p);
        case SmLoopuse_TYPE: return loopuse((SmLoopuse*)p);
        case SmEdgeuse_TYPE: return edgeuse((SmEdgeuse*)p);
        case SmVertexuse_TYPE: return vertexuse((SmVertexuse*)p);
        case SmFaceuse_TYPE: return faceuse((SmFaceuse*)p);
        default: return -4;
        }
    }
};

// Members of an owner's circular list, from its head (bounded by a generous count).
std::vector<SmTopology*> list_of(SmTopology* head)
{
    std::vector<SmTopology*> v;
    SmTopology* p = head;
    while (p != nullptr && v.size() < 100000)
    {
        v.push_back(p);
        p = p->GetNext();
        if (p == head)
            break;
    }
    return v;
}

// Walks everything reachable from the B-rep's regions, registering in first-seen order.
void walk(SmBrep* brep, Graph& g, bool include_vertex_lists = true)
{
    SmTArray<SmRegion*> regs;
    brep->GetRegions(regs);
    for (ULONG i = 0; i < regs.GetSize(); i++)
        g.region(regs[i]);
    // SmBrep::GetEdges order is observable (SmShell::GetWireEdges, shell wire ownership), and a
    // wire owned by a face shell may be unreachable from faces/vertex lists: register it first.
    SmTArray<SmEdge*> all_edges;
    brep->GetEdges(all_edges);
    for (ULONG i = 0; i < all_edges.GetSize(); ++i)
        g.edge(all_edges[i]);
    size_t r = 0, s = 0, fu = 0, lu = 0, eu = 0, vu = 0, l = 0, e = 0, f = 0, v = 0;
    bool more = true;
    while (more)
    {
        more = false;
        for (; r < g.regions.size(); r++, more = true)
            for (SmTopology* t : list_of(g.regions[r]->GetList()))
                g.shell((SmShell*)t);
        for (; s < g.shells.size(); s++, more = true)
        {
            SmShell* sh = g.shells[s];
            SM_TYPE ty = sh->*ShPeek::type();
            if (ty == SmFaceuse_TYPE)
                for (SmTopology* t : list_of(sh->GetList()))
                    g.faceuse((SmFaceuse*)t);
            else
                g.typed(ty, sh->GetList());
        }
        for (; fu < g.faceuses.size(); fu++, more = true)
        {
            SmFaceuse* p = g.faceuses[fu];
            g.face(p->*FuPeek::face());
            g.faceuse(p->*FuPeek::mate());
            for (SmTopology* t : list_of(p->GetList()))
                g.loopuse((SmLoopuse*)t);
        }
        for (; lu < g.loopuses.size(); lu++, more = true)
        {
            SmLoopuse* p = g.loopuses[lu];
            g.loop(p->*LuPeek::loop());
            g.loopuse(p->*LuPeek::mate());
            SmTopology* start = p->*LuPeek::start();
            g.typed(p->*LuPeek::type(), start);
            // the CCW walk of an edge loop
            if (p->*LuPeek::type() == SmEdgeuse_TYPE)
            {
                SmEdgeuse* q = (SmEdgeuse*)start;
                for (int k = 0; q != nullptr && k < 100000; k++)
                {
                    g.edgeuse(q);
                    q = q->GetCCWEdgeuse();
                    if (q == (SmEdgeuse*)start)
                        break;
                }
            }
        }
        for (; eu < g.edgeuses.size(); eu++, more = true)
        {
            SmEdgeuse* p = g.edgeuses[eu];
            g.edge((SmEdge*)p->GetOwner());
            g.typed(p->GetEdgeuseType(), p->*EuPeek::sorlu());
            g.edgeuse(p->GetCWEdgeuse());
            g.edgeuse(p->GetCCWEdgeuse());
            g.vertexuse(p->*EuPeek::vu());
            g.edgeuse((SmEdgeuse*)p->GetNext());
            g.edgeuse((SmEdgeuse*)p->GetLast());
        }
        for (; e < g.edges.size(); e++, more = true)
            for (SmTopology* t : list_of(g.edges[e]->GetList()))
                g.edgeuse((SmEdgeuse*)t);
        for (; vu < g.vertexuses.size(); vu++, more = true)
        {
            SmVertexuse* p = g.vertexuses[vu];
            g.vertex((SmVertex*)p->GetOwner());
            g.typed(p->GetVertexuseType(), p->*VuPeek::owner());
        }
        if (include_vertex_lists)
            for (; v < g.vertices.size(); v++, more = true)
                for (SmTopology* t : list_of(g.vertices[v]->GetList()))
                    g.vertexuse((SmVertexuse*)t);
        for (; l < g.loops.size(); l++, more = true)
            g.loopuse(g.loops[l]->GetLoopuse());
        for (; f < g.faces.size(); f++)
            ;
    }
}

// A curve block: `-1` (none), `-2` (not an SmBSplineCurve), else `type dim has_nurb
// [n p m cpts(x y z w) knots]` and, for an SmLine, `P[3] V[3] a0 a1 scale inside_out`.
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    if (c == nullptr || !c->IsKindOf(SmBSplineCurve_TYPE))
    {
        o.push_back(c == nullptr ? -1.0 : -2.0);
        return;
    }
    const SmBSplineCurve* b = (const SmBSplineCurve*)c;
    gw_CURVE* nurb = b->GetGwNurbPointer();
    o.push_back((double)c->GetType());
    o.push_back((double)b->GetDim());
    o.push_back(nurb ? 1.0 : 0.0);
    if (nurb)
    {
        o.push_back((double)nurb->pol->n);
        o.push_back((double)nurb->p);
        o.push_back((double)nurb->knt->m);
        for (gw_INDEX k = 0; k <= nurb->pol->n; k++)
        {
            o.push_back(nurb->pol->Pw[k].x);
            o.push_back(nurb->pol->Pw[k].y);
            o.push_back(nurb->pol->Pw[k].z);
            o.push_back(nurb->pol->Pw[k].w);
        }
        for (gw_INDEX k = 0; k <= nurb->knt->m; k++)
            o.push_back(nurb->knt->U[k]);
    }
    if (c->GetType() == SmLine_TYPE)
    {
        const SmLine* l = (const SmLine*)c;
        put_vec(o, l->GetLinePoint());
        put_vec(o, l->GetLineVector());
        SmExtent1d d = l->GetSTEPInterval();
        o.push_back(d.GetMin());
        o.push_back(d.GetMax());
        o.push_back(l->GetLineScale());
        o.push_back(flg(l->GetInsideOut()));
    }
}

void put_surface(std::vector<double>& o, const SmSurface* s)
{
    if (s == nullptr || !s->IsKindOf(SmBSplineSurface_TYPE))
    {
        o.push_back(s == nullptr ? -1.0 : -2.0);
        return;
    }
    if (s->GetType() == SmPlane_TYPE)
    {
        // `1 O[3] X[3] Y[3] scale[2] anal_domain(u0 v0 u1 v1)`
        const SmPlane* pl = (const SmPlane*)s;
        const SmAxis2Placement& pos = pl->GetPosition();
        o.push_back(1.0);
        put_vec(o, pos.GetOrigin());
        put_vec(o, pos.GetXAxis());
        put_vec(o, pos.GetYAxis());
        SmVector2d sc = pl->GetUVScale();
        o.push_back(sc.x);
        o.push_back(sc.y);
        SmExtent2d d = pl->GetSTEPUVDomain();
        o.push_back(d.GetMin().x);
        o.push_back(d.GetMin().y);
        o.push_back(d.GetMax().x);
        o.push_back(d.GetMax().y);
        return;
    }
    const SmBSplineSurface* b = (const SmBSplineSurface*)s;
    gw_SURFACE* sur = b->GetGwNurbPointer();
    o.push_back(0.0);
    // the surface's own type: an SmBSplineSurface subclass other than SmPlane is
    // exported as its B-spline part only
    o.push_back((double)s->GetType());
    o.push_back(flg(((SmBSplineSurface*)b)->GetOutOfBoundsEnabled()));
    o.push_back((double)b->GetBSplineSurfaceForm());
    o.push_back((double)sur->net->n);
    o.push_back((double)sur->net->m);
    o.push_back((double)sur->p);
    o.push_back((double)sur->q);
    o.push_back((double)sur->knu->m);
    o.push_back((double)sur->knv->m);
    for (gw_INDEX i = 0; i <= sur->net->n; i++)
        for (gw_INDEX j = 0; j <= sur->net->m; j++)
        {
            o.push_back(sur->net->Pw[i][j].x);
            o.push_back(sur->net->Pw[i][j].y);
            o.push_back(sur->net->Pw[i][j].z);
            o.push_back(sur->net->Pw[i][j].w);
        }
    for (gw_INDEX i = 0; i <= sur->knu->m; i++)
        o.push_back(sur->knu->U[i]);
    for (gw_INDEX i = 0; i <= sur->knv->m; i++)
        o.push_back(sur->knv->U[i]);
}

void put_list(std::vector<double>& o, const std::vector<int64_t>& v)
{
    o.push_back((double)v.size());
    for (int64_t x : v)
        o.push_back((double)x);
}

// The exported graph; layout in the FFI wrapper (`export` section).
void export_graph(SmBrep* brep, Graph& g, std::vector<double>& o)
{
    o.push_back(flg(brep->m_bValidateOnly));
    for (auto n : {g.regions.size(), g.shells.size(), g.faceuses.size(), g.faces.size(), g.loopuses.size(),
                   g.loops.size(), g.edgeuses.size(), g.edges.size(), g.vertexuses.size(), g.vertices.size()})
        o.push_back((double)n);
    for (SmRegion* p : g.regions)
    {
        o.push_back(p->GetOwner() == (SmOwningTopology*)brep ? 1.0 : 0.0);
        std::vector<int64_t> v;
        for (SmTopology* t : list_of(p->GetList()))
            v.push_back(g.find(1, t));
        put_list(o, v);
    }
    for (SmShell* p : g.shells)
    {
        o.push_back((double)g.find(0, p->GetOwner()));
        SM_TYPE ty = p->*ShPeek::type();
        o.push_back((double)ty);
        std::vector<int64_t> v;
        if (ty == SmFaceuse_TYPE)
            for (SmTopology* t : list_of(p->GetList()))
                v.push_back(g.find(2, t));
        put_list(o, v);
    }
    for (SmFaceuse* p : g.faceuses)
    {
        o.push_back((double)g.find(1, p->GetOwner()));
        o.push_back((double)g.find(3, p->*FuPeek::face()));
        o.push_back((double)g.find(2, p->*FuPeek::mate()));
        o.push_back((double)p->GetOrientation());
        std::vector<int64_t> v;
        for (SmTopology* t : list_of(p->GetList()))
            v.push_back(g.find(4, t));
        put_list(o, v);
    }
    for (SmFace* p : g.faces)
    {
        SmSurface* s = p->*FPeek::srf();
        put_surface(o, s);
        o.push_back((double)g.find(2, p->*FPeek::fu()));
        SmExtent2d d = p->*FPeek::dom();
        o.push_back(d.GetMin().x);
        o.push_back(d.GetMin().y);
        o.push_back(d.GetMax().x);
        o.push_back(d.GetMax().y);
        o.push_back((double)(p->*FPeek::rect()));
        o.push_back((double)(p->*FPeek::tol()));
        o.push_back(s && s->GetOwner() == (SmObject*)p ? 1.0 : 0.0);
    }
    for (SmLoopuse* p : g.loopuses)
    {
        o.push_back((double)g.find(2, p->GetOwner()));
        o.push_back((double)g.find(5, p->*LuPeek::loop()));
        o.push_back((double)g.find(4, p->*LuPeek::mate()));
        SM_TYPE ty = p->*LuPeek::type();
        o.push_back((double)ty);
        SmTopology* start = p->*LuPeek::start();
        o.push_back((double)(ty == SmEdgeuse_TYPE ? g.find(6, start) : ty == SmVertexuse_TYPE ? g.find(8, start) : -1));
        o.push_back((double)(p->*LuPeek::orient()));
    }
    for (SmLoop* p : g.loops)
        o.push_back((double)g.find(4, p->GetLoopuse()));
    for (SmEdgeuse* p : g.edgeuses)
    {
        o.push_back((double)g.find(7, p->GetOwner()));
        SM_TYPE ty = p->GetEdgeuseType();
        o.push_back((double)ty);
        SmTopology* sorlu = p->*EuPeek::sorlu();
        o.push_back((double)(ty == SmLoopuse_TYPE ? g.find(4, sorlu) : ty == SmShell_TYPE ? g.find(1, sorlu) : -1));
        o.push_back((double)g.find(6, p->GetCWEdgeuse()));
        o.push_back((double)g.find(6, p->GetCCWEdgeuse()));
        o.push_back((double)g.find(8, p->*EuPeek::vu()));
        o.push_back((double)p->GetOrientation());
        SmBSplineCurve* uv = p->*EuPeek::uv();
        put_curve(o, uv);
        o.push_back(uv ? (double)g.find(6, uv->GetOwner()) : -1.0);
        SmDropCurveFail* fail = p->*EuPeek::fail();
        o.push_back(fail ? 1.0 : 0.0);
        o.push_back(fail ? (double)fail->m_sFailStatus : 0.0);
        o.push_back(fail && fail->m_pBrep ? 1.0 : 0.0);
        o.push_back(flg((p->*EuPeek::face_gap()).IsInit()));
        o.push_back(flg((p->*EuPeek::ccw_gap()).IsInit()));
    }
    for (SmEdge* p : g.edges)
    {
        o.push_back((double)p->GetTolerance());
        SmExtent1d ivl = p->GetInterval();
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
        std::vector<int64_t> v;
        for (SmTopology* t : list_of(p->GetList()))
            v.push_back(g.find(6, t));
        put_list(o, v);
        put_curve(o, p->GetCurve());
    }
    for (SmVertexuse* p : g.vertexuses)
    {
        o.push_back((double)g.find(9, p->GetOwner()));
        SM_TYPE ty = p->GetVertexuseType();
        o.push_back((double)ty);
        SmTopology* own = p->*VuPeek::owner();
        o.push_back((double)(ty == SmEdgeuse_TYPE    ? g.find(6, own)
                             : ty == SmLoopuse_TYPE ? g.find(4, own)
                             : ty == SmShell_TYPE   ? g.find(1, own)
                                                    : -1));
        o.push_back(flg((p->*VuPeek::face_gap()).IsInit()));
        o.push_back(flg((p->*VuPeek::edge_gap()).IsInit()));
        SmPoint2d uv = p->*VuPeek::uv();
        o.push_back(uv.x);
        o.push_back(uv.y);
    }
    for (SmVertex* p : g.vertices)
    {
        put_vec(o, p->GetPoint());
        o.push_back((double)p->GetTolerance());
    }
    // Version 3 shell/region state (infinite region, void flags, wire/vertex shell heads, vertex-use
    // lists, face-use links), read from the native records by the shared exporter.
    sc_topology_fixture::append_shell_state(
        {*brep, g.regions, g.shells, g.faceuses, g.faces, g.loopuses, g.loops, g.edgeuses, g.edges,
         g.vertexuses, g.vertices},
        o);
}

// Builds the B-rep of a B-rep block; nullptr (status in `st`) when construction fails.
SmBrep* build(Rd& rd, const char* path, SmStatus& status)
{
    const SmContext& ctx = the_context();
    int64_t kind = rd.idx();
    SmBrep* brep = new (ctx) SmBrep();
    status = SM_SUCCESS;
    if (kind == 0)
    {
        bool rebuild = rd.flag();
        status = brep->ReadFromFile(ctx, path, SM_ASCII, rebuild ? TRUE : FALSE);
    }
    else if (kind == 1)
    {
        SmBSplineCurve* c = read_curve(rd, ctx);
        double t0 = rd.num(), t1 = rd.num();
        SmEdge* pEdge = nullptr;
        status = brep->CreateWireEdgeFromCurve(c, SmExtent1d(t0, t1), pEdge);
    }
    else if (kind == 2 || kind == 4)
    {
        SmSurface* s = read_surface(rd, ctx);
        SmPoint2d a = rd.uv(), b = rd.uv();
        SmFace* pFace = nullptr;
        status = brep->CreateFaceFromSurface(s, SmExtent2d(a, b), pFace);
        if (kind == 4)
        {
            // kind 4: the face of kind 2 plus an inner vertex loop at the surface point
            // whose parameters are the given fractions of the face domain
            double fu = rd.num(), fv = rd.num();
            if (status == SM_SUCCESS && pFace)
            {
                SmPoint2d uv(a.x + fu * (b.x - a.x), a.y + fv * (b.y - a.y));
                SmPoint3d pt(0.0, 0.0, 0.0);
                status = pFace->GetSurface()->EvaluatePoint(uv, pt);
                SmLoop* pLoop = nullptr;
                SmVertex* pVertex = nullptr;
                if (status == SM_SUCCESS)
                    status = brep->MakeVertexLoop(pFace, pt, pLoop, pVertex);
            }
        }
    }
    else if (kind == 5)
    {
        // kind 5: shell vertices at the given points, then edges between them
        // (MakeEdgeTopology with a straight line or the given curve, as the wire network)
        int64_t np = rd.idx();
        std::vector<SmVertex*> vs;
        for (int64_t i = 0; i < np; i++)
        {
            SmPoint3d p = rd.vec();
            SmShell* pShell = nullptr;
            SmVertex* pVertex = nullptr;
            if (status == SM_SUCCESS)
                status = brep->MakeShellVertex(brep->GetInfiniteRegion(), p, pShell, pVertex);
            vs.push_back(pVertex);
        }
        int64_t ne = rd.idx();
        for (int64_t i = 0; i < ne; i++)
        {
            int64_t a = rd.idx(), b = rd.idx();
            SmBSplineCurve* c = rd.flag() ? read_curve(rd, ctx) : nullptr;
            if (status != SM_SUCCESS)
            {
                delete c;
                continue;
            }
            if (c == nullptr)
                status = SmBSplineCurve::CreateLineSegment(ctx, 3, vs[(size_t)a]->GetPoint(), vs[(size_t)b]->GetPoint(), c);
            SmEdge* pEdge = nullptr;
            if (status == SM_SUCCESS)
                status = brep->MakeEdgeTopology(brep->GetInfiniteRegion(), vs[(size_t)a], vs[(size_t)b], pEdge);
            if (status != SM_SUCCESS || pEdge == nullptr)
            {
                delete c;
                if (status == SM_SUCCESS)
                    status = SM_ERR;
                continue;
            }
            pEdge->SetCurve(c, TRUE);
            c->SetOwner(pEdge);
            pEdge->SetInterval(c->GetNaturalInterval());
            pEdge->SetOrientation(SM_OT_SAME);
        }
    }
    else
    {
        status = build_wire_network(ctx, brep);
    }
    brep->m_bValidateOnly = rd.bflag();
    bool init_gaps = rd.flag();
    if (status != SM_SUCCESS)
    {
        delete brep;
        return nullptr;
    }
    if (init_gaps)
    {
        Graph g;
        walk(brep, g);
        for (SmVertexuse* p : g.vertexuses)
        {
            if (p->IsLoopVertexuse() || p->IsEdgeVertexuse())
                if (p->GetFaceuse() && p->GetFaceuse()->GetFace() && p->GetFaceuse()->GetFace()->GetSurface())
                    p->GetVertexFaceGap();
            if (p->HasVertexEdgeGap())
                p->GetVertexEdgeGap();
        }
        for (SmEdgeuse* p : g.edgeuses)
            if (p->IsLoopEdgeuse() && p->GetCCWEdgeuse() && p->GetFace() && p->GetFace()->GetSurface() &&
                p->GetEdge() && p->GetEdge()->GetCurve())
            {
                p->GetMaxEdgeFaceGap();
                p->GetEdgeCCWEdgeGap();
            }
    }
    return brep;
}

SmTopology* target_of(Graph& g, int64_t ty, int64_t i)
{
    auto at = [i](auto& v) -> SmTopology* { return (i >= 0 && i < (int64_t)v.size()) ? (SmTopology*)v[(size_t)i] : nullptr; };
    switch (ty)
    {
    case SmRegion_TYPE: return at(g.regions);
    case SmShell_TYPE: return at(g.shells);
    case SmFaceuse_TYPE: return at(g.faceuses);
    case SmFace_TYPE: return at(g.faces);
    case SmLoopuse_TYPE: return at(g.loopuses);
    case SmLoop_TYPE: return at(g.loops);
    case SmEdgeuse_TYPE: return at(g.edgeuses);
    case SmEdge_TYPE: return at(g.edges);
    case SmVertexuse_TYPE: return at(g.vertexuses);
    case SmVertex_TYPE: return at(g.vertices);
    default: return nullptr;
    }
}

const SM_TYPE KIND_TYPES[] = {SmVertexuse_TYPE, SmEdgeuse_TYPE, SmTopology_TYPE, SmOwningTopology_TYPE,
                              SmAObject_TYPE,   SmObject_TYPE,  SmShell_TYPE,    SmLoopuse_TYPE,
                              SmUnknown_TYPE,   0};

// ---- null-dereference guards (what upstream dereferences without checking) ------------

SmTopology* owner_of(SmVertexuse* p) { return p->*VuPeek::owner(); }

// GetFaceuse dereferences the loop use of a loop vertex use.
bool ok_faceuse(SmVertexuse* p) { return !p->IsLoopVertexuse() || owner_of(p) != nullptr; }

// GetLoopuse dereferences the edge use of an edge vertex use.
bool ok_loopuse(SmVertexuse* p) { return !p->IsEdgeVertexuse() || owner_of(p) != nullptr; }

// GetShell dereferences the face use of a loop vertex use's loop use.
bool ok_shell(SmVertexuse* p)
{
    if (!p->IsLoopVertexuse() || owner_of(p) == nullptr)
        return true;
    return ((SmLoopuse*)owner_of(p))->GetFaceuse() != nullptr;
}

// GetMate needs the edge's primary edge use (loop edge uses).
bool has_prim(SmEdgeuse* e)
{
    return e->GetEdgeuseType() == SmShell_TYPE || (e->GetEdge() && e->GetEdge()->GetPrimaryEdgeuse());
}

// GetVertexusesOfEdge / GetVertexusesOfFace on the vertex (null with a non-null target).
bool ok_siblings(SmVertexuse* p, const void* target, bool face)
{
    if (target == nullptr)
        return true;
    SmVertex* v = p->GetVertex();
    if (v == nullptr)
        return false;
    if (!face)
        return true;
    SmTArray<SmVertexuse*> all;
    v->GetVertexuses(all);
    for (ULONG i = 0; i < all.GetSize(); i++)
        if (!ok_faceuse(all[i]))
            return false;
    return true;
}

// CalcVertexFaceGap reaches ComputeUVPoint after GetVertexusesOfFace.
bool ok_face_gap(SmVertexuse* p)
{
    if (!p->HasVertexFaceGap())
        return true;
    if (!ok_faceuse(p))
        return false;
    SmFaceuse* fu = p->GetFaceuse();
    SmFace* f = fu ? fu->GetFace() : nullptr;
    if (!ok_siblings(p, f, true))
        return false;
    return p->GetVertex() != nullptr;
}

bool ok_edge_gap(SmVertexuse* p)
{
    if (!p->IsEdgeVertexuse())
        return true;
    SmEdgeuse* eu = p->GetEdgeuse();
    return ok_siblings(p, eu ? eu->GetEdge() : nullptr, false);
}

// ComputeUVSector: the loop use and face use queries, then for an edge vertex use of a
// face with a surface the corner mate, its vertex use (or its mate's) and the end
// vertex use's ComputeUVPoint.
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

// The cached gap states of every vertex use (vertex/edge, vertex/face).
void put_gap_states(std::vector<double>& o, Graph& g)
{
    for (SmVertexuse* p : g.vertexuses)
    {
        o.push_back(flg((p->*VuPeek::edge_gap()).IsInit()));
        o.push_back(flg((p->*VuPeek::face_gap()).IsInit()));
    }
}

void run_brep_op(int32_t op, Rd& rd, const char* path, std::vector<double>& o)
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
    if (op == OP_EXPORT)
    {
        export_graph(b.brep, g, o);
        return;
    }
    SmVertexuse* p = g.vertexuses[(size_t)rd.idx()];
    switch (op)
    {
    case OP_VU_QUERY:
    {
        o.push_back((double)p->GetVertexuseType());
        o.push_back((double)g.find(9, p->GetVertex()));
        o.push_back((double)g.find(6, p->GetEdgeuse()));
        o.push_back(ok_faceuse(p) ? (double)g.find(2, p->GetFaceuse()) : ST_NULL_DEREF);
        o.push_back(ok_loopuse(p) ? (double)g.find(4, p->GetLoopuse()) : ST_NULL_DEREF);
        o.push_back(ok_shell(p) ? (double)g.find(1, p->GetShell()) : ST_NULL_DEREF);
        o.push_back(flg(p->IsLoopVertexuse()));
        o.push_back(flg(p->IsShellVertexuse()));
        o.push_back(flg(p->IsEdgeVertexuse()));
        o.push_back(flg(p->HasVertexEdgeGap()));
        o.push_back(flg(p->HasVertexFaceGap()));
        o.push_back(ok_loopuse(p) ? flg(p->IsStrutVertexuse()) : ST_NULL_DEREF);
        o.push_back((double)p->GetType());
        for (const SM_TYPE* t = KIND_TYPES; *t != 0; t++)
            o.push_back(flg(p->IsKindOf(*t)));
        o.push_back(flg(p->IsVertexEdgeGapInit()));
        o.push_back(flg(p->IsVertexFaceGapInit()));
        break;
    }
    case OP_CONNECTED:
    {
        int64_t ty = rd.idx();
        int64_t i = rd.idx();
        SmTopology* t = target_of(g, ty, i);
        bool ok = true;
        if (t != nullptr)
        {
            if (ty == SmFace_TYPE || ty == SmFaceuse_TYPE)
                ok = ok_faceuse(p);
            else if (ty == SmLoop_TYPE || ty == SmLoopuse_TYPE)
                ok = ok_loopuse(p);
            else if (ty == SmShell_TYPE || ty == SmRegion_TYPE)
                ok = ok_shell(p);
        }
        o.push_back(ok ? flg(p->IsConnectedTo(t)) : ST_NULL_DEREF);
        break;
    }
    case OP_GAPS:
    {
        int64_t which = rd.idx();
        SmBoolean flag = rd.bflag();
        bool want_out = rd.flag();
        bool ok = (which == 0 || which == 2) ? ok_edge_gap(p) : (which == 4 || ok_face_gap(p));
        if (!ok)
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        if (which == 0)
            o.push_back(p->GetVertexEdgeGap(flag) ? 1.0 : 0.0);
        else if (which == 1)
            o.push_back(p->GetVertexFaceGap(flag) ? 1.0 : 0.0);
        else if (which == 2)
        {
            SmVertexEdgeGap gap;
            gap.SetVertexuse(p);
            o.push_back(st(p->CalcVertexEdgeGap(want_out ? &gap : NULL, flag)));
            o.push_back(want_out ? flg(gap.IsInit()) : -1.0);
        }
        else if (which == 3)
        {
            SmVertexFaceGap gap;
            gap.SetVertexuse(p);
            o.push_back(st(p->CalcVertexFaceGap(want_out ? &gap : NULL, flag)));
            o.push_back(want_out ? flg(gap.IsInit()) : -1.0);
        }
        else
        {
            p->InitVertexFaceGap();
            o.push_back(0.0);
        }
        put_gap_states(o, g);
        break;
    }
    case OP_NOTIFY:
    {
        p->Notify((SmNotifyOperation)rd.idx(), NULL, NULL, NULL);
        put_gap_states(o, g);
        break;
    }
    case OP_UV_POINT:
    {
        SmBoolean ok_make = rd.bflag();
        if (p->GetVertex() == nullptr || !ok_faceuse(p))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmPoint2d uv(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        o.push_back(st(p->ComputeUVPoint(uv, ok_make)));
        o.push_back(uv.x);
        o.push_back(uv.y);
        break;
    }
    case OP_UV_SECTOR:
    {
        SmBoolean ok_make = rd.bflag();
        if (!ok_sector(p))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmUVSectorIO io;
        o.push_back(st(p->ComputeUVSector(io, ok_make)));
        o.push_back((double)g.find(8, io.m_pVertexuse));
        o.push_back((double)g.find(6, io.m_pBegEdgeuse));
        o.push_back((double)g.find(6, io.m_pEndEdgeuse));
        o.push_back(io.m_dBegEdgeParam);
        o.push_back(io.m_dEndEdgeParam);
        for (const SmPoint2d* q : {&io.m_sBegEdgeUVPoint, &io.m_sEndEdgeUVPoint, &io.m_sBegEdgeUVTan, &io.m_sEndEdgeUVTan})
        {
            o.push_back(q->x);
            o.push_back(q->y);
        }
        o.push_back(io.m_dSectorAngDeg);
        for (SmBoolean f : {io.m_bVertexOnClosedU, io.m_bVertexOnClosedV, io.m_bVertexOnPole,
                            io.m_bVertexOnFlatCorner, io.m_bSectorMissingSeamU, io.m_bSectorMissingSeamV})
            o.push_back(flg(f));
        break;
    }
    case OP_MISSING_SEAM:
    {
        bool want_u = rd.flag();
        bool want_v = rd.flag();
        SmBoolean uv_ok = rd.bflag();
        if (!ok_sector(p))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmBoolean u = UNSURE, v = UNSURE;
        SmBoolean r = p->IsSectorMissingSeam(want_u ? &u : NULL, want_v ? &v : NULL, uv_ok);
        o.push_back(flg(r));
        o.push_back(want_u ? flg(u) : -1.0);
        o.push_back(want_v ? flg(v) : -1.0);
        break;
    }
    case OP_ON_POLE:
    {
        if (!ok_faceuse(p) || !ok_face_gap(p))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        o.push_back(flg(p->IsOnPole()));
        put_gap_states(o, g);
        break;
    }
    default:
        break;
    }
}
} // namespace
