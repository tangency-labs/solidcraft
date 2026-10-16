// SPDX-License-Identifier: Apache-2.0
// Helpers over gw NURBS curves (gw_CURVE); reads through sc::Rd.
//
// Copied verbatim from the per-shim copies they replace (only `inline` and the
// `sc` namespace were added), so behaviour is unchanged. A shim that needs a
// different variant keeps its own copy and does not include this header.
// Shims pull a helper in with `using sc::<name>;` after their includes.
#ifndef SC_GW_H
#define SC_GW_H

#include "sc_sm.h"

#include <SmNurbsCrv.h>
#include <nurbs.h>

namespace sc
{

// Reads `n p m cpts knots` into a curve from sm_AllocateNurbCurve.
inline gw_CURVE* read_nurb(Rd& rd)
{
    gw_INDEX n = (gw_INDEX)rd.idx();
    gw_DEGREE p = (gw_DEGREE)rd.idx();
    gw_INDEX m = (gw_INDEX)rd.idx();
    gw_CURVE* cur = sm_AllocateNurbCurve(n, p, m);
    for (gw_INDEX i = 0; i <= n; i++)
    {
        cur->pol->Pw[i].x = rd.num();
        cur->pol->Pw[i].y = rd.num();
        cur->pol->Pw[i].z = rd.num();
        cur->pol->Pw[i].w = rd.num();
    }
    for (gw_INDEX i = 0; i <= m; i++)
        cur->knt->U[i] = rd.num();
    return cur;
}

} // namespace sc

#endif // SC_GW_H
