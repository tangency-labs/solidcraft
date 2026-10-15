// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.sphere`.
// Wraps the SmSphere members owned by the unit (source/SMLib/src/SmSphere.cpp): the constructor
// and CreateCanonical (with MakeNurb), operator==, Copy, CreateOffsetSurface,
// CreateIsoParametricCurve, ConvertUVFromSTEPToNURBS, ConvertUVFromNURBSToSTEP, EvaluateSTEP,
// EvaluateSTEPPoint, EvaluateNormal, GetSTEPUVDomain, GlobalSurfaceIntersect,
// IntersectWithPlane, IntersectWithSphere, IntersectWithCone, CreateTrimmedCircleSegments,
// IsNurbSurfaceSphere, RebuildSTEPFromNURBParameters, IsClosed, IsClosedU, IsClosedV,
// STEPInversion, Transform, GetMaxAnalyticDomain, GetInsideOut, GetRadius and IsKindOf.
//
// One entry point, sc_surface_sphere_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/surface_sphere.rs, which validates every input
// so upstream only touches valid memory. `out` starts with the status of the sphere
// construction.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmCircle.h>
#include <SmCone.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmSphere.h>
#include <SmSurfOfRevolution.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_sphere.rs).
enum : int32_t
{
    OP_BUILD = 0,
    OP_EVAL_STEP = 1,
    OP_TO_STEP = 2,
    OP_TO_NURBS = 3,
    OP_ISO = 4,
    OP_OFFSET = 5,
    OP_REBUILD = 6,
    OP_TRANSFORM = 7,
    OP_EQ = 8,
    OP_COPY = 9,
    OP_CLOSED = 10,
    OP_STEP_INV = 11,
    OP_TRIM = 12,
    OP_NORMAL = 13,
    OP_ISECT_PLANE = 14,
    OP_ISECT_SPHERE = 15,
    OP_ISECT_CONE = 16,
    OP_GLOBAL = 17,
    OP_IS_NURB = 18,
};

// Reads the protected members through pointers to members named in a derived class.
struct Peek : SmSphere
{
    static SmBoolean SmSurfOfRevolution::*planar() { return &Peek::m_bPlanarGenerator; }
    static SmPolarConversion SmSurfOfRevolution::*polar() { return &Peek::m_vPolarConverter; }
};

struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    bool flag() { return in[at++] != 0.0; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
    SmExtent2d domain()
    {
        double a = num();
        double b = num();
        double c = num();
        double d = num();
        return SmExtent2d(a, b, c, d);
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

void put_vec(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

// A sphere block `mode O[3] X[3] Y[3] radius swap inside_out has_dom D[4] has_circle
// [c[3] x[3] y[3] d0 d1 r]`: mode 0 is SmSphere::CreateCanonical(SmAxis2Placement(O, X, Y),
// radius) (the other fields are skipped), mode 1 the constructor with those arguments and
// MakeNurb (as CreateCanonical does). Writes the status; NULL on failure.
SmSphere* read_sphere(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    int64_t mode = rd.idx();
    SmPoint3d org = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    double radius = rd.num();
    bool swap = rd.flag();
    bool inside = rd.flag();
    bool has_dom = rd.flag();
    SmExtent2d dom = rd.domain();
    bool has_circle = rd.flag();
    SmCircle* circle = NULL;
    if (has_circle && mode == 1)
    {
        SmPoint3d c = rd.vec();
        SmVector3d cx = rd.vec();
        SmVector3d cy = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double r = rd.num();
        circle = new (ctx) SmCircle(c, cx, cy, SmExtent1d(d0, d1), r, 3, &ctx);
    }
    else
    {
        for (int i = 0; i < 12; i++)
            rd.num();
    }
    SmSphere* s = NULL;
    SmStatus stt = SM_SUCCESS;
    if (mode == 0)
    {
        stt = SmSphere::CreateCanonical(ctx, SmAxis2Placement(org, x, y), radius, s);
    }
    else
    {
        SmExtent2d d = has_dom ? dom : SmExtent2d(0.0, -90.0, 360.0, 90.0);
        s = new (ctx) SmSphere(org, x, y, d, radius, swap ? TRUE : FALSE, inside ? TRUE : FALSE,
                               circle);
        stt = s->MakeNurb();
        if (stt == SM_SUCCESS && s->GetGwNurbPointer() == NULL)
            stt = SM_ERR;
    }
    o.push_back(st(stt));
    if (stt != SM_SUCCESS || s == NULL)
    {
        delete s; // owns the circle once constructed
        return NULL;
    }
    return s;
}

// `has [type step0 step1 has_nurb [p n cpts(4 n) nk knots]] [ellipse O X Y rx ry inside]`.
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    o.push_back(flg(c != NULL));
    if (!c)
        return;
    o.push_back((double)c->GetType());
    SmExtent1d ivl = c->GetSTEPInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    const SmBSplineCurve* b =
        c->IsKindOf(SmBSplineCurve_TYPE) ? (const SmBSplineCurve*)c : NULL;
    const gw_CURVE* cur = b ? b->GetGwNurbPointer() : NULL;
    o.push_back(flg(cur != NULL));
    if (cur)
    {
        o.push_back((double)cur->p);
        o.push_back((double)(cur->pol->n + 1));
        for (gw_INDEX i = 0; i <= cur->pol->n; i++)
        {
            o.push_back(cur->pol->Pw[i].x);
            o.push_back(cur->pol->Pw[i].y);
            o.push_back(cur->pol->Pw[i].z);
            o.push_back(cur->pol->Pw[i].w);
        }
        o.push_back((double)(cur->knt->m + 1));
        for (gw_INDEX i = 0; i <= cur->knt->m; i++)
            o.push_back(cur->knt->U[i]);
    }
    const SmEllipse* e = c->IsKindOf(SmEllipse_TYPE) ? (const SmEllipse*)c : NULL;
    o.push_back(flg(e != NULL));
    if (e)
    {
        const SmAxis2Placement& p = e->GetPosition();
        put_vec(o, p.GetOriginRef());
        put_vec(o, p.GetXAxisRef());
        put_vec(o, p.GetYAxisRef());
        o.push_back(e->GetXRadius());
        o.push_back(e->GetYRadius());
        o.push_back(flg(e->GetInsideOut()));
    }
}

// The sphere state.
void put_state(std::vector<double>& o, const SmSphere* s)
{
    o.push_back(flg(s->IsKindOf(SmSphere_TYPE)));
    o.push_back(flg(s->IsKindOf(SmSurfOfRevolution_TYPE)));
    o.push_back(flg(s->IsKindOf(SmBSplineSurface_TYPE)));
    o.push_back(flg(s->IsKindOf(SmSurface_TYPE)));
    o.push_back((double)s->GetType());
    o.push_back(flg(s->GetSwapUV()));
    o.push_back(flg(s->*Peek::planar()));
    o.push_back(flg(s->GetInsideOut()));
    o.push_back(s->GetRadius());
    const SmAxis2Placement& pos = s->GetPosition();
    put_vec(o, pos.GetOriginRef());
    put_vec(o, pos.GetXAxisRef());
    put_vec(o, pos.GetYAxisRef());
    SmExtent2d d = s->GetSTEPUVDomain();
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
    SmExtent2d mx = s->GetMaxAnalyticDomain();
    o.push_back(mx.GetMin().x);
    o.push_back(mx.GetMin().y);
    o.push_back(mx.GetMax().x);
    o.push_back(mx.GetMax().y);
    const SmPolarConversion& pc = s->*Peek::polar();
    o.push_back(flg(s->HavePolarConversion()));
    ULONG cnt = pc.GetAngleCount();
    o.push_back((double)cnt);
    if (cnt > 0)
    {
        o.push_back(pc.GetFirstAngle());
        o.push_back(pc.GetLastAngle());
    }
    put_curve(o, pc.GetCurve());
    put_curve(o, s->GetGenCurve());
    const gw_SURFACE* sur = s->GetGwNurbPointer();
    o.push_back(flg(sur != NULL));
    if (!sur)
        return;
    o.push_back((double)sur->p);
    o.push_back((double)sur->q);
    o.push_back((double)(sur->net->n + 1));
    o.push_back((double)(sur->net->m + 1));
    for (gw_INDEX i = 0; i <= sur->net->n; i++)
        for (gw_INDEX j = 0; j <= sur->net->m; j++)
        {
            o.push_back(sur->net->Pw[i][j].x);
            o.push_back(sur->net->Pw[i][j].y);
            o.push_back(sur->net->Pw[i][j].z);
            o.push_back(sur->net->Pw[i][j].w);
        }
    o.push_back((double)(sur->knu->m + 1));
    for (gw_INDEX i = 0; i <= sur->knu->m; i++)
        o.push_back(sur->knu->U[i]);
    o.push_back((double)(sur->knv->m + 1));
    for (gw_INDEX i = 0; i <= sur->knv->m; i++)
        o.push_back(sur->knv->U[i]);
}

// A domain block `natural [d4]`: the surface's natural UV domain, or the given one.
SmExtent2d read_dom(Rd& rd, const SmSurface* s)
{
    bool natural = rd.flag();
    SmExtent2d d = rd.domain();
    return natural ? s->GetNaturalUVDomain() : d;
}

// The intersection record: `more ncurves curves ntypes types ndevs devs nuv1 nuv2 nnull1
// nnull2`.
void put_intersection(std::vector<double>& o, SmBoolean more, SmTArray<SmCurve*>& c3d,
                      SmTArray<SmCurve*>& uv1, SmTArray<SmCurve*>& uv2,
                      SmTArray<SmTsectCurveType>& types, SmTArray<double>& devs)
{
    o.push_back(flg(more));
    o.push_back((double)c3d.GetSize());
    for (ULONG i = 0; i < c3d.GetSize(); i++)
        put_curve(o, c3d[i]);
    o.push_back((double)types.GetSize());
    for (ULONG i = 0; i < types.GetSize(); i++)
        o.push_back((double)types[i]);
    o.push_back((double)devs.GetSize());
    for (ULONG i = 0; i < devs.GetSize(); i++)
        o.push_back(devs[i]);
    ULONG null1 = 0, null2 = 0;
    for (ULONG i = 0; i < uv1.GetSize(); i++)
        null1 += uv1[i] == NULL ? 1 : 0;
    for (ULONG i = 0; i < uv2.GetSize(); i++)
        null2 += uv2[i] == NULL ? 1 : 0;
    o.push_back((double)uv1.GetSize());
    o.push_back((double)uv2.GetSize());
    o.push_back((double)null1);
    o.push_back((double)null2);
}

// A plane block `kind`: 0 `O[3] X[3] Y[3] su sv D[4]` (SmPlane(O, X, Y, (su, sv), D)),
// 1 `O[3] N[3]` (SmPlane(O, N)).
SmPlane* read_plane(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 0)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double su = rd.num();
        double sv = rd.num();
        SmExtent2d d = rd.domain();
        return new (ctx) SmPlane(o, x, y, SmVector2d(su, sv), d, &ctx);
    }
    SmPoint3d o = rd.vec();
    SmVector3d n = rd.vec();
    return new (ctx) SmPlane(o, n, &ctx);
}

