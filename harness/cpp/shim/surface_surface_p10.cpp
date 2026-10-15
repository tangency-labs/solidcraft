// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surface_p10`: SmLSIGlobalSolver::LocalSolve and
// sm_GlobalCurveIntersectCombineSolutions (source/SMLib/src/SmSurface.cpp).
//
// SmLSIGlobalSolver is a class local to SmSurface.cpp with external linkage: its declaration is
// reproduced here (SmSurface.cpp lines 17885-17932) and the library's constructor and LocalSolve
// are called. The solver runs on a hand-built surface cache tree (as the
// intersect.surface_silhouette shim builds them; tree construction belongs to unit
// surface.surface_cache_p3): patch nodes with their domain, box, continuities, node class,
// optional polar box and pseudo box; every mBA_pSurface is the cache surface.
//
// sm_GlobalCurveIntersectCombineSolutions is file-static, so it is a VERBATIM copy of upstream
// SmSurface.cpp (commit c9c5979, lines 15517-16945) inside the namespace sc_srf_p10. The static
// sm_ClassifyCurveSurfaceXSectPoint it calls (and its helper sm_ConvertToParameterDelta) is
// copied verbatim from upstream as well (lines 13053-13773, as in the surface_p4 shim).
//
// One entry point, sc_surface_surface_p10_run(in, n_in, out, cap, out_len). The program layout
// is documented in crates/solidcore-ffi/src/units/surface_surface_p10.rs, which validates every
// input; reads are bounds checked here too (a bad program is SC_ERR_ARG).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveCache.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmGeomUtility.h>
#include <SmGlobalSolver.h>
#include <SmLine.h>
#include <SmLocalSolveNd.h>
#include <SmMatrix.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmPolarBox.h>
#include <SmPseudoBox.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmSurfTypes.h>
#include <SmSurfaceCache.h>
#include <SmTol.h>
#include <SmTArray.h>
#include <SmTree.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <stdexcept>
#include <vector>

// ---- SmSurface.cpp lines 17885-17932: SmLSIGlobalSolver (declaration) -------------------------
class SmLSIGlobalSolver : public SmGlobalSolver
{
public:
  const SmSurface  * m_pSurf;
  const SmExtent2d & m_crUVDomain;
  SmPoint3d          m_vLinePoint;
  SmVector3d         m_vLineVector;
  const SmExtent1d * m_cpLineInterval;
  SmBoolean          m_bRayFire;
  double             m_dDistanceTolerance;

  SmLSIGlobalSolver(SmTree           * pSurfaceTree,
                    const SmExtent2d & crUVDomain,
                    const SmPoint3d  & crLinePoint,
                    const SmVector3d & crLineVector,
                    const SmExtent1d * cpLineInterval,
                    SmBoolean          bRayFire,
                    double             dDistanceTolerance);

 virtual ~SmLSIGlobalSolver() { m_cpLineInterval     = NULL ;
                                m_dDistanceTolerance = SM_UNDEF_DOUBLE ;
                              }

  virtual SmBoolean BranchMayContainAnswers(SmTreeNode * apBranch[SM_GS_MAX_TREES]);
  virtual SmStatus LocalSolve(SmTreeNode * apBranch[SM_GS_MAX_TREES],
                              SmBoolean & rbNeedsMoreSubdivision);
  virtual SmSolutionPairType IdenticalSolutions(const SmSolution & crSol1,
                                                const SmSolution & crSol2,
                                                const SmSolution *&pPreferredSol)
                                               const;
} ;

namespace sc_srf_p10
{
// clang-format off

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


// ---- verbatim: SmSurface.cpp lines 15517-16945: sm_GlobalCurveIntersectCombineSolutions
/*******************************************************************//**
PURPOSE: Combine set of Surface/Curve solutions into unique
            point and coincident segment solutions.

NOTES: Note that the solutions will be sorted relative to the curve.
***********************************************************************/
static SmStatus sm_GlobalCurveIntersectCombineSolutions
    (const SmSurface  & crSurface,          // in : Surface being intersected
     const SmExtent2d & crUVDomain,         // in : Domain of the surface to intersect
     const SmCurve    & crCurve,            // in : Curve to intersect with surface
     const SmExtent1d & crInterval,         // in : Domain of curve to intersect
     double             dDistanceTolerance, // in : 3D distance tolerance to use in intersection
     SmSolutionArray  & rSolutions)         // i/o: Contains results of intersection
{
  // locals
  SmBoolean bFoundCoincidence = FALSE;
  ULONG ii, jj ;

  // See if Pairs of Solutions should be combined
  //   Classify the Opening between the two solutions
  //    case 1: Solutions are two ends of a closed curve - don't combine
  //    case 2: Solutions are two different ends of a seam in a closed surface - don't combine
  //    case 3: Solution interval EndPoints touch - do combine
  //    case 4: Solution Intervals overlap - do combine
  //    case 5: There is an opening between two interval solutions
  //     5a. Curve and Surface are coincident throughout Solution Opening - do combine
  //     5b. Curve and Surface are not coincident throughout Solution Opening - don't combine
  //    case 6: GWC: a Point solution neighbors a point solution -
  //             if both TsectPoints classify to SM_TC_COINCIDENT and the
  //             curve and Surface are coincident throughout Solution Opening - mark bFoundCoincidence TRUE
  //    case 7: GWC: a coincident interval neighbors a point solution,
  //             If the TsectPoint classifies to SM_TC_COINCIDENT - combine solutions
  //             if the TsectPoint classifies to SM_TC_CROSSING, SM_TC_NEAR_TANGENT or
  //                SM_TC_TANGENT, drop the coincident interval assuming intersecting
  //                   nearly tangent curves

  // gwc: When a nearly tangent curve intersects a surface it's possible
  //      that the actual intersection point(s) plus several neighboring
  //      coincident interval subdivision spans segments are returned as
  //      solutions - Save the intersection points and drop the
  //      false coincident segments.
  // gwc: When a curve coincident to a surface is intersected its possible the
  //      local solve will return a set of intersection points - one per
  //      curveSubdivisionTree leaf node.  These intersection points need
  //      to be identified as coincident solutions so that that the DropAndTrimCurve()
  //      function can be called to test for coincidence. This is done
  //      by setting the bFoundCoincidence boolean variable to TRUE.
#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE ;
  if (bDebugMe)
    {
      rSolutions.Dump();

      SmFace *pFace = (SmFace *)crSurface.GetFace() ;
      SmEdge *pEdge = (SmEdge *)crCurve.GetEdge() ;
      SmBrep *pBrep1 = pFace ? pFace->GetBrep() : NULL ;
      SmBrep *pBrep2 = pEdge ? pEdge->GetBrep() : NULL ;

      smgfx_Erase();
      smgfx_SetLook(1,2, 0,0,1); if(pBrep1) pBrep1->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,1,0); if(pBrep2) pBrep2->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,3, 0,1,1); crSurface.DrawUV(); sm_GraphicsLoop();
      smgfx_SetLook(1,2, 0,0,0); if(pFace) pFace->Draw(SM_DM_CROSSHATCH); sm_GraphicsLoop() ;
      smgfx_SetLook(2,3, 0,1,0); crCurve.Draw(); sm_GraphicsLoop();
      smgfx_SetLook(4,5, 1,0,1); if(pEdge) pEdge->Draw(); sm_GraphicsLoop();
      smgfx_SetLook(6,7, 1,0,0); rSolutions.Draw() ; sm_GraphicsLoop() ;
      sm_GraphicsLoop();
   }
