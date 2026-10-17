// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.srf_adv_p1`.
// Wraps N_SrfExtendByDist, N_SrfOffsetGetMaxDeriv, N_SrfEvalPtDerivsUnbounded,
// N_CreateSkinSpine and N_CrossBoundaryDerivsVectorField (upstream
// source/SMLib/src/NL_SrfAdv.cpp).
//
// One entry point, sc_nlib_srf_adv_p1_eval(op, in, n_in, out, cap, out_len): `op`
// selects the routine, `in` holds its inputs, `out` receives a flat record of
// doubles: the status (0 = ok, else NL_ERROR.eno, cleared before the call; 1000 if
// the flag was set without a number), then, on success only, the results.
//
// A surface in `in` is: n m p q, then (n+1)*(m+1) control points (x y z w,
// row-major), n+p+2 u knots, m+q+2 v knots. A curve is: n p, then n+1 control points
// (x y z w), n+p+2 knots. Everything is allocated on one NL_STACKS. The Rust wrapper
// validates the inputs (valid clamped knot vectors, degrees, parameter ranges) so
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

// op 0 extend by distance: surface, dist dflg end cont inplace -> status, surface.
// op 1 offset max derivatives: surface, d -> status, Muu Muv Mvv.
// op 2 unbounded evaluation: surface, cflg ndr cnt, cnt (u v) pairs -> for each pair
//      until the first failure: status, then D[i][j] (xyz, i + j <= ndr, i outer; the
//      point for ndr = 0); then, if every evaluation succeeded: has_esur, gflg and
//      esur (if any).
// op 3 skin along spine: k, curves[0..=k], spine curve, params[0..=k], cmp align
//      z0x z0y z0z subdiv dflag deg dir -> status, surface, curves.
// op 4 cross-boundary vector field: curve, vector field curve, bndy -> status, surface.
SC_EXPORT int32_t sc_nlib_srf_adv_p1_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                          int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int64_t at = 0;
    int32_t rc = SC_OK;
    NL_FLAG err = NL_NO;
    N_ErrClear();
    switch (op)
    {
        case 0:
        {
            NL_SURFACE* sur = read_srf(in, &at, &S);
            if (sur == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            const double* prm = in + at;
            bool inplace = prm[4] != 0.0;
            NL_SURFACE* res = inplace ? sur : null_srf(&S);
            if (res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            err = N_SrfExtendByDist(sur, prm[0], (NL_FLAG)prm[1], (NL_FLAG)prm[2], (NL_FLAG)prm[3], res, &S, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_srf(o, res);
            break;
        }
        case 1:
        {
            NL_SURFACE* sur = read_srf(in, &at, &S);
            if (sur == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_REAL muu = 0.0, muv = 0.0, mvv = 0.0;
            err = N_SrfOffsetGetMaxDeriv(sur, in[at], &muu, &muv, &mvv);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                o.push_back(muu);
                o.push_back(muv);
                o.push_back(mvv);
            }
            break;
        }
        case 2:
        {
            NL_SURFACE* sur = read_srf(in, &at, &S);
            NL_SURFACE* esur = null_srf(&S);
            if (sur == NULL || esur == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_FLAG cflg = (NL_FLAG)in[at++];
            NL_INDEX ndr = (NL_INDEX)in[at++];
            NL_INDEX cnt = (NL_INDEX)in[at++];
            NL_POINT** D = N_AllocPt2dArray(ndr, ndr, &S);
            if (D == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_FLAG gflg = NL_NO;
            bool all_ok = true;
            for (NL_INDEX e = 0; e < cnt; e++)
            {
                NL_POINT P = {0.0, 0.0, 0.0};
                N_ErrClear();
                err = N_SrfEvalPtDerivsUnbounded(sur, in[at], in[at + 1], ndr, cflg, esur, &gflg, &P, D, &S);
                at += 2;
                o.push_back(nl_status(err));
                if (err != NL_NO)
                {
                    all_ok = false;
                    break;
                }
                if (ndr == 0)
                {
                    o.push_back(P.x);
                    o.push_back(P.y);
                    o.push_back(P.z);
                }
                else
                    for (NL_INDEX i = 0; i <= ndr; i++)
                        for (NL_INDEX j = 0; i + j <= ndr; j++)
                        {
                            o.push_back(D[i][j].x);
                            o.push_back(D[i][j].y);
                            o.push_back(D[i][j].z);
                        }
            }
            if (all_ok)
            {
                bool has = !N_SrfAreArraysNULL(esur);
                o.push_back(has ? 1.0 : 0.0);
                if (has)
                {
                    o.push_back((double)gflg);
                    dump_srf(o, esur);
                }
            }
            break;
        }
        case 3:
        {
            NL_INDEX k = (NL_INDEX)in[at++];
            std::vector<NL_CURVE*> cur((size_t)k + 1, NULL);
            for (NL_INDEX i = 0; i <= k && rc == SC_OK; i++)
            {
                cur[(size_t)i] = read_crv(in, &at, &S);
                if (cur[(size_t)i] == NULL)
                    rc = SC_ERR_ARG;
            }
            NL_CURVE* spine = rc == SC_OK ? read_crv(in, &at, &S) : NULL;
            NL_SURFACE* res = null_srf(&S);
            if (rc != SC_OK || spine == NULL || res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            std::vector<NL_PARAMETER> prm(in + at, in + at + k + 1);
            at += k + 1;
            const double* f = in + at;
            NL_VECTOR z0 = {f[2], f[3], f[4]};
            err = N_CreateSkinSpine(cur.data(), k, f[0] != 0.0 ? NL_YES : NL_NO, spine, prm.data(),
                                    f[1] != 0.0 ? NL_YES : NL_NO, z0, (NL_INDEX)f[5], (NL_FLAG)f[6],
                                    (NL_DEGREE)f[7], (NL_FLAG)f[8], res, &S, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                dump_srf(o, res);
                for (NL_INDEX i = 0; i <= k; i++)
                    dump_crv(o, cur[(size_t)i]);
            }
            break;
        }
        case 4:
        {
            NL_CURVE* c1 = read_crv(in, &at, &S);
            NL_CURVE* c2 = c1 == NULL ? NULL : read_crv(in, &at, &S);
            NL_SURFACE* res = null_srf(&S);
            if (c2 == NULL || res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            err = N_CrossBoundaryDerivsVectorField(c1, c2, (NL_FLAG)in[at], res, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_srf(o, res);
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
