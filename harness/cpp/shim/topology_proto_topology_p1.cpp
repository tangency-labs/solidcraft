// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.proto_topology_p1`: SmOwningTopology (list operations),
// SmAssembly / SmAssemblyInstance (instances, destructors, types), SmCopyBrepMap, the
// proto topology of source/SMLib/src/SmProtoTopology.cpp (SmVertexDefinition,
// SmEdgeDefinition, SmProtoVertex, SmProtoEdge, SmProtoFace, SmProtoTopologyManager and the
// static helpers) and the transformation functions of source/SM_API/src/SmApiGeneral.cpp.
//
// Static functions of SmProtoTopology.cpp cannot be linked against, so they are VERBATIM
// copies of upstream SmProtoTopology.cpp (commit c9c5979) inside the namespace sc_pt_p1 (line
// ranges noted at each block; generated, do not edit by hand). sm_FindEDOnDegenFace is
// global in the library. The protected final edges of SmEdgeDefinition are reached through
// explicit instantiation (Rob).
//
// One entry point, sc_topology_proto_topology_p1_run(in, n_in, out, cap, out_len): a script
// of one of three kinds (0 list, 1 proto, 2 transform), documented in
// crates/solidcore-ffi/src/units/topology_proto_topology_p1.rs, which validates the static
// shape. Each op writes `op status outputs`; an op the shim refuses because it is undefined
// upstream (a dangling or null pointer it would dereference, a double delete, a list walk
// that would not terminate) writes `op -2` and ends the script (the difftest generator
// drops such ops). Objects live for the whole process unless upstream deletes them.
#include "sc_shim.h"
#include "support/primitive_record.h"
#include <SmApiPrimitives.h>
#include <SmApiQueries.h>

#include "StdAfx.h"
#include <SmApiGeneral.h>
#include <SmAxis2Placement.h>
#include <SmAssembly.h>
#include <SmAssemblyInstance.h>
#include <SmAttribute.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmCopyBrepMap.h>
#include <SmCurve.h>
#include <SmCompositeCurve.h>
#include <SmCrvOnSurf.h>
#include <SmCurveClass.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmEllipse.h>
#include <SmFace.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmLoopuse.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmOwningTopology.h>
#include <SmPlane.h>
#include <SmPointClass.h>
#include <SmPoly.h>
#include <SmProtoTopology.h>
#include <SmSolutionArray.h>
#include <SmTol.h>
#include <SmTopoTypes.h>
#include <SmTopologyIntersector.h>
#include <SmVertex.h>
#include <nurbs.h>

#include <algorithm>
#include <cmath>
#include <map>
#include <set>
#include <vector>

#include "sc_sm.h"

using sc::put_vec;

// global in the library (SmProtoTopology.cpp), not declared in a header
SmStatus sm_FindEDOnDegenFace(SmEdge* pED1E, SmEdge* pED2E, ULONG& rlCanDelete);

namespace sc_pt_p1
{
// clang-format off
// ---- verbatim: SmProtoTopology.cpp lines 52-59: sm_CanImprint
static SmBoolean sm_CanImprint( SmPointClassificationType eType )
{
  return(   eType == SM_PC_REGION
         || eType == SM_PC_FACE
         || eType == SM_PC_EDGE
         || eType == SM_PC_VERTEX) ;

} // end sm_CanImprint

// ---- verbatim: SmProtoTopology.cpp lines 66-73: sm_TopoDimension
static int sm_TopoDimension( SmTopology *pTopo )
{
  return(  pTopo->IsKindOf(SmVertex_TYPE) ? 0   // vertex
         : pTopo->IsKindOf(SmEdge_TYPE  ) ? 1   // edge
         : pTopo->IsKindOf(SmFace_TYPE  ) ? 2   // face
         : -1 ) ;                                 // error

} // end sm_TopoDimension

// ---- verbatim: SmProtoTopology.cpp lines 84-178: sm_FindPositionFromPCs
static SmStatus sm_FindPositionFromPCs
 (SmPointClassification & rPC1,          // in : Point Classification 1
  SmPointClassification & rPC2,          // in : Point Classification 2
  SmPoint3d             & rPosition,          // out: Avg of Surf1(UV1) and Surf2(UV2)
  double                & rdDeviation )  // out: Dist between Surf1(UV1) and Surf2(UV2)
{
  SmPoint3d sPt1, sPt2;

  // These should have been set in the SmVertexDefinition constructor.
  // More information was available at that point, so if that failed, we can't improve on it.
  SmBoolean bFoundPos1 = rPC1.AreParametersSet() ;
  SmBoolean bFoundPos2 = rPC2.AreParametersSet() ;

  // error - both Parameters are not Set
  if(!bFoundPos1 && !bFoundPos2 )
    { SER( SM_ERR) ; }

  if(bFoundPos1) { SER( rPC1.FindObjPoint3d( sPt1 )); }
  if(bFoundPos2) { SER( rPC2.FindObjPoint3d( sPt2 )); }

  // locals
  SmXSectTol3d dXSectTol3d = SmTol::GetXSectTol3d( rPC1.GetObject(), rPC2.GetObject()) ;

  // deviation = dist between PtClassifyObjects when both point classifies have ParametersSet
  //             else 0.0
  rdDeviation = (bFoundPos1 && bFoundPos2) ? sPt1.DistanceBetween( sPt2 ) : 0.0;

  // inform the public when PCGap exceeds XSectTol3d
  if(rdDeviation > dXSectTol3d)
    { 
      TCHAR sBuff[SM_TBLOCK_SIZE];
      smos_sprintf(sBuff, _T(" PointClassifications in disagreement about where to place a VertexDefinition \n\t Deviation [%16.16lf] Tolerance [%16.16lf] "), (double) rdDeviation, (double) dXSectTol3d) ; 
      SE_MSG( SM_ERR, sBuff) ;
    }

  // No:  rPosition = ( bFoundPos1 && bFoundPos2 ) ? 0.5 * ( sPt1 + sPt2 ) : ( bFoundPos1 ) ? sPt1 : sPt2;
  // Not just average: give lower-dimensional topology more weight.
  if(! bFoundPos1 )  // found sPt2 only
    {
      // See if we can find a position for rPC1 from sPt2.
      SmStatus eStat = rPC1.ComputePointParameters( sPt2) ;
      if(eStat == SM_SUCCESS  &&  rPC1.AreParametersSet() )
        {
          SER( rPC1.FindObjPoint3d( sPt1 ));
          bFoundPos1 = TRUE;
        }
      else
        {
          rPosition = sPt2;
        }
    } // end missing PC1 Parameters branch

  else if(! bFoundPos2 )  // found sPt1 only
    {
      // See if we can find a position for rPC2 from sPt1.
      SmStatus eStat = rPC2.ComputePointParameters( sPt1) ;
      if(eStat == SM_SUCCESS  &&  rPC2.AreParametersSet() )
        {
          SER( rPC2.FindObjPoint3d( sPt2 ));
          bFoundPos2 = TRUE;
        }
      else
        {
          rPosition = sPt1;
        }
    } // end missing PC2 Parameters branch

  // when both PCs have Parameters
  if(bFoundPos1 && bFoundPos2 )  // found both
    {
      SmTopology *pTopo1 = SM_CAST_PTR( SmTopology, rPC1.GetObject()) ;
      int iDim1 = ( pTopo1 != NULL ) ? sm_TopoDimension( pTopo1 ) : 3;  //  high dimension means low weight.
      SmTopology *pTopo2 = SM_CAST_PTR( SmTopology, rPC2.GetObject()) ;
      int iDim2 = ( pTopo2 != NULL ) ? sm_TopoDimension( pTopo2 ) : 3;  //  high dimension means low weight.

      // Give more weight to the one with lower dimension.
      if(iDim1 < iDim2 )
        { rPosition = sPt1; }
      else if(iDim2 < iDim1 )
        { rPosition = sPt2; }
      else
        {
          // Same dimension.  Give more weight to the one with the tighter gap.
          double dGap1 = rPC1.GetGap3d();
          double dGap2 = rPC2.GetGap3d();
          double dSum  = dGap1 + dGap2;
          double dFrac = ( dSum > SM_EFF_ZERO ) ? ( dGap1 / dSum ) : 0.5;
          rPosition = ( 1-dFrac ) * sPt1  +  dFrac * sPt2;
        }
    } // end both PCs have parameters check

  // all done
  return SM_SUCCESS;

} // end static sm_FindPositionFromPCs

// ---- verbatim: SmProtoTopology.cpp lines 279-329: sm_AreSameCurve
static SmBoolean sm_AreSameCurve
 (const SmCurve    * pCrv1,   // in : 1st Curve
  const SmExtent1d & crIvl1,  // in : 1st Curve Ivl
  const SmCurve    * pCrv2,   // in : 2nd Curve
  const SmExtent1d & crIvl2,  // in : 2nd Curve Ivl
  double             dTol )   // in : XSectTol3d value to check for coincidence
{
  // low work - same curve.
  if(pCrv1 == pCrv2)
    {
      // return TRUE when intervals are the same
      return(crIvl1 == crIvl2) ;
    }

  // Just a rough check.  If two intersection curves between two Breps have the same starts, 
  // ends, and midpoints, we'll assume they're meant to be the same Edge in the end.

  // locals
  double    dParam1 = crIvl1.GetMid() ;
  double    dGuess  = crIvl2.GetMid();
  double    dParam2 ;
  double    dDist ;
  SmPoint3d sMidPt1;
  SmBoolean bSuccess = FALSE;

  // Crv1 mid point
  pCrv1->EvaluatePoint( dParam1, sMidPt1) ;

  // Drop Crv1 mid point to Crv2 - get drop distance
  pCrv2->DropPoint(crIvl2,            // in : target curve allowed domain
                   sMidPt1,           // in : Point to drop to curve
                   NULL,              // in : For Snapping, NULL=NoSnap, Vec pointing to CrvInside from desired EndPt for snapping.
                                      //      when DropPt is within dDistTol in cpVecToCrvInside dir of EndPt, Snap to Ivl EndPt.
                                      //      Vec handles ambiguities and makes sure that curves are not just touching at the ends.
                   dTol,              // in : Used for SnapDist, and SM_SO_INTERSECT sXSectTol3d distance.
                                      //      sXSectTol3d = dDistTol ;      SM_SO_INTERSECT
                                      //      sXSectTol3d = SM_BIG_DOUBLE ; SM_SO_MINIMIZE, SM_SO_NORMALIZE
                                      //      sSnapTol3d  = dDistTol ;      SM_SO_MINIMIZE, SM_SO_NORMALIZE, SM_SO_INTERSECT
                   &dGuess,           // in : If given, call cheap LocalPointSolve() else call expensive GlobalPointSolve()
                   bSuccess,          // out: TRUE = found a drop point
                   dParam2,           // out: found drop curve param
                   dDist,             // out: found drop distance
                   SM_SO_INTERSECT) ; // in : SM_SO_MINIMIZE = allow nonNormal drops near endPoints
                                      //      SM_SO_INTERSECT= like Minimize but point must be within dDistTol
                                      //      SM_SO_NORMALIZE= exclude nonNormal drops near endPoints
                                      //      default:[SM_SO_MINIMIZE] to preserve original behavior

  // return TRUE when DropPoint() succeeded and DropDist < dTol                                       
  return ( bSuccess && dDist < dTol) ;

} // end sm_AreSameCurve

// ---- verbatim: SmProtoTopology.cpp lines 336-356: sm_AreConnected(Vertex, Face)
static SmBoolean sm_AreConnected( const SmVertex *pVtx, const SmFace *pFace )
{
  SmTArray< SmVertexuse* > sVUs;
  pVtx->GetVertexuses( sVUs) ;
  SmVertexuse *pVU=NULL;
  ULONG ii, lNumVUs = sVUs.GetSize();
  for(ii=0;ii<lNumVUs;ii++)
  {
      pVU = sVUs[ii];
      SmFaceuse *pFU = pVU->GetFaceuse();
      if(pFU )
      {
          SmFace *pF = pFU->GetFace();
          if(pF == pFace )
            { return TRUE; }
      }
  }

  return FALSE;

} // end sm_AreConnected( Vertex, Face )

// ---- verbatim: SmProtoTopology.cpp lines 363-378: sm_AreConnected(Edge, Face)
static SmBoolean sm_AreConnected( const SmEdge *pEdge, const SmFace *pFace )
{
  SmTArray< SmEdgeuse* > sEUs;
  pEdge->GetEdgeuses( sEUs) ;
  SmEdgeuse *pEU;
  ULONG ii, lNumEUs = sEUs.GetSize();
  for(ii=0;ii<lNumEUs;ii++)
  {
      pEU = sEUs[ii];
      if(pEU->GetFace() == pFace )
        { return TRUE; }
  }

  return FALSE;

} // end sm_AreConnected( Edge, Face )

// ---- verbatim: SmProtoTopology.cpp lines 385-397: sm_AreConnected(Vertex, Edge)
static SmBoolean sm_AreConnected( const SmVertex *pVertex, const SmEdge *pEdge )
{
  SmVertex *pVtx = pEdge->GetStartVertex();
  if(pVtx == pVertex )
    { return TRUE; }

  pVtx = pEdge->GetOtherVertex( pVtx) ;
  if(pVtx == pVertex )
    { return TRUE; }

  return FALSE;

} // end sm_AreConnected( Vertex, Edge )

// ---- verbatim: SmProtoTopology.cpp lines 409-458: sm_AreConnected(Topology, Topology)
static SmBoolean sm_AreConnected // rtn: TRUE = Topos are connected neighbors in the same Brep Topology Graph
 (const SmTopology * pTopo1,     // in : TgtTopology1 o
  const SmTopology * pTopo2)     // in : TgtTopology2 o
{
  if(pTopo1 == pTopo2 )
    { return TRUE; }

  const SmFace *pFace1 = dynamic_cast<const SmFace*> ( pTopo1) ;
  if(pFace1 != NULL )
  {
      const SmEdge *pEdge2 = dynamic_cast<const SmEdge*> ( pTopo2) ;
      if(pEdge2 != NULL )
        { return sm_AreConnected( pEdge2, pFace1) ; }

      const SmVertex *pVertex2 = dynamic_cast<const SmVertex*> ( pTopo2) ;
      if(pVertex2 != NULL )
        { return sm_AreConnected( pVertex2, pFace1) ; }
  }

  const SmEdge *pEdge1 = dynamic_cast<const SmEdge*> (pTopo1) ;

  if(pEdge1 != NULL )
  {
      const SmFace *pFace2 = dynamic_cast<const SmFace*> ( pTopo2) ;
      if(pFace2 != NULL )
        { return sm_AreConnected( pEdge1, pFace2) ; }

      const SmVertex *pVertex2 = dynamic_cast<const SmVertex*> ( pTopo2) ;
      if(pVertex2 != NULL )
        { return sm_AreConnected( pVertex2, pEdge1) ; }
  }

  const SmVertex *pVertex1 = dynamic_cast<const SmVertex*> ( pTopo1) ;

  if(pVertex1 != NULL )
  {
      const SmFace *pFace2 =  dynamic_cast<const SmFace*> ( pTopo2) ;
      if(pFace2 != NULL )
        { return sm_AreConnected( pVertex1, pFace2) ; }

      const SmEdge *pEdge2 =  dynamic_cast<const SmEdge*> ( pTopo2) ;
      if(pEdge2 != NULL )
        { return sm_AreConnected( pVertex1, pEdge2) ; }
  }

  // I've checked, these are ok.
  // WARN( _T("SmProto: sm_AreConnected() called on unexpected topology: should be investigated." ));
  return FALSE;

} // end sm_AreConnected

// ---- verbatim: SmProtoTopology.cpp lines 472-500: sm_WhichPVToCombine
static SmProtoVertex* sm_WhichPVToCombine
 (SmProtoEdge   * pOrigPE,     // in : Tgt PE being split
  double          dSplitParam, // in : Param of Split point
  SmProtoVertex * pSplitPV )   // in : If the ProtoEdge was split, then pSplitPV is non Null
                               //      NULL on input
{
  // when ProtoEdge was split, then pSplitPV is non Null - return it
  if(pSplitPV != NULL )
    { return pSplitPV ; }

  // pOrigPE was not split at dSplitParam.
  // That should mean that dSplitParam is one end of the PE's domain,
  // but there have been examples where it's outside the domain.
  // Check proximity to the start and end PVs.

  // locals
  SmPoint3d       sSplitPt ;
  SmProtoVertex * sStartPV = pOrigPE->GetStartProtoVertex() ;
  SmProtoVertex * sEndPV   = pOrigPE->GetEndProtoVertex() ;

  // Eval PE at SplitParam
  pOrigPE->Evaluate( dSplitParam, &sSplitPt) ;

  // return matching End PV or NULL for none
  if     (sStartPV->ContainsPoint(sSplitPt)) { return sStartPV ; }
  else if(sEndPV  ->ContainsPoint(sSplitPt)) { return sEndPV ; }
  else                                       { return NULL ; }

} // end static sm_WhichPVToCombine

// ---- verbatim: SmProtoTopology.cpp lines 4435-4470: sm_IsInterior
static SmBoolean sm_IsInterior
 (const SmCurve    * cpCurve,     // in : TgtCurve
  const SmExtent1d & crDomain,    // in : TgtCurve Ivl domain
  double             dParam,      // in : TgtParam to test
  SmZoneTol3d        sZoneTol3d ) // in : Max dist between a test point and Tgt Ivl end considered in the interior
{
  // locals
  // SmScaledZero sScaleZero = SmTol::GetScaledZero(crDomain) ;
  // SmZoneTol3d  sParamTol  = sScaleZero.val;
  SmPoint3d    sStartPt ;
  SmPoint3d    sEndPt ;
  SmPoint3d    sCrvPt ;

  // low work - When Param is outside of Curve domain - it's not interior
  if(FALSE == crDomain.ContainsValue( dParam,0.0) )
    { return FALSE ; }

  // next: Check 3d Pts. if this param's Pt3d is within ZoneTol3d dist of a crv endpoint it on the boundary not the interior

  // locals
  cpCurve->EvaluatePoint( dParam, sCrvPt) ;
  cpCurve->EvaluatePoint( crDomain.GetMin(), sStartPt) ;
  cpCurve->EvaluatePoint( crDomain.GetMax(), sEndPt) ;

  // param is Boundary not Interior when ParamPt3d is on the start boundary of the curve
  if(sCrvPt.CloserThan(sZoneTol3d, sStartPt) )
    { return FALSE ; }

  // param is Boundary not Interior when ParamPt3d is on the end boundary of the curve
  if(sCrvPt.CloserThan(sZoneTol3d, sEndPt) )
    { return FALSE ; }

  // all done
  return TRUE ;

} // end static sm_IsInterior

// ---- verbatim: SmProtoTopology.cpp lines 4489-4647: sm_AreOverlapping
static SmBoolean sm_AreOverlapping
 (const SmCurveInterval  & crCI,  // in : Tgt CurveInterval to test for Overlap with
  const SmEdgeDefinition & crED)  // in : Tgt EdgeDefinition 
{
  // check assumptions - gwc: I added this check - is it true?
  SM_ASSERT_MSG(   crED.GetSrcTopo(1) == crCI.m_vMid.GetObject()
                || crED.GetSrcTopo(2) == crCI.m_vMid.GetObject(), _T("sm_AreOverlapping CurveInteval and EdgeDefinition come from the same source assumption is broken - needs review")) ; 
  // locals
  const SmCurve   * cpCICurve = crCI.GetCurveClassification()->GetCurve();
        SmExtent1d  sCIDomain = crCI.GetInterval();
                    
  const SmCurve   * cpEDCurve = crED.GetXSectCurve3d();
        SmExtent1d  sEDDomain = crED.GetXSectInterval();

  // tolerance 
  SmZoneTol3d sZoneTol3d = smos_Max( SmTol::GetSrcZoneTol3d( &crCI ), crED.GetTolerance()) ;

  // Drop ED->Curve Start and End Pts to CI->CurveClassification->Curve
    {
      // locals
      SmBoolean     bOk;
      SmPoint3d     sStartPt;
      SmPoint3d     sEndPt;
      double        dGuess = sCIDomain.Evaluate( 0.1) ;
      double        dDist ;
      double        dDrop ;
      //SmScaledZero  sScaleZero = SmTol::GetScaledZero( sCIDomain) ;
      //SmZoneTol3d   sParamTol = sScaleZero.val;

      // EdgeDefinition->Curve->StartPt
      cpEDCurve->EvaluatePoint( sEDDomain.GetMin(), sStartPt) ;

      // Drop EdgeDefinition>XSectCurve MinPt to CurveClassification Curve
      SmStatus eStat = cpCICurve->DropPoint( sCIDomain,         // in : target curve allowed domain
                                             sStartPt,            // in : Point to drop to curve
                                             NULL,              // in : For Snapping, NULL=NoSnap, Vec pointing to CrvInside from desired EndPt for snapping.
                                                                //      when DropPt is within dDistTol in cpVecToCrvInside dir of EndPt, Snap to Ivl EndPt.
                                                                //      Vec handles ambiguities and makes sure that curves are not just touching at the ends.
                                             sZoneTol3d,        // in : Used for SnapDist, and SM_SO_INTERSECT sXSectTol3d distance.
                                                                //      sXSectTol3d = dDistTol ;      SM_SO_INTERSECT 
                                                                //      sXSectTol3d = SM_BIG_DOUBLE ; SM_SO_MINIMIZE, SM_SO_NORMALIZE
                                                                //      sSnapTol3d  = dDistTol ;      SM_SO_MINIMIZE, SM_SO_NORMALIZE, SM_SO_INTERSECT
                                             &dGuess,           // in : If given, call cheap LocalPointSolve() else call expensive GlobalPointSolve()
                                             bOk,               // out: TRUE = found a drop point
                                             dDrop,             // out: found drop curve param
                                             dDist,             // out: found drop distance
                                             SM_SO_INTERSECT) ; // in : SM_SO_MINIMIZE = allow nonNormal drops near endPoints
                                                                //      SM_SO_INTERSECT= like Minimize but point must be within dDistTol
                                                                //      SM_SO_NORMALIZE= exclude nonNormal drops near endPoints
                                                                //      default:[SM_SO_MINIMIZE] to preserve original behavior  
      // When StartPoint dropped within Tol to CC->Curve, check for overlap
      if(eStat == SM_SUCCESS && bOk )
        {
          // when EDCurve->StartPt is interior to CICurve - the CI and CD Curves overlap
          if(sm_IsInterior(cpCICurve,    // in : TgtCurve
                           sCIDomain,    // in : TgtCurve Ivl domain
                           dDrop,        // in : TgtParam to test
                           sZoneTol3d))  // in : Max dist between a test point and Tgt Ivl end considered in the interior
            { return TRUE; }
        }

      // EdgeDefinition->Curve->EndPt
      cpEDCurve->EvaluatePoint( sEDDomain.GetMax(), sEndPt) ;
      dGuess = sCIDomain.Evaluate( 0.9) ;

      // Drop EdgeDefinition>XSectCurve EndPt to CurveClassification Curve
      eStat = cpCICurve->DropPoint( sCIDomain,          // in : target curve allowed domain
                                    sEndPt,             // in : Point to drop to curve
                                    NULL,               // in : For Snapping, NULL=NoSnap, Vec pointing to CrvInside from desired EndPt for snapping.
                                                        //      when DropPt is within dDistTol in cpVecToCrvInside dir of EndPt, Snap to Ivl EndPt.
                                                        //      Vec handles ambiguities and makes sure that curves are not just touching at the ends.
                                    sZoneTol3d,         // in : Used for SnapDist, and SM_SO_INTERSECT sXSectTol3d distance.
                                                        //      sXSectTol3d = dDistTol ;      SM_SO_INTERSECT 
                                                        //      sXSectTol3d = SM_BIG_DOUBLE ; SM_SO_MINIMIZE, SM_SO_NORMALIZE
                                                        //      sSnapTol3d  = dDistTol ;      SM_SO_MINIMIZE, SM_SO_NORMALIZE, SM_SO_INTERSECT
                                    &dGuess,            // in : If given, call cheap LocalPointSolve() else call expensive GlobalPointSolve()
                                    bOk,                // out: TRUE = found a drop point
                                    dDrop,              // out: found drop curve param
                                    dDist,              // out: found drop distance
                                    SM_SO_INTERSECT) ;  // in : SM_SO_MINIMIZE = allow nonNormal drops near endPoints
                                                        //      SM_SO_INTERSECT= like Minimize but point must be within dDistTol
                                                        //      SM_SO_NORMALIZE= exclude nonNormal drops near endPoints
                                                        //      default:[SM_SO_MINIMIZE] to preserve original behavior
      // When EndPoint dropped within Tol to CC->Curve, check for overlap
      if(eStat == SM_SUCCESS && bOk )
        {
          // when EDCurve->EndPt is interior to CICurve - the CI and CD Curves overlap
          if(sm_IsInterior(cpCICurve,   // in : TgtCurve
                           sCIDomain,   // in : TgtCurve Ivl domain
                           dDrop,       // in : TgtParam to test
                           sZoneTol3d)) // in : Max dist between a test point and Tgt Ivl end considered in the interior
            { return TRUE; }
        }
    } // end scope dropping ED->Curve->EndPts to CI->Curve

  // arrive here when: EdgeDefininition Start and End Pt3ds do not drop to the interior of the CI XSectCurve
  // next: see if CI XSectCurve Start and End Pt3ds do drop to the interior of the ED curve

  // Drop CI->Curve->EndPts to ED->Curve
    {
      SmPoint3d     sStartPt;
      SmPoint3d     sEndPt;
      double        dGuess = sCIDomain.Evaluate( 0.1) ;
      double        dDist  = 0.0 ; 
      double        dDrop  = 0.0 ;
      //SmScaledZero  sScaleZero = SmTol::GetScaledZero( sCIDomain) ;
      //SmZoneTol3d   sParamTol = sScaleZero.val;
      SmBoolean     bOk;

      // CurveClassification Curve StartPt
      cpCICurve->EvaluatePoint( sCIDomain.GetMin(), sStartPt) ;

      // Drop CurveClassification Curve StartPt to EdgeDefinition>Curve
      SmStatus eStat = cpEDCurve->DropPoint(sEDDomain, 
                                            sStartPt, 
                                            NULL, 
                                            sZoneTol3d, 
                                            &dGuess,
                                            bOk, 
                                            dDrop, 
                                            dDist, 
                                            SM_SO_INTERSECT) ;

      // When StartPoint dropped within Tol to ED->Curve, check for overlap
      if(eStat == SM_SUCCESS && bOk )
        {
          // when CICurve->StartPt is interior to the EDCurve - the CI and CD Curves overlap
          if(sm_IsInterior( cpEDCurve, sEDDomain, dDrop, sZoneTol3d ) )
            { return TRUE; }
        }

      // CurveClassification Curve EndPt
      cpCICurve->EvaluatePoint( sCIDomain.GetMax(), sEndPt) ;
      dGuess = sEDDomain.Evaluate( 0.9) ;

      // Drop CurveClassification Curve EndPt to EdgeDefinition>Curve
      eStat = cpEDCurve->DropPoint(sEDDomain, 
                                   sEndPt, 
                                   NULL, 
                                   sZoneTol3d, 
                                   &dGuess,
                                   bOk, 
                                   dDrop, 
                                   dDist, 
                                   SM_SO_INTERSECT) ;

      // When EndPoint dropped within Tol to ED->Curve, check for overlap
      if(eStat == SM_SUCCESS && bOk )
        {
          // when CICurve->MaxPt is interior to the EDCurve - the CI and CD Curves overlap
          if(sm_IsInterior( cpEDCurve, sEDDomain, dDrop, sZoneTol3d ) )
            { return TRUE; }
        }
    } // end scope dropping CI to ED

  // arrive here when none of the CI or CD curve endpoints drop to the interior of the other curve - CI and CD curves do not overlap
  return FALSE;

} // end sm_AreOverlapping

// clang-format on
} // namespace sc_pt_p1

