// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nurbs.b_spline_surface_n_lib_p1`.
// Wraps the SmBSplineSurface members owned by the unit (source/SMLib/src/SmBSplineSurfaceNLib.cpp):
// ApproximateOffsetSurface, CreateCoonsPatch (with the static sm_CreateCoonsSrf),
// CreateExtendedSurface (both overloads), JoinSurface, CreateNSidedPatch, CreateRuledSurface
// (with the static sm_BuildNlibCurve), CreateSweepSurface, CreateSwungSurface,
// EvaluatePointOnExtendedSurface, Reparameterize, Reverse, ExpandBoundarySpan, EvaluateSimple
// and EvaluatePoint.
//
// The file-static helpers sm_CreateNonRationalCurve, sm_MakeKnots, sm_CheckTrimBoundaries and
// sm_check_zero_surface_derivs cannot be linked against and the file defines class members, so it
// cannot be compiled inside a namespace (see geom_geom_utility_p1_static.cpp). They are copied
// below verbatim (upstream commit c9c5979) into namespace sc_bssnl_p1_tu and compiled with the
// kernel's flags. (sm_CreateNlibCurve / sm_CreateNlibSurface are plain copies, exercised by
// later units.)
//
// One entry point, sc_nurbs_b_spline_surface_n_lib_p1_eval(op, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/nurbs_b_spline_surface_n_lib_p1.rs,
// which validates every input so upstream only touches valid memory. `out` receives the op
// status (0 = SM_SUCCESS, else the SmStatus value) and the op's outputs (only on success). The
// input curves and surfaces NLib changes in place (Coons, n-sided patch, join) are never read
// after the call: upstream may leave them pointing at freed stack memory.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAssertArray.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmGeomUtility.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <NL_CrvAdv.h>
#include <NL_SrfAdv.h>

#include <vector>

#include "sc_sm.h"

using sc::flg;
using sc::put_vec;
using sc::st;

