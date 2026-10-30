// SPDX-License-Identifier: Apache-2.0
// topology.api_files: the SM_API files and conversion wrappers SmApiReadBrepFromFile,
// SmApiReadPartFromFile, SmApiWriteBrepToFile (source/SM_API/src/SmApiImportExport.cpp) and SmApiTurnToNurbs
// (source/SM_API/src/SmApiBrep.cpp), called as the library exports them. The bodies are
// the shim's own: an upstream file read by the API, SmApiCreateBox optionally filleted
// by SmApiCircularFillet, the analytic primitives of SM_API_test TestSmBrepCopy and its
// mixed box (face 0 replaced by its CopyAnalyticAsNurb); the Rust side builds the same
// ones itself. Layout in crates/solidcore-ffi/src/units/topology_api_files.rs.
#include "sc_shim.h"
#include "StdAfx.h"
#include "support/primitive_record.h"
#include <SmApiBrep.h>
#include <SmApiFillets.h>
#include <SmApiGeneral.h>
#include <SmApiImportExport.h>
#include <SmApiPrimitives.h>
#include <SmBrep.h>
#include <SmCurve.h>
#include <SmEdge.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFace.h>
#include <SmSurface.h>
#include <SmVertex.h>
#include <cmath>
#include <cstdio>
#include <vector>

namespace {
constexpr int64_t NARGS = 9;

// `0` for SM_SUCCESS, else the status.
double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }

// The B-rep record: tolerance, then the region, shell, face, loop (over the faces'
// GetLoops), edge and vertex counts, then per face its surface type (`-1` for none), per
// edge its curve type (`-1` for none) and interval, per vertex its point.
void put_brep(std::vector<double>& o, SmBrep* b) {
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
    ULONG loops = 0;
    for (ULONG f = 0; f < faces.GetSize(); f++) {
        SmTArray<SmLoop*> l;
        faces[f]->GetLoops(l);
        loops += l.GetSize();
    }
    o.push_back((double)b->GetTolerance());
    o.push_back((double)regions.GetSize());
    o.push_back((double)shells.GetSize());
    o.push_back((double)faces.GetSize());
    o.push_back((double)loops);
    o.push_back((double)edges.GetSize());
    o.push_back((double)vertices.GetSize());
    for (ULONG f = 0; f < faces.GetSize(); f++) {
        SmSurface* s = faces[f]->GetSurface();
        o.push_back(s ? (double)s->GetType() : -1.0);
    }
    for (ULONG e = 0; e < edges.GetSize(); e++) {
        SmCurve* c = edges[e]->GetCurve();
        o.push_back(c ? (double)c->GetType() : -1.0);
        SmExtent1d ivl = edges[e]->GetInterval();
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
    }
    for (ULONG v = 0; v < vertices.GetSize(); v++) {
        SmPoint3d p = vertices[v]->GetPoint();
        o.push_back(p.x);
        o.push_back(p.y);
        o.push_back(p.z);
    }
}

// `status x y z` of an evaluation (`status` only on a failure).
void put_point(std::vector<double>& o, SmStatus s, const SmPoint3d& p) {
    o.push_back(s == SM_SUCCESS ? 0.0 : 1.0);
    if (s == SM_SUCCESS) { o.push_back(p.x); o.push_back(p.y); o.push_back(p.z); }
}

// The geometry record after TurnToNurbs: the B-rep record, then per face (GetFaces) its
// surface's natural UV domain and its points at u, v in {0, 1/2, 1} of that domain (u
// outer), per edge (GetEdges) its curve's points at t in {0, 1/4, .., 1} of the edge
// interval. Parameters are `lo + (hi - lo) * s`. A face without surface or an edge
// without curve adds nothing.
void put_geometry(std::vector<double>& o, SmBrep* b) {
    put_brep(o, b);
    SmTArray<SmFace*> faces;
    SmTArray<SmEdge*> edges;
    b->GetFaces(faces);
    b->GetEdges(edges);
    for (ULONG f = 0; f < faces.GetSize(); f++) {
        SmSurface* s = faces[f]->GetSurface();
        if (!s) continue;
        SmExtent2d d = s->GetNaturalUVDomain();
        double u0 = d.GetMin().x, v0 = d.GetMin().y, u1 = d.GetMax().x, v1 = d.GetMax().y;
        o.push_back(u0);
        o.push_back(v0);
        o.push_back(u1);
        o.push_back(v1);
        for (int i = 0; i < 3; i++) {
            for (int j = 0; j < 3; j++) {
                SmPoint2d uv(u0 + (u1 - u0) * (i * 0.5), v0 + (v1 - v0) * (j * 0.5));
                SmPoint3d p;
                SmStatus s2 = s->EvaluatePoint(uv, p);
                put_point(o, s2, p);
            }
        }
    }
    for (ULONG e = 0; e < edges.GetSize(); e++) {
        SmCurve* c = edges[e]->GetCurve();
        if (!c) continue;
        SmExtent1d ivl = edges[e]->GetInterval();
        double t0 = ivl.GetMin(), t1 = ivl.GetMax();
        for (int i = 0; i < 5; i++) {
            SmPoint3d p;
            SmStatus s2 = c->EvaluatePoint(t0 + (t1 - t0) * (i * 0.25), p);
            put_point(o, s2, p);
        }
    }
}

