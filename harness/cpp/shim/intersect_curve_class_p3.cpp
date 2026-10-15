// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `intersect.curve_class_p3`: the part-3 members of SmCurveClassification
// (FindInterval, FindPointClass, IsCoincident, InsertPointClass, RemovePointClass,
// MergePointClass, CleanupAndValidate) and the static functions sm_ChangeEdgePointClassToNearVertex
// and sm_FixProblemsBySqueezing of source/SMLib/src/SmCurveClass.cpp.
//
// The two static functions cannot be linked against, so they are VERBATIM copies of upstream
// SmCurveClass.cpp (commit c9c5979) inside the namespace sc_cc_p3 (line ranges noted at each
// block; generated, do not edit by hand).
//
// One entry point, sc_intersect_curve_class_p3_run(in, n_in, out, cap, out_len): a script over a
// pool of objects and two default-constructed classifications cc[0], cc[1] (the setup ops are
// those of unit intersect.curve_class_p1; the layout is documented in
// crates/solidcore-ffi/src/units/intersect_curve_class_p3.rs, which validates the static shape).
// The run-time checks below reject (SC_ERR_ARG) the ops that are undefined behaviour upstream:
// an empty classification for FindInterval / IsCoincident / InsertPointClass / the squeeze
// (m_pData[0]), a NULL curve where the call would evaluate it, an IsCoincident walk past the
// intervals, point objects in the zone tolerance lookups, the topology walks over the bare
// stand-in edges. Objects live for the whole process (never deleted: topology destructors notify
// owners).

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

// global in the library (SmCurveClass.cpp), not declared in a header
SmXSectTol3d sm_GetXSectTol2dOr3d_ForClassifyPair(const SmCurve* cpSrcCurve, const SmSurface* cpSurface1,
                                                  double dSrcCurveOrObj1_Param,
                                                  SmZoneTol3d sSrcCurveOrObj1_ZoneTol3d,
                                                  double dObj2_SrcCurveParam, SmZoneTol3d sObj2_ZoneTol3d);

