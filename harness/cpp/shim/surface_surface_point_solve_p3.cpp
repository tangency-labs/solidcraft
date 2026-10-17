// SPDX-License-Identifier: Apache-2.0
// Native differential shim for surface.surface_point_solve_p3.
// The file-local upstream classes and two SmSurface methods are renamed before
// including SmSurfacePointSolve.cpp, as in p1/p2. Surface construction and wire
// formats follow the p2 shim; the five calls below execute unmodified upstream code.
#define GlobalPointSolve_1 sc_spsp3_GlobalPointSolve_1
#define LocalPointSolve_1 sc_spsp3_LocalPointSolve_1

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

#define SmSurfacePointSolver sc_spsp3_SmSurfacePointSolver
#define SmSurfPtGlobalSolver sc_spsp3_SmSurfPtGlobalSolver
#define SmSurfPtSolveStepper sc_spsp3_SmSurfPtSolveStepper
#define SmSrfPtErrorNormType sc_spsp3_SmSrfPtErrorNormType
#define GetTrianglesDist sc_spsp3_GetTrianglesDist
#define WellBehavedControlNet sc_spsp3_WellBehavedControlNet
#define private public
#define protected public
#include "../../../upstream/source/SMLib/src/SmSurfacePointSolve.cpp"

#include "sc_sm.h"

using sc::flg;
using sc::put_uv;
using sc::st;
#undef private
#undef protected

namespace sc_spsp3
{
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
        o.push_back((double)s.m_eSolutionType);
        o.push_back(s.m_vStart.m_adParameters[0]);
        o.push_back(s.m_vStart.m_adParameters[1]);
        o.push_back(s.m_vStart.m_dSolutionValue);
        o.push_back((double)s.m_lNumObjects);
        o.push_back((double)s.m_lNumVariables);
        o.push_back(flg(s.m_apObjects[0] == (const SmObject*)srf));
    }
}


void put_state(std::vector<double>& o, SmSurfPtSolveStepper& s)
{
    put_uv(o, s.UV());
    o.push_back((double)s.BoundHitsU());
    o.push_back((double)s.BoundHitsV());
    o.push_back(flg(s.NRStepSuspect() != FALSE));
    o.push_back((double)s.IterCount());
}

void put_solution(std::vector<double>& o, const SmSolution& s, const SmSurface* surface)
{
    o.push_back((double)s.m_eSolutionType);
    o.push_back(s.m_vStart.m_adParameters[0]);
    o.push_back(s.m_vStart.m_adParameters[1]);
    o.push_back(s.m_vStart.m_dSolutionValue);
    o.push_back((double)s.m_lNumObjects);
    o.push_back((double)s.m_lNumVariables);
    o.push_back(flg(s.m_apObjects[0] == (const SmObject*)surface));
}

void run(int op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    Problem p = read_problem(rd, ctx);
    SmBSplineSurface* b = dynamic_cast<SmBSplineSurface*>(p.srf);
    bool before = b && b->GetOutOfBoundsEnabled();
    if (op == 0)
    {
        bool has_target = rd.flag();
        double target = rd.num();
        SmSolutionRequestedType request = (SmSolutionRequestedType)rd.idx();
        SmSolutionArray solutions;
        SmStatus result = p.srf->sc_spsp3_GlobalPointSolve_1(p.dom, p.op, p.test, p.tol,
            has_target ? &target : NULL, request, solutions);
        o.push_back(st(result));
        put_solutions(o, solutions, p.srf);
        o.push_back(flg(!b || (b->GetOutOfBoundsEnabled() != FALSE) == before));
    }
    else if (op == 1)
    {
        SmPoint2d guess = rd.uv();
        SmBoolean found = FALSE;
        SmSolution solution;
        SmTerminationReasonType reason = SM_TR_STILL_ITERATING;
        int iterations = -1;
        SmStatus result = p.srf->sc_spsp3_LocalPointSolve_1(p.dom, p.op, p.test, guess,
            found, solution, reason, iterations);
        o.push_back(st(result));
        o.push_back(flg(found != FALSE));
        o.push_back((double)reason);
        o.push_back((double)iterations);
        put_solution(o, solution, p.srf);
        o.push_back(flg(!b || (b->GetOutOfBoundsEnabled() != FALSE) == before));
    }
    else
    {
        SmPoint2d uv = rd.uv();
        double tight = rd.num();
        SmPoint2d next_uv = rd.uv();
        int iter = (int)rd.idx();
        int hu = (int)rd.idx(), hv = (int)rd.idx();
        SmSurfPtSolveStepper previous(p.srf, p.dom, p.test, uv, tight, p.tol, p.op);
        previous.SetIterCount(iter);
        previous.m_iBndHitU = hu;
        previous.m_iBndHitV = hv;
        SmSurfPtSolveStepper next(previous);
        next.SetUV(next_uv);
        if (op == 2)
        {
            SmBoolean found = FALSE;
            SmStatus result = next.FindBoundarySolution(found);
            o.push_back(st(result));
            o.push_back(flg(found != FALSE));
        }
        else if (op == 3)
        {
            SmTerminationReasonType reason = SM_TR_STILL_ITERATING;
            SmStatus result = next.FindNextStep(previous, reason);
            o.push_back(st(result));
            o.push_back((double)reason);
        }
        else
        {
            SmBoolean found = TRUE;
            SmBoolean processed = next.CheckCrease(previous, found);
            o.push_back(0.0);
            o.push_back(flg(processed != FALSE));
            o.push_back(flg(found != FALSE));
        }
        put_state(o, next);
    }
    delete p.srf;
}
} // namespace sc_spsp3

SC_EXPORT int32_t sc_surface_surface_point_solve_p3_eval(int32_t op, const double* in,
    int64_t n_in, double* out, int64_t cap, int64_t* out_len)
{
    if (!in || n_in < 1 || !out_len || op < 0 || op > 4) return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    std::vector<double> output;
    sc_spsp3::Rd reader{in, 0};
    sc_spsp3::run(op, reader, ctx, output);
    return sc_copy_out(output.data(), (int64_t)output.size(), out, cap, out_len);
    SC_CATCH
}
