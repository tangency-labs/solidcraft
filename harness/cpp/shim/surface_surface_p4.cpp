// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surface_p4`: the members of SmSurface.cpp
// (source/SMLib/src/SmSurface.cpp, source/SMLib/inc/SmSurface.h) owned by the unit --
// SmSurfaceEval::Normal / EvalNormalSection, sm_PassSPConvergenceTest,
// SmSurface::CreateSeamCurve / IsOnSeam / GlobalPointSolve, sm_ClassifyCurveSurfaceXSectPoint and the
// Newton equation sets SmCSIntersectENFO, SmCSNormalizeENFO, SmSSPIntersectENFO,
// SmSSINormalizeENFO and SmSSNormalizeENFO -- plus the co-ported smsurf_EvaluateNormalSection and
// smsurf_IsPointOnNormal (exported by upstream, called directly).
//
// Static functions and file-local classes of SmSurface.cpp cannot be linked against, so those
// exercised here are VERBATIM copies of upstream SmSurface.cpp (commit c9c5979), inside the
// namespace sc_srf_p4 (line ranges noted at each block).
//
// One entry point, sc_surface_surface_p4_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/surface_surface_p4.rs, which validates every
// input. A surface block is `0 oob form n m p q r s cpts(x y z w) u v` (an
// SmBSplineSurface(gw_SURFACE*, FALSE, NULL)), `1 O[3] N[3]` (SmPlane(O, N)) or `2 basis implicit`
// (an SmCurveBoundedSurface without boundary curves over a basis block); a curve block
// is `dim n p m cpts(x y z w) knots` (SmBSplineCurve(gw_CURVE*, dim, FALSE)). `out` receives
// the op status (0 = SM_SUCCESS, else the SmStatus value) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCrvOnSurf.h>
#include <SmCurve.h>
#include <SmCurveBoundedSurface.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmGeomUtility.h>
#include <SmLine.h>
#include <SmLocalSolveNd.h>
#include <SmMatrix.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmSurfTypes.h>
#include <SmTArray.h>
#include <SmTol.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

#include "sc_sm.h"

using sc::put_uv;
using sc::put_vec;

namespace sc_srf_p4
{
// clang-format off

// ---- verbatim: SmSurface.cpp lines 4462-4501: sm_PassSPConvergenceTest
static SmBoolean sm_PassSPConvergenceTest
  (const SmSurface & crSurface,      // in : Target Surface
   const SmPoint2d & crUV,           // in : Test SurfacePoint
   const SmPoint3d & crSolvedPoint,  // in : 3D Solution Point
   double d3dTol
  )
{
  SmBoolean bRetVal;

  // Locals for call:
  SmStatus eReason;
  double dDropToSurf, rdApproxDev ;
  SmBoolean bFromLeftU, bFromLeftV ;

  // test crSolvedPoint/SurfaceNormal(crUV) dist against d3dTol
  SmStatus eStat = smsurf_IsPointOnNormal
            (crSurface,              // in : target surface
             NULL,                   // in : clamp domain: if present, clamp crUVToTest to this domain.
                                     //      NULL to ignore
             crUV,                   // in : Surface test 2Dpoint
             crSolvedPoint,          // in : input   test 3DPoint
             NULL,                   // in : associated 3d tangent, NULL to ignore
             bFromLeftU, bFromLeftV, // out: associated from left eval values, NULL to ignore
             d3dTol,                 // in : Max dist allowed between unique points
             bRetVal,                // out: TRUE=TestPoint within tol of SurfaceNormal line, FALSE=not
             eReason,                // out: SM_ERR_OUTSIDE_OF_DOMAIN    = input UVTestPoint is not within domain or
                                     //                                    input UVTestPoint is on surface natural boundary and
                                     //                                        vector from SurfacePoint to TestPoint runs outside of surface domain
                                     //      SM_ERR_BAD_SURFACE_POINT    = Surface.Evaluate() fails - testing impossible)
                                     //      SM_ERR_NOT_WITHIN_TOLERANCE = test run - Point not on normal
                                     //      SM_ERR_UNKNOWN              = test run - Point is on normal
             dDropToSurf,            // out: when rbGoodPoint == TRUE, set to SurfPt/TestPt distSq
                                     //           rbGoodPoint == FALSE, set to 0.0
             rdApproxDev) ;          // out: deviation between 3dPoint and srfNormalLine

  bRetVal &= ( eStat == SM_SUCCESS );

  return bRetVal;

} // end sm_PassSPConvergenceTest

// ---- verbatim: SmSurface.cpp lines 13053-13063: sm_ConvertToParameterDelta
static double sm_ConvertToParameterDelta
  (const SmVector3d & crTangentVector,
   const SmVector3d & crDeltaVector)
{
    double dTanLen = crTangentVector.Length();
    if (dTanLen < SM_EFF_ZERO) return 0.0;
    SmVector3d sUnitTan = crTangentVector / dTanLen;
    double dDot = sUnitTan.Dot(crDeltaVector);
    return dDot/dTanLen;

} // end sm_ConvertToParameterDelta

// ---- verbatim: SmSurface.cpp lines 13080-13773: sm_ClassifyCurveSurfaceXSectPoint
static SmStatus sm_ClassifyCurveSurfaceXSectPoint
  (const SmSurface  & crSurface,             // in : Target Surface
   const SmExtent2d & crUVDomain,            // in : Target Surface Domain
   const SmCurve    & crCurve,               // in : Target Curve
   const SmExtent1d & crInterval,            // in : Target Curve Domain
   const SmPoint2d  & crCurrentUV,           // in : Current Surface Solution Point
   double             dCurrentT,             // in : Current Curve Solution Point
   SmBoolean        & rbFoundGuess,          // out: TRUE = Found a nearby intersection guess point
                                             //         will be set to FALSE when rePointType != SM_TC_NEAR_TANGENT
                                             //         or when 2nd guessPoint falls outside of input domains.
   SmPoint2d        & rNewUVGuess,           // out: Surface GuessPoint
   double           & rdNewTGuess,           // out: Curve GuessPoint
   SmTsectCurveType & rePointType)           // out: oneof: SM_TC_TOUCHING - degenerate curve marks a point on the surface
                                             //             SM_TC_CROSSING - curve crosses surface at a single point
                                             //             SM_TC_TANGENT - one intersection point
                                             //             SM_TC_COINCIDENT - curve likely to be coincident near tgt point
                                             //             SM_TC_NEAR_TANGENT - when rbFoundGuess is set to true, curve
                                             //                 is likely to have a 2nd nearby intersection point
{
  // init output
  rbFoundGuess = FALSE ;
  rePointType  = SM_TC_CROSSING ;

  // Get SurfacePoint Evaluation and SurfaceNormal
  SmVector3d sPVV[3][3];  // out: matrix of evaluations values
                          //      organized: [D    Du    Duu  ]
                          //                 [Dv   Duv   0    ]
                          //                 [Dvv  0     0    ]
  SER(crSurface.Evaluate(crCurrentUV, 2, 2, TRUE, TRUE, TRUE, &sPVV[0][0], FALSE)); // exact Tangent values ?may need to change?

  // get surface Normal
  SmVector3d sSurfaceNormal = sPVV[1][0] * sPVV[0][1];
  if (sSurfaceNormal.LengthSquared() < SM_EFF_ZERO_SQ)
       { SER(crSurface.EvaluateNormal(crCurrentUV, TRUE, TRUE, sSurfaceNormal));
       }
  else { SER(sSurfaceNormal.Unitize());
       }
  sPVV[2][2] = sSurfaceNormal; // Must be here for smsurf_EvaluateGeometric().

  // Get the geometric properties of the curve at the current intersection
  SmVector3d sCurveGeomVec[4] ; // out: a[0] = position
                                //      a[1] = 1st derivative,   Mag = Speed,   direction = tangent vector
                                //      a[2] = Curvature Vector, Mag = K,       direction = normal vector
                                //      a[3] = Torsion Vector,   Mag = Torsion, direction = binormal vector
                                // note: Osculating circle center: center = pos + RadOfCurv * NormalVector
                                //       center = a[0] + a[2]/a[2].LengthSquared(), when a[2].LengthSquared() is not zero.
  if(SM_ERR == crCurve.EvaluateGeometric(dCurrentT, 2, TRUE, sCurveGeomVec))
    { // no extra guesses or possible coincidence signalled for degenerate curves
      rePointType = SM_TC_TOUCHING ;
      return(SM_SUCCESS) ;
    }

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE ;
  if(bDebugMe)
    {
      SmBrep *pBrep = crSurface.GetFace() ? ((SmFace *)crSurface.GetFace())->GetBrep() : NULL ;
      if(pBrep == NULL) pBrep = crCurve.GetOwner() ? ((SmEdge *)crCurve.GetEdge())->GetBrep() : NULL ;

      smgfx_Erase() ;
      smgfx_SetLook(1,2, 0,0,1) ; if(pBrep) pBrep->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,1,1) ; crSurface.DrawUV() ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3, 0,1,0) ; crCurve.Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(4,5, 1,0,0) ; sPVV[0][0].Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(5,6, 1,0,1) ; sCurveGeomVec[0].Draw() ; sm_GraphicsLoop() ;
      sm_GraphicsLoop() ;
    }
#endif // SM_DEBUG_CODE

  // no work - curve is not within 20 degrees of tangency to surface
  if (!sSurfaceNormal.IsPerpendicularTo(sCurveGeomVec[1],20.0))
    { rePointType  = SM_TC_CROSSING ;
      return SM_SUCCESS;
    }

  // Evaluate Surface Position and NormalSection in direction of CurveTangent.
  // This should give us two circles or a line and a circle to intersect.
  SmPoint3d  sSurfacePoint = sPVV[0][0] ;
  SmVector3d sSurfaceSectionTangent;
  double     dSurfaceSectionCurvature;

  // Surface differential locals
  double     dGaussianCurvature ;        // out: GaussianCurvature = K1*K2 (principal curvatures)
  double     dNormalCurvature ;          // out: MeanCurvature = 1/2(K1 + K2)
  double     dPrincipleCurvature1 ;      // out: 1st principal curvature value ; K1
  double     dPrincipleCurvature2 ;      // out: 2nd principal curvature value ; K2
  SmVector3d sEFGOfFirstFundForm ;       // out: 1st fundamental form ; [E ; F ; G]
  SmVector3d sLMNOfSecondFundForm ;      // out: 2nd fundamental form ; [L ; M ; N]
  SmVector3d PrincipleCurvatureVector1 ; // out: 3d vector tangent to surface in 1st principal direction
  SmVector3d PrincipleCurvatureVector2 ; // out: 3d vector tangent to surface in 2nd principal direction

  SER(crSurface.EvaluatePoint(crCurrentUV, sSurfacePoint));
  SER(smsurf_EvaluateNormalSection(sPVV, sCurveGeomVec[1],
                                   sSurfaceSectionTangent,
                                   dSurfaceSectionCurvature));
  SER(smsurf_EvaluateGeometric(sPVV, dGaussianCurvature,
                                     dNormalCurvature,
                                     dPrincipleCurvature1,
                                     dPrincipleCurvature2,
                                     sEFGOfFirstFundForm,
                                     sLMNOfSecondFundForm,
                                     PrincipleCurvatureVector1,
                                     PrincipleCurvatureVector2)) ;
  SmLocalSurfaceType eSurfaceType =   (   smos_Fabs(dNormalCurvature)    < SM_EFF_ZERO_SQRT
                                       && smos_Fabs(dPrincipleCurvature1) < SM_EFF_ZERO_SQRT
                                       && smos_Fabs(dPrincipleCurvature2) < SM_EFF_ZERO_SQRT) ? SM_LS_PLANAR
                                    : (dGaussianCurvature >  SM_EFF_ZERO_SQRT) ? SM_LS_ELLIPTICAL
                                    : (dGaussianCurvature < -SM_EFF_ZERO_SQRT) ? SM_LS_HYPERBOLIC
                                    : SM_LS_CYLINDRICAL ;

  // Get Curve Curvature, Normal vector, and Binormal vectors
  SmVector3d sCurveNormal    = sCurveGeomVec[2];
  double     dCurveCurvature = sCurveNormal.Length();
  if (smos_Fabs(dCurveCurvature) > SM_EFF_ZERO_SQRT)
    {
      sCurveNormal /= dCurveCurvature ;
    }
  else // curve is close to being a straight line - let CurveNormal = sSurfaceNormal
    {
      sCurveNormal = sSurfaceNormal ;
    }
  SmVector3d sCurveBinormal = sCurveGeomVec[1] * sCurveNormal ;

  // degenerate curve check
  if (sCurveBinormal.LengthSquared() < SM_EFF_ZERO_SQ)
    {
      rePointType  = SM_TC_TOUCHING ;
      return SM_SUCCESS ;
    }
  SER(sCurveBinormal.Unitize()) ;

  // get the cosine of the angle between the surfaceSectionNormal plane and
  // the plane of the curve's osculating circle (needed for Meusnier's Theorem)
  double dAngRad ;
  sCurveBinormal.AngleBetween(sSurfaceNormal, dAngRad) ;
  double dCosPhi = smos_Fabs(smos_Cosine(dAngRad - SM_PI/2.0)) ;

  // characterize the curve shape as a circle or a line
  SmBoolean bCurveLine = FALSE ;
  double dCurveRadius = 0.0;
  SmPoint3d sCurveCenter, sCurvePoint ;
  SmVector3d sCurveCircleNormal, sCurveVec ;
  if(smos_Fabs(dCurveCurvature) > SM_EFF_ZERO_SQRT)
    {
      // let Curve = curve osculating circle
      bCurveLine         = FALSE ;
      dCurveRadius       = smos_Fabs(1.0/dCurveCurvature);
      sCurveCenter       = sCurveGeomVec[0] + sCurveNormal * dCurveRadius;
      sCurveCircleNormal = sCurveBinormal ;
    }
  else // curve is a line
    {
      bCurveLine         = TRUE ;
    }

  // always load CurvePoint and CurveVec because when circle is large
  // the curve is treated as a line
  sCurvePoint = sCurveGeomVec[0] ;
  sCurveVec   = sCurveGeomVec[1] ;

  // characterize the surface section curve or
  // the surface dupin indicatrix shape (when curve plane is tangent to surface) as
  // an osculating circle in the CurvePlane, a line, or two lines
  SmBoolean bSurfaceLine = FALSE, bSurfaceTwoLines = FALSE ;
  double    dSurfaceRadius = 0.0;
  SmPoint3d sSurfaceCenter ;
  SmVector3d sSurfaceCircleNormal, sSurfaceVec, sSurfaceVec2 ;

  // if the curve plane is tangent to the surface plane
  if(smos_Fabs(dCosPhi) < SM_EFF_ZERO_RAD)
    {
      // It is either coincident, tangent or tangent and needs to be intersected
      // with the asymptopes of the dupin's indicatrix curves.
      switch(eSurfaceType)
        {
          case SM_LS_PLANAR :      rePointType = SM_TC_COINCIDENT ;
                                   return(SM_SUCCESS) ;

          case SM_LS_ELLIPTICAL :  rePointType = SM_TC_TANGENT ;
                                   return(SM_SUCCESS) ;

          case SM_LS_HYPERBOLIC :  { rePointType      = SM_TC_TANGENT ;
                                     bSurfaceLine     = TRUE ;
                                     bSurfaceTwoLines = TRUE ;

                                     // Let us compute the vectors by evaluating the hyperbola
                                     // K1*x^2 + K2*y^2 = 1 at a large X value and computing the
                                     // Y.  This should give us a vector in terms of the principle
                                     // curvature vectors which defines the asymptotic directions of the
                                     // hyperbola.
                                     double dY   = 10000.0;
                                     double dTmp = (1-dPrincipleCurvature1*dY*dY) / dPrincipleCurvature2;
                                     if (dTmp < 0.0) dTmp = 0.0;  // should be caught as elliptical or cylindrical cases, but just in case. . .
                                     double dX = smos_Sqrt( dTmp );
                                     //double dSize = smos_Sqrt(dY*dY + dX*dX) ;

                                     sSurfaceVec  = dY * PrincipleCurvatureVector1 + dX * PrincipleCurvatureVector2;
                                     sSurfaceVec2 = dY * PrincipleCurvatureVector1 - dX * PrincipleCurvatureVector2;
                                     sSurfaceVec.Unitize() ;
                                     sSurfaceVec2.Unitize() ;
                                   }
                                   break ;

          case SM_LS_CYLINDRICAL : rePointType      = SM_TC_TANGENT ;
                                   bSurfaceLine     = TRUE ;
                                   bSurfaceTwoLines = FALSE ;
                                   sSurfaceVec  =   smos_Fabs(dPrincipleCurvature1) < SM_EFF_ZERO_SQRT
                                                  ? PrincipleCurvatureVector1
                                                  : PrincipleCurvatureVector2 ;
                                   break ;

        } // end switch on eSurfaceType
    } // end curvePlane is tangent to surface branch

  // else curvePlane is not tangent to surface and when the surfaceSection is flat
  else if (smos_Fabs(dSurfaceSectionCurvature) < SM_EFF_ZERO_SQRT)
    {
      bSurfaceLine     = TRUE ;
      bSurfaceTwoLines = FALSE ;
      sSurfaceVec      = sSurfaceSectionTangent ;
    }
  else // curvePlane is not tangent to surface and surfaceSection is curved
    {
      bSurfaceLine     = FALSE ;
      bSurfaceTwoLines = FALSE ;
      sSurfaceVec      = sSurfaceSectionTangent ;

      // get surface osculating circle in the curve's plane
      //   remember to use Meusnier's theorem to pick correct SurfaceCircleRadius
      dSurfaceRadius = smos_Fabs(1.0 / dSurfaceSectionCurvature) * dCosPhi ;

      // rotate the surface normal into the curve's plane about the tangent
      // vector - do this by projecting the surface normal onto the curve plane
      // and unitizing.
      SmVector3d sSurfaceCircleCenterVec = (sCurveBinormal * (smos_Sgn(dSurfaceSectionCurvature) * sSurfaceNormal)) * sCurveBinormal ;
      sSurfaceCircleCenterVec.Unitize() ;

      // sCircle1Center = lies in direction of the sSurfaceCircleCenterVec not the sSurfaceNormal
      sSurfaceCenter       = sSurfacePoint + sSurfaceCircleCenterVec * dSurfaceRadius ;
      sSurfaceCircleNormal = sCurveBinormal;
    } // end get surface osculating circle branch

