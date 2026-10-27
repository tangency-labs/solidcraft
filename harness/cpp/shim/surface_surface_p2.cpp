// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surface_p2`: the SmSurface members owned by the unit
// (source/SMLib/src/SmSurface.cpp, source/SMLib/inc/SmSurface.h), called through `SmSurface*`
// on a base `SmSurface()` and on SmBSplineSurface objects so the virtual dispatch (derived
// override or base-class default) is upstream's, plus SmSurfaceEval, SmDerivSurfDefinition,
// SmDropCurveFail, the smsurf_* functions and the file-static helpers of SmSurface.cpp.
//
// Static functions and classes local to SmSurface.cpp cannot be linked against and the file
// cannot be recompiled inside a namespace (it defines SmSurface members), so the helpers used
// here are VERBATIM copies of upstream SmSurface.cpp (commit c9c5979), inside the namespace
// sc_srf_p2 (line ranges noted at each block; generated, do not edit by hand).
// SmLSIGlobalSolver's LocalSolve and IdenticalSolutions are defined as SM_ERR stubs: only its
// constructor and BranchMayContainAnswers are exercised.
//
// One entry point, sc_surface_surface_p2_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/surface_surface_p2.rs, which validates every
// input. The input starts with a surface block `ctor` (0: none, static op; 1: SmSurface();
// 2: SmBSplineSurface(gw_SURFACE*, FALSE, NULL) from `oob form n m p q r s cpts(x y z w) u v`;
// 3: a canonical SmTorus or SmCone written with WriteToDB to an ASCII stream, its planar
// generator flag optionally cleared in the record, and read back with SmSurface::ReadFromDB).
// `out` receives the op status (0 = SM_SUCCESS, else the SmStatus value) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAssertArray.h>
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmCone.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveCache.h>
#include <SmDatabaseIO.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmGeomUtility.h>
#include <SmGlobalSolver.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmSurfaceCache.h>
#include <SmTArray.h>
#include <SmTorus.h>
#include <SmTree.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <sstream>
#include <string>
#include <vector>

#include "sc_sm.h"

using sc::flg;
using sc::put_dom;
using sc::put_vec;
using sc::st;

namespace sc_srf_p2
{
// clang-format off
// ---- verbatim: SmSurface.cpp lines 3826-3940: sm_CCLocalSolve
static SmStatus sm_CCLocalSolve
  (const SmCurve         & crCurve1,
   const SmExtent1d      & crInterval1,
   SmContinuityType        aeConts1[2],
   const SmCurve         & crCurve2,
   const SmExtent1d      & crInterval2,
   SmContinuityType        aeConts2[2],
   SmSolverOperationType & eSolverOperation,
   double                  d3dTolerance,
   SmSolutionArray       & rSolutions)
{
  rSolutions.ReSet();

  SM_ASSERT(eSolverOperation == SM_SO_MINIMIZE ||
            eSolverOperation == SM_SO_MAXIMIZE);

  SmBoolean bFoundAnswer;
  SmSolution sSolution;
  sSolution.m_lNumVariables = 2;
  sSolution.m_eSolutionType = SM_ST_SINGLE_VALUE;
  // If we have a total discontinuity (i.e. end of curve)
  // then we always test.  If we have a derivative
  // discontinuity then we just test the ones on the left
  // side - otherwise we would do each point two times.
  SmPoint3d sPnt;
  SmSolution sSol;
  if (aeConts1[0] <= SM_CT_C0)
    {
      SER(crCurve1.EvaluatePoint(crInterval1.GetMin(),sPnt));
      SER(crCurve2.LocalPointSolve(crInterval2,eSolverOperation,sPnt,
          NULL,NULL,NULL,crInterval2.Evaluate(0.5),bFoundAnswer,sSol));
      if (bFoundAnswer)
        {
          sSol.m_lNumVariables = 2;
          sSol.m_vStart[1] = sSol.m_vStart[0];
          sSol.m_vStart[0] = crInterval1.GetMin();
          rSolutions.Add(sSol);
        }
    }
  if (aeConts1[1] <= SM_CT_DISCONTINUOUS)
    {
      SER(crCurve1.EvaluatePoint(crInterval1.GetMax(),sPnt));
      SER(crCurve2.LocalPointSolve(crInterval2,eSolverOperation,sPnt,
          NULL,NULL,NULL,crInterval2.Evaluate(0.5),bFoundAnswer,sSol));
      if (bFoundAnswer)
        {
          sSol.m_lNumVariables = 2;
          sSol.m_vStart[1] = sSol.m_vStart[0];
          sSol.m_vStart[0] = crInterval1.GetMax();
          rSolutions.Add(sSol);
        }
    }
  if (aeConts2[0] <= SM_CT_C0)
    {
      SER(crCurve2.EvaluatePoint(crInterval2.GetMin(),sPnt));
      SER(crCurve1.LocalPointSolve(crInterval1,eSolverOperation,sPnt,
          NULL,NULL,NULL,crInterval1.Evaluate(0.5),bFoundAnswer,sSol));
      if (bFoundAnswer)
        {
          sSol.m_lNumVariables = 2;
          sSol.m_vStart[1] = crInterval2.GetMin();
          rSolutions.Add(sSol);
        }
    }
  if (aeConts2[1] <= SM_CT_DISCONTINUOUS)
    {
      SER(crCurve2.EvaluatePoint(crInterval2.GetMax(),sPnt));
      SER(crCurve1.LocalPointSolve(crInterval1,eSolverOperation,sPnt,
                                   NULL,NULL,NULL,crInterval1.Evaluate(0.5),bFoundAnswer,sSol));
      if (bFoundAnswer)
        {
          sSol.m_lNumVariables = 2;
          sSol.m_vStart[1] = crInterval2.GetMax();
          rSolutions.Add(sSol);
        }
    }

  double daGuess1[4], daGuess2[4];
  ULONG lNumGuess = 0;

  if (lNumGuess == 0)
    {
      // Take midpoint of interval for guess.
      lNumGuess = 1;
      daGuess1[0] = crInterval1.Evaluate(0.5);
      daGuess2[0] = crInterval2.Evaluate(0.5);
    }

  SmSolverOperationType eLocalSolverOp = eSolverOperation;

  for (ULONG i=0; i<lNumGuess; i++)
    {
      SmBoolean bFoundSolution;
      SmSolution sSolut;
      SER(crCurve1.LocalCurveSolve(crInterval1,
                                   crCurve2,
                                   crInterval2,
                                   eLocalSolverOp,
                                   d3dTolerance,
                                   NULL,
                                   NULL,
                                   daGuess1[i],
                                   daGuess2[i],
                                   bFoundSolution,
                                   sSolut ));

      if (bFoundSolution)
        {
          sSolut.m_lNumVariables = 2;
          rSolutions.Add( sSolut );
        }
    }
  return SM_SUCCESS;

} // end sm_CCLocalSolve
// ---- end verbatim

// ---- verbatim: SmSurface.cpp lines 7244-7258: s_MapUV, s_IsEven
static SmPoint2d s_MapUV
 (double       dInU, 
  double       dInV,
  SmOrientType eOrient, 
  double       dDirU, 
  double       dDirV )
{
    SmPoint2d sRet( dInU, dInV );
    if ( dDirU < 0 ) sRet.x = 1 - sRet.x;
    if ( dDirV < 0 ) sRet.y = 1 - sRet.y;
    if ( eOrient == SM_OT_OPPOSITE ) sRet.SwapXY();

    return sRet;
}
static SmBoolean s_IsEven( int iNum ) { return ( (iNum/2) * 2 == iNum ); }
// ---- end verbatim

// ---- verbatim: SmSurface.cpp lines 11574-11614: sm_MergeDoubleArrays
static SmStatus sm_MergeDoubleArrays
  (const SmTArray<double> & crParams1,       // in : 1st input array - ordered by increasing values
   const SmTArray<double> & crParams2,       // in : 2nd input array - ordered by increasing values
   SmTArray<double>       & rSortedResults)  // out: merged array - ordered:[increasing value]
                                             //      no duplicates.
{
  // init output
  rSortedResults.ReSet();

  // low work - one or both of the arrays is empty
  if (crParams1.GetSize() == 0) { rSortedResults.Append(crParams2); return SM_SUCCESS; }
  if (crParams2.GetSize() == 0) { rSortedResults.Append(crParams1); return SM_SUCCESS; }

  // arrive here when two arrays need to be merged

  // iter while either list contains an item
  ULONG lStart1 = 0;
  ULONG lStart2 = 0;
  while (lStart1 < crParams1.GetSize() && lStart2 < crParams2.GetSize())
    {
      double dP1 = (lStart1 < crParams1.GetSize()) ? crParams1[lStart1] : SM_BIG_DOUBLE ;
      double dP2 = (lStart2 < crParams2.GetSize()) ? crParams2[lStart2] : SM_BIG_DOUBLE ;

      // values within SM_EFF_ZERO of one another are merged into one
      if (SM_ARE_SAME(dP1,dP2)) { rSortedResults.Add(dP1);
                                  lStart1 ++;
                                  lStart2 ++;
                                }
      // else add next smallest value to output
      else if (dP1 < dP2)       { rSortedResults.Add(dP1);
                                  lStart1 ++;
                                }
      else                      { rSortedResults.Add(dP2);
                                  lStart2 ++;
                                }
    } // end while any items are left in either input array

  // all done
  return SM_SUCCESS;

} // end sm_MergeDoubleArrays
// ---- end verbatim

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
// ---- end verbatim

// ---- verbatim: SmSurface.cpp lines 17322-17374: sm_CsArePerpenDisjoint
static SmBoolean sm_CsArePerpenDisjoint
  (const SmTreeNode & crCurveNode,
   const SmTreeNode & crSurfaceNode)
{
  // Surface node has a polar box
  if(   crSurfaceNode.m_eAuxDataType == SM_AD_BEZIER_SURFACE
     || crSurfaceNode.m_eAuxDataType == SM_AD_BEZIER_SURFACE_HEAD)
    {
      SmBezierPatch *pBezPatch = (SmBezierPatch*)crSurfaceNode.m_pData;
      SmVector3d sConeVec;
      double dConeAngleRad;
      SER(crSurfaceNode.m_sBBox.ComputeConeBoundTo(crCurveNode.m_sBBox,
                                                   sConeVec,
                                                   dConeAngleRad));
      //
      if (dConeAngleRad > SM_PI/2.0)
        { return FALSE; }

      //
      if (pBezPatch->GetPolarBox().IsDisjointWithCone(sConeVec,dConeAngleRad))
        { return TRUE; }

      //
      if(   crCurveNode.m_eAuxDataType != SM_AD_BEZIER_CURVE
         && crCurveNode.m_eAuxDataType != SM_AD_CURVE_DATA)
        {
          return FALSE;
        }

      //
      SmBezierSpan * pBezSpan    = (SmBezierSpan*)crCurveNode.m_pData; NER(pBezSpan);
      SmCurveCache * pCurveCache = (SmCurveCache*)crCurveNode.m_pTree->GetOwner();
      SmPoint3d     sStart1, sEnd1;
      SmExtent1d    sIvl;

      //
      pBezSpan->GetEnds(*pCurveCache,sStart1,sEnd1,sIvl);
      SmVector3d sLineVec1 = sEnd1 - sStart1;
      if (sLineVec1.LengthSquared() > SM_EFF_ZERO_SQ)
        {
          double dCurveAngleRad = pBezSpan->m_dMaxTurningAngleDeg * SM_PI/180.0 + 3.0*SM_PI/180.0;
          if (smgu_ConesAreDisjointPerpen(sLineVec1,
                                          dCurveAngleRad,
                                          sConeVec,
                                          dConeAngleRad))
            { return TRUE ; }
        }
    }

  // node does not have a polar box to check
  return FALSE;

} // end sm_CsArePerpenDisjoint
// ---- end verbatim

// ---- verbatim: SmSurface.cpp lines 17886-18068: SmLSIGlobalSolver (class, constructor, BranchMayContainAnswers)
class SmLSIGlobalSolver : public SmGlobalSolver
{
public:
  const SmSurface  * m_pSurf;                         // surface being solved for
  const SmExtent2d & m_crUVDomain;                    // intersection domain
  SmPoint3d          m_vLinePoint;
  SmVector3d         m_vLineVector;                   // Is this a UNIT vector ( should NOt be RCLxx )
  const SmExtent1d * m_cpLineInterval;                // Line intersection interval (a reference not owned)
  SmBoolean          m_bRayFire;                      // TRUE = Line intersection interval = [0,infinity]
                                                      //        skip intersections with negative line param values
  double             m_dDistanceTolerance;            // min dist between distinct 3d Points

