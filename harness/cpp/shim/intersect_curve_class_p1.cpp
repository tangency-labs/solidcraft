// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `intersect.curve_class_p1`: SmCurveInterval, the part-1 members of
// SmCurveClassification, SmTouchData::GetConnectedTouches / GetWindingCount, the
// SmCurveClassification / SmCurveInterval overloads of SmTol, and the static helpers of
// source/SMLib/src/SmCurveClass.cpp (sm_SetXSectCurveThroughPoint, ...).
//
// Static functions of SmCurveClass.cpp cannot be linked against, so they are VERBATIM copies of
// upstream SmCurveClass.cpp (commit c9c5979) inside the namespace sc_cc_p1 (line ranges noted at
// each block; generated, do not edit by hand). sm_GetXSectTol2dOr3d_ForClassifyPair is global
// and called in the library. Private members (SmCurveInterval::SetIntervalMin / SetIntervalMax,
// SmCurveClassification::m_bIsCurveOwned) are reached through explicit instantiation (Rob).
//
// One entry point, sc_intersect_curve_class_p1_run(in, n_in, out, cap, out_len): a script over a
// pool of objects and two default-constructed classifications cc[0], cc[1]. The layout is
// documented in crates/solidcore-ffi/src/units/intersect_curve_class_p1.rs, which validates the
// static shape; the run-time checks below reject (SC_ERR_ARG) the ops that are undefined
// behaviour upstream (indices past the intervals, NULL curves or surfaces that are dereferenced,
// objects of the wrong kind, topology walks over the bare stand-in edges). Objects live for the
// whole process (never deleted: topology destructors notify owners).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmCacheMgr.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmCrvOnSurf.h>
#include <SmCurve.h>
#include <SmCurveCache.h>
#include <SmCurveClass.h>
#include <SmEdge.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmFace.h>
#include <SmGeomUtility.h>
#include <SmGraphicsExtern.h>
#include <SmGraphicsOutput.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmLoop.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmPointClass.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmSurfaceIntersector.h>
#include <SmTArray.h>
#include <SmTol.h>
#include <SmTopologyIntersector.h>
#include <SmTrimSrfCache.h>
#include <SmVector3d.h>
#include <SmVertex.h>
#include <nurbs.h>

#include <map>
#include <vector>

#include "sc_sm.h"

using sc::put_vec;

// global in the library (SmCurveClass.cpp), not declared in a header
SmXSectTol3d sm_GetXSectTol2dOr3d_ForClassifyPair(const SmCurve* cpSrcCurve, const SmSurface* cpSurface1,
                                                  double dSrcCurveOrObj1_Param,
                                                  SmZoneTol3d sSrcCurveOrObj1_ZoneTol3d,
                                                  double dObj2_SrcCurveParam, SmZoneTol3d sObj2_ZoneTol3d);

