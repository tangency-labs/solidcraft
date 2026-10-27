// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surface_p3`: the members of SmSurface.cpp
// (source/SMLib/src/SmSurface.cpp, source/SMLib/inc/SmSurface.h) owned by the unit --
// SmSurface::EvaluateGeometric / EvaluateNormalSection / GetSingularities / CoverCoincidentSurface /
// LocalPlaneSurfaceIntersect / LocalSurfaceSolve / LocalSurfaceSurfaceIntersect (public members, called
// directly), smsurf_CheckShapeMatch (exported, called directly) and the Newton equation sets
// SmFindPSNormalENFO and SmSSSIntersectENFO (file-local classes).
//
// File-local classes of SmSurface.cpp cannot be linked against, so those exercised here are VERBATIM
// copies of upstream SmSurface.cpp (commit c9c5979), inside the namespace sc_srf_p3 (line ranges noted at
// each block).
//
// One entry point, sc_surface_surface_p3_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/surface_surface_p3.rs, which validates every
// input. A surface block is `0 oob form n m p q r s cpts(x y z w) u v` (an
// SmBSplineSurface(gw_SURFACE*, FALSE, NULL)), `1 O[3] N[3]` (SmPlane(O, N)) or `2 basis implicit`
// (an SmCurveBoundedSurface without boundary curves over a basis block). `out` receives
// the op status (0 = SM_SUCCESS, else the SmStatus value) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveBoundedSurface.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmGeomUtility.h>
#include <SmLocalSolveNd.h>
#include <SmMatrix.h>
#include <SmNurbsSrf.h>
#include <SmLine.h>
#include <SmPlane.h>
#include <SmSurfOfRevolution.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmSurfTypes.h>
#include <SmTArray.h>
#include <SmTol.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

#include "sc_sm.h"

using sc::put_vec;

namespace sc_srf_p3
{
// clang-format off

// ---- verbatim: SmSurface.cpp lines 25812-25841: class SmFindPSNormalENFO
class SmFindPSNormalENFO : public SmEvalNFunctionsObject
{
protected:
  const SmPoint3d & m_crPoint;                // Target Point
  const SmSurface & m_crSurface;              // Surface being searched
  double m_dSquaredTolerance;                 // convergence criterial tolerance
                                              // converges when GuessPoint/m_crPoint < Tolerance
                                              //           or   Gap/Tangent angle ~= cos(Gap/Tangent angle) * gapSize < tolerance
  SmSolverOperationType m_eSolverOperation;   // NOT USED
public:
  // constructor, destructor
  SmFindPSNormalENFO(const SmPoint3d     & crPoint,
                     const SmSurface     & crSurface,
                     double                dSquaredTolerance,
                     SmSolverOperationType eSolverOperation)
                   : m_crPoint(crPoint),
                     m_crSurface(crSurface),
                     m_dSquaredTolerance(dSquaredTolerance),
                     m_eSolverOperation(eSolverOperation)
                     { }
  virtual ~SmFindPSNormalENFO() { }

