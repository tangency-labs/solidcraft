// SPDX-License-Identifier: Apache-2.0
// intersect.api_intersectors: SmApiIntersectCurves, SmApiIntersectSurfaces,
// SmApiIntersectFaces, SmApiIntersectBreps, SmApiIntersectCurveSurface,
// SmApiIntersectCurveFace and SmApiIntersectCurveBrep (source/SM_API/src/SmApiIntersectors.cpp)
// on operands the shim builds itself with SM_API constructors (SmApiCreateCurve,
// SmApiCreateLineSegment, SmApiCreateCircle, SmApiCreateArc, SmApiCreateCylinder,
// SmApiCreateSurfaceRevolution, SmApiCreateRuledSurface, SmApiCreateSurfaceFromCornerPoints,
// SmApiCreateBox, SmApiCreateSphere, SmApiBooleanDifference), SmBSplineSurface::CreateCanonical or the CreateCanonical
// of the analytic surfaces (SmPlane, SmCylinder, SmCone, SmSphere, SmSurfOfExtrusion,
// SmSurfOfRevolution) with an optional AdjustSTEPUVDomain. The Rust side builds the same operands with its own
// constructors, so no C++ state reaches it. Layout in
// crates/solidcore-ffi/src/units/intersect_api_intersectors.rs.
#include "sc_shim.h"
#include "StdAfx.h"
#include <SmBrep.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmAxis2Placement.h>
#include <SmCone.h>
#include <SmCurve.h>
#include <SmCylinder.h>
#include <SmFace.h>
#include <SmGeomUtility.h>
#include <SmLine.h>
#include <SmPlane.h>
#include <SmSphere.h>
#include <SmSurface.h>
#include <SmSurfOfExtrusion.h>
#include <SmSurfOfRevolution.h>
#include <SmSolutionArray.h>
#include <SmContext.h>
#include <SmApiBrep.h>
#include <SmApiCurves.h>
#include <SmApiGeneral.h>
#include <SmApiIntersectors.h>
#include <SmApiPrimitives.h>
#include <SmApiSurfaces.h>
#include <memory>
#include <vector>

