// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nurbs.b_spline_curve_n_lib`.
// Wraps the SmBSplineCurve members of source/SMLib/src/SmBSplineCurveNLib.cpp owned by the unit:
// ApproximateArc (sm_ApproxCircArc), ApproximateRationalCurve, CreateHelixSegment, both
// CreateExtendedCurve overloads, CreateInterpolatingCurve, DampenKnotSpacing, DegreeReduction,
// InsertOneKnot, SetIntersectionCurveThroughPoint (with B-spline UV trim surfaces), EquallySpacedPoints,
// InsertKnots, InterpolatePoints, MakeCurvesCompatible, DeformToEndPointTargets,
// ReparametrizeWithArcLength, Scale(double), ScaleKnotVector, SyncronizeKnotsOfCurves and Trim.
//
// One entry point, sc_nurbs_b_spline_curve_n_lib_eval(op, in, n_in, out, cap, out_len). Layouts
// are documented in crates/solidcore-ffi/src/units/nurbs_b_spline_curve_n_lib.rs, which validates
// every input. Curve ops start with a curve block `dim oob n p m cpts knots` (`this`, built with
// the fast constructor owning its NURB); static ops have none. `out` receives the op status
// (0 = SM_SUCCESS, else the SmStatus value), the op's outputs, then for curve ops the state of
// `this`. A curve state is: dim oob borrowed knot_type form has_nurb [n p m cpts(x y z w) knots].
#include "sc_shim.h"

#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurveTypes.h>
#include <SmExtent1d.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

#include "sc_sm.h"
#include "sc_gw.h"

using sc::Rd;
using sc::put_vec;
using sc::read_nurb;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/nurbs_b_spline_curve_n_lib.rs).
enum : int32_t
{
    OP_ARC = 0,
    OP_RATIONAL = 1,
    OP_HELIX = 2,
    OP_EXT_PARAM = 3,
    OP_EXT_DIST = 4,
    OP_INTERP_CRV = 5,
    OP_DAMPEN = 6,
    OP_DEG_RED = 7,
    OP_INS_ONE = 8,
    OP_THROUGH = 9,
    OP_EVEN = 10,
    OP_INS_KNOTS = 11,
    OP_INTERP_PTS = 12,
    OP_COMPAT = 13,
    OP_DEFORM = 14,
    OP_REPARAM = 15,
    OP_SCALE = 16,
    OP_SCALE_KNOTS = 17,
    OP_SYNC = 18,
    OP_TRIM = 19,
};

// Reads the protected STEP flags through pointers to members named in a derived class.
struct Peek : SmBSplineCurve
{
    static SmKnotType SmBSplineCurve::*kt() { return &Peek::m_eKnotType; }
    static SmBSplineCurveForm SmBSplineCurve::*form() { return &Peek::m_eBSplineCurveForm; }
};

// A curve block `dim oob curve`: SmBSplineCurve(gw_CURVE*, lDim, FALSE).
SmBSplineCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    ULONG dim = (ULONG)rd.idx();
    bool oob = rd.flag();
    SmBSplineCurve* c = new (ctx) SmBSplineCurve(read_nurb(rd), dim, FALSE);
    c->SetOutOfBoundsEnabled(oob ? TRUE : FALSE);
    return c;
}

// Reads `n m p q r s cpts u v` into a surface from sm_AllocateNurbSurface (validated by the
// ffi wrapper).
gw_SURFACE* read_surface_nurb(Rd& rd)
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

// SetCanonical(O, X, Y) of a frame read as 9 numbers.
SmStatus read_frame(Rd& rd, SmAxis2Placement& f)
{
    SmVector3d o = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    return f.SetCanonical(o, x, y);
}

// Status, then (on success) whether there is a new curve and its state; deletes it.
void put_new(std::vector<double>& o, SmStatus s, SmBSplineCurve* c)
{
    o.push_back(st(s));
    if (s == SM_SUCCESS)
    {
        o.push_back(c ? 1.0 : 0.0);
        if (c)
            put_state(o, c);
    }
    delete c;
}

SmContinuityType cont_of(int64_t v)
{
    return (SmContinuityType)v;
}

void points(Rd& rd, SmTArray<SmPoint3d>& a)
{
    int64_t n = rd.idx();
    for (int64_t i = 0; i < n; i++)
        a.Add(rd.vec());
}

// Curve sets: `count`, then `count` curve blocks.
void read_set(Rd& rd, const SmContext& ctx, SmTArray<SmBSplineCurve*>& a)
{
    int64_t n = rd.idx();
    for (int64_t i = 0; i < n; i++)
        a.Add(read_curve(rd, ctx));
}

void put_set(std::vector<double>& o, SmTArray<SmBSplineCurve*>& a)
{
    for (ULONG i = 0; i < a.GetSize(); i++)
    {
        put_state(o, a[i]);
        delete a[i];
    }
}

