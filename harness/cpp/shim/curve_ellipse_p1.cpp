// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.ellipse_p1`: the SmEllipse / SmCircle / SmPolarConversion
// members owned by the unit (source/SMLib/src/SmEllipse.cpp, source/SMLib/src/SmCircle.cpp,
// source/SMLib/inc/SmCircle.h), called on objects built by the upstream constructors,
// CreateCanonical or ReadFromDB (virtual dispatch is upstream's).
//
// One entry point, sc_curve_ellipse_p1_run(in, n_in, out, cap, out_len). `in` is a program:
// a construction of curve A, the op count, then the ops; layouts are documented in
// crates/solidcore-ffi/src/units/curve_ellipse_p1.rs. Every read is bounds checked here (a
// bad program is SC_ERR_ARG). `out` receives the construction status; when A was built and
// has a NURB, per op `op status [values]` (values only when the status is 0); then (A built)
// the state of A. Private members are read through explicit-instantiation access.
#include "sc_shim.h"

#include <SmAssertArray.h>
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmCurveTypes.h>
#include <SmDatabaseIO.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmGeomUtility.h>
#include <SmSolutionArray.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

#include "sc_sm.h"

using sc::put_vec;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/curve_ellipse_p1.rs).
enum : int32_t
{
    OP_STATE = 1,
    OP_COPY = 2,
    OP_TO_STEP = 3,
    OP_TO_POLAR = 4,
    OP_ADJUST = 5,
    OP_GPS_STEP = 6,
    OP_GPS = 7,
    OP_JOIN = 8,
    OP_REVERSE = 9,
    OP_TRIM = 10,
    OP_ASSERT = 11,
    OP_IS_CURRENT = 12,
    OP_POLAR_ASSERT = 13,
    OP_OFFSET = 14,
    OP_SET_RADIUS = 15,
    OP_MAKE_NURB = 16,
    OP_TO_2D = 17,
    OP_EDIT_PARAM = 18,
    OP_NOTIFY = 19,
    OP_IS_ARC = 20,
    OP_LAST = 20,
};

// Constructions.
enum : int32_t
{
    K_ELLIPSE = 0,
    K_CIRCLE = 1,
    K_CIRCLE_3PT = 2,
    K_ELLIPSE_CANONICAL = 3,
    K_CIRCLE_CANONICAL = 4,
    K_READ_DB = 5,
};

// Access to private / protected members through explicit instantiation.
template <typename Tag, typename Tag::type M>
struct Rob
{
    friend typename Tag::type get(Tag) { return M; }
};

struct PcKnots
{
    typedef SmTArray<double> SmPolarConversion::*type;
    friend type get(PcKnots);
};
template struct Rob<PcKnots, &SmPolarConversion::m_vNaturalKnots>;

struct PcAngles
{
    typedef SmTArray<double> SmPolarConversion::*type;
    friend type get(PcAngles);
};
template struct Rob<PcAngles, &SmPolarConversion::m_vAngles>;

struct PcExact
{
    typedef SmTArray<SmBoolean> SmPolarConversion::*type;
    friend type get(PcExact);
};
template struct Rob<PcExact, &SmPolarConversion::m_vExactConversion>;

struct PcCurve
{
    typedef const SmEllipse* SmPolarConversion::*type;
    friend type get(PcCurve);
};
template struct Rob<PcCurve, &SmPolarConversion::m_cpCurve>;

struct EllPolar
{
    typedef SmPolarConversion SmEllipse::*type;
    friend type get(EllPolar);
};
template struct Rob<EllPolar, &SmEllipse::m_vPolarConverter>;

struct EllInsideOut
{
    typedef SmBoolean SmEllipse::*type;
    friend type get(EllInsideOut);
};
template struct Rob<EllInsideOut, &SmEllipse::m_bInsideOut>;

struct BsForm
{
    typedef SmBSplineCurveForm SmBSplineCurve::*type;
    friend type get(BsForm);
};
template struct Rob<BsForm, &SmBSplineCurve::m_eBSplineCurveForm>;

struct Bad : std::runtime_error
{
    Bad() : std::runtime_error("bad program") {}
};

