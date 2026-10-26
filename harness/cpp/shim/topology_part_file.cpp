// SPDX-License-Identifier: Apache-2.0
// difftest shim for the part reader (`topology::part_file`, outside the plan):
// SmBrepData::ReadPartFromFile (source/SMLib/src/SmBrepData.cpp) on an upstream test file.
//
// sc_topology_part_file_run reads the part file at `path` and writes a flat list of
// doubles (layout in crates/solidcore-ffi/src/units/topology_part_file.rs): the status,
// then per stand-alone curve its type, dimension and natural interval, per surface its
// type and natural UV domain, the Boolean tree nodes, and per B-rep its tolerance,
// manifold-solid flag and region, shell, face, loop, edge and vertex counts.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmBrep.h>
#include <SmBrepData.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmExtent1d.h>
#include <SmExtent2d.h>
#include <SmFace.h>
#include <SmSurface.h>
#include <SmTArray.h>

#include <vector>

SC_EXPORT int32_t sc_topology_part_file_run(const char* path, int32_t file_type, double* out,
                                            int64_t cap, int64_t* out_len)
{
    if (path == NULL || file_type < 0 || file_type > 1)
        return SC_ERR_ARG;
    SC_TRY
    // one context for the process, never destroyed, as the corpus.parity shim's (whose
    // reads of the same part files succeed)
    static SmContext* context = new SmContext();
    const SmContext& ctx = *context;
    SmTArray<SmCurve*> curves;
    SmTArray<SmSurface*> surfaces;
    SmTArray<long> trees;
    SmTArray<SmBrep*> breps;
    SmStatus st = SmBrepData::ReadPartFromFile(ctx, path, curves, surfaces, trees, breps,
                                               file_type == 0 ? SM_ASCII : SM_BINARY);
    std::vector<double> o;
    // status: 0 for SM_SUCCESS (1000L), else the SmStatus (the layout of the FFI docs)
    o.push_back(st == SM_SUCCESS ? 0.0 : (double)st);
    if (st == SM_SUCCESS)
    {
        o.push_back((double)curves.GetSize());
        for (ULONG i = 0; i < curves.GetSize(); i++)
        {
            SmCurve* c = curves[i];
            SmExtent1d ivl = c->GetNaturalInterval();
            o.push_back((double)c->GetType());
            o.push_back((double)c->GetDim());
            o.push_back(ivl.GetMin());
            o.push_back(ivl.GetMax());
        }
        o.push_back((double)surfaces.GetSize());
        for (ULONG i = 0; i < surfaces.GetSize(); i++)
        {
            SmSurface* s = surfaces[i];
            SmExtent2d d = s->GetNaturalUVDomain();
            o.push_back((double)s->GetType());
            o.push_back(d.GetMin().x);
            o.push_back(d.GetMin().y);
            o.push_back(d.GetMax().x);
            o.push_back(d.GetMax().y);
        }
        o.push_back((double)trees.GetSize());
        for (ULONG i = 0; i < trees.GetSize(); i++)
            o.push_back((double)trees[i]);
        o.push_back((double)breps.GetSize());
        for (ULONG i = 0; i < breps.GetSize(); i++)
        {
            SmBrep* b = breps[i];
            SmTArray<SmRegion*> regions;
            SmTArray<SmShell*> shells;
            SmTArray<SmFace*> faces;
            SmTArray<SmEdge*> edges;
            SmTArray<SmVertex*> vertices;
            b->GetRegions(regions);
            b->GetShells(shells);
            b->GetFaces(faces);
            b->GetEdges(edges);
            b->GetVertices(vertices);
            ULONG loops = 0;
            for (ULONG f = 0; f < faces.GetSize(); f++)
            {
                SmTArray<SmLoop*> l;
                faces[f]->GetLoops(l);
                loops += l.GetSize();
            }
            o.push_back((double)b->GetTolerance());
            o.push_back(b->IsManifoldSolid() ? 1.0 : 0.0);
            o.push_back((double)regions.GetSize());
            o.push_back((double)shells.GetSize());
            o.push_back((double)faces.GetSize());
            o.push_back((double)loops);
            o.push_back((double)edges.GetSize());
            o.push_back((double)vertices.GetSize());
        }
    }
    // the part's objects are the caller's: release them
    for (ULONG i = 0; i < curves.GetSize(); i++)
        delete curves[i];
    for (ULONG i = 0; i < surfaces.GetSize(); i++)
        delete surfaces[i];
    for (ULONG i = 0; i < breps.GetSize(); i++)
        delete breps[i];
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
