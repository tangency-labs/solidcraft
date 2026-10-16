// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.crv_adv_p2`.
// Wraps the NL_CURVE routines of upstream source/SMLib/src/NL_CrvAdv.cpp owned by the unit:
// N_CrvExtendByDist, N_CrvExtendByParamDist, N_CrvOffsetGetMax2ndDeriv,
// N_CrvEvalUnboundedPtsAndDerivs, N_CrvRemoveKnotsDerivConstraints, N_CrvRefineToKnotVector,
// N_CrvSumDiffCrv, N_CrvNonRatGetFirstDeriv, N_CrvRatGetFirstDeriv, N_ConstantMultiplyCrv,
// N_LinearMultiplyCrv, N_ConstantMultiplyCrv4d, N_CrvReparamRat, N_CrvReparamWeights,
// N_SrfReparamMultKnots and N_SrfReparamWeights.
//
// One entry point, sc_nlib_crv_adv_p2_eval(op, in, n_in, out, cap, out_len): `op` selects
// the routine, `in` holds a curve (two for N_CrvSumDiffCrv) then the routine's parameters,
// `out` receives a flat record of doubles (layouts in
// crates/solidcore-ffi/src/units/nlib_crv_adv_p2.rs). The record starts with the routine's
// status: 0 = ok, else NL_ERROR.eno (cleared before the call; 1000 if the flag was set
// without a number). Outputs follow on success; the in-place-only routines (multiplies,
// N_CrvReparamWeights, N_SrfReparamWeights) dump the curve whatever the status.
//
// A curve in `in` is: n p m, then n+1 control points (x y z w), m+1 knots, allocated on one
// NL_STACKS with the outputs. The Rust wrapper validates the curves (clamped, well-formed)
// and the parameters so upstream only touches valid memory.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>
#include <NL_CrvAdv.h>

#include <vector>

#include "sc_nlib.h"

using sc::nl_status;
using sc::null_crv;
using sc::read_crv;

namespace
{
void push_cpt(std::vector<double>& o, const NL_CPOINT& c)
{
    o.push_back(c.x);
    o.push_back(c.y);
    o.push_back(c.z);
    o.push_back(c.w);
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
        push_cpt(o, Pw[i]);
    for (NL_INDEX i = 0; i <= m; i++)
        o.push_back(U[i]);
}

// Output curve: the input itself (in place) or a new NULL curve.
NL_CURVE* out_crv(NL_CURVE* cur, double inplace, NL_STACKS* S)
{
    return inplace != 0.0 ? cur : null_crv(S);
}

void status_and_crv(std::vector<double>& o, NL_FLAG err, NL_CURVE* q)
{
    o.push_back(nl_status(err));
    if (err == NL_NO)
        dump_crv(o, q);
}
} // namespace

