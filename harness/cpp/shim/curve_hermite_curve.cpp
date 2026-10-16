// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.hermite_curve`.
// Wraps upstream SmHermiteCurve (source/SMLib/inc/SmHermiteCurve.h, source/SMLib/src/SmHermiteCurve.cpp).
#include "sc_shim.h"

#include <SmApiGeneral.h>
#include <SmDatabaseIO.h>
#include <SmExtent1d.h>
#include <SmExtent3d.h>
#include <SmHermiteCurve.h>
#include <SmPolarBox.h>
#include <SmPseudoBox.h>
#include <SmVector2d.h>
#include <SmVector3d.h>

#include <cstring>
#include <sstream>
#include <string>
#include <vector>

#include "sc_sm.h"

using sc::put_vec;

// Curve constructions (keep in sync with crates/solidcore-ffi/src/units/curve_hermite_curve.rs).
enum ScHcKind : int32_t
{
    HK_3D = 0, // SmHermiteCurve(SmPoint3d v0, SmVector3d v1, SmPoint3d v2, SmVector3d v3, dim)
    HK_2D = 1, // SmHermiteCurve(SmPoint2d, SmVector2d, SmPoint2d, SmVector2d, dim) from the x, y of v0..v3
    HK_3D_DEFAULT = 2, // 3D constructor, default dimension
    HK_2D_DEFAULT = 3, // 2D constructor, default dimension
    HK_LAST = 3,
};

// Operations (keep in sync with crates/solidcore-ffi/src/units/curve_hermite_curve.rs).
enum ScHcOp : int32_t
{
    OP_STATE = 0,
    OP_EVALUATE = 1,
    OP_EVALUATE_POINT = 2,
    OP_BBOX = 3,
    OP_CHORD = 4,
    OP_KNOTS = 5,
    OP_CONTINUITIES = 6,
    OP_EQ = 7,
    OP_IS_KIND_OF = 8,
    OP_TYPE_STRINGS = 9,
    OP_READ_DB = 10,
    OP_LAST = 10,
};

static SmVector3d sc_vec(const double* v, int i)
{
    return SmVector3d(v[3 * i], v[3 * i + 1], v[3 * i + 2]);
}

static SmHermiteCurve sc_build(int32_t kind, const double* v, int64_t dim)
{
    switch (kind)
    {
    case HK_2D:
        return SmHermiteCurve(SmVector2d(v[0], v[1]), SmVector2d(v[3], v[4]), SmVector2d(v[6], v[7]),
                              SmVector2d(v[9], v[10]), (ULONG)dim);
    case HK_3D_DEFAULT: return SmHermiteCurve(sc_vec(v, 0), sc_vec(v, 1), sc_vec(v, 2), sc_vec(v, 3));
    case HK_2D_DEFAULT:
        return SmHermiteCurve(SmVector2d(v[0], v[1]), SmVector2d(v[3], v[4]), SmVector2d(v[6], v[7]),
                              SmVector2d(v[9], v[10]));
    default: return SmHermiteCurve(sc_vec(v, 0), sc_vec(v, 1), sc_vec(v, 2), sc_vec(v, 3), (ULONG)dim);
    }
}

// The 18 member doubles (P1, D1, P2, D2, C, D) through the upstream binary writer.
static int32_t put_members(std::vector<double>& o, const SmHermiteCurve& h)
{
    std::ostringstream oss;
    SmDatabaseIOStream db(SM_BINARY, &oss);
    SmStatus st = h.WriteToDB(db, 30);
    if (st != SM_SUCCESS)
        return (int32_t)st;
    std::string s = oss.str();
    if (s.size() != 18 * sizeof(double))
        return SC_ERR_ARG;
    for (int i = 0; i < 18; ++i)
    {
        double d;
        std::memcpy(&d, s.data() + i * sizeof(double), sizeof(double));
        o.push_back(d);
    }
    return SC_OK;
}

static double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

