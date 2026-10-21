// SPDX-License-Identifier: Apache-2.0
// Include renamed upstream implementation to call its file-local helpers exactly.
#include "sc_topology_fixture.h"
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
#define SmPrimitiveCreation sc_pc1_PrimitiveCreation
#define SmSweepOptions sc_pc1_SweepOptions
#define Sm2DBooleanOperationType sc_pc1_BooleanType
#define SM_2D_UNION sc_pc1_UNION
#define SM_2D_INTERSECTION sc_pc1_INTERSECTION
#define SM_2D_DIFFERENCE sc_pc1_DIFFERENCE
#define SM_2D_EXCLUSIVE_OR sc_pc1_XOR
#define SM_2D_MERGE sc_pc1_MERGE
#define sm_CreateAnalyticRotationalSweep sc_pc1_AnalyticRotationalSweep
#include "../../../upstream/source/SMLib/src/SmPrimitiveCreation.cpp"
#undef sm_CreateAnalyticRotationalSweep
#undef SmPrimitiveCreation

namespace {
struct Pc1Probe : sc_pc1_PrimitiveCreation {
    Pc1Probe(SmRegion* region, long tag, bool analytics)
        : sc_pc1_PrimitiveCreation(region, tag, analytics) {}
    using sc_pc1_PrimitiveCreation::CreateCone;
    using sc_pc1_PrimitiveCreation::CreateSphere;
    using sc_pc1_PrimitiveCreation::CreateTorus;
    using sc_pc1_PrimitiveCreation::CreateLinearSweep;
    long tag() const { return m_lTagID; }
    SmRegion* region() const { return m_pRegion; }
};
SmBSplineCurve* pc1_curve(const double* data) {
    SmBSplineCurve* curve = nullptr;
    if (data[0] == 0) {
        SmBSplineCurve::CreateLineSegment(the_context(), 3,
            SmVector3d(data[1],data[2],data[3]), SmVector3d(data[4],data[5],data[6]), curve);
    } else {
        SmAxis2Placement frame(SmVector3d(data[1],data[2],data[3]),SmVector3d(1,0,0),SmVector3d(0,1,0));
        SmBSplineCurve::CreateCircleSegment(the_context(),3,frame,data[4],data[5],data[6],SM_CO_QUADRATIC,curve);
    }
    return curve;
}
// Sweep fixtures cover the IsAnalytic/STEP-interval branch as well as B-splines.
SmCurve* pc1_sweep_curve(const double* data, bool analytic) {
    if(!analytic) return pc1_curve(data);
    const SmVector3d origin(data[1],data[2],data[3]);
    if(data[0]==0) return new(the_context()) SmLine(origin,
        SmVector3d(data[4],data[5],data[6]),3,&the_context());
    SmAxis2Placement frame(origin,SmVector3d(1,0,0),SmVector3d(0,1,0));
    SmExtent1d interval(data[5],data[6]);
    SmCircle* circle=nullptr;
    if(SmCircle::CreateCanonical(the_context(),frame,data[4],circle,&interval)!=SM_SUCCESS) {
        delete circle;
        return nullptr;
    }
    return circle;
}
// N_CrvExtendByDist's G1 fallback reads a third control point after a
// failed normalization, including on two-point lines. Check only the exact
// extension stages selected by ExtendPathCurves; unextended huge lines remain
// valid fixtures. No undefined reference result participates in comparison.
bool pc1_extension_safe(SmBSplineCurve* curve, int end) {
    auto nurb = (NL_CURVE*)curve->GetOrCreateGwNurbPointer();
    if(!nurb) return false;
    NL_INDEX n,m; NL_DEGREE degree; NL_CPOINT* points; NL_REAL* knots;
    N_CrvGetCPtsDegreeAndKnots(nurb,&n,&points,&degree,&m,&knots);
    if(n<1) return false;
    if(n>=2) return true;
    NL_POINT a,b; NL_VECTOR v; NL_REAL magnitude;
    N_CPtToPtEuclid(points[0],&a); N_CPtToPtEuclid(points[1],&b);
    if(end==1) N_VectorDir(b,a,&v); else N_VectorDir(a,b,&v);
    return N_VectorNormalize(v,&v,&magnitude)==NL_NO;
}
bool pc1_extensions_safe(SmTArray<SmCurve*>& paths, bool closed, bool periodic, double distance) {
    SmTArray<SmCurve*> allocated;
    SmObjsDelete<SmCurve*> cleanup(&allocated);
    SmVector3d before[2],after[2];
    for(ULONG i=0;i<paths.GetSize();++i) {
        auto path=(SmBSplineCurve*)paths[i];
        auto extend=[&](int end) {
            if(!pc1_extension_safe(path,end)) return false;
            auto previous=path;
            path->CreateExtendedCurve(the_context(),distance,end,SM_CT_G1,path);
            if(!path) return false;
            if(path!=previous) allocated.AddUnique(path);
            return true;
        };
        bool start=closed&&!periodic;
        if(i>0) {
            auto previous=(SmBSplineCurve*)paths[i-1];
            previous->Evaluate(previous->GetNaturalInterval().GetMax(),1,TRUE,before);
            path->Evaluate(path->GetNaturalInterval().GetMin(),1,FALSE,after);
            start=before[1].IsParallelTo(after[1],2.0)==0;
        }
        if(start&&!extend(1)) return false;
        bool end=closed&&!periodic;
        if(i+1<paths.GetSize()) {
            auto next=(SmBSplineCurve*)paths[i+1];
            path->Evaluate(path->GetNaturalInterval().GetMax(),1,TRUE,before);
            next->Evaluate(next->GetNaturalInterval().GetMin(),1,FALSE,after);
            end=before[1].IsParallelTo(after[1],2.0)==0;
        }
        if(end&&!extend(2)) return false;
    }
    return true;
}
void pc1_samples(std::vector<double>& out, SmCurve* curve) {
    const auto interval = curve->GetNaturalInterval();
    out.push_back(interval.GetMin()); out.push_back(interval.GetMax());
    for (int i=0; i<=4; ++i) {
        SmVector3d values[2];
        curve->Evaluate(interval.Evaluate(i/4.0),1,TRUE,values);
        put_vec(out, values[0]); put_vec(out, values[1]);
    }
}
}

