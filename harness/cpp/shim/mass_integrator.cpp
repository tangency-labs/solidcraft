// SPDX-License-Identifier: Apache-2.0
// Calls the pinned SmIntegrator / SmODEIntegrator implementations, without copies
// of the algorithm under test. Evaluator arrays are adapted to upstream's index 1.
#include "sc_shim.h"
#include "StdAfx.h"
#include <SmIntegrator.h>
#include <cmath>
#include <vector>

namespace {
struct Eval : SmODEIntegFuncEvalObj {
    int kind;
    const double* c;
    int64_t n;
    Eval(int k, const double* p, int64_t count) : kind(k), c(p), n(count) {}
    SmStatus Evaluate(double t, SmTArray<double>& y, SmTArray<double>& dy) const override {
        for (int64_t i=0; i<n; ++i) {
            const double* p=c+4*i;
            if (kind==3 && t>=p[0]) return SM_ERR;
            if (kind==0 || kind==3) dy[i+1]=p[0]+p[1]*t+p[2]*t*t+p[3]*y[i+1];
            if (kind==1) dy[i+1]=p[0]*std::sin(p[1]*t+p[2])+p[3]*y[i+1];
            if (kind==2) dy[i+1]=p[0]*std::exp(p[1]*t)+p[2]*y[i+1]+p[3];
            if (!std::isfinite(dy[i+1])) return SM_ERR;
        }
        return SM_SUCCESS;
    }
};
struct Scalar : SmIntegFuncEvalObj {
    const Eval& eval;
    explicit Scalar(const Eval& e) : eval(e) {}
    SmStatus Evaluate(double t, double& result) const override {
        SmTArray<double> y(2, NULL, 2), dy(2, NULL, 2);
        y.SetAll(0.0); dy.SetAll(0.0);
        SmStatus st=eval.Evaluate(t,y,dy);
        result=dy[1];
        return st;
    }
};
}

// op=0 scalar, 1 ODE, 2 adaptive step. controls: x1 x2 eps h1 hmin tiny algorithm.
// Result: status then on success scalar, or y + good/reduced, or y+x+hdid+hnext+scale.
SC_EXPORT int32_t sc_mass_integrator_eval(int32_t op, int32_t kind, const double* coeff,
    const double* initial, int64_t n, const double* scale, int64_t ns,
    const double* controls, int64_t max_steps, double* out, int64_t cap, int64_t* len)
{
    if (op<0 || op>2 || kind<0 || kind>3 || n<0 || n>4096 || (ns!=0 && ns!=n)
        || (op==0 && n!=1) || (op==2 && ns!=n) || max_steps<0 || max_steps>1000000)
        return SC_ERR_ARG;
    SC_TRY
    Eval eval(kind,coeff,n);
    std::vector<double> result;
    if (op==0) {
        Scalar scalar(eval);
        SmIntegrator integrator(scalar);
        double value=0.0;
        SmStatus st=integrator.IntegrateIt(controls[0],controls[1],
            controls[6]==0 ? SM_IA_ROMBERG : SM_IA_SIMPSONS,controls[2],value);
        result.push_back(st==SM_SUCCESS?0.0:(double)st);
        if (st==SM_SUCCESS) result.push_back(value);
    } else {
        SmODEIntegrator integrator(eval,(ULONG)max_steps,controls[5]);
        SmTArray<double> y, ys;
        for (int64_t i=0;i<n;++i) { y.Add(initial[i]); if(ns) ys.Add(scale[i]); }
        SmStatus st;
        ULONG good=0,bad=0;
        double x=controls[0], taken=0.0, next=0.0;
        if (op==1) st=integrator.RungeKuttaODEIntegrate(y,controls[0],controls[1],
            controls[2],controls[3],controls[4],good,bad,ns ? &ys : NULL);
        else {
            SmTArray<double> y1(n+1,NULL,n+1), dy(n+1,NULL,n+1), s1(n+1,NULL,n+1);
            y1.SetAll(0.0); dy.SetAll(0.0); s1.SetAll(0.0);
            for(int64_t i=0;i<n;++i) { y1[i+1]=initial[i]; s1[i+1]=scale[i]; }
            st=eval.Evaluate(x,y1,dy);
            if(st==SM_SUCCESS) st=integrator.RungeKutta5thOrderStep(y1,dy,x,controls[3],controls[2],s1,taken,next);
            for(int64_t i=0;i<n;++i) { y[i]=y1[i+1]; ys[i]=s1[i+1]; }
        }
        result.push_back(st==SM_SUCCESS?0.0:(double)st);
        if (st==SM_SUCCESS) {
            for (int64_t i=0;i<n;++i) result.push_back(y[i]);
            if(op==1) { result.push_back(good); result.push_back(bad); }
            else { result.push_back(x); result.push_back(taken); result.push_back(next);
                for (int64_t i=0;i<n;++i) result.push_back(ys[i]); }
        }
    }
    return sc_copy_out(result.data(),(int64_t)result.size(),out,cap,len);
    SC_CATCH
}

