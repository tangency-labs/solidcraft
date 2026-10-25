// Actual original mark scope spans selection and DeleteFaces; no later stage.
namespace fillet_real_solid {
inline void deletion_current(SmBrep* body,SmTArray<SmVertex*>& out){body->GetVertices(out);}
inline void deletion_current(SmBrep* body,SmTArray<SmEdge*>& out){body->GetEdges(out);}
inline void deletion_current(SmBrep* body,SmTArray<SmFace*>& out){body->GetFaces(out);}
template<class T> void deletion_marks(std::vector<double>& out,SmBrep* body,const SmTArray<T*>& before,const std::vector<ULONG>& history,size_t& index){
    SmTArray<T*> current;deletion_current(body,current);std::vector<double> marks{double(current.GetSize())};
    for(ULONG i=0;i<current.GetSize();++i){auto prior=label(before,current[i]);if(prior<0)throw std::runtime_error("deletion created unexpected owning topology");
        marks.insert(marks.end(),{prior,double(current[i]->IsMarked(SM_MT_MARK)),double(current[i]->IsUnMarked(SM_MT_MARK)),double(history.at(index+size_t(prior))==current[i]->GetMark(SM_MT_MARK))});}
    index+=before.GetSize();section(out,marks);
}
inline std::vector<double> original_deletion(FaceExecutive& exec,SmBrep* target,SmStatus& result,std::vector<double>& intervals) {
    auto* context=const_cast<SmContext*>(exec.GetContext());auto* managed=exec.GetFilletBrep();auto& ti=exec.GetTopologyIntersector();
    std::vector<double> out{double(exec.GetDoGlobalMerge())};selection_context(out,context);
    std::vector<ULONG> epochs;for(auto type:{SM_MT_MARK,SM_MT_MARK2,SM_MT_MARK3,SM_MT_MARKIO,SM_MT_MARKASSERT})epochs.push_back(context->GetCurrentMark(type));
    SmTArray<SmVertex*> target_vertices,managed_vertices;target->GetVertices(target_vertices);managed->GetVertices(managed_vertices);
    SmTArray<SmEdge*> target_edges,managed_edges;target->GetEdges(target_edges);managed->GetEdges(managed_edges);
    SmTArray<SmFace*> target_faces,managed_faces;target->GetFaces(target_faces);managed->GetFaces(managed_faces);
    std::vector<ULONG> history;
    for(auto* body:{target,managed}) {
        SmTArray<SmVertex*> vertices;body->GetVertices(vertices);for(ULONG i=0;i<vertices.GetSize();++i)history.push_back(vertices[i]->GetMark(SM_MT_MARK));
        SmTArray<SmEdge*> edges;body->GetEdges(edges);for(ULONG i=0;i<edges.GetSize();++i)history.push_back(edges[i]->GetMark(SM_MT_MARK));
        SmTArray<SmFace*> faces;body->GetFaces(faces);for(ULONG i=0;i<faces.GetSize();++i)history.push_back(faces[i]->GetMark(SM_MT_MARK));
    }
    SmTArray<SmFace*> kept,deleted;
    {
        const auto editing=target->m_bEditingEnabled;
        struct RestoreEdit{SmBrep* target;SmBoolean editing;~RestoreEdit(){target->m_bEditingEnabled=editing;}} restore{target,editing};
        target->m_bEditingEnabled=TRUE;
        SmTArray<SmNewMarkAndLock*> locks;SmNewMarkAndLock fillet_lock(context),original_lock;
        const auto fillet_type=fillet_lock.GetMarkType();auto original_type=fillet_type;locks.Add(&fillet_lock);
        if(context!=target->GetContext()){original_lock.SetContext(const_cast<SmContext*>(target->GetContext()));locks.Add(&original_lock);original_type=original_lock.GetMarkType();}
        for(ULONG i=0;i<target_vertices.GetSize();++i)if(auto* mate=static_cast<SmVertex*>(ti.GetOtherMate(target_vertices[i]))){mate->Mark(fillet_type);target_vertices[i]->Mark(original_type);}
        for(ULONG i=0;i<target_edges.GetSize();++i)if(auto* mate=static_cast<SmEdge*>(ti.GetOtherMate(target_edges[i]))){target_edges[i]->Mark(original_type);mate->Mark(fillet_type);}
        for(ULONG s=0;result==SM_SUCCESS&&s<exec.GetFilletSolvers().GetSize();++s){SmTArray<SmFilletGeom*> geometries;exec.GetFilletSolvers()[s]->GetFilletGeoms(geometries);
            for(ULONG g=0;result==SM_SUCCESS&&g<geometries.GetSize();++g)result=geometries[g]->TrimOriginalSurfaces(kept,deleted,locks);}
        if(result==SM_SUCCESS){for(ULONG i=0;i<managed_faces.GetSize();++i){ULONG found;if(!kept.FindElement(managed_faces[i],found))deleted.AddUnique(managed_faces[i]);}result=ti.DeleteFaces(deleted);}
    }
    out.push_back(status(result));selection_context(out,context,&epochs);
    out.insert(out.end(),{double(context->GetDoingBoolean()),double(target->m_bEditingEnabled),double(ti.m_bDoingFillets),double(ti.m_bTopologyDeleted)});
    for(const auto* values:{&kept,&deleted}){std::vector<double> labels;
        for(ULONG i=0;i<values->GetSize();++i){const auto t=label(target_faces,(*values)[i]),m=label(managed_faces,(*values)[i]);
            if(t<0&&m<0)throw std::runtime_error("deletion output outside pre-call body frames");
            labels.insert(labels.end(),{t>=0?1.0:2.0,t>=0?t:m});}section(out,labels);}
    size_t history_index=0;
    deletion_marks(out,target,target_vertices,history,history_index);
    deletion_marks(out,target,target_edges,history,history_index);
    deletion_marks(out,target,target_faces,history,history_index);
    deletion_marks(out,managed,managed_vertices,history,history_index);
    deletion_marks(out,managed,managed_edges,history,history_index);
    deletion_marks(out,managed,managed_faces,history,history_index);
    if(history_index!=history.size())throw std::runtime_error("deletion mark history mismatch");
    // Validate current surface membership before the ordinary source-binding observer.
    SmTArray<SmFace*> live_faces;managed->GetFaces(live_faces);
    for(ULONG s=0;s<exec.GetFilletSolvers().GetSize();++s){SmTArray<SmFilletGeom*> gs;exec.GetFilletSolvers()[s]->GetFilletGeoms(gs);
        for(ULONG g=0;g<gs.GetSize();++g){auto* surface=gs[g]->GetFilletSurface();bool live=!surface;
            for(ULONG f=0;f<live_faces.GetSize();++f)if(live_faces[f]->GetSurface()==surface)live=true;
            if(!live)throw std::runtime_error("post-deletion FG source not in surviving managed faces");}}
    std::vector<double> passive{double(context->GetDoingBoolean()),double(ti.m_bDoingFillets),double(target->m_bEditingEnabled),double(ti.GetPrimaryBrep()==target),double(ti.GetOtherBrep()==managed),double(ti.m_bTopologyDeleted)};
    counts(passive,target);counts(passive,managed);counts(passive,exec.GetPseudoBrep());
    auto maps=edge_map_state(exec,target);section(passive,maps.first);intervals=maps.second;
    section(passive,source_surface_binding(exec,managed));section(passive,exec.attribute_state());
    SmTArray<SmFace*> current_faces;target->GetFaces(current_faces);passive.push_back(double(current_faces.GetSize()));
    for(ULONG i=0;i<current_faces.GetSize();++i){SmTArray<SmAttribute*> attributes;current_faces[i]->GetAttributes(attributes);std::vector<double> markers;
        for(ULONG j=0;j<attributes.GetSize();++j){auto id=attributes[j]->GetAttributeID();if(id==777||id==778)markers.push_back(double(id));}section(passive,markers);}
    section(out,passive);return out;
}
}