namespace sc_bssnl_p1_tu
{
// ---- verbatim: upstream source/SMLib/src/SmBSplineSurfaceNLib.cpp lines 2906-2970 ----
static SmStatus sm_CreateNonRationalCurve
  (const SmContext & crContext,        // in : context for new object construction
   SmCurve         * pCurve,           // in : curve to approximate
   SmBSplineCurve  * pOptUVTrimCurve,  // in : optional UVTrimCurve, copied to rpNewUVTimrCurve whenpCurve is nonRational
   SmSurface       * pOptBaseSurface,  // in : optional base surface, used for DropCurve when pCurve is Rational
   SmApproxTol3d     dApproxTol,       // in : max allowed deviation from NewCurve to 
   SmBSplineCurve *& rpNewCurve,       // out: non-rational approx of input pCurve
   SmBSplineCurve *& rpNewUVTrimCurve) // out: associated UVTrimCurve when
                                       //         (pCurve->IsRational && pOptBaseSurface)
                                       //      or (pCurve->NotRation && pOptUVTrimCurve)
                                       //      NULL otherwise
{
  // init output
  SmBSplineCurve *pNewCurve   = NULL;
  SmBSplineCurve *pNewUVTrimCurve = NULL;

  // locals
  SmBSplineCurve *pNurb = SM_CAST_PTR(SmBSplineCurve,pCurve);
  NER(pNurb);

  // for rational curves
  if (pNurb->IsRational()) 
    {
      // Build arc-length parameterized nonRational curve approximation
      SER(pNurb->ApproximateRationalCurve(crContext,dApproxTol,pNewCurve));
      SER(pNewCurve->ReparametrizeWithArcLength());

      // When given optional UVTrimCurve - build arc-lentgh nonRational UVTrimCurve approximation
      if(pOptBaseSurface)
        {
          SM_ASSERT(pOptBaseSurface != NULL) ;
          SmExtent1d sTrimIvl = pNewCurve->GetNaturalInterval();
          SmExtent2d sDomain  = pOptBaseSurface->GetNaturalUVDomain();
          double dMaxDist;
          double dDeviation;
          SmTArray<SmBSplineCurve*> sUVCurves;
          SER(pOptBaseSurface->DropCurve(crContext,sDomain,*pNewCurve,
                                         sTrimIvl,
                                         dApproxTol,
                                         dMaxDist,    // out: MaxDist(DropPt, 3dCurvePt), found by sampling, may be slightly less than actual max.
                                         dDeviation,  // out: MaxDist(DropPt->SrfNormalLine, 3dCurvePt), Quality Measure: expected to be close to 0.0
                                         sUVCurves));
          SM_ASSERT(sUVCurves.GetSize() == 1);
          pNewUVTrimCurve = sUVCurves[0];
        }
    } // end rational curve branch
  else // nonRational curve branch
    {
      // make a curve copy and optional UVTrimCurve copy
      pNewCurve = new(crContext) SmBSplineCurve(*pNurb);
      if (pOptUVTrimCurve) 
        {
          SM_ASSERT(pOptUVTrimCurve->IsRational() == FALSE) ;
          pNewUVTrimCurve = new(crContext) SmBSplineCurve(*pOptUVTrimCurve);
        }
    }

  // set output
  rpNewCurve       = pNewCurve;
  rpNewUVTrimCurve = pNewUVTrimCurve;

  // all done
  return SM_SUCCESS;

} // end sm_CreateNonRationalCurve 

// ---- verbatim: upstream source/SMLib/src/SmBSplineSurfaceNLib.cpp lines 5191-5249 ----
static void sm_MakeKnots
  (NL_REAL *u, 
   NL_INDEX n, 
   NL_DEGREE p, 
   NL_KNOTVECTOR *knt )
{
  NL_INDEX   i, j, k;
  NL_REAL    *U, sum, dSize, dBack, dForward;
  NL_STACKS  SL;

  /* Start NURBS */
  N_InitNurbs(&SL);

  /* Extract the knots */
  N_KnotVectorGetKnots(knt,&i,&U);

  /* Compute knot vector.
   *  Full multiplicity (p+1) on the ends,
   *  and Greville abscissae on the interior.
   */

  for( i=0; i<=p; i++ )
  {
    U[  i  ] = u[0];
    U[n+i+1] = u[n];
  }

  for( i=1; i<=n-p; i++ )
  {
      sum = 0.0;
      for( j=i; j<=i+p-1; j++ )  sum += u[j];
      U[i+p] = sum/p;
      /* GAC - added following loop to clamp U to knots */
      /* bd: not sure we want to do this, Greville abscissae are appropriate. */
      /*     But it's never hit in prog_test. */
      for (k=1; k<=n; k++) {
          if (U[i+p] < u[k]) {
              dSize = u[k]-u[k-1];
              dBack = U[i+p] - u[k-1];
              dForward = u[k] - U[i+p];
              double dTest = U[i+p];
              if (dBack < dSize/3.0) {
                  dTest = u[k-1];
              }
              else if (dForward < dSize/3.0) {
                  dTest = u[k];
              }
              // Try our best to avoid triple knots
              if (SM_ARE_SAME(dTest,U[i+p-1]) &&
                  SM_ARE_SAME(dTest,U[i+p-2])) {
                  dTest = (u[k]-u[k-1])/2.0;
              }
              U[i+p] = dTest;
              break;
          }
      }
  }

} // end sm_MakeKnots

// ---- verbatim: upstream source/SMLib/src/SmBSplineSurfaceNLib.cpp lines 7349-7410 ----
static void sm_CheckTrimBoundaries
 (const SmTArray<double> & rKnots,   // in : knot array to check
  double                 & dTMin,    // i/o: proposed min trim boundary, can't be inside the domain.
  int                      lMinIdx,  // in : index into rKnots of dTMin, negative if not set.
  double                 & dTMax,    // i/o: proposed max trim boundary
  int                      lMaxIdx,  // in : index into rKnots of dTMax, negative if not set.
  const SmExtent2d       & rDomain,  // in : dTMin and dTMax must not be interior to this domain.
  SmSurfParamType          eUVDir)   // in : 
{
  // locals
  int   ii ;
  ULONG li ;

  // pick knot direction
  SmExtent1d sDom =   (eUVDir == SM_SP_U) 
                    ? rDomain.GetUInterval() 
                    : rDomain.GetVInterval() ;

  // When min knot index and TMin are in the interior of sDom
  if(   lMinIdx >= 0 
     && dTMin   > sDom.GetMin() )
    {
      // iter every knot looking for 1st knot not within current sDom interval 
      for(ii=lMinIdx;ii>=0;ii--) 
        {
          // found 1st knot outside of sDom interval
          if(rKnots[ii] <= sDom.GetMin())
            {
              dTMin = rKnots[ii];
              break;
            }
        }

      // before the beginning
      if ( ii < 0 )
        { dTMin = rKnots[ 0 ]; } 
    } // end minKnotIndx and Tmin are inside of sDom check

  // when max knot index and tMax are in the interior of sDom
  if(   lMaxIdx >= 0 
     && dTMax < sDom.GetMax() )
    {
      // iter every knot looking for 1st knot not within curren sDom interval
      for(li=lMaxIdx;li<rKnots.GetSize();li++)
        {
          // find 1st knot outside of sDom interval
          if(rKnots[li] >= sDom.GetMax())
            {
              dTMax = rKnots[li];
              break;
            }
        }

      // past the end
      if ( li >= rKnots.GetSize() )
        { dTMax = rKnots[ rKnots.GetSize()-1 ]; }  
    } // end maxKnotIndx and Tmax are inside of sDom check

  // all done
  return;

} // end local sm_CheckTrimBoundaries()

// ---- verbatim: upstream source/SMLib/src/SmBSplineSurfaceNLib.cpp lines 7982-8074 ----
static SmBoolean sm_check_zero_surface_derivs
 (const SmPoint2d  &rUV,     // in :
  const SmExtent2d &rDomain, // in :
  const SmPoint3d  &rPt,     // NotUsed: in : pos of tgt eval - only used to set tolerance values
        SmVector3d &rSu,     // i/o: changed from zero length, if possible
        SmVector3d &rSv,     // i/o:
  const SmVector3d &rSuu,    // in :
  const SmVector3d &rSuv,    // in : mixed partial
  const SmVector3d &rSvv,    // in :
  const double      dScaledZero)    // in : pass in so it will be consistent.
{
  SM_REF1(rPt) ;
  SmBoolean bRetVal    = TRUE;

  double dNewLength = 1.1 * dScaledZero; // length of output vectors.
  double dLen;

  // The mixed partial (Suv) is the first derivative of both Su and Sv,
  // with respect to the other, and hence can be used for either one
  // if it's not zero.  Try that first.

  // If we're at the 'top' of the domain, i.e., the 'good' parameter is
  // entering a singularity instead of leaving it, then the degenerate
  // derivative is shrinking to zero, so its change is in the opposite
  // direction of its derivative.
  // If the derivative (of the derivative) is its u-derivative (which would
  // be the case when using Suv for Sv, or Suu for Su), then we check whether
  // the u value is at the top or bottom of its domain.
  SmBoolean bFlip = FALSE;

  if ( rSu.Length() < dScaledZero )
    {
      bRetVal = FALSE;

      // first check how Su is changing with v
      dLen = rSuv.Length();
      if ( dLen > dScaledZero )
        {
          rSu = rSuv * (dNewLength / dLen) ;
          bFlip = rUV.y > rDomain.GetVInterval().GetMid(); // v-deriv of Su
          bRetVal = TRUE;
        }
      else
        {
          // try change w.r.t. u
          dLen = rSuu.Length();
          if ( dLen > dScaledZero )
            {
              rSu = rSuu * (dNewLength / dLen) ;
              bFlip = rUV.x > rDomain.GetUInterval().GetMid(); // u-deriv of Su
              bRetVal = TRUE;
            }
        }

        if ( bRetVal == TRUE && bFlip )
        {
            rSu *= -1;
        }
    }

  if ( rSv.Length() < dScaledZero )
    {
      bRetVal = FALSE;

      // first check how Sv is changing with u
      dLen = rSuv.Length();
      if ( dLen > dScaledZero )
        {
          rSv = rSuv * (dNewLength / dLen) ;
          bFlip = rUV.x > rDomain.GetUInterval().GetMid(); // u-deriv of Sv
          bRetVal = TRUE;
        }
      else
        {
          // try change w.r.t. v
          dLen = rSvv.Length();
          if ( dLen > dScaledZero )
            {
              rSv = rSvv * (dNewLength / dLen) ;
              bFlip = rUV.y > rDomain.GetVInterval().GetMid(); // v-deriv of Sv
              bRetVal = TRUE;
            }
        }

        if ( bRetVal == TRUE && bFlip )
        {
            rSv *= -1;
        }
    }

  return bRetVal;

}  // end local sm_check_zero_surface_derivs

} // namespace sc_bssnl_p1_tu

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/nurbs_b_spline_surface_n_lib_p1.rs).
enum : int32_t
{
    // surface ops (`this` from a surface block)
    OP_OFFSET = 0,
    OP_EXTEND = 1,
    OP_JOIN = 2,
    OP_EXT_EVAL = 3,
    OP_REPARAM = 4,
    OP_REVERSE = 5,
    OP_EXPAND = 6,
    OP_EVAL_SIMPLE = 7,
    OP_EVAL_POINT = 8,
    // static ops
    OP_COONS = 20,
    OP_NSIDED = 21,
    OP_RULED = 22,
    OP_SWEEP = 23,
    OP_SWUNG = 24,
    OP_NONRAT = 25,
    OP_MAKE_KNOTS = 26,
    OP_TRIM_BOUNDS = 27,
    OP_ZERO_DERIVS = 28,
};