namespace
{
const int64_t ST_SKIP = -2;
const int64_t MAX_TBL = 256;

// ---- access to protected members (explicit instantiation) -------------------------------

template <typename Tag, typename Tag::type M>
struct Rob
{
    friend typename Tag::type get(Tag) { return M; }
};

struct EdFinal1
{
    typedef SmEdge* SmEdgeDefinition::*type;
    friend type get(EdFinal1);
};
template struct Rob<EdFinal1, &SmEdgeDefinition::m_pFinalEdge1>;

struct EdFinal2
{
    typedef SmEdge* SmEdgeDefinition::*type;
    friend type get(EdFinal2);
};
template struct Rob<EdFinal2, &SmEdgeDefinition::m_pFinalEdge2>;

struct AsmRemove
{
    typedef void (SmAssembly::*type)(SmAssemblyInstance*);
    friend type get(AsmRemove);
};
template struct Rob<AsmRemove, &SmAssembly::RemoveInstance>;

// The vertex list head upstream's own SmPolyBrep code posts new poly vertices on.
struct PolyVertexHead
{
    typedef SmOwningTopology* SmPolyBrep::*type;
    friend type get(PolyVertexHead);
};
template struct Rob<PolyVertexHead, &SmPolyBrep::m_pPolyVertexListHead>;

struct BrepModelSize { using type = double (SmBrep::*)(SmBoolean) const; friend type get(BrepModelSize); };
template struct Rob<BrepModelSize, &SmBrep::GetThisModelSizeEstimate>;

// ---- reading -------------------------------------------------------------------------------

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
SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    ULONG dim = (ULONG)rd.idx();
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

SmSurface* read_surface(Rd& rd, const SmContext& ctx, int64_t kind)
{
    if (kind == 1)
    {
        SmPoint3d org = rd.vec();
        SmVector3d nrm = rd.vec();
        return new (ctx) SmPlane(org, nrm, &ctx);
    }
    bool oob = rd.flag();
    SmBSplineSurface* s = new (ctx) SmBSplineSurface(read_nurb_surface(rd), FALSE, NULL);
    s->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
    return s;
}

// natural interval, then 5 evenly spaced points (status [P[3]])
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    for (int i = 0; i <= 4; i++)
    {
        SmPoint3d p;
        SmStatus s = c->EvaluatePoint(ivl.Evaluate(i / 4.0), p);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, p);
    }
}

// natural domain, then 3x3 evenly spaced points (status [P[3]])
void put_surface(std::vector<double>& o, const SmSurface* s)
{
    SmExtent2d d = s->GetNaturalUVDomain();
    o.push_back(d.GetUMin());
    o.push_back(d.GetUMax());
    o.push_back(d.GetVMin());
    o.push_back(d.GetVMax());
    for (int i = 0; i <= 2; i++)
        for (int j = 0; j <= 2; j++)
        {
            SmPoint3d p;
            SmPoint2d uv(d.GetUMin() + (d.GetUMax() - d.GetUMin()) * (i / 2.0),
                         d.GetVMin() + (d.GetVMax() - d.GetVMin()) * (j / 2.0));
            SmStatus st_ = s->EvaluatePoint(uv, p);
            o.push_back(st(st_));
            if (st_ == SM_SUCCESS)
                put_vec(o, p);
        }
}

// probe types for the type queries
const SM_TYPE PROBES[] = {SmObject_TYPE,          SmAObject_TYPE,          SmTopology_TYPE,
                          SmOwningTopology_TYPE,  SmSAGObject_TYPE,        SmAssembly_TYPE,
                          SmAssemblyInstance_TYPE, SmBrep_TYPE,            SmProtoTopologyManager_TYPE,
                          SmFace_TYPE,            0};

void put_string(std::vector<double>& o, const TCHAR* s)
{
    std::vector<double> chars;
    for (const TCHAR* c = s; c && *c; c++)
        chars.push_back((double)*c);
    o.push_back((double)chars.size());
    o.insert(o.end(), chars.begin(), chars.end());
}

void put_type(std::vector<double>& o, const SmObject* p)
{
    o.push_back((double)p->GetType());
    for (SM_TYPE t : PROBES)
        o.push_back(flg(p->IsKindOf(t)));
    put_string(o, p->GetTypeString());
    put_string(o, p->GetClassString());
}

// =====================================================================================
// LIST scripts: SmOwningTopology, SmAssembly, SmAssemblyInstance, SmCopyBrepMap
// =====================================================================================

struct ScOwning : public SmOwningTopology
{
    ScOwning() : SmOwningTopology() {}
    static void* operator new(size_t size, const SmContext& c) { return SmObject::operator new(size, c); }
    static void operator delete(void* p, const SmContext&) { smos_Free(p); }
    static void operator delete(void* p) { smos_Free(p); }
};

enum ListKind : int64_t
{
    LK_OWNING = 0,
    LK_ASSEMBLY = 1,
    LK_INSTANCE = 2,
};

enum ListOp : int64_t
{
    L_PRE_INSERT = 0,
    L_POST_INSERT = 1,
    L_INSERT_AFTER = 2,
    L_REMOVE = 3,
    L_IS_IN_LIST = 4,
    L_GET_ALL = 5,
    L_UPDATE_SIZE = 6,
    L_REVERSE = 7,
    L_SET_LIST = 8,
    L_DESTROY = 9,
    L_ASM_INSTANCES = 10,
    L_ASM_REMOVE_INSTANCE = 11,
    L_INST_NEW = 12,
    L_INST_PARENT = 13,
    L_MAP_SET = 14,
    L_MAP_GET = 15,
    L_MAP_BREPS = 16,
    L_ADD_ATTR = 17,
    L_TYPE = 18,
    L_GET_SIZE = 19,
};

// Exposes the protected links of a topology object.
struct Links : public SmOwningTopology
{
    static SmTopology* next(const SmTopology* t) { return ((const Links*)t)->m_pNext; }
    static SmTopology* last(const SmTopology* t) { return ((const Links*)t)->m_pLast; }
    static SmOwningTopology* owner(const SmTopology* t) { return ((const Links*)t)->m_pListOwner; }
};

struct ListRun
{
    std::vector<int64_t> kinds;
    std::vector<SmOwningTopology*> objs; // NULL: empty instance slot
    std::vector<bool> alive;
    SmCopyBrepMap map;
    std::map<const void*, const void*> map_entries; // what SetAt stored (GetAt's SM_ASSERTs dereference both)

    double id(const void* p) const
    {
        if (p == NULL)
            return -1.0;
        for (size_t i = 0; i < objs.size(); i++)
            if (objs[i] != NULL && (const void*)static_cast<SmTopology*>(objs[i]) == p)
                return alive[i] ? (double)i : -3.0;
        return -2.0;
    }
    bool live(int64_t i) const { return i >= 0 && i < (int64_t)objs.size() && objs[(size_t)i] != NULL && alive[(size_t)i]; }
    SmOwningTopology* get(int64_t i) const { return live(i) ? objs[(size_t)i] : NULL; }
    bool is_live_ptr(const void* p) const
    {
        double d = id(p);
        return d >= 0.0;
    }

    // every link of a live object is null or a live object, links are reciprocal,
    // next and last are null together
    bool invariant() const
    {
        for (size_t i = 0; i < objs.size(); i++)
        {
            if (!live((int64_t)i))
                continue;
            SmOwningTopology* o = objs[i];
            SmTopology* n = Links::next(o);
            SmTopology* l = Links::last(o);
            SmOwningTopology* w = Links::owner(o);
            if ((n == NULL) != (l == NULL))
                return false;
            if ((n && !is_live_ptr(n)) || (l && !is_live_ptr(l)) || (w && !is_live_ptr(w)))
                return false;
            if (n && Links::last(n) != o)
                return false;
            if (l && Links::next(l) != o)
                return false;
            if (o->GetList() && !is_live_ptr(o->GetList()))
                return false;
        }
        return true;
    }

    bool unlinked(const SmTopology* t) const
    {
        return Links::next(t) == NULL && Links::last(t) == NULL && Links::owner(t) == NULL;
    }

    // the walk from the owner's list head returns to the head (or ends at a null link)
    bool walk_terminates(const SmOwningTopology* o, bool null_ok) const
    {
        SmTopology* h = o->GetList();
        if (h == NULL)
            return true;
        SmTopology* p = h;
        for (size_t k = 0; k <= objs.size() + 1; k++)
        {
            p = Links::next(p);
            if (p == NULL)
                return null_ok;
            if (p == h)
                return true;
        }
        return false;
    }
};

void put_list_state(std::vector<double>& o, const ListRun& r)
{
    for (size_t i = 0; i < r.objs.size(); i++)
    {
        if (!r.live((int64_t)i))
        {
            o.push_back(0.0);
            continue;
        }
        const SmOwningTopology* t = r.objs[i];
        o.push_back(1.0);
        o.push_back(r.id(Links::owner(t)));
        o.push_back(r.id(Links::next(t)));
        o.push_back(r.id(Links::last(t)));
        o.push_back(r.id(t->GetList()));
        o.push_back((double)t->GetSize());
    }
    o.push_back(r.id(r.map.GetOrigBrep()));
    o.push_back(r.id(r.map.GetCopyBrep()));
}

int32_t run_list(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    ListRun r;
    int64_t n = rd.idx();
    if (n < 0 || n > 32)
        return SC_ERR_ARG;
    for (int64_t i = 0; i < n; i++)
    {
        int64_t k = rd.idx();
        r.kinds.push_back(k);
        r.alive.push_back(true);
        if (k == LK_OWNING)
            r.objs.push_back(new (ctx) ScOwning());
        else if (k == LK_ASSEMBLY)
            r.objs.push_back(new (ctx) SmAssembly(_T("assembly")));
        else if (k == LK_INSTANCE)
            r.objs.push_back(NULL);
        else
            return SC_ERR_ARG;
    }
    int64_t n_ops = rd.idx();
    if (rd.bad || n_ops < 0 || n_ops > 256)
        return SC_ERR_ARG;
    for (int64_t i = 0; i < n_ops; i++)
    {
        int64_t op = rd.idx();
        int64_t a = rd.idx();
        int64_t b = rd.idx();
        int64_t c = rd.idx();
        int64_t d = rd.idx();
        if (rd.bad)
            return SC_ERR_ARG;
        o.push_back((double)op);
        std::vector<double> out;
        double status = 0.0;
        bool skip = false;
        auto obj = [&](int64_t j) -> SmOwningTopology* { return j == -1 ? NULL : r.get(j); };
        auto bad = [&](int64_t j, bool null_ok) { return j == -1 ? !null_ok : !r.live(j); };
        switch (op)
        {
        case L_PRE_INSERT:
        case L_POST_INSERT:
        {
            if (bad(a, false) || bad(b, false))
            {
                skip = true;
                break;
            }
            SmOwningTopology* w = r.get(a);
            SmOwningTopology* x = r.get(b);
            if (!r.unlinked(x) || (w->GetList() && Links::last(w->GetList()) == NULL))
            {
                skip = true;
                break;
            }
            SmStatus s = op == L_PRE_INSERT ? w->PreInsert(x) : w->PostInsert(x);
            status = st(s);
            break;
        }
        case L_INSERT_AFTER:
        {
            if (bad(a, false) || bad(b, true) || bad(c, true))
            {
                skip = true;
                break;
            }
            SmOwningTopology* x = obj(b);
            if (x && !r.unlinked(x))
            {
                skip = true;
                break;
            }
            status = st(r.get(a)->InsertAfter(x, obj(c)));
            break;
        }
        case L_REMOVE:
        {
            if (bad(a, false) || bad(b, false))
            {
                skip = true;
                break;
            }
            status = st(r.get(a)->Remove(r.get(b)));
            break;
        }
        case L_IS_IN_LIST:
        {
            if (bad(a, false) || bad(b, true) || !r.walk_terminates(r.get(a), true))
            {
                skip = true;
                break;
            }
            ULONG idx = 0;
            SmBoolean f = r.get(a)->IsInList(obj(b), &idx);
            out.push_back(flg(f));
            out.push_back((double)idx);
            break;
        }
        case L_GET_ALL:
        {
            if (bad(a, false) || bad(c, true))
            {
                skip = true;
                break;
            }
            if ((b != -1 || c != -1) && !r.walk_terminates(r.get(a), true))
            {
                skip = true;
                break;
            }
            SmTArray<SmTopology*> arr;
            ULONG attr = (ULONG)b;
            status = st(r.get(a)->GetAll(arr, b == -1 ? NULL : &attr, obj(c)));
            out.push_back((double)arr.GetSize());
            for (ULONG k = 0; k < arr.GetSize(); k++)
                out.push_back(r.id(arr[k]));
            break;
        }
        case L_UPDATE_SIZE:
        {
            if (bad(a, false))
            {
                skip = true;
                break;
            }
            SmTopology* h = r.get(a)->GetList();
            // a null link past the head is dereferenced
            if (h && Links::next(h) && !r.walk_terminates(r.get(a), false))
            {
                skip = true;
                break;
            }
            status = st(r.get(a)->UpdateListSize());
            break;
        }
        case L_REVERSE:
        {
            if (bad(a, false) || r.get(a)->GetList() == NULL || !r.walk_terminates(r.get(a), false))
            {
                skip = true;
                break;
            }
            status = st(r.get(a)->ReverseList());
            break;
        }
        case L_SET_LIST:
        {
            if (bad(a, false) || bad(b, true))
            {
                skip = true;
                break;
            }
            r.get(a)->SetList(obj(b));
            break;
        }
        case L_DESTROY:
        {
            if (bad(a, false))
            {
                skip = true;
                break;
            }
            SmOwningTopology* x = r.get(a);
            int64_t k = r.kinds[(size_t)a];
            // what the destructor walks must terminate
            if (k == LK_ASSEMBLY && !r.walk_terminates(x, true))
            {
                skip = true;
                break;
            }
            // (members left pointing at the destroyed object fail the invariant)
            // run the destructors but keep the memory (no address reuse)
            if (k == LK_OWNING)
                ((ScOwning*)x)->~ScOwning();
            else if (k == LK_ASSEMBLY)
                ((SmAssembly*)x)->~SmAssembly();
            else
                ((SmAssemblyInstance*)x)->~SmAssemblyInstance();
            r.alive[(size_t)a] = false;
            break;
        }
        case L_ASM_INSTANCES:
        {
            if (bad(a, false) || r.kinds[(size_t)a] != LK_ASSEMBLY || !r.walk_terminates(r.get(a), true))
            {
                skip = true;
                break;
            }
            SmTArray<SmAssemblyInstance*> arr;
            ((SmAssembly*)r.get(a))->GetAssemblyInstances(arr);
            out.push_back((double)arr.GetSize());
            for (ULONG k = 0; k < arr.GetSize(); k++)
                out.push_back(r.id(arr[k]));
            break;
        }
        case L_ASM_REMOVE_INSTANCE:
        {
            if (bad(a, false) || bad(b, false) || r.kinds[(size_t)a] != LK_ASSEMBLY ||
                r.kinds[(size_t)b] != LK_INSTANCE)
            {
                skip = true;
                break;
            }
            // RemoveInstance is private (used by the instance destructor)
            (((SmAssembly*)r.get(a))->*get(AsmRemove()))((SmAssemblyInstance*)r.get(b));
            break;
        }
        case L_INST_NEW:
        {
            if (a < 0 || a >= (int64_t)r.objs.size() || r.kinds[(size_t)a] != LK_INSTANCE ||
                r.objs[(size_t)a] != NULL || bad(b, false) || r.kinds[(size_t)b] != LK_ASSEMBLY ||
                bad(c, true))
            {
                skip = true;
                break;
            }
            if (c != -1 && r.kinds[(size_t)c] == LK_OWNING)
            {
                skip = true;
                break;
            }
            SmOwningTopology* asmb = r.get(b);
            if (asmb->GetList() && Links::last(asmb->GetList()) == NULL)
            {
                skip = true;
                break;
            }
            SmSAGObject* comp = c == -1 ? NULL : (SmSAGObject*)r.get(c);
            SmAxis2Placement pl;
            r.objs[(size_t)a] = new (ctx) SmAssemblyInstance(_T("instance"), *(SmAssembly*)asmb, comp, pl,
                                                             SmVector3d(1.0, 1.0, 1.0), d != 0 ? TRUE : FALSE);
            out.push_back(r.id(((SmAssemblyInstance*)r.objs[(size_t)a])->GetComponent()));
            out.push_back(flg(((SmAssemblyInstance*)r.objs[(size_t)a])->GetMirrored()));
            break;
        }
        case L_INST_PARENT:
        {
            if (bad(a, false) || r.kinds[(size_t)a] != LK_INSTANCE)
            {
                skip = true;
                break;
            }
            out.push_back(r.id(((SmAssemblyInstance*)r.get(a))->GetParentAssembly()));
            break;
        }
        case L_MAP_SET:
        {
            // the release SM_ASSERTs still call GetType() on both objects
            if (bad(a, false) || bad(b, false))
            {
                skip = true;
                break;
            }
            r.map.SetAt(obj(a), obj(b));
            r.map_entries[static_cast<SmTopology*>(obj(a))] = static_cast<SmTopology*>(obj(b));
            break;
        }
        case L_MAP_GET:
        {
            if (bad(a, true))
            {
                skip = true;
                break;
            }
            if (a != -1)
            {
                // the release SM_ASSERTs call GetType() on the original found: it must
                // exist and be alive
                auto it = r.map_entries.find(static_cast<SmTopology*>(obj(a)));
                if (it == r.map_entries.end() || !r.is_live_ptr(it->second))
                {
                    skip = true;
                    break;
                }
            }
            SmTopology* g = r.map.GetAt(obj(a));
            out.push_back(r.id(g));
            break;
        }
        case L_MAP_BREPS:
        {
            if (bad(a, true) || bad(b, true))
            {
                skip = true;
                break;
            }
            r.map.SetBreps((SmBrep*)obj(a), (SmBrep*)obj(b));
            break;
        }
        case L_ADD_ATTR:
        {
            if (bad(a, false) || b < 0 || b > 1000)
            {
                skip = true;
                break;
            }
            r.get(a)->AddAttribute(new (ctx) SmAttribute((ULONG)b, SM_AB_STANDALONE_COPY));
            break;
        }
        case L_TYPE:
        {
            if (bad(a, false))
            {
                skip = true;
                break;
            }
            put_type(out, r.get(a));
            break;
        }
        case L_GET_SIZE:
        {
            if (bad(a, false))
            {
                skip = true;
                break;
            }
            out.push_back((double)r.get(a)->GetSize());
            out.push_back(r.id(r.get(a)->GetList()));
            break;
        }
        default: return SC_ERR_ARG;
        }
        if (!skip && !r.invariant())
            skip = true;
        if (skip)
        {
            o.push_back((double)ST_SKIP);
            return SC_OK;
        }
        o.push_back(status);
        o.push_back((double)out.size());
        o.insert(o.end(), out.begin(), out.end());
    }
    put_list_state(o, r);
    return SC_OK;
}

// =====================================================================================
// PROTO scripts
// =====================================================================================

enum PoolKind : int64_t
{
    K_CURVE = 0,
    K_PLANE = 1,
    K_BSURF = 2,
    K_VERTEX = 3,
    K_EDGE = 4,
    K_FACE = 5,
    K_REGION = 6,
    K_LOOP = 7,
    K_TRIMMED_FACE = 11, // input only: a real owned rectangular trimmed face
    K_VERTEX_AT = 10, // input only: a K_VERTEX with a point and a tolerance
};

// Pool object codes of the input that make a pool kind with extra data (unit
// topology.proto_topology_p2): an edge with its interval and zone tolerance, a vertex with its
// zone tolerance.
const int64_t OBJ_EDGE_IVL = 8;
const int64_t OBJ_VERTEX_TOL = 9;

struct ScVertex : public SmVertex
{
    bool pointed; // built with a point (a bare vertex has (0,0,0))
    ScVertex() : SmVertex(), pointed(false) {}
    explicit ScVertex(double tol) : SmVertex(), pointed(false) { m_sZoneTol3d = SmZoneTol3d(tol); }
    ScVertex(double x, double y, double z, double tol) : SmVertex(), pointed(true)
    {
        m_vPoint = SmPoint3d(x, y, z);
        m_sZoneTol3d = SmZoneTol3d(tol);
    }
};

