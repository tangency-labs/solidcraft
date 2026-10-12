// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.extent1d`. Wraps upstream SmExtent1d and SmExtentNd
// (source/SMLib/src/SmExtent1d.cpp, source/SMLib/inc/SmExtent1d.h,
//  source/SMLib/src/SmExtentNd.cpp, source/SMLib/inc/SmExtentNd.h).
//
// One entry point, sc_geom_extent1d_op(op, in, n_in, out, cap, out_len): `op` selects
// an operation (numbering shared with crates/solidcore-ffi/src/units/geom_extent1d.rs),
// `in` holds its flat double inputs and the shim writes a flat double result.
// An interval is 2 doubles [min max] stored as given (no swapping; inverted intervals
// are allowed). SmStatus results are written as 0 for SM_SUCCESS, else the SM_ERR_*
// value. Booleans are 0/1, enums their integer value. The Rust wrapper validates
// inputs (sizes, loop bounds of PeriodicWrap); the shim checks sizes again.
#include "sc_shim.h"

#include <SmExtent1d.h>
#include <SmExtentNd.h>
#include <SmTArray.h>

#include <vector>

namespace
{
// SmExtent1d with directly assigned (possibly inverted) ends.
struct RawExt : public SmExtent1d
{
    RawExt(double a, double b)
    {
        m_dMin = a;
        m_dMax = b;
    }
};

SmExtent1d ext(const double* d) { return RawExt(d[0], d[1]); }

void put_ext(std::vector<double>& o, const SmExtent1d& e)
{
    o.push_back(e.GetMin());
    o.push_back(e.GetMax());
}

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double b(SmBoolean v) { return v ? 1.0 : 0.0; }

// Number of input doubles per op (-1: variable, checked in the op).
int64_t input_size(int32_t op)
{
    switch (op)
    {
    case 0: return 2;  // constructors: a, b
    case 1: return 4;  // queries: E, t, tol
    case 2: return 5;  // modify: E, which, x, y
    case 3: return 5;  // binary: E, F, tol
    case 4: return 4;  // approximate_unbounded: E, center, half
    case 5: return 5;  // periodic value: E, v, period, tol
    case 6: return 6;  // intersect_periodic: E, F, period, dont_cross
    case 7: return 6;  // are_disjoint_periodic: E, F, period, tol
    case 8: return -1; // clamp_vector: header + data
    default: return -2;
    }
}
} // namespace

