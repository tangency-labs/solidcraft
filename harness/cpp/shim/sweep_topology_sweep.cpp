// SPDX-License-Identifier: Apache-2.0
// Real upstream map/callback scripts and protected vertex sweep helpers.
#include "sc_shim.h"
#include <SmTopologySweep.h>
#include <SmSweepGeometryCreation.h>
#include <SmTransSweepGeometry.h>
#include <SmRotationalSweepGeometry.h>
#include <SmContext.h>
#include <SmRegion.h>
#include <SmLine.h>
#include <SmAttribute.h>
#include <SmApiGeneral.h>
#include <SmApiPrimitives.h>
#include <SmShell.h>
#include <SmBrep.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <memory>
#include <vector>
#include <cmath>
#include <cstring>

struct ScVertex : SmVertex { ScVertex()=default; using SmVertex::SetPoint; ~ScVertex() override=default; };
struct ScEdge : SmEdge { ScEdge()=default; ~ScEdge() override=default; };
struct ScFace : SmFace { ScFace()=default; ~ScFace() override=default; };
struct ScRegion : SmRegion { ScRegion()=default; ~ScRegion() override=default; };

struct ScSweep : SmTopologySweep {
    using SmTopologySweep::SmTopologySweep;
    using SmTopologySweep::DoSweepVerticesHigher;
    using SmTopologySweep::DoSweepVerticesSame;
    using SmTopologySweep::TagProfile;
};

