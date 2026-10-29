// SPDX-License-Identifier: Apache-2.0
// topology.api_projection: the SM_API projection wrappers SmApiProjectBrepOntoPlane,
// SmApiCreateSilhouetteCurves, SmApiProjectCurve(SmBrep*, ..) (source/SM_API/src/
// SmApiBrep.cpp) and SmApiProjectCurve(const SmBSplineCurve*, const SmSurface*, ..)
// (source/SM_API/src/SmApiCurves.cpp), called as the library exports them. The inputs
// are the shim's own: an upstream file read by SmApiReadBrepFromFile or SmApiCreateBox
// optionally filleted by SmApiCircularFillet and moved, a curve by SmApiCreateCurve and
// a bilinear surface by SmBSplineSurface::CreateBilinearSurface; the Rust side builds the
// same ones itself. Layout in crates/solidcore-ffi/src/units/topology_api_projection.rs.
#include "sc_shim.h"
#include "StdAfx.h"
#include <SmApiBrep.h>
#include <SmApiCurves.h>
#include <SmApiFillets.h>
#include <SmApiGeneral.h>
#include <SmApiImportExport.h>
#include <SmApiPrimitives.h>
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmCurve.h>
#include <SmExtent1d.h>
#include <vector>

namespace {
constexpr int64_t NFIXED = 31;

// `0` for SM_SUCCESS, else the status.
double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }

// `type lo hi` and five `status x y z` points at d.Evaluate(i / 4) of the natural
// interval; `-1` for a null curve.
void put_curve(std::vector<double>& o, const SmCurve* c) {
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

// The body: kind 0 null, 1 `path` read by SmApiReadBrepFromFile (ASCII, no rebuild),
// 2 SmApiCreateBox(0, a[1..3]) then SmApiCircularFillet(a[4]) when a[4] > 0; then, when
// a[5] != 0, Transform by an SmAxis2Placement translated by a[6..8]. `ok` false on a
// failed build (`status` says why).
SmBrep* build_body(const char* path, const double* a, SmStatus& status, bool& ok) {
    ok = true;
    SmBrep* b = NULL;
    if (a[0] == 1.0) {
        status = SmApiReadBrepFromFile(path, TRUE, FALSE, b);
        if (status != SM_SUCCESS || !b) { delete b; ok = false; return NULL; }
    } else if (a[0] == 2.0) {
        status = SmApiCreateBox(SmVector3d(0, 0, 0), a[1], a[2], a[3], b);
        if (status != SM_SUCCESS || !b) { delete b; ok = false; return NULL; }
        if (a[4] > 0.0) {
            status = SmApiCircularFillet(b, a[4]);
            if (status != SM_SUCCESS) { delete b; ok = false; return NULL; }
        }
    }
    if (b && a[5] != 0.0) {
        SmAxis2Placement move;
        move.Translate(SmVector3d(a[6], a[7], a[8]));
        status = b->Transform(move);
        if (status != SM_SUCCESS) { delete b; ok = false; return NULL; }
    }
    return b;
}

int32_t refuse(SmStatus status, double* out, int64_t cap, int64_t* out_len) {
    std::vector<double> o = {-999.0, (double)status};
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
}

// `size` then the records of the entries after the `prefill` NULL ones.
void put_output(std::vector<double>& o, SmTArray<SmCurve*>& curves, ULONG prefill) {
    o.push_back((double)curves.GetSize());
    for (ULONG i = prefill; i < curves.GetSize(); i++) put_curve(o, curves[i]);
}
}

