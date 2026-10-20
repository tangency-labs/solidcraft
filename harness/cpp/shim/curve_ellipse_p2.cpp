// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.ellipse_p2`: the SmEllipse / SmPolarConversion members owned
// by the unit (source/SMLib/src/SmEllipse.cpp, source/SMLib/inc/SmEllipse.h), called on
// SmEllipse objects built by the geometric constructor (virtual dispatch is upstream's).
//
// One entry point, sc_curve_ellipse_p2_run(in, n_in, out, cap, out_len). `in` is a program: an
// ellipse block (curve A), the op count, then the ops; layouts are documented in
// crates/solidcore-ffi/src/units/curve_ellipse_p2.rs. Every read is bounds checked here (a bad
// program is SC_ERR_ARG). `out` receives, when A has a NURB, per op `op status [values]`
// (values only when the status is 0), then the state of A; without NURB (NLib failed) only the
// state. Private and protected members are read through explicit-instantiation access.
#include "sc_shim.h"

#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmContext.h>
#include <SmCurveTypes.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmLine.h>
#include <SmSolutionArray.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <stdexcept>
#include <vector>

#include "sc_sm.h"

using sc::put_vec;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/curve_ellipse_p2.rs).
enum : int32_t
{
    OP_STATE = 1,
    OP_EVAL_STEP = 2,
    OP_EVAL_STEP_POINT = 3,
    OP_STEP_INV = 4,
    OP_TO_NURBS = 5,
    OP_POLAR_TO_NURBS = 6,
    OP_ANGLE_TO_NURB = 7,
    OP_IS_DEGENERATE = 8,
    OP_START_PLANE = 9,
    OP_DROP = 10,
    OP_INTERSECT_ELLIPSE = 11,
    OP_INTERSECT_LINE = 12,
    OP_TRANSFORM = 13,
    OP_TO_3D = 14,
    OP_SET_UP = 15,
    OP_EQ = 16,
    OP_SET_STEP_INTERVAL = 17,
    OP_LAST = 17,
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

struct PcAngleFn
{
    typedef SmStatus (SmPolarConversion::*type)(double, double, double, double, double, double&) const;
    friend type get(PcAngleFn);
};
template struct Rob<PcAngleFn, &SmPolarConversion::AngleParamToNurbParam>;

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

const SmPolarConversion& polar(const SmEllipse* e)
{
    return e->*get(EllPolar());
}

// `c[3] x[3] y[3] d0 d1 rx ry dim inside_out` then, when `with_nurb`, a flag and (if set) a
// second block whose ellipse's NURB is passed as pOptNurb.
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

// A bounded line `p[3] v[3] d0 d1 scale`.
SmLine* read_line(Rd& rd, const SmContext& ctx)
{
    SmPoint3d p = rd.vec();
    SmVector3d v = rd.vec();
    double d0 = rd.num();
    double d1 = rd.num();
    double scale = rd.num();
    return new (ctx) SmLine(p, v, SmExtent1d(d0, d1), scale, 3, &ctx);
}

// The NURB interval of `c` between the normalised parameters `f0 <= f1`.
SmExtent1d sub_ivl(Rd& rd, const SmBSplineCurve* c)
{
    double f0 = rd.num();
    double f1 = rd.num();
    SmExtent1d nat = c->GetNaturalInterval();
    return SmExtent1d(nat.Evaluate(f0), nat.Evaluate(f1));
}

void put_sols(std::vector<double>& o, SmSolutionArray& sols, const void* a, const void* b)
{
    o.push_back((double)sols.GetSize());
    for (ULONG i = 0; i < sols.GetSize(); i++)
    {
        const SmSolution& s = sols[i];
        auto id = [&](const void* p) { return p == NULL ? -1.0 : p == a ? 0.0 : p == b ? 1.0 : 2.0; };
        o.push_back((double)s.m_eSolutionType);
        o.push_back((double)s.m_lNumVariables);
        o.push_back((double)s.m_lNumObjects);
        o.push_back(id(s.m_apObjects[0]));
        o.push_back(s.m_lNumObjects > 1 ? id(s.m_apObjects[1]) : -1.0);
        o.push_back(s.m_vStart.m_dSolutionValue);
        o.push_back(s.m_vStart[0]);
        o.push_back(s.m_lNumVariables > 1 ? s.m_vStart[1] : 0.0);
    }
}

void put_state(std::vector<double>& o, const SmEllipse* e)
{
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
    const gw_CURVE* cur = e->GetGwNurbPointer();
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

void run_op(int32_t op, Rd& rd, const SmContext& ctx, SmEllipse* a, std::vector<double>& o)
{
    switch (op)
    {
    case OP_STATE:
    {
        o.push_back(0.0);
        o.push_back((double)a->GetType());
        o.push_back((double)SmEllipse::GetClassType());
        o.push_back((double)a->GetDegree());
        o.push_back(a->GetStartAngleDeg());
        o.push_back(a->GetEndAngleDeg());
        o.push_back(flg(a->IsAnalytic()));
        o.push_back(flg(a->IsCircle()));
        put_ivl(o, a->GetMaxAnalyticDomain());
        o.push_back((double)a->IsDomainBoundary(SmExtent2d()));
        SmAxis2Placement cp;
        double r1 = 0.0, r2 = 0.0;
        SmExtent1d dom;
        SmBoolean io = FALSE;
        o.push_back(st(a->GetCanonical(cp, r1, r2, &dom, &io)));
        put_vec(o, cp.GetOriginRef());
        put_vec(o, cp.GetXAxisRef());
        put_vec(o, cp.GetYAxisRef());
        o.push_back(r1);
        o.push_back(r2);
        put_ivl(o, dom);
        o.push_back(flg(io));
        o.push_back(flg(a->GetInsideOut()));
        const SmPolarConversion& pc = polar(a);
        o.push_back((double)pc.GetClassType());
        put_chars(o, pc.GetClassString());
        put_chars(o, a->GetTypeString());
        put_chars(o, a->GetClassString());
        int64_t nt = rd.idx(0, 64);
        for (int64_t i = 0; i < nt; i++)
        {
            double t = rd.num();
            o.push_back(flg(a->IsKindOf((SM_TYPE)t)));
        }
        break;
    }
    case OP_EVAL_STEP:
    {
        double t = rd.num();
        ULONG nd = (ULONG)rd.idx(0, 4);
        SmVector3d pv[5];
        SmStatus s = a->EvaluateSTEP(t, nd, TRUE, pv);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            for (ULONG i = 0; i <= nd; i++)
                put_vec(o, pv[i]);
        break;
    }
    case OP_EVAL_STEP_POINT:
    {
        double t = rd.num();
        SmPoint3d p;
        SmStatus s = a->EvaluateSTEPPoint(t, p);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, p);
        break;
    }
    case OP_STEP_INV:
    {
        SmPoint3d p = rd.vec();
        bool classify = rd.flag();
        double zt = rd.num();
        double t = 0.0;
        SmCurveLocationType loc = SM_CL_UNKNOWN;
        SmStatus s = a->STEPInversion(p, t, classify ? &loc : NULL, zt);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(t);
            if (classify)
                o.push_back((double)loc);
        }
        break;
    }
    case OP_TO_NURBS:
    {
        double t = rd.num();
        double r = 0.0;
        SmStatus s = a->ConvertTFromSTEPToNURBS(t, r);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back(r);
        break;
    }
    case OP_POLAR_TO_NURBS:
    {
        double t = rd.num();
        double zt = rd.num();
        double r = 0.0;
        SmBoolean exact = FALSE;
        SmStatus s = polar(a).ConvertToNURBSParameter(t, r, exact, zt);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(r);
            o.push_back(flg(exact));
        }
        break;
    }
    case OP_ANGLE_TO_NURB:
    {
        double s0 = rd.num();
        double s1 = rd.num();
        double a0 = rd.num();
        double a1 = rd.num();
        double ang = rd.num();
        double r = 0.0;
        SmStatus s = (polar(a).*get(PcAngleFn()))(s0, s1, a0, a1, ang, r);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back(r);
        break;
    }
    case OP_IS_DEGENERATE:
    {
        double tol = rd.num();
        o.push_back(0.0);
        o.push_back(flg(a->IsDegenerate(tol)));
        break;
    }
    case OP_START_PLANE:
    {
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        bool snap = rd.flag();
        SmPoint3d tp;
        ULONG na = 0;
        double ang[2] = {0.0, 0.0};
        SmBoolean inside = FALSE;
        SmStatus s = a->TransformPointToStartPlane(p, tol, tp, na, ang, inside, snap ? TRUE : FALSE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            put_vec(o, tp);
            o.push_back((double)na);
            // on the axis the two angles are the domain ends; else as many as returned
            ULONG nw = na == 0 ? 2 : na;
            for (ULONG i = 0; i < nw; i++)
                o.push_back(ang[i]);
            o.push_back(flg(inside));
        }
        break;
    }
    case OP_DROP:
    {
        SmExtent1d ivl = sub_ivl(rd, a);
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx(0, 9);
        SmPoint3d p = rd.vec();
        bool has_vec = rd.flag();
        SmVector3d v = rd.vec();
        double tol = rd.num();
        bool has_target = rd.flag();
        double target = rd.num();
        SmSolutionRequestedType req = (SmSolutionRequestedType)rd.idx(0, 1);
        SmSolutionArray sols;
        SmStatus s = a->DropPointFast(ivl, sop, p, has_vec ? &v : NULL, tol,
                                      has_target ? &target : NULL, req, sols);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_sols(o, sols, a, NULL);
        break;
    }
    case OP_INTERSECT_ELLIPSE:
    {
        SmExtent1d ivl = sub_ivl(rd, a);
        bool self = rd.flag();
        SmEllipse* b = self ? a : read_ellipse(rd, ctx, false);
        SmExtent1d oivl = sub_ivl(rd, b);
        double tol = rd.num();
        if (b->GetGwNurbPointer() == NULL)
        {
            // the other curve has no NURB (NLib failed): not called (upstream crashes)
            o.push_back(-2.0);
            if (!self)
                delete b;
            break;
        }
        SmBoolean more = TRUE;
        SmSolutionArray sols;
        SmStatus s = a->IntersectWithEllipse(ivl, *b, oivl, tol, more, sols);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(more));
            put_sols(o, sols, a, b);
        }
        if (!self)
            delete b;
        break;
    }
    case OP_INTERSECT_LINE:
    {
        SmExtent1d ivl = sub_ivl(rd, a);
        SmLine* l = read_line(rd, ctx);
        SmExtent1d oivl = sub_ivl(rd, l);
        double tol = rd.num();
        SmBoolean more = TRUE;
        SmSolutionArray sols;
        SmStatus s = a->IntersectWithLine(ivl, *l, oivl, tol, more, sols);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(more));
            put_sols(o, sols, a, l);
        }
        delete l;
        break;
    }
    case OP_TRANSFORM:
    {
        SmPoint3d po = rd.vec();
        SmVector3d px = rd.vec();
        SmVector3d py = rd.vec();
        bool has_scale = rd.flag();
        SmVector3d sc = rd.vec();
        SmAxis2Placement mv;
        mv.SetCanonical(po, px, py);
        o.push_back(st(a->Transform(mv, has_scale ? &sc : NULL)));
        break;
    }
    case OP_TO_3D: o.push_back(st(a->ConvertTo3D())); break;
    case OP_SET_UP:
    {
        SmPolarConversion& pc = a->*get(EllPolar());
        o.push_back(st(pc.SetUpPolarConversion(a, TRUE, a->GetSTEPInterval(), a->GetPosition())));
        break;
    }
    case OP_EQ:
    {
        SmEllipse* b = read_ellipse(rd, ctx, false);
        o.push_back(0.0);
        o.push_back(flg(*a == *b));
        delete b;
        break;
    }
    case OP_SET_STEP_INTERVAL:
    {
        double d0 = rd.num();
        double d1 = rd.num();
        SmExtent1d d(d0, d1);
        a->SetSTEPInterval(d);
        o.push_back(0.0);
        break;
    }
    default: throw Bad();
    }
}
} // namespace

SC_EXPORT int32_t sc_curve_ellipse_p2_run(const double* in, int64_t n_in, double* out, int64_t cap,
                                          int64_t* out_len)
{
    if (in == NULL || n_in < 2)
        return SC_ERR_ARG;
    try
    {
        SC_NATIVE_LOCK;
        SmContext ctx;
        std::vector<double> o;
        Rd rd{in, n_in, 0};
        SmEllipse* a = read_ellipse(rd, ctx, true);
        if (a->GetGwNurbPointer() != NULL)
        {
            int64_t n_ops = rd.idx(0, 64);
            for (int64_t i = 0; i < n_ops; i++)
            {
                int32_t op = (int32_t)rd.idx(1, OP_LAST);
                o.push_back((double)op);
                run_op(op, rd, ctx, a, o);
            }
        }
        put_state(o, a);
        delete a;
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
