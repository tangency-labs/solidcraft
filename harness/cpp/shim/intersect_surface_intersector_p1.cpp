// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `intersect.surface_intersector_p1`: SmTsectPnt and the
// SmSurfaceIntersector members owned by the unit (source/SMLib/src/SmSurfaceIntersector.cpp,
// source/SMLib/inc/SmSurfaceIntersector.h), run on a real SmSurfaceIntersector (a derived class
// only exposes the protected state), plus the file-static helpers sm_ComputeCurveType,
// sm_IsPointOnCurve and sm_FlushCrv.
//
// Static functions of SmSurfaceIntersector.cpp cannot be linked against, so they are VERBATIM
// copies of upstream SmSurfaceIntersector.cpp (commit c9c5979) inside the namespace sc_ssi_p1
// (line ranges noted at each block; generated, do not edit by hand).
//
// One entry point, sc_intersect_surface_intersector_p1_run(in, n_in, out, cap, out_len). `in` is
// a program (layout in crates/solidcore-ffi/src/units/intersect_surface_intersector_p1.rs);
// every read is bounds checked here (a bad program is SC_ERR_ARG), and linking a point that is
// already on a list (undefined upstream) is rejected the same way.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmSurfaceIntersector.h>
#include <SmTArray.h>
#include <SmTList.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <stdexcept>
#include <vector>

#include "sc_sm.h"

using sc::put_uv;
using sc::put_vec;
using sc::st;

