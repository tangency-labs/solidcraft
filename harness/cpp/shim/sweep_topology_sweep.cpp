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
        if(p[0]<0 || p[0]>15 || p[1]<0 || p[1]>64 || p[2]<0 || p[2]>64) return SC_ERR_ARG;
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
