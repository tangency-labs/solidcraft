// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.crv_fit_p3`.
// Wraps the curve fitting routines of upstream source/SMLib/src/NL_CrvFit.cpp owned by
// the unit: N_FitCrvDerivs, N_FitCubicSplineInterp, N_FitCrvFirstDeriv,
// N_FitCrvKnotsAndTangents, N_FitCrvApproxClosedConditions, N_FitCrvHighDerivs.
//
// One entry point, sc_nlib_crv_fit_p3_eval(op, in, n_in, out, cap, out_len). `in` is
// a flat list of doubles (layouts per op below and in
// crates/solidcore-ffi/src/units/nlib_crv_fit_p3.rs, which validates them so upstream
// only touches memory it is given). Sub-records:
//   points = count (x y z)*count
//   vector = x y z
//   reals  = count values
// Output: status (0, else NL_ERROR.eno; 1000 if the flag was set without a number),
// then per op (see each case); a curve is `n p m`, the control points (x y z w) and
// the knots.
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

NL_POINT read_vec(Reader& r)
{
    NL_POINT v;
    v.x = r.next();
    v.y = r.next();
    v.z = r.next();
    return v;
}

// `c` vectors (x y z each), at least one slot.
std::vector<NL_POINT> read_vecs(Reader& r, long c)
{
    std::vector<NL_POINT> P(c > 0 ? c + 1 : 1);
    for (long i = 0; i < c; i++)
        P[i] = read_vec(r);
    return P;
}

std::vector<double> read_reals(Reader& r)
{
    long c = r.idx();
    std::vector<double> v(c > 0 ? c : 1, 0.0);
    for (long i = 0; i < c; i++)
        v[i] = r.next();
    return v;
}

void dump_vec(std::vector<double>& o, const NL_POINT& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

void curve_result(std::vector<double>& o, NL_FLAG err, NL_CURVE* cur)
{
    o.push_back(nl_status(err));
    if (err == NL_NO)
        dump_crv(o, cur);
}
} // namespace

