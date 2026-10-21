// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.curve_p8`: SmCurve::GlobalCurveIntersect
// (source/SMLib/src/SmCurve.cpp, source/SMLib/inc/SmCurve.h), called through `SmCurve*`
// on SmBSplineCurve, SmHermiteCurve and SmLine objects (a public member; no copies of
// file-static code are needed).
//
// One entry point, sc_curve_curve_p8_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/curve_curve_p8.rs, which validates every
// input. A curve block is `kind dim ...` (0: SmBSplineCurve `oob n p m cpts(x y z w)
// knots`; 1: SmHermiteCurve `P1 D1 P2 D2`; 2: bounded SmLine `P V d0 d1 scale io`), as in
// unit curve.curve_p6. `out` receives the op status (0 = SM_SUCCESS, else the SmStatus
// value) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmExtent1d.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmSolutionArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

#include "sc_sm.h"
#include "sc_gw.h"

using sc::Rd;
using sc::read_nurb;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/curve_curve_p8.rs).
enum : int32_t
{
    OP_GCI = 0,
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }

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

// The transforms `nt (O X Y has_s S)*nt` after a curve block, applied to `c` in order.
void read_transforms(Rd& rd, SmCurve* c)
{
    int64_t nt = rd.idx();
    for (int64_t i = 0; i < nt; i++)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        bool has_s = rd.flag();
        SmVector3d s = rd.vec();
        SmAxis2Placement a2p;
        a2p.SetCanonical(o, x, y);
        c->Transform(a2p, has_s ? &s : NULL);
    }
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

void run(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_GCI:
    {
        SmCurve* c = read_curve(rd, ctx);
        read_transforms(rd, c);
        double a = rd.num();
        double b = rd.num();
        bool same = rd.flag();
        SmCurve* other = c;
        if (!same)
        {
            other = read_curve(rd, ctx);
            read_transforms(rd, other);
        }
        double a2 = rd.num();
        double b2 = rd.num();
        double tol = rd.num();
        bool skip = rd.flag();
        SmSolutionArray sols;
        SmStatus s = c->GlobalCurveIntersect(SmExtent1d(a, b), *other, SmExtent1d(a2, b2), tol,
                                             sols, skip ? TRUE : FALSE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)sols.GetSize());
            for (ULONG i = 0; i < sols.GetSize(); i++)
                put_sol(o, sols[i], c, other);
        }
        if (!same)
            delete other;
        delete c;
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_curve_curve_p8_eval(int32_t op, const double* in, int64_t n_in, double* out,
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
