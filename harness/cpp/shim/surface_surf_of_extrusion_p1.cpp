// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surf_of_extrusion_p1`.
// Wraps the SmSurfOfExtrusion members owned by the unit (source/SMLib/src/SmSurfOfExtrusion.cpp):
// the two constructors (through a derived class: they are protected) with MakeNurb, and
// CreateCanonical; operator==, the copy constructor, AdjustSTEPUVDomain,
// UpdateAnalyticalDomain, Reparameterize, STEPInversion, ConvertUVFromSTEPToNURBS,
// ConvertUVFromNURBSToSTEP, EvaluateSTEP, EvaluateSTEPPoint, GetCanonical,
// GetSTEPLinearParamExtent, GetGenDirParamExtent, GetMaxAnalyticDomain, GetOrigin,
// GetExtrusionVector, CreateDirectrixFromPoint, DropPointFast, Transform, IsKindOf,
// GetType, GetSwapUVBit and the file-static sm_IntersectGenerators (copied verbatim
// below, minus its debug drawing, because a static cannot be linked against).
//
// One entry point, sc_surface_surf_of_extrusion_p1_eval(op, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/surface_surf_of_extrusion_p1.rs,
// which validates every input so upstream only touches valid memory. `out` starts with the
// status of the surface construction.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmGeomUtility.h>
#include <SmHyperbola.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmParabola.h>
#include <SmSolutionArray.h>
#include <SmSurfOfExtrusion.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <cmath>
#include <vector>

#include "sc_sm.h"

using sc::flg;
using sc::put_dom;
using sc::put_vec;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_surf_of_extrusion_p1.rs).
enum : int32_t
{
    OP_BUILD = 0,
    OP_EVAL_STEP = 1,
    OP_TO_NURBS = 2,
    OP_TO_STEP = 3,
    OP_ADJUST = 4,
    OP_UPDATE = 5,
    OP_REPARAM = 6,
    OP_TRANSFORM = 7,
    OP_DIRECTRIX = 8,
    OP_DROP_FAST = 9,
    OP_STEP_INV = 10,
    OP_EQ = 11,
    OP_COPY = 12,
    OP_GENERATORS = 13,
};

// Generator kinds.
enum : int32_t
{
    GK_BSPLINE = 0,
    GK_LINE = 1,
    GK_CIRCLE = 2,
    GK_ELLIPSE = 3,
    GK_HYPERBOLA = 4,
    GK_PARABOLA = 5,
    GK_LAZY_LINE = 6,
};

// Construction modes.
enum : int32_t
{
    BM_CANONICAL = 0, // CreateCanonical(curve, extrusion vector)
    BM_CTOR = 1,      // SmSurfOfExtrusion(curve, origin, vector, domain, swap) + MakeNurb
    BM_INFINITE = 2,  // SmSurfOfExtrusion(curve, vector, swap) + MakeNurb
};

// Reads the protected members through pointers to members named in a derived class, and
// reaches the protected constructors.
struct Peek : SmSurfOfExtrusion
{
    static SmCurveOrientType SmSurfOfExtrusion::*orient() { return &Peek::m_eOrientation; }
    static SmVector3d SmSurfOfExtrusion::*cpoint() { return &Peek::m_vCurvePoint; }
    static SmVector3d SmSurfOfExtrusion::*cvec() { return &Peek::m_vCurveVec; }
    Peek(SmBSplineCurve* c, const SmPoint3d& o, const SmVector3d& v, const SmExtent2d& d,
         SmBoolean swap)
        : SmSurfOfExtrusion(c, o, v, d, swap)
    {
    }
    Peek(SmBSplineCurve* c, const SmVector3d& v, SmBoolean swap) : SmSurfOfExtrusion(c, v, swap) {}
};

