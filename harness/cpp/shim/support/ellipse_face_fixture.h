// SPDX-License-Identifier: Apache-2.0
// Native planar faces with ellipse/circle edges and exact rational UV trims.
#pragma once
#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBrep.h>
#include <SmCircle.h>
#include <SmEllipse.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmFace.h>
#include <SmPlane.h>
#include <SmTArray.h>
#include <memory>

namespace sc_topology_fixture {
// Layout after kind 9: circle origin[3] axis rx ry angles[2] natural[2] inside_out oob.
// The face builder consumes the curves. The UV NURB is the native local ellipse's
// stored form, so both it and the 3D edge use the caller's NURB parameter interval.
template <class Reader>
SmStatus ellipse_face(SmBrep* brep, Reader& rd, const SmContext& ctx)
{
    const bool circle = rd.flag();
    const SmPoint3d origin = rd.vec();
    const int axis = (int)rd.idx();
    const SmVector3d x = axis == 0 ? SmVector3d(0, 1, 0) : SmVector3d(1, 0, 0);
    const SmVector3d y = axis == 2 ? SmVector3d(0, 1, 0) : SmVector3d(0, 0, 1);
    const double rx = rd.num(), ry = rd.num();
    const double a = rd.num(), b = rd.num(), n0 = rd.num(), n1 = rd.num();
    const SmBoolean inside_out = rd.bflag(), oob = rd.bflag();
    std::unique_ptr<SmEllipse> edge(circle
        ? (SmEllipse*)new (ctx) SmCircle(origin, x, y, SmExtent1d(a, b), rx, 3, &ctx, NULL, inside_out)
        : new (ctx) SmEllipse(origin, x, y, SmExtent1d(a, b), rx, ry, 3, &ctx, NULL, inside_out));
    SmEllipse local(SmPoint3d(0, 0, 0), SmVector3d(1, 0, 0), SmVector3d(0, 1, 0),
                    SmExtent1d(a, b), rx, ry, 2, &ctx, NULL, inside_out);
    SER(edge->EditParameterization(SmExtent1d(n0, n1)));
    SER(local.EditParameterization(SmExtent1d(n0, n1)));
    edge->SetOutOfBoundsEnabled(oob);
    std::unique_ptr<SmBSplineCurve> uv(new (ctx) SmBSplineCurve(2, local.GetGwNurbPointer()));
    SmTArray<SmCurve*> curves;
    SmTArray<SmOrientType> orientations;
    curves.Add(edge.get());
    orientations.Add(SM_OT_SAME);
    std::unique_ptr<SmBSplineCurve> chord, uv_chord;
    if (b - a < 360.0)
    {
        SmPoint3d start, end, uv_start, uv_end;
        SER(edge->EvaluatePoint(n0, start));
        SER(edge->EvaluatePoint(n1, end));
        SER(uv->EvaluatePoint(n0, uv_start));
        SER(uv->EvaluatePoint(n1, uv_end));
        SmBSplineCurve* c = nullptr;
        SER(SmBSplineCurve::CreateLineSegment(ctx, 3, end, start, c));
        chord.reset(c);
        c = nullptr;
        SER(SmBSplineCurve::CreateLineSegment(ctx, 2, uv_end, uv_start, c));
        uv_chord.reset(c);
        curves.Add(chord.get());
        orientations.Add(SM_OT_SAME);
    }
    SmTArray<ULONG> loops;
    loops.Add(curves.GetSize());
    SmTArray<SmPoint3d> points;
    const SmExtent2d domain(SmPoint2d(-2 * rx, -2 * ry), SmPoint2d(2 * rx, 2 * ry));
    std::unique_ptr<SmPlane> plane(new (ctx) SmPlane(origin, x, y, SmVector2d(1, 1), domain, &ctx));
    SmRegion* region = nullptr;
    SmShell* shell = nullptr;
    SmFace* face = nullptr;
    // MakeFaceWithCurves consumes (and discards) supplied UV curves in the
    // pinned reference. Attach our exact trims through the native setter after
    // it has built the topology, preserving the fixture's exact parameterized trims.
    SmCurve* analytic = edge.release();
    SmCurve* closing = chord.release();
    SmStatus status = brep->MakeFaceWithCurves(brep->GetInfiniteRegion(), loops, &curves, nullptr,
        orientations, points, plane.get(), domain, SM_OT_SAME, region, shell, face);
    if (plane->GetOwner() != nullptr) plane.release();
    if (status != SM_SUCCESS) return status;
    if (face == nullptr) return SM_ERR;
    SmTArray<SmEdge*> edges;
    face->GetEdges(edges);
    for (ULONG i = 0; i < edges.GetSize(); ++i)
    {
        SmEdge* e = edges[i];
        SmEdgeuse* eu = e->GetPrimaryEdgeuse();
        if (eu == nullptr) return SM_ERR;
        if (e->GetCurve() == analytic)
            eu->SetUVTrimCurve(uv.release(), 0.0, TRUE, nullptr);
        else if (e->GetCurve() == closing && closing != nullptr)
            eu->SetUVTrimCurve(uv_chord.release(), 0.0, TRUE, nullptr);
        else
            return SM_ERR;
    }
    return !uv && !uv_chord ? SM_SUCCESS : SM_ERR;
}
} // namespace sc_topology_fixture
