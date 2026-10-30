// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `primitive.api_trim_ops`: the SM_API trimmed-surface functions of
// source/SM_API/src/SmApiTrimmedSurfaces.cpp (SmApiTrimSurfaceWith3dCurves,
// SmApiTrimProjectParallel, SmApiCreatePlanarFaces) and SmApiProjectAndTrim
// (SmApiBrep.cpp), called exactly as a caller of the API would, on surfaces, curves and
// boxes made by the SM_API constructors.
//
// One entry point, sc_primitive_api_trim_ops_run(in, n_in, out, cap, out_len). `in` is
// `op args...`; layouts are documented in crates/solidcore-ffi/src/units/primitive_api_trim_ops.rs.
// Every read is bounds checked here (a bad program is SC_ERR_ARG). `out` receives `-2` when an
// input's constructor fails, else the API status (0 = SM_SUCCESS) and, on success, the B-rep:
// `has faces edges vertices` then support/primitive_record.h's record.
//
// The projection curve may be any SmBSplineCurve class (an SmLine, an SmCircle), as
// upstream's tests pass. The APIs consume the surfaces and curves they are given (they become, or are deleted with,
// the new B-rep), so the shim never frees them after a call; what a failed call leaks stays
// in this process.
#include "sc_shim.h"

#include "StdAfx.h"
#include "support/primitive_record.h"
#include <SmApiBrep.h>
#include <SmApiCurves.h>
#include <SmApiFillets.h>
#include <SmApiPrimitives.h>
#include <SmApiSurfaces.h>
#include <SmApiTrimmedSurfaces.h>
#include <SmBSplineCurve.h>
#include <SmBrep.h>
#include <SmCircle.h>
#include <SmCurve.h>
#include <SmLine.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmTypes.h>
#include <SmVector3d.h>

#include <stdexcept>
#include <vector>

#include "sc_sm.h"

using sc::flg;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/primitive_api_trim_ops.rs).
enum : int32_t
{
    OP_TRIM_3D = 1,
    OP_PROJECT_PARALLEL = 2,
    OP_PLANAR_FACES = 3,
    OP_PROJECT_AND_TRIM = 4,
};

// Input curve kinds.
enum : int32_t
{
    K_CURVE = 0,     // SmApiCreateCurve(points)
    K_CIRCLE = 1,    // SmApiCreateCircle(center, radius, SmBSplineCurve*&)
    K_ARC = 2,       // SmApiCreateArc(center, radius, start_deg, end_deg)
    K_SEGMENT = 3,   // SmApiCreateLineSegment(start, end)
    K_ANALYTIC = 4,  // SmApiCreateCircle(center, radius, SmCircle*&)
    K_RECTANGLE = 5, // SmApiCreateRectangle(center, length, width): 4 segments
};

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
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
};

// Appends the curves one spec makes (kinds 0 to max_kind).
void read_curve(Rd& rd, int64_t max_kind, SmTArray<SmCurve*>& out)
{
    int64_t kind = rd.idx(K_CURVE, max_kind);
    SmStatus s = SM_SUCCESS;
    switch (kind)
    {
    case K_CURVE:
    {
        int64_t np = rd.idx(0, 4096);
        SmTArray<SmPoint3d> pts;
        for (int64_t i = 0; i < np; i++)
            pts.Add(rd.vec());
        SmBSplineCurve* c = NULL;
        s = SmApiCreateCurve(pts, c);
        if (c)
            out.Add(c);
        break;
    }
    case K_CIRCLE:
    {
        SmPoint3d c0 = rd.vec();
        double r = rd.num();
        SmBSplineCurve* c = NULL;
        s = SmApiCreateCircle(c0, r, c);
        if (c)
            out.Add(c);
        break;
    }
    case K_ARC:
    {
        SmPoint3d c0 = rd.vec();
        double r = rd.num();
        double a0 = rd.num();
        double a1 = rd.num();
        SmBSplineCurve* c = NULL;
        s = SmApiCreateArc(c0, r, a0, a1, c);
        if (c)
            out.Add(c);
        break;
    }
    case K_SEGMENT:
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine* l = NULL;
        s = SmApiCreateLineSegment(a, b, l);
        if (l)
            out.Add(l);
        break;
    }
    case K_ANALYTIC:
    {
        SmPoint3d c0 = rd.vec();
        double r = rd.num();
        SmCircle* c = NULL;
        s = SmApiCreateCircle(c0, r, c);
        if (c)
            out.Add(c);
        break;
    }
    default:
    {
        SmPoint3d c0 = rd.vec();
        double len = rd.num();
        double wid = rd.num();
        SmTArray<SmBSplineCurve*> segs;
        s = SmApiCreateRectangle(c0, len, wid, segs);
        for (ULONG i = 0; i < segs.GetSize(); i++)
            out.Add(segs[i]);
        break;
    }
    }
    if (s != SM_SUCCESS)
        throw BuildFailed();
}

void read_curves(Rd& rd, int64_t max_kind, SmTArray<SmCurve*>& out)
{
    int64_t count = rd.idx(0, 64);
    for (int64_t i = 0; i < count; i++)
        read_curve(rd, max_kind, out);
}

