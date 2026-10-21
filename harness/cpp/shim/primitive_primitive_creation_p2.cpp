// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `primitive.primitive_creation_p2`: the public SmPrimitiveCreation
// entry points the unit owns (source/SMLib/src/SmPrimitiveCreation.cpp), each on a
// freshly constructed native B-rep. The file-local SweepProfilesAlongPath and
// sm_CreateSkinPrimitiveInternal are reached through their public callers.
//
// One entry point, sc_primitive_primitive_creation_p2_eval(op, data, n, out, cap, len).
// Layouts are documented in crates/solidcore-ffi/src/units/primitive_primitive_creation_p2.rs,
// which validates every input first. Curve records are p1's `[kind, x, y, z, a, b, c]`.
// Every op returns `status edit doing_boolean map0 map1 map2 map3 start side end`, then
// its extra values (skin: one ownership flag per curve; taper: the normal afterwards;
// planar path: whether a B-rep was returned).
#include "sc_topology_fixture.h"
#include <SmPrimitiveCreation.h>
#include <SmCompositeCurve.h>
#include <memory>

namespace {
SmBSplineCurve* pc2_curve(const double* data) {
    SmBSplineCurve* curve = nullptr;
    if (data[0] == 0) {
        SmBSplineCurve::CreateLineSegment(the_context(), 3,
            SmVector3d(data[1], data[2], data[3]), SmVector3d(data[4], data[5], data[6]), curve);
    } else {
        SmAxis2Placement frame(SmVector3d(data[1], data[2], data[3]), SmVector3d(1, 0, 0), SmVector3d(0, 1, 0));
        SmBSplineCurve::CreateCircleSegment(the_context(), 3, frame, data[4], data[5], data[6], SM_CO_QUADRATIC, curve);
    }
    return curve;
}
SmAxis2Placement pc2_frame(const double* data) {
    return SmAxis2Placement(SmVector3d(data[0], data[1], data[2]), SmVector3d(data[3], data[4], data[5]),
                            SmVector3d(data[6], data[7], data[8]));
}
// Read `count` curve records starting at `data`; false when one cannot be built.
bool pc2_curves(const double* data, int64_t count, SmTArray<SmCurve*>& curves) {
    for (int64_t i = 0; i < count; ++i) {
        SmBSplineCurve* curve = pc2_curve(data + 7 * i);
        if (!curve) {
            for (ULONG j = 0; j < curves.GetSize(); ++j) delete curves[j];
            curves.ReSet();
            return false;
        }
        curves.Add(curve);
    }
    return true;
}
SmSweepOptions pc2_options(const double* data) {
    SmSweepOptions options;
    options.bCapEndsArg = data[0] != 0;
    options.bTranslationalSweep = data[1] != 0;
    options.bMoveProfileCenterToPath = data[2] != 0;
    options.bOrientProfilePerpendicularToPath = data[3] != 0;
    options.bMoveToProfile = data[4] != 0;
    return options;
}
void pc2_record(std::vector<double>& out, SmStatus status, const SmBrep* brep, SmPrimitiveCreation* primitive,
                const SmTArray<SmFace*>& starts, const SmTArray<SmFace*>& sides, const SmTArray<SmFace*>& ends) {
    out.push_back(st(status));
    out.push_back(brep ? flg(brep->m_bEditingEnabled) : 0.0);
    out.push_back(flg(the_context().GetDoingBoolean()));
    if (primitive) {
        out.push_back(primitive->GetMapToSame()->Count());
        out.push_back(primitive->GetMapToHigher()->Count());
        out.push_back(primitive->GetMapFromSame()->Count());
        out.push_back(primitive->GetMapFromLower()->Count());
    } else {
        out.insert(out.end(), {0.0, 0.0, 0.0, 0.0});
    }
    out.push_back(starts.GetSize());
    out.push_back(sides.GetSize());
    out.push_back(ends.GetSize());
}
}

