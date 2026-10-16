// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.surf_of_revolution_p3`.
// Wraps the SmSurfOfRevolution members owned by the unit (source/SMLib/src/SmSurfOfRevolution.cpp):
// Copy, Reparameterize, ConvertUVFromSTEPToNURBS, DropPointFast, IntersectWithCone,
// IntersectWithSphere, IntersectWithSurfOfRevolution, RebuildSTEPFromNURBParameters,
// Transform, UpdateAnalyticalDomain and ReadFromDB (WriteToDB is a generator helper), and
// the inherited SmBSplineSurface::Reverse / SwapUV that reach the rebuild virtually.
//
// One entry point, sc_surface_surf_of_revolution_p3_eval(op, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/surface_surf_of_revolution_p3.rs,
// which validates every input so upstream only touches valid memory. The surface block and
// the state record are those of surface_surf_of_revolution_p1.cpp. `out` starts with the
// status of the surface construction (or of the static op).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmCircle.h>
#include <SmCone.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmDatabaseIO.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmSphere.h>
#include <SmSurfOfRevolution.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector2d.h>
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
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_surf_of_revolution_p3.rs).
enum : int32_t
{
    OP_COPY = 0,
    OP_REPARAM = 1,
    OP_TO_NURBS = 2,
    OP_DROP = 3,
    OP_INTERSECT = 4,
    OP_REBUILD = 5,
    OP_TRANSFORM = 6,
    OP_UPDATE = 7,
    OP_REVERSE = 8,
    OP_SWAP = 9,
    OP_WRITE_DB = 20,
    OP_READ_DB = 21,
};

// Generator kinds.
enum : int32_t
{
    GK_BSPLINE = 0,
    GK_LINE = 1,
    GK_CIRCLE = 2,
};

// Construction modes.
enum : int32_t
{
    BM_CANONICAL = 0, // CreateCanonical(curve, axis point, axis direction, swap)
    BM_CTOR = 1,      // SmSurfOfRevolution(curve, origin, x, y, domain, swap) + MakeNurb
};

// The other operand of OP_INTERSECT.
enum : int32_t
{
    OK_REVOLUTION = 0, // IntersectWithSurfOfRevolution with a surface block
    OK_CONE = 1,       // IntersectWithCone with SmCone::CreateCanonical(A2P, r0, r1, h)
    OK_SPHERE = 2,     // IntersectWithSphere with SmSphere::CreateCanonical(A2P, r)
};

// Reads the protected members through pointers to members named in a derived class, and
// reaches the protected constructor.
struct Peek : SmSurfOfRevolution
{
    static SmBoolean SmSurfOfRevolution::*planar() { return &Peek::m_bPlanarGenerator; }
    static SmPolarConversion SmSurfOfRevolution::*polar() { return &Peek::m_vPolarConverter; }
    Peek(SmBSplineCurve* c, const SmPoint3d& o, const SmVector3d& x, const SmVector3d& y,
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

// `n p m`, `n+1` points `x y z w`, `m+1` knots: SmBSplineCurve(gw_CURVE*, 3, FALSE).
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

// A generator block `kind ...`: a B-spline curve, a line `a[3] b[3]` (its NURB made) or a
// circle `c[3] x[3] y[3] d0 d1 r`.
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
    default: return NULL;
    }
}

