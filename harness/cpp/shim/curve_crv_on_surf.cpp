// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.crv_on_surf`: the SmCrvOnSurf members owned by the unit
// (source/SMLib/src/SmCrvOnSurf.cpp, source/SMLib/inc/SmCrvOnSurf.h), called on objects
// built from a 2D SmBSplineCurve UV curve (or the UV-line constructor) over a
// SmBSplineSurface or an SmOffsetSurface wrapping one.
//
// One entry point, sc_curve_crv_on_surf_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/curve_crv_on_surf.rs, which validates every
// input. Object ops (op < 40) read an object block, then `nops` ops (code + parameters) run in
// order on the same object; each appends its status (0 = SM_SUCCESS, else the SmStatus value)
// and its outputs. Static ops (>= 40) read their own input.
//
// The protected members (owner flag, break cache, approximation) are read and the break
// cache is loaded through ScCos, a derived class adding nothing but accessors.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmOffsetSurface.h>
#include <SmCrvOnSurf.h>
#include <SmCurve.h>
#include <SmCurveTypes.h>
#include <SmDatabaseIO.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <sstream>
#include <string>
#include <vector>

#include "sc_assert_reports.h"
#include "sc_sm.h"

using sc::Rd;
using sc::put_vec;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/curve_crv_on_surf.rs).
enum : int32_t
{
    OP_INFO = 0,
    OP_EVAL = 1,
    OP_EVAL_POINT = 2,
    OP_EVAL_UV = 3,
    OP_EVAL_NORMAL = 4,
    OP_EVAL_1ST = 5,
    OP_KNOTS = 6,
    OP_NNK = 7,
    OP_CONTS = 8,
    OP_EQUAL = 9,
    OP_COPY = 10,
    OP_EDIT_PARAM = 11,
    OP_REVERSE = 12,
    OP_TRIM = 13,
    OP_TRANSFORM = 14,
    OP_APPROX = 15,
    OP_GET_OR_CREATE = 16,
    OP_BBOX = 17,
    OP_DROP = 18,
    OP_GAP = 19,
    OP_STATE = 20,
    OP_COPY_EVAL = 21,
    OP_ASSERT_VALID = 22,
    OP_READ_DB = 40,
    OP_WRITE_DB = 41,
};

class ScCos : public SmCrvOnSurf
{
  public:
    ScCos(SmCurve& c, SmSurface& s, const SmExtent2d* d, ULONG owner, const SmContext* ctx)
        : SmCrvOnSurf(c, s, d, owner, ctx)
    {
    }
    ScCos(SmPoint2d& a, SmPoint2d& b, SmSurface& s, const SmExtent2d* d, ULONG owner,
          const SmContext* ctx)
        : SmCrvOnSurf(a, b, s, d, owner, ctx)
    {
    }
    void SetCache(SmBoolean bNeed, const SmTArray<double>& b)
    {
        m_bNeedBreaks = bNeed;
        m_vBreaks = b;
    }
};

// Read access to the protected members of any SmCrvOnSurf.
struct Peek : public SmCrvOnSurf
{
    static ULONG owner(const SmCrvOnSurf* p) { return ((const Peek*)p)->m_lOwnerFlag; }
    static SmBoolean need(const SmCrvOnSurf* p) { return ((const Peek*)p)->m_bNeedBreaks; }
    static const SmTArray<double>& breaks(const SmCrvOnSurf* p)
    {
        return ((const Peek*)p)->m_vBreaks;
    }
    static const SmBSplineCurve* approx(const SmCrvOnSurf* p)
    {
        return ((const Peek*)p)->m_pBSApproxCurve;
    }
    static SmBoolean matches(const SmCrvOnSurf* p)
    {
        return ((const Peek*)p)->m_bParametrizationMatches;
    }
};

struct OffsetPeek : public SmOffsetSurface
{
    static SmBoolean owns_base(const SmOffsetSurface* p)
    {
        return ((const OffsetPeek*)p)->m_bOwnsSurface;
    }
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

gw_CURVE* read_nurb_curve(Rd& rd)
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
    return cur;
}

// `n m p q r s cpts(x y z w, v fastest) u v`.
gw_SURFACE* read_nurb_surface(Rd& rd)
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

