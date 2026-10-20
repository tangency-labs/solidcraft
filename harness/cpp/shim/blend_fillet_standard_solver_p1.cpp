// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_standard_solver_p1`.
// Wraps source/SMLib/src/SmFilletStandardSolver.cpp: the SmCircularCrossSectionFSG and
// SmBlendCurveCrossSectionFSG CreateSurfaceFromCurves, SmLinearCrossSectionFSG::CreateSurface,
// SmCurveBasedFS::EvaluateBaseCurve, SmSurfaceSurfaceFS::SetupOffsetExtension, the
// SmConstantRadiusFS surface constructor, SetupOffsetValues and LoadJacobian, the
// SmConstantDistanceFS SetupOffsetValues, LoadJacobian and LoadInitialValues and the
// SmVariableRadiusFS radius queries and SetupOffsetValues.
//
// One entry point, sc_blend_fillet_standard_solver_p1_eval(op, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/blend_fillet_standard_solver_p1.rs,
// which validates every input so upstream only touches valid memory. `out` receives the op
// status (0 = SM_SUCCESS, else the SmStatus value; -1 rejected, see below) and the op's outputs.
//
// The SmConstantDistanceFS constructor takes an edge use: the solver is built on the first edge
// of an SmApiCreateBox box, then a derived class replaces its offset surfaces, orientations,
// distance and extension factor and clears its edge uses. The generators that need a solver
// get an SmConstantRadiusFS over two bilinear surfaces (they read its context only).
//
// Two inputs make upstream read out of bounds; the shim detects them with a dry run of the
// steps leading there (copies of the inputs, the same upstream calls) and returns status -1
// (rejected) without running the operation:
// - SmCircularCrossSectionFSG::CreateSurfaceFromCurves building a ruled or revolved surface
//   with approximated arcs whose unique knot counts differ: its second pass indexes past the
//   three cross sections it made;
// - SmBlendCurveCrossSectionFSG::CreateSurfaceFromCurves skinning with `bSyncronized = TRUE`
//   either a point cross section (rails within tolerance at a section parameter: the skinning
//   reads the point curve's control points as if it had the blend curves' count) or with the
//   G1 cross-boundary derivative surfaces (N_AdjustDerivSrf turns the Bezier strips so that the
//   first skinning attempt takes a 2-point derivative curve along the ruling and reads it as if
//   it had the cross sections' control point count); also the knot insertion that adds no knot
//   (upstream loops forever).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmApiPrimitives.h>
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmContext.h>
#include <SmEdge.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtentNd.h>
#include <SmFilletExecutive.h>
#include <SmFilletSolver.h>
#include <SmFilletStandardSolver.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmMatrix.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmOffsetSurface.h>
#include <SmPlane.h>
#include <SmSurfaceIntersector.h>
#include <SmTArray.h>
#include <nurbs.h>

#include <vector>

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/blend_fillet_standard_solver_p1.rs).
enum : int32_t
{
    OP_CIRC = 0,
    OP_BLEND = 1,
    OP_LINEAR = 2,
    OP_BASE = 3,
    OP_SS_EXT = 4,
    OP_CR = 5,
    OP_CR_JAC = 6,
    OP_CD_SETUP = 7,
    OP_CD_JAC = 8,
    OP_CD_INIT = 9,
    OP_VR = 10,
    OP_VR_SETUP = 11,
};

const double REJECTED = -1.0;

struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    bool flag() { return in[at++] != 0.0; }
    SmPoint3d pt3()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmPoint3d(x, y, z);
    }
};

double st(SmStatus s)
{
    return s == SM_SUCCESS ? 0.0 : (double)s;
}

double flg(bool b)
{
    return b ? 1.0 : 0.0;
}

void put3(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

// Reads `n p m cpts knots` into a gw_CURVE.
gw_CURVE* read_nurb_curve(Rd& rd)
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

// A B-spline curve `n p m cpts knots` (SmBSplineCurve(gw_CURVE*, 3, FALSE)).
SmBSplineCurve* read_bcurve(Rd& rd, const SmContext& ctx)
{
    return new (ctx) SmBSplineCurve(read_nurb_curve(rd), 3, FALSE);
}

// A curve `kind ...`: 0 B-spline `oob n p m cpts knots`; 1 line `start end`
// (SmLine(start, end, 3, ctx)); 2 Hermite `p1 d1 p2 d2`.
SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 0)
    {
        bool oob = rd.flag();
        SmBSplineCurve* c = read_bcurve(rd, ctx);
        c->SetOutOfBoundsEnabled(oob ? TRUE : FALSE);
        return c;
    }
    if (kind == 1)
    {
        SmPoint3d a = rd.pt3();
        SmPoint3d b = rd.pt3();
        return new (ctx) SmLine(a, b, 3, &ctx);
    }
    SmPoint3d p1 = rd.pt3();
    SmVector3d d1 = rd.pt3();
    SmPoint3d p2 = rd.pt3();
    SmVector3d d2 = rd.pt3();
    return new (ctx) SmHermiteCurve(p1, d1, p2, d2, 3);
}

// A surface `kind ...`: 0 B-spline `n m p q r s cpts u v`; 1 plane `o x y sx sy u0 v0 u1 v1`
// (SmPlane(o, x, y, (sx, sy), domain, ctx)).
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 1)
    {
        SmPoint3d o = rd.pt3();
        SmVector3d x = rd.pt3();
        SmVector3d y = rd.pt3();
        double sx = rd.num();
        double sy = rd.num();
        double u0 = rd.num();
        double v0 = rd.num();
        double u1 = rd.num();
        double v1 = rd.num();
        return new (ctx) SmPlane(o, x, y, SmVector2d(sx, sy), SmExtent2d(u0, v0, u1, v1), &ctx);
    }
    gw_INDEX n = (gw_INDEX)rd.idx();
    gw_INDEX m = (gw_INDEX)rd.idx();
    gw_DEGREE p = (gw_DEGREE)rd.idx();
    gw_DEGREE q = (gw_DEGREE)rd.idx();
    gw_INDEX r = (gw_INDEX)rd.idx();
    gw_INDEX s = (gw_INDEX)rd.idx();
    gw_SURFACE* sur = sm_AllocateNurbSurface(n, m, p, q, r, s);
    for (gw_INDEX i = 0; i <= n; i++)
        for (gw_INDEX j = 0; j <= m; j++)
        {
            sur->net->Pw[i][j].x = rd.num();
            sur->net->Pw[i][j].y = rd.num();
            sur->net->Pw[i][j].z = rd.num();
            sur->net->Pw[i][j].w = rd.num();
        }
    for (gw_INDEX i = 0; i <= r; i++)
        sur->knu->U[i] = rd.num();
    for (gw_INDEX i = 0; i <= s; i++)
        sur->knv->U[i] = rd.num();
    return new (ctx) SmBSplineSurface(sur, FALSE, NULL);
}