// `kind args`: 0 `rows columns n pts(3n)` (SmApiCreateSurface), 1 `corners(4x3)`.
SmSurface* read_surface(Rd& rd)
{
    int64_t kind = rd.idx(0, 1);
    SmSurface* res = NULL;
    SmStatus s;
    if (kind == 0)
    {
        ULONG rows = (ULONG)rd.idx(0, (int64_t)1 << 40);
        ULONG cols = (ULONG)rd.idx(0, (int64_t)1 << 40);
        int64_t np = rd.idx(0, 4096);
        SmTArray<SmPoint3d> pts;
        for (int64_t i = 0; i < np; i++)
            pts.Add(rd.vec());
        s = SmApiCreateSurface(pts, rows, cols, res);
    }
    else
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmPoint3d c = rd.vec();
        SmPoint3d d = rd.vec();
        s = SmApiCreateSurfaceFromCornerPoints(a, b, c, d, res);
    }
    if (s != SM_SUCCESS)
        throw BuildFailed();
    return res;
}

// `origin(3) length width height fillet [radius]` (SmApiCreateBox, then with `fillet`
// SmApiCircularFillet(radius) on it).
SmBrep* read_box(Rd& rd)
{
    SmVector3d o = rd.vec();
    double l = rd.num();
    double w = rd.num();
    double h = rd.num();
    bool fillet = rd.idx(0, 1) == 1;
    double r = fillet ? rd.num() : 0.0;
    SmBrep* b = NULL;
    if (SmApiCreateBox(o, l, w, h, b) != SM_SUCCESS)
        throw BuildFailed();
    if (fillet && SmApiCircularFillet(b, r) != SM_SUCCESS)
        throw BuildFailed();
    return b;
}

// The projection curve: one of the SmBSplineCurve classes (kinds curve, circle, arc,
// segment, analytic circle).
SmBSplineCurve* read_projection_curve(Rd& rd)
{
    SmTArray<SmCurve*> one;
    read_curve(rd, K_ANALYTIC, one);
    return static_cast<SmBSplineCurve*>(one[0]);
}

// The status, then on success `has` and the B-rep's counts and record (the B-rep is deleted).
void put_brep(std::vector<double>& o, SmStatus s, SmBrep* b)
{
    o.push_back(st(s));
    if (s != SM_SUCCESS)
        return;
    o.push_back(flg(b != NULL));
    if (b)
    {
        o.push_back((double)b->GetNumFaces());
        o.push_back((double)b->GetNumEdges());
        o.push_back((double)b->GetNumVertices());
        primitive_record::append(o, b);
    }
    delete b;
}

void run(Rd& rd, std::vector<double>& o)
{
    int64_t op = rd.idx(OP_TRIM_3D, OP_PROJECT_AND_TRIM);
    switch (op)
    {
    case OP_TRIM_3D:
    {
        // surface nloops loop_counts(nloops) curves
        SmSurface* srf = read_surface(rd);
        int64_t nl = rd.idx(0, 64);
        SmTArray<ULONG> loops;
        for (int64_t i = 0; i < nl; i++)
            loops.Add((ULONG)rd.idx(0, 4096));
        SmTArray<SmCurve*> curves;
        read_curves(rd, K_RECTANGLE, curves);
        SmBrep* res = NULL;
        SmStatus s = SmApiTrimSurfaceWith3dCurves(srf, loops, curves, res);
        put_brep(o, s, res);
        return;
    }
    case OP_PROJECT_PARALLEL:
    {
        // object(0 surface | 1 box) curve direction(3) reference(3) trim_type again
        // [curve direction(3) reference(3) trim_type]: with `again`, the result is trimmed
        // once more (TestSmTrimProjectParallel's second call), recorded as the first
        // status then the second call's record
        int64_t obj = rd.idx(0, 1);
        SmObject* object = obj == 0 ? (SmObject*)read_surface(rd) : (SmObject*)read_box(rd);
        SmBSplineCurve* crv = read_projection_curve(rd);
        SmVector3d dir = rd.vec();
        SmPoint3d ref = rd.vec();
        SmTrimType tt = (SmTrimType)rd.idx(SM_TT_KEEP_POINT, SM_TT_SPLIT);
        bool again = rd.idx(0, 1) == 1;
        SmBSplineCurve* crv2 = NULL;
        SmVector3d dir2;
        SmPoint3d ref2;
        SmTrimType tt2 = SM_TT_KEEP_POINT;
        if (again)
        {
            crv2 = read_projection_curve(rd);
            dir2 = rd.vec();
            ref2 = rd.vec();
            tt2 = (SmTrimType)rd.idx(SM_TT_KEEP_POINT, SM_TT_SPLIT);
        }
        SmBrep* res = NULL;
        SmStatus s = SmApiTrimProjectParallel(object, crv, dir, ref, tt, res);
        if (!again)
        {
            put_brep(o, s, res);
            return;
        }
        o.push_back(st(s));
        if (s != SM_SUCCESS)
            return;
        s = SmApiTrimProjectParallel(res, crv2, dir2, ref2, tt2, res);
        put_brep(o, s, res);
        return;
    }
    case OP_PLANAR_FACES:
    {
        // curves
        SmTArray<SmCurve*> curves;
        read_curves(rd, K_RECTANGLE, curves);
        SmBrep* res = NULL;
        SmStatus s = SmApiCreatePlanarFaces(curves, res);
        put_brep(o, s, res);
        return;
    }
    default:
    {
        // OP_PROJECT_AND_TRIM: box curve direction(3) reference(3)
        SmBrep* b = read_box(rd);
        SmBSplineCurve* crv = read_projection_curve(rd);
        SmVector3d dir = rd.vec();
        SmPoint3d ref = rd.vec();
        SmStatus s = SmApiProjectAndTrim(b, crv, dir, ref);
        put_brep(o, s, b);
        if (s != SM_SUCCESS)
            delete b;
        return;
    }
    }
}
} // namespace

SC_EXPORT int32_t sc_primitive_api_trim_ops_run(const double* in, int64_t n_in, double* out,
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
