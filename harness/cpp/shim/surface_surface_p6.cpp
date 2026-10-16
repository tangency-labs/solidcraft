// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surface_p6`: the SmSurface members owned by the unit
// (source/SMLib/src/SmSurface.cpp, source/SMLib/inc/SmSurface.h), called through `SmSurface*`
// on SmBSplineSurface, SmPlane, SmCurveBoundedSurface and SmSurfOfRevolution objects
// (virtual dispatch as upstream): CalculateContinuities, CreateNaturalUVTrimCurves,
// SmDerivSurfDefinition::GetSampleValues, LocalCurveIntersect, SimpleCoincidenceChecker,
// ClassifyIntersectingRangeSolution, SmSSGlobalSolver::IdenticalSolutions and sm_SCLocalSolve.
//
// SmSSGlobalSolver and the static functions of SmSurface.cpp cannot be linked against, so the
// code exercised here is a VERBATIM copy of upstream SmSurface.cpp (commit c9c5979) inside the
// namespace sc_srf_p6 (line ranges noted at each block): sm_CCLocalSolve and
// sm_PassSCConvergenceTest as called, and IdenticalSolutions on a class holding the members it
// reads (m_pSurf1, m_pSurf2, m_d3dTolerance of its SmGlobalSolver base). sm_SCLocalSolve takes
// what it reads from its nodes and patch instead of them (lines marked ADAPTED): the surface to
// evaluate (sm_GetSurfaceToEvaluate), the patch surface (mBA_pSurface) and the node boxes; its
// call of sm_SpLocalSolve uses the shared verbatim surface_p5 reference body,
// with the same resolved surface as the Rust path.
//
// One entry point, sc_surface_surface_p6_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/surface_surface_p6.rs, which validates every
// input.
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
#include <SmGlobalSolver.h>
#include <SmIsoCurve.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmSolutionArray.h>
#include <SmSurfOfRevolution.h>
#include <SmSurface.h>
#include <SmSurfTypes.h>
#include <SmTArray.h>
#include <SmTree.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

#include "sc_sm.h"

using sc::put_vec;

