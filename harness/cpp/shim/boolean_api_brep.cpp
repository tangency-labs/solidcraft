// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `boolean.api_brep`: the boolean entry points of
// source/SM_API/src/SmApiBrep.cpp (SmApiBoolean, SmApiBooleanUnion, SmApiBooleanDifference,
// SmApiBooleanIntersection, SmApiBooleanMerge, SmApiMergeBreps, SmApiBooleanWithOptions,
// SmApiBooleanLists, SmApiBooleanTrees), called exactly as a caller of the API would.
//
// One entry point, sc_boolean_api_brep_run(in, n_in, out, cap, out_len). `in` is
// `op args...`; layouts are documented in crates/solidcore-ffi/src/units/boolean_api_brep.rs,
// which validates every input so upstream only touches valid memory. Every read is bounds
// checked here as well (a malformed program is SC_ERR_ARG). The whole program is parsed before
// any upstream object is built. Operand B-reps are boxes (SmApiCreateBox), spheres
// (SmApiCreateSphere), sheets (SmApiCreatePlane), tori (SmApiCreateTorus) or the SmApiBoolean
// result of two operands; a failing constructor or operand Boolean is SC_ERR_ARG, except a
// failing SmApiCreateTorus, whose status becomes the call's (record `2`) so its radius guard is
// compared. `out` receives the API status (0 = SM_SUCCESS, else the SmStatus) and, on success,
// the op's outputs.
//
// Ownership follows source/SM_API/inc/SmApiBrep.h: a rejected or failed two-B-rep call leaves
// both operands with the caller (deleted here); on success the result is deleted and the second
// operand is deleted here only when upstream keeps it. A failed N-ary call after validation
// may have consumed some operands, so those are leaked rather than risk a double delete.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmApiBrep.h>
#include <SmApiGeneral.h>
#include <SmApiPrimitives.h>
#include <SmApiQueries.h>
#include <SmBrep.h>
#include <SmContext.h>
#include <SmFaceuse.h>
#include <SmRegion.h>
#include <SmShell.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector3d.h>

#include <algorithm>
#include <stdexcept>
#include <vector>

#include "sc_sm.h"

using sc::flg;
using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/boolean_api_brep.rs).
enum : int32_t
{
    OP_BOOLEAN = 1,
    OP_UNION = 2,
    OP_DIFFERENCE = 3,
    OP_INTERSECTION = 4,
    OP_MERGE = 5,
    OP_WITH_OPTIONS = 6,
    OP_MERGE_BREPS = 7,
    OP_LISTS = 8,
    OP_TREES = 9,
    OP_MEASURED = 10,
};

// Operand kinds.
enum : int32_t
{
    BK_BOX = 1,      // SmApiCreateBox(origin, length, width, height)
    BK_SPHERE = 2,   // SmApiCreateSphere(center, radius)
    BK_PLANE = 3,    // SmApiCreatePlane(origin, x, y)
    BK_BOOLEAN = 4,  // SmApiBoolean(a, b, operation)'s result
    BK_TORUS = 5,    // SmApiCreateTorus(center, major, minor)
};

// Deepest operand block (MAX_DEPTH in the Rust module).
const int MAX_DEPTH = 4;

struct Bad : std::runtime_error
{
    Bad() : std::runtime_error("bad program") {}
};

// A failed SmApiCreateTorus: its status becomes the call's, with the record `2`.
struct TorusFailed
{
    double status;
};

struct Rd
{
    const double* in;
    int64_t n;
    int64_t at;
    double num()
    {
        if (at >= n)
            throw Bad();
        return in[at++];
    }
    int64_t idx(int64_t lo, int64_t hi)
    {
        double d = num();
        if (!(d >= (double)lo && d <= (double)hi) || d != (double)(int64_t)d)
            throw Bad();
        return (int64_t)d;
    }
    bool flag() { return idx(0, 1) != 0; }
};

