// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.crv_fit_p1`.
// Wraps the curve fitting routines of upstream source/SMLib/src/NL_CrvFit.cpp owned by
// the unit: N_FitCalcCrvParamValues, N_FitCrvCalcKnotVector,
// N_FitCalcKnotVectorEndDerivs, N_FitCalcKnotVectorDeriv,
// N_FitCalcKnotVectorCrvApprox, N_FitCrvInterpGivenParams, N_FitCrvKnotsAndDerivs,
// N_FitCrvKnotsAndDeriv, N_FitCrvCPtsFromSrfData, N_FitCrvApproxKnots,
// N_FitCrvDerivsMatrix, N_FitCrvFirstDerivAndKnots.
//
// One entry point, sc_nlib_crv_fit_p1_eval(op, in, n_in, out, cap, out_len). `in` is
// a flat list of doubles (layouts per op below and in
// crates/solidcore-ffi/src/units/nlib_crv_fit_p1.rs, which validates them so upstream
// only touches memory it is given). Sub-records:
//   points  = ptp count (x y z w)*count   (NL_EPOINT ignores w)
//   reals   = count values
//   vector  = x y z w                      (NL_VECTOR / NL_CVECTOR by ptp)
//   matrix  = n mtp bw count storage...    (N_SetRealMatrix(n, n, mtp, bw), storage
//                                           row-major, then N_RealMatrixLuDecompose)
// Output: status (0, else NL_ERROR.eno; 1000 if the flag was set without a number),
// then on success: knot / parameter ops `count values`; curve ops `n p m`, the
// control points (x y z w) and knots; control point ops `count (x y z w)*count`.
// Ops with a matrix first write the LU status (stopping there when it is nonzero).
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
    long cols = mtp == NL_MT_BANDED ? (bw == 1 ? 2 : bw) : n + 1;
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
    NL_FLAG err = f(&knt);
    o.push_back(nl_status(err));
    if (err == NL_NO)
        dump_knots(o, &knt);
}

void curve_result(std::vector<double>& o, NL_FLAG err, NL_CURVE* cur)
{
    o.push_back(nl_status(err));
    if (err == NL_NO)
        dump_crv(o, cur);
}
} // namespace

