// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surface_point_solve_p1`: the file-local classes of
// upstream source/SMLib/src/SmSurfacePointSolve.cpp (SmSurfacePointSolver,
// SmSurfPtGlobalSolver, SmSurfPtSolveStepper) and its static helpers (GetTrianglesDist,
// WellBehavedControlNet).
//
// The classes are local to that file, so this translation unit compiles the upstream
// source itself (same flags as the kernel), at global scope:
//  * every header the file includes is included first, so its own #includes are no-ops;
//  * the three classes are renamed with macros, so they do not clash with the copies in
//    the library (SmSurfacePointSolve.o);
//  * SmSurface::GlobalPointSolve_1 / LocalPointSolve_1, which the file also defines, are
//    renamed before any header is read, so this TU's definitions get new names (same
//    vtable slots, never called from here);
//  * `private` / `protected` are made public for the class definitions only, so the shim
//    can call the private members and read the problem data.
//
// One entry point, sc_surface_surface_point_solve_p1_eval(op, in, n_in, out, cap,
// out_len). Layouts are documented in
// crates/solidcore-ffi/src/units/surface_surface_point_solve_p1.rs, which validates every
// input.
#define GlobalPointSolve_1 sc_spsp1_GlobalPointSolve_1
#define LocalPointSolve_1 sc_spsp1_LocalPointSolve_1

#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmContext.h>
#include <SmCurveBoundedSurface.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmFace.h>
#include <SmGeomUtility.h>
#include <SmGraphicsExtern.h>
#include <SmGraphicsOutput.h>
#include <SmIsoCurve.h>
#include <SmLocalSolve1d.h>
#include <SmNurbsSrf.h>
#include <SmOffsetSurface.h>
#include <SmPlane.h>
#include <SmSTEPSurface.h>
#include <SmSurface.h>
#include <SmSurfaceCache.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

#define SmSurfacePointSolver sc_spsp1_SmSurfacePointSolver
#define SmSurfPtGlobalSolver sc_spsp1_SmSurfPtGlobalSolver
#define SmSurfPtSolveStepper sc_spsp1_SmSurfPtSolveStepper
#define SmSrfPtErrorNormType sc_spsp1_SmSrfPtErrorNormType
#define GetTrianglesDist sc_spsp1_GetTrianglesDist
#define WellBehavedControlNet sc_spsp1_WellBehavedControlNet
#define private public
#define protected public
#include "../../../upstream/source/SMLib/src/SmSurfacePointSolve.cpp"

#include "sc_sm.h"

using sc::flg;
using sc::put_dom;
using sc::put_uv;
using sc::put_vec;
using sc::st;
#undef private
#undef protected

namespace sc_spsp1
{
enum
{
    OP_CTOR = 0,
    OP_CLOSED = 1,
    OP_FUNC = 2,
    OP_TRI_GUESS = 3,
    OP_LEG = 4,
    OP_STRIDE = 5,
    OP_SEAMS_UV = 6,
    OP_SEAMS_CPTS = 7,
    OP_CREASE = 8,
    OP_LOCAL = 9,
    OP_TRI_DIST = 10,
    OP_WELL = 11,
    OP_STEPPER = 12,
    OP_HIT_BOUND = 13,
    OP_CHECK_BOUNDS = 14,
    OP_DROP_ISO = 15,
};

// `SmSTATUS` for an index past the control net: the Rust port's InvalidInput.
constexpr double ST_BAD_INDEX = 1007.0;

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
        double x = num();
        double y = num();
        return SmPoint2d(x, y);
    }
    SmExtent2d domain()
    {
        double a = num();
        double b = num();
        double c = num();
        double d = num();
        return SmExtent2d(a, b, c, d);
    }
};

gw_SURFACE* read_nurb(Rd& rd)
{
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
    return sur;
}

// `oob form nurb`.
SmBSplineSurface* read_bspline(Rd& rd, const SmContext& ctx)
{
    bool oob = rd.flag();
    SmBSplineSurfaceForm form = (SmBSplineSurfaceForm)rd.idx();
    SmBSplineSurface* s = new (ctx) SmBSplineSurface(read_nurb(rd), FALSE, NULL);
    s->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
    s->SetBSplineSurfaceForm(form);
    return s;
}