// After a B-rep record: per edge (GetEdges) and per edge use (GetEdgeuses) with a UV
// trim curve, its type, natural interval and `status x y z` points at 0.17, 0.37 and
// 0.83 of it (primitive_record::sample), then primitive_record::append (vertices,
// edges with their use and trim counts, faces).
void put_trims(std::vector<double>& o, SmBrep* b) {
    SmTArray<SmEdge*> edges;
    b->GetEdges(edges);
    for (ULONG e = 0; e < edges.GetSize(); e++) {
        SmTArray<SmEdgeuse*> uses;
        edges[e]->GetEdgeuses(uses);
        for (ULONG u = 0; u < uses.GetSize(); u++) {
            SmCurve* c = uses[u]->GetUVTrimCurvePointer();
            if (!c) continue;
            SmExtent1d d = c->GetNaturalInterval();
            o.push_back((double)c->GetType());
            o.push_back(d.GetMin());
            o.push_back(d.GetMax());
            for (double f : {0.17, 0.37, 0.83}) {
                SmPoint3d p;
                SmStatus s = c->EvaluatePoint(d.Evaluate(f), p);
                primitive_record::sample(o, s, p);
            }
        }
    }
    primitive_record::append(o, b);
}

// SmApiCreateBox(0, a[5..7]), then SmApiCircularFillet(a[8]) when a[8] > 0. Null on a
// failed build (`status` says why).
SmBrep* build_box(const double* a, SmStatus& status) {
    SmBrep* box = nullptr;
    status = SmApiCreateBox(SmVector3d(0, 0, 0), a[5], a[6], a[7], box);
    if (status != SM_SUCCESS || !box) { delete box; return nullptr; }
    if (a[8] > 0.0) {
        status = SmApiCircularFillet(box, a[8]);
        if (status != SM_SUCCESS) { delete box; return nullptr; }
    }
    return box;
}

// The body of op 3: 3 SmApiCreateCylinder(0, a[5], a[6]), 4 SmApiCreateCone(0, a[5],
// a[6], a[7]), 5 SmApiCreateSphere(0, a[5]), 6 SmApiCreateTorus(0, a[5], a[6]), 7
// SmApiCreateBox(0, a[5..7]) with GetFaces()[0]'s surface replaced by its
// CopyAnalyticAsNurb (ReplaceSurface(face, nurb, FALSE)). Null on a failed build.
SmBrep* build_analytic(const double* a, SmStatus& status) {
    SmVector3d o(0, 0, 0);
    SmBrep* b = nullptr;
    switch ((int)a[4]) {
    case 3: status = SmApiCreateCylinder(o, a[5], a[6], b); break;
    case 4: status = SmApiCreateCone(o, a[5], a[6], a[7], b); break;
    case 5: status = SmApiCreateSphere(o, a[5], b); break;
    case 6: status = SmApiCreateTorus(o, a[5], a[6], b); break;
    default: status = SmApiCreateBox(o, a[5], a[6], a[7], b); break;
    }
    if (status != SM_SUCCESS || !b) { delete b; return nullptr; }
    if ((int)a[4] == 7) {
        SmTArray<SmFace*> faces;
        b->GetFaces(faces);
        SmSurface* nurb = NULL;
        status = faces[0]->GetSurface()->CopyAnalyticAsNurb(*b->GetContext(), nurb);
        if (status == SM_SUCCESS && !nurb) status = SM_ERR_NULL_POINTER;
        if (status == SM_SUCCESS) status = b->ReplaceSurface(faces[0], nurb, FALSE);
        if (status != SM_SUCCESS) { delete b; return nullptr; }
    }
    return b;
}

int32_t refuse(SmStatus status, double* out, int64_t cap, int64_t* out_len) {
    std::vector<double> o = {-999.0, (double)status};
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
}
}

