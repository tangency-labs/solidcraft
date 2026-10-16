// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.parabola`.
// Wraps upstream SmParabola (source/SMLib/inc/SmParabola.h, source/SMLib/src/SmParabola.cpp).
//
// One entry point, sc_curve_parabola_op(op, in, n_in, data, data_len, out, cap, out_len).
// `in` starts with two curve blocks A and B of CURVE_BLOCK doubles each:
//   kind (0: SmParabola(C, X, Y, SmExtent1d(dmin, dmax), F, dim, ctx);
//         1: SmParabola::CreateCanonical(ctx, SetCanonical(C, X, Y), F)),
//   C(3) X(3) Y(3) dmin dmax F dim
// followed by the op's arguments (documented per op below and in
// crates/solidcore-ffi/src/units/curve_parabola.rs). `out` receives the op status (0 =
// SM_SUCCESS, else the SmStatus value) and the op's outputs. Everything lives in the
// namespace sc_parabola so it cannot clash with the helpers of other shims.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmApiGeneral.h>
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveTypes.h>
#include <SmDatabaseIO.h>
#include <SmExtent1d.h>
#include <SmParabola.h>
#include <SmSolutionArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <memory>
#include <sstream>
#include <string>
#include <vector>

#include "sc_sm.h"

using sc::put_vec;

namespace sc_parabola
{
// Operations (keep in sync with crates/solidcore-ffi/src/units/curve_parabola.rs).
enum : int32_t
{
    OP_STATE = 0,      // state(A)
    OP_EQ = 1,         // args: self_flag            -> A == A or A == B
    OP_ADJUST = 2,     // args: min max              -> AdjustSTEPInterval(SmExtent1d(min, max)), state
    OP_INVERT = 3,     // args: P(3) classify        -> STEPInversion: param, location (-1 when not asked)
    OP_COPY = 4,       //                            -> Copy: state(copy), copy == A
    OP_EVAL_STEP = 5,  // args: t n                  -> EvaluateSTEP(t, n): n + 1 vectors
    OP_GPS_STEP = 6,   // args: min max op P(3) tol has_target target requested
                       //                            -> GlobalPointSolveSTEP: count, (type param value)*, state
    OP_QUERIES = 7,    // args: kind_of_type         -> IsBounded IsAnalytic IsArc STEPInterval(2)
                       //                               MaxAnalyticDomain(2) IsKindOf GetType
                       //                               type string chars, -1, class string chars
    OP_MAKE_NURB = 8,  //                            -> MakeNurb, state
    OP_REVERSE = 9,    // args: min max              -> ReverseParameterization: new interval(2),
                       //                               state after GetOrCreateGwNurbPointer
    OP_TRANSFORM = 10, // args: O(3) X(3) Y(3) has_scale S(3)
                       //                            -> SetCanonical status, Transform, state
    OP_READ_DB = 11,   // args: file_type dim version via_curve; data = stream bytes
                       //                            -> ReadFromDB (SmParabola:: or SmCurve:: with
                       //                               SmParabola_TYPE), state after GetOrCreateGwNurbPointer
    OP_VIRTUALS = 12,  // args: P(3)                 -> through SmCurve*: STEPInversion status, param,
                       //                               IsAnalytic, IsBounded, GetType
    OP_LAST = 12,
};

const int64_t CURVE_BLOCK = 14;

// Reads the protected STEP flags through pointers to members named in a derived class.
struct ParPeek : SmBSplineCurve
{
    static SmKnotType SmBSplineCurve::*kt() { return &ParPeek::m_eKnotType; }
    static SmBSplineCurveForm SmBSplineCurve::*form() { return &ParPeek::m_eBSplineCurveForm; }
};

struct Rd
{
    const double* in;
    int64_t n;
    int64_t at;
    bool ok = true;
    double num()
    {
        if (at >= n)
        {
            ok = false;
            return 0.0;
        }
        return in[at++];
    }
    int64_t idx() { return (int64_t)num(); }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }

double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

// A curve block; NULL for a bad kind.
SmParabola* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    SmVector3d c = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    double dmin = rd.num();
    double dmax = rd.num();
    double f = rd.num();
    int64_t dim = rd.idx();
    if (kind == 0)
        return new (ctx) SmParabola(c, x, y, SmExtent1d(dmin, dmax), f, (ULONG)dim, &ctx);
    if (kind == 1)
    {
        SmAxis2Placement a2p;
        a2p.SetCanonical(c, x, y);
        SmParabola* p = NULL;
        SmParabola::CreateCanonical(ctx, a2p, f, p);
        return p;
    }
    return NULL;
}

