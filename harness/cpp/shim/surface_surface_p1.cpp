// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surface_p1`: the SmSurface members owned by the unit
// (source/SMLib/src/SmSurface.cpp) and SmSTEPSurface (source/SMLib/src/SmSTEPSurface.cpp).
//
// The base-class bodies of virtual members are called qualified (`s->SmSurface::Member(..)`)
// next to the virtual call, so both are compared on every surface kind. The file-local helpers
// of SmSurface.cpp the unit owns (sm_CrossedSeamOrPole, SmIsoCurveLengthIFEO,
// SmFindSingularParamWithGivenTangentDirEFO) are copied here (they are not exported). The
// surface readers are copies of the surface.surface_cache_p3 shim's (another translation unit).
//
// One entry point, sc_surface_surface_p1_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/surface_surface_p1.rs, which validates every
// input.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurveBoundedSurface.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmLocalSolve1d.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmPolarBox.h>
#include <SmPseudoBox.h>
#include <SmSTEPSurface.h>
#include <SmSurface.h>
#include <SmSurfaceCache.h>
#include <SmTArray.h>
#include <SmTol.h>
#include <SmTree.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

#include "sc_sm.h"

using sc::flg;
using sc::put_dom;
using sc::put_uv;
using sc::put_vec;
using sc::st;

// ---- copies of the file-local helpers of SmSurface.cpp ------------------------------------

static SmBoolean sm_CrossedSeamOrPole(SmSurface* pSurf, const SmPoint2d& crPrevUV, const SmPoint2d& crCurrUV,
                                      const SmExtent2d& crDomain)
{
    // First special cases: SmPlane or bilinear.
    if (pSurf->GetType() == SmPlane_TYPE)
    {
        return FALSE;
    }

    SmBSplineSurface* pBSp = SM_CAST_PTR(SmBSplineSurface, pSurf);
    if (pBSp != NULL) // The cast will return Null if nto an SmBSplineSurface.
    {
        if (pBSp->GetDegree(SM_SP_U) == 1 && pBSp->GetDegree(SM_SP_V) == 1)
        {
            return FALSE;
        }
    }

    // Just look for a big jump.
    SmVector2d sStepVec(crCurrUV - crPrevUV);
    constexpr double sdMaxStepRatio = 0.4;

    // Check u and v separately.
    double dDomSizeU = crDomain.GetUInterval().GetLength();
    double dDomSizeV = crDomain.GetVInterval().GetLength();

    if (smos_Fabs(sStepVec.x) > sdMaxStepRatio * dDomSizeU)
    {
        return TRUE;
    }

    if (smos_Fabs(sStepVec.y) > sdMaxStepRatio * dDomSizeV)
    {
        return TRUE;
    }

    return FALSE;
}

namespace sc_ssp1
{
class SmIsoCurveLengthIFEO
{
private:
    const SmSurface& m_crSurface;
    double m_dUIsoValue;

public:
    SmIsoCurveLengthIFEO(const SmSurface& crSurface, double dUIsoValue)
        : m_crSurface(crSurface), m_dUIsoValue(dUIsoValue)
    {
    }

    SmStatus Evaluate(double dT, double& rdResult) const
    {
        SmPoint2d sUVPnt(m_dUIsoValue, dT);
        SmVector3d sPV[2][2];

        // evaluate UVPoint(UIsoValue, dT)
        SER(m_crSurface.Evaluate(sUVPnt, 1, 1, TRUE, TRUE, TRUE, sPV[0], FALSE)); // exact tangent values

        // Compute first fundamental form
        double dE = sPV[1][0].Dot(sPV[1][0]);
        double dF = sPV[1][0].Dot(sPV[0][1]);
        double dG = sPV[0][1].Dot(sPV[0][1]);
        rdResult = smos_Sqrt(dE * dG - dF * dF);
        SmVector3d sNorm = sPV[1][0] * sPV[0][1];
        rdResult = sNorm.Length();

        // all done
        return SM_SUCCESS;
    }
};

class SmFindSingularParamWithGivenTangentDirEFO
{
protected:
    const SmSurface* m_pSurface;
    SmPoint2d& m_rUV;
    SmVector3d& m_rDir;
    SmSurfParamType m_eSingularDirection;
    double m_dDistTol3d;

public:
    SmFindSingularParamWithGivenTangentDirEFO(const SmSurface* pSurface, SmPoint2d& rUV, SmVector3d& rDir,
                                              SmSurfParamType eSingularDirection, double dTol)
        : m_pSurface(pSurface), m_rUV(rUV), m_rDir(rDir), m_eSingularDirection(eSingularDirection),
          m_dDistTol3d(dTol)
    {
    }

