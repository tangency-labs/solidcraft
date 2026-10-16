// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `intersect.surface_intersector_p4`: the SmSurfaceIntersector members
// TraceCurve, FindInteriorCurves, DoPointIntersection and DoLawIntersection
// (source/SMLib/src/SmSurfaceIntersector.cpp), run on a real SmSurfaceIntersector over SmPlane /
// SmBSplineSurface / SmCurveBoundedSurface objects (a derived class only exposes the protected
// state), with the part 1 to 3 calls a program needs to set the state up (point lists,
// ComputePointValues, FlushCurve).
//
// One entry point, sc_intersect_surface_intersector_p4_run(in, n_in, out, cap, out_len). `in` is
// a program (layout in crates/solidcore-ffi/src/units/intersect_surface_intersector_p4.rs);
// every read is bounds checked here (a bad program is SC_ERR_ARG). Ops whose upstream behaviour
// is undefined report REJECTED (see the op cases).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveBoundedSurface.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmFilletExecutive.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmSurface.h>
#include <SmSurfaceCache.h>
#include <SmSurfaceIntersector.h>
#include <SmTArray.h>
#include <SmTList.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <cmath>
#include <stdexcept>
#include <vector>

#include "sc_sm.h"

using sc::put_uv;
using sc::put_vec;
using sc::st;

namespace
{
enum Op : int32_t
{
    OP_STATE = 1,
    OP_POINT = 2,
    OP_ADD_CURVE = 3,
    OP_ADD_START = 4,
    OP_ADD_THROUGH = 5,
    OP_CPV = 6,
    OP_MEMBERS = 7,
    OP_ENDS = 8,
    OP_EXT = 9,
    OP_TRACE = 10,
    OP_FLUSH = 11,
    OP_DPI = 12,
    OP_DLI = 13,
    OP_FIC = 14,
    OP_LAST = 14,
};

// Op status of an op the shim does not run.
const double REJECTED = -2.0;

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

double flg(SmBoolean b)
{
    return b ? 1.0 : 0.0;
}

// `0 o x y scale dom` (SmPlane) or `1 p q nu nv net(x y z w) u v` (SmBSplineSurface).
SmSurface* read_basic_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx(0, 1);
    if (kind == 0)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        SmVector2d s = rd.uv();
        SmExtent2d d = rd.dom4();
        return new (ctx) SmPlane(o, x, y, s, d, &ctx);
    }
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

// A basic surface (`0 ..` / `1 ..`) or `3 basic`: an SmCurveBoundedSurface (implicit outer, no
// boundary curves) over it, which owns (deletes) its basis. (`2` marks B = A.)
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    if (rd.at < rd.n && rd.in[rd.at] == 3.0)
    {
        rd.at++;
        SmSurface* basis = read_basic_surface(rd, ctx);
        SmTArray<SmCurve*> c3d(ctx), cuv(ctx);
        return new (ctx) SmCurveBoundedSurface(*basis, &c3d, &cuv, TRUE);
    }
    return read_basic_surface(rd, ctx);
}

// `0 a b` (SmLine::CreateLineSegment, dim 3) or `1 dim p ncp cpts(x y z w) knots`.
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
    ULONG dim = (ULONG)rd.idx(2, 3);
    int64_t p = rd.idx(1, 8);
    int64_t ncp = rd.idx(2, 33);
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
    SmBSplineCurve* c = new (ctx) SmBSplineCurve(dim, (ULONG)cur->pol->n, (ULONG)cur->knt->m, (short)cur->p,
                                                 cur->pol->Pw, cur->knt->U, &ctx);
    smos_Free(cur);
    return c;
}

// A point: `type ptype[4] uv0 duv0 uv1 duv1 srf0[3][3] srf1[3][3] crv_pos crv_deriv param
// angle deviation user[4]` (80 values).
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

// Reads the protected STEP flags through pointers to members named in a derived class.
struct PeekC : SmBSplineCurve
{
    static SmKnotType SmBSplineCurve::*kt() { return &PeekC::m_eKnotType; }
    static SmBSplineCurveForm SmBSplineCurve::*form() { return &PeekC::m_eBSplineCurveForm; }
};

