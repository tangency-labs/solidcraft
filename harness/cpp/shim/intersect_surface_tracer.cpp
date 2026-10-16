// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `intersect.surface_tracer`: the SmSurfaceTracer members owned by the unit
// (source/SMLib/src/SmSurfaceTracer.cpp, source/SMLib/inc/SmSurfaceTracer.h).
//
// SmSurfaceTracer is the base of the drop-curve and silhouette tracers; its curve definition
// (ComputePointValues, LocalPointSolve, DoesPointLieOnCurve, FindSolutionsOnBoundaryCurve) is
// pure virtual in spirit. The shim derives `SectionTracer`, which traces the intersection of the
// surface with the plane {P : n.(P - p0) = 0}; the Rust runner implements the same four hooks
// with the same operations in the same order (crates/difftest/src/units/intersect_surface_tracer.rs),
// so everything else compared is the unit's code.
//
// One entry point, sc_intersect_surface_tracer_run(in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/intersect_surface_tracer.rs, which validates every
// input; reads are bounds checked here too (a bad program is SC_ERR_ARG).
#include "sc_shim.h"

#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmSurfaceTracer.h>
#include <SmTArray.h>
#include <SmTol.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <stdexcept>
#include <vector>

#include "sc_sm.h"

using sc::put_uv;
using sc::put_vec;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/intersect_surface_tracer.rs).
enum : int32_t
{
    OP_STEP = 1,
    OP_TRACE = 2,
    OP_FLUSH = 3,
    OP_ADD_CURVE = 4,
    OP_ON_CURVE = 5,
    OP_ISO = 6,
    OP_START = 7,
    OP_TEST = 8,
    OP_DO_TRACE = 9,
    OP_LAST = 9,
};

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
    SmVector2d uv()
    {
        double x = num();
        double y = num();
        return SmVector2d(x, y);
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

// `type dim ivl[2]` then, for B-splines that are not lines, `has [n p m cpts(x y z w) knots]`.
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    o.push_back((double)c->GetType());
    o.push_back((double)c->GetDim());
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    if (c->IsKindOf(SmLine_TYPE) || !c->IsKindOf(SmBSplineCurve_TYPE))
        return;
    const gw_CURVE* cur = ((const SmBSplineCurve*)c)->GetGwNurbPointer();
    o.push_back(cur ? 1.0 : 0.0);
    if (!cur)
        return;
    gw_INDEX n = cur->pol->n;
    gw_INDEX m = cur->knt->m;
    o.push_back((double)n);
    o.push_back((double)cur->p);
    o.push_back((double)m);
    for (gw_INDEX i = 0; i <= n; i++)
    {
        o.push_back(cur->pol->Pw[i].x);
        o.push_back(cur->pol->Pw[i].y);
        o.push_back(cur->pol->Pw[i].z);
        o.push_back(cur->pol->Pw[i].w);
    }
    for (gw_INDEX i = 0; i <= m; i++)
        o.push_back(cur->knt->U[i]);
}

void put_curves(std::vector<double>& o, const SmTArray<SmCurve*>& a)
{
    o.push_back((double)a.GetSize());
    for (ULONG i = 0; i < a.GetSize(); i++)
        put_curve(o, a[i]);
}

void put_doubles(std::vector<double>& o, const SmTArray<double>& a)
{
    o.push_back((double)a.GetSize());
    for (ULONG i = 0; i < a.GetSize(); i++)
        o.push_back(a[i]);
}

// `n m p q r s cpts(x y z w) u v`, sizes checked by the Rust side.
SmBSplineSurface* read_bspline_surface(Rd& rd, const SmContext& ctx)
{
    gw_INDEX n = (gw_INDEX)rd.idx(1, 16);
    gw_INDEX m = (gw_INDEX)rd.idx(1, 16);
    gw_DEGREE p = (gw_DEGREE)rd.idx(1, 8);
    gw_DEGREE q = (gw_DEGREE)rd.idx(1, 8);
    gw_INDEX r = (gw_INDEX)rd.idx(3, 40);
    gw_INDEX s = (gw_INDEX)rd.idx(3, 40);
    if (p > n || q > m || r != n + p + 1 || s != m + q + 1)
        throw Bad();
    std::vector<double> v;
    for (int64_t i = 0; i < 4 * (n + 1) * (m + 1) + r + 1 + s + 1; i++)
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

// `0 bspline-surface` or `1 o x y scale[2] dom[4]` (SmPlane(o, x, y, scale, dom, ctx)).
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx(0, 1);
    if (kind == 0)
        return read_bspline_surface(rd, ctx);
    SmPoint3d o = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    SmVector2d s = rd.uv();
    SmExtent2d d = rd.dom4();
    return new (ctx) SmPlane(o, x, y, s, d, &ctx);
}

