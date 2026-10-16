// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.composite_curve_p1`: SmCompositeCurve part 1
// (source/SMLib/src/SmCompositeCurve.cpp, source/SMLib/inc/SmCompositeCurve.h): the
// constructors, type queries, GetCurveSegment, GetCanonical, GetNaturalInterval,
// GetKnots, CalculateBoundingBox, operator==, Copy, Transform, ApproximateCurve (with
// the file-static sm_convert_to_BSplineCurves), ComputeProjectedLoopOrientation,
// BuildCompositesFromCurves (with the file-static sm_FlushCrv), the type queries of
// SmCompositeCurveSegment, SmCompositeCurveRegion and SmOffsetMapAttribute, and verbatim
// copies of the file-static sm_CreateCorner, sm_CurveClassify, sm_TrimFirstBreak,
// sm_TrimLastBreak and sm_InsertSortedBreaks (the copies' only change: sm_CreateCorner
// returns SM_ERR_INVALID_INPUT instead of reading sSolutions[0] of an empty array).
//
// One entry point, sc_curve_composite_curve_p1_eval(op, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/curve_composite_curve_p1.rs,
// which validates every input. A curve block is `kind dim ...` (0: SmBSplineCurve `oob n
// p m cpts(x y z w) knots`; 1: SmHermiteCurve `P1 D1 P2 D2`; 2: bounded SmLine `P V d0
// d1 scale io`), as in unit curve.curve_p6. A composite block is `dim n curve*n closed
// has_senses sense*n has_gaps gap*n`. `out` receives the op status (0 = SM_SUCCESS, else
// the SmStatus value) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmCompositeCurve.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveTypes.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmGeomUtility.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmPolarBox.h>
#include <SmPseudoBox.h>
#include <SmSolutionArray.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <set>
#include <vector>

#include "sc_sm.h"
#include "sc_gw.h"

using sc::Rd;
using sc::put_vec;
using sc::read_nurb;