namespace sc_cc_p1
{
// clang-format off
// ---- verbatim: SmCurveClass.cpp lines 512-698: sm_SetXSectCurveThroughPoint
static SmStatus sm_SetXSectCurveThroughPoint
  (SmCurve                    * pSrcCurve3d,        // i/o: SrcCurve, expected to be CurveClassification->m_cpCurve
    SmZoneTol3d                 s3DCurveZoneTol3d,  // in : 3D Curve's ZoneTol3d - use SmCurveClassification::SrcZoneTol3d when 3D Curve is the m_cpCurve
   SmTArray<SmBSplineCurve*>  & rUVCurve,           // i/o: UVTrimCurves sharing pSrcCurve3d parameterization, possibly modified
   SmTArray<const SmSurface*> & rSurface,           // in : Surfaces intersecting 3DCurve corresponding with rUVCurve array,
                                                    //      the intersection of 1st 2 rSurface surfaces define pSrcCurve3d intersection curve,
                                                    //      if rUVCurve.GetSize() == 1, rSurface.GetSize is still expected to equal 2.
   SmTArray<SmVector2d*>      & rUVPoint,           // in : Intersection through UV-Points on corresponding surfaces of rSurface array
   SmBoolean                  & bModifiedCurve,     // out: TRUE=Curves updated, FALSE=No need to modify curves
   double                     & dParam,             // i/o: in  = guessed Curve parameter assigned to this xsect throughPoint
                                                    //      out = actual  Curve parameter assigned to this xsect throughPoint by projecting sssPoint onto pSrcCurve3d
   double                     & dDeviation,         // out: deviation from xsect point to pSrcCurve3d after modifications
   SmBoolean                  & bSet3DPoint,        // out: TRUE = p3DPoint was set, FALSE=wasn't
   SmVector3d                 * p3DPoint)           // out: xsect point 3D value
{
  // init outputs - curve not modified
  bModifiedCurve = FALSE ;
  bSet3DPoint    = FALSE ;

  // check state
  SmBoolean bOKInputs = (   pSrcCurve3d != NULL
                         // && rUVCurve.GetSize() >= 0   - GetSize() returns ULONG which is always >= 0
                         && rUVCurve.GetSize() <= rSurface.GetSize()
                         && rSurface.GetSize() >= 2
                         && rSurface.GetSize() == rUVPoint.GetSize()
                         && p3DPoint           != NULL) ;
  SM_ASSERT(bOKInputs) ;

  // locals
  //double dOrigParam = dParam ;
  SmStatus sRtn     = SM_ERR ;

  // 1st the surface evaluations and setting p3DPoint
  if(rSurface.GetSize() < 2)
    { return(SM_ERR) ; }

  // temporarily allow out of bound evaluations for surface[0], surface[1]
  SmBoolean bTmp0 = FALSE, bTmp1 = FALSE ;
  SmBSplineSurface * pBSplineSurface0 = SM_CAST_PTR(SmBSplineSurface, rSurface[0]) ;
  SmBSplineSurface * pBSplineSurface1 = SM_CAST_PTR(SmBSplineSurface, rSurface[1]) ;
  SmTemporaryChangeValue<SmBoolean> sClean0(pBSplineSurface0 ? pBSplineSurface0->GetOutOfBoundsEnabled() : bTmp0, TRUE) ;
  SmTemporaryChangeValue<SmBoolean> sClean1(pBSplineSurface1 ? pBSplineSurface1->GetOutOfBoundsEnabled() : bTmp1, TRUE) ;

  // evaluate each surface at the given intersection parameter points
  SmVector3d aSurfVal0[2][2], aSurfVal1[2][2] ;
  sRtn = rSurface[0]->Evaluate(*rUVPoint[0], 1, 1, TRUE, TRUE, TRUE, aSurfVal0[0]) ;
         rSurface[1]->Evaluate(*rUVPoint[1], 1, 1, TRUE, TRUE, TRUE, aSurfVal1[0]) ;

  // tolerances
  SM_ASSERT_BREAK_MSG(pSrcCurve3d->GetDim() == 3, _T("Bad CurveClass 3d Assumption switch call to sm_GetXSectTol2dOr3d_ForClassifyPair()")) ;
  SmZoneTol3d  sXSectObjZoneTol3d = SmTol::GetZoneTol3d_ForXSectResult(rSurface[0], rSurface[1]) ;
  SmXSectTol3d sXSectTol3d   = SmTol::GetXSectTol3d(s3DCurveZoneTol3d,    // was 1.0E-5 ;
                                                    sXSectObjZoneTol3d) ;
  SmXSectTol3d sXSectTol3dSq = sXSectTol3d * sXSectTol3d ; // was 1.0E-10 ;
  double       dAngBoundRad  = 6 * SmTol::GetAngTolRad() ; // match previous behavior SM_DEG2RAD(3.0) ; now 0.5
  SmScaledZero sScaledZero   = SmTol::GetEffZero() ;

  // let surf/surf/surf xsect point = pSurface[0] evaluation
  if(sRtn == SM_SUCCESS)
    { bSet3DPoint = TRUE ;
      *p3DPoint   = aSurfVal0[0][0] ;
    }

  // Abort when inputs are confused (after setting output variables).
  if(bOKInputs == FALSE)
     { return(SM_ERR) ;
     }

  // check state - surface xsect projections are within XSectTol3d(Curve,Surface) of one another
  double dMaxDeviationSq = aSurfVal0[0][0].DistanceBetweenSquared(aSurfVal1[0][0]) ;
  if(dMaxDeviationSq > sXSectTol3dSq)
    {
      SM_DBG_WARN(_T("Tighten Intersection Curve heuristic not applicable - \n  ThroughPoint not added: Given Surface xSectPt pair too far apart to add ThroughPoint to xSect curve to tighten tolerances\n")) ;
      // SM_DBG_WARN(_T("Distance between two surface intersection points check failed - points too far apart\n")) ;
      // return(SM_ERR);
    }

  // project p3DPoint onto pSrcCurve3d to get its param value
  SmBoolean  bCurveProjectAnswer = FALSE ;
  SmSolution sCurveProjection ;
  dParam = pSrcCurve3d->GetNaturalInterval().ClampValue(dParam);
  pSrcCurve3d->LocalPointSolve(pSrcCurve3d->GetNaturalInterval(),
                                   SM_SO_MINIMIZE,
                                   *p3DPoint,
                                   NULL, NULL, NULL,
                                   dParam,                 // in : guess param
                                   bCurveProjectAnswer,    // out: TRUE=found answer
                                   sCurveProjection) ;     // out: SmSolution container

  // when the point fails to project to the curve
  SM_ASSERT(bCurveProjectAnswer == TRUE) ;
  if(bCurveProjectAnswer == FALSE)
    {
      SM_DBG_WARN(_T("SSSPoint projection to XSect Curve failed\n")) ;
      return(SM_ERR) ;
    }

  // grab the projected point's curve param and curve/point deviation values
  dParam     = sCurveProjection.m_vStart[0] ;
  dDeviation = sCurveProjection.m_vStart.m_dSolutionValue ;

  // Note: sXSectTol3d = XSectTol3d(CurveSrcZoneTol3d,Surface)
  if(dDeviation < sXSectTol3d)
    {
      return(SM_SUCCESS) ;
    }

  // evaluate the curve at the param value for pos and tangent information
  SmVector3d sSrcCrvPt3d[2] ;
  pSrcCurve3d->Evaluate(dParam, 1, TRUE, sSrcCrvPt3d) ;

  // skip curves close enough to xsectPoint
  SmVector3d sGap = sSrcCrvPt3d[0] - *p3DPoint ;

 #if SM_DEBUG_CODE
  // set through point tangent = cross(surf1Normal,surf2Normal) ;
  SM_ASSERT(aSurfVal0[1][0].LengthSquared() > SmTol::GetEffZeroSq());
  SM_ASSERT(aSurfVal0[0][1].LengthSquared() > SmTol::GetEffZeroSq());
  SM_ASSERT(aSurfVal1[1][0].LengthSquared() > SmTol::GetEffZeroSq());
  SM_ASSERT(aSurfVal1[0][1].LengthSquared() > SmTol::GetEffZeroSq());
#endif // SM_DEBUG_CODE

  aSurfVal0[1][0].Unitize() ;
  aSurfVal0[0][1].Unitize() ;
  aSurfVal1[1][0].Unitize() ;
  aSurfVal1[0][1].Unitize() ;
  SmVector3d sNorm1          = aSurfVal0[1][0]*aSurfVal0[0][1] ;
  SmVector3d sNorm2          = aSurfVal1[1][0]*aSurfVal1[0][1] ;
  SmVector3d sXSectTangent3d = sNorm1 * sNorm2 ;

  // skip curve modification when the throughPoint is close
  // enough to a tangent surf/surf intersection point that
  // the tangent vector can't easily be computed.  Experience
  // has shown inserting throughpoints in these cases
  // easily corrupt intersection-curve shapes by
  // adding doglegs and hooks to the geometry shape.

  //  sXSectTangent3d == 0 for truly tangent surfaces
  //                     0 for points at poles
  //                     0 points at poorly constructed surface (1st Dir U and V vecs are parallel)
  if(sXSectTangent3d.LengthSquared() <= 100 * sScaledZero)  // this could be broken down in to an angle test with AngTol and a SpeedTest with ScaledZero tol
    { return(SM_SUCCESS) ; }                                // but I can't predict how that will change things - this is good enough for now
                                                            // and to change would take a study of using this over a large set of examples - (Examples not available)

  // ensure Tangent direction is same as curve's direction
  if(sXSectTangent3d.Dot(sSrcCrvPt3d[1]) < 0.0)
    {
      sXSectTangent3d = -sXSectTangent3d ;
    }

  // Surf/Surf XSectLine Tang direction is not nearly parallel to pSrcCurve3d's tangent
  double dTangAngle = 0.0 ;
  sXSectTangent3d.AngleBetween(sSrcCrvPt3d[1], dTangAngle) ;
  if(dTangAngle  > dAngBoundRad)   // works well at 3 degrees - could try bigger limits to see how it goes
    { return(SM_SUCCESS) ; }

  // unitize the tangent vector
  sXSectTangent3d.Unitize() ;

  // insert throughPoint=p3DPoint (tangent=sXSectTangent3d) at dParam into pSrcCurve3d
  if(pSrcCurve3d->IsKindOf(SmBSplineCurve_TYPE))
    {
      // force 3DCurve to interpolate v3DPoint point with given tangent
      sRtn = ((SmBSplineCurve *)pSrcCurve3d)->SetIntersectionCurveThroughPoint
                (dParam,            // i/o: suggested param value updated to actual param value
                 *p3DPoint,         // in : through point position
                 &sXSectTangent3d,  // in : optional tangent value, NULL to ignore
                 rUVCurve,          // in : UVTrimCurves associated with this curve, expected (not required) to equal number of surfaces
                 rSurface,          // in : corresponding surfaces for each pUVCurve
                 rUVPoint,          // in : UVPoint on pSurface corresponding to Through3dPoint,
                                    //      A NULL entry=project crThrough3dPoint onto pSurface
                 bModifiedCurve) ;  // out: TRUE=modified curve, FALSE=didn't
                                    // in : smallest allowed size for new spans.
                                    //      A curve span is either split and updated or
                                    //      just updated.  Splitting is skipped when the split
                                    //      point would create a span less than dMinimunSpanPercent
                                    //      of the current span size.
      // When Successful - reset dDeviation
      if(sRtn == SM_SUCCESS) { dDeviation     = 0.0 ; }
      else                   { bModifiedCurve = FALSE; }

    } // end need to insert throughPoint check

  // all done - succesfully
  return(SM_SUCCESS) ;

} // end sm_SetXSectCurveThroughPoint

// ---- verbatim: SmCurveClass.cpp lines 726-960: sm_SetXSectEdgeThroughPoint
static SmStatus sm_SetXSectEdgeThroughPoint
  (SmEdge                     * pEdge,          // i/o: 3D Edge to modify
   SmPoint3d                  * pThroughPoint,  // in : point to interpolate
   SmTArray<const SmSurface*> & rSurface,       // in : two surfaces whose xsect define shape of pEdge
   SmTArray<SmVector2d*>      & rUVPoint,       // in : 3DPoint associated UVPoints for surfaces in rSurface
   double                     & dParam,         // i/o: in  = guessed pEdge Curve parameter assigned to this xsect throughPoint
                                                //      out = pEdge Curve parameter assigned to this xsect throughPoint by projecting sssPoint onto pSrcCurve3d
   double                     & dDeviation,     // out: pThroughPoint to edge distance
   SmBoolean                  & bModifiedCurve) // out: TRUE=Curves updated and outputs are set
{
  // init outputs - curve not modified
  bModifiedCurve = FALSE ;

  // locals
  SmTArray<SmFace*> aFace ;
  SmCurve         * pSrcCurve3d       = pEdge->GetCurve() ;
  SmZoneTol3d       s3DCurveZoneTol3d = SmTol::GetZoneTol3d(pEdge) ;
  pEdge->GetFaces(aFace) ;
  SmSolution        sCurveProjection, sSurfaceProjection ;
  SmPoint3d         sTempPoint ;
  SmVector3d        sSrcCrvPt3d[2] ;
  SmBoolean         bCurveProjectAnswer      = FALSE ;
  SmExtent1d        sEdgeInterval            = pEdge->GetInterval() ;
  SmExtent1d        sCurveInterval           = pSrcCurve3d->GetNaturalInterval() ;
  SmBoolean         bCommonCurveEdgeInterval =  (   sEdgeInterval.GetMin() == sCurveInterval.GetMin()
                                                 && sEdgeInterval.GetMax() == sCurveInterval.GetMax()) ? TRUE : FALSE ;

  // check state - given or found valid pointers to edge, curve, points, and surfaces
  if(   pEdge              == NULL
     || pThroughPoint      == NULL
     || pSrcCurve3d    == NULL
     || rSurface.GetSize() < 2
     || rUVPoint.GetSize() != rSurface.GetSize())
    { return(SM_ERR) ; }

  // project pThroughPoint onto pCurve
  pSrcCurve3d->LocalPointSolve(pEdge->GetInterval(),
                               SM_SO_MINIMIZE,
                               *pThroughPoint,
                               NULL, NULL, NULL,
                               dParam,                 // in : guess param
                               bCurveProjectAnswer,    // out: TRUE=found answer
                               sCurveProjection) ;     // out: SmSolution container

  // error - when no projected point is found
  if(bCurveProjectAnswer == FALSE)
    { return(SM_ERR) ; }

  // grab the projected point's param and deviaton values
  dParam     = sCurveProjection.m_vStart[0] ;
  dDeviation = sCurveProjection.m_vStart.m_dSolutionValue ;

  // evaluate the curve at the param value for pos and tangent information
  pSrcCurve3d->Evaluate(dParam, 1, TRUE, sSrcCrvPt3d) ;

  // compute a tangent value for the throughPoint

  // temporarily allow out of bound evaluations for surface[0], surface[1]
  SmBoolean bTmp0 = FALSE, bTmp1 = FALSE ;
  SmBSplineSurface * pBSplineSurface0 = SM_CAST_PTR(SmBSplineSurface, rSurface[0]) ;
  SmBSplineSurface * pBSplineSurface1 = SM_CAST_PTR(SmBSplineSurface, rSurface[1]) ;
  SmTemporaryChangeValue<SmBoolean> sClean0(pBSplineSurface0 ? pBSplineSurface0->GetOutOfBoundsEnabled() : bTmp0, TRUE) ;
  SmTemporaryChangeValue<SmBoolean> sClean1(pBSplineSurface1 ? pBSplineSurface1->GetOutOfBoundsEnabled() : bTmp1, TRUE) ;

  SmVector3d aSurfVal0[2][2], aSurfVal1[2][2] ;
  rSurface[0]->Evaluate(*rUVPoint[0], 1, 1, TRUE, TRUE, TRUE, aSurfVal0[0]) ;
  rSurface[1]->Evaluate(*rUVPoint[1], 1, 1, TRUE, TRUE, TRUE, aSurfVal1[0]) ;

  // tolerances                s3DCurveZoneTol3d
  SM_ASSERT_BREAK_MSG(pSrcCurve3d->GetDim() == 3, _T("Bad CurveClass 3d Assumption switch call to sm_GetXSectTol2dOr3d_ForClassifyPair()")) ;
  SmXSectTol3d sXSectTol3d   = SmTol::GetXSectTol3d(s3DCurveZoneTol3d,    // was 1.0E-5 ;
                                                   smos_Max(SmTol::GetZoneTol3d(rSurface[0]),
                                                            SmTol::GetZoneTol3d(rSurface[1]))) ;
  SmXSectTol3d sXSectTol3dSq = sXSectTol3d * sXSectTol3d ; // was 1.0E-10 ;
  SmScaledZero sScaledZero   = SmTol::GetEffZero() ;
  double       dAngBoundRad  = 6 * SmTol::GetAngTolRad() ;     // was SM_DEG2RAD(3.0) ; now 0.5

  // check state - surface xsect projections are near one another
  if(   pThroughPoint->DistanceBetweenSquared(aSurfVal0[0][0])  > sXSectTol3dSq
     || pThroughPoint->DistanceBetweenSquared(aSurfVal1[0][0])  > sXSectTol3dSq
     || aSurfVal0[0][0].DistanceBetweenSquared(aSurfVal1[0][0]) > sXSectTol3dSq)
    { return(SM_ERR) ; }

  // set through point tangent = cross(surf1Normal,surf2Normal) ;
  SM_ASSERT(aSurfVal0[1][0].LengthSquared() > sScaledZero) ;
  SM_ASSERT(aSurfVal0[0][1].LengthSquared() > sScaledZero) ;
  SM_ASSERT(aSurfVal1[1][0].LengthSquared() > sScaledZero) ;
  SM_ASSERT(aSurfVal1[0][1].LengthSquared() > sScaledZero) ;

  aSurfVal0[1][0].Unitize() ;
  aSurfVal0[0][1].Unitize() ;
  aSurfVal1[1][0].Unitize() ;
  aSurfVal1[0][1].Unitize() ;
  SmVector3d sNorm1          = aSurfVal0[1][0]*aSurfVal0[0][1] ;
  SmVector3d sNorm2          = aSurfVal1[1][0]*aSurfVal1[0][1] ;
  SmVector3d sXSectTangent3d = sNorm1 * sNorm2 ;

  if(sXSectTangent3d.LengthSquared() <= sScaledZero)
    {
      // the surfaces are tangent at this point
      // Let tangent = CurrentCurveTangent
      // gwc:  We really should optimize here to find the tangent value
      sXSectTangent3d = sSrcCrvPt3d[1] ;
    }

  // ensure Tangent direction is same as curve's direction
  if(sXSectTangent3d.Dot(sSrcCrvPt3d[1]) < 0.0)
    {
      sXSectTangent3d = -sXSectTangent3d ;
    }

  // skip curve modification when the throughPoint is close
  // enough to a tangent surf/surf intersection point that
  // the tangent vector can't easily be computed.  Experience
  // has shown inserting throughpoints in these cases
  // easily corrupt intersection-curve shapes by
  // adding doglegs and hooks to the geometry shape.

  //  sXSectTangent3d == 0 for truly tangent surfaces, or
  //  sXSectTangent3d not nearly parallel to sSrcCrvPt3d[1] for nearly
  //    tangent surfaces.
  double dTangAngle = 0.0 ;
  sXSectTangent3d.AngleBetween(sSrcCrvPt3d[1], dTangAngle) ;
  if(   sXSectTangent3d.LengthSquared() <= sScaledZero
     || dTangAngle > dAngBoundRad) // works well at 3 degrees - could try bigger limits to see how it goes
    {
      return(SM_SUCCESS) ;
    }

  // unitize the tangent vector
  sXSectTangent3d.Unitize() ;

  // force the 3DCurve to interpolate the point
  // insert throughPoint=pThroughPoint  (tangent=sXSectTangent3d) at dParam into pSrcCurve3d
  if(   pSrcCurve3d->IsKindOf(SmBSplineCurve_TYPE)
     && sCurveProjection.m_vStart.m_dSolutionValue > s3DCurveZoneTol3d/2000)  // usually about 1e-8 = ApproxTol/1000 (2 gets ApproxTol)
    {
      SmTArray<SmBSplineCurve*>  aUVCurve ;
      SmTArray<const SmSurface*> aSurface ;
      SmTArray<SmVector2d*>      aOptUVPoint ;

      // for every face
      for(ULONG ii=0;ii<aFace.GetSize();ii++)
        {
          // get edgeuse connecting pEdge to this face
          SmEdgeuse *pEdgeuse = pEdge->GetEdgeuseOfFace(aFace[ii]) ;

          // Build the SetIntersectionCurveThroughPoint UVTrimCurve arrays
          if( pEdgeuse ) aUVCurve.Add(pEdgeuse->GetUVTrimCurve()) ;
          aSurface.Add(aFace[ii]->GetSurface()) ;
          aOptUVPoint.Add(NULL) ;

        } // end iter every face

      // GWC:EXPERIMENT - Leave the end-points of existing edge-curves alone.
      //  They may be attached to vertices and part of CEdges.
      //  Just add-through points.
      //  Leave the associated UVTrimCurves alone. The parameterization
      //  of an Edge->Curve that has been trimmed by a previous Curve Merge will
      //  be different than its Edge->UVTrimcurves.
      SM_TOL_LINE double dMinimumSpanPercent = 1.0E-6 ; // stop using 'move existing through point cases'
      aUVCurve.ReSet() ;                    // stop editing UVTrimCurves
      aSurface.ReSet() ;
      aOptUVPoint.ReSet() ;

      // force 3DCurve to interpolate pThroughPoint with given tangent
      // Note: Curve's natural domain may be modified
      SmStatus eStat = ((SmBSplineCurve *)pSrcCurve3d)->SetIntersectionCurveThroughPoint
          (dParam,                // i/o: suggested param value updated to actual param value
           *pThroughPoint,        // in : through point position
           &sXSectTangent3d,           // in : optional tangent value, NULL to ignore
           aUVCurve,              // in : UVTrimCurves associated with this curve, expected (not required) to equal number of surfaces
           aSurface,              // in : corresponding surfaces for each pUVCurve
           aOptUVPoint,           // in : UVPoint on pSurface corresponding to Through3dPoint,
                                  //      A NULL entry=project crThrough3dPoint onto pSurface
           bModifiedCurve,        // out: TRUE=modified curve, FALSE=didn't
           dMinimumSpanPercent) ; // in : smallest allowed size for new spans.
                                  //      A curve span is either split and updated or
                                  //      just updated.  Splitting is skipped when the split
                                  //      point would create a span less than dMinimunSpanPercent
                                  //      of the current span size.
      // If the curve was not changed, then we're done.
      if ( eStat != SM_SUCCESS || ! bModifiedCurve )
        { return SM_SUCCESS; }

      // restore common curve/edge interval if its stopped being common
      // by scaling the curve's parameterization
      sEdgeInterval  = pEdge->GetInterval() ;
      sCurveInterval = pSrcCurve3d->GetNaturalInterval() ;
      double dEMin   = sEdgeInterval.GetMin() ;
      double dEMax   = sEdgeInterval.GetMax() ;
      double dCMin   = sCurveInterval.GetMin() ;
      double dCMax   = sCurveInterval.GetMax() ;
      if(   bCommonCurveEdgeInterval
         && (   fabs(dEMin - dCMin) > sScaledZero
             || fabs(dEMax - dCMax) > sScaledZero))
        {
          // we should be fixing the case where one end of the curve's
          // natural interval was modified.
          SM_ASSERT(   (fabs(dEMin - dCMin) <= sScaledZero && dEMax == dParam)
                    || (fabs(dEMax - dCMax) <= sScaledZero && dEMin == dParam)) ;

          // change the parameter values of every pSrcCurve3d knot
          ((SmBSplineCurve *)pSrcCurve3d)->ScaleKnotVector(dEMin, dEMax) ;

          // change the parameter values of every UVTrimCurve knot
          for(ULONG ii=0;ii<aUVCurve.GetSize();ii++)
            {
              ((SmBSplineCurve *)aUVCurve[ii])->ScaleKnotVector(dEMin, dEMax) ;

            } // end iter every UVTrimCurve

        } // end need to restore common curve/edge interval check

      // reset the deviation value
      dDeviation = 0.0 ;

      // GWC:CHANGE_SSSUVTRIMCURVE
      // remove obsolete UVTrimCurves from edge->Edgeuses
      SmTArray<SmEdgeuse*> aEdgeuses ;
      pEdge->GetEdgeuses(aEdgeuses) ;
      for(ULONG ii=0;ii<aEdgeuses.GetSize();ii++)
        {
          aEdgeuses[ii]->SetUVTrimCurve(NULL, 0.0, TRUE) ; // TRUE = delete existing UVTrimCurve
        }

      // inform the public that the curves were updated
      bModifiedCurve = TRUE ;

    } // end need to adjust throughPoint Check

  // all done - successfully
  return(SM_SUCCESS) ;

} // end sm_SetXSectEdgeThroughPoint

// ---- verbatim: SmCurveClass.cpp lines 968-1080: sm_GetCCISurfaceGaps
static SmStatus sm_GetCCISurfaceGaps
  (SmPoint3d                    & rClassCurvePoint3d,   // in : Current ClassificationCurve Solution Point3d
   SmPoint3d                    & rEdgeCurvePoint,      // in : Current Edge Solution Point
   SmSolution                   & rSolution,            // in : ClassificationCurve/EdgeCurve intersection solution
   SmTArray <const SmSurface *> & rSurfaces,            // in : List of all Surfaces connected to this intersection point
                                                        //      ordered:[CurveSurfaces, EdgeSurfaces, commonEdgeCurveSurface, EdgeBooleanMateSurfaces]
   SmTArray <const SmCurve *  > & rUVCurves,            // in : edgeuse UVTrimCurve (may be NULL)
   ULONG                          lEdgeSurfaceCount,    // NotUsed: in : ignored - number of EdgeSurfaces in rSurfaces (number of Curve Surfaces == 2, the rest are pEMate->Surfaces)
   double                       & dCurveEdgeGap,        // out: CurvePoint/EdgePoint gap
   double                       & dCurveSurfaceGap,     // out: CurvePoint/SurfacesConnectedToCurve max gap
   double                       & dEdgeSurfaceGap,      // out: EdgePoint/SurfacesConnectedToEdge max gap
   SmTArray <SmPoint2d>         & rUVPoints)            // out: Surface UVPoint for each GapVector
                                                        //      plus one for the EdgeCurvePoint/CommonSurf gap
{
  SM_REF1(lEdgeSurfaceCount) ;
  // init output
  dCurveSurfaceGap = 0.0 ;
  dEdgeSurfaceGap  = 0.0 ;
  rUVPoints.ReSet() ;

  // Get CurvePoint/EdgePoint gap size
  dCurveEdgeGap = rClassCurvePoint3d.DistanceBetween(rEdgeCurvePoint) ;

  // for every surface
  SmSolutionArray sSolutions;
  double dMaxDropDist = 1.0 ;
  ULONG ii ;
  for(ii=0;ii<rSurfaces.GetSize();ii++)
    {
      // get target Point/Surface combination
      const SmSurface *pSurface = rSurfaces[ii] ;
      const SmCurve   *pUVCurve = rUVCurves[ii] ;
      SmPoint3d       *pPoint   =   ii < 2
                                  ? &rClassCurvePoint3d
                                  : &rEdgeCurvePoint ;

      // When posible use cheap localSolve else use expensive Global Solve
      SmSolution  sSol ;
      SmSolution *pSol = NULL ;
      if(pUVCurve)
        {
          // get UVGuessPoint
          SmPoint3d sGuessPoint ;
          double dTParam =   ii < 2
                           ? rSolution.m_vStart.m_adParameters[0]
                           : rSolution.m_vStart.m_adParameters[1] ;

          pUVCurve->EvaluatePoint(dTParam, sGuessPoint) ;
          SmPoint2d sUVPoint(sGuessPoint.x, sGuessPoint.y) ;

          // get classCurvePoint/Surf dist from cheap local solve
          SmBoolean  bFoundAnswer ;
          pSurface->LocalPointSolve(pSurface->GetNaturalUVDomain(),
                                    SM_SO_MINIMIZE,
                                    *pPoint,
                                    sUVPoint,
                                    bFoundAnswer,
                                    sSol) ;
          if(bFoundAnswer) { pSol = &sSol ; }
          else             { pSol = NULL ;
                             SM_DBG_WARN(_T("XSectCurvePoint failed LocalDrop to Surface")) ;
                           }

        } // end LocalSolve from UVTrimCurve Point branch
      else // need a GlobalSolve branch
        {
          // get classCurvePoint/surf dist without checking trim boundaries
          sSolutions.ReSet() ;
          SmSurfaceCache *pSC = smsurf_GetSurfaceCache(pSurface);

          // Turn off point testing so GlobalPointSolve() will keep all point solutions
          //   without classifying the solution point against the trim boundaries.
          SmTemporaryChangeValue<SmBoolean> sChange(pSC->m_bPointTestEnabled,FALSE);
          // Get a tolerance to use.
          SmVector2d sSize;
          pSurface->ApproximateSize( sSize );
          double dTol = 0.001 * sSize.Length();
          pSurface->GlobalPointSolve(pSurface->GetNaturalUVDomain(),
                                     SM_SO_MINIMIZE,
                                     *pPoint,
                                     dTol,  // not: dMaxDropDist,
                                     &dMaxDropDist,
                                     SM_SR_SINGLE,
                                     sSolutions) ;
          if(sSolutions.GetSize() > 0)
               { pSol = &(sSolutions[0]) ; }
          else { return(SM_ERR) ;
               }
        } // end GlobalSolve ClassCurvePoint/Surf distance branch

      // accumulate UVPoints and distances
      if(pSol)
        {
          double dGap = pSol->m_vStart.m_dSolutionValue ;
          SmPoint2d sUVPoint(pSol->m_vStart.m_adParameters[0],
                             pSol->m_vStart.m_adParameters[1]) ;
          rUVPoints.Add(sUVPoint) ;

          // save max CurvePoint/Surface and EdgePoint/Surface gaps
          if(ii < 2) { if(dGap > dCurveSurfaceGap) dCurveSurfaceGap = dGap ; }
          else       { if(dGap > dEdgeSurfaceGap ) dEdgeSurfaceGap  = dGap ; }
        }
      else
        {
          // Point failed to drop to connected surface
          return(SM_ERR) ;
        }
    } // end iter every surface looking for ClassCurvePoint/Surf distance

  // all done
  return(SM_SUCCESS) ;

} // end sm_GetCCISurfaceGaps

// ---- verbatim: SmCurveClass.cpp lines 1106-1235: sm_GetCCIGeometry
static SmStatus sm_GetCCIGeometry
  (SmCurveClassification        * pCurveClassification, // in : classification containing curve to check
   SmSolution                   & rSolution,            // NotUsed: in : ClassificationCurve/EdgeCurve intersection solution
   SmEdge                       * pEdge,                // in : Edge found to intersect the curve being classified
   SmEdge                      *& pEMate,               // out: edge->BooleanMateEdge
   SmTArray <const SmSurface *> & rSurfaces,            // out: List of all Surfaces connected to this intersection point
                                                        //      ordered:[CurveSurfaces(2), EdgeSurfaces, CommonEdgeCurveSurface, EdgeBooleanMateSurfaces]
   SmTArray <SmEdgeuse *>       & rEdgeuses,            // out: associated edgeuse connecting intersectionPoint to Surface (may be NULL)
   SmTArray <const SmCurve *  > & rUVCurves,            // out: edgeuse UVTrimCurve (may be NULL)
   ULONG                        & lEdgeSurfaceCount)    // out: number of EdgeSurfaces in rSurfaces including the duplicate CommonEdgeCurveSurface
{
  SM_REF1(rSolution) ;
  // init output
  pEMate  = NULL ;
  rSurfaces.ReSet() ;
  rEdgeuses.ReSet() ;
  rUVCurves.ReSet() ;
  lEdgeSurfaceCount = 0 ;

  // locals
  ULONG ii ;
  const SmSurface   *pSurface1   = pCurveClassification->GetSurface1() ;
  const SmSurface   *pSurface2   = pCurveClassification->GetSurface2() ;
  ULONG lCommonSurf              = SM_BIG_ULONG ;

  // Part 1 - gather Surfaces, edgeuses, and UVTrimCurves connected to intersection Point

  // gather the ClassCurve Surfaces and UVTrimCurves
  if(pSurface1) { rSurfaces.Add(pSurface1) ;
                  rEdgeuses.Add(NULL) ;
                  rUVCurves.Add(pCurveClassification->GetUVCurve1()) ;
                }
  if(pSurface2) { rSurfaces.Add(pSurface2) ;
                  rEdgeuses.Add(NULL) ;
                  rUVCurves.Add(pCurveClassification->GetUVCurve2()) ;
                }

  // get Faces connected to Edge and Edge->BooleanMate
  SmTArray<SmFace *> sFaces ;
  pEdge->GetFaces(sFaces) ;
  ULONG lEdgeFaceCount = sFaces.GetSize() ;

  // When curve classification is being done as part of a boolean operation
  const SmTopologyIntersector *cpTI = pCurveClassification->GetTopologyIntersector() ;
  if(cpTI)
    {
      // look for edge->BooleanMate
      pEMate =   cpTI->GetOtherMate(pEdge)
               ? (SmEdge *)cpTI->GetOtherMate(pEdge)
               : (SmEdge *)cpTI->GetBrepMate(pEdge) ;

      // when pEdge has a boolean operation temporary mate
      if(pEMate)
        {
          // add in Faces and Edgeuses connected to Edge->BooleanMate
          SmTArray<SmFace*> tFaces ;
          SM_ASSERT(pEMate->IsKindOf(SmEdge_TYPE)) ;
          pEMate->GetFaces(tFaces) ;
          sFaces.Append(tFaces) ;
        }
    } // end possible temporary mating edge check

  // gather unique surfaces and edgeuses connected to this intersection point
  ULONG lEdgeCurveCommonSurf = 0 ;
  for(ii=0;ii<sFaces.GetSize();ii++)
    {
      SmFace    *pFace = sFaces[ii] ;
      SmSurface *pSurf = pFace->GetSurface() ;

      // when pSurf is not in sSurfaces list - add it
      ULONG lIndex ;
      if(FALSE == rSurfaces.FindElement(pSurf, lIndex))
        {
          const SmEdgeuse *pEdgeuse =   ii<lEdgeFaceCount
                                      ? pEdge->GetEdgeuseOfFace(pFace)
                                      : pEMate->GetEdgeuseOfFace(pFace) ;

          if(pEdgeuse != NULL)
          {
            rSurfaces.Add( pSurf );   // Should this be outside of if?
            rEdgeuses.Add( (SmEdgeuse *)pEdgeuse );

            // don't use pEMate UVTrimCurves (they have a different parameterization)
            rUVCurves.Add( (pEdgeuse && ii < lEdgeFaceCount)
                          ? pEdgeuse->GetUVTrimCurve()
                          : NULL );
          }

          if(ii <lEdgeFaceCount) { lEdgeSurfaceCount ++ ; }
        }

      // remember an edge->Surface which is shared with the curve (nonManifold may have 2)
      SM_ASSERT(pSurface1 != NULL) ;
      if     (pSurf == pSurface1) { lEdgeCurveCommonSurf = 1 ;
                                    lCommonSurf = ii ;
                                  }
      else if(pSurf == pSurface2) { lEdgeCurveCommonSurf = 2 ;
                                    lCommonSurf = ii ;
                                  }

      // when we are done with edgeSurfaces
      if( ii == lEdgeFaceCount - 1 && lCommonSurf < lEdgeFaceCount )
        {
          // add the commonSurf a 2nd time
          pFace = sFaces[lCommonSurf] ;
          pSurf = pFace->GetSurface() ;
          const SmEdgeuse *pEdgeuse = pEdge->GetEdgeuseOfFace(pFace) ;

          if(pEdgeuse)
          {
            rSurfaces.Add( pSurf );
            rEdgeuses.Add( (SmEdgeuse *)pEdgeuse );

            // don't use pEMate UVTrimCurves (they have a different parameterization)
            rUVCurves.Add( pEdgeuse->GetUVTrimCurve() );
            lEdgeSurfaceCount++;
          }

        }
    }  // end iter every face attached to this edge

  // at least 1 edge or edge->BooleanMate face's surface is one of the intersection surfaces
  SM_ASSERT(   lEdgeCurveCommonSurf == 1
            || lEdgeCurveCommonSurf == 2) ;
  SM_ASSERT(lCommonSurf != SM_BIG_ULONG) ;

  // all done
  return(SM_SUCCESS) ;

} // end sm_GetCCIGeometry

// ---- verbatim: SmCurveClass.cpp lines 1247-1625: sm_ApplyPointSolution
static SmStatus sm_ApplyPointSolution
  (SmCurveClassification * pCurveClassification, // in : CurveClassification containing ClassCurve, NULL = leave ClassCurve alone
   SmEdge                * pEdge,                // in : Edge,                  NULL = leave EdgeCurve alone
   SmEdge                * pEMate,               // in : Edge->BooleanMateEdge, NULL = leave EdgeMateCurve alone
   const SmSurface       * pClassCurveSurface1,  // in : ClassCurve Surf1
   SmPoint2d             * pClassCurveUVPoint1,  // in : Desired Surf1 Solution point
   const SmSurface       * pClassCurveSurface2,  // in : ClassCurve Surf2
   SmPoint2d             * pClassCurveUVPoint2,  // in : Desired Surf2 Solution point
   const SmSurface       * pEdgeCurveSurface1,   // in : pEdge/pEMate Surf1
   SmPoint2d             * pEdgeCurveUVPoint1,   // in : Desired pEdge/pEMate1 Solution point
   const SmSurface       * pEdgeCurveSurface2,   // in : pEdge/pEMate Surf2
   SmPoint2d             * pEdgeCurveUVPoint2,   // in : Desired pEdge/pEMate2 Solution point
   SmSolution            & rSolution,            // i/o: ClassCurve/Edge Solution being refined.
                                                 //      Sol.m_vStart.Value, Param[0], and Param[1] are changed
   SmPoint3d             & rSSSPoint,            // out: surf/surf/surf xsect point for this solution
   SmBoolean             & bSSSPointFlag,        // out: TRUE=use Surf/surf/surf intersection point
   SmBoolean             & bModifiedCurve,       // out: TRUE=Classification Curve modified, Classification should be redone
   SmBoolean             & bModifiedEdgeCurve,   // out: TRUE=EdgeCurve modified, UVTrimCurves should be cleared
   SmBoolean             & bModifiedMateCurve,   // out: TRUE=MateCurve modified, UVTrimCurves should be cleared
   double                & dDeviation)           // out: deviation of curves from sssPoint
{
  // check input - must have a pEdge if given a pEMate
  SM_ASSERT(pEMate == NULL || pEdge != NULL) ;

  // init output
  bSSSPointFlag      = FALSE ;
  bModifiedCurve     = FALSE ;
  bModifiedEdgeCurve = FALSE ;
  bModifiedMateCurve = FALSE ;
  dDeviation         = 0.0 ;

  // locals
  const SmCurve * pSrcCurve3d   = pCurveClassification ? pCurveClassification->GetCurve() : NULL ;
  const SmCurve * pEdgeCurve    = pEdge  ? pEdge->GetCurve()  : NULL ;
  SmZoneTol3d     sSrcZoneTol3d = SmTol::GetSrcZoneTol3d(pCurveClassification) ;
  SmStatus eStat;

#ifdef SM_DEBUG_CODE
static constexpr ULONG bDebugMe = FALSE ;

  SmExtent2d sInitDomain ;
  SmExtent2d sUVDomain1 = pClassCurveSurface1 ? pClassCurveSurface1->GetNaturalUVDomain() : sInitDomain ;
  SmExtent2d sUVDomain2 = pClassCurveSurface2 ? pClassCurveSurface2->GetNaturalUVDomain() : sInitDomain ;
  SmExtent2d sUVDomain3 = pEdgeCurveSurface1  ? pEdgeCurveSurface1->GetNaturalUVDomain()  : sInitDomain ;

  // output a message if any of the UVPoints are outside their surface boundary
  if(   (pCurveClassification != NULL      && FALSE == sUVDomain1.ContainsPoint2d(*pClassCurveUVPoint1, SM_EFF_ZERO))
     || (pCurveClassification != NULL      && FALSE == sUVDomain2.ContainsPoint2d(*pClassCurveUVPoint2, SM_EFF_ZERO))
     || ((pEdge != NULL || pEMate != NULL) && FALSE == sUVDomain3.ContainsPoint2d(*pEdgeCurveUVPoint1,  SM_EFF_ZERO)))
    {
      SmVector2d sZero(0.0,0.0) ;
      SmVector2d sGap1 = pCurveClassification ? *pClassCurveUVPoint1 - sUVDomain1.ClampPoint2d(*pClassCurveUVPoint1) : sZero;
      SmVector2d sGap2 = pCurveClassification ? *pClassCurveUVPoint2 - sUVDomain2.ClampPoint2d(*pClassCurveUVPoint2) : sZero;
      SmVector2d sGap3 = (pEdge != NULL || pEMate != NULL)  ? *pEdgeCurveUVPoint1  - sUVDomain3.ClampPoint2d(*pEdgeCurveUVPoint1)  : sZero ;

      if(bDebugMe)
        {
          smgfx_Erase() ;
          smgfx_SetLook(1,2, 0,0,1) ; if(pClassCurveSurface1) pClassCurveSurface1->DrawUV(2,2) ; sm_GraphicsLoop() ;
          smgfx_SetLook(1,2, 0,1,0) ; if(pClassCurveSurface2) pClassCurveSurface2->DrawUV(2,2) ; sm_GraphicsLoop() ;
          smgfx_SetLook(1,2, 0,1,1) ; if(pEdgeCurveSurface1 ) pEdgeCurveSurface1->DrawUV(2,2) ; sm_GraphicsLoop() ;
          sm_GraphicsLoop() ;
        } // end bDebug check

      if(sGap1.LengthSquared() > SM_EFF_ZERO)  { SM_DBG_WARN(_T("FYI: SSSPoint found outside surface1 NaturalUVDomain")) ;
                                                 if(!SM_IS_ZERO(sGap1.x)) ((SmBSplineSurface *)pClassCurveSurface1)->ExpandBoundarySpan(pClassCurveUVPoint1->x, SM_SP_U) ;
                                                 if(!SM_IS_ZERO(sGap1.y)) ((SmBSplineSurface *)pClassCurveSurface1)->ExpandBoundarySpan(pClassCurveUVPoint1->y, SM_SP_V) ;
                                               }
      if(sGap2.LengthSquared() > SM_EFF_ZERO)  { SM_DBG_WARN(_T("FYI: SSSPoint found outside surface2 NaturalUVDomain")) ;
                                                 if(!SM_IS_ZERO(sGap2.x)) ((SmBSplineSurface *)pClassCurveSurface2)->ExpandBoundarySpan(pClassCurveUVPoint2->x, SM_SP_U) ;
                                                 if(!SM_IS_ZERO(sGap2.y)) ((SmBSplineSurface *)pClassCurveSurface2)->ExpandBoundarySpan(pClassCurveUVPoint2->y, SM_SP_V) ;
                                               }
      if(sGap3.LengthSquared() > SM_EFF_ZERO)  { SM_DBG_WARN(_T("FYI: SSSPoint found outside surface3 NaturalUVDomain")) ;
                                                 if(!SM_IS_ZERO(sGap3.x)) ((SmBSplineSurface *)pEdgeCurveSurface1)->ExpandBoundarySpan(pEdgeCurveUVPoint1->x, SM_SP_U) ;
                                                 if(!SM_IS_ZERO(sGap3.y)) ((SmBSplineSurface *)pEdgeCurveSurface1)->ExpandBoundarySpan(pEdgeCurveUVPoint1->y, SM_SP_V) ;
                                               }
    } // end SSSPoint outside surface boundary check

  const SmCurve* pEMateCurve = pEMate ? pEMate->GetCurve() : NULL;

  SmFace* pFace1 = pClassCurveSurface1 ? (SmFace*)pClassCurveSurface1->GetFace() : NULL;
  SmFace *pFace2 = pClassCurveSurface2 ? (SmFace *)pClassCurveSurface2->GetFace() : NULL ;
  SmFace *pFace3 = pEdgeCurveSurface1 ? (SmFace *)pEdgeCurveSurface1->GetFace() : NULL ;
  SmBrep *pBrep1 = pFace1 ? pFace1->GetBrep() : NULL ;
  SmBrep *pBrep2 = pFace2 ? pFace2->GetBrep() : NULL ;
  SmBrep *pBrep3 = pFace3 ? pFace3->GetBrep() : NULL ;

  if(bDebugMe)
    {
      SmPoint3d sSurface1Point ;
      SmBoolean bTmp0 = FALSE ;
      SmBSplineSurface * pBSplineSurface0 = SM_CAST_PTR(SmBSplineSurface, pClassCurveSurface1) ;
      SmTemporaryChangeValue<SmBoolean> sClean0(pBSplineSurface0 ? pBSplineSurface0->GetOutOfBoundsEnabled() : bTmp0, TRUE) ;
      pClassCurveSurface1->EvaluatePoint(*pClassCurveUVPoint1,sSurface1Point) ;

      smgfx_Erase() ;
      smgfx_SetLook(1,2, 0,0,1) ; if(pBrep1) pBrep1->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,1,0) ; if(pBrep2 && pBrep2 != pBrep1) pBrep2->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,0,0) ; if(pBrep3 && pBrep3 != pBrep1 && pBrep3 != pBrep2) pBrep3->Draw() ; sm_GraphicsLoop() ;