// A curve: `0 a b` (CreateLineSegment, dim 3) or `1 n p m cpts(x y z w) knots` (dim 3).
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
    int64_t n = rd.idx(1, 32);
    int64_t p = rd.idx(1, 8);
    int64_t m = rd.idx(3, 64);
    if (p > n || m != n + p + 1)
        throw Bad();
    std::vector<double> v;
    for (int64_t i = 0; i < 4 * (n + 1) + m + 1; i++)
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
        cur->knt->U[i] = v[4 * (n + 1) + i];
    SmBSplineCurve* c = new (ctx) SmBSplineCurve(3, (ULONG)cur->pol->n, (ULONG)cur->knt->m, (short)cur->p,
                                                 cur->pol->Pw, cur->knt->U, &ctx);
    smos_Free(cur);
    return c;
}

// The test tracer: curves where the surface meets the plane n.(P - p0) = 0. Keep every
// operation in sync with `SectionTracer` in the Rust runner.
class SectionTracer : public SmSurfaceTracer
{
public:
    SmPoint3d m_p0;
    SmVector3d m_n;

    SectionTracer(const SmContext& ctx, const SmSurface* s, const SmExtent2d& d, const SmPoint3d& p0,
                  const SmVector3d& n)
        : SmSurfaceTracer(s, d), m_p0(p0), m_n(n)
    {
        m_cpContext = &ctx;
    }

    // D = [S, Sv, Su, Suv] (Evaluate's 1d layout for (1, 1)).
    SmStatus Eval(const SmPoint2d& uv, SmVector3d D[4]) const
    {
        return m_cpSurface->Evaluate(uv, 1, 1, TRUE, TRUE, FALSE, D);
    }

    SmStatus ComputePointValues(const SmPoint2d& crUV, SmTracePnt& rPnt, SmTracePnt*, double*) override
    {
        SmVector3d D[4];
        SER(Eval(crUV, D));
        SmVector3d sP = D[0];
        SmVector3d sSu = D[2];
        SmVector3d sSv = D[1];
        double dF = m_n.Dot(sP - m_p0);
        double dGu = m_n.Dot(sSu);
        double dGv = m_n.Dot(sSv);
        SmVector2d sT(-dGv * m_dCurveTraceDirection, dGu * m_dCurveTraceDirection);
        double dLen = sT.Length();
        rPnt.m_ePointType = SM_TP_NORMAL;
        if (dLen < 1.0e-9)
        {
            rPnt.m_ePointType = smos_Fabs(dF) < m_dThisApproxTol3d ? SM_TP_TANGENT_POINT : SM_TP_SINGULARITY;
            sT = SmVector2d(0.0, 0.0);
        }
        else
        {
            sT = sT / dLen;
        }
        SmVector3d sT3 = sSu * sT.x + sSv * sT.y;
        SmVector3d sNorm = sSu * sSv;
        double dNLen = sNorm.Length();
        if (dNLen > 0.0)
            sNorm = sNorm / dNLen;
        rPnt.m_vUVCurvePV[0] = crUV;
        rPnt.m_vUVCurvePV[1] = sT;
        rPnt.m_v3DCurvePV[0] = sP;
        rPnt.m_v3DCurvePV[1] = sT3;
        rPnt.m_vSurfacePV[0][0] = sP;
        rPnt.m_vSurfacePV[1][0] = sSu;
        rPnt.m_vSurfacePV[0][1] = sSv;
        rPnt.m_vSurfacePV[2][2] = sNorm;
        rPnt.m_dSurfDropDist = smos_Fabs(dF);
        rPnt.m_dSurfNormLineDist = smos_Fabs(dF) * 0.5;
        return SM_SUCCESS;
    }

