// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.basis_adv`.
// Wraps routines of upstream source/SMLib/src/NL_BasisAdv.cpp:
//   N_GetCompatibleKnotArrayMult, N_BasisInsertKnots, N_BasisIncreaseKnotMult,
//   N_BasisDerivsArray.
//
// Status convention (as nlib_basis_basic.cpp): 0 = ok; when the upstream routine returns
// its error flag, the NLib error number NL_ERROR.eno (cleared before the call; 1000 if
// the flag was set without a number); SC_ERR_* for shim problems.
// The Rust wrapper (crates/solidcore-ffi/src/units/nlib_basis_adv.rs) validates every
// argument first; the checks here are a second line of defence.
//
// Defensive copies: N_GetCompatibleKnotArrayMult reads past U[m] on some inputs
// (upstream-issues UB-13); sc_nlib_basis_adv_compat_mult_walk detects exactly those and
// the wrapper rejects them, and the knot buffers still carry trailing +inf slots as a
// second line of defence. N_BasisInsertKnots reads X[mx+1] after inserting its last knot
// (UB-11; the value never changes the result), so the insert buffer carries one
// trailing slot, and its output buffer has room for the 2p+2+mx+1 knots it writes when
// R has fewer than 2p+2 knots (UB-196).
#include "sc_shim.h"

#include <algorithm>
#include <cmath>
#include <limits>
#include <vector>

#include <nurbs.h>
#include <NL_Globals.h>

namespace
{
int32_t nl_status(NL_FLAG err)
{
    if (err == NL_NO)
        return SC_OK;
    return NL_ERROR.eno != 0 ? (int32_t)NL_ERROR.eno : 1000;
}

const int64_t PAD = 8; // trailing +inf entries for N_GetCompatibleKnotArrayMult

// Outcomes of compat_mult_walk.
const int32_t WALK_IN_BOUNDS = 0;
const int32_t WALK_PAST_END = 1;
const int32_t WALK_ENDLESS = 2;

// Replays N_GetCompatibleKnotArrayMult(R, S, p) without writing output knots: its scale
// step (NL_BasisAdv.cpp:131-143, calling N_BasisReparam itself on copies) and its index
// walk (:145-204), checking every R[i] / S[j] index before the read. WALK_PAST_END when
// upstream would read past R[mr] or S[ms] (UB-13), WALK_ENDLESS when an iteration
// advances neither index (upstream would loop forever), else WALK_IN_BOUNDS. Reads of
// R[i+1] / S[j+1] happen only while i < mr-p / j < ms-p and are always in bounds.
// Requires n_r, n_s >= p+1.
int32_t compat_mult_walk(const double* r, int64_t n_r, const double* s, int64_t n_s, NL_DEGREE p)
{
    std::vector<double> R(r, r + n_r), S(s, s + n_s);
    const NL_INDEX mr = (NL_INDEX)(n_r - 1), ms = (NL_INDEX)(n_s - 1);
    NL_KNOTVECTOR knr, kns;
    N_KnotVectorFromRealArray(&knr, R.data(), mr);
    N_KnotVectorFromRealArray(&kns, S.data(), ms);
    if (fabs(R[0] - S[0]) > NL_PTOL || fabs(R[mr] - S[ms]) > NL_PTOL)
    {
        NL_INTERVAL I;
        if ((R[mr] - R[0]) > (S[ms] - S[0]))
        {
            N_CreateInterval(&I, R[0], R[mr]);
            N_BasisReparam(&kns, p, I);
        }
        else
        {
            N_CreateInterval(&I, S[0], S[ms]);
            N_BasisReparam(&knr, p, I);
        }
    }
    const NL_INDEX er = mr - p, es = ms - p;
    NL_INDEX i = p + 1, j = p + 1;
    while (i < er || j < es)
    {
        const NL_INDEX i0 = i, j0 = j;
        if (i > mr || j > ms)
            return WALK_PAST_END;
        if (fabs(R[i] - S[j]) < NL_PTOL)
        {
            while (i < er && R[i] == R[i + 1])
                i++;
            while (j < es && S[j] == S[j + 1])
                j++;
            i++;
            j++;
        }
        if (i > mr || j > ms)
            return WALK_PAST_END;
        if (R[i] < S[j])
        {
            while (i < er && R[i] == R[i + 1])
                i++;
            i++;
        }
        if (i > mr || j > ms)
            return WALK_PAST_END;
        if (R[i] > S[j])
        {
            while (j < es && S[j] == S[j + 1])
                j++;
            j++;
        }
        if (i == i0 && j == j0)
            return WALK_ENDLESS;
    }
    return WALK_IN_BOUNDS;
}
} // namespace