namespace
{
// ---- verbatim copies of file-static upstream code (SmCompositeCurve.cpp) ------------

static SmStatus sm_CreateCorner
  (const SmContext   & crContext,       // in : context for new object construction
   const SmPoint3d   & crVertexPoint,   // in : Vertex point of origin curve ends,
                                        //        only used for SM_OC_FILLET_CORNER as center of new fillet corner.
   const SmPoint3d   & crP1,            // in : position at end of offset segment ending at the corner
   const SmVector3d  & crV1,            // in : tangent  at end of offset segment ending at the corner
   const SmPoint3d   & crP2,            // in : position at beg of offset segment beginning at the corner
   const SmVector3d  & crV2,            // in : tangent  at beg of offset segment beginning at the corner
   SmOffsetCornerType  eOffsetCorner,   // in : SM_OC_LINEAR_EXTENSION: corner = 2 lines from given ends to common linear extension xsect point.
                                        //      SM_OC_FILLET_CORNER: corner = fillet centered on crVertexPoint running to given end points
                                        //      SM_OC_LINEAR_CHAMFER: corner = line between endpoints crP1 and crP2 (result is within offset distance)
   const double dOffsetDistance,        // in : only used for SM_OC_LINEAR_CHAMFER
   SmXSectTol3d        dXSectTol3d,     // in : XSect tolerance used in SM_OC_LINEAR_CHAMFER. NULL to ignore
   SmCurve          *& rpCornerCurve1,  // out: SM_OC_LINEAR_EXTENSION: line from crP1 to xsect point,
   SmCurve          *& rpCornerCurve2,  // out: SM_OC_LINEAR_EXTENSION: line from xsect point to crP2,
   SmCurve          *& rpCornerCurve3)  // out: SM_OC_LINEAR_EXTENSION: NULL
{
  // init output
  rpCornerCurve1 = NULL;
  rpCornerCurve2 = NULL;
  rpCornerCurve3 = NULL;

  if(eOffsetCorner == SM_OC_LINEAR_EXTENSION )
    {
      double dT1 = 0.0;
      double dT2 = 0.0;
      if    (  crV1.LengthSquared() > SM_EFF_ZERO_SQ
              && crV2.LengthSquared() > SM_EFF_ZERO_SQ) { SER( smgu_LineLineClosestPoint( crP1, crV1, crP2, crV2, dT1, dT2 )); }
      else if(   crV1.LengthSquared() > SM_EFF_ZERO_SQ) { SER( smgu_LineClosestPoint( crP1, crV1, crP2, dT1 )); }
      else                                              { SER( smgu_LineClosestPoint( crP2, crV2, crP1, dT2 )); }

      if(SM_IS_ZERO(dT1) )
        {
          rpCornerCurve1 = NULL;
        }
      else
        {
          SmBSplineCurve *pTempBSPlCurve;
          SmPoint3d sEnd1 = crP1 + dT1 * crV1;
          SER( SmBSplineCurve::CreateLineSegment( crContext, 3, crP1, sEnd1, pTempBSPlCurve ));
          rpCornerCurve1 = pTempBSPlCurve;
        }

      if(SM_IS_ZERO(dT2) )
        {
          rpCornerCurve2 = NULL;
        }
      else
        {
          SmBSplineCurve *pTempBSPlCurve;
          SmPoint3d sEnd2 = crP2 + dT2 * crV2;
          SER( SmBSplineCurve::CreateLineSegment( crContext, 3, sEnd2, crP2, pTempBSPlCurve ));

          rpCornerCurve2 = pTempBSPlCurve;
        }
    }

  else if(eOffsetCorner == SM_OC_FILLET_CORNER )
    {
      SmVector3d sV1 = crP1 - crVertexPoint;
      SmVector3d sV2 = crP2 - crVertexPoint;
      SmVector3d sX, sY, sZ;
      SmVector3d sCross = sV1 * sV2;

      if(sCross.LengthSquared() < SM_EFF_ZERO )
        {
          SER( sV1.MakeUnitOrthoVectors( &crV1, sX, sY, sZ ));
        }
      else
        {
          SER( sV1.MakeUnitOrthoVectors (&sV2, sX, sY, sZ ));
        }
      double dAngle;
      SER( sZ.CCWAngleBetween( sV1, sV2, dAngle ));
      dAngle = smos_Fabs(dAngle);
      SmAxis2Placement sA2P;
      sA2P.SetCanonical( crVertexPoint, sX, sY );
      SmBSplineCurve *pTempBSPlCurve;
      SER( SmBSplineCurve::CreateCircleSegment( crContext,
                                                3,
                                                sA2P,
                                                sV1.Length(),
                                                0,
                                                dAngle*180.0/SM_PI,
                                                SM_CO_QUADRATIC,
                                                pTempBSPlCurve ));
      rpCornerCurve1 = pTempBSPlCurve;
  }

  else if(eOffsetCorner == SM_OC_LINEAR_CHAMFER )
  {
      double dT1 = 0.0;
      double dT2 = 0.0;
      if( crV1.LengthSquared() > SM_EFF_ZERO_SQ && crV2.LengthSquared() > SM_EFF_ZERO_SQ) {
          SER( smgu_LineLineClosestPoint( crP1, crV1, crP2, crV2, dT1, dT2 ));
      }
      else if( crV1.LengthSquared() > SM_EFF_ZERO_SQ) {
          SER( smgu_LineClosestPoint( crP1, crV1, crP2, dT1 ));
      }
      else {
          SER( smgu_LineClosestPoint( crP2, crV2, crP1, dT2 ));
      }

      SmPoint3d xPnt;

      if(SM_IS_ZERO(dT1) ) {
          rpCornerCurve1 = NULL;
      }
      else {
          SmBSplineCurve *pTempBSPlCurve;
          SmPoint3d sEnd1 = crP1 + dT1 * crV1;
          SER( SmBSplineCurve::CreateLineSegment( crContext, 3, crP1, sEnd1, pTempBSPlCurve ));
          rpCornerCurve1 = pTempBSPlCurve;

          xPnt = sEnd1;
      }

      if(SM_IS_ZERO(dT2) ) {
          rpCornerCurve2 = NULL;
      }
      else {
          SmBSplineCurve *pTempBSPlCurve;
          SmPoint3d sEnd2 = crP2 + dT2 * crV2;
          SER( SmBSplineCurve::CreateLineSegment( crContext, 3, crP2, sEnd2, pTempBSPlCurve ));

          rpCornerCurve2 = pTempBSPlCurve;

          xPnt = sEnd2;
      }

      if( rpCornerCurve1 == NULL || rpCornerCurve2 == NULL )
          return SM_SUCCESS;

      SmVector3d sVertexPtToXPt = xPnt - crVertexPoint;
      sVertexPtToXPt.Unitize();

      SmTArray<SmCurve*> sOffsets;
      sOffsets.Add( rpCornerCurve1 );
      sOffsets.Add( rpCornerCurve2 );

      SmVector3d sPlaneNormal(0,0,1);
      SmCurve::ComputeCrvsNormal( sOffsets, sPlaneNormal );

      SmVector3d sChamferVector = sVertexPtToXPt * sPlaneNormal;
      SmPoint3d sChamferPt = crVertexPoint + dOffsetDistance * sVertexPtToXPt;

      SmBSplineCurve *pTempBSPlCurve;
      SmPoint3d sChamferStartPt = sChamferPt + dOffsetDistance * sChamferVector;
      SmPoint3d sChamferEndPt = sChamferPt - dOffsetDistance * sChamferVector;
      SER( SmBSplineCurve::CreateLineSegment( crContext, 3, sChamferStartPt, sChamferEndPt, pTempBSPlCurve ));

      rpCornerCurve3 = pTempBSPlCurve;

      SmExtent1d sRange1 = rpCornerCurve1->GetNaturalInterval();
      SmExtent1d sRange2 = rpCornerCurve2->GetNaturalInterval();
      SmExtent1d sRange3 = rpCornerCurve3->GetNaturalInterval();

      SmSolutionArray sSolutions1;
      rpCornerCurve1->GlobalCurveIntersect( sRange1, *rpCornerCurve3,  sRange3, dXSectTol3d, sSolutions1 );

      // (shim guard: upstream reads sSolutions1[0] of an empty array)
      if (sSolutions1.GetSize() == 0) return SM_ERR_INVALID_INPUT;

      SmExtent1d sTrimIvl( sRange1.GetMin(), sSolutions1[0].m_vStart[0] );
      SER( rpCornerCurve1->Trim( sTrimIvl )) ;

      SmSolutionArray sSolutions2;
      rpCornerCurve2->GlobalCurveIntersect( sRange2, *rpCornerCurve3,  sRange3, dXSectTol3d, sSolutions2 );

      // (shim guard: upstream reads sSolutions2[0] of an empty array)
      if (sSolutions2.GetSize() == 0) return SM_ERR_INVALID_INPUT;

      SmExtent1d sTrimIv2( sRange2.GetMin(), sSolutions2[0].m_vStart[0] );
      SER( rpCornerCurve2->Trim( sTrimIv2 )) ;

      SmExtent1d sTrimIv3;
      double dP1 = sSolutions1[0].m_vStart[1], dP2 = sSolutions2[0].m_vStart[1];
      if ( dP1 < dP2 )
          sTrimIv3.SetMinMax( dP1, dP2 );
      else
          sTrimIv3.SetMinMax( dP2, dP1 );
      SER( rpCornerCurve3->Trim( sTrimIv3 )) ;
    }
  else
    {
      SER( SM_ERR );
    }

  return SM_SUCCESS;
}

static SmStatus sm_CurveClassify
( const SmCurve                  & crTrimmedOffset,
  double                           dOffsetDistance,
  double                           dXSectTol3d,
  const SmTArray<const SmCurve*> & crOrigCurves,
  SmBoolean                      & rbKeepCurve )
{
    rbKeepCurve = FALSE;

    SmExtent1d sIvl = crTrimmedOffset.GetNaturalInterval();
    SmAttribute *pAttr = crTrimmedOffset.FindAttribute( SM_AI_OFFSETMAP );
    SmOffsetMapAttribute *pMapAttr = SM_CAST_PTR( SmOffsetMapAttribute, pAttr );
    const SmCurve        * pOriginator = ( pMapAttr != NULL ) ? pMapAttr->GetOrigCurve() : NULL;

    if ( pOriginator != NULL )
    {
        double dMidParam = sIvl.Evaluate( 0.5 );
        SmVector3d sPVThis[2], sPVOrig[2];
        crTrimmedOffset.Evaluate( dMidParam, 1, FALSE, sPVThis );
        pOriginator->Evaluate( dMidParam, 1, FALSE, sPVOrig );
        if ( sPVThis[1].Dot( sPVOrig[1] ) < 0.0 )
        {
            rbKeepCurve = FALSE;
            return SM_SUCCESS;
        }
    }

    double dCutoffDist = dOffsetDistance * ( 1.0 - SM_EFF_ZERO );

    double dMinDist = dOffsetDistance + 2.0 * dXSectTol3d;
    SmSolution aData[10];
    SmSolutionArray sSolutions( 10, aData );
    rbKeepCurve = TRUE;

    SmTArray< SmPoint3d > aSamplePts;
    SmPoint3d sPnt;
    SER( crTrimmedOffset.EvaluatePoint( sIvl.Evaluate( 0.5 ), sPnt ) );
    aSamplePts.Add( sPnt );

    ULONG i, j;
    for ( i = 0; i < crOrigCurves.GetSize(); i++ )
    {
        const SmCurve *pOrig = crOrigCurves[i];

        for(j = 0; j < aSamplePts.GetSize(); j++ )
        {
            sPnt = aSamplePts[j];
            SER( pOrig->GlobalPointSolve
                   (pOrig->GetNaturalInterval(),
                    SM_SO_MINIMIZE,
                    sPnt,
                    dXSectTol3d,
                    &dMinDist,
                    NULL,
                    SM_SR_SINGLE,
                    sSolutions)) ;

            if(sSolutions.GetSize() > 0 )
            {
                double dThisDist = sSolutions[0].m_vStart.m_dSolutionValue;
                if(dThisDist < dCutoffDist )
                {
                    rbKeepCurve = FALSE;
                    return SM_SUCCESS;
                }
            }
        }
    }

    return SM_SUCCESS;
}

static SmStatus sm_TrimFirstBreak
 (SmTArray<double> & rDoubleArray,
  double             dNewDouble)
{
  ULONG ii ;

  for(ii=0; ii<rDoubleArray.GetSize(); ii++ )
    {
      if(SM_ARE_SAME( dNewDouble, rDoubleArray[ii] ) )
        {
          rDoubleArray.SetAt(ii, dNewDouble) ;
          if(ii > 0) { rDoubleArray.RemoveAt(0, ii) ; }
          return SM_SUCCESS;
        }

      if(dNewDouble < rDoubleArray[ii] )
        {
          rDoubleArray.InsertAt( ii, dNewDouble, 1 );
          if(ii > 0) { rDoubleArray.RemoveAt(0, ii) ; }
          return SM_SUCCESS;
        }
    }

  rDoubleArray.ReSet() ;
  rDoubleArray.Add( dNewDouble );

  return SM_SUCCESS;
}

static SmStatus sm_TrimLastBreak
 (SmTArray<double> & rDoubleArray,
  double             dNewDouble)
{
  ULONG ii ;

  for(ii=0; ii<rDoubleArray.GetSize(); ii++ )
    {
      if(SM_ARE_SAME( dNewDouble, rDoubleArray[ii] ) )
        {
          rDoubleArray.SetAt(ii, dNewDouble) ;
          if(ii + 1 < rDoubleArray.GetSize()) { rDoubleArray.RemoveAt(ii+1, rDoubleArray.GetSize() - ii - 1) ; }
          return SM_SUCCESS;
        }

      if(dNewDouble < rDoubleArray[ii] )
        {
          rDoubleArray.InsertAt( ii, dNewDouble, 1 );
          if(ii + 1 < rDoubleArray.GetSize()) { rDoubleArray.RemoveAt(ii+1, rDoubleArray.GetSize() - ii - 1) ; }
          return SM_SUCCESS;
        }
    }

  rDoubleArray.Add( dNewDouble );

  return SM_SUCCESS;
}

static SmStatus sm_InsertSortedBreaks
 (SmTArray<double> & rDoubleArray,
  double             dNewDouble)
{
  ULONG ii ;

  for(ii=0; ii<rDoubleArray.GetSize(); ii++ )
    {
      if(SM_ARE_SAME( dNewDouble, rDoubleArray[ii] ) )
        { return SM_SUCCESS; }

      if(dNewDouble < rDoubleArray[ii] )
        {
          rDoubleArray.InsertAt( ii, dNewDouble, 1 );
          return SM_SUCCESS;
        }
    }

  rDoubleArray.Add( dNewDouble );

  return SM_SUCCESS;
}

// ---- shim plumbing -----------------------------------------------------------------

// Ops (keep in sync with crates/solidcore-ffi/src/units/curve_composite_curve_p1.rs).
enum : int32_t
{
    OP_INFO = 0,
    OP_KNOTS = 1,
    OP_BBOX = 2,
    OP_EQ = 3,
    OP_COPY = 4,
    OP_TRANSFORM = 5,
    OP_APPROX = 6,
    OP_ORIENT = 7,
    OP_BUILD = 8,
    OP_CORNER = 9,
    OP_CLASSIFY = 10,
    OP_BREAKS = 11,
    OP_TYPES = 12,
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

// A curve block (see the file comment).
SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    ULONG dim = (ULONG)rd.idx();
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

// A composite block `dim n curve*n closed has_senses sense*n has_gaps gap*n`.
SmCompositeCurve* read_composite(Rd& rd, const SmContext& ctx)
{
    ULONG dim = (ULONG)rd.idx();
    int64_t n = rd.idx();
    SmTArray<SmCurve*> curves;
    for (int64_t i = 0; i < n; i++)
        curves.Add(read_curve(rd, ctx));
    bool closed = rd.flag();
    bool has_senses = rd.flag();
    SmTArray<SmBoolean> senses;
    if (has_senses)
        for (int64_t i = 0; i < n; i++)
            senses.Add(rd.flag() ? TRUE : FALSE);
    bool has_gaps = rd.flag();
    SmTArray<double> gaps;
    if (has_gaps)
        for (int64_t i = 0; i < n; i++)
            gaps.Add(rd.num());
    return new (ctx) SmCompositeCurve(dim, curves, closed ? TRUE : FALSE,
                                      has_senses ? &senses : NULL, has_gaps ? &gaps : NULL);
}

void put_pseudo(std::vector<double>& o, const SmPseudoBox& b)
{
    for (ULONG i = 0; i < 3; i++)
        put_vec(o, b.GetBasis(i));
    for (ULONG i = 0; i < 3; i++)
    {
        o.push_back(b.GetInterval(i).GetMin());
        o.push_back(b.GetInterval(i).GetMax());
    }
}

void put_polar(std::vector<double>& o, const SmPolarBox& b)
{
    o.push_back((double)b.GetPolarBoxState());
    for (ULONG i = 0; i < 3; i++)
        put_vec(o, *b.GetBasis(i));
    SmExtent2d d = b.GetDomain();
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
}

// The state of a curve: `kind a b P[3]*7 [n+1 Pw[4] m+1 U]` (kind 0 = B-spline, with its
// NURB; 2 = line; 1 = other).
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    double kind = c->GetType() == SmBSplineCurve_TYPE ? 0.0 : c->GetType() == SmLine_TYPE ? 2.0 : 1.0;
    o.push_back(kind);
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    for (int i = 0; i <= 6; i++)
    {
        SmPoint3d p(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        c->EvaluatePoint(ivl.Evaluate(i / 6.0), p);
        put_vec(o, p);
    }
    if (kind == 0.0)
    {
        gw_CURVE* nurb = ((const SmBSplineCurve*)c)->GetGwNurbPointer();
        o.push_back((double)(nurb->pol->n + 1));
        for (gw_INDEX k = 0; k <= nurb->pol->n; k++)
        {
            o.push_back(nurb->pol->Pw[k].x);
            o.push_back(nurb->pol->Pw[k].y);
            o.push_back(nurb->pol->Pw[k].z);
            o.push_back(nurb->pol->Pw[k].w);
        }
        o.push_back((double)(nurb->knt->m + 1));
        for (gw_INDEX k = 0; k <= nurb->knt->m; k++)
            o.push_back(nurb->knt->U[k]);
    }
}

// The segment records of a composite: `n closed (transition sense gap0 gap1 owns)*n`.
void put_segments(std::vector<double>& o, const SmCompositeCurve* c)
{
    ULONG n = c->GetNumSegments();
    o.push_back((double)n);
    o.push_back(flg(c->IsClosed(c->GetNaturalInterval())));
    for (ULONG i = 0; i < n; i++)
    {
        const SmCompositeCurveSegment* s = c->GetCurveSegment(i);
        o.push_back((double)s->m_eTransition);
        o.push_back(flg(s->m_bSameSense));
        o.push_back(s->m_dGapStart);
        o.push_back(s->m_dGapEnd);
        o.push_back(flg(s->m_bOwnsCurve));
    }
}

// The type queries asked for: `nt t*nt`, answered by `kindof(t)*nt`.
template <typename F> void put_kinds(std::vector<double>& o, Rd& rd, F kindof)
{
    int64_t nt = rd.idx();
    for (int64_t i = 0; i < nt; i++)
        o.push_back(flg(kindof((SM_TYPE)rd.idx())));
}

void run(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_INFO:
    {
        SmCompositeCurve* c = read_composite(rd, ctx);
        const SmCurve* cc = c;
        o.push_back(0.0);
        put_segments(o, c);
        SmExtent1d ivl = cc->GetNaturalInterval();
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
        o.push_back((double)cc->GetDim());
        o.push_back((double)cc->GetDegree());
        o.push_back((double)cc->GetNumberNaturalKnots());
        o.push_back((double)cc->GetNumberControlPoints());
        o.push_back(flg(cc->IsAnalytic()));
        o.push_back(flg(cc->IsBounded()));
        o.push_back(flg(cc->HasRepeatedEndControlPoints()));
        o.push_back((double)cc->GetType());
        SmPoint3d p(0, 0, 0);
        o.push_back(st(cc->EvaluatePoint(ivl.GetMin(), p)));
        o.push_back(flg(c->GetCurveSegment(c->GetNumSegments()) == NULL));
        ULONG dim = 0;
        SmTArray<SmCurve*> curves;
        SmBoolean closed = FALSE;
        SmTArray<SmBoolean> senses;
        o.push_back(st(c->GetCanonical(dim, curves, closed, &senses)));
        o.push_back((double)dim);
        o.push_back(flg(closed));
        o.push_back((double)curves.GetSize());
        for (ULONG i = 0; i < senses.GetSize(); i++)
            o.push_back(flg(senses[i]));
        put_kinds(o, rd, [&](SM_TYPE t) { return cc->IsKindOf(t); });
        delete c;
        break;
    }
    case OP_KNOTS:
    {
        SmCompositeCurve* c = read_composite(rd, ctx);
        SmTArray<double> knots;
        SmTArray<ULONG> mults;
        SmStatus s = c->GetKnots(knots, &mults, NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)knots.GetSize());
            for (ULONG i = 0; i < knots.GetSize(); i++)
                o.push_back(knots[i]);
            o.push_back((double)mults.GetSize());
            for (ULONG i = 0; i < mults.GetSize(); i++)
                o.push_back((double)mults[i]);
        }
        delete c;
        break;
    }
    case OP_BBOX:
    {
        SmCompositeCurve* c = read_composite(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        bool want_p = rd.flag();
        bool want_l = rd.flag();
        bool expand = rd.flag();
        SmExtent3d nb;
        SmPseudoBox pb;
        SmPolarBox pl;
        SmStatus s = c->CalculateBoundingBox(SmExtent1d(a, b), &nb, want_p ? &pb : NULL,
                                             want_l ? &pl : NULL, expand ? TRUE : FALSE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            put_vec(o, nb.GetMin());
            put_vec(o, nb.GetMax());
            if (want_p)
                put_pseudo(o, pb);
            if (want_l)
                put_polar(o, pl);
        }
        delete c;
        break;
    }
    case OP_EQ:
    {
        SmCompositeCurve* a = read_composite(rd, ctx);
        SmCompositeCurve* b = read_composite(rd, ctx);
        o.push_back(0.0);
        o.push_back(flg(*a == *b));
        o.push_back(flg(*b == *a));
        o.push_back(flg(*a == *a));
        bool seg = a->GetNumSegments() > 0 && b->GetNumSegments() > 0;
        o.push_back(seg ? flg(*a->GetCurveSegment(0) == *b->GetCurveSegment(0)) : -1.0);
        delete a;
        delete b;
        break;
    }
    case OP_COPY:
    {
        SmCompositeCurve* c = read_composite(rd, ctx);
        SmCurve* cp = NULL;
        SmStatus s = c->Copy(ctx, cp);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(*cp == *c));
            o.push_back((double)cp->GetType());
            SmCompositeCurve* cc = (SmCompositeCurve*)cp;
            put_segments(o, cc);
            for (ULONG i = 0; i < cc->GetNumSegments(); i++)
                put_curve(o, cc->GetCurveSegment(i)->m_pParentCurve);
            delete cp;
        }
        delete c;
        break;
    }
    case OP_TRANSFORM:
    {
        SmCompositeCurve* c = read_composite(rd, ctx);
        SmPoint3d org = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        bool has_scale = rd.flag();
        SmVector3d scale = rd.vec();
        SmAxis2Placement a2p(org, x, y);
        SmStatus s = c->Transform(a2p, has_scale ? &scale : NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            for (ULONG i = 0; i < c->GetNumSegments(); i++)
                put_curve(o, c->GetCurveSegment(i)->m_pParentCurve);
        delete c;
        break;
    }
    case OP_APPROX:
    {
        SmCompositeCurve* c = read_composite(rd, ctx);
        double tol = rd.num();
        SmTArray<double> breaks;
        double gap = -1.0;
        SmBSplineCurve* r = NULL;
        SmStatus s = c->ApproximateCurve(ctx, breaks, tol, gap, r);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(gap);
            o.push_back(r != NULL ? 1.0 : 0.0);
            if (r != NULL)
                put_curve(o, r);
        }
        delete r;
        delete c;
        break;
    }
    case OP_ORIENT:
    {
        int64_t n = rd.idx();
        SmTArray<SmCurve*> curves;
        for (int64_t i = 0; i < n; i++)
            curves.Add(read_curve(rd, ctx));
        SmTArray<SmBoolean> senses;
        for (int64_t i = 0; i < n; i++)
            senses.Add(rd.flag() ? TRUE : FALSE);
        SmVector3d normal = rd.vec();
        bool want_bounds = rd.flag();
        SmExtent2d bounds;
        SmOrientType orient = SM_OT_OPPOSITE;
        SmStatus s = SmCompositeCurve::ComputeProjectedLoopOrientation(curves, senses, normal, orient,
                                                                       want_bounds ? &bounds : NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)orient);
            if (want_bounds)
            {
                o.push_back(bounds.GetMin().x);
                o.push_back(bounds.GetMin().y);
                o.push_back(bounds.GetMax().x);
                o.push_back(bounds.GetMax().y);
            }
        }
        for (ULONG i = 0; i < curves.GetSize(); i++)
            delete curves[i];
        break;
    }
    case OP_BUILD:
    {
        int64_t n = rd.idx();
        SmTArray<SmBSplineCurve*> curves;
        for (int64_t i = 0; i < n; i++)
            curves.Add((SmBSplineCurve*)read_curve(rd, ctx));
        double same = rd.num();
        double line = rd.num();
        SmTArray<SmCompositeCurve*> comps;
        SmStatus s = SmCompositeCurve::BuildCompositesFromCurves(ctx, curves, FALSE, 0.0, same, 0.0, 0.0,
                                                                 line, 0.0, comps);
        o.push_back(st(s));
        std::set<const SmCurve*> used;
        if (s == SM_SUCCESS)
        {
            o.push_back((double)comps.GetSize());
            for (ULONG c = 0; c < comps.GetSize(); c++)
            {
                SmCompositeCurve* cc = comps[c];
                o.push_back((double)cc->GetDim());
                put_segments(o, cc);
                for (ULONG i = 0; i < cc->GetNumSegments(); i++)
                {
                    const SmCurve* sc = cc->GetCurveSegment(i)->m_pParentCurve;
                    used.insert(sc);
                    put_curve(o, sc);
                }
            }
            double unused = 0.0;
            for (ULONG i = 0; i < curves.GetSize(); i++)
                if (used.count(curves[i]) == 0)
                    unused += 1.0;
            o.push_back(unused);
        }
        for (ULONG c = 0; c < comps.GetSize(); c++)
            delete comps[c];
        for (ULONG i = 0; i < curves.GetSize(); i++)
            if (used.count(curves[i]) == 0)
                delete curves[i];
        break;
    }
    case OP_CORNER:
    {
        SmPoint3d vp = rd.vec();
        SmPoint3d p1 = rd.vec();
        SmVector3d v1 = rd.vec();
        SmPoint3d p2 = rd.vec();
        SmVector3d v2 = rd.vec();
        int64_t type = rd.idx();
        double dist = rd.num();
        double tol = rd.num();
        SmCurve* c1 = NULL;
        SmCurve* c2 = NULL;
        SmCurve* c3 = NULL;
        SmStatus s = sm_CreateCorner(ctx, vp, p1, v1, p2, v2, (SmOffsetCornerType)type, dist, tol,
                                     c1, c2, c3);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            for (SmCurve* c : {c1, c2, c3})
            {
                o.push_back(c != NULL ? 1.0 : 0.0);
                if (c != NULL)
                    put_curve(o, c);
            }
        }
        delete c1;
        delete c2;
        delete c3;
        break;
    }
    case OP_CLASSIFY:
    {
        SmCurve* t = read_curve(rd, ctx);
        double dist = rd.num();
        double tol = rd.num();
        int64_t n = rd.idx();
        SmTArray<const SmCurve*> origs;
        std::vector<SmCurve*> owned;
        for (int64_t i = 0; i < n; i++)
        {
            SmCurve* c = read_curve(rd, ctx);
            owned.push_back(c);
            origs.Add(c);
        }
        bool has_orig = rd.flag();
        SmCurve* orig = NULL;
        if (has_orig)
        {
            orig = read_curve(rd, ctx);
            t->AddAttribute(new (ctx) SmOffsetMapAttribute(SM_AI_OFFSETMAP, FALSE, FALSE, 0, 0, 0, orig, 0.0));
        }
        SmBoolean keep = FALSE;
        SmStatus s = sm_CurveClassify(*t, dist, tol, origs, keep);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back(flg(keep));
        delete t;
        delete orig;
        for (SmCurve* c : owned)
            delete c;
        break;
    }
    case OP_BREAKS:
    {
        int64_t which = rd.idx();
        int64_t n = rd.idx();
        SmTArray<double> v;
        for (int64_t i = 0; i < n; i++)
            v.Add(rd.num());
        double x = rd.num();
        SmStatus s = which == 0 ? sm_TrimFirstBreak(v, x) : which == 1 ? sm_TrimLastBreak(v, x)
                                                                       : sm_InsertSortedBreaks(v, x);
        o.push_back(st(s));
        o.push_back((double)v.GetSize());
        for (ULONG i = 0; i < v.GetSize(); i++)
            o.push_back(v[i]);
        break;
    }
    case OP_TYPES:
    {
        o.push_back(0.0);
        ULONG id = (ULONG)rd.idx();
        bool corner = rd.flag();
        ULONG comp = (ULONG)rd.idx();
        ULONG crv = (ULONG)rd.idx();
        ULONG other = (ULONG)rd.idx();
        double err = rd.num();
        SmOffsetMapAttribute* attr =
            new (ctx) SmOffsetMapAttribute(id, corner ? TRUE : FALSE, FALSE, comp, crv, other, NULL, err);
        o.push_back((double)attr->GetType());
        o.push_back(flg(attr->GetCornerOffset()));
        o.push_back((double)attr->GetCurve());
        o.push_back((double)attr->GetOtherCurve());
        o.push_back(attr->GetOrigCurve() == NULL ? 1.0 : 0.0);
        o.push_back((double)attr->GetAttributeID());
        SmCompositeCurveRegion* region = new (ctx) SmCompositeCurveRegion();
        o.push_back((double)region->GetType());
        SmCompositeCurveSegment seg;
        o.push_back((double)seg.GetType());
        int64_t nt = rd.idx();
        for (int64_t i = 0; i < nt; i++)
        {
            SM_TYPE t = (SM_TYPE)rd.idx();
            o.push_back(flg(attr->IsKindOf(t)));
            o.push_back(flg(region->IsKindOf(t)));
            o.push_back(flg(seg.IsKindOf(t)));
        }
        delete attr;
        delete region;
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_curve_composite_curve_p1_eval(int32_t op, const double* in, int64_t n_in,
                                                   double* out, int64_t cap, int64_t* out_len)
{
    if (in == nullptr || n_in < 0 || out_len == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    Rd rd{in, 0};
    std::vector<double> o;
    run(op, rd, ctx, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