#ifdef SM_DEBUG_CODE
  if(bDebugMe)
    {
      SmBrep *pBrep = crSurface.GetFace() ? ((SmFace *)crSurface.GetFace())->GetBrep() : NULL ;
      if(pBrep == NULL) pBrep = crCurve.GetOwner() ? ((SmEdge *)crCurve.GetEdge())->GetBrep() : NULL ;

      smgfx_Erase() ;
      smgfx_SetLook(1,2, 0,0,1) ; if(pBrep) pBrep->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,1,1) ; crSurface.DrawUV() ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3, 0,1,0) ; crCurve.Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(4,5, 1,0,0) ; sPVV[0][0].Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(5,6, 1,0,1) ; sCurveGeomVec[0].Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3, 1,.5,0); if(bSurfaceLine) { (3.0 * sSurfaceVec).Draw(&sSurfacePoint) ; sm_GraphicsLoop() ;
                                                    (-3.0 * sSurfaceVec).Draw(&sSurfacePoint) ; sm_GraphicsLoop() ;
                                                    if(bSurfaceTwoLines)
                                                       { (3.0 * sSurfaceVec2).Draw(&sSurfacePoint) ; sm_GraphicsLoop() ;
                                                         (-3.0 * sSurfaceVec2).Draw(&sSurfacePoint) ; sm_GraphicsLoop() ; } }
                                  else { SMGFX_DRAWLIST(NULL, 
                                                        smgfx_DrawCircularArc(sSurfaceCenter, 
                                                                              (sSurfacePoint-sSurfaceCenter),
                                                                              (sSurfacePoint-sSurfaceCenter)*sSurfaceCircleNormal,
                                                                              dSurfaceRadius, 
                                                                              -90.0, 90.0, 
                                                                              dSurfaceRadius/100,
                                                                              NULL)) ; sm_GraphicsLoop() ; }
      smgfx_SetLook(3,4, .5,1,0); if(bCurveLine) { (3.0 * sCurveVec).Draw(&sCurvePoint) ; sm_GraphicsLoop() ;
                                                  (-3.0 * sCurveVec).Draw(&sCurvePoint) ; sm_GraphicsLoop() ; }
                                  else{ SMGFX_DRAWLIST(NULL, 
                                                       smgfx_DrawCircularArc(sCurveCenter, 
                                                                             (sCurvePoint-sCurveCenter),
                                                                             (sCurvePoint-sCurveCenter)*sCurveCircleNormal,
                                                                             dCurveRadius, 
                                                                             -90.0, 90.0, 
                                                                             dCurveRadius/100,
                                                                             NULL)) ; sm_GraphicsLoop() ; }
      sm_GraphicsLoop() ;
    }
#endif // SM_DEBUG_CODE

  // We need a tolerance for line and circle intersections.
  // They're purely geometric, but could well have tolerance issues.
  // Look to topology first, otherwise use heuristics.

  double dCurveTol   = SM_EFF_ZERO_SQRT; // default values, if nothing else to use.
  double dSurfaceTol = SM_EFF_ZERO_SQRT;

  SmEdge *pCurveOwner = SM_CAST_PTR( SmEdge, crCurve.GetOwner() );
  if ( pCurveOwner != NULL )
    { dCurveTol = pCurveOwner->GetTolerance(); }
  else if ( !bCurveLine )
    { dCurveTol = dCurveRadius / 1000; }

  SmFace *pSurfaceOwner  = SM_CAST_PTR( SmFace, crSurface.GetOwner() );
  if ( pSurfaceOwner != NULL )
    { dSurfaceTol = pSurfaceOwner->GetTolerance(); }
  else if ( !bSurfaceLine )
    { dSurfaceTol = dSurfaceRadius / 1000; }

  double d3DTol = dCurveTol + dSurfaceTol;

  // when both curve and surface shapes are lines
  if(bCurveLine && bSurfaceLine)
    {
      // classify the point as crossing or coincident
      rePointType =  (   (   bSurfaceTwoLines && sCurveVec.IsParallelTo(sSurfaceVec2, 2.0))
                      || (   bSurfaceLine     && sCurveVec.IsParallelTo(sSurfaceVec, 2.0)))
                    ? SM_TC_COINCIDENT
                    : SM_TC_CROSSING ;
      // all done
      return(SM_SUCCESS) ;
    }

   // when both shapes are curved - see if one should be treated as a line
   if(!bCurveLine && !bSurfaceLine)
     {
       // when osculating circle radii vastly differ - treat one shape as a line
       // (Note, 2.0 is not enough. [B30])
       if     (dCurveRadius   > 1000.0 * dSurfaceRadius) { bCurveLine   = TRUE ; }
       else if(dSurfaceRadius > 1000.0 * dCurveRadius)   { bSurfaceLine = TRUE ; }
     }

   // look for second intersections
   SmVector3d sFoundVector ;

   // when curve is linear or close enough
   if(bCurveLine)
     {
       // intersect CurveLine with SurfaceOsculating Circle
       ULONG lNumTsect;
       double adTsectLineParam[2];

       // intersect line with circle
       SER(smgu_LineCoPlanarCircleIntersect(sCurvePoint, sCurveVec,
                                            sSurfaceCenter, sSurfaceCircleNormal,
                                            dSurfaceRadius, d3DTol,
                                            lNumTsect,    adTsectLineParam));

       // If only have one or zero intersections we are done without
       // picking up extra guess or finding coincidence
       switch(lNumTsect)
         {
           // If no intersection, it's probably a tolerance thing,
           // and probably tangent.  (Was CROSSING in that case.)
           // But leave the warning in, it should be checked.
           case 0: { rePointType = SM_TC_TANGENT ; WARN(_T("Unexpected Branch")) ; return SM_SUCCESS; }
           case 1: { rePointType = SM_TC_TANGENT ; return SM_SUCCESS; }
           case 2: { rePointType = SM_TC_NEAR_TANGENT ; break ; }
         }

       // Find the other intersection -- i.e. the one we don't have
       double dLineT =   (smos_Fabs(adTsectLineParam[1]) > smos_Fabs(adTsectLineParam[0]))
                       ? adTsectLineParam[1]
                       : adTsectLineParam[0];
       SmPoint3d sOtherTsect = sCurvePoint + dLineT * sCurveVec;
       sFoundVector          = dLineT * sCurveVec ;
     } // end Curve is line branch

   // when surface is linear
   if(bSurfaceLine)
     {
       // intersect SurfaceLine with CurveOsculating Circle
       ULONG lNumTsect, lNumTsect2 = 0;
       double adTsectLineParam[2], adTsectLineParam2[2];
       SmVector3d sFoundVector2 ;
       SmTsectCurveType eLocalPointType = SM_TC_UNKNOWN, eLocalPointType2 = SM_TC_UNKNOWN ;

       // intersect line with circle
       SER(smgu_LineCoPlanarCircleIntersect(sSurfacePoint, sSurfaceVec,
                                            sCurveCenter, sCurveCircleNormal,
                                            dCurveRadius, d3DTol,
                                            lNumTsect,    adTsectLineParam));
       // use number of intersections to classify the point against SurfaceVec
       switch(lNumTsect)
         {
           // If no intersection, it's probably a tolerance thing,
           // and probably tangent.  (Was CROSSING in that case.)
           // But leave the warning in, it should be checked.
           case 0: { eLocalPointType = SM_TC_TANGENT ; WARN(_T("Unexpected Branch")) ; break ; }
           case 1: { eLocalPointType = SM_TC_TANGENT ; break ; }
           case 2: { eLocalPointType = SM_TC_NEAR_TANGENT ;
                     // Find the other intersection -- i.e. the one we don't have
                     double dLineT =   (smos_Fabs(adTsectLineParam[1]) > smos_Fabs(adTsectLineParam[0]))
                                     ? adTsectLineParam[1]
                                     : adTsectLineParam[0];
                     SmPoint3d sOtherTsect = sSurfacePoint + dLineT * sSurfaceVec;
                     sFoundVector          = dLineT * sSurfaceVec ;
                   }
                   break ;
         } // end switch on number of line/circle intersection points

       // when surface is represented by two lines
       if(bSurfaceTwoLines)
         {
           // intersect SurfaceLine2 with CurveOsculating Circle
           SER(smgu_LineCoPlanarCircleIntersect(sSurfacePoint, sSurfaceVec2,
                                                sCurveCenter, sCurveCircleNormal,
                                                dCurveRadius, d3DTol,
                                                lNumTsect2,    adTsectLineParam2));

           // use number of intersections to classify the point against SurfaceVec2
           switch(lNumTsect2)
             {
               // If no intersection, it's probably a tolerance thing,
               // and probably tangent.  (Was CROSSING in that case.)
               // But leave the warning in, it should be checked.
               case 0: { eLocalPointType2 = SM_TC_TANGENT ; WARN(_T("Unexpected Branch")) ; break ; }
               case 1: { eLocalPointType2 = SM_TC_TANGENT ; break ; }
               case 2: { eLocalPointType2 = SM_TC_NEAR_TANGENT ;
                         // Find the other intersection -- i.e. the one we don't have
                         double dLineT =   (smos_Fabs(adTsectLineParam2[1]) > smos_Fabs(adTsectLineParam2[0]))
                                         ? adTsectLineParam2[1]
                                         : adTsectLineParam2[0];
                         SmPoint3d sOtherTsect = sSurfacePoint + dLineT * sSurfaceVec;
                         sFoundVector2         = dLineT * sSurfaceVec ;
                       }
                       break ;
             } // end switch on number of line/circle intersection points
         } // end need to intersect with SurfaceVec2 check

       // When there are no near-tangent cases - classify the intersection and exit
       if(eLocalPointType != SM_TC_NEAR_TANGENT && eLocalPointType2 != SM_TC_NEAR_TANGENT)
         {
           rePointType = (!bSurfaceTwoLines) ? eLocalPointType : SM_TC_TANGENT ;
           return(SM_SUCCESS) ;
         }

       // arrive here when we have a near-tangent case
       rePointType = SM_TC_NEAR_TANGENT ;

       // when there are two near-tangent cases
       if(eLocalPointType == SM_TC_NEAR_TANGENT && eLocalPointType2 == SM_TC_NEAR_TANGENT)
         {
           // pick the closest 2nd intersection point (maybe we should use both)
           if(sFoundVector2.LengthSquared() < sFoundVector.LengthSquared())
             { sFoundVector = sFoundVector2 ; }
         }
       // when only the second intersection was a near tangent case
       else if(eLocalPointType2 == SM_TC_NEAR_TANGENT)
         {
           sFoundVector = sFoundVector2 ;
         }
       // else first intersection is a tangent case and sFoundVector is already set
     } // end when surface is linear branch

   // when curve and surface are both curved
   if(!bCurveLine && !bSurfaceLine)
     {
       // intersect osculating curve and surface circles
       ULONG lNumTsect;
       SmPoint3d sTsectPoints[2];

       // Do BiggerCircle/SmallerCircle intersection
       if (dCurveRadius > dSurfaceRadius)
         {
           SER(smgu_CircleCoPlanarCircleIntersect(sCurveCircleNormal,
               sCurveCenter,dCurveRadius,sSurfaceCenter,dSurfaceRadius,
               d3DTol,
               lNumTsect,sTsectPoints));
         }
       else
         {
           SER(smgu_CircleCoPlanarCircleIntersect(sSurfaceCircleNormal,
               sSurfaceCenter,dSurfaceRadius,sCurveCenter,dCurveRadius,
               d3DTol,
               lNumTsect,sTsectPoints));
         }

       // If only have one or zero intersections we are done without
       // picking up extra guess
       switch(lNumTsect)
         {
           // If no intersection, it's probably a tolerance thing,
           // and probably tangent.  (Was CROSSING in that case.)
           // But leave the warning in, it should be checked.
           case 0: { rePointType = SM_TC_TANGENT ; WARN(_T("Unexpected Branch")) ; return SM_SUCCESS; }
           case 1: { rePointType = SM_TC_TANGENT ;      return SM_SUCCESS; }
           case 2: { rePointType = SM_TC_NEAR_TANGENT ; break ; }
           case 3: { rePointType = SM_TC_COINCIDENT ;   return SM_SUCCESS; }
         }

       // else save new intersection guess point
       sFoundVector = sTsectPoints[0] - sSurfacePoint ;
       sFoundVector =   (sFoundVector.Length() < sTsectPoints[1].DistanceBetween(sSurfacePoint))
                      ? (sTsectPoints[1] - sSurfacePoint)
                      : (sTsectPoints[0] - sSurfacePoint) ;

     } // end curve and surface are both curved branch

