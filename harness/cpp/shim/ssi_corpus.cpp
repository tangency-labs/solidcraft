// SPDX-License-Identifier: Apache-2.0
// difftest shim for the `--ssi-corpus` mode (not a plan unit): builds the two surfaces of
// one case of jyviko's SSI corpus (https://github.com/jyviko/ssi-corpus, commit b04c8a6,
// MIT OR Apache-2.0) and runs SmSurface::GlobalSurfaceIntersect
// (source/SMLib/src/SmSurface.cpp and its SmPlane / SmCone / SmSphere / SmSurfOfRevolution
// overrides) the way the corpus' own runner/ssi_runner.cpp does in its `gsi` mode:
//  * surfaces built exactly as `build_surface` there: real SmPlane, SmCylinder, SmCone
//    (CreateCanonical), SmSphere, SmTorus, and SmBSplineSurface over an NLib surface
//    filled directly (homogeneous control points);
//  * the context is SmApiGetOrCreateContext(), as the runner;
//  * domains are GetNaturalUVDomain() (or the case's optional trim box);
//  * edges {TRUE, TRUE}, the given SmApproxTol3d (or NULL for "default"), no angle tolerance;
//  * each 3D curve sampled at n uniform parameters of its natural interval (points whose
//    EvaluatePoint fails are skipped), each UV curve likewise, with the surface evaluated at
//    every UV sample (NaN where that fails).
//
// Two entry points (the result is too large to recompute for a buffer retry):
//   sc_ssi_corpus_intersect(s1, n1, s2, n2, tol, use_default_tol, n_samples, out_len)
//     runs the case and keeps the record in this shim; *out_len = its length.
//   sc_ssi_corpus_take(out, cap, out_len) copies the kept record out (sc_copy_out rules).
//
// Surface encoding (flat doubles; validated in crates/solidcore-ffi/src/units/ssi_corpus.rs and
// bounds checked again here):
//   plane    1, origin[3], xaxis[3], yaxis[3], u0, u1, v0, v1
//   cylinder 2, origin[3], xaxis[3], yaxis[3], radius, angle0, angle1, height0, height1
//   cone     3, apex[3], xaxis[3], yaxis[3], half_angle_deg, height0, height1
//   sphere   4, center[3], xaxis[3], yaxis[3], radius, lon0, lon1, lat0, lat1
//   torus    5, center[3], xaxis[3], yaxis[3], R, r, major0, major1, minor0, minor1
//   nurbs    6, p, q, nu, nv, knots_u[nu+p+1], knots_v[nv+q+1], (x, y, z, w)[nu*nv] (Cartesian
//            points, row i = u index), then for every kind: has_domain, u0, u1, v0, v1
//
// Record: sm_status, domain1[u0,u1,v0,v1], domain2[...], n_curves, per curve:
//   type (SmTsectCurveType, -1 when none), deviation (NaN when none), has_3d, t0, t1,
//   curve GetType(), n_xyz, xyz[3*n_xyz], then for k = 1, 2: has_uv, n_uv, uv[2*n_uv],
//   s_at_uv[3*n_uv]; then n_points, points[3*n_points] (always 0 for GlobalSurfaceIntersect).
// Status: 0 ok, SC_ERR_ARG bad encoding, SC_ERR_BUILD (-1003) a surface could not be built (the
// runner's exit code 3), SC_ERR_EXCEPTION a C++ exception.
#include "sc_shim.h"

#include <SmApiGeneral.h>
#include <SmAxis2Placement.h>
#include <SmBSplineSurface.h>
#include <SmCone.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCylinder.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <NL_Nurbsdef.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmSphere.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmTorus.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <cmath>
#include <stdexcept>
#include <vector>

#define SC_ERR_BUILD (-1003)