// op: one of ScHcOp. Curve A = (kind_a, va[12], dim_a), curve B likewise.
// t: parameter; n: derivative count / type code; flags: per-op bits; w[6]: extra values;
// data/data_len/file_type: database bytes for OP_READ_DB (0 = ASCII, 1 = binary).
SC_EXPORT int32_t sc_curve_hermite_curve_op(int32_t op, int32_t kind_a, const double* va, int64_t dim_a,
                                            int32_t kind_b, const double* vb, int64_t dim_b, double t, int64_t n,
                                            int32_t flags, const double* w, const uint8_t* data, int64_t data_len,
                                            int32_t file_type, double* out, int64_t cap, int64_t* out_len)
{
    if (op < 0 || op > OP_LAST || kind_a < 0 || kind_a > HK_LAST || kind_b < 0 || kind_b > HK_LAST)
        return SC_ERR_ARG;
    if (dim_a < 0 || dim_b < 0)
        return SC_ERR_ARG;
    SC_TRY
    std::vector<double> o;
    SmHermiteCurve a = sc_build(kind_a, va, dim_a);
    switch (op)
    {
    case OP_STATE:
    {
        o.push_back((double)a.GetType());
        o.push_back((double)a.GetDim());
        SmPoint3d b0, b1, b2, b3;
        a.GetBezierPoints(b0, b1, b2, b3);
        put_vec(o, b0);
        put_vec(o, b1);
        put_vec(o, b2);
        put_vec(o, b3);
        SmExtent1d ivl = a.GetNaturalInterval();
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
        int32_t st = put_members(o, a);
        if (st != SC_OK)
            return st;
        break;
    }
    case OP_EVALUATE:
    {
        // flags bit 0: bNonZeroTangents, bit 1: bFromLeft.
        if (n < 0 || n > 16)
            return SC_ERR_ARG;
        std::vector<SmVector3d> pd((size_t)n + 1, SmVector3d(0.0, 0.0, 0.0));
        SmStatus st = a.Evaluate(t, (ULONG)n, (flags & 2) ? TRUE : FALSE, pd.data(), (flags & 1) ? TRUE : FALSE);
        o.push_back((double)st);
        for (const SmVector3d& v : pd)
            put_vec(o, v);
        break;
    }
    case OP_EVALUATE_POINT:
    {
        SmPoint3d p(0.0, 0.0, 0.0);
        SmStatus st = a.EvaluatePoint(t, p);
        o.push_back((double)st);
        put_vec(o, p);
        break;
    }
    case OP_BBOX:
    {
        // flags: 1 normal box, 2 pseudo box, 4 polar box,
        //        8 pseudo box starts as SmPseudoBox() + AddPoint3d(w0..2),
        //        16 polar box starts as SmPolarBox(w3..5).
        if ((flags & 7) == 0)
            return SC_ERR_ARG;
        SmExtent3d nb;
        SmPseudoBox pb;
        if (flags & 8)
            pb.AddPoint3d(sc_vec(w, 0));
        SmPolarBox xb;
        if (flags & 16)
            xb = SmPolarBox(sc_vec(w, 1));
        SmStatus st = a.CalculateBoundingBox(a.GetNaturalInterval(), (flags & 1) ? &nb : NULL,
                                             (flags & 2) ? &pb : NULL, (flags & 4) ? &xb : NULL);
        o.push_back((double)st);
        if (flags & 1)
        {
            o.push_back(nb.GetUMin());
            o.push_back(nb.GetVMin());
            o.push_back(nb.GetWMin());
            o.push_back(nb.GetUMax());
            o.push_back(nb.GetVMax());
            o.push_back(nb.GetWMax());
        }
        if (flags & 2)
        {
            for (ULONG k = 0; k < 3; ++k)
                put_vec(o, pb.GetBasis(k));
            for (ULONG k = 0; k < 3; ++k)
            {
                o.push_back(pb.GetInterval(k).GetMin());
                o.push_back(pb.GetInterval(k).GetMax());
            }
        }
        if (flags & 4)
        {
            SmPolarBoxState e = xb.GetPolarBoxState();
            o.push_back((double)e);
            // The basis and domain of a never-initialised box are uninitialised memory.
            if (e != SM_PS_UNINITIALIZED || (flags & 16))
            {
                for (ULONG k = 0; k < 3; ++k)
                    put_vec(o, *xb.GetBasis(k));
                SmExtent2d d = xb.GetDomain();
                o.push_back(d.GetMin().x);
                o.push_back(d.GetMin().y);
                o.push_back(d.GetMax().x);
                o.push_back(d.GetMax().y);
            }
        }
        break;
    }
    case OP_CHORD: o.push_back(a.ComputeChordHeight()); break;
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
            SmHermiteCurve b = sc_build(kind_b, vb, dim_b);
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
        SmStatus st = SmHermiteCurve::ReadFromDB(SmHermiteCurve_TYPE, db, 3, *SmApiGetOrCreateContext(), p, 30);
        o.push_back((double)st);
        if (p != NULL)
        {
            SmHermiteCurve* h = (SmHermiteCurve*)p;
            o.push_back((double)h->GetType());
            o.push_back((double)h->GetDim());
            int32_t ms = put_members(o, *h);
            delete p;
            if (ms != SC_OK)
                return ms;
        }
        break;
    }
    default: return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
