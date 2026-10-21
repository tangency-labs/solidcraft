// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `intersect.surface_drop_lib_p1`: the file-local class SmDropPt
// (constructor, operator=, SetUninitialized, IsGoodTangentDrop, IsMovingAlongBoundary,
// TestSpanAccuracy, CheckSeamJump, FixBoundaryTangents), the static helpers sm_PushPoint,
// sm_PopStack, sm_StackLook, sm_FixBoundaryCurves and sm_RemoveDuplicateDropPointSolutions,
// and SmSurface::DropIsoCurve / DropAndTrimCurve of upstream
// source/SMLib/src/SmSurfaceDropLib.cpp, on real SmPlane, SmBSplineSurface and
// SmSurfOfRevolution surfaces and SmLine, SmCircle and SmBSplineCurve curves.
//
// SmDropPt and the helpers are local to that file, so this translation unit compiles the
// upstream source itself (same flags as the kernel), at global scope:
//  * every header the file includes is included first, so its own #includes are no-ops;
//  * SmSurface::DropCurve / DropAndTrimCurve / DropIsoCurve (which the file defines) are
//    renamed before any header is read, so this TU's definitions get new names (same
//    vtable slot for the virtual DropCurve; this TU's copies are the ones called here);
//  * SmDropPt and my_AssertAndDrawDropCurve are renamed so they cannot clash with the
//    library's copies.
//
// One entry point, sc_intersect_surface_drop_lib_p1_run(in, n_in, out, cap, out_len): one
// op per call. The layouts are documented in
// crates/solidcore-ffi/src/units/intersect_surface_drop_lib_p1.rs, which validates the
// inputs; every read here is bounds checked as well (a bad program is SC_ERR_ARG). Inputs
// that reach a path the port does not model (a surface evaluation whose failure upstream
// ignores, going on with the SM_UNDEF_DOUBLE defaults or whatever the evaluator wrote before
// failing) or that are undefined upstream (sm_FixBoundaryCurves reading past its array)
// report REJECTED.
#define DropCurve sc_sdl1_DropCurve
#define DropAndTrimCurve sc_sdl1_DropAndTrimCurve
#define DropIsoCurve sc_sdl1_DropIsoCurve

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
#include <string>
#include <vector>

#define SmDropPt sc_sdl1_SmDropPt
#define my_AssertAndDrawDropCurve sc_sdl1_my_AssertAndDrawDropCurve
#include "../../../upstream/source/SMLib/src/SmSurfaceDropLib.cpp"

#include "sc_sm.h"

using sc::put_uv;
using sc::put_vec;
using sc::st;

