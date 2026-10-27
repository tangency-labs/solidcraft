// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.srf_in_volume`: the SmSrfInVolume members owned by the unit
// (source/SMLib/src/SmSrfInVolume.cpp, source/SMLib/inc/SmSrfInVolume.h), called through
// SmSrfInVolume / SmSurface on objects built from an SmBSplineSurface, SmPlane or SmCylinder base
// surface and a chain of SmTransform / SmTwistVolume / SmUnbendVolume / plain SmVolume maps.
//
// One entry point, sc_surface_srf_in_volume_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/surface_srf_in_volume.rs, which validates every
// input. `out` receives the op status (0 = SM_SUCCESS, else the SmStatus value) and the op's
// outputs. SmSrfInVolume.cpp has no file-static functions, so nothing is copied verbatim.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBendVolume.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCylinder.h>
#include <SmDatabaseIO.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmSrfInVolume.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmTransform.h>
#include <SmTwistVolume.h>
#include <SmUnbendVolume.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <SmVolume.h>
#include <nurbs.h>

#include <algorithm>
#include <sstream>
#include <string>
#include <vector>

#include "sc_sm.h"

using sc::put_dom;
using sc::put_mat;
using sc::put_vec;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_srf_in_volume.rs).
enum : int32_t
{
    OP_INFO = 0,
    OP_EVAL = 1,
    OP_EVAL_SIMPLE = 2,
    OP_EVAL_POINT = 3,
    OP_NORMAL = 4,
    OP_KNOTS = 5,
    OP_STEP_INV = 6,
    OP_REVERSE = 7,
    OP_SWAP_UV = 8,
    OP_TRIM = 9,
    OP_TRANSFORM = 10,
    OP_SPLIT = 11,
    OP_COPY = 12,
    OP_EQUAL = 13,
    OP_SET_OWNER = 14,
    OP_MAKE_OWNER = 15,
    OP_SET_BASE = 16,
    OP_SET_VOLUME = 17,
    OP_MIRROR = 18,
    OP_EXACT = 19,
    OP_READ_DB = 40,  // static: SmSrfInVolume::ReadFromDB
    OP_WRITE_DB = 41, // generator helper: WriteToDB of the object
};

enum : int64_t
{
    K_BASE = 0,
    K_TRANSFORM = 1,
    K_TWIST = 2,
    K_UNBEND = 3,
    K_BEND = 4,
    K_TWIST_C = 5,  // SmTwistVolume(ctx, origin, axis, mid, period, min)
    K_UNBEND_C = 6, // SmUnbendVolume(ctx, center, axis, mid, neutral, tol)
    K_BEND_C = 7,   // SmBendVolume(ctx, center, axis, mid, neutral, tol)
};

