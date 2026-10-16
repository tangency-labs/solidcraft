// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.curve_p3`: the SmCurve members owned by the unit
// (source/SMLib/src/SmCurve.cpp, source/SMLib/inc/SmCurve.h) called through `SmCurve*` on
// SmBSplineCurve, SmHermiteCurve and SmLine objects (ComputeCrvsNormal, CrvsFixGaps,
// FindParameterAtArcLength, LocalPointSolve, LocalCurveSolve, LocalCurveIntersect,
// IsLinear), plus the local classes of SmCurve.cpp owned by the unit
// (SmCurvePropertyGS::LocalSolve, SmFindPCExtremaEFO::Evaluate, SmBestLineFitENFO).
//
// Static functions and classes local to SmCurve.cpp cannot be linked against and the file
// cannot be recompiled inside a namespace (it defines SmCurve members), so the helpers used
// here are VERBATIM copies of upstream SmCurve.cpp (commit c9c5979), inside the namespace
// sc_crv_p3 (line ranges noted at each block; generated, do not edit by hand).
//
// One entry point, sc_curve_curve_p3_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/curve_curve_p3.rs, which validates every
// input. A curve block is `kind dim ...` (0: SmBSplineCurve `oob n p m cpts(x y z w)
// knots`; 1: SmHermiteCurve `P1 D1 P2 D2`; 2: bounded SmLine `P V d0 d1 scale io`). `out`
// receives the op status (0 = SM_SUCCESS, else the SmStatus value) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveCache.h>
#include <SmCurveTypes.h>
#include <SmExtent1d.h>
#include <SmGeomUtility.h>
#include <SmGlobalSolver.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmLocalSolve1d.h>
#include <SmLocalSolveNd.h>
#include <SmMatrix.h>
#include <SmNurbsCrv.h>
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

namespace sc_crv_p3
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

// ---- verbatim: SmCurve.cpp lines 11959-12878: SmCurvePropertyGS (class, constructor, FindBestSubdivisionNode, BranchMayContainAnswers), sm_FindAdditionalCpGuess, SmCurvePropertyGS::LocalSolve
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

/*******************************************************************//**
PURPOSE:

NOTES:
***********************************************************************/
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

/*******************************************************************//**
PURPOSE:

NOTES:
***********************************************************************/
SmStatus SmCurvePropertyGS::LocalSolve
  (SmTreeNode * apBranch[SM_GS_MAX_TREES],   // in :
   SmBoolean  & rbNeedsMoreSubdivision)      // out:
{
  // init output
  rbNeedsMoreSubdivision = FALSE;

  // locals
  SmTreeNode    * pCurveNode = apBranch[0];
  SmBezierSpan  * pBezSpan   = (SmBezierSpan*)pCurveNode->m_pData; NER(pBezSpan);
  SM_ASSERT(pCurveNode->m_eAuxDataType == SM_AD_BEZIER_CURVE);

  const SmCurve * pCurve ;
  SmExtent1d      sIvl ;

  ULONG jj;

  pCurve = (const SmBSplineCurve *)pCurveNode->GetOwnerObject() ;
  sIvl   = pBezSpan->GetInterval();
  // Note: no longer has to be SmBSplineCurve:
  //SM_ASSERT(pCurveNode->GetOwnerObject()->IsKindOf(SmBSplineCurve_TYPE)) ;

#ifdef SM_DEBUG_CODE
static constexpr SmBoolean  bDebugMe = FALSE;
  if (bDebugMe)
    {
      m_pSolutions->Dump() ;

      smgfx_Erase();
      smgfx_SetLook(1,2, 0,0,1) ; pCurve->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 1,0,0) ; pCurve->Draw(&sIvl); sm_GraphicsLoop() ;
      smgfx_SetLook(6,7, .5,1,0) ; m_pSolutions->Draw() ; sm_GraphicsLoop() ;
      sm_GraphicsLoop();
    }
