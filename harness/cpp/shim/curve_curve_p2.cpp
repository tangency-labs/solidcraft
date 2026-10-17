// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.curve_p2`: the SmCurve members owned by the unit
// (source/SMLib/src/SmCurve.cpp, source/SMLib/inc/SmCurve.h), called through `SmCurve*` on
// SmBSplineCurve and SmHermiteCurve objects so the virtual dispatch (derived override or
// base-class default) is upstream's, plus the file-static helpers and local classes of
// SmCurve.cpp.
//
// Static functions and classes local to SmCurve.cpp cannot be linked against and the file
// cannot be recompiled inside a namespace (it defines SmCurve members), so the helpers used
// here are VERBATIM copies of upstream SmCurve.cpp (commit c9c5979), inside the namespace
// sc_crv_p2 (line ranges noted at each block; generated, do not edit by hand). Their
// LocalSolve members are defined as SM_ERR stubs: only their constructors,
// BranchMayContainAnswers and FindBestSubdivisionNode are exercised. sm_TestStep is not
// static and is linked from the library.
//
// One entry point, sc_curve_curve_p2_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/curve_curve_p2.rs, which validates every
// input. Curve ops start with a curve block `kind ...` (0: SmBSplineCurve `dim oob n p m
// cpts(x y z w) knots`; 1: SmHermiteCurve `dim P1 D1 P2 D2`). `out` receives the op status
// (0 = SM_SUCCESS, else the SmStatus value) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmConic.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveCache.h>
#include <SmCurveTypes.h>
#include <SmDatabaseIO.h>
#include <SmExtent1d.h>
#include <SmExtent3d.h>
#include <SmGeomUtility.h>
#include <SmGlobalSolver.h>
#include <SmHermiteCurve.h>
#include <SmIntegrator.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmTArray.h>
#include <SmTree.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <memory>
#include <sstream>
#include <string>
#include <vector>

#include "sc_sm.h"
#include "sc_gw.h"

using sc::Rd;
using sc::put_vec;
using sc::read_nurb;

// Not static in SmCurve.cpp and not declared in a header.
SmStatus sm_TestStep(double dCurrentParameter, double dParamStepSize, SmVector3d sCurPVV[3],
                     SmVector3d sNextPVV[3], double dChordHeightTolerance,
                     double dCrvAngleToleranceDeg, double dMaximum3DDistanceSquared,
                     SmTessStepTestResultType& reResult);

namespace sc_crv_p2
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

// ---- verbatim: SmCurve.cpp lines 359-688: sm_PushPoint, sm_PopStack, sm_StackLook, sm_FlushCrv, sm_AddToCurve
static SmStatus sm_PushPoint
  (double             dCurrT,     // in : param
   SmPoint3d          aCurrPV[2], // in : position and tangent
   SmTArray<double> & rStack)     // i/o: target stack
{
  // param
  rStack.Add(dCurrT) ;

  // pos
  rStack.Add(aCurrPV[0].x);
  rStack.Add(aCurrPV[0].y);
  rStack.Add(aCurrPV[0].z);

  // tan
  rStack.Add(aCurrPV[1].x);
  rStack.Add(aCurrPV[1].y);
  rStack.Add(aCurrPV[1].z);

  // all done
  return(SM_SUCCESS) ;

} // end sm_PushPoint

/*******************************************************************//***
PURPOSE:  Pos stack values [param posXYZ tanXYZ] into output args

NOTES: returns ERR when stack is empty
                   (or too short - needs 7 values per stack entry)
************************************************************************/
static SmStatus sm_PopStack
  (double           & rdCurrT,     // out: param value
   SmPoint3d          aCurrPV[2],  // out: posXYZ and tanXYZ
   SmTArray<double> & rStack)      // in : target stack
{
  if (rStack.GetSize() < 7) SER(SM_ERR);

  // tan
  aCurrPV[1].z = rStack.GetLast() ; rStack.RemoveLast() ;
  aCurrPV[1].y = rStack.GetLast() ; rStack.RemoveLast() ;
  aCurrPV[1].x = rStack.GetLast() ; rStack.RemoveLast() ;

  // pos
  aCurrPV[0].z = rStack.GetLast() ; rStack.RemoveLast() ;
  aCurrPV[0].y = rStack.GetLast() ; rStack.RemoveLast() ;
  aCurrPV[0].x = rStack.GetLast() ; rStack.RemoveLast() ;

  // param
  rdCurrT = rStack.GetLast() ; rStack.RemoveLast() ;

  // all done
  return SM_SUCCESS;

} // end sm_PopStack

/*******************************************************************//***
PURPOSE:  Load the last point value [para, posXYZ, tanXYZ]
             into output args without, leaving the stack unmodified

NOTES:
************************************************************************/
static SmStatus sm_StackLook
  (double           & rdCurrT,     // out: StackLastPoint's param
   SmPoint3d          aCurrPV[2],  // out: StackLastPoint's pos and tan
   SmTArray<double> & rStack)      // in : target Stack
{
  // ERR - when stack is empty
  if (rStack.GetSize() < 7)
    { SER(SM_ERR) ; }

  ULONG lIdx = rStack.GetSize()-1;

  // tan
  aCurrPV[1].z = rStack[lIdx] ; lIdx--;
  aCurrPV[1].y = rStack[lIdx] ; lIdx--;
  aCurrPV[1].x = rStack[lIdx] ; lIdx--;

  // pos
  aCurrPV[0].z = rStack[lIdx] ; lIdx--;
  aCurrPV[0].y = rStack[lIdx] ; lIdx--;
  aCurrPV[0].x = rStack[lIdx] ; lIdx--;

  // param
  rdCurrT = rStack[lIdx] ;

  // all done
  return SM_SUCCESS;

} // end sm_StackLook

/*******************************************************************//***
PURPOSE: Flush out the curve defined in the control point and knots arrays
            and create a B-Spline and put it into the curves array.

NOTES: on output, crNewBSplineCurve->dimension == 3,
       when building UVTrimCurves call ConvertTo2D()
************************************************************************/
static SmStatus sm_FlushCrv
  (const SmContext     & crContext,
   double                dTolerance,
   SmTArray<SmPoint3d> & rCntrlPoly,
   SmTArray<double>    & rKnots,
   SmBSplineCurve     *& crNewBSplineCurve,
   SmBoolean             bOptCreateAnalytics)
{
// when not using analytics - don't ask for them
#ifndef USE_ANALYTICS
 bOptCreateAnalytics = FALSE ;
#endif // no USE_ANALYTICS

    if (rKnots.GetSize() == 0) { return SM_SUCCESS; }

#ifdef USE_ANALYTICS
#else // no USE_ANALYTICS
    bOptCreateAnalytics = FALSE;
#endif // no USE_ANALYTICS
    ULONG ii, jj;

    // Test to see if points are on an arc.
    if ( rCntrlPoly.GetSize() > 5 && bOptCreateAnalytics )
    {
        SmPoint3d  sP1 = rCntrlPoly[0];
        SmVector3d sV1 = rCntrlPoly[1] - sP1;
        SmPoint3d  sP2 = rCntrlPoly[3];
        SmVector3d sV2 = sP2 - rCntrlPoly[2];
        SmVector3d sNorm = sV1 * sV2;
        if ( sNorm.LengthSquared() > SM_EFF_ZERO_SQ )
        {
            SER( sNorm.Unitize() );
            SmVector3d sLineVec1 = sNorm * sV1;
            SmVector3d sLineVec2 = sNorm * sV2;
            double dParam1, dParam2;
            SmBoolean bCircle  = TRUE;
            double dTotalAngle = 0.0;
            if ( smgu_LineLineClosestPoint( sP1, sLineVec1, sP2, sLineVec2,
                        dParam1, dParam2 ) == SM_SUCCESS )
            {
                SmPoint3d sCent = sP1 + dParam1 * sLineVec1;
                double dRadius  = sCent.DistanceBetween( sP1 );
                double dSize    = sP1.DistanceBetween( sP2 )
                                + sP2.DistanceBetween( rCntrlPoly.GetLast() );
                dSize = smos_Max( dSize, SM_EFF_ZERO*(1.0+sP2.GetMaxDimension()) );
                double dAnalyticTol = 10000.0 * SM_EFF_ZERO
                        * (1.0 + sP1.GetMaxDimension() + sP2.GetMaxDimension());
                for ( ii=3; ii<rCntrlPoly.GetSize(); ii++ )
                {
                    double dTestRadius = sCent.DistanceBetween(rCntrlPoly[ii]);
                    if (   smos_Fabs( dTestRadius - dRadius ) > dAnalyticTol
                        || dRadius > 100.0 * dSize ) // Avoid Making Really big circles.
                    {
                        bCircle = FALSE;
                        break;
                    }
                    SmVector3d sVec1 = rCntrlPoly[ ii-3 ] - sCent;
                    SmVector3d sVec2 = rCntrlPoly[ ii   ] - sCent;
                    double dAngle;
                    if (   sVec1.LengthSquared() > SM_EFF_ZERO_SQ
                        && sVec2.LengthSquared() > SM_EFF_ZERO_SQ )
                    {
                        SER(sVec1.AngleBetween( sVec2, dAngle ));
                        dTotalAngle += dAngle;
                    }
                    ii+=2;
                } // end for each control point

                if ( bCircle )
                {
                    SmBSplineCurve *pArc = NULL ;
                    SmAxis2Placement sRefFrame;
                    SmVector3d sXAxis = rCntrlPoly[0] - sCent;
                    SER(sXAxis.Unitize());
                    SmVector3d sRef = rCntrlPoly[3] - sCent;
                    SmVector3d sCross = sXAxis * sRef;
                    SmVector3d sYAxis = sCross * sXAxis;
                    if ( sYAxis.LengthSquared() > SM_EFF_ZERO_SQ )
                    {
                        SER( sYAxis.Unitize() );
                        SER( sRefFrame.SetCanonical( sCent, sXAxis, sYAxis ));
                        if ( dTotalAngle > SM_PI * 2.0 )
                            { dTotalAngle = SM_PI * 2.0; }
                        dTotalAngle = SM_RAD2DEG( dTotalAngle );
                        SER( SmBSplineCurve::CreateCircleSegment( crContext, 3,
                                sRefFrame, dRadius, 0.0, dTotalAngle,
                                SM_CO_QUADRATIC, pArc ));
                        if ( pArc )
                        {
                            SmExtent1d sIvl( rKnots[0], rKnots.GetLast() );
                            pArc->EditParameterization( sIvl );
                            // Check whether the approximation is accurate.
                            SmBoolean bGoodApproximation = TRUE;
                            double dTolSq = dTolerance*dTolerance;
                            for ( jj=0; jj<rKnots.GetSize(); jj++ )
                            {
                                SmPoint3d sPnt = rCntrlPoly[jj*3];
                                double dParam = rKnots[jj];
                                SmPoint3d sTestPnt;
                                SER( pArc->EvaluatePoint( dParam, sTestPnt ));
                                double dDistSq = sTestPnt.DistanceBetweenSquared(sPnt);
                                if ( dDistSq > dTolSq ) {
                                    bGoodApproximation = FALSE;
                                }
                            }
                            if ( bGoodApproximation ) {
                                crNewBSplineCurve = pArc;
                                return SM_SUCCESS;
                            }
                            else {
                                SM_ASSERT( pArc != NULL ); delete pArc; pArc = NULL;
                            }
                        } // end if pArc==True
                    } // end if sYAxis not zero
                } // end if bCircle==True
            } // end if LineClosestPoint didn't fail
        } // end if sNorm (cross product) not zero
    } // end if trying to create a circle

    SmTArray<ULONG> sKnotMult( rKnots.GetSize(), NULL, rKnots.GetSize() );
    sKnotMult[0] = 4;
    for ( ii=1; ii+1<sKnotMult.GetSize(); ii++ ) { // note: can't say sKnotMult.GetSize()-1
        sKnotMult[ii] = 3;
    }
    sKnotMult[sKnotMult.GetSize()-1] = 4;
    SmBSplineCurve *pNewBSP = NULL ;
    ULONG lDimension = 3;
    ULONG lDegree = 3;
    SmExtent1d sIvl(rKnots[0],rKnots.GetLast());
    SER_DELETE( SmBSplineCurve::CreateCanonical( crContext,           // in : context for new object construction
                                          lDimension,          // in : valid dimension - 2 or 3
                                          lDegree,             // in : valid degree - 1 through 32
                                          rCntrlPoly,          // in : Euclidian form of the control points.
                                          SM_CF_UNSPECIFIED,   // in : oneof  SM_CF_POLYLINE_FORM,  SM_CF_PARABOLIC_ARC,
                                                               //             SM_CF_CIRCULAR_ARC,   SM_CF_HYPERBOLIC_ARC,
                                                               //             SM_CF_ELLIPTIC_ARC,   SM_CF_UNSPECIFIED,
                                                               //             SM_CF_HELICAL_ARC
                                          sKnotMult,           // in : Multiplicities of the end knots must be lDegree+1 and
                                                               //      and internal multiplicities must be lDegree or less.
                                          rKnots,              // in : unique knots in ascending order
                                          SM_KT_UNSPECIFIED,   // in : oneof  SM_KT_UNIFORM_KNOTS,  SM_KT_QUASI_UNIFORM_KNOTS,
                                                               //             SM_KT_UNSPECIFIED,    SM_KT_PIECEWISE_BEZIER_KNOTS
                                          NULL,                // in : optional associated weigths for each control point
                                          NULL,                // in : If specified contains a trimming interval to resize the curve
                                          pNewBSP )            // out: New BSplineCurve
                                         ,pNewBSP );            // if we fail, delete the BSplineCurve (JLMCC hunting memory leaks)

#ifdef SM_DEBUG_CODE
    SmBoolean bDebugMe = FALSE ;
    if (bDebugMe) {
        pNewBSP->Dump();
    }
#endif // SM_DEBUG_CODE

// Note that the following line will remove extra knots and
// decrease the size of the curve.  This can easily be done
// after drop curve
//    SER(pNewBSP->RemoveExtraKnots(SM_EFF_ZERO));
// #ifdef SM_DEBUG_CODE
//     if (bDebugMe) {
//         pNewBSP->Dump();
//     }
// #endif // SM_DEBUG_CODE

    rKnots.ReSet();
    rCntrlPoly.ReSet();
    crNewBSplineCurve = pNewBSP;
    return SM_SUCCESS;

} // end sm_FlushCrv

