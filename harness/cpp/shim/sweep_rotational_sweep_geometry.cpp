// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `sweep.rotational_sweep_geometry`: the members of
// SmRotationalSweepGeometry (source/SMLib/src/SmRotationalSweepGeometry.cpp) on real
// curves (SmBSplineCurve, B-spline and analytic SmLine segments, SmCircle, SmEllipse,
// SmHermiteCurve), vertices, wire edges and faces of a real SmBrep.
//
// One entry point, sc_sweep_rotational_sweep_geometry_eval(op, in, n_in, out, cap,
// out_len). Layouts are documented in
// crates/solidcore-ffi/src/units/sweep_rotational_sweep_geometry.rs, which validates every
// input; reads are bounds checked here as well (a malformed program is SC_ERR_ARG).
// Every op starts with the sweep block `base[3] axis[3] angle eps make_analytics
// brep_tol` (SmRotationalSweepGeometry(base, axis, angle, eps), SetMakeAnalytics; a
// B-rep gets SetTolerance(brep_tol, FALSE) before any topology when brep_tol > 0).
// Outputs: curves and surfaces as type, natural interval / domain and evaluations at
// fixed fractions of it; wire edge fixtures as the facts the Rust side must rebuild
// (brep tolerance, start vertex, closed).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmEdge.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFace.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmRotationalSweepGeometry.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <SmVertex.h>
#include <nurbs.h>

#include <stdexcept>
#include <vector>

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/sweep_rotational_sweep_geometry.rs).
enum : int32_t
{
    OP_IS_CLOSED = 0,
    OP_START_CS = 1,
    OP_END_CS = 2,
    OP_TRANSFORM = 3,
    OP_CHECK_CURVE = 4,
    OP_VERTEX_HIGHER = 5,
    OP_VERTEX_SAME = 6,
    OP_EDGE_HIGHER = 7,
    OP_EDGE_SAME = 8,
    OP_FACE_SAME = 9,
    OP_COPY = 10,
};

// Curve kinds.
enum : int32_t
{
    CK_NURB = 0,      // SmBSplineCurve(gw_CURVE*, 3, FALSE): n p m cpts[4(n+1)] knots[m+1]
    CK_BLINE = 1,     // SmBSplineCurve::CreateLineSegment(a, b): a[3] b[3]
    CK_LINE = 2,      // SmLine::CreateLineSegment(a, b, ivl?): a[3] b[3] has_ivl t0 t1
    CK_CIRCLE = 3,    // SmCircle(c, x, y, [d0, d1], r, 3, ctx, NULL, io)
    CK_ELLIPSE = 4,   // SmEllipse(c, x, y, [d0, d1], rx, ry, 3, ctx, NULL, io)
    CK_HERMITE = 5,   // SmHermiteCurve(p1, d1, p2, d2, 3)
};

// Samples per curve and per surface direction.
const int N_CURVE_SAMPLES = 7;
const int N_SURF_SAMPLES = 5;

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
};

// Reads the protected members of the sweep geometry.
struct Peek : SmRotationalSweepGeometry
{
    static SmPoint3d SmRotationalSweepGeometry::*base() { return &Peek::m_sBasePt; }
    static SmVector3d SmRotationalSweepGeometry::*axis() { return &Peek::m_sAxis; }
    static double SmRotationalSweepGeometry::*angle() { return &Peek::m_dAngle; }
    static double SmRotationalSweepGeometry::*eps() { return &Peek::m_dEps; }
    static SmBoolean SmSweepGeometryCreation::*analytics() { return &Peek::m_bMakeAnalytics; }
};

double st(SmStatus s)
{
    return s == SM_SUCCESS ? 0.0 : (double)s;
}

double flg(bool b)
{
    return b ? 1.0 : 0.0;
}