  // NewtonRaphson Iteration callback evaluate function
  virtual SmStatus Evaluate(const SmTArray<double> & crX,             // in : x of Ax=F
                            SmTArray<double>       & rF,              // out: F of Ax=F function values,
                            SmMatrix               * pOptJacobian,    // out: Partial derivatives of the functions.
                            SmBoolean              & rbFoundAnswer);  // out: Not always used, when used
                                                                      //      TRUE = converged (F members are within tolerance of 0.0
                                                                      //      FALSE= Not Used or Not Converged
} ; // end class SmFindPSNormalENFO

// ---- verbatim: SmSurface.cpp lines 25848-26037: SmFindPSNormalENFO::Evaluate
SmStatus SmFindPSNormalENFO::Evaluate
  (const SmTArray<double> & crX,           // in : x of Ax=F
   SmTArray<double>       & rF,            // out: F of Ax=F function values,
   SmMatrix               * pOptJacobian,  // out: Partial derivatives of the functions.
   SmBoolean              & rbFoundAnswer) // out: TRUE = Current GuessPoint/m_crPoint Gap less than tolerance
                                           //             or cos(gap/Tangent angle) * gapSize less than tolerance
                                           //      FALSE= Current gap larger than tolerance
{
  // check input
  SM_ASSERT(crX.GetSize() == 2);
  SM_ASSERT(rF.GetSize()  == 2);
  if (pOptJacobian)
    {
      SM_ASSERT(pOptJacobian->GetNumRows()    == 2);
      SM_ASSERT(pOptJacobian->GetNumColumns() == 2);
    }

  // inout output
  rbFoundAnswer = FALSE;

  // locals
  SmPoint3d  sPoint;
  SmVector3d sSu, sSv, sSuu, sSuv, sSvv; // ordinary surface derivatives
  SmVector3d sSuN, sSvN;                 // Normalized 1st derivs: sSu/dMU, sSv/dMV
  double dMU, dMV;                       // magnitudes of sSu, sSv

  double dFu_1, dFu_2, dFu, dFv;
  double dGv_1, dGv_2, dGv, dGu;

  SmVector3d sTempVec1, sTempVec2;

  SmPoint2d  sUV(crX[0],crX[1]);

  // get Surface evaluations
  SER(m_crSurface.Evaluate2ndDerivatives(sUV,TRUE,TRUE,sPoint,sSu,sSv,sSuv,sSuu,sSvv));

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = FALSE;
  // draw Current GuessPoint on Surface(blue)
  if (bDebugMe)
    {
      smgfx_SetLook(2,6, 0,0,1); m_crSurface.DrawAt(sUV,1); sm_GraphicsLoop();
      sm_GraphicsLoop();
    }
#endif
//cbi: just fail??? look into this.  This just means we're at a pole (probably).
  // failure - zero length surface tangents
  dMU = sSu.Length();
  dMV = sSv.Length();
  if ( dMU < SM_EFF_ZERO || dMV < SM_EFF_ZERO )
    {
      return SM_ERR;
    }

  // get gap vector
  SmVector3d sGap ( sPoint - m_crPoint );

#ifdef SM_DEBUG_CODE
  // draw gap vector(red)
  if (bDebugMe)
    {
      smgfx_SetLook(1,2, 0,1,1); sGap.Draw(&m_crPoint); sm_GraphicsLoop();
      sm_GraphicsLoop();
    }
#endif

  // A surface is normalized relative to a point when
  // the tangents of the iso parametric curve curves are
  // perpendicular to the vector from the point on the surface
  // to the normalizing point.
  // fun[0] = du*(s-p)
  // fun[1] = dv*(s-p)
  //
  //  |duu*(s-p) + du*du       duv*(s-p) + du*dv  |
  //  |duv*(s-p) + du*dv       dvv*(s-p) + dv*dv  |
  //
  // where s = s(u,v) a point on the surface
  //       p = fixed point in space
  //       du = d(s(u,v))/du
  //       dv = d(s(u,v))/dv
  //
  // note: chain rule for d(du*(s-p))/du = d(du)/du*(s-p) + du*d(s-p)/du = duu*(s-p) + du*du
  sSuN = sSu / dMU;
  sSvN = sSv / dMV;

  // Compute function values.
  rF[0] = sGap.Dot( sSuN );
  rF[1] = sGap.Dot( sSvN );

  // Compute Jacobian (derivs of function values: Fu, Gv, Fv, Gu).
  if (pOptJacobian)
    {
      // Note: if we're not close enough to a solution, the second-order
      // terms can dominate the first-order terms, and can change the
      // direction.  The first-order terms always give the correct sign
      // for the step, so the second-order term should not cause a sign
      // change.  In fact, limiting the second-order term to half the
      // size of the first term (if opposite sign) works well.
      // Note, the first order terms are always positive.
      const double d2ndDerivTermLimit = 0.5;

      // Calculate all of the normalized derivatives.
      SmVector3d sSuNu = sSu.UnitizedDerivative( sSuu );
      SmVector3d sSuNv = sSu.UnitizedDerivative( sSuv );
      SmVector3d sSvNu = sSv.UnitizedDerivative( sSuv );
      SmVector3d sSvNv = sSv.UnitizedDerivative( sSvv );

      // dFu:
      dFu_1 = sSu.Dot( sSuN );
      dFu_2 = sGap.Dot( sSuNu );
      if ( dFu_2 < -dFu_1 * d2ndDerivTermLimit )
        {  dFu_2 = -dFu_1 * d2ndDerivTermLimit; }
      dFu = dFu_1 + dFu_2;

      // dGv:
      dGv_1 = sSv. Dot( sSvN  );
      dGv_2 = sGap.Dot( sSvNv );
      if ( dGv_2 < -dGv_1 * d2ndDerivTermLimit )
        {  dGv_2 = -dGv_1 * d2ndDerivTermLimit; }
      dGv = dGv_1 + dGv_2;

      // and the other two:
      dFv = sSv.Dot( sSuN )  +  sGap.Dot( sSuNv );
      dGu = sSu.Dot( sSvN )  +  sGap.Dot( sSvNu );

      (*pOptJacobian)[0][0] = dFu;
      (*pOptJacobian)[0][1] = dFv;
      (*pOptJacobian)[1][0] = dGu;
      (*pOptJacobian)[1][1] = dGv;
    }

  // Note: the following test is not necessary, and in practice,
  // things work better without it.  Without it, the caller will
  // generally do one more iteration, which typically gives 2 or 3
  // more decimal digits of accuracy.
//  // convergence criteria
//  double dsPointMax  = sPoint.GetMaxDimension() ;
//  double dcrPointMax = m_crPoint.GetMaxDimension() ;
//  double dScale      = 1.0 + smos_Max(dsPointMax, dcrPointMax) ;
//  double dGapLengthSquared = sGap.LengthSquared() ;
//  if ( dGapLengthSquared < m_dSquaredTolerance * dScale * dScale )
//    {
//      rbFoundAnswer = TRUE;
//      return SM_SUCCESS;
//    }

  // check convergence - angle ~= cos(angle) * gapSize < Tolerance
  double dProjDistU = smos_Fabs( rF[0] / dMU );
  double dProjDistV = smos_Fabs( rF[1] / dMV );

  double dTol = smos_Sqrt( m_dSquaredTolerance );
  if ( dProjDistU < dTol && dProjDistV < dTol )
    {
      rbFoundAnswer = TRUE;
      return SM_SUCCESS;
    }

#ifdef SM_DEBUG_CODE
  if (bDebugMe)
    {
      if (pOptJacobian) pOptJacobian->Dump();
      smos_WriteBuffer(_T(" X --- \n")); crX.Dump();
      smos_WriteBuffer(_T(" F --- \n")); rF.Dump();
      //      m_crSurface.Dump() ;

      // compute and display likely next point
      if(pOptJacobian)
        {
          // copy outputs
          SmTArray<double> sF(rF) ;
          SmMatrix sJacobian(*pOptJacobian) ;

          // Solve for next Newton/Raphson parameter Steps
          SmTArray<double> sDeltas(2,NULL,2) ;
          for(ULONG ii=0; ii<2; ii++) { sF[ii] = -sF[ii] ; }
          if(SM_SUCCESS == sJacobian.SolveLinearSystem(sF,sDeltas))
            {
              // increment parameters and add NextPoint Display to Graphic
              SmPoint2d sUVNext(sUV.x + sDeltas[0],
                                sUV.y + sDeltas[1]) ;
              smgfx_SetLook(2,6, 0,0,1); m_crSurface.DrawAt(sUVNext,1); sm_GraphicsLoop();
              sm_GraphicsLoop();
            }
        } // end pOptJacobian existence check
    }
#endif // SM_DEBUG_CODE

  return SM_SUCCESS;

} // end SmFindPSNormalENFO::Evaluate

// ---- verbatim: SmSurface.cpp lines 28607-28642: class SmSSSIntersectENFO
class SmSSSIntersectENFO : public SmEvalNFunctionsObject
{
protected:
    const SmSurface & m_crSurface1;        // 1st surface of surf/surf/surf intersection
    const SmSurface & m_crSurface2;        // 2nd surface of surf/surf/surf intersection
    const SmSurface & m_crSurface3;        // 3rd surface of surf/surf/surf intersection
    double            m_dSquaredTolerance; // currently not used in this class
    ULONG             m_lCount;            // number of evaluation calls

public:
    SmPoint3d  m_sSrfPoint1;     // solution point on surface1
    SmPoint3d  m_sSrfPoint2;     // solution point on surface2
    SmPoint3d  m_sSrfPoint3;     // solution point on surface3
    SmVector3d m_sDU1, m_sDV1;   // solution point DU and DV Tangent vectors on surface1
    SmVector3d m_sDU2, m_sDV2;   // solution point DU and DV Tangent vectors on surface1
    SmVector3d m_sDU3, m_sDV3;   // solution point DU and DV Tangent vectors on surface1

public:
    SmSSSIntersectENFO(const SmSurface & crSurface1,
                       const SmSurface & crSurface2,
                       const SmSurface & crSurface3,
                       double dSquaredTolerance)
      : m_crSurface1(crSurface1),
        m_crSurface2(crSurface2),
        m_crSurface3(crSurface3),
        m_dSquaredTolerance(dSquaredTolerance),
        m_lCount(0) {}
    virtual ~SmSSSIntersectENFO() {}

