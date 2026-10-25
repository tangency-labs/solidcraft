// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.brep_p1`: the SmBrep members owned by the unit
// (source/SMLib/inc/SmBrep.h, source/SMLib/src/SmBrep.cpp) on real SmBrep objects, plus
// the file-static helpers of SmBrep.cpp the unit owns.
//
// Static functions of SmBrep.cpp cannot be linked against, so sm_CheckUVTrimCurve,
// sm_SwapFaceUV, sm_MarkLoopsToDelete, sm_RmAndDel_OldLoopVertices, sm_VoteForRegion,
// sm_FindHighestVote, sm_AddAttributes, sm_SetObjectMap, sm_GetObjectMap,
// sm_ReverseReflectionEdgeuses, sm_SwapReflectionShells, sm_FindSectorOfFace and sm_FindSideOfCurve are
// VERBATIM copies of upstream SmBrep.cpp (commit c9c5979) inside namespace sc_brep_p1
// (line ranges noted at each block; SM_DEBUG_CODE blocks dropped, as in the reference
// build). Where upstream would dereference a null pointer or recurse without bound, the
// copies return ST_NULL_DEREF instead (marked `// GUARD`): those inputs are undefined
// upstream and the Rust side reports them as InvalidInput.
//
// One entry point, sc_topology_brep_p1_eval(op, path, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/topology_brep_p1.rs, which
// validates every input. Every op starts with the B-rep block of sc_edgeuse_graph.h
// (`0 rebuild validate_only init_gaps` ReadFromFile, `1 curve t0 t1 ..` wire edge,
// `2 surface u0 v0 u1 v1 ..` face, `3 ..` the prog_test wire network). The B-rep graph
// export (traversal order, raw members) is shared with units topology.edgeuse_p1/p2.
//
// The shim's SmContext is shared by every case of a run (C++ global state); the ops
// that write the context's size estimate restore it before returning.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAttribute.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmContext.h>
#include <SmCopyBrepMap.h>
#include <SmCurve.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmFace.h>
#include <SmFaceuse.h>
#include <SmGap.h>
#include <SmLine.h>
#include <SmLocalSolveNd.h>
#include <SmLoop.h>
#include <SmLoopuse.h>
#include <SmMapPtrToPtr.h>
#include <SmMatrix.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmRegion.h>
#include <SmShell.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmTol.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <SmVertex.h>
#include <SmVertexuse.h>
#include <nurbs.h>

#include <cstring>
#include <map>
#include <set>
#include <vector>

#include <SmCacheMgr.h>
#include <SmSurfaceCache.h>

#include "sc_edgeuse_graph.h"

// SmBrep.cpp helpers with external linkage (no header declares them)
SmBoolean sm_RedistributeSeamUVTrimCurves(SmEdge* pNewE, SmBSplineCurve* pUVTrimCurve, SmSurfParamType eSeamEdge, SmExtent2d sSrfDomain);
SmStatus sm_MakeVertexSplitUVTrimCurve(SmEdgeuse* pEUOrig1, SmEdgeuse* pEUNew1, SmPoint3d sSplitPnt, double dEdgeSplitParameter, double dScaledZero, SmOrientType eOrient, SmBoolean bForTesselation);

namespace
{
// Status of an op the shim refuses because upstream would dereference null (or recurse
// without bound).
const double ST_HANG34_GUARD = -34.0; // the runner reports the case as rejected
const double ST_NULL_DEREF = -2.0;
const SmStatus SM_ST_NULL_DEREF = (SmStatus)(-2);
// Deepest sm_MarkLoopsToDelete recursion followed (as topology::brep::p1::MAX_LOOP_NESTING).
const ULONG MAX_LOOP_NESTING = 256;

// ---- raw member access ------------------------------------------------------------------
struct BrPeek : SmBrep
{
    static SmTArray<SmSurface*>* SmBrep::*deleted() { return &BrPeek::m_pDeletedSurfaces; }
    static SmTopology* SmBrep::*list() { return &BrPeek::m_pList; }
    // the size estimate accessors are protected (friends: SmTol, SmHealData)
    typedef double (SmBrep::*GetD)(SmBoolean) const;
    static GetD get_est() { return &BrPeek::GetThisModelSizeEstimate; }
    static GetD get_ratio() { return &BrPeek::GetThisLargeSmallSizeRatio; }
    typedef SmZoneTol3d (SmBrep::*SetEst)(double);
    static SetEst set_est() { return &BrPeek::SetThisModelSizeEstimate; }
    typedef void (SmBrep::*SetRatio)(double);
    static SetRatio set_ratio() { return &BrPeek::SetThisLargeSmallSizeRatio; }
};
double brep_est(const SmBrep* b, SmBoolean chase) { return (b->*BrPeek::get_est())(chase); }
double brep_ratio(const SmBrep* b, SmBoolean chase) { return (b->*BrPeek::get_ratio())(chase); }
struct CtxPeek : SmContext
{
    static double SmContext::*est() { return &CtxPeek::m_dThisModelSizeEstimate; }
    static double SmContext::*ratio() { return &CtxPeek::m_dThisLargeSmallSizeRatio; }
    static SmZoneTol3d SmContext::*tol() { return &CtxPeek::m_sThisZoneTol3d; }
};

// SmContext::GetThisModelSizeEstimate / GetThisLargeSmallSizeRatio are protected (friends:
// SmBrep, SmHealData); the same expressions on the raw members.
double ctx_est(const SmContext& c)
{
    double v = c.*CtxPeek::est();
    return v == SM_USE_DEFAULT ? SmTol::GetModelSizeEstimate() : v;
}
double ctx_ratio(const SmContext& c)
{
    double v = c.*CtxPeek::ratio();
    return v == SM_USE_DEFAULT ? SmTol::GetLargeSmallSizeRatio() : v;
}

struct CtxSave
{
    SmContext& c;
    double est, ratio;
    SmZoneTol3d tol;
    CtxSave(SmContext& ctx) : c(ctx), est(ctx.*CtxPeek::est()), ratio(ctx.*CtxPeek::ratio()), tol(ctx.*CtxPeek::tol()) {}
    ~CtxSave()
    {
        c.*CtxPeek::est() = est;
        c.*CtxPeek::ratio() = ratio;
        c.*CtxPeek::tol() = tol;
    }
};
} // namespace

