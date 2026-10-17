// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surface_cache_p1`: the SmSurfaceCache members owned by the unit
// (source/SMLib/src/SmSurfaceCache.cpp, source/SMLib/inc/SmSurfaceCache.h), called on
// SmSurfaceCache / SmPatchBoundaryBoundingPlanes / SmAltSrfCache objects built over an
// SmBSplineSurface, plus the file-static helpers of SmSurfaceCache.cpp.
//
// Static functions cannot be linked against and the file cannot be recompiled inside a
// namespace (it defines SmSurfaceCache members), so the helpers used here are VERBATIM copies of
// upstream SmSurfaceCache.cpp (commit c9c5979), inside the namespace sc_ssc_p1 (line ranges noted
// at each block; generated, do not edit by hand). Protected members are reached through derived
// "peek" classes; decomposition trees are built by hand in the tree's own node manager (the tree
// construction belongs to unit surface.surface_cache_p3).
//
// One entry point, sc_surface_surface_cache_p1_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/surface_surface_cache_p1.rs, which validates every
// input. Every op but `plane3` and `cull` starts with a B-spline surface block `oob form n m p q r
// s cpts(x y z w) u v`. `out` receives the op status (0 = SM_SUCCESS, else the SmStatus value) and
// the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPolarBox.h>
#include <SmSurface.h>
#include <SmSurfaceCache.h>
#include <SmTArray.h>
#include <SmTol.h>
#include <SmTree.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

#include "sc_sm.h"

using sc::flg;
using sc::put_dom;
using sc::put_vec;
using sc::st;

namespace sc_ssc_p1
{
// clang-format off
// ---- verbatim: SmSurfaceCache.cpp lines 192-252: sm_CheckBoundingPlaneSeparatingHalfspace
static SmStatus sm_CheckBoundingPlaneSeparatingHalfspace
 (SmBSplineSurface * pBSS, 
  const SmPoint3d  & rsPlaneBasePoint,
  const SmVector3d & rsPlaneNormal,
  SmBoolean        & rbIsSeparatingHalfspace,
  SmBoolean        & rbInvertNormal)
{
    SmTArray<SmPoint3d> sControlPoints;
    SmBSplineSurfaceForm eSurfaceForm;
    SmKnotType eKnotType;
    SmTArray<double> sWeights;
    ULONG nUDegree;
    ULONG nVDegree;
    SmTArray<ULONG> sUKnotMult, sVKnotMult;
    SmTArray<double> sUKnots, sVKnots;

    ULONG nControlPoints = pBSS->GetNumberControlPoints(SM_SP_U) * 
                           pBSS->GetNumberControlPoints(SM_SP_V);

    SER(pBSS->GetCanonical(nUDegree,nVDegree,sControlPoints,
        eSurfaceForm,sUKnotMult,sVKnotMult,sUKnots,sVKnots,
        eKnotType,sWeights));

    if (nControlPoints != sControlPoints.GetSize())  { SER(SM_ERR); }

    int lSign = 0;

    for(ULONG ii=0; ii<nControlPoints; ii++) {
        const SmPoint3d& rsCP = sControlPoints[ii];

        double dSignedDist = rsPlaneNormal.Dot( rsCP - rsPlaneBasePoint );

        if (smos_Fabs(dSignedDist) < SM_EFF_ZERO_SQRT )
            continue;

        // If there is a sign difference: the plane is not a 
        // separating halfspace
        if (   (dSignedDist > 0.0 && lSign == -1) 
            || (dSignedDist < 0.0 && lSign ==  1) )  {
            rbIsSeparatingHalfspace = FALSE;
            return SM_SUCCESS;
        }

        if (dSignedDist > 0.0)
           lSign = 1;
        else 
           lSign = -1;
    }
    
    // All points are expected to be in the 'material side' of the plane
    // (the normal pointing out of it):
    rbInvertNormal = FALSE;
    if (lSign == 1)
        rbInvertNormal = TRUE;

    rbIsSeparatingHalfspace = TRUE;
    return SM_SUCCESS;


} // end sm_CheckBoundingPlaneSeparatingHalfspace

// ---- verbatim: SmSurfaceCache.cpp lines 260-323: sm_ComputePlaneFrom3Points
static SmStatus sm_ComputePlaneFrom3Points
  (const SmPoint3d& sP1Arg, 
   const SmPoint3d& sP2Arg, 
   const SmPoint3d& sP3Arg, 
   SmPoint3d& rsPlaneBaseArg, 
   SmVector3d& rsPlaneNormalArg,
   double dTolArg,
   SmBoolean& rbPlaneExistsArg,
   SmBoolean& rb3PointsCollinearArg)
{
    rbPlaneExistsArg = FALSE;
    rb3PointsCollinearArg = FALSE;

    // Avoiding insanity: 
    SmVector3d sP12V = sP1Arg - sP2Arg;
    SmVector3d sP23V = sP2Arg - sP3Arg;
    SmVector3d sP31V = sP3Arg - sP1Arg;
   
    if ( sP12V.Length() < dTolArg ||
         sP23V.Length() < dTolArg ||
         sP31V.Length() < dTolArg ) {
        return SM_SUCCESS;
    }

    sP12V.Unitize();
    sP23V.Unitize();
    sP31V.Unitize();

    double dAngleTol = 0.01;

    SmVector3d sP123( sP12V * sP23V);         
    SmVector3d sP231( sP23V * sP31V);         
    SmVector3d sP312( sP31V * sP12V);         
    
    double dSin12 = sP123.Length();
    double dSin23 = sP231.Length();
    double dSin31 = sP312.Length();
  
    if(dSin12 < dAngleTol || dSin23 < dAngleTol || dSin31 < dAngleTol)
    {

      if(dSin12 < SM_EFF_ZERO_SQRT &&
         dSin23 < SM_EFF_ZERO_SQRT &&
         dSin31 < SM_EFF_ZERO_SQRT)
      {
        rb3PointsCollinearArg = TRUE;
      }
      return SM_SUCCESS;
    }

    // Check for safety (should be OK):
    if (sP123.Dot(sP231) < 0.0 ||
        sP231.Dot(sP312) < 0.0 ||
        sP312.Dot(sP123) < 0.) { SER(SM_ERR); }

    // If made it here, there exists a plane:
    rbPlaneExistsArg = TRUE;
    rsPlaneBaseArg =   (sP1Arg  + sP2Arg  + sP3Arg )  / 3.0;
    rsPlaneNormalArg = (sP123   + sP231   + sP312 )   / 3.0;
    rsPlaneNormalArg.Unitize();
    return SM_SUCCESS;

} // end sm_ComputePlaneFrom3Points

// ---- verbatim: SmSurfaceCache.cpp lines 679-912: sm_ComputeBezierApproximation
static SmStatus sm_ComputeBezierApproximation
  (const SmSurface  & crSurface,      // in :      
   const SmExtent2d & crUVDomain,     // in :      
   gw_SURFACE * pSurface)             // out: preallocated surface whose values get set
{
    SmVector3d sMat[2][2];
    gw_CPOINT **Pw  = pSurface->net->Pw;
    gw_KNOTVECTOR *pKNOTU = pSurface->knu;
    gw_KNOTVECTOR *pKNOTV = pSurface->knv;

    // Load knot vectors with domain bounds
    pKNOTU->m = 7;
    pKNOTV->m = 7;
    for (ULONG i=0; i<4; i++) {
        pKNOTU->U[i] = crUVDomain.GetMin().x;
        pKNOTV->U[i] = crUVDomain.GetMin().y;
        pKNOTU->U[i+4] = crUVDomain.GetMax().x;
        pKNOTV->U[i+4] = crUVDomain.GetMax().y;
    }

    SmPoint2d sSize = crUVDomain.GetSize();
    SmSurfParamType eSingDir;
    SmVector3d sDU, sDV, sDUV;

    // Now load control polygon at corners using hermite information.
    {
        SER(crSurface.Evaluate(crUVDomain.GetMin(),1,1,TRUE,TRUE,FALSE,sMat[0]));
        SmPoint3d sP00 = sMat[0][0];
        if (crSurface.IsSingularity(crUVDomain.GetMin(),eSingDir)) {
            SmPoint2d sUV = crUVDomain.GetMin();
            SmPoint2d sUVU(sUV.x+(sSize.x*SM_EFF_ZERO_SQRT),sUV.y);
            SmPoint2d sUVV(sUV.x,sUV.y+(sSize.y*SM_EFF_ZERO_SQRT));
            SER(crSurface.Evaluate(sUVU,1,1,TRUE,TRUE,FALSE,sMat[0]));
            sDU = sMat[1][0]*sSize.x;
            sDUV = sMat[1][1]*sSize.x*sSize.y;
            SER(crSurface.Evaluate(sUVV,1,1,TRUE,TRUE,FALSE,sMat[0]));
            sDV = sMat[0][1]*sSize.y;
            sDUV = (sDUV + sMat[1][1]*sSize.x*sSize.y)/2.0;
        }
        else {
            sDU = sMat[1][0]*sSize.x;
            sDV = sMat[0][1]*sSize.y;
            sDUV = sMat[1][1]*sSize.x*sSize.y;
        }
        COPY_XYZ(sP00,Pw[0][0]);
        SmPoint3d sP10 = sP00 + sDU/3.0;
        COPY_XYZ(sP10,Pw[1][0]);
        SmPoint3d sP01 = sP00 + sDV/3.0;
        COPY_XYZ(sP01,Pw[0][1]);
        SmPoint3d sP11 = sP10 + sP01 - sP00 + sDUV/9.0;
        COPY_XYZ(sP11,Pw[1][1]);
#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
        if (bDebugMe) {
            smgfx_Erase();
            smgfx_SetColor(1,0,0);
            sP00.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,0,1);
            sP10.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,1,1);
            sP01.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,0,0);
            sP11.Draw();
            sm_GraphicsLoop();
        }
