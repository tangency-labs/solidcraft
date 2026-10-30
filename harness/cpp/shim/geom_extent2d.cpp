// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.extent2d`. Wraps upstream SmExtent2d
// (source/SMLib/src/SmExtent2d.cpp, source/SMLib/inc/SmExtent2d.h).
//
// One entry point, sc_geom_extent2d_op(op, in, n_in, out, cap, out_len): `op` selects
// an operation (the numbering is shared with crates/solidcore-ffi/src/units/geom_extent2d.rs),
// `in` holds its flat double inputs and the shim writes a flat double result.
// An extent is 4 doubles [min.x min.y max.x max.y], stored as given
// (SmExtent2d(UMin, VMin, UMax, VMax), no swapping). Upstream SmStatus results are
// written as a leading double: 0 for SM_SUCCESS, else the SM_ERR_* value. Booleans
// are written as 0/1. The Rust wrapper checks n_in against the op's input size; the
// shim checks it again.
#include "sc_shim.h"

#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmExtent3d.h>
#include <SmTol.h>

#include <vector>

#include "sc_sm.h"

using sc::put_ivl;

namespace
{
SmExtent2d ext(const double* d) { return SmExtent2d(d[0], d[1], d[2], d[3]); }
SmPoint2d pt(const double* d) { return SmPoint2d(d[0], d[1]); }

void put_pt(std::vector<double>& o, const SmPoint2d& p)
{
    o.push_back(p.x);
    o.push_back(p.y);
}

void put_ext(std::vector<double>& o, const SmExtent2d& e)
{
    put_pt(o, e.GetMin());
    put_pt(o, e.GetMax());
}

double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }
double b(SmBoolean v) { return v ? 1.0 : 0.0; }

// Number of input doubles per op (-1: variable, checked in the op).
int64_t input_size(int32_t op)
{
    switch (op)
    {
    case 0: return 4;   // from_points: a, b
    case 1: return 2;   // from_point: p
    case 2: return 0;   // new
    case 3: return 6;   // from_extent3d: 6 bounds
    case 4: return -1;  // add_points: A, n*2
    case 5: return 8;   // set_min_max: A, min, max
    case 6: return 8;   // set_min_max_coords: A, 4 coords
    case 7: return 5;   // queries: A, tol
    case 8: return 6;   // evaluate: A, nx, ny
    case 9: return 8;   // union: A, B
    case 10: return 8;  // intersect: A, B
    case 11: return 9;  // compare: A, B, tol
    case 12: return 5;  // expand_relative: A, factor
    case 13: return 5;  // expand_absolute: A, d
    case 14: return 6;  // expand_absolute_vec: A, dx, dy
    case 15: return 4;  // transpose: A
    case 16: return 9;  // point_ops: A, p, tol, has_tol_v, tol_v
    case 17: return 9;  // contains_periodic: A, p, u period, v period, tol
    case 18: return 7;  // inversion: A, p, tol
    case 19: return 9;  // clip_line: A, p, v, t
    case 20: return 12; // intersect_line: A, p, v, has_tol_u, tol_u, has_tol_v, tol_v
    case 21: return 11; // map_to_domain: A, p, B, switch
    case 22: return 8;  // intervals: A, u ivl, v ivl
    case 23: return 4;  // scaled_zero: A
    case 24: return 7;  // scale: A, scale, has_scale_v, scale_v
    default: return -2;
    }
}
} // namespace

