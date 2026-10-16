// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `nurbs.b_spline_surface_p1`.
// Wraps the SmBSplineSurface members owned by the unit (source/SMLib/src/SmBSplineSurface.cpp,
// source/SMLib/inc/SmBSplineSurface.h): the constructors, copy, operator==, CalculateBoundingBox,
// CreateBilinearSurface, CreateCanonical, CreateConePatch, CreateLinearSweep,
// CreateOffsetSurface, GetCanonical, GetControlPointNet, GetControlPointsPointer, the counts,
// IsDegenerateCurve, IsParallelToVector, IsRational, GetGrevilleAbscissa, GetDegree, GetKnots,
// GetNaturalUVDomain, MakeNurb, STEPInversion, Transform, IsKindOf, ReadFromDB, GetType and the
// inline accessors.
//
// One entry point, sc_nurbs_b_spline_surface_p1_eval(op, in, n_in, out, cap, out_len). `in`
// holds a surface block (how `this` is built) then the op's parameters; layouts are documented
// in crates/solidcore-ffi/src/units/nurbs_b_spline_surface_p1.rs, which validates every input
// so upstream only touches valid memory. `out` receives: the op status (0 = SM_SUCCESS, else
// the SmStatus value), the op's outputs (only on success), then for surface ops the state of
// `this`. A surface state is: has_nurb oob borrowed knot_type form
// [n m p q r s cpts(x y z w) u v].
#include "sc_shim.h"

#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmDatabaseIO.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmPolarBox.h>
#include <SmPseudoBox.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <sstream>
#include <string>
#include <vector>

#include "sc_sm.h"

using sc::flg;
using sc::put_vec;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/nurbs_b_spline_surface_p1.rs).
enum : int32_t
{
    OP_STATE = 0,
    OP_COUNTS = 1,
    OP_DOMAIN = 2,
    OP_KNOTS = 3,
    OP_GREVILLE = 4,
    OP_GET_CANONICAL = 5,
    OP_NET = 6,
    OP_CONTROL_POINTS = 7,
    OP_PARALLEL = 8,
    OP_BBOX = 9,
    OP_TRANSFORM = 10,
    OP_EQ = 11,
    OP_IS_KIND_OF = 12,
    OP_TYPE = 13,
    OP_MAKE_NURB = 14,
    OP_COPY = 15,
    OP_OFFSET = 16,
    OP_DEGEN_CURVE = 17,
    OP_STEP_INV = 18,
    OP_SET_FORM = 19,
    OP_BILINEAR = 30,
    OP_CANONICAL = 31,
    OP_CONE = 32,
    OP_SWEEP = 33,
    OP_READ_DB = 34,
};

// Surface constructions of the surface block.
enum : int32_t
{
    CT_NONE = 0,          // static op, no surface
    CT_EMPTY = 1,         // SmBSplineSurface()
    CT_FAST_OWNED = 2,    // SmBSplineSurface(gw_SURFACE*, FALSE, NULL)
    CT_FAST_BORROWED = 3, // SmBSplineSurface(gw_SURFACE*, TRUE, NULL)
    CT_DATA = 4,          // SmBSplineSurface(n, m, p, q, r, s, Pw, U, V)
    CT_GW_COPY = 5,       // SmBSplineSurface(const gw_SURFACE*)
    CT_COPY = 6,          // copy constructor of a CT_FAST_OWNED surface
};

// Reads the protected STEP flags through pointers to members named in a derived class.
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
    SmVector3d vec()
    {
        double x = num();
        double y = num();
        double z = num();
        return SmVector3d(x, y, z);
    }
};

// Reads `n m p q r s cpts u v` into a surface from sm_AllocateNurbSurface.
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

// Reads `n p m cpts knots` into a curve from sm_AllocateNurbCurve.
gw_CURVE* read_curve_nurb(Rd& rd)
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

