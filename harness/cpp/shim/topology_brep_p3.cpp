// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.brep_p3`: the SmBrep members of the unit
// (source/SMLib/src/SmBrep.cpp) and SmApiBrepCopy (source/SM_API/src/SmApiQueries.cpp)
// on real SmBrep objects built by the shared topology fixture (sc_topology_fixture.h),
// built and normalised exactly as the topology.brep_p2 shim does, so that its Export
// and Extras ops describe the B-rep this shim runs on.
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

#include <SmApiQueries.h>
#include <SmAttribute.h>
#include <SmOffsetSurface.h>
#include <SmTrackTopologyChanges.h>

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
};

struct EdgePeek : SmEdge
{
    static SmCurve* SmEdge::*curve() { return &EdgePeek::m_pCurve; }
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

// A plain SmBSplineCurve of `c`'s NURB, or nullptr when `c` already is one (or a line)
// or has no NURB (as the topology.brep_p2 shim).
SmBSplineCurve* plain_curve(SmCurve* c)
{
    if (c == nullptr || !c->IsKindOf(SmBSplineCurve_TYPE) || c->GetType() == SmBSplineCurve_TYPE ||
        c->GetType() == SmLine_TYPE)
        return nullptr;
    SmBSplineCurve* b = (SmBSplineCurve*)c;
    gw_CURVE* nurb = b->GetGwNurbPointer();
    if (nurb == nullptr)
        return nullptr;
    return new (the_context()) SmBSplineCurve(sm_AllocateAndCopyNurbCurve(nurb), b->GetDim(), FALSE);
}

// Gives every analytic face and edge a plain B-spline copy owned by it (the
// topology.brep_p2 shim's normalisation, so that the Rust import of its export is exact).
void normalise(Graph& g)
{
    for (SmFace* f : g.faces)
    {
        SmSurface* s = f->*FPeek::srf();
        if (s == nullptr || !s->IsKindOf(SmBSplineSurface_TYPE) || s->GetType() == SmBSplineSurface_TYPE ||
            s->GetType() == SmPlane_TYPE)
            continue;
        SmBSplineSurface* b = (SmBSplineSurface*)s;
        gw_SURFACE* nurb = b->GetGwNurbPointer();
        if (nurb == nullptr)
            continue;
        SmBSplineSurface* nb = new (the_context()) SmBSplineSurface(sm_AllocateAndCopyNurbSurface(nurb), FALSE, NULL);
        nb->GetOutOfBoundsEnabled() = b->GetOutOfBoundsEnabled();
        nb->SetBSplineSurfaceForm(b->GetBSplineSurfaceForm());
        if (s->GetOwner() == (SmObject*)f)
            nb->SetOwner(f);
        f->*FPeek::srf() = nb;
    }
    for (SmEdge* e : g.edges)
    {
        SmCurve* c = e->*EdgePeek::curve();
        SmBSplineCurve* nc = plain_curve(c);
        if (nc == nullptr)
            continue;
        if (c->GetOwner() == (SmObject*)e)
            nc->SetOwner(e);
        e->*EdgePeek::curve() = nc;
    }
}

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
    normalise(g);
    return brep;
}

void put_graph(std::vector<double>& o, SmBrep* brep);

// The objects a walk registered, by their SmObject base address (an upcast from each
// object's own type), so that a saved tracker pointer is compared without a cast or a
// read: kind (the Graph's 0..9) and index.
std::map<const void*, std::pair<int, int64_t>> by_base(Graph& g)
{
    std::map<const void*, std::pair<int, int64_t>> m;
    auto add = [&](int k, auto& v) {
        for (size_t i = 0; i < v.size(); i++)
            m.emplace((const void*)(SmObject*)v[i], std::make_pair(k, (int64_t)i));
    };
    add(0, g.regions);
    add(1, g.shells);
    add(2, g.faceuses);
    add(3, g.faces);
    add(4, g.loopuses);
    add(5, g.loops);
    add(6, g.edgeuses);
    add(7, g.edges);
    add(8, g.vertexuses);
    add(9, g.vertices);
    return m;
}