void put_vec(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

void put_frame(std::vector<double>& o, const SmAxis2Placement& a)
{
    put_vec(o, a.GetOriginRef());
    put_vec(o, a.GetXAxisRef());
    put_vec(o, a.GetYAxisRef());
}

// `type a b` then N_CURVE_SAMPLES points (`status x y z`, xy only for a 2D curve).
void put_curve(std::vector<double>& o, const SmCurve* c, bool uv)
{
    o.push_back((double)c->GetType());
    SmExtent1d nat = c->GetNaturalInterval();
    o.push_back(nat.GetMin());
    o.push_back(nat.GetMax());
    for (int i = 0; i < N_CURVE_SAMPLES; i++)
    {
        SmPoint3d p(0, 0, 0);
        SmStatus s = c->EvaluatePoint(nat.Evaluate((double)i / (N_CURVE_SAMPLES - 1)), p);
        o.push_back(st(s));
        o.push_back(p.x);
        o.push_back(p.y);
        if (!uv)
            o.push_back(p.z);
    }
}

// `type umin vmin umax vmax` then N_SURF_SAMPLES^2 points (`status x y z`).
void put_surface(std::vector<double>& o, const SmSurface* s)
{
    o.push_back((double)s->GetType());
    SmExtent2d d = s->GetNaturalUVDomain();
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
    for (int i = 0; i < N_SURF_SAMPLES; i++)
        for (int j = 0; j < N_SURF_SAMPLES; j++)
        {
            double fu = (double)i / (N_SURF_SAMPLES - 1);
            double fv = (double)j / (N_SURF_SAMPLES - 1);
            SmPoint2d uv(d.GetMin().x + fu * (d.GetMax().x - d.GetMin().x),
                         d.GetMin().y + fv * (d.GetMax().y - d.GetMin().y));
            SmPoint3d p(0, 0, 0);
            SmStatus st_ = s->EvaluatePoint(uv, p);
            o.push_back(st(st_));
            put_vec(o, p);
        }
}

gw_CURVE* read_nurb(Rd& rd)
{
    gw_INDEX n = (gw_INDEX)rd.idx(0, 4096);
    gw_DEGREE p = (gw_DEGREE)rd.idx(1, 32);
    gw_INDEX m = (gw_INDEX)rd.idx(0, 4096 + 33);
    if (m != n + p + 1)
        throw Bad();
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
    return cur;
}

// A curve block; a failing constructor is a harness error (SC_ERR_ARG).
SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx(0, 5);
    switch (kind)
    {
    case CK_NURB: return new (ctx) SmBSplineCurve(read_nurb(rd), 3, FALSE);
    case CK_BLINE:
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmBSplineCurve* c = NULL;
        if (SmBSplineCurve::CreateLineSegment(ctx, 3, a, b, c) != SM_SUCCESS || !c)
            throw Bad();
        return c;
    }
    case CK_LINE:
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        bool has = rd.flag();
        double t0 = rd.num();
        double t1 = rd.num();
        SmExtent1d ivl(t0, t1);
        SmLine* c = NULL;
        if (SmLine::CreateLineSegment(ctx, 3, a, b, c, has ? &ivl : NULL) != SM_SUCCESS || !c)
            throw Bad();
        return c;
    }
    case CK_CIRCLE:
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double r = rd.num();
        bool io = rd.flag();
        return new (ctx) SmCircle(c, x, y, SmExtent1d(d0, d1), r, 3, &ctx, NULL, io ? TRUE : FALSE);
    }
    case CK_ELLIPSE:
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double rx = rd.num();
        double ry = rd.num();
        bool io = rd.flag();
        return new (ctx) SmEllipse(c, x, y, SmExtent1d(d0, d1), rx, ry, 3, &ctx, NULL,
                                   io ? TRUE : FALSE);
    }
    default:
    {
        SmPoint3d p1 = rd.vec();
        SmVector3d d1 = rd.vec();
        SmPoint3d p2 = rd.vec();
        SmVector3d d2 = rd.vec();
        return new (ctx) SmHermiteCurve(p1, d1, p2, d2, 3);
    }
    }
}

// A surface block: `0 nurb-surface` (SmBSplineSurface) or `1 o[3] x[3] y[3] su sv
// u0 v0 u1 v1` (finite SmPlane).
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx(0, 1);
    if (kind == 1)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double su = rd.num();
        double sv = rd.num();
        double u0 = rd.num();
        double v0 = rd.num();
        double u1 = rd.num();
        double v1 = rd.num();
        return new (ctx) SmPlane(o, x, y, SmVector2d(su, sv), SmExtent2d(u0, v0, u1, v1), &ctx);
    }
    gw_INDEX n = (gw_INDEX)rd.idx(0, 256);
    gw_INDEX m = (gw_INDEX)rd.idx(0, 256);
    gw_DEGREE p = (gw_DEGREE)rd.idx(1, 32);
    gw_DEGREE q = (gw_DEGREE)rd.idx(1, 32);
    gw_INDEX r = (gw_INDEX)rd.idx(0, 256 + 33);
    gw_INDEX s = (gw_INDEX)rd.idx(0, 256 + 33);
    if (r != n + p + 1 || s != m + q + 1)
        throw Bad();
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

