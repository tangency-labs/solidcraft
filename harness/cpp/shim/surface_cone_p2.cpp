// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.cone_p2`.
// Wraps the SmCone members owned by the unit (source/SMLib/src/SmCone.cpp):
// CreateOffsetSurface, IsNurbSurfaceCone and RebuildSTEPFromNURBParameters, plus a verbatim
// copy of the file-statics sm_IsConePointInTrimDomain and sm_GetEllipseTrimIntervals.
//
// One entry point, sc_surface_cone_p2_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/surface_cone_p2.rs, which validates every
// input. Cones are built as in the surface.cone_p1 shim (the same block layout) and their
// state is written in the same layout. Statuses are written raw (SM_SUCCESS is 1000).
//
// Inputs the upstream code cannot handle without undefined behaviour are answered with a
// record `SC_ERR_ARG` (-1002) instead of a call (the Rust side tests the same condition):
//  * CreateOffsetSurface dereferences the result of CreateCanonical unchecked in its apex
//    truncation branch (a NULL cone crashes, a non-finite height recurses without bound):
//    that construction is tried first with the same formulas;
//  * sm_GetEllipseTrimIntervals reads the parameter of a failed smgu_LinePlaneIntersect
//    (seam line parallel to the ellipse plane, or of zero length): it is computed first
//    with the same calls.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmCircle.h>
#include <SmCone.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCylinder.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmGeomUtility.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmSurfOfRevolution.h>
#include <SmSurface.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <cmath>
#include <vector>

// ---- verbatim copy of file-static upstream code (SmCone.cpp) ---------------------------

/*******************************************************************//**
PURPOSE: Test whether a point on a cone lies inside a STEP trim domain
            of that cone.

NOTES: SmCone::STEPInversion ignores its domain argument and only
       reports whether the point is inside the whole cone, so the
       trim domain limits are checked here.
***********************************************************************/
static SmBoolean sm_IsConePointInTrimDomain
  (const SmCone      & crCone,          // in : cone the point lies on
   const SmExtent2d  & crAnalUVDomain,  // in : STEP trim domain of the cone
   const SmPoint3d   & crPoint,         // in : point on the cone
   double              dTol)            // in : 3d tolerance
{
  SmPoint2d      sSTEPUV ;
  SmLocationType eLoc ;
  crCone.STEPInversion(crAnalUVDomain, crPoint, dTol, sSTEPUV, eLoc) ;
  if(eLoc == SM_LT_EXTERIOR)
    { return FALSE ; }

  if(!crAnalUVDomain.GetVInterval().ContainsValue(sSTEPUV.y, dTol))
    { return FALSE ; }

  // the sweep angle is arbitrary at a pole
  return(   eLoc == SM_LT_POLE
         || crAnalUVDomain.GetUInterval().ContainsPeriodicValue(sSTEPUV.x, 360.0)) ;

} // end sm_IsConePointInTrimDomain

