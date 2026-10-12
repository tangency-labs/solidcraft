// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.vector3d`.
// Wraps upstream SmVector3d (source/SMLib/inc/SmVector3d.h, source/SMLib/src/SmVector3d.cpp)
// and sm_sqrt / sm_cbrt / sm_acos / sm_asin (source/SMLib/src/SmMath.cpp).
#include "sc_shim.h"

#include <SmMath.h>
#include <SmVector2d.h>
#include <SmVector3d.h>

// Operation codes (keep in sync with crates/solidcore-ffi/src/units/geom_vector3d.rs).
enum ScVec3Op : int32_t
{
    OP_LENGTH_SQUARED = 0,     // a.LengthSquared()
    OP_LENGTH = 1,             // a.Length()
    OP_DOT = 2,                // a.Dot(b)
    OP_CROSS = 3,              // a * b
    OP_NEG = 4,                // -a
    OP_ADD = 5,                // a + b
    OP_SUB = 6,                // a - b
    OP_MUL = 7,                // a * s
    OP_LMUL = 8,               // s * a
    OP_DIV = 9,                // a / s
    OP_UNITIZE = 10,           // a.Unitize() (status or vector)
    OP_UNITIZE_OR_Z = 11,      // a.Unitize(FALSE), status ignored
    OP_TRIPLE = 12,            // a.TripleProduct(b, c)
    OP_NORMAL_IN_PLANE = 13,   // a.NormalInPlane(b)
    OP_MAX_DIM = 14,           // a.GetMaxDimension()
    OP_MIN_DIM = 15,           // a.GetMinDimension()
    OP_PARALLEL = 16,          // a.IsParallelTo(b, s, &anti) -> [ret, anti or -1]
    OP_PERPENDICULAR = 17,     // a.IsPerpendicularTo(b, s)
    OP_PROJ = 18,              // a.ProjectToPlane(b)
    OP_PROJ_PT = 19,           // a.ProjectPointToPlane(b, c)
    OP_ROT_PROJ = 20,          // a.RotateProjectToPlane(b, c)
    OP_ROT_PROJ_PT = 21,       // a.RotateProjectPointToPlane(b, c, d)
    OP_PERSP_PROJ = 22,        // a.PerspectiveProjectToPlane(b, c, d, e)
    OP_PERSP_PROJ_PT = 23,     // a.PerspectiveProjectPointToPlane(b, c, d)
    OP_DISTANCE = 24,          // a.DistanceBetween(b)
    OP_DISTANCE_SQ = 25,       // a.DistanceBetweenSquared(b)
    OP_CLOSER_THAN = 26,       // a.CloserThan(s, b)
    OP_MULTIPLY = 27,          // a.Multiply(b)
    OP_DERIV_CROSS = 28,       // a.DerivativeOfCrossProduct(b, c, d)
    OP_SOLVE2 = 29,            // a.SolveTwoLinearEquations(b, out) (status or [x, y])
    OP_INDEX = 30,             // a[idx] (const)
    OP_INDEX_SET = 31,         // a[idx] = s (non-const)
    OP_EQ = 32,                // a == b
    OP_ADD_ASSIGN = 33,        // a += b
    OP_SUB_ASSIGN = 34,        // a -= b
    OP_MUL_ASSIGN = 35,        // a *= s
    OP_DIV_ASSIGN = 36,        // a /= s
    OP_INSIDE_SECTOR = 37,     // a.IsInsideSector(b, c, d)
    OP_ORTHO = 38,             // a.MakeUnitOrthoVectors(flag ? &b : NULL, x, y, z)
    OP_ROTATE_AXIS = 39,       // a.RotateVecAboutAxis(b, s)
    OP_UNIT_DERIV = 40,        // a.UnitizedDerivative(b)
    OP_UNIT_DERIV2 = 41,       // a.UnitizedDerivative2(b, c, o1, o2) -> [failed, o1, o2]
    OP_UNIT_DERIV3 = 42,       // a.UnitizedDerivative3(b, c, d, o1, o2, o3) -> [failed, o1, o2, o3]
    OP_ANGLE = 43,             // a.AngleBetween(b, ang)
    OP_CCW_ANGLE = 44,         // a.CCWAngleBetween(b, c, ang)
    OP_COLINEAR = 45,          // a.IsColinearWith(b, c, d)
    OP_IS_INITIALIZED = 46,    // a.IsInitialized()
    OP_IS_UNDEF = 47,          // a.IsUndef()
    OP_IS_ZERO = 48,           // a.IsZero(s)
    OP_IS_2D = 49,             // a.Is2d()
    OP_FROM_NL = 50,           // SmVector3d({a.x, a.y, a.z, s}, true)
    OP_BETWEEN = 51,           // SmVector3d(a, b)
    OP_FROM_VEC2 = 52,         // SmVector3d(SmVector2d(a.x, a.y))
    OP_COPY = 53,              // SmVector3d(double*), copy ctor, operator=, Copy, Set, Swap
    OP_UNINITIALIZED = 54,     // a.SetUninitialized()
    OP_SQRT = 55,              // sm_sqrt(s)
    OP_CBRT = 56,              // sm_cbrt(s)
    OP_ACOS = 57,              // sm_acos(s)
    OP_ASIN = 58,              // sm_asin(s)
};

