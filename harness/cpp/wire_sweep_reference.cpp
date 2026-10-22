// SPDX-License-Identifier: Apache-2.0
// Independent executable: unchanged pinned SMLib performs the actual DoSweep.
#include <SmSmlibAll.h>
#include <SmTopologySweep.h>
#include <SmTransSweepGeometry.h>
#include <cmath>
#include <cstdlib>
#include <iomanip>
#include <iostream>
#include <vector>

template<class T> long index_of(const SmTArray<T*>& a, T* p) {
    for (ULONG i=0;i<a.GetSize();++i) if(a[i]==p) return static_cast<long>(i);
    return -1;
}
template<class T> void print(const std::vector<T>& a) {
    std::cout << '[';
    for(size_t i=0;i<a.size();++i) { if(i) std::cout << ','; std::cout << a[i]; }
    std::cout << ']';
}
int main(int argc,char** argv) {
    if(argc!=12) return 2;
    double a[11];
    for(int i=0;i<11;++i) { char* end=nullptr; a[i]=std::strtod(argv[i+1],&end); if(end==argv[i+1] || *end || !std::isfinite(a[i])) return 2; }
    if(a[6]<=0 || a[10]<0 || a[10]>4096 || std::floor(a[10])!=a[10]) return 2;
    try {
        SmContext source_context, target_context;
        SmContext* tc=a[7]!=0 ? &source_context : &target_context;
        SmBrep source(SM_USE_DEFAULT,SM_USE_DEFAULT,&source_context);
        SmBrep target(SM_USE_DEFAULT,SM_USE_DEFAULT,tc);
        source.SetTolerance(a[6]);target.SetTolerance(a[6]);
        SmShell* initial_shell=nullptr;SmVertex* original=nullptr;
        if(source.MakeShellVertex(source.GetInfiniteRegion(),SmPoint3d(a[0],a[1],a[2]),initial_shell,original)!=SM_SUCCESS) return 3;
        source_context.SetDoingBoolean(a[8]!=0);tc->SetDoingBoolean(a[9]!=0);
        SmTranslationalSweepGeometry geometry(SmVector3d(a[3],a[4],a[5]));
        SmTopologySweep sweep(geometry,FALSE);
        sweep.SetRepetitions(static_cast<ULONG>(a[10]));
        SmTArray<SmVertex*> selected;selected.Add(original);
        SmStatus status=sweep.DoSweep(&source,target.GetInfiniteRegion(),nullptr,nullptr,&selected);
        SmTArray<SmVertex*> sv, vertices;source.GetVertices(sv);target.GetVertices(vertices);
        SmTArray<SmEdge*> se, edges;source.GetEdges(se);target.GetEdges(edges);
        SmTArray<SmRegion*> regions;target.GetRegions(regions);
        SmTArray<SmShell*> shells;target.GetShells(shells);
        SmTArray<SmFace*> faces;target.GetFaces(faces);
        SmTArray<SmEdgeuse*> uses;target.GetEdgeuses(uses);
        SmTArray<SmVertexuse*> vus;target.GetVertexuses(vus);
        std::vector<long> ints={status==SM_SUCCESS ? 0 : status,static_cast<long>(sv.GetSize()),static_cast<long>(se.GetSize()),
            static_cast<long>(regions.GetSize()),static_cast<long>(shells.GetSize()),static_cast<long>(faces.GetSize()),
            static_cast<long>(edges.GetSize()),static_cast<long>(vertices.GetSize()),static_cast<long>(uses.GetSize()),static_cast<long>(vus.GetSize()),
            static_cast<long>(sweep.GetMapToSame()->Count()),static_cast<long>(sweep.GetMapToHigher()->Count()),
            source_context.GetDoingBoolean(),tc->GetDoingBoolean()};
        auto p=original->GetPoint();std::vector<double> values={p[0],p[1],p[2]};
        for(ULONG i=0;i<vertices.GetSize();++i) {
            auto* v=vertices[i];SmTArray<SmVertexuse*> vu;v->GetVertexuses(vu);ints.push_back(vu.GetSize());
            p=v->GetPoint();values.insert(values.end(),{p[0],p[1],p[2],v->GetTolerance()});
        }
        for(ULONG i=0;i<shells.GetSize();++i) { ints.push_back(shells[i]->IsWireShell());ints.push_back(shells[i]->GetRegion()==target.GetInfiniteRegion()); }
        for(ULONG i=0;i<edges.GetSize();++i) {
            auto* e=edges[i];SmTArray<SmEdgeuse*> eu;e->GetEdgeuses(eu);
            ints.insert(ints.end(),{index_of(vertices,e->GetStartVertex()),index_of(vertices,e->GetEndVertex()),static_cast<long>(eu.GetSize()),static_cast<long>(e->GetCurve()->GetType())});
            auto ivl=e->GetInterval();values.insert(values.end(),{ivl.GetMin(),ivl.GetMax(),e->GetTolerance()});
            for(double f:{0.,.25,.5,.75,1.}) {
                SmPoint3d q;if(e->GetCurve()->EvaluatePoint(ivl.GetMin()+(ivl.GetMax()-ivl.GetMin())*f,q)!=SM_SUCCESS) return 4;
                values.insert(values.end(),{q[0],q[1],q[2]});
            }
            for(ULONG j=0;j<eu.GetSize();++j) {
                auto* u=eu[j];ints.insert(ints.end(),{static_cast<long>(u->GetOrientation()),index_of(eu,u->GetMate()),index_of(eu,u->GetRadial()),index_of(vertices,u->GetVertexuse()->GetVertex()),index_of(shells,u->GetShell())});
            }
        }
        auto* same=sweep.GetToSame(original);auto* higher=sweep.GetToHigher(original);
        ints.insert(ints.end(),{index_of(vertices,same),index_of(edges,higher),same && sweep.GetFromSame(same)==original,higher && sweep.GetFromLower(higher)==original});
        std::cout << std::setprecision(17) << "{\"ints\":";print(ints);std::cout << ",\"values\":";print(values);std::cout << "}\n";
    } catch (...) { return 5; }
    return 0;
}
