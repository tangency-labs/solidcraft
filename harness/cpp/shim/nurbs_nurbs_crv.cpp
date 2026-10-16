// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nurbs.nurbs_crv`.
// Wraps the sm_* curve routines of upstream source/SMLib/src/SmNurbsCrv.cpp owned by the unit:
// sm_ExtractFrame, sm_GetKnots, sm_CopyNurbCurve, sm_AllocateNurbCurve,
// sm_AllocateAndCopyNurbCurve, sm_FreeNurbCurve, sm_InitNurbCurveMemory,
// sm_ComputeNurbCurveSize, sm_FixupKnotVector, sm_CreateBezSegments, sm_CrvSplit,
// sm_CrvTrim, sm_CrvGetEPolygon, sm_BasisDerivs, sm_CrvRationalBasisDerivs,
// sm_CrvDerivs, sm_BasisEval, sm_CrvEval, sm_CrvRemoveKnots, sm_BasisFindAllSpanMaxima,
// sm_BasisIEval, sm_CrvRemoveKnotMaxErr, sm_BasisIDerivs, sm_BasisFindGlobalMax,
// sm_IsNCrvDegenerate (SmStackHandler via the NLib stack of sm_CrvRemoveKnots).
//
// One entry point, sc_nurbs_nurbs_crv_eval(op, in, n_in, out, cap, out_len): `op`
// selects the routine, `in` holds its inputs (layouts in
// crates/solidcore-ffi/src/units/nurbs_nurbs_crv.rs), `out` receives a flat record of
// doubles: the routine's returned flag (0 for routines without one), then the outputs
// (only when the flag is 0, except for sm_CrvRemoveKnots, whose curve always follows).
// The Rust wrapper validates every input so upstream only touches valid memory.
#include "sc_shim.h"

#include <nurbs.h>
#include <SmNurbsCrv.h>
#include <SmTArray.h>
#include <SmExtent1d.h>
#include <SmAxis2Placement.h>

#include <vector>

#include "sc_nlib.h"

using sc::read_crv;

namespace
{
// n p m, used control points, used knots.
void dump_crv(std::vector<double>& o, const NL_CURVE* cur)
{
    NL_INDEX n = cur->pol->n;
    NL_INDEX m = cur->knt->m;
    o.push_back((double)n);
    o.push_back((double)cur->p);
    o.push_back((double)m);
    for (NL_INDEX i = 0; i <= n; i++)
    {
        o.push_back(cur->pol->Pw[i].x);
        o.push_back(cur->pol->Pw[i].y);
        o.push_back(cur->pol->Pw[i].z);
        o.push_back(cur->pol->Pw[i].w);
    }
    for (NL_INDEX i = 0; i <= m; i++)
        o.push_back(cur->knt->U[i]);
}

void push_pt(std::vector<double>& o, const NL_POINT& P)
{
    o.push_back(P.x);
    o.push_back(P.y);
    o.push_back(P.z);
}

NL_FLAG side(double s)
{
    return s == 2.0 ? NL_RIGHT : NL_LEFT;
}

int64_t idx(double v)
{
    return (int64_t)v;
}
} // namespace