namespace sc_srf_p6
{
// clang-format off
#include "surface_sp_local_solve.hpp"

// ---- verbatim: SmSurface.cpp lines 3826-3940: sm_CCLocalSolve
static SmStatus sm_CCLocalSolve
  (const SmCurve         & crCurve1,
   const SmExtent1d      & crInterval1,
   SmContinuityType        aeConts1[2],
   const SmCurve         & crCurve2,
   const SmExtent1d      & crInterval2,
   SmContinuityType        aeConts2[2],
   SmSolverOperationType & eSolverOperation,
   double                  d3dTolerance,
   SmSolutionArray       & rSolutions)
{
  rSolutions.ReSet();

  SM_ASSERT(eSolverOperation == SM_SO_MINIMIZE ||
            eSolverOperation == SM_SO_MAXIMIZE);

  SmBoolean bFoundAnswer;
  SmSolution sSolution;
  sSolution.m_lNumVariables = 2;
  sSolution.m_eSolutionType = SM_ST_SINGLE_VALUE;
  // If we have a total discontinuity (i.e. end of curve)
  // then we always test.  If we have a derivative
  // discontinuity then we just test the ones on the left
  // side - otherwise we would do each point two times.
  SmPoint3d sPnt;
  SmSolution sSol;
  if (aeConts1[0] <= SM_CT_C0)
    {
      SER(crCurve1.EvaluatePoint(crInterval1.GetMin(),sPnt));
      SER(crCurve2.LocalPointSolve(crInterval2,eSolverOperation,sPnt,
          NULL,NULL,NULL,crInterval2.Evaluate(0.5),bFoundAnswer,sSol));
      if (bFoundAnswer)
        {
          sSol.m_lNumVariables = 2;
          sSol.m_vStart[1] = sSol.m_vStart[0];
          sSol.m_vStart[0] = crInterval1.GetMin();
          rSolutions.Add(sSol);
        }
    }
  if (aeConts1[1] <= SM_CT_DISCONTINUOUS)
    {
      SER(crCurve1.EvaluatePoint(crInterval1.GetMax(),sPnt));
      SER(crCurve2.LocalPointSolve(crInterval2,eSolverOperation,sPnt,
          NULL,NULL,NULL,crInterval2.Evaluate(0.5),bFoundAnswer,sSol));
      if (bFoundAnswer)
        {
          sSol.m_lNumVariables = 2;
          sSol.m_vStart[1] = sSol.m_vStart[0];
          sSol.m_vStart[0] = crInterval1.GetMax();
          rSolutions.Add(sSol);
        }
    }
  if (aeConts2[0] <= SM_CT_C0)
    {
      SER(crCurve2.EvaluatePoint(crInterval2.GetMin(),sPnt));
      SER(crCurve1.LocalPointSolve(crInterval1,eSolverOperation,sPnt,
          NULL,NULL,NULL,crInterval1.Evaluate(0.5),bFoundAnswer,sSol));
      if (bFoundAnswer)
        {
          sSol.m_lNumVariables = 2;
          sSol.m_vStart[1] = crInterval2.GetMin();
          rSolutions.Add(sSol);
        }
    }
  if (aeConts2[1] <= SM_CT_DISCONTINUOUS)
    {
      SER(crCurve2.EvaluatePoint(crInterval2.GetMax(),sPnt));
      SER(crCurve1.LocalPointSolve(crInterval1,eSolverOperation,sPnt,
                                   NULL,NULL,NULL,crInterval1.Evaluate(0.5),bFoundAnswer,sSol));
      if (bFoundAnswer)
        {
          sSol.m_lNumVariables = 2;
          sSol.m_vStart[1] = crInterval2.GetMax();
          rSolutions.Add(sSol);
        }
    }

  double daGuess1[4], daGuess2[4];
  ULONG lNumGuess = 0;

  if (lNumGuess == 0)
    {
      // Take midpoint of interval for guess.
      lNumGuess = 1;
      daGuess1[0] = crInterval1.Evaluate(0.5);
      daGuess2[0] = crInterval2.Evaluate(0.5);
    }

  SmSolverOperationType eLocalSolverOp = eSolverOperation;

  for (ULONG i=0; i<lNumGuess; i++)
    {
      SmBoolean bFoundSolution;
      SmSolution sSolut;
      SER(crCurve1.LocalCurveSolve(crInterval1,
                                   crCurve2,
                                   crInterval2,
                                   eLocalSolverOp,
                                   d3dTolerance,
                                   NULL,
                                   NULL,
                                   daGuess1[i],
                                   daGuess2[i],
                                   bFoundSolution,
                                   sSolut ));

      if (bFoundSolution)
        {
          sSolut.m_lNumVariables = 2;
          rSolutions.Add( sSolut );
        }
    }
  return SM_SUCCESS;

} // end sm_CCLocalSolve
// ---- end verbatim

// ---- verbatim: SmSurface.cpp lines 4750-4805: sm_PassSCConvergenceTest
static SmBoolean sm_PassSCConvergenceTest
  (const SmSurface & crSurface,   // in :
   const SmPoint2d & crUV,        // in :
   const SmCurve   & crCurve,     // in :
   double            dParameter)  // in :
{
  // surface eval
  SmPoint3d sPnt;
  SmVector3d sDU, sDV;
  crSurface.Evaluate1stDerivatives(crUV,TRUE,TRUE,sPnt,sDU,sDV);  // nonZeroTangents

  // curve eval
  SmVector3d sPV[2]; // sPV[0] = crCurve 3dPoint    for param = dParameter
                     // sPV[1] = crCurve 3dTangent  for param = dParameter
  crCurve.Evaluate(dParameter,1,TRUE,sPV, TRUE);  // NonZeroTangents

  // curve surface displacement
  SmVector3d sDiff = sPnt - sPV[0];
  double dScale = 1.0 + sPnt.GetMaxDimension();

  // when surf and curve tangents are nonZero
  // and displacement is nonZero
  // and angle between any tangent and the displacement is not 90 degrees
  if(   smos_Fabs(sDU.Dot(sDiff))    > SM_EFF_ZERO * dScale
     || smos_Fabs(sDV.Dot(sDiff))    > SM_EFF_ZERO * dScale
     || smos_Fabs(sPV[1].Dot(sDiff)) > SM_EFF_ZERO * dScale)
       {
      sDiff.Unitize();
      if (sDU.LengthSquared() > SM_EFF_ZERO_SQ)
        {
          sDU.Unitize();
          if (smos_Fabs(sDU.Dot(sDiff)) > SM_EFF_ZERO)
            {
              return FALSE;
            }
        }
      if (sDV.LengthSquared() > SM_EFF_ZERO_SQ)
        {
          sDV.Unitize();
          if (smos_Fabs(sDV.Dot(sDiff)) > SM_EFF_ZERO)
            {
              return FALSE;
            }
        }
      if (sPV[1].LengthSquared() > SM_EFF_ZERO_SQ)
        {
          sPV[1].Unitize();
          if (smos_Fabs(sPV[1].Dot(sDiff)) > SM_EFF_ZERO)
            {
              return FALSE;
            }
        }
    }
  return TRUE;

} // end sm_PassSCConvergenceTest
// ---- end verbatim

// ---- verbatim (ADAPTED lines marked): SmSurface.cpp lines 4812-5129: sm_SCLocalSolve
static SmStatus sm_SCLocalSolve
  (const SmSurface       * pSrfEvalArg,             // ADAPTED: in : sm_GetSurfaceToEvaluate(pSurfaceNode, ..)
   const SmSurface       & sBSSArg,                 // ADAPTED: in : *pBezPatch->mBA_pSurface
   const SmExtent3d      & crSurfaceNodeBox,        // ADAPTED: in : pSurfaceNode->m_sBBox
   const SmExtent2d      & crUVDomain,              // in :
   SmContinuityType        aeUConts[2],             // in :
   SmContinuityType        aeVConts[2],             // in :
   const SmExtent3d      * pCurveNodeBox,           // ADAPTED: in : pCurveNode ? &pCurveNode->m_sBBox : NULL
   const SmCurve         & crCurve,                 // in :
   const SmExtent1d      & crInterval,              // in :
   SmContinuityType        aeTConts[2],             // in :
   double                  d3dTolerance,            // in :
   const SmVector3d      * cpOptVectors,            // in :
   SmSolverOperationType & eSolverOperation,        // in :
   SmBoolean               bSkipNormalize,          // in :
   SmBoolean               bProcessBoundaryCurves,  // in :
   double                & rdBestAnswerSoFarSq,     // NotUsed: out:
   SmSolutionArray       & rSolutions)              // out:
{
  SM_REF1(rdBestAnswerSoFarSq) ;
  // check input
  SM_ASSERT(   eSolverOperation == SM_SO_MINIMIZE
            || eSolverOperation == SM_SO_MAXIMIZE
            || eSolverOperation == SM_SO_3D_SIGNED_DIRECTED_MINIMIZE
            || eSolverOperation == SM_SO_NORMALIZE);

  // locals
  //const SmSurface * pSurf    = sm_GetOriginalSurface(*pSurfaceNode);
  //const SmContext * pContext = pSurf->GetContext();

  const SmSurface &sBSS = sBSSArg ;                 // ADAPTED

  const SmSurface *pSrfEval = pSrfEvalArg;          // ADAPTED

  // for two passes
  for (ULONG j=0; j<2; j++)
    {
      SmSolution      aSols[10];
      SmSolutionArray sSolutions(10,aSols);

      // Handle curve discontinuities only for min and max
      if (    eSolverOperation != SM_SO_MAXIMIZE
           && eSolverOperation != SM_SO_MINIMIZE)
          continue;

      if ( aeTConts[j] <= SM_CT_C0 )
        {
          // following line prevents multiple solves for points inside curve.
          if ( j==1 && aeTConts[j] != SM_CT_DISCONTINUOUS )
              continue;

          double dT = ( j == 0 ) ? crInterval.GetMin() : crInterval.GetMax();

          SmPoint3d sPnt;
          SER( crCurve.EvaluatePoint( dT, sPnt ));
          // ADAPTED: node/patch resolution supplies pSrfEval, as above.
          SER( sm_SpLocalSolve( pSrfEval,
                                crUVDomain,
                                aeUConts,
                                aeVConts,
                                sPnt,
                                d3dTolerance,
                                eSolverOperation,
                                TRUE,
                                bProcessBoundaryCurves,
                                sSolutions ));

          for ( ULONG k=0; k<sSolutions.GetSize(); k++ )
            {
              SmSolution & rSol = sSolutions[k];
              rSol.m_lNumVariables = 3;
              rSol.m_vStart[2] = rSol.m_vStart[1];
              rSol.m_vStart[1] = rSol.m_vStart[0];
              rSol.m_vStart[0] = dT;
              rSol.m_eSolutionType = SM_ST_SINGLE_VALUE;
              rSolutions.Add( rSol );
            }
        }

      // Handle surface boundary discontinuities
      // Note that this will not handle case where boundaries
      // are coincident with the curve.  This should be taken
      // care of automatically by the above point on surface
      // testing stuff.
      if ( !bProcessBoundaryCurves )
          continue;

      if ( aeUConts[j] <= SM_CT_C0 )
        {
          // Solve for internal curves only once.
          if ( j==1 && aeUConts[j] != SM_CT_DISCONTINUOUS )
              continue;

          double dU = crUVDomain.GetMin().x;
          SmBoolean bFlag = TRUE;
          if ( j==1 )
            {
              dU = crUVDomain.GetMax().x;
              bFlag = FALSE;
            }
          SmIsoCurve sIC( *pSrfEval, SM_SP_U, dU, bFlag );
          sIC.SetContext( NULL );

          if ( !sIC.IsDegenerate() )
            {
              SmExtent1d sVIvl( crUVDomain.GetMin().y, crUVDomain.GetMax().y );
              SmContinuityType sTmpConts[2];

              // Note that point/surface is
              // already taken care of in above section.
              sTmpConts[0] = SM_CT_C1_C2;
              sTmpConts[1] = SM_CT_C1_C2;
              SER( sm_CCLocalSolve( crCurve,
                                      crInterval,
                                      sTmpConts,
                                      sIC,
                                      sVIvl,
                                      aeVConts,
                                      eSolverOperation,
                                      d3dTolerance,
                                      sSolutions));
              for ( ULONG k=0; k < sSolutions.GetSize(); k++ )
                {
                  SmSolution & rSol = sSolutions[k];
                  rSol.m_lNumVariables = 3;
                  rSol.m_vStart[2] = rSol.m_vStart[1];
                  rSol.m_vStart[1] = dU;
                  rSol.m_eSolutionType = SM_ST_SINGLE_VALUE;
                  rSolutions.Add( rSol );
                }
            }
        }

      if ( aeVConts[j] <= SM_CT_C0 )
        {
          // Solve for internal curves only once.
          if ( j==1 && aeVConts[j] != SM_CT_DISCONTINUOUS )
              continue;

          double dV = crUVDomain.GetMin().y;
          SmBoolean bFlag = TRUE;
          if ( j==1 )
            {
              dV = crUVDomain.GetMax().y;
              bFlag = FALSE;
            }
          SmIsoCurve sIC( *pSrfEval, SM_SP_V, dV, bFlag );
          sIC.SetContext( NULL );

          if ( !sIC.IsDegenerate() )
            {
              SmExtent1d sUIvl( crUVDomain.GetMin().x, crUVDomain.GetMax().x );
              SmContinuityType sTmpConts[2];

              // Note that point/surface is
              // already taken care of in above section.
              sTmpConts[0] = SM_CT_C1_C2;
              sTmpConts[1] = SM_CT_C1_C2;
              SER( sm_CCLocalSolve( crCurve,
                                      crInterval,
                                      sTmpConts,
                                      sIC,
                                      sUIvl,
                                      aeUConts,
                                      eSolverOperation,
                                      d3dTolerance,
                                      sSolutions));

              for ( ULONG k=0; k<sSolutions.GetSize(); k++ )
                {
                  SmSolution & rSol = sSolutions[k];
                  rSol.m_lNumVariables = 3;
                  rSol.m_vStart[2] = dV;
                  rSol.m_eSolutionType = SM_ST_SINGLE_VALUE;
                  rSolutions.Add( rSol );
                }
            }
        }
  } // for to handle discontinuities

  // If we don't need to do normalize just return here
  if ( bSkipNormalize )
      return SM_SUCCESS;

  SmPoint2d sUVGuesses[6];
  double sTGuesses[6];
  ULONG lNumGuesses=0;

  // Take three guesses to try to solve for intersection.
  // Note that this is not good enough.  We'll have to add
  // some post processing to determine if additional intersections
  // exist.
  sUVGuesses[ lNumGuesses   ] = crUVDomain.Evaluate(0.5,0.5);
  sTGuesses [ lNumGuesses++ ] = crInterval.Evaluate(0.5);
  sUVGuesses[ lNumGuesses   ] = crUVDomain.Evaluate(0.5,0.5);
  sTGuesses [ lNumGuesses++ ] = crInterval.Evaluate(0.0);
  sUVGuesses[ lNumGuesses   ] = crUVDomain.Evaluate(0.5,0.5);
  sTGuesses [ lNumGuesses++ ] = crInterval.Evaluate(1.0);

  if ( pCurveNodeBox )                              // ADAPTED: if ( pCurveNode )
    {
      double dSurfBoxSize  = crSurfaceNodeBox.GetSize().Length();   // ADAPTED
      double dCurveBoxSize = pCurveNodeBox->GetSize().Length();     // ADAPTED

      // See if we need to obtain better start/guess point
      // when one bounding box is much larger than the other
      if ( dSurfBoxSize > 3.0*dCurveBoxSize )
        {
          // Drop point from center of curve to surface
          SmSolution sSolution;
          SmBoolean bFoundAnswer;
          SmPoint3d sCurvePnt, sSurfPnt;
          SER( crCurve.EvaluatePoint( crInterval.Evaluate(0.5), sCurvePnt ));
          SER( sBSS.LocalPointSolve( crUVDomain, SM_SO_MINIMIZE, sCurvePnt,
                                     sUVGuesses[0], bFoundAnswer, sSolution ));

          if ( bFoundAnswer )
            {
              // Drop point back to curve &
              // replace sUVGuess[0], sTGuesses[0] with values
              sUVGuesses[0] = SmPoint2d(
                  sSolution.m_vStart[0], sSolution.m_vStart[1] );
              SER( sBSS.EvaluatePoint( sUVGuesses[0], sSurfPnt ));

              SER( crCurve.LocalPointSolve( crInterval,
                                            SM_SO_MINIMIZE,
                                            sSurfPnt,
                                            NULL,
                                            NULL,
                                            NULL,
                                            sTGuesses[0],
                                            bFoundAnswer,
                                            sSolution ));

              if ( bFoundAnswer )
                {
                  sTGuesses[0] = sSolution.m_vStart[0];
                }
            }
        }

      if ( dCurveBoxSize > 3.0*dSurfBoxSize )
        {
          // Drop point from center of surface to curve
          SmSolution sSolution;
          SmBoolean bFoundAnswer;
          SmPoint3d sCurvePnt, sSurfPnt;
          SER( sBSS.EvaluatePoint(crUVDomain.Evaluate(0.5,0.5),sSurfPnt));
          SER( crCurve.LocalPointSolve( crInterval,
                                        SM_SO_MINIMIZE,
                                        sSurfPnt,
                                        NULL,
                                        NULL,
                                        NULL,
                                        sTGuesses[0],
                                        bFoundAnswer,
                                        sSolution ));

          if ( bFoundAnswer )
            {
              // Drop point back to surface
              sTGuesses[0] = sSolution.m_vStart[0];
              SER( crCurve.EvaluatePoint( sTGuesses[0], sCurvePnt ));
              SER( sBSS.LocalPointSolve( crUVDomain, SM_SO_MINIMIZE, sCurvePnt,
                  sUVGuesses[0], bFoundAnswer, sSolution ));
              if ( bFoundAnswer )
                {
                  sUVGuesses[0] = SmPoint2d(
                      sSolution.m_vStart[0], sSolution.m_vStart[1] );
                }
            }
        }
    }

  for (ULONG i=0; i<lNumGuesses; i++)
    {
      SmBoolean bFoundAnswer;
      SmSolution sSolution;
      SER( pSrfEval->LocalCurveSolve( crUVDomain,
                                      crCurve,
                                      crInterval,
                                      eSolverOperation,
                                      d3dTolerance,
                                      NULL,
                                      cpOptVectors,
                                      sUVGuesses[i],
                                      sTGuesses[i],
                                      bFoundAnswer,
                                      sSolution ));

      if ( bFoundAnswer && eSolverOperation != SM_SO_3D_SIGNED_DIRECTED_MINIMIZE )
        {
          SmPoint2d sUV( sSolution.m_vStart[1], sSolution.m_vStart[2] );

          if ( !sm_PassSCConvergenceTest( *pSrfEval,
                                              sUV,
                                              crCurve,
                                              sSolution.m_vStart[0] ))
            {
              bFoundAnswer = FALSE;
            }
        }

      if ( bFoundAnswer )
        {
          sSolution.m_lNumVariables = 3;
          rSolutions.Add(sSolution);
        }
    } // For each guess

  return SM_SUCCESS;

} // end sm_SCLocalSolve
// ---- end verbatim

// ---- verbatim (class reduced to the members IdenticalSolutions reads): SmSurface.cpp lines
// 20910-20938 (class SmSSGlobalSolver) and 21522-21657 (IdenticalSolutions)
class SmSSGlobalSolver : public SmGlobalSolver
{
public:
  const SmSurface  * m_pSurf1;         // surface1 being solved for
  const SmSurface  * m_pSurf2;         // surface2 being solved for

