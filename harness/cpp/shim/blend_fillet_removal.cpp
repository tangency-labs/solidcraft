// SPDX-License-Identifier: Apache-2.0
// blend.fillet_removal: SmBrep::DeleteFilletFace (source/SMLib/src/SmFilletRemoval.cpp) on real
// SmBrep fixtures. The removal is the real SmBrep member, not a copy of its algorithm; the
// B-rep construction and the raw graph export are the shared ones of `sc_topology_fixture.h`
// (read only from SMLib). Layout in crates/solidcore-ffi/src/units/blend_fillet_removal.rs.
#include "sc_topology_fixture.h"
#include <memory>

namespace {
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

// The geometry of the B-rep in list order: every edge's curve and its edge uses' stored UV
// trim curves (radial order; nothing is made), then every face's surface.
void put_geometry(std::vector<double>& o, SmBrep* b) {
    SmTArray<SmEdge*> edges;
    b->GetEdges(edges);
    o.push_back((double)edges.GetSize());
    for (ULONG i = 0; i < edges.GetSize(); ++i) {
        put_curve_samples(o, edges[i]->GetCurve());
        SmTArray<SmEdgeuse*> uses;
        edges[i]->GetEdgeuses(uses);
        o.push_back((double)uses.GetSize());
        for (ULONG j = 0; j < uses.GetSize(); ++j)
            put_curve_samples(o, uses[j]->*EuPeek::uv());
    }
    SmTArray<SmFace*> faces;
    b->GetFaces(faces);
    o.push_back((double)faces.GetSize());
    for (ULONG i = 0; i < faces.GetSize(); ++i)
        put_surface_samples(o, faces[i]->GetSurface());
}

}

// Builds the fixture B-rep from `input` (the shared B-rep block) and runs
// `DeleteFilletFace(GetFaces()[face], NULL)` with editing enabled for the call:
// `record_len status geometry... 0 graph` of the B-rep after it. A failed fixture build
// returns `-999 status`; a face index past the B-rep's faces `-998`.
SC_EXPORT int32_t sc_blend_fillet_removal_eval(const char* path, const double* input, int64_t n,
        int64_t face, double* out, int64_t cap, int64_t* out_len) {
    if (!input || n < 1 || !path || !out_len || face < 0) return SC_ERR_ARG;
    SC_TRY
    Rd rd{input, 0};
    SmStatus status = SM_SUCCESS;
    Built built;
    built.brep = build(rd, path, status);
    if (!built.brep || status != SM_SUCCESS) {
        std::vector<double> o = {-999.0, st(status)};
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    SmBrep* b = built.brep;
    SmTArray<SmFace*> faces;
    b->GetFaces(faces);
    if (face >= (int64_t)faces.GetSize()) {
        std::vector<double> bad = {-998.0};
        return sc_copy_out(bad.data(), 1, out, cap, out_len);
    }
    {
        SmTemporaryChangeValue<SmBoolean> edit(b->m_bEditingEnabled, TRUE);
        status = b->DeleteFilletFace(faces[(ULONG)face]);
    }
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
