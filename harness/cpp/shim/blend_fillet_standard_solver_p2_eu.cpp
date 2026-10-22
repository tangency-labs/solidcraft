// SPDX-License-Identifier: Apache-2.0
// Difftest shim for unit `blend.fillet_standard_solver_p2`, edge-use part: the
// SmConstantDistanceFS constructor from an edge use (mode 0) and
// SmConstantRadiusFS::LoadInitialValues with edge uses (mode 1), on the B-reps of unit
// `topology.edgeuse_p1` (sc_edgeuse_graph.h). Kept apart from
// blend_fillet_standard_solver_p2.cpp because the graph header brings its own reader.
// Layout: crates/solidcore-ffi/src/units/blend_fillet_standard_solver_p2.rs (`eval_edgeuse`).
//
// One entry point, sc_blend_fillet_standard_solver_p2_eu_eval(path, in, n_in, out, cap, out_len).

#include "sc_shim.h"

#include "StdAfx.h"
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtentNd.h>
#include <SmFace.h>
#include <SmFilletGeom.h>
#include <SmFilletStandardSolver.h>
#include <SmLoopuse.h>
#include <SmOffsetSurface.h>
#include <SmSurface.h>
#include <SmTArray.h>

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
// dereference for a loop edgeuse (as blend_fillet_standard_solver_p1_eu.cpp's eu_safe).
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

struct CdPeek : SmConstantDistanceFS
{
    CdPeek(const SmContext& ctx, double tol, double ang, double tan, double d, SmEdgeuse* eu)
        : SmConstantDistanceFS(ctx, tol, ang, tan, d, eu)
    {
    }
    SmEdgeuse* edgeuse(int i) const { return m_pEdgeuses[i]; }
    SmOffsetSurface* surface(int i) const { return m_pSurfaces[i]; }
    double orientation(int i) const { return m_dOrientations[i]; }
};

// The constructor's default fillet geometry (`new SmFilletGeom(this)` in the SmFilletSolver
// constructor, SmFilletSolver.cpp:65): `count`, then for the first geometry `solver_is_fs`
// and per rail `own` (FinishConstruction installed a copy: GetOffsetSurface no longer falls
// back to the solver's) and, for its own copy, `offset base_is_face status x y z` at the UV
// fractions (u, v) of its natural domain. `faces[i]` is rail i's face surface (or NULL).
void put_default_geom(std::vector<double>& o, SmFilletSolver& fs, SmSurface* const faces[2], double fu,
                      double fv)
{
    SmTArray<SmFilletGeom*> geoms;
    fs.GetFilletGeoms(geoms);
    o.push_back((double)geoms.GetSize());
    if (geoms.GetSize() == 0)
        return;
    SmFilletGeom* fg = geoms[0];
    o.push_back(fg->GetFilletSolver() == &fs ? 1.0 : 0.0);
    for (ULONG i = 0; i < 2; i++)
    {
        SmOffsetSurface* s = fg->GetOffsetSurface(i);
        bool own = s != NULL && s != fs.GetSurface(i);
        o.push_back(own ? 1.0 : 0.0);
        if (!own)
            continue;
        o.push_back(s->GetOffsetDistance());
        o.push_back(faces[i] != NULL && s->GetBaseSurface() == faces[i] ? 1.0 : 0.0);
        SmExtent2d d = s->GetNaturalUVDomain();
        SmPoint2d uv(d.GetMin().x + fu * (d.GetMax().x - d.GetMin().x),
                     d.GetMin().y + fv * (d.GetMax().y - d.GetMin().y));
        SmPoint3d pt(0, 0, 0);
        SmStatus es = s->EvaluatePoint(uv, pt);
        o.push_back(st(es));
        put_vec(o, es == SM_SUCCESS ? pt : SmPoint3d(0, 0, 0));
    }
}

struct CrInit : SmConstantRadiusFS
{
    CrInit(const SmContext& ctx, double tol, double ang, double tan, double r, SmEdgeuse* eu)
        : SmConstantRadiusFS(ctx, tol, ang, tan, r, eu, NULL)
    {
    }
    SmEdgeuse* edgeuse(int i) const { return m_pEdgeuses[i]; }
    SmOffsetSurface* surface(int i) const { return m_pSurfaces[i]; }
    void set_extension_factor(double e) { m_dSurfaceExtensionFactor = e; }
};

// Per rail `offset umin vmin umax vmax has_extension` after the op.
void put_offsets(std::vector<double>& o, SmFilletSolver* fs)
{
    for (int i = 0; i < 2; i++)
    {
        SmOffsetSurface* s = fs->GetSurface(i);
        o.push_back(s->GetOffsetDistance());
        SmExtent2d d = s->GetNaturalUVDomain();
        o.push_back(d.GetMin().x);
        o.push_back(d.GetMin().y);
        o.push_back(d.GetMax().x);
        o.push_back(d.GetMax().y);
        o.push_back(s->GetExtendedBaseSurface() != NULL ? 1.0 : 0.0);
    }
}

