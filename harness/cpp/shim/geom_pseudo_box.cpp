// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.pseudo_box`. Wraps upstream SmPseudoBox
// (source/SMLib/src/SmPseudoBox.cpp, source/SMLib/inc/SmPseudoBox.h), plus the
// SmPseudoBox overloads of SmExtent3d(const SmPseudoBox&) and SmTol::GetScaledZero.
//
// One entry point, sc_geom_pseudo_box_run(prog, n_prog, out, cap, out_len), runs a
// program of pseudo-box commands on four registers (all start as SmPseudoBox()) and
// writes the concatenated outputs. A command is: op code, its register operands, then
// its float arguments, all as doubles. The table below (op, registers, floats) is
// shared with crates/solidcore-ffi/src/units/geom_pseudo_box.rs, which validates
// programs before the call; the shim validates again.
//
//  op regs floats  action                                              output
//   0 r    -       reg[r].Init()                                       -
//   1 r    -       reg[r].InitBasis()                                  -
//   2 r    6       reg[r] = SmPseudoBox(SmExtent3d(umin..wmax))        -
//   3 r    6       reg[r] = SmExtent3d(umin..wmax)  (operator=)        -
//   4 r    9       reg[r].SetBasis(b0, b1, b2)                         status
//   5 r    6       reg[r].SetIntervals(SmExtent1d(a,b) x3)             -
//   6 r    3       reg[r].SetInterval(idx, SmExtent1d(a, b))           -
//   7 r    -       reg[r].SetUnbounded()                               -
//   8 r    3       reg[r].AddPoint3d(p)                                -
//   9 r    1       reg[r].ExpandAbsolute(d)                            -
//  10 r    -       GetBasis(0..2), GetInterval(0..2)                   15
//  11 r    4       ContainsPoint3d(p, tol)                             1
//  12 r    3       Evaluate(x, y, z)                                   3
//  13 r    3       InvertAbsolute(p, a)                                status [, 3]
//  14 r s t 9      reg[r].Union(reg[s], basis, reg[t])                 -
//  15 r    3       DistanceToPoint(p)                                  1
//  16 r    6       IntersectLine(p, v, n, t0, t1)                      status [, n, t0, t1]
//  17 r    -       HasNegativeVolume()                                 1
//  18 r    -       GetMaxDimension(), GetXYArea(), GetVolume()         3
//  19 r    1       IsPointSized(tol, &c, &dev)                         flag, c, dev
//  20 r    1       IsLinear(tol, &p, &t, &dev)                         flag, p, t, dev
//  21 r    1       IsPlanar(tol, &p, &n, &dev)                         flag, p, n, dev
//  22 r s  4       AreDisjoint(reg[s], has ? &v : NULL)  (has, v)      1
//  23 r s  10      AreProjectDisjoint(reg[s], type, &pt, &vec, &aux)   1
//  24 r s  1       IsContainedBy(reg[s], tol)                          1
//  25 r    4       ClassifyPoint3d(p, u, v, w, tol)                    3
//  26 r    -       CalcPlanes(points, vectors)                         36
//  27 r    -       IsBounded(types)                                    flag, 3 types
//  28 r    -       IsAxisAligned(map)                                  flag [, 3]
//  29 r s  -       AreAligned(reg[s], map)                             flag [, 3]
//  30 r    -       GetCorners                                          24
//  31 r    -       GetEdges                                            72
//  32 r    -       AssertDefined()                                     1
//  33 r t  10      reg[r].ProjectToPlane(reg[t], type, &pt, &vec, &aux) status
//  34 t r  -       reg[t] = reg[r]                                     -
//  35 r    15      reg[r] = SmPseudoBox(b0, b1, b2, SmExtent1d(a,b) x3) -
//  36 r    -       SmExtent3d(reg[r])  (Circumscribe)                  6
//  37 r    -       SmTol::GetScaledZero(reg[r])                        1
//
// Statuses are written as doubles (0 = SM_SUCCESS); booleans as 0/1; enums as their
// values; a projection type is 0 (SM_PT_PARALLEL), 1 (PERSPECTIVE) or 2 (ROTATION).
#include "sc_shim.h"

#include <SmExtent3d.h>
#include <SmPseudoBox.h>
#include <SmTArray.h>
#include <SmTol.h>

#include <cmath>
#include <vector>

#include "sc_sm.h"

using sc::put_vec;

