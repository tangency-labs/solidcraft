// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `boolean.topology_intersector_p1`: the SmTopologyIntersector members
// owned by the unit (source/SMLib/src/SmTopologyIntersector.cpp,
// source/SMLib/inc/SmTopologyIntersector.h) on real SmBrep objects, plus the file-static
// helpers of SmTopologyIntersector.cpp.
//
// Static functions of SmTopologyIntersector.cpp cannot be linked against, so sm_CanImprint,
// sm_FindFirstLowestDimensionSol, sm_IntersectShellVertices, sm_IntersectWireLaminaEdges,
// sm_SetEdgeClass, sm_CheckInteriorEdge, sReasonType, sm_TestAndSplitTwoEdges and
// sm_TestAndImprintFace are VERBATIM copies of upstream (commit c9c5979, lines 1272-1572,
// 2676-2829 and 6564-6861) inside namespace sc_ti_p1 (SM_DEBUG_CODE and CBI_CACHE are not
// defined in the reference build).
//
// One entry point, sc_boolean_topology_intersector_p1_eval(op, path, in, n_in, out, cap,
// out_len). Layouts are documented in
// crates/solidcore-ffi/src/units/boolean_topology_intersector_p1.rs, which validates every
// input. The B-rep construction, traversal and export are those of sc_topology_fixture.h
// (two B-rep blocks; `path` holds the two file paths separated by a newline). The private
// m_pProtoTopoMgr of the intersector is reached through a pointer-to-member (the manager is
// made by IntersectInsertRelate, unit boolean.topology_intersector_p2, not ported yet).
#include "sc_topology_fixture.h"

#include <SmBrepCache.h>
#include <SmCacheMgr.h>
#include <SmCurveClass.h>
#include <SmPointClass.h>
#include <SmProtoTopology.h>
#include <SmTopologyIntersector.h>
#include <SmTopologySolver.h>
#include <SmTree.h>
#include <SmTrimSrfCache.h>

#include <algorithm>
#include <string>