// {          // let Circle1 = surface osculating circle in the curve's plane
//            //   remember to use Meusnier's theorem to pick correct circle1Radius
//            dCircle1Radius = smos_Fabs(1.0 / dSurfaceSectionCurvature) * dCosPhi ;
//            // SER(sSurfaceNormal.Unitize());  // all ready unitized
//            // SER(sCurveNormal.Unitize());    // all ready unitized
//
//            // rotate the surface normal into the curve's plane about the tangent
//            // vector - do this by projecting the surface normal onto the curve plane
//            // and unitizing.
//            SmVector3d sSurfaceCircleNormal = (sCurveBinormal * sSurfaceNormal) * sCurveBinormal ;
//            sSurfaceCircleNormal.Unitize() ;
//
//            // sCircle1Center = lies in direction of the sSurfaceCircleNormal not the sSurfaceNormal
//            sCircle1Center = sSurfacePoint + sSurfaceCircleNormal * dCircle1Radius ;
//            sCircle1Normal = sCurveBinormal;
//
//            // when Curve Curvature is or is close enough to being a line
//            if (   smos_Fabs(dCurveCurvature)     < SM_EFF_ZERO_SQRT
//                || smos_Fabs(1.0/dCurveCurvature) > 2.0 * dCircle1Radius)
//              {
//                sLinePoint = sCurveGeomVec[0];
//                sLineVec   = sCurveGeomVec[1];
//                SER(sLineVec.Unitize());
//                bLineCircleIntersection = TRUE;
//              }
//            else // curve and surface curvatures are close enough to take into consideration
//              {
//                // let Circle2 = curve osculating circle
//                dCircle2Radius = smos_Fabs(1.0/dCurveCurvature);
//
//                // when surface is flat compared to the curve
//                if (dCircle1Radius > 2.0 * dCircle2Radius)
//                  {
//                    // let circle1 = curve osculating circle
//                    dCircle1Radius = smos_Fabs(1.0/dCurveCurvature);
//                    sCircle1Center = sCurveGeomVec[0] + sCurveNormal * dCircle1Radius;
//                    sCircle2Normal = sCurveBinormal;
//
//                    // let line = surface tangent line
//                    sLinePoint = sSurfacePoint;
//                    sLineVec   = sSurfaceSectionTangent;
//                    SER(sLineVec.Unitize());
//                    bLineCircleIntersection = TRUE;
//                  }
//                else // curve and surface curvature are nearly equal branch
//                  {
//                    // let circle2 = curve osculating circle
//                    sCircle2Center = sCurveGeomVec[0] + sCurveNormal * dCircle2Radius;
//                    sCircle2Normal = sCurveBinormal;
//                  } // end nearly equal curve and surface curvatures branch
//              } // end curve ans surface curvatures are close to one another
//          } // end surface curvature in curve direction check
//
//        SmBoolean  bLineCircleIntersection = FALSE, bTwoLineIntersection = FALSE ;
//
//        double     dCircle1Radius, dCircle2Radius;
//        SmPoint3d  sLinePoint,          sCircle1Center, sCircle2Center;
//        SmVector3d sLineVec, sLineVec2, sCircle1Normal, sCircle2Normal;
//        SmVector3d sFoundVector;
//
//        // If the surface section curvature is zero then use a line instead of a circle for the surface.
//        // Also if the radius of one circle is greater then twice the
//        //   other use a line for the one with the larger radius.
//
//
//        // when test = circle/line intersection
//        if (bLineCircleIntersection)
//          {
//            ULONG lNumTsect;
//            double adTsectLineParam[2];
//
//            // intersect line with circle
//            SER(smgu_LineCoPlanarCircleIntersect(sLinePoint, sLineVec,
//                                                 sCircle1Center, sCircle1Normal,
//                                                 dCircle1Radius, SM_EFF_ZERO,
//                                                 lNumTsect,    adTsectLineParam));
//
//            // If only have one or zero intersections we are done without
//            // picking up extra guess or finding coincidence
//            switch(lNumTsect)
//              {
//                case 0: rePointType = SM_TC_CROSSING ; WARN(_T("Unexpected Branch")) ; return SM_SUCCESS;
//                case 1: rePointType = SM_TC_TANGENT ;      return SM_SUCCESS;
//                case 2: rePointType = SM_TC_NEAR_TANGENT ; break ;
//              }
//
//            // Find the other intersection -- i.e. the one we don't have
//            double dLineT = adTsectLineParam[0];
//            if (smos_Fabs(adTsectLineParam[1]) > smos_Fabs(adTsectLineParam[0]))
//              {
//                dLineT = adTsectLineParam[1];
//              }
//            SmPoint3d sOtherTsect = sLinePoint + dLineT * sLineVec;
//            sFoundVector = dLineT * sLineVec;
//          }
//        else // test = circle/circle interesection
//          {
//            ULONG lNumTsect;
//            SmPoint3d sTsectPoints[2];
//
//            // Do BiggerCircle/SmallerCircle intersection
//            if (dCircle1Radius > dCircle2Radius)
//              {
//                SER(smgu_CircleCoPlanarCircleIntersect(sCircle1Normal,
//                    sCircle1Center,dCircle1Radius,sCircle2Center,dCircle2Radius,
//                    SM_EFF_ZERO,
//                    lNumTsect,sTsectPoints));
//              }
//            else
//              {
//                SER(smgu_CircleCoPlanarCircleIntersect(sCircle2Normal,
//                    sCircle2Center,dCircle2Radius,sCircle1Center,dCircle1Radius,
//                    SM_EFF_ZERO,
//                    lNumTsect,sTsectPoints));
//              }
//
//            // If only have one or zero intersections we are done without
//            // picking up extra guess
//            switch(lNumTsect)
//              {
//                case 0: rePointType = SM_TC_CROSSING ; WARN(_T("Unexpected Branch")) ; return SM_SUCCESS;
//                case 1: rePointType = SM_TC_TANGENT ;      return SM_SUCCESS;
//                case 2: rePointType = SM_TC_NEAR_TANGENT ; break ;
//                case 3: rePointType = SM_TC_COINCIDENT ;   return SM_SUCCESS;
//              }
//
//            // else save new intersection guess point
//            sFoundVector = sTsectPoints[0] - sSurfacePoint;
//            if (sFoundVector.Length() < sTsectPoints[1].DistanceBetween(sSurfacePoint))
//              {
//                sFoundVector = sTsectPoints[1] - sSurfacePoint;
//              }
//          } // end test = circle/circle intersection

  // Use the found vector to compute an approximate step in the
  // curve and surface parameters.
  SmVector3d sDU = sPVV[1][0] ;
  SmVector3d sDV = sPVV[0][1] ;

  rbFoundGuess  = TRUE;
  // old style: rdNewTGuess   = dCurrentT     + sm_ConvertToParameterDelta(sCurveGeomVec[1],sFoundVector);
  // new style: account for circlular arc distance in curves.
  //    gwc: the two approaches were about the same so didn't bother to make a change to the surface guess.
  rdNewTGuess   = dCurrentT     + smos_Sgn(sCurveGeomVec[1].Dot(sFoundVector))
                                  * (  bCurveLine ? sFoundVector.Length()
                                                  : 2.0 * dCurveRadius * smos_ArcSine(sFoundVector.Length()/2.0/dCurveRadius))
                                  / sCurveGeomVec[1].Length();
  // Use the combined 2d shift, instead of two 1-d shifts. [B30]
  SmPoint2d sDeltaUV;
  SmStatus eStat = smgu_VecLinCombTwoVectors( sDU, sDV, sFoundVector, sDeltaUV );
  if ( eStat == SM_SUCCESS )
    {
      rNewUVGuess = crCurrentUV + sDeltaUV;
    }
  else
    {
      // Well, ok, if the 2d shift fails, then try two 1-d shifts.
      // Presumably one will succeed and one will fail, but the one that fails
      // is on a degenerate surface derivative, so its magnitude doesn't mean much anyway.
      rNewUVGuess.x = crCurrentUV.x + sm_ConvertToParameterDelta(sDU,sFoundVector);
      rNewUVGuess.y = crCurrentUV.y + sm_ConvertToParameterDelta(sDV,sFoundVector);
    }

  // skip guesses outside of both given domains - keep guesses in at least one domain
  if (   !crInterval.ContainsValue(rdNewTGuess)
      && !crUVDomain.ContainsPoint2d(rNewUVGuess))
    {
      // If the guess is outside of the domain we'll be able to pick it up using
      // another method.
      rbFoundGuess = FALSE;
      return SM_SUCCESS;
    }

  // set output
  rdNewTGuess = crInterval.ClampValue(rdNewTGuess);
  rNewUVGuess = crUVDomain.ClampPoint2d(rNewUVGuess);

#ifdef SM_DEBUG_CODE
  if(bDebugMe)
    {
      SmBrep *pBrep = crSurface.GetFace() ? ((SmFace *)crSurface.GetFace())->GetBrep() : NULL ;
      if(pBrep == NULL) pBrep = crCurve.GetOwner() ? ((SmEdge *)crCurve.GetEdge())->GetBrep() : NULL ;
      SmVector3d sNewCurvePt, sNewSurfacePt ;
      crCurve.EvaluatePoint(rdNewTGuess, sNewCurvePt) ;
      crSurface.EvaluatePoint(rNewUVGuess, sNewSurfacePt) ;

      smgfx_Erase() ;
      smgfx_SetLook(1,2, 0,0,1) ; if(pBrep) pBrep->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,1,1) ; crSurface.DrawUV() ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3, 0,1,0) ; crCurve.Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(4,5, 1,0,0) ; sPVV[0][0].Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,5, 1,.5,0); sPVV[0][0].DrawPointToPoint(sPVV[0][0]+sFoundVector) ; sm_GraphicsLoop() ;
      smgfx_SetLook(5,6, 1,0,1) ; sCurveGeomVec[0].Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(7,8, 0,0,1) ; sNewCurvePt.Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(7,8, 0,.5,1); sNewSurfacePt.Draw() ; sm_GraphicsLoop() ;
      sm_GraphicsLoop() ;
    }
#endif // SM_DEBUG_CODE

  // all done
  return SM_SUCCESS;  // Found a guess - loaded it now we are done

} // end sm_ClassifyCurveSurfaceXSectPoint

// ---- verbatim: SmSurface.cpp lines 24435-24468: class SmCSIntersectENFO
class SmCSIntersectENFO : public SmEvalNFunctionsObject
{
protected:
  const SmCurve   & m_crCurve;           // Target Curve
  const SmSurface & m_crSurface;         // Target Surface
  double            m_dSquaredTolerance; // not currently used
  ULONG             m_lCount;            // Number of calls to Evaluate
  double            m_dLastSum;          //
  SmBoolean         m_bDoMinimize;       // TRUE = Find closest Curve/Surface point pair
                                         //        using different equations in Evaluate().
                                         // FALSE= minimize Curve/Surface point distance
public:
  // constructor, destructor
  SmCSIntersectENFO
    (const SmCurve   & crCurve,
     const SmSurface & crSurface,
     double            dSquaredTolerance)
   : m_crCurve(crCurve),
     m_crSurface(crSurface),
     m_dSquaredTolerance(dSquaredTolerance),
     m_lCount(0),
     m_dLastSum(0.0),
     m_bDoMinimize(FALSE)
     { }
  virtual ~SmCSIntersectENFO() {}

  // NewtonRaphson Evalaluate callback function
  virtual SmStatus Evaluate(const SmTArray<double> & crX,             // in : x of Ax=F
                            SmTArray<double>       & rF,              // out: F of Ax=F function values,
                            SmMatrix               * pOptJacobian,    // out: Partial derivatives of the functions.
                            SmBoolean              & rbFoundAnswer);  // out: Not always used, when used
                                                                      //      TRUE = converged (F members are within tolerance of 0.0
                                                                      //      FALSE= Not Used or Not Converged
} ; // end class SmCSIntersectENFO

