// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.edge`: the SmEdge members owned by the unit
// (source/SMLib/src/SmEdge.cpp, source/SMLib/inc/SmEdge.h) on real SmBrep objects.
//
// One entry point, sc_topology_edge_eval(op, path, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/topology_edge.rs, which
// validates every input. The B-rep construction, traversal and export are those of the
// `topology.vertexuse` shim (harness/cpp/shim/topology_vertexuse.cpp), with each edge
// use's cached edge/face gap length, each vertex use's cached vertex/edge gap length and
// whether each edge owns its curve added to the export. Every B-rep op starts with a
// B-rep block:
//   `0 rebuild validate_only init_gaps`         SmBrep::ReadFromFile(path) (ASCII .smb)
//   `1 curve t0 t1 validate_only init_gaps`     CreateWireEdgeFromCurve(curve, [t0, t1])
//   `2 surface u0 v0 u1 v1 validate_only init_gaps` CreateFaceFromSurface(surface, dom)
//   `3 validate_only init_gaps`                 the wire network of prog_test my_test_topology
//   `4 P[3] V[3] a b scale inside_out f0 f1 validate_only init_gaps`
//                                               CreateWireEdgeFromCurve(SmLine(P, V, [a, b],
//                                               scale), fractions [f0, f1] of its interval)
// `init_gaps` computes the cached vertex/face, vertex/edge, edge/face and edge/CCW-edge
// gaps (C++ only) before the op.
//
// The B-rep graph is exported in traversal order (each pointer mapped to its first-seen
// index per kind; -1 is null), reading raw members (protected ones through
// pointer-to-member access), never the functions under test. Ops whose upstream path
// would dereference null output -2 instead of calling it.
#include "sc_shim.h"
#include "sc_analytic_export.h"
#include "sc_exact_curve_export.h"
#include "support/topology_fixture.h"


#include "StdAfx.h"
#include <SmAttribute.h>
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmContext.h>
#include <SmCircle.h>
#include <SmCurve.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmEllipse.h>
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
#include <cstring>
#include <memory>
#include <stdexcept>
#include <vector>

#include "sc_sm.h"

using sc::put_vec;


namespace
{

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
// An unlinked edge use (the constructor is protected).
struct EuMake : SmEdgeuse
{
    EuMake() : SmEdgeuse() {}
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
void walk(SmBrep* brep, Graph& g)
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
        for (; l < g.loops.size(); l++, more = true)
            g.loopuse(g.loops[l]->GetLoopuse());
        // the vertex use lists (a list may hold uses no owner chain reaches)
        for (; v < g.vertices.size(); v++, more = true)
            for (SmTopology* t : list_of(g.vertices[v]->GetList()))
                g.vertexuse((SmVertexuse*)t);
        for (; f < g.faces.size(); f++)
            ;
    }
}

// A curve block: `-1` (none), `-2` (not an SmBSplineCurve), else `type dim has_nurb
// [n p m cpts(x y z w) knots]` and, for an SmLine, `P[3] V[3] a0 a1 scale inside_out`.
void put_curve_base(std::vector<double>& o, const SmCurve* c)
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

// Operation fingerprints and graph records use the same lossless curve transport.
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    sc_exact_curve_export::put_curve(o, c, put_curve_base);
}

