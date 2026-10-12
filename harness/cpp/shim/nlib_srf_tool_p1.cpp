// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.srf_tool_p1`.
// Wraps N_SrfInsertKnot, N_SrfSplit, N_SrfDecomposeToBez, N_SrfExtractIsoCrv,
// N_SrfInsertKnots and N_SrfExtractPatch (upstream source/SMLib/src/NL_SrfTool.cpp).
//
// One entry point, sc_nlib_srf_tool_p1_eval(op, in, n_in, out, cap, out_len): `op`
// selects the routine, `in` holds its inputs, `out` receives a flat record of
// doubles: the status (0 = ok, else NL_ERROR.eno, cleared before the call; 1000 if
// the flag was set without a number), then, on success only, the results.
//
// A surface in `in` is: n m p q, then (n+1)*(m+1) control points (x y z w,
// row-major), n+p+2 u knots, m+q+2 v knots (storage = indices in use). Everything is
// allocated on one NL_STACKS. The Rust wrapper validates the surface (valid clamped
// knot vectors, degrees 1..=NL_MAXDEG) and the parameters (finite, sorted refinement
// knots) so upstream only touches the storage it allocates.
//
// A surface is dumped as n m p q r s, the control points, the u and v knots; a
// curve as n p m, the control points, the knots.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

#include <vector>

namespace
{
double nl_status(NL_FLAG err)
{
    if (err == NL_NO)
        return 0.0;
    return NL_ERROR.eno != 0 ? (double)NL_ERROR.eno : 1000.0;
}

// Reads one surface at in[*at], allocating it on S; advances *at.
NL_SURFACE* read_srf(const double* in, int64_t* at, NL_STACKS* S)
{
    const double* d = in + *at;
    NL_INDEX n = (NL_INDEX)d[0], m = (NL_INDEX)d[1];
    NL_DEGREE p = (NL_DEGREE)d[2], q = (NL_DEGREE)d[3];
    NL_INDEX r = n + p + 1, s = m + q + 1;
    NL_SURFACE* sur = N_AllocSrfAndArrays(n, m, p, q, r, s, S);
    if (sur == NULL)
        return NULL;
    const double* c = d + 4;
    for (NL_INDEX i = 0; i <= n; i++)
        for (NL_INDEX j = 0; j <= m; j++)
        {
            NL_CPOINT& pw = sur->net->Pw[i][j];
            pw.x = c[0];
            pw.y = c[1];
            pw.z = c[2];
            pw.w = c[3];
            c += 4;
        }
    for (NL_INDEX i = 0; i <= r; i++)
        sur->knu->U[i] = *c++;
    for (NL_INDEX i = 0; i <= s; i++)
        sur->knv->U[i] = *c++;
    *at = (int64_t)(c - in);
    return sur;
}

NL_SURFACE* null_srf(NL_STACKS* S)
{
    NL_SURFACE* sur = N_AllocSrf(S);
    if (sur != NULL)
        N_SrfInitArrays(sur);
    return sur;
}

void dump_srf(std::vector<double>& o, NL_SURFACE* sur)
{
    NL_INDEX n, m, r, s;
    NL_DEGREE p, q;
    NL_CPOINT** Pw;
    NL_REAL *U, *V;
    N_SrfGetCPtsDegreesAndKnots(sur, &n, &m, &Pw, &p, &q, &r, &s, &U, &V);
    o.push_back((double)n);
    o.push_back((double)m);
    o.push_back((double)p);
    o.push_back((double)q);
    o.push_back((double)r);
    o.push_back((double)s);
    for (NL_INDEX i = 0; i <= n; i++)
        for (NL_INDEX j = 0; j <= m; j++)
        {
            o.push_back(Pw[i][j].x);
            o.push_back(Pw[i][j].y);
            o.push_back(Pw[i][j].z);
            o.push_back(Pw[i][j].w);
        }
    for (NL_INDEX i = 0; i <= r; i++)
        o.push_back(U[i]);
    for (NL_INDEX j = 0; j <= s; j++)
        o.push_back(V[j]);
}

void dump_crv(std::vector<double>& o, NL_CURVE* cur)
{
    NL_INDEX n, m;
    NL_DEGREE p;
    NL_CPOINT* Pw;
    NL_REAL* U;
    N_CrvGetCPtsDegreeAndKnots(cur, &n, &Pw, &p, &m, &U);
    o.push_back((double)n);
    o.push_back((double)p);
    o.push_back((double)m);
    for (NL_INDEX i = 0; i <= n; i++)
    {
        o.push_back(Pw[i].x);
        o.push_back(Pw[i].y);
        o.push_back(Pw[i].z);
        o.push_back(Pw[i].w);
    }
    for (NL_INDEX i = 0; i <= m; i++)
        o.push_back(U[i]);
}
} // namespace

