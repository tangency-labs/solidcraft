// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.tangent_field`: SmTangentField
// (source/SMLib/src/SmTangentField.cpp, source/SMLib/inc/SmTangentField.h).
//
// One entry point, sc_curve_tangent_field_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/curve_tangent_field.rs, which validates every
// input. A curve block is `kind dim ...` (0: SmBSplineCurve `n p m cpts(x y z w) knots`;
// 1: SmHermiteCurve `P1 D1 P2 D2`). A field block is `curve dir start(3) end(3) has_domain
// umin vmin umax vmax`. Every field lies on the same bilinear SmBSplineSurface built from
// the four corners `srf(12)` at the start of the input (P00 P10 P01 P11, knots 0 0 1 1).
// `out` receives the op status (0 = SM_SUCCESS, else the SmStatus value) and the op outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmCurveTypes.h>
#include <SmDatabaseIO.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmHermiteCurve.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmTangentField.h>
#include <SmVector2d.h>
#include <SmVector3d.h>
#include <SmTArray.h>
#include <nurbs.h>

#include <sstream>
#include <string>
#include <vector>

#include "sc_sm.h"

using sc::put_vec;

namespace sc_tf
{
enum Op : int32_t
{
    OP_STATE = 0,
    OP_EVAL = 1,
    OP_EVAL_POINT = 2,
    OP_EQ = 3,
    OP_CREATE = 4,
    OP_READ_DB = 5,
    OP_WRITE_READ = 6,
    OP_LAST = 6,
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

SmSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    gw_SURFACE* sur = sm_AllocateNurbSurface(1, 1, 1, 1, 3, 3);
    for (gw_INDEX j = 0; j <= 1; j++)
        for (gw_INDEX i = 0; i <= 1; i++)
        {
            sur->net->Pw[i][j].x = rd.num();
            sur->net->Pw[i][j].y = rd.num();
            sur->net->Pw[i][j].z = rd.num();
            sur->net->Pw[i][j].w = 1.0;
        }
    for (gw_INDEX i = 0; i <= 3; i++)
    {
        sur->knu->U[i] = i < 2 ? 0.0 : 1.0;
        sur->knv->U[i] = i < 2 ? 0.0 : 1.0;
    }
    return new (ctx) SmBSplineSurface(sur, FALSE, NULL);
}

// A field block on `srf`; `uv` receives the UV curve (owned by the caller). When
// `shared_uv` is given the block's curve is read and discarded, and `shared_uv` is used.
SmTangentField* read_field(Rd& rd, const SmContext& ctx, SmSurface* srf, SmCurve*& uv, SmCurve* shared_uv)
{
    SmCurve* c = read_curve(rd, ctx);
    if (shared_uv)
    {
        delete c;
        uv = NULL;
        c = shared_uv;
    }
    else
        uv = c;
    SmTangentDirType dir = (SmTangentDirType)rd.idx();
    SmVector3d s = rd.vec();
    SmVector3d e = rd.vec();
    bool has_domain = rd.flag();
    double u0 = rd.num();
    double v0 = rd.num();
    double u1 = rd.num();
    double v1 = rd.num();
    SmExtent2d dom;
    dom.SetMinMax(SmPoint2d(u0, v0), SmPoint2d(u1, v1));
    return new (ctx) SmTangentField(*c, *srf, dir, s, e, has_domain ? &dom : NULL);
}

void put_string(std::vector<double>& o, const TCHAR* s)
{
    std::vector<double> chars;
    for (const TCHAR* p = s; *p; ++p)
        chars.push_back((double)*p);
    o.push_back((double)chars.size());
    o.insert(o.end(), chars.begin(), chars.end());
}

const SM_TYPE kKinds[] = {SmCurve_TYPE,        SmCrvOnSurf_TYPE,    SmTangentField_TYPE,
                          SmBSplineCurve_TYPE, SmHermiteCurve_TYPE, SmLine_TYPE,
                          14000,               0};

void put_bytes(std::vector<double>& o, const std::string& b)
{
    o.push_back((double)b.size());
    for (unsigned char c : b)
        o.push_back((double)c);
}

// `type ReadType+WriteToDB` of `c` (the record a file holds for a curve) into `bytes`.
SmStatus write_curve(const SmCurve& c, SmFileType ft, ULONG version, std::string& bytes)
{
    std::ostringstream oss;
    if (ft == SM_ASCII)
    {
        // as SmDatabaseIOFile::OpenFileForWrite
        oss.setf(std::ios::fixed);
        oss.precision(16);
    }
    SmDatabaseIOStream db(ft, &oss);
    ULONG dim = c.GetDim();
    SmStatus s = db.WriteType(c.GetType(), &dim);
    if (s == SM_SUCCESS)
        s = c.WriteToDB(db, version);
    bytes = oss.str();
    return s;
}

// The queries of OP_WRITE_READ on the curve read back (see the ffi module docs).
void put_read_queries(std::vector<double>& o, const SmCurve& orig, SmCurve& c, const std::vector<double>& params,
                      double ang, SmFileType ft, ULONG version, const SmContext& ctx)
{
    o.push_back((double)c.GetType());
    for (SM_TYPE k : kKinds)
        o.push_back(c.IsKindOf(k) ? 1.0 : 0.0);
    SmExtent1d ivl = c.GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    o.push_back((c == orig) ? 1.0 : 0.0);
    for (double t : params)
    {
        SmVector3d p(0.0, 0.0, 0.0);
        SmStatus s = c.EvaluatePoint(t, p);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, p);
        SmVector3d pd[3] = {SmVector3d(0.0, 0.0, 0.0), SmVector3d(0.0, 0.0, 0.0), SmVector3d(0.0, 0.0, 0.0)};
        s = c.Evaluate(t, 1, TRUE, pd, TRUE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            put_vec(o, pd[0]);
            put_vec(o, pd[1]);
        }
        s = c.Evaluate(t, 2, TRUE, pd, TRUE);
        o.push_back(st(s));
    }
    // SmCrvOnSurf::CalculateContinuities on the derived object
    SmContinuityType mn = SM_CT_UNDEFINED;
    SmTArray<SmContinuityType> conts;
    SmStatus s = c.CalculateContinuities(mn, conts, ang);
    o.push_back(st(s));
    if (s == SM_SUCCESS)
    {
        // The interior entries and the minimum are indeterminate for a tangent field (its
        // Evaluate fails for 3 derivatives and the ignored EvaluateContinuity leaves
        // eThisContinuity uninitialised, UB-263): the count and the end entries, and the
        // minimum only without interior breaks.
        ULONG n = conts.GetSize();
        o.push_back((double)n);
        if (n > 0)
        {
            o.push_back((double)conts[0]);
            o.push_back((double)conts[n - 1]);
        }
        if (n < 3)
            o.push_back((double)mn);
    }
    // SmCrvOnSurf::Copy (not overridden): a plain SmCrvOnSurf
    SmCurve* cp = NULL;
    s = c.Copy(ctx, cp);
    o.push_back(st(s));
    if (s == SM_SUCCESS && cp)
    {
        o.push_back((double)cp->GetType());
        for (double t : params)
        {
            SmVector3d p(0.0, 0.0, 0.0);
            SmStatus se = cp->EvaluatePoint(t, p);
            o.push_back(st(se));
            if (se == SM_SUCCESS)
                put_vec(o, p);
        }
        delete cp;
    }
    // written again (the round trip)
    std::string again;
    s = write_curve(c, ft, version, again);
    o.push_back(st(s));
    if (s == SM_SUCCESS)
        put_bytes(o, again);
}