/*******************************************************************//***
PURPOSE: Add a Bezier segment to the growing control polygon and knots arrays
            of an piecewise cubic bezier approximation BSpline curve

NOTES: signals an error when a discontinuous interval is asked
            to be appended to a curve with existing intervals.
************************************************************************/
static SmStatus sm_AddToCurve
  (const SmPoint3d       caLastPV[2],  // in : span beg pos and tan
   const SmPoint3d       caCurrPV[2],  // in : span end pos and tan
   double                dLastT,       // in : span beg param
   double                dCurrT,       // in : span end param
   SmTArray<SmPoint3d> & rCntrlPoly,   // i/o: growing BSpline control polygon (piecewise cubic bezier)
   SmTArray<double>    & rKnots)       // i/o: growing BSpline knot vector (piecewise cubic bezier)
{
  // locals
  double     dScale = dCurrT - dLastT;
  SmPoint3d  sP1(caLastPV[0]);           // 1st point in BSpline interval Polygon
  SmPoint3d  sP4(caCurrPV[0]);           // 4th point in BSpline interval Polygon
  SmVector3d sD1(caLastPV[1]*dScale);
  SmVector3d sD2(caCurrPV[1]*dScale);

  // convert Hermite params into a Bezier cubic polygon:
  //   scale Hermite end tan vecs by 1/3 and add/subtract from the first/last vertices.
  SmPoint3d sP2 = sP1 + sD1 / 3.0;       // 2nd point in BSpline interval Polygon
  SmPoint3d sP3 = sP4 - sD2 / 3.0;       // 3rd point in BSpline interval Polygon

  // when adding intervals to an existing Curve start - check for continuity
  if (rKnots.GetSize() > 0)
    {
      // if curve is discontinuous in either param or XYZ space
      double dKnotScale = 100.0 * (1.0 + smos_Fabs(dLastT));
      double dPntScale  = 100.0 * (1.0 + sP1.GetMaxDimension());
      if(   smos_Fabs(rKnots.GetLast()-dLastT)        > dKnotScale * SM_EFF_ZERO
         || sP1.DistanceBetween(rCntrlPoly.GetLast()) > dPntScale  * SM_EFF_ZERO)
        {
          // signal an error for now
          SER(SM_ERR);
        }
    } // end appending interval check

  // Now add to knots and control polygon.

  // when starting the curve
  if (rKnots.GetSize() == 0)
    {
      // add the 1st control polygon point and start param
      rKnots.Add(dLastT);
      rCntrlPoly.Add(sP1);
    }

  // add end param value
  rKnots.Add(dCurrT);

  // add middle and end control polygon points
  rCntrlPoly.Add(sP2);
  rCntrlPoly.Add(sP3);
  rCntrlPoly.Add(sP4);

  // all done
  return SM_SUCCESS;

} // end sm_AddToCurve
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 3183-3254: sm_SnapSolToSpecifiedEndpoint
static SmStatus sm_SnapSolToSpecifiedEndpoint
  (const SmCurve    * cpCurve,          // in : target curve
   const SmExtent1d & crInterval,       // in : target curve interval
   const SmVector3d * cpVecToCrvInside, // in : CrvTangent pointing from EndPt into Curve interior, NULL=do nothing.
   double           & dSolution,        // out: CrvParam 1d val for snap check: 
                                        //        when DistToEnd3d < SnapTol3d in cpVecToCrvInside dir, Sol snapped to Ivl EndPt
   SmXSectTol3d       sSnapTol3d,       // in : Max 3d Snap to End distance
   SmBoolean        & rbIsGoodSolution) // out: FALSE=Sol in tol of EndPt in opposite dir of cpVecToCrvInside:[No Snap].
                                        //      TRUE =Sol beyond tol of EndPt:[No Snap] or in tol of EndPt in cpVecToCrvInside dir:[Snap].
{
  // init output
  rbIsGoodSolution = TRUE;

  // local
  SmBoolean bMinBound = FALSE ;

  // All solutions are good when cpVecToCrvInside == NULL
  if (!cpVecToCrvInside) { return SM_SUCCESS; }

  // The given point on the curve:
  SmPoint3d sSolPnt;
  SER( cpCurve->EvaluatePoint( dSolution, sSolPnt ));

  // Check the start point
  SmVector3d sPV[2];
  SER( cpCurve->Evaluate( crInterval.GetMin(), 1, TRUE, sPV, TRUE )); // NonZeroTangent values

  double dMinGap = sPV[0].DistanceBetween(sSolPnt) ;

  // when MinBoundary/Solution gap is less than tolerance
  if (dMinGap < sSnapTol3d)
    {
      bMinBound = TRUE ;

      // snap Solution to start boundary
      if (cpVecToCrvInside->Dot(sPV[1]) < 0.0)
        {
          rbIsGoodSolution = FALSE;
        }
      else
        {
          rbIsGoodSolution = TRUE;
          dSolution = crInterval.GetMin();
        }
    } // end MinBoundary/Solution gap is less than tolerance check

  // get MaxBoundaryPoint and gap
  SER( cpCurve->Evaluate( crInterval.GetMax(), 1, TRUE, sPV, TRUE )); // NonZeroTangent values
  double dMaxGap = sPV[0].DistanceBetween(sSolPnt) ;

  // when MaxBoundary/Solution gap is less than tolerance
  // and we haven't snapped to the minBoundary
  if (   dMaxGap < sSnapTol3d
      && (bMinBound == FALSE || rbIsGoodSolution == FALSE))
    {
      // snap Solution to end boundary
      sPV[1] = -sPV[1];
      if (cpVecToCrvInside->Dot(sPV[1]) < 0.0)
        {
          rbIsGoodSolution = FALSE;
        }
      else
        {
          rbIsGoodSolution = TRUE;
          dSolution = crInterval.GetMax();
        }
    } // end MaxBoundary/Solution gap is less than tolerance check

  // all done
  return SM_SUCCESS;

} // end sm_SnapSolToSpecifiedEndpoint
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

// ---- verbatim: SmCurve.cpp lines 8200-8382: SmCCGlobalSolver (class, constructor, BranchMayContainAnswers)
class SmCCGlobalSolver : public SmGlobalSolver
{
public:
  const SmExtent1d & m_crInterval1;      // interval for 1st curve being compared
  const SmExtent1d & m_crInterval2;      // interval for 2nd curve being compared

  // constructor
  SmCCGlobalSolver(SmTree           * pPointTree, 
                   SmTree           * pCurveTree,
                   const SmExtent1d & crInterval1, 
                   const SmExtent1d & crInterval2);

  // virtual methods
  virtual SmBoolean BranchMayContainAnswers(SmTreeNode * apBranch[SM_GS_MAX_TREES]);
  virtual SmStatus  LocalSolve(SmTreeNode * apBranch[SM_GS_MAX_TREES],
                               SmBoolean  & rbNeedsMoreSubdivision);

} ; // end class SmCCGlobalSolver

/*******************************************************************//**
PURPOSE:

NOTES:
***********************************************************************/
SmCCGlobalSolver::SmCCGlobalSolver
  (SmTree *pPointTree, SmTree *pCurveTree,
   const SmExtent1d & crInterval1,
   const SmExtent1d & crInterval2)
