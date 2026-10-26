// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.curve_p4`: the SmCurve members owned by the unit
// (source/SMLib/src/SmCurve.cpp, source/SMLib/inc/SmCurve.h) called through `SmCurve*` on
// SmBSplineCurve, SmHermiteCurve and SmLine objects (IsClosed, IsDegenerate,
// ApproximatePolarBox, SnapToKnots, AssertValid, HasPlaneCrossings, FixChainCurvesList,
// AnalyticCurveIntersect, SmFindCurveDistanceExtremaEFO::Evaluate), plus the file-static
// helpers and local evaluator classes of SmCurve.cpp.
//
// Static functions and classes local to SmCurve.cpp cannot be linked against and the file
// cannot be recompiled inside a namespace (it defines SmCurve members), so the helpers used
// here are VERBATIM copies of upstream SmCurve.cpp (commit c9c5979), inside the namespace
// sc_crv_p4 (line ranges noted at each block; generated, do not edit by hand).
//
// One entry point, sc_curve_curve_p4_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/curve_curve_p4.rs, which validates every
// input. A curve block is `kind dim ...` (0: SmBSplineCurve `oob n p m cpts(x y z w)
// knots`; 1: SmHermiteCurve `P1 D1 P2 D2`; 2: bounded SmLine `P V d0 d1 scale io`). `out`
// receives the op status (0 = SM_SUCCESS, else the SmStatus value) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAssertArray.h>
#include <SmBSplineCurve.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveTypes.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmGeomUtility.h>
#include <SmGlobalSolver.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmLocalSolve1d.h>
#include <SmLocalSolveNd.h>
#include <SmMatrix.h>
#include <SmNurbsCrv.h>
#include <SmPlane.h>
#include <SmPolarBox.h>
#include <SmSolutionArray.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <map>
#include <vector>

#include "sc_sm.h"
#include "sc_gw.h"

using sc::Rd;
using sc::put_vec;
using sc::read_nurb;

namespace sc_crv_p4
{
// clang-format off
// ---- verbatim: SmCurve.cpp lines 4549-4599: sm_UnitizeTangent
static SmStatus sm_UnitizeTangent
 (SmVector3d         rPVV[3],
  const SmVector3d & crGapVec,        // vec from this curve to the other
  SmVector3d       & rUnitTan,
  double           * pdLength = NULL) // just for convenience
{
  double dLen = rPVV[1].Length();
  if ( pdLength != NULL ) { *pdLength = dLen; }

  if ( dLen > SM_EFF_ZERO )  // that's where Unitize will barf.
  {
      rUnitTan = rPVV[1] / dLen;
      return SM_SUCCESS;
  }

  // Ok, what to do with a zero 1st derivative.
  // One standard thing is to set its direction the same as the 2nd deriv:
  // that's its limiting direction, the direction that it will be heading
  // with just a minuscule increase in parameter.
  // But, this is tailored to curve/curve intersection, and the zero derivative
  // is probably because we're using a projected curve.
  // In the circle example, we're at an extremum, and the second derivative
  // vector points towards the circle center, which is back in the direction
  // that this curve will be moving.
  // Here's the idea: if that direction agrees with the gap vector, then a
  // little tweak in this curve's parameter will help us get moving.  But if
  // the directions don't agree, then we're trying to move beyond the range
  // of this curve.  In that case, return the zero derivative.

  double dDot = rPVV[2].Dot( crGapVec );
  if ( dDot > SM_EFF_ZERO )
  {
      dLen = rPVV[2].Length();
      if ( dLen > SM_EFF_ZERO )  // that's where Unitize will barf.
      {
          rUnitTan = rPVV[2] / dLen;
          return SM_SUCCESS;
      }
      // Well, since the dot product was not zero, then neither vector
      // could be zero, so how could we get here?  Whatever:
      dLen = crGapVec.Length();
      if ( dLen > SM_EFF_ZERO )  // that's where Unitize will barf.
      {
          rUnitTan = crGapVec / dLen;
          return SM_SUCCESS;
      }
  }
  rUnitTan.Set( 0,0,0 );
  return SM_SUCCESS;

} // end local_UnitizeTangent
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 4620-4874: sm_FilterSolution
static SmStatus sm_FilterSolution
  (const SmCurve        & crCurve1,       // in : 1st Curve
   const SmExtent1d     & crIvl1,         // in : 1st curve interval
   const SmCurve        & crCurve2,       // in : 2nd Curve
   const SmExtent1d     & crIvl2,         // in : 2nd Curve interval
   const SmContinuityType aeConts1[2],    // in : 1st Curve Continuity Bounds
   const SmContinuityType aeConts2[2],    // in : 2nd Curve Continuity Bounds
   ULONG                  lAdjacency,     // in : 0 = Not SelfIntersecting or remote spans
                                          //      1 = Adj SelfIntersection spans: [1stCurve, 2ndCurve]
                                          //      2 = Adj SelfIntersection spans: [2ndCurve, 1stCurve]
   const SmSolution     & crSolution,     // in : Solution to review
   SmBoolean            & rbKeepSolution) // out: TRUE = Add solution m_pSolutions array
                                          //      FALSE= Cull Solution
{
  // init output
  rbKeepSolution = TRUE;

  // If we have an exact answer keep it
  if (crSolution.m_vStart.m_dSolutionValue < SM_EFF_ZERO)
    { rbKeepSolution = TRUE;
      return SM_SUCCESS;
    }

  // Eliminate adjacency cases when doing self intersection
  if(   (lAdjacency == 1 && SM_ARE_SAME(crIvl1.GetMin(),crSolution.m_vStart[0]))
     || (lAdjacency == 2 && SM_ARE_SAME(crIvl1.GetMax(),crSolution.m_vStart[0])))
    {
      rbKeepSolution = FALSE;
      return SM_SUCCESS;
    }

  // If we have an inexact answer see if it lies on a boundary

  // Get Solution CurvePoints and CurveTangents
  SmVector3d sPV1[2], sPV2[2];
  SER(crCurve1.Evaluate(crSolution.m_vStart[0],1,TRUE,sPV1, TRUE));   // nonZeroTangent values
  SER(crCurve2.Evaluate(crSolution.m_vStart[1],1,TRUE,sPV2, TRUE));   // nonZeroTangent values

  // keep answers good to ScaledZero.  [tol value: 080704]
  // dScaledZero needs to be sensitive to parameter values [B628]
  //double dScaledZero = SM_EFF_ZERO * (1.0 + smos_Max(
  //        smos_Max( sPV1[0].GetMaxDimension(), sPV1[1].GetMaxDimension() ),
  //        smos_Max( sPV2[0].GetMaxDimension(), sPV2[1].GetMaxDimension())) );

  double dScaledZero = SM_EFF_ZERO * (1.0 + smos_4Max(
          smos_Max( sPV1[0].GetMaxDimension(), sPV1[1].GetMaxDimension() ),
          smos_Max( sPV2[0].GetMaxDimension(), sPV2[1].GetMaxDimension() ),
          crSolution.m_vStart[0], crSolution.m_vStart[1]) );
  if (crSolution.m_vStart.m_dSolutionValue < dScaledZero)
    {
      rbKeepSolution = TRUE;
      return SM_SUCCESS;
    }

  // arrive here for non-zero intersection points

  // classify the solution Point against each interval
  SmBoolean bStart1  = SM_ARE_SAME(crIvl1.GetMin(),crSolution.m_vStart[0]) ;
  SmBoolean bEnd1    = SM_ARE_SAME(crIvl1.GetMax(),crSolution.m_vStart[0]) ;
  SmBoolean bMid1    = !bStart1 && !bEnd1 ;
  SmBoolean bNext1   =  bMid1 // True if there is more curve beyond a boundary
                       || ( bStart1 && aeConts1[0] >= SM_CT_G1 )
                       || ( bEnd1   && aeConts1[1] >= SM_CT_G1 );

  SmBoolean bStart2  = SM_ARE_SAME(crIvl2.GetMin(),crSolution.m_vStart[1]) ;
  SmBoolean bEnd2    = SM_ARE_SAME(crIvl2.GetMax(),crSolution.m_vStart[1]) ;
  SmBoolean bMid2    = !bStart2 && !bEnd2 ;
  SmBoolean bNext2   =    bMid2
                       || ( bStart2 && aeConts2[0] >= SM_CT_G1 )
                       || ( bEnd2   && aeConts2[1] >= SM_CT_G1 );

  // get gap vector
  SmVector3d sGapVector = sPV2[0] - sPV1[0];

  // toss answers on interval boundaries
  //  when the next interval will be closer than this one.

  // when either solution is on a boundary with a next segment
  if(   (!bMid1 && bNext1 ) || (!bMid2 && bNext2) )
    {
      // If either solution is on a cache boundary with a next segment,
      // and the curve is smooth at the boundary,
      // then if the curve is not exactly perpendicular to the gap,
      // it's getting closer in one direction or the other.
      // So reject it unless it's 'exactly' perpendicular.
      // [B124 and similar]
      // But unfortunately, this causes other problems. [B125]
      // (And 124 has been taken care of elsewhere.)
      // SmVector3d sUnitTan;
      // if( !bMid1 && bNext1 ) // on boundary of curve 1
      // {
      //     sm_UnitizeTangent( sPV1, sGapVector, sUnitTan );
      // }
      // else // on boundary of curve 2
      // {
      //     sm_UnitizeTangent( sPV2, sGapVector, sUnitTan );
      // }
      // double dDot = sUnitTan.Dot( sGapVector );
      // if ( smos_Fabs( dDot ) > 10*SM_EFF_ZERO )
      //   {
      //     rbKeepSolution = FALSE;
      //     return SM_SUCCESS;
      //   }


//      SmBoolean bKeep = TRUE ;
      // get unit tangents pointing into next segment
      SmVector3d sT1, sT2;
      sm_UnitizeTangent( sPV1,  sGapVector, sT1 );  if ( bStart1 ) { sT1 = -sT1; }
      sm_UnitizeTangent( sPV2, -sGapVector, sT2 );  if ( bStart2 ) { sT2 = -sT2; }

      // toss sol when next segment is closer to other solution current point
      double dT1Gap = sT1.Dot(sGapVector) ;
      double dT2Gap = sT2.Dot(sGapVector) ;

      SmBoolean bPerp1 = FALSE; // = sT1.IsPerpendicularTo(sGapVector, 0.5); [B124 and similar]
      SmBoolean bPerp2 = FALSE; // = sT2.IsPerpendicularTo(sGapVector, 0.5);

      SmBoolean bPerp12 = bPerp1 || bPerp2 ;

      double dDotLimit = SM_EFF_ZERO_SQRT;  // Can't be smaller than this.  [B139 B175 B452]

      if(   (!bMid1 && bNext1 && dT1Gap >  dDotLimit && !bPerp1)
         || (!bMid2 && bNext2 && dT2Gap < -dDotLimit && !bPerp2))
        {
          rbKeepSolution = FALSE;
          return SM_SUCCESS;
        }

      // toss solutions when next segment is closer to other solution next points.
      //   Distance in next interval:
      //   dist**2 = ((P2 + t2 * V2) - (P1 + t1 * V1))^2
      //           = ( (P2-P1) + t2*V2 - t1*V1 )^2
      //           = Gap.Gap + 2*Gap.(t2*V2 - t1*V1) + (t2*V2 - t1*V1)**2
      //           = Gap.Gap + 2*(Gap.V2)*t2 - 2*(Gap.V1)*t1 + (V2.V2)*t2^2 - 2*(V2.V1)*t1*t2 + (V1.V1)*t1^2
      //   d(dist**2)/dt2 =  2*(Gap.V2) + 2*(V2.V2) * t2 - 2*(V1.V2) * t1
      //   d(dist**2)/dt1 = -2*(Gap.V1) + 2*(V1.V1) * t1 - 2*(V1.V2) * t2
      //   Next segment is diverging when
      //      d(dist**2)/dt >= 0.0 at t=0
      //   Converging in next interval when Gap*V2 < 0

      // compare Next1 with This2
      if(!bMid1 && bNext1 && !bPerp12)
        { SmVector3d sV21 = -sT2 - sT1 ;
          double dV21Gap = sV21.Dot(sGapVector) ;
          if(dV21Gap < -dDotLimit )  // SM_EFF_ZERO, not 0 [B139]
            { rbKeepSolution = FALSE;
              return SM_SUCCESS;
            }
        }
      // compare Next1 with Next2
      if(!bMid1 && bNext1 && bNext2 && !bPerp12)
        { SmVector3d sV21 = sT2 - sT1 ;
          double dV21Gap = sV21.Dot(sGapVector) ;
          if(dV21Gap < -dDotLimit )
            { rbKeepSolution = FALSE;
              return SM_SUCCESS;
            }
        }
      // compare Next2 with This1
      if(!bMid2 && bNext2 && !bPerp12)
        { SmVector3d sV21 = sT2 + sT1 ;
          double dV21Gap = sV21.Dot(sGapVector) ;
          if(dV21Gap < -dDotLimit )
            { rbKeepSolution = FALSE;
              return SM_SUCCESS;
            }
        }
      // compare Next2 with Next1
      if(!bMid2 && bNext2 && bNext1 && !bPerp12)
        { SmVector3d sV21 = sT2 - sT1 ;
          double dV21Gap = sV21.Dot(sGapVector) ;
          if(dV21Gap < -dDotLimit )
            { rbKeepSolution = FALSE;
              return SM_SUCCESS;
            }
        }

      // we are not comparing this1 with this2
      // in the end we may want to

    } // end solution on an internal boundary check

  // keep minimization points found at curve/curve tangentIntersections
  // when CurveTangents are perpendicular to gapVector to within 1 degree
  if (   sGapVector.IsPerpendicularTo(sPV1[1],1.0)
      && sGapVector.IsPerpendicularTo(sPV2[1],1.0) )
    {
// gwc: moved this section out of the minimization branch and generalized it above
//      Keep this until change is tested
//
//    // toss answers on interval boundaries
//    //  when the next interval will be closer than this one.
//    if(   (!bMid1 && bNext1)  // solution on an interval boundary
//       || (!bMid2 && bNext2)) // with more curve intervals to go.
//      {
//        // get Curve unitized tangents
//        SmVector3d sT1 = sPV1[1] ; sT1.Unitize() ;
//        SmVector3d sT2 = sPV2[1] ; sT2.Unitize() ;
//
//        // get tangents pointing into the next intervals
//        if(bStart1) { sT1 = -sT1 ; }
//        if(bStart2) { sT2 = -sT2 ; }
//        double dT1T2 = sT1.Dot(sT2) ;
//        if(bMid1)   { SM_ASSERT(!bMid2) ;
//                      if(dT1T2 < 0.0) { sT1 = -sT1 ;
//                                        dT1T2 = - dT1T2 ;
//                                      }
//                    }
//        if(bMid2)   { SM_ASSERT(!bMid1) ;
//                      if(dT1T2 < 0.0) { sT2 = -sT2 ;
//                                        dT1T2 = - dT1T2 ;
//                                      }
//                    }
//
//        // distance in next interval
//        // dist**2 = ((P2 + u * V2) - (P1 + u * V1)) * ((P2 + u * V2) - (P1 + u * V1))
//        //         = (P21 + u * V21) * (P21 + u * V21)
//        //         = P21*P21 + 2*u*V21*P21 + u*u*V21*V21
//        // d(dist**2)/du = 2*V21*P21 + 2*u*V21*V21
//        // Next segment is diverging when
//        //    d(dist**2)/du >= 0.0 at u=0.0
//        // Converging in next interval when V21*P21 < 0
//        SmVector3d sV21 = sT2 - sT1 ;
//        if(sV21.Dot(sGapVector) < 0.0)
//          {
//            rbKeepSolution = FALSE;
//            return SM_SUCCESS;
//          }
//      } // end solution on interval boundary check
//
      // keep answer assuming this is a point of closest approach.
      rbKeepSolution = TRUE;
      return SM_SUCCESS;

    } // end point of closest approach solution check

  // arrive here for nonZero, nonTangent intersection cases

  // keep intersections on Curve Boundaries (not interval boundaries)
  //  A curve boundary will have a low continuity interval boundary
  if(   (aeConts1[0] <= SM_CT_C0 && bStart1)
     || (aeConts1[1] <= SM_CT_C0 && bEnd1)
     || (aeConts2[0] <= SM_CT_C0 && bStart2)
     || (aeConts2[1] <= SM_CT_C0 && bEnd2))
    {
      rbKeepSolution = TRUE;
      return SM_SUCCESS;
    }

  // else discard the answer
  rbKeepSolution = FALSE;
  return SM_SUCCESS;

} // end sm_FilterSolution
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 8395-8434: sm_GetProjDist
static double sm_GetProjDist
  (const SmCurve & crCurve,           // in : target curve
   double dParameter,                 // in : target parameter
   const SmPoint3d & crTestPoint,     // in : given test point
   const SmVector3d & crPlaneNorm,    // in : plane normal defining coordinate system for angle measure
   SmBoolean bReverseOrder,           // in : TRUE  = return negated signed distance values
                                      //      FALSE = return default signed distance value.
                                      //        NOT USED when eSolOp is PROJECTED or DIRECTED MINIMIZE/MAXIMIZE
   const SmVector3d & crDirVector,    // in : for non projected/directed cases returning signed distances
                                      //      direction test vector to determing plus or minus values.
   SmSolverOperationType eSolvOp)     // in : current solution problem type
{
  // evaluate curve at given parameter
  SmPoint3d sPnt;
  SER(crCurve.EvaluatePoint(dParameter,sPnt));

  // get vector from testPoint to curvePoint projected to given plane
  SmVector3d sVec     = crTestPoint - sPnt;
  SmVector3d sVecProj = crPlaneNorm * sVec * crPlaneNorm;

  // get projected vector length
  double dDist = sVecProj.Length();

  // return appropriate answer
  switch(eSolvOp)
    {
      case SM_SO_PROJECTED_MINIMIZE :
      case SM_SO_PROJECTED_MAXIMIZE : SM_ASSERT(dDist >= 0.0) ;
                                      return dDist ;

      case SM_SO_DIRECTED_MINIMIZE  :
      case SM_SO_DIRECTED_MAXIMIZE  : return smos_Fabs(dDist) ;

      default :                       if (bReverseOrder) sVecProj = -sVecProj;
                                      return  (crDirVector.Dot(sVecProj) >= 0.0)
                                             ?  dDist
                                             : -dDist ;
    } // end switch on eSolvOp

} // end sm_GetProjDist
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 14815-15841: SmCurvePropertyEFO (class, Evaluate)
class SmCurvePropertyEFO : public SmEvalFunctionObject
{
protected:
  const SmCurve       & m_crCurve ;           // target curve
  SmCurvePropertyType   m_eCurveProperty ;    // curve property to evaluate
  const double        * m_cpdPropertyValue ;  // a constant used to compute property value in some cases.
                                              //  Commonly a radius, distance, or size value.
                                              //  For functions of the form
                                              //   Value = Property(Curve(T)) - constant
  const SmVector3d    * m_cpOptVectors ;      // points and vectors used in the property evaluation
                                              //   with different semantics for different uses
  double                m_dSquaredTolerance ; // Tolerance for a zero value in some cases

public:
  // constructor
  SmCurvePropertyEFO
    (const SmCurve       & crCurve,           // in : target curve
     SmCurvePropertyType   eCurveProperty,    // in : curve property to evaluate
     const double        * cpdPropertyValue,  // in : needed constant to compute some property values
     const SmVector3d    * cpOptVectors,      // in : points and vectors used in the property evaluation
     double                dSquaredTolerance) // in : Tolerance for a zero value in some cases
                                              : m_crCurve          (crCurve),
                                                m_eCurveProperty   (eCurveProperty),
                                                m_cpdPropertyValue (cpdPropertyValue),
                                                m_cpOptVectors     (cpOptVectors),
                                                m_dSquaredTolerance(dSquaredTolerance)
                                              { }