struct ScEdge : public SmEdge
{
    explicit ScEdge(SmCurve* c) : SmEdge() { m_pCurve = c; }
    ScEdge(SmCurve* c, double a, double b, double tol) : SmEdge()
    {
        m_pCurve = c;
        m_vInterval = SmExtent1d(a, b);
        m_sZoneTol3d = SmZoneTol3d(tol);
    }
};

struct ScFace : public SmFace
{
    explicit ScFace(SmSurface* s) : SmFace() { m_pSurface = s; }
};

// A topology stand-in answering GetType and IsKindOf like `t` (a region or a loop).
class ScTopoStandIn : public SmTopology
{
public:
    explicit ScTopoStandIn(SM_TYPE t) : m_type(t) {}
    SM_TYPE GetType() const override { return m_type; }
    SmBoolean IsKindOf(SM_TYPE t) const override { return t == m_type || SmTopology::IsKindOf(t); }

private:
    SM_TYPE m_type;
};

struct Pool
{
    std::vector<int64_t> kinds;
    std::vector<SmObject*> ptrs;
    std::vector<bool> alive;
    std::map<const void*, int64_t> ids;
    SmBrep* operands[2] = {NULL, NULL};
    std::map<int64_t, SmBrep*> face_breps;
    std::map<int64_t, SmExtent2d> face_initial_domains;
    std::map<int64_t, SmPoint2d> face_trim_shifts;
    std::map<int64_t, std::pair<double, SmExtent2d>> face_domain_probes;
    std::set<int64_t> owned_surfaces;

    double id(const void* p) const
    {
        if (p == NULL)
            return -1.0;
        auto it = ids.find(p);
        if (it == ids.end())
            return -2.0;
        return alive[(size_t)it->second] ? (double)it->second : -3.0;
    }
    bool valid(int64_t i) const { return i >= 0 && i < (int64_t)ptrs.size() && alive[(size_t)i]; }
    bool is(int64_t i, int64_t k) const { return valid(i) && kinds[(size_t)i] == k; }
    bool is_curve(int64_t i) const { return is(i, K_CURVE); }
    bool is_topo(int64_t i) const
    {
        return valid(i) && (kinds[(size_t)i] == K_VERTEX || kinds[(size_t)i] == K_EDGE ||
                            kinds[(size_t)i] == K_FACE || kinds[(size_t)i] == K_REGION ||
                            kinds[(size_t)i] == K_LOOP);
    }
    int64_t kind_of(const void* p) const
    {
        auto it = ids.find(p);
        return it == ids.end() ? -1 : kinds[(size_t)it->second];
    }
    bool live_ptr(const void* p) const { return id(p) >= 0.0; }
    void kill(const void* p)
    {
        auto it = ids.find(p);
        if (it != ids.end())
        {
            alive[(size_t)it->second] = false;
            ids.erase(it);
        }
    }
};

// A creation-ordered table of proto objects.
struct Tbl
{
    std::vector<void*> p;
    std::vector<bool> alive;
    std::map<const void*, int64_t> ids;

    int64_t add(void* x)
    {
        ids[x] = (int64_t)p.size();
        p.push_back(x);
        alive.push_back(true);
        return (int64_t)p.size() - 1;
    }
    double id(const void* x) const
    {
        if (x == NULL)
            return -1.0;
        auto it = ids.find(x);
        if (it == ids.end())
            return -2.0;
        return alive[(size_t)it->second] ? (double)it->second : -3.0;
    }
    bool live(int64_t i) const { return i >= 0 && i < (int64_t)p.size() && alive[(size_t)i]; }
    bool live_ptr(const void* x) const { return id(x) >= 0.0; }
    void kill(const void* x)
    {
        auto it = ids.find(x);
        if (it != ids.end())
        {
            alive[(size_t)it->second] = false;
            ids.erase(it);
        }
    }
};

struct ProtoRun
{
    Pool pool;
    Tbl vds, eds, pvs, pes, pfs;
    SmProtoTopologyManager* mgr = NULL;
    unsigned first_ed_final_initialized = 0;

    SmVertexDefinition* vd(int64_t i) const { return vds.live(i) ? (SmVertexDefinition*)vds.p[(size_t)i] : NULL; }
    SmEdgeDefinition* ed(int64_t i) const { return eds.live(i) ? (SmEdgeDefinition*)eds.p[(size_t)i] : NULL; }
    SmProtoVertex* pv(int64_t i) const { return pvs.live(i) ? (SmProtoVertex*)pvs.p[(size_t)i] : NULL; }
    SmProtoEdge* pe(int64_t i) const { return pes.live(i) ? (SmProtoEdge*)pes.p[(size_t)i] : NULL; }
    SmProtoFace* pf(int64_t i) const { return pfs.live(i) ? (SmProtoFace*)pfs.p[(size_t)i] : NULL; }

    bool curve_ok(const SmCurve* c) const { return c == NULL || pool.live_ptr(c); }

    // no live object or list refers to a deleted object
    bool invariant() const
    {
        if (mgr)
        {
            for (ULONG i = 0; i < mgr->GetProtoVertices().GetSize(); i++)
                if (!pvs.live_ptr(mgr->GetProtoVertices()[i]))
                    return false;
            for (ULONG i = 0; i < mgr->GetProtoEdges().GetSize(); i++)
                if (!pes.live_ptr(mgr->GetProtoEdges()[i]))
                    return false;
            for (ULONG i = 0; i < mgr->GetProtoFaces().GetSize(); i++)
                if (!pfs.live_ptr(mgr->GetProtoFaces()[i]))
                    return false;
        }
        for (size_t i = 0; i < vds.p.size(); i++)
        {
            SmVertexDefinition* v = vd((int64_t)i);
            if (!v)
                continue;
            if (v->GetProtoVertex() && !pvs.live_ptr(v->GetProtoVertex()))
                return false;
            for (int w = 1; w <= 2; w++)
            {
                const SmObject* ob = v->GetBrepPointClassif(w)->GetObject();
                if (ob && !pool.live_ptr(ob))
                    return false;
            }
        }
        for (size_t i = 0; i < eds.p.size(); i++)
        {
            SmEdgeDefinition* e = ed((int64_t)i);
            if (!e)
                continue;
            if (e->GetProtoEdge() && !pes.live_ptr(e->GetProtoEdge()))
                return false;
            if (!curve_ok(e->GetXSectCurve3d()) || !curve_ok(e->GetXSectUVCurve(1)) ||
                !curve_ok(e->GetXSectUVCurve(2)))
                return false;
        }
        for (size_t i = 0; i < pvs.p.size(); i++)
        {
            SmProtoVertex* v = pv((int64_t)i);
            if (!v)
                continue;
            for (ULONG k = 0; k < v->GetVertexDefinitions().GetSize(); k++)
                if (!vds.live_ptr(v->GetVertexDefinitions()[k]))
                    return false;
            for (ULONG k = 0; k < v->GetProtoEdges().GetSize(); k++)
                if (!pes.live_ptr(v->GetProtoEdges()[k]))
                    return false;
        }
        for (size_t i = 0; i < pes.p.size(); i++)
        {
            SmProtoEdge* e = pe((int64_t)i);
            if (!e)
                continue;
            if ((e->GetStartProtoVertex() && !pvs.live_ptr(e->GetStartProtoVertex())) ||
                (e->GetEndProtoVertex() && !pvs.live_ptr(e->GetEndProtoVertex())))
                return false;
            for (ULONG k = 0; k < e->GetEdgeDefinitions().GetSize(); k++)
                if (!eds.live_ptr(e->GetEdgeDefinitions()[k]))
                    return false;
            for (ULONG k = 0; k < e->GetOverlappingPEs().GetSize(); k++)
                if (!pes.live_ptr(e->GetOverlappingPEs()[k]))
                    return false;
            for (ULONG k = 0; k < e->GetCoincProtoFaces()->GetSize(); k++)
                if (!pfs.live_ptr((*e->GetCoincProtoFaces())[k]))
                    return false;
        }
        for (size_t i = 0; i < pfs.p.size(); i++)
        {
            SmProtoFace* f = pf((int64_t)i);
            if (!f)
                continue;
            for (ULONG k = 0; k < f->GetCoincProtoEdges()->GetSize(); k++)
                if (!pes.live_ptr((*f->GetCoincProtoEdges())[k]))
                    return false;
        }
        return true;
    }
};

void put_pc(std::vector<double>& o, const SmPointClassification& pc, const Pool& pool)
{
    o.push_back((double)pc.GetPointClass());
    o.push_back(pool.id(pc.GetObject()));
    o.push_back(pc.GetTParam());
    SmPoint2d uv = pc.GetUVParam();
    o.push_back(uv.x);
    o.push_back(uv.y);
    o.push_back(flg(pc.AreParametersSet()));
    o.push_back(pc.GetDeviation());
    o.push_back(SmTol::GetSrcZoneTol3d(&pc));
}

// `class obj tol flags t u v gap` (flags: 1 SetTParam, 2 SetUVParam, 4 SetGap3d); the
// object must be of the class's kind (no object for UNKNOWN / NOTHING).
bool read_pc(Rd& rd, const Pool& pool, SmPointClassification& pc)
{
    int64_t cls = rd.idx();
    int64_t ob = rd.idx();
    double tol = rd.num();
    int64_t flags = rd.idx();
    double t = rd.num();
    double u = rd.num();
    double v = rd.num();
    double g = rd.num();
    bool ok;
    switch (cls)
    {
    case SM_PC_UNKNOWN:
    case SM_PC_NOTHING: ok = ob == -1; break;
    case SM_PC_REGION: ok = pool.is(ob, K_REGION); break;
    case SM_PC_FACE: ok = pool.is(ob, K_FACE); break;
    case SM_PC_EDGE: ok = pool.is(ob, K_EDGE); break;
    case SM_PC_VERTEX: ok = pool.is(ob, K_VERTEX); break;
    case SM_PC_CURVE: ok = pool.is(ob, K_CURVE); break;
    case SM_PC_SURFACE: ok = pool.is(ob, K_PLANE) || pool.is(ob, K_BSURF); break;
    default: ok = false;
    }
    if (!ok || rd.bad)
        return false;
    pc = SmPointClassification(SmZoneTol3d(tol), &the_context());
    pc.SetClassObject((SmPointClassificationType)cls, ob == -1 ? NULL : pool.ptrs[(size_t)ob]);
    if (flags & 1)
        pc.SetTParam(t);
    if (flags & 2)
        pc.SetUVParam(SmPoint2d(u, v));
    if (flags & 4)
        pc.SetGap3d(g);
    return true;
}

enum ProtoOp : int64_t
{
    P_PV_NEW = 0,
    P_PV_NEW_PCS = 1,
    P_VD_NEW = 2,
    P_PE_NEW = 3,
    P_PE_COPY = 4,
    P_ED_NEW = 5,
    P_ED_COPY = 6,
    P_PF_NEW = 7,
    P_PF_CREATE = 8,
    P_MGR_ADD_PV = 9,
    P_MGR_ADD_PE = 10,
    P_MGR_ADD_PF = 11,
    P_MGR_RESET = 12,
    P_PV_ADD_PE = 13,
    P_PV_ADD_VD = 14,
    P_PE_ADD_ED = 15,
    P_PE_ADD_OVERLAP = 16,
    P_PE_APPEND_OVERLAPS = 17,
    P_PE_CLEAR_EDS = 18,
    P_PV_CLEAR_VDS = 19,
    P_PE_SET_START = 20,
    P_PE_SET_END = 21,
    P_PE_SET_IVL = 22,
    P_PE_REPLACE_PV = 23,
    P_PE_REMOVE_ED = 24,
    P_PE_REMOVE_PV = 25,
    P_PV_REMOVE_VD = 26,
    P_PV_REMOVE_PE = 27,
    P_PE_RELATE_PF = 28,
    P_PF_SET_SRC = 29,
    P_VD_SET_SRC = 30,
    P_VD_SET_FINAL = 31,
    P_VD_SET_POS = 32,
    P_VD_SET_PV = 33,
    P_ED_SET_C3 = 34,
    P_ED_SET_UV = 35,
    P_ED_SET_PE = 36,
    P_ED_SET_IVL = 37,
    P_ED_SET_SRC = 38,
    P_ED_SET_FINAL = 39,
    P_MGR_REMOVE_VD = 40,
    P_MGR_REMOVE_PV = 41,
    P_MGR_REMOVE_PE = 42,
    P_MGR_SET_TOL = 43,
    P_MGR_SET_CAPTURE = 44,
    P_PV_COMBINE = 45,
    P_ED_DESTRUCT = 46,
    P_DELETE_PE = 47,
    P_DELETE_PV = 48,
    P_DELETE_VD = 49,
    P_DELETE_ED = 50,
    P_MGR_DESTROY = 51,
    P_VD_RESOLVE = 52,
    P_VD_SET_POINT_PARAMS = 53,
    P_VD_NUM_PES = 54,
    P_ED_EVAL = 55,
    P_ED_EVAL_MID = 56,
    P_ED_END_POS = 57,
    P_ED_END_POS_PV = 58,
    P_ED_REVERSE = 59,
    P_ED_REPARAM = 60,
    P_ED_SET_IVL_MINMAX = 61,
    P_ED_IS_SAME = 62,
    P_ED_IS_INTERIOR = 63,
    P_ED_START_END_PV = 64,
    P_PE_END_POS = 65,
    P_PE_END_POS_PV = 66,
    P_PE_EVAL = 67,
    P_PE_EVAL_MID = 68,
    P_PE_IS_SAME = 69,
    P_PE_GROUP = 70,
    P_PV_FIND_MATCH = 71,
    P_PV_CLOSEST = 72,
    P_PV_FINAL_VTX = 73,
    P_MGR_FIND_PV = 74,
    P_MGR_FIND_ED_FROM_EDGE = 75,
    P_MGR_ALL_VDS = 76,
    P_MGR_ALL_EDS = 77,
    P_MGR_CHECK_OVERLAP = 78,
    P_MGR_CAN_COMBINE = 79,
    P_MGR_REMOVE_NOEDGE_PV = 80,
    P_WHICH_PV = 81,
    P_FIND_POS = 82,
    P_SAME_CURVE = 83,
    P_IS_INTERIOR = 84,
    P_ARE_CONNECTED = 85,
    P_TOPO_DIM = 86,
    P_CAN_IMPRINT = 87,
    P_ARE_OVERLAPPING = 88,
    P_MGR_TYPE = 89,
    P_MGR_GET = 90,
    // topology.proto_topology_p3
    P_FOC_PV = 120,
    P_FOC_PE = 121,
    P_PE_SPLIT = 122,
    P_ED_SPLIT = 123,
    P_SPLIT_INT = 124,
    P_GLUE = 125,
    P_RESOLVE = 126,
};

// sm_AreConnected walks the bare stand-ins safely except an edge's start vertex (no
// edgeuses: a NULL dereference).
bool connect_crashes(const Pool& pool, const void* a, const void* b)
{
    if (a == b)
        return false;
    int64_t ka = pool.kind_of(a), kb = pool.kind_of(b);
    return (ka == K_VERTEX && kb == K_EDGE) || (ka == K_EDGE && kb == K_VERTEX);
}

void put_ids(std::vector<double>& o, const Tbl& t, const std::vector<const void*>& v)
{
    o.push_back((double)v.size());
    for (const void* x : v)
        o.push_back(t.id(x));
}

template <typename T>
std::vector<const void*> as_vec(const SmTArray<T>& a)
{
    std::vector<const void*> v;
    for (ULONG i = 0; i < a.GetSize(); i++)
        v.push_back(a[i]);
    return v;
}

// The pointers an edge definition's destructor deletes.
void ed_deletes(SmEdgeDefinition* e, std::vector<const void*>& curves)
{
    if (e->GetSrcTopo_TYPE(1) == SM_PC_EDGE)
    {
        if (e->GetXSectCurve3d())
            curves.push_back(e->GetXSectCurve3d());
        if (e->GetXSectUVCurve(1))
            curves.push_back(e->GetXSectUVCurve(1));
    }
    if (e->GetSrcTopo_TYPE(2) == SM_PC_EDGE && e->GetXSectUVCurve(2))
        curves.push_back(e->GetXSectUVCurve(2));
}

bool unique_live(const std::vector<const void*>& v, const Tbl* t, const Pool* pool)
{
    for (size_t i = 0; i < v.size(); i++)
    {
        if (t && !t->live_ptr(v[i]))
            return false;
        if (pool && !pool->live_ptr(v[i]))
            return false;
        for (size_t j = 0; j < i; j++)
            if (v[i] == v[j])
                return false;
    }
    return true;
}

void put_deleted_curves(std::vector<double>& out, const Pool& pool, const std::vector<const void*>& curves)
{
    out.push_back((double)curves.size());
    for (const void* c : curves)
        out.push_back(pool.id(c));
}

void put_eval(std::vector<double>& out, SmStatus s, int64_t mode, const SmPoint3d& p, const SmVector3d& t)
{
    out.push_back(st(s));
    if (s != SM_SUCCESS)
        return;
    if (mode & 1)
        put_vec(out, p);
    if (mode & 2)
        put_vec(out, t);
}

void put_proto_state(std::vector<double>& o, ProtoRun& r)
{
    o.push_back(r.mgr ? 1.0 : 0.0);
    if (r.mgr)
    {
        o.push_back((double)r.mgr->GetTolerance());
        o.push_back(r.mgr->GetCaptureDistance());
        put_ids(o, r.pvs, as_vec(r.mgr->GetProtoVertices()));
        put_ids(o, r.pes, as_vec(r.mgr->GetProtoEdges()));
        put_ids(o, r.pfs, as_vec(r.mgr->GetProtoFaces()));
    }
    for (size_t i = 0; i < r.vds.p.size(); i++)
    {
        SmVertexDefinition* v = r.vd((int64_t)i);
        o.push_back(v ? 1.0 : 0.0);
        if (!v)
            continue;
        put_pc(o, *v->GetBrepPointClassif(1), r.pool);
        put_pc(o, *v->GetBrepPointClassif(2), r.pool);
        put_vec(o, v->GetPosition());
        o.push_back((double)v->GetTolerance());
        o.push_back(v->GetGap());
        o.push_back(r.pvs.id(v->GetProtoVertex()));
        o.push_back(r.pool.id(v->GetFinalVertex(1)));
        o.push_back(r.pool.id(v->GetFinalVertex(2)));
        o.push_back(r.pool.id(v->GetSrcTopo(1)));
        o.push_back(r.pool.id(v->GetSrcTopo(2)));
        o.push_back((double)v->GetSrcTopo_TYPE(1));
        o.push_back((double)v->GetSrcTopo_TYPE(2));
    }
    for (size_t i = 0; i < r.eds.p.size(); i++)
    {
        SmEdgeDefinition* e = r.ed((int64_t)i);
        o.push_back(e ? 1.0 : 0.0);
        if (!e)
            continue;
        o.push_back(r.pool.id(e->GetXSectCurve3d()));
        o.push_back(r.pool.id(e->GetXSectUVCurve(1)));
        o.push_back(r.pool.id(e->GetXSectUVCurve(2)));
        o.push_back(e->GetXSectInterval().GetMin());
        o.push_back(e->GetXSectInterval().GetMax());
        o.push_back(r.pool.id(e->GetSrcTopo(1)));
        o.push_back(r.pool.id(e->GetSrcTopo(2)));
        o.push_back((double)e->GetSrcTopo_TYPE(1));
        o.push_back((double)e->GetSrcTopo_TYPE(2));
        o.push_back(r.pes.id(e->GetProtoEdge()));
        o.push_back((double)e->GetTolerance());
        o.push_back(r.pool.id(e->GetFinalEdge(1)));
        o.push_back(r.pool.id(e->GetFinalEdge(2)));
    }
    for (size_t i = 0; i < r.pvs.p.size(); i++)
    {
        SmProtoVertex* v = r.pv((int64_t)i);
        o.push_back(v ? 1.0 : 0.0);
        if (!v)
            continue;
        put_vec(o, v->GetPosition());
        o.push_back((double)v->GetTolerance());
        put_ids(o, r.vds, as_vec(v->GetVertexDefinitions()));
        put_ids(o, r.pes, as_vec(v->GetProtoEdges()));
    }
    for (size_t i = 0; i < r.pes.p.size(); i++)
    {
        SmProtoEdge* e = r.pe((int64_t)i);
        o.push_back(e ? 1.0 : 0.0);
        if (!e)
            continue;
        o.push_back(e->GetXSectInterval().GetMin());
        o.push_back(e->GetXSectInterval().GetMax());
        o.push_back(r.pvs.id(e->GetStartProtoVertex()));
        o.push_back(r.pvs.id(e->GetEndProtoVertex()));
        o.push_back((double)e->GetTolerance());
        put_ids(o, r.eds, as_vec(e->GetEdgeDefinitions()));
        put_ids(o, r.pes, as_vec(e->GetOverlappingPEs()));
        put_ids(o, r.pfs, as_vec(*e->GetCoincProtoFaces()));
    }
    for (size_t i = 0; i < r.pfs.p.size(); i++)
    {
        SmProtoFace* f = r.pf((int64_t)i);
        o.push_back(f ? 1.0 : 0.0);
        if (!f)
            continue;
        o.push_back(r.pool.id(f->GetSrcTopo(1)));
        o.push_back(r.pool.id(f->GetSrcTopo(2)));
        o.push_back((double)f->GetSrcTopo_TYPE(1));
        put_ids(o, r.pes, as_vec(*f->GetCoincProtoEdges()));
    }
    for (size_t i = 0; i < r.pool.ptrs.size(); i++)
    {
        if (r.pool.kinds[i] != K_CURVE)
            continue;
        o.push_back(r.pool.alive[i] ? 1.0 : 0.0);
        if (r.pool.alive[i])
            put_curve(o, (SmCurve*)r.pool.ptrs[i]);
    }
}

// what a proto edge's destructor needs, and the curves it deletes; false when it is
// undefined (a deleted or repeated definition or curve)
bool pe_delete_check(ProtoRun& r, SmProtoEdge* e, std::vector<const void*>& eds_out,
                     std::vector<const void*>& curves_out)
{
    for (ULONG k = 0; k < e->GetEdgeDefinitions().GetSize(); k++)
    {
        SmEdgeDefinition* d = e->GetEdgeDefinitions()[k];
        eds_out.push_back(d);
        if (r.eds.live_ptr(d))
            ed_deletes(d, curves_out);
    }
    return true;
}


// ---- topology.proto_topology_p3 helpers ---------------------------------------------------

// The objects reachable from the manager's lists, and the curves each edge definition owns.
struct Reach
{
    std::set<const void*> vds, eds, pvs, pes, pfs;
    std::map<const void*, std::vector<const void*>> ed_all, ed_dtor;
};

Reach reach(ProtoRun& r)
{
    Reach s;
    if (!r.mgr)
        return s;
    for (ULONG i = 0; i < r.mgr->GetProtoVertices().GetSize(); i++)
    {
        SmProtoVertex* v = r.mgr->GetProtoVertices()[i];
        s.pvs.insert(v);
        for (ULONG k = 0; k < v->GetVertexDefinitions().GetSize(); k++)
            s.vds.insert(v->GetVertexDefinitions()[k]);
    }
    for (ULONG i = 0; i < r.mgr->GetProtoEdges().GetSize(); i++)
    {
        SmProtoEdge* e = r.mgr->GetProtoEdges()[i];
        s.pes.insert(e);
        for (ULONG k = 0; k < e->GetEdgeDefinitions().GetSize(); k++)
        {
            SmEdgeDefinition* d = e->GetEdgeDefinitions()[k];
            s.eds.insert(d);
            std::vector<const void*>& all = s.ed_all[d];
            if (d->GetXSectCurve3d())
                all.push_back(d->GetXSectCurve3d());
            if (d->GetXSectUVCurve(1))
                all.push_back(d->GetXSectUVCurve(1));
            if (d->GetXSectUVCurve(2))
                all.push_back(d->GetXSectUVCurve(2));
            ed_deletes(d, s.ed_dtor[d]);
        }
    }
    for (ULONG i = 0; i < r.mgr->GetProtoFaces().GetSize(); i++)
        s.pfs.insert(r.mgr->GetProtoFaces()[i]);
    return s;
}

