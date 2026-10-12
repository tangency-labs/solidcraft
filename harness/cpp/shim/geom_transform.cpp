// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.transform`.
// Wraps upstream SmTransform (source/SMLib/inc/SmTransform.h, source/SMLib/src/SmTransform.cpp).
#include "sc_shim.h"

#include <SmApiGeneral.h>
#include <SmAxis2Placement.h>
#include <SmDatabaseIO.h>
#include <SmExtent3d.h>
#include <SmLine.h>
#include <SmPseudoBox.h>
#include <SmSolutionArray.h>
#include <SmTransform.h>
#include <SmVector3d.h>
#include <SmVolume.h>

#include <sstream>
#include <string>
#include <vector>

// How transform A / B is built from vectors v0..v7 and flag `zf` (keep in sync with
// crates/solidcore-ffi/src/units/geom_transform.rs).
enum ScTrKind : int32_t
{
    TK_IDENTITY = 0,     // SmTransform()
    TK_CANONICAL = 1,    // SmTransform(NULL, v0, v1, v2, zf ? &v3 : NULL)
    TK_SETTERS = 2,      // SmTransform(); SetToXAxis(v1 doubles); SetToYAxis(v2); SetToZAxis(v3); SetToDisp(v0)
    TK_AXIS2 = 3,        // SmTransform().SetAxis2Placement(SmAxis2Placement(v0, v1, v2))
    TK_COPY = 4,         // copy constructor of TK_CANONICAL
    TK_COPY_VIRTUAL = 5, // TK_CANONICAL.Copy(ctx, pNew)
    TK_CONCAT = 6,       // TK_CANONICAL, then ConcatTransform(SmTransform(NULL, v4, v5, v6, zf ? &v7 : NULL))
    TK_RESET = 7,        // TK_SETTERS, then SetCanonical(v4, v5, v6, zf ? &v7 : NULL)
    TK_LAST = 7,
};

// Operations (keep in sync with crates/solidcore-ffi/src/units/geom_transform.rs).
enum ScTrOp : int32_t
{
    OP_STATE = 0,
    OP_TRANSFORM_VECTOR = 1,
    OP_INV_POINT = 2,
    OP_INV_VECTOR = 3,
    OP_CONCAT = 4,
    OP_SCALE = 5,
    OP_TRANSFORM_SIMPLE = 6,
    OP_INVERT = 7,
    OP_EVALUATE = 8,
    OP_GUESS = 9,
    OP_GLOBAL_SOLVE = 10,
    OP_LOCAL_SOLVE = 11,
    OP_EQ = 12,
    OP_EQ_SELF = 13,
    OP_IS_KIND_OF = 14,
    OP_BBOX = 15,
    OP_READ_DB = 16,
    OP_HAS_DISCONTS = 17,
    OP_TYPE_STRINGS = 18,
    OP_INIT = 19,
    OP_SET_A2P = 20,
    OP_SET_CANONICAL = 21,
    OP_LAST = 21,
};

#define SC_TR_MAX_OUT 1024

static SmVector3d sc_vec(const double* v, int i)
{
    return SmVector3d(v[3 * i], v[3 * i + 1], v[3 * i + 2]);
}

static void sc_build(int32_t kind, const double* v, int32_t zf, SmTransform& t)
{
    SmVector3d z3 = sc_vec(v, 3), z7 = sc_vec(v, 7);
    switch (kind)
    {
    case TK_IDENTITY: t = SmTransform(); break;
    case TK_CANONICAL: t = SmTransform(NULL, sc_vec(v, 0), sc_vec(v, 1), sc_vec(v, 2), zf ? &z3 : NULL); break;
    case TK_SETTERS:
    case TK_RESET:
    {
        SmTransform s;
        s.SetToXAxis(v[3], v[4], v[5]);
        s.SetToYAxis(v[6], v[7], v[8]);
        s.SetToZAxis(v[9], v[10], v[11]);
        s.SetToDisp(v[0], v[1], v[2]);
        if (kind == TK_RESET)
            s.SetCanonical(sc_vec(v, 4), sc_vec(v, 5), sc_vec(v, 6), zf ? &z7 : NULL);
        t = s;
        break;
    }
    case TK_AXIS2:
    {
        SmTransform s;
        s.SetAxis2Placement(SmAxis2Placement(sc_vec(v, 0), sc_vec(v, 1), sc_vec(v, 2)));
        t = s;
        break;
    }
    case TK_COPY:
    {
        SmTransform s(NULL, sc_vec(v, 0), sc_vec(v, 1), sc_vec(v, 2), zf ? &z3 : NULL);
        SmTransform c(s, FALSE);
        t = c;
        break;
    }
    case TK_COPY_VIRTUAL:
    {
        SmTransform s(NULL, sc_vec(v, 0), sc_vec(v, 1), sc_vec(v, 2), zf ? &z3 : NULL);
        SmVolume* p = NULL;
        s.Copy(*SmApiGetOrCreateContext(), p, FALSE);
        t = *(SmTransform*)p;
        delete p;
        break;
    }
    case TK_CONCAT:
    {
        SmTransform s(NULL, sc_vec(v, 0), sc_vec(v, 1), sc_vec(v, 2), zf ? &z3 : NULL);
        SmTransform o(NULL, sc_vec(v, 4), sc_vec(v, 5), sc_vec(v, 6), zf ? &z7 : NULL);
        s.ConcatTransform(o);
        t = s;
        break;
    }
    default: break;
    }
}