  // destructor
  virtual ~SmCurvePropertyEFO() { }

  // evaluator
  virtual SmStatus Evaluate(double      dT,                   // in : target T value to query
                            double    & rdFOfT,               // out: F(T)     = Function value for given dT value
                            double    & rdFPrimeOfT,          // out: dF(T)/dT = Function derivative value for given dT value
                            SmBoolean & rbFoundAnswer,        // out: for some cases: TRUE = Function value is within tolerance of zero, FALSE=Not
                            SmBoolean   bSignalErrors=TRUE) ; // in : TRUE = signal errors, FALSE=errors anticipated, don't signal, default:[TRUE]

} ; // end SmCurvePropertyEFO

/*******************************************************************//***
PURPOSE--- evaluate curve at given parameter to compute
     appropriate curve property and jacobian for that property
     used in a NewtonRaphson Search to find maxima of that property.

USAGE---
  where C  = C(T)         a point on the curve
        C' = dC(T)/dT     curve 1st derivative at point C
        C" = d2C(t)/dt**2 curve 2nd derivative at point C

  m_eCurveProperty                   F(T)                          m_cpdPropertyValue m_cpOptVectors    m_dSquaredTolerance

 1. SM_CP_POINT_INVERSION              F=(C-P)**2,                   Not Used,          [0] = P a point,   Not Used
 2. SM_CP_PERPENDICULAR_TO_VECTOR      F=Dot(C',V)                   Not Used,          [0] = V a vector,  Not Used
 3. SM_CP_INFLECTION_POINTS            F=(C"xC')**2,                 Not Used,          Not Used,          When F < Tol FoundAnswer = TRUE
 4. SM_CP_RADIUS_OF_CURVATURE          F=Curvature-DesiredCurvature, Desired Radius,    Not Used,          When F < Tol FoundAnswer = TRUE
 5. SM_CP_FIRST_DERIVATIVE_LENGTH      F=Len(1stDeriv)-DesiredLen,   Desired Len,       Not Used,          When F < Tol FoundAnswer = TRUE
 6. SM_CP_PARALLEL_TO_VECTOR           F=(||C'||xV)**2,              Not Used,          [0] = V a vector,  Not Used
 7. SM_CP_SILHOUETTE_VECTOR            F=C' x C'' x C' . V,          Not Used,          [0] = View Vector, Not Used
 8. SM_CP_SILHOUETTE_POINT             F=C' x C'' x C' . (C-P)       Not Used,          [0] = View Point,  Not Used
 9. SM_CP_X_NORMAL                     F= C'.x                       Not Used,          Not Used,          Not Used
10. SM_CP_Y_NORMAL                     F= C'.y                       Not Used,          Not Used,          Not Used
11. SM_CP_Z_NORMAL                     F= C'.z                       Not Used,          Not Used,          Not Used
12. SM_CP_XYZ_NORMAL                   F= C'.x * C'.y * C'.z         Not Used,          Not Used,          Not Used
13. SM_CP_MINIMIZE_FIRST_DERIVATIVE    F= C'.C'                      Not Used,          Not Used,          Not Used
14. SM_CP_MAXIMIZE_FIRST_DERIVATIVE    F= 1 / ( C'.C' )              Not Used,          Not Used,          Not Used
15. SM_CP_MINIMIZE_RADIUS_OF_CURVATURE F= |C'||C'||C'|/|C'x C"|      Not Used,          Not Used,          Not Used
16. SM_CP_MAXIMIZE_RADIUS_OF_CURVATURE F= |C'x C"|/|C'||C'||C'|      Not Used,          Not Used,          Not Used
17. SM_CP_PLANE_INTERSECTION           F=C.sABC + D                  Plane D value,     [0] = Plane Normal,When F < Tol FoundAnswer = TRUE
18. SM_CP_CYLINDER_INTERSECTION        F=DistFromCylinder            Cylinder Radius,   [0] = CylCenter,   Not Used
                                                                                        [1] = CylAxis,

19. SM_CP_MINIMIZE_CCW_ANGLE           F=(C'xProjPlaneNormal).(C-Origin), Not Used,     [0] = PlanePoint   Not Used
    SM_CP_MINIMIZE_ANGLE_TO_PLANE                                                       [1] = PlaneNormal
    SM_CP_MINIMIZE_DIRECTED_ANGLE

20. SM_CP_PROJECTED_POINT_MINIMIZE     F=STanProj.Dot(SGapProj)                         [0] = PlanePoint   Not Used
    SM_CP_PROJECTED_POINT_MAXIMIZE                                                      [1] = PlaneNormal

21. SM_CP_DISTANCE_TO_POINT            F=(C-P)**2 - Distance**2      Desired Distance   [0] = P a point,   Not Used

22. SM_CP_TANGENT_TO_CIRCLE            F=( C - P ) . ( C' + T )      Not Used,          [0] = Circle point,Not Used
                                                                                        [1] = Circle Tangent

23. SM_CP_MAXIMA_TO_LINE               F=Dot(C',Cross(Cross(V,C-P),V)) Not Used,        [0] = Line Point,  Not Used
                                                                                        [1] = LineTangent

24. SM_CP_MAXIMA_TO_PLANE              F=Dot(C',V)                   Not Used,          [0] = PlanePoint,  Not Used
                                                                                        [1] = PlaneNormal
25. SM_CP_PROJECTED_TANGENT_THROUGH_POINT  F = ( (C-P) x C' ) . N    Not Used,          [0] = P, Through Point
                                                                                        [1] = N, Plane Normal
***********************************************************************/
SmStatus SmCurvePropertyEFO::Evaluate
 (double      dT,             // in : target T value to query
  double    & rdFOfT,         // out: F(T)     = Function value for given dT value
  double    & rdFPrimeOfT,    // out: dF(T)/dT = Function derivative value for given dT value
  SmBoolean & rbFoundAnswer,  // out: for some curveProperty cases
                              //        TRUE  = propertyValue*propertyValue < m_dSquaredTolerance
                              //        FALSE = propertyValue exceeds tolerance
                              //      for all other curveProperty cases, returns FALSE
  SmBoolean   bSignalErrors)  // in : TRUE = signal errors, FALSE=errors anticipated, don't signal, default:[TRUE]
{
  // init output
  rdFOfT = 0.0;
  rdFPrimeOfT = 0.0;
  rbFoundAnswer = FALSE;

  SmPoint3d sC[4];

  // Note: always evaluate from the Right span (low end of span).
  // See notes in SmCurvePropertyGS::LocalSolve(), above.
  const SmBoolean bLeftRightFlag = TRUE;

  // switch on stored curveProperty type
  switch (m_eCurveProperty)
   {
    case SM_CP_POINT_INVERSION:
        { // See where point on curve is equal to the input point.
            //
            // F = (C - P) . (C - P)
            //
            // F' = C' . (C-P)
            // Note: it seems this should be
            //   F' = 2.0 * C' . (C-P)
            // but in practice, leaving off the 2 gives quadratic convervence,
            // while including it gives only linear.
            if (m_cpOptVectors == NULL)
              { SER(SM_ERR); }
            SER(m_crCurve.Evaluate(dT,1,bLeftRightFlag,sC,TRUE)); // nonZeroTangent values
            if (sC[1].LengthSquared() < SM_EFF_ZERO)
              {
                return SM_ERR;
              }

            SmVector3d sCP = sC[0] - m_cpOptVectors[0];
            rdFOfT         = sCP.Dot( sCP );
            rdFPrimeOfT    = sCP.Dot( sC[1] );
        }
        break;

    case SM_CP_DISTANCE_TO_POINT:
        { // See where point on curve is a given distance from the input point.
            //
            // F = (C - P) . (C - P) - dist*dist
            //
            // F' = 2 * C' . (C-P)
            if ( m_cpOptVectors == NULL )
              { SER(SM_ERR); }
            SER( m_crCurve.Evaluate( dT, 1, bLeftRightFlag, sC, TRUE ));   // nonZeroTangent values
            if ( sC[1].LengthSquared() < SM_EFF_ZERO )
              {
                return SM_ERR;
              }

            SmVector3d sCP = sC[0] - m_cpOptVectors[0];
            rdFOfT         = sCP.Dot( sCP ) - (*m_cpdPropertyValue) * (*m_cpdPropertyValue);
            rdFPrimeOfT    = 2.0 * sCP.Dot( sC[1] );
        }
        break;


    case SM_CP_PERPENDICULAR_TO_VECTOR:
        {
            // See where first derivative doted with the first vector is zero.
            //
            // F = C' . V
            //
            // F' = C'' . V
            if (m_cpOptVectors == NULL)
              { SER(SM_ERR); }
            SmVector3d sV = *m_cpOptVectors;
            SER(m_crCurve.Evaluate(dT,2,bLeftRightFlag,sC));
            if (sC[1].LengthSquared() < SM_EFF_ZERO)
              {
                return SM_ERR;
              }

            rdFOfT = sV.Dot(sC[1]);
            rdFPrimeOfT = sV.Dot(sC[2]);
        }
        break;

    case SM_CP_INFLECTION_POINTS:
        // Note: Do not use on linear (straight) segments
        { // See where second derivative cross first vanishes
            //  F = (C'' x C')**2
            //  F' = 2(C'' x C')'.(C'' x C')
            SER(m_crCurve.Evaluate(dT,3,bLeftRightFlag,sC,TRUE)); // noZeroTangent values
            SmVector3d a = sC[2] * sC[1];
            SmVector3d aPrime = sC[2].DerivativeOfCrossProduct(sC[1],sC[3],sC[2]);
            rdFOfT = a.Dot(a);
            if (rdFOfT < m_dSquaredTolerance)
              {
                rbFoundAnswer = TRUE;
              }
            rdFPrimeOfT = 2.0 * aPrime.Dot(a);
        }
        break;

    case SM_CP_RADIUS_OF_CURVATURE:
        {
            if (m_cpdPropertyValue == NULL) SER(SM_ERR);
            double dK = *m_cpdPropertyValue;
            SM_ASSERT(dK > SM_EFF_ZERO);

            //
            //      |C'||C'||C'|
            // F =  ------------  -  K
            //      | C' x C'' |
            //
            // Where K is m_dPropertyValue
            //
            // As with SM_CP_MINIMIZE_RADIUS_OF_CURVATURE:
            //   F = numer / denom - K
            //   F * denom = numer - K * denom
            //
            // If b = |C'| = sC[1].Length()
            // and A = C' x C'', then
            //  numer = b*b*b
            //  denom = A.Length()
            //
            // Derivatives:
            // F' * denom + F * denom' = numer' - K * denom'
            // F' = ( numer' - K * denom' - F * denom' ) / denom
            //    = ( numer' - (K + F) * denom' ) / denom
            //
            // b' = ( C'.C'' ) / b
            // numer' = 3*b*b*bPrime
            // A' = C'' x C'' + C' x C''' , but C'' x C'' == 0.
            // denom' = A.A' / denom

            SER( m_crCurve.Evaluate( dT, 3, bLeftRightFlag, sC, FALSE ));   // exact tangent values ?does this need to change?
            double b = sC[1].Length();
            if (SM_IS_ZERO(b))
              {
                SER(SM_ERR);  // Zero first derivative error
              }
            double dNumer = b*b*b;

            SmVector3d A = sC[1] * sC[2];
            double dDenom = A.Length();
            if ( SM_IS_ZERO( dDenom ))
              {
                return SM_ERR;  // No curvature: infinite radius
              }
            rdFOfT = dNumer / dDenom  -  dK;

            double bPrime      = sC[1].Dot( sC[2] ) / b;
            double dNumerPrime = 3*b*b*bPrime;
            SmVector3d APrime  = sC[1] * sC[3];   // + sC[2]*sC[2]  <- v*v == 0.
            double dDenomPrime = ( A.Dot( APrime ) ) / dDenom;
            rdFPrimeOfT = ( dNumerPrime - ( dK + rdFOfT ) * dDenomPrime ) / dDenom;


            if ( smos_Fabs( rdFOfT*rdFOfT ) < m_dSquaredTolerance )
              {
                rbFoundAnswer = TRUE;
              }
        }
        break;

    case SM_CP_FIRST_DERIVATIVE_LENGTH:
        {
            if (m_cpdPropertyValue == NULL) SER(SM_ERR);
            double dK = *m_cpdPropertyValue;
            SER(m_crCurve.Evaluate(dT,2,bLeftRightFlag,sC,FALSE));  // exact tangent lengths
            //
            //    F = |C'| - K
            //
            //    F' = (2C'.C'') / (2 * |C'|) - this works well
            //
            // Where K is m_dPropertyValue which is equivalent of
            //                 2
            //    F = C'.C' - K    -- out for now
            //
            //    F' = 2C'.C''     -- out for now
            //
            double dLen = smos_Sqrt(sC[1].Dot(sC[1]));
            rdFOfT = dLen - dK;
            if (smos_Fabs(rdFOfT) < m_dSquaredTolerance) rbFoundAnswer = TRUE;

            if (dLen < SM_EFF_ZERO_SQ)
              {
                if(rdFOfT < SM_EFF_ZERO)
                  { rdFPrimeOfT = 0.0 ;
                    return SM_SUCCESS ;
                  }
                else
                  { return SM_ERR; }
              }
  //            rdFOfT = sC[1].Dot(sC[1]) - dK*dK;
            rdFPrimeOfT =  ( sC[1].Dot(sC[2]) ) / dLen;
  //            rdFPrimeOfT = 2.0 * sC[1].Dot(sC[2]);
        }
        break;

    case SM_CP_PARALLEL_TO_VECTOR:
        {
            if (m_cpOptVectors == NULL)
              { SER(SM_ERR); }
            SmVector3d sV = m_cpOptVectors[0];
            SER(m_crCurve.Evaluate(dT,3,bLeftRightFlag,sC, FALSE));  // exact Tangent values
            //          C'
            // F =  ( -----  x V ) . ( same thing )
            //         |C'|
            //
            // F' =  2.0 * (same thing) . (same thing)';
            //
            // Note: in practice, leaving off the factor of 2 in the F'
            // calculation gives quadratic convervence,
            // while including it gives only linear.

            // NOTE: The derivative of this objective function goes to
            // zero at solution.  A different objective function was
            // tried, to avoid this.  [5/19/06, modified 10/24/08]  But,
            // the modified function has extra zeroes (where the 2nd derivative
            // lines up with vec).  When it was discovered that leaving off the
            // factor of 2 in the original F' gives quadratic convergence, the
            // original objective functions were reinstated. [bd, 11/7/08]

            double dLength = sC[1].Length();
            if ( SM_IS_ZERO(dLength) )
              { return SM_ERR; }  // This happens, don't make noise about it.  { SER(SM_ERR); }
            SmVector3d sCU = sC[1] / dLength;
            SmVector3d sCUP = sC[1].UnitizedDerivative( sC[2] );
            SmVector3d sSameThing = sCU * sV;
            rdFOfT = sSameThing.Dot( sSameThing );

            SmVector3d sVP(0,0,0);
            SmVector3d sSameThingPrime = sCU.DerivativeOfCrossProduct(sV,sCUP,sVP);
            rdFPrimeOfT = sSameThing.Dot( sSameThingPrime );   // * 2.0;
        }
        break;

    case SM_CP_SILHOUETTE_VECTOR:
        {
            if (m_cpOptVectors == NULL)
              { SER(SM_ERR); }
            SmVector3d sV = m_cpOptVectors[0] ;
            // Dot product of oscillating plane vector and V should be zero
            //
            // F = C' x C'' x C' . V   -  where C' and C'' are unitized
            //
            // F  = R x C' . V
            //
            // F' = (A x C')'.V
            // where A is (C'xC'')  // might work  - does work after lots of effort
            SER(m_crCurve.Evaluate(dT,3,bLeftRightFlag,sC,TRUE)); // nonZeroTangent values
            SmVector3d sCPU = sC[1];
            if (sC[1].LengthSquared() < SM_EFF_ZERO)
              {
                return SM_ERR;
              }

            // no silhouette when tangent is perp to the viewing vector.
            if (smos_Fabs(sC[1].Dot(sV)) < SM_EFF_ZERO_SQRT)
              {
                return SM_ERR;
              }

            sCPU.Unitize();  // C' unitized
            SmVector3d sCPUP = sC[1].UnitizedDerivative(sC[2]);  // C' Unitized derivative
            if (sC[2].LengthSquared() < SM_EFF_ZERO)
              {
                return SM_ERR;
              }
            SmVector3d sCPPU = sC[2];
            sCPPU.Unitize(); // C'' unitized
            SmVector3d sCPPUP = sC[2].UnitizedDerivative(sC[3]); // C'' unitized derivative
            SmVector3d sA = sCPU * sCPPU;
            if (sA.LengthSquared() < SM_EFF_ZERO)
              {
                return SM_ERR;
              }
            SmVector3d sOscPlaneVec = sA * sCPU;
            rdFOfT                  = sV.Dot(sOscPlaneVec);
            SmVector3d sAPrime      = sCPU.DerivativeOfCrossProduct(sCPPU,sCPUP,sCPPUP);
            SmVector3d sTmp         = sA.DerivativeOfCrossProduct(sCPU,sAPrime,sCPUP);
            rdFPrimeOfT             = sTmp.Dot(sV);
        }
        break;

    case SM_CP_SILHOUETTE_POINT:
        {
            if (m_cpOptVectors == NULL)
              { SER(SM_ERR); }
            SmPoint3d sP = *m_cpOptVectors;
            SER(m_crCurve.Evaluate(dT,3,bLeftRightFlag,sC,TRUE)); // nonZeroTangent values
            SmVector3d sCMinusP = sC[0] - sP;
            // Dot product of vector form point to curve with oscellating plane vector
            // should go to zero.
            //
            // F = C' x C'' x C' . (C-P)   - where C' and C'' are unitized
            //
            // F' = (A x C')'. (C-P) + (A x C').C'   -  Note final C' is not unitized
            // where A is (C'xC'')  // might work  - might not
            SmVector3d sCPU = sC[1];
            if (sC[1].LengthSquared() < SM_EFF_ZERO)
              {
                return SM_ERR;
              }

            // If tangent is in same plane as silhouette vector than
            // it is not really a silhouette.
            if (smos_Fabs(sC[1].Dot(sCMinusP)) < SM_EFF_ZERO_SQRT)
              {
                return SM_ERR;
              }

            sCPU.Unitize();  // C' unitized
            SmVector3d sCPUP = sC[1].UnitizedDerivative(sC[2]);  // C' Unitized derivative
            if (sC[2].LengthSquared() < SM_EFF_ZERO)
              {
                return SM_ERR;
              }
            SmVector3d sCPPU = sC[2];
            sCPPU.Unitize(); // C'' unitized
            SmVector3d sCPPUP = sC[2].UnitizedDerivative(sC[3]); // C'' unitized derivative
            SmVector3d sA = sCPU * sCPPU;
            if (sA.LengthSquared() < SM_EFF_ZERO)
              {
                return SM_ERR;
              }
            SmVector3d sOscPlaneVec = sA * sCPU;
            rdFOfT = sCMinusP.Dot(sOscPlaneVec);
            SmVector3d sAPrime = sCPU.DerivativeOfCrossProduct(sCPPU,sCPUP,sCPPUP);
            SmVector3d sTmp = sA.DerivativeOfCrossProduct(sCPU,sAPrime,sCPUP);
            rdFPrimeOfT = sTmp.Dot(sCMinusP) + sOscPlaneVec.Dot(sC[1]);
        }
        break;

    case SM_CP_X_NORMAL:
        {
            SER(m_crCurve.Evaluate(dT,2,bLeftRightFlag,sC));
            // F  = C'.x
            // F' = C''.x
            rdFOfT      = sC[1].x;
            rdFPrimeOfT = sC[2].x;
            if (SM_IS_ZERO(rdFOfT) && SM_IS_ZERO(rdFPrimeOfT))
              {
                // Have answer but it doesn't count because vector is perpendicular
                // to osculating plane
                return SM_ERR;
              }
        }
        break;

    case SM_CP_Y_NORMAL:
        {
            SER(m_crCurve.Evaluate(dT,2,bLeftRightFlag,sC));
            rdFOfT = sC[1].y;
            rdFPrimeOfT = sC[2].y;
            if (SM_IS_ZERO(rdFOfT) && SM_IS_ZERO(rdFPrimeOfT))
              {
                // Have answer but it doesn't count because vector is perpendicular
                // to osculating plane
                return SM_ERR;
              }
        }
        break;

    case SM_CP_Z_NORMAL:
        {
            SER(m_crCurve.Evaluate(dT,2,bLeftRightFlag,sC));
            rdFOfT = sC[1].z;
            rdFPrimeOfT = sC[2].z;
            if (SM_IS_ZERO(rdFOfT) && SM_IS_ZERO(rdFPrimeOfT))
              {
                // Have answer but it doesn't count because vector is perpendicular
                // to osculating plane
                return SM_ERR;
              }
        }
        break;

    case SM_CP_XYZ_NORMAL:
        {
            SER(m_crCurve.Evaluate(dT,2,bLeftRightFlag,sC));
            // F = C'.x * C'.y * C'.z
            // F' = C''.x*(C'.y*C'.z) + C'.x(C''.y*C'.z + C'.y*C''.z)
            rdFOfT = sC[1].x * sC[1].y * sC[1].z;
            rdFPrimeOfT = sC[2].x * (sC[1].y*sC[1].z) +
                          sC[1].x * (sC[2].y*sC[1].z + sC[1].y*sC[2].z);
            if (SM_IS_ZERO(sC[1].x) && SM_IS_ZERO(sC[2].x))
              {
                rdFOfT = sC[1].y * sC[1].z;
                rdFPrimeOfT = (sC[2].y*sC[1].z + sC[1].y*sC[2].z);
                if (SM_IS_ZERO(rdFOfT) && SM_IS_ZERO(rdFPrimeOfT))
                  { return SM_ERR; }
              }
            if (SM_IS_ZERO(sC[1].y) && SM_IS_ZERO(sC[2].y))
              {
                rdFOfT = sC[1].x * sC[1].z;
                rdFPrimeOfT = (sC[2].x*sC[1].z + sC[1].x*sC[2].z);
                if (SM_IS_ZERO(rdFOfT) && SM_IS_ZERO(rdFPrimeOfT))
                  { return SM_ERR; }
              }
            if (SM_IS_ZERO(sC[1].z) && SM_IS_ZERO(sC[2].z))
              {
                rdFOfT = sC[1].x * sC[1].y;
                rdFPrimeOfT = (sC[2].x*sC[1].y + sC[1].x*sC[2].y);
                if (SM_IS_ZERO(rdFOfT) && SM_IS_ZERO(rdFPrimeOfT))
                  { return SM_ERR; }
              }
        }
        break;

    case SM_CP_MINIMIZE_FIRST_DERIVATIVE:
        {
            SER(m_crCurve.Evaluate(dT,2,bLeftRightFlag,sC,FALSE)); // exact tangent lengths
            //
            // F = C'.C'
            // F' = 2 C' . C''
            //
            rdFOfT = sC[1].Dot(sC[1]);
            rdFPrimeOfT = 2.0 * sC[1].Dot(sC[2]);
        }
        break;

    case SM_CP_MAXIMIZE_FIRST_DERIVATIVE:
        {
            SER(m_crCurve.Evaluate(dT,2,bLeftRightFlag,sC,FALSE)); // exact tangent lengths
            //
            // F = 1 / ( C'.C' )
            //
            //         - ( C'.C' )'           - 2 C'.C''
            // F' = -------------------  = -----------------
            //         (C'.C')(C'.C')       (C'.C')(C'.C')
            //
            rdFOfT = 1.0/(sC[1].Dot(sC[1]));
            double dDenom = (sC[1].Dot(sC[1]) * sC[1].Dot(sC[1]));
            if (dDenom < SM_EFF_ZERO_SQ)
              { return SM_ERR; }
            rdFPrimeOfT = (- 2.0 * sC[1].Dot(sC[2])) / dDenom;
        }
        break;

    case SM_CP_MINIMIZE_RADIUS_OF_CURVATURE:
        {
            //                             1      |C'||C'||C'|
            // Radius of Curvature, rho = ---  =  ------------
            //                             k      | C' x C'' |
            //
            //        b*b*b       where A  = C' x C'',
            // F  = ---------,          b  = magnitude of C', sqrt(C'.C'),
            //      sqrt( A.A )
            //
            // Derivatives:
            // Don't ever use the quotient rule to program something like this.
            // Do this instead:  if F = n/d, then
            //   F * d = n
            //   F' * d + F * d' = n'
            // then calculate d' and n', and use the already-calculated F:
            //   F' = ( n' - F * d' ) / d
            //
            // In our case, this would be
            //
            //  F' = ( 3*b^2*b' - F*m' ) / m
            //
            // say m = sqrt( A.A ) = ( A.A ) ^ 1/2    : magnitude of A
            //
            //     m' = ... < algebra > ... = A.A' / m
            //
            // and similarly,  b' = ( C'.C'' ) / b
            //
            // So:  F' = ( 3*b*b*b' - F*m' ) / m


            SER( m_crCurve.Evaluate( dT, 3, bLeftRightFlag, sC, FALSE ));  // exact tangent lengths

            double b = sC[1].Length();
            if (SM_IS_ZERO(b))
              {
                if(bSignalErrors)
                  {
                    SE_MSG(SM_ERR,_T("SmCurvePropertyEFO::Evaluate - Found Curve Zero 1stDeriv point in search for MinRadOfCurvature"));  // Zero first derivative error
                  }
                return(SM_ERR) ;
              }
            double dNumer = b*b*b;

            SmVector3d A = sC[1] * sC[2];
            double dDenom = A.Length();
            if (SM_IS_ZERO(dDenom))
              {
                return SM_ERR;  // No curvature: infinite radius
              }
            rdFOfT = dNumer / dDenom;

            double bPrime      = sC[1].Dot( sC[2] ) / b;
            double dNumerPrime = 3*b*b*bPrime;
            SmVector3d APrime  = sC[1] * sC[3];   // + sC[2]*sC[2]  <- v*v == 0.
            double dDenomPrime = ( A.Dot( APrime ) ) / dDenom;
            rdFPrimeOfT = ( dNumerPrime - rdFOfT*dDenomPrime ) / dDenom;
        }
        break;

    case SM_CP_MAXIMIZE_RADIUS_OF_CURVATURE:
        {
            // We can maximize radius of curvature by minimizing
            // its reciprocal, the magnitude of the curvature vector.
            //
            //      | C' x C'' |
            // F =  ------------
            //      |C'||C'||C'|
            //
            // As with SM_CP_MINIMIZE_RADIUS_OF_CURVATURE:
            //
            //       sqrt( A.A )
            // F  =  -----------
            //          b*b*b
            // where A = C' x C'',  b = sqrt(C'.C'),
            //
            //
            //
            // F' = ( m' - F * 3*b*b*b' ) / b*b*b
            //
            SER( m_crCurve.Evaluate( dT, 3, bLeftRightFlag, sC, FALSE ));  // exact tangent lengths

            SmVector3d A = sC[1] * sC[2];
            double dNumer = A.Length();
            if ( dNumer < SM_EFF_ZERO )
              {
                // gwc[removed]: return SM_ERR;  // No curvature: infinite radius
                rdFOfT = 0.0 ;
                rdFPrimeOfT = 0.0 ;
                rbFoundAnswer = TRUE ;
              }

            double b = sC[1].Length();
            if ( SM_IS_ZERO(b) )
              {
                SER(SM_ERR);  // Zero first derivative error
              }
            double dDenom = b*b*b;

            rdFOfT = dNumer / dDenom;

            SmVector3d APrime  = sC[1] * sC[3];   // + sC[2]*sC[2]  <- v*v == 0.
            double dNumerPrime = ( A.Dot( APrime ) ) / dNumer;
            double bPrime      = sC[1].Dot( sC[2] ) / b;
            double dDenomPrime = 3*b*b*bPrime;
            rdFPrimeOfT = ( dNumerPrime - rdFOfT*dDenomPrime ) / dDenom;
        }
        break;

    case SM_CP_PLANE_INTERSECTION:
        {
            if (m_cpOptVectors == NULL)
              { SER(SM_ERR); }
            SmVector3d sABC = m_cpOptVectors[0];
            if (m_cpdPropertyValue == NULL) SER(SM_ERR);
            double dD = *m_cpdPropertyValue;
            SER(m_crCurve.Evaluate(dT,1,bLeftRightFlag,sC,TRUE));  // nonZeroTangent values

            // Switch over to normalization if get really close
            rdFOfT = sC[0].Dot(sABC) + dD;
            if(   smos_Fabs(rdFOfT)          < SM_EFF_ZERO_SQRT
               && smos_Fabs(sC[1].Dot(sABC)) < SM_EFF_ZERO_SQRT)
              {
                // F = C'.sABC;
                // F' = C''.sABC;
                rdFOfT = sC[1].Dot(sABC);
                SmPoint3d sC2[3];
                SER(m_crCurve.Evaluate(dT,2,bLeftRightFlag,sC2));
                rdFPrimeOfT = sC2[2].Dot(sABC);
              }
            else
              {
                // F = C.sABC + D
                // F' = C'.sABC
                rdFPrimeOfT = sC[1].Dot(sABC);
              }
            if (smos_Fabs(rdFOfT) < SM_EFF_ZERO)
              {
                rbFoundAnswer = TRUE;
              }
        }
        break;

    case SM_CP_CYLINDER_INTERSECTION:
        {
            if (m_cpOptVectors == NULL)
              { SER(SM_ERR); }
            double dCylRadius = m_cpdPropertyValue[0] ;
            SmPoint3d sCenter = m_cpOptVectors[0];
            SmVector3d sN = m_cpOptVectors[1];
            SER(m_crCurve.Evaluate(dT,1,bLeftRightFlag,sC,TRUE));  // nonZeroTangent values
            SmVector3d sFp = sN * sC[1] * sN;
            SmVector3d sCF = sN * (sC[0] - sCenter) * sN;
            rdFOfT = sCF.Dot(sCF) - dCylRadius*dCylRadius;
            rdFPrimeOfT = 2.0 * sCF.Dot(sFp);
        }
        break;

    case SM_CP_MINIMIZE_CCW_ANGLE:
    case SM_CP_MINIMIZE_ANGLE_TO_PLANE:
    case SM_CP_MINIMIZE_DIRECTED_ANGLE:
        {
            if (m_cpOptVectors == NULL)
              { SER(SM_ERR); }
            SER(m_crCurve.Evaluate(dT,2,bLeftRightFlag,sC, TRUE));  // nonZeroTangent values
            // F = (C' x ProjPlaneNormal) . (C - Origin)
            // F' = (C' x ProjPlaneNormal)' . (C - Origin) + (C' x ProjPlaneNormal) . C'
            SmVector3d sCpxN   = sC[1] * m_cpOptVectors[1];
            SmVector3d sC_Orig = sC[0] - m_cpOptVectors[0];
            rdFOfT = sCpxN.Dot(sC_Orig);
            SmVector3d sCpxNp  = sC[1].DerivativeOfCrossProduct(m_cpOptVectors[1],
                sC[2],SmVector3d(0,0,0));
            rdFPrimeOfT        = sCpxNp.Dot(sC_Orig) + sCpxN.Dot(sC[1]);
        }
        break;

    case SM_CP_PROJECTED_POINT_MINIMIZE:
    case SM_CP_PROJECTED_POINT_MAXIMIZE:
        {
            // evaluate current guess
            SER(m_crCurve.Evaluate(dT,2,bLeftRightFlag,sC,TRUE)); // nonZeroTangent values

            SmVector3d sV       = sC[0] - m_cpOptVectors[0];
            SmVector3d sVProj   = m_cpOptVectors[1] * sV * m_cpOptVectors[1];
            SmVector3d sTanProj = m_cpOptVectors[1] * sC[1] * m_cpOptVectors[1];
            SmVector3d sSecProj = m_cpOptVectors[1] * sC[2] * m_cpOptVectors[1];

            rdFOfT = sTanProj.Dot(sVProj);
            rdFPrimeOfT = sSecProj.Dot(sVProj) + sTanProj.Dot(sTanProj);

            // GWC: The old cost functional was F(t) = sTanProj.Dot(sVProj).
            //      This went to zero in 3 ways.
            //         o. When the dot product of the two vecs went to zero,
            //             marking a closest point of approach between a point and
            //             a non-intersecting curve.
            //         o. When the VProj vector goes to zero, marking an
            //             intersection between the curve and the point.
            //         o. When the Tangent vector goes to zero, usually marking a
            //             a bad parameterization location on the curve.
            //      Generally, we want the first two solutions and not the third.
            //      So, use the modified functional that has only two kinds of zeros as,
            //         F(t) = Normalized(sTanProj).Dot(sVProj)
            //         F'(t) =  (  (sSecProj.Dot(sVProj) + sTanProj.Dot(sTanProj)) * (sTanProj.Length())
            //                   - (sSecProj.Dot(sTanProj) * sTanProj.Dot(sVProj)) / (sTanProj.Length())
            //                 / sTanProj.Dot(sTanProj)
            double dVDotV     = sVProj.Dot(sVProj) ;
            double dVDotTan   = sTanProj.Dot(sVProj) ;
            double dVDotSec   = sSecProj.Dot(sVProj) ;
            double dTanDotTan = sTanProj.Dot(sTanProj) ;
            double dTanDotSec = sTanProj.Dot(sSecProj) ;
            double dTanLen    = smos_Sqrt(dTanDotTan) ;
            double dVLen      = smos_Sqrt(dVDotV) ;

            if(dTanLen > SM_EFF_ZERO)
              {
                rdFOfT      = dVDotTan / dTanLen ;
                rdFPrimeOfT =  ((dVDotSec + dTanDotTan) * dTanLen  - (dTanDotSec * dVDotTan) / dTanLen)
                             / (dTanDotTan) ;
              }
            else
              {
                rdFOfT      = 0.0 ;
                rdFPrimeOfT = 0.0 ;
              }

            if ( m_eCurveProperty == SM_CP_PROJECTED_POINT_MAXIMIZE )
              { rdFOfT = -rdFOfT; }  // [B543]

            rbFoundAnswer = (   ( dVDotV   < (SM_EFF_ZERO_SQ * 100.0))
                             || (smos_Fabs(dVDotTan / dTanLen / dVLen) < SM_EFF_ZERO)) ;

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE ;
            if(bDebugMe)  // draw eval pt/tan, brep/curve/edge, target pt/projectvec, and proj V/Tan/sec
                          // looking for the curve/projectVec intersection
              {
                SmEdge *pEdge = (SmEdge *)m_crCurve.GetEdge() ;
                SmBrep *pBrep = pEdge ? pEdge->GetBrep() : NULL ;

                smgfx_Erase() ;
                smgfx_SetLook(4,5,  1,0,0) ; sC[0].Draw() ; sm_GraphicsLoop() ;
                smgfx_SetLook(1,2, .5,0,1) ; sC[1].Draw(&sC[0]) ; sm_GraphicsLoop() ;
                smgfx_SetLook(1,2,  0,0,1) ; if(pBrep) pBrep->Draw(TRUE) ; sm_GraphicsLoop() ;
                smgfx_SetLook(2,3, .5,1,0) ; m_crCurve.Draw(NULL,TRUE) ; sm_GraphicsLoop() ;
                smgfx_SetLook(3,4,  1,0,1) ; if(pEdge) pEdge->Draw() ; sm_GraphicsLoop() ;
                smgfx_SetLook(7,8,  0,1,1) ; m_cpOptVectors[0].Draw() ; sm_GraphicsLoop() ;
                smgfx_SetLook(1,2,  0,1,0) ; (30*m_cpOptVectors[1]).Draw(&m_cpOptVectors[0]) ; sm_GraphicsLoop() ;
                smgfx_SetLook(1,2,.5,.5,.5); sV.Draw(&m_cpOptVectors[0]) ; sm_GraphicsLoop() ;
                smgfx_SetLook(2,3,  0,0,0) ; sVProj.Draw(&m_cpOptVectors[0]) ; sm_GraphicsLoop() ;
                SmVector3d sTemp( m_cpOptVectors[0] + sVProj );
                smgfx_SetLook(4,5, 0,.5,1) ; sTanProj.Draw(&sTemp) ; sm_GraphicsLoop() ;
                smgfx_SetLook(6,7, 0,1,.5) ; sSecProj.Draw(&sTemp) ; sm_GraphicsLoop() ;
                sm_GraphicsLoop() ;
              }
#endif // SM_DEBUG_CODE

        }
        break;

    case SM_CP_TANGENT_TO_CIRCLE:
        {
            // Say sDiff is the difference vector, ( C(t) - P ).
            // Solution is when the angle between sDiff and the circle tangent
            // equals the angle between sDiff and the curve tangent.
            // The angles are measured using the dot products with the
            // (unit) circle tangent and the unitized curve tangent.
            // The circle tangent ( m_cpOptVectors[1] ) is unitized,
            // but instead of unitizing the curve tangent, we will multiply
            // the circle tangent by the magnitude of the curve tangent;
            // this makes the math cleaner.
            // So, F is the difference in those two dot products.
            // But since the difference vector would have to go one direction
            // for one dot product, and the other direction for the other dot,
            // we add the dot products against the same difference vector:
            // F = sDiff . C'  +  sDiff . m*T, where m is the magnitude
            // of C' and T is the circle tangent.
            // F = sDiff . ( C' +  m*T )
            // Since sDiff' == C',
            // F' = C' . ( C' + m*T ) + sDiff . ( C'' + m' * T )
            // Also, m' = ... = ( C' . C'' ) / m
            //
            // But this catches only one direction of the given tangent vector,
            // and we want both directions.  We can determine ahead of time
            // which direction is appropriate for the current geometry.
            // One configuration is where both of the vectors (the given vector
            // and the current curve tangent) are either 'leaving' or 'entering'
            // the circle segment that would connect them; the other is when
            // one is entering and the other is leaving.  In the second case,
            // we flip the given tangent vector.
            // This distinction can be made by intersecting the two tangent
            // vectors.  If the intersection is on the same side of both
            // vectors -- either both ahead of the point or both behind --
            // then we're in the first case, otherwise it's the second case.

            if (m_cpOptVectors == NULL)
              { SER(SM_ERR); }

            SER( m_crCurve.Evaluate( dT, 2, bLeftRightFlag, sC ));
            SmVector3d sDiff    = sC[0] - m_cpOptVectors[0];
            double dMag = sC[1].Length();
            if ( dMag < SM_EFF_ZERO )
              { SER(SM_ERR); } // Note: we could probably deal with this.

            // Determine which case we have.
            // The parameters returned from the line-line intersection
            // are positive for 'ahead' and negative for 'behind'.
            double dT0, dT1;
            SmStatus eStat = smgu_LineLineClosestPoint(
                        m_cpOptVectors[0], m_cpOptVectors[1],
                        sC[0], sC[1],
                        dT0, dT1 );

            SmBoolean bFlip = FALSE;
            if ( eStat == SM_SUCCESS )
              {
                bFlip = ( dT0*dT1 < 0 ); // If opposite signs, flip.
              }
            else
              {
                // Curve tangent is either parallel to given vector, or zero.
                // We already checked for zero, so it's parallel (or anti-parallel).
                // In that case, parallel is the first case, anti- is the other.
                double dDot = sC[1].Dot( m_cpOptVectors[1] );
                bFlip = ( dDot < 0 );
              }

            // Now the possibly-flipped vector:
            SmVector3d sTan = ( bFlip ) ? -m_cpOptVectors[1] : m_cpOptVectors[1];

            // Ok, we can finally calculate the function and derivative.
            SmVector3d sTanSum( sC[1] + dMag * sTan );
            rdFOfT = sDiff.Dot( sTanSum );

            double dMagPrime = ( sC[1].Dot( sC[2] ) ) / dMag;
            rdFPrimeOfT =   sC[1].Dot( sTanSum )
                          + sDiff.Dot( sC[2] + dMagPrime * sTan );
        }
        break;

    case SM_CP_MAXIMA_TO_LINE:
        {
            // Curve Tangent dotted with MinCurveToLineVec goes to zero.
            //   warning: This function also goes to zero whereever the curve intersects the given line
            //            To use this properly, make sure there are no curve/line intersections on input
            //            or check every output to distinguish between maxima points and curve/line intersections.
            //
            // F = Dot(C',Cross(Cross(V,C-P),V))
            // F'= Dot(C'',Cross(Cross(V,C-P),V)) + Dot(C',Cross(Cross(V,C'-P),V))
            if (m_cpOptVectors == NULL)
              { SER(SM_ERR); }
            SmVector3d sP = m_cpOptVectors[0];
            SmVector3d sV = m_cpOptVectors[1];
            SER(m_crCurve.Evaluate(dT,2,bLeftRightFlag,sC));
            if (sC[1].LengthSquared() < SM_EFF_ZERO)
              {
                return SM_ERR;
              }

            SmVector3d sNormAtC      = sV * ( sV * (sC[0] - sP) ) ;
            SmVector3d sNormAtCPrime = sV * ( sV * (sC[1] - sP) ) ;

            rdFOfT      = sNormAtC.Dot(sC[1]);
            rdFPrimeOfT = sNormAtC.Dot(sC[2]) + sNormAtCPrime.Dot(sC[1]);
        }
        break;

    case SM_CP_MAXIMA_TO_PLANE:
        {
            // See where first derivative doted with the first vector is zero.
            //
            // F = C' . V
            //
            // F' = C'' . V
            if (m_cpOptVectors == NULL)
              { SER(SM_ERR); }
            SmVector3d sV = m_cpOptVectors[1];
            SER(m_crCurve.Evaluate(dT,2,bLeftRightFlag,sC));
            if (sC[1].LengthSquared() < SM_EFF_ZERO)
              {
                return SM_ERR;
              }

            rdFOfT      = sV.Dot(sC[1]);
            rdFPrimeOfT = sV.Dot(sC[2]);
        }
        break;

    case SM_CP_PROJECTED_TANGENT_THROUGH_POINT:
        { // Find where input point is on the tangent vector.
            //
            // We do this by making the gap vector parallel to the tangent:
            // that puts the point on the tangent vector.  We do that by taking
            // the cross product of the gap vector with the curve tangent:
            // this is zero at solution.  We dot that vector with the normal
            // to give a scalar function that passes nicely through zero.
            //
            // F = ( (C-P) x C' ) . N
            //
            // F' = ( ( C' x C' ) + (C-P) x C'' ) . N
            //  but C' x C' is zero, so:
            //
            // F' = ( (C-P) x C'' ) . N
            //
            // Alternatively we could have made the gap vector perpendicular
            // to the binormal (actually, C' x N).  The functions are pretty similar.
            //
            // Note: we do not have to explicitly project anything to any planes.
            // That is finessed by the geometry: the component of a cross product
            // in the direction of any vector is unchanged if you add any component
            // of that vector to one of the vectors of the cross product.
            // (That's nice: much simpler objective functions, and especially
            // derivatives, plus less number crunching.)
            //
            // Do we want to use unitized vectors?  That is used in some places,
            // and avoids problems where a large second derivative of the curve
            // can cause the change in the first derivative to drive the
            // objective function in the wrong direction.  I tried both, and they work
            // equally well in ordinary situations, but at singularities (e.g., target
            // point being right on the curve), non-unitized actually performs a
            // little better.  That would be because the gap going to zero dominates
            // the objective function, not the change in the curve's first derivative.
            //

            if (m_cpOptVectors == NULL)
              { SER(SM_ERR); }
            SER(m_crCurve.Evaluate(dT,2,bLeftRightFlag,sC,TRUE)); // nonZeroTangent values
            if (sC[1].LengthSquared() < SM_EFF_ZERO)
              { return SM_ERR; }

            SmVector3d sGap    = sC[0] - m_cpOptVectors[0];
            // Should we use unitized derivatives?
            // In practice, non-unit converges better for the degenerate case
            // where the given point is right on the curve.

SmBoolean sbUseUnit=FALSE;
if ( sbUseUnit==FALSE ) {

            // Use non-unitized vectors.
            SmVector3d sCross1 = sGap * sC[1];
            rdFOfT             = sCross1.Dot( m_cpOptVectors[1] );
            SmVector3d sCross2 = sGap * sC[2];
            rdFPrimeOfT        = sCross2.Dot( m_cpOptVectors[1] );

} else { // sbUseUnit:

            // Unitize the vectors, carefully.
            SmVector3d sU_Gap( sGap );
            double dMag = sU_Gap.Length();
            if ( dMag < SM_EFF_ZERO * 100 )
            {
                rdFOfT = 0.0; // Point is on the curve, right here.  That's a solution.
                rdFPrimeOfT = 1.0;
                rbFoundAnswer = TRUE;
                break;  // Exit the switch statment.
            }
            sU_Gap /= dMag;

            SmVector3d sU_C_t( sC[1] );
            dMag = sU_C_t.Length();
            if ( dMag < SM_EFF_ZERO * 100 )
              { SER(SM_ERR); } // Zero-length tangent: illegal.

            SmVector3d sCross1 = sU_Gap * sU_C_t;
            rdFOfT             = sCross1.Dot( m_cpOptVectors[1] );

            SmVector3d sU_Gap_t = sGap.UnitizedDerivative( sC[1] ); // Deriv of sGap is C'.
            SmVector3d sU_C_t_t = sC[1].UnitizedDerivative( sC[2] );
            SmVector3d sCross2  = sU_Gap_t * sU_C_t + sU_Gap * sU_C_t_t ;
            rdFPrimeOfT         = sCross2.Dot( m_cpOptVectors[1] );
} // sbUseUnit.
        }
        break;

    default:
        SER(SM_ERR);

   } // end switch on m_eCurveProperty

#ifdef SM_DEBUG_CODE
static constexpr SmBoolean  bDebugMe = FALSE;
  if (bDebugMe)
    {
      sm_GraphicsLoop();
      m_crCurve.DrawAt(dT,2); sm_GraphicsLoop() ;
      sm_GraphicsLoop();
    }
#endif // SM_DEBUG_CODE
  return SM_SUCCESS;

} // end SmCurvePropertyEFO::Evaluate
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 16115-16473: SmCCTangentIntENFO (class, constructor, Evaluate, StaticEvaluate)
class SmCCTangentIntENFO : public SmEvalNFunctionsObject
{
protected:
    const SmCurve & m_crCurve1;   // 1st Curve
    const SmCurve & m_crCurve2;   // 2nd Curve
    double m_dSquaredTolerance;   // currently not used
  //double m_dSign;               //  1.0 = find xSect where curve tangents run in same direction
                                  // -1.0 = find xSect where curve tangents run in opposite direction
 // SumOfAllErrors = TangAngleSquared + GapSizeSquared
public:
    SmCCTangentIntENFO(const SmCurve & crCurve1,
                       const SmCurve & crCurve2,
                       double dSquaredTolerance );
    virtual ~SmCCTangentIntENFO() {}
    virtual SmStatus Evaluate
      (const SmTArray<double> & crX,             // in : x of Ax=F
       SmTArray<double>       & rF,              // out: F of Ax=F function values,
       SmMatrix               * pOptJacobian,    // out: Partial derivatives of the functions.
       SmBoolean              & rbFoundAnswer);  // out: Not always used, when used
                                                 //      TRUE = converged (F members are within tolerance of 0.0
                                                 //      FALSE= Not Used or Not Converged