  // constructor/destructor
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

  //
  virtual SmBoolean BranchMayContainAnswers(SmTreeNode * apBranch[SM_GS_MAX_TREES]);

  // NewtonRaphson Driver: SmSolution->SmSolutionEnd->m_adParameters
  //                         [0] line T
  //                         [1] surface U
  //                         [2] surface V
  virtual SmStatus LocalSolve(SmTreeNode * apBranch[SM_GS_MAX_TREES],
                              SmBoolean & rbNeedsMoreSubdivision);

  // return TRUE when two solutions are identical
  virtual SmSolutionPairType IdenticalSolutions(const SmSolution & crSol1,
                                                const SmSolution & crSol2,
                                                const SmSolution *&pPreferredSol) 
                                               const;

} ; // end class SmLSIGlobalSolver

/*******************************************************************//**
PURPOSE:  SmLSIGlobalSolver constructor

NOTES:
***********************************************************************/
SmLSIGlobalSolver::SmLSIGlobalSolver
  (SmTree *pSurfaceTree,
   const SmExtent2d & crUVDomain,
   const SmPoint3d  & crLinePoint,
   const SmVector3d & crLineVector,
   const SmExtent1d * cpLineInterval,
   SmBoolean bRayFire,
   double dDistanceTolerance)
 : SmGlobalSolver(pSurfaceTree,2),
   m_pSurf(SM_CAST_PTR(SmSurface, pSurfaceTree->GetOwnerObject())),
   m_crUVDomain(crUVDomain),
   m_vLinePoint(crLinePoint),
   m_vLineVector(crLineVector),
   m_cpLineInterval(cpLineInterval),
   m_bRayFire(bRayFire),
   m_dDistanceTolerance(dDistanceTolerance)
{

} // end SmLSIGlobalSolver::SmLSIGlobalSolver constructor

/*******************************************************************//**
PURPOSE: Check the Line/SurfaceTreeNode->SphereBoundingBox intersection
            Return FAlSE when there is no intersection because there is
            no possible line/BezierPatch(es) solution, else return TRUE.

NOTES:
  Return TRUE = Possible Line/BezierPatch(es) intersection
         FALSE= No possible intersections
***********************************************************************/
SmBoolean SmLSIGlobalSolver::BranchMayContainAnswers
  (SmTreeNode * apBranch[SM_GS_MAX_TREES])  // in : apBranch[0] = Surface Cache Tree Node
{
  // If we are normalizing and have a node with a surface do the
  // angular test using the Gauss map of the normal.

  // locals
  SmTreeNode *pSurfaceNode = apBranch[0];

  // compute SphereBound Radius for SurfaceNode with tolerance
  SmPoint3d         sSphCenter;
  double            dSphRadius, dMinRadius;
  const SmExtent3d &rSurfBox  = pSurfaceNode->GetBoundingBox() ;
  rSurfBox.ComputeSphereBound(sSphCenter, dSphRadius, &dMinRadius);
  double            dRadDistSq =   (dSphRadius + m_dDistanceTolerance)
                                 * (dSphRadius + m_dDistanceTolerance) ;

  // get Line/SphereCenter minimum distance
  double dLineParam;
  SER(smgu_LineClosestPoint(m_vLinePoint,m_vLineVector,sSphCenter,dLineParam));
  SmPoint3d sPointOnLine  = m_vLinePoint + dLineParam * m_vLineVector;
  double    dDistToLineSq = sPointOnLine.DistanceBetweenSquared(sSphCenter);

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE ;
  // draw
  if(bDebugMe)
    {
      const SmObject   *pObject  = pSurfaceNode->GetOwnerObject() ;
      SmSurface        *pSurface = pObject->IsKindOf(SmSurface_TYPE) ? (SmSurface *)pObject : NULL ;
      SmFace           *pFace    = pSurface ? (SmFace *)pSurface->GetFace() : NULL ;
      SmBrep           *pBrep    = pFace ? pFace->GetBrep() : NULL ;

      smgfx_Erase() ;
      smgfx_SetLook(1,2, 0,0,1) ; if(pBrep) pBrep->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,1,1) ; if(pSurface) pSurface->DrawUV() ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,0,0) ; if(pFace) pFace->Draw(SM_DM_CROSSHATCH) ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3, 1,0,0) ; pSurfaceNode->DrawBoundingBox() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 1,0,0) ; rSurfBox.Draw(pObject->GetContext()) ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 1,0,0) ; m_vLinePoint.Draw() ; sm_GraphicsLoop() ;
                                  (10.0 * m_vLineVector).Draw(&m_vLinePoint) ; sm_GraphicsLoop() ;
      sm_GraphicsLoop() ;
    }
