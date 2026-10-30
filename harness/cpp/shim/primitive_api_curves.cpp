// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `primitive.api_curves`: the SM_API curve constructors of
// source/SM_API/src/SmApiCurves.cpp (SmApiCreateLineSegment, SmApiCreateLine, both
// SmApiCreateCircle overloads, SmApiCreateEllipse, SmApiCreateArc, SmApiCreateHelix,
// SmApiCreateRectangle, SmApiCreateRegularPolygon, SmApiCreateCurve,
// SmApiCreateCanonicalCurve) and SmApiRemoveCurveKnots on a curve one of them made,
// called exactly as a caller of the API would.
//
// One entry point, sc_primitive_api_curves_run(in, n_in, out, cap, out_len). `in` is
// `op args...`; layouts are documented in crates/solidcore-ffi/src/units/primitive_api_curves.rs.
// Every read is bounds checked here (a bad program, or input that would make the C++ read out
// of bounds, is SC_ERR_ARG). `out` receives the API status (0 = SM_SUCCESS) and, on success,
// the state of the created curve(s). Private members are read through explicit-instantiation
// access.
#include "sc_shim.h"

#include <SmApiCurves.h>
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmCircle.h>
#include <SmCurveTypes.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmLine.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <stdexcept>
#include <vector>

#include "sc_sm.h"

using sc::flg;
using sc::put_ivl;
using sc::put_vec;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/primitive_api_curves.rs).
enum : int32_t
{
    OP_LINE_SEGMENT = 1,
    OP_LINE = 2,
    OP_CIRCLE = 3,
    OP_ANALYTIC_CIRCLE = 4,
    OP_ELLIPSE = 5,
    OP_ARC = 6,
    OP_HELIX = 7,
    OP_RECTANGLE = 8,
    OP_POLYGON = 9,
    OP_CURVE = 10,
    OP_CANONICAL = 11,
    OP_REMOVE_KNOTS = 12,
};

// Set by OP_REMOVE_KNOTS: the created curve is passed to SmApiRemoveCurveKnots, whose
// status precedes its state.
bool g_remove_knots = false;

// SmApiRemoveCurveKnots on a created curve, when asked: its status.
void maybe_remove_knots(std::vector<double>& o, SmCurve* c)
{
    if (g_remove_knots)
        o.push_back(st(SmApiRemoveCurveKnots(c)));
}

// Access to private / protected members through explicit instantiation.
template <typename Tag, typename Tag::type M>
struct Rob
{
    friend typename Tag::type get(Tag) { return M; }
};

struct PcKnots
{
    typedef SmTArray<double> SmPolarConversion::*type;
    friend type get(PcKnots);
};
template struct Rob<PcKnots, &SmPolarConversion::m_vNaturalKnots>;

struct PcAngles
{
    typedef SmTArray<double> SmPolarConversion::*type;
    friend type get(PcAngles);
};
template struct Rob<PcAngles, &SmPolarConversion::m_vAngles>;

struct PcExact
{
    typedef SmTArray<SmBoolean> SmPolarConversion::*type;
    friend type get(PcExact);
};
template struct Rob<PcExact, &SmPolarConversion::m_vExactConversion>;

struct EllPolar
{
    typedef SmPolarConversion SmEllipse::*type;
    friend type get(EllPolar);
};
template struct Rob<EllPolar, &SmEllipse::m_vPolarConverter>;

struct EllInsideOut
{
    typedef SmBoolean SmEllipse::*type;
    friend type get(EllInsideOut);
};
template struct Rob<EllInsideOut, &SmEllipse::m_bInsideOut>;

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

// point vector STEP-interval scale inside_out, then the B-spline part.
void put_line(std::vector<double>& o, SmLine* l)
{
    put_vec(o, l->GetLinePoint());
    put_vec(o, l->GetLineVector());
    put_ivl(o, l->GetSTEPInterval());
    o.push_back(l->GetLineScale());
    o.push_back(flg(l->GetInsideOut()));
    put_bspline(o, l);
}

