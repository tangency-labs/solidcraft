// SPDX-License-Identifier: Apache-2.0
// The full record of a UV trim curve (an SmBSplineCurve or one of its subclasses),
// shared by the difftest shims of units `topology.edgeuse_p3` and `topology.face_p3`.
// Rust side: crates/difftest/src/util/trim_curve.rs. Layout:
//   -1                                  no curve
//   1 type n p m Pw[0..=n](x y z w) U[0..=m] t0 t1 sample*3
// with `n = m = -1` and nothing between them and `t0` when the curve has no NURB, and
// each sample (at 0, 1/2 and 1 of the natural interval) `0 x y z` or `status` when the
// evaluation fails.
#ifndef SC_TRIM_CURVE_RECORD_H
#define SC_TRIM_CURVE_RECORD_H

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmExtent1d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

namespace sc_trim_curve
{
inline void put_trim_curve(std::vector<double>& o, const SmBSplineCurve* c)
{
    if (c == nullptr)
    {
        o.push_back(-1.0);
        return;
    }
    o.push_back(1.0);
    o.push_back((double)c->GetType());
    const gw_CURVE* g = c->GetGwNurbPointer();
    if (g == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(-1.0);
        o.push_back(-1.0);
    }
    else
    {
        o.push_back((double)g->pol->n);
        o.push_back((double)g->p);
        o.push_back((double)g->knt->m);
        for (gw_INDEX i = 0; i <= g->pol->n; i++)
        {
            o.push_back(g->pol->Pw[i].x);
            o.push_back(g->pol->Pw[i].y);
            o.push_back(g->pol->Pw[i].z);
            o.push_back(g->pol->Pw[i].w);
        }
        for (gw_INDEX i = 0; i <= g->knt->m; i++)
            o.push_back(g->knt->U[i]);
    }
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    for (int k = 0; k <= 2; k++)
    {
        SmPoint3d p(0.0, 0.0, 0.0);
        SmStatus s = c->EvaluatePoint(ivl.Evaluate(k / 2.0), p);
        if (s != SM_SUCCESS)
        {
            o.push_back((double)s);
            continue;
        }
        o.push_back(0.0);
        o.push_back(p.x);
        o.push_back(p.y);
        o.push_back(p.z);
    }
}
} // namespace sc_trim_curve

#endif