    static SmStatus StaticEvaluate
      (const SmCurve          & crCurve1,        // in :
       const SmCurve          & crCurve2,        // in :
       const SmTArray<double> & crX,             // in : x of Ax=F
       SmTArray<double>       & rF,              // out: F of Ax=F function values,
       SmMatrix               * pOptJacobian,    // out: Partial derivatives of the functions.
       SmBoolean              & rbFoundAnswer) ; // out: Just set to FALSE = Not Used
} ; // end class SmCCTangentIntENFO


/*******************************************************************//**
PURPOSE: NewtonRaphson Iteration Evaluate function Constructor
            for tangent Curve/Curve Intersections

NOTES:
***********************************************************************/
SmCCTangentIntENFO::SmCCTangentIntENFO
  (const SmCurve & crCurve1,            // in : 1st Curve
   const SmCurve & crCurve2,            // in : 2nd Curve
   double          dSquaredTolerance)   // in : currently not used
 //double          dSign                // obsolete.
 : m_crCurve1(crCurve1),
   m_crCurve2(crCurve2),
   m_dSquaredTolerance(dSquaredTolerance)
 //m_dSign(dSign)
{

} // end SmCCTangentIntENFO::SmCCTangentIntENFO constructor

/*******************************************************************//**
PURPOSE: Called once for each NR iteration to find
            tangent curve/curve intersections.
    Given X values,   Evaluate Func         = Func(X) and
                      Jacobian Matrix[i][j] = dFi/dxj.

    where Jacobi = (partial Func[i]/partial X[j])

NOTES:
  See description of objective functions in class declaration, above.

***********************************************************************/
SmStatus SmCCTangentIntENFO::Evaluate
  (const SmTArray<double> & crX,           // in : x of Ax=F
   SmTArray<double>       & rF,            // out: F of Ax=F function values,
   SmMatrix               * pOptJacobian,  // out: Partial derivatives of the functions.
   SmBoolean              & rbFoundAnswer) // out: currently always set to FALSE
{
  // pass the call along
  return( StaticEvaluate(m_crCurve1,
                         m_crCurve2,
                         crX,
                         rF,
                         pOptJacobian,
                         rbFoundAnswer));

} // end SmCCTangentIntENFO::Evaluate

/*******************************************************************//**
PURPOSE: Static Evaluate so that other functions can share these
            clever objective functions as needed

NOTES:
***********************************************************************/
SmStatus SmCCTangentIntENFO::StaticEvaluate
  (const SmCurve          & crCurve1,      // in : 1st curve being intersected
   const SmCurve          & crCurve2,      // in : 2nd curve being intersected
   const SmTArray<double> & crX,           // in : x of Ax=F
   SmTArray<double>       & rF,            // out: F of Ax=F function values,
   SmMatrix               * pOptJacobian,  // out: Partial derivatives of the functions.
   SmBoolean              & rbFoundAnswer) // out: currently always set to FALSE
{
  // check state
  SM_ASSERT(crX.GetSize() == 2);
  SM_ASSERT(rF.GetSize()  == 2);

  if (pOptJacobian)
    {
      SM_ASSERT(pOptJacobian->GetNumRows()    == 2);
      SM_ASSERT(pOptJacobian->GetNumColumns() == 2);
    }


  // init output
  rbFoundAnswer = FALSE;

#ifdef SM_DEBUG_CODE
static constexpr SmBoolean  bDebugMe = FALSE;
  if (bDebugMe)
    {
      crCurve1.Dump() ;
      crCurve2.Dump() ;

      SmEdge *pEdge1 = (SmEdge *)crCurve1.GetEdge() ;
      SmEdge *pEdge2 = (SmEdge *)crCurve2.GetEdge() ;
      SmBrep *pBrep1 = pEdge1 ? pEdge1->GetBrep() : NULL ;
      SmBrep *pBrep2 = pEdge2 ? pEdge2->GetBrep() : NULL ;

      smgfx_Erase() ;
      smgfx_SetLook(1,2,  0, 0, 1) ; if(pBrep1) pBrep1->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(1,2,  0, 1, 0) ; if(pBrep2) pBrep2->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3,  0, 1, 1) ; crCurve1.Draw() ; sm_GraphicsLoop();
      smgfx_SetLook(2,3,  1, 1, 0) ; crCurve2.Draw() ; sm_GraphicsLoop();
      smgfx_SetLook(3,4,  0, 0, 0) ; if(pEdge1) pEdge1->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(3,4, .5,.5,.5) ; if(pEdge2) pEdge2->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,6,  0, 0, 1) ; crCurve1.DrawAt(crX[0],1); sm_GraphicsLoop();
      smgfx_SetLook(2,7,  0, 1, 0) ; crCurve2.DrawAt(crX[1],1); sm_GraphicsLoop();
      sm_GraphicsLoop();
    }
