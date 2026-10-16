// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surface_point_solve_p2`: the global solver's guesses,
// refinements, seam solutions and Solve, and the local stepper's step adjustment, norms,
// convergence tests, steps and line searches, of the file-local classes of upstream
// source/SMLib/src/SmSurfacePointSolve.cpp.
//
// As the p1 shim (surface_surface_point_solve_p1.cpp), this translation unit compiles the
// upstream source itself with the classes renamed (sc_spsp2_*), SmSurface::GlobalPointSolve_1
// / LocalPointSolve_1 renamed before any header is read, and `private` / `protected` made
// public for the class definitions only.
//
// One entry point, sc_surface_surface_point_solve_p2_eval(op, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/surface_surface_point_solve_p2.rs,
// which validates every input.
#define GlobalPointSolve_1 sc_spsp2_GlobalPointSolve_1
#define LocalPointSolve_1 sc_spsp2_LocalPointSolve_1

#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmCircle.h>
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
#include <SmLine.h>
#include <SmLocalSolve1d.h>
#include <SmNurbsSrf.h>
#include <SmOffsetSurface.h>
#include <SmPlane.h>
#include <SmSTEPSurface.h>
#include <SmSolutionArray.h>
#include <SmSurfOfRevolution.h>
#include <SmSurface.h>
#include <SmSurfaceCache.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

#define SmSurfacePointSolver sc_spsp2_SmSurfacePointSolver
#define SmSurfPtGlobalSolver sc_spsp2_SmSurfPtGlobalSolver
#define SmSurfPtSolveStepper sc_spsp2_SmSurfPtSolveStepper
#define SmSrfPtErrorNormType sc_spsp2_SmSrfPtErrorNormType
#define GetTrianglesDist sc_spsp2_GetTrianglesDist
#define WellBehavedControlNet sc_spsp2_WellBehavedControlNet
#define private public
#define protected public
#include "../../../upstream/source/SMLib/src/SmSurfacePointSolve.cpp"

#include "sc_sm.h"

using sc::flg;
using sc::put_uv;
using sc::st;
#undef private
#undef protected

namespace sc_spsp2
{
enum
{
    OP_ROUGH = 0,
    OP_POLY = 1,
    OP_REFINE = 2,
    OP_PATCH = 3,
    OP_GRID = 4,
    OP_SEAMS = 5,
    OP_SOLVE = 6,
    OP_ADJUST = 7,
    OP_NORMS = 8,
    OP_CONV = 9,
    OP_PUSH = 10,
    OP_SNAP = 11,
    OP_STEP = 12,
    OP_LINE = 13,
    OP_LINE_B = 14,
    OP_REV_INFO = 15,
};

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

// Reaches the protected SmSurfOfRevolution constructor (as the surf_of_revolution_p1 shim).
struct RevPeek : SmSurfOfRevolution
{
    RevPeek(SmBSplineCurve* c, const SmPoint3d& o, const SmVector3d& x, const SmVector3d& y,
            const SmExtent2d& d, SmBoolean swap)
        : SmSurfOfRevolution(c, o, x, y, d, swap)
    {
    }
};

// `n p m`, `n+1` points `x y z w`, `m+1` knots: SmBSplineCurve(gw_CURVE*, 3, FALSE).
SmBSplineCurve* read_curve(Rd& rd, const SmContext& ctx)
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
    return new (ctx) SmBSplineCurve(cur, 3, FALSE);
}

// A generator `kind ...` (layout of the surf_of_revolution_p1 shim): 0 a B-spline curve,
// 1 a line `a[3] b[3]` (its NURB made), 2 a circle `c[3] x[3] y[3] d0 d1 r`.
SmBSplineCurve* read_gen(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 0)
        return read_curve(rd, ctx);
    if (kind == 1)
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine* l = new (ctx) SmLine(a, b, 3, &ctx);
        l->GetOrCreateGwNurbPointer();
        return l;
    }
    SmPoint3d c = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    double d0 = rd.num();
    double d1 = rd.num();
    double r = rd.num();
    return new (ctx) SmCircle(c, x, y, SmExtent1d(d0, d1), r, 3, &ctx);
}

