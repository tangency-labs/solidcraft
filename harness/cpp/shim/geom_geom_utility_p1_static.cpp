// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `geom.geom_utility_p1`: the file-static helpers sm_AddPoint and
// sm_AddClosestPoint of upstream source/SMLib/src/SmGeomUtility.cpp.
//
// Static functions cannot be linked against, so this translation unit compiles the
// upstream source file itself inside the namespace sc_gu_p1_tu (same flags as the
// kernel). Everything SmGeomUtility.cpp includes is included first, at global scope,
// except SmGeomUtility.h, so the smgu_* declarations and definitions all land in the
// namespace and do not clash with the library's global symbols. Only the two static
// helpers are used from the copy; every other smgu_* entry point is called in the
// library through geom_geom_utility_p1.cpp.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmContext.h>
#include <SmExtent1d.h>
#include <SmMatrix.h>
#include <SmPolynomial.h>
#include <SmTArray.h>
#include <SmVector3d.h>

namespace sc_gu_p1_tu
{
// SmGeomUtility.h forward-declares `class SmMatrix;`; make that name the global class.
using ::SmMatrix;
#include "../../../upstream/source/SMLib/src/SmGeomUtility.cpp"
} // namespace sc_gu_p1_tu

// sm_AddPoint(rlNum, aPoints, dTol, crPnt). io = [n, p0(3), p1(3)] in and out
// (n in 0..=2). Returns 0 and writes *status = the SmStatus.
SC_EXPORT int32_t sc_geom_geom_utility_p1_add_point(double* io, double tol, const double* pnt,
                                                    int32_t* status)
{
    if (io == nullptr || pnt == nullptr || status == nullptr)
        return SC_ERR_ARG;
    if (!(io[0] == 0.0 || io[0] == 1.0 || io[0] == 2.0))
        return SC_ERR_ARG;
    SC_TRY
    ULONG n = (ULONG)io[0];
    SmPoint3d pts[2] = {SmPoint3d(io[1], io[2], io[3]), SmPoint3d(io[4], io[5], io[6])};
    SmStatus st = sc_gu_p1_tu::sm_AddPoint(n, pts, tol, SmPoint3d(pnt[0], pnt[1], pnt[2]));
    *status = (int32_t)st;
    io[0] = (double)n;
    for (int i = 0; i < 2; ++i)
    {
        io[1 + 3 * i] = pts[i].x;
        io[2 + 3 * i] = pts[i].y;
        io[3 + 3 * i] = pts[i].z;
    }
    return SC_OK;
    SC_CATCH
}

// sm_AddClosestPoint(dDist, dParam1, dParam2, dTolerance, rlNum, ad1, ad2, adD).
// io = [n, p1[2], p2[2], d[2]] in and out (n in 0..=2).
SC_EXPORT int32_t sc_geom_geom_utility_p1_add_closest_point(double* io, double dist, double param1,
                                                            double param2, double tol, int32_t* status)
{
    if (io == nullptr || status == nullptr)
        return SC_ERR_ARG;
    if (!(io[0] == 0.0 || io[0] == 1.0 || io[0] == 2.0))
        return SC_ERR_ARG;
    SC_TRY
    ULONG n = (ULONG)io[0];
    double a1[2] = {io[1], io[2]}, a2[2] = {io[3], io[4]}, ad[2] = {io[5], io[6]};
    SmStatus st = sc_gu_p1_tu::sm_AddClosestPoint(dist, param1, param2, tol, n, a1, a2, ad);
    *status = (int32_t)st;
    io[0] = (double)n;
    io[1] = a1[0];
    io[2] = a1[1];
    io[3] = a2[0];
    io[4] = a2[1];
    io[5] = ad[0];
    io[6] = ad[1];
    return SC_OK;
    SC_CATCH
}