// op0 maps: tag analytics clear copy then four maps of four value indices (-1 absent).
// op1 pivot: two seven-double curves, tolerance.
// op2 extension: closed periodic distance count, curve blocks.
// op3 profile: count output_count tol, path block, profile blocks, existing output blocks.
SC_EXPORT int32_t sc_primitive_primitive_creation_p1_eval(int32_t op, const double* data,
    int64_t n, double* out, int64_t cap, int64_t* len) {
    if (!data || op<0 || op>6) return SC_ERR_ARG;
    SC_TRY
    std::vector<double> values;
    if (op==0) {
        if(n!=20) return SC_ERR_ARG;
        SmBrep brep(SM_USE_DEFAULT,SM_USE_DEFAULT,const_cast<SmContext*>(&the_context()));
        auto region = brep.GetInfiniteRegion();
        Pc1Probe primitive(region,(long)data[0],data[1]!=0);
        SmBrep* ids[4];
        for (auto& id : ids) id = new(the_context()) SmBrep();
        SmTArray<SmBrep*> vertices;
        SmObjsDelete<SmBrep*> cleanup(&vertices);
        for(auto id : ids) vertices.Add(id);
        auto maps = {primitive.GetMapToSame(),primitive.GetMapToHigher(),primitive.GetMapFromSame(),primitive.GetMapFromLower()};
        int at=4;
        for(auto map : maps) for(int j=0;j<4;++j) {
            int target=(int)data[at++];
            if(target < -1 || target>3) return SC_ERR_ARG;
            if(target>=0) map->Insert(ids[j],ids[target]);
        }
        SmTranslationalSweepGeometry geometry(SmVector3d(0,0,1));
        SmTopologySweep sweep(geometry);
        // Seed target maps so overwrite direction, stale entries and clear are visible.
        sweep.GetMapToSame()->Insert(ids[0],ids[3]);
        sweep.GetMapToHigher()->Insert(ids[1],ids[2]);
        if(data[2]) primitive.InitEntityMaps();
        if(data[3]) primitive.CopyEntityMaps(&sweep);
        values.insert(values.end(),{(double)primitive.tag(),primitive.region()==region?1.0:0.0});
        auto append=[&](std::initializer_list<SmMapPtrToPtr<SmTopology,SmTopology>*> selected) {
            for(auto map : selected) {
                values.push_back(map->Count());
                for(auto id : ids) {
                    auto target=map->At(id); int index=-1;
                    for(int j=0;j<4;++j) if(target==ids[j]) index=j;
                    values.push_back(index);
                }
            }
        };
        append(maps);
        append({sweep.GetMapToSame(),sweep.GetMapToHigher(),sweep.GetMapFromSame(),sweep.GetMapFromLower()});
    } else if(op==1) {
        if(n!=15) return SC_ERR_ARG;
        SmBSplineCurve* first=pc1_curve(data); SmBSplineCurve* second=pc1_curve(data+7);
        SmObjDelete clean_first(first), clean_second(second);
        if(!first || !second) return -1010;
        SmPoint3d pivot; sm_FindPivotPoint(first,second,data[14],pivot); put_vec(values,pivot);
    } else if(op==2) {
        if(n<4 || data[3]<0 || data[3]>32 || n!=4+7*(int)data[3]) return SC_ERR_ARG;
        SmTArray<SmCurve*> paths, extended;
        SmObjsDelete<SmCurve*> originals(&paths);
        for(int i=0;i<(int)data[3];++i) {
            auto curve=pc1_curve(data+4+7*i); if(!curve) return -1010; paths.Add(curve);
        }
        if(!pc1_extensions_safe(paths,data[0]!=0,data[1]!=0,data[2])) return -1010;
        ExtendPathCurves(the_context(),paths,data[0]!=0,data[1]!=0,data[2],extended);
        // Only newly allocated outputs are deleted separately (unextended aliases remain in paths).
        SmTArray<SmCurve*> allocated;
        SmObjsDelete<SmCurve*> clean_allocated(&allocated);
        values.push_back(extended.GetSize());
        for(ULONG i=0;i<extended.GetSize();++i) {
            pc1_samples(values,extended[i]);
            if(extended[i]!=paths[i]) allocated.Add(extended[i]);
        }
    } else if(op==3) {
        if(n<10 || data[0]<0 || data[0]>32 || data[1]<0 || data[1]>32 || n!=10+7*((int)data[0]+(int)data[1])) return SC_ERR_ARG;
        SmTArray<SmCurve*> profiles, positioned;
        SmObjsDelete<SmCurve*> clean_profiles(&profiles), clean_output(&positioned);
        SmBSplineCurve* path=pc1_curve(data+3); SmObjDelete clean_path(path);
        if(!path) return -1010;
        int at=10;
        for(int i=0;i<(int)data[0];++i) { auto c=pc1_curve(data+at);at+=7;if(!c)return -1010;profiles.Add(c); }
        for(int i=0;i<(int)data[1];++i) { auto c=pc1_curve(data+at);at+=7;if(!c)return -1010;positioned.Add(c); }
        PositionProfileCurves(the_context(),profiles,data[2],path,positioned);
        values.push_back(positioned.GetSize());
        for(ULONG i=0;i<positioned.GetSize();++i) pc1_samples(values,positioned[i]);
    } else if(op==4) {
        if(n<19 || data[0]<0 || data[0]>10) return SC_ERR_ARG;
        const int shape=(int)data[0];
        SmAxis2Placement frame(SmVector3d(data[8],data[9],data[10]),SmVector3d(data[11],data[12],data[13]),SmVector3d(data[14],data[15],data[16]));
        auto record=[&](SmStatus status,SmBrep* b, Pc1Probe* p) {
            SmTArray<SmFace*> faces; SmTArray<SmEdge*> edges; SmTArray<SmVertex*> vertices;
            if(b) { b->GetFaces(faces);b->GetEdges(edges);b->GetVertices(vertices); }
            values.insert(values.end(),{st(status),b?flg(b->m_bEditingEnabled):-1.0,p?(double)p->GetMapToSame()->Count():0.0,(double)faces.GetSize(),(double)edges.GetSize(),(double)vertices.GetSize()});
        };
        if(shape<=1) {
            SmBrep* b=shape==0 ? sc_pc1_PrimitiveCreation::CreateRectangle(the_context(),data[7],data[1],data[2],frame) : sc_pc1_PrimitiveCreation::CreateCircle(the_context(),data[7],data[1],frame);
            SmObjDelete clean(b); record(b?SM_SUCCESS:SM_ERR,b,nullptr);
        } else {
            SmBrep b(SM_USE_DEFAULT,SM_USE_DEFAULT,const_cast<SmContext*>(&the_context()));b.m_bEditingEnabled=data[17]!=0;
            Pc1Probe primitive(b.GetInfiniteRegion(),17,true);
            if(data[18]) primitive.GetMapToSame()->Insert(&b,&b);
            SmStatus status=SM_SUCCESS;
            if(shape==2) status=primitive.CreateBox(data[1],data[2],data[3],frame);
            if(shape==3) {
                const double length=data[1],inner=data[2],outer=data[3],start=data[4],end=data[5];
                // SmCircle's three-point constructor ignores a failed solve and
                // reads its unwritten radius. Probe that exact helper before UB.
                // Retain the primitive's validation errors, which precede it.
                if(length>SM_EFF_ZERO && inner>SM_EFF_ZERO && outer>inner+SM_EFF_ZERO && start>=-360 && start<=360 && end>start+SM_EFF_ZERO && end-start<=360) {
                    auto radial=[&](double angle) {return smos_Cosine(SM_DEG2RAD(angle))*frame.GetXAxisRef()+smos_Sine(SM_DEG2RAD(angle))*frame.GetYAxisRef();};
                    const auto a=radial(start),m=radial((start+end)/2.0),z=radial(end);
                    const auto shift=length*frame.GetZAxis();
                    for(int top=0;top<2;++top) for(double radius : {inner,outer}) {
                        auto p=frame.GetOriginRef()+radius*a,q=frame.GetOriginRef()+radius*m,r=frame.GetOriginRef()+radius*z;
                        if(top) {p+=shift;q+=shift;r+=shift;}
                        if(radius==inner) std::swap(p,r);
                        SmPoint3d center;SmVector3d x,y;SmExtent1d domain;double computed_radius=0;
                        if(smgu_CircleFrom3Points(p,q,r,center,x,y,domain,computed_radius,3,FALSE)!=SM_SUCCESS) return -1010;
                    }
                }
                status=primitive.CreateCylindricalBox(length,inner,outer,start,end,frame);
            }
            if(shape==4) status=primitive.CreateCone(data[1],data[2],data[3],frame);
            if(shape==5) status=primitive.CreateSphere(data[1],frame);
            if(shape==6) status=primitive.CreateTorus(data[1],data[2],frame);
            if(shape==7) { SmBrep* target=&b;status=primitive.CreateSphereNoPole(&the_context(),data[1],frame.GetOrigin(),data[7],target); }
            if(shape==8) { SmBrep* target=&b;status=primitive.CreateConeEllipticEnds(data[1],data[2],data[3],data[4],data[5],data[6],n==20?data[19]:0,data[7]!=0,target); }
            if(shape==9) {
                SmTArray<SmCurve*> curves;
                SmObjsDelete<SmCurve*> cleanup(&curves);
                if(n!=19) {
                    if(n<20 || data[19]<0 || data[19]>32 || data[19]!=(int)data[19] || n!=20+7*(int)data[19]) return SC_ERR_ARG;
                    for(int i=0;i<(int)data[19];++i) {
                        auto curve=pc1_sweep_curve(data+20+7*i,data[6]!=0);
                        if(!curve) return -1010;
                        curves.Add(curve);
                    }
                }
                if(data[1]<0) {
                    // The full-circle/cylinder branch hangs on its reversed V
                    // domain. Preserve negative line/open-arc sweeps: only this
                    // exact upstream branch is excluded (plan/upstream-issues.md).
                    for(ULONG i=0;i<curves.GetSize();++i) {
                        if(curves[i]->IsLinear()) continue;
                        SmAxis2Placement arc_frame;
                        double radius=0,start=0,end=0;
                        if(curves[i]->IsArc(8,b.GetTolerance(),arc_frame,radius,start,end)
                           && end-start<360.0+b.GetTolerance() && end-start>360.0-b.GetTolerance()
                           && arc_frame.GetZAxis().IsParallelTo(frame.GetZAxis(),0.5)) return -1010;
                    }
                }
                cleanup.Clear(); // CreateLinearSweep consumes every input, including on error.
                status=primitive.CreateLinearSweep(curves,frame.GetZAxis(),data[1],data[7]!=0);
            }
            if(shape==10) { SmBrep other(SM_USE_DEFAULT,SM_USE_DEFAULT,const_cast<SmContext*>(&the_context()));SmBrep initial(SM_USE_DEFAULT,SM_USE_DEFAULT,const_cast<SmContext*>(&the_context()));SmBrep* result=data[6]!=0?&initial:nullptr;status=primitive.Boolean2D(&b,&other,(sc_pc1_BooleanType)(int)data[1],result);values.push_back(result?1.0:0.0); }
            record(status,&b,&primitive);
        }
    } else if(op==6) {
        if(n<7 || data[4]<0 || data[4]>4 || data[5]<0 || data[5]>4 ||
           data[4]!=(int)data[4] || data[5]!=(int)data[5] || n!=7+7*((int)data[4]+(int)data[5])) return SC_ERR_ARG;
        SmBrep a(SM_USE_DEFAULT,SM_USE_DEFAULT,const_cast<SmContext*>(&the_context()));
        auto b=new(the_context()) SmBrep(SM_USE_DEFAULT,SM_USE_DEFAULT,const_cast<SmContext*>(&the_context()));
        SmObjDelete clean_b(b);
        int at=7;
        for(auto pair : {std::make_pair(&a,(int)data[4]),std::make_pair(b,(int)data[5])}) {
            for(int i=0;i<pair.second;++i,at+=7) {
                auto f=data+at;
                const auto angle=SM_DEG2RAD(f[1]);
                SmAxis2Placement frame(SmVector3d(0,0,f[2]),SmVector3d(1,0,0),SmVector3d(0,smos_Cosine(angle),smos_Sine(angle)));
                SmSurface* surface=nullptr;
                if(f[0]==0) {
                    SmPlane* plane=nullptr;SmPlane::CreateCanonical(the_context(),frame,plane);
                    if(!plane) return -1010;
                    plane->AdjustSTEPUVDomain(SmExtent2d(f[3],f[4],f[5],f[6]));surface=plane;
                } else {
                    SmSphere* sphere=nullptr;SmSphere::CreateCanonical(the_context(),frame,1,sphere);
                    if(!sphere) return -1010;
                    surface=sphere;
                }
                SmExtent2d domain(f[3],f[4],f[5],f[6]);
                SmFace* face=nullptr;
                if(pair.first->CreateFaceFromSurface(surface,domain,face)!=SM_SUCCESS) return -1010;
            }
        }
        a.SetTolerance(data[6]); b->SetTolerance(data[6]);
        a.m_bEditingEnabled=data[2]!=0; b->m_bEditingEnabled=data[3]!=0;
        Pc1Probe primitive(a.GetInfiniteRegion(),17,true);
        SmBrep initial(SM_USE_DEFAULT,SM_USE_DEFAULT,const_cast<SmContext*>(&the_context()));
        SmBrep* result=data[1]!=0?&initial:nullptr;
        auto status=primitive.Boolean2D(&a,b,(sc_pc1_BooleanType)(int)data[0],result);
        // On success Boolean2D deletes B and returns A. On error/empty input B survives.
        const bool consumed=result==&a;
        if(consumed) clean_b.Clear();
        values.insert(values.end(),{st(status),flg(a.m_bEditingEnabled),consumed?-1.0:flg(b->m_bEditingEnabled),result?1.0:0.0});
    } else {
        if(n!=13) return SC_ERR_ARG;
        SmBSplineCurve* curve=pc1_curve(data); SmObjDelete clean_curve(curve);
        if(!curve) return -1010;
        SmBrep brep(SM_USE_DEFAULT,SM_USE_DEFAULT,const_cast<SmContext*>(&the_context()));
        SmCurve* input=curve;
        auto status=sc_pc1_AnalyticRotationalSweep(input,SmVector3d(data[7],data[8],data[9]),SmVector3d(data[10],data[11],data[12]),&brep);
        SmTArray<SmFace*> faces;brep.GetFaces(faces);
        values.insert(values.end(),{st(status),(double)faces.GetSize(),flg(brep.m_bEditingEnabled)});
    }
    return sc_copy_out(values.data(),values.size(),out,cap,len);
    SC_CATCH
}
