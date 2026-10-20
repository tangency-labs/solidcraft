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
// UB-154 (plan/upstream-issues.md): the file-static sm_GetEllipseTrimIntervals
// (SmCone.cpp:5282) ignores the statuses of the smgu_LinePlaneIntersect calls on the
// trimmed cone's two seam lines and then compares and uses the uninitialised dParam[0..1]
// (:5399-5405, :5463-5477). It is reached only from inside IntersectWithCone and
// IntersectWithPlane, with an ellipse they compute, so the shim replays the same calls in
// the same order up to that point (seam_lines_defined: the head of
// sm_GetEllipseTrimIntervals; the *_ellipse_seams_undefined functions: the paths of the two
// callers that reach it). Status-checked steps that make upstream return before the call
// return false here too.
struct PeekCone : SmCone {
    static double SmCone::*semi() { return &PeekCone::m_dSemiAngleDeg; }
};

// The head of sm_GetEllipseTrimIntervals up to its two smgu_LinePlaneIntersect calls
// (statuses of the domain conversion and the corner evaluations ignored, as upstream):
// false when either call fails.
bool seam_lines_defined(const SmCone& crCone, const SmExtent2d& crUVDomain,
                        const SmPoint3d& rEllipseOrigin, const SmVector3d& rEllipseXAxis,
                        const SmVector3d& rEllipseYAxis)
{
    SmVector3d sEllipseZ = rEllipseXAxis * rEllipseYAxis;
    SmExtent2d sAnalUVDomain;
    crCone.ConvertDomainFromNURBSToSTEP(crUVDomain, sAnalUVDomain);
    SmPoint3d sCorners[4];
    crCone.EvaluateSTEPPoint(sAnalUVDomain.Evaluate(0.0, 0.0), sCorners[0]);
    crCone.EvaluateSTEPPoint(sAnalUVDomain.Evaluate(0.0, 1.0), sCorners[1]);
    crCone.EvaluateSTEPPoint(sAnalUVDomain.Evaluate(1.0, 0.0), sCorners[2]);
    crCone.EvaluateSTEPPoint(sAnalUVDomain.Evaluate(1.0, 1.0), sCorners[3]);
    SmVector3d sStartLineVec = sCorners[1] - sCorners[0];
    SmVector3d sEndLineVec = sCorners[3] - sCorners[2];
    double dParam[2];
    return smgu_LinePlaneIntersect(sCorners[0], sStartLineVec, rEllipseOrigin, sEllipseZ,
                                   dParam[0]) == SM_SUCCESS &&
           smgu_LinePlaneIntersect(sCorners[2], sEndLineVec, rEllipseOrigin, sEllipseZ,
                                   dParam[1]) == SM_SUCCESS;
}

