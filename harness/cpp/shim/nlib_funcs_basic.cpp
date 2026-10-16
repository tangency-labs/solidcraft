// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.funcs_basic`.
// Wraps the NL_CFUN / NL_SFUN routines of upstream source/SMLib/src/NL_FuncsBasic.cpp
// owned by the unit.
//
// One entry point, sc_nlib_funcs_basic_eval(op, in, n_in, out, cap, out_len): `op`
// selects a group of upstream calls, `in` holds its inputs, `out` receives a flat
// record of doubles (layouts in crates/difftest/src/units/nlib_funcs_basic.rs).
// Statuses of upstream calls are written into the record: 0 = ok, else NL_ERROR.eno
// (cleared before each call; 1000 if the flag was set without a number). Outputs
// upstream leaves unwritten are NaN.
//
// A curve function in `in` is: N M n m p, then N+1 values, M+1 knots. A surface
// function is: N M R S n m p q r s, then (N+1)*(M+1) values (row-major), R+1 u knots,
// S+1 v knots. Capital letters size the storage; lower-case indices (<= the storage)
// are the indices in use. Every function is allocated on one NL_STACKS (as upstream
// callers do), so N_CrvFuncCompact can free its arrays. The Rust wrapper validates
// shapes and index ranges so upstream only touches the storage it is given.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

#include <cmath>
#include <limits>
#include <vector>

#include "sc_nlib.h"

using sc::nl_status;

