// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.curve_p1`: the SmCurve members owned by the unit
// (source/SMLib/src/SmCurve.cpp, source/SMLib/inc/SmCurve.h) called through `SmCurve*` on
// SmBSplineCurve, SmHermiteCurve and SmLine objects (GlobalPointSolve, which runs the
// file-local SmCPGlobalSolver::LocalSolve over the curve cache, ApproximateCurve,
// TessellateByBisection, Tessellate, ClassifyIntersectingRangeSolution), plus the static
// helpers owned by the unit (sm_TestSpanAccuracy, sm_RefineCCSolutions).
//
// Static functions local to SmCurve.cpp cannot be linked against and the file cannot be
// recompiled inside a namespace (it defines SmCurve members), so the static helpers are
// VERBATIM copies of upstream SmCurve.cpp (commit c9c5979), inside the namespace sc_crv_p1
// (line ranges noted at each block; copied with sed, do not edit by hand).
//
// One entry point, sc_curve_curve_p1_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/curve_curve_p1.rs, which validates every
// input. A curve block is `kind dim ...` (0: SmBSplineCurve `oob n p m cpts(x y z w)
// knots`; 1: SmHermiteCurve `P1 D1 P2 D2`; 2: bounded SmLine `P V d0 d1 scale io`). `out`
// receives the op status (0 = SM_SUCCESS, else the SmStatus value) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveTypes.h>
#include <SmExtent1d.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmSolutionArray.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

#include "sc_sm.h"
#include "sc_gw.h"

using sc::Rd;
using sc::put_vec;
using sc::read_nurb;