  SmSSGlobalSolver(const SmSurface * pSurf1, const SmSurface * pSurf2, double d3dTolerance)   // ADAPTED
    : SmGlobalSolver(), m_pSurf1(pSurf1), m_pSurf2(pSurf2) { m_d3dTolerance = d3dTolerance; }

  // return TRUE when two solutions are identical
  virtual SmSolutionPairType IdenticalSolutions(const SmSolution & crSol1,
                                                const SmSolution & crSol2,
                                                const SmSolution *&pPreferredSol) const;
} ; // end class SmSSGlobalSolver

SmSolutionPairType SmSSGlobalSolver::IdenticalSolutions // rtn: oneof SM_SP_IDENTICAL
                                                        //            SM_SP_DIFFCOUNTS
                                                        //            SM_SP_DIFFOBJECTS
                                                        //            SM_SP_DIFFPARAMETERS
  (const SmSolution & crSol1,                           // in : 1st target solution
   const SmSolution & crSol2,                           // in : 2nd target solution
   const SmSolution *&pPreferredSol)                    // out: When identical, set to the preferred solution or NULL for none.
                                                        //      NULL to ignore, default:[NULL]
  const
{
  // init output
  pPreferredSol = NULL ;

  // If the parent thinks they're identical, then they are.
  SmSolutionPairType eSPType = SmGlobalSolver::IdenticalSolutions( crSol1, crSol2, pPreferredSol ) ;
  if(SM_SP_IDENTICAL == eSPType)
      { return SM_SP_IDENTICAL; }

  if(SM_SP_DIFFPARAMETERS == eSPType)
    {
      if(   crSol1.m_eSolutionType == SM_ST_SINGLE_VALUE
         && crSol2.m_eSolutionType == SM_ST_SINGLE_VALUE)
        {
          // At a singularity, the same 3d point can have different uv values.
          SmPoint2d sUV1A( crSol1.m_vStart[1], crSol1.m_vStart[2] );
          SmPoint2d sUV1B( crSol2.m_vStart[1], crSol2.m_vStart[2] );
          SmPoint2d sUV2A( crSol1.m_vStart[3], crSol1.m_vStart[4] );
          SmPoint2d sUV2B( crSol2.m_vStart[3], crSol2.m_vStart[4] );

          // Check positions.
          SmPoint3d sPt1A, sPt1B, sPt2A, sPt2B;
          m_pSurf1->EvaluatePoint( sUV1A, sPt1A );
          m_pSurf1->EvaluatePoint( sUV1B, sPt1B );
          m_pSurf2->EvaluatePoint( sUV2A, sPt2A );
          m_pSurf2->EvaluatePoint( sUV2B, sPt2B );

          double dTol = m_d3dTolerance; // untested: see how it works in practice

          if(   !sPt1A.CloserThan( dTol, sPt2A )
             || !sPt1A.CloserThan( dTol, sPt2B )
             || !sPt1B.CloserThan( dTol, sPt2A )
             || !sPt1B.CloserThan( dTol, sPt2B ) )
              { return SM_SP_DIFFPARAMETERS; }

          // 3d points are the same.  Need to check seams and poles.
          // Check poles first, it's a cheaper check.
          // If either is nonsingular, they're distinct.

          // Surface1 - both solutions are on singularities
          SmSurfParamType eSingDir;
          SmBoolean bSurf1Singular = (   m_pSurf1->IsSingularity( sUV1A, eSingDir )
                                      && m_pSurf1->IsSingularity( sUV1B, eSingDir )) ;

          // Surface2 - both solutions are on singularities
          SmBoolean bSurf2Singular = (   m_pSurf2->IsSingularity( sUV2A, eSingDir )
                                      && m_pSurf2->IsSingularity( sUV2B, eSingDir )) ;

          // when neither Surf1 nor Surf2 solutions are on a singularity
          if(   !bSurf1Singular
             && !bSurf2Singular)
            { return SM_SP_DIFFPARAMETERS; }

          SmBoolean bASol1OnSeam =    m_pSurf1->IsOnSeam( sUV1A )
                                   || m_pSurf2->IsOnSeam( sUV2A ) ;
          SmBoolean bASol2OnSeam =    m_pSurf1->IsOnSeam( sUV1B )
                                   || m_pSurf2->IsOnSeam( sUV2B ) ;

          if( bASol1OnSeam && !bASol2OnSeam) { pPreferredSol = &crSol1 ;
                                               return SM_SP_IDENTICAL ;
                                             }
          if(!bASol1OnSeam &&  bASol2OnSeam) { pPreferredSol = &crSol2 ;
                                               return SM_SP_IDENTICAL ;
                                             }
          if(!bASol1OnSeam && !bASol2OnSeam) { pPreferredSol =  (crSol1.m_vStart.m_dSolutionValue < crSol2.m_vStart.m_dSolutionValue)
                                                              ? &crSol1
                                                              : &crSol2 ;
                                               return SM_SP_IDENTICAL ;
                                             }
        }
    }

  // Identical positions, at a pole on a seam:
  // Call them distinct points.
  return eSPType;

} // end SmSSGlobalSolver::IdenticalSolutions
// ---- end verbatim
// clang-format on
} // namespace sc_srf_p6

