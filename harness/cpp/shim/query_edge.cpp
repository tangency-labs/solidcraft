// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `query.edge`: the SmEdge members owned by the unit
// (source/SMLib/src/SmEdge.cpp) on real SmBrep objects: CalculateBoundingBox,
// CalculateTightBoundingBox, ClosestPoint, RadialSectorClassify and
// RadialSectorOfCurveClassify.
//
// One entry point, sc_query_edge_eval(op, path, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/query_edge.rs, which validates every input.
// Every op starts with a B-rep block and an edit block:
//   `0 rebuild`                      SmBrep::ReadFromFile(path) (ASCII .smb)
//   `2 surface u0 v0 u1 v1`          CreateFaceFromSurface(surface, dom)
//   `4 curve t0 t1`                  CreateWireEdgeFromCurve(curve, [t0, t1])
//   edits: `edge tol has0 P0[3] has1 P1[3] drop_curve` (edge -1: none) on edge `edge`:
//   SetTolerance(tol, FALSE, FALSE) when tol >= 0, SetPoint of the start / end vertex
//   (GetVertices(start, end)), SetCurve(NULL, TRUE, FALSE, FALSE) when drop_curve.
// A surface block is `0 oob form n m p q r s cpts(x y z w) u v` (SmBSplineSurface) or
// `1 O[3] N[3]` (SmPlane(O, N)), as unit topology.edgeuse_p1; a curve block is `kind dim
// ...` (0 SmBSplineCurve `oob n p m cpts(x y z w) knots`, 1 SmHermiteCurve `P1 D1 P2 D2`,
// 2 bounded SmLine `P V d0 d1 scale io`, 3 SmEllipse `C X Y d0 d1 rx ry io`, 4 SmCircle
// `C X Y d0 d1 r io`), as unit intersect.topology_solver_p1.
//
// The graph walk and export (Graph, walk, export_graph and their helpers) are copies of
// the harness code of harness/cpp/shim/sc_edgeuse_graph.h, so the export has that
// unit's layout and traversal order (the Rust side imports it with that unit's
// importer), except that an edge curve which is not an SmBSplineCurve is exported as
// none and the curve type of every edge follows the graph. They read raw members
// (protected ones through pointer-to-member access), never the functions under test.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmFace.h>
#include <SmFaceuse.h>
#include <SmGap.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmLoop.h>
#include <SmLoopuse.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmPolarBox.h>
#include <SmPseudoBox.h>
#include <SmRegion.h>
#include <SmShell.h>
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
    OP_BBOX = 1,
    OP_TIGHT = 2,
    OP_CLOSEST = 3,
    OP_RADIAL = 4,
    OP_RADIAL_CURVE = 5,
};

// Output of an op the shim refuses because upstream would dereference null.
const double ST_NULL_DEREF = -2.0;

struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    bool flag() { return in[at++] != 0.0; }
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

gw_CURVE* read_nurb(Rd& rd)
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

// A curve block (see the header).
SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    ULONG dim = (ULONG)rd.idx();
    if (kind == 0)
    {
        bool oob = rd.flag();
        SmBSplineCurve* c = new (ctx) SmBSplineCurve(read_nurb(rd), dim, FALSE);
        c->SetOutOfBoundsEnabled(oob ? TRUE : FALSE);
        return c;
    }
    if (kind == 2)
    {
        SmPoint3d p = rd.vec();
        SmVector3d v = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double scale = rd.num();
        bool io = rd.flag();
        return new (ctx) SmLine(p, v, SmExtent1d(d0, d1), scale, 3, &ctx, NULL, io ? TRUE : FALSE);
    }
    if (kind == 3)
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
    if (kind == 4)
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
    SmVector3d p1 = rd.vec();
    SmVector3d d1 = rd.vec();
    SmVector3d p2 = rd.vec();
    SmVector3d d2 = rd.vec();
    return new (ctx) SmHermiteCurve(p1, d1, p2, d2, dim);
}

// ---- raw member access (copy of topology_edgeuse_p1.cpp) ----------------------------------

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
};
struct ShPeek : SmShell
{
    static SM_TYPE SmShell::*type() { return &ShPeek::m_tShellType; }
};

// ---- traversal-order registry (copy of topology_edgeuse_p1.cpp) ---------------------------

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

