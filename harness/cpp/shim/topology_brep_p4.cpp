// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.brep_p4`: SmBrep::DeleteEdge, DeleteVertex,
// ShrinkGeometry, StitchFaces and ~SmBrep (source/SMLib/src/SmBrep.cpp) on real B-reps
// built by the shared fixture (sc_topology_fixture.h).
//
// One entry point, sc_topology_brep_p4_eval(op, path, in, n_in, editing, target, keep,
// tol, flag, damage, damage_index, out, cap, out_len). `damage` (0 none, 1 drop the surface
// of face `damage_index`, 2 clear the owner of shell `damage_index`; ops 0, 1 and 5 only)
// damages the built B-rep before the op, and the guard below refuses (`[-2]`, nothing run)
// exactly the damaged inputs upstream dereferences NULL on. `in` is a fixture B-rep block (validated by the Rust
// wrapper through the topology.topology_traverser export, whose `walk(brep, g, true)`
// order names `target` and `keep`). Ops:
//   0 DeleteEdge(edges[target], keep >= 0 ? faces[keep]->GetSurface() : NULL)
//   1 DeleteVertex(vertices[target])
//   2 ShrinkGeometry()
//   3 StitchFaces(tol, .., keep >= 0 ? {faces[keep]} : NULL, flag)
//   4 delete brep (~SmBrep)
//   5 no operation (the Rust side checks its imported model against the same records)
// `editing` sets m_bEditingEnabled first. Output: `[status]`, then on success the ten
// record counts of the B-rep walked again (op 3 adds its four outputs first), then
// the records of every object of the B-rep after the operation (layout below; op 4
// outputs the context's DoingBoolean flag after the destructor instead). The counts and
// records are also written after a failed status of ops 0-2 (the B-rep as the failure
// left it). `[-1]` when the fixture does not build.
//
// Records. Every object that existed before the operation is tagged with its index in
// the first walk (`m_lMarkIO = index + 1`; nothing in the operations touches that
// mark, unlike `m_lUserIndex1`, which `SmTopology::Notify` copies to new objects), so
// a record is named by that index however the heap reuses addresses. Objects the
// operation made are named `NEW_BASE + index in the second walk`; the Rust side
// renumbers them canonically (difftest `topology_brep_p4.rs`). Per kind, in the walk
// kind order: the count, then per record in label order its label and its fields as
// given by `emit_records` (links are labels, `-1` null; lists are a count and labels).
#include "sc_topology_fixture.h"

#include <algorithm>
#include <utility>