#endif // SM_DEBUG_CODE

  if (   m_eCurveProperty == SM_CP_MINIMIZE_ANGLE_TO_PLANE
      || m_eCurveProperty == SM_CP_MINIMIZE_DIRECTED_ANGLE
      || m_eCurveProperty == SM_CP_MINIMIZE_CCW_ANGLE)
    {
      // Check the minimum angle made by the pseudo box to see if
      // we can eliminate having to process this node.
      SmPoint3d sCorners[8];
      pBezSpan->GetPseudoBox().CalcCorners(sCorners);
      double dMinCosTheta = SM_BIG_DOUBLE;
      for (jj=0; jj<8; jj++)
        {
          SmVector3d sVec = sCorners[jj] - m_cpOptVectors[0];
          sVec = m_cpOptVectors[1] * sVec * m_cpOptVectors[1]; // Project to view plane
          if (sVec.LengthSquared() < SM_EFF_ZERO)
            { return TRUE; }
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

          if (dCosTheta < dMinCosTheta) dMinCosTheta = dCosTheta;
        }

      // If we have no angles which are small enough than just return
      if ((dMinCosTheta*smos_Fabs(dMinCosTheta))+SM_EFF_ZERO_SQRT > m_dBestAnswerSoFarSq)
          { return SM_SUCCESS; }

    } // end cases SM_CP_MINIMIZE_ANGLE_TO_PLANE
      //           SM_CP_MINIMIZE_DIRECTED_ANGLE
      //           SM_CP_MINIMIZE_CCW_ANGLE

  // Do interval tests and reject if disjoint or adjust
  // if sIvl is not completely inside of m_crInterval
  SmContinuityType aeConts[2];
  aeConts[0] = pBezSpan->m_aeConts[0];
  aeConts[1] = pBezSpan->m_aeConts[1];
  if (sIvl.AreDisjoint(m_crInterval))
    { return SM_SUCCESS; }

  if (sIvl.GetMin() <= m_crInterval.GetMin())
    {  aeConts[0] = SM_CT_DISCONTINUOUS; }

  if (sIvl.GetMax() >= m_crInterval.GetMax())
    {  aeConts[1] = SM_CT_DISCONTINUOUS; }

  if (!sIvl.IsContainedBy(m_crInterval, SM_EFF_ZERO))
    {
      sIvl.Intersect(m_crInterval,sIvl);
      // Check for zero length intervals
      if (SM_ARE_SAME(sIvl.GetMax(),sIvl.GetMin()))
        { return SM_SUCCESS; }
    }

  // locals
  SmBoolean  bFoundSolution;
  SmSolution sSolution;

  double dGuessT[100];
  ULONG  lNumGuess;

  // Process discontinuity points when appropriate
  if (    m_eCurveProperty == SM_CP_MINIMIZE_ANGLE_TO_PLANE
      ||  m_eCurveProperty == SM_CP_MINIMIZE_DIRECTED_ANGLE
      ||  m_eCurveProperty == SM_CP_MINIMIZE_CCW_ANGLE
      ||  m_eCurveProperty == SM_CP_PLANE_INTERSECTION)
   {
      for (jj=0; jj<2; jj++)
        {
          double dParam = (jj==0) ? sIvl.GetMin()
                                  : sIvl.GetMax();
          if (aeConts[jj] < SM_CT_G1)
            {
              SmPoint3d sPnt;
              SER(pCurve->EvaluatePoint(dParam,sPnt));

              // for minimize angle cases
              if (   m_eCurveProperty == SM_CP_MINIMIZE_ANGLE_TO_PLANE
                  || m_eCurveProperty == SM_CP_MINIMIZE_DIRECTED_ANGLE
                  || m_eCurveProperty == SM_CP_MINIMIZE_CCW_ANGLE)
                {
                  SmVector3d sVec = sPnt - m_cpOptVectors[0];
                  sVec = m_cpOptVectors[1] * sVec * m_cpOptVectors[1];

                  // If the point is at the origin of rotation just ignore it
                  if (sVec.LengthSquared() > SM_EFF_ZERO_SQ)
                    {
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

                      // save solution
                      sSolution.m_eSolutionType = SM_ST_SINGLE_VALUE;
                      sSolution.m_apNodes[0] = apBranch[0];  // Point node
                      sSolution.m_apNodes[1] = apBranch[1];  // Curve node
                      sSolution.m_lNumObjects   = 1 ;
                      sSolution.m_apObjects[0]  = (SmObject *)pCurve ;
                      sSolution.m_lNumVariables = 1;
                      sSolution.m_vStart[0] = dParam;
                      sSolution.m_vStart.m_dSolutionValue = dCosTheta;
                      AddSortedSolution(sSolution,SM_SK_BY_VALUE_WITH_PARAMETER, TRUE);
                    }
                }  // end CurveProperty cases SM_CP_MINIMIZE_ANGLE_TO_PLANE
                   //                         SM_CP_MINIMIZE_DIRECTED_ANGLE
                   //                         SM_CP_MINIMIZE_CCW_ANGLE)

              if (m_eCurveProperty == SM_CP_PLANE_INTERSECTION)
                {
                  double dVal = sPnt.Dot(m_cpOptVectors[0]) + *m_cpdPropertyValue;

                  // points within tolerance
                  if (smos_Fabs(dVal) < m_d3dTolerance)
                    {
                      sSolution.m_eSolutionType           = SM_ST_SINGLE_VALUE;
                      sSolution.m_apNodes[0]              = apBranch[0];  // Point node
                      sSolution.m_apNodes[1]              = apBranch[1];  // Curve node
                      sSolution.m_lNumObjects             = 1 ;
                      sSolution.m_apObjects[0]            = (SmObject *)pCurve ;
                      sSolution.m_lNumVariables           = 1;
                      sSolution.m_vStart[0]               = dParam;
                      sSolution.m_vStart.m_dSolutionValue = smos_Fabs(dVal);

                      //
                      if (jj == 0)
                        {
                          dParam = sIvl.GetMax();
                          SER(pCurve->EvaluatePoint(dParam,sPnt));
                          double dVal2 = sPnt.Dot(m_cpOptVectors[0]) + *m_cpdPropertyValue;
                          SER(pCurve->EvaluatePoint(sIvl.Evaluate(0.4567),sPnt));
                          double dVal3 = sPnt.Dot(m_cpOptVectors[0]) + *m_cpdPropertyValue;
                          if (smos_Fabs(dVal2) < m_d3dTolerance &&
                              smos_Fabs(dVal3) < m_d3dTolerance)
                            {
                              sSolution.m_eSolutionType         = SM_ST_RANGE_OF_VALUES;
                              sSolution.m_vEnd[0]               = dParam;
                              sSolution.m_vEnd.m_dSolutionValue = smos_Fabs(dVal);
                              AddSortedSolution(sSolution,SM_SK_BY_FIRST_PARAMETER, TRUE);
                              return SM_SUCCESS;
                            }
                        } // end max interval value value check

                      // inform the public
                      AddSortedSolution(sSolution,SM_SK_BY_FIRST_PARAMETER, TRUE);

                    } // end points within tolerance check
                } // end m_eCurveProperty == SM_CP_PLANE_INTERSECTION branch
            } // end aeConts[jj] < SM_CT_G1 check
        } // end iter over interval min and max
    } // end CurveProperty cases: SM_CP_MINIMIZE_ANGLE_TO_PLANE
      //                          SM_CP_MINIMIZE_DIRECTED_ANGLE
      //                          SM_CP_MINIMIZE_CCW_ANGLE
      //                          SM_CP_PLANE_INTERSECTION

  // Note: leaving this next bit out.  Besides being a bit awkward, it's
  // obsolete now because we can solve the oscillating case, and indeed
  // sometimes we need to, e.g.  [B545]
  //// Here is something a bit awkward.
  //// In SmCurvePropertyEFO::Evaluate() (which LocalPropertyAnalysis() calls),
  //// SmCurve::Evaluate() is called with the left/right flag True,
  //// which means that, if the 'top' of this interval is at a knot,
  //// it will be evaluated in the 'next' span.  On a simple piecewise quadratic
  //// Bezier curve (B-Spline with double interior knots), there can be a
  //// discontinuity in the first derivative.  This can cause the solver to
  //// thrash (because there's one point with a jump in the function value),
  //// and indeed to return the wrong answer -- that from the next span instead
  //// of from this span.  There is no good way to tell the property evaluator
  //// which side to use (nor any way for it to decide).  So, we will have the
  //// property evaluator always use True, and we'll back off the top of the
  //// interval just enough so that evaluation stays in this span.
  //// We need to do this only if there's a C1 discontinuity there.
  //// (Note, if False was used, then we would tweak the bottom of the interval.)
  //// [081015; solver oscillation tests]
  //if ( aeConts[1] < SM_CT_G1 )
  //  {
  //    // Back it off just enough to make sure the value changes.
  //    double dNewTop = sIvl.GetMax() * ( 1.0 - 0.001 * SM_EFF_ZERO );
  //    sIvl.SetMinMax( sIvl.GetMin(), dNewTop );
  //  }

  if (   m_eCurveProperty == SM_CP_MINIMIZE_ANGLE_TO_PLANE
     ||  m_eCurveProperty == SM_CP_MINIMIZE_DIRECTED_ANGLE
     ||  m_eCurveProperty == SM_CP_MINIMIZE_CCW_ANGLE
     ||  m_eCurveProperty == SM_CP_PARALLEL_TO_VECTOR)
    {
      // There are possible optimizations here that we could use to
      // check the angle of the curve relative to the point.
      dGuessT[0] = sIvl.Evaluate(0.5);
      lNumGuess = 1;
    }
  else
    { // Just take 5 guesses
      dGuessT[0] = sIvl.Evaluate(0.5);
      dGuessT[1] = sIvl.Evaluate(0.0);
      dGuessT[2] = sIvl.Evaluate(1.0);
      dGuessT[3] = sIvl.Evaluate(0.25);
      dGuessT[4] = sIvl.Evaluate(0.75);
      lNumGuess = 5;
    }

  ULONG lNumOriginalGuess = lNumGuess;

  // If we have three zeroes in this little span, assume it's a zero function
  // and create a Range solution.  [B500. before other fix]
  // Note, if this should falsely make a Range solution (i.e., the function
  // happens to be zero in three separate places but is not zero everywhere
  // in this cache span), then the caching mechanism needs work, not this
  // 3-zero assumption.  (Cache spans should be small enough...)
  ULONG lNumSerendipities = 0;

  // each guess
  ULONG ii;
  for (ii=0; ii<lNumGuess; ii++)
    {
      if (pCurve->LocalPropertyAnalysis(sIvl,
                                        m_eCurveProperty,
                                        dGuessT[ii],
                                        m_cpdPropertyValue,
                                        m_cpOptVectors,
                                        bFoundSolution,
                                        sSolution) != SM_SUCCESS)
        {
          // Had an error just try a different starting point - perhaps some higher derivative was zero.
          continue;
        }

      if(   bFoundSolution
         && m_eCurveProperty == SM_CP_PARALLEL_TO_VECTOR)
        {
          SmVector3d sPV[2];
          SER(pCurve->Evaluate(sSolution.m_vStart[0],1,TRUE,sPV, TRUE)); // nonZeroTangent values
          double dAngle;
          SER(m_cpOptVectors[0].AngleBetween(sPV[1],dAngle));
          if (dAngle > SM_PI/2.0)      dAngle         = SM_PI - dAngle;
          if (dAngle > m_d3dTolerance) bFoundSolution = FALSE;
        }

      // If we did not find a solution the first time then stop
      if (ii==0 && !bFoundSolution) { return SM_SUCCESS; }
      if (!bFoundSolution) continue;  // If fail don't save answer

      // Have a solution.
      // Was the guess point a solution?
      if ( SM_ARE_SAME_TO_TOL( dGuessT[ii], sSolution.m_vStart[0], SM_EFF_ZERO ) )
        {
          lNumSerendipities++;
        }

      if ( m_eCurveProperty == SM_CP_PLANE_INTERSECTION )
        {
          // This returns signed solution values.  AddSortedSolution() requires absolute values.
          sSolution.m_vStart.m_dSolutionValue = smos_Fabs( sSolution.m_vStart.m_dSolutionValue );
          if ( sSolution.m_eSolutionType == SM_ST_RANGE_OF_VALUES )
            { sSolution.m_vEnd.m_dSolutionValue = smos_Fabs( sSolution.m_vEnd.m_dSolutionValue ); }

          // Also, look for a nearby guess: if nearly tangent,
          // make a circle at the solution and intersect with plane.
          if ( ii < lNumOriginalGuess )
            {
              SmBoolean bFoundNewGuess;
              double dNewGuess;
              SER(sm_FindAdditionalCpGuess(*pCurve,
                                            sIvl,
                                           *m_cpOptVectors,
                                            sSolution.m_vStart[0],
                                            bFoundNewGuess,
                                            dNewGuess));
              if (bFoundNewGuess)
                {
                  dGuessT[lNumGuess++] = dNewGuess;
                }
            }
        }

#ifdef SM_DEBUG_CODE
SmBoolean bDebugOutput = FALSE;
      if (bDebugOutput)
        {
          sm_GraphicsLoop();
          smgfx_SetLook(1,2, 0,0,0); pCurve->Draw(&sIvl); sm_GraphicsLoop();
          SmPoint3d sFndPnt;
          pCurve->EvaluatePoint(sSolution.m_vStart[0],sFndPnt);
          smgfx_SetLook(4,5, 0,1,1); sFndPnt.Draw(); sm_GraphicsLoop();
          sm_GraphicsLoop();
        }
#endif // SM_DEBUG_CODE

      sSolution.m_apNodes[0]    = apBranch[0];  // Curve node
      sSolution.m_lNumVariables = 1;
      sSolution.m_lNumObjects   = 1 ;
      sSolution.m_apObjects[0]  = (SmObject *)pCurve ;

      // Check for a Range solution.
      if ( lNumSerendipities >= 3 )
        {
          // Make a Range solution and we're done.
          sSolution.m_eSolutionType         = SM_ST_RANGE_OF_VALUES;
          sSolution.m_vStart[0]             = sIvl.GetMin();
          sSolution.m_vEnd[0]               = sIvl.GetMax();
          sSolution.m_vEnd.m_dSolutionValue = sSolution.m_vStart.m_dSolutionValue;
          AddSortedSolution( sSolution,SM_SK_BY_FIRST_PARAMETER, TRUE );
          return SM_SUCCESS;
        }

      if(   m_eCurveProperty != SM_CP_MINIMIZE_FIRST_DERIVATIVE
         && m_eCurveProperty != SM_CP_MAXIMIZE_FIRST_DERIVATIVE
         && m_eCurveProperty != SM_CP_MINIMIZE_RADIUS_OF_CURVATURE
         && m_eCurveProperty != SM_CP_MAXIMIZE_RADIUS_OF_CURVATURE
         && m_eCurveProperty != SM_CP_MINIMIZE_CCW_ANGLE
         && m_eCurveProperty != SM_CP_MINIMIZE_ANGLE_TO_PLANE
         && m_eCurveProperty != SM_CP_MINIMIZE_DIRECTED_ANGLE
         && m_eCurveProperty != SM_CP_PROJECTED_POINT_MAXIMIZE
         && m_eCurveProperty != SM_CP_PROJECTED_POINT_MINIMIZE)
        {
          AddSortedSolution(sSolution,SM_SK_BY_FIRST_PARAMETER, TRUE);
        }
      else
        {
          AddSortedSolution(sSolution,SM_SK_BY_VALUE_WITH_PARAMETER, TRUE);
        }
#ifdef SM_DEBUG_CODE
      if (bDebugMe)
        {
          sSolution.Dump() ;
          m_pSolutions->Dump() ;

          smgfx_Erase();
          smgfx_SetLook(1,2, 0,0,1) ; pCurve->Draw() ; sm_GraphicsLoop() ;
          smgfx_SetLook(3,4, 1,0,0) ; pCurve->Draw(&sIvl); sm_GraphicsLoop() ;
          smgfx_SetLook(6,7, 1,.5,0) ; sSolution.Draw() ; sm_GraphicsLoop() ;
          smgfx_SetLook(9,10, .5,1,0) ; m_pSolutions->Draw() ; sm_GraphicsLoop() ;
          sm_GraphicsLoop();
        }
#endif // SM_DEBUG_CODE
    } // end iter every guess point

  // all done
  return SM_SUCCESS;

} // end SmCurvePropertyGS::LocalSolve
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

