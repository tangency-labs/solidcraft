// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `primitive.api_curve_ops`: the SM_API curve utilities of
// source/SM_API/src/SmApiCurves.cpp (SmApiCreateCurveInterpPoints, SmApiOffsetCurve,
// SmApiEvaluateContinuity, SmApiMakeCurvesCompatible, SmApiOrderCurves), called exactly
// as a caller of the API would, on curves made by the SM_API curve constructors.
//
// One entry point, sc_primitive_api_curve_ops_run(in, n_in, out, cap, out_len). `in` is
// `op args...`; layouts are documented in crates/solidcore-ffi/src/units/primitive_api_curve_ops.rs.
// Every read is bounds checked here (a bad program is SC_ERR_ARG). `out` receives `-2` when an
// input curve's constructor fails, else the API status (0 = SM_SUCCESS) and, on success, the
// result. Private members are read through explicit-instantiation access.
#include "sc_shim.h"

#include <SmApiCurves.h>
#include <SmBSplineCurve.h>
#include <SmCircle.h>
#include <SmCurveTypes.h>
#include <SmExtent1d.h>
#include <SmLine.h>
#include <SmTArray.h>
#include <SmTypes.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <stdexcept>
#include <vector>

#include "sc_sm.h"

using sc::put_ivl;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/primitive_api_curve_ops.rs).
enum : int32_t
{
    OP_INTERP = 1,
    OP_OFFSET = 2,
    OP_CONTINUITY = 3,
    OP_COMPATIBLE = 4,
    OP_ORDER = 5,
};

// Input curve kinds.
enum : int32_t
{
    K_CURVE = 0,     // SmApiCreateCurve(points)
    K_CIRCLE = 1,    // SmApiCreateCircle(center, radius, SmBSplineCurve*&)
    K_ARC = 2,       // SmApiCreateArc(center, radius, start_deg, end_deg)
    K_SEGMENT = 3,   // SmApiCreateLineSegment(start, end)
    K_ANALYTIC = 4,  // SmApiCreateCircle(center, radius, SmCircle*&)
    K_RECTANGLE = 5, // SmApiCreateRectangle(center, length, width): 4 segments (lists only)
};

// Access to private / protected members through explicit instantiation.
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

// An input curve's constructor failed: the case compares only this marker.
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

// The curves a case owns.
struct Owned
{
    SmTArray<SmBSplineCurve*> curves;
    ~Owned()
    {
        for (ULONG i = 0; i < curves.GetSize(); i++)
            delete curves[i];
    }
};

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

// Reads one curve spec of a kind in [0, max_kind] and appends what it makes to `own`.
void read_curve(Rd& rd, int64_t max_kind, Owned& own)
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
            own.curves.Add(c);
        break;
    }
    case K_CIRCLE:
    {
        SmPoint3d c0 = rd.vec();
        double r = rd.num();
        SmBSplineCurve* c = NULL;
        s = SmApiCreateCircle(c0, r, c);
        if (c)
            own.curves.Add(c);
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
            own.curves.Add(c);
        break;
    }
    case K_SEGMENT:
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine* l = NULL;
        s = SmApiCreateLineSegment(a, b, l);
        if (l)
            own.curves.Add(l);
        break;
    }
    case K_ANALYTIC:
    {
        SmPoint3d c0 = rd.vec();
        double r = rd.num();
        SmCircle* c = NULL;
        s = SmApiCreateCircle(c0, r, c);
        if (c)
            own.curves.Add(c);
        break;
    }
    default:
    {
        // K_RECTANGLE
        SmPoint3d c0 = rd.vec();
        double len = rd.num();
        double wid = rd.num();
        SmTArray<SmBSplineCurve*> segs;
        s = SmApiCreateRectangle(c0, len, wid, segs);
        for (ULONG i = 0; i < segs.GetSize(); i++)
            own.curves.Add(segs[i]);
        break;
    }
    }
    if (s != SM_SUCCESS)
        throw BuildFailed();
}

void read_list(Rd& rd, int64_t max_kind, Owned& own)
{
    int64_t count = rd.idx(0, 64);
    for (int64_t i = 0; i < count; i++)
        read_curve(rd, max_kind, own);
}

void run(Rd& rd, std::vector<double>& o)
{
    int64_t op = rd.idx(OP_INTERP, OP_ORDER);
    switch (op)
    {
    case OP_INTERP:
    {
        // n pts(3n) parameterization
        int64_t np = rd.idx(0, 4096);
        SmTArray<SmPoint3d> pts;
        for (int64_t i = 0; i < np; i++)
            pts.Add(rd.vec());
        int64_t par = rd.idx(SM_CP_UNIFORM, SM_CP_CENTRIPETAL);
        SmCurve* res = NULL;
        SmStatus s = SmApiCreateCurveInterpPoints(pts, res, (SmCurveParameterizationType)par);
        o.push_back(st(s));
        if (s == SM_SUCCESS && res)
            put_bspline(o, (SmBSplineCurve*)res);
        delete res;
        return;
    }
    case OP_OFFSET:
    {
        // curve distance
        Owned own;
        read_curve(rd, K_ANALYTIC, own);
        double d = rd.num();
        SmBSplineCurve* res = NULL;
        SmStatus s = SmApiOffsetCurve(own.curves[0], d, res);
        o.push_back(st(s));
        if (s == SM_SUCCESS && res)
            put_bspline(o, res);
        delete res;
        return;
    }
    case OP_CONTINUITY:
    {
        // curve relative parameter
        Owned own;
        read_curve(rd, K_ANALYTIC, own);
        bool relative = rd.idx(0, 1) != 0;
        double t = rd.num();
        if (relative)
            t = own.curves[0]->GetNaturalInterval().Evaluate(t);
        SmContinuityType ct = SM_CT_UNDEFINED;
        SmStatus s = SmApiEvaluateContinuity(own.curves[0], t, ct);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back((double)ct);
        return;
    }
    case OP_COMPATIBLE:
    {
        // count curves (plain B-spline kinds)
        Owned own;
        read_list(rd, K_ARC, own);
        SmStatus s = SmApiMakeCurvesCompatible(own.curves);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)own.curves.GetSize());
            for (ULONG i = 0; i < own.curves.GetSize(); i++)
                put_bspline(o, own.curves[i]);
        }
        return;
    }
    default:
    {
        // OP_ORDER: count curves
        Owned own;
        read_list(rd, K_RECTANGLE, own);
        SmTArray<SmBSplineCurve*> work;
        for (ULONG i = 0; i < own.curves.GetSize(); i++)
            work.Add(own.curves[i]);
        SmStatus s = SmApiOrderCurves(work);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)work.GetSize());
            for (ULONG i = 0; i < work.GetSize(); i++)
            {
                ULONG at = 0;
                if (!own.curves.FindElement(work[i], at))
                    throw Bad();
                o.push_back((double)at);
            }
        }
        return;
    }
    }
}
} // namespace

SC_EXPORT int32_t sc_primitive_api_curve_ops_run(const double* in, int64_t n_in, double* out,
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
