// SPDX-License-Identifier: Apache-2.0
// Calls the real protected SmMerge::ProcessWiresAndShellVertices through a
// derived test accessor. Fixture graphs are exported BEFORE running the unit.
#include "sc_topology_fixture.h"
#include <SmMerge.h>
#include <memory>

namespace {
struct MergeAccess : SmMerge {
    MergeAccess(const SmContext& c, SmBrep* a, SmBrep* b) : SmMerge(c,a,b) {}
    using SmMerge::ProcessWiresAndShellVertices;
    SmStatus relate(SmObject* a, SmObject* b) { return m_vTI.Relate(a,b); }
};

SmBrep* operand(const char* path, const double* in, SmStatus& status) {
    if (in[0] == 4 || in[0] == 5) {
        auto* b = new (the_context()) SmBrep(SM_USE_DEFAULT,SM_USE_DEFAULT,(SmContext*)&the_context());
        status = SM_SUCCESS;
        if (in[0] == 4) {
            SmShell* s; SmVertex* v;
            status = b->MakeShellVertex(b->GetInfiniteRegion(), SmPoint3d(in[1],in[2],in[3]),s,v);
        }
        return b;
    }
    Rd rd{in,0};
    return build(rd,path,status);
}

// Seed entity ids in native B-rep owning-list order, matching arena insertion.
void native_graph(SmBrep* b, Graph& g) {
    SmTArray<SmEdge*> edges; b->GetEdges(edges);
    for (ULONG i=0;i<edges.GetSize();++i) g.edge(edges[i]);
    SmTArray<SmVertex*> vertices; b->GetVertices(vertices);
    for (ULONG i=0;i<vertices.GetSize();++i) g.vertex(vertices[i]);
    walk(b,g,true);
}

void snapshot(SmBrep* b, SmMarkType mt, std::vector<int64_t>& ids, std::vector<double>& values) {
    Graph g; native_graph(b,g);
    for (auto n : {g.regions.size(),g.shells.size(),g.faceuses.size(),g.faces.size(),g.loopuses.size(),
                   g.loops.size(),g.edgeuses.size(),g.edges.size(),g.vertexuses.size(),g.vertices.size()})
        ids.push_back((int64_t)n);
    for (auto* e : g.edges) {
        auto* v = e->GetVertex();
        ids.push_back(g.find(9,v));
        ids.push_back(g.find(9,e->GetOtherVertex(v)));
        ids.push_back(e->IsWire() ? 1 : 0);
        ids.push_back(e->IsMarked(mt) ? 1 : 0);
        ids.push_back((int64_t)list_of(e->GetList()).size());
        for (auto* eu : list_of(e->GetList())) ids.push_back(g.find(6,eu));
        values.push_back(e->GetInterval().GetMin());
        values.push_back(e->GetInterval().GetMax());
        values.push_back(e->GetTolerance());
    }
    for (auto* v : g.vertices) {
        ids.push_back(v->IsShellVertex() ? 1 : 0);
        ids.push_back(v->IsMarked(mt) ? 1 : 0);
        ids.push_back((int64_t)list_of(v->GetList()).size());
        for (auto* vu : list_of(v->GetList())) ids.push_back(g.find(8,vu));
        put_vec(values,v->GetPoint());
        values.push_back(v->GetTolerance());
    }
    for (auto* r : g.regions) ids.push_back(r->GetOwner() == (SmOwningTopology*)b ? 1 : 0);
    for (auto* sh : g.shells) ids.push_back(g.find(0,sh->GetOwner()));
    for (auto* f : g.faces) {
        ids.push_back(g.find(2,f->*FPeek::fu()));
        values.push_back(f->*FPeek::tol());
        auto d=f->*FPeek::dom();
        values.insert(values.end(),{d.GetMin().x,d.GetMin().y,d.GetMax().x,d.GetMax().y});
    }
    for (auto* fu : g.faceuses) {
        ids.push_back(g.find(1,fu->GetOwner())); ids.push_back(g.find(3,fu->*FuPeek::face()));
        ids.push_back(g.find(2,fu->*FuPeek::mate())); ids.push_back((int64_t)fu->GetOrientation());
        auto list=list_of(fu->GetList()); ids.push_back((int64_t)list.size());
        for(auto* lu:list) ids.push_back(g.find(4,lu));
    }
    for (auto* l : g.loops) ids.push_back(g.find(4,l->GetLoopuse()));
    for (auto* lu : g.loopuses) {
        ids.push_back(g.find(2,lu->GetOwner())); ids.push_back(g.find(5,lu->*LuPeek::loop()));
        ids.push_back(g.find(4,lu->*LuPeek::mate())); ids.push_back((int64_t)lu->GetOrientation());
        auto type=lu->*LuPeek::type(); auto start=lu->*LuPeek::start();
        ids.push_back(type==SmEdgeuse_TYPE ? 1 : type==SmVertexuse_TYPE ? 2 : 0);
        ids.push_back(g.typed(type,start));
    }
    for (auto* eu : g.edgeuses) {
        ids.push_back(g.find(7,eu->GetEdge()));
        ids.push_back(g.find(8,eu->GetVertexuse()));
        ids.push_back(g.find(6,eu->GetCWEdgeuse()));
        ids.push_back(g.find(6,eu->GetCCWEdgeuse()));
        auto* owner=eu->*EuPeek::sorlu();
        auto type=eu->GetEdgeuseType();
        ids.push_back(type==SmLoopuse_TYPE ? 1 : type==SmShell_TYPE ? 2 : 0);
        ids.push_back(g.typed(type,owner)); ids.push_back((int64_t)eu->GetOrientation());
    }
    for (auto* vu : g.vertexuses) {
        ids.push_back(g.find(9,vu->GetVertex()));
        auto type=vu->GetVertexuseType();
        ids.push_back(type==SmEdgeuse_TYPE ? 1 : type==SmLoopuse_TYPE ? 2 : type==SmShell_TYPE ? 3 : 0);
        ids.push_back(g.typed(type,vu->*VuPeek::owner()));
    }
}
}