// Whether N_GetCompatibleKnotArrayMult(R, S, p) stays inside R[0..=mr] and S[0..=ms]:
// *out_walk = 0 (in bounds), 1 (reads past the end, UB-13) or 2 (would loop forever).
SC_EXPORT int32_t sc_nlib_basis_adv_compat_mult_walk(const double* r, int64_t n_r, const double* s, int64_t n_s,
                                                     int32_t p, int32_t* out_walk)
{
    if (n_r < 1 || n_s < 1 || p < 0 || p > 32767 || n_r < (int64_t)p + 1 || n_s < (int64_t)p + 1)
        return SC_ERR_ARG;
    SC_TRY
    *out_walk = compat_mult_walk(r, n_r, s, n_s, (NL_DEGREE)p);
    return SC_OK;
    SC_CATCH
}

// N_GetCompatibleKnotArrayMult(R, S, p) on copies -> out_r, out_s (rescaled knots),
// out_xr[0..*out_nxr), out_xs[0..*out_nxs) (capacity cap each).
SC_EXPORT int32_t sc_nlib_basis_adv_compat_mult(const double* r, int64_t n_r, const double* s, int64_t n_s,
                                                int32_t p, double* out_r, double* out_s, double* out_xr,
                                                int64_t* out_nxr, double* out_xs, int64_t* out_nxs, int64_t cap)
{
    if (n_r < 1 || n_s < 1 || p < 0 || p > 32767 || n_r < (int64_t)p + 1 || n_s < (int64_t)p + 1 ||
        cap < n_r + n_s + 2 * PAD)
        return SC_ERR_ARG;
    SC_TRY
    if (compat_mult_walk(r, n_r, s, n_s, (NL_DEGREE)p) != WALK_IN_BOUNDS)
        return SC_ERR_ARG;
    const double inf = std::numeric_limits<double>::infinity();
    std::vector<double> rb((size_t)(n_r + PAD), inf), sb((size_t)(n_s + PAD), inf);
    for (int64_t i = 0; i < n_r; ++i)
        rb[(size_t)i] = r[i];
    for (int64_t i = 0; i < n_s; ++i)
        sb[(size_t)i] = s[i];
    // Upstream writes the new knots into caller memory without a bound: give it room
    // for every index it can reach (each write consumes a knot of the other vector).
    std::vector<double> xr((size_t)cap), xs((size_t)cap);
    NL_KNOTVECTOR knr, kns, kxr, kxs;
    N_KnotVectorFromRealArray(&knr, rb.data(), (NL_INDEX)(n_r - 1));
    N_KnotVectorFromRealArray(&kns, sb.data(), (NL_INDEX)(n_s - 1));
    N_KnotVectorFromRealArray(&kxr, xr.data(), (NL_INDEX)(cap - 1));
    N_KnotVectorFromRealArray(&kxs, xs.data(), (NL_INDEX)(cap - 1));
    N_ErrClear();
    N_GetCompatibleKnotArrayMult(&knr, &kns, (NL_DEGREE)p, &kxr, &kxs);
    NL_INDEX mxr = -1, mxs = -1;
    NL_REAL* U = nullptr;
    N_KnotVectorGetKnots(&kxr, &mxr, &U);
    N_KnotVectorGetKnots(&kxs, &mxs, &U);
    for (int64_t i = 0; i < n_r; ++i)
        out_r[i] = rb[(size_t)i];
    for (int64_t i = 0; i < n_s; ++i)
        out_s[i] = sb[(size_t)i];
    if ((int64_t)mxr + 1 > cap || (int64_t)mxs + 1 > cap)
        return SC_ERR_BUFFER;
    *out_nxr = (int64_t)mxr + 1;
    *out_nxs = (int64_t)mxs + 1;
    for (int64_t i = 0; i <= mxr; ++i)
        out_xr[i] = xr[(size_t)i];
    for (int64_t i = 0; i <= mxs; ++i)
        out_xs[i] = xs[(size_t)i];
    return SC_OK;
    SC_CATCH
}