// op 0 insert knot: surface, t mt dir inplace -> status, surface.
// op 1 split: surface, t dir -> status, left, right.
// op 2 decompose: surface -> status, kk ll, (kk+1)*(ll+1) surfaces row-major.
// op 3 iso-curve: surface, t dir -> status, curve.
// op 4 insert knots: surface, dir inplace k X[0..k) -> status, surface.
// op 5 extract patch: surface, ul ur vl vr inplace -> status, surface.
SC_EXPORT int32_t sc_nlib_srf_tool_p1_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                           int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int64_t at = 0;
    int32_t rc = SC_OK;
    NL_SURFACE* sur = read_srf(in, &at, &S);
    const double* prm = in + at;
    if (sur == NULL)
        rc = SC_ERR_ARG;
    NL_FLAG err = NL_NO;
    N_ErrClear();
    switch (rc == SC_OK ? op : -1)
    {
        case 0:
        {
            bool inplace = prm[3] != 0.0;
            NL_SURFACE* res = inplace ? sur : null_srf(&S);
            if (res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            err = N_SrfInsertKnot(sur, prm[0], (NL_INDEX)prm[1], (NL_FLAG)prm[2], res, &S, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_srf(o, res);
            break;
        }
        case 1:
        {
            NL_SURFACE* L = null_srf(&S);
            NL_SURFACE* R = null_srf(&S);
            if (L == NULL || R == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            err = N_SrfSplit(sur, prm[0], (NL_FLAG)prm[1], L, R, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                if (N_SrfAreArraysNULL(L) || N_SrfAreArraysNULL(R))
                {
                    rc = SC_ERR_ARG; // success without output (bad knot vector)
                    break;
                }
                dump_srf(o, L);
                dump_srf(o, R);
            }
            break;
        }
        case 2:
        {
            NL_SURFACE*** Q = NULL;
            NL_INDEX kk = -1, ll = -1;
            err = N_SrfDecomposeToBez(sur, &Q, &kk, &ll, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                o.push_back((double)kk);
                o.push_back((double)ll);
                for (NL_INDEX i = 0; i <= kk; i++)
                    for (NL_INDEX j = 0; j <= ll; j++)
                        dump_srf(o, Q[i][j]);
            }
            break;
        }
        case 3:
        {
            NL_CURVE* cur = N_AllocCrv(&S);
            if (cur == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            N_CrvInitArrays(cur);
            err = N_SrfExtractIsoCrv(sur, prm[0], (NL_FLAG)prm[1], cur, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_crv(o, cur);
            break;
        }
        case 4:
        {
            NL_FLAG dir = (NL_FLAG)prm[0];
            bool inplace = prm[1] != 0.0;
            int64_t k = (int64_t)prm[2];
            std::vector<NL_REAL> X(prm + 3, prm + 3 + k);
            X.push_back(0.0); // never read when k == 0 (xr = -1)
            NL_KNOTVECTOR knx;
            N_KnotVectorFromRealArray(&knx, X.data(), (NL_INDEX)(k - 1));
            NL_SURFACE* res = inplace ? sur : null_srf(&S);
            if (res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            err = N_SrfInsertKnots(sur, &knx, dir, res, &S, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_srf(o, res);
            break;
        }
        case 5:
        {
            bool inplace = prm[4] != 0.0;
            NL_SURFACE* res = inplace ? sur : null_srf(&S);
            if (res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            err = N_SrfExtractPatch(sur, prm[0], prm[1], prm[2], prm[3], res, &S, &S);
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
