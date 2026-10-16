// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.curve_bounded_surface`: SmCurveBoundedSurface
// (source/SMLib/src/SmCurveBoundedSurface.cpp, source/SMLib/inc/SmCurveBoundedSurface.h),
// called through `SmSurface*` so the virtual dispatch (the class's overrides or the
// SmSurface defaults) is upstream's.
//
// One entry point, sc_surface_curve_bounded_surface_eval(op, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/surface_curve_bounded_surface.rs,
// which validates every input. The input starts with `ctor`: 0 (static op) or 1 (a curve
// bounded surface `B`: basis `oob form n m p q r s cpts(x y z w) u v`, `implicit`, `n3d`
// curves, `nuv` curves; a curve is `dim n p m cpts(x y z w) knots`, an
// SmBSplineCurve(gw_CURVE*, dim, FALSE)). `out` receives the op status (0 = SM_SUCCESS, else
// the SmStatus value) and the op's outputs.
//
// The boundary arrays are private reference members; they are read through the object
// layout (the Itanium ABI stores a reference member as a pointer): the slots between
// m_pBasisSurface and m_bImplicitOuter, whose offsets are taken from member pointers. The
// layout is checked before use (SC_ERR_ARG if it differs).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveBoundedSurface.h>
#include <SmDatabaseIO.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <cstddef>
#include <sstream>
#include <string>
#include <vector>

#include "sc_sm.h"

using sc::flg;
using sc::put_dom;
using sc::put_vec;
using sc::st;

namespace
{
enum : int32_t
{
    OP_STATE = 0,
    OP_EVAL = 1,
    OP_EVAL_PT = 2,
    OP_QUERIES = 3,
    OP_DEFAULTS = 4,
    OP_EQ_SELF = 5,
    OP_EQ_OTHER = 6,
    OP_NORMAL = 7,
    OP_SIMPLE = 8,
    OP_SINGULAR = 9,
    OP_GEOMETRIC = 10,
    OP_NORMAL_SECTION = 11,
    OP_STEP = 12,
    OP_STEP_PT = 13,
    OP_SET_CONTEXT = 14,
    // static ops
    OP_READ_DB = 40,
};

// Access to private members through explicit instantiation.
template <typename Tag, typename Tag::type M>
struct Rob
{
    friend typename Tag::type get(Tag) { return M; }
};

struct CbsBasis
{
    typedef SmSurface* SmCurveBoundedSurface::*type;
    friend type get(CbsBasis);
};
template struct Rob<CbsBasis, &SmCurveBoundedSurface::m_pBasisSurface>;

struct CbsOuter
{
    typedef SmBoolean SmCurveBoundedSurface::*type;
    friend type get(CbsOuter);
};
template struct Rob<CbsOuter, &SmCurveBoundedSurface::m_bImplicitOuter>;

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
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
};

void put_str(std::vector<double>& o, const TCHAR* s)
{
    std::vector<double> c;
    for (const TCHAR* p = s; p && *p; ++p)
        c.push_back((double)(unsigned)*p);
    o.push_back((double)c.size());
    o.insert(o.end(), c.begin(), c.end());
}

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

SmBSplineSurface* read_bspline(Rd& rd, const SmContext& ctx)
{
    bool oob = rd.flag();
    SmBSplineSurfaceForm form = (SmBSplineSurfaceForm)rd.idx();
    SmBSplineSurface* s = new (ctx) SmBSplineSurface(read_nurb(rd), FALSE, NULL);
    s->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
    s->SetBSplineSurfaceForm(form);
    return s;
}

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