struct BrepSpec
{
    int32_t kind = BK_BOX;
    double d[6] = {0, 0, 0, 0, 0, 0};
    std::vector<BrepSpec> sub; // BK_BOOLEAN: a, b
};

BrepSpec read_spec(Rd& rd, int depth = 0)
{
    if (depth >= MAX_DEPTH)
        throw Bad();
    BrepSpec s;
    s.kind = (int32_t)rd.idx(1, 5);
    int k = 0;
    switch (s.kind)
    {
    case BK_BOX: k = 6; break;
    case BK_SPHERE: k = 4; break;
    case BK_PLANE: k = 5; break;
    case BK_BOOLEAN: k = 0; break;
    case BK_TORUS: k = 5; break;
    default: throw Bad();
    }
    if (s.kind == BK_BOOLEAN)
    {
        s.d[0] = (double)rd.idx(1, 10);
        if (s.d[0] > 3 && s.d[0] < 10)
            throw Bad();
    }
    for (int i = 0; i < k; i++)
        s.d[i] = rd.num();
    if (s.kind == BK_BOOLEAN)
    {
        s.sub.push_back(read_spec(rd, depth + 1));
        s.sub.push_back(read_spec(rd, depth + 1));
    }
    return s;
}

std::vector<BrepSpec> read_specs(Rd& rd, int64_t max)
{
    int64_t k = rd.idx(0, max);
    std::vector<BrepSpec> v;
    for (int64_t i = 0; i < k; i++)
        v.push_back(read_spec(rd));
    return v;
}

SmBrep* build(const BrepSpec& s)
{
    SmVector3d o(s.d[0], s.d[1], s.d[2]);
    SmBrep* r = NULL;
    SmApiStatus stat = SM_ERR;
    switch (s.kind)
    {
    case BK_BOX: stat = SmApiCreateBox(o, s.d[3], s.d[4], s.d[5], r); break;
    case BK_SPHERE: stat = SmApiCreateSphere(o, s.d[3], r); break;
    case BK_PLANE: stat = SmApiCreatePlane(o, s.d[3], s.d[4], r); break;
    case BK_TORUS: stat = SmApiCreateTorus(o, s.d[3], s.d[4], r); break;
    case BK_BOOLEAN:
    {
        SmBrep* a = build(s.sub[0]);
        SmBrep* b = NULL;
        try
        {
            b = build(s.sub[1]);
        }
        catch (...)
        {
            delete a;
            throw;
        }
        // Union, intersection, difference and slice delete `b` on success
        // (source/SM_API/inc/SmApiBrep.h); both remain allocated on failure.
        stat = SmApiBoolean(a, b, (SmBooleanOperationType)(int32_t)s.d[0], r);
        if (stat != SM_SUCCESS || r != a)
        {
            if (r != NULL && r != a && r != b)
                delete r;
            delete a;
            delete b;
            throw Bad();
        }
        break;
    }
    }
    if (s.kind == BK_TORUS && stat != SM_SUCCESS)
    {
        delete r;
        throw TorusFailed{st(stat)};
    }
    if (stat != SM_SUCCESS || r == NULL)
    {
        delete r;
        throw Bad();
    }
    return r;
}

// Builds every spec; on failure deletes what was built and rethrows.
std::vector<SmBrep*> build_all(const std::vector<BrepSpec>& specs)
{
    std::vector<SmBrep*> v;
    try
    {
        for (size_t i = 0; i < specs.size(); i++)
            v.push_back(build(specs[i]));
    }
    catch (...)
    {
        for (size_t i = 0; i < v.size(); i++)
            delete v[i];
        throw;
    }
    return v;
}

