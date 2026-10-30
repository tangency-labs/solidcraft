// SPDX-License-Identifier: Apache-2.0
// topology.brep_cutting: SmApiCut (source/SM_API/src/SmApiBrep.cpp), that is SmBrepCutting::DoCut
// with an SmPlaneCutter (source/SMLib/src/SmBrepCutting.cpp, SmCutter.cpp), on a body the shim
// builds itself: an upstream .smb file read by SmBrep::ReadFromFile (ASCII, no rebuild), or
// SmApiCreateBox optionally filleted by SmApiCircularFillet. The Rust side builds the same body
// with its own reader / primitives, so no C++ state reaches it. The cut is the real SmApiCut;
// the result's raw graph export is the shared one of `sc_topology_fixture.h`. Layout in
// crates/solidcore-ffi/src/units/topology_brep_cutting.rs.
#include "sc_topology_fixture.h"
#include <SmApiBrep.h>
#include <SmApiFillets.h>
#include <SmApiGeneral.h>
#include <SmApiPrimitives.h>
#include <memory>

namespace {
constexpr int64_t NARGS = 11;

// The body: kind 0 = `path` read by ReadFromFile, 1 = SmApiCreateBox(0, size a[1..3]) then,
// when a[4] > 0, SmApiCircularFillet(a[4]). Null on a failed build.
SmBrep* build_body(const char* path, const double* a, SmStatus& status) {
    if (a[0] == 0.0) {
        if (!path) { status = SM_ERR; return nullptr; }
        const SmContext& ctx = the_context();
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
    return box;
}

// Evaluates `c` at five parameters of its natural interval: `status [xyz]` each.
void put_curve_samples(std::vector<double>& o, SmCurve* c) {
    o.push_back(c ? (double)c->GetType() : -1.0);
    if (!c) return;
    SmExtent1d d = c->GetNaturalInterval();
    o.push_back(d.GetMin());
    o.push_back(d.GetMax());
    for (int i = 0; i < 5; ++i) {
        SmPoint3d p;
        SmStatus s = c->EvaluatePoint(d.Evaluate((double)i / 4), p);
        o.push_back(s == SM_SUCCESS ? 0.0 : 1.0);
        if (s == SM_SUCCESS) put_vec(o, p);
    }
}

// Evaluates `s` on a 3x3 grid of its natural UV domain.
void put_surface_samples(std::vector<double>& o, SmSurface* s) {
    o.push_back(s ? (double)s->GetType() : -1.0);
    if (!s) return;
    SmExtent2d d = s->GetNaturalUVDomain();
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
    for (int i = 0; i < 3; ++i)
        for (int j = 0; j < 3; ++j) {
            SmPoint2d uv(d.GetMin().x + (d.GetMax().x - d.GetMin().x) * i / 2.0,
                         d.GetMin().y + (d.GetMax().y - d.GetMin().y) * j / 2.0);
            SmPoint3d p;
            SmStatus st_ = s->EvaluatePoint(uv, p);
            o.push_back(st_ == SM_SUCCESS ? 0.0 : 1.0);
            if (st_ == SM_SUCCESS) put_vec(o, p);
        }
}

// The geometry of the B-rep in list order: every edge's curve and its edge use count (the UV
// trim curves are lazily made caches and not recorded), then every face's surface (as
// topology.brep_local_operation's shim records it).
void put_geometry(std::vector<double>& o, SmBrep* b) {
    SmTArray<SmEdge*> edges;
    b->GetEdges(edges);
    o.push_back((double)edges.GetSize());
    for (ULONG i = 0; i < edges.GetSize(); ++i) {
        put_curve_samples(o, edges[i]->GetCurve());
        SmTArray<SmEdgeuse*> uses;
        edges[i]->GetEdgeuses(uses);
        o.push_back((double)uses.GetSize());
    }
    SmTArray<SmFace*> faces;
    b->GetFaces(faces);
    o.push_back((double)faces.GetSize());
    for (ULONG i = 0; i < faces.GetSize(); ++i)
        put_surface_samples(o, faces[i]->GetSurface());
}
}

// Args (11): body kind, box size (3), fillet radius, plane point (3), plane normal (3).
// Runs SmApiCut(body, point, normal): `record_len status geometry... 0 graph` of the B-rep
// after the call (also after a failing one: SmApiCut does not restore it). A failed body
// build returns `-999 status`.
SC_EXPORT int32_t sc_topology_brep_cutting_eval(const char* path, const double* a, int64_t na,
                                                double* out, int64_t cap, int64_t* out_len) {
    if (!a || na != NARGS || !out_len) return SC_ERR_ARG;
    SC_TRY
    SmStatus status = SM_SUCCESS;
    std::unique_ptr<SmBrep> body(build_body(path, a, status));
    if (!body || status != SM_SUCCESS) {
        std::vector<double> o = {-999.0, st(status)};
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    SmBrep* b = body.get();
    SmVector3d point(a[5], a[6], a[7]);
    SmVector3d normal(a[8], a[9], a[10]);
    status = SmApiCut(b, point, normal);
    std::vector<double> record = {st(status)};
    put_geometry(record, b);
    std::vector<double> result = {(double)record.size()};
    result.insert(result.end(), record.begin(), record.end());
    result.push_back(0.0);
    Graph after;
    walk(b, after, true);
    export_graph(b, after, result);
    return sc_copy_out(result.data(), (int64_t)result.size(), out, cap, out_len);
    SC_CATCH
}
