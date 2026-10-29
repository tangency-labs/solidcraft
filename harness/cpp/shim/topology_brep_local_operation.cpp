// SPDX-License-Identifier: Apache-2.0
// topology.brep_local_operation: SmBrep::LocalOperation (source/SMLib/src/SmBrep.cpp) on real
// SmBrep fixtures. The operation is the real SmBrep member, not a copy of its algorithm; the
// B-rep construction and the raw graph export are the shared ones of `sc_topology_fixture.h`
// (read only from SMLib). Layout in
// crates/solidcore-ffi/src/units/topology_brep_local_operation.rs.
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

// The geometry of the B-rep in list order: every edge's curve and its edge use count (the UV
// trim curves are lazily made caches and not recorded), then every face's surface.
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

// Args: `count` then per replaced face `index offset tx ty tz`: the new surface is the plane
// through the face's surface point at the middle of the face's UV domain, moved `offset`
// along the surface normal there, with the normal plus `(tx,ty,tz)`. Op 0 exports the fixture
// graph (`0 graph`), op 2 its native face and vertex lists (`nf face* nv vertex*`); op 1 runs
// LocalOperation: `record_len status geometry... 0 graph` of the changed B-rep. A face
// without surface or a degenerate normal returns `-998`.
SC_EXPORT int32_t sc_topology_brep_local_operation_eval(int32_t op, const char* path,
        const double* input, int64_t n, const double* a, int64_t na, double* out, int64_t cap,
        int64_t* out_len) {
    if (op < 0 || op > 2 || !input || n < 1 || !a || na < 1 || !path || !out_len) return SC_ERR_ARG;
    int64_t count = (int64_t)a[0];
    if (count < 0 || na != 1 + 5 * count) return SC_ERR_ARG;
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
    Graph g;
    walk(b, g, true);
    std::vector<double> o;
    SmTArray<SmFace*> faces;
    SmTArray<SmVertex*> vertices;
    b->GetFaces(faces);
    b->GetVertices(vertices);
    if (op == 0) {
        o.push_back(0.0);
        export_graph(b, g, o);
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    if (op == 2) {
        // the native face and vertex lists (GetFaces / GetVertices order), as indices of
        // the exported graph: the import cannot derive them from the walk
        o.push_back((double)faces.GetSize());
        for (ULONG i = 0; i < faces.GetSize(); ++i) o.push_back((double)g.find(3, faces[i]));
        o.push_back((double)vertices.GetSize());
        for (ULONG i = 0; i < vertices.GetSize(); ++i) o.push_back((double)g.find(9, vertices[i]));
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    SmTArray<SmFace*> replaced;
    SmTArray<SmSurface*> surfaces;
    for (int64_t i = 0; i < count; ++i) {
        const double* r = a + 1 + 5 * i;
        int64_t index = (int64_t)r[0];
        if (index < 0 || index >= (int64_t)faces.GetSize() || !faces[index]->GetSurface()) {
            std::vector<double> bad = {-998.0};
            return sc_copy_out(bad.data(), 1, out, cap, out_len);
        }
        SmFace* f = faces[index];
        SmPoint2d uv = f->GetUVDomain().Evaluate(0.5, 0.5);
        SmPoint3d p;
        SmVector3d normal;
        if (f->GetSurface()->EvaluatePoint(uv, p) != SM_SUCCESS ||
            f->GetSurface()->EvaluateNormal(uv, TRUE, TRUE, normal) != SM_SUCCESS) {
            std::vector<double> bad = {-998.0};
            return sc_copy_out(bad.data(), 1, out, cap, out_len);
        }
        SmPoint3d origin(p.x + normal.x * r[1], p.y + normal.y * r[1], p.z + normal.z * r[1]);
        SmVector3d tilted(normal.x + r[2], normal.y + r[3], normal.z + r[4]);
        if (!(tilted.x * tilted.x + tilted.y * tilted.y + tilted.z * tilted.z > 1e-12)) {
            std::vector<double> bad = {-998.0};
            return sc_copy_out(bad.data(), 1, out, cap, out_len);
        }
        replaced.Add(f);
        surfaces.Add(new (the_context()) SmPlane(origin, tilted, &the_context()));
    }
    status = b->LocalOperation(replaced, surfaces);
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
