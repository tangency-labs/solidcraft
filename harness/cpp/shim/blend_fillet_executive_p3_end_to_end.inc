// SPDX-License-Identifier: Apache-2.0
// Real public fillet API, preserving the selected pre-operation edge and the
// freshly traversed post-operation solid. No executive fixture owns fake objects.
#include "support/primitive_record.h"
#include <SmApiGeneral.h>
#include <SmApiQueries.h>
#include <SmApiSurfaces.h>
#include <SmFilletCorner.h>
#include <SmVertex.h>
#include <SmLoop.h>
#include <cmath>
#include <limits>

namespace fillet_real_solid {
inline double status(SmStatus value) { return value == SM_SUCCESS ? 0.0 : double(value); }
inline void section(std::vector<double>& out, const std::vector<double>& data) {
    out.push_back(double(data.size())); out.insert(out.end(), data.begin(), data.end());
}
template<class T> double label(const SmTArray<T*>& list, T* value) {
    if (!value) return -1.0;
    for (ULONG i=0; i<list.GetSize(); ++i) if (list[i] == value) return double(i);
    return -2.0;
}
inline void topology(std::vector<double>& out, SmBrep* b) {
    SmTArray<SmVertex*> vertices; b->GetVertices(vertices);
    SmTArray<SmEdge*> edges; b->GetEdges(edges);
    SmTArray<SmFace*> faces; b->GetFaces(faces);
    out.push_back(double(edges.GetSize()));
    for (ULONG i=0; i<edges.GetSize(); ++i) {
        auto* edge=edges[i];
        out.push_back(edge->GetCurve() ? double(edge->GetCurve()->GetType()) : 0.0);
        SmTArray<SmVertex*> ends; edge->GetVertices(ends);
        out.push_back(double(ends.GetSize()));
        for (ULONG j=0; j<ends.GetSize(); ++j) out.push_back(label(vertices,ends[j]));
        SmTArray<SmEdgeuse*> uses; edge->GetEdgeuses(uses);
        out.push_back(double(uses.GetSize()));
        for (ULONG j=0; j<uses.GetSize(); ++j)
            out.insert(out.end(), {label(faces,uses[j]->GetFace()),double(uses[j]->GetOrientation())});
    }
    out.push_back(double(faces.GetSize()));
    for (ULONG i=0; i<faces.GetSize(); ++i) {
        auto* face=faces[i];
        out.push_back(face->GetSurface() ? double(face->GetSurface()->GetType()) : 0.0);
        SmTArray<SmLoop*> loops; face->GetLoops(loops);
        out.push_back(double(loops.GetSize()));
        for (ULONG j=0; j<loops.GetSize(); ++j) {
            SmTArray<SmEdgeuse*> uses; loops[j]->GetEdgeuses(uses);
            out.push_back(double(uses.GetSize()));
            for (ULONG q=0; q<uses.GetSize(); ++q)
                out.insert(out.end(), {label(edges,uses[q]->GetEdge()),double(uses[q]->GetOrientation())});
        }
    }
}
inline void counts(std::vector<double>& out, SmBrep* b) {
    SmTArray<SmVertex*> vertices; b->GetVertices(vertices);
    SmTArray<SmEdge*> edges; b->GetEdges(edges);
    SmTArray<SmFace*> faces; b->GetFaces(faces);
    out.insert(out.end(), {double(vertices.GetSize()), double(edges.GetSize()), double(faces.GetSize())});
}
}

namespace fillet_real_solid {
// Fillet wire: radius, xsect, continuity, thumbweight, edge count, then owning-list
// edge indices; a count of 0 (source entry only) selects every edge in GetEdges
// order. Validates the indices, runs SmApiFilletEdges on `body` and appends the
// record. Returns SC_ERR_ARG for an invalid wire, else SC_OK.
// SmApiFilletEdges with the executive's piecewise merge selected: the same
// executive set-up, then SetDoPiecewiseMerge(TRUE) after SetFilletParameters
// (which resets the merge flags; its header says they may be overridden after
// it), as prog_test's topology-based filleting selects it (smFilletTest.cpp:4521).
SmStatus piecewise_fillet_edges(SmBrep* pBrep,SmTArray<SmEdge*>& rEdges,double dRadius,
    ULONG lXSectType,ULONG lContinuity,double dThumbweight) {
    if (!pBrep || rEdges.GetSize() < 1) return SM_ERR_INVALID_INPUT;
    const SmContext* pContext=SmApiGetOrCreateContext();
    SmFilletSurfaceGeneratorType eFSGType=SM_FSG_CIRCULAR;
    if (lXSectType == 0) eFSGType=SM_FSG_LINEAR;
    else if (lXSectType == 2) eFSGType=SM_FSG_BLEND_CURVE;
    SmFilletExecutive sFilExec(*pContext,pBrep);
    sFilExec.SetDoGlobalMerge(TRUE);
    SmStatus eStat=sFilExec.SetFilletParameters(pBrep,rEdges,eFSGType,SM_FS_CONST_RADIUS,
        dRadius,NULL,-1.0,-1.0,-1.0,-1.0,lContinuity,dThumbweight);
    if (eStat != SM_SUCCESS) return eStat;
    sFilExec.SetDoPiecewiseMerge(TRUE);
    eStat=sFilExec.DoFillet();
    if (eStat != SM_SUCCESS) return eStat;
    pBrep->StitchAndOrient();
    return SM_SUCCESS;
}

// SmApiFilletEdges with an SmMakeCurveBlendSIH(dStepBackFactor) self-intersection
// handler installed on the executive (SetSelfIntersectionHandler, as prog_test's
// smFilletTest.cpp:4739 declares one), so every FlushCurve smooths the new curves
// into the previous section through SmMakeCurveBlendSIH::SmoothOutIntersection.
SmStatus curve_sih_fillet_edges(SmBrep* pBrep,SmTArray<SmEdge*>& rEdges,double dRadius,
    ULONG lXSectType,ULONG lContinuity,double dThumbweight,double dStepBackFactor) {
    if (!pBrep || rEdges.GetSize() < 1) return SM_ERR_INVALID_INPUT;
    const SmContext* pContext=SmApiGetOrCreateContext();
    SmFilletSurfaceGeneratorType eFSGType=SM_FSG_CIRCULAR;
    if (lXSectType == 0) eFSGType=SM_FSG_LINEAR;
    else if (lXSectType == 2) eFSGType=SM_FSG_BLEND_CURVE;
    SmMakeCurveBlendSIH sSIH(dStepBackFactor);
    SmFilletExecutive sFilExec(*pContext,pBrep);
    sFilExec.SetSelfIntersectionHandler(&sSIH);
    sFilExec.SetDoGlobalMerge(TRUE);
    SmStatus eStat=sFilExec.SetFilletParameters(pBrep,rEdges,eFSGType,SM_FS_CONST_RADIUS,
        dRadius,NULL,-1.0,-1.0,-1.0,-1.0,lContinuity,dThumbweight);
    if (eStat != SM_SUCCESS) return eStat;
    eStat=sFilExec.DoFillet();
    if (eStat != SM_SUCCESS) return eStat;
    pBrep->StitchAndOrient();
    return SM_SUCCESS;
}

// A fixture-selected corner assignment between the public executive's corner
// creation and its real filleting call. Missing identities are fixture errors,
// not synthetic SmStatus results. Ordinary executive statuses remain in result.
int32_t setback_fillet_edges(SmBrep* body,SmTArray<SmEdge*>& selected,
    const double* fillet,const double* setback,SmStatus& result) {
    SmTArray<SmVertex*> vertices;body->GetVertices(vertices);
    SmVertex* vertex=nullptr;ULONG matches=0;
    for(ULONG i=0;i<vertices.GetSize();++i) {
        const auto point=vertices[i]->GetPoint();
        if(point[0]==setback[0] && point[1]==setback[1] && point[2]==setback[2]) {
            vertex=vertices[i];++matches;
        }
    }
    if(matches!=1)return SC_ERR_ARG;
    SmFilletExecutive executive(*SmApiGetOrCreateContext(),body);
    executive.SetDoGlobalMerge(TRUE);
    const auto xsect=fillet[1]==0?SM_FSG_LINEAR:fillet[1]==2?SM_FSG_BLEND_CURVE:SM_FSG_CIRCULAR;
    result=executive.SetFilletParameters(body,selected,xsect,SM_FS_CONST_RADIUS,
        fillet[0],nullptr,-1,-1,-1,-1,ULONG(fillet[2]),fillet[3]);
    if(result!=SM_SUCCESS)return SC_OK;
    result=executive.CreateFilletCorners();
    if(result!=SM_SUCCESS)return SC_OK;
    auto* corner=executive.GetFilletCornerOfVertex(vertex);
    if(!corner)return SC_ERR_ARG;
    corner->SetSetBackDist(setback[3]);
    result=executive.DoFilleting();
    // SmApiSetBevelCorners also ignores this status and retains its executive
    // through StitchAndOrient. The separate lifecycle control records that scope.
    if(result==SM_SUCCESS)body->StitchAndOrient();
    return SC_OK;
}

// `curve_sih` > 0 fillets with curve_sih_fillet_edges at that step-back factor; a
// non-null `end_radius` runs SmApiVariableRadiusFillet from fillet[0] to *end_radius
// (thumbweight fillet[3] must be 1.0, the variable-radius API takes none).
// `law` (validated by the Rust binding): degree, radius count, radii, unique knot count,
// knots, multiplicities, edge map min/max; SetFilletParameters(SM_FS_VARIABLE_RADIUS)
// with an SmBSplineFilletLaw over that curve, laid out as SmApiVariableRadiusFillet.
SmStatus law_fillet_edges(SmBrep* body,SmTArray<SmEdge*>& edges,double radius,ULONG xsect,
    ULONG continuity,const double* law) {
    const SmContext* ctx=SmApiGetOrCreateContext();
    const ULONG degree=ULONG(law[0]), points=ULONG(law[1]);
    const double* radii=law+2;
    const ULONG knots=ULONG(law[2+points]);
    const double* knot=law+3+points;
    const double* mult=knot+knots;
    const double* map=mult+knots;
    SmTArray<SmPoint3d> control; SmTArray<double> unique; SmTArray<ULONG> mults;
    for (ULONG i=0; i<points; ++i) control.Add(SmPoint3d(radii[i],0,0));
    for (ULONG i=0; i<knots; ++i) { unique.Add(knot[i]); mults.Add(ULONG(mult[i])); }
    SmBSplineCurve* curve=nullptr;
    SER(SmBSplineCurve::CreateCanonical(*ctx,3,degree,control,SM_CF_UNSPECIFIED,mults,unique,
        SM_KT_UNSPECIFIED,NULL,NULL,curve));
    SmBSplineFilletLaw sLaw(curve,SmExtent1d(map[0],map[1]));
    SmFilletSurfaceGeneratorType eFSGType=SM_FSG_CIRCULAR;
    if (xsect == 0) eFSGType=SM_FSG_LINEAR;
    else if (xsect == 2) eFSGType=SM_FSG_BLEND_CURVE;
    SmFilletExecutive sFilExec(*ctx,body);
    sFilExec.SetDoGlobalMerge(TRUE);
    SER(sFilExec.SetFilletParameters(body,edges,eFSGType,SM_FS_VARIABLE_RADIUS,radius,&sLaw,
        -1.0,-1.0,-1.0,-1.0,continuity));
    SER(sFilExec.DoFillet());
    body->StitchAndOrient();
    return SM_SUCCESS;
}

int32_t fillet_and_record(SmBrep* body,const double* fillet,int64_t n,std::vector<double>& values,
    bool piecewise=false,double curve_sih=0.0,const double* end_radius=nullptr,
    const double* setback=nullptr,const double* law=nullptr) {
    if (n < 5) return SC_ERR_ARG;
    if (end_radius && (!std::isfinite(*end_radius) || fillet[3] != 1.0)) return SC_ERR_ARG;
    const double ulong_limit=std::ldexp(1.0,std::numeric_limits<ULONG>::digits);
    for (int i : {1,2}) if (fillet[i] < 0 || std::floor(fillet[i]) != fillet[i]
        || fillet[i] >= ulong_limit) return SC_ERR_ARG;
    if (fillet[4] < 0 || std::floor(fillet[4]) != fillet[4]
        || fillet[4] != double(n-5)) return SC_ERR_ARG;
    SmTArray<SmEdge*> edges, selected; body->GetEdges(edges);
    std::vector<double> indices(fillet+5, fillet+n);
    if (indices.empty()) for (ULONG i=0; i<edges.GetSize(); ++i) indices.push_back(double(i));
    std::vector<double> before;
    for (double index : indices) {
        if (index < 0 || index >= edges.GetSize() || std::floor(index) != index) return SC_ERR_ARG;
        auto* edge=edges[ULONG(index)]; selected.Add(edge);
        const auto interval=edge->GetInterval();
        before.insert(before.end(), {index,interval.GetMin(),interval.GetMax(),double(edge->GetCurve()!=nullptr)});
        if (auto* curve=edge->GetCurve()) {
            before.push_back(double(curve->GetType()));
            for (double fraction : {0.17,0.37,0.83}) {
                SmPoint3d p; const auto s=curve->EvaluatePoint(interval.Evaluate(fraction),p);
                primitive_record::sample(before,s,p);
            }
        }
    }
    SmStatus result;
    if(setback) {
        const auto rc=setback_fillet_edges(body,selected,fillet,setback,result);
        if(rc!=SC_OK)return rc;
    } else if(law) {
        result=law_fillet_edges(body,selected,fillet[0],ULONG(fillet[1]),ULONG(fillet[2]),law);
    } else {
    result=end_radius
        ? SmApiVariableRadiusFillet(body,selected,fillet[0],*end_radius,ULONG(fillet[1]),ULONG(fillet[2]))
        : curve_sih > 0.0
        ? curve_sih_fillet_edges(body,selected,fillet[0],ULONG(fillet[1]),ULONG(fillet[2]),fillet[3],curve_sih)
        : piecewise
        ? piecewise_fillet_edges(body,selected,fillet[0],ULONG(fillet[1]),ULONG(fillet[2]),fillet[3])
        : SmApiFilletEdges(body,selected,fillet[0],ULONG(fillet[1]),ULONG(fillet[2]),fillet[3]);
    }
    // Selected pointers may no longer be live; all result traversals are fresh.
    section(values,before);
    values.push_back(status(result));
    counts(values,body);
    std::vector<double> topology_record; topology(topology_record,body);
    section(values,topology_record);
    std::vector<double> geometry; primitive_record::append(geometry,body);
    section(values,geometry);
    double area,volume,mass; SmPoint3d centroid; SmVector3d inertia,products;
    const auto mass_status=SmApiBrepComputeMassProperties(body,1e-4,1.0,SmPoint3d(0,0,0),area,volume,mass,centroid,inertia,products);
    values.push_back(status(mass_status));
    if (mass_status == SM_SUCCESS) {
        values.insert(values.end(), {area,volume,mass});
        primitive_record::point(values,centroid);
        primitive_record::point(values,inertia);
        primitive_record::point(values,products);
    }
    return SC_OK;
}
}