// A surface `kind ...`: 0 B-spline `oob form nurb`; 1 bounded plane `o x y sx sy D`;
// 2 curve-bounded surface (implicit outer, no boundary curves) over a B-spline block.
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 1)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double sx = rd.num();
        double sy = rd.num();
        SmExtent2d d = rd.domain();
        return new (ctx) SmPlane(o, x, y, SmVector2d(sx, sy), d, &ctx);
    }
    SmBSplineSurface* b = read_bspline(rd, ctx);
    if (kind == 0)
        return b;
    // the curve-bounded surface owns (deletes) its basis
    SmTArray<SmCurve*> c3d(ctx), cuv(ctx);
    return new (ctx) SmCurveBoundedSurface(*b, &c3d, &cuv, TRUE);
}

// `W S`: the surface, wrapped in an SmSTEPSurface owning it when W = 1.
SmSurface* read_wrapped(Rd& rd, const SmContext& ctx)
{
    bool wrap = rd.flag();
    SmSurface* s = read_surface(rd, ctx);
    if (!wrap)
        return s;
    return new (ctx) SmSTEPSurface(*s, TRUE);
}

// The problem: `W S D op test tol`.
struct Problem
{
    SmSurface* srf;
    SmExtent2d dom;
    SmSolverOperationType op;
    SmPoint3d test;
    double tol;
};

Problem read_problem(Rd& rd, const SmContext& ctx)
{
    Problem p;
    p.srf = read_wrapped(rd, ctx);
    p.dom = rd.domain();
    p.op = (SmSolverOperationType)rd.idx();
    p.test = rd.vec();
    p.tol = rd.num();
    return p;
}

// `status has_bspl tight loose test(3) dir(3)` of the base class data.
void put_base(std::vector<double>& o, const SmSurfacePointSolver& s)
{
    o.push_back(0.0);
    o.push_back(flg(s.m_pBSplSrf != NULL));
    o.push_back(s.m_dTightTol);
    o.push_back(s.m_dLooseTol);
    put_vec(o, s.m_sTestPt);
    put_vec(o, s.m_sDirVec);
}

// The control net of the solver's B-spline, or false when it has none.
bool net(const SmSurfacePointSolver& s, ULONG& nu, ULONG& nv, double*& cpts, SmBoolean& rational)
{
    if (s.m_pBSplSrf == NULL)
        return false;
    if (s.m_pBSplSrf->GetControlPointsPointer(nu, nv, cpts) != SM_SUCCESS)
        return false;
    rational = s.m_pBSplSrf->IsRational();
    return true;
}

// `P`: base data.
void op_ctor(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmSurfPtGlobalSolver g(p.srf, p.dom, p.op, p.test, p.tol, NULL, SM_SR_SINGLE);
    put_base(o, g);
}

// `P`: ClosedSurface(U), (V), (U again), (BOTH).
void op_closed(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmSurfPtGlobalSolver g(p.srf, p.dom, p.op, p.test, p.tol, NULL, SM_SR_SINGLE);
    o.push_back(0.0);
    o.push_back(flg(g.ClosedSurface(SM_SP_U) != FALSE));
    o.push_back(flg(g.ClosedSurface(SM_SP_V) != FALSE));
    o.push_back(flg(g.ClosedSurface(SM_SP_U) != FALSE));
    o.push_back(flg(g.ClosedSurface(SM_SP_BOTH) != FALSE));
}

// `P uv`: CalcFuncVal.
void op_func(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmPoint2d uv = rd.uv();
    SmSurfPtGlobalSolver g(p.srf, p.dom, p.op, p.test, p.tol, NULL, SM_SR_SINGLE);
    o.push_back(0.0);
    o.push_back(g.CalcFuncVal(uv));
}

// `P iu iv`: FindPolygonTriangleGuess with the given initial best indices.
void op_tri_guess(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    ULONG iu = (ULONG)rd.idx();
    ULONG iv = (ULONG)rd.idx();
    SmSurfPtGlobalSolver g(p.srf, p.dom, p.op, p.test, p.tol, NULL, SM_SR_SINGLE);
    ULONG nu = 0, nv = 0;
    double* cpts = NULL;
    SmBoolean rational = FALSE;
    if (net(g, nu, nv, cpts, rational) && (iu >= nu || iv >= nv))
    {
        o.push_back(ST_BAD_INDEX);
        return;
    }
    SmPoint2d uv;
    double best = 0.0, ave = 0.0;
    SmBoolean susp = FALSE;
    SmStatus s = g.FindPolygonTriangleGuess(uv, iu, iv, best, ave, susp);
    o.push_back(st(s));
    if (s != SM_SUCCESS)
        return;
    put_uv(o, uv);
    o.push_back((double)iu);
    o.push_back((double)iv);
    o.push_back(best);
    o.push_back(ave);
    o.push_back(flg(susp != FALSE));
}