// placement STEP-interval radii inside_out, the polar converter, then the B-spline part.
void put_ellipse(std::vector<double>& o, SmEllipse* e)
{
    const SmAxis2Placement& pos = e->GetPosition();
    put_vec(o, pos.GetOriginRef());
    put_vec(o, pos.GetXAxisRef());
    put_vec(o, pos.GetYAxisRef());
    put_ivl(o, e->GetSTEPInterval());
    o.push_back(e->GetXRadius());
    o.push_back(e->GetYRadius());
    o.push_back(flg(e->*get(EllInsideOut())));
    const SmPolarConversion& pc = e->*get(EllPolar());
    o.push_back(flg(pc.IsPolarConversionPossible()));
    const SmTArray<double>& k = pc.*get(PcKnots());
    o.push_back((double)k.GetSize());
    for (ULONG i = 0; i < k.GetSize(); i++)
        o.push_back(k[i]);
    const SmTArray<double>& a = pc.*get(PcAngles());
    o.push_back((double)a.GetSize());
    for (ULONG i = 0; i < a.GetSize(); i++)
        o.push_back(a[i]);
    const SmTArray<SmBoolean>& x = pc.*get(PcExact());
    o.push_back((double)x.GetSize());
    for (ULONG i = 0; i < x.GetSize(); i++)
        o.push_back(flg(x[i]));
    put_bspline(o, e);
}

void put_segments(std::vector<double>& o, SmStatus s, SmTArray<SmBSplineCurve*>& segs)
{
    o.push_back(st(s));
    if (s == SM_SUCCESS)
    {
        o.push_back((double)segs.GetSize());
        for (ULONG i = 0; i < segs.GetSize(); i++)
            put_line(o, (SmLine*)segs[i]);
    }
    for (ULONG i = 0; i < segs.GetSize(); i++)
        delete segs[i];
}

void put_result(std::vector<double>& o, SmStatus s, SmBSplineCurve* c)
{
    o.push_back(st(s));
    if (s == SM_SUCCESS && c)
    {
        maybe_remove_knots(o, c);
        put_bspline(o, c);
    }
    delete c;
}

