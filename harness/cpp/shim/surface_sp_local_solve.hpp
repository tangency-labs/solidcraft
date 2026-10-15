// SPDX-License-Identifier: Apache-2.0
// Shared native reference for surface_p5 and surface_p6. Included inside each
// shim's namespace after the SMLib headers. The functions below retain the
// verbatim upstream SmSurface.cpp bodies (commit c9c5979); node resolution is
// adapted to the concrete surface supplied by both differential runners.
#pragma once

// ---- verbatim (two ADAPTED lines): SmSurface.cpp lines 4117-4285: sm_LoadGuesses
static SmStatus sm_LoadGuesses
  (const SmSurface      * pSurfaceOwner,      // ADAPTED: in : the node's owner surface (pSurfaceNode->GetOwnerObject())
   SmBezierPatch        * pBezPatch,          // NotUsed: in : SurfaceNode Bezier patch
   const SmExtent2d     & crUVDomain,         // in : Problem domain limits
   const SmPoint3d      & crTestPnt,          // in : target solution start point
   SmSolverOperationType  eSolverOperation,   // in : oneof  SM_SO_MINIMIZE,
                                              //             SM_SO_MAXIMIZE,
                                              //             SM_SO_NORMALIZE,
                                              //             SM_SO_INTERSECT
   SmPoint2d            * sGuesses,           // out: UVGuess Point or not used
   ULONG                & lNumGuesses)        // out: set to 1 or 0
{
  SM_REF1(pBezPatch) ;
  // check state - restricted set of operations
  SM_ASSERT(   eSolverOperation == SM_SO_MINIMIZE
            || eSolverOperation == SM_SO_MAXIMIZE
            || eSolverOperation == SM_SO_NORMALIZE
            || eSolverOperation == SM_SO_INTERSECT) ;

  // locals
  ULONG ii, jj;
  const SmSurface        *pSurface = pSurfaceOwner ; // ADAPTED (was: (SmSurface *)pSurfaceNode->GetOwnerObject())
  const SmBSplineSurface *pBSS     = SM_CAST_PTR(SmBSplineSurface, pSurface) ;
  SM_ASSERT(pSurface && pSurface->IsKindOf(SmSurface_TYPE)) ;

  // select a grid sample size
  constexpr ULONG lGridSize = 2;

  // set gridSize = 1 for linear geometry
  ULONG lGridSizeU = (pBSS && pBSS->GetNumberControlPoints( SM_SP_U ) == 2) ? 1 : lGridSize ;
  ULONG lGridSizeV = (pBSS && pBSS->GetNumberControlPoints( SM_SP_V ) == 2) ? 1 : lGridSize ;

  // get UV mid-point as 1st guess
  SmPoint2d sClosePtUV = crUVDomain.Evaluate(.5, .5) ;

  // Evaluate MidPoint
  SmPoint2d sUV;
  SmPoint3d sPt;
  SmVector3d sSu, sSv, sSuv, sSuu, sSvv;
  SmStatus sRtn = pSurface->Evaluate1stDerivatives(sClosePtUV, TRUE, TRUE, sPt, sSu, sSv) ;
  if(sRtn != SM_SUCCESS)
    {
      pSurface->EvaluatePoint( sClosePtUV, sPt );
    }

  // get current distance and upcoming NewtonRaphson values
  SmVector3d sSurfToPt = sPt - crTestPnt;
  double     dF        = sSurfToPt.Dot(sSu);
  double     dG        = sSurfToPt.Dot(sSv);

  // save current values as initial best values
  double     dBestDist = sSurfToPt.Length();
  double     dBestFG   = (sRtn == SM_SUCCESS) ? fabs(dF) + fabs(dG) : SM_BIG_DOUBLE ;

  // check each sample point - and check 1st NewtonRaphson step
  //   towards the surface point where the SurfaceNormal intersects
  //   target point.
  // First NewtonRaphson step: want (Su*Sv).(T-S) = 0, so
  //    find du, dv so
  //         0 = Su . (T-S) = F(u+du,v+dv) ~= F + du*Fu + dv*Fv  ==> [Fu Fv][du] = [-F]
  //         0 = Sv . (T-S) = G(u+du,v+dv) ~= G + du*Gu + dv*Gv      [Gu Gv][dv] = [-G]
  //    where S  = S(u,v) a point on the surface,
  //          Su = 1st u derivative
  //          Sv = 1st v derivative
  //          T  = Test point
  //
  //    solve for du,dv yields  [du] =    [ Gv -Fv][-F]
  //                            [dv]      [-Gu  Fu][-G]
  //                                   -------------------
  //                                   (Fu . Gv - Gu . Fv)

  // Make a clamping domain that runs halfway between the outermost
  // sample points and the real domain boundary.
  SmExtent2d sUVExt(crUVDomain.Evaluate(0.5 / ((double)(2*lGridSizeU)),
                                        0.5 / ((double)(2*lGridSizeV))),
                    crUVDomain.Evaluate(1.0 - 0.5 / ((double)(2*lGridSizeU)),
                                        1.0 - 0.5 / ((double)(2*lGridSizeV)))) ;

  // Sample every grid point - Take 1 NewtonRaphson Step from there and keep the best point
  for(ii=0;ii<lGridSizeU;ii++)
    {
      // select evenly spaced interior domain points
      //    exs: GridSize 1: [1/2]
      //         GridSize 2: [1/4 3/4],
      //         GridSize 3: [1/6 3/6 5/6], etc.
      double dPrmU = ((double)(2*ii+1)) / ((double)(2*lGridSizeU)) ;

      for(jj=0;jj<lGridSizeV;jj++)
        {
          double dPrmV = ((double)(2*jj+1)) / ((double)(2*lGridSizeV)) ;

          // skip grid points that don't evaluate to 2nd deriviatives
          sUV = crUVDomain.Evaluate( dPrmU, dPrmV ) ;
          sRtn = pSurface->Evaluate2ndDerivatives(sUV, TRUE, TRUE,
                                                  sPt, sSu, sSv,
                                                  sSuv, sSuu, sSvv) ;  // nonZeroTangents

          // get dist and NewtonRaphson function values from SamplePoint to TestPoint
          sSurfToPt = sPt - crTestPnt;
          dF        = sSurfToPt.Dot(sSu);
          dG        = sSurfToPt.Dot(sSv);
          double dThisDist = sSurfToPt.Length() ;
          double dThisFG   = (sRtn == SM_SUCCESS) ? fabs(dF) + fabs(dG) : SM_BIG_DOUBLE ;

          // Save the best grid point for the current operation
          if(   (eSolverOperation == SM_SO_MINIMIZE  && dThisDist < dBestDist)
             || (eSolverOperation == SM_SO_MAXIMIZE  && dThisDist > dBestDist)
             || (eSolverOperation == SM_SO_NORMALIZE && sRtn     == SM_SUCCESS && dThisFG < dBestFG  )
             || (eSolverOperation == SM_SO_INTERSECT && dThisDist < dBestDist))
            {
              sClosePtUV = sUV ;
              dBestDist  = dThisDist ;
              if(dThisFG < dBestFG) dBestFG = dThisFG ;
            }

          // skip sample points that failed to generate 2nd order derivative values
          if(sRtn != SM_SUCCESS)
            { continue ; }

          // Take 1 clamped Newton-Raphson step towards F=0 and G=0 and check if closer
          double dFu = sSu.Dot(sSu) + sSurfToPt.Dot(sSuu);
          double dFv = sSu.Dot(sSv) + sSurfToPt.Dot(sSuv);
          double dGu = dFv;
          double dGv = sSv.Dot(sSv) + sSurfToPt.Dot(sSvv);

          double dDen = dFu*dGv - dFv*dGu;
          if ( smos_Fabs( dDen ) < SM_EFF_ZERO_SQ )
            {
              continue;  // just ignore if singular.
            }

          double dNumU = dG*dFv  - dGv*dF;
          double dNumV = dF*dGu  - dG*dFu;

          sUV.Set(sUV.x + dNumU/dDen, sUV.y + dNumV/dDen);
          sUV = sUVExt.ClampPoint2d(sUV);

          // evaluate and judge the New Point
          sRtn = pSurface->Evaluate1stDerivatives(sUV, TRUE, TRUE, sPt, sSu, sSv) ;
          if(sRtn != SM_SUCCESS)
            { pSurface->EvaluatePoint( sUV, sPt ); }

          sSurfToPt = sPt - crTestPnt;
          dF        = sSurfToPt.Dot(sSu);
          dG        = sSurfToPt.Dot(sSv);
          dThisDist = sSurfToPt.Length();
          dThisFG   = fabs(dF) + fabs(dG) ;

          // Save the best NewtonRaphson point for the current operation
          if(   (eSolverOperation == SM_SO_MINIMIZE  && dThisDist < dBestDist)
             || (eSolverOperation == SM_SO_MAXIMIZE  && dThisDist > dBestDist)
             || (eSolverOperation == SM_SO_NORMALIZE && sRtn     == SM_SUCCESS && dThisFG < dBestFG  )
             || (eSolverOperation == SM_SO_INTERSECT && dThisDist < dBestDist))
            {
              sClosePtUV = sUV ;
              dBestDist  = dThisDist ;
              if(dThisFG < dBestFG) dBestFG = dThisFG ;
            }
        } // end iter every jj grid index
    } // end iter every ii grid index

  // set output
  sGuesses[0] = sClosePtUV;
  lNumGuesses = 1;

  // all done
  return SM_SUCCESS;

} // end sm_LoadGuesses