// An object by its SmObject address and that address's generation: the number of
// SM_NO_DESTRUCTION notifications of the address seen before. An address freed during
// the op can be reused by a new object (whether it is depends on the process's heap,
// which the Rust side shares); the generation tells the two objects apart.
struct Ref
{
    const void* a = nullptr;
    int64_t gen = 0;
    bool operator<(const Ref& o) const { return a != o.a ? a < o.a : gen < o.gen; }
};

// The user notify callback of an edit. SmObject::Notify calls it before the system
// callback (the tracker's) for every notification, so it resolves each pointer a
// notification names while the object is live: every geometry object (not topology,
// not the B-rep) is recorded as geometry, every pointer gets its Ref, and the events
// the tracker appends for a notification (checked to be that notification) keep the
// Refs of its pointers. An address's generation advances after its SM_NO_DESTRUCTION,
// once the tracker has handled that notification.
struct GeomRecorder : SmNotifyCallback
{
    SmBrep* brep = nullptr;
    std::map<Ref, int64_t> order;
    SmNotifyCallback* previous = nullptr;
    SmTrackTopologyChanges* trk = nullptr;
    std::map<const void*, int64_t> gens;
    std::vector<std::array<Ref, 4>> event_refs; // per tracker event: caller, data1..3
    // the last notification (its pointers and their Refs) and the address it destroys
    bool have_last = false;
    SmNotifyOperation last_op = SM_NO_UNKNOWN;
    std::array<SmObject*, 4> last_ptrs{};
    std::array<Ref, 4> last_refs{};
    const void* pending_destruction = nullptr;
    // SM_NO_DESTRUCTION notifications by caller kind: edge, edge use, face, shell
    // (~SmEdge, ~SmEdgeuse, ~SmFace, ~SmShell)
    int64_t destroyed[4] = {0, 0, 0, 0};

    Ref ref(const void* a) const
    {
        auto g = gens.find(a);
        return Ref{a, g != gens.end() ? g->second : 0};
    }

    // Labels the tracker events appended while handling the last notification, then
    // advances the generation of the address that notification destroyed.
    void sync()
    {
        if (trk != nullptr)
        {
            SmTArray<SmNotifyEvent>* ev = trk->GetNotifyEvents();
            while (event_refs.size() < ev->GetSize())
            {
                SmNotifyEvent& e = (*ev)[(ULONG)event_refs.size()];
                if (!have_last || e.m_eAction != last_op || e.m_pCaller != last_ptrs[0] ||
                    e.m_pData1 != last_ptrs[1] || e.m_pData2 != last_ptrs[2] || e.m_pData3 != last_ptrs[3])
                    throw std::runtime_error("brep_p3 tracker event is not the last notification");
                event_refs.push_back(last_refs);
            }
        }
        if (pending_destruction != nullptr)
        {
            gens[pending_destruction]++;
            pending_destruction = nullptr;
        }
    }

    SmStatus Execute(SmNotifyOperation op, SmObject* caller, SmObject* d1, SmObject* d2, SmObject* d3) override
    {
        sync();
        last_op = op;
        last_ptrs = {caller, d1, d2, d3};
        for (size_t i = 0; i < 4; i++)
        {
            SmObject* p = last_ptrs[i];
            last_refs[i] = ref((const void*)p);
            if (p != nullptr && p != (SmObject*)brep && !p->IsKindOf(SmTopology_TYPE) &&
                order.find(last_refs[i]) == order.end())
                order.emplace(last_refs[i], (int64_t)order.size());
        }
        have_last = true;
        if (op == SM_NO_DESTRUCTION && caller != nullptr)
            pending_destruction = (const void*)caller;
        if (op == SM_NO_DESTRUCTION && caller != nullptr)
        {
            if (caller->IsKindOf(SmEdge_TYPE))
                destroyed[0]++;
            if (caller->IsKindOf(SmEdgeuse_TYPE))
                destroyed[1]++;
            if (caller->IsKindOf(SmFace_TYPE))
                destroyed[2]++;
            if (caller->IsKindOf(SmShell_TYPE))
                destroyed[3]++;
        }
        // (a user callback installed before this one still sees every notification)
        return previous != nullptr ? previous->Execute(op, caller, d1, d2, d3) : SM_SUCCESS;
    }
};