namespace sc_brep_p1
{
// clang-format off
// ---- verbatim: SmBrep.cpp lines 116-199: sm_CheckUVTrimCurve (GUARD lines added)
static SmStatus sm_CheckUVTrimCurve( SmEdgeuse *pEU )
{
  SmBSplineCurve *pTrimCrv = pEU->GetUVTrimCurvePointer();  // Doesn't check mate.
  if(pTrimCrv == NULL ) { return SM_SUCCESS; }

  // 'Start' will be the start of pTrimCrv, same as start of Edge and Edge curve.
  SmBoolean bReverse = ( pEU->GetOrientation() == SM_OT_OPPOSITE) ;

  SmEdgeuse *pMate = pEU->GetMate();
  if(pMate == NULL) { return SM_ST_NULL_DEREF; } // GUARD

  SmPoint3d sStartUV, sEndUV;
//cbi this might not work for rationals:
  pTrimCrv->GetEnds( sStartUV, sEndUV) ; // Trim curve should start and end right at Edge ends.

  SmPoint3d sEdge3dStart, sEdge3dEnd;
  SmPoint3d sFace3dStart, sFace3dEnd;
  SmPoint3d sVrtx3dStart, sVrtx3dEnd;


  SmEdge *pEdge = pEU->GetEdge();
  if(pEdge == NULL || pEdge->GetCurve() == NULL) { return SM_ST_NULL_DEREF; } // GUARD
  if(pEU->GetVertexuse() == NULL || pMate->GetVertexuse() == NULL) { return SM_ST_NULL_DEREF; } // GUARD
  if(pEU->GetVertexuse()->GetVertex() == NULL || pMate->GetVertexuse()->GetVertex() == NULL) { return SM_ST_NULL_DEREF; } // GUARD
  if(pEU->GetFace() == NULL || pEU->GetFace()->GetSurface() == NULL) { return SM_ST_NULL_DEREF; } // GUARD
  SmExtent1d sEdgeIvl = pEdge->GetInterval();
  pEdge->GetCurve()->EvaluatePoint( sEdgeIvl.Evaluate(0.0), sEdge3dStart) ;


  SmVertex *pStartVtx = ( bReverse ) ? pMate->GetVertexuse()->GetVertex()
                                     :  pEU ->GetVertexuse()->GetVertex();
  sVrtx3dStart = pStartVtx->GetPoint();

  SmFace    *pFace = pEU->GetFace();
  SmSurface *pSurf = pFace->GetSurface();
  SmPoint2d sTemp( sStartUV.x, sStartUV.y) ;
  pSurf->EvaluatePoint( sTemp, sFace3dStart) ;

  double dFaceTol = pFace->GetTolerance();
  double dEdgeTol = pEdge->GetTolerance();
  double dVrtxTol = pStartVtx->GetTolerance();

  double dDistFaceVrtxStart = sFace3dStart.DistanceBetween( sVrtx3dStart) ;
  if(dDistFaceVrtxStart > dFaceTol + dVrtxTol )
    { return SM_ERR; }
  double dDistFaceEdgeStart = sFace3dStart.DistanceBetween( sEdge3dStart) ;
  if(dDistFaceEdgeStart > dFaceTol + dEdgeTol )
    { return SM_ERR; }

  // Now the other end:
  pEdge->GetCurve()->EvaluatePoint( sEdgeIvl.Evaluate(1.0), sEdge3dEnd) ;

  SmVertex *pEndVtx = ( bReverse ) ?  pEU ->GetVertexuse()->GetVertex()
                                   : pMate->GetVertexuse()->GetVertex();
  sVrtx3dEnd = pEndVtx->GetPoint();

  sTemp.Set( sEndUV.x, sEndUV.y) ;
  pSurf->EvaluatePoint( sTemp, sFace3dEnd) ;

  dVrtxTol = pEndVtx->GetTolerance(); // have to update this to End vtx.

  double dDistFaceVrtxEnd = sFace3dEnd.DistanceBetween( sVrtx3dEnd) ;
  if(dDistFaceVrtxEnd > dFaceTol + dVrtxTol )
    { return SM_ERR; }
  double dDistFaceEdgeEnd = sFace3dEnd.DistanceBetween( sEdge3dEnd) ;
  if(dDistFaceEdgeEnd > dFaceTol + dEdgeTol )
    { return SM_ERR; }

  return SM_SUCCESS;

} // end sm_CheckUVTrimCurve

// ---- verbatim: SmBrep.cpp lines 240-293: sm_SwapFaceUV
static void sm_SwapFaceUV( SmFace *pFace, SmMarkType eMarkType)
{
  // skip faces already marked
  if (pFace->IsMarked(eMarkType))
      return;

// Remove Composites - added block from below
  // swap and mark the face
      SE(pFace->SwapUV());
      pFace->Mark(eMarkType);
} // end sm_SwapFaceUV

// ---- verbatim: SmBrep.cpp lines 3366-3416: sm_MarkLoopsToDelete (GUARD lines added)
static SmStatus sm_MarkLoopsToDelete
  (SmTArray<SmLoop*>& raLoopsArg,
   SmLoop* pLoopArg, ULONG nRecLevelArg)
{
    if (nRecLevelArg > MAX_LOOP_NESTING) { return SM_ST_NULL_DEREF; } // GUARD
    if (pLoopArg == NULL) { return SM_ST_NULL_DEREF; } // GUARD
    SmFace* pF = pLoopArg->GetFace();
    if (pF == NULL) { return SM_ST_NULL_DEREF; } // GUARD

    SmBoolean bEvenLevel = (nRecLevelArg % 2 == 0) ? TRUE : FALSE;

    SmTArray<SmLoop*> aLoops;
    pF->GetLoops(aLoops);

    for (ULONG ii=0; ii<aLoops.GetSize(); ii++) {
        SmLoop* pL = aLoops[ii];
        if (pL == pLoopArg)
            continue;

        SmTArray<SmEdge*> sEdges;
        pL->GetEdges(sEdges);

        if (sEdges.GetSize() == 0 || sEdges[0] == NULL) { return SM_ST_NULL_DEREF; } // GUARD
        SmEdge* pE = sEdges[0];
        if (pE->IsWire() )  { SER(SM_ERR); }
        if (pE->IsLamina() )  { SER(SM_ERR); }
        if (!pE->IsManifold() )  { SER(SM_ERR); }

        SmEdgeuse *pEU = pE->GetPrimaryEdgeuse(); // may not belong to pF
        if (pEU == NULL) { return SM_ST_NULL_DEREF; } // GUARD
        SmEdgeuse* pEURad = pEU->GetRadial();
        if (pEURad == NULL || pEURad->GetLoopuse() == NULL || pEU->GetLoopuse() == NULL) { return SM_ST_NULL_DEREF; } // GUARD
        SmLoop* pRadL = pEURad->GetLoopuse()->GetLoop();
        if (pRadL == NULL) { return SM_ST_NULL_DEREF; } // GUARD

        if (pRadL->GetFace() == pF)
            pRadL = pEU->GetLoopuse()->GetLoop();

        SER(sm_MarkLoopsToDelete(raLoopsArg,
            pRadL,
            nRecLevelArg+1));  // recursion level
    }

    if (bEvenLevel )
        raLoopsArg.Add(pLoopArg);

    return SM_SUCCESS;

} // end sm_MarkLoopsToDelete

// ---- verbatim: SmBrep.cpp lines 10589-10639: sm_RmAndDel_OldLoopVertices (GUARD lines added)
static SmStatus sm_RmAndDel_OldLoopVertices
  (SmVertexuse *pVertexuse) // in : one of two stale LoopVertexuses on
                            //      a Vertex just connected to pNewE
{
  // when pVertexuse is a LoopVertexuse
  if (pVertexuse->IsLoopVertexuse())
    {
      // locals
      SmVertexuse * pVUDel = NULL;
      SmVertexuse * pOtherVU = (SmVertexuse*)pVertexuse->GetNext(); NER(pOtherVU);
      if (pVertexuse->GetLoopuse() == NULL || (pOtherVU->IsLoopVertexuse() && pOtherVU->GetLoopuse() == NULL)) { return SM_ST_NULL_DEREF; } // GUARD

      // When OtherVU is a LoopVertexuse attached to the same Loop as input VU
      if(   pOtherVU->IsLoopVertexuse()
         && pOtherVU->GetLoopuse()->GetLoop() == pVertexuse->GetLoopuse()->GetLoop())
        {
          // target it for deletion
          pVUDel = pOtherVU;
        }
      else // OtherVU is not a LoopVertexuse attached to the same Loop as input VU
        {
          // try the VU in the other direction of the Vertex's doubly linked Vertexuse list
          pOtherVU = (SmVertexuse*)pVertexuse->GetLast(); NER(pOtherVU);
          if (pOtherVU->IsLoopVertexuse() && pOtherVU->GetLoopuse() == NULL) { return SM_ST_NULL_DEREF; } // GUARD

          // When OtherVU is a LoopVertexuse attached to the same Loop as input VU
          if(   pOtherVU->IsLoopVertexuse()
             && pOtherVU->GetLoopuse()->GetLoop() == pVertexuse->GetLoopuse()->GetLoop())
            {
              // target it for deletion
              pVUDel = pOtherVU;
            }
          else
            {
              SER(SM_ERR);  // Unable to find the partner of this loop vertexuse
            }
        } // end OtherVU is not a LoopVertexuse attached to the same Loop as input VU branch

      // remove the Tgt LoopVertexuse and its mated pair from their Vertex
      SmVertex * pV = pVertexuse->GetVertex();
      if (pV == NULL) { return SM_ST_NULL_DEREF; } // GUARD
      pV->Remove(pVertexuse);
      pV->Remove(pVUDel);

      // delete the LoopVertexuses
      SM_ASSERT(pVertexuse != NULL) ; delete pVertexuse ; pVertexuse = NULL ;
      SM_ASSERT(pVUDel     != NULL) ; delete pVUDel ;     pVUDel     = NULL ;

    } // end IsLoopVertex() check

  // all done
  return SM_SUCCESS;

} // end sm_RmAndDel_OldLoopVertices

// ---- verbatim: SmBrep.cpp lines 11165-11201: sm_VoteForRegion, sm_FindHighestVote
static void sm_VoteForRegion
  (SmTArray<SmRegion*> & rRegions,
   SmTArray<ULONG> & rRegionVotes,
   SmRegion * pNewRegion)
{
    for (ULONG i=0; i<rRegions.GetSize();i++) {
        if (rRegions[i] == pNewRegion) {
            rRegionVotes[i] = rRegionVotes[i] + 1;
            return;
        }
    }
    rRegions.Add(pNewRegion);
    rRegionVotes.Add(1);

} // end sm_VoteForRegion

static SmRegion * sm_FindHighestVote
  (SmTArray<SmRegion*> & rRegions,
   SmTArray<ULONG> & rRegionVotes)
{
    if (rRegions.GetSize() == 0) {
        return NULL;
    }
    ULONG lHighest = 0;
    for (ULONG i=1; i<rRegions.GetSize();i++) {
        if (rRegionVotes[lHighest] < rRegionVotes[i]) {
            lHighest = i;
        }
    }
    return rRegions[lHighest];

} // end sm_FindHighestVote

// ---- verbatim: SmBrep.cpp lines 19209-19231: sm_AddAttributes
static SmStatus sm_AddAttributes
  (SmAObject              * pObject,             // in : target object
   SmTArray<ULONG>        & sIndexedAttributes,  // in : array of attribute indices
                                                 //      in rAttributes to add to pObject
   SmTArray<SmAttribute*> & rAttributes)         // in : array of attributes
{
    // for every Attribute index
    for (ULONG i=0; i<sIndexedAttributes.GetSize();i++)
      {
        // get index
        ULONG lIndex = sIndexedAttributes[i];

        // get attribute
        SmAttribute *pAttr = rAttributes[lIndex];

        // add attribute to object
        pObject->AddAttribute(pAttr);

      } // end iter every attribute index

    return SM_SUCCESS;

} // end sm_AddAttributes

// ---- verbatim: SmBrep.cpp lines 22802-22830: sm_SetObjectMap, sm_GetObjectMap
static void sm_SetObjectMap
  (SmTopology *pOrigObj,                        // in : Orig Topology Object being copied
   SmTopology *pCopyObj,                        // in : new Copy of Orig Topology, can be NULL
   SmMapPtrToPtr<SmTopology, SmTopology> &rMap, // in : Map to accumulate all From->To relationships
   SmCopyBrepMap * pOptCopyBrepMap=NULL,        // out: A map from copyBrep topology Obj ptrs to orig ptrs
   SmCopyBrepMap * pOptReverseMap =NULL )       // out: A map from orig topology Obj ptrs to copyBrep ptrs
                                                //      NULL to ignore, default:[NULL]
{
  // Use a map object track relationships.
  rMap.Insert(pOrigObj, pCopyObj);

  if (pOptCopyBrepMap ) { pOptCopyBrepMap->SetAt(pCopyObj, pOrigObj); }
  if (pOptReverseMap  ) { pOptReverseMap ->SetAt(pOrigObj, pCopyObj); }
} // end sm_SetObjectMap

static SmTopology *sm_GetObjectMap
  (SmTopology    *pFrom,                        // in : target object
   SmMapPtrToPtr<SmTopology, SmTopology> &rMap) // in : Map to accumulate all From->To relationships
{
  // Use a map object track relationships.
  SmTopology *pTo = rMap.At( pFrom) ;
  return pTo;

} // end sm_GetObjectMap

// ---- verbatim: SmBrep.cpp lines 23416-23442: sm_ReverseReflectionEdgeuses, sm_SwapReflectionShells (GUARD lines added)
static void sm_ReverseReflectionEdgeuses(SmEdge& edge)
{
    SmTArray<SmEdgeuse*> uses;
    edge.GetEdgeuses(uses);
    for (SmEdgeuse* use : uses)
        edge.Remove(use);
    for (ULONG i = 0; i < uses.GetSize(); i += 2)
    {
        edge.PostInsert(uses[i + 1]);
        edge.PostInsert(uses[i]);
    }
}

static SmStatus sm_SwapReflectionShells(SmFace& face)
{
    SmFaceuse *first, *second;
    face.GetFaceuses(first, second);
    if (first == NULL || second == NULL) { return SM_ST_NULL_DEREF; } // GUARD
    SmShell* a = first->GetShell();
    SmShell* b = second->GetShell();
    if (a == NULL || b == NULL) { return SM_ST_NULL_DEREF; } // GUARD
    if (a != b)
    {
        a->Remove(first);
        b->Remove(second);
        a->PostInsert(second);
        b->PostInsert(first);
    }
    return SM_SUCCESS; // (void upstream)
}
// ---- verbatim: SmBrep.cpp lines 11028-11158: sm_FindSectorOfFace (GUARD lines added)
static SmStatus sm_FindSectorOfFace
 (SmFace           * pFace,            // in : target face
  const SmPoint2d  * cpUVGuess,        // in : optional UVGuess on Face, NULL to ignore
  const SmPoint3d  & crTestPoint,      // in : 3d point to classify expected to be in the region to find near the intersection
                                       //      used when cpUVGuess == NULL
  const SmVector3d & crTestDir,        // in : optional test tangent from the intersection into the region to find
                                       //      used when cpUVGuess != NULL
  double           & rdDistance,       // out: distance from test point to face
  SmRegion        *& rpRegionOfSector) // out:
{
  SmPoint2d  sUVFound ;
  SmPoint3d  sSurfPnt ;
  SmVector3d sNorm ;
  SmVector3d sTestVec ;

  if (pFace->GetSurface() == NULL) { return SM_ST_NULL_DEREF; } // GUARD

  // Get SurfacePoint, SurfaceNormal, rdDistance, and sTestVec
  if (cpUVGuess)
    {
      // assume that the given cpUVGuess is on the face as expected
      sUVFound = *cpUVGuess;

      // found Surface Point
      SER(pFace->GetSurface()->EvaluatePoint(sUVFound,sSurfPnt));

      // Found Surface Point Normal
      SER(pFace->GetSurface()->EvaluateNormal(sUVFound,TRUE,TRUE,sNorm));

      // Can't classify a point on the surface - that point won't be in a region sector
      if(sNorm.IsPerpendicularTo(crTestDir, SM_EFF_ZERO_RAD))
        {
          // try old method of dropping a point to the surface

          // with cpUVGuess do a cheap local solve
          SmBoolean  bFoundAnswer;
          SmSolution sSolution;
          SER(pFace->GetSurface()->LocalPointSolve(pFace->GetUVDomain(),
                                                   SM_SO_MINIMIZE,
                                                   crTestPoint,*cpUVGuess,
                                                   bFoundAnswer, sSolution));
          if (!bFoundAnswer)
            { sUVFound = *cpUVGuess; }
          else
            { sUVFound = SmPoint2d(sSolution.m_vStart[0], sSolution.m_vStart[1]); }

          // found Surface Point
          SER(pFace->GetSurface()->EvaluatePoint(sUVFound,sSurfPnt));

          // Found Surface Point Normal
          SER(pFace->GetSurface()->EvaluateNormal(sUVFound,TRUE,TRUE,sNorm));

          // Test Vec
          sTestVec = crTestPoint - sSurfPnt;
        }
      else
        {
          // Test Vec
          sTestVec = crTestDir ;
        }
    }
  else // no UVGuess
    {
      // without cpUVGuess do a more expensive global solve
      SmSolution      sSData[16];
      SmSolutionArray sSolutions(16,sSData);
      SmSurfaceCache *pSC = smsurf_GetSurfaceCache(pFace->GetSurface());
      NER(pSC);
      SmCacheCheckOutIn sCheckIO(pSC);
        {
          // Turn off point testing so GlobalPointSolve() will keep all point solutions
          //   without classifying the solution point against the trim boundaries.
          SmTemporaryChangeValue<SmBoolean> sChange(pSC->m_bPointTestEnabled,FALSE);
          SER(pFace->GetSurface()->GlobalPointSolve(pFace->GetUVDomain(),
                                                    SM_SO_MINIMIZE,
                                                    crTestPoint,
                                                    SM_EFF_ZERO, NULL, SM_SR_ALL,
                                                    sSolutions));
        }
      if (sSolutions.GetSize() < 1) SER(SM_ERR);
      sUVFound = SmPoint2d(sSolutions[0].m_vStart[0],sSolutions[0].m_vStart[1]);

      // found Surface Point
      SER(pFace->GetSurface()->EvaluatePoint(sUVFound,sSurfPnt));

      // Found Surface Point Normal
      SER(pFace->GetSurface()->EvaluateNormal(sUVFound,TRUE,TRUE,sNorm));

      // Test Vec
      sTestVec = crTestPoint - sSurfPnt;
    }

  // size of sTestVec (From sSurfPnt to crTestPoint or crTestDir)
  rdDistance = sTestVec.Length();

  // Can't classify a point on the surface - that point won't be in a region sector
  if( rdDistance < SM_EFF_ZERO )
    { SER(SM_ERR); }

  // classify testPoint based on angle between FacePointNormal and TestVec
  if (sNorm.Dot(sTestVec) > 0.0)
    {
      SmFaceuse *pFU = pFace->GetUpwardFaceuse();
      if (pFU == NULL || pFU->GetShell() == NULL) { return SM_ST_NULL_DEREF; } // GUARD
      rpRegionOfSector = pFU->GetShell()->GetRegion();
    }
  else
    {
      SmFaceuse *pFU = pFace->GetUpwardFaceuse();
      if (pFU == NULL) { return SM_ST_NULL_DEREF; } // GUARD
      pFU = pFU->GetMate();
      if (pFU == NULL || pFU->GetShell() == NULL) { return SM_ST_NULL_DEREF; } // GUARD
      rpRegionOfSector = pFU->GetShell()->GetRegion();
    }

  // all done
  return SM_SUCCESS;

} // end sm_FindSectorOfFace

// ---- verbatim: SmBrep.cpp lines 24850-24916: sm_FindSideOfCurve
static SmStatus sm_FindSideOfCurve
 (const SmCurve    & crCurve,                // in : Curve to test
  const SmPoint3d  & crPoint,                // in : Point to test
  const SmVector3d & crProjectionDirection,  // in : Direction to project point onto curve
  double             dTol,                   // in :
  SmBoolean        & rbRightHandSide)        // out: TRUE = ProjPoint on Curve Right side
                                             //      FALSE= ProjPoint on Curve Left side
                                             //      (for observer standing on pos side of ProjectPlane
                                             //       moving along the curve in dir of curve tangent)
{
  // Init outputs
  rbRightHandSide = UNSURE;

  SmSolution      sData[16];
  SmSolutionArray sSolutions(16,sData);

  // Project Point onto Curve along the Projection direction
  SER(crCurve.GlobalPointSolve(crCurve.GetNaturalInterval(),
                               SM_SO_PROJECTED_MINIMIZE,
                               crPoint,
                               SM_EFF_ZERO_SQRT,
                               NULL,
                               &crProjectionDirection,
                               SM_SR_SINGLE,
                               sSolutions));

  // Determine if point is on left or right side of the curve relative to the projection plane.
  if (sSolutions.GetSize() != 1) 
    { SER(SM_ERR); }
  SmSolution & rSol = sSolutions[0];

  SmVector3d sPV[2];
  SER(crCurve.Evaluate(rSol.m_vStart[0],1,TRUE,sPV));
  SmVector3d sVec = crPoint - sPV[0];

  // Project vectors into projection plane
  sVec = crProjectionDirection * sVec * crProjectionDirection;

  // point is on the curve
  if(sVec.Length() < dTol )
    { return SM_ERR; }

  sVec.Unitize();
  SmVector3d sTan = crProjectionDirection * sPV[1] * crProjectionDirection;

  // projection direction is the same as the curve tangent or curve tangent is zero
  if (sTan.Length() < dTol)
    { return SM_ERR; }
  sTan.Unitize();

  // projected vec and curve tangent are parallel
  SmVector3d sSide = sTan * sVec;
  if (sSide.Length() < SM_EFF_ZERO)
    { return SM_ERR; }

  // set output
  rbRightHandSide = sSide.Dot(crProjectionDirection) <= 0.0 ;

  // all done
  return SM_SUCCESS;

} // end sm_FindSideOfCurve
// clang-format on
} // namespace sc_brep_p1