// mode -1 exports one operand, mode 0 runs the unit. Flat fixture blocks and
// mark/mate selectors are validated in the safe Rust FFI wrapper.
SC_EXPORT int32_t sc_boolean_merge_p5_eval(
    int32_t mode, const char* path_a, const double* a, int64_t na,
    const char* path_b, const double* b, int64_t nb,
    int32_t operation, int32_t mark_a, int32_t mark_b,
    const int64_t* marked_a, int64_t nma, const int64_t* marked_b, int64_t nmb,
    const int64_t* mates, int64_t nm, int32_t prefill, int32_t repeat,
    double* out, int64_t cap, int64_t* out_len) {
    if (!a || !path_a || na < 1 || !out_len) return SC_ERR_ARG;
    SC_TRY
    Built aa; SmStatus status;
    aa.brep = operand(path_a,a,status);
    if (!aa.brep || status != SM_SUCCESS) return SC_ERR_ARG;
    Graph ga; native_graph(aa.brep,ga);
    if (mode == -1) {
        std::vector<double> raw{0.0};
        export_graph(aa.brep,ga,raw);
        std::vector<double> result{(double)raw.size()};
        result.insert(result.end(),raw.begin(),raw.end());
        for (auto* v : ga.vertices) {
            std::vector<int64_t> list;
            for (auto* vu : list_of(v->GetList())) list.push_back(ga.find(8,vu));
            put_list(result,list);
        }
        return sc_copy_out(result.data(),(int64_t)result.size(),out,cap,out_len);
    }
    if (mode != 0 || !b || !path_b || nb < 1)
        return SC_ERR_ARG;
    Built bb; bb.brep = operand(path_b,b,status);
    if (!bb.brep || status != SM_SUCCESS) return SC_ERR_ARG;
    Graph gb; native_graph(bb.brep,gb);
    if (nma != (int64_t)(ga.edges.size()+ga.vertices.size()) ||
        nmb != (int64_t)(gb.edges.size()+gb.vertices.size()) || nm != (int64_t)gb.edges.size())
        return SC_ERR_ARG;
    // Only these operations append the mate to keep. Upstream permits NULL
    // here, but Rust's Vec<EdgeId> cannot represent it. Other operations never
    // use the mate, so unmated marked wires remain valid comparison inputs.
    if (operation == 1 || operation == 2 || operation == 5 || operation == 6)
        for (size_t i=0;i<gb.edges.size();++i)
            if (marked_b[i] && gb.edges[i]->IsWire() && mates[i] < 0) return SC_ERR_ARG;
    SmNewMarkAndLock la(&the_context(),mark_a);
    std::unique_ptr<SmNewMarkAndLock> lb;
    if (mark_a != mark_b) lb = std::make_unique<SmNewMarkAndLock>(&the_context(),mark_b);
    auto ma = la.GetMarkType(); auto mb = lb ? lb->GetMarkType() : ma;
    MergeAccess merge(the_context(),aa.brep,bb.brep);
    for (size_t i=0;i<ga.edges.size();++i) if(marked_a[i]) ga.edges[i]->Mark(ma);
    for (size_t i=0;i<ga.vertices.size();++i) if(marked_a[ga.edges.size()+i]) ga.vertices[i]->Mark(ma);
    for (size_t i=0;i<gb.edges.size();++i) {
        if (marked_b[i]) gb.edges[i]->Mark(mb);
        if (mates[i] >= 0) {
            if ((size_t)mates[i] >= ga.edges.size()) return SC_ERR_ARG;
            status = merge.relate(ga.edges[mates[i]],gb.edges[i]);
            if(status != SM_SUCCESS) return SC_ERR_ARG;
        }
    }
    for (size_t i=0;i<gb.vertices.size();++i) if(marked_b[gb.edges.size()+i]) gb.vertices[i]->Mark(mb);
    SmTArray<SmEdge*> keep;
    std::vector<int64_t> ids;
    for (int iteration=0;iteration<repeat;++iteration) {
        if(prefill && !ga.edges.empty()) keep.Add(ga.edges[0]);
        status = merge.ProcessWiresAndShellVertices((SmBooleanOperationType)operation,keep,ma,mb);
        ids.push_back(status == SM_SUCCESS ? 0 : (int64_t)status);
        ids.push_back((int64_t)keep.GetSize());
        for (ULONG i=0;i<keep.GetSize();++i) ids.push_back(ga.find(7,keep[i]));
        if(status != SM_SUCCESS) break;
    }
    std::vector<double> values;
    snapshot(aa.brep,ma,ids,values); snapshot(bb.brep,mb,ids,values);
    std::vector<double> result{(double)ids.size()};
    for(auto id : ids) result.push_back((double)id);
    result.insert(result.end(),values.begin(),values.end());
    return sc_copy_out(result.data(),(int64_t)result.size(),out,cap,out_len);
    SC_CATCH
}
