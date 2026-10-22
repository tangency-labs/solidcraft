// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.torus`.
// Wraps the SmTorus members owned by the unit (source/SMLib/src/SmTorus.cpp): CreateCanonical
// (the constructor and MakeNurb), operator==, Copy, CreateOffsetSurface,
// ConvertUVFromSTEPToNURBS, ConvertUVFromNURBSToSTEP, EvaluateSTEP, EvaluateSTEPPoint,
// CreateIsoParametricCurve, RebuildSTEPFromNURBParameters, Transform, GetMaxAnalyticDomain,
// GetInsideOut and IsKindOf; and the inherited SmSurfOfRevolution::TrimWithDomain.
//
// One entry point, sc_surface_torus_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/surface_torus.rs, which validates every input
// so upstream only touches valid memory. `out` starts with the status of CreateCanonical.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmSurfOfRevolution.h>
#include <SmSurface.h>
#include <SmTorus.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

#include "sc_sm.h"

using sc::flg;
using sc::put_vec;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_torus.rs).
enum : int32_t
{
    OP_BUILD = 0,
    OP_EVAL_STEP = 1,
    OP_TO_STEP = 2,
    OP_TO_NURBS = 3,
    OP_ISO = 4,
    OP_OFFSET = 5,
    OP_REBUILD = 6,
    OP_TRANSFORM = 7,
    OP_EQ = 8,
    OP_COPY = 9,
    OP_TRIM_DOMAIN = 10,
};

// Reads the protected members through pointers to members named in a derived class.
struct Peek : SmTorus
{
    static SmBoolean SmSurfOfRevolution::*planar() { return &Peek::m_bPlanarGenerator; }
    static SmPolarConversion SmSurfOfRevolution::*polar() { return &Peek::m_vPolarConverter; }
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
    SmExtent2d domain()
    {
        double a = num();
        double b = num();
        double c = num();
        double d = num();
        return SmExtent2d(a, b, c, d);
    }
};

// A torus block `O[3] X[3] Y[3] major minor swap inside_out has_dom D[4] has_circle
// [c[3] x[3] y[3] d0 d1 r]`: SmTorus::CreateCanonical(SmAxis2Placement(O, X, Y), ..).
// Writes the status; NULL on failure.
SmTorus* read_torus(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmPoint3d org = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    double major = rd.num();
    double minor = rd.num();
    bool swap = rd.flag();
    bool inside = rd.flag();
    bool has_dom = rd.flag();
    SmExtent2d dom = rd.domain();
    bool has_circle = rd.flag();
    SmCircle* circle = NULL;
    if (has_circle)
    {
        SmPoint3d c = rd.vec();
        SmVector3d cx = rd.vec();
        SmVector3d cy = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double r = rd.num();
        circle = new (ctx) SmCircle(c, cx, cy, SmExtent1d(d0, d1), r, 3, &ctx);
    }
    else
    {
        for (int i = 0; i < 12; i++)
            rd.num();
    }
    SmTorus* t = NULL;
    SmStatus stt = SmTorus::CreateCanonical(ctx, SmAxis2Placement(org, x, y), major, minor, t,
                                            swap ? TRUE : FALSE, inside ? TRUE : FALSE, circle,
                                            has_dom ? &dom : NULL);
    o.push_back(st(stt));
    if (stt != SM_SUCCESS || t == NULL)
    {
        // the torus owns the circle once constructed (and deletes it on failure); before
        // that (a degenerate radius) nobody does
        if (t == NULL && circle != NULL && !(minor > SM_EFF_ZERO && major > SM_EFF_ZERO))
            delete circle;
        delete t;
        return NULL;
    }
    return t;
}

// `has [type step0 step1 has_nurb [p n cpts(4 n) nk knots]] [ellipse O X Y rx ry inside]`.
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
    if (cur)
    {
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
    const SmEllipse* e = c->IsKindOf(SmEllipse_TYPE) ? (const SmEllipse*)c : NULL;
    o.push_back(flg(e != NULL));
    if (e)
    {
        const SmAxis2Placement& p = e->GetPosition();
        put_vec(o, p.GetOriginRef());
        put_vec(o, p.GetXAxisRef());
        put_vec(o, p.GetYAxisRef());
        o.push_back(e->GetXRadius());
        o.push_back(e->GetYRadius());
        o.push_back(flg(e->GetInsideOut()));
    }
}