void run(Rd& rd, std::vector<double>& o)
{
    int64_t op = rd.idx(OP_LINE_SEGMENT, OP_REMOVE_KNOTS);
    g_remove_knots = false;
    if (op == OP_REMOVE_KNOTS)
    {
        // a curve of one call (not the segment arrays), then SmApiRemoveCurveKnots
        g_remove_knots = true;
        op = rd.idx(OP_LINE_SEGMENT, OP_CANONICAL);
        if (op == OP_RECTANGLE || op == OP_POLYGON)
            throw Bad();
    }
    switch (op)
    {
    case OP_LINE_SEGMENT:
    case OP_LINE:
    {
        SmPoint3d a = rd.vec();
        SmVector3d b = rd.vec();
        SmLine* l = NULL;
        SmStatus s = op == OP_LINE ? SmApiCreateLine(a, b, l) : SmApiCreateLineSegment(a, b, l);
        o.push_back(st(s));
        if (s == SM_SUCCESS && l)
        {
            maybe_remove_knots(o, l);
            put_line(o, l);
        }
        delete l;
        return;
    }
    case OP_CIRCLE:
    {
        SmPoint3d c = rd.vec();
        double r = rd.num();
        SmBSplineCurve* res = NULL;
        SmStatus s = SmApiCreateCircle(c, r, res);
        put_result(o, s, res);
        return;
    }
    case OP_ANALYTIC_CIRCLE:
    case OP_ELLIPSE:
    {
        SmPoint3d c = rd.vec();
        double r1 = rd.num();
        double r2 = op == OP_ELLIPSE ? rd.num() : r1;
        SmEllipse* e = NULL;
        SmStatus s;
        if (op == OP_ELLIPSE)
            s = SmApiCreateEllipse(c, r1, r2, e);
        else
        {
            SmCircle* ci = NULL;
            s = SmApiCreateCircle(c, r1, ci);
            e = ci;
        }
        o.push_back(st(s));
        if (s == SM_SUCCESS && e)
        {
            maybe_remove_knots(o, e);
            o.push_back((double)e->GetType());
            put_ellipse(o, e);
        }
        delete e;
        return;
    }
    case OP_ARC:
    {
        SmPoint3d c = rd.vec();
        double r = rd.num();
        double a0 = rd.num();
        double a1 = rd.num();
        SmBSplineCurve* res = NULL;
        SmStatus s = SmApiCreateArc(c, r, a0, a1, res);
        put_result(o, s, res);
        return;
    }
    case OP_HELIX:
    {
        SmPoint3d c = rd.vec();
        double h = rd.num();
        double r0 = rd.num();
        double r1 = rd.num();
        double turns = rd.num();
        bool rh = rd.flag();
        double tol = rd.num();
        SmBSplineCurve* res = NULL;
        SmStatus s = SmApiCreateHelix(c, h, r0, r1, turns, rh ? TRUE : FALSE, tol, res);
        put_result(o, s, res);
        return;
    }
    case OP_RECTANGLE:
    {
        SmPoint3d c = rd.vec();
        double len = rd.num();
        double wid = rd.num();
        SmTArray<SmBSplineCurve*> segs;
        SmStatus s = SmApiCreateRectangle(c, len, wid, segs);
        put_segments(o, s, segs);
        return;
    }
    case OP_POLYGON:
    {
        SmPoint3d c = rd.vec();
        ULONG sides = (ULONG)rd.idx(0, 4096);
        double r = rd.num();
        SmTArray<SmBSplineCurve*> segs;
        SmStatus s = SmApiCreateRegularPolygon(c, sides, r, segs);
        put_segments(o, s, segs);
        return;
    }
    case OP_CURVE:
    {
        int64_t np = rd.idx(0, 4096);
        SmTArray<SmPoint3d> pts;
        for (int64_t i = 0; i < np; i++)
            pts.Add(rd.vec());
        SmBSplineCurve* res = NULL;
        SmStatus s = SmApiCreateCurve(pts, res);
        put_result(o, s, res);
        return;
    }
    default:
    {
        // OP_CANONICAL: np pts nk knots mults(nk) has_w nw w(nw) degree form
        int64_t np = rd.idx(0, 4096);
        SmTArray<SmPoint3d> pts;
        for (int64_t i = 0; i < np; i++)
            pts.Add(rd.vec());
        int64_t nk = rd.idx(0, 4096);
        SmTArray<double> knots;
        for (int64_t i = 0; i < nk; i++)
            knots.Add(rd.num());
        SmTArray<ULONG> mults;
        int64_t total = 0;
        for (int64_t i = 0; i < nk; i++)
        {
            int64_t m = rd.idx(0, 64);
            total += m;
            mults.Add((ULONG)m);
        }
        bool has_w = rd.flag();
        int64_t nw = rd.idx(0, 4096);
        SmTArray<double> w;
        for (int64_t i = 0; i < nw; i++)
            w.Add(rd.num());
        int64_t degree = rd.idx(0, 64);
        int64_t form = rd.idx(0, 6);
        // CreateCanonical reads knots [p] and [size-p-1] and N_Iges126Crv reads n+p+2 knots
        // and n+1 points / weights without checking: reject what would read out of bounds.
        if (degree >= 1 && degree <= 32 && (!has_w || nw == np))
        {
            if (np < 1 || total < degree + 1 || total < (np - 1) + degree + 2)
                throw Bad();
        }
        SmBSplineCurve* res = NULL;
        SmStatus s = SmApiCreateCanonicalCurve(pts, knots, mults, has_w ? &w : NULL, (ULONG)degree,
                                               (SmBSplineCurveForm)form, res);
        put_result(o, s, res);
        return;
    }
    }
}
} // namespace

SC_EXPORT int32_t sc_primitive_api_curves_run(const double* in, int64_t n_in, double* out,
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
    if (rd.at != rd.n)
        return SC_ERR_ARG;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
