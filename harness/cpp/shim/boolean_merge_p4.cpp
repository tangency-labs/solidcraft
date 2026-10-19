// SPDX-License-Identifier: Apache-2.0
// Real ManifoldBoolean entry point. Input graphs are exported before mutation;
// every post-operation snapshot walks surviving topology afresh.
#include "sc_topology_fixture.h"
#include <SmMerge.h>
#include <memory>

namespace {
SmBrep* make_operand(const SmContext& ctx, const double* input, SmStatus& status) {
    int kind = (int)input[0];
    status = SM_SUCCESS;
    if (kind == -1) return nullptr;
    auto* b = new (ctx) SmBrep(SM_USE_DEFAULT, SM_USE_DEFAULT, (SmContext*)&ctx);
    SmPoint3d p(input[1],input[2],input[3]);
    if (kind == 1) {
        SmShell* s = nullptr; SmVertex* v = nullptr;
        status = b->MakeShellVertex(b->GetInfiniteRegion(),p,s,v);
    } else if (kind == 2) {
        SmBSplineCurve* c = nullptr;
        status = SmBSplineCurve::CreateLineSegment(ctx,3,p,p+SmVector3d(input[4],input[5],input[6]),c);
        if (status == SM_SUCCESS) { SmEdge* e = nullptr; status = b->CreateWireEdgeFromCurve(c,c->GetNaturalInterval(),e); }
    } else if (kind == 3) {
        auto delete_unowned = [](SmPlane* s) { if (s && !s->GetOwner()) delete s; };
        std::unique_ptr<SmPlane,decltype(delete_unowned)> s(
            new (ctx) SmPlane(p,SmVector3d(0,0,1),&ctx), delete_unowned);
        SmFace* f = nullptr;
        status = b->CreateFaceFromSurface(s.get(),SmExtent2d(SmPoint2d(0,0),SmPoint2d(input[4],input[5])),f);
        // A UV-domain rejection happens before the surface gets a face owner.
        // Delete only unconsumed geometry; attached surfaces belong to the B-rep.
    }
    if (status != SM_SUCCESS) { delete b; return nullptr; }
    return b;
}

struct Access : SmMerge {
    Access(const SmContext& c, SmBrep* a, SmBrep* b, const int32_t* flags, bool manifold) : SmMerge(c) {
        m_vTI.SetPrimaryBrep(a); m_vTI.SetOtherBrep(b);
        m_vTI.SetThisApproxTol3d(1e-9); m_vTI.SetThisAngTolRad(0.3);
        m_bManifoldBoolean = manifold ? TRUE : FALSE;
        m_bIntersectionLoopClosed = TRUE;
        SetCookieCutter(flags[0] != 0); SetImprinting(flags[1] != 0);
        SetBooleanPostProcess(flags[2] != 0); SetClassifyFaces(flags[3] != 0); SetKeepOtherBrep(flags[4] != 0);
    }
    ~Access() {
        // Read current operand pointers: ManifoldBoolean may have deleted either.
        delete m_vTI.GetPrimaryBrep(); delete m_vTI.GetOtherBrep();
        for (ULONG i=0;i<m_vOneBrepPer_OrigBrepConnectedFaceSet.GetSize();++i) delete m_vOneBrepPer_OrigBrepConnectedFaceSet[i];
        for (ULONG i=0;i<m_vOneBrepPer_OtherBrepConnectedFaceSet.GetSize();++i) delete m_vOneBrepPer_OtherBrepConnectedFaceSet[i];
        m_vTI.SetPrimaryBrep(nullptr); m_vTI.SetOtherBrep(nullptr);
    }
    void snapshot(std::vector<double>& out, SmBrep* result, const SmContext& a, const SmContext& b) {
        out.insert(out.end(),{result != nullptr ? 1.0:0.0, flg(a.GetDoingBoolean()),flg(b.GetDoingBoolean()),flg(m_bIntersectionLoopClosed),
            (double)m_sIntersectionEdges.GetSize(),(double)m_sIntersectionVertices.GetSize(),
            (double)m_vOneBrepPer_OrigBrepConnectedFaceSet.GetSize(),(double)m_vOneBrepPer_OtherBrepConnectedFaceSet.GetSize()});
        for (auto* brep : {m_vTI.GetPrimaryBrep(),m_vTI.GetOtherBrep()}) {
            out.push_back(brep ? 1.0:0.0);
            if (!brep) continue;
            Graph g; walk(brep,g,true);
            std::vector<double> record{flg(brep->m_bEditingEnabled)};
            export_graph(brep,g,record);
            out.push_back((double)record.size());
            out.insert(out.end(),record.begin(),record.end());
        }
        // Counts alone cannot certify extraction after the operands are deleted.
        // Prefix each complete fresh graph with its record length for decoding.
        for (auto* components : {&m_vOneBrepPer_OrigBrepConnectedFaceSet,
                                 &m_vOneBrepPer_OtherBrepConnectedFaceSet}) {
            for (ULONG i=0; i<components->GetSize(); ++i) {
                SmBrep* brep = (*components)[i];
                std::vector<double> component;
                if (brep) {
                    Graph g; walk(brep,g,true);
                    component.push_back(flg(brep->m_bEditingEnabled));
                    export_graph(brep,g,component);
                }
                out.push_back((double)component.size());
                out.insert(out.end(),component.begin(),component.end());
            }
        }
    }
};
}

SC_EXPORT int32_t sc_boolean_merge_p4_eval(int32_t mode, const double* a, const double* b,
    int32_t operation, const int32_t* flags, int32_t manifold, int32_t distinct_contexts,
    int32_t caller_lock, double* out, int64_t cap, int64_t* len) {
    if (!a || !b || !flags || !len || (mode != -1 && mode != 0)) return SC_ERR_ARG;
    SC_TRY
    SmContext ca, cb;
    const SmContext& other_context = distinct_contexts ? cb : ca;
    SmStatus status;
    std::unique_ptr<SmBrep> aa(make_operand(ca,a,status));
    if (a[0] != -1 && (status != SM_SUCCESS || !aa)) {
        double rejected[] = {-1.0}; return sc_copy_out(rejected,1,out,cap,len);
    }
    if (mode == -1) {
        std::vector<double> output{0.0};
        if (aa) { Graph g; walk(aa.get(),g,true); export_graph(aa.get(),g,output); }
        return sc_copy_out(output.data(),(int64_t)output.size(),out,cap,len);
    }
    std::unique_ptr<SmBrep> bb(make_operand(other_context,b,status));
    if (b[0] != -1 && (status != SM_SUCCESS || !bb)) {
        double rejected[] = {-1.0}; return sc_copy_out(rejected,1,out,cap,len);
    }
    // Mark lock is destroyed before contexts, after the merge; each invocation
    // owns all its state, including native contexts (no process-global fixtures).
    SmNewMarkAndLock lock;
    Access merge(ca,aa.release(),bb.release(),flags,manifold != 0);
    SmBrep* result = nullptr;
    status = merge.ManifoldBoolean((SmBooleanOperationType)operation,result,caller_lock ? &lock : nullptr);
    std::vector<double> output{st(status)};
    merge.snapshot(output,result,ca,other_context);
    return sc_copy_out(output.data(),(int64_t)output.size(),out,cap,len);
    SC_CATCH
}