// The torus state.
void put_state(std::vector<double>& o, const SmTorus* s)
{
    o.push_back(flg(s->IsKindOf(SmTorus_TYPE)));
    o.push_back(flg(s->IsKindOf(SmSurfOfRevolution_TYPE)));
    o.push_back(flg(s->IsKindOf(SmBSplineSurface_TYPE)));
    o.push_back(flg(s->IsKindOf(SmSurface_TYPE)));
    o.push_back((double)s->GetType());
    o.push_back(flg(s->GetSwapUV()));
    o.push_back(flg(s->*Peek::planar()));
    o.push_back(flg(s->GetInsideOut()));
    o.push_back(s->GetMajorRadius());
    o.push_back(s->GetMinorRadius());
    const SmAxis2Placement& pos = s->GetPosition();
    put_vec(o, pos.GetOriginRef());
    put_vec(o, pos.GetXAxisRef());
    put_vec(o, pos.GetYAxisRef());
    SmExtent2d d = s->GetSTEPUVDomain();
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
    SmExtent2d mx = s->GetMaxAnalyticDomain();
    o.push_back(mx.GetMin().x);
    o.push_back(mx.GetMin().y);
    o.push_back(mx.GetMax().x);
    o.push_back(mx.GetMax().y);
    const SmPolarConversion& pc = s->*Peek::polar();
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

void torus_op(int32_t op, Rd& rd, const SmContext& ctx, SmTorus* s, std::vector<double>& o)
{
    switch (op)
    {
    case OP_BUILD: put_state(o, s); break;
    case OP_EVAL_STEP:
    {
        double u = rd.num();
        double v = rd.num();
        ULONG k = (ULONG)rd.idx();
        bool point = rd.flag();
        if (point)
        {
            SmPoint3d p(0.0, 0.0, 0.0);
            SmStatus stt = s->EvaluateSTEPPoint(SmPoint2d(u, v), p);
            o.push_back(st(stt));
            if (stt == SM_SUCCESS)
                put_vec(o, p);
            break;
        }
        std::vector<SmVector3d> d((k + 1) * (k + 1), SmVector3d(0.0, 0.0, 0.0));
        SmStatus stt = s->EvaluateSTEP(SmPoint2d(u, v), k, k, TRUE, TRUE, TRUE, d.data());
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            for (size_t i = 0; i < d.size(); i++)
                put_vec(o, d[i]);
        break;
    }
    case OP_TO_STEP:
    case OP_TO_NURBS:
    {
        double u = rd.num();
        double v = rd.num();
        SmPoint2d r(0.0, 0.0);
        SmStatus stt = op == OP_TO_STEP ? s->ConvertUVFromNURBSToSTEP(SmPoint2d(u, v), r)
                                        : s->ConvertUVFromSTEPToNURBS(SmPoint2d(u, v), r);
        o.push_back(st(stt));
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
        // the override ignores the status of the base extraction and then reads its
        // curve: reject the arguments it fails on
        SmBSplineCurve* probe = NULL;
        SmStatus pst = s->SmBSplineSurface::CreateIsoParametricCurve(
            ctx, param, iso, 0.0, probe, has_dom ? &dom : NULL, NULL, NULL);
        bool ok = pst == SM_SUCCESS && probe != NULL;
        delete probe;
        if (!ok)
        {
            o.push_back((double)SC_ERR_ARG);
            break;
        }
        SmBSplineCurve* c = NULL;
        double gap = -1.0;
        SmStatus stt = s->CreateIsoParametricCurve(ctx, param, iso, 0.0, c,
                                                   has_dom ? &dom : NULL, &gap, NULL);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(gap);
            put_curve(o, c);
        }
        delete c;
        break;
    }
    case OP_OFFSET:
    {
        double dist = rd.num();
        double tol = rd.num();
        bool rebuild = rd.flag();
        SmSurface* off = NULL;
        SmStatus stt = s->CreateOffsetSurface(ctx, dist, tol, off);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(flg(off != NULL));
            if (off)
            {
                SmTorus* t = (SmTorus*)off;
                if (rebuild)
                    o.push_back(st(t->RebuildSTEPFromNURBParameters()));
                put_state(o, t);
            }
        }
        delete off;
        break;
    }
    case OP_REBUILD:
    {
        SmStatus stt = s->RebuildSTEPFromNURBParameters();
        o.push_back(st(stt));
        put_state(o, s);
        break;
    }
    case OP_TRANSFORM:
    {
        SmPoint3d org = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        bool has_scale = rd.flag();
        SmVector3d sc = rd.vec();
        SmAxis2Placement a2p(org, x, y);
        SmStatus stt = s->Transform(a2p, has_scale ? &sc : NULL);
        o.push_back(st(stt));
        put_state(o, s);
        break;
    }
    case OP_EQ:
    {
        std::vector<double> o2;
        SmTorus* other = read_torus(rd, ctx, o2);
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
        SmSurface* c = NULL;
        SmStatus stt = s->Copy(ctx, c);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS && c)
        {
            put_state(o, (SmTorus*)c);
            o.push_back(flg(*s == *c));
        }
        delete c;
        break;
    }
    case OP_TRIM_DOMAIN:
    {
        SmExtent2d dom = rd.domain();
        SmStatus stt = s->TrimWithDomain(dom);
        o.push_back(st(stt));
        o.push_back(dom.GetMin().x);
        o.push_back(dom.GetMin().y);
        o.push_back(dom.GetMax().x);
        o.push_back(dom.GetMax().y);
        put_state(o, s);
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_surface_torus_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                        int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    SmTorus* s = read_torus(rd, ctx, o);
    if (s)
    {
        torus_op(op, rd, ctx, s, o);
        delete s;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