/*******************************************************************//**
PURPOSE: Find the set of intervals on an ellipse known to be
            on the cone's surface that are inside given cone's 
            trim boundaries.

NOTES: return intervals are in the ellipse's anal domain
***********************************************************************/
static
void sm_GetEllipseTrimIntervals
  (const SmContext     &crContext,            // in : for temp object construction
   const SmPoint3d     &rEllipseOrigin,       // in : origin            of ellipse
   const SmVector3d    &rEllipseXAxis,        // in : xAxis             of ellipse
   const SmVector3d    &rEllipseYAxis,        // in : yAxis             of ellipse
   double               dEllipseXRadius,      // in : x radius          of ellipse
   double               dEllipseYRadius,      // in : y radius          of ellipse
   double               dEllipseStartAngDeg,  // in : start angle (deg) of ellipse
   double               dEllipseEndAngDeg,    // in : end angle (deg)   of ellipse
   const SmCone        &crCone,               // in : Cone with trim boundaries
   const SmExtent2d    &crUVDomain,           // in : nurb trim boundaries for cone
   double               dTol,                 // in : approximation tolerance
   SmTArray<SmExtent1d> &rIvls)               // out: array of ellipse intervals inside trimmed cone
{
  // gwc:NOT DONE YET - this function is not debugged and
  //     it does not yet trim to the input dEllipseStartAngDeg and
  //     dEllipseEndAngDeg.

  // init output
  rIvls.ReSet() ;

  // find ellipse/coneTrimBoundary intersections
  SmTArray<double> dXSectAngle(6,NULL,6) ;  // coneBoundary xSect Points
  SmBoolean        bInside[6] ;             // TRUE = ith Ivl is inside trim boundaries

  // ellipse locals
  ULONG ii, lCnt = 0 ;               
  SmVector3d       sEllipseZ       = rEllipseXAxis * rEllipseYAxis ;
  SmExtent1d       sEllipseIvl (dEllipseStartAngDeg, dEllipseEndAngDeg) ;
  SmEllipse        sFullEllipse(rEllipseOrigin, rEllipseXAxis, rEllipseYAxis,
                                SmExtent1d(0.0, 360.0),
                                dEllipseXRadius, dEllipseYRadius,
                                3, &crContext) ;
  // map given NurbDomain into StepDomain - m_bInsideOut swaps Nurb GenCurve Min/Max Points
  SmExtent2d       sAnalUVDomain ;
  crCone.ConvertDomainFromNURBSToSTEP(crUVDomain, sAnalUVDomain) ;
  SmExtent1d       sSweepIvl     = sAnalUVDomain.GetUInterval() ;

  // cone locals - bottom and top trim circles sit at the v limits of the trim domain,
  //               which may be a subset of the whole cone
  SmVector3d       sConeAxis     = crCone.GetPosition().GetZAxis();
  double           dBotHeight    = sAnalUVDomain.GetVMin() ;
  double           dTopHeight    = sAnalUVDomain.GetVMax() ;
  double           dBotRadius    = crCone.GetRadius(dBotHeight) ;
  double           dTopRadius    = crCone.GetRadius(dTopHeight) ;
  SmPoint3d        rCenterBot    = crCone.GetPosition().GetOriginRef() + dBotHeight * sConeAxis;
  SmPoint3d        sCenterTop    = crCone.GetPosition().GetOriginRef() + dTopHeight * sConeAxis;
  //SmBoolean        bIsClosed     = sSweepIvl.IsClosed(360.0) ;  //unused
  //double           dStartAng     = sSweepIvl.GetMin();        //unused
  //double           dEndAng       = sSweepIvl.GetMax();        //unused

  // get cone corners [StartLine start, StartLine end, EndLine start EndLine end]
  SmPoint3d sCorners[4] ;
  crCone.EvaluateSTEPPoint(sAnalUVDomain.Evaluate(0.0, 0.0), sCorners[0]) ;
  crCone.EvaluateSTEPPoint(sAnalUVDomain.Evaluate(0.0, 1.0), sCorners[1]) ;
  crCone.EvaluateSTEPPoint(sAnalUVDomain.Evaluate(1.0, 0.0), sCorners[2]) ;
  crCone.EvaluateSTEPPoint(sAnalUVDomain.Evaluate(1.0, 1.0), sCorners[3]) ;
  SmVector3d sStartLineVec = sCorners[1] - sCorners[0] ;
  SmVector3d sEndLineVec   = sCorners[3] - sCorners[2] ;

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
static ULONG lCount      = 1 ; lCount++ ;
static ULONG lDebugCount = 0 ;
  if (bDebugMe || lCount == lDebugCount) 
    {
      SmBSplineCurve *pCirc1 = NULL ; 
      if(SM_IS_ZERO(dTopRadius))
           { SmBSplineCurve::CreateDegenerateCurve(crContext, 3, sCenterTop, pCirc1) ; }
      else { pCirc1 = new (crContext) SmCircle(sCenterTop, 
                                               crCone.GetPosition().GetXAxisRef(), 
                                               crCone.GetPosition().GetYAxisRef(),
                                               sSweepIvl, dTopRadius, 3, &crContext) ;
           }
      SmObjDelete sClean1(pCirc1) ;

      SmBSplineCurve *pCirc2 = NULL ; 
      if(SM_IS_ZERO(dBotRadius))
           { SmBSplineCurve::CreateDegenerateCurve(crContext, 3, rCenterBot, pCirc2) ; }
      else { pCirc2 = new (crContext) SmCircle(rCenterBot, 
                                               crCone.GetPosition().GetXAxisRef(), 
                                               crCone.GetPosition().GetYAxisRef(),
                                               sSweepIvl, dBotRadius, 3, &crContext) ;
           }
      SmObjDelete sClean2(pCirc2) ;
      SmFace *pFace = (SmFace *)crCone.GetFace() ;
      SmBrep *pBrep = pFace ? pFace->GetBrep() : NULL ;
      SmTArray<SmExtent1d> sEllipseNurbIvl ;
      sFullEllipse.ConvertIvlFromSTEPToNURBS(sEllipseIvl, sEllipseNurbIvl) ;

      smgfx_Erase();
      smgfx_SetLook(1,2, 0,0,1) ; if(pBrep) { pBrep->Draw(TRUE) ; } sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, 0,1,1) ; crCone.DrawUV() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 1,0,0) ; sFullEllipse.Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(6,7, 0,0,0) ; for(ii=0;ii<sEllipseNurbIvl.GetSize();ii++)
                                    { sFullEllipse.Draw(&sEllipseNurbIvl[ii]) ; sm_GraphicsLoop() ; }
      smgfx_SetLook(3,4, 0,1,0) ; (dEllipseXRadius * rEllipseXAxis).Draw(&rEllipseOrigin) ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 0,0,1) ; (dEllipseYRadius * rEllipseYAxis).Draw(&rEllipseOrigin) ; sm_GraphicsLoop() ;
      smgfx_SetLook(5,6, 1,1,0) ; sStartLineVec.Draw(&sCorners[0]) ; sm_GraphicsLoop() ;
      smgfx_SetLook(5,6, 1,1,0) ; sEndLineVec.Draw(&sCorners[2]) ; sm_GraphicsLoop() ;
      smgfx_SetLook(5,6, 1,1,0) ; pCirc1->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(5,6, 1,1,0) ; pCirc2->Draw() ; sm_GraphicsLoop() ;
      sm_GraphicsLoop();
    }