namespace
{
struct Bad
{
};

struct BuildError
{
};

struct Rd
{
    const double* d;
    int64_t n;
    int64_t at;
    double num()
    {
        if (at >= n)
            throw Bad();
        return d[at++];
    }
    SmVector3d vec()
    {
        double x = num(), y = num(), z = num();
        return SmVector3d(x, y, z);
    }
    int64_t count(int64_t lo, int64_t hi)
    {
        double v = num();
        if (!(v >= (double)lo && v <= (double)hi) || v != std::floor(v))
            throw Bad();
        return (int64_t)v;
    }
};

// Kept between sc_ssi_corpus_intersect and sc_ssi_corpus_take.
std::vector<double> g_record;

SmVector3d unit_cross(const SmVector3d& a, const SmVector3d& b)
{
    SmVector3d c(a.y * b.z - a.z * b.y, a.z * b.x - a.x * b.z, a.x * b.y - a.y * b.x);
    double l = std::sqrt(c.x * c.x + c.y * c.y + c.z * c.z);
    return SmVector3d(c.x / l, c.y / l, c.z / l);
}

struct Built
{
    SmSurface* surf = nullptr;
    SmExtent2d domain;
};

// ssi_runner.cpp build_surface, reading the flat encoding instead of the case JSON.
Built build_surface(const SmContext& ctx, Rd& rd)
{
    Built b;
    int64_t type = rd.count(1, 6);
    if (type == 1)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec(), y = rd.vec();
        double u0 = rd.num(), u1 = rd.num(), v0 = rd.num(), v1 = rd.num();
        SmExtent2d dom(u0, v0, u1, v1);
        b.surf = new (ctx) SmPlane(o, x, y, SmVector2d(1.0, 1.0), dom, &ctx);
    }
    else if (type == 2)
    {
        SmVector3d origin = rd.vec();
        SmVector3d x = rd.vec(), y = rd.vec();
        double radius = rd.num(), a0 = rd.num(), a1 = rd.num(), h0 = rd.num(), h1 = rd.num();
        SmVector3d z = unit_cross(x, y);
        SmPoint3d o = origin + z * h0;
        b.surf = new (ctx) SmCylinder(o, x, y, radius, a0, a1, h1 - h0, FALSE, FALSE, FALSE, &ctx);
    }
    else if (type == 3)
    {
        // apex-based description: radius(h) = h * tan(half_angle), h in [h0, h1] along z,
        // full 360 degree rotation
        SmVector3d apex = rd.vec();
        SmVector3d x = rd.vec(), y = rd.vec();
        double half = rd.num(), h0 = rd.num(), h1 = rd.num();
        SmVector3d z = unit_cross(x, y);
        double t = std::tan(half * M_PI / 180.0);
        SmPoint3d o = apex + z * h0;
        SmAxis2Placement pl(o, x, y);
        SmCone* cone = nullptr;
        if (SmCone::CreateCanonical(ctx, pl, h0 * t, h1 * t, h1 - h0, cone) != SM_SUCCESS || !cone)
            throw BuildError();
        b.surf = cone;
    }
    else if (type == 4)
    {
        SmVector3d c = rd.vec();
        SmVector3d x = rd.vec(), y = rd.vec();
        double radius = rd.num(), lon0 = rd.num(), lon1 = rd.num(), lat0 = rd.num(), lat1 = rd.num();
        SmExtent2d anal(lon0, lat0, lon1, lat1);
        b.surf = new (ctx) SmSphere(c, x, y, anal, radius, FALSE, FALSE, NULL, &ctx);
    }
    else if (type == 5)
    {
        SmVector3d c = rd.vec();
        SmVector3d x = rd.vec(), y = rd.vec();
        double R = rd.num(), r = rd.num(), ma0 = rd.num(), ma1 = rd.num(), mi0 = rd.num(), mi1 = rd.num();
        SmExtent2d anal(ma0, mi0, ma1, mi1);
        b.surf = new (ctx) SmTorus(c, x, y, anal, R, r, FALSE, FALSE, NULL, &ctx);
    }
    else
    {
        int64_t p = rd.count(1, NL_MAXDEG), q = rd.count(1, NL_MAXDEG);
        int64_t nu = rd.count(1, 1 << 20), nv = rd.count(1, 1 << 20);
        if (nu < p + 1 || nv < q + 1 || nu * nv > (1 << 22))
            throw Bad();
        std::vector<double> U(nu + p + 1), V(nv + q + 1);
        for (double& k : U)
            k = rd.num();
        for (double& k : V)
            k = rd.num();
        // Allocate the NLib surface and fill it directly, as the runner (it avoids the
        // SmBSplineSurface component constructor's V knot copy).
        gw_SURFACE* gs = sm_AllocateNurbSurface(nu - 1, nv - 1, static_cast<gw_DEGREE>(p),
                                                static_cast<gw_DEGREE>(q), U.size() - 1, V.size() - 1);
        if (!gs)
            throw BuildError();
        for (size_t i = 0; i < U.size(); ++i)
            gs->knu->U[i] = U[i];
        for (size_t i = 0; i < V.size(); ++i)
            gs->knv->U[i] = V[i];
        for (int64_t i = 0; i < nu; ++i)
        {
            for (int64_t j = 0; j < nv; ++j)
            {
                double px = rd.num(), py = rd.num(), pz = rd.num(), w = rd.num();
                // NLib control points are homogeneous: Cartesian = (x/w, y/w, z/w)
                gs->net->Pw[i][j].x = px * w;
                gs->net->Pw[i][j].y = py * w;
                gs->net->Pw[i][j].z = pz * w;
                gs->net->Pw[i][j].w = w;
            }
        }
        b.surf = new (ctx) SmBSplineSurface(gs, FALSE, &ctx);
    }
    b.domain = b.surf->GetNaturalUVDomain();
    int64_t has_domain = rd.count(0, 1);
    double d0 = rd.num(), d1 = rd.num(), d2 = rd.num(), d3 = rd.num();
    if (has_domain) // optional trim box in natural parameters [u0,u1,v0,v1]
        b.domain = SmExtent2d(d0, d2, d1, d3);
    return b;
}

