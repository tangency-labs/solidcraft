// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.geom_utility_p1`.
// Wraps the exported smgu_* functions of upstream source/SMLib/src/SmGeomUtility.cpp
// owned by the unit (the static helpers sm_AddPoint / sm_AddClosestPoint are wrapped
// in geom_geom_utility_p1_static.cpp).
//
// One entry point, sc_geom_geom_utility_p1_call(op, in, n_in, out, cap, out_len): `in`
// holds the op's arguments as doubles (layouts below, validated by the Rust wrapper
// crates/solidcore-ffi/src/units/geom_geom_utility_p1.rs); `out` receives
// [status, values...] where status is the upstream SmStatus (SM_SUCCESS = 1000; for
// functions returning a value instead of a status it is 1000). When status is not
// SM_SUCCESS only the status is written (upstream leaves its outputs unset).
#include "sc_shim.h"

#include <SmAxis2Placement.h>
#include <SmExtent1d.h>
#include <SmGeomUtility.h>
#include <SmMatrix.h>
#include <SmTArray.h>
#include <SmVector2d.h>
#include <SmVector3d.h>

#include <cmath>
#include <vector>

// Operation codes (keep in sync with crates/solidcore-ffi/src/units/geom_geom_utility_p1.rs).
enum ScGuP1Op : int32_t
{
    OP_POLYGON_AREA = 2,          // [n3, pts...] -> [st, area]
    OP_TRIANGLE_AREA_SQ = 3,      // [p0, p1, p2] -> [st, a2]
    OP_TRIANGLE_DECOMP = 4,       // [base, left, right] -> [st, along, height]
    OP_PARALLELOGRAM = 5,         // [base3, side3, dtb, dbs] -> [st, v1, v2]
    OP_LINE_POINT_DIST = 6,       // [lp, lv, tp] -> [st, dist, param]
    OP_LINE_LINE_CLOSEST = 7,     // [p1, v1, p2, v2] -> [st, t1, t2]
    OP_LINE_PLANE = 8,            // [lp, lv, pp, pn] -> [st, t]
    OP_LINE_PLANE_UV = 9,         // [lp, lv, pp, su, sv] -> [st, t, u, v]
    OP_LINE_CLOSEST = 10,         // [lp, lv, tp] -> [st, t]
    OP_SEGMENT_POINT_DIST = 11,   // [s, e, tp] -> [st, dist, param]
    OP_SEGMENT_ANGLE = 12,        // [s, e, tp] -> [1000, angle, param]
    OP_CIRCLE_CENTER_3PTS = 13,   // [a2, b2, c2] -> [st, ox, oy]
    OP_CIRCLE_3PTS = 14,          // [sp, mp, ep, dim, closed] -> [st, c, x, y, dmin, dmax, r]
    OP_SILHOUETTE = 15,           // [c, n, r, persp, eye] -> [st, found, (p1, p2)]
    OP_CONES_DISJOINT = 16,       // [v1, a1, v2, a2] -> [1000, bool]
    OP_LINE_CIRCLE = 17,          // [lp, lv, cc, cn, r, tol] -> [st, n, params...]
    OP_CIRCLE_CIRCLE = 18,        // [n, c1, r1, c2, r2, tol] -> [st, n, pts...]
    OP_LINE_SPHERE = 19,          // [lp, lv, c, r, tol] -> [st, n, results...]
    OP_THREE_PLANES = 20,         // [p1, v1, p2, v2, p3, v3] -> [st, p]
    OP_PLANE_EVAL = 21,           // [o, x, y, uv2] -> [1000, p]
    OP_PLANE_DROP = 22,           // [o, x, y, p] -> [1000, uv]
    OP_PLANE_POINT_DIST = 23,     // [pp, pn, tp] -> [st, d]
    OP_POINT_PROJECT = 24,        // [p, pp, pn] -> [st, p]
    OP_VECTOR_PROJECT = 25,       // [v, n] -> [st, v]
    OP_START_PLANE = 26,          // [p, o, x, y, smin, smax, tol, snap] -> [st, p, dist, n, angles..., inside]
    OP_PERSPECTIVE = 27,          // [p, pp, pn, eye] -> [st, p]
    OP_VEC_LIN_COMB = 28,         // [b1, b2, v] -> [st, u, v]
    OP_BEST_FIT_POINT = 29,       // [tol, pts...] -> [st, type, c, gap]
    OP_BEST_FIT_LINE = 30,        // [tol, pts...] -> [st, type, pt, vec, linegap, cgap, (M9)]
    OP_TRIPLE_PRODUCT = 31,       // [a, b, c] -> [1000, v]
    OP_COPY_3X3 = 32,             // [m9] -> [1000, m9]
    OP_DET_3X3 = 33,              // [m9] -> [1000, d]
    OP_DET_3_VECTORS = 34,        // [v1, v2, v3] -> [1000, d]
    OP_INVERSE_3X3 = 35,          // [a0, a1, a2] -> [st, inv9]
    OP_COPY_4X4 = 36,             // [m16] -> [1000, m16]
    OP_DET_4X4 = 37,              // [m16] -> [1000, d]
    OP_QUADRATIC = 38,            // [c0, c1, c2, tol] -> [st, n, roots...]
    OP_DIR_DERIVS = 39,           // [dx, dy, hu, hv, derivs...] -> [st, n, vecs...]
    OP_VOL_DIR_DERIVS = 40,       // [dx, dy, dz, n, derivs...] -> [st, cnt, vecs...]
    OP_DOG_LEG = 41,              // [p0, p1, p2, p3 (2d), bend, par] -> [1000, bool]
    OP_ADD_SORTED = 42,           // [value, idx, incr, with_idx, n, vals(n), idxs(n)] -> [st, n, vals, (idxs)]
    OP_SHELL_SORT = 43,           // [vals...] -> [1000, vals]
    OP_DOUBLE_LESS = 44,          // [a, b] -> [1000, bool]
};

