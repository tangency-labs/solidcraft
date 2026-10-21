// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_solver_p4`: SmFilletSolver::CalcFilletGeom
// (source/SMLib/src/SmFilletSolver.cpp:1495-2673) on its early-exit and status paths: a
// rectangle sheet (SmPrimitiveCreation::CreateRectangle), a solver derived from
// SmConstantRadiusFS (surface-only constructor over the face's surface for both rails) whose
// only override is SetExtensionFlags (the base dereferences the executive, unset here; the
// override sets the given flags), its edge uses on one edge of the sheet, its G1 knots,
// optionally an SmHermiteCurve in place of the edge's curve, optionally one bare
// SmFilletGeom (no rails) with its split flag. Without rails, CalcFilletGeom stops at the
// first rail (`NER(pThisRail)`) at the latest; without a fillet geometry it is only run on a
// non-B-spline edge curve (it returns before reading m_vFilletGeoms[0]).
//
// Entry points (layouts in crates/solidcore-ffi/src/units/blend_fillet_solver_p4.rs, which
// validates every input; reads are bounds checked here as well):
//  * sc_blend_fillet_solver_p4_calc_target(in, n_in, out, cap, out_len): the rectangle
//    (`w h tol uv_curves`, see p4_rectangle), exported in traversal order
//    (sc_edgeuse_graph.h, unit topology.edgeuse_p1).
//  * sc_blend_fillet_solver_p4_calc_geom(in, n_in, out, cap, out_len).
#include "sc_shim.h"
#include "sc_edgeuse_graph.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmFilletGeom.h>
#include <SmFilletSolver.h>
#include <SmFilletStandardSolver.h>
#include <SmHermiteCurve.h>
#include <SmPrimitiveCreation.h>

#include <memory>
#include <vector>

namespace
{
struct CalcBad
{
};

struct CRd
{
    const double* in;
    int64_t n;
    int64_t at;
    double num()
    {
        if (at >= n)
            throw CalcBad();
        return in[at++];
    }
    int64_t idx(int64_t lo, int64_t hi)
    {
        double d = num();
        if (!(d >= (double)lo && d <= (double)hi) || d != (double)(int64_t)d)
            throw CalcBad();
        return (int64_t)d;
    }
    bool flag() { return idx(0, 1) != 0; }
};

// `w h tol uv_curves`: the rectangle; with `uv_curves` its face's UV trim curves are made
// first (SmFace::CreateUVTrimCurves without tolerance adjustment, status ignored), so
// CalcFilletGeom finds them instead of creating them (SmSurface::DropCurve).
SmBrep* p4_rectangle(const SmContext& ctx, CRd& rd)
{
    double w = rd.num();
    double h = rd.num();
    double tol = rd.num();
    bool uv_curves = rd.flag();
    SmBrep* brep = SmPrimitiveCreation::CreateRectangle(ctx, tol, w, h, SmAxis2Placement());
    if (brep != NULL && uv_curves)
    {
        Graph g;
        walk(brep, g);
        for (SmFace* f : g.faces)
        {
            double m1, m2, m3, m4, m5, m6;
            f->CreateUVTrimCurves(FALSE, NULL, NULL, m1, m2, m3, m4, m5, m6); // status ignored
        }
    }
    return brep;
}

// The solver: SmConstantRadiusFS with SetExtensionFlags replaced by the given flags.
struct CalcFS : SmConstantRadiusFS
{
    SmBoolean m_bBefore, m_bAfter;
    CalcFS(const SmContext& ctx, double tol, double r0, double r1, const SmSurface& s,
           SmBoolean o0, SmBoolean o1, SmBoolean before, SmBoolean after)
        : SmConstantRadiusFS(ctx, tol, 0.1, 0.03, r0, r1, s, s, o0, o1), m_bBefore(before),
          m_bAfter(after)
    {
    }
    virtual void SetExtensionFlags()
    {
        m_bExtendBefore = m_bBefore;
        m_bExtendAfter = m_bAfter;
    }
    void set_edgeuses(SmEdgeuse* a, SmEdgeuse* b)
    {
        m_pEdgeuses[0] = a;
        m_pEdgeuses[1] = b;
    }
    void add_geom(SmBoolean split)
    {
        SmFilletGeom* g = new SmFilletGeom(this, SM_FG_DEFAULT);
        g->SetSplitFlag(split);
        m_vFilletGeoms.Add(g);
    }
    SmTArray<double>& g1() { return m_vG1Knots; }
    SmBoolean before() const { return m_bExtendBefore; }
    SmBoolean after() const { return m_bExtendAfter; }
    SmFilStatus status() { return GetStatus(); }
};
} // namespace