#endif // SM_DEBUG_CODE

  // keep, combine, drop, and compress solutions inplace in the rSolutions array in two phases.
  //   phase 1: merge coincident intervals
  //            merge point solutions close enough to be the same point
  //            identify point solutions that classify to SM_TC_COINCIDENT and
  //            remember that by setting bFoundCoincidence == TRUE.
  //   phase 2: drop coincident intervals that include or neighbor point solutions
  //               that don't classify to SM_TC_COINCIDENT
  //   rSolutions Indexing: keep a last intervalIndex and a last PointIndex,
  //             use a pointer to switch between the two within the upcoming loop.

  // locals
  SmVector2d sUVSize = crSurface.GetNaturalUVDomain().GetSize();
  double     sTSize  = crCurve.GetNaturalInterval().GetLength() ;
  SmTsectCurveType ePointType = SM_TC_UNKNOWN ;
  SmPoint2d sCurrentUV, sNewUVGuess ;
  double    dCurrentT, dNewTGuess ;
  SmBoolean bFoundGuess ;

  // set up rSolutions array indexing for Point and interval solutions
  ULONG lLastIntervalIndex = 0;
  ULONG lLastPointIndex = 0;     // may actually point to a range solution if the point has been combined
  ULONG *pLastIndex ;
  if(rSolutions.GetSize() > 0)
    {
      if(rSolutions[0].m_eSolutionType == SM_ST_RANGE_OF_VALUES)
           { lLastIntervalIndex = 0 ;
             lLastPointIndex    = SM_BIG_ULONG ;
           }
      else { lLastIntervalIndex = SM_BIG_ULONG ;
             lLastPointIndex    = 0 ;
             sCurrentUV.Set(rSolutions[0].m_vStart[1], rSolutions[0].m_vStart[2]) ;
             dCurrentT = rSolutions[0].m_vStart[0] ;
             sm_ClassifyCurveSurfaceXSectPoint( crSurface, crUVDomain,
                                                crCurve, crInterval,
                                                sCurrentUV, dCurrentT,
                                                bFoundGuess, sNewUVGuess, dNewTGuess,
                                                ePointType) ;
             if(    ePointType == SM_TC_COINCIDENT
//               || ePointType == SM_TC_NEAR_TANGENT ... for tiny two-pt intersections [B124]
               )
               {
                 bFoundCoincidence = TRUE ;
               }
           }
    } // end need to init CombineLoop locals check

  // for every rSolution after the 1st
  // - see if it can be combined with the lastProcessed solution of same type
  for (ii=1; ii<rSolutions.GetSize(); ii++)
    {
      // get current Solution
      SmSolution    & rCurrSol   = rSolutions[ii];

      // solutions are ordered by the curve parameter value
      //  When working with closed surfaces and solutions on seams
      //  multiple solutions with the same curve parameter may be generated.
      // Choose which of those to examine next.
      //   1. Find solution closest to the last lProcessedEnd solution measuring dist between sol StartPoints
      //   2. Place it in the next SolutionsArray Slot

      // get index of last solution of same type as rCurrSol
      pLastIndex =     (rCurrSol.m_eSolutionType == SM_ST_RANGE_OF_VALUES)
                      ? &lLastIntervalIndex
                      : &lLastPointIndex ;

      // get last solution
      SmSolution    & rLastSol   =   (*pLastIndex != SM_BIG_ULONG)
                                   ? rSolutions[*pLastIndex]
                                   : rSolutions[0] ;
      SmSolutionEnd * pLastEnd   =   (rLastSol.m_eSolutionType == SM_ST_RANGE_OF_VALUES)
                                   ? &rLastSol.m_vEnd
                                   : &rLastSol.m_vStart ;

      SmBoolean bDoCombine = FALSE;

      // no previous solution of same type - keep solution see if solutions array needs packing
      if(*pLastIndex == SM_BIG_ULONG)
        {
          bDoCombine = FALSE ;
          goto CombineDecided ;
        }

      {
          // init ClosestSolution to this solution (closed surface solutions?)
          ULONG  lClosest = ii;
          double dScaledZero   = SM_EFF_ZERO * (1.0 + smos_Fabs(rSolutions[ii].m_vStart[0]));

          // for every other solution - Look for other solutions that start where this one starts on the curve
          for(jj=ii+1; jj<rSolutions.GetSize(); jj++)
            {
              // when a solution starts at the same CurveParam
              if (smos_Fabs(rSolutions[lClosest].m_vStart[0]-rSolutions[jj].m_vStart[0]) < dScaledZero)
                {
                  // when OtherStart/LastEnd is closer in SurfUVSpace than ClosestStart/LastEnd UVDist
                  double dUDiff1 = smos_Fabs(rSolutions[lClosest].m_vStart[1]-(*pLastEnd)[1]);
                  double dVDiff1 = smos_Fabs(rSolutions[lClosest].m_vStart[2]-(*pLastEnd)[2]);
                  double dUDiff2 = smos_Fabs(rSolutions[jj].m_vStart[1]-(*pLastEnd)[1]);
                  double dVDiff2 = smos_Fabs(rSolutions[jj].m_vStart[2]-(*pLastEnd)[2]);

                  if (dUDiff2+dVDiff2 < dUDiff1+dVDiff1)
                    {
                      // make ThisSolution the closest solution
                      lClosest = jj;
                    }
                }
              else // quit at the first worst solution
                { break; }
            } // end iter every otherSolution matching up endPoints

          // when a closer OtherSolution was found - swap its position in the rSolutions array
          if (lClosest != ii)
            {
              // Swap lClosest and ii - the original sorting order was not what we wanted
              SmSolution sTmpSol   = rSolutions[lClosest];
              rSolutions[lClosest] = rSolutions[ii];
              rSolutions[ii]       = sTmpSol;
              SM_ASSERT(&(rCurrSol.m_vStart) == &(rSolutions[ii].m_vStart)) ;
            }
      }

      // See if Pairs of Solutions should be combined
      //   Classify the Opening between the two solutions
      //    case 1: Solutions are two ends of a closed curve - don't combine
      //    case 2: Solutions are two different ends of a seam in a closed surface - don't combine
      //    case 3: Solution interval EndPoints touch - do combine
      //    case 4: Solution Intervals overlap - do combine
      //    case 5: There is an opening between two interval solutions
      //     5a. Curves are coincident throughout Solution Opening - do combine
      //     5b. Curves are not coincident throughout Solution Opening - don't combine
      //    case 6: GWC: a Point solution neighbors a point solution -
      //             if both TsectPoints classify to SM_TC_COINCIDENT and the
      //             curve and Surface are coincident throughout Solution Opening - do combine
      //    case 7: GWC: a coincident interval neighbors a point solution,
      //             If the TsectPoint classifies to SM_TC_COINCIDENT - set bFoundCoincidence = TRUE
      //             if the TsectPoint classifies to SM_TC_CROSSING, SM_TC_NEAR_TANGENT, or
      //                SM_TC_TANGENT, drop the coincident interval assuming intersecting
      //                   nearly tangent curves

      // make new scope to isolate local variable initializations from the goto statement
      {
          // locals
          SmSolutionEnd & rCurrStart = rCurrSol.m_vStart;

          // Don't allow jumping over seam -
          //  detect seam jumping by large UVdistances between matched endPoints
          // Get UVDist Vector between Surface solution endPoints
          SmVector2d sUVDiff(smos_Fabs(rCurrStart[1]-(*pLastEnd)[1]),
                             smos_Fabs(rCurrStart[2]-(*pLastEnd)[2]) );

          // Get TDist between Curve solution endPoints
          double sTDiff = smos_Fabs(rCurrStart[0]-(*pLastEnd)[0]);

          // when the Surface endPoints are separated by the domain space
          //      and the Curve endPoints are coincident
          // Note, the parameter diffs can be a little less than the domain size
          // due to tolerances.  [my_test_csi_tol()]
          // if(   (   smos_Fabs(sUVDiff.x-sUVSize.x) < SM_EFF_ZERO_SQRT
          //        || smos_Fabs(sUVDiff.y-sUVSize.y) < SM_EFF_ZERO_SQRT)
          //    && (sTDiff < SM_EFF_ZERO_SQRT))
          if(   (   sUVDiff.x > sUVSize.x * 0.90     // maybe add? && crSurface.IsClosed( SM_SP_V
                 || sUVDiff.y > sUVSize.y * 0.90))   // maybe Add? && crSurface.IsClosed( SM_SP_U
             // && sTDiff < sTSize * 0.10 )
            {
              // don't connect solutions across Surface seams
              bDoCombine = FALSE ;
              goto CombineDecided ;
            }

          // when the Curve endPoints are separted by the domain space
          //      and the surface endPoints are coincident
          // if(   (sTDiff - sTSize) < SM_EFF_ZERO_SQRT
          //    && sUVDiff.x < SM_EFF_ZERO_SQRT
          //    && sUVDiff.y < SM_EFF_ZERO_SQRT)
          if (   sTDiff > sTSize * 0.90 )
              //      && sUVDiff.x < sUVSize.x * 0.10
              //      && sUVDiff.y < sUVSize.y * 0.10
              //      && crCurve.IsClosed( crInterval, dDistanceTolerance ))
           {
             // don't connect solutions across closed curve ends
              bDoCombine = FALSE ;
              goto CombineDecided ;
            }

          // When Solution endPoints touch on either curve or surface - combine solutions.
          // Test works for both IntervalPair and PointPair Solutions
          //   Solutions pairs on opposite sides of seams have already been culled.
          if(   SM_ARE_SAME((*pLastEnd)[0],rCurrStart[0])
             || (   SM_ARE_SAME((*pLastEnd)[1],rCurrStart[1])
                 && SM_ARE_SAME((*pLastEnd)[2],rCurrStart[2])))
            {
              bDoCombine = TRUE ;

              // for interval solutions - make some exceptions
              if(rCurrSol.m_eSolutionType == SM_ST_RANGE_OF_VALUES)
                {
                  double dStartDev1 = rLastSol.m_vStart.m_dSolutionValue;
                  double dEndDev1   = rLastSol.m_vEnd.m_dSolutionValue;

                  // don't combine when end of rLastSol is much better solution than start
                  if (   dStartDev1 > SM_EFF_ZERO
                      && dStartDev1 > dEndDev1 * 100000.0)
                    {
                      bDoCombine = FALSE;
                    }
                  double dStartDev2 = rCurrSol.m_vStart.m_dSolutionValue;
                  double dEndDev2   = rCurrSol.m_vEnd.m_dSolutionValue;

                  // don't combine when start of CurrSol is much better solution than end
                  if (   dEndDev2 > SM_EFF_ZERO
                      && dEndDev2 > dStartDev2 * 100000.0)
                    {
                      bDoCombine = FALSE;
                    }

                  //      // GWC: I'm going to delete this part of this heuristic - it makes no sense.
                  //      //      Perhaps the last two checks should be turned off as well.
                  //
                  //      // Keep those intervals which are increasing or decreasing
                  //      // GWC: This looks like a bad heuristic implementation
                  //      //       1. Anything set to FALSE from (dStartDev1 > dEndDev1 * 100000.0) will be set back to TRUE here
                  //      //       2. Anything set to FALSE from (dEndDev2 > dStartDev2 * 100000.0) can't be set back to TRUE here
                  //      if (   dStartDev2 > 100.0*dEndDev2
                  //          && dStartDev1 > 100.0*dEndDev1)
                  //        {
                  //          bDoCombine = TRUE;
                  //        }

                  // If the smaller segment is less than 1/1000 of the other then
                  // just combine them anyhow.
                  double dLastLeng = smos_Max( SM_EFF_ZERO, rLastSol.m_vEnd[0] - rLastSol.m_vStart[0] );
                  double dCurrLeng = smos_Max( SM_EFF_ZERO, rCurrSol.m_vEnd[0] - rCurrSol.m_vStart[0] );

                  if (   dLastLeng > dCurrLeng * 1000.00
                      || dCurrLeng > dLastLeng * 1000.00)
                    {
                      bDoCombine = TRUE;
                    }

                } // end special exceptions for IntervalPair Solutions checks
              goto CombineDecided ;
            }

          // for point solutions only
          if(rCurrSol.m_eSolutionType == SM_ST_SINGLE_VALUE)
            {
              // evaluate solution points
              SmPoint3d sPt1, sPt2 ;
              crCurve.EvaluatePoint(rCurrStart[0],  sPt1) ;
              crCurve.EvaluatePoint((*pLastEnd)[0], sPt2) ;
              double dDist2 = sPt1.DistanceBetweenSquared(sPt2) ;

              // combine solutions when points are close enough together
              // don't combine solutions far enough apart to be distinct.
              bDoCombine = dDist2 < dDistanceTolerance * dDistanceTolerance ;
              if(bDoCombine)
                goto CombineDecided ;

              // for solutions which are further apart
              //  see if points bound a coincident segment region
              sCurrentUV.Set(rSolutions[0].m_vStart[1], rSolutions[0].m_vStart[2]) ;
              dCurrentT = rSolutions[0].m_vStart[0] ;
              sm_ClassifyCurveSurfaceXSectPoint( crSurface, crUVDomain,
                                                 crCurve, crInterval,
                                                 sCurrentUV, dCurrentT,
                                                 bFoundGuess, sNewUVGuess, dNewTGuess,
                                                 ePointType) ;

              if(    ePointType == SM_TC_COINCIDENT
//                || ePointType == SM_TC_NEAR_TANGENT ... for tiny two-pt intersections [B124]
                )
                {
                  bFoundCoincidence = TRUE ;
                } // end combine two points into a coincident segment check

            } // end if point solutions branch

          // for range solutions only
          //    case 4: Solution Intervals overlap - do combine
          //    case 5: There is an opening between the two interval solutions
          //     5a. Curves are coincident throughout Solution Opening - do combine
          //     5b. Curves are not coincident throughout Solution Opening - don't combine
          if(rCurrSol.m_eSolutionType == SM_ST_RANGE_OF_VALUES)
            {
              SM_ASSERT(rLastSol.m_eSolutionType == SM_ST_RANGE_OF_VALUES) ;

              // When SolutionIntervals overlap on both curve and surface - combine solutions
              if((*pLastEnd)[0] >= rCurrStart[0])
                {
                  bDoCombine = (   SM_IS_BETWEEN(rCurrStart[1],
                                                 rLastSol.m_vStart[1],
                                                 rLastSol.m_vEnd[1])
                                && SM_IS_BETWEEN(rCurrStart[1],
                                                 rLastSol.m_vStart[1],
                                                 rLastSol.m_vEnd[1]))
                               ? TRUE
                               : FALSE ;
                  goto CombineDecided ;
                }

              // arrive here when Solution Intervals are separated by an opening on the curve.
              // See if curves are coincident throughout the solution opening

              // get SolutionOpening = curve parameter intervals from lastSol->End to nextSol->Start
              SmExtent1d sIvl1, sIvl2, sIvl3 ;
              sIvl1.AddValue((*pLastEnd)[0]) ; sIvl1.AddValue(rCurrStart[0]);
              sIvl2.AddValue((*pLastEnd)[1]) ; sIvl2.AddValue(rCurrStart[1]);
              sIvl3.AddValue((*pLastEnd)[2]) ; sIvl3.AddValue(rCurrStart[2]);

              // Evaluate and Drop a Curve1 SolutionOpening MidPoint to Surface
              SmPoint3d sPtOn1 ;
              SmPoint2d sGuess, sParam;
              double    dParam, dDist;
              SmBoolean bSuccess;
              SmBoolean bIsMulti;
              for(jj=0;jj<2;jj++)
                {
                  dParam = .33 * (jj+1) ; // test CurveGap params [.33  .66]
                  SER(crCurve.EvaluatePoint(sIvl1.Evaluate(dParam),sPtOn1));
                  sGuess.Set(sIvl2.Evaluate(dParam), sIvl3.Evaluate(dParam)) ;
                  SER(crSurface.DropPoint(sPtOn1, crUVDomain, &sGuess, bSuccess, sParam, dDist, bIsMulti));

                  // don't combine solutions when SolutionGap MidPoint does not drop or is too far away
                  if(  !bSuccess
                     || dDist > dDistanceTolerance)
                    { bDoCombine    = FALSE;
                      goto CombineDecided ;
                    }
                } // end 2 mid-point sample check

              // Arrive here when its likely the curves are coincident to
              // one another throughout the solutionOpening because their
              // MidPoints are within tolerance of each other.
              // Assume coincidence when the OpeningSize is small
              //   (the midPoint tests are sufficient)
              // Else assume coincidence when the curve/curve gapSize is less
              //   than tolerance at any Curve/Curve Gap Maximum within the
              //   Solution opening. (an extra expensive computation)

              // If SolutionOpening Interval is less than 1000th of curve intervals
              // assume the curves are coincident between solutions and combine
              if (   ( sIvl1.GetLength()     <  sTSize    / 1000.0 )
                  && (   ( sIvl2.GetLength() <  sUVSize.x / 1000.0 )
                      || ( sIvl3.GetLength() <  sUVSize.y / 1000.0 ) ) )
                {
                  bDoCombine = TRUE;
                  goto CombineDecided ;
                }

              else // SolutionOpening is not very small
                {
                  // We should solve for the max distance of the curve interval
                  // to the surface - but for now we'll just sample some more points
                  for(jj=0;jj<3;jj++)
                    {
                      // Evaluate and Drop a Curve1 SolutionOpening MidPoint to Surface
                      double dParameter = .25 * (jj+1) ; // test CurveGap params [.25  .50  .75]
                      SER(crCurve.EvaluatePoint(sIvl1.Evaluate( dParameter ),sPtOn1));
                      sGuess.Set(sIvl2.Evaluate( dParameter ), sIvl3.Evaluate( dParameter )) ;
                      SER(crSurface.DropPoint(sPtOn1, crUVDomain, &sGuess, bSuccess, sParam, dDist, bIsMulti));

                      // don't combine solutions when SolutionGap MidPoint does not drop or is too far away
                      if(  !bSuccess
                         || dDist > dDistanceTolerance)
                        { bDoCombine    = FALSE;
                          goto CombineDecided ;
                        }
                    }
                } // end SolutionOpening is not very small branch

              // arrive here after passing the gap between two interval solutions is coincident test
              bDoCombine = TRUE;
              goto CombineDecided ;

            } // end if range solution branch
        } // end test for merging solutions section


        // continue here after deciding whether to combine current and last solutions
CombineDecided:

      // when pair of solutions are marked for combining
      if (bDoCombine)
        {
          // Expand rLastSol interval to cover rCurrSol interval
          if (rCurrSol.m_eSolutionType == SM_ST_RANGE_OF_VALUES)
            {
              SM_ASSERT(rLastSol.m_eSolutionType == SM_ST_RANGE_OF_VALUES) ;
              if (rLastSol.m_vEnd[0] < rCurrSol.m_vEnd[0])
                {
                  rLastSol.m_vEnd = rCurrSol.m_vEnd;
                }
              SM_ASSERT(rLastSol.m_vStart[0] < rCurrSol.m_vStart[0]) ;
            }
          else // working with point solutions
            {
              // when combining a point solution we could
              // get the average solution but for now,
              // just use the better solution
              if ( rCurrSol.m_vStart.m_dSolutionValue < pLastEnd->m_dSolutionValue )
                {
                  rLastSol = rCurrSol ;
                }
              else
                {
                  // take no action - which throws away rCurrSol
                  // same thing as combining rLastSol and rCurrSol by
                  //   keeping just rLastSol.
                }
            }
        } // end bDoCombine == TRUE branch
      else // bDoCombine == FALSE branch
        {
          // Keep the current solution but squeeze it down
          // into the rSolutions array to keep the rSolutions array packed
          *pLastIndex =   lLastIntervalIndex == SM_BIG_ULONG ? lLastPointIndex + 1
                        : lLastPointIndex    == SM_BIG_ULONG ? lLastIntervalIndex + 1
                        : smos_Max(lLastIntervalIndex, lLastPointIndex) + 1 ;

          if(*pLastIndex != ii)
            { rSolutions.SetAt(*pLastIndex, rCurrSol); }
        } // end bDoCombine == FALSE branch

      //      // GWC: OBSOLETE SECTION
      //      // when its likely that two solution segments are coincident
      //      if (bDoCombine)
      //        {
      //          SmSolution & sLastSolution = rSolutions[lProcessedEnd];
      //          SmSolution & rCurrSolution = rSolutions[ii];
      //
      //          // See if we need to make an interval or just a single point
      //          sLastSolution.m_eSolutionType = SM_ST_RANGE_OF_VALUES;
      //
      //          // When ThisSol is a Range
      //          if (rCurrSolution.m_eSolutionType == SM_ST_RANGE_OF_VALUES)
      //            {
      //              // Set last Solution to be a range that includes ThisSol range
      //              if (!bSwapEnds) // when bSwapEnds == TRUE because of tolerances, no
      //                              // need to modify LastSolEnd - its already big enough.
      //                {
      //                  sLastSolution.m_vEnd = rCurrSolution.m_vEnd;
      //                  if (sLastSolution.m_vStart[0] > sLastSolution.m_vEnd[0])
      //                    {
      //                      SER(SM_ERR);
      //                    }
      //                }
      //            } // end ThisSol is a range branch
      //          else // ThisSolution is a point solution
      //            {
      //              // When LastSolStart/ThisSolStart are the same point in CurveDomain
      //              SmBoolean bSamePoint = smos_Fabs(sLastSolution.m_vStart[0]-rCurrSolution.m_vStart[0]) < SM_EFF_ZERO_SQRT ;
      //              if(bSamePoint)
      //                {
      //                  // Check LastSol/ThisSol CurvePoint 3d Distance
      //                  SmPoint3d sCurvePoint1, sCurvePoint2 ;
      //                  crCurve.EvaluatePoint(sLastSolution.m_vStart[0], sCurvePoint1) ;
      //                  crCurve.EvaluatePoint(rCurrSolution.m_vStart[0], sCurvePoint2) ;
      //                  double dDist = sCurvePoint1.DistanceBetween(sCurvePoint2) ;
      //                  double dScaledZero = 100.0 * SM_EFF_ZERO * (1.0 + sCurvePoint1.GetMaxDimension()) ;
      //                  bSamePoint = dDist < dScaledZero
      //                               ? TRUE
      //                               : FALSE ;
      //                } // end SolCurvePoint 3d Dist
      //
      //              // When LastSolStart/NextSolStart points are the same - make 1 point solution
      //              if (bSamePoint)
      //                {
      //                  sLastSolution.m_eSolutionType = SM_ST_SINGLE_VALUE;
      //                }
      //              else // expand LastSol range to include ThisSolPoint
      //                {
      //                  sLastSolution.m_vEnd = rCurrSolution.m_vStart;
      //                  // If have opposite sorting because of tolerancing
      //                  // swap the ends of sLastSolution.
      //                  if (sLastSolution.m_vStart[0] > sLastSolution.m_vEnd[0])
      //                    {
      //                      SmSolutionEnd sTmpEnd  = sLastSolution.m_vEnd;
      //                      sLastSolution.m_vEnd   = sLastSolution.m_vStart;
      //                      sLastSolution.m_vStart = sTmpEnd;
      //                    }
      //                } // end expand LastSol range to include ThisSol Point branch
      //            } // end ThisSol is a point branch
      //        } // end bDoCombine branch
      //      else // bDoCombine == FALSE branch
      //        {
      //          // Save this Solution, make it the last processed solution.
      //          // 1. Place solution in lProcessedEnd+1 rSolutions slot
      //          // 2. Increment the lProcessedEnd index value
      //          lProcessedEnd ++;
      //          SmSolution & rCurrSolution = rSolutions[ii];
      //          rSolutions.SetAt(lProcessedEnd,rCurrSolution);
      //        }
      //      GWC: End Obsolete section
    } // end iter every solution looking for solutions to combine of same type

  // Set rSolutions array Size after all combining has been done.
  if (rSolutions.GetSize() > 0)
    {
      ULONG lLastIndex =  lLastIntervalIndex == SM_BIG_ULONG ? lLastPointIndex
                        : lLastPointIndex    == SM_BIG_ULONG ? lLastIntervalIndex
                        : smos_Max(lLastIntervalIndex, lLastPointIndex) ;
      rSolutions.SetSize(lLastIndex+1);
    }

