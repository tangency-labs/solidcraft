// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surface_cache_p2`: the SmSurfaceCache members owned by the unit
// (source/SMLib/src/SmSurfaceCache.cpp): SmPatchBoundaryBoundingPlanes::GetNthBoundingPlane,
// SmSurfaceCache::AddCurvesToCache, TestAgainstTolerances, SubdivideByBlock and the destructor,
// plus the file-static sm_CreateBezierPatchBlock / sm_CreateApproximateBezierPatchBlock.
//
// Static functions cannot be linked against, so they (and the static sm_ComputeBezierApproximation
// they call) are VERBATIM copies of upstream SmSurfaceCache.cpp (commit c9c5979), inside the
// namespace sc_ssc_p2 (line ranges noted at each block; generated, do not edit by hand). Protected
// members are reached through derived "peek" classes; subdivision nodes are built by hand in the
// tree's own node manager, in the cache's patch / aux managers, as upstream's tree construction
// (unit surface.surface_cache_p3) does.
//
// One entry point, sc_surface_surface_cache_p2_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/surface_surface_cache_p2.rs, which validates every
// input. `out` receives the op status (0 = SM_SUCCESS, else the SmStatus value) and the op's
// outputs. Every cache built here is deleted (~SmSurfaceCache) before the op returns.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveBoundedSurface.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmMemBlockMgr.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmPseudoBox.h>
#include <SmSurface.h>
#include <SmSurfaceCache.h>
#include <SmTArray.h>
#include <SmTree.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

#include "sc_sm.h"

using sc::flg;
using sc::put_dom;
using sc::put_vec;
using sc::st;

namespace sc_ssc_p2
{
// clang-format off
// ---- verbatim: SmSurfaceCache.cpp lines 679-911: sm_ComputeBezierApproximation
static SmStatus sm_ComputeBezierApproximation
  (const SmSurface  & crSurface,      // in :      
   const SmExtent2d & crUVDomain,     // in :      
   gw_SURFACE * pSurface)             // out: preallocated surface whose values get set
{
    SmVector3d sMat[2][2];
    gw_CPOINT **Pw  = pSurface->net->Pw;
    gw_KNOTVECTOR *pKNOTU = pSurface->knu;
    gw_KNOTVECTOR *pKNOTV = pSurface->knv;

    // Load knot vectors with domain bounds
    pKNOTU->m = 7;
    pKNOTV->m = 7;
    for (ULONG i=0; i<4; i++) {
        pKNOTU->U[i] = crUVDomain.GetMin().x;
        pKNOTV->U[i] = crUVDomain.GetMin().y;
        pKNOTU->U[i+4] = crUVDomain.GetMax().x;
        pKNOTV->U[i+4] = crUVDomain.GetMax().y;
    }

    SmPoint2d sSize = crUVDomain.GetSize();
    SmSurfParamType eSingDir;
    SmVector3d sDU, sDV, sDUV;

    // Now load control polygon at corners using hermite information.
    {
        SER(crSurface.Evaluate(crUVDomain.GetMin(),1,1,TRUE,TRUE,FALSE,sMat[0]));
        SmPoint3d sP00 = sMat[0][0];
        if (crSurface.IsSingularity(crUVDomain.GetMin(),eSingDir)) {
            SmPoint2d sUV = crUVDomain.GetMin();
            SmPoint2d sUVU(sUV.x+(sSize.x*SM_EFF_ZERO_SQRT),sUV.y);
            SmPoint2d sUVV(sUV.x,sUV.y+(sSize.y*SM_EFF_ZERO_SQRT));
            SER(crSurface.Evaluate(sUVU,1,1,TRUE,TRUE,FALSE,sMat[0]));
            sDU = sMat[1][0]*sSize.x;
            sDUV = sMat[1][1]*sSize.x*sSize.y;
            SER(crSurface.Evaluate(sUVV,1,1,TRUE,TRUE,FALSE,sMat[0]));
            sDV = sMat[0][1]*sSize.y;
            sDUV = (sDUV + sMat[1][1]*sSize.x*sSize.y)/2.0;
        }
        else {
            sDU = sMat[1][0]*sSize.x;
            sDV = sMat[0][1]*sSize.y;
            sDUV = sMat[1][1]*sSize.x*sSize.y;
        }
        COPY_XYZ(sP00,Pw[0][0]);
        SmPoint3d sP10 = sP00 + sDU/3.0;
        COPY_XYZ(sP10,Pw[1][0]);
        SmPoint3d sP01 = sP00 + sDV/3.0;
        COPY_XYZ(sP01,Pw[0][1]);
        SmPoint3d sP11 = sP10 + sP01 - sP00 + sDUV/9.0;
        COPY_XYZ(sP11,Pw[1][1]);
#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
        if (bDebugMe) {
            smgfx_Erase();
            smgfx_SetColor(1,0,0);
            sP00.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,0,1);
            sP10.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,1,1);
            sP01.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,0,0);
            sP11.Draw();
            sm_GraphicsLoop();
        }
