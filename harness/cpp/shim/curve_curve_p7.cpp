// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.curve_p7`: SmCCIGlobalSolver::LocalSolve (the local solve of
// the curve/curve intersection global solver) and SmCurve::GlobalPropertyAnalysis
// (source/SMLib/src/SmCurve.cpp, source/SMLib/inc/SmCurve.h), plus the co-ported
// sm_FindAdditionalCcTangentGuess (unit curve.curve_p5).
//
// SmCCIGlobalSolver and the static helpers it calls are local to SmCurve.cpp and cannot be
// linked against, so they are VERBATIM copies of upstream SmCurve.cpp (commit c9c5979), inside
// the namespace sc_crv_p7 (line ranges noted at each block; generated, do not edit by hand).
// GlobalPropertyAnalysis is called through the public SmCurve member.
//
// One entry point, sc_curve_curve_p7_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/curve_curve_p7.rs, which validates every
// input. A curve block is `kind dim ...` (0: SmBSplineCurve `oob n p m cpts(x y z w) knots`;
// 1: SmHermiteCurve `P1 D1 P2 D2`; 2: bounded SmLine `P V d0 d1 scale io`;
// 3: fixed real SmEllipse, dim 3, no further payload).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmCacheMgr.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveCache.h>
#include <SmCurveTypes.h>
#include <SmExtent1d.h>
#include <SmExtent3d.h>
#include <SmGeomUtility.h>
#include <SmGlobalSolver.h>
#include <SmHermiteCurve.h>
#include <SmEllipse.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmProjectedCurve.h>
#include <SmPseudoBox.h>
#include <SmSolutionArray.h>
#include <SmTArray.h>
#include <SmTree.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <map>
#include <vector>

#include "sc_sm.h"
#include "sc_gw.h"

using sc::Rd;
using sc::put_vec;
using sc::read_nurb;

namespace sc_crv_p7
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

// ---- verbatim: SmCurve.cpp lines 4310-4443: sm_FindAdditionalCcTangentGuess
static SmStatus sm_FindAdditionalCcTangentGuess
  (const SmCurve & crCurve1,
   const SmExtent1d & crInterval1,
   const SmCurve & crCurve2,
   const SmExtent1d & crInterval2,
   double dTsectT1,
   double dTsectT2,
   SmBoolean & rbFoundGuess,
   double & rdNewTGuess1,
   double & rdNewTGuess2)
{
    rbFoundGuess = FALSE;

    SmVector3d sPV1[2];
    SER(crCurve1.Evaluate(dTsectT1,1,TRUE,sPV1, TRUE));   // NonZeroTangent values
    SmVector3d sPV2[2];
    SER(crCurve2.Evaluate(dTsectT2,1,TRUE,sPV2, TRUE));   // NonZeroTangent values
    // First do a quick test to determine the angle of
    // intersection between the curves and see if we need to look
    // for a tangency.  If within 20 degrees look for tangency.
    if (!sPV1[1].IsParallelTo(sPV2[1],20)) {
        return SM_SUCCESS;
    }

    // Get the geometric properties of the curves at the intersection
    SmVector3d sGeomVec1[4];
    SER(crCurve1.EvaluateGeometric(dTsectT1,2,TRUE,sGeomVec1));
    SmVector3d sCurvatureVector1 = sGeomVec1[2];
    double dCurveCurvature1 = sCurvatureVector1.Length();
    if (dCurveCurvature1 > SM_EFF_ZERO) SER(sCurvatureVector1.Unitize());

    SmVector3d sGeomVec2[4];
    SER(crCurve2.EvaluateGeometric(dTsectT2,2,TRUE,sGeomVec2));
    SmVector3d sCurvatureVector2 = sGeomVec2[2];
    double dCurveCurvature2 = sCurvatureVector2.Length();
    if (dCurveCurvature2 > SM_EFF_ZERO) SER(sCurvatureVector2.Unitize());

    // If the normal curvature is zero then use a line instead of
    // a circle for the surface.
    SmBoolean bLineCircleIntersection = FALSE;
    double dCircle1Radius = 0.0, dCircle2Radius = 0.0;
    SmPoint3d sLinePoint, sCircle1Center, sCircle2Center;
    SmVector3d sLineVec, sCircle1Normal, sCircle2Normal;
    SmVector3d sFoundVector;
    double dScaledTol = 1.0e-4 * (1.0 + smos_Fabs(dCurveCurvature1) + smos_Fabs(dCurveCurvature2));
    if (smos_Fabs(dCurveCurvature1) < dScaledTol) {
        // If the curve2 also has zero curvature then we are done
        if (smos_Fabs(dCurveCurvature2) < dScaledTol) {
            // Done with no new guesses needed
            return SM_SUCCESS;
        }
        dCircle1Radius = smos_Fabs(1.0/dCurveCurvature2);
        sCircle1Center = sPV2[0] + sCurvatureVector2 * dCircle1Radius;
        sCircle1Normal = sGeomVec2[1] * sGeomVec2[2];
        if (sCircle1Normal.LengthSquared() < SM_EFF_ZERO_SQ) { return SM_SUCCESS; }
        SER(sCircle1Normal.Unitize());
        sLinePoint = sPV1[0];
        sLineVec = sPV1[1];
        SER(sLineVec.Unitize());
        bLineCircleIntersection = TRUE;
    }
    else { // have curve with curvature
        if (smos_Fabs(dCurveCurvature2) < dScaledTol) {
            sLinePoint = sPV2[0];
            sLineVec = sPV2[1];
            SER(sLineVec.Unitize());
            bLineCircleIntersection = TRUE;
        }
        else {
            dCircle2Radius = smos_Fabs(1.0/dCurveCurvature2);
            sCircle2Center = sPV2[0] + sCurvatureVector2 * dCircle2Radius;
            sCircle2Normal = sGeomVec2[1] * sGeomVec2[2];
            if (sCircle2Normal.LengthSquared() < SM_EFF_ZERO_SQ) { return SM_SUCCESS; }
            SER(sCircle2Normal.Unitize());
        }
        dCircle1Radius = smos_Fabs(1.0 / dCurveCurvature1);
        sCircle1Center = sPV1[0] + sCurvatureVector1 * dCircle1Radius;
        sCircle1Normal = sGeomVec1[1] * sGeomVec1[2];
        if (sCircle1Normal.LengthSquared() < SM_EFF_ZERO_SQ) { return SM_SUCCESS; }
        SER(sCircle1Normal.Unitize());
    }


    if (bLineCircleIntersection) {
        ULONG lNumTsect;
        double adTsectLineParam[2];
        SER(smgu_LineCoPlanarCircleIntersect(sLinePoint,sLineVec,
                                             sCircle1Center, sCircle1Normal,
                                             dCircle1Radius, SM_EFF_ZERO,
                                             lNumTsect,    adTsectLineParam));
        // If only have one or zero intersections we are done without
        // picking up extra guess
        if (lNumTsect!=2) { return SM_SUCCESS; }
        // Find the other intersection -- i.e. the one we don't have
        // The one we have would be at param 0, so use the farther from zero.
        double dLineT = adTsectLineParam[0];
        if (smos_Fabs(adTsectLineParam[1]) > smos_Fabs(adTsectLineParam[0])) {
            dLineT = adTsectLineParam[1];
        }
        sFoundVector = dLineT * sLineVec;
    }
    else { // Do circle circle intersection
        ULONG lNumTsect;
        SmPoint3d sTsectPoints[2];
        SER(smgu_CircleCoPlanarCircleIntersect(sCircle1Normal,
            sCircle1Center,dCircle1Radius,sCircle2Center,dCircle2Radius,
            SM_EFF_ZERO,
            lNumTsect,sTsectPoints));
        // If only have one or zero intersections we are done without
        // picking up extra guess
        if (lNumTsect != 2) { return SM_SUCCESS; }
        sFoundVector = sTsectPoints[0] - sPV1[0];
        if (sFoundVector.Length() < sTsectPoints[1].DistanceBetween(sPV1[0])) {
            sFoundVector = sTsectPoints[1] - sPV1[0];
        }
    }

    rbFoundGuess = TRUE;
    rdNewTGuess1 = dTsectT1 + sm_ConvertToParameterDelta(sPV1[1],sFoundVector);
    rdNewTGuess2 = dTsectT2 + sm_ConvertToParameterDelta(sPV2[1],sFoundVector);

    if (!crInterval1.ContainsValue(rdNewTGuess1) &&
        !crInterval2.ContainsValue(rdNewTGuess2)) {
        // If the guess is outside of the domain we'll be able to pick it up using
        // another method.
        rbFoundGuess = FALSE;
        return SM_SUCCESS;
    }
    rdNewTGuess1 = crInterval1.ClampValue(rdNewTGuess1);
    rdNewTGuess2 = crInterval2.ClampValue(rdNewTGuess2);

    return SM_SUCCESS;  // Found a guess - loaded it now we are done

} // end sm_FindAdditionalCcTangentGuess
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 4456-4541: SmCCIGlobalSolver (class, constructor)
class SmCCIGlobalSolver : public SmGlobalSolver
{
    // inherited from SmGlobalSolver
    //  SmOperationCategory     m_eOperationCategory;       // categorizes the Operation Type
    //  SmBoolean               m_bProjectedOperation;      // FALSE for this solver
    //  SmSolutionRequestedType m_eSolutionRequested;       // one of  SM_SR_SINGLE,              // Only produce one solution - the best
    //                                                      //         SM_SR_ALL,                 // Find all solutions that satisfy the function to with
    //                                                      //                                    // the given tolerance
    //                                                      //         SM_SR_NODES                // Only find the nodes in the tree where solutions may exist
    //  ULONG                   m_lNumTrees;                // number of geometry trees to traverse, 2 for this solver
    //  ULONG                   m_lNumVariables;            // 2 for this solver, Curve1 param, Curve2 param
    //  SmTree                * m_apTrees[SM_GS_MAX_TREES]; // array of trees to traverse, sized:[m_lNumTrees]
    //  double                  m_d3dTolerance;             // 3d distance tolerance, used in many ways:
    //  double                  m_dBestAnswerSoFarSq;       // not used by this solver
    //  double                  m_dAtDistance;              // not used by this solver
    //  double                  m_dAtDistanceSq;            // not used by this solver
    //  const SmVector3d      * m_cpOptVectors;             //  SM_SO_PROJECTED_INTERSECT:         vec[0] = projection plane unit-normal
    //                                                      //  SM_SO_ROTATED_PROJECTED_INTERSECT: vec[0] = pt on rotation axis
    //                                                      //                                     vec[1] = rotation axis unit-vector
    //                                                      //                                     vec[2] = unit-XAxis of rotation plane
    //                                                      //  SM_SO_PERSPECTIVE_INTERSECT:       vec[0] = pt on view plane
    //                                                      //                                     vec[1] = view plane unit normal
    //                                                      //                                     vec[2] = eye pt
    //  SmSolutionArray       * m_pSolutions;               // array of sorted solutions

public:
    const SmExtent1d & m_crInterval1;       // domain limit on curve1
    const SmExtent1d & m_crInterval2;       // domain limit on curve2
    SmBoolean          m_bSelfIntersection; // TRUE = intersecting curve with itself, FALSE = not
    SmBoolean          m_bDoProjection;     // TRUE  = project curves to common plane then intersect, FALSE = don't
                                            // TRUE when eSolverOperation oneof: SM_SO_PROJECTED_INTERSECT
                                            //                                   SM_SO_ROTATED_PROJECTED_INTERSECT
                                            //                                   SM_SO_PERSPECTIVE_INTERSECT
    SmProjectionType   m_eProjectionType;   // when m_bDoProjection == TRUE, specifies the type of projection. oneof:
                                            //   SM_PT_PARALLEL = proj to plane containing pt[0,0,0] along normal m_cpOptVectors[0]
                                            //                    m_cpOptVectors[0] = plane unit-normal
                                            //   SM_PT_ROTATION = proj through rotation about
                                            //                    rot axis [pt=m_cpOptVectors[0], unitdir=m_cpOptVectors[1]]
                                            //                    to plane containing vector [unitXAxis-m_cpOptVectors[2]]
                                            //                    m_cpOptVectors[0] = point on rotation axis
                                            //                    m_cpOptVectors[1] = rotation axis unit-vector
                                            //                    m_cpOptVectors[2] = unit-XAxis perp to rotation axis defining target plane
                                            //   SM_PT_PERSPECTIVE = perspective project to plane towards common eye point
                                            //                    m_cpOptVectors[0] = point on view plane
                                            //                    m_cpOptVectors[1] = viee plane unit-normal
                                            //                    m_cpOptVectors[2] = eye point

