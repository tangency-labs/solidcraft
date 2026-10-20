// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.line`: the SmLine members owned by the unit
// (source/SMLib/src/SmLine.cpp, source/SMLib/inc/SmLine.h), called on SmLine objects (virtual
// dispatch is upstream's), plus the inherited SmBSplineCurve members the port re-dispatches
// for lines (GetEnds, EditParameterization, CreateAnalyticCurve).
//
// One entry point, sc_curve_line_run(in, n_in, out, cap, out_len). `in` is a program: a line
// block (curve A), the op count, then the ops; layouts are documented in
// crates/solidcore-ffi/src/units/curve_line.rs. Every read is bounds checked here (a bad
// program is SC_ERR_ARG). `out` receives `make_status`, then per op `op status [values]`
// (values only when the status is 0), then the state of A. A failed line block ends the
// record after its status.
#include "sc_shim.h"

#include <SmAssertArray.h>
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmContext.h>
#include <SmCurveTypes.h>
#include <SmDatabaseIO.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmPolarBox.h>
#include <SmPseudoBox.h>
#include <SmSolutionArray.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

#include "sc_sm.h"

using sc::put_vec;
using sc::st;

namespace
{
// Line blocks (keep in sync with crates/solidcore-ffi/src/units/curve_line.rs).
enum : int32_t
{
    MK_DEFAULT = 0,
    MK_POINTS = 1,
    MK_INFINITE = 2,
    MK_BOUNDED = 3,
    MK_SEGMENT = 4,
    MK_CANONICAL = 5,
    MK_READ = 6,
};

// Ops (keep in sync with crates/solidcore-ffi/src/units/curve_line.rs).
enum : int32_t
{
    OP_STATE = 1,
    OP_EVALUATE = 2,
    OP_EVALUATE_STEP = 3,
    OP_EVALUATE_POINT = 4,
    OP_TO_STEP = 5,
    OP_TO_NURBS = 6,
    OP_IS_LINE = 7,
    OP_QUERIES = 8,
    OP_IS_DEGENERATE = 9,
    OP_DOMAIN_BOUNDARY = 10,
    OP_APPROX_LENGTH = 11,
    OP_LENGTH = 12,
    OP_ENDS = 13,
    OP_BBOX = 14,
    OP_TIGHT_BBOX = 15,
    OP_TESSELLATE = 16,
    OP_DROP = 17,
    OP_GPS = 18,
    OP_STEP_INVERSION = 19,
    OP_INTERSECT_LINE = 20,
    OP_INTERSECT_ELLIPSE = 21,
    OP_JOIN = 22,
    OP_ADJUST = 23,
    OP_TO_2D = 24,
    OP_TO_3D = 25,
    OP_OFFSET = 26,
    OP_REFRESH = 27,
    OP_REVERSE = 28,
    OP_TRANSFORM = 29,
    OP_DEFORM = 30,
    OP_TRIM = 31,
    OP_EDIT_PARAM = 32,
    OP_EQ = 33,
    OP_COPY = 34,
    OP_ANALYTIC = 35,
    OP_ASSERT = 36,
    OP_KIND_OF = 37,
    OP_CANONICAL = 38,
    OP_SET_SCALE = 39,
    OP_SET_DOMAIN = 40,
};

// Reads the protected STEP flags through pointers to members named in a derived class.
struct Peek : SmBSplineCurve
{
    static SmKnotType SmBSplineCurve::*kt() { return &Peek::m_eKnotType; }
    static SmBSplineCurveForm SmBSplineCurve::*form() { return &Peek::m_eBSplineCurveForm; }
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
};

void put_ivl(std::vector<double>& o, const SmExtent1d& e)
{
    o.push_back(e.GetMin());
    o.push_back(e.GetMax());
}

// `n p m cpts(x y z w) knots` with m == n + p + 1, 1 <= p <= n <= 64.
gw_CURVE* read_nurb(Rd& rd)
{
    int64_t n = rd.idx(1, 64);
    int64_t p = rd.idx(1, 16);
    int64_t m = rd.idx(3, 200);
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
    return cur;
}

// A B-spline block `dim n p m cpts knots` (SmBSplineCurve(ULONG, n, m, p, Pw, U)).
SmBSplineCurve* read_bspline(Rd& rd, const SmContext& ctx)
{
    ULONG dim = (ULONG)rd.idx(2, 3);
    gw_CURVE* cur = read_nurb(rd);
    SmBSplineCurve* c = new (ctx) SmBSplineCurve(dim, (ULONG)cur->pol->n, (ULONG)cur->knt->m,
                                                 (short)cur->p, cur->pol->Pw, cur->knt->U, &ctx);
    smos_Free(cur);
    return c;
}

// A line block; NULL with *status set when the construction fails.
SmLine* read_line(Rd& rd, const SmContext& ctx, SmStatus* status)
{
    *status = SM_SUCCESS;
    int64_t kind = rd.idx(0, 6);
    switch (kind)
    {
    case MK_DEFAULT: return new (ctx) SmLine();
    case MK_POINTS:
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        ULONG dim = (ULONG)rd.idx(2, 3);
        return new (ctx) SmLine(a, b, dim, &ctx);
    }
    case MK_INFINITE:
    {
        SmPoint3d p = rd.vec();
        SmVector3d v = rd.vec();
        ULONG dim = (ULONG)rd.idx(2, 3);
        bool io = rd.flag();
        return new (ctx) SmLine(p, v, dim, TRUE, &ctx, io ? TRUE : FALSE);
    }
    case MK_BOUNDED:
    {
        SmPoint3d p = rd.vec();
        SmVector3d v = rd.vec();
        double a = rd.num();
        double b = rd.num();
        double scale = rd.num();
        ULONG dim = (ULONG)rd.idx(2, 3);
        bool io = rd.flag();
        bool has = rd.flag();
        SmBSplineCurve* nurb = has ? read_bspline(rd, ctx) : NULL;
        SmLine* l = new (ctx) SmLine(p, v, SmExtent1d(a, b), scale, dim, &ctx, nurb, io ? TRUE : FALSE);
        delete nurb;
        return l;
    }
    case MK_SEGMENT:
    {
        ULONG dim = (ULONG)rd.idx(2, 3);
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        bool has = rd.flag();
        double i0 = rd.num();
        double i1 = rd.num();
        SmExtent1d ivl(i0, i1);
        SmLine* l = NULL;
        *status = SmLine::CreateLineSegment(ctx, dim, a, b, l, has ? &ivl : NULL);
        if (*status != SM_SUCCESS)
        {
            delete l;
            return NULL;
        }
        return l;
    }
    case MK_CANONICAL:
    {
        SmPoint3d p = rd.vec();
        SmVector3d v = rd.vec();
        SmLine* l = NULL;
        *status = SmLine::CreateCanonical(ctx, p, v, l);
        return l;
    }
    default:
    {
        ULONG dim = (ULONG)rd.idx(2, 3);
        ULONG version = (ULONG)rd.idx(0, 100);
        bool binary = rd.flag();
        int64_t nb = rd.idx(0, 1 << 20);
        std::string bytes;
        for (int64_t i = 0; i < nb; i++)
            bytes.push_back((char)(unsigned char)rd.idx(0, 255));
        std::istringstream iss(bytes);
        SmDatabaseIOStream db(binary ? SM_BINARY : SM_ASCII, &iss);
        SmCurve* c = NULL;
        *status = SmLine::ReadFromDB(SmLine_TYPE, db, dim, ctx, c, version);
        if (*status != SM_SUCCESS)
        {
            delete c;
            return NULL;
        }
        return (SmLine*)c;
    }
    }
}