#endif // SM_DEBUG_CODE

  // when the Line is outside the SphereBound - No Answers possible
  if (dDistToLineSq > dRadDistSq)
    { return FALSE; }

  // For RayFiring - and sphere is behind rayStartPoint
  if (   m_bRayFire
      && dLineParam < 0.0)
    {
      // and rayStart is outside of the sphereBound
      double dDistToStartSq = m_vLinePoint.DistanceBetweenSquared(sSphCenter);
      if (dDistToStartSq > dRadDistSq)
        {
          // no solution possible
          return FALSE;
        }
    } // end RayFire check

  // when given a line interval
  if (m_cpLineInterval)
    {
      // If sphere is below line interval
      if (dLineParam < m_cpLineInterval->GetMin())
        {
          // and intervalStartPoint is outside of SphereBound
          SmPoint3d sStartPt = m_vLinePoint + m_cpLineInterval->GetMin()*m_vLineVector;
          double dDistToStartSq = sStartPt.DistanceBetweenSquared(sSphCenter);
          if (dDistToStartSq > dRadDistSq)
            {
              // no solution possible
              return FALSE;
            }
        }

      // if sphere is above line interval
      if (dLineParam > m_cpLineInterval->GetMax())
        {
          // and intervalEndPoint is outside of SphereBound
          SmPoint3d sEndPt = m_vLinePoint + m_cpLineInterval->GetMax()*m_vLineVector;
          double dDistToStartSq = sEndPt.DistanceBetweenSquared(sSphCenter);
          if (dDistToStartSq > dRadDistSq)
            {
              // no solution possible
              return FALSE;
            }
        }
    } // end need to check interval bounds check

  // check linePoint bounding box intersections
  if(   dDistToLineSq <= (  (dMinRadius + m_dDistanceTolerance)
                          * (dMinRadius + m_dDistanceTolerance))
     || rSurfBox.ContainsPoint3d(sPointOnLine, m_dDistanceTolerance))
    { return TRUE ; }
  else // check lineVector for bounding box intersections
    { ULONG lNumFound ;
      double dTEnter, dTExit ;
      rSurfBox.IntersectLine(sPointOnLine, m_vLineVector, lNumFound, dTEnter, dTExit) ;
      if( lNumFound > 0) { return TRUE ; }
      else               { return FALSE ; }
    }

} // end SmLSIGlobalSolver::BranchMayContainAnswers
// ---- end verbatim

// ---- verbatim: SmSurface.cpp lines 23712-23756: sm_IntersectAndSplit
static SmStatus sm_IntersectAndSplit
  (const SmBSplineCurve & crUVCurve,        // in: Curve to be intersected
   const SmExtent1d     & crInterval,       // in: Curve domain of interest
   const SmPoint3d      & crPlanePnt,       // in:
   const SmPoint3d      & crPlaneNorm,      // in:
   SmTArray<double>     & rSplitParameters) // in/out: Sorted list of curve parameters.
{
    double dPlaneD = - crPlaneNorm.Dot( crPlanePnt );
    SmSolution sData[16];
    SmSolutionArray sSolutions(16,sData);
    ULONG ii, jj;

    SER(crUVCurve.GlobalPropertyAnalysis(crInterval,
                                         SM_CP_PLANE_INTERSECTION,
                                         &dPlaneD,
                                         &crPlaneNorm,
                                         SM_EFF_ZERO_SQRT,
                                         sSolutions));

    for ( ii=0; ii<sSolutions.GetSize(); ii++ )
      {
        double dT = sSolutions[ii].m_vStart[0];
        ULONG lInsertIndex = rSplitParameters.GetSize();
        SmBoolean bSplit = TRUE;
        for ( jj=0; jj<rSplitParameters.GetSize(); jj++ )
          {
            if (smos_Fabs(dT - rSplitParameters[jj]) < SM_EFF_ZERO_SQRT)
              {
                bSplit = FALSE;
                break;
              }
            else if (dT < rSplitParameters[jj])
              {
                lInsertIndex = jj;
                break;
              }
          }
        if ( bSplit )
          { rSplitParameters.InsertAt(lInsertIndex,dT,1); }

      } // end for all solutions

    return SM_SUCCESS;

} // end sm_IntersectAndSplit
// ---- end verbatim

// ---- verbatim: SmSurface.cpp lines 23764-23815: sm_IntersectCurveAndSplit
static SmStatus sm_IntersectCurveAndSplit
  (const SmBSplineCurve & crUVCurve,        // in : Curve to be intersected
   const SmExtent1d     & crInterval,       // in : Curve domain of interest
   const SmLine         & crLine,           // in : Other curve being intersected
   const SmExtent1d     & crLineInterval,   // in : Interval of other curve to intersect
   SmScaledZero           sScaledZero,      // in : Used as the 2d XSect distance for the Global XSect Solver 
   SmTArray<double>     & rSplitParameters) // i/o: Accumulating list of all Curve/Bndry XSects Curve Param vals
{
  // locals
  SmSolution sData[16];
  SmSolutionArray sSolutions(16,sData);
  ULONG ii, jj;
    
  // Find UVCurve/Line 2d XSects
  SER(crUVCurve.GlobalCurveIntersect(crInterval, 
                                     crLine, 
                                     crLineInterval, 
                                     sScaledZero,
                                     sSolutions));

  // for every solution  - sort new solutions into rSplitParameters
  for ( ii=0; ii<sSolutions.GetSize(); ii++ )
    {
      double    dT           = sSolutions[ii].m_vStart[0];
      ULONG     lInsertIndex = rSplitParameters.GetSize();
      SmBoolean bSplit       = TRUE;

      // for every input SplitParam - Find index for insertion - skip duplicates
      for ( jj=0; jj<rSplitParameters.GetSize(); jj++ )
        {
          if (smos_Fabs(dT - rSplitParameters[jj]) < SmTol::GetEffZeroParam())
            {
              bSplit = FALSE;
              break;
            }
          else if (dT < rSplitParameters[jj])
            {
              lInsertIndex = jj;
              break;
            }
        }

      // insert new sorted solutions
      if ( bSplit )
        { rSplitParameters.InsertAt(lInsertIndex,dT,1); }

    } // end for all solutions

  // all done
  return SM_SUCCESS;

} // end sm_IntersectCurveAndSplit
// ---- end verbatim

// ---- verbatim: SmSurface.cpp lines 24009-24033: sm_SetUpBreaks
static SmStatus sm_SetUpBreaks
  (SmTArray<double> & rBreaks,
   const SmExtent1d & rIvl)
{
    while (!rIvl.ContainsValue(rBreaks[0])) {
       rBreaks.RemoveAt(0,1);
       if (rBreaks.GetSize() == 0) {
           rBreaks.Add(rIvl.GetMin());
           break;
       }
    }
    if (smos_Fabs(rBreaks[0]-rIvl.GetMin()) > SM_EFF_ZERO_SQRT) {
        rBreaks.InsertAt(0,rIvl.GetMin(),1);
    }

    while (!rIvl.ContainsValue(rBreaks.GetLast())) {
        rBreaks.RemoveLast();
        if (rBreaks.GetSize() == 1) break;
    }
    if (smos_Fabs(rBreaks.GetLast()-rIvl.GetMax()) > SM_EFF_ZERO_SQRT) {
        rBreaks.Add(rIvl.GetMax());
    }
    return SM_SUCCESS;

} // end sm_SetUpBreaks
// ---- end verbatim

// clang-format on

