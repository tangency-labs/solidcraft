// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.trimming_tools`: the SmTrimmingTools statics and the
// SmCurveLoops / SmIsoXSectSegment / SmCurveIntersections helpers owned by the unit
// (source/SMLib/src/SmTrimmingTools.cpp, source/SMLib/inc/SmTrimmingTools.h), plus a
// verbatim copy of the file-static sm_IsLoopUVGapWithinTolerance.
//
// One entry point, sc_topology_trimming_tools_eval(op, path, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/topology_trimming_tools.rs,
// which validates every input. Geometry ops (codes below 20) read curve and surface
// blocks; B-rep ops (20 and up) start with the B-rep block of unit topology.vertexuse
// (built by the shared sc_topology_fixture.h), whose graph that unit's Export op exports.
// The shim gives every face and edge of the B-rep the plain B-spline copy of an analytic
// surface or curve (what the Rust import rebuilds) before the op, as the heal.gap shim.
//
// Output slots: `-2` "upstream would dereference null or read out of bounds" (the shim
// did not call it), `-3` "not run" (the Rust side defers this path to an unported unit).
#include "sc_topology_fixture.h"

#include <SmAssertArray.h>
#include <SmCurveBoundedSurface.h>
#include <SmEllipse.h>
#include <SmTrimmingTools.h>

#include <memory>

// Reuses the existing p10 read-only observer, not its operations or input model.
void sc_topology_brep_p10_append_snapshot(SmBrep*, const SmTArray<SmFace*>*, std::vector<double>&);

namespace
{
enum : int32_t
{
    OP_ORDER = 1,
    OP_OUTER = 2,
    OP_MIRROR = 3,
    OP_ISO_AT = 4,
    OP_ISO = 5,
    OP_ISO_SEAM = 6,
    OP_CONTINUITY = 7,
    OP_UV_GAP = 8,
    OP_MIN = 9,
    OP_LOOPS = 10,
    OP_TRIM_NULL = 11,
    OP_TRIM_CONSTRUCTED = 12,

    OP_B_PRESENT = 20,
    OP_B_EXTREMES = 21,
    OP_B_CHECK_LOOP = 22,
    OP_B_CHECK_FACE = 23,
    OP_B_TINY = 24,
    OP_B_ENSURE = 25,
    OP_B_BACKWARDS = 26,
    OP_B_ORIENT = 27,
    OP_B_ORDER = 28,
    OP_B_DEFERRED = 29,
};

const double TT_NULL_DEREF = -2.0;
const double TT_NOT_RUN = -3.0;

// Two fixed, valid ORIGINAL-trim controls. No caller-supplied geometry, dimensions
// or flags are admitted; the original surface is never read after the consuming call.
int32_t tt_trim_constructed(int kind, std::vector<double>& output)
{
    SmContext context;
    std::unique_ptr<SmBrep> body(new (context) SmBrep());
    std::unique_ptr<SmSurface> original;
    if (kind == 0)
        original.reset(new (context) SmPlane(SmPoint3d(0, 0, 0), SmVector3d(0, 0, 1), &context));
    else
    {
        gw_SURFACE* nurb = sm_AllocateNurbSurface(2, 1, 1, 1, 4, 3);
        const double uk[5] = {0, 0, 0.5, 1, 1};
        const double vk[4] = {0, 0, 1, 1};
        for (int i = 0; i < 3; ++i)
            for (int j = 0; j < 2; ++j)
            {
                nurb->net->Pw[i][j].x = 0.5 * i;
                nurb->net->Pw[i][j].y = j;
                nurb->net->Pw[i][j].z = i == 1 ? 0.25 : 0;
                nurb->net->Pw[i][j].w = 1;
            }
        for (int i = 0; i < 5; ++i)
            nurb->knu->U[i] = uk[i];
        for (int i = 0; i < 4; ++i)
            nurb->knv->U[i] = vk[i];
        original.reset(new (context) SmBSplineSurface(nurb, FALSE, &context));
    }
    SmBoolean g1 = FALSE;
    SmStatus continuity = SmTrimmingTools::SurfaceContinuityCheck(original.get(), g1);
    if (continuity != SM_SUCCESS)
        return SC_ERR_ARG; // construction/continuity failed before the operation
    const SmPoint2d plane_uv[4] = {SmPoint2d(0, 0), SmPoint2d(1, 0), SmPoint2d(1, 1), SmPoint2d(0, 1)};
    const SmPoint2d creased_uv[6] = {SmPoint2d(0, 0), SmPoint2d(0.5, 0), SmPoint2d(1, 0),
                                    SmPoint2d(1, 1), SmPoint2d(0.5, 1), SmPoint2d(0, 1)};
    const int count = kind == 0 ? 4 : 6;
    SmPoint3d points[6];
    for (int i = 0; i < count; ++i)
        if (original->EvaluatePoint(kind == 0 ? plane_uv[i] : creased_uv[i], points[i]) != SM_SUCCESS)
            return SC_ERR_ARG;
    std::vector<std::unique_ptr<SmCurve>> owned_curves;
    SmTArray<SmCurve*> curves;
    SmTArray<SmOrientType> orientations;
    for (int i = 0; i < count; ++i)
    {
        SmLine* line = nullptr;
        SmExtent1d interval(0, 1);
        if (SmLine::CreateLineSegment(context, 3, points[i], points[(i + 1) % count], line, &interval) != SM_SUCCESS)
            return SC_ERR_ARG;
        owned_curves.emplace_back(line);
        curves.Add(line);
        orientations.Add(SM_OT_SAME);
    }
    SmTArray<ULONG> counts;
    counts.Add((ULONG)count);
    SmTArray<SmPoint3d> loop_points;
    SmTArray<SmFace*> returned;
    for (auto& curve : owned_curves)
        curve.release();
    SmSurface* consumed = original.release();
    SmStatus status = SmTrimmingTools::TrimSurfaceWithModelSpaceCurves(
        context, body->GetInfiniteRegion(), 1e-5, counts, curves, orientations, loop_points, consumed,
        SmExtent2d(SmPoint2d(0, 0), SmPoint2d(1, 1)), SM_OT_SAME, TRUE, kind == 0 ? FALSE : TRUE, FALSE, &returned);
    // Neither consumed nor curves are inspected after the call, on any status.
    output.push_back(st(continuity));
    output.push_back(flg(g1));
    output.push_back(st(status));
    output.push_back(status == SM_SUCCESS ? 1.0 : 0.0);
    sc_topology_brep_p10_append_snapshot(body.get(), status == SM_SUCCESS ? &returned : nullptr, output);
    return SC_OK;
}

// ---- geometry blocks ------------------------------------------------------------------

gw_CURVE* tt_read_nurb(Rd& rd)
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

// A curve block `kind dim ...`: 0 SmBSplineCurve `oob n p m cpts(x y z w) knots`, 2 bounded
// SmLine `P V d0 d1 scale io`, 3 SmEllipse `C X Y d0 d1 rx ry io`.
SmCurve* tt_read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    ULONG dim = (ULONG)rd.idx();
    if (kind == 0)
    {
        bool oob = rd.flag();
        SmBSplineCurve* c = new (ctx) SmBSplineCurve(tt_read_nurb(rd), dim, FALSE);
        c->SetOutOfBoundsEnabled(oob ? TRUE : FALSE);
        return c;
    }
    if (kind == 2)
    {
        SmPoint3d p = rd.vec();
        SmVector3d v = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double scale = rd.num();
        bool io = rd.flag();
        return new (ctx) SmLine(p, v, SmExtent1d(d0, d1), scale, 3, &ctx, NULL, io ? TRUE : FALSE);
    }
    SmPoint3d c = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    double d0 = rd.num();
    double d1 = rd.num();
    double rx = rd.num();
    double ry = rd.num();
    bool io = rd.flag();
    return new (ctx) SmEllipse(c, x, y, SmExtent1d(d0, d1), rx, ry, dim, &ctx, NULL, io ? TRUE : FALSE);
}

