// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.tools`. Wraps upstream SmTools (source/SMLib/src/SmTools.cpp):
// FindFirstPointWithinTolToPnt, FindFirstVertexWithinTolToPnt, FindCoincidentEdge, FindEdge,
// FindFace.
//
// Vertices for FindFirstVertexWithinTolToPnt are free SmVertex objects (point + stored
// tolerance) from a process-wide pool. The B-rep entry points read an ASCII .smb file once per
// path (cached for the process; objects are never deleted: topology destructors notify owners).
// Edges and faces are reported by their index in SmBrep::GetEdges / GetFaces order.
// Statuses: 0 = SM_SUCCESS, else the SmStatus value (SC_ERR_* for shim rejections).
#include "sc_shim.h"

#include <SmBrep.h>
#include <SmCurve.h>
#include <SmEdge.h>
#include <SmFace.h>
#include <SmLine.h>
#include <SmTools.h>
#include <SmVertex.h>

#include <map>
#include <string>
#include <vector>

namespace
{
struct ScVertex : public SmVertex
{
    ScVertex() : SmVertex() {}
    void Set(const SmPoint3d& p, double tol)
    {
        m_vPoint = p;
        m_sZoneTol3d = tol;
    }
};

const SmContext* the_context()
{
    static SmContext* p = new SmContext();
    return p;
}

SmLine* the_line()
{
    static SmLine* p = new (the_context())
        SmLine(SmPoint3d(0.0, 0.0, 0.0), SmPoint3d(1.0, 0.0, 0.0), 3, the_context());
    return p;
}

std::vector<ScVertex*>& vertex_pool(size_t n)
{
    static std::vector<ScVertex*> pool;
    while (pool.size() < n)
        pool.push_back(new (static_cast<SmObject*>(the_line())) ScVertex());
    return pool;
}

SmBrep* brep_of(const char* path)
{
    static std::map<std::string, SmBrep*> breps;
    auto it = breps.find(path);
    if (it != breps.end())
        return it->second;
    SmBrep* b = new (*the_context()) SmBrep();
    if (b->ReadFromFile(*the_context(), path, SM_ASCII) != SM_SUCCESS)
        b = nullptr; // leaked on purpose (see header)
    breps[path] = b;
    return b;
}

int32_t status_of(SmStatus s) { return s == SM_SUCCESS ? 0 : (int32_t)s; }
} // namespace

// pts[3*n]; out_ints = [found, index]; out_dbls = [closest, second closest].
SC_EXPORT int32_t sc_geom_tools_find_point(const double* pts, int64_t n, int64_t target, double tol,
                                           int64_t* out_ints, double* out_dbls)
{
    if (n < 1 || target < 0 || target >= n)
        return SC_ERR_ARG;
    SC_TRY
    SmTArray<SmPoint3d> a;
    for (int64_t i = 0; i < n; ++i)
        a.Add(SmPoint3d(pts[3 * i], pts[3 * i + 1], pts[3 * i + 2]));
    SmBoolean found = FALSE;
    ULONG idx = 0;
    double d1 = 0.0, d2 = 0.0;
    SmStatus st = SmTools::FindFirstPointWithinTolToPnt(a, (ULONG)target, tol, found, idx, d1, d2);
    out_ints[0] = found ? 1 : 0;
    out_ints[1] = (int64_t)idx;
    out_dbls[0] = d1;
    out_dbls[1] = d2;
    return status_of(st);
    SC_CATCH
}

// pts[3*n], tols[n]; out_ints = [found, index]; out_dbls = [distance].
SC_EXPORT int32_t sc_geom_tools_find_vertex(const double* pts, const double* tols, int64_t n,
                                            const double* pnt, double tol, int64_t* out_ints,
                                            double* out_dbls)
{
    if (n < 0)
        return SC_ERR_ARG;
    SC_TRY
    std::vector<ScVertex*>& pool = vertex_pool((size_t)n);
    SmTArray<SmVertex*> a;
    for (int64_t i = 0; i < n; ++i)
    {
        pool[(size_t)i]->Set(SmPoint3d(pts[3 * i], pts[3 * i + 1], pts[3 * i + 2]), tols[i]);
        a.Add(pool[(size_t)i]);
    }
    SmBoolean found = FALSE;
    ULONG idx = 0;
    double d = 0.0;
    SmStatus st = SmTools::FindFirstVertexWithinTolToPnt(a, SmPoint3d(pnt[0], pnt[1], pnt[2]), tol,
                                                         found, idx, d);
    out_ints[0] = found ? 1 : 0;
    out_ints[1] = (int64_t)idx;
    out_dbls[0] = d;
    return status_of(st);
    SC_CATCH
}

