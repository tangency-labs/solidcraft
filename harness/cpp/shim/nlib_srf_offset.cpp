// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.srf_offset`.
// Wraps upstream N_SrfOffset (source/SMLib/src/NL_SrfOffset.cpp).
//
// sc_nlib_srf_offset_eval(in, n_in, out, cap, out_len): `in` holds a surface (n m p q,
// (n+1)*(m+1) control points x y z w row-major, n+p+2 u knots, m+q+2 v knots) then
// d pp qq par eps tol. `out` receives the status (0 = ok, else NL_ERROR.eno, cleared
// before the call; 1000 if the flag was set without a number), then on success the
// offset surface (n m p q r s, used control points, used knots).
//
// N_SrfOffset does not check N_CrvEvalTangent on the generator curve of a surface of
// revolution or an extrusion and then reads an unset vector. Before the call the shim
// runs N_SrfType exactly as N_SrfOffset does and returns SC_ERR_ARG when that tangent
// cannot be evaluated. The Rust wrapper validates the surface and the parameters
// (crates/solidcore-ffi/src/units/nlib_srf_offset.rs).
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

// True when N_SrfOffset would read an unset generator tangent.
bool unset_tangent(NL_SURFACE* sur, NL_REAL eps)
{
    NL_STACKS T;
    N_InitNurbs(&T);
    NL_CURVE *curA = NULL, *curB = NULL;
    NL_POINT AA, BB, CC, DD;
    NL_VECTOR V1, V2;
    NL_REAL r1, r2, ac, al, h;
    NL_FLAG stp = 0, dir = 0;
    bool bad = false;
    NL_FLAG err = N_SrfType(sur, eps, &curA, &curB, &AA, &BB, &CC, &DD, &V1, &V2, &r1, &r2, &ac, &al, &h, &stp,
                            &dir, &T);
    if (err == NL_NO)
    {
        bool rev = stp == NL_NSPHERE || stp == NL_NTORUS || stp == NL_NCONE || stp == NL_NCYLINDER ||
                   stp == NL_NREVOLUTION;
        bool flt = !N_PtsAreEqual(CC, NL_ZERO);
        if ((rev && flt) || stp == NL_NEXTRUSION)
        {
            if (curA == NULL)
                bad = true;
            else
            {
                NL_PARAMETER u0, u1;
                NL_POINT P;
                NL_VECTOR Ta;
                N_CrvGetParamBounds(curA, &u0, &u1);
                if (N_CrvEvalTangent(curA, u0, NL_RIGHT, &P, &Ta) != NL_NO)
                    bad = true;
            }
        }
    }
    N_EndNurbs(&T);
    N_ErrClear();
    return bad;
}

} // namespace

SC_EXPORT int32_t sc_nlib_srf_offset_eval(const double* in, int64_t n_in, double* out, int64_t cap,
                                          int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int64_t at = 0;
    int32_t rc = SC_OK;
    N_ErrClear();
    NL_SURFACE* sur = read_srf(in, &at, &S);
    NL_SURFACE* res = N_AllocSrf(&S);
    if (sur == NULL || res == NULL)
        rc = SC_ERR_ARG;
    else
    {
        N_SrfInitArrays(res);
        NL_REAL d = in[at++];
        NL_DEGREE pp = (NL_DEGREE)in[at++];
        NL_DEGREE qq = (NL_DEGREE)in[at++];
        NL_FLAG par = (NL_FLAG)in[at++];
        NL_REAL eps = in[at++];
        NL_REAL tol = in[at++];
        if (unset_tangent(sur, eps))
            rc = SC_ERR_ARG;
        else
        {
            N_ErrClear();
            NL_FLAG err = N_SrfOffset(sur, d, pp, qq, par, eps, tol, res, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_srf(o, res);
        }
    }
    N_EndNurbs(&S);
    N_ErrClear();
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
