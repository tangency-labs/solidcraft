// SPDX-License-Identifier: Apache-2.0
// Live, file-backed SmTrimSrfCache tests. The input-only graph export remains
// topology_face_p2's protocol; no operation results are imported from that export.
#include "sc_topology_fixture.h"

#include <SmPointClass.h>
#include <SmSurfaceCache.h>
#include <SmTree.h>
#include <SmTrimSrfCache.h>

#include <cmath>
#include <memory>

namespace
{
struct NativeFaceCache : SmTrimSrfCache
{
    explicit NativeFaceCache(const SmSurface& s) : SmTrimSrfCache(s) {}
    SmTree* stored_tree() const { return m_pTree; }
    const SmExtent2d& stored_domain() const { return m_sUVDomain; }
};

struct NativeFaceTree : SmTree
{
    static SmMemBlockMgr SmTree::*nodes() { return &NativeFaceTree::m_sNodeMgr; }
};

struct FaceProbe
{
    int mode; // 0: domain fraction, 1: trim midpoint, 2: trim start
    int64_t edgeuse;
    SmPoint2d uv;
    double tolerance;
};

void face_domain(std::vector<double>& out, const SmExtent2d& d)
{
    out.insert(out.end(), {d.GetMin().x, d.GetMin().y, d.GetMax().x, d.GetMax().y});
}

void face_object(std::vector<double>& out, const SmObject* object, const Graph& graph)
{
    if (object == nullptr)
    {
        out.insert(out.end(), {-1.0, -1.0});
        return;
    }
    const SM_TYPE type = object->GetType();
    int64_t index = -3;
    if (type == SmFace_TYPE) index = graph.find(3, object);
    else if (type == SmEdge_TYPE) index = graph.find(7, object);
    else if (type == SmVertex_TYPE) index = graph.find(9, object);
    // An unexpected or unregistered identity is retained, never replaced with a face.
    out.insert(out.end(), {(double)type, (double)index});
}

void face_test(std::vector<double>& out, SmSurfaceCache* cache, const FaceProbe& probe,
               bool use_tolerance, const Graph& graph)
{
    SmBoolean ok = TRUE;
    SmObject* object = nullptr;
    SmZoneTol3d tolerance(probe.tolerance);
    const SmStatus status = cache->PointTest(probe.uv, ok, use_tolerance ? &tolerance : nullptr, &object);
    out.insert(out.end(), {(double)status, ok == TRUE ? 1.0 : 0.0});
    face_object(out, object, graph);
}

void face_classify(std::vector<double>& out, SmSurfaceCache* cache, const FaceProbe& probe,
                   const Graph& graph)
{
    SmPointClassificationType classification = SM_PC_UNKNOWN;
    SmObject* object = nullptr;
    const SmStatus status = cache->PointClassify(probe.uv, SmZoneTol3d(probe.tolerance), classification, object);
    out.insert(out.end(), {(double)status, (double)classification});
    face_object(out, object, graph);
}
} // namespace