    SmStatus LocalPointSolve(const SmTracePnt&, const SmPoint2d& crGuessUV, SmTracePnt&, SmBoolean& rbFoundAnswer,
                             SmPoint2d& rUVFound) const override
    {
        rbFoundAnswer = FALSE;
        SmPoint2d sUV = crGuessUV;
        for (int it = 0; it < 30; it++)
        {
            SmVector3d D[4];
            SER(Eval(sUV, D));
            double dF = m_n.Dot(D[0] - m_p0);
            if (smos_Fabs(dF) < m_dThisApproxTol3d * 1.0e-4)
            {
                rbFoundAnswer = TRUE;
                rUVFound = sUV;
                return SM_SUCCESS;
            }
            double dGu = m_n.Dot(D[2]);
            double dGv = m_n.Dot(D[1]);
            double dG2 = dGu * dGu + dGv * dGv;
            if (dG2 < 1.0e-24)
                return SM_SUCCESS;
            double dR = dF / dG2;
            SmVector2d sStep(dGu * dR, dGv * dR);
            sUV = m_vUVDomain.ClampPoint2d(sUV - sStep);
        }
        return SM_SUCCESS;
    }

    SmBoolean DoesPointLieOnCurve(SmPoint2d& rUV, SmBoolean bRefinePoint) const override
    {
        if (bRefinePoint)
        {
            SmTracePnt sA, sB;
            SmBoolean bFound = FALSE;
            SmPoint2d sR;
            if (LocalPointSolve(sA, rUV, sB, bFound, sR) != SM_SUCCESS)
                return FALSE;
            if (bFound)
                rUV = sR;
        }
        SmVector3d D[4];
        if (Eval(rUV, D) != SM_SUCCESS)
            return FALSE;
        return smos_Fabs(m_n.Dot(D[0] - m_p0)) < m_dThisApproxTol3d ? TRUE : FALSE;
    }

    SmStatus FindSolutionsOnBoundaryCurve(const SmBSplineCurve& crBoundaryCurve, const SmExtent1d& crInterval, ULONG,
                                          SmSurfParamType, SmSolutionArray& rSolutions) const override
    {
        const int N = 24;
        double dTPrev = 0.0, dFPrev = 0.0;
        for (int i = 0; i <= N; i++)
        {
            double dT = crInterval.Evaluate(i / (double)N);
            SmPoint3d sP;
            SER(crBoundaryCurve.EvaluatePoint(dT, sP));
            double dF = m_n.Dot(sP - m_p0);
            if (i > 0 && ((dFPrev <= 0.0 && dF > 0.0) || (dFPrev > 0.0 && dF <= 0.0)))
            {
                double dTT = dTPrev + (dT - dTPrev) * (dFPrev / (dFPrev - dF));
                SmSolution sSol;
                sSol.m_eSolutionType = SM_ST_SINGLE_VALUE;
                sSol.m_lNumVariables = 1;
                sSol.m_vStart[0] = dTT;
                rSolutions.Add(sSol);
            }
            dTPrev = dT;
            dFPrev = dF;
        }
        return SM_SUCCESS;
    }

    // ---- access to the protected base members ----
    SmTracePnt* NewPoint() { return (SmTracePnt*)m_vTrcPntMgr.GetNewElement(); }
    double& ApproxTol() { return m_dThisApproxTol3d; }
    double& AngTol() { return m_dThisAngTolRad; }
    SmTList<SmTracePnt>& CurvePoints() { return m_vCurvePoints; }
    SmTList<SmTracePnt>& StartPoints() { return m_vStartPoints; }
    SmTracePnt*& StartPoint() { return m_pStartPoint; }
    double& Direction() { return m_dCurveTraceDirection; }
    SmBoolean IsClosed() const { return m_bCurveIsClosed; }

