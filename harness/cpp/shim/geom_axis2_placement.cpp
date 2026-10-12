// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.axis2_placement`.
// Wraps upstream SmAxis2Placement (source/SMLib/inc/SmAxis2Placement.h,
// source/SMLib/src/SmAxis2Placement.cpp).
#include "sc_shim.h"

#include <SmAxis2Placement.h>
#include <SmTol.h>
#include <SmVector3d.h>

// How a placement is built (keep in sync with crates/solidcore-ffi/src/units/geom_axis2_placement.rs).
enum ScA2pKind : int32_t
{
    KIND_IDENTITY = 0,   // SmAxis2Placement()
    KIND_ORIENT = 1,     // SmAxis2Placement(v0, v1, v2)
    KIND_CANONICAL = 2,  // SmAxis2Placement().SetCanonical(v0, v1, v2) -> status
    KIND_STEP = 3,       // SmAxis2Placement().SetSTEPCanonical(v0, v1, v2) -> status
    KIND_PLANE = 4,      // SmAxis2Placement(v0, d)
    KIND_FROM_TO = 5,    // SmAxis2Placement(v0, v1, v2, v3, v4, v5)
    KIND_COPY = 6,       // copy constructor of SmAxis2Placement(v0, v1, v2)
    KIND_ROTATED = 7,    // SmAxis2Placement(v0, v1, v2).RotateAboutAxisAtPoint(d, v3, v4)
};

// Operation codes (keep in sync with crates/solidcore-ffi/src/units/geom_axis2_placement.rs).
enum ScA2pOp : int32_t
{
    OP_STATE = 0,        // GetOrigin, GetXAxis, GetYAxis, GetCanonical -> 18 values
    OP_MATRIX = 1,       // GetMatrix -> 16
    OP_MIRROR = 2,       // GetMirrorMatrix -> 16
    OP_TRANSLATE = 3,    // Translate(w0) -> 12
    OP_ROTATE = 4,       // RotateAboutAxis(s, w0) -> 12
    OP_ROTATE_AT = 5,    // RotateAboutAxisAtPoint(s, w0, w1) -> 12
    OP_XFORM_POINT = 6,  // TransformPoint(w0) -> 3
    OP_XFORM_VECTOR = 7, // TransformVector(w0) -> 3
    OP_XFORM_A2P = 8,    // TransformAxis2Placement(B) -> B 13 + 12
    OP_INVERT = 9,       // Invert -> 12
    OP_EQ = 10,          // A == B -> B 13 + 1
    OP_EQ_SELF = 11,     // A == A -> 1
    OP_IS_IDENTITY = 12, // IsIdentity(s) -> 1
    OP_COAXIAL = 13,     // AreCoaxial(B, SmApproxTol3d(s), t) -> B 13 + 1
    OP_CHAIN = 14,       // RotateAboutAxis(s, w0); Translate(w1); RotateAboutAxis(t, w2) -> 12
    OP_INIT = 15,        // Init -> 12
    OP_XFORM_SELF = 16,  // A.TransformAxis2Placement(A, A) (aliased output) -> 12
};

#define SC_A2P_MAX_OUT 40

static SmVector3d sc_vec(const double* v, int i)
{
    return SmVector3d(v[3 * i], v[3 * i + 1], v[3 * i + 2]);
}

// Builds placement `p` of kind `kind` from vectors v[18] (v0..v5) and scalar d;
// returns the status of SetCanonical / SetSTEPCanonical (0 for SM_SUCCESS and for the
// other kinds).
static int32_t sc_build(int32_t kind, const double* v, double d, SmAxis2Placement& p)
{
    switch (kind)
    {
    case KIND_IDENTITY: p = SmAxis2Placement(); return 0;
    case KIND_ORIENT: p = SmAxis2Placement(sc_vec(v, 0), sc_vec(v, 1), sc_vec(v, 2)); return 0;
    case KIND_CANONICAL:
    {
        SmAxis2Placement q;
        SmStatus st = q.SetCanonical(sc_vec(v, 0), sc_vec(v, 1), sc_vec(v, 2));
        p = q;
        return st == SM_SUCCESS ? 0 : (int32_t)st;
    }
    case KIND_STEP:
    {
        SmAxis2Placement q;
        SmStatus st = q.SetSTEPCanonical(sc_vec(v, 0), sc_vec(v, 1), sc_vec(v, 2));
        p = q;
        return st == SM_SUCCESS ? 0 : (int32_t)st;
    }
    case KIND_PLANE: p = SmAxis2Placement(sc_vec(v, 0), d); return 0;
    case KIND_FROM_TO:
        p = SmAxis2Placement(sc_vec(v, 0), sc_vec(v, 1), sc_vec(v, 2), sc_vec(v, 3), sc_vec(v, 4),
                             sc_vec(v, 5));
        return 0;
    case KIND_COPY:
    {
        SmAxis2Placement q(sc_vec(v, 0), sc_vec(v, 1), sc_vec(v, 2));
        SmAxis2Placement r(q);
        p = r;
        return 0;
    }
    case KIND_ROTATED:
    {
        SmAxis2Placement q(sc_vec(v, 0), sc_vec(v, 1), sc_vec(v, 2));
        q.RotateAboutAxisAtPoint(d, sc_vec(v, 3), sc_vec(v, 4));
        p = q;
        return 0;
    }
    default: return -1;
    }
}

