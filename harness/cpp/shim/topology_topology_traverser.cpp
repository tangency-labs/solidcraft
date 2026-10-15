// SPDX-License-Identifier: Apache-2.0
// Real upstream topology traversal, with stable indices from the raw fixture graph.
#include "sc_topology_fixture.h"
#include <SmTopologyTraverser.h>

SC_EXPORT int32_t sc_topology_topology_traverser_eval(
    int32_t op, const char* path, const double* in, int64_t n_in,
    int64_t start, int64_t target, int32_t mask, int32_t want, int32_t pre,
    int32_t prefill, int32_t repeat, double* out, int64_t cap, int64_t* out_len)
{
    if (!path || !in || n_in < 1 || !out_len || op < 0 || op > 8 || repeat < 1 || repeat > 2)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    SmStatus status;
    Built built;
    built.brep = build(rd, path, status);
    std::vector<double> o;
    if (!built.brep) { o.push_back(-1); o.push_back(st(status)); }
    else {
        Graph g;
        walk(built.brep, g, true);
        if (op == 8) {
            std::vector<double> raw{0.0};
            export_graph(built.brep, g, raw);
            o.push_back((double)raw.size());
            o.insert(o.end(), raw.begin(), raw.end());
            for (auto* vertex : g.vertices) {
                std::vector<int64_t> ids;
                for (auto* vu : list_of(vertex->GetList())) ids.push_back(g.find(8,vu));
                put_list(o, ids);
            }
            return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
        }
        SmNewMarkAndLock mark(&the_context(), mask);
        SmMarkType mt = mark.GetMarkType();
        SmTopologyTraverser traverser;
        auto seed = [&](SmTopology* p) { if (p) p->Mark(mt); };
        if (pre & 1) {
            if (op == 0 && start >= 0) seed(g.faces.at(start));
            if (op == 1 && start >= 0) seed(g.edges.at(start));
            if ((op == 2 || op == 3 || op == 5) && start >= 0) seed(g.faceuses.at(start));
        }
        if ((pre & 2) && !g.edges.empty()) seed(g.edges[0]);
        if ((pre & 4) && !g.vertices.empty()) seed(g.vertices[0]);
        if ((pre & 8) && !g.faceuses.empty()) seed(g.faceuses[0]);
        if ((pre & 16) && !g.vertexuses.empty()) seed(g.vertexuses[0]);
        SmTArray<SmFace*> fs;
        SmTArray<SmEdge*> es;
        SmTArray<SmVertex*> vs;
        SmTArray<SmFaceuse*> fus;
        SmTArray<SmEdgeuse*> eus;
        if (prefill) {
            if (!g.faces.empty()) fs.Add(g.faces[0]);
            if (!g.edges.empty()) es.Add(g.edges[0]);
            if (!g.vertices.empty()) vs.Add(g.vertices[0]);
            if (!g.faceuses.empty()) fus.Add(g.faceuses[0]);
            if (!g.edgeuses.empty()) eus.Add(g.edgeuses[0]);
        }
        SmBoolean closed = UNSURE;
        for (int n=0; n<repeat; ++n) {
            if (op == 0) status = traverser.CollectFaces(start < 0 ? nullptr : g.faces.at(start), fs, mt,
                                                        (want & 1) ? &es : nullptr, (want & 2) ? &vs : nullptr);
            if (op == 1) status = traverser.CollectWireEdges(start < 0 ? nullptr : g.edges.at(start), es, mt,
                                                           (want & 2) ? &vs : nullptr);
            if (op == 2) status = traverser.FaceuseTraversal(g.faceuses.at(start), closed, (want & 1) ? &fus : nullptr, mark);
            if (op == 3) status = traverser.ClosureTraversal(g.faceuses.at(start), closed, (want & 1) ? &fus : nullptr, mark);
            if (op == 4) status = traverser.ShellTraversal(g.vertices.at(start), g.shells.at(target),
                                                          (want & 1) ? &eus : nullptr, (want & 2) ? &fus : nullptr, mark);
            if (op == 5) status = traverser.FaceuseShellReplacement(g.faceuses.at(start), g.shells.at(target), fus, mark);
            if (op == 6) status = traverser.RemoveShell(g.shells.at(start), g.shells.at(target));
            if (op == 7) {
                fus.ReSet();
                for (auto* fu : g.faceuses) if (fu->GetShell() == g.shells.at(target)) fus.Add(fu);
                status = traverser.IsInnerShell(fus, closed);
            }
            if (status != SM_SUCCESS) break;
        }
        o.push_back(st(status));
        if (op == 2 || op == 3 || op == 7) o.push_back((double)closed);
        auto put = [&](auto& list, int kind) {
            o.push_back((double)list.GetSize());
            for (ULONG i=0;i<list.GetSize();i++) o.push_back((double)g.find(kind, list[i]));
        };
        if (op == 0) { put(fs, 3); if(want & 1) put(es,7); if(want & 2) put(vs,9); }
        if (op == 1) { put(es, 7); if(want & 2) put(vs,9); }
        if ((op == 2 || op == 3) && (want & 1)) put(fus,2);
        if (op == 4) { if(want & 1) put(eus,6); if(want & 2) put(fus,2); }
        if (op == 5) put(fus,2);
        for (auto* p : g.faces) o.push_back(flg(p->IsMarked(mt)));
        for (auto* p : g.edges) o.push_back(flg(p->IsMarked(mt)));
        for (auto* p : g.vertices) o.push_back(flg(p->IsMarked(mt)));
        for (auto* p : g.faceuses) o.push_back(flg(p->IsMarked(mt)));
        for (auto* p : g.vertexuses) o.push_back(flg(p->IsMarked(mt)));
        for (auto* p : g.faceuses) o.push_back((double)g.find(1,p->GetShell()));
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