namespace sc_ssi_p1
{
// clang-format off
// ---- verbatim: SmSurfaceIntersector.cpp lines 673-747: sm_ComputeCurveType
static SmTsectCurveType sm_ComputeCurveType
  (const SmSurface * cpSurface1,
   const SmSurface * cpSurface2,
   const SmCurve * cpUVCurve1,
   const SmCurve * cpUVCurve2)
{
    SmTsectCurveType eRet = SM_TC_CROSSING;
    SmPoint3d sP1, sP2;
    SmExtent1d sIvl1 = cpUVCurve1->GetNaturalInterval();
    SmExtent1d sIvl2 = cpUVCurve2->GetNaturalInterval();
    cpUVCurve1->EvaluatePoint(sIvl1.Evaluate(0.5),sP1);
    cpUVCurve2->EvaluatePoint(sIvl2.Evaluate(0.5),sP2);
    SmVector3d sNorm1, sNorm2;
    SmVector2d sUV1(sP1.x,sP1.y), sUV2(sP2.x,sP2.y);
    cpSurface1->EvaluateNormal(sUV1,TRUE,TRUE,sNorm1);
    cpSurface2->EvaluateNormal(sUV2,TRUE,TRUE,sNorm2);
    double dSign = 1.0;
    double dDot = sNorm1.Dot(sNorm2);
    if (dDot < 0.0) { dSign = -1.0; }

    double dLimit = 0.0001; // was SM_EFF_ZERO_SQRT [B162]

    // Test to see if normals coincide
    if (smos_Fabs(dDot) > 1.0 - dLimit)
    {
        // We're at least TAN.
        eRet = SM_TC_TANGENT;
        double dGaussCur1 = 0.0, dNormalCur1 = 0.0, dPrinCur11 = 0.0, dPrinCur12 = 0.0;
        SmVector3d sEFG1, sLMN1, sPrinKV11, sPrinKV12;
        SE(cpSurface1->EvaluateGeometric(sUV1,TRUE,TRUE,
            dGaussCur1, dNormalCur1, dPrinCur11, dPrinCur12,
            sEFG1,sLMN1,sPrinKV11,sPrinKV12));
        double dGaussCur2 = 0.0, dNormalCur2 = 0.0, dPrinCur21 = 0.0, dPrinCur22 = 0.0;
        SmVector3d sEFG2, sLMN2, sPrinKV21, sPrinKV22;
        SE(cpSurface2->EvaluateGeometric(sUV2,TRUE,TRUE,
            dGaussCur2, dNormalCur2, dPrinCur21, dPrinCur22,
            sEFG2,sLMN2,sPrinKV21,sPrinKV22));
        // Now check to see if we have some coincidence occuring

        // First check to see if we have coincident umbilical points
        double dScale = 1.0 + smos_Fabs(dPrinCur11);
        if (smos_Fabs(dPrinCur11-dPrinCur12) < dLimit * dScale) {
            // Surf1 is umbilical.
            // If Surf2 is not umbilical, return TAN.
            if (smos_Fabs(dPrinCur21-dPrinCur22) > dLimit * dScale) { return eRet; }
            // Surf2 is also umbilical.  If radii of curvatures are equal, then COINC.
            if (smos_Fabs(dPrinCur11-dSign*dPrinCur21) > dLimit * dScale) { return eRet; }
            // both umbilical, with the same curvature radii: COINCIDENT.
            eRet = SM_TC_COINCIDENT;
            return eRet;
        }
        // Here, Srf1 is not umbilical. If Srf2 is umbilical, just quit now.
        if (smos_Fabs(dPrinCur21-dPrinCur22) < dLimit * dScale) { return eRet; }

        // Now check to see if we have curvature vectors and magnitudes which are 
        // equivalent.
        if (smos_Fabs(sPrinKV11.Dot(sPrinKV21)) > 1.0 - dLimit) {
            // First prin crv vectors line up.  Compare the appropriate curvatures.
            if (smos_Fabs(dPrinCur11-dSign*dPrinCur21) > dLimit * dScale) { return eRet; }
            double dScale2 = 1.0 + smos_Fabs(dPrinCur12);
            if (smos_Fabs(dPrinCur12-dSign*dPrinCur22) > dLimit * dScale2) { return eRet; }
            eRet = SM_TC_COINCIDENT;
        }
        if (smos_Fabs(sPrinKV11.Dot(sPrinKV22)) > 1.0 - dLimit) {
            // First prin crv vector of Surf1 lines up with 2nd prin crv vector
            // of Surf2.  Compare the appropriate curvatures.
            if (smos_Fabs(dPrinCur11-dSign*dPrinCur22) > dLimit * dScale) { return eRet; }
            double dScale2 = 1.0 + smos_Fabs(dPrinCur12);
            if (smos_Fabs(dPrinCur12-dSign*dPrinCur21) > dLimit * dScale2) { return eRet; }
            eRet = SM_TC_COINCIDENT;
        }
    }
    return eRet;

} // end sm_ComputeCurveType

// ---- verbatim: SmSurfaceIntersector.cpp lines 4016-4254: sm_IsPointOnCurve
static SmBoolean sm_IsPointOnCurve     // rtn: TRUE=PointToTest is on curve between Prev and Next points
  (SmTsectPnt & crPointToTest,         // in : TargetPoint
   SmTsectPnt & crPrevTSP,             // in : LastPoint known to be on XSectCurve
   SmTsectPnt & crNextTSP,             // in : NextPoint known to be on XSectCurve
   double       d3DApproxTol)          // in : 
{
  // First adjust the tolerance for the angle between the two surfaces.
  // If they're close to tangent, then the region where points could
  // be considered within tolerance 'spreads out', specifically, by
  // the tangent of the half-angle between them.
  // Note, sometimes some of these values are zero, which
  // presumably means unset.
  // [regression tests: my_shell_demo iters 0,3,6,7; fillet test 2:214]

  // GWC_NEEDS_WORK extend_algorithm_to_check_for_direction_for_Singularity_points GWC_LINE ;

  // get max surfNorm/surfNorm angle at XSectCurve Segment endPoints
  double dAng = smos_Max( crPrevTSP.m_dTangentPlaneAngleRad,
                          crNextTSP.m_dTangentPlaneAngleRad );

  // when the angle is not exactly zero (tangent xSectCurve Segment)
  if ( dAng > 0.0 )
    {
      // increase the tolerance size
      double dTan = smos_Tangent( dAng / 2 );
      if ( dTan < 0.001 ) { dTan = 0.001; }
      d3DApproxTol /= dTan;
    }

  // Get xSect Segment domain interval.
  double dDeltaT = crNextTSP.m_dCurveParameter - crPrevTSP.m_dCurveParameter;

  // Build 3d hermite curve from PrevTSP to NextTSP.
  const SmPoint3d & rP1 = crPrevTSP.CrvPos();
  const SmPoint3d & rP2 = crNextTSP.CrvPos();
  SmHermiteCurve sHerm3d( rP1, crPrevTSP.CrvDeriv()*dDeltaT,
                        rP2, crNextTSP.CrvDeriv()*dDeltaT, 3 );
  sHerm3d.SetContext(NULL);

  // Get rough distance between PrevTSP and NextTSP.
  double dLength = (rP2-rP1).GetMaxDimension();

  // let s3DBox = bounding box for hermite curve
  SmExtent3d s3DBox;
  SmExtent1d sHermiteDomain = sHerm3d.GetNaturalInterval();  // (We know it's [0,1]...)
  sHerm3d.CalculateBoundingBox( sHermiteDomain, &s3DBox );

  // let sPntBox = HalfSize+Tol Sized box centered on the crPointToTest
  // (Note: that can be a pretty big box.)
  SmExtent3d sPntBox( crPointToTest.CrvPos() );
  sPntBox.ExpandAbsolute( d3DApproxTol*2.0 + dLength / 4.0 );

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe2 = FALSE;
  if (bDebugMe2)
    {
      sm_GraphicsLoop();
      smgfx_SetLook(4,6, 1,0,0) ; crPointToTest.CrvPos().Draw(); sm_GraphicsLoop();
      smgfx_SetLook(3,4, 1,0,1) ; sHerm3d.Draw(&sHermiteDomain); sm_GraphicsLoop();
      smgfx_SetLook(1,2, 0,1,0) ; s3DBox.Draw(NULL) ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, .5,.5,.5) ; sPntBox.Draw(NULL) ; sm_GraphicsLoop() ;
      sm_GraphicsLoop();
    }
#endif

  // Not on curve if the bounding box tests fails.
  if ( s3DBox.AreDisjoint( sPntBox ) )
    { return FALSE; }

  // Do bounding-box checks in the uv-space of both surfaces:
  // check the uv points against the bounding boxes of Hermite curves.
  ULONG lSurf;
  for ( lSurf=0; lSurf<2; lSurf++ )
    {
      // Make a hermite UVTrimCurve.
      SmHermiteCurve sHermUV( crPrevTSP.UVPos(lSurf),
                              crPrevTSP.UVDeriv(lSurf)*dDeltaT,
                              crNextTSP.UVPos(lSurf),
                              crNextTSP.UVDeriv(lSurf)*dDeltaT,
                              2);
      sHermUV.SetContext( NULL );
      SmExtent1d sUVDomain = sHermUV.GetNaturalInterval();

      // Let sUVBox = UVTrimCurve bounding box expanded by its diagonal length.
      // But: Have to expand u and v separately.  Example:  [B401]
      // u is 0.0 - 0.00402  on domain 0-1
      // v is 19.4 - 21.0 on domain 0-360
      // So u gets expanded by 1.6, which would include a point across the seam.

      SmExtent3d sUVBox;
      sHermUV.CalculateBoundingBox( sUVDomain, &sUVBox );

      // sUVBox.ExpandAbsolute( sUVBox.GetSize().Length() );

      // Have to check for zero-length domains:
      // expanding by their length wouldn't do anything.
      double dDiag = sUVBox.GetSize().Length();  // for checking zero-length domains.

      SmExtent1d sTemp = sUVBox.GetUInterval();
      double dLen = sTemp.GetLength();
      if ( dLen < dDiag / 100 )
        { dLen = dDiag / 100; }
      sTemp.ExpandAbsolute( dLen );
      sUVBox.SetUInterval( sTemp );

      sTemp = sUVBox.GetVInterval();
      dLen = sTemp.GetLength();
      if ( dLen < dDiag / 100 )
        { dLen = dDiag / 100; }
      sTemp.ExpandAbsolute( dLen );
      sUVBox.SetVInterval( sTemp );

      // Let sUV = UVPoint to test.
      SmPoint3d sUV( crPointToTest.UVPos( lSurf ));

      // If point is not contained by the expanded UVBounding Box - return FALSE.
      if ( ! sUVBox.ContainsPoint3d( sUV ))
        { return FALSE; }

    } // end iter both surfaces

