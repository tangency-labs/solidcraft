// SPDX-License-Identifier: Apache-2.0
// Native differential adapter for the four functions owned by surface.cone_p3.
#include "sc_shim.h"
#include "StdAfx.h"
#include <SmCone.h>
#include <SmCylinder.h>
#include <SmPlane.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmBSplineCurve.h>
#include <SmLine.h>
#include <SmEllipse.h>
#include <SmNurbsCrv.h>
#include <nurbs.h>
#include <SmGeomUtility.h>
#include <memory>
#include <vector>
#include <stdexcept>

namespace {
struct Bad : std::runtime_error { Bad() : std::runtime_error("unexpected curve") {} };
struct PeekC : SmBSplineCurve {
    static SmKnotType SmBSplineCurve::*kt() { return &PeekC::m_eKnotType; }
    static SmBSplineCurveForm SmBSplineCurve::*form() { return &PeekC::m_eBSplineCurveForm; }
};
void put_vec(std::vector<double>& o, const SmVector3d& v) { o.insert(o.end(), {v.x,v.y,v.z}); }
void put_ivl(std::vector<double>& o, const SmExtent1d& v) { o.insert(o.end(), {v.GetMin(),v.GetMax()}); }
void put_curve_nurb(std::vector<double>& o, const SmBSplineCurve* c)
{
    o.push_back((double)(c->*PeekC::kt()));
    o.push_back((double)(c->*PeekC::form()));
    const gw_CURVE* cur = c->GetGwNurbPointer();
    o.push_back(cur ? 1.0 : 0.0);
    if (!cur)
        return;
    gw_INDEX n = cur->pol->n;
    gw_INDEX m = cur->knt->m;
    o.push_back((double)n);
    o.push_back((double)cur->p);
    o.push_back((double)m);
    for (gw_INDEX i = 0; i <= n; i++)
    {
        o.push_back(cur->pol->Pw[i].x);
        o.push_back(cur->pol->Pw[i].y);
        o.push_back(cur->pol->Pw[i].z);
        o.push_back(cur->pol->Pw[i].w);
    }
    for (gw_INDEX i = 0; i <= m; i++)
        o.push_back(cur->knt->U[i]);
}

// `has` then type dim [line: P V dom scale inside_out] [ellipse: C X Y rx ry dom inside_out]
// B-spline part, natural interval.
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    o.push_back(c ? 1.0 : 0.0);
    if (!c)
        return;
    o.push_back((double)c->GetType());
    o.push_back((double)c->GetDim());
    if (c->IsKindOf(SmLine_TYPE))
    {
        const SmLine* l = (const SmLine*)c;
        put_vec(o, l->GetLinePoint());
        put_vec(o, l->GetLineVector());
        put_ivl(o, l->GetSTEPInterval());
        o.push_back(l->GetLineScale());
        o.push_back(l->GetInsideOut() ? 1.0 : 0.0);
    }
    else if (c->IsKindOf(SmEllipse_TYPE))
    {
        const SmEllipse* e = (const SmEllipse*)c;
        const SmAxis2Placement& pos = e->GetPosition();
        put_vec(o, pos.GetOrigin());
        put_vec(o, pos.GetXAxis());
        put_vec(o, pos.GetYAxis());
        o.push_back(e->GetXRadius());
        o.push_back(e->GetYRadius());
        put_ivl(o, e->GetSTEPInterval());
        o.push_back(e->GetInsideOut() ? 1.0 : 0.0);
    }
    if (!c->IsKindOf(SmBSplineCurve_TYPE))
        throw Bad();
    put_curve_nurb(o, (const SmBSplineCurve*)c);
    put_ivl(o, c->GetNaturalInterval());
}