namespace sc_ti_p1
{
/*******************************************************************//**
PURPOSE: Static helper: Return whether a PointClassificationType can be imprinted.

USAGE NOTES---- 
***********************************************************************/
static SmBoolean sm_CanImprint( SmPointClassificationType eType )
{
  return(   eType == SM_PC_REGION
         || eType == SM_PC_FACE
         || eType == SM_PC_EDGE
         || eType == SM_PC_VERTEX) ;

} // end sm_CanImprint

/*******************************************************************//**
PURPOSE: Static helper: Find the solution in an array with the lowest-dimensional object.

USAGE NOTES---- 
   Finds the first instance of such a solution.
   If no usable result, return value == rSols.GetSize().
***********************************************************************/
static ULONG sm_FindFirstLowestDimensionSol( SmSolutionArray &crSolutions )
{
  ULONG lWhichSol = 0; // Index of solution to use.

  ULONG ii, lNumSols = crSolutions.GetSize();
  if ( lNumSols < 1 )
    { return lNumSols; }  // Indicates no usable results.

  if (lNumSols > 1)
  {
      lWhichSol = lNumSols; // Would indicate nothing found.

      // Choose the lowest-dimension result: Vertex, then Edge, then Face.
      ULONG lWhichV, lWhichE, lWhichF;
      lWhichV = lWhichE = lWhichF = lNumSols;

      // Use the first one found (of each type).
      for ( ii=0; ii<lNumSols; ii++ )
      {
          const SmSolution &rSol = crSolutions[ii];
          ULONG iType = rSol.m_apObjects[0]->GetType();

          if (iType == SmVertex_TYPE)
          {
              lWhichV = ii;
              break;  // Done: use first.
          }
          else if (iType == SmEdge_TYPE)
          {
              if (lWhichE == lNumSols) { lWhichE = ii; }
          }
          else if (iType == SmFace_TYPE)
          {
              if (lWhichF == lNumSols) { lWhichF = ii; }
          }
      } // end iter every solution

      // lWhichSol defaults to lNumSols if there are none of these,
      // that should not happen.
      SM_ASSERT(   lWhichV < lNumSols
                || lWhichE < lNumSols
                || lWhichF < lNumSols);

      if      (lWhichV < lNumSols) { lWhichSol = lWhichV; }
      else if (lWhichE < lNumSols) { lWhichSol = lWhichE; }
      else if (lWhichF < lNumSols) { lWhichSol = lWhichF; }
      else { SE(SM_ERR); }

  } // end more than 1 solution check

  return lWhichSol;

} // end static sm_FindFirstLowestDimensionSol


/*******************************************************************//**
PURPOSE: Static helper: Intersect Shell Vertices in pBrep1 with pBrep2.

USAGE NOTES---- 
   Creates a ProtoVertex in the ProtoTopology Manager.
***********************************************************************/
static SmStatus sm_IntersectShellVertices( SmBrep *pBrep1, // in: Brep whose vertices to check.
                                           SmBrep *pBrep2, // in: Brep to check Brep1 vertices against.
                                           SmBoolean bSwapOrder, // in: which order in pProtoTopoMgr.
                                           SmProtoTopologyManager *pProtoTopoMgr ) // in/out: 
{
  SmTArray<SmVertex*> sVertexList;

#ifdef CBI_CACHE   //cbi: try caching later; 1st pass just do all topo.
  // Get or create caches and bounding boxes for each of the Breps
  SmExtent3d sBBox1, sBBox2;
  SmBrepCache *pBrepCache1 = (SmBrepCache*)SmCacheMgr::GetOrCreateObjectCache(SM_OC_BREP,pBrep1 );
  SmBrepCache *pBrepCache2 = (SmBrepCache*)SmCacheMgr::GetOrCreateObjectCache(SM_OC_BREP,pBrep2);
  NER(pBrepCache1);
  NER(pBrepCache2);
  //pBrepCache1->GetBBox( sBBox1 );
  //pBrepCache2->GetBBox( sBBox2 );

  // Get the vertices of Brep that are in the common box, and store them in a local array.
  pBrepCache1->GetVertexTree()->GetObjectListInBox( sCommonBox, sObjs1 );

  ULONG ii, lNumObjs = sObjs1.GetSize();
  for ( ii=0; ii<lNumObjs; ii++ )
    { sVertexList.Add( (SmVertex*)( sObjs1[ii]->m_pObject )); }
#else  // CBI_CACHE
  pBrep1->GetVertices( sVertexList );
  ULONG ii, lNumObjs = sVertexList.GetSize();
#endif  // CBI_CACHE

  SmSolutionArray sSolutions;
  SmProtoVertex *pNewPV = NULL;

  for ( ii=0; ii<lNumObjs; ii++ )
  {
      SmVertex *pVertex = sVertexList[ii];

      // Only shell vertices:
      if ( ! pVertex->IsShellVertex() ) { continue; }

      // Tol: BrepPointSolve() adds this to the Topology's tolerance.
      SmZoneTol3d sVtxZoneTol = SmTol::GetZoneTol3d( pVertex );

      // intersect pVertex with Brep
      double dBestAnswer=0.0;
      {
          SmTemporaryChangeValue<SmBoolean> sChange( pBrep2->m_bEditingEnabled, FALSE );

          SER( SmTopologySolver::BrepPointSolve( pBrep2,
                                                 pVertex->GetPoint(),
                                                 SM_SO_INTERSECT,
                                                 SM_SR_ALL,
                                                 sVtxZoneTol,
                                                 dBestAnswer, NULL,
                                                 sSolutions ));

      } // end scope for pBrep2->m_bEditingEnabled.

      // no work - no intersections
      ULONG lNumSols = sSolutions.GetSize();
      if ( lNumSols < 1 ) { continue; }

      // Find the lowest-dimensional result: Vertex first, then Edge, then Face.
      ULONG lWhichSol = sm_FindFirstLowestDimensionSol( sSolutions );

      if ( lWhichSol >= lNumSols )
        { continue; }  // No usable sols.

      // Create a PointClassification for each Brep, and a ProtoVertex from those.
      // The PointClass objects get copied into the new PV.
      SmPointClassification sPC1( sVtxZoneTol, pBrep1->GetContext() );
      sPC1.SetClassObject( SM_PC_VERTEX, pVertex );

      SmSolution & rSol = sSolutions[ lWhichSol ];
      SmPointClassification sPC2( sVtxZoneTol, pBrep2->GetContext() );
      SER( sPC2.LoadFromIntersection(SmTol::GetZoneTol3d(pVertex),rSol.m_apObjects[0], rSol.m_vStart, 0, SM_BIG_DOUBLE ));

      if ( bSwapOrder )
        { pProtoTopoMgr->FindOrCreateProtoVertex( sPC2, sPC1, pNewPV ); }
      else
        { pProtoTopoMgr->FindOrCreateProtoVertex( sPC1, sPC2, pNewPV ); }

  } // end for each m_Brep Vertex

  return SM_SUCCESS;

} // end sm_IntersectShellVertices


/*******************************************************************//**
PURPOSE: Static helper: Intersect Wire and optionally Lamina Edges in pBrep1 with pBrep2.

USAGE NOTES---- 
   Creates a ProtoEdge in the ProtoTopology Manager.
***********************************************************************/
static SmStatus sm_IntersectWireLaminaEdges( SmBrep *pBrep1,       // in: Brep whose vertices to check.
                                             SmBrep *pBrep2,       // in: Brep to check Brep1 vertices against.
                                             SmBoolean bDoLamina,  // in: do lamina as well as wire edges?
                                             SmBoolean bSwapOrder, // in: which order in pProtoTopoMgr.
                                             SmProtoTopologyManager *pProtoTopoMgr )
{

  // Locals
  SmSolutionArray sSolutions;
  SmProtoEdge *pNewPE = NULL;
  ULONG ii, jj;
  SmStatus eStat;

#ifdef CBI_CACHE   //cbi: try caching later; 1st pass just do all topo.
    // Get the curves of Brep that are in the common box, and store them in a local array.
    pBrepCache1 = (SmBrepCache*)SmCacheMgr::GetOrCreateObjectCache(SM_OC_BREP,pBrep1);
    NER(pBrepCache1);
    pBrepCache1->GetCurveTree()->GetObjectListInBox( sCommonBox, sObjs1 );

    ULONG lNumObjs = sObjs1.GetSize();
    SmTArray<SmCurve*> sCurveList;
    for ( ii=0; ii<lNumObjs; ii++ )
      {
        SmCurve* pCrv = (SmCurve*)( sObjs1[ii]->m_pObject );
        sCurveList.Add( pCrv );
      }
#else  // CBI_CACHE
  SmTArray< SmEdge* > sEdgeList;
  pBrep1->GetEdges( sEdgeList );
  ULONG lNumObjs = sEdgeList.GetSize();
#endif  // CBI_CACHE


  for ( ii=0; ii<lNumObjs; ii++ )
  {
      SmEdge *pEdge = sEdgeList[ii];
      if( pEdge == NULL )
        { continue; }

      SmBoolean bProcessThis = pEdge->IsWire();
      bProcessThis |= ( bDoLamina && pEdge->IsLamina() );
      if ( ! bProcessThis )
        { continue; }

      //cbi from IIREdgeBrep():
      // select SolverType - handle wires differently unless bForceFaceIntersections == TRUE
      SmSolverOperationType eSolverOp =   (   TRUE    //cbi  bForceFaceIntersections
                                           || !pEdge->IsWire())
                                        ? SM_SO_INTERSECT
                                        : SM_SO_INTERSECT_WIREFRAME ;
      {
          // temporarily disable EditingEnabled bit
          SmTemporaryChangeValue<SmBoolean> sChange( pBrep2->m_bEditingEnabled, FALSE );

          // Intersect Edge->Curve with Brep
          eStat = SmTopologySolver::BrepCurveSolve( pBrep2,
                                       *pEdge->GetCurve(), pEdge->GetInterval(),
                                        eSolverOp, SM_SR_ALL, pEdge->GetTolerance(),
                                        SM_BIG_DOUBLE, NULL, sSolutions );
      }

      if ( eStat != SM_SUCCESS || sSolutions.GetSize() < 1 )
        { continue; }  // No ints for this Edge.

      // Construct first CurveClassification for all of pEdge.
      SmCurveClassification sCrvClass1( pEdge->GetCurve(), pEdge->GetInterval(),
                                        NULL, pEdge->GetTolerance() );

//cbi: there's a bunch more stuff in sm_SetEdgeClass( sCrvClass1, pEdge ).
//cbi  I don't think we need it.
//cbi  They also reverse it if primary EU is OPP.

      SmVertex *pVertex = pEdge->GetStartVertex();
      sCrvClass1[0].m_vStart.SetClassObject( SM_PC_VERTEX, pVertex );
      sCrvClass1[0].m_vMid  .SetClassObject( SM_PC_EDGE  , pEdge   );
      pVertex = pEdge->GetOtherVertex( pVertex );
      sCrvClass1[0].m_vEnd  .SetClassObject( SM_PC_VERTEX, pVertex );

      // Construct second CurveClassification for pBrep2 intersections.
      SmCurveClassification sCrvClass2( pEdge->GetCurve(), pEdge->GetInterval(),
                                        NULL, pEdge->GetTolerance() );

      // Add curve/Brep intersections to CrvClass2.
      if ( sCrvClass2.InsertTopologyIntersections( sSolutions, 0 ) != SM_SUCCESS )
        { continue; }

//cbi: might have to Fixup(), etc., as in SmTopologyIntersector::MergeCurveClasses() (line 2030)
      // Syncronize the two curve classifications.
      if ( sCrvClass1.Homogenize( sCrvClass2 ) != SM_SUCCESS )
        { return SM_ERR; }

      // Create ProtoEdges from each segment.
      for ( jj=0; jj<sCrvClass2.GetSize(); jj++ )
      {
          if ( bSwapOrder )
            { pProtoTopoMgr->FindOrCreateProtoEdge( sCrvClass2[jj], sCrvClass1[jj], pNewPE ); }
          else
            { pProtoTopoMgr->FindOrCreateProtoEdge( sCrvClass1[jj], sCrvClass2[jj], pNewPE ); }
      }

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe=FALSE;
static ULONG lCount      = 1 ; lCount++ ;
static ULONG lDebugCount = 0 ;
      if (bDebugMe || lCount == lDebugCount)
      {
          sSolutions.Dump() ;
          sCrvClass1.Dump();
          sCrvClass2.Dump();

          smgfx_Erase() ;
          smgfx_SetLook(1,2, 0,0,1); pBrep1->Draw(TRUE) ; sm_GraphicsLoop() ;
          smgfx_SetLook(3,4, 1,1,0); pEdge->Draw() ; sm_GraphicsLoop() ;
          smgfx_SetLook(5,6, 1,0,0); sSolutions.Draw() ; sm_GraphicsLoop() ;
          //  smgfx_SetLook(5,6, 1,0,0); sCrvClass1->Draw() ; sm_GraphicsLoop() ;
          //  smgfx_SetLook(5,6, 1,0,0); sCrvClass2->Draw() ; sm_GraphicsLoop() ;
          sm_GraphicsLoop();
      }
#endif // SM_DEBUG_CODE


  } // end for each pBrep1 edge

  return SM_SUCCESS;

}  // end sm_IntersectWireLaminaEdges

/*******************************************************************//**
PURPOSE:   Set a CurveClassification to be as if the curve is the
              same as the edge.

NOTES:
  When rCurveClass->Interval == pEdge->Interval
    rCurveClass->m_vStart->m_ePointClass = SM_PC_VERTEX
    rCurveClass->m_vMid->m_ePointClass   = SM_PC_EDGE
    rCurveClass->m_vEnd->m_ePointClass   = SM_PC_VERTEX

  When CurveClassification interval is trimmed then appropriate
    CurveClassification EndPointClassifications are changed from
    VERTEX to EDGE as
          m_vStart->m_ePointClass = SM_PC_EDGE
          m_vEnd->m_ePointClass   = SM_PC_EDGE
***********************************************************************/

static SmStatus sm_SetEdgeClass
  (SmCurveClassification & rCurveClass,  // in : target classification
   SmEdge * pEdge)                       // in : target edge
{
  // edge locals
  SmVertex  *pV1     = pEdge->GetVertex();
  SmVertex  *pV2     = pEdge->GetOtherVertex(pV1);
  SmEdgeuse *pPrimEU = pEdge->GetPrimaryEdgeuse();

  if (pPrimEU->GetOrientation() == SM_OT_OPPOSITE)
    {
      SmVertex *pVTmp = pV1;
      pV1 = pV2;
      pV2 = pVTmp;
    }

  // CurveClassification locals
  SmCurveInterval & rCIvl = rCurveClass[0];

  // set 1st CurveClassification interval to classify to edge and its end vertices
  rCIvl.m_vStart.SetClassObject (SM_PC_VERTEX, pV1);
  rCIvl.m_vStart.SetPreSnapParam(rCurveClass.GetInterval().GetMin());
#ifdef SM_USE_NEWTOL      
  SM_NEWTOL_LINE rCIvl.m_vStart.SetGap3d    (0.0);
#else // SM_USE_OLDTOL 
  SM_OLDTOL_LINE rCIvl.m_vStart.SetDeviation   (0.0);
#endif // SM_USE_OLDTOL

  rCIvl.m_vMid.SetClassObject   (SM_PC_EDGE, pEdge);
  rCIvl.m_vMid.SetPreSnapParam  (SM_BIG_DOUBLE);
#ifdef SM_USE_NEWTOL      
  SM_NEWTOL_LINE rCIvl.m_vMid.SetGap3d     (0.0);
#else // SM_USE_OLDTOL 
  SM_OLDTOL_LINE rCIvl.m_vMid.SetDeviation     (0.0);
#endif // SM_USE_OLDTOL

  rCIvl.m_vEnd.SetClassObject   (SM_PC_VERTEX, pV2);
  rCIvl.m_vEnd.SetPreSnapParam  (rCurveClass.GetInterval().GetMax());
#ifdef SM_USE_NEWTOL      
  SM_NEWTOL_LINE rCIvl.m_vEnd.SetGap3d     (0.0);
#else // SM_USE_OLDTOL 
  SM_OLDTOL_LINE rCIvl.m_vEnd.SetDeviation     (0.0);
#endif // SM_USE_OLDTOL

  // get curve endPoints
  SmPoint3d sStart, sEnd;
  SER(rCurveClass.GetCurve()->EvaluatePoint(rCurveClass.GetInterval().GetMin(),sStart));
  SER(rCurveClass.GetCurve()->EvaluatePoint(rCurveClass.GetInterval().GetMax(),sEnd));

  // Note that the curve on the curve class may be trimmed - take
  // care of that here by replacing the vertex by edge classifications
  // for the ends of the interval.

  // define a tolerance
#ifdef SM_USE_NEWTOL      
  SM_NEWTOL_LINE double dStartTol = pV1->GetTolerance() + rCurveClass.GetMaxGap3d();
#else // SM_USE_OLDTOL 
  SM_OLDTOL_LINE double dStartTol = pV1->GetTolerance() + SmTol::GetSrcZoneTol3d(&rCurveClass);
  SM_OLDTOL_LINE // double dStartTol = pV1->GetTolerance() + rCurveClass.GetZoneTol3d();
#endif // SM_USE_OLDTOL
  SmBoolean bSuccess;
  double dParameter, dDistToCurve;
  SmCurve *pEdgeCurve = pEdge->GetCurve();

  // when curveInterval start is trimmed
  if (sStart.DistanceBetween(pV1->GetPoint()) > dStartTol)
    {
      // change CurveClassification m_vStart from Vertex to Point on Edge classification
      SER(pEdgeCurve->DropPoint(pEdge->GetInterval(), // in : target curve allowed domain
                                sStart,               // in : Point to drop to curve
                                NULL,                 // in : For Snapping, NULL=NoSnap, Vec pointing to CrvInside from desired EndPt for snapping.
                                                      //      when DropPt is within dDistTol in cpVecToCrvInside dir of EndPt, Snap to Ivl EndPt.
                                                      //      Vec handles ambiguities and makes sure that curves are not just touching at the ends.
                                dStartTol,            // in : Used for SnapDist, and SM_SO_INTERSECT sXSectTol3d distance.
                                                      //      sXSectTol3d = dDistTol ;      SM_SO_INTERSECT 
                                                      //      sXSectTol3d = SM_BIG_DOUBLE ; SM_SO_MINIMIZE, SM_SO_NORMALIZE
                                                      //      sSnapTol3d  = dDistTol ;      SM_SO_MINIMIZE, SM_SO_NORMALIZE, SM_SO_INTERSECT    
                                NULL,                 // in : If given, call cheap LocalPointSolve() else call expensive GlobalPointSolve()
                                bSuccess,             // out: TRUE = found a drop point
                                dParameter,           // out: found drop curve param
                                dDistToCurve)) ;      // out: found drop distance
                                                      // in : SM_SO_MINIMIZE = allow nonNormal drops near endPoints
                                                      //      SM_SO_INTERSECT= like Minimize but point must be within dDistTol
                                                      //      SM_SO_NORMALIZE= exclude nonNormal drops near endPoints
                                                      //      default:[SM_SO_MINIMIZE] to preserve original behavior
      rCIvl.m_vStart.SetClassObject (SM_PC_EDGE, pEdge);
      rCIvl.m_vStart.SetPreSnapParam(dParameter);
#ifdef SM_USE_NEWTOL      
      SM_NEWTOL_LINE rCIvl.m_vStart.SetGap3d    (dDistToCurve);
#else // SM_USE_OLDTOL 
      SM_OLDTOL_LINE rCIvl.m_vStart.SetDeviation   (dDistToCurve);
#endif // SM_USE_OLDTOL
      rCIvl.m_vStart.SetTParam      (dParameter);

    } // end is curveInterval start trimmed check

  // when curveInterval end is trimmed
#ifdef SM_USE_NEWTOL      
  SM_NEWTOL_LINE double dEndTol = pV2->GetTolerance() + rCurveClass.GetMaxGap3d();
#else // SM_USE_OLDTOL 
  SM_OLDTOL_LINE double dEndTol = pV2->GetTolerance() + SmTol::GetSrcZoneTol3d(&rCurveClass);
  SM_OLDTOL_LINE // double dEndTol = pV2->GetTolerance() + rCurveClass.GetZoneTol3d();
#endif // SM_USE_OLDTOL
  if (sEnd.DistanceBetween(pV2->GetPoint()) > dEndTol)
    {
      // change CurveClassification m_vEnd from Vertex to Point on Edge classification
      SER(pEdgeCurve->DropPoint(pEdge->GetInterval(),  // in : target curve allowed domain
                                sEnd,                  // in : Point to drop to curve
                                NULL,                  // in : For Snapping, NULL=NoSnap, Vec pointing to CrvInside from desired EndPt for snapping.
                                                       //      when DropPt is within dDistTol in cpVecToCrvInside dir of EndPt, Snap to Ivl EndPt.
                                                       //      Vec handles ambiguities and makes sure that curves are not just touching at the ends.
                                dEndTol,               // in : Used for SnapDist, and SM_SO_INTERSECT sXSectTol3d distance.
                                                       //      sXSectTol3d = dDistTol ;      SM_SO_INTERSECT 
                                                       //      sXSectTol3d = SM_BIG_DOUBLE ; SM_SO_MINIMIZE, SM_SO_NORMALIZE
                                                       //      sSnapTol3d  = dDistTol ;      SM_SO_MINIMIZE, SM_SO_NORMALIZE, SM_SO_INTERSECT    
                                NULL,                  // in : If given, call cheap LocalPointSolve() else call expensive GlobalPointSolve()
                                bSuccess,              // out: TRUE = found a drop point
                                dParameter,            // out: found drop curve param
                                dDistToCurve)) ;       // out: found drop distance
                                                       // in : SM_SO_MINIMIZE = allow nonNormal drops near endPoints
                                                       //      SM_SO_INTERSECT= like Minimize but point must be within dDistTol
                                                       //      SM_SO_NORMALIZE= exclude nonNormal drops near endPoints
                                                       //      default:[SM_SO_MINIMIZE] to preserve original behavior
      rCIvl.m_vEnd.SetClassObject (SM_PC_EDGE, pEdge);
      rCIvl.m_vEnd.SetPreSnapParam(dParameter);
#ifdef SM_USE_NEWTOL      
      SM_NEWTOL_LINE rCIvl.m_vEnd.SetGap3d    (dDistToCurve);
#else // SM_USE_OLDTOL 
      SM_OLDTOL_LINE rCIvl.m_vEnd.SetDeviation   (dDistToCurve);
#endif // SM_USE_OLDTOL
      rCIvl.m_vEnd.SetTParam      (dParameter);

    } // end is curveInterval end trimmed check

  return SM_SUCCESS;

} // end sm_SetEdgeClass

/*******************************************************************//**
PURPOSE: static helper:
   Test to see whether a parameter value is in the interior of an Edge, to tolerance.
   I.e., should we split the edge at this parameter value.

NOTES: If trouble (return SM_ERR), returns bInterior == FALSE.
***********************************************************************/
static SmStatus sm_CheckInteriorEdge(
        const SmEdge     * pEdge,
              double       dT,
              double       dTol,
              SmBoolean  & bInterior
    )
{
  bInterior = FALSE;

  // Check common simple case first.
  // Note, negative tol excludes the ends.
  SmExtent1d sEdgeDomain = pEdge->GetInterval();
  if ( ! sEdgeDomain.ContainsValue( dT, -SM_EFF_ZERO ) )
    { return SM_SUCCESS; }

  SmPoint3d sCrvPt[2];
  SmCurve *pCurve = pEdge->GetCurve();
  SER( pCurve->Evaluate( dT, 1, FALSE, sCrvPt ) );
  double dParamLen = sCrvPt[1].Length();
  if ( dParamLen > 0.1 )
  {
      double dParamTol = dTol / dParamLen;

      // Because the param length can vary, use this test
      // only as a rough guide: if it's anywhere close,
      // go on to the 3d test.  So use a bigger tolerance.
      dParamTol *= 20.0;
      if ( sEdgeDomain.ContainsValue( dT, -dParamTol ) )
      {
          bInterior = TRUE;
          return SM_SUCCESS;
      }
  }
  // Work in 3d.
  // This is also more reliable in case of vertex/edge tolerance.

  SmVertex *pV = pEdge->GetVertex();
  double dDist = sCrvPt[0].DistanceBetween( pV->GetPoint() );
  if ( dDist <= dTol )
    { return SM_SUCCESS; }

  pV = pEdge->GetOtherVertex( pV );
  dDist = sCrvPt[0].DistanceBetween( pV->GetPoint() );
  if ( dDist <= dTol )
    { return SM_SUCCESS; }

  bInterior = TRUE;
  return SM_SUCCESS;

} // end static sm_CheckInteriorEdge


// A local enum, for sm_TestAndSplitTwoEdges() and sm_TestAndImprintFace().
enum sReasonType
{
  NO_REASON,
  FAR_FROM_GEOM,
  CLOSE_TO_BOUNDARY
};


/*******************************************************************//**
PURPOSE: static helper:
   Test to see if two edges intersect in their interiors,
   and if so, create a new vertex and split the edges with it.

NOTES: 
   Doesn't check bounding boxes; the caller should do that first.
***********************************************************************/

static SmStatus
sm_TestAndSplitTwoEdges(SmEdge      * pEdge1,       // in
                            SmEdge      * pEdge2,       // in
                            double        dTol,         // in
                            double      & rdDistToEdge, // out: always set, whether split or not
                            double      & rdMinUnstitched,  // out: always set, whether split or not
                            SmTArray< SmVertex *> & rNewVerts,  // out
                            SmTArray< SmEdge   *> & rNewEdges1, // out
                            SmTArray< SmEdge   *> & rNewEdges2) // out
{
    // Init outputs
    rdDistToEdge = 0.0;
    rdMinUnstitched  = SM_BIG_DOUBLE;

    rNewVerts .ReSet();
    rNewEdges1.ReSet();
    rNewEdges2.ReSet();

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
    if (bDebugMe) {
        sm_GraphicsLoop();
        smgfx_Erase();
        smgfx_SetLook( 2,4, 1,0,0); pEdge1->Draw(); sm_GraphicsLoop();
        smgfx_SetLook( 2,3, 0,1,1); pEdge2->Draw(); sm_GraphicsLoop();
        sm_GraphicsLoop();
    }
#endif // SM_DEBUG_CODE


    SmCurve *pCurve1 = pEdge1->GetCurve(); NER(pCurve1);
    SmCurve *pCurve2 = pEdge2->GetCurve(); NER(pCurve2);

    SmSolutionArray sSolutions;
    SmBoolean bSkipGlobalCoincidenceCheck = TRUE; //cbi ?

    SmExtent1d sE1Domain = pEdge1->GetInterval();
    SmExtent1d sE2Domain = pEdge2->GetInterval();

    // Use a larger tol, to catch close ones for rdMinUnstitched.
    SER(pCurve1->GlobalCurveIntersect( sE1Domain, *pCurve2, sE2Domain,
        10*dTol, sSolutions, bSkipGlobalCoincidenceCheck ));

    ULONG lNumSols = sSolutions.GetSize();
    if ( lNumSols < 1 )
    {
        return SM_SUCCESS;
    }

    ULONG ii;
    for ( ii = 0; ii < lNumSols; ii++ )
    {
        SmSolution & rSol = sSolutions[ii];

        // We should be able to skip range solutions.
        if ( rSol.m_eSolutionType != SM_ST_SINGLE_VALUE )
          { continue; }

        if ( rSol.m_vStart.m_dSolutionValue > dTol )
        {
            rdMinUnstitched = smos_Min( rdMinUnstitched, rSol.m_vStart.m_dSolutionValue );
            continue;
        }

        // Check for internal to both edges.
        double dT1 = rSol.m_vStart[0];
        SmBoolean bInterior = FALSE;
        SmStatus eStat = sm_CheckInteriorEdge( pEdge1, dT1, dTol, bInterior );
        if ( bInterior == FALSE ) { continue; }

        double dT2 = rSol.m_vStart[1];
        eStat = sm_CheckInteriorEdge( pEdge2, dT2, dTol, bInterior );
        if ( bInterior == FALSE ) { continue; }

        // The intersection is interior to both Edges.
        // Split them both, and then glue the vertices.
        SmBrep *pBrep = pEdge1->GetBrep();  NER( pBrep );

        SmEdge *pE1 = NULL, *pE2 = NULL;
        SmVertex *pNewV1 = NULL, *pNewV2 = NULL;
        eStat = pBrep->MakeVertexSplitEdge( pEdge1, dT1, pE1, pE2, pNewV1 );
        if ( eStat != SM_SUCCESS)
        {
            SM_ASSERT( FALSE );
            // Maybe because of tolerance.  Check that.
            SmPoint3d sPt1, sPt2;
            pCurve1->EvaluatePoint( dT1, sPt1 );
            pCurve2->EvaluatePoint( dT2, sPt2 );
            if ( sPt1.CloserThan( rdMinUnstitched, sPt2 ) )
              { rdMinUnstitched = sPt1.DistanceBetween( sPt2 ); }

            continue;
        }
        NER(pNewV1);
        SmEdge *pNewEdge = pE2;
        if (pE2 == pEdge1) {
            pNewEdge = pE1;
        }
        rNewEdges1.Add( pNewEdge );

        pBrep = pEdge2->GetBrep();  // Presumably the same...
        eStat = pBrep->MakeVertexSplitEdge( pEdge2, dT2, pE1, pE2, pNewV2 );
        if ( eStat != SM_SUCCESS)
        {
            SM_ASSERT( FALSE );
            continue;
        }
        NER(pNewV2); // pNewV2 will be deleted, so don't add to output list.
        pNewEdge = pE2;
        if (pE2 == pEdge2) {
            pNewEdge = pE1;
        }
        rNewEdges2.Add( pNewEdge );

        rdDistToEdge = pNewV1->GetPoint().DistanceBetween( pNewV2->GetPoint() );

        // And glue the vertices.

        // First set the position of the surviving vertex to be
        // the midpoint of the two ... this should help to keep tolerances
        // from growing excessively.
        SmPoint3d sMidPt = 0.5 * ( pNewV1->GetPoint() + pNewV2->GetPoint() );
        pNewV1->SetPoint( sMidPt );

        pBrep->GlueVertices( pNewV1, pNewV2 );

        pNewV2 = NULL;
        rNewVerts.Add( pNewV1 ); // This one will survive.
    }

    return SM_SUCCESS;

} // end static sm_TestAndSplitTwoEdges


/*******************************************************************//**
PURPOSE: static helper:
   Test to see if a point is on a face in its interior,
   and if so, create a new shell vertex and imprint it into the face.

NOTES: 
***********************************************************************/
static SmStatus sm_TestAndImprintFace
 (SmFace          * pFace,
  const SmPoint3d & crPoint,       // in
  SmZoneTol3d       sSrcZoneTol3d, // in : Obj ZoneTol3d assoc with crPoint
  sReasonType     & reReason,      // out
  double          & rdDistToFace,  // out: always set, whether split or not
  SmVertex       *& rpNewVertex)   // out
{
  // Init outputs
  rdDistToFace = SM_BIG_DOUBLE;
  rpNewVertex  = NULL;
  reReason     = NO_REASON;

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
  if (bDebugMe) {
      sm_GraphicsLoop();
      smgfx_Erase();
      smgfx_SetLook( 2,4, 1,0,0); crPoint.Draw(); sm_GraphicsLoop();
      smgfx_SetLook( 1,3, 0,1,1); pFace ->Draw(); sm_GraphicsLoop();
      sm_GraphicsLoop();
  }
#endif // SM_DEBUG_CODE

  // SmZoneTol3d sFaceZoneTol3d = SmTol::GetZoneTol3d(pFace) ;
  SmSurface * pSurface       = pFace->GetSurface(); NER(pSurface);
  SmBoolean   bSuccess;
  SmBoolean   bIsMulti;
  SmPoint2d   sUV;
  SER(pSurface->DropPoint(crPoint, pFace->GetUVDomain(), NULL,
                          bSuccess, sUV, rdDistToFace, bIsMulti ));

  // Check far from surface.
  SmXSectTol3d sXSectTol3d = SmTol::GetXSectTol3d(sSrcZoneTol3d, SmTol::GetZoneTol3d(pFace)) ;
  if ( !bSuccess || rdDistToFace > sXSectTol3d )
  {
      reReason = FAR_FROM_GEOM;
      return SM_SUCCESS;
  }

  // Check interior.
  // Note, it will presumably not be close to a vertex or an edge of the face.
  SmBoolean bIntWithBoundary = FALSE;
  SmPointClassification sPointClass(sSrcZoneTol3d, pFace->GetContext()) ;  

  pFace->PointClassify( sUV, sSrcZoneTol3d, bIntWithBoundary, TRUE, sPointClass );

  if ( sPointClass.GetPointClass() == SM_PC_UNKNOWN )
  {
      reReason = FAR_FROM_GEOM;
      return SM_SUCCESS;
  }
  if (   sPointClass.GetPointClass() == SM_PC_EDGE
      || sPointClass.GetPointClass() == SM_PC_VERTEX )
  {
      reReason = CLOSE_TO_BOUNDARY;
      return SM_SUCCESS;
  }
  if ( sPointClass.GetPointClass() != SM_PC_FACE )
  {
      SM_ASSERT( FALSE );
      reReason = CLOSE_TO_BOUNDARY; // most probable reason; don't really care too much.
      return SM_SUCCESS;
  }


  // Point is on the surface, within the face.
  SmVertex *pNewV = NULL;
  SmLoop   *pNewLoop = NULL;
  if (pFace->GetBrep()->MakeVertexLoop(pFace,crPoint,pNewLoop,pNewV) != SM_SUCCESS) {
      reReason = CLOSE_TO_BOUNDARY; // most probable reason; don't really care too much.
      return SM_SUCCESS;
  }
  NER(pNewV);
  rpNewVertex = pNewV;

  return SM_SUCCESS;

} // end static sm_TestAndImprintFace

} // namespace sc_ti_p1