// ---- sm_IntersectGenerators: verbatim copy of the file-static of SmSurfOfExtrusion.cpp
// (the SM_DEBUG_CODE drawing blocks are left out; they are not compiled upstream either).
SmStatus sm_IntersectGenerators(const SmSurfOfExtrusion& rExtrusionSurf, double dTol,
                                SmCurve& rOtherGenCurve, SmVector3d& rOtherExtrusionVec,
                                SmExtent1d& rOtherIvl, SmTArray<SmCurve*>* pOpt3DCurves,
                                SmTArray<SmTsectCurveType>* pOptCurveTypes,
                                SmBoolean& rbNeedsMoreIntersections)
{
  // locals
  ULONG ii, jj, kk ;
  SmBSplineCurve     * pExtrusionGenCurve = NULL;
  SmVector3d           sExtrusionVec ;
  SmExtent1d           sExtrusionIvl         = rExtrusionSurf.GetSTEPUVDomain().GetVInterval() ;
  const SmContext    * pContext              = rExtrusionSurf.GetContext();
  ((SmSurfOfExtrusion &)rExtrusionSurf).GetCanonical(pExtrusionGenCurve, sExtrusionVec) ;
  SmExtent1d           sExtrusionGenCurveIvl = pExtrusionGenCurve->GetNaturalInterval() ;
  double               dExtrusionLength      = sExtrusionVec.Length() ;
  double               dOtherLength          = rOtherExtrusionVec.Length() ;

  // check parallel extrusion vectors
  if(!rOtherExtrusionVec.IsParallelTo(sExtrusionVec, SM_EFF_ZERO_RAD))
    {
      rbNeedsMoreIntersections = TRUE ;
      return SM_SUCCESS ;
    }

  SmCurve *pOffsetCurve [2][2] ;   // indices used for:[beg/end range solution][bot/top XSectIvl]
  double   dOffsetLength[2][2] ;   // indices used for:[beg/end range solution][bot/top XSectIvl]

  // init output
  if(pOpt3DCurves)   { pOpt3DCurves->ReSet() ; }
  if(pOptCurveTypes) { pOptCurveTypes->ReSet() ; }
  rbNeedsMoreIntersections = FALSE ;

  SmVector3d sUnitExtrusionVec = sExtrusionVec ;
  sUnitExtrusionVec.Unitize() ;
  SmSolutionArray sSolutions;
  SER(pExtrusionGenCurve->GlobalCurveSolve(sExtrusionGenCurveIvl,
                                           rOtherGenCurve,
                                           rOtherGenCurve.GetNaturalInterval(),
                                           SM_SO_PROJECTED_INTERSECT,
                                           dTol,
                                           NULL,
                                           &sUnitExtrusionVec,
                                           SM_SR_ALL,
                                           sSolutions));

  // all done - no solutions
  if(sSolutions.GetSize() == 0)
    { return SM_SUCCESS ; }

  SmBoolean  bSameExtrusionDir = rOtherExtrusionVec.Dot(sExtrusionVec) > 0 ;
  SmExtent1d sOtherLineIvl =  (bSameExtrusionDir)
                             ? SmExtent1d(rOtherIvl.GetMin()* dOtherLength/dExtrusionLength,
                                          rOtherIvl.GetMax()* dOtherLength/dExtrusionLength)
                             : SmExtent1d(rOtherIvl.GetMax()*-dOtherLength/dExtrusionLength,
                                          rOtherIvl.GetMin()*-dOtherLength/dExtrusionLength) ;

  ULONG lNewCurveCount = 0 ;
  SmPoint3d        sExtrusionPoint[2], sOtherPoint[2];
  double           dExtrusionEnd, dOtherEnd, dLineParam ;
  SmBSplineCurve  *pLineOrDegenerate = NULL ;
  SmTsectCurveType eTsectType;

  for (ii=0; ii<sSolutions.GetSize(); ii++)
    {
      SmSolution &rSol = sSolutions[ii] ;
      lNewCurveCount   = 0 ;

      for(jj=0;jj<1 || (jj<2 && rSol.m_eSolutionType == SM_ST_RANGE_OF_VALUES);jj++)
        {
          dExtrusionEnd       = jj==0 ? rSol.m_vStart[0] : rSol.m_vEnd[0] ;
          dOtherEnd           = jj==0 ? rSol.m_vStart[1] : rSol.m_vEnd[1] ;

          SER(pExtrusionGenCurve->EvaluatePoint(dExtrusionEnd, sExtrusionPoint[jj]));
          SER(rOtherGenCurve.EvaluatePoint     (dOtherEnd,     sOtherPoint[jj]));

          smgu_LineClosestPoint(sExtrusionPoint[jj], sExtrusionVec, sOtherPoint[jj], dLineParam) ;

          SmExtent1d sOthermappedIvl(sOtherLineIvl.GetMin() + dLineParam,
                                     sOtherLineIvl.GetMax() + dLineParam) ;

          SmExtent1d sXSectIvl ;
          sExtrusionIvl.Intersect(sOthermappedIvl, sXSectIvl) ;

          if(sXSectIvl.IsInit())
            { continue ; }

          if(rSol.m_eSolutionType == SM_ST_RANGE_OF_VALUES)
            {
              if     (sXSectIvl.GetMin() == sExtrusionIvl.GetMin())   { pOffsetCurve[jj][0]  = pExtrusionGenCurve ;
                                                                        dOffsetLength[jj][0] = sExtrusionIvl.GetMin() ;
                                                                      }
              else if(sXSectIvl.GetMin() == sExtrusionIvl.GetMax())   { pOffsetCurve[jj][0]  = pExtrusionGenCurve ;
                                                                        dOffsetLength[jj][0] = sExtrusionIvl.GetMax() ;
                                                                      }
              else if(sXSectIvl.GetMin() == sOthermappedIvl.GetMin()) { pOffsetCurve[jj][0]  = &rOtherGenCurve ;
                                                                        dOffsetLength[jj][0] = bSameExtrusionDir ? rOtherIvl.GetMin() : rOtherIvl.GetMax() ;
                                                                      }
              else if(sXSectIvl.GetMin() == sOthermappedIvl.GetMax()) { pOffsetCurve[jj][0]  = &rOtherGenCurve ;
                                                                        dOffsetLength[jj][0] = bSameExtrusionDir ? rOtherIvl.GetMax() : rOtherIvl.GetMin() ;
                                                                      }

              if     (sXSectIvl.GetMax() == sExtrusionIvl.GetMin())   { pOffsetCurve[jj][1]  = pExtrusionGenCurve ;
                                                                        dOffsetLength[jj][1] = sExtrusionIvl.GetMin() ;
                                                                      }
              else if(sXSectIvl.GetMax() == sExtrusionIvl.GetMax())   { pOffsetCurve[jj][1]  = pExtrusionGenCurve ;
                                                                        dOffsetLength[jj][1] = sExtrusionIvl.GetMax() ;
                                                                      }
              else if(sXSectIvl.GetMax() == sOthermappedIvl.GetMin()) { pOffsetCurve[jj][1]  = &rOtherGenCurve ;
                                                                        dOffsetLength[jj][1] = bSameExtrusionDir ? rOtherIvl.GetMin() : rOtherIvl.GetMax() ;
                                                                      }
              else if(sXSectIvl.GetMax() == sOthermappedIvl.GetMax()) { pOffsetCurve[jj][1]  = &rOtherGenCurve ;
                                                                        dOffsetLength[jj][1] = bSameExtrusionDir ? rOtherIvl.GetMax() : rOtherIvl.GetMin() ;
                                                                      }
            }

          if(sXSectIvl.GetLength() * dExtrusionLength < dTol)
            {
              lNewCurveCount++ ;
              SER(SmBSplineCurve::CreateDegenerateCurve(*pContext,
                                                        3,
                                                        sExtrusionPoint[jj] + sXSectIvl.GetMid() * sExtrusionVec,
                                                        pLineOrDegenerate));
              eTsectType     =   SM_TC_TOUCHING ;
            }
          else
            {
              lNewCurveCount++ ;
              SER(SmBSplineCurve::CreateLineSegment(*pContext,
                                                    3,
                                                    sExtrusionPoint[jj] + sXSectIvl.GetMin() * sExtrusionVec,
                                                    sExtrusionPoint[jj] + sXSectIvl.GetMax() * sExtrusionVec,
                                                    pLineOrDegenerate));
              eTsectType     =   rSol.m_eSolutionType == SM_ST_SINGLE_VALUE
                               ? SM_TC_CROSSING
                               : SM_TC_REGION_BOUNDARY ;
            }

          NER(pLineOrDegenerate);
          if(pOpt3DCurves)   { pOpt3DCurves->Add(pLineOrDegenerate); }
          if(pOptCurveTypes) { pOptCurveTypes->Add(eTsectType); }
        }

      if(rSol.m_eSolutionType == SM_ST_RANGE_OF_VALUES)
        {
          ULONG lSize = pOpt3DCurves->GetSize() ;
          if(   pOpt3DCurves != NULL
             && lNewCurveCount == 2
             && pOptCurveTypes->GetAt(lSize-1) == SM_TC_REGION_BOUNDARY
             && pOptCurveTypes->GetAt(lSize-2) == SM_TC_REGION_BOUNDARY
             && pOffsetCurve[0][0]  == pOffsetCurve[1][0]
             && SM_IS_ZERO(dOffsetLength[0][0] - dOffsetLength[1][0])
             && pOffsetCurve[0][1]  == pOffsetCurve[1][1]
             && SM_IS_ZERO(dOffsetLength[0][1] - dOffsetLength[1][1]))
            {
              for(kk=0;kk<2;kk++)
                {
                  SmCurve        * pTgtCurve   = pOffsetCurve[0][kk] ;
                  ULONG            lTgtIndex   = (pTgtCurve == &rOtherGenCurve) ? 1 : 0 ;
                  SmExtent1d       dTrimExtent1(smos_Min(rSol.m_vStart[lTgtIndex], rSol.m_vEnd[lTgtIndex]),
                                                smos_Max(rSol.m_vStart[lTgtIndex], rSol.m_vEnd[lTgtIndex]));
                  SmVector3d     & rTgtExtrusion = lTgtIndex == 0 ? sExtrusionVec : rOtherExtrusionVec ;

                  SmAxis2Placement sShiftRF;
                  sShiftRF.Translate(dOffsetLength[0][kk] * rTgtExtrusion);

                  SmCurve * pCopiedGenerator = NULL ;
                  pTgtCurve->Copy(*pContext, pCopiedGenerator) ; NER(pCopiedGenerator) ;
                  pCopiedGenerator->Transform(sShiftRF);
                  pCopiedGenerator->Trim(dTrimExtent1);

                  if(pOpt3DCurves)   { pOpt3DCurves->Add(pCopiedGenerator); }
                  if(pOptCurveTypes) { pOptCurveTypes->Add(SM_TC_REGION_BOUNDARY); }
                }
            }
          else if(   pOpt3DCurves != NULL
                  && lNewCurveCount == 2
                  && pOptCurveTypes->GetAt(lSize-1) == SM_TC_TOUCHING
                  && pOptCurveTypes->GetAt(lSize-2) == SM_TC_TOUCHING)
            {
              for(kk=0;kk<2;kk++)
                {
                  if(pOpt3DCurves)   { SmCurve *pTmpCurve = pOpt3DCurves->GetLast() ;
                                       delete pTmpCurve ; pTmpCurve = NULL ;
                                       pOpt3DCurves->RemoveLast() ;
                                     }
                  if(pOptCurveTypes) { pOptCurveTypes->RemoveLast() ; }
                }

              SmCurve        * pTgtCurve     = pOffsetCurve[0][0] ;
              ULONG            lTgtIndex     = (pTgtCurve == &rOtherGenCurve) ? 1 : 0 ;
              SmExtent1d       dTrimExtent1(rSol.m_vStart[lTgtIndex], rSol.m_vEnd[lTgtIndex]);
              SmVector3d     & rTgtExtrusion = lTgtIndex == 0 ? sExtrusionVec : rOtherExtrusionVec ;

              SmAxis2Placement sShiftRF;
              sShiftRF.Translate(dOffsetLength[0][0] * rTgtExtrusion);

              SmCurve * pCopiedGenerator = NULL ;
              pTgtCurve->Copy(*pContext, pCopiedGenerator) ; NER(pCopiedGenerator) ;
              pCopiedGenerator->Transform(sShiftRF);
              pCopiedGenerator->Trim(dTrimExtent1);

              if(pOpt3DCurves)   { pOpt3DCurves->Add(pCopiedGenerator); }
              if(pOptCurveTypes) { pOptCurveTypes->Add(SM_TC_REGION_BOUNDARY); }
            }
          else
            {
              if(pOpt3DCurves)   {
                                   SmObjsDelete<SmCurve *> sClean(pOpt3DCurves) ;
                                 }
              if(pOpt3DCurves)   { pOpt3DCurves->ReSet() ; }
              if(pOptCurveTypes) { pOptCurveTypes->ReSet() ; }

              rbNeedsMoreIntersections = TRUE ;
              return(SM_SUCCESS) ;
            }
        }
    }

  return(SM_SUCCESS) ;
}
// ---- end of the copy ----------------------------------------------------------------

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

