// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surf_of_revolution_p2`.
// Wraps the SmSurfOfRevolution members owned by the unit (source/SMLib/src/SmSurfOfRevolution.cpp):
// AdjustSTEPUVDomain, STEPInversion, CreateOffsetSurface, GlobalSurfaceIntersect,
// IsNurbSurfaceSurfOfRevolution and TrimWithDomain (CreateCanonical and MakeNurb are built by
// the surface block, as in surface_surf_of_revolution_p1.cpp).
//
// One entry point, sc_surface_surf_of_revolution_p2_eval(op, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/surface_surf_of_revolution_p2.rs,
// which validates every input so upstream only touches valid memory. The surface block and
// the state record are those of surface_surf_of_revolution_p1.cpp. `out` starts with the
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

#include <vector>

#include "sc_sm.h"

using sc::flg;
using sc::put_vec;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_surf_of_revolution_p2.rs).
enum : int32_t
{
    OP_BUILD = 0,
    OP_ADJUST = 1,
    OP_STEP_INV = 2,
    OP_OFFSET = 3,
    OP_GSSI = 4,
    OP_TRIM = 5,
    OP_IS_NURB = 20,
};

// The other operand of OP_GSSI.
enum : int32_t
{
    OKP_REVOLUTION = 0, // a surface block
    OKP_PLANE = 1,      // SmPlane::CreateCanonical(A2P)
};

// Generator kinds.
enum : int32_t
{
    GK_BSPLINE = 0,
    GK_LINE = 1,
    GK_CIRCLE = 2,
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

// A generator block `kind ...`: a B-spline curve, a line `a[3] b[3]` (its NURB made) or a
// circle `c[3] x[3] y[3] d0 d1 r`.
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
    default: return NULL;
    }
}

// A surface block `mode gen oob swap` then for BM_CANONICAL `P[3] A[3]`, for BM_CTOR
// `O[3] X[3] Y[3] D[4]`. Writes the construction status; NULL on failure.
SmSurfOfRevolution* read_surface(Rd& rd, const SmContext& ctx, std::vector<double>& o)
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

// The surface state (the layout of surface_surf_of_revolution_p1.cpp).
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

// A domain block `natural [d4]`: the surface's natural UV domain, or the given one.
SmExtent2d read_dom(Rd& rd, const SmSurface* s)
{
    bool natural = rd.flag();
    SmExtent2d d = rd.domain();
    return natural ? s->GetNaturalUVDomain() : d;
}

// The intersection record: `more ncurves curves types devs nuv1 nuv2 nnull1 nnull2`.
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


void gssi(Rd& rd, const SmContext& ctx, SmSurfOfRevolution* s, std::vector<double>& o)
{
    int64_t kind = rd.idx();
    SmSurface* other = NULL;
    std::vector<double> o2;
    if (kind == OKP_REVOLUTION)
    {
        bool self = rd.flag();
        if (self)
        {
            o2.push_back(0.0);
            other = s;
        }
        else
            other = read_surface(rd, ctx, o2);
    }
    else
    {
        SmPoint3d po = rd.vec();
        SmVector3d px = rd.vec();
        SmVector3d py = rd.vec();
        SmPlane* plane = NULL;
        SmStatus cs = SmPlane::CreateCanonical(ctx, SmAxis2Placement(po, px, py), plane);
        other = plane;
        o2.push_back(cs == SM_SUCCESS && other ? 0.0 : (double)(cs == SM_SUCCESS ? SM_ERR : cs));
        if (cs != SM_SUCCESS)
        {
            delete other;
            other = NULL;
        }
    }
    o.push_back(o2.empty() ? -1.0 : o2[0]);
    if (!other)
        return;
    SmExtent2d dom = read_dom(rd, s);
    SmExtent2d odom = read_dom(rd, other);
    bool has_tol = rd.flag();
    SmApproxTol3d tol = rd.num();
    SmBoolean edges[2] = {TRUE, TRUE};
    SmTArray<SmCurve*> c3d, uv1, uv2;
    SmTArray<SmTsectCurveType> types;
    SmTArray<double> devs;
    SmStatus stt = s->GlobalSurfaceIntersect(ctx, dom, *other, odom, edges,
                                             has_tol ? &tol : NULL, NULL, &c3d, &uv1, &uv2,
                                             &types, &devs);
    o.push_back(st(stt));
    if (stt == SM_SUCCESS)
        put_intersection(o, FALSE, c3d, uv1, uv2, types, devs);
    for (ULONG i = 0; i < c3d.GetSize(); i++)
        delete c3d[i];
    for (ULONG i = 0; i < uv1.GetSize(); i++)
        delete uv1[i];
    for (ULONG i = 0; i < uv2.GetSize(); i++)
        delete uv2[i];
    if (other != s)
        delete other;
}

void surface_op(int32_t op, Rd& rd, const SmContext& ctx, SmSurfOfRevolution* s,
                std::vector<double>& o)
{
    switch (op)
    {
    case OP_BUILD: put_state(o, s); break;
    case OP_ADJUST:
    {
        SmExtent2d d = rd.domain();
        SmStatus stt = s->AdjustSTEPUVDomain(d);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_state(o, s);
        break;
    }
    case OP_TRIM:
    {
        SmExtent2d d = read_dom(rd, s);
        SmStatus stt = s->TrimWithDomain(d);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_state(o, s);
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
        if (stt == SM_SUCCESS)
        {
            o.push_back(uv.x);
            o.push_back(uv.y);
            o.push_back((double)loc);
        }
        break;
    }
    case OP_OFFSET:
    {
        double dist = rd.num();
        SmApproxTol3d tol = rd.num();
        SmSurface* off = NULL;
        SmStatus stt = s->CreateOffsetSurface(ctx, dist, tol, off);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS && off)
            put_state(o, (SmSurfOfRevolution*)off);
        delete off;
        break;
    }
    case OP_GSSI: gssi(rd, ctx, s, o); break;
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}

// The static IsNurbSurfaceSurfOfRevolution: `mode scale` then for 0 a B-spline surface block,
// for 1 a surface-of-revolution block. Writes `found` then the state of the new surface.
void is_nurb(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    int64_t mode = rd.idx();
    double scale = rd.num();
    SmBSplineSurface* test = NULL;
    if (mode == 0)
        test = read_bspline_surface(rd, ctx);
    else
    {
        std::vector<double> o2;
        test = read_surface(rd, ctx, o2);
        o.push_back(o2.empty() ? -1.0 : o2[0]);
        if (!test)
            return;
    }
    SmSurfOfRevolution* rev = NULL;
    SmBoolean found = SmSurfOfRevolution::IsNurbSurfaceSurfOfRevolution(ctx, test, rev, scale);
    // SmBoolean is an int: a failed `SER(sm_TestForIsoCircles(..))` returns SM_ERR (1001) as a
    // truthy "found" with no surface, so the value is recorded as is.
    o.push_back((double)found);
    if (found && rev)
        put_state(o, rev);
    delete rev;
    delete test;
}
} // namespace

SC_EXPORT int32_t sc_surface_surf_of_revolution_p2_eval(int32_t op, const double* in,
                                                        int64_t n_in, double* out, int64_t cap,
                                                        int64_t* out_len)
{
    if (in == nullptr || n_in < 0 || out_len == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    if (op == OP_IS_NURB)
    {
        is_nurb(rd, ctx, o);
    }
    else
    {
        SmSurfOfRevolution* s = read_surface(rd, ctx, o);
        if (s)
        {
            surface_op(op, rd, ctx, s, o);
            delete s;
        }
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