namespace sc_crv_p1
{
// clang-format off
// ---- verbatim: SmCurve.cpp lines 81-206: sm_RefineCCSolutions

/*******************************************************************//***
PURPOSE: Local routine to refine the results of a curve/curve intersection.

NOTES: Do a local solve for each solution, using their results as guesses.
   Assumes input is curve/curve intersection.
   If not (e.g., not two variables, and two SmCurve objects), returns SM_ERR.

   This is a bit of a temporary fix because of a problem with Range solutions:
   the ranges returned are often too large, i.e., for true range solutions
   (curves within tolerance over a range), the ranges extend beyond the
   precise intersections, and for tangent and near-miss intersections,
   a finite range is returned.  The solution values are correspondingly bad.
   So this is invoked only for range solutions.
************************************************************************/
static SmStatus sm_RefineCCSolutions( SmSolutionArray &rSolutions )
{
  // Locals
  SmCurve *pCrv1, *pCrv2;
  SmExtent1d sNaturalDom1, sNaturalDom2;
  double dGuess1, dGuess2;
  double dNew1, dNew2, dDev;
  SmBoolean bFound;

  double dDesiredPrecision = 1.0e-10; // for curve/curve intersection

  double dShiftLimit = 0.002;  // Maximum relative parameter shift allowed.

  double dFrac1, dFrac2;  // Relative parameter shifts, as a fraction of the interval size.
  SmExtent1d sSolIvl1, sSolIvl2;
  SmBoolean bReverse1, bReverse2;

  ULONG ii, lNumSols = rSolutions.GetSize();

  for ( ii=0; ii<lNumSols; ii++ )
  {
      SmSolution &rSol = rSolutions[ ii ];

      // Check only range solutions.
      if ( rSol.m_eSolutionType != SM_ST_RANGE_OF_VALUES )
        { continue; }

      if ( rSol.m_lNumVariables != 2 )
        { SER( SM_ERR ); }

      pCrv1 = SM_CAST_PTR( SmCurve, rSol.m_apObjects[0] );  NER( pCrv1 );
      pCrv2 = SM_CAST_PTR( SmCurve, rSol.m_apObjects[1] );  NER( pCrv2 );

      sNaturalDom1 = pCrv1->GetNaturalInterval();
      sNaturalDom2 = pCrv2->GetNaturalInterval();

      // Set up the intervals.
      sSolIvl1.Init();
      sSolIvl1.AddValue( rSol.m_vStart.m_adParameters[0] );
      sSolIvl1.AddValue( rSol.m_vEnd  .m_adParameters[0] );
      bReverse1 = ( rSol.m_vStart.m_adParameters[0] > rSol.m_vEnd.m_adParameters[0] );

      sSolIvl2.Init();
      sSolIvl2.AddValue( rSol.m_vStart.m_adParameters[1] );
      sSolIvl2.AddValue( rSol.m_vEnd  .m_adParameters[1] );
      bReverse2 = ( rSol.m_vStart.m_adParameters[1] > rSol.m_vEnd.m_adParameters[1] );


      // The start:
      if ( rSol.m_vStart.m_dSolutionValue > 0.000001 )
      {
          dGuess1 = rSol.m_vStart[0];
          dGuess2 = rSol.m_vStart[1];

          pCrv1->LocalCurveIntersect( sNaturalDom1,
                              *pCrv2, sNaturalDom2, dDesiredPrecision,
                              dGuess1, dGuess2,
                              bFound,
                              dNew1, dNew2, dDev);

          if ( bFound && dDev < rSol.m_vStart.m_dSolutionValue )
          {
              // Do not allow big shifts.  [140915b]
              sSolIvl1.Inversion( dNew1, dFrac1 );
              sSolIvl2.Inversion( dNew2, dFrac2 );
              if ( bReverse1 ) { dFrac1 = 1.0 - dFrac1; }
              if ( bReverse2 ) { dFrac2 = 1.0 - dFrac2; }

              if ( smos_Fabs( dFrac1 ) < dShiftLimit  &&  smos_Fabs( dFrac2 ) < dShiftLimit )
              {
                  rSol.m_vStart.m_dSolutionValue  = dDev;
                  rSol.m_vStart.m_adParameters[0] = dNew1;
                  rSol.m_vStart.m_adParameters[1] = dNew2;
              }
          }
      } // end Start check.

      // The end:
      if ( rSol.m_vEnd.m_dSolutionValue > 0.000001 )
      {
          dGuess1 = rSol.m_vEnd[0];
          dGuess2 = rSol.m_vEnd[1];

          pCrv1->LocalCurveIntersect( sNaturalDom1,
                              *pCrv2, sNaturalDom2, dDesiredPrecision,
                              dGuess1, dGuess2,
                              bFound,
                              dNew1, dNew2, dDev);

          if ( bFound && dDev < rSol.m_vEnd.m_dSolutionValue )
          {
              // Do not allow big shifts.  [140915b]
              sSolIvl1.Inversion( dNew1, dFrac1 );
              sSolIvl2.Inversion( dNew2, dFrac2 );
              if ( bReverse1 ) { dFrac1 = 1.0 - dFrac1; }
              if ( bReverse2 ) { dFrac2 = 1.0 - dFrac2; }

              if ( smos_Fabs( dFrac1 ) < dShiftLimit  &&  smos_Fabs( dFrac2 ) < dShiftLimit )
              {
                  rSol.m_vEnd.m_dSolutionValue  = dDev;
                  rSol.m_vEnd.m_adParameters[0] = dNew1;
                  rSol.m_vEnd.m_adParameters[1] = dNew2;
              }
          }
      } // end End check.

  } // end for each solution

  return SM_SUCCESS;

} // end static sm_RefineCCSolutions
// ---- end verbatim

// ---- verbatim: SmCurve.cpp lines 228-350: sm_TestSpanAccuracy
/*******************************************************************//***
PURPOSE: return TRUE when HermiteSpan approx distance to a Curve interval
            is less than dApproxTol at a set of sample points

NOTES: Also returns TRUE when the interval gets very short to
   prevent calling function from infinite iterations.
************************************************************************/
static SmStatus sm_TestSpanAccuracy
  (const SmCurve   & crCurve,                 // in : Curve being approximated
   double            dLastT,                  // in : interval beg param
   const SmPoint3d   caLastPV[2],             // in : interval beg pos and tang
   double            dCurrT,                  // in : interval end param
   const SmPoint3d   caCurrPV[2],             // in : interval end pos and tang
   double            dApproxTol,              // in : max allowed dist between curve and approx
   SmBoolean         bMatchParameterization,  // in : TRUE = Check Hermite/Curve distances between common param values
                                              //      FALSE= Check Hermite/Curve distnace with nearest point calculation
   SmBoolean       & rbSpanPassesTest,        // out: TRUE =HermiteSpan is within dApproxTol of input CurveInterval (or very short)
                                              //      FALSE=HermiteSpan Not withing dApproxTol
   double          & rdMaxDistToCrv)          // out: Max dist to Curve when rbSpanPassesTest == TRUE
                                              //      Not specified when     rbSpanPassesTest == FALSE
{
  // init output
  rbSpanPassesTest = FALSE;
  rdMaxDistToCrv   = 0.0;

  // locals
  ULONG ii;
  SmBoolean     bForcePass     = FALSE;
  double        dThisSpan      = dCurrT - dLastT;
  double        dTScale        = (1.0 + smos_Fabs(dCurrT));
  SmExtent1d    sIvl           = crCurve.GetNaturalInterval();
  static constexpr double dMinStep       = 1500;
  ULONG         lNumTestPoints = 3;
  double        dApproxDist;
  SmPoint3d     sHermPnt, sCrvPnt;

  // when span is getting very small - just pass it
  if (dThisSpan < SM_EFF_ZERO * dTScale)
    { bForcePass = TRUE ; }

  // Don't let the interval get too small
  if(dThisSpan < (sIvl.GetLength())/ dMinStep )
    { bForcePass = TRUE ; }

  // hermite cuve between interval end points
  SmHermiteCurve sHermite (caLastPV[0], caLastPV[1]*dThisSpan,
                           caCurrPV[0], caCurrPV[1]*dThisSpan);
  sHermite.SetContext(NULL);

  if ( bForcePass ) // get a better error value if we're going to keep this step.
    { lNumTestPoints = 7; }

  for(ii=0; ii<lNumTestPoints; ii++)
    {
      double dNormalizedParam = ( ii + 1.0 ) / ( lNumTestPoints + 1.0 );
      double dCrvParam        = dLastT + dNormalizedParam*dThisSpan;

      // Evaluate the curve.  Then, if we're matching parameterization,
      // evaluate the Hermite curve at the same (relative) parameter,
      // else find the closest point on the Hermite curve.
      // Note, dropping a point to a Hermite curve is not expensive:
      // it's nice and smooth, so convergence is fast (2 or 3 iters),
      // plus evaluating Hermite's is fast: they store their cubic
      // power-series expansion.
      SER( crCurve.EvaluatePoint( dCrvParam, sCrvPnt ));


      // when matching parameterization
      if ( bMatchParameterization )
        {
          // Approx dist = DistanceBetween(Curve(param), Hermite(param))
          SER( sHermite.EvaluatePoint( dNormalizedParam, sHermPnt ));
          dApproxDist = sHermPnt.DistanceBetween( sCrvPnt );
        }
      else // not matching params
           // - get faster convergence when parameters are allowed to vary a small amount
           // - so, get dApproxDist from DropPoint.
        {
          // Note: can't use DropPoint because it might call GlobalPointSolve,
          //       which uses the cache, which requires SmBSplineCurves,
          //       and neither of these curves is an SmBSplineCurve.
          // Also can't use LocalPointSolve on crCurve, because crCurve might
          //       not be able to calculate two derivs.  HermiteCurve can though.

          SmSolution sSol;
          SmBoolean  bSuccess;
          SmStatus   eStat;
          SmExtent1d sHermiteDomain( 0, 1 );

          // Approx dist = DistanceBetween(Curve(param), NearestPointOnHermite)
          eStat = sHermite.LocalPointSolve(sHermiteDomain,    // in : search interval
                                           SM_SO_MINIMIZE,    // in : Find nearest point on curve
                                           sCrvPnt,           // in : target point
                                           NULL, NULL, NULL,  // in : no maxDistance, DistanceFromCurve, or VectorDirection
                                           dNormalizedParam,  // in : search starting parameter
                                           bSuccess,          // out: TRUE=converged,FALSE=didn't
                                           sSol);             // out: solution container for solver

          // save nearest distance (when drop point failed - load a big distance to quit testing)
          dApproxDist =   (eStat == SM_SUCCESS && bSuccess)
                        ? sSol.m_vStart.m_dSolutionValue
                        : 2 * dApproxTol ;

        } // end bMatchParameterization == FALSE branch

      // When ApproxDist is too big
      if(dApproxDist > dApproxTol  &&  !bForcePass )
        {
          // set output and quit testing
          rbSpanPassesTest = FALSE;
          return SM_SUCCESS;
        }
      rdMaxDistToCrv = smos_Max( rdMaxDistToCrv, dApproxDist );

    } // end iter every test point looking for max ApproxDist

  // arrive here when HermiteCurve is within ApproxTol of Curve at sample points or when bForcePass == TRUE
  rbSpanPassesTest = TRUE;

  // all done
  return SM_SUCCESS;

} // end sm_TestSpanAccuracy
// ---- end verbatim
// clang-format on
} // namespace sc_crv_p1

