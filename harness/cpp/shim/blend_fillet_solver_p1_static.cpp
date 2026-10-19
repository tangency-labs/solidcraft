// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_solver_p1`: the file-static helpers and the
// .cpp-local classes of upstream source/SMLib/src/SmFilletSolver.cpp (sm_FindNextGoodInterval,
// sm_CheckEdgeCurveDomains, sm_TestAnalyticSurface, sm_TrimRail, sm_TestManifoldTangentRollover,
// SmRailRailIntersectENFO,
// the SmFindClippedRadiusEFO constructor and Evaluate), plus the external sm_TestSideEdgeConvexity and the
// solver's B-rep queries (GetVertex, FindIndexOfRailOnFace, FindIndexOfRailXSideEdgeuse).
//
// Static functions and .cpp-local classes cannot be linked against, so this translation unit
// compiles SmFilletSolver.cpp itself, as surface_surface_point_solve_p3.cpp does: every class
// and function the file defines with external linkage is renamed (before any header, so the
// whole TU sees one consistent set of names) and does not clash with the library. The renamed
// solver class is never constructed: the solvers handed to the copied code are the library
// solvers of the main shim (blend_fillet_solver_p1.cpp, `sc_fsp1_*` helpers), whose layout is
// the same header text. Their virtuals (the shim's deterministic LoadJacobian) dispatch
// through their own vtables; the copied non-virtual getters read the same members.
//
// One entry point, sc_blend_fillet_solver_p1_static_eval(op, path, in, n_in, out, cap,
// out_len). Layouts are documented in crates/solidcore-ffi/src/units/blend_fillet_solver_p1.rs,
// which validates every input. B-rep blocks are those of unit topology.edgeuse_p1
// (sc_edgeuse_graph.h); indices are its traversal order. `-2` in an output marks a call the
// shim skipped because upstream would dereference NULL there.
#define SmFilletSolver sc_fsp1s_SmFilletSolver
#define SmFilletSphereSolveENFO sc_fsp1s_SmFilletSphereSolveENFO
#define SmRailUVCurveIntersectENFO sc_fsp1s_SmRailUVCurveIntersectENFO
#define SmRailRailIntersectENFO sc_fsp1s_SmRailRailIntersectENFO
#define SmFindClippedRadiusEFO sc_fsp1s_SmFindClippedRadiusEFO
#define SmSelfIntersectionHandler sc_fsp1s_SmSelfIntersectionHandler
#define SmMakeCurveBlendSIH sc_fsp1s_SmMakeCurveBlendSIH
#define SmMakeSurfaceBlendSIH sc_fsp1s_SmMakeSurfaceBlendSIH
#define sm_RefineRailCurveClassification sc_fsp1s_sm_RefineRailCurveClassification
#define sm_TestSideEdgeConvexity sc_fsp1s_sm_TestSideEdgeConvexity

#include "sc_shim.h"

#include "StdAfx.h"
#include <SmCurveClass.h>
#include <SmObject.h>
#include <SmPointClass.h>

#define private public
#define protected public
#include "../../../upstream/source/SMLib/src/SmFilletSolver.cpp"
#undef private
#undef protected

#include "sc_edgeuse_graph.h"

#include <SmAxis2Placement.h>
#include <SmCone.h>
#include <SmCylinder.h>
#include <SmSphere.h>
#include <SmTorus.h>

#include <stdexcept>

// Library members whose signatures name the solver class carry the renamed name in this TU
// and so do not exist in the library. Only copied code the shim never runs (the renamed
// solver's constructor, CalcFilletGeom, FindIndexInExec) calls them: these definitions
// satisfy the linker and throw if ever reached.
SmFilletGeom::SmFilletGeom(sc_fsp1s_SmFilletSolver*, SmFilletGeomType)
{
    throw std::logic_error("blend_fillet_solver_p1_static: renamed SmFilletGeom constructor");
}
SmFilletIntersector::SmFilletIntersector(const SmSurface& s1, const SmExtent2d& d1, const SmSurface& s2,
                                         const SmExtent2d& d2, sc_fsp1s_SmFilletSolver& fs, SmFilletGeom*)
    : SmAdvSurfaceIntersector(s1, d1, s2, d2, 0), m_rFilletSolver(fs)
{
    throw std::logic_error("blend_fillet_solver_p1_static: renamed SmFilletIntersector constructor");
}
ULONG SmFilletExecutive::FindSolverIndex(const sc_fsp1s_SmFilletSolver*) const
{
    throw std::logic_error("blend_fillet_solver_p1_static: renamed FindSolverIndex");
}

