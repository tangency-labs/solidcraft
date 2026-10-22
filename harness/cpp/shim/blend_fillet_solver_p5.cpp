// SPDX-License-Identifier: Apache-2.0
// SmFilletSolver::ProcessRollOver / TestRollOver, source/SMLib/src/SmFilletSolver.cpp.
// Layout and the intentionally incomplete recovery fixture are documented by the FFI.
#include "sc_shim.h"
#include "sc_rollover_worker.h"
#include "sc_edgeuse_graph.h"
#include "StdAfx.h"
#include <SmContext.h>
#include <SmCurveClass.h>
#include <SmFilletSolver.h>
#include <SmFilletGeom.h>
#include <SmFilletCorner.h>
#include <SmFilletExecutive.h>
#include <SmPrimitiveCreation.h>
#include <SmLine.h>
#include <SmEdgeuse.h>
#include <SmFace.h>
#include <SmTArray.h>
#include <cmath>
#include <memory>
#include <vector>

namespace {
struct Solver : SmFilletSolver {
    Solver(const SmContext& ctx) : SmFilletSolver(ctx, 1e-5, 1e-3, 1e-3) {
        m_pEdgeuses[0] = m_pEdgeuses[1] = NULL;
        m_pExecutive = NULL;
        m_dOrientations[0] = m_dOrientations[1] = 1.0;
        m_dSavedOffsets[0] = m_dSavedOffsets[1] = 0.0;
        m_bExtendBefore = m_bExtendAfter = FALSE;
    }
    SmFilletSolverType GetSolverType() override { return SM_FS_UNKNOWN; }
    void GetName(TCHAR* p, size_t n) override { if(n) p[0] = 0; }
    SmFilletGeom* geom() { return m_vFilletGeoms[0]; }
    void clear() { delete m_vFilletGeoms[0]; m_vFilletGeoms.ReSet(); }
    void set_eu(SmEdgeuse* eu) { m_pEdgeuses[0] = eu; }
};
struct Corner : SmFilletCorner {
    SmFilletCornerType type;
    Corner(const SmContext& ctx, SmFilletExecutive* exec, int t)
        : SmFilletCorner(ctx, NULL, NULL, NULL, exec, 1e-5, 1e-3), type((SmFilletCornerType)t) {}
    SmStatus MakeCornerTopology() override { return SM_SUCCESS; }
    SmFilletCornerType GetCornerType() const override { return type; }
    void GetName(TCHAR* p, size_t n) const override { if(n) p[0] = 0; }
};
struct Rail { int n; std::vector<double> a,b; std::vector<int> cls; };
bool integer(double x, int lo, int hi) { return std::isfinite(x) && x==std::floor(x) && x>=lo && x<=hi; }
}
// Guard the dereferences after the null-surface NER without excluding earlier
// defined returns (blend patches, missing classifications, or 1x1 rails).
static bool safe_process_rail_copies(const SmFilletGeom* geom) {
    if(geom->GetFilletGeomType()==SM_FG_BLENDS) return true;
    SmFilletEdge* rails[2]={geom->GetRail(0),geom->GetRail(1)};
    if(!rails[0]) return false;
    SmCurveClassification* a=rails[0]->GetCurveClass();
    if(!a) return true;
    if(!rails[1]) return false;
    SmCurveClassification* b=rails[1]->GetCurveClass();
    if(!b || (a->GetSize()==1 && b->GetSize()==1)) return true;
    if(!geom->GetFilletSurface()) return true; // defined NER before the Copy
    return rails[0]->GetCurve() && rails[1]->GetCurve();
}
static int32_t rollover_eval_in_process(const double* in, int64_t n,
    double* out, int64_t cap, int64_t* len) {
    if (!in || n<6 || !len || cap<0 || (cap && !out)) return SC_ERR_ARG;
    SC_TRY
    int64_t at=0;
    if(!integer(in[at],0,2)) return SC_ERR_ARG;
    int op=(int)in[at++];
    if(!integer(in[at],0,6)) return SC_ERR_ARG;
    int type=(int)in[at++];
    if(!integer(in[at],-1,12)) return SC_ERR_ARG;
    int corner=(int)in[at++];
    if(!integer(in[at],0,16)) return SC_ERR_ARG;
    int nadj=(int)in[at++];
    std::vector<int> adjusted;
    for(int i=0;i<nadj;i++) { if(at>=n || !integer(in[at],0,1)) return SC_ERR_ARG; adjusted.push_back((int)in[at++]); }
    Rail r[2];
    for(int i=0;i<2;i++) {
        if(at>=n || !integer(in[at],-1,8)) return SC_ERR_ARG;
        r[i].n=(int)in[at++];
        for(int j=0;j<r[i].n;j++) {
            if(n-at<3 || (!integer(in[at+2],100,104) && in[at+2]!=109)) return SC_ERR_ARG;
            r[i].a.push_back(in[at++]); r[i].b.push_back(in[at++]); r[i].cls.push_back((int)in[at++]);
        }
    }
    if(at!=n) return SC_ERR_ARG;
    // A supported full-rollover corner reads real face/curve data that this
    // ProcessRollOver-only fixture does not construct. Known classifications,
    // blend patches, missing classes and unsupported corners do not read it.
    if(op==0 && type!=SM_FG_BLENDS && r[0].n==1 && r[1].n==1 &&
       (r[0].cls[0]==SM_PC_UNKNOWN || r[1].cls[0]==SM_PC_UNKNOWN) && (corner==3 || corner==7)) return SC_ERR_ARG;
    SmContext ctx;
    SmFilletExecutive exec(ctx);
    Corner c0(ctx,&exec,corner<0?0:corner), c1(ctx,&exec,1);
    std::unique_ptr<SmBrep> target;
    SmFilletBrep pseudo;
    std::unique_ptr<SmFilletVertex> vertices[4];
    std::unique_ptr<SmFilletEdge> owned_rails[2];
    Solver solver(ctx);
    SmFilletGeom* fg=solver.geom();
    fg->SetFilletGeomType((SmFilletGeomType)type);
    SmFilletEdge* rails[2]={NULL,NULL};
    SmFace* face=NULL;
    {
        target.reset(SmPrimitiveCreation::CreateRectangle(ctx, 1e-5, 1.0, 1.0, SmAxis2Placement()));
        if(!target) return SC_ERR_ARG;
        SmTArray<SmFace*> faces; target->GetFaces(faces);
        if(faces.GetSize()!=1) return SC_ERR_ARG;
        face=faces[0];
    }
    for(int i=0;i<2;i++) {
        vertices[2*i].reset(new (&pseudo) SmFilletVertex(corner<0?NULL:&c0));
        vertices[2*i+1].reset(new (&pseudo) SmFilletVertex());
        SmFilletVertex* v0=vertices[2*i].get();
        SmFilletVertex* v1=vertices[2*i+1].get();
        SmStatus s=SmFilletEdge::MakeFilletEdge(&pseudo,v0,v1,rails[i]);
        if(s!=SM_SUCCESS || !rails[i]) return SC_ERR_ARG;
        owned_rails[i].reset(rails[i]);
        fg->SetRail(i,rails[i]);
        // TestRollOver installs a fresh classification; do not leak a seeded one.
        if(r[i].n>=0 && op!=2) {
            SmCurveClassification* cc=new (ctx) SmCurveClassification();
            cc->SetSize(r[i].n);
            for(int j=0;j<r[i].n;j++) {
                (*cc)[j].m_vInterval.SetMinMax(r[i].a[j],r[i].b[j]);
                SmObject* object=NULL;
                switch(r[i].cls[j]) {
                    case 101: object=pseudo.GetInfiniteRegion(); break;
                    case 102: object=face; break;
                    case 103: object=rails[i]; break;
                    case 104: object=v0; break;
                    case 109: object=rails[i]->GetPrimaryEdgeuse(); break;
                }
                (*cc)[j].m_vMid.SetClassObject((SmPointClassificationType)r[i].cls[j],object);
            }
            rails[i]->SetCurveClass(cc);
        }
        if(op==2) {
            SmLine* line=NULL;
            s=SmLine::CreateLineSegment(ctx,3,SmPoint3d(.25,.25+.25*i,0),SmPoint3d(.75,.25+.25*i,0),line);
            if(s!=SM_SUCCESS || !line) return SC_ERR_ARG;
            rails[i]->SetCurve(line);
            line->SetOwner(rails[i]);
            rails[i]->SetOriginalFace(face);
        }
    }
    SmTArray<SmFilletCorner*> adj;
    for(int i:adjusted) adj.Add(i==0?&c0:&c1);
    int status;
    if(op==1) { solver.clear(); status=(int)solver.TestRollOver(adj); }
    else if(op==2) status=(int)solver.TestRollOver(adj);
    else { if(!safe_process_rail_copies(fg)) return SC_ERR_ARG;
        solver.set_eu(rails[0]->GetPrimaryEdgeuse()); status=(int)solver.ProcessRollOver(fg,adj); }
    std::vector<double> result={(double)(status==SM_SUCCESS?0:status),(double)(op==1?type:(int)fg->GetFilletGeomType()),(double)adj.GetSize()};
    for(ULONG i=0;i<adj.GetSize();i++) result.push_back(adj[i]==&c0?0:1);
    for(int i=0;i<2;i++) {
        SmCurveClassification* cc=rails[i]->GetCurveClass();
        result.push_back(cc?(double)cc->GetSize():-1.0);
        if(cc) for(ULONG j=0;j<cc->GetSize();j++) {
            result.push_back((*cc)[j].m_vInterval.GetMin());
            result.push_back((*cc)[j].m_vInterval.GetMax());
            result.push_back((double)(*cc)[j].m_vMid.GetPointClass());
        }
    }
    return sc_copy_out(result.data(),(int64_t)result.size(),out,cap,len);
    SC_CATCH
}