namespace {

constexpr double NEW_BASE = 1000000.0;

// Raw access to SmTopology::m_lMarkIO.
struct TopPeek : SmTopology
{
    static ULONG SmTopology::*mark_io() { return &TopPeek::m_lMarkIO; }
    static SmOwningTopology* SmTopology::*list_owner() { return &TopPeek::m_pListOwner; }
};

// Names every object of the first walk by its index.
void tag_objects(Graph& g)
{
    auto tag = [](auto& v) {
        for (size_t i = 0; i < v.size(); i++)
            v[i]->*TopPeek::mark_io() = (ULONG)(i + 1);
    };
    tag(g.regions);
    tag(g.shells);
    tag(g.faceuses);
    tag(g.faces);
    tag(g.loopuses);
    tag(g.loops);
    tag(g.edgeuses);
    tag(g.edges);
    tag(g.vertexuses);
    tag(g.vertices);
}

// The label of an object after the operation (`-1` for null).
double label(const Graph& after, int kind, SmTopology* p)
{
    if (p == nullptr)
        return -1.0;
    ULONG m = p->*TopPeek::mark_io();
    if (m > 0)
        return (double)(m - 1);
    return NEW_BASE + (double)after.find(kind, p);
}

void put_labels(std::vector<double>& o, const Graph& after, int kind, SmTopology* head)
{
    std::vector<SmTopology*> v = list_of(head);
    o.push_back((double)v.size());
    for (SmTopology* t : v)
        o.push_back(label(after, kind, t));
}

// One kind's records, sorted by label.
template <class T, class F>
void emit_kind(std::vector<double>& o, const Graph& after, int kind, std::vector<T*>& objs, F fields)
{
    std::vector<std::pair<double, T*>> v;
    for (T* p : objs)
        v.push_back({label(after, kind, p), p});
    std::sort(v.begin(), v.end(), [](auto& a, auto& b) { return a.first < b.first; });
    o.push_back((double)v.size());
    for (auto& e : v)
    {
        o.push_back(e.first);
        fields(e.second);
    }
}

void emit_records(SmBrep* brep, Graph& after, std::vector<double>& o)
{
    auto L = [&](int k, SmTopology* p) { return label(after, k, p); };
    emit_kind(o, after, 0, after.regions, [&](SmRegion* p) {
        o.push_back(p->GetOwner() == (SmOwningTopology*)brep ? 1.0 : 0.0);
    });
    emit_kind(o, after, 1, after.shells,
              [&](SmShell* p) { o.push_back(L(0, p->GetOwner())); });
    emit_kind(o, after, 2, after.faceuses, [&](SmFaceuse* p) {
        o.push_back(L(1, p->GetOwner()));
        o.push_back(L(3, p->*FuPeek::face()));
        o.push_back(L(2, p->*FuPeek::mate()));
        o.push_back((double)p->GetOrientation());
        put_labels(o, after, 4, p->GetList());
    });
    emit_kind(o, after, 3, after.faces, [&](SmFace* p) {
        o.push_back(L(2, p->*FPeek::fu()));
        SmExtent2d d = p->*FPeek::dom();
        o.push_back(d.GetMin().x);
        o.push_back(d.GetMin().y);
        o.push_back(d.GetMax().x);
        o.push_back(d.GetMax().y);
        o.push_back((double)(p->*FPeek::rect()));
        o.push_back((double)(p->*FPeek::tol()));
        SmSurface* s = p->*FPeek::srf();
        o.push_back(s ? 1.0 : 0.0);
        o.push_back(s && s->GetOwner() == (SmObject*)p ? 1.0 : 0.0);
        SmExtent2d n;
        if (s)
            n = s->GetNaturalUVDomain();
        o.push_back(s ? n.GetMin().x : 0.0);
        o.push_back(s ? n.GetMin().y : 0.0);
        o.push_back(s ? n.GetMax().x : 0.0);
        o.push_back(s ? n.GetMax().y : 0.0);
    });
    emit_kind(o, after, 4, after.loopuses, [&](SmLoopuse* p) {
        o.push_back(L(2, p->GetOwner()));
        o.push_back(L(5, p->*LuPeek::loop()));
        o.push_back(L(4, p->*LuPeek::mate()));
        SM_TYPE ty = p->*LuPeek::type();
        SmTopology* start = p->*LuPeek::start();
        o.push_back(ty == SmEdgeuse_TYPE ? 1.0 : ty == SmVertexuse_TYPE ? 2.0 : 0.0);
        o.push_back(ty == SmEdgeuse_TYPE ? L(6, start) : -1.0);
        o.push_back(ty == SmVertexuse_TYPE ? L(8, start) : -1.0);
        o.push_back((double)(p->*LuPeek::orient()));
    });
    emit_kind(o, after, 5, after.loops,
              [&](SmLoop* p) { o.push_back(L(4, p->GetLoopuse())); });
    emit_kind(o, after, 6, after.edgeuses, [&](SmEdgeuse* p) {
        o.push_back(L(7, p->GetOwner()));
        SM_TYPE ty = p->GetEdgeuseType();
        SmTopology* sorlu = p->*EuPeek::sorlu();
        o.push_back(ty == SmLoopuse_TYPE ? 1.0 : ty == SmShell_TYPE ? 2.0 : 0.0);
        o.push_back(ty == SmLoopuse_TYPE ? L(4, sorlu) : -1.0);
        o.push_back(ty == SmShell_TYPE ? L(1, sorlu) : -1.0);
        o.push_back(L(6, p->GetCWEdgeuse()));
        o.push_back(L(6, p->GetCCWEdgeuse()));
        o.push_back(L(8, p->*EuPeek::vu()));
        o.push_back((double)p->GetOrientation());
        o.push_back(p->*EuPeek::uv() ? 1.0 : 0.0);
    });
    emit_kind(o, after, 7, after.edges, [&](SmEdge* p) {
        o.push_back((double)p->GetTolerance());
        SmExtent1d ivl = p->GetInterval();
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
        put_labels(o, after, 6, p->GetList());
        o.push_back(p->GetCurve() ? 1.0 : 0.0);
    });
    emit_kind(o, after, 8, after.vertexuses, [&](SmVertexuse* p) {
        o.push_back(L(9, p->GetOwner()));
        SM_TYPE ty = p->GetVertexuseType();
        SmTopology* own = p->*VuPeek::owner();
        o.push_back(ty == SmEdgeuse_TYPE ? 1.0 : ty == SmLoopuse_TYPE ? 2.0 : ty == SmShell_TYPE ? 3.0 : 0.0);
        o.push_back(ty == SmEdgeuse_TYPE ? L(6, own) : -1.0);
        o.push_back(ty == SmLoopuse_TYPE ? L(4, own) : -1.0);
        o.push_back(ty == SmShell_TYPE ? L(1, own) : -1.0);
        SmPoint2d uv = p->*VuPeek::uv();
        o.push_back(uv.x);
        o.push_back(uv.y);
    });
    emit_kind(o, after, 9, after.vertices, [&](SmVertex* p) {
        SmPoint3d pt = p->GetPoint();
        o.push_back(pt.x);
        o.push_back(pt.y);
        o.push_back(pt.z);
        o.push_back((double)p->GetTolerance());
        put_labels(o, after, 8, p->GetList());
    });
}

// ---- hand-damaged B-reps and the null-dereference guard -----------------------------------
//
// The fixtures only build whole B-reps (every file face has a surface, every shell sits in
// a region). An adversarial case damages the built B-rep before the operation; the damage is
// a raw pointer write, so the objects stay in every list and the walk still reaches them:
//   1  the surface of face `damage_index` is dropped (the face keeps its other members);
//   2  the owner (region) pointer of shell `damage_index` is cleared.
// A damaged B-rep upstream would dereference NULL on is refused (the status `-2`, nothing
// run), and only that: every other damaged input runs and is compared like any other.
//
// Kind 1, DeleteEdge of a manifold edge (SmBrep.cpp:5829-5843): the surfaces of the two
// faces of the edge (`pEU->GetFace()->GetSurface()`, the radial's) are compared and, when they
// differ, a normal is evaluated on each (`pSrf->EvaluateNormal`): a face without a surface
// facing a face with one dereferences NULL. (The swap at :6050-6051 needs the same pair, so it
// is never the first dereference.) Two faces without surfaces compare equal and skip it. The
// lamina and wire branches never read a face's surface. Kind 2, DeleteEdge of a wire edge
// (:6827-6949) and DeleteVertex of a shell vertex (:7967-7969) call
// `pShell->GetRegion()->PostInsert / Remove` on the shell of the target: whether the edit gets
// there (a new shell is made, or the shell is removed) is read off the same operation on an
// undamaged copy, where it shows as a change of the number of shells.
const double ST_NULL_DEREF_GUARD = -2.0;

// DeleteEdge(edges[target]) reads two surfaces, one of them missing.
bool ub_surfaceless_edge(Graph& g, int64_t target)
{
    SmEdge* e = g.edges[(size_t)target];
    if (!e->IsManifold())
        return false;
    SmEdgeuse* eu = e->GetPrimaryEdgeuse();
    SmEdgeuse* rad = eu ? eu->GetRadial() : nullptr;
    SmFace* a = eu ? eu->GetFace() : nullptr;
    SmFace* b = rad ? rad->GetFace() : nullptr;
    if (!a || !b)
        return false; // not a case of this damage (the whole fixtures never get here)
    return a->GetSurface() != b->GetSurface() && (!a->GetSurface() || !b->GetSurface());
}

// Whether the edit of the target reaches the region of shell `shell_index` on an undamaged
// copy of the B-rep (`shells[shell_index]` is the target's own shell and the edit adds or
// removes a shell).
bool ub_regionless_shell(int32_t op, const char* path, const double* in, int32_t editing, int64_t target,
                         int64_t shell_index)
{
    Rd rd{in, 0};
    SmStatus st0;
    Built clean;
    clean.brep = build(rd, path, st0);
    if (!clean.brep)
        return false;
    Graph c;
    walk(clean.brep, c, true);
    clean.brep->m_bEditingEnabled = editing ? TRUE : FALSE;
    SmShell* own = nullptr;
    if (op == 0)
    {
        SmEdge* e = c.edges[(size_t)target];
        if (!e->IsWire())
            return false;
        SmEdgeuse* eu = e->GetPrimaryEdgeuse();
        own = eu ? eu->GetShell() : nullptr;
    }
    else
    {
        SmVertex* v = c.vertices[(size_t)target];
        if (!v->IsShellVertex())
            return false;
        std::vector<SmTopology*> vus = list_of(v->GetList());
        if (vus.empty())
            return false;
        SmTopology* holder = ((SmVertexuse*)vus[0])->*VuPeek::owner();
        own = (SmShell*)holder;
    }
    if (!own || own != c.shells[(size_t)shell_index])
        return false;
    size_t before = c.shells.size();
    if (op == 0)
        clean.brep->DeleteEdge(c.edges[(size_t)target], nullptr);
    else
        clean.brep->DeleteVertex(c.vertices[(size_t)target]);
    Graph after;
    walk(clean.brep, after, true);
    return after.shells.size() != before;
}

} // namespace

