// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `heal.gap`: the SmGap records, SmGapArray, SmGapSample and the
// gap functions (source/SMLib/src/SmGap.cpp, source/SMLib/inc/SmGap.h).
//
// One entry point, sc_heal_gap_eval(op, path, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/heal_gap.rs, which validates every input.
// The geometry blocks build real SmBSplineCurve, SmHermiteCurve, SmLine, SmEllipse,
// SmCircle and SmCrvOnSurf curves and SmBSplineSurface, SmPlane and SmCurveBoundedSurface
// surfaces. The topology op builds its SmBrep, walks and exports it exactly as the
// `topology.vertexuse` shim does (that code is copied below, unchanged), so the Rust side
// can import the graph with that runner's importer; ops whose upstream path would
// dereference null output -2 instead of calling it.
#include "sc_shim.h"
#include "support/topology_fixture.h"


#include "StdAfx.h"
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
#include <SmCircle.h>
#include <SmCrvOnSurf.h>
#include <SmCurveBoundedSurface.h>
#include <SmEllipse.h>
#include <SmGap.h>
#include <SmHermiteCurve.h>
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

#include "sc_sm.h"

using sc::put_vec;

// SmGap.cpp's (non-static, undeclared) BuildSampleSet helper.
ULONG sm_AddUniqueGetOrderedIndex(SmTArray<double>& rArray, double sNewElement);


namespace
{
// ---- copied from harness/cpp/shim/topology_vertexuse.cpp --------------------------------

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
    const SmBSplineSurface* b = (const SmBSplineSurface*)s;
    gw_SURFACE* sur = b->GetGwNurbPointer();
    o.push_back(0.0);
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
        put_surface(o, p->GetSurface());
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


// ---- end of the copy ----------------------------------------------------------------------

// ---- heal.gap ops -------------------------------------------------------------------------

enum : int32_t
{
    OP_GF = 0,
    OP_EVAL = 1,
    OP_ADD_UNIQUE = 2,
    OP_SAMPLE = 3,
    OP_GAP_ARRAY = 4,
    OP_TOPO = 5,
};

gw_CURVE* gf_nurb_curve(Rd& rd)
{
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
    return cur;
}

// `oob form n m p q r s cpts(x y z w) u v`.
SmBSplineSurface* gf_bspline_surface(Rd& rd, const SmContext& ctx)
{
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

// A surface block: `0 bspline`, `1 O X Y sx sy u0 v0 u1 v1` (SmPlane), `2 bspline`
// (SmCurveBoundedSurface over it, implicit outer, no boundary curves).
SmSurface* gf_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 1)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double sx = rd.num();
        double sy = rd.num();
        double a = rd.num(), b = rd.num(), c = rd.num(), d = rd.num();
        return new (ctx) SmPlane(o, x, y, SmVector2d(sx, sy), SmExtent2d(a, b, c, d), &ctx);
    }
    SmBSplineSurface* b = gf_bspline_surface(rd, ctx);
    if (kind == 0)
        return b;
    SmTArray<SmCurve*> c3d(ctx), cuv(ctx);
    return new (ctx) SmCurveBoundedSurface(*b, &c3d, &cuv, TRUE);
}

// A curve block (see the FFI module): `kind dim ...`.
SmCurve* gf_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    ULONG dim = (ULONG)rd.idx();
    switch (kind)
    {
    case 0:
    {
        bool oob = rd.flag();
        SmBSplineCurve* c = new (ctx) SmBSplineCurve(gf_nurb_curve(rd), dim, FALSE);
        c->SetOutOfBoundsEnabled(oob ? TRUE : FALSE);
        return c;
    }
    case 1:
    {
        SmVector3d p1 = rd.vec();
        SmVector3d d1 = rd.vec();
        SmVector3d p2 = rd.vec();
        SmVector3d d2 = rd.vec();
        return new (ctx) SmHermiteCurve(p1, d1, p2, d2, dim);
    }
    case 2:
    {
        SmPoint3d p = rd.vec();
        SmVector3d v = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double scale = rd.num();
        bool io = rd.flag();
        return new (ctx) SmLine(p, v, SmExtent1d(d0, d1), scale, 3, &ctx, NULL, io ? TRUE : FALSE);
    }
    case 3:
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double rx = rd.num();
        double ry = rd.num();
        bool io = rd.flag();
        return new (ctx) SmEllipse(c, x, y, SmExtent1d(d0, d1), rx, ry, dim, &ctx, NULL, io ? TRUE : FALSE);
    }
    case 4:
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double r = rd.num();
        bool io = rd.flag();
        return new (ctx) SmCircle(c, x, y, SmExtent1d(d0, d1), r, dim, &ctx, NULL, io ? TRUE : FALSE);
    }
    default:
    {
        // SmCrvOnSurf over a 2D B-spline curve and a B-spline surface (owning both)
        SmBSplineCurve* uv = new (ctx) SmBSplineCurve(gf_nurb_curve(rd), 2, FALSE);
        SmBSplineSurface* s = gf_bspline_surface(rd, ctx);
        return new (ctx) SmCrvOnSurf(*uv, *s, NULL, 3, &ctx);
    }
    }
}