// Runs one SmTransform operation on transform A (kind_a, va[24], zf_a) and, for the
// binary ones, B. w[15] holds vector operands w0..w4, `flag` and `n` integer operands.
// Writes the op's values to out[SC_TR_MAX_OUT] (*out_n receives the count). Returns 0 or
// SC_ERR_ARG.
SC_EXPORT int32_t sc_geom_transform_op(int32_t op, int32_t kind_a, const double* va, int32_t zf_a,
                                       int32_t kind_b, const double* vb, int32_t zf_b,
                                       const double* w, int32_t flag, int64_t n,
                                       const uint8_t* data, int64_t data_len, int32_t file_type,
                                       double* out, int32_t* out_n)
{
    if (op < 0 || op > OP_LAST || kind_a < 0 || kind_a > TK_LAST || kind_b < 0 || kind_b > TK_LAST)
        return SC_ERR_ARG;
    SC_TRY
    *out_n = 0;
    auto put = [&](double x) {
        if (*out_n < SC_TR_MAX_OUT)
            out[(*out_n)++] = x;
    };
    auto putv = [&](const SmVector3d& r) {
        put(r.x);
        put(r.y);
        put(r.z);
    };
    auto put_mat = [&](SmTransform& t) {
        for (int i = 0; i < 4; ++i)
            for (int j = 0; j < 4; ++j)
                put(t[i][j]);
    };
    auto st_code = [](SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; };
    auto flg = [](SmBoolean x) { return x ? 1.0 : 0.0; };

    SmTransform a;
    sc_build(kind_a, va, zf_a, a);
    SmTransform b;
    auto build_b = [&]() {
        sc_build(kind_b, vb, zf_b, b);
        put_mat(b);
    };
    SmVector3d w0 = sc_vec(w, 0), w1 = sc_vec(w, 1), w2 = sc_vec(w, 2), w3 = sc_vec(w, 3);

    switch (op)
    {
    case OP_STATE:
    {
        put_mat(a);
        putv(a.GetToOrigin());
        putv(a.GetToXAxis());
        putv(a.GetToYAxis());
        putv(a.GetToZAxis());
        putv(a.GetToDisp());
        SmExtent3d d = a.GetNaturalParamDomain();
        put(d.GetUMin());
        put(d.GetVMin());
        put(d.GetWMin());
        put(d.GetUMax());
        put(d.GetVMax());
        put(d.GetWMax());
        put(flg(a.IsBoundedSimple()));
        SmLine line;
        put(flg(a.IsCurveInParamDomainSimple(line)));
        put(flg(a.HasDiscontinuitiesSimple(NULL, FALSE)));
        put((double)a.GetType());
        break;
    }
    case OP_TRANSFORM_VECTOR:
    {
        SmVector3d r;
        a.TransformVector(w0, r);
        putv(r);
        break;
    }
    case OP_INV_POINT:
    case OP_INV_VECTOR:
    {
        SmVector3d r(0, 0, 0);
        SmStatus st = op == OP_INV_POINT ? a.InvTransformPoint(w0, r) : a.InvTransformVector(w0, r);
        put(st_code(st));
        if (st == SM_SUCCESS)
            putv(r);
        break;
    }
    case OP_CONCAT:
        build_b();
        put(st_code(a.ConcatTransform(b)));
        put_mat(a);
        break;
    case OP_SCALE:
        put(st_code(a.ScaleSimple(w0, flag ? &w1 : NULL)));
        put_mat(a);
        break;
    case OP_TRANSFORM_SIMPLE:
    {
        SmAxis2Placement p(w0, w1, w2);
        put(st_code(a.TransformSimple(p, flag ? &w3 : NULL)));
        put_mat(a);
        break;
    }
    case OP_INVERT:
    {
        SmStatus st = a.InvertSimple();
        put(st_code(st));
        put_mat(a);
        break;
    }
    case OP_EVALUATE:
    {
        if (n < 0 || n > 7)
            return SC_ERR_ARG;
        ULONG k = (ULONG)n;
        std::vector<SmVector3d> d((size_t)((k + 1) * (k + 1) * (k + 1)), SmVector3d(0, 0, 0));
        SmStatus st = a.EvaluateSimple(w0, k, TRUE, TRUE, TRUE, d.data(), TRUE, TRUE);
        put(st_code(st));
        if (st == SM_SUCCESS)
            for (const SmVector3d& x : d)
                putv(x);
        break;
    }
    case OP_GUESS:
    {
        SmTArray<SmPoint3d> g;
        SmStatus st = a.InvEvaluateGuessPointSimple(w0, g);
        put(st_code(st));
        if (st == SM_SUCCESS)
        {
            put((double)g.GetSize());
            putv(g[0]);
        }
        break;
    }
    case OP_GLOBAL_SOLVE:
    {
        if (n < 0 || n > 4)
            return SC_ERR_ARG;
        SmSolutionArray sols;
        for (int64_t i = 0; i < n; ++i)
        {
            SmSolution s;
            s.m_vEnd.m_dSolutionValue = 7.0 + (double)i;
            s.m_vStart.m_adParameters[3] = 3.0 + (double)i;
            sols.Add(s);
        }
        SmBoolean found = FALSE;
        SmStatus st = a.GlobalPointSolveSimple(w0, found, sols, NULL);
        put(st_code(st));
        put(flg(found));
        put((double)sols.GetSize());
        for (ULONG i = 0; i < sols.GetSize(); ++i)
        {
            const SmSolution& s = sols[i];
            put((double)s.m_eSolutionType);
            put((double)s.m_lNumVariables);
            put(s.m_vStart.m_dSolutionValue);
            for (int j = 0; j < 4; ++j)
                put(s.m_vStart.m_adParameters[j]);
            put(s.m_vEnd.m_dSolutionValue);
            put((double)s.m_lNumObjects);
            put(s.m_apObjects[0] == NULL ? 0.0 : (s.m_apObjects[0] == (SmObject*)&a ? 1.0 : 2.0));
        }
        break;
    }
    case OP_LOCAL_SOLVE:
    {
        SmSolution s;
        SmBoolean found = FALSE;
        SmStatus st = a.LocalPointSolveSimple(w0, w1, found, s, NULL);
        put(st_code(st));
        if (st == SM_SUCCESS)
        {
            put(flg(found));
            put((double)s.m_eSolutionType);
            put((double)s.m_lNumVariables);
            put(s.m_vStart.m_dSolutionValue);
            for (int j = 0; j < 4; ++j)
                put(s.m_vStart.m_adParameters[j]);
            put((double)s.m_lNumObjects);
            put(s.m_apObjects[0] == NULL ? 0.0 : (s.m_apObjects[0] == (SmObject*)&a ? 1.0 : 2.0));
        }
        break;
    }
    case OP_EQ:
        build_b();
        put(flg(a == b));
        break;
    case OP_EQ_SELF:
    {
        SmTransform c(a, FALSE);
        put(flg(a == c));
        break;
    }
    case OP_IS_KIND_OF: put(flg(a.IsKindOf((SM_TYPE)n))); break;
    case OP_BBOX:
    {
        // n: 0 = no outputs; 1 = ProjBox with a default param pseudo box;
        //    2 = ProjPseudoBox with a default param pseudo box.
        if (n < 0 || n > 2)
            return SC_ERR_ARG;
        SmExtent3d box(w0.x, w0.y, w0.z, w1.x, w1.y, w1.z);
        SmPseudoBox param_pb, proj_pb;
        SmExtent3d proj_box;
        SmStatus st = a.EvaluateBoundingBoxSimple(box, n == 0 ? NULL : &param_pb, n == 1 ? &proj_box : NULL,
                                                  n == 2 ? &proj_pb : NULL);
        put(st_code(st));
        if (n == 1)
        {
            put(proj_box.GetUMin());
            put(proj_box.GetVMin());
            put(proj_box.GetWMin());
            put(proj_box.GetUMax());
            put(proj_box.GetVMax());
            put(proj_box.GetWMax());
        }
        break;
    }
    case OP_READ_DB:
    {
        if (data_len < 0 || (file_type != 0 && file_type != 1))
            return SC_ERR_ARG;
        std::istringstream iss(std::string((const char*)data, (size_t)data_len));
        SmDatabaseIOStream db((SmFileType)file_type, &iss);
        SmVolume* p = NULL;
        SmStatus st = SmTransform::ReadFromDB(SmTransform_TYPE, db, *SmApiGetOrCreateContext(), p, 30);
        put(st_code(st));
        if (p != NULL)
        {
            put_mat(*(SmTransform*)p);
            delete p;
        }
        break;
    }
    case OP_HAS_DISCONTS:
    {
        SmDiscontinuities3d disc;
        put(flg(a.HasDiscontinuitiesSimple(&disc, flag ? TRUE : FALSE)));
        break;
    }
    case OP_TYPE_STRINGS:
    {
        put((double)a.GetType());
        for (const TCHAR* s = a.GetTypeString(); *s; ++s)
            put((double)*s);
        put(-1.0);
        for (const TCHAR* s = a.GetClassString(); *s; ++s)
            put((double)*s);
        break;
    }
    case OP_INIT:
        a.Init();
        put_mat(a);
        break;
    case OP_SET_A2P:
        a.SetAxis2Placement(SmAxis2Placement(w0, w1, w2));
        put_mat(a);
        break;
    case OP_SET_CANONICAL:
        a.SetCanonical(w0, w1, w2, flag ? &w3 : NULL);
        put_mat(a);
        break;
    default: return SC_ERR_ARG;
    }
    return SC_OK;
    SC_CATCH
}