namespace sc_cc_p3
{
// clang-format off
// ---- verbatim: SmCurveClass.cpp lines 3299-3542: sm_ChangeEdgePointClassToNearVertex
static SmStatus sm_ChangeEdgePointClassToNearVertex
  (ULONG                   lPointClassIndex,      // in : target point classification index
   SmCurveClassification & rCurveClass,           // in : CurveClassification to review
   const SmPoint3d       & crClassifyPoint,       // in : Point Location for classification
   SmBoolean             & rbChangedToVertexFlag) // out: TRUE  = Point classification moved from edge to edge's vertex location
                                                  //      FALSE = No Changes Made.
{
  // init output
  rbChangedToVertexFlag = FALSE;

  // locals
  SmPointClassification * pPC = rCurveClass.GetPointClassByIndex(lPointClassIndex) ;

  // no work - pPC does not classify to an edge
  if (pPC->GetPointClass() != SM_PC_EDGE) 
    { return SM_SUCCESS ; }

  // more locals
  // double      dT            = pPC->GetTParam();
  SmEdge     * pEdge          = (SmEdge*)pPC->GetObject();
  SmExtent1d   sEdgeIvl       = pEdge->GetInterval();
  SmVertex   * pEdgeVertex1   = pEdge->GetVertex();
  SmVertex   * pEdgeVertex2   = pEdge->GetOtherVertex(pEdgeVertex1);
  double       dDist1         = pEdgeVertex1->GetPoint().DistanceBetween(crClassifyPoint);
  double       dDist2         = pEdgeVertex2->GetPoint().DistanceBetween(crClassifyPoint);

  // We should not snap to vertices that are not within tolerances.  [B9]
  SmZoneTol3d  sEdgeZoneTol3d = SmTol::GetObjZoneTol3d(pPC) ;
  SmZoneTol3d  sSrcZoneTol3d  = SmTol::GetSrcZoneTol3d(pPC) ;

  SM_ASSERT_BREAK_MSG(rCurveClass.GetCurve()->GetDim() == 3, _T("Bad CurveClass 3d Assumption switch call to sm_GetXSectTol2dOr3d_ForClassifyPair()")) ;
  SmXSectTol3d sXSectTol3d1   = SmTol::GetXSectTol3d(sSrcZoneTol3d, SmTol::GetZoneTol3d(pEdgeVertex1)) ; 
  SmXSectTol3d sXSectTol3d2   = SmTol::GetXSectTol3d(sSrcZoneTol3d, SmTol::GetZoneTol3d(pEdgeVertex2)) ; 

  // when target_point is closer to pEdgeVertex1 and within tolerance
  if(   dDist1 < dDist2          //     pEdgeVertex1 is closer to ClassifyPt than pEdgeVertex2
     && dDist1 < sXSectTol3d1 )  // and within XSectTol3d of the ClassifyPoint (GWC: I would think this never fires - the other classify code
                                 //                                                  looks for vertices within XSectTol3d and uses them.
                                 //                                                  I'm guessing if the code runs down this path, then
                                 //                                                  there's a tolerance bug elsewhere. 
    {
      // inform the public that the classification was modified
      MSG(_T("ChangeEdgePointClassToNearVertex: Switching PointClassification from EDGE to EdgeVertex within XSectTol3d of the ClassifyPt")) ;
      rbChangedToVertexFlag = TRUE;

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe=FALSE;
      if (bDebugMe)
        {
          SM_DUMP_AND_ASSERT_VALID(&rCurveClass) ;

          SmBrep        *pBrep  = rCurveClass.GetBrep() ;
          const SmCurve *pCurve = rCurveClass.GetCurve() ;

          smgfx_Erase() ;
          smgfx_SetLook(1,2, 0,0,1) ; if(pBrep) { pBrep->Draw(TRUE) ; } sm_GraphicsLoop() ;
          smgfx_SetLook(5,7, 1,0,0) ; crClassifyPoint.Draw(); sm_GraphicsLoop();
          smgfx_SetLook(3,4, 0,1,1) ;
          SmExtent1d range = rCurveClass.GetInterval();
          pCurve->Draw(&range) ; sm_GraphicsLoop() ;
          smgfx_SetLook(3,4, 1,0,1) ; pEdge->Draw() ; sm_GraphicsLoop() ;
          smgfx_SetLook(7,8, 0,1,.5); pEdgeVertex1->Draw() ; sm_GraphicsLoop() ;
          sm_GraphicsLoop();
        }
#endif // SM_DEBUG_CODE

      // set pPC and its mate's PointClassification to classify to the pEdgeVertex1 vertex
      pPC->SetClassObject( SM_PC_VERTEX, pEdgeVertex1 );
      pPC->SetGap3d( dDist1 ) ;

      SmPointClassification  *pPCMate = rCurveClass.GetPointClassMateByIndex( lPointClassIndex );
      if ( pPCMate != NULL ) *pPCMate = *pPC;

    } // end target_point close enough to pEdgeVertex1 check

  // when target_point is closer to pEdgeVertex2 and within tolerance
  else if(   dDist2 <= dDist1         //     pEdgeVertex2 is closer to ClassifyPt than pEdgeVertex1
          && dDist2 < sXSectTol3d2 )  // and within XSectTol3d of the ClassifyPoint (GWC: I would think this never fires - the other classify code
    {                                 //                                                  looks for vertices within XSectTol3d and uses them.
                                      //                                                  I'm guessing if the code runs down this path, then
                                      //                                                  there's a tolerance bug elsewhere. 

      // inform the public that the classification was modified
      MSG(_T("ChangeEdgePointClassToNearVertex: Switching PointClassification from EDGE to EdgeVertex within XSectTol3d of the ClassifyPt")) ;
      rbChangedToVertexFlag = TRUE;

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe=FALSE;
      if (bDebugMe)
        {
          SM_DUMP_AND_ASSERT_VALID(&rCurveClass) ;

          SmBrep        *pBrep  = rCurveClass.GetBrep() ;
          const SmCurve *pCurve = rCurveClass.GetCurve() ;

          smgfx_Erase() ;
          smgfx_SetLook(1,2, 0,0,1) ; if(pBrep) { pBrep->Draw(TRUE) ; } sm_GraphicsLoop() ;
          smgfx_SetLook(5,7, 1,0,0) ; crClassifyPoint.Draw(); sm_GraphicsLoop();
          smgfx_SetLook(3,4, 0,1,1) ;
          SmExtent1d range = rCurveClass.GetInterval();
          pCurve->Draw(&range) ; sm_GraphicsLoop() ;
          smgfx_SetLook(3,4, 1,0,1) ; pEdge->Draw() ; sm_GraphicsLoop() ;
          smgfx_SetLook(7,8, 0,1,.5); pEdgeVertex2->Draw() ; sm_GraphicsLoop() ;
          sm_GraphicsLoop();
        }
#endif // SM_DEBUG_CODE

      // set pPC and its mate's PointClassification to classify to the pEdgeVertex2 vertex
      pPC->SetClassObject( SM_PC_VERTEX, pEdgeVertex2 );
      pPC->SetGap3d( dDist2 );

      SmPointClassification  *pPCMate = rCurveClass.GetPointClassMateByIndex( lPointClassIndex );
      if ( pPCMate != NULL ) *pPCMate = *pPC;

    }  // end target_point close enough to pEdgeVertex2 check

  // Check to see if change created a short interval with
  //   both ends classified to the same vertex
  //   remember to check intervals on both sides of this point classification
  if(rbChangedToVertexFlag)
    {
      // locals - look for Vertex objects on the other ends of adjacent intervals
      SmPointClassification *pPCLast =   lPointClassIndex > 0
                                       ? rCurveClass.GetPointClassByIndex(lPointClassIndex - 1)
                                       : NULL ;
      SmPointClassification *pPCNext =   lPointClassIndex < rCurveClass.GetSize()
                                       ? rCurveClass.GetPointClassByIndex(lPointClassIndex + 1)
                                       : NULL ;
      SmVertex *pVertex     =   (SmVertex*)pPC->GetObject() ;
      SmVertex *pVertexLast =   (pPCLast && pPCLast->GetPointClass() == SM_PC_VERTEX)
                              ? (SmVertex *)pPCLast->GetObject()
                              : NULL ;
      SmVertex *pVertexNext =   (pPCNext && pPCNext->GetPointClass() == SM_PC_VERTEX)
                              ? (SmVertex *)pPCNext->GetObject()
                              : NULL ;

      // when next interval has both ends classifying to the same vertex and is
      // not meant to be a closed interval.
      //  note: review next interval first in case both intervals have to be removed
      SM_ASSERT_BREAK_MSG(rCurveClass.GetCurve()->GetDim() == 3, _T("Bad CurveClass 3d Assumption switch call to sm_GetXSectTol2dOr3d_ForClassifyPair()")) ;
      SmXSectTol3d sEdgeXSectTol3d = SmTol::GetXSectTol3d(sEdgeZoneTol3d, sEdgeZoneTol3d) ;
      if(   pVertex == pVertexNext    
         && SmTol::IsDegenerate(pEdge->GetCurve(), rCurveClass[lPointClassIndex].GetInterval(), &sEdgeXSectTol3d))
    //      && rCurveClass.GetInterval().GetLength() > 2.0 * rCurveClass[lPointClassIndex].GetInterval().GetLength())
        {
          // inform the public
          MSG(_T("ChangeEdgePointClassToNearVertex: removing degenerate NextIvl created by snapping PtClassification from Edge to Vertex")) ;
          SmCurveInterval &rCurveIvl = rCurveClass[lPointClassIndex] ;

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe=FALSE;
          if (bDebugMe)
            {
              SM_DUMP_AND_ASSERT_VALID(&rCurveClass) ; // modified Interval[lPointClassIndex]
              rCurveIvl.Dump() ;
              SmBrep *pBrep = rCurveIvl.GetBrep() ;

              smgfx_Erase() ;
              smgfx_SetLook(1,2, 0,0,1) ; if(pBrep) { pBrep->Draw(TRUE) ; } sm_GraphicsLoop() ;
              smgfx_SetLook(3,4, 1,.5,0); rCurveClass.GetCurve()->Draw() ; sm_GraphicsLoop() ;
              smgfx_SetLook(5,7, 0,1,1) ; rCurveIvl.m_vStart.Draw(); sm_GraphicsLoop();
              smgfx_SetLook(5,7, 0,1,1) ; rCurveIvl.m_vEnd.Draw(); sm_GraphicsLoop();
              sm_GraphicsLoop();
            }
#endif // SM_DEBUG_CODE

          // init then remove the interval (end intervals can end up being set to SM_IP_UNKNOWN)
          rCurveIvl.m_vMid.SetClassObject( SM_PC_UNKNOWN, NULL );

          // Don't remove a PointClass at the end, remove an interior one if possible.
          // The end must be within tol of the vertex, else it
          // wouldn't have been classified to it.  [B53]
          // (And someone else will surely snap to it...)
          SmBoolean bBiggerGapStart = ( rCurveIvl.m_vStart.GetGap3d()     > rCurveIvl.m_vEnd.GetGap3d() ) ;

          SmIntervalPosition eWhichPC =   ( lPointClassIndex == 0 )                     ? SM_IP_END
                                        : ( lPointClassIndex == rCurveClass.GetSize() ) ? SM_IP_START
                                        : bBiggerGapStart                               ? SM_IP_START : SM_IP_END ;

          rCurveClass.RemovePointClass( lPointClassIndex, eWhichPC );

        } // end when next interval has both ends classifying to the same vertex

      // when previous interval has both ends classifying to the same vertex
      if(   pVertex == pVertexLast
         && SmTol::IsDegenerate(pEdge->GetCurve(), rCurveClass[lPointClassIndex-1].GetInterval(), &sEdgeXSectTol3d))
    //      && rCurveClass.GetInterval().GetLength() > 2.0 * rCurveClass[lPointClassIndex-1].GetInterval().GetLength())
        {
          // inform the public
          MSG(_T("ChangeEdgePointClassToNearVertex: removing degenerate PrevIvl created by snapping PtClassification from Edge to Vertex")) ;
          SmCurveInterval &rCurveIvl = rCurveClass[lPointClassIndex-1] ;

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe=FALSE;
          if (bDebugMe)
            {
              SM_DUMP_AND_ASSERT_VALID(&rCurveClass) ; // modified Interval[lPointClassIndex]
              rCurveIvl.Dump() ;
              SmBrep *pBrep = rCurveIvl.GetBrep() ;

              smgfx_Erase() ;
              smgfx_SetLook(1,2, 0,0,1) ; if(pBrep) { pBrep->Draw(TRUE) ; } sm_GraphicsLoop() ;
              smgfx_SetLook(3,4, 1,.5,0); rCurveClass.GetCurve()->Draw() ; sm_GraphicsLoop() ;
              smgfx_SetLook(5,7, 0,1,1) ; rCurveIvl.m_vStart.Draw(); sm_GraphicsLoop();
              smgfx_SetLook(5,7, 0,1,1) ; rCurveIvl.m_vEnd.Draw(); sm_GraphicsLoop();
              sm_GraphicsLoop();
            }
#endif // SM_DEBUG_CODE

          // init then remove the interval (end intervals can end up being set to SM_IP_UNKNOWN)
          rCurveIvl.m_vMid.SetClassObject( SM_PC_UNKNOWN, NULL );

          // Don't remove a PointClass at the end, remove an interior one if possible.
          // The end must be within tol of the vertex, else it
          // wouldn't have been classified to it.  [B53]
          SmBoolean bBiggerGapStart = ( rCurveIvl.m_vStart.GetGap3d()     > rCurveIvl.m_vEnd.GetGap3d() ) ;

          SmIntervalPosition eWhichPC =   ( lPointClassIndex <= 1 )                     ? SM_IP_END
                                        : ( lPointClassIndex >  rCurveClass.GetSize() ) ? SM_IP_START
                                        : bBiggerGapStart                               ? SM_IP_START : SM_IP_END ;

          rCurveClass.RemovePointClass( lPointClassIndex-1, eWhichPC );

        } // end when previous interval has both ends classifying to the same vertex
    } // end need to review intervals after pointClassification change check

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe=FALSE;
  if (bDebugMe)
    {
      SM_DUMP_AND_ASSERT_VALID(&rCurveClass) ; // modified Interval[lPointClassIndex]
      SmBrep *pBrep = rCurveClass.GetBrep() ;

      smgfx_Erase() ;
      smgfx_SetLook(1,2, 0,0,1) ; if(pBrep) { pBrep->Draw(TRUE) ; } sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 1,.5,0); rCurveClass.GetCurve()->Draw() ; sm_GraphicsLoop() ;
      sm_GraphicsLoop();
    }
#endif // SM_DEBUG_CODE

