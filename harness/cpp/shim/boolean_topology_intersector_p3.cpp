// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `boolean.topology_intersector_p3`: SmTopologyIntersector::
// MergeCurveClasses, MergeCurveOnSurfaces and MergeCoincidentSurfaces
// (source/SMLib/src/SmTopologyIntersector.cpp) on real SmBrep objects.
//
// One entry point, sc_boolean_topology_intersector_p3_eval(op, path, in, n_in, out, cap,
// out_len). Layouts are documented in
// crates/solidcore-ffi/src/units/boolean_topology_intersector_p3.rs, which validates every
// input. The B-rep construction, traversal and export are those of sc_topology_fixture.h
// (two B-rep blocks; `path` holds the two file paths separated by a newline), exactly as in
// the boolean.topology_intersector_p1 shim, so the Rust runner imports the same export and
// names objects with the same `brep kind index` handles.
#include "sc_topology_fixture.h"

#include <SmCurveClass.h>
#include <SmPointClass.h>
#include <SmTopologyIntersector.h>

#include <algorithm>
#include <string>

namespace
{
enum TiOp : int32_t
{
    TI_EXPORT = 0,
    TI_RUN = 1,
    TI_VERTEX_USES = 2,
};

// The merge op of a TI_RUN input.
enum MergeOp : int64_t
{
    M_CURVE_ON_SURFACES = 1,
    M_CURVE_CLASSES = 2,
    M_COINCIDENT_SURFACES = 3,
};

// Handle kinds (`kind` of a `brep kind index` triple), as in the p1 shim.
enum HKind : int64_t
{
    H_VERTEX = 0,
    H_EDGE = 1,
    H_FACE = 2,
    H_EDGE_CURVE = 3,
    H_FACE_SURFACE = 4,
    H_BREP = 6,
    H_NULL = 8,
    // an edge use's UV trim curve (`GetUVTrimCurve`: its own or its mate's)
    H_TRIM_CURVE = 13,
};

struct Two
{
    SmBrep* brep[2] = {nullptr, nullptr};
    Graph g[2];
    // the edge curves and face surfaces at walk time, by address (never dereferenced)
    std::map<const void*, int64_t> curve_of_edge[2];
    std::map<const void*, int64_t> surface_of_face[2];

    void walk_all()
    {
        for (int b = 0; b < 2; b++)
        {
            g[b] = Graph();
            curve_of_edge[b].clear();
            surface_of_face[b].clear();
            if (brep[b] == nullptr)
                continue;
            walk(brep[b], g[b]);
            for (size_t k = 0; k < g[b].edges.size(); k++)
                if (g[b].edges[k]->GetCurve() != nullptr)
                    curve_of_edge[b].emplace((const void*)g[b].edges[k]->GetCurve(), (int64_t)k);
            for (size_t k = 0; k < g[b].faces.size(); k++)
                if (g[b].faces[k]->GetSurface() != nullptr)
                    surface_of_face[b].emplace((const void*)g[b].faces[k]->GetSurface(), (int64_t)k);
        }
    }

    SmObject* resolve(int64_t b, int64_t kind, int64_t i) const
    {
        if (kind == H_NULL || i < 0)
            return nullptr;
        if (kind == H_BREP)
            return (b == 0 || b == 1) ? (SmObject*)brep[b] : nullptr;
        if (b != 0 && b != 1)
            return nullptr;
        const Graph& gr = g[b];
        auto at = [i](auto& v) -> SmTopology* { return (i >= 0 && i < (int64_t)v.size()) ? (SmTopology*)v[(size_t)i] : nullptr; };
        switch (kind)
        {
        case H_VERTEX: return at(gr.vertices);
        case H_EDGE: return at(gr.edges);
        case H_FACE: return at(gr.faces);
        case H_EDGE_CURVE:
        {
            SmEdge* e = (SmEdge*)at(gr.edges);
            return e ? (SmObject*)e->GetCurve() : nullptr;
        }
        case H_FACE_SURFACE:
        {
            SmFace* f = (SmFace*)at(gr.faces);
            return f ? (SmObject*)f->GetSurface() : nullptr;
        }
        case H_TRIM_CURVE:
        {
            SmEdgeuse* eu = (SmEdgeuse*)at(gr.edgeuses);
            return eu ? (SmObject*)eu->GetUVTrimCurve() : nullptr;
        }
        default: return nullptr;
        }
    }