    SmStatus Evaluate(double dParam, double& rdFOfT, double& rdFPrimeOfT, SmBoolean& rbFoundAnswer)
    {
        // init output
        rdFOfT = 0.0;
        rdFPrimeOfT = 0.0;
        rbFoundAnswer = FALSE;

        // locals ;
        SmVector2d sUVPoint;
        SmVector3d sMat[4][4];
        SmVector3d *pW, *pWs, *pWss;

        // branch on parametric direction to build local values
        if (m_eSingularDirection == SM_SP_U)
        {
            sUVPoint.x = dParam;
            sUVPoint.y = m_rUV.y;
            pW = &sMat[0][1];
            pWs = &sMat[1][1];
            pWss = &sMat[2][1];
        }
        else
        {
            sUVPoint.x = m_rUV.x;
            sUVPoint.y = dParam;
            pW = &sMat[1][0];
            pWs = &sMat[1][1];
            pWss = &sMat[1][2];
        }

        // evaluate Surface to get Surf derivatives w, w', and w"
        SER(m_pSurface->Evaluate(sUVPoint, 3, 3, TRUE, TRUE, TRUE, sMat[0]));

        // compute Unitized derivitaves of w (using w' and w")
        SmVector3d sUnitVec_t, sUnitVec_tt;
        pW->UnitizedDerivative2(*pWs, *pWss, sUnitVec_t, sUnitVec_tt);

        rdFOfT = sUnitVec_t.Dot(m_rDir);
        rdFPrimeOfT = sUnitVec_tt.Dot(m_rDir);

        // all done
        return (SM_SUCCESS);
    }
};

enum
{
    OP_EVAL = 0,
    OP_PERIODIC = 1,
    OP_DEGEN_POINT = 2,
    OP_DEGEN_CURVE = 3,
    OP_FLAT = 4,
    OP_ON_BOUNDARY = 5,
    OP_IN_DOMAIN = 6,
    OP_MISC = 7,
    OP_SNAP = 8,
    OP_NSECT = 9,
    OP_PARAM_DIR = 10,
    OP_DIR_DERIVS = 11,
    OP_CONVERT = 12,
    OP_SIZE = 13,
    OP_FIND_IVL = 14,
    OP_SING = 15,
    OP_SEAM = 16,
    OP_ISO_LEN = 17,
    OP_STEP = 18,
    OP_CACHE = 19,
    OP_BBOX = 20,
    OP_DOM_CONT = 21,
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
    SmPoint2d uv()
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

void put_box(std::vector<double>& o, const SmExtent3d& e)
{
    put_vec(o, e.GetMin());
    put_vec(o, e.GetMax());
}

// `state basis(3V) D`.
void put_polar(std::vector<double>& o, const SmPolarBox& b)
{
    o.push_back((double)b.GetPolarBoxState());
    for (ULONG i = 0; i < 3; i++)
        put_vec(o, *b.GetBasis(i));
    SmExtent2d d = b.GetDomain();
    put_dom(o, d);
}

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

// `oob form nurb`.
SmBSplineSurface* read_bspline(Rd& rd, const SmContext& ctx)
{
    bool oob = rd.flag();
    SmBSplineSurfaceForm form = (SmBSplineSurfaceForm)rd.idx();
    SmBSplineSurface* s = new (ctx) SmBSplineSurface(read_nurb(rd), FALSE, NULL);
    s->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
    s->SetBSplineSurfaceForm(form);
    return s;
}

// A surface `kind ...`: 0 B-spline `oob form nurb`; 1 bounded plane `o x y sx sy D`;
// 2 curve-bounded surface (implicit outer, no boundary curves) over a B-spline block.
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 1)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double sx = rd.num();
        double sy = rd.num();
        SmExtent2d d = rd.domain();
        return new (ctx) SmPlane(o, x, y, SmVector2d(sx, sy), d, &ctx);
    }
    SmBSplineSurface* b = read_bspline(rd, ctx);
    if (kind == 0)
        return b;
    // the curve-bounded surface owns (deletes) its basis
    SmTArray<SmCurve*> c3d(ctx), cuv(ctx);
    return new (ctx) SmCurveBoundedSurface(*b, &c3d, &cuv, TRUE);
}

