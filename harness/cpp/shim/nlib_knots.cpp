// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.knots`.
// Wraps the knot-vector / B-spline basis routines of upstream
// source/SMLib/src/NL_BasisBasic.cpp:
//   N_KnotVectorFromRealArray, N_SetKnotIndex, N_KnotVectorGetKnots,
//   N_KnotVectorIsParamOutOfBounds, N_KnotVectorIsEndParam, N_KnotVectorIsValid,
//   N_BasisFindSpan, N_BasisFindSpanAndMult, N_BasisGetSpanCount,
//   N_BasisEval, N_BasisIEval, N_BasisIDerivs.
//
// Status convention: 0 = ok; when the upstream routine returns its error flag,
// the shim returns the NLib error number NL_ERROR.eno (cleared before the call;
// 1000 if the flag was set without a number); SC_ERR_* for shim problems.
// The Rust wrapper (crates/solidcore-ffi/src/units/nlib_knots.rs) validates
// every argument first; the checks here are a second line of defence against
// inputs on which upstream would read or write out of bounds.
// Deviations: span/basis degrees are bounded to 1000. Only span/full-basis routines
// require 2*p+2 knots; single-basis routines accept any nonempty vector and
// leave parameter/index errors (in that order) to upstream and nl_status.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

namespace
{
// Status for an upstream error flag.
int32_t nl_status(NL_FLAG err)
{
    if (err == NL_NO)
        return SC_OK;
    return NL_ERROR.eno != 0 ? (int32_t)NL_ERROR.eno : 1000;
}

NL_KNOTVECTOR make_knt(const double* knots, int64_t n_knots)
{
    NL_KNOTVECTOR knt;
    N_KnotVectorFromRealArray(&knt, const_cast<NL_REAL*>(knots), (NL_INDEX)(n_knots - 1));
    return knt;
}

bool valid_flag(int32_t flag) { return flag == NL_LEFT || flag == NL_RIGHT; }

// Knot vector long enough for every index the span/basis routines touch.
bool basis_domain(int64_t n_knots, int32_t degree)
{
    return degree >= 0 && degree <= 1000 && n_knots >= 2 * (int64_t)degree + 2;
}

// Upstream checks the parameter, then i <= m-p-1, before reading basis knots.
bool single_basis_domain(int64_t n_knots, int32_t degree, int32_t flag)
{
    return n_knots >= 1 && degree >= 0 && degree <= 1000 && valid_flag(flag);
}
} // namespace

// N_KnotVectorFromRealArray(knots[0..buf_len), m) + N_SetKnotIndex(new_m) +
// N_KnotVectorGetKnots: writes the resulting m and U[0..=m] (cap >= new_m+1).
SC_EXPORT int32_t sc_nlib_knots_roundtrip(const double* buf, int64_t buf_len, int64_t m, int64_t new_m,
                                          int64_t* out_m, double* out_knots, int64_t cap)
{
    if (buf_len < 1 || m < 0 || m >= buf_len || new_m < 0 || new_m >= buf_len || cap < new_m + 1)
        return SC_ERR_ARG;
    SC_TRY
    NL_KNOTVECTOR knt;
    N_KnotVectorFromRealArray(&knt, const_cast<NL_REAL*>(buf), (NL_INDEX)m);
    N_SetKnotIndex(&knt, (NL_INDEX)new_m);
    NL_INDEX mm = -1;
    NL_REAL* U = nullptr;
    N_KnotVectorGetKnots(&knt, &mm, &U);
    *out_m = (int64_t)mm;
    for (int64_t i = 0; i <= mm; ++i)
        out_knots[i] = U[i];
    return SC_OK;
    SC_CATCH
}

// N_KnotVectorIsParamOutOfBounds / N_KnotVectorIsEndParam -> out_flags[0], out_flags[1].
SC_EXPORT int32_t sc_nlib_knots_param_checks(const double* knots, int64_t n_knots, double u,
                                             int32_t* out_flags)
{
    if (n_knots < 1)
        return SC_ERR_ARG;
    SC_TRY
    NL_KNOTVECTOR knt = make_knt(knots, n_knots);
    out_flags[0] = (int32_t)N_KnotVectorIsParamOutOfBounds(&knt, u, (NL_STRING)_T("sc_shim"));
    out_flags[1] = (int32_t)N_KnotVectorIsEndParam(&knt, u, _T("sc_shim"));
    return SC_OK;
    SC_CATCH
}

// N_KnotVectorIsValid(knt, p): status 0 = valid, else NL_KNT_ERR.
SC_EXPORT int32_t sc_nlib_knots_is_valid(const double* knots, int64_t n_knots, int32_t degree)
{
    if (n_knots < 1 || degree < 0 || degree > 32767)
        return SC_ERR_ARG;
    SC_TRY
    NL_KNOTVECTOR knt = make_knt(knots, n_knots);
    N_ErrClear();
    NL_FLAG err = N_KnotVectorIsValid(&knt, (NL_DEGREE)degree, (NL_STRING)_T("sc_shim"));
    return nl_status(err);
    SC_CATCH
}

