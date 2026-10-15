// SPDX-License-Identifier: Apache-2.0
// query.face_p1: calls the actual SmApiEvaluateCurve entry point.
// Upstream: source/SM_API/src/SmApiCurves.cpp:1043-1080.
#include "sc_shim.h"
#include "StdAfx.h"
#include <SmApiCurves.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmLine.h>
#include <SmHermiteCurve.h>
#include <memory>

SC_EXPORT int32_t sc_query_face_p1_curve(int32_t kind, const double* data,
                                        double parameter, int32_t mask,
                                        double* out, int32_t* operation_status)
{
    if (!data || !out || !operation_status || kind < 0 || kind > 2 || mask < 0 || mask > 7)
        return SC_ERR_ARG;
    SC_TRY
    SmContext context;
    auto vec = [&](int i) { return SmVector3d(data[i], data[i+1], data[i+2]); };
    std::unique_ptr<SmCurve> curve;
    if (kind == 1) curve.reset(new (context) SmLine(vec(0), vec(6), 3, &context));
    if (kind == 2) curve.reset(new (context) SmHermiteCurve(vec(0), vec(3), vec(6), vec(9), 3));
    SmVector3d point(11, 13, 17), d1(19, 23, 29), d2(31, 37, 41);
    SmVector3d* p = (mask & 1) ? &point : nullptr;
    SmVector3d* q = (mask & 2) ? &d1 : nullptr;
    SmVector3d* r = (mask & 4) ? &d2 : nullptr;
    *operation_status = SmApiEvaluateCurve(curve.get(), parameter, p, q, r);
    const SmVector3d values[3] = {point, d1, d2};
    for (int i = 0; i < 3; ++i)
        for (int j = 0; j < 3; ++j) out[i*3+j] = values[i][j];
    return 0;
    SC_CATCH
}


// Reuse the shared, validated constructor callback rather than reparse its
// fixture protocol. Native objects and traversal identities stay inside apply.
#include "support/topology_fixture.h"
#include <SmBrep.h>
#include <SmFace.h>
#include <SmShell.h>
#include <SmRegion.h>
#include <SmExtent3d.h>
SC_EXPORT int32_t sc_query_face_p1_box(int32_t target, int64_t index,
    int32_t tight, int32_t use_default, const char* path, const double* input,
    int64_t n_in, double* out, int64_t cap, int64_t* out_len)
{
    if (target < 0 || target > 2 || index < 0 || !input || n_in < 1)
        return SC_ERR_ARG;
    SC_TRY
    return sc_topology_fixture::apply(path,input,n_in,[&](const sc_topology_fixture::View& view)->int32_t {
        SmStatus status=SM_SUCCESS;
        SmExtent3d box;box.SetMinMax(SmPoint3d(-17,-19,-23),SmPoint3d(29,31,37));
        if (target == 0) {
            if ((uint64_t)index >= view.faces.size()) return SC_ERR_ARG;
            SmFace* face=view.faces[(size_t)index];
            if (!face->GetSurface()) return SC_ERR_ARG;
            status=use_default ? face->CalculateBoundingBox(box) : face->CalculateBoundingBox(box,tight);
        } else if (target == 1) {
            if ((uint64_t)index >= view.shells.size()) return SC_ERR_ARG;
            SmShell* shell=view.shells[(size_t)index];
            if (shell->IsVertexShell() && !shell->GetVertex()) return SC_ERR_ARG;
            status=use_default ? shell->CalculateBoundingBox(box) : shell->CalculateBoundingBox(box,tight);
        } else {
            if ((uint64_t)index >= view.regions.size()) return SC_ERR_ARG;
            SmRegion* region=view.regions[(size_t)index];
            status=use_default ? region->CalculateBoundingBox(box) : region->CalculateBoundingBox(box,tight);
        }
        const SmPoint3d lo=box.GetMin(),hi=box.GetMax();
        const double result[7]={(double)status,lo.x,lo.y,lo.z,hi.x,hi.y,hi.z};
        return sc_copy_out(result,7,out,cap,out_len);
    });
    SC_CATCH
}