// `W S`: the surface, wrapped in an SmSTEPSurface owning it when W = 1.
SmSurface* read_wrapped(Rd& rd, const SmContext& ctx)
{
    bool wrap = rd.flag();
    SmSurface* s = read_surface(rd, ctx);
    if (!wrap)
        return s;
    return new (ctx) SmSTEPSurface(*s, TRUE);
}

// `dim n p m cpts(x y z w) knots`.
SmBSplineCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    ULONG dim = (ULONG)rd.idx();
    gw_INDEX n = (gw_INDEX)rd.idx();
    gw_DEGREE p = (gw_DEGREE)rd.idx();
    gw_INDEX m = (gw_INDEX)rd.idx();
    gw_CURVE* c = sm_AllocateNurbCurve(n, p, m);
    for (gw_INDEX i = 0; i <= n; i++)
    {
        c->pol->Pw[i].x = rd.num();
        c->pol->Pw[i].y = rd.num();
        c->pol->Pw[i].z = rd.num();
        c->pol->Pw[i].w = rd.num();
    }
    for (gw_INDEX i = 0; i <= m; i++)
        c->knt->U[i] = rd.num();
    return new (ctx) SmBSplineCurve(c, dim, FALSE);
}

// The upper half of an `h x h` matrix `D[i][j]`, `i + j <= h`, `i` fastest outside.
void put_upper(std::vector<double>& o, const SmVector3d* m, ULONG h)
{
    for (ULONG i = 0; i <= h; i++)
        for (ULONG j = 0; i + j <= h; j++)
            put_vec(o, m[i * (h + 1) + j]);
}

// `W S uv ul vl`: Evaluate1stDerivatives, Evaluate2ndDerivatives, EvaluateSTEP(2, 2, upper),
// EvaluateSTEPPoint, EvaluateSimple(2, 2, upper); each `status [values]`.
void op_eval(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_wrapped(rd, ctx);
    SmPoint2d uv = rd.uv();
    SmBoolean ul = rd.flag() ? TRUE : FALSE;
    SmBoolean vl = rd.flag() ? TRUE : FALSE;
    {
        SmPoint3d p;
        SmVector3d du, dv;
        SmStatus r = s->Evaluate1stDerivatives(uv, ul, vl, p, du, dv);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            put_vec(o, p);
            put_vec(o, du);
            put_vec(o, dv);
        }
    }
    {
        SmPoint3d p;
        SmVector3d du, dv, duv, duu, dvv;
        SmStatus r = s->Evaluate2ndDerivatives(uv, ul, vl, p, du, dv, duv, duu, dvv);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
        {
            put_vec(o, p);
            put_vec(o, du);
            put_vec(o, dv);
            put_vec(o, duv);
            put_vec(o, duu);
            put_vec(o, dvv);
        }
    }
    {
        SmVector3d m[9];
        SmStatus r = s->EvaluateSTEP(uv, 2, 2, ul, vl, TRUE, m);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
            put_upper(o, m, 2);
    }
    {
        SmPoint3d p;
        SmStatus r = s->EvaluateSTEPPoint(uv, p);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
            put_vec(o, p);
    }
    {
        SmVector3d m[9];
        SmStatus r = s->EvaluateSimple(uv, 2, 2, ul, vl, TRUE, m);
        o.push_back(st(r));
        if (r == SM_SUCCESS)
            put_upper(o, m, 2);
    }
    delete s;
}