void static_op(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_ARC:
    {
        ULONG dim = (ULONG)rd.idx();
        bool has_nc = rd.flag();
        ULONG nc = (ULONG)rd.idx();
        SmAxis2Placement f;
        SmStatus fs = read_frame(rd, f);
        double r = rd.num();
        double a0 = rd.num();
        double a1 = rd.num();
        double tol = rd.num();
        if (fs != SM_SUCCESS)
        {
            o.push_back(st(fs));
            break;
        }
        SmBSplineCurve* c = NULL;
        SmStatus s = SmBSplineCurve::ApproximateArc(ctx, dim, has_nc ? &nc : NULL, f, r, a0, a1, tol, c);
        put_new(o, s, c);
        break;
    }
    case OP_HELIX:
    {
        SmAxis2Placement f;
        SmStatus fs = read_frame(rd, f);
        double h = rd.num();
        double r0 = rd.num();
        double r1 = rd.num();
        double turns = rd.num();
        bool right = rd.flag();
        double tol = rd.num();
        if (fs != SM_SUCCESS)
        {
            o.push_back(st(fs));
            break;
        }
        SmBSplineCurve* c = NULL;
        SmStatus s = SmBSplineCurve::CreateHelixSegment(ctx, f, h, r0, r1, turns, right ? TRUE : FALSE,
                                                        tol, c);
        put_new(o, s, c);
        break;
    }
    case OP_INTERP_CRV:
    {
        SmCurveParameterizationType par = (SmCurveParameterizationType)rd.idx();
        ULONG dim = (ULONG)rd.idx();
        ULONG deg = (ULONG)rd.idx();
        SmTArray<SmPoint3d> pts;
        points(rd, pts);
        SmTArray<SmVector3d> vecs;
        points(rd, vecs);
        bool has_more = rd.flag();
        SmTArray<SmVector3d> more;
        points(rd, more);
        bool ignore = rd.flag();
        SmBSplineCurve* c = NULL;
        SmStatus s = SmBSplineCurve::CreateInterpolatingCurve(ctx, par, dim, deg, pts, vecs,
                                                              has_more ? &more : NULL,
                                                              ignore ? TRUE : FALSE, c);
        put_new(o, s, c);
        break;
    }
    case OP_INTERP_PTS:
    {
        SmTArray<SmPoint3d> pts;
        points(rd, pts);
        bool has_params = rd.flag();
        SmTArray<double> prm;
        int64_t np = rd.idx();
        for (int64_t i = 0; i < np; i++)
            prm.Add(rd.num());
        ULONG deg = (ULONG)rd.idx();
        bool has_ts = rd.flag();
        SmVector3d ts = rd.vec();
        bool has_te = rd.flag();
        SmVector3d te = rd.vec();
        bool closed = rd.flag();
        SmInterpolationType par = (SmInterpolationType)rd.idx();
        SmBSplineCurve* c = NULL;
        SmStatus s = SmBSplineCurve::InterpolatePoints(ctx, pts, has_params ? &prm : NULL, deg,
                                                       has_ts ? &ts : NULL, has_te ? &te : NULL,
                                                       closed ? TRUE : FALSE, par, c);
        put_new(o, s, c);
        break;
    }
    case OP_COMPAT:
    case OP_SYNC:
    {
        SmTArray<SmBSplineCurve*> set;
        read_set(rd, ctx, set);
        double tol = rd.num();
        SmStatus s = op == OP_COMPAT ? SmBSplineCurve::MakeCurvesCompatible(set, tol)
                                     : SmBSplineCurve::SyncronizeKnotsOfCurves(set, tol);
        o.push_back(st(s));
        put_set(o, set);
        break;
    }
    default: break;
    }
}

