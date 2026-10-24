// Passive attached classification readback: no evaluation or lazy geometry calls.
namespace fillet_real_solid {
struct ClassificationFrame {
    SmTArray<SmVertex*> vertices; SmTArray<SmEdge*> edges;
    SmTArray<SmFace*> faces; SmTArray<SmEdgeuse*> uses;
    explicit ClassificationFrame(SmBrep* body) {
        body->GetVertices(vertices); body->GetEdges(edges); body->GetFaces(faces);
        for(ULONG i=0;i<edges.GetSize();++i) { SmTArray<SmEdgeuse*> own;edges[i]->GetEdgeuses(own);uses.Append(own); }
    }
};
void class_object(std::vector<double>& state,const SmObject* object,const ClassificationFrame& target,const ClassificationFrame& pseudo) {
    if(!object){state.insert(state.end(),{0,0,-1});return;}
    for(int domain=1;domain<=2;++domain){const auto& frame=domain==1?target:pseudo;
        for(ULONG i=0;i<frame.vertices.GetSize();++i)if(object==frame.vertices[i]){state.insert(state.end(),{double(domain),1,double(i)});return;}
        for(ULONG i=0;i<frame.edges.GetSize();++i)if(object==frame.edges[i]){state.insert(state.end(),{double(domain),2,double(i)});return;}
        for(ULONG i=0;i<frame.faces.GetSize();++i)if(object==frame.faces[i]){state.insert(state.end(),{double(domain),3,double(i)});return;}
        for(ULONG i=0;i<frame.uses.GetSize();++i)if(object==frame.uses[i]){state.insert(state.end(),{double(domain),4,double(i)});return;}
    }
    state.insert(state.end(),{-1,-1,-2});
}
void class_point(std::vector<double>& state,std::vector<double>& values,const SmPointClassification& point,const ClassificationFrame& target,const ClassificationFrame& pseudo) {
    state.push_back(double(point.GetPointClass()));class_object(state,point.GetObject(),target,pseudo);class_object(state,point.GetFace(),target,pseudo);
    state.push_back(double(point.AreParametersSet()));const auto uv=point.GetUVParam();const auto uvw=point.GetUVWParam();
    values.insert(values.end(),{point.GetTParam(),uv.x,uv.y,uvw.x,uvw.y,uvw.z,point.GetPreSnapParam()});
}
std::pair<std::vector<double>,std::vector<double>> rail_classification(SmFilletExecutive& exec,SmBrep* target_body) {
    ClassificationFrame target(target_body),pseudo(exec.GetPseudoBrep());
    std::vector<double> state{double(exec.GetDoClassification()),double(exec.GetContext()->GetDoingBoolean())},values;
    for(ULONG s=0;s<exec.GetFilletSolvers().GetSize();++s){SmTArray<SmFilletGeom*> geometries;exec.GetFilletSolvers()[s]->GetFilletGeoms(geometries);
        for(ULONG g=0;g<geometries.GetSize();++g)for(ULONG r=0;r<2;++r){auto* rail=geometries[g]->GetRail(r);auto* cc=rail->GetCurveClass();
            state.insert(state.end(),{label(pseudo.edges,static_cast<SmEdge*>(rail)),double(cc!=nullptr)});if(!cc)continue;
            auto* curve=cc->GetCurve();auto* mate=rail->GetPrimaryEdgeuse()->GetMate()->GetUVTrimCurvePointer();auto* face=rail->GetOriginalFace();auto* uv=cc->GetAnyUVCurve1();
            state.insert(state.end(),{double(curve!=nullptr),double(curve&&curve==rail->GetCurve()),curve?double(curve->GetType()):0});
            class_object(state,cc->GetFace(),target,pseudo);class_object(state,face,target,pseudo);
            state.insert(state.end(),{double(uv!=nullptr),double(mate!=nullptr),double(uv&&mate&&uv==mate),double(cc->GetAnyUVCurve2()!=nullptr),
                double(cc->GetSurface1()!=nullptr),double(cc->GetSurface1()&&face&&cc->GetSurface1()==face->GetSurface()),double(cc->GetSurface2()!=nullptr),
                double(cc->GetDoingFillets()),double(cc->GetIsCurveCurrent()),double(cc->GetIgnoreTangency()),double(cc->GetOKToSetTolerance()),double(cc->GetTgtLoopsOnly()),double(cc->GetOtherClassification()!=nullptr),double(cc->GetSize())});
            auto interval=cc->GetInterval();values.insert(values.end(),{interval.GetMin(),interval.GetMax()});
            for(ULONG i=0;i<cc->GetSize();++i){const auto& part=(*cc)[i];interval=part.GetInterval();values.insert(values.end(),{interval.GetMin(),interval.GetMax()});
                class_point(state,values,part.m_vStart,target,pseudo);class_point(state,values,part.m_vMid,target,pseudo);class_point(state,values,part.m_vEnd,target,pseudo);}
        }
    }
    state.push_back(double(target.vertices.GetSize()));for(ULONG i=0;i<target.vertices.GetSize();++i)values.push_back(target.vertices[i]->GetTolerance());
    state.push_back(double(target.edges.GetSize()));for(ULONG i=0;i<target.edges.GetSize();++i)values.push_back(target.edges[i]->GetTolerance());
    return {std::move(state),std::move(values)};
}
}