// A surface of revolution `mode gen oob swap` then for mode 0 (CreateCanonical) `P[3]
// A[3]`, for mode 1 (the constructor and MakeNurb) `O[3] X[3] Y[3] D[4]` (layout of the
// surf_of_revolution_p1 shim). Writes the construction status; NULL on failure.
SmSurfOfRevolution* read_rev(Rd& rd, const SmContext& ctx, SmStatus& stt)
{
    int64_t mode = rd.idx();
    SmBSplineCurve* gen = read_gen(rd, ctx);
    bool oob = rd.flag();
    bool swap = rd.flag();
    SmSurfOfRevolution* s = NULL;
    if (mode == 0)
    {
        SmPoint3d p = rd.vec();
        SmVector3d a = rd.vec();
        stt = SmSurfOfRevolution::CreateCanonical(ctx, gen, p, a, s, swap ? TRUE : FALSE);
    }
    else
    {
        SmPoint3d org = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        SmExtent2d d = rd.domain();
        s = new (ctx) RevPeek(gen, org, x, y, d, swap ? TRUE : FALSE);
        stt = s->MakeNurb();
    }
    if (stt != SM_SUCCESS)
    {
        if (s)
            delete s; // owns gen
        else
            delete gen;
        return NULL;
    }
    s->SetOutOfBoundsEnabled(oob ? TRUE : FALSE); // the surface and its generator
    return s;
}

// The construction of a surface of revolution failed (OP_REV_INFO reports it first).
struct BuildFailed
{
};

// A surface `kind ...`: 0 B-spline `oob form nurb`; 1 bounded plane `o x y sx sy D`;
// 2 curve-bounded surface (implicit outer, no boundary curves) over a B-spline block;
// 3 surface of revolution (read_rev; throws BuildFailed when it cannot be built).
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 3)
    {
        SmStatus stt = SM_SUCCESS;
        SmSurfOfRevolution* r = read_rev(rd, ctx, stt);
        if (r == NULL)
            throw BuildFailed();
        return r;
    }
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

// `(u v value nobj nvar obj0_is_srf)` of each solution, after their count.
void put_solutions(std::vector<double>& o, SmSolutionArray& sols, const SmSurface* srf)
{
    o.push_back((double)sols.GetSize());
    for (ULONG i = 0; i < sols.GetSize(); i++)
    {
        const SmSolution& s = sols[i];
        o.push_back(s.m_vStart.m_adParameters[0]);
        o.push_back(s.m_vStart.m_adParameters[1]);
        o.push_back(s.m_vStart.m_dSolutionValue);
        o.push_back((double)s.m_lNumObjects);
        o.push_back((double)s.m_lNumVariables);
        o.push_back(flg(s.m_apObjects[0] == (const SmObject*)srf));
    }
}

// `P`: FindRoughGuess.
void op_rough(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmSurfPtGlobalSolver g(p.srf, p.dom, p.op, p.test, p.tol, NULL, SM_SR_SINGLE);
    SmPoint2d uv(0.0, 0.0);
    ULONG iu = 7, iv = 7;
    SmStatus s = g.FindRoughGuess(uv, iu, iv);
    o.push_back(st(s));
    if (s != SM_SUCCESS)
        return;
    put_uv(o, uv);
    o.push_back((double)iu);
    o.push_back((double)iv);
}

// `P iu iv`: FindPolygonGuess from the given best indices.
void op_poly(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    ULONG iu = (ULONG)rd.idx();
    ULONG iv = (ULONG)rd.idx();
    SmSurfPtGlobalSolver g(p.srf, p.dom, p.op, p.test, p.tol, NULL, SM_SR_SINGLE);
    SmPoint2d uv(0.0, 0.0);
    SmBoolean susp = FALSE;
    SmStatus s = g.FindPolygonGuess(uv, iu, iv, susp);
    o.push_back(st(s));
    if (s != SM_SUCCESS)
        return;
    put_uv(o, uv);
    o.push_back((double)iu);
    o.push_back((double)iv);
    o.push_back(flg(susp != FALSE));
}

// `P uv`: RefineGuess.
void op_refine(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmPoint2d uv = rd.uv();
    SmSurfPtGlobalSolver g(p.srf, p.dom, p.op, p.test, p.tol, NULL, SM_SR_SINGLE);
    ULONG iu = 0, iv = 0;
    SmStatus s = g.RefineGuess(uv, iu, iv);
    o.push_back(st(s));
    put_uv(o, uv);
}

// `P sub nu nv uv best`: RefinePatchByGrid.
void op_patch(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmExtent2d sub = rd.domain();
    int nu = (int)rd.idx();
    int nv = (int)rd.idx();
    SmPoint2d uv = rd.uv();
    double best = rd.num();
    SmSurfPtGlobalSolver g(p.srf, p.dom, p.op, p.test, p.tol, NULL, SM_SR_SINGLE);
    SmStatus s = g.RefinePatchByGrid(sub, nu, nv, uv, best);
    o.push_back(st(s));
    put_uv(o, uv);
    o.push_back(best);
}

// `P uv iu iv`: RefineGuessByGrid.
void op_grid(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmPoint2d uv = rd.uv();
    ULONG iu = (ULONG)rd.idx();
    ULONG iv = (ULONG)rd.idx();
    SmSurfPtGlobalSolver g(p.srf, p.dom, p.op, p.test, p.tol, NULL, SM_SR_SINGLE);
    SmStatus s = g.RefineGuessByGrid(uv, iu, iv);
    o.push_back(st(s));
    put_uv(o, uv);
}