    // The body of FindInteriorCurves' loop for one node whose middle is `crMiddle`; returns the
    // stage it stopped at (0 = curve flushed) and the status that stopped it.
    int TraceFrom(const SmPoint2d& crMiddle, SmTArray<SmCurve*>& r3D, SmTArray<SmCurve*>& rUV, SmTArray<double>& rDrop,
                  SmTArray<double>& rDev, SmStatus& rStat)
    {
        rStat = SM_SUCCESS;
        SmTracePnt sPnt1, sPnt2;
        SmBoolean bFound = FALSE;
        SmPoint2d sFoundUV;
        sPnt1.m_v3DCurvePV[1].Set(0, 0, 0);
        rStat = LocalPointSolve(sPnt1, crMiddle, sPnt2, bFound, sFoundUV);
        if (!bFound || rStat != SM_SUCCESS)
            return 1;
        SmPoint3d sSurfPnt;
        rStat = m_cpSurface->EvaluatePoint(sFoundUV, sSurfPnt);
        if (rStat != SM_SUCCESS)
            return 2;
        if (IsPointOnCurve(sSurfPnt, r3D))
            return 3;
        SmTracePnt* pStartTSP = NewPoint();
        pStartTSP->m_ePointType = SM_TP_NORMAL;
        rStat = ComputePointValues(sFoundUV, *pStartTSP, NULL, NULL);
        if (rStat != SM_SUCCESS)
            return 4;
        m_pStartPoint = pStartTSP;
        m_dCurveTraceDirection = 1.0;
        rStat = TraceCurve(*pStartTSP, r3D);
        if (rStat != SM_SUCCESS)
            return 5;
        if (!m_bCurveIsClosed && pStartTSP->m_ePointType != SM_TP_SINGULARITY)
        {
            rStat = ReverseCurveDirection();
            if (rStat != SM_SUCCESS)
            {
                FlushCurve(r3D, rUV, rDrop, rDev);
                return 6;
            }
            m_dCurveTraceDirection = -1.0;
            pStartTSP = m_vCurvePoints.GetLastNode();
            TraceCurve(*pStartTSP, r3D);
            ReverseCurveDirection();
            m_dCurveTraceDirection = 1.0;
        }
        FlushCurve(r3D, rUV, rDrop, rDev);
        m_pStartPoint = NULL;
        return 0;
    }
};

// `type param uv[2] uvtan[2] p[3] t[3] srf[3] nrm[3] drop dev`.
void read_point(Rd& rd, SmTracePnt& p)
{
    p.m_ePointType = (SmTracePointType)rd.idx(0, 4);
    p.m_dCurveParameter = rd.num();
    p.m_vUVCurvePV[0] = rd.uv();
    p.m_vUVCurvePV[1] = rd.uv();
    p.m_v3DCurvePV[0] = rd.vec();
    p.m_v3DCurvePV[1] = rd.vec();
    p.m_vSurfacePV[0][0] = rd.vec();
    p.m_vSurfacePV[2][2] = rd.vec();
    p.m_dSurfDropDist = rd.num();
    p.m_dSurfNormLineDist = rd.num();
}

void put_start_points(std::vector<double>& o, SectionTracer& t)
{
    SmTArray<SmTracePnt*> a;
    t.StartPoints().GetAllNodes(a);
    o.push_back((double)a.GetSize());
    for (ULONG i = 0; i < a.GetSize(); i++)
    {
        o.push_back((double)a[i]->m_ePointType);
        put_uv(o, a[i]->m_vUVCurvePV[0]);
    }
}

struct Acc
{
    SmTArray<SmCurve*> c3;
    SmTArray<SmCurve*> uv;
    SmTArray<double> drop;
    SmTArray<double> dev;
    ~Acc()
    {
        for (ULONG i = 0; i < c3.GetSize(); i++)
            delete c3[i];
        for (ULONG i = 0; i < uv.GetSize(); i++)
            delete uv[i];
    }
};

