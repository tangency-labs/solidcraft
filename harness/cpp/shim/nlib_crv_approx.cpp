// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.crv_approx`.
// Wraps the routines of upstream source/SMLib/src/NL_CrvApprox.cpp owned by the unit:
// N_CalcNumCPtsToApproxArc, N_ApproxCircArcWithCrvData and N_ApproxNurbsWithNonRatCrv.
//
// One entry point, sc_nlib_crv_approx_eval(op, in, n_in, out, cap, out_len): `op` selects
// the routine, `in` holds its parameters (layouts in
// crates/solidcore-ffi/src/units/nlib_crv_approx.rs), `out` receives a flat record of
// doubles starting with the routine's status: 0 = ok, else NL_ERROR.eno (cleared before
// the call; 1000 if the flag was set without a number). Outputs follow on success.
//
// A curve is: n p m, then n+1 control points (x y z w), m+1 knots. The Rust wrapper
// validates every input so upstream only touches valid memory.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>
#include <NL_CrvApprox.h>

#include <vector>

#include "sc_nlib.h"

using sc::dump_crv;
using sc::nl_status;
using sc::null_crv;
using sc::read_crv;

namespace
{

NL_SFUN* null_sfn(NL_STACKS* S)
{
    NL_SFUN* sfn = N_AllocSrfFuncData(S);
    if (sfn != NULL)
        N_SFuncInitArrays(sfn);
    return sfn;
}

void read_pt(const double* d, NL_POINT* P)
{
    P->x = d[0];
    P->y = d[1];
    P->z = d[2];
}
} // namespace

SC_EXPORT int32_t sc_nlib_crv_approx_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                          int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int32_t rc = SC_OK;
    switch (op)
    {
        case 0: // N_CalcNumCPtsToApproxArc: tol ang deg mode
                // mode 0: sfn NULL, flg NL_NO; 1: sfn NULL, flg NL_YES;
                // 2: sfn built by a first call (flg NL_YES, same deg) and passed in.
        {
            NL_SFUN* sfn = null_sfn(&S);
            if (sfn == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_INDEX nc = -1;
            int mode = (int)in[3];
            NL_DEGREE deg = (NL_DEGREE)in[2];
            if (mode == 2)
            {
                N_ErrClear();
                NL_INDEX nc0 = -1;
                NL_FLAG e0 = N_CalcNumCPtsToApproxArc(1.0, 360.0, deg, NL_YES, &nc0, sfn, &S);
                if (e0 != NL_NO)
                {
                    o.push_back(nl_status(e0));
                    break;
                }
            }
            N_ErrClear();
            NL_FLAG err = N_CalcNumCPtsToApproxArc(in[0], in[1], deg, mode == 0 ? NL_NO : NL_YES,
                                                   &nc, sfn, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                o.push_back((double)nc);
            break;
        }
        case 1: // N_ApproxCircArcWithCrvData: cen X Y rad as ae Ps Pe deg nc
        {
            NL_POINT cen, Ps, Pe;
            NL_VECTOR X, Y;
            read_pt(in, &cen);
            read_pt(in + 3, &X);
            read_pt(in + 6, &Y);
            NL_REAL rad = in[9], as = in[10], ae = in[11];
            read_pt(in + 12, &Ps);
            read_pt(in + 15, &Pe);
            NL_DEGREE deg = (NL_DEGREE)in[18];
            NL_INDEX nc = (NL_INDEX)in[19];
            NL_CURVE* q = null_crv(&S);
            if (q == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            N_ErrClear();
            NL_FLAG err = N_ApproxCircArcWithCrvData(cen, X, Y, rad, as, ae, Ps, Pe, deg, nc, q, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_crv(o, q);
            break;
        }
        case 2: // N_ApproxNurbsWithNonRatCrv: curve, tol deg par tan
        {
            int64_t at = 0;
            NL_CURVE* cur = read_crv(in, &at, &S);
            NL_CURVE* q = null_crv(&S);
            if (cur == NULL || q == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            const double* prm = in + at;
            N_ErrClear();
            NL_FLAG err = N_ApproxNurbsWithNonRatCrv(cur, prm[0], (NL_DEGREE)prm[1], (NL_FLAG)prm[2],
                                                     (NL_FLAG)prm[3], q, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_crv(o, q);
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