#endif

  // get ellipse/TopCircle(max 2), ellipse/BotCircle(max 2) 
  //     ellipse/StartSeamLine(max 1), ellipse/EndSeamLine(max 1) intersections
  ULONG lNumTsect1, lNumTsect2 ;
  SmPoint3d sTsectPoints[6] ;
  double dParam[2] ;
  smgu_CirclePlaneIntersect(dTol, 
                            dTopRadius, sCenterTop, sConeAxis,
                            rEllipseOrigin, sEllipseZ, 
                            lNumTsect1, sTsectPoints) ;
  smgu_CirclePlaneIntersect(dTol, 
                            dBotRadius, rCenterBot, sConeAxis,
                            rEllipseOrigin, sEllipseZ, 
                            lNumTsect2, sTsectPoints+2) ;
  smgu_LinePlaneIntersect  (sCorners[0], sStartLineVec,
                            rEllipseOrigin, sEllipseZ,
                            dParam[0]) ;
  smgu_LinePlaneIntersect  (sCorners[2], sEndLineVec,
                            rEllipseOrigin, sEllipseZ,
                            dParam[1]) ;

  // Check for degenerate solutions, within tol. [B10]
  // (smgu_CirclePlaneIntersect() doesn't do that.)
  if ( lNumTsect1 == 2 )
  {
      if ( sTsectPoints[0].DistanceBetween( sTsectPoints[1] ) < dTol )
        { lNumTsect1 = 1; }
  }
  if ( lNumTsect2 == 2 )
  {
      if ( sTsectPoints[2].DistanceBetween( sTsectPoints[3] ) < dTol )
        { lNumTsect2 = 1; }
  }

  // circle/Plane xSect lNumTSect == 0 is a miss - nothing to do
  //                    lNumTSect == 1 is a tangent point - not put on in/out point list
  //                    lNumTSect == 2 is a trimmed section in/out boundary
  //                    lNumTSect == 3 is a coincident circle/plane 
  SM_ASSERT(   lNumTsect1 != 3 || lNumTsect2 != 3) ;
          
  // get ellipse angle in degrees for each XSectPoint and place in the in/out dXSectAngle array
  // exclude intersection points outside the cone sweep trim boundaries

  // Consider all 4 possible ConeCircle/Ellipse intersections for addition to dXSectAngle array
  SmPoint2d sConeSTEPUV ;
  SmLocationType eLoc ;
  for(ii=0;ii<4;ii++)
    {
      // only consider circle/ellipse intersections when there are two of them
      //      i.e. don't add point tangent intersections - they don't affect in/out behavior.
      //           don't add coincident intersections - they get trimmed to cone end lines later.
      // only add intersection points in the cone trim boundary
      if(   (ii < 2 && lNumTsect1 == 2)
         || (ii >=2 && lNumTsect2 == 2))
        {
          // map XSectPoint to ConeUV point
          crCone.STEPInversion(sAnalUVDomain, sTsectPoints[ii], dTol, sConeSTEPUV, eLoc) ;

          // when ConeUV is cone trim boundaries
          //      skip circle instersections on seams because the line intersections will catch seam intersections
          if(   eLoc == SM_LT_INTERIOR
             && sSweepIvl.ContainsPeriodicValue(sConeSTEPUV.x, 360.0))
            {
              // add ellipse angleDeg for intersection to in/out angle dXSectAngle array
              sFullEllipse.STEPInversion(sTsectPoints[ii], dXSectAngle[lCnt]) ;
              lCnt++ ;
            }
        } // end skip tangent intersection cases
    } // end iter every possible Circle/Ellipse xSect

  // for StartSeamLine/ellipse xSect
  //  GWC: tolerances to think about:
  //    ContainsPeriodicValue()  increased tolerance to account for common degree domains ;
  //        to double dScaledZero = SM_EFF_ZERO * 100.0 * (1.0 + dPeriod) ;
  //        and here we use a different value for lines
  //    This computation needs both functions to have the same behavior near corners.
  double dScaledZero = SM_EFF_ZERO * (1.0 + smos_Max(sCorners[0].GetMaxDimension(),
                                                     sCorners[3].GetMaxDimension())) ;

  // circle crossings within dTol of a seam line were skipped above,
  // so seam line crossings within dTol of the line ends must be kept here
  double dStartLineLen    = sStartLineVec.Length() ;
  double dEndLineLen      = sEndLineVec.Length() ;
  double dStartParamTol   = (dStartLineLen > SM_EFF_ZERO) ? smos_Max(dScaledZero, dTol / dStartLineLen) : dScaledZero ;
  double dEndParamTol     = (dEndLineLen   > SM_EFF_ZERO) ? smos_Max(dScaledZero, dTol / dEndLineLen)   : dScaledZero ;
  if(   (   lNumTsect1 == 3 || lNumTsect2 == 3)
     || (   dParam[0] >= 0.0 - dStartParamTol
         && dParam[0] <= 1.0 + dStartParamTol))
    {
      sTsectPoints[4] = sCorners[0] + dParam[0] * sStartLineVec;
      sFullEllipse.STEPInversion( sTsectPoints[4], dXSectAngle[lCnt] );
      lCnt += 1 ;
    }

  // for EndSeamLine/ellipse xsect
  if(   (   lNumTsect1 == 3 || lNumTsect2 == 3)
     || (   dParam[1] >= 0.0 - dEndParamTol
         && dParam[1] <= 1.0 + dEndParamTol))
    {
      sTsectPoints[5] = sCorners[2] + dParam[1] * sEndLineVec;
      sFullEllipse.STEPInversion( sTsectPoints[5], dXSectAngle[lCnt] );
      lCnt += 1 ;
    }

  // there should be no more than 6 intersections
  SM_ASSERT(lCnt <= 6) ;

  // arrive here when dXSectAngle array contains an ellipse angle for
  // every ConeTrimCurve/FullEllipse intersection where the FullEllipse goes in/out
  // of the ConeTrimDomain.  There should be an even number of these transitions
  // The ellipse trim range has not yet been applied.  When there are no intersections
  // the whole curve must lie outside the cone trim domain.
  if(lCnt < 2) 
    { return ; }

  // when there are intersections there should be an even number of them
  // unless ellipse intersects a corner exactly.
  //      SM_ASSERT(lCnt % 2 == 0) ;  

  // order the XSectPoints by angle
  smgu_ShellSort_double_array(dXSectAngle.GetDataArray(), lCnt) ;
  SM_ASSERT(dXSectAngle[lCnt-1] - dXSectAngle[0] <= 360.0) ;

  // now classify the segment midPoints as in/out
  // add one interval to output for every inside segment
  double dScaledAngZero = SM_EFF_ZERO * 100.0 * 360.0 ;
  ULONG iThis, iNext, iPrev;
  for( iThis=0; iThis<lCnt; iThis++ )
    {
      iNext = (iThis+1) % lCnt;

      // get interval start/stop angles
      double dStartAngDeg = dXSectAngle[iThis] ;
      double dEndAngDeg   = dXSectAngle[iNext] ;

      // the last interval includes the zero angle
      // modify the startPoint so that the interval is
      // within [-360, 360] and Start < End
      if ( iNext == 0 ) { dStartAngDeg -= 360.0; }

      // mark degenerate intervals as unknown for now - they will be classified
      // based on their neighbor classifications once they are available
      if((dEndAngDeg - dStartAngDeg < dScaledAngZero))
        { bInside[iThis] = UNSURE ; 
          continue ;
        }

      // classify nonDegenerate interval midPoint
      SmPoint3d sSegmentMidPoint ;
      double sSegmentMidAngDeg = (dEndAngDeg + dStartAngDeg) / 2.0 ;
      sFullEllipse.EvaluateSTEPPoint(sSegmentMidAngDeg, sSegmentMidPoint) ;

      // Remember if segment MidPointUV is within cone trim boundaries
      bInside[iThis] = sm_IsConePointInTrimDomain(crCone, sAnalUVDomain, sSegmentMidPoint, dTol) ;

    } // end iter every interval to classify it

  // add one interval to output for every inside segment
  SmExtent1d sUnTrimmedIvl ;
  SmTArray<SmExtent1d> sTrimIvls ;

  for ( iThis=0; iThis<lCnt; iThis++ )
    {
      iNext = ( iThis+1 )      % lCnt;
      iPrev = ( iThis+lCnt-1 ) % lCnt;

      // get interval start/stop angles
      //   the last interval includes the zero angle
      //   modify the startPoint so that the interval is
      //   within [-360, 360] and Start < End.
      double dStartAngDeg = (iNext == 0) ? dXSectAngle[iThis] - 360.0
                                         : dXSectAngle[iThis] ;
      double dEndAngDeg   = dXSectAngle[iNext] ;

      // when the segment is inside
      if(   bInside[iThis] == TRUE   // nonDegnerate Ivl classification
         || (   bInside[iThis] == UNSURE  // Degenerate IVl is inside
             && bInside[iPrev] == FALSE   // when both neighbor intervals 
             && bInside[iNext] == FALSE)) //      are outside
        {
          // make and trim interval to ellipse interval
          // (Don't use SetMinMax, start ang can be > end ang, by machine noise.
          //   SetMinMax checks '>' without tol, and causes trouble if bad.)
          sUnTrimmedIvl.Init();
          sUnTrimmedIvl.AddValue( dStartAngDeg );
          sUnTrimmedIvl.AddValue( dEndAngDeg );

          sEllipseIvl.IntersectPeriodic(sUnTrimmedIvl, 360.0, sTrimIvls, FALSE) ;

          // add every trim invl to output
          rIvls.Append(sTrimIvls) ;                

        } // end segment inside check

    } // end iter every ellipse segment
  
  // all done

