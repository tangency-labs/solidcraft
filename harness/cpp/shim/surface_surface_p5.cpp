// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surface_p5`: the SmSurface members owned by the unit
// (source/SMLib/src/SmSurface.cpp, source/SMLib/inc/SmSurface.h), called through `SmSurface*`
// on SmBSplineSurface, SmPlane and SmCurveBoundedSurface objects (virtual dispatch as
// upstream), plus the file-static functions sm_FilterSolution and sm_SpLocalSolve and the
// IdenticalSolutions overrides of the four surface global solvers.
//
// Static functions and classes local to SmSurface.cpp cannot be linked against, so they are
// VERBATIM copies of upstream SmSurface.cpp (commit c9c5979), inside the namespace sc_srf_p5
// (line ranges noted at each block).
// sm_SpLocalSolve takes the surface sm_GetSurfaceToEvaluate would return instead of the tree
// node and Bezier patch (lines marked ADAPTED), as the port does. The four solver classes keep
// their data members and IdenticalSolutions; constructors set m_pSurf from the tree's owner and
// LocalSolve is an SM_ERR stub (it belongs to other units).
//
// One entry point, sc_surface_surface_p5_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/surface_surface_p5.rs, which validates every
// input. A surface block is as in unit surface.surface_p7. `out` receives the op status
// (0 = SM_SUCCESS, else the SmStatus value) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveBoundedSurface.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmGlobalSolver.h>
#include <SmIsoCurve.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmSurfTypes.h>
#include <SmTArray.h>
#include <SmTree.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

namespace sc_srf_p5
{
// clang-format off

#include "surface_sp_local_solve.hpp"

// ---- verbatim: SmSurface.cpp lines 12733-13043: sm_FilterSolution
static SmStatus sm_FilterSolution
  (const SmSurface      * cpBSS,           // in : target surface
   const SmExtent2d     & crUVDomain,      // in : surface intersection domain
   const SmCurve        & crCurve,         // in : target curve
   const SmExtent1d     & crInterval,      // in : curve intersection interval
   const SmContinuityType aeUConts[2],     // in : Surface U boundary continuity type
   const SmContinuityType aeVConts[2],     // in : Surface V boundary continuity type
   const SmContinuityType aeTConts[2],     // in : Curve ends continuity type
   const SmSolution     & crSolution,      // in : Current solution
   SmBoolean            & rbKeepSolution)  // out: TRUE = Keep this solution
{
  // init output
  rbKeepSolution = TRUE;

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE ;
  // draw
  if(bDebugMe)
    {
      SmFace *pFace  = (SmFace *)cpBSS->GetFace() ;
      SmEdge *pEdge  = (SmEdge *)crCurve.GetEdge() ;
      SmBrep *pBrep1 = pFace ? pFace->GetBrep() : NULL ;
      SmBrep *pBrep2 = pEdge ? pEdge->GetBrep() : NULL ;

      smgfx_Erase() ;
      smgfx_SetLook(1,2, 0,0,1) ; if(pBrep1) pBrep1->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,1,0) ; if(pBrep2) pBrep2->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,1,1) ; if(cpBSS) cpBSS->DrawUV() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 1,1,0) ; crCurve.Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(5,6, 1,0,0) ; crSolution.Draw() ; sm_GraphicsLoop() ;
      sm_GraphicsLoop() ;
    }