// `P iu iv`: RefineGuessPolygonLeg.
void op_leg(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    ULONG iu = (ULONG)rd.idx();
    ULONG iv = (ULONG)rd.idx();
    SmSurfPtGlobalSolver g(p.srf, p.dom, p.op, p.test, p.tol, NULL, SM_SR_SINGLE);
    ULONG nu = 0, nv = 0;
    double* cpts = NULL;
    SmBoolean rational = FALSE;
    if (net(g, nu, nv, cpts, rational) && (iu >= nu || iv >= nv) &&
        p.op != SM_SO_3D_SIGNED_DIRECTED_MAXIMIZE)
    {
        o.push_back(ST_BAD_INDEX);
        return;
    }
    SmPoint2d uv(0.0, 0.0);
    SmStatus s = g.RefineGuessPolygonLeg(uv, iu, iv);
    o.push_back(st(s));
    if (s != SM_SUCCESS)
        return;
    put_uv(o, uv);
}

// `P uv`: CheckBadGuessStride from a grid point at uv.
void op_stride(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmPoint2d uv = rd.uv();
    SmSurfPtGlobalSolver g(p.srf, p.dom, p.op, p.test, p.tol, NULL, SM_SR_SINGLE);
    SmSurfaceEval pt0(p.srf);
    pt0.SetUV(uv);
    SmSurfaceEval npt(p.srf);
    double f = 0.0;
    SmBoolean b = g.CheckBadGuessStride(pt0, npt, f);
    o.push_back(0.0);
    o.push_back(flg(b != FALSE));
    if (b == FALSE)
        return;
    put_uv(o, npt.GetUV());
    o.push_back(f);
}

// `P uv`: CheckGuessForSeams (UV point).
void op_seams_uv(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmPoint2d uv = rd.uv();
    SmSurfPtGlobalSolver g(p.srf, p.dom, p.op, p.test, p.tol, NULL, SM_SR_SINGLE);
    SmStatus s = g.CheckGuessForSeams(uv);
    o.push_back(st(s));
    put_uv(o, uv);
}

// `P iu iv`: CheckGuessForSeams (control points) over the solver's B-spline net.
void op_seams_cpts(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    ULONG iu = (ULONG)rd.idx();
    ULONG iv = (ULONG)rd.idx();
    SmSurfPtGlobalSolver g(p.srf, p.dom, p.op, p.test, p.tol, NULL, SM_SR_SINGLE);
    ULONG nu = 0, nv = 0;
    double* cpts = NULL;
    SmBoolean rational = FALSE;
    if (!net(g, nu, nv, cpts, rational))
    {
        o.push_back(st(SM_ERR));
        return;
    }
    if (iu >= nu || iv >= nv)
    {
        o.push_back(ST_BAD_INDEX);
        return;
    }
    SmStatus s = g.CheckGuessForSeams(iu, iv, nu, nv, cpts, rational);
    o.push_back(st(s));
    o.push_back((double)iu);
    o.push_back((double)iv);
}

// `P`: GetCreaseSubdomains.
void op_crease(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmSurfPtGlobalSolver g(p.srf, p.dom, p.op, p.test, p.tol, NULL, SM_SR_SINGLE);
    SmTArray<SmExtent2d> list(ctx);
    SmStatus s = g.GetCreaseSubdomains(list);
    o.push_back(st(s));
    o.push_back((double)list.GetSize());
    for (ULONG i = 0; i < list.GetSize(); i++)
        put_dom(o, list[i]);
}