#ifdef SM_DEBUG_CODE
  if(bDebugMe || lDebugCount == lCount)
    {
      dXSectAngle.Dump() ;
      rIvls.Dump() ;

      // Build intersection ellipse - get face and Brep pointers
      SmPoint3d sPoint ;
      SmFace *pThisFace  = ((SmFace *)crCone.GetFace()) ;
      SmBrep *pThisBrep  = pThisFace  ? pThisFace->GetBrep()  : NULL ;

      // draw Breps(Blue,Green), Surfaces(cyan,yellow), faces(black), 
      // ellipse(red), TrimPoints(blue)
      smgfx_Erase() ; 
      smgfx_SetLook(1,2, 0,0,1) ; if(pThisBrep)  { pThisBrep->Draw(TRUE) ; }  sm_GraphicsLoop() ; 
      smgfx_SetLook(1,2, 0,1,1) ; crCone.DrawUV() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 0,1,0) ; (dEllipseXRadius * rEllipseXAxis).Draw(&rEllipseOrigin) ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, 0,0,1) ; (dEllipseYRadius * rEllipseYAxis).Draw(&rEllipseOrigin) ; sm_GraphicsLoop() ;
      smgfx_SetLook(6,7, 0,0,1) ; 

      for(ii=0;ii<lCnt;ii++) { sFullEllipse.EvaluateSTEPPoint(dXSectAngle[ii], sPoint) ;
                               smgfx_SetLook(8,9, 0,0,1) ; sPoint.Draw() ; sm_GraphicsLoop() ;
                             }
      for(ii=0;ii<4;ii++) { if(   (ii < 2 && lNumTsect1 == 2)
                               || (ii >=2 && lNumTsect2 == 2)
                               || (ii >=4))
                              { smgfx_SetLook(10,11, 1,0,0) ; sTsectPoints[ii].Draw() ; sm_GraphicsLoop() ; }
                          }
      smgfx_SetLook(10,11, 1,0,0) ; (sCorners[0] + dParam[0] * sStartLineVec).Draw() ; sm_GraphicsLoop() ; 
      smgfx_SetLook(10,11, 1,0,0) ; (sCorners[2] + dParam[1] * sEndLineVec).Draw() ; sm_GraphicsLoop() ;  
      for(ii=0;ii<rIvls.GetSize();ii++) { 
                                          ULONG jj;
                                          SmExtent1d sStepIvl = rIvls[ii] ;
                                          SmTArray<SmExtent1d> sNurbIvl ;
                                          sFullEllipse.ConvertIvlFromSTEPToNURBS(sStepIvl, sNurbIvl) ;
                                          for(jj=0;jj<sNurbIvl.GetSize();jj++)
                                            { smgfx_SetLook(8,9, 0,1,0) ; sFullEllipse.Draw(&sNurbIvl[jj]) ; sm_GraphicsLoop() ; }
                                        }
      sm_GraphicsLoop() ;
    }
