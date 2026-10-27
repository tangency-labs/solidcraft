// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.face_p2`: the SmFace members owned by the unit
// (source/SMLib/src/SmFace.cpp, source/SMLib/inc/SmFace.h) on real SmBrep objects, plus
// the file-static helpers and file-local classes of SmFace.cpp.
//
// Static functions and file-local classes of SmFace.cpp cannot be linked against, so
// sm_CheckSeamCrossing, sm_AreConnected, SmPrecisePropertyLayout, SmODEAreaIFEO,
// SmODEIsoCurveLengthIFEO, sm_FindBestUVCurve and SmRescueSurfaceOwner are VERBATIM
// copies of upstream SmFace.cpp (commit c9c5979) inside namespace sc_face_p2 (line ranges
// noted at each block; SM_DEBUG_CODE is not defined in the reference build).
// sm_FixUVTrimCurves and sm_RemoveSuspectSplitAtSeamUVTrimCurves have external linkage
// and are called directly.
//
// One entry point, sc_topology_face_p2_eval(op, path, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/topology_face_p2.rs, which
// validates every input. The B-rep construction, traversal and export are those of the
// `topology.vertexuse` shim (harness/cpp/shim/topology_vertexuse.cpp), with each face's
// raw members (face use, UV domain, rectangular-trim flag, tolerance, whether its surface
// is owned by it), SmPlane surfaces as planes, and lossless analytic surface and edge
// ellipse/circle heads from the shared exporters added to the export. Every B-rep op
// starts with a B-rep block:
//   `0 rebuild validate_only init_gaps`         SmBrep::ReadFromFile(path) (ASCII .smb)
//   `1 curve t0 t1 validate_only init_gaps`     CreateWireEdgeFromCurve(curve, [t0, t1])
//   `2 surface u0 v0 u1 v1 validate_only init_gaps` CreateFaceFromSurface(surface, dom)
//   `3 validate_only init_gaps`                 the wire network of prog_test my_test_topology
// Ops whose upstream path would dereference null output -2 instead of calling it.
#include "sc_analytic_export.h"
#include "sc_exact_curve_export.h"
#include "sc_shim.h"
#include "support/topology_fixture.h"


#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmCacheMgr.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFace.h>
#include <SmFaceuse.h>
#include <SmCrvOnSurf.h>
#include <SmCurveClass.h>
#include <SmIntegrator.h>
#include <SmOffsetSurface.h>
#include <SmPointClass.h>
#include <SmAxis2Placement.h>
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
    OP_FACE_QUERY = 1,
    OP_SET = 2,
    OP_POINTS = 3,
    OP_INTERNAL = 4,
    OP_FROM_FACE = 5,
    OP_TRIM = 6,
    OP_FIX_UV = 7,
    OP_BEST_UV = 8,
    OP_CONNECTED = 9,
    OP_RESCUE = 10,
    OP_REMOVE_SUSPECT = 11,
    OP_SEAM_CROSS = 20,
    OP_LAYOUT = 21,
    OP_ODE_ISO = 22,
    OP_ODE_AREA = 23,
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
    // the surface's own type: an SmBSplineSurface subclass other than SmPlane (a
    // revolution, torus, ...) is exported as its B-spline part only
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
        sc_analytic_export::put_exact_head(o, s, the_context());
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
        sc_exact_curve_export::put_export_curve(o, p->GetCurve(), the_context(), put_curve);
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
        SmPoint2d fuv = (p->*VuPeek::face_gap()).GetFaceUV();
        o.push_back(fuv.x);
        o.push_back(fuv.y);
        o.push_back((p->*VuPeek::face_gap()).GetLength());
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

const SM_TYPE KIND_TYPES[] = {SmFace_TYPE,    SmEdgeuse_TYPE, SmTopology_TYPE, SmOwningTopology_TYPE,
                              SmAObject_TYPE, SmObject_TYPE,  SmShell_TYPE,    SmFaceuse_TYPE,
                              SmUnknown_TYPE, 0};

} // namespace

// ---- functions of SmFace.cpp with external linkage (not declared in any header) ---------
SmStatus sm_FixUVTrimCurves(SmEdgeuse* pEUCurrent, SmEdgeuse* pEUPrevious, SmVertex& crVertex, double dTol3d,
                            double dUVDistBetween, const SmExtent2d& crUVDomain, SmBoolean& rbFixedCurves);
void sm_RemoveSuspectSplitAtSeamUVTrimCurves(SmCurveClassification* pCurveClassification);