#endif

    }

    {
        SER(crSurface.Evaluate(crUVDomain.GetMax(),1,1,TRUE,TRUE,FALSE,sMat[0]));
        SmPoint3d sP00 = sMat[0][0];
        if (crSurface.IsSingularity(crUVDomain.GetMax(),eSingDir)) {
            SmPoint2d sUV = crUVDomain.GetMax();
            SmPoint2d sUVU(sUV.x-(sSize.x*SM_EFF_ZERO_SQRT),sUV.y);
            SmPoint2d sUVV(sUV.x,sUV.y-(sSize.y*SM_EFF_ZERO_SQRT));
            SER(crSurface.Evaluate(sUVU,1,1,TRUE,TRUE,FALSE,sMat[0]));
            sDU = sMat[1][0]*sSize.x;
            sDUV = sMat[1][1]*sSize.x*sSize.y;
            SER(crSurface.Evaluate(sUVV,1,1,TRUE,TRUE,FALSE,sMat[0]));
            sDV = sMat[0][1]*sSize.y;
            sDUV = (sDUV + sMat[1][1]*sSize.x*sSize.y)/2.0;
        }
        else {
            sDU = sMat[1][0]*sSize.x;
            sDV = sMat[0][1]*sSize.y;
            sDUV = sMat[1][1]*sSize.x*sSize.y;
        }
        COPY_XYZ(sP00,Pw[3][3]);
        SmPoint3d sP10 = sP00 - sDU/3.0;
        COPY_XYZ(sP10,Pw[2][3]);
        SmPoint3d sP01 = sP00 - sDV/3.0;
        COPY_XYZ(sP01,Pw[3][2]);
        SmPoint3d sP11 = sP10 + sP01 - sP00 + sDUV/9.0;
        COPY_XYZ(sP11,Pw[2][2]);
#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
        if (bDebugMe) {
            smgfx_Erase();
            smgfx_SetColor(1,0,0);
            sP00.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,0,1);
            sP10.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,1,1);
            sP01.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,0,0);
            sP11.Draw();
            sm_GraphicsLoop();
        }
#endif
    }

    {
        SER(crSurface.Evaluate(crUVDomain.Evaluate(1.0,0.0),1,1,TRUE,TRUE,FALSE,sMat[0]));
        SmPoint3d sP00 = sMat[0][0];
        if (crSurface.IsSingularity(crUVDomain.Evaluate(1.0,0.0),eSingDir)) {
            SmPoint2d sUV = crUVDomain.Evaluate(1.0,0.0);
            SmPoint2d sUVU(sUV.x-(sSize.x*SM_EFF_ZERO_SQRT),sUV.y);
            SmPoint2d sUVV(sUV.x,sUV.y+(sSize.y*SM_EFF_ZERO_SQRT));
            SER(crSurface.Evaluate(sUVU,1,1,TRUE,TRUE,FALSE,sMat[0]));
            sDU = sMat[1][0]*sSize.x;
            sDUV = sMat[1][1]*sSize.x*sSize.y;
            SER(crSurface.Evaluate(sUVV,1,1,TRUE,TRUE,FALSE,sMat[0]));
            sDV = sMat[0][1]*sSize.y;
            sDUV = (sDUV + sMat[1][1]*sSize.x*sSize.y)/2.0;
        }
        else {
            sDU = sMat[1][0]*sSize.x;
            sDV = sMat[0][1]*sSize.y;
            sDUV = sMat[1][1]*sSize.x*sSize.y;
        }
        COPY_XYZ(sP00,Pw[3][0]);
        SmPoint3d sP10 = sP00 - sDU/3.0;
        COPY_XYZ(sP10,Pw[2][0]);
        SmPoint3d sP01 = sP00 + sDV/3.0;
        COPY_XYZ(sP01,Pw[3][1]);
        SmPoint3d sP11 = sP10 + sP01 - sP00 - sDUV/9.0; 
        COPY_XYZ(sP11,Pw[2][1]);
#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
        if (bDebugMe) {
            smgfx_Erase();
            smgfx_SetColor(1,0,0);
            sP00.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,0,1);
            sP10.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,1,1);
            sP01.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,0,0);
            sP11.Draw();
            sm_GraphicsLoop();
        }
