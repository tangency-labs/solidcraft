// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_solver_p4`: SmFilletSolver::ComputePointValues
// (source/SMLib/src/SmFilletSolver.cpp:3544-3768) on a real SmConstantRadiusFS built with its
// surface-only constructor (SmFilletStandardSolver.h:38) over real SmPlane, SmBSplineSurface
// and SmSurfOfRevolution (cylinder, cone, sphere and torus shapes, built with
// SmSurfOfRevolution::CreateCanonical or its constructor as surface_offset_surface.cpp does)
// objects, plus a probe of the function-static `sdStaticStep` (ND-3). The finite-difference
// branch runs on `FdFS`, an SmConstantRadiusFS whose only override is
// OffsetRadiiCanChange() == TRUE (SetupOffsetValues stays SmConstantRadiusFS's).
//
// Entry points (layouts in crates/solidcore-ffi/src/units/blend_fillet_solver_p4.rs; every
// read is bounds checked here, a bad program is SC_ERR_ARG):
//  * sc_blend_fillet_solver_p4_point_values(in, n_in, out, cap, out_len)
//  * sc_blend_fillet_solver_p4_static_step(step): the value `sdStaticStep` of the library's
//    ComputePointValues holds, latching it first (with 1e-5) when nothing has latched it.
//    See probe() below.
// The calc_geom op lives in blend_fillet_solver_p4_calc.cpp (it needs sc_edgeuse_graph.h).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFilletSolver.h>
#include <SmFilletStandardSolver.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmOffsetSurface.h>
#include <SmPlane.h>
#include <SmSurfOfRevolution.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <cmath>
#include <memory>
#include <stdexcept>
#include <vector>

#include "sc_sm.h"

using sc::put_uv;
using sc::put_vec;
using sc::st;

namespace
{
struct Bad : std::runtime_error
{
    Bad() : std::runtime_error("bad program") {}
};

// A bounds-checked reader (the program is validated on the Rust side as well).
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
    bool flag() { return idx(0, 1) != 0; }
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

// Surface kinds (keep in sync with the ffi module).
enum : int64_t
{
    SK_PLANE = 0,
    SK_BSPLINE = 1,
    SK_SAME = 2,
    SK_REVOLUTION = 3,
};

// Generator kinds and construction modes of a surface-of-revolution block (the layout of
// surface_offset_surface.cpp / surface_surf_of_revolution_p1.cpp).
enum : int64_t
{
    GK_BSPLINE = 0,
    GK_LINE = 1,
    GK_CIRCLE = 2,
};
enum : int64_t
{
    BM_CANONICAL = 0,
    BM_CTOR = 1,
};

// Reaches the protected SmSurfOfRevolution constructor.
struct PeekRev : SmSurfOfRevolution
{
    PeekRev(SmBSplineCurve* c, const SmPoint3d& o, const SmVector3d& x, const SmVector3d& y,
            const SmExtent2d& d, SmBoolean swap)
        : SmSurfOfRevolution(c, o, x, y, d, swap)
    {
    }
};

SmBSplineCurve* read_rev_bspline(Rd& rd, const SmContext& ctx)
{
    gw_INDEX n = (gw_INDEX)rd.idx(1, 256);
    gw_DEGREE p = (gw_DEGREE)rd.idx(1, 9);
    gw_INDEX m = (gw_INDEX)rd.idx(2, 256 + 9 + 1);
    if (p > n || m != n + p + 1)
        throw Bad();
    std::vector<double> v;
    for (int64_t i = 0; i < 4 * ((int64_t)n + 1) + (int64_t)m + 1; i++)
        v.push_back(rd.num());
    gw_CURVE* cur = sm_AllocateNurbCurve(n, p, m);
    size_t k = 0;
    for (gw_INDEX i = 0; i <= n; i++)
    {
        cur->pol->Pw[i].x = v[k++];
        cur->pol->Pw[i].y = v[k++];
        cur->pol->Pw[i].z = v[k++];
        cur->pol->Pw[i].w = v[k++];
    }
    for (gw_INDEX i = 0; i <= m; i++)
        cur->knt->U[i] = v[k++];
    return new (ctx) SmBSplineCurve(cur, 3, FALSE);
}

SmBSplineCurve* read_gen(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx(0, 2);
    switch (kind)
    {
    case GK_BSPLINE: return read_rev_bspline(rd, ctx);
    case GK_LINE:
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine* l = new (ctx) SmLine(a, b, 3, &ctx);
        l->GetOrCreateGwNurbPointer();
        return l;
    }
    default:
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double r = rd.num();
        return new (ctx) SmCircle(c, x, y, SmExtent1d(d0, d1), r, 3, &ctx);
    }
    }
}

