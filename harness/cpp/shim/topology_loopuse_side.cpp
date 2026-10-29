// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.loopuse`: SmLoopuse::IsLoopuseSideInSurface (source/SMLib/
// src/SmLoopuse.cpp) on every loop use of a real SmBrep.
//
// One entry point, sc_topology_loopuse_side_in_surface(path, in, n_in, out, cap, out_len). The
// input is `periodic_u periodic_v` (`-1`: NULL, else the SmBoolean given) then the B-rep block
// of unit topology.vertexuse (sc_topology_fixture.h; validated by crates/solidcore-ffi/src/
// units/topology_loopuse.rs). Output: `0 n (side on_boundary index)*n` (the loop uses in the
// fixture's traversal order; `-2 -2 -2` where an edge use's edge has no curve, which upstream
// dereferences), or `-1 status` when the B-rep cannot be built.
#include "sc_topology_fixture.h"

#include <SmLoopuse.h>

SC_EXPORT int32_t sc_topology_loopuse_side_in_surface(const char* path, const double* in, int64_t n_in,
                                                      double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 3 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    double pu = rd.num(), pv = rd.num();
    SmBoolean bu = pu == 1.0 ? TRUE : FALSE, bv = pv == 1.0 ? TRUE : FALSE;
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
        o.push_back((double)g.loopuses.size());
        for (SmLoopuse* lu : g.loopuses)
        {
            SmTArray<SmEdgeuse*> eus;
            lu->GetEdgeuses(eus);
            bool guard = false;
            for (ULONG i = 0; i < eus.GetSize(); i++)
            {
                SmFace* f = eus[i]->GetFace();
                SmEdge* e = eus[i]->GetEdge();
                if (f != NULL && f->GetSurface() != NULL && e != NULL && e->GetCurve() == NULL)
                    guard = true;
            }
            if (guard)
            {
                o.insert(o.end(), {-2.0, -2.0, -2.0});
                continue;
            }
            SmBoolean on = FALSE;
            ULONG idx = 0;
            SmBoolean r = lu->IsLoopuseSideInSurface(pu < 0 ? NULL : &bu, pv < 0 ? NULL : &bv, &on, &idx);
            o.push_back(flg(r));
            o.push_back(flg(on));
            o.push_back((double)idx);
        }
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