    virtual SmStatus Evaluate(const SmTArray<double> & crX,             // in : x of Ax=F
                              SmTArray<double>       & rF,              // out: F of Ax=F function values,
                              SmMatrix               * pOptJacobian,    // out: Partial derivatives of the functions.
                              SmBoolean              & rbFoundAnswer);  // out: Not always used, when used
                                                                        //      TRUE = converged (F members are within tolerance of 0.0
                                                                        //      FALSE= Not Used or Not Converged
} ; // end class SmSSSIntersectENFO

// ---- verbatim: SmSurface.cpp lines 28659-28805: SmSSSIntersectENFO::Evaluate
SmStatus SmSSSIntersectENFO::Evaluate
  (const SmTArray<double> & crX,      // in : x of Ax=F, x=[u1 v1 u2 v2 u3 v3]
   SmTArray<double> & rF,             // out: function values, dist between surfPoint coordinates
   SmMatrix * pOptJacobian,           // out: Partial derivatives of the functions.
   SmBoolean & rbFoundAnswer)         // out: Not used - always set to FALSE
{
    // check inputs
    SM_ASSERT(crX.GetSize() == 6);
    SM_ASSERT(rF.GetSize()  == 6);
    if (pOptJacobian)
      {
        SM_ASSERT(pOptJacobian->GetNumRows()    == 6);
        SM_ASSERT(pOptJacobian->GetNumColumns() == 6);
      }

    m_lCount++;
    rbFoundAnswer = FALSE;

    // evaluate 1st surface positions and tangents
    SmPoint2d sUV1(crX[0],crX[1]);
    SER(m_crSurface1.Evaluate1stDerivatives(sUV1,TRUE,TRUE,m_sSrfPoint1,m_sDU1,m_sDV1));

    // evaluate 2nd surface positions and tangents
    SmPoint2d sUV2(crX[2],crX[3]);
    SER(m_crSurface2.Evaluate1stDerivatives(sUV2,TRUE,TRUE,m_sSrfPoint2,m_sDU2,m_sDV2));

    // evaluate 3rd surface positions and tangents
    SmPoint2d sUV3(crX[4],crX[5]);
    SER(m_crSurface3.Evaluate1stDerivatives(sUV3,TRUE,TRUE,m_sSrfPoint3,m_sDU3,m_sDV3));

    // if we have a zero length tangent then we must fail
    double sDU1LenSq = m_sDU1.LengthSquared();
    double sDV1LenSq = m_sDV1.LengthSquared();
    double sDU2LenSq = m_sDU2.LengthSquared();
    double sDV2LenSq = m_sDV2.LengthSquared();
    double sDU3LenSq = m_sDU3.LengthSquared();
    double sDV3LenSq = m_sDV3.LengthSquared();

    if (   sDU1LenSq < SM_EFF_ZERO_SQ
        || sDV1LenSq < SM_EFF_ZERO_SQ
        || sDU2LenSq < SM_EFF_ZERO_SQ
        || sDV2LenSq < SM_EFF_ZERO_SQ
        || sDU3LenSq < SM_EFF_ZERO_SQ
        || sDV3LenSq < SM_EFF_ZERO_SQ)
      {
        return SM_ERR;
      }

    // force near zero tangent components to equal zero
    // to prevent matrix inversions of non-invertable matrices
    if(fabs(m_sDU1.x) < SM_EFF_ZERO) m_sDU1.x = 0.0 ;
    if(fabs(m_sDU1.y) < SM_EFF_ZERO) m_sDU1.y = 0.0 ;
    if(fabs(m_sDU1.z) < SM_EFF_ZERO) m_sDU1.z = 0.0 ;
    if(fabs(m_sDV1.x) < SM_EFF_ZERO) m_sDV1.x = 0.0 ;
    if(fabs(m_sDV1.y) < SM_EFF_ZERO) m_sDV1.y = 0.0 ;
    if(fabs(m_sDV1.z) < SM_EFF_ZERO) m_sDV1.z = 0.0 ;

    if(fabs(m_sDU2.x) < SM_EFF_ZERO) m_sDU2.x = 0.0 ;
    if(fabs(m_sDU2.y) < SM_EFF_ZERO) m_sDU2.y = 0.0 ;
    if(fabs(m_sDU2.z) < SM_EFF_ZERO) m_sDU2.z = 0.0 ;
    if(fabs(m_sDV2.x) < SM_EFF_ZERO) m_sDV2.x = 0.0 ;
    if(fabs(m_sDV2.y) < SM_EFF_ZERO) m_sDV2.y = 0.0 ;
    if(fabs(m_sDV2.z) < SM_EFF_ZERO) m_sDV2.z = 0.0 ;

    if(fabs(m_sDU3.x) < SM_EFF_ZERO) m_sDU3.x = 0.0 ;
    if(fabs(m_sDU3.y) < SM_EFF_ZERO) m_sDU3.y = 0.0 ;
    if(fabs(m_sDU3.z) < SM_EFF_ZERO) m_sDU3.z = 0.0 ;
    if(fabs(m_sDV3.x) < SM_EFF_ZERO) m_sDV3.x = 0.0 ;
    if(fabs(m_sDV3.y) < SM_EFF_ZERO) m_sDV3.y = 0.0 ;
    if(fabs(m_sDV3.z) < SM_EFF_ZERO) m_sDV3.z = 0.0 ;

    // Intersection simply tries to minimize the following six functions
    // fun[0] = SX1(u1,v1) - SX2(u2,v2)
    // fun[1] = SY1(u1,v1) - SY2(u2,v2)
    // fun[2] = SZ1(u1,v1) - SZ2(u2,v2);
    // fun[3] = SX1(u1,v1) - SX3(u3,v3)
    // fun[4] = SY1(u1,v1) - SY3(u3,v3)
    // fun[5] = SZ1(u1,v1) - SZ3(u3,v3);

    SmVector3d sVecS1MinusS2 = m_sSrfPoint1 - m_sSrfPoint2 ;
    SmVector3d sVecS1MinusS3 = m_sSrfPoint1 - m_sSrfPoint3 ;
    SmVector3d sVecS2MinusS3 = m_sSrfPoint2 - m_sSrfPoint3 ;

    // It produces the following Jacobian
    //  | dSX1/du1   dSX1/dv1  -dSX2/du2  -dSX2/dv2   0          0         |
    //  | dSY1/du1   dSY1/dv1  -dSY2/du2  -dSY2/dv2   0          0         |
    //  | dSZ1/du1   dSZ1/dv1  -dSZ2/du2  -dSZ2/dv2   0          0         |
    //  | dSX1/du1   dSX1/dv1  0          0           -dSX3/du3  -dSX3/dv3 |
    //  | dSY1/du1   dSY1/dv1  0          0           -dSY3/du3  -dSY3/dv3 |
    //  | dSZ1/du1   dSZ1/dv1  0          0           -dSZ3/du3  -dSZ3/dv3 |

    if (pOptJacobian) {
        (*pOptJacobian)[0][0] =  m_sDU1.x ;
        (*pOptJacobian)[0][1] =  m_sDV1.x ;
        (*pOptJacobian)[0][2] = -m_sDU2.x ;
        (*pOptJacobian)[0][3] = -m_sDV2.x ;
        (*pOptJacobian)[0][4] =  0.0 ;
        (*pOptJacobian)[0][5] =  0.0 ;

        (*pOptJacobian)[1][0] =  m_sDU1.y ;
        (*pOptJacobian)[1][1] =  m_sDV1.y ;
        (*pOptJacobian)[1][2] = -m_sDU2.y ;
        (*pOptJacobian)[1][3] = -m_sDV2.y ;
        (*pOptJacobian)[1][4] =  0.0 ;
        (*pOptJacobian)[1][5] =  0.0 ;

        (*pOptJacobian)[2][0] =  m_sDU1.z ;
        (*pOptJacobian)[2][1] =  m_sDV1.z ;
        (*pOptJacobian)[2][2] = -m_sDU2.z ;
        (*pOptJacobian)[2][3] = -m_sDV2.z ;
        (*pOptJacobian)[2][4] =  0.0 ;
        (*pOptJacobian)[2][5] =  0.0 ;

        (*pOptJacobian)[3][0] =  m_sDU1.x ;
        (*pOptJacobian)[3][1] =  m_sDV1.x ;
        (*pOptJacobian)[3][2] =  0.0 ;
        (*pOptJacobian)[3][3] =  0.0 ;
        (*pOptJacobian)[3][4] = -m_sDU3.x ;
        (*pOptJacobian)[3][5] = -m_sDV3.x ;

        (*pOptJacobian)[4][0] =  m_sDU1.y ;
        (*pOptJacobian)[4][1] =  m_sDV1.y ;
        (*pOptJacobian)[4][2] =  0.0 ;
        (*pOptJacobian)[4][3] =  0.0 ;
        (*pOptJacobian)[4][4] = -m_sDU3.y ;
        (*pOptJacobian)[4][5] = -m_sDV3.y ;

        (*pOptJacobian)[5][0] =  m_sDU1.z ;
        (*pOptJacobian)[5][1] =  m_sDV1.z ;
        (*pOptJacobian)[5][2] =  0.0 ;
        (*pOptJacobian)[5][3] =  0.0 ;
        (*pOptJacobian)[5][4] = -m_sDU3.z ;
        (*pOptJacobian)[5][5] = -m_sDV3.z ;
    }

    // Compute function values
    rF[0] = sVecS1MinusS2.x;
    rF[1] = sVecS1MinusS2.y;
    rF[2] = sVecS1MinusS2.z;
    rF[3] = sVecS1MinusS3.x;
    rF[4] = sVecS1MinusS3.y;
    rF[5] = sVecS1MinusS3.z;

    return SM_SUCCESS;


} // end SmSSSIntersectENFO::Evaluate

// clang-format on
} // namespace sc_srf_p3

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_surface_p3.rs).
enum : int32_t
{
    OP_EVAL_GEOMETRIC = 0,
    OP_NORMAL_SECTION = 1,
    OP_SINGULARITIES = 2,
    OP_COVER = 3,
    OP_SHAPE_MATCH = 4,
    OP_FIND_PS_NORMAL = 5,
    OP_SSS_INTERSECT = 6,
    OP_LOCAL_PLANE = 7,
    OP_LOCAL_SOLVE = 8,
    OP_LOCAL_SSS = 9,
};

struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    bool flag() { return in[at++] != 0.0; }
    SmBoolean bflag() { return flag() ? TRUE : FALSE; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
    SmPoint2d uv()
    {
        double u = num();
        double v = num();
        return SmPoint2d(u, v);
    }
    SmExtent2d dom()
    {
        double u0 = num();
        double v0 = num();
        double u1 = num();
        double v1 = num();
        return SmExtent2d(SmPoint2d(u0, v0), SmPoint2d(u1, v1));
    }
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

void put_dom(std::vector<double>& o, const SmExtent2d& d)
{
    o.push_back(d.GetUMin());
    o.push_back(d.GetVMin());
    o.push_back(d.GetUMax());
    o.push_back(d.GetVMax());
}

const SmContext& the_context()
{
    static SmContext* p = new SmContext();
    return *p;
}

struct BadInput
{
};

SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 2)
    {
        // a curve-bounded surface without boundary curves over a basis surface block
        SmSurface* basis = read_surface(rd, ctx);
        bool implicit = rd.flag();
        SmTArray<SmCurve*> c3d, cuv;
        return new (ctx) SmCurveBoundedSurface(*basis, &c3d, &cuv, implicit ? TRUE : FALSE);
    }
    if (kind == 3)
    {
        // a surface of revolution of the line a-b (NURB made) about the axis P, A
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmPoint3d p = rd.vec();
        SmVector3d ax = rd.vec();
        bool swap = rd.flag();
        bool oob = rd.flag();
        SmLine* l = new (ctx) SmLine(a, b, 3, &ctx);
        l->GetOrCreateGwNurbPointer();
        SmSurfOfRevolution* s = NULL;
        SmStatus stt = SmSurfOfRevolution::CreateCanonical(ctx, l, p, ax, s, swap ? TRUE : FALSE);
        if (stt != SM_SUCCESS || !s)
            throw BadInput(); // e.g. a generator on the axis: CreateCanonical yields no surface
        s->SetOutOfBoundsEnabled(oob ? TRUE : FALSE); // the surface and its generator
        return s;
    }
    if (kind == 1)
    {
        SmPoint3d o = rd.vec();
        SmVector3d n = rd.vec();
        return new (ctx) SmPlane(o, n, &ctx);
    }
    // `0 ..` an SmBSplineSurface; `4 ..` the same with the given knots written back over
    // the NURB after the constructor has fixed them up (sm_FixupKnotVector), so
    // near-coincident knots stay as given
    bool raw_knots = kind == 4;
    bool oob = rd.flag();
    SmBSplineSurfaceForm form = (SmBSplineSurfaceForm)rd.idx();
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
    std::vector<double> ku(sur->knu->U, sur->knu->U + r + 1);
    std::vector<double> kv(sur->knv->U, sur->knv->U + s + 1);
    SmBSplineSurface* b = new (ctx) SmBSplineSurface(sur, FALSE, NULL);
    if (raw_knots)
    {
        gw_SURFACE* own = b->GetGwNurbPointer();
        for (gw_INDEX i = 0; i <= r; i++)
            own->knu->U[i] = ku[i];
        for (gw_INDEX i = 0; i <= s; i++)
            own->knv->U[i] = kv[i];
    }
    b->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
    b->SetBSplineSurfaceForm(form);
    return b;
}