SC_EXPORT int32_t sc_sweep_topology_sweep_eval(const int64_t* steps, int64_t n,
 const double* vector, int64_t kind, int64_t* out, int64_t cap, int64_t* len, double* values, int64_t value_cap) {
    if (!steps || !vector || !len || !values || n<0 || n>256 || value_cap < n*13 || kind<0 || kind>2) return SC_ERR_ARG;
    for(int j=0;j<3;++j) if(!std::isfinite(vector[j])) return SC_ERR_ARG;
    for(int64_t i=0;i<n;++i) {
        auto p=steps+4*i;
        if(p[0]<0 || p[0]>16 || p[1]<0 || p[1]>64 || p[2]<0 || p[2]>64) return SC_ERR_ARG;
        if(p[0]==14 && (p[1]<16 || p[1]>=32 || p[2]<16 || p[2]>=32)) return SC_ERR_ARG;
        if(p[0]==5 && (p[3]<0 || p[3]>3)) return SC_ERR_ARG;
        if((p[0]==10 || p[0]==11) && p[1]>=16) return SC_ERR_ARG;
    }
    SC_TRY
    SmContext context;
    SmBrep target(SM_USE_DEFAULT,SM_USE_DEFAULT,&context);
    SmSweepGeometryCreation base;
    SmTranslationalSweepGeometry translation(SmVector3d(vector[0],vector[1],vector[2]));
    SmRotationalSweepGeometry rotation(SmPoint3d(0,0,0),SmVector3d(0,0,1),90.0,SM_ZONE_TOL_3D);
    SmSweepGeometryCreation* g=kind==0 ? &base : kind==1 ? (SmSweepGeometryCreation*)&translation : (SmSweepGeometryCreation*)&rotation;
    ScSweep sweep(*g);
    std::vector<std::unique_ptr<SmCurve>> curves;
    std::vector<std::unique_ptr<SmTopology>> pool;
    for(int i=0;i<64;++i) {
        if(i<16) { auto p=new(&target) ScVertex; p->SetPoint(SmPoint3d(vector[0]*(i%8),vector[1]*(i%8),vector[2]*(i%8))); pool.emplace_back(p); }
        else if(i<32) {
            auto edge=new(&target) ScEdge; SmLine* line=nullptr;
            if(SmLine::CreateLineSegment(context,3,SmPoint3d(0,0,0),SmPoint3d(1,0,0),line)!=SM_SUCCESS || !line) return SC_ERR_ARG;
            curves.emplace_back(line); edge->SetCurve(line,FALSE,FALSE,FALSE);
            if(i%2) line->AddAttribute(new(context) SmTagAttribute(SM_AI_TAG,700+i,12+i));
            pool.emplace_back(edge);
        }
        else if(i<48) pool.emplace_back(new(&target) ScFace);
        else pool.emplace_back(new(&target) ScRegion);
    }
    auto get=[&](int64_t i)->SmTopology* { return i==64 ? nullptr : pool[i].get(); };
    auto id=[&](SmTopology* p)->int { if(!p) return 64; for(int i=0;i<64;++i) if(pool[i].get()==p) return i; return -2; };
    auto maps=[&](int j)->SmMapPtrToPtr<SmTopology,SmTopology>* {
        switch(j) { case 0:return sweep.GetMapToSame();case 1:return sweep.GetMapToHigher();case 2:return sweep.GetMapFromSame();default:return sweep.GetMapFromLower(); }
    };
    std::vector<int64_t> result;
    for(int64_t i=0;i<n;++i) {
        auto p=steps+4*i; SmStatus status=SM_SUCCESS; SmBoolean keep=TRUE; double distance=-123;
        auto a=get(p[1]); auto b=get(p[2]);
        SmAxis2Placement frame; bool has_frame=false;
        switch(p[0]) {
            case 0:sweep.SetSame(a,b);break;
            case 1:sweep.SetHigher(a,b);break;
            case 2:{ SmSweepStitchCallback callback(&sweep);status=callback.SelectTopologyToKeep(a,b,vector[0],distance,keep);break; }
            case 3:sweep.RemoveFromToSamePriv(a);break;
            case 4:sweep.RemoveFromFromSamePriv(a);break;
            case 5:maps(p[3])->Insert(a,b);break;
            case 6:sweep.SetRepetitions(p[3]);break;
            case 7:{SmBoolean closed=TRUE;status=g->IsSweepClosed(p[3],closed);keep=closed;break;}
            case 8:{status=g->GetStartCoordSystem(SmPoint3d(0,0,0),frame);has_frame=status==SM_SUCCESS;break;}
            case 9:{status=g->GetSweepTransform(frame);has_frame=status==SM_SUCCESS;break;}
            case 16:{status=g->GetEndCoordSystem(SmPoint3d(vector[0],vector[1],vector[2]),frame);has_frame=status==SM_SUCCESS;break;}
            case 10:{SmTArray<SmVertex*> vs;vs.Add((SmVertex*)a);status=sweep.DoSweepVerticesSame(target.GetInfiniteRegion(),vs);break;}
            case 11:{SmTArray<SmVertex*> vs;vs.Add((SmVertex*)a);status=sweep.DoSweepVerticesHigher(target.GetInfiniteRegion(),vs);break;}
            case 15:{
                sweep.SetDoStitching(FALSE);sweep.SetDoMerge(FALSE);sweep.SetRepetitions(1);
                SmShell* shell=nullptr;SmVertex* vertex=nullptr;
                if(target.MakeShellVertex(target.GetInfiniteRegion(),SmPoint3d(0,0,0),shell,vertex)!=SM_SUCCESS || !vertex) return SC_ERR_ARG;
                SmTArray<SmVertex*> vertices;vertices.Add(vertex);
                status=sweep.DoSweep(&target,target.GetInfiniteRegion(),NULL,NULL,&vertices,NULL,NULL,NULL);
                break;
            }
            case 14:{SmTArray<SmEdge*> es;es.Add((SmEdge*)a);es.Add((SmEdge*)b);status=sweep.TagProfile(NULL,&es,NULL,NULL,NULL,NULL);break;}
            case 13:status=sweep.DoSweep(NULL,NULL);break;
            case 12:sweep.SetTagging(p[3]%2!=0,p[3]);sweep.SetDoMerge(p[3]%2!=0);sweep.SetDoStitching((p[3]/2)%2!=0);break;
        }
        auto output_values=values+13*i; output_values[0]=distance;
        for(int j=1;j<13;++j) output_values[j]=0.0;
        if(has_frame) {
            auto origin=frame.GetOrigin();auto x=frame.GetXAxis();auto y=frame.GetYAxis();auto z=frame.GetZAxis();
            int j=1;for(auto v : {origin,x,y,z}) { output_values[j++]=v[0];output_values[j++]=v[1];output_values[j++]=v[2]; }
        }
        result.push_back(status==SM_SUCCESS ? 0 : status);result.push_back(keep);result.push_back(distance);
    }
    long tag=0; result.push_back(sweep.GetTagging(tag));result.push_back(tag);
    uint64_t repetitions=sweep.GetRepetitions();int64_t repetition_bits=0;std::memcpy(&repetition_bits,&repetitions,sizeof(repetition_bits));result.push_back(repetition_bits);result.push_back(sweep.GetDoMerge());result.push_back(sweep.GetDoStitching());
    for(int m=0;m<4;++m) { auto map=maps(m);result.push_back(map->Count());
        for(int j=0;j<=64;++j) {result.push_back(map->Contains(get(j)));result.push_back(id(map->At(get(j))));}
    }
    for(int j=16;j<32;++j) {
        auto tag=(SmTagAttribute*)pool[j]->FindAttribute(SM_AI_TAG);
        result.push_back(tag ? 1 : 0);
        for(int k=0;k<8;++k) result.push_back(tag ? tag->GetLongElementsAddress()[k] : 0);
    }
    return sc_copy_out(result.data(),result.size(),out,cap,len);
    SC_CATCH
}

