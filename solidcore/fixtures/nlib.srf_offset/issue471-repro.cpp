// SPDX-License-Identifier: Apache-2.0
// Reproduce solidcore#471 with the pinned, unmodified SMLib reference.
// Run from the repo root (writes issue471-offset-{0..7}.sms in the cwd):
// nice -n 10 c++ -std=c++17 -O2 -ffp-contract=off -fno-fast-math -DNDEBUG \
//   -DNOMINMAX -DSM_GRAPHICS_CALLBACKS=1 -Iupstream/source/SMLib/inc \
//   -Iupstream/source/SM_API/inc fixtures/nlib.srf_offset/issue471-repro.cpp \
//   -Wl,--wrap=N_SrfOffset bin/cpp/lib/libSM_API.a bin/cpp/lib/libSMLib.a \
//   -lpthread -o /tmp/issue471-repro
// nice -n 10 /tmp/issue471-repro
// The link wrapper records each input to the real N_SrfOffset without changing it.
// issue471.json contains inputs 4..7 (the four rounds); 0..3 are planar faces.
// The .sms rational flag is 1 to retain all four raw homogeneous components;
// NL_NOW is preserved for non-rational points (as in the JSON fixture).
#include "StdAfx.h"
#include <SmApiGeneral.h>
#include <SmApiCurves.h>
#include <SmApiBrep.h>
#include <SmBrep.h>
#include <SmLine.h>
#include <SmEdge.h>
#include <SmVertex.h>
#include <SmFace.h>
#include <SmBSplineSurface.h>
#include <SmFilletExecutive.h>
#include <SmFilletSolver.h>
#include <nurbs.h>
#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
extern "C" NL_FLAG __real_N_SrfOffset(NL_SURFACE*, NL_REAL, NL_DEGREE, NL_DEGREE,
                                      NL_FLAG, NL_REAL, NL_REAL, NL_SURFACE*, NL_STACKS*);

extern "C" NL_FLAG __wrap_N_SrfOffset(NL_SURFACE* n, NL_REAL d, NL_DEGREE pp,
                                      NL_DEGREE qq, NL_FLAG par, NL_REAL eps, NL_REAL tol,
                                      NL_SURFACE* result, NL_STACKS* stack)
{
    static unsigned long index = 0;
    std::fprintf(stderr, "offset %lu d=%.17g pp=%d qq=%d par=%d eps=%.17g tol=%.17g\n",
                 index, d, pp, qq, par, eps, tol);
    char file[100];
    std::snprintf(file, sizeof(file), "issue471-offset-%lu.sms", index++);
    FILE* out = std::fopen(file, "w");
    if (!out) std::abort();
    std::fprintf(out, "%ld %ld\n%d %d\n1\n", (long)n->net->n, (long)n->net->m, n->p, n->q);
    for (long i = 0; i <= n->net->n; ++i)
        for (long j = 0; j <= n->net->m; ++j)
        {
            const auto a = n->net->Pw[i][j];
            std::fprintf(out, "%.17g %.17g %.17g %.17g\n", a.x, a.y, a.z, a.w);
        }
    for (long i = 0; i <= n->net->n + n->p + 1; ++i)
        std::fprintf(out, "%.17g ", n->knu->U[i]);
    std::fprintf(out, "\n");
    for (long i = 0; i <= n->net->m + n->q + 1; ++i)
        std::fprintf(out, "%.17g ", n->knv->U[i]);
    std::fprintf(out, "\n");
    std::fclose(out);

    const auto rc = __real_N_SrfOffset(n, d, pp, qq, par, eps, tol, result, stack);
    double max_point = 0;
    if (rc == NL_NO)
        for (long i = 0; i <= result->net->n; ++i)
            for (long j = 0; j <= result->net->m; ++j)
            {
                NL_POINT p;
                N_CPtToPt(result->net->Pw[i][j], &p);
                max_point = std::max(max_point, std::sqrt(p.x*p.x + p.y*p.y + p.z*p.z));
            }
    std::fprintf(stderr, "offset rc=%d max_point=%.17g\n", rc, max_point);
    return rc;
}

int main()
{
    SmApiCreateContext();
    SmPoint3d pts[] = { SmPoint3d(-60, -40, 0), SmPoint3d(60, -40, 0),
                       SmPoint3d(60, 40, 0), SmPoint3d(-60, 40, 0) };
    SmTArray<SmCurve*> curves;
    for (int i = 0; i < 4; ++i)
    {
        SmLine* line = nullptr;
        if (SmApiCreateLineSegment(pts[i], pts[(i+1)%4], line) != SM_SUCCESS) return 1;
        curves.Add(line);
    }
    SmBrep* brep = nullptr;
    auto status = SmApiCreateTaperExtrude(curves, 40, 5, 3, FALSE, brep);
    std::fprintf(stderr, "extrude=%d\n", (int)status);
    if (status != SM_SUCCESS) return 2;

    SmTArray<SmEdge*> all, edges;
    brep->GetEdges(all);
    for (ULONG i = 0; i < all.GetSize(); ++i)
    {
        auto edge = all[i];
        if (std::abs(edge->GetStartVertex()->GetPoint().z - edge->GetEndVertex()->GetPoint().z) > 30)
            edges.Add(edge);
    }
    if (edges.GetSize() != 4) return 3;
    SmFilletExecutive executive(*brep->GetContext(), brep);
    status = executive.SetFilletParameters(brep, edges, SM_FSG_CIRCULAR,
        SM_FS_CONST_RADIUS, 4, nullptr, -1, -1, -1, -1, 1, 1.0);
    if (status != SM_SUCCESS) return 4;
    status = executive.DoFillet();
    std::fprintf(stderr, "fillet=%d\n", (int)status);
    if (status != SM_SUCCESS) return 5;

    SmTArray<SmFace*> faces;
    brep->GetFaces(faces);
    // Traversal-order face 4 is the top cap for this pinned construction.
    if (faces.GetSize() != 10) return 6;
    SmTArray<const SmFace*> open;
    open.Add(faces[4]);
    SmBrep* shell = nullptr;
    status = SmApiShellBrepFull(brep, -2, TRUE, TRUE, TRUE, open, shell);
    std::fprintf(stderr, "shell=%d\n", (int)status);
    delete shell;
}