// ---- verbatim: SmCurve.cpp lines 18198-18413: SmFindPCExtremaEFO (class, Evaluate)
class SmFindPCExtremaEFO : public SmEvalFunctionObject
{
protected:
  const SmCurve       & m_crCurve;            // target curve
  const SmPoint3d     & m_crTestPoint;        // 3d Point
  const SmExtent1d      m_sDomain;            // domain of interest on the curve
  double                m_dSquaredTolerance;  // iteration stops when GapSize < thisTol
                                              // or gap/CurveTangent angle from perp squared < thisTol
  SmSolverOperationType m_eSolverOperation;   // oneof: SM_SO_MINIMIZE
                                              //        SM_SO_MAXIMIZE
  SmBoolean             m_bHaveScale;         // Internal flag to mark when Scale has been picked
  ULONG                 m_lNumScale;          // Internal Scale Value to avoid math errors on large tangent values
// Error = 'gap/CurveTangent angle from perp * gapSize * tangentSize'

public:
  // constructor
  SmFindPCExtremaEFO
    (const SmCurve       & crCurve,
     const SmPoint3d     & crTestPoint,
     const SmExtent1d    & crDomain,
     double                dSquaredTolerance,
     SmSolverOperationType eSolverOperation)  : m_crCurve          (crCurve),
                                                m_crTestPoint      (crTestPoint),
                                                m_sDomain          (crDomain),
                                                m_dSquaredTolerance(dSquaredTolerance),
                                                m_eSolverOperation (eSolverOperation),
                                                m_bHaveScale       (FALSE),
                                                m_lNumScale        (0)
                                              { }
  //destructor
  virtual ~SmFindPCExtremaEFO() { }