// The NURB of a B-spline curve: `n p m cpts(x y z w) knots`, `-1` without one.
void tt_put_nurb(std::vector<double>& o, const SmCurve* c)
{
    gw_CURVE* nurb = (c && c->IsKindOf(SmBSplineCurve_TYPE)) ? ((const SmBSplineCurve*)c)->GetGwNurbPointer() : NULL;
    if (nurb == NULL)
    {
        o.push_back(-1.0);
        return;
    }
    o.push_back((double)nurb->pol->n);
    o.push_back((double)nurb->p);
    o.push_back((double)nurb->knt->m);
    for (gw_INDEX k = 0; k <= nurb->pol->n; k++)
    {
        o.push_back(nurb->pol->Pw[k].x);
        o.push_back(nurb->pol->Pw[k].y);
        o.push_back(nurb->pol->Pw[k].z);
        o.push_back(nurb->pol->Pw[k].w);
    }
    for (gw_INDEX k = 0; k <= nurb->knt->m; k++)
        o.push_back(nurb->knt->U[k]);
}

// A curve's natural interval and its points at five parameters (status, then 3 coords).
void tt_put_samples(std::vector<double>& o, const SmCurve* c)
{
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    for (int i = 0; i <= 4; i++)
    {
        SmPoint3d p;
        SmStatus s = c->EvaluatePoint(ivl.Evaluate(i / 4.0), p);
        o.push_back(st(s));
        put_vec(o, s == SM_SUCCESS ? p : SmPoint3d(0, 0, 0));
    }
}

// A surface block: that of sc_topology_fixture.h (0 B-spline, 1 plane), or `2 basis
// implicit`, an SmCurveBoundedSurface without boundary curves over a basis block.
SmSurface* tt_read_surface(Rd& rd, const SmContext& ctx)
{
    if (rd.in[rd.at] == 2.0)
    {
        rd.idx();
        SmSurface* basis = read_surface(rd, ctx);
        bool implicit = rd.flag();
        SmTArray<SmCurve*> c3d, cuv;
        SmSurface* s = new (ctx) SmCurveBoundedSurface(*basis, &c3d, &cuv, implicit ? TRUE : FALSE);
        return s;
    }
    return read_surface(rd, ctx);
}

// ---- verbatim copy of the file-static sm_IsLoopUVGapWithinTolerance ---------------------

SmBoolean tt_IsLoopUVGapWithinTolerance(const SmSurface& crSurface, const SmPoint2d& crStartUV,
                                        const SmPoint2d& crEndUV, const SmPoint3d& crStartPoint,
                                        const SmPoint3d& crEndPoint, SmXSectTol3d sXSectTol3d)
{
    const SmBSplineSurface* pSurface = dynamic_cast<const SmBSplineSurface*>(&crSurface);
    if (pSurface == NULL)
    {
        return FALSE;
    }

    SmExtent2d sGapDomain;
    sGapDomain.AddPoint2d(crStartUV);
    sGapDomain.AddPoint2d(crEndUV);
    const SmExtent2d sNaturalDomain = pSurface->GetNaturalUVDomain();
    if (sGapDomain.GetMin().x < sNaturalDomain.GetMin().x || sGapDomain.GetMin().y < sNaturalDomain.GetMin().y ||
        sGapDomain.GetMax().x > sNaturalDomain.GetMax().x || sGapDomain.GetMax().y > sNaturalDomain.GetMax().y)
    {
        return FALSE;
    }

    SmTArray<SmPoint3d> sPoints;
    SmTArray<double> sWeights;
    if (sGapDomain.XLength() == 0.0 || sGapDomain.YLength() == 0.0)
    {
        const SmSurfParamType eConstantDir = sGapDomain.XLength() == 0.0 ? SM_SP_U : SM_SP_V;
        const double dConstantParam = eConstantDir == SM_SP_U ? crStartUV.x : crStartUV.y;
        const SmExtent1d sGapInterval =
            eConstantDir == SM_SP_U ? sGapDomain.GetVInterval() : sGapDomain.GetUInterval();
        if (sGapInterval.GetLength() <= 0.0)
        {
            return FALSE;
        }
        SmBSplineCurve* pCurve = NULL;
        const SmStatus eStatus = pSurface->SmBSplineSurface::CreateIsoParametricCurve(
            *pSurface->GetContext(), eConstantDir, dConstantParam, SmApproxTol3d(0.0), pCurve);
        SmObjDelete sDeleteCurve(pCurve);
        if (eStatus != SM_SUCCESS || pCurve == NULL)
        {
            return FALSE;
        }
        SmExtent1d sTrimInterval = sGapInterval;
        if (pCurve->Trim(sTrimInterval) != SM_SUCCESS)
        {
            return FALSE;
        }
        const SmExtent1d sCurveInterval = pCurve->GetNaturalInterval();
        if (sCurveInterval.GetMin() > sGapInterval.GetMin() || sCurveInterval.GetMax() < sGapInterval.GetMax() ||
            pCurve->GetControlPolygon(sPoints, sWeights) != SM_SUCCESS)
        {
            return FALSE;
        }
    }
    else
    {
        SmBSplineSurface* pPatch = NULL;
        const SmStatus eStatus = pSurface->CopySubPatch(*pSurface->GetContext(), sGapDomain, pPatch);
        SmObjDelete sDeletePatch(pPatch);
        if (eStatus != SM_SUCCESS || pPatch == NULL)
        {
            return FALSE;
        }

        const SmExtent2d sPatchDomain = pPatch->GetNaturalUVDomain();
        if (sPatchDomain.GetMin().x > sGapDomain.GetMin().x || sPatchDomain.GetMin().y > sGapDomain.GetMin().y ||
            sPatchDomain.GetMax().x < sGapDomain.GetMax().x || sPatchDomain.GetMax().y < sGapDomain.GetMax().y)
        {
            return FALSE;
        }

        ULONG lNumU, lNumV;
        if (pPatch->GetControlPointNet(lNumU, lNumV, sPoints, sWeights) != SM_SUCCESS)
        {
            return FALSE;
        }
    }
    if (sPoints.GetSize() == 0)
    {
        return FALSE;
    }
    const SmBoolean bRational = sWeights.GetSize() != 0;
    for (ULONG lPoint = 0; lPoint < sPoints.GetSize(); ++lPoint)
    {
        if ((bRational && !(sWeights[lPoint] > 0.0)) || !(sPoints[lPoint].DistanceBetween(crStartPoint) <= sXSectTol3d) ||
            !(sPoints[lPoint].DistanceBetween(crEndPoint) <= sXSectTol3d))
        {
            return FALSE;
        }
    }
    return TRUE;
}