void put_nurb(std::vector<double>& o, const SmBSplineCurve* c)
{
    o.push_back((double)(c->*Peek::kt()));
    o.push_back((double)(c->*Peek::form()));
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

// type dim point vector domain scale inside_out, the B-spline part, the natural interval.
void put_line(std::vector<double>& o, const SmLine* l)
{
    o.push_back((double)l->GetType());
    o.push_back((double)l->GetDim());
    put_vec(o, l->GetLinePoint());
    put_vec(o, l->GetLineVector());
    put_ivl(o, l->GetSTEPInterval());
    o.push_back(l->GetLineScale());
    o.push_back(l->GetInsideOut() ? 1.0 : 0.0);
    put_nurb(o, l);
    put_ivl(o, l->GetNaturalInterval());
}

// Interval `kind a b`: 0 natural interval, 1 SmExtent1d(a, b), 2 STEP interval.
SmExtent1d read_ivl(Rd& rd, const SmLine* l)
{
    int64_t kind = rd.idx(0, 2);
    double a = rd.num();
    double b = rd.num();
    if (kind == 0)
        return l->GetNaturalInterval();
    if (kind == 2)
        return l->GetSTEPInterval();
    return SmExtent1d(a, b);
}

double obj_id(const SmObject* p, const SmObject* a, const SmObject* b)
{
    if (p == NULL)
        return -1.0;
    if (p == a)
        return 0.0;
    if (p == b)
        return 1.0;
    return -2.0;
}

void put_sols(std::vector<double>& o, SmSolutionArray& sols, const SmObject* a, const SmObject* b)
{
    o.push_back((double)sols.GetSize());
    for (ULONG i = 0; i < sols.GetSize(); i++)
    {
        SmSolution& s = sols[i];
        o.push_back((double)s.m_eSolutionType);
        o.push_back((double)s.m_lNumVariables);
        o.push_back((double)s.m_lNumObjects);
        o.push_back(obj_id(s.m_apObjects[0], a, b));
        o.push_back(obj_id(s.m_apObjects[1], a, b));
        o.push_back(s.m_vStart.m_dSolutionValue);
        o.push_back(s.m_vStart[0]);
        o.push_back(s.m_vStart[1]);
        o.push_back(s.m_vEnd.m_dSolutionValue);
        o.push_back(s.m_vEnd[0]);
        o.push_back(s.m_vEnd[1]);
    }
}

void put_chars(std::vector<double>& o, const TCHAR* s)
{
    for (; *s; ++s)
        o.push_back((double)*s);
}

// Runs one op on `a`; appends `status [values]`.
void run_op(int32_t op, Rd& rd, const SmContext& ctx, SmLine* a, std::vector<double>& o)
{
    switch (op)
    {
    case OP_STATE:
        o.push_back(0.0);
        put_line(o, a);
        break;
    case OP_EVALUATE:
    case OP_EVALUATE_STEP:
    {
        double t = rd.num();
        int64_t n = rd.idx(0, 8);
        std::vector<SmVector3d> pd((size_t)n + 1, SmVector3d(0.0, 0.0, 0.0));
        SmStatus s = op == OP_EVALUATE ? a->Evaluate(t, (ULONG)n, TRUE, pd.data())
                                       : a->EvaluateSTEP(t, (ULONG)n, TRUE, pd.data());
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            for (const SmVector3d& v : pd)
                put_vec(o, v);
        break;
    }
    case OP_EVALUATE_POINT:
    {
        double t = rd.num();
        SmPoint3d p;
        SmStatus s = a->EvaluatePoint(t, p);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, p);
        break;
    }
    case OP_TO_STEP:
    case OP_TO_NURBS:
    {
        double t = rd.num();
        double r = 0.0;
        SmStatus s = op == OP_TO_STEP ? a->ConvertTFromNURBSToSTEP(t, r) : a->ConvertTFromSTEPToNURBS(t, r);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back(r);
        break;
    }
    case OP_IS_LINE:
    {
        SmPoint3d p;
        SmVector3d v;
        SmBoolean b = a->IsLine(5, 0.0, p, v);
        o.push_back(0.0);
        o.push_back(b ? 1.0 : 0.0);
        put_vec(o, p);
        put_vec(o, v);
        break;
    }
    case OP_QUERIES:
    {
        SmAxis2Placement frame;
        double r = 0.0, s0 = 0.0, s1 = 0.0;
        o.push_back(0.0);
        o.push_back(a->IsBounded() ? 1.0 : 0.0);
        o.push_back(a->IsAnalytic() ? 1.0 : 0.0);
        o.push_back(a->IsClosed(a->GetNaturalInterval(), 0.0) ? 1.0 : 0.0);
        o.push_back(a->IsArc(5, 0.0, frame, r, s0, s1) ? 1.0 : 0.0);
        o.push_back((double)a->GetDegree());
        o.push_back((double)a->GetNumberNaturalKnots());
        put_ivl(o, a->GetMaxAnalyticDomain());
        put_ivl(o, a->GetSTEPInterval());
        o.push_back((double)a->GetClassType());
        put_chars(o, a->GetTypeString());
        o.push_back(-1.0);
        put_chars(o, a->GetClassString());
        break;
    }
    case OP_IS_DEGENERATE:
    {
        double tol = rd.num();
        bool has = rd.flag();
        SmExtent1d ivl = read_ivl(rd, a);
        o.push_back(0.0);
        o.push_back(a->IsDegenerate(tol, has ? &ivl : NULL) ? 1.0 : 0.0);
        break;
    }
    case OP_DOMAIN_BOUNDARY:
    {
        double u0 = rd.num();
        double v0 = rd.num();
        double u1 = rd.num();
        double v1 = rd.num();
        double tol = rd.num();
        SmExtent2d dom(SmPoint2d(u0, v0), SmPoint2d(u1, v1));
        o.push_back(0.0);
        o.push_back((double)a->IsDomainBoundary(dom, tol));
        break;
    }
    case OP_APPROX_LENGTH:
    {
        SmExtent1d ivl = read_ivl(rd, a);
        bool turn = rd.flag();
        double ang = -7.0;
        double len = a->ApproximateLength(ivl, 5, turn ? &ang : NULL);
        o.push_back(0.0);
        o.push_back(len);
        o.push_back(ang);
        break;
    }
    case OP_LENGTH:
    {
        SmExtent1d ivl = read_ivl(rd, a);
        double len = 0.0;
        SmStatus s = a->Length(ivl, 0.0, len);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            o.push_back(len);
        break;
    }
    case OP_ENDS:
    {
        SmPoint3d s0, s1;
        double w0 = 0.0, w1 = 0.0;
        a->GetEnds(s0, s1, &w0, &w1);
        o.push_back(0.0);
        put_vec(o, s0);
        put_vec(o, s1);
        o.push_back(w0);
        o.push_back(w1);
        break;
    }
    case OP_BBOX:
    case OP_TIGHT_BBOX:
    {
        SmExtent1d ivl = read_ivl(rd, a);
        int64_t flags = op == OP_BBOX ? rd.idx(1, 7) : 1;
        SmExtent3d nb;
        SmPseudoBox pb;
        SmPolarBox xb;
        SmStatus s = op == OP_BBOX
                         ? a->CalculateBoundingBox(ivl, (flags & 1) ? &nb : NULL, (flags & 2) ? &pb : NULL,
                                                   (flags & 4) ? &xb : NULL)
                         : a->CalculateTightBoundingBox(ivl, &nb);
        o.push_back(st(s));
        if (s != SM_SUCCESS)
            break;
        if (flags & 1)
        {
            put_vec(o, nb.GetMin());
            put_vec(o, nb.GetMax());
        }
        if (flags & 2)
        {
            for (ULONG k = 0; k < 3; ++k)
                put_vec(o, pb.GetBasis(k));
            for (ULONG k = 0; k < 3; ++k)
                put_ivl(o, pb.GetInterval(k));
        }
        if (flags & 4)
        {
            o.push_back((double)xb.GetPolarBoxState());
            for (ULONG k = 0; k < 3; ++k)
                put_vec(o, *xb.GetBasis(k));
            SmExtent2d d = xb.GetDomain();
            o.push_back(d.GetMin().x);
            o.push_back(d.GetMin().y);
            o.push_back(d.GetMax().x);
            o.push_back(d.GetMax().y);
        }
        break;
    }
    case OP_TESSELLATE:
    {
        SmExtent1d ivl = read_ivl(rd, a);
        int64_t nseg = rd.idx(0, 64);
        int64_t flags = rd.idx(0, 7);
        SmTArray<double> params;
        SmTArray<SmPoint3d> pts, tans;
        params.Add(-99.0); // must be reset by the call
        SmStatus s = a->Tessellate(ivl, 0.1, 15.0, (ULONG)nseg, (flags & 1) ? &params : NULL,
                                   (flags & 2) ? &pts : NULL, (flags & 4) ? &tans : NULL);
        o.push_back(st(s));
        if (s != SM_SUCCESS)
            break;
        if (flags & 1)
        {
            o.push_back((double)params.GetSize());
            for (ULONG i = 0; i < params.GetSize(); i++)
                o.push_back(params[i]);
        }
        if (flags & 2)
        {
            o.push_back((double)pts.GetSize());
            for (ULONG i = 0; i < pts.GetSize(); i++)
                put_vec(o, pts[i]);
        }
        if (flags & 4)
        {
            o.push_back((double)tans.GetSize());
            for (ULONG i = 0; i < tans.GetSize(); i++)
                put_vec(o, tans[i]);
        }
        break;
    }
    case OP_DROP:
    case OP_GPS:
    {
        SmExtent1d ivl = read_ivl(rd, a);
        SmSolverOperationType sop = (SmSolverOperationType)rd.idx(0, 6);
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        bool has = op == OP_DROP ? rd.flag() : false;
        double target = op == OP_DROP ? rd.num() : 0.0;
        SmSolutionArray sols;
        SmStatus s = op == OP_DROP
                         ? a->DropPointFast(ivl, sop, p, NULL, tol, has ? &target : NULL, SM_SR_SINGLE, sols)
                         : a->GlobalPointSolveSTEP(ivl, sop, p, tol, NULL, NULL, SM_SR_ALL, sols);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_sols(o, sols, a, NULL);
        break;
    }
    case OP_STEP_INVERSION:
    {
        SmPoint3d p = rd.vec();
        double t = 0.0;
        SmCurveLocationType loc = SM_CL_UNKNOWN;
        SmStatus s = a->STEPInversion(p, t, &loc);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(t);
            o.push_back((double)loc);
        }
        break;
    }
    case OP_INTERSECT_LINE:
    {
        SmExtent1d ivl = read_ivl(rd, a);
        bool self = rd.flag();
        SmStatus ms = SM_SUCCESS;
        SmLine* b = self ? a : read_line(rd, ctx, &ms);
        if (b == NULL)
            throw Bad();
        SmExtent1d oivl = read_ivl(rd, b);
        double tol = rd.num();
        SmBoolean more = TRUE;
        SmSolutionArray sols;
        SmStatus s = a->IntersectWithLine(ivl, *b, oivl, tol, more, sols);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(more ? 1.0 : 0.0);
            put_sols(o, sols, a, b);
        }
        if (!self)
            delete b;
        break;
    }
    case OP_INTERSECT_ELLIPSE:
    {
        SmExtent1d ivl = read_ivl(rd, a);
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double r0 = rd.num();
        double r1 = rd.num();
        double tol = rd.num();
        SmEllipse* e = new (ctx) SmEllipse(c, x, y, SmExtent1d(d0, d1), r0, r1, 3, &ctx);
        SmBoolean more = TRUE;
        SmSolutionArray sols;
        SmStatus s = a->IntersectWithEllipse(ivl, *e, e->GetNaturalInterval(), tol, more, sols);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(more ? 1.0 : 0.0);
            put_sols(o, sols, a, e);
        }
        delete e;
        break;
    }
    case OP_JOIN:
    {
        int64_t end_this = rd.idx(0, 2);
        bool is_line = rd.flag();
        SmStatus ms = SM_SUCCESS;
        SmBSplineCurve* other = is_line ? (SmBSplineCurve*)read_line(rd, ctx, &ms) : read_bspline(rd, ctx);
        if (other == NULL)
            throw Bad();
        int64_t end_other = rd.idx(0, 2);
        bool has_tol = rd.flag();
        double tol = rd.num();
        SmStatus s = a->JoinWith((ULONG)end_this, other, (ULONG)end_other, has_tol ? &tol : NULL);
        o.push_back(st(s));
        delete other;
        break;
    }
    case OP_ADJUST:
    {
        SmExtent1d ivl = read_ivl(rd, a);
        o.push_back(st(a->AdjustSTEPInterval(ivl)));
        break;
    }
    case OP_TO_2D: o.push_back(st(a->ConvertTo2D())); break;
    case OP_TO_3D: o.push_back(st(a->ConvertTo3D())); break;
    case OP_OFFSET:
    {
        SmVector3d n = rd.vec();
        double d = rd.num();
        SmBSplineCurve* c = NULL;
        double gap = -1.0;
        SmStatus s = a->CreateSimpleOffset(ctx, 0.01, n, d, c, gap);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(gap);
            if (c == NULL || !c->IsKindOf(SmLine_TYPE))
                throw Bad();
            put_line(o, (SmLine*)c);
        }
        delete c;
        break;
    }
    case OP_REFRESH: o.push_back(st(a->RefreshAnalytics())); break;
    case OP_REVERSE:
    {
        SmExtent1d ivl = read_ivl(rd, a);
        SmExtent1d n(-5.0, 5.0);
        SmStatus s = a->ReverseParameterization(ivl, n);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_ivl(o, n);
        break;
    }
    case OP_TRANSFORM:
    {
        SmPoint3d org = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        bool has = rd.flag();
        SmVector3d sc = rd.vec();
        SmAxis2Placement frame(org, x, y);
        o.push_back(st(a->Transform(frame, has ? &sc : NULL)));
        break;
    }
    case OP_DEFORM:
    {
        bool hs = rd.flag();
        SmPoint3d ps = rd.vec();
        bool he = rd.flag();
        SmPoint3d pe = rd.vec();
        bool want = rd.flag();
        bool hst = rd.flag();
        SmVector3d ts = rd.vec();
        SmInValueType tst = (SmInValueType)rd.idx(0, 2);
        bool het = rd.flag();
        SmVector3d te = rd.vec();
        SmInValueType tet = (SmInValueType)rd.idx(0, 2);
        SmCurve* c = NULL;
        SmStatus s = a->DeformToEndPointTargets(hs ? &ps : NULL, he ? &pe : NULL, want ? &c : NULL,
                                                hst ? &ts : NULL, tst, het ? &te : NULL, tet);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(c ? 1.0 : 0.0);
            if (c)
            {
                o.push_back((double)c->GetType());
                o.push_back((double)c->GetDim());
                put_nurb(o, (SmBSplineCurve*)c);
            }
        }
        delete c;
        break;
    }
    case OP_TRIM:
    {
        SmExtent1d ivl = read_ivl(rd, a);
        int64_t mode = rd.idx(0, 2);
        SmBoolean b = mode == 0 ? TRUE : mode == 1 ? FALSE : UNSURE;
        o.push_back(st(a->Trim(ivl, b)));
        break;
    }
    case OP_EDIT_PARAM:
    {
        SmExtent1d ivl = read_ivl(rd, a);
        o.push_back(st(a->EditParameterization(ivl)));
        break;
    }
    case OP_EQ:
    {
        bool self = rd.flag();
        SmStatus ms = SM_SUCCESS;
        SmLine* b = self ? a : read_line(rd, ctx, &ms);
        if (b == NULL)
            throw Bad();
        o.push_back(0.0);
        o.push_back(*a == *b ? 1.0 : 0.0);
        if (!self)
            delete b;
        break;
    }
    case OP_COPY:
    {
        SmCurve* c = NULL;
        SmStatus s = a->Copy(ctx, c);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_line(o, (SmLine*)c);
        delete c;
        break;
    }
    case OP_ANALYTIC:
    {
        SmExtent1d ivl = read_ivl(rd, a);
        SmCurve* c = NULL;
        SmStatus s = a->CreateAnalyticCurve(ctx, ivl, c);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(c ? 1.0 : 0.0);
            if (c)
            {
                if (!c->IsKindOf(SmLine_TYPE))
                    throw Bad();
                put_line(o, (SmLine*)c);
            }
        }
        delete c;
        break;
    }
    case OP_ASSERT:
    {
        bool given = rd.flag();
        bool with_list = rd.flag();
        SmAssertArray list;
        SmBoolean ok = a->AssertValid(with_list ? &list : NULL, given ? SM_LEVEL_GIVEN : SM_LEVEL_0, SM_NO_WALK,
                                      NULL);
        o.push_back(0.0);
        o.push_back(ok ? 1.0 : 0.0);
        o.push_back((double)list.GetSize());
        for (ULONG i = 0; i < list.GetSize(); i++)
        {
            const SmAssertReport* r = list[i];
            o.push_back((double)r->GetTestIndex());
            o.push_back((double)r->GetTol3d());
            o.push_back(r->GetValue());
        }
        break;
    }
    case OP_KIND_OF:
    {
        double t = (double)rd.idx(0, 100000);
        o.push_back(0.0);
        o.push_back(a->IsKindOf((SM_TYPE)t) ? 1.0 : 0.0);
        break;
    }
    case OP_CANONICAL:
    {
        SmPoint3d p, p2;
        SmVector3d v, u;
        SmExtent1d d, d2;
        SmBoolean io = 7, io2 = 7;
        double sc = 0.0;
        a->GetCanonical(p, v, &d, &io);
        a->GetCanonical(p2, u, sc, d2, &io2);
        o.push_back(0.0);
        put_vec(o, p);
        put_vec(o, v);
        put_ivl(o, d);
        o.push_back((double)io);
        put_vec(o, p2);
        put_vec(o, u);
        o.push_back(sc);
        put_ivl(o, d2);
        o.push_back((double)io2);
        break;
    }
    case OP_SET_SCALE:
        a->SetLineScale(rd.num());
        o.push_back(0.0);
        break;
    case OP_SET_DOMAIN:
    {
        double x0 = rd.num();
        double x1 = rd.num();
        a->SetAnalyticDomain(SmExtent1d(x0, x1));
        o.push_back(0.0);
        break;
    }
    default: throw Bad();
    }
}
} // namespace

SC_EXPORT int32_t sc_curve_line_run(const double* in, int64_t n_in, double* out, int64_t cap, int64_t* out_len)
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
        SmLine* a = read_line(rd, ctx, &ms);
        o.push_back(st(ms));
        if (a != NULL)
        {
            int64_t n_ops = rd.idx(0, 64);
            for (int64_t i = 0; i < n_ops; i++)
            {
                int32_t op = (int32_t)rd.idx(1, 40);
                o.push_back((double)op);
                run_op(op, rd, ctx, a, o);
            }
            put_line(o, a);
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
