// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.plane_p2`: the SmPlane members owned by the unit
// (source/SMLib/src/SmPlane.cpp, source/SMLib/inc/SmPlane.h): CreateOffsetSurface,
// GlobalSurfaceIntersect, IntersectWithPlane, TrimCurveToPlaneDomain, IsNurbSurfacePlane,
// Transform, TrimWithDomain, ReadFromDB, MakeNewPlaneLine and MakeNewPlaneEllipse.
//
// One entry point, sc_surface_plane_p2_run(in, n_in, out, cap, out_len). `in` is one op: its
// code then its operands; layouts are documented in
// crates/solidcore-ffi/src/units/surface_plane_p2.rs, which validates every value. Every read
// is bounds checked here too (a bad program is SC_ERR_ARG). `out` receives the op's record.
#include "sc_shim.h"

#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmDatabaseIO.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

#include "sc_sm.h"

using sc::put_vec;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_plane_p2.rs).
enum : int32_t
{
    OP_OFFSET = 1,
    OP_TRANSFORM = 2,
    OP_TRIM_DOMAIN = 3,
    OP_IS_NURB_PLANE = 4,
    OP_INTERSECT = 5,
    OP_GLOBAL = 6,
    OP_TRIM_CURVE = 7,
    OP_LINE = 8,
    OP_ELLIPSE = 9,
    OP_READ_DB = 10,
    OP_LAST = 10,
};

struct PeekS : SmBSplineSurface
{
    static SmKnotType SmBSplineSurface::*kt() { return &PeekS::m_eKnotType; }
    static SmBSplineSurfaceForm SmBSplineSurface::*form() { return &PeekS::m_eBSplineSurfaceForm; }
};

struct PeekC : SmBSplineCurve
{
    static SmKnotType SmBSplineCurve::*kt() { return &PeekC::m_eKnotType; }
    static SmBSplineCurveForm SmBSplineCurve::*form() { return &PeekC::m_eBSplineCurveForm; }
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

void put_dom(std::vector<double>& o, const SmExtent2d& e)
{
    o.push_back(e.GetMin().x);
    o.push_back(e.GetMin().y);
    o.push_back(e.GetMax().x);
    o.push_back(e.GetMax().y);
}

void put_ivl(std::vector<double>& o, const SmExtent1d& e)
{
    o.push_back(e.GetMin());
    o.push_back(e.GetMax());
}

// A plane block: `kind` (0 bounded `o x y scale[2] dom[4]`, 1 normal `o n`, 2 canonical `o x y`)
// then `nurb` (1: make the NURB now with GetOrCreateGwNurbPointer). NULL when CreateCanonical
// fails.
SmPlane* read_plane(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx(0, 2);
    SmPlane* p = NULL;
    switch (kind)
    {
    case 0:
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double su = rd.num();
        double sv = rd.num();
        SmExtent2d d = rd.dom4();
        p = new (ctx) SmPlane(o, x, y, SmVector2d(su, sv), d, &ctx);
        break;
    }
    case 1:
    {
        SmPoint3d o = rd.vec();
        SmVector3d nv = rd.vec();
        p = new (ctx) SmPlane(o, nv, &ctx);
        break;
    }
    default:
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        SmAxis2Placement f(o, x, y);
        if (SmPlane::CreateCanonical(ctx, f, p) != SM_SUCCESS)
        {
            delete p;
            p = NULL;
        }
        break;
    }
    }
    bool nurb = rd.flag();
    if (p != NULL && nurb)
        p->GetOrCreateGwNurbPointer();
    return p;
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

// A curve: `0 a b` (CreateLineSegment, dim 3), `1 dim n p m cpts(x y z w) knots`,
// `2 c x y d0 d1 rx ry` (SmEllipse, dim 3) or `3 c x y d0 d1 r` (SmCircle, dim 3).
SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx(0, 3);
    if (kind == 0)
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine* l = NULL;
        if (SmLine::CreateLineSegment(ctx, 3, a, b, l) != SM_SUCCESS || l == NULL)
            throw Bad();
        return l;
    }
    if (kind == 2 || kind == 3)
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double rx = rd.num();
        if (kind == 3)
            return new (ctx) SmCircle(c, x, y, SmExtent1d(d0, d1), rx, 3, &ctx);
        double ry = rd.num();
        return new (ctx) SmEllipse(c, x, y, SmExtent1d(d0, d1), rx, ry, 3, &ctx);
    }
    ULONG dim = (ULONG)rd.idx(2, 3);
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
    SmBSplineCurve* c = new (ctx) SmBSplineCurve(dim, (ULONG)cur->pol->n, (ULONG)cur->knt->m,
                                                 (short)cur->p, cur->pol->Pw, cur->knt->U, &ctx);
    smos_Free(cur);
    return c;
}

