// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `intersect.surface_drop_lib_p3`: the base body of
// SmSurface::DropCurve (upstream source/SMLib/src/SmSurfaceDropLib.cpp), called
// non-virtually (`pSurf->SmSurface::DropCurve(...)`, so an SmPlane runs the base body and
// not its override) on real SmPlane, SmBSplineSurface and SmSurfOfRevolution surfaces and
// SmLine, SmCircle and SmBSplineCurve curves, with an optional SmDropCurveFail record.
//
// One entry point, sc_intersect_surface_drop_lib_p3_run(in, n_in, out, cap, out_len): one
// drop per call. The layouts are documented in
// crates/solidcore-ffi/src/units/intersect_surface_drop_lib_p3.rs, which validates the
// inputs; every read here is bounds checked as well (a bad program is SC_ERR_ARG). The
// surface and curve blocks are those of the intersect.surface_drop_lib_p2 shim.

#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmSolutionArray.h>
#include <SmSurfOfRevolution.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <stdexcept>
#include <vector>

namespace sc_sdl3
{
// Record of a drop whose surface of revolution could not be made.
const double NO_SURFACE = -3.0;

struct Bad : std::runtime_error
{
    Bad() : std::runtime_error("bad program") {}
};

struct Rd
{
    const double* in;
    int64_t n;
    int64_t at;
    double num()
    {
        if (at >= n)
            throw Bad();
        return in[at++];
    }
    int64_t idx(int64_t lo, int64_t hi)
    {
        double d = num();
        if (!(d >= (double)lo && d <= (double)hi) || d != (double)(int64_t)d)
            throw Bad();
        return (int64_t)d;
    }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
    SmPoint2d uv()
    {
        double x = num();
        double y = num();
        return SmPoint2d(x, y);
    }
    SmExtent2d dom4()
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

void put_vec(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

// Objects made for one drop; deleted on every return.
struct Owned
{
    std::vector<SmSurface*> surfaces;
    std::vector<SmCurve*> curves;
    ~Owned()
    {
        for (size_t i = 0; i < curves.size(); i++)
            delete curves[i];
        for (size_t i = 0; i < surfaces.size(); i++)
            delete surfaces[i];
    }
};

// `0 a[3] b[3]` SmLine (its NURB made), `1 c[3] x[3] y[3] d0 d1 r` SmCircle, or
// `2 p ncp cpts(x y z w) knots` SmBSplineCurve (dim 3).
SmCurve* read_curve(Rd& rd, const SmContext& ctx, Owned& own)
{
    int64_t kind = rd.idx(0, 2);
    if (kind == 0)
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine* l = new (ctx) SmLine(a, b, 3, &ctx);
        own.curves.push_back(l);
        l->GetOrCreateGwNurbPointer();
        return l;
    }
    if (kind == 1)
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double r = rd.num();
        SmCircle* cc = new (ctx) SmCircle(c, x, y, SmExtent1d(d0, d1), r, 3, &ctx);
        own.curves.push_back(cc);
        return cc;
    }
    int64_t p = rd.idx(1, 8);
    int64_t ncp = rd.idx(2, 64);
    if (p >= ncp)
        throw Bad();
    int64_t n = ncp - 1;
    int64_t m = n + p + 1;
    std::vector<double> v;
    for (int64_t i = 0; i < 4 * ncp + m + 1; i++)
        v.push_back(rd.num());
    gw_CURVE* cur = sm_AllocateNurbCurve((gw_INDEX)n, (gw_DEGREE)p, (gw_INDEX)m);
    for (int64_t i = 0; i <= n; i++)
    {
        cur->pol->Pw[i].x = v[4 * i];
        cur->pol->Pw[i].y = v[4 * i + 1];
        cur->pol->Pw[i].z = v[4 * i + 2];
        cur->pol->Pw[i].w = v[4 * i + 3];
    }
    for (int64_t i = 0; i <= m; i++)
        cur->knt->U[i] = v[4 * ncp + i];
    SmBSplineCurve* c = new (ctx) SmBSplineCurve(cur, 3, FALSE);
    own.curves.push_back(c);
    return c;
}

// `0 o x y scale dom` SmPlane (with its NURB made), `1 p q nu nv net(x y z w) u v`
// SmBSplineSurface, or `2 gen point[3] axis[3] swap` SmSurfOfRevolution::CreateCanonical
// with the generator a line `0 a b` or a circle `1 c x y d0 d1 r` (see read_curve). NULL
// when CreateCanonical fails.
SmSurface* read_surface(Rd& rd, const SmContext& ctx, Owned& own)
{
    int64_t kind = rd.idx(0, 2);
    if (kind == 0)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        SmVector2d s = rd.uv();
        SmExtent2d d = rd.dom4();
        SmPlane* srf = new (ctx) SmPlane(o, x, y, s, d, &ctx);
        // Make the plane's NURB up front: const queries would make and keep it later,
        // changing later point solves; the Rust plane never keeps a NURB made by a
        // read-only query, so both sides start with it (plan/findings.md).
        srf->GetOrCreateGwNurbPointer();
        own.surfaces.push_back(srf);
        return srf;
    }
    if (kind == 1)
    {
        gw_DEGREE p = (gw_DEGREE)rd.idx(1, 8);
        gw_DEGREE q = (gw_DEGREE)rd.idx(1, 8);
        gw_INDEX nu = (gw_INDEX)rd.idx(2, 33);
        gw_INDEX nv = (gw_INDEX)rd.idx(2, 33);
        if (p >= nu || q >= nv)
            throw Bad();
        gw_INDEX n = nu - 1;
        gw_INDEX m = nv - 1;
        gw_INDEX r = n + p + 1;
        gw_INDEX s = m + q + 1;
        std::vector<double> v;
        for (int64_t i = 0; i < 4 * (int64_t)nu * nv + r + 1 + s + 1; i++)
            v.push_back(rd.num());
        gw_SURFACE* sur = sm_AllocateNurbSurface(n, m, p, q, r, s);
        size_t k = 0;
        for (gw_INDEX i = 0; i <= n; i++)
            for (gw_INDEX j = 0; j <= m; j++)
            {
                sur->net->Pw[i][j].x = v[k++];
                sur->net->Pw[i][j].y = v[k++];
                sur->net->Pw[i][j].z = v[k++];
                sur->net->Pw[i][j].w = v[k++];
            }
        for (gw_INDEX i = 0; i <= r; i++)
            sur->knu->U[i] = v[k++];
        for (gw_INDEX i = 0; i <= s; i++)
            sur->knv->U[i] = v[k++];
        SmSurface* srf = new (ctx) SmBSplineSurface(sur, FALSE, &ctx);
        own.surfaces.push_back(srf);
        return srf;
    }
    int64_t gk = rd.idx(0, 1);
    SmBSplineCurve* gen = NULL;
    if (gk == 0)
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine* l = new (ctx) SmLine(a, b, 3, &ctx);
        l->GetOrCreateGwNurbPointer();
        gen = l;
    }
    else
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double r = rd.num();
        gen = new (ctx) SmCircle(c, x, y, SmExtent1d(d0, d1), r, 3, &ctx);
    }
    SmPoint3d pt = rd.vec();
    SmVector3d ax = rd.vec();
    bool swap = rd.idx(0, 1) != 0;
    SmSurfOfRevolution* rev = NULL;
    if (SmSurfOfRevolution::CreateCanonical(ctx, gen, pt, ax, rev, swap ? TRUE : FALSE) != SM_SUCCESS ||
        rev == NULL)
    {
        delete rev;
        return NULL;
    }
    own.surfaces.push_back(rev);
    return rev;
}