// SmCone::IntersectWithCone (SmCone.cpp:2604) up to its elliptical branch for two
// cylinders of the same radius whose axes intersect (:3010-3137): four
// sm_AddEllipseIntervalsTrimmedToTwoCones calls, each trimming one ellipse to this cone
// and then to the other one. True when one of those eight trims reads dParam.
bool cone_ellipse_seams_undefined(const SmCone& crThis, const SmExtent2d& crUVDomain,
                                  const SmCone& crOtherCone, const SmExtent2d& crOtherUVDomain,
                                  const SmApproxTol3d* pdOptApproxTol3d)
{
    if (&crThis == &crOtherCone)
        return false;
    SmExtent3d sBBox1, sBBox2;
    if (crThis.CalculateBoundingBox(crUVDomain, &sBBox1) != SM_SUCCESS ||
        crOtherCone.CalculateBoundingBox(crOtherUVDomain, &sBBox2) != SM_SUCCESS)
        return false;
    SmVector3d sSize = sBBox2.GetSize();
    double dTol = pdOptApproxTol3d ? (double)*pdOptApproxTol3d
                                   : SM_EFF_ZERO_SQRT * (1.0 + sSize.Length());
    sBBox2.ExpandAbsolute(dTol);
    if (sBBox1.AreDisjoint(sBBox2))
        return false;
    SmVector3d sConeAxis = crThis.GetPosition().GetZAxis();
    const SmPoint3d& rBasePt = crThis.GetPosition().GetOriginRef();
    SmExtent2d sAnal = crThis.GetSTEPUVDomain();
    SmPoint3d sCenterBot = rBasePt + sAnal.GetVMin() * sConeAxis;
    SmPoint3d sCenterTop = rBasePt + sAnal.GetVMax() * sConeAxis;
    SmVector3d sOConeAxis = crOtherCone.GetPosition().GetZAxis();
    const SmPoint3d& rOBasePt = crOtherCone.GetPosition().GetOriginRef();
    SmExtent2d sOAnal = crOtherCone.GetSTEPUVDomain();
    SmPoint3d sOCenterBot = rOBasePt + sOAnal.GetVMin() * sOConeAxis;
    SmPoint3d sOCenterTop = rOBasePt + sOAnal.GetVMax() * sOConeAxis;
    double dAngle;
    if (sConeAxis.AngleBetween(sOConeAxis, dAngle) != SM_SUCCESS)
        return false;
    if (dAngle > SM_PI / 2.0)
        dAngle = SM_PI - dAngle;
    if (dAngle < SM_EFF_ZERO_SQRT) // parallel axes branch: no ellipse
        return false;
    if (!crThis.IsCylinder() || !crOtherCone.IsCylinder() ||
        smos_Fabs(crThis.GetTopRadius() - crOtherCone.GetTopRadius()) > dTol)
        return false;
    ULONG lNumInt;
    SmPoint3d aPoints[2];
    if (smgu_SegmentSegmentIntersect(sCenterBot, sCenterTop, sOCenterBot, sOCenterTop, dTol,
                                     lNumInt, aPoints) != SM_SUCCESS ||
        lNumInt != 1)
        return false;
    SmVector3d sVecToInt1, sVecToInt2;
    if (smgu_ParallelogramCrossVectors(sConeAxis, sOConeAxis, crThis.GetTopRadius(),
                                       crOtherCone.GetTopRadius(), sVecToInt1,
                                       sVecToInt2) != SM_SUCCESS)
        return false;
    SmVector3d sXAxis = sConeAxis * sOConeAxis;
    if (sXAxis.Unitize() != SM_SUCCESS)
        return false;
    SmVector3d sYAxis = sVecToInt1;
    if (sYAxis.Unitize() != SM_SUCCESS)
        return false;
    SmAxis2Placement sFrame;
    auto trims_undefined = [&]() {
        return !seam_lines_defined(crThis, crUVDomain, sFrame.GetOriginRef(),
                                   sFrame.GetXAxisRef(), sFrame.GetYAxisRef()) ||
               !seam_lines_defined(crOtherCone, crOtherUVDomain, sFrame.GetOriginRef(),
                                   sFrame.GetXAxisRef(), sFrame.GetYAxisRef());
    };
    if (sFrame.SetCanonical(aPoints[0], sXAxis, sYAxis) != SM_SUCCESS)
        return false;
    if (trims_undefined())
        return true;
    if (sFrame.SetCanonical(aPoints[0], sXAxis, -sYAxis) != SM_SUCCESS)
        return false;
    if (trims_undefined())
        return true;
    sYAxis = sVecToInt2;
    sYAxis.Unitize(); // status ignored upstream
    if (sFrame.SetCanonical(aPoints[0], sXAxis, sYAxis) != SM_SUCCESS)
        return false;
    if (trims_undefined())
        return true;
    if (sFrame.SetCanonical(aPoints[0], sXAxis, -sYAxis) != SM_SUCCESS)
        return false;
    return trims_undefined();
}

