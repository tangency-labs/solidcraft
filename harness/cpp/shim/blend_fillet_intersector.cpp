// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_intersector`: SmFilletIntersector
// (source/SMLib/src/SmFilletIntersector.cpp, source/SMLib/inc/SmFilletIntersector.h), run on a
// real SmFilletIntersector (a derived class reaches the protected state) over a real
// SmConstantRadiusAssistedFS over offsets of the two surfaces (IxCra below), plus the file-static helpers sm_FlushCrv and
// sm_FindGuessMidTsectPnt.
//
// Static functions of SmFilletIntersector.cpp cannot be linked against, so they are VERBATIM
// copies of upstream SmFilletIntersector.cpp (commit c9c5979) inside the namespace sc_fi (line
// ranges noted at each block; the SM_DEBUG_CODE block of sm_FlushCrv is left out, as the
// reference build has no SM_DEBUG_CODE).
//
// One entry point, sc_blend_fillet_intersector_run(in, n_in, out, cap, out_len). `in` is a
// program (layout in crates/solidcore-ffi/src/units/blend_fillet_intersector.rs); every read is
// bounds checked here (a bad program is SC_ERR_ARG).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmApiPrimitives.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmContext.h>
#include <SmEdge.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFilletExecutive.h>
#include <SmFilletIntersector.h>
#include <SmFilletSolver.h>
#include <SmFilletStandardSolver.h>
#include <SmNurbsSrf.h>
#include <SmOffsetSurface.h>
#include <SmPlane.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <stdexcept>
#include <vector>

#include "sc_sm.h"

using sc::put_uv;
using sc::put_vec;
using sc::st;

namespace sc_fi
{
// clang-format off
// ---- verbatim: SmFilletIntersector.cpp lines 1022-1072: sm_FlushCrv (SM_DEBUG_CODE block left out)
static SmStatus sm_FlushCrv
 (const SmContext           & crContext,    // in :
  ULONG                       lDimension,   // in :
  SmTArray<SmPoint3d>       & rCntrlPoly,   // in :
  SmTArray<double>          & rKnots,       // in :
  SmTArray<SmBSplineCurve*> & crCurves)     // out:
{
  // check input
  if (rKnots.GetSize() == 0)
    { return SM_SUCCESS; }

  // locals for BSplineCurve
  SmBSplineCurve * pNewBSP = NULL ;
  ULONG            lDegree = 3;
  SmTArray<ULONG> sKnotMult(rKnots.GetSize(), NULL, rKnots.GetSize());
  sKnotMult[0]                     = 4;
  sKnotMult[sKnotMult.GetSize()-1] = 4;
  for(ULONG i=1; i+1<sKnotMult.GetSize(); i++)  // note: can't say sKnotMult.GetSize()-1
    { sKnotMult[i] = 3 ; }

  // build the BSplineCurve
  SER(SmBSplineCurve::CreateCanonical(crContext,
                                      lDimension,
                                      lDegree,
                                      rCntrlPoly,
                                      SM_CF_UNSPECIFIED,
                                      sKnotMult,
                                      rKnots,
                                      SM_KT_UNSPECIFIED,
                                      NULL,
                                      NULL,
                                      pNewBSP));

  // set output
  crCurves.Add(pNewBSP) ;

  // all done
  return SM_SUCCESS ;

} // end sm_FlushCrv

// ---- verbatim: SmFilletIntersector.cpp lines 1081-1119: sm_FindGuessMidTsectPnt
static void sm_FindGuessMidTsectPnt
 (SmTsectPnt * pCurrPnt, // in
  SmTsectPnt * pNextPnt, // in
  double       dF0,      // in: fraction from CurrPt to NextPt
  SmTsectPnt * pMidPnt)  // out
{
  SM_ASSERT(pCurrPnt && pNextPnt && pMidPnt);
  double dF1 = 1 - dF0;

  // position:
  pMidPnt->CrvPos()   = dF1 * pCurrPnt->CrvPos() + dF0 * pNextPnt->CrvPos();

  // 1st deriv:
  pMidPnt->CrvDeriv() =  dF1 * pCurrPnt->CrvDeriv() + dF0 * pNextPnt->CrvDeriv();
  double dLen         =  dF1 * pCurrPnt->CrvDeriv().Length()
                       + dF0 * pNextPnt->CrvDeriv().Length();
  pMidPnt->CrvDeriv().Unitize();
  pMidPnt->CrvDeriv() = pMidPnt->CrvDeriv() * dLen;

  // Surface uv points:
  for (ULONG i=0; i<2; i++)
    {
      pMidPnt->UVPos(i) =   dF1 * pCurrPnt->UVPos(i)
                          + dF0 * pNextPnt->UVPos(i);

      pMidPnt->UVDeriv(i) =   dF1 * pCurrPnt->UVDeriv(i)
                            + dF0 * pNextPnt->UVDeriv(i);
    }

  // curve param:
  pMidPnt->m_dCurveParameter =   dF1 * pCurrPnt->m_dCurveParameter
                               + dF0 * pNextPnt->m_dCurveParameter;

  // deviation: be pessimistic, better safe than sorry.
  // pMidPnt->m_dDeviation = (pCurrPnt->m_dDeviation +
  //     pNextPnt->m_dDeviation) / 2.0;
  pMidPnt->m_dDeviation = smos_Max(pCurrPnt->m_dDeviation, pNextPnt->m_dDeviation );

} // end sm_FindGuessMidTsectPnt
// clang-format on
} // namespace sc_fi

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/blend_fillet_intersector.rs).
enum : int32_t
{
    OP_STATE = 0,
    OP_SPAN = 1,
    OP_NEXT = 2,
    OP_FLUSH = 3,
    OP_MID = 4,
    OP_FLUSH_CRV = 5,
    OP_DELTA = 6,
    OP_VALUES = 7,
    OP_LAW = 8,
    OP_SET_SELF = 9,
    OP_LAST = 9,
};