void put_surface_nurb(std::vector<double>& o, const gw_SURFACE* sur)
{
    o.push_back(sur ? 1.0 : 0.0);
    if (!sur)
        return;
    gw_INDEX n = sur->net->n;
    gw_INDEX m = sur->net->m;
    gw_INDEX r = sur->knu->m;
    gw_INDEX s = sur->knv->m;
    o.push_back((double)n);
    o.push_back((double)m);
    o.push_back((double)sur->p);
    o.push_back((double)sur->q);
    o.push_back((double)r);
    o.push_back((double)s);
    for (gw_INDEX i = 0; i <= n; i++)
        for (gw_INDEX j = 0; j <= m; j++)
        {
            o.push_back(sur->net->Pw[i][j].x);
            o.push_back(sur->net->Pw[i][j].y);
            o.push_back(sur->net->Pw[i][j].z);
            o.push_back(sur->net->Pw[i][j].w);
        }
    for (gw_INDEX i = 0; i <= r; i++)
        o.push_back(sur->knu->U[i]);
    for (gw_INDEX i = 0; i <= s; i++)
        o.push_back(sur->knv->U[i]);
}

// type O X Y Z domain scale oob kt form, then the plane's own NURB `has [..]`.
void put_state(std::vector<double>& o, const SmPlane* a)
{
    const SmAxis2Placement& pos = a->GetPosition();
    o.push_back((double)a->GetType());
    put_vec(o, pos.GetOrigin());
    put_vec(o, pos.GetXAxis());
    put_vec(o, pos.GetYAxis());
    put_vec(o, pos.GetZAxis());
    put_dom(o, a->GetSTEPUVDomain());
    SmVector2d sc = a->GetUVScale();
    o.push_back(sc.x);
    o.push_back(sc.y);
    o.push_back(flg(a->IsOutOfBoundsEnabled()));
    o.push_back((double)(a->*PeekS::kt()));
    o.push_back((double)(a->*PeekS::form()));
    put_surface_nurb(o, a->GetGwNurbPointer());
}

void put_curve_nurb(std::vector<double>& o, const SmBSplineCurve* c)
{
    o.push_back((double)(c->*PeekC::kt()));
    o.push_back((double)(c->*PeekC::form()));
    const gw_CURVE* cur = c->GetGwNurbPointer();
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

// `has` then type dim [line: P V dom scale inside_out] [ellipse: C X Y rx ry dom inside_out]
// B-spline part, natural interval.
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    o.push_back(c ? 1.0 : 0.0);
    if (!c)
        return;
    o.push_back((double)c->GetType());
    o.push_back((double)c->GetDim());
    if (c->IsKindOf(SmLine_TYPE))
    {
        const SmLine* l = (const SmLine*)c;
        put_vec(o, l->GetLinePoint());
        put_vec(o, l->GetLineVector());
        put_ivl(o, l->GetSTEPInterval());
        o.push_back(l->GetLineScale());
        o.push_back(l->GetInsideOut() ? 1.0 : 0.0);
    }
    else if (c->IsKindOf(SmEllipse_TYPE))
    {
        const SmEllipse* e = (const SmEllipse*)c;
        const SmAxis2Placement& pos = e->GetPosition();
        put_vec(o, pos.GetOrigin());
        put_vec(o, pos.GetXAxis());
        put_vec(o, pos.GetYAxis());
        o.push_back(e->GetXRadius());
        o.push_back(e->GetYRadius());
        put_ivl(o, e->GetSTEPInterval());
        o.push_back(e->GetInsideOut() ? 1.0 : 0.0);
    }
    if (!c->IsKindOf(SmBSplineCurve_TYPE))
        throw Bad();
    put_curve_nurb(o, (const SmBSplineCurve*)c);
    put_ivl(o, c->GetNaturalInterval());
}

void free_curves(SmTArray<SmCurve*>& a)
{
    for (ULONG i = 0; i < a.GetSize(); i++)
        delete a[i];
    a.ReSet();
}

