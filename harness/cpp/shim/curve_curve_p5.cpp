// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.curve_p5`: the SmCurve members owned by the unit
// (source/SMLib/src/SmCurve.cpp, source/SMLib/inc/SmCurve.h) called through `SmCurve*` on
// SmBSplineCurve, SmHermiteCurve, SmLine, SmEllipse and SmCircle objects (CrvsCleanup,
// RemoveDuplicateCurves, EvaluateSTEPPoint, CacheMakeOrValidate, IsArc, IsLine, IsLinear,
// IsDomainBoundary, IsPeriodic, Length, LocalPropertyAnalysis), plus the file-static helpers
// and local evaluator classes of SmCurve.cpp (sm_IsPointSolutionAcrossClosedSeamBoundary,
// sm_FindAdditionalCpGuess, SmCCTangentIntENFO, SmCCIntersectENFO, SmFindCCNormalENFO).
//
// Static functions and classes local to SmCurve.cpp cannot be linked against and the file
// cannot be recompiled inside a namespace (it defines SmCurve members), so the helpers used
// here are VERBATIM copies of upstream SmCurve.cpp (commit c9c5979), inside the namespace
// sc_crv_p5 (line ranges noted at each block; generated, do not edit by hand).
//
// One entry point, sc_curve_curve_p5_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/curve_curve_p5.rs, which validates every
// input. A curve block is `kind dim ...` (0: SmBSplineCurve `oob n p m cpts(x y z w)
// knots`; 1: SmHermiteCurve `P1 D1 P2 D2`; 2: bounded SmLine `P V d0 d1 scale io`;
// 3: SmEllipse `C X Y d0 d1 rx ry io`; 4: SmCircle `C X Y d0 d1 r io`; 5: kind 0 with the
// given knots written back over the curve's own NURB after construction). `out` receives the
// op status (0 = SM_SUCCESS, else the SmStatus value) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBrep.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmCrvOnSurf.h>
#include <SmCurve.h>
#include <SmCurveCache.h>
#include <SmCurveTypes.h>
#include <SmEdge.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmGeomUtility.h>
#include <SmGlobalSolver.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmLocalSolve1d.h>
#include <SmLocalSolveNd.h>
#include <SmMatrix.h>
#include <SmNurbsCrv.h>
#include <SmSolutionArray.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <map>
#include <stdexcept>
#include <vector>

#include "sc_sm.h"
#include "sc_gw.h"

using sc::Rd;
using sc::put_vec;
using sc::read_nurb;

namespace sc_crv_p5
{
// clang-format off
// ---- verbatim: SmCurve.cpp lines 216-226: sm_ConvertToParameterDelta
static double sm_ConvertToParameterDelta
  (const SmVector3d & crTangentVector,
   const SmVector3d & crDeltaVector)
{
    double dTanLen = crTangentVector.Length();
    if (dTanLen < SM_EFF_ZERO) { return 0.0; }
    SmVector3d sUnitTan = crTangentVector / dTanLen;
    double dDot = sUnitTan.Dot(crDeltaVector);
    return dDot/dTanLen;

} // end sm_ConvertToParameterDelta
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 4259-4303: sm_IsPointSolutionAcrossClosedSeamBoundary
static SmBoolean sm_IsPointSolutionAcrossClosedSeamBoundary
  (const SmSolution &rSol,            // in : Solution to review
   const SmCurve &crThisCurve,        // in : target curve
   const SmExtent1d &crInterval,      // in : target curve interval
   const SmCurve &crOtherCurve,       // in : Other curve of curve/curve intersection
   const SmExtent1d &crOtherInterval) // in : Other curve interval
{
  // check state
  SM_ASSERT(   rSol.m_lNumObjects == 2
            && (   rSol.m_apObjects[0] == (SmObject *)&crThisCurve
                || rSol.m_apObjects[0] == (SmObject *)&crOtherCurve)
            && (   rSol.m_apObjects[1] == (SmObject *)&crThisCurve
                || rSol.m_apObjects[1] == (SmObject *)&crOtherCurve)) ;

  double dStart1, dStart2 ;
  if((SmObject *)&crThisCurve == rSol.m_apObjects[0]) { dStart1 = rSol.m_vStart[0] ;
                                                        dStart2 = rSol.m_vStart[1] ;
                                                      }
  else                                                { dStart1 = rSol.m_vStart[1] ;
                                                        dStart2 = rSol.m_vStart[0] ;
                                                      }

  // when point solution is on a closedCurve endPoint
  if(   rSol.m_eSolutionType == SM_ST_SINGLE_VALUE
   && crThisCurve.IsClosed(crInterval)
   && crInterval.IsValueOnBoundary(dStart1))
    {
      // get Curves' pos and vec at solution point
      SmPoint3d sPV1[2], sPV2[2] ;
      crThisCurve.Evaluate (dStart1, 1, TRUE, sPV1, TRUE) ;   // NonZeroTangent values
      crOtherCurve.Evaluate(dStart2, 1, TRUE, sPV2, TRUE) ;   // NonZeroTangent values

      // negate endPoint tangents so that they point into their intervals
      if(SM_ARE_SAME(dStart1, crInterval.GetMax()))      sPV1[1] = -sPV1[1] ;
      if(SM_ARE_SAME(dStart2, crOtherInterval.GetMax())) sPV2[1] = -sPV2[1] ;

      // When Curve Tangents oppose - assume sol is connecting
      // different sides of a seam
      if(sPV1[1].Dot(sPV2[1]) < 0.0)
        { return(TRUE) ; }
    } // end clsed curve endPoint solution
  // else
  return(FALSE) ;

} // end sm_IsPointSolutionAcrossClosedSeamBoundary
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 4549-4599: sm_UnitizeTangent
static SmStatus sm_UnitizeTangent
 (SmVector3d         rPVV[3],
  const SmVector3d & crGapVec,        // vec from this curve to the other
  SmVector3d       & rUnitTan,
  double           * pdLength = NULL) // just for convenience
{
  double dLen = rPVV[1].Length();
  if ( pdLength != NULL ) { *pdLength = dLen; }

  if ( dLen > SM_EFF_ZERO )  // that's where Unitize will barf.
  {
      rUnitTan = rPVV[1] / dLen;
      return SM_SUCCESS;
  }

  // Ok, what to do with a zero 1st derivative.
  // One standard thing is to set its direction the same as the 2nd deriv:
  // that's its limiting direction, the direction that it will be heading
  // with just a minuscule increase in parameter.
  // But, this is tailored to curve/curve intersection, and the zero derivative
  // is probably because we're using a projected curve.
  // In the circle example, we're at an extremum, and the second derivative
  // vector points towards the circle center, which is back in the direction
  // that this curve will be moving.
  // Here's the idea: if that direction agrees with the gap vector, then a
  // little tweak in this curve's parameter will help us get moving.  But if
  // the directions don't agree, then we're trying to move beyond the range
  // of this curve.  In that case, return the zero derivative.

  double dDot = rPVV[2].Dot( crGapVec );
  if ( dDot > SM_EFF_ZERO )
  {
      dLen = rPVV[2].Length();
      if ( dLen > SM_EFF_ZERO )  // that's where Unitize will barf.
      {
          rUnitTan = rPVV[2] / dLen;
          return SM_SUCCESS;
      }
      // Well, since the dot product was not zero, then neither vector
      // could be zero, so how could we get here?  Whatever:
      dLen = crGapVec.Length();
      if ( dLen > SM_EFF_ZERO )  // that's where Unitize will barf.
      {
          rUnitTan = crGapVec / dLen;
          return SM_SUCCESS;
      }
  }
  rUnitTan.Set( 0,0,0 );
  return SM_SUCCESS;

} // end local_UnitizeTangent
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 12398-12474: sm_FindAdditionalCpGuess
static SmStatus sm_FindAdditionalCpGuess
  (const SmCurve    & crCurve,
   const SmExtent1d & crInterval,
   const SmVector3d & crABC,
   double             dTsectT,
   SmBoolean        & rbFoundGuess,
   double           & rdNewTGuess)
{
    rbFoundGuess = FALSE;

    SmVector3d sPV[2];
    SER(crCurve.Evaluate(dTsectT,1,TRUE,sPV,TRUE)); // nonZeroTangent values

    // First do a quick test to determine the angle of
    // intersection between the curve and  and see if we need to look
    // for a tangency.  If within 10 degrees look for tangency.
    if (!sPV[1].IsPerpendicularTo(crABC,10)) {
        return SM_SUCCESS;
    }

    // Get the geometric properties of the curve at the intersection
    SmVector3d sGeomVec[4];
    SER(crCurve.EvaluateGeometric(dTsectT,2,TRUE,sGeomVec));
    SmVector3d sCurvatureVector = sGeomVec[2];
    double dCurveCurvature = sCurvatureVector.Length();
    if (dCurveCurvature > SM_EFF_ZERO) SER(sCurvatureVector.Unitize());

    // If the curve curvature is zero then no other intersections near here
    if (smos_Fabs(dCurveCurvature) < SM_EFF_ZERO) { return SM_SUCCESS; }

    // Otherwise intersect plane and circle.
    double dCircleRadius;
    SmPoint3d sCircleCenter;
    SmVector3d sCircleNormal;
    SmVector3d sFoundVector;

    dCircleRadius = smos_Fabs(1.0 / dCurveCurvature);
    sCircleCenter = sPV[0] + sCurvatureVector * dCircleRadius;
    sCircleNormal = sGeomVec[1] * sGeomVec[2];
    if (sCircleNormal.LengthSquared() < SM_EFF_ZERO_SQ) { return SM_SUCCESS; }
    SER(sCircleNormal.Unitize());
    SmVector3d sLineVec = crABC * sCircleNormal;
    if (sLineVec.LengthSquared() < SM_EFF_ZERO_SQ) { return SM_SUCCESS; }
    sLineVec.Unitize();
    SmPoint3d sLinePoint = sPV[0] ;


    ULONG lNumTsect;
    double adTsectLineParam[2];
    SER(smgu_LineCoPlanarCircleIntersect(sLinePoint, sLineVec,
                                         sCircleCenter, sCircleNormal,
                                         dCircleRadius, SM_EFF_ZERO,
                                         lNumTsect,    adTsectLineParam));
    // If only have one or zero intersections we are done without
    // picking up extra guess
    if (lNumTsect!=2) { return SM_SUCCESS; }
    // Find the other intersection -- i.e. the one we don't have
    double dLineT = adTsectLineParam[0];
    if (smos_Fabs(adTsectLineParam[1]) > smos_Fabs(adTsectLineParam[0])) {
        dLineT = adTsectLineParam[1];
    }
    sFoundVector = dLineT * sLineVec;

    rbFoundGuess = TRUE;
    rdNewTGuess = dTsectT + sm_ConvertToParameterDelta(sPV[1],sFoundVector);

    if (!crInterval.ContainsValue(rdNewTGuess)) {
        // If the guess is outside of the domain we'll be able to pick it up using
        // another method.
        rbFoundGuess = FALSE;
        return SM_SUCCESS;
    }
    rdNewTGuess = crInterval.ClampValue(rdNewTGuess);

    return SM_SUCCESS;  // Found a guess - loaded it now we are done

} // end sm_FindAdditionalCpGuess
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 16115-16473: SmCCTangentIntENFO (class, constructor, Evaluate, StaticEvaluate)
class SmCCTangentIntENFO : public SmEvalNFunctionsObject
{
protected:
    const SmCurve & m_crCurve1;   // 1st Curve
    const SmCurve & m_crCurve2;   // 2nd Curve
    double m_dSquaredTolerance;   // currently not used
  //double m_dSign;               //  1.0 = find xSect where curve tangents run in same direction
                                  // -1.0 = find xSect where curve tangents run in opposite direction
 // SumOfAllErrors = TangAngleSquared + GapSizeSquared
public:
    SmCCTangentIntENFO(const SmCurve & crCurve1,
                       const SmCurve & crCurve2,
                       double dSquaredTolerance );
    virtual ~SmCCTangentIntENFO() {}
    virtual SmStatus Evaluate
      (const SmTArray<double> & crX,             // in : x of Ax=F
       SmTArray<double>       & rF,              // out: F of Ax=F function values,
       SmMatrix               * pOptJacobian,    // out: Partial derivatives of the functions.
       SmBoolean              & rbFoundAnswer);  // out: Not always used, when used
                                                 //      TRUE = converged (F members are within tolerance of 0.0
                                                 //      FALSE= Not Used or Not Converged