#endif // SM_DEBUG_CODE

  // See notes in this class' declaration for the objective functions.
  // Get Curve1Point, Curve2Point positions and tangents
  SmPoint3d sPVV1[3];
  SmPoint3d sPVV2[3];
  SER(crCurve1.Evaluate(crX[0],2,TRUE,sPVV1, TRUE));  // nonZeroTangent values
  SER(crCurve2.Evaluate(crX[1],2,TRUE,sPVV2, TRUE));  // nonZeroTangent values
  double sTanLenSq1 = sPVV1[1].LengthSquared();
  double sTanLenSq2 = sPVV2[1].LengthSquared();

  // if we have a zero length tangent then we must fail
  if(   sTanLenSq2 < SM_EFF_ZERO
     || sTanLenSq1 < SM_EFF_ZERO)
    { return SM_ERR; }

  SmVector3d sUFP = sPVV1[1];
  SER(sUFP.Unitize());
  SmVector3d sUGP = sPVV2[1];
  SER(sUGP.Unitize());
  SmVector3d sUFPP = sPVV1[1].UnitizedDerivative(sPVV1[2]);
  SmVector3d sUGPP = sPVV2[1].UnitizedDerivative(sPVV2[2]);
  SmVector3d sGapVec = sPVV1[0] - sPVV2[0];

  if (pOptJacobian)
    {
      (*pOptJacobian)[0][0] = sUFPP.Dot(sUGP);
      (*pOptJacobian)[0][1] = sUFP.Dot(sUGPP);
      (*pOptJacobian)[1][0] = 2.0 * sPVV1[1].Dot(sGapVec);
      (*pOptJacobian)[1][1] = - 2.0 * sPVV2[1].Dot(sGapVec);
    }

  // Compute function values

  double m_dSign = ( sUFP.Dot( sUGP ) > 0 ) ? 1.0 : -1.0;

  rF[0] = sUFP.Dot(sUGP) - m_dSign;
  rF[1] = sGapVec.Dot(sGapVec);

