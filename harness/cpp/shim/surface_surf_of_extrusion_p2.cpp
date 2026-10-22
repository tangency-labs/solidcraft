// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surf_of_extrusion_p2`.
// Wraps the SmSurfOfExtrusion members owned by the unit (source/SMLib/src/SmSurfOfExtrusion.cpp):
// GlobalSurfaceIntersect, IntersectWithPlane, IntersectWithCylinder,
// IntersectWithSurfOfExtrusion, IsNurbSurfaceSurfOfExtrusion, TrimWithDomain and ReadFromDB.
//
// One entry point, sc_surface_surf_of_extrusion_p2_eval(op, in, n_in, bytes, n_bytes, out, cap,
// out_len). Layouts are documented in crates/solidcore-ffi/src/units/surface_surf_of_extrusion_p2.rs,
// which validates every input so upstream only touches valid memory. `out` starts with the status
// of the surface construction (or database read), then the op's status and outputs.
//
// The surface blocks of a built surface are those of the `surface.surf_of_extrusion_p1` shim
// (its readers and the state layout are copied below).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmCircle.h>
#include <SmCone.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCylinder.h>
#include <SmDatabaseIO.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmGeomUtility.h>
#include <SmHyperbola.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmParabola.h>
#include <SmPlane.h>
#include <SmSurfOfExtrusion.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <cmath>
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
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_surf_of_extrusion_p2.rs).
enum : int32_t
{
    OP_STATE = 0,
    OP_PLANE = 1,
    OP_CYLINDER = 2,
    OP_EXTRUSION = 3,
    OP_GLOBAL = 4,
    OP_IS_NURB = 5,
    OP_TRIM = 6,
};

// Generator kinds.
enum : int32_t
{
    GK_BSPLINE = 0,
    GK_LINE = 1,
    GK_CIRCLE = 2,
    GK_ELLIPSE = 3,
    GK_HYPERBOLA = 4,
    GK_PARABOLA = 5,
    GK_LAZY_LINE = 6,
};

// Construction modes of a built surface.
enum : int32_t
{
    BM_CANONICAL = 0,
    BM_CTOR = 1,
    BM_INFINITE = 2,
};

// Surface sources.
enum : int32_t
{
    SRC_BUILT = 0,  // a p1 surface block
    SRC_RECORD = 1, // ReadFromDB of bytes[start .. start + len]
};

// Plane kinds.
enum : int32_t
{
    PK_BOUNDED = 0, // SmPlane(O, X, Y, scale, domain)
    PK_NORMAL = 1,  // SmPlane(O, N)
};

// Cone kinds.
enum : int32_t
{
    CK_CANONICAL = 0,      // SmCone::CreateCanonical(O, bot, top, height, swap, inside)
    CK_CANONICAL_SEMI = 1, // SmCone::CreateCanonical(O, bot, semi, pOptHeight, swap, inside)
    CK_CYLINDER = 2,       // SmCylinder::CreateCanonical(O, radius)
};

// GlobalSurfaceIntersect targets.
enum : int32_t
{
    TG_PLANE = 0,
    TG_CONE = 1,
    TG_EXTRUSION = 2,
    TG_BSPLINE = 3, // the NURB of a surface source, as an SmBSplineSurface
};

// A GlobalSurfaceIntersect whose special case needs more (or that has none) runs the
// general intersector (SmAdvSurfaceIntersector); the Rust side defers those cases (a
// surface of extrusion is not a `Surface` kind there yet, so the general intersector
// cannot take it).

struct Peek : SmSurfOfExtrusion
{
    static SmCurveOrientType SmSurfOfExtrusion::*orient() { return &Peek::m_eOrientation; }
    static SmVector3d SmSurfOfExtrusion::*cpoint() { return &Peek::m_vCurvePoint; }
    static SmVector3d SmSurfOfExtrusion::*cvec() { return &Peek::m_vCurveVec; }
    Peek(SmBSplineCurve* c, const SmPoint3d& o, const SmVector3d& v, const SmExtent2d& d,
         SmBoolean swap)
        : SmSurfOfExtrusion(c, o, v, d, swap)
    {
    }
    Peek(SmBSplineCurve* c, const SmVector3d& v, SmBoolean swap) : SmSurfOfExtrusion(c, v, swap) {}
};