void put_nurb_curve(std::vector<double>& o, const gw_CURVE* cur)
{
    if (cur == NULL)
    {
        o.push_back(-1.0);
        return;
    }
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

// A B-spline (or derived) curve: type dim nurb; other curves: type.
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    o.push_back((double)c->GetType());
    if (c->IsKindOf(SmBSplineCurve_TYPE))
    {
        const SmBSplineCurve* b = (const SmBSplineCurve*)c;
        o.push_back((double)b->GetDim());
        put_nurb_curve(o, b->GetGwNurbPointer());
    }
}

void put_surface(std::vector<double>& o, const SmSurface* s)
{
    o.push_back((double)s->GetType());
    if (s->GetType() == SmOffsetSurface_TYPE)
    {
        const SmOffsetSurface* off = (const SmOffsetSurface*)s;
        o.push_back(off->GetOffsetDistance());
        o.push_back(flg(OffsetPeek::owns_base(off)));
        put_surface(o, off->GetBaseSurface());
        return;
    }
    if (s->GetType() != SmBSplineSurface_TYPE)
        return;
    const SmBSplineSurface* b = (const SmBSplineSurface*)s;
    const gw_SURFACE* sur = b->GetGwNurbPointer();
    if (sur == NULL)
    {
        o.push_back(-1.0);
        return;
    }
    gw_INDEX n = sur->net->n;
    gw_INDEX m = sur->net->m;
    o.push_back((double)n);
    o.push_back((double)m);
    for (gw_INDEX i = 0; i <= n; i++)
        for (gw_INDEX j = 0; j <= m; j++)
        {
            o.push_back(sur->net->Pw[i][j].x);
            o.push_back(sur->net->Pw[i][j].y);
            o.push_back(sur->net->Pw[i][j].z);
            o.push_back(sur->net->Pw[i][j].w);
        }
    for (gw_INDEX i = 0; i <= sur->knu->m; i++)
        o.push_back(sur->knu->U[i]);
    for (gw_INDEX i = 0; i <= sur->knv->m; i++)
        o.push_back(sur->knv->U[i]);
}

// State: owner domain(4) need nb B[nb] gap matches has_approx [approx nurb], UV curve,
// then a recursive surface state (`type [n m cpts u v]` or
// `type distance owns_base base`).
void put_state(std::vector<double>& o, const SmCrvOnSurf* c)
{
    o.push_back((double)Peek::owner(c));
    const SmExtent2d& d = c->GetUVDomain();
    o.push_back(d.GetUMin());
    o.push_back(d.GetVMin());
    o.push_back(d.GetUMax());
    o.push_back(d.GetVMax());
    o.push_back(flg(Peek::need(c)));
    const SmTArray<double>& b = Peek::breaks(c);
    o.push_back((double)b.GetSize());
    for (ULONG i = 0; i < b.GetSize(); i++)
        o.push_back(b[i]);
    o.push_back(c->GetBSApproxMaxGap3d());
    o.push_back(flg(Peek::matches(c)));
    const SmBSplineCurve* a = Peek::approx(c);
    o.push_back(a ? 1.0 : 0.0);
    if (a)
        put_nurb_curve(o, a->GetGwNurbPointer());
    put_curve(o, c->GetUVCurve());
    put_surface(o, c->GetBaseSurface());
}

// An SmCrvOnSurf built from a block, with the parts the shim made (freed by `free_cos`).
struct Built
{
    SmCurve* uv = NULL; // NULL for the UV-line constructor (the object owns its line)
    gw_CURVE* borrowed_uv_nurb = NULL;
    SmSurface* srf = NULL;
    SmBSplineSurface* base_srf = NULL;
    gw_SURFACE* borrowed_nurb = NULL;
    bool offset_owns_base = false;
    ScCos* cos = NULL;
};