#include <SmBrep.h>
#include <SmEdge.h>
#include <SmFace.h>
#include <SmLine.h>
#include <SmPlane.h>
#include <SmCircle.h>
#include <SmBSplineSurface.h>
#include <SmBSplineCurve.h>
#include <SmContext.h>
#include <SmApiQueries.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <nurbs.h>
#include <memory>

namespace {
void put_moments(std::vector<double>& o, const SmTArray<SmVector3d>& m) {
    for (ULONG i=0;i<m.GetSize();++i) {o.push_back(m[i].x);o.push_back(m[i].y);o.push_back(m[i].z);}
}
}
// kind 0: quadratic Bezier, 1: real SmLine through p0 with vector p1.
// op 0: precise edge props, 1: public edge Length, 2: precise B-rep properties.
SC_EXPORT int32_t sc_mass_integrator_edge(int32_t op,int32_t kind,const double* p,
    const double* weights,const double* controls,double* out,int64_t cap,int64_t* len) {
    if(op<0||op>2||kind<0||kind>1)return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    std::unique_ptr<SmBrep> brep(new(ctx)SmBrep());
    SmBSplineCurve* c=nullptr;
    if(kind==1)c=new(ctx)SmLine(SmPoint3d(p[0],p[1],p[2]),SmVector3d(p[3],p[4],p[5]),SmExtent1d(0,1),1.0,3,&ctx);
    else {
        gw_CURVE* cur=sm_AllocateNurbCurve(2,2,5);
        for(int i=0;i<3;++i){cur->pol->Pw[i].x=p[3*i];cur->pol->Pw[i].y=p[3*i+1];cur->pol->Pw[i].z=p[3*i+2];cur->pol->Pw[i].w=weights[i];}
        for(int i=0;i<6;++i)cur->knt->U[i]=i<3?0.0:1.0;
        c=new(ctx)SmBSplineCurve(cur,3,FALSE);
    }
    SmEdge* edge=nullptr;
    SmStatus st=brep->CreateWireEdgeFromCurve(c,SmExtent1d(controls[0],controls[1]),edge);
    std::vector<double> result;
    if(st!=SM_SUCCESS){result.push_back(st==SM_SUCCESS?0.0:(double)st);return sc_copy_out(result.data(),result.size(),out,cap,len);}
    double length=0,area=0,volume=0,mass=0;
    SmTArray<SmVector3d> moments;
    SmPoint3d origin(controls[3],controls[4],controls[5]);
    if(op==0)st=edge->ComputePreciseProperties(controls[2],origin,controls[6],length,area,volume,moments);
    if(op==1)st=SmApiEdgeComputeLength(edge,controls[2],length);
    if(op==2){SmMassPropertyBehavior behavior(1,0,controls[6]);st=brep->ComputePreciseProperties(controls[2],origin,behavior,area,volume,mass,moments);}
    result.push_back(st==SM_SUCCESS?0.0:(double)st);
    if(st==SM_SUCCESS){if(op!=2)result.push_back(length);if(op!=1){result.push_back(area);result.push_back(volume);if(op==2)result.push_back(mass);put_moments(result,moments);}}
    return sc_copy_out(result.data(),result.size(),out,cap,len);
    SC_CATCH
}