// What an op deleted (reachable before, not after) is dead: the tables and the pool say so.
// Returns the sorted ids of the curves deleted.
std::vector<double> reconcile(ProtoRun& r, const Reach& before, bool destruct_all)
{
    Reach after = reach(r);
    std::vector<double> curves;
    for (const void* x : before.pvs)
        if (!after.pvs.count(x))
            r.pvs.kill(x);
    for (const void* x : before.vds)
        if (!after.vds.count(x))
            r.vds.kill(x);
    for (const void* x : before.pes)
        if (!after.pes.count(x))
            r.pes.kill(x);
    for (const void* x : before.pfs)
        if (!after.pfs.count(x))
            r.pfs.kill(x);
    for (const void* x : before.eds)
        if (!after.eds.count(x))
        {
            const std::vector<const void*>& cs = destruct_all ? before.ed_all.at(x) : before.ed_dtor.at(x);
            for (const void* c : cs)
            {
                if (r.pool.live_ptr(c))
                    curves.push_back(r.pool.id(c));
                r.pool.kill(c);
            }
            r.eds.kill(x);
        }
    std::sort(curves.begin(), curves.end());
    return curves;
}

bool pe_listed(ProtoRun& r, const SmProtoEdge* e)
{
    for (ULONG i = 0; i < r.mgr->GetProtoEdges().GetSize(); i++)
        if (r.mgr->GetProtoEdges()[i] == e)
            return true;
    return false;
}

bool pv_listed(ProtoRun& r, const SmProtoVertex* v)
{
    for (ULONG i = 0; i < r.mgr->GetProtoVertices().GetSize(); i++)
        if (r.mgr->GetProtoVertices()[i] == v)
            return true;
    return false;
}

// the proto edges and both end vertices are listed in the manager (what an op deletes is then
// found again through the lists)
bool pe_listed_with_ends(ProtoRun& r, SmProtoEdge* e)
{
    return pe_listed(r, e) && pv_listed(r, e->GetStartProtoVertex()) && pv_listed(r, e->GetEndProtoVertex());
}

// every curve of every live edge definition is live and held by one slot only
bool ed_curves_unique(ProtoRun& r)
{
    std::set<const void*> seen;
    for (size_t i = 0; i < r.eds.p.size(); i++)
    {
        SmEdgeDefinition* d = r.ed((int64_t)i);
        if (!d)
            continue;
        const void* cs[3] = {d->GetXSectCurve3d(), d->GetXSectUVCurve(1), d->GetXSectUVCurve(2)};
        for (const void* c : cs)
        {
            if (!c)
                continue;
            if (!r.pool.live_ptr(c) || !seen.insert(c).second)
                return false;
        }
    }
    return true;
}

// what SmProtoEdge::Split needs to run on stand-ins: definitions with curves and a proto edge,
// and both end proto vertices
bool pe_split_ok(ProtoRun& r, SmProtoEdge* e)
{
    if (!e->GetStartProtoVertex() || !e->GetEndProtoVertex())
        return false;
    for (ULONG k = 0; k < e->GetEdgeDefinitions().GetSize(); k++)
    {
        SmEdgeDefinition* d = e->GetEdgeDefinitions()[k];
        if (!r.eds.live_ptr(d) || !d->GetXSectCurve3d() || !d->GetProtoEdge())
            return false;
    }
    return true;
}

// =====================================================================================
// Unit topology.proto_topology_p2: the second part of SmProtoTopology.cpp (imprint, resolve,
// create / find / combine / update). Its ops (P2_*) run inside proto scripts; the runner of
// that unit generates them. Objects the calls make are registered after the op
// (register_new), in the order the port makes them. Refusals (op -2) are exactly the
// dereferences of null or deleted objects, double deletes and reads of uninitialised values
// the calls would make, worked out before the call by replaying the call's control flow with
// const queries; plus two conservative guards the runner mirrors: a duplicated entry in the
// list a resolve deletes from, and zero-length intervals where a reparameterisation can
// follow (plan/upstream-issues.md HANG-12).
// =====================================================================================

enum ProtoOpP2 : int64_t
{
    P2_VD_IMPRINT = 91,
    P2_ED_IMPRINT = 92,
    P2_PV_RESOLVE = 93,
    P2_PV_ADD_VD_PCS = 94,
    P2_PE_FIND_MATCH = 95,
    P2_PE_ADD_ED = 96,
    P2_PE_ADD_ED_CIS = 97,
    P2_PE_COMBINE = 98,
    P2_PE_RESOLVE = 99,
    P2_MGR_CREATE_VD = 100,
    P2_MGR_CREATE_PV = 101,
    P2_MGR_FIND_OR_CREATE_PV = 102,
    P2_MGR_FIND_PE = 103,
    P2_MGR_CREATE_PE = 104,
    P2_MGR_REMOVE_ED = 105,
    P2_MGR_UPDATE_EDGES = 106,
    P2_MGR_UPDATE_FACES = 107,
    P2_MGR_CAN_COMBINE_VDS = 108,
    P2_FIRST = 91,
    P2_LAST = 108,
};

// A curve interval `c uv tol a b pc`: an SmCurveClassification of pool curve `c` (or none)
// with first UV curve `uv` and source zone tolerance `tol`, and its interval [a, b] whose
// midpoint classification is `pc`.
struct CiIn
{
    SmCurveClassification cc;
    SmCurveInterval* ci = NULL;
    ~CiIn() { delete ci; }
};

bool read_ci(Rd& rd, const Pool& pool, CiIn& x)
{
    int64_t c = rd.idx();
    int64_t uv = rd.idx();
    double tol = rd.num();
    double a = rd.num();
    double b = rd.num();
    SmPointClassification mid;
    bool ok = read_pc(rd, pool, mid);
    if ((c != -1 && !pool.is_curve(c)) || (uv != -1 && !pool.is_curve(uv)))
        ok = false;
    if (!ok || rd.bad)
        return false;
    x.cc.SetCurve(c == -1 ? NULL : (SmCurve*)pool.ptrs[(size_t)c]);
    x.cc.SetUVCurve1(uv == -1 ? NULL : (SmCurve*)pool.ptrs[(size_t)uv]);
    SmTol::SetZoneTol3d(&x.cc, SmZoneTol3d(tol));
    x.ci = new SmCurveInterval(SmExtent1d(a, b), &the_context(), &x.cc);
    x.ci->m_vMid = mid;
    return true;
}

bool zero_len(const SmExtent1d& e) { return e.GetMin() == e.GetMax(); }

bool has_dup(const std::vector<const void*>& v)
{
    for (size_t i = 0; i < v.size(); i++)
        for (size_t j = 0; j < i; j++)
            if (v[i] == v[j])
                return true;
    return false;
}

// What IsSameEdgeDefinition(v1, v2, .., c, ..) dereferences: the definition's proto edge, and
// both curves when the edge runs between v1 and v2 and the curves differ.
bool same_ed_ok(SmEdgeDefinition* d, const SmProtoVertex* v1, const SmProtoVertex* v2, const SmCurve* c)
{
    SmProtoEdge* e = d->GetProtoEdge();
    if (e == NULL)
        return false;
    bool between = (v1 == e->GetStartProtoVertex() && v2 == e->GetEndProtoVertex()) ||
                   (v1 == e->GetEndProtoVertex() && v2 == e->GetStartProtoVertex());
    return !(between && d->GetXSectCurve3d() != c && (d->GetXSectCurve3d() == NULL || c == NULL));
}

// FindMatchingEdgeDefinition replayed: false when it would dereference a bad pointer.
bool find_match_ok(SmProtoEdge* e, const CiIn& c1, const CiIn& c2)
{
    SmBoolean ident = FALSE;
    for (ULONG k = 0; k < e->GetNumEdgeDefinitions(); k++)
    {
        SmEdgeDefinition* d = e->GetEdgeDefinitions()[k];
        if (!same_ed_ok(d, e->GetStartProtoVertex(), e->GetEndProtoVertex(), c1.cc.GetCurve()))
            return false;
        if (d->IsSameEdgeDefinition(e->GetStartProtoVertex(), e->GetEndProtoVertex(), c1.ci->m_vMid, c2.ci->m_vMid,
                                    c1.cc.GetCurve(), c1.ci->GetInterval(), ident) &&
            ident)
            return true;
    }
    return true;
}

// IsSameProtoEdge(v1, v2, .., c, ..) replayed: false when it would dereference a bad pointer.
bool same_pe_ok(SmProtoEdge* e, const SmProtoVertex* v1, const SmProtoVertex* v2, const SmCurve* c)
{
    bool between = (v1 == e->GetStartProtoVertex() && v2 == e->GetEndProtoVertex()) ||
                   (v1 == e->GetEndProtoVertex() && v2 == e->GetStartProtoVertex());
    if (!between)
        return true;
    for (ULONG k = 0; k < e->GetNumEdgeDefinitions(); k++)
        if (!same_ed_ok(e->GetEdgeDefinitions()[k], v1, v2, c))
            return false;
    return true;
}

// sm_FindPositionFromPCs on copies: whether CreateVertexDefinition / CreateProtoVertex read
// an initialised position and deviation.
bool position_ok(const SmPointClassification& pc1, const SmPointClassification& pc2)
{
    SmPointClassification a = pc1, b = pc2;
    SmPoint3d p;
    double dev;
    return sc_pt_p1::sm_FindPositionFromPCs(a, b, p, dev) == SM_SUCCESS;
}

// refused when two classifications that can be imprinted have no position (CreateProtoVertex
// would read an uninitialised one)
bool pair_position_ok(const SmPointClassification& pc1, const SmPointClassification& pc2)
{
    return !(sc_pt_p1::sm_CanImprint(pc1.GetPointClass()) && sc_pt_p1::sm_CanImprint(pc2.GetPointClass())) ||
           position_ok(pc1, pc2);
}

// The tangent EvaluateMid of a proto edge / definition writes (all curves present, success).
bool pe_tangent_ok(SmProtoEdge* e)
{
    for (ULONG k = 0; k < e->GetNumEdgeDefinitions(); k++)
        if (e->GetEdgeDefinitions()[k]->GetXSectCurve3d() == NULL)
            return false;
    SmVector3d t;
    return e->EvaluateMid(NULL, &t) == SM_SUCCESS;
}

bool ed_tangent_ok(SmEdgeDefinition* d)
{
    if (d->GetXSectCurve3d() == NULL)
        return false;
    SmVector3d t;
    return d->EvaluateMid(NULL, &t) == SM_SUCCESS;
}

// CanCombineEdgeDefinitions replayed up to its dereferences: false when sm_AreConnected or
// sm_AreSameCurve would dereference a bad pointer.
bool can_combine_eds_ok(const Pool& pool, SmEdgeDefinition* d1, SmEdgeDefinition* d2)
{
    SmTopology *t11 = d1->GetSrcTopo(1), *t12 = d1->GetSrcTopo(2);
    SmTopology *t21 = d2->GetSrcTopo(1), *t22 = d2->GetSrcTopo(2);
    if (!t11 || !t12 || !t21 || !t22)
        return true;
    if (t11 == t21 && t12 == t22)
        return true;
    if (connect_crashes(pool, t11, t21))
        return false;
    if (!sc_pt_p1::sm_AreConnected(t11, t21))
        return true;
    if (connect_crashes(pool, t12, t22))
        return false;
    if (!sc_pt_p1::sm_AreConnected(t12, t22))
        return true;
    const SmCurve *c1 = d1->GetXSectCurve3d(), *c2 = d2->GetXSectCurve3d();
    return !(c1 != c2 && (c1 == NULL || c2 == NULL));
}

// CanCombineVertexDefinitions replayed up to its dereferences (sm_AreConnected).
bool can_combine_vds_ok(const Pool& pool, SmVertexDefinition* v1, SmVertexDefinition* v2)
{
    SmTopology *t11 = v1->GetSrcTopo(1), *t12 = v1->GetSrcTopo(2);
    SmTopology *t21 = v2->GetSrcTopo(1), *t22 = v2->GetSrcTopo(2);
    if (!t11 || !t12 || !t21 || !t22)
        return true;
    if (connect_crashes(pool, t11, t21))
        return false;
    if (!sc_pt_p1::sm_AreConnected(t11, t21))
        return true;
    return !connect_crashes(pool, t12, t22);
}

// Registers the objects a call made: listed proto vertices, their definitions, listed proto
// edges, their definitions, and those definitions' curves (3d, UV 1, UV 2).
void register_new(ProtoRun& r)
{
    if (r.mgr)
    {
        for (ULONG i = 0; i < r.mgr->GetProtoVertices().GetSize(); i++)
            if (r.pvs.ids.find(r.mgr->GetProtoVertices()[i]) == r.pvs.ids.end())
                r.pvs.add(r.mgr->GetProtoVertices()[i]);
    }
    for (size_t i = 0; i < r.pvs.p.size(); i++)
    {
        SmProtoVertex* v = r.pv((int64_t)i);
        if (!v)
            continue;
        for (ULONG k = 0; k < v->GetVertexDefinitions().GetSize(); k++)
            if (r.vds.ids.find(v->GetVertexDefinitions()[k]) == r.vds.ids.end())
                r.vds.add(v->GetVertexDefinitions()[k]);
    }
    if (r.mgr)
    {
        for (ULONG i = 0; i < r.mgr->GetProtoEdges().GetSize(); i++)
            if (r.pes.ids.find(r.mgr->GetProtoEdges()[i]) == r.pes.ids.end())
                r.pes.add(r.mgr->GetProtoEdges()[i]);
    }
    // only new definitions carry new curves (a curve no longer in the pool is deleted)
    size_t first_new = r.eds.p.size();
    for (size_t i = 0; i < r.pes.p.size(); i++)
    {
        SmProtoEdge* e = r.pe((int64_t)i);
        if (!e)
            continue;
        for (ULONG k = 0; k < e->GetEdgeDefinitions().GetSize(); k++)
            if (r.eds.ids.find(e->GetEdgeDefinitions()[k]) == r.eds.ids.end())
                r.eds.add(e->GetEdgeDefinitions()[k]);
    }
    for (size_t i = first_new; i < r.eds.p.size(); i++)
    {
        SmEdgeDefinition* d = r.ed((int64_t)i);
        if (!d)
            continue;
        for (const SmCurve* c : {d->GetXSectCurve3d(), d->GetXSectUVCurve(1), d->GetXSectUVCurve(2)})
            if (c && r.pool.ids.find(c) == r.pool.ids.end())
            {
                r.pool.ids[c] = (int64_t)r.pool.ptrs.size();
                r.pool.kinds.push_back(K_CURVE);
                r.pool.ptrs.push_back((SmObject*)c);
                r.pool.alive.push_back(true);
            }
    }
}

void put_pc_pair(std::vector<double>& out, const SmPointClassification& a, const SmPointClassification& b,
                 const Pool& pool)
{
    put_pc(out, a, pool);
    put_pc(out, b, pool);
}

// Runs one P2 op (its arguments already past the code). SC_ERR_ARG for malformed arguments.
// Only the audited direct-UV segment continuation; never preflight its classification.
bool unit_segment(const SmCurve* curve, ULONG dim)
{
    if (curve == NULL || curve->GetDim() != dim || curve->GetType() != SmBSplineCurve_TYPE)
        return false;
    const SmBSplineCurve* bs = (const SmBSplineCurve*)curve;
    if (bs->GetDegree() != 1 || bs->GetNumberControlPoints() != 2 || bs->IsRational() != FALSE)
        return false;
    SmTArray<double> knots;
    SmTArray<SmPoint3d> points;
    SmTArray<double> weights;
    if (bs->GetKnotsAll(knots) != SM_SUCCESS || knots.GetSize() != 4 ||
        knots[0] != 0.0 || knots[1] != 0.0 || knots[2] != 1.0 || knots[3] != 1.0 ||
        bs->GetControlPolygon(points, weights) != SM_SUCCESS || points.GetSize() != 2)
        return false;
    for (ULONG i = 0; i < points.GetSize(); ++i)
        if (!std::isfinite(points[i].x) || !std::isfinite(points[i].y) || !std::isfinite(points[i].z))
            return false;
    return true;
}

bool face_update_edges_ok(ProtoRun& r, int which, SmFace* old_face)
{
    if (r.eds.p.empty())
        return true;
    if (r.eds.p.size() != 1 || r.pes.p.size() != 1 || r.pvs.p.size() != 2 ||
        r.ed(0) == NULL || r.pe(0) == NULL || r.pv(0) == NULL || r.pv(1) == NULL ||
        r.first_ed_final_initialized != 3)
        return false;
    SmEdgeDefinition* ed = r.ed(0);
    SmProtoEdge* pe = r.pe(0);
    const auto& pes = r.mgr->GetProtoEdges();
    const auto& pvs = r.mgr->GetProtoVertices();
    const auto& definitions = pe->GetEdgeDefinitions();
    if (pes.GetSize() != 1 || pes[0] != pe || pvs.GetSize() != 2 || pvs[0] != r.pv(0) || pvs[1] != r.pv(1) ||
        pe->GetStartProtoVertex() != r.pv(0) || pe->GetEndProtoVertex() != r.pv(1) ||
        definitions.GetSize() != 1 || definitions[0] != ed || ed->GetProtoEdge() != pe)
        return false;
    for (int i = 0; i < 2; ++i)
    {
        const auto& incident = r.pv(i)->GetProtoEdges();
        if (incident.GetSize() != 1 || incident[0] != pe)
            return false;
    }
    SmTopology* other = ed->GetSrcTopo(3 - which);
    SmCurve* c3 = ed->GetXSectCurve3d();
    SmCurve* uv = ed->GetXSectUVCurve(which);
    SmExtent1d interval = ed->GetXSectInterval();
    if (ed->GetSrcTopo(which) != old_face || !r.pool.live_ptr(other) || r.pool.kind_of(other) != K_REGION ||
        !r.pool.live_ptr(c3) || !r.pool.live_ptr(uv) || c3 == uv || ed->GetXSectUVCurve(3 - which) != NULL ||
        interval.GetMin() != 0.0 || interval.GetMax() != 1.0 ||
        pe->GetXSectInterval().GetMin() != 0.0 || pe->GetXSectInterval().GetMax() != 1.0 ||
        !unit_segment(c3, 3) || !unit_segment(uv, 2))
        return false;
    SmPoint3d midpoint;
    return uv->EvaluatePoint(0.5, midpoint) == SM_SUCCESS && std::isfinite(midpoint.x) && std::isfinite(midpoint.y);
}