namespace
{
using namespace sc_ti_p1;

// ---- private member access (pointer-to-member through explicit instantiation) -------------

template <typename Tag, typename Tag::type M>
struct TiRob
{
    friend typename Tag::type ti_get(Tag) { return M; }
};

struct TiMgr
{
    typedef SmProtoTopologyManager* SmTopologyIntersector::*type;
    friend type ti_get(TiMgr);
};
template struct TiRob<TiMgr, &SmTopologyIntersector::m_pProtoTopoMgr>;

enum TiOp : int32_t
{
    TI_EXPORT = 0,
    TI_RUN = 1,
};

// Script op codes (the `op` field of each op in a TI_RUN script).
enum TiScriptOp : int64_t
{
    S_RELATE = 1,
    S_OTHER_MATE = 2,
    S_THIS_MATE = 3,
    S_BREP_MATE = 4,
    S_IS_MATED = 5,
    S_MAPPED_BREP = 6,
    S_COMMON_EDGES = 7,
    S_COMMON_VERTICES = 8,
    S_SET_SUBSET = 9,
    S_FIXUP_END = 11,
    S_FIXUP_MID = 12,
    S_GETTERS = 13,
    S_SET_TOLS = 14,
    S_SET_BREPS = 15,
    S_REGISTER_SSI = 16,
    S_CHECK_SSI = 17,
    S_CLEAR_SSI = 18,
    S_CAN_IMPRINT = 19,
    S_LOWEST = 20,
    S_SET_EDGE_CLASS = 21,
    S_CHECK_INTERIOR = 22,
    S_SPLIT_TWO = 23,
    S_IMPRINT_FACE = 24,
    S_VERTEX_FACE = 25,
    S_EDGE_FACE = 26,
    S_FACE_FACE = 27,
    S_FACE_FACE_COIN = 28,
    S_RESOLVE = 29,
    S_IMPRINT = 30,
    S_SHELL_VERTS = 31,
    S_WIRE_LAMINA = 32,
    S_MGR_NEW = 33,
};

// Handle kinds (`kind` of a `brep kind index` triple).
enum HKind : int64_t
{
    H_VERTEX = 0,
    H_EDGE = 1,
    H_FACE = 2,
    H_EDGE_CURVE = 3,
    H_FACE_SURFACE = 4,
    H_CURVE = 5, // the shim's free curve store
    H_BREP = 6,
    H_NULL = 8,
};

struct Two
{
    SmBrep* brep[2] = {nullptr, nullptr};
    Graph g[2];
    std::vector<SmCurve*> curves; // free curves (registered intersection curves and their copies)
    // the edge curves and face surfaces at build time, by address (never dereferenced:
    // a mutating op may have freed them)
    std::map<const void*, int64_t> curve_of_edge[2];
    std::map<const void*, int64_t> surface_of_face[2];