// Wire: dimensions XYZ, radius, xsect, continuity, thumbweight, edge count,
// then owning-list edge indices. This initial fixture constructor is a box.
namespace fillet_real_solid {
int32_t real_solid(const double* input, int64_t n, double* out, int64_t cap, int64_t* out_len,
    bool piecewise, double curve_sih=0.0, const double* end_radius=nullptr,
    const double* setback=nullptr, const double* law=nullptr) {
    if (!input || n < 9) return SC_ERR_ARG;
    for (int64_t i=0; i<n; ++i) if (!std::isfinite(input[i])) return SC_ERR_ARG;
    for (int i=0; i<3; ++i) if (input[i] <= SM_EFF_ZERO) return SC_ERR_ARG;
    if (input[7] < 1) return SC_ERR_ARG;
    SC_TRY
    SmBrep* raw=nullptr;
    const auto setup=SmApiCreateBox(SmVector3d(0,0,0),input[0],input[1],input[2],raw);
    if (setup != SM_SUCCESS || !raw) return SC_ERR_ARG;
    std::unique_ptr<SmBrep> body(raw);
    std::vector<double> values;
    const auto rc=fillet_real_solid::fillet_and_record(body.get(),input+3,n-3,values,piecewise,curve_sih,end_radius,setback,law);
    if (rc != SC_OK) return rc;
    return sc_copy_out(values.data(),int64_t(values.size()),out,cap,out_len);
    SC_CATCH
}
}

SC_EXPORT int32_t sc_blend_fillet_executive_p3_real_solid(
    const double* input, int64_t n, double* out, int64_t cap, int64_t* out_len) {
    return fillet_real_solid::real_solid(input,n,out,cap,out_len,false);
}
SC_EXPORT int32_t sc_blend_fillet_executive_p3_real_variable_solid(
    const double* input, int64_t n, double end_radius,
    double* out, int64_t cap, int64_t* out_len) {
    return fillet_real_solid::real_solid(input,n,out,cap,out_len,false,0.0,&end_radius);
}

// The box wire of sc_blend_fillet_executive_p3_real_solid, filleted with a B-spline
// radius law (fillet_real_solid::law_fillet_edges); the binding validates `law`.
SC_EXPORT int32_t sc_blend_fillet_executive_p3_real_law_solid(
    const double* input, int64_t n, const double* law, int64_t law_n,
    double* out, int64_t cap, int64_t* out_len) {
    if(!law || law_n<5 || n<7 || input[6]!=1.0)return SC_ERR_ARG;
    for(int64_t i=0;i<law_n;++i)if(!std::isfinite(law[i]))return SC_ERR_ARG;
    // Block lengths only; the binding checks the values.
    if(law[1]<0||law[1]>64||std::floor(law[1])!=law[1])return SC_ERR_ARG;
    const int64_t points=int64_t(law[1]);
    if(2+points>=law_n)return SC_ERR_ARG;
    const double knots=law[2+points];
    if(knots<0||knots>64||std::floor(knots)!=knots)return SC_ERR_ARG;
    if(law_n!=3+points+2*int64_t(knots)+2)return SC_ERR_ARG;
    return fillet_real_solid::real_solid(input,n,out,cap,out_len,false,0.0,nullptr,nullptr,law);
}

// The legacy box wire is unchanged. The separate block is exactly XYZ and the
// setback distance; its finite domain is a fixture contract, not setter policy.
SC_EXPORT int32_t sc_blend_fillet_executive_p3_real_solid_setback(
    const double* input,int64_t n,const double* setback,int64_t setback_n,
    double* out,int64_t cap,int64_t* out_len) {
    if(!setback || setback_n!=4)return SC_ERR_ARG;
    for(int i=0;i<4;++i)if(!std::isfinite(setback[i]))return SC_ERR_ARG;
    return fillet_real_solid::real_solid(input,n,out,cap,out_len,false,0.0,nullptr,setback);
}

// The box wire of sc_blend_fillet_executive_p3_real_solid, filleted with the
// executive's piecewise merge (fillet_real_solid::piecewise_fillet_edges).
SC_EXPORT int32_t sc_blend_fillet_executive_p3_piecewise_solid(
    const double* input, int64_t n, double* out, int64_t cap, int64_t* out_len) {
    return fillet_real_solid::real_solid(input,n,out,cap,out_len,true);
}