// `drop this(3) other(3) tol(3) this_param(2) other_param(2) xtol in_tol`.
void put_sample(std::vector<double>& o, SmGapSample& s)
{
    o.push_back((double)s.GetGapDropType());
    put_vec(o, s.GetThisPos());
    put_vec(o, s.GetOtherPos());
    put_vec(o, s.GetTolPoint());
    o.push_back(s.GetThisParam().x);
    o.push_back(s.GetThisParam().y);
    o.push_back(s.GetOtherParam().x);
    o.push_back(s.GetOtherParam().y);
    o.push_back((double)s.GetXSectTol3d());
    o.push_back(flg(s.IsInTol()));
}

// The protected state of a gap function.
struct GfPeek : SmGapFunction
{
    static SmBoolean SmGapFunction::*set() { return &GfPeek::m_bAreSamplesSet; }
    static ULONG SmGapFunction::*max() { return &GfPeek::m_lMaxSampleIndex; }
    static ULONG SmGapFunction::*min() { return &GfPeek::m_lMinSampleIndex; }
    static SmBoolean SmGapFunction::*normal() { return &GfPeek::m_bAllNormalGaps; }
    static SmTArray<SmGapSample> SmGapFunction::*samples() { return &GfPeek::m_sSampleGaps; }
};

// `cnt_u cnt_v state max min all_normal oivl_u(2) oivl_v(2) n samples*n`.
void put_gf(std::vector<double>& o, SmGapFunction& gf)
{
    o.push_back((double)gf.GetSampleCntU());
    o.push_back((double)gf.GetSampleCntV());
    SmBoolean set = gf.*GfPeek::set();
    o.push_back(set == TRUE ? 1.0 : set == UNSURE ? 2.0 : 0.0);
    o.push_back((double)(gf.*GfPeek::max()));
    o.push_back((double)(gf.*GfPeek::min()));
    SmBoolean nrm = gf.*GfPeek::normal();
    o.push_back(nrm == TRUE ? 1.0 : nrm == UNSURE ? 2.0 : 0.0);
    o.push_back(gf.GetOtherIvlU()->GetMin());
    o.push_back(gf.GetOtherIvlU()->GetMax());
    o.push_back(gf.GetOtherIvlV()->GetMin());
    o.push_back(gf.GetOtherIvlV()->GetMax());
    SmTArray<SmGapSample>& s = gf.*GfPeek::samples();
    o.push_back((double)s.GetSize());
    for (ULONG i = 0; i < s.GetSize(); i++)
        put_sample(o, s[i]);
}

// The geometry of a gap function and the function: `kind tol this ivl other cnt_u cnt_v`.
struct Gf
{
    SmCurve* this_curve = nullptr;
    SmSurface* this_surface = nullptr;
    SmCurve* other_curve = nullptr;
    SmSurface* other_surface = nullptr;
    SmGapFunction* gf = nullptr;
    ~Gf()
    {
        delete gf;
        delete this_curve;
        delete this_surface;
        delete other_curve;
        delete other_surface;
    }
};

