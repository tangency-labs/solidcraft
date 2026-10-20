// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.plane_p1`: the SmPlane members owned by the unit
// (source/SMLib/src/SmPlane.cpp, source/SMLib/inc/SmPlane.h), called on SmPlane objects (virtual
// dispatch is upstream's), plus the inherited SmBSplineSurface members the port re-dispatches for
// planes (GetNaturalUVDomain, GetMaxAnalyticDomain, IsBilinear, IsParallelToVector, GetDegree,
// GetNumberControlPoints, Reverse, SwapUV).
//
// One entry point, sc_surface_plane_p1_run(in, n_in, out, cap, out_len). `in` is a program: a
// plane block (plane A), the op count, then the ops; layouts are documented in
// crates/solidcore-ffi/src/units/surface_plane_p1.rs. Every read is bounds checked here (a bad
// program is SC_ERR_ARG). `out` receives `make_status`, then per op `op status [values]` (values
// only when the status is 0), then the state of A. A failed plane block ends the record after
// its status.
//
// Const queries that make the NURB of a plane without one (GetNaturalUVDomain and friends) are
// asked of a copy here, so that the record does not depend on when upstream makes it lazily; the
// state prints the NURB the plane has, or the one a copy makes.
#include "sc_shim.h"

#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPlane.h>
#include <SmSolutionArray.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <stdexcept>
#include <vector>

#include "sc_sm.h"

using sc::put_vec;
using sc::st;

namespace
{
// Plane blocks (keep in sync with crates/solidcore-ffi/src/units/surface_plane_p1.rs).
enum : int32_t
{
    MK_BOUNDED = 0,
    MK_NORMAL = 1,
    MK_CANONICAL = 2,
};

// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_plane_p1.rs).
enum : int32_t
{
    OP_STATE = 1,
    OP_EVALUATE = 2,
    OP_EVAL_POINT = 3,
    OP_EVAL_FAST = 4,
    OP_NORMAL = 5,
    OP_PROJECT = 6,
    OP_QUERIES = 7,
    OP_KIND_OF = 8,
    OP_ADJUST = 9,
    OP_MAKE_NURB = 10,
    OP_REPARAM = 11,
    OP_TOGGLE = 12,
    OP_REBUILD = 13,
    OP_DROP = 14,
    OP_LINE = 15,
    OP_TRIM_LINE = 16,
    OP_ISO = 17,
    OP_DROP_CURVE = 18,
    OP_COINCIDENCE = 19,
    OP_EQ = 20,
    OP_COPY = 21,
    OP_GRAZING = 22,
    OP_INHERITED = 23,
    OP_REVERSE = 24,
    OP_SWAP = 25,
    OP_EXTEND = 26,
    OP_LAST = 26,
};

// Reads the protected STEP flags through pointers to members named in a derived class.
struct PeekS : SmBSplineSurface
{
    static SmKnotType SmBSplineSurface::*kt() { return &PeekS::m_eKnotType; }
    static SmBSplineSurfaceForm SmBSplineSurface::*form() { return &PeekS::m_eBSplineSurfaceForm; }
};

struct PeekC : SmBSplineCurve
{
    static SmKnotType SmBSplineCurve::*kt() { return &PeekC::m_eKnotType; }
    static SmBSplineCurveForm SmBSplineCurve::*form() { return &PeekC::m_eBSplineCurveForm; }
};

struct Bad : std::runtime_error
{
    Bad() : std::runtime_error("bad program") {}
};

struct Rd
{
    const double* in;
    int64_t n;
    int64_t at;
    double num()
    {
        if (at >= n)
            throw Bad();
        return in[at++];
    }
    int64_t idx(int64_t lo, int64_t hi)
    {
        double d = num();
        if (!(d >= (double)lo && d <= (double)hi) || d != (double)(int64_t)d)
            throw Bad();
        return (int64_t)d;
    }
    bool flag() { return idx(0, 1) != 0; }
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
    SmPoint2d uv()
    {
        double x = num();
        double y = num();
        return SmPoint2d(x, y);
    }
    SmExtent2d dom4()
    {
        double a = num();
        double b = num();
        double c = num();
        double d = num();
        return SmExtent2d(a, b, c, d);
    }
};

double flg(SmBoolean b)
{
    return b ? 1.0 : 0.0;
}

void put_uv(std::vector<double>& o, const SmPoint2d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
}

void put_dom(std::vector<double>& o, const SmExtent2d& e)
{
    o.push_back(e.GetMin().x);
    o.push_back(e.GetMin().y);
    o.push_back(e.GetMax().x);
    o.push_back(e.GetMax().y);
}

void put_ivl(std::vector<double>& o, const SmExtent1d& e)
{
    o.push_back(e.GetMin());
    o.push_back(e.GetMax());
}

void put_chars(std::vector<double>& o, const TCHAR* s)
{
    for (; *s; s++)
        o.push_back((double)(unsigned char)*s);
}

// A plane block; NULL with *status set when the construction fails.
SmPlane* read_plane(Rd& rd, const SmContext& ctx, SmStatus* status)
{
    *status = SM_SUCCESS;
    int64_t kind = rd.idx(0, 2);
    switch (kind)
    {
    case MK_BOUNDED:
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        SmVector2d s = rd.uv();
        SmExtent2d d = rd.dom4();
        return new (ctx) SmPlane(o, x, y, s, d, &ctx);
    }
    case MK_NORMAL:
    {
        SmPoint3d o = rd.vec();
        SmVector3d nv = rd.vec();
        return new (ctx) SmPlane(o, nv, &ctx);
    }
    default:
    {
        SmPoint3d o = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        SmAxis2Placement f(o, x, y);
        SmPlane* p = NULL;
        *status = SmPlane::CreateCanonical(ctx, f, p);
        if (*status != SM_SUCCESS)
        {
            delete p;
            return NULL;
        }
        return p;
    }
    }
}