    void index_geometry()
    {
        for (int b = 0; b < 2; b++)
        {
            curve_of_edge[b].clear();
            surface_of_face[b].clear();
            if (brep[b] == nullptr)
                continue;
            for (size_t k = 0; k < g[b].edges.size(); k++)
                if (g[b].edges[k]->GetCurve() != nullptr)
                    curve_of_edge[b].emplace((const void*)g[b].edges[k]->GetCurve(), (int64_t)k);
            for (size_t k = 0; k < g[b].faces.size(); k++)
                if (g[b].faces[k]->GetSurface() != nullptr)
                    surface_of_face[b].emplace((const void*)g[b].faces[k]->GetSurface(), (int64_t)k);
        }
    }

    SmObject* resolve(int64_t b, int64_t kind, int64_t i) const
    {
        if (kind == H_NULL || i < 0)
            return nullptr;
        if (kind == H_BREP)
            return (b == 0 || b == 1) ? (SmObject*)brep[b] : nullptr;
        if (kind == H_CURVE)
            return (i < (int64_t)curves.size()) ? (SmObject*)curves[(size_t)i] : nullptr;
        if (b != 0 && b != 1)
            return nullptr;
        const Graph& gr = g[b];
        auto at = [i](auto& v) -> SmTopology* { return (i >= 0 && i < (int64_t)v.size()) ? (SmTopology*)v[(size_t)i] : nullptr; };
        switch (kind)
        {
        case H_VERTEX: return at(gr.vertices);
        case H_EDGE: return at(gr.edges);
        case H_FACE: return at(gr.faces);
        case H_EDGE_CURVE:
        {
            SmEdge* e = (SmEdge*)at(gr.edges);
            return e ? (SmObject*)e->GetCurve() : nullptr;
        }
        case H_FACE_SURFACE:
        {
            SmFace* f = (SmFace*)at(gr.faces);
            return f ? (SmObject*)f->GetSurface() : nullptr;
        }
        default: return nullptr;
        }
    }

