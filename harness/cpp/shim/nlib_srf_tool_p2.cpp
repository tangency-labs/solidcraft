// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.srf_tool_p2`.
// Wraps N_SrfRemoveKnotConditional, N_SrfRemoveOneKnot and N_SrfElevateDegree
// (upstream source/SMLib/src/NL_SrfTool.cpp).
//
// One entry point, sc_nlib_srf_tool_p2_eval(op, in, n_in, out, cap, out_len): `op`
// selects the routine, `in` holds its inputs, `out` receives a flat record of
// doubles: the status (0 = ok, else NL_ERROR.eno, cleared before the call; 1000 if
// the flag was set without a number), then, on success only, the results.
//
// A surface in `in` is: n m p q, then (n+1)*(m+1) control points (x y z w,
// row-major), n+p+2 u knots, m+q+2 v knots (storage = indices in use). Everything is
// allocated on one NL_STACKS. The Rust wrapper validates the surface (valid clamped
// knot vectors, degrees 1..=NL_MAXDEG) and the parameters (finite; knot index,
// multiplicity and row range of N_SrfRemoveOneKnot inside the arrays) so upstream
// only touches the storage it allocates.
//
// A surface is dumped as n m p q r s, the control points, the u and v knots.
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

} // namespace

// op 0 conditional removal: surface, t nt tol dir inplace -> status, rt, surface.
// op 1 one-knot error bound: surface, r s f l dir mr -> status, mr.
// op 2 degree elevation: surface, t dir inplace -> status, surface.
SC_EXPORT int32_t sc_nlib_srf_tool_p2_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
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
            bool inplace = prm[4] != 0.0;
            NL_SURFACE* res = inplace ? sur : null_srf(&S);
            if (res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_INDEX rt = -1;
            err = N_SrfRemoveKnotConditional(sur, prm[0], (NL_INDEX)prm[1], prm[2], (NL_FLAG)prm[3], &rt, res, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                o.push_back((double)rt);
                dump_srf(o, res);
            }
            break;
        }
        case 1:
        {
            NL_REAL mr = prm[5];
            err = N_SrfRemoveOneKnot(sur, (NL_INDEX)prm[0], (NL_INDEX)prm[1], (NL_INDEX)prm[2], (NL_INDEX)prm[3],
                                     (NL_FLAG)prm[4], &mr);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                o.push_back(mr);
            break;
        }
        case 2:
        {
            bool inplace = prm[2] != 0.0;
            NL_SURFACE* res = inplace ? sur : null_srf(&S);
            if (res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            err = N_SrfElevateDegree(sur, (NL_INDEX)prm[0], (NL_FLAG)prm[1], res, &S, &S);
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