// `n p m`, `n+1` points `x y z w`, `m+1` knots: SmBSplineCurve(gw_CURVE*, 3, FALSE).
SmBSplineCurve* read_bspline(Rd& rd, const SmContext& ctx)
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
    return new (ctx) SmBSplineCurve(cur, 3, FALSE);
}

// A generator block `kind ...`: a B-spline curve, a line `a[3] b[3]` (its NURB made), a
// circle `c[3] x[3] y[3] d0 d1 r`, an ellipse `c[3] x[3] y[3] d0 d1 rx ry`, a hyperbola
// `c[3] x[3] y[3] d0 d1 a b` or a parabola `c[3] x[3] y[3] d0 d1 f`.
SmBSplineCurve* read_gen(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    switch (kind)
    {
    case GK_BSPLINE: return read_bspline(rd, ctx);
    case GK_LINE:
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine* l = new (ctx) SmLine(a, b, 3, &ctx);
        l->GetOrCreateGwNurbPointer();
        return l;
    }
    case GK_LAZY_LINE:
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        return new (ctx) SmLine(a, b, 3, &ctx);
    }
    case GK_CIRCLE:
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double r = rd.num();
        return new (ctx) SmCircle(c, x, y, SmExtent1d(d0, d1), r, 3, &ctx);
    }
    case GK_ELLIPSE:
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double rx = rd.num();
        double ry = rd.num();
        return new (ctx) SmEllipse(c, x, y, SmExtent1d(d0, d1), rx, ry, 3, &ctx);
    }
    case GK_HYPERBOLA:
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double a = rd.num();
        double b = rd.num();
        return new (ctx) SmHyperbola(c, x, y, SmExtent1d(d0, d1), a, b, 3, &ctx);
    }
    case GK_PARABOLA:
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double f = rd.num();
        return new (ctx) SmParabola(c, x, y, SmExtent1d(d0, d1), f, 3, &ctx);
    }
    default: return NULL;
    }
}