// `type dim knot_type form has [n p m cpts(x y z w) knots] ivl[2]` of a B-spline curve.
void put_bspline(std::vector<double>& o, const SmCurve* c)
{
    o.push_back((double)c->GetType());
    o.push_back((double)c->GetDim());
    if (!c->IsKindOf(SmBSplineCurve_TYPE))
        throw Bad();
    const SmBSplineCurve* b = (const SmBSplineCurve*)c;
    o.push_back((double)(b->*PeekC::kt()));
    o.push_back((double)(b->*PeekC::form()));
    const gw_CURVE* g = b->GetGwNurbPointer();
    o.push_back(g ? 1.0 : 0.0);
    if (g)
    {
        o.push_back((double)g->pol->n);
        o.push_back((double)g->p);
        o.push_back((double)g->knt->m);
        for (gw_INDEX i = 0; i <= g->pol->n; i++)
        {
            o.push_back(g->pol->Pw[i].x);
            o.push_back(g->pol->Pw[i].y);
            o.push_back(g->pol->Pw[i].z);
            o.push_back(g->pol->Pw[i].w);
        }
        for (gw_INDEX i = 0; i <= g->knt->m; i++)
            o.push_back(g->knt->U[i]);
    }
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
}

// `count curves..` from index `from` of an array.
void put_curves(std::vector<double>& o, const SmTArray<SmCurve*>& a, ULONG from)
{
    o.push_back((double)(a.GetSize() - from));
    for (ULONG i = from; i < a.GetSize(); i++)
        put_bspline(o, a[i]);
}

struct Prog
{
    const SmContext& ctx;
    std::vector<SmTsectPnt*> pts;
    std::vector<SmCurve*> curves;
    ~Prog()
    {
        for (SmTsectPnt* p : pts)
            delete p;
        for (SmCurve* c : curves)
            delete c;
    }
    SmTsectPnt* pt(Rd& rd)
    {
        int64_t i = rd.idx(0, (int64_t)pts.size() - 1);
        return pts[(size_t)i];
    }
    // `has idx`: a point or NULL
    SmTsectPnt* opt_pt(Rd& rd)
    {
        bool has = rd.flag();
        SmTsectPnt* p = pt(rd);
        return has ? p : NULL;
    }
    double index_of(const SmTsectPnt* p) const
    {
        for (size_t i = 0; i < pts.size(); i++)
            if (pts[i] == p)
                return (double)i;
        return -1.0;
    }
    // a program point's index, or `-1` and the point's values
    void put_ref(std::vector<double>& o, const SmTsectPnt* p) const
    {
        double i = index_of(p);
        o.push_back(i);
        if (i < 0.0)
            put_point(o, *p);
    }
};

// The intersector with its protected state reachable.
struct Peek : SmSurfaceIntersector
{
    Peek(const SmSurface& a, const SmExtent2d& da, const SmSurface& b, const SmExtent2d& db, SmBoolean f)
        : SmSurfaceIntersector(a, da, b, db, f)
    {
    }

    void set_context(const SmContext& c) { m_cpContext = &c; }

    void set_members(double trace_dir, double approx_tol, double start_tol, double step_tol, double ang_tol)
    {
        m_dCurveTraceDirection = trace_dir;
        m_dThisApproxTol3d = approx_tol;
        m_dStartParallelTolRadians = start_tol;
        m_dSteppingParallelTolRadians = step_tol;
        m_dThisAngTolRad = ang_tol;
    }

    void set_ends(SmTsectPnt* s, SmTsectPnt* e)
    {
        m_pStartPoint = s;
        m_pEndPoint = e;
    }

    void set_ext(bool clip, double last, double dist)
    {
        m_bDoingExtensionClipping = clip ? TRUE : FALSE;
        m_dLastThroughParmeter = last;
        m_dExtensionDistance = dist;
    }