#endif // SM_DEBUG_CODE

  // keep answers good to Zero
  if (crSolution.m_vStart.m_dSolutionValue < SM_EFF_ZERO)
    return SM_SUCCESS;
  // keep answers good to scaledZero

  // get solution CurvePoint and a random NearMid SurfacePoint
  SmVector3d sCurvePV[2];  // sCurvePV[0] = crCurve 3dPoint
                           // sCurvePV[1] = crCurve 3dTangent
  SmPoint3d sRandomPoint;
  SER(crCurve.Evaluate(crSolution.m_vStart[0],1,TRUE,sCurvePV, TRUE));   // NonZeroTangent values
  SER(cpBSS->EvaluatePoint(crUVDomain.Evaluate(0.456,0.6789),sRandomPoint));

  // Scale Zero to current size.
  double dScaledZero =   10.0 * SM_EFF_ZERO
                       * (1.0 + smos_Max(sCurvePV[0].GetMaxDimension(),
                                         sRandomPoint.GetMaxDimension()));

  // keep answers good to scaledZero
  if (crSolution.m_vStart.m_dSolutionValue < dScaledZero)
    {
      // keep solutions that pass this more generous test
      return SM_SUCCESS;
    }

  // arrive here when we have an inexact answer
  // inexact solutions are only found at interval discontinuities


  // get solution SurfacePoint: sUV, sSurfacePoint, sDU, sDV, and sNorm
  SmPoint2d sUV(crSolution.m_vStart[1],crSolution.m_vStart[2]);
  SmVector3d sDU, sDV;
  SmPoint3d sSurfacePoint;
  SER(cpBSS->Evaluate1stDerivatives(sUV,TRUE,TRUE,sSurfacePoint,sDU,sDV));
  SmVector3d sNorm = sDU * sDV;
  if (sNorm.LengthSquared() < SM_EFF_ZERO_SQ)
    {
      SER(cpBSS->EvaluateNormal(sUV,TRUE,TRUE,sNorm));
    }
  SER(sNorm.Unitize());

  // get SurfacePoint to CurvePoint Gap Vector (the error)
  SmVector3d sDiff = sCurvePV[0] - sSurfacePoint;

  // see if solution lies on curve end-point boundaries
  SmBoolean bC0CurveMin = SM_ARE_SAME(crInterval.GetMin(),crSolution.m_vStart[0]) ;
  SmBoolean bC0CurveMax = SM_ARE_SAME(crInterval.GetMax(),crSolution.m_vStart[0]) ;

  // Keep Solutions whose gaps are
  //  within 5 degrees of parallel to SurfaceNormal vector and
  //  within 5 degrees of perpendicular to CurveTangent vector
  //   A tangent Curve/Surface closest point solution
  if (   sNorm.IsParallelTo(sDiff,5.0)
      && sCurvePV[1].IsPerpendicularTo(sDiff,5.0))
    {
      // This is a modification to this rule added for the particular
      // case where a curve-subdivision end-point solution is tangent
      // to the surface's SurfacePlane but the solution happens at a
      // local maximum rather than a local minimum of the curve/surface
      // gap function.  It may be that the modification needs to be generalized
      // for cases on the boundaries of surfaces.  But for now - just
      // worry about boundaries of curves.

      // The idea is that tangent conditions represent an extremum to the
      // gap function between the curve and surface and we only want to
      // keep gap minima and exclude gap maxima. A gap maximum with a small
      // tolerance is likely to have a better intersection point nearby.
      //
      // Judge this by estimating where the curve might intersect the
      // surface based on the shape of the curve and surface at the
      // current solution.  If the distances to those intersections are
      // within the size of the current curve and surface then
      // assume this is a local maximum and don't keep the solution.

      // when the curve is continuous through this solution
      if(   (bC0CurveMin && aeTConts[0] >= SM_CT_G1_G2)
         || (bC0CurveMax && aeTConts[1] >= SM_CT_G1_G2))
        {
          // get the surface's normal section evaluation
          SmVector3d sTangentPlaneDirection, sSurfaceNormal ;
          double     dSurfaceCurvature ;
          cpBSS->EvaluateNormalSection(sUV, TRUE, TRUE, sCurvePV[1],
                                       sTangentPlaneDirection,
                                       dSurfaceCurvature,
                                       sSurfaceNormal) ;

          // compute the curve's position, tangent, curvature, and normal values
          SmVector3d aCurveFrenet[3] ;  // aCurveFrenet[0] = position
                                        // aCurveFrenet[1] = Mag = Speed, direction = tangent vector
                                        // aCurveFrenet[2] = Mag = K,     direction = normal vector
          crCurve.EvaluateGeometric(crSolution.m_vStart[0], 2, TRUE, aCurveFrenet) ;

          // look for lines
          double dCurveCurvature   = aCurveFrenet[2].Length() ;
          SmBoolean bIsCurveLine   = smos_Fabs(dCurveCurvature)   < SM_EFF_ZERO_SQRT ;
          SmBoolean bIsSurfaceLine = smos_Fabs(dSurfaceCurvature) < SM_EFF_ZERO_SQRT ;

          // handle line cases now
          if(bIsCurveLine && bIsSurfaceLine)
            {
              rbKeepSolution = TRUE ;
              return SM_SUCCESS ;
            }
          else if(bIsCurveLine)
            {
              // keep solutions when surf is bending away from curve
              // with SmVector3d sDiff = sCurvePV[0] - sSurfacePoint;
              rbKeepSolution =   (dSurfaceCurvature * sDiff.Dot(sSurfaceNormal)) > 0.0
                               ? FALSE
                               : TRUE ;
              return SM_SUCCESS ;
            }
          else if(bIsSurfaceLine)
            {
              // keep solutions when curve is bending away from surface
              // with SmVector3d sDiff = sCurvePV[0] - sSurfacePoint;
              rbKeepSolution =   sDiff.Dot(aCurveFrenet[2]) > 0.0
                               ? TRUE
                               : FALSE ;
              return SM_SUCCESS ;
            }

          // arrive here when both SurfaceNormalSection and Curve are curving at the solution

          // note: The surface curve lies in the SurfaceNormal/SurfaceTangent plane
          //       The curve lies in the plane defined by its tangent and normal vectors.
          //       The gap vector is within tolerance of being parallel to the surface normal vector.
          //       The surfaceSectionPlane and the CurvePlane won't generally be parallel to one another.
          //         Because the SectionNormalPlane was defined by the CurveTangent the two
          //         planes will always line up like two pages in a book - they share a common
          //         axis which will be parallel to the curve and the SurfaceSectionCurve's tangents.
          //
          //       To make this test, project the curve to the SurfaceSectionPlane which changes
          //       its curvature value at the solution point.  Use this projected curvature
          //       value to decide if the gap function at the solution is getting bigger or smaller
          //       for small deviations from the solution point.


          // Compute the curvature value for the curve after it's been projected
          // to the surfaceSectionPlane.  This will be a signed curvature value,
          // coincidentaly just like the surfaceSectionCurve's signed curvature value,
          //   Positive = Curve center of curvature is offset from curve in the sSurfaceNormal direction
          //   Negative = Curve center of curvature is offset from curve opposite to the sSurfaceNormal direction
          // The curve is bending towards or away from the surface depending on this sign and the
          //  direction of the gap vector compared to the surface normal vector - lot's of stuff to track.
          dCurveCurvature = aCurveFrenet[2].Dot(sSurfaceNormal) ;

          // keep the solution when the two curves are bending away from one another
          // or when the curves are both bending in the same direction keep the solution
          //  when the 'inside' curve is bending faster than the 'outside' curve so that
          //  the gap function is locally increasing not decreasing.

          // Let g = gap vector
          //     n = surface normal
          //     Kc = signed curve curvature normal after projection onto SurfaceSectionPlane
          //     Ks = signed surfaceSection curvature normal
          // cases
          //   g.n | sign(Ks) | sign(Kc) | description                                        | keep test         |
          // ------+----------+----------+----------------------------------------------------+-------------------+
          //   neg |   pos    |   pos    | both curves bending the same way                   | abs(Kc) < abs(Ks) |
          //   pos |   neg    |   neg    | surfaceSectionCurve is inside and Curve is outside |                   |
          // ------+----------+----------+----------------------------------------------------+-------------------+
          //   neg |   pos    |   neg    | both curves are bending away from one another      |  TRUE             |
          //   pos |   neg    |   pos    |                                                    |                   |
          // ------+----------+----------+----------------------------------------------------+-------------------+
          //   neg |   neg    |   pos    | both curves are bending towards one another        |  FALSE            |
          //   pos |   pos    |   neg    |                                                    |                   |
          // ------+----------+----------+----------------------------------------------------+-------------------+
          //   neg |   neg    |   neg    | both curves bending the same way                   | abs(Kc) > abs(Ks) |
          //   pos |   pos    |   pos    | Curve is inside and surfaceSectionCurve is outside |                   |
          // ------+----------+----------+----------------------------------------------------+-------------------+
          // This assumes that the gap distance is small compared to the radius of curvature values.
          // Otherwise a small correction is needed in the inequality tests.

          SmBoolean bPosGDotN       = sDiff.Dot(sSurfaceNormal) > 0.0 ;
          SmBoolean bPosKs          = dSurfaceCurvature > 0.0 ;
          SmBoolean bPosKc          = dCurveCurvature > 0.0 ;
          SmBoolean bKcBiggerThanKs = smos_Fabs(dCurveCurvature) > smos_Fabs(dSurfaceCurvature) ;

          if     (   (!bPosGDotN &&  bPosKs &&  bPosKc)
                  || ( bPosGDotN && !bPosKs && !bPosKc)) { rbKeepSolution = !bKcBiggerThanKs ; }
          else if(   (!bPosGDotN &&  bPosKs && !bPosKc)
                  || ( bPosGDotN && !bPosKs &&  bPosKc)) { rbKeepSolution =  TRUE ; }
          else if(   (!bPosGDotN && !bPosKs &&  bPosKc)
                  || ( bPosGDotN &&  bPosKs && !bPosKc)) { rbKeepSolution =  FALSE ; }
          else if(   (!bPosGDotN && !bPosKs && !bPosKc)
                  || ( bPosGDotN &&  bPosKs &&  bPosKc)) { rbKeepSolution =  bKcBiggerThanKs ; }

          return SM_SUCCESS ;
        } // end end-curve point solution review

      return SM_SUCCESS;
    }

  // If here it is not a tangent Curve/Surface closest point solution case.
  // The curve has come within tolerance distance of a boundary.  There
  // are 6 boundaries to watch, the Curve min/max boundaries and the
  // Surface U/V min/max boundaries.
  //   Keep solutions that are bounded by discontinuity boundaries.
  //   Toss solutions bounded by continuous boundaries because the actual
  //     solution will be found on another SolveBranch using the neighboring
  //     bit of domain.

  // see if solution lies on surface curve-boundaries
  SmBoolean bC0SurfUMin = SM_ARE_SAME( crUVDomain.GetMin().x, crSolution.m_vStart[1] );
  SmBoolean bC0SurfUMax = SM_ARE_SAME( crUVDomain.GetMax().x, crSolution.m_vStart[1] );
  SmBoolean bC0SurfVMin = SM_ARE_SAME( crUVDomain.GetMin().y, crSolution.m_vStart[2] );
  SmBoolean bC0SurfVMax = SM_ARE_SAME( crUVDomain.GetMax().y, crSolution.m_vStart[2] );

  // see which way the gap vector points relative to the curve and surface tangent vectors
  double dCurveDotDiff = sCurvePV[1].Dot(sDiff) ;
  double dSurfUDotDiff = sDiff.Dot(sDU) ;
  double dSurfVDotDiff = sDiff.Dot(sDV) ;

  // when the solution is on a discontinuity boundary keep it if the gap vector points out of the shape domain
  // Solutions on continuous boundaries are tossed because they will be picked up by another SolveBranch
  // In addition, for a hit within tol of a seam, protocol is to return only
  // one solution: the one that's slightly interior and presumably precise.
  // The imprecise one right on the boundary is rejected.
  // For our purposes here, a seam becomes a continuous boundary. [B161]
  // However, checking seams is somewhat expensive,
  // so let's not do it if we don't have to.  Check it after this test.

  SmBoolean bCurveSave =    (aeTConts[0] <= SM_CT_C0 && bC0CurveMin && dCurveDotDiff > 0.0)
                         || (aeTConts[1] <= SM_CT_C0 && bC0CurveMax && dCurveDotDiff < 0.0) ;
  SmBoolean bSurfUSave =    (aeUConts[0] <= SM_CT_C0 && bC0SurfUMin && dSurfUDotDiff < 0.0)
                         || (aeUConts[1] <= SM_CT_C0 && bC0SurfUMax && dSurfUDotDiff > 0.0) ;
  SmBoolean bSurfVSave =    (aeVConts[0] <= SM_CT_C0 && bC0SurfVMin && dSurfVDotDiff < 0.0)
                         || (aeVConts[1] <= SM_CT_C0 && bC0SurfVMax && dSurfVDotDiff > 0.0) ;

  // pass any boundary solution that works on a boundary
  // solutions on corners work when any of the following conditions are met
  //   Save solutions at discontinuous Curve C0 boundaries when SurfToCurve gap points out of the curve interval
  //   Save solutions at discontinuous Surface U boundaries when CurveToSurf gap points out of the surface domain
  //   Save solutions at discontinuous Surface V boundaries when CurveToSurf gap points out of the surface domain
  rbKeepSolution =   (bCurveSave || bSurfUSave || bSurfVSave)
                    ? TRUE
                    : FALSE ;

  // Ok, now check seams, if we have to.
  if ( rbKeepSolution )
    {
      if ( bCurveSave )
        {
          // Can't use passed-in interval, it's just a cache subset.
          SmExtent1d sCrvIvl = crCurve.GetNaturalInterval();
          if ( crCurve.IsClosed( sCrvIvl ) )
            { bCurveSave = FALSE; }

          rbKeepSolution = (bCurveSave || bSurfUSave || bSurfVSave) ? TRUE : FALSE;
        }
    }
  // Can't use passed-in interval, it's just a cache subset.
  SmExtent2d sSrfDomain = cpBSS->GetNaturalUVDomain();
  if ( rbKeepSolution )
    {
      if ( bSurfUSave )
        {
          if ( cpBSS->IsClosed( sSrfDomain, SM_SP_U ) )
            { bSurfUSave = FALSE; }

          rbKeepSolution = (bCurveSave || bSurfUSave || bSurfVSave) ? TRUE : FALSE;
        }
    }
  if ( rbKeepSolution )
    {
      if ( bSurfVSave )
        {
          if ( cpBSS->IsClosed( sSrfDomain, SM_SP_V ) )
            { bSurfVSave = FALSE; }

          rbKeepSolution = (bCurveSave || bSurfUSave || bSurfVSave) ? TRUE : FALSE;
        }
    }

  return SM_SUCCESS;

} // end sm_FilterSolution