#endif

    }

    {
        SER(crSurface.Evaluate(crUVDomain.GetMax(),1,1,TRUE,TRUE,FALSE,sMat[0]));
        SmPoint3d sP00 = sMat[0][0];
        if (crSurface.IsSingularity(crUVDomain.GetMax(),eSingDir)) {
            SmPoint2d sUV = crUVDomain.GetMax();
            SmPoint2d sUVU(sUV.x-(sSize.x*SM_EFF_ZERO_SQRT),sUV.y);
            SmPoint2d sUVV(sUV.x,sUV.y-(sSize.y*SM_EFF_ZERO_SQRT));
            SER(crSurface.Evaluate(sUVU,1,1,TRUE,TRUE,FALSE,sMat[0]));
            sDU = sMat[1][0]*sSize.x;
            sDUV = sMat[1][1]*sSize.x*sSize.y;
            SER(crSurface.Evaluate(sUVV,1,1,TRUE,TRUE,FALSE,sMat[0]));
            sDV = sMat[0][1]*sSize.y;
            sDUV = (sDUV + sMat[1][1]*sSize.x*sSize.y)/2.0;
        }
        else {
            sDU = sMat[1][0]*sSize.x;
            sDV = sMat[0][1]*sSize.y;
            sDUV = sMat[1][1]*sSize.x*sSize.y;
        }
        COPY_XYZ(sP00,Pw[3][3]);
        SmPoint3d sP10 = sP00 - sDU/3.0;
        COPY_XYZ(sP10,Pw[2][3]);
        SmPoint3d sP01 = sP00 - sDV/3.0;
        COPY_XYZ(sP01,Pw[3][2]);
        SmPoint3d sP11 = sP10 + sP01 - sP00 + sDUV/9.0;
        COPY_XYZ(sP11,Pw[2][2]);
#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
        if (bDebugMe) {
            smgfx_Erase();
            smgfx_SetColor(1,0,0);
            sP00.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,0,1);
            sP10.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,1,1);
            sP01.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,0,0);
            sP11.Draw();
            sm_GraphicsLoop();
        }
