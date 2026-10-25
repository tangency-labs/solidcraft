// SPDX-License-Identifier: Apache-2.0
// difftest shim for `--step-corpus` (not a port unit): builds one SMLib B-rep from a body
// of the clean-room STEP importer (crates/step-import), passed as the flat f64 array of
// step_import::flat (layout documented there), with SM_API/SMLib calls only, and reports
// what the Rust side (crates/difftest/src/step.rs, `build_rust`) reports for the same body.
//
// Recipe (identical on both sides):
//   1. one SmBrep in the SM_API context;
//   2. per face, in body order: its surface from the STEP data (SmPlane/SmCylinder/SmCone/
//      SmSphere/SmTorus/SmSurfOfRevolution/SmSurfOfExtrusion::CreateCanonical,
//      SmBSplineSurface::CreateCanonical), the 3D curve of every oriented edge of every
//      edge loop (vertex loops are skipped) bounded to the edge (SmLine with its STEP
//      domain, SmCircle/SmEllipse::CreateCanonical over the edge's angles,
//      SmBSplineCurve::CreateCanonical trimmed when the edge is a sub-interval), with
//      SM_OT_SAME when the loop runs along the curve, then SmBrep::CreateFacesWithCurves;
//   3. SmApiStitchIntoSolid; counts; SmApiBrepIsManifoldSolid; SmApiBrepComputeArea and
//      SmApiBrepComputeVolume at `accuracy`;
//   4. SmApiHealBrep; counts, manifold test and volume again.
//
// sc_step_import_build(in, n_in, accuracy, out, cap, out_len). Output: stage blocks
// `id n value*n` in order (ids in crates/solidcore-ffi/src/units/step_import.rs):
//   1 faces          nfaces made failed (status made)*nfaces
//   2 stitch         status solid stitched max_vertex_gap max_edge_gap
//   3 counts         regions shells faces edges vertices
//   4 manifold       status flag
//   5 area           status value
//   6 volume         status value
//   7 heal           status
//   8 healed_counts  regions shells faces edges vertices
//   9 healed_manifold status flag
//  10 healed_volume  status value
// A status is 0 for SM_SUCCESS, else the SmStatus. The Rust binding validates the input
// (step_import::flat::validate); this reader also refuses to read past `n_in`.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmApiBrep.h>
#include <SmApiGeneral.h>
#include <SmApiHeal.h>
#include <SmApiQueries.h>
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmCircle.h>
#include <SmCone.h>
#include <SmContext.h>
#include <SmCylinder.h>
#include <SmEllipse.h>
#include <SmFace.h>
#include <SmLine.h>
#include <SmPlane.h>
#include <SmRegion.h>
#include <SmShell.h>
#include <SmSphere.h>
#include <SmSurfOfExtrusion.h>
#include <SmSurfOfRevolution.h>
#include <SmTorus.h>

#include <cmath>
#include <vector>

