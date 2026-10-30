// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.offset_surface`.
// Wraps SmOffsetSurface (source/SMLib/src/SmOffsetSurface.cpp, source/SMLib/inc/SmOffsetSurface.h):
// the constructor, Copy, operator==, CalculateBoundingBox, GetKnots, EvaluateNormal,
// EvaluatePoint, Evaluate, EvaluateSimple, GetNaturalUVDomain, Reverse, SplitAt,
// STEPInversion, SwapUV, Transform, TrimWithDomain, CreateOffsetSurface, ReadFromDB,
// IsKindOf, AssertValid and the setters, run as a script on one offset so that the extension upstream's
// const evaluators store is seen by the following ops. WriteToDB is a generator helper.
// CreateExtendedSurface steps are refused: UB-107 and the HANG-6/HANG-22 searches inside it
// cannot be guarded in process.
//
// One entry point, sc_surface_offset_surface_eval(op, in, n_in, out, cap, out_len). Layouts
// are documented in crates/solidcore-ffi/src/units/surface_offset_surface.rs, which
// validates every input so upstream only touches valid memory. The surface-of-revolution
// block and its builder are those of surface_surf_of_revolution_p1.cpp.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAssertArray.h>
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveBoundedSurface.h>
#include <SmDatabaseIO.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmHyperbola.h>
#include <SmExtent3d.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmOffsetSurface.h>
#include <SmPlane.h>
#include <SmPolarBox.h>
#include <SmPseudoBox.h>
#include <SmSTEPSurface.h>
#include <SmSurfOfRevolution.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmTol.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <memory>
#include <cmath>
#include <stdexcept>
#include <sstream>
#include <string>
#include <vector>

#include "sc_sm.h"

using sc::put_dom;
using sc::put_ivl;
using sc::put_vec;