int32_t run_p2_op(ProtoRun& r, Rd& rd, int64_t op, std::vector<double>& out, double& status, bool& skip)
{
    SmProtoTopologyManager* mgr = r.mgr;
    if (mgr == NULL)
        skip = true;
    bool ok = true;
    auto pv_or_null = [&](int64_t j) -> SmProtoVertex* {
        if (j == -1)
            return NULL;
        if (!r.pvs.live(j))
            ok = false;
        return r.pv(j);
    };
    auto obj_or_null = [&](int64_t j, int64_t kind) -> SmObject* {
        if (j == -1)
            return NULL;
        if (!r.pool.is(j, kind))
        {
            ok = false;
            return NULL;
        }
        return r.pool.ptrs[(size_t)j];
    };
    auto curve_live = [&](const SmCurve* c) { return c != NULL && r.pool.live_ptr(c); };
    switch (op)
    {
    case P2_VD_IMPRINT:
    {
        SmVertexDefinition* d = r.vd(rd.idx());
        if (skip || !d)
        {
            skip = true;
            break;
        }
        // MergeIntoObject: bare regions, faces and edges have no B-rep; a vertex merges, after
        // which the manager is reached through the proto vertex
        for (int w = 1; w <= 2 && !skip; w++)
        {
            const SmObject* ob = d->GetBrepPointClassif(w)->GetObject();
            if (ob == NULL)
                break;
            int64_t kind = r.pool.kind_of(ob);
            if (kind == K_REGION || kind == K_FACE || kind == K_EDGE)
                skip = true;
            else if (kind != K_VERTEX)
                break;
            else if (d->GetProtoVertex() == NULL)
                skip = true;
        }
        if (skip)
            break;
        status = st(d->Imprint());
        out.push_back(r.pool.id(d->GetFinalVertex(1)));
        out.push_back(r.pool.id(d->GetFinalVertex(2)));
        break;
    }
    case P2_ED_IMPRINT:
    {
        SmEdgeDefinition* d = r.ed(rd.idx());
        if (skip || !d || d->GetProtoEdge() == NULL)
        {
            skip = true;
            break;
        }
        SmProtoVertex* sv = d->GetProtoEdge()->GetStartProtoVertex();
        SmProtoVertex* ev = d->GetProtoEdge()->GetEndProtoVertex();
        if (sv == NULL || ev == NULL)
        {
            skip = true;
            break;
        }
        // the four GetFinalVertex calls, stopping at the first NULL (NER)
        struct Call
        {
            SmProtoVertex* v;
            int w;
            SmBoolean at_start;
        } calls[4] = {{sv, 1, TRUE}, {ev, 1, FALSE}, {sv, 2, TRUE}, {ev, 2, FALSE}};
        bool all = true;
        for (const Call& c : calls)
        {
            if (c.v->GetNumVertexDefinitions() > 0)
            {
                if (d->GetXSectCurve3d() == NULL)
                    skip = true;
                for (ULONG k = 0; k < c.v->GetNumVertexDefinitions(); k++)
                {
                    SmVertex* fv = c.v->GetVertexDefinitions()[k]->GetFinalVertex(c.w);
                    if (fv && connect_crashes(r.pool, d->GetSrcTopo(c.w), fv))
                        skip = true;
                }
            }
            if (skip)
                break;
            if (c.v->GetFinalVertex(c.w, d, c.at_start) == NULL)
            {
                all = false;
                break;
            }
        }
        // MakeEdgeInFace on bare faces
        if (!skip && all && (d->GetSrcTopo_TYPE(1) == SM_PC_FACE || d->GetSrcTopo_TYPE(2) == SM_PC_FACE))
            skip = true;
        if (skip)
            break;
        status = st(d->Imprint());
        out.push_back(r.pool.id(d->GetFinalEdge(1)));
        out.push_back(r.pool.id(d->GetFinalEdge(2)));
        break;
    }
    case P2_PV_RESOLVE:
    {
        SmProtoVertex* v = r.pv(rd.idx());
        if (skip || !v)
        {
            skip = true;
            break;
        }
        std::vector<const void*> before = as_vec(v->GetVertexDefinitions());
        if (has_dup(before))
            skip = true;
        for (const void* x : before)
            if (((SmVertexDefinition*)x)->GetProtoVertex() == NULL)
                skip = true;
        if (skip)
            break;
        // SmVertexDefinition::Resolve is idempotent: resolve now, then replay the pairwise
        // deletions to see which pairs CanCombineVertexDefinitions compares
        for (const void* x : before)
        {
            int s = 0;
            ((SmVertexDefinition*)x)->Resolve(s);
        }
        std::vector<SmVertexDefinition*> vv;
        for (const void* x : before)
            vv.push_back((SmVertexDefinition*)x);
        if (vv.size() >= 2)
            for (size_t ii = 0; ii + 1 < vv.size() && !skip; ii++)
            {
                if (vv[ii] == NULL)
                    continue;
                for (size_t jj = ii + 1; jj < vv.size(); jj++)
                {
                    if (vv[jj] == NULL)
                        continue;
                    if (!can_combine_vds_ok(r.pool, vv[ii], vv[jj]))
                    {
                        skip = true;
                        break;
                    }
                    ULONG which = 0;
                    mgr->CanCombineVertexDefinitions(vv[ii], vv[jj], which);
                    if (which == 1)
                    {
                        vv[ii] = NULL;
                        break;
                    }
                    if (which == 2)
                        vv[jj] = NULL;
                }
            }
        if (skip)
            break;
        status = st(v->Resolve());
        std::vector<const void*> after = as_vec(v->GetVertexDefinitions());
        for (const void* x : before)
            if (std::find(after.begin(), after.end(), x) == after.end())
                r.vds.kill(x);
        break;
    }
    case P2_PV_ADD_VD_PCS:
    {
        SmProtoVertex* v = r.pv(rd.idx());
        SmPointClassification pc1, pc2;
        ok = read_pc(rd, r.pool, pc1) && ok;
        ok = read_pc(rd, r.pool, pc2) && ok;
        if (skip || !v || !ok)
        {
            skip = true;
            break;
        }
        SmTopology* t1 = SM_CAST_PTR(SmTopology, pc1.GetObject());
        SmTopology* t2 = SM_CAST_PTR(SmTopology, pc2.GetObject());
        if (t1 && t2 && v->FindMatchingVertexDefinition(t1, t2) == NULL && !position_ok(pc1, pc2))
        {
            skip = true;
            break;
        }
        SmVertexDefinition* nv = NULL;
        SmBoolean created = FALSE;
        status = st(v->AddVertexDefinition(pc1, pc2, nv, &created));
        register_new(r);
        out.push_back(r.vds.id(nv));
        out.push_back(flg(created));
        put_pc_pair(out, pc1, pc2, r.pool);
        break;
    }
    case P2_PE_FIND_MATCH:
    case P2_PE_ADD_ED_CIS:
    {
        SmProtoEdge* e = r.pe(rd.idx());
        CiIn c1, c2;
        ok = read_ci(rd, r.pool, c1) && ok;
        ok = read_ci(rd, r.pool, c2) && ok;
        if (skip || !e || !ok || !find_match_ok(e, c1, c2))
        {
            skip = true;
            break;
        }
        if (op == P2_PE_FIND_MATCH)
        {
            SmBoolean rev = TRUE;
            out.push_back(r.eds.id(e->FindMatchingEdgeDefinition(*c1.ci, *c2.ci, rev)));
            out.push_back(flg(rev));
            break;
        }
        // HANG-12 guard (mirrored by the runner)
        if (e->GetNumEdgeDefinitions() > 0 && zero_len(e->GetXSectInterval()))
        {
            skip = true;
            break;
        }
        SmBoolean rev = FALSE;
        if (e->FindMatchingEdgeDefinition(*c1.ci, *c2.ci, rev) == NULL)
        {
            // the copy of the curve, then the definition's constructor reads both objects' types
            const SmCurve* c = c1.cc.GetCurve();
            if (c == NULL)
            {
                skip = true;
                break;
            }
            SmCurve* tmp = NULL;
            SmStatus cs = c->Copy(the_context(), tmp);
            bool copied = cs == SM_SUCCESS && tmp != NULL;
            delete tmp;
            if (copied && (SM_CAST_PTR(SmTopology, c1.ci->m_vMid.GetObject()) == NULL ||
                           SM_CAST_PTR(SmTopology, c2.ci->m_vMid.GetObject()) == NULL))
                skip = true;
            if (copied && e->GetNumEdgeDefinitions() > 0 && !pe_tangent_ok(e))
                skip = true;
            if (skip)
                break;
        }
        status = st(e->AddEdgeDefinition(*c1.ci, *c2.ci));
        register_new(r);
        break;
    }
    case P2_PE_ADD_ED:
    {
        SmProtoEdge* e = r.pe(rd.idx());
        SmEdgeDefinition* d = r.ed(rd.idx());
        if (skip || !e || !d)
        {
            skip = true;
            break;
        }
        if (e->GetNumEdgeDefinitions() > 0 &&
            (zero_len(e->GetXSectInterval()) || !pe_tangent_ok(e) || !ed_tangent_ok(d)))
        {
            skip = true;
            break;
        }
        status = st(e->AddEdgeDefinition(d));
        break;
    }
    case P2_PE_COMBINE:
    {
        SmProtoEdge* e = r.pe(rd.idx());
        SmProtoEdge* o = r.pe(rd.idx());
        if (skip || !e || !o)
        {
            skip = true;
            break;
        }
        if (e != o)
        {
            if (o->GetStartProtoVertex() != e->GetStartProtoVertex() &&
                (o->GetStartProtoVertex() == NULL || e->GetStartProtoVertex() == NULL))
                skip = true;
            if (o->GetEndProtoVertex() != e->GetEndProtoVertex() &&
                (o->GetEndProtoVertex() == NULL || e->GetEndProtoVertex() == NULL))
                skip = true;
            // conservative guard (mirrored by the runner): when a definition can be added to
            // a non-empty proto edge, every curve evaluates at its mid and no interval is
            // zero length
            ULONG ne = e->GetNumEdgeDefinitions(), no = o->GetNumEdgeDefinitions();
            if (no >= 1 && (ne > 0 || no >= 2))
            {
                if (ne > 0 && (zero_len(e->GetXSectInterval()) || !pe_tangent_ok(e)))
                    skip = true;
                for (ULONG k = 0; k < no; k++)
                {
                    SmEdgeDefinition* d = o->GetEdgeDefinitions()[k];
                    if (zero_len(d->GetXSectInterval()) || !ed_tangent_ok(d))
                        skip = true;
                }
            }
        }
        if (skip)
            break;
        e->CombineProtoEdge(o);
        break;
    }
    case P2_PE_RESOLVE:
    {
        SmProtoEdge* e = r.pe(rd.idx());
        if (skip || !e)
        {
            skip = true;
            break;
        }
        std::vector<const void*> before = as_vec(e->GetEdgeDefinitions());
        if (has_dup(before))
        {
            skip = true;
            break;
        }
        std::vector<SmEdgeDefinition*> v;
        for (const void* x : before)
            v.push_back((SmEdgeDefinition*)x);
        std::vector<SmEdgeDefinition*> dead;
        bool cleared = false;
        if (!v.empty() && e->GetStartProtoVertex() == e->GetEndProtoVertex())
        {
            SmCurve* c = v[0]->GetXSectCurve3d();
            if (c == NULL)
            {
                skip = true;
                break;
            }
            double tol = e->GetTolerance();
            double len = c->ApproximateLength(e->GetXSectInterval(), 5);
            if (len < 2 * tol)
            {
                dead = v;
                cleared = true;
            }
            else
            {
                SmPoint3d p0, p1;
                c->GetEnds(p0, p1);
                if (p0.DistanceBetween(p1) > 2 * tol)
                {
                    SmProtoVertex* sv = e->GetStartProtoVertex();
                    if (sv == NULL)
                    {
                        skip = true;
                        break;
                    }
                    SmVertexDefinition* vd0 = sv->GetClosestVertexDefinition(p0);
                    bool add = sv->GetNumVertexDefinitions() < 2 || vd0 == sv->GetClosestVertexDefinition(p1);
                    if (add && (vd0 == NULL || !position_ok(*vd0->GetBrepPointClassif(1), *vd0->GetBrepPointClassif(2))))
                    {
                        skip = true;
                        break;
                    }
                }
            }
        }
        if (!cleared && v.size() >= 2)
        {
            // the pairwise loop, replayed with the unsigned index steps of upstream
            size_t ii = 0;
            while (!skip && ii < v.size() - 1)
            {
                SmEdgeDefinition* d1 = v[ii];
                for (size_t jj = ii + 1; jj < v.size(); jj++)
                {
                    SmEdgeDefinition* d2 = v[jj];
                    if (!can_combine_eds_ok(r.pool, d1, d2))
                    {
                        skip = true;
                        break;
                    }
                    ULONG which = 0;
                    mgr->CanCombineEdgeDefinitions(d1, d2, which);
                    if (which == 1)
                    {
                        dead.push_back(d1);
                        v.erase(v.begin() + (ptrdiff_t)ii);
                        ii--;
                        break;
                    }
                    if (which == 2)
                    {
                        dead.push_back(d2);
                        v.erase(v.begin() + (ptrdiff_t)jj);
                        jj--;
                    }
                }
                ii++;
            }
        }
        // the curves Destruct deletes (a curve shared by two deleted definitions is freed twice)
        std::vector<const void*> curves;
        for (SmEdgeDefinition* d : dead)
            for (const SmCurve* c : {d->GetXSectCurve3d(), d->GetXSectUVCurve(1), d->GetXSectUVCurve(2)})
                if (c)
                    curves.push_back(c);
        // and a deleted curve still used by a definition that stays is used after free
        std::vector<const void*> kept;
        for (const void* x : before)
            if (std::find(dead.begin(), dead.end(), (SmEdgeDefinition*)x) == dead.end())
                for (const SmCurve* c : {((SmEdgeDefinition*)x)->GetXSectCurve3d(), ((SmEdgeDefinition*)x)->GetXSectUVCurve(1),
                                         ((SmEdgeDefinition*)x)->GetXSectUVCurve(2)})
                    if (c && std::find(curves.begin(), curves.end(), (const void*)c) != curves.end())
                        skip = true;
        if (skip || has_dup(curves))
        {
            skip = true;
            break;
        }
        status = st(e->Resolve());
        std::vector<const void*> after = as_vec(e->GetEdgeDefinitions());
        std::vector<double> ids;
        for (const void* x : before)
            if (std::find(after.begin(), after.end(), x) == after.end())
                r.eds.kill(x);
        for (const void* c : curves)
        {
            ids.push_back(r.pool.id(c));
            r.pool.kill(c);
        }
        std::sort(ids.begin(), ids.end());
        out.push_back((double)ids.size());
        out.insert(out.end(), ids.begin(), ids.end());
        register_new(r);
        break;
    }
    case P2_MGR_CREATE_VD:
    case P2_MGR_CREATE_PV:
    {
        SmPointClassification pc1, pc2;
        ok = read_pc(rd, r.pool, pc1) && ok;
        ok = read_pc(rd, r.pool, pc2) && ok;
        if (skip || !ok)
        {
            skip = true;
            break;
        }
        bool reached = op == P2_MGR_CREATE_VD ||
                       (sc_pt_p1::sm_CanImprint(pc1.GetPointClass()) && sc_pt_p1::sm_CanImprint(pc2.GetPointClass()));
        if (reached && !position_ok(pc1, pc2))
        {
            skip = true;
            break;
        }
        if (op == P2_MGR_CREATE_VD)
        {
            SmVertexDefinition* nv = NULL;
            status = st(mgr->CreateVertexDefinition(pc1, pc2, nv));
            if (nv)
                r.vds.add(nv);
            out.push_back(r.vds.id(nv));
        }
        else
        {
            SmProtoVertex* nv = NULL;
            status = st(mgr->CreateProtoVertex(pc1, pc2, nv));
            register_new(r);
            out.push_back(r.pvs.id(nv));
        }
        put_pc_pair(out, pc1, pc2, r.pool);
        break;
    }
    case P2_MGR_FIND_OR_CREATE_PV:
    {
        SmPoint3d p = rd.vec();
        if (skip)
            break;
        SmProtoVertex* nv = NULL;
        SmBoolean created = FALSE;
        status = st(mgr->FindOrCreateProtoVertex(p, nv, &created));
        register_new(r);
        out.push_back(r.pvs.id(nv));
        out.push_back(flg(created));
        break;
    }
    case P2_MGR_FIND_PE:
    {
        SmProtoVertex* v1 = pv_or_null(rd.idx());
        SmProtoVertex* v2 = pv_or_null(rd.idx());
        SmPointClassification pc1, pc2;
        ok = read_pc(rd, r.pool, pc1) && ok;
        ok = read_pc(rd, r.pool, pc2) && ok;
        SmCurve* c = (SmCurve*)obj_or_null(rd.idx(), K_CURVE);
        double a = rd.num();
        double b = rd.num();
        if (skip || !ok)
        {
            skip = true;
            break;
        }
        SmExtent1d dom(a, b);
        SmBoolean ident = FALSE;
        if (v1 && v2)
        {
            SmTArray<SmProtoEdge*> common;
            v1->GetProtoEdges().FindCommonElements(v2->GetProtoEdges(), common);
            for (ULONG k = 0; k < common.GetSize(); k++)
            {
                if (!same_pe_ok(common[k], v1, v2, c))
                {
                    skip = true;
                    break;
                }
                if (common[k]->IsSameProtoEdge(v1, v2, pc1, pc2, c, dom, ident))
                    break;
            }
        }
        if (skip)
            break;
        ident = TRUE;
        SmProtoEdge* found = mgr->FindProtoEdge(v1, v2, pc1, pc2, c, dom, ident);
        out.push_back(r.pes.id(found));
        out.push_back(flg(ident));
        break;
    }
    case P2_MGR_CREATE_PE:
    {
        SmProtoVertex* v1 = pv_or_null(rd.idx());
        SmProtoVertex* v2 = pv_or_null(rd.idx());
        CiIn c1, c2;
        ok = read_ci(rd, r.pool, c1) && ok;
        ok = read_ci(rd, r.pool, c2) && ok;
        if (skip || !ok || c1.cc.GetCurve() == NULL)
        {
            skip = true;
            break;
        }
        // a copy that cannot be made or trimmed, or a midpoint off topology, returns before
        // the proto vertices are dereferenced
        const SmCurve* c = c1.cc.GetCurve();
        SmCurve* tmp = NULL;
        SmStatus cs = c->Copy(the_context(), tmp);
        bool reached = cs == SM_SUCCESS && tmp != NULL;
        if (reached)
        {
            SmExtent1d dom = c1.ci->GetInterval();
            reached = tmp->Trim(dom) == SM_SUCCESS;
        }
        delete tmp;
        if (reached && SM_CAST_PTR(SmTopology, c1.ci->m_vMid.GetObject()) &&
            SM_CAST_PTR(SmTopology, c2.ci->m_vMid.GetObject()) && (v1 == NULL || v2 == NULL))
        {
            skip = true;
            break;
        }
        SmProtoEdge* ne = NULL;
        SmStatus cs2 = mgr->CreateProtoEdge(v1, v2, *c1.ci, *c2.ci, ne);
        status = st(cs2);
        register_new(r);
        // on an error return rpNewPE is the unlisted proto edge upstream leaks
        out.push_back(cs2 == SM_SUCCESS ? r.pes.id(ne) : -1.0);
        break;
    }
    case P2_MGR_REMOVE_ED:
    {
        SmEdgeDefinition* d = r.ed(rd.idx());
        bool flag = rd.flag();
        if (skip || !d || d->GetProtoEdge() == NULL)
        {
            skip = true;
            break;
        }
        SmProtoEdge* owner = d->GetProtoEdge();
        std::vector<const void*> left = as_vec(owner->GetEdgeDefinitions());
        auto it = std::find(left.begin(), left.end(), (const void*)d);
        if (it != left.end())
            left.erase(it);
        bool dies = flag && left.empty();
        status = st(mgr->RemoveEdgeDefinition(d, flag ? TRUE : FALSE));
        if (dies)
            r.pes.kill(owner);
        break;
    }
    case P2_MGR_UPDATE_EDGES:
    {
        int64_t w = rd.idx();
        SmEdge* old_e = (SmEdge*)obj_or_null(rd.idx(), K_EDGE);
        int64_t n = rd.idx();
        SmTArray<SmEdge*> arr;
        for (int64_t k = 0; k < n && k < 4 && !rd.bad; k++)
        {
            SmEdge* x = (SmEdge*)obj_or_null(rd.idx(), K_EDGE);
            if (x == NULL)
                ok = false;
            arr.Add(x);
        }
        int64_t j = rd.idx();
        SmVertexDefinition* cvd = NULL;
        if (j != -1)
        {
            cvd = r.vd(j);
            if (!cvd)
                ok = false;
        }
        if (skip || !ok || (w != 1 && w != 2) || n < 0 || n > 4)
        {
            skip = true;
            break;
        }
        SmEdge* new_e = NULL;
        if (old_e && n == 2)
            new_e = arr[0] == old_e ? arr[1] : arr[1] == old_e ? arr[0] : NULL;
        if (new_e)
        {
            // past the early returns: the old edge's curve, the calling definition, every
            // definition's proto edge, the definitions' curves where they are dropped onto
            // or trimmed, and the new edge's curve where a midpoint is dropped onto it
            if (!curve_live(old_e->GetCurve()) || cvd == NULL)
                skip = true;
            SmTArray<SmEdgeDefinition*> all;
            mgr->GetAllEdgeDefinitions(all);
            for (ULONG k = 0; k < all.GetSize() && !skip; k++)
            {
                SmEdgeDefinition* d = all[k];
                SmProtoEdge* pe = d->GetProtoEdge();
                if (pe == NULL)
                {
                    skip = true;
                    break;
                }
                SmProtoVertex* cpv = cvd->GetProtoVertex();
                if ((pe->GetStartProtoVertex() == cpv || pe->GetEndProtoVertex() == cpv) && d->GetXSectCurve3d() == NULL)
                    skip = true;
                if (d->GetSrcTopo_TYPE((int)w) == SM_PC_EDGE && d->GetSrcTopo((int)w) == old_e &&
                    (d->GetXSectCurve3d() == NULL || !curve_live(new_e->GetCurve())))
                    skip = true;
            }
        }
        if (skip)
            break;
        status = st(mgr->UpdateEdges((int)w, old_e, arr, cvd));
        break;
    }
    case P2_MGR_UPDATE_FACES:
    {
        int64_t which = rd.idx(), old_id = rd.idx(), new_id = rd.idx();
        int64_t split_id = rd.idx(), other_id = rd.idx(), calling_id = rd.idx();
        // Bare-face scripts remain unexecuted; begin with real vertex-definition fixtures.
        if (skip || which < 1 || which > 2 || !r.pool.is(old_id, K_FACE) || !r.pool.is(new_id, K_FACE) || !r.pool.face_breps.count(old_id) ||
            !r.pool.face_breps.count(new_id) || split_id != -1 || other_id != -1 || calling_id != -1 ||
            !r.pfs.p.empty() ||
            r.pool.face_breps[old_id] != r.pool.operands[which - 1] ||
            r.pool.face_breps[new_id] != r.pool.operands[which - 1])
        {
            skip = true;
            break;
        }
        SmFace* old_face = (SmFace*)r.pool.ptrs[(size_t)old_id];
        SmFace* new_face = (SmFace*)r.pool.ptrs[(size_t)new_id];
        if (!face_update_edges_ok(r, (int)which, old_face))
        {
            skip = true;
            break;
        }
        status = st(mgr->UpdateFaces((int)which, old_face, new_face, NULL, NULL, NULL));
        for (SmFace* face : {old_face, new_face})
        {
            SmExtent2d domain = face->GetUVDomain();
            out.insert(out.end(), {domain.GetMin().x, domain.GetMin().y, domain.GetMax().x, domain.GetMax().y});
            SmTArray<SmLoopuse*> loops;
            face->GetUpwardLoopuses(loops);
            out.push_back((double)loops.GetSize());
            int64_t edge_count = 0, trim_count = 0;
            for (ULONG li = 0; li < loops.GetSize(); ++li)
            {
                SmTArray<SmEdgeuse*> edges;
                loops[li]->GetEdgeuses(edges);
                edge_count += edges.GetSize();
                for (ULONG ei = 0; ei < edges.GetSize(); ++ei)
                    if (edges[ei]->GetUVTrimCurve() != NULL)
                        ++trim_count;
            }
            out.insert(out.end(), {(double)edge_count, (double)trim_count, (double)face->GetTolerance()});
            auto probe = r.pool.face_domain_probes.find((int64_t)r.pool.id(face));
            out.push_back(probe == r.pool.face_domain_probes.end() ? 0.0 : 1.0);
            if (probe != r.pool.face_domain_probes.end())
            {
                const SmExtent2d& prepared = probe->second.second;
                out.insert(out.end(), {probe->second.first, prepared.GetMin().x, prepared.GetMin().y,
                    prepared.GetMax().x, prepared.GetMax().y});
            }
        }
        break;
    }
    case P2_MGR_CAN_COMBINE_VDS:
    {
        SmVertexDefinition* d1 = r.vd(rd.idx());
        SmVertexDefinition* d2 = r.vd(rd.idx());
        if (skip || !d1 || !d2 || !can_combine_vds_ok(r.pool, d1, d2))
        {
            skip = true;
            break;
        }
        ULONG which = 99;
        status = st(mgr->CanCombineVertexDefinitions(d1, d2, which));
        out.push_back((double)which);
        break;
    }
    default: return SC_ERR_ARG;
    }
    return SC_OK;
}

