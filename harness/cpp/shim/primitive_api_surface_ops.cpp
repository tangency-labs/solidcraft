// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `primitive.api_surface_ops`: the SM_API surface utilities of
// source/SM_API/src/SmApiSurfaces.cpp (SmApiCreateSurface, SmApiCreateSurfaceFromCornerPoints,
// SmApiCreateSurfaceFromOrderedPoints, SmApiCreateOffsetSurface) and the curve-on-surface ones of SmApiCurves.cpp
// (SmApiDropCurveToSrf, SmApiLiftUVCurveFromSrf), called exactly as a caller of the API would,
// on surfaces and curves made by the SM_API constructors.
//
// One entry point, sc_primitive_api_surface_ops_run(in, n_in, out, cap, out_len). `in` is
// `op args...`; layouts are documented in crates/solidcore-ffi/src/units/primitive_api_surface_ops.rs.
// Every read is bounds checked here (a bad program is SC_ERR_ARG). `out` receives `-2` when an
// input's constructor fails, else the API status (0 = SM_SUCCESS) and, on success, the result.
#include "sc_shim.h"

#include <SmApiCurves.h>
#include <SmApiPrimitives.h>
#include <SmApiSurfaces.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFace.h>
#include <SmLine.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <stdexcept>
#include <vector>

#include "sc_sm.h"

using sc::flg;
using sc::put_dom;
using sc::put_ivl;
using sc::put_vec;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/primitive_api_surface_ops.rs).
enum : int32_t
{
    OP_SURFACE = 1,
    OP_CORNERS = 2,
    OP_OFFSET = 3,
    OP_DROP = 4,
    OP_LIFT = 5,
    OP_ORDERED = 6,
};

// Input surface kinds.
enum : int32_t
{
    S_GRID = 0,    // SmApiCreateSurface(points, rows, columns)
    S_CORNERS = 1, // SmApiCreateSurfaceFromCornerPoints(4 corners)
    S_SPHERE = 2,  // the first face's surface of SmApiCreateSphere(center, radius)
    S_TORUS = 3,   // SmApiCreateTorus(center, major, minor, SmSurface*&)
    S_ORDERED = 4, // SmApiCreateSurfaceFromOrderedPoints(points, rows, columns)
};

// Input curve kinds.
enum : int32_t
{
    K_CURVE = 0,   // SmApiCreateCurve(points)
    K_CIRCLE = 1,  // SmApiCreateCircle(center, radius, SmBSplineCurve*&)
    K_ARC = 2,     // SmApiCreateArc(center, radius, start_deg, end_deg)
    K_SEGMENT = 3, // SmApiCreateLineSegment(start, end)
};

// Reads the protected STEP flags through pointers to members named in a derived class.
struct SrfPeek : SmBSplineSurface
{
    static SmKnotType SmBSplineSurface::*kt() { return &SrfPeek::m_eKnotType; }
    static SmBSplineSurfaceForm SmBSplineSurface::*form() { return &SrfPeek::m_eBSplineSurfaceForm; }
};

// Access to the curve's private STEP flags through explicit instantiation.
template <typename Tag, typename Tag::type M>
struct Rob
{
    friend typename Tag::type get(Tag) { return M; }
};

struct BsForm
{
    typedef SmBSplineCurveForm SmBSplineCurve::*type;
    friend type get(BsForm);
};
template struct Rob<BsForm, &SmBSplineCurve::m_eBSplineCurveForm>;

struct BsKnotType
{
    typedef SmKnotType SmBSplineCurve::*type;
    friend type get(BsKnotType);
};
template struct Rob<BsKnotType, &SmBSplineCurve::m_eKnotType>;

struct Bad : std::runtime_error
{
    Bad() : std::runtime_error("bad program") {}
};