  // Leave this next test out.  Its premise is dodgy, and it could never
  // happen anyway: dDistToEnd >= dDist, always.  (dDistToEnd is the
  // hypotenuse of a right triangle, and dDist is a leg.)
//  // Test to see if point is just off the end of the curve.
//  // This line starts at the curve end (next pt) and leaves the curve,
//  // so if the drop param is negative, it's before the end of the curve.
//  double dParam;
//  smgu_LineClosestPoint( rP2,  // current curve end point
//                         crNextTSP.CrvDeriv(),
//                         crPointToTest.CrvPos(),
//                         dParam );
//
//  // When point is beyond endPoint
//  if ( dParam > 0.0 )
//    {
//      // When point is close to the xSectCurve segment chord
//      double dDist;
//      smgu_LinePointDistance( rP2,
//                              crNextTSP.CrvDeriv(),
//                              crPointToTest.CrvPos(),
//                              dDist);
//      if (   dDist < dLength / 10.0 
//          && dDist < d3DApproxTol*20.0 )
//        {
//           // When point is close to xSectCurve segment endPoint
//           double dDistToEnd = crPointToTest.CrvPos().DistanceBetween( rP2 );
//           if ( dDistToEnd < dDist/4.0 )
//             {
//               // found an end point
//               return TRUE;
//             }
//        }
//    } // End point-is-beyond-endPoint check
  
  // Arrive here when Point projects to xSectCurve Segment
  // Find closest point on the Hermite Curve to the Point.
  SmSolution sSol;
  SmBoolean bSuccess;

  // Use midpoint as guess: should be fine for this simple curve.
  double dGuess = sHermiteDomain.Evaluate(0.5);

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
  if (bDebugMe) 
    {
      sm_GraphicsLoop();
      SmExtent1d  sInt = sHerm3d.GetNaturalInterval();
      smgfx_SetLook(4,6, 1,0,0) ; crPointToTest.CrvPos().Draw(); sm_GraphicsLoop();
      smgfx_SetLook(3,4, 1,0,1) ; sHerm3d.Draw(&sInt); sm_GraphicsLoop();
      smgfx_SetLook(1,2, 0,1,0) ; s3DBox.Draw(NULL) ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2, .5,.5,.5) ; sPntBox.Draw(NULL) ; sm_GraphicsLoop() ;
      sm_GraphicsLoop();
    }
#endif  
  // Find point on hermite curve closest to crPointToTest.
  SER( sHerm3d.LocalPointSolve( sHermiteDomain,
                                SM_SO_MINIMIZE,
                                crPointToTest.CrvPos(), 
                                NULL, NULL, NULL, dGuess,
                                bSuccess, sSol ));

  // If the test point dropped to the interior of the Hermite curve,
  // then we've moved past it.  [B391]
  // Note, we could have used NORMALIZE for the drop, but we want to check
  // the distance from the ends, when it doesn't drop to the interior.
  if ( bSuccess )
    {
      // First, if distance is within tolerance, it's good.
      double dDist = sSol.m_vStart.m_dSolutionValue;
      if ( dDist <= d3DApproxTol )
      {
          // crPointToTest has a direction: check that too.  [B401]
          SmVector3d sPV[2];
          sHerm3d.Evaluate( sSol.m_vStart[0], 1, FALSE, sPV );
          // We actually want one that's pointing back into the curve that
          // we're creating, so look for negative dot product.

          if ( crPointToTest.CrvDeriv().Dot( sPV[1] ) < 0.0 )
            { return TRUE; }
      }

      // Check value on interior.  (Pass negative tol: just excludes end points.)
      double dT = sSol.m_vStart[0];
      if ( sHermiteDomain.ContainsValue( dT, -SM_EFF_ZERO ) ) 
        {
          // But, it could be on a completely different curve,
          // far away, so just give it a more liberal tolerance.  [B395]
          // Actually, it must be very liberal, > 111.  [B493]
          //   static double dFudge = 120.0;
          // Actually, it's worse than that.  [B690]
          // Don't even use tol, use the length of the Hermite curve.

#ifdef SM_DEBUG_CODE
      if ( bDebugMe ) {
          TCHAR sBuff[128];
          smos_sprintf(sBuff, _T("\n   Dist %16.8lf  Tol %16.8lf   Dist/Tol %16.8lf   Len %16.8lf   Dist/Len %16.8lf"), 
            dDist, d3DApproxTol, dDist/d3DApproxTol, dLength, dDist/dLength );
          smos_WriteBuffer( sBuff );
      }
#endif // SM_DEBUG_CODE

          double cbiDistLimit = dLength / 2.0;

          if ( dDist <= cbiDistLimit )
            { return TRUE; }
          else
            { return FALSE; } // (Just a good place for a breakpoint.)

        } // end if point drop is interior to the curve
    } // end if point drop succeeded

  // The point is not on the curve.
  return FALSE;

} // end sm_IsPointOnCurve