SC_EXPORT int32_t sc_topology_brep_p4_eval(int32_t op, const char* path, const double* in, int64_t n_in,
                                           int32_t editing, int64_t target, int64_t keep, double tol,
                                           int32_t flag, int32_t damage, int64_t damage_index, double* out,
                                           int64_t cap, int64_t* out_len)
{
    if (!path || !in || n_in < 1 || !out_len || op < 0 || op > 5 || damage < 0 || damage > 2)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    SmStatus status;
    Built built;
    built.brep = build(rd, path, status);
    std::vector<double> o;
    if (!built.brep)
    {
        o.push_back(-1.0);
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    SmBrep* brep = built.brep;
    Graph g;
    walk(brep, g, true);
    tag_objects(g);
    brep->m_bEditingEnabled = editing ? TRUE : FALSE;
    SmFace* keep_face = (keep >= 0 && keep < (int64_t)g.faces.size()) ? g.faces[(size_t)keep] : nullptr;
    if (keep >= 0 && !keep_face)
        return SC_ERR_ARG;
    // The cleared owner of a damaged shell is put back before the B-rep is deleted (the
    // destructors of the region and of the shell expect it), when the shell is still there.
    SmShell* damaged_shell = nullptr;
    SmOwningTopology* shell_owner = nullptr;
    auto repair_shell = [&](const Graph& walked) {
        if (damaged_shell && std::find(walked.shells.begin(), walked.shells.end(), damaged_shell) != walked.shells.end())
            damaged_shell->*TopPeek::list_owner() = shell_owner;
        damaged_shell = nullptr;
    };
    if (damage != 0)
    {
        // the Rust wrapper allows the damages on the ops they are probed for (see above)
        bool edge_op = op == 0, vertex_op = op == 1;
        if (damage == 1 && (!(op == 0 || op == 5) || damage_index < 0 || damage_index >= (int64_t)g.faces.size()))
            return SC_ERR_ARG;
        if (damage == 2 && (!(edge_op || vertex_op || op == 5) || damage_index < 0 ||
                            damage_index >= (int64_t)g.shells.size()))
            return SC_ERR_ARG;
        if ((edge_op && (target < 0 || target >= (int64_t)g.edges.size())) ||
            (vertex_op && (target < 0 || target >= (int64_t)g.vertices.size())))
            return SC_ERR_ARG;
        if (damage == 1)
            g.faces[(size_t)damage_index]->*FPeek::srf() = nullptr;
        else
        {
            damaged_shell = g.shells[(size_t)damage_index];
            shell_owner = damaged_shell->*TopPeek::list_owner();
            damaged_shell->*TopPeek::list_owner() = nullptr;
        }
        // (with editing disabled both ops stop at the first line with `SM_ERR`)
        bool ub = (damage == 1 && edge_op && editing && ub_surfaceless_edge(g, target)) ||
                  (damage == 2 && (edge_op || vertex_op) &&
                   ub_regionless_shell(op, path, in, editing, target, damage_index));
        if (ub)
        {
            repair_shell(g);
            o.push_back(ST_NULL_DEREF_GUARD);
            return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
        }
    }
    switch (op)
    {
    case 0:
        if (target < 0 || target >= (int64_t)g.edges.size())
            return SC_ERR_ARG;
        status = brep->DeleteEdge(g.edges[(size_t)target], keep_face ? keep_face->GetSurface() : nullptr);
        break;
    case 1:
        if (target < 0 || target >= (int64_t)g.vertices.size())
            return SC_ERR_ARG;
        status = brep->DeleteVertex(g.vertices[(size_t)target]);
        break;
    case 2:
        status = brep->ShrinkGeometry();
        break;
    case 3:
    {
        ULONG stitched = 0, lamina = 0;
        double vgap = 0.0, egap = 0.0;
        SmTArray<SmFace*> faces;
        if (keep_face)
            faces.Add(keep_face);
        status = brep->StitchFaces(tol, stitched, lamina, vgap, egap, keep_face ? &faces : nullptr,
                                   flag ? TRUE : FALSE);
        o.push_back(st(status));
        if (status == SM_SUCCESS)
        {
            o.push_back((double)stitched);
            o.push_back((double)lamina);
            o.push_back(vgap);
            o.push_back(egap);
        }
        break;
    }
    case 5:
        status = SM_SUCCESS;
        break;
    default:
    {
        SmContext* ctx = SM_CONST_CAST(SmContext*, &the_context());
        SmBoolean prev = ctx->GetDoingBoolean();
        delete brep;
        built.brep = nullptr;
        o.push_back(0.0);
        o.push_back(flg(ctx->GetDoingBoolean()));
        ctx->SetDoingBoolean(prev);
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    }
    if (op != 3)
        o.push_back(st(status));
    // The records follow on success and, for the edits (ops 0-2), on a failed status too: a
    // failure can come after part of the topology changed (`SER(SM_ERR)` after the edge is
    // gone), and the Rust side must have left the same records. (StitchFaces and the
    // destructor are not walked after a failure.)
    if (status == SM_SUCCESS || op <= 2)
    {
        Graph after;
        walk(brep, after, true);
        for (auto n : {after.regions.size(), after.shells.size(), after.faceuses.size(), after.faces.size(),
                       after.loopuses.size(), after.loops.size(), after.edgeuses.size(), after.edges.size(),
                       after.vertexuses.size(), after.vertices.size()})
            o.push_back((double)n);
        emit_records(brep, after, o);
        repair_shell(after);
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
