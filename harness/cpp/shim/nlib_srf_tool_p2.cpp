// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.srf_tool_p2`.
// Wraps N_SrfRemoveKnotConditional, N_SrfRemoveOneKnot and N_SrfElevateDegree
// (upstream source/SMLib/src/NL_SrfTool.cpp).
//
// One entry point, sc_nlib_srf_tool_p2_eval(op, in, n_in, out, cap, out_len): `op`
// selects the routine, `in` holds its inputs, `out` receives a flat record of
// doubles: the status (0 = ok, else NL_ERROR.eno, cleared before the call; 1000 if
// the flag was set without a number), then, on success only, the results.
//
// A surface in `in` is: n m p q, then (n+1)*(m+1) control points (x y z w,
// row-major), n+p+2 u knots, m+q+2 v knots (storage = indices in use). Everything is
// allocated on one NL_STACKS. The Rust wrapper validates the surface (valid clamped
// knot vectors, degrees 1..=NL_MAXDEG) and the parameters (finite; knot index,
// multiplicity and row range of N_SrfRemoveOneKnot inside the arrays) so upstream
// only touches the storage it allocates.
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

// op 0 conditional removal: surface, t nt tol dir inplace -> status, rt, surface.
// op 1 one-knot error bound: surface, r s f l dir mr -> status, mr.
// op 2 degree elevation: surface, t dir inplace -> status, surface.
SC_EXPORT int32_t sc_nlib_srf_tool_p2_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                           int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    NL_STACKS S;
    N_InitNurbs(&S);
    std::vector<double> o;
    int64_t at = 0;
    int32_t rc = SC_OK;
    NL_SURFACE* sur = read_srf(in, &at, &S);
    const double* prm = in + at;
    if (sur == NULL)
        rc = SC_ERR_ARG;
    NL_FLAG err = NL_NO;
    N_ErrClear();
    switch (rc == SC_OK ? op : -1)
    {
        case 0:
        {
            bool inplace = prm[4] != 0.0;
            NL_SURFACE* res = inplace ? sur : null_srf(&S);
            if (res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            NL_INDEX rt = -1;
            err = N_SrfRemoveKnotConditional(sur, prm[0], (NL_INDEX)prm[1], prm[2], (NL_FLAG)prm[3], &rt, res, &S);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                o.push_back((double)rt);
                dump_srf(o, res);
            }
            break;
        }
        case 1:
        {
            NL_REAL mr = prm[5];
            err = N_SrfRemoveOneKnot(sur, (NL_INDEX)prm[0], (NL_INDEX)prm[1], (NL_INDEX)prm[2], (NL_INDEX)prm[3],
                                     (NL_FLAG)prm[4], &mr);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                o.push_back(mr);
            break;
        }
        case 2:
        {
            bool inplace = prm[2] != 0.0;
            NL_SURFACE* res = inplace ? sur : null_srf(&S);
            if (res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            err = N_SrfElevateDegree(sur, (NL_INDEX)prm[0], (NL_FLAG)prm[1], res, &S, &S);
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
