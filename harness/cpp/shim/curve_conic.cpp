// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.conic`.
// Wraps upstream SmConic (source/SMLib/inc/SmConic.h, source/SMLib/src/SmConic.cpp).
#include "sc_shim.h"

#include <SmApiGeneral.h>
#include <SmConic.h>
#include <SmDatabaseIO.h>
#include <SmExtent1d.h>
#include <SmSolutionArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>

#include <cstring>
#include <sstream>
#include <string>
#include <vector>

#include "sc_sm.h"

using sc::put_vec;

// Operations (keep in sync with crates/solidcore-ffi/src/units/curve_conic.rs).
enum ScCnOp : int32_t
{
    OP_STATE = 0,
    OP_EVALUATE = 1,
    OP_EVALUATE_POINT = 2,
    OP_IMPLICIT = 3,
    OP_INTERSECT_IMPLICIT = 4,
    OP_INTERSECT_CONIC = 5,
    OP_INVERT = 6,
    OP_KNOTS = 7,
    OP_CONTINUITIES = 8,
    OP_EQ = 9,
    OP_IS_KIND_OF = 10,
    OP_TYPE_STRINGS = 11,
    OP_READ_DB = 12,
    OP_LAST = 12,
};

// A conic description: [type, a, b, ntr, (angle, tx, ty) * ntr].
// type -1 = SmConic() (empty constructor), else SmConic((SmConicType)type, a, b);
// then Transform2D(angle, SmPoint2d(tx, ty)) for each transform in order.
static bool sc_check(const double* c, int64_t len)
{
    if (len < 4)
        return false;
    double t = c[0];
    if (!(t == -1.0 || t == 0.0 || t == 1.0 || t == 2.0 || t == 3.0 || t == 4.0))
        return false;
    double n = c[3];
    if (!(n >= 0.0 && n <= 16.0) || n != (double)(int64_t)n)
        return false;
    return len == 4 + 3 * (int64_t)n;
}

static SmConic sc_make(int t, double a, double b)
{
    if (t >= 0)
        return SmConic((SmConicType)t, a, b);
    return SmConic();
}

static SmConic sc_build(const double* c)
{
    SmConic k = sc_make((int)c[0], c[1], c[2]);
    int64_t n = (int64_t)c[3];
    for (int64_t i = 0; i < n; ++i)
        k.Transform2D(c[4 + 3 * i], SmPoint2d(c[5 + 3 * i], c[6 + 3 * i]));
    return k;
}

// Type code and the five member doubles (A, B, origin x, y, theta) through the
// upstream binary writer, then GetType, GetDim and the natural interval.
static int32_t put_state(std::vector<double>& o, const SmConic& k)
{
    std::ostringstream oss;
    SmDatabaseIOStream db(SM_BINARY, &oss);
    SmStatus st = k.WriteToDB(db, 30);
    if (st != SM_SUCCESS)
        return (int32_t)st;
    std::string s = oss.str();
    if (s.size() != 8 + 5 * sizeof(double))
        return SC_ERR_ARG;
    long long l;
    std::memcpy(&l, s.data(), 8);
    o.push_back((double)l);
    for (int i = 0; i < 5; ++i)
    {
        double d;
        std::memcpy(&d, s.data() + 8 + i * sizeof(double), sizeof(double));
        o.push_back(d);
    }
    o.push_back((double)k.GetType());
    o.push_back((double)k.GetDim());
    SmExtent1d ivl = k.GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    return SC_OK;
}

static double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