    // Emits the `brep kind index` triple of an object (`-1 8 -1` null, `-1 -3 -1` unknown,
    // e.g. a deleted or newly made object).
    void put(std::vector<double>& o, const SmObject* p) const
    {
        auto emit = [&o](double b, double k, double i) {
            o.push_back(b);
            o.push_back(k);
            o.push_back(i);
        };
        if (p == nullptr)
        {
            emit(-1.0, (double)H_NULL, -1.0);
            return;
        }
        for (int b = 0; b < 2; b++)
        {
            if (brep[b] == nullptr)
                continue;
            if ((const SmObject*)brep[b] == p)
            {
                emit(b, (double)H_BREP, b);
                return;
            }
            const Graph& gr = g[b];
            int64_t i;
            if ((i = gr.find(9, (const SmVertex*)p)) >= 0)
            {
                emit(b, (double)H_VERTEX, (double)i);
                return;
            }
            if ((i = gr.find(7, (const SmEdge*)p)) >= 0)
            {
                emit(b, (double)H_EDGE, (double)i);
                return;
            }
            if ((i = gr.find(3, (const SmFace*)p)) >= 0)
            {
                emit(b, (double)H_FACE, (double)i);
                return;
            }
            auto ce = curve_of_edge[b].find((const void*)p);
            if (ce != curve_of_edge[b].end())
            {
                emit(b, (double)H_EDGE_CURVE, (double)ce->second);
                return;
            }
            auto fs = surface_of_face[b].find((const void*)p);
            if (fs != surface_of_face[b].end())
            {
                emit(b, (double)H_FACE_SURFACE, (double)fs->second);
                return;
            }
        }
        emit(-1.0, -3.0, -1.0);
    }

