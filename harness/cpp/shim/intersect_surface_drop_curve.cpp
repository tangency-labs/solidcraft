// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `intersect.surface_drop_curve`: the SmSurfaceDropCurve members and the
// static helpers of upstream source/SMLib/src/SmSurfaceDropCurve.cpp
// (source/SMLib/inc/SmSurfaceDropCurve.h).
//
// The helpers sm_TestPoint and sm_IsGrazingIntersection are local to that file and the class keeps
// its working state private, so this translation unit compiles the upstream source itself (same
// flags as the kernel), at global scope:
//  * every header the file includes is included first (except SmSurfaceDropCurve.h), so its own
//    #includes are no-ops;
//  * the class (all its members) and the global sm_AddBreak are renamed with macros, so they do
//    not clash with the copies in the library (SmSurfaceDropCurve.o); the renamed class still
//    derives from the library's SmSurfaceTracer, whose TraceCurve dispatches to its overrides;
//  * `private` is made public for the class definition only (SmSurfaceDropCurve.h is first read
//    here), so the shim can set the working interval, working domain, knot vector and breaks.
// A derived class reaches the protected SmSurfaceTracer members (context, tolerances, trace point
// manager and lists).
//
// The drop curve is built on a real SmBSplineSurface or SmPlane and a real SmLine or
// SmBSplineCurve. One entry point, sc_intersect_surface_drop_curve_run(in, n_in, out, cap,
// out_len). Layouts are documented in crates/solidcore-ffi/src/units/intersect_surface_drop_curve.rs,
// which validates every input; reads are bounds checked here too (a bad program is SC_ERR_ARG).
#define SmSurfaceDropCurve sc_sdc_SmSurfaceDropCurve
#define sm_AddBreak sc_sdc_sm_AddBreak

#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmCacheMgr.h>
#include <SmContext.h>
#include <SmCrvOnSurf.h>
#include <SmCurve.h>
#include <SmCurveCache.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmGeomUtility.h>
#include <SmGraphicsExtern.h>
#include <SmHermiteCurve.h>
#include <SmIsoCurve.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmSurfaceCache.h>
#include <SmSurfaceTracer.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>
#ifdef SM_DEBUG_CODE
#include <SmAssertArray.h>
#include <SmBrep.h>
#include <SmFace.h>
#endif

#include <stdexcept>
#include <vector>

#define private public
#include "../../../upstream/source/SMLib/src/SmSurfaceDropCurve.cpp"

#include "sc_sm.h"

using sc::put_uv;
using sc::put_vec;
using sc::st;
#undef private