// `n m p q r s cpts(x y z w) u v` with sizes checked by the Rust side (1 <= p <= n <= 16).
SmBSplineSurface* read_bspline_surface(Rd& rd, const SmContext& ctx)
{
    gw_INDEX n = (gw_INDEX)rd.idx(1, 16);
    gw_INDEX m = (gw_INDEX)rd.idx(1, 16);
    gw_DEGREE p = (gw_DEGREE)rd.idx(1, 8);
    gw_DEGREE q = (gw_DEGREE)rd.idx(1, 8);
    gw_INDEX r = (gw_INDEX)rd.idx(3, 40);
    gw_INDEX s = (gw_INDEX)rd.idx(3, 40);
    if (p > n || q > m || r != n + p + 1 || s != m + q + 1)
        throw Bad();
    std::vector<double> v;
    for (int64_t i = 0; i < 4 * (n + 1) * (m + 1) + r + 1 + s + 1; i++)
        v.push_back(rd.num());
    gw_SURFACE* sur = sm_AllocateNurbSurface(n, m, p, q, r, s);
    size_t k = 0;
    for (gw_INDEX i = 0; i <= n; i++)
        for (gw_INDEX j = 0; j <= m; j++)
        {
            sur->net->Pw[i][j].x = v[k++];
            sur->net->Pw[i][j].y = v[k++];
            sur->net->Pw[i][j].z = v[k++];
            sur->net->Pw[i][j].w = v[k++];
        }
    for (gw_INDEX i = 0; i <= r; i++)
        sur->knu->U[i] = v[k++];
    for (gw_INDEX i = 0; i <= s; i++)
        sur->knv->U[i] = v[k++];
    return new (ctx) SmBSplineSurface(sur, FALSE, &ctx);
}

// Another surface: `0 plane-block` or `1 bspline-surface`; NULL when the plane block fails.
SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx(0, 1);
    if (kind == 0)
    {
        SmStatus s = SM_SUCCESS;
        SmPlane* p = read_plane(rd, ctx, &s);
        if (p == NULL)
            throw Bad();
        return p;
    }
    return read_bspline_surface(rd, ctx);
}

