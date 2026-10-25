// SPDX-License-Identifier: Apache-2.0
// Native pinned SmBrep::ValidatePointers / ContainsCoincidentTopology (#431).
#include "sc_edgeuse_graph.h"
#include <SmAssertArray.h>
#include <SmBrepData.h>
#include <SmPlane.h>
#include <array>

namespace {
SmBrep* sheet_grid(size_t count,SmStatus& status,int shared=0){
    const SmContext& ctx=the_context();
    SmBrepConstructor builder;SmBrep* b=builder.StartBrep(ctx);
    SmShell *outer=nullptr,*inner=nullptr;
    if(!b||builder.StartShell(outer,inner)!=SM_SUCCESS){delete b;status=SM_ERR;return nullptr;}
    SmEdge* first_edges[4]={nullptr,nullptr,nullptr,nullptr};
    for(size_t i=0;i<count;i++){
        double x=shared?0.0:(double)i*4.0;
        SmFace* face=builder.StartFace(SM_OT_SAME);
        SmPlane* surface=new(ctx)SmPlane(SmPoint3d(x,0,0),SmVector3d(1,0,0),SmVector3d(0,1,0),SmVector2d(1,1),SmExtent2d(SmPoint2d(0,0),SmPoint2d(1,1)),&ctx);
        if(!face||builder.SetFaceSurface(surface,surface->GetNaturalUVDomain())!=SM_SUCCESS||!builder.StartLoop(SM_OT_SAME)){status=SM_ERR;delete b;return nullptr;}
        SmPoint3d p[4]={SmPoint3d(x,0,0),SmPoint3d(x+1,0,0),SmPoint3d(x+1,1,0),SmPoint3d(x,1,0)};
        for(size_t j=0;j<4;j++){
            size_t k=(j+1)%4;
            SmEdge* original=i&&shared?first_edges[j]:nullptr;
            SmEdge* edge=builder.StartEdge(SM_OT_SAME,shared==1?original:nullptr,
                original&&shared==2?original->GetVertex():nullptr,
                original&&shared==2?original->GetOtherVertex(original->GetVertex()):nullptr);
            if(!edge){status=SM_ERR;delete b;return nullptr;}
            if(!i)first_edges[j]=edge;
            if(original&&shared==1){if(builder.EndEdge()!=SM_SUCCESS){status=SM_ERR;delete b;return nullptr;}continue;}
            SmCurve* c=new(ctx)SmLine(p[k],p[(k+1)%4],3,&ctx);
            SmExtent1d ivl=c->GetNaturalInterval();
            if(builder.SetEdgeCurve(c,FALSE)!=SM_SUCCESS||builder.SetLimits(SmVertexNode(ivl.GetMin()),SmVertexNode(ivl.GetMax()))!=SM_SUCCESS||builder.EndEdge()!=SM_SUCCESS){status=SM_ERR;delete b;return nullptr;}
        }
        if(builder.EndLoop()!=SM_SUCCESS||builder.EndFace()!=SM_SUCCESS){status=SM_ERR;delete b;return nullptr;}
    }
    status=builder.EndShell(FALSE);if(status==SM_SUCCESS)status=builder.EndBrep();
    if(status!=SM_SUCCESS){delete b;return nullptr;}
    return b;
}
using Id=std::array<int64_t,2>;
Id identity(const Graph& g,const void* p,const SmBrep* b) {
    if(!p)return {-1,-1};
    if(p==b)return {16011,0};
    const int64_t types[]={16012,16013,16022,16002,16004,16003,16005,16016,16006,16021};
    for(int k=0;k<10;k++){auto found=g.index[k].find(p);if(found!=g.index[k].end())return {types[k],found->second};}
    for(size_t i=0;i<g.edges.size();i++)if(g.edges[i]->GetCurve()==p)return {20001,(int64_t)i};
    for(size_t i=0;i<g.faces.size();i++)if(g.faces[i]->GetSurface()==p)return {20002,(int64_t)i};
    for(size_t i=0;i<g.edgeuses.size();i++)if((g.edgeuses[i]->*EuPeek::uv())==p)return {20003,(int64_t)i};
    return {-3,-3};
}
void put_id(std::vector<double>& out,const Id& id){out.push_back((double)id[0]);out.push_back((double)id[1]);}
void put_text(std::vector<double>& out,const TCHAR* text){std::vector<double> s;for(const TCHAR* p=text;p&&*p;p++)s.push_back((double)*p);out.push_back((double)s.size());out.insert(out.end(),s.begin(),s.end());}
void put_reports(std::vector<double>& out,const Graph& g,SmBrep* b,SmAssertArray& reports){
    out.push_back((double)reports.GetSize());
    for(ULONG i=0;i<reports.GetSize();i++){
        SmAssertReport* r=reports[i];
        out.insert(out.end(),{(double)r->m_lReportingType,(double)r->m_lListIndex,(double)r->m_lTestIndex,(double)r->m_eAssertType,flg(r->m_bOK)});
        put_id(out,identity(g,r->m_pReporter,b));put_id(out,identity(g,r->m_pOwner,b));put_id(out,identity(g,r->m_pOther,b));
        out.insert(out.end(),{(double)r->m_lOwnerType,(double)r->m_lOtherType,(double)r->m_sTol3d,r->m_dValue});
        put_text(out,r->m_pName);put_text(out,r->m_pMessage);put_text(out,r->m_pMessageForFile);put_text(out,r->m_sOwnerTypeString);put_text(out,r->m_sOtherTypeString);
    }
    out.push_back(flg(reports.GetLastAdd()));
}
void metadata(std::vector<double>& out,Graph& g,SmBrep* b){
    SmTArray<SmFace*> faces;SmTArray<SmEdge*> edges;SmTArray<SmVertex*> vertices;
    b->GetFaces(faces);b->GetEdges(edges);b->GetVertices(vertices);
    for(auto* p:{&faces}){out.push_back((double)p->GetSize());for(ULONG i=0;i<p->GetSize();i++)out.push_back((double)g.find(3,(*p)[i]));}
    out.push_back((double)edges.GetSize());for(ULONG i=0;i<edges.GetSize();i++)out.push_back((double)g.find(7,edges[i]));
    out.push_back((double)vertices.GetSize());for(ULONG i=0;i<vertices.GetSize();i++)out.push_back((double)g.find(9,vertices[i]));
    for(SmFace* f:g.faces){out.push_back((double)g.find(2,f->GetUpwardFaceuse()));put_id(out,identity(g,f->GetSurface()?f->GetSurface()->GetOwner():nullptr,b));}
    for(SmEdge* e:g.edges)put_id(out,identity(g,e->GetCurve()?e->GetCurve()->GetOwner():nullptr,b));
}
void geometry(std::vector<double>& out,Graph& g){
    out.push_back((double)(g.edges.size()+g.edgeuses.size()));
    for(size_t n=0;n<g.edges.size()+g.edgeuses.size();n++){
        SmCurve* c=n<g.edges.size()?g.edges[n]->GetCurve():(g.edgeuses[n-g.edges.size()]->*EuPeek::uv());
        out.push_back(c?(double)c->GetType():-1.0);
        if(!c)continue;
        SmExtent1d domain=c->GetNaturalInterval();out.push_back(domain.GetMin());out.push_back(domain.GetMax());
        for(int i=0;i<5;i++){SmPoint3d p;SmStatus status=c->EvaluatePoint(domain.Evaluate((double)i/4),p);out.push_back((double)status);if(status==SM_SUCCESS)put_vec(out,p);}
    }
}
struct EuCcwPeek:SmEdgeuse{static SmEdgeuse* SmEdgeuse::*ccw(){return &EuCcwPeek::m_pCCW;}};
struct EdCurvePeek:SmEdge{static SmCurve* SmEdge::*curve(){return &EdCurvePeek::m_pCurve;}};
struct FcSurfacePeek:SmFace{static SmSurface* SmFace::*surface(){return &FcSurfacePeek::m_pSurface;}};
struct FuOrientPeek:SmFaceuse{static SmOrientType SmFaceuse::*orient(){return &FuOrientPeek::m_eOrientation;}};
struct LuOrientPeek:SmLoopuse{static SmOrientType SmLoopuse::*orient(){return &LuOrientPeek::m_eOrientation;}};
struct VertexPointPeek:SmVertex{static SmPoint3d SmVertex::*point(){return &VertexPointPeek::m_vPoint;}};
struct Mutation {
    Graph& g;int kind;size_t index;double old=0;SmObject* owner=nullptr;SmCurve* curve=nullptr;SmSurface* surface=nullptr;SmEdgeuse* ccw=nullptr;SmPoint3d point;
    Mutation(Graph& graph,int k,size_t ix,double value):g(graph),kind(k),index(ix){
        if(kind==1&&!g.vertices.empty()){index%=g.vertices.size();old=g.vertices[index]->GetTolerance();g.vertices[index]->SetTolerance(value,FALSE);}
        if(kind==2&&!g.edges.empty()){index%=g.edges.size();old=g.edges[index]->GetTolerance();g.edges[index]->SetTolerance(value,FALSE,FALSE);}
        if(kind==3&&!g.faces.empty()){index%=g.faces.size();old=g.faces[index]->GetTolerance();g.faces[index]->SetTolerance(value,FALSE,FALSE);}
        if(kind==4&&!g.faceuses.empty()){index%=g.faceuses.size();old=g.faceuses[index]->GetOrientation();g.faceuses[index]->*FuOrientPeek::orient()=(SmOrientType)(int)value;}
        if(kind==5&&!g.loopuses.empty()){index%=g.loopuses.size();old=g.loopuses[index]->GetOrientation();g.loopuses[index]->*LuOrientPeek::orient()=(SmOrientType)(int)value;}
        if(kind==6&&!g.vertices.empty()){index%=g.vertices.size();point=g.vertices[index]->GetPoint();g.vertices[index]->*VertexPointPeek::point()=point+SmVector3d(value,0,0);}
        if(kind==7&&!g.edges.empty()){index%=g.edges.size();curve=g.edges[index]->GetCurve();if(curve){owner=curve->GetOwner();curve->SetOwner(NULL);}}
        if(kind==8&&!g.faces.empty()){index%=g.faces.size();surface=g.faces[index]->GetSurface();g.faces[index]->*FcSurfacePeek::surface()=nullptr;}
        if(kind==9&&!g.edges.empty()){index%=g.edges.size();curve=g.edges[index]->GetCurve();g.edges[index]->*EdCurvePeek::curve()=nullptr;}
        if(kind==10&&!g.edgeuses.empty()){index%=g.edgeuses.size();ccw=g.edgeuses[index]->GetCCWEdgeuse();g.edgeuses[index]->*EuCcwPeek::ccw()=nullptr;}
    }
    ~Mutation(){
        if(kind==1&&!g.vertices.empty())g.vertices[index]->SetTolerance(old,FALSE);
        if(kind==2&&!g.edges.empty())g.edges[index]->SetTolerance(old,FALSE,FALSE);
        if(kind==3&&!g.faces.empty())g.faces[index]->SetTolerance(old,FALSE,FALSE);
        if(kind==4&&!g.faceuses.empty())g.faceuses[index]->*FuOrientPeek::orient()=(SmOrientType)(int)old;
        if(kind==5&&!g.loopuses.empty())g.loopuses[index]->*LuOrientPeek::orient()=(SmOrientType)(int)old;
        if(kind==6&&!g.vertices.empty())g.vertices[index]->*VertexPointPeek::point()=point;
        if(kind==7&&curve)curve->SetOwner(owner);
        if(kind==8&&!g.faces.empty())g.faces[index]->*FcSurfacePeek::surface()=surface;
        if(kind==9&&!g.edges.empty())g.edges[index]->*EdCurvePeek::curve()=curve;
        if(kind==10&&!g.edgeuses.empty())g.edgeuses[index]->*EuCcwPeek::ccw()=ccw;
    }
};
}
SC_EXPORT int32_t sc_topology_brep_validation_eval(int32_t op,const char* path,const double* input,int64_t n,const double* args,int64_t na,double* out,int64_t cap,int64_t* len){
    if(op<0||op>2||!input||n<1||!args||na!=7)return SC_ERR_ARG;
    if(input[0]==100.0&&(n!=2||input[1]<1||input[1]>512||input[1]!=(double)(int)input[1]))return SC_ERR_ARG;
    if(input[0]==101.0&&(n!=2||(input[1]!=1.0&&input[1]!=2.0)))return SC_ERR_ARG;
    SC_TRY
    Rd rd{input,0};SmStatus status=SM_SUCCESS;Built built;built.brep=(input[0]==100.0)?sheet_grid((size_t)input[1],status):(input[0]==101.0)?sheet_grid(2,status,(int)input[1]):build(rd,path,status);
    std::vector<double> values;
    if(!built.brep){values={-1,(double)status};return sc_copy_out(values.data(),(int64_t)values.size(),out,cap,len);}
    Graph g;walk(built.brep,g);values.push_back(0);
    if(op==0){std::vector<double> graph;graph.push_back(0);export_graph(built.brep,g,graph);values.push_back((double)graph.size());values.insert(values.end(),graph.begin(),graph.end());metadata(values,g,built.brep);}
    else{
        Mutation mutation(g,(int)args[4],(size_t)args[5],args[6]);SmAssertArray reports;
        SmTArray<SmTopology*> first,second;
        if(op==1)values.push_back((double)built.brep->ValidatePointers(&reports,(SmAssertTestLevel)(int)args[0],nullptr,args[1]!=0?SM_WALK:SM_NO_WALK));
        else values.push_back(flg(built.brep->ContainsCoincidentTopology(args[2],&first,&second,&reports,(SmAssertTestLevel)(int)args[0],nullptr,args[3]!=0)));
        values.push_back((double)first.GetSize());for(ULONG i=0;i<first.GetSize();i++){put_id(values,identity(g,first[i],built.brep));put_id(values,identity(g,second[i],built.brep));}
        put_reports(values,g,built.brep,reports);geometry(values,g);
    }
    return sc_copy_out(values.data(),(int64_t)values.size(),out,cap,len);
    SC_CATCH
}