// `degree ncp cpts(x y z w) nknots knots mults ivl_min ivl_max` of a B-spline curve.
void put_curve(std::vector<double>& o, SmBSplineCurve* c)
{
    if (c == NULL)
        throw Bad();
    gw_CURVE* cur = c->GetOrCreateGwNurbPointer();
    if (cur == NULL)
        throw Bad();
    o.push_back((double)cur->p);
    o.push_back((double)(cur->pol->n + 1));
    for (gw_INDEX i = 0; i <= cur->pol->n; i++)
    {
        o.push_back(cur->pol->Pw[i].x);
        o.push_back(cur->pol->Pw[i].y);
        o.push_back(cur->pol->Pw[i].z);
        o.push_back(cur->pol->Pw[i].w);
    }
    SmTArray<double> knots;
    SmTArray<ULONG> mults;
    if (c->GetKnots(knots, &mults) != SM_SUCCESS)
        throw Bad();
    o.push_back((double)knots.GetSize());
    for (ULONG i = 0; i < knots.GetSize(); i++)
        o.push_back(knots[i]);
    for (ULONG i = 0; i < mults.GetSize(); i++)
        o.push_back((double)mults[i]);
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
}

// SmSurfParamType as the record value: 0 U, 1 V, 2 BOTH, 3 NEITHER, 4 UNKNOWN (5 other).
double param_code(SmSurfParamType e)
{
    switch (e)
    {
    case SM_SP_U:
        return 0.0;
    case SM_SP_V:
        return 1.0;
    case SM_SP_BOTH:
        return 2.0;
    case SM_SP_NEITHER:
        return 3.0;
    case SM_SP_UNKNOWN:
        return 4.0;
    default:
        return 5.0;
    }
}

