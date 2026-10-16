// SPDX-License-Identifier: Apache-2.0
// Helpers over NLib C structures (NL_CURVE, NL_SURFACE, NL_ERROR).
//
// Copied verbatim from the per-shim copies they replace (only `inline` and the
// `sc` namespace were added), so behaviour is unchanged. A shim that needs a
// different variant keeps its own copy and does not include this header.
// Shims pull a helper in with `using sc::<name>;` after their includes.
#ifndef SC_NLIB_H
#define SC_NLIB_H

#include <cstdint>
#include <vector>

#include <nurbs.h>
// After nurbs.h, which defines NL_PUBLIC.
#include <NL_Globals.h>

namespace sc
{

// An NLib result as an output value: 0 on success, else NL_ERROR.eno (1000 if unset).
inline double nl_status(NL_FLAG err)
{
    if (err == NL_NO)
        return 0.0;
    return NL_ERROR.eno != 0 ? (double)NL_ERROR.eno : 1000.0;
}

// An empty curve allocated on S.
inline NL_CURVE* null_crv(NL_STACKS* S)
{
    NL_CURVE* cur = N_AllocCrv(S);
    if (cur != NULL)
        N_CrvInitArrays(cur);
    return cur;
}

// An empty surface allocated on S.
inline NL_SURFACE* null_srf(NL_STACKS* S)
{
    NL_SURFACE* sur = N_AllocSrf(S);
    if (sur != NULL)
        N_SrfInitArrays(sur);
    return sur;
}

// n p m, used control points, used knots (through N_CrvGetCPtsDegreeAndKnots).
inline void dump_crv(std::vector<double>& o, NL_CURVE* cur)
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

// n m p q r s, used control points, used knots (through the upstream accessors).
inline void dump_srf(std::vector<double>& o, NL_SURFACE* sur)
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

// Reads one curve at in[*at], allocating it on S; advances *at.
inline NL_CURVE* read_crv(const double* in, int64_t* at, NL_STACKS* S)
{
    const double* d = in + *at;
    NL_INDEX n = (NL_INDEX)d[0];
    NL_DEGREE p = (NL_DEGREE)d[1];
    NL_INDEX m = (NL_INDEX)d[2];
    NL_CURVE* cur = N_AllocCrvAndArrays(n, p, m, S);
    if (cur == NULL)
        return NULL;
    const double* c = d + 3;
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

// Reads one surface at in[*at], allocating it on S; advances *at.
inline NL_SURFACE* read_srf(const double* in, int64_t* at, NL_STACKS* S)
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

} // namespace sc

#endif // SC_NLIB_H