// An input's constructor failed: the case compares only this marker.
struct BuildFailed
{
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

// Owned results and inputs of a case.
template <typename T> struct Own
{
    T* p = NULL;
    ~Own() { delete p; }
};

struct OwnArr
{
    SmTArray<SmBSplineCurve*> a;
    ~OwnArr()
    {
        for (ULONG i = 0; i < a.GetSize(); i++)
            delete a[i];
    }
};

// `has_nurb knot_type form [n m p q r s cpts u v]` of a B-spline surface.
void put_bspline_surface(std::vector<double>& o, const SmBSplineSurface* s)
{
    const gw_SURFACE* sur = s->GetGwNurbPointer();
    o.push_back(flg(sur != NULL));
    o.push_back((double)(s->*SrfPeek::kt()));
    o.push_back((double)(s->*SrfPeek::form()));
    if (!sur)
        return;
    gw_INDEX n = sur->net->n;
    gw_INDEX m = sur->net->m;
    gw_INDEX r = sur->knu->m;
    gw_INDEX ss = sur->knv->m;
    o.push_back((double)n);
    o.push_back((double)m);
    o.push_back((double)sur->p);
    o.push_back((double)sur->q);
    o.push_back((double)r);
    o.push_back((double)ss);
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
    for (gw_INDEX i = 0; i <= ss; i++)
        o.push_back(sur->knv->U[i]);
}

// `type domain(4)`, then 9 samples `status point(3)` at the domain's (0, .5, 1) grid.
void put_surface_samples(std::vector<double>& o, const SmSurface* s)
{
    o.push_back((double)s->GetType());
    SmExtent2d dom = s->GetNaturalUVDomain();
    put_dom(o, dom);
    const double f[3] = {0.0, 0.5, 1.0};
    for (int i = 0; i < 3; i++)
        for (int j = 0; j < 3; j++)
        {
            SmPoint2d uv = dom.Evaluate(f[i], f[j]);
            SmPoint3d p(0, 0, 0);
            SmStatus ps = s->EvaluatePoint(uv, p);
            o.push_back(st(ps));
            if (ps == SM_SUCCESS)
                put_vec(o, p);
        }
}

// type dim knot_type form, the NURB (`has n p m Pw U`), natural interval.
void put_bspline(std::vector<double>& o, SmBSplineCurve* c)
{
    o.push_back((double)c->GetType());
    o.push_back((double)c->GetDim());
    o.push_back((double)(c->*get(BsKnotType())));
    o.push_back((double)(c->*get(BsForm())));
    const gw_CURVE* cur = c->GetGwNurbPointer();
    o.push_back(cur ? 1.0 : 0.0);
    if (cur)
    {
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
        put_ivl(o, c->GetNaturalInterval());
    }
}

void put_curves(std::vector<double>& o, const SmTArray<SmBSplineCurve*>& a)
{
    o.push_back((double)a.GetSize());
    for (ULONG i = 0; i < a.GetSize(); i++)
        put_bspline(o, a[i]);
}

// An input surface: `p`, owned by `own`, or by the face of `brep` it was taken from.
struct InSrf
{
    Own<SmSurface> own;
    Own<SmBrep> brep;
    SmSurface* p = NULL;
};

// `0 rows cols n pts(3n)` (grid), `1 corners(4x3)` (corners), `2 center(3) radius` (sphere
// face) or `3 center(3) major minor` (torus).
void read_surface(Rd& rd, InSrf& srf)
{
    int64_t kind = rd.idx(S_GRID, S_ORDERED);
    SmStatus s;
    if (kind == S_GRID || kind == S_ORDERED)
    {
        ULONG rows = (ULONG)rd.idx(0, (int64_t)1 << 40);
        ULONG cols = (ULONG)rd.idx(0, (int64_t)1 << 40);
        int64_t np = rd.idx(0, 4096);
        SmTArray<SmPoint3d> pts;
        for (int64_t i = 0; i < np; i++)
            pts.Add(rd.vec());
        if (kind == S_GRID)
            s = SmApiCreateSurface(pts, rows, cols, srf.own.p);
        else
            s = SmApiCreateSurfaceFromOrderedPoints(pts, rows, cols, srf.own.p);
        srf.p = srf.own.p;
    }
    else if (kind == S_CORNERS)
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmPoint3d c = rd.vec();
        SmPoint3d d = rd.vec();
        s = SmApiCreateSurfaceFromCornerPoints(a, b, c, d, srf.own.p);
        srf.p = srf.own.p;
    }
    else if (kind == S_SPHERE)
    {
        SmVector3d c = rd.vec();
        double r = rd.num();
        s = SmApiCreateSphere(c, r, srf.brep.p);
        if (s == SM_SUCCESS)
        {
            SmTArray<SmFace*> faces;
            srf.brep.p->GetFaces(faces);
            if (faces.GetSize() == 0)
                throw BuildFailed();
            srf.p = faces[0]->GetSurface();
            if (!srf.p)
                throw BuildFailed();
        }
    }
    else
    {
        SmVector3d c = rd.vec();
        double major = rd.num();
        double minor = rd.num();
        s = SmApiCreateTorus(c, major, minor, srf.own.p);
        srf.p = srf.own.p;
    }
    if (s != SM_SUCCESS)
        throw BuildFailed();
}