// ---- verbatim: SmSurfaceIntersector.cpp lines 5576-5639: sm_FlushCrv
static SmStatus sm_FlushCrv
  (const SmContext & crContext,          // in : context for new SmBSplineCurve
   ULONG lDimension,                     // in : Control Point image dimension
   SmTArray<SmPoint3d> & rCntrlPoly,     // in : Array of Control Points
   SmTArray<double> & rKnots,            // in : Array of Unique Knots
   SmTArray<SmCurve*> & crCurves)        // out: new SmBsplineCurve placed at end of array
{
    // no work - no knots
    if (rKnots.GetSize() == 0) return SM_SUCCESS;

    // make a piecewise C0, degree 3, knot multiplicity array
    // 4 knots on each end, 3 knots for each internal knot
    SmTArray<ULONG> sKnotMult(rKnots.GetSize(),NULL,rKnots.GetSize());
    sKnotMult[0] = 4;
    for (ULONG i=1; i+1<sKnotMult.GetSize(); i++)  // note: can't say sKnotMult.GetSize()-1
      {
        sKnotMult[i] = 3;
      }
    sKnotMult[sKnotMult.GetSize()-1] = 4;

    // make the degree 3 SmBsplineCurve
    SmBSplineCurve *pNewBSP = NULL ;
    ULONG lDegree = 3;
    SER(SmBSplineCurve::CreateCanonical(crContext,lDimension,lDegree,
        rCntrlPoly, SM_CF_UNSPECIFIED, sKnotMult, rKnots, 
        SM_KT_UNSPECIFIED, NULL, NULL, pNewBSP));

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
    if (bDebugMe) {
        sm_GraphicsLoop();
        pNewBSP->DrawPolygon();
        sm_GraphicsLoop();
    }
    if (FALSE) {
        pNewBSP->Dump();
    }
#endif

// Note that the following line will remove extra knots and
// decrease the size of the curve.  This can easily be done
// after drop curve.
// GWC:NOTE No knots are currently removed because all the curves
//          associated with an intersection (1 3DCurve, 2 UVTrimCurves)
//          are returned with the same parameterization.  Unless all 3 curves
//          are reduced in the same fashion the returned curves would
//          have different parameterizations.  So no reduction is done.
//    SER(pNewBSP->RemoveExtraKnots(SM_EFF_ZERO));
#ifdef SM_DEBUG_CODE
    if (FALSE) {
        pNewBSP->Dump();
    }
SmBoolean bDebugMe2 = FALSE;
    if (bDebugMe2) {
        sm_GraphicsLoop();
        pNewBSP->DrawPolygon();
        sm_GraphicsLoop();
    }
#endif

    crCurves.Add(pNewBSP);
    return SM_SUCCESS;

} // end sm_FlushCrv
// clang-format on
} // namespace sc_ssi_p1

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/intersect_surface_intersector_p1.rs).
enum : int32_t
{
    OP_STATE = 1,
    OP_POINT = 2,
    OP_NEW = 3,
    OP_COPY = 4,
    OP_ASSIGN = 5,
    OP_IS_BOUNDED = 6,
    OP_CLASSIFY = 7,
    OP_SET_EXT = 8,
    OP_SET_EDGES = 9,
    OP_DELTA = 10,
    OP_SINGULARITY = 11,
    OP_STEP_SIZE = 12,
    OP_REFINE = 13,
    OP_TEST_SPAN = 14,
    OP_ADD_CURVE = 15,
    OP_ADD_START = 16,
    OP_ADD_THROUGH = 17,
    OP_REVERSE = 18,
    OP_ON_START = 19,
    OP_ON_TSECT_CURVE = 20,
    OP_ON_CURVE = 21,
    OP_CURVE_TYPE = 22,
    OP_FLUSH = 23,
    OP_STEP_BACK = 24,
    OP_LAST = 24,
};

// Value TestSpanAccuracy's deviation output starts with (it is left unset on an angle
// rejection).
const double UNSET = -777.0;

// Op status of an Assign / Add* of a linked point (not run).
const double REJECTED = -2.0;

struct Bad : std::runtime_error
{
    Bad() : std::runtime_error("bad program") {}
};

