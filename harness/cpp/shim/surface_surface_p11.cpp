// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surface_p11`: SmSurface::LocalSurfaceIntersect
// (source/SMLib/src/SmSurface.cpp:27439).
//
// Each of the two surfaces is either a non-rational biquadratic Bezier patch (3x3 net over
// [0,1]^2, kind 0) or an analytic SmPlane (kind 1: origin net[0], x axis net[1], y axis net[2],
// unit scales, domain [0,1]^2). The layout is documented and validated in
// crates/solidcore-ffi/src/units/surface_surface_p11.rs.
//
// One entry point, sc_surface_surface_p11_lsi. Output: [status, found, u, v, u2, v2, deviation]
// (the solution values only when found).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmPlane.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmSurfTypes.h>
#include <SmTArray.h>
#include <SmTol.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

static SmSurface* sc_p11_surface(SmContext& context, int32_t kind, const double* xyz)
{
    if (kind == 1)
    {
        return new (context) SmPlane(SmPoint3d(xyz[0], xyz[1], xyz[2]), SmVector3d(xyz[3], xyz[4], xyz[5]),
                                     SmVector3d(xyz[6], xyz[7], xyz[8]), SmVector2d(1, 1),
                                     SmExtent2d(0, 0, 1, 1), &context);
    }
    gw_CPOINT points[3][3];
    gw_CPOINT* rows[3] = {points[0], points[1], points[2]};
    for (int i = 0; i < 3; i++)
        for (int j = 0; j < 3; j++)
        {
            gw_CPOINT& p = points[i][j];
            const int at = 3 * (3 * i + j);
            p.x = xyz[at];
            p.y = xyz[at + 1];
            p.z = xyz[at + 2];
            p.w = NL_NOW;
        }
    double knots[6] = {0, 0, 0, 1, 1, 1};
    return new (context) SmBSplineSurface(2, 2, 2, 2, 5, 5, rows, knots, knots);
}

// kinds[2]; xyz[2*27]; domains[2*4] (umin, vmin, umax, vmax); guesses[4] (u, v, u2, v2);
// tol; plane_normal[3] used when has_normal; previous_uv[2] used when has_previous.
SC_EXPORT int32_t sc_surface_surface_p11_lsi(const int32_t* kinds, const double* xyz,
                                             const double* domains, const double* guesses, double tol,
                                             int32_t has_normal, const double* plane_normal,
                                             int32_t has_previous, const double* previous_uv,
                                             int32_t boundary_testing, double* output, int64_t cap,
                                             int64_t* out_len)
{
    SC_TRY
    SmContext context;
    SmSurface* a = sc_p11_surface(context, kinds[0], xyz);
    SmSurface* b = sc_p11_surface(context, kinds[1], xyz + 27);
    if (a == NULL || b == NULL)
    {
        delete a;
        delete b;
        return SC_ERR_ARG;
    }
    SmExtent2d dom_a(domains[0], domains[1], domains[2], domains[3]);
    SmExtent2d dom_b(domains[4], domains[5], domains[6], domains[7]);
    SmPoint2d uv(guesses[0], guesses[1]);
    SmPoint2d uv2(guesses[2], guesses[3]);
    SmVector3d normal(plane_normal[0], plane_normal[1], plane_normal[2]);
    SmPoint2d previous(previous_uv[0], previous_uv[1]);
    SmBoolean found = FALSE;
    SmSolution solution;
    SmStatus r = a->LocalSurfaceIntersect(dom_a, *b, dom_b, tol, uv, uv2, has_normal ? &normal : NULL,
                                          has_previous ? &previous : NULL, boundary_testing ? TRUE : FALSE,
                                          found, solution);
    std::vector<double> out;
    out.push_back(r == SM_SUCCESS ? 0.0 : (double)r);
    if (r == SM_SUCCESS)
    {
        out.push_back(found ? 1.0 : 0.0);
        if (found)
        {
            for (int i = 0; i < 4; i++)
                out.push_back(solution.m_vStart[i]);
            out.push_back(solution.m_vStart.m_dSolutionValue);
        }
    }
    delete a;
    delete b;
    return sc_copy_out(out.data(), (int64_t)out.size(), output, cap, out_len);
    SC_CATCH
}

// SmSurface::IntersectSurfaceBoundaries (SmSurface.cpp:21745) of surface a with b (same
// layout as above). Output: [status, n, per curve: deviation, interval min, max, 5 points
// at the interval's quarters (x, y, z)]. The 3D curve array is always passed (upstream
// dereferences it unconditionally); the UV curves and types are always NULL / SM_TC_TANGENT.
SC_EXPORT int32_t sc_surface_surface_p11_isb(const int32_t* kinds, const double* xyz,
                                             const double* domains, double tol, double* output,
                                             int64_t cap, int64_t* out_len)
{
    SC_TRY
    SmContext context;
    SmSurface* a = sc_p11_surface(context, kinds[0], xyz);
    SmSurface* b = sc_p11_surface(context, kinds[1], xyz + 27);
    if (a == NULL || b == NULL)
    {
        delete a;
        delete b;
        return SC_ERR_ARG;
    }
    SmExtent2d dom_a(domains[0], domains[1], domains[2], domains[3]);
    SmExtent2d dom_b(domains[4], domains[5], domains[6], domains[7]);
    SmTArray<SmCurve*> curves;
    SmTArray<SmTsectCurveType> types;
    SmTArray<double> deviations;
    SmStatus r = a->IntersectSurfaceBoundaries(context, dom_a, *b, dom_b, SmApproxTol3d(tol), &curves, NULL,
                                               NULL, &types, &deviations);
    std::vector<double> out;
    out.push_back(r == SM_SUCCESS ? 0.0 : (double)r);
    bool bad = false;
    if (r == SM_SUCCESS)
    {
        out.push_back((double)curves.GetSize());
        for (ULONG i = 0; i < curves.GetSize(); i++)
        {
            SmCurve* c = curves[i];
            out.push_back(deviations[i]);
            out.push_back((double)types[i]);
            SmExtent1d ivl = c->GetNaturalInterval();
            out.push_back(ivl.GetMin());
            out.push_back(ivl.GetMax());
            for (int j = 0; j <= 4; j++)
            {
                SmPoint3d p;
                if (c->EvaluatePoint(ivl.Evaluate((double)j / 4.0), p) != SM_SUCCESS)
                    bad = true;
                out.push_back(p.x);
                out.push_back(p.y);
                out.push_back(p.z);
            }
        }
    }
    for (ULONG i = 0; i < curves.GetSize(); i++)
        delete curves[i];
    delete a;
    delete b;
    if (bad)
        return SC_ERR_ARG;
    return sc_copy_out(out.data(), (int64_t)out.size(), output, cap, out_len);
    SC_CATCH
}
