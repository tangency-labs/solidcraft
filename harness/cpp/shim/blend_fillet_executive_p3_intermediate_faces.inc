#include <stdexcept>
// Actual native intermediate body allocation/corner/solver face component.
#include <SmFaceuse.h>
#include <SmRegion.h>
namespace fillet_real_solid {
namespace {
struct FaceExecutive : Executive {
    using Executive::Executive;
    std::vector<double> attribute_state() {
        if(!m_pFilletBrep)return {0};
        SmTArray<SmEdge*> edges;m_pFilletBrep->GetEdges(edges);SmTArray<SmAObject*> users;if(m_pAttributeE)m_pAttributeE->GetUsers(users);
        std::vector<double> out{1,double(m_pAttributeE!=nullptr),m_pAttributeE?double(m_pAttributeE->GetAttributeID()):0,m_pAttributeE?double(m_pAttributeE->GetType()):0,double(users.GetSize())};
        for(ULONG i=0;i<users.GetSize();++i){double value=-2;for(ULONG e=0;e<edges.GetSize();++e)if(users[i]==edges[e])value=e;out.push_back(value);}
        out.push_back(double(edges.GetSize()));
        for(ULONG i=0;i<edges.GetSize();++i){auto* found=edges[i]->FindAttribute(SM_AI_ALL_FILLET_EDGES);SmTArray<SmAttribute*> own;edges[i]->GetAttributes(own);
            out.insert(out.end(),{double(found!=nullptr),double(found&&found==m_pAttributeE),double(own.GetSize())});
            for(ULONG j=0;j<own.GetSize();++j)out.insert(out.end(),{double(own[j]->GetAttributeID()),double(own[j]->GetType())});
        }
        return out;
    }
    SmStatus stitch_faces_component(std::vector<double>& outcome,std::vector<double>& gaps) {
        SmTArray<SmEdge*> edges;m_pFilletBrep->GetEdges(edges);
        for(ULONG i=0;i<edges.GetSize();++i)edges[i]->AddAttribute(m_pAttributeE);
        ULONG stitched,lamina;double vertex_gap,edge_gap;
        const auto status=m_pFilletBrep->StitchFaces(m_pFilletBrep->GetTolerance(),stitched,lamina,vertex_gap,edge_gap);
        if(status!=SM_SUCCESS){outcome={0};GetFilletErrorInfo()->NoteError(SM_FILERR_BAD_MERGE,_T("Fillet Stitching Failure: Error Stitching Fillets Together"));return SM_ERR;}
        outcome={1,double(stitched),double(lamina)};gaps={vertex_gap,edge_gap};return SM_SUCCESS;
    }
    SmStatus build_faces() {
        m_pFilletBrep=new(m_crContext) SmBrep();NER(m_pFilletBrep);
        m_pFilletBrep->SetTolerance(m_pTargetBrep->GetTolerance());m_pFilletBrep->m_bEditingEnabled=TRUE;
        for(ULONG i=0;i<m_vFilletCorners.GetSize();++i) {
            auto* corner=m_vFilletCorners[i];
            if(corner->CalcCornerGeom()!=SM_SUCCESS) {
                TCHAR name[SM_TBLOCK_SIZE];corner->GetName(name,SM_TBLOCK_SIZE);
                NoteFilletError(SM_FILERR_VERTEX_PROBLEM,corner->GetCornerType(),SM_FIL_FAILURE,name,corner->GetFilletedVertex(),nullptr,nullptr,nullptr,nullptr,_T("Fillet Corner Error: Corner Geometry Computation Failure"));return SM_ERR;
            }
        }
        for(ULONG i=0;i<m_vFilletSolvers.GetSize();++i) {
            auto* solver=m_vFilletSolvers[i];double tolerance=solver->GetThisApproxTol3d();
            if(m_pFilletBrep->GetTolerance()<tolerance) {
#ifdef SM_USE_NEWTOL
                SM_NEWTOL_LINE m_pFilletBrep->SetZoneTol3d(tolerance);
#else
                SM_OLDTOL_LINE if(m_pFilletBrep->GetTolerance()<tolerance) {m_pFilletBrep->SetTolerance(tolerance);}
#endif
            }
            SmTArray<SmFilletGeom*> geometries;solver->GetFilletGeoms(geometries);
            for(ULONG g=0;g<geometries.GetSize();++g)if(geometries[g]->MakeFaceBrep(m_pFilletBrep)!=SM_SUCCESS) {
                GetFilletErrorInfo()->NoteError(solver->GetEdgeuse(0)->GetEdge(),SM_FIL_FAILURE,SM_FILERR_EDGE_PROBLEM,_T("Fillet Surface Computation Failure: Fillet Face Creation Error"));return SM_ERR;
            }
        }
        return SM_SUCCESS;
    }
};
}
std::pair<std::vector<double>,std::vector<double>> intermediate_faces(SmBrep* body) {
    if(!body)return {{0},{}};
    SmTArray<SmVertex*> vertices;body->GetVertices(vertices);SmTArray<SmEdge*> edges;body->GetEdges(edges);
    SmTArray<SmFace*> faces;body->GetFaces(faces);SmTArray<SmRegion*> regions;body->GetRegions(regions);SmTArray<SmShell*> shells;body->GetShells(shells);
    SmTArray<SmEdgeuse*> uses;for(ULONG i=0;i<edges.GetSize();++i){SmTArray<SmEdgeuse*> own;edges[i]->GetEdgeuses(own);uses.Append(own);}
    std::vector<double> state{1,double(vertices.GetSize()),double(edges.GetSize()),double(faces.GetSize()),double(regions.GetSize()),double(shells.GetSize()),double(body->m_bEditingEnabled)};
    topology(state,body);state.push_back(double(uses.GetSize()));
    for(ULONG i=0;i<uses.GetSize();++i){auto* use=uses[i];auto* vu=use->GetVertexuse();
        state.insert(state.end(),{label(edges,use->GetEdge()),double(use->GetOrientation()),label(vertices,vu?vu->GetVertex():nullptr),label(uses,use->GetMate()),label(uses,use->GetRadial()),label(uses,use->GetCWEdgeuse()),label(uses,use->GetCCWEdgeuse()),label(faces,use->GetFace())});}
    for(ULONG i=0;i<faces.GetSize();++i){SmFaceuse *a=nullptr,*b=nullptr;faces[i]->GetFaceuses(a,b);
        for(auto* use:{a,b}){state.push_back(double(use!=nullptr));if(use)state.insert(state.end(),{double(use->GetOrientation()),label(shells,use->GetShell()),label(regions,use->GetRegion())});}}
    for(ULONG i=0;i<shells.GetSize();++i)state.push_back(label(regions,shells[i]->GetRegion()));
    for(ULONG i=0;i<regions.GetSize();++i)state.push_back(double(regions[i]->IsVoid()));
    std::vector<double> geometry{body->GetTolerance()};primitive_record::append(geometry,body);
    return {std::move(state),std::move(geometry)};
}
std::vector<double> source_surface_binding(SmFilletExecutive& exec,SmBrep* body) {
    SmTArray<SmFace*> faces;if(body)body->GetFaces(faces);
    std::vector<double> out{double(exec.GetFilletSolvers().GetSize())};
    for(ULONG solver=0;solver<exec.GetFilletSolvers().GetSize();++solver) {
        SmTArray<SmFilletGeom*> geometries;exec.GetFilletSolvers()[solver]->GetFilletGeoms(geometries);out.push_back(double(geometries.GetSize()));
        for(ULONG g=0;g<geometries.GetSize();++g){auto* surface=geometries[g]->GetFilletSurface();auto* owner=surface?surface->GetOwner():nullptr;
            double owner_face=owner?-2:-1,alias_face=surface?-2:-1;
            for(ULONG f=0;f<faces.GetSize();++f){if(owner&&owner==faces[f])owner_face=f;if(surface&&surface==faces[f]->GetSurface())alias_face=f;}
            out.insert(out.end(),{double(surface!=nullptr),double(owner!=nullptr),owner?double(owner->GetType()):0,owner_face,alias_face});
        }
    }
    return out;
}

std::pair<std::vector<double>,std::vector<double>> shrink_state(SmFilletExecutive& exec,SmBrep* body) {
    std::vector<double> state{double(exec.GetFilletSolvers().GetSize())},values;
    SmTArray<SmFace*> faces;if(body)body->GetFaces(faces);
    for(ULONG i=0;i<exec.GetFilletSolvers().GetSize();++i) {
        SmTArray<SmFilletGeom*> gs;exec.GetFilletSolvers()[i]->GetFilletGeoms(gs);state.push_back(double(gs.GetSize()));
        auto* surface=gs.GetSize()?gs[0]->GetFilletSurface():nullptr;state.push_back(double(surface!=nullptr));
        if(surface){auto* owner=surface->GetOwner();double face=-1;
            for(ULONG f=0;f<faces.GetSize();++f)if(owner&&owner==faces[f]){face=f;break;}
            state.insert(state.end(),{double(surface->GetType()),double(owner!=nullptr),owner?double(owner->GetType()):0,face});
            auto d=surface->GetNaturalUVDomain();values.insert(values.end(),{d.GetMin().x,d.GetMin().y,d.GetMax().x,d.GetMax().y});
            state.push_back(double(face>=0));if(face>=0){auto domain=faces[ULONG(face)]->GetUVDomain();values.insert(values.end(),{domain.GetMin().x,domain.GetMin().y,domain.GetMax().x,domain.GetMax().y});}
        }
    }
    SmTArray<SmEdge*> edges;if(body)body->GetEdges(edges);state.push_back(double(edges.GetSize()));
    for(ULONG i=0;i<edges.GetSize();++i){auto d=edges[i]->GetInterval();values.insert(values.end(),{d.GetMin(),d.GetMax()});auto* c=edges[i]->GetCurve();state.push_back(double(c!=nullptr));
        if(c){state.push_back(double(c->GetType()));auto interval=c->GetNaturalInterval();values.insert(values.end(),{interval.GetMin(),interval.GetMax()});}
        SmTArray<SmEdgeuse*> uses;edges[i]->GetEdgeuses(uses);state.push_back(double(uses.GetSize()));for(ULONG u=0;u<uses.GetSize();++u)state.push_back(double(uses[u]->GetUVTrimCurvePointer()!=nullptr));
    }
    return {state,values};
}

double managed_map_label(const SmTArray<SmEdge*>& edges,SmEdge* edge) {
    auto result=label(edges,edge);if(edge&&result<0)throw std::runtime_error("mapped edge outside actual managed body");return result;
}
std::pair<std::vector<double>,std::vector<double>> edge_map_state(SmFilletExecutive& exec,SmBrep* target) {
    SmTArray<SmEdge*> edges,managed,target_edges;SmTArray<SmVertex*> target_vertices;
    exec.GetPseudoBrep()->GetEdges(edges);if(exec.GetFilletBrep())exec.GetFilletBrep()->GetEdges(managed);target->GetEdges(target_edges);target->GetVertices(target_vertices);
    auto* info=exec.GetFilletErrorInfo();size_t text_size=std::char_traits<TCHAR>::length(info->m_cComment);
    std::vector<double> state{1,double(edges.GetSize()),double(info->GetErrorCode()),label(target_edges,const_cast<SmEdge*>(info->m_pFilletedEdge)),label(target_vertices,const_cast<SmVertex*>(info->m_pFilletedVertex)),double(text_size)},values;
    for(size_t i=0;i<text_size;++i)state.push_back(double(static_cast<unsigned char>(info->m_cComment[i])));
    for(ULONG i=0;i<edges.GetSize();++i){auto* edge=SM_CAST_PTR(SmFilletEdge,edges[i]);if(!edge)throw std::runtime_error("actual pseudo fillet edge required");
        auto raw=edge->GetInterval();values.insert(values.end(),{raw.GetMin(),raw.GetMax()});SmTArray<SmEdge*> maps;edge->GetFilletBrepEdges(maps);
        state.insert(state.end(),{double(i),double(edge->GetFilletEdgeType()),double(edge->GetStatus()),managed_map_label(managed,edge->GetFilletBrepEdge()),double(maps.GetSize())});
        for(ULONG j=0;j<maps.GetSize();++j)state.push_back(managed_map_label(managed,maps[j]));
    }
    return {state,values};
}

}