namespace
{
// Rejected input or state, never an SMLib numerical result: unwinds to the entry point,
// which returns SC_ERR_ARG.
struct ScOffsetUnsafeInput
{
};

// Entry ops (keep in sync with crates/solidcore-ffi/src/units/surface_offset_surface.rs).
enum : int32_t
{
    OP_SCRIPT = 0,
    OP_WRITE_DB = 1,
    OP_READ_DB = 2,
};

// Script steps.
enum : int64_t
{
    S_STATE = 0,
    S_EVAL = 1,
    S_EVAL_SIMPLE = 2,
    S_EVAL_PT = 3,
    S_NORMAL = 4,
    S_KNOTS = 5,
    S_MAX_ANAL = 6,
    S_BBOX = 7,
    S_STEP_INV = 8,
    S_REVERSE = 9,
    S_SWAP = 10,
    S_TRANSFORM = 11,
    S_TRIM = 12,
    S_SPLIT = 13,
    S_CREATE_OFFSET = 14,
    S_EXTEND = 15,
    S_EXTEND_DIR = 16,
    S_COPY = 17,
    S_EQ = 18,
    S_SET_DIST = 19,
    S_SET_ALLOW = 20,
    S_IS_PLANAR = 21,
    S_IS_KIND_OF = 22,
    S_BASE_OFFSET = 23,
    S_ASSERT_VALID = 24,
};

// Generator kinds and construction modes of a surface-of-revolution block.
enum : int32_t
{
    GK_BSPLINE = 0,
    GK_LINE = 1,
    GK_CIRCLE = 2,
    GK_HYPERBOLA = 3,
};
enum : int32_t
{
    BM_CANONICAL = 0,
    BM_CTOR = 1,
};

// Reads the protected members of SmOffsetSurface through pointers to members named in a
// derived class.
struct PeekOff : SmOffsetSurface
{
    static SmBoolean SmOffsetSurface::*allow() { return &PeekOff::m_bAllowExtension; }
    static SmBoolean SmOffsetSurface::*owns() { return &PeekOff::m_bOwnsSurface; }
    static SmBoolean SmOffsetSurface::*per_u() { return &PeekOff::m_bIsPeriodicOnU; }
    static SmBoolean SmOffsetSurface::*per_v() { return &PeekOff::m_bIsPeriodicOnV; }
};

// Reaches the protected SmSurfOfRevolution constructor.
struct PeekRev : SmSurfOfRevolution
{
    PeekRev(SmBSplineCurve* c, const SmPoint3d& o, const SmVector3d& x, const SmVector3d& y,
            const SmExtent2d& d, SmBoolean swap)
        : SmSurfOfRevolution(c, o, x, y, d, swap)
    {
    }
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
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

// Bases borrowed by an offset remain owned by its caller. Destroy the offset roots
// before this list, and the list before the context.
using BorrowedBases = std::vector<std::unique_ptr<SmSurface>>;

// The nonzero offset evaluator asks its base for a square derivative matrix; only
// zero-distance delegation reaches SmPlane's unsigned hv-k loop with caller orders.
bool delegates_to_plane(SmOffsetSurface* s, const SmPoint2d& uv, ULONG hu, ULONG hv, bool simple)
{
    double distance = s->GetOffsetDistance();
    SmSurface* base = s->GetBaseSurface();
    if (auto* nested = SM_CAST_PTR(SmOffsetSurface, base))
    {
        distance += nested->GetOffsetDistance();
        base = nested->GetBaseSurface();
    }
    if (std::max(hu, hv) > 3 || distance != 0.0) return false;
    if (!base->GetNaturalUVDomain().ContainsPoint2d(uv) && s->GetExtendedBaseSurface())
        base = s->GetExtendedBaseSurface();
    // SmPlane::EvaluateSimple inherits the same evaluator. Other base kinds have
    // their own checked derivative layouts; only this direct delegation is UB-172.
    (void)simple;
    return SM_CAST_PTR(SmPlane, base) != NULL;
}

// UB-171: with bOnlyUpperHalf, EvaluatePlainOrSimple's non-zero path writes the mixed
// derivatives at their full-layout indices, past a caller array sized (hu+1)(hv+1).
// Mirror its checks in order: offset order above 3 and an offset nested twice fail
// before anything is written, an exact 0.0 compound distance delegates to the base.
// (A base evaluation or extension failing first would also write nothing; that cannot
// be known in advance, so those inputs are refused too.)
ULONG max_write_index(ULONG hu, ULONG hv, bool upper)
{
    ULONG hod = upper ? std::max(hu, hv) : hu + hv;
    ULONG m = 0;
    auto at = [&](ULONG i) { m = std::max(m, i); };
    if (hu >= 1) at(hv + 1);
    if (hv >= 1) at(1);
    if (hu >= 2) at(2 * (hv + 1));
    if ((upper && hod >= 2) || (!upper && hu >= 1 && hv >= 1)) at(hv + 2);
    if (hv >= 2) at(2);
    if (hu >= 3) at(3 * (hv + 1));
    if ((upper && hod >= 3) || (!upper && hu >= 2 && hv >= 1)) at(2 * (hv + 1) + 1);
    if ((upper && hod >= 3) || (!upper && hu >= 1 && hv >= 2)) at((hv + 1) + 2);
    if (hv >= 3) at(3);
    return m;
}

bool writes_past_array(SmOffsetSurface* s, ULONG hu, ULONG hv, bool upper)
{
    ULONG hod = upper ? std::max(hu, hv) : hu + hv;
    if (hod > 3) return false;
    double distance = s->GetOffsetDistance();
    if (auto* nested = SM_CAST_PTR(SmOffsetSurface, s->GetBaseSurface()))
    {
        if (SM_CAST_PTR(SmOffsetSurface, nested->GetBaseSurface())) return false;
        distance += nested->GetOffsetDistance();
    }
    if (distance == 0.0) return false;
    return max_write_index(hu, hv, upper) >= (hu + 1) * (hv + 1);
}

// ---- surface-of-revolution block (as surface_surf_of_revolution_p1.cpp) ----------------

SmBSplineCurve* read_rev_bspline(Rd& rd, const SmContext& ctx)
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
    return new (ctx) SmBSplineCurve(cur, 3, FALSE);
}

// An `SmHyperbola` whose constructor made a finite NURB. The constructor ignores a
// failing `MakeNurb` (`SE`), leaving `m_pNurb` NULL or NaN for semi axes too large or too
// small next to the centre's coordinates, which every later use dereferences (the FFI
// cannot predict the rounding): such an input is refused as a harness error.
SmBSplineCurve* checked_hyperbola(SmHyperbola* h)
{
    const gw_CURVE* cur = h->GetGwNurbPointer();
    bool ok = cur != NULL && cur->pol != NULL;
    for (gw_INDEX i = 0; ok && i <= cur->pol->n; i++)
    {
        const gw_CPOINT& p = cur->pol->Pw[i];
        ok = std::isfinite(p.x) && std::isfinite(p.y) && std::isfinite(p.z) && std::isfinite(p.w);
    }
    if (!ok)
    {
        delete h;
        throw std::invalid_argument("hyperbola generator: no finite NURB");
    }
    return h;
}

SmBSplineCurve* read_gen(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    switch (kind)
    {
    case GK_BSPLINE: return read_rev_bspline(rd, ctx);
    case GK_LINE:
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine* l = new (ctx) SmLine(a, b, 3, &ctx);
        l->GetOrCreateGwNurbPointer();
        return l;
    }
    case GK_CIRCLE:
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double r = rd.num();
        return new (ctx) SmCircle(c, x, y, SmExtent1d(d0, d1), r, 3, &ctx);
    }
    case GK_HYPERBOLA:
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double a = rd.num();
        double b = rd.num();
        return checked_hyperbola(new (ctx) SmHyperbola(c, x, y, SmExtent1d(d0, d1), a, b, 3, &ctx));
    }
    default: return NULL;
    }
}