    std::vector<double> key(const SmObject* p) const
    {
        std::vector<double> k;
        put(k, p);
        return k;
    }
};

SmObject* rd_h(Rd& rd, const Two& t)
{
    int64_t b = rd.idx();
    int64_t kind = rd.idx();
    int64_t i = rd.idx();
    return t.resolve(b, kind, i);
}

void put_list(std::vector<double>& o, const Two& t, const std::vector<SmObject*>& v)
{
    o.push_back((double)v.size());
    for (SmObject* p : v)
        t.put(o, p);
}

template <class T> std::vector<SmObject*> as_objs(const SmTArray<T*>& a)
{
    std::vector<SmObject*> v;
    for (ULONG i = 0; i < a.GetSize(); i++)
        v.push_back((SmObject*)a[i]);
    return v;
}

void put_pc(std::vector<double>& o, const Two& t, const SmPointClassification& pc)
{
    o.push_back((double)pc.GetPointClass());
    t.put(o, pc.GetObject());
}

void read_pc(Rd& rd, const Two& t, SmPointClassification& pc)
{
    int64_t cls = rd.idx();
    SmObject* obj = rd_h(rd, t);
    pc.SetClassObject((SmPointClassificationType)cls, obj);
}

// A classification spec: `curve(h) t0 t1 tol start(cls h) mid(cls h) end(cls h)`.
struct ClassSpec
{
    SmCurve* curve = nullptr;
    double t0 = 0.0, t1 = 0.0, tol = 0.0;
    int64_t cls[3] = {0, 0, 0};
    SmObject* obj[3] = {nullptr, nullptr, nullptr};
};

ClassSpec read_spec(Rd& rd, const Two& t)
{
    ClassSpec s;
    s.curve = (SmCurve*)rd_h(rd, t);
    s.t0 = rd.num();
    s.t1 = rd.num();
    s.tol = rd.num();
    for (int k = 0; k < 3; k++)
    {
        s.cls[k] = rd.idx();
        s.obj[k] = rd_h(rd, t);
    }
    return s;
}

// Sets the single interval's start, mid and end classes of a classification built with
// the full constructor.
void set_classes(SmCurveClassification& cc, const ClassSpec& s)
{
    SmCurveInterval& ivl = cc[0];
    ivl.m_vStart.SetClassObject((SmPointClassificationType)s.cls[0], s.obj[0]);
    ivl.m_vMid.SetClassObject((SmPointClassificationType)s.cls[1], s.obj[1]);
    ivl.m_vEnd.SetClassObject((SmPointClassificationType)s.cls[2], s.obj[2]);
}

void put_class(std::vector<double>& o, const Two& t, const SmCurveClassification& cc)
{
    o.push_back((double)cc.GetSize());
    for (ULONG i = 0; i < cc.GetSize(); i++)
    {
        const SmCurveInterval& ivl = cc[i];
        o.push_back(ivl.m_vInterval.GetMin());
        o.push_back(ivl.m_vInterval.GetMax());
        put_pc(o, t, ivl.m_vStart);
        put_pc(o, t, ivl.m_vMid);
        put_pc(o, t, ivl.m_vEnd);
    }
}

// Status of an op the shim refuses because upstream would dereference null.
constexpr double NULL_DEREF = -2.0;

bool edge_ok(const SmEdge* e) { return e != nullptr && e->GetCurve() != nullptr && e->GetVertex() != nullptr; }

// A classification spec MergeCurveClasses can run on: a curve (the constructor's ReSet
// and the fix-ups dereference it) and class objects of the kinds their classes cast them
// to. Checked before the classification is constructed.
bool spec_ok(const ClassSpec& s)
{
    if (s.curve == nullptr)
        return false;
    // the raw class/object pairs: SetClassObject would turn a vertex, edge or face class
    // without object into SM_PC_UNKNOWN with its parameter marked set, which the merge
    // then mishandles (a crash), so those are refused here, as the Rust runner does
    for (int k = 0; k < 3; k++)
    {
        const SmObject* obj = s.obj[k];
        switch ((SmPointClassificationType)s.cls[k])
        {
        case SM_PC_VERTEX:
            if (obj == nullptr || !obj->IsKindOf(SmVertex_TYPE))
                return false;
            break;
        case SM_PC_EDGE:
            if (obj == nullptr || !obj->IsKindOf(SmEdge_TYPE) || !edge_ok((const SmEdge*)obj))
                return false;
            break;
        case SM_PC_FACE:
            if (obj == nullptr || !obj->IsKindOf(SmFace_TYPE))
                return false;
            break;
        default: break;
        }
    }
    // MergeInterval gives an unknown start or end the mid's face (sm_ComputeFacePointClass,
    // which leaves it unknown and without object when the point does not project onto
    // that face) or region (CopyClassificationOnly; this runner names no region, so its
    // object is null). Then pStartV / pEndV is null and dereferenced (SmCurveClass.cpp
    // :17659 GetPoint, :17672 GetBrep).
    const int mid = s.cls[1];
    if ((mid == SM_PC_FACE || mid == SM_PC_REGION) && (s.cls[0] == SM_PC_UNKNOWN || s.cls[2] == SM_PC_UNKNOWN))
        return false;
    return true;
}

// The tolerance of a vertex, edge or face.
double tol_of(const SmObject* p)
{
    if (p == nullptr)
        return -1.0;
    if (p->IsKindOf(SmVertex_TYPE))
        return (double)((const SmVertex*)p)->GetTolerance();
    if (p->IsKindOf(SmEdge_TYPE))
        return (double)((const SmEdge*)p)->GetTolerance();
    if (p->IsKindOf(SmFace_TYPE))
        return (double)((const SmFace*)p)->GetTolerance();
    return -1.0;
}

// The state after the op: per B-rep the vertex, edge and face counts and every vertex and
// edge tolerance (traversal order, re-walked), then the mapped B-rep vertices, edges and
// faces (sorted by handle; upstream lists them in hash order) each with its other mate.
void put_state(std::vector<double>& o, Two& t, SmTopologyIntersector* ti)
{
    t.walk_all();
    for (int b = 0; b < 2; b++)
    {
        const Graph& gr = t.g[b];
        o.push_back((double)gr.vertices.size());
        o.push_back((double)gr.edges.size());
        o.push_back((double)gr.faces.size());
        for (SmVertex* v : gr.vertices)
            o.push_back(tol_of(v));
        for (SmEdge* e : gr.edges)
            o.push_back(tol_of(e));
    }
    const SM_TYPE types[3] = {SmVertex_TYPE, SmEdge_TYPE, SmFace_TYPE};
    for (SM_TYPE ty : types)
    {
        SmTArray<SmObject*> objs;
        ti->GetMappedBrepObjects(ty, objs);
        std::vector<SmObject*> v = as_objs(objs);
        std::sort(v.begin(), v.end(), [&t](SmObject* a, SmObject* b) { return t.key(a) < t.key(b); });
        o.push_back((double)v.size());
        for (SmObject* p : v)
        {
            t.put(o, p);
            t.put(o, ti->GetOtherMate((SmTopology*)p));
        }
    }
}

bool build_two(Rd& rd, const char* path, Two& t, std::vector<double>& o)
{
    std::string p(path);
    std::string pa = p, pb;
    size_t nl = p.find('\n');
    if (nl != std::string::npos)
    {
        pa = p.substr(0, nl);
        pb = p.substr(nl + 1);
    }
    SmStatus st_a;
    t.brep[0] = build(rd, pa.c_str(), st_a);
    if (t.brep[0] == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(st_a));
        return false;
    }
    bool has_b = rd.flag();
    if (has_b)
    {
        SmStatus st_b;
        t.brep[1] = build(rd, pb.c_str(), st_b);
        if (t.brep[1] == nullptr)
        {
            o.push_back(-3.0);
            o.push_back(st(st_b));
            return false;
        }
    }
    t.walk_all();
    return true;
}