struct Rd
{
    const double* in;
    int64_t n;
    int64_t at;
    double num()
    {
        if (at >= n)
            throw Bad();
        return in[at++];
    }
    int64_t idx(int64_t lo, int64_t hi)
    {
        double d = num();
        if (!(d >= (double)lo && d <= (double)hi) || d != (double)(int64_t)d)
            throw Bad();
        return (int64_t)d;
    }
    bool flag() { return idx(0, 1) != 0; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
    SmPoint2d uv()
    {
        double x = num();
        double y = num();
        return SmPoint2d(x, y);
    }
    SmExtent2d dom4()
    {
        double a = num();
        double b = num();
        double c = num();
        double d = num();
        return SmExtent2d(a, b, c, d);
    }
};

double flg(SmBoolean b)
{
    return b ? 1.0 : 0.0;
}

void put_dom(std::vector<double>& o, const SmExtent2d& e)
{
    o.push_back(e.GetMin().x);
    o.push_back(e.GetMin().y);
    o.push_back(e.GetMax().x);
    o.push_back(e.GetMax().y);
}

// `0 o x y scale dom` (SmPlane) or `1 p q nu nv net(x y z w) u v` (SmBSplineSurface).
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx(0, 1);
    if (kind == 0)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        SmVector2d s = rd.uv();
        SmExtent2d d = rd.dom4();
        return new (ctx) SmPlane(o, x, y, s, d, &ctx);
    }
    gw_DEGREE p = (gw_DEGREE)rd.idx(1, 8);
    gw_DEGREE q = (gw_DEGREE)rd.idx(1, 8);
    gw_INDEX nu = (gw_INDEX)rd.idx(2, 17);
    gw_INDEX nv = (gw_INDEX)rd.idx(2, 17);
    if (p >= nu || q >= nv)
        throw Bad();
    gw_INDEX n = nu - 1;
    gw_INDEX m = nv - 1;
    gw_INDEX r = n + p + 1;
    gw_INDEX s = m + q + 1;
    std::vector<double> v;
    for (int64_t i = 0; i < 4 * (int64_t)nu * nv + r + 1 + s + 1; i++)
        v.push_back(rd.num());
    gw_SURFACE* sur = sm_AllocateNurbSurface(n, m, p, q, r, s);
    size_t k = 0;
    for (gw_INDEX i = 0; i <= n; i++)
        for (gw_INDEX j = 0; j <= m; j++)
        {
            sur->net->Pw[i][j].x = v[k++];
            sur->net->Pw[i][j].y = v[k++];
            sur->net->Pw[i][j].z = v[k++];
            sur->net->Pw[i][j].w = v[k++];
        }
    for (gw_INDEX i = 0; i <= r; i++)
        sur->knu->U[i] = v[k++];
    for (gw_INDEX i = 0; i <= s; i++)
        sur->knv->U[i] = v[k++];
    return new (ctx) SmBSplineSurface(sur, FALSE, &ctx);
}

// `0 a b` (SmLine::CreateLineSegment, dim 3) or `1 dim p ncp cpts(x y z w) knots`.
SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx(0, 1);
    if (kind == 0)
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine* l = NULL;
        if (SmLine::CreateLineSegment(ctx, 3, a, b, l) != SM_SUCCESS || l == NULL)
            throw Bad();
        return l;
    }
    ULONG dim = (ULONG)rd.idx(2, 3);
    int64_t p = rd.idx(1, 8);
    int64_t ncp = rd.idx(2, 33);
    if (p >= ncp)
        throw Bad();
    int64_t n = ncp - 1;
    int64_t m = n + p + 1;
    std::vector<double> v;
    for (int64_t i = 0; i < 4 * ncp + m + 1; i++)
        v.push_back(rd.num());
    gw_CURVE* cur = sm_AllocateNurbCurve((gw_INDEX)n, (gw_DEGREE)p, (gw_INDEX)m);
    for (int64_t i = 0; i <= n; i++)
    {
        cur->pol->Pw[i].x = v[4 * i];
        cur->pol->Pw[i].y = v[4 * i + 1];
        cur->pol->Pw[i].z = v[4 * i + 2];
        cur->pol->Pw[i].w = v[4 * i + 3];
    }
    for (int64_t i = 0; i <= m; i++)
        cur->knt->U[i] = v[4 * ncp + i];
    SmBSplineCurve* c = new (ctx) SmBSplineCurve(dim, (ULONG)cur->pol->n, (ULONG)cur->knt->m, (short)cur->p,
                                                 cur->pol->Pw, cur->knt->U, &ctx);
    smos_Free(cur);
    return c;
}

SmIntersectionPointType ip_type(int64_t c)
{
    return (SmIntersectionPointType)c;
}

SmExtentPointType ep_type(int64_t c)
{
    return (SmExtentPointType)c;
}

// A point: `type ptype[4] uv0 duv0 uv1 duv1 srf0[3][3] srf1[3][3] crv_pos crv_deriv param
// angle deviation user[4]` (80 values).
void read_point(Rd& rd, SmTsectPnt& t)
{
    t.m_ePointType = ip_type(rd.idx(0, 7));
    for (int s = 0; s < 2; s++)
        for (int d = 0; d < 2; d++)
            t.m_eUVParamType[s][d] = ep_type(rd.idx(0, 5));
    for (int s = 0; s < 2; s++)
        for (int d = 0; d < 2; d++)
            t.m_vUVCurvePV[s][d] = rd.uv();
    for (int s = 0; s < 2; s++)
        for (int i = 0; i < 3; i++)
            for (int j = 0; j < 3; j++)
                t.m_vSurfacePV[s][i][j] = rd.vec();
    t.m_v3DCurvePV[0] = rd.vec();
    t.m_v3DCurvePV[1] = rd.vec();
    t.m_dCurveParameter = rd.num();
    t.m_dTangentPlaneAngleRad = rd.num();
    t.m_dDeviation = rd.num();
    for (int i = 0; i < SM_MAX_USER_DOUBLES; i++)
        t.m_adUserDoubles[i] = rd.num();
}

void put_point(std::vector<double>& o, const SmTsectPnt& t)
{
    o.push_back((double)t.m_ePointType);
    for (int s = 0; s < 2; s++)
        for (int d = 0; d < 2; d++)
            o.push_back((double)t.m_eUVParamType[s][d]);
    for (int s = 0; s < 2; s++)
        for (int d = 0; d < 2; d++)
            put_uv(o, t.m_vUVCurvePV[s][d]);
    for (int s = 0; s < 2; s++)
        for (int i = 0; i < 3; i++)
            for (int j = 0; j < 3; j++)
                put_vec(o, t.m_vSurfacePV[s][i][j]);
    put_vec(o, t.m_v3DCurvePV[0]);
    put_vec(o, t.m_v3DCurvePV[1]);
    o.push_back(t.m_dCurveParameter);
    o.push_back(t.m_dTangentPlaneAngleRad);
    o.push_back(t.m_dDeviation);
    for (int i = 0; i < SM_MAX_USER_DOUBLES; i++)
        o.push_back(t.m_adUserDoubles[i]);
}

