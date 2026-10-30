// SPDX-License-Identifier: Apache-2.0
// topology.brep_planar_section: SmBrep::CreatePlanarSectionCurves (source/SMLib/src/SmBrep.cpp)
// and SmApiIntersectBrepWithPlane (source/SM_API/src/SmApiIntersectors.cpp) on a body the
// shim builds itself: an upstream .smb file read by SmBrep::ReadFromFile (ASCII, no UV-trim
// rebuild), or SmApiCreateBox, optionally filleted by SmApiCircularFillet. The Rust side
// builds the same body with its own reader / primitives, so no C++ state reaches it. Layout
// in crates/solidcore-ffi/src/units/topology_brep_planar_section.rs.
#include "sc_shim.h"
#include "StdAfx.h"
#include <SmBrep.h>
#include <SmCurve.h>
#include <SmFace.h>
#include <SmBSplineCurve.h>
#include <nurbs.h>
#include <SmEdge.h>
#include <SmLoop.h>
#include <SmVertex.h>
#include <SmSurface.h>
#include <SmContext.h>
#include <SmApiFillets.h>
#include <SmApiGeneral.h>
#include <SmApiIntersectors.h>
#include <SmApiPrimitives.h>
#include <SmAxis2Placement.h>
#include <SmExtent3d.h>
#include <memory>
#include <vector>

namespace {
constexpr int64_t NARGS = 28;

// The body: kind 0 = `path` read by ReadFromFile, 1 = SmApiCreateBox(0, size a[1..3]) then,
// when a[4] > 0, SmApiCircularFillet(a[4]); 2 = that box with a strut in its top face (the
// face whose vertices all lie at z = a[3]): MakeVertexLoop at a[25..27], then
// MakeEdgeInFace from the face's first vertex (GetVertices) to it along
// SmBSplineCurve::CreateLineSegment, with the B-rep's editing enabled for the edit. The
// strut's edge has four edge uses on the face (IsEmbedded). Null on a failed build.
SmBrep* build_body(const char* path, const double* a, SmStatus& status) {
    SmContext& ctx = *SmApiGetOrCreateContext();
    if (a[0] == 0.0) {
        if (!path) { status = SM_ERR; return nullptr; }
        std::unique_ptr<SmBrep> brep(new (ctx) SmBrep());
        status = brep->ReadFromFile(ctx, path, SM_ASCII, FALSE);
        return status == SM_SUCCESS ? brep.release() : nullptr;
    }
    SmBrep* box = nullptr;
    status = SmApiCreateBox(SmVector3d(0, 0, 0), a[1], a[2], a[3], box);
    if (status != SM_SUCCESS || !box) { delete box; return nullptr; }
    if (a[4] > 0.0) {
        status = SmApiCircularFillet(box, a[4]);
        if (status != SM_SUCCESS) { delete box; return nullptr; }
    }
    if (a[0] == 2.0) {
        std::unique_ptr<SmBrep> owner(box);
        SmTArray<SmFace*> faces;
        box->GetFaces(faces);
        SmFace* top = nullptr;
        for (ULONG i = 0; i < faces.GetSize() && !top; ++i) {
            SmTArray<SmVertex*> vs;
            faces[i]->GetVertices(vs);
            bool all = vs.GetSize() > 0;
            for (ULONG j = 0; j < vs.GetSize(); ++j)
                if (vs[j]->GetPoint().z != a[3]) all = false;
            if (all) top = faces[i];
        }
        if (!top) { status = SM_ERR; return nullptr; }
        SmTArray<SmVertex*> vs;
        top->GetVertices(vs);
        SmVertex* start = vs[0];
        SmBoolean saved = box->m_bEditingEnabled;
        box->m_bEditingEnabled = TRUE;
        SmPoint3d point(a[25], a[26], a[27]);
        SmLoop* loop = nullptr;
        SmVertex* vertex = nullptr;
        status = box->MakeVertexLoop(top, point, loop, vertex);
        SmBSplineCurve* curve = nullptr;
        if (status == SM_SUCCESS)
            status = SmBSplineCurve::CreateLineSegment(ctx, 3, start->GetPoint(), point, curve);
        SmEdge* edge = nullptr;
        SmFace* split = nullptr;
        if (status == SM_SUCCESS && curve && vertex)
            status = box->MakeEdgeInFace(top, start, vertex, curve, nullptr,
                                         curve->GetNaturalInterval(), SM_OT_SAME, 0.0, edge,
                                         loop, split);
        else if (status == SM_SUCCESS)
            status = SM_ERR;
        box->m_bEditingEnabled = saved;
        if (status != SM_SUCCESS || !edge) {
            if (curve && !curve->GetOwner()) delete curve;
            if (status == SM_SUCCESS) status = SM_ERR;
            return nullptr;
        }
        return owner.release();
    }
    return box;
}

// `type lo hi` and five evaluated points (`status x y z` each) of `c`; `-1` for null.
void put_curve(std::vector<double>& o, SmCurve* c) {
    o.push_back(c ? (double)c->GetType() : -1.0);
    if (!c) return;
    SmExtent1d d = c->GetNaturalInterval();
    o.push_back(d.GetMin());
    o.push_back(d.GetMax());
    for (int i = 0; i < 5; ++i) {
        SmPoint3d p;
        SmStatus s = c->EvaluatePoint(d.Evaluate((double)i / 4), p);
        o.push_back(s == SM_SUCCESS ? 0.0 : 1.0);
        if (s == SM_SUCCESS) { o.push_back(p.x); o.push_back(p.y); o.push_back(p.z); }
    }
}

// A status as the Rust side records it: SM_SUCCESS is 0, an error its SmStatus code.
double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }

