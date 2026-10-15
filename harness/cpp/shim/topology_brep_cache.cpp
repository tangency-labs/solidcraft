// SPDX-License-Identifier: Apache-2.0
// SmBrepCache constructor, BuildTrees, ReSizeTrees, accessors and type queries.
// Input validated by the safe FFI: exact build tol, vertices(x y z tol),
// polygon faces(n vertex-indices...), resize vertices(x y z tol), query boxes(6).
// Output: integer-count, integer stream (topology/ids), float stream (boxes).
#include "sc_shim.h"
#include <SmBrep.h>
#include <SmBrepCache.h>
#include <SmContext.h>
#include <SmEdge.h>
#include <SmLine.h>
// Fixture-only access: SmPolyBrep is final and exposes no public insertion of
// standalone vertices (including isolated vertices not connected to any face).
// Keep this access change scoped to the native polygon header in this shim.
#define protected public
#include <SmPoly.h>
#undef protected
#include <SmTree.h>
#include <SmVertex.h>
#include <map>
#include <memory>
#include <vector>

namespace {
struct CacheVertex : SmVertex { public: CacheVertex() = default; ~CacheVertex() override = default; };
struct Reader {
    const double* in; int64_t pos = 0;
    double num() { return in[pos++]; }
    SmVector3d point() { double x=num(), y=num(), z=num(); return SmVector3d(x,y,z); }
};
void box(std::vector<double>& out, const SmExtent3d& b) {
    out.insert(out.end(), {b.GetMin().x,b.GetMin().y,b.GetMin().z,b.GetMax().x,b.GetMax().y,b.GetMax().z});
}
int64_t index(const std::map<const void*,int64_t>& ids, const void* p) {
    if (!p) return -1;
    auto it=ids.find(p); return it==ids.end() ? -2 : it->second;
}
void dump(SmTree* tree, const SmBrepCache* cache, const std::map<const void*,int64_t>& objects,
          const std::vector<SmExtent3d>& queries, std::vector<double>& ints, std::vector<double>& floats) {
    ints.push_back(tree ? 1 : 0);
    if (!tree) return;
    ints.push_back(tree->GetOwner()==cache ? 1 : 0);
    SmTArray<SmTreeNode*> nodes; tree->GetAllTreeNodes(nodes);
    std::map<const void*,int64_t> ids;
    for (ULONG i=0;i<nodes.GetSize();++i) ids[nodes[i]]=i;
    ints.push_back(index(ids,tree->GetTopNode())); ints.push_back(nodes.GetSize());
    for (ULONG i=0;i<nodes.GetSize();++i) {
        SmTreeNode* n=nodes[i];
        ints.insert(ints.end(),{(double)index(ids,n->m_pParent),(double)index(ids,n->m_pChild1),(double)index(ids,n->m_pChild2),
                               (double)n->m_eAuxDataType,(double)n->m_eGeomType});
        box(floats,n->GetBoundingBox());
        SmTArray<SmObjectList*> items;
        if (n->m_eAuxDataType==SM_AD_OBJECT_LIST) n->GetObjectList(items);
        ints.push_back(items.GetSize());
        for (ULONG j=0;j<items.GetSize();++j) { ints.push_back(index(objects,items[j]->m_pObject)); box(floats,items[j]->m_sBBox); }
    }
    SmTArray<SmObjectList*> all; tree->GetObjectList(all); ints.push_back(all.GetSize());
    for (ULONG j=0;j<all.GetSize();++j) { ints.push_back(index(objects,all[j]->m_pObject)); box(floats,all[j]->m_sBBox); }
    for (const auto& q: queries) {
        SmTArray<SmObject*> hits; tree->GetObjectsInBox(q,hits); ints.push_back(hits.GetSize());
        for (ULONG j=0;j<hits.GetSize();++j) ints.push_back(index(objects,hits[j]));
    }
}
}

