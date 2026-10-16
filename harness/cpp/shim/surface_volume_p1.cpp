// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `surface.volume_p1`: the SmVolume members owned by the unit
// (source/SMLib/src/SmVolume.cpp, source/SMLib/inc/SmVolume.h), called on compound volumes
// built from plain SmVolume, SmTransform, SmTwistVolume and SmUnbendVolume maps (the last
// two default-constructed; their simple maps are not ported, so the Rust side defers), and
// the file-level smvol_Lift*Derivative / smvol_ConcatEvals functions.
//
// One entry point, sc_surface_volume_p1_eval(op, in, n_in, out, cap, out_len). Layouts are
// documented in crates/solidcore-ffi/src/units/surface_volume_p1.rs, which validates every
// input. `out` receives the op status (0 = SM_SUCCESS, else the SmStatus value) and the op's
// outputs.
//
// SmFindPVIntersectENFO is declared only inside SmVolume.cpp; the identical declaration below
// links against the library's definition of its Evaluate (and its vtable).
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmContext.h>
#include <SmExtent3d.h>
#include <SmLine.h>
#include <SmLocalSolveNd.h>
#include <SmMatrix.h>
#include <SmTArray.h>
#include <SmTransform.h>
#include <SmTwistVolume.h>
#include <SmUnbendVolume.h>
#include <SmVector3d.h>
#include <SmVolume.h>

#include <vector>

#include "sc_sm.h"

using sc::Rd;
using sc::put_vec;

// The class of SmVolume.cpp, declared exactly as there.
class SmFindPVIntersectENFO : public SmEvalNFunctionsObject
{
  protected:
    const SmPoint3d& m_crProjSpacePoint;
    const SmVolume& m_crVolume;
    double m_dSquaredTolerance;

  public:
    SmFindPVIntersectENFO(const SmPoint3d& crProjSpacePoint, const SmVolume& crVolume,
                          double dSquaredTolerance)
        : m_crProjSpacePoint(crProjSpacePoint), m_crVolume(crVolume),
          m_dSquaredTolerance(dSquaredTolerance)
    {
    }
    virtual ~SmFindPVIntersectENFO() {}

    virtual SmStatus Evaluate(const SmTArray<double>& crX, SmTArray<double>& rF,
                              SmMatrix* pOptJacobian, SmBoolean& rbFoundAnswer);
};

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/surface_volume_p1.rs).
enum : int32_t
{
    OP_INFO = 0,
    OP_BOUNDED = 1,
    OP_EVAL = 2,
    OP_MAP = 3,
    OP_MAP_POINT = 4,
    OP_ORIENT = 5,
    OP_ORIENT_POINT = 6,
    OP_ORIENT_DIR = 7,
    OP_ORIENT_VECTOR = 8,
    OP_EVAL_SIMPLE = 9,
    OP_EVAL_POINT_SIMPLE = 10,
    OP_EVAL_1ST_SIMPLE = 11,
    OP_INV_GUESS = 12,
    OP_SIMPLE_PREDS = 13,
    OP_PT_IN_INSPACE = 14,
    OP_LINE_IN_PARAM = 15,
    OP_CURVE_IN_PARAM = 16,
    OP_CURVE_IN_INSPACE = 17,
    OP_TRANSFORM = 18,
    OP_ADD_NEXT = 19,
    OP_COPY = 20,
    OP_EQUAL = 21,
    OP_MAKE_SIMPLE = 22,
    OP_REFRESH = 23,
    OP_TRIM_BOX = 24,
    OP_HAS_DISC = 25,
    OP_PV_EVAL = 26,
    OP_TRANSFORM_SIMPLE = 27,
    OP_LIFT = 40,
    OP_CONCAT = 41,
    OP_DISCONTS = 42,
};