    SmCCIGlobalSolver(SmTree           * pCurve1Tree,
                      SmTree           * pCurve2Tree,
                      const SmExtent1d & crInterval1,
                      const SmExtent1d & crInterval2,
                      SmBoolean          bDoProjection = FALSE,
                      SmProjectionType   eProjectionType = SM_PT_PARALLEL);

    virtual SmStatus LocalSolve(SmTreeNode * apBranch[SM_GS_MAX_TREES],
                                SmBoolean  & rbNeedsMoreSubdivision);

} ; // end class SmCCIGlobalSolver

/********************************************************************
PURPOSE:

NOTES:
********************************************************************/
SmCCIGlobalSolver::SmCCIGlobalSolver
  (SmTree           * pCurve1Tree,     // in :
   SmTree           * pCurve2Tree,     // in :
   const SmExtent1d & crInterval1,     // in :
   const SmExtent1d & crInterval2,     // in :
   SmBoolean          bDoProjection,   // in : TRUE for solverOps SM_SO_PROJECTED_INTERSECT
                                       //                         SM_SO_ROTATED_PROJECTED_INTERSECT
                                       //                         SM_SO_PERSPECTIVE_INTERSECT
                                       //      default:[FALSE]
   SmProjectionType   eProjectionType) // in : oneof: SM_PT_PARALLEL,
                                       //             SM_PT_PERSPECTIVE,
                                       //             SM_PT_ROTATION
                                       //      used when bDoProjection == TRUE, default:[SM_PT_PARALLEL]
: SmGlobalSolver(pCurve1Tree, pCurve2Tree, 2),
  m_crInterval1(crInterval1),
  m_crInterval2(crInterval2),
  m_bDoProjection(bDoProjection),
  m_eProjectionType(eProjectionType)
{
  m_bSelfIntersection = (pCurve1Tree == pCurve2Tree) ? TRUE : FALSE ;

} // end SmCCIGlobalSolver::SmCCIGlobalSolver constructor
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

// ---- verbatim: SmCurve.cpp lines 4620-4874: sm_FilterSolution
static SmStatus sm_FilterSolution
  (const SmCurve        & crCurve1,       // in : 1st Curve
   const SmExtent1d     & crIvl1,         // in : 1st curve interval
   const SmCurve        & crCurve2,       // in : 2nd Curve
   const SmExtent1d     & crIvl2,         // in : 2nd Curve interval
   const SmContinuityType aeConts1[2],    // in : 1st Curve Continuity Bounds
   const SmContinuityType aeConts2[2],    // in : 2nd Curve Continuity Bounds
   ULONG                  lAdjacency,     // in : 0 = Not SelfIntersecting or remote spans
                                          //      1 = Adj SelfIntersection spans: [1stCurve, 2ndCurve]
                                          //      2 = Adj SelfIntersection spans: [2ndCurve, 1stCurve]
   const SmSolution     & crSolution,     // in : Solution to review
   SmBoolean            & rbKeepSolution) // out: TRUE = Add solution m_pSolutions array
                                          //      FALSE= Cull Solution
{
  // init output
  rbKeepSolution = TRUE;

  // If we have an exact answer keep it
  if (crSolution.m_vStart.m_dSolutionValue < SM_EFF_ZERO)
    { rbKeepSolution = TRUE;
      return SM_SUCCESS;
    }

  // Eliminate adjacency cases when doing self intersection
  if(   (lAdjacency == 1 && SM_ARE_SAME(crIvl1.GetMin(),crSolution.m_vStart[0]))
     || (lAdjacency == 2 && SM_ARE_SAME(crIvl1.GetMax(),crSolution.m_vStart[0])))
    {
      rbKeepSolution = FALSE;
      return SM_SUCCESS;
    }

  // If we have an inexact answer see if it lies on a boundary

  // Get Solution CurvePoints and CurveTangents
  SmVector3d sPV1[2], sPV2[2];
  SER(crCurve1.Evaluate(crSolution.m_vStart[0],1,TRUE,sPV1, TRUE));   // nonZeroTangent values
  SER(crCurve2.Evaluate(crSolution.m_vStart[1],1,TRUE,sPV2, TRUE));   // nonZeroTangent values

  // keep answers good to ScaledZero.  [tol value: 080704]
  // dScaledZero needs to be sensitive to parameter values [B628]
  //double dScaledZero = SM_EFF_ZERO * (1.0 + smos_Max(
  //        smos_Max( sPV1[0].GetMaxDimension(), sPV1[1].GetMaxDimension() ),
  //        smos_Max( sPV2[0].GetMaxDimension(), sPV2[1].GetMaxDimension())) );

  double dScaledZero = SM_EFF_ZERO * (1.0 + smos_4Max(
          smos_Max( sPV1[0].GetMaxDimension(), sPV1[1].GetMaxDimension() ),
          smos_Max( sPV2[0].GetMaxDimension(), sPV2[1].GetMaxDimension() ),
          crSolution.m_vStart[0], crSolution.m_vStart[1]) );
  if (crSolution.m_vStart.m_dSolutionValue < dScaledZero)
    {
      rbKeepSolution = TRUE;
      return SM_SUCCESS;
    }

  // arrive here for non-zero intersection points

  // classify the solution Point against each interval
  SmBoolean bStart1  = SM_ARE_SAME(crIvl1.GetMin(),crSolution.m_vStart[0]) ;
  SmBoolean bEnd1    = SM_ARE_SAME(crIvl1.GetMax(),crSolution.m_vStart[0]) ;
  SmBoolean bMid1    = !bStart1 && !bEnd1 ;
  SmBoolean bNext1   =  bMid1 // True if there is more curve beyond a boundary
                       || ( bStart1 && aeConts1[0] >= SM_CT_G1 )
                       || ( bEnd1   && aeConts1[1] >= SM_CT_G1 );

  SmBoolean bStart2  = SM_ARE_SAME(crIvl2.GetMin(),crSolution.m_vStart[1]) ;
  SmBoolean bEnd2    = SM_ARE_SAME(crIvl2.GetMax(),crSolution.m_vStart[1]) ;
  SmBoolean bMid2    = !bStart2 && !bEnd2 ;
  SmBoolean bNext2   =    bMid2
                       || ( bStart2 && aeConts2[0] >= SM_CT_G1 )
                       || ( bEnd2   && aeConts2[1] >= SM_CT_G1 );

  // get gap vector
  SmVector3d sGapVector = sPV2[0] - sPV1[0];

  // toss answers on interval boundaries
  //  when the next interval will be closer than this one.

  // when either solution is on a boundary with a next segment
  if(   (!bMid1 && bNext1 ) || (!bMid2 && bNext2) )
    {
      // If either solution is on a cache boundary with a next segment,
      // and the curve is smooth at the boundary,
      // then if the curve is not exactly perpendicular to the gap,
      // it's getting closer in one direction or the other.
      // So reject it unless it's 'exactly' perpendicular.
      // [B124 and similar]
      // But unfortunately, this causes other problems. [B125]
      // (And 124 has been taken care of elsewhere.)
      // SmVector3d sUnitTan;
      // if( !bMid1 && bNext1 ) // on boundary of curve 1
      // {
      //     sm_UnitizeTangent( sPV1, sGapVector, sUnitTan );
      // }
      // else // on boundary of curve 2
      // {
      //     sm_UnitizeTangent( sPV2, sGapVector, sUnitTan );
      // }
      // double dDot = sUnitTan.Dot( sGapVector );
      // if ( smos_Fabs( dDot ) > 10*SM_EFF_ZERO )
      //   {
      //     rbKeepSolution = FALSE;
      //     return SM_SUCCESS;
      //   }


//      SmBoolean bKeep = TRUE ;
      // get unit tangents pointing into next segment
      SmVector3d sT1, sT2;
      sm_UnitizeTangent( sPV1,  sGapVector, sT1 );  if ( bStart1 ) { sT1 = -sT1; }
      sm_UnitizeTangent( sPV2, -sGapVector, sT2 );  if ( bStart2 ) { sT2 = -sT2; }

      // toss sol when next segment is closer to other solution current point
      double dT1Gap = sT1.Dot(sGapVector) ;
      double dT2Gap = sT2.Dot(sGapVector) ;

      SmBoolean bPerp1 = FALSE; // = sT1.IsPerpendicularTo(sGapVector, 0.5); [B124 and similar]
      SmBoolean bPerp2 = FALSE; // = sT2.IsPerpendicularTo(sGapVector, 0.5);

      SmBoolean bPerp12 = bPerp1 || bPerp2 ;

      double dDotLimit = SM_EFF_ZERO_SQRT;  // Can't be smaller than this.  [B139 B175 B452]

      if(   (!bMid1 && bNext1 && dT1Gap >  dDotLimit && !bPerp1)
         || (!bMid2 && bNext2 && dT2Gap < -dDotLimit && !bPerp2))
        {
          rbKeepSolution = FALSE;
          return SM_SUCCESS;
        }

      // toss solutions when next segment is closer to other solution next points.
      //   Distance in next interval:
      //   dist**2 = ((P2 + t2 * V2) - (P1 + t1 * V1))^2
      //           = ( (P2-P1) + t2*V2 - t1*V1 )^2
      //           = Gap.Gap + 2*Gap.(t2*V2 - t1*V1) + (t2*V2 - t1*V1)**2
      //           = Gap.Gap + 2*(Gap.V2)*t2 - 2*(Gap.V1)*t1 + (V2.V2)*t2^2 - 2*(V2.V1)*t1*t2 + (V1.V1)*t1^2
      //   d(dist**2)/dt2 =  2*(Gap.V2) + 2*(V2.V2) * t2 - 2*(V1.V2) * t1
      //   d(dist**2)/dt1 = -2*(Gap.V1) + 2*(V1.V1) * t1 - 2*(V1.V2) * t2
      //   Next segment is diverging when
      //      d(dist**2)/dt >= 0.0 at t=0
      //   Converging in next interval when Gap*V2 < 0

      // compare Next1 with This2
      if(!bMid1 && bNext1 && !bPerp12)
        { SmVector3d sV21 = -sT2 - sT1 ;
          double dV21Gap = sV21.Dot(sGapVector) ;
          if(dV21Gap < -dDotLimit )  // SM_EFF_ZERO, not 0 [B139]
            { rbKeepSolution = FALSE;
              return SM_SUCCESS;
            }
        }
      // compare Next1 with Next2
      if(!bMid1 && bNext1 && bNext2 && !bPerp12)
        { SmVector3d sV21 = sT2 - sT1 ;
          double dV21Gap = sV21.Dot(sGapVector) ;
          if(dV21Gap < -dDotLimit )
            { rbKeepSolution = FALSE;
              return SM_SUCCESS;
            }
        }
      // compare Next2 with This1
      if(!bMid2 && bNext2 && !bPerp12)
        { SmVector3d sV21 = sT2 + sT1 ;
          double dV21Gap = sV21.Dot(sGapVector) ;
          if(dV21Gap < -dDotLimit )
            { rbKeepSolution = FALSE;
              return SM_SUCCESS;
            }
        }
      // compare Next2 with Next1
      if(!bMid2 && bNext2 && bNext1 && !bPerp12)
        { SmVector3d sV21 = sT2 - sT1 ;
          double dV21Gap = sV21.Dot(sGapVector) ;
          if(dV21Gap < -dDotLimit )
            { rbKeepSolution = FALSE;
              return SM_SUCCESS;
            }
        }

      // we are not comparing this1 with this2
      // in the end we may want to

    } // end solution on an internal boundary check

  // keep minimization points found at curve/curve tangentIntersections
  // when CurveTangents are perpendicular to gapVector to within 1 degree
  if (   sGapVector.IsPerpendicularTo(sPV1[1],1.0)
      && sGapVector.IsPerpendicularTo(sPV2[1],1.0) )
    {
// gwc: moved this section out of the minimization branch and generalized it above
//      Keep this until change is tested
//
//    // toss answers on interval boundaries
//    //  when the next interval will be closer than this one.
//    if(   (!bMid1 && bNext1)  // solution on an interval boundary
//       || (!bMid2 && bNext2)) // with more curve intervals to go.
//      {
//        // get Curve unitized tangents
//        SmVector3d sT1 = sPV1[1] ; sT1.Unitize() ;
//        SmVector3d sT2 = sPV2[1] ; sT2.Unitize() ;
//
//        // get tangents pointing into the next intervals
//        if(bStart1) { sT1 = -sT1 ; }
//        if(bStart2) { sT2 = -sT2 ; }
//        double dT1T2 = sT1.Dot(sT2) ;
//        if(bMid1)   { SM_ASSERT(!bMid2) ;
//                      if(dT1T2 < 0.0) { sT1 = -sT1 ;
//                                        dT1T2 = - dT1T2 ;
//                                      }
//                    }
//        if(bMid2)   { SM_ASSERT(!bMid1) ;
//                      if(dT1T2 < 0.0) { sT2 = -sT2 ;
//                                        dT1T2 = - dT1T2 ;
//                                      }
//                    }
//
//        // distance in next interval
//        // dist**2 = ((P2 + u * V2) - (P1 + u * V1)) * ((P2 + u * V2) - (P1 + u * V1))
//        //         = (P21 + u * V21) * (P21 + u * V21)
//        //         = P21*P21 + 2*u*V21*P21 + u*u*V21*V21
//        // d(dist**2)/du = 2*V21*P21 + 2*u*V21*V21
//        // Next segment is diverging when
//        //    d(dist**2)/du >= 0.0 at u=0.0
//        // Converging in next interval when V21*P21 < 0
//        SmVector3d sV21 = sT2 - sT1 ;
//        if(sV21.Dot(sGapVector) < 0.0)
//          {
//            rbKeepSolution = FALSE;
//            return SM_SUCCESS;
//          }
//      } // end solution on interval boundary check
//
      // keep answer assuming this is a point of closest approach.
      rbKeepSolution = TRUE;
      return SM_SUCCESS;

    } // end point of closest approach solution check

  // arrive here for nonZero, nonTangent intersection cases

  // keep intersections on Curve Boundaries (not interval boundaries)
  //  A curve boundary will have a low continuity interval boundary
  if(   (aeConts1[0] <= SM_CT_C0 && bStart1)
     || (aeConts1[1] <= SM_CT_C0 && bEnd1)
     || (aeConts2[0] <= SM_CT_C0 && bStart2)
     || (aeConts2[1] <= SM_CT_C0 && bEnd2))
    {
      rbKeepSolution = TRUE;
      return SM_SUCCESS;
    }

  // else discard the answer
  rbKeepSolution = FALSE;
  return SM_SUCCESS;

} // end sm_FilterSolution
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 4881-6065: SmCCIGlobalSolver::LocalSolve
SmStatus SmCCIGlobalSolver::LocalSolve
  (SmTreeNode * apBranch[SM_GS_MAX_TREES],  // in :
   SmBoolean  & rbNeedsMoreSubdivision)     // in :
{
  // init output
  rbNeedsMoreSubdivision = FALSE;

  // locals
  SmTreeNode    * pCurveNode1   = apBranch[0];
  SmTreeNode    * pCurveNode2   = apBranch[1];
  const SmCurve * pParentCurve1 = (SmCurve *)pCurveNode1->GetOwnerObject() ;
  const SmCurve * pParentCurve2 = (SmCurve *)pCurveNode2->GetOwnerObject() ;
  SmBezierSpan  * pBezSpan1     = (SmBezierSpan*)pCurveNode1->m_pData; NER(pBezSpan1);
  SmBezierSpan  * pBezSpan2     = (SmBezierSpan*)pCurveNode2->m_pData; NER(pBezSpan2);
  SmPseudoBox     sPseudoBox1   = pBezSpan1->GetPseudoBox();
  SmPseudoBox     sPseudoBox2   = pBezSpan2->GetPseudoBox();
  SmExtent1d      sIvl1         = pBezSpan1->GetInterval() ;
  SmExtent1d      sIvl2         = pBezSpan2->GetInterval() ;

  SM_ASSERT(pCurveNode1->m_eAuxDataType == SM_AD_BEZIER_CURVE);
  SM_ASSERT(pCurveNode2->m_eAuxDataType == SM_AD_BEZIER_CURVE);
  SM_ASSERT(pParentCurve1 && pParentCurve2) ;

  // expand target interval 3d bounding boxes
  SmPseudoBox sExpandedBox = sPseudoBox1 ;
  sExpandedBox.ExpandAbsolute( m_d3dTolerance );
  SM_ASSERT_VALID_NO_STREAM(&sExpandedBox) ;

  // locals - the curves to be intersected which get replaced if this is a projection problem
  const SmCurve *pCurve1 = pParentCurve1 ;
  const SmCurve *pCurve2 = pParentCurve2 ;

  // allow for projected solutions
  SmProjectedCurve sProjCrv1;
  SmProjectedCurve sProjCrv2;
  SmPoint3d sOrigin(0.0, 0.0, 0.0), sAuxData(0,0,0) ;
  const SmPoint3d  *pProjPoint      = NULL ;
  const SmVector3d *pProjUnitVector = NULL ;
  const SmVector3d *pAuxData        = NULL ;

  // build ProjCrvs and projected PseudoBoxes for various projections
  if (m_bDoProjection)
    {
      switch(m_eProjectionType)
        {
          // set the ProjCrvs and the Project Specification parameters for various projections
          case SM_PT_PARALLEL :
            {
              sProjCrv1.SetCanonical(*pCurve1, sOrigin, m_cpOptVectors[0], sAuxData, SM_PT_PARALLEL, FALSE, pCurve1->GetContext());
              sProjCrv2.SetCanonical(*pCurve2, sOrigin, m_cpOptVectors[0], sAuxData, SM_PT_PARALLEL, FALSE, pCurve2->GetContext());
              pProjPoint      = &sOrigin ;
              pProjUnitVector = &m_cpOptVectors[0] ;
              pAuxData        = &sAuxData ;
            } break ;
          case SM_PT_ROTATION:
            {
              sProjCrv1.SetCanonical(*pCurve1, m_cpOptVectors[0], m_cpOptVectors[1], m_cpOptVectors[2], SM_PT_ROTATION, FALSE, pCurve1->GetContext());
              sProjCrv2.SetCanonical(*pCurve2, m_cpOptVectors[0], m_cpOptVectors[1], m_cpOptVectors[2], SM_PT_ROTATION, FALSE, pCurve2->GetContext());
              pProjPoint      = &m_cpOptVectors[0] ;
              pProjUnitVector = &m_cpOptVectors[1] ;
              pAuxData        = &m_cpOptVectors[2] ;
            } break ;
          case SM_PT_PERSPECTIVE:
            {
              sProjCrv1.SetCanonical(*pCurve1, m_cpOptVectors[0], m_cpOptVectors[1], m_cpOptVectors[2], SM_PT_PERSPECTIVE, FALSE, pCurve1->GetContext());
              sProjCrv2.SetCanonical(*pCurve2, m_cpOptVectors[0], m_cpOptVectors[1], m_cpOptVectors[2], SM_PT_PERSPECTIVE, FALSE, pCurve2->GetContext());
              pProjPoint      = &m_cpOptVectors[0] ;
              pProjUnitVector = &m_cpOptVectors[1] ;
              pAuxData        = &m_cpOptVectors[2] ;
            } break ;
          default:
                  break;
        } // end switch on m_eProjectionTyep

      // pay attention - when working with projections, redefine Curve1 and Curve2 to be the projected curves
      pCurve1 = &sProjCrv1;
      pCurve2 = &sProjCrv2;

      // project the bounding boxes to the common plane
      sExpandedBox.ProjectToPlane(sExpandedBox, m_eProjectionType, pProjPoint, pProjUnitVector, pAuxData) ;
      sPseudoBox1.ProjectToPlane (sPseudoBox1,  m_eProjectionType, pProjPoint, pProjUnitVector, pAuxData) ;
      sPseudoBox2.ProjectToPlane (sPseudoBox2,  m_eProjectionType, pProjPoint, pProjUnitVector, pAuxData) ;

    } // end looking for a projection solution

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE ;
  // draw input curves being intersected
  if(bDebugMe)
    {
      SmEdge *pEdge1 = (pParentCurve1->GetOwner() && pParentCurve1->GetOwner()->IsKindOf(SmEdge_TYPE)) ? (SmEdge *)pParentCurve1->GetEdge() : NULL ;
      SmEdge *pEdge2 = (pParentCurve2->GetOwner() && pParentCurve2->GetOwner()->IsKindOf(SmEdge_TYPE)) ? (SmEdge *)pParentCurve2->GetEdge() : NULL ;
      SmBrep *pBrep1 = pEdge1 ? pEdge1->GetBrep() : NULL ;
      SmBrep *pBrep2 = pEdge2 ? pEdge2->GetBrep() : NULL ;

      smgfx_Erase() ;
      smgfx_SetLook(1,2, 0,0,1) ; if(pBrep1) pBrep1->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,1,0) ; if(pBrep2 && pBrep1 != pBrep2) pBrep2->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3, 1,0,0) ; if(pCurve1) pCurve1->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3, 1,0,1) ; if(pCurve2) pCurve2->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 0,1,1) ; if(pEdge1) pEdge1->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 1,1,0) ; if(pEdge2) pEdge2->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(5,6, 0,1,1) ; if(pCurve1) pCurve1->Draw(&sIvl1) ; sm_GraphicsLoop() ;         // subdivide ivl
      smgfx_SetLook(5,6, 0,1,1) ; if(pCurve2) pCurve2->Draw(&sIvl2) ; sm_GraphicsLoop() ;         // subdivide ivl
      smgfx_SetLook(7,8, 0,0,0) ; if(pCurve1) pCurve1->Draw(&m_crInterval1) ; sm_GraphicsLoop() ; // intsct ivl
      smgfx_SetLook(7,8, 0,0,1) ; if(pCurve2) pCurve2->Draw(&m_crInterval2) ; sm_GraphicsLoop() ; // intsct ivl
      sm_GraphicsLoop() ;
    }