#ifdef SM_DEBUG_CODE
  if (bDebugMe)
    {
      rSolutions.Dump();
    }
#endif

  // Phase 2: remove coincident segments which include or are adjacent to
  // point intersections.  The assumption here is that nearly
  // tangent intersecting curves often will be found to lie
  // within a coincident segment.  The design choice is to
  // keep the intersection point and discard the coincident segment


  // Mark every interval solution next-to or containing a point solution for removal
  SmTArray<SmBoolean> bCullSolution(rSolutions.GetSize(), NULL, rSolutions.GetSize()) ;
  SmBoolean bCompress = FALSE ;

  // search for interval solutions
  for (ii=0; ii<rSolutions.GetSize(); ii++)
    {
      SmSolution & rInvSol = rSolutions[ii];
      bCullSolution[ii]    = FALSE ;

      // skip point-solutions solutions
      if(rInvSol.m_eSolutionType != SM_ST_RANGE_OF_VALUES)
        { continue; }

      SmSolutionEnd &rInvStart = rInvSol.m_vStart ;
      SmSolutionEnd &rInvEnd   = rInvSol.m_vEnd ;
      SmExtent1d sInvSpan1 ; sInvSpan1.AddValue(rInvStart[0]) ; sInvSpan1.AddValue(rInvEnd[0]) ;
      SmExtent1d sInvSpan2 ; sInvSpan2.AddValue(rInvStart[1]) ; sInvSpan2.AddValue(rInvEnd[1]) ;
      SmExtent1d sInvSpan3 ; sInvSpan2.AddValue(rInvStart[2]) ; sInvSpan2.AddValue(rInvEnd[2]) ;

      // search for point solutions within or adjacent to this interval solution
      for(jj=0; jj<rSolutions.GetSize(); jj++)
        {
          if(ii == jj) { continue ; }

          SmSolution &rPtSol = rSolutions[jj] ;

          // skip interval solutions
          if(rPtSol.m_eSolutionType != SM_ST_SINGLE_VALUE) { continue ; }
          SmSolutionEnd &rPtStart = rPtSol.m_vStart ;

          // Get the cache subdivision intervals containing this point
          SmTreeNode *pNode1 = rPtSol.m_apNodes[0] ;
          SmTreeNode *pNode2 = rPtSol.m_apNodes[1] ;

          // These asserts will cause a crash if violated,
          // so get and use Boolean values.
          SmBoolean bNode1Valid = (pNode1 && (   pNode1->m_eAuxDataType == SM_AD_BEZIER_CURVE
                                              || pNode1->m_eAuxDataType == SM_AD_CURVE_DATA)) ;
          SmBoolean bNode2Valid = (pNode2 && (   pNode2->m_eAuxDataType == SM_AD_BEZIER_SURFACE
                                              || pNode2->m_eAuxDataType == SM_AD_BEZIER_SURFACE_HEAD
                                              || pNode2->m_eAuxDataType == SM_AD_AUX_DATA)) ;
          SM_ASSERT( bNode1Valid );
          SM_ASSERT( bNode2Valid );

          // Note, if these are not valid, then we can't check whether they were
          // taken from adjacent nodes or not.  In that case, go ahead and check
          // the intervals: we'll be able to check by looking at the curve parameters.
          if ( bNode1Valid && bNode2Valid )
            {

              SmExtent1d sPtIvl1 = ((SmBezierAux1d *)pNode1->m_pData)->m_sIvl ;
              SmExtent1d sPtIvl2 = ((SmBezierAux2d *)pNode2->m_pData)->GetUVDomain().GetUInterval() ;
              SmExtent1d sPtIvl3 = ((SmBezierAux2d *)pNode2->m_pData)->GetUVDomain().GetVInterval() ;

              // check for overlap between IntervalSolution Span and PointSubDivision Span
              SmExtent1d sXSect1 ;
              SmExtent1d sXSect2 ;
              SmExtent1d sXSect3 ;
              sPtIvl1.Intersect(sInvSpan1, sXSect1) ;
              sPtIvl2.Intersect(sInvSpan2, sXSect2) ;
              sPtIvl3.Intersect(sInvSpan3, sXSect3) ;

              // when InvSpan is adjacent to or contains solution or its sub-division end-points
              if(   sXSect1.IsInit()
                 || sXSect2.IsInit()
                 || sXSect3.IsInit())
                { continue; }
            }
          else
            { SM_ASSERT( FALSE ); } // for breakpoint.

          // Now InvSpan is adjacent to or contains solution or its sub-division end-points.

          // Check whether this pair really looks like a suspicious range solution.
          // The correct behavior might be to delete the point solution instead,
          // or not do anything.
          //
          // (Programming note: this is checked here instead of down below
          // because here we have both the range and the point solution.)
          // (Also note: this check is the same as in SmCurve::GlobalCurveIntersect().
          // It was added there to fix a bug, and here for completeness.)
          //
          // Unfortunately, we have to get into heuristics here.
          // Sometimes when there is a true range solution (or even a point solution),
          // the curves will diverge very slowly outside of the solution, and the
          // global solver's caching mechanism will pick up an extraneous point
          // solution very near the true solution. [B98; my_test_cci_adv()]
          // That would be signalled by (heuristics here): the range solution
          // is more accurate, and the range solution is relatively long and the
          // point solution is very close to it.
          // So the heuristics will be:
          // - Keep the range solution if it's big enough, and
          // - remove the point solution if it's very close to one end of
          //   the range solution, and has a worse deviation.

          // Delete the range interval only if it's 'short': 1/1000 of the whole domain.
          double dCloseParam = crInterval.GetLength() / 1000;

          if ( sInvSpan1.GetLength() < dCloseParam )
          {
              // Delete the range interval.
              bCullSolution[ii] = TRUE;
              bCompress = TRUE;
          }
          else
          {
              // Range is long enough.  See about deleting the point solution.

              double dPtParam1 = rPtSol.m_vStart[0];
              if ( dPtParam1 < sInvSpan1.GetMin() )
              {
                  double dDist = sInvSpan1.GetMin() - dPtParam1;
                  if ( dDist < dCloseParam )
                  {
                      if ( rPtSol.m_vStart.m_dSolutionValue > 2.0 * rInvStart.m_dSolutionValue )
                      {
                          // Delete the point sol.
                          bCullSolution[jj] = TRUE;
                          bCompress = TRUE;
                      }
                  }
              }
              else if ( dPtParam1 > sInvSpan1.GetMax() )
              {
                  double dDist = dPtParam1 - sInvSpan1.GetMax();
                  if ( dDist < dCloseParam )
                  {
                      if ( rPtSol.m_vStart.m_dSolutionValue > 2.0 * rInvEnd.m_dSolutionValue )
                      {
                          // Delete the point sol.
                          bCullSolution[jj] = TRUE;
                          bCompress = TRUE;
                      }
                  }
              }
              else
              {
                  // Point sol is inside the range sol: delete it.
                  bCullSolution[jj] = TRUE;
                  bCompress = TRUE;
              }
          } // end else: range is long enough


#ifdef SM_DEBUG_CODE
          if ( bCullSolution[ii] || bCullSolution[jj] ) {
              if(bDebugMe)
                {
                  SmSolution &rIvlSolution = rSolutions[ii] ;  // interval solution
                  SmSolution &rPtSolution  = rSolutions[jj] ;  // point    solution

                  rIvlSolution.Dump();
                  rPtSolution.Dump();

                  SmFace *pFace1 = (SmFace *)crSurface.GetFace() ;
                  SmEdge *pEdge2 = (SmEdge *)crCurve.GetEdge() ;
                  SmBrep *pBrep1 = pFace1 ? pFace1->GetBrep() : NULL ;
                  SmBrep *pBrep2 = pEdge2 ? pEdge2->GetBrep() : NULL ;

                  smgfx_Erase();
                  smgfx_SetLook(1,2, 0,0,1); if(pBrep1) pBrep1->Draw(TRUE) ; sm_GraphicsLoop() ;
                  smgfx_SetLook(1,2, 0,1,0); if(pBrep2) pBrep2->Draw(TRUE) ; sm_GraphicsLoop() ;
                  smgfx_SetLook(3,5, 0,1,1); crSurface.Draw(); sm_GraphicsLoop();
                  smgfx_SetLook(4,6, 0,1,0); crCurve.Draw(); sm_GraphicsLoop();
                  smgfx_SetLook(7,8, 1,0,0); rIvlSolution.Draw() ; sm_GraphicsLoop() ;
                  smgfx_SetLook(7,8, 1,0,0); rPtSolution.Draw() ; sm_GraphicsLoop() ;
                  sm_GraphicsLoop();
                }
          }
#endif // SM_DEBUG_CODE

          if ( bCullSolution[ii] )
            { break; }


          // quit searching when point solutions get past interval solutions
          if(   rPtStart[0] > sInvSpan1.GetMax()
             || rPtStart[1] > sInvSpan2.GetMax()
             || rPtStart[2] > sInvSpan3.GetMax())
            { break ; }
        } // end iter solutions looking for points
    } // end iter solutions looking for intervals

  // compress the solutions array to remove all unwanted interval solutions
  if(bCompress)
    {
      ULONG lNextIndex = 0 ;
      for(ii=0;ii<rSolutions.GetSize();ii++)
        {
          // omit solutions to drop
          if(bCullSolution[ii]) { continue ; }

          // move current solution to next open rSolutions array spot
          if(ii != lNextIndex)
            {  rSolutions[lNextIndex] = rSolutions[ii] ; }
          lNextIndex++ ;
        } // end iter every solution

      // Set rSolutions array Size after discarding unwanted interval solutions is done.
      if (rSolutions.GetSize() > 0)
        {
          rSolutions.SetSize(lNextIndex);
        }
    } // end need to compress array check

