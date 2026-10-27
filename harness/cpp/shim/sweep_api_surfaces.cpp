// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `sweep.api_surfaces`: the SM_API surface and skin entry points of
// source/SM_API/src/SmApiSurfaces.cpp (SmApiCreateExtrude, SmApiCreateSweep,
// SmApiCreateRuledSurface, SmApiCreateSurfaceRevolution, SmApiCreateSkin) and
// source/SM_API/src/SmApiPrimitives.cpp (SmApiCreateSkinPrimitive, SmApiCreateSkinFromFaces),
// called exactly as a caller of the API would.
//
// One entry point, sc_sweep_api_surfaces_run(in, n_in, out, cap, out_len). `in` is
// `op args...`; layouts are documented in crates/solidcore-ffi/src/units/sweep_api_surfaces.rs,
// which validates every input so upstream only touches valid memory. Every read is bounds
// checked here as well (a malformed program is SC_ERR_ARG). The whole program is parsed before
// any upstream object is built. `out` receives the API status (0 = SM_SUCCESS, else the
// SmStatus) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmApiCurves.h>
#include <SmApiGeneral.h>
#include <SmApiPrimitives.h>
#include <SmApiSurfaces.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmFace.h>
#include <SmLine.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <stdexcept>
#include <vector>

#include "sc_sm.h"

using sc::flg;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/sweep_api_surfaces.rs).
enum : int32_t
{
    OP_EXTRUDE = 1,
    OP_SWEEP = 2,
    OP_RULED = 3,
    OP_REVOLUTION = 4,
    OP_SKIN = 5,
    OP_SKIN_PRIMITIVE = 6,
    OP_SKIN_FROM_FACES = 7,
};

// Curve kinds of a curve block.
enum : int32_t
{
    CK_NULL = 0,            // NULL
    CK_NURB = 1,            // SmBSplineCurve(gw_CURVE*, dim, FALSE)
    CK_API_CURVE = 2,       // SmApiCreateCurve(points)
    CK_API_ARC = 3,         // SmApiCreateArc(center, radius, start, end)
    CK_API_CIRCLE = 4,      // SmApiCreateCircle(center, radius, SmBSplineCurve*&)
    CK_LINE_SEGMENT = 5,    // SmApiCreateLineSegment(start, end) (real SmLine subclass)
    CK_ANALYTIC_CIRCLE = 6, // SmApiCreateCircle(center, radius, SmCircle*&) (real SmCircle)
};

struct Bad : std::runtime_error
{
    Bad() : std::runtime_error("bad program") {}
};

// Reads the protected STEP flags through pointers to members named in a derived class.
struct Peek : SmBSplineSurface
{
    static SmKnotType SmBSplineSurface::*kt() { return &Peek::m_eKnotType; }
    static SmBSplineSurfaceForm SmBSplineSurface::*form() { return &Peek::m_eBSplineSurfaceForm; }
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
    std::vector<double> nums(int64_t k)
    {
        std::vector<double> v;
        for (int64_t i = 0; i < k; i++)
            v.push_back(num());
        return v;
    }
};

// A parsed curve block.
struct CurveSpec
{
    int32_t kind = CK_NULL;
    int64_t dim = 3, n = 0, p = 0, m = 0;
    std::vector<double> data;
};

CurveSpec read_spec(Rd& rd)
{
    CurveSpec c;
    c.kind = (int32_t)rd.idx(0, 6);
    switch (c.kind)
    {
    case CK_NULL: break;
    case CK_NURB:
        c.dim = rd.idx(1, 3);
        c.n = rd.idx(0, 4096);
        c.p = rd.idx(0, 32);
        c.m = rd.idx(0, 4096 + 33);
        c.data = rd.nums(4 * (c.n + 1) + c.m + 1);
        break;
    case CK_API_CURVE:
    {
        int64_t np = rd.idx(0, 4096);
        c.data = rd.nums(3 * np);
        break;
    }
    case CK_API_ARC: c.data = rd.nums(6); break;
    case CK_API_CIRCLE: c.data = rd.nums(4); break;
    case CK_LINE_SEGMENT: c.data = rd.nums(6); break;
    case CK_ANALYTIC_CIRCLE: c.data = rd.nums(4); break;
    default: throw Bad();
    }
    return c;
}

