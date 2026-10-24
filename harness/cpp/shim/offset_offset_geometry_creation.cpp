// SPDX-License-Identifier: Apache-2.0
// difftest shim for unit `offset.offset_geometry_creation`: SmOffsetGeometryCreation and
// SmLocalOperationGeometryCreation (source/SMLib/src/SmOffsetGeometryCreation.cpp) on two
// real SmBreps (the "original" and the "offset" B-rep of an SmOffsetExecutive).
//
// One entry point, sc_offset_offset_geometry_creation_eval(op, path, in, n_in, out, cap,
// out_len). Layouts are documented in
// crates/solidcore-ffi/src/units/offset_offset_geometry_creation.rs, which validates every
// input. The B-rep construction, traversal and export are those of
// harness/cpp/shim/sc_topology_fixture.h (two B-rep blocks, as the
// `boolean.topology_intersector_p1` shim).
//
// A RUN call makes an SmOffsetGeometryCreation (and, with a face/surface list, an
// SmLocalOperationGeometryCreation), an SmOffsetExecutive over the first B-rep whose
// flags, shell faces and original->offset edge-use relation (`m_vEUToEU`, a protected
// member reached through a derived class, filled with the pairs given) come from the
// input, locks a mark, and runs a script: fixture edits (store curves/surfaces, replace an
// edge's curve, set an edge use's UV trim curve, mark an edge use) and the unit's
// functions. Each op emits its code, a status (0 success, the SmStatus code, -2 when
// the shim refused the op because upstream would dereference null, or -6 when the script
// stops at an op nothing is compared at, see ST_NOT_RUN) and its outputs; the
// objects the unit makes are emitted as geometry (`put_curve` of the fixture header,
// surfaces as their type, UV domain, a 3x3 point sample and, for a plain B-spline surface
// or a surface of revolution, their NURB), the B-rep edits as edge / edge-use state blocks.
//
// Before every op the executive's extended-surface cache (`m_vSurfExtSurf`) is seeded
// with every face surface of both B-reps and every store surface mapped to itself, so
// `SmOffsetExecutive::GetCreateExtendedSurface` returns the base surface unextended and
// `CreateExtendedSurface` never runs here (its loops do not return for a negative offset
// distance, HANG-16, nor for a closed surface short against the extension, HANG-17). The
// Rust runner seeds the ported executive's cache the same way (`insert_extended_surface`).
#include "sc_topology_fixture.h"

#include <SmCylinder.h>
#include <SmDatabaseIO.h>
#include <SmOffsetExecutive.h>
#include <SmOffsetGeometryCreation.h>
#include <SmRelation.h>
#include <SmSSIData.h>
#include <SmSurfOfRevolution.h>
#include <SmSurfaceCache.h>
#include <SmTorus.h>

#include <cstdlib>
#include <sstream>
#include <stdexcept>
#include <string>
#include <array>

namespace
{
// ---- private member access (pointer-to-member through explicit instantiation) -------------

// The executive with its protected relation and extended-surface cache reachable.
struct ExecAccess : SmOffsetExecutive
{
    using SmOffsetExecutive::SmOffsetExecutive;
    void relate(SmEdgeuse* a, SmEdgeuse* b) { m_vEUToEU.RelatePair(a, b); }
    void seed_extended(SmSurface* s)
    {
        if (s != nullptr)
            m_vSurfExtSurf.Insert(s, s);
    }
};

enum OgOp : int32_t
{
    OG_EXPORT = 0,
    OG_RUN = 1,
};

// Script op codes.
enum OgScriptOp : int64_t
{
    S_ADD_CURVE = 1,
    S_ADD_SURFACE = 2,
    S_SET_EDGE_CURVE = 3,
    S_SET_EU_UV = 4,
    S_MARK = 5,
    S_GETTERS = 6,
    S_FACEUSE_OFFSET = 7,
    S_VERTEX_OFFSET = 8,
    S_UPDATE_FACE_DOMAIN = 9,
    S_REPLACE_EDGE_CURVES = 10,
    S_TRIM_EXTENDED_EDGE = 11,
    S_EXTEND_TRIM_EDGES = 12,
    S_MATE_EDGES = 13,
    S_HAVE_TANGENT_GAPS = 14,
    S_EDGEUSE_OFFSET = 15,
    S_LOCAL_FACEUSE_OFFSET = 16,
    S_EDGE_STATE = 17,
    S_SET_EDGE_TOL = 18,
};

// Handle kinds (`kind` of a `brep kind index` triple).
enum HKind : int64_t
{
    H_VERTEX = 0,
    H_EDGE = 1,
    H_FACE = 2,
    H_EDGE_CURVE = 3,
    H_FACE_SURFACE = 4,
    H_CURVE = 5, // the script's curve store
    H_BREP = 6,
    H_SURFACE = 7, // the script's surface store
    H_NULL = 8,
    H_EDGEUSE = 9,
    H_FACEUSE = 10,
    H_EU_UV = 11, // the UV trim curve stored on an edge use
};

struct Two
{
    SmBrep* brep[2] = {nullptr, nullptr};
    Graph g[2];
    std::vector<SmBSplineCurve*> curves;
    std::vector<SmSurface*> surfaces;