namespace
{
enum Op
{
    OP_EXPORT = 0,
    OP_LISTS = 1,
    OP_QUERY = 2,
    OP_SET_SIZE = 3,
    OP_SET_INF = 4,
    OP_DEGENERATE = 5,
    OP_ADD_DELETED = 6,
    OP_VOTE = 7,
    OP_OBJMAP = 8,
    OP_ADD_ATTRS = 9,
    OP_REFLECT_EU = 10,
    OP_REFLECT_SHELL = 11,
    OP_RM_LOOP_VU = 12,
    OP_MARK_LOOPS = 13,
    OP_SWAP_FACE_UV = 14,
    OP_CHECK_UV = 15,
    OP_LOCK_BOX = 16,
    OP_FIND_SIDE = 17,
    OP_FIND_SECTOR = 18,
    OP_UPDATE_IVLS = 19,
    OP_REDISTRIBUTE = 20,
    OP_SPLIT_UV = 21,
    OP_BREP_INTERSECT = 22,
    OP_CORNER_BLEND = 23,
    OP_EDGE_EDGE_BLEND = 24,
    OP_PARALLEL_PROJ = 25,
    OP_HEAL = 26,
    OP_EDGE_EDGE_BLEND2 = 27,
};

// The type codes IsKindOf is asked about.
const SM_TYPE KIND_TYPES[] = {SmBrep_TYPE,   SmSAGObject_TYPE, SmAObject_TYPE, SmObject_TYPE, SmRegion_TYPE,
                              SmTopology_TYPE, SmOwningTopology_TYPE, SmUnknown_TYPE, 0};
const size_t N_KIND_TYPES = sizeof(KIND_TYPES) / sizeof(KIND_TYPES[0]);

template <class T> void put_found(std::vector<double>& o, Graph& g, int kind, const SmTArray<T*>& arr)
{
    std::vector<int64_t> v;
    for (ULONG i = 0; i < arr.GetSize(); i++)
        v.push_back(g.find(kind, arr[i]));
    put_list(o, v);
}

// A registered topology object of any kind, by its type code.
int64_t find_any(Graph& g, SmTopology* t)
{
    if (t == nullptr)
        return -1;
    switch (t->GetType())
    {
    case SmRegion_TYPE: return g.find(0, (SmRegion*)t);
    case SmShell_TYPE: return g.find(1, (SmShell*)t);
    case SmFaceuse_TYPE: return g.find(2, (SmFaceuse*)t);
    case SmFace_TYPE: return g.find(3, (SmFace*)t);
    case SmLoopuse_TYPE: return g.find(4, (SmLoopuse*)t);
    case SmLoop_TYPE: return g.find(5, (SmLoop*)t);
    case SmEdgeuse_TYPE: return g.find(6, (SmEdgeuse*)t);
    case SmEdge_TYPE: return g.find(7, (SmEdge*)t);
    case SmVertexuse_TYPE: return g.find(8, (SmVertexuse*)t);
    case SmVertex_TYPE: return g.find(9, (SmVertex*)t);
    default: return -4;
    }
}

// The extras exported before the graph (OP_EXPORT): the SmBrep members the graph lacks
// (`inf_region editing_enabled size_estimate ratio head_is_infinite`), then `n_v` and
// each vertex's vertex use list in its list order (the graph export leaves it out).
void put_extras(SmBrep* brep, Graph& g, std::vector<double>& o)
{
    o.push_back((double)g.find(0, brep->GetInfiniteRegion()));
    o.push_back(flg(brep->m_bEditingEnabled));
    o.push_back(brep_est(brep, FALSE));
    o.push_back(brep_ratio(brep, FALSE));
    o.push_back(brep->*BrPeek::list() == (SmTopology*)brep->GetInfiniteRegion() ? 1.0 : 0.0);
    o.push_back((double)g.vertices.size());
    for (SmVertex* v : g.vertices)
    {
        std::vector<int64_t> vus;
        for (SmTopology* t : list_of(v->GetList()))
            vus.push_back(g.find(8, t));
        put_list(o, vus);
    }
    // the B-rep's face, edge and vertex lists in their list order (GetFaces / GetEdges /
    // GetVertices), as traversal indices
    {
        SmTArray<SmFace*> fs;
        brep->GetFaces(fs);
        put_found(o, g, 3, fs);
        SmTArray<SmEdge*> es;
        brep->GetEdges(es);
        put_found(o, g, 7, es);
        SmTArray<SmVertex*> vs;
        brep->GetVertices(vs);
        put_found(o, g, 9, vs);
    }
}

// The shared walk registers only the head of a wire shell's list; register every
// member (and what hangs off it) so wire networks export completely. Earlier indices
// are unchanged (registration appends).
void walk_more(SmBrep* brep, Graph& g)
{
    for (;;)
    {
        size_t before = g.shells.size() + g.edgeuses.size() + g.vertexuses.size() + g.edges.size() + g.vertices.size();
        for (size_t s = 0; s < g.shells.size(); s++)
        {
            SmShell* sh = g.shells[s];
            SM_TYPE ty = sh->*ShPeek::type();
            if (ty != SmFaceuse_TYPE)
                for (SmTopology* t : list_of(sh->GetList()))
                    g.typed(ty, t);
        }
        // a vertex's other uses (the walk only goes vertex use -> vertex)
        for (size_t v = 0; v < g.vertices.size(); v++)
            for (SmTopology* t : list_of(g.vertices[v]->GetList()))
                g.vertexuse((SmVertexuse*)t);
        walk(brep, g);
        size_t after = g.shells.size() + g.edgeuses.size() + g.vertexuses.size() + g.edges.size() + g.vertices.size();
        if (after == before)
            break;
    }
}

// The region list in GetRegions order, as indices.
void put_regions(SmBrep* brep, Graph& g, std::vector<double>& o)
{
    SmTArray<SmRegion*> regs;
    brep->GetRegions(regs);
    std::vector<int64_t> v;
    for (ULONG i = 0; i < regs.GetSize(); i++)
        v.push_back(g.find(0, regs[i]));
    put_list(o, v);
}

// Attaches a fresh attribute `id` to each `ty idx` target of the input.
void attach(Rd& rd, Graph& g, int64_t id)
{
    int64_t n = rd.idx();
    for (int64_t k = 0; k < n; k++)
    {
        int64_t ty = rd.idx();
        int64_t i = rd.idx();
        if (id < 0)
            continue;
        // SmSurface_TYPE / SmCurve_TYPE targets name a face's surface / an edge's curve
        if (ty == SmSurface_TYPE && i >= 0 && i < (int64_t)g.faces.size() && g.faces[(size_t)i]->GetSurface())
        {
            g.faces[(size_t)i]->GetSurface()->AddAttribute(new (the_context()) SmAttribute((ULONG)id, SM_AB_COPY));
            continue;
        }
        if (ty == SmCurve_TYPE && i >= 0 && i < (int64_t)g.edges.size() && g.edges[(size_t)i]->GetCurve())
        {
            g.edges[(size_t)i]->GetCurve()->AddAttribute(new (the_context()) SmAttribute((ULONG)id, SM_AB_COPY));
            continue;
        }
        SmTopology* t = target_of(g, ty, i);
        if (t != nullptr)
            t->AddAttribute(new (the_context()) SmAttribute((ULONG)id, SM_AB_COPY));
    }
}


// SmBrep::CreateCornerBlend (SmBrep.cpp:2488-2775) replayed read-only on a copy of the B-rep
// (surface types preserved), through SmBSplineSurface::CreateCornerBlend's curve conversion
// (SmBSplineSurfaceNLib.cpp:3060-3115, sm_CreateNonRationalCurve :2906-2965). TRUE when the real
// call would dereference memory it does not have:
// - the UV phase copies a NULL UV trim curve (`SM_ASSERT(pUVCurve != NULL)` is a release no-op
//   and `pUVCurve->Copy` dereferences it);
// - UB-195: a rational B-spline 3D curve with a neighbour surface whose DropCurve succeeds with
//   no curve, and `sUVCurves[0]` reads the empty array (the `SM_ASSERT(GetSize() == 1)` is a
//   release no-op).
// The replay keeps upstream's ordering, statuses and early returns, so it is TRUE exactly when
// the real call reaches one of those dereferences. `reaches_tangents` reports whether the call
// gets past the curve conversion into the cross-tangent approximation, the only place the
// tangency tolerance is used (four sides with a neighbour surface, or more sides; HANG-39). The
// later MakeCurvesCompatible / EditParameterization statuses are not replayed, so it may report
// a call that stops there.
static bool corner_blend_refused(SmBrep* brep, const SmContext& ctx, const SmTArray<SmEdgeuse*>& eus,
                                 const SmTArray<SmBoolean>* tans, double tol, bool& reaches_tangents)
{
    reaches_tangents = false;
    SmCopyBrepMap rev;
    SmBrep* copy = new (ctx) SmBrep(*brep, NULL, &rev, TRUE, FALSE);
    SmObjDelete copy_clean(copy);
    SmTArray<SmEdgeuse*> work;
    for (ULONG i = 0; i < eus.GetSize(); i++)
    {
        SmEdgeuse* c = (SmEdgeuse*)rev.GetAt(eus[i]);
        if (c == NULL)
            return true; // (not expected: refuse rather than run unguarded)
        work.Add(c);
    }
    SmTArray<SmEdgeuse*> ordered;
    SmTArray<SmOrientType> orients;
    SmTArray<SmBoolean> work_tan;
    if (tans)
        work_tan.Append(*tans);
    else
        work_tan.InsertAt(0, TRUE, eus.GetSize());
    for (ULONG ii = 0; ii < work.GetSize(); ii++)
    {
        if (work[ii]->GetOrientation() == SM_OT_OPPOSITE)
            work[ii] = work[ii]->GetMate();
        if (work[ii]->GetEdge()->IsWire())
            work_tan[ii] = FALSE;
    }
    SmTArray<SmBoolean> tangencies;
    tangencies.Add(work_tan[0]);
    ordered.Add(work[0]);
    orients.Add(work[0]->GetOrientation());
    work.RemoveAt(0);
    work_tan.RemoveAt(0);
    double tol_scale = 1.0;
    while (work.GetSize() > 0)
    {
        SmBoolean found = FALSE;
        for (ULONG ii = 0; ii < work.GetSize(); ii++)
        {
            SmEdgeuse* eu = work[ii];
            SmEdgeuse* last = ordered.GetLast();
            double param = (orients.GetLast() == SM_OT_SAME) ? 1.0 : 0.0;
            SmPoint3d end_point;
            if (last->NormalizedEvaluate(param, FALSE, end_point) != SM_SUCCESS)
                return false;
            SmPoint3d test_pt;
            if (eu->NormalizedEvaluate(0.0, FALSE, test_pt) != SM_SUCCESS)
                return false;
            if (test_pt.DistanceBetween(end_point) < tol * tol_scale)
            {
                tangencies.Add(work_tan[ii]);
                ordered.Add(work[ii]);
                orients.Add(SM_OT_SAME);
                work.RemoveAt(ii);
                work_tan.RemoveAt(ii);
                found = TRUE;
                tol_scale = 1.0;
                break;
            }
            if (eu->NormalizedEvaluate(1.0, FALSE, test_pt) != SM_SUCCESS)
                return false;
            if (test_pt.DistanceBetween(end_point) < tol * tol_scale)
            {
                tangencies.Add(work_tan[ii]);
                ordered.Add(work[ii]);
                orients.Add(SM_OT_OPPOSITE);
                work.RemoveAt(ii);
                work_tan.RemoveAt(ii);
                found = TRUE;
                tol_scale = 1.0;
                break;
            }
        }
        if (!found && tol_scale > 100.0)
            return false;
        if (!found)
            tol_scale *= 2.5;
    }
    // the CreateCornerBlend input arrays (copies owned here)
    SmTArray<SmCurve*> o3d;
    SmObjsDelete<SmCurve*> clean3d(&o3d);
    SmTArray<SmBSplineCurve*> ouv;
    SmObjsDelete<SmBSplineCurve*> cleanuv(&ouv);
    SmTArray<SmSurface*> osrf;
    for (ULONG ii = 0; ii < ordered.GetSize(); ii++)
    {
        SmEdgeuse* eu = ordered[ii];
        SmEdge* e = eu->GetEdge();
        SmCurve* crv = NULL;
        if (e->GetCurve()->Copy(ctx, crv) != SM_SUCCESS)
            return false;
        o3d.Add(crv);
        SmExtent1d ivl = e->GetInterval();
        if (crv->Trim(ivl) != SM_SUCCESS)
            return false;
        if (!tangencies[ii])
        {
            ouv.Add(NULL);
            osrf.Add(NULL);
        }
        else
        {
            SmBSplineCurve* uv = NULL;
            eu->GetOrCreateUVTrimCurve(uv);
            if (uv == NULL)
                return true;
            SmCurve* uv_copy = NULL;
            if (uv->Copy(ctx, uv_copy) != SM_SUCCESS)
                return false;
            SmBSplineCurve* uv_b = SM_CAST_PTR(SmBSplineCurve, uv_copy);
            if (!uv_b)
            {
                delete uv_copy;
                return false;
            }
            ouv.Add(uv_b);
            SmFace* f = eu->GetFace();
            if (f == NULL)
                return false;
            osrf.Add(f->GetSurface());
        }
    }
    // pad to 4 curves with point curves, then move the last entry to the front
    for (ULONG ii = 0; ii < 2; ii++)
    {
        if (o3d.GetSize() >= 4)
            break;
        double param = (orients[1] == SM_OT_SAME) ? 1.0 : 0.0;
        SmPoint3d pnt;
        if (o3d[1]->EvaluatePoint(param, pnt) != SM_SUCCESS)
            return false;
        SmBSplineCurve* top = NULL;
        if (SmBSplineCurve::CreatePointCurve(ctx, pnt, top) != SM_SUCCESS)
            return false;
        o3d.InsertAt(2 - ii, top);
        orients.InsertAt(2 - ii, SM_OT_SAME);
        ouv.InsertAt(2 - ii, (SmBSplineCurve*)NULL);
        osrf.InsertAt(2 - ii, (SmSurface*)NULL);
    }
    o3d.InsertAt(0, o3d.GetLast());
    o3d.RemoveLast();
    ouv.InsertAt(0, ouv.GetLast());
    ouv.RemoveLast();
    osrf.InsertAt(0, osrf.GetLast());
    osrf.RemoveLast();
    // SmBSplineSurface::CreateCornerBlend(.., dThisApproxTol3d, .., bCreateBilinearPatch = FALSE)
    ULONG n = o3d.GetSize();
    if (n < 4)
        return false;
    ULONG nulls = 0;
    for (ULONG ii = 0; ii < n; ii++)
    {
        if (ouv[ii] == NULL || osrf[ii] == NULL)
            nulls++;
        if (((n == 4 && nulls > 1) || (n > 4 && nulls > 0)) && (ouv[ii] == NULL || osrf[ii] == NULL))
            return false; // NER(crUVCurves[ii]) / NER(crSurfaces[ii])
    }
    for (ULONG ii = 0; ii < n; ii++)
    {
        SmBSplineCurve* nurb = SM_CAST_PTR(SmBSplineCurve, o3d[ii]);
        if (!nurb)
            return false; // NER(pNurb)
        if (!nurb->IsRational())
            continue; // a plain copy
        SmBSplineCurve* approx = NULL;
        if (nurb->ApproximateRationalCurve(ctx, tol / 2.0, approx) != SM_SUCCESS)
            return false;
        SmObjDelete approx_clean(approx);
        if (approx->ReparametrizeWithArcLength() != SM_SUCCESS)
            return false;
        if (osrf[ii])
        {
            SmExtent1d trim = approx->GetNaturalInterval();
            SmExtent2d dom = osrf[ii]->GetNaturalUVDomain();
            double max_dist, deviation;
            SmTArray<SmBSplineCurve*> dropped;
            SmObjsDelete<SmBSplineCurve*> dropped_clean(&dropped);
            if (osrf[ii]->DropCurve(ctx, dom, *approx, trim, tol / 2.0, max_dist, deviation, dropped) != SM_SUCCESS)
                return false;
            if (dropped.GetSize() == 0)
                return true; // UB-195
        }
    }
    reaches_tangents = n > 4;
    for (ULONG ii = 0; ii < n; ii++)
        if (osrf[ii] != NULL)
            reaches_tangents = true;
    return false;
}

// UB-157 (plan/upstream-issues.md): CreateSkinnedSurface reads and writes sIntParams[numSections + ii]
// past the array when SyncronizeKnotsOfCurves moves rail 2's natural interval. The probe of the
// surface.surface_p8 shim (OP_BLEND): 1007 for a moving rail 2 (the port's InvalidInput "too few
// intersection parameters"), otherwise 0 so native runs, including ordinary sampling failures.
static double blend_surface_ub157(const SmContext& ctx, const SmCurve& c1, const SmCurve& c2)
{
    SmCurve* k1 = NULL;
    SmCurve* k2 = NULL;
    bool moves = false;
    if (c1.Copy(ctx, k1) == SM_SUCCESS && c2.Copy(ctx, k2) == SM_SUCCESS && k1 != NULL && k2 != NULL)
    {
        SmBSplineCurve* b1 = (SmBSplineCurve*)k1;
        SmBSplineCurve* b2 = (SmBSplineCurve*)k2;
        SmExtent1d before = b2->GetNaturalInterval();
        SmTArray<SmBSplineCurve*> rails;
        rails.Add(b1);
        rails.Add(b2);
        if (SmBSplineCurve::SyncronizeKnotsOfCurves(rails, SM_EFF_ZERO * 100) == SM_SUCCESS)
        {
            SmExtent1d after = b2->GetNaturalInterval();
            moves = fabs(after.GetMin() - before.GetMin()) > SM_EFF_ZERO ||
                    fabs(after.GetMax() - before.GetMax()) > SM_EFF_ZERO;
        }
    }
    delete k1;
    delete k2;
    return moves ? 1007.0 : 0.0;
}

// SmBrep::CreateEdgeEdgeBlend (SmBrep.cpp:2850-3020) for a blend flag != 0 and no definition,
// replayed up to its CreateBlendSurface call on its own fresh rails (the edge uses are only read):
// the UB-157 probe of that call, 0 when upstream returns before it or the blend is defined.
// A refusal is a status the shim decides (the
// port's code for the same input): once the Rust side stops deferring, such a case is a guard
// agreement, not a compared C++ run.
static double edge_edge_blend_ub157(const SmContext& ctx, SmEdgeuse* e1, SmEdgeuse* e2, ULONG dir)
{
    SmEdge* edge1 = e1->GetEdge();
    SmEdge* edge2 = e2->GetEdge();
    if (edge1 == NULL || edge2 == NULL)
        return 0.0;
    SmExtent1d ivl1 = edge1->GetInterval();
    SmExtent1d ivl2 = edge2->GetInterval();
    SmBSplineCurve* rail1 = edge1->CreateTrimmedNURBSCurve(ctx);
    SmBSplineCurve* rail2 = edge2->CreateTrimmedNURBSCurve(ctx);
    SmObjDelete clean1(rail1), clean2(rail2);
    if (rail1 == NULL || rail2 == NULL)
        return 0.0;
    SmBoolean same;
    if (dir == 1)
        same = TRUE;
    else if (dir == 2)
        same = FALSE;
    else
    {
        ULONG ii, num(11);
        SmTArray<SmPoint3d> pos1, pos2;
        double d_same(0), d_opp(0);
        pos1.SetSize(num);
        pos2.SetSize(num);
        for (ii = 0; ii < num; ii++)
        {
            rail1->EvaluatePoint(ivl1.Evaluate((double)ii / (double)(num - 1)), pos1[ii]);
            rail2->EvaluatePoint(ivl2.Evaluate((double)ii / (double)(num - 1)), pos2[ii]);
        }
        for (ii = 0; ii < num; ii++)
        {
            d_same += pos1[ii].DistanceBetween(pos2[ii]);
            d_opp += pos1[ii].DistanceBetween(pos2[num - 1 - ii]);
        }
        same = (d_same <= d_opp) ? TRUE : FALSE;
    }
    if (e1->GetFace() == NULL || e2->GetFace() == NULL)
        return 0.0; // (the caller's null-dereference guard)
    SmSurface* surf1 = e1->GetFace()->GetSurface();
    SmSurface* surf2 = e2->GetFace()->GetSurface();
    if (surf1 == NULL || surf2 == NULL)
        return 0.0;
    auto rescale = [&]() {
        if (!ivl1.AreEqual(ivl2))
        {
            double scale = (ivl1.GetMin() - ivl1.GetMax()) / (ivl2.GetMin() - ivl2.GetMax());
            double offset = (ivl2.GetMin() * ivl1.GetMax()) -
                            (ivl1.GetMin() * ivl2.GetMax()) / (ivl2.GetMin() - ivl2.GetMax());
            SmExtent1d nat = rail2->GetNaturalInterval();
            nat.SetMinMax(nat.GetMin() * scale + offset, nat.GetMax() * scale + offset);
            rail2->EditParameterization(nat, FALSE);
            ivl2 = ivl1;
        }
    };
    if (same == FALSE)
    {
        rail2->ReverseParameterization(ivl2, ivl2);
    }
    rescale();
    if (same == FALSE)
        rail2->ReverseParameterization(ivl2, ivl2);
    rescale();
    return blend_surface_ub157(ctx, *rail1, *rail2);
}

// The healed B-rep (OP_HEAL): a fresh walk, the extras of OP_EXPORT, then the fields the
// Rust B-rep holds, in graph order (the order of the import, which the Rust side emits as
// its arena order; a heal that leaves the topology lists as they are keeps the two equal).
// Per region: in the B-rep; per shell: region; per face use: shell, face, mate,
// orientation, loop uses; per face: surface block, UV domain, tolerance, surface type;
// per loop use: face use, loop, mate, start kind (1 edge use, 2 vertex use, 0 other),
// start, orientation; per loop: loop use; per edge use: edge, owner kind (1 loop use,
// 2 shell, 0 other), owner, CW, CCW, vertex use, orientation, UV trim curve fingerprint,
// drop-curve fail (set, status, B-rep set), edge/face and CCW gap init flags; per edge:
// tolerance, interval, edge uses, curve type, curve fingerprint; per vertex use: vertex,
// owner kind (1 edge use, 2 loop use, 3 shell, 0 other), owner, vertex/face gap init;
// per vertex: point, tolerance; then the B-rep tolerance and validate-only flag.
void put_healed(SmBrep* brep, std::vector<double>& o)
{
    Graph g;
    walk(brep, g);
    walk_more(brep, g);
    put_extras(brep, g, o);
    for (auto n : {g.regions.size(), g.shells.size(), g.faceuses.size(), g.faces.size(), g.loopuses.size(),
                   g.loops.size(), g.edgeuses.size(), g.edges.size(), g.vertexuses.size(), g.vertices.size()})
        o.push_back((double)n);
    for (SmRegion* p : g.regions)
        o.push_back(p->GetOwner() == (SmOwningTopology*)brep ? 1.0 : 0.0);
    for (SmShell* p : g.shells)
        o.push_back((double)g.find(0, p->GetOwner()));
    for (SmFaceuse* p : g.faceuses)
    {
        o.push_back((double)g.find(1, p->GetOwner()));
        o.push_back((double)g.find(3, p->*FuPeek::face()));
        o.push_back((double)g.find(2, p->*FuPeek::mate()));
        o.push_back((double)p->GetOrientation());
        std::vector<int64_t> v;
        for (SmTopology* t : list_of(p->GetList()))
            v.push_back(g.find(4, t));
        put_list(o, v);
    }
    for (SmFace* p : g.faces)
    {
        put_surface(o, p->GetSurface());
        SmExtent2d d = p->GetUVDomain();
        o.push_back(d.GetMin().x);
        o.push_back(d.GetMin().y);
        o.push_back(d.GetMax().x);
        o.push_back(d.GetMax().y);
        o.push_back((double)p->GetTolerance());
        o.push_back(p->GetSurface() ? (double)p->GetSurface()->GetType() : -1.0);
    }
    for (SmLoopuse* p : g.loopuses)
    {
        o.push_back((double)g.find(2, p->GetOwner()));
        o.push_back((double)g.find(5, p->*LuPeek::loop()));
        o.push_back((double)g.find(4, p->*LuPeek::mate()));
        SM_TYPE ty = p->*LuPeek::type();
        SmTopology* start = p->*LuPeek::start();
        o.push_back(ty == SmEdgeuse_TYPE ? 1.0 : ty == SmVertexuse_TYPE ? 2.0 : 0.0);
        o.push_back((double)(ty == SmEdgeuse_TYPE ? g.find(6, start) : ty == SmVertexuse_TYPE ? g.find(8, start) : -1));
        o.push_back((double)(p->*LuPeek::orient()));
    }
    for (SmLoop* p : g.loops)
        o.push_back((double)g.find(4, p->GetLoopuse()));
    for (SmEdgeuse* p : g.edgeuses)
    {
        o.push_back((double)g.find(7, p->GetOwner()));
        SM_TYPE ty = p->GetEdgeuseType();
        SmTopology* sorlu = p->*EuPeek::sorlu();
        o.push_back(ty == SmLoopuse_TYPE ? 1.0 : ty == SmShell_TYPE ? 2.0 : 0.0);
        o.push_back((double)(ty == SmLoopuse_TYPE ? g.find(4, sorlu) : ty == SmShell_TYPE ? g.find(1, sorlu) : -1));
        o.push_back((double)g.find(6, p->GetCWEdgeuse()));
        o.push_back((double)g.find(6, p->GetCCWEdgeuse()));
        o.push_back((double)g.find(8, p->*EuPeek::vu()));
        o.push_back((double)p->GetOrientation());
        put_curve_id(o, p->*EuPeek::uv());
        SmDropCurveFail* fail = p->*EuPeek::fail();
        o.push_back(fail ? 1.0 : 0.0);
        o.push_back(fail ? (double)fail->m_sFailStatus : 0.0);
        o.push_back(fail && fail->m_pBrep ? 1.0 : 0.0);
        o.push_back(flg((p->*EuPeek::face_gap()).IsInit()));
        o.push_back(flg((p->*EuPeek::ccw_gap()).IsInit()));
    }
    for (SmEdge* p : g.edges)
    {
        o.push_back((double)p->GetTolerance());
        SmExtent1d ivl = p->GetInterval();
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
        std::vector<int64_t> v;
        for (SmTopology* t : list_of(p->GetList()))
            v.push_back(g.find(6, t));
        put_list(o, v);
        SmCurve* c = p->GetCurve();
        o.push_back(c ? (double)c->GetType() : -1.0);
        put_curve_id(o, SM_CAST_PTR(SmBSplineCurve, c));
    }
    for (SmVertexuse* p : g.vertexuses)
    {
        o.push_back((double)g.find(9, p->GetOwner()));
        SM_TYPE ty = p->GetVertexuseType();
        SmTopology* own = p->*VuPeek::owner();
        o.push_back(ty == SmEdgeuse_TYPE ? 1.0 : ty == SmLoopuse_TYPE ? 2.0 : ty == SmShell_TYPE ? 3.0 : 0.0);
        o.push_back((double)(ty == SmEdgeuse_TYPE   ? g.find(6, own)
                             : ty == SmLoopuse_TYPE ? g.find(4, own)
                             : ty == SmShell_TYPE   ? g.find(1, own)
                                                    : -1));
        o.push_back(flg((p->*VuPeek::face_gap()).IsInit()));
    }
    for (SmVertex* p : g.vertices)
    {
        put_vec(o, p->GetPoint());
        o.push_back((double)p->GetTolerance());
    }
    o.push_back((double)brep->GetTolerance());
    o.push_back(flg(brep->m_bValidateOnly));
}

void run_op(int32_t op, Rd& rd, const char* path, std::vector<double>& o)
{
    SmContext& ctx = const_cast<SmContext&>(the_context());
    CtxSave save(ctx);
    SmStatus bst;
    Built b;
    b.brep = build(rd, path, bst);
    if (b.brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(bst));
        return;
    }
    SmBrep* brep = b.brep;
    o.push_back(0.0);
    Graph g;
    walk(brep, g);
    walk_more(brep, g);
    switch (op)
    {
    case OP_EXPORT:
    {
        put_extras(brep, g, o);
        export_graph(brep, g, o);
        break;
    }
    case OP_LISTS:
    {
        int64_t which = rd.idx();
        int64_t attr = rd.idx();
        ULONG id = (ULONG)(attr < 0 ? 0 : attr);
        ULONG* pid = attr < 0 ? NULL : &id;
        attach(rd, g, attr);
        if (which == 7)
            for (SmFace* f : g.faces)
                if (f->GetSurface() == NULL) // GetSurfaces dereferences every face's surface
                {
                    o.push_back(ST_NULL_DEREF);
                    return;
                }
        if (which == 8)
            for (SmEdge* e : g.edges)
                if (e->GetCurve() == NULL) // GetCurves dereferences every edge's curve
                {
                    o.push_back(ST_NULL_DEREF);
                    return;
                }
        o.push_back(0.0);
        switch (which)
        {
        case 0: { SmTArray<SmRegion*> a; brep->GetRegions(a, pid); put_found(o, g, 0, a); break; }
        case 1: { SmTArray<SmFace*> a; brep->GetFaces(a, pid); put_found(o, g, 3, a); break; }
        case 2: { SmTArray<SmEdge*> a; brep->GetEdges(a, pid); put_found(o, g, 7, a); break; }
        case 3: { SmTArray<SmVertex*> a; brep->GetVertices(a, pid); put_found(o, g, 9, a); break; }
        case 4: { SmTArray<SmEdge*> a; brep->GetWireEdges(a, pid); put_found(o, g, 7, a); break; }
        case 5: { SmTArray<SmEdge*> a; brep->GetTopologicalEdges(a); put_found(o, g, 7, a); break; }
        case 6: { SmTArray<SmVertex*> a; brep->GetTopologicalVertices(a); put_found(o, g, 9, a); break; }
        case 7:
        {
            SmTArray<SmSurface*> a;
            brep->GetSurfaces(a, pid);
            std::vector<int64_t> v;
            for (ULONG i = 0; i < a.GetSize(); i++)
                v.push_back(g.find(3, (SmFace*)a[i]->GetOwner()));
            put_list(o, v);
            break;
        }
        case 8:
        {
            SmTArray<SmCurve*> a;
            brep->GetCurves(a, pid);
            std::vector<int64_t> v;
            for (ULONG i = 0; i < a.GetSize(); i++)
                v.push_back(g.find(7, (SmEdge*)a[i]->GetOwner()));
            put_list(o, v);
            break;
        }
        default: { SmTArray<SmShell*> a; brep->GetShells(a, pid); put_found(o, g, 1, a); break; }
        }
        break;
    }
    case OP_QUERY:
    {
        int64_t ty = rd.idx();
        int64_t i = rd.idx();
        SmTopology* t = target_of(g, ty, i);
        o.push_back((double)brep->GetNumFaces());
        o.push_back(flg(brep->IsLiveTopologyMember(t)));
        o.push_back((double)brep->GetTolerance());
        o.push_back(brep_est(brep, FALSE));
        o.push_back(brep_est(brep, TRUE));
        o.push_back(brep_ratio(brep, FALSE));
        o.push_back(brep_ratio(brep, TRUE));
        o.push_back((double)g.find(0, brep->GetInfiniteRegion()));
        o.push_back((double)brep->GetType());
        for (size_t k = 0; k < N_KIND_TYPES; k++)
            o.push_back(flg(brep->IsKindOf(KIND_TYPES[k])));
        o.push_back((double)std::strlen(brep->GetTypeString()));
        o.push_back((double)std::strlen(brep->GetClassString()));
        o.push_back(flg(brep->m_bEditingEnabled));
        o.push_back(flg(brep->m_bValidateOnly));
        break;
    }
    case OP_SET_SIZE:
    {
        int64_t kind = rd.idx();
        double value = rd.num();
        if (kind == 0)
        {
            SmZoneTol3d r = (brep->*BrPeek::set_est())(value);
            o.push_back((double)r);
            o.push_back((double)brep->GetTolerance());
            o.push_back(brep_est(brep, FALSE));
            o.push_back(brep_est(brep, TRUE));
            o.push_back(ctx_est(ctx));
            o.push_back((double)(ctx.*CtxPeek::tol()));
        }
        else
        {
            (brep->*BrPeek::set_ratio())(value);
            o.push_back(brep_ratio(brep, FALSE));
            o.push_back(brep_ratio(brep, TRUE));
            o.push_back(ctx_ratio(ctx));
        }
        break;
    }
    case OP_SET_INF:
    {
        int64_t set_idx = rd.idx();
        int64_t mode = rd.idx();
        int64_t reset_idx = rd.idx();
        int64_t flag = rd.idx();
        int64_t unsure_idx = rd.idx();
        auto region_at = [&](int64_t i) -> SmRegion* {
            return i >= 0 && i < (int64_t)g.regions.size() ? g.regions[(size_t)i] : nullptr;
        };
        // SmRegion::SetIsVoid(UNSURE) on the chosen region first
        if (region_at(unsure_idx) != nullptr)
            region_at(unsure_idx)->SetIsVoid((SmBoolean)2);
        if (set_idx != -2)
            brep->SetInfiniteRegion(region_at(set_idx));
        // GUARD: ResetInfiniteRegion(region, SM_VS_INFINITEREGION_SWAP) dereferences the old
        // infinite region (SmBrep.cpp:8630) after the membership check passed
        if (mode == 1 && flag == 2 && region_at(reset_idx) != nullptr && brep->GetInfiniteRegion() == nullptr)
        {
            SmTArray<SmRegion*> regs;
            brep->GetRegions(regs);
            ULONG at;
            if (regs.FindElement(region_at(reset_idx), at))
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
        }
        SmStatus s = SM_SUCCESS;
        if (mode == 1)
            s = brep->ResetInfiniteRegion(region_at(reset_idx), (SmSetRegionVoidType)flag);
        else if (mode == 2)
            s = brep->SetRegionIsVoidFlagsForNestedSolids();
        o.push_back(st(s));
        o.push_back((double)g.find(0, brep->GetInfiniteRegion()));
        put_regions(brep, g, o);
        if (mode != 0 && s == SM_SUCCESS)
        {
            std::vector<int64_t> v;
            for (SmRegion* r : g.regions)
                v.push_back(r->IsVoid());
            put_list(o, v);
        }
        break;
    }
    case OP_DEGENERATE:
    {
        double tol = rd.num();
        o.push_back(flg(brep->IsDegenerate(tol)));
        break;
    }
    case OP_ADD_DELETED:
    {
        int64_t n = rd.idx();
        for (int64_t k = 0; k < n; k++)
            brep->AddDeletedSurface(new (ctx) SmPlane(SmPoint3d(0, 0, (double)k), SmVector3d(0, 0, 1), &ctx));
        SmTArray<SmSurface*>* del = brep->*BrPeek::deleted();
        o.push_back(del ? (double)del->GetSize() : 0.0);
        break;
    }
    case OP_VOTE:
    {
        int64_t n = rd.idx();
        SmTArray<SmRegion*> regs;
        SmTArray<ULONG> votes;
        for (int64_t k = 0; k < n; k++)
        {
            int64_t i = rd.idx();
            SmRegion* r = i >= 0 && i < (int64_t)g.regions.size() ? g.regions[(size_t)i] : nullptr;
            sc_brep_p1::sm_VoteForRegion(regs, votes, r);
        }
        put_found(o, g, 0, regs);
        std::vector<int64_t> v;
        for (ULONG k = 0; k < votes.GetSize(); k++)
            v.push_back((int64_t)votes[k]);
        put_list(o, v);
        o.push_back((double)g.find(0, sc_brep_p1::sm_FindHighestVote(regs, votes)));
        break;
    }
    case OP_OBJMAP:
    {
        // SmObject-derived maps are made through the context's operator new (a stack
        // SmMapPtrToPtr crashed in this harness)
        SmMapPtrToPtr<SmTopology, SmTopology>* pmap = new (ctx) SmMapPtrToPtr<SmTopology, SmTopology>(&ctx);
        SmCopyBrepMap* pcopy = new (ctx) SmCopyBrepMap();
        SmCopyBrepMap* prev = new (ctx) SmCopyBrepMap();
        SmObjDelete clean_map(pmap), clean_copy(pcopy), clean_rev(prev);
        SmMapPtrToPtr<SmTopology, SmTopology>& map = *pmap;
        SmCopyBrepMap& copy_map = *pcopy;
        SmCopyBrepMap& rev_map = *prev;
        // SmCopyBrepMap::SetAt dereferences a null copy and GetAt its (null) result on a
        // miss (UB-98): the copy maps only see non-null copies and are only asked for
        // keys that were stored
        std::set<const void*> copy_keys, rev_keys;
        int64_t n_set = rd.idx();
        for (int64_t k = 0; k < n_set; k++)
        {
            int64_t oty = rd.idx(), oi = rd.idx(), cty = rd.idx(), ci = rd.idx();
            bool use_copy = rd.flag(), use_rev = rd.flag();
            SmTopology* orig = target_of(g, oty, oi);
            SmTopology* copy = target_of(g, cty, ci);
            if (orig == nullptr)
                continue; // the Rust side keys by entity: a null original is skipped
            sc_brep_p1::sm_SetObjectMap(orig, copy, map, use_copy && copy ? &copy_map : NULL,
                                        use_rev && copy ? &rev_map : NULL);
            if (use_copy && copy != nullptr)
                copy_keys.insert(copy);
            if (use_rev && copy != nullptr)
                rev_keys.insert(orig);
        }
        int64_t n_get = rd.idx();
        std::vector<int64_t> v;
        for (int64_t k = 0; k < n_get; k++)
        {
            int64_t ty = rd.idx(), i = rd.idx();
            SmTopology* from = target_of(g, ty, i);
            SmTopology* to = from ? sc_brep_p1::sm_GetObjectMap(from, map) : nullptr;
            v.push_back(to ? (int64_t)to->GetType() : -1);
            v.push_back(find_any(g, to));
            SmTopology* c = from && copy_keys.count(from) ? copy_map.GetAt(from) : nullptr;
            v.push_back(find_any(g, c));
            SmTopology* r = from && rev_keys.count(from) ? rev_map.GetAt(from) : nullptr;
            v.push_back(find_any(g, r));
        }
        put_list(o, v);
        break;
    }
    case OP_ADD_ATTRS:
    {
        int64_t ty = rd.idx(), i = rd.idx();
        SmTopology* t = target_of(g, ty, i);
        int64_t n_attrs = rd.idx();
        SmTArray<SmAttribute*> attrs;
        for (int64_t k = 0; k < n_attrs; k++)
            attrs.Add(new (ctx) SmAttribute((ULONG)rd.idx(), SM_AB_COPY));
        int64_t n_idx = rd.idx();
        SmTArray<ULONG> indexed;
        for (int64_t k = 0; k < n_idx; k++)
            indexed.Add((ULONG)rd.idx());
        if (t == nullptr)
        {
            // GUARD: pObject->AddAttribute dereferences the null object once per index;
            // with no index upstream returns SM_SUCCESS without touching it
            if (indexed.GetSize() == 0)
            {
                o.push_back(0.0);
                put_list(o, std::vector<int64_t>());
            }
            else
                o.push_back(ST_NULL_DEREF);
            for (ULONG k = 0; k < attrs.GetSize(); k++)
                delete attrs[k];
            break;
        }
        SmStatus s = sc_brep_p1::sm_AddAttributes(t, indexed, attrs);
        o.push_back(st(s));
        SmTArray<SmAttribute*> got;
        t->GetAttributes(got);
        std::vector<int64_t> v;
        for (ULONG k = 0; k < got.GetSize(); k++)
            v.push_back((int64_t)got[k]->GetAttributeID());
        put_list(o, v);
        // unattached attributes would leak; the attached ones belong to the object now
        for (ULONG k = 0; k < attrs.GetSize(); k++)
        {
            bool attached = false;
            for (ULONG m = 0; m < got.GetSize(); m++)
                attached = attached || got[m] == attrs[k];
            if (!attached)
                delete attrs[k];
        }
        break;
    }
    case OP_REFLECT_EU:
    {
        SmEdge* e = g.edges[(size_t)rd.idx()];
        SmTArray<SmEdgeuse*> uses;
        e->GetEdgeuses(uses);
        if (uses.GetSize() % 2 != 0)
        {
            o.push_back(ST_NULL_DEREF);
            break;
        }
        sc_brep_p1::sm_ReverseReflectionEdgeuses(*e);
        o.push_back(0.0);
        SmTArray<SmEdgeuse*> after;
        e->GetEdgeuses(after);
        put_found(o, g, 6, after);
        break;
    }
    case OP_REFLECT_SHELL:
    {
        SmFace* f = g.faces[(size_t)rd.idx()];
        SmStatus s = sc_brep_p1::sm_SwapReflectionShells(*f);
        o.push_back(st(s));
        if (s != SM_SUCCESS)
            break;
        SmFaceuse *first, *second;
        f->GetFaceuses(first, second);
        o.push_back((double)g.find(1, first->GetShell()));
        o.push_back((double)g.find(1, second->GetShell()));
        std::vector<int64_t> v;
        for (SmShell* s2 : g.shells)
            v.push_back(s2->*ShPeek::type() == SmFaceuse_TYPE ? (int64_t)list_of(s2->GetList()).size() : -1);
        put_list(o, v);
        // Owner pointers alone cannot detect stale shell membership after reflection.
        for (SmShell* s2 : g.shells)
        {
            SmTArray<SmFaceuse*> members;
            s2->GetFaceuses(members);
            put_found(o, g, 2, members);
            for (ULONG i = 0; i < members.GetSize(); ++i)
            {
                o.push_back((double)g.find(2, members[i]->GetNext()));
                o.push_back((double)g.find(2, members[i]->GetLast()));
            }
        }
        break;
    }
    case OP_RM_LOOP_VU:
    {
        SmVertexuse* vu = g.vertexuses[(size_t)rd.idx()];
        SmVertex* v = (SmVertex*)vu->GetOwner();
        // the loop uses whose start vertex use may be deleted: their dangling pointer is
        // cleared afterwards so the B-rep can be destroyed (not part of the measured state)
        std::vector<SmLoopuse*> owners;
        for (SmLoopuse* lu : g.loopuses)
            if (lu->*LuPeek::type() == SmVertexuse_TYPE)
                owners.push_back(lu);
        std::vector<SmVertexuse*> before;
        if (v != NULL)
            for (SmTopology* t : list_of(v->GetList()))
                before.push_back((SmVertexuse*)t);
        SmStatus s = sc_brep_p1::sm_RmAndDel_OldLoopVertices(vu);
        o.push_back(st(s));
        if (s == SM_ST_NULL_DEREF)
            break;
        o.push_back(v ? (double)v->GetSize() : -1.0);
        if (s == SM_SUCCESS && v != NULL)
        {
            // the remaining vertex uses of the vertex, in list order
            std::vector<SmTopology*> left = list_of(v->GetList());
            std::vector<int64_t> left_ix;
            for (SmTopology* t : left)
                left_ix.push_back(g.find(8, t));
            put_list(o, left_ix);
            // a vertex use of the vertex that is no longer on its list was deleted
            for (SmLoopuse* lu : owners)
            {
                SmTopology* start = lu->*LuPeek::start();
                bool was_ours = false, alive = false;
                for (SmVertexuse* p : before)
                    was_ours = was_ours || p == start;
                for (SmTopology* t : left)
                    alive = alive || t == start;
                if (was_ours && !alive)
                    lu->*LuPeek::start() = nullptr;
            }
        }
        break;
    }
    case OP_MARK_LOOPS:
    {
        SmLoop* l = g.loops[(size_t)rd.idx()];
        ULONG level = (ULONG)rd.idx();
        SmTArray<SmLoop*> loops;
        SmStatus s = sc_brep_p1::sm_MarkLoopsToDelete(loops, l, level);
        o.push_back(st(s));
        if (s == SM_ST_NULL_DEREF)
            break;
        put_found(o, g, 5, loops);
        break;
    }
    case OP_SWAP_FACE_UV:
    {
        SmFace* f = g.faces[(size_t)rd.idx()];
        SmMarkType mark = (SmMarkType)rd.idx();
        bool premark = rd.flag();
        if (premark)
            f->Mark(mark);
        bool swaps = !f->IsMarked(mark);
        // SmFace::SwapUV dereferences the face's surface and upward face use
        if (swaps && (f->GetSurface() == NULL || f->GetUpwardFaceuse() == NULL))
        {
            o.push_back(ST_NULL_DEREF);
            return;
        }
        sc_brep_p1::sm_SwapFaceUV(f, mark);
        o.push_back(0.0);
        o.push_back(flg(f->IsMarked(mark)));
        SmExtent2d d = f->GetUVDomain();
        o.push_back(d.GetMin().x);
        o.push_back(d.GetMin().y);
        o.push_back(d.GetMax().x);
        o.push_back(d.GetMax().y);
        // an unmarked face was swapped (surface, trim curves, loop uses): the B-rep after it
        if (swaps)
            put_healed(brep, o);
        break;
    }
    case OP_CHECK_UV:
    {
        SmEdgeuse* eu = g.edgeuses[(size_t)rd.idx()];
        o.push_back(st(sc_brep_p1::sm_CheckUVTrimCurve(eu)));
        break;
    }
    case OP_LOCK_BOX:
    {
        {
            SmLockBrepBox lock(brep);
        }
        o.push_back(0.0);
        break;
    }
    case OP_FIND_SIDE:
    {
        SmBSplineCurve* c = read_curve(rd, ctx);
        SmObjDelete clean(c);
        SmPoint3d p = rd.vec();
        SmVector3d dir = rd.vec();
        double tol = rd.num();
        SmBoolean right = UNSURE;
        SmStatus s = sc_brep_p1::sm_FindSideOfCurve(*c, p, dir, tol, right);
        o.push_back(st(s));
        o.push_back((double)right);
        break;
    }
    case OP_FIND_SECTOR:
    {
        SmFace* f = g.faces[(size_t)rd.idx()];
        bool has_guess = rd.flag();
        SmPoint2d guess = rd.uv();
        SmPoint3d test_point = rd.vec();
        SmVector3d test_dir = rd.vec();
        double dist = 0.0;
        SmRegion* region = nullptr;
        SmStatus s = sc_brep_p1::sm_FindSectorOfFace(f, has_guess ? &guess : NULL, test_point, test_dir, dist, region);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(dist);
            o.push_back((double)g.find(0, region));
        }
        break;
    }
    case OP_UPDATE_IVLS:
    {
        SmVertex* v = g.vertices[(size_t)rd.idx()];
        SmTArray<SmEdge*> edges;
        v->GetEdges(edges);
        for (ULONG i = 0; i < edges.GetSize(); i++)
            if (edges[i]->GetCurve() == NULL)
            {
                o.push_back(ST_NULL_DEREF);
                return;
            }
        SmStatus s = brep->UpdateVertexEdgeIntervals(v);
        o.push_back(st(s));
        o.push_back((double)v->GetTolerance());
        o.push_back((double)edges.GetSize());
        for (ULONG i = 0; i < edges.GetSize(); i++)
        {
            SmExtent1d ivl = edges[i]->GetInterval();
            o.push_back(ivl.GetMin());
            o.push_back(ivl.GetMax());
            o.push_back((double)edges[i]->GetTolerance());
        }
        break;
    }
    case OP_REDISTRIBUTE:
    {
        SmEdge* e = g.edges[(size_t)rd.idx()];
        bool has_uv = rd.flag();
        SmSurfParamType seam = (SmSurfParamType)rd.idx();
        SmPoint2d a = rd.uv(), b2 = rd.uv();
        SmExtent2d dom(a, b2);
        SmTArray<SmEdgeuse*> eus;
        e->GetEdgeuses(eus);
        SmBSplineCurve* uv = NULL;
        SmLine stand_in(SmPoint3d(0, 0, 0), SmPoint3d(1, 0, 0), 2, &ctx);
        if (has_uv)
            uv = &stand_in; // the function only tests the trim curve for NULL
        if (has_uv && seam != SM_SP_NEITHER && eus.GetSize() < 4)
        {
            // GUARD (UB-162): the seam branch reads sNewEdgeuses[3] of a 16-slot stack
            // array without checking the count (SM_ASSERT is a release no-op)
            o.push_back(ST_NULL_DEREF);
            break;
        }
        SmBoolean swapped = sm_RedistributeSeamUVTrimCurves(e, uv, seam, dom);
        o.push_back(0.0);
        o.push_back(flg(swapped));
        break;
    }
    case OP_SPLIT_UV:
    {
        SmEdgeuse* eu_orig = g.edgeuses[(size_t)rd.idx()];
        SmEdgeuse* eu_new = g.edgeuses[(size_t)rd.idx()];
        SmPoint3d split = rd.vec();
        double param = rd.num();
        double scaled_zero = rd.num();
        SmOrientType orient = (SmOrientType)rd.idx();
        bool for_tess = rd.flag();
        // GUARD: the edge is dereferenced (GetTolerance, SmBrep.cpp:21092) only in the
        // "split param is far from UVTrimCurve->Ends" branch (a NULL face or a failing
        // EvaluatePoint would return NER/SER before it: unreachable with imported B-reps,
        // where every edge use has an edge, so not distinguished here)
        if (eu_orig->GetUVTrimCurvePointer() != NULL && eu_orig->GetEdge() == NULL)
        {
            SmExtent1d ivl = eu_orig->GetUVTrimCurvePointer()->GetNaturalInterval();
            if (!(param < ivl.GetMin() + scaled_zero || param > ivl.GetMax() - scaled_zero))
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
        }
        SmStatus s = sm_MakeVertexSplitUVTrimCurve(eu_orig, eu_new, split, param, scaled_zero, orient, for_tess ? TRUE : FALSE);
        o.push_back(st(s));
        put_eu_state(o, g, eu_orig);
        put_eu_state(o, g, eu_new);
        o.push_back(eu_orig->GetEdge() ? (double)eu_orig->GetEdge()->GetTolerance() : -1.0);
        o.push_back(eu_new->GetEdge() ? (double)eu_new->GetEdge()->GetTolerance() : -1.0);
        break;
    }
    case OP_BREP_INTERSECT:
    {
        double tol = rd.num();
        double ang = rd.num();
        bool want_points = rd.flag();
        SmTArray<SmCurve*> curves;
        SmTArray<SmPoint3d> points;
        SmStatus s = brep->BrepIntersect(ctx, brep, tol, ang, curves, want_points ? &points : NULL);
        o.push_back(st(s));
        o.push_back((double)curves.GetSize());
        o.push_back(want_points ? (double)points.GetSize() : -1.0);
        for (ULONG i = 0; i < curves.GetSize(); i++)
            delete curves[i];
        break;
    }
    case OP_CORNER_BLEND:
    {
        int64_t n = rd.idx();
        SmTArray<SmEdgeuse*> eus;
        for (int64_t i = 0; i < n; i++)
            eus.Add(g.edgeuses[(size_t)rd.idx()]);
        bool has_tan = rd.flag();
        int64_t nt = rd.idx();
        SmTArray<SmBoolean> tans;
        for (int64_t i = 0; i < nt; i++)
            tans.Add(rd.bflag());
        double tol = rd.num();
        double ang = rd.num();
        if (eus.GetSize() >= 2)
        {
            // upstream dereferences the mate of an opposite edge use and each edge, indexes
            // the tangent flags per edge use, and copies the UV trim curve of each tangent
            // edge use unchecked: -2 there (the port's InvalidInput)
            bool null_deref = has_tan && tans.GetSize() < eus.GetSize();
            for (ULONG i = 0; i < eus.GetSize(); i++)
            {
                SmEdgeuse* p = eus[i];
                if (p->GetOrientation() == SM_OT_OPPOSITE)
                    p = p->GetMate();
                if (p == NULL || p->GetEdge() == NULL)
                    null_deref = true;
            }
            bool reaches_tangents = false;
            if (!null_deref && corner_blend_refused(brep, ctx, eus, has_tan ? &tans : NULL, tol, reaches_tangents))
                null_deref = true;
            if (null_deref)
            {
                o.push_back(ST_NULL_DEREF);
                o.push_back(0.0); // upstream resets the output array first
                break;
            }
            // HANG-39: a tangency tolerance below what doubles resolve makes the cross-tangent
            // approximation and the Coons knot merge run for tens of seconds or more; the case
            // is rejected only where upstream reaches that approximation
            if (reaches_tangents && ang < 1e-20)
            {
                o.push_back(ST_HANG34_GUARD);
                break;
            }
        }
        SmTArray<SmSurface*> blends;
        SmStatus s = brep->CreateCornerBlend(ctx, eus, has_tan ? &tans : NULL, tol, ang, blends);
        o.push_back(st(s));
        o.push_back((double)blends.GetSize());
        for (ULONG i = 0; i < blends.GetSize(); i++)
            put_surface(o, blends[i]);
        for (ULONG i = 0; i < blends.GetSize(); i++)
            delete blends[i];
        break;
    }
    case OP_EDGE_EDGE_BLEND:
    {
        int64_t flag = rd.idx();
        int64_t i1 = rd.idx(), i2 = rd.idx();
        SmEdgeuse* e1 = i1 >= 0 ? g.edgeuses[(size_t)i1] : nullptr;
        SmEdgeuse* e2 = i2 >= 0 ? g.edgeuses[(size_t)i2] : nullptr;
        int64_t dir = rd.idx();
        if (flag != 0 && e1 != nullptr && e2 != nullptr && e1->GetEdge() != NULL && e2->GetEdge() != NULL)
        {
            // the blend path: after the rails (CreateTrimmedNURBSCurve, failing -> SM_ERR_INVALID_INPUT
            // upstream and in the port) it dereferences both faces: -2 there
            SmBSplineCurve* r1 = e1->GetEdge()->CreateTrimmedNURBSCurve(ctx);
            SmBSplineCurve* r2 = e2->GetEdge()->CreateTrimmedNURBSCurve(ctx);
            bool rails_ok = r1 != NULL && r2 != NULL;
            delete r1;
            delete r2;
            if (rails_ok && (e1->GetFace() == NULL || e2->GetFace() == NULL))
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            double ub157 = edge_edge_blend_ub157(ctx, e1, e2, (ULONG)dir);
            if (ub157 != 0.0)
            {
                o.push_back(ub157);
                break;
            }
        }
        SmSurface* out = nullptr;
        SmStatus s = SmBrep::CreateEdgeEdgeBlend(ctx, (ULONG)flag, e1, e2, (ULONG)dir, NULL, out);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_surface(o, out);
        delete out;
        break;
    }
    case OP_EDGE_EDGE_BLEND2:
    {
        // the static SmBrep::CreateEdgeEdgeBlend with the second edge use in another B-rep
        // (ruled; the FFI checks the second block builds and both indices)
        SmStatus bst2;
        Built b2;
        b2.brep = build(rd, "", bst2);
        int64_t i1 = rd.idx(), i2 = rd.idx();
        int64_t dir = rd.idx();
        if (b2.brep == nullptr)
        {
            o.push_back(st(bst2));
            break;
        }
        Graph g2;
        walk(b2.brep, g2);
        walk_more(b2.brep, g2);
        SmSurface* out = nullptr;
        SmStatus s = SmBrep::CreateEdgeEdgeBlend(ctx, 0, g.edgeuses[(size_t)i1], g2.edgeuses[(size_t)i2],
                                                 (ULONG)dir, NULL, out);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_surface(o, out);
        delete out;
        break;
    }
    case OP_PARALLEL_PROJ:
    {
        SmBSplineCurve* c = read_curve(rd, ctx);
        SmObjDelete clean(c);
        SmVector3d vec = rd.vec();
        bool has_dist = rd.flag();
        double dist = rd.num();
        bool has_tol = rd.flag();
        double tol = rd.num();
        bool has_ang = rd.flag();
        double ang = rd.num();
        bool want_3d = rd.flag(), want_uv = rd.flag(), want_faces = rd.flag();
        for (SmFace* f : g.faces)
            if (f->GetSurface() == NULL)
            {
                o.push_back(ST_NULL_DEREF);
                return;
            }
        SmApproxTol3d atol(tol);
        SmTArray<SmCurve*> c3d, cuv;
        SmTArray<SmFace*> faces;
        SmStatus s = brep->CreateParallelProjectionCurves(ctx, *c, vec, has_dist ? &dist : NULL, has_tol ? &atol : NULL,
                                                          has_ang ? &ang : NULL, want_3d ? &c3d : NULL,
                                                          want_uv ? &cuv : NULL, want_faces ? &faces : NULL, NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(want_3d ? (double)c3d.GetSize() : -1.0);
            for (ULONG i = 0; i < c3d.GetSize(); i++)
                put_curve_id(o, SM_CAST_PTR(SmBSplineCurve, c3d[i]));
            o.push_back(want_uv ? (double)cuv.GetSize() : -1.0);
            for (ULONG i = 0; i < cuv.GetSize(); i++)
                put_curve_id(o, SM_CAST_PTR(SmBSplineCurve, cuv[i]));
            o.push_back(want_faces ? (double)faces.GetSize() : -1.0);
            for (ULONG i = 0; i < faces.GetSize(); i++)
                o.push_back((double)g.find(3, faces[i]));
        }
        for (ULONG i = 0; i < c3d.GetSize(); i++)
            delete c3d[i];
        for (ULONG i = 0; i < cuv.GetSize(); i++)
            delete cuv[i];
        break;
    }
    case OP_HEAL:
    {
        SmHealerOpType op_code = (SmHealerOpType)rd.idx();
        bool make_uv = rd.flag();
        // SmBrep::HealBrep ignores the healer's status and returns SM_SUCCESS whatever the
        // healer did, so the healed B-rep is the result; the Rust side runs the same
        // healer and defers at its first unported step
        SmStatus s = brep->HealBrep(op_code, NULL, NULL, NULL, make_uv ? TRUE : FALSE);
        o.push_back(st(s));
        put_healed(brep, o);
        break;
    }
    default:
        break;
    }
}
} // namespace

SC_EXPORT int32_t sc_topology_brep_p1_eval(int32_t op, const char* path, const double* in, int64_t n_in, double* out,
                                           int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    run_op(op, rd, path, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