// Runs one SmAxis2Placement operation on placement A (kind_a, va[18], da) and, for the
// binary operations, placement B (kind_b, vb[18], db); w[9] holds vector operands w0..w2,
// s and t scalar operands. Writes [status_a, A origin, A x, A y, A z] then the op's
// values to out[SC_A2P_MAX_OUT]; *out_n receives the count. Returns 0 or SC_ERR_ARG.
SC_EXPORT int32_t sc_geom_axis2_placement_op(int32_t op, int32_t kind_a, const double* va,
                                             double da, int32_t kind_b, const double* vb,
                                             double db, const double* w, double s, double t,
                                             double* out, int32_t* out_n)
{
    SC_TRY
    *out_n = 0;
    auto put = [&](double x) { out[(*out_n)++] = x; };
    auto putv = [&](const SmVector3d& r) {
        put(r.x);
        put(r.y);
        put(r.z);
    };
    auto put_state = [&](const SmAxis2Placement& p) {
        putv(p.GetOriginRef());
        putv(p.GetXAxisRef());
        putv(p.GetYAxisRef());
        putv(p.GetZAxis());
    };
    auto put_mat = [&](const SmAxis2Placement& p, bool mirror) {
        double m[4][4];
        double* pm[4] = {m[0], m[1], m[2], m[3]};
        if (mirror)
            p.GetMirrorMatrix(pm);
        else
            p.GetMatrix(pm);
        for (int i = 0; i < 4; ++i)
            for (int j = 0; j < 4; ++j)
                put(m[i][j]);
    };
    auto flg = [](SmBoolean x) { return x ? 1.0 : 0.0; };

    SmAxis2Placement a;
    int32_t st_a = sc_build(kind_a, va, da, a);
    if (st_a < 0)
        return SC_ERR_ARG;
    put((double)st_a);
    put_state(a);

    SmAxis2Placement b;
    auto build_b = [&]() -> bool {
        int32_t st_b = sc_build(kind_b, vb, db, b);
        if (st_b < 0)
            return false;
        put((double)st_b);
        put_state(b);
        return true;
    };

    SmVector3d w0 = sc_vec(w, 0), w1 = sc_vec(w, 1), w2 = sc_vec(w, 2);
    switch (op)
    {
    case OP_STATE:
    {
        putv(a.GetOrigin());
        putv(a.GetXAxis());
        putv(a.GetYAxis());
        SmPoint3d o;
        SmVector3d x, y;
        a.GetCanonical(o, x, y);
        putv(o);
        putv(x);
        putv(y);
        return SC_OK;
    }
    case OP_MATRIX: put_mat(a, false); return SC_OK;
    case OP_MIRROR: put_mat(a, true); return SC_OK;
    case OP_TRANSLATE: a.Translate(w0); put_state(a); return SC_OK;
    case OP_ROTATE: a.RotateAboutAxis(s, w0); put_state(a); return SC_OK;
    case OP_ROTATE_AT: a.RotateAboutAxisAtPoint(s, w0, w1); put_state(a); return SC_OK;
    case OP_XFORM_POINT:
    {
        SmPoint3d r;
        a.TransformPoint(w0, r);
        putv(r);
        return SC_OK;
    }
    case OP_XFORM_VECTOR:
    {
        SmVector3d r;
        a.TransformVector(w0, r);
        putv(r);
        return SC_OK;
    }
    case OP_XFORM_A2P:
    {
        if (!build_b())
            return SC_ERR_ARG;
        SmAxis2Placement r;
        a.TransformAxis2Placement(b, r);
        put_state(r);
        return SC_OK;
    }
    case OP_INVERT:
    {
        SmAxis2Placement r;
        a.Invert(r);
        put_state(r);
        return SC_OK;
    }
    case OP_EQ:
        if (!build_b())
            return SC_ERR_ARG;
        put(flg(a == b));
        return SC_OK;
    case OP_EQ_SELF: put(flg(a == a)); return SC_OK;
    case OP_IS_IDENTITY: put(flg(a.IsIdentity(s))); return SC_OK;
    case OP_COAXIAL:
        if (!build_b())
            return SC_ERR_ARG;
        put(flg(a.AreCoaxial(b, SmApproxTol3d(s), t)));
        return SC_OK;
    case OP_CHAIN:
        a.RotateAboutAxis(s, w0);
        a.Translate(w1);
        a.RotateAboutAxis(t, w2);
        put_state(a);
        return SC_OK;
    case OP_INIT: a.Init(); put_state(a); return SC_OK;
    case OP_XFORM_SELF: a.TransformAxis2Placement(a, a); put_state(a); return SC_OK;
    default: return SC_ERR_ARG;
    }
    SC_CATCH
}