: SmGlobalSolver(pPointTree,pCurveTree,1),
  m_crInterval1(crInterval1),
  m_crInterval2(crInterval2)
{

} // end SmCCGlobalSolver::SmCCGlobalSolver constructor

/*******************************************************************//**
PURPOSE: return TRUE when branch might contain answers,
            else return FALSE when branch is known not to contain answers.

NOTES:
***********************************************************************/
SmBoolean SmCCGlobalSolver::BranchMayContainAnswers
  (SmTreeNode * apBranch[SM_GS_MAX_TREES])
{
  // pass call to base class except for these special cases:
  if (   m_eSolverOperation != SM_SO_NORMALIZE
      && m_eSolverOperation != SM_SO_PROJECTED_TANGENCY)
    {
      return SmGlobalSolver::BranchMayContainAnswers(apBranch);
    }

  // Here m_eSolverOperation is either SM_SO_NORMALIZE or SM_SO_PROJECTED_TANGENCY.
  // looking for points with common tangents -
  // cull cases where segment angle turning limits prohibit
  // finding such a solution.

  // locals
  SmTreeNode *pCurveNode1 = apBranch[0];
  SmTreeNode *pCurveNode2 = apBranch[1];

  // Make sure nodes are SmBezierSpan types
  if (   (   pCurveNode1->m_eAuxDataType != SM_AD_BEZIER_CURVE
          && pCurveNode1->m_eAuxDataType != SM_AD_CURVE_DATA)
      || (   pCurveNode2->m_eAuxDataType != SM_AD_BEZIER_CURVE
          && pCurveNode2->m_eAuxDataType != SM_AD_CURVE_DATA))
    {
      return TRUE;
    }

  // get BezierSpan Data
  SmBezierAux1d *pBezSpan1 = (SmBezierAux1d*)pCurveNode1->m_pData; NER(pBezSpan1);
  SmBezierAux1d *pBezSpan2 = (SmBezierAux1d*)pCurveNode2->m_pData; NER(pBezSpan2);

  // Can't check for Normals when accumulated angle turning in both spans exceeds 90
  if (pBezSpan1->m_dMaxTurningAngleDeg + pBezSpan2->m_dMaxTurningAngleDeg > 90.0)
    {
      return TRUE;
    }

  // get 1st curve and 2nd curve CurveCaches
  SmCurveCache *pCurveCache1 = (SmCurveCache*)pCurveNode1->m_pTree->GetOwner();
  SmCurveCache *pCurveCache2 = (SmCurveCache*)pCurveNode2->m_pTree->GetOwner();

  // get 1st Bezier span ends
  SmExtent1d sIvl;
  SmPoint3d sStart1, sEnd1;
  pBezSpan1->GetEnds(*pCurveCache1,sStart1,sEnd1,sIvl);

  // get 2nd Bezier span ends
  SmPoint3d sStart2, sEnd2;
  pBezSpan2->GetEnds(*pCurveCache2,sStart2,sEnd2,sIvl);

  // for PROJECTED_TANGENCY - project endPoints to common plane
  if (m_eSolverOperation == SM_SO_PROJECTED_TANGENCY)
    {
      // Project all endPoints to a common plane (sStart1, normal vector in m_cpOptVectors[0])
      smgu_PointProjectToPlane(sEnd1,sStart1,m_cpOptVectors[0],sEnd1);
      smgu_PointProjectToPlane(sStart2,sStart1,m_cpOptVectors[0],sStart2);
      smgu_PointProjectToPlane(sEnd2,sStart1,m_cpOptVectors[0],sEnd2);
    }

  // get 1st and 2nd curve endPoint chord lines
  SmVector3d sLineVec1 = sEnd1 - sStart1;
  SmVector3d sLineVec2 = sEnd2 - sStart2;

  // If we don't have good lines quit here with a TRUE
  if (   sLineVec1.LengthSquared() < SM_EFF_ZERO_SQ
      || sLineVec2.LengthSquared() < SM_EFF_ZERO_SQ)
    {
      return TRUE;
    }

  // get closest line between 1st and 2nd curve endPoint chordLines
  double dT1, dT2;
  if (smgu_LineLineClosestPoint(sStart1,sLineVec1,sStart2,sLineVec2,dT1,dT2) == SM_SUCCESS)
    {
      // get gap vector at closest points
      SmPoint3d sP1 = sStart1 + dT1 * sLineVec1;
      SmPoint3d sP2 = sStart2 + dT2 * sLineVec2;
      SmVector3d sVec = sP1 - sP2;

      // when gap vector is zero
      if (sVec.LengthSquared() < SM_EFF_ZERO_SQ)
        {
          // Coplanarity - get angle between chords lines
          double dAng;
          SER(sLineVec1.AngleBetween(sLineVec2,dAng));
          double dAngDeg = dAng * 180.0 / SM_PI;
          if (dAngDeg > 90.0) dAngDeg = 180.0 - dAngDeg;

          // when angle is larger than the combined turning angle
          double dCombinedTurningAngle =   pBezSpan1->m_dMaxTurningAngleDeg
                                         + pBezSpan2->m_dMaxTurningAngleDeg ;
          if (dAngDeg * 0.99 - SM_EFF_ZERO > dCombinedTurningAngle)
            {
              // no normalized solution possible
              return FALSE; // No answer found
            }
        } // end
    } // end line/line closest point check

  // For now this is as far as we go with the projected tangency
  // If we need to optimize further we could some day by adding angular check
  // of possible vectors between the two segments like as is done by the following
  // normalization check.
  if (m_eSolverOperation == SM_SO_PROJECTED_TANGENCY)
    {
      return TRUE;
    }

  // Here we test to see if both ends of each
  // segment are outside of the viable angle of the other segment.

  // First Test to see if segment 2 is outside of valid angle of segment1
  if (pBezSpan1->m_dMaxTurningAngleDeg < 90.0)
    {
      double dAng1 = smgu_SegmentPointAngleDegToPerp(sStart1,sEnd1,sStart2,dT1);
      double dAng2 = smgu_SegmentPointAngleDegToPerp(sStart1,sEnd1,sEnd2,dT2);

      if (smos_Min(dAng1,dAng2) > pBezSpan1->m_dMaxTurningAngleDeg &&
          smos_Fabs(dT1-dT2) < SM_EFF_ZERO)
        {
          return FALSE;  // no answer possible
        }
    }

  // Now test to see if segment 1 is outside of valid angle of segment 2
  if (pBezSpan2->m_dMaxTurningAngleDeg < 90.0)
    {
      double dAng1 = smgu_SegmentPointAngleDegToPerp(sStart2,sEnd2,sStart1,dT1);
      double dAng2 = smgu_SegmentPointAngleDegToPerp(sStart2,sEnd2,sEnd1,dT2);

      if (smos_Min(dAng1,dAng2) > pBezSpan2->m_dMaxTurningAngleDeg &&
          smos_Fabs(dT1-dT2) < SM_EFF_ZERO)
        {
          return FALSE;  // no answer possible
        }
    }

  // all done - there may be an answer here
  return TRUE;

} // end SmCCGlobalSolver::BranchMayContainAnswers
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 8444-8470: sm_GetProjSignedAngle
static double sm_GetProjSignedAngle   // rtn: angle from -PI to +PI
  (const SmPoint3d  & sPt1,              // in : EndPoint of vector sPt1-sCenter
   const SmPoint3d  & sPt2,              // in : EndPoint of vector sPt2-sCenter
   const SmPoint3d  & sCenter,           // in : plane center point
   const SmVector3d & sPlaneNormal,      // in : plane end Point
   const SmVector3d & sSideDirection)    // in : Forces an error when Vec2.Dot(sSideDirection) < 0.0
{
  // get and check Vec1 - nonZero
  SmVector3d sVec1 = sPt1 - sCenter;
  if (sVec1.LengthSquared() < SM_EFF_ZERO_SQ)
    { return SM_BIG_DOUBLE;
    }

  // get and check Vec2 - nonZero and pointing in direction of sSideDirection
  SmVector3d sVec2 = sPt2 - sCenter;
  if (   sVec2.LengthSquared() < SM_EFF_ZERO_SQ
      || sVec2.Dot(sSideDirection) < 0.0)
    {
      return SM_BIG_DOUBLE;
    }

  // get and return  CCW angle in radians
  double dTheta;
  SE(sPlaneNormal.CCWAngleBetween(sVec1,sVec2,dTheta));
  return dTheta;

} // end sm_GetProjSignedAngle
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 10841-10940: SmCPGlobalSolver (class, constructor, BranchMayContainAnswers)
class SmCPGlobalSolver : public SmGlobalSolver
{
public:
  const SmExtent1d & m_crInterval;    // target curve interval to query
  const SmContext  * m_cpContext;     // context to use for new objects

  // constructor
  SmCPGlobalSolver(const SmContext  & rContext,
                   SmTree           * pPointTree, 
                   SmTree           * pCurveTree,
                   const SmExtent1d & crInterval);

  // methods
  virtual SmBoolean BranchMayContainAnswers(SmTreeNode * apBranch[SM_GS_MAX_TREES]);
  virtual SmStatus  LocalSolve(SmTreeNode * apBranch[SM_GS_MAX_TREES],
                               SmBoolean & rbNeedsMoreSubdivision);

} ; // end class SmCPGlobalSolver

/*******************************************************************//***
PURPOSE: SmCPGlobalSolver object constructor

NOTES:
************************************************************************/
SmCPGlobalSolver::SmCPGlobalSolver
  (const SmContext & rContext,        // in : context for new object construction
   SmTree *pPointTree,                // in : target Point spatial tree
   SmTree *pCurveTree,                // in : target Curve spatial decomposition tree
   const SmExtent1d & crInterval)     // in : target interval for curve
: SmGlobalSolver(pPointTree,pCurveTree,1),
  m_crInterval(crInterval),
  m_cpContext(&rContext)
{

} // end SmCPGlobalSolver::SmCPGlobalSolver constructor

