// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.crv_fit_p2`.
// Wraps the curve fitting routines of upstream source/SMLib/src/NL_CrvFit.cpp owned by
// the unit: N_FitFuncInterpGivenParams, N_FitCrvInterp, N_FitCrvApproxLstSq,
// N_FitCrvMatrix, N_FitCrvDerivMatrix, N_FitCrvWeightedLstSqKnots,
// N_FitCalcKnotsDerivs, N_FitCalcKnotsHighEndDerivs, N_FitCalcMatrix.
//
// One entry point, sc_nlib_crv_fit_p2_eval(op, in, n_in, out, cap, out_len). `in` is
// a flat list of doubles (layouts per op below and in
// crates/solidcore-ffi/src/units/nlib_crv_fit_p2.rs, which validates them so upstream
// only touches memory it is given). Sub-records:
//   points  = ptp count (x y z w)*count   (NL_EPOINT ignores w)
//   reals   = count values
//   vector  = x y z w                      (NL_VECTOR / NL_CVECTOR by ptp)
//   matrix  = n mtp bw count storage...    (N_SetRealMatrix(n, n, mtp, bw), storage
//                                           row-major, then N_RealMatrixLuDecompose)
// Output: status (0, else NL_ERROR.eno; 1000 if the flag was set without a number),
// then on success: knot ops `count values`; curve ops `n p m`, the control points
// (x y z w) and knots; the function op `n p m`, the values and knots; control point
// ops `count (x y z w)*count`; the matrix op `n bw count storage` (band columns). Ops with a given
// matrix first write its LU status (stopping there when it is nonzero).
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

#include <vector>

#include "sc_nlib.h"

using sc::dump_crv;
using sc::nl_status;
using sc::null_crv;

namespace
{

struct Reader
{
    const double* in;
    int64_t at;
    double next() { return in[at++]; }
    long idx() { return (long)in[at++]; }
};

// Point data of either kind.
struct Pts
{
    int ptp = NL_EPOINT;
    long count = 0;
    std::vector<NL_POINT> e;
    std::vector<NL_CPOINT> h;
    void* data() { return ptp == NL_EPOINT ? (void*)e.data() : (void*)h.data(); }
};

void read_pt(Reader& r, int ptp, NL_POINT* e, NL_CPOINT* h)
{
    double x = r.next(), y = r.next(), z = r.next(), w = r.next();
    if (ptp == NL_EPOINT)
    {
        e->x = x;
        e->y = y;
        e->z = z;
    }
    else
    {
        h->x = x;
        h->y = y;
        h->z = z;
        h->w = w;
    }
}

Pts read_pts(Reader& r)
{
    Pts p;
    p.ptp = (int)r.idx();
    long c = r.idx();
    p.count = c;
    p.e.resize(c > 0 ? c : 1);
    p.h.resize(c > 0 ? c : 1);
    for (long i = 0; i < c; i++)
        read_pt(r, p.ptp, &p.e[i], &p.h[i]);
    return p;
}

// One NL_VECTOR / NL_CVECTOR (by ptp) behind a void pointer.
struct Vec
{
    NL_POINT e;
    NL_CPOINT h;
    int ptp;
    void* data() { return ptp == NL_EPOINT ? (void*)&e : (void*)&h; }
};

Vec read_vec(Reader& r, int ptp)
{
    Vec v{};
    v.ptp = ptp;
    read_pt(r, ptp, &v.e, &v.h);
    return v;
}

std::vector<double> read_reals(Reader& r)
{
    long c = r.idx();
    std::vector<double> v(c > 0 ? c : 1, 0.0);
    for (long i = 0; i < c; i++)
        v[i] = r.next();
    return v;
}

long storage_cols(int mtp, long n, long bw)
{
    return mtp == NL_MT_BANDED ? (bw == 1 ? 2 : bw) : n + 1;
}

// Builds and LU-decomposes the matrix record; returns the LU status.
double read_matrix(Reader& r, NL_RMATRIX* cm, NL_STACKS* S)
{
    long n = r.idx();
    int mtp = (int)r.idx();
    long bw = r.idx();
    long c = r.idx();
    N_InitRealMatrix(cm);
    N_ErrClear();
    NL_FLAG err = N_SetRealMatrix(cm, n, n, (NL_MATRIXTYPE)mtp, bw, S);
    if (err != NL_NO)
    {
        r.at += c;
        return nl_status(err);
    }
    NL_REAL** RM;
    N_GetRealMatrixPtr(cm, &RM);
    long cols = storage_cols(mtp, n, bw);
    for (long k = 0; k < c; k++)
        RM[k / cols][k % cols] = r.next();
    N_ErrClear();
    return nl_status(N_RealMatrixLuDecompose(cm));
}

void dump_knots(std::vector<double>& o, NL_KNOTVECTOR* knt)
{
    NL_INDEX m;
    NL_REAL* U;
    N_KnotVectorGetKnots(knt, &m, &U);
    o.push_back((double)(m + 1));
    for (NL_INDEX i = 0; i <= m; i++)
        o.push_back(U[i]);
}

void dump_cfun(std::vector<double>& o, NL_CFUN* cfn)
{
    NL_INDEX n, m;
    NL_DEGREE p;
    NL_REAL *fu, *U;
    N_CFuncGetData(cfn, &n, &fu, &p, &m, &U);
    o.push_back((double)n);
    o.push_back((double)p);
    o.push_back((double)m);
    for (NL_INDEX i = 0; i <= n; i++)
        o.push_back(fu[i]);
    for (NL_INDEX i = 0; i <= m; i++)
        o.push_back(U[i]);
}

void dump_cpts(std::vector<double>& o, const NL_CPOINT* Pw, long count)
{
    o.push_back((double)count);
    for (long i = 0; i < count; i++)
    {
        o.push_back(Pw[i].x);
        o.push_back(Pw[i].y);
        o.push_back(Pw[i].z);
        o.push_back(Pw[i].w);
    }
}

// Runs a knot vector routine on a zero-filled buffer of `len` knots.
template <typename F> void knot_op(std::vector<double>& o, long len, F f)
{
    std::vector<double> buf(len, 0.0);
    NL_KNOTVECTOR knt;
    N_KnotVectorFromRealArray(&knt, buf.data(), (NL_INDEX)(len - 1));
    N_ErrClear();
    f(&knt);
    o.push_back(0.0);
    dump_knots(o, &knt);
}

void curve_result(std::vector<double>& o, NL_FLAG err, NL_CURVE* cur)
{
    o.push_back(nl_status(err));
    if (err == NL_NO)
        dump_crv(o, cur);
}

// Euclidean points (x y z each) as an NL_POINT array.
std::vector<NL_POINT> read_epts(Reader& r, long c)
{
    std::vector<NL_POINT> P(c > 0 ? c : 1);
    for (long i = 0; i < c; i++)
    {
        P[i].x = r.next();
        P[i].y = r.next();
        P[i].z = r.next();
    }
    return P;
}
} // namespace

