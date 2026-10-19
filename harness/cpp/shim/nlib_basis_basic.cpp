// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.basis_basic`.
// Wraps routines of upstream source/SMLib/src/NL_BasisBasic.cpp:
//   N_BasisFindGlobalMax, N_BasisFindAllSpanMaxima, N_BasisReparam,
//   N_GetCompatibleKnotArray, N_GetCompatibleKnotVectorToTol (+ ST_notdone),
//   N_BasisDerivs, N_MakeKnotsCompatible.
//
// Status convention (as nlib_knots.cpp): 0 = ok; when the upstream routine returns
// its error flag, the NLib error number NL_ERROR.eno (cleared before the call; 1000
// if the flag was set without a number); SC_ERR_* for shim problems.
// The Rust wrapper (crates/solidcore-ffi/src/units/nlib_basis_basic.rs) validates
// every argument first; the checks here are a second line of defence.
//
// Defensive copies: upstream N_GetCompatibleKnotVectorToTol may read U[-1] of a knot
// vector, so its buffers carry a leading slot (the read never changes the result).
// N_MakeKnotsCompatible reads past R[mr] / S[ms] on some inputs (upstream-issues
// UB-13); sc_nlib_basis_basic_make_compatible_walk detects exactly those and the
// wrapper rejects them, and its knot buffers still carry trailing +inf slots as a
// second line of defence.
#include "sc_shim.h"

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

NL_KNOTVECTOR make_knt(const double* knots, int64_t n_knots)
{
    NL_KNOTVECTOR knt;
    N_KnotVectorFromRealArray(&knt, const_cast<NL_REAL*>(knots), (NL_INDEX)(n_knots - 1));
    return knt;
}

bool valid_flag(int32_t flag) { return flag == NL_LEFT || flag == NL_RIGHT; }

bool basis_domain(int64_t n_knots, int32_t degree)
{
    return degree >= 0 && degree <= 1000 && n_knots >= 2 * (int64_t)degree + 2;
}

const int64_t MC_PAD = 8; // trailing +inf entries for N_MakeKnotsCompatible

// Outcomes of make_compatible_walk.
const int32_t WALK_IN_BOUNDS = 0;
const int32_t WALK_PAST_END = 1;
const int32_t WALK_ENDLESS = 2;

// Replays N_MakeKnotsCompatible(R, S, p, q) without writing new knots: its scale step
// (NL_BasisBasic.cpp:3681-3693, calling N_BasisReparam itself on copies) and its index
// walk (:3695-3735, reads at :3706-3729), checking every R[i] / S[j] index before the read. WALK_PAST_END
// when upstream would read past R[mr] or S[ms] (UB-13), WALK_ENDLESS when an iteration
// advances neither index (upstream would loop forever), else WALK_IN_BOUNDS. Reads of
// R[i+1] / S[j+1] happen only while i <= nr / j <= ns and are always in bounds.
// Requires n_r >= p+1, n_s >= q+1.
int32_t make_compatible_walk(const double* r, int64_t n_r, const double* s, int64_t n_s, NL_DEGREE p,
                             NL_DEGREE q)
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
            N_BasisReparam(&kns, q, I);
        }
        else
        {
            N_CreateInterval(&I, S[0], S[ms]);
            N_BasisReparam(&knr, p, I);
        }
    }
    const NL_INDEX nr = mr - p - 1, ns = ms - q - 1;
    NL_INDEX i = p + 1, j = q + 1;
    while (i <= nr || j <= ns)
    {
        const NL_INDEX i0 = i, j0 = j;
        if (i > mr || j > ms)
            return WALK_PAST_END;
        if (fabs(R[i] - S[j]) < NL_PTOL)
        {
            while (i <= nr && R[i] == R[i + 1])
                i++;
            while (j <= ns && S[j] == S[j + 1])
                j++;
            i++;
            j++;
        }
        if (i > mr || j > ms)
            return WALK_PAST_END;
        if (R[i] < S[j])
        {
            while (i <= nr && R[i] == R[i + 1])
                i++;
            i++; // upstream stores R[i] (in bounds: i <= max(i0, nr+1) <= mr) first
        }
        if (i > mr || j > ms)
            return WALK_PAST_END;
        if (R[i] > S[j])
        {
            while (j <= ns && S[j] == S[j + 1])
                j++;
            j++; // upstream stores S[j] first, in bounds likewise
        }
        if (i == i0 && j == j0)
            return WALK_ENDLESS;
    }
    return WALK_IN_BOUNDS;
}
} // namespace