// Whole-B-rep DoSweep (all six target arrays NULL): an SmApiCreateBox solid swept
// by a translation or rotation. in = [length, width, height, kind (1 translation,
// 2 rotation), vx, vy, vz, angle_deg, stitch, merge, repetitions]; the rotation is
// about the axis (vx, vy, vz) through the origin. Out (doubles): status, then on
// success the B-rep's region, shell, face, loop, edge and vertex counts, each
// vertex point in GetVertices order and, per GetEdges edge, its two vertex indices
// (-1 for none).
SC_EXPORT int32_t sc_sweep_topology_sweep_whole(const double* in, int64_t n_in, double* out,
                                                int64_t cap, int64_t* out_len) {
    if (!in || !out_len || n_in != 11) return SC_ERR_ARG;
    for (int64_t i = 0; i < n_in; ++i) if (!std::isfinite(in[i])) return SC_ERR_ARG;
    if (!(in[0] > 0 && in[1] > 0 && in[2] > 0) || (in[3] != 1 && in[3] != 2)
        || in[10] < 0 || in[10] > 4 || in[10] != std::floor(in[10])) return SC_ERR_ARG;
    SC_TRY
    SmApiGetOrCreateContext();
    SmBrep* box = nullptr;
    std::vector<double> o;
    SmStatus status = SmApiCreateBox(SmVector3d(0, 0, 0), in[0], in[1], in[2], box);
    std::unique_ptr<SmBrep> owned(box);
    if (status != SM_SUCCESS || !box) { o.push_back(status == SM_SUCCESS ? SM_ERR : status); return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len); }
    SmVector3d v(in[4], in[5], in[6]);
    SmTranslationalSweepGeometry translation(v);
    SmRotationalSweepGeometry rotation(SmPoint3d(0, 0, 0), v, in[7], SM_ZONE_TOL_3D);
    SmSweepGeometryCreation* g = in[3] == 1 ? (SmSweepGeometryCreation*)&translation : (SmSweepGeometryCreation*)&rotation;
    SmTopologySweep sweep(*g, in[8] != 0, FALSE, FALSE, in[9] != 0);
    sweep.SetRepetitions((ULONG)in[10]);
    status = sweep.DoSweep(box, box->GetInfiniteRegion(), NULL, NULL, NULL, NULL, NULL, NULL);
    o.push_back(status == SM_SUCCESS ? 0 : status);
    if (status == SM_SUCCESS) {
        SmTArray<SmRegion*> regions; box->GetRegions(regions);
        SmTArray<SmShell*> shells; box->GetShells(shells);
        SmTArray<SmFace*> faces; box->GetFaces(faces);
        SmTArray<SmLoop*> loops; box->GetLoops(loops);
        SmTArray<SmEdge*> edges; box->GetEdges(edges);
        SmTArray<SmVertex*> vertices; box->GetVertices(vertices);
        for (ULONG n : {regions.GetSize(), shells.GetSize(), faces.GetSize(), loops.GetSize(), edges.GetSize(), vertices.GetSize()}) o.push_back(n);
        for (ULONG i = 0; i < vertices.GetSize(); ++i) { auto p = vertices[i]->GetPoint(); o.push_back(p.x); o.push_back(p.y); o.push_back(p.z); }
        auto index = [&](SmVertex* p) -> double { for (ULONG i = 0; i < vertices.GetSize(); ++i) if (vertices[i] == p) return (double)i; return -1; };
        for (ULONG i = 0; i < edges.GetSize(); ++i) { o.push_back(index(edges[i]->GetStartVertex())); o.push_back(index(edges[i]->GetEndVertex())); }
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}

