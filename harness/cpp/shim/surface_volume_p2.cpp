// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.volume_p2`: the SmVolume members owned by the unit
// (source/SMLib/src/SmVolume.cpp, source/SMLib/inc/SmVolume.h), called on compound volumes
// built from plain SmVolume, SmTransform, SmTwistVolume (component constructor),
// SmBSplineVolume (from an sm_AllocateNurbVolume record), default and component-constructed
// SmUnbendVolume and SmBendVolume maps, each but the twist and the component unbend and bend
// optionally oriented by an SmTransform; plus SmVolume::ReadFromDB of ASCII text.
//
// One entry point, sc_surface_volume_p2_eval(op, in, n_in, data, data_len, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/surface_volume_p2.rs, which
// validates every input. `out` receives the op status (0 = SM_SUCCESS, else the SmStatus
// value) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineVolume.h>
#include <SmContext.h>
#include <SmDatabaseIO.h>
#include <SmExtent1d.h>
#include <SmBendVolume.h>
#include <SmExtent3d.h>
#include <SmMemory.h>
#include <SmNurbsVol.h>
#include <SmPseudoBox.h>
#include <SmSolutionArray.h>
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

using sc::Rd;
using sc::put_vec;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_volume_p2.rs).
enum : int32_t
{
    OP_PT_IN_PARAM = 0,
    OP_LINE_IN_INSPACE = 1,
    OP_BOX_IN_PARAM = 2,
    OP_PBOX_IN_INSPACE = 3,
    OP_EVAL_BBOX = 4,
    OP_MAP_PBOX = 5,
    OP_EVAL_POINT = 6,
    OP_EVAL_1ST = 7,
    OP_INV_EVAL = 8,
    OP_INV_MAP = 9,
    OP_GLOBAL = 10,
    OP_HAS_DISC = 11,
    OP_BBOX_SIMPLE = 12,
    OP_PBOX_SIMPLE = 13,
    OP_INV_EVAL_SIMPLE = 14,
    OP_GLOBAL_SIMPLE = 15,
    OP_LOCAL_SIMPLE = 16,
    OP_READ_DB = 20,
};

