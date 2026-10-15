// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `intersect.surface_drop_lib_p2`: SmDropPt::SetProperties and
// SmDropPt::GetNextPoint (the file-local class SmDropPt) and the static helpers
// sm_AddToCurve and sm_FlushCrv of upstream source/SMLib/src/SmSurfaceDropLib.cpp, on real
// SmPlane, SmBSplineSurface and SmSurfOfRevolution surfaces and SmLine, SmCircle and
// SmBSplineCurve curves, with the SmOffsetSurface(d, surface, FALSE, lSingularities) that
// DropCurve builds.
//
// SmDropPt and the helpers are local to that file, so this translation unit compiles the
// upstream source itself (same flags as the kernel), at global scope, as the p1 shim does:
//  * every header the file includes is included first, so its own #includes are no-ops;
//  * SmSurface::DropCurve / DropAndTrimCurve / DropIsoCurve (which the file defines) are
//    renamed before any header is read, so this TU's definitions get new names;
//  * SmDropPt and my_AssertAndDrawDropCurve are renamed so they cannot clash with the
//    library's (or the p1 shim's) copies.
//
// One entry point, sc_intersect_surface_drop_lib_p2_run(in, n_in, out, cap, out_len): one
// op per call. The layouts are documented in
// crates/solidcore-ffi/src/units/intersect_surface_drop_lib_p2.rs, which validates the
// inputs; every read here is bounds checked as well (a bad program is SC_ERR_ARG). Inputs
// that reach a path upstream leaves undefined (GetNextPoint's ignored start evaluation
// status) report REJECTED.
#define DropCurve sc_sdl2_DropCurve
#define DropAndTrimCurve sc_sdl2_DropAndTrimCurve
#define DropIsoCurve sc_sdl2_DropIsoCurve

#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAssertArray.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmCircle.h>
#include <SmCompositeCurve.h>
#include <SmContext.h>
#include <SmCrvOnSurf.h>
#include <SmCurveCache.h>
#include <SmEdge.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFace.h>
#include <SmGauss.h>
#include <SmGeomUtility.h>
#include <SmGlobalSolver.h>
#include <SmGraphicsOutput.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmLocalSolve1d.h>
#include <SmLocalSolveNd.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmOffsetSurface.h>
#include <SmPlane.h>
#include <SmSolutionArray.h>
#include <SmSurfOfRevolution.h>
#include <SmSurface.h>
#include <SmSurfaceDropCurve.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <stdexcept>
#include <vector>

#define SmDropPt sc_sdl2_SmDropPt
#define my_AssertAndDrawDropCurve sc_sdl2_my_AssertAndDrawDropCurve
#include "../../../upstream/source/SMLib/src/SmSurfaceDropLib.cpp"