namespace fillet_real_solid {
// Number of source parameters on the source wire (the largest source's count).
constexpr int64_t SOURCE_PARAMS=12;
// Builds the body of a source: 1 = `path` read by SmBrep::ReadFromFile (ASCII .smb,
// no UV-trim rebuild), 2 = SmApiCreateCylinder(origin p0..p2, radius p3, height p4),
// 3/4 = SmApiBooleanUnion/SmApiBooleanDifference of SmApiCreateBox(origin p0..p2,
// size p3..p5) with SmApiCreateBox(origin p6..p8, size p9..p11). Null on failure.
SmBrep* build_source(int kind,const char* path,const double* p) {
    SmContext& ctx=*SmApiGetOrCreateContext();
    if (kind == 1) {
        if (!path) return nullptr;
        std::unique_ptr<SmBrep> brep(new (ctx) SmBrep());
        if (brep->ReadFromFile(ctx,path,SM_ASCII,FALSE) != SM_SUCCESS) return nullptr;
        return brep.release();
    }
    if (kind == 2) {
        SmBrep* raw=nullptr;
        if (SmApiCreateCylinder(SmVector3d(p[0],p[1],p[2]),p[3],p[4],raw) != SM_SUCCESS) {
            delete raw; return nullptr;
        }
        return raw;
    }
    SmBrep* a=nullptr; SmBrep* b=nullptr;
    if (SmApiCreateBox(SmVector3d(p[0],p[1],p[2]),p[3],p[4],p[5],a) != SM_SUCCESS || !a) {
        delete a; return nullptr;
    }
    if (SmApiCreateBox(SmVector3d(p[6],p[7],p[8]),p[9],p[10],p[11],b) != SM_SUCCESS || !b) {
        delete a; delete b; return nullptr;
    }
    SmBrep* res=nullptr;
    const auto stat=kind == 3 ? SmApiBooleanUnion(a,b,res) : SmApiBooleanDifference(a,b,res);
    if (stat != SM_SUCCESS || !res) {
        // Both operands remain allocated on failure.
        if (res && res != a && res != b) delete res;
        delete a; delete b; return nullptr;
    }
    // On success pBrep1 is the result and pBrep2 is consumed.
    if (res != a) delete a;
    return res;
}
}

// Wire: source kind (1 file, 2 cylinder, 3 box union, 4 box difference), 12 source
// parameters (unused ones 0), then the fillet wire of `fillet_and_record`. `path` is
// the .smb file of kind 1 (null otherwise).
namespace fillet_real_solid {
int32_t real_source(const char* path, const double* input, int64_t n, double* out, int64_t cap,
    int64_t* out_len, bool piecewise, double curve_sih=0.0) {
    const int64_t head=1+fillet_real_solid::SOURCE_PARAMS;
    if (!input || n < head+5) return SC_ERR_ARG;
    for (int64_t i=0; i<n; ++i) if (!std::isfinite(input[i])) return SC_ERR_ARG;
    const double kind=input[0];
    if (kind != 1 && kind != 2 && kind != 3 && kind != 4) return SC_ERR_ARG;
    if ((kind == 1) != (path != nullptr)) return SC_ERR_ARG;
    const double* p=input+1;
    if (kind == 2 && (p[3] <= SM_EFF_ZERO || p[4] <= SM_EFF_ZERO)) return SC_ERR_ARG;
    if (kind >= 3) for (int i : {3,4,5,9,10,11}) if (p[i] <= SM_EFF_ZERO) return SC_ERR_ARG;
    SC_TRY
    // Upstream crashes on some file-source fillets (UB-209: extrusion1002 every edge;
    // UB-261: sphere_diff_3cylinders edge 30). The Rust wrapper runs every case in a
    // fresh child (solidcore_ffi::isolate), so such a crash is that child's
    // SC_ERR_CHILD_CRASHED and refuses exactly the inputs that crash this build.
    std::unique_ptr<SmBrep> body(fillet_real_solid::build_source(int(kind),path,p));
    if (!body) return SC_ERR_ARG;
    std::vector<double> values;
    const auto rc=fillet_real_solid::fillet_and_record(body.get(),input+head,n-head,values,piecewise,curve_sih);
    if (rc != SC_OK) return rc;
    return sc_copy_out(values.data(),int64_t(values.size()),out,cap,out_len);
    SC_CATCH
}
}

SC_EXPORT int32_t sc_blend_fillet_executive_p3_real_source(
    const char* path, const double* input, int64_t n, double* out, int64_t cap, int64_t* out_len) {
    return fillet_real_solid::real_source(path,input,n,out,cap,out_len,false);
}

// The source wire of sc_blend_fillet_executive_p3_real_source, filleted with the
// executive's piecewise merge (fillet_real_solid::piecewise_fillet_edges).
SC_EXPORT int32_t sc_blend_fillet_executive_p3_piecewise_source(
    const char* path, const double* input, int64_t n, double* out, int64_t cap, int64_t* out_len) {
    return fillet_real_solid::real_source(path,input,n,out,cap,out_len,true);
}

// Wire: the SmMakeCurveBlendSIH step-back factor (> 0), then the box wire of
// sc_blend_fillet_executive_p3_real_solid, filleted with
// fillet_real_solid::curve_sih_fillet_edges.
SC_EXPORT int32_t sc_blend_fillet_executive_p3_curve_sih_solid(
    const double* input, int64_t n, double* out, int64_t cap, int64_t* out_len) {
    if (!input || n < 1 || !std::isfinite(input[0]) || input[0] <= 0.0) return SC_ERR_ARG;
    return fillet_real_solid::real_solid(input+1,n-1,out,cap,out_len,false,input[0]);
}

// Wire: the SmMakeCurveBlendSIH step-back factor (> 0), then the source wire of
// sc_blend_fillet_executive_p3_real_source, filleted with
// fillet_real_solid::curve_sih_fillet_edges.
SC_EXPORT int32_t sc_blend_fillet_executive_p3_curve_sih_source(
    const char* path, const double* input, int64_t n, double* out, int64_t cap, int64_t* out_len) {
    if (!input || n < 1 || !std::isfinite(input[0]) || input[0] <= 0.0) return SC_ERR_ARG;
    return fillet_real_solid::real_source(path,input+1,n-1,out,cap,out_len,false,input[0]);
}

// Wire: the four corner points (u0v0, u1v0, u0v1, u1v1) of each of two
// SmApiCreateSurfaceFromCornerPoints surfaces (24 values), radius1, radius2,
// tolerance, xsect, mirror, complement. Runs SmApiSurfaceSurfaceFillet as
// TestSmSurfaceSurfaceFillet does and records [status, then for a result B-rep its
// counts, topology and geometry].
SC_EXPORT int32_t sc_blend_fillet_executive_p3_surface_surface(
    const double* input, int64_t n, double* out, int64_t cap, int64_t* out_len) {
    if (!input || n != 30) return SC_ERR_ARG;
    for (int64_t i=0; i<n; ++i) if (!std::isfinite(input[i])) return SC_ERR_ARG;
    const double ulong_limit=std::ldexp(1.0,std::numeric_limits<ULONG>::digits);
    if (input[27] < 0 || std::floor(input[27]) != input[27] || input[27] >= ulong_limit) return SC_ERR_ARG;
    for (int i : {28,29}) if (input[i] != 0 && input[i] != 1) return SC_ERR_ARG;
    SC_TRY
    SmSurface* surfaces[2]={nullptr,nullptr};
    for (int s=0; s<2; ++s) {
        const double* c=input+12*s;
        SmApiCreateSurfaceFromCornerPoints(SmPoint3d(c[0],c[1],c[2]),SmPoint3d(c[3],c[4],c[5]),
            SmPoint3d(c[6],c[7],c[8]),SmPoint3d(c[9],c[10],c[11]),surfaces[s]);
        if (!surfaces[s]) { delete surfaces[0]; return SC_ERR_ARG; }
    }
    std::unique_ptr<SmSurface> first(surfaces[0]), second(surfaces[1]);
    SmBrep* raw=nullptr;
    const auto stat=SmApiSurfaceSurfaceFillet(first.get(),second.get(),input[24],input[25],input[26],
        raw,ULONG(input[27]),input[28] != 0 ? TRUE : FALSE,input[29] != 0 ? TRUE : FALSE);
    std::unique_ptr<SmBrep> result(raw);
    std::vector<double> values{fillet_real_solid::status(stat)};
    if (stat == SM_SUCCESS && result) {
        fillet_real_solid::counts(values,result.get());
        std::vector<double> topology_record; fillet_real_solid::topology(topology_record,result.get());
        fillet_real_solid::section(values,topology_record);
        std::vector<double> geometry; primitive_record::append(geometry,result.get());
        fillet_real_solid::section(values,geometry);
    }
    return sc_copy_out(values.data(),int64_t(values.size()),out,cap,out_len);
    SC_CATCH
}

namespace fillet_real_solid {
inline void body_record(std::vector<double>& values, SmBrep* body) {
    counts(values,body);
    std::vector<double> topology_record; topology(topology_record,body);
    section(values,topology_record);
    std::vector<double> geometry; primitive_record::append(geometry,body);
    section(values,geometry);
}
}

