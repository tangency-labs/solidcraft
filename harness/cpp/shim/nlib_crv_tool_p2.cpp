// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.crv_tool_p2`.
// Wraps the NL_CURVE tools of upstream source/SMLib/src/NL_CrvTool.cpp owned by the unit:
// N_CrvRemoveKnots, N_CrvReduceDegreeOnce, N_CrvReduceDegree, N_CrvsMakeCompatible,
// N_CrvsMakeCompatibleKnotTol, N_CrvsMakeCompatibleAdjKnots, N_CrvRemoveDegenSegs.
//
// One entry point, sc_nlib_crv_tool_p2_eval(op, in, n_in, out, cap, out_len): `op`
// selects the routine, `in` holds the curve(s) then the routine's parameters, `out`
// receives a flat record of doubles (layouts in crates/difftest/src/units/nlib_crv_tool_p2.rs).
// The record starts with the routine's status: 0 = ok, else NL_ERROR.eno (cleared
// before the call; 1000 if the flag was set without a number).
//
// A curve in `in` is: n p m, then n+1 control points (x y z w), m+1 knots. The curve-set
// routines take a count first, then that many curves. Everything is allocated on one
// NL_STACKS, so the in-place variants (output curve == input curve) can free the input's
// arrays. The Rust wrapper validates the curves (clamped, well-formed) and parameters.
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

// Reads one curve at in[*at], allocating it on S; advances *at.
NL_CURVE* read_crv(const double* in, int64_t* at, NL_STACKS* S)
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

// n p m, used control points, used knots (through N_CrvGetCPtsDegreeAndKnots).
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

NL_CURVE* null_crv(NL_STACKS* S)
{
    NL_CURVE* cur = N_AllocCrv(S);
    if (cur != NULL)
        N_CrvInitArrays(cur);
    return cur;
}

// Output curve: the input itself (in place) or a new NULL curve.
NL_CURVE* out_crv(NL_CURVE* cur, double inplace, NL_STACKS* S)
{
    return inplace != 0.0 ? cur : null_crv(S);
}

// Curve-set ops: count, curves, then one parameter. Status, then every curve (also
// after an error: the routines work in place and keep their partial changes).
int32_t crvs_op(int32_t op, const double* in, std::vector<double>& o, NL_STACKS* S)
{
    int64_t at = 0;
    NL_INDEX cnt = (NL_INDEX)in[at++];
    std::vector<NL_CURVE*> cur((size_t)cnt, (NL_CURVE*)NULL);
    for (NL_INDEX i = 0; i < cnt; i++)
    {
        cur[(size_t)i] = read_crv(in, &at, S);
        if (cur[(size_t)i] == NULL)
            return SC_ERR_ARG;
    }
    double prm = in[at];
    NL_FLAG err = NL_NO;
    switch (op)
    {
        case 3:
            err = N_CrvsMakeCompatible(cur.data(), cnt - 1, S);
            break;
        case 4:
            err = N_CrvsMakeCompatibleKnotTol(cur.data(), cnt - 1, prm, S);
            break;
        case 5:
            err = N_CrvsMakeCompatibleAdjKnots(cur.data(), cnt - 1, prm, S);
            break;
        default:
            return SC_ERR_ARG;
    }
    o.push_back(nl_status(err));
    for (NL_INDEX i = 0; i < cnt; i++)
        dump_crv(o, cur[(size_t)i]);
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_nlib_crv_tool_p2_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                           int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int32_t rc = SC_OK;
    N_ErrClear();
    if (op >= 3 && op <= 5)
    {
        rc = crvs_op(op, in, o, &S);
    }
    else
    {
        int64_t at = 0;
        NL_CURVE* cur = read_crv(in, &at, &S);
        const double* prm = in + at;
        if (cur == NULL)
        {
            rc = SC_ERR_ARG;
        }
        else
        {
            N_ErrClear();
            switch (op)
            {
                case 0: // N_CrvRemoveKnots: tol inplace
                {
                    NL_CURVE* q = out_crv(cur, prm[1], &S);
                    NL_FLAG err = N_CrvRemoveKnots(cur, prm[0], q, &S);
                    o.push_back(nl_status(err));
                    if (err == NL_NO)
                        dump_crv(o, q);
                    break;
                }
                case 1: // N_CrvReduceDegreeOnce: tol inplace
                {
                    NL_CURVE* q = out_crv(cur, prm[1], &S);
                    NL_FLAG rfl = NL_NO;
                    NL_REAL mtol = 0.0;
                    NL_FLAG err = N_CrvReduceDegreeOnce(cur, prm[0], &rfl, q, &mtol, &S, &S);
                    o.push_back(nl_status(err));
                    if (err == NL_NO)
                    {
                        o.push_back(rfl == NL_YES ? 1.0 : 0.0);
                        o.push_back(mtol);
                        if (rfl == NL_YES)
                            dump_crv(o, q);
                    }
                    break;
                }
                case 2: // N_CrvReduceDegree: tol inplace
                {
                    NL_CURVE* q = out_crv(cur, prm[1], &S);
                    NL_FLAG err = N_CrvReduceDegree(cur, prm[0], q, &S, &S);
                    o.push_back(nl_status(err));
                    if (err == NL_NO)
                        dump_crv(o, q);
                    break;
                }
                case 6: // N_CrvRemoveDegenSegs: tol inplace
                {
                    NL_CURVE* q = out_crv(cur, prm[1], &S);
                    NL_INDEX nr = -1;
                    NL_FLAG err = N_CrvRemoveDegenSegs(cur, prm[0], &nr, q, &S, &S);
                    o.push_back(nl_status(err));
                    if (err == NL_NO)
                    {
                        o.push_back((double)nr);
                        if (nr > 0)
                            dump_crv(o, q);
                    }
                    break;
                }
                default:
                    rc = SC_ERR_ARG;
                    break;
            }
        }
    }
    N_EndNurbs(&S);
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