namespace
{
enum : int32_t
{
    OP_CONT = 0,
    OP_TRIM = 1,
    OP_SAMPLE = 2,
    OP_LCI = 3,
    OP_SCC = 4,
    OP_CLASSIFY = 5,
    OP_SSID = 6,
    OP_SCLS = 7,
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
    SmExtent1d ivl()
    {
        double a = num();
        double b = num();
        return SmExtent1d(a, b);
    }
    SmExtent2d dom()
    {
        double u0 = num();
        double v0 = num();
        double u1 = num();
        double v1 = num();
        return SmExtent2d(SmPoint2d(u0, v0), SmPoint2d(u1, v1));
    }
    SmExtent3d box()
    {
        SmPoint3d a = vec();
        SmPoint3d b = vec();
        return SmExtent3d(a, b);
    }
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

const SmContext& the_context()
{
    static SmContext* p = new SmContext();
    return *p;
}

SmBSplineCurve* read_bspline_curve(Rd& rd, const SmContext& ctx)
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

// `0 bspline` (SmBSplineCurve) or `1 A[3] B[3]` (SmLine::CreateLineSegment, 3D).
SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 1)
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine* l = NULL;
        if (SmLine::CreateLineSegment(ctx, 3, a, b, l) != SM_SUCCESS)
            return NULL;
        return l;
    }
    return read_bspline_curve(rd, ctx);
}

SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 3)
    {
        // an SmSurfOfRevolution::CreateCanonical of an SmLine generator
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmPoint3d p = rd.vec();
        SmVector3d ax = rd.vec();
        bool swap = rd.flag();
        SmLine* gen = new (ctx) SmLine(a, b, 3, &ctx);
        gen->GetOrCreateGwNurbPointer();
        SmSurfOfRevolution* s = NULL;
        SmStatus stt = SmSurfOfRevolution::CreateCanonical(ctx, gen, p, ax, s, swap ? TRUE : FALSE);
        if (stt != SM_SUCCESS || s == NULL)
        {
            if (s)
                delete s;
            else
                delete gen;
            return NULL;
        }
        return s;
    }
    if (kind == 2)
    {
        // a curve-bounded surface without boundary curves over a basis surface block
        SmSurface* basis = read_surface(rd, ctx);
        bool implicit = rd.flag();
        if (basis == NULL)
            return NULL;
        SmTArray<SmCurve*> c3d, cuv;
        return new (ctx) SmCurveBoundedSurface(*basis, &c3d, &cuv, implicit ? TRUE : FALSE);
    }
    if (kind == 1)
    {
        SmPoint3d o = rd.vec();
        SmVector3d n = rd.vec();
        return new (ctx) SmPlane(o, n, &ctx);
    }
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
    SmBSplineSurface* b = new (ctx) SmBSplineSurface(sur, FALSE, NULL);
    b->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
    b->SetBSplineSurfaceForm(form);
    return b;
}

