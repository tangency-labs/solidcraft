// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.bend_volume`: the SmBendVolume members owned by the unit
// (source/SMLib/src/SmBendVolume.cpp, source/SMLib/inc/SmBendVolume.h), called on a
// bend volume built by the default or the component constructor, optionally compounded with
// an SmTransform next map.
//
// One entry point, sc_surface_bend_volume_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/surface_bend_volume.rs, which validates every
// input. `out` receives the op status (0 = SM_SUCCESS, else the SmStatus value) and the op's
// outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmContext.h>
#include <SmDatabaseIO.h>
#include <SmExtent3d.h>
#include <SmLine.h>
#include <SmPseudoBox.h>
#include <SmSolutionArray.h>
#include <SmTArray.h>
#include <SmTransform.h>
#include <SmBendVolume.h>
#include <SmVector3d.h>
#include <SmVolume.h>

#include <sstream>
#include <string>
#include <vector>

#include "sc_sm.h"

using sc::Rd;
using sc::put_vec;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_bend_volume.rs).
enum : int32_t
{
    OP_INFO = 0,
    OP_EVAL = 1,
    OP_EVAL_SIMPLE = 2,
    OP_BBOX = 3,
    OP_TRIM = 4,
    OP_INV_GUESS = 5,
    OP_GLOBAL = 6,
    OP_LOCAL = 7,
    OP_PREDS = 8,
    OP_CURVE_IN = 9,
    OP_COPY = 10,
    OP_EQUAL = 11,
    OP_READ_DB = 12,
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

void put_str(std::vector<double>& o, const TCHAR* s)
{
    for (; *s; ++s)
        o.push_back((double)*s);
    o.push_back(-1.0);
}

// `kind [center axis mid neutral tol] has_next [O X Y Z]`: kind 0 = SmBendVolume(&ctx),
// 1 = the component constructor; the next map SmTransform(ctx, O, X, Y, &Z) is appended with
// AddNextMap(t, 2).
SmBendVolume* read_bend(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    SmBendVolume* v = NULL;
    if (kind == 1)
    {
        SmPoint3d c = rd.vec();
        SmVector3d axis = rd.vec();
        SmVector3d mid = rd.vec();
        double neutral = rd.num();
        double tol = rd.num();
        v = new (ctx) SmBendVolume(ctx, c, axis, mid, neutral, tol);
    }
    else
        v = new (ctx) SmBendVolume(&ctx);
    if (rd.flag())
    {
        SmVector3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        SmVector3d z = rd.vec();
        v->AddNextMap(new (ctx) SmTransform(&ctx, o, x, y, &z), 2);
    }
    return v;
}

// `type neutral tol has_orient [O(16)] has_inv [I(16)] has_next [next type]`.
void put_state(std::vector<double>& o, const SmBendVolume* v)
{
    o.push_back((double)v->GetType());
    o.push_back(v->GetBendNeutralDist());
    o.push_back(v->GetSingularityTol());
    o.push_back(v->GetOrientMap() ? 1.0 : 0.0);
    if (v->GetOrientMap())
        put_mat(o, *v->GetOrientMap());
    o.push_back(v->GetInvOrientMap() ? 1.0 : 0.0);
    if (v->GetInvOrientMap())
        put_mat(o, *v->GetInvOrientMap());
    o.push_back(v->GetNextMap() ? 1.0 : 0.0);
    if (v->GetNextMap())
        o.push_back((double)v->GetNextMap()->GetType());
}

int64_t vsize(int64_t n) { return (n + 1) * (n + 1) * (n + 1); }

void put_array(std::vector<double>& o, const SmVector3d* a, int64_t cnt)
{
    for (int64_t i = 0; i < cnt; ++i)
        put_vec(o, a[i]);
}

// `type nvars value p0 p1 p2 nobjects is_this`.
void put_solution(std::vector<double>& o, const SmSolution& s, const SmVolume* v)
{
    o.push_back((double)s.m_eSolutionType);
    o.push_back((double)s.m_lNumVariables);
    o.push_back(s.m_vStart.m_dSolutionValue);
    o.push_back(s.m_vStart.m_adParameters[0]);
    o.push_back(s.m_vStart.m_adParameters[1]);
    o.push_back(s.m_vStart.m_adParameters[2]);
    o.push_back((double)s.m_lNumObjects);
    o.push_back(s.m_apObjects[0] == (const SmObject*)v ? 1.0 : 0.0);
}

SmExtent3d read_box(Rd& rd)
{
    SmVector3d a = rd.vec();
    SmVector3d b = rd.vec();
    return SmExtent3d(a.x, a.y, a.z, b.x, b.y, b.z);
}

void put_box(std::vector<double>& o, const SmExtent3d& b)
{
    o.push_back(b.GetUMin());
    o.push_back(b.GetVMin());
    o.push_back(b.GetWMin());
    o.push_back(b.GetUMax());
    o.push_back(b.GetVMax());
    o.push_back(b.GetWMax());
}

void put_pseudo(std::vector<double>& o, SmPseudoBox& pb)
{
    for (ULONG i = 0; i < 3; ++i)
        put_vec(o, pb.GetBasis(i));
    for (ULONG i = 0; i < 3; ++i)
    {
        o.push_back(pb.GetInterval(i).GetMin());
        o.push_back(pb.GetInterval(i).GetMax());
    }
}

void bend_op(int32_t op, Rd& rd, const SmContext& ctx, SmBendVolume* v, std::vector<double>& o)
{
    switch (op)
    {
    case OP_INFO:
    {
        o.push_back(0.0);
        put_state(o, v);
        o.push_back((double)v->GetType());
        put_str(o, v->GetTypeString());
        put_str(o, v->GetClassString());
        int64_t nt = rd.idx();
        for (int64_t i = 0; i < nt; ++i)
            o.push_back(flg(v->IsKindOf((SM_TYPE)rd.idx())));
        put_vec(o, v->GetBendOrigin());
        put_vec(o, v->GetBendAxis());
        put_vec(o, v->GetBendMidDir());
        put_vec(o, v->GetBendBinormal());
        put_box(o, v->GetNaturalParamDomain());
        o.push_back(flg(v->IsBoundedSimple()));
        o.push_back(v->GetLegalMinU());
        break;
    }
    case OP_EVAL:
    case OP_EVAL_SIMPLE:
    {
        SmPoint3d p = rd.vec();
        int64_t n = rd.idx();
        bool comp = op == OP_EVAL ? rd.flag() : false;
        SmVector3d a[6 * 6 * 6];
        SmStatus s = op == OP_EVAL
                         ? v->Evaluate(p, (ULONG)n, TRUE, TRUE, TRUE, a, TRUE, TRUE, comp ? TRUE : FALSE)
                         : v->EvaluateSimple(p, (ULONG)n, TRUE, TRUE, TRUE, a, TRUE, TRUE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_array(o, a, vsize(n));
        break;
    }
    case OP_BBOX:
    {
        SmExtent3d box = read_box(rd);
        bool want_box = rd.flag();
        bool want_pseudo = rd.flag();
        SmExtent3d proj;
        SmPseudoBox pseudo;
        SmStatus s = v->EvaluateBoundingBoxSimple(box, NULL, want_box ? &proj : NULL,
                                                  want_pseudo ? &pseudo : NULL);
        o.push_back(st(s));
        if (want_box)
            put_box(o, proj);
        if (want_pseudo)
            put_pseudo(o, pseudo);
        break;
    }
    case OP_TRIM:
    {
        SmExtent3d box = read_box(rd);
        SmExtent3d trim;
        SmStatus s = v->TrimParamBoundingBoxSimple(box, trim);
        o.push_back(st(s));
        put_box(o, trim);
        break;
    }
    case OP_INV_GUESS:
    {
        SmPoint3d p = rd.vec();
        SmTArray<SmPoint3d> g;
        SmStatus s = v->InvEvaluateGuessPointSimple(p, g);
        o.push_back(st(s));
        o.push_back((double)g.GetSize());
        for (ULONG i = 0; i < g.GetSize(); ++i)
            put_vec(o, g[i]);
        break;
    }
    case OP_GLOBAL:
    case OP_LOCAL:
    {
        SmPoint3d p = rd.vec();
        SmPoint3d guess;
        if (op == OP_LOCAL)
            guess = rd.vec();
        bool has_dom = rd.flag();
        SmExtent3d dom;
        if (has_dom)
            dom = read_box(rd);
        SmBoolean found = FALSE;
        if (op == OP_GLOBAL)
        {
            SmSolutionArray sols;
            SmStatus s = v->GlobalPointSolveSimple(p, found, sols, has_dom ? &dom : NULL);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
            {
                o.push_back(flg(found));
                o.push_back((double)sols.GetSize());
                for (ULONG i = 0; i < sols.GetSize(); ++i)
                    put_solution(o, sols[i], v);
            }
        }
        else
        {
            SmSolution sol;
            SmStatus s = v->LocalPointSolveSimple(p, guess, found, sol, has_dom ? &dom : NULL);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
            {
                o.push_back(flg(found));
                put_solution(o, sol, v);
            }
        }
        break;
    }
    case OP_PREDS:
    {
        SmPoint3d p = rd.vec();
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        o.push_back(0.0);
        o.push_back(flg(v->IsPointInParamDomainSimple(p)));
        o.push_back(flg(v->IsLineInParamDomainSimple(a, b)));
        o.push_back(flg(v->IsBoundedSimple()));
        break;
    }
    case OP_CURVE_IN:
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine line(a, b, 3, &ctx);
        o.push_back(0.0);
        o.push_back(flg(v->IsCurveInParamDomainSimple(line)));
        break;
    }
    case OP_COPY:
    {
        bool simple = rd.flag();
        SmVolume* c = NULL;
        SmStatus s = v->Copy(ctx, c, simple ? TRUE : FALSE);
        o.push_back(st(s));
        if (c != NULL)
        {
            put_state(o, (SmBendVolume*)c);
            delete c;
        }
        break;
    }
    case OP_EQUAL:
    {
        SmBendVolume* other = read_bend(rd, ctx);
        bool self_cmp = rd.flag();
        o.push_back(0.0);
        o.push_back(flg(self_cmp ? (*v == *v) : (*v == *other)));
        delete other;
        break;
    }
    default: break;
    }
}

// `n_bytes bytes...`: SmBendVolume::ReadFromDB of the ASCII stream.
void read_db_op(Rd& rd, std::vector<double>& o)
{
    int64_t n = rd.idx();
    std::string data;
    for (int64_t i = 0; i < n; ++i)
        data.push_back((char)rd.idx());
    std::istringstream iss(data);
    SmDatabaseIOStream db(SM_ASCII, &iss);
    SmVolume* p = NULL;
    SmContext ctx;
    SmStatus s = SmBendVolume::ReadFromDB(SmBendVolume_TYPE, db, ctx, p, 30);
    o.push_back(st(s));
    if (p != NULL)
    {
        put_state(o, (SmBendVolume*)p);
        delete p;
    }
}
} // namespace

SC_EXPORT int32_t sc_surface_bend_volume_eval(int32_t op, const double* in, int64_t n_in,
                                                double* out, int64_t cap, int64_t* out_len)
{
    if (in == nullptr || n_in < 0 || out_len == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    std::vector<double> o;
    Rd rd{in, 0};
    if (op == OP_READ_DB)
        read_db_op(rd, o);
    else
    {
        SmContext ctx;
        SmBendVolume* v = read_bend(rd, ctx);
        if (v == NULL)
            return SC_ERR_ARG;
        bend_op(op, rd, ctx, v, o);
        delete v;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