// Map kinds of a volume block.
enum : int64_t
{
    K_BASE = 0,
    K_TRANSFORM = 1,
    K_TWIST = 2,
    K_BSPLINE = 3,
    K_UNBEND = 4,
    K_UNBEND_C = 5,
    K_BEND = 6,
    K_BEND_C = 7,
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

void put_mat(std::vector<double>& o, const SmTransform& t)
{
    SmTransform& m = (SmTransform&)t;
    for (int i = 0; i < 4; ++i)
        for (int j = 0; j < 4; ++j)
            o.push_back(m[i][j]);
}

void put_box(std::vector<double>& o, const SmExtent3d& b)
{
    put_vec(o, b.GetMin());
    put_vec(o, b.GetMax());
}

void put_pbox(std::vector<double>& o, const SmPseudoBox& b)
{
    for (ULONG i = 0; i < 3; ++i)
        put_vec(o, b.GetBasis(i));
    for (ULONG i = 0; i < 3; ++i)
    {
        o.push_back(b.GetInterval(i).GetMin());
        o.push_back(b.GetInterval(i).GetMax());
    }
}

void put_solution(std::vector<double>& o, const SmSolution& s)
{
    o.push_back((double)s.m_eSolutionType);
    o.push_back((double)s.m_lNumVariables);
    o.push_back(s.m_vStart.m_dSolutionValue);
    o.push_back(s.m_vStart.m_adParameters[0]);
    o.push_back(s.m_vStart.m_adParameters[1]);
    o.push_back(s.m_vStart.m_adParameters[2]);
    o.push_back((double)s.m_lNumObjects);
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

// `l m n p q r`, knots U V W, control points (x y z w), w fastest.
SmVolume* read_bspline(Rd& rd, const SmContext& ctx)
{
    long l = (long)rd.idx(), m = (long)rd.idx(), n = (long)rd.idx();
    gw_DEGREE p = (gw_DEGREE)rd.idx(), q = (gw_DEGREE)rd.idx(), r = (gw_DEGREE)rd.idx();
    long ku = l + p + 1, kv = m + q + 1, kw = n + r + 1;
    gw_VOLUME* rec = sm_AllocateNurbVolume(l, m, n, p, q, r, ku, kv, kw);
    if (rec == NULL)
        return NULL;
    for (long i = 0; i <= ku; i++)
        rec->knu->U[i] = rd.num();
    for (long i = 0; i <= kv; i++)
        rec->knv->U[i] = rd.num();
    for (long i = 0; i <= kw; i++)
        rec->knw->U[i] = rd.num();
    for (long i = 0; i <= l; i++)
        for (long j = 0; j <= m; j++)
            for (long k = 0; k <= n; k++)
            {
                NL_CPOINT& c = rec->mesh->Pw[i][j][k];
                c.x = rd.num();
                c.y = rd.num();
                c.z = rd.num();
                c.w = rd.num();
            }
    SmVolume* v = new (ctx) SmBSplineVolume(&ctx, rec);
    smos_Free(rec);
    return v;
}

// `n` maps `kind [data] [has_orient [O(12)]]` (no orient flag for a twist or a component
// unbend or bend), chained in
// order with AddNextMap(m, 2).
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
        case K_TWIST:
        {
            SmPoint3d o = rd.vec();
            SmVector3d axis = rd.vec();
            SmVector3d mid = rd.vec();
            double period = rd.num();
            double min_dist = rd.num();
            v = new (ctx) SmTwistVolume(ctx, o, axis, mid, period, min_dist);
            break;
        }
        case K_BSPLINE: v = read_bspline(rd, ctx); break;
        case K_UNBEND: v = new (ctx) SmUnbendVolume(&ctx); break;
        case K_UNBEND_C:
        {
            SmPoint3d c = rd.vec();
            SmVector3d axis = rd.vec();
            SmVector3d mid = rd.vec();
            double neutral = rd.num();
            double tol = rd.num();
            v = new (ctx) SmUnbendVolume(ctx, c, axis, mid, neutral, tol);
            break;
        }
        case K_BEND: v = new (ctx) SmBendVolume(&ctx); break;
        case K_BEND_C:
        {
            SmPoint3d c = rd.vec();
            SmVector3d axis = rd.vec();
            SmVector3d mid = rd.vec();
            double neutral = rd.num();
            double tol = rd.num();
            v = new (ctx) SmBendVolume(ctx, c, axis, mid, neutral, tol);
            break;
        }
        default: v = new (ctx) SmVolume(&ctx); break;
        }
        if (v == NULL)
        {
            delete head;
            return NULL;
        }
        // a twist's or component unbend's or bend's frame is its orient map: no orient
        // flag in its block
        if (kind != K_TWIST && kind != K_UNBEND_C && kind != K_BEND_C && rd.flag())
            v->SetOrientMap(read_transform(rd, ctx), 2);
        if (head == NULL)
            head = v;
        else
            head->AddNextMap(v, 2);
    }
    return head;
}

// `min max`: SmExtent3d(min, max).
SmExtent3d read_box(Rd& rd)
{
    SmVector3d a = rd.vec();
    SmVector3d b = rd.vec();
    return SmExtent3d(a, b);
}

// `X Y Z ivl0 ivl1 ivl2`: SmPseudoBox(X, Y, Z, SmExtent1d(a, b) x3).
SmPseudoBox read_pbox(Rd& rd)
{
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    SmVector3d z = rd.vec();
    double a0 = rd.num(), b0 = rd.num();
    double a1 = rd.num(), b1 = rd.num();
    double a2 = rd.num(), b2 = rd.num();
    SmExtent1d i0(a0, b0), i1(a1, b1), i2(a2, b2);
    return SmPseudoBox(x, y, z, i0, i1, i2);
}

