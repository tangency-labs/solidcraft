// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nurbs.b_spline_surface_n_lib_p2`.
// Wraps the SmBSplineSurface members owned by the unit (source/SMLib/src/SmBSplineSurfaceNLib.cpp):
// Evaluate, TrimWithDomain, CreateSkinnedSurface and CreateCornerBlend.
//
// One entry point, sc_nurbs_b_spline_surface_n_lib_p2_eval(op, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/nurbs_b_spline_surface_n_lib_p2.rs,
// which validates every input so upstream only touches valid memory. `out` receives the op
// status (0 = SM_SUCCESS, else the SmStatus value) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmGeomUtility.h>
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
using sc::put_vec;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/nurbs_b_spline_surface_n_lib_p2.rs).
enum : int32_t
{
    // surface ops (`this` from a surface block)
    OP_EVAL = 0,
    OP_TRIM = 1,
    // static ops
    OP_SKIN = 20,
    OP_CORNER = 21,
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

struct Rd
{
    const double* in;
    int64_t at;
    double num() { return in[at++]; }
    int64_t idx() { return (int64_t)in[at++]; }
    bool flag() { return in[at++] != 0.0; }
    SmPoint2d pt2()
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

// Builds a surface from a block `ctor oob form [surface]` (ctor 1 or 2).
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

// Reads `dim n p m cpts knots` into SmBSplineCurve(gw_CURVE*, dim, FALSE).
SmBSplineCurve* read_curve(Rd& rd, const SmContext& ctx)
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

SmBSplineSurface* read_plain_surface(Rd& rd, const SmContext& ctx)
{
    return new (ctx) SmBSplineSurface(read_nurb(rd), FALSE, NULL);
}

void surface_op(int32_t op, Rd& rd, SmBSplineSurface* s, std::vector<double>& o)
{
    switch (op)
    {
    case OP_EVAL:
    {
        SmPoint2d uv = rd.pt2();
        ULONG hu = (ULONG)rd.idx();
        ULONG hv = (ULONG)rd.idx();
        bool ul = rd.flag();
        bool vl = rd.flag();
        bool upper = rd.flag();
        bool nzt = rd.flag();
        std::vector<SmVector3d> d((hu + 1) * (hv + 1), SmVector3d(0.0, 0.0, 0.0));
        SmStatus stt = s->Evaluate(uv, hu, hv, ul ? TRUE : FALSE, vl ? TRUE : FALSE,
                                   upper ? TRUE : FALSE, d.data(), nzt ? TRUE : FALSE);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            for (size_t i = 0; i < d.size(); i++)
                put_vec(o, d[i]);
        break;
    }
    case OP_TRIM:
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

void static_op(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_SKIN:
    {
        int64_t k = rd.idx();
        SmTArray<SmBSplineCurve*> secs;
        for (int64_t i = 0; i < k; i++)
            secs.Add(read_curve(rd, ctx));
        bool sync = rd.flag();
        SmSurfParamType dir = (SmSurfParamType)rd.idx();
        double tol = rd.num();
        SmBSplineCurve* r1 = rd.flag() ? read_curve(rd, ctx) : NULL;
        SmBSplineCurve* r2 = rd.flag() ? read_curve(rd, ctx) : NULL;
        bool spine = rd.flag();
        bool has_params = rd.flag();
        SmTArray<double> params;
        if (has_params)
        {
            int64_t np = rd.idx();
            for (int64_t i = 0; i < np; i++)
                params.Add(rd.num());
        }
        bool has_derivs = rd.flag();
        SmBSplineSurface* ds[2] = {NULL, NULL};
        if (has_derivs)
            for (int i = 0; i < 2; i++)
                ds[i] = rd.flag() ? read_plain_surface(rd, ctx) : NULL;
        ULONG deg = (ULONG)rd.idx();
        SmBSplineSurface* res = NULL;
        SmStatus stt = SmBSplineSurface::CreateSkinnedSurface(
            ctx, secs, sync ? TRUE : FALSE, dir, tol, r1, r2, spine ? TRUE : FALSE,
            has_params ? &params : NULL, has_derivs ? ds : NULL, res, deg);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(flg(res != NULL));
            if (res)
                put_state(o, res);
        }
        delete res;
        for (ULONG i = 0; i < secs.GetSize(); i++)
            delete secs[i];
        delete r1;
        delete r2;
        delete ds[0];
        delete ds[1];
        break;
    }
    case OP_CORNER:
    {
        int64_t k = rd.idx();
        SmTArray<SmCurve*> crvs;
        SmTArray<SmOrientType> orients;
        SmTArray<SmBSplineCurve*> uvs;
        SmTArray<SmSurface*> srfs;
        for (int64_t i = 0; i < k; i++)
            crvs.Add(read_curve(rd, ctx));
        for (int64_t i = 0; i < k; i++)
            orients.Add((SmOrientType)rd.idx());
        for (int64_t i = 0; i < k; i++)
            uvs.Add(rd.flag() ? read_curve(rd, ctx) : (SmBSplineCurve*)NULL);
        for (int64_t i = 0; i < k; i++)
            srfs.Add(rd.flag() ? (SmSurface*)read_plain_surface(rd, ctx) : (SmSurface*)NULL);
        double tol = rd.num();
        double ang = rd.num();
        bool bilinear = rd.flag();
        SmTArray<SmSurface*> res;
        SmStatus stt = SmBSplineSurface::CreateCornerBlend(ctx, crvs, orients, uvs, srfs, tol, ang,
                                                           res, bilinear ? TRUE : FALSE);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back((double)res.GetSize());
            for (ULONG i = 0; i < res.GetSize(); i++)
                put_state(o, (SmBSplineSurface*)res[i]);
        }
        for (ULONG i = 0; i < res.GetSize(); i++)
            delete res[i];
        for (ULONG i = 0; i < crvs.GetSize(); i++)
            delete crvs[i];
        for (ULONG i = 0; i < uvs.GetSize(); i++)
            delete uvs[i];
        for (ULONG i = 0; i < srfs.GetSize(); i++)
            delete srfs[i];
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_nurbs_b_spline_surface_n_lib_p2_eval(int32_t op, const double* in,
                                                          int64_t n_in, double* out, int64_t cap,
                                                          int64_t* out_len)
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
        surface_op(op, rd, s, o);
        delete s;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