    bool no_curve_points() const { return m_vCurvePoints.GetFirstNode() == NULL; }
    // on one of the three lists (a list's Init() leaves stale links in its former nodes)
    bool on_list(SmTsectPnt* p) const
    {
        return m_vCurvePoints.IsInList(p) || m_vStartPoints.IsInList(p) || m_vThroughPoints.IsInList(p);
    }
    SmStatus trace(SmTsectPnt& p, SmTArray<SmCurve*>& c3) { return TraceCurve(p, c3); }
    SmStatus flush(SmTArray<SmCurve*>& c3, SmTArray<SmCurve*>& c0, SmTArray<SmCurve*>& c1,
                   SmTArray<SmTsectCurveType>& t, SmTArray<double>& d)
    {
        return FlushCurve(c3, c0, c1, t, d);
    }
    SmStatus interior(SmTArray<SmCurve*>& c3, SmTArray<SmCurve*>& c0, SmTArray<SmCurve*>& c1,
                      SmTArray<SmTsectCurveType>& t, SmTArray<double>& d)
    {
        return FindInteriorCurves(c3, c0, c1, t, d);
    }

    void put_list(std::vector<double>& o, const Prog& pr, const SmTList<SmTsectPnt>& l) const
    {
        SmTsectPnt* aData[70];
        SmTArray<SmTsectPnt*> a(70, aData);
        l.GetAllNodes(a);
        o.push_back((double)a.GetSize());
        for (ULONG i = 0; i < a.GetSize(); i++)
            pr.put_ref(o, a[i]);
    }

    // `approx_tol trace_dir start_tol step_tol ang_tol last_through ext_dist ext_clip closed
    // clipped interior start_point end_point curve_list start_list through_list npoints points`
    void put_state(std::vector<double>& o, const Prog& pr) const
    {
        o.push_back(m_dThisApproxTol3d);
        o.push_back(m_dCurveTraceDirection);
        o.push_back(m_dStartParallelTolRadians);
        o.push_back(m_dSteppingParallelTolRadians);
        o.push_back(m_dThisAngTolRad);
        o.push_back(m_dLastThroughParmeter);
        o.push_back(m_dExtensionDistance);
        o.push_back(flg(m_bDoingExtensionClipping));
        o.push_back(flg(m_bCurveIsClosed));
        o.push_back(flg(m_bClippedByThroughPoint));
        o.push_back(flg(m_bCheckForInteriorCurves));
        o.push_back(m_pStartPoint ? pr.index_of(m_pStartPoint) : -3.0);
        o.push_back(m_pEndPoint ? pr.index_of(m_pEndPoint) : -3.0);
        put_list(o, pr, m_vCurvePoints);
        put_list(o, pr, m_vStartPoints);
        put_list(o, pr, m_vThroughPoints);
        o.push_back((double)pr.pts.size());
        for (const SmTsectPnt* p : pr.pts)
            put_point(o, *p);
    }
};

// The accumulated FlushCurve outputs: `out3d` starts with the program's curves.
struct Outs
{
    SmCurve* a3[8];
    SmCurve* a0[8];
    SmCurve* a1[8];
    SmTsectCurveType at[8];
    double ad[8];
    SmTArray<SmCurve*> c3;
    SmTArray<SmCurve*> c0;
    SmTArray<SmCurve*> c1;
    SmTArray<SmTsectCurveType> types;
    SmTArray<double> devs;
    ULONG first;
    Outs() : c3(8, a3), c0(8, a0), c1(8, a1), types(8, at), devs(8, ad), first(0) {}
    ~Outs()
    {
        for (ULONG i = first; i < c3.GetSize(); i++)
            delete c3[i];
        for (ULONG i = 0; i < c0.GetSize(); i++)
            delete c0[i];
        for (ULONG i = 0; i < c1.GetSize(); i++)
            delete c1[i];
    }
    void put(std::vector<double>& o) const
    {
        put_curves(o, c3, first);
        put_curves(o, c0, 0);
        put_curves(o, c1, 0);
        o.push_back((double)types.GetSize());
        for (ULONG i = 0; i < types.GetSize(); i++)
            o.push_back((double)types[i]);
        o.push_back((double)devs.GetSize());
        for (ULONG i = 0; i < devs.GetSize(); i++)
            o.push_back(devs[i]);
    }
};