#ifdef SM_DEBUG_CODE
  if(bDebugMe)
    {
      rSolutions.Dump();
    }
#endif

//        // GWC: OBSOLETE SECTION
//        // Here is where we need to do some post processing to see if we have
//        // coincidences.
//
//        // Let's look through the intersections and see if we have coincidences
//        // between them.  If we do then we need to combine the coincident
//        // segments in some cases.  In other cases we may need to split them.
//        // In the future we could expand the intersections to cover all
//        // areas where the curve is within tolerance of the surface.
//
//        SmVector2d sUVSize = crUVDomain.GetSize();
//
//        ULONG lProcessedEnd = 0;
//
//        // for every rSolution after the 1st
//        // - see if it can be combined with the lastProcessed solution
//        for (ULONG i=1; i<rSolutions.GetSize(); i++)
//          {
//            SmBoolean bDoCombine = FALSE;
//            SmBoolean bSwapEnds  = FALSE;
//
//            // now decide if lProcessedEnd solution should be combined with next solution
//            // 1. Find solution closest to the last lProcessedEnd solution measuring dist between sol StartPoints
//            // 2. Place it in the next SolutionsArray Slot
//            // 3. Don't Combine ProcessedEnd/Next sols on opposite sides of a Surface seam
//            // 4. Don't Combine ProcessedEnd/Next sols not sharing the same curve param (opposite ends of closed curves)
//            // 5. Get ProcessedEnd/NextStart Curve Interval gap.
//            //      Drop 2 Curve Interval gap NearMidPoints to surface.
//            //        If both drop assume a coincident interval and Combine,
//            //        else Don't Combine
//              {
//                // Get Solution endParameter
//                SmSolution    & rLastSol = rSolutions[lProcessedEnd];
//                SmSolutionEnd * pLastEnd = (rLastSol.m_eSolutionType == SM_ST_RANGE_OF_VALUES)
//                                           ? &rLastSol.m_vEnd
//                                           : &rLastSol.m_vStart;
//
//                // init ClosestSolution to this solution (closed curve solutions?)
//                ULONG  lClosest = i;
//                double dScaledZero   = SM_EFF_ZERO * (1.0 + smos_Fabs(rSolutions[i].m_vStart[0]));
//
//                // for every other solution - Look for other solutions that start where this one starts
//                for (ULONG j=i+1; j<rSolutions.GetSize(); j++)
//                  {
//                    // when a solution starts at the same CurveParam
//                    if (smos_Fabs(rSolutions[lClosest].m_vStart[0]-rSolutions[j].m_vStart[0]) < dScaledZero)
//                      {
//                        // when OtherStart/LastEnd is closer in SurfUVSpace than ClosestStart/LastEnd UVDist
//                        double dUDiff1 = smos_Fabs(rSolutions[lClosest].m_vStart[1]-(*pLastEnd)[1]);
//                        double dVDiff1 = smos_Fabs(rSolutions[lClosest].m_vStart[2]-(*pLastEnd)[2]);
//                        double dUDiff2 = smos_Fabs(rSolutions[j].m_vStart[1]-(*pLastEnd)[1]);
//                        double dVDiff2 = smos_Fabs(rSolutions[j].m_vStart[2]-(*pLastEnd)[2]);
//
//                        if (dUDiff2+dVDiff2 < dUDiff1+dVDiff1)
//                          {
//                            // make ThisSolution the closest solution
//                            lClosest = j;
//                          }
//                      }
//                    else // quit at the first worst solution
//                      { break; }
//                  } // end iter every otherSolution matching up endPoints
//
//                // when a closer OtherSolution was found - place it in the next SolutionsArray Slot
//                if (lClosest != i)
//                  {
//                    // Swap lClosest and i - the original sorting order was messed up
//                    SmSolution sTmpSol   = rSolutions[lClosest];
//                    rSolutions[lClosest] = rSolutions[i];
//                    rSolutions[i]        = sTmpSol;
//                  }
//
//                // get current Solution StartParam
//                SmSolution    & rCurrSol   = rSolutions[i];
//                SmSolutionEnd & sCurrStart = rCurrSol.m_vStart;
//
//                // Don't allow jumping over seam -
//                //  detect seam jumping by large UVdistances between matched endPoints
//
//                // Get UVDist Vector between Surface solution endPoints
//                SmVector2d sUVDiff(smos_Fabs(sCurrStart[1]-(*pLastEnd)[1]),
//                                   smos_Fabs(sCurrStart[2]-(*pLastEnd)[2]) );
//
//                // Get TDist between Curve solution endPoints
//                double sTDiff = smos_Fabs(sCurrStart[0]-(*pLastEnd)[0]);
//
//                // when the Surface endPoints are separated by the domain space
//                //      and the Curve endPoints are coincident
//                if(   (   smos_Fabs(sUVDiff.x-sUVSize.x) < SM_EFF_ZERO_SQRT
//                       || smos_Fabs(sUVDiff.y-sUVSize.y) < SM_EFF_ZERO_SQRT)
//                   && (sTDiff < SM_EFF_ZERO_SQRT))
//                  {
//                    // don't connect solutions across Surface seams
//                    bDoCombine = FALSE ;
//                    goto CombineDecided ;
//                  }
//
//                // When the Curve solution endPoints are not near zero to one another
//
//                // when Last/Next solutions are not close on the Curve
//                if ((*pLastEnd)[0] > sCurrStart[0]+SM_EFF_ZERO_SQRT)
//                  { // Have a case of curve falling along seam or something
//                    // nasty like that - don't combine
//                    bDoCombine = FALSE;
//                    goto CombineDecided ;
//                  }
//
//                // see if interval from LastEnd to NextStart is coincident
//
//                // remember when LastEnd is Greater than CurrentStart (overlapping solutions)
//                if ((*pLastEnd)[0] > sCurrStart[0])
//                  { bSwapEnds = TRUE;
//                  }
//
//                // get LastEnd to CurrStart Curve Interval
//                SmExtent1d sIvl1((*pLastEnd)[0]);
//                sIvl1.AddValue(sCurrStart[0]);
//
//                // get LastEnd's associated surface UVPoint
//                SmPoint2d sUVPnt((*pLastEnd)[1], (*pLastEnd)[2]);
//
//                // Drop CurveInterval Near MidPoint to Surface
//                SmPoint3d sPtOn1;
//                SmSolution sSolData[16];
//                SmSolutionArray sSolutions(16,sSolData);
//                SER(crCurve.EvaluatePoint(sIvl1.Evaluate(0.45678),sPtOn1));
//                SER(GlobalPointSolve(crUVDomain, SM_SO_INTERSECT,
//                                     sPtOn1, dDistanceTolerance,
//                                     NULL, SM_SR_ALL, sSolutions));
//
//                // Skip combining when Curve LastEnd/NextStart Interval NearMidPoints not dropping to surface
//                if (sSolutions.GetSize() < 1)
//                  { // don't combine solutions separated by a nonCoincident Curve Gap
//                    bDoCombine = FALSE;
//                    goto CombineDecided ;
//                  }
//
//                // when Curve LastEnd/NextStart Interval near midPoint dropped to Surface
//                // drop 2nd LastEnd/NextStart Interval near midPoint to surface
//                SER(crCurve.EvaluatePoint(sIvl1.Evaluate(0.65432),sPtOn1));
//                SER(GlobalPointSolve(crUVDomain,SM_SO_INTERSECT,
//                                     sPtOn1, dDistanceTolerance,
//                                     NULL, SM_SR_ALL, sSolutions));
//
//                // Skip combining when Curve LastEnd/NextStart Interval NearMidPoints not dropping to surface
//                if (sSolutions.GetSize() < 1)
//                  { // don't combine solutions separated by a nonCoincident Curve Gap
//                    bDoCombine = FALSE;
//                    goto CombineDecided ;
//                  }
//
//                // when 2 LastEnd/ThisStart CurveInterval near midPoints drop to Surface
//                // assume LastEnd/ThisStart CurveInterval is coincident to Surface and
//                // combine solutions
//                bDoCombine = TRUE;
//              } // end decide to combine block
//
//              // continue here after deciding whether to combine current and last solutions
//      CombineDecided:
//
//            // when its likely that two solution segments are coincident
//            if (bDoCombine)
//              {
//                SmSolution & sLastSolution = rSolutions[lProcessedEnd];
//                SmSolution & rCurrSolution = rSolutions[i];
//
//                // See if we need to make an interval or just a single point
//                sLastSolution.m_eSolutionType = SM_ST_RANGE_OF_VALUES;
//
//                // When ThisSol is a Range
//                if (rCurrSolution.m_eSolutionType == SM_ST_RANGE_OF_VALUES)
//                  {
//                    // Set last Solution to be a range that includes ThisSol range
//                    if (!bSwapEnds) // when bSwapEnds == TRUE because of tolerances, no
//                                    // need to modify LastSolEnd - its already big enough.
//                      {
//                        sLastSolution.m_vEnd = rCurrSolution.m_vEnd;
//                        if (sLastSolution.m_vStart[0] > sLastSolution.m_vEnd[0])
//                          {
//                            SER(SM_ERR);
//                          }
//                      }
//                  } // end ThisSol is a range branch
//                else // ThisSolution is a point solution
//                  {
//                    // When LastSolStart/ThisSolStart are the same point in CurveDomain
//                    SmBoolean bSamePoint = smos_Fabs(sLastSolution.m_vStart[0]-rCurrSolution.m_vStart[0]) < SM_EFF_ZERO_SQRT ;
//                    if(bSamePoint)
//                      {
//                        // Check LastSol/ThisSol CurvePoint 3d Distance
//                        SmPoint3d sCurvePoint1, sCurvePoint2 ;
//                        crCurve.EvaluatePoint(sLastSolution.m_vStart[0], sCurvePoint1) ;
//                        crCurve.EvaluatePoint(rCurrSolution.m_vStart[0], sCurvePoint2) ;
//                        double dDist = sCurvePoint1.DistanceBetween(sCurvePoint2) ;
//                        double dScaledZero = 100.0 * SM_EFF_ZERO * (1.0 + sCurvePoint1.GetMaxDimension()) ;
//                        bSamePoint = dDist < dScaledZero
//                                     ? TRUE
//                                     : FALSE ;
//                      } // end SolCurvePoint 3d Dist
//
//                    // When LastSolStart/NextSolStart points are the same - make 1 point solution
//                    if (bSamePoint)
//                      {
//                        sLastSolution.m_eSolutionType = SM_ST_SINGLE_VALUE;
//                      }
//                    else // expand LastSol range to include ThisSolPoint
//                      {
//                        sLastSolution.m_vEnd = rCurrSolution.m_vStart;
//                        // If have opposite sorting because of tolerancing
//                        // swap the ends of sLastSolution.
//                        if (sLastSolution.m_vStart[0] > sLastSolution.m_vEnd[0])
//                          {
//                            SmSolutionEnd sTmpEnd  = sLastSolution.m_vEnd;
//                            sLastSolution.m_vEnd   = sLastSolution.m_vStart;
//                            sLastSolution.m_vStart = sTmpEnd;
//                          }
//                      } // end expand LastSol range to include ThisSol Point branch
//                  } // end ThisSol is a point branch
//              } // end bDoCombine branch
//            else // bDoCombine == FALSE branch
//              {
//                // Save this Solution, make it the last processed solution.
//                // 1. Place solution in lProcessedEnd+1 rSolutions slot
//                // 2. Increment the lProcessedEnd index value
//                lProcessedEnd ++;
//                SmSolution & rCurrSolution = rSolutions[i];
//                rSolutions.SetAt(lProcessedEnd,rCurrSolution);
//              }
//
//          } // end iter every solution looking for solutions to combine
//
//        // Set rSolutions array Size after all combining has been done.
//        if (rSolutions.GetSize() > 0)
//          {
//            rSolutions.SetSize(lProcessedEnd+1);
//          }
//        // GWC: END OBSOLETE SECTION