// A deviation / angle upstream leaves unset.
const double UNSET = -777.0;

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

// A point: `type ptype[4] uv0 duv0 uv1 duv1 srf0[3][3] srf1[3][3] crv_pos crv_deriv param
// angle deviation user[4]` (80 values).
void read_point(Rd& rd, SmTsectPnt& t)
{
    t.m_ePointType = (SmIntersectionPointType)rd.idx(0, 7);
    for (int s = 0; s < 2; s++)
        for (int d = 0; d < 2; d++)
            t.m_eUVParamType[s][d] = (SmExtentPointType)rd.idx(0, 5);
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
void put_bspline(std::vector<double>& o, const SmBSplineCurve* b)
{
    o.push_back((double)b->GetType());
    o.push_back((double)b->GetDim());
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
    SmExtent1d ivl = b->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
}

struct Pts
{
    std::vector<SmTsectPnt*> v;
    ~Pts()
    {
        for (SmTsectPnt* p : v)
            delete p;
    }
    SmTsectPnt* pt(Rd& rd) { return v[(size_t)rd.idx(0, (int64_t)v.size() - 1)]; }
    // `-1` for none.
    SmTsectPnt* opt(Rd& rd)
    {
        int64_t i = rd.idx(-1, (int64_t)v.size() - 1);
        return i < 0 ? NULL : v[(size_t)i];
    }
};

// The intersector with its protected state reachable.
struct Peek : SmFilletIntersector
{
    Peek(const SmSurface& a, const SmExtent2d& da, const SmSurface& b, const SmExtent2d& db,
         SmFilletSolver& fs)
        : SmFilletIntersector(a, da, b, db, fs)
    {
    }

    void set(double ang, double tol, bool clipped, double dir)
    {
        m_dThisAngTolRad = ang;
        m_dThisApproxTol3d = tol;
        m_bClippedByThroughPoint = clipped ? TRUE : FALSE;
        m_dCurveTraceDirection = dir;
    }

    ULONG curve_count() const
    {
        SmTsectPnt* aData[16];
        SmTArray<SmTsectPnt*> a(16, aData);
        m_vCurvePoints.GetAllNodes(a);
        return a.GetSize();
    }

    // `dom1[4] dom2[4] ang_tol approx_tol boundary_test touch self clipped trace_dir
    // kind_of(fillet, adv, 7) offsets[2] curve_count`.
    void put_state(std::vector<double>& o, SmFilletSolver& fs) const
    {
        put_dom(o, m_vUVDomain[0]);
        put_dom(o, m_vUVDomain[1]);
        o.push_back(m_dThisAngTolRad);
        o.push_back(m_dThisApproxTol3d);
        o.push_back(flg(m_bDoBoundaryPointOnCurveTest));
        o.push_back(flg(GetCurrFilletTouchBoundary()));
        o.push_back(flg(GetSelfIntersect()));
        o.push_back(flg(m_bClippedByThroughPoint));
        o.push_back(m_dCurveTraceDirection);
        o.push_back(flg(IsKindOf(SmFilletIntersector_TYPE)));
        o.push_back(flg(IsKindOf(SmAdvSurfaceIntersector_TYPE)));
        o.push_back(flg(IsKindOf(7)));
        for (ULONG i = 0; i < 2; i++)
        {
            SmOffsetSurface* s = fs.GetSurface(i);
            o.push_back(s ? s->GetOffsetDistance() : UNSET);
        }
        o.push_back((double)curve_count());
    }
};

void run_op(int32_t op, Rd& rd, Pts& pts, Peek& si, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_STATE:
        return;
    case OP_SPAN:
    {
        SmTsectPnt* a = pts.pt(rd);
        SmTsectPnt* b = pts.pt(rd);
        SmTsectPnt* mid = pts.opt(rd);
        SmBoolean good = FALSE;
        double dev = UNSET;
        double ang = UNSET;
        SmStatus s = si.TestSpanAccuracy(*a, *b, mid, good, dev, ang);
        o.push_back(st(s));
        if (s != SM_SUCCESS)
            return;
        o.push_back(flg(good));
        o.push_back(dev);
        o.push_back(ang);
        return;
    }
    case OP_NEXT:
    {
        SmTsectPnt* a = pts.pt(rd);
        double step = rd.num();
        SmTsectPnt* b = pts.pt(rd);
        if (a == b)
            throw Bad();
        SmBoolean good = FALSE;
        SmBoolean clipped = FALSE;
        SmBoolean boundary = FALSE;
        double dev = UNSET;
        double ang = UNSET;
        SmStatus s = si.ComputeNextPoint(*a, step, *b, good, dev, ang, clipped, boundary);
        o.push_back(st(s));
        if (s != SM_SUCCESS)
            return;
        o.push_back(flg(good));
        o.push_back(dev);
        o.push_back(ang);
        o.push_back(flg(clipped));
        o.push_back(flg(boundary));
        return;
    }
    case OP_FLUSH:
    {
        // at most one point: past that FlushCurve reaches the solver's executive
        // (m_pExecutive, unset outside an SmFilletExecutive) and its fillet geometry's
        // rails (NULL), both dereferenced
        int64_t n = rd.idx(0, 1);
        for (int64_t i = 0; i < n; i++)
        {
            SmTsectPnt* p = pts.pt(rd);
            if (p->m_pNext != NULL || p->m_pPrev != NULL)
                throw Bad();
            si.AddPointToCurve(*p);
        }
        SmCurve* aC[4];
        SmTArray<SmCurve*> c3d(4, aC), cuv1(4, aC), cuv2(4, aC);
        SmTArray<SmTsectCurveType> types;
        SmTArray<double> devs;
        SmStatus s = si.FlushCurve(c3d, cuv1, cuv2, types, devs);
        o.push_back(st(s));
        if (s != SM_SUCCESS)
            return;
        o.push_back((double)types.GetSize());
        o.push_back((double)devs.GetSize());
        return;
    }
    case OP_MID:
    {
        SmTsectPnt* a = pts.pt(rd);
        SmTsectPnt* b = pts.pt(rd);
        double f = rd.num();
        SmTsectPnt mid;
        sc_fi::sm_FindGuessMidTsectPnt(a, b, f, &mid);
        o.push_back(0.0);
        put_point(o, mid);
        return;
    }
    case OP_FLUSH_CRV:
    {
        ULONG dim = (ULONG)rd.idx(1, 3);
        int64_t nk = rd.idx(0, 64);
        SmTArray<double> knots;
        for (int64_t i = 0; i < nk; i++)
            knots.Add(rd.num());
        int64_t np = rd.idx(0, 256);
        SmTArray<SmPoint3d> poly;
        for (int64_t i = 0; i < np; i++)
            poly.Add(rd.vec());
        SmBSplineCurve* aB[4];
        SmTArray<SmBSplineCurve*> curves(4, aB);
        SmStatus s = sc_fi::sm_FlushCrv(ctx, dim, poly, knots, curves);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back((double)curves.GetSize());
        for (ULONG i = 0; i < curves.GetSize(); i++)
        {
            put_bspline(o, curves[i]);
            delete curves[i];
        }
        return;
    }
    case OP_DELTA:
    {
        SmPoint2d uv[2] = {rd.uv(), rd.uv()};
        SmPoint2d uvd[2] = {rd.uv(), rd.uv()};
        SmTsectPnt* a = pts.pt(rd);
        SmTsectPnt* b = pts.pt(rd);
        if (a == b)
            throw Bad();
        o.push_back(st(si.ComputePointValuesDelta(uv, uvd, *a, *b)));
        return;
    }
    case OP_VALUES:
    {
        SmPoint2d uv[2] = {rd.uv(), rd.uv()};
        SmTsectPnt* a = pts.pt(rd);
        SmTsectPnt* prev = pts.opt(rd);
        if (prev == a)
            throw Bad();
        o.push_back(st(si.ComputePointValues(uv, *a, prev, NULL)));
        return;
    }
    case OP_LAW:
    {
        double param = rd.num();
        SmPoint2d uv0 = rd.uv();
        SmPoint2d uv1 = rd.uv();
        SmPoint3d org = rd.vec();
        SmVector3d nrm = rd.vec();
        SmLinearFilletLaw law(1.0, 1.0);
        SmExtent1d ivl(0.0, 1.0);
        double ori[2] = {1.0, 1.0};
        double cp = UNSET;
        SmPoint2d uvs[2];
        SmStatus s = si.EvaluateLawPoint(param, uv0, uv1, law, FALSE, ivl, ori, org, nrm, cp, uvs);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(cp);
            put_uv(o, uvs[0]);
            put_uv(o, uvs[1]);
        }
        return;
    }
    case OP_SET_SELF:
    {
        si.SetSelfIntersect(rd.flag() ? TRUE : FALSE);
        o.push_back(0.0);
        return;
    }
    default:
        throw Bad();
    }
}
} // namespace