#ifdef SM_DEBUG_CODE

  if (bDebugMe)
    {
      if (pOptJacobian) pOptJacobian->Dump();
      smos_WriteBuffer(_T(" F --- \n"));
      rF.Dump();
    }
#endif // SM_DEBUG_CODE

  // all done
  return SM_SUCCESS;


  // Second algorithm: can give false convergence.
//
//  For two curves C(s) and D(t),
//
//  F(s,t) = (C' x D') dot ( ( C'' x D' ) + ( C' x D'' ) ) // modified cross prod
//  G(s,t) = (C - D) dot ( C' - D' )                       // modified difference
//
//  F() arose from a desire to zero the cross product of the tangent vectors.
//  However, the magnitude (squared, or whatever) of a vector is not a good
//  objective function because it just grazes zero, with a slope of zero at
//  solution, which destroys quadratic convergence right where you need it
//  most.  Instead, the cross product is dotted with some vector that is
//  generally in the same direction, but whose direction will not flip when the
//  cross product of tangents does; that dot product goes through zero nicely.
//
//  In practice, G(s,t) should actually use unit vectors, because differing
//  lengths of tangent vectors can overpower the desired difference.
//  Also, the unit tangents should be aligned in the same direction.
//  This adds a bit of complexity to the Jacobian, but not a big deal.
//
//  // Evaluate curves.  Need 3 derivs.
//  SmPoint3d sPVV1[4];
//  SmPoint3d sPVV2[4];
//
//  SER( crCurve1.Evaluate( crX[0], 3, TRUE, sPVV1 ));
//  SER( crCurve2.Evaluate( crX[1], 3, TRUE, sPVV2 ));
//
//  // See comments above.
//  SmVector3d sCross11 = sPVV1[1] * sPVV2[1];
//  SmVector3d sCross21 = sPVV1[2] * sPVV2[1];
//  SmVector3d sCross12 = sPVV1[1] * sPVV2[2];
//  SmVector3d sCross31 = sPVV1[3] * sPVV2[1];
//  SmVector3d sCross22 = sPVV1[2] * sPVV2[2];
//  SmVector3d sCross13 = sPVV1[1] * sPVV2[3];
//
//  SmVector3d sDiff00  = sPVV1[0] - sPVV2[0];
//  SmVector3d sDiff11  = sPVV1[1] - sPVV2[1];
//
//  SmVector3d sCtUnit = sPVV1[1];
//  double dMag = sCtUnit.Length();
//  if ( dMag > SM_EFF_ZERO )
//      sCtUnit /= dMag;
//
//  SmVector3d sDsUnit = sPVV2[1];
//  dMag = sDsUnit.Length();
//  if ( dMag > SM_EFF_ZERO )
//      sDsUnit /= dMag;
//
//  SmBoolean bFlip = ( sPVV1[1].Dot( sPVV2[1] ) ) < 0;
//
//  // Function values:
//  rF[0] = sCross11.Dot( sCross21 + sCross12 );
//  if ( bFlip )
//      rF[1] = sDiff00.Dot( sCtUnit + sDsUnit );
//  else
//      rF[1] = sDiff00.Dot( sCtUnit - sDsUnit );
//
//  // Derivatives:
//  if ( pOptJacobian )
//    {
//      (*pOptJacobian)[0][0] =   sCross21.Dot( sCross21 + sCross12 )
//                              + sCross11.Dot( sCross31 + sCross22 );
//      (*pOptJacobian)[0][1] =   sCross12.Dot( sCross21 + sCross12 )
//                              + sCross11.Dot( sCross22 + sCross13 );
//
//      SmVector3d sCtUnit_t = sPVV1[2] - sCtUnit * ( sCtUnit.Dot( sPVV1[2] ) );
//      SmVector3d sDsUnit_s = sPVV2[2] - sDsUnit * ( sDsUnit.Dot( sPVV2[2] ) );
//
//      if ( bFlip )
//      {
//          sDsUnit  *= -1;
//          sDsUnit_s *= -1;
//      }
//
//      (*pOptJacobian)[1][0] =   sPVV1[1].Dot( sCtUnit - sDsUnit )
//                              + sDiff00.Dot( sCtUnit_t );
//      (*pOptJacobian)[1][1] = - sPVV2[1].Dot( sCtUnit - sDsUnit )
//                              - sDiff00.Dot( sDsUnit_s );
//    }


  // Third algorithm: Also lost quadratic convergence.