// NULL with `stt` set when the construction fails.
SmSurface* read_revolution(Rd& rd, const SmContext& ctx, SmStatus& stt)
{
    int64_t mode = rd.idx(0, 1);
    std::unique_ptr<SmBSplineCurve> gen(read_gen(rd, ctx));
    bool oob = rd.flag();
    bool swap = rd.flag();
    SmSurfOfRevolution* s = NULL;
    stt = SM_SUCCESS;
    if (mode == BM_CANONICAL)
    {
        SmPoint3d p = rd.vec();
        SmVector3d a = rd.vec();
        stt = SmSurfOfRevolution::CreateCanonical(ctx, gen.get(), p, a, s, swap ? TRUE : FALSE);
        if (s)
            gen.release(); // ownership transfers even if MakeNurb then fails
    }
    else
    {
        SmPoint3d org = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        SmExtent2d d = rd.dom4();
        s = new (ctx) PeekRev(gen.get(), org, x, y, d, swap ? TRUE : FALSE);
        gen.release();
        stt = s->MakeNurb();
    }
    if (stt != SM_SUCCESS)
    {
        delete s;
        return NULL;
    }
    s->SetOutOfBoundsEnabled(oob ? TRUE : FALSE);
    return s;
}

// `0 o x y scale dom` (SmPlane), `1 p q nu nv net(x y z w) u v` (SmBSplineSurface) or
// `3 <revolution block>`. NULL with `stt` when a surface of revolution fails to build.
SmSurface* read_surface(Rd& rd, const SmContext& ctx, int64_t kind, SmStatus& stt)
{
    stt = SM_SUCCESS;
    if (kind == SK_PLANE)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        SmVector2d s = rd.uv();
        SmExtent2d d = rd.dom4();
        return new (ctx) SmPlane(o, x, y, s, d, &ctx);
    }
    if (kind == SK_REVOLUTION)
        return read_revolution(rd, ctx, stt);
    gw_DEGREE p = (gw_DEGREE)rd.idx(1, 8);
    gw_DEGREE q = (gw_DEGREE)rd.idx(1, 8);
    gw_INDEX nu = (gw_INDEX)rd.idx(2, 17);
    gw_INDEX nv = (gw_INDEX)rd.idx(2, 17);
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
    return new (ctx) SmBSplineSurface(sur, FALSE, &ctx);
}

// A point: `type ptype[4] uv0 duv0 uv1 duv1 srf0[3][3] srf1[3][3] crv_pos crv_deriv param
// angle deviation user[4]` (80 values; the layout of blend_fillet_intersector.cpp).
void read_point(Rd& rd, SmTsectPnt& t)
{
    t.m_ePointType = (SmIntersectionPointType)rd.idx(0, 7);
    for (int s = 0; s < 2; s++)
        for (int d = 0; d < 2; d++)
            t.m_eUVParamType[s][d] = (SmExtentPointType)rd.idx(0, 5);
    for (int s = 0; s < 2; s++)
        for (int d = 0; d < 2; d++)
            t.m_vUVCurvePV[s][d] = rd.uv();
    for (int s = 0; s < 2; s++)
        for (int i = 0; i < 3; i++)
            for (int j = 0; j < 3; j++)
                t.m_vSurfacePV[s][i][j] = rd.vec();
    t.m_v3DCurvePV[0] = rd.vec();
    t.m_v3DCurvePV[1] = rd.vec();
    t.m_dCurveParameter = rd.num();
    t.m_dTangentPlaneAngleRad = rd.num();
    t.m_dDeviation = rd.num();
    for (int i = 0; i < SM_MAX_USER_DOUBLES; i++)
        t.m_adUserDoubles[i] = rd.num();
}