    // Emits the `brep kind index` triple of an object (`-1 8 -1` null, `-1 -3 -1` unknown).
    void put(std::vector<double>& o, const SmObject* p) const
    {
        if (p == nullptr)
        {
            o.push_back(-1.0);
            o.push_back((double)H_NULL);
            o.push_back(-1.0);
            return;
        }
        for (int b = 0; b < 2; b++)
        {
            if (brep[b] == nullptr)
                continue;
            if ((const SmObject*)brep[b] == p)
            {
                o.push_back((double)b);
                o.push_back((double)H_BREP);
                o.push_back((double)b);
                return;
            }
            const Graph& gr = g[b];
            int64_t i;
            if ((i = gr.find(9, (const SmVertex*)p)) >= 0)
            {
                o.push_back((double)b);
                o.push_back((double)H_VERTEX);
                o.push_back((double)i);
                return;
            }
            if ((i = gr.find(7, (const SmEdge*)p)) >= 0)
            {
                o.push_back((double)b);
                o.push_back((double)H_EDGE);
                o.push_back((double)i);
                return;
            }
            if ((i = gr.find(3, (const SmFace*)p)) >= 0)
            {
                o.push_back((double)b);
                o.push_back((double)H_FACE);
                o.push_back((double)i);
                return;
            }
            auto ce = curve_of_edge[b].find((const void*)p);
            if (ce != curve_of_edge[b].end())
            {
                o.push_back((double)b);
                o.push_back((double)H_EDGE_CURVE);
                o.push_back((double)ce->second);
                return;
            }
            auto fs = surface_of_face[b].find((const void*)p);
            if (fs != surface_of_face[b].end())
            {
                o.push_back((double)b);
                o.push_back((double)H_FACE_SURFACE);
                o.push_back((double)fs->second);
                return;
            }
        }
        for (size_t k = 0; k < curves.size(); k++)
            if ((const SmObject*)curves[k] == p)
            {
                o.push_back(2.0);
                o.push_back((double)H_CURVE);
                o.push_back((double)k);
                return;
            }
        o.push_back(-1.0);
        o.push_back(-3.0);
        o.push_back(-1.0);
    }

