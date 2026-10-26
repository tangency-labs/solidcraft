// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `corpus.parity`: reads one upstream test model with
// SmBrep::ReadFromFile (a B-rep file) or SmBrepData::ReadPartFromFile (a part file: one
// that starts with `//[Output Summary]`) and measures every B-rep as the Rust side does
// (crates/difftest/src/units/corpus_parity.rs, after crates/solidcore/examples/corpus):
// the kind (IsManifoldSolid, IsSheet), the face, edge, vertex and loop counts, the area
// (SmApiFaceComputeArea's per-face integration without its accuracy clamp) and, for a
// solid, the volume (SmApiBrepComputeVolume). `sc_corpus_parity_roundtrip` measures each
// B-rep after an ASCII write (SmBrep::WriteToFile) and read back instead.
//
// The objects read are leaked on purpose: the runner calls this in a child process that
// exits right after (a crash or hang on one model must not stop the run).
#include "sc_shim.h"
#include "StdAfx.h"
#include <SmApiQueries.h>
#include <SmBrep.h>
#include <SmBrepData.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmEdge.h>
#include <SmExtent3d.h>
#include <SmFace.h>
#include <SmLoop.h>
#include <SmSurface.h>
#include <SmVertex.h>
#include <cmath>
#include <cstdio>
#include <cstring>
#include <limits>
#include <vector>

namespace {
const SmContext& the_context()
{
    static SmContext* p = new SmContext();
    return *p;
}

// Whether the file starts with the part file header (as the Rust side tests it).
bool is_part_file(const char* path)
{
    static const char header[] = "//[Output Summary]";
    const size_t n = sizeof(header) - 1;
    char buf[sizeof(header)] = {0};
    FILE* f = std::fopen(path, "rb");
    if (f == nullptr)
        return false;
    size_t got = std::fread(buf, 1, n, f);
    std::fclose(f);
    return got == n && std::memcmp(buf, header, n) == 0;
}

// SmApiFaceComputeArea without its [1e-4, 1e-1] accuracy clamp: face box midpoint as
// origin, area estimate (max box side)^2 / 100, area only, made positive.
SmStatus face_area(const SmFace* face, double accuracy, double& area)
{
    SmExtent3d box;
    SmStatus st = face->CalculateBoundingBox(box);
    if (st != SM_SUCCESS)
        return st;
    SmVector3d size = box.GetSize();
    double d = smos_Max(size.z, smos_Max(size.x, size.y));
    double dv = 0.0;
    SmTArray<SmVector3d> moments;
    st = face->ComputePreciseProperties(SM_OT_SAME, accuracy, box.GetMid(), d * d / 100.0, 0.0,
                                        area, dv, moments, SM_PPF_AREA);
    if (area < 0.0)
        area = -area;
    return st;
}

// One B-rep: kind (0 empty, 1 solid, 2 sheet, 3 other), faces, edges, vertices, loops,
// measure status (0 or the first failing SmStatus), area, volume (NaN unless a solid).
void measure(SmBrep* b, double accuracy, std::vector<double>& o)
{
    SmTArray<SmFace*> faces;
    SmTArray<SmEdge*> edges;
    SmTArray<SmVertex*> vertices;
    b->GetFaces(faces);
    b->GetEdges(edges);
    b->GetVertices(vertices);
    ULONG loops = 0;
    for (ULONG i = 0; i < faces.GetSize(); i++)
    {
        SmTArray<SmLoop*> l;
        faces[i]->GetLoops(l);
        loops += l.GetSize();
    }
    int kind = 3;
    if (faces.GetSize() == 0 && edges.GetSize() == 0 && vertices.GetSize() == 0)
        kind = 0;
    else if (b->IsManifoldSolid())
        kind = 1;
    else if (b->IsSheet())
        kind = 2;

    SmStatus status = SM_SUCCESS;
    double area = 0.0;
    for (ULONG i = 0; i < faces.GetSize() && status == SM_SUCCESS; i++)
    {
        double a = 0.0;
        status = face_area(faces[i], accuracy, a);
        area += a;
    }
    double volume = std::numeric_limits<double>::quiet_NaN();
    if (status == SM_SUCCESS && kind == 1)
    {
        double v = 0.0;
        SmApiStatus vs = SmApiBrepComputeVolume(b, accuracy, v);
        if (vs == SM_SUCCESS)
            volume = v;
        else
            status = (SmStatus)vs;
    }
    o.insert(o.end(), {(double)kind, (double)faces.GetSize(), (double)edges.GetSize(),
                       (double)vertices.GetSize(), (double)loops,
                       status == SM_SUCCESS ? 0.0 : (double)status, area, volume});
}

// The B-reps of model file `path` (one from a B-rep file, any number from a part file);
// the reader's status.
SmStatus read_model(const SmContext& ctx, const char* path, std::vector<SmBrep*>& breps)
{
    SmStatus st;
    if (is_part_file(path))
    {
        SmTArray<SmCurve*> curves;
        SmTArray<SmSurface*> surfaces;
        SmTArray<long> trees;
        SmTArray<SmBrep*> read;
        st = SmBrepData::ReadPartFromFile(ctx, path, curves, surfaces, trees, read, SM_ASCII);
        for (ULONG i = 0; i < read.GetSize(); i++)
            if (read[i] != nullptr)
                breps.push_back(read[i]);
    }
    else
    {
        SmBrep* b = new (ctx) SmBrep();
        st = b->ReadFromFile(ctx, path, SM_ASCII);
        breps.push_back(b);
    }
    return st;
}
} // namespace

