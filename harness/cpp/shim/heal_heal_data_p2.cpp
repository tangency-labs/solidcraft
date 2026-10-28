// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `heal.heal_data_p2`: SmEdgeProps, SmVertexProps, the
// SmProbArray class family and the SmHealData target management, tolerance,
// coincidence and degeneracy steps (source/SMLib/src/SmHealData.cpp,
// source/SMLib/src/SmEdgeProps.cpp, source/SMLib/src/SmVertexProps.cpp,
// source/SMLib/inc/SmHealProbArray.h).
//
// One entry point, sc_heal_heal_data_p2_eval(op, path, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/heal_heal_data_p2.rs, which
// validates every input. B-rep blocks are those of sc_topology_fixture.h (kinds 0..3)
// plus kind 4 (SmPrimitiveCreation::CreateBox, as prog_test
// my_test_squeeze_edge_tracks_removal) and kind 5 (a full SmSphere face, as prog_test
// my_test_closed_sphere_properties), kind 6 (an SmBrepConstructor face with one vertex
// loop on a sphere, plane or torus) and kind 7 (prog_test
// my_test_move_seam_refreshes_face_props' cone patch with a seam inside the trim). Surfaces and edge curves the shared exporters can
// carry as their exact analytic kind (planes, circles and ellipses) are kept; any other
// surface or edge curve is replaced by a plain B-spline copy (what the Rust import
// rebuilds, as the heal.gap shim does). Then the graph is walked (with the vertex
// lists) and exported, followed by each vertex's vertex use list.
#include "sc_topology_fixture.h"

#include <SmHashTable.h>
#include <SmHealData.h>
#include <SmPrimitiveCreation.h>
#include <SmPlane.h>
#include <SmSphere.h>
#include <SmTorus.h>
#include <SmBSplineSurface.h>
#include <cmath>
#include <SmBrepData.h>
#include <SmTrackTopologyChanges.h>

#include <algorithm>
#include <array>
#include <set>

