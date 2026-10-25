// SPDX-License-Identifier: Apache-2.0
// Real public fillet API, preserving the selected pre-operation edge and the
// freshly traversed post-operation solid. No executive fixture owns fake objects.
#include "support/primitive_record.h"
#include <SmApiQueries.h>
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

// Wire: dimensions XYZ, radius, xsect, continuity, thumbweight, edge count,
// then owning-list edge indices. This initial fixture constructor is a box.
SC_EXPORT int32_t sc_blend_fillet_executive_p3_real_solid(
    const double* input, int64_t n, double* out, int64_t cap, int64_t* out_len) {
    if (!input || n < 9) return SC_ERR_ARG;
    for (int64_t i=0; i<n; ++i) if (!std::isfinite(input[i])) return SC_ERR_ARG;
    for (int i=0; i<3; ++i) if (input[i] <= SM_EFF_ZERO) return SC_ERR_ARG;
    // The exclusive power-of-two bound is exact even when ULONG_MAX or
    // long double would round to 2^64. Check before the integer conversion.
    const double ulong_limit=std::ldexp(1.0,std::numeric_limits<ULONG>::digits);
    for (int i : {4,5}) if (input[i] < 0 || std::floor(input[i]) != input[i]
        || input[i] >= ulong_limit) return SC_ERR_ARG;
    if (input[7] < 1 || std::floor(input[7]) != input[7]
        || input[7] != double(n-8)) return SC_ERR_ARG;
    SC_TRY
    SmBrep* raw=nullptr;
    const auto setup=SmApiCreateBox(SmVector3d(0,0,0),input[0],input[1],input[2],raw);
    if (setup != SM_SUCCESS || !raw) return SC_ERR_ARG;
    std::unique_ptr<SmBrep> body(raw);
    SmTArray<SmEdge*> edges, selected; body->GetEdges(edges);
    std::vector<double> before;
    for (int64_t i=8; i<n; ++i) {
        if (input[i] < 0 || input[i] >= edges.GetSize() || std::floor(input[i]) != input[i]) return SC_ERR_ARG;
        auto* edge=edges[ULONG(input[i])]; selected.Add(edge);
        const auto interval=edge->GetInterval();
        before.insert(before.end(), {input[i],interval.GetMin(),interval.GetMax(),double(edge->GetCurve()!=nullptr)});
        if (auto* curve=edge->GetCurve()) {
            before.push_back(double(curve->GetType()));
            for (double fraction : {0.17,0.37,0.83}) {
                SmPoint3d p; const auto s=curve->EvaluatePoint(interval.Evaluate(fraction),p);
                primitive_record::sample(before,s,p);
            }
        }
    }
    const auto result=SmApiFilletEdges(body.get(),selected,input[3],ULONG(input[4]),ULONG(input[5]),input[6]);
    // Selected pointers may no longer be live; all result traversals are fresh.
    std::vector<double> values; fillet_real_solid::section(values,before);
    values.push_back(fillet_real_solid::status(result));
    fillet_real_solid::counts(values,body.get());
    std::vector<double> topology; fillet_real_solid::topology(topology,body.get());
    fillet_real_solid::section(values,topology);
    std::vector<double> geometry; primitive_record::append(geometry,body.get());
    fillet_real_solid::section(values,geometry);
    double area,volume,mass; SmPoint3d centroid; SmVector3d inertia,products;
    const auto mass_status=SmApiBrepComputeMassProperties(body.get(),1e-4,1.0,SmPoint3d(0,0,0),area,volume,mass,centroid,inertia,products);
    values.push_back(fillet_real_solid::status(mass_status));
    if (mass_status == SM_SUCCESS) {
        values.insert(values.end(), {area,volume,mass});
        primitive_record::point(values,centroid);
        primitive_record::point(values,inertia);
        primitive_record::point(values,products);
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