namespace sc_sdc
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/intersect_surface_drop_curve.rs).
enum : int32_t
{
    OP_CPV = 1,
    OP_STEP = 2,
    OP_ADD_BREAK = 3,
    OP_TEST_POINT = 4,
    OP_GRAZE = 5,
    OP_LPS = 6,
    OP_NEXT = 7,
    OP_SPAN = 8,
    OP_TRACE2 = 9,
    OP_INTERIOR = 10,
    OP_BOUNDARY = 11,
    OP_DO_TRACE = 12,
    OP_HOOKS = 13,
    OP_LAST = 13,
};

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
    bool flag() { return idx(0, 1) != 0; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
    SmVector2d uv()
    {
        double x = num();
        double y = num();
        return SmVector2d(x, y);
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

double flg(SmBoolean b)
{
    return b ? 1.0 : 0.0;
}

// `type dim ivl[2]` then, for B-splines that are not lines, `has [n p m cpts(x y z w) knots]`.
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    o.push_back((double)c->GetType());
    o.push_back((double)c->GetDim());
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    if (c->IsKindOf(SmLine_TYPE) || !c->IsKindOf(SmBSplineCurve_TYPE))
        return;
    const gw_CURVE* cur = ((const SmBSplineCurve*)c)->GetGwNurbPointer();
    o.push_back(cur ? 1.0 : 0.0);
    if (!cur)
        return;
    gw_INDEX n = cur->pol->n;
    gw_INDEX m = cur->knt->m;
    o.push_back((double)n);
    o.push_back((double)cur->p);
    o.push_back((double)m);
    for (gw_INDEX i = 0; i <= n; i++)
    {
        o.push_back(cur->pol->Pw[i].x);
        o.push_back(cur->pol->Pw[i].y);
        o.push_back(cur->pol->Pw[i].z);
        o.push_back(cur->pol->Pw[i].w);
    }
    for (gw_INDEX i = 0; i <= m; i++)
        o.push_back(cur->knt->U[i]);
}

void put_curves(std::vector<double>& o, const SmTArray<SmCurve*>& a)
{
    o.push_back((double)a.GetSize());
    for (ULONG i = 0; i < a.GetSize(); i++)
        put_curve(o, a[i]);
}

void put_doubles(std::vector<double>& o, const SmTArray<double>& a)
{
    o.push_back((double)a.GetSize());
    for (ULONG i = 0; i < a.GetSize(); i++)
        o.push_back(a[i]);
}

// `has [n vals]` of an optional array.
void put_opt_doubles(std::vector<double>& o, const SmTArray<double>* a)
{
    o.push_back(a ? 1.0 : 0.0);
    if (a)
        put_doubles(o, *a);
}

// `n m p q r s cpts(x y z w) u v`, sizes checked by the Rust side.
SmBSplineSurface* read_bspline_surface(Rd& rd, const SmContext& ctx)
{
    gw_INDEX n = (gw_INDEX)rd.idx(1, 16);
    gw_INDEX m = (gw_INDEX)rd.idx(1, 16);
    gw_DEGREE p = (gw_DEGREE)rd.idx(1, 8);
    gw_DEGREE q = (gw_DEGREE)rd.idx(1, 8);
    gw_INDEX r = (gw_INDEX)rd.idx(3, 40);
    gw_INDEX s = (gw_INDEX)rd.idx(3, 40);
    if (p > n || q > m || r != n + p + 1 || s != m + q + 1)
        throw Bad();
    std::vector<double> v;
    for (int64_t i = 0; i < 4 * (n + 1) * (m + 1) + r + 1 + s + 1; i++)
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

// `0 bspline-surface` or `1 o x y scale[2] dom[4]` (SmPlane(o, x, y, scale, dom, ctx), with its
// NURB made).
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx(0, 1);
    if (kind == 0)
        return read_bspline_surface(rd, ctx);
    SmPoint3d o = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    SmVector2d s = rd.uv();
    SmExtent2d d = rd.dom4();
    SmPlane* p = new (ctx) SmPlane(o, x, y, s, d, &ctx);
    // Make the plane's NURB up front: const queries (ComputePointValues' evaluations) would
    // make and keep it later, changing later point solves; the Rust plane never keeps a NURB
    // made by a read-only query, so both sides start with it (plan/findings.md).
    p->GetOrCreateGwNurbPointer();
    return p;
}

// A curve: `0 a b` (SmLine::CreateLineSegment, dim 3) or `1 n p m cpts(x y z w) knots` (dim 3).
SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx(0, 1);
    if (kind == 0)
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine* l = NULL;
        if (SmLine::CreateLineSegment(ctx, 3, a, b, l) != SM_SUCCESS || l == NULL)
            throw Bad();
        return l;
    }
    int64_t n = rd.idx(1, 32);
    int64_t p = rd.idx(1, 8);
    int64_t m = rd.idx(3, 64);
    if (p > n || m != n + p + 1)
        throw Bad();
    std::vector<double> v;
    for (int64_t i = 0; i < 4 * (n + 1) + m + 1; i++)
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
        cur->knt->U[i] = v[4 * (n + 1) + i];
    SmBSplineCurve* c = new (ctx) SmBSplineCurve(3, (ULONG)cur->pol->n, (ULONG)cur->knt->m, (short)cur->p,
                                                 cur->pol->Pw, cur->knt->U, &ctx);
    smos_Free(cur);
    return c;
}

// `n vals[n]` into a new array of the context.
SmTArray<double>* read_doubles(Rd& rd, const SmContext& ctx)
{
    int64_t n = rd.idx(0, 64);
    SmTArray<double>* a = new (ctx) SmTArray<double>(ctx);
    for (int64_t i = 0; i < n; i++)
        a->Add(rd.num());
    return a;
}