namespace
{
using namespace sc_crv_p1;

// Ops (keep in sync with crates/solidcore-ffi/src/units/curve_curve_p1.rs).
enum : int32_t
{
    OP_GPS = 0,
    OP_APPROX = 1,
    OP_BISECT = 2,
    OP_TESS = 3,
    OP_CLASSIFY = 4,
    OP_SPAN = 5,
    OP_REFINE = 6,
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

// The transforms `nt (O X Y has_s S)*nt` after a curve block, applied to `c` in order.
void read_transforms(Rd& rd, SmCurve* c)
{
    int64_t nt = rd.idx();
    for (int64_t i = 0; i < nt; i++)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        bool has_s = rd.flag();
        SmVector3d s = rd.vec();
        SmAxis2Placement a2p;
        a2p.SetCanonical(o, x, y);
        c->Transform(a2p, has_s ? &s : NULL);
    }
}

// `nv V[nv]` into `vecs`; returns NULL for nv == 0.
const SmVector3d* read_vecs(Rd& rd, SmVector3d vecs[4])
{
    int64_t nv = rd.idx();
    for (int64_t i = 0; i < nv; i++)
        vecs[i] = rd.vec();
    return nv > 0 ? vecs : NULL;
}

// A one-variable solution `type nvar nobj o0 value s0 [evalue e0]`; objects `0` = a,
// `2` = another object, `-1` = NULL.
void put_sol1(std::vector<double>& o, const SmSolution& s, const void* a)
{
    auto id = [&](const void* p) { return p == NULL ? -1.0 : p == a ? 0.0 : 2.0; };
    o.push_back((double)s.m_eSolutionType);
    o.push_back((double)s.m_lNumVariables);
    o.push_back((double)s.m_lNumObjects);
    o.push_back(s.m_lNumObjects > 0 ? id(s.m_apObjects[0]) : -1.0);
    o.push_back(s.m_vStart.m_dSolutionValue);
    o.push_back(s.m_vStart[0]);
    if (s.m_eSolutionType == SM_ST_RANGE_OF_VALUES)
    {
        o.push_back(s.m_vEnd.m_dSolutionValue);
        o.push_back(s.m_vEnd[0]);
    }
}

// A curve record `type dim has_nurb [n p m cpts(x y z w) knots]`.
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    o.push_back((double)c->GetType());
    o.push_back((double)c->GetDim());
    const SmBSplineCurve* b = c->IsKindOf(SmBSplineCurve_TYPE) ? (const SmBSplineCurve*)c : NULL;
    gw_CURVE* nurb = b ? b->GetGwNurbPointer() : NULL;
    o.push_back(nurb ? 1.0 : 0.0);
    if (!nurb)
        return;
    o.push_back((double)nurb->pol->n);
    o.push_back((double)nurb->p);
    o.push_back((double)nurb->knt->m);
    for (gw_INDEX k = 0; k <= nurb->pol->n; k++)
    {
        o.push_back(nurb->pol->Pw[k].x);
        o.push_back(nurb->pol->Pw[k].y);
        o.push_back(nurb->pol->Pw[k].z);
        o.push_back(nurb->pol->Pw[k].w);
    }
    for (gw_INDEX k = 0; k <= nurb->knt->m; k++)
        o.push_back(nurb->knt->U[k]);
}

