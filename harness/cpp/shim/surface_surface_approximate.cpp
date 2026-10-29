// SPDX-License-Identifier: Apache-2.0
// Unit surface.surface_approximate. Builds a real SmBSplineSurface, SmPlane, SmCylinder,
// SmCone, SmSphere, SmTorus or SmSurfOfRevolution from plain data, optionally wraps it in
// up to three SmOffsetSurface, SmCurveBoundedSurface, SmSTEPSurface or SmSrfInVolume
// objects, and runs SmSurface::ApproximateSurface on it.
//
// Input block: `tol level just_copy n_wraps wraps... kind` then the kind's data, as in
// surface_surface_assert_valid.cpp:
//   0 B-spline: p q nu nv rational knots_u[nu+p+1] knots_v[nv+q+1] pts[nu*nv*4] (row i = u)
//   1 plane:    O[3] X[3] Y[3] scale[2] domain[4]
//   2 cylinder: O X Y r start end h swap inside mngc
//   3 cone:     O X Y bot top start end h swap inside mngc
//   4 sphere:   O X Y domain[4] r swap inside
//   5 torus:    O X Y domain[4] major minor swap inside
//   6 revolution: O X Y domain[4] swap, generator: p n rational pts[n*4] knots[n+p+1]
// Wraps are applied innermost first, each owning its base: `0 d` SmOffsetSurface(d, base,
// TRUE, 0); `1 implicit` SmCurveBoundedSurface without curves; `2` SmSTEPSurface(base,
// TRUE); `3 O[3] X[3] Y[3] Z[3]` SmSrfInVolume(base, FALSE, SmTransform(O, X, Y, &Z), 3, 0)
// (both parts copied; an original is freed only when it was copied: a curve bounded or
// STEP base has no Copy, so the object keeps and owns the original).
// `tol` is passed as an SmApproxTol3d (fabs, SM_USE_DEFAULT = half the zone tolerance).
//
// Output: `status achieved has` then, when a surface came back, its `type` and its
// SmBSplineSurface part: `deg_u deg_v n_u n_v`, per direction `count knots[count]
// mults[count]`, then the control points `x y z w` (SM_CP_EUCLIDIAN_RATIONAL, u major).
#include "sc_shim.h"
#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmCone.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveBoundedSurface.h>
#include <SmCylinder.h>
#include <SmOffsetSurface.h>
#include <SmPlane.h>
#include <SmSTEPSurface.h>
#include <SmSphere.h>
#include <SmSrfInVolume.h>
#include <SmSurfOfRevolution.h>
#include <SmTArray.h>
#include <SmTol.h>
#include <SmTorus.h>
#include <SmTransform.h>
#include <nurbs.h>
#include <vector>