namespace sc_face_p2
{
// clang-format off
// ---- verbatim: SmFace.cpp lines 132-228: sm_CheckSeamCrossing
static SmBoolean sm_CheckSeamCrossing
(SmPoint2d        & sUV1,        // i/o: previous uv point, possibly shifted by a period 
 SmPoint2d        & sUV2,        // in : current uv point 
 const SmSurface  * pSurf,       // NotUsed: in : the surface being walked 
 const SmExtent2d & rDomain,     // in : domain of pSurf 
 SmBoolean          bClosedU,    // in : Is the surface closed in U? 
 SmBoolean          bClosedV,    // in : Is the surface closed in V? 
 SmVector2d       & rParamShift) // i/o]: If we are across a seam, add this to the uv point for the area calculation 
{
  SM_REF1(pSurf) ;
  SmBoolean bRet = FALSE;

  if ( ! bClosedU && ! bClosedV )
    { return SM_SUCCESS; }


  if ( bClosedU )
  {
      SmExtent1d sIvl( rDomain.GetUInterval() );
      double dPeriod = sIvl.GetLength();
      double dJump = sUV2.x - sUV1.x;

      // The caller is stepping along the curve in 0.05 increments.
      if ( smos_Fabs( dJump ) > 0.90 * dPeriod )
      {
          if ( sUV2.x < sUV1.x )
            {
              // Jumped from high end to low.
              rParamShift.x += dPeriod;
              sUV1.x -= dPeriod;  // Bring prev pt into same period as curr pt.
            }
          else
            {
              // Jumped from low end to high.
              rParamShift.x -= dPeriod;
              sUV1.x += dPeriod;
            }

          bRet = TRUE;
      }
  }

  if ( bClosedV )
  {
      SmExtent1d sIvl( rDomain.GetVInterval() );
      double dPeriod = sIvl.GetLength();
      double dJump = sUV2.y - sUV1.y;

      // The caller is stepping along the curve in 0.05 increments.
      if ( smos_Fabs( dJump ) > 0.90 * dPeriod )
      {
          if ( sUV2.y < sUV1.y )
            {
              // Jumped from high end to low.
              rParamShift.y += dPeriod;
              sUV1.y -= dPeriod;
            }
          else
            {
              // Jumped from low end to high.
              rParamShift.y -= dPeriod;
              sUV1.y += dPeriod;
            }

          bRet = TRUE;
      }
  }


/* 
  if ( ! rDomain.IsPoint2dOnBoundary( sUV2 ) )
    { return SM_SUCCESS; }

  if ( bClosedU && ! sIvl.ContainsValue( sUV2.x, -SM_EFF_ZERO ) ) // neg tol: on-bdry returns False.
  {
      double dLim = sIvl.GetMin() + SM_EFF_ZERO;
      if ( sUV2.x <= dLim  &&  sUV1.x > dLim ) // Pushing low-u boundary
        {
          rParamShift += sIvl;
          bRet = TRUE;
        }

      dLim = sIvl.GetMax() - SM_EFF_ZERO;
      if ( sUV2.x >= dLim  &&  sUV1.x < dLim ) // Pushing high-u boundary
        {
          rParamShift -= sIvl;
          bRet = TRUE;
        }
  }

  ... repeat for V.
*/ 


  return bRet;

}  // end sm_CheckSeamCrossing

// ---- verbatim: SmFace.cpp lines 1512-1623: sm_AreConnected
static SmBoolean sm_AreConnected
(
  const SmEdgeuse * pEU1,                    // in : check pEU1->EndPt Position
  const SmEdgeuse * pEU2,                    // in : check pEU2->StartPt Position
  SmBoolean bSingularityOK = TRUE,           // in : TRUE = if UVGap > UVtol, Rtn TRUE if UVGap is along a Singularity boundary
                                             //      FALSE= if UVGap > UVTol, Always rtn FALSE
  SmBoolean bCrossesMissingSeamOK = FALSE,   // in : TRUE = if UVGap > UVtol, Rtn TRUE if UVGap is accross a missing seam
                                             //      FALSE= if UVGap > UVTol, Always rtn FALSE
  SmBoolean * pOptOnSingularity=NULL,        // out: TRUE=UVGap > UVtol && OnSingularity,FALSE=not, NULL to ignore, default:[NULL]
  SmBoolean * pOptCrossesMissingSeam=NULL    // out: TRUE=UVGap > UVTol && CrossesMissingSeam,FALSE=not, NULL to ignore, default:[NULL]
)
{
  // init output
  if( pOptOnSingularity)        { * pOptOnSingularity      = FALSE ; } 
  if( pOptCrossesMissingSeam  ) { * pOptCrossesMissingSeam = FALSE ; }
  
  // Get end of pEU1 and start of pEU2.
  SmVertexuse *pVU1 = pEU1->GetMate()->GetVertexuse() ; // End of this is start of Mate.
  SmVertexuse *pVU2 = pEU2->GetVertexuse() ;

  SmPoint2d sUV1, sUV2 ;
  pVU1->ComputeUVPoint( sUV1, FALSE ) ; // TRUE  = Use LocalSolve with guesses from UVTrimCurve
  pVU2->ComputeUVPoint( sUV2, FALSE ) ; // FALSE = don't make UVTrimCurve, if missing use GlobalSolve
  double dGapU = smos_Fabs(sUV2.x - sUV1.x) ;
  double dGapV = smos_Fabs(sUV2.y - sUV1.y) ;

  SmSurface * pSurf     = pEU1->GetFace()->GetSurface() ;
  SmZoneTol3d s3DTol    = pVU1->GetVertex()->GetTolerance() ;
  SmTol2d     sUVTol    = SmTol::MapTo2d( s3DTol, sUV1, *pSurf ) ;

  if(sUV1.DistanceBetween( sUV2 ) < sUVTol)
    { return TRUE; }

  // when gaps across singularities are allowed
  if(bSingularityOK )
    {
      SmSurfParamType eDir1 ; // output arg
      SmSurfParamType eDir2 ; // output arg
      SmBoolean bOnSingularity1 = pSurf->IsSingularity( sUV1, eDir1, s3DTol, TRUE ) ;
      SmBoolean bOnSingularity2 = pSurf->IsSingularity( sUV2, eDir2, s3DTol, TRUE ) ;

      // both ends are on a singularity
      if(bOnSingularity1 && bOnSingularity2)
        {
          // set output
          if(pOptOnSingularity) 
            { *pOptOnSingularity = TRUE ; }
        
          // check for a properly oriented singularity at the gap
          if((eDir1 == SM_SP_BOTH) && (eDir2 == SM_SP_BOTH))
            {
              { return TRUE ; }
            }

          else if((eDir1 == SM_SP_U || eDir1 == SM_SP_BOTH) && (eDir2 == SM_SP_U || eDir2 == SM_SP_BOTH))
            {
              if(dGapV < sUVTol)
                { return TRUE ; }
            }

          else if((eDir1 == SM_SP_V || eDir1 == SM_SP_BOTH) && (eDir2 == SM_SP_V || eDir2 == SM_SP_BOTH))
            {
              if(dGapU < sUVTol)
                { return TRUE ; }
            }
        } // end both points on singularity check
    } // end when gaps across singularities are allowed check

  // when gaps across MissingSeams are allowed
  if(bCrossesMissingSeamOK )
    {
      // check for a properly oriented seam gap
      SmExtent2d sUVDomain = pSurf->GetNaturalUVDomain() ;
      SmBoolean  bClosedU  = pSurf->IsClosed(sUVDomain, SM_SP_U, &sUVTol.val) ;
      SmBoolean  bClosedV  = pSurf->IsClosed(sUVDomain, SM_SP_V, &sUVTol.val) ; 
      
      // when surface is closed
      if(bClosedU || bClosedV)
        {
          if(bClosedU && bClosedV)
            {
              if(   ((dGapU < sUVTol) || (smos_Fabs(dGapU - sUVDomain.XLength()) < sUVTol))
                 && ((dGapV < sUVTol) || (smos_Fabs(dGapV - sUVDomain.YLength()) < sUVTol)))
                { if(pOptCrossesMissingSeam) 
                   { *pOptCrossesMissingSeam = TRUE ; }
                  return TRUE ; 
                }
            }
          else if(bClosedU)
            {
              if(   (smos_Fabs(dGapU - sUVDomain.XLength()) < sUVTol)
                 && (dGapV < sUVTol))
                { if(pOptCrossesMissingSeam) 
                   { *pOptCrossesMissingSeam = TRUE ; }
                  return TRUE ; 
                }
            }
          else // bClosedV
            {
              if(   (smos_Fabs(dGapV - sUVDomain.YLength()) < sUVTol)
                 && (dGapU < sUVTol))
                { if(pOptCrossesMissingSeam) 
                   { *pOptCrossesMissingSeam = TRUE ; }
                  return TRUE ; 
                }
            } // end Surf is closed check
        } // end connections crossing missing seams ok check
    } // end when gaps across singularities are allowed check
  // all done
  return FALSE;

}  // end static sm_AreConnected

// ---- verbatim: SmFace.cpp lines 5615-5974: SmPrecisePropertyLayout, SmODEAreaIFEO, SmODEIsoCurveLengthIFEO
class SmPrecisePropertyLayout
{
public:
    ULONG m_lNumComponents;
    ULONG m_lArea;
    ULONG m_lVolume;
    ULONG m_aMoments[8];

