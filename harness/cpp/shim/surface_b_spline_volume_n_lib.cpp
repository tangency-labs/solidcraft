// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.b_spline_volume_n_lib`: the SmBSplineVolume members of
// source/SMLib/src/SmBSplineVolumeNLib.cpp (CalculateContinuitiesSimple,
// HasDiscontinuitiesSimple, IsCurveInParamDomainSimple), called through `SmVolume*` on a
// B-spline volume built with SmBSplineVolume(ctx, const gw_VOLUME*) from a record filled by
// sm_AllocateNurbVolume, optionally oriented by an SmTransform and compounded with SmTransform
// next maps (so the SmVolume compound entry points IsCurveInParamDomain and
// IsCurveInInSpaceDomain reach the override too). The parameter curves are real
// SmBSplineCurve, SmLine, SmEllipse and SmHermiteCurve objects.
//
// One entry point, sc_surface_b_spline_volume_n_lib_eval(op, in, n_in, out, cap, out_len).
// Layouts are documented in crates/solidcore-ffi/src/units/surface_b_spline_volume_n_lib.rs,
// which validates every input. `out` receives the op status (0 = SM_SUCCESS, else the
// SmStatus value) and the op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineVolume.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmEllipse.h>
#include <SmExtent1d.h>
#include <SmExtent3d.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmMemory.h>
#include <SmNurbsCrv.h>
#include <SmNurbsVol.h>
#include <SmTArray.h>
#include <SmTransform.h>
#include <SmVector3d.h>
#include <SmVolume.h>
#include <nurbs.h>

#include <vector>

#include "sc_sm.h"