SC_EXPORT int32_t sc_nurbs_nurbs_crv_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                          int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int64_t at = 0;
    int32_t rc = SC_OK;
    // Ops 0 (knots), 2 (fixup) and 19 (frame) take no curve.
    NL_CURVE* cur = NULL;
    if (op != 0 && op != 2 && op != 19)
    {
        cur = read_crv(in, &at, &S);
        if (cur == NULL)
        {
            N_EndNurbs(&S);
            return SC_ERR_ARG;
        }
    }
    const double* prm = in + at;
    switch (op)
    {
    case 0: // sm_GetKnots: nk knots... has_ivl a b
    {
        int64_t nk = idx(prm[0]);
        std::vector<double> U(prm + 1, prm + 1 + nk);
        const double* q = prm + 1 + nk;
        NL_KNOTVECTOR knt;
        knt.m = (NL_INDEX)(nk - 1);
        knt.U = U.data();
        SmTArray<double> knots;
        SmTArray<ULONG> mults;
        SmStatus st;
        if (q[0] != 0.0)
        {
            SmExtent1d ivl(q[1], q[2]);
            st = sm_GetKnots(&knt, knots, &mults, &ivl);
        }
        else
            st = sm_GetKnots(&knt, knots, &mults);
        o.push_back(st == SM_SUCCESS ? 0.0 : (double)st);
        o.push_back((double)knots.GetSize());
        for (ULONG i = 0; i < knots.GetSize(); i++)
            o.push_back(knots[i]);
        for (ULONG i = 0; i < mults.GetSize(); i++)
            o.push_back((double)mults[i]);
        break;
    }
    case 1: // sm_AllocateAndCopyNurbCurve, then sm_CopyNurbCurve into a fresh curve
    {
        NL_CURVE* c1 = sm_AllocateAndCopyNurbCurve(cur);
        NL_CURVE* c2 = sm_AllocateNurbCurve(c1->pol->n, c1->p, c1->knt->m);
        SmStatus st = sm_CopyNurbCurve(c1, c2);
        o.push_back(st == SM_SUCCESS ? 0.0 : (double)st);
        dump_crv(o, c2);
        // sm_ComputeNurbCurveSize is the allocation size of both; exercised here only.
        (void)sm_ComputeNurbCurveSize(c2);
        sm_FreeNurbCurve(c2);
        sm_FreeNurbCurve(c1);
        break;
    }
    case 2: // sm_FixupKnotVector: nk knots... tol
    {
        int64_t nk = idx(prm[0]);
        std::vector<double> U(prm + 1, prm + 1 + nk);
        NL_KNOTVECTOR knt;
        knt.m = (NL_INDEX)(nk - 1);
        knt.U = U.data();
        sm_FixupKnotVector(&knt, SmTol2d(prm[1 + nk]));
        o.push_back(0.0);
        for (double u : U)
            o.push_back(u);
        break;
    }
    case 3: // sm_CreateBezSegments
    {
        NL_INDEX nsp = 0;
        N_BasisGetSpanCount(cur->knt, cur->p, &nsp);
        SmTArray<gw_CURVE*> segs;
        segs.SetSize(nsp);
        for (NL_INDEX i = 0; i < nsp; i++)
            segs[i] = sm_AllocateNurbCurve(cur->p, cur->p, 2 * cur->p + 1);
        gw_FLAG f = sm_CreateBezSegments(cur, segs);
        o.push_back((double)f);
        if (f == 0)
        {
            o.push_back((double)nsp);
            for (NL_INDEX i = 0; i < nsp; i++)
                dump_crv(o, segs[i]);
        }
        for (NL_INDEX i = 0; i < nsp; i++)
            sm_FreeNurbCurve(segs[i]);
        break;
    }
    case 4: // sm_CrvSplit: u
    {
        gw_CURVE* L = NULL;
        gw_CURVE* R = NULL;
        gw_FLAG f = sm_CrvSplit(cur, prm[0], L, R);
        o.push_back((double)f);
        if (f == 0)
        {
            dump_crv(o, L);
            dump_crv(o, R);
        }
        sm_FreeNurbCurve(L);
        sm_FreeNurbCurve(R);
        break;
    }
    case 5: // sm_CrvTrim: ul ur
    {
        gw_CURVE* Q = NULL;
        gw_FLAG f = sm_CrvTrim(cur, prm[0], prm[1], Q);
        o.push_back((double)f);
        if (f == 0)
            dump_crv(o, Q);
        sm_FreeNurbCurve(Q);
        break;
    }
    case 6: // sm_CrvGetEPolygon: k l
    {
        NL_INDEX k = (NL_INDEX)idx(prm[0]);
        NL_INDEX l = (NL_INDEX)idx(prm[1]);
        std::vector<NL_POINT> P((size_t)(cur->pol->n + 1));
        gw_EPOLYGON ppl;
        gw_FLAG f = sm_CrvGetEPolygon(cur, k, l, &ppl, P.data());
        o.push_back((double)f);
        if (f == 0)
        {
            o.push_back((double)(ppl.n + 1));
            for (NL_INDEX i = 0; i <= ppl.n; i++)
                push_pt(o, ppl.P[i]);
        }
        break;
    }
    case 7: // sm_BasisDerivs: u side der
    case 8: // sm_CrvRationalBasisDerivs: u side der
    {
        NL_INDEX der = (NL_INDEX)idx(prm[2]);
        gw_REAL ND_STACK[GW_MAX_DERIV][GW_MAX_DEGREE];
        gw_REAL* ND[GW_MAX_DEGREE];
        for (NL_INDEX i = 0; i <= der; i++)
            ND[i] = ND_STACK[i];
        gw_INDEX spn = 0;
        gw_FLAG f = op == 7 ? sm_BasisDerivs(cur->knt, cur->p, prm[0], side(prm[1]), der, ND, &spn)
                            : sm_CrvRationalBasisDerivs(cur, prm[0], side(prm[1]), der, ND, &spn);
        o.push_back((double)f);
        if (f == 0)
        {
            o.push_back((double)spn);
            for (NL_INDEX k = 0; k <= der; k++)
                for (NL_INDEX j = 0; j <= cur->p; j++)
                    o.push_back(ND[k][j]);
        }
        break;
    }
    case 9: // sm_CrvDerivs: u side der
    {
        NL_INDEX der = (NL_INDEX)idx(prm[2]);
        NL_POINT CD[GW_MAX_DERIV + 4];
        gw_FLAG f = sm_CrvDerivs(cur, prm[0], side(prm[1]), der, CD);
        o.push_back((double)f);
        if (f == 0)
            for (NL_INDEX k = 0; k <= der; k++)
                push_pt(o, CD[k]);
        break;
    }
    case 10: // sm_BasisEval: u side
    {
        gw_REAL N[GW_MAX_DEGREE];
        gw_INDEX spn = 0;
        gw_FLAG f = sm_BasisEval(cur->knt, cur->p, prm[0], side(prm[1]), N, &spn);
        o.push_back((double)f);
        if (f == 0)
        {
            o.push_back((double)spn);
            for (NL_INDEX j = 0; j <= cur->p; j++)
                o.push_back(N[j]);
        }
        break;
    }
    case 11: // sm_CrvEval: u side
    {
        NL_POINT C;
        gw_FLAG f = sm_CrvEval(cur, prm[0], side(prm[1]), &C);
        o.push_back((double)f);
        if (f == 0)
            push_pt(o, C);
        break;
    }
    case 12: // sm_CrvRemoveKnots (in place): tol
    {
        gw_FLAG f = sm_CrvRemoveKnots(cur, prm[0], cur, &S);
        o.push_back((double)f);
        dump_crv(o, cur);
        break;
    }
    case 13: // sm_BasisFindAllSpanMaxima: i tol
    {
        gw_REAL mn[GW_MAX_DEGREE + 1], mx[GW_MAX_DEGREE + 1], u = 0.0;
        gw_FLAG f =
            sm_BasisFindAllSpanMaxima(cur->knt, (gw_INDEX)idx(prm[0]), cur->p, prm[1], mn, mx, &u);
        o.push_back((double)f);
        if (f == 0)
        {
            for (NL_INDEX j = 0; j <= cur->p; j++)
                o.push_back(mn[j]);
            for (NL_INDEX j = 0; j <= cur->p; j++)
                o.push_back(mx[j]);
            o.push_back(u);
        }
        break;
    }
    case 14: // sm_BasisIEval: i u side
    {
        gw_REAL N = 0.0;
        gw_FLAG f =
            sm_BasisIEval(cur->knt, (gw_INDEX)idx(prm[0]), cur->p, prm[1], side(prm[2]), &N);
        o.push_back((double)f);
        if (f == 0)
            o.push_back(N);
        break;
    }
    case 15: // sm_CrvRemoveKnotMaxErr: r s
    {
        gw_REAL br = 0.0;
        gw_FLAG f =
            sm_CrvRemoveKnotMaxErr(cur, (gw_INDEX)idx(prm[0]), (gw_INDEX)idx(prm[1]), &br);
        o.push_back((double)f);
        if (f == 0)
            o.push_back(br);
        break;
    }
    case 16: // sm_BasisIDerivs: i u side der
    {
        NL_INDEX der = (NL_INDEX)idx(prm[3]);
        std::vector<gw_REAL> ND((size_t)der + 1);
        gw_FLAG f = sm_BasisIDerivs(cur->knt, (gw_INDEX)idx(prm[0]), cur->p, prm[1], side(prm[2]),
                                    der, ND.data());
        o.push_back((double)f);
        if (f == 0)
            for (gw_REAL v : ND)
                o.push_back(v);
        break;
    }
    case 17: // sm_BasisFindGlobalMax: i tol
    {
        gw_REAL mx = 0.0, u = 0.0;
        gw_FLAG f = sm_BasisFindGlobalMax(cur->knt, (gw_INDEX)idx(prm[0]), cur->p, prm[1], &mx, &u);
        o.push_back((double)f);
        if (f == 0)
        {
            o.push_back(mx);
            o.push_back(u);
        }
        break;
    }
    case 18: // sm_IsNCrvDegenerate: tol
    {
        SmBoolean b = sm_IsNCrvDegenerate(cur, prm[0]);
        o.push_back(0.0);
        o.push_back(b ? 1.0 : 0.0);
        break;
    }
    case 19: // sm_ExtractFrame: origin[3] x[3] y[3] (through SmAxis2Placement)
    {
        SmAxis2Placement frame(SmPoint3d(prm[0], prm[1], prm[2]), SmVector3d(prm[3], prm[4], prm[5]),
                               SmVector3d(prm[6], prm[7], prm[8]));
        NL_POINT O, X, Y;
        sm_ExtractFrame(frame, O, X, Y);
        o.push_back(0.0);
        push_pt(o, O);
        push_pt(o, X);
        push_pt(o, Y);
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