  virtual SmStatus Evaluate(double      dT,                   // in : target T value to query
                            double    & rdFOfT,               // out: F(T)     = Function value for given dT value
                            double    & rdFPrimeOfT,          // out: dF(T)/dT = Function derivative value for given dT value
                            SmBoolean & rbFoundAnswer,        // out: TRUE = Function value is within tolerance of zero, FALSE=Not
                            SmBoolean   bSignalErrors=TRUE) ; // NotUsed: in : TRUE = signal errors, FALSE=errors anticipated, don't signal, default:[TRUE]

} ; // end class SmFindPCExtremaEFO

/****************************************************************
PURPOSE: Called once for each NR iteration to find
            curve/curve intersections.
    Given X values,   Evaluate Func         = Func(X) and
                      Jacobian Matrix[i][j] = dFi/dxj.

    where Jacobi = (partial Func[i]/partial X[j])

NOTES:
   Let t   = Curve Param Value
      C(t) = curve
       P   = TestPoint

  Evaluate Func      = (C(t) - P) * C'(t)
           1st Deriv = SM_SO_MINIMIZE: +1 * (C'(t) * C'(t) + C(t) * C"(t))
                       SM_SO_MAXIMIZE: -1 * (C'(t) * C'(t) + C(t) * C"(t))
  error = zero when gapSize is zero or gap is perp to curve
  FoundAnswer TRUE = GapSizeSq < m_dSquaredTolerance
                     or Gap/Tangent angle from perp squared < m_dSquaredTolerance
****************************************************************/
SmStatus SmFindPCExtremaEFO::Evaluate
 (double      dT,                // in : target T value to query
  double    & rdFOfT,            // out: F(T)     = Function value for given dT value
  double    & rdFPrimeOfT,       // out: dF(T)/dT = Function derivative value for given dT value
  SmBoolean & rbFoundAnswer,     // out: TRUE = GapSizeSq < m_dSquaredTolerance
                                 //             or Gap/Tangent angle from perp squared < m_dSquaredTolerance
                                 //      FALSE= Not
  SmBoolean   bSignalErrors)     // NotUsed: in : TRUE = signal errors, FALSE=errors anticipated, don't signal, default:[TRUE]   
{
  SM_REF1(bSignalErrors) ;
  // init output
  rdFOfT = 0.0;
  rdFPrimeOfT = 0.0;
  rbFoundAnswer = FALSE;

  // curve point
  SmPoint3d sPV[3];
  SmBoolean bSide = ( dT < m_sDomain.GetMid() );
  SER(m_crCurve.Evaluate(dT,2,bSide,sPV,TRUE)); // nonZeroTangent values

  // gap: sV = CurvePoint - TestPoint
  SmVector3d sV = sPV[0] - m_crTestPoint;

#ifdef SM_DEBUG_CODE
SmBoolean  bDebugMe = FALSE;
  if (bDebugMe)
    {
      m_crCurve.Dump() ;

      SmEdge *pEdge1 = (SmEdge *)m_crCurve.GetEdge() ;
      SmBrep *pBrep1 = pEdge1 ? pEdge1->GetBrep() : NULL ;

      smgfx_Erase() ;
      smgfx_SetLook(1,2,  0,0,1) ; if(pBrep1) pBrep1->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3,  0,1,1) ; m_crCurve.Draw() ; sm_GraphicsLoop();
      smgfx_SetLook(3,4,  0,0,0) ; if(pEdge1) pEdge1->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,6,  0,0,1) ; m_crCurve.DrawAt(dT,1); sm_GraphicsLoop();
      smgfx_SetLook(6,8,  0,1,0) ; m_crTestPoint.Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3,  1,0,0) ; sV.Draw(&m_crTestPoint); sm_GraphicsLoop();
      sm_GraphicsLoop();
    }