// SmApiSurfaceSurfaceFillet where a surface may already belong to a face (the
// GetFace() branch). Input: the two corner-point surfaces (24 values), owner1, owner2,
// radius1, radius2, tolerance, cross-section (native ULONG), mirror, complement.
// owner1: 0 free, 1 the face of its own sheet body (CreateFaceFromSurface over the
// natural domain). owner2: 0 free, 1 its own sheet body, 2 a second face of surface 1's
// body (owner1 1), 3 surface 1 itself (its corners are ignored). Output: [status,
// has_result, result counts/topology/geometry when present, body count, then for each
// sheet body (surface 1's first) its counts, topology and geometry after the call].
SC_EXPORT int32_t sc_blend_fillet_executive_p3_surface_surface_bodies(
    const double* input, int64_t n, double* out, int64_t cap, int64_t* out_len) {
    if (!input || n != 32) return SC_ERR_ARG;
    for (int64_t i=0; i<n; ++i) if (!std::isfinite(input[i])) return SC_ERR_ARG;
    const double owner1=input[24], owner2=input[25];
    if (owner1 != 0 && owner1 != 1) return SC_ERR_ARG;
    if (owner2 != 0 && owner2 != 1 && owner2 != 2 && owner2 != 3) return SC_ERR_ARG;
    if (owner2 == 2 && owner1 != 1) return SC_ERR_ARG;
    const double ulong_limit=std::ldexp(1.0,std::numeric_limits<ULONG>::digits);
    if (input[29] < 0 || std::floor(input[29]) != input[29] || input[29] >= ulong_limit) return SC_ERR_ARG;
    for (int i : {30,31}) if (input[i] != 0 && input[i] != 1) return SC_ERR_ARG;
    SC_TRY
    const SmContext* context=SmApiGetOrCreateContext();
    SmSurface* surfaces[2]={nullptr,nullptr};
    const int built=owner2 == 3 ? 1 : 2;
    for (int s=0; s<built; ++s) {
        const double* c=input+12*s;
        SmApiCreateSurfaceFromCornerPoints(SmPoint3d(c[0],c[1],c[2]),SmPoint3d(c[3],c[4],c[5]),
            SmPoint3d(c[6],c[7],c[8]),SmPoint3d(c[9],c[10],c[11]),surfaces[s]);
        if (!surfaces[s]) { delete surfaces[0]; return SC_ERR_ARG; }
    }
    if (owner2 == 3) surfaces[1]=surfaces[0];
    // Free surfaces stay the caller's; owned ones belong to their sheet body's face.
    std::unique_ptr<SmSurface> free1(owner1 == 0 ? surfaces[0] : nullptr);
    std::unique_ptr<SmSurface> free2(owner2 == 0 ? surfaces[1] : nullptr);
    std::unique_ptr<SmBrep> body1, body2;
    auto own=[&](std::unique_ptr<SmBrep>& body, SmSurface* surface) {
        if (!body) body.reset(new(*context) SmBrep());
        SmFace* face=nullptr;
        const SmStatus stat=body->CreateFaceFromSurface(surface,surface->GetNaturalUVDomain(),face);
        return stat == SM_SUCCESS && face != nullptr;
    };
    if (owner1 == 1 && !own(body1,surfaces[0])) {
        if (owner2 != 3 && owner2 != 0) delete surfaces[1];
        return SC_ERR_ARG;
    }
    if (owner2 == 1 && !own(body2,surfaces[1])) return SC_ERR_ARG;
    if (owner2 == 2 && !own(body1,surfaces[1])) return SC_ERR_ARG;
    SmBrep* raw=nullptr;
    const auto stat=SmApiSurfaceSurfaceFillet(surfaces[0],surfaces[1],input[26],input[27],input[28],
        raw,ULONG(input[29]),input[30] != 0 ? TRUE : FALSE,input[31] != 0 ? TRUE : FALSE);
    std::unique_ptr<SmBrep> result(raw);
    std::vector<double> values{fillet_real_solid::status(stat)};
    if (stat == SM_SUCCESS && result) {
        values.push_back(1.0);
        fillet_real_solid::body_record(values,result.get());
    } else {
        values.push_back(0.0);
    }
    values.push_back(double(int(bool(body1))+int(bool(body2))));
    if (body1) fillet_real_solid::body_record(values,body1.get());
    if (body2) fillet_real_solid::body_record(values,body2.get());
    return sc_copy_out(values.data(),int64_t(values.size()),out,cap,out_len);
    SC_CATCH
}

#include <SmPlane.h>

// tangency-labs/solidcore#460 comparison: the real_solid wire, then the shell distance
// and an open-top flag (1 = the plane faces at z = the box's height are removed, 0 =
// closed). Fillets the box as real_solid does, then SmApiShellBrepFull(distance,
// extended convex edges, merged results, no offset solid). Record: fillet status,
// shell status, then for a shell result its vertex/edge/face counts, the manifold
// query's status and flag, and the mass status with area, volume and mass.
SC_EXPORT int32_t sc_blend_fillet_executive_p3_fillet_then_shell(
    const double* input, int64_t n, double* out, int64_t cap, int64_t* out_len) {
    if (!input || n < 11) return SC_ERR_ARG;
    for (int64_t i=0; i<n; ++i) if (!std::isfinite(input[i])) return SC_ERR_ARG;
    for (int i=0; i<3; ++i) if (input[i] <= SM_EFF_ZERO) return SC_ERR_ARG;
    const double ulong_limit=std::ldexp(1.0,std::numeric_limits<ULONG>::digits);
    for (int i : {4,5}) if (input[i] < 0 || std::floor(input[i]) != input[i]
        || input[i] >= ulong_limit) return SC_ERR_ARG;
    const int64_t m=n-2; // the real_solid wire
    if (input[7] < 1 || std::floor(input[7]) != input[7]
        || input[7] != double(m-8)) return SC_ERR_ARG;
    const double distance=input[m];
    const double open_top=input[m+1];
    if (open_top != 0.0 && open_top != 1.0) return SC_ERR_ARG;
    SC_TRY
    SmBrep* raw=nullptr;
    if (SmApiCreateBox(SmVector3d(0,0,0),input[0],input[1],input[2],raw) != SM_SUCCESS || !raw)
        return SC_ERR_ARG;
    std::unique_ptr<SmBrep> body(raw);
    SmTArray<SmEdge*> edges, selected; body->GetEdges(edges);
    for (int64_t i=8; i<m; ++i) {
        if (input[i] < 0 || input[i] >= edges.GetSize() || std::floor(input[i]) != input[i]) return SC_ERR_ARG;
        selected.Add(edges[ULONG(input[i])]);
    }
    std::vector<double> values;
    const auto filleted=SmApiFilletEdges(body.get(),selected,input[3],ULONG(input[4]),ULONG(input[5]),input[6]);
    values.push_back(fillet_real_solid::status(filleted));
    if (filleted == SM_SUCCESS) {
        SmTArray<const SmFace*> open;
        if (open_top == 1.0) {
            SmTArray<SmFace*> faces; body->GetFaces(faces);
            for (ULONG i=0; i<faces.GetSize(); ++i) {
                SmSurface* surface=faces[i]->GetSurface();
                if (!surface || !surface->IsKindOf(SmPlane_TYPE)) continue;
                SmPoint3d p;
                if (surface->EvaluatePoint(faces[i]->GetUVDomain().GetMid(),p) == SM_SUCCESS
                    && std::fabs(p.z-input[2]) < 1e-6) open.Add(faces[i]);
            }
        }
        SmBrep* shelled=nullptr;
        const auto shell=SmApiShellBrepFull(body.get(),distance,TRUE,TRUE,FALSE,open,shelled);
        std::unique_ptr<SmBrep> result(shelled);
        values.push_back(fillet_real_solid::status(shell));
        values.push_back(double(open.GetSize()));
        if (shell == SM_SUCCESS && result) {
            fillet_real_solid::counts(values,result.get());
            SmBoolean manifold=FALSE;
            const auto manifold_status=SmApiBrepIsManifoldSolid(result.get(),manifold);
            values.insert(values.end(),{fillet_real_solid::status(manifold_status),double(manifold != FALSE)});
            double area,volume,mass; SmPoint3d centroid; SmVector3d inertia,products;
            const auto mass_status=SmApiBrepComputeMassProperties(result.get(),1e-4,1.0,SmPoint3d(0,0,0),area,volume,mass,centroid,inertia,products);
            values.push_back(fillet_real_solid::status(mass_status));
            if (mass_status == SM_SUCCESS) values.insert(values.end(),{area,volume,mass});
            std::vector<double> geometry; primitive_record::append(geometry,result.get());
            fillet_real_solid::section(values,geometry);
        }
    }
    return sc_copy_out(values.data(),int64_t(values.size()),out,cap,out_len);
    SC_CATCH
}

#include <SmFilletCorner.h>
#include <SmFilletGeom.h>
#include <SmFilletSolver.h>
#include <SmFilletStandardSolver.h>
#include <SmVertexuse.h>

namespace {
struct GeometryOffsetsMember {
    using type = SmOffsetSurface* (SmFilletGeom::*)[2];
    friend type member(GeometryOffsetsMember);
};
struct SolverOffsetsMember {
    using type = SmOffsetSurface* (SmFilletSolver::*)[2];
    friend type member(SolverOffsetsMember);
};
template struct ObserveMember<GeometryOffsetsMember, &SmFilletGeom::m_pOffSurfs>;
template struct ObserveMember<SolverOffsetsMember, &SmFilletSolver::m_pSurfaces>;
}

namespace fillet_real_solid {
std::pair<std::vector<double>,std::vector<double>> offset_snapshot(SmFilletExecutive& exec) {
    SmTArray<SmFilletGeom*> geometries;
    for(ULONG i=0;i<exec.GetFilletSolvers().GetSize();++i) {
        SmTArray<SmFilletGeom*> own;exec.GetFilletSolvers()[i]->GetFilletGeoms(own);geometries.Append(own);
    }
    std::vector<double> state,distances;
    for(ULONG i=0;i<exec.GetFilletSolvers().GetSize();++i) {
        auto* solver=exec.GetFilletSolvers()[i];SmTArray<SmFilletGeom*> own;solver->GetFilletGeoms(own);
        state.insert(state.end(),{double(i),label(geometries,solver->GetFilletGeomForSurfaces()),double(own.GetSize())});
        for(ULONG g=0;g<own.GetSize();++g) {
            auto* geometry=own[g];state.insert(state.end(),{label(geometries,geometry),double(geometry->GetFilletGeomType())});
            for(ULONG rail=0;rail<2;++rail) {
                auto* a=(geometry->*member(GeometryOffsetsMember{}))[rail];
                auto* b=(solver->*member(SolverOffsetsMember{}))[rail];
                auto* base_a=a?a->GetBaseSurface():nullptr;auto* base_b=b?b->GetBaseSurface():nullptr;
                state.insert(state.end(),{double(rail),double(a!=nullptr),double(b!=nullptr),
                    double(base_a!=nullptr),double(base_b!=nullptr),double(a && b && a==b),
                    double(base_a && base_b && base_a==base_b),a?double(a->GetType()):-1.0,b?double(b->GetType()):-1.0,
                    base_a?double(base_a->GetType()):-1.0,base_b?double(base_b->GetType()):-1.0});
                if(a)distances.push_back(a->GetOffsetDistance());if(b)distances.push_back(b->GetOffsetDistance());
            }
        }
    }
    return {std::move(state),std::move(distances)};
}
}

