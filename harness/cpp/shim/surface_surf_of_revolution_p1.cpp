// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surf_of_revolution_p1`.
// Wraps the SmSurfOfRevolution members owned by the unit (source/SMLib/src/SmSurfOfRevolution.cpp):
// the constructor (through a derived class: it is protected) and CreateCanonical with
// MakeNurb, operator==, CreateIsoParametricCurve, CreateDirectrixAtGeneratorParam,
// CreateGeneratorFromAngle, ConvertUVFromNURBSToSTEP, EvaluateSTEP, EvaluateSTEPPoint,
// IntersectWithPlane, IsBounded, GetMaxAnalyticDomain, GetGenDirParamExtent,
// TransformPointToStartPlane, IsKindOf and sm_TestForIsoCircles.
//
// One entry point, sc_surface_surf_of_revolution_p1_eval(op, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/surface_surf_of_revolution_p1.rs,
// which validates every input so upstream only touches valid memory. `out` starts with the
// status of the surface construction (or of the static op).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmHyperbola.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmSurfOfRevolution.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <cmath>
#include <stdexcept>
#include <vector>

#include "sc_const_make.h"
#include "sc_sm.h"

using sc::flg;
using sc::put_vec;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_surf_of_revolution_p1.rs).
enum : int32_t
{
    OP_BUILD = 0,
    OP_EVAL_STEP = 1,
    OP_TO_STEP = 2,
    OP_ISO = 3,
    OP_DIRECTRIX = 4,
    OP_GENERATOR = 5,
    OP_START_PLANE = 6,
    OP_EQ = 7,
    OP_PLANE = 8,
    OP_ISO_CIRCLES = 20,
    OP_CONST_MAKE = 21, // the BM_CTOR constructor with no MakeNurb, then sc::put_const_make
};

// Generator kinds.
enum : int32_t
{
    GK_BSPLINE = 0,
    GK_LINE = 1,
    GK_CIRCLE = 2,
    GK_HYPERBOLA = 3,
};

// Construction modes.
enum : int32_t
{
    BM_CANONICAL = 0, // CreateCanonical(curve, axis point, axis direction, swap)
    BM_CTOR = 1,      // SmSurfOfRevolution(curve, origin, x, y, domain, swap) + MakeNurb
};

// Reads the protected members through pointers to members named in a derived class, and
// reaches the protected constructor.
struct Peek : SmSurfOfRevolution
{
    static SmBoolean SmSurfOfRevolution::*planar() { return &Peek::m_bPlanarGenerator; }
    static SmPolarConversion SmSurfOfRevolution::*polar() { return &Peek::m_vPolarConverter; }
    Peek(SmBSplineCurve* c, const SmPoint3d& o, const SmVector3d& x, const SmVector3d& y,
         const SmExtent2d& d, SmBoolean swap)
        : SmSurfOfRevolution(c, o, x, y, d, swap)
    {
    }
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

// `n p m`, `n+1` points `x y z w`, `m+1` knots: SmBSplineCurve(gw_CURVE*, 3, FALSE).
SmBSplineCurve* read_bspline(Rd& rd, const SmContext& ctx)
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
    return new (ctx) SmBSplineCurve(cur, 3, FALSE);
}

// An `SmHyperbola` whose constructor made a finite NURB. The constructor ignores a
// failing `MakeNurb` (`SE`), leaving `m_pNurb` NULL or NaN for semi axes too large or too
// small next to the centre's coordinates, which every later use dereferences (the FFI
// cannot predict the rounding): such an input is refused as a harness error.
SmBSplineCurve* checked_hyperbola(SmHyperbola* h)
{
    const gw_CURVE* cur = h->GetGwNurbPointer();
    bool ok = cur != NULL && cur->pol != NULL;
    for (gw_INDEX i = 0; ok && i <= cur->pol->n; i++)
    {
        const gw_CPOINT& p = cur->pol->Pw[i];
        ok = std::isfinite(p.x) && std::isfinite(p.y) && std::isfinite(p.z) && std::isfinite(p.w);
    }
    if (!ok)
    {
        delete h;
        throw std::invalid_argument("hyperbola generator: no finite NURB");
    }
    return h;
}

// A generator block `kind ...`: a B-spline curve, a line `a[3] b[3]` (its NURB made), a
// circle `c[3] x[3] y[3] d0 d1 r` or a hyperbola `c[3] x[3] y[3] d0 d1 a b`.
SmBSplineCurve* read_gen(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    switch (kind)
    {
    case GK_BSPLINE: return read_bspline(rd, ctx);
    case GK_LINE:
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine* l = new (ctx) SmLine(a, b, 3, &ctx);
        l->GetOrCreateGwNurbPointer();
        return l;
    }
    case GK_CIRCLE:
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double r = rd.num();
        return new (ctx) SmCircle(c, x, y, SmExtent1d(d0, d1), r, 3, &ctx);
    }
    case GK_HYPERBOLA:
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double a = rd.num();
        double b = rd.num();
        return checked_hyperbola(new (ctx) SmHyperbola(c, x, y, SmExtent1d(d0, d1), a, b, 3, &ctx));
    }
    default: return NULL;
    }
}