int32_t run_proto(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    ProtoRun r;
    r.pool.operands[0] = new (ctx) SmBrep();
    r.pool.operands[1] = new (ctx) SmBrep();
    int64_t n_obj = rd.idx();
    if (n_obj < 0 || n_obj > 64)
        return SC_ERR_ARG;
    for (int64_t i = 0; i < n_obj && !rd.bad; i++)
    {
        int64_t kind = rd.idx();
        SmObject* p = NULL;
        switch (kind)
        {
        case K_CURVE: p = read_curve(rd, ctx); break;
        case K_PLANE:
        case K_BSURF: p = read_surface(rd, ctx, kind); break;
        case K_VERTEX: p = new (anchor()) ScVertex(); break;
        case K_VERTEX_AT:
        {
            double x = rd.num();
            double y = rd.num();
            double z = rd.num();
            double tol = rd.num();
            if (rd.bad)
                return SC_ERR_ARG;
            p = new (anchor()) ScVertex(x, y, z, tol);
            kind = K_VERTEX;
            break;
        }
        case K_EDGE:
        {
            int64_t c = rd.idx();
            if (!r.pool.is_curve(c))
                return SC_ERR_ARG;
            p = new (anchor()) ScEdge((SmCurve*)r.pool.ptrs[(size_t)c]);
            break;
        }
        case K_TRIMMED_FACE:
        {
            int64_t operand = rd.idx(), surface = rd.idx();
            double u0 = rd.num(), v0 = rd.num(), u1 = rd.num(), v1 = rd.num();
            double a0 = rd.num(), b0 = rd.num(), a1 = rd.num(), b1 = rd.num();
            double du = rd.num(), dv = rd.num();
            if (operand < 1 || operand > 2 || (!r.pool.is(surface, K_PLANE) && !r.pool.is(surface, K_BSURF)) ||
                r.pool.owned_surfaces.count(surface) || u0 > u1 || v0 > v1 || a0 > a1 || b0 > b1)
                return SC_ERR_ARG;
            SmBrep* body = r.pool.operands[operand - 1];
            SmFace* face = NULL;
            SmStatus made = body->CreateFaceFromSurface((SmSurface*)r.pool.ptrs[(size_t)surface],
                SmExtent2d(SmPoint2d(u0, v0), SmPoint2d(u1, v1)), face);
            if (made != SM_SUCCESS || face == NULL)
                return SC_ERR_ARG;
            r.pool.face_initial_domains[(int64_t)r.pool.ptrs.size()] =
                SmExtent2d(SmPoint2d(a0, b0), SmPoint2d(a1, b1));
            r.pool.face_trim_shifts[(int64_t)r.pool.ptrs.size()] = SmPoint2d(du, dv);
            p = face;
            kind = K_FACE;
            r.pool.face_breps[(int64_t)r.pool.ptrs.size()] = body;
            r.pool.owned_surfaces.insert(surface);
            break;
        }
        case K_FACE:
        {
            int64_t s = rd.idx();
            if (!r.pool.is(s, K_PLANE) && !r.pool.is(s, K_BSURF))
                return SC_ERR_ARG;
            p = new (anchor()) ScFace((SmSurface*)r.pool.ptrs[(size_t)s]);
            break;
        }
        case K_REGION: p = new (anchor()) ScTopoStandIn(SmRegion_TYPE); break;
        case K_LOOP: p = new (anchor()) ScTopoStandIn(SmLoop_TYPE); break;
        case OBJ_EDGE_IVL:
        {
            int64_t c = rd.idx();
            double a = rd.num();
            double b = rd.num();
            double tol = rd.num();
            if (!r.pool.is_curve(c))
                return SC_ERR_ARG;
            p = new (anchor()) ScEdge((SmCurve*)r.pool.ptrs[(size_t)c], a, b, tol);
            kind = K_EDGE;
            break;
        }
        case OBJ_VERTEX_TOL:
            p = new (anchor()) ScVertex(rd.num());
            kind = K_VERTEX;
            break;
        default: return SC_ERR_ARG;
        }
        r.pool.ids[p] = (int64_t)r.pool.ptrs.size();
        r.pool.kinds.push_back(kind);
        r.pool.ptrs.push_back(p);
        r.pool.alive.push_back(true);
    }
    double approx_tol = rd.num();
    double capture = rd.num();
    double override_tol = rd.num();
    if (rd.bad)
        return SC_ERR_ARG;
    for (const auto& entry : r.pool.face_initial_domains)
    {
        SmFace* face = (SmFace*)r.pool.ptrs[(size_t)entry.first];
        face->SetUVDomain(entry.second);
        SmPoint2d shift = r.pool.face_trim_shifts[entry.first];
        if (shift.x != 0.0 || shift.y != 0.0)
        {
            double tmp;
            if (face->CreateUVTrimCurves(FALSE, NULL, NULL, tmp, tmp, tmp, tmp, tmp, tmp) != SM_SUCCESS)
                return SC_ERR_ARG;
            int64_t transformed = 0;
            SmAxis2Placement transform;
            transform.Translate(SmVector3d(shift.x, shift.y, 0.0));
            SmTArray<SmLoopuse*> loops;
            face->GetUpwardLoopuses(loops);
            for (ULONG li = 0; li < loops.GetSize(); ++li)
            {
                SmTArray<SmEdgeuse*> edges;
                loops[li]->GetEdgeuses(edges);
                for (ULONG ei = 0; ei < edges.GetSize(); ++ei)
                {
                    SmCurve* curve = edges[ei]->GetUVTrimCurve();
                    if (curve != NULL)
                    {
                        if (curve->Transform(transform, NULL) != SM_SUCCESS)
                            return SC_ERR_ARG;
                        ++transformed;
                    }
                }
            }
            if (transformed == 0)
                return SC_ERR_ARG;
            SmExtent2d domain;
            SmStatus probe_status = face->CalculateUVDomainFromUVTrimCurves(domain);
            r.pool.face_domain_probes[entry.first] = {st(probe_status), domain};
        }
    }
    SmBrep* b1 = r.pool.operands[0];
    SmBrep* b2 = r.pool.operands[1];
    SmTopologyIntersector* ti = new SmTopologyIntersector(ctx, b1, b2, approx_tol, 0.0);
    r.mgr = new (b1) SmProtoTopologyManager(b1, b2, ti, capture, override_tol);

    int64_t n_ops = rd.idx();
    if (rd.bad || n_ops < 0 || n_ops > 256)
        return SC_ERR_ARG;
    for (int64_t i = 0; i < n_ops; i++)
    {
        int64_t op = rd.idx();
        if (rd.bad)
            return SC_ERR_ARG;
        o.push_back((double)op);
        std::vector<double> out;
        double status = 0.0;
        bool skip = r.mgr == NULL; // nothing after the manager's destruction
        SmProtoTopologyManager* mgr = r.mgr;
        auto pv_or_null = [&](int64_t j, bool& okk) -> SmProtoVertex* {
            if (j == -1)
                return NULL;
            if (!r.pvs.live(j))
                okk = false;
            return r.pv(j);
        };
        auto curve_or_null = [&](int64_t j, bool& okk) -> SmCurve* {
            if (j == -1)
                return NULL;
            if (!r.pool.is_curve(j))
                okk = false;
            return r.pool.is_curve(j) ? (SmCurve*)r.pool.ptrs[(size_t)j] : NULL;
        };
        auto topo = [&](int64_t j, bool& okk) -> SmTopology* {
            if (!r.pool.is_topo(j))
            {
                okk = false;
                return NULL;
            }
            return (SmTopology*)r.pool.ptrs[(size_t)j];
        };
        bool ok = true;
        switch (op)
        {
        case P_PV_NEW:
        {
            SmPoint3d p = rd.vec();
            double tol = rd.num();
            if (skip)
                break;
            SmProtoVertex* v = new (mgr) SmProtoVertex(p, tol, mgr);
            out.push_back((double)r.pvs.add(v));
            break;
        }
        case P_PV_NEW_PCS:
        {
            SmPointClassification pc1, pc2;
            ok = read_pc(rd, r.pool, pc1) && ok;
            ok = read_pc(rd, r.pool, pc2) && ok;
            SmPoint3d p = rd.vec();
            double tol = rd.num();
            if (skip || !ok)
            {
                skip = true;
                break;
            }
            SmProtoVertex* v = new (mgr) SmProtoVertex(pc1, pc2, p, tol, mgr);
            out.push_back((double)r.pvs.add(v));
            out.push_back((double)r.vds.add(v->GetVertexDefinitions()[0]));
            break;
        }
        case P_VD_NEW:
        {
            SmPointClassification pc1, pc2;
            ok = read_pc(rd, r.pool, pc1) && ok;
            ok = read_pc(rd, r.pool, pc2) && ok;
            SmPoint3d p = rd.vec();
            double tol = rd.num();
            double gap = rd.num();
            SmProtoVertex* owner = pv_or_null(rd.idx(), ok);
            if (skip || !ok)
            {
                skip = true;
                break;
            }
            SmVertexDefinition* v = new (mgr) SmVertexDefinition(&ctx, &pc1, &pc2, p, tol, gap, owner);
            out.push_back((double)r.vds.add(v));
            break;
        }
        case P_PE_NEW:
        {
            SmProtoVertex* s = pv_or_null(rd.idx(), ok);
            SmProtoVertex* e = pv_or_null(rd.idx(), ok);
            double a = rd.num();
            double b = rd.num();
            if (skip || !ok)
            {
                skip = true;
                break;
            }
            SmProtoEdge* x = new (mgr) SmProtoEdge(s, e, SmExtent1d(a, b), mgr);
            out.push_back((double)r.pes.add(x));
            break;
        }
        case P_PE_COPY:
        {
            SmProtoEdge* src = r.pe(rd.idx());
            if (skip || !src)
            {
                skip = true;
                break;
            }
            SmProtoEdge* x = new (mgr) SmProtoEdge(*src);
            out.push_back((double)r.pes.add(x));
            break;
        }
        case P_ED_NEW:
        {
            SmCurve* c3 = curve_or_null(rd.idx(), ok);
            double a = rd.num();
            double b = rd.num();
            SmTopology* s1 = topo(rd.idx(), ok);
            SmTopology* s2 = topo(rd.idx(), ok);
            double tol = rd.num();
            SmCurve* u1 = curve_or_null(rd.idx(), ok);
            SmCurve* u2 = curve_or_null(rd.idx(), ok);
            int64_t pe = rd.idx();
            if (pe != -1 && !r.pes.live(pe))
                ok = false;
            if (skip || !ok)
            {
                skip = true;
                break;
            }
            SmExtent1d ivl(a, b);
            SmEdgeDefinition* x = new (mgr) SmEdgeDefinition(c3, ivl, s1, s2, tol, u1, u2, r.pe(pe));
            out.push_back((double)r.eds.add(x));
            break;
        }
        case P_ED_COPY:
        {
            SmEdgeDefinition* src = r.ed(rd.idx());
            if (skip || !src)
            {
                skip = true;
                break;
            }
            SmEdgeDefinition* x = new (mgr) SmEdgeDefinition(*src);
            out.push_back((double)r.eds.add(x));
            break;
        }
        case P_PF_NEW:
        case P_PF_CREATE:
        {
            int64_t f1 = rd.idx();
            int64_t f2 = rd.idx();
            int64_t n = rd.idx();
            SmTArray<SmProtoEdge*> arr;
            for (int64_t k = 0; k < n && k < 16 && !rd.bad; k++)
            {
                int64_t j = rd.idx();
                if (!r.pes.live(j))
                    ok = false;
                else
                    arr.Add(r.pe(j));
            }
            if ((f1 != -1 && !r.pool.is(f1, K_FACE)) || (f2 != -1 && !r.pool.is(f2, K_FACE)) || n < 0 || n > 16)
                ok = false;
            if (skip || !ok)
            {
                skip = true;
                break;
            }
            SmFace* a = f1 == -1 ? NULL : (SmFace*)r.pool.ptrs[(size_t)f1];
            SmFace* b = f2 == -1 ? NULL : (SmFace*)r.pool.ptrs[(size_t)f2];
            if (op == P_PF_NEW)
            {
                SmProtoFace* x = new (mgr) SmProtoFace(a, b, arr, mgr);
                out.push_back((double)r.pfs.add(x));
            }
            else
            {
                SmProtoFace* x = NULL;
                status = st(mgr->CreateProtoFace(a, b, arr, x));
                out.push_back((double)r.pfs.add(x));
            }
            break;
        }
        case P_MGR_ADD_PV:
        {
            SmProtoVertex* v = r.pv(rd.idx());
            if (skip || !v)
            {
                skip = true;
                break;
            }
            mgr->AddProtoVertex(v);
            break;
        }
        case P_MGR_ADD_PE:
        {
            SmProtoEdge* e = r.pe(rd.idx());
            if (skip || !e)
            {
                skip = true;
                break;
            }
            mgr->AddProtoEdge(e);
            break;
        }
        case P_MGR_ADD_PF:
        {
            SmProtoFace* f = r.pf(rd.idx());
            if (skip || !f)
            {
                skip = true;
                break;
            }
            mgr->AddProtoFace(f);
            break;
        }
        case P_MGR_RESET:
            if (!skip)
                mgr->Reset();
            break;
        case P_PV_ADD_PE:
        {
            SmProtoVertex* v = r.pv(rd.idx());
            SmProtoEdge* e = r.pe(rd.idx());
            if (skip || !v || !e)
            {
                skip = true;
                break;
            }
            v->AddProtoEdge(e);
            break;
        }
        case P_PV_ADD_VD:
        {
            SmProtoVertex* v = r.pv(rd.idx());
            SmVertexDefinition* d = r.vd(rd.idx());
            if (skip || !v || !d)
            {
                skip = true;
                break;
            }
            status = st(v->AddVertexDefinition(d));
            break;
        }
        case P_PE_ADD_ED:
        {
            SmProtoEdge* e = r.pe(rd.idx());
            SmEdgeDefinition* d = r.ed(rd.idx());
            if (skip || !e || !d)
            {
                skip = true;
                break;
            }
            e->GetEdgeDefinitions().Add(d);
            break;
        }
        case P_PE_ADD_OVERLAP:
        {
            SmProtoEdge* e = r.pe(rd.idx());
            SmProtoEdge* e2 = r.pe(rd.idx());
            if (skip || !e || !e2)
            {
                skip = true;
                break;
            }
            e->AddOverlappingPE(e2);
            break;
        }
        case P_PE_APPEND_OVERLAPS:
        {
            SmProtoEdge* e = r.pe(rd.idx());
            int64_t n = rd.idx();
            SmTArray<SmProtoEdge*> arr;
            for (int64_t k = 0; k < n && k < 16 && !rd.bad; k++)
            {
                SmProtoEdge* x = r.pe(rd.idx());
                if (!x)
                    ok = false;
                arr.Add(x);
            }
            if (skip || !e || !ok || n < 0 || n > 16)
            {
                skip = true;
                break;
            }
            e->AppendOverlappingPEs(arr);
            break;
        }
        case P_PE_CLEAR_EDS:
        {
            SmProtoEdge* e = r.pe(rd.idx());
            if (skip || !e)
            {
                skip = true;
                break;
            }
            e->ClearEdgeDefinitions();
            break;
        }
        case P_PV_CLEAR_VDS:
        {
            SmProtoVertex* v = r.pv(rd.idx());
            if (skip || !v)
            {
                skip = true;
                break;
            }
            v->ClearVertexDefinitions();
            break;
        }
        case P_PE_SET_START:
        case P_PE_SET_END:
        {
            SmProtoEdge* e = r.pe(rd.idx());
            SmProtoVertex* v = pv_or_null(rd.idx(), ok);
            if (skip || !e || !ok)
            {
                skip = true;
                break;
            }
            if (op == P_PE_SET_START)
                e->SetStartProtoVertex(v);
            else
                e->SetEndProtoVertex(v);
            break;
        }
        case P_PE_SET_IVL:
        {
            SmProtoEdge* e = r.pe(rd.idx());
            double a = rd.num();
            double b = rd.num();
            if (skip || !e)
            {
                skip = true;
                break;
            }
            e->SetXSectInterval(SmExtent1d(a, b));
            break;
        }
        case P_PE_REPLACE_PV:
        {
            SmProtoEdge* e = r.pe(rd.idx());
            SmProtoVertex* a = pv_or_null(rd.idx(), ok);
            SmProtoVertex* b = pv_or_null(rd.idx(), ok);
            if (skip || !e || !ok)
            {
                skip = true;
                break;
            }
            e->ReplaceProtoVertex(a, b);
            break;
        }
        case P_PE_REMOVE_ED:
        {
            SmProtoEdge* e = r.pe(rd.idx());
            SmEdgeDefinition* d = r.ed(rd.idx());
            if (skip || !e || !d)
            {
                skip = true;
                break;
            }
            status = st(e->RemoveEdgeDefinition(d));
            break;
        }
        case P_PE_REMOVE_PV:
        {
            SmProtoEdge* e = r.pe(rd.idx());
            SmProtoVertex* v = pv_or_null(rd.idx(), ok);
            if (skip || !e || !ok)
            {
                skip = true;
                break;
            }
            status = st(e->RemoveProtoVertex(v));
            break;
        }
        case P_PV_REMOVE_VD:
        {
            SmProtoVertex* v = r.pv(rd.idx());
            SmVertexDefinition* d = r.vd(rd.idx());
            if (skip || !v || !d)
            {
                skip = true;
                break;
            }
            status = st(v->RemoveVertexDefinition(d));
            break;
        }
        case P_PV_REMOVE_PE:
        {
            SmProtoVertex* v = r.pv(rd.idx());
            SmProtoEdge* e = r.pe(rd.idx());
            if (skip || !v || !e)
            {
                skip = true;
                break;
            }
            status = st(v->RemoveProtoEdge(e));
            break;
        }
        case P_PE_RELATE_PF:
        {
            SmProtoEdge* e = r.pe(rd.idx());
            SmProtoFace* f = r.pf(rd.idx());
            if (skip || !e || !f)
            {
                skip = true;
                break;
            }
            e->RelateCoincidentPF(f);
            break;
        }
        case P_PF_SET_SRC:
        {
            SmProtoFace* f = r.pf(rd.idx());
            int64_t w = rd.idx();
            int64_t j = rd.idx();
            if (j != -1 && !r.pool.is_topo(j))
                ok = false;
            if (skip || !f || !ok)
            {
                skip = true;
                break;
            }
            f->SetSrcTopo((int)w, SM_PC_FACE, j == -1 ? NULL : (SmTopology*)r.pool.ptrs[(size_t)j]);
            break;
        }
        case P_VD_SET_SRC:
        {
            SmVertexDefinition* d = r.vd(rd.idx());
            int64_t w = rd.idx();
            int64_t cls = rd.idx();
            int64_t j = rd.idx();
            switch (cls)
            {
            case SM_PC_UNKNOWN:
            case SM_PC_NOTHING: ok = j == -1; break;
            case SM_PC_REGION: ok = r.pool.is(j, K_REGION); break;
            case SM_PC_FACE: ok = r.pool.is(j, K_FACE); break;
            case SM_PC_EDGE: ok = r.pool.is(j, K_EDGE); break;
            case SM_PC_VERTEX: ok = r.pool.is(j, K_VERTEX); break;
            default: ok = false;
            }
            if (skip || !d || !ok)
            {
                skip = true;
                break;
            }
            status = st(d->SetSrcTopo((int)w, (SmPointClassificationType)cls,
                                      j == -1 ? NULL : (SmTopology*)r.pool.ptrs[(size_t)j]));
            break;
        }
        case P_VD_SET_FINAL:
        {
            SmVertexDefinition* d = r.vd(rd.idx());
            int64_t w = rd.idx();
            int64_t j = rd.idx();
            if (j != -1 && !r.pool.is(j, K_VERTEX))
                ok = false;
            if (skip || !d || !ok)
            {
                skip = true;
                break;
            }
            status = st(d->SetFinalVertex((int)w, j == -1 ? NULL : (SmVertex*)r.pool.ptrs[(size_t)j]));
            break;
        }
        case P_VD_SET_POS:
        {
            SmVertexDefinition* d = r.vd(rd.idx());
            SmPoint3d p = rd.vec();
            if (skip || !d)
            {
                skip = true;
                break;
            }
            d->SetPosition(p);
            break;
        }
        case P_VD_SET_PV:
        {
            SmVertexDefinition* d = r.vd(rd.idx());
            SmProtoVertex* v = pv_or_null(rd.idx(), ok);
            if (skip || !d || !ok)
            {
                skip = true;
                break;
            }
            d->SetProtoVertex(v);
            break;
        }
        case P_ED_SET_C3:
        {
            SmEdgeDefinition* d = r.ed(rd.idx());
            SmCurve* c = curve_or_null(rd.idx(), ok);
            if (skip || !d || !ok)
            {
                skip = true;
                break;
            }
            d->SetXSectCurve3d(c);
            break;
        }
        case P_ED_SET_UV:
        {
            SmEdgeDefinition* d = r.ed(rd.idx());
            int64_t w = rd.idx();
            SmCurve* c = curve_or_null(rd.idx(), ok);
            if (skip || !d || !ok)
            {
                skip = true;
                break;
            }
            d->SetXSectCurveUV((int)w, c);
            break;
        }
        case P_ED_SET_PE:
        {
            SmEdgeDefinition* d = r.ed(rd.idx());
            int64_t j = rd.idx();
            if (j != -1 && !r.pes.live(j))
                ok = false;
            if (skip || !d || !ok)
            {
                skip = true;
                break;
            }
            d->SetProtoEdge(r.pe(j));
            break;
        }
        case P_ED_SET_IVL:
        {
            SmEdgeDefinition* d = r.ed(rd.idx());
            double a = rd.num();
            double b = rd.num();
            if (skip || !d)
            {
                skip = true;
                break;
            }
            d->SetXSectInterval(SmExtent1d(a, b));
            break;
        }
        case P_ED_SET_SRC:
        {
            SmEdgeDefinition* d = r.ed(rd.idx());
            int64_t w = rd.idx();
            SmTopology* t = topo(rd.idx(), ok);
            if (skip || !d || !ok)
            {
                skip = true;
                break;
            }
            d->SetSrcTopo((int)w, SM_PC_UNKNOWN, t);
            break;
        }
        case P_ED_SET_FINAL:
        {
            int64_t id = rd.idx();
            SmEdgeDefinition* d = r.ed(id);
            int64_t w = rd.idx();
            int64_t j = rd.idx();
            if (j != -1 && !r.pool.is(j, K_EDGE))
                ok = false;
            if (skip || !d || !ok)
            {
                skip = true;
                break;
            }
            SmEdge* e = j == -1 ? NULL : (SmEdge*)r.pool.ptrs[(size_t)j];
            if (w == 1)
                d->*get(EdFinal1()) = e;
            else
                d->*get(EdFinal2()) = e;
            if (id == 0)
                r.first_ed_final_initialized |= w == 1 ? 1 : 2;
            break;
        }
        case P_MGR_REMOVE_VD:
        {
            SmVertexDefinition* d = r.vd(rd.idx());
            bool flag = rd.flag();
            if (skip || !d || d->GetProtoVertex() == NULL)
            {
                skip = true;
                break;
            }
            SmProtoVertex* owner = d->GetProtoVertex();
            // the owner is deleted when it loses its last definition
            SmTArray<SmVertexDefinition*> left = owner->GetVertexDefinitions();
            ULONG idx;
            if (left.FindElement(d, idx))
                left.RemoveAt(idx);
            bool dies = flag && left.GetSize() < 1;
            if (dies)
            {
                // its definitions (none) and its proto edges' back links
                for (ULONG k = 0; k < owner->GetProtoEdges().GetSize(); k++)
                    if (!r.pes.live_ptr(owner->GetProtoEdges()[k]))
                        skip = true;
            }
            if (skip)
                break;
            status = st(mgr->RemoveVertexDefinition(d, flag ? TRUE : FALSE));
            if (dies)
                r.pvs.kill(owner);
            break;
        }
        case P_MGR_REMOVE_PV:
        {
            SmProtoVertex* v = r.pv(rd.idx());
            if (skip || !v)
            {
                skip = true;
                break;
            }
            status = st(mgr->RemoveProtoVertex(v));
            break;
        }
        case P_MGR_REMOVE_PE:
        {
            SmProtoEdge* e = r.pe(rd.idx());
            if (skip || !e)
            {
                skip = true;
                break;
            }
            status = st(mgr->RemoveProtoEdge(e));
            break;
        }
        case P_MGR_SET_TOL:
        {
            double t = rd.num();
            if (skip)
                break;
            mgr->SetTolerance(SmZoneTol3d(t));
            break;
        }
        case P_MGR_SET_CAPTURE:
        {
            double t = rd.num();
            if (skip)
                break;
            mgr->SetCaptureDistance(t);
            break;
        }
        case P_PV_COMBINE:
        {
            SmProtoVertex* v = r.pv(rd.idx());
            SmProtoVertex* w = r.pv(rd.idx());
            if (skip || !v || !w)
            {
                skip = true;
                break;
            }
            v->CombineProtoVertex(w);
            break;
        }
        case P_ED_DESTRUCT:
        {
            SmEdgeDefinition* d = r.ed(rd.idx());
            if (skip || !d)
            {
                skip = true;
                break;
            }
            std::vector<const void*> curves;
            for (const SmCurve* c : {d->GetXSectCurve3d(), d->GetXSectUVCurve(1), d->GetXSectUVCurve(2)})
                if (c)
                    curves.push_back(c);
            if (!unique_live(curves, NULL, &r.pool))
            {
                skip = true;
                break;
            }
            put_deleted_curves(out, r.pool, curves);
            status = st(d->Destruct());
            for (const void* c : curves)
                r.pool.kill(c);
            break;
        }
        case P_DELETE_ED:
        {
            SmEdgeDefinition* d = r.ed(rd.idx());
            if (skip || !d)
            {
                skip = true;
                break;
            }
            std::vector<const void*> curves;
            ed_deletes(d, curves);
            if (!unique_live(curves, NULL, &r.pool))
            {
                skip = true;
                break;
            }
            put_deleted_curves(out, r.pool, curves);
            delete d;
            r.eds.kill(d);
            for (const void* c : curves)
                r.pool.kill(c);
            break;
        }
        case P_DELETE_VD:
        {
            SmVertexDefinition* d = r.vd(rd.idx());
            if (skip || !d)
            {
                skip = true;
                break;
            }
            delete d;
            r.vds.kill(d);
            break;
        }
        case P_DELETE_PE:
        {
            SmProtoEdge* e = r.pe(rd.idx());
            if (skip || !e)
            {
                skip = true;
                break;
            }
            std::vector<const void*> eds, curves;
            pe_delete_check(r, e, eds, curves);
            if (!unique_live(eds, &r.eds, NULL) || !unique_live(curves, NULL, &r.pool))
            {
                skip = true;
                break;
            }
            put_deleted_curves(out, r.pool, curves);
            delete e;
            r.pes.kill(e);
            for (const void* d : eds)
                r.eds.kill(d);
            for (const void* c : curves)
                r.pool.kill(c);
            break;
        }
        case P_DELETE_PV:
        {
            SmProtoVertex* v = r.pv(rd.idx());
            if (skip || !v)
            {
                skip = true;
                break;
            }
            std::vector<const void*> vds = as_vec(v->GetVertexDefinitions());
            if (!unique_live(vds, &r.vds, NULL))
            {
                skip = true;
                break;
            }
            delete v;
            r.pvs.kill(v);
            for (const void* d : vds)
                r.vds.kill(d);
            break;
        }
        case P_MGR_DESTROY:
        {
            if (skip)
                break;
            std::vector<const void*> pfs = as_vec(mgr->GetProtoFaces());
            std::vector<const void*> pes = as_vec(mgr->GetProtoEdges());
            std::vector<const void*> pvs = as_vec(mgr->GetProtoVertices());
            std::vector<const void*> eds, curves, vds;
            for (const void* e : pes)
                pe_delete_check(r, (SmProtoEdge*)e, eds, curves);
            for (const void* v : pvs)
                for (ULONG k = 0; k < ((SmProtoVertex*)v)->GetVertexDefinitions().GetSize(); k++)
                    vds.push_back(((SmProtoVertex*)v)->GetVertexDefinitions()[k]);
            if (!unique_live(pfs, &r.pfs, NULL) || !unique_live(pes, &r.pes, NULL) || !unique_live(pvs, &r.pvs, NULL) ||
                !unique_live(eds, &r.eds, NULL) || !unique_live(curves, NULL, &r.pool) || !unique_live(vds, &r.vds, NULL))
            {
                skip = true;
                break;
            }
            // the proto vertices' edge lists after the proto edges unlist themselves: a
            // deleted proto edge left there is used after free by ~SmProtoVertex
            std::map<const void*, std::vector<const void*>> lists;
            for (size_t k = 0; k < r.pvs.p.size(); k++)
                if (r.pvs.alive[k])
                    lists[r.pvs.p[k]] = as_vec(((SmProtoVertex*)r.pvs.p[k])->GetProtoEdges());
            auto unlist = [&](const void* v, const void* e) {
                std::vector<const void*>& l = lists[v];
                auto it = std::find(l.begin(), l.end(), e);
                if (it != l.end())
                    l.erase(it);
            };
            for (const void* e : pes)
            {
                SmProtoEdge* x = (SmProtoEdge*)e;
                if (x->GetStartProtoVertex())
                    unlist(x->GetStartProtoVertex(), e);
                if (x->GetEndProtoVertex() && x->GetEndProtoVertex() != x->GetStartProtoVertex())
                    unlist(x->GetEndProtoVertex(), e);
            }
            for (const void* v : pvs)
                for (const void* e : lists[v])
                    if (std::find(pes.begin(), pes.end(), e) != pes.end())
                        skip = true;
            if (skip)
                break;
            put_deleted_curves(out, r.pool, curves);
            delete mgr;
            r.mgr = NULL;
            for (const void* x : pfs)
                r.pfs.kill(x);
            for (const void* x : pes)
                r.pes.kill(x);
            for (const void* x : eds)
                r.eds.kill(x);
            for (const void* x : curves)
                r.pool.kill(x);
            for (const void* x : pvs)
                r.pvs.kill(x);
            for (const void* x : vds)
                r.vds.kill(x);
            break;
        }
        case P_VD_RESOLVE:
        {
            SmVertexDefinition* d = r.vd(rd.idx());
            if (skip || !d || d->GetProtoVertex() == NULL)
            {
                skip = true;
                break;
            }
            int s = 0;
            status = st(d->Resolve(s));
            out.push_back((double)s);
            put_vec(out, d->GetPosition());
            break;
        }
        case P_VD_SET_POINT_PARAMS:
        {
            SmVertexDefinition* d = r.vd(rd.idx());
            if (skip || !d)
            {
                skip = true;
                break;
            }
            status = st(d->SetPointParameters());
            break;
        }
        case P_VD_NUM_PES:
        {
            SmVertexDefinition* d = r.vd(rd.idx());
            if (skip || !d || d->GetProtoVertex() == NULL)
            {
                skip = true;
                break;
            }
            out.push_back((double)d->GetNumProtoEdges());
            break;
        }
        case P_ED_EVAL:
        case P_ED_EVAL_MID:
        {
            SmEdgeDefinition* d = r.ed(rd.idx());
            double t = op == P_ED_EVAL ? rd.num() : 0.0;
            int64_t mode = rd.idx();
            if (skip || !d || mode < 0 || mode > 3 || (mode != 0 && d->GetXSectCurve3d() == NULL))
            {
                skip = true;
                break;
            }
            SmPoint3d p;
            SmVector3d tan;
            SmStatus s = op == P_ED_EVAL ? d->Evaluate(t, mode & 1 ? &p : NULL, mode & 2 ? &tan : NULL)
                                         : d->EvaluateMid(mode & 1 ? &p : NULL, mode & 2 ? &tan : NULL);
            put_eval(out, s, mode, p, tan);
            break;
        }
        case P_ED_END_POS:
        {
            SmEdgeDefinition* d = r.ed(rd.idx());
            bool at_start = rd.flag();
            if (skip || !d || d->GetXSectCurve3d() == NULL)
            {
                skip = true;
                break;
            }
            SmPoint3d p;
            status = st(d->GetEndPosition(at_start ? TRUE : FALSE, p));
            put_vec(out, p);
            break;
        }
        case P_ED_END_POS_PV:
        {
            SmEdgeDefinition* d = r.ed(rd.idx());
            SmProtoVertex* v = pv_or_null(rd.idx(), ok);
            if (skip || !d || !ok || d->GetProtoEdge() == NULL)
            {
                skip = true;
                break;
            }
            SmProtoEdge* e = d->GetProtoEdge();
            if ((v == e->GetStartProtoVertex() || v == e->GetEndProtoVertex()) && d->GetXSectCurve3d() == NULL)
            {
                skip = true;
                break;
            }
            SmPoint3d p;
            SmStatus s = d->GetEndPosition(v, p);
            status = st(s);
            if (s == SM_SUCCESS)
                put_vec(out, p);
            break;
        }
        case P_ED_REVERSE:
        {
            SmEdgeDefinition* d = r.ed(rd.idx());
            if (skip || !d)
            {
                skip = true;
                break;
            }
            status = st(d->ReverseOrientation());
            break;
        }
        case P_ED_REPARAM:
        {
            SmEdgeDefinition* d = r.ed(rd.idx());
            double a = rd.num();
            double b = rd.num();
            bool rev = rd.flag();
            // a zero-length domain collapses the curves to a point: later point solves on
            // them never return (plan/upstream-issues.md HANG-12)
            if (skip || !d || d->GetXSectCurve3d() == NULL || a == b)
            {
                skip = true;
                break;
            }
            status = st(d->Reparameterize(SmExtent1d(a, b), rev ? TRUE : FALSE));
            break;
        }
        case P_ED_SET_IVL_MINMAX:
        {
            SmEdgeDefinition* d = r.ed(rd.idx());
            double a = rd.num();
            double b = rd.num();
            if (skip || !d)
            {
                skip = true;
                break;
            }
            d->SetXSectInterval(a, b);
            break;
        }
        case P_ED_IS_SAME:
        case P_PE_IS_SAME:
        {
            int64_t j = rd.idx();
            SmProtoVertex* v1 = pv_or_null(rd.idx(), ok);
            SmProtoVertex* v2 = pv_or_null(rd.idx(), ok);
            SmPointClassification pc1, pc2;
            ok = read_pc(rd, r.pool, pc1) && ok;
            ok = read_pc(rd, r.pool, pc2) && ok;
            SmCurve* c = curve_or_null(rd.idx(), ok);
            double a = rd.num();
            double b = rd.num();
            if (skip || !ok)
            {
                skip = true;
                break;
            }
            SmExtent1d dom(a, b);
            SmBoolean ident = FALSE;
            SmBoolean same = FALSE;
            // every definition compared: its proto edge, and both curves when they differ
            std::vector<SmEdgeDefinition*> defs;
            if (op == P_ED_IS_SAME)
            {
                if (!r.ed(j))
                {
                    skip = true;
                    break;
                }
                defs.push_back(r.ed(j));
            }
            else
            {
                SmProtoEdge* e = r.pe(j);
                if (!e)
                {
                    skip = true;
                    break;
                }
                bool between = (v1 == e->GetStartProtoVertex() && v2 == e->GetEndProtoVertex()) ||
                               (v1 == e->GetEndProtoVertex() && v2 == e->GetStartProtoVertex());
                if (between)
                    for (ULONG k = 0; k < e->GetEdgeDefinitions().GetSize(); k++)
                        defs.push_back(e->GetEdgeDefinitions()[k]);
            }
            for (SmEdgeDefinition* d : defs)
            {
                if (d->GetProtoEdge() == NULL)
                    skip = true;
                else
                {
                    SmProtoEdge* e = d->GetProtoEdge();
                    bool between = (v1 == e->GetStartProtoVertex() && v2 == e->GetEndProtoVertex()) ||
                                   (v1 == e->GetEndProtoVertex() && v2 == e->GetStartProtoVertex());
                    if (between && d->GetXSectCurve3d() != c && (d->GetXSectCurve3d() == NULL || c == NULL))
                        skip = true;
                }
            }
            if (skip)
                break;
            if (op == P_ED_IS_SAME)
                same = r.ed(j)->IsSameEdgeDefinition(v1, v2, pc1, pc2, c, dom, ident);
            else
                same = r.pe(j)->IsSameProtoEdge(v1, v2, pc1, pc2, c, dom, ident);
            out.push_back(flg(same));
            out.push_back(flg(ident));
            break;
        }
        case P_ED_IS_INTERIOR:
        {
            SmEdgeDefinition* d = r.ed(rd.idx());
            SmPoint3d p = rd.vec();
            double tol = rd.num();
            bool at_start = rd.flag();
            if (skip || !d || d->GetXSectCurve3d() == NULL)
            {
                skip = true;
                break;
            }
            double prm = 0.0;
            SmBoolean f = d->IsInteriorPoint(p, SmXSectTol3d(tol), at_start ? TRUE : FALSE, prm);
            out.push_back(flg(f));
            out.push_back(prm);
            break;
        }
        case P_ED_START_END_PV:
        {
            SmEdgeDefinition* d = r.ed(rd.idx());
            if (skip || !d || d->GetProtoEdge() == NULL)
            {
                skip = true;
                break;
            }
            out.push_back(r.pvs.id(d->GetStartProtoVertex()));
            out.push_back(r.pvs.id(d->GetEndProtoVertex()));
            break;
        }
        case P_PE_END_POS:
        case P_PE_END_POS_PV:
        {
            SmProtoEdge* e = r.pe(rd.idx());
            bool at_start = false;
            SmProtoVertex* v = NULL;
            if (op == P_PE_END_POS)
                at_start = rd.flag();
            else
                v = pv_or_null(rd.idx(), ok);
            if (skip || !e || !ok)
            {
                skip = true;
                break;
            }
            bool reached = true;
            if (op == P_PE_END_POS_PV)
            {
                if (v == e->GetStartProtoVertex())
                    at_start = true;
                else if (v == e->GetEndProtoVertex())
                    at_start = false;
                else
                    reached = false;
            }
            if (reached)
            {
                if (e->GetNumEdgeDefinitions() == 0)
                {
                    if ((at_start ? e->GetStartProtoVertex() : e->GetEndProtoVertex()) == NULL)
                        skip = true;
                }
                else
                    for (ULONG k = 0; k < e->GetNumEdgeDefinitions(); k++)
                        if (e->GetEdgeDefinitions()[k]->GetXSectCurve3d() == NULL)
                            skip = true;
            }
            if (skip)
                break;
            SmPoint3d p;
            SmStatus s = op == P_PE_END_POS ? e->GetEndPosition(at_start ? TRUE : FALSE, p) : e->GetEndPosition(v, p);
            status = st(s);
            if (s == SM_SUCCESS)
                put_vec(out, p);
            break;
        }
        case P_PE_EVAL:
        case P_PE_EVAL_MID:
        {
            SmProtoEdge* e = r.pe(rd.idx());
            double t = op == P_PE_EVAL ? rd.num() : 0.0;
            int64_t mode = rd.idx();
            if (skip || !e || mode < 0 || mode > 3)
            {
                skip = true;
                break;
            }
            if (mode != 0)
                for (ULONG k = 0; k < e->GetNumEdgeDefinitions(); k++)
                    if (e->GetEdgeDefinitions()[k]->GetXSectCurve3d() == NULL)
                        skip = true;
            if (skip)
                break;
            SmPoint3d p;
            SmVector3d tan;
            SmStatus s = op == P_PE_EVAL ? e->Evaluate(t, mode & 1 ? &p : NULL, mode & 2 ? &tan : NULL)
                                         : e->EvaluateMid(mode & 1 ? &p : NULL, mode & 2 ? &tan : NULL);
            put_eval(out, s, mode, p, tan);
            break;
        }
        case P_PE_GROUP:
        {
            SmProtoEdge* e = r.pe(rd.idx());
            if (skip || !e)
            {
                skip = true;
                break;
            }
            SmTArray<SmProtoEdge*> g;
            e->GetOverlappingPEGroup(g);
            put_ids(out, r.pes, as_vec(g));
            break;
        }
        case P_PV_FIND_MATCH:
        {
            SmProtoVertex* v = r.pv(rd.idx());
            int64_t a = rd.idx();
            int64_t b = rd.idx();
            if ((a != -1 && !r.pool.valid(a)) || (b != -1 && !r.pool.valid(b)))
                ok = false;
            if (skip || !v || !ok)
            {
                skip = true;
                break;
            }
            const SmTopology* t1 = a == -1 ? NULL : (SmTopology*)r.pool.ptrs[(size_t)a];
            const SmTopology* t2 = b == -1 ? NULL : (SmTopology*)r.pool.ptrs[(size_t)b];
            out.push_back(r.vds.id(v->FindMatchingVertexDefinition(t1, t2)));
            break;
        }
        case P_PV_CLOSEST:
        {
            SmProtoVertex* v = r.pv(rd.idx());
            SmPoint3d p = rd.vec();
            if (skip || !v)
            {
                skip = true;
                break;
            }
            out.push_back(r.vds.id(v->GetClosestVertexDefinition(p)));
            break;
        }
        case P_PV_FINAL_VTX:
        {
            SmProtoVertex* v = r.pv(rd.idx());
            int64_t w = rd.idx();
            SmEdgeDefinition* d = r.ed(rd.idx());
            bool at_start = rd.flag();
            if (skip || !v || !d)
            {
                skip = true;
                break;
            }
            if (v->GetNumVertexDefinitions() > 0)
            {
                if (d->GetXSectCurve3d() == NULL)
                    skip = true;
                for (ULONG k = 0; k < v->GetNumVertexDefinitions(); k++)
                {
                    SmVertex* fv = v->GetVertexDefinitions()[k]->GetFinalVertex((int)w);
                    if (fv && connect_crashes(r.pool, d->GetSrcTopo((int)w), fv))
                        skip = true;
                }
            }
            if (skip)
                break;
            out.push_back(r.pool.id(v->GetFinalVertex((int)w, d, at_start ? TRUE : FALSE)));
            break;
        }
        case P_MGR_FIND_PV:
        {
            SmPoint3d p = rd.vec();
            if (skip)
                break;
            double dist = 0.0;
            SmProtoVertex* v = mgr->FindProtoVertex(p, &dist);
            out.push_back(r.pvs.id(v));
            out.push_back(dist);
            break;
        }
        case P_MGR_FIND_ED_FROM_EDGE:
        {
            int64_t j = rd.idx();
            int64_t w = rd.idx();
            if (j != -1 && !r.pool.is(j, K_EDGE))
                ok = false;
            if (skip || !ok)
            {
                skip = true;
                break;
            }
            out.push_back(r.eds.id(mgr->FindEdgeDefinitionFromEdge(j == -1 ? NULL : (SmEdge*)r.pool.ptrs[(size_t)j], (int)w)));
            break;
        }
        case P_MGR_ALL_VDS:
        {
            if (skip)
                break;
            SmTArray<SmVertexDefinition*> a;
            mgr->GetAllVertexDefinitions(a);
            put_ids(out, r.vds, as_vec(a));
            break;
        }
        case P_MGR_ALL_EDS:
        {
            if (skip)
                break;
            SmTArray<SmEdgeDefinition*> a;
            mgr->GetAllEdgeDefinitions(a);
            put_ids(out, r.eds, as_vec(a));
            break;
        }
        case P_MGR_CHECK_OVERLAP:
        case P_ARE_OVERLAPPING:
        {
            SmCurve* cc_curve = curve_or_null(rd.idx(), ok);
            double cc_tol = rd.num();
            double a = rd.num();
            double b = rd.num();
            SmPointClassification m1, m2;
            ok = read_pc(rd, r.pool, m1) && ok;
            SmEdgeDefinition* d = NULL;
            if (op == P_MGR_CHECK_OVERLAP)
                ok = read_pc(rd, r.pool, m2) && ok;
            else
                d = r.ed(rd.idx());
            if (skip || !ok || cc_curve == NULL || (op == P_ARE_OVERLAPPING && (!d || d->GetXSectCurve3d() == NULL)))
            {
                skip = true;
                break;
            }
            SmCurveClassification cc;
            cc.SetCurve(cc_curve);
            SmTol::SetZoneTol3d(&cc, SmZoneTol3d(cc_tol));
            SmCurveInterval ci1(SmExtent1d(a, b), &ctx, &cc);
            ci1.m_vMid = m1;
            if (op == P_ARE_OVERLAPPING)
            {
                out.push_back(flg(sc_pt_p1::sm_AreOverlapping(ci1, *d)));
                break;
            }
            SmCurveInterval ci2(SmExtent1d(a, b), &ctx, NULL);
            ci2.m_vMid = m2;
            // the definitions compared need a curve
            if (m1.GetEdgeObject() || m2.GetEdgeObject())
            {
                SmTArray<SmEdgeDefinition*> all;
                mgr->GetAllEdgeDefinitions(all);
                for (ULONG k = 0; k < all.GetSize(); k++)
                    if (all[k]->GetSrcTopo(1) == SM_CAST_PTR(SmTopology, m1.GetObject()) &&
                        all[k]->GetSrcTopo(2) == SM_CAST_PTR(SmTopology, m2.GetObject()) &&
                        all[k]->GetXSectCurve3d() == NULL)
                        skip = true;
            }
            if (skip)
                break;
            SmTArray<SmProtoEdge*> res;
            status = st(mgr->CheckOverlappingProtoEdges(ci1, ci2, res));
            put_ids(out, r.pes, as_vec(res));
            break;
        }
        case P_MGR_CAN_COMBINE:
        {
            SmEdgeDefinition* d1 = r.ed(rd.idx());
            SmEdgeDefinition* d2 = r.ed(rd.idx());
            if (skip || !d1 || !d2)
            {
                skip = true;
                break;
            }
            if (!(d1->GetSrcTopo(1) == d2->GetSrcTopo(1) && d1->GetSrcTopo(2) == d2->GetSrcTopo(2)) &&
                (connect_crashes(r.pool, d1->GetSrcTopo(1), d2->GetSrcTopo(1)) ||
                 connect_crashes(r.pool, d1->GetSrcTopo(2), d2->GetSrcTopo(2))))
            {
                skip = true;
                break;
            }
            ULONG can = 99;
            status = st(mgr->CanCombineEdgeDefinitions(d1, d2, can));
            out.push_back((double)can);
            break;
        }
        case P_MGR_REMOVE_NOEDGE_PV:
        {
            SmProtoEdge* e = r.pe(rd.idx());
            SmProtoVertex* v = r.pv(rd.idx());
            if (skip || !e || !v)
            {
                skip = true;
                break;
            }
            if (e->GetStartProtoVertex() != v && e->GetEndProtoVertex() != v && e->GetNumEdgeDefinitions() > 0 &&
                e->GetEdgeDefinitions()[0]->GetXSectCurve3d() == NULL)
            {
                skip = true;
                break;
            }
            out.push_back(flg(mgr->RemoveCoincidentNoEdgeProtoVertex(e, v)));
            break;
        }
        case P_WHICH_PV:
        {
            SmProtoEdge* e = r.pe(rd.idx());
            double t = rd.num();
            SmProtoVertex* sv = pv_or_null(rd.idx(), ok);
            if (skip || !e || !ok)
            {
                skip = true;
                break;
            }
            if (sv == NULL)
            {
                for (ULONG k = 0; k < e->GetNumEdgeDefinitions(); k++)
                    if (e->GetEdgeDefinitions()[k]->GetXSectCurve3d() == NULL)
                        skip = true;
                // the split point is uninitialised when the evaluation fails; both ends
                // are dereferenced
                if (e->GetNumEdgeDefinitions() == 0 || e->GetStartProtoVertex() == NULL || e->GetEndProtoVertex() == NULL)
                    skip = true;
            }
            if (skip)
                break;
            out.push_back(r.pvs.id(sc_pt_p1::sm_WhichPVToCombine(e, t, sv)));
            break;
        }
        case P_FIND_POS:
        {
            SmPointClassification pc1, pc2;
            ok = read_pc(rd, r.pool, pc1) && ok;
            ok = read_pc(rd, r.pool, pc2) && ok;
            if (skip || !ok)
            {
                skip = true;
                break;
            }
            SmPoint3d p(0.0, 0.0, 0.0);
            double dev = -1.0;
            status = st(sc_pt_p1::sm_FindPositionFromPCs(pc1, pc2, p, dev));
            put_vec(out, p);
            out.push_back(dev);
            put_pc(out, pc1, r.pool);
            put_pc(out, pc2, r.pool);
            break;
        }
        case P_SAME_CURVE:
        {
            SmCurve* c1 = curve_or_null(rd.idx(), ok);
            double a1 = rd.num();
            double b1 = rd.num();
            SmCurve* c2 = curve_or_null(rd.idx(), ok);
            double a2 = rd.num();
            double b2 = rd.num();
            double tol = rd.num();
            if (skip || !ok || (c1 != c2 && (c1 == NULL || c2 == NULL)))
            {
                skip = true;
                break;
            }
            out.push_back(flg(sc_pt_p1::sm_AreSameCurve(c1, SmExtent1d(a1, b1), c2, SmExtent1d(a2, b2), tol)));
            break;
        }
        case P_IS_INTERIOR:
        {
            SmCurve* c = curve_or_null(rd.idx(), ok);
            double a = rd.num();
            double b = rd.num();
            double t = rd.num();
            double tol = rd.num();
            if (skip || !ok || c == NULL)
            {
                skip = true;
                break;
            }
            out.push_back(flg(sc_pt_p1::sm_IsInterior(c, SmExtent1d(a, b), t, SmZoneTol3d(tol))));
            break;
        }
        case P_ARE_CONNECTED:
        {
            int64_t a = rd.idx();
            int64_t b = rd.idx();
            if ((a != -1 && !r.pool.is_topo(a)) || (b != -1 && !r.pool.is_topo(b)))
                ok = false;
            const SmTopology* t1 = a == -1 ? NULL : (SmTopology*)r.pool.ptrs[(size_t)a];
            const SmTopology* t2 = b == -1 ? NULL : (SmTopology*)r.pool.ptrs[(size_t)b];
            if (skip || !ok || connect_crashes(r.pool, t1, t2))
            {
                skip = true;
                break;
            }
            out.push_back(flg(sc_pt_p1::sm_AreConnected(t1, t2)));
            break;
        }
        case P_TOPO_DIM:
        {
            SmTopology* t = topo(rd.idx(), ok);
            if (skip || !ok)
            {
                skip = true;
                break;
            }
            out.push_back((double)sc_pt_p1::sm_TopoDimension(t));
            break;
        }
        case P_CAN_IMPRINT:
        {
            int64_t c = rd.idx();
            if (skip || c < 100 || c > 110)
            {
                skip = true;
                break;
            }
            out.push_back(flg(sc_pt_p1::sm_CanImprint((SmPointClassificationType)c)));
            break;
        }
        case P_MGR_TYPE:
            if (skip)
                break;
            put_type(out, mgr);
            break;
        case P_MGR_GET:
            if (skip)
                break;
            out.push_back(mgr->GetWhichBrep(1) == b1 ? 1.0 : 0.0);
            out.push_back(mgr->GetWhichBrep(2) == b2 ? 1.0 : 0.0);
            out.push_back((double)mgr->GetTolerance());
            out.push_back(mgr->GetCaptureDistance());
            break;
        case P_FOC_PV:
        {
            SmPointClassification pc1, pc2;
            ok = read_pc(rd, r.pool, pc1) && ok;
            ok = read_pc(rd, r.pool, pc2) && ok;
            if (skip || !ok || (sc_pt_p1::sm_CanImprint(pc1.GetPointClass()) &&
                                sc_pt_p1::sm_CanImprint(pc2.GetPointClass()) && !position_ok(pc1, pc2)))
            {
                skip = true;
                break;
            }
            SmProtoVertex* nv = NULL;
            SmBoolean created = FALSE;
            status = st(mgr->FindOrCreateProtoVertex(pc1, pc2, nv, &created));
            register_new(r);
            out.push_back(r.pvs.id(nv));
            out.push_back(flg(created));
            break;
        }
        case P_FOC_PE:
        {
            SmCurve* cc_curve = curve_or_null(rd.idx(), ok);
            double cc_tol = rd.num();
            double a = rd.num();
            double b = rd.num();
            SmPointClassification s1, m1, e1, s2, m2, e2;
            ok = read_pc(rd, r.pool, s1) && ok;
            ok = read_pc(rd, r.pool, m1) && ok;
            ok = read_pc(rd, r.pool, e1) && ok;
            ok = read_pc(rd, r.pool, s2) && ok;
            ok = read_pc(rd, r.pool, m2) && ok;
            ok = read_pc(rd, r.pool, e2) && ok;
            // creating a proto edge (p2) needs a real B-rep: refused when every end and the middle
            // classify to topology in both B-reps
            if (skip || !ok || cc_curve == NULL || a > b || !pair_position_ok(s1, s2) || !pair_position_ok(m1, m2) ||
                !pair_position_ok(e1, e2) ||
                (sc_pt_p1::sm_CanImprint(s1.GetPointClass()) && sc_pt_p1::sm_CanImprint(s2.GetPointClass()) &&
                 sc_pt_p1::sm_CanImprint(e1.GetPointClass()) && sc_pt_p1::sm_CanImprint(e2.GetPointClass()) &&
                 sc_pt_p1::sm_CanImprint(m1.GetPointClass()) && sc_pt_p1::sm_CanImprint(m2.GetPointClass())))
            {
                skip = true;
                break;
            }
            SmCurveClassification cc;
            cc.SetCurve(cc_curve);
            SmTol::SetZoneTol3d(&cc, SmZoneTol3d(cc_tol));
            SmCurveInterval ci1(SmExtent1d(a, b), &ctx, &cc);
            ci1.m_vStart = s1;
            ci1.m_vMid = m1;
            ci1.m_vEnd = e1;
            SmCurveInterval ci2(SmExtent1d(a, b), &ctx, NULL);
            ci2.m_vStart = s2;
            ci2.m_vMid = m2;
            ci2.m_vEnd = e2;
            // the definitions compared need a curve (as op CHECK_OVERLAP)
            if (m1.GetEdgeObject() || m2.GetEdgeObject())
            {
                SmTArray<SmEdgeDefinition*> all;
                mgr->GetAllEdgeDefinitions(all);
                for (ULONG k = 0; k < all.GetSize(); k++)
                    if (all[k]->GetSrcTopo(1) == SM_CAST_PTR(SmTopology, m1.GetObject()) &&
                        all[k]->GetSrcTopo(2) == SM_CAST_PTR(SmTopology, m2.GetObject()) &&
                        all[k]->GetXSectCurve3d() == NULL)
                        skip = true;
            }
            if (skip)
                break;
            SmProtoEdge* npe = NULL;
            SmBoolean created = FALSE;
            status = st(mgr->FindOrCreateProtoEdge(ci1, ci2, npe, &created));
            register_new(r);
            out.push_back(r.pes.id(npe));
            out.push_back(flg(created));
            break;
        }
        case P_PE_SPLIT:
        {
            SmProtoEdge* e = r.pe(rd.idx());
            double t = rd.num();
            if (skip || !e)
            {
                skip = true;
                break;
            }
            if (e->GetXSectInterval().ContainsValue(t, -SM_EFF_ZERO) && (!pe_split_ok(r, e) || !ed_curves_unique(r)))
            {
                skip = true;
                break;
            }
            Reach before = reach(r);
            SmProtoVertex* npv = NULL;
            SmProtoEdge* npe = NULL;
            status = st(e->Split(t, npv, npe));
            register_new(r);
            out.push_back(r.pvs.id(npv));
            out.push_back(r.pes.id(npe));
            reconcile(r, before, false);
            break;
        }
        case P_ED_SPLIT:
        {
            SmEdgeDefinition* d = r.ed(rd.idx());
            double t = rd.num();
            SmProtoVertex* mid = pv_or_null(rd.idx(), ok);
            SmProtoEdge* top = r.pe(rd.idx());
            if (skip || !d || !ok || !mid || !top)
            {
                skip = true;
                break;
            }
            if (d->GetXSectInterval().ContainsValue(t, -SM_EFF_ZERO) &&
                (!d->GetXSectCurve3d() || !d->GetProtoEdge() || !r.pes.live_ptr(d->GetProtoEdge()) ||
                 !ed_curves_unique(r)))
            {
                skip = true;
                break;
            }
            Reach before = reach(r);
            SmVertexDefinition* nvd = NULL;
            SmEdgeDefinition* ned = NULL;
            status = st(d->Split(t, mid, top, nvd, ned));
            register_new(r);
            out.push_back(r.vds.id(nvd));
            out.push_back(r.eds.id(ned));
            reconcile(r, before, false);
            break;
        }
        case P_SPLIT_INT:
        {
            SmProtoEdge* e1 = r.pe(rd.idx());
            SmProtoEdge* e2 = r.pe(rd.idx());
            if (skip || !e1 || !e2)
            {
                skip = true;
                break;
            }
            bool shared = e1->GetStartProtoVertex() == e2->GetStartProtoVertex() ||
                          e1->GetStartProtoVertex() == e2->GetEndProtoVertex() ||
                          e1->GetEndProtoVertex() == e2->GetStartProtoVertex() ||
                          e1->GetEndProtoVertex() == e2->GetEndProtoVertex();
            if (!shared && e1->GetNumEdgeDefinitions() >= 1 && e2->GetNumEdgeDefinitions() >= 1)
            {
                SmTArray<SmProtoEdge*> ov;
                e1->GetOverlappingPEs(ov);
                ULONG idx;
                if (!ov.FindElement(e2, idx) && (!pe_split_ok(r, e1) || !pe_split_ok(r, e2) || !ed_curves_unique(r) ||
                                              !pe_listed_with_ends(r, e1) || !pe_listed_with_ends(r, e2)))
                {
                    skip = true;
                    break;
                }
            }
            Reach before = reach(r);
            status = st(mgr->SplitIntersectingProtoEdges(e1, e2));
            register_new(r);
            std::vector<double> dead = reconcile(r, before, false);
            out.push_back((double)dead.size());
            out.insert(out.end(), dead.begin(), dead.end());
            break;
        }
        case P_GLUE:
        {
            if (skip)
                break;
            // only the paths that do not glue run: a pair of final vertices must be one vertex or
            // two vertices with points farther apart than their combined zone tolerance (bare stand-ins have no usable point,
            // and a glue needs a B-rep)
            for (ULONG i = 0; i < mgr->GetProtoVertices().GetSize() && !skip; i++)
            {
                SmProtoVertex* v = mgr->GetProtoVertices()[i];
                SmTArray<SmVertexDefinition*> vds = v->GetVertexDefinitions();
                for (ULONG j = 1; j < vds.GetSize(); j++)
                {
                    SmVertex* a1 = vds[0]->GetFinalVertex(1);
                    SmVertex* b1 = vds[j]->GetFinalVertex(1);
                    SmVertex* a2 = vds[0]->GetFinalVertex(2);
                    SmVertex* b2 = vds[j]->GetFinalVertex(2);
                    if (!(a1 && b1 && a2 && b2))
                        continue;
                    SmVertex* pairs[2][2] = {{a1, b1}, {a2, b2}};
                    for (int s = 0; s < 2; s++)
                    {
                        ScVertex* x = static_cast<ScVertex*>(pairs[s][0]);
                        ScVertex* y = static_cast<ScVertex*>(pairs[s][1]);
                        if (!x->pointed || !y->pointed)
                            skip = true;
                        else if (x != y && !(x->GetPoint().DistanceBetween(y->GetPoint()) > SmTol::GetXSectTol3d(x, y)))
                            skip = true;
                    }
                }
            }
            if (skip)
                break;
            Reach before = reach(r);
            status = st(mgr->GlueCoincidentBrepVertices());
            std::vector<double> dead = reconcile(r, before, false);
            out.push_back((double)dead.size());
            out.insert(out.end(), dead.begin(), dead.end());
            break;
        }
        case P_RESOLVE:
        {
            if (skip)
                break;
            // a proto edge listed twice in the manager crashes upstream (AddProtoEdge does not
            // de-duplicate, RemoveProtoEdge removes the first copy only: UB-132)
            for (ULONG a = 0; a < r.mgr->GetProtoEdges().GetSize() && !skip; a++)
                for (ULONG b = a + 1; b < r.mgr->GetProtoEdges().GetSize(); b++)
                    if (r.mgr->GetProtoEdges()[a] == r.mgr->GetProtoEdges()[b])
                        skip = true;
            if (skip)
                break;
            // only face / face groups run (an edge needs a real B-rep); every proto edge of a
            // group needs its end vertices and every source topology
            std::set<SmProtoEdge*> grp;
            for (size_t i = 0; i < r.pes.p.size(); i++)
            {
                SmProtoEdge* e = r.pe((int64_t)i);
                if (!e || e->GetOverlappingPEs().GetSize() == 0)
                    continue;
                grp.insert(e);
                for (ULONG k = 0; k < e->GetOverlappingPEs().GetSize(); k++)
                    grp.insert(e->GetOverlappingPEs()[k]);
            }
            for (SmProtoEdge* e : grp)
            {
                if (!r.pes.live_ptr(e) || !e->GetStartProtoVertex() || !e->GetEndProtoVertex() ||
                    !r.pvs.live_ptr(e->GetStartProtoVertex()) || !r.pvs.live_ptr(e->GetEndProtoVertex()) ||
                    !pe_listed_with_ends(r, e))
                {
                    skip = true;
                    continue;
                }
                // upstream crashes (reads a deleted edge) unless every overlap relation is symmetric,
                // no overlap list repeats an edge and each edge is in its own overlap group
                {
                    SmTArray<SmProtoEdge*> ov = e->GetOverlappingPEs();
                    std::vector<SmProtoEdge*> group;
                    for (ULONG a = 0; a < ov.GetSize(); a++)
                    {
                        SmProtoEdge* o = ov[a];
                        for (ULONG b = a + 1; b < ov.GetSize(); b++)
                            if (ov[b] == o)
                                skip = true;
                        if (!o || !r.pes.live_ptr(o))
                        {
                            skip = true;
                            continue;
                        }
                        bool back = false;
                        SmTArray<SmProtoEdge*> ob = o->GetOverlappingPEs();
                        for (ULONG c = 0; c < ob.GetSize(); c++)
                            if (ob[c] == e)
                                back = true;
                        if (!back)
                            skip = true;
                        group.push_back(o);
                    }
                    // GetOverlappingPEGroup: chase the overlaps until the group is complete
                    for (size_t g = 0; g < group.size() && !skip; g++)
                    {
                        SmTArray<SmProtoEdge*> og = group[g]->GetOverlappingPEs();
                        for (ULONG c = 0; c < og.GetSize(); c++)
                        {
                            if (!og[c] || !r.pes.live_ptr(og[c]))
                            {
                                skip = true;
                                break;
                            }
                            if (std::find(group.begin(), group.end(), og[c]) == group.end())
                                group.push_back(og[c]);
                        }
                    }
                    if (!skip && std::find(group.begin(), group.end(), e) == group.end())
                        skip = true;
                    if (skip)
                        continue;
                }
                // a vertex left without edges but with definitions is deleted (not tracked)
                SmProtoVertex* ends[2] = {e->GetStartProtoVertex(), e->GetEndProtoVertex()};
                for (SmProtoVertex* v : ends)
                {
                    bool left = false;
                    for (ULONG k = 0; k < v->GetProtoEdges().GetSize(); k++)
                        if (!grp.count(v->GetProtoEdges()[k]))
                            left = true;
                    if (!left && v->GetVertexDefinitions().GetSize() > 0)
                        skip = true;
                }
            }
            for (size_t i = 0; i < r.eds.p.size() && !skip; i++)
            {
                SmEdgeDefinition* d = r.ed((int64_t)i);
                if (!d)
                    continue;
                for (int w = 1; w <= 2; w++)
                {
                    SmTopology* t = d->GetSrcTopo(w);
                    if (!t || r.pool.kind_of(t) == K_EDGE)
                        skip = true;
                }
            }
            for (size_t i = 0; i < r.pfs.p.size() && !skip; i++)
            {
                SmProtoFace* f = r.pf((int64_t)i);
                if (!f)
                    continue;
                for (ULONG k = 0; k < f->GetCoincProtoEdges()->GetSize(); k++)
                    if (!r.pes.live_ptr((*f->GetCoincProtoEdges())[k]))
                        skip = true;
            }
            if (skip || !ed_curves_unique(r))
            {
                skip = true;
                break;
            }
            Reach before = reach(r);
            status = st(mgr->ResolveOverlappingProtoEdges());
            std::vector<double> dead = reconcile(r, before, true);
            out.push_back((double)dead.size());
            out.insert(out.end(), dead.begin(), dead.end());
            break;
        }
        default:
            if (op < P2_FIRST || op > P2_LAST || run_p2_op(r, rd, op, out, status, skip) != SC_OK)
                return SC_ERR_ARG;
            break;
        }
        if (rd.bad)
            return SC_ERR_ARG;
        if (!skip && !r.invariant())
            skip = true;
        if (skip)
        {
            o.push_back((double)ST_SKIP);
            return SC_OK;
        }
        o.push_back(status);
        o.push_back((double)out.size());
        o.insert(o.end(), out.begin(), out.end());
    }
    put_proto_state(o, r);
    return SC_OK;
}