void run_op(int32_t op, Rd& rd, Prog& pr, Peek& si, Outs& outs, std::vector<double>& o)
{
    switch (op)
    {
    case OP_STATE:
        o.push_back(0.0);
        si.put_state(o, pr);
        return;
    case OP_POINT:
    {
        SmTsectPnt* p = pr.pt(rd);
        o.push_back(0.0);
        put_point(o, *p);
        return;
    }
    case OP_ADD_CURVE:
    case OP_ADD_START:
    case OP_ADD_THROUGH:
    {
        SmTsectPnt* p = pr.pt(rd);
        // linking a point that is on a list corrupts the intrusive lists
        if (si.on_list(p))
        {
            o.push_back(REJECTED);
            return;
        }
        SmStatus s = op == OP_ADD_CURVE   ? si.AddPointToCurve(*p)
                     : op == OP_ADD_START ? si.AddStartPoint(*p)
                                          : si.AddThroughPoint(*p);
        o.push_back(st(s));
        return;
    }
    case OP_CPV:
    {
        SmPoint2d uvs[2];
        uvs[0] = rd.uv();
        uvs[1] = rd.uv();
        SmTsectPnt* p = pr.pt(rd);
        SmTsectPnt* prev = pr.opt_pt(rd);
        bool has_step = rd.flag();
        double step = rd.num();
        if (prev == p)
            throw Bad();
        o.push_back(st(si.ComputePointValues(uvs, *p, prev, has_step ? &step : NULL)));
        put_point(o, *p);
        return;
    }
    case OP_MEMBERS:
    {
        double td = rd.num();
        double at = rd.num();
        double sp = rd.num();
        double stp = rd.num();
        double ang = rd.num();
        si.set_members(td, at, sp, stp, ang);
        o.push_back(0.0);
        return;
    }
    case OP_ENDS:
    {
        SmTsectPnt* s = pr.opt_pt(rd);
        SmTsectPnt* e = pr.opt_pt(rd);
        si.set_ends(s, e);
        o.push_back(0.0);
        return;
    }
    case OP_EXT:
    {
        bool clip = rd.flag();
        double last = rd.num();
        double dist = rd.num();
        si.set_ext(clip, last, dist);
        o.push_back(0.0);
        return;
    }
    case OP_TRACE:
    {
        SmTsectPnt* p = pr.pt(rd);
        // a start point that is appended while it is on a list corrupts the lists
        if (si.no_curve_points() && si.on_list(p))
        {
            o.push_back(REJECTED);
            return;
        }
        o.push_back(st(si.trace(*p, outs.c3)));
        return;
    }
    case OP_FLUSH:
    {
        SmStatus s = si.flush(outs.c3, outs.c0, outs.c1, outs.types, outs.devs);
        o.push_back(st(s));
        outs.put(o);
        return;
    }
    case OP_FIC:
    {
        SmStatus s = si.interior(outs.c3, outs.c0, outs.c1, outs.types, outs.devs);
        o.push_back(st(s));
        outs.put(o);
        return;
    }
    case OP_DPI:
    case OP_DLI:
    {
        int64_t n = rd.idx(0, 8);
        std::vector<SmTsectPnt*> v;
        bool bad_points = false;
        for (int64_t i = 0; i < n; i++)
        {
            SmTsectPnt* p = pr.pt(rd);
            // a point twice, or one on a list: linked (again) in place
            for (SmTsectPnt* q : v)
                if (q == p)
                    bad_points = true;
            if (si.on_list(p))
                bad_points = true;
            v.push_back(p);
        }
        SmTsectPnt* aData[8];
        SmTArray<SmTsectPnt*> pts(8, aData);
        for (SmTsectPnt* p : v)
            pts.Add(p);
        SmBSplineCurve* c3 = NULL;
        SmBSplineCurve* c0 = NULL;
        SmBSplineCurve* c1 = NULL;
        SmTsectCurveType type = (SmTsectCurveType)-7;
        double dev = -7.0;
        SmStatus s;
        if (op == OP_DPI)
        {
            bool before = rd.flag();
            bool after = rd.flag();
            bool has_sd = rd.flag();
            SmVector3d sd = rd.vec();
            bool has_ed = rd.flag();
            SmVector3d ed = rd.vec();
            bool has_tol = rd.flag();
            SmApproxTol3d tol = SM_CAST_APPROXTOL3D(rd.num());
            bool has_ang = rd.flag();
            double ang = rd.num();
            if (bad_points)
            {
                o.push_back(REJECTED);
                return;
            }
            s = si.DoPointIntersection(pr.ctx, pts, before ? TRUE : FALSE, after ? TRUE : FALSE, has_sd ? &sd : NULL,
                                       has_ed ? &ed : NULL, has_tol ? &tol : NULL, has_ang ? &ang : NULL, c3, c0, c1,
                                       type, dev);
        }
        else
        {
            bool orient = rd.flag();
            if (pr.curves.empty())
                throw Bad();
            SmCurve* crv = pr.curves[(size_t)rd.idx(0, (int64_t)pr.curves.size() - 1)];
            double a = rd.num();
            double b = rd.num();
            SmExtent1d ivl(a, b);
            double so[2];
            so[0] = rd.num();
            so[1] = rd.num();
            bool avg = rd.flag();
            bool has_tol = rd.flag();
            SmApproxTol3d tol = SM_CAST_APPROXTOL3D(rd.num());
            if (bad_points)
            {
                o.push_back(REJECTED);
                return;
            }
            SmFilletLaw law;
            s = si.DoLawIntersection(pr.ctx, pts, law, orient ? TRUE : FALSE, *crv, ivl, so, avg ? TRUE : FALSE, FALSE,
                                     has_tol ? &tol : NULL, NULL, c3, c0, c1, type, dev);
        }
        o.push_back(st(s));
        // `has3d [curve] has0 [curve] has1 [curve] type dev` (status 0 only)
        if (s == SM_SUCCESS)
        {
            const SmBSplineCurve* cs[3] = {c3, c0, c1};
            for (const SmBSplineCurve* c : cs)
            {
                o.push_back(c ? 1.0 : 0.0);
                if (c)
                    put_bspline(o, c);
            }
            o.push_back((double)type);
            o.push_back(dev);
        }
        delete c3;
        delete c0;
        delete c1;
        return;
    }
    default:
        throw Bad();
    }
}
} // namespace