// `faces edges vertices regions manifold_solid` of a B-rep, then per region (GetRegions
// order) `is_void is_infinite shells` and per shell (GetShells order) `faceuses mate_region`:
// the index in GetRegions of the region on the other side of the shell's first faceuse
// (-1 for a shell without faceuses), so the order of the outer and void shells is compared.
void put_brep(std::vector<double>& o, const SmBrep* b)
{
    SmTArray<SmRegion*> regions;
    b->GetRegions(regions);
    o.push_back((double)b->GetNumFaces());
    o.push_back((double)b->GetNumEdges());
    o.push_back((double)b->GetNumVertices());
    o.push_back((double)regions.GetSize());
    o.push_back(flg(b->IsManifoldSolid() != FALSE));
    for (ULONG r = 0; r < regions.GetSize(); r++)
    {
        SmTArray<SmShell*> shells;
        regions[r]->GetShells(shells);
        o.push_back((double)regions[r]->IsVoid());
        o.push_back(flg(regions[r]->IsInfiniteRegion() != FALSE));
        o.push_back((double)shells.GetSize());
        for (ULONG s = 0; s < shells.GetSize(); s++)
        {
            SmTArray<SmFaceuse*> fus;
            shells[s]->GetFaceuses(fus);
            o.push_back((double)fus.GetSize());
            double mate = -1;
            SmFaceuse* fu = shells[s]->GetFirstFaceuse();
            if (fu != NULL && fu->GetMate() != NULL)
            {
                SmRegion* mr = fu->GetMate()->GetShell()->GetRegion();
                for (ULONG k = 0; k < regions.GetSize(); k++)
                    if (regions[k] == mr)
                        mate = (double)k;
            }
            o.push_back(mate);
        }
    }
}

// Accuracies of the OP_MEASURED queries: SmApiBrepComputeVolume clamps to [1e-8, 1e-1],
// SmApiFaceComputeArea to [1e-4, 1e-1].
const double VOLUME_ACCURACY = 1e-6;
const double AREA_ACCURACY = 1e-4;

// OP_MEASURED's tail: `volume_status volume faces` then the face areas in ascending order
// (a face whose SmApiFaceComputeArea fails counts as minus its status), then the number of
// those values, so the reader can split them from the integer record before them.
void put_measures(std::vector<double>& o, SmBrep* b)
{
    size_t start = o.size();
    double volume = 0.0;
    SmApiStatus vstat = SmApiBrepComputeVolume(b, VOLUME_ACCURACY, volume);
    o.push_back(st(vstat));
    o.push_back(vstat == SM_SUCCESS ? volume : 0.0);
    SmTArray<SmFace*> faces;
    b->GetFaces(faces);
    o.push_back((double)faces.GetSize());
    std::vector<double> areas;
    for (ULONG i = 0; i < faces.GetSize(); i++)
    {
        double area = 0.0;
        SmApiStatus astat = SmApiFaceComputeArea(faces[i], AREA_ACCURACY, area);
        areas.push_back(astat == SM_SUCCESS ? area : -(double)st(astat));
    }
    std::sort(areas.begin(), areas.end());
    o.insert(o.end(), areas.begin(), areas.end());
    o.push_back((double)(o.size() - start));
}

// Whether upstream keeps the second operand of a successful two-B-rep boolean
// (source/SM_API/inc/SmApiBrep.h).
bool keeps_other(int32_t op, bool keep_option)
{
    return op == SM_BO_PARTIAL_MERGE || op == SM_BO_IMPRINT_CLASSIFY || keep_option;
}