namespace
{
enum P2Op : int32_t
{
    P2_EXPORT = 0,
    P2_PROB = 1,
    P2_HEAL = 2,
    P2_VLISTS = 3,
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

// Whether the shared graph exporter transports `c` as its exact analytic kind.
// Probe that exporter instead of duplicating its eligibility and round-trip rules,
// so normalise() keeps exactly the curves the Rust import rebuilds natively.
bool exact_curve(const SmCurve* c)
{
    if (c == nullptr || (c->GetType() != SmEllipse_TYPE && c->GetType() != SmCircle_TYPE))
        return false;
    std::vector<double> head;
    sc_exact_curve_export::put_export_curve(head, c, the_context(), put_curve);
    return !head.empty() && head.front() == -3.0;
}

// Match the shared export/import: preserve exact analytic surfaces and edge curves,
// and replace other surfaces and edge curves with plain B-splines, as the heal.gap
// shim does. Replaced originals are left unowned and leaked.
void normalise(Graph& g)
{
    for (SmFace* f : g.faces)
    {
        SmSurface* s = f->*FPeek::srf();
        if (s == nullptr || !s->IsKindOf(SmBSplineSurface_TYPE) || s->GetType() == SmBSplineSurface_TYPE ||
            s->GetType() == SmPlane_TYPE)
            continue;
        // Preserve every surface the shared export imports as its exact analytic kind.
        std::vector<double> head;
        if (sc_analytic_export::put_exact_head(head, s, the_context()))
            continue;
        SmBSplineSurface* b = (SmBSplineSurface*)s;
        gw_SURFACE* nurb = b->GetGwNurbPointer();
        if (nurb == nullptr)
            continue;
        SmBSplineSurface* nb = new (the_context()) SmBSplineSurface(sm_AllocateAndCopyNurbSurface(nurb), FALSE, NULL);
        nb->GetOutOfBoundsEnabled() = b->GetOutOfBoundsEnabled();
        nb->SetBSplineSurfaceForm(b->GetBSplineSurfaceForm());
        f->*FPeek::srf() = nb;
    }
    for (SmEdge* e : g.edges)
    {
        SmCurve* c = e->*EdgePeek::curve();
        // Keep circles/ellipses that the shared exporter can carry exactly; the
        // healer then sees the same native kind as the imported Rust B-rep.
        if (exact_curve(c))
            continue;
        SmBSplineCurve* nc = plain_curve(c);
        if (nc != nullptr)
            e->*EdgePeek::curve() = nc;
    }
}

// Computes the cached vertex/face, vertex/edge, edge/face and edge/CCW-edge gaps, as
// the fixture's `build` does for `init_gaps`, but over every vertex use (the walk
// includes the vertex lists).
void init_cached_gaps(SmBrep* brep)
{
    Graph g;
    walk(brep, g, true);
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

// Builds the B-rep of a block (kinds 0..3 by the fixture, 4 box, 5 sphere face, 6 one
// vertex loop face, 7 seam-crossing cone patch).
SmBrep* build_any(Rd& rd, const char* path, SmStatus& status)
{
    int64_t kind = (int64_t)rd.in[rd.at];
    if (kind != 4 && kind != 5 && kind != 6 && kind != 7)
        return build(rd, path, status);
    rd.idx();
    const SmContext& ctx = the_context();
    SmBrep* brep = new (ctx) SmBrep();
    status = SM_SUCCESS;
    if (kind == 4)
    {
        double dx = rd.num(), dy = rd.num(), dz = rd.num();
        SmPrimitiveCreation prim(brep->GetInfiniteRegion());
        status = prim.CreateBox(dx, dy, dz, SmAxis2Placement());
    }
    else if (kind == 7)
    {
        // prog_test my_test_move_seam_refreshes_face_props: a cone/cylinder patch face
        // whose surface is replaced by the full 0..360 patch with its seam rotated by
        // `seam` degrees about the axis, so the seam lies inside the trim.
        double r0 = rd.num(), r1 = rd.num(), height = rd.num(), a0 = rd.num(), a1 = rd.num(),
               seam = rd.num();
        bool quintic = rd.flag();
        SmNurbCircleParam param = quintic ? SM_CO_QUINTIC : SM_CO_QUADRATIC;
        SmBSplineSurface* patch = nullptr;
        SmFace* face = nullptr;
        status = SmBSplineSurface::CreateConePatch(ctx, SmAxis2Placement(), r0, r1, a0, a1, height, param, patch);
        if (status == SM_SUCCESS)
            status = brep->CreateFaceFromSurface(patch, patch->GetNaturalUVDomain(), face);
        else
            delete patch;
        if (status == SM_SUCCESS && face)
        {
            double t = seam * M_PI / 180.0;
            SmAxis2Placement frame;
            frame.SetCanonical(SmPoint3d(0, 0, 0), SmVector3d(cos(t), sin(t), 0), SmVector3d(-sin(t), cos(t), 0));
            SmBSplineSurface* full = nullptr;
            status = SmBSplineSurface::CreateConePatch(ctx, frame, r0, r1, 0.0, 360.0, height, param, full);
            if (status == SM_SUCCESS)
                status = brep->ReplaceSurface(face, full, FALSE, NULL, FALSE);
            else
                delete full;
            if (status == SM_SUCCESS)
                face->SetIsRectangularTrim(FALSE);
        }
    }
    else
    {
        double r = rd.num();
        bool swap = rd.flag();
        // kind 6 only: 0 sphere, 1 plane (domain [-r, r]^2), 2 torus (major r, minor r/4)
        int64_t surf = kind == 6 ? rd.idx() : 0;
        SmSurface* surface = nullptr;
        SmPoint3d vertex(0, 0, r);
        if (surf == 1)
        {
            SmPlane* plane = nullptr;
            status = SmPlane::CreateCanonical(ctx, SmAxis2Placement(), plane);
            surface = plane;
            vertex = SmPoint3d(0, 0, 0);
        }
        else if (surf == 2)
        {
            SmTorus* torus = nullptr;
            status = SmTorus::CreateCanonical(ctx, SmAxis2Placement(), r, 0.25 * r, torus, swap ? TRUE : FALSE);
            surface = torus;
            vertex = SmPoint3d(1.25 * r, 0, 0);
        }
        else
        {
            SmSphere* sphere = nullptr;
            status = SmSphere::CreateCanonical(ctx, SmAxis2Placement(), r, sphere);
            if (status == SM_SUCCESS && swap)
                status = sphere->SwapUV();
            surface = sphere;
        }
        if (status == SM_SUCCESS)
        {
            SmExtent2d domain = surf == 1 ? SmExtent2d(SmPoint2d(-r, -r), SmPoint2d(r, r))
                                          : surface->GetNaturalUVDomain();
            if (kind == 6)
            {
                delete brep;
                SmBrepConstructor builder;
                brep = builder.StartBrep(ctx);
                if (brep) brep->SetTolerance(1e-7, FALSE);
                if (!brep || !builder.StartRegion(TRUE, NULL)) { status = SM_ERR; }
                else
                {
                    SmShell *outer = NULL, *inner = NULL;
                    status = builder.StartShell(outer, inner);
                    if (status == SM_SUCCESS && !builder.StartFace(surf == 0 && swap ? SM_OT_OPPOSITE : SM_OT_SAME)) status = SM_ERR;
                    if (status == SM_SUCCESS) status = builder.SetFaceSurface(surface, domain);
                    if (status == SM_SUCCESS && !builder.StartAndEndSingleVertexLoop(vertex)) status = SM_ERR;
                    if (status == SM_SUCCESS) status = builder.EndFace();
                    if (status == SM_SUCCESS) status = builder.EndShell(TRUE);
                    if (status == SM_SUCCESS) status = builder.EndRegion();
                    if (status == SM_SUCCESS) status = builder.EndBrep();
                }
            }
            else
            {
                SmShell* shell = nullptr;
                SmFace* face = nullptr;
                status = brep->CreateFaceInRegionFromSurface(brep->GetInfiniteRegion(), surface,
                                                             domain, shell, face);
            }
        }
    }
    if (!brep) { status = SM_ERR; return nullptr; }
    brep->m_bValidateOnly = rd.bflag();
    rd.flag(); // init_gaps: done by prepare()
    if (status != SM_SUCCESS)
    {
        delete brep;
        return nullptr;
    }
    return brep;
}

// Builds, normalises and walks; nullptr with the status written to `o` on failure.
SmBrep* prepare(Rd& rd, const char* path, Graph& g, std::vector<double>& o)
{
    SmStatus status = SM_SUCCESS;
    SmBrep* brep = build_any(rd, path, status);
    if (brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(status));
        return nullptr;
    }
    {
        Graph g0;
        walk(brep, g0, true);
        normalise(g0);
    }
    // the block's last value is `init_gaps`
    if (rd.at > 0 && rd.in[rd.at - 1] != 0.0)
        init_cached_gaps(brep);
    walk(brep, g, true);
    return brep;
}

void put_vertex_lists(Graph& g, std::vector<double>& o)
{
    for (SmVertex* v : g.vertices)
    {
        std::vector<int64_t> l;
        for (SmTopology* t : list_of(v->GetList()))
            l.push_back(g.find(8, t));
        put_list(o, l);
    }
}

// The cached gap records of every edge use and vertex use (the export only has their
// IsInit flags): per edge use `init gap eu0 eu1 t0 t1` (end/CCW-start) and
// `init gap eu t u v` (edge/face), per vertex use `init gap vu t` (vertex/edge) and
// `init gap vu u v` (vertex/face).
void put_cached_gaps(Graph& g, std::vector<double>& o)
{
    for (SmEdgeuse* eu : g.edgeuses)
    {
        const SmEdgeEdgeGap& c = eu->*EuPeek::ccw_gap();
        o.push_back(flg(c.IsInit()));
        o.push_back(c.m_dGap3d);
        o.push_back((double)g.find(6, c.GetEdgeuse(0)));
        o.push_back((double)g.find(6, c.GetEdgeuse(1)));
        o.push_back(c.GetEdgeT(0));
        o.push_back(c.GetEdgeT(1));
        const SmEdgeFaceGap& f = eu->*EuPeek::face_gap();
        o.push_back(flg(f.IsInit()));
        o.push_back(f.m_dGap3d);
        o.push_back((double)g.find(6, f.GetEdgeuse(0)));
        o.push_back(f.GetEdgeT(0));
        SmPoint2d uv = f.GetFaceUV();
        o.push_back(uv.x);
        o.push_back(uv.y);
    }
    for (SmVertexuse* vu : g.vertexuses)
    {
        const SmVertexEdgeGap& e = vu->*VuPeek::edge_gap();
        o.push_back(flg(e.IsInit()));
        o.push_back(e.m_dGap3d);
        o.push_back((double)g.find(8, e.GetVertexuse()));
        o.push_back(e.GetEdgeT(0));
        const SmVertexFaceGap& f = vu->*VuPeek::face_gap();
        o.push_back(flg(f.IsInit()));
        o.push_back(f.m_dGap3d);
        o.push_back((double)g.find(8, f.GetVertexuse()));
        SmPoint2d uv = f.GetFaceUV();
        o.push_back(uv.x);
        o.push_back(uv.y);
    }
}

void run_export(Rd& rd, const char* path, std::vector<double>& o, bool lists)
{
    Graph g;
    SmBrep* brep = prepare(rd, path, g, o);
    if (brep == nullptr)
        return;
    o.push_back(0.0);
    if (lists)
    {
        put_vertex_lists(g, o);
        put_cached_gaps(g, o);
        // the infinite region (SmBrep::m_pInfiniteRegion), which the shared graph export
        // does not carry; Fix_InfiniteRegion walks the region list from it
        o.push_back((double)g.find(0, brep->GetInfiniteRegion()));
    }
    else
        export_graph(brep, g, o);
    delete brep;
}

// ---- standalone problem arrays over SmFaceProps -----------------------------------------

typedef ULONG SmFaceProps::*FaceFlagPtr;
const FaceFlagPtr FACE_FLAGS[] = {
    &SmFaceProps::m_bBadHasNotYetProb,    &SmFaceProps::m_bBadSmallZoneTol3d,     &SmFaceProps::m_bBadLargeZoneTol3d,
    &SmFaceProps::m_bBadDegenFace,        &SmFaceProps::m_lBadFlatCorners,        &SmFaceProps::m_bBadMissingSeam,
    &SmFaceProps::m_bBadCrossedSeam,      &SmFaceProps::m_bBadNearMissSeam,       &SmFaceProps::m_bBadLoopProblems,
    &SmFaceProps::m_bBadOuterLoopOrder,   &SmFaceProps::m_lBadOrient_LoopCnt,     &SmFaceProps::m_lBadNoArea_LoopCnt,
    &SmFaceProps::m_lOuterLoopCnt,        &SmFaceProps::m_lBadNested_LoopCnt,     &SmFaceProps::m_bBadUnpairedNoArea_Loop,
    &SmFaceProps::m_lBadClosed3d_LoopCnt, &SmFaceProps::m_lBadClosedPtr_LoopCnt,  &SmFaceProps::m_lBadMissingPoles,
    &SmFaceProps::m_bBadCrossedPole,
};
const int N_FACE_FLAGS = 19;
typedef SmMixedLinkType SmFaceProps::*FaceMixedPtr;
const FaceMixedPtr FACE_MIXED[] = {&SmFaceProps::m_eSheet, &SmFaceProps::m_ePoles};
typedef SmTriedLinkType SmFaceProps::*FaceTriedPtr;
const FaceTriedPtr FACE_TRIED[] = {&SmFaceProps::m_eBeenThroughMoveSeam, &SmFaceProps::m_eBeenThroughSplitEdgesAtSeam,
                                   &SmFaceProps::m_eBeenThroughSplitFacesAtSeam};

const char* const LABELS[3][3] = {
    {"Edge ZoneTol3d too small                ", "SmallTol Edge Cnt  ", " - Bad "},
    {"Sheet Face counts                       ", "                   ", ""},
    {"odd NoAreaLoopCnt on Face: no AddSeamFix", "Bad unpaired NoAreaLoop Face Cnt ", " - Warn "},
};

void put_str(std::vector<double>& o, const TCHAR* s)
{
    size_t n = 0;
    while (s[n] != 0)
        n++;
    o.push_back((double)n);
    for (size_t i = 0; i < n; i++)
        o.push_back((double)(unsigned char)s[i]);
}

template <class T> void put_base(std::vector<double>& o, const SmProbArray<T>& a, const std::map<const void*, int64_t>& ix)
{
    o.push_back((double)a.m_sProbArray.GetSize());
    for (ULONG i = 0; i < a.m_sProbArray.GetSize(); i++)
    {
        auto it = ix.find((const void*)a.m_sProbArray.GetAt(i));
        o.push_back(it == ix.end() ? -3.0 : (double)it->second);
    }
    o.push_back((double)a.m_lOrigProbCount);
    o.push_back((double)a.m_eGetHealerOp);
    put_str(o, a.m_sOutputLabel);
    put_str(o, a.m_sDataLabel);
    put_str(o, a.m_sProbLabel);
}

// Whether SmPairProbArray::Remove(tgt) would read a member flag without a link or past
// the end of the list (a simulation of its first loop on a copy).
template <class T> bool pair_remove_is_ub(const SmPairProbArray<T>& a, T* tgt)
{
    std::vector<T*> v;
    for (ULONG i = 0; i < a.m_sProbArray.GetSize(); i++)
        v.push_back(a.m_sProbArray.GetAt(i));
    ULONG lnk_null = a.m_plObjProbFlag == NULL;
    bool removed = false;
    for (size_t ii = 0; ii < v.size() + (removed ? 2 : 0); ii += 2)
    {
        if (removed)
        {
            ii -= 2;
            removed = false;
        }
        if (ii + 1 > v.size())
            break;
        if (lnk_null)
            return true;
        if (TRUE != v[ii]->*(a.m_plObjProbFlag))
            break;
        if (ii + 1 >= v.size())
            return true;
        if (TRUE != v[ii + 1]->*(a.m_plObjProbFlag))
            break;
        if (v[ii] == tgt || v[ii + 1] == tgt)
        {
            v.erase(v.begin() + (long)ii, v.begin() + (long)ii + 2);
            removed = true;
        }
    }
    return false;
}

void put_face_flags(std::vector<double>& o, SmFaceProps* p)
{
    for (int k = 0; k < N_FACE_FLAGS; k++)
        o.push_back((double)(p->*FACE_FLAGS[k]));
    o.push_back((double)p->m_eSheet);
    o.push_back((double)p->m_ePoles);
    o.push_back((double)p->m_eBeenThroughMoveSeam);
    o.push_back((double)p->m_eBeenThroughSplitEdgesAtSeam);
    o.push_back((double)p->m_eBeenThroughSplitFacesAtSeam);
}

// `kind n nops ops...`: kind 0 SmProbArray, 1 SmPairProbArray, 2 SmMixedProbArray,
// 3 SmTriedProbArray, each over `n` SmFaceProps.
void run_prob(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    int64_t kind = rd.idx();
    int64_t n = rd.idx();
    std::vector<SmFaceProps*> objs;
    std::map<const void*, int64_t> ix;
    for (int64_t i = 0; i < n; i++)
    {
        objs.push_back(new SmFaceProps(&ctx));
        ix[(const void*)objs.back()] = i;
    }
    SmProbArray<SmFaceProps> plain;
    SmPairProbArray<SmFaceProps> pair;
    SmMixedProbArray<SmFaceProps> mixed;
    SmTriedProbArray<SmFaceProps> tried;
    SmProbArray<SmFaceProps>& base = kind == 0 ? plain : kind == 1 ? (SmProbArray<SmFaceProps>&)pair
                                                 : kind == 2 ? (SmProbArray<SmFaceProps>&)mixed
                                                             : (SmProbArray<SmFaceProps>&)tried;
    int64_t nops = rd.idx();
    bool stop = false;
    for (int64_t s = 0; s < nops; s++)
    {
        int64_t code = rd.idx();
        int64_t a = rd.idx();
        int64_t b = rd.idx();
        int64_t c = rd.idx();
        if (stop)
            continue;
        o.push_back((double)code);
        switch (code)
        {
        case 0: // setup(op, link or -1)
            if (kind == 2)
                mixed.Setup((SmHealerOpType)a, b < 0 ? (FaceMixedPtr)NULL : FACE_MIXED[b]);
            else if (kind == 3)
                tried.Setup((SmHealerOpType)a, b < 0 ? (FaceTriedPtr)NULL : FACE_TRIED[b]);
            else
                base.Setup((SmHealerOpType)a, b < 0 ? (FaceFlagPtr)NULL : FACE_FLAGS[b]);
            o.push_back(0.0);
            break;
        case 1: // add(obj, arg)
        {
            SmFaceProps* p = objs[(size_t)a];
            ULONG r = kind == 2 ? mixed.Add(p, (SmMixedLinkType)b)
                      : kind == 3 ? tried.Add(p, (SmTriedLinkType)b)
                                  : base.Add(p, b != 0 ? TRUE : FALSE);
            o.push_back((double)r);
            break;
        }
        case 2: // remove(obj)
        {
            SmFaceProps* p = objs[(size_t)a];
            if (kind == 1 && pair_remove_is_ub(pair, p))
            {
                o.push_back(ST_NULL_DEREF);
                stop = true;
                break;
            }
            SmBoolean r = kind == 1 ? pair.Remove(p) : base.Remove(p);
            o.push_back(flg(r));
            break;
        }
        case 3: base.RemoveAll(); o.push_back(0.0); break;
        case 4:
            if (kind == 1)
                pair.ReSet();
            else if (kind == 2)
                mixed.ReSet();
            else
                base.ReSet();
            o.push_back(0.0);
            break;
        case 5: o.push_back((double)base.IncOrigProbCount()); break;
        case 6: // set member flag b of obj a to c
            objs[(size_t)a]->*FACE_FLAGS[b] = (ULONG)c;
            o.push_back(0.0);
            break;
        case 7: base.SetLabels(LABELS[a][0], LABELS[a][1], LABELS[a][2]); o.push_back(0.0); break;
        case 8: // GetAt(a mod size), -1 when empty
        {
            ULONG sz = base.GetSize();
            if (sz == 0)
                o.push_back(-1.0);
            else
                o.push_back((double)ix[(const void*)base.GetAt((ULONG)a % sz)]);
            break;
        }
        case 9:
            if (kind == 1)
                pair.SetOrigCoinPairCnt((ULONG)a);
            else
                base.SetOrigProbCnt((ULONG)a);
            o.push_back(0.0);
            break;
        default: o.push_back((double)SC_ERR_ARG); break;
        }
    }
    if (!stop)
    {
        o.push_back(-100.0);
        put_base(o, base, ix);
        o.push_back(kind == 1 ? (double)pair.m_lOrigCoinPairCnt : 0.0);
        o.push_back(base.GetOrigProbCount());
        for (SmFaceProps* p : objs)
            put_face_flags(o, p);
    }
    for (SmFaceProps* p : objs)
        delete p;
}

// ---- the healer ------------------------------------------------------------------------

struct Registry
{
    std::vector<SmVertexProps*> v;
    std::vector<SmEdgeProps*> e;
    std::vector<SmFaceProps*> f;
    std::set<const void*> dead;
    std::map<const void*, int64_t> iv, ie, iff;

