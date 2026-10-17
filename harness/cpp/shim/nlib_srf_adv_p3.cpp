// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.srf_adv_p3`.
// Wraps N_SrfGetGridPtsNormals, N_SrfExtendByParamDist, N_CreateSwungSrf,
// N_CreateSkinSrf and N_CreateTransSweepSrf (upstream source/SMLib/src/NL_SrfAdv.cpp).
//
// One entry point, sc_nlib_srf_adv_p3_eval(op, in, n_in, out, cap, out_len): `op`
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

// op 0 grid: surface, nu, u[nu], nv, v[nv], ufl, vfl -> status, points, normals (xyz).
// op 1 extend: surface, par, dflg, end, cont, inplace -> status, surface.
// op 2 swung: curve P, curve T, alf -> status, surface.
// op 3 skin: k, curves[0..=k], cmp, cld, deg, dir, knotTol -> status, surface, curves.
// op 4 translational sweep: curve T, curve S -> status, surface.
SC_EXPORT int32_t sc_nlib_srf_adv_p3_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
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
            NL_INDEX nu = (NL_INDEX)in[at++];
            std::vector<NL_REAL> u(in + at, in + at + nu);
            at += nu;
            NL_INDEX nv = (NL_INDEX)in[at++];
            std::vector<NL_REAL> v(in + at, in + at + nv);
            at += nv;
            NL_FLAG ufl = (NL_FLAG)in[at++];
            NL_FLAG vfl = (NL_FLAG)in[at++];
            NL_POINT** P = N_AllocPt2dArray(nu - 1, nv - 1, &S);
            NL_POINT** N = N_AllocPt2dArray(nu - 1, nv - 1, &S);
            if (P == NULL || N == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            err = N_SrfGetGridPtsNormals(sur, u.data(), v.data(), nu - 1, nv - 1, ufl, vfl, P, N);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                for (NL_INDEX a = 0; a < nu; a++)
                    for (NL_INDEX b = 0; b < nv; b++)
                    {
                        o.push_back(P[a][b].x);
                        o.push_back(P[a][b].y);
                        o.push_back(P[a][b].z);
                    }
                for (NL_INDEX a = 0; a < nu; a++)
                    for (NL_INDEX b = 0; b < nv; b++)
                    {
                        o.push_back(N[a][b].x);
                        o.push_back(N[a][b].y);
                        o.push_back(N[a][b].z);
                    }
            }
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
            const double* prm = in + at;
            bool inplace = prm[4] != 0.0;
            NL_SURFACE* res = inplace ? sur : null_srf(&S);
            if (res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            err = N_SrfExtendByParamDist(sur, prm[0], (NL_FLAG)prm[1], (NL_FLAG)prm[2], (NL_FLAG)prm[3], res, &S, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_srf(o, res);
            break;
        }
        case 2:
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
            if (op == 2)
                err = N_CreateSwungSrf(c1, c2, in[at], res, &S);
            else
                err = N_CreateTransSweepSrf(c1, c2, res, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_srf(o, res);
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
            NL_SURFACE* res = null_srf(&S);
            if (rc != SC_OK || res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            const double* prm = in + at;
            err = N_CreateSkinSrf(cur.data(), k, prm[0] != 0.0 ? NL_YES : NL_NO, prm[1] != 0.0 ? NL_YES : NL_NO,
                                  (NL_DEGREE)prm[2], (NL_FLAG)prm[3], prm[4], res, &S, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                dump_srf(o, res);
                for (NL_INDEX i = 0; i <= k; i++)
                    dump_crv(o, cur[(size_t)i]);
            }
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