#endif // SM_DEBUG_CODE

} // end sm_GetEllipseTrimIntervals

// ---- shim -------------------------------------------------------------------------------

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_cone_p2.rs).
enum : int32_t
{
    OP_OFFSET = 1,
    OP_RECOGNIZE = 2,
    OP_REBUILD = 3,
    OP_TRIM = 4,
};

// Construction modes (the surface.cone_p1 shim's).
enum : int32_t
{
    BM_CANONICAL_SEMI = 0, // CreateCanonical(O, bot, semi, pOptHeight, swap, inside)
    BM_CANONICAL = 1,      // CreateCanonical(O, bot, top, height, swap, inside)
    BM_CYL_CANONICAL = 2,  // SmCylinder::CreateCanonical(O, radius)
    BM_CONE_UNBOUNDED = 3, // SmCone(o, x, y, base, semi, swap, inside) + MakeNurb
    BM_CONE_BOUNDED = 4,   // SmCone(o, x, y, bot, top, start, end, h, swap, inside, mngc) + MakeNurb
    BM_CYL_BOUNDED = 5,    // SmCylinder(o, x, y, r, start, end, h, swap, inside, mngc) + MakeNurb
    BM_CYL_UNBOUNDED = 6,  // SmCylinder(o, x, y, r, swap, inside) + MakeNurb
};

// NURB edits (see the FFI module docs).
enum : int32_t
{
    ED_NONE = 0,
    ED_MOVE_LAST = 1,  // R p + w t on the last generator-direction row
    ED_SCALE_LAST = 2, // (x, y, z, w) * f on the last generator-direction row
    ED_MOVE_ALL = 3,   // R p + w t on every control point
    ED_FLATTEN = 4,    // z = 0 on every control point
    ED_SCALE_LAST_XY = 5, // scale x/y on the last row, preserving z and weight
};

// Reaches the protected constructors and members.
struct ConePeek : SmCone
{
    static SmBoolean SmSurfOfRevolution::*planar() { return &ConePeek::m_bPlanarGenerator; }
    static SmPolarConversion SmSurfOfRevolution::*polar() { return &ConePeek::m_vPolarConverter; }
    static double SmCone::*base_radius() { return &ConePeek::m_dBaseRadius; }
    static double SmCone::*semi_angle() { return &ConePeek::m_dSemiAngleDeg; }
    static SmBoolean SmSurfOfRevolution::*make_nurb_gen() { return &ConePeek::m_bMakeNurbGenCurve; }
    ConePeek(const SmPoint3d& o, const SmVector3d& x, const SmVector3d& y, double base,
             double semi, SmBoolean swap, SmBoolean inside)
        : SmCone(o, x, y, base, semi, swap, inside)
    {
    }
    ConePeek(const SmPoint3d& o, const SmVector3d& x, const SmVector3d& y, double bot, double top,
             double start, double end, double h, SmBoolean swap, SmBoolean inside,
             SmBoolean mngc)
        : SmCone(o, x, y, bot, top, start, end, h, swap, inside, mngc)
    {
    }
};

struct CylPeek : SmCylinder
{
    CylPeek(const SmPoint3d& o, const SmVector3d& x, const SmVector3d& y, double r,
            SmBoolean swap, SmBoolean inside)
        : SmCylinder(o, x, y, r, swap, inside)
    {
    }
};

struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    bool flag() { return in[at++] != 0.0; }
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
};

double flg(bool v)
{
    return v ? 1.0 : 0.0;
}