// Runs one merge: `approx ang doing_fillets ignore_tangency n_relates (a b)* op args`.
void run_merge(Rd& rd, Two& t, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    double approx = rd.num();
    double ang = rd.num();
    SmTopologyIntersector* ti = new SmTopologyIntersector(ctx, t.brep[0], t.brep[1], approx, ang);
    struct CleanTi
    {
        SmTopologyIntersector* ti;
        ~CleanTi() { delete ti; }
    } clean_ti{ti};
    ti->m_bDoingFillets = rd.flag();
    ti->m_bIgnoreTangency = rd.flag();

    // pre-existing relationships (statuses reported, as the Rust side must match them)
    int64_t n_rel = rd.idx();
    for (int64_t k = 0; k < n_rel; k++)
    {
        SmTopology* a = (SmTopology*)rd_h(rd, t);
        SmTopology* b = (SmTopology*)rd_h(rd, t);
        // Relate dereferences its B-rep object (IsKindOf) only after the conflict checks
        // (an existing different mate of either object is SM_ERR first)
        bool safe = a != nullptr;
        if (!safe)
        {
            SmTopology* oth = (SmTopology*)ti->GetOtherMate(a);
            SmTopology* brp = (SmTopology*)ti->GetBrepMate(b);
            safe = (oth != nullptr && oth != b) || brp != nullptr;
        }
        o.push_back(safe ? st(ti->Relate(a, b)) : NULL_DEREF);
    }

    int64_t op = rd.idx();
    o.push_back((double)op);
    switch (op)
    {
    case M_CURVE_ON_SURFACES:
    {
        SmSurface* s1 = (SmSurface*)rd_h(rd, t);
        SmSurface* s2 = (SmSurface*)rd_h(rd, t);
        double tol = rd.num();
        SmCurve* c = (SmCurve*)rd_h(rd, t);
        SmEdge* be = (SmEdge*)rd_h(rd, t);
        SmEdge* oe = (SmEdge*)rd_h(rd, t);
        SmCurve* uv1 = (SmCurve*)rd_h(rd, t);
        SmCurve* uv2 = (SmCurve*)rd_h(rd, t);
        // In upstream's order: a surface without face is SM_ERR_NULL_POINTER (safe); then
        // p3DCurve is dereferenced; a degenerate curve never reaches the edges or a missing
        // face; else side 1's edge (sm_SetEdgeClass) or face (CurveOnClassify) is
        // dereferenced. Side 2 is checked unconditionally (the runner only generates a
        // valid side 2; whether upstream reaches it depends on side 1's classification).
        bool no_face = (s1 != nullptr && s1->GetFace() == nullptr) || (s2 != nullptr && s2->GetFace() == nullptr);
        bool refuse = false;
        if (!no_face)
        {
            refuse = c == nullptr;
            if (!refuse && !c->IsDegenerate(tol))
                refuse = (be != nullptr ? !edge_ok(be) : s1 == nullptr) || (oe != nullptr ? !edge_ok(oe) : s2 == nullptr);
        }
        if (refuse)
        {
            o.push_back(NULL_DEREF);
            return;
        }
        SmBoolean deleted = FALSE;
        SmTArray<SmEdge*> del_e;
        SmTArray<SmVertex*> del_v;
        SmStatus s = ti->MergeCurveOnSurfaces(s1, s2, tol, c, uv1, uv2, be, oe, deleted, &del_e, &del_v);
        o.push_back(st(s));
        // the outputs only on success (the port returns none with an error); deleted
        // objects are no longer in the re-walked graph: their handles are unknown
        t.walk_all();
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(deleted));
            put_list(o, t, as_objs(del_e));
            put_list(o, t, as_objs(del_v));
        }
        break;
    }
    case M_CURVE_CLASSES:
    {
        SmBoolean merge_sing = rd.flag();
        ClassSpec a = read_spec(rd, t);
        ClassSpec b = read_spec(rd, t);
        if (!spec_ok(a) || !spec_ok(b))
        {
            o.push_back(NULL_DEREF);
            return;
        }
        SmCurveClassification cc1(a.curve, SmExtent1d(a.t0, a.t1), nullptr, a.tol);
        SmCurveClassification cc2(b.curve, SmExtent1d(b.t0, b.t1), nullptr, b.tol);
        set_classes(cc1, a);
        set_classes(cc2, b);
        SmBoolean deleted = FALSE;
        SmTArray<SmEdge*> del_e;
        SmTArray<SmVertex*> del_v;
        SmStatus s = ti->MergeCurveClasses(cc1, cc2, merge_sing, deleted, &del_e, &del_v);
        o.push_back(st(s));
        t.walk_all();
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(deleted));
            put_list(o, t, as_objs(del_e));
            put_list(o, t, as_objs(del_v));
            put_class(o, t, cc1);
            put_class(o, t, cc2);
        }
        break;
    }
    case M_COINCIDENT_SURFACES:
    {
        SmSurface* s1 = (SmSurface*)rd_h(rd, t);
        SmSurface* s2 = (SmSurface*)rd_h(rd, t);
        // both faces are dereferenced (CalculateBoundingBox) unchecked
        if (s1 == nullptr || s2 == nullptr || s1->GetFace() == nullptr || s2->GetFace() == nullptr)
        {
            o.push_back(NULL_DEREF);
            return;
        }
        SmTArray<SmFace*> faces, ofaces;
        SmStatus s = ti->MergeCoincidentSurfaces(s1, s2, faces, ofaces);
        o.push_back(st(s));
        t.walk_all();
        if (s == SM_SUCCESS)
        {
            put_list(o, t, as_objs(faces));
            put_list(o, t, as_objs(ofaces));
        }
        break;
    }
    default: o.push_back(-9.0); return;
    }
    put_state(o, t, ti);
}