#endif
    }

    {
        SER(crSurface.Evaluate(crUVDomain.Evaluate(0.0,1.0),1,1,TRUE,TRUE,FALSE,sMat[0]));
        SmPoint3d sP00 = sMat[0][0];
        if (crSurface.IsSingularity(crUVDomain.Evaluate(0.0,1.0),eSingDir)) {
            SmPoint2d sUV = crUVDomain.Evaluate(0.0,1.0);
            SmPoint2d sUVU(sUV.x+(sSize.x*SM_EFF_ZERO_SQRT),sUV.y);
            SmPoint2d sUVV(sUV.x,sUV.y-(sSize.y*SM_EFF_ZERO_SQRT));
            SER(crSurface.Evaluate(sUVU,1,1,TRUE,TRUE,FALSE,sMat[0]));
            sDU = sMat[1][0]*sSize.x;
            sDUV = sMat[1][1]*sSize.x*sSize.y;
            SER(crSurface.Evaluate(sUVV,1,1,TRUE,TRUE,FALSE,sMat[0]));
            sDV = sMat[0][1]*sSize.y;
            sDUV = (sDUV + sMat[1][1]*sSize.x*sSize.y)/2.0;
        }
        else {
            sDU = sMat[1][0]*sSize.x;
            sDV = sMat[0][1]*sSize.y;
            sDUV = sMat[1][1]*sSize.x*sSize.y;
        }
        COPY_XYZ(sP00,Pw[0][3]);
        SmPoint3d sP10 = sP00 + sDU/3.0;
        COPY_XYZ(sP10,Pw[1][3]);
        SmPoint3d sP01 = sP00 - sDV/3.0;
        COPY_XYZ(sP01,Pw[0][2]);
        SmPoint3d sP11 = sP10 + sP01 - sP00 - sDUV/9.0; 
        COPY_XYZ(sP11,Pw[1][2]);
#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
        if (bDebugMe) {
            smgfx_Erase();
            smgfx_SetColor(1,0,0);
            sP00.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,0,1);
            sP10.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,1,1);
            sP01.Draw();
            sm_GraphicsLoop();
            smgfx_SetColor(0,0,0);
            sP11.Draw();
            sm_GraphicsLoop();
        }
#endif
    }

    for (ULONG j=0; j<4; j++) {
        for (ULONG k=0; k<4; k++) {
            Pw[j][k].w = NL_NOW;
        }
    }

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
    if (bDebugMe) {
        sm_GraphicsLoop();
        smgfx_SetColor(1,0,0);
        SmBSplineSurface sBSS(pSurface,TRUE,crSurface.GetContext()) ;
        sBSS.SetContext(crSurface.GetContext());
        sBSS.Dump();
        sBSS.DrawNet();
        sBSS.DrawUV(5,5);
        sm_GraphicsLoop();
    }
#endif

    return SM_SUCCESS;

} // end sm_ComputeBezierApproximation

// ---- verbatim: SmSurfaceCache.cpp lines 4785-4853: sm_CreateBezierPatchBlock
static SmStatus sm_CreateBezierPatchBlock
  (const gw_SURFACE       * pSource,      // in : BSpline surface to decompose      
   SmMemBlockMgr          & rBezierMgr,   // out: modified to hold memory for array of bezier patches, stores SmBezierPatch objects
   SmMemBlockMgr          * pBezierBlock, // in : Bezier Surface Patch memory, stores gw_SURFACE objects   
   ULONG                  & rlNumUSpans,  // out: number of u spans in pSource
   ULONG                  & rlNumVSpans,  // out: number of v spans in pSource
   SmTArray<gw_SURFACE *> & rBeziers)     // out: list of Bezier patch pointers - one for every non-zero surface span
                                          //      ordered:[patch00, patch01, ... patch0N, patch10, ... patchMN]
{
  // BSpline Surface locals               
  gw_KNOTVECTOR *knu = pSource->knu;
  gw_KNOTVECTOR *knv = pSource->knv;

  // get span counts
  gw_INDEX nspu, nspv;
  N_BasisGetSpanCount(knu,pSource->p,&nspu);
  N_BasisGetSpanCount(knv,pSource->q,&nspv);

  // set output
  rlNumUSpans = nspu;
  rlNumVSpans = nspv;

  // compute size in bytes of the BSpline Surface
  ULONG lBezSize = sm_ComputeNurbSurfaceSize(pSource->p,     pSource->q,
                                             pSource->p*2+1, pSource->q*2+1);

  // Figure out how many gw_SURFACE bezier patches per allocated block.
  // Don't let this get too big because that wastes a lot space.
  ULONG lNumPerBlock = rlNumUSpans * rlNumVSpans + pSource->p + pSource->q + 1 ;
  if (lNumPerBlock > 40) 
    {
      lNumPerBlock = 40;
    }
  // initilize bezier patch memory to hold SmBezierPatch objects augmented to hold the gw_SURFACE values.
  rBezierMgr   .Initialize(ALIGN_SIZE(sizeof(SmBezierPatch)), lNumPerBlock);
  pBezierBlock->Initialize(             lBezSize,             lNumPerBlock) ;

  // allocate enough room to hold a Bezier patch for every span cross-product plus a few
  rBeziers.SetSize(rlNumUSpans*rlNumVSpans);

  // for every surface patch - build rBeziers list, init pointer values
  for (ULONG i=0; i<rlNumUSpans*rlNumVSpans; i++) 
    {
      // get pointer to memory to hold Bezier patch
      SmBezierPatch *pBezPat  = (SmBezierPatch*)rBezierMgr.GetAt(i);
      pBezPat->m_pBezier = (gw_SURFACE *)pBezierBlock->GetAt(i);

      gw_SURFACE    *pSurface = pBezPat->GetBezierPtr();

      // initialize pSurface's (the new bezier patch) internal pointers to mimick the 
      //   NLIB data structure but all in one contiguous piece of memory.
      // note: this sets pointers - but does not set any of the data.
      sm_InitNurbSurfaceMemory(pSurface, 
                               pSource->p, pSource->q,
                               pSource->p, pSource->q,
                               pSource->p*2+1,pSource->q*2+1);
      
      // store the bezier patch object in the list of bezier patches
      rBeziers[i] = pSurface;
    } // end iter every surface patch initializing pointer addresses

  // store a bezier patch for every non-zero span
  // order: [patch00, patch01, ... patch0N, patch10, ... patchMN]
  GW_SER(sm_DecomposeSrf(pSource, rBeziers));

  // all done
  return SM_SUCCESS;

} // end sm_CreateBezierPatchBlock