// The idea here is this: just zero the gap, but since we need two functions,
// take dot products with two vectors.  This is essentially the same as the
// non-tangent case, where the two vectors are the two curves' tangent vectors.
// That has the added behavior of finding the closest approach, if the
// curves don't actually intersect.
// Here the two vectors are the common tangent direction, and the direction
// of the 2nd derivative of whichever curve has the larger 2nd derivative.
// That's just to get any basis -- any pair of non-tangent vectors.
// Note, if both curves' 2nd derivatives are zero, then we're trying to find
// the intersection of two parallel lines -- not much we can do here.
// (That does happen in practice; returning error doesn't hurt the algorithm.)
//
//  // Evaluate curves.  Need 3 derivs.
//  SmPoint3d sPVV1[4];
//  SmPoint3d sPVV2[4];
//
//  SER( crCurve1.Evaluate( crX[0], 3, TRUE, sPVV1 ));
//  SER( crCurve2.Evaluate( crX[1], 3, TRUE, sPVV2 ));
//
//  SmVector3d sGapVec( sPVV2[0] - sPVV1[0] );
//
//  // Get the unitized derivatives.
//  SmVector3d sC1_t_Unit, sC2_t_Unit;
//  ULONG lWhich2nd;
//
//  if ( sPVV1[2].LengthSquared() > sPVV2[2].LengthSquared() )
//  {
//      lWhich2nd = 1;
//      sC1_t_Unit = sPVV1[2];
//      SER( sC1_t_Unit.Unitize() );
//      sm_UnitizeTangent( sPVV2,  sGapVec, sC2_t_Unit );
//  }
//  else if ( sPVV2[2].LengthSquared() > SM_EFF_ZERO )
//  {
//      lWhich2nd = 2;
//      sm_UnitizeTangent( sPVV1, -sGapVec, sC1_t_Unit );
//      sC2_t_Unit = sPVV2[2];
//      SER( sC2_t_Unit.Unitize() );
//  }
//  else
//  {
//      // Intersecting two parallel lines: not going to happen.
//      return( SM_ERR ); // SER ?
//  }
//
//  // Function values:
//  rF[0] = sGapVec.Dot( sC1_t_Unit );
//  rF[1] = sGapVec.Dot( sC2_t_Unit );
//
//  // Derivatives:
//  if (pOptJacobian)
//    {
//      // Note: if we're not close enough to a solution, the second-order
//      // terms can dominate the first-order terms, and can change the
//      // direction.  The first-order terms always give the correct sign
//      // for the step, so the second-order term should not cause a sign
//      // change.  In fact, limiting the second-order term to half the
//      // size of the first term (if opposite size) works well.
//
//
//      // Deriv of the unitized C_t  ( C_t / || C_t || )
//      SmVector3d sC1_t_Unit_t, sC2_t_Unit_t;
//      if ( lWhich2nd == 1 )
//      {
//          sC1_t_Unit_t = sPVV1[2].UnitizedDerivative( sPVV1[3] );
//          sC2_t_Unit_t = sPVV2[1].UnitizedDerivative( sPVV2[2] );
//      }
//      else
//      {
//          sC1_t_Unit_t = sPVV1[1].UnitizedDerivative( sPVV1[2] );
//          sC2_t_Unit_t = sPVV2[2].UnitizedDerivative( sPVV2[3] );
//      }
//
//          double dTerm11 = -sPVV1[1].Dot( sC1_t_Unit   );
//          double dTerm12 =  sGapVec .Dot( sC1_t_Unit_t );
//          double dTerm21 =  sPVV2[1].Dot( sC2_t_Unit   );
//          double dTerm22 =  sGapVec .Dot( sC2_t_Unit_t );
//
//          if ( dTerm11*dTerm12 < 0 // note, dTerm11 might not be negative
//              && smos_Fabs( dTerm12 ) > smos_Fabs( dTerm11 )/2 ) {
//              dTerm12 = -dTerm11/2;
//          }
//          if ( dTerm21*dTerm22 < 0 // note, dTerm21 might not be positive
//              && smos_Fabs( dTerm22 < smos_Fabs( dTerm21 )/2 ) ) {
//              dTerm22 = -dTerm21/2;
//          }
//          (*pOptJacobian)[0][0] =  dTerm11 + dTerm12;
//          (*pOptJacobian)[0][1] =  sPVV2[1].Dot( sC1_t_Unit );
//          (*pOptJacobian)[1][0] = -sPVV1[1].Dot( sC2_t_Unit );
//          (*pOptJacobian)[1][1] =  dTerm21 + dTerm22;
//
//        } // end if pOptJacobian is not null.

} // end SmCCTangentIntENFO::StaticEvaluate
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 18425-18606: SmCPDistAtENFO (class, Evaluate)
class SmCPDistAtENFO : public SmEvalNFunctionsObject
{
protected:
    const SmCurve    & m_crCurve;              // target curve
    const SmPoint3d  & m_crPoint;              // TestPoint
    double             m_dTargetDistanceSq;    // Desired GapSizeSq
    double             m_dSquaredTolerance;    // currently not used

public:
    // constructor
    SmCPDistAtENFO
      (const SmCurve   & crCurve,
       const SmPoint3d & crPoint,
       double            dTargetDistanceSq,
       double            dSquaredTolerance)
     : m_crCurve          (crCurve),
       m_crPoint          (crPoint),
       m_dTargetDistanceSq(dTargetDistanceSq),
       m_dSquaredTolerance(dSquaredTolerance)
       { }