#endif // SM_DEBUG_CODE

  // do tight bounding check on PseudoBoxes - note: pseudoBoxes already projected for projection problems
  if (sExpandedBox.AreDisjoint(sPseudoBox2))
    { return SM_SUCCESS; }

  // arrive here when pseudoBoxes intersect

#ifdef SM_DEBUG_CODE
  // draw input curve intervals being intersected
  if (bDebugMe)
    {
      SmEdge  *pEdge1  = (SmEdge *)pParentCurve1->GetEdge() ;
      SmEdge  *pEdge2  = (SmEdge *)pParentCurve2->GetEdge() ;
      SmBrep  *pBrep1  = pEdge1 ? pEdge1->GetBrep() : NULL ;
      SmBrep  *pBrep2  = pEdge2 ? pEdge2->GetBrep() : NULL ;

      smgfx_Erase();
      smgfx_SetLook(1,2, 0,0,1) ; if(pBrep1) pBrep1->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,1,0) ; if(pBrep2 && pBrep1 != pBrep2) pBrep2->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3, 1,0,0) ; if(pCurve1) pCurve1->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3, 1,0,1) ; if(pCurve2) pCurve2->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 0,1,1) ; if(pEdge1) pEdge1->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 1,1,0) ; if(pEdge2) pEdge2->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(5,6, 0,1,1) ; if(pCurve1) pCurve1->Draw(&sIvl1) ; sm_GraphicsLoop() ;         // subdivide ivl
      smgfx_SetLook(5,6, 0,1,1) ; if(pCurve2) pCurve2->Draw(&sIvl2) ; sm_GraphicsLoop() ;         // subdivide ivl
      smgfx_SetLook(7,8, 0,0,0) ; if(pCurve1) pCurve1->Draw(&m_crInterval1) ; sm_GraphicsLoop() ; // intsct ivl
      smgfx_SetLook(7,8, 0,0,1) ; if(pCurve2) pCurve2->Draw(&m_crInterval2) ; sm_GraphicsLoop() ; // intsct ivl
      sm_GraphicsLoop();
    }