SC_EXPORT int32_t sc_blend_fillet_solver_p4_calc_target(const double* in, int64_t n_in,
                                                        double* out, int64_t cap,
                                                        int64_t* out_len)
{
    if (in == NULL || n_in != 4 || out_len == NULL || cap < 0 || (cap > 0 && out == NULL))
        return SC_ERR_ARG;
    try
    {
        const SmContext& ctx = the_context();
        CRd rd{in, n_in, 0};
        std::unique_ptr<SmBrep> target(p4_rectangle(ctx, rd));
        if (!target)
            return SC_ERR_ARG;
        Graph graph;
        walk(target.get(), graph);
        std::vector<double> o = {0.0};
        export_graph(target.get(), graph, o);
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    catch (const CalcBad&)
    {
        return SC_ERR_ARG;
    }
    catch (...)
    {
        return SC_ERR_EXCEPTION;
    }
}

// Program: `w h tol uv_curves edge eu1_same hermite geom split extend_before extend_after n_g1
// g1_fractions[n_g1] r0 r1 orient0 orient1`. Output: `status solver_status extend_before
// extend_after n_g1 g1[n_g1] knots_in[n_in]`.
SC_EXPORT int32_t sc_blend_fillet_solver_p4_calc_geom(const double* in, int64_t n_in,
                                                      double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 4 || out_len == NULL || cap < 0 || (cap > 0 && out == NULL))
        return SC_ERR_ARG;
    try
    {
        const SmContext& ctx = the_context();
        CRd rd{in, n_in, 0};
        std::unique_ptr<SmBrep> target(p4_rectangle(ctx, rd));
        if (!target)
            return SC_ERR_ARG;
        int64_t ei = rd.idx(0, 3);
        bool eu1_same = rd.flag();
        bool hermite = rd.flag();
        bool geom = rd.flag();
        SmBoolean split = rd.flag() ? TRUE : FALSE;
        SmBoolean before = rd.flag() ? TRUE : FALSE;
        SmBoolean after = rd.flag() ? TRUE : FALSE;
        int64_t ng = rd.idx(0, 8);
        std::vector<double> fr;
        for (int64_t i = 0; i < ng; i++)
        {
            double f = rd.num();
            if (!(f > 0.0 && f < 1.0))
                throw CalcBad();
            fr.push_back(f);
        }
        double r0 = rd.num();
        double r1 = rd.num();
        SmBoolean o0 = rd.flag() ? TRUE : FALSE;
        SmBoolean o1 = rd.flag() ? TRUE : FALSE;
        if (rd.at != rd.n)
            throw CalcBad();
        // a B-spline edge curve reaches m_vFilletGeoms[0]
        if (!hermite && !geom)
            throw CalcBad();

        Graph graph;
        walk(target.get(), graph);
        if (graph.edges.size() != 4 || graph.faces.size() != 1 || (size_t)ei >= graph.edges.size())
            return SC_ERR_ARG;
        SmEdge* edge = graph.edges[(size_t)ei];
        std::vector<SmTopology*> eus = list_of(edge->GetList());
        if (eus.empty())
            return SC_ERR_ARG;
        SmEdgeuse* eu0 = (SmEdgeuse*)eus[0];
        SmEdgeuse* eu1 = (!eu1_same && eus.size() > 1) ? (SmEdgeuse*)eus[1] : eu0;
        SmSurface* srf = graph.faces[0]->GetSurface();
        if (srf == NULL)
            return SC_ERR_ARG;

        SmExtent1d ivl = edge->GetInterval();
        std::vector<double> knots_in;
        for (double f : fr)
            knots_in.push_back(ivl.Evaluate(f));
        if (hermite)
        {
            SmPoint3d a = edge->GetStartVertex()->GetPoint();
            SmPoint3d b = edge->GetEndVertex()->GetPoint();
            SmVector3d t = b - a;
            SmHermiteCurve* c = new (ctx) SmHermiteCurve(a, t, b, t, 3);
            edge->SetCurve(c, TRUE, FALSE, FALSE);
            c->SetOwner(edge);
        }

        std::vector<double> o;
        {
            CalcFS fs(ctx, 1e-5, r0, r1, *srf, o0, o1, before, after);
            fs.set_edgeuses(eu0, eu1);
            for (double k : knots_in)
                fs.g1().Add(k);
            if (geom)
                fs.add_geom(split);
            SmStatus stt = fs.CalcFilletGeom();
            o.push_back(st(stt));
            o.push_back((double)fs.status());
            o.push_back(flg(fs.before()));
            o.push_back(flg(fs.after()));
            o.push_back((double)fs.g1().GetSize());
            for (ULONG i = 0; i < fs.g1().GetSize(); i++)
                o.push_back(fs.g1()[i]);
            for (double k : knots_in)
                o.push_back(k);
        }
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    catch (const CalcBad&)
    {
        return SC_ERR_ARG;
    }
    catch (...)
    {
        return SC_ERR_EXCEPTION;
    }
}
