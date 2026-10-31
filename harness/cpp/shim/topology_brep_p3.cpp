// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.brep_p3`: the SmBrep members of the unit
// (source/SMLib/src/SmBrep.cpp) and SmApiBrepCopy (source/SM_API/src/SmApiQueries.cpp)
// on real SmBrep objects built by the shared topology fixture (sc_topology_fixture.h),
// built exactly as the topology.brep_p2 shim builds it (its Export gives the entity
// counts the selectors are resolved on). The ops run on the B-rep's real geometry:
// analytic surfaces and curves are not replaced by B-spline copies.
//
// One entry point, sc_topology_brep_p3_eval(op, path, in, n_in, out, cap, out_len).
// `in` is the fixture's B-rep block, then `editing` (-1 keep, 0/1 set
// m_bEditingEnabled), then the op's arguments; layouts are documented in
// crates/solidcore-ffi/src/units/topology_brep_p3.rs, which validates every input.
// Outputs start with `0` (built) or `-1 status` (the build failed). Entities are named
// by their index in the fixture's walk of the B-rep before the op (the B-rep's face,
// edge and vertex lists registered first). After an op that may change the topology,
// only pointers captured before it that the op cannot delete are read (faces and
// edges for Orient / TurnToNURBS, which delete none); the counts come from a fresh
// walk.
#include "sc_topology_fixture.h"
#include "sc_tracked.h"

#include <SmApiQueries.h>
#include <SmAttribute.h>
#include <SmOffsetSurface.h>

#include <algorithm>
#include <array>
#include <memory>
#include <set>
#include <stdexcept>

