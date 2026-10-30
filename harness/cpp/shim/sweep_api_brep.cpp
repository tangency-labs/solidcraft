// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `sweep.api_brep`: the SM_API B-rep sweep entry points of
// source/SM_API/src/SmApiBrep.cpp (SmApiCreateLinearSweep, SmApiCreateRotationalSweep,
// SmApiCreateDraftSweep, SmApiCreatePipeSweep, SmApiCreateSweepAlongPlanarPath,
// SmApiCreateCurveSweep, SmApiCreateCurveSweepFromFaces, SmApiCreateCurveSweepFromEdges,
// SmApiCreateTaperExtrude, SmApiCreateLinearSweepWithRepetitions,
// SmApiCreateRotationalSweepWithRepetitions), called exactly as a caller of the API would.
//
// One entry point, sc_sweep_api_brep_run(in, n_in, out, cap, out_len). `in` is `op args...`;
// layouts are documented in crates/solidcore-ffi/src/units/sweep_api_brep.rs, which validates
// every input so upstream only touches valid memory. Every read is bounds checked here as well
// (a malformed program is SC_ERR_ARG). The whole program is parsed before any upstream object
// is built. `out` receives the API status (0 = SM_SUCCESS, else the SmStatus) and, on success,
// the result's counts.
#include "sc_shim.h"

#include "StdAfx.h"
#include <SmApiBrep.h>
#include <SmApiCurves.h>
#include <SmApiGeneral.h>
#include <SmApiPrimitives.h>
#include <SmApiQueries.h>
#include <SmAttribute.h>
#include <SmTopology.h>
#include <SmVertex.h>
#include <SmBSplineCurve.h>
#include <SmBrep.h>
#include <SmCircle.h>
#include <SmContext.h>
#include <SmCurve.h>
#include <SmEdge.h>
#include <SmFace.h>
#include <SmHermiteCurve.h>
#include <SmLine.h>
#include <SmPrimitiveCreation.h>
#include <SmTArray.h>
#include <SmVector3d.h>

#include <algorithm>
#include <stdexcept>
#include <memory>
#include <vector>

#include "sc_sm.h"

using sc::st;

