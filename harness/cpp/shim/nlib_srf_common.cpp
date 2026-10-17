// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.srf_common`.
// Wraps the common surface constructions of upstream source/SMLib/src/NL_SrfCommon.cpp
// owned by the unit: N_CreateSrfCornerPts, N_CreateSrfExtrudeCrv, N_CreateCylCone,
// N_CreateRuledSrf, N_CreateRevolvedSrf.
//
// One entry point, sc_nlib_srf_common_eval(op, in, n_in, out, cap, out_len): `op`
// selects the routine, `in` holds its curve(s) and parameters (layouts in
// crates/solidcore-ffi/src/units/nlib_srf_common.rs), `out` receives a flat record of
// doubles: the routine's status (0 = ok, else NL_ERROR.eno, cleared before the call;
// 1000 if the flag was set without a number), then every input curve as it is after
// the call (the routines change them in place), then on success the surface:
// n m p q r s, (n+1)*(m+1) control points x y z w (row i = u index), r+1 u knots,
// s+1 v knots. A curve is n p m, n+1 control points x y z w, m+1 knots. The output
// surface is a new NULL surface, as most upstream callers pass.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

#include <vector>

#include "sc_nlib.h"

using sc::dump_crv;
using sc::dump_srf;
using sc::nl_status;
using sc::read_crv;

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

SC_EXPORT int32_t sc_nlib_srf_common_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                          int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int32_t rc = SC_OK;
    NL_SURFACE* sur = N_AllocSrf(&S);
    if (sur == NULL)
    {
        N_EndNurbs(&S);
        return SC_ERR_ARG;
    }
    N_SrfInitArrays(sur);
    N_ErrClear();
    NL_FLAG err = NL_NO;
    NL_CURVE* crv[2] = {NULL, NULL};
    int ncrv = 0;
    int64_t at = 0;
    switch (op)
    {
        case 0: // N_CreateSrfCornerPts: P00 P10 P01 P11
            err = N_CreateSrfCornerPts(pt(in), pt(in + 3), pt(in + 6), pt(in + 9), sur, &S);
            break;
        case 1: // N_CreateSrfExtrudeCrv: curve, W d dir
        {
            crv[0] = read_crv(in, &at, &S);
            ncrv = 1;
            if (crv[0] == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            const double* d = in + at;
            err = N_CreateSrfExtrudeCrv(crv[0], pt(d), d[3], (NL_FLAG)d[4], sur, &S);
            break;
        }
        case 2: // N_CreateCylCone: C X Y rb rt as ae h ctp dir
            err = N_CreateCylCone(pt(in), pt(in + 3), pt(in + 6), in[9], in[10], in[11], in[12],
                                  in[13], (NL_FLAG)in[14], (NL_FLAG)in[15], sur, &S);
            break;
        case 3: // N_CreateRuledSrf: curveA, curveB, dir
        {
            crv[0] = read_crv(in, &at, &S);
            crv[1] = crv[0] == NULL ? NULL : read_crv(in, &at, &S);
            ncrv = 2;
            if (crv[0] == NULL || crv[1] == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            err = N_CreateRuledSrf(crv[0], crv[1], (NL_FLAG)in[at], sur, &S, &S);
            break;
        }
        case 4: // N_CreateRevolvedSrf: curve, S T al ctp
        {
            crv[0] = read_crv(in, &at, &S);
            ncrv = 1;
            if (crv[0] == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            const double* d = in + at;
            err = N_CreateRevolvedSrf(crv[0], pt(d), pt(d + 3), d[6], (NL_FLAG)d[7], sur, &S);
            break;
        }
        default:
            rc = SC_ERR_ARG;
            break;
    }
    if (rc == SC_OK)
    {
        o.push_back(nl_status(err));
        for (int k = 0; k < ncrv; k++)
            dump_crv(o, crv[k]);
        if (err == NL_NO)
            dump_srf(o, sur);
    }
    N_EndNurbs(&S);
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