SC_EXPORT int32_t sc_geom_extent1d_op(int32_t op, const double* in, int64_t n_in, double* out,
                                      int64_t cap, int64_t* out_len)
{
    int64_t need = input_size(op);
    if (need == -2 || n_in < 0 || (need >= 0 && n_in != need))
        return SC_ERR_ARG;
    SC_TRY
    std::vector<double> o;
    switch (op)
    {
    case 0:
    {
        put_ext(o, SmExtent1d(in[0], in[1]));
        put_ext(o, SmExtent1d(in[0]));
        SmExtent1d e;
        put_ext(o, e);
        break;
    }
    case 1:
    {
        const SmExtent1d e = ext(in);
        double t = in[2], tol = in[3];
        o.push_back(e.GetMin());
        o.push_back(e.GetMax());
        o.push_back(e.GetMid());
        o.push_back(e.GetLength());
        o.push_back(e.GetMaxDimension());
        o.push_back(e.ClampValue(t));
        o.push_back(e.SnapValue(t, tol));
        o.push_back(e.Evaluate(t));
        o.push_back(e.DistanceFrom(t));
        double dOther = 0.0;
        SmBoolean bOn = e.IsValueOnBoundary(t, tol, &dOther);
        o.push_back(dOther);
        double dParam = 0.0;
        SmStatus sInv = e.Inversion(t, dParam);
        o.push_back(dParam);
        // integers
        o.push_back(b(e.GetTLeftEval(t)));
        o.push_back(b(e.AssertDefined()));
        o.push_back(b(e.IsInit()));
        o.push_back(b(e.HasNegativeLength()));
        o.push_back(b(e.IsDegenerate(tol)));
        SmBoundaryType eType = SM_BT_UNKNOWN;
        o.push_back(b(e.IsBounded(&eType)));
        o.push_back((double)eType);
        o.push_back(b(e.IsBounded()));
        o.push_back(b(e.ContainsValue(t, tol)));
        o.push_back((double)e.ClassifyPoint(t, tol));
        o.push_back(b(bOn));
        o.push_back(b(e.IsValueOnBoundary(t, tol)));
        o.push_back(st(sInv));
        break;
    }
    case 2:
    {
        SmExtent1d e = ext(in);
        int which = (int)in[2];
        double x = in[3], y = in[4];
        SmStatus s = SM_SUCCESS;
        switch (which)
        {
        case 0: e.AddValue(x); break;
        case 1: s = e.SetMinMax(x, y); break;
        case 2: s = e.SetMin(x); break;
        case 3: s = e.SetMax(x); break;
        case 4: e.SetUnbounded(); break;
        case 5: e.Negate(); break;
        case 6: e.Translate(x); break;
        case 7: e.ExpandAbsolute(x); break;
        case 8: e.ExpandRelative(x); break;
        case 9: e.Scale(x); break;
        case 10: e.Init(); break;
        default: return SC_ERR_ARG;
        }
        o.push_back(st(s));
        put_ext(o, e);
        break;
    }
    case 3:
    {
        const SmExtent1d e = ext(in), f = ext(in + 2);
        double tol = in[4];
        SmExtent1d u;
        e.Union(f, u);
        put_ext(o, u);
        SmExtent1d x(7.0, 8.0); // overwritten either way
        SmStatus s = e.Intersect(f, x);
        put_ext(o, x);
        o.push_back(st(s));
        o.push_back(b(e.AreDisjoint(f, tol)));
        o.push_back(b(e.AreDisjoint(f)));
        o.push_back(b(e.AreEqual(f, tol)));
        o.push_back(b(e.AreEqual(f.GetMin(), f.GetMax(), tol)));
        o.push_back(b(e.IsContainedBy(f, tol)));
        o.push_back(b(e == f));
        break;
    }
    case 4:
    {
        const SmExtent1d e = ext(in);
        SmExtent1d x; // Init state; untouched when SetMinMax rejects
        SmExtent1d a = e.ApproximateUnbounded(in[2], in[3], &x);
        SmExtent1d a2 = e.ApproximateUnbounded(in[2], in[3]);
        put_ext(o, a);
        put_ext(o, x);
        put_ext(o, a2);
        break;
    }
    case 5:
    {
        const SmExtent1d e = ext(in);
        double v = in[2], period = in[3], tol = in[4];
        double dPer = 0.0, dMin = 0.0, dMax = 0.0, dDist = 0.0;
        SmBoolean bIn = e.ContainsPeriodicValue(v, period, &dPer, &dMin, &dMax, tol);
        SmBoolean bOn = e.IsValueOnPeriodicBoundary(v, period, tol, &dDist);
        o.push_back(e.PeriodicWrap(v));
        o.push_back(dPer);
        o.push_back(dMin);
        o.push_back(dMax);
        o.push_back(e.ClampPeriodicValue(v, period, tol));
        o.push_back(dDist);
        o.push_back(b(bIn));
        o.push_back(b(e.ContainsPeriodicValue(v, period, NULL, NULL, NULL, tol)));
        o.push_back(b(bOn));
        o.push_back(b(e.IsClosed(period)));
        break;
    }
    case 6:
    {
        const SmExtent1d e = ext(in), f = ext(in + 2);
        SmTArray<SmExtent1d> res;
        SmStatus s = e.IntersectPeriodic(f, in[4], res, in[5] != 0.0 ? TRUE : FALSE);
        o.push_back(st(s));
        o.push_back((double)res.GetSize());
        for (ULONG i = 0; i < res.GetSize(); i++)
            put_ext(o, res[i]);
        break;
    }
    case 7:
    {
        const SmExtent1d e = ext(in), f = ext(in + 2);
        o.push_back(b(e.AreDisjointPeriodic(f, in[4], in[5])));
        break;
    }
    case 8:
    {
        // header: dim, has_periodic, has_counts, n_prior_clamped, n_prior_counts
        // then: dim*2 extents, dim values, dim periodic flags (if has_periodic),
        //       prior clamped, prior counts (if has_counts)
        if (n_in < 5)
            return SC_ERR_ARG;
        int64_t dim = (int64_t)in[0];
        bool has_per = in[1] != 0.0, has_cnt = in[2] != 0.0;
        int64_t n_pc = (int64_t)in[3], n_pn = (int64_t)in[4];
        if (dim < 0 || n_pc < 0 || n_pn < 0 || (!has_cnt && n_pn != 0))
            return SC_ERR_ARG;
        int64_t total = 5 + 3 * dim + (has_per ? dim : 0) + n_pc + n_pn;
        if (n_in != total)
            return SC_ERR_ARG;
        const double* p = in + 5;
        SmExtentNd nd((ULONG)dim);
        for (int64_t i = 0; i < dim; i++)
            nd[(ULONG)i] = ext(p + 2 * i);
        p += 2 * dim;
        SmTArray<double> vals;
        for (int64_t i = 0; i < dim; i++)
            vals.Add(p[i]);
        p += dim;
        SmTArray<SmBoolean> per;
        if (has_per)
        {
            for (int64_t i = 0; i < dim; i++)
                per.Add(p[i] != 0.0 ? TRUE : FALSE);
            p += dim;
        }
        SmTArray<double> clamped;
        for (int64_t i = 0; i < n_pc; i++)
            clamped.Add(p[i]);
        p += n_pc;
        SmTArray<ULONG> counts;
        for (int64_t i = 0; i < n_pn; i++)
            counts.Add((ULONG)p[i]);
        SmBoolean r = nd.ClampVector(vals, has_per ? &per : NULL, clamped,
                                     has_cnt ? &counts : NULL);
        o.push_back(b(r));
        for (ULONG i = 0; i < clamped.GetSize(); i++)
            o.push_back(clamped[i]);
        if (has_cnt)
            for (ULONG i = 0; i < counts.GetSize(); i++)
                o.push_back((double)counts[i]);
        break;
    }
    default: return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