// `P uv value`: CheckSeams of the solution (u, v) on the surface with that value.
void op_seams(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmPoint2d uv = rd.uv();
    double value = rd.num();
    SmSurfPtGlobalSolver g(p.srf, p.dom, p.op, p.test, p.tol, NULL, SM_SR_SINGLE);
    SmSolution sol;
    sol.m_lNumVariables = 2;
    sol.m_lNumObjects = 1;
    sol.m_apObjects[0] = p.srf;
    sol.m_vStart[0] = uv.x;
    sol.m_vStart[1] = uv.y;
    sol.m_vStart.m_dSolutionValue = value;
    SmSolutionArray sols;
    SmStatus s = g.CheckSeams(g.m_dTightTol, sol, sols);
    o.push_back(st(s));
    put_solutions(o, sols, p.srf);
}

// `P`: Solve.
void op_solve(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmSurfPtGlobalSolver g(p.srf, p.dom, p.op, p.test, p.tol, NULL, SM_SR_SINGLE);
    SmSolutionArray sols;
    SmStatus s = g.Solve(sols);
    o.push_back(st(s));
    if (s != SM_SUCCESS)
        return;
    put_solutions(o, sols, p.srf);
}

// `HitBound(which, sign)` |n| times.
void hits(SmSurfPtSolveStepper& s, SmSurfParamType which, int64_t n)
{
    int sign = n < 0 ? -1 : 1;
    for (int64_t i = 0; i < (n < 0 ? -n : n); i++)
        s.HitBound(which, sign);
}

// The stepper of `P uv0 tight`.
SmSurfPtSolveStepper read_stepper(Rd& rd, const Problem& p)
{
    SmPoint2d uv0 = rd.uv();
    double tight = rd.num();
    return SmSurfPtSolveStepper(p.srf, p.dom, p.test, uv0, tight, p.tol, p.op);
}

// `UV BoundHitsU BoundHitsV NRStepSuspect IterCount` of a stepper.
void put_state(std::vector<double>& o, SmSurfPtSolveStepper& s)
{
    put_uv(o, s.UV());
    o.push_back((double)s.BoundHitsU());
    o.push_back((double)s.BoundHitsV());
    o.push_back(flg(s.NRStepSuspect() != FALSE));
    o.push_back((double)s.IterCount());
}

// `P uv0 tight uv1 hu hv`: AdjustStep of a copy of the stepper at uv0 (after hu / hv
// boundary hits) moved to uv1.
void op_adjust(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmSurfPtSolveStepper prev = read_stepper(rd, p);
    SmPoint2d uv1 = rd.uv();
    int64_t hu = rd.idx();
    int64_t hv = rd.idx();
    hits(prev, SM_SP_U, hu);
    hits(prev, SM_SP_V, hv);
    SmSurfPtSolveStepper next(prev);
    next.SetUV(uv1);
    SmBoolean adjusted = FALSE;
    double scale = -7.0;
    SmStatus s = next.AdjustStep(prev, adjusted, scale);
    o.push_back(st(s));
    o.push_back(flg(adjusted != FALSE));
    o.push_back(scale);
    put_state(o, next);
}

// `P uv0 tight`: the nine ErrorNorms, IsSingular.
void op_norms(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmSurfPtSolveStepper s = read_stepper(rd, p);
    o.push_back(0.0);
    for (int n = 0; n <= (int)SM_SPN_GAP; n++)
        o.push_back(s.ErrorNorm((SmSrfPtErrorNormType)n));
    SmSurfParamType which = SM_SP_UNKNOWN;
    SmBoolean b = s.IsSingular(&which);
    o.push_back(flg(b != FALSE));
    o.push_back((double)which);
}

// `P uv0 tight ctol`: Converged(ctol), Converged(TRUE), Converged(FALSE).
void op_conv(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmSurfPtSolveStepper s = read_stepper(rd, p);
    double tol = rd.num();
    o.push_back(0.0);
    o.push_back(flg(s.Converged(tol) != FALSE));
    o.push_back(flg(s.Converged((SmBoolean)TRUE) != FALSE));
    o.push_back(flg(s.Converged((SmBoolean)FALSE) != FALSE));
}

// `P uv0 tight ptol`: PushingBoundary(ptol), ConvergedOnBoundary(ptol) (fresh steppers).
void op_push(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmSurfPtSolveStepper s = read_stepper(rd, p);
    double tol = rd.num();
    SmSurfPtSolveStepper c(s);
    o.push_back(0.0);
    SmSurfParamType which = SM_SP_NEITHER;
    SmBoolean b = s.PushingBoundary(tol, &which);
    o.push_back(flg(b != FALSE));
    o.push_back((double)which);
    which = SM_SP_NEITHER;
    b = c.ConvergedOnBoundary(tol, &which);
    o.push_back(flg(b != FALSE));
    o.push_back((double)which);
}