namespace
{
constexpr int kRegs = 4;
constexpr int kOps = 38;

// registers and float arguments per op
const int kNRegs[kOps] = {1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 3, 1, 1, 1, 1,
                          1, 1, 1, 2, 2, 2, 1, 1, 1, 1, 2, 1, 1, 1, 2, 2, 1, 1, 1};
const int kNArgs[kOps] = {0, 0, 6, 6, 9, 6, 3, 0, 3, 1, 0, 4,  3, 3, 9, 3, 6, 0, 0,
                          1, 1, 1, 4, 10, 1, 4, 0, 0, 0, 0, 0, 0, 0, 10, 0, 15, 0, 0};

bool reg_index(double d, int& r)
{
    if (!(d >= 0.0 && d < kRegs) || d != std::floor(d))
        return false;
    r = (int)d;
    return true;
}

SmVector3d vec(const double* d) { return SmVector3d(d[0], d[1], d[2]); }

double b(SmBoolean v) { return v ? 1.0 : 0.0; }
double st(SmStatus s) { return s == SM_SUCCESS ? 0.0 : (double)s; }

bool proj_type(double d, SmProjectionType& t)
{
    if (d == 0.0)
        t = SM_PT_PARALLEL;
    else if (d == 1.0)
        t = SM_PT_PERSPECTIVE;
    else if (d == 2.0)
        t = SM_PT_ROTATION;
    else
        return false;
    return true;
}
} // namespace

