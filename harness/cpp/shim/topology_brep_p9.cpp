// SPDX-License-Identifier: Apache-2.0
// Real SmBrep member calls. Shared fixtures only construct/export the oracle graph.
#include "sc_topology_fixture.h"
#include "sc_tracked.h"
#include <SmPointClass.h>

SC_EXPORT int32_t sc_topology_brep_p9_eval(int32_t op, int32_t export_only,
    int32_t selector, int32_t variant, int32_t editing, int32_t same,
    const char* path, const double* in, int64_t n,
    double parameter, const double* point, double start_tol, double end_tol,
    double* out, int64_t cap, int64_t* len) {
    if (!path || !in || !point || !len || n < 1 || op < 0 || op > 8 || selector < 0 || variant < 0 || variant > 5)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in,0}; Built built; SmStatus status;
    built.brep = build(rd,path,status);
    if (!built.brep || rd.at != n) return SC_ERR_ARG;
    SmBrep* b = built.brep;
    Graph g; walk(b,g,true);
    SmVertexuse* start_vu = nullptr;
    SmVertexuse* end_vu = nullptr;
    if (op == 5 && variant == 0 && editing) {
        if (g.faces.empty()) return SC_ERR_ARG;
        SmFace* face = g.faces[selector % g.faces.size()];
        SmExtent2d domain = face->GetUVDomain();
        SmPoint3d p,q;
        SmPoint2d uv0(domain.GetMin().x + 0.3*domain.XLength(), domain.GetMin().y + 0.4*domain.YLength());
        SmPoint2d uv1(domain.GetMin().x + 0.7*domain.XLength(), domain.GetMin().y + 0.6*domain.YLength());
        if (face->GetSurface()->EvaluatePoint(uv0,p) != SM_SUCCESS || face->GetSurface()->EvaluatePoint(uv1,q) != SM_SUCCESS)
            return SC_ERR_ARG;
        SmLoop *l0=nullptr,*l1=nullptr; SmVertex *v0=nullptr,*v1=nullptr;
        if (b->MakeVertexLoop(face,p,l0,v0) != SM_SUCCESS || b->MakeVertexLoop(face,q,l1,v1) != SM_SUCCESS)
            return SC_ERR_ARG;
        start_vu = l0->GetLoopuse()->GetVertexuse();
        end_vu = l1->GetLoopuse()->GetVertexuse();
        g = Graph(); walk(b,g,true);
    }
    if (op == 4 || op == 6 || op == 7) {
        if (g.edges.empty()) return SC_ERR_ARG;
        SmEdge* edge = g.edges[selector % g.edges.size()];
        if (!edge->GetCurve() || !edge->GetStartVertex() || !edge->GetEndVertex()) return SC_ERR_ARG;
        if (variant == 1 || variant == 3) {
            SmPoint3d p,q;
            SmExtent1d ivl=edge->GetInterval();
            if (edge->GetCurve()->EvaluatePoint(ivl.GetMin(),p) != SM_SUCCESS ||
                edge->GetCurve()->EvaluatePoint(ivl.GetMax(),q) != SM_SUCCESS) return SC_ERR_ARG;
            SmLine* line=new (the_context()) SmLine(p,q,3,&the_context());
            if (line->EditParameterization(ivl,FALSE) != SM_SUCCESS) { delete line; return SC_ERR_ARG; }
            edge->SetCurve(line,TRUE); line->SetOwner(edge);
        }
        if (variant == 2 || variant == 3)
            edge->ResetPrimaryEdgeuse(edge->GetPrimaryEdgeuse()->GetMate());
        edge->GetStartVertex()->SetTolerance(start_tol,FALSE);
        edge->GetEndVertex()->SetTolerance(end_tol,FALSE);
    }
    if (export_only) {
        std::vector<double> result={(double)g.find(8,start_vu),(double)g.find(8,end_vu),0.0};
        export_graph(b,g,result);
        return sc_copy_out(result.data(),result.size(),out,cap,len);
    }
    b->m_bEditingEnabled = editing ? TRUE : FALSE;
    std::vector<double> record;
    if (op == 8) {
        if (g.faces.empty()) return SC_ERR_ARG;
        SmFace* face=g.faces[selector % g.faces.size()];
        SmSurface* surface=face->GetSurface();
        if (!surface) return SC_ERR_ARG;
        double y=variant == 1 ? 2.0 : 0.5;
        double x0=variant == 2 ? 0.25 : -1.0;
        double x1=variant == 2 ? 0.75 : 2.0;
        SmPoint3d p0,p1;
        if (surface->EvaluatePoint(SmPoint2d(x0,y),p0) != SM_SUCCESS ||
            surface->EvaluatePoint(SmPoint2d(x1,y),p1) != SM_SUCCESS) return SC_ERR_ARG;
        SmLine curve(p0,p1,3,&the_context());
        SmExtent1d interval(-3.0,7.0);
        status=curve.EditParameterization(interval,FALSE);
        if (status != SM_SUCCESS) return SC_ERR_ARG;
        SmTArray<SmBSplineCurve*> uv;
        SmObjsDelete<SmBSplineCurve*> cleanup(&uv);
        double max_dist=0.0,deviation=0.0;
        for (double factor : {1.0,10.0,300.0}) {
            // Deliberately call through SmSurface*: creating-UV-trim is the
            // TRUE base-class default used by MakeEdgeInFaceWithVU.
            status=surface->DropCurve(the_context(),face->GetUVDomain(),curve,
                interval,face->GetTolerance()*factor,max_dist,deviation,uv,TRUE,nullptr);
            if (status != SM_SUCCESS || uv.GetSize() != 0) break;
        }
        if (status == SM_SUCCESS && uv.GetSize() == 0)
            status=surface->DropAndTrimCurve(the_context(),face->GetUVDomain(),curve,
                interval,face->GetTolerance()*100.0,max_dist,deviation,uv,TRUE,TRUE,nullptr);
        record={max_dist,deviation,(double)uv.GetSize()};
        for (ULONG j=0;j<uv.GetSize();j++) {
            SmExtent1d d=uv[j]->GetNaturalInterval();
            record.push_back(d.GetMin()); record.push_back(d.GetMax());
            for (double t : {d.GetMin(),d.GetMin()+0.5*(d.GetMax()-d.GetMin()),d.GetMax()}) {
                SmPoint3d p;
                if (uv[j]->EvaluatePoint(t,p) != SM_SUCCESS) return SC_ERR_ARG;
                record.push_back(p.x); record.push_back(p.y); record.push_back(p.z);
            }
        }
    } else if (op == 0) {
        SmRegion* result=nullptr;
        status=b->FindInfiniteRegion(result, nullptr, nullptr);
        record.push_back(g.find(0,result));
    } else if (op == 1) {
        SmFace* face = variant == 1 || g.faces.empty() ? nullptr : g.faces[selector % g.faces.size()];
        // under a topology change tracker (sc_tracked.h): the record is its lists and events
        TrackedEdit te; // (after built: goes before it)
        te.begin(b,g);
        status=b->OrientSheetSurfaces(face,same ? TRUE : FALSE);
        te.finish(record);
    } else if (op == 2) {
        if (g.regions.empty()) return SC_ERR_ARG;
        SmRegion* region=variant == 1 ? nullptr : g.regions[selector % g.regions.size()];
        SmShell* shell=nullptr; SmVertex* vertex=nullptr;
        status=b->MakeShellVertex(region,SmPoint3d(point[0],point[1],point[2]),shell,vertex);
        Graph fresh; walk(b,fresh,true);
        record={(double)fresh.find(1,shell),(double)fresh.find(9,vertex)};
    } else if (op == 3) {
        if (g.faces.empty()) return SC_ERR_ARG;
        SmLoop* loop=nullptr; SmVertex* vertex=nullptr;
        status=b->MakeVertexLoop(g.faces[selector % g.faces.size()],SmPoint3d(point[0],point[1],point[2]),loop,vertex,
            same ? SM_OT_SAME : SM_OT_OPPOSITE);
        Graph fresh; walk(b,fresh,true);
        record={(double)fresh.find(5,loop),(double)fresh.find(9,vertex)};
    } else if (op == 4) {
        SmEdge *e1=nullptr,*e2=nullptr; SmVertex* vertex=nullptr;
        status=b->MakeVertexSplitEdge(g.edges[selector % g.edges.size()],parameter,e1,e2,vertex);
        Graph fresh; walk(b,fresh,true);
        record={(double)fresh.find(7,e1),(double)fresh.find(7,e2),(double)fresh.find(9,vertex)};
    } else if (op == 6 || op == 7) {
        SmEdge* edge=g.edges[selector % g.edges.size()];
        SmPointClassification pc(SmZoneTol3d(1e-7), &the_context());
        pc.SetClassObject(op == 6 ? SM_PC_EDGE : SM_PC_VERTEX,
            op == 6 ? (SmObject*)edge : (SmObject*)edge->GetStartVertex());
        pc.SetTParam(parameter,FALSE);
        if (variant == 4) pc.UnSetParameters();
        if (!same) pc.SetSSSPoint(SmPoint3d(point[0],point[1],point[2]));
        SmTArray<SmEdge*> edges; SmTArray<SmVertex*> vertices;
        status=pc.MergeIntoObject(SmPoint3d(point[0],point[1],point[2]),&edges,&vertices);
        Graph fresh; walk(b,fresh,true);
        record={(double)pc.GetPointClass(),(double)fresh.find(9,pc.GetVertexObject()),
            (double)edges.GetSize(),(double)vertices.GetSize(),flg(pc.AreParametersSet()),pc.GetTParam()};
    } else {
        SmFace* face = variant == 1 || g.faces.empty() ? nullptr : g.faces[selector % g.faces.size()];
        SmLine* curve = nullptr;
        if (variant != 2) {
            SmPoint3d p(0,0,0), q(1,1,0);
            if (start_vu && end_vu) { p=start_vu->GetVertex()->GetPoint(); q=end_vu->GetVertex()->GetPoint(); }
            curve = new (the_context()) SmLine(p,q,3,&the_context());
        }
        SmEdge* edge=nullptr; SmLoop* loop=nullptr; SmFace* new_face=nullptr;
        status=b->MakeEdgeInFaceWithVU(face, variant == 3 ? nullptr : start_vu,
            variant == 4 ? nullptr : end_vu, curve, nullptr, SmExtent1d(0,1),
            SM_OT_SAME, SmZoneTol3d(1e-7),edge,loop,new_face,nullptr);
        // The curve is consumed only on success; errors in this controlled fixture
        // happen before ownership changes. The successful edge owns it thereafter.
        if (status != SM_SUCCESS && curve && curve->GetOwner()==nullptr) delete curve;
        Graph fresh; walk(b,fresh,true);
        record={(double)fresh.find(7,edge),(double)fresh.find(5,loop),(double)fresh.find(3,new_face)};
    }
    Graph fresh; walk(b,fresh,true);
    std::vector<double> result={st(status),(double)record.size()};
    result.insert(result.end(),record.begin(),record.end());
    result.push_back(0.0); export_graph(b,fresh,result);
    return sc_copy_out(result.data(),result.size(),out,cap,len);
    SC_CATCH
}