// LocalSolve and IdenticalSolutions of SmLSIGlobalSolver are other units' (surface.surface_p10,
// surface.surface_p5); stubbed so the class links.
SmStatus SmLSIGlobalSolver::LocalSolve(SmTreeNode*[SM_GS_MAX_TREES], SmBoolean&)
{
    return SM_ERR;
}
SmSolutionPairType SmLSIGlobalSolver::IdenticalSolutions(const SmSolution&, const SmSolution&,
                                                         const SmSolution*&) const
{
    return SM_SP_DIFFCOUNTS;
}
} // namespace sc_srf_p2

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_surface_p2.rs).
enum : int32_t
{
    OP_EVAL = 0,
    OP_EVAL_PT = 1,
    OP_QUERIES = 2,
    OP_KNOTS = 3,
    OP_PARALLEL = 4,
    OP_REVERSE = 5,
    OP_REPARAM = 6,
    OP_SWAP = 7,
    OP_TRANSFORM = 8,
    OP_TRIM = 9,
    OP_SPLIT = 10,
    OP_STEP_INV = 11,
    OP_EXTEND = 12,
    OP_EXTEND_DIR = 13,
    OP_ADJUST_STEP = 14,
    OP_MAX_ANAL = 15,
    OP_COPY = 16,
    OP_COPY_ANAL = 17,
    OP_DROP_FAST = 18,
    OP_CONVERT_UV = 19,
    OP_SEVAL = 20,
    OP_GSSI = 21,
    // static ops
    OP_LIFT2 = 40,
    OP_LIFT3 = 41,
    OP_SCALE_STEP = 42,
    OP_GEOM = 43,
    OP_POINT_TYPE = 44,
    OP_DROP_VECS = 45,
    OP_PARAM_DELTA = 46,
    OP_MERGE = 47,
    OP_MAP_UV = 48,
    OP_IS_EVEN = 49,
    OP_BREAKS = 50,
    OP_LIMITS = 51,
    OP_DERIV_ASSERT = 52,
    OP_CROSS = 53,
    OP_CROSS_SKIN = 54,
    OP_DCF = 55,
    OP_CC_LOCAL = 56,
    OP_ISECT_SPLIT = 57,
    OP_ISECT_CURVE_SPLIT = 58,
    OP_LSI = 59,
    OP_CS_PERPEN = 60,
    OP_PLANE_3D = 61,
    OP_READ_DB = 62,
};

enum : int32_t
{
    CT_NONE = 0,
    CT_BASE = 1,
    CT_BSPLINE = 2,
    CT_REVOLUTION = 3,
};

struct Peek : SmBSplineSurface
{
    static SmKnotType SmBSplineSurface::*kt() { return &Peek::m_eKnotType; }
    static SmBSplineSurfaceForm SmBSplineSurface::*form() { return &Peek::m_eBSplineSurfaceForm; }
};

struct EvalPeek : SmSurfaceEval
{
    static SmVector3d (SmSurfaceEval::*pvv())[3][3] { return &EvalPeek::m_sPVV; }
    static SmBoolean SmSurfaceEval::*uv_set() { return &EvalPeek::m_bUVSet; }
    static SmBoolean SmSurfaceEval::*normal_set() { return &EvalPeek::m_bNormalSet; }
    static SmBoolean SmSurfaceEval::*sect_set() { return &EvalPeek::m_bSectDirSet; }
    static SmBoolean SmSurfaceEval::*fails() { return &EvalPeek::m_bFailsEval; }
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
    SmExtent1d ivl()
    {
        double a = num();
        double b = num();
        return SmExtent1d(a, b);
    }
    SmExtent3d box()
    {
        SmVector3d a = vec();
        SmVector3d b = vec();
        SmExtent3d e;
        e.AddPoint3d(a);
        e.AddPoint3d(b);
        return e;
    }
};

void put2(std::vector<double>& o, const SmVector2d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
}

