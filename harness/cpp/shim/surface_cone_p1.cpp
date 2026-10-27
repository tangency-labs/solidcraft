// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.cone_p1`.
// Wraps the SmCone / SmCylinder members owned by the unit (source/SMLib/src/SmCone.cpp,
// source/SMLib/src/SmCylinder.cpp): the constructors (protected ones through a derived
// class) with MakeNurb, both SmCone::CreateCanonical overloads and
// SmCylinder::CreateCanonical, Copy, operator==, AdjustSTEPUVDomain, GetApex,
// CreateIsoParametricCurve, CreateIsoCircleFromPoint, the STEP/NURB parameter
// conversions, Evaluate, EvaluatePoint, EvaluateSTEP, EvaluateSTEPPoint, GetHeight,
// GetSTEPLinearParamExtent, GetMaxAnalyticDomain, STEPInversion, GetPositivePoint,
// Transform, the inline queries and IsKindOf, plus a verbatim copy of the file-static
// sm_TotalCoincidenceChecker.
//
// One entry point, sc_surface_cone_p1_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/surface_cone_p1.rs, which validates every
// input so upstream only touches valid memory. Statuses are written raw (SM_SUCCESS is
// 1000).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmCircle.h>
#include <SmCone.h>
#include <SmContext.h>
#include <SmDatabaseIO.h>
#include <sstream>
#include <SmCurve.h>
#include <SmCylinder.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmGeomUtility.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmSurfOfRevolution.h>
#include <SmSurface.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <cmath>
#include <vector>

// ---- verbatim copy of file-static upstream code (SmCone.cpp) ---------------------------

