// SPDX-License-Identifier: Apache-2.0
// Difftest shim for unit `blend.fillet_standard_solver_p1`, edge-use part: the
// SmConstantRadiusFS constructor from an edge use, on the B-reps of unit
// `topology.edgeuse_p1` (sc_edgeuse_graph.h). Kept apart from
// blend_fillet_standard_solver_p1.cpp because the graph header brings its own reader.
// Layout: crates/solidcore-ffi/src/units/blend_fillet_standard_solver_p1.rs (`eval_edgeuse`).
//
// One entry point, sc_blend_fillet_standard_solver_p1_eu_eval(path, in, n_in, out, cap, out_len).

#include "sc_shim.h"

#include "StdAfx.h"
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmFace.h>
#include <SmFilletStandardSolver.h>
#include <SmLoopuse.h>
#include <SmOffsetSurface.h>
#include <SmSurface.h>

#include <vector>

#include "sc_edgeuse_graph.h"

namespace
{
const double ST_NULL_DEREF = -2.0;

// GetMate/GetRadial need the edge's primary edgeuse (loop edgeuses).
bool has_prim_eu(SmEdgeuse* p)
{
    return p->GetEdgeuseType() == SmShell_TYPE || (p->GetEdge() && p->GetEdge()->GetPrimaryEdgeuse());
}

// Every pointer EvaluateBinormal (which may create the UV trim curve) and the constructor
// dereference for a loop edgeuse: as topology_edgeuse_p3.cpp's loop_safe.
bool eu_safe(SmEdgeuse* p)
{
    if (p == nullptr || !p->IsLoopEdgeuse() || !p->GetEdge() || !p->GetEdge()->GetCurve() || !p->GetFaceuse() ||
        !p->GetLoopuse() || !p->GetFace() || !p->GetFace()->GetSurface() || !p->GetVertexuse() || !has_prim_eu(p))
        return false;
    SmEdgeuse* prim = p->GetEdge()->GetPrimaryEdgeuse();
    SmEdgeuse* pm = prim->GetMate();
    if (pm == nullptr || prim->GetVertexuse() == nullptr || pm->GetVertexuse() == nullptr)
        return false;
    SmEdgeuse* mate = p->GetMate();
    if (mate && mate->GetVertexuse() == nullptr)
        return false;
    return p->GetEdge()->GetStartVertex() != nullptr && p->GetEdge()->GetEndVertex() != nullptr;
}

struct CrPeek : SmConstantRadiusFS
{
    CrPeek(const SmContext& ctx, double tol, double ang, double tan, double r, SmEdgeuse* eu, double* r2)
        : SmConstantRadiusFS(ctx, tol, ang, tan, r, eu, r2)
    {
    }
    SmEdgeuse* edgeuse(int i) const { return m_pEdgeuses[i]; }
    SmOffsetSurface* surface(int i) const { return m_pSurfaces[i]; }
    double orientation(int i) const { return m_dOrientations[i]; }
    double radius(int i) const { return m_dFilletRadii[i]; }
};

// `tol ang tan r has_r2 r2 u v`: the constructor's arguments and a UV fraction of each
// offset surface's natural domain where its point is compared.
void run(Rd& rd, const char* path, std::vector<double>& o)
{
    SmStatus bst;
    Built b;
    b.brep = build(rd, path, bst);
    if (b.brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(bst));
        return;
    }
    o.push_back(0.0);
    Graph g;
    walk(b.brep, g);
    SmEdgeuse* p = g.edgeuses[(size_t)rd.idx()];
    double tol = rd.num();
    double ang = rd.num();
    double tan = rd.num();
    double r = rd.num();
    bool has_r2 = rd.flag();
    double r2 = rd.num();
    double fu = rd.num();
    double fv = rd.num();
    // GetRadial, both EvaluateBinormal calls, GetLoopuse()->GetFaceuse() and
    // GetFace()->GetSurface() of the edge use and its radial
    if (!eu_safe(p) || p->GetRadial() == nullptr || !eu_safe(p->GetRadial()))
    {
        o.push_back(ST_NULL_DEREF);
        return;
    }
    SmEdgeuse* radial = p->GetRadial();
    CrPeek fs(the_context(), tol, ang, tan, r, p, has_r2 ? &r2 : NULL);
    // SM_ERR is only signalled: the tangent-surface exits leave the edge uses NULL
    if (fs.edgeuse(0) == nullptr)
    {
        o.push_back((double)SM_ERR);
    }
    else
    {
        o.push_back(0.0);
        for (int i = 0; i < 2; i++)
        {
            o.push_back((double)g.find(6, fs.edgeuse(i)));
            o.push_back(fs.radius(i));
            o.push_back(fs.orientation(i));
            SmOffsetSurface* s = fs.surface(i);
            o.push_back(s->GetOffsetDistance());
            o.push_back((double)g.find(3, fs.edgeuse(i)->GetFace()));
            o.push_back(s->GetBaseSurface() == fs.edgeuse(i)->GetFace()->GetSurface() ? 1.0 : 0.0);
            SmExtent2d d = s->GetNaturalUVDomain();
            SmPoint2d uv(d.GetMin().x + fu * (d.GetMax().x - d.GetMin().x),
                         d.GetMin().y + fv * (d.GetMax().y - d.GetMin().y));
            SmPoint3d pt(0, 0, 0);
            SmStatus es = s->EvaluatePoint(uv, pt);
            o.push_back(st(es));
            put_vec(o, es == SM_SUCCESS ? pt : SmPoint3d(0, 0, 0));
        }
    }
    put_eu_state(o, g, p);
    put_eu_state(o, g, radial);
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_standard_solver_p1_eu_eval(const char* path, const double* in, int64_t n_in,
                                                             double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    run(rd, path, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
