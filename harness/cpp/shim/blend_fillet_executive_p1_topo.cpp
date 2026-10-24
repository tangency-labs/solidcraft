// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `blend.fillet_executive_p1`: `SmFilletExecutive::RemoveTopoEdges`
// (source/SMLib/src/SmFilletExecutive.cpp 3979-4120) on a fixture B-rep
// (sc_topology_fixture.h). Layout in crates/solidcore-ffi/src/units/blend_fillet_executive_p1.rs
// (`remove_topo_edges`).
//
// Input: the fixture block, then the topo edges as indices into the walk of the built B-rep.
// Output: `-1` when the fixture does not build; `-3` when two listed edges share a vertex
// (the guard: merging the edges at a vertex can delete a later listed edge, whose pointer
// upstream then reads freed); `-4` when a face of a listed edge does not own its surface
// (upstream compares surface pointers and asks the deleted surface for its face; the
// port's faces own their surfaces); else `status editing graph...`, the B-rep's editing flag and
// its exported graph after the call.
#include "sc_topology_fixture.h"
#include <SmFilletExecutive.h>

#include <set>

namespace
{
struct TopoExec : public SmFilletExecutive
{
    TopoExec(const SmContext& ctx, SmBrep* target) : SmFilletExecutive(ctx, target) {}
    ~TopoExec() { m_vTopoEdges.ReSet(); }

    SmStatus run(const std::vector<SmEdge*>& edges)
    {
        for (SmEdge* e : edges)
            m_vTopoEdges.Add(e);
        return RemoveTopoEdges();
    }
};
} // namespace

SC_EXPORT int32_t sc_blend_fillet_executive_p1_remove_topo_edges(const char* path, const double* in,
                                                                 int64_t n_in, const int64_t* edges,
                                                                 int64_t n_edges, int32_t editing,
                                                                 double* out, int64_t cap,
                                                                 int64_t* out_len)
{
    if (!path || !in || n_in < 1 || !out_len || n_edges < 0 || (n_edges > 0 && !edges))
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    SmStatus status;
    Built built;
    built.brep = build(rd, path, status);
    std::vector<double> o;
    if (!built.brep)
    {
        o.push_back(-1.0);
        return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    }
    SmBrep* brep = built.brep;
    Graph g;
    walk(brep, g, true);
    std::vector<SmEdge*> list;
    std::set<SmVertex*> seen;
    for (int64_t i = 0; i < n_edges; i++)
    {
        if (edges[i] < 0 || edges[i] >= (int64_t)g.edges.size())
            return SC_ERR_ARG;
        SmEdge* e = g.edges[(size_t)edges[i]];
        SmTArray<SmVertex*> verts;
        e->GetVertices(verts);
        for (ULONG j = 0; j < verts.GetSize(); j++)
        {
            if (!seen.insert(verts[j]).second)
            {
                o.push_back(-3.0);
                return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
            }
        }
        SmTArray<SmFace*> faces;
        e->GetFaces(faces);
        for (ULONG j = 0; j < faces.GetSize(); j++)
        {
            SmSurface* s = faces[j]->GetSurface();
            if (s && s->GetOwner() != (SmObject*)faces[j])
            {
                o.push_back(-4.0);
                return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
            }
        }
        list.push_back(e);
    }
    brep->m_bEditingEnabled = editing ? TRUE : FALSE;
    {
        TopoExec x(the_context(), brep);
        status = x.run(list);
    }
    o.push_back(st(status));
    o.push_back(flg(brep->m_bEditingEnabled));
    Graph after;
    walk(brep, after, true);
    export_graph(brep, after, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
