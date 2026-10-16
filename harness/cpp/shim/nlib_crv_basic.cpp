// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.crv_basic`.
// Wraps the NL_CURVE routines of upstream source/SMLib/src/NL_CrvBasic.cpp owned by
// the unit, plus N_CrossBoundDerivCrvNonRatSrf (NL_SrfSymbol.cpp).
//
// One entry point, sc_nlib_crv_basic_eval(op, in, n_in, out, cap, out_len): `op`
// selects a group of upstream calls, `in` holds its inputs, `out` receives a flat
// record of doubles (layouts in crates/difftest/src/units/nlib_crv_basic.rs).
// Statuses of upstream calls are written into the record: 0 = ok, else NL_ERROR.eno
// (cleared before each call; 1000 if the flag was set without a number). Outputs
// upstream leaves unwritten are NaN.
//
// A curve in `in` is: N K n p m, then N+1 control points (x y z w), K+1 knots. N, K
// size the storage; n, m (<= the storage) are the indices in use. A curve function
// is: N K n p m, N+1 values, K+1 knots. A surface is: N M R S n m p q r s, then
// (N+1)*(M+1) control points (row-major), R+1 u knots, S+1 v knots. Everything is
// allocated on one NL_STACKS (as upstream callers do), so N_CrvCompress can free its
// arrays. The Rust wrapper validates shapes and index ranges so upstream only
// touches the storage it is given.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

#include <cmath>
#include <limits>
#include <vector>

#include "sc_nlib.h"

using sc::nl_status;
using sc::null_crv;

