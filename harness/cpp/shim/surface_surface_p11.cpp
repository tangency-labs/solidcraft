// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surface_p11`: SmSurface::LocalSurfaceIntersect
// (source/SMLib/src/SmSurface.cpp:27439).
//
// Each of the two surfaces is either a non-rational biquadratic Bezier patch (3x3 net over
// [0,1]^2, kind 0), an analytic SmPlane (kind 1: origin net[0], x axis net[1], y axis net[2],
// unit scales, domain [0,1]^2) or a canonical analytic SmTorus (kind 2: centre net[0], sweep
// start net[1], 90 degree direction net[2], (major, minor) radii net[3][0..1], full analytic
// STEP domain [0,360]x[0,360] degrees, its NURB domain the case's own [0,1]^2). The layout is
// documented and validated in crates/solidcore-ffi/src/units/surface_surface_p11.rs.
//
// One entry point, sc_surface_surface_p11_lsi. Output: [status, found, u, v, u2, v2, deviation]
// (the solution values only when found).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmNurbsCrv.h>
#include <SmPlane.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmSurfTypes.h>
#include <SmTArray.h>
#include <SmTol.h>
#include <SmTorus.h>
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
    if (kind == 2)
    {
        // Canonical torus (SmTorus::CreateCanonical): centre, sweep start and 90 degree
        // direction from net[0..3], (major, minor) radii from net[3][0..1], the full analytic
        // STEP domain [0,360]x[0,360] degrees (the NURB is built by CreateCanonical over the
        // caller's [0,1]^2 NURB domain). Radii and axes are validated in the FFI wrapper;
        // a construction failure is reported as an invalid fixture, not as an operation
        // status.
        SmAxis2Placement frame(SmPoint3d(xyz[0], xyz[1], xyz[2]),
                               SmVector3d(xyz[3], xyz[4], xyz[5]),
                               SmVector3d(xyz[6], xyz[7], xyz[8]));
        SmExtent2d analytic(0, 0, 360, 360);
        SmTorus* torus = NULL;
        if (SmTorus::CreateCanonical(context, frame, xyz[9], xyz[10], torus, FALSE, FALSE, NULL,
                                     &analytic) != SM_SUCCESS)
            return NULL;
        return torus;
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

// SmSurface::LiftCurve (SmSurface.cpp:24054) of a UV curve on a surface. The surface is read
// with SmBSplineSurface::ReadFromFile(surf_path) when surf_path is given, else made from kind
// and xyz as above; the UV curve is read with SmBSplineCurve::ReadFromFile(curve_path) when
// curve_path is given, else it is a non-rational 2D SmBSplineCurve(gw_CURVE*, 2, FALSE) of
// degree `degree` with ncp control points (u, v; z = NL_NOZ) and ncp + degree + 1 knots. The
// domain (umin, vmin, umax, vmax) and interval are the surface's natural UV domain and the
// curve's natural interval unless given. Output: [status, max_dist, type, dim, degree, ncp,
// cpts (x y z w), nknots, knots, mults, interval min, max] of the lifted curve (only the status
// when LiftCurve fails). The curve values come from GetOrCreateGwNurbPointer and GetKnots.
SC_EXPORT int32_t sc_surface_surface_p11_lift(const char* surf_path, int32_t kind, const double* xyz,
                                              int32_t has_domain, const double* domain,
                                              const char* curve_path, int32_t degree, int64_t ncp,
                                              const double* cpts, const double* knots,
                                              int32_t has_interval, const double* interval, double tol,
                                              int32_t skip_checks, double* output, int64_t cap,
                                              int64_t* out_len)
{
    SC_TRY
    SmContext context;
    SmSurface* s = NULL;
    if (surf_path != NULL)
    {
        SmBSplineSurface* bs = NULL;
        SmBSplineSurface::ReadFromFile(context, surf_path, bs);
        s = bs;
    }
    else
        s = sc_p11_surface(context, kind, xyz);
    if (s == NULL)
        return SC_ERR_ARG;
    SmBSplineCurve* uv = NULL;
    if (curve_path != NULL)
        SmBSplineCurve::ReadFromFile(context, curve_path, uv);
    else
    {
        gw_INDEX n = (gw_INDEX)(ncp - 1);
        gw_INDEX m = (gw_INDEX)(ncp + degree);
        gw_CURVE* cur = sm_AllocateNurbCurve(n, (gw_DEGREE)degree, m);
        if (cur != NULL)
        {
            for (gw_INDEX i = 0; i <= n; i++)
            {
                cur->pol->Pw[i].x = cpts[2 * i];
                cur->pol->Pw[i].y = cpts[2 * i + 1];
                cur->pol->Pw[i].z = NL_NOZ;
                cur->pol->Pw[i].w = NL_NOW;
            }
            for (gw_INDEX i = 0; i <= m; i++)
                cur->knt->U[i] = knots[i];
            uv = new (context) SmBSplineCurve(cur, 2, FALSE);
        }
    }
    if (uv == NULL)
    {
        delete s;
        return SC_ERR_ARG;
    }
    SmExtent2d dom = has_domain ? SmExtent2d(domain[0], domain[1], domain[2], domain[3]) : s->GetNaturalUVDomain();
    SmExtent1d ivl = has_interval ? SmExtent1d(interval[0], interval[1]) : uv->GetNaturalInterval();
    double max_dist = -1.0;
    SmBSplineCurve* lifted = NULL;
    SmStatus r = s->LiftCurve(context, dom, *uv, ivl, SmApproxTol3d(tol), max_dist, lifted,
                              skip_checks ? TRUE : FALSE);
    std::vector<double> out;
    out.push_back(r == SM_SUCCESS ? 0.0 : (double)r);
    bool bad = false;
    if (r == SM_SUCCESS)
    {
        gw_CURVE* cur = lifted != NULL ? lifted->GetOrCreateGwNurbPointer() : NULL;
        SmTArray<double> ks;
        SmTArray<ULONG> mults;
        if (cur == NULL || lifted->GetKnots(ks, &mults) != SM_SUCCESS)
            bad = true;
        else
        {
            out.push_back(max_dist);
            out.push_back((double)lifted->GetType());
            out.push_back((double)lifted->GetDim());
            out.push_back((double)cur->p);
            out.push_back((double)(cur->pol->n + 1));
            for (gw_INDEX i = 0; i <= cur->pol->n; i++)
            {
                out.push_back(cur->pol->Pw[i].x);
                out.push_back(cur->pol->Pw[i].y);
                out.push_back(cur->pol->Pw[i].z);
                out.push_back(cur->pol->Pw[i].w);
            }
            out.push_back((double)ks.GetSize());
            for (ULONG i = 0; i < ks.GetSize(); i++)
                out.push_back(ks[i]);
            for (ULONG i = 0; i < mults.GetSize(); i++)
                out.push_back((double)mults[i]);
            SmExtent1d nat = lifted->GetNaturalInterval();
            out.push_back(nat.GetMin());
            out.push_back(nat.GetMax());
        }
    }
    delete lifted;
    delete uv;
    delete s;
    if (bad)
        return SC_ERR_ARG;
    return sc_copy_out(out.data(), (int64_t)out.size(), output, cap, out_len);
    SC_CATCH
}