void put_export_curve(std::vector<double>& o, const SmCurve* c)
{
    sc_exact_curve_export::put_export_curve(o, c, the_context(), put_curve_base);
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
    // Reuse the shared lossless record checks, then always export the regular
    // B-spline block so the importer can verify the reconstructed base bitwise.
    if (sc_analytic_export::exact_type(s->GetType()))
        sc_analytic_export::put_exact_head(o, s, the_context());
    const SmBSplineSurface* b = (const SmBSplineSurface*)s;
    gw_SURFACE* sur = b->GetGwNurbPointer();
    o.push_back(0.0);
    // the surface's own type; the importer uses the regular block as a fallback
    // when the exact head above was not written
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
        put_export_curve(o, uv);
        o.push_back(uv ? (double)g.find(6, uv->GetOwner()) : -1.0);
        SmDropCurveFail* fail = p->*EuPeek::fail();
        o.push_back(fail ? 1.0 : 0.0);
        o.push_back(fail ? (double)fail->m_sFailStatus : 0.0);
        o.push_back(fail && fail->m_pBrep ? 1.0 : 0.0);
        o.push_back(flg((p->*EuPeek::face_gap()).IsInit()));
        o.push_back(flg((p->*EuPeek::ccw_gap()).IsInit()));
        o.push_back((p->*EuPeek::face_gap()).GetLength());
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
        put_export_curve(o, p->GetCurve());
        o.push_back(p->GetCurve() && p->GetCurve()->GetOwner() == (SmObject*)p ? 1.0 : 0.0);
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
        o.push_back((p->*VuPeek::edge_gap()).GetLength());
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
    else if (kind == 2)
    {
        SmSurface* s = read_surface(rd, ctx);
        SmPoint2d a = rd.uv(), b = rd.uv();
        SmFace* pFace = nullptr;
        status = brep->CreateFaceFromSurface(s, SmExtent2d(a, b), pFace);
    }
    else if (kind == 5)
    {
        bool circle = rd.flag();
        SmPoint3d center = rd.vec();
        SmVector3d x = rd.vec(), y = rd.vec();
        double a = rd.num(), b = rd.num(), rx = rd.num(), ry = rd.num();
        SmBoolean inside_out = rd.bflag();
        double n0 = rd.num(), n1 = rd.num(), f0 = rd.num(), f1 = rd.num();
        SmBoolean oob = rd.bflag();
        double tolerance = rd.num();
        SmOrientType orientation = (SmOrientType)rd.idx();
        std::unique_ptr<SmEllipse> held(circle
            ? (SmEllipse*)new (ctx) SmCircle(center, x, y, SmExtent1d(a, b), rx, 3, &ctx, NULL, inside_out)
            : new (ctx) SmEllipse(center, x, y, SmExtent1d(a, b), rx, ry, 3, &ctx, NULL, inside_out));
        SmEllipse* curve = held.get();
        const SM_TYPE expected = circle ? SmCircle_TYPE : SmEllipse_TYPE;
        if (curve->GetType() != expected)
            throw std::runtime_error("analytic wire constructor changed its type");
        status = curve->GetGwNurbPointer() == nullptr
            ? SM_ERR : curve->EditParameterization(SmExtent1d(n0, n1));
        if (status == SM_SUCCESS)
        {
            curve->SetOutOfBoundsEnabled(oob);
            SmEdge* edge = nullptr;
            SmExtent1d natural = curve->GetNaturalInterval();
            status = brep->CreateWireEdgeFromCurve(
                curve, SmExtent1d(natural.Evaluate(f0), natural.Evaluate(f1)), edge);
            if (curve->GetOwner() != nullptr)
                held.release();
            if (status == SM_SUCCESS)
            {
                if (edge == nullptr || edge->GetCurve() != curve || edge->GetCurve()->GetType() != expected)
                    throw std::runtime_error("analytic wire builder changed the curve kind");
                edge->SetTolerance(tolerance, FALSE, FALSE);
                edge->SetOrientation(orientation);
            }
        }
    }
    else if (kind == 4)
    {
        // a wire edge on a real SmLine over the fractions [f0, f1] of its natural interval
        SmPoint3d pt = rd.vec();
        SmVector3d v = rd.vec();
        double a = rd.num(), c = rd.num(), scale = rd.num();
        SmBoolean inside_out = rd.bflag();
        double f0 = rd.num(), f1 = rd.num();
        SmLine* l = new (ctx) SmLine(pt, v, SmExtent1d(a, c), scale, 3, &ctx, NULL, inside_out);
        SmExtent1d nat = l->GetNaturalInterval();
        SmEdge* pEdge = nullptr;
        status = brep->CreateWireEdgeFromCurve(l, SmExtent1d(nat.Evaluate(f0), nat.Evaluate(f1)), pEdge);
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

// ---- ops --------------------------------------------------------------------------------

enum Op : int32_t
{
    OP_EXPORT = 0,
    OP_QUERY = 1,
    OP_FACE = 2,
    OP_CONNECTED = 3,
    OP_FILTERED = 4,
    OP_END_TANGENT = 5,
    OP_TRIMMED = 6,
    OP_SET_TOL = 7,
    OP_SET_ORIENT = 8,
    OP_REVERSE = 9,
    OP_ORIENT_SEQ = 10,
    OP_RESET_PRIMARY = 11,
    OP_ADD_PAIR = 12,
    OP_MANIFOLD_END = 13,
    OP_GAPS = 14,
    OP_NOTIFY = 15,
    OP_SET_CURVE = 16,
    OP_RADIAL = 17,
    OP_RADIAL_CURVE = 18,
    OP_UV_OF_SURFACE = 19,
    OP_SEAM = 20,
    OP_TOPOLOGICAL = 21,
    OP_TANGENT = 22,
    OP_BLEND = 23,
    OP_ODE = 24,
    OP_SET_INTERVAL = 25,
};

// The type codes `IsKindOf` is asked about.
const SM_TYPE KIND_TYPES[] = {SmEdge_TYPE,  SmEdgeuse_TYPE, SmTopology_TYPE, SmOwningTopology_TYPE,
                              SmAObject_TYPE, SmObject_TYPE, SmShell_TYPE,   SmVertex_TYPE,
                              SmUnknown_TYPE, 0};

// ---- null-dereference guards (what upstream dereferences without checking) ------------

// GetFace of a loop edge use dereferences its loop use's face use.
bool ok_face(SmEdgeuse* eu)
{
    SmTopology* lu = eu->*EuPeek::sorlu();
    return eu->GetEdgeuseType() != SmLoopuse_TYPE || lu == nullptr || ((SmLoopuse*)lu)->GetOwner() != nullptr;
}

bool all_ok_face(SmEdge* e)
{
    for (SmTopology* t : list_of(e->GetList()))
        if (!ok_face((SmEdgeuse*)t))
            return false;
    return true;
}

bool all_have_shell(SmEdge* e)
{
    for (SmTopology* t : list_of(e->GetList()))
        if (((SmEdgeuse*)t)->GetShell() == nullptr)
            return false;
    return true;
}

// The primary edge use and its mate both have vertex uses.
bool ok_both_vu(SmEdge* e)
{
    SmEdgeuse* p = e->GetPrimaryEdgeuse();
    return p && p->GetVertexuse() && p->GetMate() && p->GetMate()->GetVertexuse();
}

// GetStartVertex (start) / GetEndVertex (!start) dereference one end.
bool ok_end(SmEdge* e, bool start)
{
    SmEdgeuse* p = e->GetPrimaryEdgeuse();
    if (p == nullptr)
        return false;
    bool use_prim = (p->GetOrientation() == SM_OT_SAME) == start;
    SmEdgeuse* q = use_prim ? p : p->GetMate();
    return q && q->GetVertexuse();
}

double vid(Graph& g, SmVertex* v) { return (double)g.find(9, v); }

// The mutable state of the graph after an op: per edge use `edge orientation cw ccw vu
// has_uv ef_init`, per edge `tolerance min max list`, per vertex use `vertex ve_init
// vf_init`, per vertex `tolerance`.
void put_state(std::vector<double>& o, Graph& g)
{
    for (SmEdgeuse* p : g.edgeuses)
    {
        o.push_back((double)g.find(7, p->GetOwner()));
        o.push_back((double)p->GetOrientation());
        o.push_back((double)g.find(6, p->GetCWEdgeuse()));
        o.push_back((double)g.find(6, p->GetCCWEdgeuse()));
        o.push_back((double)g.find(8, p->*EuPeek::vu()));
        o.push_back(p->*EuPeek::uv() ? 1.0 : 0.0);
        o.push_back(flg((p->*EuPeek::face_gap()).IsInit()));
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
    }
    for (SmVertexuse* p : g.vertexuses)
    {
        o.push_back((double)g.find(9, p->GetOwner()));
        o.push_back(flg((p->*VuPeek::edge_gap()).IsInit()));
        o.push_back(flg((p->*VuPeek::face_gap()).IsInit()));
    }
    for (SmVertex* p : g.vertices)
        o.push_back((double)p->GetTolerance());
}

template <class T> void put_ptrs(std::vector<double>& o, Graph& g, int kind, SmTArray<T*>& a)
{
    o.push_back((double)a.GetSize());
    for (ULONG i = 0; i < a.GetSize(); i++)
        o.push_back((double)g.find(kind, a[i]));
}

// Verbatim copy of the file-local SmODEEdgeLengthIFEO::Evaluate (SmEdge.cpp).
SmStatus ode_edge_length_evaluate(const SmCurve& m_crCurve, const SmPoint3d& m_vOriginOfComputation,
                                  double m_dCrossSectionRadius, double dT, SmTArray<double>& rDyDxValues)
{
    for (ULONG j = 0; j < rDyDxValues.GetSize(); j++)
    {
        rDyDxValues[j] = 0.0;
    }

    SmPoint3d sPV[2];
    SER(m_crCurve.Evaluate(dT, 1, TRUE, sPV));
    sPV[0] = sPV[0] - m_vOriginOfComputation;

    SmPoint3d sIPnt = sPV[0]; // Point to integrate
    rDyDxValues[1] = 0.0;
    if (sPV[1].Length() > 0)
    { // We integrate over the length of the curve
        rDyDxValues[1] = sPV[1].Length();
    }

    double dCircumference = 2.0 * SM_PI * m_dCrossSectionRadius;
    double dAreaDelta = dCircumference * sPV[1].Length();

    rDyDxValues[2] = dAreaDelta;

    rDyDxValues[4] = sIPnt.x * dAreaDelta;
    rDyDxValues[5] = sIPnt.y * dAreaDelta;
    rDyDxValues[6] = sIPnt.z * dAreaDelta;

    rDyDxValues[7] = (sIPnt.x * sIPnt.x) * dAreaDelta;
    rDyDxValues[8] = (sIPnt.y * sIPnt.y) * dAreaDelta;
    rDyDxValues[9] = (sIPnt.z * sIPnt.z) * dAreaDelta;

    rDyDxValues[10] = (sIPnt.y * sIPnt.z) * dAreaDelta;
    rDyDxValues[11] = (sIPnt.z * sIPnt.x) * dAreaDelta;
    rDyDxValues[12] = (sIPnt.x * sIPnt.y) * dAreaDelta;

    rDyDxValues[13] = (sIPnt.y * sIPnt.y + sIPnt.z * sIPnt.z) * dAreaDelta;
    rDyDxValues[14] = (sIPnt.x * sIPnt.x + sIPnt.z * sIPnt.z) * dAreaDelta;
    rDyDxValues[15] = (sIPnt.x * sIPnt.x + sIPnt.y * sIPnt.y) * dAreaDelta;

    double dArea = SM_PI * m_dCrossSectionRadius * m_dCrossSectionRadius;
    double dVolumeDelta = dArea * sPV[1].Length();

    rDyDxValues[3] = dVolumeDelta;

    rDyDxValues[16] = sIPnt.x * dVolumeDelta;
    rDyDxValues[17] = sIPnt.y * dVolumeDelta;
    rDyDxValues[18] = sIPnt.z * dVolumeDelta;

    rDyDxValues[19] = (sIPnt.x * sIPnt.x) * dVolumeDelta;
    rDyDxValues[20] = (sIPnt.y * sIPnt.y) * dVolumeDelta;
    rDyDxValues[21] = (sIPnt.z * sIPnt.z) * dVolumeDelta;

    rDyDxValues[22] = (sIPnt.y * sIPnt.z) * dVolumeDelta;
    rDyDxValues[23] = (sIPnt.z * sIPnt.x) * dVolumeDelta;
    rDyDxValues[24] = (sIPnt.x * sIPnt.y) * dVolumeDelta;

    rDyDxValues[25] = (sIPnt.y * sIPnt.y + sIPnt.z * sIPnt.z) * dVolumeDelta;
    rDyDxValues[26] = (sIPnt.x * sIPnt.x + sIPnt.z * sIPnt.z) * dVolumeDelta;
    rDyDxValues[27] = (sIPnt.x * sIPnt.x + sIPnt.y * sIPnt.y) * dVolumeDelta;

    return SM_SUCCESS;
}

// Attaches a fresh attribute `id` to each `ty idx` target of the input.
void attach_attributes(Rd& rd, Graph& g, int64_t id)
{
    int64_t n = rd.idx();
    for (int64_t k = 0; k < n; k++)
    {
        int64_t ty = rd.idx();
        int64_t i = rd.idx();
        SmTopology* t = target_of(g, ty, i);
        if (t != nullptr && id >= 0)
            t->AddAttribute(new (the_context()) SmAttribute((ULONG)id, SM_AB_COPY));
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
    const SmContext& ctx = the_context();
    if (op == OP_ORIENT_SEQ)
    {
        int64_t n = rd.idx();
        SmTArray<SmEdge*> seq;
        for (int64_t k = 0; k < n; k++)
            seq.Add(g.edges[(size_t)rd.idx()]);
        bool ok = true;
        for (ULONG k = 0; k < seq.GetSize() && seq.GetSize() >= 2; k++)
            ok = ok && ok_end(seq[k], true) && ok_end(seq[k], false) && seq[k]->GetList() != nullptr;
        if (!ok)
        {
            o.push_back(ST_NULL_DEREF);
            return;
        }
        o.push_back(st(SmEdge::OrientEdgeSequence(seq)));
        put_state(o, g);
        return;
    }
    SmEdge* e = g.edges[(size_t)rd.idx()];
    switch (op)
    {
    case OP_QUERY:
    {
        SmVertex* tv = (SmVertex*)target_of(g, SmVertex_TYPE, rd.idx());
        SmEdgeuse* p = e->GetPrimaryEdgeuse();
        o.push_back((double)e->GetEdgeuseCount());
        o.push_back((double)g.find(6, p));
        o.push_back(e->GetCurve() ? 1.0 : 0.0);
        o.push_back(flg(e->IsWire()));
        o.push_back(flg(e->IsLamina()));
        o.push_back(flg(e->IsManifold()));
        o.push_back(flg(e->IsSpine()));
        o.push_back(ok_both_vu(e) ? flg(e->IsClosed()) : ST_NULL_DEREF);
        o.push_back(p && p->GetVertexuse() ? vid(g, e->GetVertex()) : ST_NULL_DEREF);
        o.push_back(ok_end(e, true) ? vid(g, e->GetStartVertex()) : ST_NULL_DEREF);
        o.push_back(ok_end(e, false) ? vid(g, e->GetEndVertex()) : ST_NULL_DEREF);
        if (ok_both_vu(e))
        {
            SmVertex *a = nullptr, *c = nullptr;
            e->GetVertices(a, c);
            o.push_back(vid(g, a));
            o.push_back(vid(g, c));
        }
        else
        {
            o.push_back(ST_NULL_DEREF);
            o.push_back(ST_NULL_DEREF);
        }
        if (p == nullptr || ok_both_vu(e))
        {
            SmTArray<SmVertex*> vs;
            e->GetVertices(vs);
            put_ptrs(o, g, 9, vs);
        }
        else
            o.push_back(ST_NULL_DEREF);
        {
            SmTArray<SmVertexuse*> vus;
            e->GetVertexuses(vus);
            put_ptrs(o, g, 8, vus);
        }
        {
            SmTArray<SmEdgeuse*> eus;
            o.push_back(st(e->GetEdgeuses(eus)));
            put_ptrs(o, g, 6, eus);
        }
        if (all_ok_face(e))
        {
            SmTArray<SmFace*> fs;
            e->GetFaces(fs);
            put_ptrs(o, g, 3, fs);
        }
        else
            o.push_back(ST_NULL_DEREF);
        {
            SmTArray<SmShell*> ss;
            e->GetShells(ss);
            put_ptrs(o, g, 1, ss);
        }
        if (all_have_shell(e))
        {
            SmTArray<SmRegion*> rs;
            e->GetRegions(rs);
            put_ptrs(o, g, 0, rs);
        }
        else
            o.push_back(ST_NULL_DEREF);
        o.push_back(p ? (double)g.find(6, e->GetSameOrientedEdgeuse()) : ST_NULL_DEREF);
        o.push_back(e->GetBrep() ? 1.0 : 0.0);
        o.push_back(vid(g, e->GetOtherVertex(tv)));
        o.push_back((double)e->GetType());
        for (const SM_TYPE* t = KIND_TYPES; *t != 0; t++)
            o.push_back(flg(e->IsKindOf(*t)));
        o.push_back((double)e->GetTolerance());
        o.push_back(e->GetInterval().GetMin());
        o.push_back(e->GetInterval().GetMax());
        break;
    }
    case OP_FACE:
    {
        SmFace* f = (SmFace*)target_of(g, SmFace_TYPE, rd.idx());
        if (!all_ok_face(e))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        o.push_back((double)g.find(6, e->GetEdgeuseOfFace(f)));
        o.push_back((double)g.find(6, e->GetUpwardEdgeuseOfFace(f)));
        o.push_back(f ? (double)e->GetNumberFaceOccurances(f) : ST_NULL_DEREF);
        o.push_back(flg(e->IsConnectedToFace(f)));
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
            if (ty == SmVertex_TYPE)
                ok = !e->GetPrimaryEdgeuse() || ok_both_vu(e);
            else if (ty == SmFace_TYPE || ty == SmFaceuse_TYPE)
                ok = all_ok_face(e);
            else if (ty == SmRegion_TYPE)
                ok = all_have_shell(e);
            else if (ty == SmLoop_TYPE)
                ok = ((SmLoop*)t)->GetLoopuse() != nullptr;
        }
        o.push_back(ok ? flg(e->IsConnectedTo(t)) : ST_NULL_DEREF);
        break;
    }
    case OP_FILTERED:
    {
        int64_t id = rd.idx();
        int64_t ty = rd.idx();
        int64_t i = rd.idx();
        SmTopology* target = target_of(g, ty, i);
        attach_attributes(rd, g, id);
        ULONG lid = (ULONG)(id < 0 ? 0 : id);
        ULONG* pid = id < 0 ? nullptr : &lid;
        {
            SmTArray<SmEdgeuse*> eus;
            o.push_back(st(e->GetEdgeuses(eus, pid, target)));
            put_ptrs(o, g, 6, eus);
        }
        if (all_ok_face(e))
        {
            SmTArray<SmFace*> fs;
            e->GetFaces(fs, pid);
            put_ptrs(o, g, 3, fs);
        }
        else
            o.push_back(ST_NULL_DEREF);
        if (pid == nullptr || all_have_shell(e))
        {
            SmTArray<SmShell*> ss;
            e->GetShells(ss, pid);
            put_ptrs(o, g, 1, ss);
        }
        else
            o.push_back(ST_NULL_DEREF);
        bool regions_ok = all_have_shell(e);
        if (regions_ok && pid != nullptr)
            for (SmTopology* t : list_of(e->GetList()))
                if (((SmEdgeuse*)t)->GetShell()->GetRegion() == nullptr)
                    regions_ok = false;
        if (regions_ok)
        {
            SmTArray<SmRegion*> rs;
            e->GetRegions(rs, pid);
            put_ptrs(o, g, 0, rs);
        }
        else
            o.push_back(ST_NULL_DEREF);
        SmEdgeuse* p = e->GetPrimaryEdgeuse();
        bool v_ok = p == nullptr || ok_both_vu(e);
        if (v_ok && p != nullptr && pid != nullptr)
        {
            SmEdgeuse* a = p->GetOrientation() == SM_OT_SAME ? p : p->GetMate();
            v_ok = a->GetVertexuse()->GetVertex() != nullptr;
            SmEdgeuse* c = a == p ? p->GetMate() : p;
            SmVertex* v2 = c->GetVertexuse()->GetVertex();
            (void)v2;
        }
        if (v_ok)
        {
            SmTArray<SmVertex*> vs;
            e->GetVertices(vs, pid);
            put_ptrs(o, g, 9, vs);
        }
        else
            o.push_back(ST_NULL_DEREF);
        bool vu_ok = pid == nullptr || p == nullptr || (p->GetVertexuse() && p->GetMate()->GetVertexuse());
        if (vu_ok)
        {
            SmTArray<SmVertexuse*> vus;
            e->GetVertexuses(vus, pid);
            put_ptrs(o, g, 8, vus);
        }
        else
            o.push_back(ST_NULL_DEREF);
        break;
    }
    case OP_END_TANGENT:
    {
        bool at_start = rd.flag();
        if (e->GetCurve() == nullptr)
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmVector3d t(0, 0, 0);
        o.push_back(st(e->GetEndTangent(at_start ? TRUE : FALSE, t)));
        put_vec(o, t);
        break;
    }
    case OP_TRIMMED:
    {
        SmBSplineCurve* c = e->CreateTrimmedNURBSCurve(ctx);
        put_curve(o, c);
        delete c;
        break;
    }
    case OP_SET_TOL:
    {
        double tol = rd.num();
        bool only_larger = rd.flag();
        bool cascade = rd.flag();
        bool grows = !(tol <= e->GetTolerance() && only_larger);
        SmEdgeuse* p = e->GetPrimaryEdgeuse();
        if (grows && cascade && !(p && p->GetVertexuse()))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        e->SetTolerance(tol, only_larger ? TRUE : FALSE, cascade ? TRUE : FALSE);
        o.push_back(0.0);
        put_state(o, g);
        break;
    }
    case OP_SET_ORIENT:
    {
        e->SetOrientation((SmOrientType)rd.idx());
        put_state(o, g);
        break;
    }
    case OP_REVERSE:
    {
        if (e->GetList() == nullptr)
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        e->ReverseOrientation();
        o.push_back(0.0);
        put_curve(o, e->GetCurve());
        put_state(o, g);
        break;
    }
    case OP_RESET_PRIMARY:
    {
        SmEdgeuse* np = (SmEdgeuse*)target_of(g, SmEdgeuse_TYPE, rd.idx());
        if (np != e->GetList() && e->IsInList(np) && !(ok_face(np) && ok_face(np->GetMate())))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        o.push_back(st(e->ResetPrimaryEdgeuse(np)));
        put_state(o, g);
        break;
    }
    case OP_ADD_PAIR:
    {
        SmOrientType o1 = (SmOrientType)rd.idx();
        SmOrientType o2 = (SmOrientType)rd.idx();
        SmEdgeuse* sector = (SmEdgeuse*)target_of(g, SmEdgeuse_TYPE, rd.idx());
        SmEdgeuse* anchor = nullptr;
        if (e->IsLamina())
            anchor = e->GetPrimaryEdgeuse()->GetMate();
        else if (sector != nullptr)
            anchor = sector->GetNext() == sector->GetRadial() ? sector : sector->GetRadial();
        if (anchor == nullptr)
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmEdgeuse* a = new ((SmObject*)b.brep) EuMake();
        SmEdgeuse* c = new ((SmObject*)b.brep) EuMake();
        a->SetOrientation(o1);
        c->SetOrientation(o2);
        g.edgeuse(a);
        g.edgeuse(c);
        o.push_back(st(e->AddOrientedEUPair(a, c, sector)));
        put_state(o, g);
        // unlink and free the new edge uses before the B-rep goes
        for (SmEdgeuse* x : {a, c})
        {
            SmOwningTopology* own = (SmOwningTopology*)x->GetOwner();
            if (own != nullptr)
                own->Remove(x);
            delete x;
        }
        break;
    }
    case OP_MANIFOLD_END:
    {
        SmVertexuse* vu = (SmVertexuse*)target_of(g, SmVertexuse_TYPE, rd.idx());
        bool is_start = rd.flag();
        SmTArray<SmEdgeuse*> eus;
        e->GetEdgeuses(eus);
        if (eus.GetSize() != 4)
        {
            o.push_back(st(e->InsertManifoldEdgeEnd(vu, is_start ? TRUE : FALSE)));
            put_state(o, g);
            break;
        }
        SmEdgeuse* top1 = is_start ? eus[0] : eus[3];
        SmEdgeuse* bot2 = is_start ? eus[2] : eus[1];
        bool ok = vu && vu->GetVertex() && top1->GetVertexuse() && bot2->GetVertexuse();
        if (ok && !vu->IsLoopVertexuse())
        {
            SmEdgeuse* ot1 = vu->GetEdgeuse();
            ok = ot1 && ot1->GetCWEdgeuse() && ot1->GetMate() && ot1->GetCWEdgeuse()->GetMate();
        }
        if (!ok)
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        o.push_back(st(e->InsertManifoldEdgeEnd(vu, is_start ? TRUE : FALSE)));
        put_state(o, g);
        // the vertex use lists are inconsistent now: leak the B-rep
        b.brep = nullptr;
        break;
    }
    case OP_GAPS:
    {
        int64_t which = rd.idx();
        bool want = rd.flag();
        double tol = rd.num();
        SmTol3d t3(tol < 0.0 ? 0.0 : tol);
        SmTol3d* pt = tol < 0.0 ? nullptr : &t3;
        SmGapArray arr;
        SmGapArray* pa = want ? &arr : nullptr;
        SmTArray<SmVertexuse*> vus;
        e->GetVertexuses(vus);
        bool vus_ok = vus.GetSize() == 0 || (vus[0] != nullptr && vus[1] != nullptr);
        if (!vus_ok && which != 1)
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        const SmGap* m = nullptr;
        if (which == 0)
        {
            const SmGap* up = e->GetMaxUpDimGap3d();
            const SmGap* dn = e->GetMaxDownDimGap3d();
            if (up == nullptr || dn == nullptr)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            m = e->GetMaxGap3d(pa, pt);
        }
        else if (which == 1)
            m = e->GetMaxUpDimGap3d(pa, pt);
        else
            m = e->GetMaxDownDimGap3d(pa, pt);
        o.push_back(0.0);
        o.push_back(m ? (m->IsKindOf(SmVertexEdgeGap_TYPE) ? 1.0 : 0.0) : -1.0);
        o.push_back(m ? m->GetLength() : 0.0);
        o.push_back(want ? (double)arr.GetSize() : -1.0);
        for (ULONG k = 0; want && k < arr.GetSize(); k++)
        {
            o.push_back(arr[k]->IsKindOf(SmVertexEdgeGap_TYPE) ? 1.0 : 0.0);
            o.push_back(arr[k]->GetLength());
        }
        break;
    }
    case OP_NOTIFY:
    {
        SmNotifyOperation nop = (SmNotifyOperation)rd.idx();
        bool fans = nop == SM_NO_CHANGE_GEOMETRY || nop == SM_NO_POST_EDIT || nop == SM_NO_SPLIT_IN_BREP ||
                    nop == SM_NO_MERGE_IN_BREP;
        if (fans)
        {
            SmTArray<SmVertexuse*> vus;
            e->GetVertexuses(vus);
            for (ULONG k = 0; k < vus.GetSize(); k++)
                if (vus[k] == nullptr)
                {
                    o.push_back(ST_NULL_DEREF);
                    return;
                }
        }
        e->Notify(nop, NULL, NULL, NULL);
        o.push_back(0.0);
        put_state(o, g);
        break;
    }
    case OP_SET_CURVE:
    {
        bool has = rd.flag();
        SmBSplineCurve* nc = has ? read_curve(rd, ctx) : nullptr;
        bool del = rd.flag();
        bool notify = rd.flag();
        SmCurve* old = e->GetCurve();
        e->SetCurve(nc, del ? TRUE : FALSE, TRUE, notify ? TRUE : FALSE);
        o.push_back(0.0);
        put_curve(o, e->GetCurve());
        bool returned = old != nullptr && old != nc && !del;
        o.push_back(returned ? 1.0 : 0.0);
        o.push_back(returned && old->GetOwner() == nullptr ? 1.0 : 0.0);
        if (returned)
        {
            put_curve(o, old);
            if (old->GetOwner() == nullptr)
                delete old;
        }
        put_state(o, g);
        if (nc != nullptr)
            nc->SetOwner(e);
        break;
    }
    case OP_RADIAL:
    {
        SmEdgeuse* ceu = (SmEdgeuse*)target_of(g, SmEdgeuse_TYPE, rd.idx());
        SmRegion* reg = (SmRegion*)target_of(g, SmRegion_TYPE, rd.idx());
        SmOrientType ot = (SmOrientType)rd.idx();
        if (!all_have_shell(e) || ceu == nullptr || !all_ok_face(e))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmEdgeuse* r = nullptr;
        SmFaceuse* f = nullptr;
        SmStatus s = e->FindRadialSector(ceu, reg, ot, r, f, NULL);
        o.push_back(st(s));
        o.push_back((double)g.find(6, r));
        o.push_back((double)g.find(2, f));
        break;
    }
    case OP_RADIAL_CURVE:
    {
        double param = rd.num();
        SmEdge* ce = g.edges[(size_t)rd.idx()];
        SmOrientType ot = (SmOrientType)rd.idx();
        bool force = rd.flag();
        if (ce->GetCurve() == nullptr || !all_ok_face(e))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmEdgeuse* r = nullptr;
        SmFaceuse* f = nullptr;
        SmStatus s = e->FindRadialSectorOfCurve(param, *ce->GetCurve(), ce->GetInterval(), ot, force ? TRUE : FALSE,
                                                r, f);
        o.push_back(st(s));
        o.push_back((double)g.find(6, r));
        o.push_back((double)g.find(2, f));
        break;
    }
    case OP_UV_OF_SURFACE:
    {
        SmFace* f = (SmFace*)target_of(g, SmFace_TYPE, rd.idx());
        bool ok = true;
        for (SmTopology* t : list_of(e->GetList()))
            if (!ok_face((SmEdgeuse*)t) || ((SmEdgeuse*)t)->GetFace() == nullptr)
                ok = false;
        if (!ok)
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmBSplineCurve* c = e->GetUVTrimCurveOfSurface(f ? f->GetSurface() : nullptr);
        SmEdgeuse* holder = nullptr;
        for (SmTopology* t : list_of(e->GetList()))
            if (((SmEdgeuse*)t)->GetFace()->GetSurface() == (f ? f->GetSurface() : nullptr))
            {
                holder = (SmEdgeuse*)t;
                break;
            }
        o.push_back(c ? 1.0 : 0.0);
        o.push_back((double)g.find(6, holder));
        break;
    }
    case OP_SEAM:
    {
        SmFace* f = (SmFace*)target_of(g, SmFace_TYPE, rd.idx());
        bool ok = true;
        for (SmTopology* t : list_of(e->GetList()))
            if (!ok_face((SmEdgeuse*)t))
                ok = false;
        if (f != nullptr)
        {
            if (f->GetSurface() == nullptr)
                ok = false;
            for (SmTopology* t : list_of(e->GetList()))
                if (ok_face((SmEdgeuse*)t) && ((SmEdgeuse*)t)->GetFace() == nullptr)
                    ok = false;
        }
        if (!ok)
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        o.push_back(flg(f ? e->IsSeam(*f->GetSurface()) : e->IsSeam()));
        break;
    }
    case OP_TOPOLOGICAL:
    {
        double gain = rd.num();
        bool want = rd.flag();
        SmTArray<SmEdgeuse*> eus;
        e->GetEdgeuses(eus);
        if (eus.GetSize() == 4 && eus[0]->IsLoopEdgeuse())
        {
            bool ok = ok_face(eus[0]) && ok_face(eus[2]) && eus[0]->GetFace() && eus[2]->GetFace() &&
                      eus[0]->GetFace()->GetSurface() && eus[2]->GetFace()->GetSurface();
            if (!ok)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
        }
        SmSurface* keep = nullptr;
        double dist = 0.0;
        SmExtent2d dom(SmPoint2d(-7, -7), SmPoint2d(-7, -7));
        SmBoolean r = e->IsTopological(keep, dist, gain, want ? &dom : NULL);
        o.push_back(flg(r));
        int64_t kf = -1;
        for (size_t k = 0; k < g.faces.size(); k++)
            if (keep != nullptr && g.faces[k]->GetSurface() == keep)
                kf = (int64_t)k;
        o.push_back((double)kf);
        o.push_back(dist);
        break;
    }
    case OP_TANGENT:
    {
        double tol = rd.num();
        bool coin = rd.flag();
        o.push_back(flg(e->IsTangentEdge(tol, coin ? TRUE : FALSE)));
        break;
    }
    case OP_BLEND:
    {
        SmEdgeuse* p = e->GetPrimaryEdgeuse();
        if (!e->IsLamina() && (p == nullptr || p->GetMate() == nullptr || p->GetShell() == nullptr ||
                               p->GetMate()->GetShell() == nullptr))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        o.push_back((double)g.find(6, e->GetBlendEdgeuse()));
        break;
    }
    case OP_ODE:
    {
        SmPoint3d origin = rd.vec();
        double radius = rd.num();
        double t = rd.num();
        int64_t n = rd.idx();
        if (e->GetCurve() == nullptr || n < 28)
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmTArray<double> dydx((ULONG)n, NULL, (ULONG)n);
        for (ULONG k = 0; k < dydx.GetSize(); k++)
            dydx[k] = 1.0;
        o.push_back(st(ode_edge_length_evaluate(*e->GetCurve(), origin, radius, t, dydx)));
        for (ULONG k = 0; k < dydx.GetSize(); k++)
            o.push_back(dydx[k]);
        break;
    }
    case OP_SET_INTERVAL:
    {
        double a = rd.num();
        double c = rd.num();
        e->SetInterval(SmExtent1d(a, c));
        SmExtent1d ivl = e->GetInterval();
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
        break;
    }
    default:
        break;
    }
}
} // namespace

SC_EXPORT int32_t sc_topology_edge_eval(int32_t op, const char* path, const double* in, int64_t n_in, double* out,
                                        int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    run_brep_op(op, rd, path, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