// `P uv tol`: IsLocalMin, IsLocalMax, PushingBoundary (fresh evaluators each).
void op_local(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmPoint2d uv = rd.uv();
    double tol = rd.num();
    SmSurfPtGlobalSolver g(p.srf, p.dom, p.op, p.test, p.tol, NULL, SM_SR_SINGLE);
    o.push_back(0.0);
    {
        SmSurfaceEval e(p.srf);
        e.SetUV(uv);
        o.push_back(flg(g.IsLocalMin(e, tol) != FALSE));
    }
    {
        SmSurfaceEval e(p.srf);
        e.SetUV(uv);
        o.push_back(flg(g.IsLocalMax(e, tol) != FALSE));
    }
    {
        SmSurfaceEval e(p.srf);
        e.SetUV(uv);
        SmSurfParamType which = SM_SP_UNKNOWN;
        SmBoolean b = g.PushingBoundary(e, tol, &which);
        o.push_back(flg(b != FALSE));
        o.push_back((double)which);
    }
}

// `P i j`: GetTrianglesDist over the solver's B-spline net.
void op_tri_dist(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    ULONG i = (ULONG)rd.idx();
    ULONG j = (ULONG)rd.idx();
    SmSurfPtGlobalSolver g(p.srf, p.dom, p.op, p.test, p.tol, NULL, SM_SR_SINGLE);
    ULONG nu = 0, nv = 0;
    double* cpts = NULL;
    SmBoolean rational = FALSE;
    if (!net(g, nu, nv, cpts, rational))
    {
        o.push_back(st(SM_ERR));
        return;
    }
    if (i + 1 >= nu || j + 1 >= nv)
    {
        o.push_back(ST_BAD_INDEX);
        return;
    }
    double u = 0.0, v = 0.0;
    double d = GetTrianglesDist(g.m_sTestPt, cpts, rational, i, j, nu, nv, u, v);
    o.push_back(0.0);
    o.push_back(d);
    o.push_back(u);
    o.push_back(v);
}

// `P i j`: WellBehavedControlNet over the solver's B-spline net.
void op_well(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    ULONG i = (ULONG)rd.idx();
    ULONG j = (ULONG)rd.idx();
    SmSurfPtGlobalSolver g(p.srf, p.dom, p.op, p.test, p.tol, NULL, SM_SR_SINGLE);
    ULONG nu = 0, nv = 0;
    double* cpts = NULL;
    SmBoolean rational = FALSE;
    if (!net(g, nu, nv, cpts, rational))
    {
        o.push_back(st(SM_ERR));
        return;
    }
    if (i >= nu || j >= nv)
    {
        o.push_back(ST_BAD_INDEX);
        return;
    }
    SmBoolean b = WellBehavedControlNet(g.m_sTestPt, cpts, rational, i, j, nu, nv);
    o.push_back(0.0);
    o.push_back(flg(b != FALSE));
}

// `P uv tight uv2`: a stepper at uv: base data, MaxStep, the Eval values, Pos and the
// derivatives; then SetUV(uv2), Clamp and the UV; then a copy and an assignment.
void op_stepper(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmPoint2d uv = rd.uv();
    double tight = rd.num();
    SmPoint2d uv2 = rd.uv();
    SmSurfPtSolveStepper s(p.srf, p.dom, p.test, uv, tight, p.tol, p.op);
    put_base(o, s);
    o.push_back(s.MaxStep());
    put_vec(o, s.Gap());
    put_vec(o, s.GapU());
    put_vec(o, s.GapV());
    o.push_back(s.GapLength());
    o.push_back(s.m_dFVal);
    o.push_back(s.m_dGVal);
    o.push_back(s.m_dFCheck);
    o.push_back(s.m_dGCheck);
    o.push_back(s.m_dSumCheck);
    put_vec(o, s.Pos());
    put_vec(o, s.Su());
    put_vec(o, s.Sv());
    put_vec(o, s.Suu());
    put_vec(o, s.Suv());
    put_vec(o, s.Svv());
    // the copy is not evaluated; the assignment copies the evaluation
    SmSurfPtSolveStepper c(s);
    o.push_back(flg(c.m_bSet != FALSE));
    put_uv(o, c.UV());
    SmSurfPtSolveStepper a(p.srf, p.dom, p.test, uv2, tight, p.tol, p.op);
    a = s;
    o.push_back(flg(a.m_bSet != FALSE));
    o.push_back(a.GapLength());
    s.SetUV(uv2);
    o.push_back(flg(s.m_bSet != FALSE));
    s.Clamp();
    put_uv(o, s.UV());
}