// op: one of ScCnOp on conic A (ca, ca_len), conic B (cb, cb_len) for eq / intersect.
// t: parameter or tolerance; n: derivative count / type code; flags: per-op bits;
// w[6]: extra values; data/data_len/file_type: database bytes (0 = ASCII, 1 = binary).
// Status-bearing ops put the upstream SmStatus first.
SC_EXPORT int32_t sc_curve_conic_op(int32_t op, const double* ca, int64_t ca_len, const double* cb, int64_t cb_len,
                                    double t, int64_t n, int32_t flags, const double* w, const uint8_t* data,
                                    int64_t data_len, int32_t file_type, double* out, int64_t cap, int64_t* out_len)
{
    if (op < 0 || op > OP_LAST || !sc_check(ca, ca_len) || !sc_check(cb, cb_len))
        return SC_ERR_ARG;
    SC_TRY
    std::vector<double> o;
    SmConic a = sc_build(ca);
    switch (op)
    {
    case OP_STATE:
    {
        int32_t st = put_state(o, a);
        if (st != SC_OK)
            return st;
        break;
    }
    case OP_EVALUATE:
    {
        // flags bit 0: bNonZeroTangents, bit 1: bFromLeft. Array of fresh SmVector3d.
        if (n < 0 || n > 16)
            return SC_ERR_ARG;
        std::vector<SmVector3d> pd((size_t)n + 1);
        SmStatus st = a.Evaluate(t, (ULONG)n, (flags & 2) ? TRUE : FALSE, pd.data(), (flags & 1) ? TRUE : FALSE);
        o.push_back((double)st);
        for (const SmVector3d& v : pd)
            put_vec(o, v);
        break;
    }
    case OP_EVALUATE_POINT:
    {
        SmPoint3d p;
        SmStatus st = a.EvaluatePoint(t, p);
        o.push_back((double)st);
        put_vec(o, p);
        break;
    }
    case OP_IMPLICIT:
    {
        double k[6];
        SmStatus st = a.ComputeImplicitEquation(k);
        o.push_back((double)st);
        for (int i = 0; i < 6; ++i)
            o.push_back(k[i]);
        break;
    }
    case OP_INTERSECT_IMPLICIT:
    {
        // An unknown conic solves a quartic of uninitialised coefficients.
        if (ca[0] == -1.0 || ca[0] == 4.0)
            return SC_ERR_ARG;
        double k[6];
        for (int i = 0; i < 6; ++i)
            k[i] = w[i];
        SmTArray<double> res;
        SmTArray<ULONG> mults;
        SmStatus st = a.IntersectImplicitConic(k, res, mults);
        o.push_back((double)st);
        o.push_back((double)res.GetSize());
        for (ULONG i = 0; i < res.GetSize(); ++i)
            o.push_back(res[i]);
        break;
    }
    case OP_INTERSECT_CONIC:
    {
        if (ca[0] == -1.0 || ca[0] == 4.0)
            return SC_ERR_ARG;
        SmConic b = sc_build(cb);
        SmSolutionArray sols;
        SmStatus st = a.IntersectConic(a.GetNaturalInterval(), b, b.GetNaturalInterval(), t, sols);
        o.push_back((double)st);
        o.push_back((double)sols.GetSize());
        for (ULONG i = 0; i < sols.GetSize(); ++i)
        {
            o.push_back(sols[i].m_vStart[0]);
            o.push_back(sols[i].m_vStart[1]);
            o.push_back(sols[i].m_vStart.m_dSolutionValue);
        }
        break;
    }
    case OP_INVERT:
    {
        SmBoolean ok = TRUE;
        double prm = -12345.0;
        SmStatus st = a.InvertPointOnConic(SmPoint3d(w[0], w[1], w[2]), ok, prm);
        o.push_back((double)st);
        o.push_back(flg(ok));
        if (ok)
            o.push_back(prm);
        break;
    }
    case OP_KNOTS:
    {
        // flags bit 0: interval SmExtent1d(w0, w1); bit 1: request multiplicities.
        SmExtent1d ivl(w[0], w[1]);
        SmTArray<double> knots;
        SmTArray<ULONG> mults;
        SmStatus st = a.GetKnots(knots, (flags & 2) ? &mults : NULL, (flags & 1) ? &ivl : NULL);
        o.push_back((double)st);
        o.push_back((double)knots.GetSize());
        for (ULONG i = 0; i < knots.GetSize(); ++i)
            o.push_back(knots[i]);
        o.push_back((double)mults.GetSize());
        for (ULONG i = 0; i < mults.GetSize(); ++i)
            o.push_back((double)mults[i]);
        break;
    }
    case OP_CONTINUITIES:
    {
        SmContinuityType eMin = SM_CT_UNDEFINED;
        SmTArray<SmContinuityType> conts;
        conts.Add(SM_CT_C0); // must be cleared by the call
        SmStatus st = a.CalculateContinuities(eMin, conts, t);
        o.push_back((double)st);
        o.push_back((double)eMin);
        o.push_back((double)conts.GetSize());
        for (ULONG i = 0; i < conts.GetSize(); ++i)
            o.push_back((double)conts[i]);
        break;
    }
    case OP_EQ:
    {
        // flags bit 0: compare A with itself.
        if (flags & 1)
        {
            o.push_back(flg(a == a));
        }
        else
        {
            SmConic b = sc_build(cb);
            o.push_back(flg(a == b));
        }
        break;
    }
    case OP_IS_KIND_OF: o.push_back(flg(a.IsKindOf((SM_TYPE)n))); break;
    case OP_TYPE_STRINGS:
    {
        o.push_back((double)a.GetType());
        for (const TCHAR* s = a.GetTypeString(); *s; ++s)
            o.push_back((double)*s);
        o.push_back(-1.0);
        for (const TCHAR* s = a.GetClassString(); *s; ++s)
            o.push_back((double)*s);
        break;
    }
    case OP_READ_DB:
    {
        if (data_len < 0 || (file_type != 0 && file_type != 1))
            return SC_ERR_ARG;
        std::istringstream iss(std::string((const char*)data, (size_t)data_len));
        SmDatabaseIOStream db(file_type == 0 ? SM_ASCII : SM_BINARY, &iss);
        SmCurve* p = NULL;
        SmStatus st = SmConic::ReadFromDB(SmConic_TYPE, db, 2, *SmApiGetOrCreateContext(), p, 30);
        o.push_back((double)st);
        if (st == SM_SUCCESS && p != NULL)
        {
            SmConic* k = (SmConic*)p;
            int32_t ms = put_state(o, *k);
            // The read sine and cosine, seen through a tangent at t = 0.
            SmVector3d pd[2];
            k->Evaluate(0.0, 1, TRUE, pd, FALSE);
            put_vec(o, pd[0]);
            put_vec(o, pd[1]);
            delete p;
            if (ms != SC_OK)
                return ms;
        }
        else
            delete p;
        break;
    }
    default: return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