/*******************************************************************//***
PURPOSE:

NOTES:
************************************************************************/
SmBoolean SmCPGlobalSolver::BranchMayContainAnswers
  (SmTreeNode * apBranch[SM_GS_MAX_TREES])    // in : Point/Curve nodes to compare
{
  // Use general BranchMayContainAnswers() check for all cases but NORMALIZE
  if (m_eSolverOperation != SM_SO_NORMALIZE)
    {
      return SmGlobalSolver::BranchMayContainAnswers(apBranch);
    }

  // arrive here for NORMALIZE cases

  // locals
  SmTreeNode   *pPointNode = apBranch[0];
  SmTreeNode   *pCurveNode = apBranch[1];
  SmBezierSpan *pBezSpan   = (SmBezierSpan*)pCurveNode->m_pData; NER(pBezSpan);
  SmPoint3d     sTestPnt   = pPointNode->m_sBBox.GetMin();

  // no culling - When Curve node is not part of a curve decomposition tree
  if (   pCurveNode->m_eAuxDataType != SM_AD_BEZIER_CURVE
      && pCurveNode->m_eAuxDataType != SM_AD_CURVE_DATA)
    {
      return TRUE;
    }

  // no culling until BezSpan's turning angle gets down below 90 degrees
  if (pBezSpan->m_dMaxTurningAngleDeg > 90.0)
    {
      return TRUE;
    }

  // First check whether the given point is within tolerance of either end point.
  SmPoint3d sPt0, sPt1;
  SmCurveCache *pCurveCache = (SmCurveCache*)pCurveNode->m_pTree->GetOwner();
  SmExtent1d sIvl;
  pBezSpan->GetEnds( *pCurveCache, sPt0, sPt1, sIvl);

  if ( sTestPnt.CloserThan( m_d3dTolerance, sPt0 ) )
    {
      return TRUE;
    }
  if ( sTestPnt.CloserThan( m_d3dTolerance, sPt1 ) )
    {
      return TRUE;
    }

  // compute the angle from 90 degrees for the testPoint to the span chord segment
  double dT;
  double dAngDegFromPerp = smgu_SegmentPointAngleDegToPerp( sPt0, sPt1, sTestPnt, dT );

  // cull cases where no perp drop point is possible
  if (dAngDegFromPerp*0.99 - SM_EFF_ZERO > pBezSpan->m_dMaxTurningAngleDeg)
    {
      return FALSE;
    }

  // else return TRUE
  return TRUE;

} // end SmCPGlobalSolver::BranchMayContainAnswers
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 11959-12391: SmCurvePropertyGS (class, constructor, FindBestSubdivisionNode, BranchMayContainAnswers)
class SmCurvePropertyGS : public SmGlobalSolver
{
public:
    const SmExtent1d    & m_crInterval ;         // curve interval to examine
    SmCurvePropertyType   m_eCurveProperty ;     // curve property to compute
    const double        * m_cpdPropertyValue ;   //
    const SmVector3d    * m_cpOptVectors ;       //

    // constructor
    SmCurvePropertyGS(SmTree              * pCurveTree,
                      const SmExtent1d    & crInterval,
                      SmCurvePropertyType   eCurveProperty,
                      const double        * cpdPropertyValue,
                      const SmVector3d    * cpOptVectors) ;

    // virtual methods
    virtual SmBoolean BranchMayContainAnswers(SmTreeNode * apBranch[SM_GS_MAX_TREES]) ;

    virtual SmStatus  LocalSolve(SmTreeNode * apBranch[SM_GS_MAX_TREES],
                                 SmBoolean  & rbNeedsMoreSubdivision) ;

    virtual SmStatus  FindBestSubdivisionNode(SmTreeNode * apBranch[SM_GS_MAX_TREES],
                                              ULONG      & rlBestSubdivisionIndex,
                                              SmBoolean  & rbSwapTraversalOrder)
                                             const ;
} ; // end class SmCurvePropertyGS

/*******************************************************************//**
PURPOSE:

NOTES:
***********************************************************************/
SmCurvePropertyGS::SmCurvePropertyGS
  (SmTree              * pCurveTree,
   const SmExtent1d    & crInterval,
   SmCurvePropertyType   eCurveProperty,
   const double        * cpdPropertyValue,
   const SmVector3d    * cpOptVectors)
 : SmGlobalSolver    (pCurveTree,1),
   m_crInterval      (crInterval),
   m_eCurveProperty  (eCurveProperty),
   m_cpdPropertyValue(cpdPropertyValue),
   m_cpOptVectors    (cpOptVectors)
{

} // end SmCurvePropertyGS::SmCurvePropertyGS constructor

/*******************************************************************//**
PURPOSE:

NOTES:
***********************************************************************/
SmStatus SmCurvePropertyGS::FindBestSubdivisionNode
  (SmTreeNode * apBranch[SM_GS_MAX_TREES],
   ULONG      & rlBestSubdivisionIndex,
   SmBoolean  & rbSwapTraversalOrder)
  const
{
    if (m_eCurveProperty == SM_CP_MINIMIZE_ANGLE_TO_PLANE ||
        m_eCurveProperty == SM_CP_MINIMIZE_DIRECTED_ANGLE ||
        m_eCurveProperty == SM_CP_MINIMIZE_CCW_ANGLE ||
        m_eCurveProperty == SM_CP_PROJECTED_POINT_MINIMIZE ||
        m_eCurveProperty == SM_CP_PROJECTED_POINT_MAXIMIZE)
          {
        // The optimization for minimum angle takes the branch which has
        // the bounding box whose center is angularly closer to the plane.
        rlBestSubdivisionIndex = 0;
        rbSwapTraversalOrder = FALSE;
        SmTreeNode *pCurveNode = apBranch[0];
        SmTreeNode *pNode1 = pCurveNode->m_pChild1; NER(pNode1);
        SmTreeNode *pNode2 = pCurveNode->m_pChild2; NER(pNode2);
        SmExtent3d & rBBox1 = pNode1->m_sBBox;
        SmExtent3d & rBBox2 = pNode2->m_sBBox;
        SmPoint3d sSphCent1, sSphCent2;
        double dSphRad1, dSphRad2;
        rBBox1.ComputeSphereBound(sSphCent1,dSphRad1);
        rBBox2.ComputeSphereBound(sSphCent2,dSphRad2);
        if (m_eCurveProperty == SM_CP_MINIMIZE_ANGLE_TO_PLANE ||
            m_eCurveProperty == SM_CP_MINIMIZE_DIRECTED_ANGLE ||
            m_eCurveProperty == SM_CP_MINIMIZE_CCW_ANGLE)
          {
            SmVector3d sVec1 = sSphCent1 - m_cpOptVectors[0];
            // Project sVec1 to the projection plane
            sVec1 = m_cpOptVectors[1] * sVec1 * m_cpOptVectors[1];
            if (sVec1.LengthSquared() < SM_EFF_ZERO)
              { return SM_SUCCESS; }

            SER(sVec1.Unitize());
            double dCosTheta1;
            if (m_eCurveProperty == SM_CP_MINIMIZE_ANGLE_TO_PLANE)
              {
                dCosTheta1 = SM_PI/2.0-smos_ArcCosine(smos_Fabs(sVec1.Dot(m_cpOptVectors[2])));
              }
            else
              { // SM_CP_MINIMIZE_DIRECTED_ANGLE
                m_cpOptVectors[1].CCWAngleBetween(m_cpOptVectors[2],sVec1,dCosTheta1);
                if (dCosTheta1 > SM_PI) dCosTheta1 = 2.0*SM_PI - dCosTheta1;  // make range 0 to -180
                if (m_eCurveProperty == SM_CP_MINIMIZE_CCW_ANGLE)
                  {
                    if (dCosTheta1 < 0.0) dCosTheta1 = 2.0*SM_PI + dCosTheta1;
                  }
              }
            SmVector3d sVec2 = sSphCent2 - m_cpOptVectors[0];
            sVec2 = m_cpOptVectors[1] * sVec2 * m_cpOptVectors[1]; // Project vector to plane
            if (sVec2.LengthSquared() < SM_EFF_ZERO)
              { return SM_SUCCESS; }
            SER(sVec2.Unitize());
            double dCosTheta2;
            if (m_eCurveProperty == SM_CP_MINIMIZE_ANGLE_TO_PLANE)
              {
                dCosTheta2 = SM_PI/2.0-smos_ArcCosine(smos_Fabs(sVec2.Dot(m_cpOptVectors[2])));
              }
            else
              { // SM_CP_MINIMIZE_DIRECTED_ANGLE
                m_cpOptVectors[1].CCWAngleBetween(m_cpOptVectors[2],sVec2,dCosTheta2);
                if (dCosTheta2 > SM_PI) dCosTheta2 = 2.0*SM_PI - dCosTheta2;  // make range 0 to -180
                if (m_eCurveProperty == SM_CP_MINIMIZE_CCW_ANGLE)
                  {
                    if (dCosTheta2 < 0.0) dCosTheta2 = 2.0*SM_PI + dCosTheta2;
                  }
              }

            if (dCosTheta2 < dCosTheta1)
              {
                rbSwapTraversalOrder = TRUE;
              }
          }
        else
          {
            // Project points to the plane and see which is better branch to persue
            SER(smgu_PointProjectToPlane(sSphCent1,m_cpOptVectors[0],m_cpOptVectors[1],
                sSphCent1));
            SER(smgu_PointProjectToPlane(sSphCent2,m_cpOptVectors[0],m_cpOptVectors[1],
                sSphCent2));
            if (m_eCurveProperty == SM_CP_PROJECTED_POINT_MINIMIZE)
              {
                if (sSphCent1.DistanceBetweenSquared(m_cpOptVectors[0]) >
                    sSphCent2.DistanceBetweenSquared(m_cpOptVectors[0]) )
                  {
                    rbSwapTraversalOrder = TRUE;
                  }
              }
            else
              { // Maximize
                if (sSphCent1.DistanceBetweenSquared(m_cpOptVectors[0]) <
                    sSphCent2.DistanceBetweenSquared(m_cpOptVectors[0]) )
                  {
                    rbSwapTraversalOrder = TRUE;
                  }
              }
          }
      }
    else
      {
        SER(SmGlobalSolver::FindBestSubdivisionNode(apBranch,rlBestSubdivisionIndex,rbSwapTraversalOrder));
      }

    return SM_SUCCESS;

} // end SmCurvePropertyGS::FindBestSubdivisionNode

