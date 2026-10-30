// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `boolean.api_brep`: the boolean entry points of
// source/SM_API/src/SmApiBrep.cpp (SmApiBoolean, SmApiBooleanUnion, SmApiBooleanDifference,
// SmApiBooleanIntersection, SmApiBooleanMerge, SmApiMergeBreps, SmApiBooleanWithOptions,
// SmApiBooleanLists, SmApiBooleanTrees, SmApiBooleanWithCurves, SmApiBoolean2d,
// SmApiNonManifoldBoolean, SmApiPiecewiseMerge) and its non-manifold sweeps of a B-rep
// (SmApiNonManifoldSweep, SmApiNonManifoldRotationalSweep), and SmApiTessellateBoundaries of
// an operand, called exactly as a caller of the API would.
// OP_CHECKED also runs the capi check sequence (SmBrep::ValidatePointers, then
// SmBrep::AssertValid) on an SmApiBoolean result.
//
// One entry point, sc_boolean_api_brep_run(in, n_in, out, cap, out_len). `in` is
// `op args...`; layouts are documented in crates/solidcore-ffi/src/units/boolean_api_brep.rs,
// which validates every input so upstream only touches valid memory. Every read is bounds
// checked here as well (a malformed program is SC_ERR_ARG). The whole program is parsed before
// any upstream object is built. Operand B-reps are boxes (SmApiCreateBox), spheres
// (SmApiCreateSphere), sheets (SmApiCreatePlane), planar disks (SmApiCreatePlanarCircle),
// tori (SmApiCreateTorus), cones (SmApiCreateCone), an operand moved
// by SmApiRotate / SmApiTranslate steps, a skin solid (SmApiCreateSkinPrimitive of NURB
// curves), cylinders (SmApiCreateCylinder) or the SmApiBoolean result of two operands; a failing
// constructor or operand Boolean is SC_ERR_ARG, except a failing SmApiCreateTorus, whose
// status becomes the call's (record `2`) so its radius guard is compared. `out` receives the
// API status (0 = SM_SUCCESS, else the SmStatus) and, on success, the op's outputs.
//
// Ownership follows source/SM_API/inc/SmApiBrep.h: a rejected or failed two-B-rep call leaves
// both operands with the caller (deleted here); on success the result is deleted and the second
// operand is deleted here only when upstream keeps it. OP_ATTRIBUTED tags every operand face
// and edge with an SmLongAttribute before SmApiBoolean and records the attributes the result's
// faces and edges carry (the Rust side tags its own operands the same way); OP_ATTRIBUTED_LIST
// does the same for SmApiMergeBreps, SmApiBooleanLists and SmApiBooleanTrees and their
// output B-reps. A failed N-ary call after validation
// may have consumed some operands, so those are leaked rather than risk a double delete.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmApiBrep.h>
#include <SmApiGeneral.h>
#include <SmApiPrimitives.h>
#include <SmApiQueries.h>
#include <SmAssertArray.h>
#include <SmAttribute.h>
#include <SmTopology.h>
#include <SmBSplineCurve.h>
#include <SmBrep.h>
#include <SmContext.h>
#include <SmEdge.h>
#include <SmFace.h>
#include <SmFaceuse.h>
#include <SmPrimitiveCreation.h>
#include <SmVertex.h>
#include <SmRegion.h>
#include <SmShell.h>
#include <SmSurface.h>
#include <SmTArray.h>
#include <SmVector3d.h>
#include <nurbs.h>

#include <algorithm>
#include <set>
#include <map>
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
    OP_WITH_CURVES = 11,
    OP_BOOLEAN_2D = 12,
    OP_NON_MANIFOLD = 13,
    OP_PIECEWISE = 14,
    OP_NM_SWEEP = 15,
    OP_NM_ROTATIONAL_SWEEP = 16,
    OP_ATTRIBUTED = 17,
    OP_TESSELLATE_BOUNDARIES = 18,
    OP_ATTRIBUTED_LIST = 19,
    OP_CHECKED = 20,
    OP_ATTRIBUTED_NM_SWEEP = 21,            // `behavior` then OP_NM_SWEEP's program
    OP_ATTRIBUTED_NM_ROTATIONAL_SWEEP = 22, // `behavior` then OP_NM_ROTATIONAL_SWEEP's
};

// Most entries of one non-manifold sweep selection array (MAX_SELECTION in the Rust module).
const int64_t MAX_SELECTION = 16;

// Operand kinds.
enum : int32_t
{
    BK_BOX = 1,      // SmApiCreateBox(origin, length, width, height)
    BK_SPHERE = 2,   // SmApiCreateSphere(center, radius)
    BK_PLANE = 3,    // SmApiCreatePlane(origin, x, y)
    BK_BOOLEAN = 4,  // SmApiBoolean(a, b, operation)'s result
    BK_TORUS = 5,    // SmApiCreateTorus(center, major, minor)
    BK_MOVED = 6,    // an operand moved by SmApiRotate / SmApiTranslate steps
    BK_SKIN = 7,     // SmApiCreateSkinPrimitive(counts, NURB curves, degree, cap)
    BK_DISK = 8,     // SmApiCreatePlanarCircle(origin, radius)
    BK_CYLINDER = 9, // SmApiCreateCylinder(origin, radius, height), the SmBrep*& overload
    BK_CONE = 10,    // SmApiCreateCone(origin, radius_base, radius_top, height), the SmBrep*& overload
};

