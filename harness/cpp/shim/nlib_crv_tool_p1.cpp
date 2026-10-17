// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.crv_tool_p1`.
// Wraps the NL_CURVE tools of upstream source/SMLib/src/NL_CrvTool.cpp owned by the unit:
// N_CrvInsertKnot, N_CrvSplit, N_CrvDecomposeBez, N_CrvRefine, N_CrvExtractCrvSeg,
// N_CrvRemoveKnot, N_CrvRemoveKnotMaxErr, N_CrvElevateDegree.
//
// One entry point, sc_nlib_crv_tool_p1_eval(op, in, n_in, out, cap, out_len): `op`
// selects the routine, `in` holds a curve then the routine's parameters, `out` receives
// a flat record of doubles (layouts in crates/difftest/src/units/nlib_crv_tool_p1.rs).
// The record starts with the routine's status: 0 = ok, else NL_ERROR.eno (cleared
// before the call; 1000 if the flag was set without a number); outputs follow only on
// success.
//
// A curve in `in` is: n p m, then n+1 control points (x y z w), m+1 knots. It is
// allocated on one NL_STACKS with the outputs, so the in-place variants (output curve
// == input curve) can free the input's arrays. The Rust wrapper validates the curve
// (clamped, well-formed) and the parameters so upstream only touches valid memory.
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

// Output curve: the input itself (in place) or a new NULL curve.
NL_CURVE* out_crv(NL_CURVE* cur, double inplace, NL_STACKS* S)
{
    return inplace != 0.0 ? cur : null_crv(S);
}
} // namespace

SC_EXPORT int32_t sc_nlib_crv_tool_p1_eval(int32_t op, const double* in, int64_t n_in, double* out,
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
            case 0: // N_CrvInsertKnot: u r inplace
            {
                NL_CURVE* q = out_crv(cur, prm[2], &S);
                NL_FLAG err = N_CrvInsertKnot(cur, prm[0], (NL_INDEX)prm[1], q, &S, &S);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    dump_crv(o, q);
                break;
            }
            case 1: // N_CrvSplit: u
            {
                NL_CURVE* l = null_crv(&S);
                NL_CURVE* r = null_crv(&S);
                NL_FLAG err = N_CrvSplit(cur, prm[0], l, r, &S);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                {
                    dump_crv(o, l);
                    dump_crv(o, r);
                }
                break;
            }
            case 2: // N_CrvDecomposeBez
            {
                NL_CURVE** segs = NULL;
                NL_INDEX k = -1;
                NL_FLAG err = N_CrvDecomposeBez(cur, &segs, &k, &S);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                {
                    o.push_back((double)(k + 1));
                    for (NL_INDEX i = 0; i <= k; i++)
                        dump_crv(o, segs[i]);
                }
                break;
            }
            case 3: // N_CrvRefine: nx x[0..nx) inplace
            {
                NL_INDEX nx = (NL_INDEX)prm[0];
                NL_REAL* X = N_AllocReal1dArray(nx - 1, &S);
                if (X == NULL)
                {
                    rc = SC_ERR_ARG;
                    break;
                }
                for (NL_INDEX i = 0; i < nx; i++)
                    X[i] = prm[1 + i];
                NL_KNOTVECTOR knx;
                N_KnotVectorFromRealArray(&knx, X, nx - 1);
                NL_CURVE* q = out_crv(cur, prm[1 + nx], &S);
                NL_FLAG err = N_CrvRefine(cur, &knx, q, &S, &S);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    dump_crv(o, q);
                break;
            }
            case 4: // N_CrvExtractCrvSeg: ul ur inplace
            {
                NL_CURVE* q = out_crv(cur, prm[2], &S);
                NL_FLAG err = N_CrvExtractCrvSeg(cur, prm[0], prm[1], q, &S, &S);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    dump_crv(o, q);
                break;
            }
            case 5: // N_CrvRemoveKnot: u nu tol inplace
            {
                NL_CURVE* q = out_crv(cur, prm[3], &S);
                NL_INDEX ru = -1;
                NL_FLAG err = N_CrvRemoveKnot(cur, prm[0], (NL_INDEX)prm[1], prm[2], &ru, q, &S);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                {
                    o.push_back((double)ru);
                    dump_crv(o, q);
                }
                break;
            }
            case 6: // N_CrvRemoveKnotMaxErr: r s
            {
                NL_REAL br = 0.0;
                NL_FLAG err = N_CrvRemoveKnotMaxErr(cur, (NL_INDEX)prm[0], (NL_INDEX)prm[1], &br);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    o.push_back(br);
                break;
            }
            case 7: // N_CrvElevateDegree: t inplace
            {
                NL_CURVE* q = out_crv(cur, prm[1], &S);
                NL_FLAG err = N_CrvElevateDegree(cur, (NL_INDEX)prm[0], q, &S, &S);
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