#ifdef SM_DEBUG_CODE
  if(bDebugMe)
    {
      rSolutions.Dump();
    }
#endif

  // check each coincident interval solution to see if it should be treated as a point solution
  // if     the interval is very short
  //   then change it into a point solution
  // if     One end solutionValue is zero and
  //        the GapFunction diverges at greater than linear rate
  //        to the other end and the other end solution
  //        is near tolerance - assume this is a 'coincident maybe' segment
  //   then replace the interval with a point solution.
  // if     the interval contains a local curve/curve distance minimum
  //        or an exact intersection
  //   then split the interval at that point of interest.
  //
  SmBoolean bDone2 = FALSE;
  while (!bDone2)
    {
      // We are done unless we split something.
      bDone2 = TRUE;

      // locals: curve/solution metrics
      double     dLength        = SM_BIG_DOUBLE ;
      double     dSegLeng       = SM_BIG_DOUBLE ;
      SmBoolean  bLargeInterval = FALSE ;
      SmBoolean  bSmallInterval = FALSE ;
      SmBoolean  bIsDegenerate  = FALSE ;

      // for every SM_ST_RANGE_OF_VALUES solution
      for (ii=0; ii<rSolutions.GetSize(); ii++)
        {
          SmSolution & rCurrSol = rSolutions[ii];

          // skip SM_ST_SINGLE_VALUE solutions
          if (rCurrSol.m_eSolutionType != SM_ST_RANGE_OF_VALUES)
            { continue; }

          // locals - intersection StartPoint, Start/End Deviations, Interval ParameterLengths
          SmPoint3d   sPnt;
          double      dDevStart   = rCurrSol.m_vStart.m_dSolutionValue;
          double      dDevEnd     = rCurrSol.m_vEnd.m_dSolutionValue;
          SmExtent1d  sIvl (rCurrSol.m_vStart[0]) ;
          SmExtent2d  sReg(SmPoint2d(rCurrSol.m_vStart[1],rCurrSol.m_vStart[2])) ;
          sIvl.AddValue(rCurrSol.m_vEnd[0]) ;
          sReg.AddPoint2d(SmPoint2d(rCurrSol.m_vEnd[1],rCurrSol.m_vEnd[2])) ;
          double      dIvlLength  = sIvl.GetLength() ;
          double      dRegLength  = sReg.GetSize().Length() ;
          SER(crCurve.EvaluatePoint(rCurrSol.m_vStart[0],sPnt));

          // locals: Curve 3d and solution range lengths
          if(dLength == SM_BIG_DOUBLE) { dLength = crCurve.ApproximateLength(crInterval,5); }
          dSegLeng = crCurve.ApproximateLength(sIvl,5);

          // Scaled Zeros in 3D and both Parameter Spaces
          double dScaledZero   = SM_EFF_ZERO * (1.0 + sPnt.GetMaxDimension());
          double dScaledPSTol  = 100.0 * SM_EFF_ZERO * (1.0 + crInterval.GetMaxDimension()) ;
          double dScaledPSTol2 = 100.0 * SM_EFF_ZERO * (1.0 + crUVDomain.GetMaxDimension()) ;

          // keep nonZero intervals whose ends are exact intersections to ScaledZero
          if (   dDevStart  < dScaledZero
              && dDevEnd    < dScaledZero
              && dIvlLength > dScaledPSTol
              && dRegLength > dScaledPSTol2)
            { continue; }

          // See if solution range is a large interval
          //   if it is at least 1/10 the length of the shorter curve.
          bLargeInterval = dSegLeng > dLength/10.0 ;

          // See if solution range is a small interval
          //   if it is less than ScaledZero in either curve domain
          bSmallInterval = (   (dIvlLength < dScaledPSTol)
                            || (dRegLength < dScaledPSTol2))
                           ? TRUE
                           : FALSE ;

          // for nonDegenerate intervals
          if(!bSmallInterval)
            {
              // if GapVector is diverging over length of solution range
              //  change solution to type SM_ST_SINGLE_VALUE for a tangent point intersection
              // note: side effect: rCurrSol.m_vStart is modified when bIsDegenerate == TRUE
              crSurface.ClassifyIntersectingRangeSolution(crCurve,
                                                          rCurrSol,
                                                          dDistanceTolerance,
                                                          bIsDegenerate,
                                                          rCurrSol.m_vStart) ;
            }

          // keep interval if it is at least 1/10 the length of the shorter curve.
          if (   bLargeInterval
              && !bIsDegenerate)
            {
              // gwc: This keep intervals 1/10th the length heuristic
              //      was here before the addition of the
              //      ClassifyIntersectingRangeSolution() check.
              //      Signal when these two heuristics differ so that
              //      we can come back and analyze when this heuristic should
              //      be used if ever.
              continue;
            }

          // Change very small and diverging intervals into a single pt solutions.
          if (   bSmallInterval
              || bIsDegenerate)
            {
              // load the solution with smallest deviation into m_vStart
              if (rCurrSol.m_vStart.m_dSolutionValue > rCurrSol.m_vEnd.m_dSolutionValue)
                { rCurrSol.m_vStart = rCurrSol.m_vEnd; }

              // change the solutionType
              rCurrSol.m_eSolutionType = SM_ST_SINGLE_VALUE;
              continue;
            }  // end interval small enough to be changed into a point solution check

/* // gwc: don't need to split intervals because we replace all
   //      interval solutions with DropAndTrimCurve() solutions in the next section
   //      NOTE: on second reading - I see that we don't always replace all interval
   //      solutions with DropAndTrimCurve() solutions. When the maximum curve/surface
   //      distance is found to exceed tolerance, then the current interval solutions
   //      are returned as the output.  We may have to add this section back into the
   //      code.  For now, leave it out until we can run prog_test.

          // Check to see if there is a
          // minimum value where the solution needs to be split.
          // if we found a minimum and it is less than 1/2 of the end
          // values then we will call it a splitting value
          double dSplit1, dSplit2, dSplit3;
          double dSplitDev;
          SmBoolean bSplit = FALSE;

          // when endDeviations are larger than zero
          if (   dDevStart > dScaledZero
              && dDevEnd   > dScaledZero)
            {
              SmSolution sMinSolution;
              SmBoolean bFoundSolution = FALSE;

              // look for a local distance minimum within this interval
              SER(LocalCurveSolve(sReg, crCurve, sIvl,
                                  SM_SO_MINIMIZE, dDistanceTolerance,
                                  NULL, NULL,
                                  sReg.Evaluate(0.5, 0.5), sIvl.Evaluate(0.5),
                                  bFoundSolution, sMinSolution));

              // when a solution with a better deviation than endPoint deviations was found
              if (   bFoundSolution
                  &&   sMinSolution.m_vStart.m_dSolutionValue
                     < smos_Min(dDevStart,dDevEnd)/2.0)
                {
                  // set the split values and split flag
                  dSplit1   = sMinSolution.m_vStart[0];
                  dSplit2   = sMinSolution.m_vStart[1];
                  dSplit3   = sMinSolution.m_vStart[2];
                  dSplitDev = sMinSolution.m_vStart.m_dSolutionValue;
                  bSplit    = TRUE;
                } // end found a tighter local distance point check
            } // end endDeviation larger than zero check

          // If either end deviation is greater than zero
          // then split interval near any interior intersections near nonZero deviation.
          if (   !bSplit
              && (   dDevStart > dScaledZero
                  || dDevEnd   > dScaledZero))
            {
              // let guesses = largest deviation end solution
              double dGuess1, dGuess2, dGuess3;
              if (dDevStart > dDevEnd) { dGuess1 = rCurrSol.m_vStart[0];
                                         dGuess2 = rCurrSol.m_vStart[1];
                                         dGuess3 = rCurrSol.m_vStart[2];
                                       }
              else                     { dGuess1 = rCurrSol.m_vEnd[0];
                                         dGuess2 = rCurrSol.m_vEnd[1];
                                         dGuess3 = rCurrSol.m_vEnd[2];
                                       }

              // LocalCurveIntersect Locals
              SmBoolean bFound;
              double dMyParam, dDeviation;
              SmPoint2d sUVParam ;

              // look for an intersecton
              SER(LocalCurveIntersect(sReg, crCurve, sIvl, dDistanceTolerance,
                                      SmPoint2d(dGuess2, dGuess3), dGuess1,
                                      bFound, sUVParam,  dMyParam, dDeviation));

              if (   bFound
                  && dDeviation < smos_Min(dDevStart,dDevEnd) / 2.0)
                {
                  // Only split if the split point is not on the end of the curve
                  double dSegDist = sIvl.GetMax() - sIvl.GetMin();
                  if (   dMyParam > sIvl.GetMin() + dSegDist/100.0
                      && dMyParam < sIvl.GetMax() - dSegDist/100.0)
                    {
                      // set the split values and flag
                      dSplit1   = dMyParam;
                      dSplit2   = sUVParam.x ;
                      dSplit3   = sUVParam.y ;
                      dSplitDev = dDeviation;
                      bSplit    = TRUE;
                    } // end new solution not on end of interval check
                } // end found an intersection with smaller deviations check
            }  // end endDeviation larger than zero and not yet split check

          // Split the rCurrSol and put the new solution before rCurrSol
          // Then quit and start loop again.
          if (bSplit)
            {
              // make a nwe solution copy of the current solution
              SmSolution sNewSol                = rCurrSol;

              // trim current solution interval to [SplitPoint, End]
              rCurrSol.m_vStart[0]               = dSplit1;
              rCurrSol.m_vStart[1]               = dSplit2;
              rCurrSol.m_vStart[2]               = dSplit3;
              rCurrSol.m_vStart.m_dSolutionValue = dSplitDev;

              // make new solution interval [Start, SplitPoint]
              sNewSol.m_vEnd[0]               = dSplit1;
              sNewSol.m_vEnd[1]               = dSplit2;
              sNewSol.m_vEnd[2]               = dSplit3;
              sNewSol.m_vEnd.m_dSolutionValue = dSplitDev;

              // Add new solution to rSolutions array in front of current solution
              rSolutions.InsertAt(ii,sNewSol,1);

              // restart the coincident segment check loop
              bDone2 = FALSE;
              break;
            } // end split requested check
*/ // end removed split intervals section because interval solutions are going to be replaced
        } // end iter every interval solution looking for cases to be turned into point solutions
    } // end while (!bDone2)

  // Replace coincident curve/surface solutions with dropCurve Solutions
  // gwc: I suspect this is needed to accurately find trim points for coincident
  //      curves.  The Local solver finds a point of intersection
  //      somewhere within each leaf node of the curve's subdivision intervals.
  //      These points do not necessarily mark the boundaries where the curve
  //      meanders beyond the surface's domain boundaries.
  //
  //   1. Save Far From Tangent Point solutions
  //   2. Drop Curve to Surface within dDistanceTolerance with DropAndTrimCurve()
  //   3. If Curve fails to Drop - use current Solution set and return
  //      else replace current solutions with
  //       a. Saved Far From Tangent Point Solutions plus
  //       b. an intervalSolution for every DroppedAndTrimmed Curve Segment.
  //       and return.

  // for every solution - look for a NonDegenerate RangeOfValues solution
  for (ii=0; ii<rSolutions.GetSize() && bFoundCoincidence == FALSE; ii++)
    {
      SmSolution & rSol = rSolutions[ii];
      if (rSol.m_eSolutionType == SM_ST_RANGE_OF_VALUES)
        {
          if (rSol.m_vEnd[0]-rSol.m_vStart[0] > 0.02 * crInterval.GetLength())
            {
              bFoundCoincidence = TRUE;
            }
        }
    } // end iter every solution looking for a NonDegenerate RangeOfValues solution

  // when a coincident point solution or a NonDegenerate RangeOfValues solution was found
  if (bFoundCoincidence)
    {
      // place all FarFromTangent Point Solutions on TmpArray
      SmSolution sTmpData[16];
      SmSolutionArray sTmpSols(16,sTmpData);

      // for every SM_ST_SINGLE_VALUE solution
      for (ULONG iii=0; iii<rSolutions.GetSize(); iii++)
        {
          SmSolution & rSol = rSolutions[iii];
          if (rSol.m_eSolutionType == SM_ST_SINGLE_VALUE)
            {
              // gwc: we could modify this section to use the following check
              //      and keep all non SM_TC_COINCIDENT solutions
              //      // for solutions which are further apart
              //      //  see if points bound a coincident segment region
              //      sCurrentUV.Set(rSolutions[0].m_vStart[1], rSolutions[0].m_vStart[2]) ;
              //      dCurrentT = rSolutions[0].m_vStart[0] ;
              //      sm_ClassifyCurveSurfaceXSectPoint(*this, crUVDomain,
              //                                         crCurve, crInterval,
              //                                         sCurrentUV, dCurrentT,
              //                                         bFoundGuess, sNewUVGuess, dNewTGuess,
              //                                         ePointType) ;
              //      if(ePointType != SM_TC_COINCIDENT)
              //        {
              //          sTmpSols.Add(rSol);
              //        }

              // Keep Curve/Surface through intersections
              //  (CurveTangent/SurfaceNormal angle > 2.0 degrees)
              SmVector3d sPV[2]; // sPV[0] = crCurve 3dPoint
                                 // sPV[1] = crCurve 3dTangent
              SER(crCurve.Evaluate(rSol.m_vStart[0],1,TRUE,sPV,TRUE)); // NonZeroTangent values
              SmPoint2d sUV(rSol.m_vStart[1],rSol.m_vStart[2]);
              SmVector3d sNormal;
              SER(crSurface.EvaluateNormal(sUV,TRUE,TRUE,sNormal));

              if (!sPV[1].IsPerpendicularTo(sNormal,2.0))
                {
                  // place intersection in tmpOutput
                  sTmpSols.Add(rSol);
                }
            }
        } // end keeping every NONRangeOfValues Solution not tangent with the surface

      // drop curve locals
      SmTArray<SmBSplineCurve*> sUVCurves;
      SmObjsDelete<SmBSplineCurve*> sUVClean(&sUVCurves);
      double dDistToSurface, dDeviation;

      const SmBSplineCurve* pBSC = SM_CAST_NONNULL_PTR(SmBSplineCurve, &crCurve);

      // drop and trim the 3DCurve to the Surface
      SER(crSurface.DropAndTrimCurve(*crSurface.GetContext(), crUVDomain, *pBSC, crInterval, dDistanceTolerance,
                                     dDistToSurface, dDeviation, sUVCurves, TRUE, TRUE));

      //Potential solution to CrvOnSurf edge curve (suspect)
      //const SmCurve * pC = &crCurve;

      //// drop and trim the 3DCurve to the Surface
      //SER(crSurface.DropAndTrimCurve(*crSurface.GetContext(), crUVDomain,
      //                               *pC, crInterval, dDistanceTolerance,
      //                               dDistToSurface, dDeviation, sUVCurves, TRUE, TRUE ));

      // when the curve/surface 3dDistance is beyond tolerance
      if (dDistToSurface > dDistanceTolerance)
        {
          // exit - using all the solutions already in sSolutions array as the output.
          return SM_SUCCESS;
        }

      // else when the DropAndTrimCurve DistToSurface is within tolerance
      //   add a range solution to sTmpSols for every DropAndTrimCurve output curve
      //   and use sTmpSols as the output.

      // for every projection curve segment
      for (ULONG jjj=0; jjj<sUVCurves.GetSize(); jjj++)
        {
          // add a RangeOfValues solution to the tmpOutput
          SmCurve *pUVCurve    = sUVCurves[jjj];
          SmExtent1d sIvl      = pUVCurve->GetNaturalInterval();
          SmSolution sSol;
          sSol.m_lNumVariables = 3;
          sSol.m_lNumObjects   = 2;
          sSol.m_apObjects[0]  = (SmObject *)&crCurve  ;
          sSol.m_apObjects[1]  = (SmObject *)&crSurface ;
          sSol.m_eSolutionType = SM_ST_RANGE_OF_VALUES;

          SmPoint3d sPnt;
          SER(pUVCurve->EvaluatePoint(sIvl.GetMin(),sPnt));
          sSol.m_vStart.m_dSolutionValue = dDistToSurface;
          sSol.m_vStart[0]               = sIvl.GetMin();
          sSol.m_vStart[1]               = sPnt.x;
          sSol.m_vStart[2]               = sPnt.y;

          SER(pUVCurve->EvaluatePoint(sIvl.GetMax(),sPnt));
          sSol.m_vEnd.m_dSolutionValue = dDistToSurface;
          sSol.m_vEnd[0]               = sIvl.GetMax();
          sSol.m_vEnd[1]               = sPnt.x;
          sSol.m_vEnd[2]               = sPnt.y;

          sTmpSols.Add(sSol);

        } // end making every dropCurve solution a RangeOfValues solution

      // When curve dropped to surface - replace rSolutions with TmpSols
      if (sUVCurves.GetSize() != 0)
        {
          rSolutions.ReSet();
          rSolutions.Append(sTmpSols);
        }

      return SM_SUCCESS;

    } // end a Coincident point or NonDegenerate RangeOfValues solution was found check

