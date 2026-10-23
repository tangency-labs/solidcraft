// SPDX-License-Identifier: Apache-2.0
// Unit intersect.curve_class_p5. sm_InsertSolutions below is verbatim upstream
// source/SMLib/src/SmCurveClass.cpp:4158-4791, pinned c9c5979 (Apache-2.0).
// Wire input and native guards are documented in the corresponding FFI module.
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

#include <SmCircle.h>
#include <memory>
#include <cmath>
SmStatus sm_RefineCCIToSurfaces(SmCurveClassification*, SmSolution&, SmEdge*, SmBoolean&, SmPoint3d&, double&, ULONG&);
namespace sc_cc_p5 {
static SmStatus sm_InsertSolutions               // eff: split curveClassification into intervals given curve/curve intersections
  (SmCurveClassification * pCurveClassification, // i/o: classification to modify
   SmSolutionArray       & rSolutions,           // in : ClassificationCurve/Obj intersection solutions
   SmObject              * pObj,                 // in : either an SmEdge, a 2D or 3D SmCurve, or a SmPlane
   SmXSectTol3d          * pOptXSectTol3d)       // in : Optional XSectTol3d(SrcCurve,pObj) val
                                                 //      NULL = SmTol::GetXSectTol3d(CrvClass->SrcCurve,pObj)
                                                 //      uses: size for degnerate ClassCrv check to change short RangeSols to PtSols
                                                 //            near size for Solutions-near-end checks
{
  // locals
  const SmCurve & crSrcCurve2dOr3d = *pCurveClassification->GetCurve();
  SmCurve       * pCurve2dOr3d     = SM_CAST_PTR(SmCurve, pObj) ;
  SmPlane       * pPlane           = SM_CAST_PTR(SmPlane, pObj) ;
  SmCurve       * p3DCurve         = NULL ;
  SmCurve       * p2DCurve         = NULL ;

  // tolerances
  SmZoneTol3d  sSrcZoneTol3d = SmTol::GetSrcZoneTol3d(pCurveClassification) ;
  SmZoneTol3d  sObjZoneTol3d = SmTol::GetZoneTol3d(pObj) ;
  SM_ASSERT_BREAK_MSG(   pCurveClassification->GetCurve()->GetDim() == 3 || pOptXSectTol3d, 
                      _T("Bad CurveClass 3d Assumption switch call to sm_GetXSectTol2dOr3d_ForClassifyPair()")) ;
  SmXSectTol3d sXSectTol3d   = pOptXSectTol3d ? *pOptXSectTol3d : SmTol::GetXSectTol3d(sSrcZoneTol3d, sObjZoneTol3d) ;    

  SmEdge          * pEdge      = SM_CAST_PTR(SmEdge, pObj) ;
  SmCurve         * pEdgeCurve = pEdge ? pEdge->GetCurve() : NULL ;

  // when given an Edge - set p3DCurve to Edge->Curve
  // else set up the 2D/3D pointers
  // from this point on - we test if pEdge == NULL to see if we are working with just a curve in this function.
  if(pEdge)                                               { pCurve2dOr3d = pEdgeCurve ;
                                                            p3DCurve     = pEdgeCurve ;
                                                            SM_ASSERT(p3DCurve != NULL) ;
                                                          }
  else if(pCurve2dOr3d) { if(pCurve2dOr3d->GetDim() == 3) { p3DCurve = pCurve2dOr3d ; }
                          else                            { p2DCurve = pCurve2dOr3d ; }
                        }

  // pCurve2dOr3d will be NULL only when pObj is a SmPlane from a Volume IntersectDiscontinuity() call.

  // check input - pObj expected to be oneof SmEdge, SmCurve, SmPlane
  SER_MSG(  (pEdge == NULL && pCurve2dOr3d == NULL && pPlane == NULL) ? SM_ERR : SM_SUCCESS,
          _T("Bad sm_InsertSolutions pObj input value - must be an SmEdge, SmCurve, or SmPlane pointer")) ;

#ifdef SM_DEBUG_CODE
  SmSurface* pSurface1 = (SmSurface*)pCurveClassification->GetSurface1();

  SmBoolean bDebugMe = FALSE;
  // draw curve/curve intersect solutions(red and yellow), Brep(blue) and curves(cyan and green)
  if(bDebugMe)
    {
      if(pEdge)           SM_DUMP_AND_ASSERT_VALID(pEdge) ;     // dump curve/curve XSect inputs
      if(p3DCurve) SM_DUMP_AND_ASSERT_VALID(p3DCurve) ; 
      if(p2DCurve)        SM_DUMP_AND_ASSERT_VALID(p2DCurve) ;
      if(pPlane)          SM_DUMP_AND_ASSERT_VALID(pPlane) ;

      rSolutions.Dump() ;                              // Dump curve/curve intersect solutions
      SM_DUMP_AND_ASSERT_VALID(pCurveClassification) ; // Dump CurveClassification

      SmEdge *pDEdge =   pEdge ? pEdge
                       : p3DCurve ? (SmEdge *)p3DCurve->GetEdge()
                       : NULL ;
      SmBrep *pBrep  = pDEdge ? pDEdge->GetBrep() : NULL ;
      
      smgfx_Erase() ;
      smgfx_SetLook(1,2, 0,0,1) ; if(pBrep) { pBrep->Draw(TRUE) ; } sm_GraphicsLoop() ;
      smgfx_SetLook(3,5, 0,1,1) ; if(pDEdge) pDEdge->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,5, 0,1,0) ; if(p2DCurve && pSurface1) { SmCrvOnSurf sCrv(*p2DCurve, *pSurface1) ; sCrv.Draw() ; } sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,0,1) ; if(pPlane) { pPlane->Draw(TRUE) ; } sm_GraphicsLoop() ;

      smgfx_SetLook(3,5, 1,0,1) ; if(p3DCurve) p3DCurve->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,5, 0,1,0) ; pCurveClassification->GetCurve()->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(5,6, 1,0,0) ; rSolutions.Draw() ; sm_GraphicsLoop() ;
      sm_GraphicsLoop() ;

    } // end bDebugMe check
