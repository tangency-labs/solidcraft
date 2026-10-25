// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `query.face_p4`: SmFace::ClassifyLoops (source/SMLib/src/SmFace.cpp)
// on real SmBrep objects.
//
// One entry point, sc_query_face_p4_eval(op, path, in, n_in, out, cap, out_len). Layouts
// are documented in crates/solidcore-ffi/src/units/query_face_p4.rs, which validates every
// input. Every op starts with a B-rep block, then a case block `face props_mode loops_mode`:
//   `0 rebuild validate_only init_gaps`             SmBrep::ReadFromFile(path) (ASCII .smb)
//   `2 surface u0 v0 u1 v1 validate_only init_gaps` CreateFaceFromSurface(surface, dom)
//   `4 copies surf n_loops cnt* n_curves curve* n_pts pt* validate_only init_gaps`
//      SmBrep::MakeFaceWithCurves over a real SmCylinder / SmSphere / SmTorus / SmCone or a
//      B-spline / SmPlane, with 2D line and circle UV trim curves and single vertex loops,
//      `copies` times into one B-rep (the faces of prog_test my_test_tsurf_cyl_creation_fast
//      and of missing-seam faces: circles of a cylinder without seam edge, sphere caps and
//      bands, torus bands)
//   `5 surf validate_only init_gaps`                CreateFaceInRegionFromSurface over the
//      surface's natural domain (prog_test my_test_closed_sphere_properties)
// The B-rep traversal and export (Rd .. export_graph) are copies of the harness code of
// harness/cpp/shim/topology_face_p2.cpp (via query_face_p2.cpp), so the export has that
// unit's layout and traversal order (the Rust side imports it with that unit's importer),
// except that a face's analytic surface (sc_analytic_export.h) and an edge's ellipse or
// circle (sc_exact_curve_export.h) are preceded by their exact database record, which the
// importer rebuilds instead of the B-spline part when the record is lossless.
//
// The face props ClassifyLoops reads are built here as upstream's only caller,
// SmFaceProps::SetProps_Stage3, builds them before its call (props_mode 1): SetProps_Stage2,
// ReSet(SM_PROPSTAGE_3), then SmLoopProps::SetProps for every face loop, collecting the
// closed loops (a verbatim copy of SmFaceProps.cpp lines 1300-1341 without the debug
// blocks). props_mode 0 passes fresh props (ClassifyLoops runs SetProps_Stage2 itself).
// Op 0 exports the graph, the props (their values ClassifyLoops reads) and the target loops;
// op 1 runs ClassifyLoops and exports its results.
#include "sc_analytic_export.h"
#include "sc_exact_curve_export.h"
#include "sc_shim.h"
#include "support/topology_fixture.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmCone.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveClass.h>
#include <SmCylinder.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFace.h>
#include <SmFaceProps.h>
#include <SmFaceuse.h>
#include <SmGap.h>
#include <SmHealProbArray.h>
#include <SmLine.h>
#include <SmLoop.h>
#include <SmLoopProps.h>
#include <SmLoopuse.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmPointClass.h>
#include <SmRegion.h>
#include <SmShell.h>
#include <SmSphere.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmTorus.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <SmVertex.h>
#include <SmVertexuse.h>
#include <nurbs.h>

#include <cmath>
#include <map>
#include <vector>