#ifdef SM_DEBUG_CODE
  if (bDebugMe)
    { rSolutions.Dump() ; }
#endif
  // all done
  return SM_SUCCESS ;

} // end sm_GlobalCurveIntersectCombineSolutions
// ---- end verbatim
// clang-format on
} // namespace sc_srf_p10

namespace
{
struct Bad : std::runtime_error
{
    Bad() : std::runtime_error("bad program") {}
};

struct Rd
{
    const double* in;
    int64_t n;
    int64_t at;
    double num()
    {
        if (at >= n)
            throw Bad();
        return in[at++];
    }
    int64_t idx(int64_t lo, int64_t hi)
    {
        double d = num();
        if (!(d >= (double)lo && d <= (double)hi) || d != (double)(int64_t)d)
            throw Bad();
        return (int64_t)d;
    }
    bool flag() { return idx(0, 1) != 0; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
    SmVector2d uv()
    {
        double x = num();
        double y = num();
        return SmVector2d(x, y);
    }
    SmExtent2d dom4()
    {
        double a = num();
        double b = num();
        double c = num();
        double d = num();
        return SmExtent2d(a, b, c, d);
    }
};

double st(SmStatus s)
{
    return s == SM_SUCCESS ? 0.0 : (double)s;
}

// `n m p q r s cpts(x y z w) u v`, sizes checked by the Rust side.
SmBSplineSurface* read_bspline_surface(Rd& rd, const SmContext& ctx)
{
    gw_INDEX n = (gw_INDEX)rd.idx(1, 16);
    gw_INDEX m = (gw_INDEX)rd.idx(1, 16);
    gw_DEGREE p = (gw_DEGREE)rd.idx(1, 8);
    gw_DEGREE q = (gw_DEGREE)rd.idx(1, 8);
    gw_INDEX r = (gw_INDEX)rd.idx(3, 40);
    gw_INDEX s = (gw_INDEX)rd.idx(3, 40);
    if (p > n || q > m || r != n + p + 1 || s != m + q + 1)
        throw Bad();
    std::vector<double> v;
    for (int64_t i = 0; i < 4 * (n + 1) * (m + 1) + r + 1 + s + 1; i++)
        v.push_back(rd.num());
    gw_SURFACE* sur = sm_AllocateNurbSurface(n, m, p, q, r, s);
    size_t k = 0;
    for (gw_INDEX i = 0; i <= n; i++)
        for (gw_INDEX j = 0; j <= m; j++)
        {
            sur->net->Pw[i][j].x = v[k++];
            sur->net->Pw[i][j].y = v[k++];
            sur->net->Pw[i][j].z = v[k++];
            sur->net->Pw[i][j].w = v[k++];
        }
    for (gw_INDEX i = 0; i <= r; i++)
        sur->knu->U[i] = v[k++];
    for (gw_INDEX i = 0; i <= s; i++)
        sur->knv->U[i] = v[k++];
    return new (ctx) SmBSplineSurface(sur, FALSE, &ctx);
}

// `0 bspline-surface` or `1 o x y scale[2] dom[4]` (SmPlane(o, x, y, scale, dom, ctx)).
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx(0, 1);
    if (kind == 0)
        return read_bspline_surface(rd, ctx);
    SmPoint3d o = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    SmVector2d s = rd.uv();
    SmExtent2d d = rd.dom4();
    return new (ctx) SmPlane(o, x, y, s, d, &ctx);
}

// A 3D B-spline curve `n p m cpts(x y z w) knots`.
SmBSplineCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t n = rd.idx(1, 32);
    int64_t p = rd.idx(1, 8);
    int64_t m = rd.idx(3, 64);
    if (p > n || m != n + p + 1)
        throw Bad();
    std::vector<double> v;
    for (int64_t i = 0; i < 4 * (n + 1) + m + 1; i++)
        v.push_back(rd.num());
    gw_CURVE* cur = sm_AllocateNurbCurve((gw_INDEX)n, (gw_DEGREE)p, (gw_INDEX)m);
    for (int64_t i = 0; i <= n; i++)
    {
        cur->pol->Pw[i].x = v[4 * i];
        cur->pol->Pw[i].y = v[4 * i + 1];
        cur->pol->Pw[i].z = v[4 * i + 2];
        cur->pol->Pw[i].w = v[4 * i + 3];
    }
    for (int64_t i = 0; i <= m; i++)
        cur->knt->U[i] = v[4 * (n + 1) + i];
    SmBSplineCurve* c = new (ctx) SmBSplineCurve(3, (ULONG)cur->pol->n, (ULONG)cur->knt->m, (short)cur->p,
                                                 cur->pol->Pw, cur->knt->U, &ctx);
    smos_Free(cur);
    return c;
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
};