void read_gf(Rd& rd, const SmContext& ctx, Gf& f)
{
    int64_t kind = rd.idx();
    SmXSectTol3d tol(rd.num());
    SmExtent1d ivl;
    SmExtent2d dom;
    if (kind < 3)
    {
        f.this_curve = gf_curve(rd, ctx);
        double a = rd.num(), b = rd.num();
        ivl = SmExtent1d(a, b);
    }
    else
    {
        f.this_surface = gf_surface(rd, ctx);
        double a = rd.num(), b = rd.num(), c = rd.num(), d = rd.num();
        dom = SmExtent2d(a, b, c, d);
    }
    SmPoint3d pt;
    if (kind == 0 || kind == 3)
        pt = rd.vec();
    else if (kind == 1 || kind == 4)
        f.other_curve = gf_curve(rd, ctx);
    else
        f.other_surface = gf_surface(rd, ctx);
    ULONG cu = (ULONG)rd.idx();
    ULONG cv = (ULONG)rd.idx();
    switch (kind)
    {
    case 0: f.gf = new SmCrvPtGapFunction(tol, f.this_curve, ivl, &pt, cu); break;
    case 1: f.gf = new SmCrvCrvGapFunction(tol, f.this_curve, ivl, f.other_curve, cu); break;
    case 2: f.gf = new SmCrvSrfGapFunction(tol, f.this_curve, ivl, f.other_surface, cu); break;
    case 3: f.gf = new SmSrfPtGapFunction(tol, f.this_surface, dom, &pt, cu); break;
    case 4: f.gf = new SmSrfCrvGapFunction(tol, f.this_surface, dom, f.other_curve, cu); break;
    default: f.gf = new SmSrfSrfGapFunction(tol, f.this_surface, dom, f.other_surface, cu, cv); break;
    }
}

// A point whose coordinates may be SM_UNDEF_DOUBLE or NL_NOZ (no assertion).
SmPoint3d raw_point(Rd& rd)
{
    SmPoint3d p;
    p.x = rd.num();
    p.y = rd.num();
    p.z = rd.num();
    return p;
}

// ---- topology gaps --------------------------------------------------------------------

// GetMate needs the edge's primary edge use (loop edge uses).
bool mate_ok(SmEdgeuse* e)
{
    return e->GetEdgeuseType() == SmShell_TYPE || (e->GetEdge() && e->GetEdge()->GetPrimaryEdgeuse());
}

// GetUVTrimCurve: the own curve, else the mate's (GetMate dereferences).
bool uv_curve_ok(SmEdgeuse* e) { return (e->*EuPeek::uv()) != nullptr || mate_ok(e); }

// SmEdgeuse::GetFace dereferences the loop use's face use.
bool eu_face_ok(SmEdgeuse* e)
{
    if (e->GetEdgeuseType() != SmLoopuse_TYPE)
        return true;
    SmTopology* lu = e->*EuPeek::sorlu();
    return lu == nullptr || ((SmLoopuse*)lu)->GetFaceuse() != nullptr;
}

// SmVertexuse::GetFaceuse dereferences a loop vertex use's loop use.
bool vu_faceuse_ok(SmVertexuse* p) { return !p->IsLoopVertexuse() || (p->*VuPeek::owner()) != nullptr; }

bool edge_curve_ok(SmEdge* e) { return e != nullptr && e->GetCurve() != nullptr; }

