// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.srf_fit`.
// Wraps the surface fitting routines of upstream source/SMLib/src/NL_SrfFit.cpp owned
// by the unit: N_FitCalcSrfParamValues, N_FitSrfInterpParams, N_FitSrfToPtsKnots,
// N_FitSrfLstSqApprox, N_FitSrfFuncInterp.
//
// One entry point, sc_nlib_srf_fit_eval(op, in, n_in, out, cap, out_len). `in` is a
// flat list of doubles (layouts per op below and in
// crates/solidcore-ffi/src/units/nlib_srf_fit.rs, which validates them so upstream
// only touches memory it is given). Sub-records:
//   grid  = rows cols (x y z w)*rows*cols   (row-major; NL_EPOINT ignores w)
//   reals = count values
// Output: status (0, else NL_ERROR.eno; 1000 if the flag was set without a number),
// then on success: param values `n+1 u, m+1 v`; interp params `count t`;
// surfaces `n m p q r s`, control points (x y z w, row-major), u knots, v knots;
// surface functions `n m p q r s`, values (row-major), u knots, v knots.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

#include <vector>

#include "sc_nlib.h"

using sc::dump_srf;
using sc::nl_status;
using sc::null_srf;

namespace
{
struct Reader
{
    const double* in;
    int64_t at;
    double next() { return in[at++]; }
    long idx() { return (long)in[at++]; }
};

// A grid of points of either kind, with row pointers.
struct Grid
{
    int ptp = NL_EPOINT;
    long rows = 0, cols = 0;
    std::vector<NL_POINT> e;
    std::vector<NL_CPOINT> h;
    std::vector<NL_POINT*> er;
    std::vector<NL_CPOINT*> hr;
    void** data() { return ptp == NL_EPOINT ? (void**)er.data() : (void**)hr.data(); }
};

Grid read_grid(Reader& r, int ptp)
{
    Grid g;
    g.ptp = ptp;
    g.rows = r.idx();
    g.cols = r.idx();
    g.e.resize(g.rows * g.cols);
    g.h.resize(g.rows * g.cols);
    for (long k = 0; k < g.rows * g.cols; k++)
    {
        double x = r.next(), y = r.next(), z = r.next(), w = r.next();
        g.e[k].x = x;
        g.e[k].y = y;
        g.e[k].z = z;
        g.h[k].x = x;
        g.h[k].y = y;
        g.h[k].z = z;
        g.h[k].w = w;
    }
    g.er.resize(g.rows);
    g.hr.resize(g.rows);
    for (long i = 0; i < g.rows; i++)
    {
        g.er[i] = &g.e[i * g.cols];
        g.hr[i] = &g.h[i * g.cols];
    }
    return g;
}

std::vector<double> read_reals(Reader& r)
{
    long c = r.idx();
    std::vector<double> v(c > 0 ? c : 1, 0.0);
    for (long i = 0; i < c; i++)
        v[i] = r.next();
    return v;
}

void dump_sfn(std::vector<double>& o, NL_SFUN* sfn)
{
    NL_INDEX n, m;
    NL_REAL** fuv;
    N_SrfFuncCntrlVal(sfn, &n, &m, &fuv);
    NL_DEGREE p, q;
    N_SFuncGetDegrees(sfn, &p, &q);
    NL_KNOTVECTOR *knu, *knv;
    N_SFuncGetKnotVectors(sfn, &knu, &knv);
    NL_REAL **fv2, *U, *V;
    N_SFuncGetKnots(sfn, &fv2, &U, &V);
    o.push_back((double)n);
    o.push_back((double)m);
    o.push_back((double)p);
    o.push_back((double)q);
    o.push_back((double)knu->m);
    o.push_back((double)knv->m);
    for (NL_INDEX i = 0; i <= n; i++)
        for (NL_INDEX j = 0; j <= m; j++)
            o.push_back(fuv[i][j]);
    for (NL_INDEX i = 0; i <= knu->m; i++)
        o.push_back(U[i]);
    for (NL_INDEX j = 0; j <= knv->m; j++)
        o.push_back(V[j]);
}

NL_SFUN* null_sfn(NL_STACKS* S)
{
    NL_SFUN* sfn = N_AllocSrfFuncData(S);
    if (sfn != NULL)
        N_SFuncInitArrays(sfn);
    return sfn;
}
} // namespace