// The drop curve with access to the protected SmSurfaceTracer members.
class Drop : public SmSurfaceDropCurve
{
public:
    Drop(const SmContext& ctx, const SmSurface* s, const SmExtent2d& d, const SmCurve& c, const SmExtent1d& i)
        : SmSurfaceDropCurve(s, d, c, i)
    {
        m_cpContext = &ctx;
    }
    SmTracePnt* NewPoint() { return (SmTracePnt*)m_vTrcPntMgr.GetNewElement(); }
    const SmSurface* Surf() const { return m_cpSurface; }
    double& ApproxTol() { return m_dThisApproxTol3d; }
    double& AngTol() { return m_dThisAngTolRad; }
    SmTList<SmTracePnt>& CurvePoints() { return m_vCurvePoints; }
    SmTList<SmTracePnt>& StartPoints() { return m_vStartPoints; }
};

// A trace point with every vector zeroed (SmTracePnt leaves them uninitialised).
void zero_point(SmTracePnt& p)
{
    SmVector3d z(0.0, 0.0, 0.0);
    SmVector2d z2(0.0, 0.0);
    for (int i = 0; i < 3; i++)
    {
        p.m_v3DCurvePV[i] = z;
        p.m_vUVCurvePV[i] = z2;
        for (int j = 0; j < 3; j++)
            p.m_vSurfacePV[i][j] = z;
    }
}

// `type param drop nld c3d[0](3) c3d[1](3) cuv[0](2) cuv[1](2) S00 S10 S01 S22`.
void put_point(std::vector<double>& o, const SmTracePnt& p)
{
    o.push_back((double)p.m_ePointType);
    o.push_back(p.m_dCurveParameter);
    o.push_back(p.m_dSurfDropDist);
    o.push_back(p.m_dSurfNormLineDist);
    put_vec(o, p.m_v3DCurvePV[0]);
    put_vec(o, p.m_v3DCurvePV[1]);
    put_uv(o, p.m_vUVCurvePV[0]);
    put_uv(o, p.m_vUVCurvePV[1]);
    put_vec(o, p.m_vSurfacePV[0][0]);
    put_vec(o, p.m_vSurfacePV[1][0]);
    put_vec(o, p.m_vSurfacePV[0][1]);
    put_vec(o, p.m_vSurfacePV[2][2]);
}

void put_start_points(std::vector<double>& o, Drop& t)
{
    SmTArray<SmTracePnt*> a;
    t.StartPoints().GetAllNodes(a);
    o.push_back((double)a.GetSize());
    for (ULONG i = 0; i < a.GetSize(); i++)
    {
        o.push_back((double)a[i]->m_ePointType);
        o.push_back(a[i]->m_dCurveParameter);
        put_uv(o, a[i]->m_vUVCurvePV[0]);
        put_uv(o, a[i]->m_vUVCurvePV[1]);
    }
}

struct Acc
{
    SmTArray<SmCurve*> c3;
    SmTArray<SmCurve*> uv;
    SmTArray<double> drop;
    SmTArray<double> dev;
    ~Acc()
    {
        for (ULONG i = 0; i < c3.GetSize(); i++)
            delete c3[i];
        for (ULONG i = 0; i < uv.GetSize(); i++)
            delete uv[i];
    }
};

void put_acc(std::vector<double>& o, const Acc& a)
{
    put_curves(o, a.c3);
    put_curves(o, a.uv);
    put_doubles(o, a.drop);
    put_doubles(o, a.dev);
}

struct Last
{
    bool has = false;
    SmTracePnt pnt;
};

