// SPDX-License-Identifier: Apache-2.0
// The topology change tracker of a B-rep edit, for the shims that compare it
// (topology.brep_p2, topology.brep_p3, topology.brep_p9): an SmTrackTopologyChanges on
// the B-rep plus a user notify callback that names every object a notification passes
// while it is live, and the tracker's lists and events written by identity only
// (put_tracker). The Rust runners mirror it in crates/difftest/src/units/topology_brep_p3.rs
// (`run_tracked`, `put_tracker`).
#pragma once

#include "sc_topology_fixture.h"

#include <SmTrackTopologyChanges.h>

#include <array>
#include <map>
#include <memory>
#include <stdexcept>
#include <vector>

namespace
{
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
                    throw std::runtime_error("tracked edit: tracker event is not the last notification");
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
        throw std::runtime_error("tracked edit: tracker events not all labelled");
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

// One tracked edit of `brep`, whose fixture walk before the op is `g`: begin() before
// the op installs the recorder in the B-rep's context and starts the tracker (as
// SmHealData does); finish() after it stops the tracker and writes its lists and
// events, returning where those values start in `o`. Declared after the B-rep's Built,
// so its members go first (the guard restores the context's callback before the
// recorder goes, and the B-rep's destructor notifies through whatever is installed).
struct TrackedEdit
{
    std::unique_ptr<SmTrackTopologyChanges> trk;
    GeomRecorder rec;
    UserCallbackGuard guard; // (after rec: restores before it goes)
    Tracked tracked;

    void begin(SmBrep* brep, Graph& g)
    {
        // (the tracker attaches to the B-rep's context; the recorder goes beside it)
        SmContext& ctx = const_cast<SmContext&>(*brep->GetContext());
        tracked.brep = brep;
        tracked.pre = by_base(g);
        for (size_t i = 0; i < g.edges.size(); i++)
            if (SmCurve* c = g.edges[i]->GetCurve())
                tracked.pre_curves.emplace((const void*)(SmObject*)c, (int64_t)i);
        tracked.rec = &rec;
        rec.brep = brep;
        rec.previous = ctx.GetUserNotifyCallback();
        guard.install(ctx, &rec);
        trk.reset(new SmTrackTopologyChanges(brep, TRUE));
        rec.trk = trk.get();
    }

    size_t finish(std::vector<double>& o)
    {
        trk->StopTracking();
        const size_t at = o.size();
        put_tracker(o, *trk, tracked, rec);
        return at;
    }
};
} // namespace
