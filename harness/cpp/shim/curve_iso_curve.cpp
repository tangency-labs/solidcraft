// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `curve.iso_curve`.
// Wraps upstream SmIsoCurve (source/SMLib/inc/SmIsoCurve.h, source/SMLib/src/SmIsoCurve.cpp)
// on SmBSplineSurface base surfaces.
//
// One entry point, sc_curve_iso_curve_op(op, in, n_in, data, data_len, file_type, version, out,
// cap, out_len). `in` holds curve block A (except for OP_READ_DB) then the op's parameters;
// layouts are documented in crates/solidcore-ffi/src/units/curve_iso_curve.rs, which validates
// every input so upstream only touches valid memory.
//
// A curve block is: has_srf [n m p q r s cpts(x y z w as stored) u v] surf_param iso from_left
// clamp. has_srf 0 = SmBSplineSurface() (no NURB), 1 = SmBSplineSurface(gw_SURFACE*, FALSE,
// NULL) of the inline NURB.
#include "sc_shim.h"

#include <SmApiGeneral.h>
#include <SmBSplineSurface.h>
#include <SmContext.h>
#include <SmDatabaseIO.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmIsoCurve.h>
#include <SmNurbsSrf.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <sstream>
#include <string>
#include <vector>

#include "sc_sm.h"

using sc::put_vec;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/curve_iso_curve.rs).
enum : int32_t
{
    OP_STATE = 0,
    OP_KNOTS = 1,
    OP_EVALUATE = 2,
    OP_EVALUATE_POINT = 3,
    OP_CONTINUITIES = 4,
    OP_EQ = 5,
    OP_IS_KIND_OF = 6,
    OP_TYPE_STRINGS = 7,
    OP_UV_ISO_LINE = 8,
    OP_READ_DB = 9,
    OP_LAST = 9,
};

// Reads private members through explicit instantiation (access checks do not apply there).
template <typename Tag, typename Tag::type M> struct Rob
{
    friend typename Tag::type get(Tag) { return M; }
};
struct IsoParam
{
    typedef double SmIsoCurve::*type;
    friend type get(IsoParam);
};
struct SurfParam
{
    typedef SmSurfParamType SmIsoCurve::*type;
    friend type get(SurfParam);
};
struct FromLeft
{
    typedef SmBoolean SmIsoCurve::*type;
    friend type get(FromLeft);
};
template struct Rob<IsoParam, &SmIsoCurve::m_dIsoParameter>;
template struct Rob<SurfParam, &SmIsoCurve::m_eSurfParam>;
template struct Rob<FromLeft, &SmIsoCurve::m_bFromLeft>;

struct Rd
{
    const double* in;
    int64_t n;
    int64_t at;
    bool bad;
    double num()
    {
        if (at >= n)
        {
            bad = true;
            return 0.0;
        }
        return in[at++];
    }
    int64_t idx() { return (int64_t)num(); }
};

// 0 = SM_SUCCESS, else the SmStatus value.
double stv(SmStatus s)
{
    return s == SM_SUCCESS ? 0.0 : (double)s;
}

double flg(SmBoolean b)
{
    return b ? 1.0 : 0.0;
}