// A curve: `0 a b` (CreateLineSegment, dim 3) or `1 dim n p m cpts(x y z w) knots`.
SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx(0, 1);
    if (kind == 0)
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine* l = NULL;
        if (SmLine::CreateLineSegment(ctx, 3, a, b, l) != SM_SUCCESS || l == NULL)
            throw Bad();
        return l;
    }
    ULONG dim = (ULONG)rd.idx(2, 3);
    int64_t n = rd.idx(1, 32);
    int64_t p = rd.idx(1, 8);
    int64_t m = rd.idx(3, 64);
    if (p > n || m != n + p + 1)
        throw Bad();
    std::vector<double> v;
    for (int64_t i = 0; i < 4 * (n + 1) + m + 1; i++)
        v.push_back(rd.num());
    gw_CURVE* cur = sm_AllocateNurbCurve((gw_INDEX)n, (gw_DEGREE)p, (gw_INDEX)m);
    for (int64_t i = 0; i <= n; i++)
    {
        cur->pol->Pw[i].x = v[4 * i];
        cur->pol->Pw[i].y = v[4 * i + 1];
        cur->pol->Pw[i].z = v[4 * i + 2];
        cur->pol->Pw[i].w = v[4 * i + 3];
    }
    for (int64_t i = 0; i <= m; i++)
        cur->knt->U[i] = v[4 * (n + 1) + i];
    SmBSplineCurve* c = new (ctx) SmBSplineCurve(dim, (ULONG)cur->pol->n, (ULONG)cur->knt->m, (short)cur->p,
                                                 cur->pol->Pw, cur->knt->U, &ctx);
    smos_Free(cur);
    return c;
}

void put_surface_nurb(std::vector<double>& o, const gw_SURFACE* sur)
{
    o.push_back(sur ? 1.0 : 0.0);
    if (!sur)
        return;
    gw_INDEX n = sur->net->n;
    gw_INDEX m = sur->net->m;
    gw_INDEX r = sur->knu->m;
    gw_INDEX s = sur->knv->m;
    o.push_back((double)n);
    o.push_back((double)m);
    o.push_back((double)sur->p);
    o.push_back((double)sur->q);
    o.push_back((double)r);
    o.push_back((double)s);
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
    for (gw_INDEX i = 0; i <= s; i++)
        o.push_back(sur->knv->U[i]);
}

// A copy of `a` that leaves `a` alone: the copy constructor makes the source's missing NURB
// (through a const_cast), so a plane without NURB is rebuilt from its analytic data instead,
// then given the source's exact placement.
SmPlane* side_free_copy(const SmContext& ctx, const SmPlane* a)
{
    if (a->GetGwNurbPointer() != NULL)
        return new (ctx) SmPlane(*a);
    const SmAxis2Placement& pos = a->GetPosition();
    SmPlane* c = new (ctx) SmPlane(pos.GetOrigin(), pos.GetXAxis(), pos.GetYAxis(), a->GetUVScale(),
                                   a->GetSTEPUVDomain(), &ctx);
    c->GetOutOfBoundsEnabled() = a->IsOutOfBoundsEnabled();
    // SetCanonical is not idempotent for a degenerate placement: copy it exactly
    c->SetPosition(pos);
    return c;
}

// type O X Y Z domain scale oob kt form, then the NURB (made on a copy when missing).
void put_state(std::vector<double>& o, const SmContext& ctx, const SmPlane* a)
{
    const SmAxis2Placement& pos = a->GetPosition();
    o.push_back((double)a->GetType());
    put_vec(o, pos.GetOrigin());
    put_vec(o, pos.GetXAxis());
    put_vec(o, pos.GetYAxis());
    put_vec(o, pos.GetZAxis());
    put_dom(o, a->GetSTEPUVDomain());
    SmVector2d sc = a->GetUVScale();
    put_uv(o, sc);
    o.push_back(flg(a->IsOutOfBoundsEnabled()));
    o.push_back((double)(a->*PeekS::kt()));
    o.push_back((double)(a->*PeekS::form()));
    const gw_SURFACE* sur = a->GetGwNurbPointer();
    if (sur)
    {
        put_surface_nurb(o, sur);
        return;
    }
    SmPlane* tmp = side_free_copy(ctx, a);
    put_surface_nurb(o, tmp->GetOrCreateGwNurbPointer());
    delete tmp;
}