// `has [n p m cpts knots]` of a B-spline curve's NURB.
void put_curve(std::vector<double>& o, const SmBSplineCurve* c)
{
    const gw_CURVE* cur = c ? c->GetGwNurbPointer() : NULL;
    o.push_back(flg(cur != NULL));
    if (!cur)
        return;
    gw_INDEX n = cur->pol->n;
    gw_INDEX m = cur->knt->m;
    o.push_back((double)n);
    o.push_back((double)cur->p);
    o.push_back((double)m);
    for (gw_INDEX i = 0; i <= n; i++)
    {
        o.push_back(cur->pol->Pw[i].x);
        o.push_back(cur->pol->Pw[i].y);
        o.push_back(cur->pol->Pw[i].z);
        o.push_back(cur->pol->Pw[i].w);
    }
    for (gw_INDEX i = 0; i <= m; i++)
        o.push_back(cur->knt->U[i]);
}

// `has [n m p q r s cpts u v]` of a B-spline surface's NURB.
void put_surface(std::vector<double>& o, const SmBSplineSurface* s)
{
    const gw_SURFACE* sur = s ? s->GetGwNurbPointer() : NULL;
    o.push_back(flg(sur != NULL));
    if (!sur)
        return;
    gw_INDEX n = sur->net->n;
    gw_INDEX m = sur->net->m;
    gw_INDEX r = sur->knu->m;
    gw_INDEX ss = sur->knv->m;
    o.push_back((double)n);
    o.push_back((double)m);
    o.push_back((double)sur->p);
    o.push_back((double)sur->q);
    o.push_back((double)r);
    o.push_back((double)ss);
    for (gw_INDEX i = 0; i <= n; i++)
        for (gw_INDEX j = 0; j <= m; j++)
        {
            o.push_back(sur->net->Pw[i][j].x);
            o.push_back(sur->net->Pw[i][j].y);
            o.push_back(sur->net->Pw[i][j].z);
            o.push_back(sur->net->Pw[i][j].w);
        }
    for (gw_INDEX i = 0; i <= r; i++)
        o.push_back(sur->knu->U[i]);
    for (gw_INDEX i = 0; i <= ss; i++)
        o.push_back(sur->knv->U[i]);
}

// The type queries of a solver: GetType, `len chars` of GetTypeString, IsKindOf of
// `nk` codes.
void put_types(std::vector<double>& o, const SmObject* obj, Rd& rd)
{
    o.push_back((double)obj->GetType());
    const TCHAR* s = obj->GetTypeString();
    std::vector<double> chars;
    for (const TCHAR* c = s; *c; c++)
        chars.push_back((double)*c);
    o.push_back((double)chars.size());
    o.insert(o.end(), chars.begin(), chars.end());
    int64_t nk = rd.idx();
    for (int64_t i = 0; i < nk; i++)
        o.push_back(flg(obj->IsKindOf((SM_TYPE)rd.idx()) != FALSE));
}

// ---- dry runs (see the file comment) ----------------------------------------------------

// The knot insertion of both CreateSurfaceFromCurves on copies: returns the rail 1 knots, or
// false when an insertion adds no knot.
bool dry_knots(SmBSplineCurve& r1, SmBSplineCurve& r2, SmTArray<double>& knots)
{
    if (knots.GetSize() == 2)
    {
        double dStep = (knots[1] - knots[0]) / 3.0;
        double dNew1 = knots[0] + dStep;
        double dNew2 = dNew1 + dStep;
        r1.InsertOneKnot(dNew1, 1);
        r2.InsertOneKnot(dNew1, 1);
        r1.InsertOneKnot(dNew2, 1);
        r2.InsertOneKnot(dNew2, 1);
        r1.GetKnots(knots);
    }
    ULONG old = knots.GetSize();
    while (knots.GetSize() < 4)
    {
        ULONG lMaxSpan = 0;
        double dMax = 0.0;
        for (ULONG k = 1; k < knots.GetSize(); k++)
        {
            double dSpan = knots[k] - knots[k - 1];
            if (dSpan > dMax)
            {
                lMaxSpan = k;
                dMax = dSpan;
            }
        }
        if (lMaxSpan == 0)
            return false;
        double dMid = (knots[lMaxSpan] + knots[lMaxSpan - 1]) / 2.0;
        r1.InsertOneKnot(dMid, 1);
        r2.InsertOneKnot(dMid, 1);
        r1.GetKnots(knots);
        if (knots.GetSize() == old)
            return false;
        old = knots.GetSize();
    }
    return true;
}

bool copy_curve(const SmBSplineCurve* c, const SmContext& ctx, SmBSplineCurve*& out)
{
    SmCurve* cp = NULL;
    if (c->Copy(ctx, cp) != SM_SUCCESS || cp == NULL)
        return false;
    out = (SmBSplineCurve*)cp;
    return true;
}