struct Sweep
{
    SmPoint3d base;
    SmVector3d axis;
    double angle = 0.0;
    double eps = 0.0;
    bool analytics = true;
    double brep_tol = 0.0;
};

Sweep read_sweep(Rd& rd)
{
    Sweep s;
    s.base = rd.vec();
    s.axis = rd.vec();
    s.angle = rd.num();
    s.eps = rd.num();
    s.analytics = rd.flag();
    s.brep_tol = rd.num();
    return s;
}

// A B-rep with the sweep's tolerance.
SmBrep* new_brep(const Sweep& sw, SmContext& ctx)
{
    SmBrep* b = new (ctx) SmBrep(SM_USE_DEFAULT, SM_USE_DEFAULT, &ctx);
    if (sw.brep_tol > 0.0)
        b->SetTolerance(sw.brep_tol, FALSE);
    return b;
}

// Edge block: curve, `t0 t1 opposite closed off0[3] off1[3] etol vtol0 vtol1`. The
// first vertex is at the curve point the edge starts at (t1 for an opposite edge) plus
// off0, the other at the other end plus off1 (a closed edge has one vertex).
SmEdge* read_edge(Rd& rd, SmBrep* brep, SmContext& ctx, SmVertex*& first)
{
    SmCurve* curve = read_curve(rd, ctx);
    double t0 = rd.num();
    double t1 = rd.num();
    bool opposite = rd.flag();
    bool closed = rd.flag();
    SmVector3d off0 = rd.vec();
    SmVector3d off1 = rd.vec();
    double etol = rd.num();
    double vt0 = rd.num();
    double vt1 = rd.num();
    SmExtent1d ivl(t0, t1);
    SmPoint3d pa(0, 0, 0), pb(0, 0, 0);
    if (curve->EvaluatePoint(opposite ? t1 : t0, pa) != SM_SUCCESS ||
        curve->EvaluatePoint(opposite ? t0 : t1, pb) != SM_SUCCESS)
    {
        delete curve;
        throw Bad();
    }
    pa += off0;
    pb += off1;
    SmShell* shell = NULL;
    SmVertex* v0 = NULL;
    SmVertex* v1 = NULL;
    SmEdge* edge = NULL;
    SmOrientType ot = opposite ? SM_OT_OPPOSITE : SM_OT_SAME;
    if (brep->MakeShellVertex(brep->GetInfiniteRegion(), pa, shell, v0) != SM_SUCCESS)
    {
        delete curve;
        throw Bad();
    }
    SmStatus s = closed ? brep->MakeWireEdge(brep->GetInfiniteRegion(), v0, v0, curve, ivl, ot, edge)
                        : brep->MakeWireEdgeVertex(brep->GetInfiniteRegion(), v0, curve, ivl, ot,
                                                   pb, edge, v1);
    if (s != SM_SUCCESS || !edge)
        throw Bad();
    first = v0;
    edge->SetTolerance(etol, FALSE, FALSE);
    v0->SetTolerance(vt0, FALSE, FALSE);
    if (v1)
        v1->SetTolerance(vt1, FALSE, FALSE);
    return edge;
}

// The fixture facts: brep tolerance, start vertex is the first one, primary vertex is
// the first one, closed, vertex points.
void put_fixture(std::vector<double>& o, SmBrep* brep, SmEdge* e, SmVertex* first)
{
    SmVertex* sv = e->GetStartVertex();
    SmVertex* ov = e->GetOtherVertex(sv);
    o.push_back((double)brep->GetTolerance());
    o.push_back(flg(sv == first));
    o.push_back(flg(e->GetVertex() == first));
    o.push_back(flg(e->IsClosed() ? true : false));
    put_vec(o, sv->GetPoint());
    put_vec(o, ov->GetPoint());
    o.push_back((double)sv->GetTolerance());
    o.push_back((double)ov->GetTolerance());
    o.push_back((double)e->GetTolerance());
}