// Read access to the protected members of any SmSrfInVolume.
struct Peek : public SmSrfInVolume
{
    static ULONG owner(const SmSrfInVolume* p) { return ((const Peek*)p)->m_lOwnerFlag; }
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
    SmExtent2d dom()
    {
        double u0 = num();
        double v0 = num();
        double u1 = num();
        double v1 = num();
        return SmExtent2d(SmPoint2d(u0, v0), SmPoint2d(u1, v1));
    }
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(bool b) { return b ? 1.0 : 0.0; }

// ---- volumes (as the surface.volume_p1 shim) ------------------------------------------

// `origin X Y Z`: SmTransform(ctx, origin, X, Y, &Z).
SmTransform* read_transform(Rd& rd, const SmContext& ctx)
{
    SmVector3d o = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    SmVector3d z = rd.vec();
    return new (ctx) SmTransform(&ctx, o, x, y, &z);
}

// `n` maps `kind [T(12) | C(11)] has_orient [O(12)]` (kinds 5-7 are the component constructors
// with `C` = `p0 axis mid a b`, and never have an orient map here), chained in order with
// AddNextMap(m, 2).
SmVolume* read_volume(Rd& rd, const SmContext& ctx)
{
    int64_t n = rd.idx();
    SmVolume* head = NULL;
    for (int64_t i = 0; i < n; ++i)
    {
        int64_t kind = rd.idx();
        SmVolume* v = NULL;
        switch (kind)
        {
        case K_TRANSFORM: v = read_transform(rd, ctx); break;
        case K_TWIST: v = new (ctx) SmTwistVolume(&ctx); break;
        case K_UNBEND: v = new (ctx) SmUnbendVolume(&ctx); break;
        case K_BEND: v = new (ctx) SmBendVolume(&ctx); break;
        case K_TWIST_C:
        case K_UNBEND_C:
        case K_BEND_C:
        {
            SmPoint3d p0 = rd.vec();
            SmVector3d axis = rd.vec();
            SmVector3d mid = rd.vec();
            double a = rd.num();
            double b = rd.num();
            if (kind == K_TWIST_C)
                v = new (ctx) SmTwistVolume(ctx, p0, axis, mid, a, b);
            else if (kind == K_UNBEND_C)
                v = new (ctx) SmUnbendVolume(ctx, p0, axis, mid, a, b);
            else
                v = new (ctx) SmBendVolume(ctx, p0, axis, mid, a, b);
            break;
        }
        default: v = new (ctx) SmVolume(&ctx); break;
        }
        if (rd.flag())
            v->SetOrientMap(read_transform(rd, ctx), 2);
        if (head == NULL)
            head = v;
        else
            head->AddNextMap(v, 2);
    }
    return head;
}

// The chain: `n` then per map `type [T(16) | m0 m1] has_orient [O(16)] has_inv [I(16)]` (the
// members of a twist, unbend or bend map: period and minimum, or neutral distance and
// singularity tolerance).
void put_volume(std::vector<double>& o, const SmVolume* v)
{
    int64_t n = 0;
    for (const SmVolume* p = v; p; p = p->GetNextMap())
        ++n;
    o.push_back((double)n);
    for (const SmVolume* p = v; p; p = p->GetNextMap())
    {
        o.push_back((double)p->GetType());
        if (p->GetType() == SmTransform_TYPE)
            put_mat(o, *(const SmTransform*)p);
        else if (p->GetType() == SmTwistVolume_TYPE)
        {
            o.push_back(((const SmTwistVolume*)p)->GetTwistPeriodDist());
            o.push_back(((const SmTwistVolume*)p)->GetPeriodMinDist());
        }
        else if (p->GetType() == SmUnbendVolume_TYPE)
        {
            o.push_back(((const SmUnbendVolume*)p)->GetUnbendNeutralDist());
            o.push_back(((const SmUnbendVolume*)p)->GetSingularityTol());
        }
        else if (p->GetType() == SmBendVolume_TYPE)
        {
            o.push_back(((const SmBendVolume*)p)->GetBendNeutralDist());
            o.push_back(((const SmBendVolume*)p)->GetSingularityTol());
        }
        o.push_back(p->GetOrientMap() ? 1.0 : 0.0);
        if (p->GetOrientMap())
            put_mat(o, *p->GetOrientMap());
        o.push_back(p->GetInvOrientMap() ? 1.0 : 0.0);
        if (p->GetInvOrientMap())
            put_mat(o, *p->GetInvOrientMap());
    }
}

// ---- surfaces -------------------------------------------------------------------------

// A surface block: `0 oob form n m p q r s cpts(x y z w) u v` (SmBSplineSurface(gw_SURFACE*,
// FALSE, NULL)), `1 O[3] N[3]` (SmPlane(O, N)) or `2 O[3] X[3] Y[3] r start end h swap inside`
// (SmCylinder(O, X, Y, r, start, end, h, swap, inside, FALSE) + MakeNurb, status ignored: the
// ffi wrapper admits only inputs it succeeds on).
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 1)
    {
        SmPoint3d o = rd.vec();
        SmVector3d n = rd.vec();
        return new (ctx) SmPlane(o, n, &ctx);
    }
    if (kind == 2)
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double r = rd.num();
        double a0 = rd.num();
        double a1 = rd.num();
        double h = rd.num();
        SmBoolean swap = rd.bflag();
        SmBoolean inside = rd.bflag();
        SmCylinder* c = new (ctx) SmCylinder(o, x, y, r, a0, a1, h, swap, inside, FALSE, &ctx);
        c->MakeNurb();
        return c;
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