// `W S D param`: IsPeriodic.
void op_periodic(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_wrapped(rd, ctx);
    SmExtent2d d = rd.domain();
    SmSurfParamType p = rd.flag() ? SM_SP_V : SM_SP_U;
    o.push_back(0.0);
    o.push_back(flg(s->IsPeriodic(d, p) != FALSE));
    delete s;
}

// `W S tol hasD D`: IsDegeneratePoint, base body then virtual.
void op_degen_point(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_wrapped(rd, ctx);
    double tol = rd.num();
    bool has = rd.flag();
    SmExtent2d d = rd.domain();
    const SmExtent2d* pd = has ? &d : NULL;
    o.push_back(0.0);
    o.push_back(flg(s->SmSurface::IsDegeneratePoint(tol, pd) != FALSE));
    o.push_back(flg(s->IsDegeneratePoint(tol, pd) != FALSE));
    delete s;
}

// `W S ang hasD D tol`: IsDegenerateCurve, base body then virtual.
void op_degen_curve(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_wrapped(rd, ctx);
    double ang = rd.num();
    bool has = rd.flag();
    SmExtent2d d = rd.domain();
    double tol = rd.num();
    const SmExtent2d* pd = has ? &d : NULL;
    o.push_back(0.0);
    o.push_back(flg(s->SmSurface::IsDegenerateCurve(ang, pd, tol) != FALSE));
    o.push_back(flg(s->IsDegenerateCurve(ang, pd, tol) != FALSE));
    delete s;
}

// `W S uv ang`: IsFlatCorner.
void op_flat(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_wrapped(rd, ctx);
    SmPoint2d uv = rd.uv();
    double ang = rd.num();
    o.push_back(0.0);
    o.push_back(flg(s->IsFlatCorner(uv, ang) != FALSE));
    delete s;
}

// `W S D uv hasTol tol`: IsOnBoundary: `0 on side other_u other_v` (0 where not set).
void op_on_boundary(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_wrapped(rd, ctx);
    SmExtent2d d = rd.domain();
    SmPoint2d uv = rd.uv();
    bool has = rd.flag();
    SmTol3d tol(rd.num());
    SmSurfParamType side = SM_SP_UNKNOWN;
    double ou = 0.0, ov = 0.0;
    SmBoolean on = s->IsOnBoundary(d, uv, side, has ? &tol : NULL, &ou, &ov);
    o.push_back(0.0);
    o.push_back(flg(on != FALSE));
    o.push_back((double)side);
    o.push_back(ou);
    o.push_back(ov);
    delete s;
}

// `W S D uv hasTol tol`: IsInDomain.
void op_in_domain(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_wrapped(rd, ctx);
    SmExtent2d d = rd.domain();
    SmPoint2d uv = rd.uv();
    bool has = rd.flag();
    SmTol3d tol(rd.num());
    o.push_back(0.0);
    o.push_back(flg(s->IsInDomain(d, uv, has ? &tol : NULL) != FALSE));
    delete s;
}

// `W S W S`: operator== (virtual and base), IsAnalytic, GetFace() == NULL, GetType of the first.
void op_misc(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* a = read_wrapped(rd, ctx);
    SmSurface* b = read_wrapped(rd, ctx);
    o.push_back(0.0);
    o.push_back(flg((*a == *b) != FALSE));
    o.push_back(flg(a->SmSurface::operator==(*b) != FALSE));
    o.push_back(flg(a->IsAnalytic() != FALSE));
    o.push_back(flg(a->GetFace() == NULL));
    o.push_back((double)a->GetType());
    delete a;
    delete b;
}

// `W S uv tol`: SnapToKnots.
void op_snap(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_wrapped(rd, ctx);
    SmPoint2d uv = rd.uv();
    double tol = rd.num();
    SmPoint2d out;
    SmStatus r = s->SnapToKnots(uv, tol, out);
    o.push_back(st(r));
    if (r == SM_SUCCESS)
        put_uv(o, out);
    delete s;
}

