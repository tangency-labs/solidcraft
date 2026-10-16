// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.srf_adv_p5`.
// Wraps N_CreateSkinSrfBoundaryContinuity (upstream source/SMLib/src/NL_SrfAdv.cpp).
//
// sc_nlib_srf_adv_p5_skin(in, n_in, out, cap, out_len): `in` holds
//   k, curves[0..=k],
//   hasDer, [has0, surface], [has1, surface]   (the bracketed parts only if hasDer),
//   cmp, cnt0, cnt1, bnd0, bnd1,
//   hasRail, [has0, curve], [has1, curve]      (only if hasRail),
//   deg, ES, dir,
//   hasTs, [ts[0..=k]], hasKns, [count, knots].
// `out` receives the status (0 = ok, else NL_ERROR.eno, cleared before the call;
// 1000 if the flag was set without a number), then, on success only, the surface,
// every cross section, the derivative surfaces and the rails after the call (inputs
// upstream modifies in place), each optional one preceded by a presence flag.
//
// A surface in `in` is: n m p q, then (n+1)*(m+1) control points (x y z w,
// row-major), n+p+2 u knots, m+q+2 v knots. A curve is: n p, then n+1 control points
// (x y z w), n+p+2 knots. Everything is allocated on one NL_STACKS (upstream requires
// all input data on the same stack). The Rust wrapper validates the inputs so that
// upstream only touches the storage it allocates.
//
// A surface is dumped as n m p q r s, the control points, the u and v knots; a curve
// as n p m, the control points, the knots.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

#include <vector>

#include "sc_nlib.h"

using sc::dump_crv;
using sc::dump_srf;
using sc::nl_status;
using sc::null_srf;
using sc::read_srf;

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

SC_EXPORT int32_t sc_nlib_srf_adv_p5_skin(const double* in, int64_t n_in, double* out, int64_t cap, int64_t* out_len)
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
        NL_INDEX k = (NL_INDEX)in[at++];
        std::vector<NL_CURVE*> cur((size_t)k + 1, NULL);
        for (NL_INDEX i = 0; i <= k && rc == SC_OK; i++)
        {
            cur[(size_t)i] = read_crv(in, &at, &S);
            if (cur[(size_t)i] == NULL)
                rc = SC_ERR_ARG;
        }
        if (rc != SC_OK)
            break;
        bool has_der = in[at++] != 0.0;
        NL_SURFACE* der[2] = { NULL, NULL };
        if (has_der)
            for (int i = 0; i < 2 && rc == SC_OK; i++)
                if (in[at++] != 0.0)
                {
                    der[i] = read_srf(in, &at, &S);
                    if (der[i] == NULL)
                        rc = SC_ERR_ARG;
                }
        if (rc != SC_OK)
            break;
        NL_FLAG cmp = in[at++] != 0.0 ? NL_YES : NL_NO;
        NL_FLAG cnt[2], bnd[2];
        cnt[0] = (NL_FLAG)in[at++];
        cnt[1] = (NL_FLAG)in[at++];
        bnd[0] = (NL_FLAG)in[at++];
        bnd[1] = (NL_FLAG)in[at++];
        bool has_rail = in[at++] != 0.0;
        NL_CURVE* rail[2] = { NULL, NULL };
        if (has_rail)
            for (int i = 0; i < 2 && rc == SC_OK; i++)
                if (in[at++] != 0.0)
                {
                    rail[i] = read_crv(in, &at, &S);
                    if (rail[i] == NULL)
                        rc = SC_ERR_ARG;
                }
        if (rc != SC_OK)
            break;
        NL_DEGREE deg = (NL_DEGREE)in[at++];
        NL_REAL es = in[at++];
        NL_FLAG dir = (NL_FLAG)in[at++];
        std::vector<NL_PARAMETER> ts;
        bool has_ts = in[at++] != 0.0;
        if (has_ts)
        {
            ts.assign(in + at, in + at + k + 1);
            at += k + 1;
        }
        NL_KNOTVECTOR* kns = NULL;
        if (in[at++] != 0.0)
        {
            NL_INDEX cnt_k = (NL_INDEX)in[at++];
            kns = N_AllocKnotVectorAndArray(cnt_k - 1, &S);
            if (kns == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_INDEX mk;
            NL_REAL* U;
            N_KnotVectorGetKnots(kns, &mk, &U);
            for (NL_INDEX i = 0; i < cnt_k; i++)
                U[i] = in[at++];
        }
        NL_SURFACE* res = null_srf(&S);
        if (res == NULL)
        {
            rc = SC_ERR_ARG;
            break;
        }
        NL_FLAG err = N_CreateSkinSrfBoundaryContinuity(cur.data(), k, has_der ? der : NULL, cmp, cnt, bnd,
                                                        has_rail ? rail : NULL, deg, es, dir,
                                                        has_ts ? ts.data() : NULL, kns, res, &S, &S);
        o.push_back(nl_status(err));
        if (err == NL_NO)
        {
            dump_srf(o, res);
            for (NL_INDEX i = 0; i <= k; i++)
                dump_crv(o, cur[(size_t)i]);
            if (has_der)
                for (int i = 0; i < 2; i++)
                {
                    o.push_back(der[i] != NULL ? 1.0 : 0.0);
                    if (der[i] != NULL)
                        dump_srf(o, der[i]);
                }
            if (has_rail)
                for (int i = 0; i < 2; i++)
                {
                    o.push_back(rail[i] != NULL ? 1.0 : 0.0);
                    if (rail[i] != NULL)
                        dump_crv(o, rail[i]);
                }
        }
    } while (false);
    N_EndNurbs(&S);
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