// ---- the solver classes (data members of SmSurface.cpp; constructors and LocalSolve adapted)
class SmSPGlobalSolver : public SmGlobalSolver
{
public:
  const SmSurface  * m_pSurf;
  const SmExtent2d & m_crUVDomain;
  SmSPGlobalSolver(SmTree *pTree, const SmExtent2d & crUVDomain)
   : SmGlobalSolver(pTree, 2), m_pSurf(SM_CAST_PTR(SmSurface, pTree->GetOwnerObject())), m_crUVDomain(crUVDomain) {}
  void SetTol(double dTol) { m_d3dTolerance = dTol; }
  void SetSurf(const SmSurface *pSurf) { m_pSurf = pSurf; }
  virtual SmStatus LocalSolve(SmTreeNode *[SM_GS_MAX_TREES], SmBoolean &) { return SM_ERR; }
  virtual SmSolutionPairType IdenticalSolutions(const SmSolution & crSol1, const SmSolution & crSol2,
                                                const SmSolution *&pPreferredSol) const;
};
class SmLSIGlobalSolver : public SmGlobalSolver
{
public:
  const SmSurface  * m_pSurf;
  const SmExtent2d & m_crUVDomain;
  SmLSIGlobalSolver(SmTree *pTree, const SmExtent2d & crUVDomain)
   : SmGlobalSolver(pTree, 2), m_pSurf(SM_CAST_PTR(SmSurface, pTree->GetOwnerObject())), m_crUVDomain(crUVDomain) {}
  void SetTol(double dTol) { m_d3dTolerance = dTol; }
  void SetSurf(const SmSurface *pSurf) { m_pSurf = pSurf; }
  virtual SmStatus LocalSolve(SmTreeNode *[SM_GS_MAX_TREES], SmBoolean &) { return SM_ERR; }
  virtual SmSolutionPairType IdenticalSolutions(const SmSolution & crSol1, const SmSolution & crSol2,
                                                const SmSolution *&pPreferredSol) const;
};
class SmCSGlobalSolver : public SmGlobalSolver
{
public:
  const SmSurface  * m_pSurf;
  const SmExtent1d & m_crInterval;
  const SmExtent2d & m_crUVDomain;
  SmCSGlobalSolver(SmTree *pTree, const SmExtent1d & crInterval, const SmExtent2d & crUVDomain)
   : SmGlobalSolver(pTree, 3), m_pSurf(SM_CAST_PTR(SmSurface, pTree->GetOwnerObject())), m_crInterval(crInterval), m_crUVDomain(crUVDomain) {}
  void SetTol(double dTol) { m_d3dTolerance = dTol; }
  void SetSurf(const SmSurface *pSurf) { m_pSurf = pSurf; }
  virtual SmStatus LocalSolve(SmTreeNode *[SM_GS_MAX_TREES], SmBoolean &) { return SM_ERR; }
  virtual SmSolutionPairType IdenticalSolutions(const SmSolution & crSol1, const SmSolution & crSol2,
                                                const SmSolution *&pPreferredSol) const;
};
class SmCSIGlobalSolver : public SmGlobalSolver
{
public:
  const SmSurface  * m_pSurf;
  SmBoolean          m_bFoundCoincidence;
  const SmExtent1d & m_crInterval;
  const SmExtent2d & m_crUVDomain;
  SmCSIGlobalSolver(SmTree *pTree, const SmExtent1d & crInterval, const SmExtent2d & crUVDomain)
   : SmGlobalSolver(pTree, 3), m_pSurf(SM_CAST_PTR(SmSurface, pTree->GetOwnerObject())), m_bFoundCoincidence(FALSE), m_crInterval(crInterval), m_crUVDomain(crUVDomain) {}
  void SetTol(double dTol) { m_d3dTolerance = dTol; }
  void SetSurf(const SmSurface *pSurf) { m_pSurf = pSurf; }
  virtual SmStatus LocalSolve(SmTreeNode *[SM_GS_MAX_TREES], SmBoolean &) { return SM_ERR; }
  virtual SmSolutionPairType IdenticalSolutions(const SmSolution & crSol1, const SmSolution & crSol2,
                                                const SmSolution *&pPreferredSol) const;
};


