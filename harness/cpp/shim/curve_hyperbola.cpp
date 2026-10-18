// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.hyperbola`.
// Wraps upstream SmHyperbola (source/SMLib/inc/SmHyperbola.h, source/SMLib/src/SmHyperbola.cpp):
// the constructor (and so MakeNurb), operator==, AdjustSTEPInterval, STEPInversion, Copy,
// EvaluateSTEP, GlobalPointSolveSTEP, IsBounded, IsAnalytic, IsArc, GetSTEPInterval,
// GetMaxAnalyticDomain, ReverseParameterization, Transform, ReadFromDB, IsKindOf and the type
// strings.
//
// One entry point, sc_curve_hyperbola_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/curve_hyperbola.rs, which validates every input.
// Member ops start with a hyperbola block `O[3] X[3] Y[3] dmin dmax a b dim`
// (`new (ctx) SmHyperbola(O, X, Y, SmExtent1d(dmin, dmax), a, b, dim, &ctx)`). `out` receives the
// op status (0 = SM_SUCCESS, else the SmStatus value), the op's outputs, then the state of `this`
// (member ops) or of the curve read (ReadDb). A state is
// `O[3] X[3] Y[3] dmin dmax a b dim oob borrowed knot_type form has_nurb [n p m cpts knots]`.
#include "sc_shim.h"

#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmContext.h>
#include <SmCurveTypes.h>
#include <SmDatabaseIO.h>
#include <SmExtent1d.h>
#include <SmHyperbola.h>
#include <SmSolutionArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <sstream>
#include <string>
#include <vector>

#include "sc_sm.h"

using sc::Rd;
using sc::flg;
using sc::put_vec;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/curve_hyperbola.rs).
enum : int32_t
{
    OP_STATE = 0,
    OP_EVAL_STEP = 1,
    OP_STEP_INV = 2,
    OP_ADJUST = 3,
    OP_REVERSE = 4,
    OP_TRANSFORM = 5,
    OP_GPS = 6,
    OP_EQ = 7,
    OP_COPY = 8,
    OP_READ_DB = 9,
};

// Reads the protected STEP flags through pointers to members named in a derived class.
struct Peek : SmBSplineCurve
{
    static SmKnotType SmBSplineCurve::*kt() { return &Peek::m_eKnotType; }
    static SmBSplineCurveForm SmBSplineCurve::*form() { return &Peek::m_eBSplineCurveForm; }
};

SmHyperbola* read_hyperbola(Rd& rd, const SmContext& ctx)
{
    SmPoint3d c = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    double d0 = rd.num();
    double d1 = rd.num();
    double a = rd.num();
    double b = rd.num();
    ULONG dim = (ULONG)rd.idx();
    return new (ctx) SmHyperbola(c, x, y, SmExtent1d(d0, d1), a, b, dim, &ctx);
}