    static SmStatus StaticEvaluate
      (const SmCurve          & crCurve1,        // in :
       const SmCurve          & crCurve2,        // in :
       const SmTArray<double> & crX,             // in : x of Ax=F
       SmTArray<double>       & rF,              // out: F of Ax=F function values,
       SmMatrix               * pOptJacobian,    // out: Partial derivatives of the functions.
       SmBoolean              & rbFoundAnswer) ; // out: Just set to FALSE = Not Used
} ; // end class SmCCTangentIntENFO


/*******************************************************************//**
PURPOSE: NewtonRaphson Iteration Evaluate function Constructor
            for tangent Curve/Curve Intersections

NOTES:
***********************************************************************/
SmCCTangentIntENFO::SmCCTangentIntENFO
  (const SmCurve & crCurve1,            // in : 1st Curve
   const SmCurve & crCurve2,            // in : 2nd Curve
   double          dSquaredTolerance)   // in : currently not used
 //double          dSign                // obsolete.
 : m_crCurve1(crCurve1),
   m_crCurve2(crCurve2),
   m_dSquaredTolerance(dSquaredTolerance)
 //m_dSign(dSign)
{

} // end SmCCTangentIntENFO::SmCCTangentIntENFO constructor

/*******************************************************************//**
PURPOSE: Called once for each NR iteration to find
            tangent curve/curve intersections.
    Given X values,   Evaluate Func         = Func(X) and
                      Jacobian Matrix[i][j] = dFi/dxj.

    where Jacobi = (partial Func[i]/partial X[j])

NOTES:
  See description of objective functions in class declaration, above.

***********************************************************************/
SmStatus SmCCTangentIntENFO::Evaluate
  (const SmTArray<double> & crX,           // in : x of Ax=F
   SmTArray<double>       & rF,            // out: F of Ax=F function values,
   SmMatrix               * pOptJacobian,  // out: Partial derivatives of the functions.
   SmBoolean              & rbFoundAnswer) // out: currently always set to FALSE
{
  // pass the call along
  return( StaticEvaluate(m_crCurve1,
                         m_crCurve2,
                         crX,
                         rF,
                         pOptJacobian,
                         rbFoundAnswer));

} // end SmCCTangentIntENFO::Evaluate

/*******************************************************************//**
PURPOSE: Static Evaluate so that other functions can share these
            clever objective functions as needed

NOTES:
***********************************************************************/
SmStatus SmCCTangentIntENFO::StaticEvaluate
  (const SmCurve          & crCurve1,      // in : 1st curve being intersected
   const SmCurve          & crCurve2,      // in : 2nd curve being intersected
   const SmTArray<double> & crX,           // in : x of Ax=F
   SmTArray<double>       & rF,            // out: F of Ax=F function values,
   SmMatrix               * pOptJacobian,  // out: Partial derivatives of the functions.
   SmBoolean              & rbFoundAnswer) // out: currently always set to FALSE
{
  // check state
  SM_ASSERT(crX.GetSize() == 2);
  SM_ASSERT(rF.GetSize()  == 2);

  if (pOptJacobian)
    {
      SM_ASSERT(pOptJacobian->GetNumRows()    == 2);
      SM_ASSERT(pOptJacobian->GetNumColumns() == 2);
    }


  // init output
  rbFoundAnswer = FALSE;

#ifdef SM_DEBUG_CODE
static constexpr SmBoolean  bDebugMe = FALSE;
  if (bDebugMe)
    {
      crCurve1.Dump() ;
      crCurve2.Dump() ;

      SmEdge *pEdge1 = (SmEdge *)crCurve1.GetEdge() ;
      SmEdge *pEdge2 = (SmEdge *)crCurve2.GetEdge() ;
      SmBrep *pBrep1 = pEdge1 ? pEdge1->GetBrep() : NULL ;
      SmBrep *pBrep2 = pEdge2 ? pEdge2->GetBrep() : NULL ;

      smgfx_Erase() ;
      smgfx_SetLook(1,2,  0, 0, 1) ; if(pBrep1) pBrep1->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2,  0, 1, 0) ; if(pBrep2) pBrep2->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3,  0, 1, 1) ; crCurve1.Draw() ; sm_GraphicsLoop();
      smgfx_SetLook(2,3,  1, 1, 0) ; crCurve2.Draw() ; sm_GraphicsLoop();
      smgfx_SetLook(3,4,  0, 0, 0) ; if(pEdge1) pEdge1->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, .5,.5,.5) ; if(pEdge2) pEdge2->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,6,  0, 0, 1) ; crCurve1.DrawAt(crX[0],1); sm_GraphicsLoop();
      smgfx_SetLook(2,7,  0, 1, 0) ; crCurve2.DrawAt(crX[1],1); sm_GraphicsLoop();
      sm_GraphicsLoop();
    }
#endif // SM_DEBUG_CODE

  // See notes in this class' declaration for the objective functions.
  // Get Curve1Point, Curve2Point positions and tangents
  SmPoint3d sPVV1[3];
  SmPoint3d sPVV2[3];
  SER(crCurve1.Evaluate(crX[0],2,TRUE,sPVV1, TRUE));  // nonZeroTangent values
  SER(crCurve2.Evaluate(crX[1],2,TRUE,sPVV2, TRUE));  // nonZeroTangent values
  double sTanLenSq1 = sPVV1[1].LengthSquared();
  double sTanLenSq2 = sPVV2[1].LengthSquared();

  // if we have a zero length tangent then we must fail
  if(   sTanLenSq2 < SM_EFF_ZERO
     || sTanLenSq1 < SM_EFF_ZERO)
    { return SM_ERR; }

  SmVector3d sUFP = sPVV1[1];
  SER(sUFP.Unitize());
  SmVector3d sUGP = sPVV2[1];
  SER(sUGP.Unitize());
  SmVector3d sUFPP = sPVV1[1].UnitizedDerivative(sPVV1[2]);
  SmVector3d sUGPP = sPVV2[1].UnitizedDerivative(sPVV2[2]);
  SmVector3d sGapVec = sPVV1[0] - sPVV2[0];

  if (pOptJacobian)
    {
      (*pOptJacobian)[0][0] = sUFPP.Dot(sUGP);
      (*pOptJacobian)[0][1] = sUFP.Dot(sUGPP);
      (*pOptJacobian)[1][0] = 2.0 * sPVV1[1].Dot(sGapVec);
      (*pOptJacobian)[1][1] = - 2.0 * sPVV2[1].Dot(sGapVec);
    }

  // Compute function values

  double m_dSign = ( sUFP.Dot( sUGP ) > 0 ) ? 1.0 : -1.0;

  rF[0] = sUFP.Dot(sUGP) - m_dSign;
  rF[1] = sGapVec.Dot(sGapVec);

#ifdef SM_DEBUG_CODE

  if (bDebugMe)
    {
      if (pOptJacobian) pOptJacobian->Dump();
      smos_WriteBuffer(_T(" F --- \n"));
      rF.Dump();
    }
#endif // SM_DEBUG_CODE

  // all done
  return SM_SUCCESS;


  // Second algorithm: can give false convergence.
//
//  For two curves C(s) and D(t),
//
//  F(s,t) = (C' x D') dot ( ( C'' x D' ) + ( C' x D'' ) ) // modified cross prod
//  G(s,t) = (C - D) dot ( C' - D' )                       // modified difference
//
//  F() arose from a desire to zero the cross product of the tangent vectors.
//  However, the magnitude (squared, or whatever) of a vector is not a good
//  objective function because it just grazes zero, with a slope of zero at
//  solution, which destroys quadratic convergence right where you need it
//  most.  Instead, the cross product is dotted with some vector that is
//  generally in the same direction, but whose direction will not flip when the
//  cross product of tangents does; that dot product goes through zero nicely.
//
//  In practice, G(s,t) should actually use unit vectors, because differing
//  lengths of tangent vectors can overpower the desired difference.
//  Also, the unit tangents should be aligned in the same direction.
//  This adds a bit of complexity to the Jacobian, but not a big deal.
//
//  // Evaluate curves.  Need 3 derivs.
//  SmPoint3d sPVV1[4];
//  SmPoint3d sPVV2[4];
//
//  SER( crCurve1.Evaluate( crX[0], 3, TRUE, sPVV1 ));
//  SER( crCurve2.Evaluate( crX[1], 3, TRUE, sPVV2 ));
//
//  // See comments above.
//  SmVector3d sCross11 = sPVV1[1] * sPVV2[1];
//  SmVector3d sCross21 = sPVV1[2] * sPVV2[1];
//  SmVector3d sCross12 = sPVV1[1] * sPVV2[2];
//  SmVector3d sCross31 = sPVV1[3] * sPVV2[1];
//  SmVector3d sCross22 = sPVV1[2] * sPVV2[2];
//  SmVector3d sCross13 = sPVV1[1] * sPVV2[3];
//
//  SmVector3d sDiff00  = sPVV1[0] - sPVV2[0];
//  SmVector3d sDiff11  = sPVV1[1] - sPVV2[1];
//
//  SmVector3d sCtUnit = sPVV1[1];
//  double dMag = sCtUnit.Length();
//  if ( dMag > SM_EFF_ZERO )
//      sCtUnit /= dMag;
//
//  SmVector3d sDsUnit = sPVV2[1];
//  dMag = sDsUnit.Length();
//  if ( dMag > SM_EFF_ZERO )
//      sDsUnit /= dMag;
//
//  SmBoolean bFlip = ( sPVV1[1].Dot( sPVV2[1] ) ) < 0;
//
//  // Function values:
//  rF[0] = sCross11.Dot( sCross21 + sCross12 );
//  if ( bFlip )
//      rF[1] = sDiff00.Dot( sCtUnit + sDsUnit );
//  else
//      rF[1] = sDiff00.Dot( sCtUnit - sDsUnit );
//
//  // Derivatives:
//  if ( pOptJacobian )
//    {
//      (*pOptJacobian)[0][0] =   sCross21.Dot( sCross21 + sCross12 )
//                              + sCross11.Dot( sCross31 + sCross22 );
//      (*pOptJacobian)[0][1] =   sCross12.Dot( sCross21 + sCross12 )
//                              + sCross11.Dot( sCross22 + sCross13 );
//
//      SmVector3d sCtUnit_t = sPVV1[2] - sCtUnit * ( sCtUnit.Dot( sPVV1[2] ) );
//      SmVector3d sDsUnit_s = sPVV2[2] - sDsUnit * ( sDsUnit.Dot( sPVV2[2] ) );
//
//      if ( bFlip )
//      {
//          sDsUnit  *= -1;
//          sDsUnit_s *= -1;
//      }
//
//      (*pOptJacobian)[1][0] =   sPVV1[1].Dot( sCtUnit - sDsUnit )
//                              + sDiff00.Dot( sCtUnit_t );
//      (*pOptJacobian)[1][1] = - sPVV2[1].Dot( sCtUnit - sDsUnit )
//                              - sDiff00.Dot( sDsUnit_s );
//    }


  // Third algorithm: Also lost quadratic convergence.