#endif // SM_DEBUG_CODE

  // for every ClassificationCurve/pObj intersection solution - 1st, degen ShortIvls to SinglePts - 2nd, setup and insert sols
  for (ULONG j=0; j<rSolutions.GetSize(); j++)
    {
      SmSolution & rSol = rSolutions[j];

      // First, if it's a Range solution that's shorter than tolerance,
      // turn it into a Single solution. [B162]
      if ( rSol.m_eSolutionType == SM_ST_RANGE_OF_VALUES )
        {
          SmPoint3d sPt1,   sPt2 ;
          SmPoint3d dPrms1, dPrms2 ;
          SmStatus eStat = SM_SUCCESS ;

          // Get range boundary points and curve 0 parameters
          eStat &= rSol.GetPoint( 0, sPt1, &dPrms1, FALSE ) ; // Start of range solution
          eStat &= rSol.GetPoint( 0, sPt2, &dPrms2, TRUE  ) ; // End of range solution

          // First check end points, before checking actual curve length.
          if ( eStat == SM_SUCCESS && sPt1.CloserThan( sXSectTol3d, sPt2 ) )
            {
              SmObject * pThisObj   = rSol.m_apObjects[0];
              SmCurve  * pThisCurve = SM_CAST_PTR( SmCurve, pThisObj );
              if ( pThisCurve != NULL )
                {
                  SmExtent1d sIvl( dPrms1[0] ) ;
                  sIvl.AddValue(   dPrms2[0] ) ;
                  double dLen = pThisCurve->ApproximateLength( sIvl, 6 ) ;

                  // when ClassificationCurve ArcLength is less than tolerance - turn solution into a Single Value
                  if ( dLen < sXSectTol3d )
                    {
                      rSol.m_eSolutionType = SM_ST_SINGLE_VALUE;
                      // Use the better solution if there is one. [B513]
                      if ( rSol.m_vEnd.m_dSolutionValue < rSol.m_vStart.m_dSolutionValue * 0.8 )
                        {
                          // End is better.
                          rSol.m_vStart.m_dSolutionValue  = rSol.m_vEnd.m_dSolutionValue;
                          rSol.m_vStart.m_adParameters[0] = rSol.m_vEnd.m_adParameters[0];
                          rSol.m_vStart.m_adParameters[1] = rSol.m_vEnd.m_adParameters[1];
                        }
                      else if ( rSol.m_vEnd.m_dSolutionValue < rSol.m_vStart.m_dSolutionValue * 1.2 )
                        {
                          // They're roughly the same: take the average.
                          rSol.m_vStart.m_dSolutionValue  = 0.5 * (  rSol.m_vStart.m_dSolutionValue
                                                                   + rSol.m_vEnd  .m_dSolutionValue ) ;
                          rSol.m_vStart.m_adParameters[0] = 0.5 * (  rSol.m_vStart.m_adParameters[0]
                                                                   + rSol.m_vEnd  .m_adParameters[0] ) ;
                          rSol.m_vStart.m_adParameters[1] = 0.5 * (  rSol.m_vStart.m_adParameters[1]
                                                                   + rSol.m_vEnd  .m_adParameters[1]) ;
                        }
                      // else just stick with m_vStart.

                    } // end range interval is less than tolerance check
                } // end pThisCurve existence check
            } // end range endPoints are close together check
        } // end short enough RangeSolution to turn into SingleValueSolution check

      // when the solution is a point or a zero-length interval
      if(rSol.m_eSolutionType == SM_ST_SINGLE_VALUE)
        {
          SmBoolean bSSSPointFlag = FALSE ;
          SmPoint3d vSSSPoint ;
          double    dDeviation = 0.0 ;
      
      // GWC: future this has to be changed for the NewTol model that supports Vertexuse/CurveClassification intersections

      // If classified to the end of an edge -- it should be a vertex.
      // This is fine as long as the vertex had been identified and
      // classified earlier, but sometimes the vertex can be missed
      // (due to tolerance mismatches, etc.).  If the classification
      // ends up as Edge, then a new vertex will be created, and problems
      // will ensue.  [B301]
      // If we change Edge to Vertex, and it was already classified as
      // Vertex, everything is fine.
      // Also, sometimes the cause of missing the vertex is that the
      // vertex's tolerance is smaller than that of the edge.  This should
      // never happen, for just this reason: if the tolerance of lower-dimensional
      // topology (Vertex) were smaller than that of related higher-dimensional
      // topology (Edge), then a location just off the end of the edge could snap
      // to the Edge, and miss the Vertex.  If we see that, we take the liberty of
      // increasing the vertex's tolerance.

      //static int cbi301 = 1;  // CC_140617

      // Check whether it should have classified to a vertex.
      // static double dTolFactor = 1.01;
      if ( pEdge != NULL )
        {
          double      dParam           = rSol.m_vStart[1];
          SmExtent1d  sEdgeDom         = pEdge->GetInterval();
          SmCurve    *pEdgeCrv         = pEdge->GetCurve();
          SmVertex   *pVtx             = pEdge->GetStartVertex();
          SmZoneTol3d sEdgeZoneTol3d   = SmTol::GetZoneTol3d(pEdge) ;
          SmTol1d     sParamXSectTol3d = SmTol::MapTo1d(sXSectTol3d, sEdgeDom.GetMin(), *pEdgeCurve) ; 

          // if Param is near EdgeIvlStart avoiding probs with closed curves
          if(   smos_Fabs(dParam - sEdgeDom.GetMin()) < sParamXSectTol3d                       //     within XSectTol3d of EdgeEnd
             && smos_Fabs(dParam - sEdgeDom.GetMin()) < smos_Fabs(dParam - sEdgeDom.GetMax())) // and avoids problems with closed ends
            {
              SmZoneTol3d sVtxZoneTol3d = SmTol::GetZoneTol3d(pVtx) ;

              if ( sVtxZoneTol3d < sEdgeZoneTol3d )  
                {
                  if(pCurveClassification->GetOKToSetTolerance()) { pVtx->SetTolerance( sEdgeZoneTol3d ); }
                  sVtxZoneTol3d = sEdgeZoneTol3d ;
                }

              // sXSectTol3d = GetXSectTol3d(ClassCrv, pEdge)
              // GWC: I guess this wants ClassifcationCurve/Vertex XSectTol3d here.
              SM_ASSERT_BREAK_MSG(pCurveClassification->GetCurve()->GetDim() == 3 || pCurveClassification->GetSurface1() != NULL, 
                                  _T("Bad CurveClass 3d Assumption switch call to sm_GetXSectTol2dOr3d_ForClassifyPair()")) ;
              SmXSectTol3d sClassCrv_Vtx_XSectTol3d = SmTol::GetXSectTol3d(sSrcZoneTol3d, sVtxZoneTol3d) ;
              // not needed - checks are being made in 3d
                // SmXSectTol3d sClassCrv_Vtx_XSectTol2dOr3d = sm_GetXSectTol2dOr3d_ForClassifyPair(&crSrcCurve2dOr3d, 
                //                                                                                  pSurface1,
                //                                                                                  dParam, 
                //                                                                                  sSrcZoneTol3d, 
                //                                                                                  dParam, 
                //                                                                                  sVtxZoneTol3d) ;

              // check the EdgeEnd/Vertex distance
              SmPoint3d sEdgeEndPt;
              pEdgeCrv->EvaluatePoint( dParam, sEdgeEndPt );
              double dEdgeEnd_Vtx_Dist3d = sEdgeEndPt.DistanceBetween( pVtx->GetPoint() );

              // if EdgeEnd/Vertex distance < XSectTol3d(Vtx,ClassCurve) - snap to vertex
              if ( dEdgeEnd_Vtx_Dist3d < sClassCrv_Vtx_XSectTol3d )
                {
                  // Don't add this vertex as a PointClassification if it would make a sub-tolerant CurveInterval,
                  //  where both ends are this vertex [B678]
                  SmTArray<SmTouchData> sTouchData ;
                  SmTArray<SmTopology*> sTgtOwners ;
                  sTgtOwners.Add(pVtx) ; 
                  pCurveClassification->GetPointClassifiesConnectedTo(sTgtOwners, sTouchData) ;

                  // gwc-rm 3 lines: call GetPointClassifies() has been replaced by GetPointClassifiesConnectedTo()
                  // SmTArray<SmPointClassification* > sPCs;
                  // SmTArray<double>                  sParams;
                  // pCurveClassification->GetPointClassifies(sPCs, sParams, pVtx);

                  SmBoolean                         bSkipInsert = FALSE;
                  SmXSectTol3d sXSectTol3d_VtxVtx = SmTol::GetXSectTol3d( pVtx, pVtx );
                  for ( ULONG kk = 0; kk < sTouchData.GetSize() && !bSkipInsert; ++kk )
                  {
                      double dParamMin = ( sTouchData[kk].m_dParam < rSol.m_vStart[0] ) ? sTouchData[kk].m_dParam : rSol.m_vStart[0];
                      double dParamMax = ( sTouchData[kk].m_dParam > rSol.m_vStart[0] ) ? sTouchData[kk].m_dParam : rSol.m_vStart[0];
                      SmExtent1d sIvl( dParamMin, dParamMax );
                      if ( crSrcCurve2dOr3d.ApproximateLength( sIvl, 3 ) < sXSectTol3d_VtxVtx )
                      { bSkipInsert = TRUE; }
                  }
                  if ( bSkipInsert )
                  { continue; }

                  // Insert a Vertex classification.
                  SmPointClassification sPC(sSrcZoneTol3d, pCurveClassification->GetContext()) ;
                  sPC.SetClassObject   ( SM_PC_VERTEX, pVtx );
                  sPC.SetGap3d         ( rSol.m_vStart.m_dSolutionValue );
                  sPC.SetSrcZoneTol3d  ( sSrcZoneTol3d ) ; 
                  sPC.SetPreSnapParam  ( rSol.m_vStart[0] );

                  SmBoolean bInsertionMade;
                  SER( pCurveClassification->InsertPointClass( sPC, rSol. m_vStart[0], SM_BIG_DOUBLE, bInsertionMade ));
                  continue;  // return SM_SUCCESS; [B320]
                }
            } // end if classification is near the start of the edge

          // If Param is near EdgeIvlEnd avoiding probs with closed curves
          if(   smos_Fabs(dParam - sEdgeDom.GetMax()) < sParamXSectTol3d                       //     within XSectTol3d of EdgeEnd
             && smos_Fabs(dParam - sEdgeDom.GetMax()) < smos_Fabs(dParam - sEdgeDom.GetMin())) // and avoids problems with closed ends
            {
              pVtx = pEdge->GetOtherVertex( pVtx );
              SmZoneTol3d sVtxZoneTol3d = SmTol::GetZoneTol3d(pVtx) ;

              if ( sVtxZoneTol3d < sEdgeZoneTol3d )  // Here's what can cause problems.
                {
                  if(pCurveClassification->GetOKToSetTolerance()) pVtx->SetTolerance( sEdgeZoneTol3d );
                  sVtxZoneTol3d = sEdgeZoneTol3d ;
                }

              // sXSectTol3d = GetXSectTol3d(ClassCrv, pEdge)
              // GWC: I guess this wants ClassifcationCurve/Vertex XSectTol3d here.
              SM_ASSERT_BREAK_MSG(pCurveClassification->GetCurve()->GetDim() == 3 || pCurveClassification->GetSurface1() != NULL, 
                                  _T("Bad CurveClass 3d Assumption switch call to sm_GetXSectTol2dOr3d_ForClassifyPair()")) ;
              SmXSectTol3d sClassCrv_Vtx_XSectTol3d = SmTol::GetXSectTol3d(sVtxZoneTol3d, sSrcZoneTol3d) ;

              SmPoint3d sEdgeEndPt;
              pEdgeCrv->EvaluatePoint( dParam, sEdgeEndPt );
              double dEdgeEnd_Vtx_Dist3d = sEdgeEndPt.DistanceBetween( pVtx->GetPoint() );

              if ( dEdgeEnd_Vtx_Dist3d < sClassCrv_Vtx_XSectTol3d )
                {
                  // Don't add this vertex as a PointClassification if it would make a sub-tolerant CurveInterval,
                  //  where both ends are this vertex [B678]
                  SmTArray<SmTouchData> sTouchData ;
                  SmTArray<SmTopology*> sTgtOwners ;
                  sTgtOwners.Add(pVtx) ; 
                  pCurveClassification->GetPointClassifiesConnectedTo(sTgtOwners, sTouchData) ;

                  // gwc-rm 3 lines: call GetPointClassifies() has been replaced by GetPointClassifiesConnectedTo()
                  // SmTArray<SmPointClassification* > sPCs;
                  // SmTArray<double>                  sParams;
                  // pCurveClassification->GetPointClassifies(sPCs, sParams, pVtx);

                  SmXSectTol3d sXSectTol3d_VtxVtx = SmTol::GetXSectTol3d( pVtx, pVtx );
                  SmBoolean                         bSkipInsert = FALSE;
                  for ( ULONG kk = 0; kk < sTouchData.GetSize() && !bSkipInsert; ++kk )
                  {
                      double dParamMin = ( sTouchData[kk].m_dParam < rSol.m_vStart[0] ) ? sTouchData[kk].m_dParam : rSol.m_vStart[0];
                      double dParamMax = ( sTouchData[kk].m_dParam > rSol.m_vStart[0] ) ? sTouchData[kk].m_dParam : rSol.m_vStart[0];
                      SmExtent1d sIvl( dParamMin, dParamMax );
                      if ( crSrcCurve2dOr3d.ApproximateLength( sIvl, 3 ) < sXSectTol3d_VtxVtx )
                      { bSkipInsert = TRUE; }
                  }
                  if ( bSkipInsert )
                  { continue; }

                  // Insert a Vertex classification.
                  SmPointClassification sPC(SmTol::GetSrcZoneTol3d(pCurveClassification), pCurveClassification->GetContext()) ;
                  sPC.SetClassObject   ( SM_PC_VERTEX, pVtx );
                  sPC.SetGap3d         ( rSol.m_vStart.m_dSolutionValue );
                  sPC.SetSrcZoneTol3d  ( SmTol::GetSrcZoneTol3d(pCurveClassification) ) ;
                  sPC.SetPreSnapParam  ( rSol.m_vStart[0] );
                  SmBoolean bInsertionMade;
                  SER( pCurveClassification->InsertPointClass( sPC, rSol. m_vStart[0], SM_BIG_DOUBLE, bInsertionMade ));
                  continue;  // return SM_SUCCESS; [B320]
                }
            } // end if classification is near the end of the edge

        } // end if pEdge not null, check for Vertex classification

// GWC:CHANGED_SSS
      // only try SSS for edge solutions
      if(pEdge != NULL)
        {
          // see if vertex should be placed at a surf/surf/surf intersection

          // GWC: The SM_SSSPOINT_REFINEMENT refine point trick is required
          //     for good results. 
          //
          //   When two surfaces are intersected by the general bspline surface
          //   intersection algorithm the resulting intersection curve is created
          //   as a sequence of points connected by a sequence of curve intervals.
          //   The points are computed to within machine tolerance so that the max distance
          //   to the intersection surfaces is effectively zero.  The spacing between the
          //   points is maximized to minimize the number of control points in the intersection
          //   curve.  That maximum distance is limited by the max tolerance in the segment that
          //   connects the two points.  The max distance is found when the max tolerance
          //   in the segment grows to the intersection tolerance value.
          //
          //   The intersection curves are merged into the brep here after they have been
          //   intersected with the edges of the face.  The tolerance of the intersection
          //   curve is not controlled at the edge/IntersectionCurve intersection point;
          //   the tolerance will vary randomly, depending on where the edge curve happens to intersect
          //   the intersection curve, in the range from zero to intersection tolerance.
          //   Intersection tolerance may be set larger than brep tolerance in which case
          //   merging the intersection curve into the Brep may result in a bad Brep with
          //   gaps larger than tolerance.
          //
          //   The Refine fix is actually not a fix but a part of the algorithm.  Before
          //   inserting the intersection curve into brep, refine the point location of
          //   the edge/IntersectionCurve to minimize tolerance.
          //   Computing and adding this point to the intersection curve may change
          //   the shape of the intersection curve and it may even change the shape of the edge curve.
          //   The edge curve may have the same sequence of tight tolerance and loose
          //   tolerance points along its length especially if that curve was computed
          //   as the result of another surface/surface intersection.
          //
          //   These IntersectionCurve/Edge points are refined here rather than in the
          //   general intersection algorithm for performance reasons.  Finding and using the
          //   machine tolerant intersection point between every intersection curve and
          //   every edge of every face that uses a surface in a surface/surface intersection
          //   is very expensive and uselessly conservative.  Machine tolerant accuracy
          //   of these points are only needed when inserted into the topology graph.  By
          //   doing that here rather than in the general intersection algorithm SMLib
          //   saves computing many Edge/Curve intersections that will never be used.

          // begin SSS_XSext refinement block
            {
              ULONG lRecursionCount = 0;
              // when curve/curve intersection gap tolerances are large
              //  look for a surf/surf/surf intersection defining the corner
              //   and possibly modify curve and edge shape to interpolate the position.
              sm_RefineCCIToSurfaces(pCurveClassification,  // i/o: classification to modify  (can be 3d, 2d with pSurface1
                                                            //      classification curve might be modified to interpolate vSSSPoint
                                     rSol,                  // i/o: ClassificationCurve/EdgeCurve intersection solution
                                                            //      curve param values are tightened to vSSSPoint
                                     pEdge,                 // i/o: Edge found to intersect the curve being classified
                                                            //      can be modified to interpolate vSSSPoint
                                     bSSSPointFlag,         // out: TRUE=use Surf/surf/surf intersection point
                                     vSSSPoint,             // out: surf/surf/surf xsect point for this solution
                                     dDeviation,            // out: max surf_pt/surf_pt curv/SSSiPoint distance
                                     lRecursionCount) ;     // i/o: to prevent infinite recursion
#ifdef SM_DEBUG_CODE
             // draw curve/curve intersect solutions(red and yellow), Brep(blue) and curves(cyan and green)
             if(bDebugMe)
               {
                 rSolutions.Dump() ;               // Dump curve/curve intersect solutions
                 if(pEdge)    SM_DUMP_AND_ASSERT_VALID(pEdge) ;
                 if(p3DCurve) SM_DUMP_AND_ASSERT_VALID(p3DCurve) ; 
                 if(p2DCurve) SM_DUMP_AND_ASSERT_VALID(p2DCurve) ; 
                 if(pPlane)   SM_DUMP_AND_ASSERT_VALID(pPlane) ; 
                 SM_DUMP_AND_ASSERT_VALID(pCurveClassification) ;    // Dump CurveClassification

                 SmEdge *pDEdge = pEdge ? pEdge : (SmEdge *)p3DCurve->GetEdge() ;
                 SmBrep *pBrep  = pDEdge ? pDEdge->GetBrep() : NULL ;

                 smgfx_Erase() ;
                 smgfx_SetLook(1,2, 0,0,1) ; if(pBrep) { pBrep->Draw(TRUE) ; } sm_GraphicsLoop() ;
                 smgfx_SetLook(3,5, 0,1,1) ; if(pDEdge) pDEdge->Draw() ; sm_GraphicsLoop() ;
                 smgfx_SetLook(3,5, 0,1,1) ; if(p3DCurve) p3DCurve->Draw() ; sm_GraphicsLoop() ;
                 smgfx_SetLook(3,5, 0,1,1) ; if(p2DCurve && pSurface1) { SmCrvOnSurf sCrv(*p2DCurve, *pSurface1) ; sCrv.Draw() ; } sm_GraphicsLoop() ;
                 smgfx_SetLook(1,2, 0,0,1) ; if(pPlane) pPlane->Draw(TRUE) ; sm_GraphicsLoop() ;
                 smgfx_SetLook(3,5, 0,1,0) ; pCurveClassification->GetCurve()->Draw() ; sm_GraphicsLoop() ;
                 smgfx_SetLook(5,6, 1,0,0) ; rSolutions.Draw() ; sm_GraphicsLoop() ;
                 sm_GraphicsLoop() ;
               } // end bDebugMe check
#endif // SM_DEBUG_CODE
            } // end SSS_XSect refinement block
        } // end if pEdge existence check
// END GWC:CHANGED_SSS


          // build a pointClassification object to insert
          // use the sssi pt if available otherwise use the cci pt
          SmPointClassification sPC(SmTol::GetSrcZoneTol3d(pCurveClassification), pCurveClassification->GetContext()) ;
          if(pEdge)                { sPC.SetClassObject(SM_PC_EDGE,  pEdge) ;  }
          else if(p3DCurve)        { sPC.SetClassObject(SM_PC_CURVE, p3DCurve) ; }
          else if(p2DCurve)        { sPC.SetClassObject(SM_PC_CURVE, p2DCurve) ; }
          else if(pPlane)          { sPC.SetClassObject(SM_PC_SURFACE, pPlane) ; }
          sPC.SetGap3d             (bSSSPointFlag ? dDeviation : rSol.m_vStart.m_dSolutionValue) ;
          sPC.SetSrcZoneTol3d      ( SmTol::GetSrcZoneTol3d(pCurveClassification) ) ;
          sPC.SetPreSnapParam(rSol.m_vStart[0]) ;
          sPC.SetTParam      (rSol.m_vStart[1], pCurveClassification->GetDoingCrossHatch()) ;
          if(bSSSPointFlag)
            {
              SM_ASSERT(   vSSSPoint.x != SM_UNDEF_DOUBLE
                        && vSSSPoint.y != SM_UNDEF_DOUBLE
                        && vSSSPoint.z != SM_UNDEF_DOUBLE) ;
              sPC.SetSSSPoint(vSSSPoint) ;
            }

          SmBoolean bInsertionMade ;
          SmBoolean bDoInsertion = TRUE ;

          // when ignoring tangencies
          if(pCurveClassification->GetIgnoreTangency() && pCurve2dOr3d)
            {
              // Test tangents of edge and curve to see if the angle is very
              // small between them.  Less than the angle tolerance in SmConfig.h
              // If tangent turn off bDoInsertion flag.
              SmVector3d sPV_2dOr3d[2];

              // get the Curve Tangent at the intersection point
              SER(crSrcCurve2dOr3d.Evaluate(rSol.m_vStart[0],1,TRUE,sPV_2dOr3d));
              SmVector3d sCrvTan = sPV_2dOr3d[1];

              // get the Edge->Curve Tangent at the intersection point
              SER(pCurve2dOr3d->Evaluate(rSol.m_vStart[1],1,TRUE,sPV_2dOr3d));
              SmVector3d sEdgeTan = sPV_2dOr3d[1];

              // get the angle between the Curve and Edge Tangents
              double dAngleRad;

              // If one of the tangents is zero, don't give up,
              // just leave the insertion flag TRUE.  [bd 060424]
              //SER(sEdgeTan.AngleBetween(sCrvTan,dAngleRad));
              SmStatus eStat = sEdgeTan.AngleBetween( sCrvTan, dAngleRad );
              if(eStat != SM_SUCCESS)
                 { dAngleRad = 1.0 ; }

              // mirror 2 quadrant angles into first quadrant angles
              if (dAngleRad > SM_PI/2.0)
                { dAngleRad = SM_PI - dAngleRad ; }

              // when tangents are close to parallel
              if (dAngleRad < 5.0*SM_PI*SM_CONTINUITY_ANGLE/180.0)
                {
                  // cancel the Insertion request
                  bDoInsertion = FALSE;
                }

              // exception: intersection is near the interval boundary
              if (pCurveClassification->GetInterval().IsValueOnBoundary(rSol.m_vStart[0]))
                {
                  // reschedule the Insertion
                  bDoInsertion = TRUE;
                }
            } // end ignoring tangencies check

          // when we passed near-tangency test - insert the pointClassification
          if (bDoInsertion)
            {
              SER(pCurveClassification->InsertPointClass(sPC,              // in : Point to insert or merge (ie. combined)
                                                         rSol.m_vStart[0], // in : associated parameter value
                                                         SM_BIG_DOUBLE,    // in : When inserting a sequence of pointClass objects
                                                                           //      is used to prevent two objects from both being
                                                                           //      classified to one interval endPoint.
                                                                           //      Set to SM_BIG_DOUBLE to ignore.
                                                         bInsertionMade)); // out: TRUE = inserted (IvlCnt+=1), FALSE=merged i.e. combined (IvlCnt=const)
                                                                           // in : TRUE = force point insertion even when
                                                                           //             point is close to existing classification points
                                                                           //      FALSE = use internal heuristics to decide when to merge and when to insert
                                                                           //      default:[FALSE]
            }

#ifdef SM_DEBUG_CODE
          if (bDebugMe)
            {
              SM_DUMP_AND_ASSERT_VALID(pCurveClassification) ;

              sm_GraphicsLoop();
              smgfx_SetLook(1,2, 1,0,0); crSrcCurve2dOr3d.DrawAt(rSol.m_vStart[0],0); sm_GraphicsLoop();
              sm_GraphicsLoop();
            }
#endif // SM_DEBUG_CODE

        } // end single point or zero-length interval branch
      else if (rSol.m_eSolutionType == SM_ST_RANGE_OF_VALUES)
        {
          // Check for case where things are close and reversed and just skip it
          if(rSol.m_vStart[0] > rSol.m_vEnd[0]) 
            { continue; }

          // build a curve interval to insert
          SmCurveInterval sCIvl(SmExtent1d(rSol.m_vStart[0],rSol.m_vEnd[0]), pCurveClassification->GetContext(), pCurveClassification) ;
          if(pEdge)         { if(crSrcCurve2dOr3d.GetDim() == 2 && p2DCurve) 
                                { // GWC: If we set ClassObj to 3dEdge when classification is happening in 2d
                                  //      the upcoming CoincidentMaybe() call will compare 2d and 3d curves to one another.
                                  sCIvl.m_vStart.SetClassObject(SM_PC_CURVE, p2DCurve) ;
                                  sCIvl.m_vMid.  SetClassObject(SM_PC_CURVE, p2DCurve) ;
                                  sCIvl.m_vEnd.  SetClassObject(SM_PC_CURVE, p2DCurve) ;
                                }
                              else // GWC: this was the old behavior for all pEdge != NULL cases
                                { sCIvl.m_vStart.SetClassObject(SM_PC_EDGE,  pEdge) ;
                                  sCIvl.m_vMid.  SetClassObject(SM_PC_EDGE,  pEdge) ;
                                  sCIvl.m_vEnd.  SetClassObject(SM_PC_EDGE,  pEdge) ;
                                }
                            }
          else if(p3DCurve) { sCIvl.m_vStart.SetClassObject(SM_PC_CURVE, p3DCurve) ;
                              sCIvl.m_vMid.  SetClassObject(SM_PC_CURVE, p3DCurve) ;
                              sCIvl.m_vEnd.  SetClassObject(SM_PC_CURVE, p3DCurve) ;
                            }
          else if(p2DCurve) { sCIvl.m_vStart.SetClassObject(SM_PC_CURVE, p2DCurve) ;
                              sCIvl.m_vMid.  SetClassObject(SM_PC_CURVE, p2DCurve) ;
                              sCIvl.m_vEnd.  SetClassObject(SM_PC_CURVE, p2DCurve) ;
                            }
          else if(pPlane)   { sCIvl.m_vStart.SetClassObject(SM_PC_SURFACE, pPlane) ;
                              sCIvl.m_vMid.  SetClassObject(SM_PC_SURFACE, pPlane) ;
                              sCIvl.m_vEnd.  SetClassObject(SM_PC_SURFACE, pPlane) ;
                            }

          sCIvl.m_vStart.SetGap3d       (rSol.m_vStart.m_dSolutionValue);

          sCIvl.m_vStart.SetPreSnapParam(rSol.m_vStart[0]);
          sCIvl.m_vStart.SetTParam      (rSol.m_vStart[1], pCurveClassification->GetDoingCrossHatch());

          sCIvl.m_vEnd.SetGap3d       (rSol.m_vEnd.m_dSolutionValue);

          sCIvl.m_vEnd.SetPreSnapParam(rSol.m_vEnd[0]);
          sCIvl.m_vEnd.SetTParam      (rSol.m_vEnd[1], pCurveClassification->GetDoingCrossHatch());

          double dSizeRatio = (  sCIvl.m_vInterval.GetLength()
                               / pCurveClassification->GetInterval().GetLength()) ;
          SmBoolean bInsertionMade;
          SmBoolean bDoInsertion = TRUE;

          // Range solutions can happen when the ClassificationCurve and the EdgeCurve are just tangent.
          // When ignoring tangencies, we will insert this solution only if it's short
          // (i.e., tangent) and at an end.  Note, there will usually already be
          // a vertex there, in which case this will not do anything.  [B332]
          if (   pCurveClassification->GetIgnoreTangency()
              && dSizeRatio < 0.05)
            {
              // cancel the scheduled insertion
              bDoInsertion = FALSE;

              // Check for end point intersections and insert them here
              SmExtent1d sIvl      = pCurveClassification->GetInterval();
              SmTol1d    sParamTol = SmTol::MapTo1d(sXSectTol3d, rSol.m_vStart[0], *pEdgeCurve) ;

              // See whether the Range Start is on a boundary of the curveClassification boundary.
              if ( sIvl.IsValueOnBoundary( rSol.m_vStart[0], sParamTol ))
                {
                  // If it does - insert a pointClassification.
                  // not to partition an interval but to classify an existing one
                  SER(pCurveClassification->InsertPointClass(sCIvl.m_vStart,   // in : Point to insert or merge (ie. combined)
                                                             rSol.m_vStart[0], // in : associated parameter value
                                                             SM_BIG_DOUBLE,    // in : When inserting a sequence of pointClass objects
                                                                               //      is used to prevent two objects from both being
                                                                               //      classified to one interval endPoint.
                                                                               //      Set to SM_BIG_DOUBLE to ignore.
                                                             bInsertionMade)); // out: TRUE = inserted (IvlCnt+=1), FALSE=merged i.e. combined (IvlCnt=const)
                                                                               // in : TRUE = force point insertion even when
                                                                               //             point is close to existing classification points
                                                                               //      FALSE = use internal heuristics to decide when to merge and when to insert
                                                                               //      default:[FALSE]
                }

              // Same for the Range End.
              sParamTol = SmTol::MapTo1d(sXSectTol3d, rSol.m_vEnd[0], *pEdgeCurve) ;
              if ( sIvl.IsValueOnBoundary( rSol.m_vEnd[0], sParamTol ))
                {
                  // if it is - insert a pointClassification 
                  // not to partition an interval but to classify an existing one
                  SER(pCurveClassification->InsertPointClass(sCIvl.m_vEnd,     // in : Point to insert or merge (ie. combined)
                                                             rSol.m_vEnd[0],   // in : associated parameter value
                                                             SM_BIG_DOUBLE,    // in : When inserting a sequence of pointClass objects
                                                                               //      is used to prevent two objects from both being
                                                                               //      classified to one interval endPoint.
                                                                               //      Set to SM_BIG_DOUBLE to ignore.
                                                             bInsertionMade)); // out: TRUE = inserted (IvlCnt+=1), FALSE=merged i.e. combined (IvlCnt=const)
                                                                               // in : TRUE = force point insertion even when
                                                                               //             point is close to existing classification points
                                                                               //      FALSE = use internal heuristics to decide when to merge and when to insert
                                                                               //      default:[FALSE]
                }
            } // end ignore tangencies check

          // when all tests are passed - do the insertion
          if (bDoInsertion)
            {
#ifdef SM_DEBUG_CODE
              if (bDebugMe)
                { SM_DUMP_AND_ASSERT_VALID(pCurveClassification) ;
                  sCIvl.Dump() ; 
                }
#endif // SM_DEBUG_CODE

              SER(pCurveClassification->InsertCurveInterval(sCIvl));
            } // end all tests passed check

#ifdef SM_DEBUG_CODE
          if (bDebugMe)
            {
              SM_DUMP_AND_ASSERT_VALID(pCurveClassification) ;

              sm_GraphicsLoop();
              smgfx_SetLook(1,4, 0,0,1) ; crSrcCurve2dOr3d.DrawAt(rSol.m_vStart[0],0) ; sm_GraphicsLoop();
              smgfx_SetLook(1,4, 1,0,0) ; crSrcCurve2dOr3d.DrawAt(rSol.m_vEnd[0],0) ;   sm_GraphicsLoop();
              sm_GraphicsLoop();
            }
#endif // SM_DEBUG_CODE

      } // end solution was an interval check

#ifdef SM_DEBUG_CODE
      if (bDebugMe)
        {
          SM_DUMP_AND_ASSERT_VALID(pCurveClassification);
        }
#endif // SM_DEBUG_CODE
  } // end iter every solution

  // all done
  return SM_SUCCESS;

} // end sm_InsertSolutions
}
namespace {
struct ScFace : SmFace { ScFace(SmSurface* s) : SmFace() {m_pSurface=s;} ~ScFace() {m_pSurface=NULL;} };
struct ScVertex : SmVertex { ScVertex(double x, double tol) : SmVertex() { m_vPoint=SmPoint3d(x,0,0); m_sZoneTol3d=SmZoneTol3d(tol); } };
void put_curve(std::vector<double>& out, SmCurve* curve) {
    SmExtent1d domain=curve->GetNaturalInterval();
    out.insert(out.end(),{double(curve->GetType()),double(curve->GetDim()),domain.GetMin(),domain.GetMax()});
    for(int j=0;j<=4;j++) {
        SmPoint3d point; SmStatus st=curve->EvaluatePoint(domain.Evaluate(j/4.0),point);
        out.insert(out.end(),{st==SM_SUCCESS?0.0:double(st),point.x,point.y,point.z});
    }
}
void put_pc(std::vector<double>& out, const SmPointClassification& pc, SmObject* target) {
    out.push_back(pc.GetPointClass());
    out.push_back(pc.GetObject() == target ? 1 : pc.GetObject() ? -2 : -1);
    out.push_back(pc.GetDeviation());
    out.push_back(pc.GetTParam());
    SmPoint2d uv=pc.GetUVParam(); SmPoint3d uvw=pc.GetUVWParam(), sss=pc.GetSSSPoint();
    out.insert(out.end(),{uv.x,uv.y,uvw.x,uvw.y,uvw.z,double(pc.AreParametersSet()),double(pc.GetSSSPointFlag()),sss.x,sss.y,sss.z});
    out.push_back(pc.GetPreSnapParam());
    out.push_back(SmTol::GetSrcZoneTol3d(&pc).val);
}
void put_cc(std::vector<double>& out, const SmCurveClassification& cc, SmObject* target) {
    out.push_back(cc.GetSize());
    out.push_back(cc.GetInterval().GetMin()); out.push_back(cc.GetInterval().GetMax());
    for (ULONG j=0;j<cc.GetSize();j++) {
        const SmCurveInterval& ivl = cc[j];
        out.push_back(ivl.GetMin()); out.push_back(ivl.GetMax());
        put_pc(out,ivl.m_vStart,target); put_pc(out,ivl.m_vMid,target); put_pc(out,ivl.m_vEnd,target);
    }
}
}
// All counts, codes and numeric limits are validated by the safe Rust wrapper before
// entry. Reject malformed direct C calls too; never walk caller-declared arrays early.
SC_EXPORT int32_t sc_intersect_curve_class_p5_run(const double* in, int64_t n_in, double* out, int64_t cap, int64_t* out_len) {
    if (!in || !out_len || n_in < 16 || cap < 0 || (!out && cap)) return SC_ERR_ARG;
    SC_TRY
    auto integer = [](double v, int max) { return std::isfinite(v) && v >= 0 && v <= max && v == std::floor(v); };
    if (!integer(in[0],6) || !integer(in[1],4) || !integer(in[2],1) || !integer(in[3],1) ||
        !integer(in[4],1) || !integer(in[5],1) || !integer(in[6],1) || !integer(in[7],3) ||
        !integer(in[8],1) || !integer(in[12],32) || !integer(in[13],32) || !integer(in[15],32)) return SC_ERR_ARG;
    int op=in[0], shape=in[1], dim=2+in[2], target_kind=in[7], index=in[8], n=in[12], ns=in[13], other_n=in[15];
    if (n_in != 18 + 4*(n+other_n) + 7*ns) return SC_ERR_ARG;
    for (int64_t j=0;j<n_in;j++) if (!std::isfinite(in[j]) || std::abs(in[j])>1e21) return SC_ERR_ARG;
    double scale=in[9], tol=in[10], angle=in[11], opt_tol=in[14];
    if (scale<=0 || scale>1e6 || tol<0) return SC_ERR_ARG;
    const double* bounds=in+16;
    const double* other_bounds=bounds+n+1;
    const double* classes=other_bounds+other_n+1;
    const double* sols=classes+3*(n+other_n);
    for(int side=0;side<2;side++) for(int j=0;j<(side?other_n:n);j++) {
        const double* b=side ? other_bounds : bounds;
        if (b[j]>=b[j+1]) return SC_ERR_ARG;
    }
    for(int j=0;j<3*(n+other_n);j++) if (!integer(classes[j]-100,10)) return SC_ERR_ARG;
    for(int j=0;j<ns;j++) if (!integer(sols[7*j],4)) return SC_ERR_ARG;
    // Topology mutation requires arena-backed native objects. This harness supports
    // the skip paths of merging; never call MergeInterval on a bare stand-in face.
    if(op==4 || op==5) for(int j=0;j<3*(n+other_n);j++) if(classes[j]>=101 && classes[j]<=104) return SC_ERR_ARG;
    if((op==0 || op==1) && n==0 && ns>0) return SC_ERR_ARG;
    if(op==6 && in[4] && in[5]) return SC_ERR_ARG; // no vertexuse on the standalone vertex
    if(op==4 && n!=other_n) return SC_ERR_ARG; // paired merge indexes both arrays together
    if(op==3 && (n==0 || other_n==0)) return SC_ERR_ARG; // Homogenize reads interval zero.
    SmContext ctx;
    std::unique_ptr<SmCurve> curve;
    if(shape!=1) curve.reset(new (&ctx) SmLine(SmPoint3d(0,0,0),SmPoint3d(scale,0,0),dim,&ctx));
    else curve.reset(new (&ctx) SmCircle(SmPoint3d(0,0,0),SmVector3d(1,0,0),SmVector3d(0,1,0),SmExtent1d(0,360),scale,dim,&ctx));
    auto target_curve=std::unique_ptr<SmCurve>(new (&ctx) SmLine(SmPoint3d(0,0,0),SmPoint3d(scale*std::cos(angle),scale*std::sin(angle),0),dim,&ctx));
    // Quadratic x(t)=.5+(t-.5)^2: the first tangent vanishes at t=.5.
    auto stationary = [&](double rotation) -> std::unique_ptr<SmCurve> {
        gw_CURVE* c=sm_AllocateNurbCurve(2,2,5);
        if(!c) return nullptr;
        const double x[]={.75,.25,.75};
        for(int j=0;j<3;j++) {
            c->pol->Pw[j].x=x[j]*scale*std::cos(rotation);
            c->pol->Pw[j].y=x[j]*scale*std::sin(rotation);
            c->pol->Pw[j].z=0; c->pol->Pw[j].w=1;
        }
        for(int j=0;j<6;j++) c->knt->U[j]=j<3?0:1;
        return std::unique_ptr<SmCurve>(new (&ctx) SmBSplineCurve(c,dim,FALSE));
    };
    if(shape==2 || shape==4) curve=stationary(0);
    if(shape==3 || shape==4) target_curve=stationary(angle);
    if(!curve || !target_curve) return SC_ERR_ARG;
    auto plane=std::unique_ptr<SmPlane>(new (&ctx) SmPlane(SmPoint3d(0,0,0),SmVector3d(0,0,1),&ctx));
    // An owner is required even for an empty IntersectVertices call (GetOwner()->IsKindOf).
    ScFace face(plane.get());
    plane->SetOwner(&face);
    ScVertex vertex(angle*scale,tol);
    SmObject* target=target_kind==0 ? static_cast<SmObject*>(target_curve.get()) : target_kind==1 ? static_cast<SmObject*>(plane.get()) : target_kind==2 ? static_cast<SmObject*>(&vertex) : static_cast<SmObject*>(&face);
    // InsertSolutions' plane zone tolerance would follow its owner; keep it unowned
    // except during IntersectVertices, where ownership is required by native code.
    if(op!=6) plane->SetOwner(NULL);
    auto uv1=std::unique_ptr<SmLine>(new (&ctx) SmLine(SmPoint3d(0,0,0),SmPoint3d(scale,0,0),2,&ctx));
    auto uv2=std::unique_ptr<SmLine>(new (&ctx) SmLine(SmPoint3d(0,0,0),SmPoint3d(0,scale,0),2,&ctx));
    // ExtractGeometry slices these to SmBSplineCurve without making their NURBs.
    // Supply initialized bases, as a real UV trim does, before that native cast.
    if(op==3 && (uv1->MakeNurb()!=SM_SUCCESS || uv2->MakeNurb()!=SM_SUCCESS)) return SC_ERR_ARG;
    SmCurveClassification cc[2];
    for(int side=0;side<2;side++) {
        auto& c=cc[side]; const double* b=side ? other_bounds : bounds; int count=side?other_n:n;
        c.SetCurve(curve.get(),FALSE); SmTol::SetZoneTol3d(&c,SmZoneTol3d(tol)); c.SetIgnoreTangency(in[3]);
        if(op==3 && ((side==0 && in[4]) || (side==1 && in[5]))) c.SetUVCurve1(side==0?uv1.get():uv2.get());
        c.SetSize(count); c.SetInterval(SmExtent1d(b[0],b[count]));
        for(int j=0;j<count;j++) {
            // Match the Rust fixture's explicitly owned interval construction;
            // SetSize alone default-constructs intervals without the source zone.
            c[j]=SmCurveInterval(SmExtent1d(b[j],b[j+1]),&ctx,&c);
            SmPointClassification* pcs[]={&c[j].m_vStart,&c[j].m_vMid,&c[j].m_vEnd};
            for(int k=0;k<3;k++) {
                int cl=classes[(side?3*n:0)+3*j+k];
                if(cl!=100 && cl!=110 && cl!=106 && cl!=107 && cl!=104 && cl!=102) return SC_ERR_ARG;
                pcs[k]->SetClassObject((SmPointClassificationType)cl,(cl==100 || cl==110)?NULL:target);
            }
        }
    }
    SmSolutionArray solutions;
    for(int j=0;j<ns;j++) {
        const double* s=sols+7*j; SmSolution sol;
        sol.m_eSolutionType=(SmSolutionType)(int)s[0]; sol.m_lNumObjects=2; sol.m_lNumVariables=target_kind==3?3:2;
        sol.m_apObjects[0]=curve.get(); sol.m_apObjects[1]=target;
        if(op==1 && index==1) std::swap(sol.m_apObjects[0],sol.m_apObjects[1]);
        sol.m_vStart[0]=s[1]; sol.m_vStart[1]=s[2]; sol.m_vEnd[0]=s[3]; sol.m_vEnd[1]=s[4];
        sol.m_vStart.m_dSolutionValue=s[5]; sol.m_vEnd.m_dSolutionValue=s[6]; solutions.Add(sol);
    }
    // The copied helper converts sufficiently short ranges to points before its
    // tangency branch. Keep those valid calls; only reject ranges that remain ranges
    // and would dereference its null pEdgeCurve. This check uses the same native
    // endpoint and six-segment length tests, without changing the solution.
    if(op==0 && target_kind<=1 && in[3]) for(ULONG j=0;j<solutions.GetSize();j++) {
        SmSolution& s=solutions[j];
        if(s.m_eSolutionType!=SM_ST_RANGE_OF_VALUES || s.m_vStart[0]>s.m_vEnd[0] ||
           (s.m_vEnd[0]-s.m_vStart[0])/(bounds[n]-bounds[0])>=.05) continue;
        SmXSectTol3d xt=opt_tol<0 ? SmTol::GetXSectTol3d(SmZoneTol3d(tol),SmTol::GetZoneTol3d(target)) : SmXSectTol3d(opt_tol);
        SmPoint3d p0,p1,prm0,prm1; SmStatus st=s.GetPoint(0,p0,&prm0,FALSE); st &= s.GetPoint(0,p1,&prm1,TRUE);
        SmExtent1d interval(prm0[0]); interval.AddValue(prm1[0]);
        if(st!=SM_SUCCESS || !p0.CloserThan(xt,p1) || !(curve->ApproximateLength(interval,6)<xt)) return SC_ERR_ARG;
    }
    SmStatus status=SM_SUCCESS; std::vector<double> result;
    SmTArray<SmFace*> faces, faces2; SmTArray<SmEdge*> edges, edges2; SmTArray<SmVertex*> vertices, vertices2;
    if(op==0) { SmXSectTol3d xt(opt_tol); status=sc_cc_p5::sm_InsertSolutions(&cc[0],solutions,target,opt_tol<0?NULL:&xt); }
    if(op==1) status=cc[0].InsertTopologyIntersections(solutions,index);
    if(op==2) status=cc[0].FixupClassification();
    if(op==3) {
        SmTArray<SmCurve*> curves,uv1,uv2; SmTArray<SmPoint3d> points;
        status=cc[0].ExtractGeometry(ctx,cc[1],curves,in[4]?&uv1:NULL,in[5]?&uv2:NULL,in[6]?&points:NULL);
        result.insert(result.end(),{double(curves.GetSize()),double(uv1.GetSize()),double(uv2.GetSize()),double(points.GetSize())});
        for(ULONG j=0;j<curves.GetSize();j++) {put_curve(result,curves[j]); delete curves[j];}
        for(ULONG j=0;j<uv1.GetSize();j++) put_curve(result,uv1[j]);
        for(ULONG j=0;j<uv2.GetSize();j++) put_curve(result,uv2[j]);
        for(ULONG j=0;j<points.GetSize();j++) result.insert(result.end(),{points[j].x,points[j].y,points[j].z});
        for(ULONG j=0;j<uv1.GetSize();j++) delete uv1[j];
        for(ULONG j=0;j<uv2.GetSize();j++) delete uv2[j];
    }
    if(op==4) status=cc[0].MergeClassifications(cc[1],in[4],&faces,&faces2,&edges,&edges2,&vertices,&vertices2);
    if(op==5) status=cc[0].MergeIntervals(&faces,&edges,&vertices,NULL,in[4]);
    if(op==6) { if(in[4]) vertices.Add(&vertex); status=cc[0].IntersectVertices(vertices,*plane,in[5],in[6]); }
    if(op==4 || op==5) result.insert(result.end(),{double(faces.GetSize()),double(edges.GetSize()),double(vertices.GetSize()),double(faces2.GetSize()),double(edges2.GetSize()),double(vertices2.GetSize())});
    result.insert(result.begin(),status==SM_SUCCESS?0:(double)status);
    put_cc(result,cc[0],target); put_cc(result,cc[1],target);
    for(ULONG j=0;j<solutions.GetSize();j++) {
        auto& s=solutions[j]; result.insert(result.end(),{double(s.m_eSolutionType),s.m_vStart[0],s.m_vStart[1],s.m_vEnd[0],s.m_vEnd[1],s.m_vStart.m_dSolutionValue,s.m_vEnd.m_dSolutionValue});
    }
    plane->SetOwner(NULL);
    return sc_copy_out(result.data(),result.size(),out,cap,out_len);
    SC_CATCH
}

// The verbatim sm_InsertSolutions above, for the B-rep cases of
// intersect_curve_class_p5_topo.cpp (a file-static function cannot be linked against).
SmStatus sc_cc_p5_insert_solutions(SmCurveClassification* pCurveClassification, SmSolutionArray& rSolutions,
                                   SmObject* pObj, SmXSectTol3d* pOptXSectTol3d)
{
    return sc_cc_p5::sm_InsertSolutions(pCurveClassification, rSolutions, pObj, pOptXSectTol3d);
}
