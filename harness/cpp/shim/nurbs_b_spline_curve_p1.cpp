// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nurbs.b_spline_curve_p1`.
// Wraps the SmBSplineCurve members owned by the unit (source/SMLib/src/SmBSplineCurve.cpp,
// source/SMLib/inc/SmBSplineCurve.h): the constructors, copy, operator==, CalculateContinuities,
// ConvertTo2D/3D, CreateArcFromPoints, CreateCanonical, CreateCircleSegment,
// CreateDegenerateCurve, CreateLineSegment, CreateMirrorCurve, CreatePlaneProjection,
// EditParameterization, EditEndPoint, Evaluate, EvaluatePoint, FairBlendCurveDerivatives,
// FixupKnotVector, GetControlPolygon, GetControlPointsPointer, GetDegree, GetEnds, GetKnots,
// GetNumberNaturalKnots, GetNumberControlPoints, GetGrevilleAbscissa, IsRational,
// GetNaturalInterval, GetSTEPInterval, MakeNurb, RemoveExtraKnots, SetFromGwNurb, Tessellate,
// Transform, IsKindOf, GetType and the inline accessors.
//
// One entry point, sc_nurbs_b_spline_curve_p1_eval(op, in, n_in, out, cap, out_len). `in` holds
// a curve block (how `this` is built) then the op's parameters; layouts are documented in
// crates/solidcore-ffi/src/units/nurbs_b_spline_curve_p1.rs, which validates every input so
// upstream only touches valid memory. `out` receives: the op status (0 = SM_SUCCESS, else the
// SmStatus value), the op's outputs (only on success), then for curve ops the state of `this`.
// A curve state is: dim oob borrowed knot_type form has_nurb [n p m cpts(x y z w) knots].
#include "sc_shim.h"

#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmContext.h>
#include <SmExtent1d.h>
#include <SmNurbsCrv.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

#include "sc_sm.h"

using sc::put_vec;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/nurbs_b_spline_curve_p1.rs).
enum : int32_t
{
    OP_STATE = 0,
    OP_EVALUATE = 1,
    OP_EVALUATE_POINT = 2,
    OP_INTERVALS = 3,
    OP_KNOTS = 4,
    OP_CONTROL_POLYGON = 5,
    OP_CONTROL_POINTS = 6,
    OP_ENDS = 7,
    OP_COUNTS = 8,
    OP_GREVILLE = 9,
    OP_CONVERT_2D = 10,
    OP_CONVERT_3D = 11,
    OP_EDIT_PARAM = 12,
    OP_EDIT_END = 13,
    OP_FIXUP = 14,
    OP_REMOVE_EXTRA = 15,
    OP_SET_FROM_GW = 16,
    OP_TRANSFORM = 17,
    OP_MIRROR = 18,
    OP_PROJECTION = 19,
    OP_EQ = 20,
    OP_IS_KIND_OF = 21,
    OP_TYPE = 22,
    OP_CONTINUITIES = 23,
    OP_TESSELLATE = 24,
    OP_MAKE_NURB = 25,
    OP_COPY = 26,
    OP_CANONICAL = 30,
    OP_CIRCLE = 31,
    OP_ARC = 32,
    OP_DEGENERATE = 33,
    OP_LINE = 34,
    OP_FAIR = 35,
};

// Curve constructions of the curve block.
enum : int32_t
{
    CT_NONE = 0,          // static op, no curve
    CT_EMPTY = 1,         // SmBSplineCurve(lDim)
    CT_FAST_OWNED = 2,    // SmBSplineCurve(gw_CURVE*, lDim, FALSE)
    CT_FAST_BORROWED = 3, // SmBSplineCurve(gw_CURVE*, lDim, TRUE)
    CT_DATA = 4,          // SmBSplineCurve(lDim, n, m, p, Pw, U)
    CT_GW_COPY = 5,       // SmBSplineCurve(lDim, const gw_CURVE*)
    CT_COPY = 6,          // copy constructor of a CT_FAST_OWNED curve
};