// A curve as `ivl_min ivl_max P(0) P(1/4) P(1/2) P(3/4) P(1)` over its natural interval
// (EvaluatePoint statuses: a failure writes NaN).
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    for (int i = 0; i <= 4; i++)
    {
        SmPoint3d p;
        if (c->EvaluatePoint(ivl.Evaluate(i / 4.0), p) == SM_SUCCESS)
            put_vec(o, p);
        else
            put_vec(o, SmVector3d(NAN, NAN, NAN));
    }
}

SmContinuityType cont(Rd& rd) { return (SmContinuityType)rd.idx(); }

// A solution `type nvars value end_value params[5] end_params[5]` with objects `objs`.
SmSolution read_solution(Rd& rd, SmObject* o0, SmObject* o1)
{
    SmSolution s;
    s.m_eSolutionType = (SmSolutionType)rd.idx();
    s.m_lNumVariables = (ULONG)rd.idx();
    s.m_lNumObjects = 2;
    s.m_apObjects[0] = o0;
    s.m_apObjects[1] = o1;
    s.m_vStart.m_dSolutionValue = rd.num();
    s.m_vEnd.m_dSolutionValue = rd.num();
    for (ULONG i = 0; i < 5; i++)
        s.m_vStart[i] = rd.num();
    for (ULONG i = 0; i < 5; i++)
        s.m_vEnd[i] = rd.num();
    return s;
}