// The index of `c` in `list`, -1 for NULL, -2 for a pointer not in the list.
double index_in(const SmTArray<SmCurve*>& list, const SmCurve* c)
{
    if (!c)
        return -1.0;
    for (ULONG i = 0; i < list.GetSize(); i++)
        if (list[i] == c)
            return (double)i;
    return -2.0;
}

void run(int32_t op, Rd& rd, std::vector<double>& o)
{
    SmContext ctx;
    Sweep sw = read_sweep(rd);
    SmRotationalSweepGeometry g(sw.base, sw.axis, sw.angle, sw.eps);
    g.SetMakeAnalytics(sw.analytics ? TRUE : FALSE);

    switch (op)
    {
    case OP_IS_CLOSED:
    {
        ULONG reps = (ULONG)rd.idx(0, 1 << 20);
        if (rd.at != rd.n)
            throw Bad();
        SmBoolean ret = FALSE;
        SmStatus s = g.IsSweepClosed(reps, ret);
        o.push_back(st(s));
        o.push_back(flg(ret ? true : false));
        break;
    }
    case OP_START_CS:
    case OP_END_CS:
    {
        SmPoint3d p = rd.vec();
        if (rd.at != rd.n)
            throw Bad();
        SmAxis2Placement cs;
        SmStatus s = op == OP_START_CS ? g.GetStartCoordSystem(p, cs) : g.GetEndCoordSystem(p, cs);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_frame(o, cs);
        break;
    }
    case OP_TRANSFORM:
    {
        if (rd.at != rd.n)
            throw Bad();
        SmAxis2Placement cs;
        SmStatus s = g.GetSweepTransform(cs);
        o.push_back(st(s));
        put_frame(o, cs);
        break;
    }
    case OP_COPY:
    {
        if (rd.at != rd.n)
            throw Bad();
        SmSweepGeometryCreation* c = g.Copy();
        SmRotationalSweepGeometry* r = (SmRotationalSweepGeometry*)c;
        o.push_back(0.0);
        put_vec(o, r->*Peek::base());
        put_vec(o, r->*Peek::axis());
        o.push_back(r->*Peek::angle());
        o.push_back(r->*Peek::eps());
        o.push_back(flg((r->*Peek::analytics()) ? true : false));
        delete c;
        break;
    }
    case OP_CHECK_CURVE:
    {
        SmCurve* c = read_curve(rd, ctx);
        double t0 = rd.num();
        double t1 = rd.num();
        double tol = rd.num();
        SmPoint3d sp = rd.vec();
        SmVector3d sv = rd.vec();
        double amount = rd.num();
        if (rd.at != rd.n)
        {
            delete c;
            throw Bad();
        }
        SmSweepCheckType state = SM_SC_UNKNOWN;
        SmStatus s = g.CheckCurveSweep(*c, SmExtent1d(t0, t1), tol, sp, sv, amount, state);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back((double)state);
        delete c;
        break;
    }
    case OP_VERTEX_HIGHER:
    case OP_VERTEX_SAME:
    {
        SmPoint3d p = rd.vec();
        double vt = rd.num();
        if (rd.at != rd.n)
            throw Bad();
        SmBrep* brep = new_brep(sw, ctx);
        SmShell* shell = NULL;
        SmVertex* v = NULL;
        if (brep->MakeShellVertex(brep->GetInfiniteRegion(), p, shell, v) != SM_SUCCESS)
        {
            delete brep;
            throw Bad();
        }
        v->SetTolerance(vt, FALSE, FALSE);
        o.push_back((double)v->GetTolerance());
        if (op == OP_VERTEX_HIGHER)
        {
            SmCurve* c = NULL;
            double tol = -7.0;
            SmStatus s = g.VertexSweepHigher(ctx, v, c, tol);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
            {
                o.push_back(flg(c != NULL));
                if (c)
                {
                    o.push_back(tol);
                    put_curve(o, c, false);
                }
            }
            delete c;
        }
        else
        {
            SmPoint3d q(0, 0, 0);
            double tol = -7.0;
            SmStatus s = g.VertexSweepSame(v, q, tol);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
            {
                put_vec(o, q);
                o.push_back(tol);
            }
        }
        delete brep;
        break;
    }
    case OP_EDGE_HIGHER:
    case OP_EDGE_SAME:
    {
        SmBrep* brep = new_brep(sw, ctx);
        SmEdge* e = NULL;
        SmVertex* first = NULL;
        try
        {
            e = read_edge(rd, brep, ctx, first);
        }
        catch (...)
        {
            delete brep;
            throw;
        }
        if (rd.at != rd.n)
        {
            delete brep;
            throw Bad();
        }
        put_fixture(o, brep, e, first);
        if (op == OP_EDGE_SAME)
        {
            SmCurve* c = NULL;
            double tol = -7.0;
            SmStatus s = g.EdgeSweepSame(ctx, e, c, tol);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
            {
                o.push_back(flg(c != NULL));
                o.push_back(tol);
                if (c)
                    put_curve(o, c, false);
            }
            delete c;
        }
        else
        {
            SmSurface* srf = NULL;
            SmCurve *sc = NULL, *fc = NULL, *svc = NULL, *evc = NULL;
            double ftol = -7, sctol = -7, etol = -7, svtol = -7, evtol = -7;
            SmTArray<SmCurve*> c3;
            SmTArray<SmBSplineCurve*> cuv;
            SmTArray<SmOrientType> ors;
            SmVector3d nrm(0, 0, 0);
            SmStatus s = g.EdgeSweepHigher(ctx, e, srf, ftol, sc, sctol, fc, etol, svc, svtol,
                                           evc, evtol, c3, cuv, ors, nrm);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
            {
                o.push_back(flg(srf != NULL));
                if (srf)
                    put_surface(o, srf);
                o.push_back(ftol);
                o.push_back(sctol);
                o.push_back(etol);
                o.push_back(svtol);
                o.push_back(evtol);
                put_vec(o, nrm);
                o.push_back(index_in(c3, sc));
                o.push_back(index_in(c3, fc));
                o.push_back(index_in(c3, svc));
                o.push_back(index_in(c3, evc));
                o.push_back((double)c3.GetSize());
                for (ULONG i = 0; i < c3.GetSize(); i++)
                    put_curve(o, c3[i], false);
                o.push_back((double)cuv.GetSize());
                for (ULONG i = 0; i < cuv.GetSize(); i++)
                    put_curve(o, cuv[i], true);
                o.push_back((double)ors.GetSize());
                for (ULONG i = 0; i < ors.GetSize(); i++)
                    o.push_back((double)ors[i]);
            }
            for (ULONG i = 0; i < c3.GetSize(); i++)
                delete c3[i];
            for (ULONG i = 0; i < cuv.GetSize(); i++)
                delete cuv[i];
            delete srf;
        }
        delete brep;
        break;
    }
    case OP_FACE_SAME:
    {
        SmSurface* srf = read_surface(rd, ctx);
        double ftol = rd.num();
        if (rd.at != rd.n)
        {
            delete srf;
            throw Bad();
        }
        SmBrep* brep = new_brep(sw, ctx);
        SmFace* f = NULL;
        if (brep->CreateFaceFromSurface(srf, srf->GetNaturalUVDomain(), f) != SM_SUCCESS || !f)
        {
            delete brep;
            throw Bad();
        }
        f->SetTolerance(ftol, FALSE, FALSE);
        o.push_back((double)f->GetTolerance());
        SmSurface* res = NULL;
        double tol = -7.0;
        SmStatus s = g.FaceSweepSame(ctx, f, res, tol);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(res != NULL));
            if (res)
            {
                o.push_back(tol);
                put_surface(o, res);
            }
        }
        delete res;
        delete brep;
        break;
    }
    default: throw Bad();
    }
}

} // namespace

SC_EXPORT int32_t sc_sweep_rotational_sweep_geometry_eval(int32_t op, const double* in,
                                                          int64_t n_in, double* out, int64_t cap,
                                                          int64_t* out_len)
{
    if (!in || n_in < 0 || !out_len)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, n_in, 0};
    std::vector<double> o;
    try
    {
        run(op, rd, o);
    }
    catch (const Bad&)
    {
        return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