// type dim oob borrowed knot_type form O(3) X(3) Y(3) F STEPInterval(2) has_nurb [n p m cpts(xyzw) knots]
void put_state(std::vector<double>& o, const SmParabola* c)
{
    o.push_back((double)c->GetType());
    o.push_back((double)c->GetDim());
    o.push_back(flg(c->IsOutOfBoundsEnabled()));
    o.push_back(flg(c->IsNurbBorrowed()));
    o.push_back((double)(c->*ParPeek::kt()));
    o.push_back((double)(c->*ParPeek::form()));
    SmAxis2Placement a2p;
    double f = 0.0;
    c->GetCanonical(a2p, f);
    put_vec(o, a2p.GetOriginRef());
    put_vec(o, a2p.GetXAxisRef());
    put_vec(o, a2p.GetYAxisRef());
    o.push_back(f);
    SmExtent1d ivl = c->GetSTEPInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
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

void put_string(std::vector<double>& o, const TCHAR* s)
{
    for (; *s; ++s)
        o.push_back((double)*s);
}

int32_t read_db(Rd& rd, const uint8_t* data, int64_t data_len, const SmContext& ctx, std::vector<double>& o)
{
    int64_t file_type = rd.idx();
    ULONG dim = (ULONG)rd.idx();
    ULONG version = (ULONG)rd.idx();
    bool via_curve = rd.idx() != 0;
    if (!rd.ok || (file_type != 0 && file_type != 1) || data_len < 0)
        return SC_ERR_ARG;
    std::istringstream iss(std::string((const char*)data, (size_t)data_len));
    SmDatabaseIOStream db(file_type == 0 ? SM_ASCII : SM_BINARY, &iss);
    SmCurve* c = NULL;
    SmStatus s = via_curve ? SmCurve::ReadFromDB(SmParabola_TYPE, db, dim, ctx, c, version)
                           : SmParabola::ReadFromDB(SmParabola_TYPE, db, dim, ctx, c, version);
    o.push_back(st(s));
    if (s == SM_SUCCESS && c != NULL)
    {
        SmParabola* p = (SmParabola*)c;
        p->GetOrCreateGwNurbPointer();
        put_state(o, p);
    }
    delete c;
    return SC_OK;
}

int32_t run(int32_t op, Rd& rd, const uint8_t* data, int64_t data_len, std::vector<double>& o)
{
    const SmContext& ctx = *SmApiGetOrCreateContext();
    std::unique_ptr<SmParabola> a(read_curve(rd, ctx));
    std::unique_ptr<SmParabola> b(read_curve(rd, ctx));
    if (!rd.ok || !a || !b)
        return SC_ERR_ARG;
    switch (op)
    {
    case OP_STATE:
        o.push_back(0.0);
        put_state(o, a.get());
        break;
    case OP_EQ:
    {
        bool self = rd.idx() != 0;
        o.push_back(0.0);
        o.push_back(flg(self ? (*a == *(SmCurve*)a.get()) : (*a == *(SmCurve*)b.get())));
        break;
    }
    case OP_ADJUST:
    {
        double lo = rd.num();
        double hi = rd.num();
        SmStatus s = a->AdjustSTEPInterval(SmExtent1d(lo, hi));
        o.push_back(st(s));
        put_state(o, a.get());
        break;
    }
    case OP_INVERT:
    {
        SmVector3d p = rd.vec();
        bool classify = rd.idx() != 0;
        double t = 0.0;
        SmCurveLocationType loc = SM_CL_UNKNOWN;
        SmStatus s = a->STEPInversion(p, t, classify ? &loc : NULL);
        o.push_back(st(s));
        o.push_back(t);
        o.push_back(classify ? (double)loc : -1.0);
        break;
    }
    case OP_COPY:
    {
        SmCurve* c = NULL;
        SmStatus s = a->Copy(ctx, c);
        o.push_back(st(s));
        if (s == SM_SUCCESS && c != NULL)
        {
            put_state(o, (SmParabola*)c);
            o.push_back(flg(*c == *(SmCurve*)a.get()));
        }
        delete c;
        break;
    }
    case OP_EVAL_STEP:
    {
        double t = rd.num();
        int64_t n = rd.idx();
        if (n < 0 || n > 16)
            return SC_ERR_ARG;
        std::vector<SmVector3d> pd((size_t)n + 1, SmVector3d(0.0, 0.0, 0.0));
        SmStatus s = a->EvaluateSTEP(t, (ULONG)n, TRUE, pd.data());
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            for (const SmVector3d& v : pd)
                put_vec(o, v);
        break;
    }
    case OP_GPS_STEP:
    {
        double lo = rd.num();
        double hi = rd.num();
        int64_t sop = rd.idx();
        SmVector3d p = rd.vec();
        double tol = rd.num();
        bool has_target = rd.idx() != 0;
        double target = rd.num();
        int64_t req = rd.idx();
        if (sop != SM_SO_MINIMIZE && sop != SM_SO_MAXIMIZE && sop != SM_SO_NORMALIZE && sop != SM_SO_INTERSECT)
            return SC_ERR_ARG;
        if (req != SM_SR_SINGLE && req != SM_SR_ALL)
            return SC_ERR_ARG;
        SmSolutionArray sols;
        SmStatus s = a->GlobalPointSolveSTEP(SmExtent1d(lo, hi), (SmSolverOperationType)sop, p, tol,
                                             has_target ? &target : NULL, NULL, (SmSolutionRequestedType)req,
                                             sols);
        o.push_back(st(s));
        o.push_back((double)sols.GetSize());
        for (ULONG i = 0; i < sols.GetSize(); i++)
        {
            o.push_back((double)sols[i].m_eSolutionType);
            o.push_back(sols[i].m_vStart[0]);
            o.push_back(sols[i].m_vStart.m_dSolutionValue);
        }
        put_state(o, a.get());
        break;
    }
    case OP_QUERIES:
    {
        SM_TYPE t = (SM_TYPE)rd.idx();
        o.push_back(0.0);
        o.push_back(flg(a->IsBounded()));
        o.push_back(flg(a->IsAnalytic()));
        SmAxis2Placement arc_a2p;
        double r = 0.0, s0 = 0.0, s1 = 0.0;
        o.push_back(flg(a->IsArc(0, 0.0, arc_a2p, r, s0, s1)));
        SmExtent1d si = a->GetSTEPInterval();
        o.push_back(si.GetMin());
        o.push_back(si.GetMax());
        SmExtent1d md = a->GetMaxAnalyticDomain();
        o.push_back(md.GetMin());
        o.push_back(md.GetMax());
        o.push_back(flg(a->IsKindOf(t)));
        o.push_back((double)a->GetType());
        put_string(o, a->GetTypeString());
        o.push_back(-1.0);
        put_string(o, a->GetClassString());
        break;
    }
    case OP_MAKE_NURB:
    {
        SmStatus s = a->MakeNurb();
        o.push_back(st(s));
        put_state(o, a.get());
        break;
    }
    case OP_REVERSE:
    {
        double lo = rd.num();
        double hi = rd.num();
        SmExtent1d nivl;
        SmStatus s = a->ReverseParameterization(SmExtent1d(lo, hi), nivl);
        o.push_back(st(s));
        o.push_back(nivl.GetMin());
        o.push_back(nivl.GetMax());
        a->GetOrCreateGwNurbPointer();
        put_state(o, a.get());
        break;
    }
    case OP_TRANSFORM:
    {
        SmVector3d po = rd.vec();
        SmVector3d px = rd.vec();
        SmVector3d py = rd.vec();
        bool has_scale = rd.idx() != 0;
        SmVector3d sc = rd.vec();
        SmAxis2Placement a2p;
        SmStatus cs = a2p.SetCanonical(po, px, py);
        if (cs != SM_SUCCESS)
        {
            // the frame is part of the input; report its status only
            o.push_back(-2.0);
            o.push_back(st(cs));
            break;
        }
        SmStatus s = a->Transform(a2p, has_scale ? &sc : NULL);
        o.push_back(st(s));
        put_state(o, a.get());
        break;
    }
    case OP_READ_DB: return read_db(rd, data, data_len, ctx, o);
    case OP_VIRTUALS:
    {
        SmVector3d p = rd.vec();
        SmCurve* c = a.get();
        double t = 0.0;
        SmStatus s = c->STEPInversion(p, t);
        o.push_back(0.0);
        o.push_back(st(s));
        o.push_back(t);
        o.push_back(flg(c->IsAnalytic()));
        o.push_back(flg(c->IsBounded()));
        o.push_back((double)c->GetType());
        break;
    }
    default: return SC_ERR_ARG;
    }
    return rd.ok ? SC_OK : SC_ERR_ARG;
}
} // namespace sc_parabola

SC_EXPORT int32_t sc_curve_parabola_op(int32_t op, const double* in, int64_t n_in, const uint8_t* data,
                                       int64_t data_len, double* out, int64_t cap, int64_t* out_len)
{
    if (op < 0 || op > sc_parabola::OP_LAST || n_in < 2 * sc_parabola::CURVE_BLOCK)
        return SC_ERR_ARG;
    SC_TRY
    sc_parabola::Rd rd{in, n_in, 0};
    std::vector<double> o;
    int32_t r = sc_parabola::run(op, rd, data, data_len, o);
    if (r != SC_OK)
        return r;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
