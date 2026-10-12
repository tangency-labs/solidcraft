// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.vector2d`.
// Wraps upstream SmVector2d (source/SMLib/inc/SmVector2d.h, source/SMLib/src/SmVector2d.cpp).
#include "sc_shim.h"

#include <SmVector2d.h>
#include <SmVector3d.h>

// Operation codes (keep in sync with crates/solidcore-ffi/src/units/geom_vector2d.rs).
enum ScVec2Op : int32_t
{
    OP_LENGTH_SQUARED = 0,  // a.LengthSquared()
    OP_LENGTH = 1,          // a.Length()
    OP_DOT = 2,             // a.Dot(b)
    OP_NEG = 3,             // -a
    OP_ADD = 4,             // a + b
    OP_SUB = 5,             // a - b
    OP_MUL = 6,             // a * s
    OP_LMUL = 7,            // s * a
    OP_DIV = 8,             // a / s
    OP_UNITIZE = 9,         // a.Unitize()
    OP_DISTANCE = 10,       // a.DistanceBetween(b)
    OP_DISTANCE_SQ = 11,    // a.DistanceBetweenSquared(b)
    OP_CLOSER_THAN = 12,    // a.CloserThan(s, b)
    OP_MAX_DIMENSION = 13,  // a.GetMaxDimension()
    OP_SWAP_XY = 14,        // a.SwapXY()
    OP_ROTATE = 15,         // a.Rotate(s)
    OP_EQ = 16,             // a == b
    OP_NE = 17,             // a != b
    OP_INDEX = 18,          // a[idx] (const)
    OP_INDEX_SET = 19,      // a[idx] = s (non-const)
    OP_ADD_ASSIGN = 20,     // a += b
    OP_SUB_ASSIGN = 21,     // a -= b
    OP_MUL_ASSIGN = 22,     // a *= s
    OP_ANGLE = 23,          // a.AngleBetween(b, ang)
    OP_CCW_ANGLE = 24,      // a.CCWAngleBetween(b, ang)
    OP_IS_ZERO = 25,        // a.IsZero(s)
    OP_IS_INITIALIZED = 26, // a.IsInitialized()
    OP_FROM_VEC3 = 27,      // v2 = SmVector3d(a.x, a.y, s)
    OP_UNINITIALIZED = 28,  // a.SetUninitialized()
    OP_COPY = 29,           // SmVector2d(double*), copy ctor, operator=, Copy, Set
};

// Runs one SmVector2d operation.
//   a[2], b[2] : vector operands (x, y)
//   s          : scalar operand
//   idx        : index operand (OP_INDEX / OP_INDEX_SET; caller guarantees 0 or 1)
//   out[2]     : result values; *out_n receives how many were written (1 or 2)
// Returns 0, the upstream SmStatus of a failed call, or SC_ERR_ARG.
SC_EXPORT int32_t sc_geom_vector2d_op(int32_t op, const double* a, const double* b, double s,
                                      int32_t idx, double* out, int32_t* out_n)
{
    SC_TRY
    SmVector2d va(a[0], a[1]);
    SmVector2d vb(b[0], b[1]);
    SmVector2d r;
    *out_n = 0;
    auto put1 = [&](double v) {
        out[0] = v;
        *out_n = 1;
        return (int32_t)SC_OK;
    };
    auto put2 = [&](const SmVector2d& v) {
        out[0] = v.x;
        out[1] = v.y;
        *out_n = 2;
        return (int32_t)SC_OK;
    };
    switch (op)
    {
    case OP_LENGTH_SQUARED: return put1(va.LengthSquared());
    case OP_LENGTH: return put1(va.Length());
    case OP_DOT: return put1(va.Dot(vb));
    case OP_NEG: return put2(-va);
    case OP_ADD: return put2(va + vb);
    case OP_SUB: return put2(va - vb);
    case OP_MUL: return put2(va * s);
    case OP_LMUL: return put2(s * va);
    case OP_DIV: return put2(va / s);
    case OP_UNITIZE:
    {
        SmStatus st = va.Unitize();
        if (st != SM_SUCCESS)
            return (int32_t)st;
        return put2(va);
    }
    case OP_DISTANCE: return put1(va.DistanceBetween(vb));
    case OP_DISTANCE_SQ: return put1(va.DistanceBetweenSquared(vb));
    case OP_CLOSER_THAN: return put1(va.CloserThan(s, vb) ? 1.0 : 0.0);
    case OP_MAX_DIMENSION: return put1(va.GetMaxDimension());
    case OP_SWAP_XY: va.SwapXY(); return put2(va);
    case OP_ROTATE: return put2(va.Rotate(s));
    case OP_EQ: return put1((va == vb) ? 1.0 : 0.0);
    case OP_NE: return put1((va != vb) ? 1.0 : 0.0);
    case OP_INDEX:
    {
        if (idx != 0 && idx != 1)
            return SC_ERR_ARG;
        const SmVector2d& cva = va;
        return put1(cva[(ULONG)idx]);
    }
    case OP_INDEX_SET:
        if (idx != 0 && idx != 1)
            return SC_ERR_ARG;
        va[(ULONG)idx] = s;
        return put2(va);
    case OP_ADD_ASSIGN: va += vb; return put2(va);
    case OP_SUB_ASSIGN: va -= vb; return put2(va);
    case OP_MUL_ASSIGN: va *= s; return put2(va);
    case OP_ANGLE:
    case OP_CCW_ANGLE:
    {
        double ang = -1.0;
        SmStatus st = (op == OP_ANGLE) ? va.AngleBetween(vb, ang) : va.CCWAngleBetween(vb, ang);
        if (st != SM_SUCCESS)
            return (int32_t)st;
        return put1(ang);
    }
    case OP_IS_ZERO: return put1(va.IsZero(s) ? 1.0 : 0.0);
    case OP_IS_INITIALIZED: return put1(va.IsInitialized() ? 1.0 : 0.0);
    case OP_FROM_VEC3:
    {
        SmVector3d v3(a[0], a[1], s);
        r = v3;
        return put2(r);
    }
    case OP_UNINITIALIZED: va.SetUninitialized(); return put2(va);
    case OP_COPY:
    {
        double arr[2] = {a[0], a[1]};
        SmVector2d c1(arr);
        SmVector2d c2(c1);
        SmVector2d c3;
        c3 = c2;
        SmVector2d c4;
        c4.Copy(c3);
        r.Set(c4.x, c4.y);
        return put2(r);
    }
    default: return SC_ERR_ARG;
    }
    SC_CATCH
}