// `B`: basis, implicit, n3d curves, nuv curves.
SmCurveBoundedSurface* read_cbs(Rd& rd, const SmContext& ctx)
{
    SmBSplineSurface* basis = read_bspline(rd, ctx);
    bool implicit = rd.flag();
    SmTArray<SmCurve*> c3d, cuv;
    int64_t n3d = rd.idx();
    for (int64_t i = 0; i < n3d; i++)
        c3d.Add(read_curve(rd, ctx));
    int64_t nuv = rd.idx();
    for (int64_t i = 0; i < nuv; i++)
        cuv.Add(read_curve(rd, ctx));
    return new (ctx) SmCurveBoundedSurface(*basis, &c3d, &cuv, implicit ? TRUE : FALSE);
}

// The boundary arrays (0: 3D, 1: UV); NULL when the layout is not the expected one.
const SmTArray<SmCurve*>* boundaries(const SmCurveBoundedSurface* s, int which)
{
    const char* pb = (const char*)&(s->*get(CbsBasis()));
    const char* po = (const char*)&(s->*get(CbsOuter()));
    if (po - pb != (ptrdiff_t)(3 * sizeof(void*)))
        return NULL;
    const SmTArray<SmCurve*>* const* slot =
        (const SmTArray<SmCurve*>* const*)(pb + (1 + which) * sizeof(void*));
    return *slot;
}