#endif
    }

    {
        SER(crSurface.Evaluate(crUVDomain.Evaluate(1.0,0.0),1,1,TRUE,TRUE,FALSE,sMat[0]));
        SmPoint3d sP00 = sMat[0][0];
        if (crSurface.IsSingularity(crUVDomain.Evaluate(1.0,0.0),eSingDir)) {
            SmPoint2d sUV = crUVDomain.Evaluate(1.0,0.0);
            SmPoint2d sUVU(sUV.x-(sSize.x*SM_EFF_ZERO_SQRT),sUV.y);
            SmPoint2d sUVV(sUV.x,sUV.y+(sSize.y*SM_EFF_ZERO_SQRT));
            SER(crSurface.Evaluate(sUVU,1,1,TRUE,TRUE,FALSE,sMat[0]));
            sDU = sMat[1][0]*sSize.x;
            sDUV = sMat[1][1]*sSize.x*sSize.y;
            SER(crSurface.Evaluate(sUVV,1,1,TRUE,TRUE,FALSE,sMat[0]));
            sDV = sMat[0][1]*sSize.y;
            sDUV = (sDUV + sMat[1][1]*sSize.x*sSize.y)/2.0;
        }
        else {
            sDU = sMat[1][0]*sSize.x;
            sDV = sMat[0][1]*sSize.y;
            sDUV = sMat[1][1]*sSize.x*sSize.y;
        }
        COPY_XYZ(sP00,Pw[3][0]);
        SmPoint3d sP10 = sP00 - sDU/3.0;
        COPY_XYZ(sP10,Pw[2][0]);
        SmPoint3d sP01 = sP00 + sDV/3.0;
        COPY_XYZ(sP01,Pw[3][1]);
        SmPoint3d sP11 = sP10 + sP01 - sP00 - sDUV/9.0; 
        COPY_XYZ(sP11,Pw[2][1]);
#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
        if (bDebugMe) {
            smgfx_Erase();
            smgfx_SetColor(1,0,0);
            sP00.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,0,1);
            sP10.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,1,1);
            sP01.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,0,0);
            sP11.Draw();
            sm_GraphicsLoop();
        }
#endif
    }

    {
        SER(crSurface.Evaluate(crUVDomain.Evaluate(0.0,1.0),1,1,TRUE,TRUE,FALSE,sMat[0]));
        SmPoint3d sP00 = sMat[0][0];
        if (crSurface.IsSingularity(crUVDomain.Evaluate(0.0,1.0),eSingDir)) {
            SmPoint2d sUV = crUVDomain.Evaluate(0.0,1.0);
            SmPoint2d sUVU(sUV.x+(sSize.x*SM_EFF_ZERO_SQRT),sUV.y);
            SmPoint2d sUVV(sUV.x,sUV.y-(sSize.y*SM_EFF_ZERO_SQRT));
            SER(crSurface.Evaluate(sUVU,1,1,TRUE,TRUE,FALSE,sMat[0]));
            sDU = sMat[1][0]*sSize.x;
            sDUV = sMat[1][1]*sSize.x*sSize.y;
            SER(crSurface.Evaluate(sUVV,1,1,TRUE,TRUE,FALSE,sMat[0]));
            sDV = sMat[0][1]*sSize.y;
            sDUV = (sDUV + sMat[1][1]*sSize.x*sSize.y)/2.0;
        }
        else {
            sDU = sMat[1][0]*sSize.x;
            sDV = sMat[0][1]*sSize.y;
            sDUV = sMat[1][1]*sSize.x*sSize.y;
        }
        COPY_XYZ(sP00,Pw[0][3]);
        SmPoint3d sP10 = sP00 + sDU/3.0;
        COPY_XYZ(sP10,Pw[1][3]);
        SmPoint3d sP01 = sP00 - sDV/3.0;
        COPY_XYZ(sP01,Pw[0][2]);
        SmPoint3d sP11 = sP10 + sP01 - sP00 - sDUV/9.0; 
        COPY_XYZ(sP11,Pw[1][2]);
#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
        if (bDebugMe) {
            smgfx_Erase();
            smgfx_SetColor(1,0,0);
            sP00.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,0,1);
            sP10.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,1,1);
            sP01.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,0,0);
            sP11.Draw();
            sm_GraphicsLoop();
        }
#endif
    }

    for (ULONG j=0; j<4; j++) {
        for (ULONG k=0; k<4; k++) {
            Pw[j][k].w = NL_NOW;
        }
    }

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
    if (bDebugMe) {
        sm_GraphicsLoop();
        smgfx_SetColor(1,0,0);
        SmBSplineSurface sBSS(pSurface,TRUE,crSurface.GetContext()) ;
        sBSS.SetContext(crSurface.GetContext());
        sBSS.Dump();
        sBSS.DrawNet();
        sBSS.DrawUV(5,5);
        sm_GraphicsLoop();
    }
#endif

    return SM_SUCCESS;

} // end sm_ComputeBezierApproximation

// ---- verbatim: SmSurfaceCache.cpp lines 3996-4012: sm_CullArray
static SmStatus sm_CullArray( SmTArray<double> & sArr )
{
  ULONG ii, jj, lSize = sArr.GetSize();
  for ( ii = 2, jj = 1; ii < lSize; ii += 2, jj += 1 )
  {
    sArr[jj] = sArr[ii];
  }
  // Make sure the last element is retained.
  if ( ii == lSize )
    { sArr[jj] = sArr[ lSize-1 ]; jj++; }

  sArr.SetSize( jj );

  return SM_SUCCESS;

} // end sm_CullArray

