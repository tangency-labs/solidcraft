// SPDX-License-Identifier: Apache-2.0
// topology.brep_mirror: SmBrep::Mirror (source/SMLib/src/SmBrep.cpp) on real SmBrep
// fixtures. The operation is the real SmBrep member, not a copy of its algorithm; the
// B-rep construction and the raw graph export are the shared ones of
// `sc_topology_fixture.h` (read only from SMLib). Layout in
// crates/solidcore-ffi/src/units/topology_brep_mirror.rs.
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

// The geometry of the B-rep in list order: every edge's curve and its edge uses' UV trim
// curves (radial order), then every face's surface.
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

// Args: editing, placement O/X/Y (9). Op 0 exports the fixture graph (`0 graph`), op 2 its
// native face and vertex lists (`nf face* nv vertex*`); op 1
// runs Mirror: `record_len status geometry... 0 graph` of the mirrored B-rep.
SC_EXPORT int32_t sc_topology_brep_mirror_eval(int32_t op, const char* path, const double* input,
        int64_t n, const double* a, int64_t na, double* out, int64_t cap, int64_t* out_len) {
    if (op < 0 || op > 2 || !input || n < 1 || !a || na != 10 || !path || !out_len) return SC_ERR_ARG;
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
    b->m_bEditingEnabled = a[0] != 0 ? TRUE : FALSE;
    Graph g;
    walk(b, g, true);
    std::vector<double> o;
    if (op == 0) {
        o.push_back(0.0);
        export_graph(b, g, o);
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    if (op == 2) {
        // the native face and vertex lists (GetFaces / GetVertices order), as indices of
        // the exported graph: the import cannot derive them from the walk
        SmTArray<SmFace*> faces;
        SmTArray<SmVertex*> vertices;
        b->GetFaces(faces);
        b->GetVertices(vertices);
        o.push_back((double)faces.GetSize());
        for (ULONG i = 0; i < faces.GetSize(); ++i) o.push_back((double)g.find(3, faces[i]));
        o.push_back((double)vertices.GetSize());
        for (ULONG i = 0; i < vertices.GetSize(); ++i) o.push_back((double)g.find(9, vertices[i]));
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    SmAxis2Placement plane(SmPoint3d(a[1], a[2], a[3]), SmVector3d(a[4], a[5], a[6]),
                           SmVector3d(a[7], a[8], a[9]));
    status = b->Mirror(plane);
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
