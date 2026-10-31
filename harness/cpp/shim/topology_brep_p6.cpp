// SPDX-License-Identifier: Apache-2.0
// SmBrep p6 on actual SMLib objects. Raw fixtures share the existing topology
// protocol; operation results are never used to construct the Rust input graph.
#include "sc_topology_fixture.h"
#include <memory>

namespace post
{
// A post-operation traversal and export of the whole B-rep (the walk of the
// topology.brep_p10 shim, without SmTopology flags and curve ownership), as
// (tag, value) pairs: tag 0 an integer, tag 1 a double.
struct Out
{
    std::vector<double>& v;
    void i(int64_t x)
    {
        v.push_back(0.0);
        v.push_back((double)x);
    }
    void d(double x)
    {
        v.push_back(1.0);
        v.push_back(x);
    }
};

struct EuP : SmEdgeuse
{
    static SmTopology* SmEdgeuse::*sorlu() { return &EuP::m_pSorLU; }
    static SmBSplineCurve* SmEdgeuse::*uv() { return &EuP::m_pUVTrimCurve; }
    static SmVertexuse* SmEdgeuse::*vu() { return &EuP::m_pVU; }
};
struct FuP : SmFaceuse
{
    static SmFace* SmFaceuse::*face() { return &FuP::m_pF; }
    static SmFaceuse* SmFaceuse::*mate() { return &FuP::m_pFUMate; }
};
struct LuP : SmLoopuse
{
    static SmLoop* SmLoopuse::*loop() { return &LuP::m_pL; }
    static SmLoopuse* SmLoopuse::*mate() { return &LuP::m_pLUMate; }
    static SM_TYPE SmLoopuse::*type() { return &LuP::m_tLoopuseType; }
    static SmTopology* SmLoopuse::*start() { return &LuP::m_pEUorVU; }
    static SmOrientType SmLoopuse::*orient() { return &LuP::m_eOrientation; }
};
struct VuP : SmVertexuse
{
    static SmTopology* SmVertexuse::*owner() { return &VuP::m_pSorLUorEU; }
};
struct FP : SmFace
{
    static SmFaceuse* SmFace::*fu() { return &FP::m_pFU; }
    static SmExtent2d SmFace::*dom() { return &FP::m_vUVDomain; }
    static SmBoolean SmFace::*rect() { return &FP::m_bRectangularTrim; }
    static SmSurface* SmFace::*srf() { return &FP::m_pSurface; }
    static SmZoneTol3d SmFace::*tol() { return &FP::m_sZoneTol3d; }
};
struct ShP : SmShell
{
    static SM_TYPE SmShell::*type() { return &ShP::m_tShellType; }
};

struct G
{
    std::vector<SmRegion*> regions;
    std::vector<SmShell*> shells;
    std::vector<SmFaceuse*> faceuses;
    std::vector<SmFace*> faces;
    std::vector<SmLoopuse*> loopuses;
    std::vector<SmLoop*> loops;
    std::vector<SmEdgeuse*> edgeuses;
    std::vector<SmEdge*> edges;
    std::vector<SmVertexuse*> vertexuses;
    std::vector<SmVertex*> vertices;
    std::map<const void*, int64_t> index[10];