void run_two(Rd& rd, int32_t api, std::vector<double>& o)
{
    BrepSpec a = read_spec(rd);
    BrepSpec b = read_spec(rd);
    int32_t op = SM_BO_UNKNOWN;
    bool flags[5] = {false, false, true, false, false};
    switch (api)
    {
    case OP_BOOLEAN: op = (int32_t)rd.idx(0, 10); break;
    case OP_UNION: op = SM_BO_UNION; break;
    case OP_DIFFERENCE: op = SM_BO_DIFFERENCE; break;
    case OP_INTERSECTION: op = SM_BO_INTERSECTION; break;
    case OP_MERGE: op = SM_BO_MERGE; break;
    case OP_WITH_OPTIONS:
        op = (int32_t)rd.idx(0, 10);
        for (int i = 0; i < 5; i++)
            flags[i] = rd.flag();
        break;
    case OP_MEASURED:
        // SmApiBooleanWithOptions with only bBooleanPostProcess chosen
        op = (int32_t)rd.idx(0, 10);
        flags[2] = rd.flag();
        break;
    default: throw Bad();
    }
    if (rd.at != rd.n)
        throw Bad();
    std::vector<BrepSpec> specs;
    specs.push_back(a);
    specs.push_back(b);
    std::vector<SmBrep*> bs = build_all(specs);
    SmBrep* b1 = bs[0];
    SmBrep* b2 = bs[1];
    SmBrep* res = b1; // stale output: every entry point must clear it
    SmBooleanOperationType eop = (SmBooleanOperationType)op;
    SmApiStatus stat = SM_ERR;
    switch (api)
    {
    case OP_BOOLEAN: stat = SmApiBoolean(b1, b2, eop, res); break;
    case OP_UNION: stat = SmApiBooleanUnion(b1, b2, res); break;
    case OP_DIFFERENCE: stat = SmApiBooleanDifference(b1, b2, res); break;
    case OP_INTERSECTION: stat = SmApiBooleanIntersection(b1, b2, res); break;
    case OP_MERGE: stat = SmApiBooleanMerge(b1, b2, res); break;
    case OP_WITH_OPTIONS:
    case OP_MEASURED:
        stat = SmApiBooleanWithOptions(b1, b2, eop, flags[0], flags[1], flags[2], flags[3],
                                       flags[4], res);
        break;
    }
    o.push_back(st(stat));
    if (stat != SM_SUCCESS)
    {
        // Both operands remain allocated on failure.
        if (res != NULL && res != b1 && res != b2)
            delete res;
        delete b1;
        delete b2;
        return;
    }
    bool kept = keeps_other(op, api == OP_WITH_OPTIONS && flags[4]);
    o.push_back(flg(res != NULL));
    if (res == NULL)
        return;
    o.push_back(flg(res == b1));
    o.push_back(flg(kept));
    put_brep(o, res);
    if (kept && b2 != res)
    {
        put_brep(o, b2);
        delete b2;
    }
    if (api == OP_MEASURED)
        put_measures(o, res);
    delete res; // b1 itself on the documented path; any other b1 is left (ownership unknown)
}

void run_merge_breps(Rd& rd, std::vector<double>& o)
{
    std::vector<BrepSpec> specs = read_specs(rd, 16);
    int32_t op = (int32_t)rd.idx(0, 10);
    if (rd.at != rd.n)
        throw Bad();
    std::vector<SmBrep*> bs = build_all(specs);
    SmTArray<SmBrep*> arr;
    for (size_t i = 0; i < bs.size(); i++)
        arr.Add(bs[i]);
    SmBrep* res = bs.empty() ? NULL : bs[0]; // stale output
    SmApiStatus stat = SmApiMergeBreps(arr, (SmBooleanOperationType)op, res);
    o.push_back(st(stat));
    if (stat == SM_ERR_INVALID_INPUT)
    {
        // Rejected before anything is consumed.
        o.push_back(flg(res == NULL));
        o.push_back((double)arr.GetSize());
        for (size_t i = 0; i < bs.size(); i++)
            delete bs[i];
        return;
    }
    if (stat != SM_SUCCESS)
        return; // consumed in part; leak
    o.push_back((double)arr.GetSize());
    o.push_back(flg(res != NULL));
    if (res == NULL)
        return;
    put_brep(o, res);
    delete res;
}

