// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.funcs_adv`.
// Wraps the NL_CFUN routines of upstream source/SMLib/src/NL_FuncsAdv.cpp owned by the
// unit: N_CrvFuncRefine, N_CrvFuncDeriv, N_CrvFuncMultiplyCrvFunc, N_CrvFuncMultiplyCrv,
// N_CrvFuncMultiplyCrv4d.
//
// One entry point, sc_nlib_funcs_adv_eval(op, in, n_in, out, cap, out_len): `op`
// selects the routine, `in` holds its inputs, `out` receives a flat record of doubles
// (layouts in crates/difftest/src/units/nlib_funcs_adv.rs). The record starts with the
// routine's status: 0 = ok, else NL_ERROR.eno (cleared before the call; 1000 if the
// flag was set without a number); outputs follow only on success.
//
// A curve function in `in` is: n p m, then n+1 values, m+1 knots. A curve is: n p m,
// then n+1 control points (x y z w), m+1 knots. Everything is allocated on one
// NL_STACKS with the outputs, so the in-place refinement can free the input's arrays.
// The Rust wrapper validates the inputs so upstream only touches valid memory.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

#include <vector>

#include "sc_nlib.h"

using sc::dump_crv;
using sc::nl_status;
using sc::null_crv;
using sc::read_crv;

namespace
{
// Reads one curve function at in[*at], allocating it on S; advances *at.
NL_CFUN* read_fun(const double* in, int64_t* at, NL_STACKS* S)
{
    const double* d = in + *at;
    NL_INDEX n = (NL_INDEX)d[0];
    NL_DEGREE p = (NL_DEGREE)d[1];
    NL_INDEX m = (NL_INDEX)d[2];
    NL_CFUN* cfn = N_AllocCrvFunc(n, p, m, S);
    if (cfn == NULL)
        return NULL;
    const double* c = d + 3;
    NL_REAL *fu, *U;
    N_CrvFuncCntrlValKnots(cfn, &fu, &U);
    for (NL_INDEX i = 0; i <= n; i++)
        fu[i] = *c++;
    for (NL_INDEX i = 0; i <= m; i++)
        U[i] = *c++;
    *at = (int64_t)(c - in);
    return cfn;
}

// n p m, values in use, knots in use (through N_CFuncGetData).
void dump_fun(std::vector<double>& o, NL_CFUN* cfn)
{
    NL_INDEX n, m;
    NL_DEGREE p;
    NL_REAL *fu, *U;
    N_CFuncGetData(cfn, &n, &fu, &p, &m, &U);
    o.push_back((double)n);
    o.push_back((double)p);
    o.push_back((double)m);
    for (NL_INDEX i = 0; i <= n; i++)
        o.push_back(fu[i]);
    for (NL_INDEX i = 0; i <= m; i++)
        o.push_back(U[i]);
}

NL_CFUN* null_fun(NL_STACKS* S)
{
    NL_CFUN* cfn = N_AllocCrvFuncStack(S);
    if (cfn != NULL)
        N_CFuncInitArrays(cfn);
    return cfn;
}

} // namespace

// op 0: N_CrvFuncRefine     in = fun, nx, x[0..nx), inplace   out = status [fun]
// op 1: N_CrvFuncDeriv      in = fun, d                       out = status [fun]
// op 2: N_CrvFuncMultiplyCrvFunc in = fun F, same, [fun G]    out = status [H, F, G]
//       (same != 0: G is F itself, the upstream squaring call)
// op 3: N_CrvFuncMultiplyCrv     in = fun F, curve G          out = status [H, F, G]
// op 4: N_CrvFuncMultiplyCrv4d   in = fun F, curve G          out = status [H, F, G]
SC_EXPORT int32_t sc_nlib_funcs_adv_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                         int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int64_t at = 0;
    int32_t rc = SC_OK;
    NL_CFUN* f = read_fun(in, &at, &S);
    if (f == NULL)
    {
        rc = SC_ERR_ARG;
    }
    else
    {
        switch (op)
        {
            case 0:
            {
                const double* prm = in + at;
                NL_INDEX nx = (NL_INDEX)prm[0];
                NL_REAL* X = NULL;
                if (nx > 0)
                {
                    X = N_AllocReal1dArray(nx - 1, &S);
                    if (X == NULL)
                    {
                        rc = SC_ERR_ARG;
                        break;
                    }
                    for (NL_INDEX i = 0; i < nx; i++)
                        X[i] = prm[1 + i];
                }
                NL_KNOTVECTOR knx;
                N_KnotVectorFromRealArray(&knx, X, nx - 1);
                NL_CFUN* q = prm[1 + nx] != 0.0 ? f : null_fun(&S);
                if (q == NULL)
                {
                    rc = SC_ERR_ARG;
                    break;
                }
                N_ErrClear();
                NL_FLAG err = N_CrvFuncRefine(f, &knx, q, &S, &S);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    dump_fun(o, q);
                break;
            }
            case 1:
            {
                NL_INDEX d = (NL_INDEX)in[at];
                NL_CFUN* q = null_fun(&S);
                if (q == NULL)
                {
                    rc = SC_ERR_ARG;
                    break;
                }
                N_ErrClear();
                NL_FLAG err = N_CrvFuncDeriv(f, d, q, &S);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    dump_fun(o, q);
                break;
            }
            case 2:
            {
                bool same = in[at] != 0.0;
                at++;
                NL_CFUN* g = same ? f : read_fun(in, &at, &S);
                NL_CFUN* h = null_fun(&S);
                if (g == NULL || h == NULL)
                {
                    rc = SC_ERR_ARG;
                    break;
                }
                N_ErrClear();
                NL_FLAG err = N_CrvFuncMultiplyCrvFunc(f, g, h, &S);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                {
                    dump_fun(o, h);
                    dump_fun(o, f);
                    dump_fun(o, g);
                }
                break;
            }
            case 3:
            case 4:
            {
                NL_CURVE* g = read_crv(in, &at, &S);
                NL_CURVE* h = null_crv(&S);
                if (g == NULL || h == NULL)
                {
                    rc = SC_ERR_ARG;
                    break;
                }
                N_ErrClear();
                NL_FLAG err = op == 3 ? N_CrvFuncMultiplyCrv(f, g, h, &S)
                                      : N_CrvFuncMultiplyCrv4d(f, g, h, &S);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                {
                    dump_crv(o, h);
                    dump_fun(o, f);
                    dump_crv(o, g);
                }
                break;
            }
            default:
                rc = SC_ERR_ARG;
                break;
        }
    }
    N_EndNurbs(&S);
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