void put_point(std::vector<double>& o, const SmTsectPnt& t)
{
    o.push_back((double)t.m_ePointType);
    for (int s = 0; s < 2; s++)
        for (int d = 0; d < 2; d++)
            o.push_back((double)t.m_eUVParamType[s][d]);
    for (int s = 0; s < 2; s++)
        for (int d = 0; d < 2; d++)
            put_uv(o, t.m_vUVCurvePV[s][d]);
    for (int s = 0; s < 2; s++)
        for (int i = 0; i < 3; i++)
            for (int j = 0; j < 3; j++)
                put_vec(o, t.m_vSurfacePV[s][i][j]);
    put_vec(o, t.m_v3DCurvePV[0]);
    put_vec(o, t.m_v3DCurvePV[1]);
    o.push_back(t.m_dCurveParameter);
    o.push_back(t.m_dTangentPlaneAngleRad);
    o.push_back(t.m_dDeviation);
    for (int i = 0; i < SM_MAX_USER_DOUBLES; i++)
        o.push_back(t.m_adUserDoubles[i]);
}

// SmConstantRadiusFS with OffsetRadiiCanChange() == TRUE: ComputePointValues takes its
// finite-difference branch (as for SmConstantDistanceFS / SmVariableRadiusFS) while
// SetupOffsetValues keeps the constant radii.
struct FdFS : SmConstantRadiusFS
{
    FdFS(const SmContext& ctx, double tol, double ang, double tan, double r0, double r1,
         const SmSurface& a, const SmSurface& b, SmBoolean o0, SmBoolean o1)
        : SmConstantRadiusFS(ctx, tol, ang, tan, r0, r1, a, b, o0, o1)
    {
    }
    virtual SmBoolean OffsetRadiiCanChange() { return TRUE; }
};

// Owns the two surfaces (B may be A).
struct Surfs
{
    SmSurface* a = NULL;
    SmSurface* b = NULL;
    ~Surfs()
    {
        if (b != a)
            delete b;
        delete a;
    }
};

// ---- the sdStaticStep probe -----------------------------------------------------------------

// The finite-difference block of ComputePointValues (SmFilletSolver.cpp:3577-3683, crossing
// point, no tangency) replayed with `dStaticStep` in place of `sdStaticStep`: the CrvDeriv the
// library would produce had its static held that value.
SmStatus replay(SmFilletSolver& fs, SmPoint2d uv[2], const SmExtent2d& d1, const SmExtent2d& d2,
                double dir, double dStaticStep, SmTsectPnt& r)
{
    SER(fs.ComputeSurfaceValues(uv, r));
    r.CrvDeriv() = (r.SrfNorm(0) * r.SrfNorm(1)) * dir;
    if (r.CrvDeriv().LengthSquared() < 10000.0 * SM_EFF_ZERO_SQ)
        return SM_ERR;
    SER(r.CrvDeriv().Unitize());
    SER(fs.ComputeUVVectors(r));
    double dStep = dStaticStep;
    SmTsectPnt sStepForw;
    SmBoolean bFoundForw, bClipped, bBoundaryHit;
    SER(fs.StepSolve(r, dStep, d1, d2, bFoundForw, bClipped, bBoundaryHit, sStepForw));
    if (!bFoundForw && dStep < 1.0e-4)
    {
        double dBigStep = 1.0e-3;
        SER(fs.StepSolve(r, dStep, d1, d2, bFoundForw, bClipped, bBoundaryHit, sStepForw));
        if (bFoundForw)
            dStep = dBigStep;
    }
    SmPoint3d sSrf0PtForw, sSrf1PtForw;
    if (bFoundForw)
    {
        SER(fs.GetSurface(0)->EvaluatePoint(sStepForw.UVPos(0), sSrf0PtForw));
        SER(fs.GetSurface(1)->EvaluatePoint(sStepForw.UVPos(1), sSrf1PtForw));
    }
    dStep = -dStep;
    SmTsectPnt sStepBack;
    SmBoolean bFoundBack;
    SER(fs.StepSolve(r, dStep, d1, d2, bFoundBack, bClipped, bBoundaryHit, sStepBack));
    SmPoint3d sSrf0PtBack, sSrf1PtBack;
    if (bFoundBack)
    {
        SER(fs.GetSurface(0)->EvaluatePoint(sStepBack.UVPos(0), sSrf0PtBack));
        SER(fs.GetSurface(1)->EvaluatePoint(sStepBack.UVPos(1), sSrf1PtBack));
    }
    SmPoint3d sStepPtForw, sStepPtBack;
    if (bFoundForw && bFoundBack)
    {
        sStepPtForw = (sSrf0PtForw + sSrf1PtForw) / 2.0;
        sStepPtBack = (sSrf0PtBack + sSrf1PtBack) / 2.0;
        dStep = 2.0 * dStep;
    }
    else if (bFoundForw)
    {
        sStepPtForw = (sSrf0PtForw + sSrf1PtForw) / 2.0;
        sStepPtBack = r.CrvPos();
    }
    else if (bFoundBack)
    {
        sStepPtForw = r.CrvPos();
        sStepPtBack = (sSrf0PtBack + sSrf1PtBack) / 2.0;
    }
    else
        return SM_ERR;
    dStep = smos_Fabs(dStep);
    SmVector3d sDiff = sStepPtForw - sStepPtBack;
    r.CrvDeriv() = sDiff / dStep;
    SER(r.CrvDeriv().Unitize());
    return SM_SUCCESS;
}