namespace fillet_real_solid {
// The corner-vertex component of CreateFilletBrep, cpp1940–2059. Its
// smooth-sequence prelude and later adjustment loop are omitted on BOTH sides.
SmStatus corner_vertices_component(SmFilletExecutive& exec) {
    for(ULONG i=0;i<exec.GetFilletCorners().GetSize();++i) {
        auto* corner=exec.GetFilletCorners()[i];
        const auto fail=[&](const TCHAR* comment) {
            TCHAR name[SM_TBLOCK_SIZE];corner->GetName(name,SM_TBLOCK_SIZE);
            exec.NoteFilletError(SM_FILERR_VERTEX_PROBLEM,corner->GetCornerType(),SM_FIL_FAILURE,
                name,corner->GetFilletedVertex(),nullptr,nullptr,nullptr,nullptr,comment);
            return SM_ERR;
        };
        if(corner->MakeCornerTopology()!=SM_SUCCESS)
            return fail(_T("Fillet Corner Error: Unable to Make Topology"));
        SmBoolean constant=corner->IsBlendingCorner();
        if(constant && corner->IsTangentCorner()) {
            SmTArray<SmFilletSolver*> solvers;corner->GetFilletSolvers(solvers);
            if(solvers.GetSize()==2) {
                auto* curve=solvers[0]->GetEdgeuse(0)->GetEdge()->GetCurve();
                SmPoint3d start,end;curve->GetEnds(start,end);
                const auto point=corner->GetFilletedVertex()->GetPoint();
                const auto tolerance=2.0*corner->GetThisApproxTol3d();
                if(start.DistanceBetween(point)>tolerance && end.DistanceBetween(point)>tolerance)
                    constant=FALSE;
            }
        }
        corner->SetCalcAsConstRad(constant);
        if(corner->CalcCornerVertGeom()!=SM_SUCCESS)
            return fail(_T("Fillet Corner Error: Unable to Compute Geometry"));
    }
    return SM_SUCCESS;
}
std::vector<double> snapshot(SmFilletExecutive& exec,SmTArray<SmVertex*>& target_vertices,SmTArray<SmEdge*>& target_edges,SmTArray<SmFace*>& target_faces, std::vector<double>& parameters) {
    SmTArray<SmEdgeuse*> target_uses;
    for(ULONG i=0;i<target_edges.GetSize();++i) {SmTArray<SmEdgeuse*> uses;target_edges[i]->GetEdgeuses(uses);target_uses.Append(uses);}
    auto& corners=exec.GetFilletCorners();auto& solvers=exec.GetFilletSolvers();
    auto* pseudo=exec.GetPseudoBrep();SmTArray<SmEdge*> edges;pseudo->GetEdges(edges);SmTArray<SmVertex*> vertices;pseudo->GetVertices(vertices);
    SmTArray<SmVertex*> corner_vertices;
    for(ULONG i=0;i<corners.GetSize();++i) {SmTArray<SmFilletVertex*> fv;corners[i]->GetFilletVertices(fv);for(ULONG j=0;j<fv.GetSize();++j)corner_vertices.Add(fv[j]);}
    SmTArray<SmFilletGeom*> all_geometries;
    for(ULONG i=0;i<solvers.GetSize();++i) {SmTArray<SmFilletGeom*> geoms;solvers[i]->GetFilletGeoms(geoms);all_geometries.Append(geoms);}
    std::vector<double> state={double(exec.GetDoGlobalMerge()),double(solvers.GetSize()),double(corners.GetSize()),double(vertices.GetSize()),double(edges.GetSize())};
    for(ULONG i=0;i<corners.GetSize();++i) {
        auto* c=corners[i];
        parameters.insert(parameters.end(),{c->GetThisApproxTol3d(),c->GetTangencyTolRadians()});
        double vertex_label=-2;
        for(ULONG j=0;j<target_vertices.GetSize();++j) if(target_vertices[j]==c->GetFilletedVertex())vertex_label=double(j);
        SmTArray<SmFilletVertex*> fv;c->GetFilletVertices(fv);SmTArray<SmFilletEdge*> fe;c->GetFilletEdges(fe);
        state.insert(state.end(),{double(c->GetCornerType()),vertex_label,double(fv.GetSize()),double(fe.GetSize())});
        for(ULONG j=0;j<fv.GetSize();++j) {
            auto* v=fv[j];auto* object=v->GetPointClassObject();double object_label=object ? -2 : -1;
            for(ULONG u=0;u<target_uses.GetSize();++u) if(static_cast<SmObject*>(target_uses[u])==object)object_label=double(u);
            state.insert(state.end(),{fillet_real_solid::label(corner_vertices,static_cast<SmVertex*>(v)),fillet_real_solid::label(vertices,static_cast<SmVertex*>(v)),double(v->GetFilletVertexType()),fillet_real_solid::label(corners,v->GetFilletCorner()),double(v->GetPointClass()),object?double(object->GetType()):0,object_label});
        }
        for(ULONG j=0;j<fe.GetSize();++j) state.push_back(fillet_real_solid::label(edges,static_cast<SmEdge*>(fe[j])));
    }
    for(ULONG i=0;i<solvers.GetSize();++i) {
        auto* solver=solvers[i];
        auto* constant=dynamic_cast<SmConstantRadiusFS*>(solver);
        state.insert(state.end(),{double(solver->GetSolverType()),constant?2.0:0.0});
        if(constant)parameters.insert(parameters.end(),{constant->GetFilletRadius(0),constant->GetFilletRadius(1)});
        parameters.insert(parameters.end(),{solver->GetThisApproxTol3d(),solver->GetThisAngTolRad(),solver->GetTangencyTolerance()});
        SmTArray<SmFilletGeom*> geometries;solver->GetFilletGeoms(geometries);state.push_back(double(geometries.GetSize()));
        for(ULONG j=0;j<geometries.GetSize();++j) for(ULONG rail=0;rail<2;++rail)state.push_back(fillet_real_solid::label(edges,static_cast<SmEdge*>(geometries[j]->GetRail(rail))));
    }
    for(ULONG i=0;i<edges.GetSize();++i) {
        auto* e=dynamic_cast<SmFilletEdge*>(edges[i]);
        state.push_back(e?double(e->GetFilletEdgeType()):-1);
        state.push_back(e?fillet_real_solid::label(target_faces,e->GetOriginalFace()):-1);
        state.push_back(e?fillet_real_solid::label(corners,e->GetFilletCorner()):-2);
        SmTArray<SmEdgeuse*> uses;edges[i]->GetEdgeuses(uses);state.push_back(double(uses.GetSize()));
        for(ULONG j=0;j<uses.GetSize();++j) {
            auto* vu=uses[j]->GetVertexuse();
            auto* fillet_use=dynamic_cast<SmFilletEdgeuse*>(uses[j]);
            state.insert(state.end(),{double(uses[j]->GetOrientation()),fillet_real_solid::label(corner_vertices,vu?vu->GetVertex():nullptr),fillet_real_solid::label(vertices,vu?vu->GetVertex():nullptr),fillet_use?fillet_real_solid::label(all_geometries,fillet_use->GetFilletGeom()):-2});
        }
    }
    return state;
}
}

namespace fillet_real_solid {
std::pair<std::vector<double>,std::vector<double>> rail_metadata(SmFilletExecutive& exec,bool evaluate_uv) {
    std::vector<double> state{double(exec.GetDoClassification())},values;
    SmTArray<SmEdge*> edges;exec.GetPseudoBrep()->GetEdges(edges);
    for(ULONG i=0;i<exec.GetFilletSolvers().GetSize();++i) {
        SmTArray<SmFilletGeom*> geometries;exec.GetFilletSolvers()[i]->GetFilletGeoms(geometries);
        state.insert(state.end(),{double(i),double(geometries.GetSize())});
        for(ULONG g=0;g<geometries.GetSize();++g) {
            state.push_back(double(g));
            for(ULONG rail=0;rail<2;++rail) {
                auto* edge=geometries[g]->GetRail(rail);state.insert(state.end(),{double(rail),double(edge!=nullptr)});
                if(!edge)continue;auto* curve=edge->GetCurve();
                state.insert(state.end(),{label(edges,static_cast<SmEdge*>(edge)),double(curve!=nullptr)});
                if(curve) {
                    state.push_back(double(curve->GetType()));const auto interval=curve->GetNaturalInterval();const auto edge_interval=edge->GetInterval();
                    values.insert(values.end(),{interval.GetMin(),interval.GetMax(),edge_interval.GetMin(),edge_interval.GetMax()});
                }
                SmTArray<SmEdgeuse*> uses;edge->GetEdgeuses(uses);state.push_back(double(uses.GetSize()));
                for(ULONG u=0;u<uses.GetSize();++u) {
                    auto* use=uses[u];auto* uv=use->GetUVTrimCurvePointer();
                    state.insert(state.end(),{double(use->GetOrientation()),double(uv!=nullptr)});
                    if(uv) {
                        state.insert(state.end(),{double(uv->GetType()),double(uv->GetDim())});const auto interval=uv->GetNaturalInterval();
                        values.insert(values.end(),{interval.GetMin(),interval.GetMax()});
                        if(evaluate_uv)for(double fraction:{.17,.37,.83}) {
                            SmPoint3d point;auto status=uv->EvaluatePoint(interval.Evaluate(fraction),point);state.push_back(fillet_real_solid::status(status));
                            if(status==SM_SUCCESS)values.insert(values.end(),{point.x,point.y});
                        }
                    }
                }
            }
        }
    }
    return {std::move(state),std::move(values)};
}
}