// Block: uv kind (0: borrow_uv_nurb n p m cpts knots, a 2D SmBSplineCurve;
// 1: x0 y0 x1 y1, the UV-line constructor), is_offset, borrow_surface_nurb, base surface,
// offset distance and ownership when wrapped, has_domain [umin vmin umax vmax], owner,
// has_cache [need nb B].
Built read_cos(Rd& rd, const SmContext& ctx)
{
    Built b;
    int64_t kind = rd.idx();
    SmPoint2d p0, p1;
    if (kind == 0)
    {
        const bool borrow_uv_nurb = rd.flag();
        gw_CURVE* uv_nurb = read_nurb_curve(rd);
        if (borrow_uv_nurb)
            b.borrowed_uv_nurb = uv_nurb;
        b.uv = new (ctx) SmBSplineCurve(uv_nurb, 2, borrow_uv_nurb ? TRUE : FALSE);
    }
    else
    {
        double x0 = rd.num();
        double y0 = rd.num();
        double x1 = rd.num();
        double y1 = rd.num();
        p0.Set(x0, y0);
        p1.Set(x1, y1);
    }
    bool is_offset = rd.flag();
    const bool borrow_nurb = rd.flag();
    gw_SURFACE* nurb = read_nurb_surface(rd);
    if (borrow_nurb)
        b.borrowed_nurb = nurb;
    b.base_srf = new (ctx) SmBSplineSurface(nurb, borrow_nurb ? TRUE : FALSE, NULL);
    b.srf = b.base_srf;
    if (is_offset)
    {
        const double distance = rd.num();
        b.offset_owns_base = rd.flag();
        b.srf = new (ctx)
            SmOffsetSurface(distance, *b.base_srf, b.offset_owns_base ? TRUE : FALSE);
    }
    bool has_dom = rd.flag();
    double d0 = rd.num();
    double d1 = rd.num();
    double d2 = rd.num();
    double d3 = rd.num();
    SmExtent2d dom(d0, d1, d2, d3);
    ULONG owner = (ULONG)rd.idx();
    if (kind == 0)
        b.cos = new (ctx) ScCos(*b.uv, *b.srf, has_dom ? &dom : NULL, owner, &ctx);
    else
        b.cos = new (ctx) ScCos(p0, p1, *b.srf, has_dom ? &dom : NULL, owner, &ctx);
    if (rd.flag())
    {
        bool need = rd.flag();
        int64_t nb = rd.idx();
        SmTArray<double> br;
        for (int64_t i = 0; i < nb; i++)
            br.Add(rd.num());
        b.cos->SetCache(need ? TRUE : FALSE, br);
    }
    return b;
}

// Frees the shim's parts the object does not delete itself, then the object. A wrapped
// offset either owns its B-spline base or borrows it until the wrapper is gone.
// Raw borrowed UV and surface NURBs outlive every referencing object and temporary copy.
void free_cos(Built& b)
{
    const SmCurve* cu = b.cos->GetUVCurve();
    const SmSurface* cs = b.cos->GetBaseSurface();
    ULONG owner = Peek::owner(b.cos);
    if (b.uv != NULL && !(cu == b.uv && owner > 0))
        delete b.uv;
    if (cs == b.srf && owner > 1)
        delete b.cos;
    else
    {
        delete b.srf;
        delete b.cos;
    }
    if (b.base_srf != b.srf && !b.offset_owns_base)
        delete b.base_srf;
    if (b.borrowed_nurb != NULL)
        smos_Free(b.borrowed_nurb);
    if (b.borrowed_uv_nurb != NULL)
        smos_Free(b.borrowed_uv_nurb);
}

// Deletes a copy made by the copy constructor, with the parts it does not delete itself.
void free_copy(SmCrvOnSurf* cp, const SmCrvOnSurf* orig)
{
    ULONG owner = Peek::owner(cp);
    const SmCurve* cu = cp->GetUVCurve();
    const SmSurface* cs = cp->GetBaseSurface();
    if (cu != orig->GetUVCurve() && owner == 0)
        delete cu;
    if (cs != orig->GetBaseSurface() && owner < 2)
        delete cs;
    delete cp;
}

SmExtent1d read_ivl(Rd& rd)
{
    double a = rd.num();
    double b = rd.num();
    return SmExtent1d(a, b);
}