namespace
{
const double kNaN = std::numeric_limits<double>::quiet_NaN();

double flag(NL_BOOLEAN b) { return b ? 1.0 : 0.0; }

// Reads one curve function at in[*at], allocating it on S; advances *at.
NL_CFUN* read_crv(const double* in, int64_t* at, NL_STACKS* S)
{
    const double* d = in + *at;
    NL_INDEX N = (NL_INDEX)d[0], M = (NL_INDEX)d[1];
    NL_INDEX n = (NL_INDEX)d[2], m = (NL_INDEX)d[3];
    NL_DEGREE p = (NL_DEGREE)d[4];
    NL_CFUN* cfn = N_AllocCrvFunc(N, p, M, S);
    if (cfn == NULL)
        return NULL;
    const double* c = d + 5;
    NL_REAL *fu, *U;
    N_CrvFuncCntrlValKnots(cfn, &fu, &U);
    for (NL_INDEX i = 0; i <= N; i++)
        fu[i] = *c++;
    for (NL_INDEX i = 0; i <= M; i++)
        U[i] = *c++;
    N_CFuncSetSizeIndices(cfn, n, p, m);
    *at = (int64_t)(c - in);
    return cfn;
}

// Reads one surface function at in[*at], allocating it on S; advances *at.
NL_SFUN* read_srf(const double* in, int64_t* at, NL_STACKS* S)
{
    const double* d = in + *at;
    NL_INDEX N = (NL_INDEX)d[0], M = (NL_INDEX)d[1], R = (NL_INDEX)d[2], SS = (NL_INDEX)d[3];
    NL_INDEX n = (NL_INDEX)d[4], m = (NL_INDEX)d[5];
    NL_DEGREE p = (NL_DEGREE)d[6], q = (NL_DEGREE)d[7];
    NL_INDEX r = (NL_INDEX)d[8], s = (NL_INDEX)d[9];
    NL_SFUN* sfn = N_AllocSrfFunc(N, M, p, q, R, SS, S);
    if (sfn == NULL)
        return NULL;
    const double* c = d + 10;
    NL_REAL **fuv, *U, *V;
    N_SFuncGetKnots(sfn, &fuv, &U, &V);
    for (NL_INDEX i = 0; i <= N; i++)
        for (NL_INDEX j = 0; j <= M; j++)
            fuv[i][j] = *c++;
    for (NL_INDEX i = 0; i <= R; i++)
        U[i] = *c++;
    for (NL_INDEX i = 0; i <= SS; i++)
        V[i] = *c++;
    N_SFuncSetSizeIndices(sfn, n, m, p, q, r, s);
    *at = (int64_t)(c - in);
    return sfn;
}

// n p m, values in use, knots in use (through N_CFuncGetData).
void dump_crv(std::vector<double>& o, NL_CFUN* cfn)
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

// n m p q r s, values in use, knots in use (through N_SrfFuncCntrlVal,
// N_SFuncGetDegrees, N_SFuncGetKnotVectors, N_SFuncGetKnots).
void dump_srf(std::vector<double>& o, NL_SFUN* sfn)
{
    NL_INDEX n, m;
    NL_REAL** fuv;
    N_SrfFuncCntrlVal(sfn, &n, &m, &fuv);
    NL_DEGREE p, q;
    N_SFuncGetDegrees(sfn, &p, &q);
    NL_KNOTVECTOR *knu, *knv;
    N_SFuncGetKnotVectors(sfn, &knu, &knv);
    NL_REAL **fv2, *U, *V;
    N_SFuncGetKnots(sfn, &fv2, &U, &V);
    o.push_back((double)n);
    o.push_back((double)m);
    o.push_back((double)p);
    o.push_back((double)q);
    o.push_back((double)knu->m);
    o.push_back((double)knv->m);
    for (NL_INDEX i = 0; i <= n; i++)
        for (NL_INDEX j = 0; j <= m; j++)
            o.push_back(fuv[i][j]);
    for (NL_INDEX i = 0; i <= knu->m; i++)
        o.push_back(U[i]);
    for (NL_INDEX j = 0; j <= knv->m; j++)
        o.push_back(V[j]);
}

TCHAR kRname[] = _T("sc_nlib_funcs_basic");

// Op 0: curve, der, k, k x (u flag) -> per point: status f, status fd[0..=der].
void op_crv_eval(std::vector<double>& o, NL_CFUN* cfn, const double* prm)
{
    NL_INDEX der = (NL_INDEX)prm[0];
    int64_t k = (int64_t)prm[1];
    std::vector<double> fd((size_t)der + 1);
    for (int64_t t = 0; t < k; t++)
    {
        NL_PARAMETER u = prm[2 + 2 * t];
        NL_FLAG flg = (NL_FLAG)prm[3 + 2 * t];
        NL_REAL f = kNaN;
        N_ErrClear();
        NL_FLAG err = N_CFuncEval(cfn, u, flg, &f);
        o.push_back(nl_status(err));
        o.push_back(f);
        for (double& x : fd)
            x = kNaN;
        N_ErrClear();
        err = N_CFuncDerivs(cfn, u, flg, der, fd.data());
        o.push_back(nl_status(err));
        for (double x : fd)
            o.push_back(x);
    }
}

// Op 1: curve, nf mk -> is_sized, null flags, accessors, dump.
void op_crv_inspect(std::vector<double>& o, NL_CFUN* cfn, const double* prm, NL_STACKS* S)
{
    N_ErrClear();
    NL_FLAG err = N_CrvIsFuncSized(cfn, (NL_INDEX)prm[0], (NL_INDEX)prm[1], kRname);
    o.push_back(nl_status(err));
    o.push_back(flag(N_CrvAreFuncArraysNULL(cfn)));
    NL_CFUN* nul = N_AllocCrvFuncStack(S);
    if (nul == NULL)
    {
        o.push_back(kNaN);
    }
    else
    {
        N_CFuncInitArrays(nul);
        o.push_back(flag(N_CrvAreFuncArraysNULL(nul)));
    }

    NL_INDEX n, m;
    N_CFuncGetArraySizes(cfn, &n, &m);
    NL_DEGREE p;
    N_CFuncGetDegree(cfn, &p);
    NL_CVALUE* cvl;
    NL_DEGREE p2;
    NL_KNOTVECTOR* knt;
    N_CFuncGetArrayAndKnotVector(cfn, &cvl, &p2, &knt);
    NL_KNOTVECTOR* knt2;
    N_CFuncGetKnotVector(cfn, &knt2);
    NL_REAL *fu, *U;
    NL_KNOTVECTOR* knt3;
    N_CrvFuncCntrlValKnotVector(cfn, &fu, &knt3, &U);
    o.push_back((double)n);
    o.push_back((double)m);
    o.push_back((double)p);
    o.push_back((double)cvl->n);
    o.push_back((double)p2);
    o.push_back((double)knt->m);
    o.push_back((double)knt2->m);
    o.push_back((double)knt3->m);
    o.push_back(fu[0]);
    o.push_back(U[0]);
    dump_crv(o, cfn);
}

// Op 4: surface, nf mf rk sk -> is_sized, null flags, accessors, dump.
void op_srf_inspect(std::vector<double>& o, NL_SFUN* sfn, const double* prm, NL_STACKS* S)
{
    N_ErrClear();
    NL_FLAG err = N_SrfIsFuncSized(sfn, (NL_INDEX)prm[0], (NL_INDEX)prm[1], (NL_INDEX)prm[2], (NL_INDEX)prm[3],
                                   kRname);
    o.push_back(nl_status(err));
    o.push_back(flag(N_SrfAreFuncArraysNULL(sfn)));
    NL_SFUN* nul = N_AllocSrfFuncData(S);
    if (nul == NULL)
    {
        o.push_back(kNaN);
    }
    else
    {
        N_SFuncInitArrays(nul);
        o.push_back(flag(N_SrfAreFuncArraysNULL(nul)));
    }

    NL_INDEX n, m, r, s;
    N_SFuncGetArraySizes(sfn, &n, &m, &r, &s);
    NL_SVALUE* svl;
    NL_DEGREE p, q;
    NL_KNOTVECTOR *knu, *knv;
    N_SFuncGetArrayAndKnotVectors(sfn, &svl, &p, &q, &knu, &knv);
    o.push_back((double)n);
    o.push_back((double)m);
    o.push_back((double)r);
    o.push_back((double)s);
    o.push_back((double)svl->n);
    o.push_back((double)svl->m);
    o.push_back((double)p);
    o.push_back((double)q);
    o.push_back((double)knu->m);
    o.push_back((double)knv->m);
    dump_srf(o, sfn);
}
} // namespace

