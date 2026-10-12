// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.extent3d`. Wraps upstream SmExtent3d
// (source/SMLib/src/SmExtent3d.cpp, source/SMLib/inc/SmExtent3d.h).
//
// One entry point, sc_geom_extent3d_op(op, in, n_in, out, cap, out_len): `op` selects
// an operation (the numbering is shared with crates/solidcore-ffi/src/units/geom_extent3d.rs),
// `in` holds its flat double inputs and the shim writes a flat double result.
// An extent is 6 doubles [min.x min.y min.z max.x max.y max.z], stored as given
// (SmExtent3d(UMin, VMin, WMin, UMax, VMax, WMax), no swapping). Upstream SmStatus
// results are written as a leading double: 0 for SM_SUCCESS, else the SM_ERR_* value.
// Booleans are written as 0/1. The Rust wrapper checks n_in against the op's
// input size; the shim checks it again.
#include "sc_shim.h"

#include <SmExtent3d.h>
#include <SmPseudoBox.h>
#include <SmTArray.h>

#include <cmath>
#include <vector>

namespace
{
SmExtent3d ext(const double* d) { return SmExtent3d(d[0], d[1], d[2], d[3], d[4], d[5]); }
SmPoint3d pt(const double* d) { return SmPoint3d(d[0], d[1], d[2]); }

void put_pt(std::vector<double>& o, const SmPoint3d& p)
{
    o.push_back(p.x);
    o.push_back(p.y);
    o.push_back(p.z);
}

void put_ext(std::vector<double>& o, const SmExtent3d& e)
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
    case 0: return 6;   // from_points: a, b
    case 1: return 3;   // from_point: p
    case 2: return 0;   // new
    case 3: return -1;  // add_points: A, n*3
    case 4: return 12;  // set_min_max: A, min, max
    case 5: return 12;  // set_min_max_coords: A, 6 coords
    case 6: return 8;   // set_bound: A, which, value
    case 7: return 7;   // queries: A, tol
    case 8: return 9;   // evaluate: A, nx, ny, nz
    case 9: return 12;  // union: A, B
    case 10: return 12; // intersect: A, B
    case 11: return 12; // distances: A, B
    case 12: return 7;  // expand_relative: A, factor
    case 13: return 7;  // expand_absolute: A, amount
    case 14: return 6;  // corners/edges/planes: A
    case 15: return 6;  // sphere_bound: A
    case 16: return 12; // cone_bound_to: A, B
    case 17: return 13; // disjoint / contained: A, B, tol
    case 18: return 10; // point predicates: A, p, tol
    case 19: return 13; // contains_line_seg: A, p, q, tol
    case 20: return 12; // intersect_line: A, p, v
    case 21: return 6;  // set_unbounded: A
    case 22: return 11; // approximate_unbounded: A, has_center, center, half_size
    case 23: return 15; // circumscribe: basis x, y, z, intervals x, y, z
    case 24: return 10; // intervals: A, u ivl, v ivl
    default: return -2;
    }
}
} // namespace

