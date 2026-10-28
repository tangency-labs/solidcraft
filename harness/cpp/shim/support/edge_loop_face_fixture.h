// SPDX-License-Identifier: Apache-2.0
// A one-face B-rep whose single edge loop is made of explicit curves, each with its own
// tolerance (a real `SmBrepConstructor` face, as `query.loop`'s one-face fixture).
#pragma once
#include "StdAfx.h"
#include <SmBrep.h>
#include <SmBrepData.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmEdge.h>
#include <SmExtent2d.h>
#include <SmFace.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmLoop.h>
#include <SmPlane.h>
#include <SmVector2d.h>
#include <SmVector3d.h>

#include <vector>

namespace sc_topology_fixture
{
// Layout after kind 14:
//   `origin[3] normal[3] u0 v0 u1 v1 face_tol n_edges (curve t0 t1 tol)*n_edges`
// where each curve is
//   `1 ax ay az bx by bz`         SmLine(a, b) over [t0, t1]
//   `3 p1[3] d1[3] p2[3] d2[3]`   SmHermiteCurve(p1, d1, p2, d2, 3) over [t0, t1]
//
// The face is an `SmPlane(origin, normal)` over the given UV domain; its one loop is the
// given edge curves in order, all `SM_OT_SAME` with the loop, and the loop use starts at
// the first edge. Every edge is then given its own tolerance (`SetTolerance`, not only
// if larger, no cascade) and the face its tolerance. `SmHermiteCurve` is a curve kind
// without a NURB: its curve cache measures the distance of its cubic spans, so the tree
// the coincidence checks build depends on the edge's tolerance (a B-spline span is exact
// and skips that test since v1.0.1).
//
// `brep` is replaced by the constructor's B-rep; the caller deletes it, also on failure.
// Returns the first failing status (the caller reports it as the build's).
template <class Reader>
SmStatus edge_loop_face(Reader& rd, const SmContext& ctx, SmBrep*& brep)
{
    const SmPoint3d origin = rd.vec();
    const SmVector3d normal = rd.vec();
    const double u0 = rd.num(), v0 = rd.num(), u1 = rd.num(), v1 = rd.num();
    const double face_tol = rd.num();
    const int64_t n = rd.idx();
    if (n < 1 || n > 64)
        return SM_ERR;

    // curves not yet handed to the constructor; deleted on every failing return
    std::vector<SmCurve*> curves;
    std::vector<double> lo, hi, tols;
    for (int64_t i = 0; i < n; i++)
    {
        const int64_t kind = rd.idx();
        SmCurve* c = nullptr;
        if (kind == 1)
        {
            const SmPoint3d a = rd.vec();
            const SmPoint3d b = rd.vec();
            c = new (ctx) SmLine(a, b, 3, &ctx);
        }
        else if (kind == 3)
        {
            const SmPoint3d p1 = rd.vec();
            const SmVector3d d1 = rd.vec();
            const SmPoint3d p2 = rd.vec();
            const SmVector3d d2 = rd.vec();
            c = new (ctx) SmHermiteCurve(p1, d1, p2, d2, 3);
        }
        if (c == nullptr)
        {
            for (SmCurve* p : curves)
                delete p;
            return SM_ERR;
        }
        curves.push_back(c);
        lo.push_back(rd.num());
        hi.push_back(rd.num());
        tols.push_back(rd.num());
    }

    SmBrepConstructor b;
    brep = b.StartBrep(ctx);
    if (brep == nullptr)
    {
        for (SmCurve* p : curves)
            delete p;
        return SM_ERR;
    }

    SmStatus status = SM_SUCCESS;
    std::vector<SmEdge*> edges((size_t)n, (SmEdge*)NULL);
    do
    {
        SmShell *outer = nullptr, *inner = nullptr;
        if (b.StartShell(outer, inner) != SM_SUCCESS)
        {
            status = SM_ERR;
            break;
        }
        SmFace* face = b.StartFace(SM_OT_SAME);
        if (face == nullptr)
        {
            status = SM_ERR;
            break;
        }
        SmSurface* plane = new (ctx) SmPlane(origin, normal, &ctx);
        if (b.SetFaceSurface(plane, SmExtent2d(SmPoint2d(u0, v0), SmPoint2d(u1, v1))) != SM_SUCCESS)
        {
            status = SM_ERR;
            break;
        }
        if (b.StartLoop(SM_OT_SAME) == nullptr)
        {
            status = SM_ERR;
            break;
        }
        // The loop use starts at the edge use added last and walks the others in the
        // order added: add E[1], .., E[n-1], E[0] to get E[0], .., E[n-1].
        for (int64_t k = 0; k < n && status == SM_SUCCESS; k++)
        {
            const size_t i = (size_t)((k + 1) % n);
            SmEdge* edge = b.StartEdge(SM_OT_SAME, nullptr, nullptr, nullptr);
            if (edge == nullptr)
            {
                status = SM_ERR;
                break;
            }
            SmCurve* c = curves[i];
            curves[i] = nullptr;
            if (b.SetEdgeCurve(c, FALSE) != SM_SUCCESS ||
                b.SetLimits(SmVertexNode(lo[i]), SmVertexNode(hi[i])) != SM_SUCCESS ||
                b.EndEdge() != SM_SUCCESS)
            {
                status = SM_ERR;
                break;
            }
            edges[i] = edge;
        }
        if (status != SM_SUCCESS)
            break;
        if (b.EndLoop() != SM_SUCCESS || b.EndFace() != SM_SUCCESS || b.EndShell(FALSE) != SM_SUCCESS ||
            b.EndBrep() != SM_SUCCESS)
        {
            status = SM_ERR;
            break;
        }
        // tolerances (SM_USE_OLDTOL: stored on the face and the edges)
        face->SetTolerance(SmZoneTol3d(face_tol), FALSE, FALSE);
        for (size_t i = 0; i < (size_t)n && status == SM_SUCCESS; i++)
        {
            if (edges[i] == nullptr)
                status = SM_ERR;
            else
                edges[i]->SetTolerance(SmZoneTol3d(tols[i]), FALSE, FALSE);
        }
    } while (false);

    // curves the constructor refused (only reachable on a failing return)
    for (SmCurve* p : curves)
        if (p != nullptr)
            delete p;
    return status;
}
} // namespace sc_topology_fixture