// ---- verbatim: SmSurface.cpp lines 24501-24705: SmCSIntersectENFO::Evaluate
SmStatus SmCSIntersectENFO::Evaluate
  (const SmTArray<double> & crX,  // in : crX[0] = curve t param
                                  //      crX[1] = surface u param
                                  //      crX[2] = surface v param
   SmTArray<double> & rF,         // out: error vector = surf(u,v) - curve(t)
   SmMatrix * pOptJacobian,       // opt: | -dCX/dt dSX/du dSX/dv  |
                                  //      | -dCY/dt dSY/du dSY/dv  |
                                  //      | -dCZ/dt dSZ/du dSZ/dv  |
   SmBoolean & rbFoundAnswer)     // not currently used
{
  // check input state
  SM_ASSERT(crX.GetSize() == 3);
  SM_ASSERT(rF.GetSize()  == 3);
  if (pOptJacobian)
    {
      SM_ASSERT(pOptJacobian->GetNumRows()    == 3);
      SM_ASSERT(pOptJacobian->GetNumColumns() == 3);
    }

  // count calls
  m_lCount++;

  // init output
  rbFoundAnswer = FALSE;

  // Evaluate CurvePoint
  SmPoint3d sCurvePV[2]; // sCurvePV[0] = m_crCurve 3dPoint
                         // sCurvePV[1] = m_crCurve 3dTangent
  SER(m_crCurve.Evaluate(crX[0],1,TRUE,sCurvePV,TRUE)); // nonZeroTangent values

  // Evaluate SurfacePoint
  SmPoint3d sSurfacePoint;
  SmVector3d sDU, sDV;
  SmPoint2d sUV(crX[1],crX[2]);
  SER(m_crSurface.Evaluate1stDerivatives(sUV,TRUE,TRUE,sSurfacePoint,sDU,sDV)); // nonZeroTangent values

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
  // draw CurvePoint(red) and SurfacePoint(yellow)
  if (bDebugMe)
    {
      sm_GraphicsLoop();
      smgfx_SetLook(1,2, 0,1,0); m_crCurve.DrawAt(crX[0],2); sm_GraphicsLoop();
      smgfx_SetLook(1,2, 1,1,0); m_crSurface.DrawAt(sUV,2); sm_GraphicsLoop();
      sm_GraphicsLoop();
    }
#endif

  // Fail at singular Curve and Surface Points - zero tangent lengths
  if (   sDU.LengthSquared() < SM_EFF_ZERO_SQ
      || sDV.LengthSquared() < SM_EFF_ZERO_SQ
      || sCurvePV[1].LengthSquared() < SM_EFF_ZERO_SQ)
    {
      return SM_ERR;
    }

  // get Gap Vector = S - C
  SmVector3d sVecSMinusC = sSurfacePoint - sCurvePV[0];

#ifdef SM_DEBUG_CODE
  if (bDebugMe)
    {
      sm_GraphicsLoop();
      smgfx_SetLook(2,4, 1,0,0); sVecSMinusC.Draw(&sCurvePV[0]); sm_GraphicsLoop();
      sm_GraphicsLoop();
    }
#endif

  // Intersection simply tries to minimize the following three functions
  // fun[0] = (S(u,v) - C(t)).x
  // fun[1] = (S(u,v) - C(t)).y
  // fun[2] = (S(u,v) - C(t)).z
  //
  // It produces the following Jacobian
  //  | -dC/dt.x     dS/du.x      dS/dv.x  |
  //  | -dC/dt.y     dS/du.y      dS/dv.y  |
  //  | -dC/dt.z     dS/du.z      dS/dv.z  |

  if (pOptJacobian)
    {
      (*pOptJacobian)[0][0] = -sCurvePV[1].x;
      (*pOptJacobian)[0][1] =  sDU.x;
      (*pOptJacobian)[0][2] =  sDV.x;

      (*pOptJacobian)[1][0] = -sCurvePV[1].y;
      (*pOptJacobian)[1][1] =  sDU.y;
      (*pOptJacobian)[1][2] =  sDV.y;

      (*pOptJacobian)[2][0] = -sCurvePV[1].z;
      (*pOptJacobian)[2][1] =  sDU.z;
      (*pOptJacobian)[2][2] =  sDV.z;
    }

  // Compute function values
  rF[0] = sVecSMinusC.x;
  rF[1] = sVecSMinusC.y;
  rF[2] = sVecSMinusC.z;

  // Check for solver not converging.
  // If we've iterated too many times, and it looks like a tangency situation.

  // Some parameters for the check:

  // How many times before we check.
  // Note, this should maybe higher, because it will reach 10 while the
  // caller's iteration count is only 4.
  // However, 25 causes problems in my_shell_demo 10.
  ULONG  lIterLim = 10;

  // How close to tangent to prompt switching.
  // Note, 5 degrees is too big [090408]; experience suggests that
  // this should maybe even be smaller than 1 degree. [bd]
  // Use 0.5.  [B234: limit about 0.75 for this]
  double dTangentTolDeg = 0.5;

  double dScaledZero = SM_EFF_ZERO * (1.0 + sSurfacePoint.GetMaxDimension()) ;

  if (   m_bDoMinimize
      || (   m_lCount > lIterLim
          && sVecSMinusC.LengthSquared() > m_dLastSum /* + dScaledZero */))
    {
      // if we are near a tangent point
      SmPoint3d  sCrvPVV[3];
      SmVector3d sNorm;
      SER(m_crCurve.Evaluate(crX[0],2,TRUE,sCrvPVV,TRUE));  // nonZeroTangent values
      SER(m_crSurface.EvaluateNormal(sUV,TRUE,TRUE,sNorm));
      if(sNorm.IsPerpendicularTo(sCrvPVV[1], dTangentTolDeg ))
        {
          // Slow convergence problem.  Switch to a normalize operation.
          //  this next section should be exactly the same as SmCSNormalizeENFO::Evaluate
          m_bDoMinimize = TRUE;
          SmVector3d sDUV, sDUU, sDVV;
          SER(m_crSurface.Evaluate2ndDerivatives(sUV,TRUE,TRUE,sSurfacePoint,
              sDU,sDV,sDUV,sDUU,sDVV));

          sVecSMinusC = sSurfacePoint - sCrvPVV[0];

          // Try a little trick here if the curve/surface gap distance
          // gets very small.  Translate surfacePoint by its
          // normal vector in the appropriate direction. This
          // preserves angles while increasing the gap size
          if (sVecSMinusC.LengthSquared() < 1000.0 * dScaledZero)
            {

              // recalculate gap vector with offset SurfacePoint
              if (sNorm.Dot(sVecSMinusC) >= 0.0)
                {
                  sVecSMinusC = sSurfacePoint+sNorm - sCrvPVV[0];
                }
              else
                {
                  sVecSMinusC = sSurfacePoint-sNorm - sCrvPVV[0];
                }
            } // end small gap check

          // Normalization, Minimization, Maximization
          // Normalizaiton simply tries to minimize the following three functions
          // fun[0] = dt*(S-C)
          // fun[1] = du*(S-C)
          // fun[2] = dv*(S-C)
          //
          // It produces the following Jacobian
          //  | dtt*(S-C) - dt*dt     dt*du                dt*dv                |
          //  | -du*dt                duu*(S-C) + du*du    duv*(S-C) + dv*du    |
          //  | -dv*dt                duv*(S-C) + dv*du    dvv*(S-C) + dv*dv    |

         if (pOptJacobian)
            {
              (*pOptJacobian)[0][0] =  sCrvPVV[2].Dot(sVecSMinusC) - sCrvPVV[1].Dot(sCrvPVV[1]);
              (*pOptJacobian)[0][1] =  sCrvPVV[1].Dot(sDU);
              (*pOptJacobian)[0][2] =  sCrvPVV[1].Dot(sDV);

              (*pOptJacobian)[1][0] = -sDU.Dot(sCrvPVV[1]);
              (*pOptJacobian)[1][1] =  sDUU.Dot(sVecSMinusC) + sDU.Dot(sDU);
              (*pOptJacobian)[1][2] =  sDUV.Dot(sVecSMinusC) + sDU.Dot(sDV);

              (*pOptJacobian)[2][0] = -sDV.Dot(sCrvPVV[1]);
              (*pOptJacobian)[2][1] =  (*pOptJacobian)[1][2];
              (*pOptJacobian)[2][2] =  sDVV.Dot(sVecSMinusC) + sDV.Dot(sDV);
            }

          // Compute function values
          rF[0] = sCrvPVV[1].Dot(sVecSMinusC);
          rF[1] = sDU.Dot(sVecSMinusC);
          rF[2] = sDV.Dot(sVecSMinusC);
        }
    }

  if (m_lCount > 9)
    {
      m_dLastSum = sVecSMinusC.LengthSquared();
    }

#ifdef SM_DEBUG_CODE
  if (bDebugMe)
    {
      if (pOptJacobian) pOptJacobian->Dump();
      smos_WriteBuffer(_T(" X --- \n")); crX.Dump();
      smos_WriteBuffer(_T(" F --- \n")); rF.Dump();
    }
#endif

  return SM_SUCCESS;

} // end SmCSIntersectENFO::Evaluate

// ---- verbatim: SmSurface.cpp lines 25190-25229: class SmCSNormalizeENFO
class SmCSNormalizeENFO : public SmEvalNFunctionsObject
{
protected:
    const SmCurve        & m_crCurve;             // Target Curve
    const SmSurface      & m_crSurface;           // Target Surface
    SmSolverOperationType  m_eSolverOperation;    // SM_SO_3D_SIGNED_DIRECTED_MINIMIZE
                                                  // ALL_OTHERS
    double                 m_dDistanceTolerance;  // not currently used
    const SmVector3d     * m_cpOptVectors;        // When SM_SO_3D_SIGNED_DIRECTED_MINIMIZE
                                                  //  m_cpOptVectors[0] = direction to minimize
                                                  // else - not used
    double                 m_dSquaredTolerance;   // not currently used
public:

    // constructor, destructor
    SmCSNormalizeENFO
      (const SmCurve        & crCurve,
       const SmSurface      & crSurface,
       SmSolverOperationType  eSolverOperation,
       double                 dDistanceTolerance,
       const SmVector3d     * cpOptVectors,
       double                 dSquaredTolerance)
     : m_crCurve(crCurve),
       m_crSurface(crSurface),
       m_eSolverOperation(eSolverOperation),
       m_dDistanceTolerance(dDistanceTolerance),
       m_cpOptVectors(cpOptVectors),
       m_dSquaredTolerance(dSquaredTolerance)
       { }

    virtual ~SmCSNormalizeENFO() {}

    // NewtonRaphson used evaluate callback
    virtual SmStatus Evaluate(const SmTArray<double> & crX,             // in : x of Ax=F
                              SmTArray<double>       & rF,              // out: F of Ax=F function values,
                              SmMatrix               * pOptJacobian,    // out: Partial derivatives of the functions.
                              SmBoolean              & rbFoundAnswer);  // out: Not always used, when used
                                                                        //      TRUE = converged (F members are within tolerance of 0.0
                                                                        //      FALSE= Not Used or Not Converged
} ; // end class SmCSNormalizeENFO

// ---- verbatim: SmSurface.cpp lines 25275-25526: SmCSNormalizeENFO::Evaluate
SmStatus SmCSNormalizeENFO::Evaluate
  (const SmTArray<double> & crX,           // in : x of Ax=F
   SmTArray<double>       & rF,            // out: F of Ax=F function values,
   SmMatrix               * pOptJacobian,  // out: Partial derivatives of the functions.
   SmBoolean              & rbFoundAnswer) // out: TRUE =
                                           //      FALSE=
{
  ULONG lCount =  (m_eSolverOperation == SM_SO_3D_SIGNED_DIRECTED_MINIMIZE)
                 ? 4
                 : 3 ;

  SM_ASSERT(crX.GetSize() == lCount);
  SM_ASSERT(rF.GetSize()  == lCount);
  if (pOptJacobian)
    {
      SM_ASSERT(pOptJacobian->GetNumRows()    == lCount);
      SM_ASSERT(pOptJacobian->GetNumColumns() == lCount);
    }

  // init output
  rbFoundAnswer = FALSE;

  // evaluate Curve Point and derivs
  SmPoint3d sCrvPVV[3];
  SER(m_crCurve.Evaluate(crX[0],2,TRUE,sCrvPVV,TRUE));  // nonZeroTangent values

  // evaluate Surface Point and derivs
  SmPoint3d sS;
  SmVector3d sSu, sSv, sSuu, sSuv, sSvv;
  SmPoint2d sUV(crX[1],crX[2]);
  SER(m_crSurface.Evaluate2ndDerivatives(sUV,TRUE,TRUE,sS,
      sSu,sSv,sSuv,sSuu,sSvv));

  // Naming:
  SmPoint3d &sC   = sCrvPVV[0];
  SmPoint3d &sCt  = sCrvPVV[1];
  SmPoint3d &sCtt = sCrvPVV[2];

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE ;
  // draw CurvePoint(green), SurfacePoint(Yellow)
  if (bDebugMe)
    {
      SmEdge *pEdge = (SmEdge *)m_crCurve.GetEdge() ;
      SmFace *pFace = (SmFace *)m_crSurface.GetFace() ;
      SmBrep *pBrep1 = pEdge ? pEdge->GetBrep() : NULL ;
      SmBrep *pBrep2 = pFace ? pFace->GetBrep() : NULL ;

      smgfx_Erase();
      smgfx_SetLook(1,2, 0,0,1) ; if(pBrep1) pBrep1->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,1,0) ; if(pBrep2) pBrep2->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 0,1,1) ; m_crCurve.Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 1,1,0) ; m_crCurve.DrawCurvature() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 1,0,0) ; m_crCurve.DrawParams() ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 1,1,0) ; m_crSurface.DrawUV() ; sm_GraphicsLoop() ;
      smgfx_SetLook(5,6, 1,0,0) ; m_crCurve.DrawAt(crX[0],2); sm_GraphicsLoop();
      smgfx_SetLook(7,8, 0,1,0) ; m_crSurface.DrawAt(sUV,2); sm_GraphicsLoop() ;
      sm_GraphicsLoop();
    }
#endif // SM_DEBUG_CODE

  // Fail at singular curve or surface points - zero length tangents
  if (   sCt.LengthSquared() < SM_EFF_ZERO_SQ
      || sSu.LengthSquared() < SM_EFF_ZERO_SQ
      || sSv.LengthSquared() < SM_EFF_ZERO_SQ)
    {
      return SM_ERR;
    }

  // get gap vector
  SmVector3d sSMinusC = sS - sC;

  double dScaledZero = SM_EFF_ZERO * 1000.0 * ( 1.0 + sS.GetMaxDimension());

  // Try a little trick here if the curve/surface gap distance
  // gets very small.  Translate surfacePoint by its
  // normal vector in the appropriate direction. This
  // preserves angles while increasing the gap size
  if (sSMinusC.LengthSquared() < dScaledZero)
    {
      SmVector3d sNorm;

      // get offset direction as SurfNormal or given direction
      if (m_eSolverOperation == SM_SO_3D_SIGNED_DIRECTED_MINIMIZE)
        {
          sNorm = m_cpOptVectors[0];
        }
      else
        {
          SER(m_crSurface.EvaluateNormal(sUV,TRUE,TRUE,sNorm));
        }

      // recalculate gap vector with offset SurfacePoint
      if (sNorm.Dot(sSMinusC) >= 0.0)
        {
          sSMinusC = sS+sNorm - sC;
        }
      else
        {
          sSMinusC = sS-sNorm - sC;
        }
    } // end small gap check

#ifdef SM_DEBUG_CODE
  // draw new gap vector(red)
  if (bDebugMe)
    {
      sm_GraphicsLoop();
      smgfx_SetLook(2,4, 1,0,0); sSMinusC.Draw(&sC); sm_GraphicsLoop() ;
      sm_GraphicsLoop();
    }