// The chain: `n` then per map `type [matrix(16) | period min | neutral tol] has_orient [O(16)]
// has_inv [I(16)]`.
void put_state(std::vector<double>& o, const SmVolume* v)
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
        if (p->GetType() == SmTwistVolume_TYPE)
        {
            o.push_back(((const SmTwistVolume*)p)->GetTwistPeriodDist());
            o.push_back(((const SmTwistVolume*)p)->GetPeriodMinDist());
        }
        if (p->GetType() == SmUnbendVolume_TYPE)
        {
            o.push_back(((const SmUnbendVolume*)p)->GetUnbendNeutralDist());
            o.push_back(((const SmUnbendVolume*)p)->GetSingularityTol());
        }
        if (p->GetType() == SmBendVolume_TYPE)
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

void put_cont_dump(std::vector<double>& o, const SmDiscontinuities3d& d)
{
    o.push_back(flg(d.m_bHasDiscontinuities));
    o.push_back((double)d.m_eMinContinuity);
    o.push_back(flg(d.m_bUnalignedU));
    o.push_back((double)d.m_eMinContU);
    o.push_back((double)d.m_sParamsU.GetSize());
    o.push_back(flg(d.m_bUnalignedV));
    o.push_back((double)d.m_eMinContV);
    o.push_back((double)d.m_sParamsV.GetSize());
    o.push_back(flg(d.m_bUnalignedW));
    o.push_back((double)d.m_eMinContW);
    o.push_back((double)d.m_sParamsW.GetSize());
}

void put_inv(std::vector<double>& o, SmStatus s, SmBoolean ok, const SmTArray<SmPoint3d>& pts,
             const SmTArray<double>& gaps)
{
    o.push_back(st(s));
    if (s != SM_SUCCESS)
        return;
    o.push_back(flg(ok));
    o.push_back((double)pts.GetSize());
    for (ULONG i = 0; i < pts.GetSize(); ++i)
        put_vec(o, pts[i]);
    o.push_back((double)gaps.GetSize());
    for (ULONG i = 0; i < gaps.GetSize(); ++i)
        o.push_back(gaps[i]);
}

void put_solutions(std::vector<double>& o, SmStatus s, SmBoolean found, SmSolutionArray& sols)
{
    o.push_back(st(s));
    if (s != SM_SUCCESS)
        return;
    o.push_back(flg(found));
    o.push_back((double)sols.GetSize());
    for (ULONG i = 0; i < sols.GetSize(); ++i)
        put_solution(o, sols[i]);
}