/*******************************************************************//**
PURPOSE: Local method to test whether two SmLines or SmCircles are totally coincident.

NOTES:
***********************************************************************/
static
SmStatus sm_TotalCoincidenceChecker(
            const SmCurve *cpThis,
            const SmExtent1d &crThisDomain,
            const SmCurve *cpOther,
            const SmExtent1d &crOtherDomain,
            const SmApproxTol3d sApproxTol3d,
            SmBoolean & rbAreCoinc,    // out
            SmBoolean & rbReversed)    // out
{
  // Init return flags for no coincidence.
  rbAreCoinc = FALSE;
  rbReversed = UNSURE;

  // First try equality operator.
  if ( *cpThis == *cpOther )
  {
      rbAreCoinc = TRUE;
      rbReversed = FALSE;
      return SM_SUCCESS;
  }

  if ( cpThis->GetType() != cpOther->GetType() )
    { return SM_SUCCESS; }

  // Both either SmLine or SmCircle.

  // Using end points, set bRev flag, and quit if not coincident.
  SmBoolean bRev = UNSURE;
  SmPoint3d sPt00, sPt01, sPt10, sPt11;
  cpThis ->EvaluatePoint( crThisDomain .GetMin(), sPt00 );
  cpThis ->EvaluatePoint( crThisDomain .GetMax(), sPt01 );
  cpOther->EvaluatePoint( crOtherDomain.GetMin(), sPt10 );
  cpOther->EvaluatePoint( crOtherDomain.GetMax(), sPt11 );

  double dDist_S_S = sPt00.DistanceBetween( sPt10 );
  double dDist_S_E = sPt00.DistanceBetween( sPt11 );
  double dDist_E_S, dDist_E_E;

  if ( cpThis->IsKindOf( SmLine_TYPE ) )
  {
      dDist_E_E = sPt01.DistanceBetween( sPt11 );
      if ( dDist_S_S < sApproxTol3d && dDist_E_E < sApproxTol3d )
      {
          rbAreCoinc = TRUE;
          rbReversed = FALSE;
          return SM_SUCCESS;
      }
      dDist_E_S = sPt01.DistanceBetween( sPt10 );
      if ( dDist_S_E < sApproxTol3d && dDist_E_S < sApproxTol3d )
      {
          rbAreCoinc = TRUE;
          rbReversed = TRUE;
          return SM_SUCCESS;
      }
      return SM_SUCCESS;
  }

  // Not a line.  If not a circle, return no coincidence.
  if ( ! cpThis->IsKindOf( SmCircle_TYPE ) )
    { return SM_SUCCESS; }

  if ( dDist_S_S < sApproxTol3d && dDist_S_E < sApproxTol3d )
  {
      // Start pt 1 coinc with both start pt2 and end pt2:
      // crv 2 is closed.  Check crv1 closed.
      dDist_E_S = sPt01.DistanceBetween( sPt10 );
      if ( dDist_E_S > sApproxTol3d )
        { return SM_SUCCESS; }

      // Both curves closed, same endpoints.
      // Check 1st derivs for orientation.
      SmVector3d sPV1[2], sPV2[2];
      cpThis ->Evaluate( crThisDomain .GetMin(), 1, TRUE, sPV1, TRUE );
      cpOther->Evaluate( crOtherDomain.GetMin(), 1, TRUE, sPV2, TRUE );
      double dDot = sPV1[1].Dot( sPV2[1] );
      bRev = ( dDot < -SM_EFF_ZERO );
  }
  else if ( dDist_S_S < sApproxTol3d )
  {
      // Start points coincident, start/end not.  Check end points.
      dDist_E_E = sPt01.DistanceBetween( sPt11 );
      if ( dDist_E_E > sApproxTol3d )
        { return SM_SUCCESS; }
      bRev = FALSE;
  }
  else if ( dDist_S_E < sApproxTol3d )
  {
      // Start pt 1 == end pt 2, start pt 2 distinct.
      dDist_E_S = sPt01.DistanceBetween( sPt10 );
      if ( dDist_E_S > sApproxTol3d )
        { return SM_SUCCESS; }
      bRev = TRUE;
  }
  else
  {
      // No endpoint coincidence.
      return SM_SUCCESS;
  }

  // Here, start and end points are coincident, bRev is set.
  // Check interior.  For circles, one interior point is sufficient.
  double dDist;

  cpThis ->EvaluatePoint( crThisDomain .Evaluate( 0.5 ), sPt01 );

  // If reversed, parameterization could vary.
  double dT2 = crOtherDomain.Evaluate( 0.5 );
  if ( ! bRev )
  {
      cpOther->EvaluatePoint( dT2, sPt11 );
      dDist = sPt01.DistanceBetween( sPt11 );
  }
  else
  {
      SmBoolean bOk;
      cpOther->DropPoint( crOtherDomain, sPt01, NULL, sApproxTol3d, &dT2,
                          bOk, dT2, dDist,
                          SM_SO_INTERSECT );
      if ( !bOk ) { dDist = SM_BIG_DOUBLE; }
  }

  if ( dDist > sApproxTol3d )
    { return SM_SUCCESS; }

  // All good: coincident curves.
  rbAreCoinc = TRUE;
  rbReversed = bRev;

  return SM_SUCCESS;

}  // end static sm_TotalCoincidenceChecker

// ---- shim -------------------------------------------------------------------------------

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_cone_p1.rs).
enum : int32_t
{
    OP_BUILD = 0,
    OP_EVAL_STEP = 1,
    OP_EVAL = 2,
    OP_EVAL_POINT = 3,
    OP_TO_STEP = 4,
    OP_TO_NURBS = 5,
    OP_ISO = 6,
    OP_ISO_CIRCLE = 7,
    OP_INVERSION = 8,
    OP_POSITIVE = 9,
    OP_ADJUST = 10,
    OP_TRANSFORM = 11,
    OP_EQ = 12,
    OP_COPY = 13,
    OP_COINCIDENCE = 20,
};