static int32_t rollover_target_in_process(double* out, int64_t cap, int64_t* len) {
    if(!len || cap<0 || (cap && !out)) return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    std::unique_ptr<SmBrep> target(SmPrimitiveCreation::CreateRectangle(ctx,1e-5,1.0,1.0,SmAxis2Placement()));
    if(!target) return SC_ERR_ARG;
    Graph graph; walk(target.get(),graph); std::vector<double> result={0.0};
    export_graph(target.get(),graph,result);
    return sc_copy_out(result.data(),(int64_t)result.size(),out,cap,len);
    SC_CATCH
}

// The safe C entry points contain native signals and noexcept-destructor aborts,
// in addition to ordinary exceptions. Parsing/fixture allocations run only in the child.
SC_EXPORT int32_t sc_blend_fillet_solver_p5_eval(const double* in, int64_t n,
    double* out, int64_t cap, int64_t* len) {
    if(!in || n<6 || !len || cap<0 || (cap && !out)) return SC_ERR_ARG;
    return sc_rollover::isolated(0,in,n,out,cap,len);
}
SC_EXPORT int32_t sc_blend_fillet_solver_p5_target(double* out,int64_t cap,int64_t* len) {
    if(!len || cap<0 || (cap && !out)) return SC_ERR_ARG;
    return sc_rollover::isolated(1,nullptr,0,out,cap,len);
}