// ---- verbatim: SmSurface.cpp lines 19879-19962: SmSPGlobalSolver::IdenticalSolutions
SmSolutionPairType SmSPGlobalSolver::IdenticalSolutions  // rtn: oneof SM_SP_IDENTICAL
                                                         //            SM_SP_DIFFCOUNTS
                                                         //            SM_SP_DIFFOBJECTS
                                                         //            SM_SP_DIFFPARAMETERS
  (const SmSolution & crSol1,                            // in : 1st target solution
   const SmSolution & crSol2,                            // in : 2nd target solution
   const SmSolution *&pPreferredSol)                     // out: When identical, set to the preferred solution or NULL for none.
                                                         //      NULL to ignore, default:[NULL]
  const
{
  // init output
  pPreferredSol = NULL ;

  // If the parent thinks they're identical, then they are.
  SmSolutionPairType eSPType = SmGlobalSolver::IdenticalSolutions( crSol1, crSol2, pPreferredSol ) ;
  if(SM_SP_IDENTICAL == eSPType)
      { return SM_SP_IDENTICAL; }

  if(SM_SP_DIFFPARAMETERS == eSPType)
    {
#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE ;
          if(bDebugMe)
            {
              crSol1.Dump() ;
              crSol2.Dump() ;
            }
#endif // SM_DEBUG_CODE

      // At a singularity, the same 3d point can have different uv values.
      SmPoint2d sUV1( crSol1.m_vStart[0], crSol1.m_vStart[1] );
      SmPoint2d sUV2( crSol2.m_vStart[0], crSol2.m_vStart[1] );

      // Check positions.
      SmPoint3d sPt1, sPt2;
      m_pSurf->EvaluatePoint( sUV1, sPt1 );
      m_pSurf->EvaluatePoint( sUV2, sPt2 );

      double dTol = m_d3dTolerance; // untested: see how it works in practice

      if ( ! sPt1.CloserThan( dTol, sPt2 ) )
          { return SM_SP_DIFFPARAMETERS; }

      // 3d points are the same.  Need to check seams and poles.
      // Check poles first, it's a cheaper check.
      // If either is nonsingular, they're distinct.

      SmSurfParamType eSingDir;
      if ( ! m_pSurf->IsSingularity( sUV1, eSingDir ) )
        { return SM_SP_DIFFPARAMETERS; }
      if ( ! m_pSurf->IsSingularity( sUV2, eSingDir ) )
        { return SM_SP_DIFFPARAMETERS; }

      // We're at a pole.  Distinct only if they're both on a seam.
      // When one is on a seam prefer that. Else prefer the best solution.
      // (We already know that their uv's are different, so if they're
      // both on seams, they must be on opposite sides of it.)
      SmBoolean bOnSeam1 = m_pSurf->IsOnSeam( sUV1 ) ;
      SmBoolean bOnSeam2 = m_pSurf->IsOnSeam( sUV2 ) ;
      if( bOnSeam1 && !bOnSeam2) { pPreferredSol = &crSol1 ;
                                   return SM_SP_IDENTICAL ;
                                 }
      if(!bOnSeam1 &&  bOnSeam2) { pPreferredSol = &crSol2 ;
                                   return SM_SP_IDENTICAL ;
                                 }
      if(!bOnSeam1 && !bOnSeam2) { pPreferredSol =  (crSol1.m_vStart.m_dSolutionValue < crSol2.m_vStart.m_dSolutionValue)
                                                   ? &crSol1
                                                   : &crSol2 ;
                                   return SM_SP_IDENTICAL ;
                                 }

      //      // We're at a pole.  Distinct only if they're also on a seam.
      //      // (We already know that their uv's are different.)
      //      if ( ! m_pSurf->IsOnSeam( sUV1 ) )
      //        { return SM_SP_IDENTICAL; }
      //      if ( ! m_pSurf->IsOnSeam( sUV2 ) )
      //        { return SM_SP_IDENTICAL; }
    }

  // Identical positions, at a pole on a seam:
  // Call them distinct points.
  return eSPType;

} // end SmSPGlobalSolver::IdenticalSolutions

