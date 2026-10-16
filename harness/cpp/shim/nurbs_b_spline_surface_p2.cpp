// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nurbs.b_spline_surface_p2`.
// Wraps the SmBSplineSurface members owned by the unit (source/SMLib/src/SmBSplineSurface.cpp):
// CopyAndAddAnalytics, CreateIsoParametricCurve, CreateSphereFromArcs, CreateSurfOfRevolution,
// CreateTrimmedPlaneFromRuled, IsDegeneratePoint, IsBilinear, GetControlPoint,
// GetControlPointsExpert, GetMaxAnalyticDomain, SetCanonical, SetControlPoint, CopySubPatch,
// SplitAt, SwapUV and AdjustSTEPUVDomain.
//
// One entry point, sc_nurbs_b_spline_surface_p2_eval(op, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/nurbs_b_spline_surface_p2.rs,
// which validates every input so upstream only touches valid memory. `out` receives the op
// status (0 = SM_SUCCESS, else the SmStatus value) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <vector>

#include "sc_sm.h"

using sc::flg;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/nurbs_b_spline_surface_p2.rs).
enum : int32_t
{
    // surface ops (`this` from a surface block)
    OP_ANALYTICS = 0,
    OP_ISO = 1,
    OP_DEGEN_PT = 2,
    OP_BILINEAR = 3,
    OP_GET_CP = 4,
    OP_EXPERT = 5,
    OP_MAX_DOMAIN = 6,
    OP_SET_CANON = 7,
    OP_SET_CP = 8,
    OP_SUB_PATCH = 9,
    OP_SPLIT = 10,
    OP_SWAP = 11,
    OP_ADJUST = 12,
    // static ops
    OP_SPHERE = 20,
    OP_REVOLUTION = 21,
    OP_TRIM_PLANE = 22,
};

// Surface constructions of the surface block.
enum : int32_t
{
    CT_NONE = 0,       // static op, no surface
    CT_EMPTY = 1,      // SmBSplineSurface()
    CT_FAST_OWNED = 2, // SmBSplineSurface(gw_SURFACE*, FALSE, NULL)
};

// Reads the protected STEP flags through pointers to members named in a derived class.
struct Peek : SmBSplineSurface
{
    static SmKnotType SmBSplineSurface::*kt() { return &Peek::m_eKnotType; }
    static SmBSplineSurfaceForm SmBSplineSurface::*form() { return &Peek::m_eBSplineSurfaceForm; }
};