namespace
{
// The solver the intersector runs on: an SmConstantRadiusAssistedFS, whose PointOnPlaneSolve
// override the intersector's EvaluateLawPoint reaches through the virtual. Its only
// constructor takes an edge use: it is built on the first edge of an SmApiCreateBox box, then
// the offset surfaces (offsets of the given base surfaces, allowing extension),
// orientations and radii are replaced as the SmConstantRadiusFS surfaces constructor sets
// them, and the edge uses cleared (as the blend.fillet_standard_solver_p2 shim does).
struct IxCra : SmConstantRadiusAssistedFS
{
    IxCra(const SmContext& ctx, double tol, double ang, double tan, double r0, double r1, SmEdgeuse* eu)
        : SmConstantRadiusAssistedFS(ctx, tol, ang, tan, r0, r1, eu)
    {
    }
    void setup(SmOffsetSurface* s0, SmOffsetSurface* s1, double r0, double r1, double o0, double o1)
    {
        delete m_pSurfaces[0];
        delete m_pSurfaces[1];
        m_pSurfaces[0] = s0;
        m_pSurfaces[1] = s1;
        m_dFilletRadii[0] = r0;
        m_dFilletRadii[1] = r1;
        m_dOrientations[0] = o0;
        m_dOrientations[1] = o1;
        m_pEdgeuses[0] = NULL;
        m_pEdgeuses[1] = NULL;
    }
};
} // namespace