SC_EXPORT int32_t sc_primitive_primitive_creation_p2_eval(int32_t op, const double* data, int64_t n,
    double* output, int64_t cap, int64_t* length) {
    if (!data || n < 1 || n > 4096 || op < 1 || op > 9) return SC_ERR_ARG;
    SC_TRY
    // Each case starts outside boolean mode, as the Rust side's fresh context does: a
    // failed SmMerge::ManifoldBoolean leaves the shared context in it (WR-191).
    const_cast<SmContext&>(the_context()).SetDoingBoolean(FALSE);
    std::vector<double> out;
    SmTArray<SmFace*> starts, sides, ends;
    if (op == 9) {
        // static CreateSweepAlongPlanarPath: tol cap_start cap_end move profile_count path_count, curves
        const int64_t profile_count = (int64_t)data[4], path_count = (int64_t)data[5];
        if (n != 6 + 7 * (profile_count + path_count)) return SC_ERR_ARG;
        SmTArray<SmCurve*> profiles, paths;
        if (!pc2_curves(data + 6, profile_count, profiles)) return SC_ERR_ARG;
        SmObjsDelete<SmCurve*> clean_profiles(&profiles);
        if (!pc2_curves(data + 6 + 7 * profile_count, path_count, paths)) return SC_ERR_ARG;
        SmObjsDelete<SmCurve*> clean_paths(&paths);
        SmBrep* result = nullptr;
        SmStatus status = SmPrimitiveCreation::CreateSweepAlongPlanarPath(the_context(), profiles, paths, data[0],
            data[1] != 0, data[2] != 0, data[3] != 0, result);
        std::unique_ptr<SmBrep> clean_result(result);
        pc2_record(out, status, nullptr, nullptr, starts, sides, ends);
        out.push_back(result ? 1.0 : 0.0);
        return sc_copy_out(out.data(), (int64_t)out.size(), output, cap, length);
    }
    SmBrep brep(SM_USE_DEFAULT, SM_USE_DEFAULT, const_cast<SmContext*>(&the_context()));
    brep.m_bEditingEnabled = data[0] != 0;
    SmPrimitiveCreation primitive(brep.GetInfiniteRegion());
    SmStatus status = SM_ERR;
    std::vector<double> extra;
    if (op == 1) {
        // edit h base top start end frame
        if (n != 15) return SC_ERR_ARG;
        status = primitive.CreateCone(data[1], data[2], data[3], data[4], data[5], pc2_frame(data + 6));
    } else if (op == 2) {
        // edit r start end frame
        if (n != 13) return SC_ERR_ARG;
        status = primitive.CreateSphere(data[1], data[2], data[3], pc2_frame(data + 4));
    } else if (op == 3) {
        // edit major minor start end frame
        if (n != 14) return SC_ERR_ARG;
        status = primitive.CreateTorus(data[1], data[2], data[3], data[4], pc2_frame(data + 5));
    } else if (op == 4) {
        // edit approx degree cap profile_count counts... curves...
        const int64_t profile_count = (int64_t)data[4];
        if (n < 5 + profile_count) return SC_ERR_ARG;
        const int64_t curve_count = (n - 5 - profile_count) / 7;
        if (n != 5 + profile_count + 7 * curve_count) return SC_ERR_ARG;
        SmTArray<ULONG> counts;
        for (int64_t i = 0; i < profile_count; ++i) counts.Add((ULONG)data[5 + i]);
        SmTArray<SmCurve*> curves;
        if (!pc2_curves(data + 5 + profile_count, curve_count, curves)) return SC_ERR_ARG;
        SmTArray<SmCurve*> originals(curves);
        SmTArray<SmBoolean> transferred;
        status = primitive.CreateSkinPrimitive(counts, curves, data[1], (ULONG)data[2], data[3] != 0, starts, sides,
            ends, transferred);
        for (ULONG i = 0; i < originals.GetSize(); ++i) {
            const bool consumed = i < transferred.GetSize() && transferred[i];
            extra.push_back(consumed ? 1.0 : 0.0);
            if (!consumed) delete originals[i];
        }
    } else if (op == 5 || op == 6) {
        // edit approx options(5) path: an empty edge or face list
        if (n != 14) return SC_ERR_ARG;
        std::unique_ptr<SmBSplineCurve> path(pc2_curve(data + 7));
        if (!path) return SC_ERR_ARG;
        SmSweepOptions options = pc2_options(data + 2);
        if (op == 5) {
            SmTArray<SmEdge*> edges;
            status = primitive.CreateCurveSweepFromEdges(edges, path.get(), NULL, NULL, data[1], &options, starts,
                sides, ends);
        } else {
            SmTArray<SmFace*> faces;
            status = primitive.CreateCurveSweepFromFaces(faces, path.get(), NULL, NULL, data[1], &options, starts,
                sides, ends);
        }
    } else if (op == 7) {
        // edit radius approx cap translational path
        if (n != 12) return SC_ERR_ARG;
        std::unique_ptr<SmBSplineCurve> path(pc2_curve(data + 5));
        if (!path) return SC_ERR_ARG;
        status = primitive.CreatePipeSweep(data[1], path.get(), data[2], data[3] != 0, data[4] != 0, starts, sides,
            ends);
    } else {
        // edit height draft caps nx ny nz corner curves...
        if (n < 8 || (n - 8) % 7 != 0) return SC_ERR_ARG;
        SmTArray<SmCurve*> curves;
        if (!pc2_curves(data + 8, (n - 8) / 7, curves)) return SC_ERR_ARG;
        SmVector3d normal(data[4], data[5], data[6]);
        status = primitive.CreateTaperExtrude(curves, data[1], data[2], (int)data[3], normal,
            (SmOffsetCornerType)(int)data[7]);
        // On failure the (possibly reordered, partly deleted) curves stay with the caller.
        if (status != SM_SUCCESS)
            for (ULONG i = 0; i < curves.GetSize(); ++i) delete curves[i];
        put_vec(extra, normal);
    }
    pc2_record(out, status, &brep, &primitive, starts, sides, ends);
    out.insert(out.end(), extra.begin(), extra.end());
    return sc_copy_out(out.data(), (int64_t)out.size(), output, cap, length);
    SC_CATCH
}
