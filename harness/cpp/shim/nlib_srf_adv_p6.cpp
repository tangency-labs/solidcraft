// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.srf_adv_p6`.
// Wraps N_CreateSweepScale and N_FillNSidedHole (upstream source/SMLib/src/NL_SrfAdv.cpp).
//
// sc_nlib_srf_adv_p6_sweep(in, n_in, out, cap, out_len): `in` holds
//   curT, curC, Z0 (x y z), hasZ, [curZ], hasS, [curS], tflg, tc, deg, dir, E.
// `out` receives the status (0 = ok, else NL_ERROR.eno, cleared before the call;
// 1000 if the flag was set without a number), then, on success only, the surface.
//
// sc_nlib_srf_adv_p6_fill(in, n_in, out, cap, out_len): `in` holds
//   k, curves[0..=k], hasDer, [has_i, curve] for i = 0..=k (only if hasDer),
//   eps, flg, CI (x y z), NI (x y z).
// `out` receives the status, then, on success only, the k+1 surfaces, CI and NI, and
// the curves and given derivatives after the call (upstream modifies them in place).
//
// A curve in `in` is: n p, then n+1 control points (x y z w), n+p+2 knots. All input
// lives on one NL_STACKS (upstream requires cur and der on the same stack). The Rust
// wrapper validates the inputs (well-formed clamped curves).
//
// A surface is dumped as n m p q r s, the control points (x y z w, row-major), the u
// and v knots; a curve as n p m, the control points, the knots.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

#include <vector>

#include "sc_nlib.h"

using sc::dump_crv;
using sc::dump_srf;
using sc::nl_status;

namespace
{

NL_CURVE* read_crv(const double* in, int64_t* at, NL_STACKS* S)
{
    const double* d = in + *at;
    NL_INDEX n = (NL_INDEX)d[0];
    NL_DEGREE p = (NL_DEGREE)d[1];
    NL_INDEX m = n + p + 1;
    NL_CURVE* cur = N_AllocCrvAndArrays(n, p, m, S);
    if (cur == NULL)
        return NULL;
    const double* c = d + 2;
    NL_CPOINT* Pw;
    NL_REAL* U;
    N_CrvGetCPtsAndKnots(cur, &Pw, &U);
    for (NL_INDEX i = 0; i <= n; i++)
    {
        Pw[i].x = c[0];
        Pw[i].y = c[1];
        Pw[i].z = c[2];
        Pw[i].w = c[3];
        c += 4;
    }
    for (NL_INDEX i = 0; i <= m; i++)
        U[i] = *c++;
    *at = (int64_t)(c - in);
    return cur;
}

} // namespace

SC_EXPORT int32_t sc_nlib_srf_adv_p6_sweep(const double* in, int64_t n_in, double* out, int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int64_t at = 0;
    int32_t rc = SC_OK;
    N_ErrClear();
    do
    {
        NL_CURVE* curT = read_crv(in, &at, &S);
        NL_CURVE* curC = read_crv(in, &at, &S);
        if (curT == NULL || curC == NULL)
        {
            rc = SC_ERR_ARG;
            break;
        }
        NL_VECTOR Z0;
        Z0.x = in[at++];
        Z0.y = in[at++];
        Z0.z = in[at++];
        NL_CURVE* curZ = NULL;
        if (in[at++] != 0.0)
        {
            curZ = read_crv(in, &at, &S);
            if (curZ == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
        }
        NL_CURVE* curS = NULL;
        if (in[at++] != 0.0)
        {
            curS = read_crv(in, &at, &S);
            if (curS == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
        }
        NL_FLAG tflg = in[at++] != 0.0 ? NL_YES : NL_NO;
        NL_PARAMETER tc = in[at++];
        NL_DEGREE deg = (NL_DEGREE)in[at++];
        NL_FLAG dir = (NL_FLAG)in[at++];
        NL_REAL E = in[at++];
        NL_SURFACE* sur = N_AllocSrf(&S);
        if (sur == NULL)
        {
            rc = SC_ERR_ARG;
            break;
        }
        N_SrfInitArrays(sur);
        NL_FLAG err = N_CreateSweepScale(curT, curC, Z0, curZ, curS, tflg, tc, deg, dir, E, sur, &S);
        o.push_back(nl_status(err));
        if (err == NL_NO)
            dump_srf(o, sur);
    } while (false);
    N_EndNurbs(&S);
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}

SC_EXPORT int32_t sc_nlib_srf_adv_p6_fill(const double* in, int64_t n_in, double* out, int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS SI, SS;
    N_InitNurbs(&SI);
    N_InitNurbs(&SS);
    std::vector<double> o;
    int64_t at = 0;
    int32_t rc = SC_OK;
    N_ErrClear();
    do
    {
        NL_INDEX k = (NL_INDEX)in[at++];
        std::vector<NL_CURVE*> cur((size_t)k + 1, NULL);
        for (NL_INDEX i = 0; i <= k && rc == SC_OK; i++)
        {
            cur[(size_t)i] = read_crv(in, &at, &SI);
            if (cur[(size_t)i] == NULL)
                rc = SC_ERR_ARG;
        }
        if (rc != SC_OK)
            break;
        bool has_der = in[at++] != 0.0;
        std::vector<NL_CURVE*> der((size_t)k + 1, NULL);
        if (has_der)
            for (NL_INDEX i = 0; i <= k && rc == SC_OK; i++)
                if (in[at++] != 0.0)
                {
                    der[(size_t)i] = read_crv(in, &at, &SI);
                    if (der[(size_t)i] == NULL)
                        rc = SC_ERR_ARG;
                }
        if (rc != SC_OK)
            break;
        NL_REAL eps = in[at++];
        NL_FLAG flg = in[at++] != 0.0 ? NL_YES : NL_NO;
        NL_POINT CI;
        NL_VECTOR NI;
        CI.x = in[at++];
        CI.y = in[at++];
        CI.z = in[at++];
        NI.x = in[at++];
        NI.y = in[at++];
        NI.z = in[at++];
        NL_SURFACE** sur = NULL;
        NL_FLAG err = N_FillNSidedHole(cur.data(), has_der ? der.data() : NULL, k, eps, &CI, &NI, flg, &sur, &SI, &SS);
        o.push_back(nl_status(err));
        if (err == NL_NO)
        {
            for (NL_INDEX i = 0; i <= k; i++)
                dump_srf(o, sur[i]);
            o.push_back(CI.x);
            o.push_back(CI.y);
            o.push_back(CI.z);
            o.push_back(NI.x);
            o.push_back(NI.y);
            o.push_back(NI.z);
            for (NL_INDEX i = 0; i <= k; i++)
                dump_crv(o, cur[(size_t)i]);
            if (has_der)
                for (NL_INDEX i = 0; i <= k; i++)
                {
                    o.push_back(der[(size_t)i] != NULL ? 1.0 : 0.0);
                    if (der[(size_t)i] != NULL)
                        dump_crv(o, der[(size_t)i]);
                }
        }
    } while (false);
    N_EndNurbs(&SS);
    N_EndNurbs(&SI);
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