#endif // SM_DEBUG_CODE

  // Scale the problem down to a reasonable size (0-10000)
  if (!m_bHaveScale)
    {
      while (sPV[1].LengthSquared() > 1.0e4)
        {
          m_lNumScale ++;
          sV    = sV    / 10.0;
          sPV[0] = sPV[0] / 10.0;
          sPV[1] = sPV[1] / 10.0;
          sPV[2] = sPV[2] / 10.0;
        }
      m_bHaveScale = TRUE;
    }
  else
    {
      double dScale = 1.0 ;
      ULONG kk;
      for (kk=0; kk<m_lNumScale; kk++) { dScale *= 10.0 ; }
      if(dScale > 1.0) { sV    = sV    / dScale;
                         sPV[0] = sPV[0] / dScale;
                         sPV[1] = sPV[1] / dScale;
                         sPV[2] = sPV[2] / dScale;
                       }
    }

  // Check for a zero length tangent.
  // If it's because of a degenerate curve, then we're already at a solution.
  // If not degenerate, we're at a singularity and can't do much.

  // note:  mostly this branch will only be used for degenerate curves with the nonZeroTangent evaluator
  double dTanDotTan = sPV[1].Dot(sPV[1]) ;
  if (dTanDotTan < SM_EFF_ZERO_SQ)
    {
      // degenerate curve?  Just return that we are perp, where we are.
      if ( m_crCurve.IsDegenerate() )
        {
          rdFOfT = 0.0;
          rbFoundAnswer = TRUE;
          return SM_SUCCESS;
        }
      else
        {
          return SM_ERR;
        }
    } // end zero tangent check

  double dVDotV     = sV.Dot(sV) ;
  double dVDotTan   = sV.Dot(sPV[1]) ;
  double dVDot2nd   = sV.Dot(sPV[2]) ;
//double dTanDotTan = sPV[1].Dot(sPV[1]) ;  // already defined

  // Evaluate Func = (C(t) - P) * C'(t)
  rdFOfT = dVDotTan ;

  // 1st Deriv = C'(t) * C'(t) + (C(t) - P) * C"(t)

  // Note: for the derivative, if we're not close enough to a solution,
  // the second-order term can dominate the first-order term, and can change
  // the direction.  The first-order term always gives the correct sign
  // for the step, so the second-order term should not cause a sign change.
  // This will also prevent zero-divides.
  // Note, the first order term is always positive.  [B52]
  // [cf. Fillet 1:127: succeeds at dLim2dDeriv = 0.5 but that's not due to ]
  // [    the result of this function, it should be addressed in Filleting. ]
  //   rdFPrimeOfT = sPV[1].Dot(sPV[1]) + sPV[2].Dot(sV) ;

  double dLim2dDeriv = 0.95;  // Smaller values can slow down convergence.

//  if (   m_eSolverOperation != SM_SO_MAXIMIZE
//      && m_eSolverOperation != SM_SO_NORMALIZE )
//    {
      if ( dVDot2nd < -dTanDotTan * dLim2dDeriv )
        {
          dVDot2nd = -dTanDotTan * dLim2dDeriv;
        }