// clang-format on
} // namespace sc_ssc_p1

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_surface_cache_p1.rs).
enum : int32_t
{
    OP_PLANE3 = 0,
    OP_HALFSPACE = 1,
    OP_BPLANES = 2,
    OP_BEZAPPROX = 3,
    OP_SIZE_CORNERS = 4,
    OP_SIZE_SURF = 5,
    OP_CORNERS = 6,
    OP_ALT = 7,
    OP_TREETOP = 8,
    OP_SUBDIV = 9,
    OP_CONT = 10,
    OP_ISO = 11,
    OP_POLAR = 12,
    OP_MISC = 13,
    OP_FACE = 14,
    OP_CULL = 15,
};

const int64_t kMaxObjects = 4096;
alignas(16) char g_objs[kMaxObjects];

SmObject* obj_ptr(int64_t i)
{
    return i < 0 ? nullptr : reinterpret_cast<SmObject*>(g_objs + i);
}

int64_t obj_id(const void* p)
{
    return p == nullptr ? -1 : (int64_t)(static_cast<const char*>(p) - g_objs);
}

struct TreePeek : SmTree
{
    explicit TreePeek(SmObject* owner) : SmTree(owner) {}
    SmMemBlockMgr& node_mgr() { return m_sNodeMgr; }
    SmTreeNode*& top() { return m_pTopNode; }
};

struct CachePeek : SmSurfaceCache
{
    CachePeek(const SmSurface& s, const SmExtent2d& d) : SmSurfaceCache(s, d) {}
    SmTree*& tree() { return m_pTree; }
    SmMemBlockMgr& bez() { return m_sBezMgr; }
    SmMemBlockMgr& aux() { return m_sAuxMgr; }
    SmTArray<SmContinuityType>& uconts() { return *m_pUContinuities; }
    SmTArray<SmContinuityType>& vconts() { return *m_pVContinuities; }
    ULONG& nalt() { return m_lNumAltCaches; }
    SmAltSrfCache** alts() { return m_apAltSrfCaches; }
    double iso_tol() const { return m_dIsoCurveTolerance; }
};

struct BPPeek : SmPatchBoundaryBoundingPlanes
{
    BPPeek(const SmSurface* s, const SmExtent2d& d) : SmPatchBoundaryBoundingPlanes(s, d) {}
    SmStatus compute() { return ComputeBoundingPlanes(); }
    bool planar(int i) const { return m_bBoundaryCurvePlanar[i] == TRUE; }
    bool degenerate(int i) const { return m_bBoundaryCurveDegenerate[i] == TRUE; }
    const SmPoint3d& base(int i) const { return m_vBasePoint[i]; }
    const SmPoint3d& normal(int i) const { return m_vNormalVector[i]; }
    const SmPoint3d& sample(int i, int j) const { return m_vSamplePoint[i][j]; }
    double tol(int i) const { return m_dPlaneTolerance[i]; }
};

struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    bool flag() { return in[at++] != 0.0; }
    SmPoint2d pt2()
    {
        double x = num();
        double y = num();
        return SmPoint2d(x, y);
    }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
    SmExtent2d domain()
    {
        double a = num();
        double b = num();
        double c = num();
        double d = num();
        return SmExtent2d(a, b, c, d);
    }
};

void put2(std::vector<double>& o, const SmVector2d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
}

void put_box(std::vector<double>& o, const SmExtent3d& e)
{
    put_vec(o, e.GetMin());
    put_vec(o, e.GetMax());
}

gw_SURFACE* read_nurb(Rd& rd)
{
    gw_INDEX n = (gw_INDEX)rd.idx();
    gw_INDEX m = (gw_INDEX)rd.idx();
    gw_DEGREE p = (gw_DEGREE)rd.idx();
    gw_DEGREE q = (gw_DEGREE)rd.idx();
    gw_INDEX r = (gw_INDEX)rd.idx();
    gw_INDEX s = (gw_INDEX)rd.idx();
    gw_SURFACE* sur = sm_AllocateNurbSurface(n, m, p, q, r, s);
    for (gw_INDEX i = 0; i <= n; i++)
        for (gw_INDEX j = 0; j <= m; j++)
        {
            sur->net->Pw[i][j].x = rd.num();
            sur->net->Pw[i][j].y = rd.num();
            sur->net->Pw[i][j].z = rd.num();
            sur->net->Pw[i][j].w = rd.num();
        }
    for (gw_INDEX i = 0; i <= r; i++)
        sur->knu->U[i] = rd.num();
    for (gw_INDEX i = 0; i <= s; i++)
        sur->knv->U[i] = rd.num();
    return sur;
}

SmBSplineSurface* read_bspline(Rd& rd, const SmContext& ctx)
{
    bool oob = rd.flag();
    SmBSplineSurfaceForm form = (SmBSplineSurfaceForm)rd.idx();
    SmBSplineSurface* s = new (ctx) SmBSplineSurface(read_nurb(rd), FALSE, NULL);
    s->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
    s->SetBSplineSurfaceForm(form);
    return s;
}

// Reads `dim n p m cpts knots` into SmBSplineCurve(gw_CURVE*, dim, FALSE).
SmBSplineCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    ULONG dim = (ULONG)rd.idx();
    gw_INDEX n = (gw_INDEX)rd.idx();
    gw_DEGREE p = (gw_DEGREE)rd.idx();
    gw_INDEX m = (gw_INDEX)rd.idx();
    gw_CURVE* cur = sm_AllocateNurbCurve(n, p, m);
    for (gw_INDEX i = 0; i <= n; i++)
    {
        cur->pol->Pw[i].x = rd.num();
        cur->pol->Pw[i].y = rd.num();
        cur->pol->Pw[i].z = rd.num();
        cur->pol->Pw[i].w = rd.num();
    }
    for (gw_INDEX i = 0; i <= m; i++)
        cur->knt->U[i] = rd.num();
    return new (ctx) SmBSplineCurve(cur, dim, FALSE);
}