// A surface block `mode gen oob swap` then for BM_CANONICAL `P[3] A[3]`, for BM_CTOR
// `O[3] X[3] Y[3] D[4]`. Writes the construction status; NULL on failure.
SmSurfOfRevolution* read_surface(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    int64_t mode = rd.idx();
    SmBSplineCurve* gen = read_gen(rd, ctx);
    bool oob = rd.flag();
    bool swap = rd.flag();
    SmSurfOfRevolution* s = NULL;
    SmStatus stt = SM_SUCCESS;
    if (mode == BM_CANONICAL)
    {
        SmPoint3d p = rd.vec();
        SmVector3d a = rd.vec();
        stt = SmSurfOfRevolution::CreateCanonical(ctx, gen, p, a, s, swap ? TRUE : FALSE);
    }
    else
    {
        SmPoint3d org = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        SmExtent2d d = rd.domain();
        s = new (ctx) Peek(gen, org, x, y, d, swap ? TRUE : FALSE);
        stt = s->MakeNurb();
    }
    o.push_back(st(stt));
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

// The surface state (the layout of surface_surf_of_revolution_p1.cpp).
void put_state(std::vector<double>& o, const SmSurfOfRevolution* s)
{
    o.push_back(flg(s->IsKindOf(SmSurfOfRevolution_TYPE)));
    o.push_back(flg(s->IsKindOf(SmBSplineSurface_TYPE)));
    o.push_back(flg(s->IsKindOf(SmSurface_TYPE)));
    o.push_back(flg(s->IsBounded()));
    o.push_back(flg(s->GetSwapUV()));
    o.push_back(flg(s->*Peek::planar()));
    const SmAxis2Placement& pos = s->GetPosition();
    put_vec(o, pos.GetOriginRef());
    put_vec(o, pos.GetXAxisRef());
    put_vec(o, pos.GetYAxisRef());
    SmExtent2d d = s->GetSTEPUVDomain();
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMax().y);
    o.push_back(s->GetStartAngleDeg());
    o.push_back(s->GetEndAngleDeg());
    SmExtent2d mx = s->GetMaxAnalyticDomain();
    o.push_back(mx.GetMin().x);
    o.push_back(mx.GetMin().y);
    o.push_back(mx.GetMax().x);
    o.push_back(mx.GetMax().y);
    SmSurfParamType gp = SM_SP_UNKNOWN;
    SmExtent1d gd = s->GetGenDirParamExtent(&gp);
    o.push_back(gd.GetMin());
    o.push_back(gd.GetMax());
    o.push_back((double)gp);
    const SmPolarConversion& pc = s->*Peek::polar();
    o.push_back(flg(s->HavePolarConversion()));
    ULONG cnt = pc.GetAngleCount();
    o.push_back((double)cnt);
    if (cnt > 0)
    {
        o.push_back(pc.GetFirstAngle());
        o.push_back(pc.GetLastAngle());
    }
    put_curve(o, pc.GetCurve());
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

// A domain block `natural [d4]`: the surface's natural UV domain, or the given one.
SmExtent2d read_dom(Rd& rd, const SmSurface* s)
{
    bool natural = rd.flag();
    SmExtent2d d = rd.domain();
    return natural ? s->GetNaturalUVDomain() : d;
}

// The intersection record: `more ncurves curves types devs nuv1 nuv2 nnull1 nnull2`.
void put_intersection(std::vector<double>& o, SmBoolean more, SmTArray<SmCurve*>& c3d,
                      SmTArray<SmCurve*>& uv1, SmTArray<SmCurve*>& uv2,
                      SmTArray<SmTsectCurveType>& types, SmTArray<double>& devs)
{
    o.push_back(flg(more));
    o.push_back((double)c3d.GetSize());
    for (ULONG i = 0; i < c3d.GetSize(); i++)
        put_curve(o, c3d[i]);
    o.push_back((double)types.GetSize());
    for (ULONG i = 0; i < types.GetSize(); i++)
        o.push_back((double)types[i]);
    o.push_back((double)devs.GetSize());
    for (ULONG i = 0; i < devs.GetSize(); i++)
        o.push_back(devs[i]);
    ULONG null1 = 0, null2 = 0;
    for (ULONG i = 0; i < uv1.GetSize(); i++)
        null1 += uv1[i] == NULL ? 1 : 0;
    for (ULONG i = 0; i < uv2.GetSize(); i++)
        null2 += uv2[i] == NULL ? 1 : 0;
    o.push_back((double)uv1.GetSize());
    o.push_back((double)uv2.GetSize());
    o.push_back((double)null1);
    o.push_back((double)null2);
}

void intersect(Rd& rd, const SmContext& ctx, SmSurfOfRevolution* s, std::vector<double>& o)
{
    int64_t kind = rd.idx();
    SmSurfOfRevolution* other = NULL;
    std::vector<double> o2;
    if (kind == OK_REVOLUTION)
    {
        bool self = rd.flag();
        if (self)
        {
            o2.push_back(0.0);
            other = s;
        }
        else
            other = read_surface(rd, ctx, o2);
    }
    else
    {
        SmPoint3d po = rd.vec();
        SmVector3d px = rd.vec();
        SmVector3d py = rd.vec();
        double r0 = rd.num();
        double r1 = rd.num();
        double h = rd.num();
        SmAxis2Placement a2p(po, px, py);
        SmStatus cs = SM_ERR;
        if (kind == OK_CONE)
        {
            SmCone* c = NULL;
            cs = SmCone::CreateCanonical(ctx, a2p, r0, r1, h, c);
            other = c;
        }
        else
        {
            SmSphere* sp = NULL;
            cs = SmSphere::CreateCanonical(ctx, a2p, r0, sp);
            other = sp;
        }
        o2.push_back(cs == SM_SUCCESS && other ? 0.0 : (double)(cs == SM_SUCCESS ? SM_ERR : cs));
        if (cs != SM_SUCCESS)
        {
            delete other;
            other = NULL;
        }
    }
    o.push_back(o2.empty() ? -1.0 : o2[0]);
    if (!other)
        return;
    SmExtent2d dom = read_dom(rd, s);
    SmExtent2d odom = read_dom(rd, other);
    bool has_tol = rd.flag();
    SmApproxTol3d tol = rd.num();
    SmBoolean edges[2] = {TRUE, TRUE};
    SmBoolean more = FALSE;
    SmTArray<SmCurve*> c3d, uv1, uv2;
    SmTArray<SmTsectCurveType> types;
    SmTArray<double> devs;
    SmStatus stt = SM_ERR;
    if (kind == OK_REVOLUTION)
        stt = s->IntersectWithSurfOfRevolution(ctx, dom, *other, odom, edges,
                                               has_tol ? &tol : NULL, NULL, more, &c3d, &uv1,
                                               &uv2, &types, &devs);
    else if (kind == OK_CONE)
        stt = s->IntersectWithCone(ctx, dom, *(SmCone*)other, odom, edges, has_tol ? &tol : NULL,
                                   NULL, more, &c3d, &uv1, &uv2, &types, &devs);
    else
        stt = s->IntersectWithSphere(ctx, dom, *(SmSphere*)other, odom, edges,
                                     has_tol ? &tol : NULL, NULL, more, &c3d, &uv1, &uv2,
                                     &types, &devs);
    o.push_back(st(stt));
    if (stt == SM_SUCCESS)
        put_intersection(o, more, c3d, uv1, uv2, types, devs);
    for (ULONG i = 0; i < c3d.GetSize(); i++)
        delete c3d[i];
    if (other != s)
        delete other;
}

void surface_op(int32_t op, Rd& rd, const SmContext& ctx, SmSurfOfRevolution* s,
                std::vector<double>& o)
{
    switch (op)
    {
    case OP_COPY:
    {
        SmSurface* c = NULL;
        SmStatus stt = s->Copy(ctx, c);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS && c)
        {
            put_state(o, (SmSurfOfRevolution*)c);
            o.push_back(flg(*c == *s));
        }
        delete c;
        break;
    }
    case OP_REPARAM:
    {
        SmExtent2d d = rd.domain();
        SmStatus stt = s->Reparameterize(d);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_state(o, s);
        break;
    }
    case OP_TO_NURBS:
    {
        double u = rd.num();
        double v = rd.num();
        SmPoint2d r(0.0, 0.0);
        SmStatus stt = s->ConvertUVFromSTEPToNURBS(SmPoint2d(u, v), r);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back(r.x);
            o.push_back(r.y);
        }
        break;
    }
    case OP_DROP:
    {
        SmExtent2d dom = read_dom(rd, s);
        SmSolverOperationType oper = (SmSolverOperationType)rd.idx();
        SmPoint3d p = rd.vec();
        bool has_vec = rd.flag();
        SmVector3d v = rd.vec();
        double tol = rd.num();
        bool has_target = rd.flag();
        double target = rd.num();
        SmSolutionRequestedType req = (SmSolutionRequestedType)rd.idx();
        SmSolutionArray sols;
        SmStatus stt = s->DropPointFast(dom, oper, p, has_vec ? &v : NULL, tol,
                                        has_target ? &target : NULL, req, sols);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
        {
            o.push_back((double)sols.GetSize());
            for (ULONG i = 0; i < sols.GetSize(); i++)
            {
                const SmSolution& sol = sols[i];
                o.push_back((double)sol.m_eSolutionType);
                o.push_back((double)sol.m_lNumObjects);
                o.push_back((double)sol.m_lNumVariables);
                o.push_back(flg(sol.m_apObjects[0] == s));
                o.push_back(sol.m_vStart.m_dSolutionValue);
                o.push_back(sol.m_vStart[0]);
                o.push_back(sol.m_vStart[1]);
            }
        }
        break;
    }
    case OP_INTERSECT: intersect(rd, ctx, s, o); break;
    case OP_REBUILD:
    {
        // optional edit of the NURB alone: SmBSplineSurface::Transform(frame, scale)
        bool edit = rd.flag();
        SmPoint3d fo = rd.vec();
        SmVector3d fx = rd.vec();
        SmVector3d fy = rd.vec();
        bool has_scale = rd.flag();
        SmVector3d sc = rd.vec();
        if (edit)
        {
            SmStatus es = s->SmBSplineSurface::Transform(SmAxis2Placement(fo, fx, fy),
                                                         has_scale ? &sc : NULL);
            if (es != SM_SUCCESS)
            {
                o.push_back((double)SC_ERR_ARG);
                break;
            }
        }
        SmStatus stt = s->RebuildSTEPFromNURBParameters();
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_state(o, s);
        break;
    }
    case OP_TRANSFORM:
    {
        SmPoint3d fo = rd.vec();
        SmVector3d fx = rd.vec();
        SmVector3d fy = rd.vec();
        bool has_scale = rd.flag();
        SmVector3d sc = rd.vec();
        SmStatus stt = s->Transform(SmAxis2Placement(fo, fx, fy), has_scale ? &sc : NULL);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_state(o, s);
        break;
    }
    case OP_UPDATE:
    {
        SmExtent2d d = read_dom(rd, s);
        SmStatus stt = s->UpdateAnalyticalDomain(d);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_state(o, s);
        break;
    }
    case OP_REVERSE:
    {
        // the inherited SmBSplineSurface::Reverse (virtual RebuildSTEPFromNURBParameters)
        SmSurfParamType param = rd.idx() == 0 ? SM_SP_U : SM_SP_V;
        SmStatus stt = s->Reverse(param);
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_state(o, s);
        break;
    }
    case OP_SWAP:
    {
        // the inherited SmBSplineSurface::SwapUV (virtual ToggleSwapUVBit, SetCanonical)
        SmStatus stt = s->SwapUV();
        o.push_back(st(stt));
        if (stt == SM_SUCCESS)
            put_state(o, s);
        break;
    }
    case OP_WRITE_DB:
    {
        // generator helper: the bytes upstream WriteToDB produces (ASCII with 17 digits)
        int64_t ft = rd.idx();
        ULONG version = (ULONG)rd.idx();
        std::ostringstream oss;
        oss.precision(17);
        SmDatabaseIOStream db(ft == 0 ? SM_ASCII : SM_BINARY, &oss);
        SmStatus stt = s->WriteToDB(db, version);
        o.push_back(st(stt));
        std::string bytes = oss.str();
        for (char ch : bytes)
            o.push_back((double)(unsigned char)ch);
        break;
    }
    default: o.push_back((double)SC_ERR_ARG); break;
    }
}