      smgfx_SetLook(1,2, 0,0,1) ; if(pClassCurveSurface1) pClassCurveSurface1->DrawUV() ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,0,0) ; if(pFace1) { pFace1->Draw(SM_DM_CROSSHATCH) ; } sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,1,0) ; if(pClassCurveSurface2) pClassCurveSurface2->DrawUV() ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,0,0) ; if(pFace2) { pFace2->Draw(SM_DM_CROSSHATCH) ; } sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,1,1) ; if(pEdgeCurveSurface1)  pEdgeCurveSurface1->DrawUV() ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,0,0) ; if(pFace3) { pFace3->Draw(SM_DM_CROSSHATCH) ; } sm_GraphicsLoop() ;

      smgfx_SetLook(5,7, 1,0,1) ; if(pSrcCurve3d) pSrcCurve3d->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,6, 0,1,0) ; if(pEdgeCurve)  pEdgeCurve->Draw() ;  sm_GraphicsLoop() ;
      smgfx_SetLook(3,6, 0,1,0) ; if(pEMateCurve) pEMateCurve->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(7,9, 1,0,0) ; sSurface1Point.Draw() ; sm_GraphicsLoop() ;
      sm_GraphicsLoop() ;
    } // end bDebug check
#endif // SM_DEBUG_CODE

  // add switches to turn curve/edge through point interpolation on and off
  SmBoolean gwcUpdateCurves = TRUE ;   // system found to be more robust when gwcUpdateCurves is run
  SmBoolean gwcUpdateEdges  = TRUE ;   // system found to be more robust when gwcUpdateEdges is run

  // locals
  SmTArray<SmBSplineCurve*>  aUVCurve ;
  SmTArray<const SmSurface*> aSurface ;
  SmTArray<SmVector2d*>      aUVPoint ;
  SmBoolean bCurveFoundAnswer  = FALSE ;
  SmSolution sCurveProjection ;
  double dParam, dTDeviation  = 0.0 ;

  // let rSSSPoint = pClassCurveSurface1 evaluation at the intersection point
  SmStatus sRtn =   (pSrcCurve3d       && pClassCurveSurface1) ? pClassCurveSurface1->EvaluatePoint(*pClassCurveUVPoint1, rSSSPoint)
                  : (pSrcCurve3d       && pClassCurveSurface2) ? pClassCurveSurface2->EvaluatePoint(*pClassCurveUVPoint2, rSSSPoint)
                  : ((pEdge || pEMate) && pEdgeCurveSurface1)  ? pEdgeCurveSurface1->EvaluatePoint (*pEdgeCurveUVPoint1, rSSSPoint)
                  : SM_ERR ;

  if( sRtn == SM_ERR )
      return( SM_ERR );

  // set closest point on curve to surf/surf/surf intersection point
  if(pSrcCurve3d)
    {
      if(gwcUpdateCurves == TRUE)   // system found to be more robust when gwcUpdateCurves TRUE
        {
          // set up sm_SetXSectCurveThroughPoint arguments
          dParam = rSolution.m_vStart[0] ; // init pSrcCurve3d param guess = curve/curve xsect solution
          aUVCurve.ReSet() ;
          aSurface.ReSet() ;
          aUVPoint.ReSet() ;

          // load surfaces and UVPoints marking the intersection
          aSurface.Add(pClassCurveSurface1) ;
          aUVPoint.Add(pClassCurveUVPoint1) ;

          aSurface.Add(pClassCurveSurface2) ;
          aUVPoint.Add(pClassCurveUVPoint2) ;

          // ensure surf/surf/surf solution is through point on classCurve and its UVTrimCurves
          //    side effects: curves with large tolerance are modified to interpolate xsect point
          //                  rSSSPoint is set with 3DPoint value for intersection
          //                  dParam is moved to best through point value
          //                  dTDeviation is set with surf/surf/surf xsect Point to curve distance
          SmBoolean bSetSSSPoint      = FALSE ;
          sm_SetXSectCurveThroughPoint
              ((SmCurve *)pSrcCurve3d,    // i/o: 3D Curve, possibly modified
                          sSrcZoneTol3d,  // in : pSrcCurve3d's ZoneTol3d value
                          aUVCurve,       // i/o: UVTrimCurves sharing pSrcCurve3d parameterization, possibly modified
                          aSurface,       // in : Surfaces intersecting 3DCurve corresponding with rUVCurve array,
                                          //      the intersection of 1st 2 rSurface surfaces define pSrcCurve3d intersection curve,
                                          //      if rUVCurve.GetSize() == 1, rSurface.GetSize is still expected to equal 2.
                          aUVPoint,       // in : Intersection through UV-Points on corresponding surfaces of rSurface array
                          bModifiedCurve, // out: TRUE=Curves updated, FALSE=No need to modify curves
                          dParam,         // i/o: in  = guessed Curve parameter assigned to this xsect throughPoint
                                          //      out = actual  Curve parameter assigned to this xsect throughPoint by projecting sssPoint onto pSrcCurve3d
                          dTDeviation,    // out: deviation from xsect point to pSrcCurve3d after modifications
                          bSetSSSPoint,   // out: TRUE = p3DPoint was set, FALSE=wasn't
                          &rSSSPoint) ;   // out: xsect point 3D value

          // inform the public - remove obsolete UVTrimCurves
          if(bModifiedCurve)
            { bSSSPointFlag = TRUE; }

          // move pSrcCurve3d parameter in rSolution to surf/surf/surf xsect point
          rSolution.m_vStart[0] = dParam ;

          // increase deviation to include distance to this curve from rSSSPoint
          dDeviation = smos_Max(dDeviation, dTDeviation) ;

        } // end gwcUpdateCurves == TRUE branch
      else // don't add through points to xsect curves branch
        {
          // temporarily allow out of bounds evaluations for pClassCurveSurface1
          SmBoolean bTmp0 = FALSE ;
          SmBSplineSurface * pBSplineSurface0 = SM_CAST_PTR(SmBSplineSurface, pClassCurveSurface1) ;
          SmTemporaryChangeValue<SmBoolean> sClean0(pBSplineSurface0 ? pBSplineSurface0->GetOutOfBoundsEnabled() : bTmp0, TRUE) ;

          // let rSSSPoint = pClassCurveSurface1 evaluation at the intersection point
          sRtn = pClassCurveSurface1->EvaluatePoint(*pClassCurveUVPoint1, rSSSPoint) ;

          // inform the public
          if(sRtn == SM_SUCCESS)
            { bSSSPointFlag = TRUE ; }

          // move pSrcCurve3d parameter in rSolution to surf/surf/surf xsect point
          eStat = pSrcCurve3d->LocalPointSolve(pCurveClassification->GetInterval(),
                                               SM_SO_MINIMIZE,
                                               rSSSPoint,
                                               NULL, NULL, NULL,
                                               rSolution.m_vStart[0],
                                               bCurveFoundAnswer,
                                               sCurveProjection) ;

          // when we found a project point - save the parameter and deviation values
          if ( eStat == SM_SUCCESS && bCurveFoundAnswer )
            {
              rSolution.m_vStart[0] = sCurveProjection.m_vStart[0] ;

              // increase deviation to include distance to this curve from rSSSPoint
              dDeviation = smos_Max(dDeviation, sCurveProjection.m_vStart.m_dSolutionValue) ;

            } // end Point projected to curve check
        } // end gwcUpdateCurves == FALSE branch
    } // end pSrcCurve3d existence check

  // set closest point on pEdge to surf/surf/surf intersection point
  if(pEdge)
    {
      if(gwcUpdateEdges == TRUE)
        {
          // set arguments for sm_SetXSectEdgeThroughPoint() call.
          dParam = rSolution.m_vStart[1] ;
          aSurface.ReSet() ;
          aUVPoint.ReSet() ;

          if(pEdgeCurveSurface1) { aSurface.Add(pEdgeCurveSurface1) ;
                                   aUVPoint.Add(pEdgeCurveUVPoint1) ;
                                 }
          if(pEdgeCurveSurface2) { aSurface.Add(pEdgeCurveSurface2) ;
                                   aUVPoint.Add(pEdgeCurveUVPoint2) ;
                                 }

          // fix the pEdge and its UVTrimCurves - edge is only modified if it is piecewise C0
          sm_SetXSectEdgeThroughPoint
              (pEdge,                // i/o: 3D Edge to modify
               &rSSSPoint,           // in : point to interpolate
               aSurface,             // in : two surfaces whose xsect define shape of pEdge
               aUVPoint,             // in : 3DPoint associated UVPoints for surfaces in rSurface
               dParam,               // i/o: in  = guessed pEdge Curve parameter assigned to this xsect throughPoint
                                     //      out = pEdge Curve parameter assigned to this xsect throughPoint by projecting sssPoint onto pSrcCurve3d
               dTDeviation,          // out: pThroughPoint to edge distance
               bModifiedEdgeCurve) ; // out: TRUE=Curves updated and outputs are set

          // move pEdge parameter in rSolution to surf/surf/surf xsect point
          rSolution.m_vStart[1] = dParam ;

          // increase deviation to include distance to this edge from rSSSPoint
          dDeviation = smos_Max(dDeviation, dTDeviation) ;

          // inform the public - remove obsolete UVTrimCurves
          if(bModifiedEdgeCurve)
            { bSSSPointFlag = TRUE;}

          // when there is an associated pEMate
          if(pEMate)
            {
              // use edge's solution to get a good guess on the mate
              double dParam1 ;
              pEdge->GetInterval().Inversion(dParam,dParam1) ;
              dParam1 = pEMate->GetInterval().Evaluate(dParam1) ;

              // fix the pEMate and its UVTrimCurves
              sm_SetXSectEdgeThroughPoint
                  (pEMate,               // i/o: 3D Edge to modify
                   &rSSSPoint,           // in : point to interpolate
                   aSurface,             // in : two surfaces whose xsect define shape of pEdge
                   aUVPoint,             // in : 3DPoint associated UVPoints for surfaces in rSurface
                   dParam1,              // i/o: in  = guessed pEdge Curve parameter assigned to this xsect throughPoint
                                         //      out = pEdge Curve parameter assigned to this xsect throughPoint by projecting sssPoint onto pSrcCurve3d
                   dTDeviation,          // out: pThroughPoint to edge distance
                   bModifiedMateCurve) ; // out: TRUE=Curves updated and outputs are set

              // increase deviation to include distance to this edge from rSSSPoint
              dDeviation = smos_Max(dDeviation, dTDeviation) ;

            } // end pEMate existence check

        } // end gwcUpdateEdges == TRUE branch
      else
        {
          // move pEdgeCurve parameter in rSolution to surf/surf/surf xsect point
          pEdgeCurve->LocalPointSolve(pEdge->GetInterval(),
                                      SM_SO_MINIMIZE,
                                      rSSSPoint,
                                      NULL, NULL, NULL,
                                      rSolution.m_vStart[1],
                                      bCurveFoundAnswer,
                                      sCurveProjection) ;
          if(bCurveFoundAnswer) { rSolution.m_vStart[1] = sCurveProjection.m_vStart[0] ; }

          // increase deviation to include distance from edge to rSSSPoint
          dDeviation = smos_Max(dDeviation, sCurveProjection.m_vStart.m_dSolutionValue) ;

          // to be complete we could also increase the dDeviation to include
          // the distance from the pEMate edge to the rSSSPoint.  But that
          // should be the same as the pEdge to rSSSPoint distance.

        } // end gwcUpdateEdges == FALSE branch
    } // end pEdge or pEMate existence check