SC_EXPORT int32_t sc_geom_pseudo_box_run(const double* prog, int64_t n_prog, double* out,
                                         int64_t cap, int64_t* out_len)
{
    if (n_prog < 0 || (n_prog > 0 && prog == nullptr))
        return SC_ERR_ARG;
    SC_TRY
    // validate the whole program first
    for (int64_t i = 0; i < n_prog;)
    {
        double opd = prog[i];
        if (!(opd >= 0.0 && opd < kOps) || opd != std::floor(opd))
            return SC_ERR_ARG;
        int op = (int)opd;
        int64_t n = kNRegs[op] + kNArgs[op];
        if (i + 1 + n > n_prog)
            return SC_ERR_ARG;
        int r;
        for (int k = 0; k < kNRegs[op]; ++k)
            if (!reg_index(prog[i + 1 + k], r))
                return SC_ERR_ARG;
        const double* a = prog + i + 1 + kNRegs[op];
        SmProjectionType t;
        if (op == 6 && !reg_index(a[0], r))
            return SC_ERR_ARG;
        if (op == 6 && r > 2)
            return SC_ERR_ARG;
        if ((op == 23 || op == 33) && !proj_type(a[0], t))
            return SC_ERR_ARG;
        if (op == 22 && a[0] != 0.0 && a[0] != 1.0)
            return SC_ERR_ARG;
        i += 1 + n;
    }

    SmPseudoBox reg[kRegs];
    std::vector<double> o;
    for (int64_t i = 0; i < n_prog;)
    {
        int op = (int)prog[i];
        int rg[3] = {0, 0, 0};
        for (int k = 0; k < kNRegs[op]; ++k)
            reg_index(prog[i + 1 + k], rg[k]);
        const double* a = prog + i + 1 + kNRegs[op];
        i += 1 + kNRegs[op] + kNArgs[op];
        SmPseudoBox& x = reg[rg[0]];
        switch (op)
        {
        case 0: x.Init(); break;
        case 1: x.InitBasis(); break;
        case 2: x = SmPseudoBox(SmExtent3d(a[0], a[1], a[2], a[3], a[4], a[5])); break;
        case 3: x = SmExtent3d(a[0], a[1], a[2], a[3], a[4], a[5]); break;
        case 4: o.push_back(st(x.SetBasis(vec(a), vec(a + 3), vec(a + 6)))); break;
        case 5:
            x.SetIntervals(SmExtent1d(a[0], a[1]), SmExtent1d(a[2], a[3]), SmExtent1d(a[4], a[5]));
            break;
        case 6: x.SetInterval((ULONG)a[0], SmExtent1d(a[1], a[2])); break;
        case 7: x.SetUnbounded(); break;
        case 8: x.AddPoint3d(vec(a)); break;
        case 9: x.ExpandAbsolute(a[0]); break;
        case 10:
            for (ULONG k = 0; k < 3; ++k)
                put_vec(o, x.GetBasis(k));
            for (ULONG k = 0; k < 3; ++k)
            {
                o.push_back(x.GetInterval(k).GetMin());
                o.push_back(x.GetInterval(k).GetMax());
            }
            break;
        case 11: o.push_back(b(x.ContainsPoint3d(vec(a), a[3]))); break;
        case 12: put_vec(o, x.Evaluate(a[0], a[1], a[2])); break;
        case 13:
        {
            SmPoint3d p;
            SmStatus s = x.InvertAbsolute(vec(a), p);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
                put_vec(o, p);
            break;
        }
        case 14:
        {
            SmVector3d basis[3] = {vec(a), vec(a + 3), vec(a + 6)};
            x.Union(reg[rg[1]], basis, reg[rg[2]]);
            break;
        }
        case 15: o.push_back(x.DistanceToPoint(vec(a))); break;
        case 16:
        {
            ULONG n = 0;
            double t0 = 0.0, t1 = 0.0;
            SmStatus s = x.IntersectLine(vec(a), vec(a + 3), n, t0, t1);
            o.push_back(st(s));
            if (s == SM_SUCCESS)
            {
                o.push_back((double)n);
                o.push_back(n > 0 ? t0 : 0.0);
                o.push_back(n > 0 ? t1 : 0.0);
            }
            break;
        }
        case 17: o.push_back(b(x.HasNegativeVolume())); break;
        case 18:
            o.push_back(x.GetMaxDimension());
            o.push_back(x.GetXYArea());
            o.push_back(x.GetVolume());
            break;
        case 19:
        {
            SmPoint3d c;
            double dev = 0.0;
            o.push_back(b(x.IsPointSized(a[0], &c, &dev)));
            put_vec(o, c);
            o.push_back(dev);
            break;
        }
        case 20:
        {
            SmPoint3d p;
            SmVector3d t;
            double dev = 0.0;
            o.push_back(b(x.IsLinear(a[0], &p, &t, &dev)));
            put_vec(o, p);
            put_vec(o, t);
            o.push_back(dev);
            break;
        }
        case 21:
        {
            SmPoint3d p;
            SmVector3d n;
            double dev = 0.0;
            o.push_back(b(x.IsPlanar(a[0], &p, &n, &dev)));
            put_vec(o, p);
            put_vec(o, n);
            o.push_back(dev);
            break;
        }
        case 22:
        {
            SmVector3d v = vec(a + 1);
            o.push_back(b(x.AreDisjoint(reg[rg[1]], a[0] != 0.0 ? &v : NULL)));
            break;
        }
        case 23:
        {
            SmProjectionType t = SM_PT_PARALLEL;
            proj_type(a[0], t);
            SmPoint3d pt = vec(a + 1);
            SmVector3d v = vec(a + 4), aux = vec(a + 7);
            o.push_back(b(x.AreProjectDisjoint(reg[rg[1]], t, &pt, &v, &aux)));
            break;
        }
        case 24: o.push_back(b(x.IsContainedBy(reg[rg[1]], a[0]))); break;
        case 25:
        {
            SmExtentPointType u = SM_EP_UNKNOWN, v = SM_EP_UNKNOWN, w = SM_EP_UNKNOWN;
            x.ClassifyPoint3d(vec(a), u, v, w, a[3]);
            o.push_back((double)u);
            o.push_back((double)v);
            o.push_back((double)w);
            break;
        }
        case 26:
        {
            SmPoint3d pts[6];
            SmVector3d vs[6];
            x.CalcPlanes(pts, vs);
            for (int k = 0; k < 6; ++k)
                put_vec(o, pts[k]);
            for (int k = 0; k < 6; ++k)
                put_vec(o, vs[k]);
            break;
        }
        case 27:
        {
            SmBoundaryType t[3] = {SM_BT_UNKNOWN, SM_BT_UNKNOWN, SM_BT_UNKNOWN};
            o.push_back(b(x.IsBounded(t)));
            for (int k = 0; k < 3; ++k)
                o.push_back((double)t[k]);
            break;
        }
        case 28:
        case 29:
        {
            long m[3] = {0, 0, 0};
            SmBoolean ok = op == 28 ? x.IsAxisAligned(m) : x.AreAligned(reg[rg[1]], m);
            o.push_back(b(ok));
            if (ok)
                for (int k = 0; k < 3; ++k)
                    o.push_back((double)m[k]);
            break;
        }
        case 30:
        {
            SmTArray<SmPoint3d> c;
            x.GetCorners(c);
            for (ULONG k = 0; k < c.GetSize(); ++k)
                put_vec(o, c[k]);
            break;
        }
        case 31:
        {
            SmTArray<SmPoint3d> c;
            x.GetEdges(c);
            for (ULONG k = 0; k < c.GetSize(); ++k)
                put_vec(o, c[k]);
            break;
        }
        case 32: o.push_back(b(x.AssertDefined())); break;
        case 33:
        {
            SmProjectionType t = SM_PT_PARALLEL;
            proj_type(a[0], t);
            SmPoint3d pt = vec(a + 1);
            SmVector3d v = vec(a + 4), aux = vec(a + 7);
            o.push_back(st(x.ProjectToPlane(reg[rg[1]], t, &pt, &v, &aux)));
            break;
        }
        case 34: x = reg[rg[1]]; break;
        case 35:
        {
            SmVector3d b0 = vec(a), b1 = vec(a + 3), b2 = vec(a + 6);
            SmExtent1d i0(a[9], a[10]), i1(a[11], a[12]), i2(a[13], a[14]);
            x = SmPseudoBox(b0, b1, b2, i0, i1, i2);
            break;
        }
        case 36:
        {
            SmExtent3d e(x);
            put_vec(o, e.GetMin());
            put_vec(o, e.GetMax());
            break;
        }
        case 37: o.push_back((double)SmTol::GetScaledZero(x)); break;
        default: return SC_ERR_ARG;
        }
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