// The idea here is this: just zero the gap, but since we need two functions,
// take dot products with two vectors.  This is essentially the same as the
// non-tangent case, where the two vectors are the two curves' tangent vectors.
// That has the added behavior of finding the closest approach, if the
// curves don't actually intersect.
// Here the two vectors are the common tangent direction, and the direction
// of the 2nd derivative of whichever curve has the larger 2nd derivative.
// That's just to get any basis -- any pair of non-tangent vectors.
// Note, if both curves' 2nd derivatives are zero, then we're trying to find
// the intersection of two parallel lines -- not much we can do here.
// (That does happen in practice; returning error doesn't hurt the algorithm.)
//
//  // Evaluate curves.  Need 3 derivs.
//  SmPoint3d sPVV1[4];
//  SmPoint3d sPVV2[4];
//
//  SER( crCurve1.Evaluate( crX[0], 3, TRUE, sPVV1 ));
//  SER( crCurve2.Evaluate( crX[1], 3, TRUE, sPVV2 ));
//
//  SmVector3d sGapVec( sPVV2[0] - sPVV1[0] );
//
//  // Get the unitized derivatives.
//  SmVector3d sC1_t_Unit, sC2_t_Unit;
//  ULONG lWhich2nd;
//
//  if ( sPVV1[2].LengthSquared() > sPVV2[2].LengthSquared() )
//  {
//      lWhich2nd = 1;
//      sC1_t_Unit = sPVV1[2];
//      SER( sC1_t_Unit.Unitize() );
//      sm_UnitizeTangent( sPVV2,  sGapVec, sC2_t_Unit );
//  }
//  else if ( sPVV2[2].LengthSquared() > SM_EFF_ZERO )
//  {
//      lWhich2nd = 2;
//      sm_UnitizeTangent( sPVV1, -sGapVec, sC1_t_Unit );
//      sC2_t_Unit = sPVV2[2];
//      SER( sC2_t_Unit.Unitize() );
//  }
//  else
//  {
//      // Intersecting two parallel lines: not going to happen.
//      return( SM_ERR ); // SER ?
//  }
//
//  // Function values:
//  rF[0] = sGapVec.Dot( sC1_t_Unit );
//  rF[1] = sGapVec.Dot( sC2_t_Unit );
//
//  // Derivatives:
//  if (pOptJacobian)
//    {
//      // Note: if we're not close enough to a solution, the second-order
//      // terms can dominate the first-order terms, and can change the
//      // direction.  The first-order terms always give the correct sign
//      // for the step, so the second-order term should not cause a sign
//      // change.  In fact, limiting the second-order term to half the
//      // size of the first term (if opposite size) works well.
//
//
//      // Deriv of the unitized C_t  ( C_t / || C_t || )
//      SmVector3d sC1_t_Unit_t, sC2_t_Unit_t;
//      if ( lWhich2nd == 1 )
//      {
//          sC1_t_Unit_t = sPVV1[2].UnitizedDerivative( sPVV1[3] );
//          sC2_t_Unit_t = sPVV2[1].UnitizedDerivative( sPVV2[2] );
//      }
//      else
//      {
//          sC1_t_Unit_t = sPVV1[1].UnitizedDerivative( sPVV1[2] );
//          sC2_t_Unit_t = sPVV2[2].UnitizedDerivative( sPVV2[3] );
//      }
//
//          double dTerm11 = -sPVV1[1].Dot( sC1_t_Unit   );
//          double dTerm12 =  sGapVec .Dot( sC1_t_Unit_t );
//          double dTerm21 =  sPVV2[1].Dot( sC2_t_Unit   );
//          double dTerm22 =  sGapVec .Dot( sC2_t_Unit_t );
//
//          if ( dTerm11*dTerm12 < 0 // note, dTerm11 might not be negative
//              && smos_Fabs( dTerm12 ) > smos_Fabs( dTerm11 )/2 ) {
//              dTerm12 = -dTerm11/2;
//          }
//          if ( dTerm21*dTerm22 < 0 // note, dTerm21 might not be positive
//              && smos_Fabs( dTerm22 < smos_Fabs( dTerm21 )/2 ) ) {
//              dTerm22 = -dTerm21/2;
//          }
//          (*pOptJacobian)[0][0] =  dTerm11 + dTerm12;
//          (*pOptJacobian)[0][1] =  sPVV2[1].Dot( sC1_t_Unit );
//          (*pOptJacobian)[1][0] = -sPVV1[1].Dot( sC2_t_Unit );
//          (*pOptJacobian)[1][1] =  dTerm21 + dTerm22;
//
//        } // end if pOptJacobian is not null.

} // end SmCCTangentIntENFO::StaticEvaluate
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 16497-16559: SmCCIntersectENFO (class, constructor)
class SmCCIntersectENFO : public SmEvalNFunctionsObject
{
  // SmCCIntersectENFO DOF count == 2
protected:
  const SmCurve             & m_crCurve1;        // 1st curve
  const SmCurve             & m_crCurve2;        // 2nd curve
  const SmExtentNd          * m_cpIntervals;     // NULL or sized:[2]
                                                 // bounds on problem parameters.
                                                 //  Set m_cpIntervals[i].SetUnbounded() for an unbounded interval whose prob params don't get clamped
  const SmTArray<SmBoolean> * m_cpPeriodicities; // NULL or sized:[2]
                                                 // ivl[i] == TRUE = Interval[i] is periodic and values get wrapped
                                                 // ivl[i] == FALSE= Interval[i] is NOT periodic and values get clamped
  double m_dSquaredTolerance;                    // Currently not used

 // SumOfAllErrors = 0 when difference vector zero, or is
 //                 perpendicular to both tangent vectors
public:
  SmCCIntersectENFO(const SmCurve    & crCurve1,            // in : 1st Curve
                    const SmCurve    & crCurve2,            // in : 2nd Curve
                    double             dSquaredTolerance);  // in : currently not used
 virtual ~SmCCIntersectENFO() { }

  virtual SmBoolean HasIntervals()                  { return TRUE ; }
  virtual SmStatus  SetIntervals
     (const SmExtentNd          *cpIntervals,       // in : NULL or sized:[2]
                                                    //      bounds on problem parameters.
                                                    //       Set m_cpIntervals[i].SetUnbounded() for an unbounded interval whose prob params don't get clamped
      const SmTArray<SmBoolean> *cpPeriodicities )  // in : NULL or sized:[2]
                                                    //      ivl[i] == TRUE = Interval[i] is periodic and values get wrapped
                                                    //      ivl[i] == FALSE= Interval[i] is NOT periodic and values get clamped
                                                    { m_cpIntervals     = cpIntervals;
                                                      m_cpPeriodicities = cpPeriodicities ;
                                                      return SM_SUCCESS ;
                                                    }
  virtual SmStatus Evaluate
    (const SmTArray<double> & crX,             // in : x of Ax=F
     SmTArray<double>       & rF,              // out: F of Ax=F function values,
     SmMatrix               * pOptJacobian,    // out: Partial derivatives of the functions.
     SmBoolean              & rbFoundAnswer);  // out: Always Set to FALSE

} ; // end class SmCCIntersectENFO

/*******************************************************************//**
PURPOSE: NewtonRaphson Iteration Evaluate function Constructor
            for Curve/Curve Intersections

USER NOTES ---
***********************************************************************/
SmCCIntersectENFO::SmCCIntersectENFO
  (const SmCurve    & crCurve1,            // in : 1st Curve
   const SmCurve    & crCurve2,            // in : 2nd Curve
   double             dSquaredTolerance)   // in : currently not used

 // SumOfAllErrors = 0 when Curve1/Curve2 gap is zero, or is
 // perpendicular to both tangent vectors.

 : m_crCurve1(crCurve1),
   m_crCurve2(crCurve2),
   m_cpIntervals    (NULL),
   m_cpPeriodicities(NULL),
   m_dSquaredTolerance(dSquaredTolerance)
{
} // end SmCCIntersectENFO::SmCCIntersectENFO constructor
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 16589-16842: SmCCIntersectENFO::Evaluate
SmStatus SmCCIntersectENFO::Evaluate
  (const SmTArray<double> & crX,           // in : x of Ax=F, x=[s1 s2]
   SmTArray<double>       & rF,            // out: F of Ax=F function values,
   SmMatrix               * pOptJacobian,  // out: Partial derivatives of the functions.
   SmBoolean              & rbFoundAnswer) // out: Always set to FALSE
{
  // check inputs
  SM_ASSERT(crX.GetSize() == 2);
  SM_ASSERT(rF.GetSize()  == 2);

  if (pOptJacobian)
    {
      SM_ASSERT(pOptJacobian->GetNumRows()    == 2);
      SM_ASSERT(pOptJacobian->GetNumColumns() == 2);
    }

  // init output
  rbFoundAnswer = FALSE;

  // Evaluate both curves; need two derivatives.
  SmPoint3d sPVV1[3], sPVV2[3];
  SER( m_crCurve1.Evaluate( crX[0], 2, TRUE, sPVV1, TRUE ));  // nonZeroTangent values
  SER( m_crCurve2.Evaluate( crX[1], 2, TRUE, sPVV2, TRUE ));  // nonZeroTangent values

  // get the gap - save the SolutionSize
  SmVector3d sGapVec = sPVV1[0] - sPVV2[0];
  m_dSolutionSize    = sGapVec.Length() ;
  m_dScaledZero      = SM_EFF_ZERO * (1.0 + smos_Max(sPVV1[0].GetMaxDimension(),
                                                     sPVV2[0].GetMaxDimension())) ;

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe1 = FALSE;
  if (bDebugMe1)
    {
      // m_crCurve1.Dump() ;
      // m_crCurve2.Dump() ;

      SmEdge *pEdge1 = (SmEdge *)m_crCurve1.GetEdge() ;
      SmEdge *pEdge2 = (SmEdge *)m_crCurve2.GetEdge() ;
      SmBrep *pBrep1 = pEdge1 ? pEdge1->GetBrep() : NULL ;
      SmBrep *pBrep2 = pEdge2 ? pEdge2->GetBrep() : NULL ;

      smgfx_Erase() ;
      smgfx_SetLook(1,2,  0, 0, 1) ; if(pBrep1) pBrep1->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2,  0, 1, 0) ; if(pBrep2) pBrep2->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3,  0, 1, 1) ; m_crCurve1.Draw() ; sm_GraphicsLoop();
      smgfx_SetLook(2,3,  1, 1, 0) ; m_crCurve2.Draw() ; sm_GraphicsLoop();
      smgfx_SetLook(3,4,  0, 0, 0) ; if(pEdge1) pEdge1->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, .5,.5,.5) ; if(pEdge2) pEdge2->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,6,  0, 0, 1) ; m_crCurve1.DrawAt(crX[0],1); sm_GraphicsLoop();
      smgfx_SetLook(2,7,  0, 1, 0) ; m_crCurve2.DrawAt(crX[1],1); sm_GraphicsLoop();
      smgfx_SetLook(2,3,  1,0,0) ;   sGapVec.Draw(&sPVV2[0]); sm_GraphicsLoop();
      sm_GraphicsLoop();
    }
#endif // SM_DEBUG_CODE

  // Check for nearby tangent intersection cases - this allows
  // large near-maximum conditions to 'fall off' the hill.
  // Note: the tangent ENFO works only for curves that are truly
  // tangent, not just nearly tangent.
  //
  // BUT: those functions don't behave any better than our default
  // functions: see notes there.  Moreover, if we switch, then the
  // iteration would have to be modified: different objective functions
  // could have larger values, so the iteration might think that
  // things are getting worse, and quit.  If you do something like
  // this, you'll have to check for that as well. [B98]
  //
  // BUT, having said that, it currently causes problems not to switch.
  // [B98 B115]
  //
  // Later, 7/18 [B567]: 89 and 115 now both work without switching, but one
  // prog_test fails [060221_Z].  Here's where it gets ugly.  We can make it
  // all work with a proper angle limit: Reg requires >= 0.00011,
  // B567 says <= 0.00362.  So for now we'll do that; should work on making
  // it work without switching at all.

  constexpr SmBoolean bSwitchToTanFcns = TRUE;

  // (Note, for debugging, temporarily make this static.)
  double dAngLimDeg = 0.001;   // See note above.
  double dDistLimSq = 0.01;
  SmBoolean bIsTangent = (    bSwitchToTanFcns
                           && sGapVec.LengthSquared() < dDistLimSq
                           && sPVV1[1].IsParallelTo( sPVV2[1], dAngLimDeg )
                         );

  // when near a tangent curve/curve intersection
  if( bIsTangent )
    {
      // switch to the TangentIntersection Solver Objective Functions
      SmCCTangentIntENFO::StaticEvaluate(m_crCurve1, m_crCurve2,
                                         crX, rF, pOptJacobian, rbFoundAnswer) ;
    }
  else // not near a tangency - use standard evaluation functions


