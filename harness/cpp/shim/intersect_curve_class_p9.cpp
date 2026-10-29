// SPDX-License-Identifier: Apache-2.0
// Native part-9 oracle. Real SmBrep topology and real SmLine/SmPlane/B-spline
// surfaces. Explicit cache recipes isolate traversal from trim-cache construction.
#include "sc_shim.h"
#include "StdAfx.h"
#include <SmBrep.h>
#include <SmBSplineSurface.h>
#include <SmCacheMgr.h>
#include <SmContext.h>
#include <SmCurveClass.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmFace.h>
#include <SmFaceuse.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmLoop.h>
#include <SmLoopuse.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmPointClass.h>
#include <SmShell.h>
#include <SmSurface.h>
#include <SmSurfaceCache.h>
#include <SmTol.h>
#include <SmTree.h>
#include <SmTrimSrfCache.h>
#include <SmVertex.h>
#include <SmVertexuse.h>
#include <nurbs.h>
#include <cmath>
#include <map>
#include <memory>
#include <vector>

namespace {
struct P9Tree : SmTree {
    explicit P9Tree(SmObject* owner) : SmTree(owner) { m_sNodeMgr.Initialize(ALIGN_SIZE(sizeof(SmTreeNode)),25); }
    SmTreeNode* node() { return (SmTreeNode*)m_sNodeMgr.GetNewElement(); }
    void top(SmTreeNode* p) { m_pTopNode=p; }
};
struct P9Trim : SmTrimSrfCache {
    explicit P9Trim(const SmSurface& s) : SmTrimSrfCache(s) {
        m_sAuxMgr.Initialize(ALIGN_SIZE(sizeof(SmBezierAux2d)),25);
    }
    // Loop-vertex UV projections, computed by native ComputeUVPoint while this cache is
    // already attached to the surface (see sc_intersect_curve_class_p9_run).
    typedef std::map<SmVertexuse*,SmPoint2d> UvMap;
    void recipe(const SmSurface& s, const SmTArray<SmLoopuse*>& loops, int state, bool collapsed,
                const UvMap& uvs,
                std::vector<std::unique_ptr<SmVertexList>>& vertices,
                std::vector<std::unique_ptr<SmEdgeuseList>>& edges) {
        auto* tree = new (s.GetContext()) P9Tree(this); m_pTree=tree;
        int count=(state==4 || state==6)?3:1;
        const bool split_v=state==6;
        SmTreeNode* nodes[3]={};
        for(int i=0;i<count;i++) {
            nodes[i]=tree->node(); auto* nd=nodes[i]; nd->m_pTree=tree;
            nd->m_eAuxDataType=SM_AD_SURFACE_DATA; nd->m_eGeomType=SM_NG_DEFAULT;
            nd->m_pData=(SmBezierAux2d*)m_sAuxMgr.GetNewElement();
            auto* aux=(SmBezierAux2d*)nd->m_pData;
            const double lower=i==2?.5:0, upper=i==1?.5:1;
            aux->m_sUVDomain=split_v ? SmExtent2d(SmPoint2d(0,lower),SmPoint2d(1,upper)) :
                SmExtent2d(SmPoint2d(lower,0),SmPoint2d(upper,1));
            aux->mBA_pSurface=&s; aux->m_pOwningTreeNode=nd;
            aux->m_eSplitDir=count==3 && i==0 ? (split_v?SM_SP_V:SM_SP_U) : SM_SP_UNKNOWN;
            SmPoint3d low,high;
            s.EvaluatePoint(aux->m_sUVDomain.GetMin(),low);
            s.EvaluatePoint(aux->m_sUVDomain.GetMax(),high);
            nd->m_sBBox=SmExtent3d(low,high);
            // Reverse prepend preserves the supplied native loop/list order.
            for(long li=(long)loops.GetSize()-1;li>=0;--li) {
                SmTArray<SmVertexuse*> vs; SmTArray<SmEdgeuse*> es;
                loops[li]->GetVertexuses(vs); loops[li]->GetEdgeuses(es);
                for(long j=(long)vs.GetSize()-1;j>=0;--j) {
                    auto item=std::make_unique<SmVertexList>(); item->m_pVU=vs[j];
                    item->m_vUVPoint=uvs.find(vs[j])->second; item->m_vUVTol.Set(0,0);
                    // A pole has many UV representatives. This cache recipe
                    // deliberately stores it in the lower v leaf, away from the query.
                    const auto pole=vs[j]->GetVertex()->GetPoint();
                    if(split_v && collapsed && pole.x==0 && pole.y==0 && pole.z==0)
                        item->m_vUVPoint=SmPoint2d(0,0);
                    const auto uv=item->m_vUVPoint;
                    if(split_v && (uv.x<0 || uv.x>1 || uv.y<lower || uv.y>upper)) continue;
                    aux->m_vVertexList.Prepend(item.get()); vertices.push_back(std::move(item));
                }
                for(long j=(long)es.GetSize()-1;j>=0;--j) {
                    auto item=std::make_unique<SmEdgeuseList>(); item->m_pEU=es[j];
                    aux->m_sEdgeuseList.Prepend(item.get()); edges.push_back(std::move(item));
                }
            }
        }
        if(count==3) { nodes[0]->m_pChild1=nodes[1]; nodes[0]->m_pChild2=nodes[2];
            nodes[1]->m_pParent=nodes[0]; nodes[2]->m_pParent=nodes[0]; }
        tree->top(nodes[0]);
        m_bFaceWasModified=state==2; m_bHaveTSurfaceCache=TRUE;
        m_bFaceContainmentDone=state==3?FALSE:state==5?2:TRUE;
        if(state==0) m_bHaveTSurfaceCache=FALSE;
    }
};
void pc(std::vector<double>& out,const SmPointClassification& p,const std::vector<SmVertex*>& vertices,
        const std::vector<SmEdge*>& edges) {
    auto obj=p.GetObject(); double id=-1;
    for(size_t j=0;j<vertices.size();j++) if(obj && obj==vertices[j]) id=100+j;
    for(size_t j=0;j<edges.size();j++) if(obj && obj==edges[j]) id=200+j;
    if(obj && id<0) id=-2;
    auto uv=p.GetUVParam(); auto uvw=p.GetUVWParam(); auto sss=p.GetSSSPoint();
    out.insert(out.end(),{double(p.GetPointClass()),id,p.GetDeviation(),p.GetTParam(),uv.x,uv.y,
        uvw.x,uvw.y,uvw.z,double(p.AreParametersSet()),double(p.GetSSSPointFlag()),sss.x,sss.y,sss.z,
        p.GetPreSnapParam(),SmTol::GetSrcZoneTol3d(&p).val});
}
SmSurface* make_surface(const SmContext& ctx,int kind,double scale) {
    if(kind==1) return new (&ctx) SmPlane(SmPoint3d(0,0,0),SmVector3d(1,0,0),SmVector3d(0,1,0),
        SmVector2d(scale,scale),SmExtent2d(SmPoint2d(0,0),SmPoint2d(1,1)),&ctx);
    auto* n=sm_AllocateNurbSurface(1,1,1,1,3,3);
    for(int i=0;i<2;i++) for(int j=0;j<2;j++) {
        n->net->Pw[i][j].x=scale*i; n->net->Pw[i][j].y=scale*j*(kind==2?i:1);
        n->net->Pw[i][j].z=0; n->net->Pw[i][j].w=NL_NOW;
    }
    for(int j=0;j<4;j++) { n->knu->U[j]=j<2?0:1; n->knv->U[j]=j<2?0:1; }
    return new (&ctx) SmBSplineSurface(n,FALSE,NULL);
}
}
SC_EXPORT int32_t sc_intersect_curve_class_p9_run(const double* in,int64_t n,double* out,int64_t cap,int64_t* len) {
    if(!in || n<22 || !len || cap<0 || (!out && cap)) return SC_ERR_ARG;
    for(int64_t j=0;j<n;j++) if(!std::isfinite(in[j])) return SC_ERR_ARG;
    auto integer=[](double x,int max){ return x>=0 && x<=max && x==std::floor(x); };
    for(int j=0;j<10;j++) if(!integer(in[j],j==2?6:j==1?2:1)) return SC_ERR_ARG;
    if(!integer(in[12],8)||!integer(in[13],8)||n!=22+3*(int)in[12]+23*(int)in[13]) return SC_ERR_ARG;
    if(in[10]<=0 || in[11]<0 || in[14]>in[15]) return SC_ERR_ARG;
    if((in[0]==1 && (in[8]!=0 || in[13]!=0)) ||
       (in[9]!=0 && in[12]!=0) || (in[0]==0 && (in[12]!=0 || in[9]!=0))) return SC_ERR_ARG;
    if((in[16]==in[19] && in[17]==in[20] && in[18]==in[21]) ||
       (in[8]==0 && (in[18]!=0 || in[21]!=0))) return SC_ERR_ARG;
    const double source_scale_guard=in[0]==1?1:in[10];
    for(int j=16;j<22;j++) if(!std::isfinite(in[j]*source_scale_guard)) return SC_ERR_ARG;
    for(int j=22;j<22+3*(int)in[12];j++) if(!std::isfinite(in[j]*in[10])) return SC_ERR_ARG;
    int shared=0;
    for(int j=0;j<(int)in[13];j++) {
        // kind start[3] end[3] lo hi same edge_zone p1[3] d1[3] p2[3] d2[3]
        const double* p=in+22+3*(int)in[12]+23*j;
        if(!integer(p[0],1) || !integer(p[9],1) || p[7]>p[8] || p[10]<0) return SC_ERR_ARG;
        for(int k=1;k<7;k++) if(!std::isfinite(p[k]*in[10])) return SC_ERR_ARG;
        for(int k=11;k<23;k++) if(!std::isfinite(p[k]*in[10])) return SC_ERR_ARG;
        // A Hermite edge carries its own curve, never the source curve.
        if(p[9]) { ++shared; if(p[0]!=0 || in[8]!=1) return SC_ERR_ARG; }
        else if(p[0]==0 && p[1]==p[4] && p[2]==p[5] && p[3]==p[6]) return SC_ERR_ARG;
    }
    if(shared>1) return SC_ERR_ARG;
    SC_TRY
    const int op=in[0],kind=in[1],state=in[2],nv=in[12],ne=in[13]; const double scale=in[10],zone=in[11];
    const SmContext ctx;
    std::unique_ptr<SmBrep> brep(new (ctx) SmBrep());
    auto* s=make_surface(ctx,kind,scale);
    std::vector<SmVertex*> vertices; std::vector<SmEdge*> edges;
    SmTArray<SmVertex*> loop_vertices; SmTArray<ULONG> loop_sizes; SmTArray<SmEdgeuse*> eus;
    for(int j=0;j<nv;j++) {
        const double* p=in+22+3*j; SmShell* sh=nullptr; SmVertex* v=nullptr;
        auto st=brep->MakeShellVertex(nullptr,SmPoint3d(scale*p[0],scale*p[1],0),sh,v);
        if(st!=SM_SUCCESS) return SC_ERR_ARG; v->SetTolerance(zone,FALSE);
        loop_vertices.Add(v); vertices.push_back(v);
    }
    SmRegion* region=nullptr; SmShell* shell=nullptr; SmFace* face=nullptr;
    SmStatus setup;
    if(in[9]) setup=brep->CreateFaceFromSurface(s,SmExtent2d(SmPoint2d(0,0),SmPoint2d(1,1)),face);
    else setup=brep->MakeFaceTopology(nullptr,s,SM_OT_SAME,loop_sizes,eus,loop_vertices,region,shell,face);
    if(setup!=SM_SUCCESS || !face) return SC_ERR_ARG;
    face->SetTolerance(1e-5,FALSE);
    SmTArray<SmLoopuse*> loops; face->GetUpwardFaceuse()->GetLoopuses(loops);
    // Canonical UV corner/side ids let every returned boundary object be compared,
    // independently of the primitive constructor's starting edgeuse.
    if(in[9]) { vertices.resize(4,nullptr); edges.resize(4,nullptr); }
    // Match the explicit rectangle-view tolerances on the Rust side too.
    if(in[9]) for(ULONG j=0;j<loops.GetSize();j++) {
        SmTArray<SmVertexuse*> vs; SmTArray<SmEdgeuse*> es;
        loops[j]->GetVertexuses(vs); loops[j]->GetEdgeuses(es);
        SmEdgeuse* first_edge=nullptr;
        for(ULONG k=0;k<vs.GetSize();k++) {
            vs[k]->GetVertex()->SetTolerance(zone,FALSE);
            SmPoint2d uv; vs[k]->ComputeUVPoint(uv);
            const int corner=uv.y<.5 ? (uv.x<.5?0:1) : (uv.x<.5?3:2);
            vertices[corner]=vs[k]->GetVertex();
        }
        for(ULONG k=0;k<es.GetSize();k++) {
            es[k]->GetEdge()->SetTolerance(0.0,FALSE);
            auto* trim=es[k]->GetUVTrimCurve(); if(!trim) return SC_ERR_ARG;
            SmPoint3d uv; auto st=trim->EvaluatePoint(trim->GetNaturalInterval().Evaluate(.5),uv);
            if(st!=SM_SUCCESS) return SC_ERR_ARG;
            const int side=uv.y==0?0:uv.x==1?1:uv.y==1?2:3;
            edges[side]=es[k]->GetEdge();
            if(side==0) first_edge=es[k];
        }
        // CreateFaceFromSurface starts at (0,1). This fixture supplies the same
        // explicit (0,0)->(1,0)->(1,1)->(0,1) traversal as the Rust arena view.
        if(loops[j]->SetEdgeuse(first_edge)!=SM_SUCCESS) return SC_ERR_ARG;
        SmTArray<SmVertexuse*> ordered; loops[j]->GetVertexuses(ordered);
        if(ordered.GetSize()!=4) return SC_ERR_ARG;
        for(ULONG k=0;k<4;k++) if(ordered[k]->GetVertex()!=vertices[k]) return SC_ERR_ARG;
    }
    // Loop vertex UVs are computed by native projection, including the 3D gap.
    for(int j=0;j<nv;j++) { const double* p=in+22+3*j;
        vertices[j]->SetPoint(SmPoint3d(scale*p[0],scale*p[1],scale*p[2]));
    }
    const double source_scale=op?1:scale;
    SmPoint3d a(source_scale*in[16],source_scale*in[17],source_scale*in[18]);
    SmPoint3d b(source_scale*in[19],source_scale*in[20],source_scale*in[21]);
    std::unique_ptr<SmCurve> owned(new (&ctx) SmLine(a,b,2+(int)in[8],&ctx)); SmCurve* source=owned.get();
    SmTArray<SmEdge*> targets;
    for(int j=0;j<ne;j++) {
        const double* p=in+22+3*nv+23*j; SmCurve* c=nullptr;
        if(p[9]) c=owned.release();
        else if(p[0]==1) c=new (ctx) SmHermiteCurve(SmVector3d(scale*p[11],scale*p[12],scale*p[13]),
            SmVector3d(scale*p[14],scale*p[15],scale*p[16]),
            SmVector3d(scale*p[17],scale*p[18],scale*p[19]),
            SmVector3d(scale*p[20],scale*p[21],scale*p[22]),3);
        else c=new (&ctx) SmLine(SmPoint3d(scale*p[1],scale*p[2],scale*p[3]),
            SmPoint3d(scale*p[4],scale*p[5],scale*p[6]),3,&ctx);
        SmEdge* e=nullptr; auto st=brep->CreateWireEdgeFromCurve(c,SmExtent1d(p[7],p[8]),e);
        if(st!=SM_SUCCESS || !e) return SC_ERR_ARG;
        e->SetTolerance(p[10],FALSE); targets.Add(e); edges.push_back(e);
    }
    SmCurveClassification cc; cc.SetCurve(source,FALSE); cc.SetSurface1(s);
    SmTol::SetZoneTol3d(&cc,SmZoneTol3d(zone)); cc.SetDoingCrossHatch(in[4]);
    cc.SetSize(1); cc.SetInterval(SmExtent1d(in[14],in[15]));
    cc[0]=SmCurveInterval(cc.GetInterval(),&ctx,&cc);
    std::vector<std::unique_ptr<SmVertexList>> vl; std::vector<std::unique_ptr<SmEdgeuseList>> el;
    // CacheGuard clears the cache before its externally owned list nodes, including
    // when the native operation throws or the fixture is rejected below.
    struct CacheGuard {
        SmSurface* surface;
        ~CacheGuard() { if(surface) SmCacheMgr::DeleteObjectsCache(surface); }
    } cache_guard{op?s:nullptr};
    if(op) {
        std::unique_ptr<P9Trim> cache(new (&ctx) P9Trim(*s));
        // Rust has no face point test (SmSurface::GlobalPointSolve classifies each
        // drop against the owner face unless m_bPointTestEnabled is off), so the cache
        // used for every native ComputeUVPoint call disables it, as the upstream callers
        // do around GlobalPointSolve (SmVertexuse.cpp:471-480).
        cache->m_bPointTestEnabled=FALSE;
        P9Trim* attached=cache.get();
        // Attach the cache first: ComputeUVPoint reaches the surface cache through the
        // surface, and without one it builds a real trimmed cache, which fails on
        // surfaces of extreme scale (status SM_ERR) and leaves the UV unset.
        SmCacheMgr::DeleteObjectsCache(s); s->SetCacheObj(cache.release());
        P9Trim::UvMap uvs;
        for(ULONG j=0;j<loops.GetSize();j++) {
            SmTArray<SmVertexuse*> vs; loops[j]->GetVertexuses(vs);
            for(ULONG k=0;k<vs.GetSize();k++) {
                SmPoint2d uv;
                if(vs[k]->ComputeUVPoint(uv)!=SM_SUCCESS) return SC_ERR_ARG;
                uvs[vs[k]]=uv;
            }
        }
        attached->recipe(*s,loops,state,kind==2,uvs,vl,el);
    }
    // State 2 is a cache flagged "face was modified" (m_bFaceWasModified), which is only
    // meaningful while the Brep is in edit mode. SmBrep starts with editing disabled, and
    // then SmCacheMgrTSrf::CacheMakeOrValidate (SmCacheMgrTSrf.cpp:97-107) deletes such a
    // cache and rebuilds it with BuildTree before the unit reads it; that rebuild belongs
    // to the cache units and fails on a face without loops. Enable editing so the cache is
    // used as given, which is what the Rust side models.
    if(op && state==2) brep->m_bEditingEnabled=TRUE;
    SmStatus status=op?cc.IntersectTSBound(face,in[3]):in[7]?cc.IntersectEdges(targets,*face,FALSE):
        cc.IntersectEdges(targets,*face,in[5],in[6]);
    std::vector<double> result={status==SM_SUCCESS?0.:double(status),double(cc.GetSize()),cc.GetInterval().GetMin(),cc.GetInterval().GetMax()};
    for(ULONG j=0;j<cc.GetSize();j++) { auto& iv=cc[j]; result.push_back(iv.GetMin()); result.push_back(iv.GetMax());
        pc(result,iv.m_vStart,vertices,edges); pc(result,iv.m_vMid,vertices,edges); pc(result,iv.m_vEnd,vertices,edges);
    }
    return sc_copy_out(result.data(),(int64_t)result.size(),out,cap,len);
    SC_CATCH
}