SC_EXPORT int32_t sc_nlib_crv_adv_p2_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                          int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int64_t at = 0;
    int32_t rc = SC_OK;
    NL_CURVE* cur = read_crv(in, &at, &S);
    if (cur == NULL)
    {
        rc = SC_ERR_ARG;
    }
    else
    {
        N_ErrClear();
        switch (op)
        {
            case 0: // N_CrvExtendByDist: dist flg end cont inplace
            {
                const double* prm = in + at;
                NL_CURVE* q = out_crv(cur, prm[4], &S);
                NL_FLAG err = N_CrvExtendByDist(cur, prm[0], (NL_FLAG)prm[1], (NL_FLAG)prm[2],
                                                (NL_FLAG)prm[3], q, &S, &S);
                status_and_crv(o, err, q);
                break;
            }
            case 1: // N_CrvExtendByParamDist: par end cont inplace
            {
                const double* prm = in + at;
                NL_CURVE* q = out_crv(cur, prm[3], &S);
                NL_FLAG err =
                    N_CrvExtendByParamDist(cur, prm[0], (NL_FLAG)prm[1], (NL_FLAG)prm[2], q, &S, &S);
                status_and_crv(o, err, q);
                break;
            }
            case 2: // N_CrvOffsetGetMax2ndDeriv: d N[3]
            {
                const double* prm = in + at;
                NL_VECTOR N;
                N.x = prm[1];
                N.y = prm[2];
                N.z = prm[3];
                NL_REAL Muu = 0.0;
                NL_FLAG err = N_CrvOffsetGetMax2ndDeriv(cur, prm[0], N, &Muu);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    o.push_back(Muu);
                break;
            }
            case 3: // N_CrvEvalUnboundedPtsAndDerivs: cont k, then k x (u ndr)
            {
                const double* prm = in + at;
                NL_FLAG cflg = (NL_FLAG)prm[0];
                NL_INDEX k = (NL_INDEX)prm[1];
                NL_CURVE* ecur = null_crv(&S);
                NL_FLAG gflg = NL_NO;
                NL_FLAG err = NL_NO;
                for (NL_INDEX c = 0; c < k && err == NL_NO; c++)
                {
                    NL_PARAMETER u = prm[2 + 2 * c];
                    NL_INDEX ndr = (NL_INDEX)prm[3 + 2 * c];
                    std::vector<NL_POINT> D((size_t)ndr + 1);
                    N_ErrClear();
                    err = N_CrvEvalUnboundedPtsAndDerivs(cur, u, ndr, cflg, ecur, &gflg, D.data(), &S);
                    o.push_back(nl_status(err));
                    if (err == NL_NO)
                        for (NL_INDEX j = 0; j <= ndr; j++)
                        {
                            o.push_back(D[j].x);
                            o.push_back(D[j].y);
                            o.push_back(D[j].z);
                        }
                }
                if (err == NL_NO)
                {
                    if (N_CrvAreArraysNULL(ecur))
                    {
                        o.push_back(0.0);
                    }
                    else
                    {
                        o.push_back(1.0);
                        o.push_back(gflg == NL_NO ? 0.0 : 1.0);
                        dump_crv(o, ecur);
                    }
                }
                break;
            }
            case 4: // N_CrvRemoveKnotsDerivConstraints: tol whr der inplace
            {
                const double* prm = in + at;
                NL_CURVE* q = out_crv(cur, prm[3], &S);
                NL_FLAG err = N_CrvRemoveKnotsDerivConstraints(cur, prm[0], (NL_FLAG)prm[1],
                                                               (NL_INDEX)prm[2], q, &S);
                status_and_crv(o, err, q);
                break;
            }
            case 5: // N_CrvRefineToKnotVector: r+1, X[0..=r]
            {
                const double* prm = in + at;
                NL_INDEX cnt = (NL_INDEX)prm[0];
                std::vector<NL_REAL> X(prm + 1, prm + 1 + cnt);
                NL_INDEX n;
                NL_CPOINT* Pw;
                N_CrvGetCPts(cur, &n, &Pw);
                NL_CPOINT zero;
                zero.x = zero.y = zero.z = zero.w = 0.0;
                std::vector<NL_CPOINT> Qw((size_t)(n + cnt + 1), zero);
                if (X.empty())
                    X.push_back(0.0); // never read: r = -1 fails first
                NL_FLAG err = N_CrvRefineToKnotVector(cur, X.data(), cnt - 1, Qw.data());
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    for (const NL_CPOINT& c : Qw)
                        push_cpt(o, c);
                break;
            }
            case 6: // N_CrvSumDiffCrv: second curve, opr
            {
                NL_CURVE* curQ = read_crv(in, &at, &S);
                const double* prm = in + at;
                if (curQ == NULL)
                {
                    rc = SC_ERR_ARG;
                    break;
                }
                NL_CURVE* r = null_crv(&S);
                NL_FLAG err = N_CrvSumDiffCrv(cur, curQ, (NL_FLAG)prm[0], r, &S);
                status_and_crv(o, err, r);
                break;
            }
            case 7: // N_CrvNonRatGetFirstDeriv: d
            {
                const double* prm = in + at;
                NL_CURVE* q = null_crv(&S);
                NL_FLAG err = N_CrvNonRatGetFirstDeriv(cur, (NL_INDEX)prm[0], q, &S);
                status_and_crv(o, err, q);
                break;
            }
            case 8: // N_CrvRatGetFirstDeriv
            {
                NL_CURVE* q = null_crv(&S);
                NL_FLAG err = N_CrvRatGetFirstDeriv(cur, q, &S);
                status_and_crv(o, err, q);
                break;
            }
            case 9: // N_ConstantMultiplyCrv: alpha
            {
                N_ConstantMultiplyCrv(in[at], cur);
                o.push_back(0.0);
                dump_crv(o, cur);
                break;
            }
            case 10: // N_LinearMultiplyCrv: s0 s1
            {
                N_LinearMultiplyCrv(in[at], in[at + 1], cur);
                o.push_back(0.0);
                dump_crv(o, cur);
                break;
            }
            case 11: // N_ConstantMultiplyCrv4d: alpha
            {
                N_ConstantMultiplyCrv4d(in[at], cur);
                o.push_back(0.0);
                dump_crv(o, cur);
                break;
            }
            case 12: // N_CrvReparamRat: alf bet gam del inplace
            {
                const double* prm = in + at;
                NL_CURVE* q = out_crv(cur, prm[4], &S);
                NL_FLAG err = N_CrvReparamRat(cur, prm[0], prm[1], prm[2], prm[3], q, &S);
                status_and_crv(o, err, q);
                break;
            }
            case 13: // N_CrvReparamWeights: w c d (in place)
            {
                const double* prm = in + at;
                NL_FLAG err = N_CrvReparamWeights(cur, prm[0], prm[1], prm[2]);
                o.push_back(nl_status(err));
                dump_crv(o, cur);
                break;
            }
            case 14: // N_SrfReparamMultKnots: tol inplace
            {
                const double* prm = in + at;
                NL_CURVE* q = out_crv(cur, prm[1], &S);
                NL_FLAG err = N_SrfReparamMultKnots(cur, prm[0], q, &S);
                status_and_crv(o, err, q);
                break;
            }
            case 15: // N_SrfReparamWeights: wt0 wtn (in place)
            {
                const double* prm = in + at;
                NL_FLAG err = N_SrfReparamWeights(cur, prm[0], prm[1]);
                o.push_back(nl_status(err));
                dump_crv(o, cur);
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