namespace
{
// Ops (keep in sync with crates/solidcore-ffi/src/units/sweep_api_brep.rs).
enum : int32_t
{
    OP_LINEAR = 1,
    OP_ROTATIONAL = 2,
    OP_DRAFT = 3,
    OP_PIPE = 4,
    OP_PLANAR_PATH = 5,
    OP_CURVE_SWEEP = 6,
    OP_FROM_FACES = 7,
    OP_FROM_EDGES = 8,
    OP_TAPER = 9,
    OP_LINEAR_REPS = 10,
    OP_ROTATIONAL_REPS = 11,
    OP_FROM_FACES_ATTRIBUTED = 12, // `behavior`, then OP_FROM_FACES's program
    OP_FROM_EDGES_ATTRIBUTED = 13, // `behavior`, then OP_FROM_EDGES's program
    OP_FROM_BOX_FACE = 14,
};

// Curve kinds of a curve block.
enum : int32_t
{
    CK_NULL = 0,            // NULL
    CK_API_CURVE = 2,       // SmApiCreateCurve(points)
    CK_API_ARC = 3,         // SmApiCreateArc(center, radius, start, end)
    CK_API_CIRCLE = 4,      // SmApiCreateCircle(center, radius, SmBSplineCurve*&)
    CK_LINE_SEGMENT = 5,    // SmApiCreateLineSegment(start, end)    (an SmLine)
    CK_ANALYTIC_CIRCLE = 6, // SmApiCreateCircle(center, radius, SmCircle*&) (an SmCircle)
    CK_HERMITE = 7,         // SmHermiteCurve(P1, V1, P2, V2)    (SmCurve, no Copy)
};

struct Bad : std::runtime_error
{
    Bad() : std::runtime_error("bad program") {}
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
    std::vector<double> nums(int64_t k)
    {
        std::vector<double> v;
        for (int64_t i = 0; i < k; i++)
            v.push_back(num());
        return v;
    }
};

struct CurveSpec
{
    int32_t kind = CK_NULL;
    std::vector<double> data;
};

CurveSpec read_spec(Rd& rd)
{
    CurveSpec c;
    c.kind = (int32_t)rd.idx(0, 7);
    switch (c.kind)
    {
    case CK_NULL: break;
    case CK_API_CURVE:
    {
        int64_t np = rd.idx(0, 4096);
        c.data = rd.nums(3 * np);
        break;
    }
    case CK_API_ARC: c.data = rd.nums(6); break;
    case CK_API_CIRCLE: c.data = rd.nums(4); break;
    case CK_LINE_SEGMENT: c.data = rd.nums(6); break;
    case CK_ANALYTIC_CIRCLE: c.data = rd.nums(4); break;
    case CK_HERMITE: c.data = rd.nums(12); break;
    default: throw Bad();
    }
    return c;
}

std::vector<CurveSpec> read_specs(Rd& rd)
{
    int64_t k = rd.idx(0, 64);
    std::vector<CurveSpec> v;
    for (int64_t i = 0; i < k; i++)
        v.push_back(read_spec(rd));
    return v;
}

SmVector3d v3(const double* d)
{
    return SmVector3d(d[0], d[1], d[2]);
}

// Builds the curve of a parsed block (NULL for CK_NULL). A failing API constructor is a
// harness error (the Rust side builds the same curves and rejects the same inputs).
SmCurve* build_curve(const CurveSpec& c)
{
    const SmContext& ctx = *SmApiGetOrCreateContext();
    const double* d = c.data.data();
    switch (c.kind)
    {
    case CK_NULL: return NULL;
    case CK_API_CURVE:
    {
        SmTArray<SmPoint3d> pts;
        for (size_t i = 0; i + 2 < c.data.size(); i += 3)
            pts.Add(SmPoint3d(d[i], d[i + 1], d[i + 2]));
        SmBSplineCurve* r = NULL;
        if (SmApiCreateCurve(pts, r) != SM_SUCCESS || r == NULL)
            throw Bad();
        return r;
    }
    case CK_API_ARC:
    {
        SmBSplineCurve* r = NULL;
        if (SmApiCreateArc(v3(d), d[3], d[4], d[5], r) != SM_SUCCESS || r == NULL)
            throw Bad();
        return r;
    }
    case CK_API_CIRCLE:
    {
        SmBSplineCurve* r = NULL;
        if (SmApiCreateCircle(v3(d), d[3], r) != SM_SUCCESS || r == NULL)
            throw Bad();
        return r;
    }
    case CK_LINE_SEGMENT:
    {
        SmLine* r = NULL;
        if (SmApiCreateLineSegment(v3(d), v3(d + 3), r) != SM_SUCCESS || r == NULL)
            throw Bad();
        return r;
    }
    case CK_ANALYTIC_CIRCLE:
    {
        SmCircle* r = NULL;
        if (SmApiCreateCircle(v3(d), d[3], r) != SM_SUCCESS || r == NULL)
            throw Bad();
        return r;
    }
    case CK_HERMITE:
        return new (ctx) SmHermiteCurve(v3(d), v3(d + 3), v3(d + 6), v3(d + 9));
    default: throw Bad();
    }
}

// Owns the curves and breps built for one call.
struct Owned
{
    std::vector<SmCurve*> curves;
    std::vector<SmBrep*> breps;
    ~Owned()
    {
        for (size_t i = 0; i < curves.size(); i++)
            delete curves[i];
        for (size_t i = 0; i < breps.size(); i++)
            delete breps[i];
    }
    SmCurve* add(const CurveSpec& c)
    {
        curves.push_back(NULL);
        SmCurve* r = build_curve(c);
        curves.back() = r;
        return r;
    }
    // A curve where upstream takes SmBSplineCurve* (NULL allowed).
    SmBSplineCurve* add_bspline(const CurveSpec& c)
    {
        if (c.kind == CK_HERMITE)
            throw Bad();
        return (SmBSplineCurve*)add(c);
    }
    SmTArray<SmCurve*> add_all(const std::vector<CurveSpec>& specs)
    {
        SmTArray<SmCurve*> r;
        for (size_t i = 0; i < specs.size(); i++)
            r.Add(add(specs[i]));
        return r;
    }
    // The faces of SmApiCreatePlanarCircle breps.
    SmTArray<SmFace*> discs(const std::vector<double>& circles)
    {
        SmTArray<SmFace*> faces;
        for (size_t i = 0; i + 3 < circles.size(); i += 4)
        {
            SmBrep* b = NULL;
            const double* d = circles.data() + i;
            if (SmApiCreatePlanarCircle(v3(d), d[3], b) != SM_SUCCESS || b == NULL)
                throw Bad();
            breps.push_back(b);
            SmTArray<SmFace*> fs;
            b->GetFaces(fs);
            for (ULONG j = 0; j < fs.GetSize(); j++)
                faces.Add(fs[j]);
        }
        return faces;
    }
};

// Opt-in only: read the actual result before put_brep destroys it. No mutation,
// healing or additional stitching; geometry precedes manifold and volume queries.
void observe_linear(std::vector<double>& o, SmBrep* result)
{
    SmTArray<SmVertex*> vertices; result->GetVertices(vertices);
    SmTArray<SmEdge*> edges; result->GetEdges(edges);
    if (vertices.GetSize() > 4096 || edges.GetSize() > 4096) throw Bad();
    for (ULONG i = 0; i < vertices.GetSize(); ++i) {
        const auto p = vertices[i]->GetPoint(); o.insert(o.end(), {p.x,p.y,p.z});
    }
    for (ULONG i = 0; i < edges.GetSize(); ++i) {
        const auto interval = edges[i]->GetInterval();
        o.insert(o.end(), {interval.GetMin(), interval.GetMax()});
    }
    for (ULONG i = 0; i < edges.GetSize(); ++i) {
        const auto interval = edges[i]->GetInterval();
        SmCurve* curve = edges[i]->GetCurve();
        for (double fraction : {0.17,0.37,0.83}) {
            SmPoint3d p(0,0,0);
            const SmStatus status = curve ? curve->EvaluatePoint(interval.Evaluate(fraction),p) : SM_ERR;
            o.push_back(st(status));
            if (status == SM_SUCCESS) o.insert(o.end(), {p.x,p.y,p.z});
            else o.insert(o.end(), {0.0,0.0,0.0});
        }
    }
    SmBoolean manifold = FALSE;
    const SmStatus manifold_status = SmApiBrepIsManifoldSolid(result, manifold);
    o.push_back(st(manifold_status));
    o.push_back(manifold_status == SM_SUCCESS && manifold ? 1.0 : 0.0);
    double volume = 0.0;
    const SmStatus volume_status = SmApiBrepComputeVolume(result, 1e-6, volume);
    o.push_back(st(volume_status));
    o.push_back(volume_status == SM_SUCCESS ? volume : 0.0);
}

// The status then, on success, `has [faces edges vertices]`.
void put_brep(std::vector<double>& o, SmApiStatus stat, SmBrep* res, bool observe = false)
{
    o.push_back(st(stat));
    if (stat == SM_SUCCESS)
    {
        o.push_back(res != NULL ? 1.0 : 0.0);
        if (res)
        {
            o.push_back((double)res->GetNumFaces());
            o.push_back((double)res->GetNumEdges());
            o.push_back((double)res->GetNumVertices());
        }
    }
    // Retain owning cleanup even if observing throws.
    std::unique_ptr<SmBrep> cleanup(res);
    if (observe && stat == SM_SUCCESS && res) observe_linear(o, res);
}

// The attributed ops' first attribute id (as boolean.api_brep's OP_ATTRIBUTED).
const ULONG ATTRIBUTE_BASE = 10007;

// Gives every face (GetFaces order) and edge (GetEdges order) of disc B-rep `disc` its own
// SmLongAttribute: id ATTRIBUTE_BASE + 1000 disc + 500 kind + index (kind 0 face, 1 edge),
// value the id, behaviour `behavior`.
void tag_disc(SmBrep* b, ULONG disc, SmAttributeBehaviorType behavior)
{
    const SmContext& ctx = *SmApiGetOrCreateContext();
    SmTArray<SmFace*> faces;
    b->GetFaces(faces);
    for (ULONG i = 0; i < faces.GetSize(); i++)
    {
        ULONG id = ATTRIBUTE_BASE + 1000 * disc + i;
        faces[i]->AddAttribute(new (ctx) SmLongAttribute(id, (long)id, behavior));
    }
    SmTArray<SmEdge*> edges;
    b->GetEdges(edges);
    for (ULONG i = 0; i < edges.GetSize(); i++)
    {
        ULONG id = ATTRIBUTE_BASE + 1000 * disc + 500 + i;
        edges[i]->AddAttribute(new (ctx) SmLongAttribute(id, (long)id, behavior));
    }
}

// One object's row: `n` then per attribute `id behavior value users` (value: an
// SmLongAttribute's GetValue, else 0), the attributes in ascending order.
std::vector<double> attribute_row(const SmAObject* obj)
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
        // the topology users: the port keeps no attributes on geometry, whose
        // objects (an edge's curve) also use an edge's reference attribute here
        ULONG topology_users = 0;
        for (ULONG u = 0; u < users.GetSize(); u++)
            if (users[u] != NULL && users[u]->IsKindOf(SmTopology_TYPE))
                topology_users++;
        e.push_back((double)topology_users);
        entries.push_back(e);
    }
    std::sort(entries.begin(), entries.end());
    std::vector<double> row;
    row.push_back((double)entries.size());
    for (size_t i = 0; i < entries.size(); i++)
        row.insert(row.end(), entries[i].begin(), entries[i].end());
    return row;
}