// ---- verbatim: SmSurface.cpp lines 18843-18930: SmLSIGlobalSolver::IdenticalSolutions
SmSolutionPairType SmLSIGlobalSolver::IdenticalSolutions // rtn: oneof SM_SP_IDENTICAL
                                                         //            SM_SP_DIFFCOUNTS
                                                         //            SM_SP_DIFFOBJECTS
                                                         //            SM_SP_DIFFPARAMETERS
  (const SmSolution & crSol1,                            // in : 1st target solution
   const SmSolution & crSol2,                            // in : 2nd target solution
   const SmSolution *&pPreferredSol)                     // out: When identical, set to the preferred solution or NULL for none.
                                                         //      NULL to ignore, default:[NULL]
  const
{
  // init output
  pPreferredSol = NULL ;

  // If the parent thinks they're identical, then they are.
  SmSolutionPairType eSPType = SmGlobalSolver::IdenticalSolutions( crSol1, crSol2, pPreferredSol ) ;
  if(SM_SP_IDENTICAL == eSPType)
      { return SM_SP_IDENTICAL; }

  if(SM_SP_DIFFPARAMETERS == eSPType)
    {
      if(   crSol1.m_eSolutionType == SM_ST_SINGLE_VALUE
         && crSol2.m_eSolutionType == SM_ST_SINGLE_VALUE)
        {
#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE ;
          if(bDebugMe)
            {
              crSol1.Dump() ;
              crSol2.Dump() ;
            }
#endif // SM_DEBUG_CODE

          // At a singularity, the same 3d point can have different uv values.
          SmPoint2d sUV1( crSol1.m_vStart[1], crSol1.m_vStart[2] );
          SmPoint2d sUV2( crSol2.m_vStart[1], crSol2.m_vStart[2] );

          // Check positions.
          SmPoint3d sPt1, sPt2;
          m_pSurf->EvaluatePoint( sUV1, sPt1 );
          m_pSurf->EvaluatePoint( sUV2, sPt2 );

          double dTol = m_d3dTolerance; // untested: see how it works in practice

          if ( ! sPt1.CloserThan( dTol, sPt2 ) )
              { return SM_SP_DIFFPARAMETERS; }

          // 3d points are the same.  Need to check seams and poles.
          // Check poles first, it's a cheaper check.
          // If either is nonsingular, they're distinct.

          SmSurfParamType eSingDir;
          if ( ! m_pSurf->IsSingularity( sUV1, eSingDir ) )
            { return SM_SP_DIFFPARAMETERS; }
          if ( ! m_pSurf->IsSingularity( sUV2, eSingDir ) )
            { return SM_SP_DIFFPARAMETERS; }

          // We're at a pole.  Distinct only if they're both on a seam.
          // When one is on a seam prefer that. Else prefer the best solution.
          // (We already know that their uv's are different, so if they're
          // both on seams, they must be on opposite sides of it.)
          SmBoolean bOnSeam1 = m_pSurf->IsOnSeam( sUV1 ) ;
          SmBoolean bOnSeam2 = m_pSurf->IsOnSeam( sUV2 ) ;
          if( bOnSeam1 && !bOnSeam2) { pPreferredSol = &crSol1 ;
                                       return SM_SP_IDENTICAL ;
                                     }
          if(!bOnSeam1 &&  bOnSeam2) { pPreferredSol = &crSol2 ;
                                       return SM_SP_IDENTICAL ;
                                     }
          if(!bOnSeam1 && !bOnSeam2) { pPreferredSol =  (crSol1.m_vStart.m_dSolutionValue < crSol2.m_vStart.m_dSolutionValue)
                                                       ? &crSol1
                                                       : &crSol2 ;
                                       return SM_SP_IDENTICAL ;
                                     }

          //      // We're at a pole.  Distinct only if they're also on a seam.
          //      // (We already know that their uv's are different.)
          //      if ( ! m_pSurf->IsOnSeam( sUV1 ) )   // need trick here to tell caller to save the one on the seam
          //        { return SM_SP_IDENTICAL; }
          //      if ( ! m_pSurf->IsOnSeam( sUV2 ) )
          //        { return SM_SP_IDENTICAL; }
        }
    }

  // Identical positions, at a pole on a seam:
  // Call them distinct points.
  return eSPType;

} // end SmLSIGlobalSolver::IdenticalSolutions