// Writes nothing; returns NULL and sets `stt` when the construction fails.
SmSurfOfRevolution* read_revolution(Rd& rd, const SmContext& ctx, SmStatus& stt)
{
    int64_t mode = rd.idx();
    std::unique_ptr<SmBSplineCurve> gen(read_gen(rd, ctx));
    bool oob = rd.flag();
    bool swap = rd.flag();
    SmSurfOfRevolution* s = NULL;
    stt = SM_SUCCESS;
    if (mode == BM_CANONICAL)
    {
        SmPoint3d p = rd.vec();
        SmVector3d a = rd.vec();
        stt = SmSurfOfRevolution::CreateCanonical(ctx, gen.get(), p, a, s, swap ? TRUE : FALSE);
        if (s) gen.release(); // ownership transfers even if MakeNurb then fails
    }
    else
    {
        SmPoint3d org = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double a = rd.num();
        double b = rd.num();
        double c = rd.num();
        double d = rd.num();
        s = new (ctx) PeekRev(gen.get(), org, x, y, SmExtent2d(a, b, c, d), swap ? TRUE : FALSE);
        gen.release();
        stt = s->MakeNurb();
    }
    if (stt != SM_SUCCESS)
    {
        delete s;
        return NULL;
    }
    s->SetOutOfBoundsEnabled(oob ? TRUE : FALSE);
    return s;
}

// ---- base and offset blocks -------------------------------------------------------------

SmOffsetSurface* read_offset(Rd& rd, const SmContext& ctx, SmStatus& stt, BorrowedBases& borrowed);

