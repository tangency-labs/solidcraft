// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.crv_offset`.
// Wraps upstream N_CrvOffsetPtSampling (source/SMLib/src/NL_CrvOffset.cpp).
//
// sc_nlib_crv_offset_eval(in, n_in, max_samples, out, cap, out_len): `in` holds a curve
// (n p m, n+1 control points x y z w, m+1 knots) then B[3] d deg tan par tol. `out`
// receives the status (0 = ok, else NL_ERROR.eno, cleared before the call; 1000 if the
// flag was set without a number), then on success the offset curve (n p m, used control
// points, used knots).
//
// Upstream casts the per-segment sample count ceil(f1 * gro) to NL_INDEX unchecked, so
// before the call the shim recomputes that count exactly as upstream does (same Bezier
// decomposition, N_CrvOffsetGetMax2ndDeriv on the unit-span segments) and returns
// SC_ERR_ARG when it is not finite or the total exceeds `max_samples`. The Rust wrapper
// validates the curve and parameters (crates/solidcore-ffi/src/units/nlib_crv_offset.rs).
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

#include <cmath>
#include <vector>

#include "sc_nlib.h"

using sc::dump_crv;
using sc::nl_status;
using sc::read_crv;

namespace
{

// true when the sampling count of N_CrvOffsetPtSampling is safe (or never computed).
bool samples_ok(NL_CURVE* curP, NL_VECTOR B, NL_REAL d, NL_DEGREE deg, NL_REAL tol, double max_samples)
{
    if (tol == 0.0)
        return true; // NL_NUM_ERR before any sampling.
    NL_REAL exp = (deg == 1) ? 0.5 : 0.34;
    NL_REAL f1 = pow(1.0 / tol, exp);
    NL_REAL f2 = 2.0 * (NL_REAL)deg;
    if (!std::isfinite(f1))
        return false;
    if (N_VectorNormalizeRef(&B) == NL_YES)
        return true; // fails before sampling.
    NL_STACKS SL;
    N_InitNurbs(&SL);
    NL_CURVE** bez;
    NL_INDEX k;
    bool ok = true;
    if (N_CrvDecomposeBez(curP, &bez, &k, &SL) == NL_NO)
    {
        double mu = 0.0;
        for (NL_INDEX i = 0; i <= k; i++)
        {
            N_CrvReparamToInterval(bez[i], NL_UNITSPAN);
            NL_REAL Muu;
            if (N_CrvOffsetGetMax2ndDeriv(bez[i], d, B, &Muu) == NL_YES)
                break; // fails before the cast.
            NL_REAL num = 0.125 * Muu;
            NL_REAL gro = NL_MAX(f2, sqrt(num));
            NL_REAL del = ceil(f1 * gro);
            if (!(del >= 0.0 && del <= max_samples))
            {
                ok = false;
                break;
            }
            mu += del;
            if (mu > max_samples)
            {
                ok = false;
                break;
            }
        }
    }
    N_EndNurbs(&SL);
    N_ErrClear();
    return ok;
}
} // namespace

SC_EXPORT int32_t sc_nlib_crv_offset_eval(const double* in, int64_t n_in, double max_samples,
                                          double* out, int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int64_t at = 0;
    int32_t rc = SC_OK;
    NL_CURVE* cur = read_crv(in, &at, &S);
    if (cur == NULL)
    {
        rc = SC_ERR_ARG;
    }
    else
    {
        const double* prm = in + at;
        NL_VECTOR B;
        B.x = prm[0];
        B.y = prm[1];
        B.z = prm[2];
        NL_REAL d = prm[3];
        NL_DEGREE deg = (NL_DEGREE)prm[4];
        NL_FLAG tan = (NL_FLAG)prm[5];
        NL_FLAG par = (NL_FLAG)prm[6];
        NL_REAL tol = prm[7];
        if (!samples_ok(cur, B, d, deg, tol, max_samples))
        {
            rc = SC_ERR_ARG;
        }
        else
        {
            NL_CURVE curQ;
            N_CrvInitArrays(&curQ);
            N_ErrClear();
            NL_FLAG err = N_CrvOffsetPtSampling(cur, B, d, deg, tan, par, tol, &curQ, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_crv(o, &curQ);
        }
    }
    N_EndNurbs(&S);
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