// Target = the curve of edge `edge` (copied when `copy_curve`, so it has no owner edge) over the
// edge interval, between its vertices (swapped when `swap`). Searches all GetEdges from `start`.
// out_ints = [n_edges, found, same_orient, index]; out_dbls = [max deviation].
SC_EXPORT int32_t sc_geom_tools_find_coincident_edge(const char* path, int64_t edge,
                                                     int32_t copy_curve, int32_t swap, double tol,
                                                     int64_t start, int64_t* out_ints,
                                                     double* out_dbls)
{
    if (path == nullptr || edge < 0 || start < 0)
        return SC_ERR_ARG;
    SC_TRY
    SmBrep* b = brep_of(path);
    if (b == nullptr)
        return SC_ERR_ARG;
    SmTArray<SmEdge*> edges;
    b->GetEdges(edges);
    out_ints[0] = (int64_t)edges.GetSize();
    if (edge >= (int64_t)edges.GetSize())
        return SC_ERR_ARG;
    SmEdge* e = edges[(ULONG)edge];
    SmCurve* c = e->GetCurve();
    if (c == nullptr)
        return SC_ERR_ARG;
    if (copy_curve)
    {
        SmCurve* copy = nullptr;
        if (c->Copy(*the_context(), copy) != SM_SUCCESS || copy == nullptr)
            return SC_ERR_ARG;
        c = copy; // leaked on purpose (see header)
    }
    SmVertex* v1 = e->GetVertex();
    SmVertex* v2 = e->GetOtherVertex(v1);
    if (swap)
    {
        SmVertex* t = v1;
        v1 = v2;
        v2 = t;
    }
    SmBoolean found = FALSE, same = FALSE;
    ULONG idx = 0;
    double dev = 0.0;
    SmStatus st = SmTools::FindCoincidentEdge(v1, v2, c, e->GetInterval(), tol, edges, found, same,
                                              idx, dev, (ULONG)start);
    out_ints[1] = found ? 1 : 0;
    out_ints[2] = found ? (same ? 1 : 0) : 0;
    out_ints[3] = found ? (int64_t)idx : 0;
    out_dbls[0] = dev;
    return status_of(st);
    SC_CATCH
}

// out_ints = [n_edges, index]; out_dbls = [param] (both only meaningful on status 0).
SC_EXPORT int32_t sc_geom_tools_find_edge(const char* path, const double* pnt, int64_t* out_ints,
                                          double* out_dbls)
{
    if (path == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    SmBrep* b = brep_of(path);
    if (b == nullptr)
        return SC_ERR_ARG;
    SmTArray<SmEdge*> edges;
    b->GetEdges(edges);
    out_ints[0] = (int64_t)edges.GetSize();
    ULONG idx = 0;
    double param = 0.0;
    SmStatus st = SmTools::FindEdge(b, SmPoint3d(pnt[0], pnt[1], pnt[2]), idx, param);
    out_ints[1] = (int64_t)idx;
    out_dbls[0] = param;
    return status_of(st);
    SC_CATCH
}

// out_ints = [n_faces, face index in GetFaces order or -1].
SC_EXPORT int32_t sc_geom_tools_find_face(const char* path, const double* pnt, int64_t* out_ints)
{
    if (path == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    SmBrep* b = brep_of(path);
    if (b == nullptr)
        return SC_ERR_ARG;
    SmTArray<SmFace*> faces;
    b->GetFaces(faces);
    out_ints[0] = (int64_t)faces.GetSize();
    SmFace* f = nullptr;
    SmStatus st = SmTools::FindFace(b, SmPoint3d(pnt[0], pnt[1], pnt[2]), f);
    out_ints[1] = -1;
    for (ULONG i = 0; f != nullptr && i < faces.GetSize(); ++i)
        if (faces[i] == f)
            out_ints[1] = (int64_t)i;
    return status_of(st);
    SC_CATCH
}