void put_str(std::vector<double>& o, const TCHAR* s)
{
    std::vector<double> c;
    for (const TCHAR* p = s; p && *p; ++p)
        c.push_back((double)(unsigned)*p);
    o.push_back((double)c.size());
    o.insert(o.end(), c.begin(), c.end());
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

// `has` then `has_nurb oob borrowed knot_type form [n m p q r s cpts u v]`; the base class has
// no state (`has` = 0).
void put_state(std::vector<double>& o, const SmSurface* srf)
{
    const SmBSplineSurface* s = SM_CAST_PTR(SmBSplineSurface, srf);
    o.push_back(flg(s != NULL));
    if (!s)
        return;
    const gw_SURFACE* sur = s->GetGwNurbPointer();
    o.push_back(flg(sur != NULL));
    o.push_back(flg(s->IsOutOfBoundsEnabled()));
    o.push_back(flg(s->IsNurbBorrowed()));
    o.push_back((double)(s->*Peek::kt()));
    o.push_back((double)(s->*Peek::form()));
    if (!sur)
        return;
    gw_INDEX n = sur->net->n;
    gw_INDEX m = sur->net->m;
    gw_INDEX r = sur->knu->m;
    gw_INDEX ss = sur->knv->m;
    o.push_back((double)n);
    o.push_back((double)m);
    o.push_back((double)sur->p);
    o.push_back((double)sur->q);
    o.push_back((double)r);
    o.push_back((double)ss);
    for (gw_INDEX i = 0; i <= n; i++)
        for (gw_INDEX j = 0; j <= m; j++)
        {
            o.push_back(sur->net->Pw[i][j].x);
            o.push_back(sur->net->Pw[i][j].y);
            o.push_back(sur->net->Pw[i][j].z);
            o.push_back(sur->net->Pw[i][j].w);
        }
    for (gw_INDEX i = 0; i <= r; i++)
        o.push_back(sur->knu->U[i]);
    for (gw_INDEX i = 0; i <= ss; i++)
        o.push_back(sur->knv->U[i]);
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

std::vector<SmVector3d> read_vecs(Rd& rd, int64_t k)
{
    std::vector<SmVector3d> v;
    for (int64_t i = 0; i < k; i++)
        v.push_back(rd.vec());
    return v;
}

SmTArray<SmVector3d> to_tarray(const std::vector<SmVector3d>& v)
{
    SmTArray<SmVector3d> a;
    for (const SmVector3d& x : v)
        a.Add(x);
    return a;
}

void put_solutions(std::vector<double>& o, SmSolutionArray& sols)
{
    o.push_back((double)sols.GetSize());
    for (ULONG i = 0; i < sols.GetSize(); i++)
    {
        const SmSolution& s = sols[i];
        o.push_back((double)s.m_lNumVariables);
        o.push_back((double)s.m_eSolutionType);
        o.push_back(s.m_vStart.m_dSolutionValue);
        o.push_back(s.m_vStart[0]);
        o.push_back(s.m_vStart[1]);
    }
}

void put_doubles(std::vector<double>& o, const SmTArray<double>& a)
{
    o.push_back((double)a.GetSize());
    for (ULONG i = 0; i < a.GetSize(); i++)
        o.push_back(a[i]);
}

void put_seval(std::vector<double>& o, SmSurfaceEval& e)
{
    o.push_back(flg(e.*EvalPeek::uv_set() == TRUE));
    o.push_back(flg(e.*EvalPeek::normal_set() == TRUE));
    o.push_back(flg(e.*EvalPeek::sect_set() == TRUE));
    o.push_back(flg(e.*EvalPeek::fails() == TRUE));
    put2(o, e.GetUV());
}

void surface_op(int32_t op, Rd& rd, const SmContext& ctx, SmSurface* s, std::vector<double>& o)
{
    switch (op)
    {
    case OP_EVAL:
    {
        SmPoint2d uv = rd.pt2();
        ULONG hu = (ULONG)rd.idx();
        ULONG hv = (ULONG)rd.idx();
        bool ul = rd.flag();
        bool vl = rd.flag();
        bool upper = rd.flag();
        bool nzt = rd.flag();
        std::vector<SmVector3d> d((hu + 1) * (hv + 1), SmVector3d(0.0, 0.0, 0.0));
        SmStatus stt = s->Evaluate(uv, hu, hv, ul ? TRUE : FALSE, vl ? TRUE : FALSE,
                                   upper ? TRUE : FALSE, d.data(), nzt ? TRUE : FALSE);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            for (ULONG i = 0; i <= hu; i++)
                for (ULONG j = 0; j <= hv; j++)
                    if (!upper || i + j <= hu)
                        put_vec(o, d[i * (hv + 1) + j]);
        break;
    }
    case OP_EVAL_PT:
    {
        SmPoint2d uv = rd.pt2();
        SmPoint3d p;
        SmStatus stt = s->EvaluatePoint(uv, p);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_vec(o, p);
        break;
    }
    case OP_QUERIES:
    {
        o.push_back(0.0);
        put_dom(o, s->GetNaturalUVDomain());
        put_dom(o, s->GetSTEPUVDomain());
        o.push_back((double)s->GetDegree(SM_SP_U));
        o.push_back((double)s->GetDegree(SM_SP_V));
        o.push_back((double)s->GetNumberControlPoints(SM_SP_U));
        o.push_back((double)s->GetNumberControlPoints(SM_SP_V));
        o.push_back(flg(s->IsBilinear()));
        o.push_back((double)s->GetType());
        put_str(o, s->GetTypeString());
        put_str(o, s->GetClassString());
        const SM_TYPE types[] = {SmSurface_TYPE, SmBSplineSurface_TYPE, SmPlane_TYPE,
                                 SmAObject_TYPE, SmObject_TYPE,          SmCurve_TYPE,
                                 SmCone_TYPE,    SmTorus_TYPE};
        for (SM_TYPE t : types)
            o.push_back(flg(s->IsKindOf(t)));
        break;
    }
    case OP_KNOTS:
    {
        SmSurfParamType dir = (SmSurfParamType)rd.idx();
        bool has_ivl = rd.flag();
        SmExtent1d ivl = rd.ivl();
        SmTArray<double> knots;
        SmTArray<ULONG> mults;
        SmStatus stt = s->GetKnots(dir, knots, &mults, has_ivl ? &ivl : NULL);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            put_doubles(o, knots);
            o.push_back((double)mults.GetSize());
            for (ULONG i = 0; i < mults.GetSize(); i++)
                o.push_back((double)mults[i]);
        }
        break;
    }
    case OP_PARALLEL:
    {
        SmVector3d v = rd.vec();
        o.push_back(0.0);
        o.push_back(flg(s->IsParallelToVector(v)));
        break;
    }
    case OP_REVERSE:
    {
        SmSurfParamType dir = (SmSurfParamType)rd.idx();
        o.push_back(st(s->Reverse(dir)));
        put_state(o, s);
        break;
    }
    case OP_REPARAM:
    {
        SmExtent2d d = rd.domain();
        o.push_back(st(s->Reparameterize(d)));
        put_state(o, s);
        break;
    }
    case OP_SWAP:
    {
        o.push_back(st(s->SwapUV()));
        put_state(o, s);
        break;
    }
    case OP_TRANSFORM:
    {
        SmVector3d org = rd.vec();
        SmVector3d z = rd.vec();
        SmVector3d x = rd.vec();
        bool has_scale = rd.flag();
        SmVector3d scale = rd.vec();
        SmAxis2Placement ax(org, z, x);
        o.push_back(st(s->Transform(ax, has_scale ? &scale : NULL)));
        put_state(o, s);
        break;
    }
    case OP_TRIM:
    {
        SmExtent2d d = rd.domain();
        o.push_back(st(s->TrimWithDomain(d)));
        put_dom(o, d);
        put_state(o, s);
        break;
    }
    case OP_SPLIT:
    {
        double t = rd.num();
        SmSurfParamType dir = (SmSurfParamType)rd.idx();
        SmSurface* l = NULL;
        SmSurface* r = NULL;
        SmStatus stt = s->SplitAt(ctx, t, dir, l, r);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            put_state(o, l);
            put_state(o, r);
        }
        delete l;
        delete r;
        break;
    }
    case OP_STEP_INV:
    {
        SmExtent2d d = rd.domain();
        SmVector3d p = rd.vec();
        double tol = rd.num();
        bool has_guess = rd.flag();
        SmPoint2d guess = rd.pt2();
        SmPoint2d uv;
        SmLocationType loc = SM_LT_EXTERIOR;
        SmStatus stt = s->STEPInversion(d, p, tol, uv, loc, has_guess ? &guess : NULL);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            put2(o, uv);
            o.push_back((double)loc);
        }
        break;
    }
    case OP_EXTEND:
    case OP_EXTEND_DIR:
    {
        SmSurfParamType dir = SM_SP_U;
        if (op == OP_EXTEND_DIR)
            dir = (SmSurfParamType)rd.idx();
        double dist = rd.num();
        SmContinuityType cont = (SmContinuityType)rd.idx();
        SmSurface* e = NULL;
        SmStatus stt = op == OP_EXTEND ? s->CreateExtendedSurface(ctx, dist, cont, e)
                                       : s->CreateExtendedSurface(ctx, dir, dist, cont, e);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_state(o, e);
        delete e;
        break;
    }
    case OP_ADJUST_STEP:
    {
        SmExtent2d d = rd.domain();
        o.push_back(st(s->AdjustSTEPUVDomain(d)));
        put_state(o, s);
        break;
    }
    case OP_MAX_ANAL:
    {
        SmExtent2d d = s->GetMaxAnalyticDomain();
        o.push_back(0.0);
        put_dom(o, d);
        break;
    }
    case OP_COPY:
    case OP_COPY_ANAL:
    {
        SmSurface* c = NULL;
        SmStatus stt = op == OP_COPY ? s->Copy(ctx, c) : s->CopyAndAddAnalytics(ctx, c);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back((double)c->GetType());
            put_state(o, c);
        }
        delete c;
        break;
    }
    case OP_DROP_FAST:
    {
        SmExtent2d d = rd.domain();
        SmVector3d p = rd.vec();
        SmSolutionArray sols;
        SmStatus stt = s->DropPointFast(d, SM_SO_MINIMIZE, p, NULL, 1.0e-6, NULL, SM_SR_ALL, sols);
        o.push_back(st(stt));
        break;
    }
    case OP_CONVERT_UV:
    {
        SmPoint2d uv = rd.pt2();
        SmPoint2d a, b;
        SmStatus s1 = s->ConvertUVFromSTEPToNURBS(uv, a);
        SmStatus s2 = s->ConvertUVFromNURBSToSTEP(uv, b);
        o.push_back(st(s1 != SM_SUCCESS ? s1 : s2));
        put2(o, a);
        put2(o, b);
        break;
    }
    case OP_SEVAL:
    {
        int32_t ctor = (int32_t)rd.idx();
        double u0 = rd.num();
        double v0 = rd.num();
        SmVector3d dir0 = rd.vec();
        SmSurfaceEval e0(s);
        SmSurfaceEval e1(s, u0, v0);
        SmSurfaceEval e2(s, u0, v0, dir0);
        SmSurfaceEval e;
        e = ctor == 0 ? e0 : ctor == 1 ? e1 : e2;
        o.push_back(0.0);
        put_seval(o, e);
        int64_t k = rd.idx();
        for (int64_t i = 0; i < k; i++)
        {
            int32_t kind = (int32_t)rd.idx();
            double a = rd.num();
            double b = rd.num();
            double c = rd.num();
            switch (kind)
            {
            case 0: e.SetUV(SmPoint2d(a, b)); break;
            case 1: e.SetUV(a, b); break;
            case 2: e.SetSectDir(SmVector3d(a, b, c)); break;
            default:
            {
                bool fails = e.FailsEval(FALSE) == TRUE;
                o.push_back(fails ? (double)SM_ERR : 0.0);
                if (!fails)
                    for (int ii = 0; ii < 3; ii++)
                        for (int jj = 0; ii + jj <= 2; jj++)
                            put_vec(o, (e.*EvalPeek::pvv())[ii][jj]);
                break;
            }
            }
            put_seval(o, e);
        }
        break;
    }
    case OP_GSSI:
    {
        SmExtent2d d = rd.domain();
        SmBSplineSurface* other = read_bspline(rd, ctx);
        SmExtent2d d2 = rd.domain();
        bool has_tol = rd.flag();
        SmApproxTol3d tol = rd.num();
        bool has_move = rd.flag();
        SmVector3d move = rd.vec();
        // prog_test my_test_one_ssi: both B-reps translated by the same vector first
        if (has_move)
        {
            SmAxis2Placement ax;
            ax.Translate(move);
            SmStatus ts = s->Transform(ax, NULL);
            if (ts == SM_SUCCESS)
                ts = other->Transform(ax, NULL);
            if (ts != SM_SUCCESS)
            {
                o.push_back(st(ts));
                delete other;
                break;
            }
        }
        // all outputs requested, as smTopoTest my_test_ssi does
        SmBoolean edges[2] = {TRUE, TRUE};
        SmTArray<SmCurve*> c3, c1, c2;
        SmTArray<SmTsectCurveType> types;
        SmTArray<double> devs;
        SmStatus stt = s->GlobalSurfaceIntersect(ctx, d, *other, d2, edges, has_tol ? &tol : NULL,
                                                 NULL, &c3, &c1, &c2, &types, &devs);
        o.push_back(st(stt));
        // on success: `n`, `same` (the output arrays agree in length) and per curve: 3D
        // curve, UV curve on this, UV curve on other (each `GetType()`, `0` when null, then
        // `put_curve` for a B-spline), type, deviation
        if (stt == SM_SUCCESS)
        {
            ULONG n = c3.GetSize();
            o.push_back((double)n);
            bool same = c1.GetSize() == n && c2.GetSize() == n && types.GetSize() == n &&
                        devs.GetSize() == n;
            o.push_back(flg(same));
            for (ULONG i = 0; same && i < n; i++)
            {
                const SmCurve* cs[3] = {c3[i], c1[i], c2[i]};
                for (const SmCurve* c : cs)
                {
                    o.push_back(c != NULL ? (double)c->GetType() : 0.0);
                    if (c != NULL && c->IsKindOf(SmBSplineCurve_TYPE))
                        put_curve(o, (const SmBSplineCurve*)c);
                }
                o.push_back((double)types[i]);
                o.push_back(devs[i]);
            }
        }
        for (ULONG i = 0; i < c3.GetSize(); i++)
            delete c3[i];
        for (ULONG i = 0; i < c1.GetSize(); i++)
            delete c1[i];
        for (ULONG i = 0; i < c2.GetSize(); i++)
            delete c2[i];
        delete other;
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}

