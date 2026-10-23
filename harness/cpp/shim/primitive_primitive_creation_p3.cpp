// SPDX-License-Identifier: Apache-2.0
// Include renamed upstream implementation to call its file-local helpers exactly.
#include "sc_topology_fixture.h"
#include <memory>
#include <cmath>
#include <SmTopologySweep.h>
#include <SmPrimitiveCreation.h>
#include <SmTransSweepGeometry.h>
#include <SmRotationalSweepGeometry.h>
#include <SmTopologyIntersector.h>
#include <SmCompositeCurve.h>
#include <SmSphere.h>
#include <SmCone.h>
#include <SmTorus.h>
#include <SmSurfOfExtrusion.h>
#include <SmCircle.h>
#include <SmGraphicsOutput.h>
#include <SmGeomUtility.h>
#include <SmCylinder.h>
#include <SmAssertArray.h>
#include <SmMerge.h>
#include <nurbs.h>
// Force a fresh declaration for the renamed class after dependencies are loaded.
#undef __SMPRIMITIVECREATION_H__
#define SmPrimitiveCreation sc_pc3_PrimitiveCreation
#define SmSweepOptions sc_pc3_SweepOptions
#define Sm2DBooleanOperationType sc_pc3_BooleanType
#define SM_2D_UNION sc_pc3_UNION
#define SM_2D_INTERSECTION sc_pc3_INTERSECTION
#define SM_2D_DIFFERENCE sc_pc3_DIFFERENCE
#define SM_2D_EXCLUSIVE_OR sc_pc3_XOR
#define SM_2D_MERGE sc_pc3_MERGE
#define sm_CreateAnalyticRotationalSweep sc_pc3_AnalyticRotationalSweep
#include "../../../upstream/source/SMLib/src/SmPrimitiveCreation.cpp"
#undef sm_CreateAnalyticRotationalSweep
#undef SmPrimitiveCreation

