// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.crv_common`.
// Wraps the conic constructions of upstream source/SMLib/src/NL_CrvCommon.cpp owned by
// the unit: N_CreateQuadraticArcTol, N_CreateQuarticArcTol, N_CreateQuinticArcTol,
// N_CreateCircArc, N_CreateCircArcTol, N_CalcCircWeights, N_CreateEllipticalArcTol,
// N_CreateEllipticalArcAngTypeTol.
//
// One entry point, sc_nlib_crv_common_eval(op, in, n_in, out, cap, out_len): `op`
// selects the routine, `in` holds its parameters (layouts in
// crates/solidcore-ffi/src/units/nlib_crv_common.rs), `out` receives a flat record of
// doubles: the routine's status (0 = ok, else NL_ERROR.eno, cleared before the call;
// 1000 if the flag was set without a number), then on success either a curve
// (n p m, n+1 control points x y z w, m+1 knots) or, for N_CalcCircWeights, k+1 and
// the weights w[0..k]. The output curve is a new NULL curve, as upstream callers do.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

#include <vector>

#include "sc_nlib.h"

using sc::dump_crv;
using sc::nl_status;

namespace
{
NL_POINT pt(const double* d)
{
    NL_POINT p;
    p.x = d[0];
    p.y = d[1];
    p.z = d[2];
    return p;
}

} // namespace

SC_EXPORT int32_t sc_nlib_crv_common_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                          int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int32_t rc = SC_OK;
    NL_CURVE* cur = N_AllocCrv(&S);
    if (cur == NULL)
    {
        N_EndNurbs(&S);
        return SC_ERR_ARG;
    }
    N_CrvInitArrays(cur);
    N_ErrClear();
    NL_FLAG err = NL_NO;
    bool is_curve = true;
    switch (op)
    {
        // 0..4: C X Y r as ae [at] (ctp)
        case 0: // N_CreateQuadraticArcTol: C X Y r as ae at
            err = N_CreateQuadraticArcTol(pt(in), pt(in + 3), pt(in + 6), in[9], in[10], in[11],
                                          in[12], cur, &S);
            break;
        case 1: // N_CreateQuarticArcTol: C X Y r as ae at
            err = N_CreateQuarticArcTol(pt(in), pt(in + 3), pt(in + 6), in[9], in[10], in[11],
                                        in[12], cur, &S);
            break;
        case 2: // N_CreateQuinticArcTol: C X Y r as ae at
            err = N_CreateQuinticArcTol(pt(in), pt(in + 3), pt(in + 6), in[9], in[10], in[11],
                                        in[12], cur, &S);
            break;
        case 3: // N_CreateCircArc: C X Y r as ae ctp
            err = N_CreateCircArc(pt(in), pt(in + 3), pt(in + 6), in[9], in[10], in[11],
                                  (NL_FLAG)in[12], cur, &S);
            break;
        case 4: // N_CreateCircArcTol: C X Y r as ae at ctp
            err = N_CreateCircArcTol(pt(in), pt(in + 3), pt(in + 6), in[9], in[10], in[11], in[12],
                                     (NL_FLAG)in[13], cur, &S);
            break;
        case 5: // N_CalcCircWeights: as ae ctp
        {
            is_curve = false;
            NL_REAL w[32];
            NL_INDEX k = -1;
            err = N_CalcCircWeights(in[0], in[1], (NL_FLAG)in[2], w, &k);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                o.push_back((double)(k + 1));
                for (NL_INDEX i = 0; i <= k; i++)
                    o.push_back(w[i]);
            }
            break;
        }
        case 6: // N_CreateEllipticalArcTol: C X Y r1 r2 as ae at ctp
            err = N_CreateEllipticalArcTol(pt(in), pt(in + 3), pt(in + 6), in[9], in[10], in[11],
                                           in[12], in[13], (NL_FLAG)in[14], cur, &S);
            break;
        case 7: // N_CreateEllipticalArcAngTypeTol: C X Y r1 r2 as ae at atp ctp
            err = N_CreateEllipticalArcAngTypeTol(pt(in), pt(in + 3), pt(in + 6), in[9], in[10],
                                                  in[11], in[12], in[13], (NL_FLAG)in[14],
                                                  (NL_FLAG)in[15], cur, &S);
            break;
        default:
            rc = SC_ERR_ARG;
            is_curve = false;
            break;
    }
    if (rc == SC_OK && is_curve)
    {
        o.push_back(nl_status(err));
        if (err == NL_NO)
            dump_crv(o, cur);
    }
    N_EndNurbs(&S);
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
