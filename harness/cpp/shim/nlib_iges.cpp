// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.iges`.
// Wraps N_Iges126Crv, N_IgesClampCrv, N_IgesCorrectCrvKnots, N_IgesValidate,
// N_Iges128Srf, N_IgesClampSrf, N_IgesCorrectSrfKnots (source/SMLib/src/NL_Iges.cpp).
//
// One entry point, sc_nlib_iges_eval(op, in, n_in, out, cap, out_len). `out` is a flat
// record: the upstream status (0 = ok, else NL_ERROR.eno, cleared before the call;
// 1000 if the flag was set without a number), then on success the resulting curve
// (n p m, (n+1) control points x y z w, m+1 knots) or surface (n m p q r s, net
// row-major, r+1 u knots, s+1 v knots). Input layouts per op are below and in
// crates/solidcore-ffi/src/units/nlib_iges.rs, which validates them so upstream only
// touches the arrays it is given.
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
// p n b0 b1, (n+1) control points x y z w, n+p+2 knots -> curve on S.
bool read_crv(const double* in, NL_CURVE* cur, double* b, NL_STACKS* S)
{
    NL_DEGREE p = (NL_DEGREE)in[0];
    NL_INDEX n = (NL_INDEX)in[1];
    NL_INDEX m = n + p + 1;
    b[0] = in[2];
    b[1] = in[3];
    N_CrvInitArrays(cur);
    if (N_AllocCrvArrays(cur, n, p, m, S) != 0)
        return false;
    NL_CPOINT* Pw;
    NL_REAL* U;
    N_CrvGetCPtsAndKnots(cur, &Pw, &U);
    const double* c = in + 4;
    for (NL_INDEX i = 0; i <= n; i++)
    {
        N_CPtFromWxWyWz(c[0], c[1], c[2], c[3], &Pw[i]);
        c += 4;
    }
    for (NL_INDEX i = 0; i <= m; i++)
        U[i] = *c++;
    return true;
}

// p q n m ub0 ub1 vb0 vb1, net (n+1)x(m+1) row-major x y z w, n+p+2 u knots,
// m+q+2 v knots -> surface on S.
bool read_srf(const double* in, NL_SURFACE* sur, double* ub, double* vb, NL_STACKS* S)
{
    NL_DEGREE p = (NL_DEGREE)in[0], q = (NL_DEGREE)in[1];
    NL_INDEX n = (NL_INDEX)in[2], m = (NL_INDEX)in[3];
    NL_INDEX r = n + p + 1, s = m + q + 1;
    ub[0] = in[4];
    ub[1] = in[5];
    vb[0] = in[6];
    vb[1] = in[7];
    N_SrfInitArrays(sur);
    if (N_AllocSrfArrays(sur, n, m, p, q, r, s, S) != 0)
        return false;
    NL_CPOINT** Pw;
    NL_REAL *U, *V;
    N_SrfGetCPtsAndKnots(sur, &Pw, &U, &V);
    const double* c = in + 8;
    for (NL_INDEX i = 0; i <= n; i++)
        for (NL_INDEX j = 0; j <= m; j++)
        {
            N_CPtFromWxWyWz(c[0], c[1], c[2], c[3], &Pw[i][j]);
            c += 4;
        }
    for (NL_INDEX i = 0; i <= r; i++)
        U[i] = *c++;
    for (NL_INDEX j = 0; j <= s; j++)
        V[j] = *c++;
    return true;
}
} // namespace