struct ConePeek : SmCone
{
    static SmPolarConversion SmSurfOfRevolution::*polar() { return &ConePeek::m_vPolarConverter; }
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
    SmExtent2d domain()
    {
        double a = num();
        double b = num();
        double c = num();
        double d = num();
        return SmExtent2d(a, b, c, d);
    }
};

SmBSplineCurve* read_bspline(Rd& rd, const SmContext& ctx)
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

SmBSplineCurve* read_gen(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    switch (kind)
    {
    case GK_BSPLINE: return read_bspline(rd, ctx);
    case GK_LINE:
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        SmLine* l = new (ctx) SmLine(a, b, 3, &ctx);
        l->GetOrCreateGwNurbPointer();
        return l;
    }
    case GK_LAZY_LINE:
    {
        SmPoint3d a = rd.vec();
        SmPoint3d b = rd.vec();
        return new (ctx) SmLine(a, b, 3, &ctx);
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
    case GK_ELLIPSE:
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double rx = rd.num();
        double ry = rd.num();
        return new (ctx) SmEllipse(c, x, y, SmExtent1d(d0, d1), rx, ry, 3, &ctx);
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
        return new (ctx) SmHyperbola(c, x, y, SmExtent1d(d0, d1), a, b, 3, &ctx);
    }
    case GK_PARABOLA:
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double f = rd.num();
        return new (ctx) SmParabola(c, x, y, SmExtent1d(d0, d1), f, 3, &ctx);
    }
    default: return NULL;
    }
}

// A p1 surface block `mode gen oob swap ...`; NULL with *status set on failure.
SmSurfOfExtrusion* read_built(Rd& rd, const SmContext& ctx, SmStatus* status)
{
    int64_t mode = rd.idx();
    SmBSplineCurve* gen = read_gen(rd, ctx);
    bool oob = rd.flag();
    bool swap = rd.flag();
    SmSurfOfExtrusion* s = NULL;
    SmStatus stt = SM_SUCCESS;
    if (mode == BM_CANONICAL)
    {
        SmVector3d e = rd.vec();
        stt = SmSurfOfExtrusion::CreateCanonical(ctx, gen, e, s);
    }
    else if (mode == BM_CTOR)
    {
        SmPoint3d org = rd.vec();
        SmVector3d e = rd.vec();
        SmExtent2d d = rd.domain();
        s = new (ctx) Peek(gen, org, e, d, swap ? TRUE : FALSE);
        stt = s->MakeNurb();
    }
    else
    {
        SmVector3d e = rd.vec();
        s = new (ctx) Peek(gen, e, swap ? TRUE : FALSE);
        stt = s->MakeNurb();
    }
    *status = stt;
    if (stt != SM_SUCCESS)
    {
        if (s)
            delete s; // owns gen
        else
            delete gen;
        return NULL;
    }
    s->SetOutOfBoundsEnabled(oob ? TRUE : FALSE); // the surface and its generator
    return s;
}

// A record block `ascii version start len`: SmSurfOfExtrusion::ReadFromDB of those bytes.
// A generator that is not an SmBSplineCurve (upstream only asserts, then casts it) is
// rejected with SC_ERR_ARG.
SmSurfOfExtrusion* read_record(Rd& rd, const SmContext& ctx, const uint8_t* bytes,
                               SmStatus* status)
{
    bool ascii = rd.flag();
    ULONG version = (ULONG)rd.idx();
    int64_t start = rd.idx();
    int64_t len = rd.idx();
    std::istringstream iss(std::string((const char*)bytes + start, (size_t)len));
    SmDatabaseIOStream db(ascii ? SM_ASCII : SM_BINARY, &iss);
    db.SetVersionNum(version);
    SmSurface* srf = NULL;
    SmStatus stt =
        SmSurfOfExtrusion::ReadFromDB(SmSurfOfExtrusion_TYPE, db, ctx, srf, version);
    *status = stt;
    if (stt != SM_SUCCESS)
    {
        delete srf;
        return NULL;
    }
    SmSurfOfExtrusion* s = (SmSurfOfExtrusion*)srf;
    if (!s->GetGenCurve() || !s->GetGenCurve()->IsKindOf(SmBSplineCurve_TYPE))
    {
        delete s;
        *status = (SmStatus)SC_ERR_ARG;
        return NULL;
    }
    return s;
}