// Program: `surfA surfB` (surfB `2` = the same object as A), `dom1[4] dom2[4]`, the solver
// `r0 r1 orient0 orient1 tol ang_tol tangency_tol`, the intersector settings `ang_tol approx_tol
// clipped_by_through trace_dir`, `npoints points`, then one op `op args`. Output: `op status
// [values]`, the state, then every point.
SC_EXPORT int32_t sc_blend_fillet_intersector_run(const double* in, int64_t n_in, double* out,
                                                  int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 2)
        return SC_ERR_ARG;
    SC_NATIVE_LOCK;
    try
    {
        SmContext ctx;
        std::vector<double> o;
        Rd rd{in, n_in, 0};
        SmSurface* a = read_surface(rd, ctx);
        SmSurface* b = NULL;
        bool same = false;
        {
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
        double r0 = rd.num();
        double r1 = rd.num();
        bool o0 = rd.flag();
        bool o1 = rd.flag();
        double ftol = rd.num();
        double fang = rd.num();
        double ftan = rd.num();
        double ang = rd.num();
        double tol = rd.num();
        bool clipped = rd.flag();
        double dir = rd.num();
        Pts pts;
        int64_t np = rd.idx(0, 16);
        for (int64_t i = 0; i < np; i++)
        {
            SmTsectPnt* t = new SmTsectPnt();
            pts.v.push_back(t);
            read_point(rd, *t);
        }
        SmBrep* box = NULL;
        {
            if (SmApiCreateBox(SmVector3d(0.0, 0.0, 0.0), 1.0, 1.0, 1.0, box) != SM_SUCCESS || box == NULL)
                throw Bad();
            SmTArray<SmEdge*> box_edges;
            box->GetEdges(box_edges);
            SmEdgeuse* box_eu = box_edges.GetSize() == 0 ? NULL : box_edges[0]->GetPrimaryEdgeuse();
            if (box_eu == NULL)
            {
                delete box;
                throw Bad();
            }
            IxCra fs(ctx, ftol, fang, ftan, r0, r1, box_eu);
            {
                double d0 = o0 ? 1.0 : -1.0;
                double d1 = o1 ? 1.0 : -1.0;
                SmOffsetSurface* s0 = new (ctx) SmOffsetSurface(r0 * d0, *a);
                SmOffsetSurface* s1 = new (ctx) SmOffsetSurface(r1 * d1, *b);
                s0->SetAllowExtension(TRUE);
                s1->SetAllowExtension(TRUE);
                fs.setup(s0, s1, r0, r1, d0, d1);
            }
            {
                Peek si(*a, d1, *b, d2, fs);
                si.set(ang, tol, clipped, dir);
                int32_t op = (int32_t)rd.idx(0, OP_LAST);
                o.push_back((double)op);
                run_op(op, rd, pts, si, ctx, o);
                si.put_state(o, fs);
                o.push_back((double)pts.v.size());
                for (SmTsectPnt* t : pts.v)
                    put_point(o, *t);
                // unlink every point before the lists go away
                for (SmTsectPnt* t : pts.v)
                    t->ReSet();
            }
        }
        delete box;
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