struct PeekC : SmBSplineCurve
{
    static SmKnotType SmBSplineCurve::*kt() { return &PeekC::m_eKnotType; }
    static SmBSplineCurveForm SmBSplineCurve::*form() { return &PeekC::m_eBSplineCurveForm; }
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

// Reads `n m p q r s cpts u v` into a surface from sm_AllocateNurbSurface.
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

// Builds a surface from a block `oob form [surface]` (ctor 1 or 2).
SmBSplineSurface* read_surface(Rd& rd, const SmContext& ctx, int32_t ctor)
{
    bool oob = rd.flag();
    SmBSplineSurfaceForm form = (SmBSplineSurfaceForm)rd.idx();
    SmBSplineSurface* s = NULL;
    switch (ctor)
    {
    case CT_EMPTY: s = new (ctx) SmBSplineSurface(); break;
    case CT_FAST_OWNED: s = new (ctx) SmBSplineSurface(read_nurb(rd), FALSE, NULL); break;
    default: return NULL;
    }
    s->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
    s->SetBSplineSurfaceForm(form);
    return s;
}

void put_state(std::vector<double>& o, const SmBSplineSurface* s)
{
    const gw_SURFACE* sur = s->GetGwNurbPointer();
    o.push_back(flg(sur != NULL));
    o.push_back(flg(s->IsOutOfBoundsEnabled()));
    o.push_back(flg(s->IsNurbBorrowed()));
    o.push_back((double)(s->*Peek::kt()));
    o.push_back((double)(s->*Peek::form()));
    if (!sur)
        return;
    gw_INDEX n = sur->net->n;
    gw_INDEX m = sur->net->m;
    gw_INDEX r = sur->knu->m;
    gw_INDEX ss = sur->knv->m;
    o.push_back((double)n);
    o.push_back((double)m);
    o.push_back((double)sur->p);
    o.push_back((double)sur->q);
    o.push_back((double)r);
    o.push_back((double)ss);
    for (gw_INDEX i = 0; i <= n; i++)
        for (gw_INDEX j = 0; j <= m; j++)
        {
            o.push_back(sur->net->Pw[i][j].x);
            o.push_back(sur->net->Pw[i][j].y);
            o.push_back(sur->net->Pw[i][j].z);
            o.push_back(sur->net->Pw[i][j].w);
        }
    for (gw_INDEX i = 0; i <= r; i++)
        o.push_back(sur->knu->U[i]);
    for (gw_INDEX i = 0; i <= ss; i++)
        o.push_back(sur->knv->U[i]);
}

void put_curve(std::vector<double>& o, const SmBSplineCurve* c)
{
    o.push_back((double)c->GetDim());
    o.push_back(flg(c->IsOutOfBoundsEnabled()));
    o.push_back(flg(c->IsNurbBorrowed()));
    o.push_back((double)(c->*PeekC::kt()));
    o.push_back((double)(c->*PeekC::form()));
    const gw_CURVE* cur = c->GetGwNurbPointer();
    o.push_back(flg(cur != NULL));
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

// Reads `n p m cpts knots` into SmBSplineCurve(gw_CURVE*, 3, FALSE).
SmBSplineCurve* read_curve(Rd& rd, const SmContext& ctx)
{
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
    return new (ctx) SmBSplineCurve(cur, 3, FALSE);
}

// Outputs status, then on success `has` and the state of `s`; deletes `s`.
void put_new(std::vector<double>& o, SmStatus stt, SmSurface* s)
{
    o.push_back(st(stt));
    if (stt == SM_SUCCESS)
    {
        o.push_back(flg(s != NULL));
        if (s)
            put_state(o, (SmBSplineSurface*)s);
    }
    delete s;
}

void surface_op(int32_t op, Rd& rd, const SmContext& ctx, SmBSplineSurface* s,
                std::vector<double>& o)
{
    switch (op)
    {
    case OP_ANALYTICS:
    {
        SmSurface* res = NULL;
        SmStatus stt = s->CopyAndAddAnalytics(ctx, res);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            o.push_back(res ? (double)res->GetType() : 0.0);
        delete res;
        break;
    }
    case OP_ISO:
    {
        SmSurfParamType param = (SmSurfParamType)rd.idx();
        double iso = rd.num();
        bool has_dom = rd.flag();
        SmExtent2d dom = rd.domain();
        bool want_uv = rd.flag();
        SmBSplineCurve* c = NULL;
        double gap = -1.0;
        SmCurve* uv = NULL;
        SmStatus stt = s->CreateIsoParametricCurve(ctx, param, iso, 0.0, c, has_dom ? &dom : NULL,
                                                   &gap, want_uv ? &uv : NULL);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(gap);
            o.push_back(flg(uv != NULL));
            o.push_back(flg(c != NULL));
            if (c)
                put_curve(o, c);
        }
        delete c;
        delete uv;
        break;
    }
    case OP_DEGEN_PT:
    {
        double tol = rd.num();
        bool has_dom = rd.flag();
        SmExtent2d dom = rd.domain();
        SmBoolean b = s->IsDegeneratePoint(tol, has_dom ? &dom : NULL);
        o.push_back(0.0);
        o.push_back(flg(b != FALSE));
        break;
    }
    case OP_BILINEAR:
    {
        o.push_back(0.0);
        o.push_back(flg(s->IsBilinear() != FALSE));
        break;
    }
    case OP_GET_CP:
    {
        SmControlPointFormType form = (SmControlPointFormType)rd.idx();
        ULONG u = (ULONG)rd.idx();
        ULONG v = (ULONG)rd.idx();
        SmPoint3d p(0.0, 0.0, 0.0);
        double w = 0.0;
        SmStatus stt = s->GetControlPoint(form, u, v, p, w);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(p.x);
            o.push_back(p.y);
            o.push_back(p.z);
            o.push_back(w);
        }
        break;
    }
    case OP_EXPERT:
    {
        SmControlPointFormType form = (SmControlPointFormType)rd.idx();
        int64_t su = rd.idx();
        int64_t eu = rd.idx();
        int64_t sv = rd.idx();
        int64_t ev = rd.idx();
        int64_t us = rd.idx();
        int64_t vs = rd.idx();
        int64_t pt = form == SM_CP_NON_RATIONAL ? 3 : 4;
        // the slots written: through the last point's last value (none for an empty block)
        int64_t len = (su <= eu && sv <= ev) ? (eu - su) * us + (ev - sv) * vs + pt : 0;
        std::vector<double> buf((size_t)(len > 0 ? len : 1), 0.0);
        SmStatus stt = s->GetControlPointsExpert(form, (ULONG)su, (ULONG)eu, (ULONG)sv, (ULONG)ev,
                                                 (ULONG)us, (ULONG)vs, buf.data());
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back((double)len);
            for (int64_t i = 0; i < len; i++)
                o.push_back(buf[(size_t)i]);
        }
        break;
    }
    case OP_MAX_DOMAIN:
    {
        SmExtent2d d = s->GetMaxAnalyticDomain();
        o.push_back(0.0);
        o.push_back(d.GetMin().x);
        o.push_back(d.GetMin().y);
        o.push_back(d.GetMax().x);
        o.push_back(d.GetMax().y);
        break;
    }
    case OP_SET_CANON:
    {
        ULONG ud = (ULONG)rd.idx();
        ULONG vd = (ULONG)rd.idx();
        int64_t ncp = rd.idx();
        SmTArray<SmPoint3d> pts;
        for (int64_t i = 0; i < ncp; i++)
            pts.Add(rd.vec());
        SmBSplineSurfaceForm form = (SmBSplineSurfaceForm)rd.idx();
        SmTArray<double> uk, vk;
        SmTArray<ULONG> um, vm;
        int64_t nu = rd.idx();
        for (int64_t i = 0; i < nu; i++)
            uk.Add(rd.num());
        for (int64_t i = 0; i < nu; i++)
            um.Add((ULONG)rd.idx());
        int64_t nv = rd.idx();
        for (int64_t i = 0; i < nv; i++)
            vk.Add(rd.num());
        for (int64_t i = 0; i < nv; i++)
            vm.Add((ULONG)rd.idx());
        SmKnotType kt = (SmKnotType)rd.idx();
        bool has_w = rd.flag();
        SmTArray<double> w;
        if (has_w)
        {
            int64_t nw = rd.idx();
            for (int64_t i = 0; i < nw; i++)
                w.Add(rd.num());
        }
        SmStatus stt = s->SetCanonical(ud, vd, pts, form, um, vm, uk, vk, kt, has_w ? &w : NULL);
        o.push_back(st(stt));
        put_state(o, s);
        break;
    }
    case OP_SET_CP:
    {
        SmControlPointFormType form = (SmControlPointFormType)rd.idx();
        ULONG u = (ULONG)rd.idx();
        ULONG v = (ULONG)rd.idx();
        SmVector3d p = rd.vec();
        double w = rd.num();
        SmStatus stt = s->SetControlPoint(form, u, v, p, w);
        o.push_back(st(stt));
        put_state(o, s);
        break;
    }
    case OP_SUB_PATCH:
    {
        SmExtent2d dom = rd.domain();
        SmBSplineSurface* res = NULL;
        SmStatus stt = s->CopySubPatch(ctx, dom, res);
        put_new(o, stt, res);
        break;
    }
    case OP_SPLIT:
    {
        double t = rd.num();
        SmSurfParamType param = (SmSurfParamType)rd.idx();
        SmSurface* l = NULL;
        SmSurface* r = NULL;
        SmStatus stt = s->SplitAt(ctx, t, param, l, r);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(flg(l != NULL && r != NULL));
            if (l && r)
            {
                put_state(o, (SmBSplineSurface*)l);
                put_state(o, (SmBSplineSurface*)r);
            }
        }
        delete l;
        delete r;
        break;
    }
    case OP_SWAP:
    {
        SmStatus stt = s->SwapUV();
        o.push_back(st(stt));
        put_state(o, s);
        break;
    }
    case OP_ADJUST:
    {
        SmExtent2d dom = rd.domain();
        SmStatus stt = s->AdjustSTEPUVDomain(dom);
        o.push_back(st(stt));
        put_state(o, s);
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}