// ---- geometry ops ---------------------------------------------------------------------------

int64_t tt_index_of(const std::vector<SmCurve*>& v, const SmCurve* c)
{
    for (size_t i = 0; i < v.size(); i++)
        if (v[i] == c)
            return (int64_t)i;
    return -1;
}

// `nloops counts.. orients.. normal[3] nouter outer..` after FindOuterLoops.
void tt_put_outer(std::vector<double>& o, const std::vector<SmCurve*>& curves, SmTArray<SmCurve*>& in,
                  SmTArray<ULONG>& counts, SmTArray<SmOrientType>& orients, const SmVector3d& normal,
                  SmTArray<ULONG>& outer)
{
    o.push_back((double)in.GetSize());
    for (ULONG i = 0; i < in.GetSize(); i++)
        o.push_back((double)tt_index_of(curves, in[i]));
    o.push_back((double)counts.GetSize());
    for (ULONG i = 0; i < counts.GetSize(); i++)
        o.push_back((double)counts[i]);
    for (ULONG i = 0; i < orients.GetSize(); i++)
        o.push_back((double)orients[i]);
    put_vec(o, normal);
    o.push_back((double)outer.GetSize());
    for (ULONG i = 0; i < outer.GetSize(); i++)
        o.push_back((double)outer[i]);
}

// FindOuterLoops leaves its outputs undefined (reads past its arrays) when a loop of zero
// area is never classified; that shows as fewer output curves than input curves, so the
// call is made on copies and only reported when it kept every curve.
void tt_find_outer(std::vector<double>& o, const std::vector<SmCurve*>& curves, SmTArray<SmCurve*>& in,
                   SmTArray<ULONG>& counts, SmTArray<SmOrientType>& orients, SmVector3d& normal, double tol)
{
    ULONG n_in = in.GetSize();
    SmTArray<ULONG> outer;
    SmStatus s = SmTrimmingTools::FindOuterLoops(in, counts, orients, normal, tol, outer);
    o.push_back(st(s));
    if (in.GetSize() != n_in)
    {
        o.push_back(TT_NULL_DEREF);
        return;
    }
    tt_put_outer(o, curves, in, counts, orients, normal, outer);
}

