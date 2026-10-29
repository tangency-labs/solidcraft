// SPDX-License-Identifier: Apache-2.0
// Native sheet face whose four edges are real SmHermiteCurve segments of an
// exact cubic B-spline sheet's affine UV loop (no UV trims).
#pragma once
#include "StdAfx.h"
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmCurve.h>
#include <SmEdge.h>
#include <SmFace.h>
#include <SmHermiteCurve.h>
#include <SmNurbsSrf.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <memory>
#include <nurbs.h>

namespace sc_topology_fixture
{
namespace hermite_sheet_detail
{
// The fixed sheet of the fixture, from the scalar scale alone (never a C++ export):
// S(u, v) = C(u) + v * extrusion, C the cubic Hermite of (P0, D0, P1, D1).
struct Shape
{
    SmVector3d p0, p1, d0, d1, extrusion;
    explicit Shape(double s)
        : p0(0, 0, 0), p1(s, .2 * s, .1 * s), d0(.6 * s, 1.2 * s, .7 * s),
          d1(1.2 * s, -.8 * s, -.4 * s), extrusion(0, 0, s)
    {
    }
    SmVector3d at(double u) const
    {
        const double u2 = u * u, u3 = u2 * u;
        return p0 * (2 * u3 - 3 * u2 + 1) + d0 * (u3 - 2 * u2 + u) + p1 * (-2 * u3 + 3 * u2) +
               d1 * (u3 - u2);
    }
    SmVector3d tangent(double u) const
    {
        const double u2 = u * u;
        return p0 * (6 * u2 - 6 * u) + d0 * (3 * u2 - 4 * u + 1) + p1 * (-6 * u2 + 6 * u) +
               d1 * (3 * u2 - 2 * u);
    }
};
} // namespace hermite_sheet_detail

// Layout after kind 15: scale edge_tol. Both are finite; the Rust guard rejects a
// non-positive scale and a negative edge tolerance (zero is allowed, the native
// exact setter takes it).
//
// The surface is an exact degree 3x1 clamped SmBSplineSurface (all weights 1)
// with S(u, v) = C(u) + v * extrusion over the unit domains. The four UV corners
// [.2,.2], [.8,.7], [.8,.9], [.2,.4] form a counter-clockwise loop; each edge is a
// real SmHermiteCurve over that affine UV segment, with endpoints S(ua, va) and
// S(ub, vb) and derivatives C'(ua)*(ub-ua) + extrusion*(vb-va) (likewise at ub).
// SmBrep::MakeFaceWithCurves builds the face from the four SAME-oriented curves,
// one loop, no UV curves and no loop points (the ellipse-face convention); the
// fixture then gives the face tolerance 1e-7 and every edge exactly edge_tol. The
// edges own their curves and the face owns the surface.
template <class Reader>
SmStatus hermite_sheet_face(SmBrep* brep, Reader& rd, const SmContext& ctx)
{
    using hermite_sheet_detail::Shape;
    const double scale = rd.num();
    const double edge_tol = rd.num();
    const Shape sh(scale);

    const SmVector3d cp[4] = {sh.p0, sh.p0 + sh.d0 / 3.0, sh.p1 - sh.d1 / 3.0, sh.p1};
    gw_SURFACE* raw = sm_AllocateNurbSurface(3, 1, 3, 1, 7, 3);
    for (int i = 0; i < 4; ++i)
        for (int j = 0; j < 2; ++j)
        {
            const SmVector3d p = cp[i] + sh.extrusion * (double)j;
            raw->net->Pw[i][j].x = p.x;
            raw->net->Pw[i][j].y = p.y;
            raw->net->Pw[i][j].z = p.z;
            raw->net->Pw[i][j].w = 1.0;
        }
    for (int i = 0; i < 8; ++i)
        raw->knu->U[i] = i < 4 ? 0.0 : 1.0;
    for (int i = 0; i < 4; ++i)
        raw->knv->U[i] = i < 2 ? 0.0 : 1.0;
    std::unique_ptr<SmBSplineSurface> surface(new (ctx) SmBSplineSurface(raw, FALSE, NULL));

    static const double u[4] = {.2, .8, .8, .2};
    static const double v[4] = {.2, .7, .9, .4};
    SmTArray<SmCurve*> curves;
    SmTArray<SmOrientType> orientations;
    std::unique_ptr<SmHermiteCurve> owned[4];
    for (int i = 0; i < 4; ++i)
    {
        const int j = (i + 1) % 4;
        const double du = u[j] - u[i], dv = v[j] - v[i];
        const SmPoint3d a = sh.at(u[i]) + sh.extrusion * v[i];
        const SmPoint3d b = sh.at(u[j]) + sh.extrusion * v[j];
        const SmVector3d da = sh.tangent(u[i]) * du + sh.extrusion * dv;
        const SmVector3d db = sh.tangent(u[j]) * du + sh.extrusion * dv;
        owned[i].reset(new (ctx) SmHermiteCurve(a, da, b, db, 3));
        curves.Add(owned[i].get());
        orientations.Add(SM_OT_SAME);
    }
    SmTArray<ULONG> loops;
    loops.Add(4);
    SmTArray<SmPoint3d> points;
    const SmExtent2d domain(SmPoint2d(0, 0), SmPoint2d(1, 1));
    SmRegion* region = nullptr;
    SmShell* shell = nullptr;
    SmFace* face = nullptr;
    // The factory can reject the surface before touching the curves. It nulls
    // deleted array entries and leaves adopted curves owned by their edges.
    struct UnconsumedCurves
    {
        SmTArray<SmCurve*>& values;
        ~UnconsumedCurves()
        {
            for (ULONG i = 0; i < values.GetSize(); ++i)
            {
                SmCurve* curve = values.GetAt(i);
                if (curve != nullptr && curve->GetOwner() == nullptr)
                    delete curve;
            }
        }
    } cleanup{curves};
    for (int i = 0; i < 4; ++i)
        owned[i].release();
    SmStatus status = brep->MakeFaceWithCurves(brep->GetInfiniteRegion(), loops, &curves, nullptr,
        orientations, points, surface.get(), domain, SM_OT_SAME, region, shell, face);
    if (surface->GetOwner() != nullptr)
        surface.release();
    if (status != SM_SUCCESS)
        return status;
    if (face == nullptr)
        return SM_ERR;
    SmTArray<SmEdge*> edges;
    face->GetEdges(edges);
    // The fixture is only valid when the native builder kept the four real Hermite
    // edges (no substitution, no culling); tolerances are then the fixture's.
    if (edges.GetSize() != 4)
        return SM_ERR;
    for (ULONG i = 0; i < edges.GetSize(); ++i)
    {
        SmEdge* e = edges[i];
        if (e == nullptr || e->GetCurve() == nullptr || e->GetCurve()->GetType() != SmHermiteCurve_TYPE)
            return SM_ERR;
        e->SetTolerance(SmZoneTol3d(edge_tol), FALSE, FALSE);
    }
    face->SetTolerance(SmZoneTol3d(1e-7), FALSE, FALSE);
    return SM_SUCCESS;
}
} // namespace sc_topology_fixture