// p: plane origin[3], UV scales[2], UV domain[4], computation origin[3], accuracy,
// thickness, orientation (0 SAME, 1 OPPOSITE), property flags.
// kind 0 rectangle, 1 disk, 2 annulus, 3 quadratic B-spline patch, 4 rotated plane.
// op 0 properties, 1 export input UV domain/bbox scale/oriented trims, 2 API area, 3 API centroid,
// 4 API Brep area, 5 API Brep volume, 6 API Brep mass properties (sheet rejection), 7 Surface::Area.
SC_EXPORT int32_t sc_mass_integrator_face(int32_t op,int32_t kind,const double* p,double* out,int64_t cap,int64_t* len){
    if(op<0||op>7||kind<0||kind>4)return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    std::unique_ptr<SmBrep> brep(new(ctx)SmBrep());
    SmExtent2d dom(SmPoint2d(p[5],p[6]),SmPoint2d(p[7],p[8]));
    SmVector3d xaxis=kind==4?SmVector3d(.6,.8,0):SmVector3d(1,0,0);
    SmVector3d yaxis=kind==4?SmVector3d(-.8,.6,0):SmVector3d(0,1,0);
    SmSurface* surface=nullptr;
    if(kind==3){
        gw_SURFACE* s=sm_AllocateNurbSurface(2,2,2,2,5,5);
        for(int i=0;i<3;++i)for(int j=0;j<3;++j){
            s->net->Pw[i][j].x=p[0]+i*.5*p[3];
            s->net->Pw[i][j].y=p[1]+j*.5*p[4];
            s->net->Pw[i][j].z=p[2]+(i==1&&j==1?2.0:0.0);
            s->net->Pw[i][j].w=1.0;
        }
        for(int i=0;i<6;++i){s->knu->U[i]=i<3?0:1;s->knv->U[i]=i<3?0:1;}
        surface=new(ctx)SmBSplineSurface(s,FALSE,NULL);
    }else surface=new(ctx)SmPlane(SmPoint3d(p[0],p[1],p[2]),xaxis,yaxis,SmVector2d(p[3],p[4]),dom,&ctx);
    SmFace* face=nullptr;
    SmStatus st;
    if(kind==1||kind==2){
        SmTArray<ULONG> loops;SmTArray<SmCurve*> curves;SmTArray<SmOrientType> orientations;
        for(int i=0;i<(kind==2?2:1);++i){
            loops.Add(1);orientations.Add(i==0?SM_OT_SAME:SM_OT_OPPOSITE);
            curves.Add(new(ctx)SmCircle(SmPoint3d(p[0],p[1],p[2]),xaxis,yaxis,SmExtent1d(0,360),i==0?.5:.2,3,&ctx));
        }
        SmShell* shell=nullptr;SmTArray<SmFace*> faces;
        st=brep->CreateFacesWithCurves(NULL,loops,&curves,orientations,surface,shell,faces);
        if(st==SM_SUCCESS&&faces.GetSize()==1)face=faces[0];else if(st==SM_SUCCESS)st=SM_ERR;
    }else st=brep->CreateFaceFromSurface(surface,dom,face);
    std::vector<double> result;
    if(st!=SM_SUCCESS){result.push_back(st==SM_SUCCESS?0.0:(double)st);return sc_copy_out(result.data(),result.size(),out,cap,len);}
    if(op==1){
        SmTArray<ULONG> loops;SmTArray<SmBSplineCurve*> curves;
        // Match op 0's order: estimate the scale before creating oriented trims.
        SmExtent3d bbox;face->CalculateBoundingBox(bbox);SmVector3d size=bbox.GetSize();
        double d=smos_Max(size.z,smos_Max(size.x,size.y));
        st=face->CreateOrientedTrimBoundary(ctx,TRUE,TRUE,loops,curves);
        result.push_back(st==SM_SUCCESS?0.0:(double)st);
        if(st==SM_SUCCESS){
            // Export integration inputs only: actual UV domain and bbox-derived scale.
            result.push_back(d*d/100.0);
            SmExtent2d domain=face->GetUVDomain();
            result.push_back(domain.GetMin().x);result.push_back(domain.GetMin().y);
            result.push_back(domain.GetMax().x);result.push_back(domain.GetMax().y);
            result.push_back(curves.GetSize());for(ULONG i=0;i<curves.GetSize();++i){gw_CURVE* c=curves[i]->GetGwNurbPointer();result.push_back(c->pol->n);result.push_back(c->p);result.push_back(c->knt->m);
            for(gw_INDEX j=0;j<=c->pol->n;++j){result.push_back(c->pol->Pw[j].x);result.push_back(c->pol->Pw[j].y);result.push_back(c->pol->Pw[j].z);result.push_back(c->pol->Pw[j].w);}
            for(gw_INDEX j=0;j<=c->knt->m;++j)result.push_back(c->knt->U[j]);}}
        for(ULONG i=0;i<curves.GetSize();++i)delete curves[i];
    }else{
        double area=0,volume=0,mass=0;SmPoint3d centroid;SmVector3d inertia,products;SmTArray<SmVector3d> moments;
        SmPoint3d origin(p[9],p[10],p[11]);
        if(op==0){SmExtent3d bbox;face->CalculateBoundingBox(bbox);SmVector3d s=bbox.GetSize();double d=smos_Max(s.z,smos_Max(s.x,s.y));
            st=face->ComputePreciseProperties(p[14]==0?SM_OT_SAME:SM_OT_OPPOSITE,p[12],origin,d*d/100.0,p[13],area,volume,moments,(ULONG)p[15]);}
        if(op==7)st=face->GetSurface()->Area(dom,p[12],area);
        if(op==2)st=SmApiFaceComputeArea(face,p[12],area);
        if(op==3)st=SmApiFaceComputeCentroid(face,p[12],area,centroid);
        if(op==4)st=SmApiBrepComputeArea(brep.get(),p[12],area);
        if(op==5)st=SmApiBrepComputeVolume(brep.get(),p[12],volume);
        if(op==6)st=SmApiBrepComputeMassProperties(brep.get(),p[12],1.0,origin,area,volume,mass,centroid,inertia,products);
        result.push_back(st==SM_SUCCESS?0.0:(double)st);
        if(st==SM_SUCCESS){if(op==5)result.push_back(volume);else{result.push_back(area);if(op==0){result.push_back(volume);put_moments(result,moments);}if(op==3){result.push_back(centroid.x);result.push_back(centroid.y);result.push_back(centroid.z);}}}
    }
    return sc_copy_out(result.data(),result.size(),out,cap,len);
    SC_CATCH
}