// `dim has [n p m cpts knots]`.
void put_curve(std::vector<double>& o, const SmBSplineCurve* c)
{
    o.push_back((double)c->GetDim());
    const gw_CURVE* cur = c->GetGwNurbPointer();
    o.push_back(flg(cur != NULL));
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

void put_polar(std::vector<double>& o, const SmPolarBox& b)
{
    o.push_back((double)b.GetPolarBoxState());
    if (b.GetPolarBoxState() == SM_PS_UNINITIALIZED)
        return;
    for (ULONG k = 0; k < 3; ++k)
        put_vec(o, *b.GetBasis(k));
    put_dom(o, b.GetDomain());
}

// Alternate cache user data: `hv V hb b hl l hd d hp p`.
struct Key
{
    bool hv, hb, hl, hd, hp;
    SmVector3d v;
    SmBoolean b;
    long l;
    double d;
    int64_t p;
    void read(Rd& rd)
    {
        hv = rd.flag();
        v = rd.vec();
        hb = rd.flag();
        b = rd.flag() ? TRUE : FALSE;
        hl = rd.flag();
        l = (long)rd.idx();
        hd = rd.flag();
        d = rd.num();
        hp = rd.flag();
        p = rd.idx();
    }
    const SmVector3d* pv() const { return hv ? &v : NULL; }
    const SmBoolean* pb() const { return hb ? &b : NULL; }
    const long* pl() const { return hl ? &l : NULL; }
    const double* pd() const { return hd ? &d : NULL; }
    const void* pp() const { return hp ? (const void*)obj_ptr(p) : NULL; }
};

// `k` then `k` curves into a new array, or `-1` for NULL.
SmTArray<SmCurve*>* read_curves(Rd& rd, const SmContext& ctx)
{
    int64_t k = rd.idx();
    if (k < 0)
        return NULL;
    SmTArray<SmCurve*>* a = new (ctx) SmTArray<SmCurve*>(ctx);
    for (int64_t i = 0; i < k; i++)
        a->Add(read_curve(rd, ctx));
    return a;
}

void put_curve_array(std::vector<double>& o, const SmTArray<SmCurve*>& a)
{
    o.push_back((double)a.GetSize());
    for (ULONG i = 0; i < a.GetSize(); i++)
        put_curve(o, SM_CAST_PTR(SmBSplineCurve, a[i]));
}

// Subdivision tree nodes `n` x `parent c1 c2 aux split marked class face D`, allocated in
// order from the tree's node manager; patches (aux 3) from m_sBezMgr, the others from
// m_sAuxMgr; every node's mBA_pSurface is the cache surface.
std::vector<SmTreeNode*> read_tree(Rd& rd, CachePeek* cache, const SmSurface* s, const SmContext& ctx)
{
    int64_t n = rd.idx();
    TreePeek* tree = new (ctx) TreePeek(cache);
    tree->node_mgr().Initialize(ALIGN_SIZE(sizeof(SmTreeNode)), 25);
    cache->bez().Initialize(ALIGN_SIZE(sizeof(SmBezierPatch)), 25);
    cache->aux().Initialize(ALIGN_SIZE(sizeof(SmBezierAux2d)), 25);
    cache->tree() = tree;
    std::vector<SmTreeNode*> nodes;
    for (int64_t k = 0; k < n; k++)
        nodes.push_back((SmTreeNode*)tree->node_mgr().GetNewElement());
    auto ptr = [&](int64_t k) -> SmTreeNode* { return k < 0 ? nullptr : nodes[(size_t)k]; };
    for (int64_t k = 0; k < n; k++)
    {
        SmTreeNode* nd = nodes[(size_t)k];
        nd->m_pTree = tree;
        nd->m_pParent = ptr(rd.idx());
        nd->m_pChild1 = ptr(rd.idx());
        nd->m_pChild2 = ptr(rd.idx());
        nd->m_eAuxDataType = (SmAuxDataType)rd.idx();
        nd->m_eGeomType = SM_NG_DEFAULT;
        SmBezierAux2d* a = nd->m_eAuxDataType == SM_AD_BEZIER_SURFACE
                               ? (SmBezierAux2d*)cache->bez().GetNewElement()
                               : (SmBezierAux2d*)cache->aux().GetNewElement();
        a->m_eSplitDir = (SmSurfParamType)rd.idx();
        a->m_bIsMarked = rd.flag() ? TRUE : FALSE;
        a->m_eNodeClass = (SmNodeClassType)rd.idx();
        a->m_pFace = obj_ptr(rd.idx());
        a->m_sUVDomain = rd.domain();
        a->mBA_pSurface = s;
        a->m_pOwningTreeNode = nd;
        nd->m_pData = a;
    }
    tree->top() = n > 0 ? nodes[0] : nullptr;
    return nodes;
}

int64_t index_of(const std::vector<SmTreeNode*>& nodes, const SmTreeNode* p)
{
    if (p == nullptr)
        return -1;
    for (size_t i = 0; i < nodes.size(); i++)
        if (nodes[i] == p)
            return (int64_t)i;
    return -2;
}

void cache_op(int32_t op, Rd& rd, const SmContext& ctx, SmBSplineSurface* s, std::vector<double>& o)
{
    switch (op)
    {
    case OP_HALFSPACE:
    {
        SmVector3d base = rd.vec();
        SmVector3d normal = rd.vec();
        SmBoolean sep = FALSE;
        SmBoolean inv = FALSE;
        SmStatus r = sc_ssc_p1::sm_CheckBoundingPlaneSeparatingHalfspace(s, base, normal, sep, inv);
        o.push_back(st(r));
        o.push_back(flg(sep == TRUE));
        o.push_back(flg(inv == TRUE));
        return;
    }
    case OP_BPLANES:
    {
        SmExtent2d d = rd.domain();
        ULONG idx = (ULONG)rd.idx();
        BPPeek bp(s, d);
        SmBSplineCurve* c = NULL;
        SmStatus r = bp.CreateBoundaryCurveForIndex(idx, c);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            put_curve(o, c);
            delete c;
        }
        for (ULONG i = 0; i < 4; i++)
        {
            SmPoint2d a, b;
            bp.GetBoundaryUVEndPoints(i, a, b);
            put2(o, a);
            put2(o, b);
        }
        r = bp.compute();
        o.push_back(st(r));
        if (r != SM_SUCCESS)
            return;
        o.push_back(flg(bp.ArePlanesComputed() == TRUE));
        for (int i = 0; i < 4; i++)
        {
            o.push_back(flg(bp.degenerate(i)));
            o.push_back(flg(bp.planar(i)));
            if (!bp.planar(i))
                continue;
            put_vec(o, bp.base(i));
            put_vec(o, bp.normal(i));
            o.push_back(bp.tol(i));
            for (int j = 0; j < SM_PB_SAMPLECOUNT; j++)
                put_vec(o, bp.sample(i, j));
        }
        return;
    }
    case OP_BEZAPPROX:
    {
        SmExtent2d d = rd.domain();
        gw_SURFACE* bez = sm_AllocateNurbSurface(3, 3, 3, 3, 7, 7);
        SmStatus r = sc_ssc_p1::sm_ComputeBezierApproximation(*s, d, bez);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            for (int i = 0; i <= 3; i++)
                for (int j = 0; j <= 3; j++)
                {
                    o.push_back(bez->net->Pw[i][j].x);
                    o.push_back(bez->net->Pw[i][j].y);
                    o.push_back(bez->net->Pw[i][j].z);
                    o.push_back(bez->net->Pw[i][j].w);
                }
            for (int i = 0; i <= 7; i++)
                o.push_back(bez->knu->U[i]);
            for (int i = 0; i <= 7; i++)
                o.push_back(bez->knv->U[i]);
        }
        smos_Free(bez);
        return;
    }
    default: break;
    }

    // ops on a cache of the surface over its natural domain (or the given one)
    CachePeek* cache = new (ctx) CachePeek(*s, s->GetNaturalUVDomain());
    switch (op)
    {
    case OP_SIZE_CORNERS:
    {
        SmVector3d a = rd.vec();
        SmVector3d b = rd.vec();
        SmVector3d c = rd.vec();
        SmVector3d d = rd.vec();
        SmVector2d ar, mx, mn;
        SmStatus r = cache->ComputeSizeTolerances(a, b, c, d, ar, mx, mn);
        o.push_back(st(r));
        put2(o, ar);
        put2(o, mx);
        put2(o, mn);
        break;
    }
    case OP_SIZE_SURF:
    {
        SmExtent2d d = rd.domain();
        SmVector2d ar, mx, mn;
        SmStatus r = cache->ComputeSizeTolerances(s, d, ar, mx, mn);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            put2(o, ar);
            put2(o, mx);
            put2(o, mn);
        }
        break;
    }
    case OP_CORNERS:
    {
        SmExtent2d d = rd.domain();
        bool has = rd.flag();
        gw_SURFACE* bez = has ? read_nurb(rd) : NULL;
        SmPoint3d a, b, c, e;
        SmExtent2d dom(0.0, 0.0, 0.0, 0.0);
        SmStatus r = cache->GetCorners(bez, d, a, b, c, e, dom);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            put_vec(o, a);
            put_vec(o, b);
            put_vec(o, c);
            put_vec(o, e);
            put_dom(o, dom);
        }
        if (bez)
            smos_Free(bez);
        break;
    }
    case OP_ALT:
    {
        int64_t nalt = rd.idx();
        for (int64_t i = 0; i < nalt; i++)
        {
            SmAltSrfCacheType t = (SmAltSrfCacheType)rd.idx();
            double ap = rd.num();
            double an = rd.num();
            Key key;
            key.read(rd);
            SmTArray<SmCurve*>* c3 = read_curves(rd, ctx);
            SmTArray<SmCurve*>* cu = read_curves(rd, ctx);
            cache->alts()[i] = new (ctx) SmAltSrfCache(c3, cu, t, ap, an, key.pv(), key.pb(), key.pl(),
                                                       key.pd(), key.pp());
            delete c3;
            delete cu;
        }
        cache->nalt() = (ULONG)nalt;
        o.push_back(0.0);
        int64_t nq = rd.idx();
        for (int64_t q = 0; q < nq; q++)
        {
            SmAltSrfCacheType t = (SmAltSrfCacheType)rd.idx();
            double ap = rd.num();
            double an = rd.num();
            Key key;
            key.read(rd);
            bool w3 = rd.flag();
            bool wu = rd.flag();
            for (int64_t i = 0; i < nalt; i++)
            {
                o.push_back(flg(cache->alts()[i]->MatchCache(t, ap, an, key.pv(), key.pb(), key.pl(), key.pd(),
                                                             key.pp()) == TRUE));
                SmTArray<SmCurve*> a3, au;
                SmStatus r = cache->alts()[i]->GetCopyOfCurves(ctx, w3 ? &a3 : NULL, wu ? &au : NULL);
                o.push_back(st(r));
                o.push_back((double)a3.GetSize());
                o.push_back((double)au.GetSize());
                if (r == SM_SUCCESS)
                {
                    SmObjsDelete<SmCurve*> c1(&a3);
                    SmObjsDelete<SmCurve*> c2(&au);
                }
            }
            SmTArray<SmCurve*> a3, au;
            SmBoolean has = cache->HasCachedCurves(ctx, t, ap, an, key.pv(), key.pb(), key.pl(), key.pd(),
                                                   key.pp(), w3 ? &a3 : NULL, wu ? &au : NULL);
            o.push_back(flg(has == TRUE));
            put_curve_array(o, a3);
            put_curve_array(o, au);
            SmObjsDelete<SmCurve*> c1(&a3);
            SmObjsDelete<SmCurve*> c2(&au);
        }
        break;
    }
    case OP_TREETOP:
    {
        int64_t nu = rd.idx();
        int64_t nv = rd.idx();
        TreePeek* tree = new (ctx) TreePeek(cache);
        tree->node_mgr().Initialize(ALIGN_SIZE(sizeof(SmTreeNode)), 25);
        cache->bez().Initialize(ALIGN_SIZE(sizeof(SmBezierPatch)), 25);
        cache->tree() = tree;
        SmTArray<SmTreeNode*> first;
        std::vector<SmBezierPatch*> pats;
        for (int64_t k = 0; k < nu * nv; k++)
        {
            SmTreeNode* nd = (SmTreeNode*)tree->node_mgr().GetNewElement();
            SmBezierPatch* p = (SmBezierPatch*)cache->bez().GetNewElement();
            SmVector3d a = rd.vec();
            SmVector3d b = rd.vec();
            nd->m_sBBox = SmExtent3d();
            nd->m_sBBox.AddPoint3d(a);
            nd->m_sBBox.AddPoint3d(b);
            nd->m_pTree = tree;
            nd->m_pParent = NULL;
            nd->m_pChild1 = NULL;
            nd->m_pChild2 = NULL;
            nd->m_eAuxDataType = SM_AD_BEZIER_SURFACE;
            nd->m_eGeomType = SM_NG_DEFAULT;
            nd->m_pData = p;
            p->m_eSplitDir = SM_SP_UNKNOWN;
            p->m_bIsMarked = FALSE;
            p->m_eNodeClass = SM_NC_UNKNOWN;
            p->m_pFace = NULL;
            p->mBA_pSurface = s;
            p->m_pOwningTreeNode = nd;
            p->m_sUVDomain = rd.domain();
            first.Add(nd);
            pats.push_back(p);
        }
        ULONG lu = (ULONG)nu;
        ULONG lv = (ULONG)nv;
        SmStatus r = cache->BuildTreeTop(first, lu, lv);
        o.push_back(st(r));
        SmTArray<SmTreeNode*> all;
        tree->GetAllTreeNodes(all);
        std::vector<SmTreeNode*> nodes;
        for (ULONG i = 0; i < all.GetSize(); i++)
            nodes.push_back(all[i]);
        o.push_back((double)index_of(nodes, tree->GetTopNode()));
        o.push_back((double)nodes.size());
        for (SmTreeNode* nd : nodes)
        {
            o.push_back((double)index_of(nodes, nd->m_pParent));
            o.push_back((double)index_of(nodes, nd->m_pChild1));
            o.push_back((double)index_of(nodes, nd->m_pChild2));
            o.push_back((double)nd->m_eAuxDataType);
            o.push_back((double)nd->m_eGeomType);
            const SmBezierAux2d* a = (const SmBezierAux2d*)nd->m_pData;
            o.push_back((double)a->m_eSplitDir);
            o.push_back(flg(a->m_bIsMarked == TRUE));
            o.push_back((double)a->m_eNodeClass);
            o.push_back(flg(a->mBA_pSurface != NULL));
            o.push_back((double)index_of(nodes, a->m_pOwningTreeNode));
            put_box(o, nd->m_sBBox);
            put_dom(o, a->GetUVDomain());
        }
        o.push_back((double)cache->GetPatchCount());
        for (ULONG i = 0; i < lu; i++)
            for (ULONG j = 0; j < lv; j++)
            {
                SmBezierPatch* p = cache->GetPatchAt(i, j, lu, lv);
                int64_t k = -1;
                for (size_t x = 0; x < pats.size(); x++)
                    if (pats[x] == p)
                        k = (int64_t)x;
                o.push_back((double)k);
            }
        break;
    }
    case OP_SUBDIV:
    {
        std::vector<SmTreeNode*> nodes = read_tree(rd, cache, s, ctx);
        int64_t nprog = rd.idx();
        for (int64_t q = 0; q < nprog; q++)
        {
            int64_t code = rd.idx();
            switch (code)
            {
            case 0:
            {
                SmNodeClassType c = (SmNodeClassType)rd.idx();
                int64_t k = rd.idx();
                SmTArray<SmTreeNode*> stack;
                for (int64_t i = 0; i < k; i++)
                    stack.Add(nodes[(size_t)rd.idx()]);
                SmTreeNode* f = cache->FindUnmarkedLeafNode(c, stack);
                o.push_back((double)index_of(nodes, f));
                break;
            }
            case 1:
            {
                SmTreeNode* start = nodes[(size_t)rd.idx()];
                SmTArray<SmTreeNode*> region;
                SmTreeNode* cn = NULL;
                cache->GetConnectedRegion(start, region, cn);
                o.push_back((double)region.GetSize());
                for (ULONG i = 0; i < region.GetSize(); i++)
                    o.push_back((double)index_of(nodes, region[i]));
                o.push_back((double)index_of(nodes, cn));
                break;
            }
            case 2: o.push_back(st(cache->MarkFaceContainment())); break;
            case 3:
            {
                SmBezierAux2d* a = (SmBezierAux2d*)nodes[(size_t)rd.idx()]->m_pData;
                a->m_bIsMarked = rd.flag() ? TRUE : FALSE;
                a->m_eNodeClass = (SmNodeClassType)rd.idx();
                a->m_pFace = obj_ptr(rd.idx());
                break;
            }
            case 4:
                for (SmTreeNode* nd : nodes)
                {
                    const SmBezierAux2d* a = (const SmBezierAux2d*)nd->m_pData;
                    o.push_back(flg(a->m_bIsMarked == TRUE));
                    o.push_back((double)a->m_eNodeClass);
                    o.push_back((double)obj_id(a->m_pFace));
                }
                break;
            case 5:
            {
                const gw_SURFACE* b = nodes[(size_t)rd.idx()]->GetFirstBezierPatch();
                o.push_back(b == NULL ? 0.0 : (b == s->GetGwNurbPointer() ? 1.0 : 2.0));
                break;
            }
            default: break;
            }
        }
        break;
    }
    case OP_CONT:
    {
        int64_t nu = rd.idx();
        for (int64_t i = 0; i < nu; i++)
            cache->uconts().Add((SmContinuityType)rd.idx());
        int64_t nv = rd.idx();
        for (int64_t i = 0; i < nv; i++)
            cache->vconts().Add((SmContinuityType)rd.idx());
        SmExtent2d d = rd.domain();
        o.push_back(0.0);
        o.push_back((double)cache->GetSurfaceContinuity(d));
        break;
    }
    case OP_ISO:
    {
        // a cache over the given domain
        SmExtent2d cd = rd.domain();
        delete cache;
        cache = new (ctx) CachePeek(*s, cd);
        int64_t k = rd.idx();
        o.push_back(0.0);
        for (int64_t i = 0; i < k; i++)
        {
            SmExtent2d d = rd.domain();
            double tol = rd.num();
            SmBoolean cached = FALSE;
            SmTArray<SmBSplineCurve*> curves;
            SmStatus r = cache->GetIsoBoundaryCurves(d, tol, cached, curves);
            o.push_back(st(r));
            if (r == SM_SUCCESS)
            {
                o.push_back(flg(cached == TRUE));
                for (ULONG j = 0; j < curves.GetSize(); j++)
                    put_curve(o, curves[j]);
                if (cached != TRUE)
                {
                    SmObjsDelete<SmBSplineCurve*> c(&curves);
                }
            }
            o.push_back(cache->iso_tol());
        }
        break;
    }
    case OP_POLAR:
    {
        std::vector<SmTreeNode*> nodes = read_tree(rd, cache, s, ctx);
        int64_t k = rd.idx();
        o.push_back(0.0);
        for (int64_t i = 0; i < k; i++)
        {
            SmBezierAux2d* a = (SmBezierAux2d*)nodes[(size_t)rd.idx()]->m_pData;
            put_polar(o, a->GetPolarBox());
        }
        for (SmTreeNode* nd : nodes)
            o.push_back((double)((const SmBezierAux2d*)nd->m_pData)->GetPolarBoxPtr()->GetPolarBoxState());
        break;
    }
    case OP_MISC:
    {
        SmExtent2d d = rd.domain();
        bool pte = rd.flag();
        SmPoint2d uv = rd.pt2();
        delete cache;
        cache = new (ctx) CachePeek(*s, d);
        cache->m_bPointTestEnabled = pte ? TRUE : FALSE;
        o.push_back(0.0);
        o.push_back((double)cache->GetType());
        o.push_back(flg(cache->IsKindOf(SmSurfaceCache_TYPE) == TRUE));
        o.push_back(flg(cache->IsKindOf(SmObject_TYPE) == TRUE));
        o.push_back(flg(cache->IsKindOf(SmSurface_TYPE) == TRUE));
        o.push_back(flg(cache->IsKindOf(SmTrimSrfCache_TYPE) == TRUE));
        o.push_back(flg(cache->GetProcessBoundaryCurves() == TRUE));
        o.push_back((double)cache->GetPatchCount());
        SmBoolean ok = FALSE;
        o.push_back(st(cache->PointTest(uv, ok)));
        o.push_back(flg(ok == TRUE));
        SmPointClassificationType pc = SM_PC_UNKNOWN;
        SmObject* obj = NULL;
        o.push_back(st(cache->PointClassify(uv, SmTol::GetZoneTol3d(), pc, obj)));
        SmExtent2d fd = d;
        o.push_back(st(cache->ImplantEdgeuse(NULL, TRUE, fd)));
        o.push_back(st(cache->ImplantPolyEdge(NULL, NULL)));
        put_dom(o, cache->GetUVDomain());
        o.push_back(flg(cache->GetSurface() == s));
        o.push_back(flg(cache->GetPatchBoundaryBoundingPlanes().ArePlanesComputed() == TRUE));
        break;
    }
    case OP_FACE:
        o.push_back(0.0);
        o.push_back(flg(cache->GetFace() != NULL));
        break;
    default: break;
    }
    delete cache;
}
} // namespace