// Whether N_MakeKnotsCompatible(R, S, p, q) stays inside R[0..=mr] and S[0..=ms]:
// *out_walk = 0 (in bounds), 1 (reads past the end, UB-13) or 2 (would loop forever).
SC_EXPORT int32_t sc_nlib_basis_basic_make_compatible_walk(const double* r, int64_t n_r, const double* s,
                                                           int64_t n_s, int32_t p, int32_t q, int32_t* out_walk)
{
    if (n_r < 1 || n_s < 1 || p < 0 || q < 0 || p > 32767 || q > 32767 || n_r < (int64_t)p + 1 ||
        n_s < (int64_t)q + 1)
        return SC_ERR_ARG;
    SC_TRY
    *out_walk = make_compatible_walk(r, n_r, s, n_s, (NL_DEGREE)p, (NL_DEGREE)q);
    return SC_OK;
    SC_CATCH
}

// N_BasisFindGlobalMax(knt, i, p, tol) -> out[0] = max, out[1] = u.
SC_EXPORT int32_t sc_nlib_basis_basic_global_max(const double* knots, int64_t n_knots, int64_t index,
                                                 int32_t degree, double tol, double* out)
{
    if (!basis_domain(n_knots, degree) || index < 0)
        return SC_ERR_ARG;
    SC_TRY
    NL_KNOTVECTOR knt = make_knt(knots, n_knots);
    NL_REAL mx = 0.0, u = 0.0;
    N_ErrClear();
    NL_FLAG err = N_BasisFindGlobalMax(&knt, (NL_INDEX)index, (NL_DEGREE)degree, tol, &mx, &u);
    out[0] = mx;
    out[1] = u;
    return nl_status(err);
    SC_CATCH
}

// N_BasisFindAllSpanMaxima(knt, i, p, tol) -> out_min[0..=p], out_max[0..=p], out_u.
SC_EXPORT int32_t sc_nlib_basis_basic_span_maxima(const double* knots, int64_t n_knots, int64_t index,
                                                  int32_t degree, double tol, double* out_min,
                                                  double* out_max, int64_t cap, double* out_u)
{
    if (!basis_domain(n_knots, degree) || index < 0 || cap < degree + 1)
        return SC_ERR_ARG;
    SC_TRY
    NL_KNOTVECTOR knt = make_knt(knots, n_knots);
    NL_REAL u = 0.0;
    N_ErrClear();
    NL_FLAG err = N_BasisFindAllSpanMaxima(&knt, (NL_INDEX)index, (NL_DEGREE)degree, tol, out_min, out_max, &u);
    *out_u = u;
    return nl_status(err);
    SC_CATCH
}

// N_BasisReparam(knt, p, [ul, ur]) on a copy of knots -> out_knots[0..n_knots).
SC_EXPORT int32_t sc_nlib_basis_basic_reparam(const double* knots, int64_t n_knots, int32_t degree,
                                              double ul, double ur, double* out_knots)
{
    if (n_knots < 1 || degree < 0 || degree > 32767 || n_knots < (int64_t)degree + 1)
        return SC_ERR_ARG;
    SC_TRY
    for (int64_t i = 0; i < n_knots; ++i)
        out_knots[i] = knots[i];
    NL_KNOTVECTOR knt = make_knt(out_knots, n_knots);
    NL_INTERVAL I;
    N_CreateInterval(&I, ul, ur);
    N_BasisReparam(&knt, (NL_DEGREE)degree, I);
    return SC_OK;
    SC_CATCH
}