void run(int32_t op, Rd& rd, const SmContext& ctx, std::vector<double>& o)
{
    if (op == OP_READ_DB)
    {
        SmFileType ft = rd.idx() == 0 ? SM_ASCII : SM_BINARY;
        ULONG dim = (ULONG)rd.idx();
        ULONG version = (ULONG)rd.idx();
        int64_t nb = rd.idx();
        std::string bytes;
        for (int64_t i = 0; i < nb; i++)
            bytes.push_back((char)(unsigned char)rd.idx());
        std::istringstream iss(bytes);
        SmDatabaseIOStream db(ft, &iss);
        SmCurve* c = NULL;
        SmStatus s = SmTangentField::ReadFromDB(SmTangentField_TYPE, db, dim, ctx, c, version);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            delete c;
        return;
    }

    SmSurface* srf = read_surface(rd, ctx);
    if (op == OP_CREATE)
    {
        double tol = rd.num();
        SmCurve* base3d = read_curve(rd, ctx);
        SmCurve* base_uv = read_curve(rd, ctx);
        SmCurve* sc = read_curve(rd, ctx);
        double sp = rd.num();
        SmOrientType so = rd.flag() ? SM_OT_OPPOSITE : SM_OT_SAME;
        SmCurve* ec = read_curve(rd, ctx);
        double ep = rd.num();
        SmOrientType eo = rd.flag() ? SM_OT_OPPOSITE : SM_OT_SAME;
        SmTangentField* tf = NULL;
        SmStatus s = SmTangentField::Create(ctx, tol, *base3d, *(SmBSplineCurve*)base_uv, *srf, *sc, sp, so,
                                            *ec, ep, eo, tf);
        o.push_back(st(s));
        if (s == SM_SUCCESS && tf)
        {
            SmExtent1d ivl = tf->GetNaturalInterval();
            SmVector3d a(0.0, 0.0, 0.0), b(0.0, 0.0, 0.0);
            tf->EvaluatePoint(ivl.GetMin(), a);
            tf->EvaluatePoint(ivl.GetMax(), b);
            put_vec(o, a);
            put_vec(o, b);
            delete tf;
        }
        delete ec;
        delete sc;
        delete base_uv;
        delete base3d;
        delete srf;
        return;
    }

    if (op == OP_WRITE_READ)
    {
        int64_t mode = rd.idx();
        SmTangentField* tf = NULL;
        SmCurve* uv = NULL;
        SmCurve* others[3] = {NULL, NULL, NULL};
        SmStatus cs = SM_SUCCESS;
        if (mode == 0)
            tf = read_field(rd, ctx, srf, uv, NULL);
        else
        {
            double tol = rd.num();
            others[0] = read_curve(rd, ctx);
            uv = read_curve(rd, ctx);
            others[1] = read_curve(rd, ctx);
            double sp = rd.num();
            SmOrientType so = rd.flag() ? SM_OT_OPPOSITE : SM_OT_SAME;
            others[2] = read_curve(rd, ctx);
            double ep = rd.num();
            SmOrientType eo = rd.flag() ? SM_OT_OPPOSITE : SM_OT_SAME;
            cs = SmTangentField::Create(ctx, tol, *others[0], *(SmBSplineCurve*)uv, *srf, *others[1], sp, so,
                                        *others[2], ep, eo, tf);
        }
        SmFileType ft = rd.idx() == 0 ? SM_ASCII : SM_BINARY;
        ULONG version = (ULONG)rd.idx();
        bool cache_breaks = rd.flag();
        double ang = rd.num();
        int64_t np = rd.idx();
        std::vector<double> params;
        for (int64_t i = 0; i < np; i++)
            params.push_back(rd.num());
        o.push_back(st(cs));
        if (cs == SM_SUCCESS && tf)
        {
            if (cache_breaks)
            {
                SmTArray<double> knots;
                SmStatus ks = tf->GetKnots(knots);
                o.push_back(st(ks));
                if (ks == SM_SUCCESS)
                {
                    o.push_back((double)knots.GetSize());
                    for (ULONG i = 0; i < knots.GetSize(); i++)
                        o.push_back(knots[i]);
                }
            }
            std::string bytes;
            SmStatus ws = write_curve(*tf, ft, version, bytes);
            o.push_back(st(ws));
            if (ws == SM_SUCCESS)
            {
                put_bytes(o, bytes);
                std::istringstream iss(bytes);
                SmDatabaseIOStream db(ft, &iss);
                SM_TYPE type = 0;
                ULONG dim = 0;
                SmCurve* c = NULL;
                SmStatus rs = db.ReadType(type, &dim);
                if (rs == SM_SUCCESS)
                    rs = SmCurve::ReadFromDB(type, db, dim, ctx, c, version);
                o.push_back(st(rs));
                if (rs == SM_SUCCESS && c)
                {
                    put_read_queries(o, *tf, *c, params, ang, ft, version, ctx);
                    delete c;
                }
            }
            delete tf;
        }
        delete uv;
        for (SmCurve* c : others)
            delete c;
        delete srf;
        return;
    }

    SmCurve* uv_a = NULL;
    SmTangentField* a = read_field(rd, ctx, srf, uv_a, NULL);
    switch (op)
    {
    case OP_STATE:
    {
        o.push_back(0.0);
        o.push_back((double)a->GetType());
        for (SM_TYPE k : kKinds)
            o.push_back(a->IsKindOf(k) ? 1.0 : 0.0);
        SmExtent1d ivl = a->GetNaturalInterval();
        o.push_back(ivl.GetMin());
        o.push_back(ivl.GetMax());
        put_string(o, a->GetTypeString());
        put_string(o, a->GetClassString());
        break;
    }
    case OP_EVAL:
    {
        double t = rd.num();
        ULONG n = (ULONG)rd.idx();
        std::vector<SmVector3d> pd((size_t)n + 1, SmVector3d(0.0, 0.0, 0.0));
        SmStatus s = a->Evaluate(t, n, TRUE, pd.data(), TRUE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            for (const SmVector3d& v : pd)
                put_vec(o, v);
        break;
    }
    case OP_EVAL_POINT:
    {
        double t = rd.num();
        SmVector3d p(0.0, 0.0, 0.0);
        SmStatus s = a->EvaluatePoint(t, p);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, p);
        break;
    }
    case OP_EQ:
    {
        // flags: bit 0 = B on the same surface object, bit 1 = B shares A's UV curve,
        // bit 2 = compare A with itself (B is still read).
        int64_t flags = rd.idx();
        SmSurface* srf_b = (flags & 1) ? srf : read_surface(rd, ctx);
        SmCurve* uv_b = NULL;
        SmTangentField* b = read_field(rd, ctx, srf_b, uv_b, (flags & 2) ? uv_a : NULL);
        SmBoolean eq = (flags & 4) ? (*a == *a) : (*a == *b);
        o.push_back(0.0);
        o.push_back(eq ? 1.0 : 0.0);
        delete b;
        delete uv_b;
        if (srf_b != srf)
            delete srf_b;
        break;
    }
    default: break;
    }
    delete a;
    delete uv_a;
    delete srf;
}
} // namespace sc_tf

SC_EXPORT int32_t sc_curve_tangent_field_eval(int32_t op, const double* in, int64_t n_in, double* out, int64_t cap,
                                              int64_t* out_len)
{
    if (in == nullptr || n_in < 0 || out_len == nullptr || op < 0 || op > sc_tf::OP_LAST)
        return SC_ERR_ARG;
    SC_TRY
    SmContext ctx;
    sc_tf::Rd rd{in, 0};
    std::vector<double> o;
    sc_tf::run(op, rd, ctx, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