// Args (9): has_path, ascii, rebuild, prefill (bits: 1 curves, 2 surfaces, 4 Boolean tree
// nodes, 8 B-reps), body (0 null, 1 the file at `path` read by SmApiReadBrepFromFile
// (ASCII, no rebuild), 2 box, 3-7 build_analytic), sizes (3), fillet radius.
// Op 1: SmApiReadBrepFromFile(has_path ? path : NULL, ascii, rebuild) -> `status brep
//   trims`.
// Op 2: SmApiReadPartFromFile(has_path ? path : NULL, ascii, ...) with the arrays the
//   prefill bits give one entry each (NULL pointers, the node 7) -> `status part`.
// Op 3: SmApiTurnToNurbs(body) -> `status geometry trims` (`status` only on a failure).
// Op 4: SmApiWriteBrepToFile(box, path, ascii), then op 1 on that file, then the file is
//   removed (upstream TestSmImportExport) -> `status brep trims`.
// A failed body build or write returns `-999 status`.
SC_EXPORT int32_t sc_topology_api_files_eval(int32_t op, const char* path, const double* a,
        int64_t na, double* out, int64_t cap, int64_t* out_len) {
    if (op < 1 || op > 4 || !a || na != NARGS || !out_len) return SC_ERR_ARG;
    if ((a[0] != 0.0 || op == 4 || (op == 3 && a[4] == 1.0)) && !path) return SC_ERR_ARG;
    SC_TRY
    SmApiGetOrCreateContext();
    const char* name = a[0] != 0.0 ? path : NULL;
    SmBoolean ascii = a[1] != 0.0 ? TRUE : FALSE;
    SmBoolean rebuild = a[2] != 0.0 ? TRUE : FALSE;
    std::vector<double> o;
    if (op == 1 || op == 4) {
        if (op == 4) {
            SmStatus status = SM_SUCCESS;
            SmBrep* box = build_box(a, status);
            if (!box) return refuse(status, out, cap, out_len);
            status = SmApiWriteBrepToFile(box, path, ascii);
            delete box;
            if (status != SM_SUCCESS) {
                std::remove(path);
                return refuse(status, out, cap, out_len);
            }
            name = path;
        }
        SmBrep* b = NULL;
        SmStatus s = SmApiReadBrepFromFile(name, ascii, rebuild, b);
        if (op == 4) std::remove(path);
        o.push_back(st(s));
        if (s == SM_SUCCESS && b) {
            put_brep(o, b);
            put_trims(o, b);
        }
        delete b;
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    if (op == 2) {
        int prefill = (int)a[3];
        SmTArray<SmCurve*> curves;
        SmTArray<SmSurface*> surfaces;
        SmTArray<long> trees;
        SmTArray<SmBrep*> breps;
        if (prefill & 1) curves.Add(NULL);
        if (prefill & 2) surfaces.Add(NULL);
        if (prefill & 4) trees.Add(7);
        if (prefill & 8) breps.Add(NULL);
        SmStatus s = SmApiReadPartFromFile(name, ascii, curves, surfaces, trees, breps);
        o.push_back(st(s));
        if (s == SM_SUCCESS) {
            // the topology.part_file record
            o.push_back((double)curves.GetSize());
            for (ULONG i = 0; i < curves.GetSize(); i++) {
                SmCurve* c = curves[i];
                SmExtent1d ivl = c->GetNaturalInterval();
                o.push_back((double)c->GetType());
                o.push_back((double)c->GetDim());
                o.push_back(ivl.GetMin());
                o.push_back(ivl.GetMax());
            }
            o.push_back((double)surfaces.GetSize());
            for (ULONG i = 0; i < surfaces.GetSize(); i++) {
                SmSurface* sf = surfaces[i];
                SmExtent2d d = sf->GetNaturalUVDomain();
                o.push_back((double)sf->GetType());
                o.push_back(d.GetMin().x);
                o.push_back(d.GetMin().y);
                o.push_back(d.GetMax().x);
                o.push_back(d.GetMax().y);
            }
            o.push_back((double)trees.GetSize());
            for (ULONG i = 0; i < trees.GetSize(); i++) o.push_back((double)trees[i]);
            o.push_back((double)breps.GetSize());
            for (ULONG i = 0; i < breps.GetSize(); i++) put_brep(o, breps[i]);
            for (ULONG i = 0; i < curves.GetSize(); i++) delete curves[i];
            for (ULONG i = 0; i < surfaces.GetSize(); i++) delete surfaces[i];
            for (ULONG i = 0; i < breps.GetSize(); i++) delete breps[i];
        }
        // (a refused call leaves the prefilled NULL entries, which are not the shim's)
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    // op 3
    SmBrep* b = NULL;
    if (a[4] == 1.0) {
        SmStatus status = SmApiReadBrepFromFile(path, TRUE, FALSE, b);
        if (status != SM_SUCCESS || !b) { delete b; return refuse(status, out, cap, out_len); }
    } else if (a[4] == 2.0) {
        SmStatus status = SM_SUCCESS;
        b = build_box(a, status);
        if (!b) return refuse(status, out, cap, out_len);
    } else if (a[4] >= 3.0) {
        SmStatus status = SM_SUCCESS;
        b = build_analytic(a, status);
        if (!b) return refuse(status, out, cap, out_len);
    }
    SmStatus s = SmApiTurnToNurbs(b);
    o.push_back(st(s));
    if (s == SM_SUCCESS && b) {
        put_geometry(o, b);
        put_trims(o, b);
    }
    delete b;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}

// The byte-for-byte write (sc_topology_api_files_write): SmApiWriteBrepToFile of a B-rep
// read from an upstream test file (SmBrep::ReadFromFile, ASCII) or made by an SM_API
// primitive constructor, to `tmp` (inside a directory the runner made for it), read back
// byte for byte and removed.
namespace
{
// Sources (keep in sync with `Source` in crates/solidcore-ffi/src/units/topology_api_files.rs).
enum : int32_t
{
    SRC_FILE = 0,     // SmBrep::ReadFromFile(ctx, path, SM_ASCII, rebuild)
    SRC_BOX = 1,      // SmApiCreateBox(O, length, width, height)
    SRC_SPHERE = 2,   // SmApiCreateSphere(C, radius)
    SRC_CYLINDER = 3, // SmApiCreateCylinder(O, radius, height)
    SRC_CONE = 4,     // SmApiCreateCone(O, radius_base, radius_top, height)
    SRC_TORUS = 5,    // SmApiCreateTorus(C, major, minor)
    SRC_PLANE = 6,    // SmApiCreatePlane(O, x, y)
};

// Arguments of each source.
const int64_t WRITE_ARGS[] = {0, 6, 4, 5, 6, 5, 5};

// The whole file `path`, or false when it cannot be read.
bool read_file_bytes(const char* path, std::vector<double>& o)
{
    FILE* f = std::fopen(path, "rb");
    if (f == nullptr)
        return false;
    std::vector<unsigned char> bytes;
    unsigned char buf[4096];
    size_t got;
    while ((got = std::fread(buf, 1, sizeof(buf), f)) > 0)
        bytes.insert(bytes.end(), buf, buf + got);
    std::fclose(f);
    o.push_back((double)bytes.size());
    for (unsigned char c : bytes)
        o.push_back((double)c);
    return true;
}
} // namespace

// Output: `source_status`, then on success `write_status`, then on success `n` and the
// file's `n` bytes. Statuses are 0 (SM_SUCCESS) or the SmStatus.
SC_EXPORT int32_t sc_topology_api_files_write(const char* path, const char* tmp,
                                                       int32_t kind, const double* a, int64_t n,
                                                       int32_t ascii, int32_t rebuild,
                                                       double* out, int64_t cap,
                                                       int64_t* out_len)
{
    if (tmp == nullptr || kind < SRC_FILE || kind > SRC_PLANE || (kind == SRC_FILE && path == nullptr) ||
        n != WRITE_ARGS[kind] || (n > 0 && a == nullptr))
        return SC_ERR_ARG;
    for (int64_t i = 0; i < n; i++)
        if (!std::isfinite(a[i]))
            return SC_ERR_ARG;
    SC_TRY
    const SmContext& ctx = *SmApiGetOrCreateContext();
    SmBrep* b = nullptr;
    SmStatus cs = SM_SUCCESS;
    SmVector3d o = n >= 3 ? SmVector3d(a[0], a[1], a[2]) : SmVector3d(0.0, 0.0, 0.0);
    switch (kind)
    {
    case SRC_FILE:
        b = new (ctx) SmBrep();
        cs = b->ReadFromFile(ctx, path, SM_ASCII, rebuild ? TRUE : FALSE);
        break;
    case SRC_BOX: cs = SmApiCreateBox(o, a[3], a[4], a[5], b); break;
    case SRC_SPHERE: cs = SmApiCreateSphere(o, a[3], b); break;
    case SRC_CYLINDER: cs = SmApiCreateCylinder(o, a[3], a[4], b); break;
    case SRC_CONE: cs = SmApiCreateCone(o, a[3], a[4], a[5], b); break;
    case SRC_TORUS: cs = SmApiCreateTorus(o, a[3], a[4], b); break;
    default: cs = SmApiCreatePlane(o, a[3], a[4], b); break;
    }
    std::vector<double> rec;
    rec.push_back(st(cs));
    if (cs == SM_SUCCESS && b != nullptr)
    {
        SmStatus ws = SmApiWriteBrepToFile(b, tmp, ascii ? TRUE : FALSE);
        rec.push_back(st(ws));
        if (ws == SM_SUCCESS && !read_file_bytes(tmp, rec))
        {
            delete b;
            std::remove(tmp);
            return SC_ERR_ARG;
        }
        std::remove(tmp);
    }
    delete b;
    return sc_copy_out(rec.data(), (int64_t)rec.size(), out, cap, out_len);
    SC_CATCH
}