namespace
{
struct Rd
{
    const double* p;
    int64_t n;
    int64_t i = 0;
    bool ok = true;
    double num()
    {
        if (i >= n)
        {
            ok = false;
            return 0.0;
        }
        return p[i++];
    }
    int64_t idx()
    {
        double x = num();
        if (!(x >= 0.0 && x < 1.0e15))
        {
            ok = false;
            return 0;
        }
        return (int64_t)x;
    }
    SmPoint3d pt()
    {
        double x = num(), y = num(), z = num();
        return SmPoint3d(x, y, z);
    }
};

struct Frame
{
    SmPoint3d o;
    SmVector3d x, y, z;
};

Frame frame(Rd& r)
{
    Frame f;
    f.o = r.pt();
    f.x = r.pt();
    f.y = r.pt();
    f.z = r.pt();
    return f;
}

double st(SmStatus s)
{
    return s == SM_SUCCESS ? 0.0 : (double)s;
}

// A STEP curve block: its kind and parameters (not yet built).
struct CurveData
{
    int kind = -1;
    SmPoint3d o;
    SmVector3d d;
    double mag = 0.0;
    Frame f;
    double r1 = 0.0, r2 = 0.0;
    long degree = 0;
    SmTArray<SmPoint3d> pts;
    bool rational = false;
    SmTArray<double> weights;
    SmTArray<double> knots;
    SmTArray<ULONG> mults;
};

void read_curve(Rd& r, CurveData& c)
{
    c.kind = (int)r.idx();
    switch (c.kind)
    {
    case 0:
        c.o = r.pt();
        c.d = r.pt();
        c.mag = r.num();
        break;
    case 1:
        c.f = frame(r);
        c.r1 = r.num();
        break;
    case 2:
        c.f = frame(r);
        c.r1 = r.num();
        c.r2 = r.num();
        break;
    case 3:
    {
        c.degree = (long)r.idx();
        int64_t n = r.idx();
        c.rational = r.idx() != 0;
        int64_t nk = r.idx();
        if (!r.ok || n > r.n || nk > r.n)
        {
            r.ok = false;
            return;
        }
        for (int64_t j = 0; j < n && r.ok; ++j)
            c.pts.Add(r.pt());
        if (c.rational)
            for (int64_t j = 0; j < n && r.ok; ++j)
                c.weights.Add(r.num());
        for (int64_t j = 0; j < nk && r.ok; ++j)
            c.knots.Add(r.num());
        for (int64_t j = 0; j < nk && r.ok; ++j)
            c.mults.Add((ULONG)r.idx());
        break;
    }
    default:
        r.ok = false;
    }
}

// The curve bounded to [lo, hi] (bounded = edge use) or as given (a swept curve).
SmStatus make_curve(const SmContext& ctx, const CurveData& c, bool bounded, double lo, double hi, SmCurve*& out)
{
    out = NULL;
    SmExtent1d ivl(lo, hi);
    switch (c.kind)
    {
    case 0:
        if (bounded)
        {
            out = new (ctx) SmLine(c.o, c.d, ivl, c.mag, 3, &ctx);
            return SM_SUCCESS;
        }
        else
        {
            SmLine* l = NULL;
            SmStatus s = SmLine::CreateCanonical(ctx, c.o, c.d, l);
            out = l;
            return s;
        }
    case 1:
    {
        SmCircle* e = NULL;
        SmStatus s = SmCircle::CreateCanonical(ctx, SmAxis2Placement(c.f.o, c.f.x, c.f.y), c.r1, e,
                                               bounded ? &ivl : NULL);
        out = e;
        return s;
    }
    case 2:
    {
        SmEllipse* e = NULL;
        SmStatus s = SmEllipse::CreateCanonical(ctx, SmAxis2Placement(c.f.o, c.f.x, c.f.y), c.r1, c.r2, e,
                                                bounded ? &ivl : NULL);
        out = e;
        return s;
    }
    case 3:
    {
        // Trim only to a strict sub-interval of the knot range.
        double k0 = c.knots[0], k1 = c.knots[c.knots.GetSize() - 1];
        bool trim = bounded && !(lo == k0 && hi == k1);
        SmBSplineCurve* b = NULL;
        SmStatus s = SmBSplineCurve::CreateCanonical(ctx, 3, (ULONG)c.degree, c.pts, SM_CF_UNSPECIFIED, c.mults,
                                                     c.knots, SM_KT_UNSPECIFIED, c.rational ? &c.weights : NULL,
                                                     trim ? &ivl : NULL, b);
        out = b;
        return s;
    }
    default:
        return SM_ERR;
    }
}

SmStatus make_surface(const SmContext& ctx, Rd& r, SmSurface*& out)
{
    out = NULL;
    int kind = (int)r.idx();
    switch (kind)
    {
    case 0:
    {
        Frame f = frame(r);
        if (!r.ok)
            return SM_ERR;
        SmPlane* p = NULL;
        SmStatus s = SmPlane::CreateCanonical(ctx, SmAxis2Placement(f.o, f.x, f.y), p);
        out = p;
        return s;
    }
    case 1:
    {
        Frame f = frame(r);
        double rad = r.num();
        if (!r.ok)
            return SM_ERR;
        SmCylinder* p = NULL;
        SmStatus s = SmCylinder::CreateCanonical(ctx, SmAxis2Placement(f.o, f.x, f.y), rad, p);
        out = p;
        return s;
    }
    case 2:
    {
        Frame f = frame(r);
        double rad = r.num(), semi = r.num();
        if (!r.ok)
            return SM_ERR;
        SmCone* p = NULL;
        SmStatus s = SmCone::CreateCanonical(ctx, SmAxis2Placement(f.o, f.x, f.y), rad, semi, p);
        out = p;
        return s;
    }
    case 3:
    {
        Frame f = frame(r);
        double rad = r.num();
        if (!r.ok)
            return SM_ERR;
        SmSphere* p = NULL;
        SmStatus s = SmSphere::CreateCanonical(ctx, SmAxis2Placement(f.o, f.x, f.y), rad, p);
        out = p;
        return s;
    }
    case 4:
    {
        Frame f = frame(r);
        double maj = r.num(), min = r.num();
        if (!r.ok)
            return SM_ERR;
        SmTorus* p = NULL;
        SmStatus s = SmTorus::CreateCanonical(ctx, SmAxis2Placement(f.o, f.x, f.y), maj, min, p);
        out = p;
        return s;
    }
    case 5:
    case 6:
    {
        CurveData c;
        read_curve(r, c);
        // revolution: axis_origin[3] axis_dir[3]; extrusion: dir[3] magnitude
        SmPoint3d a = kind == 5 ? r.pt() : SmPoint3d(0.0, 0.0, 0.0);
        SmVector3d d = r.pt();
        double mag = kind == 6 ? r.num() : 0.0;
        if (!r.ok)
            return SM_ERR;
        SmCurve* swept = NULL;
        SmStatus s = make_curve(ctx, c, false, 0.0, 0.0, swept);
        if (s != SM_SUCCESS)
            return s;
        SmBSplineCurve* b = (SmBSplineCurve*)swept;
        if (kind == 5)
        {
            SmSurfOfRevolution* p = NULL;
            s = SmSurfOfRevolution::CreateCanonical(ctx, b, a, d, p);
            out = p;
        }
        else
        {
            SmSurfOfExtrusion* p = NULL;
            s = SmSurfOfExtrusion::CreateCanonical(ctx, b, SmVector3d(d.x * mag, d.y * mag, d.z * mag), p);
            out = p;
        }
        return s;
    }
    case 7:
    {
        long ud = (long)r.idx(), vd = (long)r.idx();
        int64_t nu = r.idx(), nv = r.idx();
        bool rational = r.idx() != 0;
        int64_t nuk = r.idx(), nvk = r.idx();
        if (!r.ok || nu > r.n || nv > r.n || nu * nv > r.n || nuk > r.n || nvk > r.n)
            return SM_ERR;
        SmTArray<SmPoint3d> pts;
        SmTArray<double> w, uk, vk;
        SmTArray<ULONG> um, vm;
        for (int64_t j = 0; j < nu * nv && r.ok; ++j)
            pts.Add(r.pt());
        if (rational)
            for (int64_t j = 0; j < nu * nv && r.ok; ++j)
                w.Add(r.num());
        for (int64_t j = 0; j < nuk && r.ok; ++j)
            uk.Add(r.num());
        for (int64_t j = 0; j < nuk && r.ok; ++j)
            um.Add((ULONG)r.idx());
        for (int64_t j = 0; j < nvk && r.ok; ++j)
            vk.Add(r.num());
        for (int64_t j = 0; j < nvk && r.ok; ++j)
            vm.Add((ULONG)r.idx());
        if (!r.ok)
            return SM_ERR;
        SmBSplineSurface* p = NULL;
        SmStatus s = SmBSplineSurface::CreateCanonical(ctx, (ULONG)ud, (ULONG)vd, pts, SM_SF_UNSPECIFIED, um, vm, uk,
                                                       vk, SM_KT_UNSPECIFIED, rational ? &w : NULL, NULL, p);
        out = p;
        return s;
    }
    default:
        r.ok = false;
        return SM_ERR;
    }
}

struct Edge
{
    double same_sense = 1.0;
    double t0 = 0.0, t1 = 0.0;
    CurveData c;
};

void put_stage(std::vector<double>& o, int id, const std::vector<double>& v)
{
    o.push_back((double)id);
    o.push_back((double)v.size());
    o.insert(o.end(), v.begin(), v.end());
}

void counts(SmBrep* b, std::vector<double>& v)
{
    SmTArray<SmRegion*> regions;
    SmTArray<SmShell*> shells;
    SmTArray<SmFace*> faces;
    SmTArray<SmEdge*> edges;
    SmTArray<SmVertex*> vertices;
    b->GetRegions(regions);
    b->GetShells(shells);
    b->GetFaces(faces);
    b->GetEdges(edges);
    b->GetVertices(vertices);
    v.push_back((double)regions.GetSize());
    v.push_back((double)shells.GetSize());
    v.push_back((double)faces.GetSize());
    v.push_back((double)edges.GetSize());
    v.push_back((double)vertices.GetSize());
}
} // namespace

