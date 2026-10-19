// SPDX-License-Identifier: Apache-2.0
// Actual SmFilletExecutive p3 calls. The derived fixture sets all control flags
// explicitly: UB-185, the upstream constructor leaves m_bIsInitialized uninitialized.
// Layout: op target initialized prior_boolean editing want_error seed_code tol
//         piecewise insertion global trimming classification.
// Empty targets exercise native no-geometry execution, not a mock executive.
#include "sc_shim.h"
#include "StdAfx.h"
#include <SmBrep.h>
#include <SmApiFillets.h>
#include <SmApiBrep.h>
#include <SmApiPrimitives.h>
#include <SmContext.h>
#include <SmFilletExecutive.h>
#include <cstring>
#include <vector>
#include <memory>

namespace {
// Read a private observation without modifying the upstream class or layout.
// Explicit template instantiation may name private members (C++ [temp.explicit]).
struct FilletStatusMember {
    using type = SmFilStatus SmFilletErrorInfo::*;
    friend type member(FilletStatusMember);
};
template<class Tag, typename Tag::type Pointer> struct ObserveMember {
    friend typename Tag::type member(Tag) { return Pointer; }
};
template struct ObserveMember<FilletStatusMember, &SmFilletErrorInfo::m_eFilletStatus>;
SmFilStatus fillet_status(const SmFilletErrorInfo& info) {
    return info.*member(FilletStatusMember{});
}
struct BrepTolerance : SmBrep {
    static SmZoneTol3d SmBrep::* value() { return &BrepTolerance::m_sThisZoneTol3d; }
};
struct Executive : SmFilletExecutive {
    Executive(const SmContext& context, SmBrep* target) : SmFilletExecutive(context, target) {}
    void configure(const double* in) {
        m_bIsInitialized = in[2] != 0;
        m_bDoPiecewiseMerge = in[8] != 0;
        m_bDoTopologyInsertion = in[9] != 0;
        m_bDoGlobalMerge = in[10] != 0;
        m_bDoTrimming = in[11] != 0;
        m_bDoClassification = in[12] != 0;
        m_sErrorInfo.SetErrorCode(static_cast<SmFilletErrorType>(static_cast<int>(in[6])));
    }
    SmBrep* fillet() { return m_pFilletBrep; }
    bool doing_fillets() { return m_vTI.m_bDoingFillets != FALSE; }
    SmFilletErrorInfo& info() { return m_sErrorInfo; }
    ~Executive() {
        // The upstream destructor does not own/delete this preview B-rep.
        // Remove intersector references before deleting a remaining preview.
        if (m_pFilletBrep) {
            if (m_vTI.GetPrimaryBrep() == m_pFilletBrep) m_vTI.SetPrimaryBrep(NULL);
            if (m_vTI.GetOtherBrep() == m_pFilletBrep) m_vTI.SetOtherBrep(NULL);
            delete m_pFilletBrep;
            m_pFilletBrep = NULL;
        }
    }
};
}