static SmVector3d sc_v3(const double* a) { return SmVector3d(a[0], a[1], a[2]); }
static SmVector2d sc_v2(const double* a) { return SmVector2d(a[0], a[1]); }
static void sc_put3(std::vector<double>& o, const SmVector3d& v)
{
    o.push_back(v.x);
    o.push_back(v.y);
    o.push_back(v.z);
}
static double sc_flag(SmBoolean b) { return b ? 1.0 : 0.0; }

static int32_t sc_run(int32_t op, const double* in, int64_t n_in, std::vector<double>& o)
{
    const double* x = in;
    SmStatus st = SM_SUCCESS;
    // Pushes the status; returns false when it is not SM_SUCCESS (stop writing).
    auto status = [&o](SmStatus s) {
        o.push_back((double)s);
        return s == SM_SUCCESS;
    };
    switch (op)
    {
    case OP_POLYGON_AREA:
    {
        ULONG n = (ULONG)((n_in - 3) / 3);
        std::vector<SmPoint3d> pts;
        for (ULONG i = 0; i < n; ++i)
            pts.push_back(sc_v3(x + 3 + 3 * i));
        double area = 0.0;
        st = smgu_PolygonArea(pts.data(), n, sc_v3(x), area);
        if (status(st))
            o.push_back(area);
        return SC_OK;
    }
    case OP_TRIANGLE_AREA_SQ:
    {
        double a = 0.0;
        st = smgu_TriangleAreaSquared(sc_v3(x), sc_v3(x + 3), sc_v3(x + 6), a);
        if (status(st))
            o.push_back(a);
        return SC_OK;
    }
    case OP_TRIANGLE_DECOMP:
    {
        double along = 0.0, h = 0.0;
        st = smgu_TriangleDecomposition(x[0], x[1], x[2], along, h);
        if (status(st))
        {
            o.push_back(along);
            o.push_back(h);
        }
        return SC_OK;
    }
    case OP_PARALLELOGRAM:
    {
        SmVector3d v1, v2;
        st = smgu_ParallelogramCrossVectors(sc_v3(x), sc_v3(x + 3), x[6], x[7], v1, v2);
        if (status(st))
        {
            sc_put3(o, v1);
            sc_put3(o, v2);
        }
        return SC_OK;
    }
    case OP_LINE_POINT_DIST:
    {
        double d = 0.0, t = 0.0;
        st = smgu_LinePointDistance(sc_v3(x), sc_v3(x + 3), sc_v3(x + 6), d, &t);
        if (status(st))
        {
            o.push_back(d);
            o.push_back(t);
        }
        return SC_OK;
    }
    case OP_LINE_LINE_CLOSEST:
    {
        double t1 = 0.0, t2 = 0.0;
        st = smgu_LineLineClosestPoint(sc_v3(x), sc_v3(x + 3), sc_v3(x + 6), sc_v3(x + 9), t1, t2);
        if (status(st))
        {
            o.push_back(t1);
            o.push_back(t2);
        }
        return SC_OK;
    }
    case OP_LINE_PLANE:
    {
        double t = 0.0;
        st = smgu_LinePlaneIntersect(sc_v3(x), sc_v3(x + 3), sc_v3(x + 6), sc_v3(x + 9), t);
        if (status(st))
            o.push_back(t);
        return SC_OK;
    }
    case OP_LINE_PLANE_UV:
    {
        double t = 0.0, u = 0.0, v = 0.0;
        st = smgu_LinePlaneIntersect(sc_v3(x), sc_v3(x + 3), sc_v3(x + 6), sc_v3(x + 9), sc_v3(x + 12),
                                     t, u, v);
        if (status(st))
        {
            o.push_back(t);
            o.push_back(u);
            o.push_back(v);
        }
        return SC_OK;
    }
    case OP_LINE_CLOSEST:
    {
        double t = 0.0;
        st = smgu_LineClosestPoint(sc_v3(x), sc_v3(x + 3), sc_v3(x + 6), t);
        if (status(st))
            o.push_back(t);
        return SC_OK;
    }
    case OP_SEGMENT_POINT_DIST:
    {
        double d = 0.0, t = 0.0;
        st = smgu_SegmentPointDistance(sc_v3(x), sc_v3(x + 3), sc_v3(x + 6), d, t);
        if (status(st))
        {
            o.push_back(d);
            o.push_back(t);
        }
        return SC_OK;
    }
    case OP_SEGMENT_ANGLE:
    {
        double t = 0.0;
        double a = smgu_SegmentPointAngleDegToPerp(sc_v3(x), sc_v3(x + 3), sc_v3(x + 6), t);
        status(SM_SUCCESS);
        o.push_back(a);
        o.push_back(t);
        return SC_OK;
    }
    case OP_CIRCLE_CENTER_3PTS:
    {
        SmPoint2d c(0.0, 0.0);
        st = smgu_CircleCenterFrom3Points(sc_v2(x), sc_v2(x + 2), sc_v2(x + 4), c);
        if (status(st))
        {
            o.push_back(c.x);
            o.push_back(c.y);
        }
        return SC_OK;
    }
    case OP_CIRCLE_3PTS:
    {
        SmPoint3d c;
        SmVector3d xa, ya;
        SmExtent1d dom;
        double r = 0.0;
        st = smgu_CircleFrom3Points(sc_v3(x), sc_v3(x + 3), sc_v3(x + 6), c, xa, ya, dom, r, (ULONG)x[9],
                                    x[10] != 0.0 ? TRUE : FALSE);
        if (status(st))
        {
            sc_put3(o, c);
            sc_put3(o, xa);
            sc_put3(o, ya);
            o.push_back(dom.GetMin());
            o.push_back(dom.GetMax());
            o.push_back(r);
        }
        return SC_OK;
    }
    case OP_SILHOUETTE:
    {
        SmBoolean found = FALSE;
        SmPoint3d p1, p2;
        st = smgu_CircleFindSilhouettePoints(sc_v3(x), sc_v3(x + 3), x[6], x[7] != 0.0 ? TRUE : FALSE,
                                             sc_v3(x + 8), found, p1, p2);
        if (status(st))
        {
            o.push_back(sc_flag(found));
            if (found)
            {
                sc_put3(o, p1);
                sc_put3(o, p2);
            }
        }
        return SC_OK;
    }
    case OP_CONES_DISJOINT:
    {
        SmBoolean b = smgu_ConesAreDisjointPerpen(sc_v3(x), x[3], sc_v3(x + 4), x[7]);
        status(SM_SUCCESS);
        o.push_back(sc_flag(b));
        return SC_OK;
    }
    case OP_LINE_CIRCLE:
    {
        ULONG n = 0;
        double t[2] = {0.0, 0.0};
        st = smgu_LineCoPlanarCircleIntersect(sc_v3(x), sc_v3(x + 3), sc_v3(x + 6), sc_v3(x + 9), x[12],
                                              x[13], n, t);
        if (status(st))
        {
            o.push_back((double)n);
            for (ULONG i = 0; i < n && i < 2; ++i)
                o.push_back(t[i]);
        }
        return SC_OK;
    }
    case OP_CIRCLE_CIRCLE:
    {
        ULONG n = 0;
        SmPoint3d p[2];
        st = smgu_CircleCoPlanarCircleIntersect(sc_v3(x), sc_v3(x + 3), x[6], sc_v3(x + 7), x[10], x[11],
                                                n, p);
        if (status(st))
        {
            o.push_back((double)n);
            ULONG np = (n == 1 || n == 2) ? n : 0;
            for (ULONG i = 0; i < np; ++i)
                sc_put3(o, p[i]);
        }
        return SC_OK;
    }
    case OP_LINE_SPHERE:
    {
        ULONG n = 0;
        double r[3] = {0.0, 0.0, 0.0};
        st = smgu_LineIntersectSphere(sc_v3(x), sc_v3(x + 3), sc_v3(x + 6), x[9], x[10], n, r);
        if (status(st))
        {
            o.push_back((double)n);
            for (ULONG i = 0; i < n && i < 3; ++i)
                o.push_back(r[i]);
        }
        return SC_OK;
    }
    case OP_THREE_PLANES:
    {
        SmPoint3d p;
        st = smgu_IntersectThreePlanes(sc_v3(x), sc_v3(x + 3), sc_v3(x + 6), sc_v3(x + 9), sc_v3(x + 12),
                                       sc_v3(x + 15), p);
        if (status(st))
            sc_put3(o, p);
        return SC_OK;
    }
    case OP_PLANE_EVAL:
    {
        SmAxis2Placement a(sc_v3(x), sc_v3(x + 3), sc_v3(x + 6));
        SmPoint3d p = smgu_PlaneEvaluatePoint(a, sc_v2(x + 9));
        status(SM_SUCCESS);
        sc_put3(o, p);
        return SC_OK;
    }
    case OP_PLANE_DROP:
    {
        SmAxis2Placement a(sc_v3(x), sc_v3(x + 3), sc_v3(x + 6));
        SmPoint2d p = smgu_PlaneDropPoint(a, sc_v3(x + 9));
        status(SM_SUCCESS);
        o.push_back(p.x);
        o.push_back(p.y);
        return SC_OK;
    }
    case OP_PLANE_POINT_DIST:
    {
        double d = 0.0;
        st = smgu_PlanePointDistance(sc_v3(x), sc_v3(x + 3), sc_v3(x + 6), d);
        if (status(st))
            o.push_back(d);
        return SC_OK;
    }
    case OP_POINT_PROJECT:
    {
        SmPoint3d p;
        st = smgu_PointProjectToPlane(sc_v3(x), sc_v3(x + 3), sc_v3(x + 6), p);
        if (status(st))
            sc_put3(o, p);
        return SC_OK;
    }
    case OP_VECTOR_PROJECT:
    {
        SmVector3d v;
        st = smgu_VectorProjectToPlane(sc_v3(x), sc_v3(x + 3), v);
        if (status(st))
            sc_put3(o, v);
        return SC_OK;
    }
    case OP_START_PLANE:
    {
        SmAxis2Placement a(sc_v3(x + 3), sc_v3(x + 6), sc_v3(x + 9));
        SmExtent1d sweep(x[12], x[13]);
        SmPoint3d p;
        double dist = 0.0;
        ULONG n = 0;
        double ang[2] = {0.0, 0.0};
        SmBoolean inside = FALSE;
        st = smgu_TransformPointToStartPlane(sc_v3(x), a, sweep, x[14], p, dist, n, ang, inside,
                                             x[15] != 0.0 ? TRUE : FALSE);
        if (status(st))
        {
            sc_put3(o, p);
            o.push_back(dist);
            o.push_back((double)n);
            for (ULONG i = 0; i < n && i < 2; ++i)
                o.push_back(ang[i]);
            o.push_back(sc_flag(inside));
        }
        return SC_OK;
    }
    case OP_PERSPECTIVE:
    {
        SmPoint3d p;
        st = smgu_PointPerspectiveToPlane(sc_v3(x), sc_v3(x + 3), sc_v3(x + 6), sc_v3(x + 9), p);
        if (status(st))
            sc_put3(o, p);
        return SC_OK;
    }
    case OP_VEC_LIN_COMB:
    {
        SmVector2d uv;
        st = smgu_VecLinCombTwoVectors(sc_v3(x), sc_v3(x + 3), sc_v3(x + 6), uv);
        if (status(st))
        {
            o.push_back(uv.x);
            o.push_back(uv.y);
        }
        return SC_OK;
    }
    case OP_BEST_FIT_POINT:
    {
        ULONG n = (ULONG)((n_in - 1) / 3);
        SmTArray<SmPoint3d> pts;
        for (ULONG i = 0; i < n; ++i)
            pts.Add(sc_v3(x + 1 + 3 * i));
        SmPointSetType t = SM_PST_UNKNOWN;
        SmVector3d c;
        double gap = 0.0;
        st = smgu_BestFitPointToPoints(pts, x[0], t, c, gap);
        if (status(st))
        {
            o.push_back((double)t);
            sc_put3(o, c);
            o.push_back(gap);
        }
        return SC_OK;
    }
    case OP_BEST_FIT_LINE:
    {
        ULONG n = (ULONG)((n_in - 1) / 3);
        SmTArray<SmPoint3d> pts;
        for (ULONG i = 0; i < n; ++i)
            pts.Add(sc_v3(x + 1 + 3 * i));
        SmPointSetType t = SM_PST_UNKNOWN;
        SmVector3d lp, lv;
        double gap = 0.0, cgap = 0.0;
        SmMatrix m(3, 3);
        st = smgu_BestFitLineToPoints(pts, x[0], t, lp, lv, gap, cgap, &m);
        if (status(st))
        {
            o.push_back((double)t);
            sc_put3(o, lp);
            sc_put3(o, lv);
            o.push_back(gap);
            o.push_back(cgap);
            // M is written only past the early return (types LINEAR / SCATTERED).
            if (t == SM_PST_LINEAR || t == SM_PST_SCATTERED)
                for (ULONG i = 0; i < 3; ++i)
                    for (ULONG j = 0; j < 3; ++j)
                        o.push_back(m.GetAt(i, j));
        }
        return SC_OK;
    }
    case OP_TRIPLE_PRODUCT:
    {
        double v = smgu_VecTripleProduct(x, x + 3, x + 6);
        status(SM_SUCCESS);
        o.push_back(v);
        return SC_OK;
    }
    case OP_COPY_3X3:
    {
        double a[3][3], b[3][3];
        for (int i = 0; i < 9; ++i)
        {
            a[i / 3][i % 3] = x[i];
            b[i / 3][i % 3] = NAN;
        }
        smgu_3x3Copy(a, b);
        status(SM_SUCCESS);
        for (int i = 0; i < 9; ++i)
            o.push_back(b[i / 3][i % 3]);
        return SC_OK;
    }
    case OP_DET_3X3:
    {
        double d = smgu_3x3Determinate(x[0], x[1], x[2], x[3], x[4], x[5], x[6], x[7], x[8]);
        status(SM_SUCCESS);
        o.push_back(d);
        return SC_OK;
    }
    case OP_DET_3_VECTORS:
    {
        double d = smgu_Determinant3Vectors(sc_v3(x), sc_v3(x + 3), sc_v3(x + 6));
        status(SM_SUCCESS);
        o.push_back(d);
        return SC_OK;
    }
    case OP_INVERSE_3X3:
    {
        double inv[3][3];
        st = smgu_3x3Inverse(x, x + 3, x + 6, inv);
        if (status(st))
            for (int i = 0; i < 9; ++i)
                o.push_back(inv[i / 3][i % 3]);
        return SC_OK;
    }
    case OP_COPY_4X4:
    {
        double a[4][4], b[4][4];
        for (int i = 0; i < 16; ++i)
        {
            a[i / 4][i % 4] = x[i];
            b[i / 4][i % 4] = NAN;
        }
        smgu_4x4Copy(a, b);
        status(SM_SUCCESS);
        for (int i = 0; i < 16; ++i)
            o.push_back(b[i / 4][i % 4]);
        return SC_OK;
    }
    case OP_DET_4X4:
    {
        double a[4][4];
        for (int i = 0; i < 16; ++i)
            a[i / 4][i % 4] = x[i];
        double d = smgu_4x4Determinate(a);
        status(SM_SUCCESS);
        o.push_back(d);
        return SC_OK;
    }
    case OP_QUADRATIC:
    {
        double c[3] = {x[0], x[1], x[2]};
        ULONG n = 0;
        double r[2] = {0.0, 0.0};
        st = smgu_SolveQuadraticEqn(c, x[3], n, r);
        if (status(st))
        {
            o.push_back((double)n);
            for (ULONG i = 0; i < n && i < 2; ++i)
                o.push_back(r[i]);
        }
        return SC_OK;
    }
    case OP_DIR_DERIVS:
    {
        ULONG hu = (ULONG)x[2], hv = (ULONG)x[3];
        ULONG nd = (hu + 1) * (hv + 1);
        std::vector<SmVector3d> d;
        for (ULONG i = 0; i < nd; ++i)
            d.push_back(sc_v3(x + 4 + 3 * i));
        SmVector3d dd[4];
        st = smgu_DirectionalDerivs(sc_v2(x), hu, hv, d.data(), dd);
        if (status(st))
        {
            ULONG m = hu < hv ? hu : hv;
            ULONG cnt = (m < 3 ? m : 3) + 1;
            o.push_back((double)cnt);
            for (ULONG i = 0; i < cnt; ++i)
                sc_put3(o, dd[i]);
        }
        return SC_OK;
    }
    case OP_VOL_DIR_DERIVS:
    {
        ULONG n = (ULONG)x[3];
        ULONG nd = (n + 1) * (n + 1) * (n + 1);
        std::vector<SmVector3d> d;
        for (ULONG i = 0; i < nd; ++i)
            d.push_back(sc_v3(x + 4 + 3 * i));
        SmVector3d dd[4];
        st = smgu_VolDirectionalDerivs(sc_v3(x), n, d.data(), dd);
        if (status(st))
        {
            ULONG cnt = (n < 3 ? n : 3) + 1;
            o.push_back((double)cnt);
            for (ULONG i = 0; i < cnt; ++i)
                sc_put3(o, dd[i]);
        }
        return SC_OK;
    }
    case OP_DOG_LEG:
    {
        SmVector2d p0 = sc_v2(x), p1 = sc_v2(x + 2), p2 = sc_v2(x + 4), p3 = sc_v2(x + 6);
        SmBoolean b = smgu_IsDogLeg(p0, p1, p2, p3, x[8], x[9]);
        status(SM_SUCCESS);
        o.push_back(sc_flag(b));
        return SC_OK;
    }
    case OP_ADD_SORTED:
    {
        ULONG n = (ULONG)x[4];
        bool with_idx = x[3] != 0.0;
        SmTArray<double> vals;
        SmTArray<ULONG> idxs;
        for (ULONG i = 0; i < n; ++i)
        {
            vals.Add(x[5 + i]);
            idxs.Add((ULONG)x[5 + n + i]);
        }
        st = smgu_AddSorted(x[0], (ULONG)x[1], x[2] != 0.0 ? TRUE : FALSE, vals, with_idx ? &idxs : NULL);
        if (status(st))
        {
            o.push_back((double)vals.GetSize());
            for (ULONG i = 0; i < vals.GetSize(); ++i)
                o.push_back(vals[i]);
            if (with_idx)
                for (ULONG i = 0; i < idxs.GetSize(); ++i)
                    o.push_back((double)idxs[i]);
        }
        return SC_OK;
    }
    case OP_SHELL_SORT:
    {
        std::vector<double> v(x, x + n_in);
        smgu_ShellSort_double_array(v.data(), (ULONG)v.size());
        status(SM_SUCCESS);
        for (double d : v)
            o.push_back(d);
        return SC_OK;
    }
    case OP_DOUBLE_LESS:
    {
        SmBoolean b = smgu_DoubleLessThan(x[0], x[1], NULL);
        status(SM_SUCCESS);
        o.push_back(sc_flag(b));
        return SC_OK;
    }
    default:
        return SC_ERR_ARG;
    }
}

SC_EXPORT int32_t sc_geom_geom_utility_p1_call(int32_t op, const double* in, int64_t n_in, double* out,
                                               int64_t cap, int64_t* out_len)
{
    if (n_in < 0 || (n_in > 0 && in == nullptr))
        return SC_ERR_ARG;
    SC_TRY
    std::vector<double> o;
    int32_t r = sc_run(op, in, n_in, o);
    if (r != SC_OK)
        return r;
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