#ifdef SM_DEBUG_CODE

  // check gaps

  // get the xyz point for each surface solution domain point
  SmBoolean bTmp0 = FALSE, bTmp1 = FALSE, bTmp2 = FALSE ;
  SmBSplineSurface * pBSplineSurface0 = SM_CAST_PTR(SmBSplineSurface, pClassCurveSurface1) ;
  SmBSplineSurface * pBSplineSurface1 = SM_CAST_PTR(SmBSplineSurface, pClassCurveSurface2) ;
  SmBSplineSurface * pBSplineSurface2 = SM_CAST_PTR(SmBSplineSurface, pEdgeCurveSurface1 ) ;
  SmTemporaryChangeValue<SmBoolean> sClean0(pBSplineSurface0 ? pBSplineSurface0->GetOutOfBoundsEnabled() : bTmp0, TRUE) ;
  SmTemporaryChangeValue<SmBoolean> sClean1(pBSplineSurface1 ? pBSplineSurface1->GetOutOfBoundsEnabled() : bTmp1, TRUE) ;
  SmTemporaryChangeValue<SmBoolean> sClean2(pBSplineSurface2 ? pBSplineSurface2->GetOutOfBoundsEnabled() : bTmp2, TRUE) ;

  SmPoint3d sSurface1Point(0,0,0), sSurface2Point(0,0,0), sSurface3Point(0,0,0) ;
  if(pCurveClassification) pClassCurveSurface1->EvaluatePoint(*pClassCurveUVPoint1,sSurface1Point) ;
  if(pCurveClassification) pClassCurveSurface2->EvaluatePoint(*pClassCurveUVPoint2,sSurface2Point) ;
  if(pEdge || pEMate)      pEdgeCurveSurface1->EvaluatePoint (*pEdgeCurveUVPoint1,sSurface3Point) ;

  // get the xyz point for each curve solution domain point
  SmPoint3d sClassCurvePoint, sEdgeCurvePoint ;
  if(pSrcCurve3d) pSrcCurve3d->EvaluatePoint(rSolution.m_vStart[0], sClassCurvePoint) ;
  if(pEdgeCurve ) pEdgeCurve->EvaluatePoint (rSolution.m_vStart[1], sEdgeCurvePoint) ;

  // get the distances between
  // 2 curve/curve xsect points and 3 surf/surf/surf xsect points
//  double dDistCC = (pSrcCurve3d && pEdgeCurve) ? sClassCurvePoint.DistanceBetween(sEdgeCurvePoint) : 0 ;

  //double dDist1a = (pSrcCurve3d && pCurveClassification) ? sSurface1Point.DistanceBetween(sClassCurvePoint) : 0.0 ;
  //double dDist2a = (pSrcCurve3d && pCurveClassification) ? sSurface2Point.DistanceBetween(sClassCurvePoint) : 0.0 ;
  //double dDist3a = (pSrcCurve3d && (pEdge || pEMate)   ) ? sSurface3Point.DistanceBetween(sClassCurvePoint) : 0.0 ;

  //double dDist1b = (pEdgeCurve  && pCurveClassification) ? sSurface1Point.DistanceBetween(sEdgeCurvePoint) : 0.0 ;
  //double dDist2b = (pEdgeCurve  && pCurveClassification) ? sSurface2Point.DistanceBetween(sEdgeCurvePoint) : 0.0 ;
  //double dDist3b = (pEdgeCurve  && (pEdge || pEMate)   ) ? sSurface3Point.DistanceBetween(sEdgeCurvePoint) : 0.0 ;

  //double dDist12 = (pCurveClassification)                      ? sSurface1Point.DistanceBetween(sSurface2Point) : 0.0 ;
  //double dDist23 = (pCurveClassification && (pEdge || pEMate)) ? sSurface2Point.DistanceBetween(sSurface3Point) : 0.0 ;
  //double dDist31 = (pCurveClassification && (pEdge || pEMate)) ? sSurface3Point.DistanceBetween(sSurface1Point) : 0.0 ;

//  double dMaxDistSS = smos_3Max(dDist12, dDist23, dDist31) ;
// double dmaxDistCS = smos_Max(smos_3Max(dDist1a, dDist2a, dDist3a),
//                              smos_3Max(dDist1b, dDist2b, dDist3b)) ;

  // draw Breps, Surfaces, Curves, Points
  if(bDebugMe)
    {
        smgfx_Erase() ;
        smgfx_SetLook(1,2, 0,0,1) ; if(pBrep1) pBrep1->Draw() ; sm_GraphicsLoop() ;
        smgfx_SetLook(1,2, 0,1,0) ; if(pBrep2 && pBrep2 != pBrep1) pBrep2->Draw() ; sm_GraphicsLoop() ;
        smgfx_SetLook(1,2, 0,0,0) ; if(pBrep3 && pBrep3 != pBrep1 && pBrep3 != pBrep2) pBrep3->Draw() ; sm_GraphicsLoop() ;

        smgfx_SetLook(1,2, 0,0,1) ; if(pClassCurveSurface1) pClassCurveSurface1->DrawUV() ; sm_GraphicsLoop() ;
        smgfx_SetLook(1,2, 0,0,0) ; if(pFace1) { pFace1->Draw(SM_DM_CROSSHATCH) ; } sm_GraphicsLoop() ;
        smgfx_SetLook(1,2, 0,1,0) ; if(pClassCurveSurface2) pClassCurveSurface2->DrawUV() ; sm_GraphicsLoop() ;
        smgfx_SetLook(1,2, 0,0,0) ; if(pFace2) { pFace2->Draw(SM_DM_CROSSHATCH) ; } sm_GraphicsLoop() ;
        smgfx_SetLook(1,2, 0,1,1) ; if(pEdgeCurveSurface1)  pEdgeCurveSurface1->DrawUV() ; sm_GraphicsLoop() ;
        smgfx_SetLook(1,2, 0,0,0) ; if(pFace3) { pFace3->Draw(SM_DM_CROSSHATCH) ; } sm_GraphicsLoop() ;

        smgfx_SetLook(5,7, 1,0,1) ; if(pSrcCurve3d) pSrcCurve3d->Draw() ; sm_GraphicsLoop() ;
        smgfx_SetLook(3,6, 0,1,0) ; if(pEdgeCurve)  pEdgeCurve->Draw() ;  sm_GraphicsLoop() ;
        smgfx_SetLook(3,6, 0,1,0) ; if(pEMateCurve) pEMateCurve->Draw() ; sm_GraphicsLoop() ;
        smgfx_SetLook(7,9, 1,0,0) ; sSurface1Point.Draw() ; sm_GraphicsLoop() ;
        smgfx_SetLook(7,9, 1,0,0) ; sSurface2Point.Draw() ; sm_GraphicsLoop() ;
        smgfx_SetLook(7,9, 1,0,0) ; sSurface3Point.Draw() ; sm_GraphicsLoop() ;
        sm_GraphicsLoop() ;

    } // end draw section
#endif // SM_DEBUG_CODE

  // all done
  return(SM_SUCCESS) ;

} // end sm_ApplyPointSolution