// NEW SECTION NOT YET IMPLEMENTED: near-tangent case.
//    {
//      // In near tangent situations move the guess points to sample the gap function
//      //  gap function = minimum gap between a point on one curve and the other curve.
//      if(bIsNearTangent)
//        {
//          // for possibly two tries
//          SmBoolean bDone = FALSE ;
//          ULONG ii ;
//          for(ii=0;ii<2 && bDone == FALSE;ii++)
//            {
//              const SmCurve &crFromCurve = ii == 0 ? m_crCurve1 : m_crCurve2 ;
//              const SmCurve &crToCurve   = ii == 0 ? m_crCurve2 : m_crCurve1 ;
//              SmPoint3d     *pC1    = ii == 0 ? sPVV1      : sPVV2 ;
//              SmPoint3d     *pC2    = ii == 0 ? sPVV2      : sPVV1 ;
//
//              // try projecting Curve 1 guess point down to curve 2 using 2nd order curve approx
//              // let C2(dt) = C2 + C2' * dt + 1/2 C2" * dt**2 + H.O.T.
//              //  find intersection of C2 with plane perp to C1 tangent at C1
//              //  0 = Dot(C1', C2(dt)-C1)
//              //  0 = 1/2 * Dot(C1',C2") * dt**2 + Dot(C1', C2') * dt + Dot(C1', C2-C1)
//              ULONG lNumSolutions ;
//              double adCoef[3], adSolutions[2] ;
//              adCoef[0] = pC1[1].Dot(pC2[0] - pC1[0]) ;   // Dot(C1', C2-C1) ;
//              adCoef[1] = pC1[1].Dot(pC2[1]) ;            // Dot(C1', C2') ;
//              adCoef[2] = pC1[1].Dot(pC2[2]) / 2.0 ;      // 1/2 * Dot(C1',C2") ;
//
//              // solve Ax**2 + Bx + C = 0 where C = a[0], B = a[1], A = a[2]
//              SmPolynomial::SolveQuadraticEqn(adCoef, SM_EFF_ZERO, lNumSolutions, adSolutions) ;
//
//              switch(lNumSolutions)
//                {
//                  case 0 : // this is probably an error case - the two nearly tangent
//                           // curves should be able to project to one another.
//                           WARN(_T("Unable to project point between nearly parallel curves")) ;
//
//                           // if ii == 0 try jumping from curve 2 to curve 1
//                           //    ii == 1 this isn't working - carry on
//                           break ;
//                  case 2 : // pick the solution closest to the current C1 point
//                           { double dT1 = crX[(ii+1)%2] + adSolutions[0] ;
//                             double dT2 = crX[(ii+1)%2] + adSolutions[1] ;
//                             SmPoint3d dP1, dP2 ;
//                             m_crCurve2.EvaluatePoint( dT1, dP1 );
//                             m_crCurve2.EvaluatePoint( dT2, dP2 );
//                             double dGap1 = (dP1 - pC1[0]).Length() ;
//                             double dGap2 = (dP2 - pC1[0]).Length() ;
//                             if(dGap1 > dGap2) { double dTmp = adSolutions[0] ;
//                                                 adSolutions[0] = adSolutions[1] ;
//                                                 adSolutions[1] = adSolutions[0] ;
//                                               }
//                           }
//                           // no break here - carry on to case 1 code on purpose
//                  case 1 : { double dT      = crX[(ii+1)%2] + adSolutions[0] ;
//                             double dPeriod =   (*m_cpPeriodicities)[(ii+1)%2]
//                                              ? (*m_cpIntervals)[(ii+1)%2].GetLength()
//                                              : 0.0 ;
//                             // when out of bounds
//                             if(!(*m_cpIntervals)[(ii+1)%2].ContainsPeriodicValue(dT, dPeriod))
//                               {
//                                 // if ii == 0 try jumping from curve 2 to curve 1
//                                 //    ii == 1 this isn't working - carry on
//                                 break ;
//                               }
//
//                             // arrive here when gap function guess is in bounds
//                             // save the updated param value in the crX array
//                             // update the sPVV1 or pVV2 values and leave this loop.
//                             // note: this is a cheat.  Evaluate functions are not supposed to
//                             //       modify param values.  However, we are trying to
//                             //       force NewtonRaphson to ignore gap-function maximums and
//                             //       only seek gapFunction minimums.  To do that we need
//                             //       to sample gapFunctions when working with nearly tangent curves.
//                             //       And this cheat moves the param value on one of the curves
//                             //       to approximate the gap function.
//                             ((SmTArray<double> & )crX)[(ii+1)%2] = dT ;
//                               crToCurve.Evaluate( dT, 2, TRUE, pC2 );
//                               sGapVec = sPVV1[0] - sPVV2[0];
//                               m_dSolutionSize    = sGapVec.Length() ;
//                               m_dScaledZero      = SM_EFF_ZERO * (1.0 + smos_Max(sPVV1[0].GetMaxDimension(),
//                                                                                  sPVV2[0].GetMaxDimension())) ;
//                               bDone = TRUE ;
//                           }
//                           break ;
//                  default: break ;
//               } // end switch on number of quadratic solutions
//           } // end iter both curves looking for a gapFunction sample
//        } // end near tangent case
//    } // end not tangent case
// end NEW SECTION NOT YET IMPLEMENTED: near-tangent case.

  // else: the usual case, not tangent.
    {
      // Get the unitized derivatives.
      SmVector3d sC1_t_Unit( sPVV1[1] );
      // sC1_t_Unit.Unitize();  // often get zero-vec errors.
      sm_UnitizeTangent( sPVV1, -sGapVec, sC1_t_Unit );

      SmVector3d sC2_t_Unit( sPVV2[1] );
      // sC2_t_Unit.Unitize();
      sm_UnitizeTangent( sPVV2,  sGapVec, sC2_t_Unit );

      // Function values:
      rF[0] = sGapVec.Dot( sC1_t_Unit );
      rF[1] = sGapVec.Dot( sC2_t_Unit );

      // Derivatives:
      if (pOptJacobian)
        {
          // Note: if we're not close enough to a solution, the second-order
          // terms can dominate the first-order terms, and can change the
          // direction.  The first-order terms always give the correct sign
          // for the step, so the second-order term should not cause a sign
          // change.  In fact, limiting the second-order term to half the
          // size of the first term (if opposite size) works well.
          // Note, the first order terms are always positive.

          // Deriv of the unitized C_t  ( C_t / || C_t || )
          SmVector3d sC1_t_Unit_t = sPVV1[1].UnitizedDerivative( sPVV1[2] );
          SmVector3d sC2_t_Unit_t = sPVV2[1].UnitizedDerivative( sPVV2[2] );

          double dTerm11 =  sPVV1[1].Dot( sC1_t_Unit   );
          double dTerm12 =  sGapVec .Dot( sC1_t_Unit_t );
          double dTerm21 = -sPVV2[1].Dot( sC2_t_Unit   );
          double dTerm22 =  sGapVec .Dot( sC2_t_Unit_t );

          if ( dTerm12 < -dTerm11/2 ) { // note, dTerm11 > 0 always
              dTerm12 = -dTerm11/2;
          }
          if ( dTerm22 > -dTerm21/2 ) { // note, dTerm21 < 0 always
              dTerm22 = -dTerm21/2;
          }
          (*pOptJacobian)[0][0] =  dTerm11 + dTerm12;
          (*pOptJacobian)[0][1] = -sPVV2[1].Dot( sC1_t_Unit );
          (*pOptJacobian)[1][0] =  sPVV1[1].Dot( sC2_t_Unit );
          (*pOptJacobian)[1][1] =  dTerm21 + dTerm22;
        }
    } // end not near tangent branch

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe2 = FALSE ;
      if (bDebugMe2)
        {
          TCHAR sBuff[SM_TBLOCK_SIZE];
          smos_sprintf(sBuff, _T(" crX[ %16.16lf, %16.16lf]\n"), crX[0], crX[1]) ;
          smos_WriteBuffer(sBuff) ;
          if (pOptJacobian) pOptJacobian->Dump();
          smos_WriteBuffer(_T(" F --- \n"));
          rF.Dump();
          smos_sprintf(sBuff, _T(" Gap = %16.16lf\n"), m_dSolutionSize );
          smos_WriteBuffer(sBuff);
        }