// Input: surface_type loop_count occurrence build_mode editing modified test_tol
//        boundary_probes sample_count (u_fraction v_fraction source_tolerance)*.
// Output is documented in the safe FFI module. Every native result is computed here.
SC_EXPORT int32_t sc_surface_trim_srf_cache_faces_eval(
    const char* path, const double* input, int64_t input_len,
    double* output, int64_t capacity, int64_t* output_len)
{
    if (path == nullptr || input == nullptr || output_len == nullptr || input_len < 9 || capacity < 0 ||
        (capacity != 0 && output == nullptr)) return SC_ERR_ARG;
    // Repeat the layout guard before any casts or native dereferences.
    for (int64_t i = 0; i < input_len; ++i)
        if (!std::isfinite(input[i])) return SC_ERR_ARG;
    if ((input[0] != SmPlane_TYPE && input[0] != SmBSplineSurface_TYPE) ||
        input[1] < 0 || input[1] > UINT32_MAX || std::floor(input[1]) != input[1] ||
        input[2] < 0 || input[2] > UINT32_MAX || std::floor(input[2]) != input[2] ||
        input[8] < 0 || input[8] > 64 || std::floor(input[8]) != input[8]) return SC_ERR_ARG;
    if (input[3] < 0 || input[3] > 5 || std::floor(input[3]) != input[3]) return SC_ERR_ARG;
    for (int i = 4; i <= 7; ++i)
        if (input[i] != 0.0 && input[i] != 1.0) return SC_ERR_ARG;
    const int64_t count = (int64_t)input[8];
    if (input_len != 9 + count * 3) return SC_ERR_ARG;
    for (int64_t i = 0; i < count; ++i)
        if (input[11 + i * 3] < 0.0) return SC_ERR_ARG;

    SC_TRY
    SmContext context;
    Built built;
    built.brep = new (context) SmBrep();
    const SmStatus read_status = built.brep->ReadFromFile(context, path, SM_ASCII, FALSE);
    std::vector<double> out{(double)read_status};
    if (read_status != SM_SUCCESS)
        return sc_copy_out(out.data(), (int64_t)out.size(), output, capacity, output_len);

    Graph graph;
    walk(built.brep, graph);
    SmFace* face = nullptr;
    SmTArray<SmLoopuse*> loops;
    int64_t occurrence = (int64_t)input[2];
    for (SmFace* candidate : graph.faces)
    {
        SmSurface* surface = candidate->GetSurface();
        if (surface == nullptr || surface->GetType() != (SM_TYPE)input[0]) continue;
        candidate->GetUpwardLoopuses(loops);
        if (loops.GetSize() != (ULONG)input[1]) continue;
        if (occurrence-- == 0) { face = candidate; break; }
    }
    if (face == nullptr || face->GetSurface() == nullptr) return SC_ERR_ARG;

    const SmExtent2d domain = face->GetUVDomain();
    out.insert(out.end(), {(double)graph.find(3, face), (double)face->GetSurface()->GetType(),
                           (double)loops.GetSize(), face->GetSurface()->GetOwner() == face ? 1.0 : 0.0});
    face_domain(out, domain);
    built.brep->m_bEditingEnabled = input[4] != 0.0 ? TRUE : FALSE;
    std::unique_ptr<NativeFaceCache> cache(new (context) NativeFaceCache(*face->GetSurface()));
    // input[3]: 0 none, 1 BuildTree, 2 BuildTree then GetTree, 3 BuildTree, editing
    // off, GetTree, 4 GetTree, 5 editing off, GetTree (GetTree's virtual BuildTree).
    const int mode = (int)input[3];
    const bool build_tree = mode >= 1;
    SmStatus build_status = (mode >= 1 && mode <= 3) ? cache->BuildTree(nullptr) : SM_SUCCESS;
    if (build_status == SM_SUCCESS && mode >= 2)
    {
        if (mode == 3 || mode == 5) built.brep->m_bEditingEnabled = FALSE;
        build_status = cache->GetTree() != nullptr ? SM_SUCCESS : SM_ERR;
    }
    cache->m_bFaceWasModified = input[5] != 0.0 ? TRUE : FALSE;
    out.insert(out.end(), {build_tree ? (double)build_status : -1.0,
                           built.brep->m_bEditingEnabled == TRUE ? 1.0 : 0.0,
                           cache->m_bFaceWasModified == TRUE ? 1.0 : 0.0,
                           cache->m_bProcessBoundaryCurves == TRUE ? 1.0 : 0.0,
                           cache->m_bPointTestEnabled == TRUE ? 1.0 : 0.0,
                           cache->m_bHaveTSurfaceCache == TRUE ? 1.0 : 0.0,
                           (double)cache->m_bFaceContainmentDone});
    face_domain(out, cache->stored_domain());
    const ULONG nodes = cache->stored_tree() == nullptr ? 0 :
        (cache->stored_tree()->*NativeFaceTree::nodes()).GetNumActiveElements();
    out.push_back((double)nodes);
    if (build_status != SM_SUCCESS)
    {
        out.push_back(0.0);
        return sc_copy_out(out.data(), (int64_t)out.size(), output, capacity, output_len);
    }

    std::vector<FaceProbe> probes;
    for (int64_t i = 0; i < count; ++i)
    {
        const double* p = input + 9 + i * 3;
        SmPoint2d uv(domain.GetMin().x + p[0] * (domain.GetMax().x - domain.GetMin().x),
                     domain.GetMin().y + p[1] * (domain.GetMax().y - domain.GetMin().y));
        if (!std::isfinite(uv.x) || !std::isfinite(uv.y)) return SC_ERR_ARG;
        probes.push_back({0, -1, uv, p[2]});
    }
    if (input[7] != 0.0)
    {
        // Two probes per loop, including the hole, using actual upward UV trim curves.
        if (loops.GetSize() > 32) return SC_ERR_ARG;
        for (ULONG i = 0; i < loops.GetSize(); ++i)
        {
            SmTArray<SmEdgeuse*> edges;
            loops[i]->GetEdgeuses(edges);
            if (edges.GetSize() == 0) return SC_ERR_ARG;
            SmEdgeuse* edgeuse = edges[0];
            SmBSplineCurve* curve = edgeuse->GetUVTrimCurve();
            if (curve == nullptr) return SC_ERR_ARG;
            const SmExtent1d interval = curve->GetNaturalInterval();
            for (int mode : {1, 2})
            {
                const double t = mode == 1 ? (interval.GetMin() + interval.GetMax()) / 2.0 : interval.GetMin();
                SmPoint3d p;
                if (curve->EvaluatePoint(t, p) != SM_SUCCESS) return SC_ERR_ARG;
                probes.push_back({mode, graph.find(6, edgeuse), SmPoint2d(p.x, p.y), 1.0e-5});
            }
        }
    }
    out.push_back((double)probes.size());
    for (const FaceProbe& probe : probes)
    {
        out.insert(out.end(), {(double)probe.mode, (double)probe.edgeuse,
                               probe.uv.x, probe.uv.y, probe.tolerance});
        // Direct derived calls, then virtual dispatch through the retained base pointer.
        SmBoolean ok = TRUE;
        SmObject* object = nullptr;
        SmZoneTol3d tolerance(probe.tolerance);
        SmStatus status = cache->PointTest(probe.uv, ok, input[6] != 0.0 ? &tolerance : nullptr, &object);
        out.insert(out.end(), {(double)status, ok == TRUE ? 1.0 : 0.0});
        face_object(out, object, graph);
        SmPointClassificationType classification = SM_PC_UNKNOWN;
        object = nullptr;
        status = cache->PointClassify(probe.uv, tolerance, classification, object);
        out.insert(out.end(), {(double)status, (double)classification});
        face_object(out, object, graph);
        SmSurfaceCache* retained = cache.get();
        face_test(out, retained, probe, input[6] != 0.0, graph);
        face_classify(out, retained, probe, graph);
    }
    return sc_copy_out(out.data(), (int64_t)out.size(), output, capacity, output_len);
    SC_CATCH
}