#endif // SM_DEBUG_CODE

  // low work - Eliminate self-intersection cases that can't intersect
  ULONG lAdjacency = 0;
  if (m_bSelfIntersection)
    {
      if (pBezSpan1 == pBezSpan2)
        { return SM_SUCCESS; }
      if (pBezSpan1->m_lLeft == pBezSpan2->m_lLeft-1)
        {
          // 1 span just before 2
          lAdjacency = 1;
          if (   pBezSpan1->m_aeConts[1] > SM_CT_C0
              &&   pBezSpan1->m_dMaxTurningAngleDeg
                 + pBezSpan2->m_dMaxTurningAngleDeg < 360.0)
            { return SM_SUCCESS; }
        }
      if (pBezSpan1->m_lLeft == pBezSpan2->m_lLeft+1)
        {
          // 2 span just before 1
          lAdjacency = 2;
          if (   pBezSpan1->m_aeConts[0] > SM_CT_C0
              &&   pBezSpan1->m_dMaxTurningAngleDeg
                 + pBezSpan2->m_dMaxTurningAngleDeg < 360.0)
            { return SM_SUCCESS; }
        }
    } // end m_bSelfIntersection == TRUE check

  // If we are just testing for intersection stop here and do the test.
  if (m_eSolverOperation == SM_SO_INTERSECTION_TEST)
    {
      SmExtent3d sBBox = pCurveNode1->m_sBBox;
      sBBox.ExpandAbsolute(m_d3dTolerance);
      SmExtent3d sResult;
      sBBox.Intersect(pCurveNode2->m_sBBox,sResult);

      // GWC: should there be a test on sResult - if empty then no solution?
      //      guess we asssume if the pseudo boxes intersect, the regular boxes will intersect as well.

      // Store a union of the intersection 3D bounding box and UV box for
      // the intersections.
      if (m_pSolutions->GetSize() == 0)
        {
          SmSolution sSol;
          sSol.m_apNodes[0] = apBranch[0];
          sSol.m_apNodes[1] = apBranch[1];
          sSol.m_lNumVariables = 7;
          sSol.m_eSolutionType = SM_ST_SINGLE_VALUE;
          sSol.m_vStart[6] = 1;                    // out: possible leafNode/leafNode intersection count
          sSol.m_vStart[0] = sResult.GetMin().x;
          sSol.m_vStart[1] = sResult.GetMin().y;
          sSol.m_vStart[2] = sResult.GetMin().z;
          sSol.m_vStart[3] = sResult.GetMax().x;
          sSol.m_vStart[4] = sResult.GetMax().y;
          sSol.m_vStart[5] = sResult.GetMax().z;
          sSol.m_vEnd[0] = sIvl1.GetMin();
          sSol.m_vEnd[1] = sIvl1.GetMax();
          sSol.m_vEnd[2] = sIvl2.GetMin();
          sSol.m_vEnd[3] = sIvl2.GetMax();
          sSol.m_lNumObjects = 2 ;
          sSol.m_apObjects[0] = (SmObject *)pParentCurve1 ;
          sSol.m_apObjects[1] = (SmObject *)pParentCurve2 ;

          // However, if the is only one subdivision (total) in each tree,
          // then we could well have multiple intersections, or coincidence,
          // and we won't be calling this again -- to do the following 'else'
          // branch which will increment sSol.m_vStart[6].
          // If that is the case, set sSol.m_vStart[6] to 2.
          // Check that case by comparing the tree-node intervals sIvl1 and sIvl2
          // with the whole curve/edge intervals, m_crInterval1 and m_crInterval2.
          // [B162]
          if ( sIvl1 == m_crInterval1  &&  sIvl2 == m_crInterval2 )
            { sSol.m_vStart[6] = 2; }

          m_pSolutions->Add(sSol);

        }  // end m_pSolutions->GetSize() == 0 branch
      else // m_pSolutions->GetSize() > 0 branch
        {
          // increment the possible leafNode/leafNode intersection total count
          SmSolution & rSol = (*m_pSolutions)[0];
          rSol.m_vStart[6]  = rSol.m_vStart[6] + 1.0;  // Number of contributors to intersection

          // build previous intersection bounding box
          SmExtent3d sOldBox(SmPoint3d(rSol.m_vStart[0],rSol.m_vStart[1],rSol.m_vStart[2]),
                             SmPoint3d(rSol.m_vStart[3],rSol.m_vStart[4],rSol.m_vStart[5]));

          // union new intersection box with previous
          sResult.Union(sOldBox,sResult);

          // update output
          rSol.m_vStart[0] = sResult.GetMin().x;
          rSol.m_vStart[1] = sResult.GetMin().y;
          rSol.m_vStart[2] = sResult.GetMin().z;
          rSol.m_vStart[3] = sResult.GetMax().x;
          rSol.m_vStart[4] = sResult.GetMax().y;
          rSol.m_vStart[5] = sResult.GetMax().z;

          // build previous intersection interval bounds
          SmExtent1d sOldIvl1(rSol.m_vEnd[0],rSol.m_vEnd[1]);
          SmExtent1d sOldIvl2(rSol.m_vEnd[2],rSol.m_vEnd[3]);

          // union new intersection interval with previous
          sOldIvl1.Union(sIvl1,sIvl1);
          sOldIvl2.Union(sIvl2,sIvl2);

          // update output
          rSol.m_vEnd[0] = sIvl1.GetMin();
          rSol.m_vEnd[1] = sIvl1.GetMax();
          rSol.m_vEnd[2] = sIvl2.GetMin();
          rSol.m_vEnd[3] = sIvl2.GetMax();
        }  // end m_pSolutions->GetSize() > 0 branch

      // all done when just testing for possible intersections
      return SM_SUCCESS;

    } // end m_eSolverOperation == SM_SO_INTERSECTION_TEST check

  // Test for leafNode interval containment within given operation interval
  // and trim the Bezier intervals when they are not completely
  // contained by the processing intervals.
  if (m_crInterval1.AreDisjoint(sIvl1)) { return SM_SUCCESS; }
  if (m_crInterval2.AreDisjoint(sIvl2)) { return SM_SUCCESS; }

  SmContinuityType aeConts1[2], aeConts2[2];
  aeConts1[0] = pBezSpan1->m_aeConts[0];
  aeConts1[1] = pBezSpan1->m_aeConts[1];
  aeConts2[0] = pBezSpan2->m_aeConts[0];
  aeConts2[1] = pBezSpan2->m_aeConts[1];

  // trim sIvl1 to m_crInterval1 - set trimmed endContinuities = SM_CT_DISCONTINUOUS
  if (sIvl1.GetMin() <= m_crInterval1.GetMin())
    {aeConts1[0] = SM_CT_DISCONTINUOUS;
    }
  if (sIvl1.GetMax() >= m_crInterval1.GetMax())
    { aeConts1[1] = SM_CT_DISCONTINUOUS;
    }

  if (!sIvl1.IsContainedBy(m_crInterval1, SM_EFF_ZERO))
    { sIvl1.Intersect(m_crInterval1,sIvl1);
      // Check for zero length intervals
      if (SM_ARE_SAME(sIvl1.GetMax(),sIvl1.GetMin()))
        { return SM_SUCCESS; }
    }

  // get sIvl1 endPoints and the chord between them
  SmPoint3d sLinePt1, sEnd1;
  pCurve1->EvaluatePoint(sIvl1.GetMin(),sLinePt1);
  pCurve1->EvaluatePoint(sIvl1.GetMax(),sEnd1);
  SmVector3d sLineVec1 = sEnd1 - sLinePt1;

  // trim sIvl2 to m_crInterval2 - set trimmed endContinuities = SM_CT_DISCONTINUOUS
  if (sIvl2.GetMin() <= m_crInterval2.GetMin())
    { aeConts2[0] = SM_CT_DISCONTINUOUS;
    }
  if (sIvl2.GetMax() >= m_crInterval2.GetMax())
    { aeConts2[1] = SM_CT_DISCONTINUOUS;
    }
  if (!sIvl2.IsContainedBy(m_crInterval2, SM_EFF_ZERO))
    { sIvl2.Intersect(m_crInterval2,sIvl2);
      // Check for zero length intervals
      if (SM_ARE_SAME(sIvl2.GetMax(),sIvl2.GetMin())) { return SM_SUCCESS; }
    }

#ifdef SM_DEBUG_CODE
  // draw input curve intervals being intersected after intervals have been trimmed
  if (bDebugMe)
    {
      SmEdge  *pEdge1  = (SmEdge *)pParentCurve1->GetEdge() ;
      SmEdge  *pEdge2  = (SmEdge *)pParentCurve2->GetEdge() ;
      SmBrep  *pBrep1  = pEdge1 ? pEdge1->GetBrep() : NULL ;
      SmBrep  *pBrep2  = pEdge2 ? pEdge2->GetBrep() : NULL ;

      smgfx_Erase();
      smgfx_SetLook(1,2, 0,0,1) ; if(pBrep1) pBrep1->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,1,0) ; if(pBrep2 && pBrep1 != pBrep2) pBrep2->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3, 1,0,0) ; if(pCurve1) pCurve1->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3, 1,0,1) ; if(pCurve2) pCurve2->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 0,1,1) ; if(pEdge1) pEdge1->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 1,1,0) ; if(pEdge2) pEdge2->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(5,6, 0,1,1) ; if(pCurve1) pCurve1->Draw(&sIvl1) ; sm_GraphicsLoop() ;         // subdivide ivl
      smgfx_SetLook(5,6, 0,1,1) ; if(pCurve2) pCurve2->Draw(&sIvl2) ; sm_GraphicsLoop() ;         // subdivide ivl
      sm_GraphicsLoop();
    }
#endif // SM_DEBUG_CODE

  // get sIvl2 EndPoints and the chord between them
  SmPoint3d sLinePt2, sEnd2;
  pCurve2->EvaluatePoint(sIvl2.GetMin(),sLinePt2);
  pCurve2->EvaluatePoint(sIvl2.GetMax(),sEnd2);
  SmVector3d sLineVec2 = sEnd2 - sLinePt2;

  // GWC::CLOSED_CURVE_INTERSECTIONS - added endContinuity check for closed Curves
  // adjust continuity for boundary endSpans on closed curves
  SmExtent1d sParentCurve1Interval = pParentCurve1->GetNaturalInterval() ;
  SmExtent1d sParentCurve2Interval = pParentCurve2->GetNaturalInterval() ;

  // if     either end of span1 is at the curve1's natural spanEnd
  //    and we are intersecting the entire curve1
  //    and curve1 is closed
  if(   (   SM_IS_ZERO( sIvl1.GetMin() - sParentCurve1Interval.GetMin())
         || SM_IS_ZERO( sIvl1.GetMax() - sParentCurve1Interval.GetMax()))
     && (   SM_IS_ZERO( m_crInterval1.GetMin() - sParentCurve1Interval.GetMin())
         && SM_IS_ZERO( m_crInterval1.GetMax() - sParentCurve1Interval.GetMax()))
     && pParentCurve1->IsClosed(sParentCurve1Interval))
    {
      // get pCurve1's end continuity
      SmContinuityType sContinuityType = SM_CT_DISCONTINUOUS ;
      pParentCurve1->EvaluateContinuity(sParentCurve1Interval.GetMin(), FALSE,
                                        *pParentCurve1, sParentCurve1Interval.GetMax(), TRUE,
                                        sContinuityType) ;
      // set span endContinuities as appropriate
      if(SM_IS_ZERO( sIvl1.GetMin() - sParentCurve1Interval.GetMin()))
        { aeConts1[0] = sContinuityType ;
        }
      if(SM_IS_ZERO( sIvl1.GetMax() - sParentCurve1Interval.GetMax()))
        { aeConts1[1] = sContinuityType ;
        }
    } // end span1 is endSpan on closed curve1 check

  // if     either end of span2 is at the curve2's natural spanEnd
  //    and we are intersecting the entire curve2
  //    and curve2 is closed
  if(   (   SM_IS_ZERO( sIvl2.GetMin() - sParentCurve2Interval.GetMin())
         || SM_IS_ZERO( sIvl2.GetMax() - sParentCurve2Interval.GetMax()))
     && (   SM_IS_ZERO( m_crInterval2.GetMin() - sParentCurve2Interval.GetMin())
         && SM_IS_ZERO( m_crInterval2.GetMax() - sParentCurve2Interval.GetMax()))
     && pParentCurve2->IsClosed(sParentCurve2Interval))
    {
      // get pCurve2's end continuity
      SmContinuityType sContinuityType = SM_CT_DISCONTINUOUS ;
      pParentCurve2->EvaluateContinuity(sParentCurve2Interval.GetMin(), FALSE,
                                        *pParentCurve2, sParentCurve2Interval.GetMax(), TRUE,
                                        sContinuityType) ;
      // set span endContinuities as appropriate
      if(SM_IS_ZERO( sIvl2.GetMin() - sParentCurve2Interval.GetMin()))
        { aeConts2[0] = sContinuityType ;
        }
      if(SM_IS_ZERO( sIvl2.GetMax() - sParentCurve2Interval.GetMax()))
        { aeConts2[1] = sContinuityType ;
        }
    } // end span2 is endSpan on closed curve2 check