// op 0 N_Iges126Crv: k m v0 v1, t[k+m+2], w[k+1], P[k+1] (x y z).
// op 1 N_IgesValidate: m p v0 v1, U[m+1] -> status only.
// op 2 N_Iges128Srf: k1 k2 m1 m2 u0 u1 v0 v1, s[k1+m1+2], t[k2+m2+2],
//      w[(k1+1)*(k2+1)] row-major (w[i][j]), P[(k1+1)*(k2+1)] (x y z) row-major.
// op 3 N_IgesClampCrv: curve (read_crv) with bounds.
// op 4 N_IgesCorrectCrvKnots: curve (read_crv), bounds ignored.
// op 5 N_IgesClampSrf: surface (read_srf) with bounds.
// op 6 N_IgesCorrectSrfKnots: surface (read_srf), bounds ignored.
SC_EXPORT int32_t sc_nlib_iges_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                    int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int32_t rc = SC_OK;
    NL_FLAG err;
    switch (op)
    {
        case 0:
        {
            NL_INDEX k = (NL_INDEX)in[0];
            NL_DEGREE m = (NL_DEGREE)in[1];
            NL_PARAMETER v[2] = {in[2], in[3]};
            NL_INDEX nt = k + (NL_INDEX)m + 2;
            std::vector<NL_REAL> t(in + 4, in + 4 + nt);
            std::vector<NL_REAL> w(in + 4 + nt, in + 4 + nt + k + 1);
            std::vector<NL_POINT> P((size_t)(k + 1));
            const double* c = in + 4 + nt + k + 1;
            for (NL_INDEX i = 0; i <= k; i++)
            {
                P[i].x = c[0];
                P[i].y = c[1];
                P[i].z = c[2];
                c += 3;
            }
            NL_CURVE cur;
            N_CrvInitArrays(&cur);
            N_ErrClear();
            err = N_Iges126Crv(k, m, t.data(), w.data(), P.data(), v, &cur, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_crv(o, &cur);
            break;
        }
        case 1:
        {
            NL_INDEX m = (NL_INDEX)in[0];
            NL_DEGREE p = (NL_DEGREE)in[1];
            NL_PARAMETER v[2] = {in[2], in[3]};
            std::vector<NL_REAL> U(in + 4, in + 4 + m + 1);
            N_ErrClear();
            err = N_IgesValidate(U.data(), m, p, v);
            o.push_back(nl_status(err));
            break;
        }
        case 2:
        {
            NL_INDEX k1 = (NL_INDEX)in[0], k2 = (NL_INDEX)in[1];
            NL_DEGREE m1 = (NL_DEGREE)in[2], m2 = (NL_DEGREE)in[3];
            NL_PARAMETER u[2] = {in[4], in[5]};
            NL_PARAMETER v[2] = {in[6], in[7]};
            NL_INDEX ns = k1 + (NL_INDEX)m1 + 2, nt = k2 + (NL_INDEX)m2 + 2;
            NL_INDEX np = (k1 + 1) * (k2 + 1);
            const double* c = in + 8;
            std::vector<NL_REAL> s(c, c + ns);
            c += ns;
            std::vector<NL_REAL> t(c, c + nt);
            c += nt;
            std::vector<NL_REAL> wf(c, c + np);
            c += np;
            std::vector<NL_POINT> Pf((size_t)np);
            for (NL_INDEX i = 0; i < np; i++)
            {
                Pf[i].x = c[0];
                Pf[i].y = c[1];
                Pf[i].z = c[2];
                c += 3;
            }
            std::vector<NL_REAL*> w((size_t)(k1 + 1));
            std::vector<NL_POINT*> P((size_t)(k1 + 1));
            for (NL_INDEX i = 0; i <= k1; i++)
            {
                w[i] = wf.data() + i * (k2 + 1);
                P[i] = Pf.data() + i * (k2 + 1);
            }
            NL_SURFACE sur;
            N_SrfInitArrays(&sur);
            N_ErrClear();
            err = N_Iges128Srf(k1, k2, m1, m2, s.data(), t.data(), w.data(), P.data(), u, v, &sur, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_srf(o, &sur);
            break;
        }
        case 3:
        case 4:
        {
            NL_CURVE cur;
            NL_PARAMETER b[2];
            if (!read_crv(in, &cur, b, &S))
            {
                rc = SC_ERR_ARG;
                break;
            }
            N_ErrClear();
            err = op == 3 ? N_IgesClampCrv(&cur, b, &S) : N_IgesCorrectCrvKnots(&cur, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_crv(o, &cur);
            break;
        }
        case 5:
        case 6:
        {
            NL_SURFACE sur;
            NL_PARAMETER ub[2], vb[2];
            if (!read_srf(in, &sur, ub, vb, &S))
            {
                rc = SC_ERR_ARG;
                break;
            }
            N_ErrClear();
            err = op == 5 ? N_IgesClampSrf(&sur, ub, vb, &S) : N_IgesCorrectSrfKnots(&sur, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_srf(o, &sur);
            break;
        }
        default:
            rc = SC_ERR_ARG;
            break;
    }
    N_EndNurbs(&S);
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