// `P uv0 tight stol`: SnapToBoundaries(stol).
void op_snap(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmSurfPtSolveStepper s = read_stepper(rd, p);
    double tol = rd.num();
    SmBoolean b = s.SnapToBoundaries(tol);
    o.push_back(0.0);
    o.push_back(flg(b != FALSE));
    put_state(o, s);
}

// `P uv0 tight plane`: Step of a copy of the stepper at uv0, from it.
void op_step(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmSurfPtSolveStepper prev = read_stepper(rd, p);
    bool plane = rd.flag();
    SmSurfPtSolveStepper next(prev);
    SmTerminationReasonType r = next.Step(prev, plane ? TRUE : FALSE);
    o.push_back(0.0);
    o.push_back((double)r);
    put_state(o, next);
}

// `P uv0 tight uv1`: LineSearch of a copy of the stepper at uv0 moved to uv1, from it.
void op_line(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmSurfPtSolveStepper prev = read_stepper(rd, p);
    SmPoint2d uv1 = rd.uv();
    SmSurfPtSolveStepper next(prev);
    next.SetUV(uv1);
    SmStatus s = next.LineSearch(prev);
    o.push_back(st(s));
    put_state(o, next);
}

// `P uv0 tight uv1 norm`: LineSearchBracketted as op_line.
void op_line_b(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmSurfPtSolveStepper prev = read_stepper(rd, p);
    SmPoint2d uv1 = rd.uv();
    SmSrfPtErrorNormType norm = (SmSrfPtErrorNormType)rd.idx();
    SmSurfPtSolveStepper next(prev);
    next.SetUV(uv1);
    SmStatus s = next.LineSearchBracketted(prev, norm);
    o.push_back(st(s));
    put_state(o, next);
}
// `W 3 R`: builds the surface of revolution R (wrapped when W = 1); writes the construction
// status, then on success the natural UV domain of the (wrapped) surface and the control
// point counts of the surface of revolution.
void op_rev_info(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    bool wrap = rd.flag();
    rd.idx(); // kind 3
    SmStatus stt = SM_SUCCESS;
    SmSurfOfRevolution* r = read_rev(rd, ctx, stt);
    o.push_back(st(stt));
    if (r == NULL)
        return;
    double nu = (double)r->GetNumberControlPoints(SM_SP_U);
    double nv = (double)r->GetNumberControlPoints(SM_SP_V);
    SmSurface* s = wrap ? (SmSurface*)new (ctx) SmSTEPSurface(*r, TRUE) : (SmSurface*)r;
    SmExtent2d d = s->GetNaturalUVDomain();
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
    o.push_back(nu);
    o.push_back(nv);
    delete s;
}
} // namespace sc_spsp2

SC_EXPORT int32_t sc_surface_surface_point_solve_p2_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                                         int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    sc_spsp2::Rd rd{in, 0};
    switch (op)
    {
    case sc_spsp2::OP_ROUGH: sc_spsp2::op_rough(rd, ctx, o); break;
    case sc_spsp2::OP_POLY: sc_spsp2::op_poly(rd, ctx, o); break;
    case sc_spsp2::OP_REFINE: sc_spsp2::op_refine(rd, ctx, o); break;
    case sc_spsp2::OP_PATCH: sc_spsp2::op_patch(rd, ctx, o); break;
    case sc_spsp2::OP_GRID: sc_spsp2::op_grid(rd, ctx, o); break;
    case sc_spsp2::OP_SEAMS: sc_spsp2::op_seams(rd, ctx, o); break;
    case sc_spsp2::OP_SOLVE: sc_spsp2::op_solve(rd, ctx, o); break;
    case sc_spsp2::OP_ADJUST: sc_spsp2::op_adjust(rd, ctx, o); break;
    case sc_spsp2::OP_NORMS: sc_spsp2::op_norms(rd, ctx, o); break;
    case sc_spsp2::OP_CONV: sc_spsp2::op_conv(rd, ctx, o); break;
    case sc_spsp2::OP_PUSH: sc_spsp2::op_push(rd, ctx, o); break;
    case sc_spsp2::OP_SNAP: sc_spsp2::op_snap(rd, ctx, o); break;
    case sc_spsp2::OP_STEP: sc_spsp2::op_step(rd, ctx, o); break;
    case sc_spsp2::OP_LINE: sc_spsp2::op_line(rd, ctx, o); break;
    case sc_spsp2::OP_LINE_B: sc_spsp2::op_line_b(rd, ctx, o); break;
    case sc_spsp2::OP_REV_INFO: sc_spsp2::op_rev_info(rd, ctx, o); break;
    default: return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