// Cache flags `point_test have_t_surface_cache face_was_modified process_boundary_curves`.
void read_flags(Rd& rd, CachePeek* cache)
{
    cache->m_bPointTestEnabled = rd.flag() ? TRUE : FALSE;
    cache->m_bHaveTSurfaceCache = rd.flag() ? TRUE : FALSE;
    cache->m_bFaceWasModified = rd.flag() ? TRUE : FALSE;
    cache->SetProcessBoundaryCurves(rd.flag() ? TRUE : FALSE);
}

// Tree nodes `n` x `parent c1 c2 aux dom[4] class bbox[6] uconts[2] vconts[2] polar pseudo`,
// allocated in order from the tree's node manager (node 0 the top); patches (aux
// SM_AD_BEZIER_SURFACE) from m_sBezMgr, the others from m_sAuxMgr; every mBA_pSurface is the
// cache surface. `polar` is `0` (uninitialised: built on demand) or `1 k vecs[k]` (AddVector3d
// of each); `pseudo` (patches only) is `0` (SmPseudoBox()) or `1 basis[3] k pts[k]`
// (SmPseudoBox(), SetBasis, AddPoint3d of each); the continuities (patches only) are
// SmContinuityType codes.
std::vector<SmTreeNode*> read_tree(Rd& rd, CachePeek* cache, const SmSurface* s, const SmContext& ctx)
{
    int64_t n = rd.idx(1, 16);
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
        nd->m_pParent = ptr(rd.idx(-1, n - 1));
        nd->m_pChild1 = ptr(rd.idx(-1, n - 1));
        nd->m_pChild2 = ptr(rd.idx(-1, n - 1));
        nd->m_eAuxDataType = (SmAuxDataType)rd.idx(0, 7);
        nd->m_eGeomType = SM_NG_DEFAULT;
        bool patch = nd->m_eAuxDataType == SM_AD_BEZIER_SURFACE;
        SmBezierAux2d* a =
            patch ? (SmBezierAux2d*)cache->bez().GetNewElement() : (SmBezierAux2d*)cache->aux().GetNewElement();
        a->m_eSplitDir = SM_SP_UNKNOWN;
        a->m_bIsMarked = FALSE;
        a->m_pFace = NULL;
        a->m_sUVDomain = rd.dom4();
        a->m_eNodeClass = (SmNodeClassType)rd.idx(0, 3);
        a->mBA_pSurface = s;
        a->m_pOwningTreeNode = nd;
        double b[6];
        for (int i = 0; i < 6; i++)
            b[i] = rd.num();
        nd->m_sBBox = SmExtent3d(b[0], b[1], b[2], b[3], b[4], b[5]);
        SmContinuityType uc0 = (SmContinuityType)rd.idx(0, 13);
        SmContinuityType uc1 = (SmContinuityType)rd.idx(0, 13);
        SmContinuityType vc0 = (SmContinuityType)rd.idx(0, 13);
        SmContinuityType vc1 = (SmContinuityType)rd.idx(0, 13);
        if (rd.flag())
        {
            SmPolarBox pb;
            int64_t kv = rd.idx(0, 16);
            for (int64_t i = 0; i < kv; i++)
                pb.AddVector3d(rd.vec());
            *const_cast<SmPolarBox*>(a->GetPolarBoxPtr()) = pb;
        }
        if (patch)
        {
            SmBezierPatch* bp = (SmBezierPatch*)a;
            bp->m_pBezier = NULL;
            bp->m_eUCurveConts[0] = uc0;
            bp->m_eUCurveConts[1] = uc1;
            bp->m_eVCurveConts[0] = vc0;
            bp->m_eVCurveConts[1] = vc1;
            SmPseudoBox pbox;
            if (rd.flag())
            {
                SmVector3d b1 = rd.vec();
                SmVector3d b2 = rd.vec();
                SmVector3d b3 = rd.vec();
                pbox.SetBasis(b1, b2, b3);
                int64_t kp = rd.idx(0, 16);
                for (int64_t i = 0; i < kp; i++)
                    pbox.AddPoint3d(rd.vec());
            }
            bp->GetPseudoBox() = pbox;
        }
        nd->m_pData = a;
    }
    tree->top() = nodes[0];
    return nodes;
}