SC_EXPORT int32_t sc_nlib_crv_fit_p1_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
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
    case 0: // param values: ptp par points(count only, data follows)
    {
        int ptp = (int)r.idx();
        int par = (int)r.idx();
        long c = r.idx();
        Pts p;
        p.ptp = ptp;
        p.e.resize(c);
        p.h.resize(c);
        for (long i = 0; i < c; i++)
            read_pt(r, ptp, &p.e[i], &p.h[i]);
        std::vector<double> u(c, 0.0);
        N_ErrClear();
        NL_FLAG err = N_FitCalcCrvParamValues(p.data(), (NL_INDEX)(c - 1), ptp, par, u.data());
        o.push_back(nl_status(err));
        if (err == NL_NO)
        {
            o.push_back((double)c);
            o.insert(o.end(), u.begin(), u.end());
        }
        break;
    }
    case 1: // N_FitCrvCalcKnotVector: p reals
    case 2: // N_FitCalcKnotVectorEndDerivs: p reals
    {
        long p = r.idx();
        long c = r.idx();
        std::vector<double> u(r.in + r.at, r.in + r.at + c);
        long k = c - 1;
        if (op == 1)
            knot_op(o, k + p + 2,
                    [&](NL_KNOTVECTOR* knt)
                    {
                        N_FitCrvCalcKnotVector(u.data(), k, (NL_DEGREE)p, knt);
                        return (NL_FLAG)NL_NO;
                    });
        else
            knot_op(o, k + p + 4,
                    [&](NL_KNOTVECTOR* knt)
                    {
                        N_FitCalcKnotVectorEndDerivs(u.data(), k, (NL_DEGREE)p, knt);
                        return (NL_FLAG)NL_NO;
                    });
        break;
    }
    case 3: // N_FitCalcKnotVectorDeriv: p whr reals
    {
        long p = r.idx();
        int whr = (int)r.idx();
        long c = r.idx();
        std::vector<double> u(r.in + r.at, r.in + r.at + c);
        long k = c - 1;
        knot_op(o, k + p + 3,
                [&](NL_KNOTVECTOR* knt) { return N_FitCalcKnotVectorDeriv(u.data(), k, (NL_DEGREE)p, whr, knt); });
        break;
    }
    case 4: // N_FitCalcKnotVectorCrvApprox: n p reals
    {
        long n = r.idx();
        long p = r.idx();
        long c = r.idx();
        std::vector<double> u(r.in + r.at, r.in + r.at + c);
        knot_op(o, n + p + 2,
                [&](NL_KNOTVECTOR* knt)
                { return N_FitCalcKnotVectorCrvApprox(u.data(), c - 1, n, (NL_DEGREE)p, knt); });
        break;
    }
    case 5: // N_FitCrvInterpGivenParams: p points reals(u) reals(knots)
    {
        long p = r.idx();
        Pts a = read_pts(r);
        long na = (long)a.e.size();
        std::vector<double> u = read_reals(r);
        std::vector<double> U = read_reals(r);
        NL_KNOTVECTOR knt;
        N_KnotVectorFromRealArray(&knt, U.data(), (NL_INDEX)(U.size() - 1));
        NL_CURVE* cur = null_crv(&S);
        N_ErrClear();
        NL_FLAG err = N_FitCrvInterpGivenParams(a.data(), na - 1, a.ptp, u.data(), &knt, (NL_DEGREE)p, cur, &S);
        curve_result(o, err, cur);
        break;
    }
    case 6: // N_FitCrvKnotsAndDerivs: p points reals(u) reals(knots) vec vec
    {
        long p = r.idx();
        Pts a = read_pts(r);
        long na = (long)a.e.size();
        std::vector<double> u = read_reals(r);
        std::vector<double> U = read_reals(r);
        Vec ds = read_vec(r, a.ptp);
        Vec de = read_vec(r, a.ptp);
        NL_KNOTVECTOR knt;
        N_KnotVectorFromRealArray(&knt, U.data(), (NL_INDEX)(U.size() - 1));
        NL_CURVE* cur = null_crv(&S);
        N_ErrClear();
        NL_FLAG err = N_FitCrvKnotsAndDerivs(a.data(), na - 1, a.ptp, u.data(), &knt, (NL_DEGREE)p, ds.data(),
                                             de.data(), cur, &S);
        curve_result(o, err, cur);
        break;
    }
    case 7: // N_FitCrvKnotsAndDeriv: p whr points reals(u) reals(knots) vec
    {
        long p = r.idx();
        int whr = (int)r.idx();
        Pts a = read_pts(r);
        long na = (long)a.e.size();
        std::vector<double> u = read_reals(r);
        std::vector<double> U = read_reals(r);
        Vec t = read_vec(r, a.ptp);
        NL_KNOTVECTOR knt;
        N_KnotVectorFromRealArray(&knt, U.data(), (NL_INDEX)(U.size() - 1));
        NL_CURVE* cur = null_crv(&S);
        N_ErrClear();
        NL_FLAG err = N_FitCrvKnotsAndDeriv(a.data(), na - 1, a.ptp, u.data(), &knt, (NL_DEGREE)p, t.data(), whr,
                                            cur, &S);
        curve_result(o, err, cur);
        break;
    }
    case 8: // N_FitCrvCPtsFromSrfData: ptp dir k rows cols grid matrix
    {
        int ptp = (int)r.idx();
        int dir = (int)r.idx();
        long k = r.idx();
        long rows = r.idx();
        long cols = r.idx();
        std::vector<NL_POINT> e(rows * cols);
        std::vector<NL_CPOINT> h(rows * cols);
        for (long i = 0; i < rows * cols; i++)
            read_pt(r, ptp, &e[i], &h[i]);
        std::vector<NL_POINT*> er(rows);
        std::vector<NL_CPOINT*> hr(rows);
        for (long i = 0; i < rows; i++)
        {
            er[i] = &e[i * cols];
            hr[i] = &h[i * cols];
        }
        NL_RMATRIX cm;
        double lu = read_matrix(r, &cm, &S);
        o.push_back(lu);
        if (lu != 0.0)
            break;
        NL_INDEX mn, mm;
        N_GetMaxIndexRealMatrix(&cm, &mn, &mm);
        std::vector<NL_CPOINT> Qw(mn + 1);
        void** A = ptp == NL_EPOINT ? (void**)er.data() : (void**)hr.data();
        N_ErrClear();
        NL_FLAG err = N_FitCrvCPtsFromSrfData(A, rows - 1, cols - 1, ptp, k, dir, &cm, Qw.data());
        o.push_back(nl_status(err));
        if (err == NL_NO)
            dump_cpts(o, Qw.data(), mn + 1);
        break;
    }
    case 9: // N_FitCrvApproxKnots: n p points reals(u) reals(knots)
    {
        long n = r.idx();
        long p = r.idx();
        Pts a = read_pts(r);
        long na = (long)a.e.size();
        std::vector<double> u = read_reals(r);
        std::vector<double> U = read_reals(r);
        NL_KNOTVECTOR knt;
        N_KnotVectorFromRealArray(&knt, U.data(), (NL_INDEX)(U.size() - 1));
        NL_CURVE* cur = null_crv(&S);
        N_ErrClear();
        NL_FLAG err = N_FitCrvApproxKnots(a.data(), na - 1, a.ptp, u.data(), &knt, n, (NL_DEGREE)p, cur, &S);
        curve_result(o, err, cur);
        break;
    }
    case 10: // N_FitCrvDerivsMatrix: p points reals(knots) vec vec matrix
    {
        long p = r.idx();
        Pts a = read_pts(r);
        long na = (long)a.e.size();
        std::vector<double> U = read_reals(r);
        Vec ds = read_vec(r, a.ptp);
        Vec de = read_vec(r, a.ptp);
        NL_KNOTVECTOR knt;
        N_KnotVectorFromRealArray(&knt, U.data(), (NL_INDEX)(U.size() - 1));
        NL_RMATRIX cm;
        double lu = read_matrix(r, &cm, &S);
        o.push_back(lu);
        if (lu != 0.0)
            break;
        NL_INDEX mn, mm;
        N_GetMaxIndexRealMatrix(&cm, &mn, &mm);
        long cnt = mn + 1 > na + 2 ? mn + 1 : na + 2;
        std::vector<NL_CPOINT> Pw(cnt);
        N_ErrClear();
        NL_FLAG err = N_FitCrvDerivsMatrix(a.data(), na - 1, a.ptp, (NL_DEGREE)p, &knt, ds.data(), de.data(), &cm,
                                           Pw.data());
        o.push_back(nl_status(err));
        if (err == NL_NO)
            dump_cpts(o, Pw.data(), mn + 1);
        break;
    }
    case 11: // N_FitCrvFirstDerivAndKnots: p points points(derivs) reals(u) reals(knots)
    {
        long p = r.idx();
        Pts a = read_pts(r);
        long na = (long)a.e.size();
        Pts d = read_pts(r);
        std::vector<double> u = read_reals(r);
        std::vector<double> U = read_reals(r);
        NL_KNOTVECTOR knt;
        N_KnotVectorFromRealArray(&knt, U.data(), (NL_INDEX)(U.size() - 1));
        NL_CURVE* cur = null_crv(&S);
        N_ErrClear();
        NL_FLAG err =
            N_FitCrvFirstDerivAndKnots(a.data(), d.data(), na - 1, a.ptp, u.data(), &knt, (NL_DEGREE)p, cur, &S);
        curve_result(o, err, cur);
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