// ---- verbatim: SmCurveClass.cpp lines 3048-3275: sm_MakeConsistentClassifications
static SmStatus sm_MakeConsistentClassifications(SmCurveClassification & rCurveClass)
{
  // locals
  SmZoneTol3d sSrcZoneTol3d = SmTol::GetSrcZoneTol3d(&rCurveClass) ;

  // for every interval
  for (ULONG i=0; i<rCurveClass.GetSize(); i++)
    {
      SmCurveInterval & rCIvl = rCurveClass[i];

      // If interval classifies to an Edge
      if (rCIvl.m_vMid.GetPointClass() == SM_PC_EDGE)
        {
          // if startPoint classifies to a Vertex
          if (rCIvl.m_vStart.GetPointClass() == SM_PC_VERTEX)
             {
               SmEdge   *pClassifyEdge   = (SmEdge*)rCIvl.m_vMid.GetObject();
               SmVertex *pClassifyVertex = (SmVertex*)rCIvl.m_vStart.GetObject();
               SmVertex *pEdgeVertex1    = pClassifyEdge->GetVertex();
               SmVertex *pEdgeVertex2    = pClassifyEdge->GetOtherVertex(pEdgeVertex1);

               // check that classified vertex is one of the Edge's vertices
               if (   pClassifyVertex != pEdgeVertex1
                   && pClassifyVertex != pEdgeVertex2)
                 {
                   // Something is wrong: what looks like a coincident edge with:[Mid=Edge,Start=Vertex]
                   //     does not have the ClassifyStartV == Edge->[StartV or EndV] as expected
                   //     Since every Edge always has just one StartV and one EndV, it's not that the ClassifyEdge
                   //     is missing a connection to the ClassifyVertex.  It's more likely that one of the classifications
                   //     of Ivl.Mid=EDGE or the Ivl.Start=VERTEX was made to the wrong Object.  Perhaps one in the neighborhood
                   //     of the Classify EDGE and VERTEX objects themselves.
                   WARN(_T("MakeConsistentClassification: bad CoinEdge classify:[ClassifyMid=Edge,ClassifyStart=Vertex] but Edge:[StartV,EndV] != ClassifyStart")) ;

#ifdef SM_DEBUG_CODE
 SmBoolean bDebugMe=FALSE;
                   if (bDebugMe)
                     {
                       SM_DUMP_AND_ASSERT_VALID(&rCurveClass);
                       SmBrep *pBrep = pClassifyEdge->GetBrep() ;

                       smgfx_Erase();
                       smgfx_SetLook(1,2, 0,0,1) ; if(pBrep) { pBrep->Draw(TRUE) ; } sm_GraphicsLoop() ;
                       smgfx_SetLook(3,5, 0,1,1) ; rCurveClass.GetCurve()->DrawWDeriv(rCIvl.m_vInterval); sm_GraphicsLoop() ;
                       smgfx_SetLook(4,5, 1,0,1) ; pClassifyEdge->Draw(); sm_GraphicsLoop() ;
                       smgfx_SetLook(3,8, 1,0,0) ; pClassifyVertex->GetPoint().Draw(); sm_GraphicsLoop() ;
                       smgfx_SetLook(3,9, 0,1,0) ; pEdgeVertex1->GetPoint().Draw(); sm_GraphicsLoop() ;
                       smgfx_SetLook(3,9, 0,0,1) ; pEdgeVertex2->GetPoint().Draw(); sm_GraphicsLoop() ;
                       sm_GraphicsLoop();
                     }
#endif // SM_DEBUG_CODE
                   // Try 1 - Simple ClassifyEdge substitution: look for better ClassifyEdge assuming ClassifyVertex is correct
                   // Check every Edge attached to ClassifyVertex - pick the first Edge within XSectTol3d at the Ivl MidPoint
                   SmBoolean bFixedProblem = FALSE;

                   // for every edge attached to the vertex
                   SmTArray<SmEdge*> pVertexEdges;
                   pClassifyVertex->GetEdges(pVertexEdges);
                   for (ULONG kk=0; kk<pVertexEdges.GetSize(); kk++)
                     {
                       SmEdge     * pVertexEdge = pVertexEdges[kk];

                       SmBoolean    bSuccess;
                       SmPoint3d    sMidPnt;
                       double       dDropParam, dDistToCurve;
                       double       dTMid       = rCIvl.m_vInterval.Evaluate(0.5);

                       // Ivl MidPoint, XSectTol3d(SrcCrv,VertexEdge)
                       SER(rCurveClass.GetCurve()->EvaluatePoint(dTMid,sMidPnt));
                       SM_ASSERT_BREAK_MSG(rCurveClass.GetCurve()->GetDim() == 3, _T("Bad CurveClass 3d Assumption switch call to sm_GetXSectTol2dOr3d_ForClassifyPair()")) ;
                       SmXSectTol3d sXSectTol3d = SmTol::GetXSectTol3d(sSrcZoneTol3d, SmTol::GetZoneTol3d(pVertexEdge)) ;

                       // drop a midInterval Point to this VertexEdge
                       SER(pVertexEdge->GetCurve()->DropPoint(pVertexEdge->GetInterval(), // in : target curve allowed domain
                                                              sMidPnt,               // in : Point to drop to curve
                                                              NULL,                  // in : For Snapping, NULL=NoSnap, Vec pointing to CrvInside from desired EndPt for snapping.
                                                                                     //      when DropPt is within dDistTol in cpVecToCrvInside dir of EndPt, Snap to Ivl EndPt.
                                                                                     //      Vec handles ambiguities and makes sure that curves are not just touching at the ends.
                                                              sXSectTol3d,           // in : Used for SnapDist, and SM_SO_INTERSECT sXSectTol3d distance.
                                                                                     //      sXSectTol3d = dDistTol ;      SM_SO_INTERSECT
                                                                                     //      sXSectTol3d = SM_BIG_DOUBLE ; SM_SO_MINIMIZE, SM_SO_NORMALIZE
                                                                                     //      sSnapTol3d  = dDistTol ;      SM_SO_MINIMIZE, SM_SO_NORMALIZE, SM_SO_INTERSECT
                                                              NULL,                  // in : If given, call cheap LocalPointSolve() else call expensive GlobalPointSolve()
                                                              bSuccess,              // out: TRUE = found a drop point
                                                              dDropParam,            // out: found drop curve param
                                                              dDistToCurve)) ;       // out: found drop distance
                                                                                     // in : SM_SO_MINIMIZE = allow nonNormal drops near endPoints
                                                                                     //      SM_SO_INTERSECT= like Minimize but point must be within dDistTol
                                                                                     //      SM_SO_NORMALIZE= exclude nonNormal drops near endPoints
                                                                                     //      default:[SM_SO_MINIMIZE] to preserve original behavior

                       // if point was close enough to edge - substitute the ClassifyMid Edge target with the found VertexEdge
                       if (bSuccess)
                         {
                           // set the interval's Classification object to this edge
                           MSG(_T("MakeConsistentClassification: fixing bad CoinEdge classify, switching ClassifyEdge = a StartVertexEdge within XSectTol3d at mid interval")) ;
                           rCIvl.m_vMid.SetClassObject(SM_PC_EDGE, pVertexEdge);
                           bFixedProblem = TRUE;
                           break;
                         } // end point was close enough check
                     } // end iter every edge attached to the vertex

                   // arrive here when none of the Edges attached to ClassifyVertex are likely coincident with the ClassifyVertex

                   // Try 2 - simple ClassifyVertex substitution: Substitute ClassifyVertex with the EdgeVertex
                   //         closest to ClassifyVertex
                   //         gwc: I'm surprised that this helps
                   if (!bFixedProblem)
                     {
                       MSG(_T("MakeConsistentClassification: fixing bad CoinEdge classify, switching Start ClassifyVertex = Closest EdgeVertex")) ;
                       if (pClassifyVertex->GetPoint().DistanceBetween(pEdgeVertex1->GetPoint()) <
                           pClassifyVertex->GetPoint().DistanceBetween(pEdgeVertex2->GetPoint()) )
                            { rCIvl.m_vStart.SetClassObject(SM_PC_VERTEX, pEdgeVertex1); }
                       else { rCIvl.m_vStart.SetClassObject(SM_PC_VERTEX, pEdgeVertex2); }
                     } // end bFixedProblem == FALSE check

                 } // end classified vertex is one of the Edge's vertices check
             } // end Classification IVl with Mid=EDGE and startPoint classified to a Vertex check

          // if endPoint classifies to a Vertex
          if (rCIvl.m_vEnd.GetPointClass() == SM_PC_VERTEX)
            {
              SmEdge   *pClassifyEdge    = (SmEdge*)rCIvl.m_vMid.GetObject();
              SmVertex *pClassifyVertex  = (SmVertex*)rCIvl.m_vEnd.GetObject();
              SmVertex *pEdgeVertex1 = pClassifyEdge->GetVertex();
              SmVertex *pEdgeVertex2 = pClassifyEdge->GetOtherVertex(pEdgeVertex1);

              // check that classified vertex is one of the Edge's vertices
              if (   pClassifyVertex != pEdgeVertex1
                  && pClassifyVertex != pEdgeVertex2)
                {
                   // Something is wrong: what looks like a coincident edge with:[Mid=Edge,End=Vertex]
                   //     does not have the ClassifyEndV == Edge->[StartV or EndV] as expected
                   //     Since every Edge always has just one StartV and one EndV, it's not that the ClassifyEdge
                   //     is missing a connection to the ClassifyVertex.  It's more likely that one of the classifications
                   //     of Ivl.Mid=EDGE or the Ivl.Start=VERTEX was made to the wrong Object.  Perhaps one in the neighborhood
                   //     of the Classify EDGE and VERTEX objects themselves.
                  WARN(_T("MakeConsistentClassification: bad CoinEdge classify:[ClassifyMid=Edge,ClassifyEnd=Vertex] but Edge:[StartV,EndV] != ClassifyStart")) ;

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe=FALSE;
              if (bDebugMe)
                {
                  SM_DUMP_AND_ASSERT_VALID(&rCurveClass);
                  SmBrep *pBrep = pClassifyEdge->GetBrep() ;

                  smgfx_Erase();
                  smgfx_SetLook(1,2, 0,0,1) ; if(pBrep) { pBrep->Draw(TRUE) ; } sm_GraphicsLoop() ;
                  smgfx_SetLook(3,5, 0,1,1) ; rCurveClass.GetCurve()->DrawWDeriv(rCIvl.m_vInterval); sm_GraphicsLoop() ;
                  smgfx_SetLook(4,5, 1,0,1) ; pClassifyEdge->Draw(); sm_GraphicsLoop() ;
                  smgfx_SetLook(3,8, 1,0,0) ; pClassifyVertex->GetPoint().Draw(); sm_GraphicsLoop() ;
                  smgfx_SetLook(3,9, 0,1,0) ; pEdgeVertex1->GetPoint().Draw(); sm_GraphicsLoop() ;
                  smgfx_SetLook(3,9, 0,0,1) ; pEdgeVertex2->GetPoint().Draw(); sm_GraphicsLoop() ;
                  sm_GraphicsLoop();
                }
#endif // SM_DEBUG_CODE

                   // Try 1 - Simple ClassifyEdge substitution: look for better ClassifyEdge assuming ClassifyVertex is correct
                   // Check every Edge attached to ClassifyVertex - pick the first Edge within XSectTol3d at the Ivl MidPoint
                  SmBoolean bFixedProblem = FALSE;

                  // for every edge attached to the vertex
                  SmTArray<SmEdge*> sEdges;
                  pClassifyVertex->GetEdges(sEdges);
                  for (ULONG kk=0; kk<sEdges.GetSize(); kk++)
                    {
                      SmEdge * pVertexEdge = sEdges[kk];

                      SmBoolean bSuccess;
                      SmPoint3d sMidPnt;
                      double dDropParam, dDistToCurve;
                      double   dTMid  = rCIvl.m_vInterval.Evaluate(0.5);

                       // Ivl MidPoint, XSectTol3d(SrcCrv,VertexEdge)
                      SER(rCurveClass.GetCurve()->EvaluatePoint(dTMid,sMidPnt));
                      SM_ASSERT_BREAK_MSG(rCurveClass.GetCurve()->GetDim() == 3, _T("Bad CurveClass 3d Assumption switch call to sm_GetXSectTol2dOr3d_ForClassifyPair()")) ;
                      SmXSectTol3d sXSectTol3d = SmTol::GetXSectTol3d(sSrcZoneTol3d, SmTol::GetZoneTol3d(pVertexEdge)) ;

                      SER(pVertexEdge->GetCurve()->DropPoint(pVertexEdge->GetInterval(), // in : target curve allowed domain
                                                             sMidPnt,               // in : Point to drop to curve
                                                             NULL,                  // in : For Snapping, NULL=NoSnap, Vec pointing to CrvInside from desired EndPt for snapping.
                                                                                    //      when DropPt is within dDistTol in cpVecToCrvInside dir of EndPt, Snap to Ivl EndPt.
                                                                                    //      Vec handles ambiguities and makes sure that curves are not just touching at the ends.
                                                             sXSectTol3d,           // in : Used for SnapDist, and SM_SO_INTERSECT sXSectTol3d distance.
                                                                                    //      sXSectTol3d = dDistTol ;      SM_SO_INTERSECT
                                                                                    //      sXSectTol3d = SM_BIG_DOUBLE ; SM_SO_MINIMIZE, SM_SO_NORMALIZE
                                                                                    //      sSnapTol3d  = dDistTol ;      SM_SO_MINIMIZE, SM_SO_NORMALIZE, SM_SO_INTERSECT
                                                             NULL,                  // in : If given, call cheap LocalPointSolve() else call expensive GlobalPointSolve()
                                                             bSuccess,              // out: TRUE = found a drop point
                                                             dDropParam,            // out: found drop curve param
                                                             dDistToCurve)) ;       // out: found drop distance
                                                                                    // in : SM_SO_MINIMIZE = allow nonNormal drops near endPoints
                                                                                    //      SM_SO_INTERSECT= like Minimize but point must be within dDistTol
                                                                                    //      SM_SO_NORMALIZE= exclude nonNormal drops near endPoints
                                                                                    //      default:[SM_SO_MINIMIZE] to preserve original behavior

                      // if point was close enough to edge - substitute the ClassifyMid Edge target with the found VertexEdge
                      if (bSuccess)
                        {
                          // set the interval's Classification object to this edge
                          MSG(_T("MakeConsistentClassification: fixing bad CoinEdge classify, switching ClassifyEdge = a EndVertexEdge within XSectTol3d at mid interval")) ;
                          rCIvl.m_vMid.SetClassObject(SM_PC_EDGE, pVertexEdge);
                          bFixedProblem = TRUE;
                          break;
                        } // end point was close enough check
                    }  // end iter every edge attached to the vertex

                  // arrive here when none of the Edges attached to ClassifyVertex are likely coincident with the ClassifyVertex

                  // Try 2 - simple ClassifyVertex substitution: Substitute ClassifyVertex with the EdgeVertex
                  //         closest to ClassifyVertex
                  //         gwc: I'm surprised that this helps
                  if (!bFixedProblem)
                    {
                      MSG(_T("MakeConsistentClassification: fixing bad CoinEdge classify, switching End ClassifyVertex = Closest EdgeVertex")) ;
                      if (pClassifyVertex->GetPoint().DistanceBetween(pEdgeVertex1->GetPoint()) <
                          pClassifyVertex->GetPoint().DistanceBetween(pEdgeVertex2->GetPoint()) )
                           { rCIvl.m_vEnd.SetClassObject(SM_PC_VERTEX, pEdgeVertex1); }
                      else { rCIvl.m_vEnd.SetClassObject(SM_PC_VERTEX, pEdgeVertex2); }
                    }
                } // end classified vertex is one of the Edge's vertices check
            } // end Classification IVl with Mid=EDGE and EndPoint classified to a Vertex check
        } // end interval classified to Edge check
    } // end iter every interval

  return SM_SUCCESS;

} // end sm_MakeConsistentClassifications

// ---- verbatim: SmCurveClass.cpp lines 3752-3806: sm_GetCurveCurveAngle
static double sm_GetCurveCurveAngle
  (const SmCurve *pThisCurve,       // in : 1st tgt curve
   double         dThisParam,       // in : param for ThisCurve evaluation
   const SmCurve *pOtherCurve,      // in : 2nd tgt Curve
   double         dOtherParam,      // in : param for OtherCurve evaluation
   double         dDeviation)       // in : expected deviation between curve points - only used for debug code check
{
  SmVector3d sThisPV[2], sOtherPV[2] ;

  // no work - no curves
  if(   pThisCurve == NULL
     || pOtherCurve == NULL)
   { return 0.0 ; }

  // check input - curves are 3d and params are valid
  SM_ASSERT(pThisCurve-> GetDim() == 3) ;
  SM_ASSERT(pOtherCurve->GetDim() == 3) ;
  SM_ASSERT(pThisCurve-> GetNaturalInterval().ContainsValue(dThisParam)  == TRUE) ;
  SM_ASSERT(pOtherCurve->GetNaturalInterval().ContainsValue(dOtherParam) == TRUE) ;

  // Evaluate the Curve locations
  pThisCurve->Evaluate (dThisParam,  1, TRUE, sThisPV) ;
  pOtherCurve->Evaluate(dOtherParam, 1, TRUE, sOtherPV) ;

  // check for zero-length tangents.
  SmScaledZero dScaledZero = SmTol::GetScaledZero( sThisPV[0] );
  if ( sThisPV[1].Length() < dScaledZero )
    { return 0.0; }
  if ( sOtherPV[1].Length() < dScaledZero )
    { return 0.0; }

#ifdef SM_DEBUG_CODE
  // check that points are the same
  double dChkDist = sThisPV[0].DistanceBetween(sOtherPV[0]) ;
  double dChkTol  = 5.0 * smos_Max(dDeviation, dScaledZero.val) ;

  if(dChkDist >= dChkTol)
    { SM_ASSERT(dChkDist < dChkTol) ; }
#else
    SM_REF1(dDeviation);
#endif // SM_DEBUG_CODE

  // get the angle between the tangents
  double dAngleRadians = 0.0 ;
  sThisPV[1].AngleBetween(sOtherPV[1], dAngleRadians) ;

  // return angle from 0 to PI/2
  if(dAngleRadians < 0.0) dAngleRadians = -dAngleRadians ;
  if(dAngleRadians > SM_PI/2.0) dAngleRadians = smos_Fabs(SM_PI - dAngleRadians) ;

  // all done
  return(dAngleRadians) ;

} // end sm_GetCurveCurveAngle