namespace
{
const double kNaN = std::numeric_limits<double>::quiet_NaN();

double flag(NL_BOOLEAN b) { return b ? 1.0 : 0.0; }

// Reads one curve at in[*at], allocating it on S; advances *at.
NL_CURVE* read_crv(const double* in, int64_t* at, NL_STACKS* S)
{
    const double* d = in + *at;
    NL_INDEX N = (NL_INDEX)d[0], K = (NL_INDEX)d[1];
    NL_INDEX n = (NL_INDEX)d[2];
    NL_DEGREE p = (NL_DEGREE)d[3];
    NL_INDEX m = (NL_INDEX)d[4];
    NL_CURVE* cur = N_AllocCrvAndArrays(N, p, K, S);
    if (cur == NULL)
        return NULL;
    const double* c = d + 5;
    NL_CPOINT* Pw;
    NL_REAL* U;
    N_CrvGetCPtsAndKnots(cur, &Pw, &U);
    for (NL_INDEX i = 0; i <= N; i++)
    {
        Pw[i].x = c[0];
        Pw[i].y = c[1];
        Pw[i].z = c[2];
        Pw[i].w = c[3];
        c += 4;
    }
    for (NL_INDEX i = 0; i <= K; i++)
        U[i] = *c++;
    N_CrvSetSizeIndices(cur, n, p, m);
    *at = (int64_t)(c - in);
    return cur;
}

// Reads one curve function at in[*at], allocating it on S; advances *at.
NL_CFUN* read_cfn(const double* in, int64_t* at, NL_STACKS* S)
{
    const double* d = in + *at;
    NL_INDEX N = (NL_INDEX)d[0], K = (NL_INDEX)d[1];
    NL_INDEX n = (NL_INDEX)d[2];
    NL_DEGREE p = (NL_DEGREE)d[3];
    NL_INDEX m = (NL_INDEX)d[4];
    NL_CFUN* cfn = N_AllocCrvFunc(N, p, K, S);
    if (cfn == NULL)
        return NULL;
    const double* c = d + 5;
    NL_REAL *fu, *U;
    N_CrvFuncCntrlValKnots(cfn, &fu, &U);
    for (NL_INDEX i = 0; i <= N; i++)
        fu[i] = *c++;
    for (NL_INDEX i = 0; i <= K; i++)
        U[i] = *c++;
    N_CFuncSetSizeIndices(cfn, n, p, m);
    *at = (int64_t)(c - in);
    return cfn;
}

// Reads one surface at in[*at], allocating it on S; advances *at.
NL_SURFACE* read_srf(const double* in, int64_t* at, NL_STACKS* S)
{
    const double* d = in + *at;
    NL_INDEX N = (NL_INDEX)d[0], M = (NL_INDEX)d[1], R = (NL_INDEX)d[2], SS = (NL_INDEX)d[3];
    NL_INDEX n = (NL_INDEX)d[4], m = (NL_INDEX)d[5];
    NL_DEGREE p = (NL_DEGREE)d[6], q = (NL_DEGREE)d[7];
    NL_INDEX r = (NL_INDEX)d[8], s = (NL_INDEX)d[9];
    NL_SURFACE* sur = N_AllocSrfAndArrays(N, M, p, q, R, SS, S);
    if (sur == NULL)
        return NULL;
    const double* c = d + 10;
    for (NL_INDEX i = 0; i <= N; i++)
        for (NL_INDEX j = 0; j <= M; j++)
        {
            NL_CPOINT& pw = sur->net->Pw[i][j];
            pw.x = c[0];
            pw.y = c[1];
            pw.z = c[2];
            pw.w = c[3];
            c += 4;
        }
    for (NL_INDEX i = 0; i <= R; i++)
        sur->knu->U[i] = *c++;
    for (NL_INDEX i = 0; i <= SS; i++)
        sur->knv->U[i] = *c++;
    N_SrfSetSizeIndices(sur, n, m, p, q, r, s);
    *at = (int64_t)(c - in);
    return sur;
}

// n p m, used control points, used knots (through N_CrvGetCPtsDegreeAndKnots);
// a NULL curve is the single value -1.
void dump_crv(std::vector<double>& o, NL_CURVE* cur)
{
    if (cur == NULL || N_CrvAreArraysNULL(cur))
    {
        o.push_back(-1.0);
        return;
    }
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

// n p m, used values, used knots (through N_CFuncGetData); NULL is -1.
void dump_cfn(std::vector<double>& o, NL_CFUN* cfn)
{
    if (cfn == NULL || N_CrvAreFuncArraysNULL(cfn))
    {
        o.push_back(-1.0);
        return;
    }
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

NL_CFUN* null_cfn(NL_STACKS* S)
{
    NL_CFUN* cfn = N_AllocCrvFuncStack(S);
    if (cfn != NULL)
        N_CFuncInitArrays(cfn);
    return cfn;
}

// Op 0: predicates, checks and measures of one curve. Params: tol np mk.
void op_inspect(std::vector<double>& o, NL_CURVE* cur, const double* prm, NL_STACKS* S)
{
    NL_FLAG err;
    TCHAR rname[] = _T("sc_nlib_crv_basic");

    o.push_back(flag(N_CrvIs3d(cur)));
    N_ErrClear();
    err = N_CrvWeightsAreValid(cur, rname);
    o.push_back(nl_status(err));
    N_ErrClear();
    err = N_CrvIsSized(cur, (NL_INDEX)prm[1], (NL_INDEX)prm[2], rname);
    o.push_back(nl_status(err));
    o.push_back(flag(N_IsCrvRat(cur)));
    o.push_back(flag(N_CrvIsClosed(cur)));
    o.push_back(flag(N_CrvIsDegen(cur)));
    o.push_back(flag(N_CrvIsLine(cur, prm[0])));
    o.push_back(flag(N_CrvAreWeightsEqual(cur)));
    o.push_back(flag(N_Crv4dIsDegen(cur)));
    o.push_back(flag(N_CrvIsInZ0Plane(cur)));

    NL_PARAMETER ul, ur;
    N_CrvGetParamBounds(cur, &ul, &ur);
    o.push_back(ul);
    o.push_back(ur);

    NL_INDEX n, m;
    N_CrvGetArraySizes(cur, &n, &m);
    NL_DEGREE p;
    N_CrvGetDegree(cur, &p);
    o.push_back((double)n);
    o.push_back((double)p);
    o.push_back((double)m);

    NL_POINT PP = {kNaN, kNaN, kNaN};
    NL_VECTOR NN = {kNaN, kNaN, kNaN};
    NL_FLAG flt = -7;
    N_ErrClear();
    err = N_CrvIsPlanar(cur, prm[0], &PP, &NN, &flt, S);
    o.push_back(nl_status(err));
    if (err != NL_NO)
    {
        // The Rust port returns only the error: the outputs are not compared.
        for (int k = 0; k < 7; k++)
            o.push_back(kNaN);
        return;
    }
    o.push_back(flt == -7 ? kNaN : (double)flt);
    o.push_back(PP.x);
    o.push_back(PP.y);
    o.push_back(PP.z);
    o.push_back(NN.x);
    o.push_back(NN.y);
    o.push_back(NN.z);
}
} // namespace

// op 0 inspect: curve, tol np mk.
// op 1 epolygon: curve, k l -> status, points (3 per point).
// op 2 modify: curve, kind, params -> status, curve dump.
//   kind 0 compress; 1 3d->2d; 2 2d->3d; 3 rat->non-rat; 4 non-rat->rat; 5 make
//   non-rat; 6 prune rat (flg); 7 reparam (a b); 8 set size indices (n p m); 9 copy
//   into a NULL curve; 10 copy into the second curve (follows the params); 11 size
//   arrays to (n p m) -> status, n p m only.
// op 3 equal: curve P, curve Q, tol3d tol1d -> flag.
// op 4 compatible: count k, k curves -> flag.
// op 5 rat compatible: count k, k curves -> each dump.
// op 6 coincident: curve P, curve Q, tol -> flag, P dump, Q dump.
// op 7 conic: ctp as ae -> ae n p m.
// op 8 cross boundary: surface, side, has_dst, [curve] -> status, curve dump.
// op 9 numerator/denominator: curve, mode, ... (see the runner).
SC_EXPORT int32_t sc_nlib_crv_basic_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                         int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int64_t at = 0;
    int32_t rc = SC_OK;
    TCHAR rname[] = _T("sc_nlib_crv_basic");
    switch (op)
    {
        case 0:
        {
            NL_CURVE* cur = read_crv(in, &at, &S);
            if (cur == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            op_inspect(o, cur, in + at, &S);
            break;
        }
        case 1:
        {
            NL_CURVE* cur = read_crv(in, &at, &S);
            if (cur == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_INDEX k = (NL_INDEX)in[at], l = (NL_INDEX)in[at + 1];
            NL_EPOLYGON ppl;
            N_ErrClear();
            NL_FLAG err = N_CrvGetEPolygon(cur, k, l, &ppl, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                for (NL_INDEX i = 0; i <= ppl.n; i++)
                {
                    o.push_back(ppl.P[i].x);
                    o.push_back(ppl.P[i].y);
                    o.push_back(ppl.P[i].z);
                }
            }
            break;
        }
        case 2:
        {
            NL_CURVE* cur = read_crv(in, &at, &S);
            if (cur == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            const double* prm = in + at;
            int kind = (int)prm[0];
            NL_CURVE* res = cur;
            NL_FLAG err = NL_NO;
            N_ErrClear();
            switch (kind)
            {
                case 0:
                    err = N_CrvCompress(cur, &S);
                    break;
                case 1:
                    N_Crv3dTo2d(cur);
                    break;
                case 2:
                    N_Crv2dTo3d(cur);
                    break;
                case 3:
                    N_CrvRatToNonRat(cur);
                    break;
                case 4:
                    N_CrvNonRatToRat(cur);
                    break;
                case 5:
                    N_CrvMakeNonRat(cur);
                    break;
                case 6:
                    N_CrvPruneRat(cur, (NL_FLAG)prm[1]);
                    break;
                case 7:
                {
                    NL_INTERVAL I;
                    N_CreateInterval(&I, prm[1], prm[2]);
                    N_CrvReparamToInterval(cur, I);
                    break;
                }
                case 8:
                    N_CrvSetSizeIndices(cur, (NL_INDEX)prm[1], (NL_DEGREE)prm[2], (NL_INDEX)prm[3]);
                    break;
                case 9:
                {
                    res = null_crv(&S);
                    if (res == NULL || !N_CrvAreArraysNULL(res))
                    {
                        rc = SC_ERR_ARG;
                        break;
                    }
                    err = N_CrvCopy(cur, res, &S);
                    break;
                }
                case 10:
                {
                    int64_t at2 = at + 1;
                    res = read_crv(in, &at2, &S);
                    if (res == NULL || N_CrvAreArraysNULL(res))
                    {
                        rc = SC_ERR_ARG;
                        break;
                    }
                    err = N_CrvCopy(cur, res, &S);
                    break;
                }
                case 11:
                {
                    err = N_CrvSizeArrays(cur, (NL_INDEX)prm[1], (NL_DEGREE)prm[2], (NL_INDEX)prm[3], rname, &S);
                    o.push_back(nl_status(err));
                    NL_INDEX n, m;
                    NL_DEGREE p;
                    N_CrvGetArraySizes(cur, &n, &m);
                    N_CrvGetDegree(cur, &p);
                    o.push_back((double)n);
                    o.push_back((double)p);
                    o.push_back((double)m);
                    break;
                }
                default:
                    rc = SC_ERR_ARG;
            }
            if (rc != SC_OK || kind == 11)
                break;
            o.push_back(nl_status(err));
            dump_crv(o, res);
            break;
        }
        case 3:
        {
            NL_CURVE* P = read_crv(in, &at, &S);
            NL_CURVE* Q = P == NULL ? NULL : read_crv(in, &at, &S);
            if (Q == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            o.push_back(flag(N_CrvsAreEqual(P, Q, in[at], in[at + 1])));
            break;
        }
        case 4:
        case 5:
        {
            int64_t k = (int64_t)in[0];
            at = 1;
            std::vector<NL_CURVE*> crvs;
            for (int64_t i = 0; i < k; i++)
            {
                NL_CURVE* cur = read_crv(in, &at, &S);
                if (cur == NULL)
                {
                    rc = SC_ERR_ARG;
                    break;
                }
                crvs.push_back(cur);
            }
            if (rc != SC_OK)
                break;
            if (op == 4)
            {
                if (crvs.empty())
                {
                    rc = SC_ERR_ARG;
                    break;
                }
                o.push_back(flag(N_CrvsAreCombatible(crvs.data(), (NL_INDEX)(k - 1))));
            }
            else
            {
                N_CrvsMakeRatCompatible(crvs.empty() ? NULL : crvs.data(), (NL_INDEX)(k - 1));
                for (NL_CURVE* cur : crvs)
                    dump_crv(o, cur);
            }
            break;
        }
        case 6:
        {
            NL_CURVE* P = read_crv(in, &at, &S);
            NL_CURVE* Q = P == NULL ? NULL : read_crv(in, &at, &S);
            if (Q == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            o.push_back(flag(N_CrvsAreCoincident(P, Q, in[at], &S)));
            dump_crv(o, P);
            dump_crv(o, Q);
            break;
        }
        case 7:
        {
            NL_FLAG ctp = (NL_FLAG)in[0];
            NL_REAL ae = in[2];
            NL_INDEX n = -1, m = -1;
            NL_DEGREE p = -1;
            N_CrvGetConicData(ctp, in[1], &ae, &n, &p, &m);
            o.push_back(ae);
            o.push_back((double)n);
            o.push_back((double)p);
            o.push_back((double)m);
            break;
        }
        case 8:
        {
            NL_SURFACE* sur = read_srf(in, &at, &S);
            if (sur == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_FLAG side = (NL_FLAG)in[at];
            bool has_dst = in[at + 1] != 0.0;
            at += 2;
            NL_CURVE* cur = has_dst ? read_crv(in, &at, &S) : null_crv(&S);
            if (cur == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            N_ErrClear();
            NL_FLAG err = N_CrossBoundDerivCrvNonRatSrf(sur, side, cur, &S);
            o.push_back(nl_status(err));
            dump_crv(o, cur);
            break;
        }
        case 9:
        {
            NL_CURVE* cur = read_crv(in, &at, &S);
            if (cur == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            int mode = (int)in[at];
            bool has_c = in[at + 1] != 0.0;
            at += 2;
            NL_CURVE* dc = has_c ? read_crv(in, &at, &S) : null_crv(&S);
            if (dc == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            bool has_f = in[at] != 0.0;
            at += 1;
            NL_CFUN* df = has_f ? read_cfn(in, &at, &S) : null_cfn(&S);
            if (df == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_FLAG err = NL_NO;
            N_ErrClear();
            switch (mode)
            {
                case 0: // N_CrvGetNumAndDenom(cur, dc, df)
                    err = N_CrvGetNumAndDenom(cur, dc, df, &S);
                    break;
                case 1: // N_CrvGetDenomCrvFunc(cur, df)
                    err = N_CrvGetDenomCrvFunc(cur, df, &S);
                    break;
                case 2: // N_CreateCrvFromNumAndDenom(cur, df or NULL, dc)
                    err = N_CreateCrvFromNumAndDenom(cur, has_f ? df : NULL, dc, &S);
                    break;
                default:
                    rc = SC_ERR_ARG;
            }
            if (rc != SC_OK)
                break;
            o.push_back(nl_status(err));
            dump_crv(o, dc);
            dump_cfn(o, df);
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