// ---- Non-manifold bodies (prog_test smSweepTest.cpp) --------------------------------------
//
// sc_sweep_topology_sweep_body builds one of these bodies with SMLib calls in the
// order prog_test makes them, then runs the whole-B-rep DoSweep (all six target
// arrays NULL) on it:
//   0  SmApiCreateBox(origin, 4, 3, 2) plus a shell vertex at (10,0,0) extended by
//      MakeWireEdgeVertex to (15,2,0) and a shell vertex at (-5,0,0)
//   1  my_create_test_brep: my_test_tsurf_creation (smTopoTest.cpp; a 10x10 B-spline
//      plane face with a notched outer loop, a triangular and a circular inner loop),
//      then MakeVertexLoop at (3.5,8.5,0), MakeEdgeInFace from vertex 0 to vertex 3,
//      MakeWireEdgeVertex from vertex 1 to (15,2,0) and MakeShellVertex at (-5,0,0)
//   2  an empty B-rep (no topology at all)
//   3  a single shell vertex at (-5,0,0)
//   4  a single wire edge from a shell vertex at (10,0,0) to (15,2,0)
//   5  SmApiCreateBox(origin, 4, 3, 2) alone
//   6  my_test_tsurf_creation alone (the face of body 1 without its additions)
//
// in = [body, kind (1 translation, 2 rotation), vx, vy, vz, angle_deg, cx, cy, cz,
// eps, stitch, merge, repetitions, tagging, tag_id, targets]; the translation is
// (vx,vy,vz), the rotation is about the axis (vx,vy,vz) through (cx,cy,cz) with
// tolerance eps. targets 0 sweeps the whole B-rep (all six target arrays NULL), 1
// passes every GetFaces face as the faces-higher list (the others NULL), 2 takes the
// six lists from seven more inputs (n_in 23): [faces higher, edges higher, vertices
// higher, faces same, edges same, vertices same, null_region], each list selector
// -1 NULL, 0 every GetFaces/GetEdges/GetVertices entity, 1 GetWireEdges (edges) or
// GetShellVertices (vertices), 2 an empty array, 3 every entity but the last (as
// prog_test case 1 builds sFaces), 10+i the single entity i of that Get list;
// null_region 1 passes a NULL region (the documented default, the infinite region).
// kind 0 (lists only) is a base SmSweepGeometryCreation, whose sweeps return SM_ERR.
// Out (doubles): status; on success the counts, vertex points and edge vertex
// indices of sc_sweep_topology_sweep_whole, then the wire-edge and shell-vertex
// counts, then per GetFaces face and per GetEdges edge its SM_AI_TAG
// [present, eight longs].