// `faces rows... edges rows...` of `b`, the rows of each list sorted (the attribute sets
// are compared, not the lists' order).
void put_attribute_rows(std::vector<double>& o, SmBrep* b)
{
    std::vector<std::vector<double>> rows;
    SmTArray<SmFace*> faces;
    b->GetFaces(faces);
    for (ULONG i = 0; i < faces.GetSize(); i++)
        rows.push_back(attribute_row(faces[i]));
    std::sort(rows.begin(), rows.end());
    o.push_back((double)rows.size());
    for (size_t i = 0; i < rows.size(); i++)
        o.insert(o.end(), rows[i].begin(), rows[i].end());
    rows.clear();
    SmTArray<SmEdge*> edges;
    b->GetEdges(edges);
    for (ULONG i = 0; i < edges.GetSize(); i++)
        rows.push_back(attribute_row(edges[i]));
    std::sort(rows.begin(), rows.end());
    o.push_back((double)rows.size());
    for (size_t i = 0; i < rows.size(); i++)
        o.insert(o.end(), rows[i].begin(), rows[i].end());
}

// put_brep, then on success the start, side and end face counts.
void put_sweep(std::vector<double>& o, SmApiStatus stat, SmBrep* res, const SmTArray<SmFace*>& a,
               const SmTArray<SmFace*>& b, const SmTArray<SmFace*>& c)
{
    put_brep(o, stat, res);
    if (stat == SM_SUCCESS)
    {
        o.push_back((double)a.GetSize());
        o.push_back((double)b.GetSize());
        o.push_back((double)c.GetSize());
    }
}

