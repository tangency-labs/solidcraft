// SPDX-License-Identifier: Apache-2.0
// SmBrep p6 on actual SMLib objects. Raw fixtures share the existing topology
// protocol; operation results are never used to construct the Rust input graph.
#include "sc_topology_fixture.h"
#include <memory>
SC_EXPORT int32_t sc_topology_brep_p6_eval(
    int32_t op, const char* path, const double* input, int64_t n,
    int64_t seed, const int64_t* indices, int64_t count, int32_t repeat,
    int32_t lock_mask, int32_t flags, double* out, int64_t cap, int64_t* len)
{
    if (!path || !input || !len || n < 1 || op < 0 || op > 2 || count < 0 || static_cast<uint64_t>(count) > UINT32_MAX ||
        (count && !indices) || (op != 1 && count != 0) || repeat < 1 || repeat > 2 || flags < 0 || flags > 31 ||
        lock_mask < 0 || lock_mask > 31)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{input, 0};
    Built built;
    SmStatus status;
    built.brep = build(rd, path, status);
    if (!built.brep) {
        const double rejected[] = {-1, st(status)};
        return sc_copy_out(rejected, 2, out, cap, len);
    }
    SmBrep* brep = built.brep;
    Graph g;
    walk(brep, g, true);
    if (op == 0 && (seed < 0 || (uint64_t)seed >= g.faceuses.size())) return SC_ERR_ARG;
    for (int64_t i=0; i<count; ++i) {
        if (indices[i] < 0 || (uint64_t)indices[i] >= g.edges.size()) return SC_ERR_ARG;
        auto* edge = g.edges[indices[i]];
        // CopyEdges dereferences the returned trimmed NURBS without a null
        // check. Keep the guard branch-specific and test the actual constructor.
        if (op == 1) {
            SmBSplineCurve* curve = edge->CreateTrimmedNURBSCurve(the_context());
            if (!curve) {
                const double rejected[] = {-2};
                return sc_copy_out(rejected, 1, out, cap, len);
            }
            delete curve;
        }
    }
    std::unique_ptr<SmNewMarkAndLock> outer;
    if (lock_mask) outer.reset(new SmNewMarkAndLock(&the_context(), lock_mask));
    const SmMarkType types[] = {SM_MT_MARK, SM_MT_MARK2, SM_MT_MARK3, SM_MT_MARKIO, SM_MT_MARKASSERT};
    ULONG before[5];
    for (int m=0; m<5; ++m) {
        before[m] = the_context().GetCurrentMark(types[m]);
        for (auto* fu : g.faceuses) fu->UnMark(types[m]);
    }
    Built destination;
    destination.brep = new(the_context()) SmBrep();
    SmTArray<SmEdge*> copied;
    SmShell* new_shell = g.shells.empty() ? NULL : g.shells[0];
    SmRegion* new_region = g.regions.empty() ? NULL : g.regions[0];
    SmFace* new_face = g.faces.empty() ? NULL : g.faces[0];
    if (!g.edges.empty()) copied.Add(g.edges[0]);
    for (int j=0; j<repeat; ++j) {
        if (op == 0)
            status = brep->TestClosureSplitRegion((flags & 1) != 0, g.faceuses[seed], new_shell, new_region);
        else if (op == 1) {
            SmTArray<SmEdge*> edges;
            for (int64_t i=0; i<count; ++i) edges.Add(g.edges[indices[i]]);
            status = brep->CopyEdges(edges, destination.brep, (flags & 2) ? &copied : NULL);
            new_shell = NULL; new_region = NULL; new_face = NULL;
        } else {
            SmSurface* surface = (flags & 4) ? NULL : new(the_context()) SmPlane(SmPoint3d(2,-3,5), SmVector3d(0,0,1));
            SmTArray<ULONG> loops;
            SmTArray<SmEdgeuse*> edgeuses;
            SmTArray<SmVertex*> vertices;
            status = brep->MakeFaceTopology(NULL, surface, (flags & 8) ? SM_OT_OPPOSITE : SM_OT_SAME,
                                            loops, edgeuses, vertices, new_region, new_shell, new_face,
                                            (flags & 16) != 0, (flags & 1) != 0, NULL);
            // The successful face owns surface; null-surface failure allocated none.
        }
        if (status != SM_SUCCESS) break;
    }
    std::vector<double> result{st(status)};
    if (op == 0) {
        result.push_back(g.find(1, new_shell));
        result.push_back(g.find(0, new_region));
    } else if (op == 1 && (flags & 2)) result.push_back(copied.GetSize());
    else if (op == 2) {
        result.push_back(new_face ? 1 : 0);
        result.push_back(new_shell ? 1 : 0);
        result.push_back(new_region ? 1 : 0);
    }
    for (int m=0; m<5; ++m) {
        result.push_back(the_context().GetCurrentMark(types[m]) - before[m]);
        result.push_back(the_context().IsMarkLocked(types[m]) ? 1 : 0);
        for (auto* fu : g.faceuses) result.push_back(fu->IsMarked(types[m]) ? 1 : 0);
    }
    for (auto* fu : g.faceuses) result.push_back(g.find(1,fu->GetShell()));
    SmTArray<SmFace*> faces; brep->GetFaces(faces);
    SmTArray<SmEdge*> edges; brep->GetEdges(edges);
    SmTArray<SmVertex*> vertices; brep->GetVertices(vertices);
    result.push_back(faces.GetSize()); result.push_back(edges.GetSize()); result.push_back(vertices.GetSize());
    SmTArray<SmEdge*> dest_edges; destination.brep->GetEdges(dest_edges); result.push_back(dest_edges.GetSize());
    SmTArray<SmRegion*> dest_regions; destination.brep->GetRegions(dest_regions);
    SmRegion* infinite = destination.brep->GetInfiniteRegion();
    int64_t infinite_index = -1;
    for (ULONG i=0; i<dest_regions.GetSize(); ++i) {
        if (dest_regions[i] == infinite) { infinite_index = i; break; }
    }
    result.push_back(dest_regions.GetSize());
    result.push_back(infinite_index);
    result.push_back(infinite ? infinite->IsVoid() : -1);
    return sc_copy_out(result.data(), result.size(), out, cap, len);
    SC_CATCH
}