// Builds `this` from a surface block: ctor oob form [surface]. Borrowed NURBs are never freed.
SmBSplineSurface* read_surface(Rd& rd, const SmContext& ctx, int32_t ctor)
{
    bool oob = rd.idx() != 0;
    SmBSplineSurfaceForm form = (SmBSplineSurfaceForm)rd.idx();
    SmBSplineSurface* s = NULL;
    switch (ctor)
    {
    case CT_EMPTY: s = new (ctx) SmBSplineSurface(); break;
    case CT_FAST_OWNED: s = new (ctx) SmBSplineSurface(read_nurb(rd), FALSE, NULL); break;
    case CT_FAST_BORROWED: s = new (ctx) SmBSplineSurface(read_nurb(rd), TRUE, NULL); break;
    case CT_DATA:
    {
        gw_SURFACE* src = read_nurb(rd);
        s = new (ctx) SmBSplineSurface((ULONG)src->net->n, (ULONG)src->net->m, (short)src->p,
                                       (short)src->q, (ULONG)src->knu->m, (ULONG)src->knv->m,
                                       src->net->Pw, src->knu->U, src->knv->U);
        smos_Free(src);
        break;
    }
    case CT_GW_COPY:
    {
        gw_SURFACE* src = read_nurb(rd);
        s = new (ctx) SmBSplineSurface(src);
        smos_Free(src);
        break;
    }
    case CT_COPY:
    {
        SmBSplineSurface* orig = new (ctx) SmBSplineSurface(read_nurb(rd), FALSE, NULL);
        orig->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
        orig->SetBSplineSurfaceForm(form);
        s = new (ctx) SmBSplineSurface(*orig);
        delete orig;
        break;
    }
    default: return NULL;
    }
    s->GetOutOfBoundsEnabled() = oob ? TRUE : FALSE;
    s->SetBSplineSurfaceForm(form);
    return s;
}