// Whether SmCircularCrossSectionFSG::CreateSurfaceFromCurves would run its second pass on a
// ruled or revolved surface.
bool circ_reads_past(const SmCircularCrossSectionFSG& fsg, bool approx, bool complement,
                     double tolpct, SmBSplineCurve* c0, SmBSplineCurve* r10, SmBSplineCurve* r20,
                     double tol, const SmContext& ctx)
{
    if (!approx)
        return false;
    (void)fsg;
    SmBSplineCurve *c = NULL, *r1 = NULL, *r2 = NULL;
    if (!copy_curve(c0, ctx, c) || !copy_curve(r10, ctx, r1) || !copy_curve(r20, ctx, r2))
    {
        delete c;
        delete r1;
        delete r2;
        return false;
    }
    bool bad = false;
    SmTArray<double> k1, k2;
    r1->GetKnots(k1);
    r2->GetKnots(k2);
    if (k2.GetSize() != k1.GetSize())
    {
        SmTArray<SmBSplineCurve*> rails;
        rails.Add(r1);
        rails.Add(r2);
        rails.Add(c);
        if (SmBSplineCurve::SyncronizeKnotsOfCurves(rails, SM_EFF_ZERO_SQRT) != SM_SUCCESS)
            goto done;
    }
    if (!dry_knots(*r1, *r2, k1))
        goto done;
    {
        double tolA = 1.0e-8;
        SmPoint3d lp, lp2;
        SmVector3d lv, lv2;
        bool sweep = false, revolve = false;
        if (c->GetDegree() == 1 && c->IsLine(11, tolA, lp, lv))
        {
            sweep = true;
            double a;
            if (r1->IsLine(11, tolA, lp2, lv2))
            {
                if (lv2.AngleBetween(lv, a) != SM_SUCCESS)
                    goto done;
                if (a > SM_EFF_ZERO_SQRT)
                    sweep = false;
            }
            if (r2->IsLine(11, tolA, lp2, lv2))
            {
                if (lv2.AngleBetween(lv, a) != SM_SUCCESS)
                    goto done;
                if (a > SM_EFF_ZERO_SQRT)
                    sweep = false;
            }
        }
        double rad, sa, ea;
        SmAxis2Placement frame;
        if (c->IsArc(11, tolA, frame, rad, sa, ea))
        {
            revolve = true;
            double rad2, sa2, ea2, rad3, sa3, ea3;
            SmAxis2Placement f2, f3;
            if (!r1->IsArc(11, tolA, f2, rad2, sa2, ea2))
                revolve = false;
            else if (smos_Fabs(ea2 - ea) * rad2 > tol)
                revolve = false;
            else if (!f2.AreCoaxial(frame, tol, rad2))
                revolve = false;
            if (revolve)
            {
                if (!r2->IsArc(11, tolA, f3, rad3, sa3, ea3))
                    revolve = false;
                else if (smos_Fabs(ea3 - ea) * rad3 > tol)
                    revolve = false;
                else if (!f3.AreCoaxial(frame, tol, rad3))
                    revolve = false;
                else if (smos_Fabs(ea3 - ea2) * rad3 > tol)
                    revolve = false;
                else if (!f3.AreCoaxial(f2, tol, rad3))
                    revolve = false;
            }
        }
        if (!sweep && !revolve)
            goto done;
        // the first pass's unique knot counts (ii = 1: jj 0, 1; then the last knot)
        ULONG lMax = 0, lMin = 1000000;
        ULONG n = k1.GetSize();
        double ts[3] = {k1[0], k1[1], k1[n - 1]};
        for (int it = 0; it < 3; it++)
        {
            double dT = ts[it];
            SmVector3d cpv[2];
            SmPoint3d p1, p2;
            if (c->Evaluate(dT, 1, TRUE, cpv) != SM_SUCCESS || r1->EvaluatePoint(dT, p1) != SM_SUCCESS ||
                r2->EvaluatePoint(dT, p2) != SM_SUCCESS)
                goto done;
            SmVector3d v1 = p1 - cpv[0];
            SmVector3d v2 = p2 - cpv[0];
            ULONG cnt;
            if (p1.DistanceBetween(p2) < tol)
            {
                SmBSplineCurve* pc = NULL;
                if (SmBSplineCurve::CreatePointCurve(ctx, p1, pc) != SM_SUCCESS || pc == NULL)
                    goto done;
                cnt = pc->GetNumberOfUniqueKnots();
                delete pc;
            }
            else
            {
                double d1 = v1.Length(), d2 = v2.Length();
                if (d1 < SM_EFF_ZERO || d2 < SM_EFF_ZERO)
                    goto done;
                if (smos_Fabs(d1 - d2) > d1 / 10.0)
                    continue;
                double ang;
                if (v1.Unitize() != SM_SUCCESS || v2.Unitize() != SM_SUCCESS)
                    goto done;
                SmVector3d z = v1 * v2;
                if (z.Unitize() != SM_SUCCESS || z.CCWAngleBetween(v1, v2, ang) != SM_SUCCESS)
                    goto done;
                if (smos_Fabs(ang) < SM_EFF_ZERO_SQRT || smos_Fabs(ang) > SM_PI - SM_EFF_ZERO_SQRT)
                    goto done;
                if (ang < 0.0)
                    z = -z;
                if (z.CCWAngleBetween(v1, v2, ang) != SM_SUCCESS || ang < SM_EFF_ZERO)
                    goto done;
                SmVector3d y = z * v1;
                SmVector3d x = y * z;
                if (complement)
                {
                    y = -y;
                    ang = 2.0 * SM_PI - ang;
                }
                if (x.Unitize() != SM_SUCCESS || y.Unitize() != SM_SUCCESS)
                    goto done;
                SmAxis2Placement pl;
                if (pl.SetCanonical(cpv[0], x, y) != SM_SUCCESS)
                    goto done;
                SmBSplineCurve* arc = NULL;
                if (SmBSplineCurve::ApproximateArc(ctx, 3, NULL, pl, d1, 0.0, ang * 180.0 / SM_PI, tolpct,
                                                   arc) != SM_SUCCESS ||
                    arc == NULL)
                    goto done;
                cnt = arc->GetNumberOfUniqueKnots();
                delete arc;
            }
            lMax = cnt > lMax ? cnt : lMax;
            lMin = cnt < lMin ? cnt : lMin;
        }
        bad = lMax != lMin;
    }
done:
    delete c;
    delete r1;
    delete r2;
    return bad;
}