void put_curve_nurb(std::vector<double>& o, const SmBSplineCurve* c)
{
    o.push_back((double)(c->*PeekC::kt()));
    o.push_back((double)(c->*PeekC::form()));
    const gw_CURVE* cur = c->GetGwNurbPointer();
    o.push_back(cur ? 1.0 : 0.0);
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

// type dim [line: P V dom scale inside_out] B-spline part, natural interval.
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    o.push_back((double)c->GetType());
    o.push_back((double)c->GetDim());
    if (c->IsKindOf(SmLine_TYPE))
    {
        const SmLine* l = (const SmLine*)c;
        put_vec(o, l->GetLinePoint());
        put_vec(o, l->GetLineVector());
        put_ivl(o, l->GetSTEPInterval());
        o.push_back(l->GetLineScale());
        o.push_back(l->GetInsideOut() ? 1.0 : 0.0);
    }
    if (!c->IsKindOf(SmBSplineCurve_TYPE))
        throw Bad();
    put_curve_nurb(o, (const SmBSplineCurve*)c);
    put_ivl(o, c->GetNaturalInterval());
}

double obj_id(const SmObject* p, const SmObject* a)
{
    if (p == NULL)
        return -1.0;
    return p == a ? 0.0 : 1.0;
}

void put_sols(std::vector<double>& o, SmSolutionArray& sols, const SmObject* a)
{
    o.push_back((double)sols.GetSize());
    for (ULONG i = 0; i < sols.GetSize(); i++)
    {
        const SmSolution& s = sols[i];
        o.push_back((double)s.m_eSolutionType);
        o.push_back((double)s.m_lNumVariables);
        o.push_back((double)s.m_lNumObjects);
        o.push_back(s.m_lNumObjects > 0 ? obj_id(s.m_apObjects[0], a) : -1.0);
        o.push_back(s.m_vStart.m_dSolutionValue);
        for (ULONG k = 0; k < 3; k++)
            o.push_back(k < s.m_lNumVariables ? s.m_vStart[k] : 0.0);
        bool range = s.m_eSolutionType == SM_ST_RANGE_OF_VALUES;
        o.push_back(range ? s.m_vEnd.m_dSolutionValue : 0.0);
        for (ULONG k = 0; k < 3; k++)
            o.push_back(range && k < s.m_lNumVariables ? s.m_vEnd[k] : 0.0);
    }
}

// Domain `kind a b c d`: 0 STEP domain, 1 SmExtent2d(a, b, c, d), 2 natural domain (of a copy).
SmExtent2d read_dom(Rd& rd, const SmContext& ctx, const SmPlane* a)
{
    int64_t kind = rd.idx(0, 2);
    SmExtent2d d = rd.dom4();
    if (kind == 0)
        return a->GetSTEPUVDomain();
    if (kind == 2)
    {
        SmPlane* tmp = side_free_copy(ctx, a);
        SmExtent2d e = tmp->GetNaturalUVDomain();
        delete tmp;
        return e;
    }
    return d;
}

// The file-static sm_IsGrazingIntersection of SmPlane.cpp, copied verbatim (not exported).
SmBoolean is_grazing_intersection(const SmCurve* cpCurve, double dCurveParam, const SmLine* cpLine,
                                  double dLineParam)
{
    SmVector3d sCurveEval[3];
    SmVector3d sLineEval[2];
    cpCurve->Evaluate(dCurveParam, 2, FALSE, sCurveEval);
    cpLine->Evaluate(dLineParam, 1, FALSE, sLineEval);
    if (!sCurveEval[1].IsParallelTo(sLineEval[1]))
    {
        return FALSE;
    }
    SmVector3d sCross(sCurveEval[1] * sCurveEval[2]);
    if (sCross.LengthSquared() > SM_EFF_ZERO_SQ)
    {
        return TRUE;
    }
    return FALSE;
}