    // The sort key of an object (its handle triple), for outputs listed in hash order upstream.
    std::vector<double> key(const SmObject* p) const
    {
        std::vector<double> k;
        put(k, p);
        return k;
    }
};

bool edge_ok(const SmEdge* e) { return e != nullptr && e->GetCurve() != nullptr && e->GetVertex() != nullptr; }

// The tolerance of a vertex, edge or face (-1 for anything else).
double tol_of(const SmObject* p)
{
    if (p == nullptr)
        return -1.0;
    if (p->IsKindOf(SmVertex_TYPE))
        return (double)((const SmVertex*)p)->GetTolerance();
    if (p->IsKindOf(SmEdge_TYPE))
        return (double)((const SmEdge*)p)->GetTolerance();
    if (p->IsKindOf(SmFace_TYPE))
        return (double)((const SmFace*)p)->GetTolerance();
    return -1.0;
}
bool face_ok(const SmFace* f) { return f != nullptr && f->GetSurface() != nullptr; }

SmObject* rd_h(Rd& rd, const Two& t)
{
    int64_t b = rd.idx();
    int64_t kind = rd.idx();
    int64_t i = rd.idx();
    return t.resolve(b, kind, i);
}

void put_list(std::vector<double>& o, const Two& t, const std::vector<SmObject*>& v)
{
    o.push_back((double)v.size());
    for (SmObject* p : v)
        t.put(o, p);
}

template <class T> std::vector<SmObject*> as_objs(const SmTArray<T*>& a)
{
    std::vector<SmObject*> v;
    for (ULONG i = 0; i < a.GetSize(); i++)
        v.push_back((SmObject*)a[i]);
    return v;
}

void sort_objs(std::vector<SmObject*>& v, const Two& t)
{
    std::sort(v.begin(), v.end(), [&t](SmObject* a, SmObject* b) { return t.key(a) < t.key(b); });
}

void put_pc(std::vector<double>& o, const Two& t, const SmPointClassification& pc)
{
    o.push_back((double)pc.GetPointClass());
    t.put(o, pc.GetObject());
}

void read_pc(Rd& rd, const Two& t, SmPointClassification& pc)
{
    int64_t cls = rd.idx();
    SmObject* obj = rd_h(rd, t);
    pc.SetClassObject((SmPointClassificationType)cls, obj);
}

void put_pc_full(std::vector<double>& o, const Two& t, const SmPointClassification& pc)
{
    put_pc(o, t, pc);
    o.push_back(pc.GetPreSnapParam());
    o.push_back(pc.GetDeviation());
    o.push_back(pc.GetTParam());
    o.push_back(flg(pc.AreParametersSet()));
}

// Builds the two B-reps of a run; `o` gets `-1 status` / `-3 status` on failure.
bool build_two(Rd& rd, const char* path, Two& t, std::vector<double>& o)
{
    std::string p(path);
    std::string pa = p, pb;
    size_t nl = p.find('\n');
    if (nl != std::string::npos)
    {
        pa = p.substr(0, nl);
        pb = p.substr(nl + 1);
    }
    SmStatus st_a;
    t.brep[0] = build(rd, pa.c_str(), st_a);
    if (t.brep[0] == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(st_a));
        return false;
    }
    bool has_b = rd.flag();
    if (has_b)
    {
        SmStatus st_b;
        t.brep[1] = build(rd, pb.c_str(), st_b);
        if (t.brep[1] == nullptr)
        {
            o.push_back(-3.0);
            o.push_back(st(st_b));
            return false;
        }
    }
    walk(t.brep[0], t.g[0]);
    if (t.brep[1])
        walk(t.brep[1], t.g[1]);
    t.index_geometry();
    return true;
}