SC_EXPORT int32_t sc_step_import_build(const double* in, int64_t n_in, double accuracy, double* out, int64_t cap,
                                       int64_t* out_len)
{
    if (in == nullptr || out_len == nullptr || n_in < 7 || !std::isfinite(accuracy))
        return SC_ERR_ARG;
    SC_TRY
    SmContext* ctx = SmApiGetOrCreateContext();
    Rd r{in, n_in};
    if (r.num() != 21.0 || r.num() != 1.0)
        return SC_ERR_ARG;
    r.idx(); // kind
    int64_t nv = r.idx(), ne = r.idx(), nf = r.idx();
    r.idx(); // shells
    if (!r.ok || nv > n_in || ne > n_in || nf > n_in)
        return SC_ERR_ARG;
    for (int64_t i = 0; i < nv; ++i)
        r.pt();
    std::vector<Edge> edges((size_t)ne);
    for (int64_t i = 0; i < ne && r.ok; ++i)
    {
        Edge& e = edges[(size_t)i];
        r.idx();
        r.idx();
        e.same_sense = r.num();
        e.t0 = r.num();
        e.t1 = r.num();
        read_curve(r, e.c);
    }
    if (!r.ok)
        return SC_ERR_ARG;

    SmBrep* brep = new (*ctx) SmBrep();
    std::vector<double> o;

    // 2. faces
    std::vector<double> fv;
    double made_total = 0.0, failed = 0.0;
    std::vector<double> per_face;
    for (int64_t fi = 0; fi < nf; ++fi)
    {
        r.num(); // same_sense (orientation is left to the stitch)
        SmSurface* srf = NULL;
        SmStatus s = make_surface(*ctx, r, srf);
        if (!r.ok)
        {
            delete brep;
            return SC_ERR_ARG;
        }
        int64_t nl = r.idx();
        SmTArray<ULONG> loops;
        SmTArray<SmCurve*> curves;
        SmTArray<SmOrientType> orients;
        for (int64_t li = 0; li < nl && r.ok; ++li)
        {
            r.num(); // outer
            bool orientation = r.num() != 0.0;
            int64_t tag = r.idx();
            if (tag == 1)
            {
                r.idx();
                continue;
            }
            int64_t n = r.idx();
            if (n > n_in)
            {
                r.ok = false;
                break;
            }
            std::vector<std::pair<int64_t, bool>> oes;
            for (int64_t j = 0; j < n && r.ok; ++j)
            {
                int64_t ei = r.idx();
                bool fwd = r.num() != 0.0;
                oes.push_back(std::make_pair(ei, fwd));
            }
            if (!orientation)
            {
                std::vector<std::pair<int64_t, bool>> rev;
                for (size_t j = oes.size(); j-- > 0;)
                    rev.push_back(std::make_pair(oes[j].first, !oes[j].second));
                oes.swap(rev);
            }
            loops.Add((ULONG)oes.size());
            for (size_t j = 0; j < oes.size(); ++j)
            {
                if (oes[j].first >= ne)
                {
                    r.ok = false;
                    break;
                }
                const Edge& e = edges[(size_t)oes[j].first];
                bool same = e.same_sense != 0.0;
                double lo = same ? e.t0 : e.t1, hi = same ? e.t1 : e.t0;
                SmCurve* c = NULL;
                if (s == SM_SUCCESS)
                    s = make_curve(*ctx, e.c, true, lo, hi, c);
                curves.Add(c);
                orients.Add(oes[j].second == same ? SM_OT_SAME : SM_OT_OPPOSITE);
            }
        }
        if (!r.ok)
        {
            delete brep;
            return SC_ERR_ARG;
        }
        double made = 0.0;
        if (s == SM_SUCCESS)
        {
            SmShell* shell = NULL;
            SmTArray<SmFace*> faces;
            s = brep->CreateFacesWithCurves(NULL, loops, &curves, orients, srf, shell, faces);
            made = (double)faces.GetSize();
        }
        else
        {
            for (ULONG j = 0; j < curves.GetSize(); ++j)
                delete curves[j];
            delete srf;
        }
        if (s != SM_SUCCESS)
            failed += 1.0;
        made_total += made;
        per_face.push_back(st(s));
        per_face.push_back(made);
    }
    fv.push_back((double)nf);
    fv.push_back(made_total);
    fv.push_back(failed);
    fv.insert(fv.end(), per_face.begin(), per_face.end());
    put_stage(o, 1, fv);

    // 3. stitch, counts, manifold, area, volume
    {
        SmBoolean solid = FALSE;
        ULONG n = 0;
        double mv = 0.0, me = 0.0;
        SmStatus s = SmApiStitchIntoSolid(brep, solid, n, mv, me);
        put_stage(o, 2, {st(s), solid ? 1.0 : 0.0, (double)n, mv, me});
    }
    {
        std::vector<double> v;
        counts(brep, v);
        put_stage(o, 3, v);
    }
    {
        SmBoolean m = FALSE;
        SmStatus s = SmApiBrepIsManifoldSolid(brep, m);
        put_stage(o, 4, {st(s), m ? 1.0 : 0.0});
    }
    {
        double a = 0.0;
        SmStatus s = SmApiBrepComputeArea(brep, accuracy, a);
        put_stage(o, 5, {st(s), s == SM_SUCCESS ? a : 0.0});
    }
    {
        double vol = 0.0;
        SmStatus s = SmApiBrepComputeVolume(brep, accuracy, vol);
        put_stage(o, 6, {st(s), s == SM_SUCCESS ? vol : 0.0});
    }
    // 4. heal and measure again
    {
        SmStatus s = SmApiHealBrep(brep);
        put_stage(o, 7, {st(s)});
    }
    {
        std::vector<double> v;
        counts(brep, v);
        put_stage(o, 8, v);
    }
    {
        SmBoolean m = FALSE;
        SmStatus s = SmApiBrepIsManifoldSolid(brep, m);
        put_stage(o, 9, {st(s), m ? 1.0 : 0.0});
    }
    {
        double vol = 0.0;
        SmStatus s = SmApiBrepComputeVolume(brep, accuracy, vol);
        put_stage(o, 10, {st(s), s == SM_SUCCESS ? vol : 0.0});
    }
    delete brep;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