// Reads the protected STEP flags through pointers to members named in a derived class.
struct PeekC : SmBSplineCurve
{
    static SmKnotType SmBSplineCurve::*kt() { return &PeekC::m_eKnotType; }
    static SmBSplineCurveForm SmBSplineCurve::*form() { return &PeekC::m_eBSplineCurveForm; }
};

// `type dim knot_type form has [n p m cpts(x y z w) knots] ivl[2]` of a B-spline curve.
void put_bspline(std::vector<double>& o, const SmCurve* c)
{
    o.push_back((double)c->GetType());
    o.push_back((double)c->GetDim());
    if (!c->IsKindOf(SmBSplineCurve_TYPE))
        throw Bad();
    const SmBSplineCurve* b = (const SmBSplineCurve*)c;
    o.push_back((double)(b->*PeekC::kt()));
    o.push_back((double)(b->*PeekC::form()));
    const gw_CURVE* g = b->GetGwNurbPointer();
    o.push_back(g ? 1.0 : 0.0);
    if (g)
    {
        o.push_back((double)g->pol->n);
        o.push_back((double)g->p);
        o.push_back((double)g->knt->m);
        for (gw_INDEX i = 0; i <= g->pol->n; i++)
        {
            o.push_back(g->pol->Pw[i].x);
            o.push_back(g->pol->Pw[i].y);
            o.push_back(g->pol->Pw[i].z);
            o.push_back(g->pol->Pw[i].w);
        }
        for (gw_INDEX i = 0; i <= g->knt->m; i++)
            o.push_back(g->knt->U[i]);
    }
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
}

struct Prog
{
    const SmContext& ctx;
    std::vector<SmTsectPnt*> pts;
    std::vector<SmCurve*> curves;
    ~Prog()
    {
        for (SmTsectPnt* p : pts)
            delete p;
        for (SmCurve* c : curves)
            delete c;
    }
    SmTsectPnt* pt(Rd& rd)
    {
        int64_t i = rd.idx(0, (int64_t)pts.size() - 1);
        return pts[(size_t)i];
    }
    // True when a point is on a list: linking it again (or assigning to it, which clears its
    // links) corrupts upstream's intrusive lists, so the op is not run.
    static bool linked(const SmTsectPnt* p) { return p->m_pNext != NULL || p->m_pPrev != NULL; }
    double index_of(const SmTsectPnt* p) const
    {
        for (size_t i = 0; i < pts.size(); i++)
            if (pts[i] == p)
                return (double)i;
        return -2.0;
    }
};

// The intersector with its protected state reachable.
struct Peek : SmSurfaceIntersector
{
    Peek(const SmSurface& a, const SmExtent2d& da, const SmSurface& b, const SmExtent2d& db, SmBoolean f)
        : SmSurfaceIntersector(a, da, b, db, f)
    {
    }

    void put_list(std::vector<double>& o, const Prog& pr, const SmTList<SmTsectPnt>& l) const
    {
        SmTsectPnt* aData[50];
        SmTArray<SmTsectPnt*> a(50, aData);
        l.GetAllNodes(a);
        o.push_back((double)a.GetSize());
        for (ULONG i = 0; i < a.GetSize(); i++)
            o.push_back(pr.index_of(a[i]));
    }

    void put_state(std::vector<double>& o, const Prog& pr) const
    {
        put_dom(o, m_vUVDomain[0]);
        put_dom(o, m_vUVDomain[1]);
        o.push_back(flg(m_bClosedU[0]));
        o.push_back(flg(m_bClosedU[1]));
        o.push_back(flg(m_bClosedV[0]));
        o.push_back(flg(m_bClosedV[1]));
        o.push_back(flg(m_bUseSurfaceEdges[0]));
        o.push_back(flg(m_bUseSurfaceEdges[1]));
        o.push_back(m_dThisAngTolRad);
        o.push_back(m_dThisApproxTol3d);
        o.push_back(m_dSteppingParallelTolRadians);
        o.push_back(m_dStartParallelTolRadians);
        o.push_back(m_dCurveTraceDirection);
        o.push_back(m_dLastThroughParmeter);
        o.push_back(m_dExtensionDistance);
        o.push_back(flg(m_bDoingExtensionClipping));
        o.push_back(flg(m_bCurveIsClosed));
        o.push_back(flg(m_bCheckForInteriorCurves));
        o.push_back(flg(m_bDoingSelfIntersection));
        o.push_back(flg(m_bClippedByThroughPoint));
        o.push_back(flg(m_bDoBoundaryPointOnCurveTest));
        o.push_back(m_pStartPoint ? pr.index_of(m_pStartPoint) : -1.0);
        o.push_back(m_pEndPoint ? pr.index_of(m_pEndPoint) : -1.0);
        put_list(o, pr, m_vCurvePoints);
        put_list(o, pr, m_vStartPoints);
        put_list(o, pr, m_vThroughPoints);
        o.push_back((double)pr.pts.size());
        for (const SmTsectPnt* p : pr.pts)
            put_point(o, *p);
    }

    SmBoolean on_start(const SmTsectPnt& t, int64_t list, ULONG* skip) const
    {
        const SmTList<SmTsectPnt>& l = list == 0 ? m_vStartPoints : list == 1 ? m_vThroughPoints : m_vCurvePoints;
        return IsPointOnStartPoint(t, l, skip);
    }
};