namespace {
// Reads the argument stream; `ok` turns false past its end or on a malformed count.
struct Args {
    const double* a;
    int64_t n;
    int64_t at = 0;
    bool ok = true;
    double next() {
        if (at >= n) { ok = false; return 0.0; }
        return a[at++];
    }
    SmPoint3d point() {
        double x = next(), y = next(), z = next();
        return SmPoint3d(x, y, z);
    }
    // A count in [lo, hi].
    ULONG count(double lo, double hi) {
        double c = next();
        if (!(c >= lo && c <= hi) || c != (double)(ULONG)c) { ok = false; return 0; }
        return (ULONG)c;
    }
};

// A curve: 1 = SmApiCreateCurve(n points), 2 = SmApiCreateLineSegment(a, b),
// 3 = SmApiCreateCircle(center, r) (SmBSplineCurve overload), 4 = SmApiCreateArc(center,
// r, start, end). Null on a failed build (`status` says why).
SmCurve* build_curve(Args& in, SmStatus& status) {
    ULONG kind = in.count(1, 4);
    if (!in.ok) { status = SM_ERR_INVALID_INPUT; return nullptr; }
    if (kind == 1) {
        ULONG n = in.count(4, 64);
        SmTArray<SmPoint3d> pts;
        for (ULONG i = 0; i < n; ++i) pts.Add(in.point());
        if (!in.ok) { status = SM_ERR_INVALID_INPUT; return nullptr; }
        SmBSplineCurve* c = nullptr;
        status = SmApiCreateCurve(pts, c);
        if (status != SM_SUCCESS) { delete c; return nullptr; }
        return c;
    }
    if (kind == 2) {
        SmPoint3d p = in.point(), q = in.point();
        if (!in.ok) { status = SM_ERR_INVALID_INPUT; return nullptr; }
        SmLine* c = nullptr;
        status = SmApiCreateLineSegment(p, q, c);
        if (status != SM_SUCCESS) { delete c; return nullptr; }
        return c;
    }
    SmPoint3d center = in.point();
    double r = in.next();
    if (kind == 3) {
        if (!in.ok) { status = SM_ERR_INVALID_INPUT; return nullptr; }
        SmBSplineCurve* c = nullptr;
        status = SmApiCreateCircle(center, r, c);
        if (status != SM_SUCCESS) { delete c; return nullptr; }
        return c;
    }
    double s = in.next(), e = in.next();
    if (!in.ok) { status = SM_ERR_INVALID_INPUT; return nullptr; }
    SmBSplineCurve* c = nullptr;
    status = SmApiCreateArc(center, r, s, e, c);
    if (status != SM_SUCCESS) { delete c; return nullptr; }
    return c;
}

// The distinct knots and multiplicities of a clamped degree-`d` spline with `n` control
// points: n - d + 1 equally spaced values in [0, 1], end multiplicities d + 1.
void clamped_knots(ULONG n, ULONG d, SmTArray<double>& knots, SmTArray<ULONG>& mults) {
    ULONG m = n - d + 1;
    for (ULONG i = 0; i < m; ++i) {
        knots.Add((double)i / (double)(m - 1));
        mults.Add(i == 0 || i == m - 1 ? d + 1 : 1);
    }
}

// An analytic surface: `type`, its data, then `has_domain [umin vmin umax vmax]`
// (AdjustSTEPUVDomain of that STEP domain when has_domain is 1). Types: 1 = SmPlane (frame),
// 2 = SmCylinder (frame r), 3 = SmCone (frame rbot rtop h), 4 = SmSphere (frame r), where a
// frame is `origin xaxis yaxis` (SmAxis2Placement::SetCanonical); 5 = SmSurfOfExtrusion
// (n points of an SmApiCreateCurve generator, extrusion vector), 6 = SmSurfOfRevolution
// (n points of an SmApiCreateCurve generator, axis point, axis direction).
SmSurface* build_analytic(Args& in, SmStatus& status) {
    ULONG type = in.count(1, 6);
    if (!in.ok) { status = SM_ERR_INVALID_INPUT; return nullptr; }
    const SmContext& ctx = *SmApiGetOrCreateContext();
    SmSurface* s = nullptr;
    if (type <= 4) {
        SmPoint3d o = in.point(), x = in.point(), y = in.point();
        double a = 0.0, b = 0.0, h = 0.0;
        if (type == 2 || type == 4) a = in.next();
        if (type == 3) { a = in.next(); b = in.next(); h = in.next(); }
        if (!in.ok) { status = SM_ERR_INVALID_INPUT; return nullptr; }
        SmAxis2Placement frame;
        status = frame.SetCanonical(o, x, y);
        if (status != SM_SUCCESS) return nullptr;
        if (type == 1) {
            SmPlane* p = nullptr;
            status = SmPlane::CreateCanonical(ctx, frame, p);
            s = p;
        } else if (type == 2) {
            SmCylinder* p = nullptr;
            status = SmCylinder::CreateCanonical(ctx, frame, a, p);
            s = p;
        } else if (type == 3) {
            SmCone* p = nullptr;
            status = SmCone::CreateCanonical(ctx, frame, a, b, h, p);
            s = p;
        } else {
            SmSphere* p = nullptr;
            status = SmSphere::CreateCanonical(ctx, frame, a, p);
            s = p;
        }
    } else {
        ULONG n = in.count(4, 64);
        SmTArray<SmPoint3d> pts;
        for (ULONG i = 0; i < n; ++i) pts.Add(in.point());
        SmPoint3d p1 = in.point();
        SmPoint3d p2 = type == 6 ? in.point() : SmPoint3d(0, 0, 0);
        if (!in.ok) { status = SM_ERR_INVALID_INPUT; return nullptr; }
        SmBSplineCurve* gen = nullptr;
        status = SmApiCreateCurve(pts, gen);
        if (status != SM_SUCCESS) { delete gen; return nullptr; }
        // the new surface owns its generator (a failed creation leaks it: the shim runs
        // in a fresh child process)
        if (type == 5) {
            SmSurfOfExtrusion* p = nullptr;
            status = SmSurfOfExtrusion::CreateCanonical(ctx, gen, SmVector3d(p1), p);
            s = p;
        } else {
            SmSurfOfRevolution* p = nullptr;
            status = SmSurfOfRevolution::CreateCanonical(ctx, gen, p1, SmVector3d(p2), p);
            s = p;
        }
    }
    if (status != SM_SUCCESS || !s) {
        delete s;
        if (status == SM_SUCCESS) status = SM_ERR;
        return nullptr;
    }
    bool has_domain = in.next() != 0.0;
    if (has_domain) {
        double u0 = in.next(), v0 = in.next(), u1 = in.next(), v1 = in.next();
        if (!in.ok) { delete s; status = SM_ERR_INVALID_INPUT; return nullptr; }
        SmExtent2d d;
        status = d.SetMinMax(u0, v0, u1, v1);
        if (status == SM_SUCCESS) status = s->AdjustSTEPUVDomain(d);
        if (status != SM_SUCCESS) { delete s; return nullptr; }
    }
    if (!in.ok) { delete s; status = SM_ERR_INVALID_INPUT; return nullptr; }
    return s;
}

// UB-123 (plan/upstream-issues): SmCone::IntersectWithCone dereferences the absent
// tolerance SmApiIntersectSurfaces passes (NULL) for coincident, overlapping cylinders.
// True exactly for the pairs that reach that branch (the early exits of the guard in
// harness/cpp/shim/surface_cone_p3.cpp, over the natural UV domains).
bool ub_123(SmSurface* s1, SmSurface* s2) {
    auto is_cone = [](SmSurface* s) {
        return s->GetType() == SmCone_TYPE || s->GetType() == SmCylinder_TYPE;
    };
    if (s1 == s2 || !is_cone(s1) || !is_cone(s2)) return false;
    SmCone* a = (SmCone*)s1;
    const SmCone& c = *(SmCone*)s2;
    if (!a->IsCylinder() || !c.IsCylinder()) return false;
    SmExtent3d box1, box2;
    if (a->CalculateBoundingBox(s1->GetNaturalUVDomain(), &box1) != SM_SUCCESS
        || c.CalculateBoundingBox(s2->GetNaturalUVDomain(), &box2) != SM_SUCCESS)
        return false;
    double tol_default = SM_EFF_ZERO_SQRT * (1 + box2.GetSize().Length());
    box2.ExpandAbsolute(tol_default);
    if (box1.AreDisjoint(box2)) return false;
    SmVector3d axis = a->GetPosition().GetZAxis(), other_axis = c.GetPosition().GetZAxis();
    SmExtent2d d = a->GetSTEPUVDomain(), e = c.GetSTEPUVDomain();
    SmPoint3d bottom = a->GetPosition().GetOrigin() + d.GetVMin() * axis;
    SmPoint3d other_bottom = c.GetPosition().GetOrigin() + e.GetVMin() * other_axis;
    SmPoint3d other_top = c.GetPosition().GetOrigin() + e.GetVMax() * other_axis;
    double angle = 0;
    axis.AngleBetween(other_axis, angle);
    if (angle > SM_PI / 2) angle = SM_PI - angle;
    if (!(angle < SM_EFF_ZERO_SQRT)) return false;
    double t = 0, t2 = 0;
    smgu_LineClosestPoint(bottom, axis, other_bottom, t);
    double distance = (bottom + t * axis).DistanceBetween(other_bottom);
    double rmin = smos_Min(a->GetBotRadius(), a->GetTopRadius());
    double rmax = smos_Max(a->GetBotRadius(), a->GetTopRadius());
    double omin = smos_Min(c.GetBotRadius(), c.GetTopRadius());
    double omax = smos_Max(c.GetBotRadius(), c.GetTopRadius());
    bool radial_disjoint = distance - tol_default > rmax + omax
        || rmax + tol_default < omin - distance || omax + tol_default < rmin - distance;
    smgu_LineClosestPoint(bottom, axis, other_top, t2);
    double h = a->GetHeight();
    bool axial_disjoint = (t > h + tol_default && t2 > h + tol_default)
        || (t < 0.0 - tol_default && t2 < 0.0 - tol_default);
    return !radial_disjoint && !axial_disjoint && distance <= tol_default;
}

// sm_IntersectGenerators (SmSurfOfExtrusion.cpp:2658) replayed on copies of the two
// generators, up to its range-solution test (:2978): true when it would read the curve-type
// array SmApiIntersectSurfaces passes as NULL (`pOptCurveTypes->GetAt` at :2981, reached
// when the first range solution has both ends overlapping, `lNewCurveCount == 2`; a range
// solution with fewer returns "too complicated" first, without reading it). See UB-81
// (same lines: the NULL curve list and unset offsets).
bool generators_read_null_types(const SmSurfOfExtrusion& ext, const SmCurve& other_gen,
                                const SmVector3d& other_vec, const SmExtent1d& other_ivl,
                                double tol)
{
    SmBSplineCurve* gen = NULL;
    SmVector3d vec;
    ((SmSurfOfExtrusion&)ext).GetCanonical(gen, vec);
    if (!gen) return false;
    SmExtent1d ext_ivl = ext.GetSTEPUVDomain().GetVInterval();
    double ext_len = vec.Length();
    double other_len = other_vec.Length();
    if (!other_vec.IsParallelTo(vec, SM_EFF_ZERO_RAD)) return false;
    SmVector3d unit = vec;
    unit.Unitize();
    // (copies: the replay leaves the operands' curve caches as they are)
    const SmContext& ctx = *SmApiGetOrCreateContext();
    SmCurve* c1 = NULL;
    SmCurve* c2 = NULL;
    gen->Copy(ctx, c1);
    std::unique_ptr<SmCurve> own1(c1);
    other_gen.Copy(ctx, c2);
    std::unique_ptr<SmCurve> own2(c2);
    if (!c1 || !c2) return false;
    SmSolutionArray sols;
    if (c1->GlobalCurveSolve(gen->GetNaturalInterval(), *c2, other_gen.GetNaturalInterval(),
                             SM_SO_PROJECTED_INTERSECT, tol, NULL, &unit, SM_SR_ALL,
                             sols) != SM_SUCCESS)
        return false;
    if (sols.GetSize() == 0) return false;
    bool same_dir = other_vec.Dot(vec) > 0;
    SmExtent1d other_line_ivl =
        same_dir ? SmExtent1d(other_ivl.GetMin() * other_len / ext_len,
                              other_ivl.GetMax() * other_len / ext_len)
                 : SmExtent1d(other_ivl.GetMax() * -other_len / ext_len,
                              other_ivl.GetMin() * -other_len / ext_len);
    for (ULONG ii = 0; ii < sols.GetSize(); ii++) {
        SmSolution& sol = sols[ii];
        bool range = sol.m_eSolutionType == SM_ST_RANGE_OF_VALUES;
        ULONG count = 0;
        for (ULONG jj = 0; jj < 1 || (jj < 2 && range); jj++) {
            double ext_end = jj == 0 ? sol.m_vStart[0] : sol.m_vEnd[0];
            double other_end = jj == 0 ? sol.m_vStart[1] : sol.m_vEnd[1];
            SmPoint3d p1, p2;
            // (upstream returns these statuses before the test)
            if (c1->EvaluatePoint(ext_end, p1) != SM_SUCCESS) return false;
            if (c2->EvaluatePoint(other_end, p2) != SM_SUCCESS) return false;
            double line_param = 0.0;
            smgu_LineClosestPoint(p1, vec, p2, line_param);
            SmExtent1d mapped(other_line_ivl.GetMin() + line_param,
                              other_line_ivl.GetMax() + line_param);
            SmExtent1d xsect;
            ext_ivl.Intersect(mapped, xsect);
            if (xsect.IsInit()) continue;
            count++;
        }
        if (range) return count == 2;
    }
    return false;
}

// SmApiIntersectSurfaces inputs whose special case dereferences a NULL output or an unset
// generator inside sm_IntersectGenerators: an extrusion with an extrusion
// (IntersectWithSurfOfExtrusion), or an extrusion with a rectilinear cone/cylinder either way
// round (IntersectWithCylinder). For the cylinder, an unbounded height interval or a NULL
// iso circle is UB-93 (its generator is made from unset values or stays NULL).
bool ub_null_types(SmSurface* s1, SmSurface* s2)
{
    SmSurfOfExtrusion* ext = NULL;
    SmCone* cyl = NULL;
    if (s1->GetType() == SmSurfOfExtrusion_TYPE) {
        ext = (SmSurfOfExtrusion*)s1;
        if (s2->GetType() == SmSurfOfExtrusion_TYPE) {
            const SmSurfOfExtrusion& o = *(SmSurfOfExtrusion*)s2;
            if (!o.GetGenCurve()) return false;
            return generators_read_null_types(*ext, *o.GetGenCurve(), o.GetExtrusionVector(),
                                              o.GetSTEPUVDomain().GetVInterval(),
                                              SM_EFF_ZERO_SQRT);
        }
        // (SmSurfOfExtrusion::GlobalSurfaceIntersect: SmCone_TYPE only)
        if (s2->GetType() != SmCone_TYPE) return false;
        cyl = (SmCone*)s2;
    } else if ((s1->GetType() == SmCone_TYPE || s1->GetType() == SmCylinder_TYPE) &&
               s2->GetType() == SmSurfOfExtrusion_TYPE && ((SmCone*)s1)->IsCylinder()) {
        // (SmCone::GlobalSurfaceIntersect, the cylinder case)
        ext = (SmSurfOfExtrusion*)s2;
        cyl = (SmCone*)s1;
    } else {
        return false;
    }
    // SmSurfOfExtrusion::IntersectWithCylinder (:3164) to its sm_IntersectGenerators call
    double bot = cyl->GetBotRadius(), top = cyl->GetTopRadius();
    if (smos_Fabs(bot - top) > SM_EFF_ZERO * (1.0 + smos_Max(bot, top))) return false;
    double tol = SM_EFF_ZERO_SQRT;
    SmExtent1d ivl(cyl->GetBotHeight(), cyl->GetTopHeight());
    if (ivl.GetMin() <= -SM_INFINITE_PARAMETER || ivl.GetMax() >= SM_INFINITE_PARAMETER)
        return true;
    SmVector3d axis = cyl->GetPosition().GetZAxis();
    SmPoint3d start(cyl->GetPosition().GetOriginRef() + ivl.GetMin() * axis);
    SmBSplineCurve* circle = NULL;
    cyl->CreateIsoCircleFromPoint(*SmApiGetOrCreateContext(), start, tol, circle);
    std::unique_ptr<SmBSplineCurve> own(circle);
    if (!circle) return true;
    return generators_read_null_types(*ext, *circle, axis, ivl, tol);
}

// A surface: 1 = SmBSplineSurface::CreateCanonical(nu x nv net, degrees du, dv, clamped
// knots), 2 = SmApiCreateCylinder(origin, r, h) (SmSurface overload),
// 3 = SmApiCreateSurfaceRevolution(SmApiCreateCurve(points), origin, axis, angle),
// 4 = SmApiCreateRuledSurface(SmApiCreateCurve(points), SmApiCreateCurve(points)),
// 5 = SmApiCreateSurfaceFromCornerPoints(uminvmin, umaxvmin, uminvmax, umaxvmax),
// 6 = an analytic surface (build_analytic),
// 7 = SmApiCreateSurfaceFromOrderedPoints(rows x columns points, row by row).
SmSurface* build_surface(Args& in, SmStatus& status) {
    ULONG kind = in.count(1, 7);
    if (!in.ok) { status = SM_ERR_INVALID_INPUT; return nullptr; }
    if (kind == 1) {
        ULONG nu = in.count(2, 16), nv = in.count(2, 16);
        ULONG du = in.count(1, 3), dv = in.count(1, 3);
        if (!in.ok || du >= nu || dv >= nv) { status = SM_ERR_INVALID_INPUT; return nullptr; }
        SmTArray<SmPoint3d> pts;
        for (ULONG i = 0; i < nu * nv; ++i) pts.Add(in.point());
        if (!in.ok) { status = SM_ERR_INVALID_INPUT; return nullptr; }
        SmTArray<double> uk, vk;
        SmTArray<ULONG> um, vm;
        clamped_knots(nu, du, uk, um);
        clamped_knots(nv, dv, vk, vm);
        SmBSplineSurface* s = nullptr;
        status = SmBSplineSurface::CreateCanonical(*SmApiGetOrCreateContext(), du, dv, pts,
            SM_SF_UNSPECIFIED, um, vm, uk, vk, SM_KT_UNSPECIFIED, NULL, NULL, s);
        if (status != SM_SUCCESS) { delete s; return nullptr; }
        return s;
    }
    if (kind == 2) {
        SmPoint3d o = in.point();
        double r = in.next(), h = in.next();
        if (!in.ok) { status = SM_ERR_INVALID_INPUT; return nullptr; }
        SmSurface* s = nullptr;
        status = SmApiCreateCylinder(o, r, h, s);
        if (status != SM_SUCCESS) { delete s; return nullptr; }
        return s;
    }
    if (kind == 5) {
        SmPoint3d c1 = in.point(), c2 = in.point(), c3 = in.point(), c4 = in.point();
        if (!in.ok) { status = SM_ERR_INVALID_INPUT; return nullptr; }
        SmSurface* s = nullptr;
        status = SmApiCreateSurfaceFromCornerPoints(c1, c2, c3, c4, s);
        if (status != SM_SUCCESS) { delete s; return nullptr; }
        return s;
    }
    if (kind == 6) return build_analytic(in, status);
    if (kind == 7) {
        ULONG rows = in.count(2, 16), cols = in.count(2, 16);
        if (!in.ok) { status = SM_ERR_INVALID_INPUT; return nullptr; }
        SmTArray<SmPoint3d> pts;
        for (ULONG i = 0; i < rows * cols; ++i) pts.Add(in.point());
        if (!in.ok) { status = SM_ERR_INVALID_INPUT; return nullptr; }
        SmSurface* s = nullptr;
        status = SmApiCreateSurfaceFromOrderedPoints(pts, rows, cols, s);
        if (status != SM_SUCCESS) { delete s; return nullptr; }
        return s;
    }
    auto points_curve = [&](SmBSplineCurve*& c) {
        ULONG n = in.count(4, 64);
        SmTArray<SmPoint3d> pts;
        for (ULONG i = 0; i < n; ++i) pts.Add(in.point());
        if (!in.ok) { status = SM_ERR_INVALID_INPUT; return; }
        status = SmApiCreateCurve(pts, c);
    };
    if (kind == 3) {
        SmBSplineCurve* c = nullptr;
        points_curve(c);
        std::unique_ptr<SmBSplineCurve> gen(c);
        SmPoint3d o = in.point();
        SmVector3d axis = in.point();
        double angle = in.next();
        if (!in.ok) { status = SM_ERR_INVALID_INPUT; return nullptr; }
        if (status != SM_SUCCESS) return nullptr;
        SmSurface* s = nullptr;
        status = SmApiCreateSurfaceRevolution(gen.get(), o, axis, angle, s);
        if (status != SM_SUCCESS) { delete s; return nullptr; }
        return s;
    }
    SmBSplineCurve* c1 = nullptr;
    points_curve(c1);
    std::unique_ptr<SmBSplineCurve> g1(c1);
    if (!in.ok || status != SM_SUCCESS) return nullptr;
    SmBSplineCurve* c2 = nullptr;
    points_curve(c2);
    std::unique_ptr<SmBSplineCurve> g2(c2);
    if (!in.ok || status != SM_SUCCESS) return nullptr;
    SmSurface* s = nullptr;
    status = SmApiCreateRuledSurface(g1.get(), g2.get(), s);
    if (status != SM_SUCCESS) { delete s; return nullptr; }
    return s;
}

// A body: 1 = SmApiCreateBox(origin, l, w, h), 2 = SmApiCreateSphere(origin, r),
// 3 = SmApiCreateCylinder(origin, r, h) (SmBrep overload), 4 = SmApiBooleanDifference of
// SmApiCreateBox(origin, l, w, h) and SmApiCreateCylinder(corigin, r, ch) (a box with a hole).
SmBrep* build_body(Args& in, SmStatus& status) {
    ULONG kind = in.count(1, 4);
    SmPoint3d o = in.point();
    if (!in.ok) { status = SM_ERR_INVALID_INPUT; return nullptr; }
    SmBrep* b = nullptr;
    if (kind == 1) {
        double l = in.next(), w = in.next(), h = in.next();
        if (!in.ok) { status = SM_ERR_INVALID_INPUT; return nullptr; }
        status = SmApiCreateBox(o, l, w, h, b);
    } else if (kind == 2) {
        double r = in.next();
        if (!in.ok) { status = SM_ERR_INVALID_INPUT; return nullptr; }
        status = SmApiCreateSphere(o, r, b);
    } else if (kind == 3) {
        double r = in.next(), h = in.next();
        if (!in.ok) { status = SM_ERR_INVALID_INPUT; return nullptr; }
        status = SmApiCreateCylinder(o, r, h, b);
    } else {
        double l = in.next(), w = in.next(), h = in.next();
        SmPoint3d co = in.point();
        double r = in.next(), ch = in.next();
        if (!in.ok) { status = SM_ERR_INVALID_INPUT; return nullptr; }
        status = SmApiCreateBox(o, l, w, h, b);
        if (status != SM_SUCCESS) { delete b; return nullptr; }
        SmBrep* cyl = nullptr;
        status = SmApiCreateCylinder(co, r, ch, cyl);
        if (status != SM_SUCCESS) { delete cyl; delete b; return nullptr; }
        // on success the result aliases b and cyl is deleted; on failure both remain
        SmBrep* holed = nullptr;
        status = SmApiBooleanDifference(b, cyl, holed);
        if (status != SM_SUCCESS || !holed) {
            delete cyl;
            delete b;
            if (status == SM_SUCCESS) status = SM_ERR;
            return nullptr;
        }
        return holed;
    }
    if (status != SM_SUCCESS) { delete b; return nullptr; }
    return b;
}

// `type lo hi` and five evaluated points (`status x y z` each) of `c`.
void put_curve(std::vector<double>& o, SmCurve* c) {
    o.push_back((double)c->GetType());
    SmExtent1d d = c->GetNaturalInterval();
    o.push_back(d.GetMin());
    o.push_back(d.GetMax());
    for (int i = 0; i < 5; ++i) {
        SmPoint3d p;
        SmStatus s = c->EvaluatePoint(d.Evaluate((double)i / 4), p);
        o.push_back(s == SM_SUCCESS ? 0.0 : 1.0);
        if (s == SM_SUCCESS) { o.push_back(p.x); o.push_back(p.y); o.push_back(p.z); }
    }
}

int32_t refused(SmStatus status, double* out, int64_t cap, int64_t* out_len) {
    std::vector<double> o = {-999.0, (double)status};
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
}

// The caller-owned curves the call left in the array (after a failed
// SmApiIntersectFaces the array is empty: it deleted and reset them itself).
void delete_curves(SmTArray<SmCurve*>& curves) {
    for (ULONG i = 0; i < curves.GetSize(); ++i) delete curves[i];
}

// `status n curve* npoints point*` (npoints -1 when not asked for): what the call left
// in the arrays, also on a failed status (SmApiIntersectBreps' partial outputs;
// SmApiIntersectFaces empties them).
void put_intersections(std::vector<double>& o, SmStatus status, SmTArray<SmCurve*>& curves,
                       SmTArray<SmPoint3d>* points) {
    o.push_back((double)status);
    o.push_back((double)curves.GetSize());
    for (ULONG i = 0; i < curves.GetSize(); ++i) put_curve(o, curves[i]);
    if (!points) { o.push_back(-1.0); return; }
    o.push_back((double)points->GetSize());
    for (ULONG i = 0; i < points->GetSize(); ++i) {
        o.push_back((*points)[i].x);
        o.push_back((*points)[i].y);
        o.push_back((*points)[i].z);
    }
}
}