// Surface state: `type has_nurb [n m p q r s cpts u v]`.
void put_surface(std::vector<double>& o, const SmSurface* srf)
{
    o.push_back((double)srf->GetType());
    SmBSplineSurface* s = SM_CAST_PTR(SmBSplineSurface, (SmSurface*)srf);
    if (!s)
    {
        o.push_back(0.0);
        return;
    }
    // a plane's NURB is made on demand, also by const methods: make it before dumping
    if (s->IsKindOf(SmPlane_TYPE))
        s->GetOrCreateGwNurbPointer();
    const gw_SURFACE* sur = s->GetGwNurbPointer();
    o.push_back(flg(sur != NULL));
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

// Full state: owner_flag in_param surface volume.
void put_state(std::vector<double>& o, const SmSrfInVolume* c)
{
    o.push_back((double)Peek::owner(c));
    o.push_back(flg(c->GetInParamSpace()));
    put_surface(o, c->GetSurface());
    put_volume(o, c->GetVolume());
}

// An SmSrfInVolume built from a block, with its original parts (freed by `free_siv`).
struct Built
{
    SmSurface* surface = NULL;
    SmVolume* volume = NULL;
    SmSrfInVolume* siv = NULL;
    bool leak = false; // parts replaced by the op: nothing is freed
};

// Block: surface block, volume block, in_param copy_flag owner_flag.
Built read_siv(Rd& rd, const SmContext& ctx)
{
    Built b;
    b.surface = read_surface(rd, ctx);
    b.volume = read_volume(rd, ctx);
    bool in_param = rd.flag();
    ULONG copy = (ULONG)rd.idx();
    ULONG owner = (ULONG)rd.idx();
    b.siv = new (ctx) SmSrfInVolume(*b.surface, in_param ? TRUE : FALSE, *b.volume, copy, owner, &ctx);
    return b;
}

// Frees the parts the object does not delete itself (before it, while it is alive as
// their owner), then the object.
void free_siv(Built& b)
{
    if (b.leak)
        return;
    SmSurface* cs = b.siv->GetSurface();
    SmVolume* cv = b.siv->GetVolume();
    bool siv_deletes_surface = b.siv->OwnsSurface() != FALSE;
    bool siv_deletes_volume = b.siv->OwnsVolume() != FALSE;
    if (cs != b.surface && !siv_deletes_surface)
        delete cs;
    if (!(cs == b.surface && siv_deletes_surface))
        delete b.surface;
    if (cv != b.volume && !siv_deletes_volume)
        delete cv;
    if (!(cv == b.volume && siv_deletes_volume))
        delete b.volume;
    delete b.siv;
}

// The defined entries of an evaluation matrix `D[u][v]` (hu x hv, flat `u * (hv + 1) + v`):
// all for unequal orders or a full matrix, else `u + v <= n`.
void put_derivs(std::vector<double>& o, const SmVector3d* d, ULONG hu, ULONG hv, bool upper)
{
    for (ULONG i = 0; i <= hu; i++)
        for (ULONG j = 0; j <= hv; j++)
            if (hu != hv || !upper || i + j <= hu)
                put_vec(o, d[i * (hv + 1) + j]);
}

void siv_op(int32_t op, Rd& rd, const SmContext& ctx, Built& bb, std::vector<double>& o)
{
    SmSrfInVolume* c = bb.siv;
    switch (op)
    {
    case OP_INFO:
    {
        o.push_back(0.0);
        o.push_back((double)c->GetType());
        o.push_back((double)c->GetDegree(SM_SP_U));
        o.push_back((double)c->GetDegree(SM_SP_V));
        put_dom(o, c->GetNaturalUVDomain());
        put_dom(o, c->GetSTEPUVDomain());
        put_dom(o, c->GetMaxAnalyticDomain());
        o.push_back(c->OwnsSurface() ? 1.0 : 0.0);
        o.push_back(c->OwnsVolume() ? 1.0 : 0.0);
        SmPoint2d in = rd.uv();
        SmPoint2d r1, r2;
        SmStatus s1 = c->ConvertUVFromSTEPToNURBS(in, r1);
        SmStatus s2 = c->ConvertUVFromNURBSToSTEP(in, r2);
        o.push_back(st(s1));
        o.push_back(r1.x);
        o.push_back(r1.y);
        o.push_back(st(s2));
        o.push_back(r2.x);
        o.push_back(r2.y);
        int64_t nt = rd.idx();
        for (int64_t i = 0; i < nt; i++)
            o.push_back(c->IsKindOf((SM_TYPE)rd.idx()) ? 1.0 : 0.0);
        put_state(o, c);
        break;
    }
    case OP_EVAL:
    case OP_EVAL_SIMPLE:
    {
        SmPoint2d uv = rd.uv();
        ULONG hu = (ULONG)rd.idx();
        ULONG hv = (ULONG)rd.idx();
        SmBoolean ul = rd.bflag(), vl = rd.bflag(), oh = rd.bflag();
        SmBoolean nz = op == OP_EVAL ? rd.bflag() : FALSE;
        // SmSrfInVolume::Evaluate also writes Duv one slot past a (0, 0) request with
        // bOnlyUpperHalf == FALSE (the caller's array must hold it): size the buffer for it
        std::vector<SmVector3d> d(std::max<size_t>((hu + 1) * (hv + 1), 32));
        SmStatus s = op == OP_EVAL ? c->Evaluate(uv, hu, hv, ul, vl, oh, d.data(), nz)
                                   : c->EvaluateSimple(uv, hu, hv, ul, vl, oh, d.data());
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_derivs(o, d.data(), hu, hv, oh != FALSE);
        break;
    }
    case OP_EVAL_POINT:
    {
        SmPoint2d uv = rd.uv();
        SmPoint3d p;
        SmStatus s = c->EvaluatePoint(uv, p);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, p);
        break;
    }
    case OP_NORMAL:
    {
        SmPoint2d uv = rd.uv();
        SmBoolean ul = rd.bflag(), vl = rd.bflag();
        SmVector3d n;
        SmStatus s = c->EvaluateNormal(uv, ul, vl, n);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, n);
        break;
    }
    case OP_KNOTS:
    {
        SmSurfParamType param = rd.flag() ? SM_SP_V : SM_SP_U;
        bool has_ivl = rd.flag();
        double a = rd.num();
        double b = rd.num();
        SmExtent1d ivl(a, b);
        SmTArray<double> k;
        SmTArray<ULONG> m;
        SmStatus s = c->GetKnots(param, k, &m, has_ivl ? &ivl : NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)k.GetSize());
            for (ULONG i = 0; i < k.GetSize(); i++)
                o.push_back(k[i]);
            o.push_back((double)m.GetSize());
            for (ULONG i = 0; i < m.GetSize(); i++)
                o.push_back((double)m[i]);
        }
        break;
    }
    case OP_STEP_INV:
    {
        SmExtent2d dom = rd.dom();
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        SmPoint2d uv;
        SmLocationType loc = SM_LT_EXTERIOR;
        {
            // upstream UB (plan/upstream-issues.md): STEPInversion reads sParamPoints[0] of the
            // volume's inverse even when it succeeded with no points (a null dereference).
            // Probe the same volume call and report SM_ERR, as the Rust port does.
            SmBoolean ok = FALSE;
            SmTArray<SmPoint3d> pts;
            SmTArray<double> gaps;
            SmStatus vs = c->GetInParamSpace()
                              ? c->GetVolume()->InvEvaluatePoint(p, NULL, ok, pts, gaps)
                              : c->GetVolume()->InvMapPoint(p, NULL, ok, pts, gaps);
            if (vs == SM_SUCCESS && ok == TRUE && pts.GetSize() == 0)
            {
                o.push_back(st(SM_ERR));
                break;
            }
        }
        SmStatus s = c->STEPInversion(dom, p, tol, uv, loc, NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(uv.x);
            o.push_back(uv.y);
            o.push_back((double)loc);
        }
        break;
    }
    case OP_REVERSE:
    {
        SmSurfParamType param = rd.flag() ? SM_SP_V : SM_SP_U;
        SmStatus s = c->Reverse(param);
        o.push_back(st(s));
        put_state(o, c);
        break;
    }
    case OP_SWAP_UV:
    {
        SmStatus s = c->SwapUV();
        o.push_back(st(s));
        put_state(o, c);
        break;
    }
    case OP_TRIM:
    {
        SmExtent2d dom = rd.dom();
        SmStatus s = c->TrimWithDomain(dom);
        o.push_back(st(s));
        put_dom(o, dom);
        put_state(o, c);
        break;
    }
    case OP_TRANSFORM:
    {
        SmPoint3d org = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        bool has_scale = rd.flag();
        SmVector3d sc = rd.vec();
        SmAxis2Placement ax(org, x, y);
        SmStatus s = c->Transform(ax, has_scale ? &sc : NULL);
        o.push_back(st(s));
        put_state(o, c);
        break;
    }
    case OP_SPLIT:
    {
        SmSurfParamType param = rd.flag() ? SM_SP_V : SM_SP_U;
        double t = rd.num();
        SmSurface *l = NULL, *r = NULL;
        SmStatus s = c->SplitAt(ctx, t, param, l, r);
        o.push_back(st(s));
        if (s == SM_SUCCESS && l != NULL && r != NULL)
        {
            put_state(o, (SmSrfInVolume*)l);
            put_state(o, (SmSrfInVolume*)r);
            // the halves own their surface and their (copied) volume
            delete l;
            delete r;
        }
        break;
    }
    case OP_COPY:
    {
        SmSurface* cp = NULL;
        SmStatus s = c->Copy(ctx, cp);
        o.push_back(st(s));
        if (s == SM_SUCCESS && cp != NULL)
        {
            SmSrfInVolume* cc = (SmSrfInVolume*)cp;
            put_state(o, cc);
            // the copy deletes what it owns; free the copies it does not own
            SmSurface* csur = cc->GetSurface();
            SmVolume* cvol = cc->GetVolume();
            bool del_s = cc->OwnsSurface() != FALSE;
            bool del_v = cc->OwnsVolume() != FALSE;
            bool sur_copied = csur != c->GetSurface();
            bool vol_copied = cvol != c->GetVolume();
            if (sur_copied && !del_s)
                delete csur;
            if (vol_copied && !del_v)
                delete cvol;
            delete cc;
        }
        break;
    }
    case OP_EQUAL:
    {
        bool self_cmp = rd.flag();
        if (self_cmp)
        {
            o.push_back(0.0);
            o.push_back((*c == *(SmSurface*)c) ? 1.0 : 0.0);
            break;
        }
        Built other = read_siv(rd, ctx);
        o.push_back(0.0);
        o.push_back((*c == *(SmSurface*)other.siv) ? 1.0 : 0.0);
        free_siv(other);
        break;
    }
    case OP_SET_OWNER:
    {
        ULONG flag = (ULONG)rd.idx();
        c->SetOwnerFlag(flag);
        o.push_back(0.0);
        put_state(o, c);
        // the owner flag changes who deletes what: leave the parts alone
        bb.leak = true;
        break;
    }
    case OP_MAKE_OWNER:
    {
        SmStatus s = c->MakeOwner();
        o.push_back(st(s));
        put_state(o, c);
        break;
    }
    case OP_SET_BASE:
    {
        SmSurface* ns = read_surface(rd, ctx);
        bool in_param = rd.flag();
        bool copy = rd.flag();
        bool owns = rd.flag();
        SmStatus s = c->SetBaseSurface(ns, in_param ? TRUE : FALSE, copy ? TRUE : FALSE,
                                       owns ? TRUE : FALSE);
        o.push_back(st(s));
        put_state(o, c);
        bb.leak = true;
        break;
    }
    case OP_SET_VOLUME:
    {
        SmVolume* nv = read_volume(rd, ctx);
        bool copy = rd.flag();
        bool owns = rd.flag();
        SmStatus s = c->SetVolume(nv, copy ? TRUE : FALSE, owns ? TRUE : FALSE);
        o.push_back(st(s));
        put_state(o, c);
        bb.leak = true;
        break;
    }
    case OP_MIRROR:
    {
        SmPoint3d org = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        SmAxis2Placement ax(org, x, y);
        SmSurface* m = NULL;
        SmStatus s = c->CreateMirrorSurface(ctx, ax, m);
        o.push_back(st(s));
        // the state of the mirrored surface (a copy of this one with its volume mirrored;
        // not freed: its ownership follows the copy flags)
        if (s == SM_SUCCESS && m != NULL)
            put_state(o, (SmSrfInVolume*)m);
        break;
    }
    case OP_EXACT:
    {
        SmBSplineSurface* b = NULL;
        SmStatus s = c->MakeExactBSplineIfPossible(b);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(b != NULL));
            if (b != NULL)
                put_surface(o, b);
        }
        delete b;
        break;
    }
    case OP_WRITE_DB:
    {
        // generator helper: the bytes upstream WriteToDB produces (ASCII with 17 digits)
        ULONG version = (ULONG)rd.idx();
        std::ostringstream oss;
        oss.precision(17);
        SmDatabaseIOStream db(SM_ASCII, &oss);
        SmStatus s = c->WriteToDB(db, version);
        o.push_back(st(s));
        std::string bytes = oss.str();
        for (char ch : bytes)
            o.push_back((double)(unsigned char)ch);
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}