    explicit SmPrecisePropertyLayout(ULONG lFlags) : m_lNumComponents(1)
    {
        m_lArea = Add(lFlags & SM_PPF_AREA,1);
        m_lVolume = Add(lFlags & SM_PPF_VOLUME,1);
        const ULONG aMomentFlags[] = {
            SM_PPF_AREA_FIRST_MOMENTS, SM_PPF_AREA_SECOND_MOMENTS,
            SM_PPF_AREA_SECOND_MOMENTS, SM_PPF_AREA_SECOND_MOMENTS,
            SM_PPF_VOLUME_FIRST_MOMENTS, SM_PPF_VOLUME_SECOND_MOMENTS,
            SM_PPF_VOLUME_SECOND_MOMENTS, SM_PPF_VOLUME_SECOND_MOMENTS
        };
        for (ULONG i=0; i<8; ++i) m_aMoments[i] = Add(lFlags & aMomentFlags[i],3);
    }

private:
    ULONG Add(ULONG lSelected,ULONG lCount)
    {
        if (!lSelected) return 0;
        const ULONG lIndex = m_lNumComponents + 1;
        m_lNumComponents += lCount;
        return lIndex;
    }
};

/*******************************************************************//**
PURPOSE: Helper class for SmFace::ComputePreciseProperties

NOTES: 
***********************************************************************/
class SmODEAreaIFEO : public SmODEIntegFuncEvalObj
{
private:
    const SmSurface  & m_crSurface;
    const SmCurve    & m_crUVCurve;
    double             m_dMinV;
    double             m_dIsoCurveAccuracy;
    SmTArray<double> * m_pYScale;
    SmPoint3d          m_vOriginOfComputation;
    double             m_dFaceThickness;
    const SmPrecisePropertyLayout & m_crLayout;
public:
    SmODEAreaIFEO(const SmSurface & crSurface,
        const SmCurve & crUVCurve, double dMinV,
        double dIsoCurveAccuracy, SmTArray<double> * pYScale,
        const SmPoint3d & crOriginOfComputation,
        double dFaceThickness, const SmPrecisePropertyLayout & crLayout)
        : m_crSurface(crSurface), m_crUVCurve(crUVCurve),
          m_dMinV(dMinV),
          m_dIsoCurveAccuracy(dIsoCurveAccuracy),
          m_pYScale(pYScale),
          m_vOriginOfComputation(crOriginOfComputation),
          m_dFaceThickness(dFaceThickness), m_crLayout(crLayout)
    {}
    virtual SmStatus Evaluate(double dT,                             // in : 
                              SmTArray<double> & rYValues,           // NotUsed: out: 
                              SmTArray<double> & rDyDxValues) const; // out: 

} ; // end class SmODEAreaIFEO

/****************************************************************
PURPOSE:

NOTES:
****************************************************************/
class SmODEIsoCurveLengthIFEO : public SmODEIntegFuncEvalObj
{
private:
    const SmSurface & m_crSurface;
    double            m_dUIsoValue;
    SmPoint3d         m_vOriginOfComputation;
    double            m_dFaceThickness;
    const SmPrecisePropertyLayout & m_crLayout;
public:
    SmODEIsoCurveLengthIFEO(const SmSurface & crSurface,double dUIsoValue,
        const SmPoint3d & crOriginOfComputation,
        double dFaceThickness, const SmPrecisePropertyLayout & crLayout)
        : m_crSurface(crSurface), m_dUIsoValue(dUIsoValue),
        m_vOriginOfComputation(crOriginOfComputation), m_dFaceThickness(dFaceThickness),
        m_crLayout(crLayout)
    {}
    virtual SmStatus Evaluate(double dT,                             // in :
                              SmTArray<double> & rYValues,           // NotUsed: out:
                              SmTArray<double> & rDyDxValues) const; // out:

} ; // end class SmODEIsoCurveLengthIFEO

/****************************************************************
PURPOSE:

NOTES:
****************************************************************/
SmStatus SmODEIsoCurveLengthIFEO::Evaluate
 (double             dT,           // in :
  SmTArray<double> & rYValues,     // NotUsed: out:
  SmTArray<double> & rDyDxValues)  // out:
 const
{
  SM_REF1(rYValues) ;
    ULONG i, j, ii, kkk;
    SmVector3d sPV[2][2];
    SmVector3d sPV2[2][2];
    for (i=0; i<2; i++)
    {
        for (ii=0; ii<2; ii++)
        {   sPV [i][ii].x = sPV [i][ii].y = sPV [i][ii].z =0.0;
            sPV2[i][ii].x = sPV2[i][ii].y = sPV2[i][ii].z =0.0;
        }
    }

    for (j=0; j<rDyDxValues.GetSize(); j++) {
        rDyDxValues[j] = 0.0;
    }

    double dOffsetAreaSum = 0.0;
    for (i=0; i<2; i++) {
        if (m_dFaceThickness < SM_EFF_ZERO && i == 1) continue;

        SmPoint2d sUVPnt(m_dUIsoValue,dT);

        if (m_dFaceThickness < SM_EFF_ZERO) {
            SER(m_crSurface.EvaluateSimple(sUVPnt,1,1,TRUE,TRUE,TRUE,sPV[0]));
        }
        else if (i==0) {
            SmOffsetSurface sOffset(m_dFaceThickness/2.0,((SmSurface&)m_crSurface));
            sOffset.SetContext(m_crSurface.GetContext());
            SER(sOffset.EvaluateSimple(sUVPnt,1,1,TRUE,TRUE,TRUE,sPV[0]));
        }
        else {
            sPV2[0][0] = sPV[0][0];
            sPV2[1][0] = sPV[1][0];
            sPV2[0][1] = sPV[0][1];
            SmOffsetSurface sOffset(-m_dFaceThickness/2.0,((SmSurface&)m_crSurface));
            sOffset.SetContext(m_crSurface.GetContext());
            SER(sOffset.EvaluateSimple(sUVPnt,1,1,TRUE,TRUE,TRUE,sPV[0]));
        }

        sPV[0][0] = sPV[0][0] - m_vOriginOfComputation;
        // already done: sPV2[0][0] = sPV2[0][0] - m_vOriginOfComputation;
#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
        if (bDebugMe) {
            sPV[0][0].Draw();
            sm_GraphicsLoop();
        }
#endif // SM_DEBUG_CODE
        // Compute first fundamental form
        //    double dE = sPV[1][0].Dot(sPV[1][0]);
        //    double dF = sPV[1][0].Dot(sPV[0][1]);
        //    double dG = sPV[0][1].Dot(sPV[0][1]);
        //    rdResult = smos_Sqrt(dE * dG - dF * dF);
        SmVector3d sNorm = sPV[1][0] * sPV[0][1];

        double dAreaDelta = sNorm.Length();
        SmPoint3d sIPnt = sPV[0][0];  // Point to integrate
        rDyDxValues[1] = 0.0;
        if (sPV[1][0].Length() > 0) {
            rDyDxValues[1] += dAreaDelta;  // This is the integration function used
        }
        // to control tolerances
        dOffsetAreaSum += dAreaDelta; // Also needed for thick-face volume, even without AREA.
        if (m_crLayout.m_lArea) rDyDxValues[m_crLayout.m_lArea] += dAreaDelta;

        // Compute area static moments
        ULONG lIndex = m_crLayout.m_aMoments[0];
        if (lIndex) {
            rDyDxValues[lIndex]   += sIPnt.x * dAreaDelta;
            rDyDxValues[lIndex+1] += sIPnt.y * dAreaDelta;
            rDyDxValues[lIndex+2] += sIPnt.z * dAreaDelta;
        }

        lIndex = m_crLayout.m_aMoments[1];
        if (lIndex) {
            // Compute area Ixx, Iyy, Izz
            rDyDxValues[lIndex]   += (sIPnt.x*sIPnt.x) * dAreaDelta;
            rDyDxValues[lIndex+1] += (sIPnt.y*sIPnt.y) * dAreaDelta;
            rDyDxValues[lIndex+2] += (sIPnt.z*sIPnt.z) * dAreaDelta;

            // Compute area Iyz, Izx, Ixy
            rDyDxValues[lIndex+3] += (sIPnt.y*sIPnt.z) * dAreaDelta;
            rDyDxValues[lIndex+4] += (sIPnt.z*sIPnt.x) * dAreaDelta;
            rDyDxValues[lIndex+5] += (sIPnt.x*sIPnt.y) * dAreaDelta;

            // Compute area second moments Ixx, Iyy, Izz about coordinate axes
            rDyDxValues[lIndex+6] += (sIPnt.y*sIPnt.y + sIPnt.z*sIPnt.z) * dAreaDelta;
            rDyDxValues[lIndex+7] += (sIPnt.x*sIPnt.x + sIPnt.z*sIPnt.z) * dAreaDelta;
            rDyDxValues[lIndex+8] += (sIPnt.x*sIPnt.x + sIPnt.y*sIPnt.y) * dAreaDelta;
        }

        // Volume moments need the local volume contribution, but not its total
        // or any area moments. Surface-only queries skip all volume arithmetic.
        if (!m_crLayout.m_lVolume && !m_crLayout.m_aMoments[4] && !m_crLayout.m_aMoments[5]) continue;

        // If we have a thick face then we need to wait until the second
        // time around the loop.
        if (m_dFaceThickness > SM_EFF_ZERO && i==0) continue;

        // Now Do Mass Properties
        if (m_dFaceThickness < SM_EFF_ZERO) {
            // Do volumetric property
            double dDistToPlane = sPV[0][0].z;
            SmVector3d sUnitNorm = sNorm;
            if (sUnitNorm.LengthSquared() > SM_EFF_ZERO_SQ) {
                SER(sUnitNorm.Unitize());
            }
            double dVolumeDelta = dAreaDelta * dDistToPlane * sUnitNorm.z;
            if (m_crLayout.m_lVolume) rDyDxValues[m_crLayout.m_lVolume] = dVolumeDelta;

            // Compute volume static moments
            lIndex = m_crLayout.m_aMoments[4];
            if (lIndex) {
                rDyDxValues[lIndex]   = sIPnt.x * dVolumeDelta;
                rDyDxValues[lIndex+1] = sIPnt.y * dVolumeDelta;
                rDyDxValues[lIndex+2] = sIPnt.z * dVolumeDelta / 2.0;
            }

            lIndex = m_crLayout.m_aMoments[5];
            if (lIndex) {
                // Compute Volume Moments Ixx, Iyy, Izz
                rDyDxValues[lIndex]   = (sIPnt.x*sIPnt.x) * dVolumeDelta;
                rDyDxValues[lIndex+1] = (sIPnt.y*sIPnt.y) * dVolumeDelta;
                rDyDxValues[lIndex+2] = (sIPnt.z*sIPnt.z/3.0) * dVolumeDelta;

                // Compute Volume Moments Iyz, Izx, Ixy
                rDyDxValues[lIndex+3] = (sIPnt.y*sIPnt.z) * dVolumeDelta / 2.0;
                rDyDxValues[lIndex+4] = (sIPnt.z*sIPnt.x) * dVolumeDelta / 2.0;
                rDyDxValues[lIndex+5] = (sIPnt.x*sIPnt.y) * dVolumeDelta;

                // Compute Volume Second Moments Ixx, Iyy, Izz about Coordinate Axes
                rDyDxValues[lIndex+6] = (sIPnt.y*sIPnt.y + sIPnt.z*sIPnt.z/3.0) * dVolumeDelta;
                rDyDxValues[lIndex+7] = (sIPnt.x*sIPnt.x + sIPnt.z*sIPnt.z/3.0) * dVolumeDelta;
                rDyDxValues[lIndex+8] = (sIPnt.x*sIPnt.x + sIPnt.y*sIPnt.y) * dVolumeDelta;
            }
        }
        else {
            // Do volumetric property
            // Volume is average of areas times thickness of face.
            double dVolumeDelta = dOffsetAreaSum * m_dFaceThickness / 2.0;
            if (m_crLayout.m_lVolume) rDyDxValues[m_crLayout.m_lVolume] = dVolumeDelta;

            // The thickness-direction centroid is only needed for moments.
            if (!m_crLayout.m_aMoments[4] && !m_crLayout.m_aMoments[5]) continue;

            // Find New Center of Gravity for little volume.
            SmVector3d sNormFirst = sPV2[1][0] * sPV2[0][1];
            double dAreaDeltaFirst = sNormFirst.Length();

            // Perform a fake integration over the area to find the COG
            double dDelta = m_dFaceThickness/100;
            double dTotalVolume = dDelta * dAreaDeltaFirst;
            double dTValue = 0.0;

            for (kkk=0; kkk<100; kkk++) {
                double dTol = (kkk+1.0) / 100.0;
                double dCurrArea = dAreaDeltaFirst + dTol * (dAreaDelta - dAreaDeltaFirst);
                double dCurrVolume = dCurrArea * dDelta;
                double dT2 = dCurrVolume / (dCurrVolume+dTotalVolume);
                dTValue = dTValue + dT2 * (dTol - dTValue);
                dTotalVolume += dCurrVolume;
            }

            double dTNew = dTValue;

            // Compute new center point
            SmVector3d sVec = sPV[0][0] - sPV2[0][0];
            sIPnt = sPV2[0][0] + dTNew * sVec;

            // Compute volume static moments
            lIndex = m_crLayout.m_aMoments[4];
            if (lIndex) {
                rDyDxValues[lIndex]   = sIPnt.x * dVolumeDelta;
                rDyDxValues[lIndex+1] = sIPnt.y * dVolumeDelta;
                rDyDxValues[lIndex+2] = sIPnt.z * dVolumeDelta;
            }

            lIndex = m_crLayout.m_aMoments[5];
            if (lIndex) {
                // Compute Volume Moments Ixx, Iyy, Izz
                rDyDxValues[lIndex]   = (sIPnt.x*sIPnt.x) * dVolumeDelta;
                rDyDxValues[lIndex+1] = (sIPnt.y*sIPnt.y) * dVolumeDelta;
                rDyDxValues[lIndex+2] = (sIPnt.z*sIPnt.z) * dVolumeDelta;

                // Compute Volume Moments Iyz, Izx, Ixy
                rDyDxValues[lIndex+3] = (sIPnt.y*sIPnt.z) * dVolumeDelta;
                rDyDxValues[lIndex+4] = (sIPnt.z*sIPnt.x) * dVolumeDelta;
                rDyDxValues[lIndex+5] = (sIPnt.x*sIPnt.y) * dVolumeDelta;

                // Compute Volume Second Moments Ixx, Iyy, Izz about Coordinate Axes
                rDyDxValues[lIndex+6] = (sIPnt.y*sIPnt.y + sIPnt.z*sIPnt.z) * dVolumeDelta;
                rDyDxValues[lIndex+7] = (sIPnt.x*sIPnt.x + sIPnt.z*sIPnt.z) * dVolumeDelta;
                rDyDxValues[lIndex+8] = (sIPnt.x*sIPnt.x + sIPnt.y*sIPnt.y) * dVolumeDelta;
            }
        }
    }

    return SM_SUCCESS;

} // end SmODEIsoCurveLengthIFEO::Evaluate

/*******************************************************************//**
PURPOSE:

NOTES:
***********************************************************************/
SmStatus SmODEAreaIFEO::Evaluate
(
  double             dT,          // in :
  SmTArray<double> & rYValues,    // NotUsed: out:
  SmTArray<double> & rDyDxValues  // out:
) const
{
  SM_REF1(rYValues) ;
    SmPoint3d sPV[2];
    SER(m_crUVCurve.Evaluate(dT,1,TRUE,sPV));

    SmODEIsoCurveLengthIFEO sEval(m_crSurface,sPV[0].x,m_vOriginOfComputation,m_dFaceThickness,m_crLayout);
    SmODEIntegrator sIntegrator(sEval);

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
    if (bDebugMe) {
        smgfx_ChangeColor(FALSE);
        SmIsoCurve sCrv(m_crSurface,SM_SP_U,sPV[0].x,TRUE);
        sCrv.SetContext(NULL);
        if (smos_Fabs(sPV[0].y - m_dMinV) > SM_EFF_ZERO_SQRT) {
            SmExtent1d sIvl(m_dMinV,sPV[0].y);
            sCrv.DrawWDeriv(sIvl,0);
            sm_GraphicsLoop();
        }
    }
#endif // SM_DEBUG_CODE

    if (smos_Fabs(sPV[1].x) > SM_EFF_ZERO) {
        const ULONG lNumComponents = m_pYScale->GetSize();
        SmTArray<double> dY(lNumComponents,NULL,lNumComponents);
        double dStep = sPV[0].y - m_dMinV;

        ULONG lNumOK, lNumBad;
        // A failed inner integration is not a zero-area contribution; return
        // the failure so callers cannot accept plausible partial properties.
        SER(sIntegrator.RungeKuttaODEIntegrate(dY,m_dMinV,
            sPV[0].y, m_dIsoCurveAccuracy, dStep/2.0, 0.0, lNumOK, lNumBad,
            m_pYScale));
        rDyDxValues.ReSet();
        rDyDxValues.Add(0.0);
        rDyDxValues.Append(dY);
        for (ULONG i=1; i<rDyDxValues.GetSize(); i++) {
            rDyDxValues[i] = rDyDxValues[i] * sPV[1].x * -1.0;
        }
    }

    return SM_SUCCESS;

} // end SmODEAreaIFEO::Evaluate

// ---- verbatim: SmFace.cpp lines 7769-7920: sm_FindBestUVCurve
static SmStatus sm_FindBestUVCurve
  (const SmSurface              * cpSurface,        // in : target surface
   SmEdgeuse                    * cpEU,             // in : target edgeuse
   SmTArray<SmBSplineCurve*>    & rUVCurves,        // in : array of candidate UVCurves
   const SmTArray<SmOrientType> & rOrientations,    // in : associated orientations
   SmBSplineCurve               *&rpUVCurve)        // out: the found UVCurve
{
  // init output
  rpUVCurve       = NULL;

  // locals
  SmEdgeuse *pPrimEU = cpEU->GetEdge()->GetPrimaryEdgeuse();

  // get edgeuse->3DMidpoint and orientation adjusted tangent
  SmVector3d sEdgePV[2];
  SmCurve  * pCurve = cpEU->GetEdge()->GetCurve();
  SmExtent1d sIvl   = cpEU->GetEdge()->GetInterval();
  SER(pCurve->Evaluate(sIvl.Evaluate(0.5), 1, TRUE, sEdgePV));
  if (pPrimEU->GetOrientation() != cpEU->GetOrientation())
    {
      sEdgePV[1] = - sEdgePV[1];
    }

  // Find the closest UV curve which has the same orientation
  // of tangent when lifted into 3d.
  ULONG lClosest = 0;
  double dClosestDist = SM_BIG_DOUBLE;
  for (ULONG i=0; i<rUVCurves.GetSize(); i++)
    {
      SmBSplineCurve *pUVCurve = rUVCurves[i];
      if (pUVCurve == NULL) continue; // this is ok - not an error

      // project UVTrimCurve into 3D space
      SmCrvOnSurf sCrvOnSurf(*pUVCurve,SM_CONST_CAST(SmSurface&,*cpSurface), NULL, 0, cpEU->GetContext());
//      sCrvOnSurf.SetContext(NULL);

 // for debug only force the construction of the cache
#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE ;
      if(bDebugMe)
        {
          double dLength ;
          SmExtent1d sInterval = pUVCurve->GetNaturalInterval() ;
          sCrvOnSurf.Length( sInterval, .0001, dLength) ;
        }
#endif // SM_DEBUG_CODE

      // get UVTrimCurve->3DMidPoint and orientation adjusted Tangent
      SmExtent1d sInterval = sCrvOnSurf.GetNaturalInterval();
      SmVector3d sPV[2];
      SER(sCrvOnSurf.Evaluate( sInterval.Evaluate(0.5),1,TRUE,sPV));
      if (rOrientations[i] != pPrimEU->GetOrientation())
        {
          sPV[1] = - sPV[1];
        }

#ifdef SM_DEBUG_CODE
      // draw Edge MidTangent(red), UVCurve->3DMidTangent(green), UVCurve->3DCurve(black), Edge(blue)
      //      Edge StartTangent(red), UVCurve->3DStartTangent(green)
      if (bDebugMe)
        {
          SmFace *pFace = (SmFace *)cpSurface->GetFace() ;
          SmBrep *pBrep = pFace ? pFace->GetBrep() : NULL ;

          smgfx_Erase();
          smgfx_SetLook(1,2, 0,0,1); if(pBrep) pBrep->Draw(TRUE) ; sm_GraphicsLoop() ;
          smgfx_SetLook(3,4, 1,0,0); sEdgePV[0].Draw(); sEdgePV[1].Draw(&sEdgePV[0]); sm_GraphicsLoop() ;
          smgfx_SetLook(4,5, 0,1,0); sPV[0].Draw(); sPV[1].Draw(&sPV[0]); sm_GraphicsLoop() ;
          smgfx_SetLook(1,2, 0,0,0); sCrvOnSurf.Draw(); sm_GraphicsLoop() ;
          smgfx_SetLook(2,3, 0,0,1); cpEU->GetEdge()->GetCurve()->DrawParams(); sm_GraphicsLoop();

          // evaluate edge and UVTrimCurve->3Dcurve startPoint and Tangent
          SmPoint3d sStart, sTanStart;
          SmVector3d sPVSt[2];
          SER(cpEU->NormalizedEvaluate(0.0,FALSE,sStart,&sTanStart));  // TRUE = UV Eval, FALSE = 3d Eval
          SER(sCrvOnSurf.Evaluate( sInterval.Evaluate(0.0),1,TRUE,sPVSt));

          smgfx_SetLook(5,6, 1,0,0); sStart.Draw(); sTanStart.Draw(&sStart); sm_GraphicsLoop() ;
          smgfx_SetLook(6,7, 0,1,0); sPVSt[0].Draw(); sPVSt[1].Draw(&sPVSt[0]); sm_GraphicsLoop() ;
          sm_GraphicsLoop();
        }
#endif // SM_DEBUG_CODE

      // skip UVCurve with opposite pointing tangents
      if (sPV[1].Dot(sEdgePV[1]) < 0.0)
        { continue;
        }

      // use UVCurves that are more than SM_EFF_ZERO from edge
      double dDist = sPV[0].DistanceBetween(sEdgePV[0]);
      if (dDist < dClosestDist)
        {
          // save best hit
          dClosestDist = dDist;
          lClosest = i;

          // use exact hits
          if (dClosestDist < SM_EFF_ZERO)
            {
              // found a keeper
              break; // Should be close enough
            }
        }
    } // end iter every UVTrimCurve

  // when best hit exceeds tolerance
  if (   dClosestDist == SM_BIG_DOUBLE
      || dClosestDist > cpEU->GetEdge()->GetTolerance() * 100.0)
    {
      // The UV curve is either not found or not very good.
      // Set the return to NULL and the above routine will
      // recompute the UV Curve. gwc: no need to inform the public - this is working path
      //  SM_DBG_WARN(_T("UV curve not good enough - recomputing"));
      rpUVCurve = NULL;
      return SM_SUCCESS;

    } // end UVTrimCurve not Found check

  // use the found UVTrimCurve for this Edgeuse
  rpUVCurve = rUVCurves[lClosest];

  // reverse UVTrimCurve orientation as needed
  if (rOrientations[lClosest] != pPrimEU->GetOrientation())
    {
     SmExtent1d sInterval = rpUVCurve->GetNaturalInterval();
     SER(rpUVCurve->ReverseParameterization( sInterval, sInterval ));
    }

  // revertex Edgeuse orientation as needed
  if (cpEU->GetOrientation() != cpEU->GetEdge()->GetPrimaryEdgeuse()->GetOrientation())
    {
      SmExtent1d sInterval = rpUVCurve->GetNaturalInterval();
      SER(rpUVCurve->ReverseParameterization( sInterval, sInterval ));
    }

  // set UVTrimCurve parameterRange equal to Edge parameter range
  SER(rpUVCurve->EditParameterization(cpEU->GetEdge()->GetInterval()));

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
  if (bDebugMe)
    {
      rpUVCurve->Dump();
      cpEU->GetEdge()->GetCurve()->Dump();
    }
#endif // SM_DEBUG_CODE

  // all done
  rUVCurves[lClosest] = NULL;   // Set to null to prevent reuse
  return SM_SUCCESS;

} // end sm_FindBestUVCurve

// ---- verbatim: SmFace.cpp lines 7927-7940: SmRescueSurfaceOwner
class SmRescueSurfaceOwner
{
  SmSurface *m_psSurface;
  SmObject  *m_psOwner;

public:
  SmRescueSurfaceOwner(SmSurface *psSurface)  { m_psSurface = psSurface;
                                                m_psOwner   = m_psSurface->GetOwner();
                                              }
  ~SmRescueSurfaceOwner(void)                 { m_psSurface->SetOwner(m_psOwner);
                                                m_psOwner   = NULL;
                                                m_psSurface = NULL;
                                              }
} ; // end classSmRescueSurfaceOwner
// clang-format on
} // namespace sc_face_p2

