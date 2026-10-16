// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.polar_box`. Wraps upstream SmPolarBox
// (source/SMLib/src/SmPolarBox.cpp, source/SMLib/inc/SmPolarBox.h).
//
// One entry point, sc_geom_polar_box_run(prog, n_prog, out, cap, out_len), runs a
// small program of polar-box commands on four box registers (all start as
// SmPolarBox()) and writes the concatenated outputs. A command is an op code followed
// by its arguments, all doubles; register indices are 0..3. The command set and its
// layout are shared with crates/solidcore-ffi/src/units/geom_polar_box.rs (which
// validates programs before the call; the shim validates again):
//
//  op args               action                                         output
//   0 r                  reg[r].ReSet()                                 -
//   1 r v3               reg[r] = SmPolarBox(v)                         -
//   2 r v3               reg[r].AddVector3d(v)                          -
//   3 r v3 b3            reg[r].AddVector3d(v, &b)                      -
//   4 r                  dump                                           state, basis 9, domain 4
//   5 r v3               reg[r].ComputePolarCoord(v)                    2
//   6 r                  reg[r].GetPolarArea()                          1
//   7 r a b              reg[r].EvaluateNormalized(a, b)                3
//   8 r                  reg[r].GetBoundaryVectors(n, vecs)             status [, n, n*3]
//   9 r v3 ang           reg[r].IsDisjointWithCone(v, ang)              1
//  10 r v3 ang           reg[r].IsPerpendicularDisjointWithCone(v, ang) 1
//  11 r s                reg[r].AreDisjoint(reg[s])                     1
//  12 r s t              SmPolarBox x; reg[r].Cross(reg[s], x); reg[t] = x       status
//  13 r s t              SmPolarBox x; reg[r].Union(reg[s], x); reg[t] = x       status
//  14 r s                reg[r].Union(reg[s], reg[r])  (r != s)                  status
//  15 r v3               reg[r].HasPerpendicularToVector(v)             1
//  16 r d                reg[r].ExpandAbsolute(d)                       -
//  17 t r                reg[t] = reg[r]                                -
//
// Union reads an unset corner count (undefined behaviour) when the boxes pass its
// axis tests but one is uninitialised; the shim detects that case beforehand,
// skips the call and writes status SM_ERR_INVALID_INPUT (the Rust port's error).
// Statuses are written as doubles (0 = SM_SUCCESS); booleans as 0/1; states as the
// SmPolarBoxState value.
#include "sc_shim.h"

#include <SmPolarBox.h>

#include <cmath>
#include <vector>

#include "sc_sm.h"

using sc::put_vec;

namespace
{
constexpr int kRegs = 4;

int64_t arg_count(int op)
{
    switch (op)
    {
    case 0: return 1;
    case 1: return 4;
    case 2: return 4;
    case 3: return 7;
    case 4: return 1;
    case 5: return 4;
    case 6: return 1;
    case 7: return 3;
    case 8: return 1;
    case 9: return 5;
    case 10: return 5;
    case 11: return 2;
    case 12: return 3;
    case 13: return 3;
    case 14: return 2;
    case 15: return 4;
    case 16: return 2;
    case 17: return 2;
    default: return -1;
    }
}

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

// Union's early exits, as in SmPolarBox::Union: true when it returns before reading
// boundary vectors.
bool union_exits_early(const SmPolarBox& a, const SmPolarBox& c)
{
    if (a.GetPolarBoxState() == SM_PS_UNBOUNDED || c.GetPolarBoxState() == SM_PS_UNBOUNDED)
        return true;
    SmVector3d sMid = *a.GetBasis(0) + *c.GetBasis(0);
    double dDot = a.GetBasis(0)->Dot(*c.GetBasis(0));
    return sMid.LengthSquared() < SM_EFF_ZERO_SQ || dDot < SM_EFF_ZERO;
}
} // namespace