// ---- verbatim: SmSurface.cpp lines 17670-17757: SmCSGlobalSolver::IdenticalSolutions
SmSolutionPairType SmCSGlobalSolver::IdenticalSolutions // rtn: oneof SM_SP_IDENTICAL
                                                        //            SM_SP_DIFFCOUNTS
                                                        //            SM_SP_DIFFOBJECTS
                                                        //            SM_SP_DIFFPARAMETERS
  (const SmSolution & crSol1,                           // in : 1st target solution
   const SmSolution & crSol2,                           // in : 2nd target solution
   const SmSolution *&pPreferredSol)                    // out: When identical, set to the preferred solution or NULL for none.
                                                        //      NULL to ignore, default:[NULL]
  const
{
  // init output
  pPreferredSol = NULL ;

  // If the parent thinks they're identical, then they are.
  SmSolutionPairType eSPType = SmGlobalSolver::IdenticalSolutions( crSol1, crSol2, pPreferredSol ) ;
  if(SM_SP_IDENTICAL == eSPType)
      { return SM_SP_IDENTICAL; }

  if(SM_SP_DIFFPARAMETERS == eSPType)
    {
      if(   crSol1.m_eSolutionType == SM_ST_SINGLE_VALUE
         && crSol2.m_eSolutionType == SM_ST_SINGLE_VALUE)
        {
#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE ;
          if(bDebugMe)
            {
              crSol1.Dump() ;
              crSol2.Dump() ;
            }
#endif // SM_DEBUG_CODE

          // At a singularity, the same 3d point can have different uv values.
          SmPoint2d sUV1( crSol1.m_vStart[1], crSol1.m_vStart[2] );
          SmPoint2d sUV2( crSol2.m_vStart[1], crSol2.m_vStart[2] );

          // Check positions.
          SmPoint3d sPt1, sPt2;
          m_pSurf->EvaluatePoint( sUV1, sPt1 );
          m_pSurf->EvaluatePoint( sUV2, sPt2 );

          double dTol = m_d3dTolerance; // untested: see how it works in practice

          if ( ! sPt1.CloserThan( dTol, sPt2 ) )
              { return SM_SP_DIFFPARAMETERS; }

          // 3d points are the same.  Need to check seams and poles.
          // Check poles first, it's a cheaper check.
          // If either is nonsingular, they're distinct.

          SmSurfParamType eSingDir;
          if ( ! m_pSurf->IsSingularity( sUV1, eSingDir ) )
            { return SM_SP_DIFFPARAMETERS; }
          if ( ! m_pSurf->IsSingularity( sUV2, eSingDir ) )
            { return SM_SP_DIFFPARAMETERS; }

          // We're at a pole.  Distinct only if they're both on a seam.
          // When one is on a seam prefer that. Else prefer the best solution.
          // (We already know that their uv's are different, so if they're
          // both on seams, they must be on opposite sides of it.)
          SmBoolean bOnSeam1 = m_pSurf->IsOnSeam( sUV1 ) ;
          SmBoolean bOnSeam2 = m_pSurf->IsOnSeam( sUV2 ) ;
          if( bOnSeam1 && !bOnSeam2) { pPreferredSol = &crSol1 ;
                                       return SM_SP_IDENTICAL ;
                                     }
          if(!bOnSeam1 &&  bOnSeam2) { pPreferredSol = &crSol2 ;
                                       return SM_SP_IDENTICAL ;
                                     }
          if(!bOnSeam1 && !bOnSeam2) { pPreferredSol =  (crSol1.m_vStart.m_dSolutionValue < crSol2.m_vStart.m_dSolutionValue)
                                                       ? &crSol1
                                                       : &crSol2 ;
                                       return SM_SP_IDENTICAL ;
                                     }

          //      // We're at a pole.  Distinct only if they're also on a seam.
          //      // (We already know that their uv's are different.)
          //      if ( ! m_pSurf->IsOnSeam( sUV1 ) )
          //        { return SM_SP_IDENTICAL; }
          //      if ( ! m_pSurf->IsOnSeam( sUV2 ) )
          //        { return SM_SP_IDENTICAL; }
        }
    }

  // Identical positions, at a pole on a seam:
  // Call them distinct points.
  return eSPType;

} // end SmCSGlobalSolver::IdenticalSolutions

// ---- verbatim: SmSurface.cpp lines 14784-14869: SmCSIGlobalSolver::IdenticalSolutions
SmSolutionPairType SmCSIGlobalSolver::IdenticalSolutions // rtn: oneof SM_SP_IDENTICAL
                                                         //            SM_SP_DIFFCOUNTS
                                                         //            SM_SP_DIFFOBJECTS
                                                         //            SM_SP_DIFFPARAMETERS
  (const SmSolution & crSol1,                            // in : 1st target solution
   const SmSolution & crSol2,                            // in : 2nd target solution
   const SmSolution *&pPreferredSol)                     // out: When identical, set to the preferred solution or NULL for none.
                                                         //      NULL to ignore, default:[NULL]
  const
{
  // init output
  pPreferredSol = NULL ;

  // If the parent thinks they're identical, then they are.
  SmSolutionPairType eSPType = SmGlobalSolver::IdenticalSolutions( crSol1, crSol2, pPreferredSol ) ;
  if(SM_SP_IDENTICAL == eSPType)
    { return SM_SP_IDENTICAL; }

  if(SM_SP_DIFFPARAMETERS == eSPType)
    {
      if(   crSol1.m_eSolutionType == SM_ST_SINGLE_VALUE
         && crSol2.m_eSolutionType == SM_ST_SINGLE_VALUE)
        {
#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE ;
          if(bDebugMe)
            {
              crSol1.Dump() ;
              crSol2.Dump() ;
            }
#endif // SM_DEBUG_CODE

          // At a singularity, the same 3d point can have different uv values.
          SmPoint2d sUV1( crSol1.m_vStart[1], crSol1.m_vStart[2] );
          SmPoint2d sUV2( crSol2.m_vStart[1], crSol2.m_vStart[2] );

          // Check positions.
          SmPoint3d sPt1, sPt2;
          m_pSurf->EvaluatePoint( sUV1, sPt1 );
          m_pSurf->EvaluatePoint( sUV2, sPt2 );

          double dTol = m_d3dTolerance; // untested: see how it works in practice

          if ( ! sPt1.CloserThan( dTol, sPt2 ) )
              { return SM_SP_DIFFPARAMETERS; }

          // 3d points are the same.  Need to check seams and poles.
          // Check poles first, it's a cheaper check.
          // If either is nonsingular, they're distinct.

          SmSurfParamType eSingDir;
          if ( ! m_pSurf->IsSingularity( sUV1, eSingDir ) )
            { return SM_SP_DIFFPARAMETERS; }
          if ( ! m_pSurf->IsSingularity( sUV2, eSingDir ) )
            { return SM_SP_DIFFPARAMETERS; }

          // We're at a pole.  Distinct only if they're both on a seam.
          // When one is on a seam prefer that. Else prefer the best solution.
          // (We already know that their uv's are different, so if they're
          // both on seams, they must be on opposite sides of it.)
          SmBoolean bOnSeam1 = m_pSurf->IsOnSeam( sUV1 ) ;
          SmBoolean bOnSeam2 = m_pSurf->IsOnSeam( sUV2 ) ;
          if( bOnSeam1 && !bOnSeam2) { pPreferredSol = &crSol1 ;
                                       return SM_SP_IDENTICAL ;
                                     }
          if(!bOnSeam1 &&  bOnSeam2) { pPreferredSol = &crSol2 ;
                                       return SM_SP_IDENTICAL ;
                                     }
          if(!bOnSeam1 && !bOnSeam2) { pPreferredSol =  (crSol1.m_vStart.m_dSolutionValue < crSol2.m_vStart.m_dSolutionValue)
                                                       ? &crSol1
                                                       : &crSol2 ;
                                       return SM_SP_IDENTICAL ;
                                     }

          //      if ( !bOnSeam1 )
          //        { return SM_SP_IDENTICAL; }
          //      if ( !bOnSeam2 )
          //        { return SM_SP_IDENTICAL; }
        }
    }

  // Identical positions, at a pole on a seam:
  // Call them distinct points.
  return eSPType;

} // end SmCSIGlobalSolver::IdenticalSolutions

