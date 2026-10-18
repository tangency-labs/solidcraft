// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nurbs.b_spline_curve_p2`.
// Wraps the SmBSplineCurve members owned by the unit (source/SMLib/src/SmBSplineCurve.cpp,
// source/SMLib/inc/SmBSplineCurve.h): IsIntersectorApprox, CalculateBoundingBox,
// CreateAnalyticCurve, CreateByJoining, CreatePointCurve, CreateSimpleOffset,
// HasRepeatedEndControlPoints, GetCanonical, IsLine, IsPiecewiseC0, IsDegenerate,
// GetControlPointsExpert, GetMaxAnalyticDomain, GetNumberOfUniqueKnots, GetPolygon, MatchWeights,
// DegreeElevate, ReverseParameterization, SetCanonical, SetControlPoint, AdjustSTEPInterval and
// ReadFromDB.
//
// One entry point, sc_nurbs_b_spline_curve_p2_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/nurbs_b_spline_curve_p2.rs, which validates every
// input. Curve ops start with a curve block `has_nurb dim oob [n p m cpts knots]` (`this`: with a
// NURB the fast constructor owning it, else SmBSplineCurve(lDim)); static ops have none. `out`
// receives the op status (0 = SM_SUCCESS, else the SmStatus value), the op's outputs, then for
// curve ops the state of `this`. A curve state is: dim oob borrowed knot_type form has_nurb
// [n p m cpts(x y z w) knots].
#include "sc_shim.h"

#include <SmBSplineCurve.h>
#include <SmContext.h>
#include <SmCurveTypes.h>
#include <SmDatabaseIO.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmNurbsCrv.h>
#include <SmPolarBox.h>
#include <SmPseudoBox.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <sstream>
#include <string>
#include <vector>

#include "sc_sm.h"
#include "sc_gw.h"

using sc::Rd;
using sc::flg;
using sc::put_vec;
using sc::read_nurb;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/nurbs_b_spline_curve_p2.rs).
enum : int32_t
{
    OP_INTERSECTOR = 0,
    OP_BBOX = 1,
    OP_ANALYTIC = 2,
    OP_JOIN = 3,
    OP_POINT = 4,
    OP_OFFSET = 5,
    OP_REPEATED = 6,
    OP_CANONICAL = 7,
    OP_IS_LINE = 8,
    OP_C0 = 9,
    OP_DEGENERATE = 10,
    OP_EXPERT = 11,
    OP_MAX_DOMAIN = 12,
    OP_UNIQUE = 13,
    OP_POLYGON = 14,
    OP_MATCH = 15,
    OP_ELEVATE = 16,
    OP_REVERSE = 17,
    OP_SET_CANONICAL = 18,
    OP_SET_CP = 19,
    OP_ADJUST = 20,
    OP_READ_DB = 21,
};

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