// A surface block `mode gen oob swap` then for BM_CANONICAL `P[3] A[3]`, for BM_CTOR
// `O[3] X[3] Y[3] D[4]`. Writes the construction status; NULL on failure.
SmSurfOfRevolution* read_surface(Rd& rd, const SmContext& ctx, std::vector<double>& o,
                                 bool lazy = false)
{
    int64_t mode = rd.idx();
    SmBSplineCurve* gen = read_gen(rd, ctx);
    bool oob = rd.flag();
    bool swap = rd.flag();
    SmSurfOfRevolution* s = NULL;
    SmStatus stt = SM_SUCCESS;
    if (mode == BM_CANONICAL)
    {
        SmPoint3d p = rd.vec();
        SmVector3d a = rd.vec();
        stt = SmSurfOfRevolution::CreateCanonical(ctx, gen, p, a, s, swap ? TRUE : FALSE);
    }
    else
    {
        SmPoint3d org = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        SmExtent2d d = rd.domain();
        s = new (ctx) Peek(gen, org, x, y, d, swap ? TRUE : FALSE);
        if (!lazy)
            stt = s->MakeNurb();
    }
    o.push_back(st(stt));
    if (stt != SM_SUCCESS)
    {
        if (s)
            delete s; // owns gen
        else
            delete gen;
        return NULL;
    }
    s->SetOutOfBoundsEnabled(oob ? TRUE : FALSE); // the surface and its generator
    return s;
}

// `has [type step0 step1 has_nurb [p n cpts(4 n) nk knots]]`.
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
    if (!cur)
        return;
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

// The surface state.
void put_state(std::vector<double>& o, const SmSurfOfRevolution* s)
{
    o.push_back(flg(s->IsKindOf(SmSurfOfRevolution_TYPE)));
    o.push_back(flg(s->IsKindOf(SmBSplineSurface_TYPE)));
    o.push_back(flg(s->IsKindOf(SmSurface_TYPE)));
    o.push_back(flg(s->IsBounded()));
    o.push_back(flg(s->GetSwapUV()));
    o.push_back(flg(s->*Peek::planar()));
    const SmAxis2Placement& pos = s->GetPosition();
    put_vec(o, pos.GetOriginRef());
    put_vec(o, pos.GetXAxisRef());
    put_vec(o, pos.GetYAxisRef());
    SmExtent2d d = s->GetSTEPUVDomain();
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
    o.push_back(s->GetStartAngleDeg());
    o.push_back(s->GetEndAngleDeg());
    SmExtent2d mx = s->GetMaxAnalyticDomain();
    o.push_back(mx.GetMin().x);
    o.push_back(mx.GetMin().y);
    o.push_back(mx.GetMax().x);
    o.push_back(mx.GetMax().y);
    SmSurfParamType gp = SM_SP_UNKNOWN;
    SmExtent1d gd = s->GetGenDirParamExtent(&gp);
    o.push_back(gd.GetMin());
    o.push_back(gd.GetMax());
    o.push_back((double)gp);
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

void surface_op(int32_t op, Rd& rd, const SmContext& ctx, SmSurfOfRevolution* s,
                std::vector<double>& o)
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
    {
        double u = rd.num();
        double v = rd.num();
        SmPoint2d r;
        SmStatus stt = s->ConvertUVFromNURBSToSTEP(SmPoint2d(u, v), r);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(r.x);
            o.push_back(r.y);
        }
        break;
    }
    case OP_ISO:
    {
        SmSurfParamType param = rd.idx() == 0 ? SM_SP_U : SM_SP_V;
        double iso = rd.num();
        bool has_dom = rd.flag();
        SmExtent2d dom = rd.domain();
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
    case OP_DIRECTRIX:
    {
        double t = rd.num();
        SmBSplineCurve* c = NULL;
        double radius = 0.0;
        SmPoint3d axis(0.0, 0.0, 0.0);
        SmStatus stt = s->CreateDirectrixAtGeneratorParam(ctx, t, c, radius, axis);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(radius);
            put_vec(o, axis);
            put_curve(o, c);
        }
        delete c;
        break;
    }
    case OP_GENERATOR:
    {
        double ang = rd.num();
        SmBSplineCurve* c = NULL;
        SmStatus stt = s->CreateGeneratorFromAngle(ctx, ang, c);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_curve(o, c);
        delete c;
        break;
    }
    case OP_START_PLANE:
    {
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        bool snap = rd.flag();
        SmPoint3d q(0.0, 0.0, 0.0);
        ULONG n = 0;
        double ang[2] = {0.0, 0.0};
        SmBoolean inside = FALSE;
        SmStatus stt =
            s->TransformPointToStartPlane(p, tol, q, n, ang, inside, snap ? TRUE : FALSE);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            put_vec(o, q);
            o.push_back((double)n);
            for (ULONG i = 0; i < n && i < 2; i++)
                o.push_back(ang[i]);
            o.push_back(flg(inside));
        }
        break;
    }
    case OP_EQ:
    {
        std::vector<double> o2;
        SmSurfOfRevolution* other = read_surface(rd, ctx, o2);
        o.push_back(o2.empty() ? -1.0 : o2[0]);
        if (other)
        {
            o.push_back(flg(*s == *other));
            o.push_back(flg(*other == *s));
            delete other;
        }
        break;
    }
    case OP_PLANE:
    {
        SmPoint3d po = rd.vec();
        SmVector3d px = rd.vec();
        SmVector3d py = rd.vec();
        SmExtent2d pdom = rd.domain();
        bool has_tol = rd.flag();
        SmApproxTol3d tol = rd.num();
        SmPlane* plane = NULL;
        SmStatus pst = SmPlane::CreateCanonical(ctx, SmAxis2Placement(po, px, py), plane);
        if (pst != SM_SUCCESS || plane == NULL)
        {
            o.push_back((double)SC_ERR_ARG);
            break;
        }
        SmBoolean edges[2] = {TRUE, TRUE};
        SmBoolean more = FALSE;
        SmTArray<SmCurve*> c3d;
        SmTArray<SmTsectCurveType> types;
        SmTArray<double> devs;
        SmStatus stt = s->IntersectWithPlane(ctx, s->GetNaturalUVDomain(), *plane, pdom, edges,
                                             has_tol ? &tol : NULL, NULL, more, &c3d, NULL,
                                             NULL, &types, &devs);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(flg(more));
            o.push_back((double)c3d.GetSize());
            for (ULONG i = 0; i < c3d.GetSize(); i++)
                put_curve(o, c3d[i]);
            for (ULONG i = 0; i < types.GetSize(); i++)
                o.push_back((double)types[i]);
            for (ULONG i = 0; i < devs.GetSize(); i++)
                o.push_back(devs[i]);
        }
        for (ULONG i = 0; i < c3d.GetSize(); i++)
            delete c3d[i];
        delete plane;
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}