#ifdef SM_DEBUG_CODE
  if (bDebugMe)
    {
      SmEdge  *pEdge1  = (SmEdge *)pParentCurve1->GetEdge() ;
      SmEdge  *pEdge2  = (SmEdge *)pParentCurve2->GetEdge() ;
      SmBrep  *pBrep1  = pEdge1 ? pEdge1->GetBrep() : NULL ;
      SmBrep  *pBrep2  = pEdge2 ? pEdge2->GetBrep() : NULL ;

      smgfx_Erase();
      smgfx_SetLook(1,2, 0,0,1) ; if(pBrep1) pBrep1->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,1,0) ; if(pBrep2 && pBrep1 != pBrep2) pBrep2->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3, 1,0,0) ; if(pCurve1) pCurve1->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3, 1,0,1) ; if(pCurve2) pCurve2->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 0,1,1) ; if(pEdge1) pEdge1->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 1,1,0) ; if(pEdge2) pEdge2->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(5,6, 0,1,1) ; if(pCurve1) pCurve1->Draw(&sIvl1) ; sm_GraphicsLoop() ;         // subdivide ivl
      smgfx_SetLook(5,6, 0,1,1) ; if(pCurve2) pCurve2->Draw(&sIvl2) ; sm_GraphicsLoop() ;         // subdivide ivl
      sm_GraphicsLoop();
    }
#endif // SM_DEBUG_CODE

  // get angle between intervals as angle between interval EndPoint Chords
  // to determine when multiple intersections are possible
  // let angle between very short intervals = 2*PI
  double dAngRad;
  if (   sLineVec1.LengthSquared() < SM_EFF_ZERO_SQ
      || sLineVec2.LengthSquared() < SM_EFF_ZERO_SQ)
    {
      // GWC:CHANGED_ONE_LINE
      dAngRad = SM_PI/2.0;
      // dAngRad = 2.0*SM_PI;   // If we have something this small then we
                                // should have only one intersection.
    }
  else
    {
      SER( sLineVec1.AngleBetween( sLineVec2, dAngRad ));
    }

  // mirror angles into the 1st quadrant
  if ( dAngRad > SM_PI / 2 ) { dAngRad = SM_PI - dAngRad; }

  double dAngDeg = SM_RAD2DEG( dAngRad );

  // get combined span turning angle
  double dCombinedTurnAngle =   pBezSpan1->m_dMaxTurningAngleDeg
                              + pBezSpan2->m_dMaxTurningAngleDeg;

  // multiple xsects possible when spanAngle is less than turning angle
  SmBoolean bOnlyOneIntersectionPossible = FALSE;

  // Special case for two linear spans.
  if ( dCombinedTurnAngle < SM_EFF_ZERO * 100.0 )
    {
      // Two 'lines'.  Either a single intersection, or a range solution.
      // Could be Range if the change in separation along the lengths
      // is less than our tolerance.
      // The maximum change in separation would be the shorter line length
      // times the angle in radians.
      double dLen = smos_Min( sLineVec1.Length(), sLineVec2.Length() );
      double dMaxChange = dLen * dAngRad;
      bOnlyOneIntersectionPossible = ( dMaxChange > m_d3dTolerance );
    }
  else
    {
      // General case.
      // (Use 100*tol because degrees not radians (57 deg/rad plus a bit).)
      // [Fillet regression test 2:222]
      double dScaledZero = 100.0 * SM_EFF_ZERO
                          * (1.0 + smos_Max(sLineVec1.GetMaxDimension(),
                                            sLineVec2.GetMaxDimension())) ;
      bOnlyOneIntersectionPossible =
            (dAngDeg * 0.99 > dCombinedTurnAngle + dScaledZero) ;
    }

  // when selfIntersecting - cull cases with no intersections possible
  if (m_bSelfIntersection)
    {
      // when intersecting a neighboring span
      if (   pBezSpan1->m_lLeft == pBezSpan2->m_lLeft-1     // 1 span just before 2
          || pBezSpan1->m_lLeft == pBezSpan2->m_lLeft+1)    // 2 span just before 1
        {
          // when only 1 intersection is possible
          if (bOnlyOneIntersectionPossible)
            {
              // eliminate spans that run in the same direction - they can't intersect
              if (sLineVec1.Dot(sLineVec2) > 0.0)
                {
                  return SM_SUCCESS;
                } // end similarly directed span check
            } // end onlyOneIntersectionPossible check
        } // end intersecting with next or previous span
    } // end m_bSelfIntersection == TRUE check

// GWC:REWRITE SECTION
  // Do a quick test to see if the the end points of nonZero length intervals correspond
  // in such a way that we don't need to do additional testing.
  if(   sLineVec1.LengthSquared() >= SM_EFF_ZERO_SQ
     && sLineVec2.LengthSquared() >= SM_EFF_ZERO_SQ
     && (   aeConts1[0] == SM_CT_DISCONTINUOUS
         || aeConts1[1] == SM_CT_DISCONTINUOUS)
     && (   aeConts2[0] == SM_CT_DISCONTINUOUS
         || aeConts2[1] == SM_CT_DISCONTINUOUS))
    {
      // for all endPoint combinations
      ULONG j1, j2;
      for (j1=0; j1<2; j1++)
        {
          for (j2=0; j2<2; j2++)
            {
              if (   aeConts1[j1] == SM_CT_DISCONTINUOUS
                  && aeConts2[j2] == SM_CT_DISCONTINUOUS)
                {
                  SmPoint3d  sPt1, sPt2 ;
                  double     dT1,  dT2 ;
                  SmVector3d sDir1, sDir2 ;

                  // get span1 endPoint locals
                  if(j1 == 0) { sPt1  =  sLinePt1;
                                dT1   =  sIvl1.GetMin();
                                sDir1 =  sLineVec1;
                              }
                  else        { sPt1  =  sEnd1;
                                dT1   =  sIvl1.GetMax();
                                sDir1 = -sLineVec1;
                              }

                  // get span2 endPoint locals
                  if(j2 == 0) { sPt2  =  sLinePt2;
                                dT2   =  sIvl2.GetMin();
                                sDir2 =  sLineVec2;
                              }
                  else        { sPt2  =  sEnd2;
                                dT2   =  sIvl2.GetMax();
                                sDir2 = -sLineVec2;
                              }

                  // get distance between endPoints
                  SmSolution sSol;
                  sSol.m_vStart.m_dSolutionValue = sPt1.DistanceBetween(sPt2);

                  // when endPoints are within tolerance of each other
                  if (sSol.m_vStart.m_dSolutionValue < m_d3dTolerance)
                    {
                      // look for cases where we know there are intersections
                      ULONG bTestOne = FALSE;

                      ULONG lDegree1 = pCurve1->GetDegree();
                      ULONG lDegree2 = pCurve2->GetDegree();
                      if (   bOnlyOneIntersectionPossible
                          || (   sDir1.Dot(sDir2)   < -SM_EFF_ZERO
                              && dCombinedTurnAngle < 90.0) )
                        {
                          bTestOne = TRUE;
                        }
                      else if (   lDegree1 <= 2
                               && lDegree2 <= 2
                               && !bOnlyOneIntersectionPossible)
                        {
                          // We have a low total degree situation see if we can eliminate
                          // the need to go further in those situations where the curves are
                          // tangent to each other at the points.

                          // get span endPoint positions and tangents
                          SmVector3d sPV1[2], sPV2[2];
                          SER(pCurve1->Evaluate(dT1,1,TRUE,sPV1,TRUE));  // nonZeroTangent values
                          SER(pCurve2->Evaluate(dT2,1,TRUE,sPV2,TRUE));  // nonZeroTangent values
                          SER(sPV1[1].Unitize());
                          SER(sPV2[1].Unitize());

                          // get span EndPoint and chord EndPoint tangent crossProducts
                          SmVector3d sCross = sPV1[1] * sPV2[1];
                          SER(sDir1.Unitize());
                          SER(sDir2.Unitize());
                          SmVector3d sCross2 = sDir1 * sDir2;

                          // when both sets of tangents are parallel
                          if (   sCross.LengthSquared() < SM_EFF_ZERO_SQ
                              && sCross2.LengthSquared() > SM_EFF_ZERO_SQ)
                            {
                              bTestOne = TRUE;
                            }
                        } // end low degree, multiple intersection case check

                      // when these endPoints
                      if (bTestOne)
                        {
                          sSol.m_eSolutionType = SM_ST_SINGLE_VALUE;
                          sSol.m_lNumVariables = 2;
                          sSol.m_apNodes[0]    = apBranch[0];  // Curve1 node
                          sSol.m_apNodes[1]    = apBranch[1];  // Curve2 node
                          sSol.m_vStart[0]     = dT1;
                          sSol.m_vStart[1]     = dT2;
                          sSol.m_lNumObjects   = 2 ;
                          sSol.m_apObjects[0]  = (SmObject *)pParentCurve1 ;
                          sSol.m_apObjects[1]  = (SmObject *)pParentCurve2 ;

                          // get closest point between 2 lines
                          double dParam1, dParam2;
                          if (smgu_LineLineClosestPoint(sLinePt1,sLineVec1,sLinePt2,sLineVec2,
                              dParam1,dParam2) == SM_SUCCESS)
                            {
                              // If the intersection occurs at or beyond the
                              // interval ends then the curve ends are to be
                              // the intersection.  Otherwise we have to
                              // continue and do iteration.
                              if (   (   dParam1 <     SM_EFF_ZERO_SQRT
                                      || dParam1 > 1.0-SM_EFF_ZERO_SQRT)
                                  && (   dParam2 <     SM_EFF_ZERO_SQRT
                                      || dParam2 > 1.0-SM_EFF_ZERO_SQRT))
                                {
                                  AddSortedSolution(sSol,SM_SK_BY_ALL_PARAMETERS, TRUE);
                                  return SM_SUCCESS;
                                }
                            } // end FoundIntersection between two lines check
                          else if (sDir1.Dot(sDir2) < SM_EFF_ZERO)
                            {
                              // Curves are parallel and point in opposite direction.
                              // do a line test/point test.
                              SER(smgu_LineClosestPoint(sLinePt1,sLineVec1,sPt2,dParam1));
                              if (   dParam1 < SM_EFF_ZERO
                                  || dParam1 > 1.0-SM_EFF_ZERO)
                                {
                                  // there is a small gap between these two intervals - count as an intersection
                                  AddSortedSolution(sSol,SM_SK_BY_ALL_PARAMETERS, TRUE);
                                  return SM_SUCCESS;
                                }
                            }
                        } // end bTestOne == TRUE check
                    }  // end endPoints are closer than tolerance apart check

                } // end both current endPoints are discontinuous check
            } // end iter both span2 endPoints
        } // end iter both span1 EndPoints
    } // end both spans are nonZero length check

#ifdef SM_DEBUG_CODE
  if (bDebugMe)
    {
      SmEdge  *pEdge1  = (SmEdge *)pParentCurve1->GetEdge() ;
      SmEdge  *pEdge2  = (SmEdge *)pParentCurve2->GetEdge() ;
      SmBrep  *pBrep1  = pEdge1 ? pEdge1->GetBrep() : NULL ;
      SmBrep  *pBrep2  = pEdge2 ? pEdge2->GetBrep() : NULL ;

      smgfx_Erase();
      smgfx_SetLook(1,2, 0,0,1) ; if(pBrep1) pBrep1->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,1,0) ; if(pBrep2 && pBrep1 != pBrep2) pBrep2->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3, 1,0,0) ; if(pCurve1) pCurve1->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3, 1,0,1) ; if(pCurve2) pCurve2->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 0,1,1) ; if(pEdge1) pEdge1->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 1,1,0) ; if(pEdge2) pEdge2->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(5,6, 0,1,1) ; if(pCurve1) pCurve1->Draw(&sIvl1) ; sm_GraphicsLoop() ;         // subdivide ivl
      smgfx_SetLook(5,6, 0,1,1) ; if(pCurve2) pCurve2->Draw(&sIvl2) ; sm_GraphicsLoop() ;         // subdivide ivl
      sm_GraphicsLoop();
    }