#endif // SM_DEBUG_CODE

  return SM_SUCCESS;

} // end SmCCIntersectENFO::Evaluate
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 17130-17163: SmFindCCNormalENFO (class)
class SmFindCCNormalENFO : public SmEvalNFunctionsObject
{
protected:
    const SmCurve &       m_crCurve1;          // this curve
    const SmCurve &       m_crCurve2;          // other curve
    double                m_dSquaredTolerance;
    double                m_dSquaredAtDistance;
    SmSolverOperationType m_eSolverOperation;
    const SmVector3d *    m_cpOptVectors;
    ULONG                 m_lCount;
    ULONG                 m_bStepBack;
public:
    SmFindCCNormalENFO(const SmCurve        & crCurve1,
                       const SmCurve        & crCurve2,
                       double                 dSquaredTolerance,
                       double                 dSquaredAtDistance,
                       SmSolverOperationType  eSolverOperation,
                       const SmVector3d     * cpOptVectors=NULL)
                     : m_crCurve1(crCurve1),
                       m_crCurve2(crCurve2),
                       m_dSquaredTolerance(dSquaredTolerance),
                       m_dSquaredAtDistance(dSquaredAtDistance),
                       m_eSolverOperation(eSolverOperation),
                       m_cpOptVectors(cpOptVectors),
                       m_lCount(0),
                       m_bStepBack(FALSE) { }
    virtual ~SmFindCCNormalENFO() { }
    virtual SmStatus Evaluate(const SmTArray<double> & crX,             // in : x of Ax=F
                              SmTArray<double>       & rF,              // out: F of Ax=F function values,
                              SmMatrix               * pOptJacobian,    // out: Partial derivatives of the functions.
                              SmBoolean              & rbFoundAnswer);  // out: Not always used, when used
                                                                        //      TRUE = converged (F members are within tolerance of 0.0
                                                                        //      FALSE= Not Used or Not Converged
} ; // end class SmFindCCNormalENFO
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 17170-17863: SmFindCCNormalENFO::Evaluate
SmStatus SmFindCCNormalENFO::Evaluate
  (const SmTArray<double> & crX,           // in : x of Ax=F
   SmTArray<double>       & rF,            // out: F of Ax=F function values,
   SmMatrix               * pOptJacobian,  // out: Partial derivatives of the functions.
   SmBoolean              & rbFoundAnswer) // out: TRUE =
                                           //      FALSE=
{
  // check inputs
  SM_ASSERT(crX.GetSize() == 2);
  SM_ASSERT(rF.GetSize()  == 2);
  SM_ASSERT(pOptJacobian == NULL || pOptJacobian->GetNumRows() == 2);
  SM_ASSERT(pOptJacobian == NULL || pOptJacobian->GetNumColumns() == 2);

  // count iterations
  if(pOptJacobian) m_lCount ++;

  // init output
  rbFoundAnswer = FALSE;

  // curve evals
  SmPoint3d sPVV1[3], sPVV2[3];
  SER(m_crCurve1.Evaluate(crX[0],2,TRUE,sPVV1,TRUE));    // this curve  nonZeroTangent values
  SER(m_crCurve2.Evaluate(crX[1],2,TRUE,sPVV2,TRUE));    // other curve nonZeroTangent values

  // curve/curve distance
  SmVector3d sVecFMinusG = sPVV1[0] - sPVV2[0];

#ifdef SM_DEBUG_CODE
static constexpr SmBoolean  bDebugMe = FALSE;
SmBoolean bDebugMe2 = FALSE ;
  if (bDebugMe)
  {
    if(bDebugMe2)
      {
        smgfx_Erase() ;
        smgfx_SetLook(1,2, 0,0,0) ; m_crCurve1.Draw(NULL,TRUE) ; sm_GraphicsLoop() ;
        smgfx_SetLook(1,2, 0,0,0) ; m_crCurve2.Draw(NULL,TRUE) ; sm_GraphicsLoop() ;
      }
    sm_GraphicsLoop();
    smgfx_SetLook(1,6, 0,1,0); m_crCurve1.DrawAt(crX[0],2); sm_GraphicsLoop();
    smgfx_SetLook(1,6, 0,0,1); m_crCurve2.DrawAt(crX[1],2); sm_GraphicsLoop();
    smgfx_SetLook(1,2, 1,0,0); sVecFMinusG.Draw(&sPVV2[0]); sm_GraphicsLoop();
    sm_GraphicsLoop();
  }
#endif //SM_DEBUG_CODE

  // failure - zero length tangents (GWC: Curve::Evaluate was modified to eliminate zero length tangents)
  double sTanLenSq1 = sPVV1[1].LengthSquared();
  double sTanLenSq2 = sPVV2[1].LengthSquared();

  if(   sTanLenSq1 < SM_EFF_ZERO_SQ
     || sTanLenSq2 < SM_EFF_ZERO_SQ)
    { return SM_ERR; }

  // Try a little trick here if the distance between them
  // gets very small.  Try to translate a curve by its
  // curvature vector in the appropriate direction.
  if (m_lCount == 1 || m_bStepBack)
    {
      if(   sVecFMinusG.LengthSquared() < m_dSquaredTolerance
         && m_eSolverOperation != SM_SO_AT_DISTANCE
         && m_eSolverOperation != SM_SO_PROJECTED_TANGENCY)
        {
          m_bStepBack = TRUE;
          SmVector3d sTrans(0,0,0);
          SmVector3d sPV[3];

          // curve 1 curvature
          SER(m_crCurve1.EvaluateGeometric(crX[0],2,TRUE,sPV));
          if (sPV[2].LengthSquared() > SM_EFF_ZERO_SQ)
            {
              sPV[2].Unitize();
              sTrans = sPV[2];
            }
          else
            {
              // curve 2 curvature
              SER(m_crCurve2.EvaluateGeometric(crX[1],2,TRUE,sPV));
              if (sPV[2].LengthSquared() > SM_EFF_ZERO_SQ)
                {
                  sPV[2].Unitize();
                  sTrans = sPV[2];
                }
            }

          //
          if (sTrans.Dot(sVecFMinusG) >= 0.0)
            {
              sVecFMinusG = sPVV1[0] + sTrans - sPVV2[0];
            }
          else
            {
              sVecFMinusG = sPVV1[0] - sTrans - sPVV2[0];
            }
        } // end sVecFMinusG is small and operation not SM_SO_AT_DISTANCE or SM_SO_PROJECTED_TANGENCY check
    } // end m_lCount == 1 || m_bStepBack check

  switch (m_eSolverOperation)
    {
      case SM_SO_PROJECTED_TANGENCY:
        {
          if (m_cpOptVectors == NULL) SER(SM_ERR);
          // Here we are doing projected tangency function.
          // Basically we have to project all of the vectors to be
          // perpendicular to the vector given as the plane normal.

          // Set up Jacobian matrix for the following two functions
          //
          // fun[0] = (f' x v) . (f' x v)
          // fun[1] = (g' x v) . (g' x v)
          //
          //  | 2 F0 F0/df    2 F0 F0/dg |
          //  | 2 F1 F1/df    2 F1 F1/dg |
          //
          // Where F0 is (f' x v) and F1 is (g' x v)

          // Where f' is the projected tangent of the first curve and
          // where g' is the projected tangent of the second curve and
          // where f'' is the projected second derivative of the first curve
          // where g'' is the projected second derivative of the second curve
          // where v is the projected vector (f-g).

          SmVector3d sN   = m_cpOptVectors[0];
          SmVector3d sFp  = sN * sPVV1[1] * sN;
          SmVector3d sFpp = sN * sPVV1[2] * sN;
          SmVector3d sGp  = sN * sPVV2[1] * sN;
          SmVector3d sGpp = sN * sPVV2[2] * sN;
          SmVector3d sV   = sN * sVecFMinusG * sN;

          // If projection of vector between curves is too small just error out
          if (sV.LengthSquared() < m_dSquaredTolerance)
            {
              rbFoundAnswer = FALSE;
              return SM_ERR;
            }

          SmVector3d sZeroVec(0,0,0);
          SmVector3d sF0   = sFp * sV;
          SmVector3d sF1   = sGp * sV;
          SmVector3d sF0df = sFp.DerivativeOfCrossProduct(sV, sFpp, sFp);
          SmVector3d sF0dg = sFp.DerivativeOfCrossProduct(sV, sZeroVec, -sGp);
          SmVector3d sF1df = sGp.DerivativeOfCrossProduct(sV, sZeroVec, sFp);
          SmVector3d sF1dg = sGp.DerivativeOfCrossProduct(sV, sGpp, -sGp);

          if (pOptJacobian)
            {
              (*pOptJacobian)[0][0] = 2.0 * sF0.Dot(sF0df);
              (*pOptJacobian)[0][1] = 2.0 * sF0.Dot(sF0dg);
              (*pOptJacobian)[1][0] = 2.0 * sF1.Dot(sF1df);
              (*pOptJacobian)[1][1] = 2.0 * sF1.Dot(sF1dg);
            }

          // Compute function values
          // For this one we take dot product of tangent and vector to point on
          // other curve and try to minimize that.
          // fun[0] = (f' x v) . (f' x v)
          // fun[1] = (g' x v) . (g' x v)
          rF[0] = sF0.Dot(sF0);
          rF[1] = sF1.Dot(sF1);

        } // end SM_SO_PROJECTED_TANGENCY
        break;

      case SM_SO_PROJECTED_MINIMIZE:
      case SM_SO_PROJECTED_MAXIMIZE:
        {
          if (m_cpOptVectors == NULL) SER(SM_ERR);
          // Here we are doing projected minimize/maximize function.
          // Basically we have to project all of the vectors to be
          // perpendicular to the vector given as the plane normal.

          // Set up Jacobian matrix for the following two functions
          //  GWC: was fun[0] = f'.v
          //           fun[1] = g'.v
          //       but those functions go to zero when
          //         0. v goes to zero                                (desired)
          //         1. angle(f',v) and angle(g'.v) go to 90 degrees  (desired)
          //         2. when f' or g' go to zero                      (undesirable)
          //       to prevent NR running off in directions that take
          //          f' and g' to zero we modify the functionals to
          //
          // fun[0] = |f'| . v    which goes to zero for cases 0 and 1
          // fun[1] = |g'| . v    but not 2.
          //
          //   fun[0] and fun[1] go to infinity when ||f'|| or ||g'|| go
          //    to zero.  The function SmCurve::Evaluate has been modified
          //    so that it always returns a nonZero tangent value as long
          //    as the 2nd derivative is not zero when the tangent is zero.
          //
          //
          //  |  (f''.v + f'.f')*sqrt(f'.f')/f'.f')     -f'.g'/sqrt(f'.f')  |
          //  | -(f'.v)*(f''.f')/sqrt(f'.f')/(f'.f')                        |
          //  |                                                             |
          //  |  g'.f'/sqrt(g'.g')      (g''.v - g'.g')*sqrt(g'.g')/g'.g')  |
          //  |                        -(g'.v)*(g''.g')/sqrt(g'.g')/(g'.g') |
          //
          // where v   is the projected vector (f-g).
          // Where f'  is the projected tangent of the first curve and
          // where g'  is the projected tangent of the second curve and
          // where f'' is the projected second derivative of the first curve
          // where g'' is the projected second derivative of the second curve

          SmVector3d sN   = m_cpOptVectors[0];
          SmVector3d sV   = sN * sVecFMinusG * sN;
          SmVector3d sFp  = sN * sPVV1[1] * sN;
          SmVector3d sFpp = sN * sPVV1[2] * sN;
          SmVector3d sGp  = sN * sPVV2[1] * sN;
          SmVector3d sGpp = sN * sPVV2[2] * sN;

          double dVDotV    = sV.Dot(sV) ;
          double dVDotFp   = sV.Dot(sFp) ;
          double dVDotFpp  = sV.Dot(sFpp) ;
          double dVDotGp   = sV.Dot(sGp) ;
          double dVDotGpp  = sV.Dot(sGpp) ;
          double dFpDotFp  = sFp.Dot(sFp) ;
          double dFpDotFpp = sFp.Dot(sFpp) ;
          double dFpDotGp  = sFp.Dot(sGp) ;
          double dGpDotGp  = sGp.Dot(sGp) ;
          double dGpDotGpp = sGp.Dot(sGpp) ;
          double dFpLen    = smos_Sqrt(dFpDotFp) ;
          double dGpLen    = smos_Sqrt(dGpDotGp) ;

          if (pOptJacobian)
            {
              // projected tangent might be zero
              if(dFpLen > SM_EFF_ZERO)
                { (*pOptJacobian)[0][0] =  ( (dVDotFpp + dFpDotFp )*dFpLen
                                            - (dVDotFp  * dFpDotFpp)/dFpLen)
                                          / dFpDotFp ;
                  (*pOptJacobian)[0][1] =  - dFpDotGp/dFpLen ;
                }
              else
                { (*pOptJacobian)[0][0] = 0.0 ;
                  (*pOptJacobian)[0][1] = 0.0 ;
                }

              // projected tangent might be zero
              if(dGpLen > SM_EFF_ZERO)
                { (*pOptJacobian)[1][0] =     dFpDotGp/dGpLen ;
                  (*pOptJacobian)[1][1] =  ( (dVDotGpp - dGpDotGp )*dGpLen
                                            - (dVDotGp  * dGpDotGpp)/dGpLen)
                                          / dGpDotGp ;
                }
              else
                { (*pOptJacobian)[1][0] = 0.0 ;
                  (*pOptJacobian)[1][1] = 0.0 ;
                }
            }

          //      if (pOptJacobian)
          //        {
          //          (*pOptJacobian)[0][0] =   sFpp.Dot(sV) + sFp.Dot(sFp);
          //          (*pOptJacobian)[0][1] = - sFp.Dot(sGp);
          //          (*pOptJacobian)[1][0] = - (*pOptJacobian)[0][1];
          //          (*pOptJacobian)[1][1] =   sGpp.Dot(sV) - sGp.Dot(sGp);
          //        }

          // Compute function values
          // For this one we take dot product of tangent and vector to point on
          // other curve and try to minimize that.
          // fun[0] = f'(v)
          // fun[1] = g'(v)
          rF[0] = (dFpLen > SM_EFF_ZERO) ? sFp.Dot(sV)/dFpLen : 0.0 ;
          rF[1] = (dGpLen > SM_EFF_ZERO) ? sGp.Dot(sV)/dGpLen : 0.0 ;

          if (dVDotV < m_dSquaredTolerance)
            {
              rbFoundAnswer = TRUE;
              break;
            }

          double dProjDistSquared1 = (rF[0]*rF[0]) / dVDotV;
          double dProjDistSquared2 = (rF[1]*rF[1]) / dVDotV;

          if(   dProjDistSquared1 < m_dSquaredTolerance
             && dProjDistSquared2 < m_dSquaredTolerance)
            {
              rbFoundAnswer = TRUE;
              break;
            }
        } // end SM_SO_PROJECTED_MINIMIZE or SM_SO_PROJECTED_MAXIMIZE
        break;

      case SM_SO_SIGNED_DIRECTED_MINIMIZE:
      case SM_SO_DIRECTED_MINIMIZE:
      case SM_SO_DIRECTED_MAXIMIZE:
        {
          if (m_cpOptVectors == NULL) SER(SM_ERR);
          // Here we are doing 2D minimize/maximize function along a given vector
          // relative to a given reference plane normal between the two curves.
          // Basically we are minimizing the dot product between a perpendicular
          // vector and making the projected curve tangents parallel.

          // Set up Jacobian matrix for the following two functions
          //
          // fun[0] = (f' x N) . g'
          // fun[1] = |(f-g) x V| = ((f-g) x V) . ((f-g) x V) = d^2
          //
          // Where f is sPVV1 and g is sPVV2 and V is a perpendicular vector to the
          // direction used in the minimization and N is the reference plane normal.
          //
          // Jacobian is as follows
          //
          // |  (f' x N)'.g'   (f' x N) . g''    |
          // |  2d.dd/dt       2d.dd/ts          |
          //
          //
          // d = ((f-g) . V) dd/dt = df/dt . V
          //                 dd/ds = -dg/dt . V
          //

          SmVector3d sVecFMinusGLocal = sPVV1[0] - sPVV2[0];
          SmVector3d sV          = m_cpOptVectors[1] * m_cpOptVectors[0];
          SmVector3d sFpxN       = sPVV1[1] * m_cpOptVectors[0];

          if (pOptJacobian)
            {
              SmVector3d sZeroVec(0,0,0);
              SmVector3d sFpxNp = sPVV1[1].DerivativeOfCrossProduct(m_cpOptVectors[0],sPVV1[2],sZeroVec);

              (*pOptJacobian)[0][0] =  sFpxNp.Dot(sPVV2[1]);
              (*pOptJacobian)[0][1] =  sFpxN.Dot(sPVV2[2]);
              (*pOptJacobian)[1][0] =  sPVV1[1].Dot(sV);
              (*pOptJacobian)[1][1] = -sPVV2[1].Dot(sV);
            }

          rF[0] = sFpxN.Dot(sPVV2[1]);
          rF[1] = sVecFMinusGLocal.Dot(sV);
        } // end SM_SO_SIGNED_DIRECTED_MINIMIZE or SM_SO_DIRECTED_MINIMIZE or SM_SO_DIRECTED_MAXIMIZE
        break;

      case SM_SO_AT_DISTANCE:
          {
            // Here we are doing distance at function
            // Distance at for two curves is defined when
            // the distance between the points is the same and
            // the tangent vectors have the same angle with the
            // vector between the points on the curve.
            // Set up Jacobian matrix for the following two functions
            //           g(v)'
            // fun[0] = ------- . (g(v)-f(u))
            //          |g(v)'|
            //
            // fun[1] = (g(v)-f(u))^2 - c^2
            //
            // Where f is in sPVV1 as [f  f' f"] and
            //       g is in sPVV2 as [g  g' g"] and
            //       c^2 is the target distance squared.
            //
            // Jacobian is as follows
            //
            // |  - ugp.f'   (ugp'.(g-f) + ugp.g') |
            // |  - 2f'(g-f)    2g'(g-f)           |
            //
            // Where ufp is Unitized F Prime = f'/|f'|
            // Where ugp is Unitized G Prime = g'/|g'|

            SmVector3d sVecGMinusF = sPVV2[0] - sPVV1[0];

            SmVector3d sUFP = sPVV1[1];
            SmVector3d sUGP = sPVV2[1];
            SER(sUFP.Unitize());
            SER(sUGP.Unitize());
            SmVector3d sUFPP = sPVV1[1].UnitizedDerivative(sPVV1[2]);
            SmVector3d sUGPP = sPVV2[1].UnitizedDerivative(sPVV2[2]);

            if (pOptJacobian)
              {
                (*pOptJacobian)[0][0] = - sUGP.Dot(sPVV1[1]);
                (*pOptJacobian)[0][1] =   sUGPP.Dot(sVecGMinusF)
                                        + sUGP.Dot(sPVV2[1]) ;
                (*pOptJacobian)[1][0] = - 2.0 * sPVV1[1].Dot(sVecGMinusF);
                (*pOptJacobian)[1][1] =   2.0 * sPVV2[1].Dot(sVecGMinusF);
              }

            // function values
            rF[0] = sUGP.Dot(sVecGMinusF) ;
            rF[1] = sVecGMinusF.Dot(sVecGMinusF) - m_dSquaredAtDistance;

          } // end SM_SO_AT_DISTANCE
        break;

  // obsolete
  //          case SM_SO_AT_DISTANCE:
  //              {
  //                // Here we are doing distance at function
  //                // Distance at for two curves is defined when
  //                // the distance between the points is the same and
  //                // the tangent vectors have the same angle with the
  //                // vector between the points on the curve.
  //                // Set up Jacobian matrix for the following two functions
  //                //           f'                  g'
  //                // fun[0] = ----  . (f-g)   -   ---- . (g-f)
  //                //          |f'|                |g'|
  //                //
  //                // fun[1] = (f-g)^2 - c^2
  //                //
  //                // Where f is sPVV1 and s is sPVV2 and c^2 is the target distance
  //                // squared.
  //                //
  //                // Jacobian is as follows
  //                //
  //                // |  ufp'.(f-g) + ufp.f' + ugp.f'   -(ufp.g' + ugp'.(g-f) + ugp.g') |
  //                // |  2f'(f-g)                       -2g'(f-g)                       |
  //                //
  //                // Where ufp is Unitized F Prime = f'/|f'|
  //                // Where usp is Unitized G Prime = g'/|g'|
  //                //
  //                // When the derivative directions are opposite here is the equations
  //                //
  //                //           f'                  g'
  //                // fun[0] = ----  . (f-g)   +   ---- . (g-f)
  //                //          |f'|                |g'|
  //                //
  //                // |  ufp'.(f-g) + ufp.f' - ugp.f'   -(ufp.g' - ugp'.(g-f) - ugp.g') |
  //                // |  2f'(f-g)                       -2g'(f-g)                       |
  //
  //                double     dSign       = 1.0;
  //                SmVector3d sVecGMinusF = sPVV2[0] - sPVV1[0];
  //                if (sPVV1[1].Dot(sVecFMinusG) * sPVV2[1].Dot(sVecGMinusF) < 0.0)
  //                  {
  //                    dSign = -1.0;   // Here we need to negate our second term
  //                                    // fun[0] which essentially says that the first derivatives
  //                                    // do not agree relative to the vector between the points.
  //                  }
  //
  //                SmVector3d sUFP = sPVV1[1];
  //                SmVector3d sUGP = sPVV2[1];
  //                SER(sUFP.Unitize());
  //                SER(sUGP.Unitize());
  //                SmVector3d sUFPP = sPVV1[1].UnitizedDerivative(sPVV1[2]);
  //                SmVector3d sUGPP = sPVV2[1].UnitizedDerivative(sPVV2[2]);
  //
  //                if (pOptJacobian)
  //                  {
  //                    (*pOptJacobian)[0][0] =   sUFPP.Dot(sVecFMinusG)
  //                                            + sUFP.Dot(sPVV1[1])
  //                                            + dSign * sUGP.Dot(sPVV1[1]);
  //                    (*pOptJacobian)[0][1] = - (sUFP.Dot(sPVV2[1])
  //                                            + dSign * sUFPP.Dot(sVecGMinusF)
  //                                            + dSign * sUGP.Dot(sPVV2[1]));
  //                    (*pOptJacobian)[1][0] =   2.0 * sPVV1[1].Dot(sVecFMinusG);
  //                    (*pOptJacobian)[1][1] = - 2.0 * sPVV2[1].Dot(sVecFMinusG);
  //                  }
  //
  //                // Now compute the function values
  //                //double dAng1 = sUFP.Dot(sVecFMinusG);
  //                //double dAng2 = dSign*sUGP.Dot(sVecGMinusF);
  //                //double dDist = sVecFMinusG.Dot(sVecFMinusG);
  //
  //                rF[0] = sUFP.Dot(sVecFMinusG) - dSign*sUGP.Dot(sVecGMinusF);
  //                rF[1] = sVecFMinusG.Dot(sVecFMinusG) - m_dSquaredAtDistance;
  //
  //                // There is a possibility that we will get stuck in an inverse condition
  //                // where curves are parallel ('Z' shaped).  If this happens then add
  //                // 1.0 to the function to kick it out.  It should not converge to this
  //                // position but if the starting point is there then it may stay there
  //                // because everything goes to zero.
  //
  //                // Test if the vectors are in same line using cross product
  //                SmVector3d sCross = sUFP * sUGP;
  //                if ( sCross.LengthSquared() < SM_EFF_ZERO )
  //                  {
  //                    rF[0] += 1.0;
  //                    rF[1] += 1.0;
  //                  }
  //              } // end SM_SO_AT_DISTANCE
  //            break;
  // end obsolete

      case SM_SO_SIGNED_PIVOT_MINIMIZE:
        {
          // Here we are doing projected angle-minimize between two curves
          // relative to a given reference plane normal and a pivot point.
          // i.e. Find min angle A such that when curve1 is rotated about pivot pt
          // by A, it just touches curve2 when viewed along projection vector.
          // Let P be pivot pt, then we are minimizing the following two functions:
          //
          // fun[0] = (f-P) . (f-P) - (g-P) .(g-p)
          //                   f'              g'
          // fun[1] = (f-P) . ----  - (g-P) . ----
          //                  |f'|            |g'|
          // fun[0] - makes the distance between the pivot point and the points on
          //          the curve equal - they have the same radii.
          // fun[1] - makes the angles between the tangent vector and the vector
          //          to the pivot point equal for each curve.
          //
          // Note: (1) All the vectors should be projected along reference plane normal,
          //       (2) If two tangent vectors (f' & g') are pointing 'opposite' ways, we'll
          //           change the equation fun[1] to be a + instead of a -.
          //
          SmPoint3d  sPivot = m_cpOptVectors[1]; //input pivot point
          SmVector3d sN     = m_cpOptVectors[0];
          SmVector3d sFp    = sN * sPVV1[1] * sN;
          SmVector3d sFpp   = sN * sPVV1[2] * sN;
          SmVector3d sGp    = sN * sPVV2[1] * sN;
          SmVector3d sGpp   = sN * sPVV2[2] * sN;
          SmVector3d sUFp   = sFp;
          SmVector3d sUGp   = sGp;
          sUFp.Unitize();
          sUGp.Unitize();
          SmVector3d sPivF  = sN * (sPVV1[0] - sPivot) * sN;
          SmVector3d sPivG  = sN * (sPVV2[0] - sPivot) * sN;
          double dSign = 1.0;
          if (sFp.Dot(sPivF)*sGp.Dot(sPivG) < 0.0)
            {
              // Flip the tangent vector so that two tangents point to same 'side'
              dSign = -1.0;
            }
          SmVector3d sTempV;
          SmVector3d sUFPP = sFp.UnitizedDerivative(sFpp);
          SmVector3d sUGPP = sGp.UnitizedDerivative(sGpp);
          if (pOptJacobian)
            {
              (*pOptJacobian)[0][0] =  2.0 * sPivF.Dot(sFp);
              (*pOptJacobian)[0][1] = -2.0 * sPivG.Dot(sGp);
              (*pOptJacobian)[1][0] =        sFp.Dot(sUFp) + sPivF.Dot(sUFPP);
              (*pOptJacobian)[1][1] = -1.0 * dSign * (sGp.Dot(sUGp) + sPivG.Dot(sUGPP));
            }

          // Compute function values
          rF[0] = sPivF.Dot(sPivF) - sPivG.Dot(sPivG);
          rF[1] = sPivF.Dot(sUFp) - dSign * sPivG.Dot(sUGp);
        } // end SM_SO_SIGNED_PIVOT_MINIMIZE
        break;

      case SM_SO_SURFACE_PROJECTED_INTERSECT:
        {
          // Here the second curve is an SmCrvOnSurf, and we are finding the
          // point on the first curve that drops along the surface normal to
          // a point on the surface curve.  In other words, where the projection
          // of a curve that's off the surface would cross the surface curve.
          //
          // The objective functions are the same as for dropping a point to
          // the surface: the gap vector is perpendicular to both of the
          // surface's first derivative vectors.  But the independent variables
          // are both different: instead of the two surface parameters, they are
          // the parameters of the two curves, of course.  So this will involve
          // finding the first derivatives of the surface's first derivatives
          // w.r.t. the parameter of the surface curve.
          //
          // Terminology:
          //  C(t) is the space curve, m_crCurve1
          //  D(s) is the curve on surface
          //  S(u,v) is the surface
          //  Derivatives will be denoted by lower-case subscripts:
          //   Ct, Su, Sv, etc.;
          //   and the derivative of Su w.r.t. s will be Sus.
          //
          // Then:
          //  F(s,t) = (D-C) . Su
          //  G(s,t) = (D-C) . Sv
          //
          // Fs =  Ds . Su  +  (D-C) . Sus
          // Ft = -Dt . Su
          // Gs =  Ds . Sv  +  (D-C) . Svs
          // Gt = -Dt . Sv
          //
          // However, it was observed that the changing magnitudes of the
          // surface derivatives could push this in the wrong direction,
          // so we are now using unitized derivatives.  The complexity that
          // that adds to the derivatives is nicely encapsulated in
          // SmVector3d::UnitizedDerivative().

          SmCrvOnSurf *pCrvOnSurf = SM_CAST_NONNULL_PTR( SmCrvOnSurf, &m_crCurve2) ;
          if ( pCrvOnSurf == NULL )
            {
              rbFoundAnswer = FALSE;
              return SM_ERR;
            }

          // Just for nomenclature clarity:
          //SmPoint3d  &rC  = sPVV1[0];
          SmVector3d &rCt = sPVV1[1];

          //SmPoint3d  &rS  = sPVV2[0];
          SmVector3d &rSs = sPVV2[1]; // deriv of 3-d surface curve w.r.t. s

          SmVector3d sGap = - sVecFMinusG;

          // Surface quantities:
          SmVector3d sUDers[2], sVDers[2];
          pCrvOnSurf->EvaluateSurface1stDerivDerivs( crX[1], 1, FALSE, sUDers, sVDers );
          SmVector3d &rSu  = sUDers[0];
          SmVector3d &rSus = sUDers[1];
          SmVector3d &rSv  = sVDers[0];
          SmVector3d &rSvs = sVDers[1];

          // Unitize derivatives:
          SmVector3d sSuUnit( rSu );
          sSuUnit.Unitize();
          SmVector3d sSuUnit_s = rSu.UnitizedDerivative( rSus );
          SmVector3d sSvUnit( rSv );
          sSvUnit.Unitize();
          SmVector3d sSvUnit_s = rSv.UnitizedDerivative( rSvs );

          // Compute function values
          rF[0] = sGap.Dot( sSuUnit );
          rF[1] = sGap.Dot( sSvUnit );

          if (pOptJacobian)
            {
              (*pOptJacobian)[0][0] = -rCt.Dot( sSuUnit );
              (*pOptJacobian)[0][1] =  rSs.Dot( sSuUnit )  +  sGap.Dot( sSuUnit_s );
              (*pOptJacobian)[1][0] = -rCt.Dot( sSvUnit );
              (*pOptJacobian)[1][1] =  rSs.Dot( sSvUnit )  +  sGap.Dot( sSvUnit_s );
            }

        } // end SM_SO_SURFACE_PROJECTED_INTERSECT
        break;

      default:
        {
          // use normalization function
          // Curves are normalized when the projection of
          // the tangents of the curve to the vector between
          // the points on the curve is zero (they are perpendicular)
          // Set up Jacobian matrix for the following two functions
          // fun[0] = f'(f-g)
          // fun[1] = g'(f-g)
          //
          //  |f''(f-g) + f'f'        -f'g'          |
          //  |g'f'                   g''(f-g)-g'g'  |
          //
          // Where f is sPVV1 and g is sPVV2

          if (pOptJacobian)
            {
              (*pOptJacobian)[0][0] =   sPVV1[2].Dot(sVecFMinusG)
                                      + sPVV1[1].Dot(sPVV1[1]);
              (*pOptJacobian)[0][1] = - sPVV1[1].Dot(sPVV2[1]);
              (*pOptJacobian)[1][0] = - (*pOptJacobian)[0][1];
              (*pOptJacobian)[1][1] =   sPVV2[2].Dot(sVecFMinusG)
                                      - sPVV2[1].Dot(sPVV2[1]);
            }

          // Compute function values
          // For this one we take dot product of tangent and vector to point on
          // other curve and try to minimize that.
          // fun[0] = f'(f-g)
          // fun[1] = g'(f-g)
          rF[0] = sPVV1[1].Dot(sVecFMinusG);
          rF[1] = sPVV2[1].Dot(sVecFMinusG);

          if (sVecFMinusG.LengthSquared() < m_dSquaredTolerance)
            {
              rbFoundAnswer = TRUE;
              break;
            }
          double dProjDistSquared1 = (rF[0]*rF[0]) / (sVecFMinusG.LengthSquared());
          double dProjDistSquared2 = (rF[1]*rF[1]) / (sVecFMinusG.LengthSquared());

          if (dProjDistSquared1 < m_dSquaredTolerance &&
              dProjDistSquared2 < m_dSquaredTolerance)
            {
              rbFoundAnswer = TRUE;
              break;
            }
        } // end default
        break;
    } // end switch on m_eSolverOperation

#ifdef SM_DEBUG_CODE
  if (bDebugMe)
    {
      TCHAR sBuff[SM_TBLOCK_SIZE] ;
      if (pOptJacobian)
        {
          smos_sprintf(sBuff,  _T("%s SmFindCCNormalENFO::Evaluate - Iter[%ld]\n"),m_lCount==1?_T("\n"):_T(""),m_lCount) ;
          smos_WriteBuffer(sBuff) ;
          smos_sprintf(sBuff, _T("  x[0] = %16.16lf,   f[0] = %16.16lf,    Jacob[0][0] = %16.16lf, [0][1] = %16.16lf\n"),
                     crX[0], rF[0], pOptJacobian ? (*pOptJacobian)[0][0] : 0.0, pOptJacobian ? (*pOptJacobian)[0][1] : 0.0) ;
          smos_WriteBuffer(sBuff) ;
          smos_sprintf(sBuff, _T("  x[1] = %16.16lf,   f[1] = %16.16lf,         [1][0] = %16.16lf, [1][1] = %16.16lf\n"),
                     crX[1], rF[1], pOptJacobian ? (*pOptJacobian)[1][0] : 0.0, pOptJacobian ? (*pOptJacobian)[1][1] : 0.0) ;
          smos_WriteBuffer(sBuff) ;
          if(bDebugMe2)
            {
              smos_WriteBuffer(_T(" Jacobian from equations--- \n"));
              pOptJacobian->Dump();
              SmMatrix sTmpJ(2,2);
              SmTArray<double> sTmpX(crX);
              smos_WriteBuffer(_T(" Jacobian from finite differences--- \n")) ;
              SER(ComputeJacobian(sTmpX,rF,sTmpJ));
              sTmpJ.Dump();
            }
        }
    }
#endif // SM_DEBUG_CODE

  // all done
  return SM_SUCCESS;

} // end SmFindCCNormalENFO::Evaluate
// ---- end verbatim
// clang-format on
} // namespace sc_crv_p5