SmSurfOfExtrusion* read_src(Rd& rd, const SmContext& ctx, const uint8_t* bytes,
                            SmStatus* status)
{
    int64_t kind = rd.idx();
    if (kind == SRC_BUILT)
        return read_built(rd, ctx, status);
    return read_record(rd, ctx, bytes, status);
}

// `has [type step0 step1 has_nurb [p n cpts(4 n) nk knots]]`.
void put_curve(std::vector<double>& o, const SmCurve* c)
{
    o.push_back(flg(c != NULL));
    if (!c)
        return;
    o.push_back((double)c->GetType());
    SmExtent1d ivl = c->GetSTEPInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    const SmBSplineCurve* b =
        c->IsKindOf(SmBSplineCurve_TYPE) ? (const SmBSplineCurve*)c : NULL;
    const gw_CURVE* cur = b ? b->GetGwNurbPointer() : NULL;
    o.push_back(flg(cur != NULL));
    if (!cur)
        return;
    o.push_back((double)cur->p);
    o.push_back((double)(cur->pol->n + 1));
    for (gw_INDEX i = 0; i <= cur->pol->n; i++)
    {
        o.push_back(cur->pol->Pw[i].x);
        o.push_back(cur->pol->Pw[i].y);
        o.push_back(cur->pol->Pw[i].z);
        o.push_back(cur->pol->Pw[i].w);
    }
    o.push_back((double)(cur->knt->m + 1));
    for (gw_INDEX i = 0; i <= cur->knt->m; i++)
        o.push_back(cur->knt->U[i]);
}

// The p1 surface state.
void put_state(std::vector<double>& o, SmSurfOfExtrusion* s)
{
    o.push_back(flg(s->IsKindOf(SmSurfOfExtrusion_TYPE)));
    o.push_back(flg(s->IsKindOf(SmBSplineSurface_TYPE)));
    o.push_back(flg(s->IsKindOf(SmSurface_TYPE)));
    o.push_back((double)s->GetType());
    o.push_back(flg(s->GetSwapUVBit()));
    o.push_back(flg(s->IsOutOfBoundsEnabled()));
    o.push_back((double)(s->*Peek::orient()));
    put_vec(o, s->GetOrigin());
    put_vec(o, s->GetExtrusionVector());
    put_vec(o, s->*Peek::cpoint());
    put_vec(o, s->*Peek::cvec());
    put_dom(o, s->GetSTEPUVDomain());
    SmSurfParamType lp = SM_SP_UNKNOWN;
    SmExtent1d ld = s->GetSTEPLinearParamExtent(&lp);
    o.push_back(ld.GetMin());
    o.push_back(ld.GetMax());
    o.push_back((double)lp);
    SmSurfParamType gp = SM_SP_UNKNOWN;
    SmExtent1d gd = s->GetGenDirParamExtent(&gp);
    o.push_back(gd.GetMin());
    o.push_back(gd.GetMax());
    o.push_back((double)gp);
    put_dom(o, s->GetMaxAnalyticDomain());
    SmBSplineCurve* gc = NULL;
    SmVector3d ev(0.0, 0.0, 0.0);
    s->GetCanonical(gc, ev);
    o.push_back(flg(gc == s->GetGenCurve()));
    put_vec(o, ev);
    put_curve(o, s->GetGenCurve());
    const gw_SURFACE* sur = s->GetGwNurbPointer();
    o.push_back(flg(sur != NULL));
    if (!sur)
        return;
    o.push_back((double)sur->p);
    o.push_back((double)sur->q);
    o.push_back((double)(sur->net->n + 1));
    o.push_back((double)(sur->net->m + 1));
    for (gw_INDEX i = 0; i <= sur->net->n; i++)
        for (gw_INDEX j = 0; j <= sur->net->m; j++)
        {
            o.push_back(sur->net->Pw[i][j].x);
            o.push_back(sur->net->Pw[i][j].y);
            o.push_back(sur->net->Pw[i][j].z);
            o.push_back(sur->net->Pw[i][j].w);
        }
    o.push_back((double)(sur->knu->m + 1));
    for (gw_INDEX i = 0; i <= sur->knu->m; i++)
        o.push_back(sur->knu->U[i]);
    o.push_back((double)(sur->knv->m + 1));
    for (gw_INDEX i = 0; i <= sur->knv->m; i++)
        o.push_back(sur->knv->U[i]);
}