// Whether SmBlendCurveCrossSectionFSG::CreateSurfaceFromCurves would loop forever or skin a
// point cross section.
bool blend_reads_past(SmBSplineCurve* c0, SmBSplineCurve* r10, SmBSplineCurve* r20, double tol, bool g1,
                      const SmContext& ctx)
{
    SmBSplineCurve *c = NULL, *r1 = NULL, *r2 = NULL;
    if (!copy_curve(c0, ctx, c) || !copy_curve(r10, ctx, r1) || !copy_curve(r20, ctx, r2))
    {
        delete c;
        delete r1;
        delete r2;
        return false;
    }
    bool bad = false;
    SmTArray<double> k1, k2;
    r1->GetKnots(k1);
    r2->GetKnots(k2);
    if (k2.GetSize() == k1.GetSize())
    {
        if (!dry_knots(*r1, *r2, k1))
            bad = true;
        else
        {
            bool point = false;
            for (ULONG i = 0; i < k1.GetSize(); i++)
            {
                SmPoint3d p1, p2;
                if (r1->EvaluatePoint(k1[i], p1) == SM_SUCCESS && r2->EvaluatePoint(k1[i], p2) == SM_SUCCESS &&
                    p1.DistanceBetween(p2) < tol)
                    point = true;
            }
            if (point || g1)
            {
                double tolA = 1.0e-8;
                SmPoint3d lp, lp2;
                SmVector3d lv, lv2;
                bool line = c->IsLine(11, tolA, lp, lv) && c->GetDegree() == 1;
                bool sweep = false;
                if (line)
                {
                    sweep = true;
                    double a;
                    if (r1->IsLine(11, tolA, lp2, lv2) && (lv2.AngleBetween(lv, a) != SM_SUCCESS || a > SM_EFF_ZERO_SQRT))
                        sweep = false;
                    if (r2->IsLine(11, tolA, lp2, lv2) && (lv2.AngleBetween(lv, a) != SM_SUCCESS || a > SM_EFF_ZERO_SQRT))
                        sweep = false;
                }
                bool revolve = false;
                if (!sweep)
                {
                    double rad, sa, ea;
                    SmAxis2Placement f;
                    revolve = c->IsArc(11, tolA, f, rad, sa, ea) && r1->IsArc(11, tolA, f, rad, sa, ea) &&
                              r2->IsArc(11, tolA, f, rad, sa, ea);
                }
                bad = !sweep && !revolve;
            }
        }
    }
    delete c;
    delete r1;
    delete r2;
    return bad;
}

// ---- generators ---------------------------------------------------------------------------

// A solver for the generators' context: an SmConstantRadiusFS over two bilinear surfaces.
struct CtxSolver
{
    SmBSplineSurface* sa = NULL;
    SmBSplineSurface* sb = NULL;
    SmConstantRadiusFS* fs = NULL;
    ~CtxSolver()
    {
        delete fs;
        delete sa;
        delete sb;
    }
    bool build(const SmContext& ctx, double tol)
    {
        SmBSplineSurface::CreateBilinearSurface(ctx, SmPoint3d(0, 0, 0), SmPoint3d(1, 0, 0),
                                                SmPoint3d(0, 1, 0), SmPoint3d(1, 1, 0), sa);
        SmBSplineSurface::CreateBilinearSurface(ctx, SmPoint3d(0, 0, 0), SmPoint3d(0, 0, 1),
                                                SmPoint3d(1, 0, 0), SmPoint3d(1, 0, 1), sb);
        if (sa == NULL || sb == NULL)
            return false;
        fs = new (ctx) SmConstantRadiusFS(ctx, tol, 30.0 * SM_PI / 180.0, 2.0 * SM_PI / 180.0, 1.0,
                                          1.0, *sa, *sb, TRUE, TRUE);
        return true;
    }
};

// `approx tolpct mirror complement tol g1 center rail1 rail2`.
int32_t circ(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    bool approx = rd.flag();
    double tolpct = rd.num();
    bool mirror = rd.flag();
    bool complement = rd.flag();
    double tol = rd.num();
    bool g1 = rd.flag();
    SmBSplineCurve* c = read_bcurve(rd, ctx);
    SmBSplineCurve* r1 = read_bcurve(rd, ctx);
    SmBSplineCurve* r2 = read_bcurve(rd, ctx);
    SmCircularCrossSectionFSG fsg(approx ? TRUE : FALSE, tolpct, mirror ? TRUE : FALSE,
                                  complement ? TRUE : FALSE);
    if (circ_reads_past(fsg, approx, complement, tolpct, c, r1, r2, tol, ctx))
    {
        o.push_back(REJECTED);
    }
    else
    {
        SmBSplineSurface* res = NULL;
        SmStatus s = fsg.CreateSurfaceFromCurves(ctx, *c, *r1, *r2, tol, g1 ? TRUE : FALSE, NULL, NULL,
                                                 NULL, NULL, res);
        o.push_back(st(s));
        put_curve(o, c);
        put_curve(o, r1);
        put_curve(o, r2);
        if (s == SM_SUCCESS)
            put_surface(o, res);
        delete res;
    }
    delete c;
    delete r1;
    delete r2;
    return SC_OK;
}