#undef sm_TestSideEdgeConvexity
// The library's sm_TestSideEdgeConvexity (not static, SmFilletSolver.cpp:6072).
SmBoolean sm_TestSideEdgeConvexity(SmEdge* pSideEdge, SmBoolean bIsTangentialSideEdge);

// Library solvers made by the main shim (handles are `SmFilletSolver*` of the library).
extern "C" void* sc_fsp1_solver_new(int32_t load, int32_t found, double tol);
extern "C" void* sc_fsp1_dist_solver_new(double dist, double tol);
extern "C" void sc_fsp1_solver_delete(void* fs);
extern "C" void sc_fsp1_solver_set_edgeuses(void* fs, SmEdgeuse* eu0, SmEdgeuse* eu1);
extern "C" void sc_fsp1_solver_set_surface(void* fs, uint32_t rail, SmOffsetSurface* s);
extern "C" void sc_fsp1_dist_solver_set_orientation(void* fs, uint32_t rail, double o);
extern "C" SmVertex* sc_fsp1_solver_vertex(void* fs, uint32_t index);
extern "C" uint32_t sc_fsp1_solver_rail_on_face(void* fs, SmFace* face);
extern "C" uint32_t sc_fsp1_solver_rail_x_side(void* fs, SmEdgeuse* side);

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/blend_fillet_solver_p1.rs).
enum : int32_t
{
    S_FIND_NEXT = 0,
    S_CHECK_DOMAINS = 1,
    S_ANALYTIC = 2,
    S_RAIL_RAIL = 3,
    S_CLIPPED = 4,
    S_CONVEXITY = 5,
    S_TRIM_RAIL = 6,
    S_QUERIES = 7,
    S_ANALYTIC_REV = 8,
    S_ROLLOVER = 9,
};

// An SmEdge outside any B-rep's lists (no edge uses); its context comes from the B-rep.
struct FreeEdge : SmEdge
{
    FreeEdge() {}
    ~FreeEdge() {}
};

sc_fsp1s_SmFilletSolver& copy_view(void* fs) { return *(sc_fsp1s_SmFilletSolver*)fs; }