// Program: `surfA surfB` (surfB `2` = the same object as A), `dom1[4] dom2[4] fillet`,
// `ncurves curves`, `npoints points`, `nops ops`. Output: per op `op status [values]`, then the
// final state.
SC_EXPORT int32_t sc_intersect_surface_intersector_p4_run(const double* in, int64_t n_in, double* out,
                                                          int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 2)
        return SC_ERR_ARG;
    try
    {
        SmContext ctx;
        std::vector<double> o;
        Rd rd{in, n_in, 0};
        SmSurface* a = read_surface(rd, ctx);
        SmSurface* b = NULL;
        bool same = false;
        if (rd.at < rd.n && rd.in[rd.at] == 2.0)
        {
            rd.at++;
            same = true;
            b = a;
        }
        else
            b = read_surface(rd, ctx);
        SmExtent2d d1 = rd.dom4();
        SmExtent2d d2 = rd.dom4();
        bool fillet = rd.flag();
        Prog pr{ctx, {}, {}};
        int64_t nc = rd.idx(0, 8);
        for (int64_t i = 0; i < nc; i++)
            pr.curves.push_back(read_curve(rd, ctx));
        int64_t np = rd.idx(0, 64);
        for (int64_t i = 0; i < np; i++)
        {
            SmTsectPnt* t = new SmTsectPnt();
            pr.pts.push_back(t);
            read_point(rd, *t);
        }
        int64_t nops = rd.idx(0, 64);
        {
            Peek si(*a, d1, *b, d2, fillet ? TRUE : FALSE);
            si.set_context(ctx);
            Outs outs;
            for (SmCurve* c : pr.curves)
                outs.c3.Add(c);
            outs.first = outs.c3.GetSize();
            for (int64_t i = 0; i < nops; i++)
            {
                int32_t op = (int32_t)rd.idx(1, OP_LAST);
                o.push_back((double)op);
                run_op(op, rd, pr, si, outs, o);
            }
            si.put_state(o, pr);
            // unlink every point before the lists go away
            for (SmTsectPnt* t : pr.pts)
                t->ReSet();
            si.set_ends(NULL, NULL);
        }
        if (!same)
            delete b;
        delete a;
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
