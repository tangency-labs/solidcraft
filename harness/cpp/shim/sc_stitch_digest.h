// SPDX-License-Identifier: Apache-2.0
// Shared by the difftest shims of units `heal.stitch_p1` and `heal.stitch_p2`: the
// topology of a B-rep after a stitch, walked in list order so that it does not depend on
// how either side numbers its records. Mirrored by `topology_digest` in
// crates/difftest/src/units/heal_stitch_p1.rs. Layout:
//   n_shells (SmBrep::GetShells)
//   per edge of GetEdges: start and end vertex (index in GetVertices, -1 null or not
//     listed), tolerance, interval min and max, edge use count (SmEdge::GetEdgeuses)
//   per face of GetFaces: loop count (SmFace::GetLoops), then per loop its edge use
//     count (SmLoop::GetEdgeuses) and per edge use the edge (index in GetEdges, -1 none)
//     and the SmOrientType code
// Only lookups go through the pointer maps; nothing iterates them.
#ifndef SC_STITCH_DIGEST_H
#define SC_STITCH_DIGEST_H

#include <SmBrep.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmExtent1d.h>
#include <SmFace.h>
#include <SmLoop.h>
#include <SmShell.h>
#include <SmTArray.h>
#include <SmVertex.h>
#include <map>
#include <vector>

namespace
{
void put_topology(SmBrep* brep, std::vector<double>& o)
{
    SmTArray<SmShell*> ss;
    brep->GetShells(ss);
    SmTArray<SmFace*> fs;
    brep->GetFaces(fs);
    SmTArray<SmEdge*> es;
    brep->GetEdges(es);
    SmTArray<SmVertex*> vs;
    brep->GetVertices(vs);
    std::map<const SmVertex*, double> vi;
    for (ULONG i = 0; i < vs.GetSize(); i++)
        vi.emplace(vs[i], (double)i);
    std::map<const SmEdge*, double> ei;
    for (ULONG i = 0; i < es.GetSize(); i++)
        ei.emplace(es[i], (double)i);
    auto vfind = [&](const SmVertex* v) {
        auto it = vi.find(v);
        return it == vi.end() ? -1.0 : it->second;
    };
    auto efind = [&](const SmEdge* e) {
        auto it = ei.find(e);
        return it == ei.end() ? -1.0 : it->second;
    };
    o.push_back((double)ss.GetSize());
    for (ULONG i = 0; i < es.GetSize(); i++)
    {
        SmEdge* e = es[i];
        o.push_back(vfind(e->GetStartVertex()));
        o.push_back(vfind(e->GetEndVertex()));
        o.push_back((double)e->GetTolerance());
        SmExtent1d iv = e->GetInterval();
        o.push_back(iv.GetMin());
        o.push_back(iv.GetMax());
        SmTArray<SmEdgeuse*> eus;
        e->GetEdgeuses(eus);
        o.push_back((double)eus.GetSize());
    }
    for (ULONG i = 0; i < fs.GetSize(); i++)
    {
        SmTArray<SmLoop*> ls;
        fs[i]->GetLoops(ls);
        o.push_back((double)ls.GetSize());
        for (ULONG j = 0; j < ls.GetSize(); j++)
        {
            SmTArray<SmEdgeuse*> eus;
            ls[j]->GetEdgeuses(eus);
            o.push_back((double)eus.GetSize());
            for (ULONG m = 0; m < eus.GetSize(); m++)
            {
                o.push_back(efind(eus[m]->GetEdge()));
                o.push_back((double)eus[m]->GetOrientation());
            }
        }
    }
}
} // namespace

#endif