// `status [needs_more] count curves... types... deviations...` of an intersection.
void put_intersection(std::vector<double>& o, SmTArray<SmCurve*>& c3, SmTArray<SmCurve*>& uv1,
                      SmTArray<SmCurve*>& uv2, SmTArray<SmTsectCurveType>& ty,
                      SmTArray<double>& dev)
{
    o.push_back((double)c3.GetSize());
    for (ULONG i = 0; i < c3.GetSize(); i++)
        put_curve(o, c3[i]);
    o.push_back((double)uv1.GetSize());
    for (ULONG i = 0; i < uv1.GetSize(); i++)
        o.push_back(uv1[i] ? 1.0 : 0.0);
    o.push_back((double)uv2.GetSize());
    for (ULONG i = 0; i < uv2.GetSize(); i++)
        o.push_back(uv2[i] ? 1.0 : 0.0);
    o.push_back((double)ty.GetSize());
    for (ULONG i = 0; i < ty.GetSize(); i++)
        o.push_back((double)ty[i]);
    o.push_back((double)dev.GetSize());
    for (ULONG i = 0; i < dev.GetSize(); i++)
        o.push_back(dev[i]);
}

// `has tol` and the optional tolerance.
const double* read_opt(Rd& rd, double& slot)
{
    bool has = rd.flag();
    slot = rd.num();
    return has ? &slot : NULL;
}