// =====================================================================================
// TRANSFORM cases: SmApiTransform, SmApiScale, SmApiScaleByPt, SmApiTranslate, SmApiRotate
// =====================================================================================

enum TObj : int64_t
{
    T_CURVE = 0,
    T_PLANE = 1,
    T_BSURF = 2,
    T_BREP = 3,
    T_POLY = 4,
    T_OTHER = 5,
    T_NULL = 6,
    T_COMPOSITE = 7,
    T_CRV_ON_SURF = 8,
    T_POLY_PTS = 9,
    T_PRIMITIVE_BOX = 10,
    T_PRIMITIVE_SPHERE = 11,
    T_CALLER_FACE = 12,
    T_COPIED_BOX = 13,
    T_COPIED_SPHERE = 14,
};

// SmCrvOnSurf's UV-line constructor is protected.
class ScCos : public SmCrvOnSurf
{
public:
    ScCos(SmPoint2d& a, SmPoint2d& b, SmSurface& s, const SmContext* ctx) : SmCrvOnSurf(a, b, s, NULL, 0, ctx) {}
};

int32_t apply_transform_op(Rd& rd, SmObject* obj, SmApiStatus& s)
{
    int64_t op = rd.idx();
    switch (op)
    {
    case 0:
    {
        SmPoint3d org = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        if (rd.bad)
            return SC_ERR_ARG;
        SmAxis2Placement pl(org, x, y);
        s = SmApiTransform(obj, pl);
        break;
    }
    case 1:
    {
        SmVector3d sc = rd.vec();
        if (rd.bad)
            return SC_ERR_ARG;
        s = SmApiScale(obj, sc);
        break;
    }
    case 2:
    {
        SmVector3d ref = rd.vec();
        SmVector3d sc = rd.vec();
        if (rd.bad)
            return SC_ERR_ARG;
        s = SmApiScaleByPt(obj, ref, sc);
        break;
    }
    case 3:
    {
        SmVector3d v = rd.vec();
        if (rd.bad)
            return SC_ERR_ARG;
        s = SmApiTranslate(obj, v);
        break;
    }
    case 4:
    {
        SmVector3d ref = rd.vec();
        SmVector3d ax = rd.vec();
        double deg = rd.num();
        if (rd.bad)
            return SC_ERR_ARG;
        s = SmApiRotate(obj, ref, ax, deg);
        break;
    }
    default: return SC_ERR_ARG;
    }
    return SC_OK;
}