// Map kinds of a volume block.
enum : int64_t
{
    K_BASE = 0,
    K_TRANSFORM = 1,
    K_TWIST = 2,
    K_UNBEND = 3,
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

// `n` maps `kind [T(12)] has_orient [O(12)]`, chained in order with AddNextMap(m, 2).
SmVolume* read_volume(Rd& rd, const SmContext& ctx)
{
    int64_t n = rd.idx();
    SmVolume* head = NULL;
    for (int64_t i = 0; i < n; ++i)
    {
        int64_t kind = rd.idx();
        SmVolume* v = NULL;
        switch (kind)
        {
        case K_TRANSFORM: v = read_transform(rd, ctx); break;
        case K_TWIST: v = new (ctx) SmTwistVolume(&ctx); break;
        case K_UNBEND: v = new (ctx) SmUnbendVolume(&ctx); break;
        default: v = new (ctx) SmVolume(&ctx); break;
        }
        if (rd.flag())
            v->SetOrientMap(read_transform(rd, ctx), 2);
        if (head == NULL)
            head = v;
        else
            head->AddNextMap(v, 2);
    }
    return head;
}

// The chain: `n` then per map `type [T(16)] has_orient [O(16)] has_inv [I(16)]`.
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
        o.push_back(p->GetOrientMap() ? 1.0 : 0.0);
        if (p->GetOrientMap())
            put_mat(o, *p->GetOrientMap());
        o.push_back(p->GetInvOrientMap() ? 1.0 : 0.0);
        if (p->GetInvOrientMap())
            put_mat(o, *p->GetInvOrientMap());
    }
}

void put_cont_dump(std::vector<double>& o, const SmDiscontinuities3d& d)
{
    o.push_back(flg(d.m_bHasDiscontinuities));
    o.push_back((double)d.m_eMinContinuity);
    o.push_back(flg(d.m_bUnalignedU));
    o.push_back((double)d.m_eMinContU);
    o.push_back((double)d.m_sParamsU.GetSize());
    o.push_back((double)d.m_sContsU.GetSize());
    o.push_back(flg(d.m_bUnalignedV));
    o.push_back((double)d.m_eMinContV);
    o.push_back((double)d.m_sParamsV.GetSize());
    o.push_back((double)d.m_sContsV.GetSize());
    o.push_back(flg(d.m_bUnalignedW));
    o.push_back((double)d.m_eMinContW);
    o.push_back((double)d.m_sParamsW.GetSize());
    o.push_back((double)d.m_sContsW.GetSize());
}

int64_t vsize(int64_t n) { return (n + 1) * (n + 1) * (n + 1); }

void put_array(std::vector<double>& o, const SmVector3d* a, int64_t cnt)
{
    for (int64_t i = 0; i < cnt; ++i)
        put_vec(o, a[i]);
}

void read_vecs(Rd& rd, SmVector3d* a, int64_t cnt)
{
    for (int64_t i = 0; i < cnt; ++i)
        a[i] = rd.vec();
}