// `cont scale tol g1 center rail1 rail2 has_sides [surf1 uv1 surf2 uv2]`.
int32_t blend(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    ULONG cont = (ULONG)rd.idx();
    double scale = rd.num();
    double tol = rd.num();
    bool g1 = rd.flag();
    SmBSplineCurve* c = read_bcurve(rd, ctx);
    SmBSplineCurve* r1 = read_bcurve(rd, ctx);
    SmBSplineCurve* r2 = read_bcurve(rd, ctx);
    bool sides = rd.flag();
    SmSurface* s1 = NULL;
    SmSurface* s2 = NULL;
    SmBSplineCurve* uv1 = NULL;
    SmBSplineCurve* uv2 = NULL;
    if (sides)
    {
        s1 = read_surface(rd, ctx);
        uv1 = read_bcurve(rd, ctx);
        s2 = read_surface(rd, ctx);
        uv2 = read_bcurve(rd, ctx);
    }
    SmBlendCurveCrossSectionFSG fsg(scale, cont);
    if (blend_reads_past(c, r1, r2, tol, g1, ctx))
    {
        o.push_back(REJECTED);
    }
    else
    {
        SmBSplineSurface* res = NULL;
        SmStatus s =
            fsg.CreateSurfaceFromCurves(ctx, *c, *r1, *r2, tol, g1 ? TRUE : FALSE, s1, uv1, s2, uv2, res);
        o.push_back(st(s));
        put_curve(o, c);
        put_curve(o, r1);
        put_curve(o, r2);
        if (s == SM_SUCCESS)
            put_surface(o, res);
        delete res;
    }
    delete c;
    delete r1;
    delete r2;
    delete s1;
    delete s2;
    delete uv1;
    delete uv2;
    return SC_OK;
}

// `rail1 rail2`.
int32_t linear(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmBSplineCurve* r1 = read_bcurve(rd, ctx);
    SmBSplineCurve* r2 = read_bcurve(rd, ctx);
    CtxSolver cs;
    if (!cs.build(ctx, 1.0e-6))
    {
        delete r1;
        delete r2;
        return SC_ERR_ARG;
    }
    SmLinearCrossSectionFSG fsg;
    SmTArray<SmTsectPnt*> pts;
    SmBSplineSurface* res = NULL;
    SmStatus s = fsg.CreateSurface(*cs.fs, NULL, r1, r2, pts, NULL, NULL, NULL, NULL, res);
    o.push_back(st(s));
    if (s == SM_SUCCESS)
        put_surface(o, res);
    delete res;
    delete r1;
    delete r2;
    return SC_OK;
}

// ---- SmCurveBasedFS -----------------------------------------------------------------------

struct CurveBased : SmCurveBasedFS
{
    CurveBased(const SmContext& ctx) : SmCurveBasedFS(ctx, 1.0e-6, 30.0 * SM_PI / 180.0, 2.0 * SM_PI / 180.0)
    {
    }
    SmStatus eval(double t, ULONG n, SmVector3d* out) { return EvaluateBaseCurve(t, n, out); }
    const SmBSplineCurve* extended() const { return m_pExtended; }
};

// `curve ncalls` then `ncalls` x `t nderiv`, `nk kinds[nk]`.
int32_t base(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmCurve* c = read_curve(rd, ctx);
    CurveBased* fs = new (ctx) CurveBased(ctx);
    fs->SetOriginalCurve(c);
    int64_t calls = rd.idx();
    o.push_back(0.0);
    for (int64_t i = 0; i < calls; i++)
    {
        double t = rd.num();
        ULONG n = (ULONG)rd.idx();
        std::vector<SmVector3d> pv(n + 1);
        SmStatus s = fs->eval(t, n, pv.data());
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            for (ULONG j = 0; j <= n; j++)
                put3(o, pv[j]);
    }
    put_curve(o, fs->extended());
    o.push_back((double)fs->GetSolverType());
    put_types(o, fs, rd);
    fs->SetOriginalCurve(NULL);
    delete fs;
    delete c;
    return SC_OK;
}

// ---- SmSurfaceSurfaceFS -------------------------------------------------------------------

// `ext surface u0 v0 u1 v1 nk kinds[nk]`.
int32_t ss_ext(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    double ext = rd.num();
    SmSurface* s = read_surface(rd, ctx);
    double u0 = rd.num();
    double v0 = rd.num();
    double u1 = rd.num();
    double v1 = rd.num();
    SmBSplineSurface* sa = NULL;
    SmBSplineSurface* sb = NULL;
    SmBSplineSurface::CreateBilinearSurface(ctx, SmPoint3d(0, 0, 0), SmPoint3d(1, 0, 0),
                                            SmPoint3d(0, 1, 0), SmPoint3d(1, 1, 0), sa);
    SmBSplineSurface::CreateBilinearSurface(ctx, SmPoint3d(0, 0, 0), SmPoint3d(0, 0, 1),
                                            SmPoint3d(1, 0, 0), SmPoint3d(1, 0, 1), sb);
    if (sa == NULL || sb == NULL)
    {
        delete sa;
        delete sb;
        delete s;
        return SC_ERR_ARG;
    }
    SmSurfaceSurfaceFS* fs = new (ctx) SmSurfaceSurfaceFS(
        ctx, 1.0e-6, 30.0 * SM_PI / 180.0, 2.0 * SM_PI / 180.0, 1.0, 1.0, *sa, *sb, TRUE, TRUE,
        SmPoint2d(0.5, 0.5), SmPoint2d(0.5, 0.5));
    fs->SetExtensionFactor(ext);
    SmExtent2d d(u0, v0, u1, v1);
    fs->SetupOffsetExtension(s, d);
    o.push_back(0.0);
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
    o.push_back((double)fs->GetSolverType());
    put_types(o, fs, rd);
    delete fs;
    delete sa;
    delete sb;
    delete s;
    return SC_OK;
}

// ---- SmConstantRadiusFS -------------------------------------------------------------------

struct CrBuilt
{
    SmSurface* a = NULL;
    SmSurface* b = NULL;
    SmConstantRadiusFS* fs = NULL;
    SmSurface* copy = NULL;
    SmOffsetSurface* extra_a = NULL;
    SmOffsetSurface* extra_copy = NULL;
    ~CrBuilt()
    {
        delete extra_a;
        delete extra_copy;
        delete fs;
        delete copy;
        delete a;
        if (b != a)
            delete b;
    }
    // `same surfaceA [surfaceB] r0 r1 o0 o1`.
    bool build(Rd& rd, const SmContext& ctx)
    {
        bool same = rd.flag();
        a = read_surface(rd, ctx);
        b = same ? a : read_surface(rd, ctx);
        double r0 = rd.num();
        double r1 = rd.num();
        bool o0 = rd.flag();
        bool o1 = rd.flag();
        fs = new (ctx) SmConstantRadiusFS(ctx, 1.0e-6, 30.0 * SM_PI / 180.0, 2.0 * SM_PI / 180.0, r0, r1,
                                          *a, *b, o0 ? TRUE : FALSE, o1 ? TRUE : FALSE);
        if (a->Copy(ctx, copy) != SM_SUCCESS || copy == NULL)
            return false;
        extra_a = new (ctx) SmOffsetSurface(r0, *a);
        extra_copy = new (ctx) SmOffsetSurface(r0, *copy);
        return true;
    }
    // Selector 0 NULL, 1 GetSurface(0), 2 GetSurface(1), 3 offset of base A, 4 offset of a copy.
    SmSurface* select(int64_t sel)
    {
        switch (sel)
        {
        case 1: return fs->GetSurface(0);
        case 2: return fs->GetSurface(1);
        case 3: return extra_a;
        case 4: return extra_copy;
        default: return NULL;
        }
    }
};