void put_state(std::vector<double>& o, const SmHyperbola* h)
{
    SmAxis2Placement pos;
    double a = 0.0;
    double b = 0.0;
    h->GetCanonical(pos, a, b);
    put_vec(o, pos.GetOriginRef());
    put_vec(o, pos.GetXAxisRef());
    put_vec(o, pos.GetYAxisRef());
    SmExtent1d d = h->GetSTEPInterval();
    o.push_back(d.GetMin());
    o.push_back(d.GetMax());
    o.push_back(a);
    o.push_back(b);
    o.push_back((double)h->GetDim());
    o.push_back(h->IsOutOfBoundsEnabled() ? 1.0 : 0.0);
    o.push_back(h->IsNurbBorrowed() ? 1.0 : 0.0);
    o.push_back((double)(h->*Peek::kt()));
    o.push_back((double)(h->*Peek::form()));
    const gw_CURVE* cur = h->GetGwNurbPointer();
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

void member_op(int32_t op, Rd& rd, const SmContext& ctx, SmHyperbola* h, std::vector<double>& o)
{
    switch (op)
    {
    case OP_STATE:
    {
        o.push_back(0.0);
        o.push_back(flg(h->IsBounded()));
        o.push_back(flg(h->IsAnalytic()));
        SmAxis2Placement arc_pos;
        double r = 0.0, a0 = 0.0, a1 = 0.0;
        o.push_back(flg(h->IsArc(0, 0.0, arc_pos, r, a0, a1)));
        SmExtent1d md = h->GetMaxAnalyticDomain();
        o.push_back(md.GetMin());
        o.push_back(md.GetMax());
        SmExtent1d sd = h->GetSTEPInterval();
        o.push_back(sd.GetMin());
        o.push_back(sd.GetMax());
        o.push_back((double)h->GetType());
        for (const TCHAR* s = h->GetTypeString(); *s; ++s)
            o.push_back((double)*s);
        o.push_back(-1.0);
        for (const TCHAR* s = h->GetClassString(); *s; ++s)
            o.push_back((double)*s);
        o.push_back(-1.0);
        int64_t nt = rd.idx();
        for (int64_t i = 0; i < nt; i++)
            o.push_back(flg(h->IsKindOf((SM_TYPE)rd.idx())));
        break;
    }
    case OP_EVAL_STEP:
    {
        double t = rd.num();
        ULONG nd = (ULONG)rd.idx();
        SmVector3d pv[4];
        SmStatus s = h->EvaluateSTEP(t, nd, TRUE, pv);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            for (ULONG i = 0; i <= nd; i++)
                put_vec(o, pv[i]);
        break;
    }
    case OP_STEP_INV:
    {
        SmPoint3d p = rd.vec();
        bool classify = rd.flag();
        double t = 0.0;
        SmCurveLocationType loc = SM_CL_UNKNOWN;
        SmStatus s = h->STEPInversion(p, t, classify ? &loc : NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(t);
            if (classify)
                o.push_back((double)loc);
        }
        break;
    }
    case OP_ADJUST:
    {
        double a = rd.num();
        double b = rd.num();
        o.push_back(st(h->AdjustSTEPInterval(SmExtent1d(a, b))));
        break;
    }
    case OP_REVERSE:
    {
        double a = rd.num();
        double b = rd.num();
        SmExtent1d old(a, b);
        SmExtent1d nw(old);
        SmStatus s = h->ReverseParameterization(old, nw);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(nw.GetMin());
            o.push_back(nw.GetMax());
        }
        // the NURB upstream rebuilds lazily, made here before the state is read
        h->GetOrCreateGwNurbPointer();
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
        o.push_back(st(h->Transform(mv, has_scale ? &sc : NULL)));
        break;
    }
    case OP_GPS:
    {
        double a = rd.num();
        double b = rd.num();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        bool has_target = rd.flag();
        double target = rd.num();
        SmSolutionRequestedType req = (SmSolutionRequestedType)rd.idx();
        SmSolutionArray sols;
        // GlobalPointSolve dereferences the NURB: refuse a curve that would have none
        // after the interval adjustment (upstream crashes).
        SmHyperbola* probe = new (ctx) SmHyperbola(*h);
        bool no_nurb = probe->AdjustSTEPInterval(SmExtent1d(a, b)) == SM_SUCCESS &&
                       probe->GetGwNurbPointer() == NULL;
        delete probe;
        if (no_nurb)
        {
            o.push_back((double)SC_ERR_ARG);
            break;
        }
        SmStatus s = h->GlobalPointSolveSTEP(SmExtent1d(a, b), sop, p, tol,
                                             has_target ? &target : NULL, NULL, req, sols);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)sols.GetSize());
            for (ULONG i = 0; i < sols.GetSize(); i++)
                o.push_back(sols[i].m_vStart[0]);
        }
        break;
    }
    case OP_EQ:
    {
        SmHyperbola* other = read_hyperbola(rd, ctx);
        o.push_back(0.0);
        o.push_back(flg(*h == *other));
        delete other;
        break;
    }
    case OP_COPY:
    {
        SmCurve* c = NULL;
        SmStatus s = h->Copy(ctx, c);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(*h == *c));
            put_state(o, (SmHyperbola*)c);
        }
        delete c;
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_curve_hyperbola_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                          int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    if (op == OP_READ_DB)
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
        SmStatus s = SmCurve::ReadFromDB(SmHyperbola_TYPE, db, dim, ctx, c, version);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(c ? 1.0 : 0.0);
            if (c)
                put_state(o, (SmHyperbola*)c);
        }
        delete c;
    }
    else
    {
        SmHyperbola* h = read_hyperbola(rd, ctx);
        member_op(op, rd, ctx, h, o);
        put_state(o, h);
        delete h;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