// Mode 1, `eu tol ang tan r rail u v ext`: the SmConstantRadiusFS edge-use constructor,
// m_dSurfaceExtensionFactor = ext, then LoadInitialValues from *GetSurface(rail) at the
// UV fractions (u, v) of its natural domain.
void run_cr(Rd& rd, Graph& g, std::vector<double>& o)
{
    SmEdgeuse* p = g.edgeuses[(size_t)rd.idx()];
    double tol = rd.num();
    double ang = rd.num();
    double tan = rd.num();
    double r = rd.num();
    ULONG rail = (ULONG)rd.idx();
    double fu = rd.num();
    double fv = rd.num();
    double ext = rd.num();
    if (!eu_safe(p) || p->GetRadial() == nullptr || !eu_safe(p->GetRadial()))
    {
        o.push_back(ST_NULL_DEREF);
        return;
    }
    CrInit fs(the_context(), tol, ang, tan, r, p);
    // SM_ERR is only signalled: the tangent-surface exits leave the edge uses NULL
    if (fs.edgeuse(0) == nullptr)
    {
        o.push_back((double)SM_ERR);
        return;
    }
    fs.set_extension_factor(ext);
    o.push_back(0.0);
    SmOffsetSurface* s1 = fs.surface((int)rail);
    SmExtent2d d = s1->GetNaturalUVDomain();
    SmExtentNd ivls(4);
    ivls[0] = SmExtent1d(d.GetMin().x, d.GetMax().x);
    ivls[1] = SmExtent1d(d.GetMin().y, d.GetMax().y);
    SmBoolean alPer[16];
    SmTArray<SmBoolean> per(16, alPer);
    per.Add(FALSE);
    per.Add(FALSE);
    double adGuess[16];
    SmTArray<double> guess(16, adGuess);
    guess.Add(d.GetMin().x + fu * (d.GetMax().x - d.GetMin().x));
    guess.Add(d.GetMin().y + fv * (d.GetMax().y - d.GetMin().y));
    ULONG do2 = 1;
    SmSurface* ps2 = NULL;
    ULONG idx = 999;
    SmStatus ls = fs.LoadInitialValues(do2, rail, *s1, 0, ps2, idx, ivls, per, guess);
    o.push_back(st(ls));
    o.push_back(ps2 == NULL ? -1.0 : ps2 == fs.surface(0) ? 0.0 : ps2 == fs.surface(1) ? 1.0 : -1.0);
    o.push_back((double)idx);
    if (ls == SM_SUCCESS)
    {
        for (ULONG n = 2; n < 4; n++)
        {
            o.push_back(ivls[n].GetMin());
            o.push_back(ivls[n].GetMax());
        }
    }
    o.push_back((double)per.GetSize());
    for (ULONG n = 0; n < per.GetSize(); n++)
        o.push_back(per[n] ? 1.0 : 0.0);
    o.push_back((double)guess.GetSize());
    for (ULONG n = 0; n < guess.GetSize(); n++)
        o.push_back(guess[n]);
    put_offsets(o, &fs);
}

// Mode 0, `eu tol ang tan dist u v`: the constructor's arguments and a UV fraction of each
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
    if (rd.idx() == 1)
    {
        run_cr(rd, g, o);
        return;
    }
    SmEdgeuse* p = g.edgeuses[(size_t)rd.idx()];
    double tol = rd.num();
    double ang = rd.num();
    double tan = rd.num();
    double dist = rd.num();
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
    CdPeek fs(the_context(), tol, ang, tan, dist, p);
    o.push_back(0.0);
    // the tangent-surface exits (SM_ERR only signalled) leave no edge uses or surfaces
    bool built = fs.edgeuse(0) != nullptr;
    o.push_back(built ? 1.0 : 0.0);
    o.push_back(fs.orientation(0));
    o.push_back(fs.orientation(1));
    o.push_back(fs.GetDistance());
    if (built)
    {
        for (int i = 0; i < 2; i++)
        {
            o.push_back((double)g.find(6, fs.edgeuse(i)));
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
    SmSurface* const faces[2] = {p->GetFace()->GetSurface(), radial->GetFace()->GetSurface()};
    put_default_geom(o, fs, faces, fu, fv);
    put_eu_state(o, g, p);
    put_eu_state(o, g, radial);
}
} // namespace

SC_EXPORT int32_t sc_blend_fillet_standard_solver_p2_eu_eval(const char* path, const double* in, int64_t n_in,
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