// `P uv k (u v which tob)*k`: HitBound after moving to each uv; the counts after each.
void op_hit_bound(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmPoint2d uv = rd.uv();
    int64_t k = rd.idx();
    SmSurfPtSolveStepper s(p.srf, p.dom, p.test, uv, p.tol, p.tol, p.op);
    o.push_back(0.0);
    for (int64_t i = 0; i < k; i++)
    {
        SmPoint2d at = rd.uv();
        SmSurfParamType which = rd.flag() ? SM_SP_V : SM_SP_U;
        int tob = (int)rd.num();
        s.SetUV(at);
        s.HitBound(which, tob);
        o.push_back((double)s.BoundHitsU());
        o.push_back((double)s.BoundHitsV());
    }
}

// `P curr next D`: CheckBounds.
void op_check_bounds(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmPoint2d curr = rd.uv();
    SmPoint2d next = rd.uv();
    SmExtent2d d = rd.domain();
    SmSurfPtSolveStepper s(p.srf, p.dom, p.test, curr, p.tol, p.tol, p.op);
    int hu = 7, hv = 7;
    SmPoint2d c(0.0, 0.0), cs(0.0, 0.0);
    double scale = -7.0;
    SmBoolean b = s.CheckBounds(curr, next, d, hu, hv, c, cs, scale);
    o.push_back(0.0);
    o.push_back(flg(b != FALSE));
    o.push_back((double)hu);
    o.push_back((double)hv);
    put_uv(o, c);
    put_uv(o, cs);
    o.push_back(scale);
}

// `P uv which crease param`: DropToIsoCurve; the result and the UV.
void op_drop_iso(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmPoint2d uv = rd.uv();
    SmSurfParamType which = rd.flag() ? SM_SP_V : SM_SP_U;
    double crease = rd.num();
    double param = rd.num();
    SmSurfPtSolveStepper s(p.srf, p.dom, p.test, uv, p.tol, p.tol, p.op);
    SmBoolean b = s.DropToIsoCurve(which, crease, param);
    o.push_back(0.0);
    o.push_back(flg(b != FALSE));
    put_uv(o, s.UV());
}
} // namespace sc_spsp1

SC_EXPORT int32_t sc_surface_surface_point_solve_p1_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                                         int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    sc_spsp1::Rd rd{in, 0};
    switch (op)
    {
    case sc_spsp1::OP_CTOR: sc_spsp1::op_ctor(rd, ctx, o); break;
    case sc_spsp1::OP_CLOSED: sc_spsp1::op_closed(rd, ctx, o); break;
    case sc_spsp1::OP_FUNC: sc_spsp1::op_func(rd, ctx, o); break;
    case sc_spsp1::OP_TRI_GUESS: sc_spsp1::op_tri_guess(rd, ctx, o); break;
    case sc_spsp1::OP_LEG: sc_spsp1::op_leg(rd, ctx, o); break;
    case sc_spsp1::OP_STRIDE: sc_spsp1::op_stride(rd, ctx, o); break;
    case sc_spsp1::OP_SEAMS_UV: sc_spsp1::op_seams_uv(rd, ctx, o); break;
    case sc_spsp1::OP_SEAMS_CPTS: sc_spsp1::op_seams_cpts(rd, ctx, o); break;
    case sc_spsp1::OP_CREASE: sc_spsp1::op_crease(rd, ctx, o); break;
    case sc_spsp1::OP_LOCAL: sc_spsp1::op_local(rd, ctx, o); break;
    case sc_spsp1::OP_TRI_DIST: sc_spsp1::op_tri_dist(rd, ctx, o); break;
    case sc_spsp1::OP_WELL: sc_spsp1::op_well(rd, ctx, o); break;
    case sc_spsp1::OP_STEPPER: sc_spsp1::op_stepper(rd, ctx, o); break;
    case sc_spsp1::OP_HIT_BOUND: sc_spsp1::op_hit_bound(rd, ctx, o); break;
    case sc_spsp1::OP_CHECK_BOUNDS: sc_spsp1::op_check_bounds(rd, ctx, o); break;
    case sc_spsp1::OP_DROP_ISO: sc_spsp1::op_drop_iso(rd, ctx, o); break;
    default: return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