void volume_op(int32_t op, Rd& rd, SmVolume* v, std::vector<double>& o)
{
    switch (op)
    {
    case OP_PT_IN_PARAM:
    {
        SmVector3d p = rd.vec();
        bool comp = rd.flag();
        o.push_back(0.0);
        o.push_back(flg(v->IsPointInParamDomain(p, comp)));
        break;
    }
    case OP_LINE_IN_INSPACE:
    {
        SmVector3d a = rd.vec();
        SmVector3d b = rd.vec();
        bool comp = rd.flag();
        SmPoint3d pa, pb;
        SmBoolean in = v->IsLineInInSpaceDomain(a, b, &pa, &pb, comp);
        o.push_back(0.0);
        o.push_back(flg(in));
        put_vec(o, pa);
        put_vec(o, pb);
        break;
    }
    case OP_BOX_IN_PARAM:
    {
        SmExtent3d box = read_box(rd);
        bool comp = rd.flag();
        o.push_back(0.0);
        o.push_back(flg(v->IsBoxInParamDomain(box, comp)));
        break;
    }
    case OP_PBOX_IN_INSPACE:
    {
        SmPseudoBox pb = read_pbox(rd);
        bool comp = rd.flag();
        o.push_back(0.0);
        o.push_back(flg(v->IsPseudoBoxInInSpaceDomain(pb, comp)));
        break;
    }
    case OP_EVAL_BBOX:
    {
        SmExtent3d box = read_box(rd);
        bool has_ppb = rd.flag();
        SmPseudoBox ppb;
        if (has_ppb)
            ppb = read_pbox(rd);
        bool w_ob = rd.flag(), w_opb = rd.flag(), w_ib = rd.flag(), w_ipb = rd.flag();
        bool comp = rd.flag();
        SmExtent3d ob, ib;
        SmPseudoBox opb, ipb;
        SmStatus s = v->EvaluateBoundingBox(box, has_ppb ? &ppb : NULL, w_ob ? &ob : NULL,
                                            w_opb ? &opb : NULL, w_ib ? &ib : NULL,
                                            w_ipb ? &ipb : NULL, comp);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            if (w_ob)
                put_box(o, ob);
            if (w_opb)
                put_pbox(o, opb);
            if (w_ib)
                put_box(o, ib);
            if (w_ipb)
                put_pbox(o, ipb);
        }
        break;
    }
    case OP_MAP_PBOX:
    {
        SmPseudoBox pb = read_pbox(rd);
        bool w_box = rd.flag(), w_pb = rd.flag();
        bool comp = rd.flag(), trim = rd.flag();
        SmExtent3d ob;
        SmPseudoBox opb;
        SmStatus s = v->MapPseudoBox(pb, w_box ? &ob : NULL, w_pb ? &opb : NULL, comp, trim);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            if (w_box)
                put_box(o, ob);
            if (w_pb)
                put_pbox(o, opb);
        }
        break;
    }
    case OP_EVAL_POINT:
    {
        SmVector3d p = rd.vec();
        bool comp = rd.flag();
        SmPoint3d q;
        SmStatus s = v->EvaluatePoint(p, q, comp);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, q);
        break;
    }
    case OP_EVAL_1ST:
    {
        SmVector3d p = rd.vec();
        bool l0 = rd.flag(), l1 = rd.flag(), l2 = rd.flag();
        bool nz = rd.flag(), comp = rd.flag();
        SmPoint3d q;
        SmVector3d du, dv, dw;
        SmStatus s = v->Evaluate1stDerivatives(p, l0, l1, l2, q, du, dv, dw, nz, comp);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            put_vec(o, q);
            put_vec(o, du);
            put_vec(o, dv);
            put_vec(o, dw);
        }
        break;
    }
    case OP_INV_EVAL:
    case OP_INV_MAP:
    {
        SmVector3d p = rd.vec();
        bool has_guess = rd.flag();
        SmPoint3d g;
        if (has_guess)
            g = rd.vec();
        bool has_dom = rd.flag();
        SmExtent3d dom;
        SmPseudoBox pdom;
        if (has_dom)
        {
            if (op == OP_INV_EVAL)
                dom = read_box(rd);
            else
                pdom = read_pbox(rd);
        }
        bool comp = rd.flag();
        SmBoolean ok = FALSE;
        SmTArray<SmPoint3d> pts;
        SmTArray<double> gaps;
        SmStatus s = op == OP_INV_EVAL
                         ? v->InvEvaluatePoint(p, has_guess ? &g : NULL, ok, pts, gaps,
                                               has_dom ? &dom : NULL, comp)
                         : v->InvMapPoint(p, has_guess ? &g : NULL, ok, pts, gaps,
                                          has_dom ? &pdom : NULL, comp);
        put_inv(o, s, ok, pts, gaps);
        break;
    }
    case OP_GLOBAL:
    {
        SmVector3d p = rd.vec();
        bool has_dom = rd.flag();
        SmExtent3d dom;
        if (has_dom)
            dom = read_box(rd);
        bool comp = rd.flag();
        SmBoolean found = FALSE;
        SmSolutionArray sols;
        SmStatus s = v->GlobalPointSolve(p, found, sols, has_dom ? &dom : NULL, comp);
        put_solutions(o, s, found, sols);
        break;
    }
    case OP_HAS_DISC:
    {
        bool want = rd.flag();
        bool geometric = rd.flag();
        SmDiscontinuities3d d;
        d.SetUnaligned();
        SmBoolean b = v->HasDiscontinuities(want ? &d : NULL, geometric);
        o.push_back(0.0);
        o.push_back(flg(b));
        put_cont_dump(o, d);
        break;
    }
    case OP_BBOX_SIMPLE:
    {
        SmExtent3d box = read_box(rd);
        bool has_ppb = rd.flag();
        SmPseudoBox ppb;
        if (has_ppb)
            ppb = read_pbox(rd);
        bool w_box = rd.flag(), w_pb = rd.flag();
        SmExtent3d ob;
        SmPseudoBox opb;
        SmStatus s = v->EvaluateBoundingBoxSimple(box, has_ppb ? &ppb : NULL, w_box ? &ob : NULL,
                                                  w_pb ? &opb : NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            if (w_box)
                put_box(o, ob);
            if (w_pb)
                put_pbox(o, opb);
        }
        break;
    }
    case OP_PBOX_SIMPLE:
    {
        SmPseudoBox pb = read_pbox(rd);
        bool w_box = rd.flag(), w_pb = rd.flag();
        SmExtent3d ob;
        SmPseudoBox opb;
        SmStatus s = v->EvaluatePseudoBoxSimple(pb, w_box ? &ob : NULL, w_pb ? &opb : NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            if (w_box)
                put_box(o, ob);
            if (w_pb)
                put_pbox(o, opb);
        }
        break;
    }
    case OP_INV_EVAL_SIMPLE:
    {
        SmVector3d p = rd.vec();
        bool has_guess = rd.flag();
        SmPoint3d g;
        if (has_guess)
            g = rd.vec();
        bool has_dom = rd.flag();
        SmExtent3d dom;
        if (has_dom)
            dom = read_box(rd);
        SmBoolean ok = FALSE;
        SmTArray<SmPoint3d> pts;
        SmTArray<double> gaps;
        SmStatus s = v->InvEvaluatePointSimple(p, has_guess ? &g : NULL, ok, pts, gaps,
                                               has_dom ? &dom : NULL);
        put_inv(o, s, ok, pts, gaps);
        break;
    }
    case OP_GLOBAL_SIMPLE:
    {
        SmVector3d p = rd.vec();
        bool has_dom = rd.flag();
        SmExtent3d dom;
        if (has_dom)
            dom = read_box(rd);
        SmBoolean found = FALSE;
        SmSolutionArray sols;
        SmStatus s = v->GlobalPointSolveSimple(p, found, sols, has_dom ? &dom : NULL);
        put_solutions(o, s, found, sols);
        break;
    }
    case OP_LOCAL_SIMPLE:
    {
        SmVector3d p = rd.vec();
        SmVector3d g = rd.vec();
        bool has_dom = rd.flag();
        SmExtent3d dom;
        if (has_dom)
            dom = read_box(rd);
        SmBoolean found = FALSE;
        SmSolution sol;
        SmStatus s = v->LocalPointSolveSimple(p, g, found, sol, has_dom ? &dom : NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(found));
            put_solution(o, sol);
        }
        break;
    }
    }
}