void run_op(int32_t op, Rd& rd, Prog& pr, Peek& si, std::vector<double>& o)
{
    switch (op)
    {
    case OP_STATE:
        o.push_back(0.0);
        si.put_state(o, pr);
        return;
    case OP_POINT:
    {
        SmTsectPnt* p = pr.pt(rd);
        o.push_back(0.0);
        put_point(o, *p);
        return;
    }
    case OP_NEW:
        pr.pts.push_back(new SmTsectPnt());
        o.push_back(0.0);
        o.push_back((double)(pr.pts.size() - 1));
        return;
    case OP_COPY:
    {
        SmTsectPnt* p = pr.pt(rd);
        pr.pts.push_back(new SmTsectPnt(*p));
        o.push_back(0.0);
        o.push_back((double)(pr.pts.size() - 1));
        return;
    }
    case OP_ASSIGN:
    {
        SmTsectPnt* dst = pr.pt(rd);
        SmTsectPnt* src = pr.pt(rd);
        if (Prog::linked(dst))
        {
            o.push_back(REJECTED);
            return;
        }
        *dst = *src;
        o.push_back(0.0);
        return;
    }
    case OP_IS_BOUNDED:
    {
        SmTsectPnt* p = pr.pt(rd);
        o.push_back(0.0);
        o.push_back(flg(p->IsBounded()));
        return;
    }
    case OP_CLASSIFY:
    {
        SmTsectPnt* p = pr.pt(rd);
        SmPoint2d uvs[2];
        uvs[0] = rd.uv();
        uvs[1] = rd.uv();
        bool has0 = rd.flag();
        bool has1 = rd.flag();
        SmExtent2d doms[2];
        doms[0] = rd.dom4();
        doms[1] = rd.dom4();
        bool has_tol = rd.flag();
        double tol = rd.num();
        const SmSurface* srfs[2] = {has0 ? si.GetSurface(0) : NULL, has1 ? si.GetSurface(1) : NULL};
        if (has_tol)
            p->ClassifyPointParams(uvs, srfs, doms, tol);
        else
            p->ClassifyPointParams(uvs, srfs, doms);
        o.push_back(0.0);
        for (int s = 0; s < 2; s++)
            for (int d = 0; d < 2; d++)
                o.push_back((double)p->m_eUVParamType[s][d]);
        return;
    }
    case OP_SET_EXT:
        si.SetExtensionDistance(rd.num());
        o.push_back(0.0);
        return;
    case OP_SET_EDGES:
    {
        bool a = rd.flag();
        bool b = rd.flag();
        si.SetUseSurfaceEdges(a ? TRUE : FALSE, b ? TRUE : FALSE);
        o.push_back(0.0);
        return;
    }
    case OP_DELTA:
    {
        SmTsectPnt* a = pr.pt(rd);
        SmTsectPnt* b = pr.pt(rd);
        SmPoint2d uv[2];
        SmPoint2d duv[2];
        o.push_back(st(si.ComputePointValuesDelta(uv, duv, *a, *b)));
        return;
    }
    case OP_SINGULARITY:
    {
        SmPoint2d uvs[2];
        uvs[0] = rd.uv();
        uvs[1] = rd.uv();
        SmTsectPnt* p = pr.pt(rd);
        bool has_prev = rd.flag();
        SmTsectPnt* prev = pr.pt(rd);
        o.push_back(st(si.ComputeSingularityPoint(uvs, *p, has_prev ? prev : NULL)));
        return;
    }
    case OP_STEP_SIZE:
    {
        SmTsectPnt* p = pr.pt(rd);
        double old = rd.num();
        double nw = -1.0;
        SmStatus s = si.ComputeStepSize(*p, old, nw);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back(nw);
        return;
    }
    case OP_REFINE:
    {
        SmPoint2d uv0 = rd.uv();
        SmPoint2d uv1 = rd.uv();
        SmIntersectionPointType t = ip_type(rd.idx(0, 7));
        bool has_n = rd.flag();
        SmVector3d nrm = rd.vec();
        bool has_prev = rd.flag();
        SmVector2d prev[2];
        prev[0] = rd.uv();
        prev[1] = rd.uv();
        bool bdry = rd.flag();
        SmBoolean found = FALSE;
        SmVector2d out[2];
        SmStatus s = si.RefinePoint(uv0, uv1, t, has_n ? &nrm : NULL, has_prev ? prev : NULL, bdry ? TRUE : FALSE,
                                    found, out);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(found));
            if (found)
            {
                put_uv(o, out[0]);
                put_uv(o, out[1]);
            }
        }
        return;
    }
    case OP_TEST_SPAN:
    {
        SmTsectPnt* a = pr.pt(rd);
        SmTsectPnt* b = pr.pt(rd);
        SmBoolean good = TRUE;
        double dev = UNSET;
        double ang = UNSET;
        SmStatus s = si.TestSpanAccuracy(*a, *b, NULL, good, dev, ang);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(good));
            o.push_back(dev);
            o.push_back(ang);
        }
        return;
    }
    case OP_ADD_CURVE:
    case OP_ADD_START:
    case OP_ADD_THROUGH:
    {
        SmTsectPnt* p = pr.pt(rd);
        if (Prog::linked(p))
        {
            o.push_back(REJECTED);
            return;
        }
        SmStatus s = op == OP_ADD_CURVE   ? si.AddPointToCurve(*p)
                     : op == OP_ADD_START ? si.AddStartPoint(*p)
                                          : si.AddThroughPoint(*p);
        o.push_back(st(s));
        return;
    }
    case OP_REVERSE:
        o.push_back(st(si.ReverseCurveDirection()));
        return;
    case OP_ON_START:
    {
        SmTsectPnt* p = pr.pt(rd);
        int64_t list = rd.idx(0, 2);
        bool has_skip = rd.flag();
        ULONG skip = (ULONG)rd.idx(0, 1000);
        o.push_back(0.0);
        o.push_back(flg(si.on_start(*p, list, has_skip ? &skip : NULL)));
        return;
    }
    case OP_ON_TSECT_CURVE:
    {
        SmTsectPnt* a = pr.pt(rd);
        SmTsectPnt* b = pr.pt(rd);
        SmTsectPnt* c = pr.pt(rd);
        double tol = rd.num();
        o.push_back(0.0);
        o.push_back(flg(sc_ssi_p1::sm_IsPointOnCurve(*a, *b, *c, tol)));
        return;
    }
    case OP_ON_CURVE:
    {
        SmPoint3d p = rd.vec();
        bool has_skip = rd.flag();
        ULONG skip = (ULONG)rd.idx(0, 1000);
        SmCurve* aData[8];
        SmTArray<SmCurve*> crvs(8, aData);
        for (SmCurve* c : pr.curves)
            crvs.Add(c);
        o.push_back(0.0);
        o.push_back(flg(si.IsPointOnCurve(p, crvs, has_skip ? &skip : NULL)));
        return;
    }
    case OP_CURVE_TYPE:
    {
        if (pr.curves.empty())
            throw Bad();
        int64_t i = rd.idx(0, (int64_t)pr.curves.size() - 1);
        int64_t j = rd.idx(0, (int64_t)pr.curves.size() - 1);
        o.push_back(0.0);
        o.push_back((double)sc_ssi_p1::sm_ComputeCurveType(si.GetSurface(0), si.GetSurface(1),
                                                            pr.curves[(size_t)i], pr.curves[(size_t)j]));
        return;
    }
    case OP_FLUSH:
    {
        ULONG dim = (ULONG)rd.idx(1, 3);
        int64_t nk = rd.idx(0, 64);
        int64_t np = rd.idx(0, 200);
        if (!(nk == 0 || (nk >= 2 && np == 3 * (nk - 1) + 1)))
            throw Bad();
        SmPoint3d aP[16];
        SmTArray<SmPoint3d> poly(16, aP);
        for (int64_t i = 0; i < np; i++)
            poly.Add(rd.vec());
        double aK[16];
        SmTArray<double> knots(16, aK);
        for (int64_t i = 0; i < nk; i++)
        {
            double k = rd.num();
            if (i > 0 && !(k > knots[(ULONG)(i - 1)]))
                throw Bad();
            knots.Add(k);
        }
        SmCurve* aC[4];
        SmTArray<SmCurve*> out(4, aC);
        SmStatus s = sc_ssi_p1::sm_FlushCrv(pr.ctx, dim, poly, knots, out);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)out.GetSize());
            for (ULONG i = 0; i < out.GetSize(); i++)
            {
                put_bspline(o, out[i]);
                delete out[i];
            }
        }
        return;
    }
    case OP_STEP_BACK:
    {
        SmSurfParamType dir = rd.flag() ? SM_SP_V : SM_SP_U;
        ULONG srf = (ULONG)rd.idx(0, 1);
        ULONG bdir = (ULONG)rd.idx(0, 1);
        SmTsectPnt* a = pr.pt(rd);
        SmTsectPnt* b = pr.pt(rd);
        double step = rd.num();
        o.push_back(st(si.StepBackFromSingularity(dir, srf, bdir, *a, *b, step)));
        return;
    }
    default:
        throw Bad();
    }
}
} // namespace