void walk(SmBrep* brep, Graph& g)
{
    SmTArray<SmRegion*> regs;
    brep->GetRegions(regs);
    for (ULONG i = 0; i < regs.GetSize(); i++)
        g.region(regs[i]);
    size_t r = 0, s = 0, fu = 0, lu = 0, eu = 0, vu = 0, l = 0, e = 0, f = 0;
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
        for (; f < g.faces.size(); f++)
            ;
    }
}

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
        put_surface(o, p->GetSurface());
        SmExtent2d d = p->GetUVDomain();
        o.push_back(d.GetMin().x);
        o.push_back(d.GetMin().y);
        o.push_back(d.GetMax().x);
        o.push_back(d.GetMax().y);
        o.push_back((double)p->GetTolerance());
        o.push_back(p->GetSurface() ? (double)p->GetSurface()->GetType() : -1.0);
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
        // (deviation from the copied export: an edge curve that is not an
        // SmBSplineCurve is exported as none; its kind follows the graph)
        const SmCurve* c = p->GetCurve();
        put_curve(o, (c != nullptr && c->IsKindOf(SmBSplineCurve_TYPE)) ? c : nullptr);
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
    }
    for (SmVertex* p : g.vertices)
    {
        put_vec(o, p->GetPoint());
        o.push_back((double)p->GetTolerance());
    }
    o.push_back((double)brep->GetTolerance());
    // the curve type of every edge (-1 none), after the graph
    for (SmEdge* p : g.edges)
        o.push_back(p->GetCurve() ? (double)p->GetCurve()->GetType() : -1.0);
}

// ---- B-rep construction --------------------------------------------------------------------

// Builds the B-rep of a B-rep block; nullptr (status in `status`) when construction fails.
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
    else if (kind == 2)
    {
        SmSurface* s = read_surface(rd, ctx);
        SmPoint2d a = rd.uv(), b = rd.uv();
        SmFace* pFace = nullptr;
        status = brep->CreateFaceFromSurface(s, SmExtent2d(a, b), pFace);
    }
    else
    {
        SmCurve* c = read_curve(rd, ctx);
        double t0 = rd.num(), t1 = rd.num();
        SmEdge* pEdge = nullptr;
        status = brep->CreateWireEdgeFromCurve(c, SmExtent1d(t0, t1), pEdge);
    }
    if (status != SM_SUCCESS)
    {
        delete brep;
        return nullptr;
    }
    return brep;
}

// Applies the edit block to the walked B-rep.
void edit(Rd& rd, Graph& g)
{
    int64_t e = rd.idx();
    double tol = rd.num();
    bool has0 = rd.flag();
    SmPoint3d p0 = rd.vec();
    bool has1 = rd.flag();
    SmPoint3d p1 = rd.vec();
    bool drop = rd.flag();
    if (e < 0 || e >= (int64_t)g.edges.size())
        return;
    SmEdge* pEdge = g.edges[(size_t)e];
    if (tol >= 0.0)
        pEdge->SetTolerance(SM_CAST_ZONETOL3D(tol), FALSE, FALSE);
    if (has0 || has1)
    {
        SmVertex *pStart = nullptr, *pEnd = nullptr;
        pEdge->GetVertices(pStart, pEnd);
        if (has0 && pStart)
            pStart->SetPoint(p0);
        if (has1 && pEnd)
            pEnd->SetPoint(p1);
    }
    if (drop)
        pEdge->SetCurve(NULL, TRUE, FALSE, FALSE);
}

struct Built
{
    SmBrep* brep = nullptr;
    ~Built() { delete brep; }
};

SmEdgeuse* eu_at(Graph& g, int64_t i)
{
    return (i >= 0 && i < (int64_t)g.edgeuses.size()) ? g.edgeuses[(size_t)i] : nullptr;
}

SmEdge* edge_at(Graph& g, int64_t i)
{
    return (i >= 0 && i < (int64_t)g.edges.size()) ? g.edges[(size_t)i] : nullptr;
}

void put_pseudo(std::vector<double>& o, const SmPseudoBox& b)
{
    for (ULONG i = 0; i < 3; i++)
        put_vec(o, b.GetBasis(i));
    for (ULONG i = 0; i < 3; i++)
    {
        o.push_back(b.GetInterval(i).GetMin());
        o.push_back(b.GetInterval(i).GetMax());
    }
}