// A plane block `kind O[3] (X[3] Y[3] scale[2] D[4] | N[3])`.
SmPlane* read_plane(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    SmPoint3d o = rd.vec();
    if (kind == PK_BOUNDED)
    {
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double su = rd.num();
        double sv = rd.num();
        SmExtent2d d = rd.domain();
        return new (ctx) SmPlane(o, x, y, SmVector2d(su, sv), d, &ctx);
    }
    SmVector3d nv = rd.vec();
    return new (ctx) SmPlane(o, nv, &ctx);
}

// A cone block `kind O[3] X[3] Y[3] p[6]`; NULL with *status set on failure.
SmCone* read_cone(Rd& rd, const SmContext& ctx, SmStatus* status)
{
    int64_t kind = rd.idx();
    SmPoint3d org = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    double p[6];
    for (int i = 0; i < 6; i++)
        p[i] = rd.num();
    SmAxis2Placement a2p(org, x, y);
    SmCone* c = NULL;
    SmStatus stt = SM_SUCCESS;
    switch (kind)
    {
    case CK_CANONICAL:
        stt = SmCone::CreateCanonical(ctx, a2p, p[0], p[1], p[2], c, p[3] != 0.0 ? TRUE : FALSE,
                                      p[4] != 0.0 ? TRUE : FALSE);
        break;
    case CK_CANONICAL_SEMI:
    {
        double h = p[3];
        stt = SmCone::CreateCanonical(ctx, a2p, p[0], p[1], c, p[2] != 0.0 ? &h : NULL,
                                      p[4] != 0.0 ? TRUE : FALSE, p[5] != 0.0 ? TRUE : FALSE);
        break;
    }
    default:
    {
        SmCylinder* cy = NULL;
        stt = SmCylinder::CreateCanonical(ctx, a2p, p[0], cy);
        c = cy;
        break;
    }
    }
    *status = stt;
    if (stt != SM_SUCCESS)
    {
        delete c;
        return NULL;
    }
    return c;
}

// Whether CreateIsoCircleFromPoint(point, tol) of the cone would read uninitialised values
// (as the `surface.cone_p1` shim tests it): the polar converter's curve is dereferenced
// unchecked, and a STEP inversion or NURB conversion that leaves its outputs unset is read.
bool iso_circle_is_undefined(SmCone* s, const SmPoint3d& p, double tol)
{
    const SmPolarConversion& pc = s->*ConePeek::polar();
    if (pc.GetCurve() == NULL)
        return true;
    const SmPoint3d& rOrigin = s->GetPosition().GetOriginRef();
    SmVector3d sZAxis = s->GetPosition().GetZAxis();
    SmExtent1d sHeightStepIvl = s->GetSTEPUVDomain().GetVInterval();
    double dParam = 0.0;
    smgu_LineClosestPoint(rOrigin, sZAxis, p, dParam);
    if (!sHeightStepIvl.ContainsValue(dParam, tol))
        return false;
    double dRadius = s->GetRadius(dParam);
    SmPoint3d sCenter = rOrigin + dParam * sZAxis;
    SmVector3d sVec = p - sCenter;
    if (sVec.LengthSquared() < 2 * SM_EFF_ZERO_SQ)
        sVec = s->GetPosition().GetXAxis();
    sVec.Unitize();
    SmPoint3d sPointOnSurface = sCenter + dRadius * sVec;
    const double nan = std::nan("");
    SmPoint2d suv(nan, nan);
    SmLocationType eloc = SM_LT_EXTERIOR;
    s->STEPInversion(s->GetSTEPUVDomain(), sPointOnSurface, tol, suv, eloc);
    if (std::isnan(suv.x) || std::isnan(suv.y))
        return true;
    SmPoint2d nuv(nan, nan);
    return s->ConvertUVFromSTEPToNURBS(suv, nuv) != SM_SUCCESS &&
           (!s->GetSwapUV() || std::isnan(nuv.x));
}