namespace
{
// ---- null-dereference guards (what upstream dereferences without checking) ------------

SmSurface* surf_of(SmFace* f) { return f->*FPeek::srf(); }

SmTopology* owner_of(SmVertexuse* p) { return p->*VuPeek::owner(); }

// GetFaceuse dereferences the loop use of a loop vertex use (as the vertexuse shim).
bool ok_faceuse(SmVertexuse* p) { return !p->IsLoopVertexuse() || owner_of(p) != nullptr; }

// GetVertexusesOfFace on the vertex (as the vertexuse shim).
bool ok_siblings(SmVertexuse* p, const void* target)
{
    if (target == nullptr)
        return true;
    SmVertex* v = p->GetVertex();
    if (v == nullptr)
        return false;
    SmTArray<SmVertexuse*> all;
    v->GetVertexuses(all);
    for (ULONG i = 0; i < all.GetSize(); i++)
        if (!ok_faceuse(all[i]))
            return false;
    return true;
}

// CalcVertexFaceGap reaches ComputeUVPoint after GetVertexusesOfFace (as the vertexuse
// shim).
bool ok_face_gap(SmVertexuse* p)
{
    if (!p->HasVertexFaceGap())
        return true;
    if (!ok_faceuse(p))
        return false;
    SmFaceuse* fu = p->GetFaceuse();
    SmFace* f = fu ? fu->GetFace() : nullptr;
    if (!ok_siblings(p, f))
        return false;
    return p->GetVertex() != nullptr;
}

void put_loops(std::vector<double>& o, Graph& g, const SmTArray<SmLoop*>& ls)
{
    std::vector<int64_t> v;
    for (ULONG k = 0; k < ls.GetSize(); k++)
        v.push_back(g.find(5, ls[k]));
    put_list(o, v);
}

void put_string(std::vector<double>& o, const TCHAR* s)
{
    std::vector<double> c;
    for (const TCHAR* q = s; q && *q; q++)
        c.push_back((double)*q);
    o.push_back((double)c.size());
    o.insert(o.end(), c.begin(), c.end());
}

void put_domain(std::vector<double>& o, const SmExtent2d& d)
{
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
}

// The holder of an edge use's UV trim curve as GetUVTrimCurve finds it.
SmBSplineCurve* uv_of(SmEdgeuse* p) { return p ? p->GetUVTrimCurve() : nullptr; }

// What sm_FixUVTrimCurves dereferences past its early returns.
bool ok_fix(SmEdgeuse* cur, SmEdgeuse* prev)
{
    if (cur == nullptr || prev == nullptr)
        return false;
    SmFace* f = cur->GetFace();
    if (f == nullptr || surf_of(f) == nullptr)
        return false;
    if (cur->GetEdge() == nullptr || prev->GetEdge() == nullptr)
        return false;
    if (cur->GetEdge()->GetCurve() == nullptr || prev->GetEdge()->GetCurve() == nullptr)
        return false;
    if (uv_of(cur) == nullptr || uv_of(prev) == nullptr)
        return false;
    SmVertexuse* vu = cur->GetVertexuse();
    if (vu == nullptr || vu->GetVertex() == nullptr || !ok_faceuse(vu) || !ok_face_gap(vu))
        return false;
    return true;
}

// What sm_AreConnected dereferences (and ComputeUVPoint's vertex).
bool ok_connected(SmEdgeuse* a, SmEdgeuse* b)
{
    if (a->GetMate() == nullptr || a->GetMate()->GetVertexuse() == nullptr || b->GetVertexuse() == nullptr)
        return false;
    if (a->GetMate()->GetVertexuse()->GetVertex() == nullptr || b->GetVertexuse()->GetVertex() == nullptr)
        return false;
    if (!ok_faceuse(a->GetMate()->GetVertexuse()) || !ok_faceuse(b->GetVertexuse()))
        return false;
    SmFace* f = a->GetFace();
    return f != nullptr && surf_of(f) != nullptr;
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
    switch (op)
    {
    case OP_FACE_QUERY:
    {
        SmFace* p = g.faces[(size_t)rd.idx()];
        SmBoolean tgt_only = rd.bflag();
        bool has_loops = rd.flag();
        int64_t nl = rd.idx();
        SmTArray<SmLoop*> loops;
        for (int64_t k = 0; k < nl; k++)
            loops.Add(g.loops[(size_t)rd.idx()]);
        o.push_back(p->GetSurface() ? 1.0 : 0.0);
        put_domain(o, p->GetUVDomain());
        o.push_back((double)p->GetRectangularTrim());
        o.push_back((double)p->GetTolerance());
        SmFaceuse *fu1 = nullptr, *fu2 = nullptr;
        p->GetFaceuses(fu1, fu2);
        o.push_back((double)g.find(2, fu1));
        o.push_back((double)g.find(2, fu2));
        o.push_back((double)g.find(2, p->GetUpwardFaceuse()));
        SmTArray<SmLoop*> ls;
        p->GetLoops(ls, tgt_only, has_loops ? &loops : NULL);
        put_loops(o, g, ls);
        p->GetOuterLoops(ls);
        put_loops(o, g, ls);
        o.push_back((double)p->GetType());
        o.push_back((double)SmFace::GetClassType());
        for (const SM_TYPE* t = KIND_TYPES; *t != 0; t++)
            o.push_back(flg(p->IsKindOf(*t)));
        put_string(o, p->GetTypeString());
        put_string(o, p->GetClassString());
        break;
    }
    case OP_SET:
    {
        SmFace* p = g.faces[(size_t)rd.idx()];
        double u0 = rd.num(), v0 = rd.num(), u1 = rd.num(), v1 = rd.num();
        int64_t fu = rd.idx();
        SmFaceuse* old = p->*FPeek::fu();
        p->SetUVDomain(SmExtent2d(u0, v0, u1, v1));
        p->SetFaceuse(fu < 0 ? nullptr : g.faceuses[(size_t)fu]);
        put_domain(o, p->GetUVDomain());
        o.push_back((double)g.find(2, p->GetUpwardFaceuse()));
        SmFaceuse *fu1 = nullptr, *fu2 = nullptr;
        p->GetFaceuses(fu1, fu2);
        o.push_back((double)g.find(2, fu1));
        o.push_back((double)g.find(2, fu2));
        SmTArray<SmLoop*> ls;
        p->GetLoops(ls);
        put_loops(o, g, ls);
        p->SetFaceuse(old); // restore before the B-rep is deleted
        break;
    }
    case OP_POINTS:
    {
        SmFace* p = g.faces[(size_t)rd.idx()];
        ULONG count = (ULONG)rd.idx();
        if (rd.flag())
        {
            // a replacement UV domain (SetUVDomain) first
            double u0 = rd.num(), v0 = rd.num(), u1 = rd.num(), v1 = rd.num();
            p->SetUVDomain(SmExtent2d(u0, v0, u1, v1));
        }
        SmExtent2d d = p->*FPeek::dom();
        bool degenerate = d.GetUInterval().GetLength() <= SM_EFF_ZERO || d.GetVInterval().GetLength() <= SM_EFF_ZERO;
        if (!degenerate && count > 0 && surf_of(p) == nullptr)
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmTArray<SmPoint2d> uv;
        SmTArray<SmPoint3d> pv;
        o.push_back(st(p->GetPointsInFace(count, uv, pv)));
        o.push_back((double)uv.GetSize());
        for (ULONG k = 0; k < uv.GetSize(); k++)
        {
            o.push_back(uv[k].x);
            o.push_back(uv[k].y);
        }
        for (ULONG k = 0; k < pv.GetSize(); k++)
            put_vec(o, pv[k]);
        break;
    }
    case OP_INTERNAL:
    {
        SmFace* p = g.faces[(size_t)rd.idx()];
        SmPoint3d q(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        // Drop the untrimmed trimmed-surface cache that reading the file left on the face's
        // surface (ReadFromFile's healer builds it while Brep editing is enabled); the Rust
        // side imports the exported B-rep without caches (query.face_p3 does the same).
        SmSurface* srf = p->GetSurface();
        if (srf != nullptr && SmCacheMgr::GetObjectCache(SM_OC_TRIMSRF, srf) != nullptr)
            SmCacheMgr::DeleteObjectsCache(SM_OC_TRIMSRF, srf);
        o.push_back(st(p->CalculateAnInternalPoint(q)));
        put_vec(o, q);
        break;
    }
    case OP_FROM_FACE:
    {
        SmFace* p = g.faces[(size_t)rd.idx()];
        SmOrientType orient = (SmOrientType)rd.idx();
        bool want3 = rd.flag();
        bool want_uv = rd.flag();
        if (want3 || want_uv)
        {
            SmFaceuse* up = p->GetUpwardFaceuse();
            if (up == nullptr || (up->GetOrientation() != orient && up->GetMate() == nullptr))
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
        }
        SmTArray<ULONG> loops;
        SmTArray<SmCurve*> c3;
        SmTArray<SmBSplineCurve*> cuv;
        SmTArray<SmOrientType> orients;
        SmTArray<SmPoint3d> pts;
        SmStatus r = p->CreateCurvesFromFace(ctx, orient, loops, want3 ? &c3 : NULL, want_uv ? &cuv : NULL, orients, pts);
        o.push_back(st(r));
        o.push_back((double)loops.GetSize());
        for (ULONG k = 0; k < loops.GetSize(); k++)
            o.push_back((double)loops[k]);
        o.push_back((double)orients.GetSize());
        for (ULONG k = 0; k < orients.GetSize(); k++)
            o.push_back((double)orients[k]);
        o.push_back((double)pts.GetSize());
        for (ULONG k = 0; k < pts.GetSize(); k++)
            put_vec(o, pts[k]);
        o.push_back((double)c3.GetSize());
        for (ULONG k = 0; k < c3.GetSize(); k++)
        {
            put_curve(o, c3[k]);
            delete c3[k];
        }
        o.push_back((double)cuv.GetSize());
        for (ULONG k = 0; k < cuv.GetSize(); k++)
        {
            put_curve(o, cuv[k]);
            delete cuv[k];
        }
        break;
    }
    case OP_TRIM:
    {
        SmFace* p = g.faces[(size_t)rd.idx()];
        bool has_tol = rd.flag();
        SmApproxTol3d tol = rd.num();
        SmTArray<SmCurve*> in3, inuv;
        bool has3 = rd.flag();
        int64_t n3 = rd.idx();
        for (int64_t k = 0; k < n3; k++)
            in3.Add(rd.flag() ? read_curve(rd, ctx) : nullptr);
        bool has_uv = rd.flag();
        int64_t nuv = rd.idx();
        for (int64_t k = 0; k < nuv; k++)
            inuv.Add(rd.flag() ? read_curve(rd, ctx) : nullptr);
        bool want3 = rd.flag();
        bool want_uv = rd.flag();
        bool ok = true;
        if (has_uv)
            ok = surf_of(p) != nullptr;
        else if (!has3)
            ok = false;
        else
            for (ULONG k = 0; k < in3.GetSize(); k++)
                ok = ok && in3[k] != nullptr;
        if (!ok)
            o.push_back(ST_NULL_DEREF);
        else
        {
            SmTArray<SmCurve*> out3, outuv;
            SmStatus r = p->CreateCurvesByTrimming(ctx, has_tol ? &tol : NULL, has3 ? &in3 : NULL, has_uv ? &inuv : NULL,
                                                   want3 ? &out3 : NULL, want_uv ? &outuv : NULL, FALSE);
            o.push_back(st(r));
            o.push_back(want3 ? (double)out3.GetSize() : -1.0);
            for (ULONG k = 0; k < out3.GetSize(); k++)
            {
                put_curve(o, out3[k]);
                delete out3[k];
            }
            o.push_back(want_uv ? (double)outuv.GetSize() : -1.0);
            for (ULONG k = 0; k < outuv.GetSize(); k++)
            {
                put_curve(o, outuv[k]);
                delete outuv[k];
            }
        }
        for (ULONG k = 0; k < in3.GetSize(); k++)
            delete in3[k];
        for (ULONG k = 0; k < inuv.GetSize(); k++)
            delete inuv[k];
        break;
    }
    case OP_FIX_UV:
    {
        SmEdgeuse* cur = g.edgeuses[(size_t)rd.idx()];
        int64_t prev_sel = rd.idx();
        int64_t vtx_sel = rd.idx();
        double tol = rd.num();
        double dist = rd.num();
        bool face_dom = rd.flag();
        double u0 = rd.num(), v0 = rd.num(), u1 = rd.num(), v1 = rd.num();
        SmEdgeuse* prev = prev_sel < 0 ? cur->GetCWEdgeuse() : g.edgeuses[(size_t)prev_sel];
        SmVertex* v = vtx_sel >= 0                                        ? g.vertices[(size_t)vtx_sel]
                      : (cur->GetVertexuse() ? cur->GetVertexuse()->GetVertex() : nullptr);
        SmExtent2d dom(u0, v0, u1, v1);
        if (face_dom)
        {
            SmFace* f = cur->GetFace();
            if (f == nullptr)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            dom = f->*FPeek::dom();
        }
        SmVector2d size = dom.GetMax() - dom.GetMin();
        bool early = dist < SM_EFF_ZERO / 10.0 || dist > smos_Min(size.x, size.y) / 20.0;
        if (v == nullptr || (!early && !ok_fix(cur, prev)))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmBoolean fixed = UNSURE;
        o.push_back(st(sm_FixUVTrimCurves(cur, prev, *v, tol, dist, dom, fixed)));
        o.push_back((double)fixed);
        put_curve(o, uv_of(cur));
        put_curve(o, uv_of(prev));
        break;
    }
    case OP_BEST_UV:
    {
        SmEdgeuse* eu = g.edgeuses[(size_t)rd.idx()];
        int64_t n = rd.idx();
        SmTArray<SmBSplineCurve*> cands;
        SmTArray<SmOrientType> orients;
        for (int64_t k = 0; k < n; k++)
        {
            // 0 null, 1 a curve block, 2 a copy of the edge use's UV trim curve
            int64_t kind = rd.idx();
            SmBSplineCurve* c = nullptr;
            if (kind == 1)
                c = read_curve(rd, ctx);
            else if (kind == 2 && uv_of(eu) != nullptr)
            {
                SmCurve* cp = nullptr;
                if (uv_of(eu)->Copy(ctx, cp) == SM_SUCCESS)
                    c = (SmBSplineCurve*)cp;
            }
            cands.Add(c);
            orients.Add((SmOrientType)rd.idx());
        }
        SmFace* f = eu->GetFace();
        SmEdge* e = eu->GetEdge();
        bool ok = f != nullptr && surf_of(f) != nullptr && e != nullptr && e->GetPrimaryEdgeuse() != nullptr &&
                  e->GetCurve() != nullptr;
        if (!ok)
            o.push_back(ST_NULL_DEREF);
        else
        {
            SmBSplineCurve* found = nullptr;
            SmTArray<SmBSplineCurve*> work(cands);
            o.push_back(st(sc_face_p2::sm_FindBestUVCurve(surf_of(f), eu, work, orients, found)));
            int64_t which = -1;
            for (ULONG k = 0; k < cands.GetSize(); k++)
                if (found != nullptr && cands[k] == found)
                    which = (int64_t)k;
            o.push_back((double)which);
            put_curve(o, found);
            for (ULONG k = 0; k < work.GetSize(); k++)
                o.push_back(work[k] ? 1.0 : 0.0);
        }
        for (ULONG k = 0; k < cands.GetSize(); k++)
            delete cands[k];
        break;
    }
    case OP_CONNECTED:
    {
        SmEdgeuse* a = g.edgeuses[(size_t)rd.idx()];
        SmEdgeuse* c = g.edgeuses[(size_t)rd.idx()];
        SmBoolean sing = rd.bflag();
        SmBoolean seam = rd.bflag();
        if (!ok_connected(a, c))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmBoolean on_sing = UNSURE, crosses = UNSURE;
        o.push_back(flg(sc_face_p2::sm_AreConnected(a, c, sing, seam, &on_sing, &crosses)));
        o.push_back((double)on_sing);
        o.push_back((double)crosses);
        break;
    }
    case OP_RESCUE:
    {
        SmFace* p = g.faces[(size_t)rd.idx()];
        SmSurface* s = surf_of(p);
        if (s == nullptr)
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        bool to_null = rd.flag();
        o.push_back(s->GetOwner() == (SmObject*)p ? 1.0 : 0.0);
        {
            sc_face_p2::SmRescueSurfaceOwner r(s);
            s->SetOwner(to_null ? nullptr : (SmObject*)b.brep);
            o.push_back(s->GetOwner() == nullptr ? 1.0 : 0.0);
        }
        o.push_back(s->GetOwner() == (SmObject*)p ? 1.0 : 0.0);
        break;
    }
    case OP_REMOVE_SUSPECT:
    {
        int64_t n = rd.idx();
        SmCurveClassification cc;
        cc.SetSize((ULONG)n);
        for (int64_t k = 0; k < n; k++)
        {
            int64_t a = rd.idx();
            int64_t e = rd.idx();
            if (a >= 0)
                cc[(ULONG)k].m_vStart.SetClassObject(SM_PC_VERTEX, g.vertices[(size_t)a]);
            if (e >= 0)
                cc[(ULONG)k].m_vEnd.SetClassObject(SM_PC_VERTEX, g.vertices[(size_t)e]);
        }
        sm_RemoveSuspectSplitAtSeamUVTrimCurves(&cc);
        for (SmEdgeuse* p : g.edgeuses)
            o.push_back(p->*EuPeek::uv() ? 1.0 : 0.0);
        break;
    }
    default:
        break;
    }
}

// A layout's indices: `num area volume moments*8`.
void put_layout(std::vector<double>& o, const sc_face_p2::SmPrecisePropertyLayout& l)
{
    o.push_back((double)l.m_lNumComponents);
    o.push_back((double)l.m_lArea);
    o.push_back((double)l.m_lVolume);
    for (ULONG k = 0; k < 8; k++)
        o.push_back((double)l.m_aMoments[k]);
}

void run_geom_op(int32_t op, Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    switch (op)
    {
    case OP_SEAM_CROSS:
    {
        SmPoint2d uv1 = rd.uv();
        SmPoint2d uv2 = rd.uv();
        double u0 = rd.num(), v0 = rd.num(), u1 = rd.num(), v1 = rd.num();
        SmBoolean cu = rd.bflag();
        SmBoolean cv = rd.bflag();
        SmVector2d shift = rd.uv();
        SmBoolean r = sc_face_p2::sm_CheckSeamCrossing(uv1, uv2, NULL, SmExtent2d(u0, v0, u1, v1), cu, cv, shift);
        o.push_back(flg(r));
        o.push_back(uv1.x);
        o.push_back(uv1.y);
        o.push_back(shift.x);
        o.push_back(shift.y);
        break;
    }
    case OP_LAYOUT:
    {
        sc_face_p2::SmPrecisePropertyLayout l((ULONG)rd.idx());
        put_layout(o, l);
        break;
    }
    case OP_ODE_ISO:
    {
        SmSurface* s = read_surface(rd, ctx);
        double u = rd.num();
        double t = rd.num();
        SmPoint3d origin = rd.vec();
        double thick = rd.num();
        sc_face_p2::SmPrecisePropertyLayout l((ULONG)rd.idx());
        int64_t n = rd.idx();
        SmTArray<double> y, d;
        for (int64_t k = 0; k < n; k++)
        {
            d.Add(7.0);
            y.Add(0.0);
        }
        sc_face_p2::SmODEIsoCurveLengthIFEO e(*s, u, origin, thick, l);
        o.push_back(st(e.Evaluate(t, y, d)));
        for (ULONG k = 0; k < d.GetSize(); k++)
            o.push_back(d[k]);
        delete s;
        break;
    }
    case OP_ODE_AREA:
    {
        SmSurface* s = read_surface(rd, ctx);
        SmBSplineCurve* c = read_curve(rd, ctx);
        double t = rd.num();
        double min_v = rd.num();
        double acc = rd.num();
        int64_t ny = rd.idx();
        SmTArray<double> yscale;
        for (int64_t k = 0; k < ny; k++)
            yscale.Add(rd.num());
        SmPoint3d origin = rd.vec();
        double thick = rd.num();
        sc_face_p2::SmPrecisePropertyLayout l((ULONG)rd.idx());
        int64_t n = rd.idx();
        SmTArray<double> y, d;
        for (int64_t k = 0; k < n; k++)
        {
            d.Add(rd.num());
            y.Add(0.0);
        }
        sc_face_p2::SmODEAreaIFEO e(*s, *c, min_v, acc, &yscale, origin, thick, l);
        o.push_back(st(e.Evaluate(t, y, d)));
        o.push_back((double)d.GetSize());
        for (ULONG k = 0; k < d.GetSize(); k++)
            o.push_back(d[k]);
        delete c;
        delete s;
        break;
    }
    default:
        break;
    }
}
} // namespace

SC_EXPORT int32_t sc_topology_face_p2_eval(int32_t op, const char* path, const double* in, int64_t n_in,
                                           double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    if (op < OP_SEAM_CROSS)
        run_brep_op(op, rd, path, o);
    else
        run_geom_op(op, rd, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