// ---- verbatim: SmCurveClass.cpp lines 3821-3920: sm_CullOrRefineCCIIn3d
static SmStatus sm_CullOrRefineCCIIn3d // eff: tigthen 2d curve/curve intersections by checking 3dcurve/curve intersection tolerances
 (const SmSurface  & crSurface,        // in : surface of face being classified against
  const SmCurve    & crUVCurve1,       // in : UVCurve being classified
  const SmExtent1d & crInterval1,      // in : UVCurve's interval
  const SmCurve    & cr3DCurve2,       // in : 3dCurve for face's trimCurve used to generate rSolutions
  const SmExtent1d & crInterval2,      // in : 3DCurve's interval
  double             dXSectTol3d,      // in : XSectTol3d(SrcCurve,Edge) to use in 3dCurve/3dCurve intersection
  SmSolutionArray  & rSolutions)       // in : solutions from 2dCurve/2dCurve intersections
{
  // project UVCurve into 3d space
  SmCrvOnSurf s3DCurve1(SM_CONST_CAST(SmCurve&,  crUVCurve1),
                        SM_CONST_CAST(SmSurface&,crSurface),
                        NULL,
                        0,
                        crSurface.GetContext());
//  s3DCurve1.SetContext(crSurface.GetContext());

  // for every solution
  for (ULONG i=0; i<rSolutions.GetSize(); i++)
    {
      SmSolution & rSol = rSolutions[i];

      // when the solution is a point (not an interval)
      if (rSol.m_eSolutionType == SM_ST_SINGLE_VALUE)
        {
          double dNewParam1, dNewParam2, dDeviation;
          SmBoolean bFoundIntersection;

          // get the 3dCurve/3Dcurve intersection
          SER(s3DCurve1.LocalCurveIntersect(crInterval1,         // in : ThisCurve intersection limits
                                            cr3DCurve2,          // in : other curve
                                            crInterval2,         // in : other curve intersection limits
                                            dXSectTol3d,         // in : Passed to SmLocalSolveNd::SolveIt() and
                                                                 //      LocalCurveSolve()
                                            rSol.m_vStart[0],    // in : ThisCurve GuessPoint
                                            rSol.m_vStart[1],    // in : OtherCurve GuessPoint
                                            bFoundIntersection,  // out: TRUE = curves intersect
                                            dNewParam1,          // out: ThisCurve intersection point
                                            dNewParam2,          // out: OtherCurve intersection point
                                            dDeviation));        // out: Intersection Gap Vector size

          // when 3dCurve/3dCurve intersection is tighter than 2dCurve/2dCurve intersection
          if (bFoundIntersection && dDeviation < rSol.m_vStart.m_dSolutionValue)
            {
              // use it
              rSol.m_vStart.m_dSolutionValue = dDeviation;
              rSol.m_vStart[0] = dNewParam1;
              rSol.m_vStart[1] = dNewParam2;
            } // end better 3d xsect solution check

          // when 3dCurve/3dCurve intersection is not good enough
          if(!bFoundIntersection || dDeviation > dXSectTol3d)
            {
              // remove the solution
              rSolutions.RemoveAt(i) ;
            }
        } // end SM_ST_SINGLE_VALUE solution check

      // when the solution is a coincident curve pair
      else if(rSol.m_eSolutionType == SM_ST_RANGE_OF_VALUES)
        {
          // get quick max edge/UVTrimCurve 3d distance estimate at 8 sample points
          double dMaxCurveCurveDist3d = 0.0 ;
          SmExtent1d sSolInterval1(rSol.m_vStart[0], rSol.m_vEnd[0]) ;
          SER(s3DCurve1.CurveMaxDistanceBetween(sSolInterval1,
                                                cr3DCurve2,
                                                rSol.m_vStart[1],
                                                rSol.m_vEnd[1],
                                                8,
                                                NULL,
                                                dMaxCurveCurveDist3d));

          // when the quick estimate is good but not very tight
          if (   dMaxCurveCurveDist3d < dXSectTol3d
              && dMaxCurveCurveDist3d > dXSectTol3d / 100.00)
            {
              // get max dist value possible between curves by setting samplePointCount = 0
              SER(s3DCurve1.CurveMaxDistanceBetween(sSolInterval1,
                                                    cr3DCurve2,
                                                    rSol.m_vStart[1],
                                                    rSol.m_vEnd[1],
                                                    0,
                                                    NULL,
                                                    dMaxCurveCurveDist3d));
            }

          // when max edge/UVTrimCurve 3d distance is not good enough
          if(dMaxCurveCurveDist3d > dXSectTol3d)
            {
              // remove the solution
              rSolutions.RemoveAt(i) ;
            }

        } // end SM_ST_RANGE_OF_VALUES solution check
    } // end iter every solution

  // all done
  return SM_SUCCESS;

} // end sm_CullOrRefineCCIIn3d

// ---- verbatim: SmCurveClass.cpp lines 3937-4044: sm_ComputeFacePointClass
static SmStatus sm_ComputeFacePointClass
  (const SmPoint3d       & cr3DPoint,     // in : Point expected to be on face
   SmZoneTol3d             sSrcZoneTol3d, // in : ZoneTol3d(3Dpoint Assoc Obj, e.g. SrcCurve for a CurveClassification with this Face)
   SmFace                * pFace,         // in : target face
   SmPointClassification & rPointClass)   // out: classification to remember point is on face
{
  // locals
  SmSurface *pSurface = pFace->GetSurface(); NER(pSurface);

// Method:
// 1: project to surface without checking trimmed (face) boundaries:
//    'point testing turned off'.
// 2: face->PointClassify().

  // Project Point to Face->Surface
  SmSolution sData[16];
  SmSolutionArray sSolutions(16,sData);

  // Turn off point testing in the face (temporarily).
  SmSurfaceCache  * pSC = smsurf_GetSurfaceCache(pSurface) ;
  SmCacheCheckOutIn sCheckIO(pSC);

  // Local scope for temporary assignment to pSC->PointTestEnabled block:
    {
      // Turn off point testing so GlobalPointSolve() will keep all point solutions
      //   without classifying the solution point against the trim boundaries.
      SmTemporaryChangeValue<SmBoolean> sCV(pSC->m_bPointTestEnabled,FALSE);

      SmXSectTol3d sXSectTol3d = SmTol::GetXSectTol3d(sSrcZoneTol3d, SmTol::GetZoneTol3d(pFace)) ;

      SER(pSurface->GlobalPointSolve(pFace->GetUVDomain(),
                                     SM_SO_INTERSECT,
                                     cr3DPoint,
                                     sSrcZoneTol3d,
                                     NULL,
                                     SM_SR_ALL,
                                     sSolutions));

      // If global point solve fails, try again with a bigger tolerance.
      if (sSolutions.GetSize() == 0)
        {
          SER(pSurface->GlobalPointSolve(pFace->GetUVDomain(),
                                         SM_SO_INTERSECT,
                                         cr3DPoint,
                                         10 * sXSectTol3d,  // increased tol in hopes of a solution
                                         NULL,
                                         SM_SR_ALL,
                                         sSolutions));

          // If point still fails to project, give it one more try.
          if (sSolutions.GetSize() == 0)
            {
              SER(pSurface->GlobalPointSolve(pFace->GetUVDomain(),
                                             SM_SO_INTERSECT,
                                             cr3DPoint,
                                             100 * sXSectTol3d,  // additional increased tol in hopes of a solution
                                             NULL,
                                             SM_SR_ALL,
                                             sSolutions));
              // signal error and give up if point still fails to project
              if (sSolutions.GetSize() == 0)
                {
                  return SM_ERR;
                } // end 3rd project failed check
            } // end 2nd project failed check
        } // end 1st project point to surface failed check
    } // end local scope for temporary assignment to pSC->PointTestEnabled block

  // Find the best solution, and classify it to the face.
  double dBestDist = SM_BIG_DOUBLE;
  SmPoint2d sUV;
  for (ULONG i=0; i<sSolutions.GetSize(); i++)
    {
      SmSolution & rSol = sSolutions[i];
      if (rSol.m_vStart.m_dSolutionValue < dBestDist)
        {
          dBestDist = rSol.m_vStart.m_dSolutionValue;
          sUV.Set( rSol.m_vStart[0], rSol.m_vStart[1] );
        }
    }

  pFace->PointClassify(sUV,            // in : UVPoint to Classify
                       sSrcZoneTol3d,  // in : ZoneTol3d of obj assoc with UVPoint, not this face
                                       //     (when none, use: SmTol::GetZoneTol3d(Brep_Context_Or_NULL)
                       TRUE,           // in : TRUE = return point face->Vertex/Edge intersections when found
                                       //      FALSE= return loop-containment classifications.
                       TRUE,           // in : TRUE = force classification to be done only in UV space.
                                       //      else when LoopClassification fails try again in 3d.
                       rPointClass ) ; // out: Classification of the point.
                                       //        when in : m_ePointClass = SM_PC_FACE
                                       //             out: m_ePointClass = SM_PC_UNKNOWN
                                       //             on : m_ePointClass = SM_PC_EDGE or SM_PC_VERTEX
#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
  if ( bDebugMe )
    {
      if ( rPointClass.GetPointClass() != SM_PC_FACE )
        {
          SM_DBG_MSG( TRUE,
            _T("MSG: sm_ComputeFacePointClass() did NOT classify to Face.\n")) ;
        }
    }
#endif // SM_DEBUG_CODE

  // all done
  return SM_SUCCESS;

} // end sm_ComputeFacePointClass

// ---- verbatim: SmCurveClass.cpp lines 4802-4810: sm_ObjectDimension
static int sm_ObjectDimension( SmObject *pObj )
{
  return(  pObj == NULL                    ? 4
         : pObj->IsKindOf( SmFace_TYPE )   ? 2
         : pObj->IsKindOf( SmEdge_TYPE )   ? 1
         : pObj->IsKindOf( SmVertex_TYPE ) ? 0
         :                                   4 ) ; // UNKNOWN, etc.

} // end sm_ObjectDimension

// clang-format on
} // namespace sc_cc_p1