struct MissingPolarCurve {};
template<class Base> struct CheckedCone : Base {
    using Base::Base;
    SmStatus ConvertUVFromSTEPToNURBS(const SmPoint2d& step, SmPoint2d& nurbs) const override {
        // MakeNurb ignores RebuildPolarConverter's failure. ConvertToNURBSParameter
        // then dereferences GetCurve() without checking it (SmEllipse.cpp:491).
        // Guard the actual conversion, preserving early returns and converters
        // that have a curve but no exact table (HavePolarConversion() == FALSE).
        if (!this->m_vPolarConverter.GetCurve()) throw MissingPolarCurve();
        return Base::ConvertUVFromSTEPToNURBS(step, nurbs);
    }
};
struct TestCone : CheckedCone<SmCone> {
    TestCone(const double* v) : CheckedCone<SmCone>(SmPoint3d(v[0],v[1],v[2]), SmVector3d(v[3],v[4],v[5]),
        SmVector3d(v[6],v[7],v[8]),v[9],v[10],v[12],v[13],v[11],(SmBoolean)v[14],(SmBoolean)v[15],FALSE) {}
};
SmCone* make_cone(const SmContext& ctx, const double* v) {
    if(v[16]) return new(ctx) CheckedCone<SmCylinder>(SmPoint3d(v[0],v[1],v[2]),SmVector3d(v[3],v[4],v[5]),
        SmVector3d(v[6],v[7],v[8]),v[9],v[12],v[13],v[11],(SmBoolean)v[14],(SmBoolean)v[15],FALSE,&ctx);
    return new(ctx) TestCone(v);
}
SmExtent2d domain(const SmSurface& s, const double* f) {
    SmExtent2d n=s.GetNaturalUVDomain();
    double u=n.GetUMin(),v=n.GetVMin(),du=n.GetUMax()-u,dv=n.GetVMax()-v;
    return SmExtent2d(u+du*f[0],v+dv*f[1],u+du*f[2],v+dv*f[3]);
}
struct Curves {
    SmTArray<SmCurve*> xyz,uv1,uv2;
    SmTArray<SmTsectCurveType> types;
    SmTArray<double> deviations;
    ~Curves() {
        for(ULONG i=0;i<xyz.GetSize();++i) delete xyz[i];
        for(ULONG i=0;i<uv1.GetSize();++i) delete uv1[i];
        for(ULONG i=0;i<uv2.GetSize();++i) delete uv2[i];
    }
};
}
SC_EXPORT int32_t sc_surface_cone_p3_intersect(int32_t op,const double* cone,const double* other,
    const double* domains,double tolerance,int32_t has_tolerance,int32_t same,
    double* out,int64_t cap,int64_t* len) {
    SC_NATIVE_LOCK;
    try {
    SmContext ctx;
    std::unique_ptr<SmCone> a(make_cone(ctx,cone));
    SmStatus build=a->MakeNurb();
    if(build!=SM_SUCCESS) { double r[3]={(double)build,2.0,0.0}; return sc_copy_out(r,3,out,cap,len); }
    std::unique_ptr<SmSurface> b;
    if(op==0 || op==3) { b.reset(make_cone(ctx,other)); build=((SmCone*)b.get())->MakeNurb(); }
    else {
        b.reset(new(ctx) SmPlane(SmPoint3d(other[0],other[1],other[2]),SmVector3d(other[3],other[4],other[5]),
            SmVector3d(other[6],other[7],other[8]),SmVector2d(1,1),SmExtent2d(other[9],other[10],other[11],other[12]),&ctx));
        if(!((SmPlane*)b.get())->GetOrCreateGwNurbPointer()) build=SM_ERR;
    }
    if(build!=SM_SUCCESS) { double r[3]={(double)build,2.0,0.0}; return sc_copy_out(r,3,out,cap,len); }
    SmSurface* target=(same ? a.get() : b.get());
    SmExtent2d da=domain(*a,domains), db=domain(*target,domains+4);
    SmApproxTol3d tol(tolerance);
    SmBoolean more=UNSURE;
    SmBoolean edges[2]={TRUE,TRUE};
    Curves curves;
    SmStatus status;
    // Native IntersectWithCone's coincident branch dereferences pdOptApproxTol3d.
    // Supplying a tolerance is required only there; preserve all other default paths.
    if((op==0 || (op==3 && target->GetType()==SmCone_TYPE)) && !same && !has_tolerance && a->IsCylinder() && ((SmCone*)target)->IsCylinder()) {
        const SmCone& c=*(SmCone*)target;
        SmExtent3d box1,box2;
        if(a->CalculateBoundingBox(da,&box1)==SM_SUCCESS && c.CalculateBoundingBox(db,&box2)==SM_SUCCESS) {
            double tol_default=SM_EFF_ZERO_SQRT*(1+box2.GetSize().Length());
            box2.ExpandAbsolute(tol_default);
            if(!box1.AreDisjoint(box2)) {
                SmVector3d axis=a->GetPosition().GetZAxis(), other_axis=c.GetPosition().GetZAxis();
                SmExtent2d d=a->GetSTEPUVDomain(), e=c.GetSTEPUVDomain();
                SmPoint3d bottom=a->GetPosition().GetOrigin()+d.GetVMin()*axis;
                SmPoint3d other_bottom=c.GetPosition().GetOrigin()+e.GetVMin()*other_axis;
                SmPoint3d other_top=c.GetPosition().GetOrigin()+e.GetVMax()*other_axis;
                double angle=0; axis.AngleBetween(other_axis,angle);
                if(angle>SM_PI/2) angle=SM_PI-angle;
                if(angle<SM_EFF_ZERO_SQRT) {
                    double t=0,t2=0;
                    smgu_LineClosestPoint(bottom,axis,other_bottom,t);
                    double distance=(bottom+t*axis).DistanceBetween(other_bottom);
                    double rmin=smos_Min(a->GetBotRadius(),a->GetTopRadius());
                    double rmax=smos_Max(a->GetBotRadius(),a->GetTopRadius());
                    double omin=smos_Min(c.GetBotRadius(),c.GetTopRadius());
                    double omax=smos_Max(c.GetBotRadius(),c.GetTopRadius());
                    bool radial_disjoint=distance-tol_default>rmax+omax || rmax+tol_default<omin-distance || omax+tol_default<rmin-distance;
                    smgu_LineClosestPoint(bottom,axis,other_top,t2);
                    double h=a->GetHeight();
                    bool axial_disjoint=(t>h+tol_default && t2>h+tol_default) || (t<0.0-tol_default && t2<0.0-tol_default);
                    if(!radial_disjoint && !axial_disjoint && distance<=tol_default) return SC_ERR_ARG;
                }
            }
        }
    }
    if(op==0) status=a->IntersectWithCone(ctx,da,*(SmCone*)target,db,edges,has_tolerance?&tol:NULL,NULL,
        more,&curves.xyz,&curves.uv1,&curves.uv2,&curves.types,&curves.deviations);
    else if(op==1) status=a->IntersectWithPlane(ctx,da,*(SmPlane*)target,db,edges,has_tolerance?&tol:NULL,NULL,
        more,&curves.xyz,&curves.uv1,&curves.uv2,&curves.types,&curves.deviations);
    else { status=a->GlobalSurfaceIntersect(ctx,da,*target,db,edges,has_tolerance?&tol:NULL,NULL,
        &curves.xyz,&curves.uv1,&curves.uv2,&curves.types,&curves.deviations); more=FALSE; }
    std::vector<double> result={(double)status,(double)more,(double)curves.xyz.GetSize()};
    for(ULONG i=0;i<curves.xyz.GetSize();++i) put_curve(result,curves.xyz[i]);
    for(auto* uv : {&curves.uv1,&curves.uv2}) {
        result.push_back(uv->GetSize());
        for(ULONG i=0;i<uv->GetSize();++i) put_curve(result,(*uv)[i]);
    }
    result.push_back(curves.types.GetSize());
    for(ULONG i=0;i<curves.types.GetSize();++i) result.push_back(curves.types[i]);
    result.push_back(curves.deviations.GetSize());
    for(ULONG i=0;i<curves.deviations.GetSize();++i) result.push_back(curves.deviations[i]);
    return sc_copy_out(result.data(),(int64_t)result.size(),out,cap,len);
    } catch (const MissingPolarCurve&) {
        return SC_ERR_ARG;
    } catch (...) {
        return SC_ERR_EXCEPTION;
    }
}