// Ops (the argument stream starts after the op):
// 1 SmApiIntersectCurves: want_points, same, curve1 [curve2 unless same]
//   -> `status n (t1 t2 [x y z])*` (`status 0` on a failed status);
// 2 SmApiIntersectSurfaces: surface1 surface2 -> `status n curve*` (the curves the call left
//   in the array, also on a failed status); `-998 0` for the coincident cylinders of UB-123;
//   `-997 0` for the extrusion pairs whose sm_IntersectGenerators dereferences the NULL
//   curve-type array or an unset cylinder generator (ub_null_types);
// 3 SmApiIntersectFaces: want_points, one, body1 [body2 unless one], face1, face2 (indices
//   into GetFaces of body1 and of body2, or both of body1 with `one`);
// 4 SmApiIntersectBreps: want_points, same, body1 [body2 unless same];
//   3 and 4 -> `status n curve* npoints point*`;
// 5 SmApiIntersectCurveSurface: curve, surface;
// 6 SmApiIntersectCurveFace: curve, body, face (index into GetFaces);
// 7 SmApiIntersectCurveBrep: curve, body;
//   5 to 7 -> `status n curve* npoints point*` (empty on a failed status: the call
//   removes its own outputs).
// A failed operand build or a bad face index returns `-999 status`.
SC_EXPORT int32_t sc_intersect_api_intersectors_eval(int32_t op, const double* a, int64_t na,
        double* out, int64_t cap, int64_t* out_len) {
    if (op < 1 || op > 7 || !a || na < 0 || !out_len) return SC_ERR_ARG;
    SC_TRY
    Args in{a, na};
    SmStatus status = SM_SUCCESS;
    std::vector<double> o;
    if (op >= 5) {
        std::unique_ptr<SmCurve> c(build_curve(in, status));
        if (!c) return refused(status, out, cap, out_len);
        SmTArray<SmCurve*> curves;
        SmTArray<SmPoint3d> points;
        if (op == 5) {
            std::unique_ptr<SmSurface> s(build_surface(in, status));
            if (!s) return refused(status, out, cap, out_len);
            if (in.at != in.n) return SC_ERR_ARG;
            status = SmApiIntersectCurveSurface(c.get(), s.get(), curves, points);
        } else {
            std::unique_ptr<SmBrep> b(build_body(in, status));
            if (!b) return refused(status, out, cap, out_len);
            if (op == 6) {
                ULONG fi = in.count(0, 1e6);
                if (!in.ok || in.at != in.n) return SC_ERR_ARG;
                SmTArray<SmFace*> faces;
                b->GetFaces(faces);
                if (fi >= faces.GetSize()) return refused(SM_ERR_INVALID_INPUT, out, cap, out_len);
                status = SmApiIntersectCurveFace(c.get(), faces[fi], curves, points);
            } else {
                if (in.at != in.n) return SC_ERR_ARG;
                status = SmApiIntersectCurveBrep(c.get(), b.get(), curves, points);
            }
        }
        put_intersections(o, status, curves, &points);
        delete_curves(curves);
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    if (op == 1) {
        bool want_points = in.next() != 0.0;
        bool same = in.next() != 0.0;
        std::unique_ptr<SmCurve> c1(build_curve(in, status));
        if (!c1) return refused(status, out, cap, out_len);
        std::unique_ptr<SmCurve> c2;
        if (!same) {
            c2.reset(build_curve(in, status));
            if (!c2) return refused(status, out, cap, out_len);
        }
        if (in.at != in.n) return SC_ERR_ARG;
        SmTArray<SmPoint3d> points;
        SmTArray<double> t1, t2;
        status = SmApiIntersectCurves(c1.get(), same ? c1.get() : c2.get(),
                                      want_points ? &points : NULL, &t1, &t2);
        o.push_back((double)status);
        if (status != SM_SUCCESS) { o.push_back(0.0); }
        else {
            o.push_back((double)t1.GetSize());
            for (ULONG i = 0; i < t1.GetSize(); ++i) {
                o.push_back(t1[i]);
                o.push_back(t2[i]);
                if (want_points) {
                    o.push_back(points[i].x);
                    o.push_back(points[i].y);
                    o.push_back(points[i].z);
                }
            }
        }
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    if (op == 2) {
        std::unique_ptr<SmSurface> s1(build_surface(in, status));
        if (!s1) return refused(status, out, cap, out_len);
        std::unique_ptr<SmSurface> s2(build_surface(in, status));
        if (!s2) return refused(status, out, cap, out_len);
        if (in.at != in.n) return SC_ERR_ARG;
        if (ub_123(s1.get(), s2.get())) {
            std::vector<double> r = {-998.0, 0.0};
            return sc_copy_out(r.data(), (int64_t)r.size(), out, cap, out_len);
        }
        if (ub_null_types(s1.get(), s2.get())) {
            std::vector<double> r = {-997.0, 0.0};
            return sc_copy_out(r.data(), (int64_t)r.size(), out, cap, out_len);
        }
        SmTArray<SmCurve*> curves;
        status = SmApiIntersectSurfaces(s1.get(), s2.get(), &curves);
        // what the call left in the array, also on a failed status (curves a special-case
        // intersector appended before failing)
        o.push_back((double)status);
        o.push_back((double)curves.GetSize());
        for (ULONG i = 0; i < curves.GetSize(); ++i) put_curve(o, curves[i]);
        delete_curves(curves);
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    bool want_points = in.next() != 0.0;
    bool single = in.next() != 0.0;
    std::unique_ptr<SmBrep> b1(build_body(in, status));
    if (!b1) return refused(status, out, cap, out_len);
    std::unique_ptr<SmBrep> b2;
    if (!single) {
        b2.reset(build_body(in, status));
        if (!b2) return refused(status, out, cap, out_len);
    }
    SmTArray<SmCurve*> curves;
    SmTArray<SmPoint3d> points;
    if (op == 3) {
        ULONG i1 = in.count(0, 1e6), i2 = in.count(0, 1e6);
        if (!in.ok || in.at != in.n) return SC_ERR_ARG;
        SmTArray<SmFace*> f1, f2;
        b1->GetFaces(f1);
        (single ? b1 : b2)->GetFaces(f2);
        if (i1 >= f1.GetSize() || i2 >= f2.GetSize()) return refused(SM_ERR_INVALID_INPUT, out, cap, out_len);
        status = SmApiIntersectFaces(f1[i1], f2[i2], curves, want_points ? &points : NULL);
    } else {
        if (in.at != in.n) return SC_ERR_ARG;
        status = SmApiIntersectBreps(b1.get(), single ? b1.get() : b2.get(), curves,
                                     want_points ? &points : NULL);
    }
    put_intersections(o, status, curves, want_points ? &points : NULL);
    delete_curves(curves);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