// Installs a user notify callback in a context and puts the previous one back on every
// exit (an exception included). Declared after the recorder and the Built B-rep, it is
// destroyed first: the B-rep's destructor notifies through whatever is installed.
struct UserCallbackGuard
{
    SmContext* ctx = nullptr;
    SmNotifyCallback* previous = nullptr;
    void install(SmContext& c, SmNotifyCallback* cb)
    {
        ctx = &c;
        previous = c.GetUserNotifyCallback();
        c.SetUserNotifyCallback(cb);
    }
    ~UserCallbackGuard()
    {
        if (ctx != nullptr)
            ctx->SetUserNotifyCallback(previous);
    }
};

// An object a tracker list or event names, by its Ref (it may be deleted by now; see
// GeomRecorder): `-1 -1` null, `-10 0` the B-rep; a topology object either walk
// registered: its kind's type code, then for a face / edge / vertex its index before
// the op (or 100000 + its index in the walk after it), for another kind `-5`; the
// curve an edge had before the op: `-20 edge`; else the curve an edge has after it:
// `-21 edge` (its index in the walk after the op); other geometry: `-30 n`, its first-seen
// order in the tracker's lists and events (seen live by GeomRecorder); anything else
// `-2 -5`. The objects before the op are generation 0 of their address and those after
// it the address's last generation, so an object made at the address of one destroyed
// during the op is never taken for it.
struct Tracked
{
    std::map<const void*, std::pair<int, int64_t>> pre, post;
    std::map<const void*, int64_t> pre_curves, post_curves;
    const GeomRecorder* rec = nullptr;
    std::map<Ref, int64_t> numbered; // geometry by first appearance in the output
    SmBrep* brep = nullptr;
};

void put_tracked(std::vector<double>& o, SmObject* p, Tracked& t, Ref r)
{
    static const double codes[10] = {SmRegion_TYPE,   SmShell_TYPE, SmFaceuse_TYPE,   SmFace_TYPE,
                                     SmLoopuse_TYPE,  SmLoop_TYPE,  SmEdgeuse_TYPE,   SmEdge_TYPE,
                                     SmVertexuse_TYPE, SmVertex_TYPE};
    const void* a = (const void*)p;
    if (p == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(-1.0);
        return;
    }
    // (before the op: generation 0; after it: the address's last generation)
    const bool before = r.gen == 0;
    const bool after = r.gen == t.rec->ref(a).gen;    if (p == (SmObject*)t.brep)
    {
        o.push_back(-10.0);
        o.push_back(0.0);
        return;
    }
    auto i = before ? t.pre.find(a) : t.pre.end();
    auto j = after ? t.post.find(a) : t.post.end();
    if (i != t.pre.end() || j != t.post.end())
    {
        int k = i != t.pre.end() ? i->second.first : j->second.first;
        o.push_back(codes[k]);
        if (k == 3 || k == 7 || k == 9)
            o.push_back(i != t.pre.end() ? (double)i->second.second : 100000.0 + (double)j->second.second);
        else
            o.push_back(-5.0);
        return;
    }
    auto c = before ? t.pre_curves.find(a) : t.pre_curves.end();
    if (c != t.pre_curves.end())
    {
        o.push_back(-20.0);
        o.push_back((double)c->second);
        return;
    }
    auto pc = after ? t.post_curves.find(a) : t.post_curves.end();
    if (pc != t.post_curves.end())
    {
        o.push_back(-21.0);
        o.push_back((double)pc->second);
        return;
    }
    if (t.rec->order.find(r) != t.rec->order.end())
    {
        auto n = t.numbered.emplace(r, (int64_t)t.numbered.size()).first;
        o.push_back(-30.0);
        o.push_back((double)n->second);
        return;
    }
    o.push_back(-2.0);
    o.push_back(-5.0);
}

