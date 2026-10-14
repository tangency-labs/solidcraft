// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.curve_p9`: SmCurve::GlobalCurveSolve (and through it
// SmCCGlobalSolver::LocalSolve and sm_SolveMinSignedAngle), GlobalCurveSelfIntersect,
// CalculateTightBoundingBox, CalculateTightPseudoBox, IsOnPlane and
// IsOnPositiveSideOfPlanes (source/SMLib/src/SmCurve.cpp, source/SMLib/inc/SmCurve.h),
// called through `SmCurve*` on SmBSplineCurve, SmHermiteCurve, SmLine and SmEllipse objects (public
// members; no copies of file-static code are needed).
//
// One entry point, sc_curve_curve_p9_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/curve_curve_p9.rs, which validates every
// input. A curve block is `kind dim ...` (0: SmBSplineCurve `oob n p m cpts(x y z w)
// knots`; 1: SmHermiteCurve `P1 D1 P2 D2`; 2: bounded SmLine `P V d0 d1 scale io`; 3: SmEllipse
// `C X Y d0 d1 rx ry io`, no source NURB, as in unit curve.curve_p5). `out` receives the op status (0 = SM_SUCCESS, else the SmStatus
// value) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmExtent3d.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmPseudoBox.h>
#include <SmSolutionArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/curve_curve_p9.rs).
enum : int32_t
{
    OP_GCS = 0,
    OP_SELF = 1,
    OP_TBB = 2,
    OP_TPB = 3,
    OP_ON_PLANE = 4,
    OP_POS_SIDE = 5,
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
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }

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

// A curve block (see the file comment).
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
        return new (ctx) SmEllipse(c, x, y, SmExtent1d(d0, d1), rx, ry, dim, &ctx, NULL,
                                   io ? TRUE : FALSE);
    }
    SmVector3d p1 = rd.vec();
    SmVector3d d1 = rd.vec();
    SmVector3d p2 = rd.vec();
    SmVector3d d2 = rd.vec();
    return new (ctx) SmHermiteCurve(p1, d1, p2, d2, dim);
}

// A solution `type nvar nobj o0 o1 value s0 s1 [evalue e0 e1]`; objects `0` = a, `1` = b,
// `2` = another object, `-1` = NULL.
void put_sol(std::vector<double>& o, const SmSolution& s, const void* a, const void* b)
{
    auto id = [&](const void* p) { return p == NULL ? -1.0 : p == a ? 0.0 : p == b ? 1.0 : 2.0; };
    o.push_back((double)s.m_eSolutionType);
    o.push_back((double)s.m_lNumVariables);
    o.push_back((double)s.m_lNumObjects);
    o.push_back(s.m_lNumObjects > 0 ? id(s.m_apObjects[0]) : -1.0);
    o.push_back(s.m_lNumObjects > 1 ? id(s.m_apObjects[1]) : -1.0);
    o.push_back(s.m_vStart.m_dSolutionValue);
    o.push_back(s.m_vStart[0]);
    o.push_back(s.m_lNumVariables > 1 ? s.m_vStart[1] : 0.0);
    if (s.m_eSolutionType == SM_ST_RANGE_OF_VALUES)
    {
        o.push_back(s.m_vEnd.m_dSolutionValue);
        o.push_back(s.m_vEnd[0]);
        o.push_back(s.m_lNumVariables > 1 ? s.m_vEnd[1] : 0.0);
    }
}

void put_sols(std::vector<double>& o, SmStatus s, SmSolutionArray& sols, const void* a,
              const void* b)
{
    o.push_back(st(s));
    if (s == SM_SUCCESS)
    {
        o.push_back((double)sols.GetSize());
        for (ULONG i = 0; i < sols.GetSize(); i++)
            put_sol(o, sols[i], a, b);
    }
}

void put_vec(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

void run(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_GCS:
    {
        SmCurve* c = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        bool same = rd.flag();
        SmCurve* other = same ? c : read_curve(rd, ctx);
        double a2 = rd.num();
        double b2 = rd.num();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        double tol = rd.num();
        bool has_target = rd.flag();
        double target = rd.num();
        int64_t nvec = rd.idx();
        SmVector3d vecs[3];
        for (int64_t i = 0; i < nvec; i++)
            vecs[i] = rd.vec();
        SmSolutionRequestedType req = (SmSolutionRequestedType)rd.idx();
        SmSolutionArray sols;
        SmStatus s = c->GlobalCurveSolve(SmExtent1d(a, b), *other, SmExtent1d(a2, b2), sop, tol,
                                         has_target ? &target : NULL, nvec > 0 ? vecs : NULL, req,
                                         sols);
        put_sols(o, s, sols, c, other);
        if (!same)
            delete other;
        delete c;
        break;
    }
    case OP_SELF:
    {
        SmCurve* c = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        double tol = rd.num();
        SmSolutionArray sols;
        SmStatus s = c->GlobalCurveSelfIntersect(SmExtent1d(a, b), tol, sols);
        put_sols(o, s, sols, c, c);
        delete c;
        break;
    }
    case OP_TBB:
    {
        SmCurve* c = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        SmExtent3d box;
        SmStatus s = c->CalculateTightBoundingBox(SmExtent1d(a, b), &box);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(box.GetUMin());
            o.push_back(box.GetVMin());
            o.push_back(box.GetWMin());
            o.push_back(box.GetUMax());
            o.push_back(box.GetVMax());
            o.push_back(box.GetWMax());
        }
        delete c;
        break;
    }
    case OP_TPB:
    {
        SmCurve* c = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        SmVector3d b0 = rd.vec();
        SmVector3d b1 = rd.vec();
        SmVector3d b2 = rd.vec();
        SmPseudoBox box;
        SmStatus s = c->CalculateTightPseudoBox(SmExtent1d(a, b), b0, b1, b2, box);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            for (ULONG i = 0; i < 3; i++)
                put_vec(o, box.GetBasis(i));
            for (ULONG i = 0; i < 3; i++)
            {
                o.push_back(box.GetInterval(i).GetMin());
                o.push_back(box.GetInterval(i).GetMax());
            }
        }
        delete c;
        break;
    }
    case OP_ON_PLANE:
    {
        SmCurve* c = read_curve(rd, ctx);
        SmPoint3d p = rd.vec();
        SmVector3d n = rd.vec();
        double tol = rd.num();
        bool has_ivl = rd.flag();
        double a = rd.num();
        double b = rd.num();
        SmExtent1d ivl(a, b);
        SmBoolean on = 7;
        double dist = SM_UNDEF_DOUBLE;
        SmStatus s = c->IsOnPlane(p, n, tol, on, dist, has_ivl ? &ivl : NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(on ? 1.0 : 0.0);
            o.push_back(dist);
        }
        delete c;
        break;
    }
    case OP_POS_SIDE:
    {
        SmCurve* c = read_curve(rd, ctx);
        int64_t n = rd.idx();
        SmTArray<SmVector3d> pts;
        SmTArray<SmVector3d> nrms;
        for (int64_t i = 0; i < n; i++)
        {
            pts.Add(rd.vec());
            nrms.Add(rd.vec());
        }
        SmBoolean pos = c->IsOnPositiveSideOfPlanes(pts, nrms);
        o.push_back(0.0);
        o.push_back(pos ? 1.0 : 0.0);
        delete c;
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_curve_curve_p9_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                         int64_t cap, int64_t* out_len)
{
    if (in == nullptr || n_in < 0 || out_len == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    Rd rd{in, 0};
    std::vector<double> o;
    run(op, rd, ctx, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