// The trailing sweep arguments: path, scale reference, scale curve, options.
struct SweepArgs
{
    CurveSpec path, scale_ref, scale;
    bool has_options = false;
    bool flags[5] = {false, false, false, false, false};
};

SweepArgs read_sweep_args(Rd& rd)
{
    SweepArgs a;
    a.path = read_spec(rd);
    a.scale_ref = read_spec(rd);
    a.scale = read_spec(rd);
    a.has_options = rd.flag();
    for (int i = 0; i < 5; i++)
        a.flags[i] = rd.flag();
    return a;
}

SmSweepOptions options_of(const SweepArgs& a)
{
    SmSweepOptions o;
    o.bCapEndsArg = a.flags[0] ? TRUE : FALSE;
    o.bTranslationalSweep = a.flags[1] ? TRUE : FALSE;
    o.bMoveProfileCenterToPath = a.flags[2] ? TRUE : FALSE;
    o.bOrientProfilePerpendicularToPath = a.flags[3] ? TRUE : FALSE;
    o.bMoveToProfile = a.flags[4] ? TRUE : FALSE;
    return o;
}

void run(Rd& rd, std::vector<double>& o, bool observe = false)
{
    int32_t op = (int32_t)rd.idx(1, 14);
    if (observe && op != OP_LINEAR) throw Bad();
    // the attributed curve sweeps: the behaviour of the inputs' tags, then the plain op
    int64_t behavior = -1;
    if (op == OP_FROM_FACES_ATTRIBUTED || op == OP_FROM_EDGES_ATTRIBUTED)
    {
        behavior = rd.idx(0, 4);
        op -= 5;
    }
    switch (op)
    {
    case OP_LINEAR:
    case OP_LINEAR_REPS:
    {
        std::vector<CurveSpec> specs = read_specs(rd);
        std::vector<double> dir = rd.nums(4);
        ULONG reps = op == OP_LINEAR_REPS ? (ULONG)rd.idx(0, 1 << 20) : 1;
        bool cap = rd.flag();
        if (rd.at != rd.n)
            throw Bad();
        Owned own;
        SmTArray<SmCurve*> curves = own.add_all(specs);
        SmBrep* res = NULL;
        SmApiStatus stat =
            op == OP_LINEAR
                ? SmApiCreateLinearSweep(curves, v3(dir.data()), dir[3], cap ? TRUE : FALSE, res)
                : SmApiCreateLinearSweepWithRepetitions(curves, v3(dir.data()), dir[3], reps,
                                                        cap ? TRUE : FALSE, res);
        put_brep(o, stat, res, observe);
        break;
    }
    case OP_ROTATIONAL:
    case OP_ROTATIONAL_REPS:
    {
        std::vector<CurveSpec> specs = read_specs(rd);
        std::vector<double> ax = rd.nums(7);
        ULONG reps = op == OP_ROTATIONAL_REPS ? (ULONG)rd.idx(0, 1 << 20) : 1;
        bool cap = rd.flag();
        if (rd.at != rd.n)
            throw Bad();
        Owned own;
        SmTArray<SmCurve*> curves = own.add_all(specs);
        SmBrep* res = NULL;
        SmApiStatus stat =
            op == OP_ROTATIONAL
                ? SmApiCreateRotationalSweep(curves, v3(ax.data()), v3(ax.data() + 3), ax[6],
                                             cap ? TRUE : FALSE, res)
                : SmApiCreateRotationalSweepWithRepetitions(curves, v3(ax.data()),
                                                            v3(ax.data() + 3), ax[6], reps,
                                                            cap ? TRUE : FALSE, res);
        put_brep(o, stat, res);
        break;
    }
    case OP_DRAFT:
    case OP_TAPER:
    {
        std::vector<CurveSpec> specs = read_specs(rd);
        std::vector<double> ha = rd.nums(2);
        bool corner = rd.flag();
        int caps = (int)rd.idx(-16, 16);
        if (rd.at != rd.n)
            throw Bad();
        Owned own;
        SmTArray<SmCurve*> curves = own.add_all(specs);
        SmBrep* res = NULL;
        SmApiStatus stat =
            op == OP_DRAFT
                ? SmApiCreateDraftSweep(curves, ha[0], ha[1], corner ? TRUE : FALSE, caps, res)
                : SmApiCreateTaperExtrude(curves, ha[0], ha[1], caps, corner ? TRUE : FALSE, res);
        put_brep(o, stat, res);
        break;
    }
    case OP_PIPE:
    {
        double radius = rd.num();
        CurveSpec path = read_spec(rd);
        bool cap = rd.flag();
        if (rd.at != rd.n)
            throw Bad();
        Owned own;
        SmBSplineCurve* p = own.add_bspline(path);
        SmBrep* res = NULL;
        SmApiStatus stat = SmApiCreatePipeSweep(radius, p, cap ? TRUE : FALSE, res);
        put_brep(o, stat, res);
        break;
    }
    case OP_PLANAR_PATH:
    {
        std::vector<CurveSpec> prof = read_specs(rd);
        std::vector<CurveSpec> path = read_specs(rd);
        bool move = rd.flag();
        int caps = (int)rd.idx(-16, 16);
        if (rd.at != rd.n)
            throw Bad();
        Owned own;
        SmTArray<SmCurve*> pc = own.add_all(prof);
        SmTArray<SmCurve*> pp = own.add_all(path);
        SmBrep* res = NULL;
        SmApiStatus stat = SmApiCreateSweepAlongPlanarPath(pc, pp, move ? TRUE : FALSE, caps, res);
        put_brep(o, stat, res);
        break;
    }
    case OP_CURVE_SWEEP:
    {
        std::vector<CurveSpec> prof = read_specs(rd);
        SweepArgs a = read_sweep_args(rd);
        if (rd.at != rd.n)
            throw Bad();
        Owned own;
        SmTArray<SmCurve*> pc = own.add_all(prof);
        SmBSplineCurve* path = own.add_bspline(a.path);
        SmBSplineCurve* sref = own.add_bspline(a.scale_ref);
        SmBSplineCurve* scl = own.add_bspline(a.scale);
        SmSweepOptions opts = options_of(a);
        SmBrep* res = NULL;
        SmTArray<SmFace*> s, sd, e;
        SmApiStatus stat = SmApiCreateCurveSweep(pc, path, sref, scl, a.has_options ? &opts : NULL,
                                                 res, &s, &sd, &e);
        put_sweep(o, stat, res, s, sd, e);
        break;
    }
    case OP_FROM_FACES:
    case OP_FROM_EDGES:
    {
        int64_t nf = rd.idx(0, 16);
        std::vector<double> circles = rd.nums(4 * nf);
        SweepArgs a = read_sweep_args(rd);
        if (rd.at != rd.n)
            throw Bad();
        Owned own;
        SmTArray<SmFace*> faces = own.discs(circles);
        if (behavior >= 0)
            for (size_t i = 0; i < own.breps.size(); i++)
                tag_disc(own.breps[i], (ULONG)i, (SmAttributeBehaviorType)behavior);
        SmBSplineCurve* path = own.add_bspline(a.path);
        SmBSplineCurve* sref = own.add_bspline(a.scale_ref);
        SmBSplineCurve* scl = own.add_bspline(a.scale);
        SmSweepOptions opts = options_of(a);
        SmBrep* res = NULL;
        SmTArray<SmFace*> s, sd, e;
        SmApiStatus stat;
        if (op == OP_FROM_FACES)
            stat = SmApiCreateCurveSweepFromFaces(faces, path, sref, scl,
                                                  a.has_options ? &opts : NULL, res, &s, &sd, &e);
        else
        {
            SmTArray<SmEdge*> edges;
            for (ULONG i = 0; i < faces.GetSize(); i++)
            {
                SmTArray<SmEdge*> fe;
                faces[i]->GetEdges(fe);
                for (ULONG j = 0; j < fe.GetSize(); j++)
                    edges.Add(fe[j]);
            }
            stat = SmApiCreateCurveSweepFromEdges(edges, path, sref, scl,
                                                  a.has_options ? &opts : NULL, res, &s, &sd, &e);
        }
        // attributed: the result's rows on success, then each disc's (the borrowed
        // inputs), read before put_sweep deletes the result
        std::vector<double> rows;
        if (behavior >= 0)
        {
            if (stat == SM_SUCCESS && res != NULL)
                put_attribute_rows(rows, res);
            for (size_t i = 0; i < own.breps.size(); i++)
                put_attribute_rows(rows, own.breps[i]);
        }
        put_sweep(o, stat, res, s, sd, e);
        o.insert(o.end(), rows.begin(), rows.end());
        break;
    }
    case OP_FROM_BOX_FACE:
    {
        std::vector<double> box = rd.nums(6);
        int64_t fi = rd.idx(0, 5);
        SweepArgs a = read_sweep_args(rd);
        if (rd.at != rd.n)
            throw Bad();
        Owned own;
        SmBrep* b = NULL;
        if (SmApiCreateBox(v3(box.data()), box[3], box[4], box[5], b) != SM_SUCCESS || b == NULL)
            throw Bad();
        own.breps.push_back(b);
        SmTArray<SmFace*> all;
        b->GetFaces(all);
        if ((ULONG)fi >= all.GetSize())
            throw Bad();
        SmTArray<SmFace*> faces;
        faces.Add(all[(ULONG)fi]);
        SmBSplineCurve* path = own.add_bspline(a.path);
        SmBSplineCurve* sref = own.add_bspline(a.scale_ref);
        SmBSplineCurve* scl = own.add_bspline(a.scale);
        SmSweepOptions opts = options_of(a);
        SmBrep* res = NULL;
        SmTArray<SmFace*> s, sd, e;
        SmApiStatus stat = SmApiCreateCurveSweepFromFaces(faces, path, sref, scl,
                                                          a.has_options ? &opts : NULL, res, &s, &sd, &e);
        // The volume first: put_sweep deletes the result.
        double volume = 0.0;
        SmApiStatus volume_status = SM_SUCCESS;
        if (stat == SM_SUCCESS && res)
            volume_status = SmApiBrepComputeVolume(res, 1e-6, volume);
        put_sweep(o, stat, res, s, sd, e);
        if (stat == SM_SUCCESS && res)
        {
            o.push_back(st(volume_status));
            o.push_back(volume_status == SM_SUCCESS ? volume : 0.0);
        }
        break;
    }
    default: throw Bad();
    }
}
} // namespace

SC_EXPORT int32_t sc_sweep_api_brep_run(const double* in, int64_t n_in, double* out, int64_t cap,
                                        int64_t* out_len)
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
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}

// Additive entry: the legacy entry and its wire format remain unchanged.
SC_EXPORT int32_t sc_sweep_api_brep_observe_linear(const double* in, int64_t n_in,
    double* out, int64_t cap, int64_t* out_len)
{
    SC_TRY
    SmApiGetOrCreateContext();
    Rd rd{in, n_in, 0};
    std::vector<double> o;
    try { run(rd, o, true); }
    catch (const Bad&) { return SC_ERR_ARG; }
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