    // destructor
   virtual ~SmCPDistAtENFO() { }

    // evaluator
    virtual SmStatus Evaluate
      (const SmTArray<double> & crX,             // in : x of Ax=F
       SmTArray<double>       & rF,              // out: F of Ax=F function values,
       SmMatrix               * pOptJacobian,    // out: Partial derivatives of the functions.
       SmBoolean              & rbFoundAnswer);  // out: Always set to FALSE

} ; // end class SmCPDistAtENF

/*******************************************************************//**
PURPOSE: Called once for each NR iteration to find
            curve/curve intersections.
    Given X values,   Evaluate Func         = Func(X) and
                      Jacobian Matrix[i][j] = dFi/dxj.

    where Jacobi = (partial Func[i]/partial X[j])

NOTES:
  Given problem: |C - P| = d  where C(s) = curve
  Solve (C-P)*(C-P) - d*d = 0 with the following equations

  crX[0] =  s     curve param value
  crX[1] = Vec.x  Gap Vector X coordinate
  crX[2] = Vec.y  Gap Vector Y coordinate
  crX[3] = Vec.z  Gap Vector Z coordinate

  Func[0] = (C-P).x*(C-P).x - Vec.x*Vec.x
  Func[1] = (C-P).y*(C-P).y - Vec.y*Vec.y
  Func[2] = (C-P).z*(C-P).z - Vec.z*Vec.z
  Func[3] = Vec*Vec - d*d

  Where d = m_dTargetDistanceSq

  SumOfAllErrors = "

***********************************************************************/
SmStatus SmCPDistAtENFO::Evaluate
  (const SmTArray<double> & crX,           // in : x of Ax=F
   SmTArray<double>       & rF,            // out: F of Ax=F function values,
   SmMatrix               * pOptJacobian,  // out: Partial derivatives of the functions.
   SmBoolean              & rbFoundAnswer) // out: Always set To False
{
  // check state
  SM_ASSERT(crX.GetSize() == 4);
  SM_ASSERT(rF.GetSize() == 4);
  if (pOptJacobian) { SM_ASSERT(pOptJacobian->GetNumRows() == 4);
                      SM_ASSERT(pOptJacobian->GetNumColumns() == 4);
                    }

  // Given the original problem
  //     |f(s) - P| = d
  // Where f(s) is curve, P is point, and d is the distance at
  // Square each side
  //     (f(s) - P)^2 = d^2
  // Introduce three component values which SUM to the distance
  //     Sqrt(Cx^2 + Cy^2 + Cz^2) = d
  // Solve the following equations
  //    (X(s)-Xp)^2 - Cx^2 = 0
  //    (Y(s)-Yp)^2 - Cy^2 = 0
  //    (Z(s)-Zp)^2 - Cz^2 = 0
  //    Cx^2  + Cy^2 + Cz^2 - d^2 = 0
  //
  // Where X(s) is X component of curve f(s) evaluated at (s).
  // Where Xp is X component of point.
  // Where Cx is the X component of the distance

  // init output
  rbFoundAnswer = FALSE;

  // CurvePoint position and tangent
  SmPoint3d sPV[2];
  SER(m_crCurve.Evaluate(crX[0],1,TRUE,sPV, TRUE)); // nonZeroTangent values
  double sTanLenSq1 = sPV[1].LengthSquared();

  // if we have a zero length tangent then we must fail
  if (sTanLenSq1 < SM_EFF_ZERO)
    { return SM_ERR; }

  // gap = C(s) - TestPoint
  SmVector3d sGap = sPV[0] - m_crPoint;

#ifdef SM_DEBUG_CODE
SmBoolean  bDebugMe = FALSE;
  if (bDebugMe)
    {
      m_crCurve.Dump() ;

      SmEdge *pEdge1 = (SmEdge *)m_crCurve.GetEdge() ;
      SmBrep *pBrep1 = pEdge1 ? pEdge1->GetBrep() : NULL ;

      smgfx_Erase() ;
      smgfx_SetLook(1,2,  0,0,1) ; if(pBrep1) pBrep1->Draw(TRUE) ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3,  0,1,1) ; m_crCurve.Draw() ; sm_GraphicsLoop();
      smgfx_SetLook(3,4,  0,0,0) ; if(pEdge1) pEdge1->Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,6,  0,0,1) ; m_crCurve.DrawAt(crX[0],1); sm_GraphicsLoop();
      smgfx_SetLook(6,8,  0,1,0) ; m_crPoint.Draw() ; sm_GraphicsLoop() ;
      smgfx_SetLook(2,3,  1,0,0) ; sGap.Draw(&m_crPoint); sm_GraphicsLoop();
      sm_GraphicsLoop();
    }
#endif // SM_DEBUG_CODE

  // Set up Jacobian matrix for the following four functions
  // Note that Cx = Cx^2 from above function.
  // fun[0] = (X(s)-Xp)^2 - Cx
  // fun[1] = (Y(s)-Yp)^2 - Cy
  // fun[2] = (Z(s)-Zp)^2 - Cz
  // fun[3] = Cx + Cy + Cz - d^2
  //
  //  | 2X'(X-Xp)   -1      0      0  |
  //  | 2Y'(Y-Yp)    0     -1      0  |
  //  | 2Z'(Z-Zp)    0      0     -1  |
  //  |     0        1      1      1  |
  //
  //

  if (pOptJacobian)
    {
      (*pOptJacobian)[0][0] = 2.0 * sPV[1].x * (sPV[0].x - m_crPoint.x);
      (*pOptJacobian)[0][1] = -1.0;
      (*pOptJacobian)[0][2] = 0.0;
      (*pOptJacobian)[0][3] = 0.0;

      (*pOptJacobian)[1][0] = 2.0 * sPV[1].y * (sPV[0].y - m_crPoint.y);
      (*pOptJacobian)[1][1] = 0.0;
      (*pOptJacobian)[1][2] = -1.0;
      (*pOptJacobian)[1][3] = 0.0;

      (*pOptJacobian)[2][0] = 2.0 * sPV[1].z * (sPV[0].z - m_crPoint.z);
      (*pOptJacobian)[2][1] = 0.0;
      (*pOptJacobian)[2][2] = 0.0;
      (*pOptJacobian)[2][3] = -1.0;

      (*pOptJacobian)[3][0] = 0.0;
      (*pOptJacobian)[3][1] = 1.0;
      (*pOptJacobian)[3][2] = 1.0;
      (*pOptJacobian)[3][3] = 1.0;
    }

  // Compute function values
  SmVector3d sFunVec = sGap.Multiply(sGap);
  rF[0] = sFunVec.x - crX[1];
  rF[1] = sFunVec.y - crX[2];
  rF[2] = sFunVec.z - crX[3];
  rF[3] = crX[1] + crX[2] + crX[3] - m_dTargetDistanceSq;

#ifdef SM_DEBUG_CODE
  if (bDebugMe)
    {
      if (pOptJacobian) pOptJacobian->Dump();
      smos_WriteBuffer(_T(" F --- \n"));
      rF.Dump();
    }
#endif // SM_DEBUG_CODE

  // all done
  return SM_SUCCESS;

} // end SmCPDistAtENFO::Evaluate
// ---- end verbatim
// clang-format on
} // namespace sc_crv_p4

namespace
{
using namespace sc_crv_p4;

// Ops (keep in sync with crates/solidcore-ffi/src/units/curve_curve_p4.rs).
enum : int32_t
{
    OP_IS_CLOSED = 0,
    OP_IS_DEGENERATE = 1,
    OP_POLAR_BOX = 2,
    OP_SNAP = 3,
    OP_ASSERT = 4,
    OP_FIX_CHAIN = 5,
    OP_FILTER = 6,
    OP_PROJ_DIST = 7,
    OP_PROPERTY = 8,
    OP_CC_TANGENT = 9,
    OP_CP_DIST_AT = 10,
    OP_DIST_EXTREMA = 11,
    OP_ANALYTIC = 12,
    OP_PLANES = 13,
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
    if (kind == 3)
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double rx = rd.num();
        double ry = rd.num();
        bool io = rd.flag();
        return new (ctx) SmEllipse(c, x, y, SmExtent1d(d0, d1), rx, ry, dim, &ctx, NULL,
                                   io ? TRUE : FALSE);
    }
    SmVector3d p1 = rd.vec();
    SmVector3d d1 = rd.vec();
    SmVector3d p2 = rd.vec();
    SmVector3d d2 = rd.vec();
    return new (ctx) SmHermiteCurve(p1, d1, p2, d2, dim);
}

// An analytic operand: `0` line `P V d0 d1 scale io`, `1` ellipse `C X Y d0 d1 rx ry dim io`,
// `2` curve block.
SmCurve* read_operand(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 0)
    {
        SmPoint3d p = rd.vec();
        SmVector3d v = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double scale = rd.num();
        bool io = rd.flag();
        return new (ctx) SmLine(p, v, SmExtent1d(d0, d1), scale, 3, &ctx, NULL, io ? TRUE : FALSE);
    }
    if (kind == 1)
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double rx = rd.num();
        double ry = rd.num();
        ULONG dim = (ULONG)rd.idx();
        bool io = rd.flag();
        return new (ctx) SmEllipse(c, x, y, SmExtent1d(d0, d1), rx, ry, dim, &ctx, NULL,
                                   io ? TRUE : FALSE);
    }
    return read_curve(rd, ctx);
}