//    }

  rdFPrimeOfT = dTanDotTan + dVDot2nd ;

  // Set sign of 1st Derive for maximize
  //cbi: check this.
  if ( m_eSolverOperation == SM_SO_MAXIMIZE )
    {
      // Let - sign of FofT rule
      rdFPrimeOfT = - (rdFPrimeOfT);
    }

  // Done when GapSize goes to zero
  if (dVDotV < m_dSquaredTolerance)
    {
      // for SM_SO_MINIMIZE = SUCCESS
      //     SM_SO_MAXIMIZE = FAILURE
      if (m_eSolverOperation == SM_SO_MAXIMIZE)
        { return SM_ERR; }
      rbFoundAnswer = TRUE;
      return SM_SUCCESS;
    }

  // Find gap/CurveTangent angle from perpendicular squared
  // Test this to see if we satisfy our tolerance
  double dGapTanAngleFromPerpSquared =   (dVDotTan * dVDotTan)
                                       / (dVDotV   * dTanDotTan) ;
  if (dGapTanAngleFromPerpSquared < m_dSquaredTolerance)
    {
      rbFoundAnswer = TRUE;
      return SM_SUCCESS;
    }

  // all done
  return SM_SUCCESS;

} // end SmFindPCExtremaEFO::Evaluate
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 18667-18996: SmBestLineFitENFO (class, constructor, Evaluate)
class SmBestLineFitENFO : public SmEvalNFunctionsObject
{
protected:
  const SmCurve             & m_crCurve1;        // 1st curve
  const SmCurve             & m_crCurve2;        // 2nd curve
  const SmExtentNd          * m_cpIntervals;     // Intervals for 1st and 2nd curves
  const SmTArray<SmBoolean> * m_cpPeriodicities; // periodic flag for each parametric dimension
                                                 // TRUE = periodic
  double m_dSquaredTolerance;                    // Currently not used

 // SumOfAllErrors = 0 when difference vector zero, or is
 //                 perpendicular to both tangent vectors
public:
  SmBestLineFitENFO(const SmCurve    & crCurve1,            // in : 1st Curve
                    const SmCurve    & crCurve2,            // in : 2nd Curve
                    double             dSquaredTolerance);  // in : currently not used
 virtual ~SmBestLineFitENFO() { }

  virtual SmBoolean HasIntervals()                  { return TRUE ; }
  virtual SmStatus  SetIntervals
     (const SmExtentNd          *cpIntervals,
      const SmTArray<SmBoolean> *cpPeriodicities )  { m_cpIntervals     = cpIntervals;
                                                      m_cpPeriodicities = cpPeriodicities ;
                                                      return SM_SUCCESS ;
                                                    }
  virtual SmStatus Evaluate
    (const SmTArray<double> & crX,             // in : x of Ax=F
     SmTArray<double>       & rF,              // out: F of Ax=F function values,
     SmMatrix               * pOptJacobian,    // out: Partial derivatives of the functions.
     SmBoolean              & rbFoundAnswer);  // out: Always Set to FALSE

} ; // end class SmBestLineFitENFO

/*******************************************************************//**
PURPOSE: NewtonRaphson Iteration Evaluate function Constructor
            for Curve/Curve Intersections

USER NOTES ---
***********************************************************************/
SmBestLineFitENFO::SmBestLineFitENFO
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
} // end SmBestLineFitENFO::SmBestLineFitENFO constructor

/***************************************************************
PURPOSE: Called once for each NR iteration to find
            curve/curve intersections.
    Given X values,   Evaluate Func         = Func(X) and
                      Jacobian Matrix[i][j] = dFi/dxj.

    where Jacobi = (partial Func[i]/partial X[j])

NOTES:
  Let Gap = Curve1Point - Curve2Point
  crX[0]  = s parameter for m_crCurve1
  crX[1]  = s parameter for m_crCurve2
  Func[0] = Gap Dot C1'
  Func[1] = Gap Dot C2'

  Or should we use the unitized tangents?  Answer: Yes, definitely: [071120].
  Two curves, V-shaped, both curves heading upwards, both guesses above the
  intersection, but steps moved upwards, away from the intersection.  The
  problem is that the 2nd derivs have a large component downwards, opposite to
  the 1st derivs.  This says that the 1st derivs are getting smaller as you go
  up.  If their magnitudes are part of the objective functions, the functions
  will decrease going upwards, even though the gap increases.  Using the
  unitized derivatives converges very well.
  Note, this means that we have to take the derivative of a unitized vector,
  which is a bit hairy, but not too bad, and it's already encapsulated in
  SmVector3d::UnitizedDerivative().

***************************************************************/
SmStatus SmBestLineFitENFO::Evaluate
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
  // large near maxima conditions to 'fall off' the hill.
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

  static constexpr SmBoolean bSwitchToTanFcns = TRUE;

  double dAngLimDeg = 0.01;   // Note: .01 works better than 2.0.
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

} // end SmBestLineFitENFO::Evaluate
// ---- end verbatim
// clang-format on

// Exposes the protected solver state the tests set.
template <class S> struct Open : S
{
    using S::S;
    void set(SmSolverOperationType op, double tol, double best, SmSolutionArray* sols)
    {
        this->m_eSolverOperation = op;
        this->m_d3dTolerance = tol;
        this->m_dBestAnswerSoFarSq = best;
        this->m_pSolutions = sols;
    }
};
} // namespace sc_crv_p3