// `solver set_offsets d0 d1 do_setup user nk kinds[nk]`.
int32_t cr(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    CrBuilt b;
    if (!b.build(rd, ctx))
        return SC_ERR_ARG;
    SmConstantRadiusFS* fs = b.fs;
    bool set = rd.flag();
    double d0 = rd.num();
    double d1 = rd.num();
    bool setup = rd.flag();
    double user = rd.num();
    if (set)
    {
        fs->GetSurface(0)->SetOffsetDistance(d0);
        fs->GetSurface(1)->SetOffsetDistance(d1);
    }
    double dist = -999.0;
    SmStatus s = SM_SUCCESS;
    if (setup)
    {
        SmTsectPnt tp;
        tp.m_adUserDoubles[0] = user;
        s = fs->SetupOffsetValues(tp, &dist);
    }
    o.push_back(st(s));
    o.push_back(dist);
    o.push_back(fs->GetFilletRadius(0));
    o.push_back(fs->GetFilletRadius(1));
    o.push_back(fs->GetOrientation(0));
    o.push_back(fs->GetOrientation(1));
    o.push_back(fs->GetSurface(0)->GetOffsetDistance());
    o.push_back(fs->GetSurface(1)->GetOffsetDistance());
    o.push_back(fs->GetConversionTol());
    o.push_back(fs->GetExtensionFactor());
    o.push_back((double)fs->GetJacobianSize());
    o.push_back(flg(fs->OffsetRadiiCanChange() != FALSE));
    o.push_back((double)fs->GetSolverType());
    put_types(o, fs, rd);
    return SC_OK;
}

// `solver rail ne np sel1 off1 sel2 off2 nx x[nx] nf f[nf] has_j rows cols`.
int32_t cr_jac(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    CrBuilt b;
    if (!b.build(rd, ctx))
        return SC_ERR_ARG;
    ULONG rail = (ULONG)rd.idx();
    ULONG ne = (ULONG)rd.idx();
    ULONG np = (ULONG)rd.idx();
    SmSurface* s1 = b.select(rd.idx());
    ULONG off1 = (ULONG)rd.idx();
    SmSurface* s2 = b.select(rd.idx());
    ULONG off2 = (ULONG)rd.idx();
    int64_t nx = rd.idx();
    SmTArray<double> x;
    for (int64_t i = 0; i < nx; i++)
        x.Add(rd.num());
    int64_t nf = rd.idx();
    SmTArray<double> f;
    for (int64_t i = 0; i < nf; i++)
        f.Add(rd.num());
    bool has_j = rd.flag();
    ULONG rows = (ULONG)rd.idx();
    ULONG cols = (ULONG)rd.idx();
    SmMatrix j(has_j ? rows : 1, has_j ? cols : 1);
    if (has_j)
        for (ULONG r = 0; r < rows; r++)
            for (ULONG c = 0; c < cols; c++)
                j[r][c] = 0.0;
    SmBoolean found = TRUE;
    SmStatus stt = b.fs->LoadJacobian(ne, np, rail, s1, off1, s2, off2, x, f, has_j ? &j : NULL, found);
    o.push_back(st(stt));
    o.push_back((double)ne);
    o.push_back((double)np);
    o.push_back((double)off1);
    o.push_back((double)off2);
    o.push_back(flg(found != FALSE));
    for (ULONG i = 0; i < f.GetSize(); i++)
        o.push_back(f[i]);
    if (has_j)
        for (ULONG r = 0; r < rows; r++)
            for (ULONG c = 0; c < cols; c++)
                o.push_back(j[r][c]);
    o.push_back(b.fs->GetSurface(0)->GetOffsetDistance());
    o.push_back(b.fs->GetSurface(1)->GetOffsetDistance());
    return SC_OK;
}

// ---- SmConstantDistanceFS -----------------------------------------------------------------

struct Cd : SmConstantDistanceFS
{
    Cd(const SmContext& ctx, double dist, SmEdgeuse* eu)
        : SmConstantDistanceFS(ctx, 1.0e-6, 30.0 * SM_PI / 180.0, 2.0 * SM_PI / 180.0, dist, eu)
    {
    }
    void setup(SmOffsetSurface* s0, SmOffsetSurface* s1, double o0, double o1, double dist, double ext)
    {
        delete this->m_pSurfaces[0];
        delete this->m_pSurfaces[1];
        this->m_pSurfaces[0] = s0;
        this->m_pSurfaces[1] = s1;
        this->m_dOrientations[0] = o0;
        this->m_dOrientations[1] = o1;
        this->m_dDistance = dist;
        this->m_pEdgeuses[0] = NULL;
        this->m_pEdgeuses[1] = NULL;
        this->m_dSurfaceExtensionFactor = ext;
    }
};

