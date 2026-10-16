// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.b_spline_volume`: the SmBSplineVolume members owned by the
// unit (source/SMLib/src/SmBSplineVolume.cpp, source/SMLib/inc/SmBSplineVolume.h), called on a
// B-spline volume built with SmBSplineVolume(ctx, const gw_VOLUME*) from a record filled by
// sm_AllocateNurbVolume, optionally oriented by an SmTransform and compounded with SmTransform
// next maps; plus the SmVolume base members that dispatch to its overrides.
//
// One entry point, sc_surface_b_spline_volume_eval(op, in, n_in, out, cap, out_len). Layouts
// are documented in crates/solidcore-ffi/src/units/surface_b_spline_volume.rs, which validates
// every input. `out` receives the op status (0 = SM_SUCCESS, else the SmStatus value) and the
// op's outputs.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineVolume.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmExtent1d.h>
#include <SmExtent3d.h>
#include <SmMemory.h>
#include <SmNurbsVol.h>
#include <SmPseudoBox.h>
#include <SmTArray.h>
#include <SmTransform.h>
#include <SmVector3d.h>
#include <SmVolume.h>
#include <nurbs.h>

#include <vector>

#include "sc_sm.h"

using sc::Rd;
using sc::put_vec;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_b_spline_volume.rs).
enum : int32_t
{
    OP_INFO = 0,
    OP_COPY = 1,
    OP_EQUAL = 2,
    OP_EVAL = 3,
    OP_BBOX = 4,
    OP_ISO = 5,
    OP_INV_GUESS = 6,
    OP_TRANSFORM = 7,
    OP_PREDS = 8,
};

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double flg(SmBoolean b) { return b ? 1.0 : 0.0; }

void put_mat(std::vector<double>& o, const SmTransform& t)
{
    SmTransform& m = (SmTransform&)t;
    for (int i = 0; i < 4; ++i)
        for (int j = 0; j < 4; ++j)
            o.push_back(m[i][j]);
}

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

// The record: knots U V W and the control points (counts follow from the input).
void put_nurb(std::vector<double>& o, const SmBSplineVolume* b)
{
    const gw_VOLUME* rec = b->GetGwNurbPointer();
    for (long i = 0; i <= rec->knu->m; i++)
        o.push_back(rec->knu->U[i]);
    for (long i = 0; i <= rec->knv->m; i++)
        o.push_back(rec->knv->U[i]);
    for (long i = 0; i <= rec->knw->m; i++)
        o.push_back(rec->knw->U[i]);
    for (long i = 0; i <= rec->mesh->m; i++)
        for (long j = 0; j <= rec->mesh->n; j++)
            for (long k = 0; k <= rec->mesh->o; k++)
            {
                const NL_CPOINT& c = rec->mesh->Pw[i][j][k];
                o.push_back(c.x);
                o.push_back(c.y);
                o.push_back(c.z);
                o.push_back(c.w);
            }
}

// The chain: `n` then per map `type [T(16) | nurb] has_orient [O(16)] has_inv [I(16)]`.
void put_state(std::vector<double>& o, const SmVolume* v)
{
    int64_t n = 0;
    for (const SmVolume* p = v; p; p = p->GetNextMap())
        ++n;
    o.push_back((double)n);
    for (const SmVolume* p = v; p; p = p->GetNextMap())
    {
        o.push_back((double)p->GetType());
        if (p->GetType() == SmTransform_TYPE)
            put_mat(o, *(const SmTransform*)p);
        else if (p->GetType() == SmBSplineVolume_TYPE)
            put_nurb(o, (const SmBSplineVolume*)p);
        o.push_back(p->GetOrientMap() ? 1.0 : 0.0);
        if (p->GetOrientMap())
            put_mat(o, *p->GetOrientMap());
        o.push_back(p->GetInvOrientMap() ? 1.0 : 0.0);
        if (p->GetInvOrientMap())
            put_mat(o, *p->GetInvOrientMap());
    }
}

void put_knots(std::vector<double>& o, const SmBSplineVolume* b, SmVolumeParamType e,
               const SmExtent1d* ivl)
{
    SmTArray<double> k;
    SmTArray<ULONG> m;
    SmStatus s = b->GetKnots(e, k, &m, ivl);
    o.push_back(st(s));
    o.push_back((double)k.GetSize());
    for (ULONG i = 0; i < k.GetSize(); ++i)
        o.push_back(k[i]);
    o.push_back((double)m.GetSize());
    for (ULONG i = 0; i < m.GetSize(); ++i)
        o.push_back((double)m[i]);
}