struct Rd
{
    const double* in;
    int64_t n;
    int64_t at;
    double num()
    {
        if (at >= n)
            throw Bad();
        return in[at++];
    }
    int64_t idx(int64_t lo, int64_t hi)
    {
        double d = num();
        if (!(d >= (double)lo && d <= (double)hi) || d != (double)(int64_t)d)
            throw Bad();
        return (int64_t)d;
    }
    bool flag() { return idx(0, 1) != 0; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
};

double flg(SmBoolean b)
{
    return b ? 1.0 : 0.0;
}

void put_ivl(std::vector<double>& o, const SmExtent1d& e)
{
    o.push_back(e.GetMin());
    o.push_back(e.GetMax());
}

void put_chars(std::vector<double>& o, const TCHAR* s)
{
    for (; *s; ++s)
        o.push_back((double)*s);
    o.push_back(-1.0);
}

SmPolarConversion& polar(SmEllipse* e)
{
    return e->*get(EllPolar());
}

void put_nurb(std::vector<double>& o, const SmBSplineCurve* c)
{
    const gw_CURVE* cur = c->GetGwNurbPointer();
    o.push_back(cur ? 1.0 : 0.0);
    if (cur)
    {
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
}

void put_state(std::vector<double>& o, SmEllipse* e)
{
    o.push_back((double)e->GetType());
    const SmAxis2Placement& pos = e->GetPosition();
    put_vec(o, pos.GetOriginRef());
    put_vec(o, pos.GetXAxisRef());
    put_vec(o, pos.GetYAxisRef());
    put_ivl(o, e->GetSTEPInterval());
    o.push_back(e->GetXRadius());
    o.push_back(e->GetYRadius());
    o.push_back(flg(e->*get(EllInsideOut())));
    o.push_back((double)e->GetDim());
    o.push_back((double)(e->*get(BsForm())));
    put_nurb(o, e);
    const SmPolarConversion& pc = polar(e);
    o.push_back(flg(pc.IsPolarConversionPossible()));
    o.push_back((pc.*get(PcCurve())) != NULL ? 1.0 : 0.0);
    const SmTArray<double>& k = pc.*get(PcKnots());
    o.push_back((double)k.GetSize());
    for (ULONG i = 0; i < k.GetSize(); i++)
        o.push_back(k[i]);
    const SmTArray<double>& a = pc.*get(PcAngles());
    o.push_back((double)a.GetSize());
    for (ULONG i = 0; i < a.GetSize(); i++)
        o.push_back(a[i]);
    const SmTArray<SmBoolean>& x = pc.*get(PcExact());
    o.push_back((double)x.GetSize());
    for (ULONG i = 0; i < x.GetSize(); i++)
        o.push_back(flg(x[i]));
}

void put_sols(std::vector<double>& o, SmSolutionArray& sols)
{
    o.push_back((double)sols.GetSize());
    for (ULONG i = 0; i < sols.GetSize(); i++)
    {
        const SmSolution& s = sols[i];
        o.push_back((double)s.m_eSolutionType);
        o.push_back((double)s.m_lNumVariables);
        o.push_back(s.m_vStart.m_dSolutionValue);
        o.push_back(s.m_vStart[0]);
    }
}

void put_reports(std::vector<double>& o, SmAssertArray& list, const void* a, const void* pc)
{
    o.push_back((double)list.GetSize());
    for (ULONG i = 0; i < list.GetSize(); i++)
    {
        const SmAssertReport* r = list[i];
        auto id = [&](const void* p) { return p == NULL ? -1.0 : p == a ? 0.0 : p == pc ? 1.0 : 2.0; };
        o.push_back((double)r->m_lReportingType);
        o.push_back((double)r->m_lListIndex);
        o.push_back((double)r->m_lTestIndex);
        o.push_back((double)r->m_eAssertType);
        o.push_back(id(r->m_pOwner));
        o.push_back(id(r->m_pReporter));
        o.push_back((double)r->m_lOwnerType);
        o.push_back((double)r->m_sTol3d);
        o.push_back(r->m_dValue);
    }
}

// `c[3] x[3] y[3] d0 d1 rx ry dim inside_out`, then (with_nurb) a flag and a nested
// ellipse block whose NURB is passed as pOptNurb.
SmEllipse* read_ellipse(Rd& rd, const SmContext& ctx, bool with_nurb)
{
    SmPoint3d c = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    double d0 = rd.num();
    double d1 = rd.num();
    double rx = rd.num();
    double ry = rd.num();
    ULONG dim = (ULONG)rd.idx(2, 3);
    bool io = rd.flag();
    SmEllipse* src = NULL;
    if (with_nurb && rd.flag())
        src = read_ellipse(rd, ctx, false);
    SmEllipse* e = new (ctx) SmEllipse(c, x, y, SmExtent1d(d0, d1), rx, ry, dim, &ctx, src,
                                       io ? TRUE : FALSE);
    delete src;
    return e;
}

// `c[3] x[3] y[3] d0 d1 r dim inside_out`, a flag and a nested ellipse block (pOptNurb).
SmCircle* read_circle(Rd& rd, const SmContext& ctx)
{
    SmPoint3d c = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    double d0 = rd.num();
    double d1 = rd.num();
    double r = rd.num();
    ULONG dim = (ULONG)rd.idx(2, 3);
    bool io = rd.flag();
    SmEllipse* src = NULL;
    if (rd.flag())
        src = read_ellipse(rd, ctx, false);
    SmCircle* e = new (ctx) SmCircle(c, x, y, SmExtent1d(d0, d1), r, dim, &ctx, src,
                                     io ? TRUE : FALSE);
    delete src;
    return e;
}

// An interval of `c` from two normalised parameters (extrapolated outside [0, 1]).
SmExtent1d frac_ivl(Rd& rd, const SmBSplineCurve* c)
{
    double f0 = rd.num();
    double f1 = rd.num();
    SmExtent1d nat = c->GetNaturalInterval();
    return SmExtent1d(nat.Evaluate(f0), nat.Evaluate(f1));
}

// A NURB parameter: `raw value` or `frac f`.
double param(Rd& rd, const SmBSplineCurve* c)
{
    bool raw = rd.flag();
    double v = rd.num();
    return raw ? v : c->GetNaturalInterval().Evaluate(v);
}

void run_op(int32_t op, Rd& rd, const SmContext& ctx, SmEllipse* a, std::vector<double>& o)
{
    bool is_circle = a->IsKindOf(SmCircle_TYPE) ? true : false;
    switch (op)
    {
    case OP_STATE:
    {
        o.push_back(0.0);
        o.push_back((double)a->GetType());
        put_chars(o, a->GetTypeString());
        put_chars(o, a->GetClassString());
        o.push_back(is_circle ? ((SmCircle*)a)->GetRadius() : 0.0);
        int64_t nt = rd.idx(0, 64);
        for (int64_t i = 0; i < nt; i++)
        {
            double t = rd.num();
            o.push_back(flg(a->IsKindOf((SM_TYPE)t)));
        }
        break;
    }
    case OP_COPY:
    {
        SmCurve* c = NULL;
        SmStatus s = a->Copy(ctx, c);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(*a == *c));
            put_state(o, (SmEllipse*)c);
        }
        delete c;
        break;
    }
    case OP_TO_STEP:
    {
        double t = param(rd, a);
        double r = 0.0;
        SmStatus s = a->ConvertTFromNURBSToSTEP(t, r);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back(r);
        break;
    }
    case OP_TO_POLAR:
    {
        double t = param(rd, a);
        double r = 0.0;
        SmBoolean exact = FALSE;
        SmStatus s = polar(a).ConvertToPolarParameter(t, r, exact);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(r);
            o.push_back(flg(exact));
        }
        break;
    }
    case OP_ADJUST:
    {
        double d0 = rd.num();
        double d1 = rd.num();
        o.push_back(st(a->AdjustSTEPInterval(SmExtent1d(d0, d1))));
        break;
    }
    case OP_GPS_STEP:
    case OP_GPS:
    {
        SmExtent1d ivl = op == OP_GPS ? frac_ivl(rd, a) : a->GetSTEPInterval();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx(0, 6);
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        bool has_target = rd.flag();
        double target = rd.num();
        SmSolutionRequestedType req = (SmSolutionRequestedType)rd.idx(0, 1);
        SmSolutionArray sols;
        SmStatus s = op == OP_GPS
                         ? a->GlobalPointSolve(ivl, sop, p, tol, has_target ? &target : NULL, NULL,
                                               req, sols)
                         : a->GlobalPointSolveSTEP(ivl, sop, p, tol, has_target ? &target : NULL,
                                                   NULL, req, sols);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_sols(o, sols);
        break;
    }
    case OP_JOIN:
    {
        ULONG end_this = (ULONG)rd.idx(0, 1);
        int64_t kind = rd.idx(0, 2);
        SmBSplineCurve* other = NULL;
        if (kind == 0)
            other = read_ellipse(rd, ctx, false);
        else if (kind == 1)
            other = read_circle(rd, ctx);
        else
            other = new (ctx) SmBSplineCurve(3, a->GetGwNurbPointer());
        ULONG end_other = (ULONG)rd.idx(0, 1);
        bool has_gap = rd.flag();
        double gap = rd.num();
        if (other->GetGwNurbPointer() == NULL)
        {
            // the other curve has no NURB (NLib failed): not called
            o.push_back(-2.0);
            delete other;
            break;
        }
        o.push_back(st(a->JoinWith(end_this, other, end_other, has_gap ? &gap : NULL)));
        delete other;
        break;
    }
    case OP_REVERSE:
    {
        SmExtent1d old = frac_ivl(rd, a);
        SmExtent1d nw = old;
        SmStatus s = a->ReverseParameterization(old, nw);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_ivl(o, nw);
        break;
    }
    case OP_TRIM:
    {
        SmExtent1d ivl = frac_ivl(rd, a);
        int64_t mode = rd.idx(0, 2);
        SmStatus s = a->Trim(ivl, (SmBoolean)mode, FALSE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_ivl(o, ivl);
        break;
    }
    case OP_ASSERT:
    {
        bool given = rd.flag();
        SmAssertArray list;
        SmBoolean ok = a->AssertValid(&list, given ? SM_LEVEL_GIVEN : SM_LEVEL_0, SM_NO_WALK, NULL);
        o.push_back(0.0);
        o.push_back(flg(ok));
        put_reports(o, list, a, &polar(a));
        break;
    }
    case OP_IS_CURRENT:
    case OP_POLAR_ASSERT:
    {
        SmAssertArray list;
        SmBoolean ok = op == OP_IS_CURRENT ? polar(a).IsCurrent(&list) : polar(a).AssertValid(&list);
        o.push_back(0.0);
        o.push_back(flg(ok));
        put_reports(o, list, a, &polar(a));
        break;
    }
    case OP_OFFSET:
    {
        double tol = rd.num();
        SmVector3d n = rd.vec();
        double d = rd.num();
        bool match = rd.flag();
        SmBSplineCurve* nc = NULL;
        double gap = -1.0;
        SmStatus s = a->CreateSimpleOffset(ctx, tol, n, d, nc, gap, match ? TRUE : FALSE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(gap);
            o.push_back(nc == NULL ? -1.0 : nc->IsKindOf(SmEllipse_TYPE) ? 1.0 : 0.0);
            if (nc != NULL)
            {
                if (nc->IsKindOf(SmEllipse_TYPE))
                    put_state(o, (SmEllipse*)nc);
                else
                {
                    o.push_back((double)nc->GetType());
                    o.push_back((double)nc->GetDim());
                    put_nurb(o, nc);
                }
            }
        }
        delete nc;
        break;
    }
    case OP_SET_RADIUS:
    {
        double r = rd.num();
        if (!is_circle)
            throw Bad();
        ((SmCircle*)a)->SetRadius(r);
        o.push_back(st(a->MakeNurb()));
        break;
    }
    case OP_MAKE_NURB: o.push_back(st(a->MakeNurb())); break;
    case OP_TO_2D: o.push_back(st(a->ConvertTo2D())); break;
    case OP_EDIT_PARAM:
    {
        double d0 = rd.num();
        double d1 = rd.num();
        bool notify = rd.flag();
        o.push_back(st(a->EditParameterization(SmExtent1d(d0, d1), notify ? TRUE : FALSE)));
        break;
    }
    case OP_NOTIFY:
    {
        int64_t code = rd.idx(0, 1);
        SmNotifyOperation n = code == 0 ? SM_NO_PRE_EDIT : SM_NO_POST_EDIT;
        a->Notify(n, a, NULL, NULL);
        o.push_back(0.0);
        break;
    }
    case OP_IS_ARC:
    {
        if (!is_circle)
            throw Bad();
        SmAxis2Placement f;
        double r = 0.0, s0 = 0.0, s1 = 0.0;
        SmBoolean ok = a->IsArc(5, 1e-6, f, r, s0, s1);
        o.push_back(0.0);
        o.push_back(flg(ok));
        put_vec(o, f.GetOriginRef());
        put_vec(o, f.GetXAxisRef());
        put_vec(o, f.GetYAxisRef());
        o.push_back(r);
        o.push_back(s0);
        o.push_back(s1);
        break;
    }
    default: throw Bad();
    }
}