SC_EXPORT int32_t sc_surface_surface_cache_p1_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                                   int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    if (op == OP_PLANE3)
    {
        SmVector3d a = rd.vec();
        SmVector3d b = rd.vec();
        SmVector3d c = rd.vec();
        double tol = rd.num();
        SmPoint3d base(0.0, 0.0, 0.0);
        SmVector3d normal(0.0, 0.0, 0.0);
        SmBoolean exists = FALSE;
        SmBoolean coll = FALSE;
        SmStatus r = sc_ssc_p1::sm_ComputePlaneFrom3Points(a, b, c, base, normal, tol, exists, coll);
        o.push_back(st(r));
        o.push_back(flg(exists == TRUE));
        o.push_back(flg(coll == TRUE));
        if (exists == TRUE)
        {
            put_vec(o, base);
            put_vec(o, normal);
        }
    }
    else if (op == OP_CULL)
    {
        int64_t n = rd.idx();
        SmTArray<double> a;
        for (int64_t i = 0; i < n; i++)
            a.Add(rd.num());
        SmStatus r = sc_ssc_p1::sm_CullArray(a);
        o.push_back(st(r));
        o.push_back((double)a.GetSize());
        for (ULONG i = 0; i < a.GetSize(); i++)
            o.push_back(a[i]);
    }
    else
    {
        SmBSplineSurface* s = read_bspline(rd, ctx);
        cache_op(op, rd, ctx, s, o);
        delete s;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