void put_vec(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

// A surface block `mode oob O[3] X[3] Y[3] p[8]`. Writes the construction status; NULL on
// failure.
SmCone* read_cone(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    int64_t mode = rd.idx();
    bool oob = rd.flag();
    SmPoint3d org = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    double p[8];
    for (int i = 0; i < 8; i++)
        p[i] = rd.num();
    SmAxis2Placement a2p(org, x, y);
    SmCone* c = NULL;
    SmStatus stt = SM_SUCCESS;
    switch (mode)
    {
    case BM_CANONICAL_SEMI:
    {
        double h = p[3];
        stt = SmCone::CreateCanonical(ctx, a2p, p[0], p[1], c, p[2] != 0.0 ? &h : NULL,
                                      p[4] != 0.0 ? TRUE : FALSE, p[5] != 0.0 ? TRUE : FALSE);
        break;
    }
    case BM_CANONICAL:
        stt = SmCone::CreateCanonical(ctx, a2p, p[0], p[1], p[2], c, p[3] != 0.0 ? TRUE : FALSE,
                                      p[4] != 0.0 ? TRUE : FALSE);
        break;
    case BM_CYL_CANONICAL:
    {
        SmCylinder* cy = NULL;
        stt = SmCylinder::CreateCanonical(ctx, a2p, p[0], cy);
        c = cy;
        break;
    }
    case BM_CONE_UNBOUNDED:
        c = new (ctx) ConePeek(org, x, y, p[0], p[1], p[2] != 0.0 ? TRUE : FALSE,
                               p[3] != 0.0 ? TRUE : FALSE);
        stt = c->MakeNurb();
        break;
    case BM_CONE_BOUNDED:
        c = new (ctx) ConePeek(org, x, y, p[0], p[1], p[2], p[3], p[4], p[5] != 0.0 ? TRUE : FALSE,
                               p[6] != 0.0 ? TRUE : FALSE, p[7] != 0.0 ? TRUE : FALSE);
        stt = c->MakeNurb();
        break;
    case BM_CYL_BOUNDED:
        c = new (ctx) SmCylinder(org, x, y, p[0], p[1], p[2], p[3], p[4] != 0.0 ? TRUE : FALSE,
                                 p[5] != 0.0 ? TRUE : FALSE, p[6] != 0.0 ? TRUE : FALSE, &ctx);
        stt = c->MakeNurb();
        break;
    case BM_CYL_UNBOUNDED:
        c = new (ctx) CylPeek(org, x, y, p[0], p[1] != 0.0 ? TRUE : FALSE,
                              p[2] != 0.0 ? TRUE : FALSE);
        stt = c->MakeNurb();
        break;
    default: stt = SC_ERR_ARG; break;
    }
    o.push_back((double)stt);
    if (stt != SM_SUCCESS)
    {
        if (c)
            delete c;
        return NULL;
    }
    c->SetOutOfBoundsEnabled(oob ? TRUE : FALSE); // the surface and its generator
    return c;
}

// `has [type step0 step1 has_nurb [p n cpts(4 n) nk knots]]`.
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    o.push_back(flg(c != NULL));
    if (!c)
        return;
    o.push_back((double)c->GetType());
    SmExtent1d ivl = c->GetSTEPInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    const SmBSplineCurve* b =
        c->IsKindOf(SmBSplineCurve_TYPE) ? (const SmBSplineCurve*)c : NULL;
    const gw_CURVE* cur = b ? b->GetGwNurbPointer() : NULL;
    o.push_back(flg(cur != NULL));
    if (!cur)
        return;
    o.push_back((double)cur->p);
    o.push_back((double)(cur->pol->n + 1));
    for (gw_INDEX i = 0; i <= cur->pol->n; i++)
    {
        o.push_back(cur->pol->Pw[i].x);
        o.push_back(cur->pol->Pw[i].y);
        o.push_back(cur->pol->Pw[i].z);
        o.push_back(cur->pol->Pw[i].w);
    }
    o.push_back((double)(cur->knt->m + 1));
    for (gw_INDEX i = 0; i <= cur->knt->m; i++)
        o.push_back(cur->knt->U[i]);
}

// The cone state (the surface.cone_p1 shim's layout without the type strings).
void put_state(std::vector<double>& o, const SmCone* s)
{
    o.push_back((double)s->GetType());
    o.push_back(flg(s->IsKindOf(SmCylinder_TYPE)));
    o.push_back(flg(s->IsKindOf(SmCone_TYPE)));
    o.push_back(flg(s->IsCylinder()));
    o.push_back(s->*ConePeek::base_radius());
    o.push_back(s->*ConePeek::semi_angle());
    o.push_back(flg(s->GetInsideOut()));
    o.push_back(flg(s->GetSwapUV()));
    o.push_back(flg(s->*ConePeek::planar()));
    o.push_back(flg(s->*ConePeek::make_nurb_gen()));
    o.push_back(flg(s->IsBounded()));
    const SmAxis2Placement& pos = s->GetPosition();
    put_vec(o, pos.GetOriginRef());
    put_vec(o, pos.GetXAxisRef());
    put_vec(o, pos.GetYAxisRef());
    SmExtent2d d = s->GetSTEPUVDomain();
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
    o.push_back(s->GetBotRadius());
    o.push_back(s->GetTopRadius());
    SmExtent2d mx = s->GetMaxAnalyticDomain();
    o.push_back(mx.GetMin().x);
    o.push_back(mx.GetMin().y);
    o.push_back(mx.GetMax().x);
    o.push_back(mx.GetMax().y);
    SmBoolean cyl = FALSE;
    SmPoint3d apex(0.0, 0.0, 0.0);
    double apex_param = 0.0;
    SmStatus ast = s->GetApex(cyl, apex, apex_param);
    o.push_back((double)ast);
    o.push_back(flg(cyl));
    put_vec(o, apex);
    o.push_back(apex_param);
    // defined without a polar curve too: the point evaluations fail (NER in
    // ConvertToPolarParameter) and the ignored statuses leave initialised default points
    o.push_back(s->GetHeight());
    const SmPolarConversion& pc = s->*ConePeek::polar();
    o.push_back(flg(s->HavePolarConversion()));
    ULONG cnt = pc.GetAngleCount();
    o.push_back((double)cnt);
    if (cnt > 0)
    {
        o.push_back(pc.GetFirstAngle());
        o.push_back(pc.GetLastAngle());
    }
    put_curve(o, pc.GetCurve());
    put_curve(o, s->GetGenCurve());
    const gw_SURFACE* sur = s->GetGwNurbPointer();
    o.push_back(flg(sur != NULL));
    if (!sur)
        return;
    o.push_back((double)sur->p);
    o.push_back((double)sur->q);
    o.push_back((double)(sur->net->n + 1));
    o.push_back((double)(sur->net->m + 1));
    for (gw_INDEX i = 0; i <= sur->net->n; i++)
        for (gw_INDEX j = 0; j <= sur->net->m; j++)
        {
            o.push_back(sur->net->Pw[i][j].x);
            o.push_back(sur->net->Pw[i][j].y);
            o.push_back(sur->net->Pw[i][j].z);
            o.push_back(sur->net->Pw[i][j].w);
        }
    o.push_back((double)(sur->knu->m + 1));
    for (gw_INDEX i = 0; i <= sur->knu->m; i++)
        o.push_back(sur->knu->U[i]);
    o.push_back((double)(sur->knv->m + 1));
    for (gw_INDEX i = 0; i <= sur->knv->m; i++)
        o.push_back(sur->knv->U[i]);
}