// The NURB of a B-spline curve: `n (x y z w)* nknots knot* mult*` (unique knots).
void put_nurb(std::vector<double>& o, SmBSplineCurve* c) {
    gw_CURVE* nurb = c ? c->GetGwNurbPointer() : nullptr;
    if (!nurb || !nurb->pol) { o.push_back(-1.0); return; }
    o.push_back((double)(nurb->pol->n + 1));
    for (long i = 0; i <= (long)nurb->pol->n; ++i) {
        o.push_back(nurb->pol->Pw[i].x); o.push_back(nurb->pol->Pw[i].y);
        o.push_back(nurb->pol->Pw[i].z); o.push_back(nurb->pol->Pw[i].w);
    }
    SmTArray<double> knots;
    SmTArray<ULONG> mults;
    c->GetKnots(knots, &mults);
    o.push_back((double)knots.GetSize());
    for (ULONG i = 0; i < knots.GetSize(); ++i) o.push_back(knots[i]);
    for (ULONG i = 0; i < mults.GetSize(); ++i) o.push_back((double)mults[i]);
}

void put_curves(std::vector<double>& o, SmTArray<SmCurve*>& curves) {
    o.push_back((double)curves.GetSize());
    for (ULONG i = 0; i < curves.GetSize(); ++i) put_curve(o, curves[i]);
}
}