SC_EXPORT int32_t sc_geom_extent3d_op(int32_t op, const double* in, int64_t n_in, double* out,
                                      int64_t cap, int64_t* out_len)
{
    int64_t need = input_size(op);
    if (need == -2 || n_in < 0 || (need >= 0 && n_in != need))
        return SC_ERR_ARG;
    if (op == 3 && (n_in < 6 || (n_in - 6) % 3 != 0))
        return SC_ERR_ARG;
    SC_TRY
    std::vector<double> o;
    switch (op)
    {
    case 0: put_ext(o, SmExtent3d(pt(in), pt(in + 3))); break;
    case 1: put_ext(o, SmExtent3d(pt(in))); break;
    case 2:
    {
        SmExtent3d e;
        put_ext(o, e);
        o.push_back(b(e.IsInit()));
        o.push_back(b(e.HasNegativeVolume()));
        break;
    }
    case 3:
    {
        SmExtent3d e = ext(in);
        for (int64_t i = 6; i < n_in; i += 3)
            e.AddPoint3d(pt(in + i));
        put_ext(o, e);
        break;
    }
    case 4:
    {
        SmExtent3d e = ext(in);
        o.push_back(st(e.SetMinMax(pt(in + 6), pt(in + 9))));
        put_ext(o, e);
        break;
    }
    case 5:
    {
        SmExtent3d e = ext(in);
        o.push_back(st(e.SetMinMax(in[6], in[7], in[8], in[9], in[10], in[11])));
        put_ext(o, e);
        break;
    }
    case 6:
    {
        SmExtent3d e = ext(in);
        SmStatus s = SM_SUCCESS;
        switch ((int)in[6])
        {
        case 0: s = e.SetUMin(in[7]); break;
        case 1: s = e.SetUMax(in[7]); break;
        case 2: s = e.SetVMin(in[7]); break;
        case 3: s = e.SetVMax(in[7]); break;
        default: return SC_ERR_ARG;
        }
        o.push_back(st(s));
        put_ext(o, e);
        break;
    }
    case 7:
    {
        const SmExtent3d e = ext(in);
        double tol = in[6];
        put_pt(o, e.GetMid());
        o.push_back(e.GetUMin());
        o.push_back(e.GetUMax());
        o.push_back(e.GetVMin());
        o.push_back(e.GetVMax());
        o.push_back(e.GetWMin());
        o.push_back(e.GetWMax());
        for (ULONG i = 0; i < 4; ++i)
        {
            o.push_back(e.GetMin(i));
            o.push_back(e.GetMax(i));
        }
        o.push_back(e.GetMaxLength());
        o.push_back(e.GetMaxDimension());
        put_pt(o, e.GetSize());
        o.push_back(e.XLength());
        o.push_back(e.YLength());
        o.push_back(e.ZLength());
        o.push_back(b(e.AssertDefined()));
        o.push_back(b(e.HasNegativeVolume()));
        o.push_back(b(e.IsInit()));
        SmBoundaryType bt[3] = {SM_BT_UNKNOWN, SM_BT_UNKNOWN, SM_BT_UNKNOWN};
        o.push_back(b(e.IsBounded(bt)));
        for (int i = 0; i < 3; ++i)
            o.push_back((double)bt[i]);
        o.push_back(b(e.AnyBounds()));
        o.push_back(b(e.IsPointSized(tol)));
        break;
    }
    case 8: put_pt(o, ext(in).Evaluate(in[6], in[7], in[8])); break;
    case 9:
    {
        SmExtent3d r;
        ext(in).Union(ext(in + 6), r);
        put_ext(o, r);
        break;
    }
    case 10:
    {
        SmExtent3d r;
        SmStatus s = ext(in).Intersect(ext(in + 6), r);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
            put_ext(o, r);
        break;
    }
    case 11:
    {
        const SmExtent3d a = ext(in), c = ext(in + 6);
        o.push_back(a.MaximumDistanceSquared(c));
        o.push_back(a.MinimumDistanceSquared(c));
        o.push_back(a.MaximumDistance(c));
        o.push_back(a.MinimumDistance(c));
        break;
    }
    case 12:
    {
        SmExtent3d e = ext(in);
        put_ext(o, e.ExpandRelative(in[6]));
        break;
    }
    case 13:
    {
        SmExtent3d e = ext(in);
        put_ext(o, e.ExpandAbsolute(in[6]));
        break;
    }
    case 14:
    {
        const SmExtent3d e = ext(in);
        SmTArray<SmPoint3d> c, ed, pp;
        SmTArray<SmVector3d> pn;
        e.GetCorners(c);
        e.GetEdges(ed);
        e.GetPlanes(pp, pn);
        for (ULONG i = 0; i < c.GetSize(); ++i)
            put_pt(o, c[i]);
        for (ULONG i = 0; i < ed.GetSize(); ++i)
            put_pt(o, ed[i]);
        for (ULONG i = 0; i < pp.GetSize(); ++i)
            put_pt(o, pp[i]);
        for (ULONG i = 0; i < pn.GetSize(); ++i)
            put_pt(o, pn[i]);
        break;
    }
    case 15:
    {
        SmVector3d c;
        double r = 0.0, rmin = 0.0;
        ext(in).ComputeSphereBound(c, r, &rmin);
        put_pt(o, c);
        o.push_back(r);
        o.push_back(rmin);
        break;
    }
    case 16:
    {
        SmVector3d v(NAN, NAN, NAN);
        double ang = NAN;
        SmStatus s = ext(in).ComputeConeBoundTo(ext(in + 6), v, ang);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            put_pt(o, v);
            o.push_back(ang);
        }
        break;
    }
    case 17:
    {
        const SmExtent3d a = ext(in), c = ext(in + 6);
        o.push_back(b(a.AreDisjoint(c, in[12])));
        o.push_back(b(a.IsContainedBy(c, in[12])));
        break;
    }
    case 18:
    {
        const SmExtent3d e = ext(in);
        SmVector3d bn(NAN, NAN, NAN);
        o.push_back(b(e.ContainsPoint3d(pt(in + 6), in[9])));
        o.push_back(b(e.IsPoint3dOnBoundary(pt(in + 6), in[9], &bn)));
        put_pt(o, bn);
        break;
    }
    case 19: o.push_back(b(ext(in).ContainsLineSeg3d(pt(in + 6), pt(in + 9), in[12]))); break;
    case 20:
    {
        ULONG n = 99;
        double t0 = NAN, t1 = NAN;
        SmStatus s = ext(in).IntersectLine(pt(in + 6), pt(in + 9), n, t0, t1);
        o.push_back(st(s));
        if (s == SM_SUCCESS)
        {
            o.push_back((double)n);
            o.push_back(n > 0 ? t0 : NAN);
            o.push_back(n > 0 ? t1 : NAN);
        }
        break;
    }
    case 21:
    {
        SmExtent3d e = ext(in);
        e.SetUnbounded();
        put_ext(o, e);
        break;
    }
    case 22:
    {
        const SmExtent3d e = ext(in);
        SmPoint3d c = pt(in + 7);
        SmExtent3d x; // Init state; untouched when SetMinMax rejects
        SmExtent3d a = e.ApproximateUnbounded(in[6] != 0.0 ? &c : NULL, in[10], &x);
        put_ext(o, a);
        put_ext(o, x);
        break;
    }
    case 23:
    {
        SmVector3d bx = pt(in), by = pt(in + 3), bz = pt(in + 6);
        SmExtent1d ix(in[9], in[10]), iy(in[11], in[12]), iz(in[13], in[14]);
        SmPseudoBox pb(bx, by, bz, ix, iy, iz);
        put_ext(o, SmExtent3d(pb));
        break;
    }
    case 24:
    {
        SmExtent3d e = ext(in);
        SmExtent1d u = e.GetUInterval(), v = e.GetVInterval(), w = e.GetWInterval();
        o.push_back(u.GetMin());
        o.push_back(u.GetMax());
        o.push_back(v.GetMin());
        o.push_back(v.GetMax());
        o.push_back(w.GetMin());
        o.push_back(w.GetMax());
        o.push_back(st(e.SetUInterval(SmExtent1d(in[6], in[7]))));
        o.push_back(st(e.SetVInterval(SmExtent1d(in[8], in[9]))));
        put_ext(o, e);
        break;
    }
    default: return SC_ERR_ARG;
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