/*******************************************************************//**
PURPOSE: Cull checking cases that are known not to generate a solution

NOTES: The default is to always return TRUE, some cases return false as follows:
  case  SM_CP_PROJECTED_POINT_MINIMIZE, false when proj dist to bounding box is greater than current m_dBestAnswerSoFarSq
  case  SM_CP_PROJECTED_POINT_MAXIMIZE, false when proj dist to bounding box is closer than current m_dBestAnswerSoFarSq
  case  SM_CP_DISTANCE_TO_POINT,        false when min/max distances to bbox don't bracket the target distance
  cases SM_CP_MINIMIZE_ANGLE_TO_PLANE
        SM_CP_MINIMIZE_DIRECTED_ANGLE
        SM_CP_MINIMIZE_CCW_ANGLE        false when any of the bbox corner angles is less than current best answer
  case  SM_CP_CYLINDER_INTERSECTION     false when bbox does not intersect the cylinder radius
  case  SM_CP_PLANE_INTERSECTION        false when bbox and pseudo bbox does not intersect plane
  case  SM_CP_PERPENDICULAR_TO_VECTOR   false when chord angle to vector plus and minus turning angle does not include 90 degrees
  case  SM_CP_MAXIMA_TO_PLANE           false when chord angle to plane normal plus and minus turning angle does not include 90 degrees
 else return TRUE

 Performance is helped by any cheap culling conditions added here.  Missing a
 culling opportunity won't stop the system from getting the right solutions,
 but might slow it up.  Adding a false culling condition here will cause
 the system to miss solutions.
***********************************************************************/
SmBoolean SmCurvePropertyGS::BranchMayContainAnswers
  (SmTreeNode * apBranch[SM_GS_MAX_TREES])
{
  // locals
  SmTreeNode   * pCurveNode = apBranch[0];
  SmBezierSpan * pBezSpan   = (SmBezierSpan*)pCurveNode->m_pData; NER(pBezSpan);
  SmExtent3d   & rBBox      = pCurveNode->m_sBBox;


  // return false when target m_cpOptVectors[0] point is further from branch bounding box than current m_dBestAnswerSoFarSq
  if (m_eCurveProperty == SM_CP_PROJECTED_POINT_MINIMIZE)
    {
      double    dRadius;
      SmPoint3d sCenter;
      rBBox.ComputeSphereBound(sCenter,dRadius);
      SER(smgu_PointProjectToPlane(sCenter,m_cpOptVectors[0],m_cpOptVectors[1],
                                   sCenter));
      double dDist = m_cpOptVectors[0].DistanceBetween(sCenter) - dRadius - m_d3dTolerance;
      if (dDist < 0.0) dDist = 0.0;
      if (dDist*dDist > m_dBestAnswerSoFarSq)
        { return FALSE; }
      return TRUE;
    }

  // return false when when target m_cpOptVectors[0] point is closer to branch bounding box than current m_dBestAnswerSoFarSq
  else if (m_eCurveProperty == SM_CP_PROJECTED_POINT_MAXIMIZE)
    {
      double    dRadius;
      SmPoint3d sCenter;
      rBBox.ComputeSphereBound(sCenter,dRadius);
      SER(smgu_PointProjectToPlane(sCenter,m_cpOptVectors[0],m_cpOptVectors[1],
                                   sCenter));
      double dDist = m_cpOptVectors[0].DistanceBetween(sCenter) + dRadius + m_d3dTolerance;
      if (dDist*dDist < m_dBestAnswerSoFarSq)
        { return FALSE; }
      return TRUE;
    }

  // return false when min/max distances to bbox don't bracket the target distance
  else if (m_eCurveProperty == SM_CP_DISTANCE_TO_POINT)
    {
      SmExtent3d sPointBox( m_cpOptVectors[0] );
      double dRadSquared = (*m_cpdPropertyValue) * (*m_cpdPropertyValue);

      double dDistSq = sPointBox.MaximumDistanceSquared( rBBox );
      if ( dDistSq < dRadSquared - SM_EFF_ZERO )
        { return FALSE; }

      dDistSq = sPointBox.MinimumDistanceSquared( rBBox );
      if ( dDistSq > dRadSquared + SM_EFF_ZERO )
        { return FALSE; }

      // Also, if curve box contains point:
      if ( dDistSq < SM_EFF_ZERO_SQ )
        {
          double dSizeSq = rBBox.GetSize().LengthSquared();
          if ( dSizeSq < dRadSquared - SM_EFF_ZERO )
            { return FALSE; }
        }
      return TRUE;
    }

  // return false when any of the bbox corner angles is less than current best answer
  else if (m_eCurveProperty == SM_CP_MINIMIZE_ANGLE_TO_PLANE ||
           m_eCurveProperty == SM_CP_MINIMIZE_DIRECTED_ANGLE ||
           m_eCurveProperty == SM_CP_MINIMIZE_CCW_ANGLE)
    {
      // See if any of the bbox corner points have a smaller angle
      // than the one we already have.
      ULONG i, j, k;
      for (i=0; i<2; i++)
        {
          double dXV = 1.0 * i;
          for (j=0; j<2; j++)
            {
              double dYV = 1.0 * j;
              for (k=0; k<2; k++)
                {
                  double     dZV = 1.0 * j;
                  SmPoint3d  sTestPnt = rBBox.Evaluate(dXV,dYV,dZV);
                  SmVector3d sVec = sTestPnt - m_cpOptVectors[0];
                  sVec = m_cpOptVectors[1] * sVec * m_cpOptVectors[1];
                  if (sVec.LengthSquared() < SM_EFF_ZERO)
                    { return TRUE;   }
                  SER(sVec.Unitize());
                  double dCosTheta;
                  if (m_eCurveProperty == SM_CP_MINIMIZE_ANGLE_TO_PLANE)
                    {
                      dCosTheta = SM_PI/2.0-smos_ArcCosine(smos_Fabs(sVec.Dot(m_cpOptVectors[2])));
                    }
                  else
                    { // SM_CP_MINIMIZE_DIRECTED_ANGLE
                      m_cpOptVectors[1].CCWAngleBetween(m_cpOptVectors[2],sVec,dCosTheta);
                      if (dCosTheta > SM_PI) dCosTheta = 2.0*SM_PI - dCosTheta;  // make range 0 to -180
                      if (m_eCurveProperty == SM_CP_MINIMIZE_CCW_ANGLE)
                        {
                          if (dCosTheta < 0.0) dCosTheta = 2.0*SM_PI + dCosTheta;
                        }
                    }

                  if ((dCosTheta*smos_Fabs(dCosTheta))+SM_EFF_ZERO_SQRT < m_dBestAnswerSoFarSq)
                    { return TRUE; }
                }
            }
        }

      // If all of points pass the test then we are done with this branch
      return FALSE;
    }

  // return false when bbox does not intersect the cylinder radius
  else if (m_eCurveProperty == SM_CP_CYLINDER_INTERSECTION)
    {
      double dCylRadius = *m_cpdPropertyValue;
      double dSphRadius;
      SmPoint3d sCenter;
      rBBox.ComputeSphereBound(sCenter,dSphRadius);

      SmVector3d sVec = m_cpOptVectors[1]*(sCenter-m_cpOptVectors[0])*m_cpOptVectors[1];

      if (smos_Fabs(sVec.Length() - dCylRadius) - m_d3dTolerance > dSphRadius)
        { return FALSE;   }
      return TRUE;
    }

  // return false when bbox and pseudo bbox does not intersect plane
  else if (m_eCurveProperty == SM_CP_PLANE_INTERSECTION)
    {
      SmBoolean bRet = FALSE;
      SM_ASSERT(m_cpdPropertyValue && m_cpOptVectors);
      const SmVector3d & sABC = *m_cpOptVectors;
      double dD = *m_cpdPropertyValue;
      double dRadius;
      SmPoint3d sCenter;
      rBBox.ComputeSphereBound(sCenter,dRadius);
      double dDist = sCenter.Dot(sABC) + dD;

      if (smos_Fabs(dDist) > dRadius + m_d3dTolerance)
        { return FALSE; }
      if (smos_Fabs(dDist) < m_d3dTolerance)
        { bRet = TRUE; }

      // Here we'll test corners then other points to get a tighter test
      SmPoint3d sCorner ;
      double dDist2 = sABC.Dot(rBBox.GetMax()) + dD;
      if (dDist * dDist2 < 0.0 || smos_Fabs(dDist2) < m_d3dTolerance) bRet = TRUE;
      if (!bRet) { dDist2 = sABC.Dot(rBBox.GetMin()) + dD;
                   if (dDist * dDist2 < 0.0 || smos_Fabs(dDist2) < m_d3dTolerance) bRet = TRUE; }
      if (!bRet) { sCorner = rBBox.Evaluate(1,0,0);
                   dDist2 = sABC.Dot(sCorner) + dD;
                   if (dDist * dDist2 < 0.0 || smos_Fabs(dDist2) < m_d3dTolerance) bRet = TRUE; }
      if (!bRet) { sCorner = rBBox.Evaluate(1,1,0);
                   dDist2 = sABC.Dot(sCorner) + dD;
                   if (dDist * dDist2 < 0.0 || smos_Fabs(dDist2) < m_d3dTolerance) bRet = TRUE; }
      if (!bRet) { sCorner = rBBox.Evaluate(1,0,1);
                   dDist2 = sABC.Dot(sCorner) + dD;
                   if (dDist * dDist2 < 0.0 || smos_Fabs(dDist2) < m_d3dTolerance) bRet = TRUE; }
      if (!bRet) { sCorner = rBBox.Evaluate(0,1,0);
                   dDist2 = sABC.Dot(sCorner) + dD;
                   if (dDist * dDist2 < 0.0 || smos_Fabs(dDist2) < m_d3dTolerance) bRet = TRUE; }
      if (!bRet) { sCorner = rBBox.Evaluate(0,1,1);
                   dDist2 = sABC.Dot(sCorner) + dD;
                   if (dDist * dDist2 < 0.0 || smos_Fabs(dDist2) < m_d3dTolerance) bRet = TRUE; }
      if (!bRet) { sCorner = rBBox.Evaluate(0,0,1);
                   dDist2 = sABC.Dot(sCorner) + dD;
                   if (dDist * dDist2 < 0.0 || smos_Fabs(dDist2) < m_d3dTolerance) bRet = TRUE; }

      // See if passed all of box tests and bounding box is completely on
      // one side of plane.
      if (bRet == FALSE)
        { return bRet;   }

      // Let's try the tighter pseudo box test
      if (pCurveNode->m_eAuxDataType == SM_AD_BEZIER_CURVE)
        {
          // Try tight pseudo box test
          SmBezierSpan *pSpan = (SmBezierSpan*)pCurveNode->m_pData; NER(pSpan);
          SmPoint3d sCorners[8];
          pSpan->GetPseudoBox().CalcCorners(sCorners);
          dDist = sABC.Dot(sCorners[0]) + dD;
          if (smos_Fabs(dDist) < m_d3dTolerance)
            { return TRUE;   }
          ULONG j;
          for (j=1; j<8; j++)
            {
              dDist2 = sABC.Dot(sCorners[j]) + dD;
              if (dDist * dDist2 < 0.0 || smos_Fabs(dDist2) < m_d3dTolerance)
                { return TRUE;   }
            }
          return FALSE;
        }

      return TRUE;
    } // end if (m_eCurveProperty == SM_CP_PLANE_INTERSECTION)


  // Make sure that the nodes have data in them
  if (   pCurveNode->m_eAuxDataType != SM_AD_BEZIER_CURVE
      && pCurveNode->m_eAuxDataType != SM_AD_CURVE_DATA)
    { return TRUE; }

  // Don't try to make the call too soon - wait for angle
  // to get down below 90 degrees
  if (pBezSpan->m_dMaxTurningAngleDeg > 90.0)
    { return TRUE; }

  // return false when turning angle is not near 90 degrees
  if (   m_eCurveProperty == SM_CP_PERPENDICULAR_TO_VECTOR
      || m_eCurveProperty == SM_CP_MAXIMA_TO_PLANE)
    {
      SmPoint3d      sLinePt, sEnd;
      SmCurveCache * pCurveCache = (SmCurveCache*)pCurveNode->m_pTree->GetOwner();
      SmExtent1d     sIvl;

      pBezSpan->GetEnds(*pCurveCache,sLinePt,sEnd,sIvl);
      SmVector3d sLineVec = sEnd - sLinePt;

      // no solutions for degenerate spans
      if(sLineVec.LengthSquared() < SM_EFF_ZERO_SQ)
        { return FALSE ; } 

      SM_ASSERT(m_cpOptVectors != NULL);
      double dAngle;
      if(m_eCurveProperty == SM_CP_PERPENDICULAR_TO_VECTOR) { if(m_cpOptVectors[0].LengthSquared() < SM_EFF_ZERO_SQ)
                                                                { return FALSE ; }
                                                              SER(sLineVec.AngleBetween(m_cpOptVectors[0],dAngle)); 
                                                            }
      else                                                  { if(m_cpOptVectors[1].LengthSquared() < SM_EFF_ZERO_SQ)
                                                                { return FALSE ; }
                                                              SER(sLineVec.AngleBetween(m_cpOptVectors[1],dAngle)); 
                                                            }

      // If the angle is not near 90 degrees then we can return a FALSE
      if (dAngle + pBezSpan->m_dMaxTurningAngleDeg < SM_PI/2.0 ||
          dAngle - pBezSpan->m_dMaxTurningAngleDeg > SM_PI/2.0)
        {
          return FALSE;
        }
    }


//    double dT;
//    double dAngDegFromPerp =
//        smgu_SegmentPointAngleDegToPerp(sLinePt,sEnd,sTestPnt,dT);
//    if (dAngDegFromPerp*0.99 - SM_EFF_ZERO > pBezSpan->m_dMaxTurningAngleDeg/2.0) {
//        return FALSE;
//    }

  return TRUE;

} // end SmCurvePropertyGS::BranchMayContainAnswers
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 14568-14601: SmLengthIFEO
class SmLengthIFEO : public SmIntegFuncEvalObj
{
private:
    const SmBSplineCurve & m_crCurve;

public:
    // constructor
    SmLengthIFEO(const SmBSplineCurve & crCurve )
               : m_crCurve(crCurve)
               { }