double dist2(const SmVector3d& a, const SmVector3d& b)
{
    SmVector3d d = a - b;
    return d.LengthSquared();
}

// Runs the library's ComputePointValues once on a fixed finite-difference crossing point
// (latching `sdStaticStep` with 1e-5 if nothing latched it yet) and tells which of the two
// values it can hold (1e-3 or 1e-5) produced its curve tangent, by replaying the block with
// each. The surfaces are a cubic B-spline sheet A(u, v) = (u, v, u^3) and the plane
// y = 0.25; the offsets are zero, so (0.4, 0.25) / (0.4, 0.064) is on the intersection curve
// (u, 0.25, u^3), whose curvature varies, so the two steps' chords differ.
int32_t probe(double* step)
{
    SmContext ctx;
    Surfs s;
    {
        gw_SURFACE* sur = sm_AllocateNurbSurface(3, 1, 3, 1, 7, 3);
        const double z[4] = {0.0, 0.0, 0.0, 1.0};
        for (gw_INDEX i = 0; i <= 3; i++)
            for (gw_INDEX j = 0; j <= 1; j++)
            {
                sur->net->Pw[i][j].x = (double)i / 3.0;
                sur->net->Pw[i][j].y = (double)j;
                sur->net->Pw[i][j].z = z[i];
                sur->net->Pw[i][j].w = NL_NOW;
            }
        const double ku[8] = {0, 0, 0, 0, 1, 1, 1, 1};
        const double kv[4] = {0, 0, 1, 1};
        for (int i = 0; i < 8; i++)
            sur->knu->U[i] = ku[i];
        for (int i = 0; i < 4; i++)
            sur->knv->U[i] = kv[i];
        s.a = new (ctx) SmBSplineSurface(sur, FALSE, &ctx);
    }
    s.b = new (ctx) SmPlane(SmPoint3d(0.0, 0.25, 0.0), SmVector3d(1.0, 0.0, 0.0),
                            SmVector3d(0.0, 0.0, 1.0), SmVector2d(1.0, 1.0),
                            SmExtent2d(-1.0, -1.0, 2.0, 2.0), &ctx);
    SmExtent2d d1(0.0, 0.0, 1.0, 1.0);
    SmExtent2d d2(-1.0, -1.0, 2.0, 2.0);
    FdFS fs(ctx, 1e-5, 0.1, 0.03, 0.0, 0.0, *s.a, *s.b, TRUE, TRUE);
    SmTsectPnt in;
    in.m_ePointType = SM_IP_CROSSING;
    SmPoint2d uv[2] = {SmPoint2d(0.4, 0.25), SmPoint2d(0.4, 0.064)};
    SmTsectPnt lib = in;
    SmStatus stt = fs.ComputePointValues(uv, d1, d2, 1.0, lib, NULL);
    if (stt != SM_SUCCESS)
        return (int32_t)stt;
    const double cand[2] = {1.0e-3, 1.0e-5};
    double d[2];
    for (int i = 0; i < 2; i++)
    {
        SmTsectPnt r = in;
        SmPoint2d uvr[2] = {uv[0], uv[1]};
        stt = replay(fs, uvr, d1, d2, 1.0, cand[i], r);
        if (stt != SM_SUCCESS)
            return (int32_t)stt;
        d[i] = dist2(r.CrvDeriv(), lib.CrvDeriv());
    }
    // the matching replay is bit-identical; the other one is far off
    for (int i = 0; i < 2; i++)
        if (d[i] == 0.0 && d[1 - i] > 0.0)
        {
            *step = cand[i];
            return SC_OK;
        }
    return SC_ERR_EXCEPTION;
}
} // namespace