SmVector3d v3(const double* d)
{
    return SmVector3d(d[0], d[1], d[2]);
}

// Builds the curve of a parsed block (NULL for CK_NULL). A failing API constructor is a
// harness error (the Rust side builds the same curves and rejects the same inputs).
SmCurve* build_curve(const CurveSpec& c)
{
    const SmContext& ctx = *SmApiGetOrCreateContext();
    const double* d = c.data.data();
    switch (c.kind)
    {
    case CK_NULL: return NULL;
    case CK_NURB:
    {
        gw_CURVE* cur = sm_AllocateNurbCurve((gw_INDEX)c.n, (gw_DEGREE)c.p, (gw_INDEX)c.m);
        int64_t at = 0;
        for (int64_t i = 0; i <= c.n; i++)
        {
            cur->pol->Pw[i].x = d[at++];
            cur->pol->Pw[i].y = d[at++];
            cur->pol->Pw[i].z = d[at++];
            cur->pol->Pw[i].w = d[at++];
        }
        for (int64_t i = 0; i <= c.m; i++)
            cur->knt->U[i] = d[at++];
        return new (ctx) SmBSplineCurve(cur, (ULONG)c.dim, FALSE);
    }
    case CK_API_CURVE:
    {
        SmTArray<SmPoint3d> pts;
        for (size_t i = 0; i + 2 < c.data.size(); i += 3)
            pts.Add(SmPoint3d(d[i], d[i + 1], d[i + 2]));
        SmBSplineCurve* r = NULL;
        if (SmApiCreateCurve(pts, r) != SM_SUCCESS || r == NULL)
            throw Bad();
        return r;
    }
    case CK_API_ARC:
    {
        SmBSplineCurve* r = NULL;
        if (SmApiCreateArc(v3(d), d[3], d[4], d[5], r) != SM_SUCCESS || r == NULL)
            throw Bad();
        return r;
    }
    case CK_API_CIRCLE:
    {
        SmBSplineCurve* r = NULL;
        if (SmApiCreateCircle(v3(d), d[3], r) != SM_SUCCESS || r == NULL)
            throw Bad();
        return r;
    }
    case CK_LINE_SEGMENT:
    {
        SmLine* r = NULL;
        if (SmApiCreateLineSegment(v3(d), v3(d + 3), r) != SM_SUCCESS || r == NULL)
            throw Bad();
        return r;
    }
    case CK_ANALYTIC_CIRCLE:
    {
        SmCircle* r = NULL;
        if (SmApiCreateCircle(v3(d), d[3], r) != SM_SUCCESS || r == NULL)
            throw Bad();
        return r;
    }
    default: throw Bad();
    }
}

// Owns the curves built for one call.
struct Curves
{
    std::vector<SmCurve*> all;
    ~Curves()
    {
        for (size_t i = 0; i < all.size(); i++)
            delete all[i];
    }
    SmCurve* add(const CurveSpec& c)
    {
        all.push_back(NULL);
        SmCurve* r = build_curve(c);
        all.back() = r;
        return r;
    }
    // SmLine : SmBSplineCurve (SmLine.h:49); SmCircle : SmEllipse : SmBSplineCurve
    // (SmCircle.h:59, SmEllipse.h:243). Admit these real subclasses only for
    // CreateLinearSweep, which uses their NURB (SmBSplineSurface.cpp:1588-1629).
    SmBSplineCurve* add_bspline(const CurveSpec& c, bool linear_sweep = false)
    {
        const bool linear_subclass = linear_sweep &&
            (c.kind == CK_LINE_SEGMENT || c.kind == CK_ANALYTIC_CIRCLE);
        if (c.kind == CK_NULL || (c.kind >= CK_LINE_SEGMENT && !linear_subclass))
            throw Bad();
        return (SmBSplineCurve*)add(c);
    }
};

