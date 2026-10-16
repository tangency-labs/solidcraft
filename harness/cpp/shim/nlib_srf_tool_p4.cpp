// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.srf_tool_p4`.
// Wraps N_SrfRemoveKnotsConstraints and N_MakeSrfsCompatibleUV
// (upstream source/SMLib/src/NL_SrfTool.cpp).
//
// One entry point, sc_nlib_srf_tool_p4_eval(op, in, n_in, out, cap, out_len): `op`
// selects the routine, `in` holds its inputs, `out` receives a flat record of
// doubles: the status (0 = ok, else NL_ERROR.eno, cleared before the call; 1000 if
// the flag was set without a number), then, on success only, the results.
//
// A surface in `in` is: n m p q, then (n+1)*(m+1) control points (x y z w,
// row-major), n+p+2 u knots, m+q+2 v knots (storage = indices in use). Everything is
// allocated on one NL_STACKS. The Rust wrapper validates every surface (valid clamped
// knot vectors, degrees 1..=NL_MAXDEG) and the parameters, so upstream only touches
// the storage it allocates.
//
// A surface is dumped as n m p q r s, the control points, the u and v knots.
#include "sc_shim.h"

#include <nurbs.h>
#include <NL_Globals.h>

#include <vector>

#include "sc_nlib.h"

using sc::dump_srf;
using sc::nl_status;
using sc::null_srf;
using sc::read_srf;

namespace
{
} // namespace

// op 0 remove knots with constraints: surface, cu, cu u knots, cv, cv v knots,
//      tol dir inplace -> status, surface.
// op 1 make compatible in u and v: count, count surfaces -> status, count surfaces.
SC_EXPORT int32_t sc_nlib_srf_tool_p4_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                           int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int64_t at = 0;
    int32_t rc = SC_OK;
    NL_FLAG err = NL_NO;
    switch (op)
    {
        case 0:
        {
            NL_SURFACE* sur = read_srf(in, &at, &S);
            if (sur == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_INDEX cu = (NL_INDEX)in[at++];
            std::vector<NL_PARAMETER> uk(in + at, in + at + cu);
            at += cu;
            NL_INDEX cv = (NL_INDEX)in[at++];
            std::vector<NL_PARAMETER> vk(in + at, in + at + cv);
            at += cv;
            const double* prm = in + at;
            bool inplace = prm[2] != 0.0;
            NL_SURFACE* res = inplace ? sur : null_srf(&S);
            if (res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            N_ErrClear();
            err = N_SrfRemoveKnotsConstraints(sur, cu > 0 ? uk.data() : NULL, cu - 1, cv > 0 ? vk.data() : NULL,
                                              cv - 1, prm[0], (NL_FLAG)prm[1], res, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_srf(o, res);
            break;
        }
        case 1:
        {
            NL_INDEX cnt = (NL_INDEX)in[0];
            at = 1;
            std::vector<NL_SURFACE*> surs;
            for (NL_INDEX i = 0; i < cnt && rc == SC_OK; i++)
            {
                NL_SURFACE* s = read_srf(in, &at, &S);
                if (s == NULL)
                    rc = SC_ERR_ARG;
                surs.push_back(s);
            }
            if (rc != SC_OK || cnt < 1)
            {
                rc = SC_ERR_ARG;
                break;
            }
            N_ErrClear();
            err = N_MakeSrfsCompatibleUV(surs.data(), cnt - 1, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                for (NL_SURFACE* s : surs)
                    dump_srf(o, s);
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
