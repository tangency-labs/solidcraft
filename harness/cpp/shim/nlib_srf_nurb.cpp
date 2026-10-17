// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.srf_nurb`.
// Wraps the NL_SURFACE routines of upstream source/SMLib/src/NL_SrfNurb.cpp owned by the
// unit: N_SrfBasisDerivs (with N_ComputeBasisFunctions, N_ComputeDerivatives[Impl],
// N_SwapRealPointers, NL_BASISDERIVATIVES::operator[]), N_SrfEvalPt[Impl], N_SrfDerivs
// (with N_ComputeDerivativeMatrix[Spec]), N_SrfGetBBox, N_SrfTransform,
// N_SrfMinMaxWeightPosVectors, N_SrfEvalPtPtDerivNormalFast, N_SrfTranslate,
// N_SrfRotateAtPt, N_SrfReverse, N_SrfType, N_SrfReparam, N_SrfIsClosedSmooth,
// N_SrfEvalPtPtDerivNormalPole, N_SrfEvalPtNormalAtPole, N_ConeEllipticEnds.
//
// One entry point, sc_nlib_srf_nurb_eval(op, in, n_in, out, cap, out_len): `op` selects
// the routine, `in` holds a surface (every op but 15, N_ConeEllipticEnds) then the
// routine's parameters, `out` receives a flat record of doubles: the status (0 = ok,
// else NL_ERROR.eno, cleared before the call; 1000 if the flag was set without a
// number; void routines report 0), then, on success only, the results.
//
// A surface in `in` is: n m p q, then (n+1)*(m+1) control points (x y z w, row-major),
// n+p+2 u knots, m+q+2 v knots (storage = indices in use). The Rust wrapper validates
// it (valid clamped knot vectors, degrees 1..=NL_MAXDEG) and every parameter.
//
// A surface is dumped as n m p q r s, the control points, the u and v knots; a curve
// as n p m, the control points, the knots.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

#include <memory>
#include <vector>

#include "sc_nlib.h"

using sc::dump_crv;
using sc::dump_srf;
using sc::nl_status;
using sc::null_srf;
using sc::read_srf;

namespace
{

void push_pt(std::vector<double>& o, const NL_POINT& p)
{
    o.push_back(p.x);
    o.push_back(p.y);
    o.push_back(p.z);
}

NL_POINT pt(const double* d)
{
    NL_POINT p;
    p.x = d[0];
    p.y = d[1];
    p.z = d[2];
    return p;
}

// Zero-filled SD[0..=udr][0..=vdr].
struct Sd
{
    std::vector<NL_POINT> data;
    std::vector<NL_POINT*> rows;
    Sd(NL_INDEX udr, NL_INDEX vdr) : data((size_t)((udr + 1) * (vdr + 1))), rows((size_t)(udr + 1))
    {
        for (auto& p : data)
            p.x = p.y = p.z = 0.0;
        for (NL_INDEX i = 0; i <= udr; i++)
            rows[(size_t)i] = &data[(size_t)(i * (vdr + 1))];
    }
};

void push_pdn(std::vector<double>& o, const NL_POINT& P, const NL_VECTOR& SU, const NL_VECTOR& SV,
              const NL_VECTOR& N)
{
    push_pt(o, P);
    push_pt(o, SU);
    push_pt(o, SV);
    push_pt(o, N);
}
} // namespace