// N_BasisInsertKnots(R, p, X) -> out[0..*out_len) (the refined knot vector).
SC_EXPORT int32_t sc_nlib_basis_adv_insert(const double* r, int64_t n_r, int32_t p, const double* x, int64_t n_x,
                                           double* out, int64_t cap, int64_t* out_len)
{
    if (n_r < 1 || n_x < 1 || p < 0 || p > 32767 || n_r < (int64_t)p + 1)
        return SC_ERR_ARG;
    SC_TRY
    std::vector<double> rb(r, r + n_r);
    std::vector<double> xb((size_t)(n_x + 1), 0.0); // + X[mx+1] read
    for (int64_t i = 0; i < n_x; ++i)
        xb[(size_t)i] = x[i];
    // Upstream writes p+1 + max(0, m-2p-1) + mx+1 + p+1 knots: m+mx+2 once R has at
    // least 2p+2 knots, 2p+2+mx+1 below that (UB-196).
    const int64_t ns = std::max(n_r, 2 * (int64_t)p + 2) + n_x;
    std::vector<double> sb((size_t)ns);
    NL_KNOTVECTOR knr, knx, kns;
    N_KnotVectorFromRealArray(&knr, rb.data(), (NL_INDEX)(n_r - 1));
    N_KnotVectorFromRealArray(&knx, xb.data(), (NL_INDEX)(n_x - 1));
    N_KnotVectorFromRealArray(&kns, sb.data(), (NL_INDEX)(ns - 1));
    N_ErrClear();
    NL_FLAG err = N_BasisInsertKnots(&knr, (NL_DEGREE)p, &knx, &kns);
    *out_len = 0;
    if (err != NL_NO)
        return nl_status(err);
    NL_INDEX ms = -1;
    NL_REAL* S = nullptr;
    N_KnotVectorGetKnots(&kns, &ms, &S);
    return sc_copy_out(S, (int64_t)ms + 1, out, cap, out_len);
    SC_CATCH
}

// N_BasisIncreaseKnotMult(U, p, t) -> out[0..*out_len) (the knots to insert).
SC_EXPORT int32_t sc_nlib_basis_adv_inc_mult(const double* u, int64_t n_u, int32_t p, int64_t t, double* out,
                                             int64_t cap, int64_t* out_len)
{
    if (n_u < 1 || p < 0 || p > 32767 || t < 0 || t > 1000)
        return SC_ERR_ARG;
    SC_TRY
    std::vector<double> ub(u, u + n_u);
    // At most n_u distinct inner knots, t copies each.
    std::vector<double> xb((size_t)(n_u * t + 1));
    NL_KNOTVECTOR knt, knx;
    N_KnotVectorFromRealArray(&knt, ub.data(), (NL_INDEX)(n_u - 1));
    N_KnotVectorFromRealArray(&knx, xb.data(), (NL_INDEX)(n_u * t));
    N_ErrClear();
    NL_FLAG err = N_BasisIncreaseKnotMult(&knt, (NL_DEGREE)p, (NL_INDEX)t, &knx);
    *out_len = 0;
    if (err != NL_NO)
        return nl_status(err);
    NL_INDEX mx = -1;
    NL_REAL* X = nullptr;
    N_KnotVectorGetKnots(&knx, &mx, &X);
    return sc_copy_out(X, (int64_t)mx + 1, out, cap, out_len);
    SC_CATCH
}

// N_BasisDerivsArray(knt, p, u[0..n_u), n_u-1, flg, der) ->
// out_nd[(k*(p+1) + i)*n_u + j] (k = 0..=der, i = 0..=p, j < n_u), out_spn[0..n_u).
SC_EXPORT int32_t sc_nlib_basis_adv_derivs_array(const double* knots, int64_t n_knots, int32_t p, const double* u,
                                                 int64_t n_u, int32_t flag, int64_t der, double* out_nd,
                                                 int64_t cap, int64_t* out_spn)
{
    if (p < 0 || p > 1000 || n_knots < 2 * (int64_t)p + 2 || n_u < 1 || der < 0 || der > 1000 ||
        (flag != NL_LEFT && flag != NL_RIGHT) || cap < (der + 1) * ((int64_t)p + 1) * n_u)
        return SC_ERR_ARG;
    SC_TRY
    std::vector<double> kb(knots, knots + n_knots);
    std::vector<double> ub(u, u + n_u);
    NL_KNOTVECTOR knt;
    N_KnotVectorFromRealArray(&knt, kb.data(), (NL_INDEX)(n_knots - 1));
    std::vector<NL_REAL*> rows((size_t)((der + 1) * ((int64_t)p + 1)));
    std::vector<NL_REAL**> planes((size_t)(der + 1));
    for (int64_t k = 0; k <= der; ++k)
    {
        for (int64_t i = 0; i <= p; ++i)
            rows[(size_t)(k * (p + 1) + i)] = out_nd + (k * (p + 1) + i) * n_u;
        planes[(size_t)k] = rows.data() + k * (p + 1);
    }
    std::vector<NL_INDEX> spn((size_t)n_u, -1);
    N_ErrClear();
    NL_FLAG err = N_BasisDerivsArray(&knt, (NL_DEGREE)p, ub.data(), (NL_INDEX)(n_u - 1), (NL_FLAG)flag,
                                     (NL_INDEX)der, planes.data(), spn.data());
    for (int64_t j = 0; j < n_u; ++j)
        out_spn[j] = (int64_t)spn[(size_t)j];
    return nl_status(err);
    SC_CATCH
}