// Construction modes.
enum : int32_t
{
    BM_CANONICAL_SEMI = 0, // CreateCanonical(O, bot, semi, pOptHeight, swap, inside)
    BM_CANONICAL = 1,      // CreateCanonical(O, bot, top, height, swap, inside)
    BM_CYL_CANONICAL = 2,  // SmCylinder::CreateCanonical(O, radius)
    BM_CONE_UNBOUNDED = 3, // SmCone(o, x, y, base, semi, swap, inside) + MakeNurb
    BM_CONE_BOUNDED = 4,   // SmCone(o, x, y, bot, top, start, end, h, swap, inside, mngc) + MakeNurb
    BM_CYL_BOUNDED = 5,    // SmCylinder(o, x, y, r, start, end, h, swap, inside, mngc) + MakeNurb
    BM_CYL_UNBOUNDED = 6,  // SmCylinder(o, x, y, r, swap, inside) + MakeNurb
};

// Curve kinds (coincidence op).
enum : int32_t
{
    CK_LINE = 1,
    CK_CIRCLE = 2,
    CK_ELLIPSE = 3,
};

// Reaches the protected constructors and members.
struct ConePeek : SmCone
{
    static SmBoolean SmSurfOfRevolution::*planar() { return &ConePeek::m_bPlanarGenerator; }
    static SmPolarConversion SmSurfOfRevolution::*polar() { return &ConePeek::m_vPolarConverter; }
    static double SmCone::*base_radius() { return &ConePeek::m_dBaseRadius; }
    static double SmCone::*semi_angle() { return &ConePeek::m_dSemiAngleDeg; }
    static SmBoolean SmSurfOfRevolution::*make_nurb_gen() { return &ConePeek::m_bMakeNurbGenCurve; }
    ConePeek(const SmPoint3d& o, const SmVector3d& x, const SmVector3d& y, double base,
             double semi, SmBoolean swap, SmBoolean inside)
        : SmCone(o, x, y, base, semi, swap, inside)
    {
    }
    ConePeek(const SmPoint3d& o, const SmVector3d& x, const SmVector3d& y, double bot, double top,
             double start, double end, double h, SmBoolean swap, SmBoolean inside,
             SmBoolean mngc)
        : SmCone(o, x, y, bot, top, start, end, h, swap, inside, mngc)
    {
    }
};

struct CylPeek : SmCylinder
{
    CylPeek(const SmPoint3d& o, const SmVector3d& x, const SmVector3d& y, double r,
            SmBoolean swap, SmBoolean inside)
        : SmCylinder(o, x, y, r, swap, inside)
    {
    }
};

struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    bool flag() { return in[at++] != 0.0; }
    SmBoolean b() { return flag() ? TRUE : FALSE; }
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

double flg(bool v)
{
    return v ? 1.0 : 0.0;
}