// op 0 crv eval: curve, der, k, k x (u flag).
// op 1 crv inspect: curve, nf mk.
// op 2 crv modify: curve, kind, params -> status, curve dump.
//   kind 0 compact; 1 set size indices (n p m); 2 set ptrs (lf, lf values, lu, lu knots);
//   3 size arrays (n p m); 4 size arrays of a NULL function (n p m) -> status, n p m, null flag.
// op 3 srf eval: surface, k, k x (u v ufl vfl) -> per point: status F.
// op 4 srf inspect: surface, nf mf rk sk.
// op 5 srf modify: surface, kind, n m p q r s -> status, surface dump.
//   kind 0 set size indices; 1 size arrays; 2 size arrays of a NULL function
//   -> status, n m p q r s, null flag.
SC_EXPORT int32_t sc_nlib_funcs_basic_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                           int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int64_t at = 0;
    int32_t rc = SC_OK;
    switch (op)
    {
        case 0:
        case 1:
        case 2:
        {
            NL_CFUN* cfn = read_crv(in, &at, &S);
            if (cfn == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            const double* prm = in + at;
            if (op == 0)
            {
                op_crv_eval(o, cfn, prm);
                break;
            }
            if (op == 1)
            {
                op_crv_inspect(o, cfn, prm, &S);
                break;
            }
            int kind = (int)prm[0];
            NL_FLAG err = NL_NO;
            N_ErrClear();
            switch (kind)
            {
                case 0:
                    err = N_CrvFuncCompact(cfn, &S);
                    break;
                case 1:
                    N_CFuncSetSizeIndices(cfn, (NL_INDEX)prm[1], (NL_DEGREE)prm[2], (NL_INDEX)prm[3]);
                    break;
                case 2:
                {
                    const double* c = prm + 1;
                    NL_INDEX lf = (NL_INDEX)*c++;
                    NL_REAL* fu = N_AllocReal1dArray(lf - 1, &S);
                    if (fu == NULL)
                    {
                        rc = SC_ERR_ARG;
                        break;
                    }
                    for (NL_INDEX i = 0; i < lf; i++)
                        fu[i] = *c++;
                    NL_INDEX lu = (NL_INDEX)*c++;
                    NL_REAL* U = N_AllocReal1dArray(lu - 1, &S);
                    if (U == NULL)
                    {
                        rc = SC_ERR_ARG;
                        break;
                    }
                    for (NL_INDEX i = 0; i < lu; i++)
                        U[i] = *c++;
                    N_CrvFuncSetPtrs(cfn, fu, U);
                    break;
                }
                case 3:
                    err = N_CFuncSizeArrays(cfn, (NL_INDEX)prm[1], (NL_DEGREE)prm[2], (NL_INDEX)prm[3], kRname, &S);
                    break;
                case 4:
                {
                    NL_CFUN* nul = N_AllocCrvFuncStack(&S);
                    if (nul == NULL)
                    {
                        rc = SC_ERR_ARG;
                        break;
                    }
                    N_CFuncInitArrays(nul);
                    err = N_CFuncSizeArrays(nul, (NL_INDEX)prm[1], (NL_DEGREE)prm[2], (NL_INDEX)prm[3], kRname, &S);
                    o.push_back(nl_status(err));
                    NL_INDEX n, m;
                    NL_DEGREE p;
                    N_CFuncGetArraySizes(nul, &n, &m);
                    N_CFuncGetDegree(nul, &p);
                    o.push_back((double)n);
                    o.push_back((double)p);
                    o.push_back((double)m);
                    o.push_back(flag(N_CrvAreFuncArraysNULL(nul)));
                    break;
                }
                default:
                    rc = SC_ERR_ARG;
            }
            if (rc != SC_OK || kind == 4)
                break;
            o.push_back(nl_status(err));
            dump_crv(o, cfn);
            break;
        }
        case 3:
        case 4:
        case 5:
        {
            NL_SFUN* sfn = read_srf(in, &at, &S);
            if (sfn == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            const double* prm = in + at;
            if (op == 3)
            {
                int64_t k = (int64_t)prm[0];
                for (int64_t t = 0; t < k; t++)
                {
                    const double* x = prm + 1 + 4 * t;
                    NL_REAL F = kNaN;
                    N_ErrClear();
                    NL_FLAG err = N_SrfFuncEvalPt(sfn, x[0], x[1], (NL_FLAG)x[2], (NL_FLAG)x[3], &F);
                    o.push_back(nl_status(err));
                    o.push_back(F);
                }
                break;
            }
            if (op == 4)
            {
                op_srf_inspect(o, sfn, prm, &S);
                break;
            }
            int kind = (int)prm[0];
            NL_INDEX n = (NL_INDEX)prm[1], m = (NL_INDEX)prm[2];
            NL_DEGREE p = (NL_DEGREE)prm[3], q = (NL_DEGREE)prm[4];
            NL_INDEX r = (NL_INDEX)prm[5], s = (NL_INDEX)prm[6];
            NL_FLAG err = NL_NO;
            N_ErrClear();
            switch (kind)
            {
                case 0:
                    N_SFuncSetSizeIndices(sfn, n, m, p, q, r, s);
                    break;
                case 1:
                    err = N_SFuncSizeArrays(sfn, n, m, p, q, r, s, kRname, &S);
                    break;
                case 2:
                {
                    NL_SFUN* nul = N_AllocSrfFuncData(&S);
                    if (nul == NULL)
                    {
                        rc = SC_ERR_ARG;
                        break;
                    }
                    N_SFuncInitArrays(nul);
                    err = N_SFuncSizeArrays(nul, n, m, p, q, r, s, kRname, &S);
                    o.push_back(nl_status(err));
                    NL_INDEX n2, m2, r2, s2;
                    NL_DEGREE p2, q2;
                    N_SFuncGetArraySizes(nul, &n2, &m2, &r2, &s2);
                    N_SFuncGetDegrees(nul, &p2, &q2);
                    o.push_back((double)n2);
                    o.push_back((double)m2);
                    o.push_back((double)p2);
                    o.push_back((double)q2);
                    o.push_back((double)r2);
                    o.push_back((double)s2);
                    o.push_back(flag(N_SrfAreFuncArraysNULL(nul)));
                    break;
                }
                default:
                    rc = SC_ERR_ARG;
            }
            if (rc != SC_OK || kind == 2)
                break;
            o.push_back(nl_status(err));
            dump_srf(o, sfn);
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