// The natural interval of `c` between the normalised parameters `f0`, `f1`.
SmExtent1d sub_ivl(Rd& rd, const SmCurve* c)
{
    double f0 = rd.num();
    double f1 = rd.num();
    SmExtent1d nat = c->GetNaturalInterval();
    return SmExtent1d(nat.Evaluate(f0), nat.Evaluate(f1));
}

void put_sols(std::vector<double>& o, SmSolutionArray& sols, const void* a, const void* b)
{
    o.push_back((double)sols.GetSize());
    for (ULONG i = 0; i < sols.GetSize(); i++)
    {
        const SmSolution& s = sols[i];
        auto id = [&](const void* p) { return p == NULL ? -1.0 : p == a ? 0.0 : p == b ? 1.0 : 2.0; };
        o.push_back((double)s.m_eSolutionType);
        o.push_back((double)s.m_lNumVariables);
        o.push_back((double)s.m_lNumObjects);
        o.push_back(id(s.m_apObjects[0]));
        o.push_back(s.m_lNumObjects > 1 ? id(s.m_apObjects[1]) : -1.0);
        o.push_back(s.m_vStart.m_dSolutionValue);
        o.push_back(s.m_vStart[0]);
        o.push_back(s.m_lNumVariables > 1 ? s.m_vStart[1] : 0.0);
        if (s.m_eSolutionType == SM_ST_RANGE_OF_VALUES)
        {
            o.push_back(s.m_vEnd.m_dSolutionValue);
            o.push_back(s.m_vEnd[0]);
            o.push_back(s.m_lNumVariables > 1 ? s.m_vEnd[1] : 0.0);
        }
    }
}

void run(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_IS_CLOSED:
    {
        SmCurve* c = read_curve(rd, ctx);
        bool base = rd.flag();
        double a = rd.num();
        double b = rd.num();
        double tol = rd.num();
        SmExtent1d ivl(a, b);
        SmBoolean r = base ? c->SmCurve::IsClosed(ivl, tol) : c->IsClosed(ivl, tol);
        o.push_back(0.0);
        o.push_back(flg(r));
        delete c;
        break;
    }
    case OP_IS_DEGENERATE:
    {
        SmCurve* c = read_curve(rd, ctx);
        bool base = rd.flag();
        double tol = rd.num();
        bool has = rd.flag();
        double a = rd.num();
        double b = rd.num();
        SmExtent1d ivl(a, b);
        const SmExtent1d* p = has ? &ivl : NULL;
        SmBoolean r = base ? c->SmCurve::IsDegenerate(tol, p) : c->IsDegenerate(tol, p);
        o.push_back(0.0);
        o.push_back(flg(r));
        delete c;
        break;
    }
    case OP_POLAR_BOX:
    {
        SmCurve* c = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        ULONG n = (ULONG)rd.idx();
        SmPolarBox box;
        SmStatus s = c->ApproximatePolarBox(SmExtent1d(a, b), box, n);
        o.push_back(st(s));
        o.push_back((double)box.GetPolarBoxState());
        for (ULONG i = 0; i < 3; i++)
            put_vec(o, *box.GetBasis(i));
        SmExtent2d d = box.GetDomain();
        o.push_back(d.GetMin().x);
        o.push_back(d.GetMin().y);
        o.push_back(d.GetMax().x);
        o.push_back(d.GetMax().y);
        delete c;
        break;
    }
    case OP_SNAP:
    {
        SmCurve* c = read_curve(rd, ctx);
        double t = rd.num();
        double tol = rd.num();
        double r = 0.0;
        SmStatus s = c->SnapToKnots(t, tol, r);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back(r);
        delete c;
        break;
    }
    case OP_ASSERT:
    {
        SmCurve* c = read_curve(rd, ctx);
        int64_t owner_kind = rd.idx();
        bool given = rd.flag();
        SmCurve* owner = NULL;
        if (owner_kind == 1)
        {
            owner = new (ctx) SmHermiteCurve(SmVector3d(0, 0, 0), SmVector3d(1, 0, 0),
                                             SmVector3d(1, 0, 0), SmVector3d(1, 0, 0), 3);
            c->SetOwner(owner);
        }
        SmAssertArray list;
        SmBoolean ok = c->SmCurve::AssertValid(&list, given ? SM_LEVEL_GIVEN : SM_LEVEL_0,
                                               SM_NO_WALK, NULL);
        o.push_back(0.0);
        o.push_back(flg(ok));
        o.push_back((double)list.GetSize());
        for (ULONG i = 0; i < list.GetSize(); i++)
        {
            const SmAssertReport* r = list[i];
            o.push_back((double)r->GetTestIndex());
            o.push_back((double)r->GetOwnerType());
            o.push_back((double)r->GetTol3d());
            o.push_back(r->GetValue());
            o.push_back((double)r->GetListIndex());
        }
        if (owner != NULL)
            c->SetOwner(NULL);
        delete c;
        delete owner;
        break;
    }
    case OP_FIX_CHAIN:
    {
        int64_t n = rd.idx();
        SmTArray<SmCurve*> curves;
        std::map<const SmCurve*, int64_t> index;
        for (int64_t k = 0; k < n; k++)
        {
            SmCurve* c = rd.flag() ? read_curve(rd, ctx) : NULL;
            if (c != NULL)
                index[c] = k;
            curves.Add(c);
        }
        double tol = rd.num();
        bool del = rd.flag();
        SmTArray<ULONG> starts;
        ULONG nchain = SmCurve::FixChainCurvesList(curves, starts, tol, del ? TRUE : FALSE);
        o.push_back(0.0);
        o.push_back((double)nchain);
        o.push_back((double)starts.GetSize());
        for (ULONG i = 0; i < starts.GetSize(); i++)
            o.push_back((double)starts[i]);
        o.push_back((double)curves.GetSize());
        for (ULONG i = 0; i < curves.GetSize(); i++)
        {
            SmCurve* c = curves[i];
            if (c == NULL)
            {
                o.push_back(-1.0);
                continue;
            }
            o.push_back((double)index[c]);
            SmExtent1d nat = c->GetNaturalInterval();
            o.push_back(nat.GetMin());
            o.push_back(nat.GetMax());
            SmPoint3d p0, p1;
            c->EvaluatePoint(nat.GetMin(), p0);
            c->EvaluatePoint(nat.GetMax(), p1);
            put_vec(o, p0);
            put_vec(o, p1);
        }
        for (ULONG i = 0; i < curves.GetSize(); i++)
            delete curves[i];
        break;
    }
    case OP_FILTER:
    {
        SmCurve* c1 = read_curve(rd, ctx);
        double a1 = rd.num();
        double b1 = rd.num();
        SmCurve* c2 = read_curve(rd, ctx);
        double a2 = rd.num();
        double b2 = rd.num();
        SmContinuityType k1[2], k2[2];
        k1[0] = (SmContinuityType)rd.idx();
        k1[1] = (SmContinuityType)rd.idx();
        k2[0] = (SmContinuityType)rd.idx();
        k2[1] = (SmContinuityType)rd.idx();
        ULONG adj = (ULONG)rd.idx();
        SmSolution sol;
        sol.m_vStart[0] = rd.num();
        sol.m_vStart[1] = rd.num();
        sol.m_vStart.m_dSolutionValue = rd.num();
        SmBoolean keep = FALSE;
        SmStatus s = sm_FilterSolution(*c1, SmExtent1d(a1, b1), *c2, SmExtent1d(a2, b2), k1, k2,
                                       adj, sol, keep);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back(flg(keep));
        delete c1;
        delete c2;
        break;
    }
    case OP_PROJ_DIST:
    {
        SmCurve* c = read_curve(rd, ctx);
        double t = rd.num();
        SmPoint3d p = rd.vec();
        SmVector3d nrm = rd.vec();
        bool rev = rd.flag();
        SmVector3d dir = rd.vec();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        double r = sm_GetProjDist(*c, t, p, nrm, rev ? TRUE : FALSE, dir, sop);
        o.push_back(0.0);
        o.push_back(r);
        delete c;
        break;
    }
    case OP_PROPERTY:
    {
        SmCurve* c = read_curve(rd, ctx);
        SmCurvePropertyType prop = (SmCurvePropertyType)rd.idx();
        bool has_val = rd.flag();
        double val = rd.num();
        int64_t nv = rd.idx();
        SmVector3d vecs[3];
        for (int64_t i = 0; i < nv; i++)
            vecs[i] = rd.vec();
        double sqtol = rd.num();
        double t = rd.num();
        bool signal = rd.flag();
        SmCurvePropertyEFO efo(*c, prop, has_val ? &val : NULL, nv > 0 ? vecs : NULL, sqtol);
        double f = 0.0, fp = 0.0;
        SmBoolean found = FALSE;
        SmStatus s = efo.Evaluate(t, f, fp, found, signal ? TRUE : FALSE);
        o.push_back(st(s));
        o.push_back(f);
        o.push_back(fp);
        o.push_back(flg(found));
        delete c;
        break;
    }
    case OP_CC_TANGENT:
    {
        SmCurve* c1 = read_curve(rd, ctx);
        SmCurve* c2 = read_curve(rd, ctx);
        SmTArray<double> x, f;
        x.Add(rd.num());
        x.Add(rd.num());
        f.Add(0.0);
        f.Add(0.0);
        bool jac = rd.flag();
        SmMatrix m(2, 2);
        for (ULONG i = 0; i < 2; i++)
            for (ULONG j = 0; j < 2; j++)
                m[i][j] = 0.0;
        SmBoolean found = FALSE;
        SmStatus s = SmCCTangentIntENFO::StaticEvaluate(*c1, *c2, x, f, jac ? &m : NULL, found);
        o.push_back(st(s));
        o.push_back(f[0]);
        o.push_back(f[1]);
        for (ULONG i = 0; i < 2; i++)
            for (ULONG j = 0; j < 2; j++)
                o.push_back(m[i][j]);
        o.push_back(flg(found));
        delete c1;
        delete c2;
        break;
    }
    case OP_CP_DIST_AT:
    {
        SmCurve* c = read_curve(rd, ctx);
        SmPoint3d p = rd.vec();
        double dsq = rd.num();
        double sqtol = rd.num();
        SmTArray<double> x, f;
        for (int i = 0; i < 4; i++)
        {
            x.Add(rd.num());
            f.Add(0.0);
        }
        bool jac = rd.flag();
        SmMatrix m(4, 4);
        for (ULONG i = 0; i < 4; i++)
            for (ULONG j = 0; j < 4; j++)
                m[i][j] = 0.0;
        SmCPDistAtENFO enfo(*c, p, dsq, sqtol);
        SmBoolean found = FALSE;
        SmStatus s = enfo.Evaluate(x, f, jac ? &m : NULL, found);
        o.push_back(st(s));
        for (ULONG i = 0; i < 4; i++)
            o.push_back(f[i]);
        for (ULONG i = 0; i < 4; i++)
            for (ULONG j = 0; j < 4; j++)
                o.push_back(m[i][j]);
        o.push_back(flg(found));
        delete c;
        break;
    }
    case OP_DIST_EXTREMA:
    {
        SmCurve* c1 = read_curve(rd, ctx);
        SmCurve* c2 = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        double tol = rd.num();
        double t = rd.num();
        SmExtent1d dom(a, b);
        SmFindCurveDistanceExtremaEFO efo(c1, c2, dom, tol);
        double f = 0.0, fp = 0.0;
        SmBoolean found = FALSE;
        SmStatus s = efo.Evaluate(t, f, fp, found, TRUE);
        o.push_back(st(s));
        o.push_back(f);
        o.push_back(fp);
        o.push_back(flg(found));
        delete c1;
        delete c2;
        break;
    }
    case OP_ANALYTIC:
    {
        SmCurve* a = read_operand(rd, ctx);
        SmExtent1d ia = sub_ivl(rd, a);
        SmCurve* b = read_operand(rd, ctx);
        SmExtent1d ib = sub_ivl(rd, b);
        double tol = rd.num();
        SmBoolean more = FALSE;
        SmSolutionArray sols;
        SmStatus s = a->AnalyticCurveIntersect(ia, *b, ib, tol, more, sols);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(more));
            put_sols(o, sols, a, b);
        }
        delete a;
        delete b;
        break;
    }
    case OP_PLANES:
    {
        SmCurve* c = read_curve(rd, ctx);
        int64_t np = rd.idx();
        SmTArray<SmPlane*> planes;
        for (int64_t i = 0; i < np; i++)
        {
            SmPoint3d p = rd.vec();
            SmVector3d nrm = rd.vec();
            planes.Add(new (ctx) SmPlane(p, nrm, &ctx));
        }
        SmBoolean r = c->HasPlaneCrossings(planes);
        o.push_back(0.0);
        o.push_back(flg(r));
        for (ULONG i = 0; i < planes.GetSize(); i++)
            delete planes[i];
        delete c;
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_curve_curve_p4_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                         int64_t cap, int64_t* out_len)
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