// A base block; NULL (with `stt`) when a surface of revolution fails to build.
SmSurface* read_surface(Rd& rd, const SmContext& ctx, SmStatus& stt, BorrowedBases& borrowed)
{
    int64_t kind = rd.idx();
    switch (kind)
    {
    case 0:
    {
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
    case 1:
    {
        SmPoint3d o = rd.vec();
        SmVector3d n = rd.vec();
        return new (ctx) SmPlane(o, n, &ctx);
    }
    case 2:
    {
        std::unique_ptr<SmSurface> basis(read_surface(rd, ctx, stt, borrowed));
        bool implicit = rd.flag();
        if (basis == NULL)
            return NULL;
        SmTArray<SmCurve*> c3d, cuv;
        auto* result = new (ctx) SmCurveBoundedSurface(*basis, &c3d, &cuv, implicit ? TRUE : FALSE);
        basis.release();
        return result;
    }
    case 3:
    {
        std::unique_ptr<SmSurface> base(read_surface(rd, ctx, stt, borrowed));
        if (base == NULL)
            return NULL;
        auto* result = new (ctx) SmSTEPSurface(*base, TRUE);
        base.release();
        return result;
    }
    case 4: return read_revolution(rd, ctx, stt);
    default: return read_offset(rd, ctx, stt, borrowed);
    }
}

// An offset block `d owns sing allow base`.
SmOffsetSurface* read_offset(Rd& rd, const SmContext& ctx, SmStatus& stt, BorrowedBases& borrowed)
{
    double d = rd.num();
    SmBoolean owns = rd.bflag();
    ULONG sing = (ULONG)rd.idx();
    SmBoolean allow = rd.bflag();
    std::unique_ptr<SmSurface> base(read_surface(rd, ctx, stt, borrowed));
    if (base == NULL)
        return NULL;
    std::unique_ptr<SmOffsetSurface> off(new (ctx) SmOffsetSurface(d, *base, owns, sing));
    if (owns) base.release();
    else borrowed.push_back(std::move(base));
    off->SetAllowExtension(allow);
    return off.release();
}

// ---- outputs ------------------------------------------------------------------------------

// A 3x3 EvaluatePoint grid over the natural domain: each status and point.
void put_grid(std::vector<double>& o, const SmSurface* s)
{
    SmExtent2d d = s->GetNaturalUVDomain();
    for (int i = 0; i < 3; i++)
        for (int j = 0; j < 3; j++)
        {
            SmPoint2d uv = d.Evaluate(i / 2.0, j / 2.0);
            SmPoint3d p;
            SmStatus stt = s->EvaluatePoint(uv, p);
            o.push_back(st(stt));
            if (stt == SM_SUCCESS)
                put_vec(o, p);
        }
}

void put_state(std::vector<double>& o, SmOffsetSurface* s)
{
    put_dom(o, s->GetNaturalUVDomain());
    o.push_back(s->GetOffsetDistance());
    o.push_back(flg(s->*PeekOff::allow()));
    o.push_back(flg(s->*PeekOff::owns()));
    o.push_back((double)(s->*PeekOff::per_u()));
    o.push_back((double)(s->*PeekOff::per_v()));
    o.push_back((double)s->GetSingularities());
    SmSurface* e = s->GetExtendedBaseSurface();
    o.push_back(e ? 1.0 : 0.0);
    if (e)
    {
        put_dom(o, e->GetNaturalUVDomain());
        o.push_back((double)e->GetType());
    }
    SmSurface* b = s->GetBaseSurface();
    o.push_back((double)b->GetType());
    put_dom(o, b->GetNaturalUVDomain());
    o.push_back((double)s->GetType());
    put_grid(o, s);
}

// A surface an op returns: its type, then the offset's state, or (another kind) its
// natural domain and point grid.
void put_result(std::vector<double>& o, SmSurface* r)
{
    o.push_back((double)r->GetType());
    SmOffsetSurface* off = SM_CAST_PTR(SmOffsetSurface, r);
    if (off)
    {
        put_state(o, off);
    }
    else
    {
        put_dom(o, r->GetNaturalUVDomain());
        put_grid(o, r);
    }
}

// The entries upstream writes: all, or with bOnlyUpperHalf those with i + j up to the
// larger order (the others are untouched caller memory).
void put_matrix(std::vector<double>& o, const std::vector<SmVector3d>& a, ULONG hu, ULONG hv, bool upper)
{
    ULONG hod = hu > hv ? hu : hv;
    for (ULONG i = 0; i <= hu; i++)
        for (ULONG j = 0; j <= hv; j++)
            if (!upper || i + j <= hod)
                put_vec(o, a[i * (hv + 1) + j]);
}

// Follows SmOffsetSurface::CalculateBoundingBox over `d` down the chain on the actual
// surfaces after the preceding ops: each level tests the domain against its own
// m_pSurface and passes the call on, or runs the SmSurface body on itself, whose offset
// surface cache can recurse (UB-173) or subdivide without progress (HANG-21). Whether
// the box reaches that body at any level (refused).
bool box_reaches_surface_body(SmOffsetSurface* s, const SmExtent2d& d)
{
    for (SmOffsetSurface* level = s; level != NULL;
         level = SM_CAST_PTR(SmOffsetSurface, level->GetBaseSurface()))
    {
        if (!d.IsContainedBy(level->GetBaseSurface()->GetNaturalUVDomain(), SM_EFF_ZERO))
            return true;
    }
    return false;
}

// A report text as `len chars...`.
void av_text(std::vector<double>& o, const TCHAR* text)
{
    std::vector<double> t;
    for (const TCHAR* p = text; p && *p; p++)
        t.push_back((double)*p);
    o.push_back((double)t.size());
    o.insert(o.end(), t.begin(), t.end());
}

// A report object: 0 null, 1 the checked offset, 9 another.
double av_id(const void* p, const SmSurface* s)
{
    if (p == NULL)
        return 0.0;
    return p == s ? 1.0 : 9.0;
}

// The inherited SmSurface::AssertValid (levels 0..2) ignores two statuses and reads
// what the failed call left (undefined): sm_AssertTestSurface6 the iso curves of a
// failed CreateIsoBoundaries, sm_AssertTestSurface4 the continuity minima of a
// CalculateContinuities that failed before writing them. Whether `s` reaches either.
bool base_check_reads_unwritten(const SmContext& ctx, SmSurface* s)
{
    SmApproxTol3d tol = SmTol::GetApproxTol3d(s);
    for (int ii = 0; ii < 2; ii++)
    {
        SmSurfParamType dir = ii == 1 ? SM_SP_V : SM_SP_U;
        if (s->IsClosed(s->GetNaturalUVDomain(), dir))
            continue;
        SmBSplineCurve *c1 = NULL, *c2 = NULL;
        SmStatus st = s->CreateIsoBoundaries(ctx, dir, tol, c1, c2);
        bool good = st == SM_SUCCESS && c1 != NULL && c2 != NULL;
        delete c1;
        delete c2;
        if (!good)
            return true;
    }
    SmContinuityType min_u = SM_CT_G1, min_v = SM_CT_G1;
    SmTArray<SmContinuityType> cu(ctx), cv(ctx);
    SmStatus su = s->CalculateContinuities(SM_SP_U, min_u, cu);
    SmStatus sv = s->CalculateContinuities(SM_SP_V, min_v, cv);
    return (su != SM_SUCCESS && cu.GetSize() == 0) || (sv != SM_SUCCESS && cv.GetSize() == 0);
}

// ---- the script -----------------------------------------------------------------------------

void run_step(int64_t step, Rd& rd, const SmContext& ctx, SmOffsetSurface* s, std::vector<double>& o)
{
    switch (step)
    {
    case S_STATE:
        o.push_back(0.0);
        put_state(o, s);
        break;
    case S_EVAL:
    case S_EVAL_SIMPLE:
    {
        SmPoint2d uv = rd.uv();
        ULONG hu = (ULONG)rd.idx();
        ULONG hv = (ULONG)rd.idx();
        SmBoolean ul = rd.bflag();
        SmBoolean vl = rd.bflag();
        SmBoolean upper = rd.bflag();
        if (upper && hu > hv && delegates_to_plane(s, uv, hu, hv, step == S_EVAL_SIMPLE)) throw ScOffsetUnsafeInput{};
        if (writes_past_array(s, hu, hv, upper)) throw ScOffsetUnsafeInput{};
        std::vector<SmVector3d> a((hu + 1) * (hv + 1)); // SM_UNDEF_DOUBLE entries
        SmStatus stt;
        if (step == S_EVAL)
        {
            SmBoolean nzt = rd.bflag();
            stt = s->Evaluate(uv, hu, hv, ul, vl, upper, a.data(), nzt);
        }
        else
        {
            stt = s->EvaluateSimple(uv, hu, hv, ul, vl, upper, a.data());
        }
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_matrix(o, a, hu, hv, upper != FALSE);
        break;
    }
    case S_EVAL_PT:
    {
        SmPoint2d uv = rd.uv();
        SmPoint3d p;
        SmStatus stt = s->EvaluatePoint(uv, p);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_vec(o, p);
        break;
    }
    case S_NORMAL:
    {
        SmPoint2d uv = rd.uv();
        SmBoolean ul = rd.bflag();
        SmBoolean vl = rd.bflag();
        SmVector3d n;
        SmStatus stt = s->EvaluateNormal(uv, ul, vl, n);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_vec(o, n);
        break;
    }
    case S_KNOTS:
    {
        SmSurfParamType param = (SmSurfParamType)rd.idx();
        bool has_ivl = rd.flag();
        double a = rd.num();
        double b = rd.num();
        SmExtent1d ivl(a, b);
        SmTArray<double> knots;
        SmTArray<ULONG> mults;
        SmStatus stt = s->GetKnots(param, knots, &mults, has_ivl ? &ivl : NULL);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back((double)knots.GetSize());
            for (ULONG i = 0; i < knots.GetSize(); i++)
                o.push_back(knots[i]);
            o.push_back((double)mults.GetSize());
            for (ULONG i = 0; i < mults.GetSize(); i++)
                o.push_back((double)mults[i]);
        }
        break;
    }
    case S_MAX_ANAL:
        o.push_back(0.0);
        put_dom(o, s->GetMaxAnalyticDomain());
        break;
    case S_BBOX:
    {
        SmExtent2d d = rd.dom();
        bool w0 = rd.flag();
        bool w1 = rd.flag();
        bool w2 = rd.flag();
        SmBoolean expand = rd.bflag();
        if (box_reaches_surface_body(s, d)) throw ScOffsetUnsafeInput{};
        SmExtent3d nb;
        SmPseudoBox pb;
        SmPolarBox qb;
        SmStatus stt = s->CalculateBoundingBox(d, w0 ? &nb : NULL, w1 ? &pb : NULL, w2 ? &qb : NULL, NULL,
                                               NULL, expand);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            if (w0)
            {
                put_vec(o, nb.GetMin());
                put_vec(o, nb.GetMax());
            }
            if (w1)
            {
                for (ULONG i = 0; i < 3; i++)
                    put_vec(o, pb.GetBasis(i));
                for (ULONG i = 0; i < 3; i++)
                    put_ivl(o, pb.GetInterval(i));
            }
            if (w2)
            {
                o.push_back((double)qb.GetPolarBoxState());
                for (ULONG i = 0; i < 3; i++)
                    put_vec(o, *qb.GetBasis(i));
                put_dom(o, qb.GetDomain());
            }
        }
        break;
    }
    case S_STEP_INV:
    {
        SmExtent2d d = rd.dom();
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        bool has_guess = rd.flag();
        SmPoint2d g = rd.uv();
        SmPoint2d uv(0.0, 0.0);
        SmLocationType loc = SM_LT_INTERIOR;
        SmStatus stt = s->STEPInversion(d, p, tol, uv, loc, has_guess ? &g : NULL);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(uv.x);
            o.push_back(uv.y);
            o.push_back((double)loc);
        }
        break;
    }
    case S_REVERSE:
        o.push_back(st(s->Reverse((SmSurfParamType)rd.idx())));
        break;
    case S_SWAP:
        o.push_back(st(s->SwapUV()));
        break;
    case S_TRANSFORM:
    {
        SmPoint3d org = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        bool has_scale = rd.flag();
        SmVector3d scale = rd.vec();
        SmAxis2Placement ax(org, x, y);
        o.push_back(st(s->Transform(ax, has_scale ? &scale : NULL)));
        break;
    }
    case S_TRIM:
    {
        SmExtent2d d = rd.dom();
        SmStatus stt = s->TrimWithDomain(d);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_dom(o, d);
        break;
    }
    case S_SPLIT:
    {
        double t = rd.num();
        SmSurfParamType param = (SmSurfParamType)rd.idx();
        SmSurface* l = NULL;
        SmSurface* r = NULL;
        SmStatus stt = s->SplitAt(ctx, t, param, l, r);
        std::unique_ptr<SmSurface> left(l), right(r);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            put_result(o, l);
            put_result(o, r);

        }
        break;
    }
    case S_EXTEND:
    case S_EXTEND_DIR:
        // CreateExtendedSurface reaches UB-107 and the HANG-6/HANG-22 searches, which cannot
        // be guarded in process: refused (the FFI refuses these steps before calling).
        throw ScOffsetUnsafeInput{};
    case S_CREATE_OFFSET:
    case S_COPY:
    {
        SmSurface* r = NULL;
        SmStatus stt;
        if (step == S_CREATE_OFFSET)
        {
            double d = rd.num();
            stt = s->CreateOffsetSurface(ctx, d, 0.0, r);
        }
        else
        {
            stt = s->Copy(ctx, r);
        }
        std::unique_ptr<SmSurface> result(r);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS && r)
        {
            put_result(o, r);
        }
        break;
    }
    case S_EQ:
    {
        SmStatus bst = SM_SUCCESS;
        BorrowedBases borrowed;
        std::unique_ptr<SmOffsetSurface> other(read_offset(rd, ctx, bst, borrowed));
        o.push_back(st(bst));
        if (other)
        {
            o.push_back(flg(*s == *other));
            o.push_back(flg(*s == *s));

        }
        break;
    }
    case S_SET_DIST:
        s->SetOffsetDistance(rd.num());
        o.push_back(0.0);
        break;
    case S_SET_ALLOW:
        s->SetAllowExtension(rd.bflag());
        o.push_back(0.0);
        break;
    case S_IS_PLANAR:
    {
        double tol = rd.num();
        // the SmSurface body boxes the natural domain through the virtual (UB-173): an
        // inner offset's extension, made by the outer's STEPInversion, widens it
        if (box_reaches_surface_body(s, s->GetNaturalUVDomain())) throw ScOffsetUnsafeInput{};
        SmPoint3d p(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        SmVector3d n(SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE, SM_UNDEF_DOUBLE);
        double dev = SM_UNDEF_DOUBLE;
        SmBoolean r = s->IsPlanar(tol, &p, &n, &dev);
        o.push_back(0.0);
        o.push_back(flg(r));
        put_vec(o, p);
        put_vec(o, n);
        o.push_back(dev);
        break;
    }
    case S_IS_KIND_OF:
        o.push_back(0.0);
        o.push_back(flg(s->IsKindOf((SM_TYPE)rd.idx())));
        break;
    case S_BASE_OFFSET:
    {
        // the virtual through SmSurface*: SmPlane, SmSurfOfRevolution and SmOffsetSurface
        // override it; SmBSplineSurface's forwards to the SmSurface body
        double d = rd.num();
        double tol = rd.num();
        SmSurface* r = NULL;
        SmStatus stt = s->GetBaseSurface()->CreateOffsetSurface(ctx, d, tol, r);
        std::unique_ptr<SmSurface> result(r);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS && r) put_result(o, r);
        break;
    }
    case S_ASSERT_VALID:
    {
        int64_t level = rd.idx();
        // the self-intersection test boxes the natural domain, as IsPlanar (UB-173)
        if (box_reaches_surface_body(s, s->GetNaturalUVDomain())) throw ScOffsetUnsafeInput{};
        if (level != SM_LEVEL_GIVEN && base_check_reads_unwritten(ctx, s)) throw ScOffsetUnsafeInput{};
        SmAssertArray reports;
        SmSurface* srf = s;
        SmBoolean ok = srf->AssertValid(&reports, (SmAssertTestLevel)level, SM_NO_WALK);
        o.push_back(0.0);
        o.push_back(ok ? 1.0 : 0.0);
        o.push_back((double)reports.GetSize());
        for (ULONG i = 0; i < reports.GetSize(); i++)
        {
            SmAssertReport* r = reports[i];
            o.insert(o.end(), {(double)r->m_lReportingType, (double)r->m_lListIndex,
                               (double)r->m_lTestIndex, (double)r->m_eAssertType, r->m_bOK ? 1.0 : 0.0});
            o.push_back(av_id(r->m_pReporter, s));
            o.push_back(av_id(r->m_pOwner, s));
            o.push_back(av_id(r->m_pOther, s));
            o.insert(o.end(), {(double)r->m_lOwnerType, (double)r->m_lOtherType, (double)r->m_sTol3d,
                               r->m_dValue, r->m_dDist3d});
            av_text(o, r->m_pName);
            av_text(o, r->m_pMessage);
            av_text(o, r->m_pMessageForFile);
            av_text(o, r->m_sOwnerTypeString);
            av_text(o, r->m_sOtherTypeString);
        }
        o.push_back(reports.GetLastAdd() ? 1.0 : 0.0);
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}