    void note(SmHealData& h)
    {
        for (ULONG i = 0; i < h.m_sTgtVertexProps.GetSize(); i++)
            if (iv.find(h.m_sTgtVertexProps[i]) == iv.end() || dead.count(h.m_sTgtVertexProps[i]))
            {
                dead.erase(h.m_sTgtVertexProps[i]);
                iv[h.m_sTgtVertexProps[i]] = (int64_t)v.size();
                v.push_back(h.m_sTgtVertexProps[i]);
            }
        for (ULONG i = 0; i < h.m_sTgtEdgeProps.GetSize(); i++)
            if (ie.find(h.m_sTgtEdgeProps[i]) == ie.end() || dead.count(h.m_sTgtEdgeProps[i]))
            {
                dead.erase(h.m_sTgtEdgeProps[i]);
                ie[h.m_sTgtEdgeProps[i]] = (int64_t)e.size();
                e.push_back(h.m_sTgtEdgeProps[i]);
            }
        for (ULONG i = 0; i < h.m_sTgtFaceProps.GetSize(); i++)
            if (iff.find(h.m_sTgtFaceProps[i]) == iff.end() || dead.count(h.m_sTgtFaceProps[i]))
            {
                dead.erase(h.m_sTgtFaceProps[i]);
                iff[h.m_sTgtFaceProps[i]] = (int64_t)f.size();
                f.push_back(h.m_sTgtFaceProps[i]);
            }
    }
    bool alive(const void* p) const { return dead.find(p) == dead.end(); }
    template <class T> bool alive_at(T* p, size_t i, const std::map<const void*, int64_t>& ids) const
    {
        auto it = ids.find(p);
        return alive(p) && it != ids.end() && it->second == (int64_t)i;
    }
    template <class T> T* pick(std::vector<T*>& all, const std::map<const void*, int64_t>& ids, int64_t sel)
    {
        std::vector<T*> live;
        for (size_t i = 0; i < all.size(); ++i)
            if (alive_at(all[i], i, ids))
                live.push_back(all[i]);
        if (live.empty())
            return nullptr;
        return live[(size_t)(sel % (int64_t)live.size())];
    }
};

template <class T> int64_t idx_of(const std::map<const void*, int64_t>& m, const T* p)
{
    if (p == nullptr)
        return -1;
    auto it = m.find((const void*)p);
    return it == m.end() ? -3 : it->second;
}

struct EdgeuseGapPeek : SmEdgeuse
{
    static SmEdgeFaceGap SmEdgeuse::*face_gap() { return &EdgeuseGapPeek::m_sMaxEdgeFaceGap3d; }
    static SmEdgeEdgeGap SmEdgeuse::*ccw_gap() { return &EdgeuseGapPeek::m_sEdgeCCWEdgeGap3d; }
};
struct VertexuseGapPeek : SmVertexuse
{
    static SmVertexFaceGap SmVertexuse::*face_gap() { return &VertexuseGapPeek::m_sVertexFaceGap3d; }
    static SmVertexEdgeGap SmVertexuse::*edge_gap() { return &VertexuseGapPeek::m_sVertexEdgeGap3d; }
};

struct VxPeek : SmObjsInVoxels<SmVertexProps*>
{
    static SmExtent3d SmObjsInVoxels<SmVertexProps*>::*box() { return &VxPeek::m_sBBox; }
};

// An add naming a live target that the same step does not remove gives that entity a
// second props record (upstream adds only topology it has just created). A catch-up
// that can delete topology then frees the entity through one record and dereferences
// it through the other (fuzz: SmEdge::GetVertices in Fix_DegenEdges).
bool readds_live_target(SmHealData& h, SmBrep* brep, SmTArray<SmTopology*>& removed,
                        SmTArray<SmTopology*>& added)
{
    for (ULONG i = 0; i < added.GetSize(); ++i)
    {
        SmTopology* t = added[i];
        ULONG idx;
        if (t == NULL || !brep->IsLiveTopologyMember(t) || removed.FindElement(t, idx))
            continue;
        if ((t->IsKindOf(SmFace_TYPE) && h.m_sTgtFaces.FindElement((SmFace*)t, idx)) ||
            (t->IsKindOf(SmEdge_TYPE) && h.m_sTgtEdges.FindElement((SmEdge*)t, idx)) ||
            (t->IsKindOf(SmVertex_TYPE) && h.m_sTgtVertices.FindElement((SmVertex*)t, idx)))
            return true;
    }
    return false;
}

// A seed step that would queue a face props record a second time (upstream queues each
// once; the repair then works on the record twice).
bool degen_face_seeded(SmHealData& h)
{
    for (ULONG i = 0; i < h.m_sTgtFaceProps.GetSize(); ++i)
    {
        ULONG idx;
        if (h.m_sBadFaceProps_DegenFaces.m_sProbArray.FindElement(h.m_sTgtFaceProps[i], idx))
            return true;
    }
    return false;
}

// Get*Props_ForTgts indexes props by the matching target's position.
template<class T, class P>
bool removal_lookup_is_ub(const SmTArray<T*>& targets, const SmTArray<P*>& props,
                          const SmTArray<SmTopology*>& removed)
{
    for (ULONG i = 0; i < removed.GetSize(); ++i)
        for (ULONG j = 0; j < targets.GetSize(); ++j)
            if ((SmTopology*)targets[j] == removed[i])
            {
                if (j >= props.GetSize()) return true;
                break;
            }
    return false;
}

// Remove*Props removes a target only when its props already names that target.
template<class T, class P, class F>
size_t target_count_after_removals(const SmTArray<T*>& targets, const SmTArray<P*>& removed, F object)
{
    std::vector<T*> remaining;
    for (ULONG i = 0; i < targets.GetSize(); ++i) remaining.push_back(targets[i]);
    for (ULONG i = 0; i < removed.GetSize(); ++i)
    {
        auto it = std::find(remaining.begin(), remaining.end(), object(removed[i]));
        if (it != remaining.end()) remaining.erase(it);
    }
    return remaining.size();
}

// The edge use owning a gap member (traversal index), -3 when none does.
int64_t eu_of_gap(Graph& g, const void* gap, bool ccw)
{
    if (gap == nullptr)
        return -1;
    for (size_t i = 0; i < g.edgeuses.size(); i++)
    {
        SmEdgeuse* eu = g.edgeuses[i];
        const void* m = ccw ? (const void*)&(eu->*EdgeuseGapPeek::ccw_gap()) : (const void*)&(eu->*EdgeuseGapPeek::face_gap());
        if (m == gap)
            return (int64_t)i;
    }
    return -3;
}
int64_t vu_of_gap(Graph& g, const void* gap, bool edge)
{
    if (gap == nullptr)
        return -1;
    for (size_t i = 0; i < g.vertexuses.size(); i++)
    {
        SmVertexuse* vu = g.vertexuses[i];
        const void* m =
            edge ? (const void*)&(vu->*VertexuseGapPeek::edge_gap()) : (const void*)&(vu->*VertexuseGapPeek::face_gap());
        if (m == gap)
            return (int64_t)i;
    }
    return -3;
}

void read_targets(Rd& rd, Graph& g, SmTArray<SmVertex*>& vs, SmTArray<SmEdge*>& es, SmTArray<SmFace*>& fs)
{
    int64_t n = rd.idx();
    for (int64_t i = 0; i < n; i++)
    {
        int64_t s = rd.idx();
        if (!g.vertices.empty())
            vs.Add(g.vertices[(size_t)(s % (int64_t)g.vertices.size())]);
    }
    n = rd.idx();
    for (int64_t i = 0; i < n; i++)
    {
        int64_t s = rd.idx();
        if (!g.edges.empty())
            es.Add(g.edges[(size_t)(s % (int64_t)g.edges.size())]);
    }
    n = rd.idx();
    for (int64_t i = 0; i < n; i++)
    {
        int64_t s = rd.idx();
        if (!g.faces.empty())
            fs.Add(g.faces[(size_t)(s % (int64_t)g.faces.size())]);
    }
}

template <class T> void put_targets(std::vector<double>& o, Graph& g, int kind, const SmTArray<T*>& a)
{
    o.push_back((double)a.GetSize());
    for (ULONG i = 0; i < a.GetSize(); i++)
        o.push_back((double)g.find(kind, a.GetAt(i)));
}
template <class T> void put_props_list(std::vector<double>& o, const std::map<const void*, int64_t>& m, const SmTArray<T*>& a)
{
    o.push_back((double)a.GetSize());
    for (ULONG i = 0; i < a.GetSize(); i++)
        o.push_back((double)idx_of(m, a.GetAt(i)));
}

void put_ext1(std::vector<double>& o, const SmExtent1d& e)
{
    o.push_back(e.GetMin());
    o.push_back(e.GetMax());
}

// After a successful repair step (which may have edited the fixture graph): a summary
// that reads only the live B-rep and the healer, not the fixture graph. `-101`, the
// target and new-object list sizes, the repair problem list sizes, the B-rep's face,
// edge and vertex counts, per face (sorted) its loop and edge counts, and the vertex
// points in lexicographic order.
void put_repair_summary(SmBrep* brep, SmHealData& h, std::vector<double>& o)
{
    o.push_back(-101.0);
    for (ULONG n : {h.m_sTgtVertices.GetSize(), h.m_sTgtEdges.GetSize(), h.m_sTgtFaces.GetSize(),
                    h.m_sNewFaces_FromFixDegenFaces.GetSize(), h.m_sNewEdges_FromSplitEdgesAtSeams.GetSize(),
                    h.m_sNewVertices_FromSplitEdgesAtSeams.GetSize(), h.m_sNewFaces_FromSplitFaceAtSeams.GetSize(),
                    h.m_sNewEdgesOnSeams_FromSplitFaceAtSeams.GetSize(),
                    h.m_sNewVertices_FromSplitFaceAtSeams.GetSize(), h.m_sBadFaceProps_DegenFaces.GetSize(),
                    h.m_sBadFaceProps_CrossedSeam.GetSize(), h.m_sBadFaceProps_MissingSeam.GetSize(),
                    h.m_sBadFaceProps_NearMissSeam.GetSize(), h.m_sRanFaceProps_MoveSeam.GetSize(),
                    h.m_sRanFaceProps_SplitEdgesAtSeam.GetSize()})
        o.push_back((double)n);
    SmTArray<SmFace*> faces;
    SmTArray<SmEdge*> edges;
    SmTArray<SmVertex*> vertices;
    brep->GetFaces(faces);
    brep->GetEdges(edges);
    brep->GetVertices(vertices);
    o.push_back((double)faces.GetSize());
    o.push_back((double)edges.GetSize());
    o.push_back((double)vertices.GetSize());
    std::vector<std::pair<ULONG, ULONG>> per_face;
    for (ULONG i = 0; i < faces.GetSize(); ++i)
    {
        SmTArray<SmLoop*> loops;
        SmTArray<SmEdge*> face_edges;
        faces[i]->GetLoops(loops);
        faces[i]->GetEdges(face_edges);
        per_face.emplace_back(loops.GetSize(), face_edges.GetSize());
    }
    std::sort(per_face.begin(), per_face.end());
    for (const auto& f : per_face)
    {
        o.push_back((double)f.first);
        o.push_back((double)f.second);
    }
    std::vector<std::array<double, 3>> pts;
    for (ULONG i = 0; i < vertices.GetSize(); ++i)
    {
        const SmPoint3d& p = vertices[i]->GetPoint();
        pts.push_back({p.x, p.y, p.z});
    }
    std::sort(pts.begin(), pts.end());
    for (const auto& p : pts)
        o.insert(o.end(), p.begin(), p.end());
}

void dump(SmHealData& h, Graph& g, Registry& r, std::vector<double>& o)
{
    o.push_back(-100.0);
    put_targets(o, g, 9, h.m_sTgtVertices);
    put_targets(o, g, 7, h.m_sTgtEdges);
    put_targets(o, g, 3, h.m_sTgtFaces);
    put_props_list(o, r.iv, h.m_sTgtVertexProps);
    put_props_list(o, r.ie, h.m_sTgtEdgeProps);
    put_props_list(o, r.iff, h.m_sTgtFaceProps);
    o.push_back((double)h.m_lOldVertexCnt);
    o.push_back((double)h.m_lOldEdgeCnt);
    o.push_back((double)h.m_lOldFaceCnt);
    o.push_back((double)h.m_eStopHealOp);
    o.push_back((double)h.m_eDoneHealOp);
    o.push_back((double)h.m_lRecursionCount);

    o.push_back((double)r.v.size());
    for (size_t ri = 0; ri < r.v.size(); ++ri)
    {
        SmVertexProps* p = r.v[ri];
        if (!r.alive_at(p, ri, r.iv))
        {
            o.push_back(0.0);
            continue;
        }
        o.push_back(1.0);
        o.push_back((double)g.find(9, p->m_pVertex));
        o.push_back((double)p->m_lVertexIndx);
        o.push_back((double)p->m_sZoneTol3d);
        o.push_back((double)p->m_sOrigZoneTol3d);
        o.push_back((double)vu_of_gap(g, p->m_pMaxGap3d_VtxEdge, true));
        o.push_back(p->m_pMaxGap3d_VtxEdge ? p->m_pMaxGap3d_VtxEdge->GetLength() : 0.0);
        o.push_back((double)vu_of_gap(g, p->m_pMaxGap3d_VtxFace, false));
        o.push_back(p->m_pMaxGap3d_VtxFace ? p->m_pMaxGap3d_VtxFace->GetLength() : 0.0);
        o.push_back((double)p->m_bBadSmallZoneTol3d);
        o.push_back((double)p->m_bBadLargeZoneTol3d);
        o.push_back((double)p->m_bWarnLargeGap3d);
        o.push_back((double)p->m_bBadCoincidentVertex);
        o.push_back((double)p->m_bBadBackPointer);
        o.push_back(flg(p->IsSet()));
        o.push_back(flg(p->HasProblems()));
    }
    o.push_back((double)r.e.size());
    for (size_t ri = 0; ri < r.e.size(); ++ri)
    {
        SmEdgeProps* p = r.e[ri];
        if (!r.alive_at(p, ri, r.ie))
        {
            o.push_back(0.0);
            continue;
        }
        o.push_back(1.0);
        o.push_back((double)g.find(7, p->m_pEdge));
        o.push_back((double)p->m_lEdgeIndx);
        put_ext1(o, p->m_sEdgeInterval);
        o.push_back((double)p->m_sZoneTol3d);
        o.push_back((double)p->m_sOrigZoneTol3d);
        o.push_back(p->m_dApproxEdgeLength3d);
        o.push_back(p->m_cpCurve ? 1.0 : 0.0);
        put_ext1(o, p->m_sNaturalInterval);
        o.push_back((double)eu_of_gap(g, p->m_pMaxGap3d_EdgeEdge, true));
        o.push_back(p->m_pMaxGap3d_EdgeEdge ? p->m_pMaxGap3d_EdgeEdge->GetLength() : 0.0);
        o.push_back((double)eu_of_gap(g, p->m_pMaxGap3d_EdgeFace, false));
        o.push_back(p->m_pMaxGap3d_EdgeFace ? p->m_pMaxGap3d_EdgeFace->GetLength() : 0.0);
        o.push_back((double)p->m_bBadSmallZoneTol3d);
        o.push_back((double)p->m_bBadLargeZoneTol3d);
        o.push_back((double)p->m_bWarnLargeGap3d);
        o.push_back((double)p->m_bBadDegenEdge);
        o.push_back((double)p->m_bBadUncontainedEdge);
        o.push_back((double)p->m_bBadCoincidentEdge);
        o.push_back((double)p->m_bBadMissedEdgeXSect);
        o.push_back((double)p->m_bBadBackPointer);
        o.push_back(flg(p->IsSet_Metadata()));
        o.push_back(flg(p->IsSet_Gaps()));
        o.push_back(flg(p->HasProblems()));
    }
    o.push_back((double)r.f.size());
    for (size_t ri = 0; ri < r.f.size(); ++ri)
    {
        SmFaceProps* p = r.f[ri];
        if (!r.alive_at(p, ri, r.iff))
        {
            o.push_back(0.0);
            continue;
        }
        o.push_back(1.0);
        o.push_back((double)g.find(3, p->m_pFace));
        o.push_back((double)p->m_lFaceIndx);
        o.push_back((double)p->m_sZoneTol3d);
        o.push_back((double)p->m_sOrigZoneTol3d);
        o.push_back((double)p->m_ePropStage);
        o.push_back((double)p->m_bBadBackPointer);
        put_face_flags(o, p);
    }

    // problem arrays
    put_base(o, h.m_sNotYetFaceProps, r.iff);
    {
        // back pointers: a mixed list of the three kinds
        const SmProbArray<SmObjProps>& a = h.m_sBadObjProps_BackPointers;
        o.push_back((double)a.m_sProbArray.GetSize());
        o.push_back((double)a.m_lOrigProbCount);
        o.push_back((double)a.m_eGetHealerOp);
        put_str(o, a.m_sOutputLabel);
        put_str(o, a.m_sDataLabel);
        put_str(o, a.m_sProbLabel);
    }
    put_base(o, h.m_sBadEdgeProps_Uncontained, r.ie);
    put_base(o, h.m_sBadEdgeProps_SmallZoneTol3d, r.ie);
    put_base(o, h.m_sBadEdgeProps_LargeZoneTol3d, r.ie);
    put_base(o, h.m_sWarnEdgeProps_LargeGap3d, r.ie);
    put_base(o, h.m_sBadEdgeProps_DegenEdges, r.ie);
    put_base(o, h.m_sBadVertexProps_SmallZoneTol3d, r.iv);
    put_base(o, h.m_sBadVertexProps_LargeZoneTol3d, r.iv);
    put_base(o, h.m_sWarnVertexProps_LargeGap3d, r.iv);
    put_base(o, h.m_sBadFaceProps_SmallZoneTol3d, r.iff);
    put_base(o, h.m_sBadFaceProps_LargeZoneTol3d, r.iff);
    put_base(o, h.m_sBadVertexProps_CoinVertices, r.iv);
    o.push_back((double)h.m_sBadVertexProps_CoinVertices.m_lOrigCoinPairCnt);
    put_base(o, h.m_sBadFaceProps_DegenFaces, r.iff);
    put_base(o, h.m_sBadEdgeProps_CoinEdges, r.ie);
    o.push_back((double)h.m_sBadEdgeProps_CoinEdges.m_lOrigCoinPairCnt);
    put_base(o, h.m_sBadEdgeProps_MissedEdgeXSects, r.ie);
    o.push_back((double)h.m_sBadEdgeProps_MissedEdgeXSects.m_lOrigCoinPairCnt);
    put_base(o, h.m_sFaceProps_Sheets, r.iff);
    put_base(o, h.m_sFaceProps_NoVertexPole, r.iff);
    put_base(o, h.m_sBadFaceProps_FlatCorner, r.iff);
    put_base(o, h.m_sBadFaceProps_MissingSeam, r.iff);
    put_base(o, h.m_sBadFaceProps_CrossedPole, r.iff);
    put_base(o, h.m_sBadFaceProps_CrossedSeam, r.iff);
    put_base(o, h.m_sBadFaceProps_NearMissSeam, r.iff);
    put_base(o, h.m_sRanFaceProps_MoveSeam, r.iff);
    put_base(o, h.m_sRanFaceProps_SplitEdgesAtSeam, r.iff);
    put_base(o, h.m_sBadFaceProps_LoopProblems, r.iff);
    put_base(o, h.m_sBadFaceProps_OuterLoopOrder, r.iff);
    put_base(o, h.m_sBadFaceProps_OrientLoop, r.iff);
    put_base(o, h.m_sBadFaceProps_NoArea_Loop, r.iff);
    put_base(o, h.m_sBadFaceProps_MultiOuterLoops, r.iff);
    put_base(o, h.m_sBadFaceProps_NestedOuterLoops, r.iff);
    put_base(o, h.m_sBadFaceProps_UnpairedNoAreaLoop, r.iff);
    put_base(o, h.m_sBadFaceProps_Closed3dLoop, r.iff);
    put_base(o, h.m_sBadFaceProps_ClosedPtrLoop, r.iff);
    put_base(o, h.m_sBadFaceProps_MissingPole, r.iff);
    put_base(o, h.m_sRanFaceProps_SplitFaceAtSeam, r.iff);
    o.push_back((double)h.m_lRanRegion_TestClosure_Cnt);
    o.push_back((double)h.m_lFixRegion_SplitRegion_Cnt);

    // histograms and statistics
    for (const SmTArray<ULONG>* a : {&h.m_sLoopGap3d_Histogram, &h.m_sEdgeFaceGap3d_Histogram, &h.m_sEdgeLen3d_Histogram,
                                     &h.m_sVertEdgeGap3d_Histogram, &h.m_sVertFaceGap3d_Histogram,
                                     &h.m_sVertVertGap3d_Histogram})
    {
        o.push_back((double)a->GetSize());
        for (ULONG i = 0; i < a->GetSize(); i++)
            o.push_back((double)a->GetAt(i));
    }
    o.push_back(h.m_dMaxEdgeLength);
    o.push_back(h.m_dAvgEdgeLength);
    o.push_back(h.m_dNonDegen_MinEdgeLength);
    o.push_back((double)h.m_lNonDegenEdgeCnt);

    // vertex voxel model
    o.push_back((double)h.m_lXCnt);
    o.push_back((double)h.m_lYCnt);
    o.push_back((double)h.m_lZCnt);
    put_vec(o, h.m_sVertexBBox.GetMin());
    put_vec(o, h.m_sVertexBBox.GetMax());
    SmTArray<SmPointItem<SmVertexProps*>>& items = h.m_pVerticesInVoxels->GetPointItems();
    o.push_back((double)items.GetSize());
    for (ULONG i = 0; i < items.GetSize(); i++)
    {
        o.push_back((double)idx_of(r.iv, items[i].m_sTgtItem));
        put_vec(o, items[i].m_sTgtPoint3d);
        o.push_back((double)items[i].m_sTgtZoneTol3d);
    }
}

// A topology entry of a GetXProps_ForTgts list: kind (graph order) and index.
SmTopology* topo_of(Graph& g, int64_t kind, int64_t i)
{
    auto at = [i](auto& v) -> SmTopology* {
        return v.empty() ? nullptr : (SmTopology*)v[(size_t)(i % (int64_t)v.size())];
    };
    switch (kind)
    {
    case 0: return at(g.regions);
    case 1: return at(g.shells);
    case 2: return at(g.faceuses);
    case 3: return at(g.faces);
    case 4: return at(g.loopuses);
    case 5: return at(g.loops);
    case 6: return at(g.edgeuses);
    case 7: return at(g.edges);
    case 8: return at(g.vertexuses);
    default: return at(g.vertices);
    }
}

// Whether a cache step would read a target topology list past its end.
template <class P, class T, class Pred> bool lists_drift(const SmTArray<P*>& props, const SmTArray<T*>& tgts, ULONG start, Pred needs)
{
    for (ULONG ii = start; ii < props.GetSize(); ii++)
        if (needs(props.GetAt(ii)) && ii >= tgts.GetSize())
            return true;
    return false;
}

// ---- steps of unit heal.heal_data_p1 ---------------------------------------------------
//
// 23 flag            ReSet(flag) (last step only)
// 24 sel             MoveToNotYetProblems(alive face props sel), outputs its index (-3 none)
// 25                 Cache_FaceProps_Stage2
// 26                 Cache_FaceProps_Stage3
// 27                 Fix_BadSheets (with a fresh SmNewMarkAndLock of the B-rep's context)
// 28 flag            Make_UVTrimCurves(flag)
// 29                 Fix_InfiniteRegion (last step only)
// 30 mode op flag .. HealBrep(mode 0: NULL B-rep; 1: every target; 2: `targets`
//                    follow), op, bMakeUVTrimCurves; modes 1, 2 last step only, no state
// 31                 the stage 2/3 snapshot of every registered face props record

enum P1Kind
{
    P1_STATUS = 0,  // `status new_probs` follows (the case stops on a failure)
    P1_REFUSED = 1, // `-2` written, the case stops
    P1_WRITTEN = 2, // outputs written, no status
    P1_LAST = 3,    // `status new_probs`, then the case stops without the state
};

struct P1Out
{
    P1Kind kind;
    SmStatus stat;
    SmBoolean bNew;
};

void mark_all_dead(SmHealData& h, Registry& r)
{
    for (ULONG i = 0; i < h.m_sTgtVertexProps.GetSize(); i++)
        r.dead.insert(h.m_sTgtVertexProps[i]);
    for (ULONG i = 0; i < h.m_sTgtEdgeProps.GetSize(); i++)
        r.dead.insert(h.m_sTgtEdgeProps[i]);
    for (ULONG i = 0; i < h.m_sTgtFaceProps.GetSize(); i++)
        r.dead.insert(h.m_sTgtFaceProps[i]);
    for (ULONG i = 0; i < h.m_sNotYetFaceProps.GetSize(); i++)
        r.dead.insert(h.m_sNotYetFaceProps[i]);
}

// Whether a Cache_FaceProps_Stage2/3 or Make_UVTrimCurves step would use the null
// face of a props record (only before Cache_FaceProps_Gaps); the Rust port refuses it.
bool step_meets_null_face(SmHealData& h, SmPropStageType stage, bool any)
{
    ULONG n = h.m_sTgtFaceProps.GetSize();
    if (n == 0 || n == h.m_lOldFaceCnt)
        return false;
    for (ULONG ii = h.m_lOldFaceCnt; ii < n; ii++)
    {
        SmFaceProps* p = h.m_sTgtFaceProps[ii];
        if (p->m_pFace == NULL && (any || !p->HasProps(stage, p->m_pFace)))
            return true;
    }
    return false;
}

void put_face_snapshot(std::vector<double>& o, Graph& g, SmFaceProps& p)
{
    o.push_back((double)g.find(3, p.m_pFace));
    o.push_back((double)p.m_lFaceIndx);
    o.push_back((double)p.m_ePropStage);
    o.push_back((double)p.m_eNotYetFaceType);
    o.push_back(p.m_cpSurface != NULL ? 1.0 : 0.0);
    o.push_back(p.m_sNaturalUVDomain.GetUMin());
    o.push_back(p.m_sNaturalUVDomain.GetVMin());
    o.push_back(p.m_sNaturalUVDomain.GetUMax());
    o.push_back(p.m_sNaturalUVDomain.GetVMax());
    o.push_back((double)p.m_bClosedU);
    o.push_back((double)p.m_bClosedV);
    o.push_back((double)p.m_bClosedSurf);
    o.push_back((double)p.m_eClosedValU);
    o.push_back((double)p.m_eClosedValV);
    o.push_back((double)p.m_lPoles);
    o.push_back((double)p.m_lApproxPoles);
    o.push_back((double)p.m_lNoVertexPoles);
    o.push_back((double)p.m_sPolePoints.GetSize());
    for (ULONG i = 0; i < p.m_sPolePoints.GetSize(); i++)
        put_vec(o, p.m_sPolePoints[i]);
    o.push_back((double)p.m_sPoleNormals.GetSize());
    for (ULONG i = 0; i < p.m_sPoleNormals.GetSize(); i++)
        put_vec(o, p.m_sPoleNormals[i]);
    o.push_back((double)p.m_sFlatCornerPoints.GetSize());
    for (ULONG i = 0; i < p.m_sFlatCornerPoints.GetSize(); i++)
        put_vec(o, p.m_sFlatCornerPoints[i]);
    o.push_back(p.m_pCrvClassU != NULL ? 1.0 : 0.0);
    o.push_back(p.m_pCrvClassV != NULL ? 1.0 : 0.0);
    o.push_back((double)p.m_eBadCrossedSeam);
    o.push_back((double)p.m_eBadMissingSeam);
    o.push_back((double)p.m_eBadNearMissSeam);
    o.push_back((double)p.m_lClosedLoopCnt);
    o.push_back((double)p.m_sLoopProps.GetSize());
}

P1Out p1_step(int64_t code, Rd& rd, SmHealData& h, SmBrep* brep, Graph& g, Registry& r, std::vector<double>& o)
{
    P1Out res{P1_STATUS, SM_SUCCESS, FALSE};
    switch (code)
    {
    case 23:
    {
        SmBoolean flag = rd.bflag();
        mark_all_dead(h, r);
        h.ReSet(flag);
        break;
    }
    case 24:
    {
        int64_t sel = rd.idx();
        SmFaceProps* p = r.pick(r.f, r.iff, sel);
        res.kind = P1_WRITTEN;
        if (p == nullptr)
        {
            o.push_back(-3.0);
            break;
        }
        h.MoveToNotYetProblems(p);
        o.push_back((double)idx_of(r.iff, p));
        break;
    }
    case 25:
    case 26:
    {
        SmPropStageType stage = code == 25 ? SM_PROPSTAGE_2 : SM_PROPSTAGE_3;
        if (step_meets_null_face(h, stage, false))
        {
            o.push_back(ST_NULL_DEREF);
            res.kind = P1_REFUSED;
            break;
        }
        res.stat = code == 25 ? h.Cache_FaceProps_Stage2(brep, &res.bNew, SM_HO_CACHE_FACEPROPS_2)
                              : h.Cache_FaceProps_Stage3(brep, &res.bNew, SM_HO_CACHE_FACEPROPS_3);
        break;
    }
    case 27:
    {
        for (ULONG i = 0; i < h.m_sFaceProps_Sheets.GetSize(); i++)
            if (h.m_sFaceProps_Sheets[i]->m_pFace == NULL)
            {
                o.push_back(ST_NULL_DEREF);
                res.kind = P1_REFUSED;
                return res;
            }
        SmNewMarkAndLock lock(brep->GetContext());
        res.stat = h.Fix_BadSheets(brep, lock, SM_HO_FIX_BADSHEETS);
        break;
    }
    case 28:
    {
        SmBoolean flag = rd.bflag();
        if (flag && step_meets_null_face(h, SM_PROPSTAGE_GAPS, true))
        {
            o.push_back(ST_NULL_DEREF);
            res.kind = P1_REFUSED;
            break;
        }
        res.stat = h.Make_UVTrimCurves(brep, flag, SM_HO_MAKE_UVTRIMCURVES);
        break;
    }
    case 29: res.stat = h.Fix_InfiniteRegion(brep, SM_HO_FIX_INFINITE_REGIONS); break;
    case 30:
    {
        int64_t mode = rd.idx();
        SmHealerOpType op = (SmHealerOpType)rd.idx();
        SmBoolean make = rd.bflag();
        if (mode == 0)
        {
            res.stat = h.HealBrep(NULL, op, NULL, NULL, NULL, make);
            break;
        }
        SmTArray<SmVertex*> vs;
        SmTArray<SmEdge*> es;
        SmTArray<SmFace*> fs;
        if (mode == 2)
            read_targets(rd, g, vs, es, fs);
        mark_all_dead(h, r);
        res.stat = mode == 2 ? h.HealBrep(brep, op, &vs, &es, &fs, make) : h.HealBrep(brep, op, NULL, NULL, NULL, make);
        res.kind = P1_LAST;
        break;
    }
    default: // 31
    {
        o.push_back((double)r.f.size());
        for (size_t ri = 0; ri < r.f.size(); ++ri)
        {
            SmFaceProps* p = r.f[ri];
            if (!r.alive_at(p, ri, r.iff))
            {
                o.push_back(0.0);
                continue;
            }
            o.push_back(1.0);
            put_face_snapshot(o, g, *p);
        }
        res.kind = P1_WRITTEN;
        break;
    }
    }
    return res;
}

void run_heal(Rd& rd, const char* path, std::vector<double>& o)
{
    Graph g;
    SmBrep* brep = prepare(rd, path, g, o);
    if (brep == nullptr)
        return;
    o.push_back(0.0);
    SmTArray<SmVertex*> vs;
    SmTArray<SmEdge*> es;
    SmTArray<SmFace*> fs;
    read_targets(rd, g, vs, es, fs);
    SmHealData* h = new SmHealData(brep, vs, es, fs);
    Registry r;
    r.note(*h);
    h->m_lXCnt = (ULONG)rd.idx();
    h->m_lYCnt = (ULONG)rd.idx();
    h->m_lZCnt = (ULONG)rd.idx();

    int64_t nsteps = rd.idx();
    bool stop = false;
    for (int64_t s = 0; s < nsteps && !stop; s++)
    {
        int64_t code = rd.idx();
        o.push_back((double)code);
        SmBoolean bNew = FALSE;
        SmStatus stat = SM_SUCCESS;
        // Mutation can invalidate the fixture graph and retired props. Preserve
        // complete state dumps on empty-queue paths; terminate actual repair
        // attempts after their status (Rust defers unavailable mutating callees).
        bool repair = (code == 34 && h->m_sBadVertexProps_CoinVertices.m_sProbArray.GetSize() >= 2)
                   || (code == 35 && h->m_sBadFaceProps_DegenFaces.GetSize() != 0)
                   || (code == 36 && h->m_sBadEdgeProps_DegenEdges.GetSize() != 0)
                   || (code == 37 && (h->m_sBadFaceProps_MissingSeam.GetSize() != 0 || h->m_sBadFaceProps_CrossedSeam.GetSize() != 0 || h->m_sBadFaceProps_NearMissSeam.GetSize() != 0))
                   || (code == 38 && (h->m_sBadFaceProps_CrossedSeam.GetSize() != 0 || h->m_sBadFaceProps_CrossedPole.GetSize() != 0))
                   || (code == 39 && h->m_sBadFaceProps_MissingSeam.GetSize() != 0);
        if (repair) mark_all_dead(*h, r);
        switch (code)
        {
        case 0:
        {
            bool bad = false;
            for (ULONG ii = h->m_lOldEdgeCnt; ii < h->m_sTgtEdgeProps.GetSize(); ii++)
            {
                SmEdgeProps* p = h->m_sTgtEdgeProps[ii];
                if (!p->IsSet_Gaps())
                {
                    if (ii >= h->m_sTgtEdges.GetSize())
                    {
                        bad = true;
                        break;
                    }
                    if (!p->IsSet_Metadata() && h->m_sTgtEdges[ii]->GetCurve() == NULL)
                    {
                        bad = true;
                        break;
                    }
                }
            }
            if (bad && !(h->m_sTgtEdgeProps.GetSize() == 0 || h->m_sTgtEdgeProps.GetSize() == h->m_lOldEdgeCnt))
            {
                o.push_back(ST_NULL_DEREF);
                stop = true;
                continue;
            }
            stat = h->Cache_EdgeProps(brep, &bNew, SM_HO_CACHE_EDGEPROPS);
            break;
        }
        case 1:
        {
            bool bad = !(h->m_sTgtVertexProps.GetSize() == 0 || h->m_sTgtVertexProps.GetSize() == h->m_lOldVertexCnt) &&
                       lists_drift(h->m_sTgtVertexProps, h->m_sTgtVertices, h->m_lOldVertexCnt,
                                   [](SmVertexProps* p) { return !p->IsSet(); });
            if (bad)
            {
                o.push_back(ST_NULL_DEREF);
                stop = true;
                continue;
            }
            stat = h->Cache_VertexProps(brep, &bNew, SM_HO_CACHE_VERTEXPROPS);
            break;
        }
        case 2:
        {
            bool bad = !(h->m_sTgtFaceProps.GetSize() == 0 || h->m_sTgtFaceProps.GetSize() == h->m_lOldFaceCnt) &&
                       lists_drift(h->m_sTgtFaceProps, h->m_sTgtFaces, h->m_lOldFaceCnt,
                                   [](SmFaceProps* p) { return !p->HasProps(SM_PROPSTAGE_GAPS, p->m_pFace); });
            if (bad)
            {
                o.push_back(ST_NULL_DEREF);
                stop = true;
                continue;
            }
            stat = h->Cache_FaceProps_Gaps(brep, &bNew, SM_HO_CACHE_FACEPROPS_GAPS);
            break;
        }
        case 3: stat = h->Fix_TolSizes(brep, SM_HO_FIX_TOLSIZES); break;
        case 4:
        {
            // every visited props needs its vertex; re-running on new vertices must put
            // one into a voxel (else the first index of an empty bucket list is read)
            ULONG n = h->m_sTgtVertexProps.GetSize();
            ULONG old = h->m_lOldVertexCnt;
            bool bad = false;
            if (!(n == 0 || n == old))
            {
                for (ULONG ii = (old == 0 ? 0 : old); ii < n; ii++)
                    if (h->m_sTgtVertexProps[ii]->m_pVertex == NULL)
                        bad = true;
                if (old != 0 && (old > n || (h->m_pVerticesInVoxels->*VxPeek::box()).IsInit()))
                    bad = true;
            }
            if (bad)
            {
                o.push_back(ST_NULL_DEREF);
                stop = true;
                continue;
            }
            stat = h->Cache_CoinVertices(brep, &bNew, SM_HO_CACHE_COIN_VERTICES);
            break;
        }
        case 5:
        {
            if (h->m_sTgtFaces.GetSize() < h->m_sTgtFaceProps.GetSize())
            {
                o.push_back(ST_NULL_DEREF);
                stop = true;
                continue;
            }
            stat = h->Cache_DegenFaces(brep, &bNew, SM_HO_CACHE_DEGEN_FACES);
            break;
        }
        case 6: stat = h->Cache_CoinEdges(brep, &bNew, SM_HO_CACHE_COIN_EDGES); break;
        case 7: stat = h->Fix_CoinEdges(brep, SM_HO_FIX_COIN_EDGES); break;
        case 8: stat = h->Cache_MissedEdgeXSects(brep, &bNew, SM_HO_CACHE_MISSED_EDGEXSECTS); break;
        case 9: stat = h->Fix_MissedEdgeXSects(brep, SM_HO_FIX_MISSED_EDGEXSECTS); break;
        case 10: stat = h->Fix_UncontainedEdges(brep, SM_HO_FIX_UNCONTAINED_EDGES); break;
        case 11: stat = h->Fix_BadGaps(brep, SM_HO_FIX_BADGAPS); break;
        case 12:
        case 13:
        case 14:
        {
            int64_t sel = rd.idx();
            SmBoolean b1 = UNSURE, b2 = UNSURE;
            if (code == 12)
            {
                SmVertexProps* p = r.pick(r.v, r.iv, sel);
                if (p == nullptr)
                {
                    o.push_back(-3.0);
                    continue;
                }
                if (pair_remove_is_ub(h->m_sBadVertexProps_CoinVertices, p))
                {
                    o.push_back(ST_NULL_DEREF);
                    stop = true;
                    continue;
                }
                h->RemoveVertexProps_FromLists(p, &b1, &b2);
                o.push_back((double)idx_of(r.iv, p));
            }
            else if (code == 13)
            {
                SmEdgeProps* p = r.pick(r.e, r.ie, sel);
                if (p == nullptr)
                {
                    o.push_back(-3.0);
                    continue;
                }
                if (pair_remove_is_ub(h->m_sBadEdgeProps_CoinEdges, p) ||
                    pair_remove_is_ub(h->m_sBadEdgeProps_MissedEdgeXSects, p))
                {
                    o.push_back(ST_NULL_DEREF);
                    stop = true;
                    continue;
                }
                h->RemoveEdgeProps_FromLists(p, &b1, &b2);
                o.push_back((double)idx_of(r.ie, p));
            }
            else
            {
                SmFaceProps* p = r.pick(r.f, r.iff, sel);
                if (p == nullptr)
                {
                    o.push_back(-3.0);
                    continue;
                }
                h->RemoveFaceProps_FromLists(p, &b1, &b2);
                o.push_back((double)idx_of(r.iff, p));
            }
            o.push_back((double)b1);
            o.push_back((double)b2);
            continue;
        }
        case 15:
        {
            int64_t which = rd.idx();
            SmBoolean stale = rd.bflag();
            int64_t n = rd.idx();
            SmTArray<SmTopology*> tg;
            for (int64_t i = 0; i < n; i++)
            {
                int64_t k = rd.idx();
                int64_t j = rd.idx();
                SmTopology* t = topo_of(g, k, j);
                if (t != nullptr)
                    tg.Add(t);
            }
            o.push_back(0.0);
            if (which == 0)
            {
                SmTArray<SmVertexProps*> out;
                ULONG c = h->GetVertexProps_ForTgts(&tg, &out, stale);
                o.push_back((double)c);
                for (ULONG i = 0; i < out.GetSize(); i++)
                    o.push_back((double)idx_of(r.iv, out[i]));
            }
            else if (which == 1)
            {
                SmTArray<SmEdgeProps*> out;
                ULONG c = h->GetEdgeProps_ForTgts(&tg, &out, stale);
                o.push_back((double)c);
                for (ULONG i = 0; i < out.GetSize(); i++)
                    o.push_back((double)idx_of(r.ie, out[i]));
            }
            else
            {
                SmTArray<SmFaceProps*> out;
                ULONG c = h->GetFaceProps_ForTgts(&tg, &out, stale);
                o.push_back((double)c);
                for (ULONG i = 0; i < out.GetSize(); i++)
                    o.push_back((double)idx_of(r.iff, out[i]));
            }
            continue;
        }
        case 16: // FreeTargets (last step only)
            for (ULONG i = 0; i < h->m_sTgtVertexProps.GetSize(); i++)
                r.dead.insert(h->m_sTgtVertexProps[i]);
            for (ULONG i = 0; i < h->m_sTgtEdgeProps.GetSize(); i++)
                r.dead.insert(h->m_sTgtEdgeProps[i]);
            for (ULONG i = 0; i < h->m_sTgtFaceProps.GetSize(); i++)
                r.dead.insert(h->m_sTgtFaceProps[i]);
            for (ULONG i = 0; i < h->m_sNotYetFaceProps.GetSize(); i++)
                r.dead.insert(h->m_sNotYetFaceProps[i]);
            h->FreeTargets();
            break;
        case 17:
        {
            SmTArray<SmVertex*> v2;
            SmTArray<SmEdge*> e2;
            SmTArray<SmFace*> f2;
            read_targets(rd, g, v2, e2, f2);
            h->LoadTargets(brep, v2, e2, f2);
            r.note(*h);
            break;
        }
        case 18:
            h->m_lOldVertexCnt = (ULONG)rd.idx();
            h->m_lOldEdgeCnt = (ULONG)rd.idx();
            h->m_lOldFaceCnt = (ULONG)rd.idx();
            break;
        case 19: h->m_eDoneHealOp = (SmHealerOpType)rd.idx(); break;
        case 20:
        {
            int64_t sel = rd.idx();
            bool meta = rd.flag();
            SmEdgeProps* p = r.pick(r.e, r.ie, sel);
            if (p != nullptr)
                p->ReSet(meta ? TRUE : FALSE);
            break;
        }
        case 21:
        {
            int64_t sel = rd.idx();
            SmVertexProps* p = r.pick(r.v, r.iv, sel);
            if (p != nullptr)
                p->ReSet();
            break;
        }
        case 22: h->SetupEmptyProbArrays(); break;
        // ---- steps of unit heal.heal_data_p1 (see p1_step) ----
        case 23:
        case 24:
        case 25:
        case 26:
        case 27:
        case 28:
        case 29:
        case 30:
        case 31:
        {
            P1Out res = p1_step(code, rd, *h, brep, g, r, o);
            if (res.kind == P1_REFUSED)
            {
                stop = true;
                continue;
            }
            if (res.kind == P1_WRITTEN)
                continue;
            stat = res.stat;
            bNew = res.bNew;
            if (res.kind == P1_LAST)
            {
                o.push_back(st(stat));
                o.push_back(flg(bNew));
                stop = true;
                continue;
            }
            break;
        }
        case 32:
        {
            SmHealerOpType op = (SmHealerOpType)rd.idx();
            bool make = rd.flag();
            mark_all_dead(*h, r);
            stat = h->HealTgts(op, make ? TRUE : FALSE);
            r.note(*h);
            o.push_back((double)h->m_eStopHealOp);
            o.push_back((double)h->m_eDoneHealOp);
            o.push_back((double)h->m_lRecursionCount);
            o.push_back((double)h->m_sTgtVertices.GetSize());
            o.push_back((double)h->m_sTgtEdges.GetSize());
            o.push_back((double)h->m_sTgtFaces.GetSize());
            stop = true; // full healing may destroy the original fixture graph
            break;
        }
        case 33:
        {
            SmHealerOpType op = (SmHealerOpType)rd.idx();
            SmTArray<SmTopology*> removed, added;
            for (int pass = 0; pass < 2; ++pass)
            {
                int64_t n = rd.idx();
                for (int64_t i = 0; i < n; ++i)
                {
                    int kind = (int)rd.idx();
                    int64_t sel = rd.idx();
                    SmTopology* t = kind == 10 ? NULL : topo_of(g, kind, sel);
                    (pass == 0 ? removed : added).Add(t);
                }
            }
            if (stop) continue;
            if (removal_lookup_is_ub(h->m_sTgtFaces, h->m_sTgtFaceProps, removed) ||
                removal_lookup_is_ub(h->m_sTgtEdges, h->m_sTgtEdgeProps, removed) ||
                removal_lookup_is_ub(h->m_sTgtVertices, h->m_sTgtVertexProps, removed))
            {
                o.push_back(ST_NULL_DEREF);
                stop = true;
                continue;
            }
            SmTArray<SmFaceProps*> rf;
            SmTArray<SmEdgeProps*> re;
            SmTArray<SmVertexProps*> rv;
            h->GetFaceProps_ForTgts(&removed, &rf, TRUE);
            h->GetEdgeProps_ForTgts(&removed, &re, TRUE);
            h->GetVertexProps_ForTgts(&removed, &rv, TRUE);
            // Resolve first: duplicates not present in a target list, and
            // repeated unrelated topology entries, are safe native inputs.
            std::set<const void*> deleting;
            bool unsafe_remove = false;
            for (ULONG i = 0; i < rf.GetSize(); ++i)
                unsafe_remove |= !deleting.insert(rf[i]).second;
            for (ULONG i = 0; i < re.GetSize(); ++i)
                unsafe_remove |= !deleting.insert(re[i]).second || pair_remove_is_ub(h->m_sBadEdgeProps_CoinEdges, re[i]) || pair_remove_is_ub(h->m_sBadEdgeProps_MissedEdgeXSects, re[i]);
            for (ULONG i = 0; i < rv.GetSize(); ++i)
                unsafe_remove |= !deleting.insert(rv[i]).second || pair_remove_is_ub(h->m_sBadVertexProps_CoinVertices, rv[i]);
            if (unsafe_remove)
            {
                o.push_back(ST_NULL_DEREF);
                stop = true;
                continue;
            }
            if ((op < h->m_eDoneHealOp ? op : h->m_eDoneHealOp) >= SM_HO_FIX_COIN_VERTICES &&
                readds_live_target(*h, brep, removed, added))
            {
                o.push_back(ST_NULL_DEREF);
                stop = true;
                continue;
            }
            // Native UpdateTargetLists sizes every props list to its TARGET
            // count. Reject holes left by removing an uncached props record,
            // including drift already introduced by a prior Remove*Props step.
            size_t faces_after = target_count_after_removals(h->m_sTgtFaces, rf, [](SmFaceProps* p) { return p->m_pFace; });
            size_t edges_after = target_count_after_removals(h->m_sTgtEdges, re, [](SmEdgeProps* p) { return (SmEdge*)p->m_pEdge; });
            size_t vertices_after = target_count_after_removals(h->m_sTgtVertices, rv, [](SmVertexProps* p) { return (SmVertex*)p->m_pVertex; });
            bool drift = faces_after != h->m_sTgtFaceProps.GetSize() - rf.GetSize() ||
                         edges_after != h->m_sTgtEdgeProps.GetSize() - re.GetSize() ||
                         vertices_after != h->m_sTgtVertexProps.GetSize() - rv.GetSize();
            if (added.GetSize() != 0 && drift)
            {
                o.push_back(ST_NULL_DEREF);
                stop = true;
                continue;
            }
            ULONG new_vertices = 0;
            for (ULONG i = 0; i < added.GetSize(); ++i)
                if (added[i] != NULL && brep->IsLiveTopologyMember(added[i]) && added[i]->IsKindOf(SmVertex_TYPE))
                    ++new_vertices;
            SmHealerOpType through = op < h->m_eDoneHealOp ? op : h->m_eDoneHealOp;
            if (added.GetSize() != 0 && through >= SM_HO_CACHE_COIN_VERTICES &&
                vertices_after > 0 && new_vertices > 0 &&
                (h->m_pVerticesInVoxels->*VxPeek::box()).IsInit())
            {
                o.push_back(ST_NULL_DEREF);
                stop = true;
                continue;
            }
            // Catch-up from Fix_CoinVertices onward can invalidate fixture
            // topology and recycle props addresses, just like HealTgts.
            if (added.GetSize() != 0 && through >= SM_HO_FIX_COIN_VERTICES)
            {
                mark_all_dead(*h, r);
                stop = true;
            }
            for (ULONG i = 0; i < rf.GetSize(); ++i) r.dead.insert(rf[i]);
            for (ULONG i = 0; i < re.GetSize(); ++i) r.dead.insert(re[i]);
            for (ULONG i = 0; i < rv.GetSize(); ++i) r.dead.insert(rv[i]);
            stat = h->UpdateTargetLists(&removed, &added, op);
            r.note(*h);
            break;
        }
        case 42:
        {
            bool null_face = rd.flag();
            if (degen_face_seeded(*h))
            {
                o.push_back(ST_NULL_DEREF);
                stop = true;
                continue;
            }
            for (ULONG i = 0; i < h->m_sTgtFaceProps.GetSize(); ++i)
            {
                SmFaceProps* p = h->m_sTgtFaceProps[i];
                p->m_pFace = null_face || i >= h->m_sTgtFaces.GetSize() ? NULL : h->m_sTgtFaces[i];
                p->m_lFaceIndx = i;
                p->m_eBadCrossedSeam = SM_SP_NEITHER;
                h->m_sBadFaceProps_DegenFaces.Add(p);
                h->m_sBadFaceProps_CrossedSeam.Add(p);
                h->m_sBadFaceProps_MissingSeam.Add(p);
            }
            break;
        }
        case 41:
            if (degen_face_seeded(*h))
            {
                o.push_back(ST_NULL_DEREF);
                stop = true;
                continue;
            }
            for (ULONG i = 0; i < h->m_sTgtFaceProps.GetSize(); ++i)
            {
                SmFaceProps* p = h->m_sTgtFaceProps[i];
                p->m_pFace = h->m_sTgtFaces[i];
                p->m_lFaceIndx = i;
                h->m_sBadFaceProps_DegenFaces.Add(p);
            }
            break;
        case 34: stat = h->Fix_CoinVertices(brep, SM_HO_FIX_COIN_VERTICES); break;
        case 35: stat = h->Fix_DegenFaces(brep, SM_HO_FIX_DEGEN_FACES); break;
        case 36: stat = h->Fix_DegenEdges(brep, SM_HO_FIX_DEGEN_EDGES); break;
        case 37:
        case 38:
        {
            // UB-265: v1.0.1 Fix_SplitEdgesAtSeam ends by reading the mark of every target
            // face props' m_pFace (IsMarked) without a null check. Reject the whole case
            // (`-999 265`, replacing the record) while a target face props has no face
            // (before Cache_FaceProps_Gaps, or seeded with a null face).
            if (code == 38)
            {
                bool null_face = false;
                for (ULONG i = 0; i < h->m_sTgtFaceProps.GetSize(); ++i)
                    null_face |= h->m_sTgtFaceProps[i]->m_pFace == NULL;
                if (null_face)
                {
                    o.clear();
                    o.push_back(-999.0);
                    o.push_back(265.0);
                    stop = true;
                    continue;
                }
            }
            SmNewMarkAndLock lock(brep->GetContext());
            stat = code == 37 ? h->Fix_MoveSeam(brep, lock, SM_HO_FIX_MOVESEAM)
                              : h->Fix_SplitEdgesAtSeam(brep, lock, SM_HO_FIX_SPLITEDGE_ATSEAM);
            break;
        }
        case 39: stat = h->Fix_SplitFaceAtSeams(brep, SM_HO_FIX_SPLITFACE_ATSEAMS); break;
        case 40:
        {
            bool active = rd.flag();
            SmHealerOpType op = (SmHealerOpType)rd.idx();
            SmTrackTopologyChanges tracker(brep, active ? TRUE : FALSE);
            stat = h->UpdateTargetLists(tracker, op);
            tracker.StopTracking();
            break;
        }
        default: stat = SC_ERR_ARG; break;
        }
        if (code == 38 && stat == SM_SUCCESS)
        {
            o.push_back((double)h->m_sTgtFaceProps.GetSize());
            for (ULONG i = 0; i < h->m_sTgtFaceProps.GetSize(); ++i)
            {
                SmFaceProps* p = h->m_sTgtFaceProps[i];
                o.push_back((double)p->m_eBadCrossedSeam);
                o.push_back((double)p->m_bBadCrossedSeam);
                o.push_back((double)p->m_eBeenThroughSplitEdgesAtSeam);
            }
        }
        if (repair && stat == SM_SUCCESS) put_repair_summary(brep, *h, o);
        if (repair) { r.note(*h); stop = true; }
        o.push_back(st(stat));
        o.push_back(flg(bNew));
        if (stat != SM_SUCCESS)
            stop = true;
    }
    if (!stop)
        dump(*h, g, r, o);

    // free what the healer no longer owns, then the healer and the B-rep
    std::set<const void*> owned;
    for (ULONG i = 0; i < h->m_sTgtVertexProps.GetSize(); i++)
        owned.insert(h->m_sTgtVertexProps[i]);
    for (ULONG i = 0; i < h->m_sTgtEdgeProps.GetSize(); i++)
        owned.insert(h->m_sTgtEdgeProps[i]);
    for (ULONG i = 0; i < h->m_sTgtFaceProps.GetSize(); i++)
        owned.insert(h->m_sTgtFaceProps[i]);
    for (ULONG i = 0; i < h->m_sNotYetFaceProps.GetSize(); i++)
        owned.insert(h->m_sNotYetFaceProps[i]);
    for (size_t ri = 0; ri < r.v.size(); ++ri)
        if (r.alive_at(r.v[ri], ri, r.iv) && owned.find(r.v[ri]) == owned.end())
            delete r.v[ri];
    for (size_t ri = 0; ri < r.e.size(); ++ri)
        if (r.alive_at(r.e[ri], ri, r.ie) && owned.find(r.e[ri]) == owned.end())
            delete r.e[ri];
    for (size_t ri = 0; ri < r.f.size(); ++ri)
        if (r.alive_at(r.f[ri], ri, r.iff) && owned.find(r.f[ri]) == owned.end())
            delete r.f[ri];
    delete h;
    delete brep;
}
} // namespace

SC_EXPORT int32_t sc_heal_heal_data_p2_eval(int32_t op, const char* path, const double* in, int64_t n_in, double* out,
                                            int64_t cap, int64_t* out_len)
{
    if (in == nullptr || n_in < 0 || out_len == nullptr || path == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    switch (op)
    {
    case P2_EXPORT: run_export(rd, path, o, false); break;
    case P2_VLISTS: run_export(rd, path, o, true); break;
    case P2_PROB: run_prob(rd, o); break;
    case P2_HEAL: run_heal(rd, path, o); break;
    default: return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