void sphere_op(int32_t op, Rd& rd, const SmContext& ctx, SmSphere* s, std::vector<double>& o)
{
    switch (op)
    {
    case OP_BUILD: put_state(o, s); break;
    case OP_EVAL_STEP:
    {
        double u = rd.num();
        double v = rd.num();
        ULONG k = (ULONG)rd.idx();
        bool point = rd.flag();
        if (point)
        {
            SmPoint3d p(0.0, 0.0, 0.0);
            SmStatus stt = s->EvaluateSTEPPoint(SmPoint2d(u, v), p);
            o.push_back(st(stt));
            if (stt == SM_SUCCESS)
                put_vec(o, p);
            break;
        }
        std::vector<SmVector3d> d((k + 1) * (k + 1), SmVector3d(0.0, 0.0, 0.0));
        SmStatus stt = s->EvaluateSTEP(SmPoint2d(u, v), k, k, TRUE, TRUE, TRUE, d.data());
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            for (size_t i = 0; i < d.size(); i++)
                put_vec(o, d[i]);
        break;
    }
    case OP_TO_STEP:
    case OP_TO_NURBS:
    {
        double u = rd.num();
        double v = rd.num();
        SmPoint2d r(0.0, 0.0);
        SmStatus stt = op == OP_TO_STEP ? s->ConvertUVFromNURBSToSTEP(SmPoint2d(u, v), r)
                                        : s->ConvertUVFromSTEPToNURBS(SmPoint2d(u, v), r);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(r.x);
            o.push_back(r.y);
        }
        break;
    }
    case OP_NORMAL:
    {
        double u = rd.num();
        double v = rd.num();
        SmVector3d n(0.0, 0.0, 0.0);
        SmStatus stt = s->EvaluateNormal(SmPoint2d(u, v), TRUE, TRUE, n);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_vec(o, n);
        break;
    }
    case OP_ISO:
    {
        SmSurfParamType param = rd.idx() == 0 ? SM_SP_U : SM_SP_V;
        double iso = rd.num();
        bool has_dom = rd.flag();
        SmExtent2d dom = rd.domain();
        // the override ignores the status of the base extraction and then reads its
        // curve: reject the arguments it fails on
        SmBSplineCurve* probe = NULL;
        SmStatus pst = s->SmBSplineSurface::CreateIsoParametricCurve(
            ctx, param, iso, 0.0, probe, has_dom ? &dom : NULL, NULL, NULL);
        bool ok = pst == SM_SUCCESS && probe != NULL;
        delete probe;
        if (!ok)
        {
            o.push_back((double)SC_ERR_ARG);
            break;
        }
        SmBSplineCurve* c = NULL;
        double gap = -1.0;
        SmStatus stt = s->CreateIsoParametricCurve(ctx, param, iso, 0.0, c,
                                                   has_dom ? &dom : NULL, &gap, NULL);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(gap);
            put_curve(o, c);
        }
        delete c;
        break;
    }
    case OP_OFFSET:
    {
        double dist = rd.num();
        double tol = rd.num();
        bool rebuild = rd.flag();
        SmSurface* off = NULL;
        SmStatus stt = s->CreateOffsetSurface(ctx, dist, tol, off);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(flg(off != NULL));
            if (off)
            {
                SmSphere* t = (SmSphere*)off;
                if (rebuild)
                    o.push_back(st(t->RebuildSTEPFromNURBParameters()));
                put_state(o, t);
            }
        }
        delete off;
        break;
    }
    case OP_REBUILD:
    {
        SmStatus stt = s->RebuildSTEPFromNURBParameters();
        o.push_back(st(stt));
        put_state(o, s);
        break;
    }
    case OP_TRANSFORM:
    {
        SmPoint3d org = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        bool has_scale = rd.flag();
        SmVector3d sc = rd.vec();
        SmAxis2Placement a2p(org, x, y);
        SmStatus stt = s->Transform(a2p, has_scale ? &sc : NULL);
        o.push_back(st(stt));
        put_state(o, s);
        break;
    }
    case OP_EQ:
    {
        std::vector<double> o2;
        SmSphere* other = read_sphere(rd, ctx, o2);
        o.push_back(o2.empty() ? -1.0 : o2[0]);
        if (other)
        {
            o.push_back(flg(*s == *other));
            o.push_back(flg(*other == *s));
            delete other;
        }
        break;
    }
    case OP_COPY:
    {
        SmSurface* c = NULL;
        SmStatus stt = s->Copy(ctx, c);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS && c)
        {
            put_state(o, (SmSphere*)c);
            o.push_back(flg(*s == *c));
        }
        delete c;
        break;
    }
    case OP_CLOSED:
    {
        int64_t p = rd.idx();
        SmSurfParamType param = p == 0 ? SM_SP_U : (p == 1 ? SM_SP_V : SM_SP_BOTH);
        SmExtent2d dom = read_dom(rd, s);
        SmContinuityType cont = SM_CT_UNDEFINED;
        SmBoolean closed = s->IsClosed(dom, param, NULL, &cont);
        o.push_back(flg(closed));
        o.push_back((double)cont);
        o.push_back(flg(s->IsClosedU()));
        o.push_back(flg(s->IsClosedV()));
        break;
    }
    case OP_STEP_INV:
    {
        bool natural = rd.flag();
        SmExtent2d given = rd.domain();
        SmExtent2d dom = natural ? s->GetSTEPUVDomain() : given;
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        bool has_guess = rd.flag();
        double gx = rd.num();
        double gy = rd.num();
        SmPoint2d guess(gx, gy);
        SmPoint2d uv(0.0, 0.0);
        SmLocationType loc = SM_LT_EXTERIOR;
        SmStatus stt = s->STEPInversion(dom, p, tol, uv, loc, has_guess ? &guess : NULL);
        o.push_back(st(stt));
        o.push_back(uv.x);
        o.push_back(uv.y);
        o.push_back((double)loc);
        break;
    }
    case OP_TRIM:
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double tol = rd.num();
        SmTArray<SmCurve*> curves;
        SmStatus stt = s->CreateTrimmedCircleSegments(ctx, c, x, y, SmExtent1d(d0, d1), tol, curves);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back((double)curves.GetSize());
            for (ULONG i = 0; i < curves.GetSize(); i++)
                put_curve(o, curves[i]);
        }
        for (ULONG i = 0; i < curves.GetSize(); i++)
            delete curves[i];
        break;
    }
    case OP_ISECT_PLANE:
    case OP_ISECT_SPHERE:
    case OP_ISECT_CONE:
    case OP_GLOBAL:
    {
        SmSurface* other = NULL;
        std::vector<double> o2;
        if (op == OP_ISECT_SPHERE)
            other = read_sphere(rd, ctx, o2);
        else if (op == OP_ISECT_CONE)
        {
            SmPoint3d po = rd.vec();
            SmVector3d px = rd.vec();
            SmVector3d py = rd.vec();
            double r0 = rd.num();
            double r1 = rd.num();
            double h = rd.num();
            SmCone* c = NULL;
            SmStatus cs = SmCone::CreateCanonical(ctx, SmAxis2Placement(po, px, py), r0, r1, h, c);
            o2.push_back(cs == SM_SUCCESS && c ? 0.0 : (double)(cs == SM_SUCCESS ? SM_ERR : cs));
            if (cs != SM_SUCCESS)
            {
                delete c;
                c = NULL;
            }
            other = c;
        }
        else
        {
            other = read_plane(rd, ctx);
            o2.push_back(0.0);
        }
        o.push_back(o2.empty() ? -1.0 : o2[0]);
        if (!other)
            break;
        SmExtent2d dom = read_dom(rd, s);
        SmExtent2d odom = read_dom(rd, other);
        bool has_tol = rd.flag();
        SmApproxTol3d tol = rd.num();
        SmBoolean edges[2] = {TRUE, TRUE};
        SmBoolean more = FALSE;
        SmTArray<SmCurve*> c3d, uv1, uv2;
        SmTArray<SmTsectCurveType> types;
        SmTArray<double> devs;
        SmStatus stt = SM_ERR;
        if (op == OP_ISECT_PLANE)
            stt = s->IntersectWithPlane(ctx, dom, *(SmPlane*)other, odom, edges,
                                        has_tol ? &tol : NULL, NULL, more, &c3d, &uv1, &uv2,
                                        &types, &devs);
        else if (op == OP_ISECT_SPHERE)
            stt = s->IntersectWithSphere(ctx, dom, *(SmSphere*)other, odom, edges,
                                         has_tol ? &tol : NULL, NULL, more, &c3d, &uv1, &uv2,
                                         &types, &devs);
        else if (op == OP_ISECT_CONE)
            stt = s->IntersectWithCone(ctx, dom, *(SmCone*)other, odom, edges,
                                       has_tol ? &tol : NULL, NULL, more, &c3d, &uv1, &uv2,
                                       &types, &devs);
        else
            stt = s->GlobalSurfaceIntersect(ctx, dom, *other, odom, edges, has_tol ? &tol : NULL,
                                            NULL, &c3d, &uv1, &uv2, &types, &devs);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_intersection(o, more, c3d, uv1, uv2, types, devs);
        for (ULONG i = 0; i < c3d.GetSize(); i++)
            delete c3d[i];
        delete other;
        break;
    }
    case OP_IS_NURB:
    {
        double scale = rd.num();
        double sx = rd.num();
        double sy = rd.num();
        double sz = rd.num();
        int64_t npert = rd.idx();
        const gw_SURFACE* src = s->GetGwNurbPointer();
        gw_SURFACE* g = sm_AllocateAndCopyNurbSurface((gw_SURFACE*)src);
        for (gw_INDEX i = 0; i <= g->net->n; i++)
            for (gw_INDEX j = 0; j <= g->net->m; j++)
            {
                g->net->Pw[i][j].x *= sx;
                g->net->Pw[i][j].y *= sy;
                g->net->Pw[i][j].z *= sz;
            }
        for (int64_t k = 0; k < npert; k++)
        {
            int64_t i = rd.idx();
            int64_t j = rd.idx();
            double dx = rd.num();
            double dy = rd.num();
            double dz = rd.num();
            if (i < 0 || j < 0 || i > g->net->n || j > g->net->m)
                continue;
            g->net->Pw[i][j].x += dx;
            g->net->Pw[i][j].y += dy;
            g->net->Pw[i][j].z += dz;
        }
        SmBSplineSurface* test = new (ctx) SmBSplineSurface(g, FALSE, NULL);
        SmSphere* r = NULL;
        // SmBoolean is an int: a failed `SER(..)` returns its status as a truthy "found" with
        // no sphere, so the value is recorded as is.
        SmBoolean found = SmSphere::IsNurbSurfaceSphere(ctx, test, r, scale);
        o.push_back((double)found);
        o.push_back(flg(r != NULL));
        if (r)
            put_state(o, r);
        delete r;
        delete test;
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_surface_sphere_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                         int64_t cap, int64_t* out_len)
{
    if (in == nullptr || n_in < 0 || out_len == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    SmSphere* s = read_sphere(rd, ctx, o);
    if (s)
    {
        sphere_op(op, rd, ctx, s, o);
        delete s;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