SC_EXPORT int32_t sc_geom_extent2d_op(int32_t op, const double* in, int64_t n_in, double* out,
                                      int64_t cap, int64_t* out_len)
{
    int64_t need = input_size(op);
    if (need == -2 || n_in < 0 || (need >= 0 && n_in != need))
        return SC_ERR_ARG;
    if (op == 4 && (n_in < 4 || (n_in - 4) % 2 != 0))
        return SC_ERR_ARG;
    SC_TRY
    std::vector<double> o;
    switch (op)
    {
    case 0: put_ext(o, SmExtent2d(pt(in), pt(in + 2))); break;
    case 1: put_ext(o, SmExtent2d(pt(in))); break;
    case 2:
    {
        SmExtent2d e;
        put_ext(o, e);
        o.push_back(b(e.IsInit()));
        o.push_back(b(e.HasNegativeArea()));
        break;
    }
    case 3:
    {
        const SmExtent3d x(in[0], in[1], in[2], in[3], in[4], in[5]);
        put_ext(o, SmExtent2d(x));
        break;
    }
    case 4:
    {
        SmExtent2d e = ext(in);
        for (int64_t i = 4; i < n_in; i += 2)
            e.AddPoint2d(pt(in + i));
        put_ext(o, e);
        break;
    }
    case 5:
    {
        SmExtent2d e = ext(in);
        o.push_back(st(e.SetMinMax(pt(in + 4), pt(in + 6))));
        put_ext(o, e);
        break;
    }
    case 6:
    {
        SmExtent2d e = ext(in);
        o.push_back(st(e.SetMinMax(in[4], in[5], in[6], in[7])));
        put_ext(o, e);
        break;
    }
    case 7:
    {
        const SmExtent2d e = ext(in);
        double tol = in[4];
        put_pt(o, e.GetMid());
        o.push_back(e.GetUMin());
        o.push_back(e.GetUMax());
        o.push_back(e.GetVMin());
        o.push_back(e.GetVMax());
        o.push_back(e.GetMaxDimension());
        put_pt(o, e.GetSize());
        o.push_back(e.XLength());
        o.push_back(e.YLength());
        put_ivl(o, e.GetUInterval());
        put_ivl(o, e.GetVInterval());
        o.push_back(b(e.IsInit()));
        o.push_back(b(e.HasNegativeArea()));
        o.push_back(b(e.IsDegenerate(tol)));
        SmBoundaryType bt[2] = {SM_BT_UNKNOWN, SM_BT_UNKNOWN};
        o.push_back(b(e.IsBounded(bt)));
        o.push_back((double)bt[0]);
        o.push_back((double)bt[1]);
        o.push_back(b(e.IsBounded()));
        break;
    }
    case 8: put_pt(o, ext(in).Evaluate(in[4], in[5])); break;
    case 9:
    {
        SmExtent2d r;
        ext(in).Union(ext(in + 4), r);
        put_ext(o, r);
        break;
    }
    case 10:
    {
        SmExtent2d r;
        SmStatus s = ext(in).Intersect(ext(in + 4), r);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_ext(o, r);
        break;
    }
    case 11:
    {
        const SmExtent2d a = ext(in), c = ext(in + 4);
        double tol = in[8];
        o.push_back(b(a.AreDisjoint(c, tol)));
        o.push_back(b(a.AreEqual(c, tol)));
        o.push_back(b(a.IsContainedBy(c, tol)));
        o.push_back(b(a == c));
        o.push_back(a.GetMaxBoundaryDist2d(c));
        break;
    }
    case 12:
    {
        SmExtent2d e = ext(in);
        put_ext(o, e.ExpandRelative(in[4]));
        break;
    }
    case 13:
    {
        SmExtent2d e = ext(in);
        put_ext(o, e.ExpandAbsolute(in[4]));
        break;
    }
    case 14:
    {
        SmExtent2d e = ext(in);
        put_ext(o, e.ExpandAbsolute(SmVector2d(in[4], in[5])));
        break;
    }
    case 15:
    {
        SmExtent2d e = ext(in);
        e.Transpose();
        put_ext(o, e);
        break;
    }
    case 16:
    {
        const SmExtent2d e = ext(in);
        const SmPoint2d p = pt(in + 4);
        double tol = in[6];
        double tol_v = in[8];
        double dist = -1.0;
        o.push_back(b(e.ContainsPoint2d(p, tol)));
        o.push_back(b(e.ContainsPoint2d(p, tol, &dist)));
        o.push_back(dist);
        SmExtentPointType tu = SM_EP_UNKNOWN, tv = SM_EP_UNKNOWN;
        e.ClassifyPoint2d(p, tu, tv, tol, in[7] != 0.0 ? &tol_v : NULL);
        o.push_back((double)tu);
        o.push_back((double)tv);
        SmVector2d bn(-7.0, -7.0);
        o.push_back(b(e.IsPoint2dOnBoundary(p, tol, &bn)));
        put_pt(o, bn);
        o.push_back(b(e.IsPoint2dOnBoundary(p, tol)));
        o.push_back((double)e.GetPoint2dBoundaries(p, tol));
        put_pt(o, e.ClampPoint2d(p));
        o.push_back(b(e.GetULeftEval(p.x)));
        o.push_back(b(e.GetVLeftEval(p.y)));
        break;
    }
    case 17:
    {
        const SmExtent2d e = ext(in);
        SmPoint2d per(-7.0, -7.0);
        o.push_back(b(e.ContainsPeriodicPoint2d(pt(in + 4), in[6], in[7], in[8], &per)));
        put_pt(o, per);
        o.push_back(b(e.ContainsPeriodicPoint2d(pt(in + 4), in[6], in[7], in[8])));
        break;
    }
    case 18:
    {
        SmPoint2d r(0.0, 0.0);
        SmStatus s = ext(in).Inversion(pt(in + 4), r, in[6]);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_pt(o, r);
        break;
    }
    case 19: o.push_back(ext(in).ClipLine2d(pt(in + 4), SmVector2d(in[6], in[7]), in[8])); break;
    case 20:
    {
        SmBoolean found = TRUE;
        SmExtent1d ivl(-7.0, -7.0);
        double tu = in[9], tv = in[11];
        SmStatus s = ext(in).IntersectWithInfiniteLine(pt(in + 4), SmVector2d(in[6], in[7]),
                                                       found, ivl, in[8] != 0.0 ? &tu : NULL,
                                                       in[10] != 0.0 ? &tv : NULL);
        o.push_back(st(s));
        o.push_back(b(found));
        if (found)
            put_ivl(o, ivl);
        break;
    }
    case 21:
    {
        SmPoint2d r(0.0, 0.0);
        SmStatus s = ext(in).Point2dMapToDomain(pt(in + 4), ext(in + 6), in[10] != 0.0, r);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_pt(o, r);
        break;
    }
    case 22:
    {
        SmExtent2d e = ext(in);
        put_ivl(o, e.GetUInterval());
        put_ivl(o, e.GetVInterval());
        o.push_back(st(e.SetUInterval(SmExtent1d(in[4], in[5]))));
        o.push_back(st(e.SetVInterval(SmExtent1d(in[6], in[7]))));
        put_ext(o, e);
        break;
    }
    case 23: o.push_back((double)SmTol::GetScaledZero(ext(in))); break;
    case 24:
    {
        SmExtent2d e = ext(in);
        double scale_v = in[6];
        put_ext(o, e.Scale(in[4], in[5] != 0.0 ? &scale_v : NULL));
        break;
    }
    default: return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