// Args (25): source kind, box size (3), fillet radius, plane point (3), plane normal (3),
// has_tol, tol, has_ang, ang, want_3d, want_uv, want_faces, force_uv_classify, has_translate,
// translation (3), walk_n, walk_i. With has_translate the body is first moved by
// `Transform` of an SmAxis2Placement translated by the translation; with walk_n > 1 the
// plane point is `CalculateBoundingBox(box).Evaluate(t, t, t)`, t = walk_i / (walk_n - 1)
// (prog_test my_test_tsurf_section). Then strut point (3) for source kind 2.
// Op 4: on every face, the forced-UV continuation's callees for each section curve
// without UV curve -> `0 nfaces (status nforced (curve_nurb drop_status ndrops (drop
// drop_nurb lift_status maxdist lifted)*)*)*`.
// Op 3: SmSurface::CreatePlanarSectionCurves of every face's surface over the face's UV
// domain (GetFaces order; tolerances from the args) -> `0 nfaces (status n3d curve* nuv
// curve*)*`, a null UV curve being `-1`.
// Op 1: CreatePlanarSectionCurves -> `status n3d curve* nuv curve* nfaces face_index*`
// (a count of -1 for an output not asked for; face indices into GetFaces). Op 2:
// SmApiIntersectBrepWithPlane(point, normal) -> `status n curve*`. A failed body build
// returns `-999 status`.
SC_EXPORT int32_t sc_topology_brep_planar_section_eval(int32_t op, const char* path,
        const double* a, int64_t na, double* out, int64_t cap, int64_t* out_len) {
    if (op < 1 || op > 4 || !a || na != NARGS || !out_len) return SC_ERR_ARG;
    SC_TRY
    SmStatus status = SM_SUCCESS;
    std::unique_ptr<SmBrep> body(build_body(path, a, status));
    if (!body) {
        std::vector<double> o = {-999.0, (double)status};
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    if (a[19] != 0.0) {
        SmAxis2Placement move;
        move.Translate(SmVector3d(a[20], a[21], a[22]));
        status = body->Transform(move);
        if (status != SM_SUCCESS) {
            std::vector<double> o = {-999.0, (double)status};
            return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
        }
    }
    SmPoint3d point(a[5], a[6], a[7]);
    if (a[23] > 1.0) {
        SmExtent3d box;
        status = body->CalculateBoundingBox(box);
        if (status != SM_SUCCESS) {
            std::vector<double> o = {-999.0, (double)status};
            return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
        }
        double t = (a[24] * 1.0) / (a[23] - 1.0);
        point = box.Evaluate(t, t, t);
    }
    SmVector3d normal(a[8], a[9], a[10]);
    std::vector<double> o;
    if (op == 2) {
        SmTArray<SmCurve*> curves;
        status = SmApiIntersectBrepWithPlane(body.get(), point, normal, curves);
        o.push_back(st(status));
        put_curves(o, curves);
        for (ULONG i = 0; i < curves.GetSize(); ++i) delete curves[i];
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    const SmContext& ctx = *SmApiGetOrCreateContext();
    SmApproxTol3d tol = a[12];
    double ang = a[14];
    if (op == 4) {
        // the forced-UV continuation's callees on every face: for each non-degenerate
        // section curve without UV curve, DropAndTrimCurve(face UV domain, curve, its
        // interval, face tol / 3, TRUE, TRUE) and LiftCurve of every drop (tol / 3, TRUE)
        SmTArray<SmFace*> faces;
        body->GetFaces(faces);
        o.push_back(0.0);
        o.push_back((double)faces.GetSize());
        for (ULONG i = 0; i < faces.GetSize(); ++i) {
            SmFace* face = faces[i];
            SmSurface* surface = face->GetSurface();
            double ftol = (double)face->GetTolerance();
            SmTArray<SmCurve*> f3, fuv;
            SmStatus fs = surface->CreatePlanarSectionCurves(ctx, face->GetUVDomain(), point,
                normal, a[11] != 0.0 ? &tol : nullptr, a[13] != 0.0 ? &ang : nullptr, &f3, &fuv);
            o.push_back(st(fs));
            std::vector<double> forced;
            ULONG nforced = 0;
            for (ULONG j = 0; fs == SM_SUCCESS && j < f3.GetSize(); ++j) {
                SmCurve* c = f3[j];
                if ((j < fuv.GetSize() && fuv[j]) || c->IsDegenerate()) continue;
                ++nforced;
                put_nurb(forced, dynamic_cast<SmBSplineCurve*>(c));
                double maxdrop = 0.0, dev = 0.0;
                SmTArray<SmBSplineCurve*> drops;
                SmStatus ds = surface->DropAndTrimCurve(ctx, face->GetUVDomain(), *c,
                    c->GetNaturalInterval(), ftol / 3.0, maxdrop, dev, drops, TRUE, TRUE);
                forced.push_back(st(ds));
                forced.push_back((double)drops.GetSize());
                for (ULONG k = 0; k < drops.GetSize(); ++k) {
                    put_curve(forced, drops[k]);
                    put_nurb(forced, drops[k]);
                    double maxdist = 0.0;
                    SmBSplineCurve* lifted = nullptr;
                    SmStatus ls = surface->LiftCurve(ctx, face->GetUVDomain(), *drops[k],
                        drops[k]->GetNaturalInterval(), ftol / 3.0, maxdist, lifted, TRUE);
                    forced.push_back(st(ls));
                    forced.push_back(maxdist);
                    put_curve(forced, lifted);
                    delete lifted;
                    delete drops[k];
                }
            }
            o.push_back((double)nforced);
            o.insert(o.end(), forced.begin(), forced.end());
            for (ULONG j = 0; j < f3.GetSize(); ++j) delete f3[j];
            for (ULONG j = 0; j < fuv.GetSize(); ++j) delete fuv[j];
        }
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    if (op == 3) {
        SmTArray<SmFace*> faces;
        body->GetFaces(faces);
        o.push_back(0.0);
        o.push_back((double)faces.GetSize());
        for (ULONG i = 0; i < faces.GetSize(); ++i) {
            SmTArray<SmCurve*> f3, fuv;
            SmStatus fs = faces[i]->GetSurface()->CreatePlanarSectionCurves(ctx,
                faces[i]->GetUVDomain(), point, normal, a[11] != 0.0 ? &tol : nullptr,
                a[13] != 0.0 ? &ang : nullptr, &f3, &fuv);
            o.push_back(st(fs));
            put_curves(o, f3);
            put_curves(o, fuv);
            for (ULONG j = 0; j < f3.GetSize(); ++j) delete f3[j];
            for (ULONG j = 0; j < fuv.GetSize(); ++j) delete fuv[j];
        }
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    SmTArray<SmCurve*> c3, cuv;
    SmTArray<SmFace*> faces;
    status = body->CreatePlanarSectionCurves(ctx, point, normal,
        a[11] != 0.0 ? &tol : nullptr, a[13] != 0.0 ? &ang : nullptr,
        a[15] != 0.0 ? &c3 : nullptr, a[16] != 0.0 ? &cuv : nullptr,
        a[17] != 0.0 ? &faces : nullptr, a[18] != 0.0 ? TRUE : FALSE);
    o.push_back(st(status));
    if (a[15] != 0.0) put_curves(o, c3); else o.push_back(-1.0);
    if (a[16] != 0.0) put_curves(o, cuv); else o.push_back(-1.0);
    if (a[17] != 0.0) {
        SmTArray<SmFace*> all;
        body->GetFaces(all);
        o.push_back((double)faces.GetSize());
        for (ULONG i = 0; i < faces.GetSize(); ++i) {
            double at = -1.0;
            for (ULONG j = 0; j < all.GetSize(); ++j)
                if (all[j] == faces[i]) { at = (double)j; break; }
            o.push_back(at);
        }
    } else {
        o.push_back(-1.0);
    }
    for (ULONG i = 0; i < c3.GetSize(); ++i) delete c3[i];
    for (ULONG i = 0; i < cuv.GetSize(); ++i) delete cuv[i];
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