namespace sc_sdl1
{
enum
{
    OP_CTOR = 1,
    OP_ASSIGN = 2,
    OP_UNINIT = 3,
    OP_GOOD_TANGENT = 4,
    OP_MOVING = 5,
    OP_SPAN = 6,
    OP_SEAM = 7,
    OP_FIX_TANGENTS = 8,
    OP_STACK = 9,
    OP_FIX_CURVES = 10,
    OP_DUPS = 11,
    OP_ISO = 12,
    OP_TRIM = 13,
};

// Op status of an op the shim does not run (undefined upstream).
const double REJECTED = -2.0;
// Record of an op whose surface of revolution could not be made.
const double NO_SURFACE = -3.0;
// Trim curve entry for a NULL curve (in place of `GetType()` and the curve data).
const double NULL_CURVE = -4.0;

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

// `nbytes bytes`: a file path.
std::string read_path(Rd& rd)
{
    int64_t n = rd.idx(1, 4096);
    std::string p;
    for (int64_t i = 0; i < n; i++)
        p.push_back((char)rd.idx(1, 255));
    return p;
}

// `0 a[3] b[3]` SmLine (its NURB made), `1 c[3] x[3] y[3] d0 d1 r` SmCircle,
// `2 p ncp cpts(x y z w) knots` SmBSplineCurve (dim 3), or `3 path` SmBSplineCurve::ReadFromFile.
SmCurve* read_curve(Rd& rd, const SmContext& ctx, Owned& own)
{
    int64_t kind = rd.idx(0, 3);
    if (kind == 3)
    {
        std::string path = read_path(rd);
        SmBSplineCurve* c = NULL;
        SmBSplineCurve::ReadFromFile(ctx, path.c_str(), c);
        if (c == NULL)
            throw Bad();
        own.curves.push_back(c);
        return c;
    }
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

// `0 o x y scale dom` SmPlane, `1 p q nu nv net(x y z w) u v` SmBSplineSurface,
// `2 gen point[3] axis[3] swap` SmSurfOfRevolution::CreateCanonical with the generator a
// line `0 a b` or a circle `1 c x y d0 d1 r` (see read_curve), or `3 path`
// SmBSplineSurface::ReadFromFile. NULL when CreateCanonical fails.
SmSurface* read_surface(Rd& rd, const SmContext& ctx, Owned& own)
{
    int64_t kind = rd.idx(0, 3);
    if (kind == 3)
    {
        std::string path = read_path(rd);
        SmBSplineSurface* s = NULL;
        SmBSplineSurface::ReadFromFile(ctx, path.c_str(), s);
        if (s == NULL)
            throw Bad();
        own.surfaces.push_back(s);
        return s;
    }
    if (kind == 0)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        SmVector2d s = rd.uv();
        SmExtent2d d = rd.dom4();
        SmSurface* srf = new (ctx) SmPlane(o, x, y, s, d, &ctx);
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
void read_pt(Rd& rd, sc_sdl1_SmDropPt& p)
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

void put_pt(std::vector<double>& o, const sc_sdl1_SmDropPt& p)
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

SmSurfParamType test_bdry(int64_t c)
{
    switch (c)
    {
    case 0:
        return SM_SP_U;
    case 1:
        return SM_SP_V;
    case 2:
        return SM_SP_UNKNOWN;
    case 3:
        return SM_SP_BOTH;
    default:
        return SM_SP_NEITHER;
    }
}

// The first-side sector of IsMovingAlongBoundary (TRUE when it is SM_POC_INSIDE).
bool first_inside(double ang, double lim, bool inside_first)
{
    if (ang > -SM_PI + lim && ang < 0.0 - lim)
        return inside_first;
    if (ang > 0.0 + lim && ang < SM_PI - lim)
        return !inside_first;
    return false;
}

// True when one of IsMovingAlongBoundary's evaluations (whose status it ignores) fails, so
// it would go on with the vectors' SM_UNDEF_DOUBLE defaults or partial writes (not ported):
// the evaluations it makes, in its order.
bool moving_undefined(const SmSurface& s, const sc_sdl1_SmDropPt& p, const SmExtent2d& d, double tx,
                      double ty, double ang_deg, SmSurfParamType e)
{
    SmSurfParamType t = (e != SM_SP_U && e != SM_SP_V) ? SM_SP_BOTH : e;
    double lim = SM_DEG2RAD(ang_deg);
    const SmExtent1d& ur = d.GetUInterval();
    const SmExtent1d& vr = d.GetVInterval();
    SmPoint3d pt;
    SmVector3d su, sv, nrm;
    if ((t == SM_SP_U || t == SM_SP_BOTH) && ur.IsValueOnBoundary(p.m_UVPD[0].x, tx))
    {
        SmBoolean low = (p.m_UVPD[0].x < ur.GetMid());
        if (s.Evaluate1stDerivatives(p.m_UVPD[0], low, FALSE, pt, su, sv) != SM_SUCCESS ||
            s.EvaluateNormal(p.m_UVPD[0], low, FALSE, nrm) != SM_SUCCESS)
            return true;
        if (sv.LengthSquared() < SM_EFF_ZERO_SQ || p.m_CrvPD[1].LengthSquared() < SM_EFF_ZERO_SQ)
            return false;
        double a = 0.0;
        if (nrm.CCWAngleBetween(sv, p.m_CrvPD[1], a) != SM_SUCCESS)
            return true;
        if (first_inside(a, lim, low != FALSE))
        {
            SmPoint2d o(low ? ur.GetMax() : ur.GetMin(), p.m_UVPD[0].y);
            if (s.Evaluate1stDerivatives(o, !low, FALSE, pt, su, sv) != SM_SUCCESS ||
                s.EvaluateNormal(o, !low, FALSE, nrm) != SM_SUCCESS)
                return true;
            if (nrm.CCWAngleBetween(sv, p.m_CrvPD[1], a) != SM_SUCCESS)
                return true;
        }
        return false;
    }
    if ((t == SM_SP_V || t == SM_SP_BOTH) && vr.IsValueOnBoundary(p.m_UVPD[0].y, ty))
    {
        SmBoolean low = (p.m_UVPD[0].y < vr.GetMid());
        if (s.Evaluate1stDerivatives(p.m_UVPD[0], FALSE, low, pt, su, sv) != SM_SUCCESS ||
            s.EvaluateNormal(p.m_UVPD[0], FALSE, low, nrm) != SM_SUCCESS)
            return true;
        double a = 0.0;
        if (nrm.CCWAngleBetween(su, p.m_CrvPD[1], a) != SM_SUCCESS)
            return true;
        if (first_inside(a, lim, low == FALSE))
        {
            SmPoint2d o(p.m_UVPD[0].x, low ? vr.GetMax() : vr.GetMin());
            if (s.Evaluate1stDerivatives(o, !low, FALSE, pt, su, sv) != SM_SUCCESS ||
                s.EvaluateNormal(o, !low, FALSE, nrm) != SM_SUCCESS)
                return true;
            if (nrm.CCWAngleBetween(su, p.m_CrvPD[1], a) != SM_SUCCESS)
                return true;
        }
    }
    return false;
}

// True when one of TestSpanAccuracy's domain clamp checks would evaluate a point that fails
// to evaluate (an ignored status; the point keeps its SM_UNDEF_DOUBLE defaults or partial
// writes, a path not ported): any of the three
// Hermite samples outside the domain whose point or clamped point does not evaluate.
bool span_undefined(const SmSurface& s, const SmExtent2d& d, const sc_sdl1_SmDropPt& a,
                    const sc_sdl1_SmDropPt& b)
{
    SmExtent1d ivl(a.m_dT, b.m_dT);
    double dt = b.m_dT - a.m_dT;
    if (dt < ivl.GetLength() / 1000.0)
        return false;
    SmHermiteCurve h(a.m_UVPD[0], a.m_UVPD[1] * dt, b.m_UVPD[0], b.m_UVPD[1] * dt);
    for (ULONG ii = 0; ii < 3; ii++)
    {
        double nt = (ii + 1.0) / (3 + 1.0);
        SmPoint3d hp;
        h.EvaluatePoint(nt, hp);
        SmPoint2d uv(hp.x, hp.y);
        if (!d.ContainsPoint2d(uv, SM_EFF_ZERO))
        {
            SmPoint3d p1, p2;
            if (s.EvaluatePoint(uv, p1) != SM_SUCCESS || s.EvaluatePoint(d.ClampPoint2d(uv), p2) != SM_SUCCESS)
                return true;
        }
    }
    return false;
}

void run(Rd& rd, std::vector<double>& o)
{
    SmContext ctx;
    Owned own;
    int64_t op = rd.idx(OP_CTOR, OP_TRIM);
    switch (op)
    {
    case OP_CTOR:
    {
        double t = rd.num();
        SmBoolean degen = tri(rd);
        sc_sdl1_SmDropPt p(t, degen);
        put_pt(o, p);
        return;
    }
    case OP_ASSIGN:
    {
        sc_sdl1_SmDropPt src(0.0, FALSE);
        read_pt(rd, src);
        sc_sdl1_SmDropPt dst(1.0, TRUE);
        dst = src;
        dst = dst;
        put_pt(o, dst);
        return;
    }
    case OP_UNINIT:
    {
        sc_sdl1_SmDropPt p(0.0, FALSE);
        read_pt(rd, p);
        p.SetUninitialized();
        put_pt(o, p);
        return;
    }
    case OP_GOOD_TANGENT:
    {
        sc_sdl1_SmDropPt p(0.0, FALSE);
        read_pt(rd, p);
        SmBoolean pole = tri(rd);
        o.push_back(p.IsGoodTangentDrop(pole) ? 1.0 : 0.0);
        return;
    }
    case OP_MOVING:
    {
        SmSurface* s = read_surface(rd, ctx, own);
        sc_sdl1_SmDropPt p(0.0, FALSE);
        read_pt(rd, p);
        SmExtent2d d = rd.dom4();
        double tx = rd.num();
        double ty = rd.num();
        double ang = rd.num();
        SmSurfParamType e = test_bdry(rd.idx(0, 4));
        if (s == NULL)
        {
            o.push_back(NO_SURFACE);
            return;
        }
        if (moving_undefined(*s, p, d, tx, ty, ang, e))
        {
            o.push_back(REJECTED);
            return;
        }
        o.push_back((double)p.IsMovingAlongBoundary(*s, d, tx, ty, ang, e));
        return;
    }
    case OP_SPAN:
    {
        SmSurface* s = read_surface(rd, ctx, own);
        SmCurve* c = read_curve(rd, ctx, own);
        SmExtent2d d = rd.dom4();
        sc_sdl1_SmDropPt a(0.0, FALSE), b(0.0, FALSE);
        read_pt(rd, a);
        read_pt(rd, b);
        double tol = rd.num();
        double tx = rd.num();
        double ty = rd.num();
        SmBoolean refine = (SmBoolean)rd.idx(0, 1);
        if (s == NULL)
        {
            o.push_back(NO_SURFACE);
            return;
        }
        if (span_undefined(*s, d, a, b))
        {
            o.push_back(REJECTED);
            return;
        }
        SmBoolean pass = UNSURE;
        double md = -1.0, mp = -1.0, mv = -1.0;
        SmStatus stt = a.TestSpanAccuracy(*s, d, *c, b, tol, tx, ty, pass, md, mp, mv, refine);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back((double)pass);
            o.push_back(md);
            o.push_back(mp);
            o.push_back(mv);
        }
        return;
    }
    case OP_SEAM:
    {
        SmSurface* s = read_surface(rd, ctx, own);
        int64_t cl = rd.idx(0, 3);
        SmSurfParamType closure = cl == 0 ? SM_SP_U : cl == 1 ? SM_SP_V : cl == 2 ? SM_SP_BOTH : SM_SP_NEITHER;
        double tol = rd.num();
        sc_sdl1_SmDropPt a(0.0, FALSE), b(0.0, FALSE);
        read_pt(rd, a);
        read_pt(rd, b);
        if (s == NULL)
        {
            o.push_back(NO_SURFACE);
            return;
        }
        SmStatus stt = a.CheckSeamJump(s, closure, tol, b);
        o.push_back(st(stt));
        put_pt(o, b);
        return;
    }
    case OP_FIX_TANGENTS:
    {
        SmExtent2d d = rd.dom4();
        sc_sdl1_SmDropPt a(0.0, FALSE), b(0.0, FALSE);
        read_pt(rd, a);
        read_pt(rd, b);
        double tx = rd.num();
        double ty = rd.num();
        double tol = rd.num();
        SmBoolean ch = UNSURE;
        SmStatus stt = a.FixBoundaryTangents(d, b, tx, ty, tol, ch);
        o.push_back(st(stt));
        o.push_back((double)ch);
        put_pt(o, a);
        put_pt(o, b);
        return;
    }
    case OP_STACK:
    {
        SmTArray<double> stack;
        int64_t n = rd.idx(0, 256);
        for (int64_t i = 0; i < n; i++)
        {
            int64_t k = rd.idx(0, 2);
            if (k == 0)
            {
                double t = rd.num();
                SmPoint2d uv[2];
                uv[0] = rd.uv();
                uv[1] = rd.uv();
                sm_PushPoint(t, uv, stack);
                continue;
            }
            double t = 0.0;
            SmPoint2d uv[2];
            uv[0].Set(0.0, 0.0);
            uv[1].Set(0.0, 0.0);
            SmStatus stt = k == 1 ? sm_PopStack(t, uv, stack) : sm_StackLook(t, uv, stack);
            o.push_back(st(stt));
            if (stt == SM_SUCCESS)
            {
                o.push_back(t);
                put_uv(o, uv[0]);
                put_uv(o, uv[1]);
            }
        }
        o.push_back((double)stack.GetSize());
        for (ULONG i = 0; i < stack.GetSize(); i++)
            o.push_back(stack[i]);
        return;
    }
    case OP_FIX_CURVES:
    {
        SmExtent2d d = rd.dom4();
        double tx = rd.num();
        double ty = rd.num();
        int64_t n = rd.idx(0, 256);
        SmTArray<SmPoint3d> poly;
        for (int64_t i = 0; i < n; i++)
            poly.Add(rd.vec());
        if (n >= 4 && n % 3 == 0)
        {
            o.push_back(REJECTED);
            return;
        }
        SmBoolean ch = FALSE;
        sm_FixBoundaryCurves(poly, d, tx, ty, ch);
        o.push_back((double)ch);
        for (ULONG i = 0; i < poly.GetSize(); i++)
            put_vec(o, poly[i]);
        return;
    }
    case OP_DUPS:
    {
        double tx = rd.num();
        double ty = rd.num();
        int64_t n = rd.idx(0, 256);
        SmSolutionArray sols;
        for (int64_t i = 0; i < n; i++)
        {
            SmSolution s;
            s.m_vStart[0] = rd.num();
            s.m_vStart[1] = rd.num();
            s.m_vStart.m_dSolutionValue = (double)i;
            sols.Add(s);
        }
        sm_RemoveDuplicateDropPointSolutions(sols, tx, ty);
        o.push_back((double)sols.GetSize());
        for (ULONG i = 0; i < sols.GetSize(); i++)
        {
            o.push_back(sols[i].m_vStart.m_dSolutionValue);
            o.push_back(sols[i].m_vStart[0]);
            o.push_back(sols[i].m_vStart[1]);
        }
        return;
    }
    case OP_ISO:
    {
        SmSurface* s = read_surface(rd, ctx, own);
        SmCurve* c = read_curve(rd, ctx, own);
        SmExtent2d d = rd.dom4();
        double i0 = rd.num();
        double i1 = rd.num();
        SmPoint2d uv = rd.uv();
        double tol = rd.num();
        if (s == NULL)
        {
            o.push_back(NO_SURFACE);
            return;
        }
        SmBoolean ok = UNSURE;
        double md = -1.0, mv = -1.0;
        SmBSplineCurve* iso = NULL;
        SmStatus stt = s->sc_sdl1_DropIsoCurve(ctx, d, *c, SmExtent1d(i0, i1), uv, tol, ok, md, iso, &mv);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back((double)ok);
            o.push_back(md);
            o.push_back(mv);
            o.push_back(iso != NULL ? 1.0 : 0.0);
            if (iso != NULL)
                put_curve(o, iso);
        }
        delete iso;
        return;
    }
    case OP_TRIM:
    {
        SmSurface* s = read_surface(rd, ctx, own);
        SmCurve* c = read_curve(rd, ctx, own);
        bool has_dom = rd.idx(0, 1) != 0;
        SmExtent2d d = has_dom ? rd.dom4() : SmExtent2d();
        bool has_ivl = rd.idx(0, 1) != 0;
        double i0 = has_ivl ? rd.num() : 0.0;
        double i1 = has_ivl ? rd.num() : 0.0;
        double tol = rd.num();
        SmBoolean keep = (SmBoolean)rd.idx(0, 1);
        SmBoolean on = (SmBoolean)rd.idx(0, 1);
        if (s == NULL)
        {
            o.push_back(NO_SURFACE);
            return;
        }
        if (!has_dom)
            d = s->GetNaturalUVDomain();
        SmExtent1d ivl = has_ivl ? SmExtent1d(i0, i1) : c->GetNaturalInterval();
        double md = -1.0, mv = -1.0;
        SmTArray<SmBSplineCurve*> uvs;
        SmStatus stt = s->sc_sdl1_DropAndTrimCurve(ctx, d, *c, ivl, tol, md, mv, uvs, keep, on, NULL);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(md);
            o.push_back(mv);
            o.push_back((double)uvs.GetSize());
            // per curve: its GetType() then its data; a NULL entry (a failed SM_CAST_PTR
            // upstream) is the sentinel NULL_CURVE
            for (ULONG i = 0; i < uvs.GetSize(); i++)
            {
                if (uvs[i] == NULL)
                {
                    o.push_back(NULL_CURVE);
                    continue;
                }
                o.push_back((double)uvs[i]->GetType());
                put_curve(o, uvs[i]);
            }
        }
        for (ULONG i = 0; i < uvs.GetSize(); i++)
            delete uvs[i];
        return;
    }
    default:
        throw Bad();
    }
}
} // namespace sc_sdl1

SC_EXPORT int32_t sc_intersect_surface_drop_lib_p1_run(const double* in, int64_t n_in, double* out, int64_t cap,
                                                       int64_t* out_len)
{
    if (in == nullptr || n_in < 1 || out_len == nullptr || (out == nullptr && cap > 0))
        return SC_ERR_ARG;
    SC_TRY
    sc_sdl1::Rd rd{in, n_in, 0};
    std::vector<double> o;
    try
    {
        sc_sdl1::run(rd, o);
    }
    catch (const sc_sdl1::Bad&)
    {
        return SC_ERR_ARG;
    }
    if (rd.at != n_in)
        return SC_ERR_ARG;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
