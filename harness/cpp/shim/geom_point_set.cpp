// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.point_set`. Wraps upstream SmPointSet3d / SmPointSequence
// (source/SMLib/src/SmPointSet.cpp, source/SMLib/inc/SmPointSet.h).
//
// sc_geom_point_set_run(pts, n_pts, tol, closed, has_normal, normal, prog, n_prog,
//                       out, cap, out_len)
// builds SmPointSequence(points, tol, closed, UNSURE, has_normal ? &normal : NULL)
// and runs a program of commands on it (the cached properties depend on call order).
// Output starts with the given normal after the constructor unitized it in place
// (3 doubles; 0,0,0 without a normal), then each command's output in order.
//
//  op floats  action                                                     output
//   0 -       GetCentroid()                                              3
//   1 -       GetNormal()                                                3
//   2 4       GetBBox(has ? &z : NULL)   (has, z)                        min 3, max 3, z 3
//   3 4       GetTightPBox(has ? &z : NULL)  (has, z)                    basis 9, intervals 6, z 3
//   4 -       GetArea()                                                  1
//   5 3       PointContainment(p)                                        1 (enum value)
//   6 -       SmPointSet3d::ClearCache()                                 -
//   7 -       GetPoints().GetSize()                                      1
//
// Rejected because upstream reads outside the empty point array: GetArea and
// PointContainment of an empty set (smgu_PolygonArea reads m_pData[-1];
// PointContainment is safe, but kept symmetric) and GetTightPBox of an empty set with
// a given normal (planar, so SetUpTightPseudoBox reads m_pData[0]).
// The table is shared with crates/solidcore-ffi/src/units/geom_point_set.rs.
#include "sc_shim.h"

#include <SmPointSet.h>
#include <SmPseudoBox.h>
#include <SmTArray.h>

#include <cmath>
#include <vector>

#include "sc_sm.h"

using sc::put_vec;

namespace
{
constexpr int kOps = 8;
const int kNArgs[kOps] = {0, 0, 4, 4, 0, 3, 0, 0};

} // namespace

SC_EXPORT int32_t sc_geom_point_set_run(const double* pts, int64_t n_pts, double tol,
                                        int32_t closed, int32_t has_normal,
                                        const double* normal, const double* prog,
                                        int64_t n_prog, double* out, int64_t cap,
                                        int64_t* out_len)
{
    if (n_pts < 0 || (n_pts > 0 && pts == nullptr) || n_prog < 0 ||
        (n_prog > 0 && prog == nullptr) || (has_normal && normal == nullptr))
        return SC_ERR_ARG;
    SC_TRY
    // validate the whole program first
    for (int64_t i = 0; i < n_prog;)
    {
        double opd = prog[i];
        if (!(opd >= 0.0 && opd < kOps) || opd != std::floor(opd))
            return SC_ERR_ARG;
        int op = (int)opd;
        if (i + 1 + kNArgs[op] > n_prog)
            return SC_ERR_ARG;
        if ((op == 2 || op == 3) && prog[i + 1] != 0.0 && prog[i + 1] != 1.0)
            return SC_ERR_ARG;
        if ((op == 4 || op == 5) && n_pts == 0)
            return SC_ERR_ARG;
        if (op == 3 && n_pts == 0 && has_normal)
            return SC_ERR_ARG;
        i += 1 + kNArgs[op];
    }

    SmTArray<SmPoint3d> arr;
    for (int64_t i = 0; i < n_pts; ++i)
        arr.Add(SmPoint3d(pts[3 * i], pts[3 * i + 1], pts[3 * i + 2]));
    SmVector3d nrm(0, 0, 0);
    if (has_normal)
        nrm.Set(normal[0], normal[1], normal[2]);
    SmPointSequence seq(arr, tol, closed ? TRUE : FALSE, UNSURE, has_normal ? &nrm : NULL);

    std::vector<double> o;
    put_vec(o, nrm);
    for (int64_t i = 0; i < n_prog;)
    {
        int op = (int)prog[i];
        const double* a = prog + i + 1;
        i += 1 + kNArgs[op];
        switch (op)
        {
        case 0: put_vec(o, seq.GetCentroid()); break;
        case 1: put_vec(o, seq.GetNormal()); break;
        case 2:
        {
            SmVector3d z(a[1], a[2], a[3]);
            SmExtent3d e = seq.GetBBox(a[0] != 0.0 ? &z : NULL);
            o.push_back(e.GetUMin());
            o.push_back(e.GetVMin());
            o.push_back(e.GetWMin());
            o.push_back(e.GetUMax());
            o.push_back(e.GetVMax());
            o.push_back(e.GetWMax());
            put_vec(o, z);
            break;
        }
        case 3:
        {
            SmVector3d z(a[1], a[2], a[3]);
            SmPseudoBox pb = seq.GetTightPBox(a[0] != 0.0 ? &z : NULL);
            for (ULONG k = 0; k < 3; ++k)
                put_vec(o, pb.GetBasis(k));
            for (ULONG k = 0; k < 3; ++k)
            {
                o.push_back(pb.GetInterval(k).GetMin());
                o.push_back(pb.GetInterval(k).GetMax());
            }
            put_vec(o, z);
            break;
        }
        case 4: o.push_back(seq.GetArea()); break;
        case 5: o.push_back((double)seq.PointContainment(SmPoint3d(a[0], a[1], a[2]))); break;
        case 6: seq.SmPointSet3d::ClearCache(); break;
        case 7: o.push_back((double)seq.GetPoints().GetSize()); break;
        }
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