void run_script(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmStatus bst = SM_SUCCESS;
    BorrowedBases borrowed;
    std::unique_ptr<SmOffsetSurface> s(read_offset(rd, ctx, bst, borrowed));
    o.push_back(st(bst));
    int64_t n_steps = rd.idx();
    if (s == NULL)
    {
        // skip the steps (the record is the construction status only)
        rd.at = -1;
        return;
    }
    for (int64_t i = 0; i < n_steps; i++)
        run_step(rd.idx(), rd, ctx, s.get(), o);
}

void write_db(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    SmStatus bst = SM_SUCCESS;
    BorrowedBases borrowed;
    std::unique_ptr<SmOffsetSurface> s(read_offset(rd, ctx, bst, borrowed));
    bool binary = rd.flag();
    ULONG version = (ULONG)rd.idx();
    o.push_back(st(bst));
    if (s == NULL)
        return;
    std::ostringstream oss;
    oss.precision(17);
    SmDatabaseIOStream db(binary ? SM_BINARY : SM_ASCII, &oss);
    SmStatus stt = s->WriteToDB(db, version);
    o.push_back(st(stt));
    std::string bytes = oss.str();
    for (char ch : bytes)
        o.push_back((double)(unsigned char)ch);
}

void read_db(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    bool binary = rd.flag();
    ULONG version = (ULONG)rd.idx();
    int64_t nb = rd.idx();
    std::string bytes;
    for (int64_t i = 0; i < nb; i++)
        bytes.push_back((char)(unsigned char)rd.idx());
    std::istringstream iss(bytes);
    SmDatabaseIOStream db(binary ? SM_BINARY : SM_ASCII, &iss);
    SmSurface* s = NULL;
    SmStatus stt = SmOffsetSurface::ReadFromDB(SmOffsetSurface_TYPE, db, ctx, s, version);
    o.push_back(st(stt));
    if (stt == SM_SUCCESS && s)
        put_state(o, (SmOffsetSurface*)s);
    delete s;
}
} // namespace

SC_EXPORT int32_t sc_surface_offset_surface_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                                 int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL)
        return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    Rd rd{in, 0};
    std::vector<double> o;
    switch (op)
    {
    case OP_SCRIPT: run_script(rd, ctx, o); break;
    case OP_WRITE_DB: write_db(rd, ctx, o); break;
    case OP_READ_DB: read_db(rd, ctx, o); break;
    default: *out_len = 0; return SC_ERR_ARG;
    }
    // (a failed construction skips the rest of the input)
    if (rd.at != n_in && rd.at != -1)
    {
        *out_len = 0;
        return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    catch (const ScOffsetUnsafeInput&)
    {
        *out_len = 0;
        return SC_ERR_ARG;
    }
    catch (...)
    {
        *out_len = 0;
        return SC_ERR_EXCEPTION;
    }
}