void lift_op(Rd& rd, std::vector<double>& o)
{
    int64_t form = rd.idx();
    int64_t order = rd.idx();
    o.push_back(0.0);
    SmVector3d c[4], d1[3], d2[6], d3[10], r[10];
    if (form == 0) // curve
    {
        read_vecs(rd, c, order);
        read_vecs(rd, d1, 3);
        if (order >= 2)
            read_vecs(rd, d2, 6);
        if (order >= 3)
            read_vecs(rd, d3, 10);
        if (order == 1)
            r[0] = smvol_LiftFirstDerivative(c[0], d1[0], d1[1], d1[2]);
        else if (order == 2)
            r[0] = smvol_LiftSecondDerivative(c[0], c[1], d1[0], d1[1], d1[2], d2[0], d2[1], d2[2],
                                              d2[3], d2[4], d2[5]);
        else
            r[0] = smvol_LiftThirdDerivative(c[0], c[1], c[2], d1[0], d1[1], d1[2], d2[0], d2[1],
                                             d2[2], d2[3], d2[4], d2[5], d3[0], d3[1], d3[2],
                                             d3[3], d3[4], d3[5], d3[6], d3[7], d3[8], d3[9]);
        put_vec(o, r[0]);
    }
    else if (form == 1) // surface
    {
        SmVector3d s1[2], s2[3], s3[4];
        read_vecs(rd, s1, 2);
        if (order >= 2)
            read_vecs(rd, s2, 3);
        if (order >= 3)
            read_vecs(rd, s3, 4);
        read_vecs(rd, d1, 3);
        if (order >= 2)
            read_vecs(rd, d2, 6);
        if (order >= 3)
            read_vecs(rd, d3, 10);
        if (order == 1)
        {
            smvol_LiftFirstDerivative(s1[0], s1[1], d1[0], d1[1], d1[2], r[0], r[1]);
            put_array(o, r, 2);
        }
        else if (order == 2)
        {
            smvol_LiftSecondDerivative(s1[0], s1[1], s2[0], s2[1], s2[2], d1[0], d1[1], d1[2],
                                       d2[0], d2[1], d2[2], d2[3], d2[4], d2[5], r[0], r[1], r[2]);
            put_array(o, r, 3);
        }
        else
        {
            smvol_LiftThirdDerivative(s1[0], s1[1], s2[0], s2[1], s2[2], s3[0], s3[1], s3[2], s3[3],
                                      d1[0], d1[1], d1[2], d2[0], d2[1], d2[2], d2[3], d2[4], d2[5],
                                      d3[0], d3[1], d3[2], d3[3], d3[4], d3[5], d3[6], d3[7], d3[8],
                                      d3[9], r[0], r[1], r[2], r[3]);
            put_array(o, r, 4);
        }
    }
    else // volume
    {
        SmVector3d f1[3], f2[6], f3[10];
        read_vecs(rd, f1, 3);
        if (order >= 2)
            read_vecs(rd, f2, 6);
        if (order >= 3)
            read_vecs(rd, f3, 10);
        read_vecs(rd, d1, 3);
        if (order >= 2)
            read_vecs(rd, d2, 6);
        if (order >= 3)
            read_vecs(rd, d3, 10);
        if (order == 1)
        {
            smvol_LiftFirstDerivative(f1[0], f1[1], f1[2], d1[0], d1[1], d1[2], r[0], r[1], r[2]);
            put_array(o, r, 3);
        }
        else if (order == 2)
        {
            smvol_LiftSecondDerivative(f1[0], f1[1], f1[2], f2[0], f2[1], f2[2], f2[3], f2[4],
                                       f2[5], d1[0], d1[1], d1[2], d2[0], d2[1], d2[2], d2[3],
                                       d2[4], d2[5], r[0], r[1], r[2], r[3], r[4], r[5]);
            put_array(o, r, 6);
        }
        else
        {
            smvol_LiftThirdDerivative(
                f1[0], f1[1], f1[2], f2[0], f2[1], f2[2], f2[3], f2[4], f2[5], f3[0], f3[1], f3[2],
                f3[3], f3[4], f3[5], f3[6], f3[7], f3[8], f3[9], d1[0], d1[1], d1[2], d2[0], d2[1],
                d2[2], d2[3], d2[4], d2[5], d3[0], d3[1], d3[2], d3[3], d3[4], d3[5], d3[6], d3[7],
                d3[8], d3[9], r[0], r[1], r[2], r[3], r[4], r[5], r[6], r[7], r[8], r[9]);
            put_array(o, r, 10);
        }
    }
}

void static_op(int32_t op, Rd& rd, std::vector<double>& o)
{
    switch (op)
    {
    case OP_LIFT: lift_op(rd, o); break;
    case OP_CONCAT:
    {
        int64_t n = rd.idx();
        SmVector3d a[64], b[64], c[64];
        read_vecs(rd, a, vsize(n));
        read_vecs(rd, b, vsize(n));
        SmStatus s = smvol_ConcatEvals((ULONG)n, a, b, c);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_array(o, c, vsize(n));
        break;
    }
    case OP_DISCONTS:
    {
        o.push_back(0.0);
        SmDiscontinuities3d d;
        put_cont_dump(o, d);
        d.SetUnaligned();
        put_cont_dump(o, d);
        d.Init();
        put_cont_dump(o, d);
        break;
    }
    }
}