// clang-format on
} // namespace sc_srf_p5

namespace
{

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

void put_vec(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

void put_uv(std::vector<double>& o, const SmVector2d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
}

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


// ---- ops (keep in sync with crates/solidcore-ffi/src/units/surface_surface_p5.rs) ----------
enum : int32_t
{
    OP_IS_SING = 0,
    OP_NORMAL_SING = 1,
    OP_ISO = 2,
    OP_LPS = 3,
    OP_LPS0 = 4,
    OP_DROP = 5,
    OP_LSSI = 6,
    OP_COVER = 7,
    OP_FILTER = 8,
    OP_SP_LOCAL = 9,
    OP_IDENT = 10,
    OP_MAXDIST = 11,
};

void put_solution(std::vector<double>& o, const SmSolution& s, int n)
{
    o.push_back(s.m_vStart.m_dSolutionValue);
    for (int i = 0; i < n; i++)
        o.push_back(s.m_vStart[i]);
}

void read_solution(Rd& rd, SmSolution& s)
{
    s.m_eSolutionType = (SmSolutionType)rd.idx();
    s.m_lNumVariables = (ULONG)rd.idx();
    s.m_vStart.m_dSolutionValue = rd.num();
    for (int i = 0; i < 3; i++)
        s.m_vStart[i] = rd.num();
}

void run(int32_t op, Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    SmSurface* s = read_surface(rd, ctx);
    switch (op)
    {
    case OP_IS_SING:
    {
        SmPoint2d uv = rd.uv();
        double tol = rd.num();
        SmBoolean pt_only = rd.bflag();
        SmSurfParamType dir = SM_SP_UNKNOWN;
        SmBoolean r = s->IsSingularity(uv, dir, tol, pt_only);
        o.push_back(0.0);
        o.push_back(flg(r));
        o.push_back((double)dir);
        break;
    }
    case OP_NORMAL_SING:
    {
        SmPoint2d uv = rd.uv();
        double tol = rd.num();
        SmVector3d n(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        SmStatus r = s->EvaluateNormalAtSingularity(uv, n, tol);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
            put_vec(o, n);
        break;
    }
    case OP_ISO:
    {
        SmSurfParamType param = (SmSurfParamType)rd.idx();
        double iso = rd.num();
        double tol = rd.num();
        bool has_dom = rd.flag();
        SmExtent2d dom = rd.dom();
        bool want_uv = rd.flag();
        SmBSplineCurve* c = NULL;
        double gap = 0.0;
        SmCurve* uvc = NULL;
        SmStatus r = s->CreateIsoParametricCurve(ctx, param, iso, SmApproxTol3d(tol), c, has_dom ? &dom : NULL, &gap,
                                                 want_uv ? &uvc : NULL);
        o.push_back(st(r));
        if (r == SM_SUCCESS && c != NULL)
        {
            SmExtent1d ivl = c->GetNaturalInterval();
            o.push_back(gap);
            o.push_back(uvc != NULL ? 1.0 : 0.0);
            o.push_back(ivl.GetMin());
            o.push_back(ivl.GetMax());
            for (int i = 0; i <= 8; i++)
            {
                SmPoint3d p;
                c->EvaluatePoint(ivl.Evaluate(i / 8.0), p);
                put_vec(o, p);
            }
        }
        delete c;
        delete uvc;
        break;
    }
    case OP_LPS:
    case OP_LPS0:
    {
        SmExtent2d d = rd.dom();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        SmPoint3d p = rd.vec();
        SmPoint2d g = rd.uv();
        SmBoolean found = FALSE;
        SmSolution sol;
        SmTerminationReasonType reason = SM_TR_UNDEFINED;
        int iters = 0;
        SmStatus r = (op == OP_LPS) ? s->LocalPointSolve(d, sop, p, g, found, sol)
                                    : s->LocalPointSolve_0(d, sop, p, g, found, sol, reason, iters);
        o.push_back(st(r));
        if (r != SM_SUCCESS)
            break;
        o.push_back(flg(found));
        if (found)
            put_solution(o, sol, 2);
        if (op == OP_LPS0)
        {
            o.push_back((double)reason);
            o.push_back((double)iters);
        }
        break;
    }
    case OP_DROP:
    {
        SmExtent2d d = rd.dom();
        SmPoint3d p = rd.vec();
        bool has_guess = rd.flag();
        SmPoint2d g = rd.uv();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        bool has_tgt = rd.flag();
        SmVector3d tgt = rd.vec();
        bool want = rd.flag();
        SmBoolean ok = FALSE, multi = FALSE;
        SmPoint2d uv(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        double gap = 0.0;
        SmSolutionArray sols;
        SmStatus r = s->DropPoint(p, d, has_guess ? &g : NULL, ok, uv, gap, multi, sop, has_tgt ? &tgt : NULL,
                                  want ? &sols : NULL);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back(flg(ok));
            o.push_back(uv.x);
            o.push_back(uv.y);
            o.push_back(gap);
            o.push_back(flg(multi));
            o.push_back(want ? (double)sols.GetSize() : -1.0);
        }
        break;
    }
    case OP_LSSI:
    {
        SmExtent2d d1 = rd.dom();
        SmSurface* s2 = read_surface(rd, ctx);
        SmExtent2d d2 = rd.dom();
        SmSurface* s3 = read_surface(rd, ctx);
        SmExtent2d d3 = rd.dom();
        SmPoint3d g1 = rd.vec();
        bool has2 = rd.flag();
        SmPoint3d g2 = rd.vec();
        SmBoolean found = FALSE;
        SmSolution sol;
        SmStatus r = s->LocalSurfaceSurfaceIntersect(d1, *s2, d2, *s3, d3, g1, has2 ? &g2 : NULL, found, sol);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back(flg(found));
            if (found)
                put_solution(o, sol, 6);
        }
        delete s2;
        delete s3;
        break;
    }
    case OP_COVER:
    {
        SmSurface* s2 = read_surface(rd, ctx);
        double tol = rd.num();
        bool merged = rd.flag();
        double max_dist = 0.0;
        SmSurface* cover = NULL;
        SmExtent2d md;
        SmStatus r = s->CoverOtherCoincidentSurface(s2, tol, max_dist, cover, NULL, NULL, merged ? &md : NULL);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back(cover == NULL ? -1.0 : (cover == s ? 0.0 : 1.0));
            o.push_back(max_dist);
        }
        delete s2;
        break;
    }
    case OP_FILTER:
    {
        SmExtent2d d = rd.dom();
        SmBSplineCurve* c = read_curve(rd, ctx);
        double i0 = rd.num(), i1 = rd.num();
        SmContinuityType uc[2], vc[2], tc[2];
        uc[0] = (SmContinuityType)rd.idx();
        uc[1] = (SmContinuityType)rd.idx();
        vc[0] = (SmContinuityType)rd.idx();
        vc[1] = (SmContinuityType)rd.idx();
        tc[0] = (SmContinuityType)rd.idx();
        tc[1] = (SmContinuityType)rd.idx();
        SmSolution sol;
        sol.m_eSolutionType = SM_ST_SINGLE_VALUE;
        sol.m_lNumVariables = 3;
        sol.m_vStart.m_dSolutionValue = rd.num();
        sol.m_vStart[0] = rd.num();
        sol.m_vStart[1] = rd.num();
        sol.m_vStart[2] = rd.num();
        SmBoolean keep = TRUE;
        SmStatus r = sc_srf_p5::sm_FilterSolution(s, d, *c, SmExtent1d(i0, i1), uc, vc, tc, sol, keep);
        o.push_back(st(r));
        o.push_back(flg(keep));
        delete c;
        break;
    }
    case OP_SP_LOCAL:
    {
        SmExtent2d d = rd.dom();
        SmContinuityType uc[2], vc[2];
        uc[0] = (SmContinuityType)rd.idx();
        uc[1] = (SmContinuityType)rd.idx();
        vc[0] = (SmContinuityType)rd.idx();
        vc[1] = (SmContinuityType)rd.idx();
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        SmBoolean solve_surf = rd.bflag(), proc_bnd = rd.bflag();
        SmSolutionArray sols;
        SmStatus r = sc_srf_p5::sm_SpLocalSolve(s, d, uc, vc, p, tol, sop, solve_surf, proc_bnd, sols);
        o.push_back(st(r));
        o.push_back((double)sols.GetSize());
        for (ULONG i = 0; i < sols.GetSize(); i++)
            put_solution(o, sols[i], 2);
        break;
    }
    case OP_IDENT:
    {
        int64_t kind = rd.idx();
        double tol = rd.num();
        SmSolution a, b;
        read_solution(rd, a);
        read_solution(rd, b);
        SmTree tree((SmObject*)s);
        SmExtent2d dom = s->GetNaturalUVDomain();
        SmExtent1d ivl(0.0, 1.0);
        const SmSolution* pref = NULL;
        SmSolutionPairType pr = SM_SP_DIFFCOUNTS;
        if (kind == 0)
        {
            sc_srf_p5::SmSPGlobalSolver gs(&tree, dom);
            gs.SetTol(tol);
            gs.SetSurf(s);
            pr = gs.IdenticalSolutions(a, b, pref);
        }
        else if (kind == 1)
        {
            sc_srf_p5::SmLSIGlobalSolver gs(&tree, dom);
            gs.SetTol(tol);
            gs.SetSurf(s);
            pr = gs.IdenticalSolutions(a, b, pref);
        }
        else if (kind == 2)
        {
            sc_srf_p5::SmCSGlobalSolver gs(&tree, ivl, dom);
            gs.SetTol(tol);
            gs.SetSurf(s);
            pr = gs.IdenticalSolutions(a, b, pref);
        }
        else
        {
            sc_srf_p5::SmCSIGlobalSolver gs(&tree, ivl, dom);
            gs.SetTol(tol);
            gs.SetSurf(s);
            pr = gs.IdenticalSolutions(a, b, pref);
        }
        o.push_back(0.0);
        o.push_back((double)pr);
        o.push_back(pref == NULL ? -1.0 : (pref == &a ? 0.0 : 1.0));
        break;
    }
    case OP_MAXDIST:
    {
        SmExtent2d d = rd.dom();
        SmBSplineCurve* c = read_curve(rd, ctx);
        double a = rd.num(), b = rd.num();
        ULONG ns = (ULONG)rd.idx();
        bool has_needed = rd.flag();
        double needed = rd.num();
        double found = 0.0, ct = 0.0;
        SmPoint2d sp(0.0, 0.0);
        SmStatus r = c->SurfaceMaxDistanceBetween(SmExtent1d(a, b), *s, d, ns, has_needed ? &needed : NULL, found, &ct, &sp);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back(found);
            o.push_back(ct);
            o.push_back(sp.x);
            o.push_back(sp.y);
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

SC_EXPORT int32_t sc_surface_surface_p5_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
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