// An edit block `kind f R[9] t[3]` applied to the NURB control points (homogeneous).
void edit_nurb(Rd& rd, gw_SURFACE* sur, bool swap)
{
    int64_t kind = rd.idx();
    double f = rd.num();
    double r[9];
    for (int i = 0; i < 9; i++)
        r[i] = rd.num();
    SmVector3d t = rd.vec();
    if (kind == ED_NONE || sur == NULL)
        return;
    for (gw_INDEX i = 0; i <= sur->net->n; i++)
        for (gw_INDEX j = 0; j <= sur->net->m; j++)
        {
            // the generator direction is u with swap_uv, else v
            bool last = swap ? i == sur->net->n : j == sur->net->m;
            gw_CPOINT& p = sur->net->Pw[i][j];
            double w = p.w == NL_NOW ? 1.0 : p.w;
            if ((kind == ED_MOVE_LAST && last) || kind == ED_MOVE_ALL)
            {
                double x = p.x;
                double y = p.y;
                double z = p.z;
                p.x = r[0] * x + r[1] * y + r[2] * z + w * t.x;
                p.y = r[3] * x + r[4] * y + r[5] * z + w * t.y;
                p.z = r[6] * x + r[7] * y + r[8] * z + w * t.z;
            }
            else if (kind == ED_SCALE_LAST && last)
            {
                p.x *= f;
                p.y *= f;
                p.z *= f;
                if (p.w != NL_NOW)
                    p.w *= f;
            }
            else if (kind == ED_FLATTEN)
            {
                p.z = 0.0;
            }
            else if (kind == ED_SCALE_LAST_XY && last)
            {
                p.x *= f;
                p.y *= f;
            }
        }
}

// CreateOffsetSurface's apex truncation, its CreateCanonical tried first with the same
// formulas: false when it fails or its height is not finite.
bool offset_defined(const SmContext& ctx, const SmCone* s, double dist, double tol)
{
    SmApproxTol3d dThisApproxTol3d(tol);
    SmBoolean swap = s->GetSwapUV();
    SmBoolean inside = s->GetInsideOut();
    double dOffsetSign = 1.0 * (swap ? -1.0 : 1.0) * (inside ? -1.0 : 1.0);
    const SmAxis2Placement& pos = s->GetPosition();
    const SmVector3d& crOrigin = pos.GetOriginRef();
    const SmVector3d& crXAxis = pos.GetXAxisRef();
    const SmVector3d& crYAxis = pos.GetYAxisRef();
    SmVector3d sZAxis = pos.GetZAxis();
    double dHeight = s->GetHeight();
    double semi = s->*ConePeek::semi_angle();
    double dSin = smos_Sine(semi * SM_PI / 180.0);
    double dCos = smos_Cosine(semi * SM_PI / 180.0);
    double dOffZDist = -dSin * dOffsetSign * dist;
    double dOffRadDist = dCos * dOffsetSign * dist;
    double dOldBotRadius = s->GetBotRadius();
    double dOldTopRadius = s->GetTopRadius();
    double dNewBotRadius = dOldBotRadius + dOffRadDist;
    double dNewTopRadius = dOldTopRadius + dOffRadDist;
    if (dNewBotRadius < dThisApproxTol3d && dNewTopRadius < dThisApproxTol3d)
        return true;
    if (!(dNewBotRadius < dThisApproxTol3d || dNewTopRadius < dThisApproxTol3d))
        return true;
    double dTanAng, dNewHeight;
    SmVector3d sBotOrigin;
    if (dNewBotRadius < dThisApproxTol3d)
    {
        dTanAng = dHeight / dOldTopRadius;
        dNewHeight = dNewTopRadius * dTanAng;
        sBotOrigin = crOrigin + (dHeight + dOffZDist - dNewHeight) * sZAxis;
        dNewBotRadius = 0.0;
    }
    else
    {
        dTanAng = dHeight / dOldBotRadius;
        dNewHeight = dNewBotRadius * dTanAng;
        sBotOrigin = crOrigin + dOffZDist * sZAxis;
        dNewTopRadius = 0.0;
    }
    if (!std::isfinite(dNewHeight) || !std::isfinite(sBotOrigin.x) ||
        !std::isfinite(sBotOrigin.y) || !std::isfinite(sBotOrigin.z))
        return false;
    SmAxis2Placement newPosition;
    newPosition.SetCanonical(sBotOrigin, crXAxis, crYAxis);
    SmCone* pOffCone = NULL;
    SmCone::CreateCanonical(ctx, newPosition, dNewBotRadius, dNewTopRadius, dNewHeight, pOffCone,
                            swap, inside);
    bool ok = pOffCone != NULL;
    delete pOffCone;
    return ok;
}