void read_db(Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    int64_t ft = rd.idx();
    ULONG version = (ULONG)rd.idx();
    int64_t nb = rd.idx();
    std::string bytes;
    for (int64_t i = 0; i < nb; i++)
        bytes.push_back((char)(unsigned char)rd.idx());
    std::istringstream iss(bytes);
    SmDatabaseIOStream db(ft == 0 ? SM_ASCII : SM_BINARY, &iss);
    SmSurface* s = NULL;
    SmStatus stt = SmSurfOfRevolution::ReadFromDB(SmSurfOfRevolution_TYPE, db, ctx, s, version);
    o.push_back(st(stt));
    if (stt == SM_SUCCESS && s)
        put_state(o, (SmSurfOfRevolution*)s);
    delete s;
}
} // namespace

SC_EXPORT int32_t sc_surface_surf_of_revolution_p3_eval(int32_t op, const double* in,
                                                        int64_t n_in, double* out, int64_t cap,
                                                        int64_t* out_len)
{
    if (in == nullptr || n_in < 0 || out_len == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    std::vector<double> o;
    Rd rd{in, 0};
    if (op == OP_READ_DB)
    {
        read_db(rd, ctx, o);
    }
    else
    {
        SmSurfOfRevolution* s = read_surface(rd, ctx, o);
        if (s)
        {
            surface_op(op, rd, ctx, s, o);
            delete s;
        }
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