// Status, then (on success) whether there is a new curve and its state; deletes it.
void put_new(std::vector<double>& o, SmStatus s, SmCurve* c)
{
    o.push_back(st(s));
    if (s == SM_SUCCESS)
    {
        o.push_back(c ? 1.0 : 0.0);
        if (c)
            put_state(o, (SmBSplineCurve*)c);
    }
    delete c;
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

// `dim deg ncp P[3 ncp] form nk K[nk] M[nk] kt has_w [nw W[nw]] has_trim a b` into the
// arguments of CreateCanonical / SetCanonical.
struct Canon
{
    ULONG dim;
    ULONG deg;
    SmTArray<SmPoint3d> cps;
    SmBSplineCurveForm form;
    SmTArray<double> knots;
    SmTArray<ULONG> mults;
    SmKnotType kt;
    bool has_w;
    SmTArray<double> w;
    bool has_trim;
    SmExtent1d trim;
};

void read_canon(Rd& rd, Canon& c)
{
    c.dim = (ULONG)rd.idx();
    c.deg = (ULONG)rd.idx();
    int64_t ncp = rd.idx();
    for (int64_t i = 0; i < ncp; i++)
        c.cps.Add(rd.vec());
    c.form = (SmBSplineCurveForm)rd.idx();
    int64_t nk = rd.idx();
    for (int64_t i = 0; i < nk; i++)
        c.knots.Add(rd.num());
    for (int64_t i = 0; i < nk; i++)
        c.mults.Add((ULONG)rd.idx());
    c.kt = (SmKnotType)rd.idx();
    c.has_w = rd.flag();
    if (c.has_w)
    {
        int64_t nw = rd.idx();
        for (int64_t i = 0; i < nw; i++)
            c.w.Add(rd.num());
    }
    c.has_trim = rd.flag();
    double a = rd.num();
    double b = rd.num();
    c.trim = SmExtent1d(a, b);
}

void static_op(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_JOIN:
    {
        int64_t k = rd.idx();
        SmTArray<SmBSplineCurve*> segs;
        for (int64_t i = 0; i < k; i++)
            segs.Add(read_curve(rd, ctx));
        bool has_senses = rd.flag();
        SmTArray<SmBoolean> senses;
        for (int64_t i = 0; i < k; i++)
            senses.Add(rd.flag() ? TRUE : FALSE);
        SmBSplineCurve* c = NULL;
        SmStatus s = SmBSplineCurve::CreateByJoining(ctx, segs, has_senses ? &senses : NULL, c);
        put_new(o, s, c);
        // the segments afterwards (elevated / made 2D in place)
        for (ULONG i = 0; i < segs.GetSize(); i++)
        {
            put_state(o, segs[i]);
            delete segs[i];
        }
        break;
    }
    case OP_POINT:
    {
        SmPoint3d p = rd.vec();
        SmBSplineCurve* c = NULL;
        SmStatus s = SmBSplineCurve::CreatePointCurve(ctx, p, c);
        put_new(o, s, c);
        break;
    }
    case OP_READ_DB:
    {
        ULONG dim = (ULONG)rd.idx();
        int32_t file_type = (int32_t)rd.idx();
        ULONG version = (ULONG)rd.idx();
        int64_t nb = rd.idx();
        std::string bytes;
        for (int64_t i = 0; i < nb; i++)
            bytes.push_back((char)(unsigned char)rd.idx());
        std::istringstream iss(bytes);
        SmDatabaseIOStream db(file_type == 0 ? SM_ASCII : SM_BINARY, &iss);
        SmCurve* c = NULL;
        SmStatus s = SmBSplineCurve::ReadFromDB(SmBSplineCurve_TYPE, db, dim, ctx, c, version);
        put_new(o, s, c);
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}

void curve_op(int32_t op, Rd& rd, const SmContext& ctx, SmBSplineCurve* c, std::vector<double>& o)
{
    switch (op)
    {
    case OP_INTERSECTOR:
        o.push_back(0.0);
        o.push_back(flg(c->IsIntersectorApprox()));
        break;
    case OP_BBOX:
    {
        double a = rd.num();
        double b = rd.num();
        bool want_n = rd.flag();
        bool want_ps = rd.flag();
        bool want_pl = rd.flag();
        SmExtent1d ivl(a, b);
        SmExtent3d nb;
        SmPseudoBox pb;
        SmPolarBox pl;
        SmStatus s = c->CalculateBoundingBox(ivl, want_n ? &nb : NULL, want_ps ? &pb : NULL,
                                             want_pl ? &pl : NULL, TRUE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            if (want_n)
            {
                for (ULONG i = 0; i < 3; i++)
                    o.push_back(nb.GetMin(i));
                for (ULONG i = 0; i < 3; i++)
                    o.push_back(nb.GetMax(i));
            }
            if (want_ps)
                put_pseudo(o, pb);
            if (want_pl)
                put_polar(o, pl);
        }
        break;
    }
    case OP_ANALYTIC:
    {
        double a = rd.num();
        double b = rd.num();
        SmExtent1d ivl(a, b);
        SmCurve* r = NULL;
        SmStatus s = c->CreateAnalyticCurve(ctx, ivl, r);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back(r ? 1.0 : 0.0);
        delete r;
        break;
    }
    case OP_OFFSET:
    {
        double tol = rd.num();
        SmVector3d nrm = rd.vec();
        double d = rd.num();
        bool match = rd.flag();
        SmBSplineCurve* r = NULL;
        double gap = 0.0;
        SmStatus s = c->CreateSimpleOffset(ctx, tol, nrm, d, r, gap, match ? TRUE : FALSE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(gap);
            o.push_back(r ? 1.0 : 0.0);
            if (r)
                put_state(o, r);
        }
        delete r;
        break;
    }
    case OP_REPEATED:
        o.push_back(0.0);
        o.push_back(flg(c->HasRepeatedEndControlPoints(rd.num())));
        break;
    case OP_CANONICAL:
    {
        ULONG dim = 0, deg = 0;
        SmTArray<SmPoint3d> pts;
        SmBSplineCurveForm form = SM_CF_UNSPECIFIED;
        SmTArray<ULONG> mults;
        SmTArray<double> knots;
        SmKnotType kt = SM_KT_UNSPECIFIED;
        SmTArray<double> w;
        SmStatus s = c->GetCanonical(dim, deg, pts, form, mults, knots, kt, w);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)dim);
            o.push_back((double)deg);
            o.push_back((double)form);
            o.push_back((double)kt);
            o.push_back((double)pts.GetSize());
            for (ULONG i = 0; i < pts.GetSize(); i++)
                put_vec(o, pts[i]);
            o.push_back((double)knots.GetSize());
            for (ULONG i = 0; i < knots.GetSize(); i++)
                o.push_back(knots[i]);
            for (ULONG i = 0; i < mults.GetSize(); i++)
                o.push_back((double)mults[i]);
            o.push_back((double)w.GetSize());
            for (ULONG i = 0; i < w.GetSize(); i++)
                o.push_back(w[i]);
        }
        break;
    }
    case OP_IS_LINE:
    {
        ULONG ns = (ULONG)rd.idx();
        double tol = rd.num();
        SmPoint3d p(0.0, 0.0, 0.0);
        SmVector3d v(0.0, 0.0, 0.0);
        SmBoolean yes = c->IsLine(ns, tol, p, v);
        o.push_back(0.0);
        o.push_back(flg(yes));
        if (yes)
        {
            put_vec(o, p);
            put_vec(o, v);
        }
        break;
    }
    case OP_C0:
        o.push_back(0.0);
        o.push_back(flg(c->IsPiecewiseC0()));
        break;
    case OP_DEGENERATE:
    {
        double tol = rd.num();
        bool has_ivl = rd.flag();
        double a = rd.num();
        double b = rd.num();
        SmExtent1d ivl(a, b);
        o.push_back(0.0);
        o.push_back(flg(c->IsDegenerate(tol, has_ivl ? &ivl : NULL)));
        break;
    }
    case OP_EXPERT:
    {
        SmControlPointFormType form = (SmControlPointFormType)rd.idx();
        ULONG start = (ULONG)rd.idx();
        ULONG end = (ULONG)rd.idx();
        ULONG stride = (ULONG)rd.idx();
        int64_t count = end >= start ? (int64_t)(end - start + 1) : 0;
        std::vector<double> buf((size_t)(count * (int64_t)stride) + 1, 0.0);
        SmStatus s = c->GetControlPointsExpert(form, start, end, stride, buf.data());
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)(count * (int64_t)stride));
            for (int64_t i = 0; i < count * (int64_t)stride; i++)
                o.push_back(buf[(size_t)i]);
        }
        break;
    }
    case OP_MAX_DOMAIN:
    {
        SmExtent1d d = c->GetMaxAnalyticDomain();
        o.push_back(0.0);
        o.push_back(d.GetMin());
        o.push_back(d.GetMax());
        break;
    }
    case OP_UNIQUE:
        o.push_back(0.0);
        o.push_back((double)c->GetNumberOfUniqueKnots());
        break;
    case OP_POLYGON:
    {
        double a = rd.num();
        double b = rd.num();
        SmExtent1d ivl(a, b);
        SmTArray<SmPoint3d> pts;
        SmStatus s = c->GetPolygon(ivl, pts);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)pts.GetSize());
            for (ULONG i = 0; i < pts.GetSize(); i++)
                put_vec(o, pts[i]);
        }
        break;
    }
    case OP_MATCH:
    {
        SmBSplineCurve* other = read_curve(rd, ctx);
        o.push_back(st(c->MatchWeights(other)));
        delete other;
        break;
    }
    case OP_ELEVATE: o.push_back(st(c->DegreeElevate((ULONG)rd.idx()))); break;
    case OP_REVERSE:
    {
        double a = rd.num();
        double b = rd.num();
        SmExtent1d old(a, b);
        SmExtent1d nw(old);
        SmStatus s = c->ReverseParameterization(old, nw);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(nw.GetMin());
            o.push_back(nw.GetMax());
        }
        break;
    }
    case OP_SET_CANONICAL:
    {
        Canon k;
        read_canon(rd, k);
        o.push_back(st(c->SetCanonical(k.dim, k.deg, k.cps, k.form, k.mults, k.knots, k.kt,
                                       k.has_w ? &k.w : NULL, k.has_trim ? &k.trim : NULL)));
        break;
    }
    case OP_SET_CP:
    {
        SmControlPointFormType form = (SmControlPointFormType)rd.idx();
        ULONG index = (ULONG)rd.idx();
        SmPoint3d p = rd.vec();
        double w = rd.num();
        o.push_back(st(c->SetControlPoint(form, index, p, w)));
        break;
    }
    case OP_ADJUST:
    {
        double a = rd.num();
        double b = rd.num();
        SmExtent1d ivl(a, b);
        o.push_back(st(c->AdjustSTEPInterval(ivl)));
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}

bool is_static(int32_t op)
{
    return op == OP_JOIN || op == OP_POINT || op == OP_READ_DB;
}
} // namespace

SC_EXPORT int32_t sc_nurbs_b_spline_curve_p2_eval(int32_t op, const double* in, int64_t n_in,
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