// Program: `surfA surfB` (surfB kind `2` = the same object as A), `dom1[4] dom2[4]`, the
// solver `r0 r1 orient0 orient1 tol ang_tol tangency_tol fd`, `uv0[2] uv1[2] dir`, the point
// (80 values), `has_prev [prev (80 values)]`. Output: `status point[80] offset0 offset1`, or
// `-1 build_status` when a surface of revolution fails to build.
SC_EXPORT int32_t sc_blend_fillet_solver_p4_point_values(const double* in, int64_t n_in,
                                                         double* out, int64_t cap,
                                                         int64_t* out_len)
{
    if (in == NULL || n_in < 2 || out_len == NULL || cap < 0 || (cap > 0 && out == NULL))
        return SC_ERR_ARG;
    try
    {
        SmContext ctx;
        std::vector<double> o;
        Rd rd{in, n_in, 0};
        Surfs s;
        SmStatus bstt = SM_SUCCESS;
        int64_t ka = rd.idx(0, 3);
        if (ka == SK_SAME)
            throw Bad();
        s.a = read_surface(rd, ctx, ka, bstt);
        if (s.a == NULL)
        {
            o.push_back(-1.0);
            o.push_back((double)bstt);
            return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
        }
        int64_t kb = rd.idx(0, 3);
        if (kb == SK_SAME)
            s.b = s.a;
        else
        {
            s.b = read_surface(rd, ctx, kb, bstt);
            if (s.b == NULL)
            {
                o.push_back(-1.0);
                o.push_back((double)bstt);
                return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
            }
        }
        SmExtent2d d1 = rd.dom4();
        SmExtent2d d2 = rd.dom4();
        double r0 = rd.num();
        double r1 = rd.num();
        SmBoolean o0 = rd.flag() ? TRUE : FALSE;
        SmBoolean o1 = rd.flag() ? TRUE : FALSE;
        double tol = rd.num();
        double ang = rd.num();
        double tan = rd.num();
        bool fd = rd.flag();
        SmPoint2d uv[2];
        uv[0] = rd.uv();
        uv[1] = rd.uv();
        double dir = rd.num();
        SmTsectPnt pnt;
        read_point(rd, pnt);
        SmTsectPnt prev;
        bool has_prev = rd.flag();
        if (has_prev)
            read_point(rd, prev);
        if (rd.at != rd.n)
            throw Bad();
        auto run = [&](SmConstantRadiusFS& fs) {
            SmStatus stt = fs.ComputePointValues(uv, d1, d2, dir, pnt, has_prev ? &prev : NULL);
            o.push_back(st(stt));
            put_point(o, pnt);
            for (ULONG i = 0; i < 2; i++)
            {
                SmOffsetSurface* os = fs.GetSurface(i);
                o.push_back(os ? os->GetOffsetDistance() : -777.0);
            }
        };
        if (fd)
        {
            FdFS fs(ctx, tol, ang, tan, r0, r1, *s.a, *s.b, o0, o1);
            run(fs);
        }
        else
        {
            SmConstantRadiusFS fs(ctx, tol, ang, tan, r0, r1, *s.a, *s.b, o0, o1);
            run(fs);
        }
        // unlink the points before they go away (they are in no list)
        pnt.ReSet();
        prev.ReSet();
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    catch (const Bad&)
    {
        return SC_ERR_ARG;
    }
    catch (...)
    {
        return SC_ERR_EXCEPTION;
    }
}

// The value of the library's `sdStaticStep` (see probe()); SC_ERR_EXCEPTION when the probe
// cannot tell, an SmStatus when its ComputePointValues fails.
SC_EXPORT int32_t sc_blend_fillet_solver_p4_static_step(double* step)
{
    if (step == NULL)
        return SC_ERR_ARG;
    try
    {
        return probe(step);
    }
    catch (...)
    {
        return SC_ERR_EXCEPTION;
    }
}