    // evaluator
    virtual SmStatus Evaluate(double dT, double & rdResult) const;

} ; // end class SmLengthIFEO

/*******************************************************************//**
PURPOSE:

NOTES:
***********************************************************************/
SmStatus SmLengthIFEO::Evaluate
  (double dT,
   double & rdResult)
  const
{
  SmPoint3d sPV[2];
  SER(m_crCurve.Evaluate(dT,1,TRUE,sPV,FALSE));  // exact Tangent values

  // The function being integrated is the first derivative of the curve
  rdResult = sPV[1].Length();
  return SM_SUCCESS;

} // end SmLengthIFEO::Evaluate
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 21032-21099: sm_FindTestParams
static void sm_FindTestParams
  (double                   dParamStart, // in :
   double                   dParamEnd,   // in :
   ULONG                    lDeg,        // NotUsed: in :
   const SmTArray<double> & crKnots,     // out:
   SmTArray<double>       & rParams)     // out:
{
  SM_REF1(lDeg) ;
    rParams.ReSet();
    if (crKnots.GetSize() == 0) {
        rParams.Add(dParamStart);
        rParams.Add(dParamEnd);
        return;
    }
    ULONG lStart = 0;
    while (crKnots[lStart] < dParamStart-SM_EFF_ZERO &&
        lStart < crKnots.GetSize()-1) {
        lStart++;
    }
    ULONG lEnd = crKnots.GetSize()-1;
    while (lEnd > 0 && crKnots[lEnd]-SM_EFF_ZERO > dParamEnd) {
        lEnd --;
    }

    // If same interval test 3 middle points of interval and return
    if (lStart >= lEnd) {
        rParams.Add(dParamStart + 0.25 * (dParamEnd-dParamStart));
        rParams.Add(dParamStart + 0.50 * (dParamEnd-dParamStart));
        rParams.Add(dParamStart + 0.75 * (dParamEnd-dParamStart));
        return;
    }

    // Add 3 start points first, if there's a gap between dParamStart and first knot.
    double dDelta = crKnots[lStart] - dParamStart;
    if ( dDelta > SM_EFF_ZERO_SQRT )
      {
        rParams.Add(dParamStart + 0.25 * dDelta );
        rParams.Add(dParamStart + 0.50 * dDelta );
        rParams.Add(dParamStart + 0.75 * dDelta );
      }
    else
      {
        rParams.Add( dParamStart );
      }

    // Add 3 points between each knot pair.
    ULONG k;
    for (k=lStart+1; k<=lEnd; k++) {
        dDelta = crKnots[k] -  crKnots[k-1];
        rParams.Add(crKnots[k-1] + 0.25 * dDelta );
        rParams.Add(crKnots[k-1] + 0.50 * dDelta );
        rParams.Add(crKnots[k-1] + 0.75 * dDelta );
    }

    // Add 3 points at end if there's a gap.
    dDelta = dParamEnd - crKnots[lEnd];
    if ( dDelta > SM_EFF_ZERO_SQRT )
      {
        rParams.Add(crKnots[lEnd] + 0.25 * dDelta );
        rParams.Add(crKnots[lEnd] + 0.50 * dDelta );
        rParams.Add(crKnots[lEnd] + 0.75 * dDelta );
      }
    else
      {
        rParams.Add( dParamEnd );
      }

} // end sm_FindTestParams
// ---- end verbatim

// clang-format on

SmStatus SmCCGlobalSolver::LocalSolve(SmTreeNode**, SmBoolean&) { return SM_ERR; }
SmStatus SmCPGlobalSolver::LocalSolve(SmTreeNode**, SmBoolean&) { return SM_ERR; }
SmStatus SmCurvePropertyGS::LocalSolve(SmTreeNode**, SmBoolean&) { return SM_ERR; }

// Exposes the protected solver state the tests set.
template <class S> struct Open : S
{
    using S::S;
    void set(SmSolverOperationType op, double tol, double best, const SmVector3d* opt)
    {
        this->m_eSolverOperation = op;
        this->m_d3dTolerance = tol;
        this->m_dBestAnswerSoFarSq = best;
        this->SmGlobalSolver::m_cpOptVectors = opt;
    }
};
} // namespace sc_crv_p2