void run_script(Rd& rd, Two& t, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    int64_t ctor_kind = rd.idx();
    double approx = rd.num();
    double ang = rd.num();
    SmTopologyIntersector* ti;
    if (ctor_kind == 0)
        ti = new SmTopologyIntersector(ctx, t.brep[0], t.brep[1], approx, ang);
    else
    {
        ti = new SmTopologyIntersector(ctx);
        ti->SetPrimaryBrep(t.brep[0]);
        ti->SetOtherBrep(t.brep[1]);
        ti->SetThisApproxTol3d(approx);
        ti->SetThisAngTolRad(ang);
    }
    SmProtoTopologyManager* mgr = nullptr;

    int64_t n_ops = rd.idx();
    for (int64_t k = 0; k < n_ops; k++)
    {
        int64_t op = rd.idx();
        o.push_back((double)op);
        switch (op)
        {
        case S_RELATE:
        {
            SmObject* a = rd_h(rd, t);
            SmObject* b = rd_h(rd, t);
            if (a == nullptr || ctor_kind != 0)
            {
                // upstream dereferences null (IsKindOf of a null object; the maps of the
                // context-only constructor)
                o.push_back(ST_NULL_DEREF);
                break;
            }
            // Relate of a vertex/edge to a non-vertex/non-edge: NER returns SM_ERR_NULL_POINTER
            o.push_back(st(ti->Relate(a, b)));
            o.push_back(tol_of(a));
            o.push_back(tol_of(b));
            break;
        }
        case S_OTHER_MATE: t.put(o, ti->GetOtherMate(rd_h(rd, t))); break;
        case S_THIS_MATE: t.put(o, ti->GetThisMate(rd_h(rd, t))); break;
        case S_BREP_MATE: t.put(o, ti->GetBrepMate(rd_h(rd, t))); break;
        case S_IS_MATED:
        {
            SmObject* a = rd_h(rd, t);
            SmObject* b = rd_h(rd, t);
            o.push_back(flg(ti->IsMatedPair(a, b)));
            break;
        }
        case S_MAPPED_BREP:
        {
            int64_t ty = rd.idx();
            if (ctor_kind != 0)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            SmTArray<SmObject*> list;
            ti->GetMappedBrepObjects((SM_TYPE)ty, list);
            std::vector<SmObject*> v = as_objs(list);
            sort_objs(v, t); // upstream: hash order
            put_list(o, t, v);
            break;
        }
        case S_COMMON_EDGES:
        {
            SmTArray<SmEdge*> a, b;
            ti->GetCommonEdges(a, b);
            put_list(o, t, as_objs(a));
            put_list(o, t, as_objs(b));
            break;
        }
        case S_COMMON_VERTICES:
        {
            SmTArray<SmVertex*> a, b;
            ti->GetCommonVertices(a, b);
            put_list(o, t, as_objs(a));
            put_list(o, t, as_objs(b));
            break;
        }
        case S_SET_SUBSET:
        {
            int64_t n = rd.idx();
            SmTArray<SmObject*> subset;
            for (int64_t i = 0; i < n; i++)
            {
                SmObject* p = rd_h(rd, t);
                if (p)
                    subset.Add(p);
            }
            o.push_back(st(ti->SetSubset(subset)));
            SmMapPtrToPtr<SmObject, SmObject>* s = ti->GetSubset();
            if (s == nullptr)
                o.push_back(-1.0);
            else
            {
                SmTArray<SmObject*> keys;
                s->GetAllKeys(keys);
                std::vector<SmObject*> v = as_objs(keys);
                sort_objs(v, t);
                put_list(o, t, v);
            }
            break;
        }
        case S_FIXUP_END:
        case S_FIXUP_MID:
        {
            int64_t dim = rd.idx();
            double tol = rd.num();
            SmPointClassification pcb(tol, &ctx), pco(tol, &ctx);
            read_pc(rd, t, pcb);
            read_pc(rd, t, pco);
            SmStatus s = (op == S_FIXUP_END) ? ti->FixupEndPointClassMates((ULONG)dim, pcb, pco, nullptr)
                                             : ti->FixupMidPointClassMates((ULONG)dim, pcb, pco, nullptr);
            o.push_back(st(s));
            put_pc(o, t, pcb);
            put_pc(o, t, pco);
            break;
        }
        case S_GETTERS:
        {
            t.put(o, ti->GetPrimaryBrep());
            t.put(o, ti->GetOtherBrep());
            o.push_back(ti->GetThisApproxTol3d());
            o.push_back(ti->GetThisAngTolRad());
            break;
        }
        case S_SET_TOLS:
        {
            ti->SetThisApproxTol3d(rd.num());
            ti->SetThisAngTolRad(rd.num());
            o.push_back(ti->GetThisApproxTol3d());
            o.push_back(ti->GetThisAngTolRad());
            break;
        }
        case S_SET_BREPS:
        {
            ti->SetPrimaryBrep((SmBrep*)rd_h(rd, t));
            ti->SetOtherBrep((SmBrep*)rd_h(rd, t));
            t.put(o, ti->GetPrimaryBrep());
            t.put(o, ti->GetOtherBrep());
            break;
        }
        case S_REGISTER_SSI:
        {
            SmSurface* s1 = (SmSurface*)rd_h(rd, t);
            SmSurface* s2 = (SmSurface*)rd_h(rd, t);
            int64_t n = rd.idx();
            SmTArray<SmCurve*> c3, u1, u2;
            SmTArray<SmTsectCurveType> types;
            SmTArray<double> devs;
            for (int64_t i = 0; i < n; i++)
            {
                SmBSplineCurve* c = read_curve(rd, ctx);
                t.curves.push_back(c);
                c3.Add(c);
                for (SmTArray<SmCurve*>* arr : {&u1, &u2})
                {
                    if (rd.flag())
                    {
                        SmBSplineCurve* uv = read_curve(rd, ctx);
                        t.curves.push_back(uv);
                        arr->Add(uv);
                    }
                    else
                        arr->Add(nullptr);
                }
                types.Add((SmTsectCurveType)rd.idx());
                devs.Add(rd.num());
            }
            o.push_back(st(ti->RegisterExistingSSI(s1, s2, c3, u1, u2, types, devs)));
            break;
        }
        case S_CHECK_SSI:
        {
            SmSurface* s1 = (SmSurface*)rd_h(rd, t);
            SmSurface* s2 = (SmSurface*)rd_h(rd, t);
            SmBoolean have = FALSE;
            SmTArray<SmCurve*> c3, u1, u2;
            SmTArray<SmTsectCurveType> types;
            SmTArray<double> devs;
            SmStatus s = ti->CheckIfHaveExistingSSI(ctx, s1, s2, have, c3, u1, u2, types, devs);
            o.push_back(st(s));
            o.push_back(flg(have));
            o.push_back((double)c3.GetSize());
            for (ULONG i = 0; i < c3.GetSize(); i++)
            {
                put_curve(o, c3[i]);
                put_curve(o, i < u1.GetSize() ? u1[i] : nullptr);
                put_curve(o, i < u2.GetSize() ? u2[i] : nullptr);
                o.push_back(i < types.GetSize() ? (double)types[i] : -1.0);
                o.push_back(i < devs.GetSize() ? devs[i] : -1.0);
            }
            // the copies are leaked upstream by callers that ignore them; free them here
            for (ULONG i = 0; i < c3.GetSize(); i++)
            {
                if (c3[i] != nullptr && std::find(t.curves.begin(), t.curves.end(), c3[i]) == t.curves.end())
                    delete c3[i];
                if (i < u1.GetSize() && u1[i] != nullptr && std::find(t.curves.begin(), t.curves.end(), u1[i]) == t.curves.end())
                    delete u1[i];
                if (i < u2.GetSize() && u2[i] != nullptr && std::find(t.curves.begin(), t.curves.end(), u2[i]) == t.curves.end())
                    delete u2[i];
            }
            break;
        }
        case S_CLEAR_SSI: o.push_back(st(ti->ClearExistingSSI())); break;
        case S_CAN_IMPRINT: o.push_back(flg(sm_CanImprint((SmPointClassificationType)rd.idx()))); break;
        case S_LOWEST:
        {
            int64_t n = rd.idx();
            SmSolutionArray sols;
            bool ok = true;
            for (int64_t i = 0; i < n; i++)
            {
                SmObject* p = rd_h(rd, t);
                SmSolution sol;
                sol.m_apObjects[0] = p;
                sol.m_lNumObjects = 1;
                sols.Add(sol);
                if (p == nullptr && n > 1)
                    ok = false; // GetType of a null object
            }
            if (!ok)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            o.push_back((double)sm_FindFirstLowestDimensionSol(sols));
            break;
        }
        case S_SET_EDGE_CLASS:
        {
            SmEdge* e = (SmEdge*)rd_h(rd, t);
            double t0 = rd.num(), t1 = rd.num(), tol = rd.num();
            if (e == nullptr || e->GetCurve() == nullptr || e->GetPrimaryEdgeuse() == nullptr || e->GetVertex() == nullptr)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            SmCurveClassification cc(e->GetCurve(), SmExtent1d(t0, t1), nullptr, tol);
            SmStatus s = sm_SetEdgeClass(cc, e);
            o.push_back(st(s));
            if (s != SM_SUCCESS)
                break;
            o.push_back((double)cc.GetSize());
            put_pc_full(o, t, cc[0].m_vStart);
            put_pc_full(o, t, cc[0].m_vMid);
            put_pc_full(o, t, cc[0].m_vEnd);
            break;
        }
        case S_CHECK_INTERIOR:
        {
            SmEdge* e = (SmEdge*)rd_h(rd, t);
            double tt = rd.num(), tol = rd.num();
            if (e == nullptr || e->GetCurve() == nullptr || e->GetVertex() == nullptr)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            SmBoolean interior = FALSE;
            SmStatus s = sm_CheckInteriorEdge(e, tt, tol, interior);
            o.push_back(st(s));
            o.push_back(flg(interior));
            break;
        }
        case S_SPLIT_TWO:
        {
            SmEdge* e1 = (SmEdge*)rd_h(rd, t);
            SmEdge* e2 = (SmEdge*)rd_h(rd, t);
            double tol = rd.num();
            if (!edge_ok(e1) || !edge_ok(e2))
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            double dist = 0.0, unst = 0.0;
            SmTArray<SmVertex*> nv;
            SmTArray<SmEdge*> ne1, ne2;
            SmStatus s = sm_TestAndSplitTwoEdges(e1, e2, tol, dist, unst, nv, ne1, ne2);
            o.push_back(st(s));
            if (s != SM_SUCCESS)
                break;
            o.push_back(dist);
            o.push_back(unst);
            o.push_back((double)nv.GetSize());
            o.push_back((double)ne1.GetSize());
            o.push_back((double)ne2.GetSize());
            break;
        }
        case S_IMPRINT_FACE:
        {
            SmFace* f = (SmFace*)rd_h(rd, t);
            SmPoint3d p = rd.vec();
            double tol = rd.num();
            if (!face_ok(f))
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            sReasonType reason = NO_REASON;
            double dist = 0.0;
            SmVertex* nv = nullptr;
            SmStatus s = sm_TestAndImprintFace(f, p, tol, reason, dist, nv);
            o.push_back(st(s));
            if (s != SM_SUCCESS)
                break;
            o.push_back((double)reason);
            o.push_back(dist);
            o.push_back(nv ? 1.0 : 0.0);
            break;
        }
        case S_VERTEX_FACE:
        {
            int64_t nv = rd.idx();
            SmTArray<SmVertex*> verts;
            for (int64_t i = 0; i < nv; i++)
                verts.Add((SmVertex*)rd_h(rd, t));
            int64_t nf = rd.idx();
            SmTArray<SmFace*> faces;
            for (int64_t i = 0; i < nf; i++)
                faces.Add((SmFace*)rd_h(rd, t));
            SmBoolean manifold = rd.bflag();
            SmBoolean ignore = rd.bflag();
            bool ok = true;
            for (ULONG i = 0; i < faces.GetSize(); i++)
                if (faces[i] != nullptr && !face_ok(faces[i]))
                    ok = false;
            if (!ok)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            SmTArray<SmVertex*> pv;
            SmTArray<SmFace*> pf;
            double mx = 0.0, mn = 0.0;
            SmStatus s = ti->VertexFaceIntersect(verts, faces, pv, pf, mx, mn, manifold, ignore);
            o.push_back(st(s));
            if (s != SM_SUCCESS)
                break;
            put_list(o, t, as_objs(pv));
            put_list(o, t, as_objs(pf));
            o.push_back(mx);
            o.push_back(mn);
            o.push_back((double)verts.GetSize());
            for (ULONG i = 0; i < verts.GetSize(); i++)
                o.push_back(verts[i] ? 1.0 : 0.0);
            break;
        }
        case S_EDGE_FACE:
        {
            int64_t ne = rd.idx();
            SmTArray<SmEdge*> edges;
            for (int64_t i = 0; i < ne; i++)
                edges.Add((SmEdge*)rd_h(rd, t));
            int64_t nf = rd.idx();
            SmTArray<SmFace*> faces;
            for (int64_t i = 0; i < nf; i++)
                faces.Add((SmFace*)rd_h(rd, t));
            SmBoolean manifold = rd.bflag();
            SmBoolean ignore = rd.bflag();
            bool ok = true;
            for (ULONG i = 0; i < edges.GetSize(); i++)
                if (edges[i] != nullptr && !edge_ok(edges[i]))
                    ok = false;
            for (ULONG i = 0; i < faces.GetSize(); i++)
                if (faces[i] != nullptr && !face_ok(faces[i]))
                    ok = false;
            if (!ok)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            SmTArray<SmVertex*> nv;
            SmTArray<SmEdge*> se, nwe;
            SmTArray<SmFace*> pf;
            double mx = 0.0, mn = 0.0;
            SmStatus s = ti->EdgeFaceIntersect(edges, faces, nv, se, nwe, pf, mx, mn, manifold, ignore);
            o.push_back(st(s));
            if (s != SM_SUCCESS)
                break;
            o.push_back((double)nv.GetSize());
            put_list(o, t, as_objs(se));
            o.push_back((double)nwe.GetSize());
            put_list(o, t, as_objs(pf));
            o.push_back(mx);
            o.push_back(mn);
            break;
        }
        case S_FACE_FACE:
        {
            int64_t nv = rd.idx();
            SmTArray<SmVertex*> verts;
            for (int64_t i = 0; i < nv; i++)
                verts.Add((SmVertex*)rd_h(rd, t));
            int64_t n1 = rd.idx();
            SmTArray<SmFace*> f1;
            for (int64_t i = 0; i < n1; i++)
                f1.Add((SmFace*)rd_h(rd, t));
            int64_t n2 = rd.idx();
            SmTArray<SmFace*> f2;
            for (int64_t i = 0; i < n2; i++)
                f2.Add((SmFace*)rd_h(rd, t));
            SmBoolean ignore = rd.bflag();
            SmBoolean nesting = rd.bflag();
            bool ok = true;
            for (ULONG i = 0; i < f1.GetSize(); i++)
                if (!face_ok(f1[i]))
                    ok = false;
            for (ULONG i = 0; i < f2.GetSize(); i++)
                if (!face_ok(f2[i]))
                    ok = false;
            for (ULONG i = 0; i < verts.GetSize(); i++)
                if (verts[i] == nullptr)
                    ok = false;
            if (!ok)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            SmTArray<SmFace*> pf, nf;
            SmTArray<SmEdge*> ne;
            double mx = 0.0, mn = 0.0;
            SmStatus s = ti->FaceFaceIntersect(verts, f1, f2, pf, nf, ne, mx, mn, ignore, nesting);
            o.push_back(st(s));
            if (s != SM_SUCCESS)
                break;
            put_list(o, t, as_objs(pf));
            o.push_back((double)nf.GetSize());
            o.push_back((double)ne.GetSize());
            o.push_back(mx);
            o.push_back(mn);
            break;
        }
        case S_FACE_FACE_COIN:
        {
            int64_t ne = rd.idx();
            SmTArray<SmEdge*> edges;
            for (int64_t i = 0; i < ne; i++)
                edges.Add((SmEdge*)rd_h(rd, t));
            int64_t nd = rd.idx();
            SmTArray<SmFace*> del;
            for (int64_t i = 0; i < nd; i++)
                del.Add((SmFace*)rd_h(rd, t));
            SmBoolean ignore = rd.bflag();
            SmBoolean nesting = rd.bflag();
            bool ok = true;
            for (ULONG i = 0; i < edges.GetSize(); i++)
            {
                if (edges[i] == nullptr)
                    continue;
                SmTArray<SmFace*> fs;
                edges[i]->GetFaces(fs);
                for (ULONG j = 0; j < fs.GetSize(); j++)
                    if (!face_ok(fs[j]))
                        ok = false;
            }
            if (!ok)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            SmTArray<SmFace*> pf, df;
            double mx = 0.0, mn = 0.0;
            SmStatus s = ti->FaceFaceCoincidence(edges, del, pf, df, mx, mn, ignore, nesting);
            o.push_back(st(s));
            if (s != SM_SUCCESS)
                break;
            put_list(o, t, as_objs(pf));
            put_list(o, t, as_objs(df));
            o.push_back(mx);
            o.push_back(mn);
            break;
        }
        case S_MGR_NEW:
        {
            double capture = rd.num();
            double override_tol = rd.num();
            if (t.brep[1] == nullptr || mgr != nullptr)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            mgr = new (t.brep[0]) SmProtoTopologyManager(t.brep[0], t.brep[1], ti, capture, override_tol);
            ti->*ti_get(TiMgr()) = mgr;
            o.push_back(0.0);
            break;
        }
        case S_RESOLVE:
        case S_IMPRINT:
        {
            if (mgr == nullptr)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            SmStatus s = (op == S_RESOLVE) ? ti->ResolveIntersections() : ti->ImprintIntersections();
            o.push_back(st(s));
            o.push_back((double)mgr->GetProtoVertices().GetSize());
            o.push_back((double)mgr->GetProtoEdges().GetSize());
            break;
        }
        case S_SHELL_VERTS:
        case S_WIRE_LAMINA:
        {
            int64_t first = rd.idx();
            SmBoolean lamina = (op == S_WIRE_LAMINA) ? rd.bflag() : FALSE;
            SmBoolean swap = rd.bflag();
            if (mgr == nullptr || t.brep[1] == nullptr)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            SmBrep* b1 = (first == 0) ? t.brep[0] : t.brep[1];
            SmBrep* b2 = (first == 0) ? t.brep[1] : t.brep[0];
            SmStatus s = (op == S_SHELL_VERTS) ? sm_IntersectShellVertices(b1, b2, swap, mgr)
                                               : sm_IntersectWireLaminaEdges(b1, b2, lamina, swap, mgr);
            o.push_back(st(s));
            o.push_back((double)mgr->GetProtoVertices().GetSize());
            o.push_back((double)mgr->GetProtoEdges().GetSize());
            break;
        }
        default: o.push_back(-9.0); break;
        }
    }
    delete ti; // the destructor frees the table (and its curves), maps and subset
    if (mgr)
        delete mgr;
}

void run_ti(int32_t op, Rd& rd, const char* path, std::vector<double>& o)
{
    Two t;
    struct Clean
    {
        Two& t;
        ~Clean()
        {
            for (SmCurve* c : t.curves)
                delete c;
            delete t.brep[0];
            delete t.brep[1];
        }
    } clean{t};
    if (!build_two(rd, path, t, o))
        return;
    o.push_back(0.0);
    if (op == TI_EXPORT)
    {
        for (int b = 0; b < 2; b++)
        {
            if (t.brep[b] == nullptr)
            {
                o.push_back(0.0);
                continue;
            }
            std::vector<double> ex;
            ex.push_back(0.0);
            export_graph(t.brep[b], t.g[b], ex);
            o.push_back((double)ex.size());
            o.insert(o.end(), ex.begin(), ex.end());
        }
        return;
    }
    run_script(rd, t, o);
}
} // namespace

SC_EXPORT int32_t sc_boolean_topology_intersector_p1_eval(int32_t op, const char* path, const double* in, int64_t n_in,
                                                          double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    run_ti(op, rd, path, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