// Most curves (and profiles) of one BK_SKIN operand (MAX_SKIN_CURVES in the Rust module),
// and most control points of one of its curves (MAX_SKIN_POINTS).
const int64_t MAX_SKIN_CURVES = 16;
const int64_t MAX_SKIN_POINTS = 512;

// Steps of a BK_MOVED operand.
enum : int32_t
{
    STEP_ROTATE = 0,    // SmApiRotate(brep, point, axis, angle_deg)
    STEP_TRANSLATE = 1, // SmApiTranslate(brep, vector)
};

// Most steps of one BK_MOVED operand (MAX_STEPS in the Rust module).
const int64_t MAX_STEPS = 8;

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
    std::vector<BrepSpec> sub; // BK_BOOLEAN: a, b; BK_MOVED: the operand
    std::vector<double> steps; // BK_MOVED: per step its kind then 7 (rotate) or 3 numbers
    // BK_SKIN: profile counts, then per curve `p n+1 (n+1)*4 points (n+p+2) knots`, and in
    // d[0] d[1] the loft degree and cap flag
    std::vector<double> counts, curves;
};

BrepSpec read_spec(Rd& rd, int depth = 0)
{
    if (depth >= MAX_DEPTH)
        throw Bad();
    BrepSpec s;
    s.kind = (int32_t)rd.idx(1, 10);
    int k = 0;
    switch (s.kind)
    {
    case BK_BOX: k = 6; break;
    case BK_SPHERE: k = 4; break;
    case BK_PLANE: k = 5; break;
    case BK_BOOLEAN: k = 0; break;
    case BK_TORUS: k = 5; break;
    case BK_MOVED: k = 0; break;
    case BK_SKIN: k = 0; break;
    case BK_DISK: k = 4; break;
    case BK_CYLINDER: k = 5; break;
    case BK_CONE: k = 6; break;
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
    if (s.kind == BK_MOVED)
    {
        int64_t n = rd.idx(0, MAX_STEPS);
        for (int64_t i = 0; i < n; i++)
        {
            int64_t t = rd.idx(STEP_ROTATE, STEP_TRANSLATE);
            s.steps.push_back((double)t);
            int m = t == STEP_ROTATE ? 7 : 3;
            for (int j = 0; j < m; j++)
                s.steps.push_back(rd.num());
        }
        s.sub.push_back(read_spec(rd, depth + 1));
    }
    if (s.kind == BK_SKIN)
    {
        int64_t np = rd.idx(2, MAX_SKIN_CURVES);
        int64_t total = 0;
        for (int64_t i = 0; i < np; i++)
        {
            int64_t c = rd.idx(1, MAX_SKIN_CURVES);
            total += c;
            s.counts.push_back((double)c);
        }
        int64_t nc = rd.idx(1, MAX_SKIN_CURVES);
        if (nc != total)
            throw Bad();
        for (int64_t i = 0; i < nc; i++)
        {
            int64_t p = rd.idx(1, 17);
            int64_t n1 = rd.idx(2, MAX_SKIN_POINTS);
            if (p >= n1)
                throw Bad();
            s.curves.push_back((double)p);
            s.curves.push_back((double)n1);
            for (int64_t j = 0; j < 4 * n1 + n1 + p + 1; j++)
                s.curves.push_back(rd.num());
        }
        s.d[0] = (double)rd.idx(1, 17);
        s.d[1] = rd.flag() ? 1.0 : 0.0;
    }
    return s;
}

// The BK_SKIN operand: SmApiCreateSkinPrimitive of its curves (each an
// SmBSplineCurve(gw_CURVE*, 3, FALSE)), which leaves the curves with the caller.
SmApiStatus build_skin(const BrepSpec& s, SmBrep*& r)
{
    const SmContext& ctx = *SmApiGetOrCreateContext();
    std::vector<SmBSplineCurve*> owned;
    SmTArray<ULONG> counts;
    SmTArray<const SmCurve*> curves;
    for (size_t i = 0; i < s.counts.size(); i++)
        counts.Add((ULONG)s.counts[i]);
    const double* d = s.curves.data();
    size_t at = 0;
    while (at < s.curves.size())
    {
        int64_t p = (int64_t)d[at++];
        int64_t n1 = (int64_t)d[at++];
        gw_CURVE* cur = sm_AllocateNurbCurve((gw_INDEX)(n1 - 1), (gw_DEGREE)p, (gw_INDEX)(n1 + p));
        for (int64_t j = 0; j < n1; j++)
        {
            cur->pol->Pw[j].x = d[at++];
            cur->pol->Pw[j].y = d[at++];
            cur->pol->Pw[j].z = d[at++];
            cur->pol->Pw[j].w = d[at++];
        }
        for (int64_t j = 0; j <= n1 + p; j++)
            cur->knt->U[j] = d[at++];
        owned.push_back(new (ctx) SmBSplineCurve(cur, 3, FALSE));
        curves.Add(owned.back());
    }
    SmApiStatus stat = SmApiCreateSkinPrimitive(counts, curves, (ULONG)s.d[0],
                                                s.d[1] != 0.0 ? TRUE : FALSE, r, NULL, NULL, NULL);
    for (size_t i = 0; i < owned.size(); i++)
        delete owned[i];
    return stat;
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
    case BK_SKIN: stat = build_skin(s, r); break;
    case BK_DISK: stat = SmApiCreatePlanarCircle(o, s.d[3], r); break;
    case BK_CYLINDER: stat = SmApiCreateCylinder(o, s.d[3], s.d[4], r); break;
    case BK_CONE: stat = SmApiCreateCone(o, s.d[3], s.d[4], s.d[5], r); break;
    case BK_MOVED:
    {
        r = build(s.sub[0]);
        stat = SM_SUCCESS;
        const std::vector<double>& p = s.steps;
        for (size_t i = 0; i < p.size() && stat == SM_SUCCESS;)
        {
            if ((int32_t)p[i] == STEP_ROTATE)
            {
                stat = SmApiRotate(r, SmVector3d(p[i + 1], p[i + 2], p[i + 3]),
                                   SmVector3d(p[i + 4], p[i + 5], p[i + 6]), p[i + 7]);
                i += 8;
            }
            else
            {
                stat = SmApiTranslate(r, SmVector3d(p[i + 1], p[i + 2], p[i + 3]));
                i += 4;
            }
        }
        break;
    }
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

// The tail of a successful OP_BOOLEAN_2D (with a result) or sweep: `faces` then the face
// areas in ascending order (a failed face as minus its status), then the number of those
// values. No volume: these results are sheets or non-manifold.
void put_areas(std::vector<double>& o, SmBrep* b)
{
    size_t start = o.size();
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

// OP_ATTRIBUTED: the first application attribute id, and the accuracy of the edge lengths
// (SmApiEdgeComputeLength's absolute accuracy).
const ULONG ATTRIBUTE_BASE = 10007;
const double LENGTH_ACCURACY = 1e-9;

// Gives every face (GetFaces order) and edge (GetEdges order) of operand `operand` its own
// SmLongAttribute: id ATTRIBUTE_BASE + 1000 operand + 500 kind + index (kind 0 face, 1
// edge), value the id, behaviour `behavior`: one attribute id per origin, as a history
// caller tags its operands.
void tag_operand(SmBrep* b, ULONG operand, SmAttributeBehaviorType behavior)
{
    const SmContext& ctx = *SmApiGetOrCreateContext();
    SmTArray<SmFace*> faces;
    b->GetFaces(faces);
    for (ULONG i = 0; i < faces.GetSize(); i++)
    {
        ULONG id = ATTRIBUTE_BASE + 1000 * operand + i;
        faces[i]->AddAttribute(new (ctx) SmLongAttribute(id, (long)id, behavior));
    }
    SmTArray<SmEdge*> edges;
    b->GetEdges(edges);
    for (ULONG i = 0; i < edges.GetSize(); i++)
    {
        ULONG id = ATTRIBUTE_BASE + 1000 * operand + 500 + i;
        edges[i]->AddAttribute(new (ctx) SmLongAttribute(id, (long)id, behavior));
    }
}

// One object's row: `measure n` then per attribute `id behavior value users` (value: an
// SmLongAttribute's GetValue, else 0; users: the GetUsers that are SmTopology objects,
// within `scope` when given), the attributes in ascending
// order: which of two merged coplanar faces survives (and so whose attributes come first
// in its list) depends on the C++ process's earlier heap use, the set does not.
std::vector<double> attribute_row(const SmAObject* obj, double measure,
                                  const std::set<const SmAObject*>* scope = NULL)
{
    SmTArray<SmAttribute*> attrs;
    obj->GetAttributes(attrs);
    std::vector<std::vector<double>> entries;
    for (ULONG i = 0; i < attrs.GetSize(); i++)
    {
        SmAttribute* a = attrs[i];
        SmLongAttribute* l = dynamic_cast<SmLongAttribute*>(a);
        SmTArray<SmAObject*> users;
        a->GetUsers(users);
        std::vector<double> e;
        e.push_back((double)a->GetAttributeID());
        e.push_back((double)a->GetBehavior());
        e.push_back(l != NULL ? (double)l->GetValue() : 0.0);
        // the topology users (the port keeps no attributes on geometry, whose objects,
        // an edge's curve, also use an edge's reference attribute here), and with a
        // `scope` only the users that are objects of the same B-rep
        double n = 0.0;
        for (ULONG u = 0; u < users.GetSize(); u++)
            if (users[u] != NULL && users[u]->IsKindOf(SmTopology_TYPE) &&
                (scope == NULL || scope->count(users[u]) != 0))
                n += 1.0;
        e.push_back(n);
        entries.push_back(e);
    }
    std::sort(entries.begin(), entries.end());
    std::vector<double> row;
    row.push_back(measure);
    row.push_back((double)entries.size());
    for (size_t i = 0; i < entries.size(); i++)
        row.insert(row.end(), entries[i].begin(), entries[i].end());
    return row;
}

// Rows sorted by their attribute part (from index 1), then by measure, so the order of
// the result's lists does not matter.
bool row_less(const std::vector<double>& x, const std::vector<double>& y)
{
    if (std::lexicographical_compare(x.begin() + 1, x.end(), y.begin() + 1, y.end()))
        return true;
    if (std::lexicographical_compare(y.begin() + 1, y.end(), x.begin() + 1, x.end()))
        return false;
    return x[0] < y[0];
}

void put_rows(std::vector<double>& o, std::vector<std::vector<double>>& rows)
{
    std::sort(rows.begin(), rows.end(), row_less);
    o.push_back((double)rows.size());
    for (size_t i = 0; i < rows.size(); i++)
        o.insert(o.end(), rows[i].begin(), rows[i].end());
}

// `faces rows... edges rows...` of a B-rep: each face's row with its area
// (SmApiFaceComputeArea at AREA_ACCURACY; a failure as minus its status) and each edge's
// with its length (SmApiEdgeComputeLength at LENGTH_ACCURACY, likewise).
void put_attributes(std::vector<double>& o, SmBrep* b, bool scoped = false)
{
    std::vector<std::vector<double>> rows;
    SmTArray<SmFace*> faces;
    b->GetFaces(faces);
    SmTArray<SmEdge*> edges;
    b->GetEdges(edges);
    // `scoped`: users counted within `b` (the port keeps one attribute store per B-rep,
    // so a reference attribute shared with another B-rep is one attribute per B-rep)
    std::set<const SmAObject*> objects;
    for (ULONG i = 0; i < faces.GetSize(); i++)
        objects.insert(faces[i]);
    for (ULONG i = 0; i < edges.GetSize(); i++)
        objects.insert(edges[i]);
    const std::set<const SmAObject*>* scope = scoped ? &objects : NULL;
    for (ULONG i = 0; i < faces.GetSize(); i++)
    {
        double area = 0.0;
        SmApiStatus astat = SmApiFaceComputeArea(faces[i], AREA_ACCURACY, area);
        rows.push_back(attribute_row(faces[i], astat == SM_SUCCESS ? area : -(double)st(astat), scope));
    }
    put_rows(o, rows);
    rows.clear();
    for (ULONG i = 0; i < edges.GetSize(); i++)
    {
        double length = 0.0;
        SmApiStatus lstat = SmApiEdgeComputeLength(edges[i], LENGTH_ACCURACY, length);
        rows.push_back(attribute_row(edges[i], lstat == SM_SUCCESS ? length : -(double)st(lstat), scope));
    }
    put_rows(o, rows);
}

// OP_ATTRIBUTED `brep brep operation behavior [copy_first flags x5]`: tags both operands
// (tag_operand, operand 0 and 1), runs SmApiBoolean (or, with the optional tail,
// SmApiBooleanWithOptions with those five flags, the first operand first replaced by its
// SmApiBrepCopy when `copy_first`), and records `status` then on success the two-B-rep
// record of run_two followed by the attribute rows of the result and of a kept second
// operand (put_attributes) and the number of those values last (as OP_MEASURED's tail).
void run_attributed(Rd& rd, std::vector<double>& o)
{
    BrepSpec a = read_spec(rd);
    BrepSpec b = read_spec(rd);
    int32_t op = (int32_t)rd.idx(0, 10);
    SmAttributeBehaviorType behavior = (SmAttributeBehaviorType)rd.idx(0, 4);
    bool with_options = rd.at != rd.n;
    bool copy_first = false;
    bool flags[5] = {false, false, true, false, false};
    if (with_options)
    {
        copy_first = rd.flag();
        for (int i = 0; i < 5; i++)
            flags[i] = rd.flag();
    }
    if (rd.at != rd.n)
        throw Bad();
    std::vector<BrepSpec> specs;
    specs.push_back(a);
    specs.push_back(b);
    std::vector<SmBrep*> bs = build_all(specs);
    SmBrep* b1 = bs[0];
    SmBrep* b2 = bs[1];
    tag_operand(b1, 0, behavior);
    tag_operand(b2, 1, behavior);
    if (copy_first)
    {
        SmBrep* copy = NULL;
        SmApiStatus cstat = SmApiBrepCopy(b1, copy);
        delete b1;
        if (cstat != SM_SUCCESS || copy == NULL)
        {
            o.push_back(st(cstat != SM_SUCCESS ? cstat : SM_ERR));
            delete b2;
            return;
        }
        b1 = copy;
    }
    SmBrep* res = b1;
    SmApiStatus stat = with_options
        ? SmApiBooleanWithOptions(b1, b2, (SmBooleanOperationType)op, flags[0], flags[1],
                                  flags[2], flags[3], flags[4], res)
        : SmApiBoolean(b1, b2, (SmBooleanOperationType)op, res);
    o.push_back(st(stat));
    if (stat != SM_SUCCESS)
    {
        if (res != NULL && res != b1 && res != b2)
            delete res;
        delete b1;
        delete b2;
        return;
    }
    bool kept = keeps_other(op, with_options && flags[4]);
    o.push_back(flg(res != NULL));
    if (res == NULL)
        return;
    o.push_back(flg(res == b1));
    o.push_back(flg(kept));
    put_brep(o, res);
    if (kept && b2 != res)
        put_brep(o, b2);
    size_t start = o.size();
    put_attributes(o, res);
    if (kept && b2 != res)
    {
        put_attributes(o, b2);
        delete b2;
    }
    o.push_back((double)(o.size() - start));
    delete res;
}

// The failed reports (!m_bOK) of one check list as `groups` then per
// (m_lReportingType, m_lListIndex, m_lTestIndex), in ascending order, `type list test
// count`: which tests fail and how often, not the entities they name (the two sides
// number a boolean result's entities differently).
void put_failed_tests(std::vector<double>& o, SmAssertArray& reports)
{
    std::map<std::vector<double>, long> groups;
    for (ULONG i = 0; i < reports.GetSize(); i++)
    {
        SmAssertReport* r = reports[i];
        if (r != NULL && !r->m_bOK)
            groups[{(double)r->m_lReportingType, (double)r->m_lListIndex, (double)r->m_lTestIndex}]++;
    }
    o.push_back((double)groups.size());
    for (std::map<std::vector<double>, long>::const_iterator g = groups.begin(); g != groups.end(); ++g)
    {
        o.insert(o.end(), g->first.begin(), g->first.end());
        o.push_back((double)g->second);
    }
}

// OP_CHECKED `brep brep operation level`: SmApiBoolean, then on success the two-B-rep
// record of run_two followed by the capi check of the result at `level`
// (SmAssertTestLevel 0 to 2): `pointers_status` (ValidatePointers' SmStatus, its default
// SM_NO_WALK) and its failed tests (put_failed_tests), then `assert_valid` (-1 when
// skipped because ValidatePointers reported a failure, else AssertValid's SmBoolean with
// SM_WALK) and its failed tests, then the number of those values (as OP_MEASURED's tail).
void run_checked(Rd& rd, std::vector<double>& o)
{
    BrepSpec a = read_spec(rd);
    BrepSpec b = read_spec(rd);
    int32_t op = (int32_t)rd.idx(0, 10);
    SmAssertTestLevel level = (SmAssertTestLevel)rd.idx(0, 2);
    if (rd.at != rd.n)
        throw Bad();
    std::vector<BrepSpec> specs;
    specs.push_back(a);
    specs.push_back(b);
    std::vector<SmBrep*> bs = build_all(specs);
    SmBrep* b1 = bs[0];
    SmBrep* b2 = bs[1];
    SmBrep* res = b1;
    SmApiStatus stat = SmApiBoolean(b1, b2, (SmBooleanOperationType)op, res);
    o.push_back(st(stat));
    if (stat != SM_SUCCESS)
    {
        if (res != NULL && res != b1 && res != b2)
            delete res;
        delete b1;
        delete b2;
        return;
    }
    bool kept = keeps_other(op, false);
    o.push_back(flg(res != NULL));
    if (res == NULL)
        return;
    o.push_back(flg(res == b1));
    o.push_back(flg(kept));
    put_brep(o, res);
    if (kept && b2 != res)
        put_brep(o, b2);
    size_t start = o.size();
    SmAssertArray pointers, asserts;
    o.push_back((double)res->ValidatePointers(&pointers, level));
    put_failed_tests(o, pointers);
    bool failed = false;
    for (ULONG i = 0; i < pointers.GetSize(); i++)
        failed |= pointers[i] != NULL && !pointers[i]->m_bOK;
    o.push_back(failed ? -1.0 : flg(res->AssertValid(&asserts, level, SM_WALK) != FALSE));
    put_failed_tests(o, asserts);
    o.push_back((double)(o.size() - start));
    if (kept && b2 != res)
        delete b2;
    delete res;
}

// OP_ATTRIBUTED_LIST's tagging: every B-rep `i` of `bs` tagged as operand `i`
// (tag_operand) when `behavior` is one (>= 0).
void tag_all(const std::vector<SmBrep*>& bs, int32_t behavior)
{
    if (behavior < 0)
        return;
    for (size_t i = 0; i < bs.size(); i++)
        tag_operand(bs[i], (ULONG)i, (SmAttributeBehaviorType)behavior);
}

// OP_ATTRIBUTED_LIST's tail after a successful record: put_attributes of each non-null
// output B-rep in order, then the number of those values.
void put_list_attributes(std::vector<double>& o, SmBrep* const* bs, ULONG n, int32_t behavior)
{
    if (behavior < 0)
        return;
    size_t start = o.size();
    for (ULONG i = 0; i < n; i++)
        if (bs[i] != NULL)
            put_attributes(o, bs[i], true);
    o.push_back((double)(o.size() - start));
}

void run_merge_breps(Rd& rd, std::vector<double>& o, int32_t behavior = -1)
{
    std::vector<BrepSpec> specs = read_specs(rd, 16);
    int32_t op = (int32_t)rd.idx(0, 10);
    if (rd.at != rd.n)
        throw Bad();
    std::vector<SmBrep*> bs = build_all(specs);
    tag_all(bs, behavior);
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
    {
        put_list_attributes(o, &res, 0, behavior);
        return;
    }
    put_brep(o, res);
    put_list_attributes(o, &res, 1, behavior);
    delete res;
}

void run_lists(Rd& rd, std::vector<double>& o, int32_t behavior = -1)
{
    std::vector<BrepSpec> s1 = read_specs(rd, 8);
    std::vector<BrepSpec> s2 = read_specs(rd, 8);
    int64_t op = rd.idx(-(int64_t)1 << 31, ((int64_t)1 << 31) - 1);
    if (rd.at != rd.n)
        throw Bad();
    std::vector<BrepSpec> all(s1);
    all.insert(all.end(), s2.begin(), s2.end());
    std::vector<SmBrep*> bs = build_all(all);
    tag_all(bs, behavior);
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
        std::vector<SmBrep*> outs;
        for (ULONG i = 0; i < rb.GetSize(); i++)
            outs.push_back(rb[i]);
        put_list_attributes(o, outs.data(), (ULONG)outs.size(), behavior);
    }
    // The inputs are never modified; outputs are new objects.
    for (ULONG i = 0; i < rb.GetSize(); i++)
        delete rb[i];
    for (ULONG i = 0; i < rs.GetSize(); i++)
        delete rs[i];
    for (size_t i = 0; i < bs.size(); i++)
        delete bs[i];
}

void run_trees(Rd& rd, std::vector<double>& o, int32_t behavior = -1)
{
    std::vector<BrepSpec> specs = read_specs(rd, 16);
    int64_t t = rd.idx(0, 64);
    SmTArray<long> tree;
    for (int64_t i = 0; i < t; i++)
        tree.Add((long)rd.idx(-64, 64));
    if (rd.at != rd.n)
        throw Bad();
    std::vector<SmBrep*> bs = build_all(specs);
    tag_all(bs, behavior);
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
    std::vector<SmBrep*> outs;
    for (ULONG i = 0; i < arr.GetSize(); i++)
        outs.push_back(arr[i]);
    put_list_attributes(o, outs.data(), (ULONG)outs.size(), behavior);
    for (ULONG i = 0; i < arr.GetSize(); i++)
        delete arr[i];
}

// OP_ATTRIBUTED_LIST `behavior inner...`: the OP_MERGE_BREPS, OP_LISTS or OP_TREES call
// `inner` with every B-rep of its lists (in input order, the lists of OP_LISTS
// concatenated) tagged as operand `i` (tag_operand); a successful record ends with the
// attribute rows of each output B-rep (put_attributes) and the number of those values.
void run_attributed_list(Rd& rd, std::vector<double>& o)
{
    int32_t behavior = (int32_t)rd.idx(0, 4);
    switch ((int32_t)rd.idx(1, 17))
    {
    case OP_MERGE_BREPS: run_merge_breps(rd, o, behavior); break;
    case OP_LISTS: run_lists(rd, o, behavior); break;
    case OP_TREES: run_trees(rd, o, behavior); break;
    default: throw Bad();
    }
}

// The index of `e` in `b`'s GetEdges list, -1 when `b` no longer has it (compared by
// address only: an edge the merge deleted is never dereferenced).
double edge_index(const SmBrep* b, const SmEdge* e)
{
    SmTArray<SmEdge*> edges;
    b->GetEdges(edges);
    for (ULONG i = 0; i < edges.GetSize(); i++)
        if (edges[i] == e)
            return (double)i;
    return -1.0;
}

// The two-B-rep entry points added with SmApiBooleanWithCurves: OP_WITH_CURVES and
// OP_NON_MANIFOLD (`brep brep operation`, the two-B-rep record; OP_WITH_CURVES then
// `n` and each intersection edge's index in the result's GetEdges, -1 when gone),
// OP_BOOLEAN_2D (`brep brep 2d_operation`: `assigned`, then the result and its areas
// (put_areas), or both operands when an empty operand leaves rpResult unassigned) and OP_PIECEWISE
// (`brep brep create_new`: `has is_first`, the result, then the second operand, which
// upstream imprints but keeps).
void run_more_two(Rd& rd, int32_t api, std::vector<double>& o)
{
    BrepSpec a = read_spec(rd);
    BrepSpec b = read_spec(rd);
    int32_t op = SM_BO_UNKNOWN;
    bool create_new = false;
    switch (api)
    {
    case OP_WITH_CURVES:
    case OP_NON_MANIFOLD: op = (int32_t)rd.idx(0, 10); break;
    case OP_BOOLEAN_2D: op = (int32_t)rd.idx(0, 4); break;
    case OP_PIECEWISE: create_new = rd.flag(); break;
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
    SmBrep* res = NULL;
    SmTArray<SmEdge*> edges;
    SmApiStatus stat = SM_ERR;
    switch (api)
    {
    case OP_WITH_CURVES:
        stat = SmApiBooleanWithCurves(b1, b2, (SmBooleanOperationType)op, res, edges);
        break;
    case OP_NON_MANIFOLD:
        stat = SmApiNonManifoldBoolean(b1, b2, (SmBooleanOperationType)op, res);
        break;
    case OP_BOOLEAN_2D:
        stat = SmApiBoolean2d(b1, b2, (Sm2DBooleanOperationType)op, res);
        break;
    case OP_PIECEWISE:
        stat = SmApiPiecewiseMerge(b1, b2, create_new ? TRUE : FALSE, res);
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
    if (api == OP_BOOLEAN_2D)
    {
        // Assigned: rpResult aliases pBrep1 and pBrep2 was deleted; otherwise (an operand
        // without faces) both are unchanged.
        o.push_back(flg(res != NULL));
        if (res == NULL)
        {
            put_brep(o, b1);
            put_brep(o, b2);
            delete b1;
            delete b2;
            return;
        }
        put_brep(o, res);
        put_areas(o, res);
        delete res;
        return;
    }
    if (api == OP_PIECEWISE)
    {
        o.push_back(flg(res != NULL));
        if (res == NULL)
            return; // ownership unknown; leak
        o.push_back(flg(res == b1));
        put_brep(o, res);
        if (b2 != res)
        {
            put_brep(o, b2);
            delete b2;
        }
        delete res;
        return;
    }
    bool kept = keeps_other(op, false);
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
    if (api == OP_WITH_CURVES)
    {
        o.push_back((double)edges.GetSize());
        for (ULONG i = 0; i < edges.GetSize(); i++)
            o.push_back(edge_index(res, edges[i]));
    }
    delete res;
}

// One selection array of a non-manifold sweep: `count` (-1 for a NULL array) then the
// entries, each an index into the B-rep's list or -1 for a NULL entry.
template <class T>
bool read_selection(Rd& rd, const SmTArray<T*>& all, SmTArray<T*>& out)
{
    int64_t n = rd.idx(-1, MAX_SELECTION);
    for (int64_t i = 0; i < n; i++)
    {
        int64_t at = rd.idx(-1, (int64_t)all.GetSize() - 1);
        out.Add(at < 0 ? (T*)NULL : all[(ULONG)at]);
    }
    return n >= 0;
}

// OP_NM_SWEEP (`brep dir[3] dist stitch merge region` then the selections) and
// OP_NM_ROTATIONAL_SWEEP (`brep base[3] axis[3] angle stitch merge region` then the
// selections): the status, then the swept B-rep and its areas (put_areas) on success, the
// untouched B-rep on SM_ERR_INVALID_INPUT. `region` is pRegionBrep: 0 NULL, 1 the swept
// B-rep, 2 another B-rep (SmApiCreateBox(origin, 1, 1, 1)). With `behavior` (>= 0, the
// OP_ATTRIBUTED_* ops) every face and edge of the B-rep is tagged first (tag_operand,
// operand 0), and a successful sweep's record ends with its attribute rows
// (put_attributes) and the number of those values, after the areas.
void run_sweep(Rd& rd, int32_t api, std::vector<double>& o, int64_t behavior = -1)
{
    BrepSpec spec = read_spec(rd);
    double v[7] = {0, 0, 0, 0, 0, 0, 0};
    int nv = api == OP_NM_SWEEP ? 4 : 7;
    for (int i = 0; i < nv; i++)
        v[i] = rd.num();
    bool stitch = rd.flag();
    bool merge = rd.flag();
    int64_t region = rd.idx(0, 2);
    // The selections index the operand's lists, so it is built first; the rest of the
    // program is still read before upstream is called.
    std::vector<BrepSpec> specs;
    specs.push_back(spec);
    std::vector<SmBrep*> bs = build_all(specs);
    SmBrep* brep = bs[0];
    SmTArray<SmFace*> faces, sel_faces;
    SmTArray<SmEdge*> edges, sel_edges;
    SmTArray<SmVertex*> vertices, sel_vertices;
    brep->GetFaces(faces);
    brep->GetEdges(edges);
    brep->GetVertices(vertices);
    bool has_faces = false, has_edges = false, has_vertices = false;
    try
    {
        has_faces = read_selection(rd, faces, sel_faces);
        has_edges = read_selection(rd, edges, sel_edges);
        has_vertices = read_selection(rd, vertices, sel_vertices);
        if (rd.at != rd.n)
            throw Bad();
    }
    catch (...)
    {
        delete brep;
        throw;
    }
    SmBrep* other = NULL;
    if (region == 2 && (SmApiCreateBox(SmVector3d(0, 0, 0), 1, 1, 1, other) != SM_SUCCESS || !other))
    {
        delete other;
        delete brep;
        throw Bad();
    }
    if (behavior >= 0)
        tag_operand(brep, 0, (SmAttributeBehaviorType)behavior);
    SmBrep* region_brep = region == 0 ? NULL : region == 1 ? brep : other;
    SmTArray<SmFace*>* pf = has_faces ? &sel_faces : NULL;
    SmTArray<SmEdge*>* pe = has_edges ? &sel_edges : NULL;
    SmTArray<SmVertex*>* pv = has_vertices ? &sel_vertices : NULL;
    SmApiStatus stat;
    if (api == OP_NM_SWEEP)
        stat = SmApiNonManifoldSweep(brep, region_brep, SmVector3d(v[0], v[1], v[2]), v[3],
                                     stitch ? TRUE : FALSE, merge ? TRUE : FALSE, pf, pe, pv);
    else
        stat = SmApiNonManifoldRotationalSweep(brep, region_brep, SmPoint3d(v[0], v[1], v[2]),
                                               SmVector3d(v[3], v[4], v[5]), v[6],
                                               stitch ? TRUE : FALSE, merge ? TRUE : FALSE,
                                               pf, pe, pv);
    o.push_back(st(stat));
    if (stat == SM_SUCCESS || stat == SM_ERR_INVALID_INPUT)
        put_brep(o, brep);
    if (stat == SM_SUCCESS)
        put_areas(o, brep);
    if (stat == SM_SUCCESS && behavior >= 0)
    {
        size_t start = o.size();
        put_attributes(o, brep);
        o.push_back((double)(o.size() - start));
    }
    delete brep;
    delete other;
}

// SmApiTessellateBoundaries of an operand whose `GetEdges` edges at the listed indices
// first lose their curve (SetCurve(NULL, TRUE, FALSE, FALSE), as upstream's test): the
// status, then (outputs are kept on SM_ERR) the edges as `GetEdges` indices, the vertex
// counts and the points.
void run_tessellate_boundaries(Rd& rd, std::vector<double>& o)
{
    BrepSpec spec = read_spec(rd);
    double tol = rd.num();
    int64_t nd = rd.idx(0, 64);
    std::vector<int64_t> drop;
    for (int64_t i = 0; i < nd; i++)
        drop.push_back(rd.idx(0, 4095));
    if (rd.at != rd.n)
        throw Bad();
    std::vector<BrepSpec> specs;
    specs.push_back(spec);
    std::vector<SmBrep*> bs = build_all(specs);
    SmBrep* b = bs[0];
    SmTArray<SmEdge*> all;
    b->GetEdges(all);
    for (int64_t d : drop)
    {
        if (d >= (int64_t)all.GetSize())
        {
            delete b;
            throw Bad();
        }
        all[(ULONG)d]->SetCurve(NULL, TRUE, FALSE, FALSE);
    }
    SmTArray<SmPoint3d> points;
    SmTArray<ULONG> counts;
    SmTArray<SmEdge*> edges;
    SmApiStatus stat = SmApiTessellateBoundaries(b, tol, points, counts, edges);
    o.push_back(st(stat));
    o.push_back((double)edges.GetSize());
    for (ULONG i = 0; i < edges.GetSize(); i++)
    {
        double at = -1.0;
        for (ULONG j = 0; j < all.GetSize(); j++)
            if (all[j] == edges[i])
            {
                at = (double)j;
                break;
            }
        o.push_back(at);
    }
    o.push_back((double)counts.GetSize());
    for (ULONG i = 0; i < counts.GetSize(); i++)
        o.push_back((double)counts[i]);
    o.push_back((double)points.GetSize());
    for (ULONG i = 0; i < points.GetSize(); i++)
    {
        o.push_back(points[i].x);
        o.push_back(points[i].y);
        o.push_back(points[i].z);
    }
    delete b;
}

void run(Rd& rd, std::vector<double>& o)
{
    int32_t api = (int32_t)rd.idx(1, 22);
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
    case OP_WITH_CURVES:
    case OP_BOOLEAN_2D:
    case OP_NON_MANIFOLD:
    case OP_PIECEWISE: run_more_two(rd, api, o); break;
    case OP_NM_SWEEP:
    case OP_NM_ROTATIONAL_SWEEP: run_sweep(rd, api, o); break;
    case OP_ATTRIBUTED: run_attributed(rd, o); break;
    case OP_TESSELLATE_BOUNDARIES: run_tessellate_boundaries(rd, o); break;
    case OP_ATTRIBUTED_LIST: run_attributed_list(rd, o); break;
    case OP_CHECKED: run_checked(rd, o); break;
    case OP_ATTRIBUTED_NM_SWEEP:
    case OP_ATTRIBUTED_NM_ROTATIONAL_SWEEP:
    {
        int64_t behavior = rd.idx(0, 4);
        run_sweep(rd, api - 6, o, behavior);
        break;
    }
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