// N_BasisGetSpanCount(knt, p) -> out_count.
SC_EXPORT int32_t sc_nlib_knots_span_count(const double* knots, int64_t n_knots, int32_t degree,
                                           int64_t* out_count)
{
    if (n_knots < 1 || degree < 0 || degree > 32767)
        return SC_ERR_ARG;
    SC_TRY
    NL_KNOTVECTOR knt = make_knt(knots, n_knots);
    NL_INDEX nsp = -1;
    N_BasisGetSpanCount(&knt, (NL_DEGREE)degree, &nsp);
    *out_count = (int64_t)nsp;
    return SC_OK;
    SC_CATCH
}

// N_BasisFindSpan(knt, p, u, flg) -> out_span.
SC_EXPORT int32_t sc_nlib_knots_find_span(const double* knots, int64_t n_knots, int32_t degree, double u,
                                          int32_t flag, int64_t* out_span)
{
    if (!basis_domain(n_knots, degree) || !valid_flag(flag))
        return SC_ERR_ARG;
    SC_TRY
    NL_KNOTVECTOR knt = make_knt(knots, n_knots);
    NL_INDEX spn = -1;
    N_ErrClear();
    NL_FLAG err = N_BasisFindSpan(&knt, (NL_DEGREE)degree, u, (NL_FLAG)flag, &spn);
    *out_span = (int64_t)spn;
    return nl_status(err);
    SC_CATCH
}

// N_BasisFindSpanAndMult(knt, p, u, flg) -> out_span, out_mult.
SC_EXPORT int32_t sc_nlib_knots_find_span_and_mult(const double* knots, int64_t n_knots, int32_t degree,
                                                   double u, int32_t flag, int64_t* out_span,
                                                   int64_t* out_mult)
{
    if (!basis_domain(n_knots, degree) || !valid_flag(flag))
        return SC_ERR_ARG;
    SC_TRY
    NL_KNOTVECTOR knt = make_knt(knots, n_knots);
    NL_INDEX spn = -1, mlt = -1;
    N_ErrClear();
    NL_FLAG err = N_BasisFindSpanAndMult(&knt, (NL_DEGREE)degree, u, (NL_FLAG)flag, &spn, &mlt);
    *out_span = (int64_t)spn;
    *out_mult = (int64_t)mlt;
    return nl_status(err);
    SC_CATCH
}

// N_BasisEval(knt, p, u, flg) -> out_basis[0..=p] (cap >= p+1), out_span.
SC_EXPORT int32_t sc_nlib_knots_basis_eval(const double* knots, int64_t n_knots, int32_t degree, double u,
                                           int32_t flag, double* out_basis, int64_t cap, int64_t* out_span)
{
    // left/right scratch arrays hold NL_MAXDEG + 2 entries, indexed up to p.
    if (!basis_domain(n_knots, degree) || degree > NL_MAXDEG + 1 || !valid_flag(flag) || cap < degree + 1)
        return SC_ERR_ARG;
    SC_TRY
    NL_KNOTVECTOR knt = make_knt(knots, n_knots);
    NL_INDEX spn = -1;
    N_ErrClear();
    NL_FLAG err = N_BasisEval(&knt, (NL_DEGREE)degree, u, (NL_FLAG)flag, out_basis, &spn);
    *out_span = (int64_t)spn;
    return nl_status(err);
    SC_CATCH
}

// N_BasisIEval(knt, i, p, u, flg) -> out_value.
SC_EXPORT int32_t sc_nlib_knots_basis_i_eval(const double* knots, int64_t n_knots, int64_t index,
                                             int32_t degree, double u, int32_t flag, double* out_value)
{
    if (!single_basis_domain(n_knots, degree, flag) || index < 0)
        return SC_ERR_ARG;
    SC_TRY
    NL_KNOTVECTOR knt = make_knt(knots, n_knots);
    NL_REAL N = 0.0;
    N_ErrClear();
    NL_FLAG err = N_BasisIEval(&knt, (NL_INDEX)index, (NL_DEGREE)degree, u, (NL_FLAG)flag, &N);
    *out_value = N;
    return nl_status(err);
    SC_CATCH
}

// N_BasisIDerivs(knt, i, p, u, flg, der) -> out_derivs[0..=der] (cap >= der+1).
SC_EXPORT int32_t sc_nlib_knots_basis_i_derivs(const double* knots, int64_t n_knots, int64_t index,
                                               int32_t degree, double u, int32_t flag, int64_t der,
                                               double* out_derivs, int64_t cap)
{
    if (!single_basis_domain(n_knots, degree, flag) || index < 0 || der < 0 || cap < der + 1)
        return SC_ERR_ARG;
    SC_TRY
    NL_KNOTVECTOR knt = make_knt(knots, n_knots);
    N_ErrClear();
    NL_FLAG err =
        N_BasisIDerivs(&knt, (NL_INDEX)index, (NL_DEGREE)degree, u, (NL_FLAG)flag, (NL_INDEX)der, out_derivs);
    return nl_status(err);
    SC_CATCH
}