void static_op(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_READ_DB:
    {
        ULONG version = (ULONG)rd.idx();
        int64_t nb = rd.idx();
        std::string bytes;
        for (int64_t i = 0; i < nb; i++)
            bytes.push_back((char)(unsigned char)rd.idx());
        std::istringstream iss(bytes);
        SmDatabaseIOStream db(SM_ASCII, &iss);
        SmSurface* c = NULL;
        SmStatus s = SmSrfInVolume::ReadFromDB(SmSrfInVolume_TYPE, db, ctx, c, version);
        o.push_back(st(s));
        if (s == SM_SUCCESS && c != NULL)
        {
            o.push_back(1.0);
            put_state(o, (SmSrfInVolume*)c);
            delete c;
        }
        else
            o.push_back(0.0);
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_surface_srf_in_volume_eval(int32_t op, const double* in, int64_t n_in,
                                                double* out, int64_t cap, int64_t* out_len)
{
    if (in == nullptr || n_in < 0 || out_len == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    Rd rd{in, 0};
    std::vector<double> o;
    if (op >= OP_READ_DB && op != OP_WRITE_DB)
    {
        static_op(op, rd, ctx, o);
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    Built b = read_siv(rd, ctx);
    siv_op(op, rd, ctx, b, o);
    free_siv(b);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