void tt_geometry_op(int32_t op, Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    switch (op)
    {
    case OP_ORDER:
    {
        int64_t k = rd.idx();
        std::vector<SmCurve*> curves;
        SmTArray<SmCurve*> in;
        for (int64_t i = 0; i < k; i++)
        {
            curves.push_back(tt_read_curve(rd, ctx));
            in.Add(curves.back());
        }
        double tol = rd.num();
        bool outer = rd.flag();
        SmTArray<SmCurve*> ordered;
        SmTArray<ULONG> counts;
        SmTArray<SmOrientType> orients;
        SmPoint3d pt;
        SmVector3d normal;
        SmExtent3d box;
        SmStatus s = SmTrimmingTools::OrderCurvesIntoLoops(in, tol, ordered, counts, orients, pt, normal, box);
        o.push_back(st(s));
        if (s == SM_SUCCESS || s == SM_ERR)
        {
            o.push_back((double)ordered.GetSize());
            for (ULONG i = 0; i < ordered.GetSize(); i++)
                o.push_back((double)tt_index_of(curves, ordered[i]));
            o.push_back((double)counts.GetSize());
            for (ULONG i = 0; i < counts.GetSize(); i++)
                o.push_back((double)counts[i]);
            for (ULONG i = 0; i < orients.GetSize(); i++)
                o.push_back((double)orients[i]);
            put_vec(o, pt);
            put_vec(o, normal);
            put_vec(o, box.GetMin());
            put_vec(o, box.GetMax());
            if (outer && s == SM_SUCCESS)
                tt_find_outer(o, curves, ordered, counts, orients, normal, tol);
        }
        for (SmCurve* c : curves)
            delete c;
        break;
    }
    case OP_OUTER:
    {
        int64_t k = rd.idx();
        std::vector<SmCurve*> curves;
        SmTArray<SmCurve*> in;
        for (int64_t i = 0; i < k; i++)
        {
            curves.push_back(tt_read_curve(rd, ctx));
            in.Add(curves.back());
        }
        int64_t nl = rd.idx();
        SmTArray<ULONG> counts;
        for (int64_t i = 0; i < nl; i++)
            counts.Add((ULONG)rd.idx());
        SmTArray<SmOrientType> orients;
        for (int64_t i = 0; i < k; i++)
            orients.Add((SmOrientType)rd.idx());
        SmVector3d normal = rd.vec();
        double tol = rd.num();
        tt_find_outer(o, curves, in, counts, orients, normal, tol);
        for (SmCurve* c : curves)
            delete c;
        break;
    }
    case OP_MIRROR:
    {
        SmBSplineCurve* c = (SmBSplineCurve*)tt_read_curve(rd, ctx);
        SmSurfParamType side = (SmSurfParamType)rd.idx();
        double low = rd.num();
        double high = rd.num();
        double tol = rd.num();
        SmStatus s = SmTrimmingTools::MirrorCurve(c, side, low, high, tol);
        o.push_back(st(s));
        o.push_back((double)c->GetDim());
        tt_put_nurb(o, c);
        delete c;
        break;
    }
    case OP_ISO_AT:
    {
        SmBSplineCurve* c = (SmBSplineCurve*)tt_read_curve(rd, ctx);
        SmSurfParamType side = (SmSurfParamType)rd.idx();
        double low = rd.num();
        double tol = rd.num();
        o.push_back(flg(SmTrimmingTools::IsIsoCurve(c, side, low, tol)));
        delete c;
        break;
    }
    case OP_ISO:
    {
        SmBSplineCurve* c = (SmBSplineCurve*)tt_read_curve(rd, ctx);
        SmSurfParamType side = SM_SP_UNKNOWN;
        double param = 0.0;
        SmBoolean r = SmTrimmingTools::IsIsoCurve(c, side, param);
        o.push_back(flg(r));
        if (r)
        {
            o.push_back((double)side);
            o.push_back(param);
        }
        delete c;
        break;
    }
    case OP_ISO_SEAM:
    {
        SmSurface* s = tt_read_surface(rd, ctx);
        SmBSplineCurve* c = (SmBSplineCurve*)tt_read_curve(rd, ctx);
        SmSurfParamType side = SM_SP_UNKNOWN;
        double param = 0.0;
        o.push_back(flg(SmTrimmingTools::IsIsoCurveOnSeam(s, c, side, param)));
        delete c;
        delete s;
        break;
    }
    case OP_CONTINUITY:
    {
        SmSurface* s = tt_read_surface(rd, ctx);
        SmBoolean g1 = UNSURE;
        SmStatus stt = SmTrimmingTools::SurfaceContinuityCheck(s, g1);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            o.push_back(flg(g1));
        delete s;
        break;
    }
    case OP_UV_GAP:
    {
        SmSurface* s = tt_read_surface(rd, ctx);
        SmPoint2d a = rd.uv();
        SmPoint2d b = rd.uv();
        double tol = rd.num();
        bool given = rd.flag();
        SmPoint3d pa = rd.vec();
        SmPoint3d pb = rd.vec();
        SmStatus sa = given ? SM_SUCCESS : s->EvaluatePoint(a, pa);
        SmStatus sb = given ? SM_SUCCESS : s->EvaluatePoint(b, pb);
        o.push_back(st(sa));
        o.push_back(st(sb));
        if (sa == SM_SUCCESS && sb == SM_SUCCESS)
        {
            put_vec(o, pa);
            put_vec(o, pb);
            o.push_back(flg(tt_IsLoopUVGapWithinTolerance(*s, a, b, pa, pb, SmXSectTol3d(tol))));
        }
        delete s;
        break;
    }
    case OP_MIN:
    {
        double a = rd.num();
        double b = rd.num();
        o.push_back(SmTrimmingTools::Min(a, b));
        break;
    }
    case OP_TRIM_NULL:
    {
        // the null-region error of TrimSurfaceWithModelSpaceCurves (it deletes its inputs)
        SmSurface* s = tt_read_surface(rd, ctx);
        int64_t k = rd.idx();
        SmTArray<SmCurve*> curves;
        SmTArray<SmOrientType> orients;
        SmTArray<ULONG> counts;
        for (int64_t i = 0; i < k; i++)
        {
            curves.Add(tt_read_curve(rd, ctx));
            orients.Add(SM_OT_SAME);
        }
        counts.Add((ULONG)k);
        SmTArray<SmPoint3d> pts;
        SmExtent2d dom = s->GetNaturalUVDomain();
        SmStatus stt = SmTrimmingTools::TrimSurfaceWithModelSpaceCurves(ctx, (SmRegion*)NULL, 1e-5, counts, curves,
                                                                         orients, pts, s, dom, SM_OT_SAME, TRUE, TRUE,
                                                                         FALSE, NULL);
        o.push_back(st(stt));
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}

// ---- the SmCurveLoops script ----------------------------------------------------------------

struct TtLoops
{
    std::vector<SmCurve*> store;
    int64_t key(const SmCurve* c) const { return tt_index_of(store, c); }
    // registers curves of the loops not in the store yet (in list order)
    void absorb(const SmCurveLoops& l)
    {
        for (ULONG i = 0; i < l.m_sCurves.GetSize(); i++)
            if (key(l.m_sCurves[i]) < 0)
                store.push_back(l.m_sCurves[i]);
    }
};

// Whether SampleNearbyPoints can run without reading past the loop arrays.
bool tt_sample_safe(SmCurveLoops& l, SmCurve* c, double param)
{
    SmExtent1d d = c->GetNaturalInterval();
    double tol = d.GetLength() / 1000;
    if (d.ContainsValue(param, -tol))
        return true;
    ULONG adj = 0;
    l.FindAdjacentLoopCurve(c, param, adj);
    return adj < l.m_sCurves.GetSize() && adj < l.m_sOrients.GetSize();
}

void tt_loops_op(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    TtLoops t;
    int64_t ns = rd.idx();
    for (int64_t i = 0; i < ns; i++)
        t.store.push_back(tt_read_curve(rd, ctx));
    int64_t nl = rd.idx();
    SmTArray<ULONG> counts;
    for (int64_t i = 0; i < nl; i++)
        counts.Add((ULONG)rd.idx());
    int64_t nk = rd.idx();
    SmTArray<SmCurve*> keys;
    SmTArray<SmBSplineCurve*> uv_keys;
    for (int64_t i = 0; i < nk; i++)
    {
        SmCurve* c = t.store[(size_t)rd.idx()];
        keys.Add(c);
        uv_keys.Add((SmBSplineCurve*)c);
    }
    SmTArray<SmOrientType> orients;
    for (int64_t i = 0; i < nk; i++)
        orients.Add((SmOrientType)rd.idx());
    int64_t mode = rd.idx();
    SmSurface* srf = NULL;
    if (rd.flag())
        srf = tt_read_surface(rd, ctx);
    SmCurveLoops l(counts, mode == 1 ? &uv_keys : NULL, mode == 0 ? &keys : NULL, orients);
    int64_t nops = rd.idx();
    for (int64_t q = 0; q < nops; q++)
    {
        int64_t code = rd.idx();
        o.push_back((double)code);
        switch (code)
        {
        case 1:
        {
            ULONG s = 99;
            SmStatus r = l.GetLoopStartIndex((ULONG)rd.idx(), s);
            o.push_back(st(r));
            o.push_back((double)s);
            break;
        }
        case 2:
        {
            ULONG s = 99, e = 99;
            SmStatus r = l.GetLoopStartEnd((ULONG)rd.idx(), s, e);
            o.push_back(st(r));
            if (r == SM_SUCCESS)
            {
                o.push_back((double)s);
                o.push_back((double)e);
            }
            break;
        }
        case 3:
        {
            SmCurve* c = t.store[(size_t)rd.idx()];
            double param = rd.num();
            ULONG adj = 0;
            SmStatus r = l.FindAdjacentLoopCurve(c, param, adj);
            o.push_back(st(r));
            if (r == SM_SUCCESS)
                o.push_back((double)adj);
            break;
        }
        case 4:
        {
            ULONG a = (ULONG)rd.idx(), b = (ULONG)rd.idx();
            ULONG e = 99, lv = 99;
            SmStatus r = l.FindEnteringAndLeaving(a, b, e, lv);
            o.push_back(st(r));
            if (r == SM_SUCCESS)
            {
                o.push_back((double)e);
                o.push_back((double)lv);
            }
            break;
        }
        case 5:
        {
            SmCurve* c = t.store[(size_t)rd.idx()];
            double param = rd.num();
            if (!tt_sample_safe(l, c, param))
            {
                o.push_back(TT_NULL_DEREF);
                break;
            }
            SmPoint3d p1, p2;
            o.push_back(st(l.SampleNearbyPoints(c, param, p1, p2)));
            put_vec(o, p1);
            put_vec(o, p2);
            break;
        }
        case 6:
        {
            SmCurve* c = t.store[(size_t)rd.idx()];
            double param = rd.num();
            SmSurfParamType dir = (SmSurfParamType)rd.idx();
            double iso = rd.num();
            double ztol = rd.num();
            SmPoint2d d0 = rd.uv(), d1 = rd.uv();
            if (srf == NULL || !tt_sample_safe(l, c, param))
            {
                o.push_back(TT_NULL_DEREF);
                break;
            }
            SmCurveIntersections ci(NULL, dir, iso, NULL);
            SmZoneTol3d z(ztol);
            o.push_back(flg(l.IsSameSideIntersection(c, param, &ci, srf, SmExtent2d(d0, d1), z)));
            break;
        }
        case 7:
        {
            ULONG idx = (ULONG)rd.idx();
            SmCurveLoops other = l;
            o.push_back(st(l.AppendLoop(other, idx)));
            break;
        }
        case 8:
        {
            SmCurve* nc = t.store[(size_t)rd.idx()];
            SmCurve* after = t.store[(size_t)rd.idx()];
            o.push_back(st(l.InsertIntoLoops(nc, after)));
            break;
        }
        case 9:
        {
            SmIsoXSectSegment seg(t.store[(size_t)rd.idx()]);
            double a = rd.num(), b = rd.num();
            seg.m_sInterval = SmExtent1d(a, b);
            seg.m_lStartEnter = (ULONG)rd.idx();
            seg.m_lStartLeave = (ULONG)rd.idx();
            seg.m_lEndEnter = (ULONG)rd.idx();
            seg.m_lEndLeave = (ULONG)rd.idx();
            o.push_back(st(l.InsertIsoXSectSegment(seg)));
            o.push_back(seg.m_sInterval.GetMin());
            o.push_back(seg.m_sInterval.GetMax());
            t.absorb(l);
            break;
        }
        case 10:
        {
            int64_t nint = rd.idx();
            std::vector<SmCurveIntersections*> ints;
            SmCurveIntersections* head = NULL;
            // built back to front so that the list runs in input order
            std::vector<std::vector<double>> blocks;
            for (int64_t i = 0; i < nint; i++)
            {
                int64_t iso = rd.idx();
                SmSurfParamType dir = (SmSurfParamType)rd.idx();
                double isop = rd.num();
                SmCurveIntersections* ci =
                    new SmCurveIntersections(iso < 0 ? NULL : t.store[(size_t)iso], dir, isop, NULL);
                int64_t nsol = rd.idx();
                for (int64_t j = 0; j < nsol; j++)
                {
                    int64_t obj = rd.idx();
                    double param = rd.num();
                    SmSolution s;
                    s.m_lNumObjects = 2;
                    s.m_lNumVariables = 2;
                    s.m_apObjects[0] = ci->m_pIsoCurve;
                    s.m_apObjects[1] = obj == -1 ? NULL : obj == -2 ? (SmObject*)srf : t.store[(size_t)obj];
                    s.m_vStart.m_adParameters[0] = 0.0;
                    s.m_vStart.m_adParameters[1] = param;
                    ci->m_sSolArray.Add(s);
                }
                ints.push_back(ci);
            }
            for (size_t i = ints.size(); i-- > 0;)
            {
                ints[i]->m_pNext = head;
                head = ints[i];
            }
            SmCurve* orig = t.store[(size_t)rd.idx()];
            SmCurve* nc = t.store[(size_t)rd.idx()];
            SmZoneTol3d z(rd.num());
            o.push_back(st(l.UpdateSolutionObjects(head, orig, nc, z)));
            for (SmCurveIntersections* ci : ints)
            {
                o.push_back((double)ci->m_sSolArray.GetSize());
                for (ULONG j = 0; j < ci->m_sSolArray.GetSize(); j++)
                {
                    const SmObject* p = ci->m_sSolArray[j].m_apObjects[1];
                    o.push_back(p == NULL ? -1.0 : p == (SmObject*)srf ? -2.0 : (double)t.key((const SmCurve*)p));
                }
                delete ci;
            }
            break;
        }
        case 11:
            o.push_back(st(l.Reset()));
            break;
        case 12:
        {
            SmIsoXSectSegment seg(NULL);
            seg.m_lStartEnter = (ULONG)rd.idx();
            seg.m_lStartLeave = (ULONG)rd.idx();
            seg.m_lEndEnter = (ULONG)rd.idx();
            seg.m_lEndLeave = (ULONG)rd.idx();
            o.push_back((double)seg.GetMinIndex());
            o.push_back((double)seg.GetMaxIndex());
            break;
        }
        default: o.push_back((double)SC_ERR_ARG); break;
        }
    }
    // final state
    o.push_back(flg(l.IsValid()));
    o.push_back((double)l.GetNumLoops());
    for (ULONG i = 0; i < l.m_sLoopCounts.GetSize(); i++)
        o.push_back((double)l.m_sLoopCounts[i]);
    o.push_back((double)l.m_sCurves.GetSize());
    for (ULONG i = 0; i < l.m_sCurves.GetSize(); i++)
        o.push_back((double)t.key(l.m_sCurves[i]));
    o.push_back((double)l.m_sOrients.GetSize());
    for (ULONG i = 0; i < l.m_sOrients.GetSize(); i++)
        o.push_back((double)l.m_sOrients[i]);
    o.push_back((double)(t.store.size() - (size_t)ns));
    for (size_t i = (size_t)ns; i < t.store.size(); i++)
        tt_put_samples(o, t.store[i]);
    for (SmCurve* c : t.store)
        delete c;
    delete srf;
}

// ---- B-rep ops ------------------------------------------------------------------------------

struct TtFacePeek : SmFace
{
    static SmSurface* SmFace::*surface() { return &TtFacePeek::m_pSurface; }
};
struct TtEdgePeek : SmEdge
{
    static SmCurve* SmEdge::*curve() { return &TtEdgePeek::m_pCurve; }
};

// A plain SmBSplineCurve of `c`'s NURB, or nullptr when `c` already is one (or a line)
// or has no NURB.
SmBSplineCurve* tt_plain_curve(SmCurve* c)
{
    if (c == nullptr || !c->IsKindOf(SmBSplineCurve_TYPE) || c->GetType() == SmBSplineCurve_TYPE ||
        c->GetType() == SmLine_TYPE)
        return nullptr;
    SmBSplineCurve* b = (SmBSplineCurve*)c;
    gw_CURVE* nurb = b->GetGwNurbPointer();
    if (nurb == nullptr)
        return nullptr;
    return new (the_context()) SmBSplineCurve(sm_AllocateAndCopyNurbCurve(nurb), b->GetDim(), FALSE);
}

// The Rust side imports the export's NURBs as plain B-splines (and lines), and SmPlanes
// as planes, and exact analytic surfaces as their own kind: give every other face and
// edge of the C++ B-rep the same geometry, as the heal.gap shim does. Replaced
// originals are left unowned and leaked.
void tt_normalise(Graph& g)
{
    for (SmFace* f : g.faces)
    {
        SmSurface* s = f->*TtFacePeek::surface();
        if (s == nullptr || !s->IsKindOf(SmBSplineSurface_TYPE) || s->GetType() == SmBSplineSurface_TYPE ||
            s->GetType() == SmPlane_TYPE)
            continue;
        // Preserve every surface the shared export imports as its exact analytic kind.
        std::vector<double> head;
        if (sc_analytic_export::put_exact_head(head, s, the_context()))
            continue;
        SmBSplineSurface* b = (SmBSplineSurface*)s;
        gw_SURFACE* nurb = b->GetGwNurbPointer();
        if (nurb == nullptr)
            continue;
        SmBSplineSurface* nb = new (the_context()) SmBSplineSurface(sm_AllocateAndCopyNurbSurface(nurb), FALSE, NULL);
        nb->GetOutOfBoundsEnabled() = b->GetOutOfBoundsEnabled();
        nb->SetBSplineSurfaceForm(b->GetBSplineSurfaceForm());
        f->*TtFacePeek::surface() = nb;
    }
    for (SmEdge* e : g.edges)
    {
        SmBSplineCurve* nc = tt_plain_curve(e->*TtEdgePeek::curve());
        if (nc != nullptr)
            e->*TtEdgePeek::curve() = nc;
    }
}

// Every edge use's own UV trim curve, in order.
void tt_put_uv_curves(std::vector<double>& o, Graph& g)
{
    o.push_back((double)g.edgeuses.size());
    for (SmEdgeuse* p : g.edgeuses)
    {
        SmBSplineCurve* c = p->*EuPeek::uv();
        if (c == nullptr)
            o.push_back(-1.0);
        else
        {
            o.push_back((double)c->GetDim());
            tt_put_nurb(o, c);
        }
    }
}

// A loop whose loop uses upstream can walk: it has its loop use.
bool tt_ok_loop(SmLoop* l) { return l != nullptr && l->GetLoopuse() != nullptr; }

// The face and surface of a loop (nullptr when missing).
SmFace* tt_loop_face(SmLoop* l)
{
    SmLoopuse* lu = l->GetLoopuse();
    SmFaceuse* fu = lu ? lu->GetFaceuse() : nullptr;
    return fu ? fu->GetFace() : nullptr;
}

// Whether GetEdgeUVExtremes of `eu` dereferences nothing null: its UV trim curve (own or
// mate's), else its and its mate's vertex uses.
bool tt_ok_extremes(SmEdgeuse* eu)
{
    if (eu->GetUVTrimCurve() != nullptr)
        return true;
    SmEdgeuse* m = eu->GetMate();
    return eu->GetVertexuse() != nullptr && m != nullptr && m->GetVertexuse() != nullptr;
}

bool tt_ok_loop_extremes(SmLoop* l)
{
    SmLoopuse* a;
    SmLoopuse* b;
    l->GetLoopuses(a, b);
    SmTArray<SmEdgeuse*> eus;
    a->GetEdgeuses(eus);
    for (ULONG i = 0; i < eus.GetSize(); i++)
        if (!tt_ok_extremes(eus[i]))
            return false;
    return true;
}

// Whether the loop walks of FixTrimLoopOrientation / FixTrimLoopOrder dereference
// nothing null: every loop has its loop use, and a face with loops has a surface and UV
// trim curves on every edge use (SmFace::ComputeLoopOrientation and the UV boxes of
// FixTrimLoopOrder read them).
bool tt_ok_face_loops(SmFace* f)
{
    SmTArray<SmLoop*> ls;
    f->GetLoops(ls);
    for (ULONG i = 0; i < ls.GetSize(); i++)
        if (!tt_ok_loop(ls[i]))
            return false;
    return ls.GetSize() == 0 || (f->GetSurface() != nullptr && SmTrimmingTools::AreUVCurvesPresent(f));
}

// The pre-step of the repair and check ops: `k side` (k = -1: none) mirrors the UV trim
// curve of the k-th edge use (modulo their count) of `l`'s first loop use across its face
// surface's natural domain with SmTrimmingTools::MirrorCurve (only a plain B-spline curve;
// `-1` when skipped), so that loops cross seams or jump in UV.
void tt_mirror_pre(Rd& rd, SmLoop* l, std::vector<double>& o)
{
    int64_t k = rd.idx();
    SmSurfParamType side = (SmSurfParamType)rd.idx();
    if (k < 0 || !tt_ok_loop(l))
    {
        o.push_back(-1.0);
        return;
    }
    SmLoopuse* a;
    SmLoopuse* b;
    l->GetLoopuses(a, b);
    SmTArray<SmEdgeuse*> eus;
    a->GetEdgeuses(eus);
    SmFace* f = tt_loop_face(l);
    SmSurface* s = f ? f->GetSurface() : nullptr;
    if (eus.GetSize() == 0 || s == nullptr)
    {
        o.push_back(-1.0);
        return;
    }
    SmBSplineCurve* c = eus[(ULONG)(k % (int64_t)eus.GetSize())]->GetUVTrimCurve();
    if (c == nullptr || (c->GetType() != SmBSplineCurve_TYPE && c->GetType() != SmLine_TYPE))
    {
        o.push_back(-1.0);
        return;
    }
    SmExtent2d d = s->GetNaturalUVDomain();
    double low = side == SM_SP_U ? d.GetMin().x : d.GetMin().y;
    double high = side == SM_SP_U ? d.GetMax().x : d.GetMax().y;
    o.push_back(st(SmTrimmingTools::MirrorCurve(c, side, low, high, 0.0)));
}

// The reports of an assert list: `n [type list test owner_type owner tol value]..`.
void tt_put_reports(std::vector<double>& o, Graph& g, SmAssertArray& a)
{
    o.push_back((double)a.GetSize());
    for (ULONG i = 0; i < a.GetSize(); i++)
    {
        SmAssertReport* r = a[i];
        o.push_back((double)r->m_lReportingType);
        o.push_back((double)r->m_lListIndex);
        o.push_back((double)r->m_lTestIndex);
        o.push_back((double)r->m_lOwnerType);
        o.push_back(r->m_lOwnerType == SmLoop_TYPE ? (double)g.find(5, (const SmLoop*)r->m_pOwner)
                                                    : (double)g.find(3, (const SmFace*)r->m_pOwner));
        o.push_back((double)r->m_sTol3d);
        o.push_back(r->m_dValue);
    }
    o.push_back(flg(a.m_bLastAdd));
}

void tt_brep_op(int32_t op, Rd& rd, const char* path, std::vector<double>& o)
{
    SmStatus status = SM_SUCCESS;
    Built built;
    built.brep = build(rd, path, status);
    if (built.brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(status));
        return;
    }
    o.push_back(0.0);
    Graph g;
    walk(built.brep, g);
    tt_normalise(g);
    switch (op)
    {
    case OP_B_PRESENT:
    {
        for (SmLoop* l : g.loops)
            o.push_back(tt_ok_loop(l) ? flg(SmTrimmingTools::AreUVCurvesPresent(l)) : TT_NULL_DEREF);
        for (SmFace* f : g.faces)
        {
            SmTArray<SmLoop*> ls;
            f->GetLoops(ls);
            bool ok = true;
            for (ULONG i = 0; i < ls.GetSize(); i++)
                ok = ok && tt_ok_loop(ls[i]);
            o.push_back(ok ? flg(SmTrimmingTools::AreUVCurvesPresent(f)) : TT_NULL_DEREF);
        }
        break;
    }
    case OP_B_EXTREMES:
    {
        SmEdgeuse* eu = g.edgeuses.at((size_t)rd.idx());
        if (!tt_ok_extremes(eu))
        {
            o.push_back(TT_NULL_DEREF);
            break;
        }
        bool has_curve = eu->GetUVTrimCurve() != nullptr;
        SmPoint2d s, e;
        double sw = -7.0, ew = -7.0;
        SmStatus r = SmTrimmingTools::GetEdgeUVExtremes(eu, s, sw, e, ew);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back(s.x);
            o.push_back(s.y);
            o.push_back(e.x);
            o.push_back(e.y);
            o.push_back(flg(has_curve));
            if (has_curve)
            {
                o.push_back(sw);
                o.push_back(ew);
            }
        }
        break;
    }
    case OP_B_CHECK_LOOP:
    {
        SmLoop* l = g.loops.at((size_t)rd.idx());
        double tol = rd.num();
        bool want_list = rd.flag();
        tt_mirror_pre(rd, l, o);
        if (rd.flag())
        {
            size_t selector = (size_t)rd.idx();
            double displacement = rd.num();
            SmLoopuse *lu, *mate;
            l->GetLoopuses(lu, mate);
            SmTArray<SmEdgeuse*> eus;
            if (lu != nullptr)
                lu->GetEdgeuses(eus);
            SmBSplineCurve* curve = eus.GetSize() ? eus[(ULONG)(selector % eus.GetSize())]->GetUVTrimCurve() : nullptr;
            if (curve == nullptr || (curve->GetType() != SmBSplineCurve_TYPE && curve->GetType() != SmLine_TYPE))
                o.push_back(-1.0);
            else
            {
                SmPoint3d point;
                SmStatus r = curve->EvaluatePoint(curve->GetNaturalInterval().GetMin(), point);
                if (r == SM_SUCCESS)
                {
                    point.x += displacement;
                    r = curve->EditEndPoint(point, TRUE, NULL);
                }
                o.push_back(st(r));
            }
        }
        if (rd.flag())
        {
            double edge_tol = rd.num(), vertex_tol = rd.num();
            for (SmEdge* e : g.edges)
                e->SetTolerance(SmZoneTol3d(edge_tol), FALSE, FALSE);
            for (SmVertex* v : g.vertices)
                v->SetTolerance(SmZoneTol3d(vertex_tol), FALSE, FALSE);
        }
        if (rd.flag())
        {
            const size_t selector = (size_t)rd.idx();
            SmLoopuse* lu = l->GetLoopuse();
            SmTArray<SmEdgeuse*> eus;
            if (lu != nullptr) lu->GetEdgeuses(eus);
            if (eus.GetSize()) eus[(ULONG)(selector % eus.GetSize())]->DeleteUVTrimCurve();
        }
        SmFace* f = tt_ok_loop(l) ? tt_loop_face(l) : nullptr;
        SmSurface* s = f ? f->GetSurface() : nullptr;
        if (s == nullptr || !tt_ok_loop_extremes(l))
        {
            o.push_back(TT_NULL_DEREF);
            break;
        }
        SmAssertArray a;
        ULONG closure = 99;
        SmStatus r = SmTrimmingTools::CheckLoop(s, l, tol, closure, want_list ? &a : NULL);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
            o.push_back((double)closure);
        tt_put_reports(o, g, a);
        break;
    }
    case OP_B_CHECK_FACE:
    {
        SmFace* f = g.faces.at((size_t)rd.idx());
        double tol = rd.num();
        bool want_list = rd.flag();
        bool skip = rd.flag();
        SmTArray<SmLoop*> ls;
        f->GetLoops(ls);
        bool ok = true;
        for (ULONG i = 0; i < ls.GetSize(); i++)
            ok = ok && tt_ok_loop(ls[i]);
        if (!ok)
        {
            o.push_back(TT_NULL_DEREF);
            break;
        }
        // With UV curves on every loop CheckFace reaches CheckLoop of every loop (which
        // reads each loop's UV extremes) and SmFace::ComputeLoopOrientation: guarded
        // (TT_NULL_DEREF) where upstream would dereference a missing surface or extreme.
        bool all_uv = ls.GetSize() > 0 && SmTrimmingTools::AreUVCurvesPresent(f);
        SmAssertArray a;
        if (all_uv)
        {
            if (f->GetSurface() == nullptr)
                ok = false;
            for (ULONG i = 0; ok && !skip && i < ls.GetSize(); i++)
                ok = tt_ok_loop_extremes(ls[i]);
            if (!ok)
            {
                o.push_back(TT_NULL_DEREF);
                break;
            }
        }
        o.push_back(st(SmTrimmingTools::CheckFace(f, tol, want_list ? &a : NULL, SM_LEVEL_0, NULL,
                                                  skip ? TRUE : FALSE)));
        tt_put_reports(o, g, a);
        break;
    }
    case OP_B_TINY:
    {
        bool of_face = rd.flag();
        size_t idx = (size_t)rd.idx();
        double tol = rd.num();
        SmTArray<SmLoop*> ls;
        if (of_face)
            g.faces.at(idx)->GetLoops(ls);
        else
            ls.Add(g.loops.at(idx));
        tt_mirror_pre(rd, ls.GetSize() > 0 ? ls[0] : nullptr, o);
        // a gap to close at an edge use without UV trim curve dereferences it: checked by
        // a dry run of the gap test on unedited curves (edits only shrink the gaps they close)
        for (ULONG i = 0; i < ls.GetSize(); i++)
        {
            if (!tt_ok_loop(ls[i]) || !tt_ok_loop_extremes(ls[i]))
            {
                o.push_back(TT_NULL_DEREF);
                return;
            }
            SmLoopuse* a;
            SmLoopuse* b;
            ls[i]->GetLoopuses(a, b);
            SmTArray<SmEdgeuse*> eus;
            a->GetEdgeuses(eus);
            ULONG n = eus.GetSize();
            for (ULONG k = 0; k < n; k++)
            {
                SmEdgeuse* cur = eus[k];
                SmEdgeuse* prev = eus[k == 0 ? n - 1 : k - 1];
                if (cur->GetUVTrimCurve() != nullptr && prev->GetUVTrimCurve() != nullptr)
                    continue;
                SmPoint2d ps, pe, cs, ce;
                if (SmTrimmingTools::GetEdgeUVExtremes(prev, ps, pe) != SM_SUCCESS ||
                    SmTrimmingTools::GetEdgeUVExtremes(cur, cs, ce) != SM_SUCCESS)
                    continue;
                if (pe.DistanceBetween(cs) < 2.0 * tol)
                {
                    o.push_back(TT_NULL_DEREF);
                    return;
                }
            }
        }
        SmBoolean made = UNSURE;
        SmStatus r = of_face ? SmTrimmingTools::FixTrimLoopTinyParameterSpaceGaps(g.faces.at(idx), tol, made)
                             : SmTrimmingTools::FixTrimLoopTinyParameterSpaceGaps(g.loops.at(idx), tol, made);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
            o.push_back(flg(made));
        tt_put_uv_curves(o, g);
        break;
    }
    case OP_B_ENSURE:
    {
        SmLoop* l = g.loops.at((size_t)rd.idx());
        tt_mirror_pre(rd, l, o);
        SmFace* f = tt_ok_loop(l) ? tt_loop_face(l) : nullptr;
        SmSurface* s = f ? f->GetSurface() : nullptr;
        if (s == nullptr)
        {
            o.push_back(TT_NULL_DEREF);
            break;
        }
        SmLoopuse* a;
        SmLoopuse* b;
        l->GetLoopuses(a, b);
        SmTArray<SmEdgeuse*> eus;
        a->GetEdgeuses(eus);
        for (ULONG i = 0; i < eus.GetSize(); i++)
            if (eus[i]->GetUVTrimCurve() == nullptr)
            {
                o.push_back(TT_NULL_DEREF);
                return;
            }
        SmBoolean all = UNSURE;
        SmStatus r = SmTrimmingTools::EnsureLastEdgeuseUVCorrect(s, eus, all);
        o.push_back(st(r));
        o.push_back(flg(all));
        o.push_back((double)eus.GetSize());
        for (ULONG i = 0; i < eus.GetSize(); i++)
            o.push_back((double)g.find(6, eus[i]));
        break;
    }
    case OP_B_BACKWARDS:
    {
        SmFace* f = g.faces.at((size_t)rd.idx());
        SmSurface* s = f->GetSurface();
        SmTArray<SmLoop*> ls;
        f->GetLoops(ls);
        tt_mirror_pre(rd, ls.GetSize() > 0 ? ls[0] : nullptr, o);
        if (s == nullptr)
        {
            o.push_back(TT_NULL_DEREF);
            break;
        }
        SmExtent2d bounds = s->GetNaturalUVDomain();
        bool closed = s->IsClosed(bounds, SM_SP_U) || s->IsClosed(bounds, SM_SP_V);
        if (closed)
        {
            if (ls.GetSize() == 0 || !tt_ok_loop(ls[0]) || !tt_ok_loop_extremes(ls[0]))
            {
                o.push_back(TT_NULL_DEREF);
                break;
            }
            SmLoopuse* a;
            SmLoopuse* b;
            ls[0]->GetLoopuses(a, b);
            SmTArray<SmEdgeuse*> eus;
            a->GetEdgeuses(eus);
            bool all_uv = true;
            for (ULONG i = 0; i < eus.GetSize(); i++)
                all_uv = all_uv && eus[i]->GetUVTrimCurve() != nullptr;
            if (!all_uv)
            {
                o.push_back(TT_NULL_DEREF);
                break;
            }
        }
        SmBoolean made = UNSURE;
        SmStatus r = SmTrimmingTools::FixBackwardsOuterLoopOnSeam(f, made);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
            o.push_back(flg(made));
        tt_put_uv_curves(o, g);
        break;
    }
    case OP_B_ORIENT:
    {
        // FixTrimLoopOrientation of the face (SmFace::ComputeLoopOrientation of every
        // loop): guarded like CheckFace's ComputeLoopOrientation
        SmFace* f = g.faces.at((size_t)rd.idx());
        if (!tt_ok_face_loops(f))
        {
            o.push_back(TT_NULL_DEREF);
            break;
        }
        ULONG n = 99;
        SmStatus r = SmTrimmingTools::FixTrimLoopOrientation(f, n);
        o.push_back(st(r));
        o.push_back((double)n);
        break;
    }
    case OP_B_ORDER:
    {
        // FixTrimLoopOrder (UV boxes of every loop's trim curves, then
        // SmFace::ComputeLoopOrientation): guarded like FixTrimLoopOrientation
        SmFace* f = g.faces.at((size_t)rd.idx());
        if (!tt_ok_face_loops(f))
        {
            o.push_back(TT_NULL_DEREF);
            break;
        }
        SmBoolean made = UNSURE;
        o.push_back(st(SmTrimmingTools::FixTrimLoopOrder(f, made)));
        break;
    }
    case OP_B_DEFERRED:
    {
        // SplitAtSeams: always deferred in Rust. ClearUVTrimCurves (void): `0`, and
        // CreateUVTrimCurves: its status; then every edge use's UV trim curve.
        int64_t kind = rd.idx();
        SmFace* f = g.faces.at((size_t)rd.idx());
        if (kind == 0)
        {
            SmTArray<SmFace*> new_faces;
            o.push_back(st(SmTrimmingTools::SplitAtSeams(the_context(), f, new_faces)));
        }
        else if (kind == 1)
        {
            SmTrimmingTools::ClearUVTrimCurves(f);
            o.push_back(0.0);
        }
        else
            o.push_back(st(SmTrimmingTools::CreateUVTrimCurves(f)));
        tt_put_uv_curves(o, g);
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_topology_trimming_tools_eval(int32_t op, const char* path, const double* in, int64_t n_in,
                                                  double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    if (op == OP_TRIM_CONSTRUCTED && (n_in != 1 || (in[0] != 0.0 && in[0] != 1.0)))
        return SC_ERR_ARG;
    SC_TRY
    if (op == OP_TRIM_CONSTRUCTED)
    {
        std::vector<double> result;
        int32_t status = tt_trim_constructed((int)in[0], result);
        if (status != SC_OK)
            return status;
        return sc_copy_out(result.data(), (int64_t)result.size(), out, cap, out_len);
    }
    Rd rd{in, 0};
    std::vector<double> o;
    if (op == OP_LOOPS)
        tt_loops_op(rd, o);
    else if (op < OP_B_PRESENT)
        tt_geometry_op(op, rd, o);
    else
        tt_brep_op(op, rd, path, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