// Reads `n m p q r s cpts u v` into a surface from sm_AllocateNurbSurface.
gw_SURFACE* read_nurb(Rd& rd)
{
    gw_INDEX n = (gw_INDEX)rd.idx();
    gw_INDEX m = (gw_INDEX)rd.idx();
    gw_DEGREE p = (gw_DEGREE)rd.idx();
    gw_DEGREE q = (gw_DEGREE)rd.idx();
    gw_INDEX r = (gw_INDEX)rd.idx();
    gw_INDEX s = (gw_INDEX)rd.idx();
    if (rd.bad || n < 0 || m < 0 || p < 1 || q < 1 || r != n + p + 1 || s != m + q + 1 ||
        rd.n - rd.at < 4 * (int64_t)(n + 1) * (m + 1) + (r + 1) + (s + 1))
    {
        rd.bad = true;
        return NULL;
    }
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

SmBSplineSurface* read_surface(Rd& rd, const SmContext& ctx)
{
    int64_t has = rd.idx();
    if (has == 0)
        return new (ctx) SmBSplineSurface();
    if (has != 1)
    {
        rd.bad = true;
        return NULL;
    }
    gw_SURFACE* sur = read_nurb(rd);
    if (sur == NULL)
        return NULL;
    return new (ctx) SmBSplineSurface(sur, FALSE, NULL);
}

// The curve parameters: surf_param iso from_left clamp.
SmIsoCurve* read_curve(Rd& rd, const SmSurface& srf, const SmContext& ctx)
{
    SmSurfParamType sp = (SmSurfParamType)rd.idx();
    double iso = rd.num();
    SmBoolean fl = rd.idx() ? TRUE : FALSE;
    SmBoolean clamp = rd.idx() ? TRUE : FALSE;
    if (rd.bad)
        return NULL;
    return new (ctx) SmIsoCurve(srf, sp, iso, fl, clamp);
}

void put_state(std::vector<double>& o, const SmIsoCurve& c)
{
    o.push_back(c.*get(IsoParam()));
    o.push_back((double)(c.*get(SurfParam())));
    o.push_back(flg(c.*get(FromLeft())));
    o.push_back((double)c.GetType());
    o.push_back((double)c.GetDim());
    SmExtent1d ivl = c.GetNaturalInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
}

int32_t run(int32_t op, Rd& rd, const uint8_t* data, int64_t data_len, int32_t file_type, int64_t version,
            std::vector<double>& o)
{
    const SmContext& ctx = *SmApiGetOrCreateContext();
    if (op == OP_READ_DB)
    {
        if (data_len < 0 || (file_type != 0 && file_type != 1) || version < 0)
            return SC_ERR_ARG;
        std::istringstream iss(std::string((const char*)data, (size_t)data_len));
        SmDatabaseIOStream db(file_type == 0 ? SM_ASCII : SM_BINARY, &iss);
        SmCurve* p = NULL;
        SmStatus st = SmIsoCurve::ReadFromDB(SmIsoCurve_TYPE, db, 3, ctx, p, (ULONG)version);
        o.push_back(stv(st));
        if (st == SM_SUCCESS && p != NULL)
            put_state(o, *(SmIsoCurve*)p);
        delete p;
        return SC_OK;
    }

    SmBSplineSurface* srf = read_surface(rd, ctx);
    if (srf == NULL)
        return SC_ERR_ARG;
    SmIsoCurve* a = read_curve(rd, *srf, ctx);
    if (a == NULL)
    {
        delete srf;
        return SC_ERR_ARG;
    }
    int32_t rc = SC_OK;
    switch (op)
    {
    case OP_STATE: put_state(o, *a); break;
    case OP_KNOTS:
    {
        bool has_ivl = rd.idx() != 0;
        double lo = rd.num();
        double hi = rd.num();
        if (rd.bad)
        {
            rc = SC_ERR_ARG;
            break;
        }
        SmExtent1d ivl(lo, hi);
        SmTArray<double> knots;
        SmTArray<ULONG> mults;
        SmStatus st = a->GetKnots(knots, &mults, has_ivl ? &ivl : NULL);
        o.push_back(stv(st));
        if (st == SM_SUCCESS)
        {
            o.push_back((double)knots.GetSize());
            for (ULONG i = 0; i < knots.GetSize(); ++i)
                o.push_back(knots[i]);
            o.push_back((double)mults.GetSize());
            for (ULONG i = 0; i < mults.GetSize(); ++i)
                o.push_back((double)mults[i]);
        }
        break;
    }
    case OP_EVALUATE:
    {
        double t = rd.num();
        int64_t n = rd.idx();
        SmBoolean fl = rd.idx() ? TRUE : FALSE;
        SmBoolean nz = rd.idx() ? TRUE : FALSE;
        if (rd.bad || n < 0 || n > 8)
        {
            rc = SC_ERR_ARG;
            break;
        }
        // generous output array: the surface writes its derivative layout into it
        std::vector<SmVector3d> pd(64);
        SmStatus st = a->Evaluate(t, (ULONG)n, fl, pd.data(), nz);
        o.push_back(stv(st));
        if (st == SM_SUCCESS)
            for (int64_t i = 0; i <= n; ++i)
                put_vec(o, pd[(size_t)i]);
        break;
    }
    case OP_EVALUATE_POINT:
    {
        double t = rd.num();
        if (rd.bad)
        {
            rc = SC_ERR_ARG;
            break;
        }
        SmPoint3d p;
        SmStatus st = a->EvaluatePoint(t, p);
        o.push_back(stv(st));
        if (st == SM_SUCCESS)
            put_vec(o, p);
        break;
    }
    case OP_CONTINUITIES:
    {
        double tol = rd.num();
        if (rd.bad)
        {
            rc = SC_ERR_ARG;
            break;
        }
        SmContinuityType eMin = SM_CT_UNDEFINED;
        SmTArray<SmContinuityType> conts;
        conts.Add(SM_CT_C0); // must be cleared by the call
        SmStatus st = a->CalculateContinuities(eMin, conts, tol);
        o.push_back(stv(st));
        if (st == SM_SUCCESS)
        {
            o.push_back((double)eMin);
            o.push_back((double)conts.GetSize());
            for (ULONG i = 0; i < conts.GetSize(); ++i)
                o.push_back((double)conts[i]);
        }
        break;
    }
    case OP_EQ:
    {
        // mode 0: B on its own surface (block follows); 1: A == A; 2: B on A's surface.
        int64_t mode = rd.idx();
        if (mode == 1)
        {
            o.push_back(flg(*a == *a));
            break;
        }
        SmBSplineSurface* srf_b = NULL;
        if (mode == 0)
        {
            srf_b = read_surface(rd, ctx);
            if (srf_b == NULL)
            {
                rc = SC_ERR_ARG;
                break;
            }
        }
        else if (mode != 2)
        {
            rc = SC_ERR_ARG;
            break;
        }
        SmIsoCurve* b = read_curve(rd, srf_b ? *srf_b : *srf, ctx);
        if (b == NULL)
            rc = SC_ERR_ARG;
        else
        {
            o.push_back(flg(*a == *b));
            o.push_back(flg(*b == *a));
            delete b;
        }
        delete srf_b;
        break;
    }
    case OP_IS_KIND_OF:
    {
        int64_t t = rd.idx();
        if (rd.bad)
        {
            rc = SC_ERR_ARG;
            break;
        }
        o.push_back(flg(a->IsKindOf((SM_TYPE)t)));
        break;
    }
    case OP_TYPE_STRINGS:
    {
        o.push_back((double)a->GetType());
        for (const TCHAR* s = a->GetTypeString(); *s; ++s)
            o.push_back((double)*s);
        o.push_back(-1.0);
        for (const TCHAR* s = a->GetClassString(); *s; ++s)
            o.push_back((double)*s);
        break;
    }
    case OP_UV_ISO_LINE:
    {
        SmCurve* line = NULL;
        SmStatus st = a->CreateUVIsoLine(line, &ctx);
        o.push_back(stv(st));
        if (st == SM_SUCCESS && line != NULL)
        {
            o.push_back((double)line->GetType());
            o.push_back((double)line->GetDim());
            SmExtent1d ivl = line->GetNaturalInterval();
            o.push_back(ivl.GetMin());
            o.push_back(ivl.GetMax());
            SmPoint3d p;
            o.push_back(stv(line->EvaluatePoint(ivl.GetMin(), p)));
            put_vec(o, p);
            o.push_back(stv(line->EvaluatePoint(ivl.GetMax(), p)));
            put_vec(o, p);
        }
        delete line;
        break;
    }
    default: rc = SC_ERR_ARG; break;
    }
    delete a;
    delete srf;
    return rc;
}
} // namespace

SC_EXPORT int32_t sc_curve_iso_curve_op(int32_t op, const double* in, int64_t n_in, const uint8_t* data,
                                        int64_t data_len, int32_t file_type, int64_t version, double* out,
                                        int64_t cap, int64_t* out_len)
{
    if (op < 0 || op > OP_LAST || n_in < 0)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, n_in, 0, false};
    std::vector<double> o;
    int32_t rc = run(op, rd, data, data_len, file_type, version, o);
    if (rc != SC_OK)
        return rc;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