// Args: body kind, box size (3), fillet radius, has_translate, translation (3), point (3),
// normal or projection vector (3), prefill (NULL entries already in the output array),
// want_2d, want_3d, surface corners (u0v0, u1v0, u0v1, u1v1; 12), n, then n points (3n)
// of the curve (SmApiCreateCurve).
// Op 1: SmApiProjectBrepOntoPlane(body, point, normal, out) -> `status size curve*`.
// Op 2: SmApiCreateSilhouetteCurves(body, point, normal, out) -> `status size curve*`.
// Op 3: SmApiProjectCurve(body, curve, vector, out) -> `status size curve*`.
// Op 4: SmApiProjectCurve(curve, surface, want_2d ? &c2 : NULL, want_3d ? &c3 : NULL)
//   -> `status n2d curve* n3d curve*` (`-1` for an output not asked for).
// `size curve*` are the output array's size and its entries after the prefilled ones.
// A failed body or curve build returns `-999 status`.
SC_EXPORT int32_t sc_topology_api_projection_eval(int32_t op, const char* path,
        const double* a, int64_t na, double* out, int64_t cap, int64_t* out_len) {
    if (op < 1 || op > 4 || !a || na < NFIXED || !out_len || !path) return SC_ERR_ARG;
    int64_t npts = (int64_t)a[30];
    if (npts < 0 || na != NFIXED + 3 * npts) return SC_ERR_ARG;
    SC_TRY
    SmApiGetOrCreateContext();
    SmStatus status = SM_SUCCESS;
    std::vector<double> o;
    ULONG prefill = (ULONG)a[15];
    SmVector3d point(a[9], a[10], a[11]);
    SmVector3d normal(a[12], a[13], a[14]);
    SmBSplineCurve* curve = NULL;
    if (op >= 3) {
        SmTArray<SmPoint3d> pts;
        for (int64_t i = 0; i < npts; i++)
            pts.Add(SmPoint3d(a[NFIXED + 3 * i], a[NFIXED + 3 * i + 1], a[NFIXED + 3 * i + 2]));
        status = SmApiCreateCurve(pts, curve);
        if (status != SM_SUCCESS || !curve) {
            delete curve;
            return refuse(status, out, cap, out_len);
        }
    }
    if (op == 4) {
        SmBSplineSurface* surface = NULL;
        status = SmBSplineSurface::CreateBilinearSurface(*SmApiGetOrCreateContext(),
            SmPoint3d(a[18], a[19], a[20]), SmPoint3d(a[21], a[22], a[23]),
            SmPoint3d(a[24], a[25], a[26]), SmPoint3d(a[27], a[28], a[29]), surface);
        if (status != SM_SUCCESS || !surface) {
            delete surface;
            delete curve;
            return refuse(status, out, cap, out_len);
        }
        SmTArray<SmBSplineCurve*> c2, c3;
        SmStatus s = SmApiProjectCurve(curve, surface, a[16] != 0.0 ? &c2 : NULL,
                                       a[17] != 0.0 ? &c3 : NULL);
        o.push_back(st(s));
        if (a[16] != 0.0) {
            o.push_back((double)c2.GetSize());
            for (ULONG i = 0; i < c2.GetSize(); i++) put_curve(o, c2[i]);
        } else {
            o.push_back(-1.0);
        }
        if (a[17] != 0.0) {
            o.push_back((double)c3.GetSize());
            for (ULONG i = 0; i < c3.GetSize(); i++) put_curve(o, c3[i]);
        } else {
            o.push_back(-1.0);
        }
        for (ULONG i = 0; i < c2.GetSize(); i++) delete c2[i];
        for (ULONG i = 0; i < c3.GetSize(); i++) delete c3[i];
        delete surface;
        delete curve;
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    bool ok = true;
    SmBrep* body = build_body(path, a, status, ok);
    if (!ok) { delete curve; return refuse(status, out, cap, out_len); }
    SmTArray<SmCurve*> curves;
    for (ULONG i = 0; i < prefill; i++) curves.Add(NULL);
    SmStatus s = SM_SUCCESS;
    if (op == 1) s = SmApiProjectBrepOntoPlane(body, point, normal, curves);
    else if (op == 2) s = SmApiCreateSilhouetteCurves(body, point, normal, curves);
    else s = SmApiProjectCurve(body, curve, normal, curves);
    o.push_back(st(s));
    put_output(o, curves, prefill);
    for (ULONG i = prefill; i < curves.GetSize(); i++) delete curves[i];
    delete body;
    delete curve;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