// Builds A; pushes the construction status. NULL when not built.
SmEllipse* build(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    int64_t kind = rd.idx(0, 5);
    switch (kind)
    {
    case K_ELLIPSE: o.push_back(0.0); return read_ellipse(rd, ctx, true);
    case K_CIRCLE: o.push_back(0.0); return read_circle(rd, ctx);
    case K_CIRCLE_3PT:
    {
        SmPoint3d p0 = rd.vec();
        SmPoint3d p1 = rd.vec();
        SmPoint3d p2 = rd.vec();
        ULONG dim = (ULONG)rd.idx(2, 3);
        bool closed = rd.flag();
        // the constructor ignores this status (then uses uninitialised values)
        SmPoint3d c;
        SmVector3d x, y;
        SmExtent1d dom;
        double r = 0.0;
        SmStatus s = smgu_CircleFrom3Points(p0, p1, p2, c, x, y, dom, r, dim, closed ? TRUE : FALSE);
        o.push_back(st(s));
        if (s != SM_SUCCESS)
            return NULL;
        return new (ctx) SmCircle(p0, p1, p2, dim, closed ? TRUE : FALSE, &ctx);
    }
    case K_ELLIPSE_CANONICAL:
    case K_CIRCLE_CANONICAL:
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double r1 = rd.num();
        double r2 = kind == K_ELLIPSE_CANONICAL ? rd.num() : 0.0;
        bool has_ivl = rd.flag();
        double d0 = rd.num();
        double d1 = rd.num();
        bool has_io = rd.flag();
        SmBoolean io = rd.flag() ? TRUE : FALSE;
        SmAxis2Placement place;
        place.SetCanonical(c, x, y);
        SmExtent1d ivl(d0, d1);
        SmStatus s;
        SmEllipse* e = NULL;
        if (kind == K_ELLIPSE_CANONICAL)
        {
            s = SmEllipse::CreateCanonical(ctx, place, r1, r2, e, has_ivl ? &ivl : NULL,
                                           has_io ? &io : NULL);
        }
        else
        {
            SmCircle* cc = NULL;
            s = SmCircle::CreateCanonical(ctx, place, r1, cc, has_ivl ? &ivl : NULL,
                                          has_io ? &io : NULL);
            e = cc;
        }
        o.push_back(st(s));
        if (s != SM_SUCCESS)
        {
            delete e;
            return NULL;
        }
        return e;
    }
    case K_READ_DB:
    {
        bool circle = rd.flag();
        ULONG dim = (ULONG)rd.idx(2, 3);
        bool binary = rd.flag();
        ULONG version = (ULONG)rd.idx(0, 1000);
        int64_t nb = rd.idx(0, 1 << 20);
        std::string bytes;
        for (int64_t i = 0; i < nb; i++)
            bytes.push_back((char)(unsigned char)rd.idx(0, 255));
        std::istringstream iss(bytes);
        SmDatabaseIOStream db(binary ? SM_BINARY : SM_ASCII, &iss);
        SmCurve* c = NULL;
        SmStatus s = SmCurve::ReadFromDB(circle ? SmCircle_TYPE : SmEllipse_TYPE, db, dim, ctx, c,
                                         version);
        o.push_back(st(s));
        if (s != SM_SUCCESS || c == NULL)
        {
            delete c;
            return NULL;
        }
        return (SmEllipse*)c;
    }
    default: throw Bad();
    }
}
} // namespace

SC_EXPORT int32_t sc_curve_ellipse_p1_run(const double* in, int64_t n_in, double* out, int64_t cap,
                                          int64_t* out_len)
{
    if (in == NULL || n_in < 2)
        return SC_ERR_ARG;
    try
    {
        SmContext ctx;
        std::vector<double> o;
        Rd rd{in, n_in, 0};
        SmEllipse* a = build(rd, ctx, o);
        if (a != NULL)
        {
            int64_t n_ops = rd.idx(0, 64);
            if (a->GetGwNurbPointer() != NULL)
            {
                for (int64_t i = 0; i < n_ops; i++)
                {
                    int32_t op = (int32_t)rd.idx(1, OP_LAST);
                    o.push_back((double)op);
                    run_op(op, rd, ctx, a, o);
                }
            }
            put_state(o, a);
            delete a;
        }
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    catch (const Bad&)
    {
        return SC_ERR_ARG;
    }
    catch (...)
    {
        return SC_ERR_EXCEPTION;
    }
}