SC_EXPORT int32_t sc_blend_fillet_executive_p3_eval(const double* in, int64_t n,
    double* out, int64_t cap, int64_t* out_len) {
    if (!in || n != 13) return SC_ERR_ARG;
    SC_TRY
    SmContext context;
    context.SetDoingBoolean(in[3] != 0);
    SmBrep* target = in[1] != 0 ? new (context) SmBrep() : NULL;
    if (target) {
        // Fixture member assignment, not a test of the separately claimed SetTolerance.
        target->*BrepTolerance::value() = SmZoneTol3d(in[7]);
        target->m_bEditingEnabled = in[4] != 0;
    }
    std::vector<double> values;
    {
        Executive executive(context, target);
        executive.configure(in);
        SmFilletErrorInfo error_out;
        error_out.SetErrorCode(SM_FILERR_BAD_INT);
        std::strcpy(error_out.m_cComment, "sentinel");
        SmFilletErrorInfo* optional = in[5] != 0 ? &error_out : NULL;
        SmStatus status;
        switch (static_cast<int>(in[0])) {
            case 0: status = executive.DoFillet(); break;
            case 1: status = executive.CreateFilletBrep(optional); break;
            case 2: status = executive.DoFilleting(optional); break;
            default: delete target; return SC_ERR_ARG;
        }
        SmFilletErrorInfo& error = executive.info();
        SmBrep* fillet = executive.fillet();
        values = {
            status == SM_SUCCESS ? 0.0 : static_cast<double>(status),
            static_cast<double>(error.GetErrorCode()),
            static_cast<double>(fillet_status(error)),
            static_cast<double>(error_out.GetErrorCode()),
            static_cast<double>(fillet_status(error_out)),
            context.GetDoingBoolean() != FALSE ? 1.0 : 0.0,
            target ? (target->m_bEditingEnabled != FALSE ? 1.0 : 0.0) : -1.0,
            executive.doing_fillets() ? 1.0 : 0.0,
            fillet ? 1.0 : 0.0,
            target ? static_cast<double>(target->GetTolerance()) : -1.0,
            fillet ? static_cast<double>(fillet->GetTolerance()) : -1.0,
            static_cast<double>(std::strlen(error.m_cComment)),
            static_cast<double>(std::strlen(error_out.m_cComment)),
            // Boundary encoding uses the upstream enum discriminants.
            static_cast<double>(error.m_eCornerType),
            static_cast<double>(error_out.m_eCornerType)
        };
        for (size_t i=0; i<std::strlen(error.m_cComment); ++i)
            values.push_back(static_cast<unsigned char>(error.m_cComment[i]));
        for (size_t i=0; i<std::strlen(error_out.m_cComment); ++i)
            values.push_back(static_cast<unsigned char>(error_out.m_cComment[i]));
    }
    delete target;
    return sc_copy_out(values.data(), static_cast<int64_t>(values.size()), out, cap, out_len);
    SC_CATCH
}

// Fixed examples from tests/SM_API_test/src/TestSmFillets.cpp. The selector is
// fixture data, not a public filleting parameter; each example keeps its original
// dimensions/radii/edge lists. Actual API execution reaches the executive unit.
SC_EXPORT int32_t sc_blend_fillet_executive_p3_upstream(int32_t example,
    double* out, int64_t cap, int64_t* out_len) {
    if (example < 0 || example > 6) return SC_ERR_ARG;
    SC_TRY
    SmBrep* box = NULL;
    SmStatus setup = example < 2
        ? SmApiCreateBox(SmVector3d(0,0,0), 5,10,15,box)
        : SmApiCreateBox(SmVector3d(0,0,0), 10,10,10,box);
    if (setup != SM_SUCCESS || !box) return SC_ERR_ARG;
    std::unique_ptr<SmBrep> owner(box);
    SmTArray<SmEdge*> edges;
    box->GetEdges(edges);
    SmTArray<SmEdge*> selected;
    for (ULONG i=0; i<edges.GetSize() && i<4; ++i) selected.Add(edges[i]);
    SmStatus status = SM_ERR;
    if (example == 0) status = SmApiCircularFillet(box, 0.5);
    else if (example == 1) status = SmApiChamferFillet(box, 0.5);
    else if (example == 2) status = SmApiFilletEdges(box,selected,1.0,1,1,1.0);
    else if (example == 3) {
        SmTArray<double> radii;
        SmTArray<SmFilletSurfaceGeneratorType> types;
        for (ULONG i=0; i<selected.GetSize(); ++i) {
            radii.Add(0.5+0.25*i); types.Add(SM_FSG_CIRCULAR);
        }
        status = SmApiFilletEdgesPerEdge(box,selected,radii,types,1,1.0);
    } else if (example == 4) status = SmApiVariableRadiusFillet(box,selected,0.5,2.0,1,1);
    else {
        SmTArray<SmVertex*> vertices, bevel;
        box->GetVertices(vertices);
        if (example == 6 && vertices.GetSize() > 0) bevel.Add(vertices[0]);
        status = SmApiSetBevelCorners(box,edges,1.0,bevel);
    }
    // Every topology lookup is fresh after the mutating operation.
    SmTArray<SmEdge*> result_edges;
    SmTArray<SmVertex*> result_vertices;
    box->GetEdges(result_edges);
    box->GetVertices(result_vertices);
    double values[] = { status == SM_SUCCESS ? 0.0 : static_cast<double>(status),
        static_cast<double>(box->GetNumFaces()), static_cast<double>(result_edges.GetSize()),
        static_cast<double>(result_vertices.GetSize()) };
    return sc_copy_out(values,4,out,cap,out_len);
    SC_CATCH
}
