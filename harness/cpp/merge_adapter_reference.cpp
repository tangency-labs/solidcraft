// SPDX-License-Identifier: Apache-2.0
// Calls the pinned original SMLib; no geometry or merge algorithm is reproduced.
#include <SmSmlibAll.h>
#include <SmMerge.h>
#include <array>
#include <cmath>
#include <cstdlib>
#include <iomanip>
#include <iostream>
#include <vector>

constexpr ULONG attribute_id = 4242;
struct Copies : SmNotifyCallback {
    std::array<long,4> counts{};
    SmStatus Execute(SmNotifyOperation op, SmObject* caller, SmObject*, SmObject*, SmObject*) override {
        const long types[] = {16021,16016,16002,16011};
        if(op == SM_NO_COPY && caller) for(int i=0;i<4;++i) if(caller->GetType()==types[i]) ++counts[i];
        return SM_SUCCESS;
    }
};
template<class T> void print(const std::vector<T>& a) {
    std::cout << '[';
    for(size_t i=0;i<a.size();++i) { if(i) std::cout << ','; std::cout << a[i]; }
    std::cout << ']';
}
long index_of(const SmTArray<SmVertex*>& a, SmVertex* p) {
    for(ULONG i=0;i<a.GetSize();++i) if(a[i]==p) return i;
    return -1;
}
SmVertex* point(SmBrep& b, const SmPoint3d& p) {
    SmShell* shell=nullptr; SmVertex* vertex=nullptr;
    if(b.MakeShellVertex(b.GetInfiniteRegion(),p,shell,vertex)!=SM_SUCCESS) throw 1;
    return vertex;
}
void dump(SmBrep* b, std::vector<long>& ints, std::vector<double>& values) {
    if(!b) { ints.push_back(0); return; }
    ints.push_back(1);
    SmTArray<SmRegion*> regions; b->GetRegions(regions);
    SmTArray<SmShell*> shells; b->GetShells(shells);
    SmTArray<SmFace*> faces; b->GetFaces(faces);
    SmTArray<SmVertex*> vertices; b->GetVertices(vertices);
    SmTArray<SmEdge*> edges; b->GetEdges(edges);
    ints.insert(ints.end(),{static_cast<long>(regions.GetSize()),static_cast<long>(shells.GetSize()),static_cast<long>(faces.GetSize()),static_cast<long>(vertices.GetSize()),static_cast<long>(edges.GetSize()),b->m_bEditingEnabled,b->GetContext()->GetDoingBoolean()});
    for(ULONG i=0;i<vertices.GetSize();++i) {
        auto* vertex=vertices[i]; auto p=vertex->GetPoint();
        values.insert(values.end(),{p[0],p[1],p[2],vertex->GetTolerance()});
        auto* attribute=static_cast<SmLongAttribute*>(vertex->FindAttribute(attribute_id));
        SmTArray<SmAObject*> users; if(attribute) attribute->GetUsers(users);
        ints.insert(ints.end(),{attribute ? attribute->GetValue() : -1,static_cast<long>(users.GetSize())});
    }
    for(ULONG i=0;i<edges.GetSize();++i) {
        auto* edge=edges[i]; SmTArray<SmEdgeuse*> uses; edge->GetEdgeuses(uses);
        ints.insert(ints.end(),{index_of(vertices,edge->GetStartVertex()),index_of(vertices,edge->GetEndVertex()),static_cast<long>(uses.GetSize()),static_cast<long>(edge->GetCurve()->GetType())});
        auto interval=edge->GetInterval(); values.insert(values.end(),{interval.GetMin(),interval.GetMax(),edge->GetTolerance()});
        for(double f:{0.,.25,.5,.75,1.}) {
            SmPoint3d p;
            if(edge->GetCurve()->EvaluatePoint(interval.GetMin()+f*(interval.GetMax()-interval.GetMin()),p)!=SM_SUCCESS) throw 2;
            values.insert(values.end(),{p[0],p[1],p[2]});
        }
    }
}
int main(int argc,char** argv) {
    if(argc!=6 && argc!=7) return 2;
    double a[5];
    for(int i=0;i<5;++i) { char* end=nullptr; a[i]=std::strtod(argv[i+1],&end); if(end==argv[i+1]||*end||!std::isfinite(a[i])) return 2; }
    int operation=a[0],kind=a[1],shared=a[2],repetitions=a[4];double scale=a[3];
    int constructor_uses_source=0;
    if(argc==7) {
        char* end=nullptr;double flag=std::strtod(argv[6],&end);
        if(end==argv[6]||*end||!std::isfinite(flag)||(flag!=0. && flag!=1.)) return 2;
        constructor_uses_source=static_cast<int>(flag);
    }
    if(operation!=a[0]||operation<0||operation>5||kind!=a[1]||kind<0||kind>3||shared!=a[2]||shared<0||shared>1||scale<=0||repetitions!=a[4]||repetitions<1||repetitions>3||(operation==5&&repetitions!=1)) return 2;
    try {
        SmContext source_context,target_context;
        SmContext* dc=shared ? &source_context : &target_context;
        Copies source_log,target_log; Copies* dl=shared ? &source_log : &target_log;
        source_context.SetUserNotifyCallback(&source_log); dc->SetUserNotifyCallback(dl);
        SmBrep* source=new (source_context) SmBrep(SM_USE_DEFAULT,SM_USE_DEFAULT,&source_context);
        SmBrep* dest=new (*dc) SmBrep(SM_USE_DEFAULT,SM_USE_DEFAULT,dc);
        double tolerance=scale*1e-6;source->SetTolerance(tolerance);dest->SetTolerance(tolerance);
        SmTArray<SmVertex*> selected;
        if(kind==1||kind==2) {
            auto p=SmPoint3d(scale,2*scale,3*scale),q=SmPoint3d(4*scale,-scale,5*scale);
            selected.Add(point(*source,p));selected.Add(point(*source,q));
            if(kind==2) {
                SmLine* curve=nullptr;SmExtent1d interval;SmEdge* edge=nullptr;
                if(SmLine::CreateLineSegment(source_context,3,p,q,curve,nullptr)!=SM_SUCCESS) return 3;
                interval=curve->GetNaturalInterval();
                if(source->MakeWireEdge(source->GetInfiniteRegion(),selected[0],selected[1],curve,interval,SM_OT_SAME,edge)!=SM_SUCCESS) return 3;
            }
            selected[0]->AddAttribute(new (source_context) SmLongAttribute(attribute_id,73,SM_AB_REFERENCE));
        }
        if(kind!=3) point(*dest,SmPoint3d(-7*scale,4*scale,2*scale));
        source_log.counts={};dl->counts={};
        SmStatus status=SM_SUCCESS;long copied=0,mates=0;SmBoolean maybe_open=FALSE;
        if(operation==2||operation==3||operation==5) {
            SmMerge merge(constructor_uses_source ? source_context : *dc,dest,source,tolerance,0.);
            for(int i=0;i<repetitions && status==SM_SUCCESS;++i) {
                SmBrep* result=nullptr;
                if(operation==2) { SmTArray<SmFace*> faces;status=source->CopyFaces(faces,dest,nullptr,TRUE,&merge); }
                else if(operation==3) status=merge.PiecewiseMerge(nullptr,TRUE,TRUE,result);
                else { status=merge.ManifoldBoolean(SM_BO_UNION,result);source=merge.GetTopologyIntersector().GetOtherBrep(); }
            }
        } else for(int i=0;i<repetitions && status==SM_SUCCESS;++i) {
            if(operation==0) { SmTArray<SmVertex*> out;status=source->CopyVertices(selected,dest,&out);copied+=out.GetSize(); }
            else if(operation==1) status=dest->MergeBrep(*source);
            else status=source->OrientTrimmedSurfaces(TRUE,maybe_open,TRUE);
        }
        std::vector<long> ints={status==SM_SUCCESS ? 0 : status,copied,mates,maybe_open};std::vector<double> values;
        dump(source,ints,values);dump(dest,ints,values);
        ints.insert(ints.end(),source_log.counts.begin(),source_log.counts.end());ints.insert(ints.end(),dl->counts.begin(),dl->counts.end());
        std::cout<<std::setprecision(17)<<"{\"ints\":";print(ints);std::cout<<",\"values\":";print(values);std::cout<<",\"deferred\":null}\n";
        source_context.SetUserNotifyCallback(nullptr);dc->SetUserNotifyCallback(nullptr);
        delete source;delete dest;
    } catch(...) { return 4; }
    return 0;
}