void run_op(int32_t op, Rd& rd, const SmContext& ctx, SectionTracer& t, Acc& acc, std::vector<double>& o)
{
    switch (op)
    {
    case OP_STEP:
    {
        double d = rd.num();
        SmTracePnt p;
        double r = 0.0;
        SmStatus s = t.ComputeStepSize(p, d, r);
        o.push_back(st(s));
        o.push_back(r);
        break;
    }
    case OP_TRACE:
    {
        SmPoint2d uv = rd.uv();
        SmStatus s = SM_SUCCESS;
        int stage = t.TraceFrom(uv, acc.c3, acc.uv, acc.drop, acc.dev, s);
        o.push_back((double)stage);
        o.push_back(st(s));
        o.push_back(flg(t.IsClosed()));
        o.push_back((double)acc.c3.GetSize());
        break;
    }
    case OP_FLUSH:
    {
        int64_t n = rd.idx(0, 64);
        for (int64_t i = 0; i < n; i++)
        {
            SmTracePnt* p = t.NewPoint();
            read_point(rd, *p);
            t.CurvePoints().Append(p);
        }
        bool has = rd.flag();
        SmCurve* c = has ? read_curve(rd, ctx) : NULL;
        SmStatus s = t.FlushCurve(acc.c3, acc.uv, acc.drop, acc.dev, c);
        delete c;
        o.push_back(st(s));
        o.push_back((double)acc.c3.GetSize());
        break;
    }
    case OP_ADD_CURVE:
        acc.c3.Add(read_curve(rd, ctx));
        o.push_back(0.0);
        break;
    case OP_ON_CURVE:
    {
        SmPoint3d p = rd.vec();
        o.push_back(0.0);
        o.push_back(flg(t.IsPointOnCurve(p, acc.c3)));
        break;
    }
    case OP_ISO:
    {
        SmSurfParamType e = rd.flag() ? SM_SP_V : SM_SP_U;
        double v = rd.num();
        SmBoolean b = FALSE;
        SmStatus s = t.TestIsoCurve(e, v, b);
        o.push_back(st(s));
        o.push_back(flg(b));
        break;
    }
    case OP_START:
    {
        int64_t n = rd.idx(0, 64);
        for (int64_t i = 0; i < n; i++)
        {
            SmTracePnt* p = t.NewPoint();
            p->m_ePointType = (SmTracePointType)rd.idx(0, 4);
            p->m_vUVCurvePV[0] = rd.uv();
            p->m_v3DCurvePV[1] = rd.vec();
            t.AddStartPoint(*p);
        }
        o.push_back(0.0);
        put_start_points(o, t);
        break;
    }
    case OP_TEST:
    {
        SmTracePnt* p = t.NewPoint();
        read_point(rd, *p);
        SmBoolean done = FALSE;
        SmStatus s = t.TestPoint(*p, done);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(done));
            o.push_back(flg(t.IsClosed()));
            put_uv(o, p->m_vUVCurvePV[0]);
            put_vec(o, p->m_v3DCurvePV[0]);
            put_start_points(o, t);
        }
        break;
    }
    case OP_DO_TRACE:
    {
        Acc r;
        SmApproxTol3d tol(t.ApproxTol());
        double ang = t.AngTol();
        SmStatus s = t.DoTrace(ctx, &tol, &ang, &r.c3, &r.uv, &r.drop, &r.dev);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            put_curves(o, r.c3);
            put_curves(o, r.uv);
            put_doubles(o, r.drop);
            put_doubles(o, r.dev);
        }
        break;
    }
    default:
        throw Bad();
    }
}
} // namespace

SC_EXPORT int32_t sc_intersect_surface_tracer_run(const double* in, int64_t n_in, double* out, int64_t cap,
                                                  int64_t* out_len)
{
    if (in == NULL || n_in < 2)
        return SC_ERR_ARG;
    try
    {
        SmContext ctx;
        std::vector<double> o;
        Rd rd{in, n_in, 0};
        SmSurface* srf = read_surface(rd, ctx);
        SmExtent2d dom = rd.dom4();
        SmPoint3d p0 = rd.vec();
        SmVector3d nv = rd.vec();
        bool has_tol = rd.flag();
        double tol = rd.num();
        bool has_ang = rd.flag();
        double ang = rd.num();
        {
            SectionTracer t(ctx, srf, dom, p0, nv);
            o.push_back(t.ApproxTol());
            if (has_tol)
                t.ApproxTol() = tol;
            if (has_ang)
                t.AngTol() = ang;
            Acc acc;
            int64_t n_ops = rd.idx(0, 64);
            for (int64_t i = 0; i < n_ops; i++)
            {
                int32_t op = (int32_t)rd.idx(1, OP_LAST);
                o.push_back((double)op);
                run_op(op, rd, ctx, t, acc, o);
            }
            // final state
            o.push_back(t.ApproxTol());
            o.push_back(t.AngTol());
            o.push_back(t.Direction());
            o.push_back(flg(t.IsClosed()));
            SmTArray<SmTracePnt*> cp;
            t.CurvePoints().GetAllNodes(cp);
            o.push_back((double)cp.GetSize());
            put_start_points(o, t);
            put_curves(o, acc.c3);
            put_curves(o, acc.uv);
            put_doubles(o, acc.drop);
            put_doubles(o, acc.dev);
        }
        delete srf;
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