// Whether GetGapEndPoints (and the accessors it calls) would dereference null.
bool end_points_crash(int64_t kind, SmVertexuse* vu, SmEdgeuse* eu0, SmEdgeuse* eu1, double t0, double t1,
                      bool uv_init)
{
    SmEdgeuse* veu = (vu && vu->IsEdgeVertexuse()) ? vu->GetEdgeuse() : nullptr;
    switch (kind)
    {
    case 0: // vertex/edge: GetEdge(0) = vu->GetEdgeuse()->GetEdge()
        if (vu && veu == nullptr)
            return true;
        if (t0 != SM_UNDEF_DOUBLE && !edge_curve_ok(vu ? veu->GetEdge() : nullptr))
            return true;
        return false;
    case 1: // vertex/face trim curve
        if (vu && !vu_faceuse_ok(vu))
            return true;
        if (veu && !uv_curve_ok(veu))
            return true;
        if (t0 != SM_UNDEF_DOUBLE && veu == nullptr)
            return true;
        return false;
    case 2: // vertex/face
    {
        if (vu && !vu_faceuse_ok(vu))
            return true;
        if (!uv_init)
            return false;
        SmFaceuse* fu = vu ? vu->GetFaceuse() : nullptr;
        SmFace* f = fu ? fu->GetFace() : nullptr;
        return f == nullptr || f->GetSurface() == nullptr;
    }
    case 3: // edge/edge
        if (t0 != SM_UNDEF_DOUBLE && !edge_curve_ok(eu0 ? eu0->GetEdge() : nullptr))
            return true;
        if (t1 != SM_UNDEF_DOUBLE && !edge_curve_ok(eu1 ? eu1->GetEdge() : nullptr))
            return true;
        return false;
    case 4: // edge/face trim curve (t1 = UVTrimCurveT)
        if (eu0 && !eu_face_ok(eu0))
            return true;
        if (eu0 && !uv_curve_ok(eu0))
            return true;
        if (t0 != SM_UNDEF_DOUBLE && !edge_curve_ok(eu0 ? eu0->GetEdge() : nullptr))
            return true;
        if (t1 != SM_UNDEF_DOUBLE && eu0 == nullptr)
            return true;
        return false;
    default: // edge/face
    {
        if (eu0 && !eu_face_ok(eu0))
            return true;
        if (t0 != SM_UNDEF_DOUBLE && !edge_curve_ok(eu0 ? eu0->GetEdge() : nullptr))
            return true;
        if (!uv_init)
            return false;
        SmFace* f = eu0 ? eu0->GetFace() : nullptr;
        return f == nullptr || f->GetSurface() == nullptr;
    }
    }
}

struct FacePeek : SmFace
{
    static SmSurface* SmFace::*surface() { return &FacePeek::m_pSurface; }
};
struct EdgePeek : SmEdge
{
    static SmCurve* SmEdge::*curve() { return &EdgePeek::m_pCurve; }
};

// A plain SmBSplineCurve of `c`'s NURB, or nullptr when `c` already is one (or a line)
// or has no NURB.
SmBSplineCurve* plain_curve(SmCurve* c)
{
    if (c == nullptr || !c->IsKindOf(SmBSplineCurve_TYPE) || c->GetType() == SmBSplineCurve_TYPE ||
        c->GetType() == SmLine_TYPE)
        return nullptr;
    SmBSplineCurve* b = (SmBSplineCurve*)c;
    gw_CURVE* nurb = b->GetGwNurbPointer();
    if (nurb == nullptr)
        return nullptr;
    return new (the_context()) SmBSplineCurve(sm_AllocateAndCopyNurbCurve(nurb), b->GetDim(), FALSE);
}

// The Rust side imports the export's NURBs as plain B-splines (and lines), and SmPlanes
// as planes: give every other face and edge of the C++ B-rep the same geometry (the
// analytic originals are left unowned and leaked).
void normalise(Graph& g)
{
    for (SmFace* f : g.faces)
    {
        SmSurface* s = f->*FacePeek::surface();
        if (s == nullptr || !s->IsKindOf(SmBSplineSurface_TYPE) || s->GetType() == SmBSplineSurface_TYPE ||
            s->GetType() == SmPlane_TYPE)
            continue;
        SmBSplineSurface* b = (SmBSplineSurface*)s;
        gw_SURFACE* nurb = b->GetGwNurbPointer();
        if (nurb == nullptr)
            continue;
        SmBSplineSurface* nb =
            new (the_context()) SmBSplineSurface(sm_AllocateAndCopyNurbSurface(nurb), FALSE, NULL);
        nb->GetOutOfBoundsEnabled() = b->GetOutOfBoundsEnabled();
        nb->SetBSplineSurfaceForm(b->GetBSplineSurfaceForm());
        f->*FacePeek::surface() = nb;
    }
    for (SmEdge* e : g.edges)
    {
        SmBSplineCurve* nc = plain_curve(e->*EdgePeek::curve());
        if (nc != nullptr)
            e->*EdgePeek::curve() = nc;
    }
}

template <class T> T* pick(std::vector<T*>& v, int64_t i)
{
    return (i >= 0 && i < (int64_t)v.size()) ? v[(size_t)i] : nullptr;
}

