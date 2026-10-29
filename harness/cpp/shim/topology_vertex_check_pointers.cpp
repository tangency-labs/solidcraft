// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.vertex`: SmVertex::CheckPointers (source/SMLib/src/
// SmVertex.cpp) on every vertex of a real SmBrep.
//
// One entry point, sc_topology_vertex_check_pointers(path, in, n_in, out, cap, out_len). The
// input is the B-rep block of unit topology.vertexuse (built by the shared
// sc_topology_fixture.h; validated by crates/solidcore-ffi/src/units/topology_vertex.rs).
// Output: `0 n flag*n` (the vertices in the fixture's traversal order), or `-1 status` when
// the B-rep cannot be built.
#include "sc_topology_fixture.h"

SC_EXPORT int32_t sc_topology_vertex_check_pointers(const char* path, const double* in, int64_t n_in,
                                                    double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    SmStatus status = SM_SUCCESS;
    Built built;
    built.brep = build(rd, path, status);
    if (built.brep == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(status));
    }
    else
    {
        o.push_back(0.0);
        Graph g;
        walk(built.brep, g);
        o.push_back((double)g.vertices.size());
        for (SmVertex* v : g.vertices)
            o.push_back(flg(v->CheckPointers()));
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
