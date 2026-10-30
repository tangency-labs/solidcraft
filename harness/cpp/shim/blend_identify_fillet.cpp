// SPDX-License-Identifier: Apache-2.0
// blend.identify_fillet: SmFace::IdentifyFillet (source/SMLib/src/SmFace.cpp) on the faces of real
// SmBrep fixtures. The identification is the real SmFace member, not a copy of its algorithm;
// the B-rep construction is the shared one of `sc_topology_fixture.h` (read only from SMLib).
// Layout in crates/solidcore-ffi/src/units/blend_identify_fillet.rs.
#include "sc_topology_fixture.h"

namespace {
// The position of `e` in `edges` (first occurrence), or -1.
double edge_index(const SmTArray<SmEdge*>& edges, SmEdge* e) {
    for (ULONG i = 0; i < edges.GetSize(); ++i)
        if (edges[i] == e) return (double)i;
    return -1.0;
}
}

// Builds the fixture B-rep from `input` (the shared B-rep block) and runs
// `IdentifyFillet(SM_ZONE_TOL_3D, 5.0, flags & 1, flags & 2, ...)` on `GetFaces()[face]`, with
// `pMinMaxRadii` when `flags & 4`. The outputs start at the values `SmApiFillets.cpp`'s
// `sm_IsFilletFace` gives them. Returns `status is_fillet cross_section solver_type rail_dir
// radius dist [radii_min radii_max] n_min idx* n_max idx*`, the rail edges as positions in the
// face's outer-loop `GetEdges` list. A failed fixture build returns `-999 status`; a face index
// past the B-rep's faces `-998`.
SC_EXPORT int32_t sc_blend_identify_fillet_eval(const char* path, const double* input, int64_t n,
        int64_t face, int64_t flags, double* out, int64_t cap, int64_t* out_len) {
    if (!input || n < 1 || !path || !out_len || face < 0 || flags < 0 || flags > 7)
        return SC_ERR_ARG;
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
    SmFace* f = faces[(ULONG)face];
    SmBoolean is_fillet = FALSE;
    ULONG cross = 0, solver = 0;
    SmSurfParamType rail_dir = SM_SP_U;
    double radius = 0, dist = 0;
    SmTArray<SmEdge*> rail_min, rail_max;
    SmExtent1d radii;
    status = f->IdentifyFillet(SM_ZONE_TOL_3D, 5.0, (flags & 1) ? TRUE : FALSE,
                               (flags & 2) ? TRUE : FALSE, is_fillet, cross, solver, rail_dir,
                               radius, dist, rail_min, rail_max, (flags & 4) ? &radii : NULL);
    std::vector<double> o = {st(status)};
    if (status == SM_SUCCESS) {
        o.push_back(is_fillet ? 1.0 : 0.0);
        o.push_back((double)cross);
        o.push_back((double)solver);
        o.push_back((double)rail_dir);
        o.push_back(radius);
        o.push_back(dist);
        if (flags & 4) {
            o.push_back(radii.GetMin());
            o.push_back(radii.GetMax());
        }
        SmTArray<SmLoop*> loops;
        f->GetLoops(loops);
        SmTArray<SmEdge*> edges;
        if (loops.GetSize() > 0 && loops[0] != NULL) loops[0]->GetEdges(edges);
        o.push_back((double)rail_min.GetSize());
        for (ULONG i = 0; i < rail_min.GetSize(); ++i) o.push_back(edge_index(edges, rail_min[i]));
        o.push_back((double)rail_max.GetSize());
        for (ULONG i = 0; i < rail_max.GetSize(); ++i) o.push_back(edge_index(edges, rail_max[i]));
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