void static_op(int32_t op, Rd& rd, SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_LIFT2:
    {
        SmVector3d v[7];
        for (int i = 0; i < 7; i++)
            v[i] = rd.vec();
        o.push_back(0.0);
        put_vec(o, smsurf_LiftSecondDerivative(v[0], v[1], v[2], v[3], v[4], v[5], v[6]));
        break;
    }
    case OP_LIFT3:
    {
        SmVector3d v[12];
        for (int i = 0; i < 12; i++)
            v[i] = rd.vec();
        o.push_back(0.0);
        put_vec(o, smsurf_LiftThirdDerivative(v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7],
                                              v[8], v[9], v[10], v[11]));
        break;
    }
    case OP_SCALE_STEP:
    {
        ULONG hu = (ULONG)rd.idx();
        ULONG hv = (ULONG)rd.idx();
        bool ua = rd.flag();
        bool va = rd.flag();
        std::vector<SmVector3d> d = read_vecs(rd, (int64_t)((hu + 1) * (hv + 1)));
        smsurf_ScaleSTEPAngularDerivatives(d.data(), hu, hv, ua ? TRUE : FALSE, va ? TRUE : FALSE);
        o.push_back(0.0);
        for (const SmVector3d& x : d)
            put_vec(o, x);
        break;
    }
    case OP_GEOM:
    {
        SmVector3d e[3][3];
        for (int i = 0; i < 3; i++)
            for (int j = 0; j < 3; j++)
                e[i][j] = rd.vec();
        double k, h, k1, k2;
        SmVector3d efg, lmn, v1, v2;
        SmStatus stt = smsurf_EvaluateGeometric(e, k, h, k1, k2, efg, lmn, v1, v2);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(k);
            o.push_back(h);
            o.push_back(k1);
            o.push_back(k2);
            put_vec(o, efg);
            put_vec(o, lmn);
            put_vec(o, v1);
            put_vec(o, v2);
            put_vec(o, e[1][0]);
            put_vec(o, e[0][1]);
        }
        break;
    }
    case OP_POINT_TYPE:
    {
        SmVector3d n = rd.vec();
        SmVector3d uu = rd.vec();
        SmVector3d vv = rd.vec();
        bool want_uu = rd.flag();
        bool want_vv = rd.flag();
        SmSurfaceSideType su = (SmSurfaceSideType)rd.idx();
        SmSurfaceSideType sv = SM_SS_ON;
        SmLocalSurfaceType t =
            smsurf_ComputeSurfacePointType(n, uu, vv, want_uu ? &su : NULL, want_vv ? &sv : NULL);
        o.push_back(0.0);
        o.push_back((double)t);
        o.push_back((double)su);
        break;
    }
    case OP_DROP_VECS:
    {
        SmVector3d du = rd.vec();
        SmVector3d dv = rd.vec();
        int64_t k = rd.idx();
        std::vector<SmVector3d> v = read_vecs(rd, k);
        std::vector<SmVector2d> r((size_t)k + 1, SmVector2d(0.0, 0.0));
        SmStatus stt = smsurf_DropVectors(du, dv, (ULONG)k, v.data(), r.data());
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            for (int64_t i = 0; i < k; i++)
                put2(o, r[(size_t)i]);
        break;
    }
    case OP_PARAM_DELTA:
    {
        SmVector3d t = rd.vec();
        SmVector3d d = rd.vec();
        o.push_back(0.0);
        o.push_back(sc_srf_p2::sm_ConvertToParameterDelta(t, d));
        break;
    }
    case OP_MERGE:
    {
        SmTArray<double> a, b, r;
        int64_t na = rd.idx();
        for (int64_t i = 0; i < na; i++)
            a.Add(rd.num());
        int64_t nb = rd.idx();
        for (int64_t i = 0; i < nb; i++)
            b.Add(rd.num());
        SmStatus stt = sc_srf_p2::sm_MergeDoubleArrays(a, b, r);
        o.push_back(st(stt));
        put_doubles(o, r);
        break;
    }
    case OP_MAP_UV:
    {
        double u = rd.num();
        double v = rd.num();
        SmOrientType ot = (SmOrientType)rd.idx();
        double du = rd.num();
        double dv = rd.num();
        o.push_back(0.0);
        put2(o, sc_srf_p2::s_MapUV(u, v, ot, du, dv));
        break;
    }
    case OP_IS_EVEN:
    {
        int i = (int)rd.num();
        o.push_back(0.0);
        o.push_back(flg(sc_srf_p2::s_IsEven(i)));
        break;
    }
    case OP_BREAKS:
    {
        SmExtent1d ivl = rd.ivl();
        SmTArray<double> b;
        int64_t n = rd.idx();
        for (int64_t i = 0; i < n; i++)
            b.Add(rd.num());
        SmStatus stt = sc_srf_p2::sm_SetUpBreaks(b, ivl);
        o.push_back(st(stt));
        put_doubles(o, b);
        break;
    }
    case OP_LIMITS:
    {
        SmVector3d p = rd.vec();
        o.push_back(0.0);
        o.push_back(SmSurface::GetDerivativeLimit(p));
        o.push_back(SmSurface::GetNormalLimit());
        break;
    }
    case OP_DERIV_ASSERT:
    {
        SmBSplineCurve dummy;
        SmDerivSurfDefinition d;
        d.m_lNumSamples = (ULONG)rd.idx();
        d.m_eTangencyType = (SmTangencyType)rd.idx();
        d.m_dStartDerivDirAngleDeg = rd.num();
        d.m_dEndDerivDirAngleDeg = rd.num();
        d.m_pTangentReference = rd.flag() ? &dummy : NULL;
        d.m_dStartTanLength = rd.num();
        d.m_dEndTanScale = rd.num();
        d.m_dStartAngleDeg = rd.num();
        d.m_dEndAngleDeg = rd.num();
        d.m_vVector = rd.vec();
        d.m_eCurvatureType = (SmCurvatureType)rd.idx();
        d.m_eBlendEndType = (SmBlendEndType)rd.idx();
        d.m_dStartRadius = rd.num();
        d.m_dEndRadScale = rd.num();
        d.m_pCurvatureReference = rd.flag() ? &dummy : NULL;
        d.m_bFlipCurvature = rd.flag() ? TRUE : FALSE;
        bool with_list = rd.flag();
        SmAssertArray list;
        SmBoolean ok = d.AssertValid(with_list ? &list : NULL);
        o.push_back(0.0);
        o.push_back(flg(ok == TRUE));
        o.push_back((double)list.GetSize());
        for (ULONG i = 0; i < list.GetSize(); i++)
        {
            const SmAssertReport* r = list[i];
            o.push_back((double)r->GetTestIndex());
            o.push_back((double)r->GetTol3d());
            o.push_back(r->GetValue());
        }
        o.push_back(flg(d.IsFromSurfaceBoundaryCurve() == TRUE));
        o.push_back((double)d.GetNumDerivs());
        o.push_back((double)d.GetClassType());
        put_str(o, d.GetClassString());
        // the default constructor
        SmDerivSurfDefinition def;
        o.push_back((double)def.m_lNumSamples);
        o.push_back((double)def.m_eTangencyType);
        o.push_back((double)def.m_eCurvatureType);
        o.push_back((double)def.m_eBlendEndType);
        o.push_back(def.m_dStartTanLength);
        o.push_back(def.m_dEndTanScale);
        put_vec(o, def.m_vVector);
        break;
    }
    case OP_CROSS:
    case OP_CROSS_SKIN:
    {
        SmDerivSurfDefinition d;
        int64_t n = rd.idx();
        int64_t flags = rd.idx(); // bits: s2 s3 ep e1 e2 e3
        std::vector<SmVector3d> sp = read_vecs(rd, n);
        int64_t n1 = rd.idx();
        std::vector<SmVector3d> s1 = read_vecs(rd, n1);
        SmTArray<SmVector3d> asp = to_tarray(sp), as1 = to_tarray(s1);
        SmTArray<SmVector3d> opt[6];
        for (int b = 0; b < 6; b++)
            if (flags & (1 << b))
            {
                int64_t k = rd.idx();
                opt[b] = to_tarray(read_vecs(rd, k));
            }
        auto p = [&](int b) { return (flags & (1 << b)) ? &opt[b] : (SmTArray<SmVector3d>*)NULL; };
        SmTArray<SmBSplineCurve*> secs;
        SmStatus stt = d.CreateCrossSectionCurves(ctx, secs, asp, as1, p(0), p(1), p(2), p(3), p(4), p(5));
        o.push_back(st(stt));
        if (op == OP_CROSS)
        {
            if (stt == SM_SUCCESS)
            {
                o.push_back((double)secs.GetSize());
                for (ULONG i = 0; i < secs.GetSize(); i++)
                    put_curve(o, secs[i]);
            }
        }
        else
        {
            // rails and skin parameters follow
            SmTArray<double> params;
            int64_t np = rd.idx();
            for (int64_t i = 0; i < np; i++)
                params.Add(rd.num());
            SmBSplineCurve* r1 = read_curve(rd, ctx);
            SmBSplineCurve* r2 = rd.flag() ? read_curve(rd, ctx) : NULL;
            double tol = rd.num();
            if (stt == SM_SUCCESS)
            {
                SmBSplineSurface* skin = NULL;
                SmStatus s2 = d.SkinDerivSurface(ctx, skin, secs, params, *r1, r2, tol);
                o.push_back(st(s2));
                if (s2 == SM_SUCCESS)
                    put_state(o, skin);
                delete skin;
            }
            delete r1;
            delete r2;
        }
        for (ULONG i = 0; i < secs.GetSize(); i++)
            delete secs[i];
        break;
    }
    case OP_DCF:
    {
        SmDropCurveFail f;
        o.push_back(0.0);
        o.push_back(flg(f.IsUnInit() == TRUE));
        SmStatus fs = (SmStatus)rd.idx();
        ULONG from = (ULONG)rd.idx();
        SmCreateUVTrimCurvePathType path = (SmCreateUVTrimCurvePathType)rd.idx();
        bool has_ctx = rd.flag();
        SmExtent2d dom = rd.domain();
        SmExtent1d ivl = rd.ivl();
        double tol = rd.num();
        double a10 = rd.num();
        double a11 = rd.num();
        SmBoolean b13 = (SmBoolean)rd.idx();
        SmBoolean b14 = (SmBoolean)rd.idx();
        SmBoolean b15 = (SmBoolean)rd.idx();
        SmBoolean b16 = (SmBoolean)rd.idx();
        SmBoolean b17 = (SmBoolean)rd.idx();
        double a18 = rd.num();
        SmBoolean b19 = (SmBoolean)rd.idx();
        SmBoolean b20 = (SmBoolean)rd.idx();
        SmSurfParamType clos = (SmSurfParamType)rd.idx();
        ULONG sing = (ULONG)rd.idx();
        ULONG asing = (ULONG)rd.idx();
        int64_t npp = rd.idx();
        SmTArray<SmPoint3d> poles = to_tarray(read_vecs(rd, npp));
        int64_t npn = rd.idx();
        SmTArray<SmPoint3d> normals = to_tarray(read_vecs(rd, npn));
        int64_t ncp = rd.idx();
        SmTArray<SmPoint3d> poly = to_tarray(read_vecs(rd, ncp));
        SmTArray<double> knots;
        int64_t nk = rd.idx();
        for (int64_t i = 0; i < nk; i++)
            knots.Add(rd.num());
        int64_t nc = rd.idx();
        SmTArray<SmBSplineCurve*> uvs;
        for (int64_t i = 0; i < nc; i++)
            uvs.Add(read_curve(rd, ctx));
        f.Set(fs, from, path, has_ctx ? &ctx : NULL, dom, NULL, NULL, ivl, tol, a10, a11, NULL, b13,
              b14, b15, b16, b17, a18, b19, b20, clos, &poles, &normals, sing, asing, NULL, &poly,
              &knots, &uvs, NULL);
        auto dump = [&](const SmDropCurveFail& g) {
            o.push_back((double)g.m_sFailStatus);
            o.push_back((double)g.m_lFromCall);
            o.push_back((double)g.m_eCreatePathType);
            o.push_back(flg(g.m_cpContext != NULL));
            put_dom(o, g.m_sUVDomain);
            o.push_back(g.m_sInterval.GetMin());
            o.push_back(g.m_sInterval.GetMax());
            o.push_back((double)g.m_sApproxTol3d);
            o.push_back(g.m_dMaxDropToSurfArg);
            o.push_back(g.m_dMaxApproxDev);
            o.push_back((double)g.m_bKeepAllDropCurves);
            o.push_back((double)g.m_bDegenCurve);
            o.push_back((double)g.m_bDegenSurfU);
            o.push_back((double)g.m_bDegenSurfV);
            o.push_back((double)g.m_bDropDomainIsContained);
            o.push_back(g.m_dMaxDropToSurf);
            o.push_back((double)g.m_sSolutions.GetSize());
            o.push_back((double)g.m_sUVCurves.GetSize());
            for (ULONG i = 0; i < g.m_sUVCurves.GetSize(); i++)
                put_curve(o, g.m_sUVCurves[i]);
            o.push_back((double)g.m_bClosedU);
            o.push_back((double)g.m_bClosedV);
            o.push_back((double)g.m_eSrfClosure);
            o.push_back((double)g.m_sSrfPolePoints.GetSize());
            for (ULONG i = 0; i < g.m_sSrfPolePoints.GetSize(); i++)
                put_vec(o, g.m_sSrfPolePoints[i]);
            o.push_back((double)g.m_sSrfPoleNormals.GetSize());
            for (ULONG i = 0; i < g.m_sSrfPoleNormals.GetSize(); i++)
                put_vec(o, g.m_sSrfPoleNormals[i]);
            o.push_back((double)g.m_lSingularities);
            o.push_back((double)g.m_lApproxSingularities);
            o.push_back((double)g.m_sCtrlPoly.GetSize());
            for (ULONG i = 0; i < g.m_sCtrlPoly.GetSize(); i++)
                put_vec(o, g.m_sCtrlPoly[i]);
            o.push_back((double)g.m_sKnots.GetSize());
            for (ULONG i = 0; i < g.m_sKnots.GetSize(); i++)
                o.push_back(g.m_sKnots[i]);
        };
        o.push_back(flg(f.IsUnInit() == TRUE));
        dump(f);
        SmDropCurveFail g;
        g = f;
        dump(g);
        f.UnInit();
        o.push_back(flg(f.IsUnInit() == TRUE));
        dump(f);
        for (ULONG i = 0; i < uvs.GetSize(); i++)
            delete uvs[i];
        break;
    }
    case OP_CC_LOCAL:
    {
        SmBSplineCurve* c1 = read_curve(rd, ctx);
        SmExtent1d i1 = rd.ivl();
        SmContinuityType k1[2];
        k1[0] = (SmContinuityType)rd.idx();
        k1[1] = (SmContinuityType)rd.idx();
        SmBSplineCurve* c2 = read_curve(rd, ctx);
        SmExtent1d i2 = rd.ivl();
        SmContinuityType k2[2];
        k2[0] = (SmContinuityType)rd.idx();
        k2[1] = (SmContinuityType)rd.idx();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        double tol = rd.num();
        SmSolutionArray sols;
        SmStatus stt = sc_srf_p2::sm_CCLocalSolve(*c1, i1, k1, *c2, i2, k2, sop, tol, sols);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_solutions(o, sols);
        delete c1;
        delete c2;
        break;
    }
    case OP_ISECT_SPLIT:
    {
        SmBSplineCurve* c = read_curve(rd, ctx);
        SmExtent1d ivl = rd.ivl();
        SmVector3d pp = rd.vec();
        SmVector3d pn = rd.vec();
        SmTArray<double> sp;
        int64_t n = rd.idx();
        for (int64_t i = 0; i < n; i++)
            sp.Add(rd.num());
        SmStatus stt = sc_srf_p2::sm_IntersectAndSplit(*c, ivl, pp, pn, sp);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_doubles(o, sp);
        delete c;
        break;
    }
    case OP_ISECT_CURVE_SPLIT:
    {
        SmBSplineCurve* c = read_curve(rd, ctx);
        SmExtent1d ivl = rd.ivl();
        SmVector3d lp = rd.vec();
        SmVector3d lv = rd.vec();
        SmExtent1d livl = rd.ivl();
        double sz = rd.num();
        SmTArray<double> sp;
        int64_t n = rd.idx();
        for (int64_t i = 0; i < n; i++)
            sp.Add(rd.num());
        SmLine* line = new (ctx) SmLine(lp, lv, livl, 1.0, 2, &ctx, NULL, FALSE);
        SmStatus stt = sc_srf_p2::sm_IntersectCurveAndSplit(*c, ivl, *line, livl, sz, sp);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_doubles(o, sp);
        delete line;
        delete c;
        break;
    }
    case OP_LSI:
    {
        SmExtent3d box = rd.box();
        SmVector3d lp = rd.vec();
        SmVector3d lv = rd.vec();
        bool has_ivl = rd.flag();
        SmExtent1d ivl = rd.ivl();
        bool ray = rd.flag();
        double tol = rd.num();
        SmTree tree((SmObject*)NULL);
        SmTreeNode node;
        node.m_sBBox = box;
        node.m_pTree = &tree;
        SmExtent2d dom(0.0, 0.0, 1.0, 1.0);
        sc_srf_p2::SmLSIGlobalSolver gs(&tree, dom, lp, lv, has_ivl ? &ivl : NULL,
                                        ray ? TRUE : FALSE, tol);
        SmTreeNode* branch[SM_GS_MAX_TREES] = {&node};
        SmBoolean r = gs.BranchMayContainAnswers(branch);
        o.push_back(0.0);
        o.push_back(flg(r == TRUE));
        break;
    }
    case OP_CS_PERPEN:
    {
        SmAuxDataType ca = (SmAuxDataType)rd.idx();
        SmExtent3d cb = rd.box();
        SmAuxDataType sa = (SmAuxDataType)rd.idx();
        SmExtent3d sb = rd.box();
        SmTreeNode cn, sn;
        cn.m_eAuxDataType = ca;
        cn.m_sBBox = cb;
        sn.m_eAuxDataType = sa;
        sn.m_sBBox = sb;
        SmBoolean r = sc_srf_p2::sm_CsArePerpenDisjoint(cn, sn);
        o.push_back(0.0);
        o.push_back(flg(r == TRUE));
        break;
    }
    case OP_PLANE_3D:
    {
        int64_t k = rd.idx();
        SmTArray<SmCurve*> crvs;
        for (int64_t i = 0; i < k; i++)
            crvs.Add(read_curve(rd, ctx));
        double tol = rd.num();
        SmSurface* plane = NULL;
        SmTArray<ULONG> counts;
        SmTArray<SmCurve*> ordered;
        SmTArray<SmOrientType> orients;
        SmTArray<ULONG> outer;
        SmStatus stt = SmSurface::CreatePlaneWith3DCurves(ctx, crvs, tol, plane, counts, ordered,
                                                          orients, &outer);
        o.push_back(st(stt));
        delete plane;
        for (ULONG i = 0; i < crvs.GetSize(); i++)
            delete crvs[i];
        break;
    }
    case OP_READ_DB:
    {
        SM_TYPE type = (SM_TYPE)rd.idx();
        int32_t file_type = (int32_t)rd.idx();
        ULONG version = (ULONG)rd.idx();
        int64_t nb = rd.idx();
        std::string bytes;
        for (int64_t i = 0; i < nb; i++)
            bytes.push_back((char)(unsigned char)rd.idx());
        std::istringstream iss(bytes);
        SmDatabaseIOStream db(file_type == 0 ? SM_ASCII : SM_BINARY, &iss);
        SmSurface* s = NULL;
        SmStatus stt = SmSurface::ReadFromDB(type, db, ctx, s, version);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS && s)
            put_state(o, s);
        delete s;
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