static SmStatus sc_test_tsurf_creation(const SmContext& crContext, SmBrep*& rpNewBrep)
{
    SmBrep* pBrep = new (crContext) SmBrep();
    rpNewBrep = pBrep;
    pBrep->SetTolerance(0.0001);
    SmBSplineSurface* pPlane = NULL;
    SER(SmBSplineSurface::CreateBilinearSurface(crContext,
        SmPoint3d(0,0,0), SmPoint3d(10,0,0), SmPoint3d(0,10,0), SmPoint3d(10,10,0), pPlane));
    SmSurface* pSurface;
    SER(pPlane->CopyAndAddAnalytics(crContext, pSurface));
    delete pPlane; pPlane = NULL;
    pPlane = SM_CAST_PTR(SmBSplineSurface, pSurface);
    if (!pPlane) return SM_ERR;
    SmTArray<SmCurve*> s3DCurves;
    SmTArray<SmOrientType> sCurveOrients;
    struct Seg { SmPoint3d a, b; SmOrientType o; };
    const Seg segs[] = {
        {SmPoint3d(0,0,0), SmPoint3d(10,0,0), SM_OT_SAME},
        {SmPoint3d(10,5,0), SmPoint3d(10,0,0), SM_OT_OPPOSITE},
        {SmPoint3d(10,5,0), SmPoint3d(5,5,0), SM_OT_SAME},
        {SmPoint3d(5,5,0), SmPoint3d(5,10,0), SM_OT_SAME},
        {SmPoint3d(5,10,0), SmPoint3d(0,10,0), SM_OT_SAME},
        {SmPoint3d(0,10,0), SmPoint3d(0,0,0), SM_OT_SAME},
        {SmPoint3d(1,5,0), SmPoint3d(1,8,0), SM_OT_SAME},
        {SmPoint3d(1,8,0), SmPoint3d(4,5,0), SM_OT_SAME},
        {SmPoint3d(4,5,0), SmPoint3d(1,5,0), SM_OT_SAME},
    };
    for (const Seg& s : segs) {
        SmBSplineCurve* pCurve = NULL;
        SER(SmBSplineCurve::CreateLineSegment(crContext, 3, s.a, s.b, pCurve));
        s3DCurves.Add(pCurve);
        sCurveOrients.Add(s.o);
    }
    SmAxis2Placement sRefFrame;
    sRefFrame.SetCanonical(SmPoint3d(6,2.5,0), SmVector3d(-1,0,0), SmVector3d(0,-1,0));
    SmBSplineCurve* pCurve = NULL;
    SER(SmBSplineCurve::CreateCircleSegment(crContext, 3, sRefFrame, 1.5, 0.0, 360.0, SM_CO_QUADRATIC, pCurve));
    s3DCurves.Add(pCurve);
    sCurveOrients.Add(SM_OT_OPPOSITE);
    SmTArray<ULONG> sLoops;
    sLoops.Add(6); sLoops.Add(3); sLoops.Add(1);
    SmTArray<SmPoint3d> sLoopPoints;
    SmRegion* pNewRegion; SmShell* pNewShell; SmFace* pNewFace;
    SER(pBrep->MakeFaceWithCurves(pBrep->GetInfiniteRegion(), sLoops, &s3DCurves, NULL,
        sCurveOrients, sLoopPoints, pPlane, pPlane->GetNaturalUVDomain(), SM_OT_SAME,
        pNewRegion, pNewShell, pNewFace));
    return SM_SUCCESS;
}