#endif

  // Unitize Ct and get its derivative.
  SmVector3d sCt_Unit( sCt );
  sCt_Unit.Unitize();
  SmVector3d sCt_Unit_t = sCt.UnitizedDerivative( sCtt );

  // branch on SolverOperation
  if (m_eSolverOperation == SM_SO_3D_SIGNED_DIRECTED_MINIMIZE)
    {
      // 3D Signed Directed Minimize evalautes the four functions
      // fun[0] = Ct*(N)               // Make curve tangent lie in surface tangent plane - N is unitized surface Normal
      // fun[1] = S.x - C.x - w * V.x  // Intersection of line from curve to surface along V
      // fun[2] = S.y - C.y - w * V.y
      // fun[3] = S.z - C.z - w * V.z
      // and associated Jacobian:
      //  |  Ctt * N    Ct * dNdu   Ct * dNdv    0.0    |
      //  |  -Ct.x      Su.x        Sv.x        -V.x    |
      //  |  -Ct.y      Su.y        Sv.y        -V.y    |
      //  |  -Ct.z      Su.z        Sv.z        -V.z    |
      //
      // Amended to use unit Ct and N in the objective functions.

      NER(m_cpOptVectors);

      // Surface Normal, unit, and derivatives
      SmVector3d sCross  = sSu * sSv;
      SmVector3d sCrossu = sSu.DerivativeOfCrossProduct( sSv, sSuu, sSuv );
      SmVector3d sCrossv = sSu.DerivativeOfCrossProduct( sSv, sSuv, sSvv );

      // 'N' will mean 'Unit'.
      SmVector3d sN( sCross );
      sN.Unitize();
      SmVector3d sNu = sCross.UnitizedDerivative( sCrossu );
      SmVector3d sNv = sCross.UnitizedDerivative( sCrossv );

      if (pOptJacobian)
        {
          (*pOptJacobian)[0][0] =  sCt_Unit_t.Dot( sN  );
          (*pOptJacobian)[0][1] =  sCt_Unit  .Dot( sNu );
          (*pOptJacobian)[0][2] =  sCt_Unit  .Dot( sNv );
          (*pOptJacobian)[0][3] =  0.0;

          (*pOptJacobian)[1][0] = -sCt.x;
          (*pOptJacobian)[1][1] =  sSu.x;
          (*pOptJacobian)[1][2] =  sSv.x;
          (*pOptJacobian)[1][3] = -m_cpOptVectors[0].x;

          (*pOptJacobian)[2][0] = -sCt.y;
          (*pOptJacobian)[2][1] =  sSu.y;
          (*pOptJacobian)[2][2] =  sSv.y;
          (*pOptJacobian)[2][3] = -m_cpOptVectors[0].y;

          (*pOptJacobian)[3][0] = -sCt.z;
          (*pOptJacobian)[3][1] =  sSu.z;
          (*pOptJacobian)[3][2] =  sSv.z;
          (*pOptJacobian)[3][3] = -m_cpOptVectors[0].z;
        }

      // Compute function values
      SmVector3d sVec = sS - sC - crX[3] * m_cpOptVectors[0];

      rF[0] = sCt_Unit.Dot(sN);
      rF[1] = sVec.x;
      rF[2] = sVec.y;
      rF[3] = sVec.z;
    }

  else
    { // Normalization, Minimization, Maximization
      // Normalizaiton simply tries to minimize the following three functions
      // fun[0] = Ct*(S-C)
      // fun[1] = Su*(S-C)
      // fun[2] = Sv*(S-C)
      //
      // It produces the following Jacobian
      //  | Ctt*(S-C) - Ct*Ct     Ct*Su                Ct*Sv                |
      //  | -Su*Ct                Suu*(S-C) + Su*Su    Suv*(S-C) + Sv*Su    |
      //  | -Sv*Ct                Suv*(S-C) + Sv*Su    Svv*(S-C) + Sv*Sv    |
      //
      // Amended to use unitized Ct, Su, and Sv in the objective functions.

      // Unitize sSu and sSv and get their derivatives.
      SmVector3d sSu_Unit( sSu );
      sSu_Unit.Unitize();
      SmVector3d sSu_Unit_u = sSu.UnitizedDerivative( sSuu );
      SmVector3d sSu_Unit_v = sSu.UnitizedDerivative( sSuv );
      SmVector3d sSv_Unit( sSv );
      sSv_Unit.Unitize();
      SmVector3d sSv_Unit_u = sSv.UnitizedDerivative( sSuv );  // Note: not the same as sSu_Unit_v.
      SmVector3d sSv_Unit_v = sSv.UnitizedDerivative( sSvv );

      if (pOptJacobian)
        {
          (*pOptJacobian)[0][0] =  sCt_Unit_t.Dot( sSMinusC ) - sCt_Unit.Dot( sCt );
          (*pOptJacobian)[0][1] =  sCt_Unit  .Dot( sSu );
          (*pOptJacobian)[0][2] =  sCt_Unit  .Dot( sSv );

          (*pOptJacobian)[1][0] = -sSu_Unit  .Dot( sCt );
          (*pOptJacobian)[1][1] =  sSu_Unit_u.Dot( sSMinusC ) + sSu_Unit.Dot( sSu );
          (*pOptJacobian)[1][2] =  sSu_Unit_v.Dot( sSMinusC ) + sSu_Unit.Dot( sSv );

          (*pOptJacobian)[2][0] = -sSv_Unit  .Dot( sCt );
          (*pOptJacobian)[2][1] =  sSv_Unit_u.Dot( sSMinusC ) + sSv_Unit.Dot( sSu ); 
          (*pOptJacobian)[2][2] =  sSv_Unit_v.Dot( sSMinusC ) + sSv_Unit.Dot( sSv );
        }

      // Compute function values
      rF[0] = sCt_Unit.Dot( sSMinusC );
      rF[1] = sSu_Unit.Dot( sSMinusC );
      rF[2] = sSv_Unit.Dot( sSMinusC );

    }

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe2 = FALSE;
  if (bDebugMe2)
    {
      if (pOptJacobian && m_eSolverOperation == SM_SO_3D_SIGNED_DIRECTED_MINIMIZE)
        {
          pOptJacobian->Dump();
          smos_WriteBuffer(_T(" X --- \n")); crX.Dump();
          smos_WriteBuffer(_T(" F --- \n")); rF.Dump();
          SmMatrix sTmpJ(4,4);
          SmTArray<double> sTmpX(crX);
          SER(ComputeJacobian(sTmpX,rF,sTmpJ));
          sTmpJ.Dump();
        }
      else
        {
          if (pOptJacobian) pOptJacobian->Dump();
          smos_WriteBuffer(_T(" X --- \n")); crX.Dump();
          smos_WriteBuffer(_T(" F --- \n")); rF.Dump();
        }
    }
#endif

  return SM_SUCCESS;

} // end SmCSNormalizeENFO::Evaluate

// ---- verbatim: SmSurface.cpp lines 26582-26615: class SmSSPIntersectENFO
class SmSSPIntersectENFO : public SmEvalNFunctionsObject
{
protected:
  const SmSurface & m_crSurface;         // 1st target surface
  const SmSurface & m_crOtherSurface;    // 2nd target surface
  SmBoolean         m_bSteepestDescent;  // TRUE = each iteration let PlaneThroughPoint = avg(surf1Point,surf2Point)
                                         //                           PlaneNormal = cross(surf1Normal,surf2Normal)
                                         // FALSE= each iteration let Plane = m_cpPlane in surf/surf/Plane intersection
  const SmPlane   * m_cpPlane;           // 3rd target surface - a plane
  double            m_dSquaredTolerance; // max deviation allowed for a solution
public:

  // constructor, destructor
  SmSSPIntersectENFO(const SmSurface & crSurface,
                     const SmSurface & crOtherSurface,
                     SmBoolean         bSteepestDescent,
                     const SmPlane   * cpOptPlane,
                     double            dSquaredTolerance)
    : m_crSurface(crSurface),
      m_crOtherSurface(crOtherSurface),
      m_bSteepestDescent(bSteepestDescent),
      m_cpPlane(cpOptPlane),
      m_dSquaredTolerance(dSquaredTolerance)
    { }
  virtual ~SmSSPIntersectENFO() { }

  // NewtonRaphson Equation Set Evaluation Callback Function
  virtual SmStatus Evaluate(const SmTArray<double> & crX,             // in : x of Ax=F
                            SmTArray<double>       & rF,              // out: F of Ax=F function values,
                            SmMatrix               * pOptJacobian,    // out: Partial derivatives of the functions.
                            SmBoolean              & rbFoundAnswer);  // out: Not always used, when used
                                                                      //      TRUE = converged (F members are within tolerance of 0.0
                                                                      //      FALSE= Not Used or Not Converged
} ; // end class SmSSPIntersectENFO

// ---- verbatim: SmSurface.cpp lines 26648-26826: SmSSPIntersectENFO::Evaluate
SmStatus SmSSPIntersectENFO::Evaluate
  (const SmTArray<double> & crX,           // in : x of Ax=F
   SmTArray<double>       & rF,            // out: F of Ax=F function values,
   SmMatrix               * pOptJacobian,  // out: Partial derivatives of the functions.
   SmBoolean              & rbFoundAnswer) // out: TRUE = current crX vector satisfies equations to tolerances
                                           //      FALSE= current crX not good enough - new one evaluated but not tested
{
  // check input
  SM_ASSERT(crX.GetSize() == 4);
  SM_ASSERT(rF.GetSize()  == 4);
  if (pOptJacobian)
    {
      SM_ASSERT(pOptJacobian->GetNumRows()    == 4);
      SM_ASSERT(pOptJacobian->GetNumColumns() == 4);
    }

  // init output
  rbFoundAnswer = FALSE;

  // locals for surf1, surf2, and plane evaluations
  SmPoint3d sF, sG, sH;
  SmVector3d sDUF, sDVF, sDUG, sDVG, sDUH, sDVH;;
  SmPoint2d sUV(crX[0],crX[1]), sUV2(crX[2],crX[3]);
  SmPlane *pPlane = NULL ;
  SmObjDelete sClean1 ;

  // get surface 3d points - with 1st derivatives
  SER(m_crSurface.     Evaluate1stDerivatives(sUV, TRUE,TRUE,sF,sDUF,sDVF));
  SER(m_crOtherSurface.Evaluate1stDerivatives(sUV2,TRUE,TRUE,sG,sDUG,sDVG));

  // pick plane to use in this evaluation and get Plane derivatives

  // when doing steepestDescent - plane = plane(surf1Point, surf2Point)
  // else                         plane = m_cpPlane
  if (m_bSteepestDescent)
    {
      // If we are doing a steepest descent then we just recompute a plane

      // get planeThroughPoint
      sH = (sF+sG) / 2.0;  // Put plane between points of surfaces

      // get planeNormal
      SmVector3d sNorm1 = sDUF * sDVF;
      SmVector3d sNorm2 = sDUG * sDVG;
      if (sNorm1.LengthSquared() < SM_EFF_ZERO) { SER(m_crSurface.EvaluateNormal(sUV,TRUE,TRUE,sNorm1)); }
      if (sNorm2.LengthSquared() < SM_EFF_ZERO) { SER(m_crOtherSurface.EvaluateNormal(sUV2,TRUE,TRUE,sNorm2)); }
      SmVector3d sPlaneNorm = sNorm1 * sNorm2;
      if (sPlaneNorm.LengthSquared() < SM_EFF_ZERO_SQ)
        { return SM_ERR;
        }
      sPlaneNorm.Unitize();

      // instance a temporary plane
      pPlane = new (*m_crSurface.GetContext()) SmPlane(sH,sPlaneNorm);
      sClean1.SetObj(pPlane) ;

    }
  else // let Plane = m_cpPlane
    {
      NER(m_cpPlane);
      pPlane = (SmPlane *)m_cpPlane ;
    }

  // evaluate Plane derivatives
  SER(pPlane->Evaluate1stDerivatives(SmPoint2d(0,0),TRUE,TRUE,sH,sDUH,sDVH));

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
  // draw Surf1(blue), Surf2(green), plane(cyan), Surf1Point(red), Surf2Point(magenta), PlaneOrig(yellow)
  if (bDebugMe)
    {
      smgfx_Erase();
      smgfx_SetLook(1,2, 0,0,1); m_crSurface.DrawUV(); sm_GraphicsLoop();
      smgfx_SetLook(1,2, 0,1,0); m_crOtherSurface.DrawUV(); sm_GraphicsLoop();
      smgfx_SetLook(1,2, 0,1,1); pPlane->DrawUV(); sm_GraphicsLoop();

      smgfx_SetLook(2,4, 1,0,0); m_crSurface.DrawAt(sUV,1); sm_GraphicsLoop();
      smgfx_SetLook(2,4, 1,0,1); m_crOtherSurface.DrawAt(sUV2,1); sm_GraphicsLoop();
      smgfx_SetLook(2,4, 1,1,0); pPlane->DrawAt(SmPoint2d(0,0),1);
      sm_GraphicsLoop();
    }
#endif

  // Utilize plane equation aX + bY + cZ + d = 0 in solver
  SmVector3d sABC = sDUH * sDVH;
  SER(sABC.Unitize());
  double dD = - sABC.Dot(sH);

  double sDUFLenSq = sDUF.LengthSquared();
  double sDVFLenSq = sDVF.LengthSquared();

  double sDUGLenSq = sDUG.LengthSquared();
  double sDVGLenSq = sDVG.LengthSquared();

  // if we have a zero length tangent then we must fail
  if (   sDUFLenSq < SM_EFF_ZERO_SQ || sDVFLenSq < SM_EFF_ZERO_SQ
      || sDUGLenSq < SM_EFF_ZERO_SQ || sDVGLenSq < SM_EFF_ZERO_SQ)
    {
      return SM_ERR;
    }

  // get gap vector
  SmVector3d sDiff1 = sF - sG;

  // evaluate F[3]
  rF[3] = sABC.Dot(sF) + dD + sABC.Dot(sG) + dD;

  // exit criteria - Surface points on plane and close to each other
  if (   sDiff1.LengthSquared() < SM_EFF_ZERO_SQ
      && rF[3] < SM_EFF_ZERO)
    {
      rbFoundAnswer = TRUE;
      return SM_SUCCESS;
    }

#ifdef SM_DEBUG_CODE
  if (bDebugMe)
    {
      smgfx_SetLook(1,2, 1,0,0); sDiff1.Draw(&sG); sm_GraphicsLoop();
      sm_GraphicsLoop();
    }
#endif

  // Intersection simply tries to minimize the following equations.
  // It is essentially intersecting the three surfaces by intersecting
  // tangent planes.
  // fun[0] = (f-g) . X     - X component of vector f-g
  // fun[1] = (f-g) . Y     - Y component of vector f-g
  // fun[2] = (f-g) . Z     - Z component of vector f-g
  // fun[3] =   A * f.x + B * f.y + C * f.z + D  - 2 Plane equations
  //          + A * g.x + B * g.y + C * g.z + D
  //
  // Where f is surface1, g is surface2 and h is surface3
  //
  // It produces the following Jacobian
  //  | duf.X      dvf.X       -dug.X       -dvg.X  |
  //  | duf.Y      dvf.Y       -dug.Y       -dvg.Y  |
  //  | duf.Z      dvf.Z       -dug.Z       -dvg.Z  |
  //  | sABC.duf   sABC.dvf  sABC.dug     sABC.dvg  |

  // GWC_NEEDS_WORK jacobian_is_degenerate_when_surfacePointNormals_are_parallel GWC_LINE ;
  if (pOptJacobian)
    {
      (*pOptJacobian)[0][0] = sDUF.x;
      (*pOptJacobian)[0][1] = sDVF.x;
      (*pOptJacobian)[0][2] = - sDUG.x;
      (*pOptJacobian)[0][3] = - sDVG.x;
      (*pOptJacobian)[1][0] = sDUF.y;
      (*pOptJacobian)[1][1] = sDVF.y;
      (*pOptJacobian)[1][2] = - sDUG.y;
      (*pOptJacobian)[1][3] = - sDVG.y;
      (*pOptJacobian)[2][0] = sDUF.z;
      (*pOptJacobian)[2][1] = sDVF.z;
      (*pOptJacobian)[2][2] = - sDUG.z;
      (*pOptJacobian)[2][3] = - sDVG.z;
      (*pOptJacobian)[3][0] = sABC.Dot(sDUF);
      (*pOptJacobian)[3][1] = sABC.Dot(sDVF);
      (*pOptJacobian)[3][2] = sABC.Dot(sDUG);
      (*pOptJacobian)[3][3] = sABC.Dot(sDVG);
    }

  // Compute function values
  rF[0] = sDiff1.x;
  rF[1] = sDiff1.y;
  rF[2] = sDiff1.z;
  // rF[3] - computed above

#ifdef SM_DEBUG_CODE
  if (bDebugMe)
    {
      if (pOptJacobian) pOptJacobian->Dump();
      smos_WriteBuffer(_T(" X --- \n")); SM_DUMP_TARRAY( crX );
      smos_WriteBuffer(_T(" F --- \n")); SM_DUMP_TARRAY( rF  );
    }
#endif

  return SM_SUCCESS;

} // end SmSSPIntersectENFO::Evaluate