// SmCone::IntersectWithPlane (SmCone.cpp:3219) up to its elliptical branch (:3664-3834),
// which trims the ellipse to the cone's natural domain. True when that trim reads dParam.
// No known input reaches it: the generator lines tested at :3716 and :3735 are as long
// as the seams and one of them is the least steep to the plane, so (up to rounding) they
// fail first and return. Kept so the guard follows every path into
// sm_GetEllipseTrimIntervals.
bool plane_ellipse_seams_undefined(const SmCone& crThis, const SmExtent2d& crUVDomain,
                                   const SmPlane& crPlane, const SmExtent2d& crOtherUVDomain,
                                   const SmApproxTol3d* pdOptApproxTol3d)
{
    double dTol = pdOptApproxTol3d ? (double)*pdOptApproxTol3d : SM_EFF_ZERO_SQRT;
    SmExtent3d sBBox1, sBBox2;
    if (crThis.CalculateBoundingBox(crUVDomain, &sBBox1) != SM_SUCCESS ||
        crPlane.CalculateBoundingBox(crOtherUVDomain, &sBBox2) != SM_SUCCESS)
        return false;
    sBBox2.ExpandAbsolute(dTol);
    if (sBBox1.AreDisjoint(sBBox2))
        return false;
    SmVector3d sConeAxis = crThis.GetPosition().GetZAxis();
    SmExtent2d sAnal = crThis.GetSTEPUVDomain();
    SmPoint3d rCenterBot = crThis.GetPosition().GetOriginRef() + sAnal.GetVMin() * sConeAxis;
    SmPoint3d sCenterTop = crThis.GetPosition().GetOriginRef() + sAnal.GetVMax() * sConeAxis;
    SmVector3d sPlaneNormal = crPlane.GetPosition().GetZAxis();
    const SmPoint3d& rPlaneOrigin = crPlane.GetPosition().GetOriginRef();
    double dAngTol = SM_EFF_ZERO * 100.0 * 360.0;
    double dConeAxisPlaneNormalAngle;
    if (sConeAxis.AngleBetween(sPlaneNormal, dConeAxisPlaneNormalAngle) != SM_SUCCESS)
        return false;
    if (dConeAxisPlaneNormalAngle > SM_PI / 2.0)
    {
        dConeAxisPlaneNormalAngle = SM_PI - dConeAxisPlaneNormalAngle;
        sPlaneNormal = -sPlaneNormal;
    }
    double dBotRadius = crThis.GetBotRadius();
    if (SM_PI / 2.0 - dConeAxisPlaneNormalAngle < dAngTol) // plane parallel to the axis
        return false;
    if (dConeAxisPlaneNormalAngle < dAngTol) // plane perpendicular to the axis
        return false;
    double dAngleCone = smos_Fabs(SM_DEG2RAD(crThis.*PeekCone::semi()));
    double dAnglePlane = SM_PI / 2.0 - smos_Fabs(dConeAxisPlaneNormalAngle);
    if (!crThis.IsCylinder() && dAnglePlane < dAngleCone + dAngTol)
        return false;
    SmVector3d sVec = sConeAxis * (sConeAxis * sPlaneNormal);
    sVec.Unitize();
    SmPoint3d sPt1 = rCenterBot - dBotRadius * sVec;
    SmPoint3d sPt2 = sCenterTop - crThis.GetTopRadius() * sVec;
    SmVector3d sLineVec = sPt2 - sPt1;
    double sLineLength1 = sLineVec.Length();
    double dTParam1, dTParam2;
    if (smgu_LinePlaneIntersect(sPt1, sLineVec, rPlaneOrigin, sPlaneNormal, dTParam1) !=
        SM_SUCCESS)
        return false;
    SmPoint3d sEnd1 = sPt1 + dTParam1 * sLineVec;
    sPt1 = rCenterBot + dBotRadius * sVec;
    sPt2 = sCenterTop + crThis.GetTopRadius() * sVec;
    sLineVec = sPt2 - sPt1;
    double sLineLength2 = sLineVec.Length();
    if (smgu_LinePlaneIntersect(sPt1, sLineVec, rPlaneOrigin, sPlaneNormal, dTParam2) !=
        SM_SUCCESS)
        return false;
    if ((dTParam1 < 0.0 - sLineLength1 && dTParam2 < 0.0 - sLineLength2) ||
        (dTParam1 > 1.0 + sLineLength1 && dTParam2 > 1.0 + sLineLength2))
        return false;
    SmPoint3d sEnd2 = sPt1 + dTParam2 * sLineVec;
    SmVector3d sXAxis = (sEnd2 - sEnd1) / 2.0;
    SmPoint3d sCenter = sEnd1 + sXAxis;
    double dMajRad = sXAxis.Length();
    if (SM_IS_ZERO(dMajRad)) // degenerate point output
        return false;
    sXAxis.Unitize();
    SmVector3d sYAxis = sPlaneNormal * sXAxis;
    sYAxis.Unitize();
    return !seam_lines_defined(crThis, crThis.GetNaturalUVDomain(), sCenter, sXAxis, sYAxis);
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
    // UB-154: refuse exactly the inputs on which a seam-line intersection inside
    // sm_GetEllipseTrimIntervals fails and upstream reads the uninitialised parameter.
    if((op==0 || (op==3 && target->GetType()==SmCone_TYPE))
       && cone_ellipse_seams_undefined(*a,da,*(SmCone*)target,db,has_tolerance?&tol:NULL))
        return SC_ERR_ARG;
    if((op==1 || op==2)
       && plane_ellipse_seams_undefined(*a,da,*(SmPlane*)target,db,has_tolerance?&tol:NULL))
        return SC_ERR_ARG;
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
