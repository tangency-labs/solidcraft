// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surface_p7`: the SmSurface members owned by the unit
// (source/SMLib/src/SmSurface.cpp, source/SMLib/inc/SmSurface.h), called through `SmSurface*`
// on SmBSplineSurface and SmPlane objects (virtual dispatch as upstream; `base` calls the
// SmSurface body explicitly), plus the co-ported Evaluate1stDerivatives,
// Evaluate2ndDerivatives and EvaluateDirectionalDerivs and the file-static helpers.
//
// Static functions of SmSurface.cpp cannot be linked against, so the helpers exercised here
// are VERBATIM copies of upstream SmSurface.cpp (commit c9c5979), inside the namespace
// sc_srf_p7 (line ranges noted at each block). sm_LoadGuesses takes its node's owner surface
// directly (two lines marked ADAPTED), as the port does: reaching it through a tree node
// needs a real SmSurfaceCache.
//
// One entry point, sc_surface_surface_p7_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/surface_surface_p7.rs, which validates every
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
#include <SmCurve.h>
#include <SmCurveBoundedSurface.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
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

namespace sc_srf_p7
{
// clang-format off
// ---- verbatim (two ADAPTED lines): SmSurface.cpp lines 4117-4285: sm_LoadGuesses
static SmStatus sm_LoadGuesses
  (const SmSurface      * pSurfaceOwner,      // ADAPTED: in : the node's owner surface (pSurfaceNode->GetOwnerObject())
   SmBezierPatch        * pBezPatch,          // NotUsed: in : SurfaceNode Bezier patch
   const SmExtent2d     & crUVDomain,         // in : Problem domain limits
   const SmPoint3d      & crTestPnt,          // in : target solution start point
   SmSolverOperationType  eSolverOperation,   // in : oneof  SM_SO_MINIMIZE,
                                              //             SM_SO_MAXIMIZE,
                                              //             SM_SO_NORMALIZE,
                                              //             SM_SO_INTERSECT
   SmPoint2d            * sGuesses,           // out: UVGuess Point or not used
   ULONG                & lNumGuesses)        // out: set to 1 or 0
{
  SM_REF1(pBezPatch) ;
  // check state - restricted set of operations
  SM_ASSERT(   eSolverOperation == SM_SO_MINIMIZE
            || eSolverOperation == SM_SO_MAXIMIZE
            || eSolverOperation == SM_SO_NORMALIZE
            || eSolverOperation == SM_SO_INTERSECT) ;

  // locals
  ULONG ii, jj;
  const SmSurface        *pSurface = pSurfaceOwner ; // ADAPTED (was: (SmSurface *)pSurfaceNode->GetOwnerObject())
  const SmBSplineSurface *pBSS     = SM_CAST_PTR(SmBSplineSurface, pSurface) ;
  SM_ASSERT(pSurface && pSurface->IsKindOf(SmSurface_TYPE)) ;

  // select a grid sample size
  constexpr ULONG lGridSize = 2;

  // set gridSize = 1 for linear geometry
  ULONG lGridSizeU = (pBSS && pBSS->GetNumberControlPoints( SM_SP_U ) == 2) ? 1 : lGridSize ;
  ULONG lGridSizeV = (pBSS && pBSS->GetNumberControlPoints( SM_SP_V ) == 2) ? 1 : lGridSize ;

  // get UV mid-point as 1st guess
  SmPoint2d sClosePtUV = crUVDomain.Evaluate(.5, .5) ;

  // Evaluate MidPoint
  SmPoint2d sUV;
  SmPoint3d sPt;
  SmVector3d sSu, sSv, sSuv, sSuu, sSvv;
  SmStatus sRtn = pSurface->Evaluate1stDerivatives(sClosePtUV, TRUE, TRUE, sPt, sSu, sSv) ;
  if(sRtn != SM_SUCCESS)
    {
      pSurface->EvaluatePoint( sClosePtUV, sPt );
    }

  // get current distance and upcoming NewtonRaphson values
  SmVector3d sSurfToPt = sPt - crTestPnt;
  double     dF        = sSurfToPt.Dot(sSu);
  double     dG        = sSurfToPt.Dot(sSv);

  // save current values as initial best values
  double     dBestDist = sSurfToPt.Length();
  double     dBestFG   = (sRtn == SM_SUCCESS) ? fabs(dF) + fabs(dG) : SM_BIG_DOUBLE ;

  // check each sample point - and check 1st NewtonRaphson step
  //   towards the surface point where the SurfaceNormal intersects
  //   target point.
  // First NewtonRaphson step: want (Su*Sv).(T-S) = 0, so
  //    find du, dv so
  //         0 = Su . (T-S) = F(u+du,v+dv) ~= F + du*Fu + dv*Fv  ==> [Fu Fv][du] = [-F]
  //         0 = Sv . (T-S) = G(u+du,v+dv) ~= G + du*Gu + dv*Gv      [Gu Gv][dv] = [-G]
  //    where S  = S(u,v) a point on the surface,
  //          Su = 1st u derivative
  //          Sv = 1st v derivative
  //          T  = Test point
  //
  //    solve for du,dv yields  [du] =    [ Gv -Fv][-F]
  //                            [dv]      [-Gu  Fu][-G]
  //                                   -------------------
  //                                   (Fu . Gv - Gu . Fv)

  // Make a clamping domain that runs halfway between the outermost
  // sample points and the real domain boundary.
  SmExtent2d sUVExt(crUVDomain.Evaluate(0.5 / ((double)(2*lGridSizeU)),
                                        0.5 / ((double)(2*lGridSizeV))),
                    crUVDomain.Evaluate(1.0 - 0.5 / ((double)(2*lGridSizeU)),
                                        1.0 - 0.5 / ((double)(2*lGridSizeV)))) ;

  // Sample every grid point - Take 1 NewtonRaphson Step from there and keep the best point
  for(ii=0;ii<lGridSizeU;ii++)
    {
      // select evenly spaced interior domain points
      //    exs: GridSize 1: [1/2]
      //         GridSize 2: [1/4 3/4],
      //         GridSize 3: [1/6 3/6 5/6], etc.
      double dPrmU = ((double)(2*ii+1)) / ((double)(2*lGridSizeU)) ;

      for(jj=0;jj<lGridSizeV;jj++)
        {
          double dPrmV = ((double)(2*jj+1)) / ((double)(2*lGridSizeV)) ;

          // skip grid points that don't evaluate to 2nd deriviatives
          sUV = crUVDomain.Evaluate( dPrmU, dPrmV ) ;
          sRtn = pSurface->Evaluate2ndDerivatives(sUV, TRUE, TRUE,
                                                  sPt, sSu, sSv,
                                                  sSuv, sSuu, sSvv) ;  // nonZeroTangents

          // get dist and NewtonRaphson function values from SamplePoint to TestPoint
          sSurfToPt = sPt - crTestPnt;
          dF        = sSurfToPt.Dot(sSu);
          dG        = sSurfToPt.Dot(sSv);
          double dThisDist = sSurfToPt.Length() ;
          double dThisFG   = (sRtn == SM_SUCCESS) ? fabs(dF) + fabs(dG) : SM_BIG_DOUBLE ;

          // Save the best grid point for the current operation
          if(   (eSolverOperation == SM_SO_MINIMIZE  && dThisDist < dBestDist)
             || (eSolverOperation == SM_SO_MAXIMIZE  && dThisDist > dBestDist)
             || (eSolverOperation == SM_SO_NORMALIZE && sRtn     == SM_SUCCESS && dThisFG < dBestFG  )
             || (eSolverOperation == SM_SO_INTERSECT && dThisDist < dBestDist))
            {
              sClosePtUV = sUV ;
              dBestDist  = dThisDist ;
              if(dThisFG < dBestFG) dBestFG = dThisFG ;
            }

          // skip sample points that failed to generate 2nd order derivative values
          if(sRtn != SM_SUCCESS)
            { continue ; }

          // Take 1 clamped Newton-Raphson step towards F=0 and G=0 and check if closer
          double dFu = sSu.Dot(sSu) + sSurfToPt.Dot(sSuu);
          double dFv = sSu.Dot(sSv) + sSurfToPt.Dot(sSuv);
          double dGu = dFv;
          double dGv = sSv.Dot(sSv) + sSurfToPt.Dot(sSvv);

          double dDen = dFu*dGv - dFv*dGu;
          if ( smos_Fabs( dDen ) < SM_EFF_ZERO_SQ )
            {
              continue;  // just ignore if singular.
            }

          double dNumU = dG*dFv  - dGv*dF;
          double dNumV = dF*dGu  - dG*dFu;

          sUV.Set(sUV.x + dNumU/dDen, sUV.y + dNumV/dDen);
          sUV = sUVExt.ClampPoint2d(sUV);

          // evaluate and judge the New Point
          sRtn = pSurface->Evaluate1stDerivatives(sUV, TRUE, TRUE, sPt, sSu, sSv) ;
          if(sRtn != SM_SUCCESS)
            { pSurface->EvaluatePoint( sUV, sPt ); }

          sSurfToPt = sPt - crTestPnt;
          dF        = sSurfToPt.Dot(sSu);
          dG        = sSurfToPt.Dot(sSv);
          dThisDist = sSurfToPt.Length();
          dThisFG   = fabs(dF) + fabs(dG) ;

          // Save the best NewtonRaphson point for the current operation
          if(   (eSolverOperation == SM_SO_MINIMIZE  && dThisDist < dBestDist)
             || (eSolverOperation == SM_SO_MAXIMIZE  && dThisDist > dBestDist)
             || (eSolverOperation == SM_SO_NORMALIZE && sRtn     == SM_SUCCESS && dThisFG < dBestFG  )
             || (eSolverOperation == SM_SO_INTERSECT && dThisDist < dBestDist))
            {
              sClosePtUV = sUV ;
              dBestDist  = dThisDist ;
              if(dThisFG < dBestFG) dBestFG = dThisFG ;
            }
        } // end iter every jj grid index
    } // end iter every ii grid index

  // set output
  sGuesses[0] = sClosePtUV;
  lNumGuesses = 1;

  // all done
  return SM_SUCCESS;

} // end sm_LoadGuesses

// ---- verbatim: SmSurface.cpp lines 4750-4805: sm_PassSCConvergenceTest
static SmBoolean sm_PassSCConvergenceTest
  (const SmSurface & crSurface,   // in :
   const SmPoint2d & crUV,        // in :
   const SmCurve   & crCurve,     // in :
   double            dParameter)  // in :
{
  // surface eval
  SmPoint3d sPnt;
  SmVector3d sDU, sDV;
  crSurface.Evaluate1stDerivatives(crUV,TRUE,TRUE,sPnt,sDU,sDV);  // nonZeroTangents

  // curve eval
  SmVector3d sPV[2]; // sPV[0] = crCurve 3dPoint    for param = dParameter
                     // sPV[1] = crCurve 3dTangent  for param = dParameter
  crCurve.Evaluate(dParameter,1,TRUE,sPV, TRUE);  // NonZeroTangents

  // curve surface displacement
  SmVector3d sDiff = sPnt - sPV[0];
  double dScale = 1.0 + sPnt.GetMaxDimension();

  // when surf and curve tangents are nonZero
  // and displacement is nonZero
  // and angle between any tangent and the displacement is not 90 degrees
  if(   smos_Fabs(sDU.Dot(sDiff))    > SM_EFF_ZERO * dScale
     || smos_Fabs(sDV.Dot(sDiff))    > SM_EFF_ZERO * dScale
     || smos_Fabs(sPV[1].Dot(sDiff)) > SM_EFF_ZERO * dScale)
       {
      sDiff.Unitize();
      if (sDU.LengthSquared() > SM_EFF_ZERO_SQ)
        {
          sDU.Unitize();
          if (smos_Fabs(sDU.Dot(sDiff)) > SM_EFF_ZERO)
            {
              return FALSE;
            }
        }
      if (sDV.LengthSquared() > SM_EFF_ZERO_SQ)
        {
          sDV.Unitize();
          if (smos_Fabs(sDV.Dot(sDiff)) > SM_EFF_ZERO)
            {
              return FALSE;
            }
        }
      if (sPV[1].LengthSquared() > SM_EFF_ZERO_SQ)
        {
          sPV[1].Unitize();
          if (smos_Fabs(sPV[1].Dot(sDiff)) > SM_EFF_ZERO)
            {
              return FALSE;
            }
        }
    }
  return TRUE;

} // end sm_PassSCConvergenceTest

// ---- verbatim: SmSurface.cpp lines 7837-7921: sm_CheckSeamDrop
static SmStatus sm_CheckSeamDrop(
          SmSurface  *pThisSurf,
          SmPoint2d  &sThisUV,  // in/out: STEP parameter, possibly moved across seam
    const SmExtent2d &sThisDomain,
    const SmSurface  *pOtherSurf,
    const SmPoint2d  &sOtherUV,  // in: Nurbs parameter
    const SmExtent2d &sOtherDomain,
          SmLocationType eLocation) // where the dropped point is on pThisSurf.
{

  if ( eLocation == SM_LT_POLE )
    { return SM_ERR; }

  if ( eLocation != SM_LT_U_SEAM && eLocation != SM_LT_V_SEAM && eLocation != SM_LT_UV_SEAM )
    { return SM_SUCCESS; }

  // pOther point is dropped onto a seam in pThis.
  // sUVOther is a boundary point on pOther.
  // pOther is evauated in Nurbs parameterization, pThis is STEP.

  SmVector3d sPtDer[4];
  pThisSurf->EvaluateSTEP( sThisUV, 1, 1, TRUE, TRUE, TRUE, sPtDer );

  SmVector3d sPt, sThisDeriv, sOtherU, sOtherV, sOtherDeriv;

  double dDotU, dDotV;

  pOtherSurf->Evaluate1stDerivatives( sOtherUV, TRUE, TRUE, sPt, sOtherU, sOtherV );

  // Unitize them, so that dot-product comparisons mean direction only.
  if ( sOtherU.Unitize() != SM_SUCCESS )
    { return SM_ERR; }
  if ( sOtherV.Unitize() != SM_SUCCESS )
    { return SM_ERR; }

  // Flip directions of the derivatives if they are at the top of their domains,
  // so that the derivatives point into the surface.
  if ( sOtherUV.x > sOtherDomain.GetUInterval().GetMid() )
    { sOtherU = -sOtherU; }
  if ( sOtherUV.y > sOtherDomain.GetVInterval().GetMid() )
    { sOtherV = -sOtherV; }

  if ( eLocation == SM_LT_U_SEAM || eLocation == SM_LT_UV_SEAM )
    {
      // Closed in U: sUVThis.x is at low end of u-range but could be the high end.
      // Get our u-deriv, and whichever deriv of pOtherSurf that lines up with it.
      sThisDeriv = sPtDer[2];

      dDotU = sThisDeriv.Dot( sOtherU );
      dDotV = sThisDeriv.Dot( sOtherV );

      if ( smos_Fabs( dDotU ) > smos_Fabs( dDotV ) )
        { sOtherDeriv = sOtherU; }
      else
        { sOtherDeriv = sOtherV; }

      if ( sThisDeriv.Dot( sOtherDeriv ) > 0.0 )
        { sThisUV.x = sThisDomain.GetMin().x; }
      else
        { sThisUV.x = sThisDomain.GetMax().x; }
    }

  if ( eLocation == SM_LT_V_SEAM || eLocation == SM_LT_UV_SEAM )
    {
      // Closed in V: sUVThis.y is at low end of v-range but could be the high end.
      // Get our v-deriv, and whichever deriv of pOtherSurf that lines up with it.
      sThisDeriv = sPtDer[1];

      dDotU = sThisDeriv.Dot( sOtherU );
      dDotV = sThisDeriv.Dot( sOtherV );

      if ( smos_Fabs( dDotU ) > smos_Fabs( dDotV ) )
        { sOtherDeriv = sOtherU; }
      else
        { sOtherDeriv = sOtherV; }

      if ( sThisDeriv.Dot( sOtherDeriv ) > 0.0 )
        { sThisUV.y = sThisDomain.GetMin().y; }
      else
        { sThisUV.y = sThisDomain.GetMax().y; }
    }

  return SM_SUCCESS;

} // end sm_CheckSeamDrop

// ---- verbatim: SmSurface.cpp lines 10256-10359: sm_GoodDropResult
static SmBoolean sm_GoodDropResult(
        const SmSurface  * pSurface,   // in
        const SmExtent2d & crDomain,   // in
        const SmPoint3d  & crPtToDrop, // in
        const SmPoint2d  & crGuessUV,  // in
        const SmSolution & crSol       // in: result of drop using crGuessUV
    )
{
  // Check whether it's right on.
  if ( crSol.m_vStart.m_dSolutionValue < SM_EFF_ZERO )
  {
      return TRUE;
  }

  SmPoint2d sResultUV( crSol.m_vStart[0], crSol.m_vStart[1] );

  // If result is far from guess, it's suspicious.
  constexpr double dMaxStep = 0.50; // 50% of domain: big steps do happen.
  SmVector2d sUVStep( sResultUV - crGuessUV );
  if ( smos_Fabs( sUVStep.x ) > dMaxStep * crDomain.XLength() ) {
      return FALSE;
  }
  if ( smos_Fabs( sUVStep.y ) > dMaxStep * crDomain.YLength() ) {
      return FALSE;
  }
  // end if check on big step size.

  // Check whether it's a solution or not.
  // Note, function values could be bad if it's legitimately off a boundary
  // (assuming SM_SO_MINIMIZE); must check for that.

  // Evaluate the error functions: how far from the surface normal.
  // Note, this test is loose: if the local solver can get anywhere near,
  // then it should be good.
  SmPoint3d sSrfPoint;
  SmVector3d Su, Sv;
  pSurface->Evaluate1stDerivatives( sResultUV, TRUE, TRUE, sSrfPoint, Su, Sv );
  Su.Unitize();
  Sv.Unitize();
  SmVector3d sGap( crPtToDrop - sSrfPoint );
  double Fu = sGap.Dot( Su );
  double Fv = sGap.Dot( Sv );
  if ( smos_Fabs( Fu ) + smos_Fabs( Fv ) < 0.0001 ) {
      return TRUE;
  }

  // Need to check legitimately off boundary.
  // If it's off the lower end of a domain, and the solution is before the
  // beginning, then the dot of Gap with the deriv will be negative, so a
  // positive value indicates error; reverse for the top of a domain.

  // Check u-param first.
  if ( sResultUV.x < crDomain.GetMin().x + SM_EFF_ZERO )
  {
      if ( Fu >  SM_EFF_ZERO ) {
          return FALSE;
      }
  }
  else if ( sResultUV.x > crDomain.GetMax().x - SM_EFF_ZERO )
  {
      if ( Fu < -SM_EFF_ZERO ) {
          return FALSE;
      }
  }
  else if ( smos_Fabs( Fu ) > 0.001 )
  {
      return FALSE; // interior, bad fcn value.
  }
  // end check u-param.

  // Now check v-param.
  if ( sResultUV.y < crDomain.GetMin().y + SM_EFF_ZERO )
  {
      if ( Fv >  SM_EFF_ZERO ) {
          return FALSE;
      }
  }
  else if ( sResultUV.y > crDomain.GetMax().y - SM_EFF_ZERO )
  {
      if ( Fv < -SM_EFF_ZERO ) {
          return FALSE;
      }
  }
  else if ( smos_Fabs( Fv ) > 0.001 )
  {
      return FALSE; // interior, bad fcn value.
  }
  // end check v-param.

  // One more: if the solution 3d point is farther than the guess.
  // (Remember, this routine uses MINIMIZE.)
  SmPoint3d sGuess3dPt;
  pSurface->EvaluatePoint( crGuessUV, sGuess3dPt );
  double dDsqGuess  = crPtToDrop.DistanceBetweenSquared( sSrfPoint );
  double dDsqResult = crPtToDrop.DistanceBetweenSquared( sGuess3dPt );
  if ( dDsqResult > dDsqGuess )
  {
      return FALSE;
  }

  // If we made it here, there's nothing to complain about.
  return TRUE;

} // end helper function sm_GoodDropResult

// ---- verbatim: SmSurface.cpp lines 20854-20902: sm_PassSSConvergenceTest
static SmBoolean sm_PassSSConvergenceTest
  (const SmSurface & crSurface,
   const SmPoint2d & crUV,
   const SmSurface & crSurface2,
   const SmPoint2d & crUV2)

{
    SmPoint3d sPnt;
    SmVector3d sDU, sDV;
    crSurface.Evaluate1stDerivatives(crUV,TRUE,TRUE,sPnt,sDU,sDV);
    SmPoint3d sPnt2;
    SmVector3d sDU2, sDV2;
    crSurface2.Evaluate1stDerivatives(crUV2,TRUE,TRUE,sPnt2,sDU2,sDV2);
    SmVector3d sDiff = sPnt - sPnt2;
    if (smos_Fabs(sDU.Dot(sDiff)) > SM_EFF_ZERO ||
        smos_Fabs(sDV.Dot(sDiff)) > SM_EFF_ZERO ||
        smos_Fabs(sDU2.Dot(sDiff)) > SM_EFF_ZERO ||
        smos_Fabs(sDV2.Dot(sDiff)) > SM_EFF_ZERO) {
        if (sDiff.LengthSquared() > SM_EFF_ZERO_SQ) {
            sDiff.Unitize();
        }
        if (sDU.LengthSquared() > SM_EFF_ZERO_SQ) {
            sDU.Unitize();
            if (smos_Fabs(sDU.Dot(sDiff)) > SM_EFF_ZERO) {
                return FALSE;
            }
        }
        if (sDV.LengthSquared() > SM_EFF_ZERO_SQ) {
            sDV.Unitize();
            if (smos_Fabs(sDV.Dot(sDiff)) > SM_EFF_ZERO) {
                return FALSE;
            }
        }
        if (sDU2.LengthSquared() > SM_EFF_ZERO_SQ) {
            sDU2.Unitize();
            if (smos_Fabs(sDU2.Dot(sDiff)) > SM_EFF_ZERO) {
                return FALSE;
            }
        }
        if (sDV2.LengthSquared() > SM_EFF_ZERO_SQ) {
            sDV2.Unitize();
            if (smos_Fabs(sDV2.Dot(sDiff)) > SM_EFF_ZERO) {
                return FALSE;
            }
        }
    }
    return TRUE;

} // end sm_PassSSConvergenceTest

// clang-format on
} // namespace sc_srf_p7

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_surface_p7.rs).
enum : int32_t
{
    OP_EVAL1 = 0,
    OP_EVAL2 = 1,
    OP_DIRDERIV = 2,
    OP_NORMAL = 3,
    OP_IS_CLOSED = 4,
    OP_IS_PLANAR = 5,
    OP_APPROX_LEN = 6,
    OP_HIGHER = 7,
    OP_INWARD = 8,
    OP_DROP_PTS = 9,
    OP_DROP_VECS = 10,
    OP_DEGEN = 11,
    OP_FLAT = 12,
    OP_BREP = 13,
    OP_GPS0 = 14,
    OP_GCS = 15,
    OP_GSS = 16,
    OP_LCS = 17,
    OP_PROJ = 18,
    OP_LOAD_GUESSES = 19,
    OP_PASS_SC = 20,
    OP_PASS_SS = 21,
    OP_GOOD_DROP = 22,
    OP_CHECK_SEAM = 23,
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

// The first SOLS_OUT solutions of a global solve: type, value, variable count (at most
// 4), the start parameters and, for a range, the end parameters (surface.surface_p11).
const ULONG SOLS_OUT = 16;
void put_sols(std::vector<double>& o, SmSolutionArray& sols)
{
    for (ULONG i = 0; i < sols.GetSize() && i < SOLS_OUT; i++)
    {
        SmSolution& s = sols[i];
        ULONG n = s.m_lNumVariables < 4 ? s.m_lNumVariables : 4;
        o.push_back((double)s.m_eSolutionType);
        o.push_back(s.m_vStart.m_dSolutionValue);
        o.push_back((double)n);
        for (ULONG j = 0; j < n; j++)
            o.push_back(s.m_vStart[j]);
        if (s.m_eSolutionType == SM_ST_RANGE_OF_VALUES)
            for (ULONG j = 0; j < n; j++)
                o.push_back(s.m_vEnd[j]);
    }
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

void run(int32_t op, Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    SmSurface* s = read_surface(rd, ctx);
    switch (op)
    {
    case OP_EVAL1:
    {
        SmPoint2d uv = rd.uv();
        SmBoolean ul = rd.bflag(), vl = rd.bflag();
        SmPoint3d p;
        SmVector3d du, dv;
        SmStatus r = s->Evaluate1stDerivatives(uv, ul, vl, p, du, dv);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            put_vec(o, p);
            put_vec(o, du);
            put_vec(o, dv);
        }
        break;
    }
    case OP_EVAL2:
    {
        SmPoint2d uv = rd.uv();
        SmBoolean ul = rd.bflag(), vl = rd.bflag();
        SmPoint3d p;
        SmVector3d du, dv, duv, duu, dvv;
        SmStatus r = s->Evaluate2ndDerivatives(uv, ul, vl, p, du, dv, duv, duu, dvv);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
            for (const SmVector3d* v : {&p, &du, &dv, &duv, &duu, &dvv})
                put_vec(o, *v);
        break;
    }
    case OP_DIRDERIV:
    {
        SmPoint2d uv = rd.uv();
        SmVector2d dir = rd.uv();
        ULONG h = (ULONG)rd.idx();
        SmBoolean ul = rd.bflag(), vl = rd.bflag();
        SmVector3d d[4];
        SmStatus r = s->EvaluateDirectionalDerivs(uv, dir, h, d, ul, vl);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
            for (ULONG i = 0; i <= h; i++)
                put_vec(o, d[i]);
        break;
    }
    case OP_NORMAL:
    {
        SmPoint2d uv = rd.uv();
        SmBoolean ul = rd.bflag(), vl = rd.bflag();
        bool base = rd.flag();
        SmVector3d n(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        SmStatus r = base ? s->SmSurface::EvaluateNormal(uv, ul, vl, n) : s->EvaluateNormal(uv, ul, vl, n);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
            put_vec(o, n);
        break;
    }
    case OP_IS_CLOSED:
    {
        SmExtent2d d = rd.dom();
        SmSurfParamType param = (SmSurfParamType)rd.idx();
        bool has_tol = rd.flag();
        double tol = rd.num();
        bool want = rd.flag();
        bool base = rd.flag();
        SmContinuityType c = SM_CT_UNDEFINED;
        SmBoolean r = base ? s->SmSurface::IsClosed(d, param, has_tol ? &tol : NULL, want ? &c : NULL)
                           : s->IsClosed(d, param, has_tol ? &tol : NULL, want ? &c : NULL);
        o.push_back(0.0);
        o.push_back(flg(r));
        o.push_back(want ? (double)c : -1.0);
        break;
    }
    case OP_IS_PLANAR:
    {
        double tol = rd.num();
        bool base = rd.flag();
        SmPoint3d p(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        SmVector3d n(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        double dev = SM_UNDEF_DOUBLE;
        SmBoolean r = base ? s->SmSurface::IsPlanar(tol, &p, &n, &dev) : s->IsPlanar(tol, &p, &n, &dev);
        o.push_back(0.0);
        o.push_back(flg(r));
        put_vec(o, p);
        put_vec(o, n);
        o.push_back(dev);
        break;
    }
    case OP_APPROX_LEN:
    {
        SmExtent2d d = rd.dom();
        SmVector2d r = s->ApproxDerivativeLengths(d);
        o.push_back(0.0);
        put_uv(o, r);
        break;
    }
    case OP_HIGHER:
    {
        SmPoint2d uv = rd.uv();
        SmVector2d g1 = rd.uv();
        bool w3 = rd.flag(), w4 = rd.flag();
        SmVector3d g2, g3, g4;
        SmStatus r = s->ComputeHigherOrderDerivs(uv, g1, g2, w3 ? &g3 : NULL, w4 ? &g4 : NULL);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            put_vec(o, g2);
            if (w3)
                put_vec(o, g3);
            if (w4)
                put_vec(o, g4);
        }
        break;
    }
    case OP_INWARD:
    {
        SmPoint2d uv = rd.uv();
        SmVector3d v;
        SmStatus r = s->ComputeInwardVector(uv, v);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
            put_vec(o, v);
        break;
    }
    case OP_DROP_PTS:
    case OP_DROP_VECS:
    {
        SmPoint2d uv = rd.uv();
        SmBoolean ul = rd.bflag(), vl = rd.bflag();
        std::vector<SmVector3d> v = read_vecs(rd);
        std::vector<SmVector2d> res(v.size());
        SmStatus r = op == OP_DROP_PTS
                         ? s->DropPointsToTangentPlane(uv, ul, vl, (ULONG)v.size(), v.data(), res.data())
                         : s->DropVectors(uv, ul, vl, (ULONG)v.size(), v.data(), res.data());
        o.push_back(st(r));
        if (r == SM_SUCCESS)
            for (const SmVector2d& x : res)
                put_uv(o, x);
        break;
    }
    case OP_DEGEN:
    {
        SmPoint2d uv = rd.uv();
        SmVector3d dir = rd.vec();
        SmBoolean from_pole = rd.bflag();
        SmSurfParamType sdir = (SmSurfParamType)rd.idx();
        double tol = rd.num();
        SmStatus r = s->FindDegenParamForDirection(uv, dir, from_pole, sdir, tol);
        o.push_back(st(r));
        put_uv(o, uv);
        break;
    }
    case OP_FLAT:
    {
        double ang = rd.num();
        bool want = rd.flag();
        SmTArray<SmPoint3d> pts;
        ULONG r = s->GetFlatCorners(want ? &pts : NULL, ang);
        o.push_back(0.0);
        o.push_back((double)r);
        if (want)
            for (ULONG i = 0; i < pts.GetSize(); i++)
                put_vec(o, pts[i]);
        break;
    }
    case OP_BREP:
        o.push_back(0.0);
        o.push_back(flg(s->GetBrep() != NULL));
        break;
    case OP_GPS0:
    {
        SmExtent2d d = rd.dom();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        double tgt = 0.0;
        const double* pt = opt_target(rd, tgt);
        SmSolutionRequestedType req = (SmSolutionRequestedType)rd.idx();
        SmSolutionArray sols;
        SmStatus r = s->GlobalPointSolve_0(d, sop, p, tol, pt, req, sols);
        o.push_back(st(r));
        o.push_back((double)sols.GetSize());
        break;
    }
    case OP_GCS:
    case OP_LCS:
    {
        SmExtent2d d = rd.dom();
        SmBSplineCurve* c = read_curve(rd, ctx);
        double a = rd.num(), b = rd.num();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        double tol = rd.num();
        std::vector<SmVector3d> vecs = read_vecs(rd);
        const SmVector3d* pv = vecs.empty() ? NULL : vecs.data();
        if (op == OP_GCS)
        {
            double tgt = 0.0;
            const double* pt = opt_target(rd, tgt);
            SmSolutionRequestedType req = (SmSolutionRequestedType)rd.idx();
            SmSolutionArray sols;
            SmStatus r = s->GlobalCurveSolve(d, *c, SmExtent1d(a, b), sop, tol, pt, pv, req, sols);
            o.push_back(st(r));
            o.push_back((double)sols.GetSize());
            if (r == SM_SUCCESS)
                put_sols(o, sols);
        }
        else
        {
            SmPoint2d guv = rd.uv();
            double gt = rd.num();
            SmBoolean found = FALSE;
            SmSolution sol;
            SmStatus r = s->LocalCurveSolve(d, *c, SmExtent1d(a, b), sop, tol, NULL, pv, guv, gt, found, sol);
            o.push_back(st(r));
            o.push_back(flg(found));
            if (r == SM_SUCCESS && found)
            {
                o.push_back(sol.m_vStart.m_dSolutionValue);
                o.push_back(sol.m_vStart[0]);
                o.push_back(sol.m_vStart[1]);
                o.push_back(sol.m_vStart[2]);
            }
        }
        delete c;
        break;
    }
    case OP_GSS:
    {
        SmExtent2d d = rd.dom();
        SmSurface* s2 = read_surface(rd, ctx);
        SmExtent2d d2 = rd.dom();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        double tol = rd.num();
        std::vector<SmVector3d> vecs = read_vecs(rd);
        double tgt = 0.0;
        const double* pt = opt_target(rd, tgt);
        SmSolutionRequestedType req = (SmSolutionRequestedType)rd.idx();
        SmSolutionArray sols;
        SmStatus r = s->GlobalSurfaceSolve(d, *s2, d2, sop, tol, pt, vecs.empty() ? NULL : vecs.data(), req, sols);
        o.push_back(st(r));
        o.push_back((double)sols.GetSize());
        if (r == SM_SUCCESS)
            put_sols(o, sols);
        delete s2;
        break;
    }
    case OP_PROJ:
    {
        SmExtent2d d = rd.dom();
        SmBSplineCurve* c = read_curve(rd, ctx);
        SmVector3d v = rd.vec();
        double dist = 0.0;
        const double* pd = opt_target(rd, dist);
        SmTArray<SmCurve*> c3d, cuv;
        SmStatus r = s->CreateParallelProjectionCurves(ctx, d, *c, v, pd, NULL, NULL, &c3d, &cuv);
        o.push_back(st(r));
        o.push_back((double)c3d.GetSize());
        for (ULONG i = 0; i < c3d.GetSize(); i++)
            delete c3d[i];
        for (ULONG i = 0; i < cuv.GetSize(); i++)
            delete cuv[i];
        delete c;
        break;
    }
    case OP_LOAD_GUESSES:
    {
        SmExtent2d d = rd.dom();
        SmPoint3d p = rd.vec();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        SmPoint2d g[1];
        ULONG n = 0;
        SmStatus r = sc_srf_p7::sm_LoadGuesses(s, NULL, d, p, sop, g, n);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
            put_uv(o, g[0]);
        break;
    }
    case OP_PASS_SC:
    {
        SmPoint2d uv = rd.uv();
        SmBSplineCurve* c = read_curve(rd, ctx);
        double t = rd.num();
        o.push_back(0.0);
        o.push_back(flg(sc_srf_p7::sm_PassSCConvergenceTest(*s, uv, *c, t)));
        delete c;
        break;
    }
    case OP_PASS_SS:
    {
        SmPoint2d uv = rd.uv();
        SmSurface* s2 = read_surface(rd, ctx);
        SmPoint2d uv2 = rd.uv();
        o.push_back(0.0);
        o.push_back(flg(sc_srf_p7::sm_PassSSConvergenceTest(*s, uv, *s2, uv2)));
        delete s2;
        break;
    }
    case OP_GOOD_DROP:
    {
        SmExtent2d d = rd.dom();
        SmPoint3d p = rd.vec();
        SmPoint2d g = rd.uv();
        SmSolution sol;
        sol.m_vStart.m_dSolutionValue = rd.num();
        sol.m_vStart[0] = rd.num();
        sol.m_vStart[1] = rd.num();
        o.push_back(0.0);
        o.push_back(flg(sc_srf_p7::sm_GoodDropResult(s, d, p, g, sol)));
        break;
    }
    case OP_CHECK_SEAM:
    {
        SmPoint2d uv = rd.uv();
        SmExtent2d d = rd.dom();
        SmSurface* s2 = read_surface(rd, ctx);
        SmPoint2d uv2 = rd.uv();
        SmExtent2d d2 = rd.dom();
        SmLocationType loc = (SmLocationType)rd.idx();
        SmStatus r = sc_srf_p7::sm_CheckSeamDrop(s, uv, d, s2, uv2, d2, loc);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
            put_uv(o, uv);
        delete s2;
        break;
    }
    default:
        o.push_back((double)SC_ERR_ARG);
        break;
    }
    delete s;
}
} // namespace

SC_EXPORT int32_t sc_surface_surface_p7_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
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