void run_op(int32_t op, Rd& rd, const SmContext& ctx, const SmCurve* crv, Drop& t, Last& last, Acc& acc,
            std::vector<double>& o)
{
    switch (op)
    {
    case OP_CPV:
    {
        double param = rd.num();
        SmPoint2d uv = rd.uv();
        bool use_prev = rd.flag();
        SmTracePnt p;
        zero_point(p);
        p.m_dCurveParameter = param;
        SmTracePnt prev;
        SmTracePnt* pprev = NULL;
        if (use_prev && last.has)
        {
            prev = last.pnt;
            pprev = &prev;
        }
        SmStatus s = t.ComputePointValues(uv, p, pprev, NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            put_point(o, p);
            o.push_back(pprev ? 1.0 : 0.0);
            if (pprev)
                put_uv(o, prev.m_vUVCurvePV[1]);
            last.has = true;
            last.pnt = p;
        }
        break;
    }
    case OP_STEP:
    {
        double param = rd.num();
        double old = rd.num();
        SmTracePnt p;
        zero_point(p);
        p.m_dCurveParameter = param;
        double r = 0.0;
        SmStatus s = t.ComputeStepSize(p, old, r);
        o.push_back(st(s));
        o.push_back(r);
        break;
    }
    case OP_ADD_BREAK:
    {
        SmTArray<double>* a = read_doubles(rd, ctx);
        double v = rd.num();
        sm_AddBreak(*a, v);
        o.push_back(0.0);
        put_doubles(o, *a);
        delete a;
        break;
    }
    case OP_TEST_POINT:
    {
        SmExtent2d d = rd.dom4();
        SmPoint2d uv = rd.uv();
        SmPoint3d pt = rd.vec();
        double tol = rd.num();
        SmBoolean good = FALSE;
        double drop = -1.0, nld = -1.0;
        SmStatus s = sm_TestPoint(*t.Surf(), d, uv, pt, tol, good, drop, nld);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(good));
            o.push_back(drop);
            o.push_back(nld);
        }
        break;
    }
    case OP_GRAZE:
    {
        double p1 = rd.num();
        SmCurve* c2 = read_curve(rd, ctx);
        double p2 = rd.num();
        o.push_back(0.0);
        o.push_back(flg(sm_IsGrazingIntersection(crv, p1, c2, p2)));
        delete c2;
        break;
    }
    case OP_LPS:
    {
        double param = rd.num();
        SmPoint2d guess = rd.uv();
        SmTracePnt a, b;
        zero_point(a);
        zero_point(b);
        b.m_dCurveParameter = param;
        SmBoolean found = FALSE;
        SmPoint2d res(0.0, 0.0);
        SmStatus s = t.LocalPointSolve(a, guess, b, found, res);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(found));
            put_point(o, b);
            if (found)
                put_uv(o, res);
        }
        break;
    }
    case OP_NEXT:
    {
        bool first = rd.flag();
        int64_t kind = rd.idx(0, 4);
        double step = rd.num();
        if (!last.has)
        {
            o.push_back(-1.0);
            break;
        }
        SmTracePnt* p = t.NewPoint();
        *p = last.pnt;
        p->m_ePointType = (SmTracePointType)kind;
        if (first)
            t.CurvePoints().Append(p);
        SmTracePnt nx;
        zero_point(nx);
        SmBoolean found = FALSE, clipped = FALSE, bnd = FALSE;
        double dev = 0.0, ang = 0.0;
        SmStatus s = t.ComputeNextPoint(*p, step, nx, found, dev, ang, clipped, bnd);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(found));
            o.push_back(dev);
            o.push_back(ang);
            o.push_back(flg(clipped));
            o.push_back(flg(bnd));
            put_point(o, nx);
            put_point(o, *p);
        }
        break;
    }
    case OP_SPAN:
    {
        double step = rd.num();
        if (!last.has)
        {
            o.push_back(-1.0);
            break;
        }
        double dev = -1.0;
        SmBoolean ok = FALSE;
        SmStatus s = t.ComputeSpanDeviation(last.pnt, last.pnt, step, dev, ok);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(ok));
            if (ok)
                o.push_back(dev);
        }
        break;
    }
    case OP_TRACE2:
    {
        if (!last.has)
        {
            o.push_back(-1.0);
            break;
        }
        SmTracePnt* p = t.NewPoint();
        *p = last.pnt;
        SmStatus s = t.TraceCurve2(*p, acc.c3, acc.uv, acc.drop, acc.dev);
        o.push_back(st(s));
        break;
    }
    case OP_INTERIOR:
    {
        SmStatus s = t.FindInteriorCurves(acc.c3, acc.uv, acc.drop, acc.dev);
        o.push_back(st(s));
        break;
    }
    case OP_BOUNDARY:
    {
        SmStatus s = t.FindBoundaryStartPoints(acc.c3, acc.uv, acc.drop, acc.dev);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_start_points(o, t);
        break;
    }
    case OP_DO_TRACE:
    {
        Acc r;
        SmApproxTol3d tol(t.ApproxTol());
        double ang = t.AngTol();
        SmStatus s = t.DoTrace(ctx, &tol, &ang, &r.c3, &r.uv, &r.drop, &r.dev);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_acc(o, r);
        break;
    }
    case OP_HOOKS:
    {
        SmPoint3d pt = rd.vec();
        SmTracePnt p;
        zero_point(p);
        SmBoolean done = TRUE;
        SmStatus s = t.TestPoint(p, done);
        o.push_back(st(s));
        o.push_back(flg(done));
        o.push_back(flg(t.IsPointOnCurve(pt, acc.c3)));
        break;
    }
    default:
        throw Bad();
    }
}
} // namespace sc_sdc

