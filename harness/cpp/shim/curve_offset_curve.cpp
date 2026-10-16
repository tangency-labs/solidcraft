// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.offset_curve`.
// Wraps upstream SmOffsetCurve (source/SMLib/inc/SmOffsetCurve.h, source/SMLib/src/SmOffsetCurve.cpp).
//
// One entry point, sc_curve_offset_curve_op(op, in, n_in, out, cap, out_len). The input
// layout is documented (and validated) in crates/solidcore-ffi/src/units/curve_offset_curve.rs:
//   curve ops:  op  base-block  spec  has_outer [spec]  args...
//   READ_DB:    op  file_type version nb bytes[nb]  probes
//   WRITE_DB:   curve op with args `file_type version` (a fixture helper: upstream
//               WriteToDB, not part of the unit)
// base-block: 0 dim oob n p m cpts(x y z w) knots   (SmBSplineCurve(gw_CURVE*, dim, FALSE))
//             1 dim P1 D1 P2 D2                      (SmHermiteCurve)
// spec:       dim a b nx ny nz dist owns nrev        (SmOffsetCurve(dim, base, SmExtent1d(a, b),
//             normal, dist, owns), then nrev x ReverseParameterization(natural interval))
// probes:     k t[k]: per t EvaluatePoint (status [P]) and Evaluate(t, 3, TRUE, ..., TRUE)
//             (status [4 vectors]).
// Every output starts with the op status (0 = SM_SUCCESS, else the SmStatus value).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmApiGeneral.h>
#include <SmBSplineCurve.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveTypes.h>
#include <SmDatabaseIO.h>
#include <SmExtent1d.h>
#include <SmHermiteCurve.h>
#include <SmNurbsCrv.h>
#include <SmOffsetCurve.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <cstring>
#include <sstream>
#include <string>
#include <vector>

#include "sc_sm.h"

using sc::Rd;
using sc::put_vec;