namespace
{
struct ApConePeek : SmCone
{
    ApConePeek(const SmPoint3d& o, const SmVector3d& x, const SmVector3d& y, double bot, double top,
               double start, double end, double h, SmBoolean swap, SmBoolean inside, SmBoolean mngc)
        : SmCone(o, x, y, bot, top, start, end, h, swap, inside, mngc)
    {
    }
};

struct ApRevolution : SmSurfOfRevolution
{
    ApRevolution(SmBSplineCurve* gen, const SmPoint3d& o, const SmVector3d& x, const SmVector3d& y,
                 const SmExtent2d& d, SmBoolean swap)
        : SmSurfOfRevolution(gen, o, x, y, d, swap)
    {
    }
};

struct ApRd
{
    const double* in;
    int64_t n;
    int64_t at;
    bool ok = true;
    double num()
    {
        if (at >= n)
        {
            ok = false;
            return 0.0;
        }
        return in[at++];
    }
    int64_t idx() { return (int64_t)num(); }
    SmBoolean b() { return num() != 0.0 ? TRUE : FALSE; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
    SmExtent2d dom()
    {
        double a = num();
        double b2 = num();
        double c = num();
        double d = num();
        return SmExtent2d(a, b2, c, d);
    }
};

gw_CPOINT ap_point(ApRd& rd, bool rational)
{
    gw_CPOINT p;
    p.x = rd.num();
    p.y = rd.num();
    p.z = rd.num();
    double w = rd.num();
    p.w = rational ? w : NL_NOW;
    return p;
}

// The surface of kind `kind` read from `rd`; NULL on a bad block.
SmSurface* ap_build(ApRd& rd, SmContext& ctx, int64_t kind)
{
    if (kind == 0)
    {
        int64_t p = rd.idx(), q = rd.idx(), nu = rd.idx(), nv = rd.idx();
        bool rational = rd.num() != 0.0;
        if (p < 1 || q < 1 || nu < 2 || nv < 2 || nu > 64 || nv > 64)
            return NULL;
        std::vector<double> ku(nu + p + 1), kv(nv + q + 1);
        for (auto& v : ku)
            v = rd.num();
        for (auto& v : kv)
            v = rd.num();
        std::vector<gw_CPOINT> pts(nu * nv);
        for (auto& c : pts)
            c = ap_point(rd, rational);
        if (!rd.ok)
            return NULL;
        std::vector<gw_CPOINT*> rows(nu);
        for (int64_t i = 0; i < nu; i++)
            rows[i] = &pts[i * nv];
        return new (ctx) SmBSplineSurface((ULONG)(nu - 1), (ULONG)(nv - 1), (short)p, (short)q,
                                          (ULONG)(nu + p), (ULONG)(nv + q), rows.data(), ku.data(),
                                          kv.data());
    }
    if (kind < 1 || kind > 6)
        return NULL;
    SmPoint3d o = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    if (kind == 1)
    {
        double sx = rd.num(), sy = rd.num();
        SmExtent2d d = rd.dom();
        if (!rd.ok)
            return NULL;
        return new (ctx) SmPlane(o, x, y, SmVector2d(sx, sy), d, &ctx);
    }
    if (kind == 2)
    {
        double r = rd.num(), s0 = rd.num(), s1 = rd.num(), h = rd.num();
        SmBoolean swap = rd.b(), inside = rd.b(), mngc = rd.b();
        if (!rd.ok)
            return NULL;
        return new (ctx) SmCylinder(o, x, y, r, s0, s1, h, swap, inside, mngc, &ctx);
    }
    if (kind == 3)
    {
        double bot = rd.num(), top = rd.num(), s0 = rd.num(), s1 = rd.num(), h = rd.num();
        SmBoolean swap = rd.b(), inside = rd.b(), mngc = rd.b();
        if (!rd.ok)
            return NULL;
        return new (ctx) ApConePeek(o, x, y, bot, top, s0, s1, h, swap, inside, mngc);
    }
    if (kind == 4)
    {
        SmExtent2d d = rd.dom();
        double r = rd.num();
        SmBoolean swap = rd.b(), inside = rd.b();
        if (!rd.ok)
            return NULL;
        return new (ctx) SmSphere(o, x, y, d, r, swap, inside, NULL, &ctx);
    }
    if (kind == 5)
    {
        SmExtent2d d = rd.dom();
        double major = rd.num(), minor = rd.num();
        SmBoolean swap = rd.b(), inside = rd.b();
        if (!rd.ok)
            return NULL;
        return new (ctx) SmTorus(o, x, y, d, major, minor, swap, inside, NULL, &ctx);
    }
    SmExtent2d d = rd.dom();
    SmBoolean swap = rd.b();
    int64_t p = rd.idx(), cn = rd.idx();
    bool rational = rd.num() != 0.0;
    if (p < 1 || cn < 2 || cn > 64)
        return NULL;
    std::vector<gw_CPOINT> pts(cn);
    for (auto& c : pts)
        c = ap_point(rd, rational);
    std::vector<double> k(cn + p + 1);
    for (auto& v : k)
        v = rd.num();
    if (!rd.ok)
        return NULL;
    SmBSplineCurve* gen = new (ctx)
        SmBSplineCurve(3, (ULONG)(cn - 1), (ULONG)(cn + p), (short)p, pts.data(), k.data());
    return new (ctx) ApRevolution(gen, o, x, y, d, swap);
}

// The SmBSplineSurface part of `s`: degrees, counts, knots and control points.
void ap_put(std::vector<double>& v, SmBSplineSurface* s)
{
    ULONG nu = s->SmBSplineSurface::GetNumberControlPoints(SM_SP_U);
    ULONG nv = s->SmBSplineSurface::GetNumberControlPoints(SM_SP_V);
    v.push_back((double)s->SmBSplineSurface::GetDegree(SM_SP_U));
    v.push_back((double)s->SmBSplineSurface::GetDegree(SM_SP_V));
    v.push_back((double)nu);
    v.push_back((double)nv);
    for (int dir = 0; dir < 2; dir++)
    {
        SmTArray<double> k;
        SmTArray<ULONG> m;
        SmStatus st = s->SmBSplineSurface::GetKnots(dir == 0 ? SM_SP_U : SM_SP_V, k, &m);
        if (st != SM_SUCCESS)
        {
            v.push_back(-1.0);
            continue;
        }
        v.push_back((double)k.GetSize());
        for (ULONG i = 0; i < k.GetSize(); i++)
            v.push_back(k[i]);
        for (ULONG i = 0; i < m.GetSize(); i++)
            v.push_back((double)m[i]);
    }
    for (ULONG i = 0; i < nu; i++)
        for (ULONG j = 0; j < nv; j++)
        {
            SmPoint3d p;
            double w = 0.0;
            SmStatus st = s->GetControlPoint(SM_CP_EUCLIDIAN_RATIONAL, i, j, p, w);
            if (st != SM_SUCCESS)
                p = SmPoint3d(0.0, 0.0, 0.0), w = -1.0;
            v.insert(v.end(), {p.x, p.y, p.z, w});
        }
}
} // namespace

SC_EXPORT int32_t sc_surface_surface_approximate_eval(const double* input, int64_t n, double* out,
                                                     int64_t cap, int64_t* out_len)
{
    SC_TRY
    SmContext ctx;
    ApRd rd{input, n, 0};
    double tol = rd.num();
    int64_t level = rd.idx();
    bool just_copy = rd.num() != 0.0;
    int64_t n_wrap = rd.idx();
    if (!rd.ok || level < 0 || n_wrap < 0 || n_wrap > 3)
        return SC_ERR_ARG;
    struct Wrap
    {
        int64_t type;
        double d[12];
    };
    std::vector<Wrap> wraps((size_t)n_wrap);
    for (auto& w : wraps)
    {
        w.type = rd.idx();
        int64_t m = w.type == 0 || w.type == 1 ? 1 : w.type == 3 ? 12 : 0;
        if (w.type < 0 || w.type > 3)
            return SC_ERR_ARG;
        for (int64_t i = 0; i < m; i++)
            w.d[i] = rd.num();
    }
    int64_t kind = rd.idx();
    SmSurface* surface = ap_build(rd, ctx, kind);
    if (surface == NULL)
        return SC_ERR_ARG;
    if (!rd.ok || rd.at != n)
    {
        delete surface;
        return SC_ERR_ARG;
    }
    for (const Wrap& w : wraps)
    {
        if (w.type == 0)
            surface = new (ctx) SmOffsetSurface(w.d[0], *surface, TRUE, 0);
        else if (w.type == 1)
        {
            SmTArray<SmCurve*> c3d, cuv;
            SmBoolean implicit = w.d[0] != 0.0 ? TRUE : FALSE;
            surface = new (ctx) SmCurveBoundedSurface(*surface, &c3d, &cuv, implicit);
        }
        else if (w.type == 2)
            surface = new (ctx) SmSTEPSurface(*surface, TRUE);
        else
        {
            SmVector3d o(w.d[0], w.d[1], w.d[2]), x(w.d[3], w.d[4], w.d[5]);
            SmVector3d y(w.d[6], w.d[7], w.d[8]), z(w.d[9], w.d[10], w.d[11]);
            SmTransform* t = new (ctx) SmTransform(&ctx, o, x, y, &z);
            SmSurface* base = surface;
            SmSrfInVolume* siv = new (ctx) SmSrfInVolume(*base, FALSE, *t, 3, 0, &ctx);
            // Copy flag 3 makes the object own (and later delete) what it points to. A
            // failed Copy (SmSurface's default for curve bounded and STEP surfaces) leaves
            // it pointing to the original: free only the originals it replaced by copies.
            if (siv->GetSurface() != base)
                delete base;
            if (siv->GetVolume() != t)
                delete t;
            surface = siv;
        }
    }

    SmBSplineSurface* result = NULL;
    double achieved = 0.0;
    SmStatus st = surface->ApproximateSurface(ctx, SmApproxTol3d(tol), achieved, (ULONG)level,
                                              result, just_copy ? TRUE : FALSE);
    std::vector<double> v;
    v.push_back(st == SM_SUCCESS ? 0.0 : (double)st);
    v.push_back(achieved);
    v.push_back(result != NULL ? 1.0 : 0.0);
    if (result != NULL)
    {
        v.push_back((double)result->GetType());
        ap_put(v, result);
        delete result;
    }
    delete surface;
    return sc_copy_out(v.data(), (int64_t)v.size(), out, cap, out_len);
    SC_CATCH
}