void volume_op(int32_t op, Rd& rd, const SmContext& ctx, SmVolume* v, std::vector<double>& o)
{
    SmBSplineVolume* b = (SmBSplineVolume*)v;
    SmVector3d buf[64];
    switch (op)
    {
    case OP_INFO:
    {
        o.push_back(0.0);
        o.push_back((double)v->GetType());
        int64_t nt = rd.idx();
        for (int64_t i = 0; i < nt; ++i)
            o.push_back(flg(v->IsKindOf((SM_TYPE)rd.idx())));
        o.push_back(flg(v->IsBoundedSimple()));
        o.push_back(flg(v->IsBounded()));
        for (int e = 0; e <= (int)SM_VP_NONE; ++e)
            o.push_back((double)b->GetDegree((SmVolumeParamType)e));
        SmExtent3d d = v->GetNaturalParamDomain();
        put_vec(o, d.GetMin());
        put_vec(o, d.GetMax());
        for (int e = 0; e <= (int)SM_VP_NONE; ++e)
            put_knots(o, b, (SmVolumeParamType)e, NULL);
        double a = rd.num();
        double c = rd.num();
        SmExtent1d ivl(a, c);
        put_knots(o, b, SM_VP_U, &ivl);
        put_knots(o, b, SM_VP_V, &ivl);
        put_knots(o, b, SM_VP_W, &ivl);
        put_state(o, v);
        break;
    }
    case OP_COPY:
    {
        bool simple = rd.flag();
        SmVolume* c = NULL;
        SmStatus s = v->Copy(ctx, c, simple);
        o.push_back(st(s));
        if (s == SM_SUCCESS && c)
            put_state(o, c);
        delete c;
        break;
    }
    case OP_EQUAL:
    {
        int64_t mode = rd.idx();
        o.push_back(0.0);
        if (mode == 0)
            o.push_back(flg(*v == *v));
        else if (mode == 1)
        {
            SmVolume* c = NULL;
            v->Copy(ctx, c, FALSE);
            o.push_back(c ? flg(*v == *c) : -1.0);
            delete c;
        }
        else
        {
            SmVolume* other = read_volume(rd, ctx);
            o.push_back(other ? flg(*v == *other) : -1.0);
            delete other;
        }
        break;
    }
    case OP_EVAL:
    {
        SmVector3d p = rd.vec();
        int64_t n = rd.idx();
        bool comp = rd.flag();
        o.push_back(0.0);
        SmStatus s = v->EvaluateSimple(p, (ULONG)n, TRUE, TRUE, TRUE, buf);
        o.push_back(st(s));
        s = v->Evaluate(p, (ULONG)n, TRUE, TRUE, TRUE, buf, TRUE, TRUE, comp);
        o.push_back(st(s));
        SmPoint3d q;
        s = v->MapPoint(p, q, comp);
        o.push_back(st(s));
        break;
    }
    case OP_BBOX:
    {
        SmVector3d a = rd.vec();
        SmVector3d c = rd.vec();
        bool want_box = rd.flag();
        bool want_pb = rd.flag();
        SmExtent3d box(a, c);
        SmExtent3d proj;
        SmPseudoBox pb;
        SmStatus s = b->EvaluateBoundingBoxSimple(box, NULL, want_box ? &proj : NULL,
                                                  want_pb ? &pb : NULL);
        o.push_back(st(s));
        if (want_box)
        {
            put_vec(o, proj.GetMin());
            put_vec(o, proj.GetMax());
        }
        if (want_pb)
        {
            for (ULONG i = 0; i < 3; ++i)
                put_vec(o, pb.GetBasis(i));
            for (ULONG i = 0; i < 3; ++i)
            {
                o.push_back(pb.GetInterval(i).GetMin());
                o.push_back(pb.GetInterval(i).GetMax());
            }
        }
        break;
    }
    case OP_ISO:
    {
        int64_t kind = rd.idx();
        double p1 = rd.num();
        double p2 = rd.num();
        SmCurve* crv = NULL;
        SmStatus s = b->EvaluateIsoParametricCurveSimple(ctx, (SmVolumeParamsType)kind, p1, p2,
                                                         1.0e-3, crv, NULL);
        o.push_back(st(s));
        o.push_back(crv ? 1.0 : 0.0);
        delete crv;
        break;
    }
    case OP_INV_GUESS:
    {
        SmVector3d p = rd.vec();
        SmTArray<SmPoint3d> g;
        SmStatus s = v->InvEvaluateGuessPointSimple(p, g);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)g.GetSize());
            for (ULONG i = 0; i < g.GetSize(); ++i)
                put_vec(o, g[i]);
        }
        break;
    }
    case OP_TRANSFORM:
    {
        SmVector3d org = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        bool has_scale = rd.flag();
        SmVector3d sc = rd.vec();
        bool simple = rd.flag();
        SmAxis2Placement ax(org, x, y);
        SmStatus s = simple ? v->TransformSimple(ax, has_scale ? &sc : NULL)
                            : v->Transform(ax, has_scale ? &sc : NULL);
        o.push_back(st(s));
        put_state(o, v);
        break;
    }
    case OP_PREDS:
    {
        SmVector3d p = rd.vec();
        SmVector3d a = rd.vec();
        SmVector3d c = rd.vec();
        SmVector3d d = rd.vec();
        SmVector3d e = rd.vec();
        o.push_back(0.0);
        o.push_back(flg(v->IsPointInParamDomainSimple(p)));
        o.push_back(flg(v->IsLineInParamDomainSimple(a, c)));
        SmExtent3d box(a, c);
        SmExtent3d trim(d, e);
        SmStatus s = v->TrimParamBoundingBoxSimple(box, trim);
        o.push_back(st(s));
        put_vec(o, trim.GetMin());
        put_vec(o, trim.GetMax());
        break;
    }
    }
}
} // namespace

SC_EXPORT int32_t sc_surface_b_spline_volume_eval(int32_t op, const double* in, int64_t n_in,
                                                  double* out, int64_t cap, int64_t* out_len)
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
