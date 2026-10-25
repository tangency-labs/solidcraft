// Actual shared attribute identities, with ordered body-qualified edge users.
namespace fillet_real_solid {
inline void merge_attribute_users(std::vector<double>& out,SmAttribute* attribute,const SmTArray<SmEdge*>& target,const SmTArray<SmEdge*>& managed){
    SmTArray<SmAObject*> users;attribute->GetUsers(users);out.insert(out.end(),{double(attribute->GetAttributeID()),double(attribute->GetType()),double(users.GetSize())});
    for(ULONG i=0;i<users.GetSize();++i){double body=0,edge=-1;unsigned matches=0;
        for(ULONG e=0;e<target.GetSize();++e)if(users[i]==target[e]){body=1;edge=e;++matches;}
        for(ULONG e=0;e<managed.GetSize();++e)if(users[i]==managed[e]){body=2;edge=e;++matches;}
        if(matches!=1)throw std::runtime_error("merge attribute user lacks unique current body edge");out.insert(out.end(),{body,edge});
    }
}
inline std::vector<double> merge_attribute_state(FaceExecutive& exec,SmBrep* target){
    SmTArray<SmEdge*> target_edges,managed_edges;target->GetEdges(target_edges);exec.GetFilletBrep()->GetEdges(managed_edges);
    auto* expected=exec.edge_attribute_pointer();std::vector<double> out{double(expected!=nullptr)};
    if(expected)merge_attribute_users(out,expected,target_edges,managed_edges);
    SmTArray<SmAttribute*> tags;
    for(const auto* edges:{&target_edges,&managed_edges})for(ULONG i=0;i<edges->GetSize();++i){SmTArray<SmAttribute*> own;(*edges)[i]->GetAttributes(own);
        for(ULONG a=0;a<own.GetSize();++a)if(own[a]->GetAttributeID()==SM_AI_FILLET_EDGES)tags.AddUnique(own[a]);}
    out.push_back(double(tags.GetSize()));for(ULONG a=0;a<tags.GetSize();++a)merge_attribute_users(out,tags[a],target_edges,managed_edges);
    for(const auto* edges:{&target_edges,&managed_edges}){out.push_back(double(edges->GetSize()));for(ULONG i=0;i<edges->GetSize();++i){SmTArray<SmAttribute*> own,relevant;(*edges)[i]->GetAttributes(own);
        for(ULONG a=0;a<own.GetSize();++a){auto id=own[a]->GetAttributeID();if(id==SM_AI_FILLET_EDGES||id==SM_AI_ALL_FILLET_EDGES)relevant.Add(own[a]);}
        out.push_back(double(relevant.GetSize()));for(ULONG a=0;a<relevant.GetSize();++a){auto* attribute=relevant[a];if(attribute->GetAttributeID()==SM_AI_ALL_FILLET_EDGES&&attribute!=expected)throw std::runtime_error("unexpected unrelated executive edge attribute");
            out.insert(out.end(),{double(attribute->GetAttributeID()),double(attribute==expected),attribute->GetAttributeID()==SM_AI_FILLET_EDGES?label(tags,attribute):-1.0});}
    }}return out;
}
}