// `has` then, when set, `has_nurb oob borrowed knot_type form [n m p q r s cpts u v]`.
void put_surface(std::vector<double>& o, const SmSurface* srf)
{
    const SmBSplineSurface* s = dynamic_cast<const SmBSplineSurface*>(srf);
    o.push_back(flg(s != NULL));
    if (!s)
        return;
    const gw_SURFACE* sur = s->GetGwNurbPointer();
    o.push_back(flg(sur != NULL));
    o.push_back(flg(s->IsOutOfBoundsEnabled()));
    o.push_back(flg(s->IsNurbBorrowed()));
    o.push_back((double)(s->*Peek::kt()));
    o.push_back((double)(s->*Peek::form()));
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

// The status, then on success the created surface.
void put_result(std::vector<double>& o, SmApiStatus stat, SmSurface* res)
{
    o.push_back(st(stat));
    if (stat == SM_SUCCESS)
        put_surface(o, res);
    delete res;
}

void run(Rd& rd, std::vector<double>& o)
{
    int32_t op = (int32_t)rd.idx(1, 7);
    switch (op)
    {
    case OP_EXTRUDE:
    {
        CurveSpec cs = read_spec(rd);
        std::vector<double> v = rd.nums(3);
        if (rd.at != rd.n)
            throw Bad();
        Curves cv;
        SmBSplineCurve* c = cv.add_bspline(cs, true);
        SmSurface* res = NULL;
        SmApiStatus stat = SmApiCreateExtrude(c, v3(v.data()), res);
        put_result(o, stat, res);
        // the extruded curve's NURB afterwards (NLib makes a 2D curve 3D in place)
        const gw_CURVE* cur = c->GetGwNurbPointer();
        o.push_back(flg(cur != NULL));
        if (cur)
        {
            o.push_back((double)cur->pol->n);
            for (gw_INDEX i = 0; i <= cur->pol->n; i++)
            {
                o.push_back(cur->pol->Pw[i].x);
                o.push_back(cur->pol->Pw[i].y);
                o.push_back(cur->pol->Pw[i].z);
                o.push_back(cur->pol->Pw[i].w);
            }
        }
        break;
    }
    case OP_SWEEP:
    case OP_RULED:
    {
        CurveSpec a = read_spec(rd);
        CurveSpec b = read_spec(rd);
        if (rd.at != rd.n)
            throw Bad();
        Curves cv;
        SmBSplineCurve* c1 = cv.add_bspline(a);
        SmBSplineCurve* c2 = cv.add_bspline(b);
        SmSurface* res = NULL;
        SmApiStatus stat = op == OP_SWEEP ? SmApiCreateSweep(c1, c2, res)
                                          : SmApiCreateRuledSurface(c1, c2, res);
        put_result(o, stat, res);
        break;
    }
    case OP_REVOLUTION:
    {
        CurveSpec cs = read_spec(rd);
        std::vector<double> ax = rd.nums(7);
        if (rd.at != rd.n)
            throw Bad();
        Curves cv;
        SmBSplineCurve* c = cs.kind == CK_NULL ? NULL : cv.add_bspline(cs);
        SmSurface* res = NULL;
        SmApiStatus stat = SmApiCreateSurfaceRevolution(c, v3(ax.data()), v3(ax.data() + 3),
                                                        ax[6], res);
        o.push_back(st(stat));
        if (stat == SM_SUCCESS) {
            o.push_back(flg(res != NULL));
            if (res) {
                o.push_back(flg(res->GetOwner() == NULL));
                const auto domain = res->GetNaturalUVDomain();
                o.push_back(domain.GetMin().x);o.push_back(domain.GetMin().y);
                o.push_back(domain.GetMax().x);o.push_back(domain.GetMax().y);
                for (int u = 0; u <= 4; ++u) for (int v = 0; v <= 4; ++v) {
                    SmPoint3d point;
                    const auto status = res->EvaluatePoint(domain.Evaluate(u/4.0,v/4.0),point);
                    o.push_back(st(status));
                    if (status == SM_SUCCESS) {
                        o.push_back(point.x);o.push_back(point.y);o.push_back(point.z);
                    }
                }
            }
        }
        delete res;
        break;
    }
    case OP_SKIN:
    {
        int64_t k = rd.idx(0, 64);
        std::vector<CurveSpec> specs;
        for (int64_t i = 0; i < k; i++)
            specs.push_back(read_spec(rd));
        if (rd.at != rd.n)
            throw Bad();
        Curves cv;
        SmTArray<SmBSplineCurve*> profiles;
        for (size_t i = 0; i < specs.size(); i++)
            profiles.Add(cv.add_bspline(specs[i]));
        SmSurface* res = NULL;
        SmApiStatus stat = SmApiCreateSkin(profiles, res);
        put_result(o, stat, res);
        break;
    }
    case OP_SKIN_PRIMITIVE:
    {
        int64_t np = rd.idx(0, 64);
        SmTArray<ULONG> counts;
        for (int64_t i = 0; i < np; i++)
            counts.Add((ULONG)rd.idx(0, 1 << 20));
        int64_t nc = rd.idx(0, 256);
        std::vector<CurveSpec> specs;
        for (int64_t i = 0; i < nc; i++)
            specs.push_back(read_spec(rd));
        ULONG degree = (ULONG)rd.idx(0, 1 << 20);
        bool cap = rd.flag();
        if (rd.at != rd.n)
            throw Bad();
        Curves cv;
        SmTArray<const SmCurve*> curves;
        for (size_t i = 0; i < specs.size(); i++)
            curves.Add(cv.add(specs[i]));
        SmBrep* res = NULL;
        SmTArray<SmFace*> start, side, end;
        SmApiStatus stat = SmApiCreateSkinPrimitive(counts, curves, degree, cap ? TRUE : FALSE,
                                                    res, &start, &side, &end);
        o.push_back(st(stat));
        if (stat == SM_SUCCESS)
        {
            o.push_back(flg(res != NULL));
            o.push_back((double)start.GetSize());
            o.push_back((double)side.GetSize());
            o.push_back((double)end.GetSize());
        }
        delete res;
        break;
    }
    case OP_SKIN_FROM_FACES:
    {
        int64_t nf = rd.idx(0, 16);
        std::vector<double> circles = rd.nums(4 * nf);
        // 0 and above NL_MAXDEG are refused by the API before any work
        ULONG degree = (ULONG)rd.idx(0, 1 << 20);
        if ((degree >= 4 && degree <= 17) || rd.at != rd.n)
            throw Bad();
        std::vector<SmBrep*> planes;
        SmTArray<SmFace*> faces;
        bool ok = true;
        for (int64_t i = 0; i < nf && ok; i++)
        {
            SmBrep* b = NULL;
            const double* d = circles.data() + 4 * i;
            if (SmApiCreatePlanarCircle(v3(d), d[3], b) != SM_SUCCESS || b == NULL)
            {
                ok = false;
                break;
            }
            planes.push_back(b);
            SmTArray<SmFace*> fs;
            b->GetFaces(fs);
            if (fs.GetSize() > 0)
                faces.Add(fs[0]);
        }
        if (!ok)
        {
            for (size_t i = 0; i < planes.size(); i++)
                delete planes[i];
            throw Bad();
        }
        SmTArray<SmFace*> new_faces;
        SmBrep* res = NULL;
        SmApiStatus stat = SmApiCreateSkinFromFaces(faces, degree, new_faces, res);
        o.push_back(st(stat));
        if (stat == SM_SUCCESS)
        {
            o.push_back(flg(res != NULL));
            o.push_back((double)new_faces.GetSize());
        }
        delete res;
        for (size_t i = 0; i < planes.size(); i++)
            delete planes[i];
        break;
    }
    default: throw Bad();
    }
}
} // namespace

SC_EXPORT int32_t sc_sweep_api_surfaces_run(const double* in, int64_t n_in, double* out,
                                            int64_t cap, int64_t* out_len)
{
    SC_TRY
    SmApiGetOrCreateContext();
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
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