SC_EXPORT int32_t sc_intersect_surface_drop_curve_run(const double* in, int64_t n_in, double* out, int64_t cap,
                                                      int64_t* out_len)
{
    using namespace sc_sdc;
    if (in == NULL || n_in < 2)
        return SC_ERR_ARG;
    try
    {
        SC_NATIVE_LOCK;
        SmContext ctx;
        std::vector<double> o;
        Rd rd{in, n_in, 0};
        SmSurface* srf = read_surface(rd, ctx);
        SmExtent2d dom = rd.dom4();
        SmCurve* crv = NULL;
        try
        {
            crv = read_curve(rd, ctx);
        }
        catch (...)
        {
            delete srf;
            throw;
        }
        double i0 = rd.num();
        double i1 = rd.num();
        bool on_surface = rd.flag();
        bool has_tol = rd.flag();
        double tol = rd.num();
        bool has_ang = rd.flag();
        double ang = rd.num();
        bool has_wi = rd.flag();
        double wi0 = rd.num();
        double wi1 = rd.num();
        bool has_wd = rd.flag();
        SmExtent2d wd = rd.dom4();
        {
            Drop t(ctx, srf, dom, *crv, SmExtent1d(i0, i1));
            o.push_back(t.ApproxTol());
            o.push_back(t.AngTol());
            t.SetCurveIsOnSurface(on_surface ? TRUE : FALSE);
            if (has_tol)
                t.ApproxTol() = tol;
            if (has_ang)
                t.AngTol() = ang;
            if (has_wi)
                t.m_vWorkingInterval = SmExtent1d(wi0, wi1);
            if (has_wd)
                t.m_vWorkingUVDomain = wd;
            if (rd.flag())
                t.m_pCurveKnotVector = read_doubles(rd, ctx);
            if (rd.flag())
                t.m_pCurveBreaks = read_doubles(rd, ctx);
            Last last;
            Acc acc;
            int64_t n_ops = rd.idx(0, 64);
            for (int64_t i = 0; i < n_ops; i++)
            {
                int32_t op = (int32_t)rd.idx(1, OP_LAST);
                o.push_back((double)op);
                run_op(op, rd, ctx, crv, t, last, acc, o);
            }
            // final state
            o.push_back(t.ApproxTol());
            o.push_back(t.AngTol());
            o.push_back(t.GetMaxDistToSurface());
            o.push_back(flg(t.GetCurveIsOnSurface()));
            o.push_back(t.m_vWorkingInterval.GetMin());
            o.push_back(t.m_vWorkingInterval.GetMax());
            put_uv(o, t.m_vWorkingUVDomain.GetMin());
            put_uv(o, t.m_vWorkingUVDomain.GetMax());
            put_opt_doubles(o, t.m_pCurveKnotVector);
            put_opt_doubles(o, t.m_pCurveBreaks);
            SmTArray<SmTracePnt*> cp;
            t.CurvePoints().GetAllNodes(cp);
            o.push_back((double)cp.GetSize());
            put_start_points(o, t);
            put_acc(o, acc);
        }
        delete crv;
        delete srf;
        if (rd.at != rd.n)
            return SC_ERR_ARG;
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