// The planar generator flag line of an ASCII SmSurfOfRevolution record (its value first).
const char* const PLANAR_LINE = "1 SmSurfOfRevolution Planar Generator Curve";

// A revolution record block `kind O X Y a b c swap inside non_planar`: SmTorus::CreateCanonical
// (kind 0: major a, minor b) or SmCone::CreateCanonical (kind 1: bot a, top b, height c),
// WriteToDB to an ASCII stream, the planar generator flag cleared in the record when
// non_planar is set (the way SmTorus/SmCone::ReadFromDB get a non-planar generator), then
// SmSurface::ReadFromDB. On failure writes the status and returns NULL.
static SmSurface* read_revolution(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    int64_t kind = rd.idx();
    SmPoint3d org = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    double a = rd.num();
    double b = rd.num();
    double c = rd.num();
    SmBoolean swap = rd.flag() ? TRUE : FALSE;
    SmBoolean inside = rd.flag() ? TRUE : FALSE;
    bool non_planar = rd.flag();
    SmAxis2Placement a2p(org, x, y);
    SmSurface* made = NULL;
    SM_TYPE type = SmTorus_TYPE;
    SmStatus stt = SM_SUCCESS;
    if (kind == 0)
    {
        SmTorus* t = NULL;
        stt = SmTorus::CreateCanonical(ctx, a2p, a, b, t, swap, inside);
        made = t;
    }
    else
    {
        SmCone* cn = NULL;
        stt = SmCone::CreateCanonical(ctx, a2p, a, b, c, cn, swap, inside);
        made = cn;
        type = SmCone_TYPE;
    }
    if (stt != SM_SUCCESS || made == NULL)
    {
        o.push_back(st(stt == SM_SUCCESS ? SM_ERR : stt));
        delete made;
        return NULL;
    }
    // the format SmDatabaseIOFile::OpenFileForWrite sets for an ASCII file (a stream the
    // caller passes in keeps the stream's own 6-digit default)
    std::ostringstream oss;
    oss.setf(std::ios::fixed);
    oss.precision(16);
    SmDatabaseIOStream writer(SM_ASCII, &oss);
    stt = made->WriteToDB(writer, 30);
    delete made;
    if (stt != SM_SUCCESS)
    {
        o.push_back(st(stt));
        return NULL;
    }
    std::string text = oss.str();
    if (non_planar)
    {
        size_t at = text.find(PLANAR_LINE);
        if (at == std::string::npos)
        {
            o.push_back((double)SC_ERR_ARG);
            return NULL;
        }
        text[at] = '0';
    }
    std::istringstream iss(text);
    SmDatabaseIOStream reader(SM_ASCII, &iss);
    SmSurface* s = NULL;
    stt = SmSurface::ReadFromDB(type, reader, ctx, s, 30);
    if (stt != SM_SUCCESS || s == NULL)
    {
        o.push_back(st(stt == SM_SUCCESS ? SM_ERR : stt));
        delete s;
        return NULL;
    }
    return s;
}

SC_EXPORT int32_t sc_surface_surface_p2_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                             int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    int32_t ctor = (int32_t)rd.idx();
    if (ctor == CT_NONE)
    {
        static_op(op, rd, ctx, o);
    }
    else
    {
        SmSurface* s = NULL;
        if (ctor == CT_BASE)
            s = new (ctx) SmSurface();
        else if (ctor == CT_BSPLINE)
            s = read_bspline(rd, ctx);
        else if (ctor == CT_REVOLUTION)
        {
            s = read_revolution(rd, ctx, o);
            if (s == NULL)
                return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
        }
        else
            return SC_ERR_ARG;
        surface_op(op, rd, ctx, s, o);
        delete s;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