// The intersection outputs: `more ncurves curves ntypes types nuv1 has... nuv2 has... ndev devs`.
void put_tsect(std::vector<double>& o, SmBoolean more, SmTArray<SmCurve*>& curves,
               SmTArray<SmCurve*>& uv1, SmTArray<SmCurve*>& uv2,
               SmTArray<SmTsectCurveType>& types, SmTArray<double>& devs)
{
    o.push_back(flg(more != FALSE));
    o.push_back((double)curves.GetSize());
    for (ULONG i = 0; i < curves.GetSize(); i++)
        put_curve(o, curves[i]);
    o.push_back((double)types.GetSize());
    for (ULONG i = 0; i < types.GetSize(); i++)
        o.push_back((double)types[i]);
    o.push_back((double)uv1.GetSize());
    for (ULONG i = 0; i < uv1.GetSize(); i++)
        o.push_back(flg(uv1[i] != NULL));
    o.push_back((double)uv2.GetSize());
    for (ULONG i = 0; i < uv2.GetSize(); i++)
        o.push_back(flg(uv2[i] != NULL));
    o.push_back((double)devs.GetSize());
    for (ULONG i = 0; i < devs.GetSize(); i++)
        o.push_back(devs[i]);
}

void free_curves(SmTArray<SmCurve*>& a)
{
    for (ULONG i = 0; i < a.GetSize(); i++)
        delete a[i];
    a.ReSet();
}

struct Tsect
{
    SmTArray<SmCurve*> curves, uv1, uv2;
    SmTArray<SmTsectCurveType> types;
    SmTArray<double> devs;
    SmBoolean more = FALSE;
    ~Tsect()
    {
        free_curves(curves);
        free_curves(uv1);
        free_curves(uv2);
    }
};

// `has_tol tol` -> an optional SmApproxTol3d.
struct OptTol
{
    bool has;
    SmApproxTol3d tol;
    explicit OptTol(Rd& rd) : has(rd.flag()), tol(rd.num()) {}
    const SmApproxTol3d* ptr() const { return has ? &tol : NULL; }
};

// The test surface of IsNurbSurfaceSurfOfExtrusion: `raw` then either an inline NURB `n m p q
// cpts(4, v fastest) U V` or the NURB of a surface source; then `transpose`, `reparam D[4]`,
// `perturb i j d[3]` edits (flag-prefixed).
SmBSplineSurface* read_test_surface(Rd& rd, const SmContext& ctx, const uint8_t* bytes,
                                    std::vector<double>& o, bool* guard)
{
    bool raw = rd.flag();
    SmBSplineSurface* t = NULL;
    if (raw)
    {
        gw_INDEX n = (gw_INDEX)rd.idx();
        gw_INDEX m = (gw_INDEX)rd.idx();
        gw_DEGREE p = (gw_DEGREE)rd.idx();
        gw_DEGREE q = (gw_DEGREE)rd.idx();
        gw_INDEX r = n + p + 1;
        gw_INDEX s = m + q + 1;
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
        t = new (ctx) SmBSplineSurface(sur, FALSE, &ctx);
        o.push_back(0.0);
    }
    else
    {
        SmStatus stt = SM_SUCCESS;
        SmSurfOfExtrusion* src = read_src(rd, ctx, bytes, &stt);
        o.push_back(st(stt));
        if (!src)
            return NULL;
        t = new (ctx) SmBSplineSurface(src->GetGwNurbPointer());
        delete src;
    }
    bool transpose = rd.flag();
    bool reparam = rd.flag();
    SmExtent2d d = rd.domain();
    bool perturb = rd.flag();
    int64_t pi = rd.idx();
    int64_t pj = rd.idx();
    SmVector3d pd = rd.vec();
    if (transpose)
        o.push_back(st(t->SwapUV()));
    if (reparam)
        o.push_back(st(t->Reparameterize(d)));
    if (perturb)
    {
        gw_SURFACE* sur = t->GetOrCreateGwNurbPointer();
        if (!sur || pi < 0 || pj < 0 || pi > sur->net->n || pj > sur->net->m)
        {
            delete t;
            *guard = true;
            return NULL;
        }
        sur->net->Pw[pi][pj].x += pd.x;
        sur->net->Pw[pi][pj].y += pd.y;
        sur->net->Pw[pi][pj].z += pd.z;
    }
    return t;
}