// N_GetCompatibleKnotVectorToTol(knt[0..n_vec), n_vec-1, tol) (or, with use_array != 0,
// N_GetCompatibleKnotArray, tol ignored). Input vectors are concatenated in `flat` with
// lengths `lens`. Writes the (adjusted) input knots to out_knots (same layout), the new
// knots of vector r to out_new[r*new_cap ..] with count out_new_lens[r].
SC_EXPORT int32_t sc_nlib_basis_basic_compatible(const double* flat, const int64_t* lens, int64_t n_vec,
                                                 double tol, int32_t use_array, double* out_knots,
                                                 double* out_new, int64_t new_cap, int64_t* out_new_lens)
{
    if (n_vec < 1)
        return SC_ERR_ARG;
    int64_t total = 0;
    for (int64_t r = 0; r < n_vec; ++r)
    {
        if (lens[r] < 1 || lens[r] < n_vec - 1)
            return SC_ERR_ARG;
        total += lens[r] - 1;
    }
    if (new_cap < total + 1)
        return SC_ERR_ARG;
    SC_TRY
    // One leading slot per vector: upstream may read U[-1].
    std::vector<std::vector<double>> bufs((size_t)n_vec);
    std::vector<NL_KNOTVECTOR> kv((size_t)n_vec);
    std::vector<NL_KNOTVECTOR*> kp((size_t)n_vec);
    int64_t off = 0;
    for (int64_t r = 0; r < n_vec; ++r)
    {
        bufs[r].assign((size_t)lens[r] + 1, 0.0);
        for (int64_t i = 0; i < lens[r]; ++i)
            bufs[r][(size_t)i + 1] = flat[off + i];
        off += lens[r];
        N_KnotVectorFromRealArray(&kv[r], bufs[r].data() + 1, (NL_INDEX)(lens[r] - 1));
        kp[r] = &kv[r];
    }
    NL_STACKS SK;
    N_InitNurbs(&SK);
    NL_KNOTVECTOR** knx = nullptr;
    N_ErrClear();
    NL_FLAG err = use_array != 0 ? N_GetCompatibleKnotArray(kp.data(), (NL_INDEX)(n_vec - 1), &knx, &SK)
                                 : N_GetCompatibleKnotVectorToTol(kp.data(), (NL_INDEX)(n_vec - 1), tol, &knx, &SK);
    int32_t st = nl_status(err);
    off = 0;
    for (int64_t r = 0; r < n_vec; ++r)
    {
        for (int64_t i = 0; i < lens[r]; ++i)
            out_knots[off + i] = bufs[r][(size_t)i + 1];
        off += lens[r];
        out_new_lens[r] = 0;
    }
    if (st == SC_OK && knx != nullptr)
    {
        for (int64_t r = 0; r < n_vec; ++r)
        {
            NL_INDEX m = -1;
            NL_REAL* U = nullptr;
            N_KnotVectorGetKnots(knx[r], &m, &U);
            int64_t cnt = (int64_t)m + 1;
            if (cnt > new_cap)
            {
                st = SC_ERR_BUFFER;
                break;
            }
            for (int64_t i = 0; i < cnt; ++i)
                out_new[r * new_cap + i] = U[i];
            out_new_lens[r] = cnt;
        }
    }
    N_EndNurbs(&SK);
    return st;
    SC_CATCH
}

