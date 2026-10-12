// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.geom_utility_p2`.
// Wraps the smgu_* functions of source/SMLib/src/SmGeomUtility.cpp owned by the unit
// (see crates/solidcore-ffi/src/units/geom_geom_utility_p2.rs for the layouts).
#include "sc_shim.h"

#include <SmGeomUtility.h>
#include <SmMath.h>
#include <SmTArray.h>
#include <SmTypes.h>
#include <SmVector2d.h>
#include <SmVector3d.h>

// Operation codes (keep in sync with crates/solidcore-ffi/src/units/geom_geom_utility_p2.rs).
enum ScGu2Op : int32_t
{
    OP_TRIANGLE_POINT_DISTANCE = 0,
    OP_LINE_LINE_AT_DISTANCE = 1,
    OP_IS_POINT_ON_SEGMENT = 2,
    OP_SEG_SEG_PROJECTED = 3,
    OP_SEG_SEG_PERSPECTIVE = 4,
    OP_SEG_SEG_INTERSECT = 5,
    OP_SEG_SEG_CLOSEST = 6,
    OP_CIRCLE_PLANE = 7,
    OP_LINE_ELLIPSE = 8,
    OP_SPHERE_CIRCLES = 9,
    OP_TWO_PLANES = 10,
    OP_BEST_FIT_PLANE = 11,
    OP_MULT_4X4 = 12,
    OP_INVERSE_4X4 = 13,
    OP_FRENET = 14,
    OP_CONTINUITY = 15,
};