// ---- verbatim: SmSurfaceCache.cpp lines 4860-4966: sm_CreateApproximateBezierPatchBlock
static SmStatus sm_CreateApproximateBezierPatchBlock
  (const SmSurface        & crSurface,     // in : Non-BSpline surface to decompose                                       
   SmMemBlockMgr          & rBezierMgr,    // out: modified to hold memory for array of SmBezierPatch objects 
   SmMemBlockMgr          * pBezierBlock,  // out: modified to hold memory for array of NURB gw_SURFACE objects               
   ULONG                  & rlNumUSpans,   // out: number of u spans in pSource                                       
   ULONG                  & rlNumVSpans,   // out: number of v spans in pSource 
   SmTArray<double>       & rUSplits,      // out: Split Points in U direction                                      
   SmTArray<double>       & rVSplits,      // out: Split Points in V direction                                      
   SmTArray<gw_SURFACE *> & rBeziers)      // out: list of Bezier patch pointers - one for every non-zero surface span
                                           //      ordered:[patch00, patch01, ... patch0N, patch10, ... patchMN]      
{
  // Note we are creating approximate cubic beziers
  //   - this can be used to reduce the cache size for an SmBSplineSurface 
  //                      or for an SmOffsetSurface
  double dKnot ;
  SER(crSurface.GetKnots(SM_SP_U,rUSplits));

  // limit initial U knot count to 6
  if (rUSplits.GetSize() > 6) 
    {
      double dMin = rUSplits[0];
      double dMax = rUSplits.GetLast();        
      rUSplits.SetSize(6);
      rUSplits.SetAt(0, dMin) ;
      rUSplits.SetAt(5, dMax) ;
      for (ULONG i=1; i<=4; i++) 
        {
          dKnot = dMin + (i / 5.0) * (dMax - dMin);
          rUSplits.SetAt(i, dKnot);
        }
    }

  // limit initial V knot count to 6
  SER(crSurface.GetKnots(SM_SP_V,rVSplits));
  if (rVSplits.GetSize() > 6) 
    {
      double dMin = rVSplits[0];
      double dMax = rVSplits.GetLast();        
      rVSplits.SetSize(6);
      rVSplits.SetAt(0, dMin) ;
      rVSplits.SetAt(5, dMax) ;
      for (ULONG i=1; i<=4; i++) 
        {
          dKnot = dMin + (i / 5.0) * (dMax - dMin);
          if (i == 5) dKnot = dMax;
          rVSplits.SetAt(i, dKnot);
        }
    }

  // get non-zero span count
  rlNumUSpans = rUSplits.GetSize() - 1;
  rlNumVSpans = rVSplits.GetSize() - 1;

  // get memory size in bytes of one degree 3x3 gw_SURFACE
  ULONG lBezSize     = sm_ComputeNurbSurfaceSize(3,3,3*2+1,3*2+1);
  ULONG lNumPerBlock = rlNumUSpans * rlNumVSpans + 16;
  SM_ASSERT(lNumPerBlock <= 5*5 + 16) ;

  // Don't let lNumPerBlock get too big because it wastes space
  if (lNumPerBlock > 100) lNumPerBlock = 100;
  if (lNumPerBlock <  30) lNumPerBlock =  30;

  // initilize bezier patch memory to hold SmBezierPatch objects augmented to hold the gw_SURFACE values.
  rBezierMgr.   Initialize(ALIGN_SIZE(sizeof(SmBezierPatch)), lNumPerBlock);
  pBezierBlock->Initialize(            lBezSize,              lNumPerBlock);

  ULONG lCount = 0;
  rBeziers.SetSize(rlNumUSpans*rlNumVSpans);

  // for every U span
  for (ULONG i=0; i<rlNumUSpans; i++) 
    {
      double dMinU = rUSplits[i];
      double dMaxU = rUSplits[i+1];

      // for every V Span
      for (ULONG j=0; j<rlNumVSpans; j++) 
        {
          double          dMinV   = rVSplits[j];
          double          dMaxV   = rVSplits[j+1];

          // set next patch extent
          SmExtent2d sUVDomain(dMinU, dMinV, 
                               dMaxU, dMaxV);
          
          // get next patch memory from rBezierMgr
          SmBezierPatch * pBezPat = (SmBezierPatch*)rBezierMgr.GetAt(lCount);
          pBezPat->m_pBezier = (gw_SURFACE *)pBezierBlock->GetAt(lCount);

          gw_SURFACE    * pSurface = pBezPat->GetBezierPtr();

          // set next patch memory internal pointers to mimick NLib structure but in contiguous memory
          sm_InitNurbSurfaceMemory(pSurface,3,3,3,3,3*2+1,3*2+1);

          // approximate the surface shape with a degree 3x3 bezier patch
          sm_ComputeBezierApproximation(crSurface,sUVDomain,pSurface);

          // make the surface pointer array
          rBeziers[lCount] = pSurface;
          lCount ++;
        }
    } // end iter every USpan making a degree 3x3 bezier approximation
    
  // all done 
  return SM_SUCCESS;

} // end sm_CreateApproximateBezierPatchBlock