namespace
{
enum Op : int32_t
{
    OP_EXPORT = 0,
    OP_CLASSIFY = 1,
};
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


// ---- B-rep construction -------------------------------------------------------------------

// A surface block of kinds 4 and 5: `0|1 ..` read_surface, or `kind scale a b c swap` with
// kind 10 SmCylinder(radius scale*a), 11 SmSphere(radius scale*a), 12 SmTorus(major scale*a,
// minor scale*b), 13 SmCone(bottom radius scale*a, semi-angle b degrees), real analytic
// surfaces made by their CreateCanonical at the default placement, SwapUV'd when `swap`.
// `dom` receives the natural domain with an unbounded direction (above 1e5 wide: the
// cylinder's and cone's axis) clamped to [0, scale*c].
SmSurface* read_any_surface(Rd& rd, const SmContext& ctx, SmExtent2d& dom)
{
    int64_t kind = (int64_t)rd.in[rd.at];
    if (kind < 10)
    {
        SmSurface* s = read_surface(rd, ctx);
        dom = s->GetNaturalUVDomain();
        return s;
    }
    rd.at++;
    double scale = rd.num(), a = rd.num(), b = rd.num(), c = rd.num();
    bool swap = rd.flag();
    SmAxis2Placement origin;
    SmBSplineSurface* s = nullptr;
    SmStatus st = SM_ERR;
    if (kind == 10)
    {
        SmCylinder* p = nullptr;
        st = SmCylinder::CreateCanonical(ctx, origin, scale * a, p);
        s = p;
    }
    else if (kind == 11)
    {
        SmSphere* p = nullptr;
        st = SmSphere::CreateCanonical(ctx, origin, scale * a, p);
        s = p;
    }
    else if (kind == 12)
    {
        SmTorus* p = nullptr;
        st = SmTorus::CreateCanonical(ctx, origin, scale * a, scale * b, p);
        s = p;
    }
    else
    {
        SmCone* p = nullptr;
        st = SmCone::CreateCanonical(ctx, origin, scale * a, b, p);
        s = p;
    }
    if (st != SM_SUCCESS || s == nullptr)
        return nullptr;
    if (swap && s->SwapUV() != SM_SUCCESS)
    {
        delete s;
        return nullptr;
    }
    dom = s->GetNaturalUVDomain();
    SmPoint2d lo = dom.GetMin(), hi = dom.GetMax();
    if (hi.x - lo.x > 1e5)
    {
        lo.x = 0.0;
        hi.x = scale * c;
    }
    if (hi.y - lo.y > 1e5)
    {
        lo.y = 0.0;
        hi.y = scale * c;
    }
    dom = SmExtent2d(lo, hi);
    return s;
}

// The point at fractions (a, b) of `dom`.
SmPoint2d at_frac(const SmExtent2d& dom, double a, double b)
{
    return SmPoint2d(dom.GetMin().x + a * (dom.GetMax().x - dom.GetMin().x),
                     dom.GetMin().y + b * (dom.GetMax().y - dom.GetMin().y));
}

// One MakeFaceWithCurves face (kind 4 after `copies`); SM_SUCCESS or the failing status.
SmStatus make_face_with_curves(Rd& rd, const SmContext& ctx, SmBrep* brep)
{
    SmExtent2d dom;
    SmSurface* srf = read_any_surface(rd, ctx, dom);
    SmTArray<ULONG> counts;
    int64_t n_loops = rd.idx();
    for (int64_t i = 0; i < n_loops; i++)
        counts.Add((ULONG)rd.idx());
    SmTArray<SmBSplineCurve*> uv;
    SmTArray<SmOrientType> orients;
    int64_t n_curves = rd.idx();
    SmStatus status = srf ? SM_SUCCESS : SM_ERR;
    for (int64_t i = 0; i < n_curves; i++)
    {
        // `type a b c d orient`: 0 line from fractions (a, b) to (c, d) of the domain;
        // 1 circle centred at fractions (a, b), radius c times the smaller domain side
        // (d unused), counter-clockwise; orient 1 SM_OT_SAME, 2 SM_OT_OPPOSITE
        int64_t type = rd.idx();
        double a = rd.num(), b = rd.num(), c = rd.num(), d = rd.num();
        SmOrientType orient = rd.idx() == 2 ? SM_OT_OPPOSITE : SM_OT_SAME;
        SmBSplineCurve* curve = nullptr;
        if (type == 0)
        {
            SmPoint2d p = at_frac(dom, a, b), q = at_frac(dom, c, d);
            if (status == SM_SUCCESS)
                status = SmBSplineCurve::CreateLineSegment(ctx, 2, SmPoint3d(p.x, p.y, 0.0), SmPoint3d(q.x, q.y, 0.0),
                                                           curve);
        }
        else
        {
            SmPoint2d ctr = at_frac(dom, a, b);
            double side = std::fmin(dom.GetMax().x - dom.GetMin().x, dom.GetMax().y - dom.GetMin().y);
            SmAxis2Placement frame;
            frame.SetCanonical(SmPoint3d(ctr.x, ctr.y, 0.0), SmVector3d(0, 0, 1), SmVector3d(1, 0, 0));
            if (status == SM_SUCCESS)
                status = SmBSplineCurve::CreateCircleSegment(ctx, 2, frame, c * side, 0.0, 360.0, SM_CO_QUADRATIC,
                                                             curve);
        }
        if (curve)
        {
            uv.Add(curve);
            orients.Add(orient);
        }
    }
    SmTArray<SmPoint3d> points;
    int64_t n_pts = rd.idx();
    for (int64_t i = 0; i < n_pts; i++)
    {
        double a = rd.num(), b = rd.num();
        SmPoint3d p;
        if (status == SM_SUCCESS)
            status = srf->EvaluatePoint(at_frac(dom, a, b), p);
        points.Add(p);
    }
    if (status != SM_SUCCESS)
    {
        for (ULONG i = 0; i < uv.GetSize(); i++)
            delete uv[i];
        delete srf;
        return status;
    }
    SmRegion* new_region = nullptr;
    SmShell* new_shell = nullptr;
    SmFace* new_face = nullptr;
    status = brep->MakeFaceWithCurves(brep->GetInfiniteRegion(), counts, NULL, &uv, orients, points, srf, dom, SM_OT_SAME,
                                      new_region, new_shell, new_face);
    return status;
}

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
    else if (kind == 4)
    {
        brep->SetTolerance(1e-7, FALSE);
        int64_t copies = rd.idx();
        int64_t start = rd.at;
        for (int64_t i = 0; i < copies; i++)
        {
            rd.at = start;
            SmStatus s = make_face_with_curves(rd, ctx, brep);
            if (status == SM_SUCCESS)
                status = s;
        }
    }
    else
    {
        brep->SetTolerance(1e-7, FALSE);
        SmExtent2d dom;
        SmSurface* s = read_any_surface(rd, ctx, dom);
        SmShell* shell = nullptr;
        SmFace* face = nullptr;
        status = s ? brep->CreateFaceInRegionFromSurface(brep->GetInfiniteRegion(), s, dom, shell, face) : SM_ERR;
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

// ---- the case: face props and target loops --------------------------------------------------

struct Case
{
    SmFace* face = nullptr;
    ULONG face_index = 0;
    SmFaceProps* props = nullptr;
    SmTArray<SmLoop*> loops;
    bool rejected = false;
    ~Case() { delete props; }
};

// props_mode 1: the face props SetProps_Stage3 has built when it calls ClassifyLoops, and
// its closed loops (SmFaceProps.cpp, SetProps_Stage3, verbatim from `ReSet(SM_PROPSTAGE_3)`
// to the closed loop count, without the debug blocks; m_pFace is the face).
void stage3_prefix(SmFaceProps& self, SmTArray<SmLoop*>& sClosedLoops)
{
    self.ReSet(SM_PROPSTAGE_3);
    self.m_ePropStage = SM_PROPSTAGE_3a;
    ULONG ii;
    SmTArray<SmLoop*> sLoops;
    self.m_pFace->GetLoops(sLoops);
    self.m_sLoopProps.SetSize(sLoops.GetSize());
    for (ii = 0; ii < sLoops.GetSize(); ii++)
    {
        SmLoop* pLoop = sLoops[ii];
        self.m_sLoopProps[ii].SetContext(self.m_cpContext);
        self.m_sLoopProps[ii].SetProps(pLoop, self);
        if (self.m_sLoopProps[ii].m_tLoopuseType == SmVertexuse_TYPE)
        {
            if ((self.m_sLoopProps[ii].m_lSeamCrossingCntU > 0 || self.m_sLoopProps[ii].m_lSeamCrossingCntV > 0) &&
                (self.m_sLoopProps[ii].m_bVertexLoopOnPole))
            {
                sClosedLoops.Add(pLoop);
            }
        }
        else if (self.m_sLoopProps[ii].m_bClosed3d || self.m_sLoopProps[ii].m_bClosedPtrs)
        {
            sClosedLoops.Add(pLoop);
        }
        else
        {
            self.m_sLoopProps[ii].m_eContainmentType = SM_CMT_WIRELOOP;
        }
    }
    self.m_lClosedLoopCnt = sClosedLoops.GetSize();
}

// Whether a fresh SmFaceProps of the face would be on a closed surface (SetProps_Stage2 on
// a separate build of the same B-rep, so the case's B-rep is untouched).
bool fresh_props_closed(Rd rd, const char* path, int64_t face_sel)
{
    SmStatus st;
    SmBrep* brep = build(rd, path, st);
    if (brep == nullptr)
        return true;
    Graph g;
    walk(brep, g);
    bool closed = true;
    if (!g.faces.empty())
    {
        ULONG fi = (ULONG)(face_sel % (int64_t)g.faces.size());
        SmFace* face = g.faces[fi];
        SmFaceProps props(face, fi, face->GetContext());
        props.SetProps_Stage2(face, fi, SM_HO_ALL);
        closed = props.m_bClosedU != FALSE || props.m_bClosedV != FALSE;
    }
    delete brep;
    return closed;
}

// The guard against inputs ClassifyLoops reads outside its arrays with (the FFI wrapper
// documents each): a touch list of odd length, a touch connected to something other
// than a loop, a touch loop index outside the target loops, an empty seam classification
// of a loop crossing that seam, more target loops than loop props on a closed surface.
bool staged_guard(const SmFaceProps& p, ULONG n_target)
{
    const SmTArray<SmTouchData>* lists[2] = {&p.m_sDoneTouchListU, &p.m_sDoneTouchListV};
    for (const SmTArray<SmTouchData>* l : lists)
    {
        if (l->GetSize() % 2 != 0)
            return true;
        for (ULONG i = 0; i < l->GetSize(); i++)
        {
            const SmTouchData& t = l->GetAt(i);
            if (t.m_pConnectedTo != nullptr && !t.m_pConnectedTo->IsKindOf(SmLoop_TYPE))
                return true;
            if (t.m_lLoopIndx >= n_target)
                return true;
        }
    }
    bool closed = p.m_bClosedU != FALSE || p.m_bClosedV != FALSE;
    if (closed && n_target > p.m_sLoopProps.GetSize())
        return true;
    for (ULONG i = 0; i < p.m_sLoopProps.GetSize(); i++)
    {
        const SmLoopProps& lp = p.m_sLoopProps[i];
        if (p.m_bClosedU != FALSE && lp.m_lSeamCrossingCntU > 0 && (!p.m_pCrvClassU || p.m_pCrvClassU->GetSize() == 0))
            return true;
        if (p.m_bClosedV != FALSE && lp.m_lSeamCrossingCntV > 0 && (!p.m_pCrvClassV || p.m_pCrvClassV->GetSize() == 0))
            return true;
    }
    return false;
}

// Sets up the case on `brep`: `face_sel props_mode loops_mode` (see the FFI wrapper).
void setup(Rd& rd, const Rd& brep_rd, const char* path, Graph& g, Case& c)
{
    int64_t face_sel = rd.idx();
    int64_t props_mode = rd.idx();
    int64_t loops_mode = rd.idx();
    if (g.faces.empty())
    {
        c.rejected = true;
        return;
    }
    c.face_index = (ULONG)(face_sel % (int64_t)g.faces.size());
    c.face = g.faces[c.face_index];
    c.props = new SmFaceProps(c.face, c.face_index, c.face->GetContext());
    SmTArray<SmLoop*> base;
    if (props_mode == 1)
    {
        c.props->SetProps_Stage2(c.face, c.face_index, SM_HO_ALL);
        stage3_prefix(*c.props, base);
    }
    else
    {
        if (fresh_props_closed(brep_rd, path, face_sel))
            c.rejected = true;
        c.face->GetLoops(base);
    }
    // loops_mode: 0 empty (ClassifyLoops gets them), 1 base, 2 base reversed, 3 base
    // without its last loop, 4 base with its first loop replaced by a loop of the next
    // face (prepended to an empty base)
    if (loops_mode == 1 || loops_mode == 3 || loops_mode == 4)
    {
        ULONG first = 0;
        if (loops_mode == 4)
        {
            SmFace* other = g.faces[(c.face_index + 1) % g.faces.size()];
            SmTArray<SmLoop*> ol;
            other->GetLoops(ol);
            if (other == c.face || ol.GetSize() == 0)
                c.rejected = true;
            else
                c.loops.Add(ol[0]);
            first = base.GetSize() > 0 ? 1 : 0;
        }
        ULONG n = base.GetSize() - (loops_mode == 3 && base.GetSize() > 0 ? 1 : 0);
        for (ULONG i = first; i < n; i++)
            c.loops.Add(base[i]);
    }
    else if (loops_mode == 2)
    {
        for (ULONG i = base.GetSize(); i > 0; i--)
            c.loops.Add(base[i - 1]);
    }
    if (props_mode == 1)
    {
        ULONG n_target = c.loops.GetSize();
        if (n_target == 0)
        {
            SmTArray<SmLoop*> all;
            c.face->GetLoops(all);
            n_target = all.GetSize();
        }
        if (staged_guard(*c.props, n_target))
            c.rejected = true;
    }
}

int64_t topo_index(Graph& g, const SmObject* o)
{
    if (o == nullptr)
        return -1;
    if (o->IsKindOf(SmLoop_TYPE))
        return g.find(5, (const SmLoop*)o);
    if (o->IsKindOf(SmVertex_TYPE))
        return g.find(9, (const SmVertex*)o);
    return -2;
}

void put_classification(std::vector<double>& o, Graph& g, const SmCurveClassification* cc)
{
    if (cc == nullptr)
    {
        o.push_back(0.0);
        return;
    }
    o.push_back(1.0);
    o.push_back((double)cc->GetSize());
    for (ULONG i = 0; i < cc->GetSize(); i++)
    {
        const SmCurveInterval& iv = cc->GetAt(i);
        const SmPointClassification* ends[2] = {&iv.m_vStart, &iv.m_vEnd};
        for (const SmPointClassification* pc : ends)
        {
            o.push_back((double)pc->GetPointClass());
            o.push_back(pc->GetPointClass() == SM_PC_VERTEX ? (double)topo_index(g, pc->GetObject()) : -1.0);
        }
    }
}

void put_touches(std::vector<double>& o, Graph& g, const SmTArray<SmTouchData>& l)
{
    o.push_back((double)l.GetSize());
    for (ULONG i = 0; i < l.GetSize(); i++)
    {
        const SmTouchData& t = l.GetAt(i);
        o.push_back((double)topo_index(g, t.m_pConnectedTo));
        o.push_back((double)t.m_eLoopDir);
        o.push_back((double)t.m_lLoopIndx);
    }
}

// The props block (layout in the FFI wrapper).
void put_props(std::vector<double>& o, Graph& g, const SmFaceProps& p)
{
    o.push_back((double)p.m_ePropStage);
    o.push_back((double)p.m_lFaceIndx);
    o.push_back((double)p.m_bClosedU);
    o.push_back((double)p.m_bClosedV);
    o.push_back((double)p.m_lPoles);
    o.push_back((double)p.m_eNotYetFaceType);
    o.push_back((double)p.m_sLoopProps.GetSize());
    for (ULONG i = 0; i < p.m_sLoopProps.GetSize(); i++)
    {
        const SmLoopProps& lp = p.m_sLoopProps[i];
        o.push_back((double)lp.m_lSeamCrossingCntU);
        o.push_back((double)lp.m_lSeamWindingCntU);
        o.push_back((double)lp.m_lSeamCrossingCntV);
        o.push_back((double)lp.m_lSeamWindingCntV);
    }
    put_classification(o, g, p.m_pCrvClassU);
    put_classification(o, g, p.m_pCrvClassV);
    put_touches(o, g, p.m_sDoneTouchListU);
    put_touches(o, g, p.m_sDoneTouchListV);
    // what the loop bisector and GetLoopCrossings read
    o.push_back((double)p.m_sZoneTol3d);
    o.push_back((double)p.m_lNoVertexPoles);
    o.push_back((double)p.m_eBadMissingSeam);
    o.push_back((double)p.m_sPolePoints.GetSize());
    for (ULONG i = 0; i < p.m_sPolePoints.GetSize(); i++)
    {
        o.push_back(p.m_sPolePoints[i].x);
        o.push_back(p.m_sPolePoints[i].y);
        o.push_back(p.m_sPolePoints[i].z);
    }
}

void put_loops(std::vector<double>& o, Graph& g, const SmTArray<SmLoop*>& l)
{
    o.push_back((double)l.GetSize());
    for (ULONG i = 0; i < l.GetSize(); i++)
        o.push_back((double)g.find(5, l[i]));
}

// The loop tree in pre-order (item, its child subtree, then its sibling subtree), each item
// `pole loop io partner_pole partner_loop partner_io child sibling` (io: index into the
// classify records, -2 outside; child, sibling: pre-order numbers, -1 none).
void put_tree(std::vector<double>& o, Graph& g, SmLooptreeItem* root, const SmTArray<SmClassifyLoopIO>& ios)
{
    std::vector<SmLooptreeItem*> order;
    std::map<const SmLooptreeItem*, int64_t> num;
    std::vector<SmLooptreeItem*> stack;
    if (root)
        stack.push_back(root);
    while (!stack.empty() && order.size() < 100000)
    {
        SmLooptreeItem* it = stack.back();
        stack.pop_back();
        num[it] = (int64_t)order.size();
        order.push_back(it);
        if (it->m_pSibling)
            stack.push_back(it->m_pSibling);
        if (it->m_pChild)
            stack.push_back(it->m_pChild);
    }
    auto io_index = [&](const SmClassifyLoopIO* p) -> double {
        if (p == nullptr)
            return -1.0;
        for (ULONG i = 0; i < ios.GetSize(); i++)
            if (&ios.GetAt(i) == p)
                return (double)i;
        return -2.0;
    };
    auto link = [&](const SmLooptreeItem* p) -> double { return p ? (double)num[p] : -1.0; };
    o.push_back((double)order.size());
    for (SmLooptreeItem* it : order)
    {
        o.push_back(flg(it->m_bPoleVertex));
        o.push_back((double)g.find(5, it->m_pLoop));
        o.push_back(io_index(it->m_pClassifyLoopIO));
        o.push_back(flg(it->m_bPartnerPoleVertex));
        o.push_back((double)g.find(5, it->m_pPartnerLoop));
        o.push_back(io_index(it->m_pPartnerClassifyLoopIO));
        o.push_back(link(it->m_pChild));
        o.push_back(link(it->m_pSibling));
    }
}

void run_op(int32_t op, Rd& rd, const char* path, std::vector<double>& o)
{
    Rd brep_rd = rd;
    SmStatus bst;
    Built b;
    b.brep = build(rd, path, bst);
    if (b.brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back((double)bst);
        return;
    }
    o.push_back(0.0);
    Graph g;
    walk(b.brep, g);
    Case c;
    setup(rd, brep_rd, path, g, c);
    if (op == OP_EXPORT)
    {
        // the graph, then the case block, then the case block's length
        export_graph(b.brep, g, o);
        size_t start = o.size();
        o.push_back(c.rejected ? 1.0 : 0.0);
        if (!c.rejected)
        {
            o.push_back((double)c.face_index);
            put_props(o, g, *c.props);
            put_loops(o, g, c.loops);
        }
        o.push_back((double)(o.size() - start));
        return;
    }
    o.push_back(c.rejected ? 1.0 : 0.0);
    if (c.rejected)
        return;
    // the out-params start from the values the runner's `initial_out` gives the Rust
    // side (values ClassifyLoops never leaves with status 0 or SM_ERR before its init),
    // so an early return compares what it leaves
    SmHealNotYetFaceType classifiable = SM_HNF_NO_LOOP_TREE_ROOT;
    SmTArray<SmClassifyLoopIO> ios;
    SmCrvDirOnFaceType start = SM_CD_LAMINA, stop = SM_CD_LAMINA;
    SmTArray<ULONG> no_area;
    no_area.Add(77);
    SmLooptreeItem* root = nullptr;
    SmCurveClassification scratch;
    SmStatus s = c.face->ClassifyLoops(c.loops, classifiable, ios, start, stop, no_area, root, scratch, *c.props);
    o.push_back(st(s));
    put_loops(o, g, c.loops);
    o.push_back((double)classifiable);
    o.push_back((double)c.props->m_eNotYetFaceType);
    o.push_back((double)ios.GetSize());
    for (ULONG i = 0; i < ios.GetSize(); i++)
    {
        const SmClassifyLoopIO& io = ios[i];
        o.push_back((double)g.find(5, io.m_pLoop));
        o.push_back((double)io.m_tLoopuseType);
        o.push_back((double)io.m_eOrient);
        o.push_back((double)io.m_bDegen_Loop);
        o.push_back((double)io.m_bArea_Loop);
        o.push_back((double)io.m_eContainmentType);
    }
    o.push_back((double)start);
    o.push_back((double)stop);
    o.push_back((double)no_area.GetSize());
    for (ULONG i = 0; i < no_area.GetSize(); i++)
        o.push_back((double)no_area[i]);
    put_tree(o, g, root, ios);
    o.push_back((double)scratch.GetSize());
    delete root;
}
} // namespace

SC_EXPORT int32_t sc_query_face_p4_eval(int32_t op, const char* path, const double* in, int64_t n_in, double* out,
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