void run_lists(Rd& rd, std::vector<double>& o)
{
    std::vector<BrepSpec> s1 = read_specs(rd, 8);
    std::vector<BrepSpec> s2 = read_specs(rd, 8);
    int64_t op = rd.idx(-(int64_t)1 << 31, ((int64_t)1 << 31) - 1);
    if (rd.at != rd.n)
        throw Bad();
    std::vector<BrepSpec> all(s1);
    all.insert(all.end(), s2.begin(), s2.end());
    std::vector<SmBrep*> bs = build_all(all);
    SmTArray<SmBrep*> b1, b2;
    for (size_t i = 0; i < bs.size(); i++)
        (i < s1.size() ? b1 : b2).Add(bs[i]);
    SmTArray<SmSurface*> srf1, srf2;
    SmTArray<SmBrep*> rb;
    SmTArray<SmSurface*> rs;
    SmApiStatus stat = SmApiBooleanLists(b1, srf1, b2, srf2, (int)op, rb, rs);
    o.push_back(st(stat));
    if (stat == SM_SUCCESS)
    {
        o.push_back((double)rb.GetSize());
        for (ULONG i = 0; i < rb.GetSize(); i++)
        {
            o.push_back(flg(rb[i] != NULL));
            if (rb[i] != NULL)
                put_brep(o, rb[i]);
        }
        o.push_back((double)rs.GetSize());
    }
    // The inputs are never modified; outputs are new objects.
    for (ULONG i = 0; i < rb.GetSize(); i++)
        delete rb[i];
    for (ULONG i = 0; i < rs.GetSize(); i++)
        delete rs[i];
    for (size_t i = 0; i < bs.size(); i++)
        delete bs[i];
}

void run_trees(Rd& rd, std::vector<double>& o)
{
    std::vector<BrepSpec> specs = read_specs(rd, 16);
    int64_t t = rd.idx(0, 64);
    SmTArray<long> tree;
    for (int64_t i = 0; i < t; i++)
        tree.Add((long)rd.idx(-64, 64));
    if (rd.at != rd.n)
        throw Bad();
    std::vector<SmBrep*> bs = build_all(specs);
    SmTArray<SmBrep*> arr;
    for (size_t i = 0; i < bs.size(); i++)
        arr.Add(bs[i]);
    SmApiStatus stat = SmApiBooleanTrees(arr, tree);
    o.push_back(st(stat));
    if (stat != SM_SUCCESS)
        return; // consumed in part; leak
    o.push_back((double)arr.GetSize());
    for (ULONG i = 0; i < arr.GetSize(); i++)
    {
        o.push_back(flg(arr[i] != NULL));
        if (arr[i] != NULL)
            put_brep(o, arr[i]);
    }
    for (ULONG i = 0; i < arr.GetSize(); i++)
        delete arr[i];
}

void run(Rd& rd, std::vector<double>& o)
{
    int32_t api = (int32_t)rd.idx(1, 10);
    switch (api)
    {
    case OP_BOOLEAN:
    case OP_UNION:
    case OP_DIFFERENCE:
    case OP_INTERSECTION:
    case OP_MERGE:
    case OP_WITH_OPTIONS:
    case OP_MEASURED: run_two(rd, api, o); break;
    case OP_MERGE_BREPS: run_merge_breps(rd, o); break;
    case OP_LISTS: run_lists(rd, o); break;
    case OP_TREES: run_trees(rd, o); break;
    default: throw Bad();
    }
}
} // namespace

SC_EXPORT int32_t sc_boolean_api_brep_run(const double* in, int64_t n_in, double* out,
                                          int64_t cap, int64_t* out_len)
{
    SC_TRY
    SmApiGetOrCreateContext();
    Rd rd{in, n_in, 0};
    std::vector<double> o;
    try
    {
        run(rd, o);
    }
    catch (const Bad&)
    {
        return SC_ERR_ARG;
    }
    catch (const TorusFailed& t)
    {
        o.clear();
        o.push_back(t.status);
        o.push_back(2.0);
    }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