// `E(9 vectors, [i][j] j fastest) dir`: smsurf_EvaluateNormalSection: `status tangent
// curvature E` (the matrix after the call).
void op_nsect(Rd& rd, const SmContext&, std::vector<double>& o)
{
    SmVector3d e[3][3];
    for (int i = 0; i < 3; i++)
        for (int j = 0; j < 3; j++)
            e[i][j] = rd.vec();
    SmVector3d dir = rd.vec();
    SmVector3d t;
    double k = 0.0;
    SmStatus r = smsurf_EvaluateNormalSection(e, dir, t, k);
    o.push_back(st(r));
    if (r == SM_SUCCESS)
    {
        put_vec(o, t);
        o.push_back(k);
        for (int i = 0; i < 3; i++)
            for (int j = 0; j < 3; j++)
                put_vec(o, e[i][j]);
    }
}

// `W S uv vec`: ComputeParamDirection.
void op_param_dir(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_wrapped(rd, ctx);
    SmPoint2d uv = rd.uv();
    SmVector3d v = rd.vec();
    SmVector2d out;
    SmStatus r = s->ComputeParamDirection(uv, v, out);
    o.push_back(st(r));
    if (r == SM_SUCCESS)
        put_uv(o, out);
    delete s;
}

// `W S uv dir h ul vl`: EvaluateDirectionalDerivs: `status [h+1 vectors]`.
void op_dir_derivs(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_wrapped(rd, ctx);
    SmPoint2d uv = rd.uv();
    SmVector2d dir = rd.uv();
    ULONG h = (ULONG)rd.idx();
    SmBoolean ul = rd.flag() ? TRUE : FALSE;
    SmBoolean vl = rd.flag() ? TRUE : FALSE;
    SmVector3d d[8];
    SmStatus r = s->EvaluateDirectionalDerivs(uv, dir, h, d, ul, vl);
    o.push_back(st(r));
    if (r == SM_SUCCESS)
        for (ULONG i = 0; i <= h; i++)
            put_vec(o, d[i]);
    delete s;
}

// `W S D`: ConvertDomainFromSTEPToNURBS then ConvertDomainFromNURBSToSTEP, each `status [D]`.
void op_convert(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_wrapped(rd, ctx);
    SmExtent2d d = rd.domain();
    o.push_back(0.0);
    SmExtent2d a;
    SmStatus r = s->ConvertDomainFromSTEPToNURBS(d, a);
    o.push_back(st(r));
    if (r == SM_SUCCESS)
        put_dom(o, a);
    SmExtent2d b;
    r = s->ConvertDomainFromNURBSToSTEP(d, b);
    o.push_back(st(r));
    if (r == SM_SUCCESS)
        put_dom(o, b);
    delete s;
}

// `W S hasD D`: ApproximateSize.
void op_size(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_wrapped(rd, ctx);
    bool has = rd.flag();
    SmExtent2d d = rd.domain();
    SmVector2d size;
    SmStatus r = s->ApproximateSize(size, has ? &d : NULL);
    o.push_back(st(r));
    if (r == SM_SUCCESS)
        put_uv(o, size);
    delete s;
}

// `C ivl param D`: FindUVCurveIvlInDomain.
void op_find_ivl(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmBSplineCurve* c = read_curve(rd, ctx);
    double a = rd.num();
    double b = rd.num();
    double param = rd.num();
    SmExtent2d d = rd.domain();
    SmExtent1d out;
    SmStatus r = SmSurface::FindUVCurveIvlInDomain(*c, SmExtent1d(a, b), param, d, out);
    o.push_back(st(r));
    if (r == SM_SUCCESS)
    {
        o.push_back(out.GetMin());
        o.push_back(out.GetMax());
    }
    delete c;
}

// `W S uv dir sdir tol s`: SmFindSingularParamWithGivenTangentDirEFO::Evaluate.
void op_sing(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_wrapped(rd, ctx);
    SmPoint2d uv = rd.uv();
    SmVector3d dir = rd.vec();
    SmSurfParamType sdir = rd.flag() ? SM_SP_V : SM_SP_U;
    double tol = rd.num();
    double t = rd.num();
    SmFindSingularParamWithGivenTangentDirEFO efo(s, uv, dir, sdir, tol);
    double f = 0.0, fp = 0.0;
    SmBoolean found = FALSE;
    SmStatus r = efo.Evaluate(t, f, fp, found);
    o.push_back(st(r));
    if (r == SM_SUCCESS)
    {
        o.push_back(f);
        o.push_back(fp);
        o.push_back(flg(found != FALSE));
    }
    delete s;
}