static SmStatus sc_create_test_brep(const SmContext& crContext, SmBrep*& rpBrep)
{
    SER(sc_test_tsurf_creation(crContext, rpBrep));
    SmTemporaryChangeValue<SmBoolean> sChangeDB(((SmContext*)&crContext)->GetDoingBooleanRef(), TRUE);
    SmTArray<SmVertex*> sVertices;
    rpBrep->GetVertices(sVertices);
    SmTArray<SmFace*> sFaces;
    rpBrep->GetFaces(sFaces);
    if (sFaces.GetSize() < 1 || sVertices.GetSize() < 4) return SM_ERR;
    SmFace* pFace = sFaces[0];
    SmLoop* pNewLoop; SmVertex* pNewVertex;
    SER(rpBrep->MakeVertexLoop(pFace, SmPoint3d(3.5,8.5,0.0), pNewLoop, pNewVertex));
    SmBSplineCurve* pLine = NULL;
    SER(SmBSplineCurve::CreateLineSegment(crContext, 3, SmPoint3d(0,0,0), SmPoint3d(5,5,0), pLine));
    SmEdge* pNewEdge; SmFace* pNewFace;
    rpBrep->m_bEditingEnabled = TRUE;
    SER(rpBrep->MakeEdgeInFace(pFace, sVertices[0], sVertices[3], pLine, NULL,
        pLine->GetNaturalInterval(), SM_OT_SAME, SM_ZONE_TOL_3D, pNewEdge, pNewLoop, pNewFace));
    pLine = NULL;
    SER(SmBSplineCurve::CreateLineSegment(crContext, 3, SmPoint3d(10,0,0), SmPoint3d(15,2.0,0), pLine));
    SER(rpBrep->MakeWireEdgeVertex(rpBrep->GetInfiniteRegion(), sVertices[1], pLine,
        pLine->GetNaturalInterval(), SM_OT_SAME, SmPoint3d(15,2,0), pNewEdge, pNewVertex));
    pLine = NULL;
    SmShell* pNewShell;
    SER(rpBrep->MakeShellVertex(rpBrep->GetInfiniteRegion(), SmPoint3d(-5.0,0,0), pNewShell, pNewVertex));
    return SM_SUCCESS;
}

// A shell vertex at (10,0,0) extended by a wire edge to (15,2,0).
static SmStatus sc_add_wire(const SmContext& ctx, SmBrep* pBrep)
{
    SmShell* pShell = NULL; SmVertex* pStart = NULL;
    SER(pBrep->MakeShellVertex(pBrep->GetInfiniteRegion(), SmPoint3d(10,0,0), pShell, pStart));
    SmBSplineCurve* pLine = NULL;
    SER(SmBSplineCurve::CreateLineSegment(ctx, 3, SmPoint3d(10,0,0), SmPoint3d(15,2,0), pLine));
    SmEdge* pEdge = NULL; SmVertex* pEnd = NULL;
    SER(pBrep->MakeWireEdgeVertex(pBrep->GetInfiniteRegion(), pStart, pLine,
        pLine->GetNaturalInterval(), SM_OT_SAME, SmPoint3d(15,2,0), pEdge, pEnd));
    return SM_SUCCESS;
}

static SmStatus sc_build_body(int body, const SmContext& ctx, SmBrep*& rpBrep)
{
    SmShell* pShell = NULL; SmVertex* pVertex = NULL;
    switch (body) {
    case 0:
        SER(SmApiCreateBox(SmVector3d(0,0,0), 4, 3, 2, rpBrep));
        SER(sc_add_wire(ctx, rpBrep));
        return rpBrep->MakeShellVertex(rpBrep->GetInfiniteRegion(), SmPoint3d(-5,0,0), pShell, pVertex);
    case 1:
        return sc_create_test_brep(ctx, rpBrep);
    case 2:
        rpBrep = new (ctx) SmBrep();
        return SM_SUCCESS;
    case 5:
        return SmApiCreateBox(SmVector3d(0,0,0), 4, 3, 2, rpBrep);
    case 6:
        return sc_test_tsurf_creation(ctx, rpBrep);
    case 3:
        rpBrep = new (ctx) SmBrep();
        return rpBrep->MakeShellVertex(rpBrep->GetInfiniteRegion(), SmPoint3d(-5,0,0), pShell, pVertex);
    default:
        rpBrep = new (ctx) SmBrep();
        return sc_add_wire(ctx, rpBrep);
    }
}

