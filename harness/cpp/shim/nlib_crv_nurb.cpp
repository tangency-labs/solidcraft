// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.crv_nurb`.
// Wraps the NL_CURVE routines of upstream source/SMLib/src/NL_CrvNurb.cpp owned by the unit:
// N_CrvGetGrevilleAbscissa, N_CrvEvalCurvature, N_CrvEval, N_CrvGetBBox, N_CrvDerivs,
// N_CrvEvalTangent, N_CrvEvalFrenetFrame, N_CrvTransform, N_CrvGetMinMaxWeightsAndPts,
// N_CrvLineFromPtAndVector, N_CrvScale, N_CrvScaleCPts, N_CrvTranslate,
// N_CrvProjectOntoPlane, N_CrvReverse, N_CrvGetBBoxMaxDiagDist, N_CrvGetMaxPosVector,
// N_CrvGetLinearSegs, N_CrvGetDegenSegs, N_CrvReparam, N_CrvGetType,
// N_CrvGetCurvatureDeriv, N_CrvIsClosedContinuity.
//
// One entry point, sc_nlib_crv_nurb_eval(op, in, n_in, out, cap, out_len): `op` selects
// the routine, `in` holds a curve (every op but 9, N_CrvLineFromPtAndVector) then the
// routine's parameters, `out` receives a flat record of doubles (layouts in
// crates/difftest/src/units/nlib_crv_nurb.rs). The record starts with the routine's
// status: 0 = ok, else NL_ERROR.eno (cleared before the call; 1000 if the flag was set
// without a number); outputs follow only on success. Routines without a status (void)
// always report 0.
//
// A curve in `in` is: n p m, then n+1 control points (x y z w), m+1 knots. The Rust
// wrapper validates the curve (clamped, well-formed) and the parameters so upstream
// only touches valid memory.
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
void push_pt(std::vector<double>& o, NL_POINT P)
{
    o.push_back(P.x);
    o.push_back(P.y);
    o.push_back(P.z);
}

NL_POINT pt(const double* d)
{
    NL_POINT P;
    P.x = d[0];
    P.y = d[1];
    P.z = d[2];
    return P;
}

// Output curve: the input itself (in place) or a new NULL curve.
NL_CURVE* out_crv(NL_CURVE* cur, double inplace, NL_STACKS* S)
{
    return inplace != 0.0 ? cur : null_crv(S);
}

NL_FLAG side(double s)
{
    return s == 2.0 ? NL_RIGHT : NL_LEFT;
}
} // namespace