// ---- verbatim: SmSurface.cpp lines 4462-4501: sm_PassSPConvergenceTest
static SmBoolean sm_PassSPConvergenceTest
  (const SmSurface & crSurface,      // in : Target Surface
   const SmPoint2d & crUV,           // in : Test SurfacePoint
   const SmPoint3d & crSolvedPoint,  // in : 3D Solution Point
   double d3dTol
  )
{
  SmBoolean bRetVal;

  // Locals for call:
  SmStatus eReason;
  double dDropToSurf, rdApproxDev ;
  SmBoolean bFromLeftU, bFromLeftV ;

  // test crSolvedPoint/SurfaceNormal(crUV) dist against d3dTol
  SmStatus eStat = smsurf_IsPointOnNormal
            (crSurface,              // in : target surface
             NULL,                   // in : clamp domain: if present, clamp crUVToTest to this domain.
                                     //      NULL to ignore
             crUV,                   // in : Surface test 2Dpoint
             crSolvedPoint,          // in : input   test 3DPoint
             NULL,                   // in : associated 3d tangent, NULL to ignore
             bFromLeftU, bFromLeftV, // out: associated from left eval values, NULL to ignore
             d3dTol,                 // in : Max dist allowed between unique points
             bRetVal,                // out: TRUE=TestPoint within tol of SurfaceNormal line, FALSE=not
             eReason,                // out: SM_ERR_OUTSIDE_OF_DOMAIN    = input UVTestPoint is not within domain or
                                     //                                    input UVTestPoint is on surface natural boundary and
                                     //                                        vector from SurfacePoint to TestPoint runs outside of surface domain
                                     //      SM_ERR_BAD_SURFACE_POINT    = Surface.Evaluate() fails - testing impossible)
                                     //      SM_ERR_NOT_WITHIN_TOLERANCE = test run - Point not on normal
                                     //      SM_ERR_UNKNOWN              = test run - Point is on normal
             dDropToSurf,            // out: when rbGoodPoint == TRUE, set to SurfPt/TestPt distSq
                                     //           rbGoodPoint == FALSE, set to 0.0
             rdApproxDev) ;          // out: deviation between 3dPoint and srfNormalLine

  bRetVal &= ( eStat == SM_SUCCESS );

  return bRetVal;

} // end sm_PassSPConvergenceTest