void put_state(std::vector<double>& o, const SmBSplineSurface* s)
{
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

// SetCanonical(O, X, Y) of a frame read as 9 numbers.
SmStatus read_frame(Rd& rd, SmAxis2Placement& f)
{
    SmVector3d o = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    return f.SetCanonical(o, x, y);
}

SmExtent2d read_domain(Rd& rd)
{
    double a = rd.num();
    double b = rd.num();
    double c = rd.num();
    double d = rd.num();
    return SmExtent2d(a, b, c, d);
}

// Status, then (on success) the new surface's state; deletes it.
void put_new(std::vector<double>& o, SmStatus s, SmSurface* c)
{
    o.push_back(st(s));
    if (s == SM_SUCCESS && c)
        put_state(o, (SmBSplineSurface*)c);
    delete c;
}

void put_knots(std::vector<double>& o, const SmTArray<double>& k, const SmTArray<ULONG>& mu)
{
    o.push_back((double)k.GetSize());
    for (ULONG i = 0; i < k.GetSize(); i++)
        o.push_back(k[i]);
    for (ULONG i = 0; i < mu.GetSize(); i++)
        o.push_back((double)mu[i]);
}

void put_pseudo(std::vector<double>& o, const SmPseudoBox& b)
{
    for (ULONG i = 0; i < 3; i++)
        put_vec(o, b.GetBasis(i));
    for (ULONG i = 0; i < 3; i++)
    {
        o.push_back(b.GetInterval(i).GetMin());
        o.push_back(b.GetInterval(i).GetMax());
    }
}

void put_polar(std::vector<double>& o, const SmPolarBox& b)
{
    o.push_back((double)b.GetPolarBoxState());
    for (ULONG i = 0; i < 3; i++)
        put_vec(o, *b.GetBasis(i));
    SmExtent2d d = b.GetDomain();
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
}

void static_op(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    switch (op)
    {
    case OP_BILINEAR:
    {
        SmPoint3d p00 = rd.vec();
        SmPoint3d p10 = rd.vec();
        SmPoint3d p01 = rd.vec();
        SmPoint3d p11 = rd.vec();
        SmBSplineSurface* s = NULL;
        SmStatus stt = SmBSplineSurface::CreateBilinearSurface(ctx, p00, p10, p01, p11, s);
        put_new(o, stt, s);
        break;
    }
    case OP_CANONICAL:
    {
        ULONG ud = (ULONG)rd.idx();
        ULONG vd = (ULONG)rd.idx();
        int64_t ncp = rd.idx();
        SmTArray<SmPoint3d> cps;
        for (int64_t i = 0; i < ncp; i++)
            cps.Add(rd.vec());
        SmBSplineSurfaceForm form = (SmBSplineSurfaceForm)rd.idx();
        SmTArray<double> uk, vk;
        SmTArray<ULONG> um, vm;
        int64_t nu = rd.idx();
        for (int64_t i = 0; i < nu; i++)
            uk.Add(rd.num());
        for (int64_t i = 0; i < nu; i++)
            um.Add((ULONG)rd.idx());
        int64_t nv = rd.idx();
        for (int64_t i = 0; i < nv; i++)
            vk.Add(rd.num());
        for (int64_t i = 0; i < nv; i++)
            vm.Add((ULONG)rd.idx());
        SmKnotType kt = (SmKnotType)rd.idx();
        bool has_w = rd.idx() != 0;
        SmTArray<double> w;
        if (has_w)
        {
            int64_t nw = rd.idx();
            for (int64_t i = 0; i < nw; i++)
                w.Add(rd.num());
        }
        bool has_dom = rd.idx() != 0;
        SmExtent2d dom = read_domain(rd);
        SmBSplineSurface* s = NULL;
        SmStatus stt = SmBSplineSurface::CreateCanonical(ctx, ud, vd, cps, form, um, vm, uk, vk, kt,
                                                         has_w ? &w : NULL,
                                                         has_dom ? &dom : NULL, s);
        put_new(o, stt, s);
        break;
    }
    case OP_CONE:
    {
        SmAxis2Placement f;
        SmStatus fs = read_frame(rd, f);
        double rb = rd.num();
        double rt = rd.num();
        double a0 = rd.num();
        double a1 = rd.num();
        double h = rd.num();
        SmNurbCircleParam prm = rd.idx() == 0 ? SM_CO_QUADRATIC : SM_CO_QUINTIC;
        if (fs != SM_SUCCESS)
        {
            o.push_back(st(fs));
            break;
        }
        SmBSplineSurface* s = NULL;
        SmStatus stt = SmBSplineSurface::CreateConePatch(ctx, f, rb, rt, a0, a1, h, prm, s);
        put_new(o, stt, s);
        break;
    }
    case OP_SWEEP:
    {
        ULONG dim = (ULONG)rd.idx();
        SmBSplineCurve* c = new (ctx) SmBSplineCurve(read_curve_nurb(rd), dim, FALSE);
        SmVector3d v = rd.vec();
        SmBSplineSurface* s = NULL;
        SmStatus stt = SmBSplineSurface::CreateLinearSweep(ctx, *c, v, s);
        put_new(o, stt, s);
        // the swept curve's NURB afterwards (NLib makes a 2D curve 3D in place)
        const gw_CURVE* cur = c->GetGwNurbPointer();
        o.push_back((double)cur->pol->n);
        for (gw_INDEX i = 0; i <= cur->pol->n; i++)
        {
            o.push_back(cur->pol->Pw[i].x);
            o.push_back(cur->pol->Pw[i].y);
            o.push_back(cur->pol->Pw[i].z);
            o.push_back(cur->pol->Pw[i].w);
        }
        delete c;
        break;
    }
    case OP_READ_DB:
    {
        int32_t file_type = (int32_t)rd.idx();
        ULONG version = (ULONG)rd.idx();
        int64_t nb = rd.idx();
        std::string bytes;
        for (int64_t i = 0; i < nb; i++)
            bytes.push_back((char)(unsigned char)rd.idx());
        std::istringstream iss(bytes);
        SmDatabaseIOStream db(file_type == 0 ? SM_ASCII : SM_BINARY, &iss);
        SmSurface* s = NULL;
        SmStatus stt = SmBSplineSurface::ReadFromDB(SmBSplineSurface_TYPE, db, ctx, s, version);
        put_new(o, stt, s);
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}

void surface_op(int32_t op, Rd& rd, const SmContext& ctx, SmBSplineSurface* s,
                std::vector<double>& o)
{
    switch (op)
    {
    case OP_STATE: o.push_back(0.0); break;
    case OP_COUNTS:
        o.push_back(0.0);
        o.push_back((double)s->GetDegree(SM_SP_U));
        o.push_back((double)s->GetDegree(SM_SP_V));
        o.push_back((double)s->GetNumberNaturalKnots(SM_SP_U));
        o.push_back((double)s->GetNumberNaturalKnots(SM_SP_V));
        o.push_back((double)s->GetNumberControlPoints(SM_SP_U));
        o.push_back((double)s->GetNumberControlPoints(SM_SP_V));
        o.push_back((double)s->GetNumberControlPoints(SM_SP_BOTH));
        o.push_back(flg(s->IsRational()));
        o.push_back(flg(s->GetOutOfBoundsEnabled()));
        o.push_back((double)s->GetBSplineSurfaceForm());
        break;
    case OP_DOMAIN:
    {
        SmExtent2d d = s->GetNaturalUVDomain();
        o.push_back(0.0);
        o.push_back(d.GetMin().x);
        o.push_back(d.GetMin().y);
        o.push_back(d.GetMax().x);
        o.push_back(d.GetMax().y);
        break;
    }
    case OP_KNOTS:
    {
        SmSurfParamType prm = rd.idx() == 0 ? SM_SP_U : SM_SP_V;
        bool has = rd.idx() != 0;
        double a = rd.num();
        double b = rd.num();
        SmExtent1d ivl(a, b);
        SmTArray<double> k;
        SmTArray<ULONG> mu;
        SmStatus stt = s->GetKnots(prm, k, &mu, has ? &ivl : NULL);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_knots(o, k, mu);
        break;
    }
    case OP_GREVILLE:
    {
        SmSurfParamType prm = rd.idx() == 0 ? SM_SP_U : SM_SP_V;
        ULONG i = (ULONG)rd.idx();
        o.push_back(0.0);
        o.push_back(s->GetGrevilleAbscissa(prm, i));
        break;
    }
    case OP_GET_CANONICAL:
    {
        ULONG ud = 0, vd = 0;
        SmTArray<SmPoint3d> pts;
        SmBSplineSurfaceForm form = SM_SF_UNSPECIFIED;
        SmTArray<ULONG> um, vm;
        SmTArray<double> uk, vk, w;
        SmKnotType kt = SM_KT_UNSPECIFIED;
        SmStatus stt = s->GetCanonical(ud, vd, pts, form, um, vm, uk, vk, kt, w);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back((double)ud);
            o.push_back((double)vd);
            o.push_back((double)form);
            o.push_back((double)kt);
            put_knots(o, uk, um);
            put_knots(o, vk, vm);
            o.push_back((double)pts.GetSize());
            for (ULONG i = 0; i < pts.GetSize(); i++)
                put_vec(o, pts[i]);
            o.push_back((double)w.GetSize());
            for (ULONG i = 0; i < w.GetSize(); i++)
                o.push_back(w[i]);
        }
        break;
    }
    case OP_NET:
    {
        ULONG uc = 0, vc = 0;
        SmTArray<SmPoint3d> pts;
        SmTArray<double> w;
        SmStatus stt = s->GetControlPointNet(uc, vc, pts, w);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back((double)uc);
            o.push_back((double)vc);
            o.push_back((double)pts.GetSize());
            for (ULONG i = 0; i < pts.GetSize(); i++)
                put_vec(o, pts[i]);
            o.push_back((double)w.GetSize());
            for (ULONG i = 0; i < w.GetSize(); i++)
                o.push_back(w[i]);
        }
        break;
    }
    case OP_CONTROL_POINTS:
    {
        ULONG uc = 0, vc = 0;
        double* pts = NULL;
        SmStatus stt = s->GetControlPointsPointer(uc, vc, pts);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back((double)uc);
            o.push_back((double)vc);
            for (ULONG i = 0; i < 4 * uc * vc; i++)
                o.push_back(pts[i]);
        }
        break;
    }
    case OP_PARALLEL:
    {
        SmVector3d v = rd.vec();
        o.push_back(0.0);
        o.push_back(flg(s->IsParallelToVector(v) != FALSE));
        break;
    }
    case OP_BBOX:
    {
        SmExtent2d dom = read_domain(rd);
        bool want_n = rd.idx() != 0;
        bool want_ps = rd.idx() != 0;
        bool want_pl = rd.idx() != 0;
        bool has_pg = rd.idx() != 0;
        SmVector3d b1 = rd.vec();
        SmVector3d b2 = rd.vec();
        SmVector3d b3 = rd.vec();
        bool has_plg = rd.idx() != 0;
        int64_t nvec = rd.idx();
        SmPolarBox plg;
        for (int64_t i = 0; i < nvec; i++)
            plg.AddVector3d(rd.vec());
        bool expand = rd.idx() != 0;
        SmPseudoBox pg;
        pg.SetBasis(b1, b2, b3);
        SmExtent3d nb;
        SmPseudoBox pb;
        SmPolarBox pl;
        SmStatus stt = s->CalculateBoundingBox(dom, want_n ? &nb : NULL, want_ps ? &pb : NULL,
                                               want_pl ? &pl : NULL, has_pg ? &pg : NULL,
                                               has_plg ? &plg : NULL, expand ? TRUE : FALSE);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            if (want_n)
            {
                for (ULONG i = 0; i < 3; i++)
                    o.push_back(nb.GetMin(i));
                for (ULONG i = 0; i < 3; i++)
                    o.push_back(nb.GetMax(i));
            }
            if (want_ps)
                put_pseudo(o, pb);
            if (want_pl)
                put_polar(o, pl);
        }
        break;
    }
    case OP_TRANSFORM:
    {
        SmAxis2Placement f;
        SmStatus fs = read_frame(rd, f);
        bool has_s = rd.idx() != 0;
        SmVector3d sc = rd.vec();
        if (fs != SM_SUCCESS)
        {
            o.push_back(st(fs));
            break;
        }
        o.push_back(st(s->Transform(f, has_s ? &sc : NULL)));
        break;
    }
    case OP_EQ:
    {
        int32_t ctor = (int32_t)rd.idx();
        SmBSplineSurface* other = read_surface(rd, ctx, ctor);
        o.push_back(0.0);
        o.push_back(flg(*s == *other));
        o.push_back(flg(*s == *s));
        delete other;
        break;
    }
    case OP_IS_KIND_OF:
    {
        SM_TYPE t = (SM_TYPE)rd.idx();
        o.push_back(0.0);
        o.push_back(flg(s->IsKindOf(t)));
        break;
    }
    case OP_TYPE:
        o.push_back(0.0);
        o.push_back((double)s->GetType());
        o.push_back((double)SmBSplineSurface::GetClassType());
        break;
    case OP_MAKE_NURB: o.push_back(st(s->MakeNurb())); break;
    case OP_COPY:
    {
        SmSurface* cp = NULL;
        SmStatus stt = s->Copy(ctx, cp);
        put_new(o, stt, cp);
        break;
    }
    case OP_OFFSET:
    {
        double d = rd.num();
        double tol = rd.num();
        SmSurface* r = NULL;
        SmStatus stt = s->CreateOffsetSurface(ctx, d, tol, r);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            o.push_back(r ? (double)r->GetType() : 0.0);
        delete r;
        break;
    }
    case OP_DEGEN_CURVE:
    {
        double ang = rd.num();
        bool has = rd.idx() != 0;
        SmExtent2d dom = read_domain(rd);
        double tol = rd.num();
        o.push_back(0.0);
        o.push_back(flg(s->IsDegenerateCurve(ang, has ? &dom : NULL, tol) != FALSE));
        break;
    }
    case OP_STEP_INV:
    {
        SmExtent2d dom = read_domain(rd);
        SmPoint3d p = rd.vec();
        double tol = rd.num();
        bool has_g = rd.idx() != 0;
        double gu = rd.num();
        double gv = rd.num();
        SmPoint2d g(gu, gv);
        SmPoint2d uv;
        SmLocationType loc = SM_LT_EXTERIOR;
        SmStatus stt = s->STEPInversion(dom, p, tol, uv, loc, has_g ? &g : NULL);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(uv.x);
            o.push_back(uv.y);
            o.push_back((double)loc);
        }
        break;
    }
    case OP_SET_FORM:
    {
        SmBSplineSurfaceForm f = (SmBSplineSurfaceForm)rd.idx();
        s->SetBSplineSurfaceForm(f);
        s->ToggleSwapUVBit();
        o.push_back(st(s->RebuildSTEPFromNURBParameters()));
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}
} // namespace

SC_EXPORT int32_t sc_nurbs_b_spline_surface_p1_eval(int32_t op, const double* in, int64_t n_in,
                                                    double* out, int64_t cap, int64_t* out_len)
{
    (void)n_in;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    int32_t ctor = (int32_t)rd.idx();
    if (ctor == CT_NONE)
    {
        static_op(op, rd, ctx, o);
    }
    else
    {
        SmBSplineSurface* s = read_surface(rd, ctx, ctor);
        if (s == NULL)
            return SC_ERR_ARG;
        surface_op(op, rd, ctx, s, o);
        put_state(o, s);
        delete s;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
