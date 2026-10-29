// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nurbs.b_spline_curve_p3`.
// Wraps the SmBSplineCurve members owned by the unit (source/SMLib/src/SmBSplineCurve.cpp,
// source/SMLib/inc/SmBSplineCurve.h): CreateByScaleTransRot, HasInternalPole, IsSegmentLine,
// GetControlPoint, IsOnPlane, JoinWith, SubdivideAtDiscontinuity, TrimGap, AssertValid and
// AssertNoPeriodicJump.
//
// One entry point, sc_nurbs_b_spline_curve_p3_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/nurbs_b_spline_curve_p3.rs, which validates every
// input. Every op starts with a curve block `has_nurb dim oob [n p m cpts knots]` (`this`: with a
// NURB the fast constructor owning it, else SmBSplineCurve(lDim)). `out` receives the op's
// result record, then the state of `this`. A curve state is: dim oob borrowed knot_type form
// has_nurb [n p m cpts(x y z w) knots].
#include "sc_shim.h"

#include <SmAssertArray.h>
#include <SmBSplineCurve.h>
#include <SmContext.h>
#include <SmCurveTypes.h>
#include <SmExtent1d.h>
#include <SmNurbsCrv.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <cmath>
#include <limits>
#include <vector>

#include "sc_sm.h"
#include "sc_gw.h"

using sc::Rd;
using sc::put_vec;
using sc::read_nurb;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/nurbs_b_spline_curve_p3.rs).
enum : int32_t
{
    OP_SCALE = 0,
    OP_POLE = 1,
    OP_SEGMENT = 2,
    OP_CP = 3,
    OP_ON_PLANE = 4,
    OP_JOIN = 5,
    OP_SUBDIVIDE = 6,
    OP_GAP = 7,
    OP_ASSERT = 8,
    OP_JUMP = 9,
};

const double NaN = std::numeric_limits<double>::quiet_NaN();

// Reads the protected STEP flags through pointers to members named in a derived class.
struct Peek : SmBSplineCurve
{
    static SmKnotType SmBSplineCurve::*kt() { return &Peek::m_eKnotType; }
    static SmBSplineCurveForm SmBSplineCurve::*form() { return &Peek::m_eBSplineCurveForm; }
};

// A curve block `has_nurb dim oob [curve]`.
SmBSplineCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    bool has = rd.flag();
    ULONG dim = (ULONG)rd.idx();
    bool oob = rd.flag();
    SmBSplineCurve* c = has ? new (ctx) SmBSplineCurve(read_nurb(rd), dim, FALSE)
                            : new (ctx) SmBSplineCurve(dim);
    c->SetOutOfBoundsEnabled(oob ? TRUE : FALSE);
    return c;
}

void put_state(std::vector<double>& o, const SmBSplineCurve* c)
{
    o.push_back((double)c->GetDim());
    o.push_back(c->IsOutOfBoundsEnabled() ? 1.0 : 0.0);
    o.push_back(c->IsNurbBorrowed() ? 1.0 : 0.0);
    o.push_back((double)(c->*Peek::kt()));
    o.push_back((double)(c->*Peek::form()));
    const gw_CURVE* cur = c->GetGwNurbPointer();
    o.push_back(cur ? 1.0 : 0.0);
    if (!cur)
        return;
    gw_INDEX n = cur->pol->n;
    gw_INDEX m = cur->knt->m;
    o.push_back((double)n);
    o.push_back((double)cur->p);
    o.push_back((double)m);
    for (gw_INDEX i = 0; i <= n; i++)
    {
        o.push_back(cur->pol->Pw[i].x);
        o.push_back(cur->pol->Pw[i].y);
        o.push_back(cur->pol->Pw[i].z);
        o.push_back(cur->pol->Pw[i].w);
    }
    for (gw_INDEX i = 0; i <= m; i++)
        o.push_back(cur->knt->U[i]);
}