// clang-format on
} // namespace sc_ssc_p2

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_surface_cache_p2.rs).
enum : int32_t
{
    OP_NTH = 0,
    OP_ADD = 1,
    OP_BEZ_BLOCK = 2,
    OP_APPROX_BLOCK = 3,
    OP_TEST = 4,
    OP_SUBDIV_BLOCK = 5,
};

const int64_t kMaxObjects = 4096;
alignas(16) char g_objs[kMaxObjects];

const void* obj_ptr(int64_t i)
{
    return i < 0 ? nullptr : static_cast<const void*>(g_objs + i);
}

struct TreePeek : SmTree
{
    explicit TreePeek(SmObject* owner) : SmTree(owner) {}
    SmMemBlockMgr& node_mgr() { return m_sNodeMgr; }
    SmTreeNode*& top() { return m_pTopNode; }
};

struct CachePeek : SmSurfaceCache
{
    CachePeek(const SmSurface& s, const SmExtent2d& d, double ch, double ang, double asp, double mx, double mn,
              double ratio)
        : SmSurfaceCache(s, d, ch, ang, asp, mx, mn, ratio)
    {
    }
    SmTree*& tree() { return m_pTree; }
    SmMemBlockMgr& bez() { return m_sBezMgr; }
    SmMemBlockMgr& aux() { return m_sAuxMgr; }
    ULONG& nalt() { return m_lNumAltCaches; }
    SmAltSrfCache** alts() { return m_apAltSrfCaches; }
    double& start_fast() { return m_dStartFastSubdivisionAngleDeg; }
    SmBoolean& switch_fast() { return m_bSwitchToFastSubdivision; }
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

void put_box(std::vector<double>& o, const SmExtent3d& e)
{
    put_vec(o, e.GetMin());
    put_vec(o, e.GetMax());
}

// `n m p q r s cpts(x y z w, v index fastest) u v`.
void put_nurb(std::vector<double>& o, const gw_SURFACE* s)
{
    gw_INDEX n = s->net->n;
    gw_INDEX m = s->net->m;
    o.push_back((double)n);
    o.push_back((double)m);
    o.push_back((double)s->p);
    o.push_back((double)s->q);
    o.push_back((double)s->knu->m);
    o.push_back((double)s->knv->m);
    for (gw_INDEX i = 0; i <= n; i++)
        for (gw_INDEX j = 0; j <= m; j++)
        {
            o.push_back(s->net->Pw[i][j].x);
            o.push_back(s->net->Pw[i][j].y);
            o.push_back(s->net->Pw[i][j].z);
            o.push_back(s->net->Pw[i][j].w);
        }
    for (gw_INDEX i = 0; i <= s->knu->m; i++)
        o.push_back(s->knu->U[i]);
    for (gw_INDEX i = 0; i <= s->knv->m; i++)
        o.push_back(s->knv->U[i]);
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

// `oob form nurb`; with `raw_knots` the given knots are written back over the NURB after
// the constructor has fixed them up (sm_FixupKnotVector), so near-coincident knots stay.
SmBSplineSurface* read_bspline(Rd& rd, const SmContext& ctx, bool raw_knots = false)
{
    bool oob = rd.flag();
    SmBSplineSurfaceForm form = (SmBSplineSurfaceForm)rd.idx();
    gw_SURFACE* sur = read_nurb(rd);
    std::vector<double> ku(sur->knu->U, sur->knu->U + sur->knu->m + 1);
    std::vector<double> kv(sur->knv->U, sur->knv->U + sur->knv->m + 1);
    SmBSplineSurface* s = new (ctx) SmBSplineSurface(sur, FALSE, NULL);
    if (raw_knots)
    {
        gw_SURFACE* own = s->GetGwNurbPointer();
        for (size_t i = 0; i < ku.size(); i++)
            own->knu->U[i] = ku[i];
        for (size_t i = 0; i < kv.size(); i++)
            own->knv->U[i] = kv[i];
    }
    s->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
    s->SetBSplineSurfaceForm(form);
    return s;
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

// `k` then `k` curves into a new array, or `-1` for NULL.
SmTArray<SmCurve*>* read_curves(Rd& rd, const SmContext& ctx)
{
    int64_t k = rd.idx();
    if (k < 0)
        return NULL;
    SmTArray<SmCurve*>* a = new (ctx) SmTArray<SmCurve*>(ctx);
    for (int64_t i = 0; i < k; i++)
        a->Add(read_curve(rd, ctx));
    return a;
}

// Alternate cache user data: `hv V hb b hl l hd d hp p`.
struct Key
{
    bool hv, hb, hl, hd, hp;
    SmVector3d v;
    SmBoolean b;
    long l;
    double d;
    int64_t p;
    void read(Rd& rd)
    {
        hv = rd.flag();
        v = rd.vec();
        hb = rd.flag();
        b = rd.flag() ? TRUE : FALSE;
        hl = rd.flag();
        l = (long)rd.idx();
        hd = rd.flag();
        d = rd.num();
        hp = rd.flag();
        p = rd.idx();
    }
    const SmVector3d* pv() const { return hv ? &v : NULL; }
    const SmBoolean* pb() const { return hb ? &b : NULL; }
    const long* pl() const { return hl ? &l : NULL; }
    const double* pd() const { return hd ? &d : NULL; }
    const void* pp() const { return hp ? obj_ptr(p) : NULL; }
};

// A surface `kind ...`: 0 B-spline `oob form nurb`; 1 bounded plane `o x y sx sy D`;
// 2 curve-bounded surface (implicit outer, no boundary curves) over a B-spline block;
// 3 / 4 are 0 / 2 with the B-spline's knots kept as given (read_bspline's raw_knots).
// `*basis` receives a separately allocated surface to delete after it (always NULL now).
SmSurface* read_surface(Rd& rd, const SmContext& ctx, SmSurface** basis)
{
    *basis = NULL;
    int64_t kind = rd.idx();
    if (kind == 1)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double sx = rd.num();
        double sy = rd.num();
        SmExtent2d d = rd.domain();
        return new (ctx) SmPlane(o, x, y, SmVector2d(sx, sy), d, &ctx);
    }
    SmBSplineSurface* b = read_bspline(rd, ctx, kind == 3 || kind == 4);
    if (kind == 0 || kind == 3)
        return b;
    // the curve-bounded surface owns (deletes) its basis
    SmTArray<SmCurve*> c3d(ctx), cuv(ctx);
    return new (ctx) SmCurveBoundedSurface(*b, &c3d, &cuv, TRUE);
}

// Cache options `ch ang asp max min ratio start`.
CachePeek* read_cache(Rd& rd, const SmSurface* s, const SmExtent2d& d)
{
    double ch = rd.num();
    double ang = rd.num();
    double asp = rd.num();
    double mx = rd.num();
    double mn = rd.num();
    double ratio = rd.num();
    double start = rd.num();
    CachePeek* c = new (*s->GetContext()) CachePeek(*s, d, ch, ang, asp, mx, mn, ratio);
    c->start_fast() = start;
    return c;
}

TreePeek* new_tree(CachePeek* cache, const SmContext& ctx)
{
    TreePeek* tree = new (ctx) TreePeek(cache);
    tree->node_mgr().Initialize(ALIGN_SIZE(sizeof(SmTreeNode)), 25);
    cache->bez().Initialize(ALIGN_SIZE(sizeof(SmBezierPatch)), 25);
    cache->aux().Initialize(ALIGN_SIZE(sizeof(SmBezierAux2d)), 25);
    cache->tree() = tree;
    return tree;
}

// A node from the tree's node manager with data from the patch (aux 3) or aux manager.
SmTreeNode* new_node(TreePeek* tree, CachePeek* cache, SmAuxDataType aux, const SmExtent2d& d, const SmSurface* s)
{
    SmTreeNode* nd = (SmTreeNode*)tree->node_mgr().GetNewElement();
    nd->m_pTree = tree;
    nd->m_pParent = NULL;
    nd->m_pChild1 = NULL;
    nd->m_pChild2 = NULL;
    nd->m_eAuxDataType = aux;
    nd->m_eGeomType = SM_NG_DEFAULT;
    SmBezierAux2d* a = aux == SM_AD_BEZIER_SURFACE ? (SmBezierAux2d*)cache->bez().GetNewElement()
                                                   : (SmBezierAux2d*)cache->aux().GetNewElement();
    a->m_eSplitDir = SM_SP_U;
    a->m_sUVDomain = d;
    a->mBA_pSurface = s;
    a->m_pOwningTreeNode = nd;
    nd->m_pData = a;
    return nd;
}

// Every node of the tree's node manager: `count` then `parent c1 c2 aux split D box`.
void put_tree(std::vector<double>& o, TreePeek* tree)
{
    SmTArray<void*> all;
    tree->node_mgr().GetActiveElements(all);
    o.push_back((double)all.GetSize());
    auto index_of = [&](const SmTreeNode* p) -> double {
        if (p == NULL)
            return -1.0;
        for (ULONG i = 0; i < all.GetSize(); i++)
            if (all[i] == (const void*)p)
                return (double)i;
        return -2.0;
    };
    for (ULONG i = 0; i < all.GetSize(); i++)
    {
        const SmTreeNode* nd = (const SmTreeNode*)all[i];
        o.push_back(index_of(nd->m_pParent));
        o.push_back(index_of(nd->m_pChild1));
        o.push_back(index_of(nd->m_pChild2));
        o.push_back((double)nd->m_eAuxDataType);
        const SmBezierAux2d* a = (const SmBezierAux2d*)nd->m_pData;
        o.push_back((double)a->m_eSplitDir);
        put_dom(o, a->m_sUVDomain);
        put_box(o, nd->m_sBBox);
    }
}

void op_nth(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* basis = NULL;
    SmSurface* s = read_surface(rd, ctx, &basis);
    SmExtent2d d = rd.domain();
    SmPatchBoundaryBoundingPlanes bp(s, d);
    int64_t k = rd.idx();
    o.push_back(0.0);
    for (int64_t i = 0; i < k; i++)
    {
        ULONG index = (ULONG)rd.idx();
        SmBoolean exists = FALSE;
        SmBoolean degen = TRUE;
        SmPoint3d base(0.0, 0.0, 0.0);
        SmVector3d normal(0.0, 0.0, 0.0);
        SmVector3d* samples = NULL;
        double tol = -1.0;
        SmStatus r = bp.GetNthBoundingPlane(index, exists, degen, base, normal, samples, tol);
        o.push_back(st(r));
        o.push_back(flg(exists == TRUE));
        o.push_back(flg(degen == TRUE));
        if (exists == TRUE)
        {
            put_vec(o, base);
            put_vec(o, normal);
            for (int j = 0; j < SM_PB_SAMPLECOUNT; j++)
                put_vec(o, samples[j]);
            o.push_back(tol);
        }
    }
    delete s;
    delete basis;
}

void op_add(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* basis = NULL;
    SmSurface* s = read_surface(rd, ctx, &basis);
    CachePeek* cache = new (ctx) CachePeek(*s, s->GetNaturalUVDomain(), 0.0, 0.0, 0.0, 0.0, 0.0, 0.001);
    o.push_back(0.0);
    int64_t na = rd.idx();
    for (int64_t i = 0; i < na; i++)
    {
        SmAltSrfCacheType t = (SmAltSrfCacheType)rd.idx();
        double ap = rd.num();
        double an = rd.num();
        Key key;
        key.read(rd);
        SmTArray<SmCurve*>* c3 = read_curves(rd, ctx);
        SmTArray<SmCurve*>* cu = read_curves(rd, ctx);
        SmStatus r = cache->AddCurvesToCache(t, ap, an, key.pv(), key.pb(), key.pl(), key.pd(), key.pp(), c3, cu);
        o.push_back(st(r));
        delete c3;
        delete cu;
    }
    ULONG nalt = cache->nalt();
    o.push_back((double)nalt);
    for (ULONG i = 0; i < nalt; i++)
    {
        SmTArray<SmCurve*> a3, au;
        SmStatus r = cache->alts()[i]->GetCopyOfCurves(ctx, &a3, &au);
        o.push_back(st(r));
        o.push_back((double)a3.GetSize());
        o.push_back((double)au.GetSize());
        if (r == SM_SUCCESS)
        {
            SmObjsDelete<SmCurve*> d1(&a3);
            SmObjsDelete<SmCurve*> d2(&au);
        }
    }
    int64_t nq = rd.idx();
    for (int64_t q = 0; q < nq; q++)
    {
        SmAltSrfCacheType t = (SmAltSrfCacheType)rd.idx();
        double ap = rd.num();
        double an = rd.num();
        Key key;
        key.read(rd);
        for (ULONG i = 0; i < nalt; i++)
            o.push_back(flg(cache->alts()[i]->MatchCache(t, ap, an, key.pv(), key.pb(), key.pl(), key.pd(),
                                                         key.pp()) == TRUE));
    }
    delete cache;
    delete s;
    delete basis;
}

void op_bez_block(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmBSplineSurface* s = read_bspline(rd, ctx);
    SmMemBlockMgr bez_mgr;
    SmMemBlockMgr bez_block;
    ULONG nu = 0;
    ULONG nv = 0;
    SmTArray<gw_SURFACE*> beziers;
    SmStatus r = sc_ssc_p2::sm_CreateBezierPatchBlock(s->GetGwNurbPointer(), bez_mgr, &bez_block, nu, nv, beziers);
    o.push_back(st(r));
    if (r == SM_SUCCESS)
    {
        o.push_back((double)nu);
        o.push_back((double)nv);
        o.push_back((double)beziers.GetSize());
        for (ULONG i = 0; i < beziers.GetSize(); i++)
            put_nurb(o, beziers[i]);
    }
    delete s;
}

void op_approx_block(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* basis = NULL;
    SmSurface* s = read_surface(rd, ctx, &basis);
    SmMemBlockMgr bez_mgr;
    SmMemBlockMgr bez_block;
    ULONG nu = 0;
    ULONG nv = 0;
    SmTArray<double> us, vs;
    SmTArray<gw_SURFACE*> beziers;
    SmStatus r = sc_ssc_p2::sm_CreateApproximateBezierPatchBlock(*s, bez_mgr, &bez_block, nu, nv, us, vs, beziers);
    o.push_back(st(r));
    if (r == SM_SUCCESS)
    {
        o.push_back((double)nu);
        o.push_back((double)nv);
        o.push_back((double)us.GetSize());
        for (ULONG i = 0; i < us.GetSize(); i++)
            o.push_back(us[i]);
        o.push_back((double)vs.GetSize());
        for (ULONG i = 0; i < vs.GetSize(); i++)
            o.push_back(vs[i]);
        o.push_back((double)beziers.GetSize());
        for (ULONG i = 0; i < beziers.GetSize(); i++)
            put_nurb(o, beziers[i]);
    }
    delete s;
    delete basis;
}

// `S D opts aux children chsat angsat nodeD [aux 3: nurb ivl(3 x lo hi)]`.
void op_test(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* basis = NULL;
    SmSurface* s = read_surface(rd, ctx, &basis);
    SmExtent2d cd = rd.domain();
    CachePeek* cache = read_cache(rd, s, cd);
    TreePeek* tree = new_tree(cache, ctx);
    SmAuxDataType aux = (SmAuxDataType)rd.idx();
    bool children = rd.flag();
    bool chsat = rd.flag();
    bool angsat = rd.flag();
    SmExtent2d nd_dom = rd.domain();
    SmTreeNode* nd = new_node(tree, cache, aux, nd_dom, s);
    tree->top() = nd;
    SmBezierAux2d* a = (SmBezierAux2d*)nd->m_pData;
    a->m_bChordHeightSatisfied = chsat ? TRUE : FALSE;
    a->m_bAngleTolSatisfied = angsat ? TRUE : FALSE;
    gw_SURFACE* bez = NULL;
    if (aux == SM_AD_BEZIER_SURFACE)
    {
        SmBezierPatch* bp = (SmBezierPatch*)a;
        bez = read_nurb(rd);
        bp->m_pBezier = bez;
        double l0 = rd.num(), h0 = rd.num(), l1 = rd.num(), h1 = rd.num(), l2 = rd.num(), h2 = rd.num();
        bp->GetPseudoBox().SetIntervals(SmExtent1d(l0, h0), SmExtent1d(l1, h1), SmExtent1d(l2, h2));
    }
    if (children)
    {
        SmTreeNode* c1 = new_node(tree, cache, SM_AD_AUX_DATA, nd_dom, s);
        SmTreeNode* c2 = new_node(tree, cache, SM_AD_AUX_DATA, nd_dom, s);
        nd->m_pChild1 = c1;
        nd->m_pChild2 = c2;
        c1->m_pParent = nd;
        c2->m_pParent = nd;
    }
    SmBoolean needs = TRUE;
    SmSurfParamType dir = SM_SP_UNKNOWN;
    SmStatus r = cache->TestAgainstTolerances(nd, needs, dir);
    o.push_back(st(r));
    if (r == SM_SUCCESS)
    {
        o.push_back(flg(needs == TRUE));
        o.push_back((double)dir);
    }
    o.push_back(flg(a->m_bChordHeightSatisfied == TRUE));
    o.push_back(flg(a->m_bAngleTolSatisfied == TRUE));
    o.push_back(flg(cache->switch_fast() == TRUE));
    if (r == SM_SUCCESS)
        put_box(o, nd->m_sBBox);
    if (bez)
        smos_Free(bez);
    delete cache;
    delete s;
    delete basis;
}

// `S opts D nosurf`.
void op_subdiv_block(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* basis = NULL;
    SmSurface* s = read_surface(rd, ctx, &basis);
    CachePeek* cache = read_cache(rd, s, s->GetNaturalUVDomain());
    TreePeek* tree = new_tree(cache, ctx);
    SmExtent2d d = rd.domain();
    bool nosurf = rd.flag();
    SmTreeNode* nd = new_node(tree, cache, SM_AD_AUX_DATA, d, nosurf ? NULL : s);
    tree->top() = nd;
    SmStatus r = cache->SubdivideByBlock(tree, nd);
    o.push_back(st(r));
    put_tree(o, tree);
    delete cache;
    delete s;
    delete basis;
}
} // namespace

SC_EXPORT int32_t sc_surface_surface_cache_p2_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                                   int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    switch (op)
    {
    case OP_NTH: op_nth(rd, ctx, o); break;
    case OP_ADD: op_add(rd, ctx, o); break;
    case OP_BEZ_BLOCK: op_bez_block(rd, ctx, o); break;
    case OP_APPROX_BLOCK: op_approx_block(rd, ctx, o); break;
    case OP_TEST: op_test(rd, ctx, o); break;
    case OP_SUBDIV_BLOCK: op_subdiv_block(rd, ctx, o); break;
    default: return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