// ---- verbatim: SmSurface.cpp lines 26944-26972: class SmSSINormalizeENFO
class SmSSINormalizeENFO : public SmEvalNFunctionsObject
{
protected:
  const SmSurface & m_crSurface;          // 1st surface to intersect
  const SmSurface & m_crOtherSurface;     // 2nd surface to intersect
  double            m_dSquaredTolerance;  //
  SmVector3d        m_vPlaneNormal;       // A, B, C of plane = Ax + By + Cz + D = 0
  double            m_dPlaneD;            // D of plane definiton
public:

  // constructor, destructor
  SmSSINormalizeENFO(const SmSurface & crSurface,
      const SmSurface & crOtherSurface,
      double dSquaredTolerance,
      const SmVector3d & crPlaneNormal,
      double dPlaneD)
    : m_crSurface(crSurface), m_crOtherSurface(crOtherSurface),
      m_dSquaredTolerance(dSquaredTolerance), m_vPlaneNormal(crPlaneNormal),
      m_dPlaneD(dPlaneD) {}
  virtual ~SmSSINormalizeENFO() {}

  // method
  virtual SmStatus Evaluate(const SmTArray<double> & crX,             // in : x of Ax=F
                            SmTArray<double>       & rF,              // out: F of Ax=F function values,
                            SmMatrix               * pOptJacobian,    // out: Partial derivatives of the functions.
                            SmBoolean              & rbFoundAnswer);  // out: Not always used, when used
                                                                      //      TRUE = converged (F members are within tolerance of 0.0
                                                                      //      FALSE= Not Used or Not Converged
} ; // end class SmSSINormalizeENFO

// ---- verbatim: SmSurface.cpp lines 27010-27256: SmSSINormalizeENFO::Evaluate
SmStatus SmSSINormalizeENFO::Evaluate
  (const SmTArray<double> & crX,           // in : x of Ax=F
   SmTArray<double>       & rF,            // out: F of Ax=F function values,
   SmMatrix               * pOptJacobian,  // out: Partial derivatives of the functions.
   SmBoolean              & rbFoundAnswer) // out: Always set to False in this function
{
  // check input
  SM_ASSERT(crX.GetSize() == 4);
  SM_ASSERT(rF.GetSize()  == 4);
  if (pOptJacobian)
    {
      SM_ASSERT(pOptJacobian->GetNumRows()   == 4);
      SM_ASSERT(pOptJacobian->GetNumColumns() == 4);
    }

  // init output
  rbFoundAnswer = FALSE;

  // loclas for surf1, surf2, and plane evaluations
  SmPoint3d  sSurfacePoint,  sSrfPoint2;
  SmVector3d sDU,  sDV,  sDUV,  sDUU,  sDVV;
  SmVector3d sDU2, sDV2, sDUV2, sDUU2, sDVV2;
  SmPoint2d  sUV(crX[0],crX[1]), sUV2(crX[2],crX[3]);
  SmVector3d sPlaneNorm(m_vPlaneNormal);

  // get surface 3d points - with 2nd derivatives
  SER(m_crSurface.Evaluate2ndDerivatives     (sUV,TRUE,TRUE,sSurfacePoint,
                                              sDU,sDV,sDUV,sDUU,sDVV));
  SER(m_crOtherSurface.Evaluate2ndDerivatives(sUV2,TRUE,TRUE,sSrfPoint2,
                                              sDU2,sDV2,sDUV2,sDUU2,sDVV2));

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
  // draw Surf1(blue), Surf2(green), plane(cyan), Surf1Point(red), Surf2Point(magenta), PlaneOrig(yellow)
  if (bDebugMe)
    {
      smgfx_Erase();
      smgfx_SetLook(1,2, 0,0,1); m_crSurface.DrawUV(); sm_GraphicsLoop();
      smgfx_SetLook(1,2, 0,1,0); m_crOtherSurface.DrawUV(); sm_GraphicsLoop();
      smgfx_SetLook(2,4, 0,1,1); sPlaneNorm.Draw(&sSurfacePoint); sm_GraphicsLoop();
      smgfx_SetLook(2,4, 0,1,1); sPlaneNorm.Draw(&sSrfPoint2); sm_GraphicsLoop();

      smgfx_SetLook(4,6, 1,0,0); m_crSurface.DrawAt(sUV,1); sm_GraphicsLoop();
      smgfx_SetLook(4,6, 1,0,1); m_crOtherSurface.DrawAt(sUV2,1); sm_GraphicsLoop();
      sm_GraphicsLoop();
    }
#endif

  // fail on surface singularities
  double sDULenSq  = sDU.LengthSquared();
  double sDVLenSq  = sDV.LengthSquared();
  double sDULenSq2 = sDU2.LengthSquared();
  double sDVLenSq2 = sDV2.LengthSquared();
  // if we have a zero length tangent then we must fail
  if (   sDULenSq  < SM_EFF_ZERO_SQ || sDVLenSq  < SM_EFF_ZERO_SQ
      || sDULenSq2 < SM_EFF_ZERO_SQ || sDVLenSq2 < SM_EFF_ZERO_SQ)
    {
      return SM_ERR;
    }

  // when the gap is small
  SmVector3d sGap = sSrfPoint2 - sSurfacePoint;
  double     dTol = SM_EFF_ZERO * 1000.0 * ( 1.0 + sSurfacePoint.GetMaxDimension());
  if (sGap.LengthSquared() < dTol)
    {
      // We have a hard time solving when gaps get really
      // close so we can translate one surfacePoint by its normal
      // and solve a little better
      SmVector3d sNorm, sNorm2;
      SER(m_crSurface.EvaluateNormal(sUV,TRUE,TRUE,sNorm));
      SER(m_crOtherSurface.EvaluateNormal(sUV2,TRUE,TRUE,sNorm2));
      SmVector3d sAveNorm ;
      sAveNorm =   (sNorm.Dot(sNorm2) < 0.0)
                 ? (sNorm - sNorm2)/2.0
                 : (sNorm + sNorm2)/2.0 ;

      // project AvdNorm to Plane
      SmVector3d sMoveInPlane = sPlaneNorm * sAveNorm * sPlaneNorm;
      if (sNorm.Dot(sGap) > 0.0) { sSurfacePoint = sSurfacePoint - sMoveInPlane;
                                   sGap      = sSrfPoint2 - sSurfacePoint;
                                 }
      else                       { sSurfacePoint = sSurfacePoint + sMoveInPlane;
                                   sGap      = sSrfPoint2 - sSurfacePoint;
                                 }
//        rbFoundAnswer = TRUE;
//        return SM_SUCCESS;
    } // working with small gap checks

#ifdef SM_DEBUG_CODE
  if (bDebugMe)
    {
      smgfx_SetLook(1,2, 1,0,0); sGap.Draw(&sSurfacePoint); sm_GraphicsLoop();
      sm_GraphicsLoop() ;
    }
#endif

  // Seek solution where S = S2 on given plane.
  //  strategy: make Gap perpendicular to Plane (parallel to plane normal)
  //            make S lie on plane
  //            make S2 lie on plane.
  //            This is only TRUE when S = S2 on given plane.
  //  The 1st two equations make Gap perp to plane by making it
  //        perp to two vectors in that plane.
  //  The second two equations keep the points in the plane.
  //
  // let
  // S   = sSurfacePoint, S2 = sSrfPoint2,
  // Gap = S2-S
  // N   = du x dv, N2 = du2 x dv2
  // V   = N x dPlaneNorm,  V2 = N2 x dPlaneNorm   (2 vectors in the plane)
  //
  // fun[0] = V * (S2-S)                 // gap is perpendicular to V
  // fun[1] = V2 * (S2-S)                // gap is perpendicular to V2
  // fun[2] = S * dPlaneNorm + dPlaneD   // S is on plane
  // fun[3] = S2 * dPlaneNorm + dPlaneD  // S2 is on plane
  //
  // It produces the following Jacobian
  //  | (dV/du)*(S2-S) - V*du   (dV/dv)(S2-S) - V*dv   V*du2                      V*dv2                     |
  //  | -V2*du                  -V2*dv                 (dV2/du2)*(S2-S) + V2*du2  (dV2/dv2)*(S2-S) + V2*dv2 |
  //  | du*sPlaneNorm           dv*sPlaneNorm          0                          0                         |
  //  | 0                       0                      du2*sPlaneNorm             dv2*sPlaneNorm            |

  // get surface normals
  SmVector3d sN  = sDU * sDV;      // Compute normals
  SmVector3d sN2 = sDU2 * sDV2;

  // unitize surf1 Normal
  SmVector3d sNU = sN;
  if (sNU.LengthSquared() < SM_EFF_ZERO_SQ)
    {
      SER(m_crSurface.EvaluateNormal(sUV,TRUE,TRUE,sNU));
      if (sNU.LengthSquared() < SM_EFF_ZERO_SQ)
        {
          return SM_ERR;
        }
    }
  sNU.Unitize();

  // unitize surf2 Normal
  SmVector3d sN2U = sN2;
  if (sN2U.LengthSquared() < SM_EFF_ZERO_SQ)
    {
      SER(m_crSurface.EvaluateNormal(sUV2,TRUE,TRUE,sN2U));
      if (sN2U.LengthSquared() < SM_EFF_ZERO_SQ)
        {
          return SM_ERR;
        }
    }
  sN2U.Unitize();

  // Get two plane vectors equal to surf/plane intersection directions
  SmVector3d sV  = sNU * sPlaneNorm; // Compute vectors in tangent plane and on plane
  SmVector3d sV2 = sN2U * sPlaneNorm;

  // build the jacobian
  if (pOptJacobian)
    {
      SmVector3d sZeroVec(0.0,0.0,0.0);
      SmVector3d dN_du   = sDU.DerivativeOfCrossProduct(sDV,sDUU,sDUV);
//                 dN_du   = sN.UnitizedDerivative(dN_du);
      SmVector3d dN_dv   = sDU.DerivativeOfCrossProduct(sDV,sDUV,sDVV);
//                 dN_dv   = sN.UnitizedDerivative(dN_dv);
      SmVector3d dN2_du2 = sDU2.DerivativeOfCrossProduct(sDV2,sDUU2,sDUV2);
//                 dN2_du2 = sN2.UnitizedDerivative(dN2_du2);
      SmVector3d dN2_dv2 = sDU2.DerivativeOfCrossProduct(sDV2,sDUV2,sDVV2);
//                 dN2_dv2 = sN2.UnitizedDerivative(dN2_dv2);
      SmVector3d dV_du   = sN.DerivativeOfCrossProduct(sPlaneNorm,dN_du,sZeroVec);
      SmVector3d dV_dv   = sN.DerivativeOfCrossProduct(sPlaneNorm,dN_dv,sZeroVec);
      SmVector3d dV2_du2 = sN2.DerivativeOfCrossProduct(sPlaneNorm,dN2_du2,sZeroVec);
      SmVector3d dV2_dv2 = sN2.DerivativeOfCrossProduct(sPlaneNorm,dN2_dv2,sZeroVec);

      (*pOptJacobian)[0][0] = dV_du.Dot(sGap) - sV.Dot(sDU);
      (*pOptJacobian)[0][1] = dV_dv.Dot(sGap) - sV.Dot(sDV);
      (*pOptJacobian)[0][2] = sV.Dot(sDU2);
      (*pOptJacobian)[0][3] = sV.Dot(sDV2);

      (*pOptJacobian)[1][0] = - sV2.Dot(sDU);
      (*pOptJacobian)[1][1] = - sV2.Dot(sDV);
      (*pOptJacobian)[1][2] = dV2_du2.Dot(sGap) + sV2.Dot(sDU2);
      (*pOptJacobian)[1][3] = dV2_dv2.Dot(sGap) + sV2.Dot(sDV2);

      (*pOptJacobian)[2][0] = sDU.Dot(sPlaneNorm);
      (*pOptJacobian)[2][1] = sDV.Dot(sPlaneNorm);
      (*pOptJacobian)[2][2] = 0.0;
      (*pOptJacobian)[2][3] = 0.0;

      (*pOptJacobian)[3][0] = 0.0;
      (*pOptJacobian)[3][1] = 0.0;
      (*pOptJacobian)[3][2] = sDU2.Dot(sPlaneNorm);
      (*pOptJacobian)[3][3] = sDV2.Dot(sPlaneNorm);
    }

  // Compute function values
  rF[0] = sV.Dot(sGap);
  rF[1] = sV2.Dot(sGap);
  rF[2] = sSurfacePoint.Dot(sPlaneNorm)  + m_dPlaneD;
  rF[3] = sSrfPoint2.Dot(sPlaneNorm) + m_dPlaneD;

  if (pOptJacobian)
    { // GWC:TODO
      //     The above jacobian had some errors in it
      //     that have been corrected but not tested.
      //     Need to come
      //     back and change following function for
      //     2nd derivatives and then try removing this
      //     block all together.

      // Compute the jacobian the hard way - it works
      // The above natural jacobian does not.
      //
      // Pass in the surface domains.
      SmTArray< SmExtent1d > sDomains;
      SmExtent2d sDom( m_crSurface.GetNaturalUVDomain() );
      SmExtent1d sIvl = sDom.GetUInterval();
      sDomains.Add( sIvl );
      sIvl = sDom.GetVInterval();
      sDomains.Add( sIvl );
      sDom = m_crOtherSurface.GetNaturalUVDomain();
      sIvl = sDom.GetUInterval();
      sDomains.Add( sIvl );
      sIvl = sDom.GetVInterval();
      sDomains.Add( sIvl );

      SmTArray<double> sTmpX(crX);
      SER( ComputeJacobian( sTmpX, rF, *pOptJacobian, &sDomains ));
    }

#ifdef SM_DEBUG_CODE
  if (bDebugMe)
    {
      if (pOptJacobian)
        {
          pOptJacobian->Dump();
          SmMatrix sTmpJ(4,4);
          SmTArray<double> sTmpX(crX);
          SER(ComputeJacobian(sTmpX,rF,sTmpJ));
          sTmpJ.Dump();
          smos_WriteBuffer(_T(" X --- \n")); crX.Dump();
          smos_WriteBuffer(_T(" F --- \n")); rF.Dump();
        }
    }
#endif

  return SM_SUCCESS;

} // end SmSSINormalizeENFO::Evaluate

// ---- verbatim: SmSurface.cpp lines 28056-28091: class SmSSNormalizeENFO
class SmSSNormalizeENFO : public SmEvalNFunctionsObject
{
protected:
  const SmSurface      & m_crSurface;          // 1st Surface
  const SmSurface      & m_crOtherSurface;     // 2nd Surface
  SmSolverOperationType  m_eSolverOperation;   // SM_SO_3D_SIGNED_DIRECTED_MINIMIZE
  const SmVector3d     * m_cpOptVectors;       // extra data needed for case
                                               // SM_SO_3D_SIGNED_DIRECTED_MINIMIZE
                                               //   m_cpOptVectors[0] =
  double                 m_dSquaredTolerance;  // NOT USED in this evaluator
public:

  // constructor, destructor
  SmSSNormalizeENFO
    (const SmSurface & crSurface,
     const SmSurface & crOtherSurface,
     SmSolverOperationType eSolverOperation,
     const SmVector3d * cpOptVectors,
     double dSquaredTolerance)
    : m_crSurface(crSurface),
      m_crOtherSurface(crOtherSurface),
      m_eSolverOperation(eSolverOperation),
      m_cpOptVectors(cpOptVectors),
      m_dSquaredTolerance(dSquaredTolerance)
      {}

  virtual ~SmSSNormalizeENFO() {}

  // NewtonRaphson evaluator callback mathod
  virtual SmStatus Evaluate(const SmTArray<double> & crX,             // in : x of Ax=F
                            SmTArray<double>       & rF,              // out: F of Ax=F function values,
                            SmMatrix               * pOptJacobian,    // out: Partial derivatives of the functions.
                            SmBoolean              & rbFoundAnswer);  // out: Not always used, when used
                                                                      //      TRUE = converged (F members are within tolerance of 0.0
                                                                      //      FALSE= Not Used or Not Converged
} ; // end class SmSSNormalizeENFO

// ---- verbatim: SmSurface.cpp lines 28147-28404: SmSSNormalizeENFO::Evaluate
SmStatus SmSSNormalizeENFO::Evaluate
  (const SmTArray<double> & crX,           // in : x of Ax=F
   SmTArray<double>       & rF,            // out: F of Ax=F function values,
   SmMatrix               * pOptJacobian,  // out: Partial derivatives of the functions.
   SmBoolean              & rbFoundAnswer) // out: Always set to False in this function
{
  // check input
  ULONG lCount =   (m_eSolverOperation == SM_SO_3D_SIGNED_DIRECTED_MINIMIZE)
                 ? 5
                 : 4 ;
  SM_ASSERT(crX.GetSize() == lCount);
  SM_ASSERT(rF.GetSize()  == lCount);
  if (pOptJacobian)
    {
      SM_ASSERT(pOptJacobian->GetNumRows()    == lCount);
      SM_ASSERT(pOptJacobian->GetNumColumns() == lCount);
    }

  // input output
  rbFoundAnswer = FALSE;

  // Surf1Point and Surf2Point current solution values
  SmPoint3d sSrfPoint1, sSrfPoint2;
  SmVector3d sS1u, sS1v, sS1uu, sS1uv, sS1vv;
  SmVector3d sS2u, sS2v, sS2uu, sS2uv, sS2vv;
  SmPoint2d sUV1(crX[0],crX[1]);
  SmPoint2d sUV2(crX[2],crX[3]);
  SER(m_crSurface.Evaluate2ndDerivatives     (sUV1,TRUE,TRUE,sSrfPoint1,
                                              sS1u,sS1v,sS1uv,sS1uu,sS1vv));
  SER(m_crOtherSurface.Evaluate2ndDerivatives(sUV2,TRUE,TRUE,sSrfPoint2,
                                              sS2u,sS2v,sS2uv,sS2uu,sS2vv));

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
  // draw Surf1(blue), Surf2(green), Surf1Point(red), Surf2Point(magenta),
  if (bDebugMe)
    {
      smgfx_Erase();
      smgfx_SetLook(1,2, 0,0,1); m_crSurface.DrawUV(); sm_GraphicsLoop();
      smgfx_SetLook(1,2, 0,1,0); m_crOtherSurface.DrawUV(); sm_GraphicsLoop();

      smgfx_SetLook(2,4, 1,0,0); m_crSurface.DrawAt(sUV1,1); sm_GraphicsLoop();
      smgfx_SetLook(2,4, 1,0,1); m_crOtherSurface.DrawAt(sUV2,1); sm_GraphicsLoop();
      sm_GraphicsLoop();
    }
#endif

  // fail on surface singularities
  double sDULenSq1 = sS1u.LengthSquared();
  double sDVLenSq1 = sS1v.LengthSquared();
  double sDULenSq2 = sS2u.LengthSquared();
  double sDVLenSq2 = sS2v.LengthSquared();
  // if we have a zero length tangent then we must fail
  if (   sDULenSq1 < SM_EFF_ZERO_SQ || sDVLenSq1 < SM_EFF_ZERO_SQ
      || sDULenSq2 < SM_EFF_ZERO_SQ || sDVLenSq2 < SM_EFF_ZERO_SQ)
    {
      return SM_ERR;
    }

  // when the gap is small
  SmVector3d sGap = sSrfPoint2 - sSrfPoint1;
  double     dTol = SM_EFF_ZERO * 1000.0 * ( 1.0 + sSrfPoint1.GetMaxDimension());
  if (sGap.LengthSquared() < dTol)
    {
      // We have a hard time solving when gaps get really
      // close so we can translate one surfacePoint by its normal
      // and solve a little better
      SmVector3d sNorm;
      if (m_eSolverOperation == SM_SO_3D_SIGNED_DIRECTED_MINIMIZE)
        {
          sNorm = m_cpOptVectors[0];
        }
      else
        {
          SER(m_crSurface.EvaluateNormal(sUV1,TRUE,TRUE,sNorm));
        }

      sGap =   (sNorm.Dot(sGap) > 0.0)
             ? sSrfPoint2 - (sSrfPoint1 - sNorm)
             : sSrfPoint2 - (sSrfPoint1 + sNorm) ;

//        rbFoundAnswer = TRUE;
//        return SM_SUCCESS;
    } // working with small gap checks

#ifdef SM_DEBUG_CODE
  if (bDebugMe)
    {
      smgfx_SetLook(2,4, 1,0,0); sGap.Draw(&sSrfPoint1); sm_GraphicsLoop();
      sm_GraphicsLoop();
    }
#endif

  // Unitize sSu1 and sSv1 and get their derivatives.
  SmVector3d sS1u_Unit( sS1u );
  sS1u_Unit.Unitize();
  SmVector3d sS1u_Unit_u = sS1u.UnitizedDerivative( sS1uu );
  SmVector3d sS1u_Unit_v = sS1u.UnitizedDerivative( sS1uv );
  SmVector3d sS1v_Unit( sS1v );
  sS1v_Unit.Unitize();
  SmVector3d sS1v_Unit_u = sS1v.UnitizedDerivative( sS1uv );  // Note: not the same as sS1u_Unit_v.
  SmVector3d sS1v_Unit_v = sS1v.UnitizedDerivative( sS1vv );

  if (m_eSolverOperation == SM_SO_3D_SIGNED_DIRECTED_MINIMIZE)
    {
      // SM_SO_3D_SIGNED_DIRECTED_MINIMIZE tries to minimize the following five functions:
      // fun[0] = S1u*(N2)   // Makes tangent planes parallel
      // fun[1] = S1v*(N2)   // Also makes tangent planes parallel
      // fun[2] = S1.x - S2.x - w * V.x
      // fun[3] = S1.y - S2.y - w * V.y
      // fun[4] = S1.z - S2.z - w * V.z
      //
      // It produces the following Jacobian:
      //  | S1uu*(N2)      S1uv*(N2)      S1u*(dN2/du2)    S1u*(dN2/dv2)       0.0  |
      //  | S1uv*(N2)      S1vv*(N2)      S1v*(dN2/du2)    S1v*(dN2/dv2)       0.0  |
      //  | S1u.x          S1v.x         -S2u.x           -S2v.x              -V.x  |
      //  | S1u.y          S1v.y         -S2u.y           -S2v.y              -V.y  |
      //  | S1u.z          S1v.z         -S2u.z           -S2v.z              -V.z  |
      //
      // Amended, as above, to use unitized S1u, S1v, and N2.

      NER(m_cpOptVectors);

      // Surf2 unit Normal and NormalDerivatives
      SmVector3d sCross2 = sS2u * sS2v;
      SmVector3d sCross2u = sS2u.DerivativeOfCrossProduct( sS2v, sS2uu, sS2uv );
      SmVector3d sCross2v = sS2u.DerivativeOfCrossProduct( sS2v, sS2uv, sS2vv );

      // 'N' will mean 'Unit'.
      SmVector3d sN2( sCross2 );
      sN2.Unitize();
      SmVector3d sN2u = sCross2.UnitizedDerivative( sCross2u );
      SmVector3d sN2v = sCross2.UnitizedDerivative( sCross2v );

      if (pOptJacobian)
        {
          (*pOptJacobian)[0][0] =   sS1u_Unit_u.Dot( sN2  );
          (*pOptJacobian)[0][1] =   sS1u_Unit_v.Dot( sN2  );
          (*pOptJacobian)[0][2] =   sS1u_Unit  .Dot( sN2u );
          (*pOptJacobian)[0][3] =   sS1u_Unit  .Dot( sN2v );
          (*pOptJacobian)[0][4] =   0.0;

          (*pOptJacobian)[1][0] =   sS1v_Unit_u.Dot( sN2  );
          (*pOptJacobian)[1][1] =   sS1v_Unit_v.Dot( sN2  );
          (*pOptJacobian)[1][2] =   sS1v_Unit  .Dot( sN2u );
          (*pOptJacobian)[1][3] =   sS1v_Unit  .Dot( sN2v );
          (*pOptJacobian)[1][4] =   0.0;

          (*pOptJacobian)[2][0] =   sS1u.x;
          (*pOptJacobian)[2][1] =   sS1v.x;
          (*pOptJacobian)[2][2] = - sS2u.x;
          (*pOptJacobian)[2][3] = - sS2v.x;
          (*pOptJacobian)[2][4] = - m_cpOptVectors[0].x;

          (*pOptJacobian)[3][0] =   sS1u.y;
          (*pOptJacobian)[3][1] =   sS1v.y;
          (*pOptJacobian)[3][2] = - sS2u.y;
          (*pOptJacobian)[3][3] = - sS2v.y;
          (*pOptJacobian)[3][4] = - m_cpOptVectors[0].y;

          (*pOptJacobian)[4][0] =   sS1u.z;
          (*pOptJacobian)[4][1] =   sS1v.z;
          (*pOptJacobian)[4][2] = - sS2u.z;
          (*pOptJacobian)[4][3] = - sS2v.z;
          (*pOptJacobian)[4][4] = - m_cpOptVectors[0].z;
        }

      // Compute function values
      rF[0] = sS1u_Unit.Dot(sN2);
      rF[1] = sS1v_Unit.Dot(sN2);
      SmVector3d sVec = sSrfPoint1 - sSrfPoint2 - crX[4] * m_cpOptVectors[0];
      rF[2] = sVec.x;
      rF[3] = sVec.y;
      rF[4] = sVec.z;
    }
  else
    {
      // Normalization simply tries to minimize the following four functions
      // fun[0] = S1u*(S2-S1)
      // fun[1] = S1v*(S2-S1)
      // fun[2] = S2u*(S2-S1)
      // fun[3] = S2v*(S2-S1)
      //
      // It produces the following Jacobian
      // | S1uu*(S2-S1) - S1u*S1u   S1uv(S2-S1) - S1u*S1v        S1u*S2u                  S1u*S2v      |
      // | S1uv*(S2-S1) - S1v*S1u   S1vv(S2-S1) - S1v*S1v        S1v*S2u                  S1v*S2v      |
      // |          -S2u*S1u            -S2u*S1v       S2uu*(S2-S1) + S2u*S2u   S2uv*(S2-S1) + S2u*S2v |
      // |          -S2v*S1u            -S2v*S1v       S2uv*(S2-S1) + S2u*S2v   S2vv*(S2-S1) + S2v*S2v |
      //
      // Amended, as above, to use unitized first derivatives.

      // Unitize sDu2 and sDv2 and get their derivatives.
      SmVector3d sS2u_Unit( sS2u );
      sS2u_Unit.Unitize();
      SmVector3d sS2u_Unit_u = sS2u.UnitizedDerivative( sS2uu );
      SmVector3d sS2u_Unit_v = sS2u.UnitizedDerivative( sS2uv );
      SmVector3d sS2v_Unit( sS2v );
      sS2v_Unit.Unitize();
      SmVector3d sS2v_Unit_u = sS2v.UnitizedDerivative( sS2uv );  // Note: not the same as sS2u_Unit_v.
      SmVector3d sS2v_Unit_v = sS2v.UnitizedDerivative( sS2vv );

      if (pOptJacobian)
        {
          (*pOptJacobian)[0][0] =   sS1u_Unit_u.Dot( sGap ) - sS1u_Unit.Dot( sS1u );
          (*pOptJacobian)[0][1] =   sS1u_Unit_v.Dot( sGap ) - sS1u_Unit.Dot( sS1v );
          (*pOptJacobian)[0][2] =   sS1u_Unit  .Dot( sS2u );
          (*pOptJacobian)[0][3] =   sS1u_Unit  .Dot( sS2v );

          (*pOptJacobian)[1][0] =   sS1v_Unit_u.Dot( sGap ) - sS1v_Unit.Dot( sS1u );
          (*pOptJacobian)[1][1] =   sS1v_Unit_v.Dot( sGap ) - sS1v_Unit.Dot( sS1v );
          (*pOptJacobian)[1][2] =   sS1v_Unit  .Dot( sS2u );
          (*pOptJacobian)[1][3] =   sS1v_Unit  .Dot( sS2v );

          (*pOptJacobian)[2][0] = - sS2u_Unit  .Dot( sS1u );
          (*pOptJacobian)[2][1] = - sS2u_Unit  .Dot( sS1v );
          (*pOptJacobian)[2][2] =   sS2u_Unit_u.Dot( sGap ) + sS2u_Unit.Dot( sS2u );
          (*pOptJacobian)[2][3] =   sS2u_Unit_v.Dot( sGap ) + sS2u_Unit.Dot( sS2v );

          (*pOptJacobian)[3][0] = - sS2v_Unit  .Dot( sS1u );
          (*pOptJacobian)[3][1] = - sS2v_Unit  .Dot( sS1v );
          (*pOptJacobian)[3][2] =   sS2v_Unit_u.Dot( sGap ) + sS2v_Unit.Dot( sS2v );
          (*pOptJacobian)[3][3] =   sS2v_Unit_v.Dot( sGap ) + sS2v_Unit.Dot( sS2v );
        }

      // Compute function values
      rF[0] = sS1u_Unit.Dot( sGap );
      rF[1] = sS1v_Unit.Dot( sGap );
      rF[2] = sS2u_Unit.Dot( sGap );
      rF[3] = sS2v_Unit.Dot( sGap );
    }

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe2 = FALSE;
  if (bDebugMe2)
    {
      if (pOptJacobian && m_eSolverOperation == SM_SO_3D_SIGNED_DIRECTED_MINIMIZE)
        {
          pOptJacobian->Dump();
          smos_WriteBuffer(_T(" X --- \n")); crX.Dump();
          smos_WriteBuffer(_T(" F --- \n")); rF.Dump();
          SmMatrix sTmpJ(5,5);
          SmTArray<double> sTmpX(crX);
          SER(ComputeJacobian(sTmpX,rF,sTmpJ));
          sTmpJ.Dump();
        }
      else
       {
         if (pOptJacobian) pOptJacobian->Dump();
         smos_WriteBuffer(_T(" X --- \n")); crX.Dump();
         smos_WriteBuffer(_T(" F --- \n")); rF.Dump();
       }
   }
#endif

  return SM_SUCCESS;

} // end SmSSNormalizeENFO::Evaluate