namespace sc_sdl2
{
enum
{
    OP_SET = 1,
    OP_NEXT = 2,
    OP_ADD = 3,
    OP_FLUSH = 4,
};

// Op status of an op the shim does not run (undefined upstream).
const double REJECTED = -2.0;
// Record of an op whose surface of revolution could not be made.
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

void put_uv(std::vector<double>& o, const SmVector2d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
}

// Objects made for one op; deleted on every return.
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

// `0 o x y scale dom` SmPlane (with its NURB made), `1 p q nu nv net(x y z w) u v` SmBSplineSurface, or
// `2 gen point[3] axis[3] swap` SmSurfOfRevolution::CreateCanonical with the generator a
// line `0 a b` or a circle `1 c x y d0 d1 r` (see read_curve). NULL when CreateCanonical
// fails.
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
        // Make the plane's NURB up front: const queries (GetNextPoint's evaluations) would
        // make and keep it later, changing later point solves; the Rust plane never keeps a
        // NURB made by a read-only query, so both sides start with it (plan/findings.md).
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

SmBoolean tri(Rd& rd)
{
    return (SmBoolean)rd.idx(0, 2);
}

// The 38-value drop point state: t good crv[3][3] uv[2][2] pd3d[2][3] normal[3]
// from_left_u from_left_v drop_to_surf approx_dev normal_to_crv srf_to_crv speed_ratio
// on_boundary on_seam on_pole degen leaving entering deriv_rtn.
void read_pt(Rd& rd, sc_sdl2_SmDropPt& p)
{
    p.m_dT = rd.num();
    p.m_bGoodDrop = tri(rd);
    for (int i = 0; i < 3; i++)
        p.m_CrvPD[i] = rd.vec();
    for (int i = 0; i < 2; i++)
        p.m_UVPD[i] = rd.uv();
    for (int i = 0; i < 2; i++)
        p.m_3dPD[i] = rd.vec();
    p.m_SrfNormal = rd.vec();
    p.m_bFromLeftU = tri(rd);
    p.m_bFromLeftV = tri(rd);
    p.m_dDropToSurf = rd.num();
    p.m_dApproxDev = rd.num();
    p.m_dNormalToCrvAngDeg = rd.num();
    p.m_dSrfToCrvAngDeg = rd.num();
    p.m_dSrfToCrvSpeedRatio = rd.num();
    p.m_bOnBoundary = tri(rd);
    p.m_bOnSeam = tri(rd);
    p.m_bOnPole = tri(rd);
    p.m_bDegenCurve = tri(rd);
    p.m_bLeavingDomain = tri(rd);
    p.m_bEnteringDomain = tri(rd);
    double r = rd.num();
    if (!(r >= 0.0 && r <= 1.0e8) || r != (double)(int64_t)r)
        throw Bad();
    p.m_eDropDerivRtn = (SmStatus)(int64_t)r;
}

void put_pt(std::vector<double>& o, const sc_sdl2_SmDropPt& p)
{
    o.push_back(p.m_dT);
    o.push_back((double)p.m_bGoodDrop);
    for (int i = 0; i < 3; i++)
        put_vec(o, p.m_CrvPD[i]);
    for (int i = 0; i < 2; i++)
        put_uv(o, p.m_UVPD[i]);
    for (int i = 0; i < 2; i++)
        put_vec(o, p.m_3dPD[i]);
    put_vec(o, p.m_SrfNormal);
    o.push_back((double)p.m_bFromLeftU);
    o.push_back((double)p.m_bFromLeftV);
    o.push_back(p.m_dDropToSurf);
    o.push_back(p.m_dApproxDev);
    o.push_back(p.m_dNormalToCrvAngDeg);
    o.push_back(p.m_dSrfToCrvAngDeg);
    o.push_back(p.m_dSrfToCrvSpeedRatio);
    o.push_back((double)p.m_bOnBoundary);
    o.push_back((double)p.m_bOnSeam);
    o.push_back((double)p.m_bOnPole);
    o.push_back((double)p.m_bDegenCurve);
    o.push_back((double)p.m_bLeavingDomain);
    o.push_back((double)p.m_bEnteringDomain);
    o.push_back((double)p.m_eDropDerivRtn);
}

// `degree ncp cpts(x y z w) nknots knots mults ivl_min ivl_max` of a B-spline curve.
void put_curve(std::vector<double>& o, SmBSplineCurve* c)
{
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

SmSurfParamType closure_of(int64_t c)
{
    return c == 0 ? SM_SP_U : c == 1 ? SM_SP_V : c == 2 ? SM_SP_BOTH : SM_SP_NEITHER;
}

// The growing curve state after AddToCurve / FlushCrv: `npoly poly(x y z) nknots knots
// ncurves curves`; the curves are deleted.
void put_state(std::vector<double>& o, SmTArray<SmPoint3d>& poly, SmTArray<double>& knots,
               SmTArray<SmBSplineCurve*>& curves)
{
    o.push_back((double)poly.GetSize());
    for (ULONG i = 0; i < poly.GetSize(); i++)
        put_vec(o, poly[i]);
    o.push_back((double)knots.GetSize());
    for (ULONG i = 0; i < knots.GetSize(); i++)
        o.push_back(knots[i]);
    o.push_back((double)curves.GetSize());
    for (ULONG i = 0; i < curves.GetSize(); i++)
        put_curve(o, curves[i]);
}

// The unused curve / surface arguments of sm_AddToCurve and sm_FlushCrv.
SmCurve& unused_curve(const SmContext& ctx, Owned& own)
{
    SmLine* l = new (ctx) SmLine(SmPoint3d(0, 0, 0), SmPoint3d(1, 0, 0), 3, &ctx);
    own.curves.push_back(l);
    return *l;
}

SmSurface& unused_surface(const SmContext& ctx, Owned& own)
{
    SmSurface* s = new (ctx) SmPlane(SmPoint3d(0, 0, 0), SmVector3d(1, 0, 0), SmVector3d(0, 1, 0),
                                     SmVector2d(1, 1), SmExtent2d(0, 0, 1, 1), &ctx);
    own.surfaces.push_back(s);
    return *s;
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

void run(Rd& rd, std::vector<double>& o)
{
    SmContext ctx;
    Owned own;
    int64_t op = rd.idx(OP_SET, OP_FLUSH);
    switch (op)
    {
    case OP_SET:
    {
        SmSurface* s = read_surface(rd, ctx, own);
        sc_sdl2_SmDropPt p(0.0, FALSE);
        read_pt(rd, p);
        double dist = rd.num();
        ULONG sing = (ULONG)rd.idx(0, 31);
        double tol = rd.num();
        SmExtent2d d = rd.dom4();
        SmBoolean adjust = (SmBoolean)rd.idx(0, 1);
        if (s == NULL)
        {
            o.push_back(NO_SURFACE);
            return;
        }
        SmOffsetSurface off(dist, *s, FALSE, sing);
        SmStatus stt = p.SetProperties(*s, off, sing, tol, d, adjust);
        o.push_back(st(stt));
        put_pt(o, p);
        o.push_back(off.GetOffsetDistance());
        return;
    }
    case OP_NEXT:
    {
        SmSurface* s = read_surface(rd, ctx, own);
        SmCurve* c = read_curve(rd, ctx, own);
        sc_sdl2_SmDropPt a(0.0, FALSE);
        read_pt(rd, a);
        double dist = rd.num();
        ULONG sing = (ULONG)rd.idx(0, 31);
        bool has_poles = rd.idx(0, 1) != 0;
        SmTArray<SmPoint3d> poles;
        if (has_poles)
            for (int i = 0; i < 4; i++)
                poles.Add(rd.vec());
        SmExtent2d d = rd.dom4();
        SmSurfParamType closure = closure_of(rd.idx(0, 3));
        double tol = rd.num();
        double tx = rd.num();
        double ty = rd.num();
        double end_t = rd.num();
        if (s == NULL)
        {
            o.push_back(NO_SURFACE);
            return;
        }
        // GetNextPoint ignores the status of its start evaluation (a path not ported)
        double zu = SM_EFF_ZERO * (1.0 + d.GetUInterval().GetLength());
        double zv = SM_EFF_ZERO * (1.0 + d.GetVInterval().GetLength());
        if (d.ContainsPoint2d(a.m_UVPD[0], smos_Max(zu, zv)) && !(a.m_bOnBoundary && a.m_bLeavingDomain))
        {
            SmVector3d p, su, sv, suv, suu, svv;
            if (s->Evaluate2ndDerivatives(a.m_UVPD[0], a.m_bFromLeftU, a.m_bFromLeftV, p, su, sv, suv, suu, svv) !=
                SM_SUCCESS)
            {
                o.push_back(REJECTED);
                return;
            }
        }
        SmOffsetSurface off(dist, *s, FALSE, sing);
        sc_sdl2_SmDropPt b(0.0, FALSE);
        SmStatus stt = a.GetNextPoint(*s, off, sing, has_poles ? &poles : NULL, d, closure, *c, tol, tx, ty, end_t, b);
        o.push_back(st(stt));
        put_pt(o, b);
        o.push_back(off.GetOffsetDistance());
        return;
    }
    case OP_ADD:
    {
        SmSurface* s = read_surface(rd, ctx, own);
        SmExtent2d d = rd.dom4();
        double tol = rd.num();
        double tx = rd.num();
        double ty = rd.num();
        int64_t np = rd.idx(0, 20000);
        std::vector<sc_sdl2_SmDropPt> pts(np, sc_sdl2_SmDropPt(0.0, FALSE));
        for (int64_t i = 0; i < np; i++)
            read_pt(rd, pts[i]);
        int64_t ns = rd.idx(0, 20000);
        std::vector<std::pair<int64_t, int64_t>> spans;
        for (int64_t i = 0; i < ns; i++)
        {
            int64_t ia = rd.idx(0, np - 1);
            int64_t ib = rd.idx(0, np - 1);
            spans.push_back(std::make_pair(ia, ib));
        }
        bool flush = rd.idx(0, 1) != 0;
        if (s == NULL)
        {
            o.push_back(NO_SURFACE);
            return;
        }
        SmCurve& crv = unused_curve(ctx, own);
        SmTArray<SmPoint3d> poly;
        SmTArray<double> knots;
        Curves curves;
        bool failed = false;
        for (size_t i = 0; i < spans.size() && !failed; i++)
        {
            SmBoolean ch = FALSE;
            SmStatus stt = sm_AddToCurve(ctx, *s, d, crv, pts[spans[i].first], pts[spans[i].second], poly, knots,
                                         tol, tx, ty, curves.a, ch);
            o.push_back(st(stt));
            o.push_back((double)ch);
            failed = stt != SM_SUCCESS;
        }
        if (flush && !failed)
        {
            SmBoolean ch = FALSE;
            SmStatus stt = sm_FlushCrv(ctx, *s, d, crv, poly, knots, tx, ty, curves.a, ch);
            o.push_back(st(stt));
            o.push_back((double)ch);
        }
        put_state(o, poly, knots, curves.a);
        return;
    }
    case OP_FLUSH:
    {
        SmExtent2d d = rd.dom4();
        double tx = rd.num();
        double ty = rd.num();
        int64_t np = rd.idx(0, 20000);
        SmTArray<SmPoint3d> poly;
        for (int64_t i = 0; i < np; i++)
            poly.Add(rd.vec());
        int64_t nk = rd.idx(0, 20000);
        SmTArray<double> knots;
        for (int64_t i = 0; i < nk; i++)
            knots.Add(rd.num());
        // the shapes sm_AddToCurve builds: no knots, or 1 + 3 (n - 1) points for n >= 2
        // increasing knots (others read past the arrays or are undefined in NLib)
        if (nk != 0 && (nk < 2 || np != 1 + 3 * (nk - 1)))
            throw Bad();
        for (int64_t i = 1; i < nk; i++)
            if (!(knots[(ULONG)i] > knots[(ULONG)(i - 1)]))
                throw Bad();
        Curves curves;
        SmBoolean ch = FALSE;
        SmStatus stt = sm_FlushCrv(ctx, unused_surface(ctx, own), d, unused_curve(ctx, own), poly, knots, tx, ty, curves.a, ch);
        o.push_back(st(stt));
        o.push_back((double)ch);
        put_state(o, poly, knots, curves.a);
        return;
    }
    default:
        throw Bad();
    }
}
} // namespace sc_sdl2

SC_EXPORT int32_t sc_intersect_surface_drop_lib_p2_run(const double* in, int64_t n_in, double* out, int64_t cap,
                                                       int64_t* out_len)
{
    if (in == nullptr || n_in < 1 || out_len == nullptr || (out == nullptr && cap > 0))
        return SC_ERR_ARG;
    SC_TRY
    sc_sdl2::Rd rd{in, n_in, 0};
    std::vector<double> o;
    try
    {
        sc_sdl2::run(rd, o);
    }
    catch (const sc_sdl2::Bad&)
    {
        return SC_ERR_ARG;
    }
    if (rd.at != n_in)
        return SC_ERR_ARG;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