// op 0 basis derivs: u v ufl vfl mfl udr vdr -> status, usp vsp, BD[k][l][i][j].
// op 1 eval point: u v ufl vfl -> status, x y z.
// op 2 derivs: u v ufl vfl mfl udr vdr -> status, SD[k][l] (x y z).
// op 3 bbox -> status, xl xr yb yt zn zf.
// op 4 transform: M[16] (row major) -> status, surface.
// op 5 min-max weights / position vectors -> status, wmin wmax pmin pmax.
// op 6 point/derivs/normal (fast): u v ufl vfl -> status, P SU SV N.
// op 7 translate: T -> status, surface.
// op 8 rotate: P V al -> status, surface.
// op 9 reverse: dir inplace -> status, surface.
// op 10 type: tol -> status, stp dir AA BB CC DD XX YY r1 r2 ac al h, hasA [curA], hasB [curB].
// op 11 reparam: us ue vs ve -> status, surface.
// op 12 closed smooth: n t0 t1 dflg -> status, gnflg.
// op 13 point/derivs/normal at pole: u v ufl vfl -> status, P SU SV N.
// op 14 point/normal at pole (caller SD): u v ufl vfl -> status, P SU SV N.
// op 15 cone with elliptic ends (no surface input): rf rr h tyf tyr txf txr -> status, surface.
SC_EXPORT int32_t sc_nlib_srf_nurb_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                        int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int64_t at = 0;
    int32_t rc = SC_OK;
    NL_SURFACE* sur = NULL;
    if (op != 15)
    {
        sur = read_srf(in, &at, &S);
        if (sur == NULL)
            rc = SC_ERR_ARG;
    }
    const double* prm = in + at;
    NL_FLAG err = NL_NO;
    N_ErrClear();
    switch (rc == SC_OK ? op : -1)
    {
        case 0:
        {
            NL_INDEX udr = (NL_INDEX)prm[5], vdr = (NL_INDEX)prm[6];
            std::unique_ptr<NL_BASISDERIVATIVES> BD(new NL_BASISDERIVATIVES());
            NL_INDEX usp = -1, vsp = -1;
            err = N_SrfBasisDerivs(sur, prm[0], prm[1], (NL_FLAG)prm[2], (NL_FLAG)prm[3], (NL_FLAG)prm[4], udr,
                                   vdr, *BD, &usp, &vsp);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                o.push_back((double)usp);
                o.push_back((double)vsp);
                NL_BASISDERIVATIVES& bd = *BD;
                for (NL_INDEX k = 0; k <= udr; k++)
                    for (NL_INDEX l = 0; l <= vdr; l++)
                        for (NL_INDEX i = 0; i <= sur->p; i++)
                            for (NL_INDEX j = 0; j <= sur->q; j++)
                                o.push_back(bd[(size_t)k][l][i][j]);
            }
            break;
        }
        case 1:
        {
            NL_POINT P = {0.0, 0.0, 0.0};
            err = N_SrfEvalPt(sur, prm[0], prm[1], (NL_FLAG)prm[2], (NL_FLAG)prm[3], &P);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                push_pt(o, P);
            break;
        }
        case 2:
        {
            NL_INDEX udr = (NL_INDEX)prm[5], vdr = (NL_INDEX)prm[6];
            Sd sd(udr, vdr);
            err = N_SrfDerivs(sur, prm[0], prm[1], (NL_FLAG)prm[2], (NL_FLAG)prm[3], (NL_FLAG)prm[4], udr, vdr,
                              sd.rows.data());
            o.push_back(nl_status(err));
            if (err == NL_NO)
                for (auto& p : sd.data)
                    push_pt(o, p);
            break;
        }
        case 3:
        {
            NL_MINMAXBOX box = {0, 0, 0, 0, 0, 0};
            err = N_SrfGetBBox(sur, &box);
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
        case 4:
        {
            std::vector<double> mdata(prm, prm + 16);
            double* mrows[4] = {mdata.data(), mdata.data() + 4, mdata.data() + 8, mdata.data() + 12};
            NL_RMATRIX M;
            N_CreateRealMatrix(&M, 3, 3, mrows, NL_MT_FULL, 3);
            N_SrfTransform(sur, &M);
            o.push_back(0.0);
            dump_srf(o, sur);
            break;
        }
        case 5:
        {
            NL_REAL wmin = 0.0, wmax = 0.0, pmin = 0.0, pmax = 0.0;
            N_SrfMinMaxWeightPosVectors(sur, &wmin, &wmax, &pmin, &pmax);
            o.push_back(0.0);
            o.push_back(wmin);
            o.push_back(wmax);
            o.push_back(pmin);
            o.push_back(pmax);
            break;
        }
        case 6:
        {
            NL_POINT P;
            NL_VECTOR SU, SV, N;
            Sd sd(1, 1);
            err = N_SrfEvalPtPtDerivNormalFast(sur, prm[0], prm[1], (NL_FLAG)prm[2], (NL_FLAG)prm[3], &P, &SU, &SV,
                                               &N, sd.rows.data());
            o.push_back(nl_status(err));
            if (err == NL_NO)
                push_pdn(o, P, SU, SV, N);
            break;
        }
        case 7:
        {
            N_SrfTranslate(sur, pt(prm));
            o.push_back(0.0);
            dump_srf(o, sur);
            break;
        }
        case 8:
        {
            err = N_SrfRotateAtPt(sur, pt(prm), pt(prm + 3), prm[6]);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_srf(o, sur);
            break;
        }
        case 9:
        {
            bool inplace = prm[1] != 0.0;
            NL_SURFACE* res = inplace ? sur : null_srf(&S);
            if (res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            err = N_SrfReverse(sur, (NL_FLAG)prm[0], res, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_srf(o, res);
            break;
        }
        case 10:
        {
            NL_CURVE *curA = NULL, *curB = NULL;
            NL_POINT AA, BB, CC, DD;
            NL_VECTOR XX, YY;
            NL_REAL r1 = 0.0, r2 = 0.0, ac = 0.0, al = 0.0, h = 0.0;
            NL_FLAG stp = 0, dir = 0;
            err = N_SrfType(sur, prm[0], &curA, &curB, &AA, &BB, &CC, &DD, &XX, &YY, &r1, &r2, &ac, &al, &h, &stp,
                            &dir, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                o.push_back((double)stp);
                o.push_back((double)dir);
                push_pt(o, AA);
                push_pt(o, BB);
                push_pt(o, CC);
                push_pt(o, DD);
                push_pt(o, XX);
                push_pt(o, YY);
                o.push_back(r1);
                o.push_back(r2);
                o.push_back(ac);
                o.push_back(al);
                o.push_back(h);
                o.push_back(curA != NULL ? 1.0 : 0.0);
                if (curA != NULL)
                    dump_crv(o, curA);
                o.push_back(curB != NULL ? 1.0 : 0.0);
                if (curB != NULL)
                    dump_crv(o, curB);
            }
            break;
        }
        case 11:
        {
            N_SrfReparam(sur, prm[0], prm[1], prm[2], prm[3]);
            o.push_back(0.0);
            dump_srf(o, sur);
            break;
        }
        case 12:
        {
            NL_REAL tols[2] = {prm[1], prm[2]};
            NL_FLAG gn = -1;
            err = N_SrfIsClosedSmooth(sur, (NL_INDEX)prm[0], tols, (NL_FLAG)prm[3], &gn);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                o.push_back((double)gn);
            break;
        }
        case 13:
        {
            NL_POINT P;
            NL_VECTOR SU, SV, N;
            err = N_SrfEvalPtPtDerivNormalPole(sur, prm[0], prm[1], (NL_FLAG)prm[2], (NL_FLAG)prm[3], &P, &SU, &SV,
                                               &N);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                push_pdn(o, P, SU, SV, N);
            break;
        }
        case 14:
        {
            NL_POINT P;
            NL_VECTOR SU, SV, N;
            Sd sd(1, 1);
            err = N_SrfEvalPtNormalAtPole(sur, prm[0], prm[1], (NL_FLAG)prm[2], (NL_FLAG)prm[3], &P, &SU, &SV, &N,
                                          sd.rows.data());
            o.push_back(nl_status(err));
            if (err == NL_NO)
                push_pdn(o, P, SU, SV, N);
            break;
        }
        case 15:
        {
            NL_SURFACE* res = null_srf(&S);
            if (res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            err = N_ConeEllipticEnds(prm[0], prm[1], prm[2], prm[3], prm[4], prm[5], prm[6], res, &S);
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