static void sc_put_tag(std::vector<double>& o, SmTopology* p)
{
    auto tag = (SmTagAttribute*)p->FindAttribute(SM_AI_TAG);
    o.push_back(tag ? 1 : 0);
    for (int k = 0; k < 8; ++k) o.push_back(tag ? (double)tag->GetLongElementsAddress()[k] : 0.0);
}

SC_EXPORT int32_t sc_sweep_topology_sweep_body(const double* in, int64_t n_in, double* out,
                                               int64_t cap, int64_t* out_len) {
    if (!in || !out_len || (n_in != 16 && n_in != 23)) return SC_ERR_ARG;
    for (int64_t i = 0; i < n_in; ++i) if (!std::isfinite(in[i])) return SC_ERR_ARG;
    const bool lists = n_in == 23;
    if (in[0] < 0 || in[0] > 6 || in[0] != std::floor(in[0])
        || (in[1] != 1 && in[1] != 2 && !(lists && in[1] == 0))
        || in[12] < 0 || in[12] > 4 || in[12] != std::floor(in[12])
        || std::fabs(in[14]) > 1e9 || in[14] != std::floor(in[14])
        || (lists ? in[15] != 2 : (in[15] != 0 && in[15] != 1))) return SC_ERR_ARG;
    if (lists) {
        for (int j = 16; j < 22; ++j)
            if (in[j] != std::floor(in[j]) || in[j] < -1 || in[j] > 10000 || (in[j] > 3 && in[j] < 10)) return SC_ERR_ARG;
        if (in[22] != 0 && in[22] != 1) return SC_ERR_ARG;
    }
    SC_TRY
    const SmContext& ctx = *SmApiGetOrCreateContext();
    SmBrep* brep = nullptr;
    std::vector<double> o;
    SmStatus status = sc_build_body((int)in[0], ctx, brep);
    std::unique_ptr<SmBrep> owned(brep);
    if (status != SM_SUCCESS || !brep) { o.push_back(status == SM_SUCCESS ? SM_ERR : status); return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len); }
    SmVector3d v(in[2], in[3], in[4]);
    SmTranslationalSweepGeometry translation(v);
    SmRotationalSweepGeometry rotation(SmPoint3d(in[6], in[7], in[8]), v, in[5], in[9]);
    SmSweepGeometryCreation base(in[9]);
    SmSweepGeometryCreation* g = in[1] == 0 ? &base : in[1] == 1 ? (SmSweepGeometryCreation*)&translation : (SmSweepGeometryCreation*)&rotation;
    SmTopologySweep sweep(*g, in[10] != 0, FALSE, FALSE, in[11] != 0);
    sweep.SetRepetitions((ULONG)in[12]);
    sweep.SetTagging(in[13] != 0, (long)in[14]);
    SmTArray<SmFace*> faces_higher;
    if (in[15] == 1) brep->GetFaces(faces_higher);
    SmTArray<SmFace*> list_faces[2]; SmTArray<SmEdge*> list_edges[2]; SmTArray<SmVertex*> list_vertices[2];
    SmTArray<SmFace*>* fp[2] = {NULL, NULL}; SmTArray<SmEdge*>* ep[2] = {NULL, NULL}; SmTArray<SmVertex*>* vp[2] = {NULL, NULL};
    if (lists) {
        // Fill one list from its selector; false for an index past the Get list.
        auto pick = [](auto& all, auto& wire, double sel, auto& list) -> bool {
            if (sel == 0) { for (ULONG i = 0; i < all.GetSize(); ++i) list.Add(all[i]); }
            else if (sel == 1) { for (ULONG i = 0; i < wire.GetSize(); ++i) list.Add(wire[i]); }
            else if (sel == 3) { for (ULONG i = 0; i + 1 < all.GetSize(); ++i) list.Add(all[i]); }
            else if (sel >= 10) { ULONG i = (ULONG)(sel - 10); if (i >= all.GetSize()) return false; list.Add(all[i]); }
            return true;
        };
        SmTArray<SmFace*> all_faces; brep->GetFaces(all_faces);
        SmTArray<SmEdge*> all_edges; brep->GetEdges(all_edges);
        SmTArray<SmEdge*> wire_edges; brep->GetWireEdges(wire_edges);
        SmTArray<SmVertex*> all_vertices; brep->GetVertices(all_vertices);
        SmTArray<SmVertex*> shell_vertices; brep->GetShellVertices(shell_vertices);
        for (int s = 0; s < 2; ++s) {
            double fs = in[16 + 3 * s], es = in[17 + 3 * s], vs = in[18 + 3 * s];
            if ((fs >= 0 && !pick(all_faces, all_faces, fs, list_faces[s]))
                || (es >= 0 && !pick(all_edges, wire_edges, es, list_edges[s]))
                || (vs >= 0 && !pick(all_vertices, shell_vertices, vs, list_vertices[s]))) return SC_ERR_ARG;
            if (fs >= 0) fp[s] = &list_faces[s];
            if (es >= 0) ep[s] = &list_edges[s];
            if (vs >= 0) vp[s] = &list_vertices[s];
        }
        status = sweep.DoSweep(brep, in[22] == 1 ? NULL : brep->GetInfiniteRegion(), fp[0], ep[0], vp[0], fp[1], ep[1], vp[1]);
    } else
    status = sweep.DoSweep(brep, brep->GetInfiniteRegion(), in[15] == 1 ? &faces_higher : NULL,
                           NULL, NULL, NULL, NULL, NULL);
    o.push_back(status == SM_SUCCESS ? 0 : status);
    if (status == SM_SUCCESS) {
        SmTArray<SmRegion*> regions; brep->GetRegions(regions);
        SmTArray<SmShell*> shells; brep->GetShells(shells);
        SmTArray<SmFace*> faces; brep->GetFaces(faces);
        SmTArray<SmLoop*> loops; brep->GetLoops(loops);
        SmTArray<SmEdge*> edges; brep->GetEdges(edges);
        SmTArray<SmVertex*> vertices; brep->GetVertices(vertices);
        for (ULONG n : {regions.GetSize(), shells.GetSize(), faces.GetSize(), loops.GetSize(), edges.GetSize(), vertices.GetSize()}) o.push_back(n);
        for (ULONG i = 0; i < vertices.GetSize(); ++i) { auto p = vertices[i]->GetPoint(); o.push_back(p.x); o.push_back(p.y); o.push_back(p.z); }
        auto index = [&](SmVertex* p) -> double { for (ULONG i = 0; i < vertices.GetSize(); ++i) if (vertices[i] == p) return (double)i; return -1; };
        for (ULONG i = 0; i < edges.GetSize(); ++i) { o.push_back(index(edges[i]->GetStartVertex())); o.push_back(index(edges[i]->GetEndVertex())); }
        SmTArray<SmEdge*> wires; brep->GetWireEdges(wires);
        SmTArray<SmVertex*> shell_vertices; brep->GetShellVertices(shell_vertices);
        o.push_back(wires.GetSize()); o.push_back(shell_vertices.GetSize());
        for (ULONG i = 0; i < faces.GetSize(); ++i) sc_put_tag(o, faces[i]);
        for (ULONG i = 0; i < edges.GetSize(); ++i) sc_put_tag(o, edges[i]);
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