// Output: `read_status n` then 8 values per B-rep (see `measure`); `read_status` is 0 or
// the reader's SmStatus (then `n` is 0).
SC_EXPORT int32_t sc_corpus_parity_measure(const char* path, double accuracy, double* out,
                                           int64_t cap, int64_t* out_len)
{
    if (path == nullptr || !(accuracy > 0.0) || !std::isfinite(accuracy))
        return SC_ERR_ARG;
    SC_TRY
    std::vector<SmBrep*> breps;
    SmStatus st = read_model(the_context(), path, breps);
    std::vector<double> o;
    if (st != SM_SUCCESS)
    {
        o.push_back((double)st);
        o.push_back(0.0);
    }
    else
    {
        o.push_back(0.0);
        o.push_back((double)breps.size());
        for (SmBrep* b : breps)
            measure(b, accuracy, o);
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}

// The round trip of every B-rep of model file `path`: `SmBrep::WriteToFile` (SM_ASCII,
// native geometry; `SmBrepData::FromBrep` then `WriteToFile`) to `tmp`, a new B-rep
// read back from it with `ReadFromFile`, measured as `sc_corpus_parity_measure` does.
// Output as `sc_corpus_parity_measure`'s, with the read-back B-reps' records; a B-rep
// whose write or read back fails has the failing SmStatus as its record's status.
SC_EXPORT int32_t sc_corpus_parity_roundtrip(const char* path, const char* tmp,
                                             double accuracy, double* out, int64_t cap,
                                             int64_t* out_len)
{
    if (path == nullptr || tmp == nullptr || !(accuracy > 0.0) || !std::isfinite(accuracy))
        return SC_ERR_ARG;
    SC_TRY
    const SmContext& ctx = the_context();
    std::vector<SmBrep*> breps;
    SmStatus st = read_model(ctx, path, breps);
    std::vector<double> o;
    if (st != SM_SUCCESS)
    {
        o.push_back((double)st);
        o.push_back(0.0);
    }
    else
    {
        o.push_back(0.0);
        o.push_back((double)breps.size());
        const double nan = std::numeric_limits<double>::quiet_NaN();
        for (SmBrep* b : breps)
        {
            SmStatus ws = b->WriteToFile(tmp, SM_ASCII, TRUE, FALSE, 0.0);
            SmBrep* back = new (ctx) SmBrep();
            SmStatus rs = ws == SM_SUCCESS ? back->ReadFromFile(ctx, tmp, SM_ASCII) : ws;
            if (rs == SM_SUCCESS)
                measure(back, accuracy, o);
            else
                o.insert(o.end(), {0.0, 0.0, 0.0, 0.0, 0.0, (double)rs, 0.0, nan});
        }
        std::remove(tmp);
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