// `W S prev curr D`: sm_CrossedSeamOrPole.
void op_seam(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_wrapped(rd, ctx);
    SmPoint2d a = rd.uv();
    SmPoint2d b = rd.uv();
    SmExtent2d d = rd.domain();
    o.push_back(0.0);
    o.push_back(flg(sm_CrossedSeamOrPole(s, a, b, d) != FALSE));
    delete s;
}

// `W S u t`: SmIsoCurveLengthIFEO::Evaluate.
void op_iso_len(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_wrapped(rd, ctx);
    double u = rd.num();
    double t = rd.num();
    SmIsoCurveLengthIFEO e(*s, u);
    double v = 0.0;
    SmStatus r = e.Evaluate(t, v);
    o.push_back(st(r));
    if (r == SM_SUCCESS)
        o.push_back(v);
    delete s;
}

// `S param hasIvl lo hi`: an SmSTEPSurface over S: GetNaturalUVDomain, GetDomainWithContinuity,
// then GetKnots `status [n knots mults]`.
void op_step(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* base = read_surface(rd, ctx);
    SmSTEPSurface* s = new (ctx) SmSTEPSurface(*base, TRUE);
    SmSurfParamType p = rd.flag() ? SM_SP_V : SM_SP_U;
    bool has = rd.flag();
    double lo = rd.num();
    double hi = rd.num();
    SmExtent1d ivl(lo, hi);
    o.push_back(0.0);
    put_dom(o, s->GetNaturalUVDomain());
    put_dom(o, s->GetDomainWithContinuity(SM_CT_C1, SmPoint2d(0.0, 0.0), SmVector2d(1.0, 0.0)));
    SmTArray<double> knots(ctx);
    SmTArray<ULONG> mults(ctx);
    SmStatus r = s->GetKnots(p, knots, &mults, has ? &ivl : NULL);
    o.push_back(st(r));
    if (r == SM_SUCCESS)
    {
        o.push_back((double)knots.GetSize());
        for (ULONG i = 0; i < knots.GetSize(); i++)
            o.push_back(knots[i]);
        for (ULONG i = 0; i < mults.GetSize(); i++)
            o.push_back((double)mults[i]);
    }
    delete s;
}

// `W S trim`: CacheMakeOrValidate(SM_OC_TRIMSRF or SM_OC_SURFACE, this, NULL, ..): `status [top
// box]`.
void op_cache(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_wrapped(rd, ctx);
    bool trim = rd.flag();
    SmCacheObj* cache = NULL;
    SmStatus r = s->CacheMakeOrValidate(trim ? SM_OC_TRIMSRF : SM_OC_SURFACE, s, NULL, cache);
    o.push_back(st(r));
    if (r == SM_SUCCESS && cache != NULL)
    {
        SmTree* t = ((SmSurfaceCache*)cache)->GetTree();
        if (t != NULL && t->GetTopNode() != NULL)
            put_box(o, t->GetTopNode()->m_sBBox);
    }
    delete s;
}

// `W S D normal pseudo expand polar`: CalculateBoundingBox (virtual), polar box last (`state basis D`): `status [box] [basis(3V)
// intervals(3 lo hi)]`.
void op_bbox(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_wrapped(rd, ctx);
    SmExtent2d d = rd.domain();
    bool want_n = rd.flag();
    bool want_p = rd.flag();
    SmBoolean expand = rd.flag() ? TRUE : FALSE;
    // the FFI asks for a polar box only of B-splines and planes (UB-84)
    bool want_pl = rd.flag();
    SmExtent3d nb;
    SmPseudoBox pb;
    SmPolarBox pl;
    SmStatus r = s->CalculateBoundingBox(d, want_n ? &nb : NULL, want_p ? &pb : NULL, want_pl ? &pl : NULL, NULL,
                                         NULL, expand);
    o.push_back(st(r));
    if (r == SM_SUCCESS)
    {
        if (want_n)
            put_box(o, nb);
        if (want_p)
        {
            for (int i = 0; i < 3; i++)
                put_vec(o, pb.GetBasis((ULONG)i));
            for (int i = 0; i < 3; i++)
            {
                o.push_back(pb.GetInterval(i).GetMin());
                o.push_back(pb.GetInterval(i).GetMax());
            }
        }
        if (want_pl)
            put_polar(o, pl);
    }
    delete s;
}