void put_tess(std::vector<double>& o, SmStatus s, const SmTArray<double>& t,
              const SmTArray<SmPoint3d>& p, const SmTArray<SmVector3d>& d)
{
    o.push_back(st(s));
    if (s != SM_SUCCESS)
        return;
    o.push_back((double)t.GetSize());
    o.push_back((double)p.GetSize());
    o.push_back((double)d.GetSize());
    for (ULONG i = 0; i < t.GetSize(); i++)
        o.push_back(t[i]);
    for (ULONG i = 0; i < p.GetSize(); i++)
        put_vec(o, p[i]);
    for (ULONG i = 0; i < d.GetSize(); i++)
        put_vec(o, d[i]);
}

void run(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_GPS:
    {
        SmCurve* c = read_curve(rd, ctx);
        read_transforms(rd, c);
        double a = rd.num();
        double b = rd.num();
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx();
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        bool has_tg = rd.flag();
        double tg = rd.num();
        SmVector3d vecs[4];
        const SmVector3d* pv = read_vecs(rd, vecs);
        SmSolutionRequestedType req = (SmSolutionRequestedType)rd.idx();
        SmSolutionArray sols;
        SmStatus s = c->GlobalPointSolve(SmExtent1d(a, b), sop, p, tol, has_tg ? &tg : NULL, pv,
                                         req, sols);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)sols.GetSize());
            for (ULONG i = 0; i < sols.GetSize(); i++)
                put_sol1(o, sols[i], c);
        }
        delete c;
        break;
    }
    case OP_APPROX:
    {
        SmCurve* c = read_curve(rd, ctx);
        int64_t nb = rd.idx();
        SmTArray<double> breaks;
        for (int64_t i = 0; i < nb; i++)
            breaks.Add(rd.num());
        double tol = rd.num();
        bool analytics = rd.flag();
        bool match = rd.flag();
        bool copy = rd.flag();
        double achieved = SM_UNDEF_DOUBLE;
        SmBSplineCurve* r = NULL;
        SmStatus s = c->ApproximateCurve(ctx, breaks, tol, achieved, r, analytics ? TRUE : FALSE,
                                         match ? TRUE : FALSE, copy ? TRUE : FALSE);
        o.push_back(st(s));
        if (s == SM_SUCCESS && r != NULL)
        {
            o.push_back(achieved != SM_UNDEF_DOUBLE ? 1.0 : 0.0);
            o.push_back(achieved != SM_UNDEF_DOUBLE ? achieved : 0.0);
            put_curve(o, r);
        }
        delete r;
        delete c;
        break;
    }
    case OP_BISECT:
    case OP_TESS:
    {
        SmCurve* c = read_curve(rd, ctx);
        double a = rd.num();
        double b = rd.num();
        double chord = rd.num();
        double angle = rd.num();
        double last = rd.num();
        SmTArray<double> t;
        SmTArray<SmPoint3d> p;
        SmTArray<SmVector3d> d;
        SmStatus s = op == OP_BISECT
                         ? c->TessellateByBisection(SmExtent1d(a, b), chord, angle, last, &t, &p, &d)
                         : c->Tessellate(SmExtent1d(a, b), chord, angle, (ULONG)last, &t, &p, &d);
        put_tess(o, s, t, p, d);
        delete c;
        break;
    }
    case OP_CLASSIFY:
    {
        SmCurve* c1 = read_curve(rd, ctx);
        SmCurve* c2 = read_curve(rd, ctx);
        SmSolution sol;
        sol.m_eSolutionType = (SmSolutionType)rd.idx();
        sol.m_lNumVariables = 2;
        sol.m_vStart.m_dSolutionValue = rd.num();
        sol.m_vStart.m_adParameters[0] = rd.num();
        sol.m_vStart.m_adParameters[1] = rd.num();
        sol.m_vEnd.m_dSolutionValue = rd.num();
        sol.m_vEnd.m_adParameters[0] = rd.num();
        sol.m_vEnd.m_adParameters[1] = rd.num();
        SmBoolean degen = FALSE;
        SmSolutionEnd pt;
        pt.m_dSolutionValue = SM_UNDEF_DOUBLE;
        pt.m_adParameters[0] = SM_UNDEF_DOUBLE;
        pt.m_adParameters[1] = SM_UNDEF_DOUBLE;
        SmStatus s = c1->ClassifyIntersectingRangeSolution(*c2, sol, 0.0, degen, pt);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(degen));
            bool written = pt.m_dSolutionValue != SM_UNDEF_DOUBLE;
            o.push_back(written ? 1.0 : 0.0);
            if (written)
            {
                o.push_back(pt.m_dSolutionValue);
                o.push_back(pt.m_adParameters[0]);
                o.push_back(pt.m_adParameters[1]);
            }
        }
        delete c1;
        delete c2;
        break;
    }
    case OP_SPAN:
    {
        SmCurve* c = read_curve(rd, ctx);
        double lt = rd.num();
        SmPoint3d lpv[2];
        lpv[0] = rd.vec();
        lpv[1] = rd.vec();
        double ct = rd.num();
        SmPoint3d cpv[2];
        cpv[0] = rd.vec();
        cpv[1] = rd.vec();
        double tol = rd.num();
        bool match = rd.flag();
        SmBoolean pass = FALSE;
        double dist = 0.0;
        SmStatus s = sm_TestSpanAccuracy(*c, lt, lpv, ct, cpv, tol, match ? TRUE : FALSE, pass, dist);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(pass));
            o.push_back(dist);
        }
        delete c;
        break;
    }
    case OP_REFINE:
    {
        SmCurve* c1 = read_curve(rd, ctx);
        SmCurve* c2 = read_curve(rd, ctx);
        int64_t n = rd.idx();
        SmSolutionArray sols;
        for (int64_t i = 0; i < n; i++)
        {
            SmSolution sol;
            sol.m_eSolutionType = (SmSolutionType)rd.idx();
            sol.m_lNumVariables = (ULONG)rd.idx();
            sol.m_lNumObjects = 2;
            for (int k = 0; k < 2; k++)
            {
                int64_t ob = rd.idx() - 1; // stored +1: 0 = NULL
                sol.m_apObjects[k] = ob == 0 ? (SmObject*)c1 : ob == 1 ? (SmObject*)c2 : NULL;
            }
            sol.m_vStart.m_dSolutionValue = rd.num();
            sol.m_vStart.m_adParameters[0] = rd.num();
            sol.m_vStart.m_adParameters[1] = rd.num();
            sol.m_vEnd.m_dSolutionValue = rd.num();
            sol.m_vEnd.m_adParameters[0] = rd.num();
            sol.m_vEnd.m_adParameters[1] = rd.num();
            sols.Add(sol);
        }
        SmStatus s = sm_RefineCCSolutions(sols);
        o.push_back(st(s));
        o.push_back((double)sols.GetSize());
        for (ULONG i = 0; i < sols.GetSize(); i++)
        {
            o.push_back(sols[i].m_vStart.m_dSolutionValue);
            o.push_back(sols[i].m_vStart.m_adParameters[0]);
            o.push_back(sols[i].m_vStart.m_adParameters[1]);
            o.push_back(sols[i].m_vEnd.m_dSolutionValue);
            o.push_back(sols[i].m_vEnd.m_adParameters[0]);
            o.push_back(sols[i].m_vEnd.m_adParameters[1]);
        }
        delete c1;
        delete c2;
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_curve_curve_p1_eval(int32_t op, const double* in, int64_t n_in, double* out,
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