SC_EXPORT int32_t sc_nlib_crv_nurb_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                        int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int64_t at = 0;
    int32_t rc = SC_OK;
    NL_CURVE* cur = NULL;
    if (op != 9)
        cur = read_crv(in, &at, &S);
    const double* prm = in + at;
    if (op != 9 && cur == NULL)
    {
        rc = SC_ERR_ARG;
    }
    else
    {
        N_ErrClear();
        switch (op)
        {
            case 0: // N_CrvGetGrevilleAbscissa (curve knots): deg idx
            {
                NL_INDEX m;
                NL_REAL* U;
                N_CrvGetKnots(cur, &m, &U);
                NL_REAL g = 0.0;
                NL_FLAG err = N_CrvGetGrevilleAbscissa(U, m, (NL_DEGREE)prm[0], (NL_INDEX)prm[1], &g);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    o.push_back(g);
                break;
            }
            case 1: // N_CrvEvalCurvature: u side cfl
            {
                NL_REAL kap = 0.0;
                NL_POINT P, C;
                NL_VECTOR T, N;
                NL_FLAG cfl = prm[2] != 0.0 ? NL_YES : NL_NO;
                NL_FLAG err = N_CrvEvalCurvature(cur, prm[0], side(prm[1]), &kap, &P, &T, cfl, &C, &N);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                {
                    o.push_back(kap);
                    push_pt(o, P);
                    push_pt(o, T);
                    if (cfl == NL_YES)
                    {
                        push_pt(o, C);
                        push_pt(o, N);
                    }
                }
                break;
            }
            case 2: // N_CrvEval: u side
            {
                NL_POINT C;
                NL_FLAG err = N_CrvEval(cur, prm[0], side(prm[1]), &C);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    push_pt(o, C);
                break;
            }
            case 3: // N_CrvGetBBox
            {
                NL_MINMAXBOX box;
                NL_FLAG err = N_CrvGetBBox(cur, &box);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                {
                    o.push_back(box.xl);
                    o.push_back(box.xr);
                    o.push_back(box.yb);
                    o.push_back(box.yt);
                    o.push_back(box.zn);
                    o.push_back(box.zf);
                }
                break;
            }
            case 4: // N_CrvDerivs: u side der
            {
                NL_INDEX der = (NL_INDEX)prm[2];
                std::vector<NL_POINT> CD((size_t)der + 1);
                NL_FLAG err = N_CrvDerivs(cur, prm[0], side(prm[1]), der, CD.data());
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    for (NL_INDEX k = 0; k <= der; k++)
                        push_pt(o, CD[(size_t)k]);
                break;
            }
            case 5: // N_CrvEvalTangent: u side
            {
                NL_POINT P;
                NL_VECTOR T;
                NL_FLAG err = N_CrvEvalTangent(cur, prm[0], side(prm[1]), &P, &T);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                {
                    push_pt(o, P);
                    push_pt(o, T);
                }
                break;
            }
            case 6: // N_CrvEvalFrenetFrame: u side
            {
                NL_POINT P;
                NL_VECTOR T, N, B;
                NL_FLAG err = N_CrvEvalFrenetFrame(cur, prm[0], side(prm[1]), &P, &T, &N, &B);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                {
                    push_pt(o, P);
                    push_pt(o, T);
                    push_pt(o, N);
                    push_pt(o, B);
                }
                break;
            }
            case 7: // N_CrvTransform: M[16] (row major)
            {
                std::vector<double> mdata(prm, prm + 16);
                double* mrows[4] = {mdata.data(), mdata.data() + 4, mdata.data() + 8, mdata.data() + 12};
                NL_RMATRIX M;
                N_CreateRealMatrix(&M, 3, 3, mrows, NL_MT_FULL, 3);
                N_CrvTransform(cur, &M);
                o.push_back(0.0);
                dump_crv(o, cur);
                break;
            }
            case 8: // N_CrvGetMinMaxWeightsAndPts
            {
                NL_REAL wmin = 0.0, wmax = 0.0, pmin = 0.0, pmax = 0.0;
                N_CrvGetMinMaxWeightsAndPts(cur, &wmin, &wmax, &pmin, &pmax);
                o.push_back(0.0);
                o.push_back(wmin);
                o.push_back(wmax);
                o.push_back(pmin);
                o.push_back(pmax);
                break;
            }
            case 9: // N_CrvLineFromPtAndVector: P[3] V[3] (no curve)
            {
                NL_CURVE* q = null_crv(&S);
                NL_FLAG err = N_CrvLineFromPtAndVector(pt(prm), pt(prm + 3), q, &S);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    dump_crv(o, q);
                break;
            }
            case 10: // N_CrvScale: C[3] f[3]
            {
                N_CrvScale(cur, pt(prm), pt(prm + 3));
                o.push_back(0.0);
                dump_crv(o, cur);
                break;
            }
            case 11: // N_CrvScaleCPts: f
            {
                N_CrvScaleCPts(cur, prm[0]);
                o.push_back(0.0);
                dump_crv(o, cur);
                break;
            }
            case 12: // N_CrvTranslate: T[3]
            {
                N_CrvTranslate(cur, pt(prm));
                o.push_back(0.0);
                dump_crv(o, cur);
                break;
            }
            case 13: // N_CrvProjectOntoPlane: O[3] N[3] E[3] prj inplace
            {
                NL_CURVE* q = out_crv(cur, prm[10], &S);
                NL_FLAG prj = prm[9] == 2.0 ? NL_PERSPECTIVE : NL_PARALLEL;
                NL_FLAG err = N_CrvProjectOntoPlane(cur, pt(prm), pt(prm + 3), pt(prm + 6), prj, q, &S);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    dump_crv(o, q);
                break;
            }
            case 14: // N_CrvReverse: inplace
            {
                NL_CURVE* q = out_crv(cur, prm[0], &S);
                NL_FLAG err = N_CrvReverse(cur, q, &S);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    dump_crv(o, q);
                break;
            }
            case 15: // N_CrvGetBBoxMaxDiagDist
            {
                NL_REAL d = 0.0;
                NL_FLAG err = N_CrvGetBBoxMaxDiagDist(cur, &d);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    o.push_back(d);
                break;
            }
            case 16: // N_CrvGetMaxPosVector
            {
                NL_POINT P;
                NL_REAL mag = 0.0;
                N_CrvGetMaxPosVector(cur, &P, &mag);
                o.push_back(0.0);
                push_pt(o, P);
                o.push_back(mag);
                break;
            }
            case 17: // N_CrvGetLinearSegs: tol
            {
                NL_REAL** segs = NULL;
                NL_FLAG* linf = NULL;
                NL_INDEX nn = -1;
                NL_FLAG err = N_CrvGetLinearSegs(cur, prm[0], &segs, &linf, &nn, &S);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                {
                    o.push_back((double)(nn + 1));
                    for (NL_INDEX i = 0; i <= nn; i++)
                    {
                        o.push_back(segs[i][0]);
                        o.push_back(segs[i][1]);
                        o.push_back(linf[i] == NL_YES ? 1.0 : 0.0);
                    }
                }
                break;
            }
            case 18: // N_CrvGetDegenSegs: tol
            {
                NL_REAL** segs = NULL;
                NL_INDEX nn = -1;
                NL_FLAG err = N_CrvGetDegenSegs(cur, prm[0], &segs, &nn, &S);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                {
                    o.push_back((double)(nn + 1));
                    for (NL_INDEX i = 0; i <= nn; i++)
                    {
                        o.push_back(segs[i][0]);
                        o.push_back(segs[i][1]);
                    }
                }
                break;
            }
            case 19: // N_CrvReparam: ul ur
            {
                N_CrvReparam(cur, prm[0], prm[1]);
                o.push_back(0.0);
                dump_crv(o, cur);
                break;
            }
            case 20: // N_CrvGetType: tol
            {
                NL_POINT AA;
                NL_VECTOR BB, XX, YY;
                NL_REAL r = 0.0, al = 0.0;
                NL_FLAG typ = NL_NFREEFORM;
                NL_FLAG err = N_CrvGetType(cur, prm[0], &AA, &BB, &XX, &YY, &r, &al, &typ);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                {
                    o.push_back((double)typ);
                    if (typ == NL_NLINE)
                    {
                        push_pt(o, AA);
                        push_pt(o, BB);
                    }
                    else if (typ == NL_NCIRCLE)
                    {
                        push_pt(o, AA);
                        push_pt(o, BB);
                        push_pt(o, XX);
                        push_pt(o, YY);
                        o.push_back(r);
                        o.push_back(al);
                    }
                }
                break;
            }
            case 21: // N_CrvGetCurvatureDeriv: u side
            {
                NL_REAL kpp = 0.0;
                NL_FLAG err = N_CrvGetCurvatureDeriv(cur, prm[0], side(prm[1]), &kpp);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    o.push_back(kpp);
                break;
            }
            case 22: // N_CrvIsClosedContinuity: n tols[3]
            {
                NL_REAL tols[3] = {prm[1], prm[2], prm[3]};
                NL_FLAG gn = NL_NO;
                NL_FLAG err = N_CrvIsClosedContinuity(cur, (NL_INDEX)prm[0], tols, &gn);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    o.push_back(gn == NL_YES ? 1.0 : 0.0);
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