void static_op(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_SPHERE:
    {
        int64_t k = rd.idx();
        SmTArray<SmBSplineCurve*> arcs;
        for (int64_t i = 0; i < k; i++)
            arcs.Add(read_curve(rd, ctx));
        double tol = rd.num();
        SmTArray<SmCurve*> ccw;
        SmTArray<SmOrientType> orients;
        SmBSplineSurface* res = NULL;
        SmStatus stt = SmBSplineSurface::CreateSphereFromArcs(ctx, arcs, tol, ccw, orients, res);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back((double)ccw.GetSize());
            for (ULONG i = 0; i < ccw.GetSize(); i++)
            {
                double at = -1.0;
                for (ULONG j = 0; j < arcs.GetSize(); j++)
                    if ((SmCurve*)arcs[j] == ccw[i])
                        at = (double)j;
                o.push_back(at);
            }
            for (ULONG i = 0; i < orients.GetSize(); i++)
                o.push_back((double)orients[i]);
            o.push_back(flg(res != NULL));
            if (res)
                put_state(o, res);
        }
        delete res;
        for (ULONG i = 0; i < arcs.GetSize(); i++)
            delete arcs[i];
        break;
    }
    case OP_REVOLUTION:
    {
        SmBSplineCurve* c = read_curve(rd, ctx);
        SmVector3d origin = rd.vec();
        SmVector3d axis = rd.vec();
        double ang = rd.num();
        bool swap = rd.flag();
        SmBSplineSurface* res = NULL;
        SmStatus stt = SmBSplineSurface::CreateSurfOfRevolution(ctx, c, origin, axis, ang, res,
                                                                swap ? TRUE : FALSE, NULL);
        put_new(o, stt, res);
        delete c;
        break;
    }
    case OP_TRIM_PLANE:
    {
        SmBSplineCurve* c1 = read_curve(rd, ctx);
        SmBSplineCurve* c2 = read_curve(rd, ctx);
        SmTArray<SmCurve*> trims;
        SmTArray<SmOrientType> orients;
        SmBSplineSurface* res = NULL;
        SmStatus stt =
            SmBSplineSurface::CreateTrimmedPlaneFromRuled(ctx, *c1, *c2, trims, orients, res);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back((double)trims.GetSize());
            o.push_back(flg(res != NULL));
            if (res)
                put_state(o, res);
        }
        delete res;
        for (ULONG i = 0; i < trims.GetSize(); i++)
            delete trims[i];
        delete c1;
        delete c2;
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_nurbs_b_spline_surface_p2_eval(int32_t op, const double* in, int64_t n_in,
                                                    double* out, int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    int32_t ctor = (int32_t)rd.idx();
    if (ctor == CT_NONE)
    {
        static_op(op, rd, ctx, o);
    }
    else
    {
        SmBSplineSurface* s = read_surface(rd, ctx, ctor);
        if (s == NULL)
            return SC_ERR_ARG;
        surface_op(op, rd, ctx, s, o);
        delete s;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
