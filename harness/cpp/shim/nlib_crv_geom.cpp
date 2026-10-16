// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.crv_geom`.
// Wraps the NL_CURVE routines of upstream source/SMLib/src/NL_CrvGeom.cpp owned by the unit:
// N_CrvClosestPt, N_CrvArcLength, N_ApproxCrvWithPolyline.
//
// One entry point, sc_nlib_crv_geom_eval(op, in, n_in, out, cap, out_len): `op` selects
// the routine, `in` holds a curve then the routine's parameters, `out` receives a flat
// record of doubles (layouts in crates/difftest/src/units/nlib_crv_geom.rs). The record
// starts with the routine's status: 0 = ok, else NL_ERROR.eno (cleared before the call;
// 1000 if the flag was set without a number).
//
// A curve in `in` is: n p m, then n+1 control points (x y z w), m+1 knots. The Rust
// wrapper validates the curve (clamped, well-formed) and the parameters so upstream
// only touches valid memory and terminates.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

#include <cmath>
#include <vector>

#include "sc_nlib.h"

using sc::nl_status;
using sc::read_crv;

namespace
{
void push_pt(std::vector<double>& o, NL_POINT P)
{
    o.push_back(P.x);
    o.push_back(P.y);
    o.push_back(P.z);
}
} // namespace

SC_EXPORT int32_t sc_nlib_crv_geom_eval(int32_t op, const double* in, int64_t n_in, double* out,
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
            case 0: // N_CrvClosestPt: P[3] u0 top toc
            {
                NL_POINT P;
                P.x = prm[0];
                P.y = prm[1];
                P.z = prm[2];
                // Outputs upstream may leave unwritten read back as NaN.
                NL_PARAMETER u = NAN;
                NL_POINT Q;
                Q.x = NAN;
                Q.y = NAN;
                Q.z = NAN;
                NL_FLAG err = N_CrvClosestPt(cur, P, prm[3], prm[4], prm[5], &u, &Q);
                o.push_back(nl_status(err));
                o.push_back(u);
                push_pt(o, Q);
                break;
            }
            case 1: // N_CrvArcLength: ul ur tol flg
            {
                NL_REAL len = 0.0;
                NL_FLAG err = N_CrvArcLength(cur, prm[0], prm[1], prm[2], (NL_FLAG)prm[3], &len);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                    o.push_back(len);
                break;
            }
            case 2: // N_ApproxCrvWithPolyline: tol tflg rflg
            {
                NL_POINT* pts = NULL;
                NL_REAL* pars = NULL;
                NL_INDEX n = -1;
                NL_FLAG rflg = (NL_FLAG)prm[2];
                NL_FLAG err =
                    N_ApproxCrvWithPolyline(cur, prm[0], (NL_FLAG)prm[1], rflg, &pts, &pars, &n, &S);
                o.push_back(nl_status(err));
                if (err == NL_NO)
                {
                    o.push_back((double)(n + 1));
                    if (rflg != NL_PARAMETERS)
                        for (NL_INDEX i = 0; i <= n; i++)
                            push_pt(o, pts[i]);
                    if (rflg != NL_POINTS)
                        for (NL_INDEX i = 0; i <= n; i++)
                            o.push_back(pars[i]);
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