void run_op(int32_t op, Rd& rd, const SmContext& ctx, SmPlane* a, std::vector<double>& o)
{
    switch (op)
    {
    case OP_STATE:
        o.push_back(0.0);
        put_state(o, ctx, a);
        break;
    case OP_EVALUATE:
    {
        SmPoint2d uv = rd.uv();
        ULONG hu = (ULONG)rd.idx(0, 4);
        ULONG hv = (ULONG)rd.idx(0, 4);
        bool upper = rd.flag();
        if (upper && hu > hv)
            throw Bad();
        std::vector<SmVector3d> d((hu + 1) * (hv + 1));
        for (size_t i = 0; i < d.size(); i++)
            d[i].Set(0.0, 0.0, 0.0);
        SmStatus s = a->Evaluate(uv, hu, hv, TRUE, TRUE, upper ? TRUE : FALSE, d.data());
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            for (size_t i = 0; i < d.size(); i++)
                put_vec(o, d[i]);
        break;
    }
    case OP_EVAL_POINT:
    {
        SmPoint2d uv = rd.uv();
        SmPoint3d p;
        SmStatus s = a->EvaluatePoint(uv, p);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, p);
        break;
    }
    case OP_EVAL_FAST:
    {
        SmPoint2d uv = rd.uv();
        SmPoint3d p;
        SmStatus s = a->EvaluatePointFast(uv, p);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, p);
        break;
    }
    case OP_NORMAL:
    {
        SmPoint2d uv = rd.uv();
        SmVector3d nv;
        SmStatus s = a->EvaluateNormal(uv, TRUE, TRUE, nv);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, nv);
        break;
    }
    case OP_PROJECT:
    {
        SmPoint3d p = rd.vec();
        SmPoint2d uv;
        SmStatus s = a->ProjectPointToUVDomain(p, uv);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_uv(o, uv);
        break;
    }
    case OP_QUERIES:
    {
        SmPoint2d uv = rd.uv();
        o.push_back(0.0);
        o.push_back(flg(a->IsBounded()));
        SmPoint3d pp;
        SmVector3d pn;
        double dev = -7.0;
        o.push_back(flg(a->IsPlanar(SM_EFF_ZERO, &pp, &pn, &dev)));
        put_vec(o, pp);
        put_vec(o, pn);
        o.push_back(dev);
        SmSurfParamType sd = SM_SP_UNKNOWN;
        SmTArray<SmPoint2d> cross;
        cross.Add(uv);
        o.push_back(flg(a->IsOnSeam(uv, NULL, &sd, &cross)));
        o.push_back((double)sd);
        o.push_back((double)cross.GetSize());
        SmContinuityType ct = SM_CT_UNDEFINED;
        o.push_back(flg(a->IsClosed(a->GetSTEPUVDomain(), SM_SP_U, NULL, &ct)));
        o.push_back((double)ct);
        SmSurfParamType d1 = SM_SP_UNKNOWN;
        o.push_back(flg(a->IsSingularity(uv, d1)));
        o.push_back((double)d1);
        SmSurfParamType d2 = SM_SP_UNKNOWN;
        o.push_back(flg(a->IsSingularity(uv, 0, NULL, d2)));
        o.push_back((double)d2);
        o.push_back((double)a->GetType());
        put_chars(o, a->GetTypeString());
        o.push_back(-1.0);
        put_chars(o, a->GetClassString());
        break;
    }
    case OP_KIND_OF:
    {
        double t = (double)rd.idx(0, 100000);
        o.push_back(0.0);
        o.push_back(flg(a->IsKindOf((SM_TYPE)t)));
        break;
    }
    case OP_ADJUST:
    {
        SmExtent2d d = read_dom(rd, ctx, a);
        o.push_back(st(a->AdjustSTEPUVDomain(d)));
        break;
    }
    case OP_MAKE_NURB: o.push_back(st(a->MakeNurb())); break;
    case OP_REPARAM:
    {
        SmExtent2d d = read_dom(rd, ctx, a);
        o.push_back(st(a->Reparameterize(d)));
        break;
    }
    case OP_TOGGLE:
        a->ToggleSwapUVBit();
        o.push_back(0.0);
        break;
    case OP_REBUILD:
        // upstream dereferences a NULL NURB (SmBSplineSurface::EvaluatePoint) when none can be made
        if (a->GetOrCreateGwNurbPointer() == NULL)
        {
            o.push_back(st(SM_ERR)); // the port returns SM_ERR (no NURB)
            break;
        }
        o.push_back(st(a->RebuildSTEPFromNURBParameters()));
        break;
    case OP_DROP:
    {
        SmExtent2d d = read_dom(rd, ctx, a);
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx(0, 5);
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        bool has = rd.flag();
        double target = rd.num();
        SmSolutionArray sols;
        SmStatus s = a->DropPointFast(d, sop, p, NULL, tol, has ? &target : NULL, SM_SR_ALL, sols);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_sols(o, sols, a);
        break;
    }
    case OP_LINE:
    {
        SmExtent2d d = read_dom(rd, ctx, a);
        SmPoint3d p = rd.vec();
        SmVector3d v = rd.vec();
        bool has = rd.flag();
        double i0 = rd.num();
        double i1 = rd.num();
        bool ray = rd.flag();
        double tol = rd.num();
        SmExtent1d ivl(i0, i1);
        SmSolutionArray sols;
        SmStatus s = a->GlobalLineIntersect(d, p, v, has ? &ivl : NULL, ray ? TRUE : FALSE, tol, sols);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_sols(o, sols, a);
        break;
    }
    case OP_TRIM_LINE:
    {
        SmExtent2d d = read_dom(rd, ctx, a);
        SmPoint3d p0 = rd.vec();
        SmPoint3d p1 = rd.vec();
        double tol = rd.num();
        SmPoint3d t0, t1;
        SmBoolean found = FALSE;
        SmStatus s = a->TrimPlaneLineToPlaneDomain(d, p0, p1, tol, t0, t1, found);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(found));
            if (found)
            {
                put_vec(o, t0);
                put_vec(o, t1);
            }
        }
        break;
    }
    case OP_ISO:
    {
        SmSurfParamType param = rd.idx(0, 1) == 0 ? SM_SP_U : SM_SP_V;
        double iso = rd.num();
        bool has = rd.flag();
        SmExtent2d d = rd.dom4();
        bool want = rd.flag();
        SmBSplineCurve* c = NULL;
        SmCurve* uv = NULL;
        double gap = -7.0;
        SmStatus s = a->CreateIsoParametricCurve(ctx, param, iso, 0.0, c, has ? &d : NULL, &gap, want ? &uv : NULL);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            if (c == NULL)
                throw Bad();
            put_curve(o, c);
            o.push_back(gap);
            o.push_back(uv ? 1.0 : 0.0);
            if (uv)
                put_curve(o, uv);
        }
        delete c;
        delete uv;
        break;
    }
    case OP_DROP_CURVE:
    {
        SmExtent2d d = read_dom(rd, ctx, a);
        SmCurve* c = read_curve(rd, ctx);
        bool given = rd.flag();
        double i0 = rd.num();
        double i1 = rd.num();
        double tol = rd.num();
        bool keep = rd.flag();
        bool creating = rd.flag();
        SmExtent1d ivl = given ? SmExtent1d(i0, i1) : c->GetNaturalInterval();
        double dmax = -7.0;
        double ddev = -7.0;
        SmTArray<SmBSplineCurve*> arr;
        SmStatus s = a->DropCurve(ctx, d, *c, ivl, tol, dmax, ddev, arr, keep ? TRUE : FALSE, NULL,
                                  creating ? TRUE : FALSE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(dmax);
            o.push_back(ddev);
            o.push_back((double)arr.GetSize());
            for (ULONG i = 0; i < arr.GetSize(); i++)
                put_curve(o, arr[i]);
        }
        for (ULONG i = 0; i < arr.GetSize(); i++)
            delete arr[i];
        delete c;
        break;
    }
    case OP_COINCIDENCE:
    {
        SmSurface* other = read_surface(rd, ctx);
        double tol = rd.num();
        bool whole = rd.flag();
        SmBoolean coinc = UNSURE;
        double dist = -7.0;
        SmSurface* containing = NULL;
        SmStatus s = a->CoincidenceCheck(*other, tol, coinc, dist, whole ? TRUE : FALSE, containing);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(coinc));
            o.push_back(dist);
        }
        delete other;
        break;
    }
    case OP_EQ:
    {
        SmStatus ms = SM_SUCCESS;
        SmPlane* b = read_plane(rd, ctx, &ms);
        if (b == NULL)
            throw Bad();
        o.push_back(0.0);
        o.push_back(flg(*a == *b));
        delete b;
        break;
    }
    case OP_COPY:
    {
        SmPlane* c = NULL;
        SmStatus s = a->Copy(ctx, c);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_state(o, ctx, c);
        delete c;
        break;
    }
    case OP_GRAZING:
    {
        SmCurve* c = read_curve(rd, ctx);
        double cp = rd.num();
        SmPoint3d l0 = rd.vec();
        SmPoint3d l1 = rd.vec();
        double lp = rd.num();
        SmLine* l = NULL;
        if (SmLine::CreateLineSegment(ctx, 3, l0, l1, l) != SM_SUCCESS || l == NULL)
            throw Bad();
        o.push_back(0.0);
        o.push_back(flg(is_grazing_intersection(c, cp, l, lp)));
        delete l;
        delete c;
        break;
    }
    case OP_INHERITED:
    {
        SmVector3d v = rd.vec();
        SmPlane* tmp = side_free_copy(ctx, a);
        o.push_back(0.0);
        put_dom(o, tmp->GetNaturalUVDomain());
        put_dom(o, tmp->GetMaxAnalyticDomain());
        o.push_back(flg(tmp->IsBilinear()));
        o.push_back(flg(tmp->IsParallelToVector(v)));
        o.push_back((double)tmp->GetDegree(SM_SP_U));
        o.push_back((double)tmp->GetDegree(SM_SP_V));
        o.push_back((double)tmp->GetNumberControlPoints(SM_SP_U));
        o.push_back((double)tmp->GetNumberControlPoints(SM_SP_V));
        delete tmp;
        break;
    }
    case OP_REVERSE:
    {
        SmSurfParamType param = rd.idx(0, 1) == 0 ? SM_SP_U : SM_SP_V;
        // upstream passes a NULL NURB to N_SrfReverse when none can be made
        if (a->GetOrCreateGwNurbPointer() == NULL)
        {
            o.push_back(st(SM_ERR)); // the port returns SM_ERR (no NURB)
            break;
        }
        o.push_back(st(a->Reverse(param)));
        break;
    }
    case OP_SWAP: o.push_back(st(a->SwapUV())); break;
    case OP_EXTEND:
    {
        bool has_dir = rd.flag();
        SmSurfParamType dir = (SmSurfParamType)rd.idx(0, SM_SP_VMAX);
        double dist = rd.num();
        SmContinuityType cont = (SmContinuityType)rd.idx(0, SM_CT_CINFINITY);
        SmPlane* tmp = side_free_copy(ctx, a);
        // upstream copies a NULL NURB (N_SrfCopy) when none can be made
        if (tmp->GetOrCreateGwNurbPointer() == NULL)
        {
            delete tmp;
            o.push_back(st(SM_ERR)); // the port returns SM_ERR (no NURB)
            break;
        }
        SmSurface* e = NULL;
        SmStatus stt = has_dir ? tmp->CreateExtendedSurface(ctx, dir, dist, cont, e)
                               : tmp->CreateExtendedSurface(ctx, dist, cont, e);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            SmBSplineSurface* bs = e ? SM_CAST_PTR(SmBSplineSurface, e) : NULL;
            put_surface_nurb(o, bs ? bs->GetGwNurbPointer() : NULL);
        }
        delete e;
        delete tmp;
        break;
    }
    default: throw Bad();
    }
}
} // namespace

SC_EXPORT int32_t sc_surface_plane_p1_run(const double* in, int64_t n_in, double* out, int64_t cap,
                                          int64_t* out_len)
{
    if (in == NULL || n_in < 2)
        return SC_ERR_ARG;
    try
    {
        SC_NATIVE_LOCK;
        SmContext ctx;
        std::vector<double> o;
        Rd rd{in, n_in, 0};
        SmStatus ms = SM_SUCCESS;
        SmPlane* a = read_plane(rd, ctx, &ms);
        o.push_back(st(ms));
        if (a != NULL)
        {
            int64_t n_ops = rd.idx(0, 64);
            for (int64_t i = 0; i < n_ops; i++)
            {
                int32_t op = (int32_t)rd.idx(1, OP_LAST);
                o.push_back((double)op);
                run_op(op, rd, ctx, a, o);
            }
            put_state(o, ctx, a);
            delete a;
        }
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    catch (const Bad&)
    {
        return SC_ERR_ARG;
    }
    catch (...)
    {
        return SC_ERR_EXCEPTION;
    }
}