  // no changes made - all done
  return SM_SUCCESS;

} // end sm_ChangeEdgePointClassToNearVertex

// ---- verbatim: SmCurveClass.cpp lines 3569-3708: sm_FixProblemsBySqueezing
static SmStatus sm_FixProblemsBySqueezing
  (SmCurveClassification & rCurveClass,        // in : 1st curveClassification of common curve
   SmCurveClassification & rCurveClassOther)   // in : 2nd curveClassification of common curve
{
  // locals
  ULONG              lIndex1,     lIndex2;
  SmIntervalPosition ePosition1,  ePosition2;
  double             dEndDist3d1, dEndDist3d2;

  // gwc note: expect both SrcZoneTol3d vals to be the same since we expect both SrcCurves to be the same intersection curve
#ifdef SM_DEBUG_CODE
  SmZoneTol3d sSrcZoneTol3d      = SmTol::GetSrcZoneTol3d(&rCurveClass) ;
  SmZoneTol3d sOtherSrcZoneTol3d = SmTol::GetSrcZoneTol3d(&rCurveClassOther) ;
  SM_ASSERT_MSG(SM_ARE_SAME(sSrcZoneTol3d, sOtherSrcZoneTol3d), _T("FixProblemsBySqueezing: Surprised to find different SrcZoneTol3d values for expected common SrcCurve")) ;
#endif // SM_DEBUG_CODE

  // for every CurveClass Interval
  for (ULONG ii=0; ii<rCurveClass.GetSize(); ii++)
    {
      SmCurveInterval & rCIvl              = rCurveClass[ii];
      SmZoneTol3d       sStartObjZoneTol3d = SmTol::GetObjZoneTol3d(&rCIvl.m_vStart) ;
      SmZoneTol3d       sEndObjZoneTol3d   = SmTol::GetObjZoneTol3d(&rCIvl.m_vEnd) ;

      // classify ThisCurve Interval BeginPoint against OtherCurve
      if(SM_SUCCESS != rCurveClassOther.FindInterval(rCIvl.m_vInterval.GetMin(), // in : target parameter value
                                                     SM_BIG_DOUBLE,              // in : previous call Param to prevent double classifications, SM_BIG_DOUBLE to ignore
                                                     sStartObjZoneTol3d,         // in : SmTol::GetZoneTol3d(pAssocObj), always 3d Tol even when SrcCurve->Dim == 2
                                                     lIndex1,                    // out: containing interval index
                                                     dEndDist3d1,                // out: for SM_IP_START and SM_IP_END pts, dist3d (or dist2d) along m_cpCurve to ivl end point
                                                     ePosition1))                // out: oneof SM_IP_START, SM_IP_INSIDE, SM_IP_END, SM_IP_OUTSIDE dParameter classification
        { continue; }                                                            

      // classify ThisCurve Interval EndPoint against OtherCurve
      if(SM_SUCCESS != rCurveClassOther.FindInterval(rCIvl.m_vInterval.GetMax(), // in : target parameter value
                                                     SM_BIG_DOUBLE,              // in : previous call Param to prevent double classifications, SM_BIG_DOUBLE to ignore
                                                     sEndObjZoneTol3d,           // in : SmTol::GetZoneTol3d(pAssocObj), always 3d Tol even when SrcCurve->Dim == 2
                                                     lIndex2,                    // out: containing interval index
                                                     dEndDist3d2,                // out: for SM_IP_START and SM_IP_END pts, dist3d (or dist2d) along m_cpCurve to ivl end point
                                                     ePosition2))                // out: oneof SM_IP_START, SM_IP_INSIDE, SM_IP_END, SM_IP_OUTSIDE dParameter classification
        { continue; }  
        
      // gwc note: it seems a case is missing - what happens when ePosition[1 or 2] is not SM_IP_START or SM_IP_END
      //           answer: case should not come up as long as the input classifications are homogonized before this call.
      //                   But if they are homongonized, then none of these hueristics should ever fire.
      //                   Perhaps we can safely get rid of this method.                                                            

      // When classifying a param that lies on an Interval boundary (expected case), its
      // possible that the param can be classified to the lastInterval's endPoint
      // or the nextIntervals StartPoint.  Make sure that beginPoints map to
      // BeginPoints and EndPoints map to EndPoints.
      if(   ePosition2 == SM_IP_START
         && lIndex2     > lIndex1    ) { ePosition2 = SM_IP_END;
                                         lIndex2 --;
                                       }
      if(   ePosition1 == SM_IP_END
         && lIndex1     < lIndex2    ) { ePosition1 = SM_IP_START;
                                         lIndex1 ++;
                                       }

      // Now both classifications should be mapped to same interval.
      // If Not: try heuristic to fix things.
      //   Heuristic: remove OtherCurve boundaryPoint to merge two OtherCurve intervals.
      //              An endPoint is selected from the shorter of the two OtherCurve
      //              intervals containing the thisCurve EndPoints based on what
      //              the endPoints connect to and their tolerances.
      //   gwc: 1. This hueristic does not check to see if the otherCurve intervals
      //           are neighbors.
      //        2. This hueristic does not necessarily remove the boundary point between the
      //           the two intervals even if they are neighbors.
      //        3. So this heuristic does not guarantee that the thisCurve endPoints
      //           map to a single OtherCurve interval after the squeeze.
      //           Since its unclear what this heuristic is trying to achieve
      //           this may or may not be a problem.

      // when ThisCurve interval EndPoints map to different OtherCurve Intervals
      if (lIndex1 != lIndex2)
        {
#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
          if (bDebugMe)
            {
              SM_DUMP_AND_ASSERT_VALID(&rCurveClass);
              SM_DUMP_AND_ASSERT_VALID(&rCurveClassOther);
            }
#endif // SM_DEBUG_CODE

          // get OtherCurve intervals containing thisCurve Interval endPoints
          SmCurveInterval & rCIvl1 = rCurveClassOther[lIndex1];
          SmCurveInterval & rCIvl2 = rCurveClassOther[lIndex2];

          // For the shortest OtherCurve interval
          ULONG lShortestIndex =  rCIvl1.m_vInterval.GetLength() < rCIvl2.m_vInterval.GetLength()
                                ? lIndex1
                                : lIndex2 ;

          // select the otherCurve interval end to squeeze
          //   when ends are connected to different kinds of topology
          //      save end connected to most specific topology, e.g. save a vertex connection over a face connection
          //   else save the end with the best tolerance.
          SmIntervalPosition eSqueezePos = rCurveClassOther.FindBestSqueezeEnd(lShortestIndex);

          MSG(_T("FixProblemsBySqueezing: removing OtherPtClassification to merge two OtherIntervals to align with a single matching ThisCurveClassification interval")) ;

          // remove eSqueezePos end of lShortestIndex to merge two intervals into one.
          SER(rCurveClassOther.ConnectIntervals(lShortestIndex, // in : interval to be removed (thought of as squeezed to zero length)
                                                eSqueezePos,    // in : interval endPoint to be removed
                                                FALSE));        // in : TRUE = OK to delete end interval and shorten m_vInterval
                                                                //      FALSE= only merge end_intervals with their neighbors

        } // end ThisCurve interval EndPoints map to different OtherCurve Intervals check

      // else when: ThisCurve Interval endPoints map to the same OtherCurve
      //            boundary Point - remove ThisCurve Interval treating it like a zero length object
      else if(   ePosition1 == ePosition2
              && (   ePosition1 == SM_IP_START
                  || ePosition2 == SM_IP_END))
        {
          // Select ThisCurve IntervalEndPoint with worst tolerance
          SmIntervalPosition eSqueezePos = rCurveClass.FindBestSqueezeEnd(ii);

          MSG(_T("FixProblemsBySqueezing: removing ThisPtClassification to merge two ThisIntervals to align with a single matching OtherCurveClassification interval")) ;

          // squeeze Selected ThisCurve IntervalEndPoint to its other endPoint
          // location.  This either shortens the curve or lengthens a neighbor
          //  interval to replace this interval
          SER(rCurveClass.ConnectIntervals(ii,          // in : interval to be removed (thought of as squeezed to zero length)
                                           eSqueezePos, // in : interval endPoint to be removed
                                           TRUE));      // in : TRUE = OK to delete end interval and shorten m_vInterval
                                                        //      FALSE= only merge end_intervals with their neighbors
          // after modyfing ThisCurve - start over
          return sm_FixProblemsBySqueezing(rCurveClass,rCurveClassOther);

        } // end thisCurveInterval is equivalent to zero length

    } // end iter every CurveClass interval

  // all done
  return SM_SUCCESS;

} // end sm_FixProblemsBySqueezing