// `n m p q`, `(n+1)(m+1)` points `x y z w` (v index fastest), `n+p+2` u knots, `m+q+2` v
// knots: SmBSplineSurface(gw_SURFACE*, FALSE, NULL).
SmBSplineSurface* read_bspline_surface(Rd& rd, const SmContext& ctx)
{
    gw_INDEX n = (gw_INDEX)rd.idx();
    gw_INDEX m = (gw_INDEX)rd.idx();
    gw_DEGREE p = (gw_DEGREE)rd.idx();
    gw_DEGREE q = (gw_DEGREE)rd.idx();
    gw_INDEX r = n + p + 1;
    gw_INDEX s = m + q + 1;
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

void iso_circles(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmBSplineSurface* srf = read_bspline_surface(rd, ctx);
    SmSurfParamType param = rd.idx() == 0 ? SM_SP_U : SM_SP_V;
    double tol = rd.num();
    double ang = rd.num();
    const double nan = std::nan("");
    SmBoolean found = FALSE, par = FALSE, same = FALSE, coax = FALSE, common = FALSE,
              planar = FALSE;
    double radius = nan;
    SmExtent1d angles(nan, nan);
    SmAxis2Placement frame(SmPoint3d(nan, nan, nan), SmVector3d(nan, nan, nan),
                           SmVector3d(nan, nan, nan));
    SmPoint3d lp(nan, nan, nan);
    SmVector3d lv(nan, nan, nan);
    SmStatus stt = sm_TestForIsoCircles(srf, param, tol, ang, found, par, same, coax, common,
                                        planar, radius, angles, frame, lp, lv);
    o.push_back(st(stt));
    if (stt == SM_SUCCESS)
    {
        for (SmBoolean b : {found, par, same, coax, common, planar})
            o.push_back(flg(b));
        o.push_back(radius);
        o.push_back(angles.GetMin());
        o.push_back(angles.GetMax());
        put_vec(o, frame.GetOriginRef());
        put_vec(o, frame.GetXAxisRef());
        put_vec(o, frame.GetYAxisRef());
        put_vec(o, lp);
        put_vec(o, lv);
    }
    delete srf;
}
} // namespace

SC_EXPORT int32_t sc_surface_surf_of_revolution_p1_eval(int32_t op, const double* in,
                                                        int64_t n_in, double* out, int64_t cap,
                                                        int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    if (op == OP_ISO_CIRCLES)
    {
        iso_circles(rd, ctx, o);
    }
    else
    {
        SmSurfOfRevolution* s = read_surface(rd, ctx, o, op == OP_CONST_MAKE);
        if (s)
        {
            if (op == OP_CONST_MAKE)
                sc::put_const_make(ctx, s, o);
            else
                surface_op(op, rd, ctx, s, o);
            delete s;
        }
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