void run_ti(int32_t op, Rd& rd, const char* path, std::vector<double>& o)
{
    Two t;
    struct Clean
    {
        Two& t;
        ~Clean()
        {
            delete t.brep[0];
            delete t.brep[1];
        }
    } clean{t};
    if (!build_two(rd, path, t, o))
        return;
    o.push_back(0.0);
    if (op == TI_VERTEX_USES)
    {
        // each vertex's uses in its list order (the shared export does not carry it)
        for (int b = 0; b < 2; b++)
        {
            if (t.brep[b] == nullptr)
            {
                o.push_back(-1.0);
                continue;
            }
            const Graph& gr = t.g[b];
            o.push_back((double)gr.vertices.size());
            for (SmVertex* v : gr.vertices)
            {
                std::vector<SmTopology*> uses = list_of(v->GetList());
                o.push_back((double)uses.size());
                for (SmTopology* u : uses)
                    o.push_back((double)gr.find(8, (const SmVertexuse*)u));
            }
        }
        return;
    }
    if (op == TI_EXPORT)
    {
        for (int b = 0; b < 2; b++)
        {
            if (t.brep[b] == nullptr)
            {
                o.push_back(0.0);
                continue;
            }
            std::vector<double> ex;
            ex.push_back(0.0);
            export_graph(t.brep[b], t.g[b], ex);
            o.push_back((double)ex.size());
            o.insert(o.end(), ex.begin(), ex.end());
        }
        return;
    }
    run_merge(rd, t, o);
}
} // namespace

SC_EXPORT int32_t sc_boolean_topology_intersector_p3_eval(int32_t op, const char* path, const double* in, int64_t n_in,
                                                          double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    run_ti(op, rd, path, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