void run(int32_t op, Rd& rd, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    SmSurface* s = read_surface(rd, ctx);
    switch (op)
    {
    case OP_CONT:
    {
        SmSurfParamType param = (SmSurfParamType)rd.idx();
        if (s == NULL)
        {
            o.push_back(-3.0);
            break;
        }
        SmContinuityType eMin = SM_CT_UNDEFINED;
        SmTArray<SmContinuityType> conts;
        SmStatus r = s->CalculateContinuities(param, eMin, conts);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back((double)eMin);
            o.push_back((double)conts.GetSize());
            for (ULONG i = 0; i < conts.GetSize(); i++)
                o.push_back((double)conts[i]);
        }
        break;
    }
    case OP_TRIM:
    {
        SmExtent2d d = rd.dom();
        SmSurfParamType side = (SmSurfParamType)rd.idx();
        SmBoolean remove = rd.bflag();
        if (s == NULL)
        {
            o.push_back(-3.0);
            break;
        }
        SmTArray<SmCurve*> c3d;
        SmTArray<SmBSplineCurve*> cuv;
        SmTArray<SmOrientType> orients;
        SmTArray<double> gaps;
        SmStatus r = s->CreateNaturalUVTrimCurves(ctx, d, side, remove, c3d, cuv, orients, &gaps);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back((double)c3d.GetSize());
            for (ULONG i = 0; i < c3d.GetSize(); i++)
            {
                o.push_back((double)orients[i]);
                o.push_back(gaps[i]);
                put_curve(o, c3d[i]);
                put_curve(o, cuv[i]);
            }
        }
        for (ULONG i = 0; i < c3d.GetSize(); i++)
            delete c3d[i];
        for (ULONG i = 0; i < cuv.GetSize(); i++)
            delete cuv[i];
        break;
    }
    case OP_SAMPLE:
    {
        SmDerivSurfDefinition def;
        def.m_lNumSamples = (ULONG)rd.idx();
        def.m_eTangencyType = (SmTangencyType)rd.idx();
        def.m_dStartDerivDirAngleDeg = rd.num();
        def.m_dEndDerivDirAngleDeg = rd.num();
        def.m_dStartTanLength = rd.num();
        def.m_dEndTanScale = rd.num();
        def.m_dStartAngleDeg = rd.num();
        def.m_dEndAngleDeg = rd.num();
        bool has_ref = rd.flag();
        SmCurve* ref = has_ref ? read_curve(rd, ctx) : NULL;
        SmCurve* bc = read_curve(rd, ctx);
        SmExtent1d ivl = rd.ivl();
        SmBoolean right = rd.bflag();
        SmBoolean start = rd.bflag();
        bool want2 = rd.flag();
        bool want3 = rd.flag();
        if (s == NULL || bc == NULL || (has_ref && ref == NULL))
        {
            o.push_back(-3.0);
            delete ref;
            delete bc;
            break;
        }
        def.m_pTangentReference = ref;
        SmTArray<double> params;
        SmTArray<SmVector3d> pos, d1, d2, d3;
        SmStatus r = def.GetSampleValues(*s, *bc, ivl, right, start, params, pos, d1, want2 ? &d2 : NULL,
                                         want3 ? &d3 : NULL);
        def.m_pTangentReference = NULL;
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back((double)params.GetSize());
            for (ULONG i = 0; i < params.GetSize(); i++)
            {
                o.push_back(params[i]);
                put_vec(o, pos[i]);
                put_vec(o, d1[i]);
                if (want2)
                    put_vec(o, d2[i]);
                if (want3)
                    put_vec(o, d3[i]);
            }
        }
        delete ref;
        delete bc;
        break;
    }
    case OP_LCI:
    {
        SmExtent2d d = rd.dom();
        SmCurve* c = read_curve(rd, ctx);
        SmExtent1d ivl = rd.ivl();
        double tol = rd.num();
        SmPoint2d g = rd.uv();
        double gt = rd.num();
        if (s == NULL || c == NULL)
        {
            o.push_back(-3.0);
            delete c;
            break;
        }
        SmBoolean found = FALSE;
        SmPoint2d uv;
        double t = 0.0, dev = 0.0;
        SmStatus r = s->LocalCurveIntersect(d, *c, ivl, tol, g, gt, found, uv, t, dev);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back(flg(found));
            if (found)
            {
                o.push_back(uv.x);
                o.push_back(uv.y);
                o.push_back(t);
                o.push_back(dev);
            }
        }
        delete c;
        break;
    }
    case OP_SCC:
    {
        SmExtent2d d = rd.dom();
        SmCurve* c = read_curve(rd, ctx);
        SmExtent1d ivl = rd.ivl();
        double tol = rd.num();
        SmBoolean partial = rd.bflag();
        if (s == NULL || c == NULL)
        {
            o.push_back(-3.0);
            delete c;
            break;
        }
        SmBoolean found = FALSE;
        SmSolution sol;
        SmStatus r = s->SimpleCoincidenceChecker(d, *c, ivl, tol, found, sol, partial);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back(flg(found));
            if (found)
            {
                o.push_back((double)sol.m_eSolutionType);
                o.push_back(flg(sol.m_apObjects[0] == c));
                o.push_back(flg(sol.m_apObjects[1] == s));
                o.push_back(sol.m_vStart.m_dSolutionValue);
                for (ULONG i = 0; i < 3; i++)
                    o.push_back(sol.m_vStart[i]);
                o.push_back(sol.m_vEnd.m_dSolutionValue);
                for (ULONG i = 0; i < 3; i++)
                    o.push_back(sol.m_vEnd[i]);
            }
        }
        delete c;
        break;
    }
    case OP_CLASSIFY:
    {
        SmCurve* c = read_curve(rd, ctx);
        SmSolution sol = read_solution(rd, c, s);
        if (s == NULL || c == NULL)
        {
            o.push_back(-3.0);
            delete c;
            break;
        }
        SmBoolean degen = FALSE;
        SmSolutionEnd pt;
        pt.m_dSolutionValue = 12345.0;
        pt.m_adParameters[0] = 12345.0;
        pt.m_adParameters[1] = 12345.0;
        pt.m_adParameters[2] = 12345.0;
        SmStatus r = s->ClassifyIntersectingRangeSolution(*c, sol, 0.0, degen, pt);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back(flg(degen));
            o.push_back(pt.m_dSolutionValue);
            for (ULONG i = 0; i < 3; i++)
                o.push_back(pt.m_adParameters[i]);
        }
        delete c;
        break;
    }
    case OP_SSID:
    {
        SmSurface* s2 = read_surface(rd, ctx);
        double tol = rd.num();
        SmSolution a = read_solution(rd, s, s2);
        SmSolution b = read_solution(rd, s, s2);
        if (s == NULL || s2 == NULL)
        {
            o.push_back(-3.0);
            delete s2;
            break;
        }
        sc_srf_p6::SmSSGlobalSolver solver(s, s2, tol);
        const SmSolution* pref = NULL;
        SmSolutionPairType t = solver.IdenticalSolutions(a, b, pref);
        o.push_back(0.0);
        o.push_back((double)t);
        o.push_back(pref == NULL ? 0.0 : pref == &a ? 1.0 : 2.0);
        delete s2;
        break;
    }
    case OP_SCLS:
    {
        SmExtent2d d = rd.dom();
        SmContinuityType uc[2], vc[2], tc[2];
        uc[0] = cont(rd);
        uc[1] = cont(rd);
        vc[0] = cont(rd);
        vc[1] = cont(rd);
        SmExtent3d sbox = rd.box();
        bool has_cbox = rd.flag();
        SmExtent3d cbox = rd.box();
        SmCurve* c = read_curve(rd, ctx);
        SmExtent1d ivl = rd.ivl();
        tc[0] = cont(rd);
        tc[1] = cont(rd);
        double tol = rd.num();
        bool has_vec = rd.flag();
        SmVector3d vec = rd.vec();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        SmBoolean skip = rd.bflag();
        SmBoolean process = rd.bflag();
        if (s == NULL || c == NULL)
        {
            o.push_back(-3.0);
            delete c;
            break;
        }
        double best = 0.0;
        SmSolutionArray sols;
        SmStatus r = sc_srf_p6::sm_SCLocalSolve(s, *s, sbox, d, uc, vc, has_cbox ? &cbox : NULL, *c, ivl, tc, tol,
                                                has_vec ? &vec : NULL, sop, skip, process, best, sols);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            o.push_back((double)sols.GetSize());
            for (ULONG i = 0; i < sols.GetSize(); i++)
            {
                o.push_back((double)sols[i].m_eSolutionType);
                o.push_back((double)sols[i].m_lNumVariables);
                o.push_back(sols[i].m_vStart.m_dSolutionValue);
                for (ULONG k = 0; k < 3; k++)
                    o.push_back(sols[i].m_vStart[k]);
            }
        }
        delete c;
        break;
    }
    default:
        o.push_back((double)SC_ERR_ARG);
        break;
    }
    delete s;
}
} // namespace

SC_EXPORT int32_t sc_surface_surface_p6_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                             int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    run(op, rd, o);
    if (rd.at != n_in)
    {
        *out_len = 0;
        return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