// A solution record `type nvars nobjs start_value start[3] end_value end[3] node0 node1 obj0
// obj1`; nodes are indices into `n0` / `n1` (-1: NULL or not found), objects `0` the curve, `1`
// the surface, `-1` NULL or other.
void put_sol(std::vector<double>& o, const SmSolution& s, const std::vector<SmTreeNode*>& n0,
             const std::vector<SmTreeNode*>& n1, const SmObject* curve, const SmObject* surface)
{
    o.push_back((double)s.m_eSolutionType);
    o.push_back((double)s.m_lNumVariables);
    o.push_back((double)s.m_lNumObjects);
    o.push_back(s.m_vStart.m_dSolutionValue);
    for (int i = 0; i < 3; i++)
        o.push_back(s.m_vStart[i]);
    o.push_back(s.m_vEnd.m_dSolutionValue);
    for (int i = 0; i < 3; i++)
        o.push_back(s.m_vEnd[i]);
    auto find = [](const std::vector<SmTreeNode*>& ns, const SmTreeNode* p) -> double {
        for (size_t i = 0; i < ns.size(); i++)
            if (ns[i] == p)
                return (double)i;
        return -1.0;
    };
    o.push_back(find(n0, s.m_apNodes[0]));
    o.push_back(find(n1, s.m_apNodes[1]));
    for (int i = 0; i < 2; i++)
    {
        const SmObject* p = s.m_apObjects[i];
        o.push_back(p != NULL && p == curve ? 0.0 : (p != NULL && p == surface ? 1.0 : -1.0));
    }
}

// LSI: `flags tree branch uvdom[4] point[3] vector[3] has_ivl ivl[2] ray dist_tol tol3d`.
void run_lsi(Rd& rd, const SmContext& ctx, SmSurface* srf, std::vector<double>& o)
{
    CachePeek* cache = new (ctx) CachePeek(*srf, srf->GetNaturalUVDomain());
    read_flags(rd, cache);
    std::vector<SmTreeNode*> nodes = read_tree(rd, cache, srf, ctx);
    SmTreeNode* node = nodes[(size_t)rd.idx(0, (int64_t)nodes.size() - 1)];
    SmExtent2d dom = rd.dom4();
    SmPoint3d lp = rd.vec();
    SmVector3d lv = rd.vec();
    bool has_ivl = rd.flag();
    double i0 = rd.num();
    double i1 = rd.num();
    SmExtent1d ivl(i0, i1);
    bool ray = rd.flag();
    double dist_tol = rd.num();
    double tol3d = rd.num();
    {
        SmLSIGlobalSolver gs(cache->tree(), dom, lp, lv, has_ivl ? &ivl : NULL, ray ? TRUE : FALSE, dist_tol);
        SmSolutionArray sols;
        gs.SetSolutions(&sols);
        gs.SetSolverOperation(SM_SO_INTERSECT);
        gs.Set3DTolerance(tol3d);
        gs.SetSolutionRequested(SM_SR_ALL);
        SmTreeNode* branch[SM_GS_MAX_TREES] = {NULL};
        branch[0] = node;
        SmBoolean more = FALSE;
        SmStatus r = gs.LocalSolve(branch, more);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back(more ? 1.0 : 0.0);
            o.push_back((double)sols.GetSize());
            std::vector<SmTreeNode*> none;
            for (ULONG i = 0; i < sols.GetSize(); i++)
                put_sol(o, sols[i], nodes, none, NULL, srf);
        }
    }
    delete cache;
}

// Combine: `curve uvdom[4] ivl[2] tol k_curve_nodes x (aux ivl[2]) surface_tree n_sols x (type
// nvars start_value start[3] end_value end[3] node0 node1)`. The surface tree's cache is over
// the surface's natural domain with default flags.
void run_combine(Rd& rd, const SmContext& ctx, SmSurface* srf, std::vector<double>& o)
{
    SmBSplineCurve* crv = read_curve(rd, ctx);
    SmExtent2d dom = rd.dom4();
    double i0 = rd.num();
    double i1 = rd.num();
    SmExtent1d ivl(i0, i1);
    double tol = rd.num();
    int64_t kc = rd.idx(0, 16);
    std::vector<SmTreeNode*> cnodes;
    std::vector<SmBezierAux1d*> caux;
    for (int64_t i = 0; i < kc; i++)
    {
        SmTreeNode* nd = new SmTreeNode();
        SmBezierAux1d* a = new SmBezierAux1d();
        nd->m_eAuxDataType = (SmAuxDataType)rd.idx(0, 7);
        double a0 = rd.num();
        double a1 = rd.num();
        a->m_sIvl = SmExtent1d(a0, a1);
        nd->m_pData = a;
        cnodes.push_back(nd);
        caux.push_back(a);
    }
    CachePeek* cache = new (ctx) CachePeek(*srf, srf->GetNaturalUVDomain());
    {
        std::vector<SmTreeNode*> snodes = read_tree(rd, cache, srf, ctx);
        int64_t ns = rd.idx(0, 64);
        SmSolutionArray sols;
        for (int64_t i = 0; i < ns; i++)
        {
            SmSolution s;
            s.m_eSolutionType = (SmSolutionType)rd.idx(0, 4);
            s.m_lNumVariables = (ULONG)rd.idx(0, 3);
            s.m_lNumObjects = 2;
            s.m_apObjects[0] = crv;
            s.m_apObjects[1] = srf;
            s.m_vStart.m_dSolutionValue = rd.num();
            for (int j = 0; j < 3; j++)
                s.m_vStart[j] = rd.num();
            s.m_vEnd.m_dSolutionValue = rd.num();
            for (int j = 0; j < 3; j++)
                s.m_vEnd[j] = rd.num();
            int64_t n0 = rd.idx(-1, (int64_t)cnodes.size() - 1);
            int64_t n1 = rd.idx(-1, (int64_t)snodes.size() - 1);
            s.m_apNodes[0] = n0 < 0 ? NULL : cnodes[(size_t)n0];
            s.m_apNodes[1] = n1 < 0 ? NULL : snodes[(size_t)n1];
            sols.Add(s);
        }
        SmStatus r = sc_srf_p10::sm_GlobalCurveIntersectCombineSolutions(*srf, dom, *crv, ivl, tol, sols);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back((double)sols.GetSize());
            for (ULONG i = 0; i < sols.GetSize(); i++)
                put_sol(o, sols[i], cnodes, snodes, crv, srf);
        }
    }
    delete cache;
    for (size_t i = 0; i < cnodes.size(); i++)
    {
        cnodes[i]->m_pData = NULL;
        delete cnodes[i];
        delete caux[i];
    }
    delete crv;
}
} // namespace

SC_EXPORT int32_t sc_surface_surface_p10_run(const double* in, int64_t n_in, double* out, int64_t cap,
                                             int64_t* out_len)
{
    if (in == NULL || n_in < 2)
        return SC_ERR_ARG;
    try
    {
        SmContext ctx;
        std::vector<double> o;
        Rd rd{in, n_in, 0};
        int64_t op = rd.idx(1, 2);
        SmSurface* srf = read_surface(rd, ctx);
        if (op == 1)
            run_lsi(rd, ctx, srf, o);
        else
            run_combine(rd, ctx, srf, o);
        delete srf;
        if (rd.at != rd.n)
            return SC_ERR_ARG;
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    catch (const Bad&)
    {
        return SC_ERR_ARG;
    }
    catch (...)
    {
        return SC_ERR_EXCEPTION;
    }
}