void curve_op(int32_t op, Rd& rd, const SmContext& ctx, SmBSplineCurve* c, std::vector<double>& o)
{
    switch (op)
    {
    case OP_RATIONAL:
    {
        double tol = rd.num();
        SmBSplineCurve* r = NULL;
        SmStatus s = c->ApproximateRationalCurve(ctx, tol, r);
        put_new(o, s, r);
        break;
    }
    case OP_EXT_PARAM:
    {
        double t = rd.num();
        SmContinuityType ct = cont_of(rd.idx());
        bool precise = rd.flag();
        SmBSplineCurve* r = NULL;
        SmStatus s = c->CreateExtendedCurve(ctx, t, ct, r, precise ? TRUE : FALSE);
        put_new(o, s, r);
        break;
    }
    case OP_EXT_DIST:
    {
        double d = rd.num();
        int end = (int)rd.idx();
        SmContinuityType ct = cont_of(rd.idx());
        SmBSplineCurve* r = NULL;
        SmStatus s = c->CreateExtendedCurve(ctx, d, end, ct, r);
        put_new(o, s, r);
        break;
    }
    case OP_DAMPEN:
    {
        double f = rd.num();
        ULONG mult = (ULONG)rd.idx();
        o.push_back(st(c->DampenKnotSpacing(f, mult)));
        break;
    }
    case OP_DEG_RED:
    {
        double tol = rd.num();
        bool mx = rd.flag();
        o.push_back(st(c->DegreeReduction(tol, mx ? TRUE : FALSE)));
        break;
    }
    case OP_INS_ONE:
    {
        double k = rd.num();
        ULONG r = (ULONG)rd.idx();
        o.push_back(st(c->InsertOneKnot(k, r)));
        break;
    }
    case OP_THROUGH:
    {
        double knot = rd.num();
        SmPoint3d p = rd.vec();
        bool has_t = rd.flag();
        SmVector3d t = rd.vec();
        double pct = rd.num();
        SmTArray<SmBSplineCurve*> uvc;
        SmTArray<const SmSurface*> srf;
        SmTArray<SmVector2d*> uvp;
        int64_t nuv = rd.idx();
        std::vector<SmVector2d> uv_pts((size_t)nuv);
        for (int64_t i = 0; i < nuv; i++)
        {
            uvc.Add(read_curve(rd, ctx));
            srf.Add(new (ctx) SmBSplineSurface(read_surface_nurb(rd), FALSE, NULL));
            bool has_uv = rd.flag();
            double u = rd.num();
            double v = rd.num();
            uv_pts[(size_t)i] = SmVector2d(u, v);
            uvp.Add(has_uv ? &uv_pts[(size_t)i] : NULL);
        }
        SmBoolean modified = FALSE;
        SmStatus s = c->SetIntersectionCurveThroughPoint(knot, p, has_t ? &t : NULL, uvc, srf, uvp,
                                                         modified, pct);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(modified ? 1.0 : 0.0);
            o.push_back(knot);
        }
        for (ULONG i = 0; i < uvc.GetSize(); i++)
        {
            put_state(o, uvc[i]);
            delete uvc[i];
            delete srf[i];
        }
        break;
    }
    case OP_EVEN:
    {
        double t0 = rd.num();
        double t1 = rd.num();
        ULONG n = (ULONG)rd.idx();
        double tol = rd.num();
        SmTArray<SmPoint3d> pts;
        SmTArray<double> prm;
        SmStatus s = c->EquallySpacedPoints(t0, t1, n, tol, &pts, &prm);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)pts.GetSize());
            for (ULONG i = 0; i < pts.GetSize(); i++)
                put_vec(o, pts[i]);
            for (ULONG i = 0; i < prm.GetSize(); i++)
                o.push_back(prm[i]);
        }
        break;
    }
    case OP_INS_KNOTS:
    {
        int64_t nk = rd.idx();
        SmTArray<double> k;
        for (int64_t i = 0; i < nk; i++)
            k.Add(rd.num());
        o.push_back(st(c->InsertKnots(k)));
        break;
    }
    case OP_DEFORM:
    {
        bool has_s = rd.flag();
        SmPoint3d ps = rd.vec();
        bool has_e = rd.flag();
        SmPoint3d pe = rd.vec();
        bool has_st = rd.flag();
        SmVector3d ts = rd.vec();
        SmInValueType st_type = (SmInValueType)rd.idx();
        bool has_et = rd.flag();
        SmVector3d te = rd.vec();
        SmInValueType et_type = (SmInValueType)rd.idx();
        SmCurve* nc = NULL;
        SmStatus s = c->DeformToEndPointTargets(has_s ? &ps : NULL, has_e ? &pe : NULL, &nc,
                                                has_st ? &ts : NULL, st_type, has_et ? &te : NULL,
                                                et_type);
        o.push_back(st(s));
        break;
    }
    case OP_REPARAM: o.push_back(st(c->ReparametrizeWithArcLength())); break;
    case OP_SCALE: o.push_back(st(c->Scale(rd.num()))); break;
    case OP_SCALE_KNOTS:
    {
        double a = rd.num();
        double b = rd.num();
        o.push_back(st(c->ScaleKnotVector(a, b)));
        break;
    }
    case OP_TRIM:
    {
        double a = rd.num();
        double b = rd.num();
        int64_t mode = rd.idx();
        SmExtent1d ivl(a, b);
        SmBoolean notify = mode == 0 ? TRUE : mode == 1 ? FALSE : UNSURE;
        SmStatus s = c->Trim(ivl, notify);
        o.push_back(st(s));
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}

bool is_static(int32_t op)
{
    return op == OP_ARC || op == OP_HELIX || op == OP_INTERP_CRV || op == OP_INTERP_PTS ||
           op == OP_COMPAT || op == OP_SYNC;
}
} // namespace

SC_EXPORT int32_t sc_nurbs_b_spline_curve_n_lib_eval(int32_t op, const double* in, int64_t n_in,
                                                     double* out, int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    if (is_static(op))
    {
        static_op(op, rd, ctx, o);
    }
    else
    {
        SmBSplineCurve* c = read_curve(rd, ctx);
        curve_op(op, rd, ctx, c, o);
        put_state(o, c);
        delete c;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
