// Actual true-context constructor through ordered geometry/corner insertion.
namespace fillet_real_solid {
inline std::vector<double> insertion_checkpoint(FaceExecutive& exec,SmBrep* target,
                                               SmStatus prepare,const std::vector<double>& before,bool select_original=false,bool delete_original=false,bool merge_original=false) {
    auto* context=const_cast<SmContext*>(exec.GetContext());
    const auto old_boolean=context->GetDoingBoolean();
    const auto old_edit=target->m_bEditingEnabled;
    struct Restore {
        SmContext* context;SmBrep* target;SmBoolean boolean_value,editing_value;
        ~Restore(){target->m_bEditingEnabled=editing_value;context->SetDoingBoolean(boolean_value);}
    } restore{context,target,old_boolean,old_edit};
    std::vector<double> statuses{status(prepare),-1,-1,-1,-1},passive_before,passive_after,intervals_before,intervals_after;
    std::vector<double> selection,selection_intervals;
    SmStatus result=prepare;
    auto& ti=exec.GetTopologyIntersector();
    auto observe=[&](std::vector<double>& state,std::vector<double>& intervals){
        auto* managed=exec.GetFilletBrep();
        state={double(context->GetDoingBoolean()),
            double(ti.m_bDoingFillets),double(target->m_bEditingEnabled),double(ti.GetPrimaryBrep()==target),
            double(managed&&ti.GetOtherBrep()==managed),double(ti.m_bTopologyDeleted)};
        counts(state,target);if(managed)counts(state,managed);else state.insert(state.end(),{0,0,0});
        counts(state,exec.GetPseudoBrep());
        // Current owning traversals only; no old pointers survive a topology edit.
        auto maps=edge_map_state(exec,target);section(state,maps.first);intervals=maps.second;
        section(state,source_surface_binding(exec,managed));section(state,exec.attribute_state());
        SmTArray<SmFace*> faces;target->GetFaces(faces);state.push_back(double(faces.GetSize()));
        for(ULONG i=0;i<faces.GetSize();++i){SmTArray<SmAttribute*> attributes;faces[i]->GetAttributes(attributes);
            std::vector<double> markers;for(ULONG j=0;j<attributes.GetSize();++j){auto id=attributes[j]->GetAttributeID();if(id==777||id==778)markers.push_back(double(id));}
            section(state,markers);
        }
    };
    if(result==SM_SUCCESS){result=exec.CreateFilletCorners();statuses[1]=status(result);}
    if(result==SM_SUCCESS){
        // Native DoFilleting establishes these flags before CreateFilletBrep.
        context->SetDoingBoolean(TRUE);ti.m_bDoingFillets=TRUE;
        result=exec.CreateFilletBrep(nullptr);statuses[2]=status(result);
    }
    if(result==SM_SUCCESS){
        statuses[3]=0; // Completed void/ignored-status first-FG owner shrink loop.
        for(ULONG i=0;i<exec.GetFilletSolvers().GetSize();++i){SmTArray<SmFilletGeom*> geometries;exec.GetFilletSolvers()[i]->GetFilletGeoms(geometries);
            if(geometries.GetSize()){auto* surface=geometries[0]->GetFilletSurface();auto* face=surface?SM_CAST_PTR(SmFace,surface->GetOwner()):nullptr;if(face)face->ShrinkGeometry();}
        }
        observe(passive_before,intervals_before);
        target->m_bEditingEnabled=TRUE;ti.SetPrimaryBrep(target);ti.SetOtherBrep(exec.GetFilletBrep());
        result=exec.SetFilletBrepEdgeMap();
        for(ULONG i=0;result==SM_SUCCESS&&i<exec.GetFilletSolvers().GetSize();++i){
            SmTArray<SmFilletGeom*> geometries;exec.GetFilletSolvers()[i]->GetFilletGeoms(geometries);
            for(ULONG j=0;result==SM_SUCCESS&&j<geometries.GetSize();++j){result=geometries[j]->InsertIntersectionTopology();}
        }
        for(ULONG i=0;result==SM_SUCCESS&&i<exec.GetFilletCorners().GetSize();++i){result=exec.GetFilletCorners()[i]->InsertIntersectionTopology();}
        statuses[4]=status(result);target->m_bEditingEnabled=old_edit;observe(passive_after,intervals_after);
        if(result==SM_SUCCESS && select_original)selection=original_selection(exec,target,result);
        if(result==SM_SUCCESS && delete_original)selection=original_deletion(exec,target,result,selection_intervals);
        if(result==SM_SUCCESS && merge_original)selection=original_merge(exec,target,result,selection_intervals);
    }
    std::vector<double> out;section(out,before);section(out,statuses);section(out,passive_before);section(out,intervals_before);section(out,passive_after);section(out,intervals_after);
    // Failure snapshots remain passive until that partial geometry's query safety is audited.
    out.push_back(double(result==SM_SUCCESS));
    if(result==SM_SUCCESS){
        auto target_record=intermediate_faces(target),managed_record=intermediate_faces(exec.GetFilletBrep());
        section(out,target_record.first);section(out,target_record.second);
        section(out,managed_record.first);section(out,managed_record.second);
    }
    if(select_original)section(out,selection);
    if(delete_original||merge_original){section(out,selection);section(out,selection_intervals);}
    return out;
}
}