void curve_op(int32_t op, Rd& rd, const SmContext& ctx, SmBSplineCurve* c, std::vector<double>& o)
{
    switch (op)
    {
    case OP_SCALE:
    {
        SmVector3d sv = rd.vec();
        SmVector3d ev = rd.vec();
        double deg = rd.num();
        SmBSplineCurve* r = NULL;
        SmStatus s = c->CreateByScaleTransRot(ctx, sv, ev, deg, r);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(r ? 1.0 : 0.0);
            if (r)
                put_state(o, r);
        }
        delete r;
        break;
    }
    case OP_POLE:
        o.push_back(0.0);
        o.push_back((double)c->HasInternalPole(rd.num()));
        break;
    case OP_SEGMENT:
    {
        double a = rd.num();
        double b = rd.num();
        ULONG ns = (ULONG)rd.idx();
        double tol = rd.num();
        SmExtent1d ivl(a, b);
        SmPoint3d p(NaN, NaN, NaN);
        SmVector3d v(NaN, NaN, NaN);
        SmBoolean r = c->IsSegmentLine(ivl, ns, tol, p, v);
        // the SmBoolean, or an SmStatus passed through it by SER
        if (r == FALSE || r == TRUE)
        {
            o.push_back(0.0);
            o.push_back((double)r);
            put_vec(o, p);
            put_vec(o, v);
        }
        else
            o.push_back((double)r);
        break;
    }
    case OP_CP:
    {
        SmControlPointFormType form = (SmControlPointFormType)rd.idx();
        ULONG index = (ULONG)rd.idx();
        SmPoint3d p;
        double w = 0.0;
        SmStatus s = c->GetControlPoint(form, index, p, w);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            put_vec(o, p);
            o.push_back(w);
        }
        break;
    }
    case OP_ON_PLANE:
    {
        SmPoint3d pp = rd.vec();
        SmVector3d pn = rd.vec();
        double tol = rd.num();
        bool has_ivl = rd.flag();
        double a = rd.num();
        double b = rd.num();
        SmExtent1d ivl(a, b);
        SmBoolean on = FALSE;
        double maxd = NaN;
        SmStatus s = c->IsOnPlane(pp, pn, tol, on, maxd, has_ivl ? &ivl : NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(on ? 1.0 : 0.0);
            o.push_back(maxd);
        }
        break;
    }
    case OP_JOIN:
    {
        bool this_end = rd.flag();
        SmBSplineCurve* other = read_curve(rd, ctx);
        bool other_end = rd.flag();
        bool has_tol = rd.flag();
        double tol = rd.num();
        SmStatus s = c->JoinWith(this_end ? 1 : 0, other, other_end ? 1 : 0, has_tol ? &tol : NULL);
        o.push_back(st(s));
        put_state(o, other);
        delete other;
        break;
    }
    case OP_SUBDIVIDE:
    {
        SmContinuityType ct = (SmContinuityType)rd.idx();
        double ang = rd.num();
        SmTArray<SmBSplineCurve*> parts;
        SmStatus s = c->SubdivideAtDiscontinuity(ctx, ct, parts, ang);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)parts.GetSize());
            for (ULONG i = 0; i < parts.GetSize(); i++)
                put_state(o, parts[i]);
        }
        for (ULONG i = 0; i < parts.GetSize(); i++)
            delete parts[i];
        break;
    }
    case OP_GAP:
    {
        SmBSplineCurve* other = read_curve(rd, ctx);
        int fix = (int)rd.idx();
        double dmin = rd.num();
        int r = c->TrimGap(other, fix, dmin);
        o.push_back(0.0);
        o.push_back((double)r);
        put_state(o, other);
        delete other;
        break;
    }
    case OP_ASSERT:
    {
        bool given = rd.flag();
        SmAssertArray list;
        SmBoolean ok = c->AssertValid(&list, given ? SM_LEVEL_GIVEN : SM_LEVEL_0, SM_NO_WALK, NULL);
        o.push_back(0.0);
        o.push_back(ok ? 1.0 : 0.0);
        o.push_back((double)list.GetSize());
        for (ULONG i = 0; i < list.GetSize(); i++)
        {
            const SmAssertReport* r = list[i];
            o.push_back((double)r->GetTestIndex());
            o.push_back((double)r->GetTol3d());
            o.push_back(r->GetValue());
            o.push_back(r->GetOwnerParam().x);
            o.push_back(r->GetOwnerParam().y);
        }
        break;
    }
    case OP_JUMP:
    {
        bool closed_u = rd.flag();
        bool closed_v = rd.flag();
        double u0 = rd.num();
        double v0 = rd.num();
        double u1 = rd.num();
        double v1 = rd.num();
        SmExtent2d dom(SmPoint2d(u0, v0), SmPoint2d(u1, v1));
        SmBoolean ok = c->AssertNoPeriodicJump(closed_u ? TRUE : FALSE, closed_v ? TRUE : FALSE, dom);
        o.push_back(0.0);
        o.push_back(ok ? 1.0 : 0.0);
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_nurbs_b_spline_curve_p3_eval(int32_t op, const double* in, int64_t n_in,
                                                  double* out, int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    SmBSplineCurve* c = read_curve(rd, ctx);
    curve_op(op, rd, ctx, c, o);
    put_state(o, c);
    delete c;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