#endif // SM_DEBUG_CODE

  // Do a coincidence test if more than one intersection is possible
  if (!bOnlyOneIntersectionPossible)
    {
#ifdef SM_TOLERANT_XSECT
      SmSolutionArray sSolutions ;
      SmBoolean bAdditionalWorkNeeded ;
      SmBoolean bSnapToEnds = TRUE ; // this choice needs to be reviewed

      // find coincident point or range(s)
      SER(pCurve1->SimpleCoincidenceChecker(sIvl1, *pCurve2, sIvl2, m_d3dTolerance,
                                            &sPseudoBox1, &sPseudoBox2,
                                            bAdditionalWorkNeeded, sSolutions,
                                            bSnapToEnds));


      ULONG       lNumFound  = sSolutions.GetSize();
      SmSolution *aSolutions = sSolutions.GetDataArray();

#else  // NO SM_TOLERANT_XSECT
      SmSolution aSolutions[2];
      ULONG lNumFound=0;

      // GWC:Updated PseudoBoxes so that they are projected to common plane - no need to skip that check any longer
      // if (m_bDoProjection)
      //   { // find coincident point or range(s)
      //     SER(pCurve1->SimpleCoincidenceChecker(sIvl1, *pCurve2, sIvl2,
      //                                           m_d3dTolerance,
      //                                           NULL, NULL,
      //                                           lNumFound, aSolutions));
      //   }
      // else
      //   { // find coincident point or range(s)
      //     SER(pCurve1->SimpleCoincidenceChecker(sIvl1, *pCurve2, sIvl2,
      //                                           m_d3dTolerance,
      //                                           &sPseudoBox1, &sPseudoBox2,
      //                                           lNumFound, aSolutions));
      //   }

      // find coincident point or range(s)
      SER(pCurve1->SimpleCoincidenceChecker(sIvl1, *pCurve2, sIvl2,
                                            m_d3dTolerance,
                                            &sPseudoBox1, &sPseudoBox2,
                                            lNumFound, aSolutions));
#endif // NO SM_TOLERANT_XSECT

      // when a coincident solution was found (either point or range)
      SmBoolean bFoundCoin = FALSE;
      ULONG kk;
      for (kk=0; kk<lNumFound; kk++)
        {
          aSolutions[kk].m_lNumVariables = 2;
          aSolutions[kk].m_apNodes[0]    = apBranch[0];  // Curve1 node
          aSolutions[kk].m_apNodes[1]    = apBranch[1];  // Curve2 node
          aSolutions[kk].m_lNumObjects   = 2 ;
          aSolutions[kk].m_apObjects[0]  = (SmObject *)pParentCurve1 ;
          aSolutions[kk].m_apObjects[1]  = (SmObject *)pParentCurve2 ;

          // always save range solutions
          if (aSolutions[kk].m_eSolutionType == SM_ST_RANGE_OF_VALUES)
            {
              bFoundCoin = TRUE;
              // add solution to SmGlobalSolver::m_pSolutions
              AddSortedSolution(aSolutions[kk],SM_SK_BY_ALL_PARAMETERS, TRUE);
            }
          else
            { // Single valued solutions
              // If SimpleCoincidenceChecker() finds an end-point solution,
              // it will just return that.  Find the closest approach. [B606 xhatch]
              SmSolution &rSol = aSolutions[kk];
              if ( rSol.m_vStart.m_dSolutionValue > m_d3dTolerance / 100.0 ) // just rule out really tight ones.
                {
                  SmBoolean bFoundInt;
                  double dParam1, dParam2, dDeviation;
                  SER( pCurve1->LocalCurveIntersect( sIvl1, *pCurve2, sIvl2,
                                                     m_d3dTolerance,
                                                     rSol.m_vStart[0],
                                                     rSol.m_vStart[1],
                                                     bFoundInt,
                                                     dParam1, dParam2, dDeviation ));

                  if ( bFoundInt && dDeviation < rSol.m_vStart.m_dSolutionValue )
                    {
                      rSol.m_vStart[0] = dParam1;
                      rSol.m_vStart[1] = dParam2;
                      rSol.m_vStart.m_dSolutionValue = dDeviation;
                    }
                }

              // Now check the refined solution.
              SmBoolean bKeepSolution;

              // filter solutions
              SER(sm_FilterSolution(*pCurve1,sIvl1,*pCurve2,sIvl2,
                                     aeConts1,aeConts2,lAdjacency,
                                     aSolutions[kk],bKeepSolution));

              // save aSolutions, which should be kept, to m_pSolutions
              if (bKeepSolution)
                {
                  // add solution to SmGlobalSolver::m_pSolutions
                  AddSortedSolution(aSolutions[kk],SM_SK_BY_ALL_PARAMETERS, TRUE);
                  // No, don't. [091211, 091215; also my_test_cci_tol, my_test_cci_adv]
                  // bFoundCoin = TRUE; // formerly done only for range solutions [091023]
                }
            }
        }

      // If we found a coincidence we are done
      if (bFoundCoin) { return SM_SUCCESS; }

    } // end multiple intersection possible check

  double daGuess1[10], daGuess2[10];
  ULONG lNumGuess = 0;

  // Do a little testing to see if we are just barely intersecting
  // each other at a corner point or something
  if (   !bOnlyOneIntersectionPossible
      && !m_bDoProjection)     // GWC: don't yet project SmExtent3d to common plane - so skip this optimization for projection problems.
    {
      SmExtent3d sIntBBox;
      if (pCurveNode1->m_sBBox.AreDisjoint(pCurveNode2->m_sBBox))
        {
          // Temporarily remove this optimization because it causes problems
          // when curves are disjoint by a plane not a corner.
//            bOnlyOneIntersectionPossible = TRUE;
          bOnlyOneIntersectionPossible = FALSE;
        }
      else
        {
          SmStatus eStat = pCurveNode1->m_sBBox.Intersect( pCurveNode2->m_sBBox, sIntBBox );

          if ( eStat == SM_SUCCESS )
            {
              // If all three dimensions are zero then only one possible intersection.
              // (Can have multiple intersections if only one dimension is not zero.)
              ULONG lNumZeroDimensions = 0;
              double dTol = SM_EFF_ZERO * (1.0 + sIntBBox.GetMax().GetMaxDimension());
              if (sIntBBox.GetMax().x - sIntBBox.GetMin().x < dTol) {lNumZeroDimensions ++;}
              if (sIntBBox.GetMax().y - sIntBBox.GetMin().y < dTol) {lNumZeroDimensions ++;}
              if (sIntBBox.GetMax().z - sIntBBox.GetMin().z < dTol) {lNumZeroDimensions ++;}
              if (lNumZeroDimensions >= 3)
                { bOnlyOneIntersectionPossible = TRUE; }
            }
        }
    } // end multiple intersections and not doingProjections check

  // look for curve EndPoint/span intersections when
  // span comes from end of curve by checking nearest distance
  // from each spanEndPoint to the other span whenever those
  // span endPoints are discontinuous.

  // If we have a total discontinuity (i.e. end of curve)
  // then we always test.  If we have a derivative
  // discontinuity then we just test the ones on the left
  // side - otherwise we would do each point two times.
  SmBoolean bFoundEndAnswer = FALSE;
  SmBoolean bFoundAnswer;
  SmSolution sPointSolution;
  SmSolution sSolution2;
  sSolution2.m_lNumVariables = 2;
  sSolution2.m_eSolutionType = SM_ST_SINGLE_VALUE;
  sSolution2.m_apNodes[0]    = apBranch[0];  // Curve1 node
  sSolution2.m_apNodes[1]    = apBranch[1];  // Curve2 node
  sSolution2.m_lNumObjects   = 2 ;
  sSolution2.m_apObjects[0]  = (SmObject *)pParentCurve1 ;
  sSolution2.m_apObjects[1]  = (SmObject *)pParentCurve2 ;
  SmSolutionArray sEndPointSolutions ;

  // note: pCurve1 and pCurve2 are curves being intersected after optional projection
  //       pParentCurve1 and pParentCurve2 are curves being intersected before optional projection.

  // when start of span1 is a Curve endPoint - look for a solution from endPoint to span2
  if (   aeConts1[0] <= SM_CT_C0
      && sPseudoBox2.DistanceToPoint(sLinePt1) < m_d3dTolerance)
    {
      // find min distance between span2 and span1-StartPoint
      pCurve2->LocalPointSolve(sIvl2,               // in : search interval
                               SM_SO_MINIMIZE,      // in : specify specific operation to optimize
                               sLinePt1,            // in : point specializing this search
                               &m_d3dTolerance,     // in : Opt Max allowed solution distance (NULL to ignore) 
                                                    //      for SM_SO_RAYFIRE
                                                    //          SM_SO_3D_SIGNED_DIRECTED_MINIMIZE
                               NULL,                // in : required curve/TestPoint desired dist (NULL when not used)
                                                    //      for SM_SO_AT_DISTANCE
                               m_cpOptVectors,      // in : required Vector direction (NULL when not used)
                                                    //      for SM_SO_RAYFIRE
                                                    //          SM_SO_3D_SIGNED_DIRECTED_MINIMIZE
                               sIvl2.Evaluate(0.5), // in : search starting parameter
                               bFoundAnswer,        // out: TRUE=converged,FALSE=didn't
                               sPointSolution) ;    // out: solution container for solver

      // when start point is within tolerance
      if (   bFoundAnswer
          && sPointSolution.m_vStart.m_dSolutionValue < m_d3dTolerance)
        {
          // when start point is tightly within tolerance or closest span2 point is in the interior
          if (   sPointSolution.m_vStart.m_dSolutionValue < m_d3dTolerance/100.0
              || !sIvl2.IsValueOnBoundary(sPointSolution.m_vStart[0]))
            {
              // add this StartPoint/span2 solution to m_pSolutions array
              sSolution2.m_vStart.m_dSolutionValue = sPointSolution.m_vStart.m_dSolutionValue;
              sSolution2.m_vStart[0]               = sIvl1.GetMin();
              sSolution2.m_vStart[1]               = sPointSolution.m_vStart[0];
              bFoundEndAnswer = TRUE;

              // look for exit conditions
              if (bOnlyOneIntersectionPossible)
                {
                  SmPoint3d sCrvPnt;
                  double dParam;
                  SER(pCurve2->EvaluatePoint(sPointSolution.m_vStart[0],sCrvPnt));
                  if (sLineVec1.LengthSquared() < SM_EFF_ZERO_SQ)
                    {
                      // add solution to SmGlobalSolver::m_pSolutions
                      AddSortedSolution(sSolution2,SM_SK_BY_ALL_PARAMETERS, TRUE);
                      return SM_SUCCESS;
                    }
                  SER(smgu_LineClosestPoint(sLinePt1,sLineVec1,sCrvPnt,dParam));
                  if (   dParam < SM_EFF_ZERO
                      || dParam > 1.0-SM_EFF_ZERO)
                    {
                      // add solution to SmGlobalSolver::m_pSolutions
                      AddSortedSolution(sSolution2,SM_SK_BY_ALL_PARAMETERS, TRUE);
                      return SM_SUCCESS;
                    } // end
                } // end looking for exit conditions for bOnlyOneIntersectionPossible cases check

              // accumulate this solution for possible later use
              sEndPointSolutions.Add(sSolution2) ;

            } // end found a very tight closest point or an interior span2 point check
        } // end found a closest point within tolerance check
    } // end aeConts1[0] <= SM_CT_C0 check

  // when end of span1 is a Curve endPoint - look for a solution from endPoint to span2
  if (   aeConts1[1] <= SM_CT_DISCONTINUOUS
      && sPseudoBox2.DistanceToPoint(sEnd1) < m_d3dTolerance)
    {
      // find min distance between span2 and span1 EndPoint
      pCurve2->LocalPointSolve(sIvl2,SM_SO_MINIMIZE,sEnd1,&m_d3dTolerance,NULL,m_cpOptVectors,
          sIvl2.Evaluate(0.5),bFoundAnswer,sPointSolution);

      // when end point is within tolerance
      if (bFoundAnswer && sPointSolution.m_vStart.m_dSolutionValue < m_d3dTolerance)
        {
          // when end point is tightly within tolerance or closest span2 point is in the interior
          if (   sPointSolution.m_vStart.m_dSolutionValue < m_d3dTolerance/100.0
              || !sIvl2.IsValueOnBoundary(sPointSolution.m_vStart[0]))
            {
              // add this EndPoint/span2 solution to m_pSolutions array
              sSolution2.m_vStart.m_dSolutionValue = sPointSolution.m_vStart.m_dSolutionValue;
              sSolution2.m_vStart[0] = sIvl1.GetMax();
              sSolution2.m_vStart[1] = sPointSolution.m_vStart[0];
              bFoundEndAnswer = TRUE;

              // look for exit conditions
              if (bOnlyOneIntersectionPossible)
                {
                  SmPoint3d sCrvPnt;
                  double dParam;
                  SER(pCurve2->EvaluatePoint(sPointSolution.m_vStart[0],sCrvPnt));
                  if (sLineVec1.LengthSquared() < SM_EFF_ZERO_SQ)
                    {
                      // add solution to SmGlobalSolver::m_pSolutions
                      AddSortedSolution(sSolution2,SM_SK_BY_ALL_PARAMETERS, TRUE);
                      return SM_SUCCESS;
                    }
                  SER(smgu_LineClosestPoint(sLinePt1,sLineVec1,sCrvPnt,dParam));
                  if (dParam < SM_EFF_ZERO || dParam > 1.0-SM_EFF_ZERO)
                    {
                      // add solution to SmGlobalSolver::m_pSolutions
                      AddSortedSolution(sSolution2,SM_SK_BY_ALL_PARAMETERS, TRUE);
                      return SM_SUCCESS;
                    } // end
                } // end looking for exit conditions for bOnlyOneIntersectionPossible cases check

              // accumulate this solution for possible later use
              sEndPointSolutions.Add(sSolution2) ;

            } // end found a very tight closest point or an interior span2 point check
        } // end found a closest point within tolerance check
    } // end aeConts1[1] <= SM_CT_DISCONTINUOUS check

  // when start of span2 is a Curve endPoint - look for a solution from endPoint to span2
  if (   aeConts2[0] <= SM_CT_C0
      && sPseudoBox1.DistanceToPoint(sLinePt2) < m_d3dTolerance)
    {
      // find min distance between span1 and span2 StartPoint
      pCurve1->LocalPointSolve(sIvl1,SM_SO_MINIMIZE,sLinePt2,&m_d3dTolerance,NULL,m_cpOptVectors,
          sIvl1.Evaluate(0.5),bFoundAnswer,sPointSolution);

      // when start point is within tolerance
      if (bFoundAnswer && sPointSolution.m_vStart.m_dSolutionValue < m_d3dTolerance)
        {
          // when start point is tightly within tolerance or closest span1 point is in the interior
          if (   sPointSolution.m_vStart.m_dSolutionValue < m_d3dTolerance/100.0
              || !sIvl1.IsValueOnBoundary(sPointSolution.m_vStart[0]))
            {
              // add this StartPoint/span2 solution to m_pSolutions array
              sSolution2.m_vStart.m_dSolutionValue = sPointSolution.m_vStart.m_dSolutionValue;
              sSolution2.m_vStart[0]               = sPointSolution.m_vStart[0];
              sSolution2.m_vStart[1]               = sIvl2.GetMin();
              bFoundEndAnswer = TRUE;

              // look for exit conditions
              if (bOnlyOneIntersectionPossible)
                {
                  SmPoint3d sCrvPnt;
                  double dParam;
                  SER(pCurve1->EvaluatePoint(sPointSolution.m_vStart[0],sCrvPnt));
                  if (sLineVec2.LengthSquared() < SM_EFF_ZERO_SQ)
                    {
                      // add solution to SmGlobalSolver::m_pSolutions
                      AddSortedSolution(sSolution2,SM_SK_BY_ALL_PARAMETERS, TRUE);
                      return SM_SUCCESS;
                    }
                  SER(smgu_LineClosestPoint(sLinePt2,sLineVec2,sCrvPnt,dParam));
                  if (dParam < SM_EFF_ZERO || dParam > 1.0-SM_EFF_ZERO)
                    {
                      // add solution to SmGlobalSolver::m_pSolutions
                      AddSortedSolution(sSolution2,SM_SK_BY_ALL_PARAMETERS, TRUE);
                      return SM_SUCCESS;
                    } // end
                } // end looking for exit conditions for bOnlyOneIntersectionPossible cases check

              // accumulate this solution for possible later use
              sEndPointSolutions.Add(sSolution2) ;

            } // end found a very tight closest point or an interior span2 point check
        } // end found a closest point within tolerance check
    } // end aeConts2[0] <= SM_CT_C0 check

  // when end of span2 is a Curve endPoint - look for a solution from endPoint to span2
  if (   aeConts2[1] <= SM_CT_DISCONTINUOUS
      && sPseudoBox1.DistanceToPoint(sEnd2) < m_d3dTolerance)
    {
      // find min distance between span2 and span1 EndPoint
      pCurve1->LocalPointSolve(sIvl1,SM_SO_MINIMIZE,sEnd2,&m_d3dTolerance,NULL,m_cpOptVectors,
          sIvl1.Evaluate(0.5),bFoundAnswer,sPointSolution);

      // when end point is within tolerance
      if (bFoundAnswer && sPointSolution.m_vStart.m_dSolutionValue < m_d3dTolerance)
        {
          // when end point is tightly within tolerance or closest span1 point is in the interior
          if (   sPointSolution.m_vStart.m_dSolutionValue < m_d3dTolerance/100.0
              || !sIvl1.IsValueOnBoundary(sPointSolution.m_vStart[0]))
            {
              // add this EndPoint/span1 solution to m_pSolutions array
              sSolution2.m_vStart.m_dSolutionValue = sPointSolution.m_vStart.m_dSolutionValue;
              sSolution2.m_vStart[0]               = sPointSolution.m_vStart[0];
              sSolution2.m_vStart[1]               = sIvl2.GetMax();
              bFoundEndAnswer = TRUE;

              // look for exit conditions
              if (bOnlyOneIntersectionPossible)
                {
                  SmPoint3d sCrvPnt;
                  double dParam;
                  SER(pCurve1->EvaluatePoint(sPointSolution.m_vStart[0],sCrvPnt));
                  if (sLineVec2.LengthSquared() < SM_EFF_ZERO_SQ)
                    {
                      // add solution to SmGlobalSolver::m_pSolutions
                      AddSortedSolution(sSolution2,SM_SK_BY_ALL_PARAMETERS, TRUE);
                      return SM_SUCCESS;
                    }
                  SER(smgu_LineClosestPoint(sLinePt2,sLineVec2,sCrvPnt,dParam));
                  if (dParam < SM_EFF_ZERO || dParam > 1.0-SM_EFF_ZERO)
                    {
                      // add solution to SmGlobalSolver::m_pSolutions
                      AddSortedSolution(sSolution2,SM_SK_BY_ALL_PARAMETERS, TRUE);
                      return SM_SUCCESS;
                    } // end
                } // end looking for exit conditions for bOnlyOneIntersectionPossible cases check

              // accumulate this solution for possible later use
              sEndPointSolutions.Add(sSolution2) ;

            } // end found a very tight closest point or an interior span2 point check
        } // end found a closest point within tolerance check
    } // end aeConts1[1] <= SM_CT_DISCONTINUOUS check

  // If only one segment possible then choose an intelligent guess point.
  if (bOnlyOneIntersectionPossible)
    {
      double dT1, dT2;
      if(smgu_LineLineClosestPoint(sLinePt1,sLineVec1,
                                   sLinePt2,sLineVec2,dT1,dT2) != SM_SUCCESS)
        {
          dT1 = 0.5; dT2 = 0.5;  // Just take a midpoint if this fails
        }
      SmExtent1d sNormIvl(0.0,1.0);
      daGuess1[0] = sIvl1.Evaluate(sNormIvl.ClampValue(dT1));
      daGuess2[0] = sIvl2.Evaluate(sNormIvl.ClampValue(dT2));
      lNumGuess = 1;
    }
  else // multiple intersections are possible - try an array of guess points
    {
      // First put in mid point
      daGuess1[0] = sIvl1.Evaluate(0.5);
      daGuess2[0] = sIvl2.Evaluate(0.5);
      // Now an end point
      daGuess1[1] = sIvl1.Evaluate(0.0);
      daGuess2[1] = sIvl2.Evaluate(0.0);
      // Now an end point
      daGuess1[2] = sIvl1.Evaluate(0.0);
      daGuess2[2] = sIvl2.Evaluate(1.0);
      // Now an end point
      daGuess1[3] = sIvl1.Evaluate(1.0);
      daGuess2[3] = sIvl2.Evaluate(0.0);
      // Now an end point
      daGuess1[4] = sIvl1.Evaluate(1.0);
      daGuess2[4] = sIvl2.Evaluate(1.0);
      lNumGuess = 5;
    }

  // exit condition: when either curve is degenerate
  if (   pCurve1->IsDegenerate()
      || pCurve2->IsDegenerate())
    {
      // to preserve previous behavior - add in all accumulated EndPointSolutions
      ULONG ii;
      for (ii=0;ii<sEndPointSolutions.GetSize();ii++)
        {
          // add solution to SmGlobalSolver::m_pSolutions
          AddSortedSolution(sSolution2,SM_SK_BY_ALL_PARAMETERS, TRUE);
        }
      return SM_SUCCESS;
    }

  SmBoolean bAddedSolution = FALSE ;

  // for every guess point
  ULONG i;
  for (i=0; i<lNumGuess; i++)
    {
      // span/span NewtonRaphson Intersection locals
      SmBoolean bFoundSolution;
      double dMyParameter, dOtherParameter, dDeviation;

      // execute span/span NewtonRaphson Intersection
      SER(pCurve1->LocalCurveIntersect(sIvl1,
                                       *pCurve2, sIvl2,
                                       m_d3dTolerance,
                                       daGuess1[i], daGuess2[i],
                                       bFoundSolution, dMyParameter, dOtherParameter, dDeviation));

      // when a solution was found
      if (bFoundSolution)
        {
          // place the solution into an SmSolution object.
          SmSolution sSolution;
          sSolution.m_lNumVariables           = 2;
          sSolution.m_vStart[0]               = dMyParameter;
          sSolution.m_vStart[1]               = dOtherParameter;
          sSolution.m_vStart.m_dSolutionValue = dDeviation;
          sSolution.m_eSolutionType           = SM_ST_SINGLE_VALUE;
          sSolution.m_apNodes[0]              = apBranch[0];  // Curve1 node
          sSolution.m_apNodes[1]              = apBranch[1];  // Curve2 node
          sSolution.m_lNumObjects             = 2 ;
          sSolution.m_apObjects[0]            = (SmObject *)pParentCurve1 ;
          sSolution.m_apObjects[1]            = (SmObject *)pParentCurve2 ;
#ifdef SM_DEBUG_CODE
          // draw intersection point
          if (bDebugMe)
            {
              sm_GraphicsLoop();
              smgfx_SetLook(5,7, 1,0,0); pCurve1->DrawAt(sSolution.m_vStart[0],0); sm_GraphicsLoop();
              sm_GraphicsLoop();
            }
#endif // SM_DEBUG_CODE

          // Filter out intersections which are actually outside of the boundary
          // of the curve and are not of the tangent sort
          SmBoolean bKeepSolution = FALSE;
          SER(sm_FilterSolution(*pCurve1,sIvl1,*pCurve2,sIvl2,aeConts1,aeConts2,lAdjacency,
              sSolution,bKeepSolution));

          // skip not-to-be-kept solutions
          if (!bKeepSolution) { continue; }

          // Filter out the nasty end case where the intersection converges to
          // the end of the curve but there is a minimization of a point intersection
          // which is closer near the end.
          if (   sIvl1.IsValueOnBoundary(dMyParameter)
              && sIvl2.IsValueOnBoundary(dOtherParameter))
            {
              if (   bFoundEndAnswer
                  && dDeviation > m_d3dTolerance/100.0)
                {
                  continue;
                }
            }

#ifdef SM_DEBUG_CODE
          if (bDebugMe)
            {
              sm_GraphicsLoop();
              smgfx_SetLook(5,9,  1,0,0);
              pCurve1->DrawAt(sSolution.m_vStart[0],0); sm_GraphicsLoop();
              smgfx_SetLook(5,11, 0,0,1);
              pCurve2->DrawAt(sSolution.m_vStart[1],0); sm_GraphicsLoop();
              sm_GraphicsLoop();
            }
#endif // SM_DEBUG_CODE
          // add solution to SmGlobalSolver::m_pSolutions
          AddSortedSolution(sSolution,SM_SK_BY_ALL_PARAMETERS, TRUE);
          bAddedSolution = TRUE ;

          // when multiple solutions are possible and more iterations are coming
          if (   i < 5
              && !bOnlyOneIntersectionPossible)
            {
              // see if additional tangent guessPoints can be generated
              SmBoolean bFoundGuess;
              double dNewMyGuess, dNewOtherGuess;
              SER(sm_FindAdditionalCcTangentGuess(*pCurve1,sIvl1,
                  *pCurve2,sIvl2,dMyParameter,dOtherParameter,bFoundGuess,
                  dNewMyGuess,dNewOtherGuess));

              // when a new guessPoint was found - add it to the guessPoint array
              if (bFoundGuess)
                {
                  daGuess1[lNumGuess]   = dNewMyGuess;
                  daGuess2[lNumGuess++] = dNewOtherGuess;
                }
            } // end multiple solutions check
        } // end found a span/span NewtonRaphson intersection check
    } // end iter every guess point - looking for NewtonRaphson intersections

  // add in accumulated endPoint solutions when
  // no actual intersections were found or allowing for multiple solutions
  if(   !bAddedSolution
     || !bOnlyOneIntersectionPossible)
    {
      // to preserve previous behavior - add in all accumulated EndPointSolutions
      ULONG ii;
      for (ii=0;ii<sEndPointSolutions.GetSize();ii++)
        {
          // add solution to SmGlobalSolver::m_pSolutions
          AddSortedSolution(sEndPointSolutions[ii], SM_SK_BY_ALL_PARAMETERS, TRUE);
        }
    }

  // all done
  return SM_SUCCESS;

} // end SmCCIGlobalSolver::LocalSolve
// ---- end verbatim

