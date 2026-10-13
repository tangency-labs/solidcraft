// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.projected_curve`: SmProjectedCurve
// (source/SMLib/inc/SmProjectedCurve.h, source/SMLib/src/SmProjectedCurve.cpp), called through
// `SmCurve*` where the method is virtual so the dispatch is upstream's.
//
// One entry point, sc_curve_projected_curve_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/curve_projected_curve.rs, which validates every
// input. Curve ops start with a projected-curve block: a base curve block (as unit
// curve.curve_p2: 0 = SmBSplineCurve `dim oob n p m cpts(x y z w) knots`, 1 = SmHermiteCurve
// `dim P1 D1 P2 D2`) then `ctor has_P P[3] has_V V[3] has_A A[3] type owns`.
// `out` receives the op status (0 = SM_SUCCESS, else the SmStatus value) and the outputs.
//
// The private members are read through the explicit-instantiation access rule (no upstream
// header is modified).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveTypes.h>
#include <SmDatabaseIO.h>
#include <SmExtent1d.h>
#include <SmHermiteCurve.h>
#include <SmProjectedCurve.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <sstream>
#include <string>
#include <vector>

namespace
{
// ---- private member access -------------------------------------------------------------
template <typename Tag, typename Tag::type M> struct Rob
{
    friend typename Tag::type get(Tag) { return M; }
};
struct PcCurve
{
    typedef const SmCurve* SmProjectedCurve::*type;
    friend type get(PcCurve);
};
struct PcPoint
{
    typedef SmPoint3d SmProjectedCurve::*type;
    friend type get(PcPoint);
};
struct PcVec
{
    typedef SmVector3d SmProjectedCurve::*type;
    friend type get(PcVec);
};
struct PcAux
{
    typedef SmVector3d SmProjectedCurve::*type;
    friend type get(PcAux);
};
struct PcType
{
    typedef SmProjectionType SmProjectedCurve::*type;
    friend type get(PcType);
};
struct PcOwns
{
    typedef SmBoolean SmProjectedCurve::*type;
    friend type get(PcOwns);
};
} // namespace
template struct Rob<PcCurve, &SmProjectedCurve::m_cpCurve>;
template struct Rob<PcPoint, &SmProjectedCurve::m_sProjPoint>;
template struct Rob<PcVec, &SmProjectedCurve::m_sProjVec>;
template struct Rob<PcAux, &SmProjectedCurve::m_sAuxData>;
template struct Rob<PcType, &SmProjectedCurve::m_eProjType>;
template struct Rob<PcOwns, &SmProjectedCurve::m_bOwnsCurve>;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/curve_projected_curve.rs).
enum : int32_t
{
    OP_STATE = 0,
    OP_MAKE_ORTHO = 1,
    OP_EVAL = 2,
    OP_EVAL_POINT = 3,
    OP_KNOTS = 4,
    OP_CONTINUITIES = 5,
    OP_EDIT_PARAM = 6,
    OP_REVERSE = 7,
    OP_TRIM = 8,
    OP_TRANSFORM = 9,
    OP_COPY = 10,
    OP_EQUAL = 11,
    OP_ENDS = 12,
    OP_READ_DB = 20,
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
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

void put_vec(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}

gw_CURVE* read_nurb(Rd& rd)
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

// A base curve block (see the file comment).
SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    ULONG dim = (ULONG)rd.idx();
    if (kind == 0)
    {
        bool oob = rd.flag();
        SmBSplineCurve* c = new (ctx) SmBSplineCurve(read_nurb(rd), dim, FALSE);
        c->SetOutOfBoundsEnabled(oob ? TRUE : FALSE);
        return c;
    }
    SmVector3d p1 = rd.vec();
    SmVector3d d1 = rd.vec();
    SmVector3d p2 = rd.vec();
    SmVector3d d2 = rd.vec();
    return new (ctx) SmHermiteCurve(p1, d1, p2, d2, dim);
}