// A curve's fingerprint through SmCurve: `1 t0 t1 P(t0) P(mid) P(t1)`, `-1` for none.
void put_crv(std::vector<double>& o, const SmCurve* c)
{
    if (c == NULL)
    {
        o.push_back(-1.0);
        return;
    }
    o.push_back(1.0);
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    for (double t : {ivl.GetMin(), ivl.GetMid(), ivl.GetMax()})
    {
        SmPoint3d p(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        c->EvaluatePoint(t, p);
        put_vec(o, p);
    }
}

// `n start cls[n]` -> `status found index` (index 999 when not found: upstream leaves it).
void run_find_next(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    int64_t n = rd.idx();
    ULONG start = (ULONG)rd.idx();
    SmPlane obj(SmPoint3d(0.0, 0.0, 0.0), SmVector3d(0.0, 0.0, 1.0), &ctx);
    SmCurveClassification cc;
    cc.SetSize((ULONG)n);
    for (int64_t i = 0; i < n; i++)
        cc[(ULONG)i].m_vMid.SetClassObject((SmPointClassificationType)rd.idx(), &obj);
    SmBoolean found = TRUE;
    ULONG index = 999;
    SmStatus s = sm_FindNextGoodInterval(&cc, start, found, index);
    o.push_back(st(s));
    o.push_back(flg(found));
    o.push_back((double)index);
}

// Builds the case's B-rep; on failure writes `-1 status` and returns NULL.
SmBrep* brep_of(Rd& rd, const char* path, Graph& g, std::vector<double>& o)
{
    SmStatus bst = SM_SUCCESS;
    SmBrep* b = build(rd, path, bst);
    if (b == NULL)
    {
        o.push_back(-1.0);
        o.push_back((double)bst);
        return NULL;
    }
    walk(b, g);
    o.push_back(0.0);
    return b;
}

template <class T> T* at(std::vector<T*>& v, int64_t i)
{
    return (i >= 0 && i < (int64_t)v.size()) ? v[(size_t)i] : NULL;
}

// `block edge before after` -> `0 ret curve` (`-2` for an edge without curve).
void run_check_domains(Rd& rd, const char* path, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    Graph g;
    SmBrep* b = brep_of(rd, path, g, o);
    if (b == NULL)
        return;
    SmEdge* e = at(g.edges, rd.idx());
    SmBoolean before = rd.bflag();
    SmBoolean after = rd.bflag();
    if (e == NULL || e->GetCurve() == NULL)
        o.push_back(-2.0);
    else
    {
        SmObjDelete del;
        SmCurve* c = NULL;
        SmBoolean r = sm_CheckEdgeCurveDomains(&ctx, e, c, before, after, del);
        o.push_back(flg(r));
        put_crv(o, c);
    }
    delete b;
}

// `surface` -> `ret`.
void run_analytic(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    SmSurface* s = read_surface(rd, ctx);
    o.push_back(flg(sm_TestAnalyticSurface((SmBSplineSurface*)s)));
    delete s;
}

// `kind origin x y p[4]` -> `status ret` (`ret` only when the surface was built): kind 0
// SmCone::CreateCanonical(bot, top, height), 1 SmCylinder(radius, start, end, height) +
// MakeNurb, 2 SmSphere::CreateCanonical(radius), 3 SmTorus::CreateCanonical(major, minor).
void run_analytic_rev(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    int64_t kind = rd.idx();
    SmPoint3d org = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    double p[4];
    for (int i = 0; i < 4; i++)
        p[i] = rd.num();
    SmAxis2Placement a2p(org, x, y);
    SmBSplineSurface* s = NULL;
    SmStatus stt = SM_SUCCESS;
    switch (kind)
    {
    case 0:
    {
        SmCone* c = NULL;
        stt = SmCone::CreateCanonical(ctx, a2p, p[0], p[1], p[2], c, FALSE, FALSE);
        s = c;
        break;
    }
    case 1:
    {
        SmCylinder* c = new (ctx) SmCylinder(org, x, y, p[0], p[1], p[2], p[3], FALSE, FALSE, FALSE, &ctx);
        stt = c->MakeNurb();
        s = c;
        break;
    }
    case 2:
    {
        SmSphere* c = NULL;
        stt = SmSphere::CreateCanonical(ctx, a2p, p[0], c);
        s = c;
        break;
    }
    default:
    {
        SmTorus* c = NULL;
        stt = SmTorus::CreateCanonical(ctx, a2p, p[0], p[1], c);
        s = c;
        break;
    }
    }
    o.push_back(st(stt));
    if (stt != SM_SUCCESS || s == NULL)
    {
        delete s;
        return;
    }
    o.push_back(flg(sm_TestAnalyticSurface(s)));
    delete s;
}

// `nx x[nx] nf f[nf] has_j rows cols` as in the main shim.
struct Arrays
{
    SmTArray<double> x;
    SmTArray<double> f;
    SmMatrix* j = NULL;
    ~Arrays() { delete j; }
    void read(Rd& rd)
    {
        int64_t nx = rd.idx();
        for (int64_t i = 0; i < nx; i++)
            x.Add(rd.num());
        int64_t nf = rd.idx();
        for (int64_t i = 0; i < nf; i++)
            f.Add(rd.num());
        bool has_j = rd.flag();
        ULONG rows = (ULONG)rd.idx();
        ULONG cols = (ULONG)rd.idx();
        if (has_j)
        {
            j = new SmMatrix(rows, cols);
            for (ULONG r = 0; r < rows; r++)
                for (ULONG c = 0; c < cols; c++)
                    (*j)[r][c] = 0.0;
        }
    }
    void put(std::vector<double>& o)
    {
        for (ULONG i = 0; i < f.GetSize(); i++)
            o.push_back(f[i]);
        if (j)
            for (ULONG r = 0; r < j->GetNumRows(); r++)
                for (ULONG c = 0; c < j->GetNumColumns(); c++)
                    o.push_back((*j)[r][c]);
    }
};

// `rail other load1 found1 load2 found2 arrays` -> `status found f J`.
void run_rail_rail(Rd& rd, std::vector<double>& o)
{
    ULONG rail = (ULONG)rd.idx();
    ULONG other = (ULONG)rd.idx();
    int32_t load1 = rd.flag() ? 1 : 0;
    int32_t found1 = rd.flag() ? 1 : 0;
    int32_t load2 = rd.flag() ? 1 : 0;
    int32_t found2 = rd.flag() ? 1 : 0;
    Arrays a;
    a.read(rd);
    void* fs1 = sc_fsp1_solver_new(load1, found1, 1.0e-6);
    void* fs2 = sc_fsp1_solver_new(load2, found2, 1.0e-6);
    {
        SmRailRailIntersectENFO e(rail, copy_view(fs1), other, copy_view(fs2));
        SmBoolean found = FALSE;
        SmStatus s = e.Evaluate(a.x, a.f, a.j, found);
        o.push_back(st(s));
        o.push_back(flg(found));
        a.put(o);
    }
    sc_fsp1_solver_delete(fs1);
    sc_fsp1_solver_delete(fs2);
}

// `idx dist tol base0 base1 has0 off0 has1 off1 o0 o1 dom0[4] dom1[4] param iso uv0[2]
// uv1[2] eval radius` -> `surf other param iso orient other_orient other_dom[4] target tol
// guess uv[2] other_uv[2]` (surfaces as 0/1 for the solver's first/second offset surface,
// -1 for none; base i is the base of offset surface i), then with `eval`
// `Evaluate(radius, F, F', found)`: `status F F' found sUV[idx][2] sUV[1-idx][2] guess
// off0 off1` (sUV is the caller's array m_rUV / m_rOtherUV refer to; the offset distances
// Evaluate leaves), or `-2` without both offset surfaces (upstream dereferences NULL).
void run_clipped(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    ULONG idx = (ULONG)rd.idx();
    double dist = rd.num();
    double tol = rd.num();
    SmSurface* base[2] = {NULL, NULL};
    base[0] = read_surface(rd, ctx);
    base[1] = read_surface(rd, ctx);
    void* fs = sc_fsp1_dist_solver_new(dist, tol);
    SmOffsetSurface* made[2] = {NULL, NULL};
    for (uint32_t i = 0; i < 2; i++)
    {
        bool has = rd.flag();
        double off = rd.num();
        if (has)
        {
            made[i] = new (ctx) SmOffsetSurface(off, *base[i]);
            sc_fsp1_solver_set_surface(fs, i, made[i]);
        }
    }
    sc_fsp1_dist_solver_set_orientation(fs, 0, rd.num());
    sc_fsp1_dist_solver_set_orientation(fs, 1, rd.num());
    SmExtent2d doms[2];
    for (int i = 0; i < 2; i++)
    {
        SmPoint2d a = rd.uv();
        SmPoint2d b = rd.uv();
        doms[i] = SmExtent2d(a, b);
    }
    SmSurfParamType param = (SmSurfParamType)rd.idx();
    double iso = rd.num();
    SmVector2d uv[2];
    uv[0] = rd.uv();
    uv[1] = rd.uv();
    bool eval = rd.flag();
    double radius = rd.num();
    {
        SmFindClippedRadiusEFO e(&copy_view(fs), idx, doms, param, iso, uv);
        auto key = [&](SmOffsetSurface* s) { return s == NULL ? -1.0 : (s == made[0] ? 0.0 : (s == made[1] ? 1.0 : -3.0)); };
        o.push_back(key(e.m_pSurface));
        o.push_back(key(e.m_pOtherSurface));
        o.push_back((double)e.m_eSurfParam);
        o.push_back(e.m_dIsoParam);
        o.push_back(e.m_dOrientation);
        o.push_back(e.m_dOtherOrientation);
        o.push_back(e.m_OtherDomain.GetMin().x);
        o.push_back(e.m_OtherDomain.GetMin().y);
        o.push_back(e.m_OtherDomain.GetMax().x);
        o.push_back(e.m_OtherDomain.GetMax().y);
        o.push_back(e.m_dTargetDistance);
        o.push_back(e.m_dThisApproxTol3d);
        o.push_back(e.m_dGuessT);
        o.push_back(e.m_rUV.x);
        o.push_back(e.m_rUV.y);
        o.push_back(e.m_rOtherUV.x);
        o.push_back(e.m_rOtherUV.y);
        if (eval)
        {
            if (made[0] == NULL || made[1] == NULL)
                o.push_back(-2.0);
            else
            {
                double f = -7.0, fp = -7.0;
                SmBoolean found = UNSURE;
                SmStatus s = e.Evaluate(radius, f, fp, found);
                o.push_back(st(s));
                o.push_back(f);
                o.push_back(fp);
                o.push_back(flg(found));
                o.push_back(uv[idx].x);
                o.push_back(uv[idx].y);
                o.push_back(uv[1 - idx].x);
                o.push_back(uv[1 - idx].y);
                o.push_back(e.m_dGuessT);
                o.push_back(made[0]->GetOffsetDistance());
                o.push_back(made[1]->GetOffsetDistance());
            }
        }
    }
    sc_fsp1_solver_delete(fs); // deletes its offset surfaces
    delete base[0];
    delete base[1];
}

// `block edge tangential` -> `0 ret`; edge -1 is a fresh SmEdge without edge uses.
void run_convexity(Rd& rd, const char* path, std::vector<double>& o)
{
    Graph g;
    SmBrep* b = brep_of(rd, path, g, o);
    if (b == NULL)
        return;
    int64_t ei = rd.idx();
    SmBoolean tangential = rd.bflag();
    if (ei < 0)
    {
        FreeEdge* e = new (b) FreeEdge();
        o.push_back(flg(sm_TestSideEdgeConvexity(e, tangential)));
        delete e;
    }
    else
        o.push_back(flg(sm_TestSideEdgeConvexity(at(g.edges, ei), tangential)));
    delete b;
}

// `zt(curve) zt(uv) zt(uv_mate) n knots[n] mults[n]` (`-1` for the knots without curve or
// when GetKnots fails).
void put_zone_tol_and_knots(std::vector<double>& o, const SmCurve* c, const SmCurve* uv, const SmCurve* uv_mate)
{
    o.push_back(SmTol::GetZoneTol3d((const SmObject*)c));
    o.push_back(SmTol::GetZoneTol3d((const SmObject*)uv));
    o.push_back(SmTol::GetZoneTol3d((const SmObject*)uv_mate));
    SmTArray<double> knots;
    SmTArray<ULONG> mults;
    if (c == NULL || c->GetKnots(knots, &mults) != SM_SUCCESS)
    {
        o.push_back(-1.0);
        return;
    }
    o.push_back((double)knots.GetSize());
    for (ULONG i = 0; i < knots.GetSize(); i++)
        o.push_back(knots[i]);
    for (ULONG i = 0; i < mults.GetSize(); i++)
        o.push_back((double)mults[i]);
}

// `block edge t0 t1` -> `0 zt[3] knots status curve uv uv_mate` (`-2` without primary edge
// use or mate). The rail is a plain SmEdge viewed as the SmFilletEdge sm_TrimRail takes: it
// calls only SmEdge/SmEdgeuse methods neither fillet class overrides.
void run_trim_rail(Rd& rd, const char* path, std::vector<double>& o)
{
    Graph g;
    SmBrep* b = brep_of(rd, path, g, o);
    if (b == NULL)
        return;
    SmEdge* e = at(g.edges, rd.idx());
    double t0 = rd.num();
    double t1 = rd.num();
    SmEdgeuse* prim = e ? e->GetPrimaryEdgeuse() : NULL;
    SmEdgeuse* mate = prim ? prim->GetMate() : NULL;
    if (mate == NULL)
        o.push_back(-2.0);
    else
    {
        // before the trim: the zone tolerance SmBSplineCurve::Trim reads for each curve
        // (SmTol::GetZoneTol3d(this): the owning edge's tolerance) and the 3D curve's
        // unique knots and multiplicities (`n knots[n] mults[n]`, `-1` without curve or
        // when GetKnots fails)
        put_zone_tol_and_knots(o, e->GetCurve(), prim->GetUVTrimCurve(), mate->GetUVTrimCurve());
        SmStatus s = sm_TrimRail((SmFilletEdge*)e, SmExtent1d(t0, t1));
        o.push_back(st(s));
        put_crv(o, e->GetCurve());
        put_crv(o, prim->GetUVTrimCurve());
        put_crv(o, mate->GetUVTrimCurve());
    }
    delete b;
}

// `block edge tol` -> `0 tangent opposite` (`-2` where upstream dereferences NULL: no
// primary edge use, an edge use without face use, no radial or no mate).
void run_rollover(Rd& rd, const char* path, std::vector<double>& o)
{
    Graph g;
    SmBrep* b = brep_of(rd, path, g, o);
    if (b == NULL)
        return;
    SmEdge* e = at(g.edges, rd.idx());
    double tol = rd.num();
    SmEdgeuse* eu = e ? e->GetPrimaryEdgeuse() : NULL;
    SmEdgeuse* radial = eu ? eu->GetRadial() : NULL;
    SmEdgeuse* other = radial ? radial->GetMate() : NULL;
    if (other == NULL || eu->GetFaceuse() == NULL || other->GetFaceuse() == NULL)
        o.push_back(-2.0);
    else
    {
        SmBoolean opposite = FALSE;
        SmBoolean tangent = sm_TestManifoldTangentRollover(e, opposite, tol, 0);
        o.push_back(flg(tangent));
        o.push_back(flg(opposite));
    }
    delete b;
}

// `block eu0 eu1 face side` -> `0 v0 v1 on_face x_side` (vertices by export index).
void run_queries(Rd& rd, const char* path, std::vector<double>& o)
{
    Graph g;
    SmBrep* b = brep_of(rd, path, g, o);
    if (b == NULL)
        return;
    SmEdgeuse* eu0 = at(g.edgeuses, rd.idx());
    SmEdgeuse* eu1 = at(g.edgeuses, rd.idx());
    SmFace* face = at(g.faces, rd.idx());
    SmEdgeuse* side = at(g.edgeuses, rd.idx());
    void* fs = sc_fsp1_solver_new(0, 0, 1.0e-6);
    sc_fsp1_solver_set_edgeuses(fs, eu0, eu1);
    for (uint32_t i = 0; i < 2; i++)
    {
        SmVertex* v = sc_fsp1_solver_vertex(fs, i);
        o.push_back(v == NULL ? -1.0 : (double)g.find(9, v));
    }
    // FindIndexOfRailOnFace dereferences GetLoopuse()->GetFaceuse() of edge use 0
    if (eu0 != NULL && (eu0->GetLoopuse() == NULL || eu0->GetLoopuse()->GetFaceuse() == NULL))
        o.push_back(-2.0);
    else
        o.push_back((double)sc_fsp1_solver_rail_on_face(fs, face));
    // FindIndexOfRailXSideEdgeuse dereferences the side edge use's edge, edge use 0, both
    // vertex uses on a closed side edge and edge use 0's CCW neighbour across vertices
    bool ub = side == NULL || side->GetEdge() == NULL || eu0 == NULL;
    if (!ub && side->GetEdge()->IsClosed())
    {
        if (eu0->GetVertexuse() == NULL || side->GetVertexuse() == NULL)
            ub = true;
        else if (side->GetVertexuse()->GetVertex() != eu0->GetVertexuse()->GetVertex() &&
                 eu0->GetCCWEdgeuse() == NULL)
            ub = true;
    }
    if (ub)
        o.push_back(-2.0);
    else
        o.push_back((double)sc_fsp1_solver_rail_x_side(fs, side));
    sc_fsp1_solver_delete(fs);
    delete b;
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_solver_p1_static_eval(int32_t op, const char* path, const double* in,
                                                        int64_t n_in, double* out, int64_t cap,
                                                        int64_t* out_len)
{
    if (in == NULL || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    switch (op)
    {
    case S_FIND_NEXT: run_find_next(rd, o); break;
    case S_CHECK_DOMAINS: run_check_domains(rd, path, o); break;
    case S_ANALYTIC: run_analytic(rd, o); break;
    case S_RAIL_RAIL: run_rail_rail(rd, o); break;
    case S_CLIPPED: run_clipped(rd, o); break;
    case S_CONVEXITY: run_convexity(rd, path, o); break;
    case S_TRIM_RAIL: run_trim_rail(rd, path, o); break;
    case S_QUERIES: run_queries(rd, path, o); break;
    case S_ANALYTIC_REV: run_analytic_rev(rd, o); break;
    case S_ROLLOVER: run_rollover(rd, path, o); break;
    default: return SC_ERR_ARG;
    }
    (void)n_in;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