// N_BasisDerivs(knt, p, u, flg, der) -> out_nd[k*(p+1) + j] (k = 0..=der), out_span.
SC_EXPORT int32_t sc_nlib_basis_basic_derivs(const double* knots, int64_t n_knots, int32_t degree, double u,
                                             int32_t flag, int64_t der, double* out_nd, int64_t cap,
                                             int64_t* out_span)
{
    if (!basis_domain(n_knots, degree) || !valid_flag(flag) || der < 0 || der > 1000 ||
        cap < (der + 1) * ((int64_t)degree + 1))
        return SC_ERR_ARG;
    SC_TRY
    NL_KNOTVECTOR knt = make_knt(knots, n_knots);
    std::vector<NL_REAL*> rows((size_t)der + 1);
    for (int64_t k = 0; k <= der; ++k)
        rows[(size_t)k] = out_nd + k * ((int64_t)degree + 1);
    NL_INDEX spn = -1;
    N_ErrClear();
    NL_FLAG err = N_BasisDerivs(&knt, (NL_DEGREE)degree, u, (NL_FLAG)flag, (NL_INDEX)der, rows.data(), &spn);
    *out_span = (int64_t)spn;
    return nl_status(err);
    SC_CATCH
}

// N_MakeKnotsCompatible(R, S, p, q) on copies -> out_r, out_s (rescaled knots),
// out_xr[0..*out_nxr), out_xs[0..*out_nxs) (capacity cap each).
SC_EXPORT int32_t sc_nlib_basis_basic_make_compatible(const double* r, int64_t n_r, const double* s, int64_t n_s,
                                                      int32_t p, int32_t q, double* out_r, double* out_s,
                                                      double* out_xr, int64_t* out_nxr, double* out_xs,
                                                      int64_t* out_nxs, int64_t cap)
{
    if (n_r < 1 || n_s < 1 || p < 0 || q < 0 || p > 32767 || q > 32767 || n_r < (int64_t)p + 1 ||
        n_s < (int64_t)q + 1 || cap < n_r + n_s + 2 * MC_PAD)
        return SC_ERR_ARG;
    SC_TRY
    if (make_compatible_walk(r, n_r, s, n_s, (NL_DEGREE)p, (NL_DEGREE)q) != WALK_IN_BOUNDS)
        return SC_ERR_ARG;
    const double inf = std::numeric_limits<double>::infinity();
    std::vector<double> rb((size_t)(n_r + MC_PAD), inf), sb((size_t)(n_s + MC_PAD), inf);
    for (int64_t i = 0; i < n_r; ++i)
        rb[(size_t)i] = r[i];
    for (int64_t i = 0; i < n_s; ++i)
        sb[(size_t)i] = s[i];
    // Upstream writes the new knots into caller memory without a bound: give it room
    // for every index it can reach (each write advances i or j past a real knot).
    std::vector<double> xr((size_t)cap), xs((size_t)cap);
    NL_KNOTVECTOR knr = make_knt(rb.data(), n_r), kns = make_knt(sb.data(), n_s);
    NL_KNOTVECTOR kxr, kxs;
    N_KnotVectorFromRealArray(&kxr, xr.data(), (NL_INDEX)(cap - 1));
    N_KnotVectorFromRealArray(&kxs, xs.data(), (NL_INDEX)(cap - 1));
    N_ErrClear();
    N_MakeKnotsCompatible(&knr, &kns, (NL_DEGREE)p, (NL_DEGREE)q, &kxr, &kxs);
    NL_INDEX mxr = -1, mxs = -1;
    NL_REAL* U = nullptr;
    N_KnotVectorGetKnots(&kxr, &mxr, &U);
    N_KnotVectorGetKnots(&kxs, &mxs, &U);
    for (int64_t i = 0; i < n_r; ++i)
        out_r[i] = rb[(size_t)i];
    for (int64_t i = 0; i < n_s; ++i)
        out_s[i] = sb[(size_t)i];
    *out_nxr = (int64_t)mxr + 1;
    *out_nxs = (int64_t)mxs + 1;
    for (int64_t i = 0; i <= mxr; ++i)
        out_xr[i] = xr[(size_t)i];
    for (int64_t i = 0; i <= mxs; ++i)
        out_xs[i] = xs[(size_t)i];
    return SC_OK;
    SC_CATCH
}