// State of a base curve: 0 dim n p m cpts knots (B-spline) or 1 dim P(0) C'(0) P(1) C'(1)
// (Hermite, evaluated back); -1 for another type, -2 for none.
void put_base(std::vector<double>& o, const SmCurve* c)
{
    if (c == NULL)
        o.push_back(-2.0);
    else if (c->IsKindOf(SmBSplineCurve_TYPE))
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

// The members: P V A type owns, then the base curve state.
void put_state(std::vector<double>& o, const SmProjectedCurve* pc)
{
    put_vec(o, pc->*get(PcPoint()));
    put_vec(o, pc->*get(PcVec()));
    put_vec(o, pc->*get(PcAux()));
    o.push_back((double)(int)(pc->*get(PcType())));
    o.push_back((double)(int)(pc->*get(PcOwns())));
    put_base(o, pc->*get(PcCurve()));
}

// A projected-curve block: the base curve plus `ctor has_P P has_V V has_A A type owns`.
// ctor 0: the constructor; 1: SetCanonical on an empty curve (missing vectors are
// default-constructed). `base_out` receives the base curve when this code must delete it.
struct Built
{
    SmProjectedCurve* pc = NULL;
    SmCurve* own_base = NULL; // deleted by the caller (curve not owned by pc)
    SmStatus status = SM_SUCCESS;
};

Built read_projected(Rd& rd, const SmContext& ctx)
{
    Built b;
    SmCurve* base = read_curve(rd, ctx);
    int64_t ctor = rd.idx();
    bool hp = rd.flag();
    SmVector3d p = rd.vec();
    bool hv = rd.flag();
    SmVector3d v = rd.vec();
    bool ha = rd.flag();
    SmVector3d a = rd.vec();
    SmProjectionType type = (SmProjectionType)rd.idx();
    SmBoolean owns = (SmBoolean)rd.idx();
    SmVector3d undef;
    if (ctor == 0)
        b.pc = new (ctx) SmProjectedCurve(base, hp ? &p : NULL, hv ? &v : NULL, ha ? &a : NULL, type, owns, &ctx);
    else
    {
        b.pc = new (ctx) SmProjectedCurve(NULL, NULL, NULL, NULL, SM_PT_UNKNOWN, FALSE, &ctx);
        b.status = b.pc->SetCanonical(*base, hp ? p : undef, hv ? v : undef, ha ? a : undef, type, owns);
    }
    if (owns == FALSE)
        b.own_base = base;
    return b;
}

void free_built(Built& b)
{
    delete b.pc;
    delete b.own_base;
}

void curve_op(int32_t op, Rd& rd, const SmContext& ctx, Built& b, std::vector<double>& o)
{
    SmProjectedCurve* pc = b.pc;
    SmCurve* c = pc;
    switch (op)
    {
    case OP_STATE:
    {
        SmExtent1d ivl = c->GetNaturalInterval();
        o.push_back(st(b.status));
        put_state(o, pc);
        o.push_back((double)c->GetType());
        o.push_back((double)c->GetDim());
        o.push_back((double)c->GetDegree());
        o.push_back((double)c->GetNumberNaturalKnots());
        o.push_back((double)c->GetNumberControlPoints());
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
        o.push_back(flg(c->IsAnalytic()));
        o.push_back(flg(c->IsBounded()));
        o.push_back(flg(c->IsRational()));
        const SM_TYPE kinds[] = {SmCurve_TYPE, SmProjectedCurve_TYPE, SmBSplineCurve_TYPE, SmOffsetCurve_TYPE,
                                 12060,        12000,                 1};
        for (SM_TYPE k : kinds)
            o.push_back(flg(c->IsKindOf(k)));
        break;
    }
    case OP_MAKE_ORTHO:
    {
        SmStatus s = pc->MakeOrthoNormal();
        o.push_back(st(s));
        put_state(o, pc);
        break;
    }
    case OP_EVAL:
    {
        double t = rd.num();
        ULONG n = (ULONG)rd.idx();
        bool left = rd.flag();
        bool nz = rd.flag();
        SmVector3d v[8];
        SmStatus s = c->Evaluate(t, n, left ? TRUE : FALSE, v, nz ? TRUE : FALSE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            for (ULONG i = 0; i <= n; i++)
                put_vec(o, v[i]);
        break;
    }
    case OP_EVAL_POINT:
    {
        SmPoint3d p;
        SmStatus s = c->EvaluatePoint(rd.num(), p);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, p);
        break;
    }
    case OP_KNOTS:
    {
        bool has = rd.flag();
        double a = rd.num();
        double bb = rd.num();
        SmExtent1d ivl(a, bb);
        SmTArray<double> k;
        SmTArray<ULONG> m;
        SmStatus s = c->GetKnots(k, &m, has ? &ivl : NULL);
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
    case OP_CONTINUITIES:
    {
        double ang = rd.num();
        SmContinuityType mn = SM_CT_UNDEFINED;
        SmTArray<SmContinuityType> all;
        all.Add(SM_CT_C0); // must be cleared by the call
        SmStatus s = c->CalculateContinuities(mn, all, ang);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)(int)mn);
            o.push_back((double)all.GetSize());
            for (ULONG i = 0; i < all.GetSize(); i++)
                o.push_back((double)(int)all[i]);
        }
        break;
    }
    case OP_EDIT_PARAM:
    {
        double a = rd.num();
        double bb = rd.num();
        SmStatus s = c->EditParameterization(SmExtent1d(a, bb));
        o.push_back(st(s));
        put_state(o, pc);
        break;
    }
    case OP_REVERSE:
    {
        double a = rd.num();
        double bb = rd.num();
        SmExtent1d nv(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        SmStatus s = c->ReverseParameterization(SmExtent1d(a, bb), nv);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(nv.GetMin());
            o.push_back(nv.GetMax());
        }
        put_state(o, pc);
        break;
    }
    case OP_TRIM:
    {
        double a = rd.num();
        double bb = rd.num();
        int64_t mode = rd.idx();
        SmExtent1d ivl(a, bb);
        SmBoolean notify = mode == 0 ? TRUE : mode == 1 ? FALSE : UNSURE;
        SmStatus s = c->Trim(ivl, notify);
        o.push_back(st(s));
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
        put_state(o, pc);
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
        put_state(o, pc);
        break;
    }
    case OP_COPY:
    {
        SmCurve* nc = NULL;
        SmStatus s = c->Copy(ctx, nc);
        if (s == SM_SUCCESS && nc != NULL && ((SmProjectedCurve*)nc)->*get(PcCurve()) == NULL)
        {
            // the owned base curve has no Copy: upstream returns a curve without base
            // curve (the port reports the base curve's SM_ERR)
            o.push_back(st(SM_ERR));
        }
        else
        {
            o.push_back(st(s));
            if (s == SM_SUCCESS)
            {
                const SmProjectedCurve* np = (const SmProjectedCurve*)nc;
                put_state(o, np);
                // a borrowed base curve is shared, an owned one copied
                o.push_back(flg(np->*get(PcCurve()) == pc->*get(PcCurve())));
                o.push_back(flg(*nc == *c));
            }
        }
        delete nc;
        break;
    }
    case OP_EQUAL:
    {
        bool self = rd.flag();
        o.push_back(0.0);
        if (self)
            o.push_back(flg(*c == *c));
        else
        {
            Built b2 = read_projected(rd, ctx);
            o.push_back(flg(*c == *b2.pc));
            free_built(b2);
        }
        break;
    }
    case OP_ENDS:
    {
        SmPoint3d a, bb;
        double wa = SM_UNDEF_DOUBLE, wb = SM_UNDEF_DOUBLE;
        c->GetEnds(a, bb, &wa, &wb);
        o.push_back(0.0);
        put_vec(o, a);
        put_vec(o, bb);
        o.push_back(wa);
        o.push_back(wb);
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}

// ReadDb: `version binary nb B[nb]` -> status [state].
void read_db(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    ULONG version = (ULONG)rd.idx();
    bool binary = rd.flag();
    int64_t nb = rd.idx();
    std::string bytes;
    for (int64_t i = 0; i < nb; i++)
        bytes.push_back((char)(unsigned char)rd.idx());
    std::istringstream iss(bytes);
    SmDatabaseIOStream db(binary ? SM_BINARY : SM_ASCII, &iss);
    SmCurve* c = NULL;
    SmStatus s = SmProjectedCurve::ReadFromDB(SmProjectedCurve_TYPE, db, 3, ctx, c, version);
    o.push_back(st(s));
    if (s == SM_SUCCESS && c != NULL)
    {
        put_state(o, (const SmProjectedCurve*)c);
        o.push_back((double)c->GetType());
    }
    delete c;
}
} // namespace

SC_EXPORT int32_t sc_curve_projected_curve_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                                int64_t cap, int64_t* out_len)
{
    if (in == nullptr || n_in < 0 || out_len == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    Rd rd{in, 0};
    std::vector<double> o;
    if (op == OP_READ_DB)
        read_db(rd, ctx, o);
    else
    {
        Built b = read_projected(rd, ctx);
        curve_op(op, rd, ctx, b, o);
        free_built(b);
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