namespace
{
enum P3Op : int32_t
{
    P3_CONSTRUCT = 2,
    P3_COPY = 3,
    P3_MERGE = 4,
    P3_COPY_VERTICES = 5,
    P3_DELETE_FACE = 6,
    P3_WIRE = 7,
    P3_ORIENT = 8,
    P3_SQUEEZE = 9,
    P3_TURN_NURBS = 10,
    P3_API_COPY_NULL = 11,
    P3_TURN_NURBS_OFFSET = 12,
    P3_ATTRIBUTED_COPY = 13,
    P3_ATTRIBUTED_MERGE = 14,
};

struct BrepPeek : SmBrep
{
    static double SmBrep::*mse() { return &BrepPeek::m_dThisModelSizeEstimate; }
    static double SmBrep::*lsr() { return &BrepPeek::m_dThisLargeSmallSizeRatio; }
};

struct RegPeek : SmRegion
{
    static SmBoolean SmRegion::*void_flag() { return &RegPeek::m_bIsVoidFlag; }
};

struct ShellPeek : SmShell
{
    static SM_TYPE SmShell::*shell_type() { return &ShellPeek::m_tShellType; }
};

template <class T> T* at(std::vector<T*>& v, int64_t i)
{
    return (i >= 0 && i < (int64_t)v.size()) ? v[(size_t)i] : nullptr;
}

// The fixture B-rep as the brep_p2 shim builds it, registered in list order and walked.
SmBrep* prepare(Rd& rd, const char* path, Graph& g, SmStatus& bst)
{
    SmBrep* brep = build(rd, path, bst);
    if (brep == nullptr)
        return nullptr;
    int64_t editing = rd.idx();
    if (editing >= 0)
        brep->m_bEditingEnabled = editing ? TRUE : FALSE;
    SmTArray<SmFace*> fs;
    SmTArray<SmEdge*> es;
    SmTArray<SmVertex*> vs;
    brep->GetFaces(fs);
    brep->GetEdges(es);
    brep->GetVertices(vs);
    for (ULONG i = 0; i < fs.GetSize(); i++)
        g.face(fs[i]);
    for (ULONG i = 0; i < es.GetSize(); i++)
        g.edge(es[i]);
    for (ULONG i = 0; i < vs.GetSize(); i++)
        g.vertex(vs[i]);
    walk(brep, g, true);
    return brep;
}

void put_graph(std::vector<double>& o, SmBrep* brep);

// `faces edges vertices regions` of a fresh walk of `brep`, then for every region (in
// list order) `void infinite nshells` and for each of its shells `type size has_head`
// (m_tShellType, m_lListSize, m_pList != NULL).
void put_counts(std::vector<double>& o, SmBrep* brep)
{
    SmTArray<SmFace*> fs;
    SmTArray<SmEdge*> es;
    SmTArray<SmVertex*> vs;
    SmTArray<SmRegion*> rs;
    brep->GetFaces(fs);
    brep->GetEdges(es);
    brep->GetVertices(vs);
    brep->GetRegions(rs);
    o.push_back((double)fs.GetSize());
    o.push_back((double)es.GetSize());
    o.push_back((double)vs.GetSize());
    o.push_back((double)rs.GetSize());
    for (ULONG i = 0; i < rs.GetSize(); i++)
    {
        SmRegion* r = rs[i];
        SmTArray<SmShell*> ss;
        r->GetShells(ss);
        o.push_back((double)(r->*RegPeek::void_flag()));
        o.push_back(r == brep->GetInfiniteRegion() ? 1.0 : 0.0);
        o.push_back((double)ss.GetSize());
        for (ULONG j = 0; j < ss.GetSize(); j++)
        {
            SmShell* sh = ss[j];
            o.push_back((double)(sh->*ShellPeek::shell_type()));
            o.push_back((double)sh->GetSize());
            o.push_back(sh->GetList() != NULL ? 1.0 : 0.0);
        }
    }
    put_graph(o, brep);
}

// The topology graph of `brep` after an op: a fresh `walk` (the fixture's first-seen order
// from the region and edge lists) names every object by its index in its kind's list,
// then every owner / adjacency link is written as such an index (`-1` null, `-3` an
// object the walk did not reach):
//   the ten kind counts (regions shells faceuses faces loopuses loops edgeuses edges
//   vertexuses vertices), then per
//   region:    void infinite nshells shell*        (GetShells order)
//   shell:     type size region head               (head: its list's first member)
//   faceuse:   face mate shell next last orient nloopuses loopuse*
//   face:      faceuse                             (m_pFU)
//   loopuse:   loop mate faceuse type start orient
//   loop:      loopuse
//   edgeuse:   edge owner_type owner cw ccw vertexuse next last orient
//   edge:      nedgeuses edgeuse*                  (its radial list from the head)
//   vertexuse: vertex owner_type owner
//   vertex:    nvertexuses vertexuse*
void put_graph(std::vector<double>& o, SmBrep* brep)
{
    Graph g;
    walk(brep, g, true);
    // (each pointer cast to its kind first, as `walk` registers it)
    auto I = [&](int k, auto p) { return (double)g.find(k, p); };
    auto T = [&](SM_TYPE t, const SmTopology* p) -> double {
        switch (t)
        {
        case SmShell_TYPE: return (double)g.find(1, (const SmShell*)p);
        case SmFaceuse_TYPE: return (double)g.find(2, (const SmFaceuse*)p);
        case SmLoopuse_TYPE: return (double)g.find(4, (const SmLoopuse*)p);
        case SmEdgeuse_TYPE: return (double)g.find(6, (const SmEdgeuse*)p);
        case SmVertexuse_TYPE: return (double)g.find(8, (const SmVertexuse*)p);
        default: return p == nullptr ? -1.0 : -4.0;
        }
    };
    for (size_t n : {g.regions.size(), g.shells.size(), g.faceuses.size(), g.faces.size(), g.loopuses.size(),
                     g.loops.size(), g.edgeuses.size(), g.edges.size(), g.vertexuses.size(), g.vertices.size()})
        o.push_back((double)n);
    for (SmRegion* r : g.regions)
    {
        std::vector<SmTopology*> ss = list_of(r->GetList());
        o.push_back((double)(r->*RegPeek::void_flag()));
        o.push_back(r == brep->GetInfiniteRegion() ? 1.0 : 0.0);
        o.push_back((double)ss.size());
        for (SmTopology* t : ss)
            o.push_back(I(1, (SmShell*)t));
    }
    for (SmShell* sh : g.shells)
    {
        SM_TYPE ty = sh->*ShellPeek::shell_type();
        o.push_back((double)ty);
        o.push_back((double)sh->GetSize());
        o.push_back(I(0, (SmRegion*)sh->GetOwner()));
        o.push_back(T(ty, sh->GetList()));
    }
    for (SmFaceuse* fu : g.faceuses)
    {
        std::vector<SmTopology*> lus = list_of(fu->GetList());
        o.push_back(I(3, fu->*FuPeek::face()));
        o.push_back(I(2, fu->*FuPeek::mate()));
        o.push_back(I(1, (SmShell*)fu->GetOwner()));
        o.push_back(I(2, (SmFaceuse*)fu->GetNext()));
        o.push_back(I(2, (SmFaceuse*)fu->GetLast()));
        o.push_back((double)fu->GetOrientation());
        o.push_back((double)lus.size());
        for (SmTopology* t : lus)
            o.push_back(I(4, (SmLoopuse*)t));
    }
    for (SmFace* f : g.faces)
        o.push_back(I(2, f->*FPeek::fu()));
    for (SmLoopuse* lu : g.loopuses)
    {
        o.push_back(I(5, lu->*LuPeek::loop()));
        o.push_back(I(4, lu->*LuPeek::mate()));
        o.push_back(I(2, (SmFaceuse*)lu->GetOwner()));
        o.push_back((double)(lu->*LuPeek::type()));
        o.push_back(T(lu->*LuPeek::type(), lu->*LuPeek::start()));
        o.push_back((double)(lu->*LuPeek::orient()));
    }
    for (SmLoop* l : g.loops)
        o.push_back(I(4, l->GetLoopuse()));
    for (SmEdgeuse* eu : g.edgeuses)
    {
        o.push_back(I(7, (SmEdge*)eu->GetOwner()));
        o.push_back((double)eu->GetEdgeuseType());
        o.push_back(T(eu->GetEdgeuseType(), eu->*EuPeek::sorlu()));
        o.push_back(I(6, eu->GetCWEdgeuse()));
        o.push_back(I(6, eu->GetCCWEdgeuse()));
        o.push_back(I(8, eu->*EuPeek::vu()));
        o.push_back(I(6, (SmEdgeuse*)eu->GetNext()));
        o.push_back(I(6, (SmEdgeuse*)eu->GetLast()));
        o.push_back((double)eu->GetOrientation());
    }
    for (SmEdge* e : g.edges)
    {
        std::vector<SmTopology*> eus = list_of(e->GetList());
        o.push_back((double)eus.size());
        for (SmTopology* t : eus)
            o.push_back(I(6, (SmEdgeuse*)t));
    }
    for (SmVertexuse* vu : g.vertexuses)
    {
        o.push_back(I(9, (SmVertex*)vu->GetOwner()));
        o.push_back((double)vu->GetVertexuseType());
        o.push_back(T(vu->GetVertexuseType(), vu->*VuPeek::owner()));
    }
    for (SmVertex* v : g.vertices)
    {
        std::vector<SmTopology*> vus = list_of(v->GetList());
        o.push_back((double)vus.size());
        for (SmTopology* t : vus)
            o.push_back(I(8, (SmVertexuse*)t));
    }
}

// P3_ATTRIBUTED_COPY: the first application attribute id (as the boolean.api_brep shim).
const ULONG ATTRIBUTE_BASE = 10007;

// Every face (GetFaces order) and edge (GetEdges order) of `b` gets its own
// SmLongAttribute, id = value = ATTRIBUTE_BASE + 500 kind + index (kind 0 face, 1 edge),
// behaviour `behavior`.
void tag_brep(SmBrep* b, SmAttributeBehaviorType behavior)
{
    const SmContext& ctx = *b->GetContext();
    SmTArray<SmFace*> faces;
    b->GetFaces(faces);
    for (ULONG i = 0; i < faces.GetSize(); i++)
        faces[i]->AddAttribute(new (ctx) SmLongAttribute(ATTRIBUTE_BASE + i, (long)(ATTRIBUTE_BASE + i), behavior));
    SmTArray<SmEdge*> edges;
    b->GetEdges(edges);
    for (ULONG i = 0; i < edges.GetSize(); i++)
        edges[i]->AddAttribute(
            new (ctx) SmLongAttribute(ATTRIBUTE_BASE + 500 + i, (long)(ATTRIBUTE_BASE + 500 + i), behavior));
}

// `faces rows... edges rows...` of `b`: per object `n` then per attribute (ascending)
// `id behavior value users`, users counted among `b`'s faces and edges (the port keeps
// one attribute store per B-rep); the rows sorted.
void put_attribute_rows(std::vector<double>& o, SmBrep* b)
{
    SmTArray<SmFace*> faces;
    b->GetFaces(faces);
    SmTArray<SmEdge*> edges;
    b->GetEdges(edges);
    std::set<const SmAObject*> scope;
    for (ULONG i = 0; i < faces.GetSize(); i++)
        scope.insert(faces[i]);
    for (ULONG i = 0; i < edges.GetSize(); i++)
        scope.insert(edges[i]);
    auto row = [&](const SmAObject* obj) {
        SmTArray<SmAttribute*> attrs;
        obj->GetAttributes(attrs);
        std::vector<std::vector<double>> entries;
        for (ULONG i = 0; i < attrs.GetSize(); i++)
        {
            SmLongAttribute* l = dynamic_cast<SmLongAttribute*>(attrs[i]);
            SmTArray<SmAObject*> users;
            attrs[i]->GetUsers(users);
            double n = 0.0;
            for (ULONG u = 0; u < users.GetSize(); u++)
                if (scope.count(users[u]) != 0)
                    n += 1.0;
            entries.push_back({(double)attrs[i]->GetAttributeID(), (double)attrs[i]->GetBehavior(),
                               l != NULL ? (double)l->GetValue() : 0.0, n});
        }
        std::sort(entries.begin(), entries.end());
        std::vector<double> r;
        r.push_back((double)entries.size());
        for (auto& e : entries)
            r.insert(r.end(), e.begin(), e.end());
        return r;
    };
    for (int kind = 0; kind < 2; kind++)
    {
        std::vector<std::vector<double>> rows;
        if (kind == 0)
            for (ULONG i = 0; i < faces.GetSize(); i++)
                rows.push_back(row(faces[i]));
        else
            for (ULONG i = 0; i < edges.GetSize(); i++)
                rows.push_back(row(edges[i]));
        std::sort(rows.begin(), rows.end());
        o.push_back((double)rows.size());
        for (auto& r : rows)
            o.insert(o.end(), r.begin(), r.end());
    }
}

void run(int32_t op, Rd& rd, const char* path, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    if (op == P3_CONSTRUCT)
    {
        // no fixture: a new B-rep with the given size estimate and ratio, in a context of
        // its own (the constructor passes the estimate and ratio to its context)
        double mse = rd.num(), lsr = rd.num();
        SmContext* own = new SmContext();
        SmBrep* b = new (*own) SmBrep(mse, lsr);
        o.push_back(0.0);
        o.push_back(0.0);
        o.push_back((double)b->GetTolerance());
        o.push_back(b->*BrepPeek::mse());
        o.push_back(b->*BrepPeek::lsr());
        o.push_back(flg(b->m_bEditingEnabled));
        SmRegion* inf = b->GetInfiniteRegion();
        o.push_back(inf ? flg(inf->*RegPeek::void_flag()) : -1.0);
        put_counts(o, b);
        delete b;
        delete own;
        return;
    }
    if (op == P3_API_COPY_NULL)
    {
        SmBrep* r = nullptr;
        o.push_back(0.0);
        o.push_back(st((SmStatus)SmApiBrepCopy(NULL, r)));
        delete r;
        return;
    }
    SmStatus bst;
    Built b;
    Graph g;
    b.brep = prepare(rd, path, g, bst);
    if (b.brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(bst));
        return;
    }
    SmBrep* brep = b.brep;
    o.push_back(0.0);
    // the edits of `brep` run under a topology change tracker (SmHealData's), whose
    // lists and events are compared after the op's outputs
    TrackedEdit te; // (after b: goes before it)
    if (op == P3_DELETE_FACE || op == P3_WIRE || op == P3_ORIENT || op == P3_SQUEEZE || op == P3_TURN_NURBS ||
        op == P3_TURN_NURBS_OFFSET)
        te.begin(brep, g);
    switch (op)
    {
    case P3_COPY:
    {
        SmBoolean analytics = rd.bflag();
        bool api = rd.flag();
        SmBrep* c = nullptr;
        SmStatus s = SM_SUCCESS;
        if (api)
            s = (SmStatus)SmApiBrepCopy(brep, c);
        else
            c = new (ctx) SmBrep(*brep, NULL, NULL, TRUE, analytics);
        o.push_back(st(s));
        o.push_back((double)c->GetTolerance());
        o.push_back(flg(c->m_bEditingEnabled));
        put_counts(o, c);
        delete c;
        break;
    }
    case P3_ATTRIBUTED_COPY:
    {
        // the B-rep's faces and edges tagged (tag_brep), then SmApiBrepCopy: `status tol
        // editing counts`, then the copy's and the source's attribute rows
        SmAttributeBehaviorType behavior = (SmAttributeBehaviorType)rd.idx();
        tag_brep(brep, behavior);
        SmBrep* c = nullptr;
        SmStatus s = (SmStatus)SmApiBrepCopy(brep, c);
        o.push_back(st(s));
        o.push_back((double)c->GetTolerance());
        o.push_back(flg(c->m_bEditingEnabled));
        put_counts(o, c);
        put_attribute_rows(o, c);
        put_attribute_rows(o, brep);
        delete c;
        break;
    }
    case P3_ATTRIBUTED_MERGE:
    {
        // the B-rep's faces and edges tagged (tag_brep), then MergeBrep of it into a second
        // build of it: `status counts`, then the second build's and the source's attribute rows
        SmBoolean analytics = rd.bflag();
        SmAttributeBehaviorType behavior = (SmAttributeBehaviorType)rd.idx();
        tag_brep(brep, behavior);
        Rd rd2{rd.in, 0};
        SmStatus bst2;
        Built b2;
        Graph g2;
        b2.brep = prepare(rd2, path, g2, bst2);
        if (b2.brep == nullptr)
        {
            o.push_back(st(bst2));
            break;
        }
        o.push_back(st(b2.brep->MergeBrep(*brep, NULL, NULL, analytics)));
        put_counts(o, b2.brep);
        put_attribute_rows(o, b2.brep);
        put_attribute_rows(o, brep);
        break;
    }
    case P3_MERGE:
    {
        // merge the fixture B-rep into a second build of it
        SmBoolean analytics = rd.bflag();
        Rd rd2{rd.in, 0};
        SmStatus bst2;
        Built b2;
        Graph g2;
        b2.brep = prepare(rd2, path, g2, bst2);
        if (b2.brep == nullptr)
        {
            o.push_back(st(bst2));
            break;
        }
        o.push_back(st(b2.brep->MergeBrep(*brep, NULL, NULL, analytics)));
        put_counts(o, b2.brep);
        break;
    }
    case P3_COPY_VERTICES:
    {
        int64_t n = rd.idx();
        SmTArray<SmVertex*> vs;
        for (int64_t i = 0; i < n; i++)
            vs.Add(at(g.vertices, rd.idx()));
        Rd rd2{rd.in, 0};
        SmStatus bst2;
        Built b2;
        Graph g2;
        b2.brep = prepare(rd2, path, g2, bst2);
        if (b2.brep == nullptr)
        {
            o.push_back(st(bst2));
            break;
        }
        std::vector<double> geometry;
        auto points = [&](const std::vector<SmVertex*>& vertices) {
            geometry.push_back((double)vertices.size());
            for (SmVertex* v : vertices)
                put_vec(geometry, v->GetPoint());
        };
        points(g.vertices);
        points(g2.vertices);
        SmTArray<SmVertex*> nv;
        o.push_back(st(brep->CopyVertices(vs, b2.brep, &nv)));
        o.push_back((double)nv.GetSize());
        put_counts(o, b2.brep);
        points(g.vertices);
        points(g2.vertices);
        Graph post;
        walk(b2.brep, post, true);
        geometry.push_back((double)nv.GetSize());
        for (ULONG i = 0; i < nv.GetSize(); ++i)
        {
            geometry.push_back((double)post.find(9, nv[i]));
            put_vec(geometry, nv[i]->GetPoint());
        }
        o.insert(o.end(), geometry.begin(), geometry.end());
        break;
    }
    case P3_DELETE_FACE:
    {
        SmFace* f = at(g.faces, rd.idx());
        SmBoolean del = rd.bflag();
        SmBoolean nest = rd.bflag();
        SmRegion* keep = at(g.regions, rd.idx());
        SmTArray<SmTopology*> gone;
        o.push_back(st(brep->DeleteFace(f, del, nest, &gone, keep)));
        o.push_back((double)gone.GetSize());
        put_counts(o, brep);
        break;
    }
    case P3_WIRE:
    {
        int64_t which = rd.idx();
        SmRegion* r = at(g.regions, rd.idx());
        SmVertex* v1 = at(g.vertices, rd.idx());
        SmVertex* v2 = at(g.vertices, rd.idx());
        SmBSplineCurve* c = read_curve(rd, ctx);
        double t0 = rd.num(), t1 = rd.num();
        SmOrientType orient = (SmOrientType)rd.idx();
        SmVector3d end = rd.vec();
        SmEdge* e = nullptr;
        SmVertex* v = nullptr;
        SmStatus s;
        if (which == 0)
            s = brep->MakeWireEdge(r, v1, v2, c, SmExtent1d(t0, t1), orient, e);
        else
            s = brep->MakeWireEdgeVertex(r, v1, c, SmExtent1d(t0, t1), orient, end, e, v);
        // the curve belongs to the B-rep once an edge took it (also on a later failure)
        bool bound = false;
        {
            SmTArray<SmEdge*> es;
            brep->GetEdges(es);
            for (ULONG i = 0; i < es.GetSize(); i++)
                bound = bound || es[i]->GetCurve() == (SmCurve*)c;
        }
        if (!bound)
            delete c;
        o.push_back(st(s));
        put_counts(o, brep);
        break;
    }
    case P3_ORIENT:
    {
        SmBoolean outward = rd.bflag();
        SmBoolean unify = rd.bflag();
        int64_t n = rd.idx();
        bool has_list = n >= 0;
        SmTArray<SmFace*> swap;
        for (int64_t i = 0; i < n; i++)
            swap.Add(at(g.faces, rd.idx()));
        SmBoolean maybe = FALSE;
        SmStatus s = brep->OrientTrimmedSurfaces(outward, maybe, unify, has_list ? &swap : NULL);
        o.push_back(st(s));
        o.push_back(flg(maybe));
        // Orient deletes nothing: the faces registered before the op are live
        for (SmFace* f : g.faces)
        {
            SmExtent2d d = f->GetUVDomain();
            o.push_back(d.GetMin().x);
            o.push_back(d.GetMin().y);
            o.push_back(d.GetMax().x);
            o.push_back(d.GetMax().y);
            SmFaceuse* fu = f->GetUpwardFaceuse();
            o.push_back((double)g.find(2, fu));
        }
        put_counts(o, brep);
        break;
    }
    case P3_SQUEEZE:
    {
        SmEdge* e = at(g.edges, rd.idx());
        int64_t which = rd.idx();
        SmVertex* any = at(g.vertices, rd.idx());
        SmVertex* v = which == 0 ? e->GetVertex() : which == 1 ? e->GetOtherVertex(e->GetVertex()) : any;
        o.push_back(st(brep->SqueezeEdge(e, v)));
        put_counts(o, brep);
        break;
    }
    case P3_TURN_NURBS_OFFSET:
    case P3_TURN_NURBS:
    {
        if (op == P3_TURN_NURBS_OFFSET)
        {
            // every face-owned B-spline kind surface becomes an SmOffsetSurface of it by
            // `d` (owning its base) before the call, so TurnToNURBS' offset arm runs; the
            // base is detached from its face first, so the constructor's IsPeriodic /
            // GetSingularities use the context tolerance (not SmFace::GetTolerance)
            double d = rd.num();
            for (SmFace* f : g.faces)
            {
                SmSurface* s = f->*FPeek::srf();
                if (s == nullptr || s->GetOwner() != (SmObject*)f || !s->IsKindOf(SmBSplineSurface_TYPE))
                    continue;
                s->SetOwner(NULL);
                SmOffsetSurface* off = new (the_context()) SmOffsetSurface(d, *s, TRUE);
                off->SetOwner(f);
                f->*FPeek::srf() = off;
            }
        }
        o.push_back(st(brep->TurnToNURBS()));
        // TurnToNURBS deletes no face or edge
        for (SmEdge* e : g.edges)
            put_curve(o, e->GetCurve());
        for (SmFace* f : g.faces)
        {
            SmSurface* s = f->GetSurface();
            o.push_back(s ? (double)s->GetType() : -1.0);
            if (op == P3_TURN_NURBS_OFFSET && s != nullptr)
            {
                SmExtent2d d = s->GetNaturalUVDomain();
                o.push_back(d.GetMin().x);
                o.push_back(d.GetMin().y);
                o.push_back(d.GetMax().x);
                o.push_back(d.GetMax().y);
            }
        }
        put_counts(o, brep);
        break;
    }
    default: o.push_back(-9.0); break;
    }
    if (te.trk)
    {
        const size_t tracker_at = te.finish(o);
        // the edges, edge uses, faces and shells the op destroyed (their destructors'
        // SM_NO_DESTRUCTION), after the tracker
        if (op == P3_DELETE_FACE)
            for (int64_t n : te.rec.destroyed)
                o.push_back((double)n);
        if (op == P3_ORIENT)
        {
            // the trailer the runner splits on: for each face before the op the number of
            // its SmFace::SwapUV POST_EDITs among the events, the face count and where
            // the tracker's values start
            SmTArray<SmNotifyEvent>* ev = te.trk->GetNotifyEvents();
            for (SmFace* f : g.faces)
            {
                int64_t swaps = 0;
                for (ULONG i = 0; i < ev->GetSize(); i++)
                    if ((*ev)[i].m_eAction == SM_NO_POST_EDIT && (*ev)[i].m_pCaller == (SmObject*)f)
                        swaps++;
                o.push_back((double)swaps);
            }
            o.push_back((double)g.faces.size());
            o.push_back((double)tracker_at);
        }
    }
}
} // namespace

SC_EXPORT int32_t sc_topology_brep_p3_eval(int32_t op, const char* path, const double* in, int64_t n_in,
                                           double* out, int64_t cap, int64_t* out_len)
{
    if (in == nullptr || n_in < 1 || out_len == nullptr || path == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    run(op, rd, path, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