void run_op(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_OFFSET:
    {
        SmPlane* a = read_plane(rd, ctx);
        double dist = rd.num();
        if (a == NULL)
            throw Bad();
        SmSurface* off = NULL;
        SmStatus s = a->CreateOffsetSurface(ctx, dist, 0.0, off);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            if (off == NULL || !off->IsKindOf(SmPlane_TYPE))
                throw Bad();
            put_state(o, (const SmPlane*)off);
        }
        delete off;
        delete a;
        break;
    }
    case OP_TRANSFORM:
    {
        SmPlane* a = read_plane(rd, ctx);
        SmPoint3d po = rd.vec();
        SmVector3d px = rd.vec();
        SmVector3d py = rd.vec();
        bool has_scale = rd.flag();
        SmVector3d sc = rd.vec();
        if (a == NULL)
            throw Bad();
        SmAxis2Placement f(po, px, py);
        SmStatus s = a->Transform(f, has_scale ? &sc : NULL);
        o.push_back(st(s));
        put_state(o, a);
        delete a;
        break;
    }
    case OP_TRIM_DOMAIN:
    {
        SmPlane* a = read_plane(rd, ctx);
        SmExtent2d d = rd.dom4();
        if (a == NULL)
            throw Bad();
        SmStatus s = a->TrimWithDomain(d);
        o.push_back(st(s));
        put_state(o, a);
        delete a;
        break;
    }
    case OP_IS_NURB_PLANE:
    {
        SmBSplineSurface* bs = read_bspline_surface(rd, ctx);
        double scale = rd.num();
        SmPlane* p = NULL;
        SmBoolean r = SmPlane::IsNurbSurfacePlane(ctx, bs, p, scale);
        o.push_back(0.0);
        o.push_back(r ? 1.0 : 0.0);
        o.push_back(p ? 1.0 : 0.0);
        if (p)
            put_state(o, p);
        delete p;
        delete bs;
        break;
    }
    case OP_INTERSECT:
    case OP_GLOBAL:
    {
        SmPlane* a = read_plane(rd, ctx);
        SmExtent2d da = rd.dom4();
        bool same = rd.flag();
        SmSurface* b = NULL;
        if (!same)
        {
            int64_t kind = rd.idx(0, 1);
            b = kind == 0 ? (SmSurface*)read_plane(rd, ctx) : (SmSurface*)read_bspline_surface(rd, ctx);
        }
        SmExtent2d db = rd.dom4();
        double tol_slot = 0.0;
        const double* tol = read_opt(rd, tol_slot);
        if (a == NULL || (!same && b == NULL))
            throw Bad();
        const SmSurface* other = same ? (const SmSurface*)a : b;
        SmBoolean edges[2] = {TRUE, TRUE};
        SmTArray<SmCurve*> c3, uv1, uv2;
        SmTArray<SmTsectCurveType> ty;
        SmTArray<double> dev;
        SmStatus s;
        SmBoolean more = FALSE;
        if (op == OP_INTERSECT)
        {
            if (!other->IsKindOf(SmPlane_TYPE))
                throw Bad();
            SmApproxTol3d t3 = tol ? *tol : 0.0;
            s = a->IntersectWithPlane(ctx, da, *(const SmPlane*)other, db, edges, tol ? &t3 : NULL, NULL,
                                      more, &c3, &uv1, &uv2, &ty, &dev);
        }
        else
        {
            SmApproxTol3d t3 = tol ? *tol : 0.0;
            s = a->GlobalSurfaceIntersect(ctx, da, *other, db, edges, tol ? &t3 : NULL, NULL, &c3, &uv1,
                                          &uv2, &ty, &dev);
        }
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            if (op == OP_INTERSECT)
                o.push_back(flg(more));
            put_intersection(o, c3, uv1, uv2, ty, dev);
        }
        free_curves(c3);
        free_curves(uv1);
        free_curves(uv2);
        delete b;
        delete a;
        break;
    }
    case OP_TRIM_CURVE:
    {
        SmPlane* a = read_plane(rd, ctx);
        SmExtent2d d = rd.dom4();
        SmCurve* c = read_curve(rd, ctx);
        double tol = rd.num();
        if (a == NULL)
            throw Bad();
        SmTArray<SmCurve*> out;
        SmStatus s = a->TrimCurveToPlaneDomain(d, *c, tol, out);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)out.GetSize());
            for (ULONG i = 0; i < out.GetSize(); i++)
                put_curve(o, out[i]);
        }
        free_curves(out);
        delete c;
        delete a;
        break;
    }
    case OP_LINE:
    {
        SmPlane* a = read_plane(rd, ctx);
        SmExtent2d d = rd.dom4();
        SmPoint3d p0 = rd.vec();
        SmPoint3d p1 = rd.vec();
        double tol = rd.num();
        if (a == NULL)
            throw Bad();
        SmBoolean found = FALSE;
        SmBSplineCurve* c = a->MakeNewPlaneLine(ctx, d, p0, p1, tol, found);
        o.push_back(0.0);
        o.push_back(flg(found));
        put_curve(o, c);
        delete c;
        delete a;
        break;
    }
    case OP_ELLIPSE:
    {
        SmPlane* a = read_plane(rd, ctx);
        SmExtent2d d = rd.dom4();
        SmPoint3d c = rd.vec();
        double rx = rd.num();
        double ry = rd.num();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double i0 = rd.num();
        double i1 = rd.num();
        double tol = rd.num();
        if (a == NULL)
            throw Bad();
        SmTArray<SmBSplineCurve*> out;
        SmStatus s = a->MakeNewPlaneEllipse(ctx, d, c, rx, ry, x, y, SmExtent1d(i0, i1), tol, out);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)out.GetSize());
            for (ULONG i = 0; i < out.GetSize(); i++)
                put_curve(o, out[i]);
        }
        for (ULONG i = 0; i < out.GetSize(); i++)
            delete out[i];
        delete a;
        break;
    }
    case OP_READ_DB:
    {
        bool binary = rd.flag();
        ULONG version = (ULONG)rd.idx(0, 100);
        int64_t nb = rd.idx(0, 1 << 20);
        std::string bytes;
        for (int64_t i = 0; i < nb; i++)
            bytes.push_back((char)(unsigned char)rd.idx(0, 255));
        std::istringstream iss(bytes);
        SmDatabaseIOStream db(binary ? SM_BINARY : SM_ASCII, &iss);
        SmSurface* s = NULL;
        SmStatus stt = SmPlane::ReadFromDB(SmPlane_TYPE, db, ctx, s, version);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            if (s == NULL || !s->IsKindOf(SmPlane_TYPE))
                throw Bad();
            put_state(o, (const SmPlane*)s);
        }
        delete s;
        break;
    }
    default: throw Bad();
    }
}
} // namespace

SC_EXPORT int32_t sc_surface_plane_p2_run(const double* in, int64_t n_in, double* out, int64_t cap,
                                          int64_t* out_len)
{
    if (in == NULL || n_in < 1)
        return SC_ERR_ARG;
    try
    {
        SmContext ctx;
        std::vector<double> o;
        Rd rd{in, n_in, 0};
        int32_t op = (int32_t)rd.idx(1, OP_LAST);
        run_op(op, rd, ctx, o);
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