// Reads the protected STEP flags through pointers to members named in a derived class.
struct Peek : SmBSplineCurve
{
    static SmKnotType SmBSplineCurve::*kt() { return &Peek::m_eKnotType; }
    static SmBSplineCurveForm SmBSplineCurve::*form() { return &Peek::m_eBSplineCurveForm; }
};

struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
};

// Reads `n p m cpts knots` into a curve from sm_AllocateNurbCurve.
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

// Builds `this` from a curve block: ctor dim oob [n p m cpts knots]. Borrowed NURBs are
// never freed (the curve may have swapped or freed them).
SmBSplineCurve* read_curve(Rd& rd, const SmContext& ctx, int32_t ctor)
{
    ULONG dim = (ULONG)rd.idx();
    bool oob = rd.idx() != 0;
    SmBSplineCurve* c = NULL;
    switch (ctor)
    {
    case CT_EMPTY: c = new (ctx) SmBSplineCurve(dim); break;
    case CT_FAST_OWNED: c = new (ctx) SmBSplineCurve(read_nurb(rd), dim, FALSE); break;
    case CT_FAST_BORROWED: c = new (ctx) SmBSplineCurve(read_nurb(rd), dim, TRUE); break;
    case CT_DATA:
    {
        gw_CURVE* src = read_nurb(rd);
        c = new (ctx) SmBSplineCurve(dim, (ULONG)src->pol->n, (ULONG)src->knt->m, (short)src->p,
                                     src->pol->Pw, src->knt->U);
        smos_Free(src);
        break;
    }
    case CT_GW_COPY:
    {
        gw_CURVE* src = read_nurb(rd);
        c = new (ctx) SmBSplineCurve(dim, src);
        smos_Free(src);
        break;
    }
    case CT_COPY:
    {
        SmBSplineCurve* orig = new (ctx) SmBSplineCurve(read_nurb(rd), dim, FALSE);
        orig->SetOutOfBoundsEnabled(oob);
        c = new (ctx) SmBSplineCurve(*orig);
        delete orig;
        break;
    }
    default: return NULL;
    }
    c->SetOutOfBoundsEnabled(oob);
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

// SetCanonical(O, X, Y) of a frame read as 9 numbers.
SmStatus read_frame(Rd& rd, SmAxis2Placement& f)
{
    SmVector3d o = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    return f.SetCanonical(o, x, y);
}

// Status, then (on success) the new curve's state; deletes it.
void put_new(std::vector<double>& o, SmStatus s, SmCurve* c)
{
    o.push_back(st(s));
    if (s == SM_SUCCESS && c)
        put_state(o, (SmBSplineCurve*)c);
    delete c;
}

void static_op(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_CANONICAL:
    {
        ULONG dim = (ULONG)rd.idx();
        ULONG deg = (ULONG)rd.idx();
        int64_t ncp = rd.idx();
        SmTArray<SmPoint3d> cps;
        for (int64_t i = 0; i < ncp; i++)
            cps.Add(rd.vec());
        SmBSplineCurveForm form = (SmBSplineCurveForm)rd.idx();
        int64_t nk = rd.idx();
        SmTArray<double> knots;
        SmTArray<ULONG> mults;
        for (int64_t i = 0; i < nk; i++)
            knots.Add(rd.num());
        for (int64_t i = 0; i < nk; i++)
            mults.Add((ULONG)rd.idx());
        SmKnotType kt = (SmKnotType)rd.idx();
        bool has_w = rd.idx() != 0;
        SmTArray<double> w;
        if (has_w)
        {
            int64_t nw = rd.idx();
            for (int64_t i = 0; i < nw; i++)
                w.Add(rd.num());
        }
        bool has_trim = rd.idx() != 0;
        double a = rd.num();
        double b = rd.num();
        SmExtent1d trim(a, b);
        SmBSplineCurve* c = NULL;
        SmStatus s = SmBSplineCurve::CreateCanonical(ctx, dim, deg, cps, form, mults, knots, kt,
                                                     has_w ? &w : NULL, has_trim ? &trim : NULL, c);
        put_new(o, s, c);
        break;
    }
    case OP_CIRCLE:
    {
        ULONG dim = (ULONG)rd.idx();
        SmAxis2Placement f;
        SmStatus fs = read_frame(rd, f);
        double r = rd.num();
        double a0 = rd.num();
        double a1 = rd.num();
        SmNurbCircleParam prm = rd.idx() == 0 ? SM_CO_QUADRATIC : SM_CO_QUINTIC;
        if (fs != SM_SUCCESS)
        {
            o.push_back(st(fs));
            break;
        }
        SmBSplineCurve* c = NULL;
        SmStatus s = SmBSplineCurve::CreateCircleSegment(ctx, dim, f, r, a0, a1, prm, c);
        put_new(o, s, c);
        break;
    }
    case OP_ARC:
    {
        SmPoint3d cen = rd.vec();
        SmPoint3d p1 = rd.vec();
        SmPoint3d p2 = rd.vec();
        SmNurbCircleParam prm = rd.idx() == 0 ? SM_CO_QUADRATIC : SM_CO_QUINTIC;
        SmBSplineCurve* c = NULL;
        SmStatus s = SmBSplineCurve::CreateArcFromPoints(ctx, 3, cen, p1, p2, prm, c);
        put_new(o, s, c);
        break;
    }
    case OP_DEGENERATE:
    {
        ULONG dim = (ULONG)rd.idx();
        SmPoint3d p = rd.vec();
        SmBSplineCurve* c = NULL;
        SmStatus s = SmBSplineCurve::CreateDegenerateCurve(ctx, dim, p, c);
        put_new(o, s, c);
        break;
    }
    case OP_LINE:
    {
        ULONG dim = (ULONG)rd.idx();
        SmPoint3d p0 = rd.vec();
        SmPoint3d p1 = rd.vec();
        SmBSplineCurve* c = NULL;
        SmStatus s = SmBSplineCurve::CreateLineSegment(ctx, dim, p0, p1, c);
        put_new(o, s, c);
        break;
    }
    case OP_FAIR:
    {
        int64_t np = rd.idx();
        SmTArray<SmPoint3d> pts;
        for (int64_t i = 0; i < np; i++)
            pts.Add(rd.vec());
        int64_t nt = rd.idx();
        SmTArray<SmVector3d> tans;
        for (int64_t i = 0; i < nt; i++)
            tans.Add(rd.vec());
        bool has_h = rd.idx() != 0;
        int64_t nh = rd.idx();
        SmTArray<SmVector3d> hi;
        for (int64_t i = 0; i < nh; i++)
            hi.Add(rd.vec());
        bool prevent = rd.idx() != 0;
        SmStatus s = SmBSplineCurve::FairBlendCurveDerivatives(pts, tans, has_h ? &hi : NULL,
                                                               prevent ? TRUE : FALSE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            for (ULONG i = 0; i < tans.GetSize(); i++)
                put_vec(o, tans[i]);
            for (ULONG i = 0; i < hi.GetSize(); i++)
                put_vec(o, hi[i]);
        }
        break;
    }
    default: break;
    }
}