// Runs the op; SC_ERR_ARG when upstream would read undefined values (no output then).
int32_t surface_op(int32_t op, Rd& rd, const SmContext& ctx, const uint8_t* bytes,
                   SmSurfOfExtrusion* s, std::vector<double>& o)
{
    switch (op)
    {
    case OP_STATE:
        o.push_back(0.0);
        put_state(o, s);
        break;
    case OP_PLANE:
    {
        SmPlane* pl = read_plane(rd, ctx);
        bool has_dom = rd.flag();
        SmExtent2d dom = rd.domain();
        bool has_odom = rd.flag();
        SmExtent2d odom = rd.domain();
        OptTol tol(rd);
        if (!has_dom)
            dom = s->GetNaturalUVDomain();
        if (!has_odom)
            odom = pl->GetNaturalUVDomain();
        Tsect t;
        SmBoolean edges[2] = {TRUE, TRUE};
        SmStatus stt = s->IntersectWithPlane(ctx, dom, *pl, odom, edges, tol.ptr(), NULL, t.more,
                                             &t.curves, &t.uv1, &t.uv2, &t.types, &t.devs);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_tsect(o, t.more, t.curves, t.uv1, t.uv2, t.types, t.devs);
        delete pl;
        break;
    }
    case OP_CYLINDER:
    {
        SmStatus cst = SM_SUCCESS;
        SmCone* c = read_cone(rd, ctx, &cst);
        OptTol tol(rd);
        o.push_back(st(cst));
        if (!c)
            break;
        // the bottom iso circle IntersectWithCylinder makes (with its tolerance)
        double dtol = tol.has ? (double)tol.tol : SM_EFF_ZERO_SQRT;
        SmPoint3d start = c->GetPosition().GetOriginRef() +
                          smos_Min(c->GetBotHeight(), c->GetTopHeight()) *
                              c->GetPosition().GetZAxis();
        double dr = SM_EFF_ZERO * (1.0 + smos_Max(c->GetBotRadius(), c->GetTopRadius()));
        if (smos_Fabs(c->GetBotRadius() - c->GetTopRadius()) <= dr &&
            iso_circle_is_undefined(c, start, dtol))
        {
            delete c;
            return SC_ERR_ARG;
        }
        Tsect t;
        SmBoolean edges[2] = {TRUE, TRUE};
        SmStatus stt =
            s->IntersectWithCylinder(ctx, s->GetNaturalUVDomain(), *c, c->GetNaturalUVDomain(),
                                     edges, tol.ptr(), NULL, t.more, &t.curves, &t.uv1, &t.uv2,
                                     &t.types, &t.devs);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_tsect(o, t.more, t.curves, t.uv1, t.uv2, t.types, t.devs);
        delete c;
        break;
    }
    case OP_EXTRUSION:
    {
        SmStatus ost = SM_SUCCESS;
        SmSurfOfExtrusion* other = read_src(rd, ctx, bytes, &ost);
        OptTol tol(rd);
        o.push_back(st(ost));
        if (!other)
            break;
        Tsect t;
        SmBoolean edges[2] = {TRUE, TRUE};
        SmStatus stt = s->IntersectWithSurfOfExtrusion(
            ctx, s->GetNaturalUVDomain(), *other, other->GetNaturalUVDomain(), edges, tol.ptr(),
            NULL, t.more, &t.curves, &t.uv1, &t.uv2, &t.types, &t.devs);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_tsect(o, t.more, t.curves, t.uv1, t.uv2, t.types, t.devs);
        delete other;
        break;
    }
    case OP_GLOBAL:
    {
        int64_t kind = rd.idx();
        SmSurface* other = NULL;
        SmStatus ost = SM_SUCCESS;
        if (kind == TG_PLANE)
            other = read_plane(rd, ctx);
        else if (kind == TG_CONE)
            other = read_cone(rd, ctx, &ost);
        else
        {
            SmSurfOfExtrusion* e = read_src(rd, ctx, bytes, &ost);
            if (e && kind == TG_BSPLINE)
            {
                other = new (ctx) SmBSplineSurface(e->GetGwNurbPointer());
                delete e;
            }
            else
                other = e;
        }
        bool has_dom = rd.flag();
        SmExtent2d dom = rd.domain();
        bool has_odom = rd.flag();
        SmExtent2d odom = rd.domain();
        OptTol tol(rd);
        bool has_ang = rd.flag();
        double ang = rd.num();
        o.push_back(st(ost));
        if (!other)
            break;
        if (!has_dom)
            dom = s->GetNaturalUVDomain();
        if (!has_odom)
            odom = other->GetNaturalUVDomain();
        SmBoolean edges[2] = {TRUE, TRUE};

        // the special case GlobalSurfaceIntersect tries (same dispatch)
        bool special = false;
        {
            Tsect t;
            SmStatus pst = SM_SUCCESS;
            switch (other->GetType())
            {
            case SmPlane_TYPE:
                special = true;
                pst = s->IntersectWithPlane(ctx, dom, *(SmPlane*)other, odom, edges, tol.ptr(),
                                            has_ang ? &ang : NULL, t.more, &t.curves, &t.uv1,
                                            &t.uv2, &t.types, &t.devs);
                break;
            case SmCone_TYPE:
            {
                special = true;
                SmCone* c = (SmCone*)other;
                double dtol = tol.has ? (double)tol.tol : SM_EFF_ZERO_SQRT;
                double dr =
                    SM_EFF_ZERO * (1.0 + smos_Max(c->GetBotRadius(), c->GetTopRadius()));
                SmPoint3d start = c->GetPosition().GetOriginRef() +
                                  smos_Min(c->GetBotHeight(), c->GetTopHeight()) *
                                      c->GetPosition().GetZAxis();
                if (smos_Fabs(c->GetBotRadius() - c->GetTopRadius()) <= dr &&
                    iso_circle_is_undefined(c, start, dtol))
                {
                    delete other;
                    return SC_ERR_ARG;
                }
                pst = s->IntersectWithCylinder(ctx, dom, *c, odom, edges, tol.ptr(),
                                               has_ang ? &ang : NULL, t.more, &t.curves, &t.uv1,
                                               &t.uv2, &t.types, &t.devs);
                break;
            }
            case SmSurfOfExtrusion_TYPE:
                special = true;
                pst = s->IntersectWithSurfOfExtrusion(
                    ctx, dom, *(SmSurfOfExtrusion*)other, odom, edges, tol.ptr(),
                    has_ang ? &ang : NULL, t.more, &t.curves, &t.uv1, &t.uv2, &t.types, &t.devs);
                break;
            default: break;
            }
            if (special && pst != SM_SUCCESS)
            {
                o.push_back(st(pst));
                delete other;
                break;
            }
        }
        Tsect t;
        SmStatus stt = s->GlobalSurfaceIntersect(ctx, dom, *other, odom, edges, tol.ptr(),
                                                 has_ang ? &ang : NULL, &t.curves, &t.uv1, &t.uv2,
                                                 &t.types, &t.devs);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_tsect(o, FALSE, t.curves, t.uv1, t.uv2, t.types, t.devs);
        delete other;
        break;
    }
    case OP_TRIM:
    {
        SmExtent2d d = rd.domain();
        SmStatus stt = s->TrimWithDomain(d);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_state(o, s);
        break;
    }
    default: return SC_ERR_ARG;
    }
    return SC_OK;
}
} // namespace