namespace
{
using namespace sc_crv_p5;

// Ops (keep in sync with crates/solidcore-ffi/src/units/curve_curve_p5.rs).
enum : int32_t
{
    OP_CLEANUP = 0,
    OP_REMOVE_DUP = 1,
    OP_STEP_POINT = 2,
    OP_SEAM = 3,
    OP_CACHE = 4,
    OP_CP_GUESS = 5,
    OP_IS_ARC = 6,
    OP_IS_LINE = 7,
    OP_IS_LINEAR = 8,
    OP_DOMAIN = 9,
    OP_PERIODIC = 10,
    OP_LENGTH = 11,
    OP_LPA = 12,
    OP_CCI = 13,
    OP_CC_TAN = 14,
    OP_NORMAL = 15,
    OP_EDGE_CACHE = 16,
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

// A curve block (see the file comment).
SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    ULONG dim = (ULONG)rd.idx();
    if (kind == 0 || kind == 5)
    {
        bool oob = rd.flag();
        SmBSplineCurve* c = new (ctx) SmBSplineCurve(read_nurb(rd), dim, FALSE);
        if (kind == 5)
        {
            // write the given knots back over the fixed-up NURB
            gw_CURVE* own = c->GetGwNurbPointer();
            gw_INDEX m = own->knt->m;
            for (gw_INDEX i = 0; i <= m; i++)
                own->knt->U[i] = rd.in[rd.at - (int64_t)(m + 1) + (int64_t)i];
        }
        c->SetOutOfBoundsEnabled(oob ? TRUE : FALSE);
        return c;
    }
    if (kind == 2)
    {
        SmPoint3d p = rd.vec();
        SmVector3d v = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double scale = rd.num();
        bool io = rd.flag();
        return new (ctx) SmLine(p, v, SmExtent1d(d0, d1), scale, 3, &ctx, NULL, io ? TRUE : FALSE);
    }
    if (kind == 3)
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double rx = rd.num();
        double ry = rd.num();
        bool io = rd.flag();
        return new (ctx) SmEllipse(c, x, y, SmExtent1d(d0, d1), rx, ry, dim, &ctx, NULL,
                                   io ? TRUE : FALSE);
    }
    if (kind == 4)
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double r = rd.num();
        bool io = rd.flag();
        return new (ctx) SmCircle(c, x, y, SmExtent1d(d0, d1), r, dim, &ctx, NULL, io ? TRUE : FALSE);
    }
    SmVector3d p1 = rd.vec();
    SmVector3d d1 = rd.vec();
    SmVector3d p2 = rd.vec();
    SmVector3d d2 = rd.vec();
    return new (ctx) SmHermiteCurve(p1, d1, p2, d2, dim);
}

