// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.vertex`: the SmVertex members owned by the unit
// (source/SMLib/src/SmVertex.cpp, source/SMLib/inc/SmVertex.h) on real SmBrep objects.
//
// One entry point, sc_topology_vertex_eval(op, path, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/topology_vertex.rs, which
// validates every input. The B-rep construction, traversal and export are those of the
// `topology.vertexuse` shim (harness/cpp/shim/topology_vertexuse.cpp), with each vertex's
// vertex use list and poly vertex state, each vertex use's cached gap values
// and each edge's curve identity added to the export. Every B-rep op starts with a B-rep
// block:
//   `0 rebuild validate_only init_gaps`         SmBrep::ReadFromFile(path) (ASCII .smb)
//   `1 curve t0 t1 validate_only init_gaps`     CreateWireEdgeFromCurve(curve, [t0, t1])
//   `2 surface u0 v0 u1 v1 validate_only init_gaps` CreateFaceFromSurface(surface, dom)
//   `3 validate_only init_gaps`                 the wire network of prog_test my_test_topology
//   `4 closed n segments validate_only init_gaps` a chain of wire edges (build_chain)
// `init_gaps` computes the cached vertex/face, vertex/edge, edge/face and edge/CCW-edge
// gaps (C++ only) before the op, so that cached gaps and their resets are observable.
//
// The B-rep graph is exported in traversal order (each pointer mapped to its first-seen
// index per kind; -1 is null), reading raw members (protected ones through
// pointer-to-member access), never the functions under test. Ops whose upstream path
// would dereference null output -2 instead of calling it.
#include "sc_shim.h"
#include "support/topology_fixture.h"
#include "sc_analytic_export.h"


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
    OP_QUERY = 1,
    OP_CONNECTED = 2,
    OP_VU_OF_FACE = 3,
    OP_VUS_OF_FACE = 4,
    OP_VUS_OF_EDGE = 5,
    OP_COMMON_EDGES = 6,
    OP_SHELL_IN_REGION = 7,
    OP_TANGENT = 8,
    OP_TOPOLOGICAL = 9,
    OP_ON_POLE = 10,
    OP_GAPS = 11,
    OP_NOTIFY = 12,
    OP_SET_POINT = 13,
    OP_SET_TOLERANCE = 14,
    OP_REFRESH_TOLERANCE = 15,
    OP_EXACT = 16,
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
struct VPeek : SmVertex
{
    static SmPolyVertex* SmVertex::*poly() { return &VPeek::m_pPolyVertex3D; }
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

// A chain of wire edges, each segment's start joined to the previous segment's end
// vertex (the last one closing back to the first vertex when `closed`), as
// prog_test my_test_edge_geometry_repairs (smTopoTest.cpp) builds one: MakeShellVertex, MakeWireEdgeVertex,
// MakeWireEdge. Block: `closed n` then per segment `0 a[3] b[3] smline has_ivl t0 t1 rev`
// (SmLine::CreateLineSegment, re-parameterised to [t0, t1] when has_ivl, or
// SmBSplineCurve::CreateLineSegment) or `1 curve rev` (an SmBSplineCurve); `rev` makes
// the edge run against the curve (SM_OT_OPPOSITE).
SmStatus build_chain(Rd& rd, const SmContext& ctx, SmBrep* pBrep)
{
    bool closed = rd.flag();
    int64_t n = rd.idx();
    SmVertex* first = nullptr;
    SmVertex* prev = nullptr;
    SmShell* pShell = nullptr;
    for (int64_t i = 0; i < n; i++)
    {
        SmCurve* c = nullptr;
        if (rd.idx() == 0)
        {
            SmPoint3d a = rd.vec();
            SmPoint3d b = rd.vec();
            bool smline = rd.flag();
            bool has_ivl = rd.flag();
            double t0 = rd.num(), t1 = rd.num();
            if (smline)
            {
                SmLine* l = nullptr;
                SmExtent1d ivl(t0, t1);
                SER(SmLine::CreateLineSegment(ctx, 3, a, b, l, has_ivl ? &ivl : NULL));
                c = l;
            }
            else
            {
                SmBSplineCurve* l = nullptr;
                SER(SmBSplineCurve::CreateLineSegment(ctx, 3, a, b, l));
                c = l;
            }
        }
        else
            c = read_curve(rd, ctx);
        bool rev = rd.flag();
        SmExtent1d ivl = c->GetNaturalInterval();
        SmPoint3d p0, p1;
        SER(c->EvaluatePoint(ivl.GetMin(), p0));
        SER(c->EvaluatePoint(ivl.GetMax(), p1));
        SmOrientType o = rev ? SM_OT_OPPOSITE : SM_OT_SAME;
        if (prev == nullptr)
        {
            SER(pBrep->MakeShellVertex(pBrep->GetInfiniteRegion(), rev ? p1 : p0, pShell, first));
            prev = first;
        }
        SmEdge* pEdge = nullptr;
        if (closed && i == n - 1)
        {
            SER(pBrep->MakeWireEdge(pBrep->GetInfiniteRegion(), prev, first, c, ivl, o, pEdge));
        }
        else
        {
            SmVertex* next = nullptr;
            SER(pBrep->MakeWireEdgeVertex(pBrep->GetInfiniteRegion(), prev, c, ivl, o, rev ? p0 : p1, pEdge, next));
            prev = next;
        }
    }
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

// Walks everything reachable from the B-rep's regions (and the vertices' use lists),
// registering in first-seen order.
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
    // An SmPlane derives from SmBSplineSurface: exported as the whole plane (frame, UV
    // scale and analytic domain) like sc_edgeuse_graph.h, not as its B-spline part, so the
    // Rust side sees the kind the C++ code branches on (`3 O X Y su sv umin vmin umax vmax`)
    if (s != nullptr && s->GetType() == SmPlane_TYPE)
    {
        const SmPlane* pl = (const SmPlane*)s;
        o.push_back(3.0);
        put_vec(o, pl->GetPosition().GetOrigin());
        put_vec(o, pl->GetPosition().GetXAxisRef());
        put_vec(o, pl->GetPosition().GetYAxisRef());
        SmVector2d sc = pl->GetUVScale();
        SmExtent2d dom = pl->GetSTEPUVDomain();
        o.push_back(sc.x);
        o.push_back(sc.y);
        o.push_back(dom.GetUMin());
        o.push_back(dom.GetVMin());
        o.push_back(dom.GetUMax());
        o.push_back(dom.GetVMax());
        return;
    }
    if (s == nullptr || !s->IsKindOf(SmBSplineSurface_TYPE))
    {
        o.push_back(s == nullptr ? -1.0 : -2.0);
        return;
    }
    const SmBSplineSurface* b = (const SmBSplineSurface*)s;
    gw_SURFACE* sur = b->GetGwNurbPointer();
    // `4 type` marks a subclass (cone, sphere, torus, revolution, extrusion, ...) exported as
    // its B-spline part only; the block follows as for kind 0
    if (s->GetType() != SmBSplineSurface_TYPE)
    {
        o.push_back(4.0);
        o.push_back((double)s->GetType());
    }
    else
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
    {
        // an analytic face surface whose record is lossless travels exactly first
        // (sc_analytic_export.h: `2 type version surf_oob gen_oob nbytes bytes`), then
        // its regular block as for any face
        sc_analytic_export::put_exact_head(o, p->GetSurface(), the_context());
        put_surface(o, p->GetSurface());
        // the face's UV domain and tolerance (the vertex/face gaps solve against them)
        SmExtent2d d = p->GetUVDomain();
        o.push_back(d.GetMin().x);
        o.push_back(d.GetMin().y);
        o.push_back(d.GetMax().x);
        o.push_back(d.GetMax().y);
        o.push_back((double)p->GetTolerance());
        o.push_back((double)g.find(2, p->GetUpwardFaceuse()));
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
    for (size_t ei = 0; ei < g.edges.size(); ei++)
    {
        SmEdge* p = g.edges[ei];
        // the curve's identity: the first edge holding the same curve object (-1: none)
        double cid = -1.0;
        if (p->GetCurve() != nullptr)
            for (size_t ej = 0; ej <= ei; ej++)
                if (g.edges[ej]->GetCurve() == p->GetCurve())
                {
                    cid = (double)ej;
                    break;
                }
        o.push_back(cid);
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
        const SmVertexEdgeGap& eg = p->*VuPeek::edge_gap();
        o.push_back((double)g.find(8, eg.GetVertexuse()));
        o.push_back(eg.GetLength());
        o.push_back(eg.GetEdgeT());
        const SmVertexFaceGap& fg = p->*VuPeek::face_gap();
        o.push_back((double)g.find(8, fg.GetVertexuse()));
        o.push_back(fg.GetLength());
        SmPoint2d fuv = fg.GetFaceUV();
        o.push_back(fuv.x);
        o.push_back(fuv.y);
        SmPoint2d uv = p->*VuPeek::uv();
        o.push_back(uv.x);
        o.push_back(uv.y);
    }
    for (SmVertex* p : g.vertices)
    {
        put_vec(o, p->GetPoint());
        o.push_back((double)p->GetTolerance());
        o.push_back(p->*VPeek::poly() ? 1.0 : 0.0);
        std::vector<int64_t> v;
        for (SmTopology* t : list_of(p->GetList()))
            v.push_back(g.find(8, t));
        put_list(o, v);
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
    else if (kind == 4)
    {
        status = build_chain(rd, ctx, brep);
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

template <class T> T* at_or_null(std::vector<T*>& v, int64_t i)
{
    return (i >= 0 && i < (int64_t)v.size()) ? v[(size_t)i] : nullptr;
}

const SM_TYPE KIND_TYPES[] = {SmVertex_TYPE,   SmOwningTopology_TYPE, SmTopology_TYPE, SmAObject_TYPE,
                              SmObject_TYPE,   SmVertexuse_TYPE,      SmEdge_TYPE,     SmShell_TYPE,
                              SmUnknown_TYPE,  0};

// ---- null-dereference guards (what upstream dereferences without checking) ------------

SmTopology* owner_of(SmVertexuse* p) { return p->*VuPeek::owner(); }

std::vector<SmVertexuse*> uses_of(SmVertex* v)
{
    std::vector<SmVertexuse*> r;
    for (SmTopology* t : list_of(v->GetList()))
        r.push_back((SmVertexuse*)t);
    return r;
}

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
    for (SmVertexuse* q : uses_of(v))
        if (!ok_faceuse(q))
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

// SmVertexuse::IsConnectedTo(target) of a vertex use.
bool ok_vu_connected(SmVertexuse* p, SmTopology* t)
{
    if (t == nullptr)
        return true;
    SM_TYPE ty = t->GetType();
    if (ty == SmFace_TYPE || ty == SmFaceuse_TYPE)
        return ok_faceuse(p);
    if (ty == SmLoop_TYPE || ty == SmLoopuse_TYPE)
        return ok_loopuse(p);
    if (ty == SmShell_TYPE || ty == SmRegion_TYPE)
        return ok_shell(p);
    return true;
}

// GetEdges: GetEdgeuse()->GetFace() of every edge vertex use; a null edge is an error of
// the port (a malformed B-rep).
bool ok_edges(SmVertex* v)
{
    for (SmVertexuse* p : uses_of(v))
    {
        if (!p->IsEdgeVertexuse())
            continue;
        SmEdgeuse* eu = p->GetEdgeuse();
        if (eu == nullptr || eu->GetEdge() == nullptr)
            return false;
        if (eu->IsLoopEdgeuse() && eu->GetLoopuse() != nullptr && eu->GetLoopuse()->GetFaceuse() == nullptr)
            return false;
    }
    return true;
}

bool ok_faceuses(SmVertex* v)
{
    for (SmVertexuse* p : uses_of(v))
        if (!p->IsShellVertexuse() && !ok_faceuse(p))
            return false;
    return true;
}

// GetFaces: a face use without face is an error of the port (a malformed B-rep).
bool ok_faces(SmVertex* v)
{
    if (!ok_faceuses(v))
        return false;
    for (SmVertexuse* p : uses_of(v))
        if (!p->IsShellVertexuse() && p->GetFaceuse() != nullptr && p->GetFaceuse()->GetFace() == nullptr)
            return false;
    return true;
}

bool ok_shells(SmVertex* v)
{
    for (SmVertexuse* p : uses_of(v))
        if (!ok_shell(p))
            return false;
    return true;
}

// GetRegions: a shell without region is an error of the port (a malformed B-rep).
bool ok_regions(SmVertex* v)
{
    if (!ok_shells(v))
        return false;
    for (SmVertexuse* p : uses_of(v))
        if (p->GetShell() != nullptr && p->GetShell()->GetRegion() == nullptr)
            return false;
    return true;
}

// The vertex at the start of an edge use (GetVertexuse()->GetVertex()).
bool ok_eu_vertex(SmEdgeuse* eu) { return eu != nullptr && eu->GetVertexuse() != nullptr; }

// SmEdge::GetVertices(pV1, pV2): the primary edge use, its mate and their vertex uses.
bool ok_pair(SmEdge* e)
{
    SmEdgeuse* prim = e->GetPrimaryEdgeuse();
    if (prim == nullptr || !ok_eu_vertex(prim))
        return false;
    return ok_eu_vertex(prim->GetMate());
}

// SmEdge::GetStartVertex.
bool ok_start(SmEdge* e)
{
    SmEdgeuse* prim = e->GetPrimaryEdgeuse();
    if (prim == nullptr)
        return false;
    return prim->GetOrientation() == SM_OT_SAME ? ok_eu_vertex(prim) : ok_eu_vertex(prim->GetMate());
}

// SmEdge::GetVertices(array): empty without primary edge use.
bool ok_edge_vertices(SmEdge* e) { return e->GetPrimaryEdgeuse() == nullptr || ok_pair(e); }

const int64_t MAX_WALK = 100000;

// SmLoopuse::GetVertices.
bool ok_loopuse_vertices(SmLoopuse* lu)
{
    SM_TYPE ty = lu->*LuPeek::type();
    SmTopology* start = lu->*LuPeek::start();
    if (ty == SmVertexuse_TYPE)
        return start != nullptr;
    if (ty != SmEdgeuse_TYPE)
        return start == nullptr;
    SmEdgeuse* eu = (SmEdgeuse*)start;
    for (int64_t k = 0; eu != nullptr; k++)
    {
        if (k > MAX_WALK || eu->GetVertexuse() == nullptr)
            return false;
        eu = eu->GetCCWEdgeuse();
        if (eu == (SmEdgeuse*)start)
            return true;
    }
    return true;
}

bool ok_connected(SmVertex* v, SmTopology* t)
{
    if (t == nullptr)
        return true;
    switch (t->GetType())
    {
    case SmEdge_TYPE: return ok_edge_vertices((SmEdge*)t);
    case SmFace_TYPE: return ok_faces(v);
    case SmFaceuse_TYPE: return ok_faceuses(v);
    case SmLoop_TYPE:
        return ((SmLoop*)t)->GetLoopuse() != nullptr && ok_loopuse_vertices(((SmLoop*)t)->GetLoopuse());
    case SmLoopuse_TYPE: return ok_loopuse_vertices((SmLoopuse*)t);
    case SmShell_TYPE: return ok_shells(v);
    case SmRegion_TYPE: return ok_regions(v);
    default: return true;
    }
}

// IsOnPole: SmVertexuse::IsOnPole of every use.
bool ok_on_pole(SmVertex* v)
{
    for (SmVertexuse* p : uses_of(v))
        if (!ok_faceuse(p) || !ok_face_gap(p))
            return false;
    return true;
}

// AreEdgesTangent: GetStartVertex and GetEndTangent (a null curve is dereferenced; a
// failed evaluation leaves the default SmVector3d, SM_UNDEF_DOUBLE, which is defined:
// WR-256).
bool ok_tangent(SmEdge* e)
{
    return ok_start(e) && e->GetCurve() != nullptr;
}

// IsTopologicalVertex, following its control flow up to the tangent reads (IsOnPole is
// evaluated here as upstream does; it only fills caches the real call fills as well).
// Only a null curve whose tangent is read is refused: a failed GetEndTangent leaves the
// tangent IsLine wrote or the default SmVector3d (WR-256).
bool ok_topological(SmVertex* v)
{
    if (!ok_edges(v))
        return false;
    SmTArray<SmEdge*> es;
    v->GetEdges(es);
    if (es.GetSize() != 2 || es[0] == es[1])
        return true;
    if (!ok_on_pole(v))
        return false;
    if (v->IsOnPole())
        return true;
    SmEdge* e[2] = {es[0], es[1]};
    SmCurve* c[2] = {e[0]->GetCurve(), e[1]->GetCurve()};
    bool line[2];
    for (int i = 0; i < 2; i++)
    {
        SmPoint3d pt;
        SmVector3d tv;
        line[i] = c[i] && c[i]->IsLine(5, e[i]->GetTolerance() / 1000.0, pt, tv);
    }
    if (line[0] && line[1])
        return true;
    if (!ok_start(e[0]) || !ok_start(e[1]))
        return false;
    bool at_start[2] = {e[0]->GetStartVertex() == v, e[1]->GetStartVertex() == v};
    double param[2];
    for (int i = 0; i < 2; i++)
        param[i] = at_start[i] ? e[i]->GetInterval().GetMin() : e[i]->GetInterval().GetMax();
    if (!SM_ARE_SAME(param[0], param[1]))
        return true;
    if (c[0] == c[1])
        return true;
    for (int i = 0; i < 2; i++)
    {
        if (c[i] == nullptr)
            return false;
        SmVector3d t;
        if (at_start[i] && e[i]->GetEndTangent(TRUE, t) != SM_SUCCESS)
            return true; // SER returns before the other edge's tangent is read
    }
    return true;
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

template <class T> void put_ids(std::vector<double>& o, Graph& g, int kind, SmTArray<T*>& a)
{
    o.push_back((double)a.GetSize());
    for (ULONG i = 0; i < a.GetSize(); i++)
        o.push_back((double)g.find(kind, a[i]));
}

// A gap of GetMaxUpDimGap3d: `kind vu length` (kind 0 vertex/edge, 1 vertex/face, the
// vertex use whose cache holds it), `-1` for none.
void put_gap(std::vector<double>& o, Graph& g, const SmGap* p)
{
    if (p == nullptr)
    {
        o.push_back(-1.0);
        return;
    }
    for (size_t i = 0; i < g.vertexuses.size(); i++)
    {
        SmVertexuse* u = g.vertexuses[i];
        if (p == &(u->*VuPeek::edge_gap()) || p == &(u->*VuPeek::face_gap()))
        {
            o.push_back(p == &(u->*VuPeek::edge_gap()) ? 0.0 : 1.0);
            o.push_back((double)i);
            o.push_back(p->GetLength());
            return;
        }
    }
    o.push_back(-3.0);
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
    SmVertex* v = g.vertices[(size_t)rd.idx()];
    switch (op)
    {
    case OP_QUERY:
    {
        o.push_back((double)v->GetType());
        for (const SM_TYPE* t = KIND_TYPES; *t != 0; t++)
            o.push_back(flg(v->IsKindOf(*t)));
        o.push_back((double)g.find(8, v->GetPrimaryVertexuse()));
        SmTArray<SmVertexuse*> vus;
        v->GetVertexuses(vus);
        put_ids(o, g, 8, vus);
        o.push_back(ok_shells(v) ? flg(v->GetBrep() == b.brep) : ST_NULL_DEREF);
        o.push_back(flg(v->IsShellVertex()));
        o.push_back(flg(v->IsLoopVertex()));
        bool oe = ok_edges(v);
        o.push_back(oe ? flg(v->IsWireVertex()) : ST_NULL_DEREF);
        o.push_back(oe ? flg(v->IsLaminaVertex()) : ST_NULL_DEREF);
        if (oe)
        {
            SmTArray<SmEdge*> a;
            v->GetEdges(a);
            put_ids(o, g, 7, a);
        }
        else
            o.push_back(ST_NULL_DEREF);
        if (ok_faces(v))
        {
            SmTArray<SmFace*> a;
            v->GetFaces(a);
            put_ids(o, g, 3, a);
        }
        else
            o.push_back(ST_NULL_DEREF);
        if (ok_faceuses(v))
        {
            SmTArray<SmFaceuse*> a;
            v->GetFaceuses(a);
            put_ids(o, g, 2, a);
        }
        else
            o.push_back(ST_NULL_DEREF);
        if (ok_shells(v))
        {
            SmTArray<SmShell*> a;
            v->GetShells(a);
            put_ids(o, g, 1, a);
        }
        else
            o.push_back(ST_NULL_DEREF);
        if (ok_regions(v))
        {
            SmTArray<SmRegion*> a;
            v->GetRegions(a);
            put_ids(o, g, 0, a);
        }
        else
            o.push_back(ST_NULL_DEREF);
        break;
    }
    case OP_CONNECTED:
    {
        int64_t ty = rd.idx();
        int64_t i = rd.idx();
        SmTopology* t = target_of(g, ty, i);
        o.push_back(ok_connected(v, t) ? flg(v->IsConnectedTo(t)) : ST_NULL_DEREF);
        break;
    }
    case OP_VU_OF_FACE:
    {
        SmFace* f = at_or_null(g.faces, rd.idx());
        bool ok = true;
        if (f != nullptr)
            for (SmVertexuse* p : uses_of(v))
            {
                if (!ok_faceuse(p))
                {
                    ok = false;
                    break;
                }
                SmFaceuse* fu = p->GetFaceuse();
                if (fu == nullptr)
                    continue;
                if (fu->GetFace() == nullptr)
                {
                    ok = false;
                    break;
                }
                if (fu->GetFace()->GetUpwardFaceuse() == fu)
                    break;
            }
        o.push_back(ok ? (double)g.find(8, v->GetVertexuseOfFace(f)) : ST_NULL_DEREF);
        break;
    }
    case OP_VUS_OF_FACE:
    case OP_VUS_OF_EDGE:
    {
        bool face = op == OP_VUS_OF_FACE;
        int64_t i = rd.idx();
        int64_t ty = rd.idx();
        int64_t ti = rd.idx();
        SmTopology* tgt = target_of(g, ty, ti);
        SmTopology* x = face ? (SmTopology*)at_or_null(g.faces, i) : (SmTopology*)at_or_null(g.edges, i);
        bool ok = true;
        if (x != nullptr)
            for (SmVertexuse* p : uses_of(v))
            {
                bool match;
                if (face)
                {
                    if (!ok_faceuse(p))
                    {
                        ok = false;
                        break;
                    }
                    match = p->GetFaceuse() != nullptr && p->GetFaceuse()->GetFace() == (SmFace*)x;
                }
                else
                    match = p->GetEdgeuse() != nullptr && p->GetEdgeuse()->GetEdge() == (SmEdge*)x;
                if (match && !ok_vu_connected(p, tgt))
                {
                    ok = false;
                    break;
                }
            }
        if (!ok)
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmTArray<SmVertexuse*> a;
        if (face)
            v->GetVertexusesOfFace((SmFace*)x, a, tgt);
        else
            v->GetVertexusesOfEdge((SmEdge*)x, a, tgt);
        put_ids(o, g, 8, a);
        break;
    }
    case OP_COMMON_EDGES:
    {
        SmVertex* other = at_or_null(g.vertices, rd.idx());
        SmFace* f = at_or_null(g.faces, rd.idx());
        bool ok = ok_edges(v);
        if (ok)
        {
            SmTArray<SmEdge*> es;
            v->GetEdges(es, f);
            for (ULONG k = 0; k < es.GetSize(); k++)
                ok = ok && ok_pair(es[k]);
            if (ok)
                put_ids(o, g, 7, es);
        }
        if (!ok)
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmTArray<SmEdge*> a;
        v->GetCommonEdges(other, a, f);
        put_ids(o, g, 7, a);
        break;
    }
    case OP_SHELL_IN_REGION:
    {
        SmRegion* r = at_or_null(g.regions, rd.idx());
        if (!ok_shells(v))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmShell* s = (SmShell*)0x1;
        o.push_back(st(v->GetShellInRegion(r, s)));
        o.push_back((double)g.find(1, s));
        break;
    }
    case OP_TANGENT:
    {
        SmEdge* e1 = g.edges[(size_t)rd.idx()];
        SmEdge* e2 = g.edges[(size_t)rd.idx()];
        double tol = rd.num();
        if (!ok_tangent(e1) || !ok_tangent(e2))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        o.push_back(flg(v->AreEdgesTangent(e1, e2, tol)));
        break;
    }
    case OP_TOPOLOGICAL:
    {
        if (!ok_topological(v))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        o.push_back(flg(v->IsTopologicalVertex()));
        put_gap_states(o, g);
        break;
    }
    case OP_ON_POLE:
    {
        if (!ok_on_pole(v))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        o.push_back(flg(v->IsOnPole()));
        put_gap_states(o, g);
        break;
    }
    case OP_GAPS:
    {
        bool want_array = rd.flag();
        bool has_tol = rd.flag();
        double tol = rd.num();
        bool ok = true;
        for (SmVertexuse* p : uses_of(v))
            ok = ok && ok_edge_gap(p) && ok_face_gap(p);
        if (!ok)
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmGapArray arr;
        SmTol3d t3(tol);
        // upstream adds to the array without checking it: the shim always passes one and
        // reports the null dereference when none was asked for and a gap was added
        const SmGap* m = v->GetMaxUpDimGap3d(&arr, has_tol ? &t3 : NULL);
        if (!want_array && arr.GetSize() > 0)
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        o.push_back(0.0);
        put_gap(o, g, m);
        if (want_array)
        {
            o.push_back((double)arr.GetSize());
            for (ULONG k = 0; k < arr.GetSize(); k++)
                put_gap(o, g, arr[k]);
        }
        else
            o.push_back(-1.0);
        put_gap_states(o, g);
        break;
    }
    case OP_NOTIFY:
    {
        v->Notify((SmNotifyOperation)rd.idx(), NULL, NULL, NULL);
        put_gap_states(o, g);
        break;
    }
    case OP_SET_POINT:
    {
        SmPoint3d p = rd.vec();
        v->SetPoint(p);
        put_vec(o, v->GetPoint());
        put_gap_states(o, g);
        break;
    }
    case OP_SET_TOLERANCE:
    {
        double tol = rd.num();
        SmBoolean larger = rd.bflag();
        SmBoolean cascade = rd.bflag();
        v->SetTolerance(tol, larger, cascade);
        o.push_back((double)v->GetTolerance());
        break;
    }
    case OP_REFRESH_TOLERANCE:
    {
        SmBoolean nested = rd.bflag();
        if (nested && (!ok_edges(v) || !ok_faces(v)))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        o.push_back(st(v->RefreshTolerance(nested)));
        o.push_back((double)v->GetTolerance());
        for (SmEdge* e : g.edges)
            o.push_back((double)e->GetTolerance());
        break;
    }
    case OP_EXACT:
    {
        SmBoolean exact = rd.bflag();
        bool poly = rd.flag();
        v->SetExact(exact);
        // a dummy non-null back pointer (never dereferenced), cleared again below
        v->SetPolyVertex(poly ? (SmPolyVertex*)&o : NULL);
        o.push_back(flg(v->IsExact()));
        o.push_back(v->GetPolyVertex() != NULL ? 1.0 : 0.0);
        v->SetPolyVertex(NULL);
        break;
    }
    default:
        break;
    }
}
} // namespace

SC_EXPORT int32_t sc_topology_vertex_eval(int32_t op, const char* path, const double* in, int64_t n_in,
                                          double* out, int64_t cap, int64_t* out_len)
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