SC_EXPORT int32_t sc_nlib_crv_fit_p2_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                          int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    Reader r{in, 0};
    int32_t rc = SC_OK;
    switch (op)
    {
    case 0: // N_FitFuncInterpGivenParams: p reals(f) reals(u)
    {
        long p = r.idx();
        std::vector<double> f = read_reals(r);
        std::vector<double> u = read_reals(r);
        long n = (long)f.size() - 1;
        NL_CFUN* cfn = N_AllocCrvFuncStack(&S);
        if (cfn != NULL)
            N_CFuncInitArrays(cfn);
        N_ErrClear();
        NL_FLAG err = N_FitFuncInterpGivenParams(f.data(), n, (NL_DEGREE)p, u.data(), cfn, &S);
        o.push_back(nl_status(err));
        if (err == NL_NO)
            dump_cfun(o, cfn);
        break;
    }
    case 1: // N_FitCrvInterp: p par count (x y z)*count
    {
        long p = r.idx();
        int par = (int)r.idx();
        long c = r.idx();
        std::vector<NL_POINT> P = read_epts(r, c);
        NL_CURVE* cur = null_crv(&S);
        N_ErrClear();
        NL_FLAG err = N_FitCrvInterp(P.data(), c - 1, (NL_DEGREE)p, par, cur, &S);
        curve_result(o, err, cur);
        break;
    }
    case 2: // N_FitCrvApproxLstSq: n p par count (x y z)*count
    {
        long n = r.idx();
        long p = r.idx();
        int par = (int)r.idx();
        long c = r.idx();
        std::vector<NL_POINT> P = read_epts(r, c);
        NL_CURVE* cur = null_crv(&S);
        N_ErrClear();
        NL_FLAG err = N_FitCrvApproxLstSq(P.data(), c - 1, n, (NL_DEGREE)p, par, cur, &S);
        curve_result(o, err, cur);
        break;
    }
    case 3: // N_FitCrvMatrix: p points matrix
    {
        long p = r.idx();
        Pts a = read_pts(r);
        NL_RMATRIX cm;
        double lu = read_matrix(r, &cm, &S);
        o.push_back(lu);
        if (lu != 0.0)
            break;
        NL_INDEX mn, mm;
        N_GetMaxIndexRealMatrix(&cm, &mn, &mm);
        long cnt = p == 1 ? a.count : mn + 1;
        std::vector<NL_CPOINT> Pw(cnt > 0 ? cnt : 1);
        N_ErrClear();
        NL_FLAG err = N_FitCrvMatrix(a.data(), a.count - 1, a.ptp, (NL_DEGREE)p, &cm, Pw.data());
        o.push_back(nl_status(err));
        if (err == NL_NO)
            dump_cpts(o, Pw.data(), cnt);
        break;
    }
    case 4: // N_FitCrvDerivMatrix: p whr points reals(knots) vec matrix
    {
        long p = r.idx();
        int whr = (int)r.idx();
        Pts a = read_pts(r);
        std::vector<double> U = read_reals(r);
        Vec t = read_vec(r, a.ptp);
        NL_KNOTVECTOR knt;
        N_KnotVectorFromRealArray(&knt, U.data(), (NL_INDEX)(U.size() - 1));
        NL_RMATRIX cm;
        double lu = read_matrix(r, &cm, &S);
        o.push_back(lu);
        if (lu != 0.0)
            break;
        NL_INDEX mn, mm;
        N_GetMaxIndexRealMatrix(&cm, &mn, &mm);
        std::vector<NL_CPOINT> Pw(mn + 1);
        N_ErrClear();
        NL_FLAG err =
            N_FitCrvDerivMatrix(a.data(), a.count - 1, a.ptp, (NL_DEGREE)p, &knt, t.data(), whr, &cm, Pw.data());
        o.push_back(nl_status(err));
        if (err == NL_NO)
            dump_cpts(o, Pw.data(), mn + 1);
        break;
    }
    case 5: // N_FitCrvWeightedLstSqKnots: n p points reals(wp) points(derivs) reals(wd)
            // reals(I) reals(u) reals(knots)
    {
        long n = r.idx();
        long p = r.idx();
        Pts a = read_pts(r);
        std::vector<double> wp = read_reals(r);
        Pts d = read_pts(r);
        long s = (long)r.in[r.at] - 1;
        std::vector<double> wd = read_reals(r);
        std::vector<double> Ir = read_reals(r);
        std::vector<NL_INDEX> I(Ir.size());
        for (size_t i = 0; i < Ir.size(); i++)
            I[i] = (NL_INDEX)Ir[i];
        std::vector<double> u = read_reals(r);
        std::vector<double> U = read_reals(r);
        NL_KNOTVECTOR knt;
        N_KnotVectorFromRealArray(&knt, U.data(), (NL_INDEX)(U.size() - 1));
        NL_CURVE* cur = null_crv(&S);
        N_ErrClear();
        NL_FLAG err = N_FitCrvWeightedLstSqKnots(a.data(), wp.data(), a.count - 1, d.data(), wd.data(), I.data(), s,
                                                 a.ptp, u.data(), &knt, n, (NL_DEGREE)p, cur, &S);
        curve_result(o, err, cur);
        break;
    }
    case 6: // N_FitCalcKnotsDerivs: p reals(u)
    {
        long p = r.idx();
        std::vector<double> u = read_reals(r);
        long k = (long)u.size() - 1;
        knot_op(o, 2 * k + p + 3, [&](NL_KNOTVECTOR* knt) { N_FitCalcKnotsDerivs(u.data(), k, (NL_DEGREE)p, knt); });
        break;
    }
    case 7: // N_FitCalcKnotsHighEndDerivs: p k l reals(u)
    {
        long p = r.idx();
        long k = r.idx();
        long l = r.idx();
        std::vector<double> u = read_reals(r);
        long n = (long)u.size() - 1;
        knot_op(o, n + k + l + p + 2,
                [&](NL_KNOTVECTOR* knt) { N_FitCalcKnotsHighEndDerivs(u.data(), n, (NL_DEGREE)p, k, l, knt); });
        break;
    }
    case 8: // N_FitCalcMatrix: p itp reals(t) reals(knots)
    {
        long p = r.idx();
        int itp = (int)r.idx();
        std::vector<double> t = read_reals(r);
        std::vector<double> U = read_reals(r);
        NL_KNOTVECTOR knt;
        N_KnotVectorFromRealArray(&knt, U.data(), (NL_INDEX)(U.size() - 1));
        NL_RMATRIX cm;
        N_InitRealMatrix(&cm);
        N_ErrClear();
        NL_FLAG err = N_FitCalcMatrix(&knt, (NL_DEGREE)p, t.data(), (NL_INDEX)(t.size() - 1), itp, &cm, &S);
        o.push_back(nl_status(err));
        if (err == NL_NO)
        {
            NL_INDEX mn, mm;
            N_GetMaxIndexRealMatrix(&cm, &mn, &mm);
            NL_INDEX bw;
            NL_MATRIXTYPE mtp;
            NL_REAL** RM;
            N_GetRealMatrixData(&cm, &mn, &mm, &RM, &mtp, &bw);
            // With bandwidth 1 the second storage column is outside the band and
            // never initialised by upstream, so only the band column is reported.
            long cols = bw == 1 ? 1 : storage_cols((int)mtp, mn, bw);
            o.push_back((double)mn);
            o.push_back((double)bw);
            o.push_back((double)((mn + 1) * cols));
            for (long i = 0; i <= mn; i++)
                for (long j = 0; j < cols; j++)
                    o.push_back(RM[i][j]);
        }
        break;
    }
    default:
        rc = SC_ERR_ARG;
    }
    N_EndNurbs(&S);
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