namespace
{
// Operations (keep in sync with crates/solidcore-ffi/src/units/curve_offset_curve.rs).
enum ScOcOp : int32_t
{
    OP_STATE = 0,
    OP_EVAL = 1,
    OP_EVAL_POINT = 2,
    OP_KNOTS = 3,
    OP_CONTINUITIES = 4,
    OP_REVERSE = 5,
    OP_TRIM = 6,
    OP_EQ = 7,
    OP_COPY = 8,
    OP_IS_KIND_OF = 9,
    OP_READ_DB = 10,
    OP_WRITE_DB = 11,
    OP_LAST = 11,
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

SmCurve* read_base(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    ULONG dim = (ULONG)rd.idx();
    if (kind == 0)
    {
        bool oob = rd.flag();
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
        SmBSplineCurve* c = new (ctx) SmBSplineCurve(cur, dim, FALSE);
        c->SetOutOfBoundsEnabled(oob ? TRUE : FALSE);
        return c;
    }
    SmVector3d p1 = rd.vec();
    SmVector3d d1 = rd.vec();
    SmVector3d p2 = rd.vec();
    SmVector3d d2 = rd.vec();
    return new (ctx) SmHermiteCurve(p1, d1, p2, d2, dim);
}

// An offset of `base` from a spec block; *owns receives the ownership flag.
SmOffsetCurve* read_offset(Rd& rd, const SmContext& ctx, SmCurve* base, bool* owns, bool never_own = false)
{
    ULONG dim = (ULONG)rd.idx();
    double a = rd.num();
    double b = rd.num();
    SmVector3d n = rd.vec();
    double d = rd.num();
    *owns = rd.flag() && !never_own;
    int64_t nrev = rd.idx();
    SmOffsetCurve* c = new (ctx) SmOffsetCurve(dim, *base, SmExtent1d(a, b), n, d, *owns ? TRUE : FALSE);
    for (int64_t i = 0; i < nrev; i++)
    {
        SmExtent1d sNew;
        c->ReverseParameterization(c->GetNaturalInterval(), sNew);
    }
    return c;
}

// The members through the upstream binary writer: normal, distance, interval, reversed.
void put_members(std::vector<double>& o, const SmOffsetCurve* c)
{
    std::ostringstream oss;
    SmDatabaseIOStream db(SM_BINARY, &oss);
    c->WriteToDB(db, 30);
    std::string s = oss.str();
    if (s.size() < 6 * sizeof(double) + sizeof(SmBoolean))
    {
        o.push_back(-1.0);
        return;
    }
    for (int i = 0; i < 6; ++i)
    {
        double d;
        std::memcpy(&d, s.data() + i * sizeof(double), sizeof(double));
        o.push_back(d);
    }
    SmBoolean b;
    std::memcpy(&b, s.data() + 6 * sizeof(double), sizeof(SmBoolean));
    o.push_back((double)b);
}

// State: type dim members[7] base_type nnk.
void put_state(std::vector<double>& o, const SmOffsetCurve* c)
{
    o.push_back((double)c->GetType());
    o.push_back((double)c->GetDim());
    put_members(o, c);
    const SmCurve* b = c->GetBaseCurve();
    o.push_back(b ? (double)b->GetType() : -1.0);
    o.push_back(b ? (double)c->GetNumberNaturalKnots() : -1.0);
}

void put_probes(std::vector<double>& o, Rd& rd, const SmCurve* c)
{
    int64_t k = rd.idx();
    for (int64_t i = 0; i < k; i++)
    {
        double t = rd.num();
        SmPoint3d p(0.0, 0.0, 0.0);
        SmStatus s = c->EvaluatePoint(t, p);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, p);
        SmVector3d v[4];
        s = c->Evaluate(t, 3, TRUE, v, TRUE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            for (int j = 0; j < 4; j++)
                put_vec(o, v[j]);
    }
}

void skip_probes(Rd& rd)
{
    int64_t k = rd.idx();
    rd.at += k;
}

struct Built
{
    SmCurve* base = NULL;
    SmOffsetCurve* inner = NULL;
    bool inner_owns = false;
    SmOffsetCurve* outer = NULL;
    bool outer_owns = false;
    SmOffsetCurve* top() const { return outer ? outer : inner; }
    ~Built()
    {
        // an owning offset deletes the curve it is built on (and that one its own)
        bool inner_gone = false;
        if (outer)
        {
            delete outer;
            inner_gone = outer_owns;
        }
        if (!inner_gone)
            delete inner;
        if (!inner_owns)
            delete base;
    }
};

int32_t run(int32_t op, Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = *SmApiGetOrCreateContext();
    if (op == OP_READ_DB)
    {
        int64_t ft = rd.idx();
        ULONG version = (ULONG)rd.idx();
        int64_t nb = rd.idx();
        std::string bytes;
        for (int64_t i = 0; i < nb; i++)
            bytes.push_back((char)(unsigned char)rd.idx());
        std::istringstream iss(bytes);
        SmDatabaseIOStream db(ft == 0 ? SM_ASCII : SM_BINARY, &iss);
        SmCurve* c = NULL;
        SmStatus s = SmOffsetCurve::ReadFromDB(SmOffsetCurve_TYPE, db, 3, ctx, c, version);
        o.push_back(st(s));
        if (s == SM_SUCCESS && c != NULL)
        {
            put_state(o, (SmOffsetCurve*)c);
            put_probes(o, rd, c);
            delete c;
        }
        else
            skip_probes(rd);
        return SC_OK;
    }

    Built b;
    b.base = read_base(rd, ctx);
    b.inner = read_offset(rd, ctx, b.base, &b.inner_owns);
    if (rd.flag())
        b.outer = read_offset(rd, ctx, b.inner, &b.outer_owns);
    SmOffsetCurve* c = b.top();

    switch (op)
    {
    case OP_STATE:
    {
        o.push_back(0.0);
        put_state(o, c);
        break;
    }
    case OP_EVAL:
    {
        double t = rd.num();
        ULONG n = (ULONG)rd.idx();
        bool left = rd.flag();
        bool nonzero = rd.flag();
        std::vector<SmVector3d> v((size_t)n + 1, SmVector3d(0.0, 0.0, 0.0));
        SmStatus s = c->Evaluate(t, n, left ? TRUE : FALSE, v.data(), nonzero ? TRUE : FALSE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            for (const SmVector3d& x : v)
                put_vec(o, x);
        break;
    }
    case OP_EVAL_POINT:
    {
        double t = rd.num();
        SmPoint3d p(0.0, 0.0, 0.0);
        SmStatus s = c->EvaluatePoint(t, p);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, p);
        break;
    }
    case OP_KNOTS:
    {
        bool has_ivl = rd.flag();
        double a = rd.num();
        double bb = rd.num();
        bool want_mults = rd.flag();
        SmExtent1d ivl(a, bb);
        SmTArray<double> knots;
        SmTArray<ULONG> mults;
        SmStatus s = c->GetKnots(knots, want_mults ? &mults : NULL, has_ivl ? &ivl : NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)knots.GetSize());
            for (ULONG i = 0; i < knots.GetSize(); ++i)
                o.push_back(knots[i]);
            o.push_back((double)mults.GetSize());
            for (ULONG i = 0; i < mults.GetSize(); ++i)
                o.push_back((double)mults[i]);
        }
        break;
    }
    case OP_CONTINUITIES:
    {
        double angle = rd.num();
        SmContinuityType eMin = SM_CT_UNDEFINED;
        SmTArray<SmContinuityType> conts;
        SmStatus s = c->CalculateContinuities(eMin, conts, angle);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)eMin);
            o.push_back((double)conts.GetSize());
            for (ULONG i = 0; i < conts.GetSize(); ++i)
                o.push_back((double)conts[i]);
        }
        break;
    }
    case OP_REVERSE:
    {
        double a = rd.num();
        double bb = rd.num();
        SmExtent1d sNew;
        SmStatus s = c->ReverseParameterization(SmExtent1d(a, bb), sNew);
        o.push_back(st(s));
        o.push_back(sNew.GetMin());
        o.push_back(sNew.GetMax());
        put_state(o, c);
        put_probes(o, rd, c);
        break;
    }
    case OP_TRIM:
    {
        double a = rd.num();
        double bb = rd.num();
        int64_t mode = rd.idx();
        SmExtent1d ivl(a, bb);
        SmBoolean bNotify = mode == 0 ? TRUE : mode == 1 ? FALSE : UNSURE;
        SmStatus s = c->Trim(ivl, bNotify);
        o.push_back(st(s));
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
        put_state(o, c);
        put_probes(o, rd, c);
        break;
    }
    case OP_EQ:
    {
        bool self = rd.flag();
        bool copy = rd.flag();
        o.push_back(0.0);
        if (self)
        {
            o.push_back(flg(*c == *c));
        }
        else if (copy)
        {
            SmCurve* cp = NULL;
            c->Copy(ctx, cp);
            o.push_back(flg(*c == *cp));
            o.push_back(flg(*cp == *c));
            delete cp;
        }
        else
        {
            // a second offset over the same curve the top offset is built on
            SmCurve* under = b.outer ? (SmCurve*)b.inner : b.base;
            bool owns2 = false;
            // the shared curve stays owned by `b`: the ownership flag is read but not
            // passed (operator== does not compare it)
            SmOffsetCurve* c2 = read_offset(rd, ctx, under, &owns2, true);
            o.push_back(flg(*c == *c2));
            o.push_back(flg(*c2 == *c));
            delete c2;
        }
        break;
    }
    case OP_COPY:
    {
        SmCurve* cp = NULL;
        SmStatus s = c->Copy(ctx, cp);
        o.push_back(st(s));
        if (s == SM_SUCCESS && cp != NULL)
        {
            put_state(o, (SmOffsetCurve*)cp);
            put_probes(o, rd, cp);
            delete cp;
        }
        else
            skip_probes(rd);
        break;
    }
    case OP_WRITE_DB:
    {
        // fixture helper: the bytes upstream WriteToDB produces (ASCII with 17 digits)
        int64_t ft = rd.idx();
        ULONG version = (ULONG)rd.idx();
        std::ostringstream oss;
        oss.precision(17);
        SmDatabaseIOStream db(ft == 0 ? SM_ASCII : SM_BINARY, &oss);
        SmStatus s = c->WriteToDB(db, version);
        o.push_back(st(s));
        std::string bytes = oss.str();
        for (char ch : bytes)
            o.push_back((double)(unsigned char)ch);
        break;
    }
    case OP_IS_KIND_OF:
    {
        SM_TYPE t = (SM_TYPE)rd.idx();
        o.push_back(0.0);
        o.push_back(flg(c->IsKindOf(t)));
        break;
    }
    default: return SC_ERR_ARG;
    }
    return SC_OK;
}
} // namespace

// op: one of ScOcOp; in/n_in: the flat input (validated by the Rust wrapper).
SC_EXPORT int32_t sc_curve_offset_curve_op(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                           int64_t* out_len)
{
    if (op < 0 || op > OP_LAST || n_in < 0)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    int32_t r = run(op, rd, o);
    if (r != SC_OK)
        return r;
    if (rd.at != n_in)
        return SC_ERR_ARG;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