void append_constructed_transform_record(std::vector<double>& o, SmBrep* body)
{
        SmTArray<SmVertex*> vertices; body->GetVertices(vertices);
        SmTArray<SmEdge*> edges; body->GetEdges(edges);
        SmTArray<SmFace*> faces; body->GetFaces(faces);
        o.insert(o.end(), {(double)vertices.GetSize(), (double)edges.GetSize(), (double)faces.GetSize(), (body->*get(BrepModelSize()))(FALSE)});
        primitive_record::append(o,body);
        for (ULONG i=0;i<faces.GetSize();i++) o.push_back((double)faces[i]->GetRectangularTrim());
}

int32_t run_transform(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    int64_t kind = rd.idx();
    SmObject* obj = NULL;
    SmBrep* source = NULL;
    std::vector<double> source_before;
    switch (kind)
    {
    case T_CURVE:
    {
        bool owned = rd.flag();
        SmCurve* c = read_curve(rd, ctx);
        if (owned)
            c->SetOwner(anchor());
        obj = c;
        break;
    }
    case T_PLANE:
    case T_BSURF:
    {
        bool owned = rd.flag();
        SmSurface* s = read_surface(rd, ctx, kind == T_PLANE ? 1 : 2);
        if (owned)
            s->SetOwner(anchor());
        obj = s;
        break;
    }
    case T_PRIMITIVE_BOX:
    case T_PRIMITIVE_SPHERE:
    case T_COPIED_BOX:
    case T_COPIED_SPHERE:
    {
        SmPoint3d origin = rd.vec(); SmBrep* body = NULL; SmApiStatus created;
        if (kind == T_PRIMITIVE_BOX || kind == T_COPIED_BOX) {
            double x=rd.num(), y=rd.num(), z=rd.num();
            created = SmApiCreateBox(origin, x, y, z, body);
        } else { double radius=rd.num(); created=SmApiCreateSphere(origin, radius, body); }
        if (created != SM_SUCCESS || !body) { o.push_back((double)created); delete body; return SC_OK; }
        if (kind == T_COPIED_BOX || kind == T_COPIED_SPHERE) {
            source = body; primitive_record::append(source_before, source);
            SmBrep* copy = NULL; auto copied = SmApiBrepCopy(source, copy);
            if (copied != SM_SUCCESS || !copy) { o.push_back((double)copied); delete copy; delete source; return SC_OK; }
            body = copy;
        }
        obj = body; break;
    }
    case T_CALLER_FACE:
    {
        SmPoint3d origin=rd.vec(); double width=rd.num(), height=rd.num();
        SmBrep* body=new(ctx) SmBrep(); SmFace* face=NULL;
        SmPlane* surface=new(ctx) SmPlane(origin,SmVector3d(0,0,1),&ctx);
        SmStatus created=body->CreateFaceFromSurface(surface,SmExtent2d(0,0,width,height),face);
        if(created!=SM_SUCCESS || !face) { o.push_back((double)created); delete body; return SC_OK; }
        obj=body; break;
    }
    case T_BREP: obj = new (ctx) SmBrep(); break;
    case T_POLY: obj = new (ctx) SmPolyBrep(SmZoneTol3d(1.0e-5)); break;
    case T_POLY_PTS:
    {
        SmPolyBrep* b = new (ctx) SmPolyBrep(SmZoneTol3d(1.0e-5));
        int64_t n = rd.idx();
        for (int64_t i = 0; i < n && i < 4 && !rd.bad; i++)
        {
            SmVector3d p = rd.vec();
            double t = rd.num();
            SmPolyVertex* v = new (b) SmPolyVertex(p, t);
            (b->*get(PolyVertexHead()))->PostInsert(v);
        }
        obj = b;
        break;
    }
    case T_OTHER: obj = new (ctx) ScOwning(); break;
    case T_NULL: obj = NULL; break;
    case T_COMPOSITE:
    {
        bool owned = rd.flag();
        int64_t n = rd.idx();
        SmTArray<SmCurve*> curves;
        for (int64_t i = 0; i < n && i < 4 && !rd.bad; i++)
            curves.Add(read_curve(rd, ctx));
        SmCompositeCurve* c = new (ctx) SmCompositeCurve(3, curves, FALSE, NULL, NULL);
        if (owned)
            c->SetOwner(anchor());
        obj = c;
        break;
    }
    case T_CRV_ON_SURF:
    {
        bool owned = rd.flag();
        SmSurface* s = read_surface(rd, ctx, 2);
        double a0 = rd.num();
        double a1 = rd.num();
        double b0 = rd.num();
        double b1 = rd.num();
        SmPoint2d a(a0, a1);
        SmPoint2d b(b0, b1);
        SmCurve* c = new (ctx) ScCos(a, b, *s, &ctx);
        if (owned)
            c->SetOwner(anchor());
        obj = c;
        break;
    }
    default: return SC_ERR_ARG;
    }
    SmApiStatus s = 0;
    int32_t applied = apply_transform_op(rd, obj, s);
    if (applied != SC_OK) return applied;
    o.push_back((double)s);
    if (kind == T_PRIMITIVE_BOX || kind == T_PRIMITIVE_SPHERE || kind == T_CALLER_FACE || kind == T_COPIED_BOX || kind == T_COPIED_SPHERE) {
        append_constructed_transform_record(o, (SmBrep*)obj);
    }
    if (source) {
        o.insert(o.end(), source_before.begin(), source_before.end());
        primitive_record::append(o, source);
        delete (SmBrep*)obj; obj = NULL;
        delete source;
    }
    if (kind == T_CURVE)
        put_curve(o, (SmCurve*)obj);
    else if (kind == T_PLANE || kind == T_BSURF)
        put_surface(o, (SmSurface*)obj);
    else if (kind == T_POLY || kind == T_POLY_PTS)
    {
        // the poly B-rep's tolerance, then each vertex's point and tolerance
        const SmPolyBrep* b = (const SmPolyBrep*)obj;
        o.push_back(b->GetTolerance());
        SmTArray<SmPolyVertex*> vs;
        b->GetPolyVertices(vs);
        o.push_back((double)vs.GetSize());
        for (ULONG i = 0; i < vs.GetSize(); i++)
        {
            SmPoint3d p = vs[i]->GetPoint();
            o.push_back(p.x);
            o.push_back(p.y);
            o.push_back(p.z);
            o.push_back(vs[i]->GetTolerance());
        }
    }
    return SC_OK;
}

int32_t run_transform_sequence(Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    const int64_t kind = rd.idx();
    SmBrep* body = NULL;
    SmApiStatus created = SM_SUCCESS;
    if (kind == T_PRIMITIVE_BOX) {
        SmPoint3d origin = rd.vec();
        double x=rd.num(), y=rd.num(), z=rd.num();
        created = SmApiCreateBox(origin, x, y, z, body);
    } else if (kind == T_PRIMITIVE_SPHERE) {
        SmPoint3d origin = rd.vec(); double radius=rd.num();
        created = SmApiCreateSphere(origin, radius, body);
    } else if (kind == T_CALLER_FACE) {
        SmPoint3d origin=rd.vec(); double width=rd.num(), height=rd.num();
        body=new(ctx) SmBrep(); SmFace* face=NULL;
        SmPlane* surface=new(ctx) SmPlane(origin,SmVector3d(0,0,1),&ctx);
        created=body->CreateFaceFromSurface(surface,SmExtent2d(0,0,width,height),face);
        if (!face && created == SM_SUCCESS) created=SM_ERR_NULL_POINTER;
    } else { return SC_ERR_ARG; }
    if (created != SM_SUCCESS || !body) { delete body; return SC_ERR_ARG; }
    const int64_t count=rd.idx();
    if (rd.bad || count<2 || count>8) { delete body; return SC_ERR_ARG; }
    o.push_back((double)count);
    for (int64_t i=0; i<count; i++) {
        SmApiStatus status=0;
        const int32_t applied=apply_transform_op(rd,body,status);
        if (applied!=SC_OK) { delete body; return applied; }
        std::vector<double> record;
        append_constructed_transform_record(record,body);
        o.push_back((double)status);
        o.push_back((double)record.size());
        o.insert(o.end(),record.begin(),record.end());
    }
    delete body;
    return SC_OK;
}

int32_t run(Rd& rd, std::vector<double>& o)
{
    int64_t kind = rd.idx();
    int32_t s;
    switch (kind)
    {
    case 0: s = run_list(rd, o); break;
    case 1: s = run_proto(rd, o); break;
    case 2: s = run_transform(rd, o); break;
    case 3: s = run_transform_sequence(rd, o); break;
    default: return SC_ERR_ARG;
    }
    if (s != SC_OK)
        return s;
    // a script ended by a refused op need not be consumed
    if (rd.bad)
        return SC_ERR_ARG;
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_topology_proto_topology_p1_run(const double* in, int64_t n_in, double* out, int64_t cap,
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