#include <SmCurveClass.h>
#include "blend_fillet_executive_p3_rail_classification.inc"
#include "blend_fillet_executive_p3_intermediate_faces.inc"
#include "blend_fillet_executive_p3_original_selection.inc"
#include "blend_fillet_executive_p3_original_deletion.inc"
#include "blend_fillet_executive_p3_merge_attributes.inc"
#include "blend_fillet_executive_p3_original_merge.inc"
#include "blend_fillet_executive_p3_insertion.inc"

SC_EXPORT int32_t sc_blend_fillet_executive_p3_real_prefix(
    const double* input,int64_t n,int32_t checkpoint,double* out,int64_t cap,int64_t* out_len) {
    if(checkpoint<1 || checkpoint>19 || !input || n<9) return SC_ERR_ARG;
    for(int64_t i=0;i<n;++i) if(!std::isfinite(input[i])) return SC_ERR_ARG;
    for(int i=0;i<3;++i) if(input[i]<=SM_EFF_ZERO) return SC_ERR_ARG;
    const double ulong_limit=std::ldexp(1.0,std::numeric_limits<ULONG>::digits);
    for(int i:{4,5}) if(input[i]<0 || std::floor(input[i])!=input[i] || input[i]>=ulong_limit) return SC_ERR_ARG;
    if(input[7]<1 || std::floor(input[7])!=input[7] || input[7]!=double(n-8))return SC_ERR_ARG;
    SC_TRY
    SmBrep* raw=nullptr;
    if(SmApiCreateBox(SmVector3d(0,0,0),input[0],input[1],input[2],raw)!=SM_SUCCESS || !raw)return SC_ERR_ARG;
    std::unique_ptr<SmBrep> body(raw);
    SmTArray<SmVertex*> vertices;body->GetVertices(vertices);
    SmTArray<SmEdge*> edges,selected;body->GetEdges(edges);
    SmTArray<SmFace*> faces;body->GetFaces(faces);
    std::vector<double> before;
    for(int64_t i=8;i<n;++i) {
        if(input[i]<0 || input[i]>=edges.GetSize() || std::floor(input[i])!=input[i])return SC_ERR_ARG;
        auto* edge=edges[ULONG(input[i])];selected.Add(edge);const auto interval=edge->GetInterval();
        before.insert(before.end(),{input[i],interval.GetMin(),interval.GetMax(),double(edge->GetCurve()!=nullptr)});
        if(auto* curve=edge->GetCurve()) {
            before.push_back(double(curve->GetType()));
            for(double fraction:{0.17,0.37,0.83}) {SmPoint3d p;auto s=curve->EvaluatePoint(interval.Evaluate(fraction),p);primitive_record::sample(before,s,p);}
        }
    }
    fillet_real_solid::FaceExecutive exec(*body->GetContext(),body.get());exec.SetDoGlobalMerge(TRUE);
    const auto xsect=input[4]==0?SM_FSG_LINEAR:input[4]==2?SM_FSG_BLEND_CURVE:SM_FSG_CIRCULAR;
    const auto prepare=exec.SetFilletParameters(body.get(),selected,xsect,SM_FS_CONST_RADIUS,input[3],nullptr,-1,-1,-1,-1,ULONG(input[5]),input[6]);
    if(checkpoint==16 || checkpoint==17 || checkpoint==18 || checkpoint==19){auto values=fillet_real_solid::insertion_checkpoint(exec,body.get(),prepare,before,checkpoint==17,checkpoint==18,checkpoint==19);return sc_copy_out(values.data(),int64_t(values.size()),out,cap,out_len);}
    if(checkpoint==14 || checkpoint==15) {
        double corner_status=-1,constructor_status=-1,shrink_status=-1,map_status=-1;
        std::pair<std::vector<double>,std::vector<double>> maps_before{{0},{}},maps_after{{0},{}};
        if(prepare==SM_SUCCESS){auto c=exec.CreateFilletCorners();corner_status=fillet_real_solid::status(c);if(c==SM_SUCCESS)constructor_status=fillet_real_solid::status(exec.CreateFilletBrep(nullptr));}
        if(checkpoint==15 && constructor_status==0) {
            shrink_status=0;
            for(ULONG i=0;i<exec.GetFilletSolvers().GetSize();++i){SmTArray<SmFilletGeom*> gs;exec.GetFilletSolvers()[i]->GetFilletGeoms(gs);if(gs.GetSize()){auto* surface=gs[0]->GetFilletSurface();auto* face=SM_CAST_PTR(SmFace,surface->GetOwner());if(face)face->ShrinkGeometry();}}
            maps_before=fillet_real_solid::edge_map_state(exec,body.get());
            map_status=fillet_real_solid::status(exec.SetFilletBrepEdgeMap());
            maps_after=fillet_real_solid::edge_map_state(exec,body.get());
        }
        std::vector<double> values;fillet_real_solid::section(values,before);
        std::vector<double> statuses{fillet_real_solid::status(prepare),corner_status,constructor_status};
        if(checkpoint==15)statuses.insert(statuses.end(),{shrink_status,map_status});
        fillet_real_solid::section(values,statuses);
        auto record=fillet_real_solid::intermediate_faces(exec.GetFilletBrep());
        fillet_real_solid::section(values,record.first);fillet_real_solid::section(values,record.second);
        fillet_real_solid::section(values,fillet_real_solid::source_surface_binding(exec,exec.GetFilletBrep()));
        fillet_real_solid::section(values,exec.attribute_state());
        if(checkpoint==15){fillet_real_solid::section(values,maps_before.first);fillet_real_solid::section(values,maps_before.second);fillet_real_solid::section(values,maps_after.first);fillet_real_solid::section(values,maps_after.second);}
        return sc_copy_out(values.data(),int64_t(values.size()),out,cap,out_len);
    }
    double corners=-1,topology=-1,vertex_geometry=-1,smooth=-1,adjust=-1,solver_geometry=-1,corner_edges=-1,trim=-1,classification=-1,faces_status=-1,stitch_status=-1,shrink_status=-1;
    std::vector<double> prior,parameters;
    std::pair<std::vector<double>,std::vector<double>> offsets_before,rails_before,classes_before;
    SmTArray<SmFilletCorner*> adjusted_corners;
    std::vector<double> attributes_before,attributes_after,stitch_outcome{0},stitch_gaps;
    std::pair<std::vector<double>,std::vector<double>> shrink_before,shrink_after;
    if(prepare==SM_SUCCESS && checkpoint>=2) {
        const auto c=exec.CreateFilletCorners();corners=fillet_real_solid::status(c);
        if(checkpoint>=4)offsets_before=fillet_real_solid::offset_snapshot(exec);
        if(c==SM_SUCCESS && (checkpoint==3 || checkpoint==4)) {
            // Direct first-corner operation after setup/corner creation. This
            // is not the complete CreateFilletBrep prelude: smooth-sequence
            // processing is omitted on BOTH sides, never reported as success.
            std::vector<double> unused;
            prior=fillet_real_solid::snapshot(exec,vertices,edges,faces,unused);
            if(exec.GetFilletCorners().GetSize()) {
                auto* corner=exec.GetFilletCorners()[0];
                const auto t=corner->MakeCornerTopology();topology=fillet_real_solid::status(t);
                if(t==SM_SUCCESS && checkpoint==4)vertex_geometry=fillet_real_solid::status(corner->CalcCornerVertGeom());
            }
        }
        if(c==SM_SUCCESS && checkpoint>=5) {
            std::vector<double> unused;
            prior=fillet_real_solid::snapshot(exec,vertices,edges,faces,unused);
            if(checkpoint>=6)smooth=fillet_real_solid::status(exec.ProcessSmoothEdgeSequences());
            if(checkpoint==5 || smooth==0)topology=fillet_real_solid::status(fillet_real_solid::corner_vertices_component(exec));
            if(checkpoint>=6 && topology==0) {
                adjust=0;
                for(ULONG i=0;i<exec.GetFilletCorners().GetSize();++i) {
                    auto* corner=exec.GetFilletCorners()[i];
                    if(!corner->CheckAndFixDegeneracy() && corner->AdjustTopology()!=SM_SUCCESS) {
                        TCHAR name[SM_TBLOCK_SIZE];corner->GetName(name,SM_TBLOCK_SIZE);
                        exec.NoteFilletError(SM_FILERR_VERTEX_PROBLEM,corner->GetCornerType(),SM_FIL_FAILURE,name,corner->GetFilletedVertex(),nullptr,nullptr,nullptr,nullptr,_T("Fillet Corner Error: Topology Adjustment Failure"));
                        adjust=fillet_real_solid::status(SM_ERR);break;
                    }
                }
                if(checkpoint==7 && adjust==0 && exec.GetFilletSolvers().GetSize())
                    solver_geometry=fillet_real_solid::status(exec.GetFilletSolvers()[0]->CalcFilletGeom());
                if(checkpoint>=8 && adjust==0) {
                    solver_geometry=0;
                    for(ULONG i=0;i<exec.GetFilletSolvers().GetSize();++i) {
                        auto* solver=exec.GetFilletSolvers()[i];
                        if(solver->CalcFilletGeom()!=SM_SUCCESS) {
                            exec.GetFilletErrorInfo()->NoteError(solver->GetEdgeuse(0)->GetEdge(),SM_FS_SURF_TRACING_FAILURE,SM_FILERR_EDGE_PROBLEM,_T("Fillet Surface Computation Failure"));
                            solver_geometry=fillet_real_solid::status(SM_ERR);break;
                        }
                    }
                    if(solver_geometry==0) {
                        corner_edges=0;
                        for(ULONG i=0;i<exec.GetFilletCorners().GetSize();++i) {
                            auto* corner=exec.GetFilletCorners()[i];
                            const auto fail=[&](const TCHAR* comment) {
                                TCHAR name[SM_TBLOCK_SIZE];corner->GetName(name,SM_TBLOCK_SIZE);
                                exec.NoteFilletError(SM_FILERR_VERTEX_PROBLEM,corner->GetCornerType(),SM_FIL_FAILURE,name,corner->GetFilletedVertex(),nullptr,nullptr,nullptr,nullptr,comment);
                                corner_edges=fillet_real_solid::status(SM_ERR);
                            };
                            if(corner->AdjustTangentCorner()!=SM_SUCCESS){fail(_T("Fillet Corner Error: Topology Adjustment Failure"));break;}
                            if(corner->CalcCornerVertGeom()!=SM_SUCCESS){fail(_T("Fillet Corner Error: Vertex Computation Failure"));break;}
                            if(corner->CalcCornerEdgeGeom()!=SM_SUCCESS){fail(_T("Fillet Corner Error: Corner Edge Computation Failure"));break;}
                        }
                        if(checkpoint>=9 && corner_edges==0) {
                            rails_before=fillet_real_solid::rail_metadata(exec,false);trim=0;
                            for(ULONG i=0;i<exec.GetFilletSolvers().GetSize();++i) {
                                auto* solver=exec.GetFilletSolvers()[i];
                                if(solver->TrimRails()!=SM_SUCCESS) {
                                    exec.GetFilletErrorInfo()->NoteError(solver->GetEdgeuse(0)->GetEdge(),SM_FV_NO_INT_RAIL_EU,SM_FILERR_EDGE_PROBLEM,_T("Fillet Surface Computation Failure: Unable to Trim Rail Curve"));
                                    trim=fillet_real_solid::status(SM_ERR);break;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if(checkpoint>=10 && trim==0) {
        classes_before=fillet_real_solid::rail_classification(exec,body.get());classification=0;
        if(exec.GetDoClassification()) {
            for(ULONG i=0;i<exec.GetFilletSolvers().GetSize();++i) {
                auto* solver=exec.GetFilletSolvers()[i];
                if(solver->TestRollOver(adjusted_corners)!=SM_SUCCESS) {
                    exec.GetFilletErrorInfo()->NoteError(solver->GetEdgeuse(0)->GetEdge(),SM_FV_NO_INT_RAIL_EU,SM_FILERR_EDGE_PROBLEM,_T("Fillet Solver Error: Unable to Process Rollover"));
                    classification=fillet_real_solid::status(SM_ERR);break;
                }
            }
            if(classification==0)for(ULONG i=0;i<adjusted_corners.GetSize();++i) {
                auto* corner=adjusted_corners[i];corner->ReInitializeCornerEdges();
                if(corner->CalcCornerEdgeGeom()!=SM_SUCCESS) {
                    TCHAR name[SM_TBLOCK_SIZE];corner->GetName(name,SM_TBLOCK_SIZE);
                    exec.NoteFilletError(SM_FILERR_VERTEX_PROBLEM,corner->GetCornerType(),SM_FIL_FAILURE,name,corner->GetFilletedVertex(),nullptr,nullptr,nullptr,nullptr,_T("Fillet Corner Error: Edge Computation Failure After Rollover Adjustment"));
                    classification=fillet_real_solid::status(SM_ERR);break;
                }
            }
        }
    }
    if(checkpoint>=11 && classification==0)faces_status=fillet_real_solid::status(exec.build_faces());
    if(checkpoint>=12 && faces_status==0) {
        attributes_before=exec.attribute_state();
        stitch_status=fillet_real_solid::status(exec.stitch_faces_component(stitch_outcome,stitch_gaps));
        attributes_after=exec.attribute_state();
        if(checkpoint==13 && stitch_status==0) {
            shrink_before=fillet_real_solid::shrink_state(exec,exec.GetFilletBrep());
            shrink_status=0;
            for(ULONG i=0;i<exec.GetFilletSolvers().GetSize();++i){SmTArray<SmFilletGeom*> gs;exec.GetFilletSolvers()[i]->GetFilletGeoms(gs);if(gs.GetSize()){auto* surface=gs[0]->GetFilletSurface();auto* face=SM_CAST_PTR(SmFace,surface->GetOwner());if(face)face->ShrinkGeometry();}}
            shrink_after=fillet_real_solid::shrink_state(exec,exec.GetFilletBrep());
        }
    }
    auto state=fillet_real_solid::snapshot(exec,vertices,edges,faces,parameters);
    std::vector<double> values;fillet_real_solid::section(values,before);
    if(checkpoint>=6) {
        values.insert(values.end(),{fillet_real_solid::status(prepare),corners,smooth,topology,adjust});
        if(checkpoint>=7)values.push_back(solver_geometry);
        if(checkpoint>=8)values.push_back(corner_edges);
        if(checkpoint>=9)values.push_back(trim);
        if(checkpoint>=10)values.push_back(classification);
        if(checkpoint>=11)values.push_back(faces_status);
        if(checkpoint>=12)values.push_back(stitch_status);
        if(checkpoint==13)values.push_back(shrink_status);
    } else values.insert(values.end(),{fillet_real_solid::status(prepare),corners,topology});
    if(checkpoint==4)values.push_back(vertex_geometry);
    fillet_real_solid::section(values,prior);fillet_real_solid::section(values,state);fillet_real_solid::section(values,parameters);
    if(checkpoint==4) {
        std::vector<double> vertex_state,vertex_values;
        if(exec.GetFilletCorners().GetSize()) {
            SmTArray<SmFilletVertex*> vertices;exec.GetFilletCorners()[0]->GetFilletVertices(vertices);
            for(ULONG i=0;i<vertices.GetSize();++i) {
                auto* vertex=vertices[i];
                vertex_state.insert(vertex_state.end(),{double(i),double(vertex->GetStatus()),double(vertex->IsProcessed())});
                if(vertex->IsProcessed()) {
                    primitive_record::point(vertex_values,vertex->GetPoint());
                    vertex_values.insert(vertex_values.end(),{vertex->GetTolerance(),vertex->GetOriginalTParam()});
                }
            }
        }
        fillet_real_solid::section(values,vertex_state);fillet_real_solid::section(values,vertex_values);
    }
    if(checkpoint>=5) {
        std::vector<double> flags,vertex_state,geometry;
        for(ULONG c=0;c<exec.GetFilletCorners().GetSize();++c) {
            auto* corner=exec.GetFilletCorners()[c];
            flags.insert(flags.end(),{double(c),double(corner->IsBlendingCorner()),
                double(corner->IsTangentCorner()),double(corner->IsCalcAsConstRad())});
            SmTArray<SmFilletVertex*> vertices;corner->GetFilletVertices(vertices);
            for(ULONG v=0;v<vertices.GetSize();++v) {
                auto* vertex=vertices[v];
                vertex_state.insert(vertex_state.end(),{double(c),double(v),double(vertex->GetStatus()),double(vertex->IsProcessed())});
                if(vertex->IsProcessed()) {
                    primitive_record::point(geometry,vertex->GetPoint());
                    geometry.insert(geometry.end(),{vertex->GetTolerance(),vertex->GetOriginalTParam()});
                }
            }
        }
        fillet_real_solid::section(values,flags);fillet_real_solid::section(values,vertex_state);fillet_real_solid::section(values,geometry);
    }
    if(checkpoint>=4) {
        const auto after=fillet_real_solid::offset_snapshot(exec);
        fillet_real_solid::section(values,offsets_before.first);fillet_real_solid::section(values,offsets_before.second);
        fillet_real_solid::section(values,after.first);fillet_real_solid::section(values,after.second);
    }
    if(checkpoint>=6) {
        std::vector<double> flags;
        for(ULONG i=0;i<exec.GetFilletCorners().GetSize();++i) {
            auto* corner=exec.GetFilletCorners()[i];
            flags.insert(flags.end(),{double(i),double(corner->IsDegenerateCorner()),double(corner->IsTopologyAdjusted())});
        }
        fillet_real_solid::section(values,flags);
    }
    if(checkpoint>=7) {
        std::vector<double> generated_state,generated_values;
        const auto evaluated=[&](SmStatus status,const SmPoint3d& point) {
            generated_state.push_back(fillet_real_solid::status(status));
            if(status==SM_SUCCESS)primitive_record::point(generated_values,point);
        };
        for(ULONG i=0;i<exec.GetFilletSolvers().GetSize();++i) {
            SmTArray<SmFilletGeom*> geometries;exec.GetFilletSolvers()[i]->GetFilletGeoms(geometries);
            generated_state.insert(generated_state.end(),{double(i),double(geometries.GetSize())});
            for(ULONG g=0;g<geometries.GetSize();++g) {
                auto* geom=geometries[g];auto* surface=geom->GetFilletSurface();
                generated_state.insert(generated_state.end(),{double(g),double(surface!=nullptr)});
                if(surface) {
                    generated_state.push_back(double(surface->GetType()));
                    const auto domain=surface->GetNaturalUVDomain();
                    generated_values.insert(generated_values.end(),{domain.GetMin().x,domain.GetMin().y,domain.GetMax().x,domain.GetMax().y});
                    for(double u:{.17,.37,.83})for(double v:{.17,.37,.83}) {
                        SmPoint3d point;auto status=surface->EvaluatePoint(SmPoint2d(domain.GetUInterval().Evaluate(u),domain.GetVInterval().Evaluate(v)),point);evaluated(status,point);
                    }
                }
                auto* center=geom->GetCenterLineCurve();generated_state.push_back(double(center!=nullptr));
                if(center) {
                    generated_state.push_back(double(center->GetType()));const auto interval=center->GetNaturalInterval();
                    generated_values.insert(generated_values.end(),{interval.GetMin(),interval.GetMax()});
                    for(double fraction:{.17,.37,.83}){SmPoint3d point;auto status=center->EvaluatePoint(interval.Evaluate(fraction),point);evaluated(status,point);}
                }
                for(ULONG rail=0;rail<2;++rail) {
                    auto* edge=geom->GetRail(rail);generated_state.insert(generated_state.end(),{double(rail),double(edge!=nullptr)});
                    if(edge) {
                        auto* curve=edge->GetCurve();generated_state.push_back(double(curve!=nullptr));
                        if(curve) {
                            generated_state.push_back(double(curve->GetType()));const auto interval=curve->GetNaturalInterval();const auto edge_interval=edge->GetInterval();
                            generated_values.insert(generated_values.end(),{interval.GetMin(),interval.GetMax(),edge_interval.GetMin(),edge_interval.GetMax()});
                            for(double fraction:{.17,.37,.83}){SmPoint3d point;auto status=curve->EvaluatePoint(interval.Evaluate(fraction),point);evaluated(status,point);}
                        }
                    }
                }
            }
        }
        fillet_real_solid::section(values,generated_state);fillet_real_solid::section(values,generated_values);
    }
    if(checkpoint>=8) {
        std::vector<double> edge_state,edge_values;
        SmTArray<SmEdge*> edges;exec.GetPseudoBrep()->GetEdges(edges);
        for(ULONG i=0;i<exec.GetFilletCorners().GetSize();++i) {
            SmTArray<SmFilletEdge*> owned;exec.GetFilletCorners()[i]->GetFilletEdges(owned);
            edge_state.insert(edge_state.end(),{double(i),double(owned.GetSize())});
            for(ULONG j=0;j<owned.GetSize();++j) {
                auto* edge=owned[j];auto* curve=edge->GetCurve();
                edge_state.insert(edge_state.end(),{fillet_real_solid::label(edges,static_cast<SmEdge*>(edge)),double(edge->GetFilletEdgeType()),double(edge->GetStatus()),double(curve!=nullptr)});
                if(curve) {
                    edge_state.push_back(double(curve->GetType()));const auto interval=curve->GetNaturalInterval();const auto edge_interval=edge->GetInterval();
                    edge_values.insert(edge_values.end(),{interval.GetMin(),interval.GetMax(),edge_interval.GetMin(),edge_interval.GetMax()});
                    for(double fraction:{0.0,.17,.37,.83,1.0}) {
                        SmPoint3d point;auto status=curve->EvaluatePoint(interval.Evaluate(fraction),point);
                        edge_state.push_back(fillet_real_solid::status(status));if(status==SM_SUCCESS)primitive_record::point(edge_values,point);
                    }
                }
            }
        }
        fillet_real_solid::section(values,edge_state);fillet_real_solid::section(values,edge_values);
    }
    if(checkpoint>=9) {
        const auto after=fillet_real_solid::rail_metadata(exec,true);
        fillet_real_solid::section(values,rails_before.first);fillet_real_solid::section(values,rails_before.second);
        fillet_real_solid::section(values,after.first);fillet_real_solid::section(values,after.second);
    }
    if(checkpoint>=10) {
        const auto after=fillet_real_solid::rail_classification(exec,body.get());
        fillet_real_solid::section(values,classes_before.first);fillet_real_solid::section(values,classes_before.second);
        fillet_real_solid::section(values,after.first);fillet_real_solid::section(values,after.second);
        std::vector<double> adjusted;
        for(ULONG i=0;i<adjusted_corners.GetSize();++i)adjusted.push_back(fillet_real_solid::label(exec.GetFilletCorners(),adjusted_corners[i]));
        fillet_real_solid::section(values,adjusted);
    }
    if(checkpoint>=11) {
        const auto record=fillet_real_solid::intermediate_faces(exec.GetFilletBrep());
        fillet_real_solid::section(values,record.first);fillet_real_solid::section(values,record.second);
        fillet_real_solid::section(values,fillet_real_solid::source_surface_binding(exec,exec.GetFilletBrep()));
    }
    if(checkpoint>=12) {
        fillet_real_solid::section(values,attributes_before);
        fillet_real_solid::section(values,attributes_after.empty()?exec.attribute_state():attributes_after);
        fillet_real_solid::section(values,stitch_outcome);
        fillet_real_solid::section(values,stitch_gaps);
    }
    if(checkpoint==13){fillet_real_solid::section(values,shrink_before.first);fillet_real_solid::section(values,shrink_before.second);fillet_real_solid::section(values,shrink_after.first);fillet_real_solid::section(values,shrink_after.second);}
    return sc_copy_out(values.data(),int64_t(values.size()),out,cap,out_len);
    SC_CATCH
}

// SmApiFilletPreview on a source body. Wire: source kind (0 = SmApiCreateBox at the origin
// with size p0..p2, else the kinds of fillet_real_solid::build_source), 12 source
// parameters (unused ones 0), radius, selected-edge count, then owning-list edge indices.
// Output: the API status; the body's vertex, edge and face counts after the call; the
// number of preview surfaces, then per surface `has` and, for a surface, its GetType, its
// natural UV domain (umin vmin umax vmax) and a 5 x 5 grid over that domain of
// EvaluatePoint `status [x y z]`. The caller owns the surfaces (upstream never frees them).
namespace fillet_real_solid {
constexpr int PREVIEW_GRID=5;
void preview_surface(std::vector<double>& out, const SmSurface* s) {
    out.push_back(s ? 1.0 : 0.0);
    if (!s) return;
    out.push_back(double(s->GetType()));
    const SmExtent2d d=s->GetNaturalUVDomain();
    const double u0=d.GetMin().x, v0=d.GetMin().y, u1=d.GetMax().x, v1=d.GetMax().y;
    out.insert(out.end(), {u0, v0, u1, v1});
    for (int i=0; i<PREVIEW_GRID; ++i)
        for (int j=0; j<PREVIEW_GRID; ++j) {
            const double u=u0+(u1-u0)*(double(i)/double(PREVIEW_GRID-1));
            const double v=v0+(v1-v0)*(double(j)/double(PREVIEW_GRID-1));
            SmPoint3d p;
            const SmStatus st=s->EvaluatePoint(SmPoint2d(u,v),p);
            out.push_back(status(st));
            if (st == SM_SUCCESS) out.insert(out.end(), {p.x, p.y, p.z});
        }
}
}

SC_EXPORT int32_t sc_blend_fillet_executive_p3_fillet_preview(
    const char* path, const double* input, int64_t n, double* out, int64_t cap, int64_t* out_len) {
    const int64_t head=1+fillet_real_solid::SOURCE_PARAMS;
    if (!input || n < head+2) return SC_ERR_ARG;
    for (int64_t i=0; i<n; ++i) if (!std::isfinite(input[i])) return SC_ERR_ARG;
    const double kind=input[0];
    if (kind != 0 && kind != 1 && kind != 2 && kind != 3 && kind != 4) return SC_ERR_ARG;
    if ((kind == 1) != (path != nullptr)) return SC_ERR_ARG;
    const double* p=input+1;
    if (kind == 0) for (int i : {0,1,2}) if (p[i] <= SM_EFF_ZERO) return SC_ERR_ARG;
    if (kind == 2 && (p[3] <= SM_EFF_ZERO || p[4] <= SM_EFF_ZERO)) return SC_ERR_ARG;
    if (kind >= 3) for (int i : {3,4,5,9,10,11}) if (p[i] <= SM_EFF_ZERO) return SC_ERR_ARG;
    const double radius=input[head];
    const double count=input[head+1];
    if (count < 0 || count != std::floor(count) || n != head+2+int64_t(count)) return SC_ERR_ARG;
    SC_TRY
    std::unique_ptr<SmBrep> body;
    if (kind == 0) {
        SmBrep* raw=nullptr;
        if (SmApiCreateBox(SmVector3d(0.0,0.0,0.0),p[0],p[1],p[2],raw) != SM_SUCCESS) {
            delete raw; return SC_ERR_ARG;
        }
        body.reset(raw);
    } else {
        body.reset(fillet_real_solid::build_source(int(kind),path,p));
    }
    if (!body) return SC_ERR_ARG;
    SmTArray<SmEdge*> edges; body->GetEdges(edges);
    SmTArray<SmEdge*> selected;
    for (int64_t i=0; i<int64_t(count); ++i) {
        const double index=input[head+2+i];
        if (index < 0 || index != std::floor(index) || index >= double(edges.GetSize())) return SC_ERR_ARG;
        selected.Add(edges[ULONG(index)]);
    }
    SmTArray<SmSurface*> surfaces;
    const SmStatus stat=SmApiFilletPreview(body.get(),selected,radius,surfaces);
    std::vector<double> values;
    values.push_back(fillet_real_solid::status(stat));
    fillet_real_solid::counts(values,body.get());
    values.push_back(double(surfaces.GetSize()));
    for (ULONG i=0; i<surfaces.GetSize(); ++i) fillet_real_solid::preview_surface(values,surfaces[i]);
    // (each distinct surface once: two geometries of a split could share one)
    for (ULONG i=0; i<surfaces.GetSize(); ++i) {
        bool seen=false;
        for (ULONG j=0; j<i; ++j) seen=seen || surfaces[j] == surfaces[i];
        if (!seen) delete surfaces[i];
    }
    return sc_copy_out(values.data(),int64_t(values.size()),out,cap,out_len);
    SC_CATCH
}