// clang-format on

// Exposes the protected solver state the tests set.
template <class S> struct Open : S
{
    using S::S;
    void set(SmSolverOperationType op, double tol, const SmVector3d* vecs, SmSolutionArray* sols)
    {
        this->m_eSolverOperation = op;
        this->m_d3dTolerance = tol;
        this->m_dBestAnswerSoFarSq = SM_BIG_DOUBLE;
        this->m_cpOptVectors = vecs;
        this->m_pSolutions = sols;
    }
};
} // namespace sc_crv_p7

namespace
{
using namespace sc_crv_p7;

// Ops (keep in sync with crates/solidcore-ffi/src/units/curve_curve_p7.rs).
enum : int32_t
{
    OP_CCI_SOLVE = 0,
    OP_CCI_LOCAL = 1,
    OP_PROPERTY = 2,
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
    if (kind == 3)
    {
        // Fixed genuine ellipse; angular degrees differ from its natural [0,1] domain.
        return new (ctx) SmEllipse(SmPoint3d(0., 0., 0.), SmVector3d(1., 0., 0.),
                                  SmVector3d(0., 1., 0.), SmExtent1d(0., 180.),
                                  3., 2., 3, &ctx, NULL, FALSE);
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
    SmVector3d p1 = rd.vec();
    SmVector3d d1 = rd.vec();
    SmVector3d p2 = rd.vec();
    SmVector3d d2 = rd.vec();
    return new (ctx) SmHermiteCurve(p1, d1, p2, d2, dim);
}

// The transforms `nt (O X Y has_s S)*nt` after a curve block, applied to `c` in order.
void read_transforms(Rd& rd, SmCurve* c)
{
    int64_t nt = rd.idx();
    for (int64_t i = 0; i < nt; i++)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        bool has_s = rd.flag();
        SmVector3d s = rd.vec();
        SmAxis2Placement a2p;
        a2p.SetCanonical(o, x, y);
        c->Transform(a2p, has_s ? &s : NULL);
    }
}

// `nv V[nv]` into `vecs`; returns NULL for nv == 0.
const SmVector3d* read_vecs(Rd& rd, SmVector3d vecs[4])
{
    int64_t nv = rd.idx();
    for (int64_t i = 0; i < nv; i++)
        vecs[i] = rd.vec();
    return nv > 0 ? vecs : NULL;
}

// A curve's decomposition: `0` (the curve's object cache, as SmCurve's solvers get it) or
// `1 a b full chord angle offaxis minseg aux max3d ratio` (an SmCurveCache built and
// tessellated here). Pushes the build status; returns NULL on failure.
struct Cache
{
    SmCurveCache* own = NULL;
    SmTree* tree = NULL;
    ~Cache() { delete own; }
};

void read_cache(Rd& rd, const SmContext& ctx, SmCurve* c, Cache& out, std::vector<double>& o)
{
    if (!rd.flag())
    {
        SmCurveCache* cc = (SmCurveCache*)SmCacheMgr::GetOrCreateObjectCache(SM_OC_CURVE, c);
        out.tree = cc ? cc->GetTree() : NULL;
        o.push_back(out.tree ? 0.0 : (double)SM_ERR_NULL_POINTER);
        return;
    }
    double a = rd.num();
    double b = rd.num();
    SmBoolean full = rd.flag() ? TRUE : FALSE;
    double chord = rd.num();
    double angle = rd.num();
    double offaxis = rd.num();
    ULONG minseg = (ULONG)rd.idx();
    SmBoolean aux = rd.flag() ? TRUE : FALSE;
    double max3d = rd.num();
    double ratio = rd.num();
    out.own = new (ctx) SmCurveCache(*c, SmExtent1d(a, b), full, chord, angle, offaxis, minseg, aux,
                               max3d, ratio);
    SmStatus ts = out.own->Tessellate();
    out.tree = ts == SM_SUCCESS ? out.own->GetTree() : NULL;
    o.push_back(ts == SM_SUCCESS && out.tree == NULL ? (double)SM_ERR_NULL_POINTER : st(ts));
}

// Pre-order index of every node of `tree`; `leaves` gets the span (leaf data) nodes.
void index_nodes(SmTree* tree, std::map<const SmTreeNode*, int64_t>& idx,
                 std::vector<SmTreeNode*>* leaves)
{
    std::vector<SmTreeNode*> stack;
    if (tree && tree->GetTopNode())
        stack.push_back(tree->GetTopNode());
    while (!stack.empty())
    {
        SmTreeNode* nd = stack.back();
        stack.pop_back();
        int64_t k = (int64_t)idx.size();
        idx[nd] = k;
        if (leaves && nd->m_pData != NULL && nd->m_eAuxDataType == SM_AD_BEZIER_CURVE)
            leaves->push_back(nd);
        if (nd->m_pChild2)
            stack.push_back(nd->m_pChild2);
        if (nd->m_pChild1)
            stack.push_back(nd->m_pChild1);
    }
}

// A solution `type nvar nobj o0 o1 n0 n1 sval s[P] eval e[P]` with P = max(nvar, 2);
// objects `0` = a, `1` = b, `2` = other, `-1` = NULL; nodes are pre-order indices in the
// first / second tree (`-1` = NULL, `-2` = not found).
void put_sol(std::vector<double>& o, const SmSolution& s, const void* a, const void* b,
             std::map<const SmTreeNode*, int64_t>& n1, std::map<const SmTreeNode*, int64_t>& n2)
{
    auto id = [&](const void* p) { return p == NULL ? -1.0 : p == a ? 0.0 : p == b ? 1.0 : 2.0; };
    auto nd = [](std::map<const SmTreeNode*, int64_t>& m, const SmTreeNode* p) {
        if (p == NULL)
            return -1.0;
        auto it = m.find(p);
        return it == m.end() ? -2.0 : (double)it->second;
    };
    ULONG np = s.m_lNumVariables > 2 ? s.m_lNumVariables : 2;
    if (np > 10)
        np = 10;
    o.push_back((double)s.m_eSolutionType);
    o.push_back((double)s.m_lNumVariables);
    o.push_back((double)s.m_lNumObjects);
    o.push_back(s.m_lNumObjects > 0 ? id(s.m_apObjects[0]) : -1.0);
    o.push_back(s.m_lNumObjects > 1 ? id(s.m_apObjects[1]) : -1.0);
    o.push_back(nd(n1, s.m_apNodes[0]));
    // (a one-object solution's second node can be an uninitialised branch slot of
    // SmGlobalSolver::SolveIt, see plan/findings.md: reported only for two objects)
    o.push_back(s.m_lNumObjects > 1 ? nd(n2, s.m_apNodes[1]) : -1.0);
    o.push_back(s.m_vStart.m_dSolutionValue);
    for (ULONG i = 0; i < np; i++)
        o.push_back(s.m_vStart[i]);
    o.push_back(s.m_vEnd.m_dSolutionValue);
    for (ULONG i = 0; i < np; i++)
        o.push_back(s.m_vEnd[i]);
}

// The CCI solver set-up shared by both CCI ops: `c1 self [c2] cache1 [cache2] i1a i1b i2a i2b
// sop tol proj ptype vecs`.
void run_cci(bool local, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmCurve* c1 = read_curve(rd, ctx);
    bool self = rd.flag();
    SmCurve* c2 = self ? c1 : read_curve(rd, ctx);
    {
        Cache k1, k2;
        read_cache(rd, ctx, c1, k1, o);
        if (!self)
            read_cache(rd, ctx, c2, k2, o);
        SmTree* t1 = k1.tree;
        SmTree* t2 = self ? k1.tree : k2.tree;
        double a1 = rd.num();
        double b1 = rd.num();
        SmExtent1d ivl1(a1, b1);
        double a2 = rd.num();
        double b2 = rd.num();
        SmExtent1d ivl2(a2, b2);
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        double tol = rd.num();
        SmBoolean proj = rd.flag() ? TRUE : FALSE;
        SmProjectionType ptype = (SmProjectionType)rd.idx();
        SmVector3d vecs[4];
        const SmVector3d* pv = read_vecs(rd, vecs);
        int64_t leaf1 = local ? rd.idx() : 0;
        int64_t leaf2 = local ? rd.idx() : 0;
        if (t1 != NULL && t2 != NULL)
        {
            std::map<const SmTreeNode*, int64_t> n1, n2;
            std::vector<SmTreeNode*> l1, l2;
            index_nodes(t1, n1, &l1);
            index_nodes(t2, n2, &l2);
            SmSolutionArray sols;
            if (!local)
            {
                Open<SmCCIGlobalSolver> s(t1, t2, ivl1, ivl2, proj, ptype);
                SmStatus ss = s.SolveIt(sop, SM_SR_ALL, tol, SM_BIG_DOUBLE, pv, sols);
                o.push_back(st(ss));
                if (ss == SM_SUCCESS)
                {
                    o.push_back((double)sols.GetSize());
                    for (ULONG i = 0; i < sols.GetSize(); i++)
                        put_sol(o, sols[i], c1, c2, n1, n2);
                }
            }
            else
            {
                o.push_back((double)l1.size());
                o.push_back((double)l2.size());
                if (!l1.empty() && !l2.empty())
                {
                    Open<SmCCIGlobalSolver> s(t1, t2, ivl1, ivl2, proj, ptype);
                    s.set(sop, tol, pv, &sols);
                    SmTreeNode* br[SM_GS_MAX_TREES] = {l1[(size_t)leaf1 % l1.size()],
                                                       l2[(size_t)leaf2 % l2.size()], NULL};
                    SmBoolean needs = FALSE;
                    SmStatus ls = s.LocalSolve(br, needs);
                    o.push_back(st(ls));
                    if (ls == SM_SUCCESS)
                    {
                        o.push_back(flg(needs));
                        o.push_back((double)sols.GetSize());
                        for (ULONG i = 0; i < sols.GetSize(); i++)
                            put_sol(o, sols[i], c1, c2, n1, n2);
                    }
                }
            }
        }
    }
    if (!self)
        delete c2;
    delete c1;
}

void run(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_CCI_SOLVE: run_cci(false, rd, ctx, o); break;
    case OP_CCI_LOCAL: run_cci(true, rd, ctx, o); break;
    case OP_PROPERTY:
    {
        SmCurve* c = read_curve(rd, ctx);
        read_transforms(rd, c);
        double a = rd.num();
        double b = rd.num();
        SmCurvePropertyType prop = (SmCurvePropertyType)rd.idx();
        bool has_val = rd.flag();
        double val = rd.num();
        SmVector3d vecs[4];
        const SmVector3d* pv = read_vecs(rd, vecs);
        double tol = rd.num();
        bool comb = rd.flag();
        {
            SmSolutionArray sols;
            SmTArray<SmPoint3d> pts, tines;
            SmStatus s = c->GlobalPropertyAnalysis(SmExtent1d(a, b), prop, has_val ? &val : NULL,
                                                   pv, tol, sols, comb ? &pts : NULL,
                                                   comb ? &tines : NULL);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
            {
                // the solutions name the curve and nodes of its object cache's tree
                SmCurveCache* cc =
                    (SmCurveCache*)SmCacheMgr::GetObjectCache(SM_OC_CURVE, c);
                std::map<const SmTreeNode*, int64_t> n1;
                index_nodes(cc ? cc->GetTree() : NULL, n1, NULL);
                o.push_back((double)sols.GetSize());
                for (ULONG i = 0; i < sols.GetSize(); i++)
                    put_sol(o, sols[i], c, NULL, n1, n1);
                o.push_back((double)pts.GetSize());
                for (ULONG i = 0; i < pts.GetSize(); i++)
                    put_vec(o, pts[i]);
                o.push_back((double)tines.GetSize());
                for (ULONG i = 0; i < tines.GetSize(); i++)
                    put_vec(o, tines[i]);
            }
        }
        delete c;
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_curve_curve_p7_eval(int32_t op, const double* in, int64_t n_in, double* out,
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