// clang-format on
} // namespace sc_srf_p4

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_surface_p4.rs).
enum : int32_t
{
    OP_NORMAL_EVAL = 0,
    OP_SECTION_FN = 1,
    OP_SECTION_EVAL = 2,
    OP_ON_NORMAL = 3,
    OP_PASS_SP = 4,
    OP_SEAM = 5,
    OP_IS_ON_SEAM = 6,
    OP_GPS = 7,
    OP_CS_INTERSECT = 8,
    OP_CS_NORMALIZE = 9,
    OP_SSP_INTERSECT = 10,
    OP_SSI_NORMALIZE = 11,
    OP_SS_NORMALIZE = 12,
    OP_CLASSIFY = 13,
    OP_STEP_INV = 14,
};

struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    bool flag() { return in[at++] != 0.0; }
    SmBoolean bflag() { return flag() ? TRUE : FALSE; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
    SmPoint2d uv()
    {
        double u = num();
        double v = num();
        return SmPoint2d(u, v);
    }
    SmExtent2d dom()
    {
        double u0 = num();
        double v0 = num();
        double u1 = num();
        double v1 = num();
        return SmExtent2d(SmPoint2d(u0, v0), SmPoint2d(u1, v1));
    }
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

const SmContext& the_context()
{
    static SmContext* p = new SmContext();
    return *p;
}

SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 2)
    {
        // a curve-bounded surface without boundary curves over a basis surface block
        SmSurface* basis = read_surface(rd, ctx);
        bool implicit = rd.flag();
        SmTArray<SmCurve*> c3d, cuv;
        return new (ctx) SmCurveBoundedSurface(*basis, &c3d, &cuv, implicit ? TRUE : FALSE);
    }
    if (kind == 1)
    {
        SmPoint3d o = rd.vec();
        SmVector3d n = rd.vec();
        return new (ctx) SmPlane(o, n, &ctx);
    }
    bool oob = rd.flag();
    SmBSplineSurfaceForm form = (SmBSplineSurfaceForm)rd.idx();
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
    SmBSplineSurface* b = new (ctx) SmBSplineSurface(sur, FALSE, NULL);
    b->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
    b->SetBSplineSurfaceForm(form);
    return b;
}

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

const double* opt_target(Rd& rd, double& store)
{
    bool has = rd.flag();
    store = rd.num();
    return has ? &store : NULL;
}

std::vector<SmVector3d> read_vecs(Rd& rd)
{
    int64_t n = rd.idx();
    std::vector<SmVector3d> v;
    for (int64_t i = 0; i < n; i++)
        v.push_back(rd.vec());
    return v;
}

// Evaluate one equation set at x: status, then (on success) found, F and the Jacobian.
template <typename Enfo>
void eval_enfo(Enfo& e, int n, const std::vector<double>& x, std::vector<double>& o)
{
    SmTArray<double> X, F;
    for (int i = 0; i < n; i++)
    {
        X.Add(x[i]);
        F.Add(0.0);
    }
    SmMatrix J(n, n);
    SmBoolean found = FALSE;
    SmStatus r = e.Evaluate(X, F, &J, found);
    o.push_back(st(r));
    if (r == SM_SUCCESS)
    {
        o.push_back(flg(found));
        if (found)
        {
            // upstream returns early: only the last function value is set
            o.push_back(F[n - 1]);
            return;
        }
        for (int i = 0; i < n; i++)
            o.push_back(F[i]);
        for (int i = 0; i < n; i++)
            for (int j = 0; j < n; j++)
                o.push_back(J[i][j]);
    }
}

std::vector<std::vector<double>> read_points(Rd& rd, int n)
{
    int64_t count = rd.idx();
    std::vector<std::vector<double>> xs;
    for (int64_t k = 0; k < count; k++)
    {
        std::vector<double> x;
        for (int i = 0; i < n; i++)
            x.push_back(rd.num());
        xs.push_back(x);
    }
    return xs;
}

// the reason code of smsurf_IsPointOnNormal: 0 unknown (on normal), 1 outside of domain, 2 bad surface point,
// 3 not within tolerance
double reason_code(SmStatus r)
{
    if (r == SM_ERR_OUTSIDE_OF_DOMAIN)
        return 1.0;
    if (r == SM_ERR_BAD_SURFACE_POINT)
        return 2.0;
    if (r == SM_ERR_NOT_WITHIN_TOLERANCE)
        return 3.0;
    return 0.0;
}

void run(int32_t op, Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    SmSurface* s = read_surface(rd, ctx);
    switch (op)
    {
    case OP_NORMAL_EVAL:
    {
        SmPoint2d uv = rd.uv();
        SmSurfaceEval e(s, uv.x, uv.y);
        if (e.FailsEval())
        {
            o.push_back((double)SM_ERR);
            break;
        }
        SmVector3d n = e.Normal();
        o.push_back(0.0);
        put_vec(o, n);
        break;
    }
    case OP_SECTION_FN:
    {
        SmPoint2d uv = rd.uv();
        SmVector3d dir = rd.vec();
        SmVector3d pvv[3][3];
        SmStatus r = s->Evaluate(uv, 2, 2, TRUE, TRUE, TRUE, &pvv[0][0], TRUE);
        if (r == SM_SUCCESS)
        {
            SmVector3d n = pvv[1][0] * pvv[0][1];
            if (n.LengthSquared() < SM_EFF_ZERO_SQ)
                r = s->EvaluateNormal(uv, TRUE, TRUE, n);
            else
                n.Unitize();
            pvv[2][2] = n;
        }
        if (r != SM_SUCCESS)
        {
            o.push_back(st(r));
            break;
        }
        SmVector3d tangent;
        double curv = 0.0;
        r = smsurf_EvaluateNormalSection(pvv, dir, tangent, curv);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            put_vec(o, tangent);
            o.push_back(curv);
        }
        break;
    }
    case OP_SECTION_EVAL:
    {
        SmPoint2d uv = rd.uv();
        SmVector3d dir = rd.vec();
        SmSurfaceEval e(s, uv.x, uv.y, dir);
        if (e.FailsEval())
        {
            o.push_back((double)SM_ERR);
            break;
        }
        SmVector3d n = e.Normal();
        SmVector3d t = e.SectTangent();
        double k = e.SectCurvature();
        o.push_back(0.0);
        put_vec(o, n);
        put_vec(o, t);
        o.push_back(k);
        break;
    }
    case OP_ON_NORMAL:
    {
        bool has_dom = rd.flag();
        SmExtent2d d = rd.dom();
        SmPoint2d uv = rd.uv();
        SmPoint3d p = rd.vec();
        bool has_d = rd.flag();
        SmVector3d dv = rd.vec();
        double tol = rd.num();
        SmBoolean fu = TRUE, fv = TRUE, good = FALSE;
        SmStatus reason = SM_ERR_UNKNOWN;
        double drop = 0.0, dev = 0.0;
        SmStatus r = smsurf_IsPointOnNormal(*s, has_dom ? &d : NULL, uv, p, has_d ? &dv : NULL, fu, fv, tol, good, reason,
                                            drop, dev);
        o.push_back(st(r));
        o.push_back(flg(fu));
        o.push_back(flg(fv));
        o.push_back(flg(good));
        o.push_back(reason_code(reason));
        o.push_back(drop);
        o.push_back(reason == SM_ERR_BAD_SURFACE_POINT ? 0.0 : dev);
        break;
    }
    case OP_PASS_SP:
    {
        SmPoint2d uv = rd.uv();
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        o.push_back(0.0);
        o.push_back(flg(sc_srf_p4::sm_PassSPConvergenceTest(*s, uv, p, tol)));
        break;
    }
    case OP_SEAM:
    {
        SmSurfParamType param = (SmSurfParamType)rd.idx();
        bool known = rd.flag();
        SmCrvOnSurf* seam = NULL;
        SmStatus r = s->CreateSeamCurve(ctx, param, seam, known ? TRUE : UNSURE);
        o.push_back(st(r));
        o.push_back(flg(seam != NULL));
        if (seam != NULL)
        {
            SmExtent1d iv = seam->GetNaturalInterval();
            o.push_back(iv.GetMin());
            o.push_back(iv.GetMax());
            for (int k = 0; k < 5; k++)
            {
                double t = iv.GetMin() + (iv.GetMax() - iv.GetMin()) * k / 4.0;
                SmPoint3d pt(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
                SmStatus er = seam->EvaluatePoint(t, pt);
                o.push_back(st(er));
                put_vec(o, pt);
            }
            delete seam;
        }
        break;
    }
    case OP_IS_ON_SEAM:
    {
        SmPoint2d uv = rd.uv();
        bool has_tol = rd.flag();
        double tol = rd.num();
        SmTol3d tol3d(tol);
        SmSurfParamType dir = SM_SP_NEITHER;
        SmTArray<SmPoint2d> cross;
        SmBoolean r = s->IsOnSeam(uv, has_tol ? &tol3d : NULL, &dir, &cross);
        o.push_back(0.0);
        o.push_back(flg(r));
        o.push_back((double)dir);
        o.push_back((double)cross.GetSize());
        for (ULONG i = 0; i < cross.GetSize(); i++)
            put_uv(o, SmVector2d(cross[i].x, cross[i].y));
        break;
    }
    case OP_GPS:
    {
        SmExtent2d d = rd.dom();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        double tgt = 0.0;
        const double* pt = opt_target(rd, tgt);
        SmSolutionRequestedType req = (SmSolutionRequestedType)rd.idx();
        SmSolutionArray sols;
        SmStatus r = s->GlobalPointSolve(d, sop, p, SmZoneTol3d(tol), pt, req, sols);
        o.push_back(st(r));
        o.push_back((double)sols.GetSize());
        for (ULONG i = 0; i < sols.GetSize(); i++)
        {
            o.push_back(sols[i].m_vStart.m_dSolutionValue);
            o.push_back(sols[i].m_vStart[0]);
            o.push_back(sols[i].m_vStart[1]);
        }
        break;
    }
    case OP_STEP_INV:
    {
        SmExtent2d d = rd.dom();
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        bool has_guess = rd.flag();
        SmPoint2d guess = rd.uv();
        SmPoint2d uv;
        SmLocationType loc = SM_LT_EXTERIOR;
        SmStatus r = s->STEPInversion(d, p, tol, uv, loc, has_guess ? &guess : NULL);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back(uv.x);
            o.push_back(uv.y);
            o.push_back((double)loc);
        }
        break;
    }
    case OP_CS_INTERSECT:
    {
        SmBSplineCurve* c = read_curve(rd, ctx);
        std::vector<std::vector<double>> xs = read_points(rd, 3);
        sc_srf_p4::SmCSIntersectENFO e(*c, *s, SM_EFF_ZERO * SM_EFF_ZERO);
        o.push_back(0.0);
        for (const std::vector<double>& x : xs)
            eval_enfo(e, 3, x, o);
        delete c;
        break;
    }
    case OP_CS_NORMALIZE:
    {
        SmBSplineCurve* c = read_curve(rd, ctx);
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        std::vector<SmVector3d> vecs = read_vecs(rd);
        int n = sop == SM_SO_3D_SIGNED_DIRECTED_MINIMIZE ? 4 : 3;
        std::vector<std::vector<double>> xs = read_points(rd, n);
        sc_srf_p4::SmCSNormalizeENFO e(*c, *s, sop, SM_EFF_ZERO, vecs.empty() ? NULL : vecs.data(),
                                       SM_EFF_ZERO * SM_EFF_ZERO);
        o.push_back(0.0);
        for (const std::vector<double>& x : xs)
            eval_enfo(e, n, x, o);
        delete c;
        break;
    }
    case OP_SSP_INTERSECT:
    {
        SmSurface* s2 = read_surface(rd, ctx);
        bool steepest = rd.flag();
        bool has_plane = rd.flag();
        SmPoint3d po = rd.vec();
        SmVector3d pn = rd.vec();
        std::vector<std::vector<double>> xs = read_points(rd, 4);
        SmPlane* plane = has_plane ? new (ctx) SmPlane(po, pn, &ctx) : NULL;
        sc_srf_p4::SmSSPIntersectENFO e(*s, *s2, steepest ? TRUE : FALSE, plane, SM_EFF_ZERO * SM_EFF_ZERO);
        o.push_back(0.0);
        for (const std::vector<double>& x : xs)
            eval_enfo(e, 4, x, o);
        delete plane;
        delete s2;
        break;
    }
    case OP_SSI_NORMALIZE:
    {
        SmSurface* s2 = read_surface(rd, ctx);
        SmVector3d pn = rd.vec();
        double pd = rd.num();
        std::vector<std::vector<double>> xs = read_points(rd, 4);
        sc_srf_p4::SmSSINormalizeENFO e(*s, *s2, SM_EFF_ZERO * SM_EFF_ZERO, pn, pd);
        o.push_back(0.0);
        for (const std::vector<double>& x : xs)
            eval_enfo(e, 4, x, o);
        delete s2;
        break;
    }
    case OP_SS_NORMALIZE:
    {
        SmSurface* s2 = read_surface(rd, ctx);
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        std::vector<SmVector3d> vecs = read_vecs(rd);
        int n = sop == SM_SO_3D_SIGNED_DIRECTED_MINIMIZE ? 5 : 4;
        std::vector<std::vector<double>> xs = read_points(rd, n);
        sc_srf_p4::SmSSNormalizeENFO e(*s, *s2, sop, vecs.empty() ? NULL : vecs.data(), SM_EFF_ZERO * SM_EFF_ZERO);
        o.push_back(0.0);
        for (const std::vector<double>& x : xs)
            eval_enfo(e, n, x, o);
        delete s2;
        break;
    }
    case OP_CLASSIFY:
    {
        SmExtent2d d = rd.dom();
        SmBSplineCurve* c = read_curve(rd, ctx);
        double a = rd.num(), b = rd.num();
        SmPoint2d uv = rd.uv();
        double t = rd.num();
        SmBoolean found = FALSE;
        SmPoint2d guess_uv(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        double guess_t = SM_UNDEF_DOUBLE;
        SmTsectCurveType type = SM_TC_UNKNOWN;
        SmStatus r = sc_srf_p4::sm_ClassifyCurveSurfaceXSectPoint(*s, d, *c, SmExtent1d(a, b), uv, t, found, guess_uv,
                                                                  guess_t, type);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back(flg(found));
            o.push_back(guess_uv.x);
            o.push_back(guess_uv.y);
            o.push_back(guess_t);
            o.push_back((double)type);
        }
        delete c;
        break;
    }
    default:
        o.push_back((double)SC_ERR_ARG);
        break;
    }
    delete s;
}
} // namespace

SC_EXPORT int32_t sc_surface_surface_p4_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                             int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    run(op, rd, o);
    if (rd.at != n_in)
    {
        *out_len = 0;
        return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