void cos_op(int32_t op, Rd& rd, const SmContext& ctx, Built& bb, std::vector<double>& o)
{
    ScCos* c = bb.cos;
    switch (op)
    {
    case OP_INFO:
    {
        o.push_back(0.0);
        o.push_back((double)c->GetType());
        o.push_back((double)c->GetDim());
        o.push_back((double)c->GetDegree());
        SmExtent1d ivl = c->GetNaturalInterval();
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
        o.push_back(flg(c->IsAnalytic()));
        o.push_back(flg(c->IsBounded()));
        int64_t nt = rd.idx();
        for (int64_t i = 0; i < nt; i++)
            o.push_back(flg(c->IsKindOf((SM_TYPE)rd.idx())));
        put_state(o, c);
        break;
    }
    case OP_STATE:
    {
        o.push_back(0.0);
        put_state(o, c);
        break;
    }
    case OP_COPY_EVAL:
    {
        const double t = rd.num();
        const ULONG n = (ULONG)rd.idx();
        const bool left = rd.flag();
        const bool nz = rd.flag();
        SmCurve* copy = NULL;
        SmStatus s = c->Copy(ctx, copy);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            SmCrvOnSurf* cp = (SmCrvOnSurf*)copy;
            put_state(o, cp);
            std::vector<SmVector3d> v(n + 1);
            s = cp->Evaluate(t, n, left ? TRUE : FALSE, v.data(), nz ? TRUE : FALSE);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
                for (ULONG i = 0; i <= n; i++)
                    put_vec(o, v[i]);
            free_copy(cp, c);
        }
        break;
    }
    case OP_EVAL:
    {
        double t = rd.num();
        ULONG n = (ULONG)rd.idx();
        bool left = rd.flag();
        bool nz = rd.flag();
        std::vector<SmVector3d> v(n + 1);
        SmStatus s = c->Evaluate(t, n, left ? TRUE : FALSE, v.data(), nz ? TRUE : FALSE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            for (ULONG i = 0; i <= n; i++)
                put_vec(o, v[i]);
        break;
    }
    case OP_EVAL_POINT:
    {
        double t = rd.num();
        SmPoint3d p;
        SmStatus s = c->EvaluatePoint(t, p);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, p);
        break;
    }
    case OP_EVAL_UV:
    {
        double t = rd.num();
        SmPoint2d p;
        SmStatus s = c->EvaluateUVPoint(t, p);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(p.x);
            o.push_back(p.y);
        }
        break;
    }
    case OP_EVAL_NORMAL:
    {
        double t = rd.num();
        bool left = rd.flag();
        SmVector2d uv[2];
        SmVector3d nrm;
        SmStatus s = c->EvaluateSurfaceNormal(t, left ? TRUE : FALSE, uv, nrm);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(uv[0].x);
            o.push_back(uv[0].y);
            o.push_back(uv[1].x);
            o.push_back(uv[1].y);
            put_vec(o, nrm);
        }
        break;
    }
    case OP_EVAL_1ST:
    {
        double t = rd.num();
        ULONG n = (ULONG)rd.idx();
        bool left = rd.flag();
        ULONG sz = n + 1 < 2 ? 2 : n + 1;
        std::vector<SmVector3d> du(sz), dv(sz);
        SmStatus s = c->EvaluateSurface1stDerivDerivs(t, n, left ? TRUE : FALSE, du.data(), dv.data());
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            for (ULONG i = 0; i < 2; i++)
            {
                put_vec(o, du[i]);
                put_vec(o, dv[i]);
            }
        break;
    }
    case OP_KNOTS:
    {
        bool has_ivl = rd.flag();
        SmExtent1d ivl = read_ivl(rd);
        bool want = rd.flag();
        SmTArray<double> k;
        SmTArray<ULONG> m;
        SmStatus s = c->GetKnots(k, want ? &m : NULL, has_ivl ? &ivl : NULL);
        o.push_back(st(s));
        o.push_back((double)k.GetSize());
        for (ULONG i = 0; i < k.GetSize(); i++)
            o.push_back(k[i]);
        if (want)
        {
            o.push_back((double)m.GetSize());
            for (ULONG i = 0; i < m.GetSize(); i++)
                o.push_back((double)m[i]);
        }
        put_state(o, c);
        break;
    }
    case OP_NNK:
    {
        ULONG n = c->GetNumberNaturalKnots();
        o.push_back(0.0);
        o.push_back((double)n);
        put_state(o, c);
        break;
    }
    case OP_CONTS:
    {
        double angle = rd.num();
        SmContinuityType mn = SM_CT_UNDEFINED;
        SmTArray<SmContinuityType> cs;
        SmStatus s = c->CalculateContinuities(mn, cs, angle);
        o.push_back(st(s));
        o.push_back((double)mn);
        o.push_back((double)cs.GetSize());
        for (ULONG i = 0; i < cs.GetSize(); i++)
            o.push_back((double)cs[i]);
        put_state(o, c);
        break;
    }
    case OP_EQUAL:
    {
        bool self_cmp = rd.flag();
        if (self_cmp)
        {
            o.push_back(0.0);
            o.push_back(flg(*c == *(SmCurve*)c));
            break;
        }
        Built other = read_cos(rd, ctx);
        o.push_back(0.0);
        o.push_back(flg(*c == *(SmCurve*)other.cos));
        o.push_back(flg(*other.cos == *(SmCurve*)c));
        free_cos(other);
        break;
    }
    case OP_COPY:
    {
        SmCurve* cp = NULL;
        SmStatus s = c->Copy(ctx, cp);
        o.push_back(st(s));
        if (s == SM_SUCCESS && cp != NULL)
        {
            SmCrvOnSurf* cc = (SmCrvOnSurf*)cp;
            put_state(o, cc);
            o.push_back(flg(*cc == *(SmCurve*)c));
            free_copy(cc, c);
        }
        break;
    }
    case OP_EDIT_PARAM:
    {
        SmExtent1d ivl = read_ivl(rd);
        SmStatus s = c->EditParameterization(ivl);
        o.push_back(st(s));
        put_state(o, c);
        break;
    }
    case OP_REVERSE:
    {
        SmExtent1d ivl = read_ivl(rd);
        SmExtent1d nv(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        SmStatus s = c->ReverseParameterization(ivl, nv);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(nv.GetMin());
            o.push_back(nv.GetMax());
        }
        put_state(o, c);
        break;
    }
    case OP_TRIM:
    {
        SmExtent1d ivl = read_ivl(rd);
        int64_t mode = rd.idx();
        SmBoolean notify = mode == 0 ? TRUE : mode == 1 ? FALSE : UNSURE;
        SmStatus s = c->Trim(ivl, notify, FALSE);
        o.push_back(st(s));
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
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
    case OP_APPROX:
    {
        int64_t nb = rd.idx();
        SmTArray<double> br;
        for (int64_t i = 0; i < nb; i++)
            br.Add(rd.num());
        double tol = rd.num();
        bool match = rd.flag();
        double gap = SM_UNDEF_DOUBLE;
        SmBSplineCurve* nc = NULL;
        SmStatus s = c->ApproximateCurve(ctx, br, tol, gap, nc, TRUE, match ? TRUE : FALSE, FALSE);
        o.push_back(st(s));
        o.push_back(gap);
        o.push_back(nc ? 1.0 : 0.0);
        if (nc)
        {
            put_curve(o, nc);
            delete nc;
        }
        put_state(o, c);
        break;
    }
    case OP_GET_OR_CREATE:
    {
        double tol = rd.num();
        bool has_br = rd.flag();
        int64_t nb = rd.idx();
        SmTArray<double> br;
        for (int64_t i = 0; i < nb; i++)
            br.Add(rd.num());
        bool has_match = rd.flag();
        SmBoolean match = rd.flag() ? TRUE : FALSE;
        SmBSplineCurve* bs = NULL;
        SmStatus s = c->GetOrCreateBSApproxPointer(bs, tol, has_br ? &br : NULL, has_match ? &match : NULL);
        o.push_back(st(s));
        o.push_back(bs ? 1.0 : 0.0);
        if (bs)
            put_curve(o, bs);
        put_state(o, c);
        break;
    }
    case OP_BBOX:
    {
        SmExtent1d ivl = read_ivl(rd);
        SmExtent3d box;
        SmStatus s = c->CalculateBoundingBox(ivl, &box, NULL, NULL, TRUE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            put_vec(o, box.GetMin());
            put_vec(o, box.GetMax());
        }
        put_state(o, c);
        break;
    }
    case OP_DROP:
    {
        SmExtent1d ivl = read_ivl(rd);
        SmPoint3d p = rd.vec();
        bool has_vec = rd.flag();
        SmVector3d v = rd.vec();
        double tol = rd.num();
        bool has_guess = rd.flag();
        double guess = rd.num();
        SmSolverOperationType eop = (SmSolverOperationType)rd.idx();
        SmBoolean ok = FALSE;
        double par = 0.0, dist = 0.0;
        SmStatus s = c->DropPoint(ivl, p, has_vec ? &v : NULL, tol, has_guess ? &guess : NULL, ok, par, dist, eop);
        o.push_back(st(s));
        o.push_back(flg(ok));
        o.push_back(par);
        o.push_back(dist);
        put_state(o, c);
        break;
    }
    case OP_GAP:
    {
        bool self_other = rd.flag();
        Built other;
        const SmCurve* oc = c;
        if (!self_other)
        {
            other = read_cos(rd, ctx);
            oc = other.cos;
        }
        SmExtent1d ivl = read_ivl(rd);
        double s0 = rd.num();
        double s1 = rd.num();
        ULONG ns = (ULONG)rd.idx();
        double tol = rd.num();
        bool has_max = rd.flag();
        double mx = rd.num();
        double dist = 0.0, dev = 0.0, t1 = 0.0, t2 = 0.0, t3 = 0.0, t4 = 0.0;
        SmStatus s = c->SurfaceCurveMaxDistanceBetween(ivl, *oc, s0, s1, ns, tol, has_max ? &mx : NULL, dist, dev, &t1, &t2, &t3, &t4);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(dist);
            o.push_back(dev);
            o.push_back(t1);
            o.push_back(t2);
            o.push_back(t3);
            o.push_back(t4);
        }
        if (!self_other)
            free_cos(other);
        break;
    }
    case OP_ASSERT_VALID:
    {
        // `level with_list` (output: sc_assert_reports.h)
        SmAssertTestLevel level = (SmAssertTestLevel)rd.idx();
        bool with_list = rd.flag();
        SmAssertArray list;
        SmBoolean ok = c->AssertValid(with_list ? &list : NULL, level, SM_NO_WALK, NULL);
        o.push_back(0.0);
        sc::put_assert_reports(o, ok, list, (const SmCurve*)c);
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
        int64_t ft = rd.idx();
        ULONG version = (ULONG)rd.idx();
        int64_t nb = rd.idx();
        std::string bytes;
        for (int64_t i = 0; i < nb; i++)
            bytes.push_back((char)(unsigned char)rd.idx());
        std::istringstream iss(bytes);
        SmDatabaseIOStream db(ft == 0 ? SM_ASCII : SM_BINARY, &iss);
        SmCurve* c = NULL;
        SmStatus s = SmCrvOnSurf::ReadFromDB(SmCrvOnSurf_TYPE, db, 3, ctx, c, version);
        o.push_back(st(s));
        if (s == SM_SUCCESS && c != NULL)
        {
            o.push_back(1.0);
            put_state(o, (SmCrvOnSurf*)c);
            delete c;
        }
        else
            o.push_back(0.0);
        break;
    }
    case OP_WRITE_DB:
    {
        // generator helper: the bytes upstream WriteToDB produces (ASCII with 17 digits)
        int64_t ft = rd.idx();
        ULONG version = (ULONG)rd.idx();
        Built b = read_cos(rd, ctx);
        std::ostringstream oss;
        oss.precision(17);
        SmDatabaseIOStream db(ft == 0 ? SM_ASCII : SM_BINARY, &oss);
        SmStatus s = b.cos->WriteToDB(db, version);
        o.push_back(st(s));
        std::string bytes = oss.str();
        for (char ch : bytes)
            o.push_back((double)(unsigned char)ch);
        free_cos(b);
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_curve_crv_on_surf_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                            int64_t cap, int64_t* out_len)
{
    if (in == nullptr || n_in < 0 || out_len == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    Rd rd{in, 0};
    std::vector<double> o;
    if (op >= OP_READ_DB)
        static_op(op, rd, ctx, o);
    else
    {
        // object block, then nops ops (the leading op code only selects the object path)
        Built b = read_cos(rd, ctx);
        int64_t nops = rd.idx();
        for (int64_t i = 0; i < nops; i++)
        {
            int32_t code = (int32_t)rd.idx();
            cos_op(code, rd, ctx, b, o);
        }
        free_cos(b);
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