// A surface block `mode gen oob swap` then for BM_CANONICAL `E[3]`, for BM_CTOR
// `O[3] E[3] D[4]`, for BM_INFINITE `E[3]`. Writes the construction status; NULL on
// failure.
SmSurfOfExtrusion* read_surface(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    int64_t mode = rd.idx();
    SmBSplineCurve* gen = read_gen(rd, ctx);
    bool oob = rd.flag();
    bool swap = rd.flag();
    SmSurfOfExtrusion* s = NULL;
    SmStatus stt = SM_SUCCESS;
    if (mode == BM_CANONICAL)
    {
        SmVector3d e = rd.vec();
        stt = SmSurfOfExtrusion::CreateCanonical(ctx, gen, e, s);
    }
    else if (mode == BM_CTOR)
    {
        SmPoint3d org = rd.vec();
        SmVector3d e = rd.vec();
        SmExtent2d d = rd.domain();
        s = new (ctx) Peek(gen, org, e, d, swap ? TRUE : FALSE);
        stt = s->MakeNurb();
    }
    else
    {
        SmVector3d e = rd.vec();
        s = new (ctx) Peek(gen, e, swap ? TRUE : FALSE);
        stt = s->MakeNurb();
    }
    o.push_back(st(stt));
    if (stt != SM_SUCCESS)
    {
        if (s)
            delete s; // owns gen
        else
            delete gen;
        return NULL;
    }
    s->SetOutOfBoundsEnabled(oob ? TRUE : FALSE); // the surface and its generator
    return s;
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

// The surface state.
void put_state(std::vector<double>& o, SmSurfOfExtrusion* s)
{
    o.push_back(flg(s->IsKindOf(SmSurfOfExtrusion_TYPE)));
    o.push_back(flg(s->IsKindOf(SmBSplineSurface_TYPE)));
    o.push_back(flg(s->IsKindOf(SmSurface_TYPE)));
    o.push_back((double)s->GetType());
    o.push_back(flg(s->GetSwapUVBit()));
    o.push_back(flg(s->IsOutOfBoundsEnabled()));
    o.push_back((double)(s->*Peek::orient()));
    put_vec(o, s->GetOrigin());
    put_vec(o, s->GetExtrusionVector());
    put_vec(o, s->*Peek::cpoint());
    put_vec(o, s->*Peek::cvec());
    put_dom(o, s->GetSTEPUVDomain());
    SmSurfParamType lp = SM_SP_UNKNOWN;
    SmExtent1d ld = s->GetSTEPLinearParamExtent(&lp);
    o.push_back(ld.GetMin());
    o.push_back(ld.GetMax());
    o.push_back((double)lp);
    SmSurfParamType gp = SM_SP_UNKNOWN;
    SmExtent1d gd = s->GetGenDirParamExtent(&gp);
    o.push_back(gd.GetMin());
    o.push_back(gd.GetMax());
    o.push_back((double)gp);
    put_dom(o, s->GetMaxAnalyticDomain());
    SmBSplineCurve* gc = NULL;
    SmVector3d ev(0.0, 0.0, 0.0);
    s->GetCanonical(gc, ev);
    o.push_back(flg(gc == s->GetGenCurve()));
    put_vec(o, ev);
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

// An op that edits the surface: its status, then on success the state.
void edited(std::vector<double>& o, SmStatus stt, SmSurfOfExtrusion* s)
{
    o.push_back(st(stt));
    if (stt == SM_SUCCESS)
        put_state(o, s);
}

void surface_op(int32_t op, Rd& rd, const SmContext& ctx, SmSurfOfExtrusion* s,
                std::vector<double>& o)
{
    switch (op)
    {
    case OP_BUILD: put_state(o, s); break;
    case OP_EVAL_STEP:
    {
        double u = rd.num();
        double v = rd.num();
        ULONG k = (ULONG)rd.idx();
        bool point = rd.flag();
        if (point)
        {
            SmPoint3d p(0.0, 0.0, 0.0);
            SmStatus stt = s->EvaluateSTEPPoint(SmPoint2d(u, v), p);
            o.push_back(st(stt));
            if (stt == SM_SUCCESS)
                put_vec(o, p);
            break;
        }
        std::vector<SmVector3d> d((k + 1) * (k + 1), SmVector3d(0.0, 0.0, 0.0));
        SmStatus stt = s->EvaluateSTEP(SmPoint2d(u, v), k, k, TRUE, TRUE, TRUE, d.data());
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            for (size_t i = 0; i < d.size(); i++)
                put_vec(o, d[i]);
        break;
    }
    case OP_TO_NURBS:
    case OP_TO_STEP:
    {
        double u = rd.num();
        double v = rd.num();
        SmPoint2d r(0.0, 0.0);
        SmStatus stt = op == OP_TO_NURBS ? s->ConvertUVFromSTEPToNURBS(SmPoint2d(u, v), r)
                                         : s->ConvertUVFromNURBSToSTEP(SmPoint2d(u, v), r);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(r.x);
            o.push_back(r.y);
        }
        break;
    }
    case OP_ADJUST:
    {
        SmExtent2d d = rd.domain();
        edited(o, s->AdjustSTEPUVDomain(d), s);
        break;
    }
    case OP_UPDATE:
    {
        SmExtent2d d = rd.domain();
        edited(o, s->UpdateAnalyticalDomain(d), s);
        break;
    }
    case OP_REPARAM:
    {
        SmExtent2d d = rd.domain();
        edited(o, s->Reparameterize(d), s);
        break;
    }
    case OP_TRANSFORM:
    {
        SmPoint3d po = rd.vec();
        SmVector3d px = rd.vec();
        SmVector3d py = rd.vec();
        bool has_scale = rd.flag();
        SmVector3d sc = rd.vec();
        SmAxis2Placement rf(po, px, py);
        edited(o, s->Transform(rf, has_scale ? &sc : NULL), s);
        break;
    }
    case OP_DIRECTRIX:
    {
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        SmBSplineCurve* c = NULL;
        SmStatus stt = s->CreateDirectrixFromPoint(ctx, p, tol, c);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_curve(o, c);
        delete c;
        break;
    }
    case OP_DROP_FAST:
    {
        SmExtent2d d = rd.domain();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        SmPoint3d p = rd.vec();
        bool has_vec = rd.flag();
        SmVector3d iv = rd.vec();
        double tol = rd.num();
        bool has_target = rd.flag();
        double target = rd.num();
        SmSolutionRequestedType req = rd.idx() == 0 ? SM_SR_SINGLE : SM_SR_ALL;
        SmSolutionArray sols;
        SmStatus stt = s->DropPointFast(d, sop, p, has_vec ? &iv : NULL, tol,
                                        has_target ? &target : NULL, req, sols);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back((double)sols.GetSize());
            for (ULONG i = 0; i < sols.GetSize(); i++)
            {
                const SmSolution& r = sols[i];
                o.push_back((double)r.m_eSolutionType);
                o.push_back((double)r.m_lNumVariables);
                o.push_back((double)r.m_lNumObjects);
                o.push_back(flg(r.m_apObjects[0] == (SmObject*)s));
                o.push_back(r.m_vStart.m_dSolutionValue);
                o.push_back(r.m_vStart[0]);
                o.push_back(r.m_vStart[1]);
            }
        }
        break;
    }
    case OP_STEP_INV:
    {
        SmExtent2d d = rd.domain();
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        bool has_guess = rd.flag();
        double gx = rd.num();
        double gy = rd.num();
        SmPoint2d guess(gx, gy);
        SmPoint2d uv(0.0, 0.0);
        SmLocationType loc = SM_LT_EXTERIOR;
        SmStatus stt = s->STEPInversion(d, p, tol, uv, loc, has_guess ? &guess : NULL);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(uv.x);
            o.push_back(uv.y);
            o.push_back((double)loc);
        }
        break;
    }
    case OP_EQ:
    {
        std::vector<double> o2;
        SmSurfOfExtrusion* other = read_surface(rd, ctx, o2);
        o.push_back(o2.empty() ? -1.0 : o2[0]);
        if (other)
        {
            o.push_back(flg(*s == *other));
            o.push_back(flg(*other == *s));
            delete other;
        }
        break;
    }
    case OP_COPY:
    {
        SmSurface* c = NULL;
        SmStatus stt = s->Copy(ctx, c);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS && c)
        {
            SmSurfOfExtrusion* e = (SmSurfOfExtrusion*)c;
            o.push_back(flg(*s == *e));
            put_state(o, e);
        }
        delete c;
        break;
    }
    case OP_GENERATORS:
    {
        SmBSplineCurve* other = read_gen(rd, ctx);
        SmVector3d ov = rd.vec();
        double i0 = rd.num();
        double i1 = rd.num();
        double tol = rd.num();
        SmExtent1d oi(i0, i1);
        SmTArray<SmCurve*> curves;
        SmTArray<SmTsectCurveType> types;
        SmBoolean more = FALSE;
        SmStatus stt = sm_IntersectGenerators(*s, tol, *other, ov, oi, &curves, &types, more);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(flg(more));
            o.push_back((double)curves.GetSize());
            for (ULONG i = 0; i < curves.GetSize(); i++)
                put_curve(o, curves[i]);
            o.push_back((double)types.GetSize());
            for (ULONG i = 0; i < types.GetSize(); i++)
                o.push_back((double)types[i]);
        }
        for (ULONG i = 0; i < curves.GetSize(); i++)
            delete curves[i];
        delete other;
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_surface_surf_of_extrusion_p1_eval(int32_t op, const double* in,
                                                       int64_t n_in, double* out, int64_t cap,
                                                       int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    SmSurfOfExtrusion* s = read_surface(rd, ctx, o);
    if (s)
    {
        surface_op(op, rd, ctx, s, o);
        delete s;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