// Surface constructions of the surface block.
enum : int32_t
{
    CT_NONE = 0,       // static op, no surface
    CT_EMPTY = 1,      // SmBSplineSurface()
    CT_FAST_OWNED = 2, // SmBSplineSurface(gw_SURFACE*, FALSE, NULL)
};

// Reads the protected STEP flags through pointers to members named in a derived class.
struct Peek : SmBSplineSurface
{
    static SmKnotType SmBSplineSurface::*kt() { return &Peek::m_eKnotType; }
    static SmBSplineSurfaceForm SmBSplineSurface::*form() { return &Peek::m_eBSplineSurfaceForm; }
};

struct PeekC : SmBSplineCurve
{
    static SmKnotType SmBSplineCurve::*kt() { return &PeekC::m_eKnotType; }
    static SmBSplineCurveForm SmBSplineCurve::*form() { return &PeekC::m_eBSplineCurveForm; }
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
    SmPoint2d pt2()
    {
        double x = num();
        double y = num();
        return SmPoint2d(x, y);
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

// Reads `n m p q r s cpts u v` into a surface from sm_AllocateNurbSurface.
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

// Builds a surface from a block `ctor oob form [surface]` (ctor 1 or 2).
SmBSplineSurface* read_surface(Rd& rd, const SmContext& ctx, int32_t ctor)
{
    bool oob = rd.flag();
    SmBSplineSurfaceForm form = (SmBSplineSurfaceForm)rd.idx();
    SmBSplineSurface* s = NULL;
    switch (ctor)
    {
    case CT_EMPTY: s = new (ctx) SmBSplineSurface(); break;
    case CT_FAST_OWNED: s = new (ctx) SmBSplineSurface(read_nurb(rd), FALSE, NULL); break;
    default: return NULL;
    }
    s->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
    s->SetBSplineSurfaceForm(form);
    return s;
}

void put_state(std::vector<double>& o, const SmBSplineSurface* s)
{
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

// Status, then (on success) the new surface's state; deletes it.
void put_new(std::vector<double>& o, SmStatus s, SmSurface* c)
{
    o.push_back(st(s));
    if (s == SM_SUCCESS)
    {
        o.push_back(flg(c != NULL));
        if (c)
            put_state(o, (SmBSplineSurface*)c);
    }
    delete c;
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

void put_curve(std::vector<double>& o, const SmBSplineCurve* c)
{
    o.push_back((double)c->GetDim());
    o.push_back(flg(c->IsOutOfBoundsEnabled()));
    o.push_back(flg(c->IsNurbBorrowed()));
    o.push_back((double)(c->*PeekC::kt()));
    o.push_back((double)(c->*PeekC::form()));
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

void surface_op(int32_t op, Rd& rd, const SmContext& ctx, SmBSplineSurface* s, std::vector<double>& o)
{
    switch (op)
    {
    case OP_OFFSET:
    {
        double d = rd.num();
        double tol = rd.num();
        int pt = (int)rd.idx();
        SmBSplineSurface* r = NULL;
        SmStatus stt = s->ApproximateOffsetSurface(ctx, d, tol, r, pt);
        put_new(o, stt, r);
        break;
    }
    case OP_EXTEND:
    {
        bool dir_overload = rd.flag();
        SmSurfParamType dir = (SmSurfParamType)rd.idx();
        double dist = rd.num();
        SmContinuityType cont = (SmContinuityType)rd.idx();
        SmSurface* r = NULL;
        SmStatus stt = dir_overload ? s->CreateExtendedSurface(ctx, dir, dist, cont, r)
                                    : s->CreateExtendedSurface(ctx, dist, cont, r);
        put_new(o, stt, r);
        break;
    }
    case OP_JOIN:
    {
        SmBSplineSurface* other = new (ctx) SmBSplineSurface(read_nurb(rd), FALSE, NULL);
        SmSurfParamType dir = (SmSurfParamType)rd.idx();
        double tol = rd.num();
        SmBSplineSurface* r = NULL;
        SmStatus stt = s->JoinSurface(ctx, other, dir, tol, r);
        put_new(o, stt, r);
        delete other;
        break;
    }
    case OP_EXT_EVAL:
    {
        SmPoint2d uv = rd.pt2();
        SmPoint3d p(0.0, 0.0, 0.0);
        SmStatus stt = s->EvaluatePointOnExtendedSurface(uv, p);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_vec(o, p);
        break;
    }
    case OP_REPARAM:
    {
        SmExtent2d dom = rd.domain();
        SmStatus stt = s->Reparameterize(dom);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_state(o, s);
        break;
    }
    case OP_REVERSE:
    {
        SmSurfParamType dir = (SmSurfParamType)rd.idx();
        SmStatus stt = s->Reverse(dir);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_state(o, s);
        break;
    }
    case OP_EXPAND:
    {
        double par = rd.num();
        SmSurfParamType dir = (SmSurfParamType)rd.idx();
        SmStatus stt = s->ExpandBoundarySpan(par, dir);
        o.push_back(st(stt));
        put_state(o, s);
        break;
    }
    case OP_EVAL_SIMPLE:
    {
        SmPoint2d uv = rd.pt2();
        ULONG hu = (ULONG)rd.idx();
        ULONG hv = (ULONG)rd.idx();
        bool ul = rd.flag();
        bool vl = rd.flag();
        bool upper = rd.flag();
        std::vector<SmVector3d> d((hu + 1) * (hv + 1), SmVector3d(0.0, 0.0, 0.0));
        SmStatus stt = s->EvaluateSimple(uv, hu, hv, ul ? TRUE : FALSE, vl ? TRUE : FALSE,
                                         upper ? TRUE : FALSE, d.data());
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            for (size_t i = 0; i < d.size(); i++)
                put_vec(o, d[i]);
        break;
    }
    case OP_EVAL_POINT:
    {
        SmPoint2d uv = rd.pt2();
        SmPoint3d p(0.0, 0.0, 0.0);
        SmStatus stt = s->EvaluatePoint(uv, p);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_vec(o, p);
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}

void static_op(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_COONS:
    {
        double tol = rd.num();
        bool bilinear = rd.flag();
        SmTArray<SmBSplineCurve*> bdry, ders;
        for (int i = 0; i < 4; i++)
            bdry.Add(read_curve(rd, ctx));
        int64_t nd = rd.idx();
        for (int64_t i = 0; i < nd; i++)
            ders.Add(read_curve(rd, ctx));
        SmBSplineSurface* r = NULL;
        SmStatus stt = SmBSplineSurface::CreateCoonsPatch(ctx, tol, bilinear ? TRUE : FALSE, bdry,
                                                          ders, r);
        put_new(o, stt, r);
        for (ULONG i = 0; i < bdry.GetSize(); i++)
            delete bdry[i];
        for (ULONG i = 0; i < ders.GetSize(); i++)
            delete ders[i];
        break;
    }
    case OP_NSIDED:
    {
        int64_t k = rd.idx();
        SmTArray<SmBSplineCurve*> bdry, ders;
        for (int64_t i = 0; i < k; i++)
            bdry.Add(read_curve(rd, ctx));
        for (int64_t i = 0; i < k; i++)
            ders.Add(rd.flag() ? read_curve(rd, ctx) : (SmBSplineCurve*)NULL);
        double eps = rd.num();
        bool has_center = rd.flag();
        SmPoint3d ci = rd.vec();
        SmVector3d ni = rd.vec();
        SmTArray<SmSurface*> surfs;
        SmStatus stt = SmBSplineSurface::CreateNSidedPatch(ctx, bdry, ders, eps, surfs,
                                                           has_center ? &ci : NULL,
                                                           has_center ? &ni : NULL);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back((double)surfs.GetSize());
            for (ULONG i = 0; i < surfs.GetSize(); i++)
                put_state(o, (SmBSplineSurface*)surfs[i]);
            put_vec(o, ni);
        }
        for (ULONG i = 0; i < surfs.GetSize(); i++)
            delete surfs[i];
        for (ULONG i = 0; i < bdry.GetSize(); i++)
            delete bdry[i];
        for (ULONG i = 0; i < ders.GetSize(); i++)
            delete ders[i];
        break;
    }
    case OP_RULED:
    {
        SmBSplineCurve* c1 = read_curve(rd, ctx);
        SmBSplineCurve* c2 = read_curve(rd, ctx);
        SmSurfParamType dir = (SmSurfParamType)rd.idx();
        SmBSplineSurface* r = NULL;
        SmStatus stt = SmBSplineSurface::CreateRuledSurface(ctx, *c1, *c2, dir, r);
        put_new(o, stt, r);
        delete c1;
        delete c2;
        break;
    }
    case OP_SWEEP:
    {
        SmBSplineCurve* sec = read_curve(rd, ctx);
        SmBSplineCurve* traj = read_curve(rd, ctx);
        SmBSplineCurve* scale = rd.flag() ? read_curve(rd, ctx) : NULL;
        bool trans = rd.flag();
        double tol = rd.num();
        SmBSplineSurface* r = NULL;
        SmStatus stt = SmBSplineSurface::CreateSweepSurface(ctx, sec, traj, scale,
                                                            trans ? TRUE : FALSE, tol, r);
        put_new(o, stt, r);
        delete sec;
        delete traj;
        delete scale;
        break;
    }
    case OP_SWUNG:
    {
        SmBSplineCurve* sec = read_curve(rd, ctx);
        SmBSplineCurve* traj = read_curve(rd, ctx);
        double scale = rd.num();
        SmBSplineSurface* r = NULL;
        SmStatus stt = SmBSplineSurface::CreateSwungSurface(ctx, sec, traj, scale, r);
        put_new(o, stt, r);
        delete sec;
        delete traj;
        break;
    }
    case OP_NONRAT:
    {
        SmBSplineCurve* c = read_curve(rd, ctx);
        SmBSplineCurve* uv = rd.flag() ? read_curve(rd, ctx) : NULL;
        SmBSplineSurface* base =
            rd.flag() ? new (ctx) SmBSplineSurface(read_nurb(rd), FALSE, NULL) : NULL;
        double tol = rd.num();
        SmBSplineCurve* nc = NULL;
        SmBSplineCurve* nuv = NULL;
        SmStatus stt = sc_bssnl_p1_tu::sm_CreateNonRationalCurve(ctx, c, uv, base,
                                                                 SmApproxTol3d(tol), nc, nuv);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(flg(nc != NULL));
            if (nc)
                put_curve(o, nc);
            o.push_back(flg(nuv != NULL));
            if (nuv)
                put_curve(o, nuv);
        }
        delete nc;
        delete nuv;
        delete c;
        delete uv;
        delete base;
        break;
    }
    case OP_MAKE_KNOTS:
    {
        int64_t n = rd.idx();
        int64_t p = rd.idx();
        std::vector<double> u(n + 1);
        for (int64_t i = 0; i <= n; i++)
            u[i] = rd.num();
        std::vector<double> kv(n + p + 2, 0.0);
        NL_KNOTVECTOR knt;
        knt.m = (NL_INDEX)(n + p + 1);
        knt.U = kv.data();
        sc_bssnl_p1_tu::sm_MakeKnots(u.data(), (NL_INDEX)n, (NL_DEGREE)p, &knt);
        o.push_back(0.0);
        for (int64_t i = 0; i <= n + p + 1; i++)
            o.push_back(kv[i]);
        break;
    }
    case OP_TRIM_BOUNDS:
    {
        int64_t nk = rd.idx();
        SmTArray<double> knots;
        for (int64_t i = 0; i < nk; i++)
            knots.Add(rd.num());
        double tmin = rd.num();
        int min_idx = (int)rd.idx() - 1; // stored + 1 so that "unset" (-1) is 0
        double tmax = rd.num();
        int max_idx = (int)rd.idx() - 1;
        SmExtent2d dom = rd.domain();
        SmSurfParamType dir = (SmSurfParamType)rd.idx();
        sc_bssnl_p1_tu::sm_CheckTrimBoundaries(knots, tmin, min_idx, tmax, max_idx, dom, dir);
        o.push_back(0.0);
        o.push_back(tmin);
        o.push_back(tmax);
        break;
    }
    case OP_ZERO_DERIVS:
    {
        SmPoint2d uv = rd.pt2();
        SmExtent2d dom = rd.domain();
        SmPoint3d pt = rd.vec();
        SmVector3d su = rd.vec();
        SmVector3d sv = rd.vec();
        SmVector3d suu = rd.vec();
        SmVector3d suv = rd.vec();
        SmVector3d svv = rd.vec();
        double sz = rd.num();
        SmBoolean ok =
            sc_bssnl_p1_tu::sm_check_zero_surface_derivs(uv, dom, pt, su, sv, suu, suv, svv, sz);
        o.push_back(0.0);
        o.push_back(flg(ok != FALSE));
        put_vec(o, su);
        put_vec(o, sv);
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_nurbs_b_spline_surface_n_lib_p1_eval(int32_t op, const double* in,
                                                          int64_t n_in, double* out, int64_t cap,
                                                          int64_t* out_len)
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
        SmBSplineSurface* s = read_surface(rd, ctx, ctor);
        if (s == NULL)
            return SC_ERR_ARG;
        surface_op(op, rd, ctx, s, o);
        // JoinSurface may leave `this` pointing at freed stack memory: never read it
        // again (deleting only frees the surface's own block).
        delete s;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