// The tracker of an edit (SmTrackTopologyChanges on the B-rep): `n rm*`, `n add*`,
// `n chg*`, then `n` events of `action caller data1 data2 data3` (put_tracked each).
// The events use the Refs the recorder gave their pointers when they happened. The add
// and change lists hold live objects (the tracker drops a destroyed one), so they use
// each address's last generation; a removed object resolves as the object before the op
// at its address when there was one (generation 0), else as the last one there.
void put_tracker(std::vector<double>& o, SmTrackTopologyChanges& trk, Tracked& t, GeomRecorder& rec)
{
    rec.sync();
    Graph post;
    walk(t.brep, post, true);
    t.post = by_base(post);
    // (the live curves of the B-rep after the op, by their edge)
    for (size_t i = 0; i < post.edges.size(); i++)
        if (SmCurve* c = post.edges[i]->GetCurve())
            t.post_curves.emplace((const void*)(SmObject*)c, (int64_t)i);
    bool removed = true;
    for (SmTArray<SmTopology*>* l : {trk.GetRmList(), trk.GetAddList(), trk.GetChgList()})
    {
        o.push_back((double)l->GetSize());
        for (ULONG i = 0; i < l->GetSize(); i++)
        {
            SmObject* p = (SmObject*)(*l)[i]; // (SmTopology* up to SmObject*)
            Ref r = rec.ref((const void*)p);
            if (removed && t.pre.find((const void*)p) != t.pre.end())
                r.gen = 0;
            put_tracked(o, p, t, r);
        }
        removed = false;
    }
    SmTArray<SmNotifyEvent>* ev = trk.GetNotifyEvents();
    if (rec.event_refs.size() != ev->GetSize())
        throw std::runtime_error("brep_p3 tracker events not all labelled");
    o.push_back((double)ev->GetSize());
    for (ULONG i = 0; i < ev->GetSize(); i++)
    {
        SmNotifyEvent& e = (*ev)[i];
        const std::array<Ref, 4>& r = rec.event_refs[i];
        o.push_back((double)e.m_eAction);
        put_tracked(o, e.m_pCaller, t, r[0]);
        put_tracked(o, e.m_pData1, t, r[1]);
        put_tracked(o, e.m_pData2, t, r[2]);
        put_tracked(o, e.m_pData3, t, r[3]);
    }
}

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
    std::unique_ptr<SmTrackTopologyChanges> trk;
    GeomRecorder rec;
    UserCallbackGuard guard; // (after rec and b: restores before either goes)
    Tracked tracked;
    // (the tracker attaches to the B-rep's context; the recorder goes beside it)
    SmContext& ctx_ = const_cast<SmContext&>(*brep->GetContext());
    if (op == P3_DELETE_FACE || op == P3_WIRE || op == P3_ORIENT || op == P3_SQUEEZE || op == P3_TURN_NURBS ||
        op == P3_TURN_NURBS_OFFSET)
    {
        tracked.brep = brep;
        tracked.pre = by_base(g);
        for (size_t i = 0; i < g.edges.size(); i++)
            if (SmCurve* c = g.edges[i]->GetCurve())
                tracked.pre_curves.emplace((const void*)(SmObject*)c, (int64_t)i);
        tracked.rec = &rec;
        rec.brep = brep;
        rec.previous = ctx_.GetUserNotifyCallback();
        guard.install(ctx_, &rec);
        trk.reset(new SmTrackTopologyChanges(brep, TRUE));
        rec.trk = trk.get();
    }
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
    if (trk)
    {
        trk->StopTracking();
        const size_t tracker_at = o.size();
        put_tracker(o, *trk, tracked, rec);
        // the edges, edge uses, faces and shells the op destroyed (their destructors'
        // SM_NO_DESTRUCTION), after the tracker
        if (op == P3_DELETE_FACE)
            for (int64_t n : rec.destroyed)
                o.push_back((double)n);
        if (op == P3_ORIENT)
        {
            // the trailer the runner splits on: for each face before the op the number of
            // its SmFace::SwapUV POST_EDITs among the events, the face count and where
            // the tracker's values start
            SmTArray<SmNotifyEvent>* ev = trk->GetNotifyEvents();
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