// `W S cont uv vec`: GetDomainWithContinuity (virtual): `status [D]` (`SM_ERR` without a surface
// cache, where upstream would crash).
void op_dom_cont(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmSurface* s = read_wrapped(rd, ctx);
    SmContinuityType c = (SmContinuityType)rd.idx();
    SmPoint2d uv = rd.uv();
    SmVector2d v = rd.uv();
    // the base body reads the surface cache through a NULL pointer when none can be made
    // (after SE(SM_ERR)): report SM_ERR instead (the built cache is reused by the call)
    if (s->GetType() != SmSTEPSurface_TYPE && smsurf_GetSurfaceCache(s) == NULL)
    {
        o.push_back(st(SM_ERR));
        delete s;
        return;
    }
    o.push_back(0.0);
    put_dom(o, s->GetDomainWithContinuity(c, uv, v));
    delete s;
}
} // namespace sc_ssp1

SC_EXPORT int32_t sc_surface_surface_p1_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                             int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    sc_ssp1::Rd rd{in, 0};
    switch (op)
    {
    case sc_ssp1::OP_EVAL: sc_ssp1::op_eval(rd, ctx, o); break;
    case sc_ssp1::OP_PERIODIC: sc_ssp1::op_periodic(rd, ctx, o); break;
    case sc_ssp1::OP_DEGEN_POINT: sc_ssp1::op_degen_point(rd, ctx, o); break;
    case sc_ssp1::OP_DEGEN_CURVE: sc_ssp1::op_degen_curve(rd, ctx, o); break;
    case sc_ssp1::OP_FLAT: sc_ssp1::op_flat(rd, ctx, o); break;
    case sc_ssp1::OP_ON_BOUNDARY: sc_ssp1::op_on_boundary(rd, ctx, o); break;
    case sc_ssp1::OP_IN_DOMAIN: sc_ssp1::op_in_domain(rd, ctx, o); break;
    case sc_ssp1::OP_MISC: sc_ssp1::op_misc(rd, ctx, o); break;
    case sc_ssp1::OP_SNAP: sc_ssp1::op_snap(rd, ctx, o); break;
    case sc_ssp1::OP_NSECT: sc_ssp1::op_nsect(rd, ctx, o); break;
    case sc_ssp1::OP_PARAM_DIR: sc_ssp1::op_param_dir(rd, ctx, o); break;
    case sc_ssp1::OP_DIR_DERIVS: sc_ssp1::op_dir_derivs(rd, ctx, o); break;
    case sc_ssp1::OP_CONVERT: sc_ssp1::op_convert(rd, ctx, o); break;
    case sc_ssp1::OP_SIZE: sc_ssp1::op_size(rd, ctx, o); break;
    case sc_ssp1::OP_FIND_IVL: sc_ssp1::op_find_ivl(rd, ctx, o); break;
    case sc_ssp1::OP_SING: sc_ssp1::op_sing(rd, ctx, o); break;
    case sc_ssp1::OP_SEAM: sc_ssp1::op_seam(rd, ctx, o); break;
    case sc_ssp1::OP_ISO_LEN: sc_ssp1::op_iso_len(rd, ctx, o); break;
    case sc_ssp1::OP_STEP: sc_ssp1::op_step(rd, ctx, o); break;
    case sc_ssp1::OP_CACHE: sc_ssp1::op_cache(rd, ctx, o); break;
    case sc_ssp1::OP_BBOX: sc_ssp1::op_bbox(rd, ctx, o); break;
    case sc_ssp1::OP_DOM_CONT: sc_ssp1::op_dom_cont(rd, ctx, o); break;
    default: return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