    template <class T> int64_t add(int k, std::vector<T*>& v, T* p)
    {
        if (p == nullptr)
            return -1;
        auto it = index[k].find((const void*)p);
        if (it != index[k].end())
            return it->second;
        int64_t i = (int64_t)v.size();
        index[k][(const void*)p] = i;
        v.push_back(p);
        return i;
    }
    template <class T> int64_t find(int k, const T* p) const
    {
        if (p == nullptr)
            return -1;
        auto it = index[k].find((const void*)p);
        return it == index[k].end() ? -3 : it->second;
    }
    int64_t typed(SM_TYPE t, SmTopology* p)
    {
        if (p == nullptr)
            return -1;
        switch (t)
        {
        case SmShell_TYPE: return add(1, shells, (SmShell*)p);
        case SmLoopuse_TYPE: return add(4, loopuses, (SmLoopuse*)p);
        case SmEdgeuse_TYPE: return add(6, edgeuses, (SmEdgeuse*)p);
        case SmVertexuse_TYPE: return add(8, vertexuses, (SmVertexuse*)p);
        case SmFaceuse_TYPE: return add(2, faceuses, (SmFaceuse*)p);
        default: return -4;
        }
    }
    int64_t find_typed(SM_TYPE t, const SmTopology* p) const
    {
        if (p == nullptr)
            return -1;
        switch (t)
        {
        case SmShell_TYPE: return find(1, p);
        case SmLoopuse_TYPE: return find(4, p);
        case SmEdgeuse_TYPE: return find(6, p);
        case SmVertexuse_TYPE: return find(8, p);
        case SmFaceuse_TYPE: return find(2, p);
        default: return -4;
        }
    }
};

std::vector<SmTopology*> list_of(SmTopology* head)
{
    std::vector<SmTopology*> v;
    SmTopology* p = head;
    while (p != nullptr && v.size() < 1000000)
    {
        v.push_back(p);
        p = p->GetNext();
        if (p == head)
            break;
    }
    return v;
}

void walk(SmBrep* brep, G& g)
{
    SmTArray<SmRegion*> regs;
    brep->GetRegions(regs);
    for (ULONG i = 0; i < regs.GetSize(); i++)
        g.add(0, g.regions, regs[i]);
    size_t r = 0, s = 0, fu = 0, lu = 0, eu = 0, vu = 0, l = 0, e = 0, v = 0;
    bool more = true;
    while (more)
    {
        more = false;
        for (; r < g.regions.size(); r++, more = true)
            for (SmTopology* t : list_of(g.regions[r]->GetList()))
                g.add(1, g.shells, (SmShell*)t);
        for (; s < g.shells.size(); s++, more = true)
        {
            SmShell* sh = g.shells[s];
            SM_TYPE ty = sh->*ShP::type();
            if (ty == SmFaceuse_TYPE)
                for (SmTopology* t : list_of(sh->GetList()))
                    g.add(2, g.faceuses, (SmFaceuse*)t);
            else
                g.typed(ty, sh->GetList());
        }
        for (; fu < g.faceuses.size(); fu++, more = true)
        {
            SmFaceuse* p = g.faceuses[fu];
            g.add(3, g.faces, p->*FuP::face());
            g.add(2, g.faceuses, p->*FuP::mate());
            for (SmTopology* t : list_of(p->GetList()))
                g.add(4, g.loopuses, (SmLoopuse*)t);
        }
        for (; lu < g.loopuses.size(); lu++, more = true)
        {
            SmLoopuse* p = g.loopuses[lu];
            g.add(5, g.loops, p->*LuP::loop());
            g.add(4, g.loopuses, p->*LuP::mate());
            SmTopology* start = p->*LuP::start();
            g.typed(p->*LuP::type(), start);
            if (p->*LuP::type() == SmEdgeuse_TYPE)
            {
                SmEdgeuse* q = (SmEdgeuse*)start;
                for (int k = 0; q != nullptr && k < 1000000; k++)
                {
                    g.add(6, g.edgeuses, q);
                    q = q->GetCCWEdgeuse();
                    if (q == (SmEdgeuse*)start)
                        break;
                }
            }
        }
        for (; eu < g.edgeuses.size(); eu++, more = true)
        {
            SmEdgeuse* p = g.edgeuses[eu];
            g.add(7, g.edges, (SmEdge*)p->GetOwner());
            g.typed(p->GetEdgeuseType(), p->*EuP::sorlu());
            g.add(6, g.edgeuses, p->GetCWEdgeuse());
            g.add(6, g.edgeuses, p->GetCCWEdgeuse());
            g.add(8, g.vertexuses, p->*EuP::vu());
            g.add(6, g.edgeuses, (SmEdgeuse*)p->GetNext());
            g.add(6, g.edgeuses, (SmEdgeuse*)p->GetLast());
        }
        for (; e < g.edges.size(); e++, more = true)
            for (SmTopology* t : list_of(g.edges[e]->GetList()))
                g.add(6, g.edgeuses, (SmEdgeuse*)t);
        for (; vu < g.vertexuses.size(); vu++, more = true)
        {
            SmVertexuse* p = g.vertexuses[vu];
            g.add(9, g.vertices, (SmVertex*)p->GetOwner());
            g.typed(p->GetVertexuseType(), p->*VuP::owner());
        }
        for (; v < g.vertices.size(); v++, more = true)
            for (SmTopology* t : list_of(g.vertices[v]->GetList()))
                g.add(8, g.vertexuses, (SmVertexuse*)t);
        for (; l < g.loops.size(); l++, more = true)
            g.add(4, g.loopuses, g.loops[l]->GetLoopuse());
    }
}

void put_list(Out& o, const std::vector<int64_t>& v)
{
    o.i((int64_t)v.size());
    for (int64_t x : v)
        o.i(x);
}

void put_point(Out& o, SmStatus s, const SmPoint3d& p)
{
    o.i(s == SM_SUCCESS ? 0 : (int64_t)s);
    if (s == SM_SUCCESS)
    {
        o.d(p.x);
        o.d(p.y);
        o.d(p.z);
    }
}

// `0` (none) or `1, type, dim, interval (2 d)` and five evaluations at 0, 1/4, 1/2,
// 3/4, 1 of the natural interval.
void put_curve(Out& o, const SmCurve* c)
{
    if (c == NULL)
    {
        o.i(0);
        return;
    }
    o.i(1);
    o.i((int64_t)c->GetType());
    o.i((int64_t)c->GetDim());
    SmExtent1d ivl = c->GetNaturalInterval();
    o.d(ivl.GetMin());
    o.d(ivl.GetMax());
    for (int k = 0; k <= 4; k++)
    {
        double t = ivl.GetMin() + 0.25 * k * (ivl.GetMax() - ivl.GetMin());
        SmPoint3d p(0.0, 0.0, 0.0);
        SmStatus s = c->EvaluatePoint(t, p);
        put_point(o, s, p);
    }
}

// `0` or `1, type, domain (4 d)` and a 3 x 3 grid of evaluations, then whether the
// face owns it.
void put_surface(Out& o, const SmSurface* s, const SmObject* owner)
{
    if (s == NULL)
    {
        o.i(0);
        return;
    }
    o.i(1);
    o.i((int64_t)s->GetType());
    SmExtent2d dom = s->GetNaturalUVDomain();
    o.d(dom.GetMin().x);
    o.d(dom.GetMin().y);
    o.d(dom.GetMax().x);
    o.d(dom.GetMax().y);
    for (int a = 0; a <= 2; a++)
        for (int b = 0; b <= 2; b++)
        {
            SmPoint2d uv(dom.GetMin().x + 0.5 * a * (dom.GetMax().x - dom.GetMin().x),
                         dom.GetMin().y + 0.5 * b * (dom.GetMax().y - dom.GetMin().y));
            SmPoint3d p(0.0, 0.0, 0.0);
            SmStatus st2 = s->EvaluatePoint(uv, p);
            put_point(o, st2, p);
        }
    o.i(s->GetOwner() == owner ? 1 : 0);
}

void export_graph(SmBrep* brep, G& g, Out& o)
{
    o.d((double)brep->GetTolerance());
    for (auto n : {g.regions.size(), g.shells.size(), g.faceuses.size(), g.faces.size(), g.loopuses.size(),
                   g.loops.size(), g.edgeuses.size(), g.edges.size(), g.vertexuses.size(), g.vertices.size()})
        o.i((int64_t)n);
    for (SmRegion* p : g.regions)
    {
        o.i(p == brep->GetInfiniteRegion() ? 1 : 0);
        o.i((int64_t)p->IsVoid());
        std::vector<int64_t> v;
        for (SmTopology* t : list_of(p->GetList()))
            v.push_back(g.find(1, t));
        put_list(o, v);
    }
    for (SmShell* p : g.shells)
    {
        o.i(g.find(0, p->GetOwner()));
        SM_TYPE ty = p->*ShP::type();
        o.i((int64_t)ty);
        std::vector<int64_t> v;
        if (ty == SmFaceuse_TYPE)
            for (SmTopology* t : list_of(p->GetList()))
                v.push_back(g.find(2, t));
        else
            v.push_back(g.find_typed(ty, p->GetList()));
        put_list(o, v);
    }
    for (SmFaceuse* p : g.faceuses)
    {
        o.i(g.find(1, p->GetOwner()));
        o.i(g.find(3, p->*FuP::face()));
        o.i(g.find(2, p->*FuP::mate()));
        o.i((int64_t)p->GetOrientation());
        std::vector<int64_t> v;
        for (SmTopology* t : list_of(p->GetList()))
            v.push_back(g.find(4, t));
        put_list(o, v);
    }
    for (SmFace* p : g.faces)
    {
        put_surface(o, p->*FP::srf(), p);
        o.i(g.find(2, p->*FP::fu()));
        SmExtent2d dm = p->*FP::dom();
        o.d(dm.GetMin().x);
        o.d(dm.GetMin().y);
        o.d(dm.GetMax().x);
        o.d(dm.GetMax().y);
        o.i((int64_t)(p->*FP::rect()));
        o.d((double)(p->*FP::tol()));
    }
    for (SmLoopuse* p : g.loopuses)
    {
        o.i(g.find(2, p->GetOwner()));
        o.i(g.find(5, p->*LuP::loop()));
        o.i(g.find(4, p->*LuP::mate()));
        SM_TYPE ty = p->*LuP::type();
        o.i((int64_t)ty);
        o.i(g.find_typed(ty, p->*LuP::start()));
        o.i((int64_t)(p->*LuP::orient()));
    }
    for (SmLoop* p : g.loops)
        o.i(g.find(4, p->GetLoopuse()));
    for (SmEdgeuse* p : g.edgeuses)
    {
        o.i(g.find(7, p->GetOwner()));
        SM_TYPE ty = p->GetEdgeuseType();
        o.i((int64_t)ty);
        o.i(g.find_typed(ty, p->*EuP::sorlu()));
        o.i(g.find(6, p->GetCWEdgeuse()));
        o.i(g.find(6, p->GetCCWEdgeuse()));
        o.i(g.find(8, p->*EuP::vu()));
        o.i((int64_t)p->GetOrientation());
        o.i(g.find(6, p->GetNext()));
        o.i(g.find(6, p->GetLast()));
        put_curve(o, p->*EuP::uv());
    }
    for (SmEdge* p : g.edges)
    {
        o.d((double)p->GetTolerance());
        SmExtent1d ivl = p->GetInterval();
        o.d(ivl.GetMin());
        o.d(ivl.GetMax());
        std::vector<int64_t> v;
        for (SmTopology* t : list_of(p->GetList()))
            v.push_back(g.find(6, t));
        put_list(o, v);
        // (edge marks are not exported: the nested GetWireEdges of TransferChildrenTo
        // marks the C++ edges, while the port keeps those marks in a per-call record,
        // topology.shell; the context's mark counters and locks are compared instead)
        put_curve(o, p->GetCurve());
    }
    for (SmVertexuse* p : g.vertexuses)
    {
        o.i(g.find(9, p->GetOwner()));
        SM_TYPE ty = p->GetVertexuseType();
        o.i((int64_t)ty);
        o.i(g.find_typed(ty, p->*VuP::owner()));
    }
    for (SmVertex* p : g.vertices)
    {
        SmPoint3d pt = p->GetPoint();
        o.d(pt.x);
        o.d(pt.y);
        o.d(pt.z);
        o.d((double)p->GetTolerance());
        std::vector<int64_t> v;
        for (SmTopology* t : list_of(p->GetList()))
            v.push_back(g.find(8, t));
        put_list(o, v);
    }
}

// One MakeFaceTopology call of op 3, read from the input after the B-rep block.
struct FaceSpec
{
    SmSurface* surface = nullptr;
    SmRegion* region = nullptr;
    int64_t flags = 0;
    SmTArray<ULONG> loops;
    SmTArray<SmEdgeuse*> edgeuses;
    SmTArray<SmVertex*> vertices;
    bool has_uv = false;
    SmTArray<SmBSplineCurve*> uv;
};

// Whether upstream dereferences null for this call on the current topology: an input
// edge use without edge, mate, shell or vertex (or a mate without vertex), loop counts
// past the edge uses, or loop vertices merged into a NULL keeper shell.
bool unsafe_call(const FaceSpec& f, SmRegion* infinite)
{
    uint64_t total = 0;
    for (ULONG i = 0; i < f.loops.GetSize(); i++)
        total += f.loops[i];
    if (total > f.edgeuses.GetSize())
        return true;
    for (ULONG i = 0; i < f.edgeuses.GetSize(); i++)
    {
        SmEdgeuse* eu = f.edgeuses[i];
        if (eu->GetEdge() == NULL || eu->GetShell() == NULL || eu->GetVertexuse() == NULL ||
            eu->GetVertexuse()->GetVertex() == NULL)
            return true;
        SmEdgeuse* mate = eu->GetMate();
        if (mate == NULL || mate->GetVertexuse() == NULL || mate->GetVertexuse()->GetVertex() == NULL)
            return true;
    }
    bool keeper = f.loops.GetSize() > 0 && f.loops[0] > 0;
    if (!keeper)
    {
        SmRegion* region = f.region ? f.region : infinite;
        for (ULONG i = 0; i < f.vertices.GetSize(); i++)
        {
            SmVertex* v = f.vertices[i];
            if (v->IsShellVertex())
                continue;
            SmShell* shell = NULL;
            v->GetShellInRegion(region, shell);
            if (shell != NULL)
                return true;
        }
    }
    return false;
}
} // namespace post
SC_EXPORT int32_t sc_topology_brep_p6_eval(
    int32_t op, const char* path, const double* input, int64_t n,
    int64_t seed, const int64_t* indices, int64_t count, int32_t repeat,
    int32_t lock_mask, int32_t flags, double* out, int64_t cap, int64_t* len)
{
    if (!path || !input || !len || n < 1 || op < 0 || op > 3 || count < 0 || static_cast<uint64_t>(count) > UINT32_MAX ||
        (count && !indices) || (op != 1 && count != 0) || repeat < 1 || repeat > 2 || (op == 3 && repeat != 1) || flags < 0 || flags > 31 ||
        lock_mask < 0 || lock_mask > 31)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{input, 0};
    Built built;
    SmStatus status;
    built.brep = build(rd, path, status);
    if (!built.brep) {
        const double rejected[] = {-1, st(status)};
        return sc_copy_out(rejected, 2, out, cap, len);
    }
    SmBrep* brep = built.brep;
    Graph g;
    walk(brep, g, true);
    if (op == 0 && (seed < 0 || (uint64_t)seed >= g.faceuses.size())) return SC_ERR_ARG;
    for (int64_t i=0; i<count; ++i) {
        if (indices[i] < 0 || (uint64_t)indices[i] >= g.edges.size()) return SC_ERR_ARG;
        auto* edge = g.edges[indices[i]];
        // CopyEdges dereferences the returned trimmed NURBS without a null
        // check. Keep the guard branch-specific and test the actual constructor.
        if (op == 1) {
            SmBSplineCurve* curve = edge->CreateTrimmedNURBSCurve(the_context());
            if (!curve) {
                const double rejected[] = {-2};
                return sc_copy_out(rejected, 1, out, cap, len);
            }
            delete curve;
        }
    }
    std::unique_ptr<SmNewMarkAndLock> outer;
    if (lock_mask) outer.reset(new SmNewMarkAndLock(&the_context(), lock_mask));
    const SmMarkType types[] = {SM_MT_MARK, SM_MT_MARK2, SM_MT_MARK3, SM_MT_MARKIO, SM_MT_MARKASSERT};
    ULONG before[5];
    for (int m=0; m<5; ++m) {
        before[m] = the_context().GetCurrentMark(types[m]);
        for (auto* fu : g.faceuses) fu->UnMark(types[m]);
        if (op == 3)
            for (auto* e : g.edges) e->UnMark(types[m]);
    }
    if (op == 3) {
        // the face specs follow the B-rep block (validated by the FFI wrapper)
        std::vector<post::FaceSpec> specs((size_t)rd.idx());
        for (auto& f : specs) {
            if (rd.flag()) f.surface = read_surface(rd, the_context());
            int64_t r = rd.idx();
            f.region = r < 0 ? NULL : g.regions[(size_t)r];
            f.flags = rd.idx();
            int64_t nl = rd.idx();
            for (int64_t i = 0; i < nl; ++i) f.loops.Add((ULONG)rd.idx());
            int64_t ne = rd.idx();
            for (int64_t i = 0; i < ne; ++i) f.edgeuses.Add(g.edgeuses[(size_t)rd.idx()]);
            int64_t nv = rd.idx();
            for (int64_t i = 0; i < nv; ++i) f.vertices.Add(g.vertices[(size_t)rd.idx()]);
            int64_t nu = rd.idx();
            f.has_uv = nu >= 0;
            for (int64_t i = 0; i < nu; ++i) f.uv.Add(rd.flag() ? read_curve(rd, the_context()) : NULL);
        }
        std::vector<double> calls;
        std::vector<SmFace*> faces;
        std::vector<SmShell*> shells;
        std::vector<SmRegion*> regions;
        for (auto& f : specs) {
            if (post::unsafe_call(f, brep->GetInfiniteRegion())) {
                const double rejected[] = {-2};
                return sc_copy_out(rejected, 1, out, cap, len);
            }
            SmFace* face = NULL; SmShell* shell = NULL; SmRegion* region = NULL;
            status = brep->MakeFaceTopology(f.region, f.surface, (f.flags & 4) ? SM_OT_OPPOSITE : SM_OT_SAME,
                                            f.loops, f.edgeuses, f.vertices, region, shell, face,
                                            (f.flags & 2) != 0, (f.flags & 1) != 0, f.has_uv ? &f.uv : NULL);
            calls.push_back(st(status));
            faces.push_back(face); shells.push_back(shell); regions.push_back(region);
            if (status != SM_SUCCESS) break;
        }
        post::G pg;
        post::walk(brep, pg);
        std::vector<double> result{(double)calls.size()};
        for (size_t i = 0; i < calls.size(); ++i) {
            result.push_back(calls[i]);
            result.push_back(pg.find(3, faces[i]));
            result.push_back(pg.find(1, shells[i]));
            result.push_back(pg.find(0, regions[i]));
        }
        for (int m=0; m<5; ++m) {
            result.push_back(the_context().GetCurrentMark(types[m]) - before[m]);
            result.push_back(the_context().IsMarkLocked(types[m]) ? 1 : 0);
        }
        post::Out o{result};
        post::export_graph(brep, pg, o);
        return sc_copy_out(result.data(), result.size(), out, cap, len);
    }
    Built destination;
    destination.brep = new(the_context()) SmBrep();
    SmTArray<SmEdge*> copied;
    SmShell* new_shell = g.shells.empty() ? NULL : g.shells[0];
    SmRegion* new_region = g.regions.empty() ? NULL : g.regions[0];
    SmFace* new_face = g.faces.empty() ? NULL : g.faces[0];
    if (!g.edges.empty()) copied.Add(g.edges[0]);
    for (int j=0; j<repeat; ++j) {
        if (op == 0)
            status = brep->TestClosureSplitRegion((flags & 1) != 0, g.faceuses[seed], new_shell, new_region);
        else if (op == 1) {
            SmTArray<SmEdge*> edges;
            for (int64_t i=0; i<count; ++i) edges.Add(g.edges[indices[i]]);
            status = brep->CopyEdges(edges, destination.brep, (flags & 2) ? &copied : NULL);
            new_shell = NULL; new_region = NULL; new_face = NULL;
        } else {
            SmSurface* surface = (flags & 4) ? NULL : new(the_context()) SmPlane(SmPoint3d(2,-3,5), SmVector3d(0,0,1));
            SmTArray<ULONG> loops;
            SmTArray<SmEdgeuse*> edgeuses;
            SmTArray<SmVertex*> vertices;
            status = brep->MakeFaceTopology(NULL, surface, (flags & 8) ? SM_OT_OPPOSITE : SM_OT_SAME,
                                            loops, edgeuses, vertices, new_region, new_shell, new_face,
                                            (flags & 16) != 0, (flags & 1) != 0, NULL);
            // The successful face owns surface; null-surface failure allocated none.
        }
        if (status != SM_SUCCESS) break;
    }
    std::vector<double> result{st(status)};
    if (op == 0) {
        result.push_back(g.find(1, new_shell));
        result.push_back(g.find(0, new_region));
    } else if (op == 1 && (flags & 2)) result.push_back(copied.GetSize());
    else if (op == 2) {
        result.push_back(new_face ? 1 : 0);
        result.push_back(new_shell ? 1 : 0);
        result.push_back(new_region ? 1 : 0);
    }
    for (int m=0; m<5; ++m) {
        result.push_back(the_context().GetCurrentMark(types[m]) - before[m]);
        result.push_back(the_context().IsMarkLocked(types[m]) ? 1 : 0);
        for (auto* fu : g.faceuses) result.push_back(fu->IsMarked(types[m]) ? 1 : 0);
    }
    for (auto* fu : g.faceuses) result.push_back(g.find(1,fu->GetShell()));
    SmTArray<SmFace*> faces; brep->GetFaces(faces);
    SmTArray<SmEdge*> edges; brep->GetEdges(edges);
    SmTArray<SmVertex*> vertices; brep->GetVertices(vertices);
    result.push_back(faces.GetSize()); result.push_back(edges.GetSize()); result.push_back(vertices.GetSize());
    SmTArray<SmEdge*> dest_edges; destination.brep->GetEdges(dest_edges); result.push_back(dest_edges.GetSize());
    SmTArray<SmRegion*> dest_regions; destination.brep->GetRegions(dest_regions);
    SmRegion* infinite = destination.brep->GetInfiniteRegion();
    int64_t infinite_index = -1;
    for (ULONG i=0; i<dest_regions.GetSize(); ++i) {
        if (dest_regions[i] == infinite) { infinite_index = i; break; }
    }
    result.push_back(dest_regions.GetSize());
    result.push_back(infinite_index);
    result.push_back(infinite ? infinite->IsVoid() : -1);
    if (op == 0 || op == 1) {
        // the whole post-operation graph, in the same traversal/export order for both
        // ops: the split B-rep for op 0, the destination B-rep (its infinite region,
        // wire shells, edges and vertices) for op 1.
        SmBrep* graph_brep = op == 0 ? brep : destination.brep;
        post::G pg;
        post::walk(graph_brep, pg);
        post::Out o{result};
        post::export_graph(graph_brep, pg, o);
    }
    return sc_copy_out(result.data(), result.size(), out, cap, len);
    SC_CATCH
}