SC_EXPORT int32_t sc_topology_brep_cache_eval(const double* in, int64_t n,
                                              double* out, int64_t cap, int64_t* len) {
    if (!in || n<7) return SC_ERR_ARG;
    SC_TRY
    Reader r{in}; bool exact=r.num()!=0, build=r.num()!=0; double tol=r.num();
    SmContext context;
    std::unique_ptr<SmBrep> brep;
    std::unique_ptr<SmPolyBrep> poly;
    if (exact) brep.reset(new (context) SmBrep(SM_USE_DEFAULT,SM_USE_DEFAULT,&context));
    else poly.reset(new (context) SmPolyBrep(tol));
    std::map<const void*,int64_t> objects;
    int64_t nv=(int64_t)r.num();
    std::vector<SmPolyVertex*> vertices;
    for (int64_t i=0;i<nv;++i) {
        SmVector3d p=r.point(); double t=r.num();
        SmPolyVertex* v=new (poly.get()) SmPolyVertex(p,t); poly->m_pPolyVertexListHead->PostInsert(v);
        vertices.push_back(v); objects[v]=i;
    }
    int64_t nf=(int64_t)r.num();
    if (nf) {
        SmPolyRegion* region=new (poly.get()) SmPolyRegion(poly.get());
        SmPolyShell* shell=new (poly.get()) SmPolyShell(region);
        for (int64_t i=0;i<nf;++i) {
            SmPolyFace* face=new (poly.get()) SmPolyFace(tol,NULL,NULL,shell,NULL);
            objects[face]=200000+i;
            SmPolyLoop* loop=NULL; face->StartPolyEdgeLoop(loop);
            int64_t count=(int64_t)r.num(); std::vector<SmPolyVertex*> vs;
            for(int64_t j=0;j<count;++j) vs.push_back(vertices[(int64_t)r.num()]);
            for(int64_t j=0;j<count;++j) {
                SmPolyEdge* edge=NULL; auto a=vs[j]; auto b=vs[(j+1)%count];
                loop->AddPolyEdge(tol,a->GetPoint(),b->GetPoint(),a,NULL,poly.get(),edge);
            }
            loop->FinishPolyEdgeLoop(poly.get());
        }
        SmTArray<SmPolyEdge*> edges; poly->GetPolyEdges(edges);
        for(ULONG i=0;i<edges.GetSize();++i) objects[edges[i]]=100000+i;
    }
    SmBrepCache cache(exact ? static_cast<SmSAGObject*>(brep.get()) : static_cast<SmSAGObject*>(poly.get()));
    std::vector<double> ints, floats;
    ints.push_back(cache.GetType());
    for (SM_TYPE type: {SmBrepCache_TYPE,SmObject_TYPE,SmCacheObj_TYPE,SmBrep_TYPE,SmBrepCache_TYPE+1}) ints.push_back(cache.IsKindOf(type)?1:0);
    if (build) { auto s=cache.BuildTrees(); ints.push_back(s==SM_SUCCESS?0:s); }
    else ints.push_back(0);
    int64_t nr=(int64_t)r.num();
    std::vector<std::unique_ptr<CacheVertex>> resized;
    SmTArray<SmVertex*> resize;
    for (int64_t i=0;i<nr;++i) {
        SmVector3d p=r.point(); double t=r.num();
        // Constructor's raw tolerance is writable through SetTolerance.
        SmObject* source = exact ? static_cast<SmObject*>(brep.get()) : static_cast<SmObject*>(poly.get());
        std::unique_ptr<CacheVertex> v(new (source) CacheVertex());
        v->SetPoint(p); v->SetTolerance(t,FALSE,FALSE); resize.Add(v.get()); resized.push_back(std::move(v));
    }
    int64_t ne=(int64_t)r.num();
    std::vector<std::unique_ptr<SmBrep>> wire_breps;
    SmTArray<SmEdge*> resize_edges;
    for(int64_t i=0;i<ne;++i) {
        SmVector3d a=r.point(), b=r.point(); double et=r.num(), at=r.num(), bt=r.num();
        std::unique_ptr<SmBrep> wire(new (context) SmBrep(SM_USE_DEFAULT,SM_USE_DEFAULT,&context));
        SmLine* line=NULL;
        if(SmLine::CreateLineSegment(context,3,a,b,line)!=SM_SUCCESS) return SC_ERR_ARG;
        SmEdge* edge=NULL;
        // Construct two distinct endpoint vertices even below the model tolerance.
        // CreateWireEdgeFromCurve would GlueVertices on tiny segments; the Rust
        // fixture intentionally builds the same explicit two-endpoint graph.
        SmShell* shell=NULL; SmVertex* start=NULL; SmVertex* end=NULL;
        if(wire->MakeShellVertex(wire->GetInfiniteRegion(),a,shell,start)!=SM_SUCCESS) return SC_ERR_ARG;
        if(wire->MakeWireEdgeVertex(wire->GetInfiniteRegion(),start,line,SmExtent1d(0.0,1.0),SM_OT_SAME,b,edge,end)!=SM_SUCCESS) return SC_ERR_ARG;
        edge->SetTolerance(et,FALSE,FALSE);
        auto v=edge->GetVertex(); auto other=edge->GetOtherVertex(v);
        v->SetTolerance(at,FALSE,FALSE); other->SetTolerance(bt,FALSE,FALSE);
        resize_edges.Add(edge); wire_breps.push_back(std::move(wire));
    }
    if (build && (nr || ne)) { auto s=cache.ReSizeTrees(NULL,&resize_edges,&resize); ints.push_back(s==SM_SUCCESS?0:s); }
    else ints.push_back(0);
    int64_t nq=(int64_t)r.num(); std::vector<SmExtent3d> queries;
    for(int64_t i=0;i<nq;++i) { double a[6]; for(double& v:a) v=r.num(); queries.emplace_back(a[0],a[1],a[2],a[3],a[4],a[5]); }
    dump(cache.GetVertexTree(),&cache,objects,queries,ints,floats);
    dump(cache.GetCurveTree(),&cache,objects,queries,ints,floats);
    dump(cache.GetSurfaceTree(),&cache,objects,queries,ints,floats);
    std::vector<double> result; result.push_back(ints.size()); result.insert(result.end(),ints.begin(),ints.end()); result.insert(result.end(),floats.begin(),floats.end());
    return sc_copy_out(result.data(),(int64_t)result.size(),out,cap,len);
    SC_CATCH
}