SC_EXPORT int32_t sc_nlib_srf_fit_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
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
    case 0: // N_FitCalcSrfParamValues: ptp par grid
    {
        int ptp = (int)r.idx();
        int par = (int)r.idx();
        Grid g = read_grid(r, ptp);
        std::vector<double> u(g.rows, 0.0), v(g.cols, 0.0);
        N_ErrClear();
        NL_FLAG err = N_FitCalcSrfParamValues(g.data(), g.rows - 1, g.cols - 1, ptp, par, u.data(), v.data());
        o.push_back(nl_status(err));
        if (err == NL_NO)
        {
            o.insert(o.end(), u.begin(), u.end());
            o.insert(o.end(), v.begin(), v.end());
        }
        break;
    }
    case 1: // N_FitSrfInterpParams: ptp par dir grid
    {
        int ptp = (int)r.idx();
        int par = (int)r.idx();
        int dir = (int)r.idx();
        Grid g = read_grid(r, ptp);
        long c = dir == NL_UDIR ? g.rows : g.cols;
        std::vector<double> t(c, 0.0);
        N_ErrClear();
        NL_FLAG err = N_FitSrfInterpParams(g.data(), g.rows - 1, g.cols - 1, ptp, par, dir, t.data());
        o.push_back(nl_status(err));
        if (err == NL_NO)
        {
            o.push_back((double)c);
            o.insert(o.end(), t.begin(), t.end());
        }
        break;
    }
    case 2: // N_FitSrfToPtsKnots: ptp p q grid reals(u) reals(v) reals(U) reals(V)
    {
        int ptp = (int)r.idx();
        long p = r.idx();
        long q = r.idx();
        Grid g = read_grid(r, ptp);
        std::vector<double> u = read_reals(r);
        std::vector<double> v = read_reals(r);
        std::vector<double> U = read_reals(r);
        std::vector<double> V = read_reals(r);
        NL_KNOTVECTOR knu, knv;
        N_KnotVectorFromRealArray(&knu, U.data(), (NL_INDEX)(U.size() - 1));
        N_KnotVectorFromRealArray(&knv, V.data(), (NL_INDEX)(V.size() - 1));
        NL_SURFACE* sur = null_srf(&S);
        N_ErrClear();
        NL_FLAG err = N_FitSrfToPtsKnots(g.data(), g.rows - 1, g.cols - 1, ptp, u.data(), v.data(), &knu, &knv,
                                         (NL_DEGREE)p, (NL_DEGREE)q, sur, &S);
        o.push_back(nl_status(err));
        if (err == NL_NO)
            dump_srf(o, sur);
        break;
    }
    case 3: // N_FitSrfLstSqApprox: n m p q par grid
    {
        long n = r.idx();
        long m = r.idx();
        long p = r.idx();
        long q = r.idx();
        int par = (int)r.idx();
        Grid g = read_grid(r, NL_EPOINT);
        NL_SURFACE* sur = null_srf(&S);
        N_ErrClear();
        NL_FLAG err = N_FitSrfLstSqApprox(g.er.data(), g.rows - 1, g.cols - 1, n, m, (NL_DEGREE)p, (NL_DEGREE)q, par,
                                          sur, &S);
        o.push_back(nl_status(err));
        if (err == NL_NO)
            dump_srf(o, sur);
        break;
    }
    case 4: // N_FitSrfFuncInterp: p q rows cols values reals(u) reals(v)
    {
        long p = r.idx();
        long q = r.idx();
        long rows = r.idx();
        long cols = r.idx();
        std::vector<double> f(rows * cols);
        for (long k = 0; k < rows * cols; k++)
            f[k] = r.next();
        std::vector<double*> fr(rows);
        for (long i = 0; i < rows; i++)
            fr[i] = &f[i * cols];
        std::vector<double> u = read_reals(r);
        std::vector<double> v = read_reals(r);
        NL_SFUN* sfn = null_sfn(&S);
        N_ErrClear();
        NL_FLAG err = N_FitSrfFuncInterp(fr.data(), rows - 1, cols - 1, (NL_DEGREE)p, (NL_DEGREE)q, u.data(),
                                         v.data(), sfn, &S);
        o.push_back(nl_status(err));
        if (err == NL_NO)
            dump_sfn(o, sfn);
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