// clang-format on
} // namespace sc_cc_p3

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

// Ops (keep in sync with crates/solidcore-ffi/src/units/intersect_curve_class_p3.rs).
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
    OP_FIND_INTERVAL = 30,
    OP_FIND_PC = 31,
    OP_IS_COINCIDENT = 32,
    OP_INSERT = 33,
    OP_REMOVE_PC = 34,
    OP_MERGE = 35,
    OP_CLEANUP = 36,
    OP_NEAR_VERTEX = 37,
    OP_SQUEEZE = 38,
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

void put_vec(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

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
// ---- guards of the part-3 ops -------------------------------------------------------------

// No interval boundary classification refers to a point: a point is an SmPoint3d, not an
// SmObject, and the zone tolerance lookups of FindInterval / InsertPointClass read the objects
// of every interval start and end (SmTol::GetObjZoneTol3d crashes on it).
bool pcs_ok(const SmCurveClassification& cc, const Pool& pool)
{
    for (ULONG i = 0; i < cc.GetSize(); i++)
    {
        const SmCurveInterval& ivl = cc.GetAt(i);
        const SmObject* a = ivl.m_vStart.GetObject();
        const SmObject* b = ivl.m_vEnd.GetObject();
        if (a != NULL && pool.kind_of(a) == K_POINT)
            return false;
        if (b != NULL && pool.kind_of(b) == K_POINT)
            return false;
    }
    return true;
}

bool pc_ok(const SmPointClassification& pc, const Pool& pool)
{
    return pc.GetObject() == NULL || pool.kind_of(pc.GetObject()) != K_POINT;
}

bool has_edge(const SmCurveClassification& cc)
{
    for (ULONG i = 0; i < cc.GetSize(); i++)
    {
        const SmCurveInterval& ivl = cc.GetAt(i);
        if (ivl.m_vStart.GetPointClass() == SM_PC_EDGE || ivl.m_vMid.GetPointClass() == SM_PC_EDGE ||
            ivl.m_vEnd.GetPointClass() == SM_PC_EDGE)
            return true;
    }
    return false;
}

// FindInterval(param) is defined: it needs an interval and, unless the parameter snaps to an
// interval boundary within SM_EFF_ZERO_PARAM before any curve use (below the first minimum and
// above the last maximum it evaluates the curve), a curve. Replays the checks of FindInterval
// that come before its first use of m_cpCurve.
bool find_ok(const SmCurveClassification& cc, const Pool& pool, double param)
{
    (void)pool;
    ULONG n = cc.GetSize();
    if (n == 0)
        return false;
    if (cc.GetCurve() != NULL)
        return true;
    double eps = SmTol::GetEffZeroParam();
    if (param < cc.GetAt(0).GetMin() - eps)
        return false;
    if (param > cc.GetAt(n - 1).GetMax() + eps)
        return false;
    for (ULONG i = 0; i < n; i++)
    {
        const SmCurveInterval& ivl = cc.GetAt(i);
        if (param > ivl.GetMax() + eps * (double)(i == n - 1))
            continue;
        if (param > ivl.GetMax() - eps)
            return true;
        if (param >= ivl.GetMin() - eps && param < ivl.GetMin() + eps)
            return true;
        return false; // within the interval: evaluates the curve
    }
    return false;
}

// A point classification `class obj t gap presnap src_tol sss_flag P[3]` (the same as the
// SetClassObject / SetTParam / SetGap3d / SetPreSnapParam / SetSrcZoneTol3d / SetSSSPoint ops).
bool read_pc(Rd& rd, const Pool& pool, SmPointClassification& pc)
{
    int64_t c = rd.idx();
    int64_t obj = rd.idx();
    double t = rd.num();
    double gap = rd.num();
    double presnap = rd.num();
    double tol = rd.num();
    bool sss = rd.flag();
    SmPoint3d p = rd.vec();
    if (rd.bad)
        return false;
    int64_t want = kind_of_class(c);
    if (want == -2)
        return false;
    bool ok = true;
    void* po = pool.get(obj, {}, ok);
    if (!ok)
        return false;
    if (obj != -1)
    {
        int64_t k = pool.kinds[(size_t)obj];
        if (!(k == want || (want == K_BSURF && k == K_PLANE)))
            return false;
    }
    pc.SetClassObject((SmPointClassificationType)c, (SmObject*)po);
    pc.SetTParam(t);
    pc.SetGap3d(gap);
    pc.SetPreSnapParam(presnap);
    pc.SetSrcZoneTol3d(SmZoneTol3d(tol));
    if (sss)
        pc.SetSSSPoint(p);
    return true;
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
        // ---- the part-3 members ------------------------------------------------------------------
        case OP_FIND_INTERVAL:
        case OP_FIND_PC:
        {
            double prm = rd.num();
            double last = op == OP_FIND_INTERVAL ? rd.num() : SM_BIG_DOUBLE;
            double tol = rd.num();
            if (rd.bad || !find_ok(cc, pool, prm))
                return SC_ERR_ARG;
            if (op == OP_FIND_INTERVAL)
            {
                ULONG idx = 0;
                double dist = 0.0;
                SmIntervalPosition pos = SM_IP_OUTSIDE;
                SmStatus s = cc.FindInterval(prm, last, SmZoneTol3d(tol), idx, dist, pos);
                o.push_back(st(s));
                if (s == SM_SUCCESS)
                {
                    o.push_back((double)idx);
                    o.push_back(dist);
                    o.push_back((double)pos);
                }
            }
            else
            {
                SmPointClassification pc;
                SmStatus s = cc.FindPointClass(prm, SmZoneTol3d(tol), pc);
                o.push_back(st(s));
                if (s == SM_SUCCESS)
                    put_pc(o, pc, pool);
            }
            break;
        }
        case OP_IS_COINCIDENT:
        {
            SmExtent1d ext;
            {
                double a = rd.num();
                double b = rd.num();
                ext = SmExtent1d(a, b);
            }
            double tol = rd.num();
            if (rd.bad || !pcs_ok(cc, pool))
                return SC_ERR_ARG;
            double lo = ext.GetMin();
            double hi = ext.GetMax();
            // IsCoincident ignores the statuses of its two FindInterval calls and reads
            // m_pData[ii] for every ii in [index1, index2] (index2 - 1 wraps unsigned): run the
            // two searches here (const, deterministic) and replay the walk to reject a walk
            // past the intervals (undefined upstream)
            if (size == 0 || !find_ok(cc, pool, lo) || !find_ok(cc, pool, hi))
                return SC_ERR_ARG;
            ULONG i1 = 0, i2 = 0;
            double d1 = 0.0, d2 = 0.0;
            SmIntervalPosition p1 = SM_IP_OUTSIDE, p2 = SM_IP_OUTSIDE;
            cc.FindInterval(lo, SM_BIG_DOUBLE, SmZoneTol3d(tol), i1, d1, p1);
            cc.FindInterval(hi, SM_BIG_DOUBLE, SmZoneTol3d(tol), i2, d2, p2);
            if (p1 == SM_IP_END)
                i1++;
            if (p2 == SM_IP_START)
                i2--;
            for (ULONG ii = i1; ii <= i2; ii++)
            {
                if (ii >= (ULONG)size)
                    return SC_ERR_ARG;
                if (cc.GetAt(ii).m_vMid.GetPointClass() != SM_PC_EDGE)
                    break;
            }
            o.push_back(0.0);
            o.push_back(flg(cc.IsCoincident(ext, SmZoneTol3d(tol))));
            break;
        }
        case OP_INSERT:
        {
            SmPointClassification pc;
            if (!read_pc(rd, pool, pc))
                return SC_ERR_ARG;
            double prm = rd.num();
            double last = rd.num();
            bool force = rd.flag();
            if (rd.bad || size == 0 || cc.GetCurve() == NULL || !pcs_ok(cc, pool) || !pc_ok(pc, pool) ||
                !find_ok(cc, pool, prm))
                return SC_ERR_ARG;
            // a vertex/edge classification pair on the found boundary walks SmEdge::GetVertex
            // of a stand-in edge (no edgeuse: crash). The boundary the insertion would merge
            // with is the one FindInterval reports (the "undo" step only demotes it to inside).
            if (!force)
            {
                ULONG idx = 0;
                double dist = 0.0;
                SmIntervalPosition pos = SM_IP_OUTSIDE;
                SmStatus s = cc.FindInterval(prm, last, SmTol::GetObjZoneTol3d(&pc), idx, dist, pos);
                if (s == SM_SUCCESS && (pos == SM_IP_START || pos == SM_IP_END))
                {
                    const SmCurveInterval& ivl = cc.GetAt(idx);
                    SmPointClassificationType bt =
                        pos == SM_IP_START ? ivl.m_vStart.GetPointClass() : ivl.m_vEnd.GetPointClass();
                    SmPointClassificationType nt = pc.GetPointClass();
                    if ((bt == SM_PC_VERTEX && nt == SM_PC_EDGE) || (bt == SM_PC_EDGE && nt == SM_PC_VERTEX))
                        return SC_ERR_ARG;
                }
            }
            SmBoolean made = FALSE;
            SmStatus s = cc.InsertPointClass(pc, prm, last, made, force ? TRUE : FALSE);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
                o.push_back(flg(made));
            break;
        }
        case OP_REMOVE_PC:
        {
            int64_t j = rd.idx();
            int64_t pos = rd.idx();
            if (rd.bad || j < 0 || j > 64)
                return SC_ERR_ARG;
            o.push_back(st(cc.RemovePointClass((ULONG)j, (SmIntervalPosition)pos)));
            break;
        }
        case OP_MERGE:
        {
            int64_t index = rd.idx();
            if (rd.bad || size == 0 || index < 0 || index > size || cc.GetCurve() == NULL ||
                !pcs_ok(cc, pool))
                return SC_ERR_ARG;
            // MergeIntoObject of a face / edge / vertex / region reaches its B-rep (none here)
            const SmPointClassification& pc =
                index == size ? cc.GetAt((ULONG)size - 1).m_vEnd : cc.GetAt((ULONG)index).m_vStart;
            int64_t c = (int64_t)pc.GetPointClass();
            if (!(c == 100 || c == 110 || c == 105 || c == 106 || c == 107))
                return SC_ERR_ARG;
            // NER(object): the classes above have a pool object of the right kind or none
            o.push_back(st(cc.MergePointClass((ULONG)index, NULL, NULL)));
            break;
        }
        case OP_CLEANUP:
        {
            // AreCoincidentMaybe reads other[ii] and the curves of both classifications, and
            // walks the edges of edge-classified intervals (stand-ins here)
            if (size > 0)
            {
                if ((int64_t)other.GetSize() < size || cc.GetCurve() == NULL || other.GetCurve() == NULL ||
                    cc.GetCurve()->GetDim() != 3 || other.GetCurve()->GetDim() != 3 || !pcs_ok(cc, pool) ||
                    !pcs_ok(other, pool) || has_edge(cc) || has_edge(other))
                    return SC_ERR_ARG;
            }
            else if (cc.GetCurve() == NULL || cc.GetCurve()->GetDim() != 3)
                return SC_ERR_ARG;
            o.push_back(st(cc.CleanupAndValidate(other)));
            put_cc(o, other, pool);
            break;
        }
        case OP_NEAR_VERTEX:
        {
            int64_t index = rd.idx();
            SmPoint3d p = rd.vec();
            if (rd.bad || size == 0 || index < 0 || index > size || cc.GetCurve() == NULL || !pcs_ok(cc, pool))
                return SC_ERR_ARG;
            // only the classifications that are not edges return before the topology walk
            const SmPointClassification& pc =
                index == size ? cc.GetAt((ULONG)size - 1).m_vEnd : cc.GetAt((ULONG)index).m_vStart;
            if (pc.GetPointClass() == SM_PC_EDGE)
                return SC_ERR_ARG;
            SmBoolean changed = FALSE;
            SmStatus s = sc_cc_p3::sm_ChangeEdgePointClassToNearVertex((ULONG)index, cc, p, changed);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
                o.push_back(flg(changed));
            break;
        }
        case OP_SQUEEZE:
        {
            if (size > 0 && ((int64_t)other.GetSize() == 0 || other.GetCurve() == NULL))
                return SC_ERR_ARG;
            if (!pcs_ok(cc, pool) || !pcs_ok(other, pool))
                return SC_ERR_ARG;
            // every interval boundary of cc is searched in other (see OP_FIND_INTERVAL)
            for (int64_t j = 0; j < size; j++)
            {
                const SmCurveInterval& ivl = cc.GetAt((ULONG)j);
                if (!find_ok(other, pool, ivl.GetMin()) || !find_ok(other, pool, ivl.GetMax()))
                    return SC_ERR_ARG;
            }
            o.push_back(st(sc_cc_p3::sm_FixProblemsBySqueezing(cc, other)));
            put_cc(o, other, pool);
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

SC_EXPORT int32_t sc_intersect_curve_class_p3_run(const double* in, int64_t n_in, double* out, int64_t cap,
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
