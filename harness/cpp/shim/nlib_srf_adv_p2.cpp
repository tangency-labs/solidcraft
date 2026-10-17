// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nlib.srf_adv_p2`.
// Wraps ST_surxsdr (the recursive core of N_SrfExtendByDist) and
// N_SrfEvalPtNormalDeriv (upstream source/SMLib/src/NL_SrfAdv.cpp).
//
// One entry point, sc_nlib_srf_adv_p2_eval(op, in, n_in, out, cap, out_len): `op`
// selects the routine, `in` holds its inputs, `out` receives a flat record of
// doubles: the status (0 = ok, else NL_ERROR.eno, cleared before the call; 1000 if
// the flag was set without a number), then, on success only, the results.
//
// A surface in `in` is: n m p q, then (n+1)*(m+1) control points (x y z w,
// row-major), n+p+2 u knots, m+q+2 v knots (storage = indices in use). Everything is
// allocated on one NL_STACKS. The Rust wrapper validates the surface (valid clamped
// knot vectors, degrees 1..=NL_MAXDEG) and the parameters.
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

// File-local in spirit, but defined with external linkage in NL_SrfAdv.cpp.
NL_FLAG ST_surxsdr(NL_SURFACE* surP, NL_REAL dist, NL_FLAG dflg, NL_FLAG end, NL_FLAG cont, NL_SURFACE* surQ,
                   NL_STACKS* SP, NL_STACKS* SQ, int depth);

namespace
{

void push_pt(std::vector<double>& o, const NL_POINT& p)
{
    o.push_back(p.x);
    o.push_back(p.y);
    o.push_back(p.z);
}

} // namespace

// op 0 extension: surface, dist dflg end cont depth inplace -> status, surface.
// op 1 normal derivatives: surface, u v ufl vfl -> status, Nu Nv Nuu Nuv Nvv (xyz each).
SC_EXPORT int32_t sc_nlib_srf_adv_p2_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
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
            bool inplace = prm[5] != 0.0;
            NL_SURFACE* res = inplace ? sur : null_srf(&S);
            if (res == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            err = ST_surxsdr(sur, prm[0], (NL_FLAG)prm[1], (NL_FLAG)prm[2], (NL_FLAG)prm[3], res, &S, &S,
                             (int)prm[4]);
            o.push_back(nl_status(err));
            if (err == NL_NO)
                dump_srf(o, res);
            break;
        }
        case 1:
        {
            NL_POINT** ND = N_AllocPt2dArray(2, 2, &S);
            if (ND == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
            err = N_SrfEvalPtNormalDeriv(sur, prm[0], prm[1], (NL_FLAG)prm[2], (NL_FLAG)prm[3], ND);
            o.push_back(nl_status(err));
            if (err == NL_NO)
            {
                push_pt(o, ND[1][0]);
                push_pt(o, ND[0][1]);
                push_pt(o, ND[2][0]);
                push_pt(o, ND[1][1]);
                push_pt(o, ND[0][2]);
            }
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
