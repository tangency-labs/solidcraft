// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.crv_in_volume`: the SmCrvInVolume members owned by the unit
// (source/SMLib/src/SmCrvInVolume.cpp, source/SMLib/inc/SmCrvInVolume.h), called through
// SmCrvInVolume / SmCurve on objects built from an SmBSplineCurve or SmHermiteCurve base
// curve and an identity SmTransform volume (the simplest SmVolume; the Rust side builds
// the same volume and runs every op through it).
//
// The static op OP_EXACT builds its own object from an SmBSplineCurve, SmHermiteCurve, SmLine
// or SmCircle base curve and a chain of SmTransform / SmTwistVolume / SmUnbendVolume /
// SmBendVolume / plain SmVolume maps (as the surface.srf_in_volume shim) and runs
// MakeExactBSplineIfPossible.
//
// One entry point, sc_curve_crv_in_volume_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/curve_crv_in_volume.rs, which validates every
// input. `out` receives the op status (0 = SM_SUCCESS, else the SmStatus value) and the op's
// outputs.
//
// The break cache (m_bNeedBreaks, m_sBreaks, m_sConts) and the owner flag are protected:
// ScCiv, a derived class adding nothing but accessors, loads and reads them (the cache as
// ReadFromDB would).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBendVolume.h>
#include <SmBSplineCurve.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmCrvInVolume.h>
#include <SmCurve.h>
#include <SmCurveTypes.h>
#include <SmDatabaseIO.h>
#include <SmExtent1d.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmTArray.h>
#include <SmTransform.h>
#include <SmTwistVolume.h>
#include <SmUnbendVolume.h>
#include <SmVector3d.h>
#include <SmVolume.h>
#include <nurbs.h>

#include <sstream>
#include <string>
#include <vector>

#include "sc_sm.h"
#include "sc_gw.h"

using sc::Rd;
using sc::put_vec;
using sc::read_nurb;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/curve_crv_in_volume.rs).
enum : int32_t
{
    OP_INFO = 0,
    OP_EVAL = 1,
    OP_EVAL_POINT = 2,
    OP_KNOTS = 3,
    OP_MULTS = 4,
    OP_NNK = 5,
    OP_CONTS = 6,
    OP_EQUAL = 7,
    OP_COPY = 8,
    OP_EDIT_PARAM = 9,
    OP_REVERSE = 10,
    OP_TRIM = 11,
    OP_TRANSFORM = 12,
    OP_READ_DB = 40,
    OP_EXACT = 41,
};

// Map kinds of an OP_EXACT volume chain (as the surface.srf_in_volume shim).
enum : int64_t
{
    K_BASE = 0,
    K_TRANSFORM = 1,
    K_TWIST = 2,
    K_UNBEND = 3,
    K_BEND = 4,
    K_TWIST_C = 5,
    K_UNBEND_C = 6,
    K_BEND_C = 7,
};

class ScCiv : public SmCrvInVolume
{
  public:
    ScCiv(SmCurve& c, SmBoolean bInParam, SmVolume& v, ULONG lCopy, ULONG lOwner,
          const SmContext* ctx)
        : SmCrvInVolume(c, bInParam, v, lCopy, lOwner, ctx)
    {
    }
    void SetCache(SmBoolean bNeed, const SmTArray<double>& b, const SmTArray<SmContinuityType>& c)
    {
        m_bNeedBreaks = bNeed;
        m_sBreaks = b;
        m_sConts = c;
    }
};

// Read access to the protected members of any SmCrvInVolume.
struct Peek : public SmCrvInVolume
{
    static ULONG owner(const SmCrvInVolume* p) { return ((const Peek*)p)->m_lOwnerFlag; }
    static SmBoolean need(const SmCrvInVolume* p) { return ((const Peek*)p)->m_bNeedBreaks; }
    static const SmTArray<double>& breaks(const SmCrvInVolume* p)
    {
        return ((const Peek*)p)->m_sBreaks;
    }
    static const SmTArray<SmContinuityType>& conts(const SmCrvInVolume* p)
    {
        return ((const Peek*)p)->m_sConts;
    }
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }

// A curve block `0 dim oob n p m cpts knots` or `1 dim P1 D1 P2 D2` (`2`: kind 0 with the
// given knots written back over the curve's own NURB after construction).
SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    ULONG dim = (ULONG)rd.idx();
    if (kind == 0 || kind == 2)
    {
        bool oob = rd.flag();
        SmBSplineCurve* c = new (ctx) SmBSplineCurve(read_nurb(rd), dim, FALSE);
        if (kind == 2)
        {
            // write the given knots back over the fixed-up NURB
            gw_CURVE* own = c->GetGwNurbPointer();
            gw_INDEX m = own->knt->m;
            for (gw_INDEX i = 0; i <= m; i++)
                own->knt->U[i] = rd.in[rd.at - (int64_t)(m + 1) + (int64_t)i];
        }
        c->SetOutOfBoundsEnabled(oob ? TRUE : FALSE);
        return c;
    }
    SmVector3d p1 = rd.vec();
    SmVector3d d1 = rd.vec();
    SmVector3d p2 = rd.vec();
    SmVector3d d2 = rd.vec();
    return new (ctx) SmHermiteCurve(p1, d1, p2, d2, dim);
}

// State of a base curve: 0 dim n p m cpts knots (B-spline), 1 dim P(0) C'(0) P(1) C'(1)
// (Hermite), -1 otherwise.
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    if (c->IsKindOf(SmBSplineCurve_TYPE))
    {
        const SmBSplineCurve* b = (const SmBSplineCurve*)c;
        o.push_back(0.0);
        o.push_back((double)b->GetDim());
        const gw_CURVE* cur = b->GetGwNurbPointer();
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
    else if (c->IsKindOf(SmHermiteCurve_TYPE))
    {
        SmVector3d a[2], b[2];
        c->Evaluate(0.0, 1, TRUE, a, FALSE);
        c->Evaluate(1.0, 1, TRUE, b, FALSE);
        o.push_back(1.0);
        o.push_back((double)c->GetDim());
        put_vec(o, a[0]);
        put_vec(o, a[1]);
        put_vec(o, b[0]);
        put_vec(o, b[1]);
    }
    else
        o.push_back(-1.0);
}

// Cache state: need nb B[nb] nc C[nc].
void put_cache(std::vector<double>& o, const SmCrvInVolume* c)
{
    o.push_back(Peek::need(c) ? 1.0 : 0.0);
    const SmTArray<double>& b = Peek::breaks(c);
    o.push_back((double)b.GetSize());
    for (ULONG i = 0; i < b.GetSize(); i++)
        o.push_back(b[i]);
    const SmTArray<SmContinuityType>& k = Peek::conts(c);
    o.push_back((double)k.GetSize());
    for (ULONG i = 0; i < k.GetSize(); i++)
        o.push_back((double)k[i]);
}

// Full state: owner_flag in_param cache curve.
void put_state(std::vector<double>& o, const SmCrvInVolume* c)
{
    o.push_back((double)Peek::owner(c));
    o.push_back(c->GetInParamSpace() ? 1.0 : 0.0);
    put_cache(o, c);
    put_curve(o, c->GetCurve());
}

// An SmCrvInVolume built from a block, with its original parts (freed by `free_civ`).
struct Built
{
    SmCurve* curve = NULL;
    SmTransform* volume = NULL;
    ScCiv* civ = NULL;
};

// Block: curve block, in_param copy_flag owner_flag, has_cache [need nb B nc C].
Built read_civ(Rd& rd, const SmContext& ctx)
{
    Built b;
    b.curve = read_curve(rd, ctx);
    bool in_param = rd.flag();
    ULONG copy = (ULONG)rd.idx();
    ULONG owner = (ULONG)rd.idx();
    b.volume = new (ctx) SmTransform(&ctx);
    b.civ = new (ctx) ScCiv(*b.curve, in_param ? TRUE : FALSE, *b.volume, copy, owner, &ctx);
    if (rd.flag())
    {
        bool need = rd.flag();
        int64_t nb = rd.idx();
        SmTArray<double> br;
        for (int64_t i = 0; i < nb; i++)
            br.Add(rd.num());
        int64_t nc = rd.idx();
        SmTArray<SmContinuityType> co;
        for (int64_t i = 0; i < nc; i++)
            co.Add((SmContinuityType)rd.idx());
        b.civ->SetCache(need ? TRUE : FALSE, br, co);
    }
    return b;
}