SC_EXPORT int32_t sc_geom_polar_box_run(const double* prog, int64_t n_prog, double* out,
                                        int64_t cap, int64_t* out_len)
{
    if (n_prog < 0 || (n_prog > 0 && prog == nullptr))
        return SC_ERR_ARG;
    SC_TRY
    // validate the whole program first
    for (int64_t i = 0; i < n_prog;)
    {
        double opd = prog[i];
        if (!(opd >= 0.0 && opd <= 17.0) || opd != std::floor(opd))
            return SC_ERR_ARG;
        int op = (int)opd;
        int64_t n = arg_count(op);
        if (i + 1 + n > n_prog)
            return SC_ERR_ARG;
        int nregs = (op == 11 || op == 14 || op == 17) ? 2 : (op == 12 || op == 13) ? 3 : 1;
        int r;
        for (int k = 0; k < nregs; ++k)
            if (!reg_index(prog[i + 1 + k], r))
                return SC_ERR_ARG;
        if (op == 14 && prog[i + 1] == prog[i + 2])
            return SC_ERR_ARG;
        i += 1 + n;
    }

    SmPolarBox reg[kRegs];
    std::vector<double> o;
    for (int64_t i = 0; i < n_prog;)
    {
        int op = (int)prog[i];
        const double* a = prog + i + 1;
        i += 1 + arg_count(op);
        int r = 0, s = 0, t = 0;
        reg_index(a[0], r);
        switch (op)
        {
        case 0: reg[r].ReSet(); break;
        case 1: reg[r] = SmPolarBox(vec(a + 1)); break;
        case 2: reg[r].AddVector3d(vec(a + 1)); break;
        case 3:
        {
            SmVector3d b2 = vec(a + 4);
            reg[r].AddVector3d(vec(a + 1), &b2);
            break;
        }
        case 4:
        {
            o.push_back((double)reg[r].GetPolarBoxState());
            for (ULONG k = 0; k < 3; ++k)
                put_vec(o, *reg[r].GetBasis(k));
            SmExtent2d d = reg[r].GetDomain();
            o.push_back(d.GetMin().x);
            o.push_back(d.GetMin().y);
            o.push_back(d.GetMax().x);
            o.push_back(d.GetMax().y);
            break;
        }
        case 5:
        {
            SmPoint2d p = reg[r].ComputePolarCoord(vec(a + 1));
            o.push_back(p.x);
            o.push_back(p.y);
            break;
        }
        case 6: o.push_back(reg[r].GetPolarArea()); break;
        case 7: put_vec(o, reg[r].EvaluateNormalized(a[1], a[2])); break;
        case 8:
        {
            SmPolarBoxState e = reg[r].GetPolarBoxState();
            if (e == SM_PS_UNINITIALIZED || e == SM_PS_UNBOUNDED)
            {
                o.push_back((double)SM_ERR_INVALID_INPUT);
                break;
            }
            ULONG n = 0;
            SmVector3d v[4];
            SmStatus sts = reg[r].GetBoundaryVectors(n, v);
            o.push_back(st(sts));
            o.push_back((double)n);
            for (ULONG k = 0; k < n; ++k)
                put_vec(o, v[k]);
            break;
        }
        case 9: o.push_back(b(reg[r].IsDisjointWithCone(vec(a + 1), a[4]))); break;
        case 10: o.push_back(b(reg[r].IsPerpendicularDisjointWithCone(vec(a + 1), a[4]))); break;
        case 11:
            reg_index(a[1], s);
            o.push_back(b(reg[r].AreDisjoint(reg[s])));
            break;
        case 12:
        {
            reg_index(a[1], s);
            reg_index(a[2], t);
            SmPolarBox x;
            SmStatus sts = reg[r].Cross(reg[s], x);
            reg[t] = x;
            o.push_back(st(sts));
            break;
        }
        case 13:
        {
            reg_index(a[1], s);
            reg_index(a[2], t);
            if (!union_exits_early(reg[r], reg[s])
                && (reg[r].GetPolarBoxState() == SM_PS_UNINITIALIZED
                    || reg[s].GetPolarBoxState() == SM_PS_UNINITIALIZED))
            {
                o.push_back((double)SM_ERR_INVALID_INPUT);
                break;
            }
            SmPolarBox x;
            SmStatus sts = reg[r].Union(reg[s], x);
            reg[t] = x;
            o.push_back(st(sts));
            break;
        }
        case 14:
        {
            reg_index(a[1], s);
            // after ReSet + AddVector3d, reg[r] is a single point; only reg[s] can be
            // uninitialised here
            if (!union_exits_early(reg[r], reg[s])
                && reg[s].GetPolarBoxState() == SM_PS_UNINITIALIZED)
            {
                o.push_back((double)SM_ERR_INVALID_INPUT);
                break;
            }
            o.push_back(st(reg[r].Union(reg[s], reg[r])));
            break;
        }
        case 15: o.push_back(b(reg[r].HasPerpendicularToVector(vec(a + 1)))); break;
        case 16: reg[r].ExpandAbsolute(a[1]); break;
        case 17:
            reg_index(a[1], s);
            reg[r] = reg[s];
            break;
        default: return SC_ERR_ARG;
        }
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