namespace
{
using namespace sc_crv_p3;

// Ops (keep in sync with crates/solidcore-ffi/src/units/curve_curve_p3.rs).
enum : int32_t
{
    OP_NORMAL = 0,
    OP_FIX_GAPS = 1,
    OP_ARC_LENGTH = 2,
    OP_PC_EXTREMA = 3,
    OP_LINE_FIT = 4,
    OP_POINT_SOLVE = 5,
    OP_CURVE_SOLVE = 6,
    OP_INTERSECT = 7,
    OP_PROPERTY_LOCAL = 8,
    OP_IS_LINEAR = 9,
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

// `nv V[nv]` into `vecs`; returns NULL for nv == 0.
const SmVector3d* read_vecs(Rd& rd, SmVector3d vecs[4])
{
    int64_t nv = rd.idx();
    for (int64_t i = 0; i < nv; i++)
        vecs[i] = rd.vec();
    return nv > 0 ? vecs : NULL;
}

// A solution `type nvar nobj o0 o1 value s0 s1 [evalue e0 e1]`; objects `0` = a, `1` = b,
// `2` = other, `-1` = NULL.
void put_sol(std::vector<double>& o, const SmSolution& s, const void* a, const void* b)
{
    auto id = [&](const void* p) { return p == NULL ? -1.0 : p == a ? 0.0 : p == b ? 1.0 : 2.0; };
    o.push_back((double)s.m_eSolutionType);
    o.push_back((double)s.m_lNumVariables);
    o.push_back((double)s.m_lNumObjects);
    o.push_back(s.m_lNumObjects > 0 ? id(s.m_apObjects[0]) : -1.0);
    o.push_back(s.m_lNumObjects > 1 ? id(s.m_apObjects[1]) : -1.0);
    o.push_back(s.m_vStart.m_dSolutionValue);
    o.push_back(s.m_vStart[0]);
    o.push_back(s.m_vStart[1]);
    if (s.m_eSolutionType == SM_ST_RANGE_OF_VALUES)
    {
        o.push_back(s.m_vEnd.m_dSolutionValue);
        o.push_back(s.m_vEnd[0]);
        o.push_back(s.m_vEnd[1]);
    }
}

void run(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_NORMAL:
    {
        int64_t n = rd.idx();
        SmTArray<SmCurve*> curves;
        for (int64_t k = 0; k < n; k++)
            curves.Add(read_curve(rd, ctx));
        SmVector3d nrm(0, 0, 0);
        SmStatus s = SmCurve::ComputeCrvsNormal(curves, nrm);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, nrm);
        for (ULONG i = 0; i < curves.GetSize(); i++)
            delete curves[i];
        break;
    }
    case OP_FIX_GAPS:
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
        ULONG fixed = SmCurve::CrvsFixGaps(curves, tol);
        o.push_back(0.0);
        o.push_back((double)fixed);
        o.push_back((double)curves.GetSize());
        for (ULONG i = 0; i < curves.GetSize(); i++)
        {
            SmBSplineCurve* c = (SmBSplineCurve*)curves[i];
            o.push_back((double)index[c]);
            gw_CURVE* nurb = c->GetGwNurbPointer();
            o.push_back((double)(nurb->pol->n + 1));
            for (gw_INDEX k = 0; k <= nurb->pol->n; k++)
            {
                o.push_back(nurb->pol->Pw[k].x);
                o.push_back(nurb->pol->Pw[k].y);
                o.push_back(nurb->pol->Pw[k].z);
                o.push_back(nurb->pol->Pw[k].w);
            }
        }
        for (ULONG i = 0; i < curves.GetSize(); i++)
            delete curves[i];
        break;
    }
    case OP_ARC_LENGTH:
    {
        SmCurve* c = read_curve(rd, ctx);
        double start = rd.num();
        double len = rd.num();
        double end = 0.0;
        SmBoolean past = FALSE;
        SmStatus s = c->FindParameterAtArcLength(start, len, end, &past);
        o.push_back(st(s));
        if (s == SM_SUCCESS || s == SM_ERR_NOT_CONVERGING)
        {
            o.push_back(end);
            o.push_back(flg(past));
        }
        delete c;
        break;
    }
    case OP_PC_EXTREMA:
    {
        SmCurve* c = read_curve(rd, ctx);
        SmPoint3d p = rd.vec();
        double a = rd.num();
        double b = rd.num();
        double sqtol = rd.num();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        int64_t nt = rd.idx();
        SmExtent1d dom(a, b);
        SmFindPCExtremaEFO efo(*c, p, dom, sqtol, sop);
        o.push_back(0.0);
        for (int64_t i = 0; i < nt; i++)
        {
            double t = rd.num();
            double f = 0.0, fp = 0.0;
            SmBoolean found = FALSE;
            SmStatus s = efo.Evaluate(t, f, fp, found, TRUE);
            o.push_back(st(s));
            o.push_back(f);
            o.push_back(fp);
            o.push_back(flg(found));
        }
        delete c;
        break;
    }
    case OP_LINE_FIT:
    {
        SmCurve* c1 = read_curve(rd, ctx);
        SmCurve* c2 = read_curve(rd, ctx);
        SmTArray<double> x, f;
        x.Add(rd.num());
        x.Add(rd.num());
        f.Add(0.0);
        f.Add(0.0);
        bool jac = rd.flag();
        SmMatrix m(2, 2);
        for (ULONG i = 0; i < 2; i++)
            for (ULONG j = 0; j < 2; j++)
                m[i][j] = 0.0;
        SmBestLineFitENFO enfo(*c1, *c2, SM_EFF_ZERO_SQ);
        SmBoolean found = FALSE;
        SmStatus s = enfo.Evaluate(x, f, jac ? &m : NULL, found);
        o.push_back(st(s));
        o.push_back(f[0]);
        o.push_back(f[1]);
        for (ULONG i = 0; i < 2; i++)
            for (ULONG j = 0; j < 2; j++)
                o.push_back(m[i][j]);
        o.push_back(flg(found));
        o.push_back(enfo.GetSolutionSize());
        o.push_back(enfo.GetScaledZero());
        delete c1;
        delete c2;
        break;
    }
    case OP_POINT_SOLVE:
    {
        SmCurve* c = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        SmPoint3d p = rd.vec();
        bool has_dt = rd.flag();
        double dt = rd.num();
        bool has_target = rd.flag();
        double target = rd.num();
        SmVector3d vecs[4];
        const SmVector3d* pv = read_vecs(rd, vecs);
        double guess = rd.num();
        SmBoolean found = FALSE;
        SmSolution sol;
        SmStatus s = c->LocalPointSolve(SmExtent1d(a, b), sop, p, has_dt ? &dt : NULL,
                                        has_target ? &target : NULL, pv, guess, found, sol);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(found));
            put_sol(o, sol, c, NULL);
        }
        delete c;
        break;
    }
    case OP_CURVE_SOLVE:
    {
        SmCurve* c1 = read_curve(rd, ctx);
        double a1 = rd.num();
        double b1 = rd.num();
        SmCurve* c2 = read_curve(rd, ctx);
        double a2 = rd.num();
        double b2 = rd.num();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        double tol = rd.num();
        bool has_target = rd.flag();
        double target = rd.num();
        SmVector3d vecs[4];
        const SmVector3d* pv = read_vecs(rd, vecs);
        double g1 = rd.num();
        double g2 = rd.num();
        SmBoolean found = FALSE;
        SmSolution sol;
        SmStatus s = c1->LocalCurveSolve(SmExtent1d(a1, b1), *c2, SmExtent1d(a2, b2), sop, tol,
                                         has_target ? &target : NULL, pv, g1, g2, found, sol);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(found));
            put_sol(o, sol, c1, c2);
        }
        delete c1;
        delete c2;
        break;
    }
    case OP_INTERSECT:
    {
        SmCurve* c1 = read_curve(rd, ctx);
        double a1 = rd.num();
        double b1 = rd.num();
        SmCurve* c2 = read_curve(rd, ctx);
        double a2 = rd.num();
        double b2 = rd.num();
        double tol = rd.num();
        double g1 = rd.num();
        double g2 = rd.num();
        SmBoolean found = FALSE;
        double my = 0.0, other = 0.0, dev = 0.0;
        SmStatus s = c1->LocalCurveIntersect(SmExtent1d(a1, b1), *c2, SmExtent1d(a2, b2), tol, g1,
                                             g2, found, my, other, dev);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(found));
            if (found)
            {
                o.push_back(my);
                o.push_back(other);
                o.push_back(dev);
            }
        }
        delete c1;
        delete c2;
        break;
    }
    case OP_PROPERTY_LOCAL:
    {
        SmCurve* c = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        SmBoolean full = rd.flag() ? TRUE : FALSE;
        double chord = rd.num();
        double angle = rd.num();
        double offaxis = rd.num();
        ULONG minseg = (ULONG)rd.idx();
        bool aux = rd.flag();
        double max3d = rd.num();
        double ratio = rd.num();
        double i0 = rd.num();
        double i1 = rd.num();
        SmCurvePropertyType prop = (SmCurvePropertyType)rd.idx();
        bool has_val = rd.flag();
        double val = rd.num();
        SmVector3d vecs[4];
        const SmVector3d* pv = read_vecs(rd, vecs);
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        double tol = rd.num();
        double best = rd.num();
        int64_t leaf = rd.idx();
        {
            SmCurveCache cc(*c, SmExtent1d(a, b), full, chord, angle, offaxis, minseg,
                            aux ? TRUE : FALSE, max3d, ratio);
            SmStatus ts = cc.Tessellate();
            o.push_back(st(ts));
            if (ts == SM_SUCCESS)
            {
                // the curve leaves in pre-order from the top node
                std::map<const void*, int64_t> span_of;
                for (ULONG i = 0; i < cc.GetSpanCount(); ++i)
                    span_of[(const void*)cc.GetAt(i)] = (int64_t)i;
                SmTree* tree = cc.GetTree();
                std::vector<SmTreeNode*> leaves, stack;
                if (tree && tree->GetTopNode())
                    stack.push_back(tree->GetTopNode());
                while (!stack.empty())
                {
                    SmTreeNode* nd = stack.back();
                    stack.pop_back();
                    if (nd->m_pData != NULL && span_of.count((const void*)nd->m_pData))
                        leaves.push_back(nd);
                    if (nd->m_pChild2)
                        stack.push_back(nd->m_pChild2);
                    if (nd->m_pChild1)
                        stack.push_back(nd->m_pChild1);
                }
                o.push_back((double)leaves.size());
                if (!leaves.empty())
                {
                    SmTreeNode* nd = leaves[(size_t)leaf % leaves.size()];
                    SmExtent1d ivl(i0, i1);
                    SmSolutionArray sols;
                    Open<SmCurvePropertyGS> s(tree, ivl, prop, has_val ? &val : NULL, pv);
                    s.set(sop, tol, best, &sols);
                    SmTreeNode* br[SM_GS_MAX_TREES] = {nd, NULL, NULL};
                    SmBoolean needs = FALSE;
                    SmStatus ls = s.LocalSolve(br, needs);
                    o.push_back(st(ls));
                    o.push_back(flg(needs));
                    o.push_back((double)sols.GetSize());
                    for (ULONG i = 0; i < sols.GetSize(); i++)
                        put_sol(o, sols[i], c, NULL);
                }
            }
        }
        delete c;
        break;
    }
    case OP_IS_LINEAR:
    {
        SmCurve* c = read_curve(rd, ctx);
        double tol = rd.num();
        bool want = rd.flag();
        SmPoint3d pt(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        SmVector3d tan(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        double dev = 0.0;
        SmBoolean r = c->IsLinear(tol, &pt, &tan, want ? &dev : NULL);
        o.push_back(0.0);
        o.push_back(flg(r));
        if (r)
        {
            put_vec(o, pt);
            put_vec(o, tan);
        }
        o.push_back(dev);
        delete c;
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_curve_curve_p3_eval(int32_t op, const double* in, int64_t n_in, double* out,
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
