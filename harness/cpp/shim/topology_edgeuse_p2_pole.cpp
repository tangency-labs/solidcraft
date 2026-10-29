// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `topology.edgeuse_p2`: SmEdgeuse::HasDogLegNearPole and
// SmEdgeuse::IsEdgeuseSideInSurface (source/SMLib/src/SmEdgeuse.cpp) on every edge use of a
// real SmBrep.
//
// One entry point, sc_topology_edgeuse_p2_pole(path, in, n_in, out, cap, out_len). The input
// is `smp_count periodic_u periodic_v` (`-1`: NULL, else the SmBoolean given) then the B-rep
// block of unit topology.vertexuse (sc_topology_fixture.h; validated by crates/solidcore-ffi/
// src/units/topology_edgeuse_p2.rs). Output: `0 n (dog_leg index side on_boundary)*n` (the
// edge uses in the fixture's traversal order; `index` is `-1` when HasDogLegNearPole leaves
// it unset), or `-1 status` when the B-rep cannot be built. `-2` marks a call skipped because
// upstream reads out of bounds or dereferences null: HasDogLegNearPole of an
// intersector-form UV trim curve with fewer than four control points (it reads
// `lNum - 4`), IsEdgeuseSideInSurface of an edge without curve on a face with a surface.
#include "sc_topology_fixture.h"

#include <SmEdgeuse.h>

SC_EXPORT int32_t sc_topology_edgeuse_p2_pole(const char* path, const double* in, int64_t n_in,
                                              double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 4 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    ULONG smp = (ULONG)rd.idx();
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
        o.push_back((double)g.edgeuses.size());
        for (SmEdgeuse* eu : g.edgeuses)
        {
            SmBSplineCurve* trim = eu->GetUVTrimCurve();
            SmFace* f = eu->GetFace();
            SmSurface* s = f ? f->GetSurface() : NULL;
            bool dog_guard = false;
            if (trim != NULL && s != NULL && s->GetSingularities() != SM_SS_NONE && trim->IsIntersectorApprox())
            {
                SmTArray<SmPoint3d> poly;
                trim->GetPolygon(trim->GetNaturalInterval(), poly);
                dog_guard = poly.GetSize() < 4;
            }
            if (dog_guard)
            {
                o.push_back(-2.0);
                o.push_back(-2.0);
            }
            else
            {
                SmBoolean idx = -1;
                SmBoolean dog = eu->HasDogLegNearPole(&idx);
                o.push_back(flg(dog));
                o.push_back((double)idx);
            }
            SmEdge* e = eu->GetEdge();
            if (s != NULL && e != NULL && e->GetCurve() == NULL)
            {
                o.push_back(-2.0);
                o.push_back(-2.0);
                continue;
            }
            SmBoolean on = FALSE;
            SmBoolean side = eu->IsEdgeuseSideInSurface(smp, pu < 0 ? NULL : &bu, pv < 0 ? NULL : &bv, &on);
            o.push_back(flg(side));
            o.push_back(flg(on));
        }
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