// The calls of sm_GetEllipseTrimIntervals whose failures it ignores before reading their
// outputs (smgu_LinePlaneIntersect): false when one fails.
bool trim_defined(const SmCone* s, const SmVector3d& x, const SmVector3d& y,
                  const SmPoint3d& origin, const SmExtent2d& dom)
{
    SmVector3d sEllipseZ = x * y;
    // status ignored as upstream (defined): a failed corner conversion leaves the default
    // domain, a reversed result is replaced by the NURB domain
    SmExtent2d sAnalUVDomain;
    s->ConvertDomainFromNURBSToSTEP(dom, sAnalUVDomain);
    SmPoint3d c[4];
    s->EvaluateSTEPPoint(sAnalUVDomain.Evaluate(0.0, 0.0), c[0]);
    s->EvaluateSTEPPoint(sAnalUVDomain.Evaluate(0.0, 1.0), c[1]);
    s->EvaluateSTEPPoint(sAnalUVDomain.Evaluate(1.0, 0.0), c[2]);
    s->EvaluateSTEPPoint(sAnalUVDomain.Evaluate(1.0, 1.0), c[3]);
    double p0, p1;
    return smgu_LinePlaneIntersect(c[0], c[1] - c[0], origin, sEllipseZ, p0) == SM_SUCCESS &&
           smgu_LinePlaneIntersect(c[2], c[3] - c[2], origin, sEllipseZ, p1) == SM_SUCCESS;
}

void cone_op(int32_t op, Rd& rd, const SmContext& ctx, SmCone* s, std::vector<double>& o)
{
    switch (op)
    {
    case OP_OFFSET:
    {
        double dist = rd.num();
        double tol = rd.num();
        if (!offset_defined(ctx, s, dist, tol))
        {
            o.push_back((double)SC_ERR_ARG);
            break;
        }
        SmSurface* off = NULL;
        SmStatus stt = s->CreateOffsetSurface(ctx, dist, SmApproxTol3d(tol), off);
        o.push_back((double)stt);
        o.push_back(flg(off != NULL));
        if (off != NULL)
        {
            if (off->IsKindOf(SmCone_TYPE))
                put_state(o, (const SmCone*)off);
            delete off;
        }
        break;
    }
    case OP_RECOGNIZE:
    {
        edit_nurb(rd, s->GetGwNurbPointer(), s->GetSwapUV() != FALSE);
        double scale = rd.num();
        SmBSplineSurface* test = new (ctx) SmBSplineSurface(*(SmBSplineSurface*)s);
        SmCone* found = NULL;
        SmBoolean res = SmCone::IsNurbSurfaceCone(ctx, test, found, scale);
        o.push_back((double)res);
        o.push_back(flg(found != NULL));
        if (found != NULL)
        {
            put_state(o, found);
            delete found;
        }
        delete test;
        break;
    }
    case OP_REBUILD:
    {
        edit_nurb(rd, s->GetGwNurbPointer(), s->GetSwapUV() != FALSE);
        SmStatus stt = s->RebuildSTEPFromNURBParameters();
        o.push_back((double)stt);
        put_state(o, s);
        break;
    }
    case OP_TRIM:
    {
        SmPoint3d origin = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double rx = rd.num();
        double ry = rd.num();
        double a0 = rd.num();
        double a1 = rd.num();
        // the trim domain: corner points at fractions of the natural domain
        double f0 = rd.num();
        double f1 = rd.num();
        double f2 = rd.num();
        double f3 = rd.num();
        SmExtent2d nat = s->GetNaturalUVDomain();
        SmExtent2d dom(nat.Evaluate(f0, f1), nat.Evaluate(f2, f3));
        double tol = rd.num();
        if (!trim_defined(s, x, y, origin, dom))
        {
            o.push_back((double)SC_ERR_ARG);
            break;
        }
        SmTArray<SmExtent1d> ivls;
        sm_GetEllipseTrimIntervals(ctx, origin, x, y, rx, ry, a0, a1, *s, dom, tol, ivls);
        o.push_back((double)SM_SUCCESS);
        o.push_back((double)ivls.GetSize());
        for (ULONG i = 0; i < ivls.GetSize(); i++)
        {
            o.push_back(ivls[i].GetMin());
            o.push_back(ivls[i].GetMax());
        }
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_surface_cone_p2_eval(int32_t op, const double* in, int64_t n_in,
                                          double* out, int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    SmCone* s = read_cone(rd, ctx, o);
    if (s)
    {
        cone_op(op, rd, ctx, s, o);
        delete s;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