// `has_nurb oob borrowed knot_type form [n m p q r s cpts u v]` (preceded by 1), or 0.
void put_bss(std::vector<double>& o, const SmSurface* srf)
{
    const SmBSplineSurface* s = SM_CAST_PTR(SmBSplineSurface, srf);
    o.push_back(flg(s != NULL));
    if (!s)
        return;
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

// `type dim a b` then 5 x `status x y z` (points at a + (b - a) * i / 4), then for a
// B-spline `has_nurb [n p m cpts knots]`.
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    o.push_back((double)c->GetType());
    o.push_back((double)c->GetDim());
    SmExtent1d ivl = c->GetNaturalInterval();
    double a = ivl.GetMin();
    double b = ivl.GetMax();
    o.push_back(a);
    o.push_back(b);
    for (int i = 0; i <= 4; i++)
    {
        double t = a + (b - a) * ((double)i / 4.0);
        SmPoint3d p(0.0, 0.0, 0.0);
        SmStatus stt = c->EvaluatePoint(t, p);
        o.push_back(st(stt));
        put_vec(o, stt == SM_SUCCESS ? p : SmPoint3d(0.0, 0.0, 0.0));
    }
    const SmBSplineCurve* bc = SM_CAST_PTR(SmBSplineCurve, c);
    if (!bc)
        return;
    const gw_CURVE* cur = bc->GetGwNurbPointer();
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

// `type implicit basis(put_bss) n3d curves nuv curves`; false on a layout mismatch.
bool put_state(std::vector<double>& o, const SmSurface* srf)
{
    const SmCurveBoundedSurface* s = SM_CAST_PTR(SmCurveBoundedSurface, srf);
    o.push_back((double)srf->GetType());
    if (!s)
        return true;
    const SmTArray<SmCurve*>* c3d = boundaries(s, 0);
    const SmTArray<SmCurve*>* cuv = boundaries(s, 1);
    if (!c3d || !cuv)
        return false;
    o.push_back(flg(s->*get(CbsOuter()) == TRUE));
    put_bss(o, s->*get(CbsBasis()));
    o.push_back((double)c3d->GetSize());
    for (ULONG i = 0; i < c3d->GetSize(); i++)
        put_curve(o, (*c3d)[i]);
    o.push_back((double)cuv->GetSize());
    for (ULONG i = 0; i < cuv->GetSize(); i++)
        put_curve(o, (*cuv)[i]);
    return true;
}

// `(hu+1)(hv+1)` vectors in the upstream 1d order, only `i + j <= hu` when `upper`.
void put_matrix(std::vector<double>& o, const std::vector<SmVector3d>& d, ULONG hu, ULONG hv,
                bool upper)
{
    for (ULONG i = 0; i <= hu; i++)
        for (ULONG j = 0; j <= hv; j++)
            if (!upper || i + j <= hu)
                put_vec(o, d[i * (hv + 1) + j]);
}

bool surface_op(int32_t op, Rd& rd, const SmContext& ctx, SmSurface* s, std::vector<double>& o)
{
    switch (op)
    {
    case OP_STATE:
    {
        o.push_back(0.0);
        return put_state(o, s);
    }
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
            put_matrix(o, d, hu, hv, upper);
        return true;
    }
    case OP_EVAL_PT:
    {
        SmPoint2d uv = rd.pt2();
        SmPoint3d p;
        SmStatus stt = s->EvaluatePoint(uv, p);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_vec(o, p);
        return true;
    }
    case OP_QUERIES:
    {
        int64_t k = rd.idx();
        o.push_back(0.0);
        put_dom(o, s->GetNaturalUVDomain());
        put_dom(o, s->GetSTEPUVDomain());
        o.push_back((double)s->GetDegree(SM_SP_U));
        o.push_back((double)s->GetDegree(SM_SP_V));
        o.push_back((double)s->GetNumberControlPoints(SM_SP_U));
        o.push_back((double)s->GetNumberControlPoints(SM_SP_V));
        o.push_back(flg(s->IsBilinear()));
        o.push_back((double)s->GetType());
        for (int64_t i = 0; i < k; i++)
            o.push_back(flg(s->IsKindOf((SM_TYPE)rd.idx())));
        put_str(o, s->GetTypeString());
        put_str(o, s->GetClassString());
        return true;
    }
    case OP_DEFAULTS:
    {
        // the SmSurface defaults the class does not override
        SmVector3d v = rd.vec();
        o.push_back(0.0);
        SmTArray<double> knots;
        SmTArray<ULONG> mults;
        o.push_back(st(s->GetKnots(SM_SP_U, knots, &mults, NULL)));
        o.push_back(flg(s->IsParallelToVector(v)));
        put_dom(o, s->GetMaxAnalyticDomain());
        SmSurface* c = NULL;
        o.push_back(st(s->Copy(ctx, c)));
        delete c;
        c = NULL;
        o.push_back(st(s->CopyAndAddAnalytics(ctx, c)));
        delete c;
        SmSurface* l = NULL;
        SmSurface* r = NULL;
        o.push_back(st(s->SplitAt(ctx, 0.5, SM_SP_U, l, r)));
        delete l;
        delete r;
        o.push_back(st(s->Reverse(SM_SP_U)));
        o.push_back(st(s->SwapUV()));
        return put_state(o, s);
    }
    case OP_EQ_SELF:
    {
        o.push_back(0.0);
        o.push_back(flg(*s == *s));
        return true;
    }
    case OP_EQ_OTHER:
    {
        SmCurveBoundedSurface* other = read_cbs(rd, ctx);
        o.push_back(0.0);
        o.push_back(flg(*s == *other));
        delete other;
        return true;
    }
    case OP_NORMAL:
    {
        SmPoint2d uv = rd.pt2();
        bool ul = rd.flag();
        bool vl = rd.flag();
        SmVector3d nrm(0.0, 0.0, 0.0);
        SmStatus stt = s->EvaluateNormal(uv, ul ? TRUE : FALSE, vl ? TRUE : FALSE, nrm);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_vec(o, nrm);
        return true;
    }
    case OP_SIMPLE:
    case OP_STEP:
    {
        SmPoint2d uv = rd.pt2();
        ULONG hu = (ULONG)rd.idx();
        ULONG hv = (ULONG)rd.idx();
        bool ul = rd.flag();
        bool vl = rd.flag();
        bool upper = rd.flag();
        std::vector<SmVector3d> d((hu + 1) * (hv + 1), SmVector3d(0.0, 0.0, 0.0));
        SmStatus stt =
            op == OP_SIMPLE
                ? s->EvaluateSimple(uv, hu, hv, ul ? TRUE : FALSE, vl ? TRUE : FALSE,
                                    upper ? TRUE : FALSE, d.data())
                : s->EvaluateSTEP(uv, hu, hv, ul ? TRUE : FALSE, vl ? TRUE : FALSE,
                                  upper ? TRUE : FALSE, d.data());
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_matrix(o, d, hu, hv, upper);
        return true;
    }
    case OP_SINGULAR:
    {
        SmPoint2d uv = rd.pt2();
        double tol = rd.num();
        SmVector3d nrm(0.0, 0.0, 0.0);
        SmStatus stt = s->EvaluateNormalAtSingularity(uv, nrm, tol);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_vec(o, nrm);
        return true;
    }
    case OP_GEOMETRIC:
    {
        SmPoint2d uv = rd.pt2();
        bool ul = rd.flag();
        bool vl = rd.flag();
        double gauss = 0.0, mean = 0.0, k1 = 0.0, k2 = 0.0;
        SmVector3d efg, lmn, d1, d2;
        SmStatus stt = s->EvaluateGeometric(uv, ul ? TRUE : FALSE, vl ? TRUE : FALSE, gauss,
                                            mean, k1, k2, efg, lmn, d1, d2);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(gauss);
            o.push_back(mean);
            o.push_back(k1);
            o.push_back(k2);
            put_vec(o, efg);
            put_vec(o, lmn);
            put_vec(o, d1);
            put_vec(o, d2);
        }
        return true;
    }
    case OP_NORMAL_SECTION:
    {
        SmPoint2d uv = rd.pt2();
        bool ul = rd.flag();
        bool vl = rd.flag();
        SmVector3d dir = rd.vec();
        SmVector3d tan, nrm;
        double curv = 0.0;
        SmStatus stt = s->EvaluateNormalSection(uv, ul ? TRUE : FALSE, vl ? TRUE : FALSE, dir,
                                                tan, curv, nrm);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            put_vec(o, tan);
            o.push_back(curv);
            put_vec(o, nrm);
        }
        return true;
    }
    case OP_STEP_PT:
    {
        SmPoint2d uv = rd.pt2();
        SmPoint3d p;
        SmStatus stt = s->EvaluateSTEPPoint(uv, p);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_vec(o, p);
        return true;
    }
    case OP_SET_CONTEXT:
    {
        SmCurveBoundedSurface* cbs = SM_CAST_PTR(SmCurveBoundedSurface, s);
        if (cbs)
            cbs->SetContext(&ctx);
        o.push_back(0.0);
        return put_state(o, s);
    }
    default: o.push_back((double)SC_ERR_ARG); return true;
    }
}