// The solver block `same surfaceA [surfaceB] d0 d1 o0 o1 dist ext`: rail i offsets its base by
// d_i with orientation o_i.
struct CdBuilt
{
    SmBrep* brep = NULL;
    SmSurface* a = NULL;
    SmSurface* b = NULL;
    SmSurface* copy = NULL;
    Cd* fs = NULL;
    SmOffsetSurface* extra_a = NULL;
    SmOffsetSurface* extra_copy = NULL;
    ~CdBuilt()
    {
        delete extra_a;
        delete extra_copy;
        delete fs;
        delete copy;
        delete a;
        if (b != a)
            delete b;
        delete brep;
    }
    bool build(Rd& rd, const SmContext& ctx)
    {
        bool same = rd.flag();
        a = read_surface(rd, ctx);
        b = same ? a : read_surface(rd, ctx);
        double d0 = rd.num();
        double d1 = rd.num();
        double o0 = rd.num();
        double o1 = rd.num();
        double dist = rd.num();
        double ext = rd.num();
        if (SmApiCreateBox(SmVector3d(0.0, 0.0, 0.0), 1.0, 1.0, 1.0, brep) != SM_SUCCESS || brep == NULL)
            return false;
        SmTArray<SmEdge*> edges;
        brep->GetEdges(edges);
        if (edges.GetSize() == 0)
            return false;
        SmEdgeuse* eu = edges[0]->GetPrimaryEdgeuse();
        if (eu == NULL)
            return false;
        fs = new (ctx) Cd(ctx, dist, eu);
        SmOffsetSurface* s0 = new (ctx) SmOffsetSurface(d0, *a);
        SmOffsetSurface* s1 = new (ctx) SmOffsetSurface(d1, *b);
        s0->SetAllowExtension(TRUE);
        s1->SetAllowExtension(TRUE);
        fs->setup(s0, s1, o0, o1, dist, ext);
        if (a->Copy(ctx, copy) != SM_SUCCESS || copy == NULL)
            return false;
        extra_a = new (ctx) SmOffsetSurface(d0, *a);
        extra_copy = new (ctx) SmOffsetSurface(d0, *copy);
        return true;
    }
    SmSurface* select(int64_t sel)
    {
        switch (sel)
        {
        case 1: return fs->GetSurface(0);
        case 2: return fs->GetSurface(1);
        case 3: return extra_a;
        case 4: return extra_copy;
        default: return NULL;
        }
    }
};

// `solver user nk kinds[nk]`.
int32_t cd_setup(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    CdBuilt b;
    if (!b.build(rd, ctx))
        return SC_ERR_ARG;
    double user = rd.num();
    SmTsectPnt tp;
    tp.m_adUserDoubles[0] = user;
    double dist = -999.0;
    SmStatus s = b.fs->SetupOffsetValues(tp, &dist);
    o.push_back(st(s));
    o.push_back(dist);
    o.push_back(b.fs->GetSurface(0)->GetOffsetDistance());
    o.push_back(b.fs->GetSurface(1)->GetOffsetDistance());
    o.push_back(b.fs->GetDistance());
    o.push_back(flg(b.fs->CanMakeAnalytic() != FALSE));
    o.push_back((double)b.fs->GetJacobianSize());
    o.push_back((double)b.fs->GetSolverType());
    put_types(o, b.fs, rd);
    return SC_OK;
}

// `solver rail ne np sel1 off1 sel2 off2 nx x[nx] nf f[nf] has_j rows cols`.
int32_t cd_jac(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    CdBuilt b;
    if (!b.build(rd, ctx))
        return SC_ERR_ARG;
    ULONG rail = (ULONG)rd.idx();
    ULONG ne = (ULONG)rd.idx();
    ULONG np = (ULONG)rd.idx();
    SmSurface* s1 = b.select(rd.idx());
    ULONG off1 = (ULONG)rd.idx();
    SmSurface* s2 = b.select(rd.idx());
    ULONG off2 = (ULONG)rd.idx();
    int64_t nx = rd.idx();
    SmTArray<double> x;
    for (int64_t i = 0; i < nx; i++)
        x.Add(rd.num());
    int64_t nf = rd.idx();
    SmTArray<double> f;
    for (int64_t i = 0; i < nf; i++)
        f.Add(rd.num());
    bool has_j = rd.flag();
    ULONG rows = (ULONG)rd.idx();
    ULONG cols = (ULONG)rd.idx();
    SmMatrix j(has_j ? rows : 1, has_j ? cols : 1);
    if (has_j)
        for (ULONG r = 0; r < rows; r++)
            for (ULONG c = 0; c < cols; c++)
                j[r][c] = 0.0;
    SmBoolean found = TRUE;
    SmStatus stt = b.fs->LoadJacobian(ne, np, rail, s1, off1, s2, off2, x, f, has_j ? &j : NULL, found);
    o.push_back(st(stt));
    o.push_back((double)ne);
    o.push_back((double)np);
    o.push_back(flg(found != FALSE));
    for (ULONG i = 0; i < f.GetSize(); i++)
        o.push_back(f[i]);
    if (has_j)
        for (ULONG r = 0; r < rows; r++)
            for (ULONG c = 0; c < cols; c++)
                o.push_back(j[r][c]);
    o.push_back(b.fs->GetSurface(0)->GetOffsetDistance());
    o.push_back(b.fs->GetSurface(1)->GetOffsetDistance());
    return SC_OK;
}