void put_polar(std::vector<double>& o, const SmPolarBox& b)
{
    o.push_back((double)b.GetPolarBoxState());
    for (ULONG i = 0; i < 3; i++)
        put_vec(o, *b.GetBasis(i));
    SmExtent2d d = b.GetDomain();
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
}

// The face surface of a loop edge use exists (RadialSectorClassify dereferences both).
bool has_surface(SmEdgeuse* p)
{
    return p != nullptr && p->IsLoopEdgeuse() && p->GetFace() != nullptr && p->GetFace()->GetSurface() != nullptr;
}

void run(int32_t op, Rd& rd, Graph& g, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    switch (op)
    {
    case OP_BBOX:
    {
        SmEdge* e = edge_at(g, rd.idx());
        bool want_n = rd.flag();
        bool want_p = rd.flag();
        bool want_l = rd.flag();
        bool tight = rd.flag();
        SmExtent3d nb;
        SmPseudoBox pb;
        SmPolarBox pl;
        SmStatus s = e->CalculateBoundingBox(want_n ? &nb : NULL, want_p ? &pb : NULL, want_l ? &pl : NULL,
                                             tight ? TRUE : FALSE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            if (want_n)
            {
                put_vec(o, nb.GetMin());
                put_vec(o, nb.GetMax());
            }
            if (want_p)
                put_pseudo(o, pb);
            if (want_l)
                put_polar(o, pl);
        }
        break;
    }
    case OP_TIGHT:
    {
        SmEdge* e = edge_at(g, rd.idx());
        SmExtent3d b;
        b.Init();
        SmStatus s = e->CalculateTightBoundingBox(&b);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            put_vec(o, b.GetMin());
            put_vec(o, b.GetMax());
        }
        break;
    }
    case OP_CLOSEST:
    {
        SmEdge* e = edge_at(g, rd.idx());
        SmPoint3d p = rd.vec();
        if (e->GetCurve() == NULL)
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmBoolean ok = FALSE;
        double t = 0.0, d = 0.0;
        SmStatus s = e->ClosestPoint(p, ok, t, d);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(ok));
            if (ok)
            {
                o.push_back(t);
                o.push_back(d);
            }
        }
        break;
    }
    case OP_RADIAL:
    {
        SmEdgeuse* sec = eu_at(g, rd.idx());
        SmEdgeuse* cls = eu_at(g, rd.idx());
        double n = rd.num();
        SmEdge* e = sec ? sec->GetEdge() : NULL;
        SmEdgeuse* rad = sec ? sec->GetRadial() : NULL;
        if (e == NULL || e->GetCurve() == NULL || cls == NULL || !has_surface(sec) || !has_surface(rad) ||
            !has_surface(cls))
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        ULONG res = 99;
        SmStatus s = e->RadialSectorClassify(n, sec, cls, res, NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back((double)res);
        break;
    }
    case OP_RADIAL_CURVE:
    {
        SmEdgeuse* sec = eu_at(g, rd.idx());
        double t = rd.num();
        SmCurve* c = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        SmOrientType orient = (SmOrientType)rd.idx();
        bool force = rd.flag();
        SmEdge* e = sec ? sec->GetEdge() : NULL;
        if (e == NULL || !has_surface(sec) || !has_surface(sec->GetRadial()))
        {
            o.push_back(ST_NULL_DEREF);
            delete c;
            break;
        }
        ULONG res = 99;
        SmStatus s = e->RadialSectorOfCurveClassify(sec, t, *c, SmExtent1d(a, b), orient, force ? TRUE : FALSE, res);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back((double)res);
        delete c;
        break;
    }
    default: break;
    }
}
} // namespace

SC_EXPORT int32_t sc_query_edge_eval(int32_t op, const char* path, const double* in, int64_t n_in, double* out,
                                     int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    SmStatus status = SM_SUCCESS;
    Built b;
    b.brep = build(rd, path, status);
    if (b.brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(status));
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    Graph g;
    walk(b.brep, g);
    edit(rd, g);
    o.push_back(0.0);
    if (op == OP_EXPORT)
        export_graph(b.brep, g, o);
    else
        run(op, rd, g, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