// `type`: SmVolume::ReadFromDB(type, ASCII stream of data) into a NULL volume.
void read_db_op(Rd& rd, const char* data, int64_t data_len, std::vector<double>& o)
{
    int64_t type = rd.idx();
    std::istringstream iss(std::string(data, (size_t)data_len));
    SmDatabaseIOStream db(SM_ASCII, &iss);
    SmContext ctx;
    SmVolume* p = NULL;
    SmStatus s = SmVolume::ReadFromDB((SM_TYPE)type, db, ctx, p, 30);
    o.push_back(st(s));
    o.push_back(p != NULL ? 1.0 : 0.0);
    if (p != NULL)
    {
        put_state(o, p);
        delete p;
    }
}
} // namespace

SC_EXPORT int32_t sc_surface_volume_p2_eval(int32_t op, const double* in, int64_t n_in,
                                            const char* data, int64_t data_len, double* out,
                                            int64_t cap, int64_t* out_len)
{
    if (in == nullptr || n_in < 0 || out_len == nullptr || data_len < 0 ||
        (data == nullptr && data_len > 0))
        return SC_ERR_ARG;
    SC_TRY
    std::vector<double> o;
    Rd rd{in, 0};
    if (op == OP_READ_DB)
        read_db_op(rd, data == nullptr ? "" : data, data_len, o);
    else
    {
        SmContext ctx;
        SmVolume* v = read_volume(rd, ctx);
        if (v == NULL)
            return SC_ERR_ARG;
        volume_op(op, rd, v, o);
        delete v;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