bool static_op(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_READ_DB:
    {
        SM_TYPE type = (SM_TYPE)rd.idx();
        ULONG version = (ULONG)rd.idx();
        int64_t nb = rd.idx();
        std::string bytes;
        for (int64_t i = 0; i < nb; i++)
            bytes.push_back((char)(unsigned char)rd.idx());
        std::istringstream iss(bytes);
        SmDatabaseIOStream db(SM_ASCII, &iss);
        SmSurface* s = NULL;
        SmStatus stt = SmSurface::ReadFromDB(type, db, ctx, s, version);
        o.push_back(st(stt));
        bool ok = true;
        if (stt == SM_SUCCESS && s)
            ok = put_state(o, s);
        delete s;
        return ok;
    }
    default: o.push_back((double)SC_ERR_ARG); return true;
    }
}
} // namespace

SC_EXPORT int32_t sc_surface_curve_bounded_surface_eval(int32_t op, const double* in, int64_t n_in,
                                                       double* out, int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    int32_t ctor = (int32_t)rd.idx();
    bool ok = true;
    if (ctor == 0)
    {
        ok = static_op(op, rd, ctx, o);
    }
    else if (ctor == 1)
    {
        SmCurveBoundedSurface* s = read_cbs(rd, ctx);
        ok = surface_op(op, rd, ctx, s, o);
        delete s;
    }
    else
    {
        return SC_ERR_ARG;
    }
    if (!ok)
        return SC_ERR_ARG;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