namespace
{
enum : int64_t
{
    K_CURVE = 0,
    K_PLANE = 1,
    K_BSURF = 2,
    K_POINT = 3,
    K_VERTEX = 4,
    K_EDGE = 5,
    K_FACE = 6,
    K_LOOP = 7,
    K_OTHER = 8,
};

// Ops (keep in sync with crates/solidcore-ffi/src/units/intersect_curve_class_p1.rs).
enum : int64_t
{
    OP_SET_SIZE = 0,
    OP_REMOVE_AT = 1,
    OP_IVL_SET = 2,
    OP_IVL_SET_MIN = 3,
    OP_IVL_SET_MAX = 4,
    OP_IVL_RESET = 5,
    OP_IVL_ASSIGN = 6,
    OP_IVL_NEW = 7,
    OP_PC_SET_CLASS = 8,
    OP_PC_SET_T = 9,
    OP_PC_SET_UV = 10,
    OP_PC_SET_GAP = 11,
    OP_PC_SET_SSS = 12,
    OP_PC_SET_PRESNAP = 13,
    OP_PC_SET_SRC_TOL = 14,
    OP_SET_CURVE = 15,
    OP_SET_SURFACE1 = 16,
    OP_SET_SURFACE2 = 17,
    OP_SET_FACE = 18,
    OP_SET_INTERSECT_DATA = 19,
    OP_SET_IS_CURVE_CURRENT = 20,
    OP_SET_IGNORE_TANGENCY = 21,
    OP_SET_IS_CURVE_OWNED = 22,
    OP_SET_OK_TO_SET_TOL = 23,
    OP_SET_FILLET_DATA = 24,
    OP_SET_TGT_LOOPS_ONLY = 25,
    OP_ADD_TGT_LOOP = 26,
    OP_SET_SRC_TOL = 27,
    OP_SET_UV_CURVE1 = 28,
    OP_SET_UV_CURVE2 = 29,
    OP_SUB_INTERVAL = 30,
    OP_CLASSIFY_CRV_LOC = 31,
    OP_SET_PC_PARAMS = 32,
    OP_SQUEEZE_END = 33,
    OP_IS_REFERENCED = 34,
    OP_MAKE_CONSISTENT = 35,
    OP_TYPE = 36,
    OP_SRC_TOL = 37,
    OP_IVL_TOLS = 38,
    OP_OBJ_DIM = 39,
    OP_XSECT_CURVE = 40,
    OP_APPLY = 41,
    OP_ANGLE = 42,
    OP_CULL = 43,
    OP_TOL_PAIR = 44,
    OP_GAPS = 45,
    OP_TOUCH_CONNECTED = 46,
    OP_TOUCH_WINDING = 47,
    OP_EDGE_THROUGH = 48,
    OP_CCI_GEOMETRY = 49,
    OP_FACE_POINT_CLASS = 50,
};

// Access to private / protected members through explicit instantiation.
template <typename Tag, typename Tag::type M>
struct Rob
{
    friend typename Tag::type get(Tag) { return M; }
};

struct IvlSetMin
{
    typedef SmStatus (SmCurveInterval::*type)(double);
    friend type get(IvlSetMin);
};
template struct Rob<IvlSetMin, &SmCurveInterval::SetIntervalMin>;

struct IvlSetMax
{
    typedef SmStatus (SmCurveInterval::*type)(double);
    friend type get(IvlSetMax);
};
template struct Rob<IvlSetMax, &SmCurveInterval::SetIntervalMax>;

struct CcOwned
{
    typedef SmBoolean SmCurveClassification::*type;
    friend type get(CcOwned);
};
template struct Rob<CcOwned, &SmCurveClassification::m_bIsCurveOwned>;

struct Rd
{
    const double* in;
    int64_t n;
    int64_t at;
    bool bad = false;
    double num()
    {
        if (at >= n)
        {
            bad = true;
            return 0.0;
        }
        return in[at++];
    }
    int64_t idx() { return (int64_t)num(); }
    bool flag() { return num() != 0.0; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

struct ScVertex : public SmVertex
{
    ScVertex() : SmVertex() {}
};

struct ScEdge : public SmEdge
{
    explicit ScEdge(SmCurve* c) : SmEdge() { m_pCurve = c; }
};

struct ScFace : public SmFace
{
    explicit ScFace(SmSurface* s) : SmFace() { m_pSurface = s; }
};

// SmObject stand-in of one class; answers GetType and IsKindOf like that class.
class ScStandIn : public SmObject
{
public:
    explicit ScStandIn(SM_TYPE t) : m_type(t) {}
    SM_TYPE GetType() const override { return m_type; }
    SmBoolean IsKindOf(SM_TYPE t) const override { return t == m_type || SmObject::IsKindOf(t); }

private:
    SM_TYPE m_type;
};

const SmContext& the_context()
{
    static SmContext* p = new SmContext();
    return *p;
}

// An object carrying the context, for the topology `operator new(size_t, SmObject*)`.
SmObject* anchor()
{
    static SmLine* p = new (&the_context())
        SmLine(SmPoint3d(0.0, 0.0, 0.0), SmPoint3d(1.0, 0.0, 0.0), 3, &the_context());
    return p;
}

gw_CURVE* read_nurb(Rd& rd)
{
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
    return cur;
}

// `n m p q r s cpts(x y z w, v fastest) u v`.
gw_SURFACE* read_nurb_surface(Rd& rd)
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

// A curve block `kind dim ...` (0: SmBSplineCurve `oob n p m cpts(x y z w) knots`;
// 1: SmHermiteCurve `P1 D1 P2 D2`; 2: bounded SmLine `P V d0 d1 scale io`; 3 / 4: SmEllipse /
// SmCircle `C X Y d0 d1 rx [ry] io`).
SmCurve* read_curve(Rd& rd, const SmContext& ctx, bool& is_bspline, ULONG& dim)
{
    int64_t kind = rd.idx();
    dim = (ULONG)rd.idx();
    is_bspline = kind == 0;
    if (kind == 3 || kind == 4)
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double rx = rd.num();
        double ry = kind == 3 ? rd.num() : rx;
        bool io = rd.flag();
        if (kind == 3)
            return new (ctx) SmEllipse(c, x, y, SmExtent1d(d0, d1), rx, ry, dim, &ctx, NULL, io ? TRUE : FALSE);
        return new (ctx) SmCircle(c, x, y, SmExtent1d(d0, d1), rx, dim, &ctx, NULL, io ? TRUE : FALSE);
    }
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

struct Pool
{
    std::vector<int64_t> kinds;
    std::vector<void*> ptrs; // SmObject* or SmPoint3d*
    std::vector<bool> bspline;
    std::vector<ULONG> dims;
    std::map<const void*, int64_t> ids;

    double id(const void* p) const
    {
        if (p == NULL)
            return -1.0;
        auto it = ids.find(p);
        return it == ids.end() ? -2.0 : (double)it->second;
    }
    bool valid(int64_t i) const { return i >= 0 && i < (int64_t)ptrs.size(); }
    // pool object i (NULL for -1); `ok` false for an index out of range or of the wrong kind
    void* get(int64_t i, std::initializer_list<int64_t> want, bool& ok) const
    {
        if (i == -1)
            return NULL;
        if (!valid(i))
        {
            ok = false;
            return NULL;
        }
        bool match = want.size() == 0;
        for (int64_t w : want)
            match = match || kinds[(size_t)i] == w;
        if (!match)
            ok = false;
        return ptrs[(size_t)i];
    }
    int64_t kind_of(const void* p) const
    {
        auto it = ids.find(p);
        return it == ids.end() ? -1 : kinds[(size_t)it->second];
    }
};

void put_pc(std::vector<double>& o, const SmPointClassification& pc, const Pool& pool)
{
    o.push_back((double)pc.GetPointClass());
    o.push_back(pool.id(pc.GetObject()));
    o.push_back(pool.id(pc.GetFace()));
    o.push_back(pc.GetDeviation());
    o.push_back(pc.GetTParam());
    SmPoint2d uv = pc.GetUVParam();
    o.push_back(uv.x);
    o.push_back(uv.y);
    put_vec(o, pc.GetUVWParam());
    o.push_back(flg(pc.AreParametersSet()));
    o.push_back(pc.GetPreSnapParam());
    o.push_back(flg(pc.GetSSSPointFlag()));
    put_vec(o, pc.GetSSSPoint());
    o.push_back(SmTol::GetSrcZoneTol3d(&pc));
}

void put_ivl(std::vector<double>& o, const SmCurveInterval& ivl, const Pool& pool)
{
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    put_pc(o, ivl.m_vStart, pool);
    put_pc(o, ivl.m_vMid, pool);
    put_pc(o, ivl.m_vEnd, pool);
}

void put_cc(std::vector<double>& o, const SmCurveClassification& cc, const Pool& pool)
{
    o.push_back((double)cc.GetSize());
    SmExtent1d ivl = cc.GetInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    o.push_back(SmTol::GetSrcZoneTol3d(&cc));
    o.push_back(flg(cc.*get(CcOwned())));
    o.push_back(flg(cc.GetIsCurveCurrent()));
    o.push_back(flg(cc.GetIgnoreTangency()));
    o.push_back(flg(cc.GetDoingCrossHatch()));
    o.push_back(flg(cc.GetDoingFillets()));
    o.push_back(flg(cc.GetOKToSetTolerance()));
    o.push_back(flg(cc.GetTgtLoopsOnly()));
    o.push_back(pool.id(cc.GetCurve()));
    o.push_back(pool.id(cc.GetUVCurve1()));
    o.push_back(pool.id(cc.GetUVCurve2()));
    o.push_back(pool.id(cc.GetAnyUVCurve1()));
    o.push_back(pool.id(cc.GetAnyUVCurve2()));
    o.push_back(pool.id(cc.GetSurface1()));
    o.push_back(pool.id(cc.GetSurface2()));
    o.push_back(pool.id(cc.GetFace()));
    o.push_back(pool.id(cc.GetTopologyIntersector()));
    o.push_back(pool.id(cc.GetOtherClassification()));
    const SmTArray<SmLoop*>& loops = cc.GetTgtLoops();
    o.push_back((double)loops.GetSize());
    for (ULONG i = 0; i < loops.GetSize(); i++)
        o.push_back(pool.id(loops[i]));
    for (ULONG i = 0; i < cc.GetSize(); i++)
        put_ivl(o, cc.GetAt(i), pool);
}

// natural interval, control point and knot counts of a B-spline (else -1 -1), 7 points
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    const SmBSplineCurve* b = c->IsKindOf(SmBSplineCurve_TYPE) ? (const SmBSplineCurve*)c : NULL;
    const gw_CURVE* cur = b ? b->GetGwNurbPointer() : NULL;
    o.push_back(cur ? (double)cur->pol->n : -1.0);
    o.push_back(cur ? (double)cur->knt->m : -1.0);
    for (int i = 0; i <= 6; i++)
    {
        SmPoint3d p;
        SmStatus s = c->EvaluatePoint(ivl.Evaluate(i / 6.0), p);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, p);
    }
}

bool is_surface(int64_t k) { return k == K_PLANE || k == K_BSURF; }

// the object kind a classification type needs; -1 for none (UNKNOWN, NOTHING), -2 invalid
int64_t kind_of_class(int64_t c)
{
    switch (c)
    {
    case 100:
    case 110: return -1;
    case 102: return K_FACE;
    case 103: return K_EDGE;
    case 104: return K_VERTEX;
    case 105: return K_POINT;
    case 106: return K_CURVE;
    case 107: return K_BSURF; // or K_PLANE
    default: return -2;
    }
}

SmPointClassification* pick(SmCurveInterval& ivl, int64_t w)
{
    return w == 0 ? &ivl.m_vStart : w == 1 ? &ivl.m_vMid : w == 2 ? &ivl.m_vEnd : NULL;
}

// A point classification ComputePointParameters may be called on: not a point or stand-in.
bool computable(const SmPointClassification& pc, const Pool& pool)
{
    int64_t k = pc.GetObject() == NULL ? -1 : pool.kind_of(pc.GetObject());
    return k != K_POINT && k != K_LOOP && k != K_OTHER;
}

bool read_touches(Rd& rd, const Pool& pool, SmTArray<SmTouchData>& out)
{
    int64_t n = rd.idx();
    if (n < 0 || n > 64)
        return false;
    for (int64_t i = 0; i < n; i++)
    {
        SmTouchData t;
        int64_t c = rd.idx();
        t.m_eLoopDir = (SmCrvDirOnFaceType)rd.idx();
        t.m_lLoopIndx = (ULONG)rd.idx();
        t.m_dParam = rd.num();
        t.m_lPtClassifyIndx = (ULONG)rd.idx();
        int64_t pc = rd.idx();
        if ((c != -1 && !pool.valid(c)) || (pc != -1 && !pool.valid(pc)))
            return false;
        t.m_pConnectedTo = c == -1 ? NULL : (SmTopology*)pool.ptrs[(size_t)c];
        t.m_pPtClassify = pc == -1 ? NULL : (SmPointClassification*)pool.ptrs[(size_t)pc];
        out.Add(t);
    }
    return !rd.bad;
}

void put_touch(std::vector<double>& o, const SmTouchData& t, const Pool& pool)
{
    o.push_back(pool.id(t.m_pConnectedTo));
    o.push_back((double)t.m_eLoopDir);
    o.push_back((double)t.m_lLoopIndx);
    o.push_back(t.m_dParam);
    o.push_back((double)t.m_lPtClassifyIndx);
    o.push_back(pool.id(t.m_pPtClassify));
}

int32_t run(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    Pool pool;

    int64_t n_obj = rd.idx();
    for (int64_t i = 0; i < n_obj && !rd.bad; i++)
    {
        int64_t kind = rd.idx();
        void* p = NULL;
        bool bs = false;
        ULONG dim = 0;
        switch (kind)
        {
        case K_CURVE: p = read_curve(rd, ctx, bs, dim); break;
        case K_PLANE:
        {
            SmPoint3d org = rd.vec();
            SmVector3d nrm = rd.vec();
            p = new (ctx) SmPlane(org, nrm, &ctx);
            break;
        }
        case K_BSURF:
        {
            bool oob = rd.flag();
            SmBSplineSurface* s = new (ctx) SmBSplineSurface(read_nurb_surface(rd), FALSE, NULL);
            s->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
            p = s;
            break;
        }
        case K_POINT: p = new SmPoint3d(rd.vec()); break;
        case K_VERTEX: p = new (anchor()) ScVertex(); break;
        case K_EDGE:
        {
            int64_t c = rd.idx();
            bool ok = true;
            SmCurve* co = (SmCurve*)pool.get(c, {K_CURVE}, ok);
            if (!ok)
                return SC_ERR_ARG;
            p = new (anchor()) ScEdge(co);
            break;
        }
        case K_FACE:
        {
            int64_t s = rd.idx();
            bool ok = true;
            SmSurface* so = (SmSurface*)pool.get(s, {K_PLANE, K_BSURF}, ok);
            if (!ok)
                return SC_ERR_ARG;
            p = new (anchor()) ScFace(so);
            break;
        }
        case K_LOOP: p = new (&ctx) ScStandIn(SmLoop_TYPE); break;
        case K_OTHER: p = new (&ctx) ScStandIn(SmObject_TYPE + 7); break;
        default: return SC_ERR_ARG;
        }
        pool.ids[p] = (int64_t)pool.ptrs.size();
        pool.kinds.push_back(kind);
        pool.ptrs.push_back(p);
        pool.bspline.push_back(bs);
        pool.dims.push_back(dim);
    }
    if (rd.bad)
        return SC_ERR_ARG;

    SmCurveClassification ccs[2];
    // the pool's curves are never deleted: clear the ownership flags before the
    // classifications are destroyed (on every return)
    struct Disown
    {
        SmCurveClassification* c;
        ~Disown()
        {
            c[0].*get(CcOwned()) = FALSE;
            c[1].*get(CcOwned()) = FALSE;
        }
    } disown{ccs};

    int64_t n_ops = rd.idx();
    for (int64_t i = 0; i < n_ops && !rd.bad; i++)
    {
        int64_t op = rd.idx();
        int64_t t = rd.idx();
        if (t < 0 || t > 1)
            return SC_ERR_ARG;
        SmCurveClassification& cc = ccs[t];
        SmCurveClassification& other = ccs[1 - t];
        int64_t size = (int64_t)cc.GetSize();
        bool ok = true;
        o.push_back((double)op);
        switch (op)
        {
        case OP_SET_SIZE:
        {
            int64_t n = rd.idx();
            if (n < 0 || n > 64)
                return SC_ERR_ARG;
            cc.SetSize((ULONG)n);
            o.push_back(0.0);
            break;
        }
        case OP_REMOVE_AT:
        {
            int64_t j = rd.idx();
            if (size == 0 || j < 0 || j > 64)
                return SC_ERR_ARG;
            cc.RemoveAt((ULONG)j);
            o.push_back(0.0);
            break;
        }
        case OP_IVL_SET:
        case OP_IVL_SET_MIN:
        case OP_IVL_SET_MAX:
        case OP_IVL_RESET:
        case OP_IVL_ASSIGN:
        case OP_IVL_NEW:
        {
            int64_t j = rd.idx();
            if (j < 0 || j >= size)
                return SC_ERR_ARG;
            SmCurveInterval& ivl = cc.GetAt((ULONG)j);
            if (op == OP_IVL_SET)
            {
                double a = rd.num();
                double b = rd.num();
                ivl.SetInterval(SmExtent1d(a, b));
                o.push_back(0.0);
            }
            else if (op == OP_IVL_SET_MIN)
                o.push_back(st((ivl.*get(IvlSetMin()))(rd.num())));
            else if (op == OP_IVL_SET_MAX)
                o.push_back(st((ivl.*get(IvlSetMax()))(rd.num())));
            else if (op == OP_IVL_RESET)
            {
                ivl.ReSet();
                o.push_back(0.0);
            }
            else if (op == OP_IVL_ASSIGN)
            {
                int64_t k = rd.idx();
                if (k < 0 || k >= (int64_t)other.GetSize())
                    return SC_ERR_ARG;
                ivl = other.GetAt((ULONG)k);
                o.push_back(0.0);
            }
            else
            {
                bool with = rd.flag();
                bool owner = rd.flag();
                double a = rd.num();
                double b = rd.num();
                const SmCurveClassification* po = owner ? &cc : NULL;
                if (with)
                    ivl = SmCurveInterval(SmExtent1d(a, b), &ctx, po);
                else
                    ivl = SmCurveInterval(&ctx, po);
                o.push_back(0.0);
            }
            break;
        }
        case OP_PC_SET_CLASS:
        case OP_PC_SET_T:
        case OP_PC_SET_UV:
        case OP_PC_SET_GAP:
        case OP_PC_SET_SSS:
        case OP_PC_SET_PRESNAP:
        case OP_PC_SET_SRC_TOL:
        {
            int64_t j = rd.idx();
            int64_t w = rd.idx();
            if (j < 0 || j >= size || w < 0 || w > 2)
                return SC_ERR_ARG;
            SmPointClassification& pc = *pick(cc.GetAt((ULONG)j), w);
            if (op == OP_PC_SET_CLASS)
            {
                int64_t c = rd.idx();
                int64_t obj = rd.idx();
                int64_t want = kind_of_class(c);
                if (want == -2)
                    return SC_ERR_ARG;
                void* po = pool.get(obj, {}, ok);
                if (!ok)
                    return SC_ERR_ARG;
                if (obj != -1)
                {
                    int64_t k = pool.kinds[(size_t)obj];
                    if (!(k == want || (want == K_BSURF && k == K_PLANE)))
                        return SC_ERR_ARG;
                }
                pc.SetClassObject((SmPointClassificationType)c, (SmObject*)po);
            }
            else if (op == OP_PC_SET_T)
                pc.SetTParam(rd.num());
            else if (op == OP_PC_SET_UV)
            {
                double u = rd.num();
                double v = rd.num();
                pc.SetUVParam(SmPoint2d(u, v));
            }
            else if (op == OP_PC_SET_GAP)
                pc.SetGap3d(rd.num());
            else if (op == OP_PC_SET_SSS)
                pc.SetSSSPoint(rd.vec());
            else if (op == OP_PC_SET_PRESNAP)
                pc.SetPreSnapParam(rd.num());
            else
                pc.SetSrcZoneTol3d(SmZoneTol3d(rd.num()));
            o.push_back(0.0);
            break;
        }
        case OP_SET_CURVE:
        {
            int64_t obj = rd.idx();
            bool owned = rd.flag();
            SmCurve* c = (SmCurve*)pool.get(obj, {K_CURVE}, ok);
            if (!ok)
                return SC_ERR_ARG;
            // SetCurve deletes an owned previous curve: the pool's curves are never deleted,
            // so the previous one is disowned first (the flag is data here)
            cc.*get(CcOwned()) = FALSE;
            cc.SetCurve(c, owned ? TRUE : FALSE);
            o.push_back(0.0);
            break;
        }
        case OP_SET_SURFACE1:
        case OP_SET_SURFACE2:
        {
            SmSurface* s = (SmSurface*)pool.get(rd.idx(), {K_PLANE, K_BSURF}, ok);
            if (!ok)
                return SC_ERR_ARG;
            if (op == OP_SET_SURFACE1)
                cc.SetSurface1(s);
            else
                cc.SetSurface2(s);
            o.push_back(0.0);
            break;
        }
        case OP_SET_FACE:
        {
            SmFace* f = (SmFace*)pool.get(rd.idx(), {K_FACE}, ok);
            if (!ok)
                return SC_ERR_ARG;
            cc.SetFace(f);
            o.push_back(0.0);
            break;
        }
        case OP_SET_INTERSECT_DATA:
        {
            SmSurface* s1 = (SmSurface*)pool.get(rd.idx(), {K_PLANE, K_BSURF}, ok);
            SmSurface* s2 = (SmSurface*)pool.get(rd.idx(), {K_PLANE, K_BSURF}, ok);
            SmCurve* uv2 = (SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
            bool cur = rd.flag();
            void* ti = pool.get(rd.idx(), {K_OTHER}, ok);
            if (!ok)
                return SC_ERR_ARG;
            cc.SetIntersectData(s1, s2, uv2, cur ? TRUE : FALSE, (const SmTopologyIntersector*)ti);
            o.push_back(0.0);
            break;
        }
        case OP_SET_IS_CURVE_CURRENT:
            cc.SetIsCurveCurrent(rd.flag() ? TRUE : FALSE);
            o.push_back(0.0);
            break;
        case OP_SET_IGNORE_TANGENCY:
            cc.SetIgnoreTangency(rd.flag() ? TRUE : FALSE);
            o.push_back(0.0);
            break;
        case OP_SET_IS_CURVE_OWNED:
            // the flag only: the pool's curves are never deleted (see OP_SET_CURVE)
            cc.SetIsCurveOwned(rd.flag() ? TRUE : FALSE);
            o.push_back(0.0);
            break;
        case OP_SET_OK_TO_SET_TOL:
            cc.SetOKToSetTolerance(rd.flag() ? TRUE : FALSE);
            o.push_back(0.0);
            break;
        case OP_SET_FILLET_DATA:
        {
            void* oc = pool.get(rd.idx(), {K_OTHER}, ok);
            bool b = rd.flag();
            if (!ok)
                return SC_ERR_ARG;
            cc.SetFilletData((SmCurveClassification*)oc, b ? TRUE : FALSE);
            o.push_back(0.0);
            break;
        }
        case OP_SET_TGT_LOOPS_ONLY:
            cc.SetTgtLoopsOnly(rd.flag() ? TRUE : FALSE);
            o.push_back(0.0);
            break;
        case OP_ADD_TGT_LOOP:
        {
            int64_t j = rd.idx();
            void* l = pool.get(j, {K_LOOP}, ok);
            if (!ok || j == -1)
                return SC_ERR_ARG;
            cc.AddTgtLoop((SmLoop*)l);
            o.push_back(0.0);
            break;
        }
        case OP_SET_SRC_TOL:
            SmTol::SetZoneTol3d(&cc, SmZoneTol3d(rd.num()));
            o.push_back(0.0);
            break;
        case OP_SET_UV_CURVE1:
        case OP_SET_UV_CURVE2:
        {
            SmCurve* c = (SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
            if (!ok)
                return SC_ERR_ARG;
            if (op == OP_SET_UV_CURVE1)
                cc.SetUVCurve1(c);
            else
                cc.SetUVCurve2(c);
            o.push_back(0.0);
            break;
        }
        case OP_SUB_INTERVAL:
        {
            int64_t j = rd.idx();
            if (j < 0 || j >= size)
                return SC_ERR_ARG;
            SmExtent1d e = cc.GetSubInterval((ULONG)j);
            o.push_back(0.0);
            o.push_back(e.GetMin());
            o.push_back(e.GetMax());
            break;
        }
        case OP_CLASSIFY_CRV_LOC:
        {
            int64_t j = rd.idx();
            int64_t w = rd.idx();
            double prm = rd.num();
            if (j < 0 || j >= size || w < 0 || w > 2)
                return SC_ERR_ARG;
            SmPointClassification* pc = pick(cc.GetAt((ULONG)j), w);
            if (!pc->GetSSSPointFlag() && cc.GetCurve() == NULL)
                return SC_ERR_ARG;
            SmPoint3d p = cc.GetClassifyCrvLoc(pc, prm);
            o.push_back(0.0);
            put_vec(o, p);
            break;
        }
        case OP_SET_PC_PARAMS:
        {
            bool mids = rd.flag();
            if (size > 0 && cc.GetCurve() == NULL)
                return SC_ERR_ARG;
            for (int64_t j = 0; j < size; j++)
            {
                SmCurveInterval& ivl = cc.GetAt((ULONG)j);
                if (!computable(ivl.m_vStart, pool) || !computable(ivl.m_vEnd, pool) ||
                    (mids && !computable(ivl.m_vMid, pool)))
                    return SC_ERR_ARG;
            }
            o.push_back(st(cc.SetPointClassParameters(mids ? TRUE : FALSE)));
            break;
        }
        case OP_SQUEEZE_END:
        {
            int64_t j = rd.idx();
            if (j < 0 || j >= size)
                return SC_ERR_ARG;
            SmIntervalPosition pos = cc.FindBestSqueezeEnd((ULONG)j);
            o.push_back(0.0);
            o.push_back((double)pos);
            break;
        }
        case OP_IS_REFERENCED:
        {
            void* po = pool.get(rd.idx(), {}, ok);
            if (!ok)
                return SC_ERR_ARG;
            o.push_back(0.0);
            o.push_back(flg(cc.IsReferenced((const SmObject*)po)));
            break;
        }
        case OP_MAKE_CONSISTENT:
        {
            // the stand-in edges have no vertices: SmEdge::GetVertex would crash
            for (int64_t j = 0; j < size; j++)
            {
                SmCurveInterval& ivl = cc.GetAt((ULONG)j);
                if (ivl.m_vMid.GetPointClass() == SM_PC_EDGE &&
                    (ivl.m_vStart.GetPointClass() == SM_PC_VERTEX || ivl.m_vEnd.GetPointClass() == SM_PC_VERTEX))
                    return SC_ERR_ARG;
            }
            o.push_back(st(sc_cc_p1::sm_MakeConsistentClassifications(cc)));
            break;
        }
        case OP_TYPE:
        {
            int64_t n = rd.idx();
            if (n < 0 || n > 8)
                return SC_ERR_ARG;
            std::vector<SM_TYPE> types;
            for (int64_t k = 0; k < n; k++)
                types.push_back((SM_TYPE)rd.idx());
            o.push_back(0.0);
            o.push_back((double)cc.GetType());
            for (SM_TYPE ty : types)
                o.push_back(flg(cc.IsKindOf(ty)));
            const TCHAR* str = cc.GetTypeString();
            std::vector<double> cs;
            for (const TCHAR* c = str; *c; c++)
                cs.push_back((double)*c);
            o.push_back((double)cs.size());
            o.insert(o.end(), cs.begin(), cs.end());
            break;
        }
        case OP_SRC_TOL:
            o.push_back(0.0);
            o.push_back(SmTol::GetSrcZoneTol3d(&cc));
            o.push_back(SmTol::GetSrcZoneTol3d((const SmCurveClassification*)NULL));
            break;
        case OP_IVL_TOLS:
        {
            int64_t j = rd.idx();
            if (j < 0 || j >= size)
                return SC_ERR_ARG;
            const SmCurveInterval& ivl = cc.GetAt((ULONG)j);
            int64_t k = ivl.m_vMid.GetObject() == NULL ? -1 : pool.kind_of(ivl.m_vMid.GetObject());
            if (k == K_POINT)
                return SC_ERR_ARG;
            // SmTol::GetSrcZoneTol3d(ivl) reads the interval's back pointer (NULL: crash). The
            // port has no back pointer and takes the owner explicitly (this classification):
            // compare only intervals owned by this classification.
            if (ivl.GetCurveClassification() != &cc)
                return SC_ERR_ARG;
            o.push_back(0.0);
            o.push_back(SmTol::GetSrcZoneTol3d(&ivl));
            o.push_back(SmTol::GetObjZoneTol3d(&ivl));
            o.push_back(SmTol::GetObjZoneTol3d((const SmCurveInterval*)NULL));
            o.push_back(SmTol::GetScaledZero(ivl));
            break;
        }
        case OP_OBJ_DIM:
        {
            void* po = pool.get(rd.idx(), {}, ok);
            if (!ok || (po != NULL && pool.kind_of(po) == K_POINT))
                return SC_ERR_ARG;
            o.push_back(0.0);
            o.push_back((double)sc_cc_p1::sm_ObjectDimension((SmObject*)po));
            break;
        }
        case OP_XSECT_CURVE:
        {
            int64_t cobj = rd.idx();
            SmCurve* c = (SmCurve*)pool.get(cobj, {K_CURVE}, ok);
            double tol = rd.num();
            SmTArray<SmBSplineCurve*> uvc;
            int64_t n_uvc = rd.idx();
            for (int64_t k = 0; k < n_uvc && k < 8; k++)
            {
                int64_t j = rd.idx();
                SmCurve* u = (SmCurve*)pool.get(j, {K_CURVE}, ok);
                if (j == -1 || !ok || !pool.bspline[(size_t)j] || pool.dims[(size_t)j] != 2)
                    return SC_ERR_ARG;
                uvc.Add((SmBSplineCurve*)u);
            }
            SmTArray<const SmSurface*> srf;
            int64_t n_srf = rd.idx();
            for (int64_t k = 0; k < n_srf && k < 8; k++)
            {
                int64_t j = rd.idx();
                const SmSurface* s = (const SmSurface*)pool.get(j, {K_PLANE, K_BSURF}, ok);
                if (j == -1)
                    return SC_ERR_ARG;
                srf.Add(s);
            }
            int64_t n_uv = rd.idx();
            std::vector<SmVector2d> uvs;
            for (int64_t k = 0; k < n_uv && k < 8; k++)
            {
                double u = rd.num();
                double v = rd.num();
                uvs.push_back(SmVector2d(u, v));
            }
            double prm = rd.num();
            double dev = rd.num();
            SmPoint3d p3 = rd.vec();
            if (!ok || c == NULL || n_uvc > 8 || n_srf > 8 || n_uv > 8 || rd.bad)
                return SC_ERR_ARG;
            // the first two UV points are read whenever there are two surfaces
            if (n_srf >= 2 && n_uv < 2)
                return SC_ERR_ARG;
            SmTArray<SmVector2d*> uvp;
            for (auto& u : uvs)
                uvp.Add(&u);
            SmBoolean mod = FALSE;
            SmBoolean set3 = FALSE;
            SmStatus s = sc_cc_p1::sm_SetXSectCurveThroughPoint(c, SmZoneTol3d(tol), uvc, srf, uvp, mod, prm, dev, set3, &p3);
            o.push_back(st(s));
            o.push_back(flg(mod));
            o.push_back(prm);
            o.push_back(dev);
            o.push_back(flg(set3));
            put_vec(o, p3);
            put_curve(o, c);
            for (ULONG k = 0; k < uvc.GetSize(); k++)
                put_curve(o, uvc[k]);
            break;
        }
        case OP_APPLY:
        {
            bool use_cc = rd.flag();
            void* edge = pool.get(rd.idx(), {K_EDGE}, ok);
            void* emate = pool.get(rd.idx(), {K_EDGE}, ok);
            const SmSurface* srf[4];
            SmPoint2d uvs[4];
            for (int k = 0; k < 4; k++)
            {
                srf[k] = (const SmSurface*)pool.get(rd.idx(), {K_PLANE, K_BSURF}, ok);
                double u = rd.num();
                double v = rd.num();
                uvs[k].Set(u, v);
            }
            SmSolution sol;
            sol.m_eSolutionType = SM_ST_SINGLE_VALUE;
            sol.m_vStart[0] = rd.num();
            sol.m_vStart[1] = rd.num();
            sol.m_vStart.m_dSolutionValue = rd.num();
            SmPoint3d sss = rd.vec();
            if (!ok || rd.bad)
                return SC_ERR_ARG;
            SmCurveClassification* pcc = use_cc ? &cc : NULL;
            bool src = pcc != NULL && pcc->GetCurve() != NULL;
            bool edge_eval = (edge != NULL || emate != NULL) && srf[2] != NULL;
            // the classification curve's two surfaces are dereferenced together
            if (src && ((srf[0] == NULL) != (srf[1] == NULL)))
                return SC_ERR_ARG;
            if (src && srf[0] == NULL && edge_eval)
                return SC_ERR_ARG;
            // the edge path walks topology the stand-in edges do not have
            if (edge != NULL)
                return SC_ERR_ARG;
            SmBoolean flag = FALSE, mc = FALSE, me = FALSE, mm = FALSE;
            double dev = 0.0;
            SmStatus s = sc_cc_p1::sm_ApplyPointSolution(pcc, (SmEdge*)edge, (SmEdge*)emate, srf[0], &uvs[0], srf[1],
                                                         &uvs[1], srf[2], &uvs[2], srf[3], &uvs[3], sol, sss, flag, mc,
                                                         me, mm, dev);
            o.push_back(st(s));
            put_vec(o, sss);
            o.push_back(flg(flag));
            o.push_back(flg(mc));
            o.push_back(flg(me));
            o.push_back(flg(mm));
            o.push_back(dev);
            o.push_back(sol.m_vStart[0]);
            o.push_back(sol.m_vStart[1]);
            if (src)
                put_curve(o, pcc->GetCurve());
            break;
        }
        case OP_ANGLE:
        {
            const SmCurve* c1 = (const SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
            double t1 = rd.num();
            const SmCurve* c2 = (const SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
            double t2 = rd.num();
            double dev = rd.num();
            if (!ok)
                return SC_ERR_ARG;
            o.push_back(0.0);
            o.push_back(sc_cc_p1::sm_GetCurveCurveAngle(c1, t1, c2, t2, dev));
            break;
        }
        case OP_CULL:
        {
            int64_t sj = rd.idx();
            const SmSurface* s = (const SmSurface*)pool.get(sj, {K_PLANE, K_BSURF}, ok);
            int64_t uj = rd.idx();
            const SmCurve* uvc = (const SmCurve*)pool.get(uj, {K_CURVE}, ok);
            double a0 = rd.num();
            double a1 = rd.num();
            const SmCurve* c2 = (const SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
            double b0 = rd.num();
            double b1 = rd.num();
            double tol = rd.num();
            int64_t n = rd.idx();
            if (!ok || s == NULL || uvc == NULL || c2 == NULL || n < 0 || n > 16 || pool.dims[(size_t)uj] != 2)
                return SC_ERR_ARG;
            SmSolutionArray sols;
            for (int64_t k = 0; k < n; k++)
            {
                SmSolution sol;
                sol.m_eSolutionType = (SmSolutionType)rd.idx();
                sol.m_vStart[0] = rd.num();
                sol.m_vStart[1] = rd.num();
                sol.m_vEnd[0] = rd.num();
                sol.m_vEnd[1] = rd.num();
                sol.m_vStart.m_dSolutionValue = rd.num();
                sols.Add(sol);
            }
            if (rd.bad)
                return SC_ERR_ARG;
            SmStatus r = sc_cc_p1::sm_CullOrRefineCCIIn3d(*s, *uvc, SmExtent1d(a0, a1), *c2, SmExtent1d(b0, b1), tol, sols);
            o.push_back(st(r));
            o.push_back((double)sols.GetSize());
            for (ULONG k = 0; k < sols.GetSize(); k++)
            {
                const SmSolution& x = sols[k];
                o.push_back((double)x.m_eSolutionType);
                o.push_back(x.m_vStart[0]);
                o.push_back(x.m_vStart[1]);
                o.push_back(x.m_vEnd[0]);
                o.push_back(x.m_vEnd[1]);
                o.push_back(x.m_vStart.m_dSolutionValue);
            }
            break;
        }
        case OP_TOL_PAIR:
        {
            const SmCurve* c = (const SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
            const SmSurface* s = (const SmSurface*)pool.get(rd.idx(), {K_PLANE, K_BSURF}, ok);
            double p1 = rd.num();
            double t1 = rd.num();
            double p2 = rd.num();
            double t2 = rd.num();
            if (!ok || c == NULL)
                return SC_ERR_ARG;
            o.push_back(0.0);
            o.push_back(sm_GetXSectTol2dOr3d_ForClassifyPair(c, s, p1, SmZoneTol3d(t1), p2, SmZoneTol3d(t2)));
            break;
        }
        case OP_GAPS:
        {
            SmPoint3d cp = rd.vec();
            SmPoint3d ep = rd.vec();
            SmSolution sol;
            sol.m_vStart[0] = rd.num();
            sol.m_vStart[1] = rd.num();
            int64_t n = rd.idx();
            if (n < 0 || n > 8)
                return SC_ERR_ARG;
            SmTArray<const SmSurface*> srf;
            SmTArray<const SmCurve*> uvc;
            for (int64_t k = 0; k < n; k++)
            {
                const SmSurface* s = (const SmSurface*)pool.get(rd.idx(), {K_PLANE, K_BSURF}, ok);
                const SmCurve* u = (const SmCurve*)pool.get(rd.idx(), {K_CURVE}, ok);
                if (s == NULL)
                    return SC_ERR_ARG;
                srf.Add(s);
                uvc.Add(u);
            }
            ULONG cnt = (ULONG)rd.idx();
            if (!ok || rd.bad)
                return SC_ERR_ARG;
            double g1 = -7.0, g2 = -7.0, g3 = -7.0;
            SmTArray<SmPoint2d> uvs;
            SmStatus r = sc_cc_p1::sm_GetCCISurfaceGaps(cp, ep, sol, srf, uvc, cnt, g1, g2, g3, uvs);
            o.push_back(st(r));
            o.push_back(g1);
            o.push_back(g2);
            o.push_back(g3);
            o.push_back((double)uvs.GetSize());
            for (ULONG k = 0; k < uvs.GetSize(); k++)
            {
                o.push_back(uvs[k].x);
                o.push_back(uvs[k].y);
            }
            break;
        }
        case OP_TOUCH_CONNECTED:
        case OP_TOUCH_WINDING:
        {
            int64_t c = op == OP_TOUCH_CONNECTED ? rd.idx() : -1;
            if (c != -1 && !pool.valid(c))
                return SC_ERR_ARG;
            SmTArray<SmTouchData> in;
            if (!read_touches(rd, pool, in))
                return SC_ERR_ARG;
            o.push_back(0.0);
            if (op == OP_TOUCH_CONNECTED)
            {
                SmTArray<SmTouchData> out;
                ULONG cnt = SmTouchData::GetConnectedTouches(c == -1 ? NULL : (SmTopology*)pool.ptrs[(size_t)c], in, &out);
                ULONG cnt2 = SmTouchData::GetConnectedTouches(c == -1 ? NULL : (SmTopology*)pool.ptrs[(size_t)c], in);
                o.push_back((double)cnt);
                o.push_back((double)cnt2);
                for (ULONG k = 0; k < out.GetSize(); k++)
                    put_touch(o, out[k], pool);
            }
            else
            {
                if (in.GetSize() % 2 != 0)
                    return SC_ERR_ARG;
                SmOrientType ot = SM_OT_UNKNOWN;
                ULONG w = SmTouchData::GetWindingCount(in, ot);
                o.push_back((double)w);
                o.push_back((double)ot);
            }
            break;
        }
        case OP_EDGE_THROUGH:
        {
            // deferred in the port (the edge's topology); run for the record only
            SmEdge* e = (SmEdge*)pool.get(rd.idx(), {K_EDGE}, ok);
            SmPoint3d p = rd.vec();
            SmTArray<const SmSurface*> srf;
            SmTArray<SmVector2d*> uvp;
            std::vector<SmVector2d> uvs(2);
            for (int k = 0; k < 2; k++)
            {
                srf.Add((const SmSurface*)pool.get(rd.idx(), {K_PLANE, K_BSURF}, ok));
                double u = rd.num();
                double v = rd.num();
                uvs[(size_t)k] = SmVector2d(u, v);
            }
            for (auto& u : uvs)
                uvp.Add(&u);
            double prm = rd.num();
            if (!ok || e == NULL || srf[0] == NULL || srf[1] == NULL || e->GetCurve() == NULL)
                return SC_ERR_ARG;
            double dev = -7.0;
            SmBoolean mod = FALSE;
            SmStatus s = sc_cc_p1::sm_SetXSectEdgeThroughPoint(e, &p, srf, uvp, prm, dev, mod);
            o.push_back(st(s));
            o.push_back(prm);
            o.push_back(dev);
            o.push_back(flg(mod));
            break;
        }
        case OP_CCI_GEOMETRY:
        {
            SmEdge* e = (SmEdge*)pool.get(rd.idx(), {K_EDGE}, ok);
            if (!ok || e == NULL)
                return SC_ERR_ARG;
            SmSolution sol;
            SmEdge* mate = NULL;
            SmTArray<const SmSurface*> srf;
            SmTArray<SmEdgeuse*> eus;
            SmTArray<const SmCurve*> uvc;
            ULONG cnt = 0;
            SmStatus s = sc_cc_p1::sm_GetCCIGeometry(&cc, sol, e, mate, srf, eus, uvc, cnt);
            o.push_back(st(s));
            o.push_back((double)srf.GetSize());
            o.push_back((double)cnt);
            break;
        }
        case OP_FACE_POINT_CLASS:
        {
            SmFace* f = (SmFace*)pool.get(rd.idx(), {K_FACE}, ok);
            SmPoint3d p = rd.vec();
            double tol = rd.num();
            if (!ok || f == NULL || f->GetSurface() == NULL)
                return SC_ERR_ARG;
            SmPointClassification pc;
            SmStatus s = sc_cc_p1::sm_ComputeFacePointClass(p, SmZoneTol3d(tol), f, pc);
            o.push_back(st(s));
            o.push_back((double)pc.GetPointClass());
            break;
        }
        default: return SC_ERR_ARG;
        }
        if (rd.bad)
            return SC_ERR_ARG;
        put_cc(o, cc, pool);
    }
    if (rd.bad || rd.at != rd.n)
        return SC_ERR_ARG;
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_intersect_curve_class_p1_run(const double* in, int64_t n_in, double* out, int64_t cap,
                                                  int64_t* out_len)
{
    if (in == NULL || n_in < 0 || out_len == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, n_in, 0};
    std::vector<double> o;
    int32_t s = run(rd, o);
    if (s != SC_OK)
    {
        *out_len = 0;
        return s;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
