// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.curve_cache`: SmCurveCache (source/SMLib/src/SmCurveCache.cpp,
// source/SMLib/inc/SmCurveCache.h) built over an SmBSplineCurve or SmHermiteCurve, tessellated,
// and its spans, continuities and decomposition tree read back.
//
// One entry point, sc_curve_curve_cache_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/curve_curve_cache.rs, which validates every
// input. Every op starts with a curve block (0: SmBSplineCurve `dim n p m cpts(x y z w) knots`;
// 1: SmHermiteCurve `dim P1 D1 P2 D2`), then the cache interval `a b` and the constructor
// arguments `full chord angle offaxis minseg aux max3d ratio`. `out` receives the op status
// (0 = SM_SUCCESS, else the SmStatus value) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveCache.h>
#include <SmCurveTypes.h>
#include <SmExtent1d.h>
#include <SmExtent3d.h>
#include <SmHermiteCurve.h>
#include <SmNurbsCrv.h>
#include <SmPseudoBox.h>
#include <SmTree.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <map>
#include <vector>

#include "sc_sm.h"

using sc::Rd;
using sc::put_vec;

namespace
{
enum
{
    OP_TESSELLATE = 0,
    OP_ANALYTIC = 1,
    OP_TYPE = 2,
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }

SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    ULONG dim = (ULONG)rd.idx();
    if (kind == 0)
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
        return new (ctx) SmBSplineCurve(cur, dim, FALSE);
    }
    SmVector3d p1 = rd.vec();
    SmVector3d d1 = rd.vec();
    SmVector3d p2 = rd.vec();
    SmVector3d d2 = rd.vec();
    return new (ctx) SmHermiteCurve(p1, d1, p2, d2, dim);
}

// `left right c0 c1 len chsq turn ivl_min ivl_max start[3] end[3]` of span data.
void put_aux(std::vector<double>& o, const SmCurveCache& cc, const SmBezierAux1d& a)
{
    o.push_back((double)a.m_lLeft);
    o.push_back((double)a.m_lRight);
    o.push_back((double)a.m_aeConts[0]);
    o.push_back((double)a.m_aeConts[1]);
    o.push_back(a.m_dMaxLength);
    o.push_back(a.m_dMaxChordHeightSquared);
    o.push_back(a.m_dMaxTurningAngleDeg);
    SmPoint3d s, e;
    SmExtent1d ivl;
    a.GetEnds(cc, s, e, ivl);
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    put_vec(o, s);
    put_vec(o, e);
}

void put_pbox(std::vector<double>& o, SmPseudoBox& b)
{
    for (ULONG k = 0; k < 3; ++k)
        put_vec(o, b.GetBasis(k));
    for (ULONG k = 0; k < 3; ++k)
    {
        o.push_back(b.GetInterval(k).GetMin());
        o.push_back(b.GetInterval(k).GetMax());
    }
}

void tessellate(SmCurveCache& cc, bool aux, std::vector<double>& o)
{
    SmStatus s = cc.Tessellate();
    o.push_back(st(s));
    if (s != SM_SUCCESS)
        return;

    // spans
    ULONG n = cc.GetSpanCount();
    o.push_back((double)n);
    std::map<const void*, int64_t> span_of, parent_of;
    for (ULONG i = 0; i < n; ++i)
    {
        SmBezierSpan* sp = cc.GetAt(i);
        span_of[(const void*)sp] = (int64_t)i;
        put_aux(o, cc, *sp);
        put_pbox(o, sp->GetPseudoBox());
    }
    if (aux)
        for (ULONG i = 0; i < n; ++i)
            parent_of[(const void*)cc.GetParentAt(i)] = (int64_t)i;

    // continuities
    const SmTArray<SmContinuityType>& conts = cc.GetContinuitiesArray();
    o.push_back((double)conts.GetSize());
    for (ULONG i = 0; i < conts.GetSize(); ++i)
        o.push_back((double)conts[i]);

    // tree, pre-order from the top node
    SmTree* tree = cc.GetTree();
    SmTreeNode* top = tree ? tree->GetTopNode() : NULL;
    std::vector<SmTreeNode*> order;
    std::map<const SmTreeNode*, int64_t> index;
    std::vector<SmTreeNode*> stack;
    if (top)
        stack.push_back(top);
    while (!stack.empty())
    {
        SmTreeNode* nd = stack.back();
        stack.pop_back();
        if (index.count(nd))
            continue;
        index[nd] = (int64_t)order.size();
        order.push_back(nd);
        if (nd->m_pChild2)
            stack.push_back(nd->m_pChild2);
        if (nd->m_pChild1)
            stack.push_back(nd->m_pChild1);
    }
    auto idx_of = [&](const SmTreeNode* p) -> double {
        if (p == NULL)
            return -1.0;
        auto it = index.find(p);
        return it == index.end() ? -2.0 : (double)it->second;
    };
    o.push_back((double)order.size());
    for (SmTreeNode* nd : order)
    {
        o.push_back((double)nd->m_eAuxDataType);
        o.push_back((double)nd->m_eGeomType);
        o.push_back(idx_of(nd->m_pParent));
        o.push_back(idx_of(nd->m_pChild1));
        o.push_back(idx_of(nd->m_pChild2));
        put_vec(o, nd->m_sBBox.GetMin());
        put_vec(o, nd->m_sBBox.GetMax());
        const void* d = (const void*)nd->m_pData;
        if (d == NULL)
            o.push_back(0.0);
        else if (span_of.count(d))
        {
            o.push_back(1.0);
            o.push_back((double)span_of[d]);
        }
        else if (parent_of.count(d))
        {
            o.push_back(2.0);
            o.push_back((double)parent_of[d]);
            put_aux(o, cc, *(const SmBezierAux1d*)nd->m_pData);
        }
        else
            o.push_back(3.0);
    }
}
} // namespace

SC_EXPORT int32_t sc_curve_curve_cache_eval(int32_t op, const double* in, int64_t n_in,
                                            double* out, int64_t cap, int64_t* out_len)
{
    if (in == nullptr || n_in < 0 || out_len == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    Rd rd{in, 0};
    std::vector<double> o;
    SmCurve* c = read_curve(rd, ctx);
    double a = rd.num();
    double b = rd.num();
    SmBoolean full = rd.flag() ? TRUE : FALSE;
    double chord = rd.num();
    double angle = rd.num();
    double offaxis = rd.num();
    ULONG minseg = (ULONG)rd.idx();
    bool aux = rd.flag();
    double max3d = rd.num();
    double ratio = rd.num();
    {
        SmCurveCache cc(*c, SmExtent1d(a, b), full, chord, angle, offaxis, minseg,
                        aux ? TRUE : FALSE, max3d, ratio);
        switch (op)
        {
        case OP_TESSELLATE: tessellate(cc, aux, o); break;
        case OP_ANALYTIC:
        {
            const SmCurve* an = cc.GetAnalyticCurve();
            o.push_back(0.0);
            o.push_back(an == NULL ? 0.0 : (an == c ? 1.0 : 2.0));
            break;
        }
        case OP_TYPE:
        {
            o.push_back(0.0);
            o.push_back((double)cc.GetType());
            o.push_back(cc.IsKindOf(SmCurveCache_TYPE) ? 1.0 : 0.0);
            o.push_back(cc.IsKindOf(SmObject_TYPE) ? 1.0 : 0.0);
            o.push_back(cc.IsKindOf(SmCurve_TYPE) ? 1.0 : 0.0);
            break;
        }
        default: o.push_back((double)SC_ERR_ARG); break;
        }
    }
    delete c;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