SC_EXPORT int32_t sc_nlib_crv_fit_p3_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
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
    case 0: // N_FitCrvDerivs: p par points ds de has_cur [cn cp reals(knots)]
            // -> status, curve
    {
        long p = r.idx();
        int par = (int)r.idx();
        long c = r.idx();
        std::vector<NL_POINT> P = read_vecs(r, c);
        NL_VECTOR Ds = read_vec(r);
        NL_VECTOR De = read_vec(r);
        long has_cur = r.idx();
        NL_CURVE* cur = null_crv(&S);
        if (has_cur != 0)
        {
            long cn = r.idx();
            long cp = r.idx();
            std::vector<double> U = read_reals(r);
            long cm = (long)U.size() - 1;
            if (N_AllocCrvArrays(cur, cn, (NL_DEGREE)cp, cm, &S) != 0)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_CPOINT* Pw;
            NL_REAL* CU;
            N_CrvGetCPtsAndKnots(cur, &Pw, &CU);
            for (long i = 0; i <= cn; i++)
                Pw[i].x = Pw[i].y = Pw[i].z = Pw[i].w = 0.0;
            for (long i = 0; i <= cm; i++)
                CU[i] = U[i];
        }
        N_ErrClear();
        NL_FLAG err = N_FitCrvDerivs(P.data(), c - 1, (NL_DEGREE)p, Ds, De, par, cur, &S);
        curve_result(o, err, cur);
        break;
    }
    case 1: // N_FitCubicSplineInterp: par points ds de -> status, curve
    {
        int par = (int)r.idx();
        long c = r.idx();
        std::vector<NL_POINT> P = read_vecs(r, c);
        NL_VECTOR Ds = read_vec(r);
        NL_VECTOR De = read_vec(r);
        NL_CURVE* cur = null_crv(&S);
        N_ErrClear();
        NL_FLAG err = N_FitCubicSplineInterp(P.data(), c - 1, Ds, De, par, cur, &S);
        curve_result(o, err, cur);
        break;
    }
    case 2: // N_FitCrvFirstDeriv: p vec par points (d: x y z per point)
            // -> status, D after the call (x y z per point), curve
    {
        long p = r.idx();
        int vec = (int)r.idx();
        int par = (int)r.idx();
        long c = r.idx();
        std::vector<NL_POINT> P = read_vecs(r, c);
        std::vector<NL_POINT> D = read_vecs(r, c);
        NL_CURVE* cur = null_crv(&S);
        N_ErrClear();
        NL_FLAG err = N_FitCrvFirstDeriv(P.data(), D.data(), c - 1, (NL_DEGREE)p, vec, par, cur, &S);
        o.push_back(nl_status(err));
        for (long i = 0; i < c; i++)
            dump_vec(o, D[i]);
        if (err == NL_NO)
            dump_crv(o, cur);
        break;
    }
    case 3: // N_FitCrvKnotsAndTangents: p der per tan points has_u [u: one per point]
            // ts te has_knt [reals(knots)]
            // tan: 0 none, 1 start, 2 end, 3 both, 4 one vector as both
            // -> status, ts te after the call, knots after (count values; 0 = NULL),
            //    curve
    {
        long p = r.idx();
        int der = (int)r.idx();
        double per = r.next();
        long tan = r.idx();
        long c = r.idx();
        std::vector<NL_POINT> P = read_vecs(r, c);
        long has_u = r.idx();
        std::vector<double> u(c > 0 ? c : 1, 0.0);
        if (has_u != 0)
            for (long i = 0; i < c; i++)
                u[i] = r.next();
        NL_VECTOR Ts = read_vec(r);
        NL_VECTOR Te = read_vec(r);
        long has_knt = r.idx();
        NL_KNOTVECTOR* knt = NULL;
        if (has_knt != 0)
        {
            std::vector<double> U = read_reals(r);
            long m = (long)U.size() - 1;
            knt = N_AllocKnotVectorAndArray(m, &S);
            if (knt == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_INDEX mm;
            NL_REAL* KU;
            N_KnotVectorGetKnots(knt, &mm, &KU);
            for (long i = 0; i <= m; i++)
                KU[i] = U[i];
        }
        NL_VECTOR* pts = NULL;
        NL_VECTOR* pte = NULL;
        if (tan == 1 || tan == 3)
            pts = &Ts;
        if (tan == 2 || tan == 3)
            pte = &Te;
        if (tan == 4)
            pts = pte = &Ts;
        NL_CURVE* cur = null_crv(&S);
        N_ErrClear();
        NL_FLAG err = N_FitCrvKnotsAndTangents(P.data(), c - 1, has_u != 0 ? u.data() : NULL, pts, pte, der, &knt,
                                               per, (NL_DEGREE)p, cur, &S, &S);
        o.push_back(nl_status(err));
        dump_vec(o, Ts);
        dump_vec(o, Te);
        if (knt == NULL)
            o.push_back(0.0);
        else
        {
            NL_INDEX m;
            NL_REAL* KU;
            N_KnotVectorGetKnots(knt, &m, &KU);
            o.push_back((double)(m + 1));
            for (NL_INDEX i = 0; i <= m; i++)
                o.push_back(KU[i]);
        }
        if (err == NL_NO)
            dump_crv(o, cur);
        break;
    }
    case 4: // N_FitCrvApproxClosedConditions: p k par has_d points [d: k vectors, D[1..k]]
            // -> status, curve
    {
        long p = r.idx();
        long k = r.idx();
        int par = (int)r.idx();
        long has_d = r.idx();
        long c = r.idx();
        std::vector<NL_POINT> P = read_vecs(r, c);
        std::vector<NL_POINT> D(k + 1);
        if (has_d != 0)
            for (long i = 1; i <= k; i++)
                D[i] = read_vec(r);
        NL_CURVE* cur = null_crv(&S);
        N_ErrClear();
        NL_FLAG err = N_FitCrvApproxClosedConditions(P.data(), c - 1, (NL_DEGREE)p, has_d != 0 ? D.data() : NULL, k,
                                                     par, cur, &S);
        curve_result(o, err, cur);
        break;
    }
    case 5: // N_FitCrvHighDerivs: p par points kk (Ds[1..kk]) ll (De[1..ll]) -> status, curve
    {
        long p = r.idx();
        int par = (int)r.idx();
        long c = r.idx();
        std::vector<NL_POINT> P = read_vecs(r, c);
        long kk = r.idx();
        std::vector<NL_POINT> Ds(kk + 1);
        for (long i = 1; i <= kk; i++)
            Ds[i] = read_vec(r);
        long ll = r.idx();
        std::vector<NL_POINT> De(ll + 1);
        for (long i = 1; i <= ll; i++)
            De[i] = read_vec(r);
        NL_CURVE* cur = null_crv(&S);
        N_ErrClear();
        NL_FLAG err = N_FitCrvHighDerivs(P.data(), c - 1, (NL_DEGREE)p, Ds.data(), kk, De.data(), ll, par, cur, &S);
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