void curve_op(int32_t op, Rd& rd, const SmContext& ctx, SmBSplineCurve* c, std::vector<double>& o)
{
    switch (op)
    {
    case OP_STATE: o.push_back(0.0); break;
    case OP_EVALUATE:
    {
        double t = rd.num();
        ULONG nd = (ULONG)rd.idx();
        bool left = rd.idx() != 0;
        bool nonzero = rd.idx() != 0;
        std::vector<SmVector3d> pv(nd + 1);
        SmStatus s = c->Evaluate(t, nd, left ? TRUE : FALSE, pv.data(), nonzero ? TRUE : FALSE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            for (ULONG i = 0; i <= nd; i++)
                put_vec(o, pv[i]);
        break;
    }
    case OP_EVALUATE_POINT:
    {
        double t = rd.num();
        SmPoint3d p;
        SmStatus s = c->EvaluatePoint(t, p);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, p);
        break;
    }
    case OP_INTERVALS:
    {
        o.push_back(0.0);
        SmExtent1d a = c->GetNaturalInterval();
        SmExtent1d b = c->GetSTEPInterval();
        o.push_back(a.GetMin());
        o.push_back(a.GetMax());
        o.push_back(b.GetMin());
        o.push_back(b.GetMax());
        break;
    }
    case OP_KNOTS:
    {
        bool has = rd.idx() != 0;
        double a = rd.num();
        double b = rd.num();
        SmExtent1d ivl(a, b);
        SmTArray<double> k;
        SmTArray<ULONG> mu;
        SmStatus s = c->GetKnots(k, &mu, has ? &ivl : NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)k.GetSize());
            for (ULONG i = 0; i < k.GetSize(); i++)
                o.push_back(k[i]);
            for (ULONG i = 0; i < mu.GetSize(); i++)
                o.push_back((double)mu[i]);
        }
        break;
    }
    case OP_CONTROL_POLYGON:
    {
        SmTArray<SmPoint3d> pts;
        SmTArray<double> w;
        SmStatus s = c->GetControlPolygon(pts, w);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)pts.GetSize());
            for (ULONG i = 0; i < pts.GetSize(); i++)
                put_vec(o, pts[i]);
            o.push_back((double)w.GetSize());
            for (ULONG i = 0; i < w.GetSize(); i++)
                o.push_back(w[i]);
        }
        break;
    }
    case OP_CONTROL_POINTS:
    {
        ULONG count = 0;
        double* pts = NULL;
        SmStatus s = c->GetControlPointsPointer(count, pts);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)count);
            for (ULONG i = 0; i < 4 * count; i++)
                o.push_back(pts[i]);
        }
        break;
    }
    case OP_ENDS:
    {
        SmPoint3d a, b;
        double wa = 0.0, wb = 0.0;
        c->GetEnds(a, b, &wa, &wb);
        o.push_back(0.0);
        put_vec(o, a);
        put_vec(o, b);
        o.push_back(wa);
        o.push_back(wb);
        break;
    }
    case OP_COUNTS:
        o.push_back(0.0);
        o.push_back((double)c->GetDegree());
        o.push_back((double)c->GetNumberNaturalKnots());
        o.push_back((double)c->GetNumberControlPoints());
        o.push_back(c->IsRational() ? 1.0 : 0.0);
        o.push_back(c->GetInsideOut() ? 1.0 : 0.0);
        o.push_back(c->GetOutOfBoundsEnabled() ? 1.0 : 0.0);
        break;
    case OP_GREVILLE:
    {
        ULONG i = (ULONG)rd.idx();
        o.push_back(0.0);
        o.push_back(c->GetGrevilleAbscissa(i));
        break;
    }
    case OP_CONVERT_2D: o.push_back(st(c->ConvertTo2D())); break;
    case OP_CONVERT_3D: o.push_back(st(c->ConvertTo3D())); break;
    case OP_EDIT_PARAM:
    {
        double a = rd.num();
        double b = rd.num();
        bool notify = rd.idx() != 0;
        o.push_back(st(c->EditParameterization(SmExtent1d(a, b), notify ? TRUE : FALSE)));
        break;
    }
    case OP_EDIT_END:
    {
        SmPoint3d p = rd.vec();
        bool start = rd.idx() != 0;
        bool has_w = rd.idx() != 0;
        double w = rd.num();
        o.push_back(st(c->EditEndPoint(p, start ? TRUE : FALSE, has_w ? &w : NULL)));
        break;
    }
    case OP_FIXUP: o.push_back(st(c->FixupKnotVector(rd.num()))); break;
    case OP_REMOVE_EXTRA: o.push_back(st(c->RemoveExtraKnots(rd.num()))); break;
    case OP_SET_FROM_GW:
    {
        bool has = rd.idx() != 0;
        gw_CURVE* src = has ? read_nurb(rd) : NULL;
        o.push_back(st(c->SetFromGwNurb(3, src)));
        if (src)
            smos_Free(src);
        break;
    }
    case OP_TRANSFORM:
    {
        SmAxis2Placement f;
        SmStatus fs = read_frame(rd, f);
        bool has_s = rd.idx() != 0;
        SmVector3d sc = rd.vec();
        if (fs != SM_SUCCESS)
        {
            o.push_back(st(fs));
            break;
        }
        o.push_back(st(c->Transform(f, has_s ? &sc : NULL)));
        break;
    }
    case OP_MIRROR:
    {
        SmAxis2Placement f;
        SmStatus fs = read_frame(rd, f);
        if (fs != SM_SUCCESS)
        {
            o.push_back(st(fs));
            break;
        }
        SmCurve* m = NULL;
        SmStatus s = c->CreateMirrorCurve(ctx, f, m);
        put_new(o, s, m);
        break;
    }
    case OP_PROJECTION:
    {
        SmProjectionType pt = (SmProjectionType)rd.idx();
        SmPoint3d p = rd.vec();
        SmVector3d nrm = rd.vec();
        SmVector3d a = rd.vec();
        SmCurve* r = NULL;
        SmStatus s = c->CreatePlaneProjection(ctx, pt, p, nrm, a, r);
        put_new(o, s, r);
        break;
    }
    case OP_EQ:
    {
        int32_t ctor = (int32_t)rd.idx();
        SmBSplineCurve* other = read_curve(rd, ctx, ctor);
        o.push_back(0.0);
        o.push_back((*c == *other) ? 1.0 : 0.0);
        o.push_back((*c == *c) ? 1.0 : 0.0);
        delete other;
        break;
    }
    case OP_IS_KIND_OF:
    {
        SM_TYPE t = (SM_TYPE)rd.idx();
        o.push_back(0.0);
        o.push_back(c->IsKindOf(t) ? 1.0 : 0.0);
        break;
    }
    case OP_TYPE:
        o.push_back(0.0);
        o.push_back((double)c->GetType());
        o.push_back((double)SmBSplineCurve::GetClassType());
        break;
    case OP_CONTINUITIES:
    {
        double tol = rd.num();
        SmContinuityType mn = SM_CT_UNDEFINED;
        SmTArray<SmContinuityType> all;
        SmStatus s = c->CalculateContinuities(mn, all, tol);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)mn);
            o.push_back((double)all.GetSize());
            for (ULONG i = 0; i < all.GetSize(); i++)
                o.push_back((double)all[i]);
        }
        break;
    }
    case OP_TESSELLATE:
    {
        double a = rd.num();
        double b = rd.num();
        double ch = rd.num();
        double ang = rd.num();
        ULONG mins = (ULONG)rd.idx();
        SmTArray<double> prm;
        SmTArray<SmPoint3d> pts;
        SmTArray<SmVector3d> tans;
        SmStatus s = c->Tessellate(SmExtent1d(a, b), ch, ang, mins, &prm, &pts, &tans);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)prm.GetSize());
            for (ULONG i = 0; i < prm.GetSize(); i++)
                o.push_back(prm[i]);
            o.push_back((double)pts.GetSize());
            for (ULONG i = 0; i < pts.GetSize(); i++)
                put_vec(o, pts[i]);
            o.push_back((double)tans.GetSize());
            for (ULONG i = 0; i < tans.GetSize(); i++)
                put_vec(o, tans[i]);
        }
        break;
    }
    case OP_MAKE_NURB: o.push_back(st(c->MakeNurb())); break;
    case OP_COPY:
    {
        SmCurve* cp = NULL;
        SmStatus s = c->Copy(ctx, cp);
        put_new(o, s, cp);
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_nurbs_b_spline_curve_p1_eval(int32_t op, const double* in, int64_t n_in,
                                                  double* out, int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    int32_t ctor = (int32_t)rd.idx();
    if (ctor == CT_NONE)
    {
        static_op(op, rd, ctx, o);
    }
    else
    {
        SmBSplineCurve* c = read_curve(rd, ctx, ctor);
        if (c == NULL)
            return SC_ERR_ARG;
        curve_op(op, rd, ctx, c, o);
        put_state(o, c);
        delete c;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