std::vector<SmVector3d> read_vecs(Rd& rd)
{
    int64_t n = rd.idx();
    std::vector<SmVector3d> v;
    for (int64_t i = 0; i < n; i++)
        v.push_back(rd.vec());
    return v;
}

// Evaluate one equation set at x: status, then (on success) found, F and the Jacobian.
template <typename Enfo>
void eval_enfo(Enfo& e, int n, const std::vector<double>& x, std::vector<double>& o)
{
    SmTArray<double> X, F;
    for (int i = 0; i < n; i++)
    {
        X.Add(x[i]);
        F.Add(0.0);
    }
    SmMatrix J(n, n);
    SmBoolean found = FALSE;
    SmStatus r = e.Evaluate(X, F, &J, found);
    o.push_back(st(r));
    if (r == SM_SUCCESS)
    {
        o.push_back(flg(found));
        for (int i = 0; i < n; i++)
            o.push_back(F[i]);
        for (int i = 0; i < n; i++)
            for (int j = 0; j < n; j++)
                o.push_back(J[i][j]);
    }
}

std::vector<std::vector<double>> read_points(Rd& rd, int n)
{
    int64_t count = rd.idx();
    std::vector<std::vector<double>> xs;
    for (int64_t k = 0; k < count; k++)
    {
        std::vector<double> x;
        for (int i = 0; i < n; i++)
            x.push_back(rd.num());
        xs.push_back(x);
    }
    return xs;
}

