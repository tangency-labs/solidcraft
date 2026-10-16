// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.curve_p6`: the SmCurve members owned by the unit
// (source/SMLib/src/SmCurve.cpp, source/SMLib/inc/SmCurve.h) called through `SmCurve*` on
// SmBSplineCurve, SmHermiteCurve and SmLine objects: CalculateBoundingBox, IsPlanar,
// DropPoint, GlobalPointSolveSTEP, CurveMaxDistanceBetween, SimpleCoincidenceChecker,
// GlobalCoincidenceChecker and DeformToEndPointTargets (all public members; no copies of
// file-static code are needed).
//
// One entry point, sc_curve_curve_p6_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/curve_curve_p6.rs, which validates every
// input. A curve block is `kind dim ...` (0: SmBSplineCurve `oob n p m cpts(x y z w)
// knots`; 1: SmHermiteCurve `P1 D1 P2 D2`; 2: bounded SmLine `P V d0 d1 scale io`), as in
// unit curve.curve_p3. `out` receives the op status (0 = SM_SUCCESS, else the SmStatus
// value) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveTypes.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmGlobalSolver.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmPolarBox.h>
#include <SmPseudoBox.h>
#include <SmSolutionArray.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

#include "sc_sm.h"
#include "sc_gw.h"

using sc::Rd;
using sc::put_vec;
using sc::read_nurb;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/curve_curve_p6.rs).
enum : int32_t
{
    OP_BBOX = 0,
    OP_IS_PLANAR = 1,
    OP_DROP_POINT = 2,
    OP_GPS_STEP = 3,
    OP_MAX_DIST = 4,
    OP_SIMPLE = 5,
    OP_GLOBAL = 6,
    OP_DEFORM = 7,
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

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
    SmVector3d p1 = rd.vec();
    SmVector3d d1 = rd.vec();
    SmVector3d p2 = rd.vec();
    SmVector3d d2 = rd.vec();
    return new (ctx) SmHermiteCurve(p1, d1, p2, d2, dim);
}

// A solution `type nvar nobj o0 o1 value s0 s1 [evalue e0 e1]`; objects `0` = a, `1` = b,
// `2` = other, `-1` = NULL.
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

// The state of a curve: `kind a b P[3]*7 [n+1 Pw[4] m+1 U]` (kind 0 = B-spline, with its
// NURB; 2 = line; 1 = other).
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    double kind = c->GetType() == SmBSplineCurve_TYPE ? 0.0 : c->GetType() == SmLine_TYPE ? 2.0 : 1.0;
    o.push_back(kind);
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    for (int i = 0; i <= 6; i++)
    {
        SmPoint3d p(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        c->EvaluatePoint(ivl.Evaluate(i / 6.0), p);
        put_vec(o, p);
    }
    if (kind == 0.0)
    {
        gw_CURVE* nurb = ((const SmBSplineCurve*)c)->GetGwNurbPointer();
        o.push_back((double)(nurb->pol->n + 1));
        for (gw_INDEX k = 0; k <= nurb->pol->n; k++)
        {
            o.push_back(nurb->pol->Pw[k].x);
            o.push_back(nurb->pol->Pw[k].y);
            o.push_back(nurb->pol->Pw[k].z);
            o.push_back(nurb->pol->Pw[k].w);
        }
        o.push_back((double)(nurb->knt->m + 1));
        for (gw_INDEX k = 0; k <= nurb->knt->m; k++)
            o.push_back(nurb->knt->U[k]);
    }
}