// ---- verbatim (ADAPTED lines marked): SmSurface.cpp lines 4512-4741: sm_SpLocalSolve
static SmStatus sm_SpLocalSolve
  (const SmSurface       * pSrfEvalIn,               // ADAPTED: in : what sm_GetSurfaceToEvaluate(node, patch surface) returns
   const SmExtent2d      & crUVDomain,               // in : Problem domain limit
   SmContinuityType        aeUConts[2],              // in : Patch U dir continuity with its decomposition neighbors
   SmContinuityType        aeVConts[2],              // in : Patch V dir continuity with its decomposition neighbors
   const SmPoint3d       & crPoint,                  // in : Target Starting Point
   double                  d3dTolerance,             // in : when operation == INTERSECT, max allowed point/surface solution distance others are discarded
   SmSolverOperationType & eSolverOperation,         // in : one of SM_SO_MINIMIZE,
                                                     //             SM_SO_MAXIMIZE,
                                                     //             SM_SO_NORMALIZE,
                                                     //             SM_SO_INTERSECT
   SmBoolean               bSolveForSurface,         // in : TRUE = Find SurfacePoint closest/furthest from crPoint
                                                     //      False= skip SurfacePoint Solutions
   SmBoolean               bProcessBoundaryCurves,   // in : TRUE = Find Boundary and C0 discontinuity Curve/crPoint solutions
                                                     //      False= skip CurvePoint Solutions
   SmSolutionArray       & rSolutions)               // out: Any found solutions
{
  // check input
  SM_ASSERT(   eSolverOperation == SM_SO_MINIMIZE
            || eSolverOperation == SM_SO_MAXIMIZE
            || eSolverOperation == SM_SO_INTERSECT
            || eSolverOperation == SM_SO_NORMALIZE);

  // init output
  rSolutions.ReSet();

  // locals
  //const SmSurface       * pSurf    = sm_GetOriginalSurface(*pSurfaceNode);
  //const SmContext       * pContext = pSurf->GetContext();
  SmSolverOperationType   eSO      = eSolverOperation;

  // make a BSplineSurface object from the input bezier patch
  // ADAPTED: (was: const SmSurface &sBSP = *pBezPatch->mBA_pSurface ;)

  // Get exact surface to use in solve loop
  const SmSurface *pSrfEval = pSrfEvalIn; // ADAPTED (was: sm_GetSurfaceToEvaluate(pSurfaceNode, &sBSP))

  // when asked to find Surface/Point solutions
  if ( bSolveForSurface )
    {
      // Find a guess point for LocalPointSolve.
      SmPoint2d sGuesses[10];
      ULONG     lNumGuesses;
      SER( sm_LoadGuesses( pSrfEval, NULL,
                            crUVDomain, crPoint, eSolverOperation,
                            sGuesses, lNumGuesses ));

      // locals for localPointSolve
      SmBoolean bFoundSolution = FALSE;
      SmSolution sSolution;

      // for every guess point
      for (ULONG i=0; i<lNumGuesses; i++)
        {
          // Find NewtonRaphson solution for closest surf point to crPoint to within SM_EFF_ZERO
          // op = MIN/MAX       boundary solutions reported (gap may not be perp to surface)
          // op = XSECT/NORM NO boundary solutions reported (gap always perp to Surface)
          if(SM_SUCCESS != pSrfEval->LocalPointSolve(crUVDomain, eSO, crPoint, sGuesses[i],
                                                     bFoundSolution, sSolution))
            { continue;  // If have an error try another guess
            }

          // skip intersection solutions larger than d3dTolerance
          if(   bFoundSolution
             && eSolverOperation == SM_SO_INTERSECT
             && sSolution.m_vStart.m_dSolutionValue > d3dTolerance)
            {
              bFoundSolution = FALSE;
            }

          // no solution - go to next guess
          if (!bFoundSolution)
            { continue; }

          // When solution is exactly on domain boundary
          if ( crUVDomain.IsPoint2dOnBoundary(
                  SmPoint2d( sSolution.m_vStart[0], sSolution.m_vStart[1] ) ) )
            {
              // skip Solution Points which are not exactly (to within ScaledZero)
              // on the SurfPoint's SurfaceNormal Line
              SmPoint2d sUV(sSolution.m_vStart[0],sSolution.m_vStart[1]);
              if (!sm_PassSPConvergenceTest(*pSrfEval, sUV, crPoint, d3dTolerance))
                { continue; }

            } // end solution on domainBoundary check

#ifdef SM_DEBUG_CODE
SmBoolean bDebugMe = false;
          if (bDebugMe)
            {
              sm_GraphicsLoop();
              smgfx_SetLook(1,2, 0,0,1); crPoint.Draw(); sm_GraphicsLoop();
              smgfx_SetLook(1,2, 0,0,0); sBSP.DrawUV(2,2); sm_GraphicsLoop();
              smgfx_SetLook(2,4, 0,1,1);
              SmPoint3d sFndPnt;
              SmPoint2d sUV(sSolution.m_vStart[0],sSolution.m_vStart[1]);
              pSrfEval->EvaluatePoint(sUV,sFndPnt);
              smgfx_SetLook(4,6, 1,0,0); sFndPnt.Draw(); sm_GraphicsLoop();
              sm_GraphicsLoop();
            }
#endif

          // keep solution - set output
          sSolution.m_lNumVariables = 2;
          rSolutions.Add(sSolution);

          // If intersecting we need only one intersection
          if (eSolverOperation == SM_SO_INTERSECT) return SM_SUCCESS;
          if (eSolverOperation == SM_SO_MINIMIZE)  return SM_SUCCESS;

        } // End iter each guess point
    } // end bSolveForSurface == TRUE check

  // arive here after skipping or adding all surfacePoint solutions

  // when looking for boundaryCurve/Point solutions
  if (   bProcessBoundaryCurves
      && eSolverOperation != SM_SO_NORMALIZE)
    {
      // Iter lower and upper boundaries
      for (ULONG kk=0; kk<2; kk++)
        {
          //    When lower U Boundary C0 or discontinuous
          // or when upper U Boundary discontinuous
          if (   (kk == 0 && aeUConts[kk] <= SM_CT_C0)
              || (kk == 1 && aeUConts[kk] == SM_CT_DISCONTINUOUS))
            {
              // Build boundary isoParameter curve
              double dUParam  ;
              SmBoolean bFromLeft ;
              if (kk==0) { // Doing min domain value
                           dUParam   = crUVDomain.GetMin().x;
                           bFromLeft = TRUE;
                         }
              else       { // Doing max domain value
                           dUParam   = crUVDomain.GetMax().x;
                           bFromLeft = FALSE;
                         }
              SmIsoCurve sIsoCrv(*pSrfEval,SM_SP_U,dUParam,bFromLeft);
              sIsoCrv.SetContext(NULL);

              // for NonDegenerate Boundary Curves
              if (!sIsoCrv.IsDegenerate())
                {
                  SmExtent1d sVIvl(crUVDomain.GetMin().y,crUVDomain.GetMax().y);
                  SmBoolean bFoundSolution;

                  // look for a Curve/Point solution
                  SmSolution sSolution;
                  SER(sIsoCrv.LocalPointSolve(sVIvl, eSO, crPoint,
                                              NULL, NULL, NULL,
                                              sVIvl.Evaluate(0.5),
                                              bFoundSolution, sSolution));

                  // skip out of tolerance intersection solutions
                  if (   bFoundSolution
                      && eSolverOperation == SM_SO_INTERSECT
                      && sSolution.m_vStart.m_dSolutionValue > d3dTolerance)
                    {
                      bFoundSolution = FALSE;
                    }

                  // add solutions to output
                  if (bFoundSolution)
                    {
                      sSolution.m_vStart[1]     = sSolution.m_vStart[0];
                      sSolution.m_vStart[0]     = dUParam;
                      sSolution.m_lNumVariables = 2;
                      rSolutions.Add(sSolution);
                    }
                } // end non degenerate curve check
            } // end appropriate boundary discontinuity check

          //    When lower U Boundary C0 or discontinuous
          // or when upper U Boundary discontinuous
          if (   (kk == 0 && aeVConts[kk] <= SM_CT_C0)
              || (kk == 1 && aeVConts[kk] == SM_CT_DISCONTINUOUS) )
            {
              // Build boundary isoParameter curve
              double dVParam  ;
              SmBoolean bFromLeft ;
              if (kk==0) { // Doing min domain value
                           dVParam   = crUVDomain.GetMin().y;
                           bFromLeft = TRUE;
                         }
              else       { // Doing max domain value
                           dVParam   = crUVDomain.GetMax().y;
                           bFromLeft = FALSE;
                         }
              SmIsoCurve sIsoCrv(*pSrfEval,SM_SP_V,dVParam,bFromLeft);
              sIsoCrv.SetContext(NULL);

              // for NonDegenerate Boundary Curves
              if (!sIsoCrv.IsDegenerate())
                {
                  SmExtent1d sUIvl(crUVDomain.GetMin().x,crUVDomain.GetMax().x);
                  SmBoolean bFoundSolution;

                  // look for a Curve/Point solution
                  SmSolution sSolution;
                  SER(sIsoCrv.LocalPointSolve(sUIvl, eSO, crPoint,
                                              NULL, NULL, NULL,
                                              sUIvl.Evaluate(0.5),
                                              bFoundSolution, sSolution));

                  // skip out of tolerance intersection solutions
                  if(   bFoundSolution
                     && eSolverOperation == SM_SO_INTERSECT
                     && sSolution.m_vStart.m_dSolutionValue > d3dTolerance)
                    {
                      bFoundSolution = FALSE;
                    }

                  // add solutions to output
                  if (bFoundSolution)
                    {
                      // sSolution.m_vStart[0]  = sSolution.m_vStart[0];
                      sSolution.m_vStart[1]     = dVParam;
                      sSolution.m_lNumVariables = 2;
                      rSolutions.Add(sSolution);
                    }
                } // end nonDegenerate Curve check
            } // end appropriate boundary discontinuity check
        } // end iter lower/upper boundaries
    } // when appropriate to look for boundary curve/point solution checks

  return SM_SUCCESS;

} // end sm_SpLocalSolve