#define SC_V3_MAX_OUT 10

// Runs one SmVector3d operation.
//   v[15]   : five vector operands a, b, c, d, e (x, y, z each)
//   s       : scalar operand
//   idx     : index operand (OP_INDEX: 0..5, OP_INDEX_SET: 0..2; checked by the caller)
//   flag    : OP_ORTHO: nonzero passes &b as the Y reference, else NULL
//   out[10] : result values; *out_n receives how many were written
// Returns 0, the upstream SmStatus of a failed call, or SC_ERR_ARG.
SC_EXPORT int32_t sc_geom_vector3d_op(int32_t op, const double* v, double s, int32_t idx,
                                      int32_t flag, double* out, int32_t* out_n)
{
    SC_TRY
    SmVector3d a(v[0], v[1], v[2]);
    SmVector3d b(v[3], v[4], v[5]);
    SmVector3d c(v[6], v[7], v[8]);
    SmVector3d d(v[9], v[10], v[11]);
    SmVector3d e(v[12], v[13], v[14]);
    *out_n = 0;
    auto put = [&](double x) { out[(*out_n)++] = x; };
    auto put1 = [&](double x) {
        put(x);
        return (int32_t)SC_OK;
    };
    auto putv = [&](const SmVector3d& r) {
        put(r.x);
        put(r.y);
        put(r.z);
    };
    auto put3 = [&](const SmVector3d& r) {
        putv(r);
        return (int32_t)SC_OK;
    };
    auto flg = [](SmBoolean x) { return x ? 1.0 : 0.0; };
    switch (op)
    {
    case OP_LENGTH_SQUARED: return put1(a.LengthSquared());
    case OP_LENGTH: return put1(a.Length());
    case OP_DOT: return put1(a.Dot(b));
    case OP_CROSS: return put3(a * b);
    case OP_NEG: return put3(-a);
    case OP_ADD: return put3(a + b);
    case OP_SUB: return put3(a - b);
    case OP_MUL: return put3(a * s);
    case OP_LMUL: return put3(s * a);
    case OP_DIV: return put3(a / s);
    case OP_UNITIZE:
    {
        SmStatus st = a.Unitize();
        if (st != SM_SUCCESS)
            return (int32_t)st;
        return put3(a);
    }
    case OP_UNITIZE_OR_Z: a.Unitize(FALSE); return put3(a);
    case OP_TRIPLE: return put1(a.TripleProduct(b, c));
    case OP_NORMAL_IN_PLANE: return put3(a.NormalInPlane(b));
    case OP_MAX_DIM: return put1(a.GetMaxDimension());
    case OP_MIN_DIM: return put1(a.GetMinDimension());
    case OP_PARALLEL:
    {
        SmBoolean anti = 7;
        SmBoolean r = a.IsParallelTo(b, s, &anti);
        put(flg(r));
        return put1(anti == 7 ? -1.0 : flg(anti));
    }
    case OP_PERPENDICULAR: return put1(flg(a.IsPerpendicularTo(b, s)));
    case OP_PROJ: return put3(a.ProjectToPlane(b));
    case OP_PROJ_PT: return put3(a.ProjectPointToPlane(b, c));
    case OP_ROT_PROJ: return put3(a.RotateProjectToPlane(b, c));
    case OP_ROT_PROJ_PT: return put3(a.RotateProjectPointToPlane(b, c, d));
    case OP_PERSP_PROJ: return put3(a.PerspectiveProjectToPlane(b, c, d, e));
    case OP_PERSP_PROJ_PT: return put3(a.PerspectiveProjectPointToPlane(b, c, d));
    case OP_DISTANCE: return put1(a.DistanceBetween(b));
    case OP_DISTANCE_SQ: return put1(a.DistanceBetweenSquared(b));
    case OP_CLOSER_THAN: return put1(flg(a.CloserThan(s, b)));
    case OP_MULTIPLY: return put3(a.Multiply(b));
    case OP_DERIV_CROSS: return put3(a.DerivativeOfCrossProduct(b, c, d));
    case OP_SOLVE2:
    {
        SmPoint2d sol(0.0, 0.0);
        SmStatus st = a.SolveTwoLinearEquations(b, sol);
        if (st != SM_SUCCESS)
            return (int32_t)st;
        put(sol.x);
        return put1(sol.y);
    }
    case OP_INDEX:
    {
        if (idx < 0 || idx > 5)
            return SC_ERR_ARG;
        const SmVector3d& ca = a;
        return put1(ca[(ULONG)idx]);
    }
    case OP_INDEX_SET:
        if (idx < 0 || idx > 2)
            return SC_ERR_ARG;
        a[(ULONG)idx] = s;
        return put3(a);
    case OP_EQ: return put1((a == b) ? 1.0 : 0.0);
    case OP_ADD_ASSIGN: a += b; return put3(a);
    case OP_SUB_ASSIGN: a -= b; return put3(a);
    case OP_MUL_ASSIGN: a *= s; return put3(a);
    case OP_DIV_ASSIGN: a /= s; return put3(a);
    case OP_INSIDE_SECTOR: return put1(flg(a.IsInsideSector(b, c, d)));
    case OP_ORTHO:
    {
        SmVector3d x, y, z;
        SmStatus st = a.MakeUnitOrthoVectors(flag ? &b : NULL, x, y, z);
        if (st != SM_SUCCESS)
            return (int32_t)st;
        putv(x);
        putv(y);
        return put3(z);
    }
    case OP_ROTATE_AXIS: return put3(a.RotateVecAboutAxis(b, s));
    case OP_UNIT_DERIV: return put3(a.UnitizedDerivative(b));
    case OP_UNIT_DERIV2:
    {
        SmVector3d o1(0, 0, 0), o2(0, 0, 0);
        SmStatus st = a.UnitizedDerivative2(b, c, o1, o2);
        put(st == SM_SUCCESS ? 0.0 : (double)st);
        putv(o1);
        return put3(o2);
    }
    case OP_UNIT_DERIV3:
    {
        SmVector3d o1(0, 0, 0), o2(0, 0, 0), o3(0, 0, 0);
        SmStatus st = a.UnitizedDerivative3(b, c, d, o1, o2, o3);
        put(st == SM_SUCCESS ? 0.0 : (double)st);
        putv(o1);
        putv(o2);
        return put3(o3);
    }
    case OP_ANGLE:
    {
        double ang = -1.0;
        SmStatus st = a.AngleBetween(b, ang);
        if (st != SM_SUCCESS)
            return (int32_t)st;
        return put1(ang);
    }
    case OP_CCW_ANGLE:
    {
        double ang = -1.0;
        SmStatus st = a.CCWAngleBetween(b, c, ang);
        if (st != SM_SUCCESS)
            return (int32_t)st;
        return put1(ang);
    }
    case OP_COLINEAR: return put1(flg(a.IsColinearWith(b, c, d)));
    case OP_IS_INITIALIZED: return put1(flg(a.IsInitialized()));
    case OP_IS_UNDEF: return put1(flg(a.IsUndef()));
    case OP_IS_ZERO: return put1(flg(a.IsZero(s)));
    case OP_IS_2D: return put1(flg(a.Is2d()));
    case OP_FROM_NL:
    {
        double p[4] = {v[0], v[1], v[2], s};
        SmVector3d r(p, true);
        return put3(r);
    }
    case OP_BETWEEN:
    {
        SmVector3d r(a, b);
        return put3(r);
    }
    case OP_FROM_VEC2:
    {
        SmVector2d v2(v[0], v[1]);
        SmVector3d r(v2);
        return put3(r);
    }
    case OP_COPY:
    {
        double arr[3] = {v[0], v[1], v[2]};
        SmVector3d c1(arr);
        SmVector3d c2(c1);
        SmVector3d c3;
        c3 = c2;
        SmVector3d c4;
        c4.Copy(c3);
        SmVector3d c5(0, 0, 0);
        c5.Set(c4.x, c4.y, c4.z);
        SmVector3d c6(1, 2, 3);
        c6.Swap(c5);
        return put3(c6);
    }
    case OP_UNINITIALIZED: a.SetUninitialized(); return put3(a);
    case OP_SQRT: return put1(sm_sqrt(s));
    case OP_CBRT: return put1(sm_cbrt(s));
    case OP_ACOS: return put1(sm_acos(s));
    case OP_ASIN: return put1(sm_asin(s));
    default: return SC_ERR_ARG;
    }
    SC_CATCH
}