// ssi_runner.cpp sample_curve: uniform in the natural parameter, failed evaluations skipped.
std::vector<SmPoint3d> sample_curve(const SmCurve* c, int n, SmExtent1d* ivl_out)
{
    std::vector<SmPoint3d> out;
    if (!c)
        return out;
    SmExtent1d ivl = c->GetNaturalInterval();
    if (ivl_out)
        *ivl_out = ivl;
    for (int i = 0; i < n; ++i)
    {
        double t = ivl.GetMin() + (ivl.GetMax() - ivl.GetMin()) * (static_cast<double>(i) / (n - 1));
        SmPoint3d p;
        if (c->EvaluatePoint(t, p) == SM_SUCCESS)
            out.push_back(p);
    }
    return out;
}

void put3(std::vector<double>& o, const SmVector3d& p)
{
    o.push_back(p.x);
    o.push_back(p.y);
    o.push_back(p.z);
}

// ssi_runner.cpp curve_json, as a flat record.
void put_curve(std::vector<double>& o, const SmCurve* c3, const SmCurve* uv1, const SmCurve* uv2,
               const SmSurface* s1, const SmSurface* s2, int type, double dev, int n)
{
    o.push_back((double)type);
    o.push_back(dev);
    SmExtent1d ivl;
    std::vector<SmPoint3d> pts = sample_curve(c3, n, &ivl);
    o.push_back(c3 ? 1.0 : 0.0);
    o.push_back(c3 ? ivl.GetMin() : NAN);
    o.push_back(c3 ? ivl.GetMax() : NAN);
    o.push_back(c3 ? (double)c3->GetType() : -1.0);
    o.push_back((double)pts.size());
    for (const SmPoint3d& p : pts)
        put3(o, p);
    const SmCurve* uvs[2] = {uv1, uv2};
    const SmSurface* ss[2] = {s1, s2};
    for (int k = 0; k < 2; ++k)
    {
        o.push_back(uvs[k] ? 1.0 : 0.0);
        std::vector<SmPoint3d> uv = sample_curve(uvs[k], n, nullptr);
        o.push_back((double)uv.size());
        for (const SmPoint3d& p : uv)
        {
            o.push_back(p.x);
            o.push_back(p.y);
        }
        for (const SmPoint3d& p : uv)
        {
            SmPoint3d q;
            if (ss[k]->EvaluatePoint(SmPoint2d(p.x, p.y), q) != SM_SUCCESS)
                q = SmPoint3d(NAN, NAN, NAN);
            put3(o, q);
        }
    }
}