void put_vec(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

// A surface block `mode oob O[3] X[3] Y[3] p[8]`. Writes the construction status; NULL on
// failure.
SmCone* read_cone(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    int64_t mode = rd.idx();
    bool oob = rd.flag();
    SmPoint3d org = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    double p[8];
    for (int i = 0; i < 8; i++)
        p[i] = rd.num();
    SmAxis2Placement a2p(org, x, y);
    SmCone* c = NULL;
    SmStatus stt = SM_SUCCESS;
    switch (mode)
    {
    case BM_CANONICAL_SEMI:
    {
        double h = p[3];
        stt = SmCone::CreateCanonical(ctx, a2p, p[0], p[1], c, p[2] != 0.0 ? &h : NULL,
                                      p[4] != 0.0 ? TRUE : FALSE, p[5] != 0.0 ? TRUE : FALSE);
        break;
    }
    case BM_CANONICAL:
        stt = SmCone::CreateCanonical(ctx, a2p, p[0], p[1], p[2], c, p[3] != 0.0 ? TRUE : FALSE,
                                      p[4] != 0.0 ? TRUE : FALSE);
        break;
    case BM_CYL_CANONICAL:
    {
        SmCylinder* cy = NULL;
        stt = SmCylinder::CreateCanonical(ctx, a2p, p[0], cy);
        c = cy;
        break;
    }
    case BM_CONE_UNBOUNDED:
        c = new (ctx) ConePeek(org, x, y, p[0], p[1], p[2] != 0.0 ? TRUE : FALSE,
                               p[3] != 0.0 ? TRUE : FALSE);
        stt = c->MakeNurb();
        break;
    case BM_CONE_BOUNDED:
        c = new (ctx) ConePeek(org, x, y, p[0], p[1], p[2], p[3], p[4], p[5] != 0.0 ? TRUE : FALSE,
                               p[6] != 0.0 ? TRUE : FALSE, p[7] != 0.0 ? TRUE : FALSE);
        stt = c->MakeNurb();
        break;
    case BM_CYL_BOUNDED:
        c = new (ctx) SmCylinder(org, x, y, p[0], p[1], p[2], p[3], p[4] != 0.0 ? TRUE : FALSE,
                                 p[5] != 0.0 ? TRUE : FALSE, p[6] != 0.0 ? TRUE : FALSE, &ctx);
        stt = c->MakeNurb();
        break;
    case BM_CYL_UNBOUNDED:
        c = new (ctx) CylPeek(org, x, y, p[0], p[1] != 0.0 ? TRUE : FALSE,
                              p[2] != 0.0 ? TRUE : FALSE);
        stt = c->MakeNurb();
        break;
    default: stt = SC_ERR_ARG; break;
    }
    o.push_back((double)stt);
    if (stt != SM_SUCCESS)
    {
        if (c)
            delete c;
        return NULL;
    }
    c->SetOutOfBoundsEnabled(oob ? TRUE : FALSE); // the surface and its generator
    return c;
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

bool has_polar_curve(const SmCone* s)
{
    const SmPolarConversion& pc = s->*ConePeek::polar();
    return pc.GetCurve() != NULL;
}

// `len chars...` of a string (GetTypeString / GetClassString).
void put_str(std::vector<double>& o, const TCHAR* t)
{
    std::vector<double> c;
    for (const TCHAR* q = t; q && *q; q++)
        c.push_back((double)*q);
    o.push_back((double)c.size());
    o.insert(o.end(), c.begin(), c.end());
}

// The cone state.
void put_state(std::vector<double>& o, const SmCone* s)
{
    o.push_back((double)s->GetType());
    put_str(o, s->GetTypeString());
    put_str(o, s->GetClassString());
    o.push_back(flg(s->IsKindOf(SmCylinder_TYPE)));
    o.push_back(flg(s->IsKindOf(SmCone_TYPE)));
    o.push_back(flg(s->IsKindOf(SmSurfOfRevolution_TYPE)));
    o.push_back(flg(s->IsKindOf(SmBSplineSurface_TYPE)));
    o.push_back(flg(s->IsKindOf(SmSurface_TYPE)));
    o.push_back(flg(s->IsCylinder()));
    o.push_back(s->*ConePeek::base_radius());
    o.push_back(s->*ConePeek::semi_angle());
    o.push_back(flg(s->GetInsideOut()));
    o.push_back(flg(s->GetSwapUV()));
    o.push_back(flg(s->*ConePeek::planar()));
    o.push_back(flg(s->*ConePeek::make_nurb_gen()));
    o.push_back(flg(s->IsBounded()));
    const SmAxis2Placement& pos = s->GetPosition();
    put_vec(o, pos.GetOriginRef());
    put_vec(o, pos.GetXAxisRef());
    put_vec(o, pos.GetYAxisRef());
    SmExtent2d d = s->GetSTEPUVDomain();
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
    o.push_back(s->GetBotRadius());
    o.push_back(s->GetTopRadius());
    o.push_back(s->GetBotHeight());
    o.push_back(s->GetTopHeight());
    o.push_back(s->GetRadius(1.25));
    SmSurfParamType lp = SM_SP_UNKNOWN;
    SmExtent1d le = s->GetSTEPLinearParamExtent(&lp);
    o.push_back(le.GetMin());
    o.push_back(le.GetMax());
    o.push_back((double)lp);
    SmExtent2d mx = s->GetMaxAnalyticDomain();
    o.push_back(mx.GetMin().x);
    o.push_back(mx.GetMin().y);
    o.push_back(mx.GetMax().x);
    o.push_back(mx.GetMax().y);
    SmBoolean cyl = FALSE;
    SmPoint3d apex(0.0, 0.0, 0.0);
    double apex_param = 0.0;
    SmStatus ast = s->GetApex(cyl, apex, apex_param);
    o.push_back((double)ast);
    o.push_back(flg(cyl));
    put_vec(o, apex);
    o.push_back(apex_param);
    SmAxis2Placement ca;
    double cr = 0.0, csa = 0.0;
    SmBoolean cswap = FALSE, cinside = FALSE;
    s->GetCanonical(ca, cr, csa, &cswap, &cinside);
    put_vec(o, ca.GetOriginRef());
    o.push_back(cr);
    o.push_back(csa);
    o.push_back(flg(cswap));
    o.push_back(flg(cinside));
    // Failed evaluations retain the initialized default SmPoint3d sentinels;
    // GetHeight is defined even without a polar curve (both failures give zero).
    o.push_back(1.0);
    o.push_back(s->GetHeight());
    const SmPolarConversion& pc = s->*ConePeek::polar();
    o.push_back(flg(s->HavePolarConversion()));
    ULONG cnt = pc.GetAngleCount();
    o.push_back((double)cnt);
    if (cnt > 0)
    {
        o.push_back(pc.GetFirstAngle());
        o.push_back(pc.GetLastAngle());
    }
    put_curve(o, pc.GetCurve());
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

void put_derivs(std::vector<double>& o, const std::vector<SmVector3d>& d)
{
    for (size_t i = 0; i < d.size(); i++)
        put_vec(o, d[i]);
}

void cone_op(int32_t op, Rd& rd, const SmContext& ctx, SmCone* s, std::vector<double>& o)
{
    SmSurface* common = s; // real native Cone/Cylinder virtual dispatch (issue #76)
    switch (op)
    {
    case OP_BUILD: put_state(o, s); break;
    case 14: // SmSurfaceEval callback over a real analytic object
    {
        const double u = rd.num(), v = rd.num();
        SmSurfaceEval eval(s, u, v);
        o.push_back(SM_SUCCESS);
        put_vec(o, eval.Pos()); put_vec(o, eval.Su()); put_vec(o, eval.Sv());
        put_vec(o, eval.Suu()); put_vec(o, eval.Suv()); put_vec(o, eval.Svv());
        break;
    }
    case 17:
    case 18: // inherited common queries, preserving analytic virtual evaluation
    {
        SmExtent2d domain = common->GetNaturalUVDomain();
        if (op == 18) {
            const SmPoint2d low = domain.Evaluate(0.2, 0.2), high = domain.Evaluate(0.8, 0.8);
            domain.SetMinMax(low, high);
        }
        o.push_back(SM_SUCCESS);
        o.push_back((double)common->IsDegeneratePoint(SM_ZONE_TOL_3D / 10.0, &domain));
        o.push_back((double)common->IsDegenerateCurve(SM_EFF_ZERO_DEG, &domain, SM_ZONE_TOL_3D / 10.0));
        SmVector3d normal(0.0, 0.0, 0.0);
        SmStatus status = common->EvaluateNormal(domain.Evaluate(0.5, 0.5), TRUE, TRUE, normal);
        o.push_back((double)status);
        if (status == SM_SUCCESS) put_vec(o, normal);
        SmExtent3d box;
        status = common->CalculateBoundingBox(domain, &box, NULL, NULL, NULL, NULL, FALSE);
        o.push_back((double)status);
        if (status == SM_SUCCESS) { put_vec(o, box.GetMin()); put_vec(o, box.GetMax()); }
        break;
    }
    case OP_EVAL_STEP:
    {
        double u = rd.num();
        double v = rd.num();
        ULONG hu = (ULONG)rd.idx();
        ULONG hv = (ULONG)rd.idx();
        bool point = rd.flag();
        if (point)
        {
            SmPoint3d p(0.0, 0.0, 0.0);
            SmStatus stt = common->EvaluateSTEPPoint(SmPoint2d(u, v), p);
            o.push_back((double)stt);
            if (stt == SM_SUCCESS)
                put_vec(o, p);
            break;
        }
        std::vector<SmVector3d> d((hu + 1) * (hv + 1), SmVector3d(0.0, 0.0, 0.0));
        SmStatus stt = common->EvaluateSTEP(SmPoint2d(u, v), hu, hv, TRUE, TRUE, TRUE, d.data());
        o.push_back((double)stt);
        if (stt == SM_SUCCESS)
            put_derivs(o, d);
        break;
    }
    case OP_EVAL:
    {
        double u = rd.num();
        double v = rd.num();
        ULONG hu = (ULONG)rd.idx();
        ULONG hv = (ULONG)rd.idx();
        SmBoolean ul = rd.b();
        SmBoolean vl = rd.b();
        SmBoolean upper = rd.b();
        SmBoolean nonzero = rd.b();
        std::vector<SmVector3d> d((hu + 1) * (hv + 1), SmVector3d(0.0, 0.0, 0.0));
        SmStatus stt = common->Evaluate(SmPoint2d(u, v), hu, hv, ul, vl, upper, d.data(), nonzero);
        o.push_back((double)stt);
        if (stt == SM_SUCCESS)
            put_derivs(o, d);
        break;
    }
    case OP_EVAL_POINT:
    {
        double u = rd.num();
        double v = rd.num();
        SmPoint3d p(0.0, 0.0, 0.0);
        SmStatus stt = common->EvaluatePoint(SmPoint2d(u, v), p);
        o.push_back((double)stt);
        if (stt == SM_SUCCESS)
            put_vec(o, p);
        break;
    }
    case OP_TO_STEP:
    {
        double u = rd.num();
        double v = rd.num();
        SmPoint2d r;
        SmStatus stt = common->ConvertUVFromNURBSToSTEP(SmPoint2d(u, v), r);
        o.push_back((double)stt);
        if (stt == SM_SUCCESS)
        {
            o.push_back(r.x);
            o.push_back(r.y);
        }
        break;
    }
    case OP_TO_NURBS:
    {
        double u = rd.num();
        double v = rd.num();
        // ConvertToNURBSParameter dereferences the polar curve unchecked
        if (!has_polar_curve(s))
        {
            o.push_back((double)SC_ERR_ARG);
            break;
        }
        SmPoint2d r;
        SmStatus stt = common->ConvertUVFromSTEPToNURBS(SmPoint2d(u, v), r);
        o.push_back((double)stt);
        if (stt == SM_SUCCESS)
        {
            o.push_back(r.x);
            o.push_back(r.y);
        }
        break;
    }
    case OP_ISO:
    {
        SmSurfParamType param = rd.idx() == 0 ? SM_SP_U : SM_SP_V;
        double iso = rd.num();
        bool has_dom = rd.flag();
        SmExtent2d dom = rd.domain();
        bool want_uv = rd.flag();
        SmBSplineCurve* c = NULL;
        SmCurve* uvc = NULL;
        double gap = -1.0;
        SmStatus stt = s->CreateIsoParametricCurve(ctx, param, iso, 0.0, c,
                                                   has_dom ? &dom : NULL, &gap,
                                                   want_uv ? &uvc : NULL);
        o.push_back((double)stt);
        if (stt == SM_SUCCESS)
        {
            o.push_back(gap);
            put_curve(o, c);
            put_curve(o, uvc);
        }
        delete c;
        delete uvc;
        break;
    }
    case OP_ISO_CIRCLE:
    {
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        // ConvertUVFromSTEPToNURBS dereferences the polar curve unchecked
        if (!has_polar_curve(s))
        {
            o.push_back((double)SC_ERR_ARG);
            break;
        }
        // The STEP inversion CreateIsoCircleFromPoint makes (same computation): when
        // it leaves the parameters unset, upstream reads them uninitialised.
        {
            const SmPoint3d& rOrigin = s->GetPosition().GetOriginRef();
            SmVector3d sZAxis = s->GetPosition().GetZAxis();
            SmExtent1d sHeightStepIvl = s->GetSTEPUVDomain().GetVInterval();
            double dParam = 0.0;
            smgu_LineClosestPoint(rOrigin, sZAxis, p, dParam);
            if (sHeightStepIvl.ContainsValue(dParam, tol))
            {
                double dRadius = s->GetRadius(dParam);
                SmPoint3d sCenter = rOrigin + dParam * sZAxis;
                SmVector3d sVec = p - sCenter;
                if (sVec.LengthSquared() < 2 * SM_EFF_ZERO_SQ)
                    sVec = s->GetPosition().GetXAxis();
                sVec.Unitize();
                SmPoint3d sPointOnSurface = sCenter + dRadius * sVec;
                const double nan = std::nan("");
                SmPoint2d suv(nan, nan);
                SmLocationType eloc = SM_LT_EXTERIOR;
                s->STEPInversion(s->GetSTEPUVDomain(), sPointOnSurface, tol, suv, eloc);
                if (std::isnan(suv.x) || std::isnan(suv.y))
                {
                    o.push_back((double)SC_ERR_ARG);
                    break;
                }
                // ConvertUVFromSTEPToNURBS's status is ignored too: after a failed polar
                // conversion rNURBSUV.y is unset and rNURBSUV.x holds only what the
                // converter wrote; the iso parameter used is .y (.x with swap_uv)
                SmPoint2d nuv(nan, nan);
                if (s->ConvertUVFromSTEPToNURBS(suv, nuv) != SM_SUCCESS &&
                    (!s->GetSwapUV() || std::isnan(nuv.x)))
                {
                    o.push_back((double)SC_ERR_ARG);
                    break;
                }
            }
        }
        SmBSplineCurve* c = NULL;
        SmStatus stt = s->CreateIsoCircleFromPoint(ctx, p, tol, c);
        o.push_back((double)stt);
        if (stt == SM_SUCCESS)
            put_curve(o, c);
        delete c;
        break;
    }
    case OP_INVERSION:
    case 16: // common STEP inversion over an interior analytic subdomain
    {
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        bool has_guess = rd.flag();
        double gu = rd.num();
        double gv = rd.num();
        SmPoint2d guess(gu, gv);
        const double nan = std::nan("");
        SmPoint2d uv(nan, nan);
        SmLocationType loc = SM_LT_EXTERIOR;
        SmExtent2d domain = s->GetSTEPUVDomain();
        if (op == 16) {
            const SmPoint2d low = domain.Evaluate(0.2, 0.2);
            const SmPoint2d high = domain.Evaluate(0.8, 0.8);
            domain.SetMinMax(low, high);
        }
        SmStatus stt = common->STEPInversion(domain, p, tol, uv, loc,
                                             has_guess ? &guess : NULL);
        o.push_back((double)stt);
        // outputs are set on success and for a point outside the sweep (SM_ERR); a
        // failed generator drop leaves the height unset
        if (stt == SM_SUCCESS || !std::isnan(uv.y))
        {
            o.push_back(uv.x);
            o.push_back(uv.y);
            o.push_back((double)loc);
        }
        break;
    }
    case OP_POSITIVE:
    {
        SmPoint3d p = rd.vec();
        SmPoint3d q(0.0, 0.0, 0.0);
        double ap = 0.0;
        SmBoolean neg = s->GetPositivePoint(p, q, ap);
        o.push_back((double)SM_SUCCESS);
        o.push_back(flg(neg));
        put_vec(o, q);
        o.push_back(ap);
        break;
    }
    case OP_ADJUST:
    {
        SmExtent2d d = rd.domain();
        SmStatus stt = s->AdjustSTEPUVDomain(d);
        o.push_back((double)stt);
        if (stt == SM_SUCCESS)
            put_state(o, s);
        break;
    }
    case OP_TRANSFORM:
    {
        SmPoint3d to = rd.vec();
        SmVector3d tx = rd.vec();
        SmVector3d ty = rd.vec();
        bool has_scale = rd.flag();
        SmVector3d sc = rd.vec();
        SmAxis2Placement a2p(to, tx, ty);
        SmStatus stt = s->Transform(a2p, has_scale ? &sc : NULL);
        o.push_back((double)stt);
        if (stt == SM_SUCCESS)
            put_state(o, s);
        break;
    }
    case OP_EQ:
    {
        std::vector<double> o2;
        SmCone* other = read_cone(rd, ctx, o2);
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
        SmSurface* cp = NULL;
        SmStatus stt = s->Copy(ctx, cp);
        o.push_back((double)stt);
        if (stt == SM_SUCCESS && cp)
        {
            o.push_back(flg(*cp == *s));
            put_state(o, (SmCone*)cp);
        }
        delete cp;
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}

// A curve block `kind ...`: a line `a[3] b[3]` (two-point constructor, NURB made), a circle
// `c[3] x[3] y[3] d0 d1 r` or an ellipse `c[3] x[3] y[3] d0 d1 rx ry`.
SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    switch (kind)
    {
    case CK_LINE:
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine* l = new (ctx) SmLine(a, b, 3, &ctx);
        l->GetOrCreateGwNurbPointer();
        return l;
    }
    case CK_CIRCLE:
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double r = rd.num();
        return new (ctx) SmCircle(c, x, y, SmExtent1d(d0, d1), r, 3, &ctx);
    }
    case CK_ELLIPSE:
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
    default: return NULL;
    }
}

void coincidence(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmCurve* a = read_curve(rd, ctx);
    double a0 = rd.num();
    double a1 = rd.num();
    SmCurve* b = read_curve(rd, ctx);
    double b0 = rd.num();
    double b1 = rd.num();
    double tol = rd.num();
    if (!a || !b)
    {
        delete a;
        delete b;
        o.push_back((double)SC_ERR_ARG);
        return;
    }
    SmBoolean coinc = UNSURE;
    SmBoolean rev = UNSURE;
    SmStatus stt =
        sm_TotalCoincidenceChecker(a, SmExtent1d(a0, a1), b, SmExtent1d(b0, b1), tol, coinc, rev);
    o.push_back((double)stt);
    o.push_back((double)coinc);
    o.push_back((double)rev);
    delete a;
    delete b;
}
} // namespace

SC_EXPORT int32_t sc_surface_cone_p1_eval(int32_t op, const double* in, int64_t n_in,
                                          double* out, int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    if (op == OP_COINCIDENCE)
    {
        coincidence(rd, ctx, o);
    }
    else
    {
        SmCone* s = read_cone(rd, ctx, o);
        if (s)
        {
            if (op == 100 || op == 101 || op >= 1000)
            {
                const bool binary = op == 101 || op >= 1100;
                std::ostringstream stream;
                SmDatabaseIOStream writer(binary ? SM_BINARY : SM_ASCII, &stream);
                SmStatus status = s->WriteToDB(writer, 30);
                if (op < 1000) {
                    o.push_back((double)status);
                    if (status == SM_SUCCESS) {
                        const std::string bytes = stream.str();
                        for (unsigned char byte : bytes) o.push_back((double)byte);
                    }
                } else {
                    SmSurface* read = NULL;
                    std::istringstream input(stream.str());
                    SmDatabaseIOStream reader(binary ? SM_BINARY : SM_ASCII, &input);
                    if (status == SM_SUCCESS)
                        status = SmSurface::ReadFromDB(s->GetType(), reader, ctx, read, 30);
                    if (status == SM_SUCCESS && read)
                        cone_op(op - (binary ? 1100 : 1000), rd, ctx, static_cast<SmCone*>(read), o);
                    else o.push_back((double)status);
                    delete read;
                }
            }
            else cone_op(op, rd, ctx, s, o);
            delete s;
        }
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