void volume_op(int32_t op, Rd& rd, const SmContext& ctx, SmVolume* v, std::vector<double>& o)
{
    SmVector3d buf[216];
    switch (op)
    {
    case OP_INFO:
    {
        o.push_back(0.0);
        o.push_back((double)v->GetType());
        for (const TCHAR* s = v->GetTypeString(); *s; ++s)
            o.push_back((double)*s);
        o.push_back(-1.0);
        for (const TCHAR* s = v->GetClassString(); *s; ++s)
            o.push_back((double)*s);
        o.push_back(-1.0);
        int64_t nt = rd.idx();
        for (int64_t i = 0; i < nt; ++i)
            o.push_back(flg(v->IsKindOf((SM_TYPE)rd.idx())));
        SmVolume* last = v->GetLastMap();
        o.push_back(last ? (double)last->GetType() : -1.0);
        put_vec(o, v->GetOrientOrigin());
        put_vec(o, v->GetOrientXAxis());
        put_vec(o, v->GetOrientYAxis());
        put_vec(o, v->GetOrientZAxis());
        put_state(o, v);
        break;
    }
    case OP_BOUNDED:
    {
        o.push_back(0.0);
        o.push_back(flg(v->IsBoundedSimple()));
        o.push_back(flg(v->IsBounded()));
        SmExtent3d d = v->GetNaturalParamDomain();
        put_vec(o, d.GetMin());
        put_vec(o, d.GetMax());
        break;
    }
    case OP_EVAL:
    case OP_MAP:
    {
        SmVector3d p = rd.vec();
        int64_t n = rd.idx();
        bool l0 = rd.flag(), l1 = rd.flag(), l2 = rd.flag();
        bool nz = rd.flag();
        bool comp = rd.flag();
        SmStatus s = op == OP_EVAL
                         ? v->Evaluate(p, (ULONG)n, l0, l1, l2, buf, nz, TRUE, comp)
                         : v->Map(p, (ULONG)n, l0, l1, l2, buf, nz, TRUE, comp);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_array(o, buf, vsize(n));
        break;
    }
    case OP_MAP_POINT:
    {
        SmVector3d p = rd.vec();
        bool comp = rd.flag();
        SmPoint3d q;
        SmStatus s = v->MapPoint(p, q, comp);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, q);
        break;
    }
    case OP_ORIENT:
    {
        SmVector3d p = rd.vec();
        int64_t n = rd.idx();
        SmStatus s = v->Orient(p, (ULONG)n, TRUE, TRUE, TRUE, buf, TRUE, TRUE);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_array(o, buf, vsize(n));
        break;
    }
    case OP_ORIENT_POINT:
    {
        SmVector3d p = rd.vec();
        SmPoint3d q;
        SmStatus s = v->OrientPoint(p, q);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, q);
        break;
    }
    case OP_ORIENT_DIR:
    {
        SmVector3d p = rd.vec();
        SmVector3d dir = rd.vec();
        int64_t n = rd.idx();
        SmVector3d dd[4];
        SmStatus s = v->OrientDirectionalDerivs(p, dir, (ULONG)n, dd);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_array(o, dd, n + 1);
        break;
    }
    case OP_ORIENT_VECTOR:
    {
        SmVector3d p = rd.vec();
        SmVector3d dir = rd.vec();
        SmVector3d r;
        SmStatus s = v->OrientVector(p, dir, r);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, r);
        break;
    }
    case OP_EVAL_SIMPLE:
    {
        SmVector3d p = rd.vec();
        int64_t n = rd.idx();
        SmStatus s = v->EvaluateSimple(p, (ULONG)n, TRUE, TRUE, TRUE, buf);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_array(o, buf, vsize(n));
        break;
    }
    case OP_EVAL_POINT_SIMPLE:
    {
        SmVector3d p = rd.vec();
        SmPoint3d q;
        SmStatus s = v->EvaluatePointSimple(p, q);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_vec(o, q);
        break;
    }
    case OP_EVAL_1ST_SIMPLE:
    {
        SmVector3d p = rd.vec();
        bool l0 = rd.flag(), l1 = rd.flag(), l2 = rd.flag();
        bool nz = rd.flag();
        SmPoint3d q;
        SmVector3d du, dv, dw;
        SmStatus s = v->Evaluate1stDerivativesSimple(p, l0, l1, l2, q, du, dv, dw, nz);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            put_vec(o, q);
            put_vec(o, du);
            put_vec(o, dv);
            put_vec(o, dw);
        }
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
    case OP_SIMPLE_PREDS:
    {
        SmVector3d p = rd.vec();
        SmVector3d a = rd.vec();
        SmVector3d b = rd.vec();
        o.push_back(0.0);
        o.push_back(flg(v->IsPointInParamDomainSimple(p)));
        o.push_back(flg(v->IsLineInParamDomainSimple(a, b)));
        SmLine line(a, b, 3, &ctx);
        o.push_back(flg(v->IsCurveInParamDomainSimple(line)));
        break;
    }
    case OP_PT_IN_INSPACE:
    {
        SmVector3d p = rd.vec();
        bool comp = rd.flag();
        SmPoint3d q;
        SmBoolean in = v->IsPointInInSpaceDomain(p, &q, comp);
        o.push_back(0.0);
        o.push_back(flg(in));
        put_vec(o, q);
        break;
    }
    case OP_LINE_IN_PARAM:
    case OP_CURVE_IN_PARAM:
    case OP_CURVE_IN_INSPACE:
    {
        SmVector3d a = rd.vec();
        SmVector3d b = rd.vec();
        bool comp = rd.flag();
        SmBoolean in = FALSE;
        if (op == OP_LINE_IN_PARAM)
            in = v->IsLineInParamDomain(a, b, comp);
        else
        {
            SmLine line(a, b, 3, &ctx);
            in = op == OP_CURVE_IN_PARAM ? v->IsCurveInParamDomain(line, comp)
                                         : v->IsCurveInInSpaceDomain(line, comp);
        }
        o.push_back(0.0);
        o.push_back(flg(in));
        break;
    }
    case OP_TRANSFORM:
    case OP_TRANSFORM_SIMPLE:
    {
        SmVector3d org = rd.vec();
        SmVector3d x = rd.vec();
        SmVector3d y = rd.vec();
        bool has_scale = rd.flag();
        SmVector3d sc = rd.vec();
        SmAxis2Placement ax(org, x, y);
        SmStatus s = op == OP_TRANSFORM ? v->Transform(ax, has_scale ? &sc : NULL)
                                        : v->TransformSimple(ax, has_scale ? &sc : NULL);
        o.push_back(st(s));
        put_state(o, v);
        break;
    }
    case OP_ADD_NEXT:
    {
        SmVolume* other = read_volume(rd, ctx);
        bool copy = rd.flag();
        SmStatus s = v->AddNextMap(other, copy ? 1 : 2);
        if (copy)
            delete other;
        o.push_back(st(s));
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
        SmVolume* other = read_volume(rd, ctx);
        bool self_cmp = rd.flag();
        o.push_back(0.0);
        o.push_back(flg(self_cmp ? (*v == *v) : (*v == *other)));
        delete other;
        break;
    }
    case OP_MAKE_SIMPLE:
    {
        SmStatus s = v->MakeMapSimple();
        o.push_back(st(s));
        put_state(o, v);
        break;
    }
    case OP_REFRESH:
    {
        SmStatus s = v->RefreshInvOrientMap();
        o.push_back(st(s));
        put_state(o, v);
        break;
    }
    case OP_TRIM_BOX:
    {
        SmVector3d a = rd.vec();
        SmVector3d b = rd.vec();
        SmVector3d c = rd.vec();
        SmVector3d d = rd.vec();
        SmExtent3d box(a, b);
        SmExtent3d trim(c, d);
        SmStatus s = v->TrimParamBoundingBoxSimple(box, trim);
        o.push_back(st(s));
        put_vec(o, trim.GetMin());
        put_vec(o, trim.GetMax());
        break;
    }
    case OP_HAS_DISC:
    {
        SmDiscontinuities3d d;
        d.SetUnaligned();
        SmBoolean b = v->HasDiscontinuitiesSimple(&d);
        o.push_back(0.0);
        o.push_back(flg(b));
        put_cont_dump(o, d);
        break;
    }
    case OP_PV_EVAL:
    {
        SmVector3d p = rd.vec();
        double sq = rd.num();
        SmVector3d x3 = rd.vec();
        bool want_jac = rd.flag();
        SmTArray<double> x, f;
        x.SetSize(3);
        f.SetSize(3);
        x[0] = x3.x;
        x[1] = x3.y;
        x[2] = x3.z;
        f[0] = f[1] = f[2] = 0.0;
        SmMatrix jac(3, 3);
        for (int i = 0; i < 3; ++i)
            for (int j = 0; j < 3; ++j)
                jac[i][j] = 0.0;
        SmBoolean found = FALSE;
        SmFindPVIntersectENFO e(p, *v, sq);
        SmStatus s = e.Evaluate(x, f, want_jac ? &jac : NULL, found);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back(flg(found));
            o.push_back(f[0]);
            o.push_back(f[1]);
            o.push_back(f[2]);
            if (want_jac)
                for (int i = 0; i < 3; ++i)
                    for (int j = 0; j < 3; ++j)
                        o.push_back(jac[i][j]);
        }
        break;
    }
    }
}
} // namespace

SC_EXPORT int32_t sc_surface_volume_p1_eval(int32_t op, const double* in, int64_t n_in, double* out,
                                            int64_t cap, int64_t* out_len)
{
    if (in == nullptr || n_in < 0 || out_len == nullptr)
        return SC_ERR_ARG;
    SC_TRY
    std::vector<double> o;
    Rd rd{in, 0};
    SmContext ctx;
    if (op >= OP_LIFT)
        static_op(op, rd, o);
    else
    {
        SmVolume* v = read_volume(rd, ctx);
        if (v == NULL)
            return SC_ERR_ARG;
        volume_op(op, rd, ctx, v, o);
        delete v;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