// `kind sel0 sel1 t0 t1 u v has_uv` after the B-rep block.
void run_topo(Rd& rd, const char* path, std::vector<double>& o)
{
    SmStatus status = SM_SUCCESS;
    SmBrep* brep = build(rd, path, status);
    int64_t kind = rd.idx();
    int64_t sel0 = rd.idx();
    int64_t sel1 = rd.idx();
    double t0 = rd.num();
    double t1 = rd.num();
    SmPoint2d uv = rd.uv();
    bool has_uv = rd.flag();
    if (brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(status));
        return;
    }
    Graph g;
    walk(brep, g);
    normalise(g);
    SmVertexuse* vu = (kind <= 2) ? pick(g.vertexuses, sel0) : nullptr;
    SmEdgeuse* eu0 = (kind >= 3) ? pick(g.edgeuses, sel0) : nullptr;
    SmEdgeuse* eu1 = (kind == 3) ? pick(g.edgeuses, sel1) : nullptr;
    // the parameters as the gap sees them
    double et0 = SM_UNDEF_DOUBLE, et1 = SM_UNDEF_DOUBLE;
    bool uv_init = false;
    switch (kind)
    {
    case 0: et0 = t0; break;
    case 1: et0 = t0; break; // UVTrimCurveT, checked through t0 in end_points_crash
    // (the record keeps the UV point only when Set accepts it)
    case 2: uv_init = vu && has_uv && uv.IsInitialized(); break;
    case 3: et0 = t0; et1 = t1; break;
    case 4: et0 = t0; et1 = t1; break;
    default: et0 = t0; uv_init = eu0 && t0 != SM_UNDEF_DOUBLE && has_uv && uv.IsInitialized(); break;
    }
    if (end_points_crash(kind, vu, eu0, eu1, et0, et1, uv_init))
    {
        o.push_back(ST_NULL_DEREF);
        delete brep;
        return;
    }
    SmGap* gap = nullptr;
    switch (kind)
    {
    case 0: gap = new SmVertexEdgeGap(vu, t0); break;
    case 1: gap = new SmVertexFaceTrimCurveGap(vu, t0); break;
    case 2: gap = new SmVertexFaceGap(vu, has_uv ? &uv : NULL); break;
    case 3: gap = new SmEdgeEdgeGap(eu0, eu1, t0, t1); break;
    case 4: gap = new SmEdgeFaceTrimCurveGap(eu0, t0, t1); break;
    default: gap = new SmEdgeFaceGap(eu0, t0, has_uv ? &uv : NULL); break;
    }
    o.push_back(0.0);
    o.push_back(gap->GetLength());
    o.push_back(flg(gap->IsInit()));
    o.push_back(gap->GetEdgeT(0));
    o.push_back(gap->GetEdgeT(1));
    o.push_back(gap->GetUVTrimCurveT());
    SmPoint2d fuv = gap->GetFaceUV();
    o.push_back(fuv.x);
    o.push_back(fuv.y);
    o.push_back((double)g.find(9, gap->GetVertex()));
    o.push_back((double)g.find(8, gap->GetVertexuse()));
    o.push_back((double)g.find(7, gap->GetEdge(0)));
    o.push_back((double)g.find(7, gap->GetEdge(1)));
    o.push_back((double)g.find(6, gap->GetEdgeuse(0)));
    o.push_back((double)g.find(6, gap->GetEdgeuse(1)));
    o.push_back((double)g.find(3, gap->GetFace()));
    o.push_back((double)gap->GetType());
    SmPoint3d p1, p2;
    SmStatus s = gap->GetGapEndPoints(p1, p2);
    o.push_back(st(s));
    put_vec(o, p1);
    put_vec(o, p2);
    // CacheGap on a fresh copy of the record
    SmGap* c = gap->MakeCopy();
    c->m_dGap3d = 0.0;
    c->CacheGap();
    o.push_back(c->GetLength());
    delete c;
    delete gap;
    delete brep;
}