void free_curves(SmTArray<SmCurve*>& a)
{
    for (ULONG i = 0; i < a.GetSize(); ++i)
        delete a[i];
    a.RemoveAll();
}

void put_domain(std::vector<double>& o, const SmExtent2d& d)
{
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().y);
}
} // namespace

SC_EXPORT int32_t sc_ssi_corpus_intersect(const double* s1, int64_t n1, const double* s2, int64_t n2,
                                          double tol, int32_t use_default_tol, int32_t n_samples,
                                          int64_t* out_len)
{
    SC_TRY
    g_record.clear();
    *out_len = 0;
    if (s1 == nullptr || s2 == nullptr || n1 < 1 || n2 < 1 || n_samples < 2)
        return SC_ERR_ARG;
    Built a, b;
    try
    {
        SmContext* pctx = SmApiGetOrCreateContext();
        if (pctx == nullptr)
            return SC_ERR_BUILD;
        const SmContext& ctx = *pctx;
        try
        {
            Rd r1{s1, n1, 0}, r2{s2, n2, 0};
            a = build_surface(ctx, r1);
            b = build_surface(ctx, r2);
            if (r1.at != r1.n || r2.at != r2.n)
                throw Bad();
        }
        catch (const BuildError&)
        {
            delete a.surf;
            delete b.surf;
            return SC_ERR_BUILD;
        }
        catch (const std::exception&)
        {
            // the runner reports any surface construction exception as a build error
            delete a.surf;
            delete b.surf;
            return SC_ERR_BUILD;
        }

        SmApproxTol3d t3(use_default_tol ? 0.0 : tol);
        const SmApproxTol3d* ptol = use_default_tol ? nullptr : &t3;
        SmTArray<SmCurve*> c3, uv1, uv2;
        SmTArray<SmTsectCurveType> types;
        SmTArray<double> devs;
        SmBoolean edges[2] = {TRUE, TRUE};
        SmStatus stat = a.surf->GlobalSurfaceIntersect(ctx, a.domain, *b.surf, b.domain, edges, ptol, NULL,
                                                       &c3, &uv1, &uv2, &types, &devs);

        std::vector<double>& o = g_record;
        o.push_back((double)stat);
        put_domain(o, a.domain);
        put_domain(o, b.domain);
        o.push_back((double)c3.GetSize());
        for (ULONG i = 0; i < c3.GetSize(); ++i)
        {
            const SmCurve* u1 = i < uv1.GetSize() ? uv1[i] : nullptr;
            const SmCurve* u2 = i < uv2.GetSize() ? uv2[i] : nullptr;
            int ty = i < types.GetSize() ? static_cast<int>(types[i]) : -1;
            double dv = i < devs.GetSize() ? devs[i] : NAN;
            put_curve(o, c3[i], u1, u2, a.surf, b.surf, ty, dv, n_samples);
        }
        o.push_back(0.0); // GlobalSurfaceIntersect returns no isolated points
        free_curves(c3);
        free_curves(uv1);
        free_curves(uv2);
        delete a.surf;
        delete b.surf;
        *out_len = (int64_t)o.size();
        return SC_OK;
    }
    catch (const Bad&)
    {
        g_record.clear();
        return SC_ERR_ARG;
    }
    catch (...)
    {
        g_record.clear();
        return SC_ERR_EXCEPTION;
    }
    SC_CATCH
}

SC_EXPORT int32_t sc_ssi_corpus_take(double* out, int64_t cap, int64_t* out_len)
{
    SC_TRY
    int32_t st = sc_copy_out(g_record.data(), (int64_t)g_record.size(), out, cap, out_len);
    if (st == SC_OK)
        g_record.clear();
    return st;
    SC_CATCH
}