SmBSplineCurve* read_curve(Rd& rd)
{
    int64_t kind = rd.idx(K_CURVE, K_SEGMENT);
    SmStatus s = SM_SUCCESS;
    SmBSplineCurve* res = NULL;
    switch (kind)
    {
    case K_CURVE:
    {
        int64_t np = rd.idx(0, 4096);
        SmTArray<SmPoint3d> pts;
        for (int64_t i = 0; i < np; i++)
            pts.Add(rd.vec());
        s = SmApiCreateCurve(pts, res);
        break;
    }
    case K_CIRCLE:
    {
        SmPoint3d c = rd.vec();
        double r = rd.num();
        s = SmApiCreateCircle(c, r, res);
        break;
    }
    case K_ARC:
    {
        SmPoint3d c = rd.vec();
        double r = rd.num();
        double a0 = rd.num();
        double a1 = rd.num();
        s = SmApiCreateArc(c, r, a0, a1, res);
        break;
    }
    default:
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine* l = NULL;
        s = SmApiCreateLineSegment(a, b, l);
        res = l;
        break;
    }
    }
    if (s != SM_SUCCESS)
    {
        delete res;
        throw BuildFailed();
    }
    return res;
}

void run(Rd& rd, std::vector<double>& o)
{
    int64_t op = rd.idx(OP_SURFACE, OP_ORDERED);
    switch (op)
    {
    case OP_SURFACE:
    case OP_CORNERS:
    case OP_ORDERED:
    {
        // the surface spec of the matching kind (its own status is the record's)
        Own<SmSurface> res;
        SmStatus s;
        if (op == OP_SURFACE || op == OP_ORDERED)
        {
            ULONG rows = (ULONG)rd.idx(0, (int64_t)1 << 40);
            ULONG cols = (ULONG)rd.idx(0, (int64_t)1 << 40);
            int64_t np = rd.idx(0, 4096);
            SmTArray<SmPoint3d> pts;
            for (int64_t i = 0; i < np; i++)
                pts.Add(rd.vec());
            if (op == OP_SURFACE)
                s = SmApiCreateSurface(pts, rows, cols, res.p);
            else
                s = SmApiCreateSurfaceFromOrderedPoints(pts, rows, cols, res.p);
        }
        else
        {
            SmPoint3d a = rd.vec();
            SmPoint3d b = rd.vec();
            SmPoint3d c = rd.vec();
            SmPoint3d d = rd.vec();
            s = SmApiCreateSurfaceFromCornerPoints(a, b, c, d, res.p);
        }
        o.push_back(st(s));
        if (s == SM_SUCCESS && res.p)
        {
            o.push_back((double)res.p->GetType());
            put_bspline_surface(o, (SmBSplineSurface*)res.p);
            put_surface_samples(o, res.p);
        }
        return;
    }
    case OP_OFFSET:
    {
        // surface distance
        InSrf srf;
        read_surface(rd, srf);
        double d = rd.num();
        Own<SmSurface> res;
        SmStatus s = SmApiCreateOffsetSurface(srf.p, d, res.p);
        o.push_back(st(s));
        if (s == SM_SUCCESS && res.p)
            put_surface_samples(o, res.p);
        return;
    }
    case OP_DROP:
    {
        // curve surface lift
        Own<SmBSplineCurve> crv;
        crv.p = read_curve(rd);
        InSrf srf;
        read_surface(rd, srf);
        bool lift = rd.flag();
        OwnArr uv, c3d;
        SmStatus s = SmApiDropCurveToSrf(crv.p, srf.p, &uv.a, lift ? &c3d.a : NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            put_curves(o, uv.a);
            if (lift)
                put_curves(o, c3d.a);
        }
        return;
    }
    default:
    {
        // OP_LIFT: uv_curve surface
        Own<SmBSplineCurve> crv;
        crv.p = read_curve(rd);
        InSrf srf;
        read_surface(rd, srf);
        OwnArr c3d;
        SmStatus s = SmApiLiftUVCurveFromSrf(crv.p, srf.p, &c3d.a);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_curves(o, c3d.a);
        return;
    }
    }
}
} // namespace

SC_EXPORT int32_t sc_primitive_api_surface_ops_run(const double* in, int64_t n_in, double* out,
                                                   int64_t cap, int64_t* out_len)
{
    SC_TRY
    Rd rd{in, n_in, 0};
    std::vector<double> o;
    try
    {
        run(rd, o);
    }
    catch (const Bad&)
    {
        return SC_ERR_ARG;
    }
    catch (const BuildFailed&)
    {
        o.assign(1, -2.0);
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    if (rd.at != rd.n)
        return SC_ERR_ARG;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