void run(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_BBOX:
    {
        SmCurve* c = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        bool want_n = rd.flag();
        bool want_p = rd.flag();
        bool want_l = rd.flag();
        bool expand = rd.flag();
        SmExtent3d nb;
        SmPseudoBox pb;
        SmPolarBox pl;
        SmStatus s = c->CalculateBoundingBox(SmExtent1d(a, b), want_n ? &nb : NULL,
                                             want_p ? &pb : NULL, want_l ? &pl : NULL,
                                             expand ? TRUE : FALSE);
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
        delete c;
        break;
    }
    case OP_IS_PLANAR:
    {
        SmCurve* c = read_curve(rd, ctx);
        double tol = rd.num();
        SmPoint3d pt(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        SmVector3d nrm(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        double dev = SM_UNDEF_DOUBLE;
        SmBoolean r = c->IsPlanar(tol, &pt, &nrm, &dev);
        o.push_back(0.0);
        o.push_back(flg(r));
        put_vec(o, pt);
        put_vec(o, nrm);
        o.push_back(dev);
        delete c;
        break;
    }
    case OP_DROP_POINT:
    {
        SmCurve* c = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        SmPoint3d p = rd.vec();
        bool has_vec = rd.flag();
        SmVector3d v = rd.vec();
        double tol = rd.num();
        bool has_guess = rd.flag();
        double guess = rd.num();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        SmBoolean ok = FALSE;
        double param = 0.0, dist = 0.0;
        SmStatus s = c->DropPoint(SmExtent1d(a, b), p, has_vec ? &v : NULL, tol,
                                  has_guess ? &guess : NULL, ok, param, dist, sop);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(ok));
            o.push_back(param);
            o.push_back(dist);
        }
        delete c;
        break;
    }
    case OP_GPS_STEP:
    {
        SmCurve* c = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        bool has_tg = rd.flag();
        double tg = rd.num();
        SmSolutionRequestedType req = (SmSolutionRequestedType)rd.idx();
        SmSolutionArray sols;
        SmStatus s = c->GlobalPointSolveSTEP(SmExtent1d(a, b), sop, p, tol, has_tg ? &tg : NULL,
                                             NULL, req, sols);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)sols.GetSize());
            for (ULONG i = 0; i < sols.GetSize(); i++)
                put_sol(o, sols[i], c, NULL);
        }
        delete c;
        break;
    }
    case OP_MAX_DIST:
    {
        SmCurve* c = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        bool same = rd.flag();
        SmCurve* other = same ? c : read_curve(rd, ctx);
        double s0 = rd.num();
        double s1 = rd.num();
        ULONG nsamp = (ULONG)rd.idx();
        bool has_max = rd.flag();
        double maxd = rd.num();
        double dist = SM_UNDEF_DOUBLE, t1 = SM_UNDEF_DOUBLE, t2 = SM_UNDEF_DOUBLE;
        SmBoolean dir = 7; // sentinel: untouched
        SmStatus s = c->CurveMaxDistanceBetween(SmExtent1d(a, b), *other, s0, s1, nsamp,
                                                has_max ? &maxd : NULL, dist, &t1, &t2, &dir);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(dist);
            o.push_back(t1);
            o.push_back(t2);
            o.push_back(dir == 7 ? 0.0 : 1.0);
            o.push_back(dir == 7 ? 0.0 : flg(dir));
        }
        if (!same)
            delete other;
        delete c;
        break;
    }
    case OP_SIMPLE:
    {
        SmCurve* c = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        SmCurve* other = read_curve(rd, ctx);
        double a2 = rd.num();
        double b2 = rd.num();
        double tol = rd.num();
        bool pb1 = rd.flag();
        bool pb2 = rd.flag();
        SmExtent1d i1(a, b), i2(a2, b2);
        SmPseudoBox box1, box2;
        SmStatus s = SM_SUCCESS;
        if (pb1)
            s = c->CalculateBoundingBox(i1, NULL, &box1, NULL);
        if (s == SM_SUCCESS && pb2)
            s = other->CalculateBoundingBox(i2, NULL, &box2, NULL);
        ULONG n = 0;
        SmSolution sols[2];
        if (s == SM_SUCCESS)
            s = c->SimpleCoincidenceChecker(i1, *other, i2, tol, pb1 ? &box1 : NULL,
                                            pb2 ? &box2 : NULL, n, sols);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)n);
            for (ULONG i = 0; i < n && i < 2; i++)
                put_sol(o, sols[i], c, other);
        }
        delete other;
        delete c;
        break;
    }
    case OP_GLOBAL:
    {
        SmCurve* c = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        SmCurve* other = read_curve(rd, ctx);
        double a2 = rd.num();
        double b2 = rd.num();
        double tol = rd.num();
        bool snap = rd.flag();
        SmBoolean more = FALSE;
        SmSolutionArray sols;
        SmStatus s = c->GlobalCoincidenceChecker(SmExtent1d(a, b), *other, SmExtent1d(a2, b2), tol,
                                                 more, sols, snap ? TRUE : FALSE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(more));
            o.push_back((double)sols.GetSize());
            for (ULONG i = 0; i < sols.GetSize(); i++)
                put_sol(o, sols[i], c, other);
        }
        delete other;
        delete c;
        break;
    }
    case OP_DEFORM:
    {
        SmCurve* c = read_curve(rd, ctx);
        bool has_s = rd.flag();
        SmPoint3d sp = rd.vec();
        bool has_e = rd.flag();
        SmPoint3d ep = rd.vec();
        bool want = rd.flag();
        bool has_st = rd.flag();
        SmVector3d stan = rd.vec();
        SmInValueType stt = (SmInValueType)rd.idx();
        bool has_et = rd.flag();
        SmVector3d etan = rd.vec();
        SmInValueType ett = (SmInValueType)rd.idx();
        SmCurve* nc = NULL;
        SmStatus s = c->DeformToEndPointTargets(has_s ? &sp : NULL, has_e ? &ep : NULL,
                                                want ? &nc : NULL, has_st ? &stan : NULL, stt,
                                                has_et ? &etan : NULL, ett);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(nc ? 1.0 : 0.0);
            put_curve(o, nc ? nc : c);
        }
        delete nc;
        delete c;
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_curve_curve_p6_eval(int32_t op, const double* in, int64_t n_in, double* out,
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