namespace
{
using namespace sc_crv_p2;

// Ops (keep in sync with crates/solidcore-ffi/src/units/curve_curve_p2.rs).
enum : int32_t
{
    OP_INFO = 0,
    OP_EVAL = 1,
    OP_EVAL_POINT = 2,
    OP_ENDS = 3,
    OP_KNOTS = 4,
    OP_CONTINUITIES = 5,
    OP_APPROX_LENGTH = 6,
    OP_EVAL_STEP = 7,
    OP_EVAL_GEOM = 8,
    OP_EVAL_CONTINUITY = 9,
    OP_STEP_OFF = 10,
    OP_SPLIT = 11,
    OP_CONVERT_T = 12,
    OP_REPEATED = 13,
    OP_EQUAL = 14,
    OP_COPY = 15,
    OP_TRIM = 16,
    OP_TRANSFORM = 17,
    OP_EDIT_PARAM = 18,
    OP_REVERSE = 19,
    OP_CONVERT_2D = 20,
    OP_ANALYTIC = 21,
    OP_STEP_INVERSION = 22,
    OP_DROP_FAST = 23,
    OP_INTERSECT_LINE = 24,
    OP_SNAP = 25,
    OP_LENGTH_IFEO = 26,
    // static ops (no curve block)
    OP_READ_DB = 40,
    OP_FLUSH = 41,
    OP_ADD_TO_CURVE = 42,
    OP_STACK = 43,
    OP_PARAM_DELTA = 44,
    OP_UNITIZE_TAN = 45,
    OP_PROJ_ANGLE = 46,
    OP_TEST_PARAMS = 47,
    OP_TEST_STEP = 48,
    OP_PROPERTY_GS = 49,
    OP_CC_GS = 50,
    OP_CP_GS = 51,
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

// A curve block (see the file comment).
SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    ULONG dim = (ULONG)rd.idx();
    if (kind == 0)
    {
        bool oob = rd.flag();
        SmBSplineCurve* c = new (ctx) SmBSplineCurve(read_nurb(rd), dim, FALSE);
        c->SetOutOfBoundsEnabled(oob ? TRUE : FALSE);
        return c;
    }
    SmVector3d p1 = rd.vec();
    SmVector3d d1 = rd.vec();
    SmVector3d p2 = rd.vec();
    SmVector3d d2 = rd.vec();
    return new (ctx) SmHermiteCurve(p1, d1, p2, d2, dim);
}

// State of a curve: 0 dim n p m cpts knots (B-spline) or 1 dim P1 D1 P2 D2 (Hermite);
// -1 for another type.
void put_state(std::vector<double>& o, const SmCurve* c)
{
    if (c->IsKindOf(SmBSplineCurve_TYPE))
    {
        const SmBSplineCurve* b = (const SmBSplineCurve*)c;
        o.push_back(0.0);
        o.push_back((double)b->GetDim());
        const gw_CURVE* cur = b->GetGwNurbPointer();
        if (cur == NULL)
        {
            o.push_back(-1.0);
            return;
        }
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
    else if (c->IsKindOf(SmHermiteCurve_TYPE))
    {
        // the defining data evaluated back: position and derivative at both ends
        SmVector3d a[2], b[2];
        c->Evaluate(0.0, 1, TRUE, a, FALSE);
        c->Evaluate(1.0, 1, TRUE, b, FALSE);
        o.push_back(1.0);
        o.push_back((double)c->GetDim());
        put_vec(o, a[0]);
        put_vec(o, a[1]);
        put_vec(o, b[0]);
        put_vec(o, b[1]);
    }
    else if (c->IsKindOf(SmConic_TYPE))
    {
        // the defining data evaluated back: position and tangent at 0 and 0.5
        o.push_back(2.0);
        o.push_back((double)c->GetDim());
        const double ts[2] = {0.0, 0.5};
        for (double t : ts)
        {
            SmVector3d v[2];
            if (c->Evaluate(t, 1, TRUE, v, FALSE) == SM_SUCCESS)
            {
                put_vec(o, v[0]);
                put_vec(o, v[1]);
            }
            else
                o.push_back(-1.0);
        }
    }
    else
        o.push_back(-1.0);
}

void put_evals(std::vector<double>& o, const SmVector3d* v, ULONG n)
{
    for (ULONG i = 0; i < n; i++)
        put_vec(o, v[i]);
}

// A hand-built tree: `nn` nodes `parent c1 c2 aux has_data min[3] max[3]` (node 0 is the top).
class ScTree : public SmTree
{
public:
    ScTree() : SmTree((SmObject*)nullptr) {}
    void set_top(SmTreeNode* n) { m_pTopNode = n; }
};

struct Tree
{
    std::unique_ptr<ScTree> tree;
    std::vector<SmTreeNode> nodes;
};

// Never dereferenced: the tests avoid every path that reads span data.
alignas(16) char g_dummy_data[64];

bool read_tree(Rd& rd, Tree& t)
{
    int64_t nn = rd.idx();
    if (nn < 1)
        return false;
    t.tree.reset(new ((const SmContext*)nullptr) ScTree());
    t.nodes.resize((size_t)nn);
    for (int64_t k = 0; k < nn; k++)
    {
        int64_t par = rd.idx(), c1 = rd.idx(), c2 = rd.idx();
        int aux = (int)rd.idx();
        bool data = rd.flag();
        double v[6];
        for (int j = 0; j < 6; j++)
            v[j] = rd.num();
        SmTreeNode& nd = t.nodes[(size_t)k];
        nd.m_pTree = t.tree.get();
        nd.m_pParent = par < 0 ? nullptr : &t.nodes[(size_t)par];
        nd.m_pChild1 = c1 < 0 ? nullptr : &t.nodes[(size_t)c1];
        nd.m_pChild2 = c2 < 0 ? nullptr : &t.nodes[(size_t)c2];
        nd.m_eAuxDataType = (SmAuxDataType)aux;
        nd.m_pData = data ? (SmTreeNodeData*)(void*)g_dummy_data : nullptr;
        nd.m_sBBox = SmExtent3d(v[0], v[1], v[2], v[3], v[4], v[5]);
    }
    t.tree->set_top(&t.nodes[0]);
    return true;
}

void static_op(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_READ_DB:
    {
        SM_TYPE type = (SM_TYPE)rd.idx();
        ULONG dim = (ULONG)rd.idx();
        ULONG version = (ULONG)rd.idx();
        int64_t nb = rd.idx();
        std::string bytes;
        for (int64_t i = 0; i < nb; i++)
            bytes.push_back((char)(unsigned char)rd.idx());
        std::istringstream iss(bytes);
        SmDatabaseIOStream db(SM_ASCII, &iss);
        SmCurve* c = NULL;
        SmStatus s = SmCurve::ReadFromDB(type, db, dim, ctx, c, version);
        o.push_back(st(s));
        if (s == SM_SUCCESS && c != NULL)
        {
            o.push_back(1.0);
            put_state(o, c);
        }
        else
            o.push_back(0.0);
        delete c;
        break;
    }
    case OP_FLUSH:
    {
        double tol = rd.num();
        bool analytics = rd.flag();
        int64_t nk = rd.idx();
        SmTArray<double> knots;
        for (int64_t i = 0; i < nk; i++)
            knots.Add(rd.num());
        int64_t np = rd.idx();
        SmTArray<SmPoint3d> poly;
        for (int64_t i = 0; i < np; i++)
            poly.Add(rd.vec());
        SmBSplineCurve* c = NULL;
        SmStatus s = sm_FlushCrv(ctx, tol, poly, knots, c, analytics ? TRUE : FALSE);
        o.push_back(st(s));
        o.push_back(c != NULL ? 1.0 : 0.0);
        if (c != NULL)
            put_state(o, c);
        o.push_back((double)knots.GetSize());
        for (ULONG i = 0; i < knots.GetSize(); i++)
            o.push_back(knots[i]);
        o.push_back((double)poly.GetSize());
        for (ULONG i = 0; i < poly.GetSize(); i++)
            put_vec(o, poly[i]);
        delete c;
        break;
    }
    case OP_ADD_TO_CURVE:
    {
        SmPoint3d last[2], curr[2];
        last[0] = rd.vec();
        last[1] = rd.vec();
        curr[0] = rd.vec();
        curr[1] = rd.vec();
        double lt = rd.num();
        double ct = rd.num();
        int64_t nk = rd.idx();
        SmTArray<double> knots;
        for (int64_t i = 0; i < nk; i++)
            knots.Add(rd.num());
        int64_t np = rd.idx();
        SmTArray<SmPoint3d> poly;
        for (int64_t i = 0; i < np; i++)
            poly.Add(rd.vec());
        SmStatus s = sm_AddToCurve(last, curr, lt, ct, poly, knots);
        o.push_back(st(s));
        o.push_back((double)knots.GetSize());
        for (ULONG i = 0; i < knots.GetSize(); i++)
            o.push_back(knots[i]);
        o.push_back((double)poly.GetSize());
        for (ULONG i = 0; i < poly.GetSize(); i++)
            put_vec(o, poly[i]);
        break;
    }
    case OP_STACK:
    {
        // `ns` initial values, then `nops` ops: 0 push t P T, 1 pop, 2 look
        int64_t ns = rd.idx();
        SmTArray<double> stack;
        for (int64_t i = 0; i < ns; i++)
            stack.Add(rd.num());
        int64_t nops = rd.idx();
        o.push_back(0.0);
        for (int64_t k = 0; k < nops; k++)
        {
            int64_t code = rd.idx();
            if (code == 0)
            {
                double t = rd.num();
                SmPoint3d pv[2];
                pv[0] = rd.vec();
                pv[1] = rd.vec();
                o.push_back(st(sm_PushPoint(t, pv, stack)));
            }
            else
            {
                double t = SM_UNDEF_DOUBLE;
                SmPoint3d pv[2];
                SmStatus s = code == 1 ? sm_PopStack(t, pv, stack) : sm_StackLook(t, pv, stack);
                o.push_back(st(s));
                if (s == SM_SUCCESS)
                {
                    o.push_back(t);
                    put_vec(o, pv[0]);
                    put_vec(o, pv[1]);
                }
            }
            o.push_back((double)stack.GetSize());
        }
        break;
    }
    case OP_PARAM_DELTA:
    {
        SmVector3d t = rd.vec();
        SmVector3d d = rd.vec();
        o.push_back(0.0);
        o.push_back(sm_ConvertToParameterDelta(t, d));
        break;
    }
    case OP_UNITIZE_TAN:
    {
        SmVector3d pvv[3];
        pvv[0] = rd.vec();
        pvv[1] = rd.vec();
        pvv[2] = rd.vec();
        SmVector3d gap = rd.vec();
        SmVector3d u(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        double len = SM_UNDEF_DOUBLE;
        SmStatus s = sm_UnitizeTangent(pvv, gap, u, &len);
        o.push_back(st(s));
        put_vec(o, u);
        o.push_back(len);
        break;
    }
    case OP_PROJ_ANGLE:
    {
        SmPoint3d p1 = rd.vec();
        SmPoint3d p2 = rd.vec();
        SmPoint3d c = rd.vec();
        SmVector3d n = rd.vec();
        SmVector3d side = rd.vec();
        o.push_back(0.0);
        o.push_back(sm_GetProjSignedAngle(p1, p2, c, n, side));
        break;
    }
    case OP_TEST_PARAMS:
    {
        double a = rd.num();
        double b = rd.num();
        ULONG deg = (ULONG)rd.idx();
        int64_t nk = rd.idx();
        SmTArray<double> knots;
        for (int64_t i = 0; i < nk; i++)
            knots.Add(rd.num());
        SmTArray<double> params;
        sm_FindTestParams(a, b, deg, knots, params);
        o.push_back(0.0);
        o.push_back((double)params.GetSize());
        for (ULONG i = 0; i < params.GetSize(); i++)
            o.push_back(params[i]);
        break;
    }
    case OP_TEST_STEP:
    {
        double cur_t = rd.num();
        double step = rd.num();
        SmVector3d cur[3], next[3];
        cur[0] = rd.vec();
        cur[1] = rd.vec();
        next[0] = rd.vec();
        next[1] = rd.vec();
        double chord = rd.num();
        double ang = rd.num();
        double dsq = rd.num();
        SmTessStepTestResultType r = SM_ST_NOT_TESTED;
        SmStatus s = sm_TestStep(cur_t, step, cur, next, chord, ang, dsq, r);
        o.push_back(st(s));
        o.push_back((double)(int)r);
        break;
    }
    case OP_PROPERTY_GS:
    case OP_CC_GS:
    case OP_CP_GS:
    {
        // trees (1 for the property solver, 2 otherwise), then
        // `op tol best nopt opt[3 nopt]`, for the property solver `prop has_val val`,
        // then `which` (0 = BranchMayContainAnswers, 1 = FindBestSubdivisionNode) and the
        // branch node indices (one per tree).
        int ntrees = op == OP_PROPERTY_GS ? 1 : 2;
        std::vector<Tree> trees((size_t)ntrees);
        for (int i = 0; i < ntrees; i++)
            if (!read_tree(rd, trees[(size_t)i]))
            {
                o.push_back((double)SC_ERR_ARG);
                return;
            }
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        double tol = rd.num();
        double best = rd.num();
        int64_t nopt = rd.idx();
        SmVector3d opt[4];
        for (int64_t i = 0; i < nopt; i++)
            opt[i] = rd.vec();
        SmCurvePropertyType prop = SM_CP_PERPENDICULAR_TO_VECTOR;
        bool has_val = false;
        double val = 0.0;
        if (op == OP_PROPERTY_GS)
        {
            prop = (SmCurvePropertyType)rd.idx();
            has_val = rd.flag();
            val = rd.num();
        }
        int64_t which = rd.idx();
        SmTreeNode* br[SM_GS_MAX_TREES] = {nullptr, nullptr, nullptr};
        for (int i = 0; i < ntrees; i++)
            br[i] = &trees[(size_t)i].nodes[(size_t)rd.idx()];
        SmExtent1d ivl(0.0, 1.0);
        SmBoolean may = FALSE;
        SmStatus bs = SM_SUCCESS;
        ULONG idx = 99;
        SmBoolean swap = FALSE;
        if (op == OP_PROPERTY_GS)
        {
            Open<SmCurvePropertyGS> s(trees[0].tree.get(), ivl, prop, has_val ? &val : nullptr,
                                      nopt ? opt : nullptr);
            s.set(sop, tol, best, nullptr);
            if (which == 0)
                may = s.BranchMayContainAnswers(br);
            else
                bs = s.FindBestSubdivisionNode(br, idx, swap);
        }
        else if (op == OP_CC_GS)
        {
            Open<SmCCGlobalSolver> s(trees[0].tree.get(), trees[1].tree.get(), ivl, ivl);
            s.set(sop, tol, best, nopt ? opt : nullptr);
            may = s.BranchMayContainAnswers(br);
        }
        else
        {
            Open<SmCPGlobalSolver> s(ctx, trees[0].tree.get(), trees[1].tree.get(), ivl);
            s.set(sop, tol, best, nopt ? opt : nullptr);
            may = s.BranchMayContainAnswers(br);
        }
        if (which == 0)
        {
            o.push_back(0.0);
            o.push_back(may ? 1.0 : 0.0);
        }
        else
        {
            o.push_back(st(bs));
            if (bs == SM_SUCCESS)
            {
                o.push_back((double)idx);
                o.push_back(flg(swap));
            }
        }
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}

void curve_op(int32_t op, Rd& rd, const SmContext& ctx, SmCurve* c, std::vector<double>& o)
{
    switch (op)
    {
    case OP_INFO:
    {
        SmExtent1d ivl = c->GetNaturalInterval();
        o.push_back(0.0);
        o.push_back((double)c->GetType());
        o.push_back((double)c->GetDim());
        o.push_back((double)c->GetDegree());
        o.push_back((double)c->GetNumberNaturalKnots());
        o.push_back((double)c->GetNumberControlPoints());
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
        o.push_back(flg(c->IsAnalytic()));
        o.push_back(flg(c->IsBounded()));
        o.push_back(flg(c->IsRational()));
        o.push_back(c->GetOwner() == NULL ? 1.0 : 0.0);
        const SM_TYPE kinds[] = {SmCurve_TYPE,         12060,          SmBSplineCurve_TYPE,
                                 SmHermiteCurve_TYPE,  SmLine_TYPE,    SmEllipse_TYPE,
                                 16016,                12000,          1};
        for (SM_TYPE k : kinds)
            o.push_back(flg(c->IsKindOf(k)));
        break;
    }
    case OP_EVAL:
    {
        double t = rd.num();
        ULONG n = (ULONG)rd.idx();
        bool left = rd.flag();
        bool nz = rd.flag();
        SmVector3d v[8];
        SmStatus s = c->Evaluate(t, n, left ? TRUE : FALSE, v, nz ? TRUE : FALSE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_evals(o, v, n + 1);
        break;
    }
    case OP_EVAL_POINT:
    {
        SmPoint3d p;
        SmStatus s = c->EvaluatePoint(rd.num(), p);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, p);
        break;
    }
    case OP_ENDS:
    {
        SmPoint3d a, b;
        double wa = SM_UNDEF_DOUBLE, wb = SM_UNDEF_DOUBLE;
        c->GetEnds(a, b, &wa, &wb);
        o.push_back(0.0);
        put_vec(o, a);
        put_vec(o, b);
        o.push_back(wa);
        o.push_back(wb);
        break;
    }
    case OP_KNOTS:
    {
        bool has = rd.flag();
        double a = rd.num();
        double b = rd.num();
        SmExtent1d ivl(a, b);
        SmTArray<double> k;
        SmTArray<ULONG> m;
        SmStatus s = c->GetKnots(k, &m, has ? &ivl : NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)k.GetSize());
            for (ULONG i = 0; i < k.GetSize(); i++)
                o.push_back(k[i]);
            o.push_back((double)m.GetSize());
            for (ULONG i = 0; i < m.GetSize(); i++)
                o.push_back((double)m[i]);
        }
        break;
    }
    case OP_CONTINUITIES:
    {
        double ang = rd.num();
        SmContinuityType mn = SM_CT_UNDEFINED;
        SmTArray<SmContinuityType> all;
        SmStatus s = c->CalculateContinuities(mn, all, ang);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)(int)mn);
            o.push_back((double)all.GetSize());
            for (ULONG i = 0; i < all.GetSize(); i++)
                o.push_back((double)(int)all[i]);
        }
        break;
    }
    case OP_APPROX_LENGTH:
    {
        double a = rd.num();
        double b = rd.num();
        ULONG n = (ULONG)rd.idx();
        bool want = rd.flag();
        double turn = SM_UNDEF_DOUBLE;
        double len = c->ApproximateLength(SmExtent1d(a, b), n, want ? &turn : NULL);
        o.push_back(0.0);
        o.push_back(len);
        bool has_turn = want && c->GetDim() == 2;
        o.push_back(has_turn ? 1.0 : 0.0);
        if (has_turn)
            o.push_back(turn);
        break;
    }
    case OP_EVAL_STEP:
    {
        double t = rd.num();
        ULONG n = (ULONG)rd.idx();
        bool left = rd.flag();
        SmVector3d v[8];
        SmStatus s = c->EvaluateSTEP(t, n, left ? TRUE : FALSE, v);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_evals(o, v, n + 1);
        break;
    }
    case OP_EVAL_GEOM:
    {
        double t = rd.num();
        ULONG n = (ULONG)rd.idx();
        bool left = rd.flag();
        SmVector3d v[4];
        SmStatus s = c->EvaluateGeometric(t, n, left ? TRUE : FALSE, v);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_evals(o, v, n + 1);
        break;
    }
    case OP_EVAL_CONTINUITY:
    {
        double t = rd.num();
        bool left = rd.flag();
        SmCurve* other = read_curve(rd, ctx);
        double t2 = rd.num();
        bool left2 = rd.flag();
        double ang = rd.num();
        SmContinuityType ct = SM_CT_UNDEFINED;
        SmVector3d a[4], b[4];
        SmStatus s = c->EvaluateContinuity(t, left ? TRUE : FALSE, *other, t2, left2 ? TRUE : FALSE,
                                           ct, a, b, ang);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)(int)ct);
            put_evals(o, a, 3);
            put_evals(o, b, 3);
        }
        delete other;
        break;
    }
    case OP_STEP_OFF:
    {
        double a = rd.num();
        double b = rd.num();
        double start = rd.num();
        bool same = rd.flag();
        double dist = rd.num();
        double tol = rd.num();
        SmBoolean clamped = FALSE;
        double end = SM_UNDEF_DOUBLE;
        SmStatus s = c->EuclidianStepOff(SmExtent1d(a, b), start, same ? SM_OT_SAME : SM_OT_OPPOSITE,
                                         dist, tol, clamped, end);
        o.push_back(st(s));
        if (s == SM_SUCCESS || s == SM_ERR)
        {
            o.push_back(flg(clamped));
            o.push_back(end);
        }
        break;
    }
    case OP_SPLIT:
    {
        SmCurve* nc = NULL;
        SmStatus s = c->CreateBySplitCurve(ctx, rd.num(), nc);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            put_state(o, c);
            put_state(o, nc);
        }
        delete nc;
        break;
    }
    case OP_CONVERT_T:
    {
        double t = rd.num();
        double a = SM_UNDEF_DOUBLE, b = SM_UNDEF_DOUBLE;
        SmStatus s1 = c->ConvertTFromNURBSToSTEP(t, a);
        SmStatus s2 = c->ConvertTFromSTEPToNURBS(t, b);
        o.push_back(st(s1 != SM_SUCCESS ? s1 : s2));
        o.push_back(a);
        o.push_back(b);
        break;
    }
    case OP_REPEATED:
        o.push_back(0.0);
        o.push_back(flg(c->HasRepeatedEndControlPoints(rd.num())));
        break;
    case OP_EQUAL:
    {
        bool self = rd.flag();
        SmCurve* other = self ? c : read_curve(rd, ctx);
        o.push_back(0.0);
        o.push_back(flg(*c == *other));
        if (!self)
            delete other;
        break;
    }
    case OP_COPY:
    {
        SmCurve* nc = NULL;
        SmStatus s = c->Copy(ctx, nc);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_state(o, nc);
        delete nc;
        break;
    }
    case OP_TRIM:
    {
        double a = rd.num();
        double b = rd.num();
        SmExtent1d ivl(a, b);
        SmStatus s = c->Trim(ivl);
        o.push_back(st(s));
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
        put_state(o, c);
        break;
    }
    case OP_TRANSFORM:
    {
        SmPoint3d org = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        bool has_scale = rd.flag();
        SmVector3d sc = rd.vec();
        SmAxis2Placement ax(org, x, y);
        SmStatus s = c->Transform(ax, has_scale ? &sc : NULL);
        o.push_back(st(s));
        put_state(o, c);
        break;
    }
    case OP_EDIT_PARAM:
    {
        double a = rd.num();
        double b = rd.num();
        SmStatus s = c->EditParameterization(SmExtent1d(a, b));
        o.push_back(st(s));
        put_state(o, c);
        break;
    }
    case OP_REVERSE:
    {
        double a = rd.num();
        double b = rd.num();
        SmExtent1d nv(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        SmStatus s = c->ReverseParameterization(SmExtent1d(a, b), nv);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(nv.GetMin());
            o.push_back(nv.GetMax());
        }
        put_state(o, c);
        break;
    }
    case OP_CONVERT_2D:
    {
        SmStatus s = c->ConvertTo2D();
        o.push_back(st(s));
        put_state(o, c);
        break;
    }
    case OP_ANALYTIC:
    {
        double a = rd.num();
        double b = rd.num();
        SmCurve* nc = NULL;
        SmStatus s = c->CreateAnalyticCurve(ctx, SmExtent1d(a, b), nc);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back(nc != NULL ? 1.0 : 0.0);
        delete nc;
        break;
    }
    case OP_STEP_INVERSION:
    {
        SmPoint3d p = rd.vec();
        double t = SM_UNDEF_DOUBLE;
        SmStatus s = c->STEPInversion(p, t, NULL);
        o.push_back(st(s));
        break;
    }
    case OP_DROP_FAST:
    {
        SmPoint3d p = rd.vec();
        SmSolutionArray sols;
        SmStatus s = c->DropPointFast(c->GetNaturalInterval(), SM_SO_MINIMIZE, p, NULL, 1e-6, NULL,
                                      SM_SR_SINGLE, sols);
        o.push_back(st(s));
        break;
    }
    case OP_INTERSECT_LINE:
    {
        SmLine line;
        SmBoolean more = FALSE;
        SmSolutionArray sols;
        SmExtent1d ivl = c->GetNaturalInterval();
        SmStatus s = c->IntersectWithLine(ivl, line, ivl, 1e-6, more, sols);
        o.push_back(st(s));
        o.push_back(flg(more));
        o.push_back((double)sols.GetSize());
        break;
    }
    case OP_SNAP:
    {
        double a = rd.num();
        double b = rd.num();
        bool has_vec = rd.flag();
        SmVector3d v = rd.vec();
        double sol = rd.num();
        double tol = rd.num();
        SmBoolean good = FALSE;
        SmStatus s = sm_SnapSolToSpecifiedEndpoint(c, SmExtent1d(a, b), has_vec ? &v : NULL, sol,
                                                   tol, good);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(sol);
            o.push_back(flg(good));
        }
        break;
    }
    case OP_LENGTH_IFEO:
    {
        double t = rd.num();
        double r = SM_UNDEF_DOUBLE;
        SmLengthIFEO f(*(SmBSplineCurve*)c);
        SmStatus s = f.Evaluate(t, r);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back(r);
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_curve_curve_p2_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                         int64_t cap, int64_t* out_len)
{
    if (in == nullptr || n_in < 0 || out_len == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    Rd rd{in, 0};
    std::vector<double> o;
    if (op >= OP_READ_DB)
        static_op(op, rd, ctx, o);
    else
    {
        SmCurve* c = read_curve(rd, ctx);
        curve_op(op, rd, ctx, c, o);
        delete c;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