using sc::Rd;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_b_spline_volume_n_lib.rs).
enum : int32_t
{
    OP_CONTS = 0,
    OP_HAS_DISC = 1,
    OP_CURVE_IN = 2,
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

// `origin X Y Z`: SmTransform(ctx, origin, X, Y, &Z).
SmTransform* read_transform(Rd& rd, const SmContext& ctx)
{
    SmVector3d o = rd.vec();
    SmVector3d x = rd.vec();
    SmVector3d y = rd.vec();
    SmVector3d z = rd.vec();
    return new (ctx) SmTransform(&ctx, o, x, y, &z);
}

// `l m n p q r`, knots U V W, control points (x y z w), w fastest; `has_orient [O(12)]`;
// `n_next` then `T(12)` per next transform.
SmVolume* read_volume(Rd& rd, const SmContext& ctx)
{
    long l = (long)rd.idx(), m = (long)rd.idx(), n = (long)rd.idx();
    gw_DEGREE p = (gw_DEGREE)rd.idx(), q = (gw_DEGREE)rd.idx(), r = (gw_DEGREE)rd.idx();
    long ku = l + p + 1, kv = m + q + 1, kw = n + r + 1;
    gw_VOLUME* rec = sm_AllocateNurbVolume(l, m, n, p, q, r, ku, kv, kw);
    if (rec == NULL)
        return NULL;
    for (long i = 0; i <= ku; i++)
        rec->knu->U[i] = rd.num();
    for (long i = 0; i <= kv; i++)
        rec->knv->U[i] = rd.num();
    for (long i = 0; i <= kw; i++)
        rec->knw->U[i] = rd.num();
    for (long i = 0; i <= l; i++)
        for (long j = 0; j <= m; j++)
            for (long k = 0; k <= n; k++)
            {
                NL_CPOINT& c = rec->mesh->Pw[i][j][k];
                c.x = rd.num();
                c.y = rd.num();
                c.z = rd.num();
                c.w = rd.num();
            }
    SmVolume* v = new (ctx) SmBSplineVolume(&ctx, rec);
    smos_Free(rec);
    if (rd.flag())
        v->SetOrientMap(read_transform(rd, ctx), 2);
    int64_t n_next = rd.idx();
    for (int64_t i = 0; i < n_next; ++i)
        v->AddNextMap(read_transform(rd, ctx), 2);
    return v;
}

// A curve block: `0 n p cpts(x y z w) knots` (SmBSplineCurve(gw_CURVE*, 3, FALSE)),
// `1 P V d0 d1` (bounded SmLine, scale 1), `2 C X Y d0 d1 rx ry` (SmEllipse), or
// `3 P1 D1 P2 D2` (SmHermiteCurve, dimension 3).
SmCurve* read_curve(Rd& rd, const SmContext& ctx)
{
    int64_t kind = rd.idx();
    if (kind == 0)
    {
        gw_INDEX n = (gw_INDEX)rd.idx();
        gw_DEGREE p = (gw_DEGREE)rd.idx();
        gw_INDEX m = n + p + 1;
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
    if (kind == 1)
    {
        SmPoint3d p = rd.vec();
        SmVector3d v = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        return new (ctx) SmLine(p, v, SmExtent1d(d0, d1), 1.0, 3, &ctx, NULL, FALSE);
    }
    if (kind == 2)
    {
        SmPoint3d c = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        double d0 = rd.num();
        double d1 = rd.num();
        double rx = rd.num();
        double ry = rd.num();
        return new (ctx) SmEllipse(c, x, y, SmExtent1d(d0, d1), rx, ry, 3, &ctx, NULL, FALSE);
    }
    SmVector3d p1 = rd.vec();
    SmVector3d d1 = rd.vec();
    SmVector3d p2 = rd.vec();
    SmVector3d d2 = rd.vec();
    return new (ctx) SmHermiteCurve(p1, d1, p2, d2, 3);
}

void put_dir(std::vector<double>& o, SmBoolean unaligned, SmContinuityType min,
             const SmTArray<double>& params, const SmTArray<SmContinuityType>& conts)
{
    o.push_back(flg(unaligned));
    o.push_back((double)min);
    o.push_back((double)params.GetSize());
    for (ULONG i = 0; i < params.GetSize(); ++i)
        o.push_back(params[i]);
    o.push_back((double)conts.GetSize());
    for (ULONG i = 0; i < conts.GetSize(); ++i)
        o.push_back((double)conts[i]);
}

void volume_op(int32_t op, Rd& rd, const SmContext& ctx, SmVolume* v, std::vector<double>& o)
{
    switch (op)
    {
    case OP_CONTS:
    {
        SmVolumeParamType e = (SmVolumeParamType)rd.idx();
        bool geom = rd.flag();
        // outputs holding stale values, to see them reset
        SmContinuityType min = SM_CT_G1;
        SmTArray<double> params;
        SmTArray<SmContinuityType> conts;
        params.Add(-7.0);
        params.Add(9.0);
        conts.Add(SM_CT_C0);
        SmStatus s = v->CalculateContinuitiesSimple(e, min, params, conts, geom ? TRUE : FALSE);
        o.push_back(st(s));
        put_dir(o, FALSE, min, params, conts);
        break;
    }
    case OP_HAS_DISC:
    {
        bool given = rd.flag();
        bool geom = rd.flag();
        SmDiscontinuities3d d;
        d.SetUnaligned();
        SmBoolean r = v->HasDiscontinuitiesSimple(given ? &d : NULL, geom ? TRUE : FALSE);
        o.push_back(0.0);
        o.push_back(flg(r));
        if (given)
        {
            o.push_back(flg(d.m_bHasDiscontinuities));
            o.push_back((double)d.m_eMinContinuity);
            put_dir(o, d.m_bUnalignedU, d.m_eMinContU, d.m_sParamsU, d.m_sContsU);
            put_dir(o, d.m_bUnalignedV, d.m_eMinContV, d.m_sParamsV, d.m_sContsV);
            put_dir(o, d.m_bUnalignedW, d.m_eMinContW, d.m_sParamsW, d.m_sContsW);
        }
        break;
    }
    case OP_CURVE_IN:
    {
        int64_t which = rd.idx();
        bool comp = rd.flag();
        SmCurve* c = read_curve(rd, ctx);
        SmBoolean r = which == 0   ? v->IsCurveInParamDomainSimple(*c)
                      : which == 1 ? v->IsCurveInParamDomain(*c, comp ? TRUE : FALSE)
                                   : v->IsCurveInInSpaceDomain(*c, comp ? TRUE : FALSE);
        delete c;
        o.push_back(0.0);
        o.push_back(flg(r));
        break;
    }
    }
}
} // namespace

SC_EXPORT int32_t sc_surface_b_spline_volume_n_lib_eval(int32_t op, const double* in,
                                                        int64_t n_in, double* out, int64_t cap,
                                                        int64_t* out_len)
{
    if (in == nullptr || n_in < 0 || out_len == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    std::vector<double> o;
    Rd rd{in, 0};
    SmContext ctx;
    SmVolume* v = read_volume(rd, ctx);
    if (v == NULL)
        return SC_ERR_ARG;
    volume_op(op, rd, ctx, v, o);
    delete v;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