// Runs one operation. `in` holds the op's flat inputs (lengths checked by the Rust
// wrapper), `ival` an integer argument. Writes out[0] = upstream status (0 for
// SM_SUCCESS and for functions without one) followed by the op's outputs; *out_n = values written.
SC_EXPORT int32_t sc_geom_geom_utility_p2_op(int32_t op, const double* in, int64_t n_in,
                                             int64_t ival, double* out, int64_t cap,
                                             int64_t* out_n)
{
    if (cap < 64 || n_in < 0)
        return SC_ERR_ARG;
    SC_TRY
    *out_n = 0;
    auto put = [&](double x) { out[(*out_n)++] = x; };
    auto putv = [&](const SmVector3d& r) {
        put(r.x);
        put(r.y);
        put(r.z);
    };
    // upstream status, SM_SUCCESS (1000) reported as 0
    auto stat = [](SmStatus st) { return st == SM_SUCCESS ? 0.0 : (double)st; };
    auto vec = [&](int64_t i) { return SmVector3d(in[i], in[i + 1], in[i + 2]); };
    auto hits = [&](SmStatus st, ULONG n, const double* a, const double* b, const double* d) {
        put(stat(st));
        put((double)n);
        for (int k = 0; k < 2; ++k)
            put(a[k]);
        for (int k = 0; k < 2; ++k)
            put(b[k]);
        for (int k = 0; k < 2; ++k)
            put(d[k]);
        return (int32_t)SC_OK;
    };

    switch (op)
    {
    case OP_TRIANGLE_POINT_DISTANCE:
    {
        double dist = 0.0, u = 0.0, v = 0.0;
        SmPoint3d closest(0.0, 0.0, 0.0);
        SmStatus st = smgu_TrianglePointDistance(vec(0), vec(3), vec(6), vec(9), dist, closest, u, v);
        put(stat(st));
        put(dist);
        putv(closest);
        put(u);
        put(v);
        return SC_OK;
    }
    case OP_LINE_LINE_AT_DISTANCE:
    {
        double a[4] = {0, 0, 0, 0}, b[4] = {0, 0, 0, 0};
        SmStatus st = smgu_LineLineAtDistance(vec(0), vec(3), vec(6), vec(9), in[12], a, b);
        put(stat(st));
        for (int k = 0; k < 4; ++k)
            put(a[k]);
        for (int k = 0; k < 4; ++k)
            put(b[k]);
        return SC_OK;
    }
    case OP_IS_POINT_ON_SEGMENT:
    {
        double dist = 0.0, param = 0.0;
        SmBoolean on = smgu_IsPointOnSegment(vec(0), vec(3), vec(6), in[9], dist, param);
        put(0.0);
        put(on ? 1.0 : 0.0);
        put(dist);
        put(param);
        return SC_OK;
    }
    case OP_SEG_SEG_PROJECTED:
    {
        ULONG n = 0;
        double a[2] = {0, 0}, b[2] = {0, 0}, d[2] = {0, 0};
        SmStatus st = smgu_SegmentSegmentProjectedIntersect(vec(0), vec(3), vec(6), vec(9), vec(12),
                                                            in[15], n, a, b, d);
        return hits(st, n, a, b, d);
    }
    case OP_SEG_SEG_PERSPECTIVE:
    {
        ULONG n = 0;
        double a[2] = {0, 0}, b[2] = {0, 0}, d[2] = {0, 0};
        SmStatus st = smgu_SegmentSegmentPerpspectiveIntersect(
            vec(0), vec(3), vec(6), vec(9), vec(12), vec(15), vec(18), in[21], n, a, b, d);
        return hits(st, n, a, b, d);
    }
    case OP_SEG_SEG_INTERSECT:
    {
        ULONG n = 0;
        SmPoint3d pts[2] = {SmPoint3d(0, 0, 0), SmPoint3d(0, 0, 0)};
        SmStatus st = smgu_SegmentSegmentIntersect(vec(0), vec(3), vec(6), vec(9), in[12], n, pts);
        put(stat(st));
        put((double)n);
        for (ULONG k = 0; k < n && k < 2; ++k)
            putv(pts[k]);
        return SC_OK;
    }
    case OP_SEG_SEG_CLOSEST:
    {
        ULONG n = 0;
        double a[2] = {0, 0}, b[2] = {0, 0}, d[2] = {0, 0};
        SmStatus st = smgu_SegmentSegmentClosestPoint(vec(0), vec(3), vec(6), vec(9), in[12], n, a, b, d);
        return hits(st, n, a, b, d);
    }
    case OP_CIRCLE_PLANE:
    {
        // in: tol, radius, origin, circle normal, plane point, plane normal
        ULONG n = 0;
        SmPoint3d pts[2] = {SmPoint3d(0, 0, 0), SmPoint3d(0, 0, 0)};
        SmStatus st = smgu_CirclePlaneIntersect(in[0], in[1], vec(2), vec(5), vec(8), vec(11), n, pts);
        put(stat(st));
        put((double)n);
        for (ULONG k = 0; k < n && k < 2; ++k)
            putv(pts[k]);
        return SC_OK;
    }
    case OP_LINE_ELLIPSE:
    {
        // in: line pnt, line vec, center, x axis, y axis, rx, ry, tol
        ULONG n = 0;
        double t[2] = {0, 0};
        SmStatus st = smgu_LineCoPlanarEllipseIntersect(vec(0), vec(3), vec(6), vec(9), in[15], vec(12),
                                                        in[16], in[17], n, t);
        put(stat(st));
        put((double)n);
        for (ULONG k = 0; k < n && k < 2; ++k)
            put(t[k]);
        return SC_OK;
    }
    case OP_SPHERE_CIRCLES:
    {
        // in: sphere center, radius, c1, n1, c2, n2, tol
        ULONG n = 0;
        SmPoint3d pts[2] = {SmPoint3d(0, 0, 0), SmPoint3d(0, 0, 0)};
        SmStatus st = smgu_SphereCircleSphereCircleIntersect(vec(0), in[3], vec(4), vec(7), vec(10),
                                                             vec(13), in[16], n, pts);
        put(stat(st));
        put((double)n);
        for (ULONG k = 0; k < n && k < 2; ++k)
            putv(pts[k]);
        return SC_OK;
    }
    case OP_TWO_PLANES:
    {
        SmPoint3d p(0, 0, 0);
        SmVector3d v(0, 0, 0);
        SmBoolean coincident = 7;
        SmStatus st = smgu_IntersectTwoPlanes(vec(0), vec(3), vec(6), vec(9), p, v, &coincident);
        put(stat(st));
        put(coincident == 7 ? -1.0 : (coincident ? 1.0 : 0.0));
        putv(p);
        putv(v);
        return SC_OK;
    }
    case OP_BEST_FIT_PLANE:
    {
        // in: points (3 per point), tol
        if (n_in < 1 || (n_in - 1) % 3 != 0)
            return SC_ERR_ARG;
        SmTArray<SmPoint3d> pts;
        for (int64_t i = 0; i + 3 <= n_in - 1; i += 3)
            pts.Add(vec(i));
        SmPointSetType type = SM_PST_UNKNOWN;
        SmVector3d p(0, 0, 0), nrm(0, 0, 0);
        double gap = 0.0, cgap = 0.0;
        SmStatus st = smgu_BestFitPlaneToPoints(pts, in[n_in - 1], type, p, nrm, gap, cgap);
        put(stat(st));
        put((double)type);
        putv(p);
        putv(nrm);
        put(gap);
        put(cgap);
        return SC_OK;
    }
    case OP_MULT_4X4:
    case OP_INVERSE_4X4:
    {
        double a[4][4], b[4][4], r[4][4];
        for (int i = 0; i < 4; ++i)
            for (int j = 0; j < 4; ++j)
            {
                a[i][j] = in[4 * i + j];
                b[i][j] = op == OP_MULT_4X4 ? in[16 + 4 * i + j] : 0.0;
                r[i][j] = 0.0;
            }
        SmStatus st = SM_SUCCESS;
        if (op == OP_MULT_4X4)
            smgu_4x4Mult(a, b, r);
        else
            st = smgu_4x4Inverse(a, r);
        put(stat(st));
        for (int i = 0; i < 4; ++i)
            for (int j = 0; j < 4; ++j)
                put(r[i][j]);
        return SC_OK;
    }
    case OP_FRENET:
    {
        // in: 4 derivative vectors; ival = number of geometric vectors (0..3)
        if (ival < 0 || ival > 3)
            return SC_ERR_ARG;
        SmVector3d vals[4] = {vec(0), vec(3), vec(6), vec(9)};
        SmVector3d frame[4] = {SmVector3d(0, 0, 0), SmVector3d(0, 0, 0), SmVector3d(0, 0, 0),
                               SmVector3d(0, 0, 0)};
        SmStatus st = smgu_FrenetFromDerivatives(vals, (ULONG)ival, frame);
        put(stat(st));
        for (int k = 0; k <= ival; ++k)
            putv(frame[k]);
        return SC_OK;
    }
    case OP_CONTINUITY:
    {
        // in: pos1, vec1, 2nd1, pos2, vec2, 2nd2, angle deg, 3rd1, 3rd2
        // ival bit 0: 3rd1 given, bit 1: 3rd2 given
        SmPoint3d p1 = vec(0), p2 = vec(9);
        SmVector3d v1 = vec(3), s1 = vec(6), v2 = vec(12), s2 = vec(15);
        SmVector3d t1 = vec(19), t2 = vec(22);
        SmContinuityType ct = SM_CT_UNDEFINED;
        SmStatus st = smgu_EvaluateCurveContinuity(p1, v1, s1, p2, v2, s2, ct, in[18],
                                                   (ival & 1) ? &t1 : NULL,
                                                   (ival & 2) ? &t2 : NULL);
        put(stat(st));
        put((double)ct);
        return SC_OK;
    }
    default: return SC_ERR_ARG;
    }
    SC_CATCH
}