SC_EXPORT int32_t sc_surface_surf_of_extrusion_p2_eval(int32_t op, const double* in, int64_t n_in,
                                                       const uint8_t* bytes, int64_t n_bytes,
                                                       double* out, int64_t cap, int64_t* out_len)
{
    (void)n_in;
    (void)n_bytes;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    if (op == OP_IS_NURB)
    {
        bool guard = false;
        SmBSplineSurface* t = read_test_surface(rd, ctx, bytes, o, &guard);
        if (guard)
            return SC_ERR_ARG;
        double scale = rd.num();
        if (t)
        {
            SmSurfOfExtrusion* r = NULL;
            SmBoolean ret = SmSurfOfExtrusion::IsNurbSurfaceSurfOfExtrusion(ctx, t, r, scale);
            // TRUE with a surface, FALSE, or a failing SER status returned as the boolean
            if (ret == TRUE && r)
            {
                // whether the NURB was made, then the state with it made (the state's
                // GetGenDirParamExtent would make it lazily)
                o.push_back(0.0);
                o.push_back(1.0);
                o.push_back(flg(r->GetGwNurbPointer() != NULL));
                SmStatus mst = r->GetGwNurbPointer() ? SM_SUCCESS : r->MakeNurb();
                o.push_back(st(mst));
                if (mst == SM_SUCCESS)
                    put_state(o, r);
            }
            else if (ret == FALSE)
            {
                o.push_back(0.0);
                o.push_back(0.0);
            }
            else
                o.push_back((double)ret);
            delete r;
            delete t;
        }
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    SmStatus stt = SM_SUCCESS;
    SmSurfOfExtrusion* s = read_src(rd, ctx, bytes, &stt);
    o.push_back(st(stt));
    if (s)
    {
        int32_t r = surface_op(op, rd, ctx, bytes, s, o);
        delete s;
        if (r != SC_OK)
            return r;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
