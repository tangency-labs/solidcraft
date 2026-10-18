// SPDX-License-Identifier: Apache-2.0
// Calls the real protected SmMerge::ProcessWiresAndShellVertices through a
// derived test accessor. Fixture graphs are exported BEFORE running the unit.
#include "sc_topology_fixture.h"
#include "sc_merge_fixture.h"
#include <SmMerge.h>
#include <memory>

namespace {
struct MergeAccess : SmMerge {
    MergeAccess(const SmContext& c, SmBrep* a, SmBrep* b) : SmMerge(c,a,b) {}
    using SmMerge::ProcessWiresAndShellVertices;
    SmStatus relate(SmObject* a, SmObject* b) { return m_vTI.Relate(a,b); }
};

}

// mode -1 exports one operand, mode 0 runs the unit. Flat fixture blocks and
// mark/mate selectors are validated in the safe Rust FFI wrapper.
SC_EXPORT int32_t sc_boolean_merge_p5_eval(
    int32_t mode, const char* path_a, const double* a, int64_t na,
    const char* path_b, const double* b, int64_t nb,
    int32_t operation, int32_t mark_a, int32_t mark_b,
    const int64_t* marked_a, int64_t nma, const int64_t* marked_b, int64_t nmb,
    const int64_t* mates, int64_t nm, int32_t prefill, int32_t repeat,
    double* out, int64_t cap, int64_t* out_len) {
    if (!a || !path_a || na < 1 || !out_len) return SC_ERR_ARG;
    SC_TRY
    Built aa; SmStatus status;
    aa.brep = operand(path_a,a,status);
    if (!aa.brep || status != SM_SUCCESS) return SC_ERR_ARG;
    Graph ga; native_graph(aa.brep,ga);
    if (mode == -1) {
        std::vector<double> raw{0.0};
        export_graph(aa.brep,ga,raw);
        std::vector<double> result{(double)raw.size()};
        result.insert(result.end(),raw.begin(),raw.end());
        for (auto* v : ga.vertices) {
            std::vector<int64_t> list;
            for (auto* vu : list_of(v->GetList())) list.push_back(ga.find(8,vu));
            put_list(result,list);
        }
        return sc_copy_out(result.data(),(int64_t)result.size(),out,cap,out_len);
    }
    if (mode != 0 || !b || !path_b || nb < 1)
        return SC_ERR_ARG;
    Built bb; bb.brep = operand(path_b,b,status);
    if (!bb.brep || status != SM_SUCCESS) return SC_ERR_ARG;
    Graph gb; native_graph(bb.brep,gb);
    if (nma != (int64_t)(ga.edges.size()+ga.vertices.size()) ||
        nmb != (int64_t)(gb.edges.size()+gb.vertices.size()) || nm != (int64_t)gb.edges.size())
        return SC_ERR_ARG;
    // Only these operations append the mate to keep. Upstream permits NULL
    // here, but Rust's Vec<EdgeId> cannot represent it. Other operations never
    // use the mate, so unmated marked wires remain valid comparison inputs.
    if (operation == 1 || operation == 2 || operation == 5 || operation == 6)
        for (size_t i=0;i<gb.edges.size();++i)
            if (marked_b[i] && gb.edges[i]->IsWire() && mates[i] < 0) return SC_ERR_ARG;
    SmNewMarkAndLock la(&the_context(),mark_a);
    std::unique_ptr<SmNewMarkAndLock> lb;
    if (mark_a != mark_b) lb = std::make_unique<SmNewMarkAndLock>(&the_context(),mark_b);
    auto ma = la.GetMarkType(); auto mb = lb ? lb->GetMarkType() : ma;
    MergeAccess merge(the_context(),aa.brep,bb.brep);
    for (size_t i=0;i<ga.edges.size();++i) if(marked_a[i]) ga.edges[i]->Mark(ma);
    for (size_t i=0;i<ga.vertices.size();++i) if(marked_a[ga.edges.size()+i]) ga.vertices[i]->Mark(ma);
    for (size_t i=0;i<gb.edges.size();++i) {
        if (marked_b[i]) gb.edges[i]->Mark(mb);
        if (mates[i] >= 0) {
            if ((size_t)mates[i] >= ga.edges.size()) return SC_ERR_ARG;
            status = merge.relate(ga.edges[mates[i]],gb.edges[i]);
            if(status != SM_SUCCESS) return SC_ERR_ARG;
        }
    }
    for (size_t i=0;i<gb.vertices.size();++i) if(marked_b[gb.edges.size()+i]) gb.vertices[i]->Mark(mb);
    SmTArray<SmEdge*> keep;
    std::vector<int64_t> ids;
    for (int iteration=0;iteration<repeat;++iteration) {
        if(prefill && !ga.edges.empty()) keep.Add(ga.edges[0]);
        status = merge.ProcessWiresAndShellVertices((SmBooleanOperationType)operation,keep,ma,mb);
        ids.push_back(status == SM_SUCCESS ? 0 : (int64_t)status);
        ids.push_back((int64_t)keep.GetSize());
        for (ULONG i=0;i<keep.GetSize();++i) ids.push_back(ga.find(7,keep[i]));
        if(status != SM_SUCCESS) break;
    }
    std::vector<double> values;
    snapshot(aa.brep,ma,ids,values); snapshot(bb.brep,mb,ids,values);
    std::vector<double> result{(double)ids.size()};
    for(auto id : ids) result.push_back((double)id);
    result.insert(result.end(),values.begin(),values.end());
    return sc_copy_out(result.data(),(int64_t)result.size(),out,cap,out_len);
    SC_CATCH
}