void run(int32_t op, Rd& rd, const char* path, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    switch (op)
    {
    case OP_GF:
    {
        Gf f;
        read_gf(rd, ctx, f);
        bool set_cnt = rd.flag();
        ULONG su = (ULONG)rd.idx();
        ULONG sv = (ULONG)rd.idx();
        bool set_tol = rd.flag();
        double tol = rd.num();
        bool stop = rd.flag();
        int64_t mode = rd.idx();
        if (set_cnt)
            f.gf->SetSampleCnt(su, sv);
        if (set_tol)
            f.gf->SetXSectTol3d(SmXSectTol3d(tol));
        if (mode == 0)
        {
            SmGapSample* p = NULL;
            SmStatus s = f.gf->GetMaxGapSample(p, stop ? TRUE : FALSE);
            o.push_back(st(s));
        }
        else
        {
            f.gf->GetSampleSet(stop ? TRUE : FALSE);
            o.push_back(0.0);
        }
        put_gf(o, *f.gf);
        break;
    }
    case OP_EVAL:
    {
        Gf f;
        read_gf(rd, ctx, f);
        double u = rd.num();
        double v = rd.num();
        bool hu = rd.flag();
        double gu = rd.num();
        bool hv = rd.flag();
        double gv = rd.num();
        SmGapSample s;
        SmStatus r = f.gf->Evaluate(u, v, s, hu ? &gu : NULL, hv ? &gv : NULL);
        o.push_back(st(r));
        put_sample(o, s);
        break;
    }
    case OP_ADD_UNIQUE:
    {
        int64_t n = rd.idx();
        SmTArray<double> a;
        for (int64_t i = 0; i < n; i++)
            a.Add(rd.num());
        double v = rd.num();
        ULONG idx = sm_AddUniqueGetOrderedIndex(a, v);
        o.push_back((double)idx);
        o.push_back((double)a.GetSize());
        for (ULONG i = 0; i < a.GetSize(); i++)
            o.push_back(a[i]);
        break;
    }
    case OP_SAMPLE:
    {
        SmPoint3d tp = raw_point(rd);
        double tu = rd.num(), tv = rd.num();
        SmPoint3d op = raw_point(rd);
        double ou = rd.num(), ov = rd.num();
        SmGapDropType dt = (SmGapDropType)rd.idx();
        SmXSectTol3d tol(rd.num());
        SmGapSample s;
        s.SetCanonical(tp, tu, tv, op, ou, ov, dt, tol);
        put_sample(o, s);
        o.push_back(s.GetLength());
        o.push_back(s.GetThisParam(0));
        o.push_back(s.GetThisParam(1));
        o.push_back(s.GetOtherParam(0));
        o.push_back(s.GetOtherParam(1));
        break;
    }
    case OP_GAP_ARRAY:
    {
        SmGapArray a, b;
        std::vector<SmVertexEdgeGap*> keep;
        for (int k = 0; k < 2; k++)
        {
            int64_t n = rd.idx();
            for (int64_t i = 0; i < n; i++)
            {
                SmVertexEdgeGap* g = new SmVertexEdgeGap();
                g->SmGap::operator=(rd.num());
                keep.push_back(g);
                (k == 0 ? a : b).Add(g);
            }
        }
        b.Append(a);
        for (SmGapArray* x : {&a, &b})
        {
            o.push_back((double)x->GetSize());
            const SmGap* m = x->GetMaxGap();
            ULONG mi = SM_BIG_ULONG;
            for (ULONG i = 0; i < x->GetSize(); i++)
                if ((*x)[i] == m)
                {
                    mi = i;
                    break;
                }
            o.push_back(m ? (double)mi : -1.0);
            o.push_back(x->GetMaxGapLength());
        }
        for (ULONG i = 0; i < b.GetSize(); i++)
        {
            o.push_back(b[i]->GetLength());
            if (i + 1 < b.GetSize())
            {
                o.push_back(flg(*b[i] > *b[i + 1]));
                o.push_back(flg(*b[i] < *b[i + 1]));
            }
        }
        a.ReSet();
        b.ReSet();
        o.push_back((double)b.GetSize());
        for (SmVertexEdgeGap* g : keep)
            delete g;
        break;
    }
    case OP_TOPO: run_topo(rd, path, o); break;
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_heal_gap_eval(int32_t op, const char* path, const double* in, int64_t n_in, double* out,
                                   int64_t cap, int64_t* out_len)
{
    if (in == nullptr || n_in < 0 || out_len == nullptr || path == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    run(op, rd, path, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
