// SPDX-License-Identifier: Apache-2.0
// Shared fixture helpers of the SmMerge shims (boolean.merge_p5, boolean.merge_p2):
// operand construction (the shared topology blocks plus `[4, x, y, z]`, an isolated
// shell vertex, and `[5]`, an empty B-rep), native-order graph seeding and the
// post-operation snapshot. Include after sc_topology_fixture.h.
#pragma once
#include "sc_topology_fixture.h"

namespace {
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
} // namespace