// Frees the parts the object does not delete itself (before it, while it is alive as
// their owner), then the object.
void free_civ(Built& b)
{
    SmCurve* cc = b.civ->GetCurve();
    SmVolume* cv = b.civ->GetVolume();
    bool civ_deletes_curve = b.civ->OwnsCurve() != FALSE;
    bool civ_deletes_volume = b.civ->OwnsVolume() != FALSE;
    if (cc != b.curve && !civ_deletes_curve)
        delete cc;
    if (!(cc == b.curve && civ_deletes_curve))
        delete b.curve;
    if (cv != b.volume && !civ_deletes_volume)
        delete cv;
    if (!(cv == b.volume && civ_deletes_volume))
        delete b.volume;
    delete b.civ;
}

void civ_op(int32_t op, Rd& rd, const SmContext& ctx, Built& bb, std::vector<double>& o)
{
    ScCiv* c = bb.civ;
    switch (op)
    {
    case OP_INFO:
    {
        o.push_back(0.0);
        o.push_back((double)c->GetType());
        o.push_back((double)c->GetDim());
        o.push_back((double)c->GetDegree());
        o.push_back((double)c->GetNumberControlPoints());
        SmExtent1d ivl = c->GetNaturalInterval();
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
        o.push_back(c->IsAnalytic() ? 1.0 : 0.0);
        o.push_back(c->IsBounded() ? 1.0 : 0.0);
        o.push_back(c->OwnsCurve() ? 1.0 : 0.0);
        o.push_back(c->OwnsVolume() ? 1.0 : 0.0);
        int64_t nt = rd.idx();
        for (int64_t i = 0; i < nt; i++)
            o.push_back(c->IsKindOf((SM_TYPE)rd.idx()) ? 1.0 : 0.0);
        put_state(o, c);
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
    case OP_KNOTS:
    {
        bool has_ivl = rd.flag();
        double a = rd.num();
        double b = rd.num();
        bool want = rd.flag();
        SmExtent1d ivl(a, b);
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
        put_cache(o, c);
        break;
    }
    case OP_MULTS:
    {
        int64_t nk = rd.idx();
        SmTArray<double> k;
        for (int64_t i = 0; i < nk; i++)
            k.Add(rd.num());
        SmTArray<ULONG> m;
        SmStatus s = c->GenerateMults(k, m);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            for (ULONG i = 0; i < m.GetSize(); i++)
                o.push_back((double)m[i]);
        break;
    }
    case OP_NNK:
    {
        ULONG n = c->GetNumberNaturalKnots();
        o.push_back(0.0);
        o.push_back((double)n);
        put_cache(o, c);
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
        put_cache(o, c);
        break;
    }
    case OP_EQUAL:
    {
        bool self_cmp = rd.flag();
        if (self_cmp)
        {
            o.push_back(0.0);
            o.push_back((*c == *(SmCurve*)c) ? 1.0 : 0.0);
            break;
        }
        Built other = read_civ(rd, ctx);
        o.push_back(0.0);
        o.push_back((*c == *(SmCurve*)other.civ) ? 1.0 : 0.0);
        free_civ(other);
        break;
    }
    case OP_COPY:
    {
        SmCurve* cp = NULL;
        SmStatus s = c->Copy(ctx, cp);
        o.push_back(st(s));
        if (s == SM_SUCCESS && cp != NULL)
        {
            SmCrvInVolume* cc = (SmCrvInVolume*)cp;
            put_state(o, cc);
            o.push_back(sc::flg(*cp == *c));
            // the copy deletes what it owns; free the copies it does not own (they are
            // never the originals when copied)
            SmCurve* ccur = cc->GetCurve();
            SmVolume* cvol = cc->GetVolume();
            bool del_c = cc->OwnsCurve() != FALSE;
            bool del_v = cc->OwnsVolume() != FALSE;
            bool cur_copied = ccur != c->GetCurve();
            bool vol_copied = cvol != c->GetVolume();
            if (cur_copied && !del_c)
                delete ccur;
            if (vol_copied && !del_v)
                delete cvol;
            delete cc;
        }
        break;
    }
    case OP_EDIT_PARAM:
    {
        double a = rd.num();
        double b = rd.num();
        SmStatus s = c->EditParameterization(SmExtent1d(a, b));
        o.push_back(st(s));
        put_state(o, c);
        break;
    }
    case OP_REVERSE:
    {
        double a = rd.num();
        double b = rd.num();
        SmExtent1d nv(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        SmStatus s = c->ReverseParameterization(SmExtent1d(a, b), nv);
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
        double a = rd.num();
        double b = rd.num();
        int64_t mode = rd.idx();
        SmBoolean notify = mode == 0 ? TRUE : mode == 1 ? FALSE : UNSURE;
        SmExtent1d ivl(a, b);
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
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}

// ---- OP_EXACT ---------------------------------------------------------------------------

// An exact-op curve block: a curve block (kinds 0, 1), `2 P1 P2` (SmLine(P1, P2, 3)) or
// `3 C X Y a0 a1 r` (SmCircle(C, X, Y, SmExtent1d(a0, a1), r, 3)).
SmCurve* read_exact_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 2)
    {
        SmPoint3d p1 = rd.vec();
        SmPoint3d p2 = rd.vec();
        return new (ctx) SmLine(p1, p2, 3, &ctx);
    }
    if (kind == 3)
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double a0 = rd.num();
        double a1 = rd.num();
        double r = rd.num();
        return new (ctx) SmCircle(c, x, y, SmExtent1d(a0, a1), r, 3, &ctx);
    }
    rd.at--; // the kind belongs to the plain curve block
    return read_curve(rd, ctx);
}

// `origin X Y Z`: SmTransform(ctx, origin, X, Y, &Z).
SmTransform* read_transform(Rd& rd, const SmContext& ctx)
{
    SmVector3d o = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    SmVector3d z = rd.vec();
    return new (ctx) SmTransform(&ctx, o, x, y, &z);
}

// `n` maps `kind [T(12) | C(11)] has_orient [O(12)]` (kinds 5-7 are the component
// constructors with `C` = `p0 axis mid a b`, and never have an orient map here), chained
// in order with AddNextMap(m, 2).
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

// An exact result: `type dim had_nurb` then the NURB after GetOrCreateGwNurbPointer
// (`n p m cpts(x y z w) knots`, or `-1`), the natural interval `a b`, and
// `st P[3] T[3]` of Evaluate(t, 1, TRUE, .., TRUE) at the interval fractions 0, 1/4,
// 1/2, 3/4, 1.
void put_exact_curve(std::vector<double>& o, SmBSplineCurve* c)
{
    o.push_back((double)c->GetType());
    o.push_back((double)c->GetDim());
    o.push_back(c->GetGwNurbPointer() != NULL ? 1.0 : 0.0);
    const gw_CURVE* cur = c->GetOrCreateGwNurbPointer();
    if (cur == NULL)
        o.push_back(-1.0);
    else
    {
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
    SmExtent1d ivl = c->GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    for (int i = 0; i <= 4; i++)
    {
        SmVector3d pn[2];
        SmStatus s = c->Evaluate(ivl.Evaluate(0.25 * i), 1, TRUE, pn, TRUE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            put_vec(o, pn[0]);
            put_vec(o, pn[1]);
        }
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
        SmCurve* c = NULL;
        SmStatus s = SmCrvInVolume::ReadFromDB(SmCrvInVolume_TYPE, db, 3, ctx, c, version);
        o.push_back(st(s));
        if (s == SM_SUCCESS && c != NULL)
        {
            o.push_back(1.0);
            put_state(o, (SmCrvInVolume*)c);
            delete c;
        }
        else
            o.push_back(0.0);
        break;
    }
    case OP_EXACT:
    {
        // the object owns (and deletes) the curve and the volume chain
        SmCurve* curve = read_exact_curve(rd, ctx);
        bool in_param = rd.flag();
        SmVolume* volume = read_volume(rd, ctx);
        SmCrvInVolume* civ =
            new (ctx) SmCrvInVolume(*curve, in_param ? TRUE : FALSE, *volume, 0, 3, &ctx);
        SmBSplineCurve* b = NULL;
        SmStatus s = civ->MakeExactBSplineIfPossible(b);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(b != NULL ? 1.0 : 0.0);
            if (b != NULL)
                put_exact_curve(o, b);
        }
        delete b;
        delete civ;
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_curve_crv_in_volume_eval(int32_t op, const double* in, int64_t n_in,
                                              double* out, int64_t cap, int64_t* out_len)
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
        Built b = read_civ(rd, ctx);
        civ_op(op, rd, ctx, b, o);
        free_civ(b);
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