// The evaluation matrix of the surface at uv (two derivatives, upper half) with the unit normal in
// [2][2], as the shape match and the normal section need it.
SmStatus eval_matrix(const SmSurface& s, const SmPoint2d& uv, SmVector3d pvv[3][3])
{
    SmStatus r = s.Evaluate(uv, 2, 2, TRUE, TRUE, TRUE, &pvv[0][0], FALSE);
    if (r != SM_SUCCESS)
        return r;
    SmVector3d n = pvv[1][0] * pvv[0][1];
    if (n.LengthSquared() < SM_EFF_ZERO_SQ)
        r = s.EvaluateNormal(uv, TRUE, TRUE, n);
    else
        n.Unitize();
    pvv[2][2] = n;
    return r;
}

void put_solution(std::vector<double>& o, SmBoolean found, const SmSolution& sol, int n)
{
    o.push_back(flg(found));
    if (found)
    {
        o.push_back(sol.m_vStart.m_dSolutionValue);
        for (int i = 0; i < n; i++)
            o.push_back(sol.m_vStart[i]);
    }
}

void run(int32_t op, Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    SmSurface* s = read_surface(rd, ctx);
    switch (op)
    {
    case OP_EVAL_GEOMETRIC:
    {
        SmPoint2d uv = rd.uv();
        SmBoolean fu = rd.bflag();
        SmBoolean fv = rd.bflag();
        double gauss = 0.0, norm = 0.0, k1 = 0.0, k2 = 0.0;
        SmVector3d efg, lmn, v1, v2;
        SmStatus r = s->EvaluateGeometric(uv, fu, fv, gauss, norm, k1, k2, efg, lmn, v1, v2);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back(gauss);
            o.push_back(norm);
            o.push_back(k1);
            o.push_back(k2);
            put_vec(o, efg);
            put_vec(o, lmn);
            put_vec(o, v1);
            put_vec(o, v2);
        }
        break;
    }
    case OP_NORMAL_SECTION:
    {
        SmPoint2d uv = rd.uv();
        SmBoolean fu = rd.bflag();
        SmBoolean fv = rd.bflag();
        SmVector3d dir = rd.vec();
        SmVector3d tangent, normal;
        double curv = 0.0;
        SmStatus r = s->EvaluateNormalSection(uv, fu, fv, dir, tangent, curv, normal);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            put_vec(o, tangent);
            o.push_back(curv);
            put_vec(o, normal);
        }
        break;
    }
    case OP_SINGULARITIES:
    {
        bool want_pts = rd.flag();
        bool want_nrm = rd.flag();
        SmTArray<SmPoint3d> pts;
        SmTArray<SmVector3d> nrm;
        ULONG approx = 0;
        ULONG sing = s->GetSingularities(want_pts ? &pts : NULL, want_nrm ? &nrm : NULL, &approx);
        o.push_back(0.0);
        o.push_back((double)sing);
        o.push_back((double)approx);
        if (want_pts)
            for (ULONG i = 0; i < 4; i++)
                put_vec(o, pts[i]);
        if (want_nrm)
            for (ULONG i = 0; i < 4; i++)
                put_vec(o, nrm[i]);
        break;
    }
    case OP_COVER:
    {
        SmSurface* other = read_surface(rd, ctx);
        double tol = rd.num();
        bool has_dom = rd.flag();
        SmExtent2d dom = rd.dom();
        SmBoolean ok = FALSE;
        double max_dist = 0.0;
        SmExtent2d covered;
        SmStatus r = s->CoverCoincidentSurface(other, tol, ok, max_dist, has_dom ? &dom : NULL, &covered);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back(flg(ok));
            o.push_back(max_dist);
            if (ok)
                put_dom(o, covered);
            put_dom(o, s->GetSTEPUVDomain());
        }
        delete other;
        break;
    }
    case OP_SHAPE_MATCH:
    {
        SmSurface* s2 = read_surface(rd, ctx);
        SmPoint2d uv0 = rd.uv();
        SmPoint2d uv1 = rd.uv();
        double tol = rd.num();
        double ang = rd.num();
        SmVector3d p0[3][3], p1[3][3];
        SmStatus r = eval_matrix(*s, uv0, p0);
        if (r == SM_SUCCESS)
            r = eval_matrix(*s2, uv1, p1);
        if (r != SM_SUCCESS)
        {
            o.push_back(st(r));
            delete s2;
            break;
        }
        SmBoolean match = FALSE;
        r = smsurf_CheckShapeMatch(p0, p1, tol, ang, match);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
            o.push_back(flg(match));
        delete s2;
        break;
    }
    case OP_FIND_PS_NORMAL:
    {
        SmPoint3d p = rd.vec();
        double sq = rd.num();
        std::vector<std::vector<double>> xs = read_points(rd, 2);
        sc_srf_p3::SmFindPSNormalENFO e(p, *s, sq, SM_SO_MINIMIZE);
        o.push_back(0.0);
        for (const std::vector<double>& x : xs)
            eval_enfo(e, 2, x, o);
        break;
    }
    case OP_SSS_INTERSECT:
    {
        SmSurface* s2 = read_surface(rd, ctx);
        SmSurface* s3 = read_surface(rd, ctx);
        std::vector<std::vector<double>> xs = read_points(rd, 6);
        sc_srf_p3::SmSSSIntersectENFO e(*s, *s2, *s3, SM_EFF_ZERO * SM_EFF_ZERO);
        o.push_back(0.0);
        for (const std::vector<double>& x : xs)
            eval_enfo(e, 6, x, o);
        delete s2;
        delete s3;
        break;
    }
    case OP_LOCAL_PLANE:
    {
        SmSurface* s2 = read_surface(rd, ctx);
        SmExtent2d d1 = rd.dom();
        SmExtent2d d2 = rd.dom();
        double tol = rd.num();
        SmPoint2d g1 = rd.uv();
        SmPoint2d g2 = rd.uv();
        SmVector3d po = rd.vec();
        SmVector3d pn = rd.vec();
        SmBoolean found = FALSE;
        SmSolution sol;
        SmStatus r = s->LocalPlaneSurfaceIntersect(d1, *s2, d2, tol, g1, g2, po, pn, found, sol);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
            put_solution(o, found, sol, 4);
        delete s2;
        break;
    }
    case OP_LOCAL_SOLVE:
    {
        SmSurface* s2 = read_surface(rd, ctx);
        SmExtent2d d1 = rd.dom();
        SmExtent2d d2 = rd.dom();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        std::vector<SmVector3d> vecs = read_vecs(rd);
        SmPoint2d g1 = rd.uv();
        SmPoint2d g2 = rd.uv();
        SmBoolean found = FALSE;
        SmSolution sol;
        SmStatus r = s->LocalSurfaceSolve(d1, *s2, d2, sop, SM_EFF_ZERO, NULL, vecs.empty() ? NULL : vecs.data(), g1,
                                          g2, found, sol);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
            put_solution(o, found, sol, 4);
        delete s2;
        break;
    }
    case OP_LOCAL_SSS:
    {
        SmSurface* s2 = read_surface(rd, ctx);
        SmSurface* s3 = read_surface(rd, ctx);
        SmExtent2d d1 = rd.dom();
        SmExtent2d d2 = rd.dom();
        SmExtent2d d3 = rd.dom();
        SmPoint2d g1 = rd.uv();
        SmPoint2d g2 = rd.uv();
        SmPoint2d g3 = rd.uv();
        SmBoolean found = FALSE;
        SmSolution sol;
        SmStatus r = s->LocalSurfaceSurfaceIntersect(d1, *s2, d2, *s3, d3, g1, g2, g3, found, sol);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
            put_solution(o, found, sol, 6);
        delete s2;
        delete s3;
        break;
    }
    default:
        o.push_back((double)SC_ERR_ARG);
        break;
    }
    delete s;
}
} // namespace

SC_EXPORT int32_t sc_surface_surface_p3_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                             int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    try
    {
        run(op, rd, o);
    }
    catch (const BadInput&)
    {
        *out_len = 0;
        return SC_ERR_ARG;
    }
    if (rd.at != n_in)
    {
        *out_len = 0;
        return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