// `solver do2 rail sel1 s1_index s2_index dim` `dim` x `min max`, `np per[np] ng guess[ng]`.
int32_t cd_init(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    CdBuilt b;
    if (!b.build(rd, ctx))
        return SC_ERR_ARG;
    ULONG do2 = rd.flag() ? 1 : 0;
    ULONG rail = (ULONG)rd.idx();
    SmSurface* s1 = b.select(rd.idx());
    ULONG s1_index = (ULONG)rd.idx();
    ULONG s2_index = (ULONG)rd.idx();
    ULONG dim = (ULONG)rd.idx();
    SmExtentNd ivls(dim);
    for (ULONG i = 0; i < dim; i++)
    {
        double lo = rd.num();
        double hi = rd.num();
        ivls[i] = SmExtent1d(lo, hi);
    }
    int64_t np = rd.idx();
    SmTArray<SmBoolean> per;
    for (int64_t i = 0; i < np; i++)
        per.Add(rd.flag() ? TRUE : FALSE);
    int64_t ng = rd.idx();
    SmTArray<double> g;
    for (int64_t i = 0; i < ng; i++)
        g.Add(rd.num());
    if (s1 == NULL)
        return SC_ERR_ARG;
    SmSurface* s2 = NULL;
    SmStatus stt = b.fs->LoadInitialValues(do2, rail, *s1, s1_index, s2, s2_index, ivls, per, g);
    o.push_back(st(stt));
    double s2_rail = -1.0;
    if (s2 != NULL)
        s2_rail = s2 == b.fs->GetSurface(0) ? 0.0 : s2 == b.fs->GetSurface(1) ? 1.0 : -2.0;
    o.push_back(s2_rail);
    o.push_back((double)s2_index);
    for (ULONG i = 0; i < dim; i++)
    {
        o.push_back(ivls[i].GetMin());
        o.push_back(ivls[i].GetMax());
    }
    o.push_back((double)per.GetSize());
    for (ULONG i = 0; i < per.GetSize(); i++)
        o.push_back(flg(per[i] != FALSE));
    o.push_back((double)g.GetSize());
    for (ULONG i = 0; i < g.GetSize(); i++)
        o.push_back(g[i]);
    o.push_back(b.fs->GetSurface(0)->GetOffsetDistance());
    o.push_back(b.fs->GetSurface(1)->GetOffsetDistance());
    return SC_OK;
}

// ---- SmVariableRadiusFS -------------------------------------------------------------------

// The block `surfaceA surfaceB o0 o1 center law_kind [r0 r1 | lawcurve e0 e1] orient fsg`
// builds SmVariableRadiusFS(ctx, tol, ang, tang, A, B, o0, o1, center, law, orient) and sets a
// fillet surface generator of type `fsg` (1 linear, 2 circular, 3 blend curve).
struct VrBuilt
{
    SmSurface* a = NULL;
    SmSurface* b = NULL;
    SmCurve* center = NULL;
    SmFilletLaw* law = NULL;
    SmFilletSurfaceGenerator* fsg = NULL;
    SmVariableRadiusFS* fs = NULL;
    ~VrBuilt()
    {
        delete fs;
        delete fsg;
        delete law;
        delete center;
        delete a;
        delete b;
    }
    bool build(Rd& rd, const SmContext& ctx)
    {
        a = read_surface(rd, ctx);
        b = read_surface(rd, ctx);
        double o0 = rd.num();
        double o1 = rd.num();
        center = read_curve(rd, ctx);
        int64_t kind = rd.idx();
        if (kind == 0)
        {
            double r0 = rd.num();
            double r1 = rd.num();
            law = new (ctx) SmLinearFilletLaw(r0, r1);
        }
        else
        {
            SmBSplineCurve* lc = read_bcurve(rd, ctx);
            double e0 = rd.num();
            double e1 = rd.num();
            law = new (ctx) SmBSplineFilletLaw(lc, SmExtent1d(e0, e1));
        }
        bool orient = rd.flag();
        int64_t fsg_kind = rd.idx();
        if (fsg_kind == 1)
            fsg = new (ctx) SmLinearCrossSectionFSG();
        else if (fsg_kind == 2)
            fsg = new (ctx) SmCircularCrossSectionFSG();
        else
            fsg = new (ctx) SmBlendCurveCrossSectionFSG();
        fs = new (ctx) SmVariableRadiusFS(ctx, 1.0e-6, 30.0 * SM_PI / 180.0, 2.0 * SM_PI / 180.0, a, b, o0,
                                          o1, center, *law, orient ? TRUE : FALSE);
        fs->SetFilletSurfaceGenerator(fsg);
        return true;
    }
};

// `solver set_ivl e0 e1 nk kinds[nk]`.
int32_t vr(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    VrBuilt b;
    if (!b.build(rd, ctx))
        return SC_ERR_ARG;
    bool set = rd.flag();
    double e0 = rd.num();
    double e1 = rd.num();
    if (set)
        b.fs->SetFilletLawInterval(SmExtent1d(e0, e1));
    o.push_back(0.0);
    o.push_back(flg(b.fs->OffsetRadiiCanChange() != FALSE));
    o.push_back(flg(b.fs->GetFilletLawOrientation() != FALSE));
    SmExtent1d ivl = b.fs->GetFilletLawInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    o.push_back(flg(b.fs->GetFilletLaw() == b.law));
    o.push_back((double)b.fs->GetJacobianSize());
    o.push_back((double)b.fs->GetSolverType());
    put_types(o, b.fs, rd);
    return SC_OK;
}

// `solver user`.
int32_t vr_setup(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    VrBuilt b;
    if (!b.build(rd, ctx))
        return SC_ERR_ARG;
    double user = rd.num();
    SmTsectPnt tp;
    tp.m_adUserDoubles[0] = user;
    double dist = -999.0;
    SmStatus s = b.fs->SetupOffsetValues(tp, &dist);
    o.push_back(st(s));
    o.push_back(dist);
    o.push_back(b.fs->GetSurface(0)->GetOffsetDistance());
    o.push_back(b.fs->GetSurface(1)->GetOffsetDistance());
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_standard_solver_p1_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                                          int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    int32_t rc = SC_OK;
    switch (op)
    {
    case OP_CIRC: rc = circ(rd, ctx, o); break;
    case OP_BLEND: rc = blend(rd, ctx, o); break;
    case OP_LINEAR: rc = linear(rd, ctx, o); break;
    case OP_BASE: rc = base(rd, ctx, o); break;
    case OP_SS_EXT: rc = ss_ext(rd, ctx, o); break;
    case OP_CR: rc = cr(rd, ctx, o); break;
    case OP_CR_JAC: rc = cr_jac(rd, ctx, o); break;
    case OP_CD_SETUP: rc = cd_setup(rd, ctx, o); break;
    case OP_CD_JAC: rc = cd_jac(rd, ctx, o); break;
    case OP_CD_INIT: rc = cd_init(rd, ctx, o); break;
    case OP_VR: rc = vr(rd, ctx, o); break;
    case OP_VR_SETUP: rc = vr_setup(rd, ctx, o); break;
    default: return SC_ERR_ARG;
    }
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