void put_curve_points(std::vector<double>& o, const SmCurve* c)
{
    SmExtent1d nat = c->GetNaturalInterval();
    o.push_back(nat.GetMin());
    o.push_back(nat.GetMax());
    SmPoint3d p0, pm, p1;
    c->EvaluatePoint(nat.GetMin(), p0);
    c->EvaluatePoint(nat.Evaluate(0.5), pm);
    c->EvaluatePoint(nat.GetMax(), p1);
    put_vec(o, p0);
    put_vec(o, pm);
    put_vec(o, p1);
}

// Matrix and function value outputs of an ENFO evaluation.
void put_eval(std::vector<double>& o, SmStatus s, const SmTArray<double>& f, SmMatrix& j,
              SmBoolean found)
{
    o.push_back(st(s));
    o.push_back(f[0]);
    o.push_back(f[1]);
    o.push_back(j[0][0]);
    o.push_back(j[0][1]);
    o.push_back(j[1][0]);
    o.push_back(j[1][1]);
    o.push_back(flg(found));
}

void run(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_CLEANUP:
    {
        int64_t n = rd.idx();
        SmTArray<SmCurve*> curves;
        std::map<const SmCurve*, int64_t> index;
        for (int64_t k = 0; k < n; k++)
        {
            SmCurve* c = read_curve(rd, ctx);
            index[c] = k;
            curves.Add(c);
        }
        double tol = rd.num();
        ULONG removed = SmCurve::CrvsCleanup(curves, tol, TRUE);
        o.push_back(0.0);
        o.push_back((double)removed);
        o.push_back((double)curves.GetSize());
        for (ULONG i = 0; i < curves.GetSize(); i++)
        {
            o.push_back((double)index[curves[i]]);
            put_curve_points(o, curves[i]);
        }
        for (ULONG i = 0; i < curves.GetSize(); i++)
            delete curves[i];
        break;
    }
    case OP_REMOVE_DUP:
    {
        int64_t n = rd.idx();
        SmTArray<SmCurve*> curves;
        std::map<const SmCurve*, int64_t> index;
        for (int64_t k = 0; k < n; k++)
        {
            SmCurve* c = rd.flag() ? read_curve(rd, ctx) : NULL;
            if (c != NULL)
                index[c] = k;
            curves.Add(c);
        }
        SmCurve::RemoveDuplicateCurves(curves);
        o.push_back(0.0);
        o.push_back((double)curves.GetSize());
        for (ULONG i = 0; i < curves.GetSize(); i++)
            o.push_back(curves[i] == NULL ? -1.0 : (double)index[curves[i]]);
        for (ULONG i = 0; i < curves.GetSize(); i++)
            delete curves[i];
        break;
    }
    case OP_STEP_POINT:
    {
        SmCurve* c = read_curve(rd, ctx);
        double t = rd.num();
        SmPoint3d p;
        SmStatus s = c->EvaluateSTEPPoint(t, p);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, p);
        delete c;
        break;
    }
    case OP_SEAM:
    {
        SmCurve* a = read_curve(rd, ctx);
        double a0 = rd.num();
        double a1 = rd.num();
        SmCurve* b = read_curve(rd, ctx);
        double b0 = rd.num();
        double b1 = rd.num();
        SmSolutionType type = (SmSolutionType)rd.idx();
        double s0 = rd.num();
        double s1 = rd.num();
        bool this_first = rd.flag();
        SmSolution sol;
        sol.m_eSolutionType = type;
        sol.m_lNumVariables = 2;
        sol.m_lNumObjects = 2;
        sol.m_apObjects[0] = this_first ? (SmObject*)a : (SmObject*)b;
        sol.m_apObjects[1] = this_first ? (SmObject*)b : (SmObject*)a;
        sol.m_vStart[0] = s0;
        sol.m_vStart[1] = s1;
        SmBoolean r = sm_IsPointSolutionAcrossClosedSeamBoundary(sol, *a, SmExtent1d(a0, a1), *b,
                                                                 SmExtent1d(b0, b1));
        o.push_back(0.0);
        o.push_back(flg(r));
        delete a;
        delete b;
        break;
    }
    case OP_CACHE:
    {
        SmCurve* c = read_curve(rd, ctx);
        SmObjectCacheType type = (SmObjectCacheType)rd.idx();
        bool with_old = rd.flag();
        SmCacheObj* old = NULL;
        SmStatus s = SM_SUCCESS;
        if (with_old)
            s = c->CacheMakeOrValidate(SM_OC_CURVE, c, NULL, old);
        SmCacheObj* made = NULL;
        if (s == SM_SUCCESS)
            s = c->CacheMakeOrValidate(type, c, old, made);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(with_old && made == old));
            SmCurveCache* cache = (SmCurveCache*)made;
            o.push_back((double)cache->GetSpanCount());
            for (ULONG i = 0; i < cache->GetSpanCount(); i++)
            {
                SmExtent1d& ivl = cache->GetAt(i)->GetInterval();
                o.push_back(ivl.GetMin());
                o.push_back(ivl.GetMax());
            }
        }
        delete c;
        break;
    }
    case OP_EDGE_CACHE:
    {
        // the cache of a curve that defines a wire edge's shape, the edge's tolerance set
        // (SmTol::GetApproxTol3d(&m_crCurve) in Tessellate reads it through GetEdge)
        SmCurve* c = read_curve(rd, ctx);
        double tol = rd.num();
        SmBrep* brep = new (ctx) SmBrep();
        SmEdge* edge = NULL;
        SmStatus s = brep->CreateWireEdgeFromCurve(c, c->GetNaturalInterval(), edge);
        if (s == SM_SUCCESS && (edge == NULL || edge->GetCurve() != c))
            throw std::runtime_error("CreateWireEdgeFromCurve did not keep the curve");
        SmCacheObj* made = NULL;
        if (s == SM_SUCCESS)
        {
            edge->SetTolerance(tol, FALSE, FALSE);
            s = c->CacheMakeOrValidate(SM_OC_CURVE, c, NULL, made);
        }
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            SmCurveCache* cache = (SmCurveCache*)made;
            o.push_back((double)cache->GetSpanCount());
            for (ULONG i = 0; i < cache->GetSpanCount(); i++)
            {
                SmExtent1d& ivl = cache->GetAt(i)->GetInterval();
                o.push_back(ivl.GetMin());
                o.push_back(ivl.GetMax());
            }
        }
        // the B-rep owns the curve once the edge holds it (CreateWireEdgeFromCurve
        // consumes it on success)
        if (s != SM_SUCCESS && c->GetOwner() == NULL)
            delete c;
        delete brep;
        break;
    }
    case OP_CP_GUESS:
    {
        SmCurve* c = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        SmVector3d abc = rd.vec();
        double t = rd.num();
        SmBoolean found = FALSE;
        double guess = 0.0;
        SmStatus s = sm_FindAdditionalCpGuess(*c, SmExtent1d(a, b), abc, t, found, guess);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(found));
            o.push_back(found ? guess : 0.0);
        }
        delete c;
        break;
    }
    case OP_IS_ARC:
    {
        SmCurve* c = read_curve(rd, ctx);
        ULONG n = (ULONG)rd.idx();
        double tol = rd.num();
        SmAxis2Placement f;
        double r = 0.0, s0 = 0.0, s1 = 0.0;
        SmBoolean ok = c->IsArc(n, tol, f, r, s0, s1);
        o.push_back(0.0);
        o.push_back(flg(ok));
        if (ok)
        {
            put_vec(o, f.GetOriginRef());
            put_vec(o, f.GetXAxisRef());
            put_vec(o, f.GetYAxisRef());
            o.push_back(r);
            o.push_back(s0);
            o.push_back(s1);
        }
        delete c;
        break;
    }
    case OP_IS_LINE:
    {
        SmCurve* c = read_curve(rd, ctx);
        ULONG n = (ULONG)rd.idx();
        double tol = rd.num();
        SmPoint3d p;
        SmVector3d v;
        SmBoolean ok = c->IsLine(n, tol, p, v);
        o.push_back(0.0);
        o.push_back(flg(ok));
        if (ok)
        {
            put_vec(o, p);
            put_vec(o, v);
        }
        delete c;
        break;
    }
    case OP_IS_LINEAR:
    {
        SmCurve* c = read_curve(rd, ctx);
        double tol = rd.num();
        bool want = rd.flag();
        SmPoint3d p;
        SmVector3d v;
        double dev = 0.0;
        SmBoolean ok = c->IsLinear(tol, &p, &v, want ? &dev : NULL);
        o.push_back(0.0);
        o.push_back(flg(ok));
        if (ok)
        {
            put_vec(o, p);
            put_vec(o, v);
        }
        o.push_back(dev);
        delete c;
        break;
    }
    case OP_DOMAIN:
    {
        SmCurve* c = read_curve(rd, ctx);
        double umin = rd.num();
        double vmin = rd.num();
        double umax = rd.num();
        double vmax = rd.num();
        double tol = rd.num();
        SmSurfParamType r = c->IsDomainBoundary(SmExtent2d(umin, vmin, umax, vmax), tol);
        o.push_back(0.0);
        o.push_back((double)r);
        delete c;
        break;
    }
    case OP_PERIODIC:
    {
        SmCurve* c = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        SmBoolean r = c->IsPeriodic(SmExtent1d(a, b));
        o.push_back(0.0);
        o.push_back(flg(r));
        delete c;
        break;
    }
    case OP_LENGTH:
    {
        SmCurve* c = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        double acc = rd.num();
        double len = 0.0;
        SmStatus s = c->Length(SmExtent1d(a, b), acc, len);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back(len);
        delete c;
        break;
    }
    case OP_LPA:
    {
        SmCurve* c = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        SmCurvePropertyType prop = (SmCurvePropertyType)rd.idx();
        double guess = rd.num();
        bool has_val = rd.flag();
        double val = rd.num();
        int64_t nv = rd.idx();
        SmVector3d vecs[3];
        for (int64_t i = 0; i < nv; i++)
            vecs[i] = rd.vec();
        SmBoolean found = FALSE;
        SmSolution sol;
        SmStatus s = c->LocalPropertyAnalysis(SmExtent1d(a, b), prop, guess, has_val ? &val : NULL,
                                              nv > 0 ? vecs : NULL, found, sol);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(found));
            if (found)
            {
                o.push_back((double)sol.m_eSolutionType);
                o.push_back(sol.m_vStart[0]);
                o.push_back(sol.m_vStart.m_dSolutionValue);
            }
        }
        delete c;
        break;
    }
    case OP_CCI:
    case OP_CC_TAN:
    {
        SmCurve* a = read_curve(rd, ctx);
        SmCurve* b = read_curve(rd, ctx);
        SmTArray<double> x(2);
        x.SetSize(2);
        x[0] = rd.num();
        x[1] = rd.num();
        bool jac = rd.flag();
        SmTArray<double> f(2);
        f.SetSize(2);
        f[0] = 0.0;
        f[1] = 0.0;
        SmMatrix j(2, 2);
        j[0][0] = j[0][1] = j[1][0] = j[1][1] = 0.0;
        SmBoolean found = FALSE;
        if (op == OP_CCI)
        {
            SmCCIntersectENFO e(*a, *b, SM_EFF_ZERO_SQ);
            SmStatus s = e.Evaluate(x, f, jac ? &j : NULL, found);
            put_eval(o, s, f, j, found);
            o.push_back(e.GetSolutionSize());
            o.push_back(e.GetScaledZero());
        }
        else
        {
            SmCCTangentIntENFO e(*a, *b, SM_EFF_ZERO_SQ);
            SmStatus s = e.Evaluate(x, f, jac ? &j : NULL, found);
            put_eval(o, s, f, j, found);
        }
        delete a;
        delete b;
        break;
    }
    case OP_NORMAL:
    {
        SmCurve* a = read_curve(rd, ctx);
        SmCurve* b = read_curve(rd, ctx);
        double sqtol = rd.num();
        double sqdist = rd.num();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        int64_t nv = rd.idx();
        SmVector3d vecs[3];
        for (int64_t i = 0; i < nv; i++)
            vecs[i] = rd.vec();
        SmFindCCNormalENFO e(*a, *b, sqtol, sqdist, sop, nv > 0 ? vecs : NULL);
        int64_t calls = rd.idx();
        o.push_back(0.0);
        for (int64_t k = 0; k < calls; k++)
        {
            SmTArray<double> x(2);
            x.SetSize(2);
            x[0] = rd.num();
            x[1] = rd.num();
            bool jac = rd.flag();
            SmTArray<double> f(2);
            f.SetSize(2);
            f[0] = 0.0;
            f[1] = 0.0;
            SmMatrix j(2, 2);
            j[0][0] = j[0][1] = j[1][0] = j[1][1] = 0.0;
            SmBoolean found = FALSE;
            SmStatus s = e.Evaluate(x, f, jac ? &j : NULL, found);
            put_eval(o, s, f, j, found);
        }
        delete a;
        delete b;
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_curve_curve_p5_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                         int64_t cap, int64_t* out_len)
{
    if (in == nullptr || n_in < 0 || out_len == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    Rd rd{in, 0};
    std::vector<double> o;
    run(op, rd, ctx, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