// Program: `surfA surfB` (surfB `2` = the same object as A), `dom1[4] dom2[4] fillet`,
// `ncurves curves`, `npoints points`, `nops ops`. Output: the constructor status, then per op
// `op status [values]`, then the final state.
SC_EXPORT int32_t sc_intersect_surface_intersector_p1_run(const double* in, int64_t n_in, double* out,
                                                          int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 2)
        return SC_ERR_ARG;
    try
    {
        SmContext ctx;
        std::vector<double> o;
        Rd rd{in, n_in, 0};
        SmSurface* a = read_surface(rd, ctx);
        SmSurface* b = NULL;
        bool same = false;
        {
            // surface B: `2` for the same object, else a surface block
            double peek = rd.in[rd.at < rd.n ? rd.at : 0];
            if (rd.at < rd.n && peek == 2.0)
            {
                rd.at++;
                same = true;
                b = a;
            }
            else
                b = read_surface(rd, ctx);
        }
        SmExtent2d d1 = rd.dom4();
        SmExtent2d d2 = rd.dom4();
        bool fillet = rd.flag();
        Prog pr{ctx, {}, {}};
        int64_t nc = rd.idx(0, 8);
        for (int64_t i = 0; i < nc; i++)
            pr.curves.push_back(read_curve(rd, ctx));
        int64_t np = rd.idx(0, 64);
        for (int64_t i = 0; i < np; i++)
        {
            SmTsectPnt* t = new SmTsectPnt();
            pr.pts.push_back(t);
            read_point(rd, *t);
        }
        int64_t nops = rd.idx(0, 64);
        {
            Peek si(*a, d1, *b, d2, fillet ? TRUE : FALSE);
            o.push_back(0.0);
            for (int64_t i = 0; i < nops; i++)
            {
                int32_t op = (int32_t)rd.idx(1, OP_LAST);
                o.push_back((double)op);
                run_op(op, rd, pr, si, o);
            }
            si.put_state(o, pr);
            // unlink every point before the lists go away
            for (SmTsectPnt* t : pr.pts)
                t->ReSet();
        }
        if (!same)
            delete b;
        delete a;
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    catch (const Bad&)
    {
        return SC_ERR_ARG;
    }
    catch (...)
    {
        return SC_ERR_EXCEPTION;
    }
}