// The SmDropCurveFail record (see the ffi module).
void put_record(std::vector<double>& o, SmDropCurveFail& f)
{
    o.push_back(st(f.m_sFailStatus));
    o.push_back((double)f.m_lFromCall);
    o.push_back((double)f.m_eCreatePathType);
    o.push_back(f.m_cpContext ? 1.0 : 0.0);
    SmExtent2d d = f.m_sUVDomain;
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
    o.push_back(f.m_sInterval.GetMin());
    o.push_back(f.m_sInterval.GetMax());
    o.push_back((double)f.m_sApproxTol3d);
    o.push_back(f.m_dMaxDropToSurfArg);
    o.push_back(f.m_dMaxApproxDev);
    o.push_back((double)f.m_bKeepAllDropCurves);
    o.push_back((double)f.m_bDegenCurve);
    o.push_back((double)f.m_bDegenSurfU);
    o.push_back((double)f.m_bDegenSurfV);
    o.push_back((double)f.m_bDropDomainIsContained);
    o.push_back(f.m_dMaxDropToSurf);
    o.push_back((double)f.m_bClosedU);
    o.push_back((double)f.m_bClosedV);
    o.push_back(param_code(f.m_eSrfClosure));
    o.push_back((double)f.m_lSingularities);
    o.push_back((double)f.m_lApproxSingularities);
    o.push_back((double)f.m_sSrfPolePoints.GetSize());
    for (ULONG i = 0; i < f.m_sSrfPolePoints.GetSize(); i++)
        put_vec(o, f.m_sSrfPolePoints[i]);
    o.push_back((double)f.m_sSrfPoleNormals.GetSize());
    for (ULONG i = 0; i < f.m_sSrfPoleNormals.GetSize(); i++)
        put_vec(o, f.m_sSrfPoleNormals[i]);
    SmSolutionArray& sols = f.m_sSolutions;
    o.push_back((double)sols.GetSize());
    for (ULONG i = 0; i < sols.GetSize(); i++)
    {
        o.push_back(sols[i].m_vStart[0]);
        o.push_back(sols[i].m_vStart[1]);
    }
    o.push_back((double)f.m_sCtrlPoly.GetSize());
    for (ULONG i = 0; i < f.m_sCtrlPoly.GetSize(); i++)
        put_vec(o, f.m_sCtrlPoly[i]);
    o.push_back((double)f.m_sKnots.GetSize());
    for (ULONG i = 0; i < f.m_sKnots.GetSize(); i++)
        o.push_back(f.m_sKnots[i]);
    o.push_back((double)f.m_sUVCurves.GetSize());
    for (ULONG i = 0; i < f.m_sUVCurves.GetSize(); i++)
        put_curve(o, f.m_sUVCurves[i]);
}

struct Curves
{
    SmTArray<SmBSplineCurve*> a;
    ~Curves()
    {
        for (ULONG i = 0; i < a.GetSize(); i++)
            delete a[i];
    }
};

// `surface curve domain[4] interval[2] tol keep_all want_record`.
void run(Rd& rd, std::vector<double>& o)
{
    SmContext ctx;
    Owned own;
    SmSurface* s = read_surface(rd, ctx, own);
    SmCurve* c = read_curve(rd, ctx, own);
    SmExtent2d d = rd.dom4();
    double i0 = rd.num();
    double i1 = rd.num();
    double tol = rd.num();
    SmBoolean keep_all = (SmBoolean)rd.idx(0, 1);
    bool want_record = rd.idx(0, 1) != 0;
    if (s == NULL)
    {
        o.push_back(NO_SURFACE);
        return;
    }
    SmExtent1d ivl(i0, i1);
    double max_drop = -1.0;
    double max_dev = -1.0;
    Curves curves;
    SmDropCurveFail fail;
    SmStatus stt = s->SmSurface::DropCurve(ctx, d, *c, ivl, tol, max_drop, max_dev, curves.a, keep_all,
                                           want_record ? &fail : NULL, FALSE);
    o.push_back(st(stt));
    o.push_back(max_drop);
    o.push_back(max_dev);
    o.push_back((double)curves.a.GetSize());
    for (ULONG i = 0; i < curves.a.GetSize(); i++)
        put_curve(o, curves.a[i]);
    if (want_record)
        put_record(o, fail);
}
} // namespace sc_sdl3

SC_EXPORT int32_t sc_intersect_surface_drop_lib_p3_run(const double* in, int64_t n_in, double* out, int64_t cap,
                                                       int64_t* out_len)
{
    if (in == nullptr || n_in < 1 || out_len == nullptr || (out == nullptr && cap > 0))
        return SC_ERR_ARG;
    SC_TRY
    sc_sdl3::Rd rd{in, n_in, 0};
    std::vector<double> o;
    try
    {
        sc_sdl3::run(rd, o);
    }
    catch (const sc_sdl3::Bad&)
    {
        return SC_ERR_ARG;
    }
    if (rd.at != n_in)
        return SC_ERR_ARG;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
