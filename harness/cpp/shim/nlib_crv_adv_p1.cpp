// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.crv_adv_p1`.
// Wraps the NL_CURVE routines of upstream source/SMLib/src/NL_CrvAdv.cpp owned by the unit:
// N_CrvEvalPt, N_CrvDerivsAtKnot, N_CrvEvalEvenSpacedPts, N_CrvGetMax2ndDeriv,
// N_CrvPlanarOffsetGetDeriv, and the file-private ST_curxcdr / ST_curxcrr through their
// public one-line wrappers N_CrvExtendByDist / N_CrvExtendByParamDist (depth 0).
//
// One entry point, sc_nlib_crv_adv_p1_eval(op, in, n_in, out, cap, out_len): `op`
// selects the routine, `in` holds a curve then the routine's parameters, `out` receives
// a flat record of doubles (layouts in crates/difftest/src/units/nlib_crv_adv_p1.rs).
// The record starts with the routine's status: 0 = ok, else NL_ERROR.eno (cleared
// before the call; 1000 if the flag was set without a number); outputs follow only on
// success.
//
// A curve in `in` is: n p m, then n+1 control points (x y z w), m+1 knots. It is
// allocated on one NL_STACKS with the outputs, so the in-place extensions (output curve
// == input curve) can free the input's arrays. The Rust wrapper validates the curve
// (clamped, well-formed) and the parameters so upstream only touches valid memory.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>
#include <NL_CrvAdv.h>

#include <vector>

#include "sc_nlib.h"

using sc::dump_crv;
using sc::nl_status;
using sc::null_crv;
using sc::read_crv;

namespace
{

// Output curve: the input itself (in place) or a new NULL curve.
NL_CURVE* out_crv(NL_CURVE* cur, double inplace, NL_STACKS* S)
{
    return inplace != 0.0 ? cur : null_crv(S);
}
} // namespace

SC_EXPORT int32_t sc_nlib_crv_adv_p1_eval(int32_t op, const double* in, int64_t n_in, double* out,
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
            case 0: // N_CrvEvalPt: u flg
            {
                NL_CPOINT Cw;
                NL_FLAG err = N_CrvEvalPt(cur, prm[0], (NL_FLAG)prm[1], &Cw);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                {
                    o.push_back(Cw.x);
                    o.push_back(Cw.y);
                    o.push_back(Cw.z);
                    o.push_back(Cw.w);
                }
                break;
            }
            case 1: // N_CrvDerivsAtKnot: u flg der
            {
                NL_INDEX der = (NL_INDEX)prm[2];
                std::vector<NL_CPOINT> Dw((size_t)der + 1);
                NL_FLAG err = N_CrvDerivsAtKnot(cur, prm[0], (NL_FLAG)prm[1], der, Dw.data());
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    for (NL_INDEX k = 0; k <= der; k++)
                    {
                        o.push_back(Dw[k].x);
                        o.push_back(Dw[k].y);
                        o.push_back(Dw[k].z);
                        o.push_back(Dw[k].w);
                    }
                break;
            }
            case 2: // N_CrvEvalEvenSpacedPts: u0 u1 n tol
            {
                NL_INDEX n = (NL_INDEX)prm[2];
                size_t cnt = n >= 0 ? (size_t)n + 1 : 1;
                std::vector<NL_POINT> P(cnt);
                std::vector<NL_PARAMETER> u(cnt);
                NL_FLAG err = N_CrvEvalEvenSpacedPts(cur, prm[0], prm[1], n, prm[3], P.data(), u.data());
                o.push_back(nl_status(err));
                if (err == NL_NO)
                {
                    for (NL_INDEX k = 0; k <= n; k++)
                    {
                        o.push_back(P[k].x);
                        o.push_back(P[k].y);
                        o.push_back(P[k].z);
                    }
                    for (NL_INDEX k = 0; k <= n; k++)
                        o.push_back(u[k]);
                }
                break;
            }
            case 3: // N_CrvGetMax2ndDeriv
            {
                NL_REAL Muu = 0.0;
                NL_FLAG err = N_CrvGetMax2ndDeriv(cur, &Muu);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    o.push_back(Muu);
                break;
            }
            case 4: // N_CrvPlanarOffsetGetDeriv: u d N[3] flg
            {
                NL_VECTOR N;
                N.x = prm[2];
                N.y = prm[3];
                N.z = prm[4];
                NL_POINT OD[3];
                NL_FLAG err = N_CrvPlanarOffsetGetDeriv(cur, prm[0], prm[1], N, (NL_FLAG)prm[5], OD);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    for (int k = 1; k <= 2; k++)
                    {
                        o.push_back(OD[k].x);
                        o.push_back(OD[k].y);
                        o.push_back(OD[k].z);
                    }
                break;
            }
            case 5: // N_CrvExtendByDist (ST_curxcdr): dist flg end cont inplace
            {
                NL_CURVE* q = out_crv(cur, prm[4], &S);
                NL_FLAG err = N_CrvExtendByDist(cur, prm[0], (NL_FLAG)prm[1], (NL_FLAG)prm[2],
                                                (NL_FLAG)prm[3], q, &S, &S);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    dump_crv(o, q);
                break;
            }
            case 6: // N_CrvExtendByParamDist (ST_curxcrr): par end cont inplace
            {
                NL_CURVE* q = out_crv(cur, prm[3], &S);
                NL_FLAG err =
                    N_CrvExtendByParamDist(cur, prm[0], (NL_FLAG)prm[1], (NL_FLAG)prm[2], q, &S, &S);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    dump_crv(o, q);
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