    SmObject* resolve(int64_t b, int64_t kind, int64_t i) const
    {
        if (kind == H_NULL || i < 0)
            return nullptr;
        if (kind == H_BREP)
            return (b == 0 || b == 1) ? (SmObject*)brep[b] : nullptr;
        if (kind == H_CURVE)
            return (i < (int64_t)curves.size()) ? (SmObject*)curves[(size_t)i] : nullptr;
        if (kind == H_SURFACE)
            return (i < (int64_t)surfaces.size()) ? (SmObject*)surfaces[(size_t)i] : nullptr;
        if (b != 0 && b != 1)
            return nullptr;
        const Graph& gr = g[b];
        auto at = [i](auto& v) -> SmTopology* { return (i >= 0 && i < (int64_t)v.size()) ? (SmTopology*)v[(size_t)i] : nullptr; };
        switch (kind)
        {
        case H_VERTEX: return at(gr.vertices);
        case H_EDGE: return at(gr.edges);
        case H_FACE: return at(gr.faces);
        case H_EDGEUSE: return at(gr.edgeuses);
        case H_FACEUSE: return at(gr.faceuses);
        case H_EDGE_CURVE:
        {
            SmEdge* e = (SmEdge*)at(gr.edges);
            return e ? (SmObject*)e->GetCurve() : nullptr;
        }
        case H_FACE_SURFACE:
        {
            SmFace* f = (SmFace*)at(gr.faces);
            return f ? (SmObject*)f->GetSurface() : nullptr;
        }
        case H_EU_UV:
        {
            SmEdgeuse* eu = (SmEdgeuse*)at(gr.edgeuses);
            return eu ? (SmObject*)eu->GetUVTrimCurvePointer() : nullptr;
        }
        default: return nullptr;
        }
    }
};

const double ST_OK = 0.0;
// The database version of the exported concrete surface records (OG_EXPORT).
const ULONG SURFACE_DB_VERSION = 33;
// An op nothing comparable runs at, so the script stops here (the runner mirrors each
// predicate): a non-local FaceuseOffset whose concrete kind is not exported (the Rust
// side defers), or an SmSurfOfRevolution with self-intersection enabled (20 s to
// minutes, HANG-26), and an EdgeuseOffset with |offset distance| above
// MAX_EDGEUSE_OFFSET_DISTANCE (HANG-27). The runner rejects the hang cases from the
// Rust script before calling the shim; this stop is a safety net.
const double ST_NOT_RUN = -6.0;
// HANG-27: SmCrvOnSurf::ApproximateCurve of an EdgeuseOffset rail grows with the
// offset distance (Mouse.smb: 1 s at 1e4, 10 s at 1e6, 28 s at 1e8, no return within
// 90 s at 1e250 and 1e300); the bound keeps every script under the fuzz's 20 s.
const double MAX_EDGEUSE_OFFSET_DISTANCE = 1.0e6;

bool has_surface_record(long type)
{
    return type == SmSurfOfRevolution_TYPE || type == SmSurfOfExtrusion_TYPE ||
           type == SmCone_TYPE || type == SmCylinder_TYPE || type == SmSphere_TYPE ||
           type == SmTorus_TYPE;
}

bool eu_ok(SmEdgeuse* eu)
{
    if (eu == nullptr || eu->GetEdge() == nullptr || eu->GetEdge()->GetCurve() == nullptr)
        return false;
    if (eu->GetVertexuse() == nullptr || eu->GetVertexuse()->GetVertex() == nullptr)
        return false;
    if (eu->GetMate() == nullptr || !eu->IsLoopEdgeuse())
        return false;
    if (eu->GetCWEdgeuse() == nullptr || eu->GetCCWEdgeuse() == nullptr)
        return false;
    if (eu->GetFaceuse() == nullptr || eu->GetFace() == nullptr || eu->GetFace()->GetSurface() == nullptr)
        return false;
    return true;
}

// Whether every edge use of the B-rep is a loop edge use with edge (with curve), vertex,
// mate, CW/CCW neighbours and a face with surface, and every edge has a vertex: what the
// topology-walking functions dereference without checking.
bool brep_ok(SmBrep* brep, Graph& g)
{
    if (brep == nullptr)
        return false;
    for (SmEdgeuse* eu : g.edgeuses)
        if (!eu_ok(eu))
            return false;
    for (SmEdge* e : g.edges)
        if (e->GetVertex() == nullptr || e->GetCurve() == nullptr)
            return false;
    return true;
}

SmObject* rd_h(Rd& rd, const Two& t)
{
    int64_t b = rd.idx();
    int64_t kind = rd.idx();
    int64_t i = rd.idx();
    return t.resolve(b, kind, i);
}

void put_srf(std::vector<double>& o, const SmSurface* s)
{
    if (s == nullptr)
    {
        o.push_back(-1.0);
        return;
    }
    o.push_back((double)s->GetType());
    SmExtent2d d = s->GetNaturalUVDomain();
    o.push_back(d.GetMin().x);
    o.push_back(d.GetMax().x);
    o.push_back(d.GetMin().y);
    o.push_back(d.GetMax().y);
    for (int i = 0; i <= 2; i++)
        for (int j = 0; j <= 2; j++)
        {
            SmPoint2d uv(d.GetMin().x + (d.GetMax().x - d.GetMin().x) * ((double)i / 2.0),
                         d.GetMin().y + (d.GetMax().y - d.GetMin().y) * ((double)j / 2.0));
            SmPoint3d p;
            SmStatus st_e = s->EvaluatePoint(uv, p);
            if (st_e == SM_SUCCESS)
            {
                o.push_back(0.0);
                put_vec(o, p);
            }
            else
                o.push_back(st(st_e));
        }
    // the NURB of a plain B-spline surface or a surface of revolution (the kinds this
    // unit builds whose sample does not pin degree, knots or net): `has_nurb [n m p q r s
    // net (x y z w)* u_knots* v_knots*]`
    long ty = s->GetType();
    if (ty == SmBSplineSurface_TYPE || ty == SmSurfOfRevolution_TYPE)
    {
        const gw_SURFACE* sur = ((const SmBSplineSurface*)s)->GetGwNurbPointer();
        o.push_back(sur != nullptr ? 1.0 : 0.0);
        if (sur == nullptr)
            return;
        gw_INDEX n = sur->net->n;
        gw_INDEX m = sur->net->m;
        gw_INDEX r = sur->knu->m;
        gw_INDEX ss = sur->knv->m;
        o.push_back((double)n);
        o.push_back((double)m);
        o.push_back((double)sur->p);
        o.push_back((double)sur->q);
        o.push_back((double)r);
        o.push_back((double)ss);
        for (gw_INDEX i = 0; i <= n; i++)
            for (gw_INDEX j = 0; j <= m; j++)
            {
                o.push_back(sur->net->Pw[i][j].x);
                o.push_back(sur->net->Pw[i][j].y);
                o.push_back(sur->net->Pw[i][j].z);
                o.push_back(sur->net->Pw[i][j].w);
            }
        for (gw_INDEX i = 0; i <= r; i++)
            o.push_back(sur->knu->U[i]);
        for (gw_INDEX i = 0; i <= ss; i++)
            o.push_back(sur->knv->U[i]);
    }
}

void put_orients(std::vector<double>& o, const SmTArray<SmOrientType>& v)
{
    o.push_back((double)v.GetSize());
    for (ULONG k = 0; k < v.GetSize(); k++)
        o.push_back((double)v[k]);
}

void put_curves(std::vector<double>& o, const SmTArray<SmCurve*>& v)
{
    o.push_back((double)v.GetSize());
    for (ULONG k = 0; k < v.GetSize(); k++)
        put_curve(o, v[k]);
}

// `curve t0 t1 tolerance vertex_point other_vertex_point n_edgeuses uv_trim_curve*`.
void put_edge_state(std::vector<double>& o, SmEdge* e)
{
    put_curve(o, e->GetCurve());
    SmExtent1d ivl = e->GetInterval();
    o.push_back(ivl.GetMin());
    o.push_back(ivl.GetMax());
    o.push_back((double)e->GetTolerance());
    SmVertex* v = e->GetVertex();
    if (v)
    {
        o.push_back(1.0);
        put_vec(o, v->GetPoint());
        SmVertex* w = e->GetOtherVertex(v);
        if (w)
        {
            o.push_back(1.0);
            put_vec(o, w->GetPoint());
        }
        else
            o.push_back(0.0);
    }
    else
        o.push_back(0.0);
    SM_PTR_ARRAY(eus, SmEdgeuse, 16);
    e->GetEdgeuses(eus);
    o.push_back((double)eus.GetSize());
    for (ULONG k = 0; k < eus.GetSize(); k++)
        put_curve(o, eus[k]->GetUVTrimCurve());
}

template <class T> void delete_curves(SmTArray<T*>& v)
{
    for (ULONG k = 0; k < v.GetSize(); k++)
        delete v[k];
    v.ReSet();
}

// Whether `ReplaceEdgeCurves(eu, c3, NULL, orig)` would reverse the curves (and dereference
// the null UV curve): the shape test of the function, replicated.
bool would_reverse(SmEdgeuse* eu, SmCurve* c3, SmEdgeuse* orig)
{
    SmEdge* edge = eu->GetEdge();
    SmCurve* euc = edge->GetCurve();
    SmExtent1d ivl = edge->GetInterval();
    double tol = edge->GetTolerance();
    SmPoint3d es, ee, cs, ce;
    euc->EvaluatePoint(ivl.GetMin(), es);
    euc->EvaluatePoint(ivl.GetMax(), ee);
    SmExtent1d civl = c3->GetNaturalInterval();
    c3->EvaluatePoint(civl.GetMin(), cs);
    c3->EvaluatePoint(civl.GetMax(), ce);
    if (es.DistanceBetween(cs) < tol && ee.DistanceBetween(ce) < tol)
        return false;
    if (es.DistanceBetween(ce) < tol && ee.DistanceBetween(cs) < tol)
        return false;
    SmVector3d pv[2], pv2[2];
    if (orig == nullptr)
        euc->Evaluate(ivl.Evaluate(0.5), 1, TRUE, pv);
    else
    {
        SmEdge* oe = orig->GetEdge();
        oe->GetCurve()->Evaluate(oe->GetInterval().Evaluate(0.5), 1, TRUE, pv);
    }
    c3->Evaluate(civl.Evaluate(0.5), 1, TRUE, pv2);
    return pv[1].Dot(pv2[1]) < 0.0;
}

// Unit-local block 6 constructs the concrete classes explicitly. Legacy files
// named cylinder/torus can instead contain SmCone/SmSurfOfRevolution records.
SmBrep* build_input(Rd& rd, const char* path, SmStatus& status)
{
    Rd peek = rd;
    if (peek.idx() != 6)
        return build(rd, path, status);
    rd = peek;
    int64_t kind = rd.idx();
    double a = rd.num(), b = rd.num();
    const SmContext& ctx = the_context();
    SmPoint3d origin(0.0, 0.0, 0.0);
    SmVector3d x(1.0, 0.0, 0.0), y(0.0, 1.0, 0.0);
    SmBSplineSurface* surface = kind == 0
        ? static_cast<SmBSplineSurface*>(new (ctx) SmCylinder(origin, x, y, a, 0.0, 360.0, b,
                                                             FALSE, FALSE, FALSE, &ctx))
        : static_cast<SmBSplineSurface*>(new (ctx) SmTorus(origin, x, y,
                                                         SmExtent2d(0.0, 0.0, 360.0, 360.0),
                                                         a, b, FALSE, FALSE, nullptr, &ctx));
    status = surface->MakeNurb();
    if (status != SM_SUCCESS)
    {
        delete surface;
        return nullptr;
    }
    SmBrep* brep = new (ctx) SmBrep();
    brep->m_bValidateOnly = FALSE;
    SmFace* face = nullptr;
    status = brep->CreateFaceFromSurface(surface, surface->GetNaturalUVDomain(), face);
    if (status != SM_SUCCESS)
    {
        delete brep;
        return nullptr;
    }
    return brep;
}

// Reads the protected decomposition tree of a surface cache without building it
// (`GetTree` would build it).
struct CacheTreePeek : SmSurfaceCache
{
    static bool built(const SmSurfaceCache* c) { return c->*(&CacheTreePeek::m_pTree) != nullptr; }
};

// The state of a surface's cache slot (`m_pCacheObj`, a trimmed surface cache or a
// surface cache): 0 none, 1 a cache whose tree is not built (made during a Boolean
// operation: its continuity arrays are empty), 2 a cache whose tree is built. Reading
// files builds B-reps while doing a Boolean operation, so their face surfaces can keep
// caches without trees into the script; the Rust import restores this state.
double cache_tree_state(const SmSurface* s)
{
    if (s == nullptr || s->GetCacheObj() == nullptr)
        return 0.0;
    return CacheTreePeek::built(static_cast<const SmSurfaceCache*>(s->GetCacheObj())) ? 2.0 : 1.0;
}

bool build_two(Rd& rd, const char* path, Two& t, std::vector<double>& o)
{
    std::string p(path);
    std::string pa = p, pb;
    size_t nl = p.find('\n');
    if (nl != std::string::npos)
    {
        pa = p.substr(0, nl);
        pb = p.substr(nl + 1);
    }
    SmStatus st_a;
    t.brep[0] = build_input(rd, pa.c_str(), st_a);
    if (t.brep[0] == nullptr)
    {
        o.push_back(-1.0);
        o.push_back(st(st_a));
        return false;
    }
    bool has_b = rd.flag();
    if (has_b)
    {
        SmStatus st_b;
        t.brep[1] = build_input(rd, pb.c_str(), st_b);
        if (t.brep[1] == nullptr)
        {
            o.push_back(-3.0);
            o.push_back(st(st_b));
            return false;
        }
    }
    walk(t.brep[0], t.g[0]);
    if (t.brep[1])
        walk(t.brep[1], t.g[1]);
    return true;
}

void run_script(Rd& rd, Two& t, std::vector<double>& o)
{
    const SmContext& ctx = the_context();
    double dist = rd.num();
    double approx = rd.num();
    bool make_analytics = rd.flag();
    SmOffsetGeometryCreation geom(dist, approx);
    geom.SetMakeAnalytics(make_analytics ? TRUE : FALSE);

    // the local-operation creation (its lists are the face/surface pairs given)
    // The listed surfaces are store surfaces (the FFI refuses a face's own: the local
    // creation owns them), named by handle; the store is filled by the script's ops, so
    // the handles are resolved again before each local op (upstream copies the lists at
    // construction; the local creation is rebuilt then). Its m_bMakeAnalytics stays
    // FALSE, as the constructor leaves it (SetMakeAnalytics is never called on it), so
    // FaceuseOffset returns the listed surface as is.
    int64_t n_local = rd.idx();
    SmTArray<SmFace*> local_faces(ctx);
    SmTArray<SmSurface*> local_surfaces(ctx);
    std::vector<std::array<int64_t, 3>> local_srf;
    for (int64_t k = 0; k < n_local; k++)
    {
        SmFace* f = (SmFace*)rd_h(rd, t);
        std::array<int64_t, 3> h;
        h[0] = rd.idx();
        h[1] = rd.idx();
        h[2] = rd.idx();
        local_faces.Add(f);
        local_surfaces.Add((SmSurface*)t.resolve(h[0], h[1], h[2]));
        local_srf.push_back(h);
    }
    SmLocalOperationGeometryCreation local(dist, approx, local_faces, local_surfaces);

    // the executive
    ExecAccess exec(ctx, SM_OO_SOLID_OFFSET, geom, t.brep[0]);
    exec.SetInset(rd.bflag());
    exec.SetExtendConvexEdges(rd.bflag());
    int64_t n_shell = rd.idx();
    if (n_shell > 0)
    {
        SmTArray<SmFace*> shell(ctx);
        for (int64_t k = 0; k < n_shell; k++)
            shell.Add((SmFace*)rd_h(rd, t));
        exec.SetShellFaces(shell);
    }
    int64_t n_pairs = rd.idx();
    for (int64_t k = 0; k < n_pairs; k++)
    {
        SmEdgeuse* a = (SmEdgeuse*)rd_h(rd, t);
        SmEdgeuse* b = (SmEdgeuse*)rd_h(rd, t);
        exec.relate(a, b);
    }

    // the mark the trim uses
    SmNewMarkAndLock mark_lock(&ctx);
    SmMarkType mark = mark_lock.GetMarkType();

    bool ok[2] = {brep_ok(t.brep[0], t.g[0]), brep_ok(t.brep[1], t.g[1])};
    bool all_ok = ok[0] && (t.brep[1] == nullptr || ok[1]);

    int64_t n_ops = rd.idx();
    for (int64_t k = 0; k < n_ops; k++)
    {
        int64_t op = rd.idx();
        o.push_back((double)op);
        // seed the extended-surface cache with the surfaces as they stand now (see the
        // header): every face surface of both B-reps and every store surface
        for (int b = 0; b < 2; b++)
        {
            if (t.brep[b] == nullptr)
                continue;
            for (size_t q = 0; q < t.g[b].faces.size(); q++)
                if (t.g[b].faces[q] != nullptr)
                    exec.seed_extended(t.g[b].faces[q]->GetSurface());
        }
        for (size_t q = 0; q < t.surfaces.size(); q++)
            exec.seed_extended(t.surfaces[q]);
        switch (op)
        {
        case S_ADD_CURVE:
        {
            SmBSplineCurve* c = read_curve(rd, ctx);
            t.curves.push_back(c);
            o.push_back(ST_OK);
            o.push_back((double)(t.curves.size() - 1));
            break;
        }
        case S_ADD_SURFACE:
        {
            SmSurface* s = read_surface(rd, ctx);
            t.surfaces.push_back(s);
            o.push_back(ST_OK);
            o.push_back((double)(t.surfaces.size() - 1));
            break;
        }
        case S_SET_EDGE_CURVE:
        {
            SmEdge* e = (SmEdge*)rd_h(rd, t);
            SmBSplineCurve* c = (SmBSplineCurve*)rd_h(rd, t);
            if (e == nullptr || c == nullptr)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            SmBSplineCurve* copy = new (ctx) SmBSplineCurve(*c);
            e->SetCurve(copy, TRUE);
            copy->SetOwner(e);
            e->SetInterval(copy->GetNaturalInterval());
            o.push_back(ST_OK);
            put_edge_state(o, e);
            break;
        }
        case S_SET_EU_UV:
        {
            SmEdgeuse* eu = (SmEdgeuse*)rd_h(rd, t);
            SmBSplineCurve* c = (SmBSplineCurve*)rd_h(rd, t);
            double max_dist = rd.num();
            if (eu == nullptr)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            SmBSplineCurve* copy = c ? new (ctx) SmBSplineCurve(*c) : nullptr;
            eu->SetUVTrimCurve(copy, max_dist, TRUE);
            o.push_back(ST_OK);
            put_curve(o, eu->GetUVTrimCurve());
            break;
        }
        case S_MARK:
        {
            SmEdgeuse* eu = (SmEdgeuse*)rd_h(rd, t);
            if (eu == nullptr)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            eu->Mark(mark);
            o.push_back(ST_OK);
            o.push_back(flg(eu->IsMarked(mark)));
            break;
        }
        case S_GETTERS:
        {
            o.push_back(ST_OK);
            o.push_back(geom.GetThisApproxTol3d());
            o.push_back(geom.GetTangencyTolRadians());
            o.push_back(geom.GetTangencyTolDegrees());
            o.push_back(geom.GetOffsetDistance());
            o.push_back(local.GetThisApproxTol3d());
            o.push_back(local.GetTangencyTolDegrees());
            o.push_back(local.GetOffsetDistance());
            o.push_back(exec.GetTangencyTolDegrees());
            o.push_back(flg(exec.GetInset()));
            o.push_back(flg(exec.GetExtendConvexEdges()));
            SmTArray<SmFace*> sh(ctx);
            exec.GetShellFaces(sh);
            o.push_back((double)sh.GetSize());
            break;
        }
        case S_FACEUSE_OFFSET:
        case S_LOCAL_FACEUSE_OFFSET:
        {
            SmFaceuse* fu = (SmFaceuse*)rd_h(rd, t);
            bool skip = rd.flag();
            if (fu == nullptr || fu->GetFace() == nullptr || fu->GetFace()->GetSurface() == nullptr)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            if (op == S_FACEUSE_OFFSET)
            {
                long ty = fu->GetFace()->GetSurface()->GetType();
                if ((ty != SmPlane_TYPE && ty != SmBSplineSurface_TYPE && !has_surface_record(ty)) ||
                    (ty == SmSurfOfRevolution_TYPE && !skip))
                {
                    o.push_back(ST_NOT_RUN); // unsupported import or HANG-26, not a parity pass
                    return;
                }
            }
            ULONG lidx = 0;
            bool lfound = false;
            if (op == S_LOCAL_FACEUSE_OFFSET)
            {
                // the listed surfaces, as the store stands now
                for (size_t k = 0; k < local_srf.size(); k++)
                    local_surfaces[(ULONG)k] = (SmSurface*)t.resolve(local_srf[k][0], local_srf[k][1], local_srf[k][2]);
                // a listed face without surface (not added yet, or deleted) is dereferenced
                // upstream
                lfound = local_faces.FindElement(fu->GetFace(), lidx) ? true : false;
                if (lfound && local_surfaces[lidx] == nullptr)
                {
                    o.push_back(ST_NULL_DEREF);
                    break;
                }
            }
            SmLocalOperationGeometryCreation local_now(dist, approx, local_faces, local_surfaces);
            SmSurface* ns = nullptr;
            SmSSIData ssi;
            SmStatus s = (op == S_LOCAL_FACEUSE_OFFSET)
                             ? local_now.FaceuseOffset(ctx, fu, skip ? TRUE : FALSE, ns, ssi)
                             : geom.FaceuseOffset(ctx, fu, skip ? TRUE : FALSE, ns, ssi);
            o.push_back(st(s));
            if (s == SM_SUCCESS) // the out-parameters: only after success
            {
                o.push_back(ns ? 1.0 : 0.0);
                if (ns)
                    put_srf(o, ns);
                o.push_back((double)ssi.m_v3DCurves.GetSize());
                for (ULONG j = 0; j < ssi.m_v3DCurves.GetSize(); j++)
                {
                    put_curve(o, ssi.m_v3DCurves[j]);
                    put_curve(o, j < ssi.m_vUVCurves1.GetSize() ? ssi.m_vUVCurves1[j] : nullptr);
                    put_curve(o, j < ssi.m_vUVCurves2.GetSize() ? ssi.m_vUVCurves2[j] : nullptr);
                    o.push_back(j < ssi.m_vCurveTypes.GetSize() ? (double)ssi.m_vCurveTypes[j] : -1.0);
                    o.push_back(j < ssi.m_vDeviations.GetSize() ? ssi.m_vDeviations[j] : -1.0);
                }
            }
            if (op == S_LOCAL_FACEUSE_OFFSET && ns != nullptr)
            {
                // the listed store surface, returned as is (owned by the store already)
                bool listed = false;
                for (size_t q = 0; q < t.surfaces.size(); q++)
                    if (t.surfaces[q] == ns)
                        listed = true;
                if (!listed)
                    t.surfaces.push_back(ns);
            }
            else
                delete ns;
            delete_curves(ssi.m_v3DCurves);
            delete_curves(ssi.m_vUVCurves1);
            delete_curves(ssi.m_vUVCurves2);
            break;
        }
        case S_VERTEX_OFFSET:
        {
            SmVertex* v = (SmVertex*)rd_h(rd, t);
            int64_t n = rd.idx();
            SmTArray<SmEdge*> edges(ctx);
            bool bad = false;
            for (int64_t j = 0; j < n; j++)
            {
                SmEdge* e = (SmEdge*)rd_h(rd, t);
                if (e == nullptr || e->GetCurve() == nullptr)
                    bad = true;
                edges.Add(e);
            }
            if (!bad && n == 1)
            {
                SM_PTR_ARRAY(faces, SmFace, 8);
                edges[0]->GetFaces(faces);
                if (faces.GetSize() == 0 || faces[0]->GetSurface() == nullptr ||
                    !edges[0]->GetCurve()->IsKindOf(SmBSplineCurve_TYPE))
                    bad = true;
            }
            // no all_ok: VertexOffset walks no loops (IsLaminaVertex is GetEdges + IsLamina),
            // so wire and other non-loop B-reps are compared; the per-edge checks stay
            if (bad)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            SmTArray<SmCurve*> curves(ctx);
            SmTArray<SmOrientType> orients(ctx);
            SmSurface* ns = nullptr;
            SmStatus s = geom.VertexOffset(ctx, v, edges, curves, orients, ns);
            o.push_back(st(s));
            if (s == SM_SUCCESS) // the out-parameters: only after success
            {
                put_curves(o, curves);
                put_orients(o, orients);
                o.push_back(ns ? 1.0 : 0.0);
                if (ns)
                    put_srf(o, ns);
            }
            delete_curves(curves);
            delete ns;
            break;
        }
        case S_UPDATE_FACE_DOMAIN:
        {
            SmFace* f = (SmFace*)rd_h(rd, t);
            SmSurface* ext = (SmSurface*)rd_h(rd, t);
            if (f == nullptr || f->GetSurface() == nullptr || ext == nullptr)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            SmStatus s = geom.UpdateFaceSurfaceDomain(f, ext);
            o.push_back(st(s));
            SmExtent2d d = f->GetUVDomain();
            o.push_back(d.GetMin().x);
            o.push_back(d.GetMax().x);
            o.push_back(d.GetMin().y);
            o.push_back(d.GetMax().y);
            put_srf(o, f->GetSurface());
            break;
        }
        case S_REPLACE_EDGE_CURVES:
        {
            SmEdgeuse* eu = (SmEdgeuse*)rd_h(rd, t);
            SmBSplineCurve* c3 = (SmBSplineCurve*)rd_h(rd, t);
            SmBSplineCurve* uv = (SmBSplineCurve*)rd_h(rd, t);
            SmEdgeuse* orig = (SmEdgeuse*)rd_h(rd, t);
            bool bad = eu == nullptr || eu->GetEdge() == nullptr;
            if (!bad && c3 != nullptr)
            {
                SmEdge* e = eu->GetEdge();
                bad = e->GetCurve() == nullptr || eu->GetVertexuse() == nullptr ||
                      eu->GetVertexuse()->GetVertex() == nullptr || eu->GetMate() == nullptr ||
                      eu->GetMate()->GetVertexuse() == nullptr || eu->GetMate()->GetVertexuse()->GetVertex() == nullptr;
                if (!bad && orig != nullptr)
                    bad = orig->GetEdge() == nullptr || orig->GetEdge()->GetCurve() == nullptr;
                if (!bad && uv == nullptr && would_reverse(eu, c3, orig))
                    bad = true;
            }
            if (bad)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            SmCurve* c3c = c3 ? new (ctx) SmBSplineCurve(*c3) : nullptr;
            SmBSplineCurve* uvc = uv ? new (ctx) SmBSplineCurve(*uv) : nullptr;
            SmBoolean del3 = FALSE, deluv = FALSE;
            SmStatus s = geom.ReplaceEdgeCurves(eu, c3c, uvc, orig, del3, deluv);
            o.push_back(st(s));
            if (s == SM_SUCCESS) // the out-parameters: only after success
            {
                o.push_back(flg(del3));
                o.push_back(flg(deluv));
            }
            put_edge_state(o, eu->GetEdge()); // the edge's state: after every status
            if (del3)
                delete c3c;
            if (deluv)
                delete uvc;
            break;
        }
        case S_TRIM_EXTENDED_EDGE:
        {
            SmEdgeuse* base = (SmEdgeuse*)rd_h(rd, t);
            SmEdgeuse* other_base = (SmEdgeuse*)rd_h(rd, t);
            SmEdgeuse* off = (SmEdgeuse*)rd_h(rd, t);
            SmEdgeuse* other_off = (SmEdgeuse*)rd_h(rd, t);
            SmCurve* c3 = (SmCurve*)rd_h(rd, t);
            SmBSplineCurve* uv_base = (SmBSplineCurve*)rd_h(rd, t);
            SmBSplineCurve* uv_other = (SmBSplineCurve*)rd_h(rd, t);
            if (!all_ok || base == nullptr || c3 == nullptr)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            SmBoolean good = FALSE;
            SmExtent1d ivl;
            SmStatus s = geom.TrimExtendedEdge(ctx, &exec, base, other_base, off, other_off, c3, uv_base, uv_other, good,
                                               ivl, mark);
            o.push_back(st(s));
            if (s == SM_SUCCESS) // the out-parameters: only after success
            {
                o.push_back(flg(good));
                if (good)
                {
                    o.push_back(ivl.GetMin());
                    o.push_back(ivl.GetMax());
                }
            }
            break;
        }
        case S_EXTEND_TRIM_EDGES:
        {
            SmEdgeuse* eu = (SmEdgeuse*)rd_h(rd, t);
            SmEdgeuse* other = (SmEdgeuse*)rd_h(rd, t);
            SmEdgeuse* off = (SmEdgeuse*)rd_h(rd, t);
            SmEdgeuse* other_off = (SmEdgeuse*)rd_h(rd, t);
            bool bad = !all_ok || eu == nullptr || off == nullptr || eu->GetUVTrimCurve() == nullptr;
            if (!bad && !eu->GetEdge()->IsLamina() && eu->GetRadial() != nullptr &&
                eu->GetRadial()->GetUVTrimCurve() == nullptr)
                bad = true;
            if (!bad && !eu->GetEdge()->IsLamina() && eu->GetRadial() == nullptr)
                bad = true;
            if (bad)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            SmStatus s = geom.ExtendTrimEdges(ctx, &exec, eu, other, off, other_off, mark);
            o.push_back(st(s));
            put_edge_state(o, off->GetEdge());
            o.push_back(other_off ? 1.0 : 0.0);
            if (other_off)
                put_edge_state(o, other_off->GetEdge());
            break;
        }
        case S_MATE_EDGES:
        {
            SmEdgeuse* eu = (SmEdgeuse*)rd_h(rd, t);
            SmEdgeuse* other = (SmEdgeuse*)rd_h(rd, t);
            SmEdgeuse* off = (SmEdgeuse*)rd_h(rd, t);
            SmEdgeuse* other_off = (SmEdgeuse*)rd_h(rd, t);
            if (!all_ok)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            SmStatus s = geom.MateEdges(ctx, &exec, eu, other, off, other_off, mark);
            o.push_back(st(s));
            o.push_back(off ? 1.0 : 0.0);
            if (off)
                put_edge_state(o, off->GetEdge());
            o.push_back(other_off ? 1.0 : 0.0);
            if (other_off)
                put_edge_state(o, other_off->GetEdge());
            break;
        }
        case S_HAVE_TANGENT_GAPS:
        {
            int64_t n = rd.idx();
            SmEdgeuse* off = (SmEdgeuse*)rd_h(rd, t);
            SmEdgeuse* other_off = (SmEdgeuse*)rd_h(rd, t);
            if (!all_ok || off == nullptr || other_off == nullptr)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            o.push_back(ST_OK);
            o.push_back(flg(geom.HaveTangentGaps((ULONG)n, off, other_off)));
            break;
        }
        case S_EDGEUSE_OFFSET:
        {
            SmEdgeuse* orig = (SmEdgeuse*)rd_h(rd, t);
            SmEdgeuse* other_orig = (SmEdgeuse*)rd_h(rd, t);
            SmEdgeuse* off = (SmEdgeuse*)rd_h(rd, t);
            SmEdgeuse* other_off = (SmEdgeuse*)rd_h(rd, t);
            if (!all_ok || orig == nullptr)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            if (fabs(dist) > MAX_EDGEUSE_OFFSET_DISTANCE)
            {
                o.push_back(ST_NOT_RUN); // HANG-27: the runner rejects such a script
                return;
            }
            if (exec.GetExtendConvexEdges())
            {
                // a tangent or lamina edge goes to ExtendTrimEdges, which dereferences the
                // offset edge use (null is refused) and its UV trim curves
                bool lamina = orig->GetEdge()->IsLamina();
                bool tangent = orig->IsTangentSector(geom.GetTangencyTolDegrees());
                if (tangent || lamina)
                {
                    bool bad = off == nullptr || orig->GetUVTrimCurve() == nullptr;
                    if (!bad && !lamina && (orig->GetRadial() == nullptr || orig->GetRadial()->GetUVTrimCurve() == nullptr))
                        bad = true;
                    if (bad)
                    {
                        o.push_back(ST_NULL_DEREF);
                        break;
                    }
                }
            }
            SmSurface* ns = nullptr;
            SmTArray<SmCurve*> curves(ctx);
            SmTArray<SmOrientType> orients(ctx);
            SmSSIData ssi;
            SmStatus s = geom.EdgeuseOffset(ctx, &exec, orig, other_orig, off, other_off, ns, curves, orients, ssi, mark);
            o.push_back(st(s));
            if (s == SM_SUCCESS) // the out-parameters: only after success
            {
                o.push_back(ns ? 1.0 : 0.0);
                if (ns)
                    put_srf(o, ns);
                put_curves(o, curves);
                put_orients(o, orients);
                o.push_back((double)ssi.m_v3DCurves.GetSize());
            }
            o.push_back(off ? 1.0 : 0.0); // the edges' states: after every status
            if (off)
                put_edge_state(o, off->GetEdge());
            o.push_back(other_off ? 1.0 : 0.0);
            if (other_off)
                put_edge_state(o, other_off->GetEdge());
            delete_curves(curves);
            delete_curves(ssi.m_v3DCurves);
            delete_curves(ssi.m_vUVCurves1);
            delete_curves(ssi.m_vUVCurves2);
            delete ns;
            break;
        }
        case S_EDGE_STATE:
        {
            SmEdge* e = (SmEdge*)rd_h(rd, t);
            if (e == nullptr)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            o.push_back(ST_OK);
            put_edge_state(o, e);
            break;
        }
        case S_SET_EDGE_TOL:
        {
            SmEdge* e = (SmEdge*)rd_h(rd, t);
            double tol = rd.num();
            if (e == nullptr)
            {
                o.push_back(ST_NULL_DEREF);
                break;
            }
            e->SetTolerance(tol, FALSE, FALSE);
            o.push_back(ST_OK);
            o.push_back((double)e->GetTolerance());
            break;
        }
        default: o.push_back(-9.0); return;
        }
    }
}

void run_og(int32_t op, Rd& rd, const char* path, std::vector<double>& o)
{
    Two t;
    struct Clean
    {
        Two& t;
        ~Clean()
        {
            for (SmBSplineCurve* c : t.curves)
                delete c;
            for (SmSurface* s : t.surfaces)
                delete s;
            delete t.brep[0];
            delete t.brep[1];
        }
    } clean{t};
    if (!build_two(rd, path, t, o))
        return;
    o.push_back(0.0);
    if (op == OG_EXPORT)
    {
        for (int b = 0; b < 2; b++)
        {
            if (t.brep[b] == nullptr)
            {
                o.push_back(0.0);
                continue;
            }
            std::vector<double> ex;
            ex.push_back(0.0);
            export_graph(t.brep[b], t.g[b], ex);
            o.push_back((double)ex.size());
            o.insert(o.end(), ex.begin(), ex.end());
        }
        // Per B-rep, per face (export order): the binary WriteToDB record of a
        // supported concrete subclass. The graph's surface block carries only its
        // B-spline part; its type code selects the Rust polymorphic reader.
        for (int b = 0; b < 2; b++)
        {
            if (t.brep[b] == nullptr)
            {
                o.push_back(0.0);
                continue;
            }
            o.push_back((double)t.g[b].faces.size());
            for (SmFace* f : t.g[b].faces)
            {
                SmSurface* s = f->GetSurface();
                if (s == nullptr || !has_surface_record(s->GetType()))
                {
                    o.push_back(-1.0);
                    continue;
                }
                std::ostringstream oss;
                SmDatabaseIOStream db(SM_BINARY, &oss);
                if (s->WriteToDB(db, SURFACE_DB_VERSION) != SM_SUCCESS)
                    throw std::runtime_error("offset creator surface record export failed");
                std::string bytes = oss.str();
                o.push_back((double)bytes.size());
                for (char ch : bytes)
                    o.push_back((double)(unsigned char)ch);
            }
        }
        // Per B-rep, per face (export order): the state of the face surface's cache
        // after the build (`cache_tree_state`).
        for (int b = 0; b < 2; b++)
        {
            if (t.brep[b] == nullptr)
            {
                o.push_back(0.0);
                continue;
            }
            o.push_back((double)t.g[b].faces.size());
            for (SmFace* f : t.g[b].faces)
                o.push_back(cache_tree_state(f->GetSurface()));
        }
        return;
    }
    run_script(rd, t, o);
}
} // namespace

SC_EXPORT int32_t sc_offset_offset_geometry_creation_eval(int32_t op, const char* path, const double* in, int64_t n_in,
                                                          double* out, int64_t cap, int64_t* out_len)
{
    if (in == NULL || n_in < 1 || out_len == NULL || path == NULL)
        return SC_ERR_ARG;
    SC_TRY
    Rd rd{in, 0};
    std::vector<double> o;
    run_og(op, rd, path, o);
    return sc_copy_out(o.data(), (int64_t)o.size(), out, cap, out_len);
    SC_CATCH
}