namespace {
SmBSplineCurve* pc3_curve(const double* data) {
    SmBSplineCurve* curve = nullptr;
    if(data[0]==0) {
        SmBSplineCurve::CreateLineSegment(the_context(),3,
            SmVector3d(data[1],data[2],data[3]),SmVector3d(data[4],data[5],data[6]),curve);
    } else {
        SmAxis2Placement frame(SmVector3d(data[1],data[2],data[3]),SmVector3d(1,0,0),SmVector3d(0,1,0));
        SmBSplineCurve::CreateCircleSegment(the_context(),3,frame,data[4],data[5],data[6],SM_CO_QUADRATIC,curve);
    }
    return curve;
}
bool pc3_sampleable(SmBSplineCurve* curve) {
    if(!curve) return false;
    const auto ivl=curve->GetNaturalInterval();
    for(int i=0;i<=4;i++) {
        SmVector3d v[2];
        if(curve->Evaluate(ivl.Evaluate(i/4.0),1,TRUE,v)!=SM_SUCCESS) return false;
        for(auto& p:v) if(!std::isfinite(p.x)||!std::isfinite(p.y)||!std::isfinite(p.z)) return false;
    }
    return true;
}
void pc3_samples(std::vector<double>& out,SmBSplineCurve* curve) {
    const auto ivl=curve->GetNaturalInterval();
    out.push_back(ivl.GetMin());out.push_back(ivl.GetMax());
    out.push_back(curve->GetDegree());out.push_back(curve->GetNumberControlPoints());
    out.push_back(curve->IsRational()!=FALSE);
    for(int i=0;i<=4;i++) {
        SmVector3d v[2];curve->Evaluate(ivl.Evaluate(i/4.0),1,TRUE,v);
        put_vec(out,v[0]);put_vec(out,v[1]);
    }
}
struct Pc3Probe : sc_pc3_PrimitiveCreation {
    explicit Pc3Probe(SmRegion* region):sc_pc3_PrimitiveCreation(region){}
    using sc_pc3_PrimitiveCreation::CreateLinearSweep;
    using sc_pc3_PrimitiveCreation::CreateRotationalSweep;
};
}
SC_EXPORT int32_t sc_primitive_primitive_creation_p3_eval(int32_t op,const double* data,int64_t n,
    double* output,int64_t cap,int64_t* length) {
    if(!data||n<0||n>1024||op<0||op>7) return SC_ERR_ARG;
    SC_TRY
    std::vector<double> out;
    if(op==0) {
        if(n!=41) return SC_ERR_ARG;
        std::unique_ptr<SmBSplineCurve> offset(pc3_curve(data+20));
        SmTArray<SmBSplineCurve*> originals;
        originals.Add(pc3_curve(data+27));originals.Add(pc3_curve(data+34));
        if(!pc3_sampleable(offset.get())||!pc3_sampleable(originals[0])||!pc3_sampleable(originals[1])) {
            for(ULONG i=0;i<originals.GetSize();i++) delete originals[i];
            return SC_ERR_ARG;
        }
        // UB-57: the apex branch ignores a failed three-point circle solve,
        // then uses its undefined centre/radius. Reproduce only its selected
        // arithmetic on an offset copy, so the tested call is not mutated twice.
        bool safe=true;
        const int source_index=(int)data[2];
        if(data[1]!=0 && data[0]==0 && data[27+7*source_index]==1) {
            SmBSplineCurve offset_copy(*offset);
            offset_copy.DegreeReduction(data[8],TRUE);
            auto source=originals[(ULONG)source_index];
            auto ivl=source->GetNaturalInterval();
            SmPoint3d start,end,offset_start;
            source->EvaluatePoint(ivl.GetMin(),start);source->EvaluatePoint(ivl.GetMax(),end);
            offset_copy.EvaluatePoint(offset_copy.GetNaturalInterval().GetMin(),offset_start);
            if(start.DistanceBetweenSquared(offset_start)>end.DistanceBetweenSquared(offset_start)) {
                SmPoint3d mid,center;SmVector3d x,y;double radius=0;ULONG dim=3;SmBoolean closed=FALSE;
                source->EvaluatePoint(ivl.GetMid(),mid);
                safe=smgu_CircleFrom3Points(start,mid,end,center,x,y,ivl,radius,dim,closed)==SM_SUCCESS;
                if(safe) for(int axis=0;axis<3;axis++)
                    safe=safe && std::isfinite(radius/data[10]*data[11+axis]);
            }
        }
        if(!safe) {for(ULONG i=0;i<originals.GetSize();i++) delete originals[i];return SC_ERR_ARG;}
        SmTArray<SmCurve*> parents;parents.Add(originals[0]);parents.Add(originals[1]);
        std::unique_ptr<SmCompositeCurve> composite(new(the_context()) SmCompositeCurve(3,parents,FALSE));
        if(data[1]!=0) offset->AddAttribute(new(the_context()) SmOffsetMapAttribute(SM_AI_OFFSETMAP,
            data[0]!=0,FALSE,0,(ULONG)data[2],(ULONG)data[3],NULL,0.0));
        SmTArray<SmBSplineCurve*> unmarried;
        unmarried.Add(data[6]!=0?originals[0]:NULL);unmarried.Add(data[7]!=0?originals[1]:NULL);
        SmBoolean missing=data[5]!=0;
        SmAxis2Placement move(SmVector3d(data[11],data[12],data[13]),
            SmVector3d(data[14],data[15],data[16]),SmVector3d(data[17],data[18],data[19]));
        SmBSplineSurface* raw=nullptr;
        SmStatus status=sm_CreateRuledSurfaceFromOffsetCurve(the_context(),offset.get(), originals,
            composite.get(),data[8],data[9],data[10],move,data[4]!=0,unmarried,missing,raw);
        std::unique_ptr<SmBSplineSurface> surface(raw);
        out={double(status==SM_SUCCESS?0:status),double(missing!=FALSE),double(unmarried[0]!=NULL),double(unmarried[1]!=NULL)};
        pc3_samples(out,offset.get());pc3_samples(out,originals[0]);pc3_samples(out,originals[1]);
        out.push_back(surface?1:0);
        if(surface) {
            const auto uv=surface->GetNaturalUVDomain();
            out.push_back(uv.GetMin().x);out.push_back(uv.GetMin().y);out.push_back(uv.GetMax().x);out.push_back(uv.GetMax().y);
            for(int u=0;u<=4;u++) for(int v=0;v<=4;v++) {
                SmPoint3d point;surface->EvaluatePoint(uv.Evaluate(u/4.0,v/4.0),point);put_vec(out,point);
            }
            out.push_back(surface->FindAttribute(SM_AI_OFFSETMAP)!=NULL);
        }
    } else {
        // Real public entry points on one freshly constructed native B-rep.
        if(n<10||(n-10)%7!=0) return SC_ERR_ARG;
        if(op==4 && n==10) return SC_ERR_ARG; // UB-78: zero skin cross-sections
        SmBrep brep(SM_USE_DEFAULT,SM_USE_DEFAULT,const_cast<SmContext*>(&the_context()));Pc3Probe primitive(brep.GetInfiniteRegion());
        SmTArray<SmCurve*> curves;
        for(int64_t i=10;i<n;i+=7) {
            auto curve=pc3_curve(data+i);
            if(!pc3_sampleable(curve)) { delete curve;return SC_ERR_ARG; }
            curves.Add(curve);
        }
        SmTArray<SmFace*> starts,sides,ends;
        SmStatus status=SM_ERR;
        if(op==1) status=primitive.CreateLinearSweep(curves,SmVector3d(data[0],data[1],data[2]),
            data[3],(ULONG)data[4],data[5]!=0,data[6]!=0);
        if(op==2) status=primitive.CreateRotationalSweep(curves,SmVector3d(data[0],data[1],data[2]),
            SmVector3d(data[3],data[4],data[5]));
        if(op==3) status=primitive.CreateRotationalSweep(curves,SmVector3d(data[0],data[1],data[2]),
            SmVector3d(data[3],data[4],data[5]),data[6],(ULONG)data[7],data[8]!=0,data[9]!=0);
        if(op==4) {
            SmTArray<SmBSplineCurve*> profiles;
            for(ULONG i=0;i<curves.GetSize();i++) profiles.Add((SmBSplineCurve*)curves[i]);
            SmTArray<SmFace*> input_faces;
            status=primitive.CreateSkinFromFaces(input_faces,profiles,data[0],(ULONG)data[1],sides);
        }
        if(op==5) {
            SmBSplineCurve* path=nullptr;SmBSplineCurve::CreateLineSegment(the_context(),3,
                SmVector3d(0,0,0),SmVector3d(0,0,2),path);
            std::unique_ptr<SmBSplineCurve> clean(path);
            sc_pc3_SweepOptions options;options.bTranslationalSweep=TRUE;
            status=primitive.CreateCurveSweep(curves,path,NULL,NULL,data[0],&options,starts,sides,ends,data[1]!=0);
        }
        if(op==6) {
            SmTArray<SmCurve*> paths,profiles;
            for(ULONG i=0;i<curves.GetSize();i++) {
                if(i<(ULONG)data[2]) paths.Add(curves[i]); else profiles.Add(curves[i]);
            }
            status=primitive.CreateSwungPrimitive(paths,profiles,data[0],data[1]);
        }
        if(op==7) {
            SmFace* face=NULL;
            status=primitive.CreateBlendPrimitive(NULL,NULL,face,(SmCurvatureType)(int)data[0],
                (SmBlendEndType)(int)data[1],(ULONG)data[2],data[3],data[4]);
        }
        out={double(status==SM_SUCCESS?0:status),double(brep.m_bEditingEnabled!=FALSE),double(the_context().GetDoingBoolean()!=FALSE),
            double(starts.GetSize()),double(sides.GetSize()),double(ends.GetSize())};
        // The topology the entry point left in the B-rep.
        SmTArray<SmFace*> faces;SmTArray<SmEdge*> edges;SmTArray<SmVertex*> vertices;
        brep.GetFaces(faces);brep.GetEdges(edges);brep.GetVertices(vertices);
        out.push_back(double(faces.GetSize()));out.push_back(double(edges.GetSize()));
        out.push_back(double(vertices.GetSize()));
        // Only non-consuming entry points retain caller ownership of these curves.
        for(ULONG i=0;i<curves.GetSize();i++)
            if(curves[i] && (op>=4 || curves[i]->GetOwner()==NULL)) delete curves[i];
    }
    return sc_copy_out(out.data(),(int64_t)out.size(),output,cap,length);
    SC_CATCH
}
