// SPDX-License-Identifier: Apache-2.0
// Native adapter for offset.offset_executive_p1 (PR #175).
// Upstream: source/SMLib/inc/SmOffsetExecutive.h, src/SmOffsetExecutive.cpp.
//
// Rust API/protocol: crates/solidcore-ffi/src/units/offset_offset_executive_p1.rs.
// config -> default/custom geometry ctor views, native executive ctor/dtor,
//           state setters/getters, SmRelation/GetOffsetEdgeuse.
// Config input begins distance, approx_tol, tangency_tol_radians, operation.
// Its constructor-state and final-state snapshots both use the supplied geometry;
// other modes retain the native default 2-degree tangency constructor behavior.
// synchronize -> SyncronizeOffsetEdgeuses, which calls all four owned static helpers.
// extend -> GetCreateExtendedSurface with real SmPlane/SmBSplineSurface geometry.
// Extension takes an internal nonnegative magnitude, not a public signed offset:
// SmOffsetExecutive.cpp:114-120,206-223 pass fabs(distance) to the geometry creator
// and preserve the sign via SetInset; SmBrep.cpp:12004 uses bounding-sphere radius/2.
// Reject negative extension distances before geometry construction as SC_ERR_ARG;
// both signed zeros remain valid. Config only observes state, so stays sign-agnostic.
// probe -> independent safe topology paths; not a replacement for arithmetic tests.
// face tail -> BuildOffsetOfFaceuse's contained-domain branch on a MakeFaceWithCurves
//              plane face, including MapEdgeuses and self-intersection merging.
// map twin -> MapEdgeuses alone, between two MakeFaceWithCurves rectangle faces.
// replace -> ReplaceImplicitOffsets on one CreateFaceFromSurface face whose surface is
//            an owning SmOffsetSurface(d, base) of a real SmPlane, SmBSplineSurface or
//            SmSurfOfExtrusion (lazy: no NURB until GetOrCreateGwNurbPointer).
// Deviations: bounded plain-C fixture protocol, traversal IDs instead of pointers;
// extension-only nonnegative-distance validation. Native array ordering is unchanged.
// No implementation file is included; no native numerical/helper code is copied.
#include "sc_shim.h"
#include "StdAfx.h"
#include <SmAxis2Placement.h>
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmContext.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmFace.h>
#include <SmFaceuse.h>
#include <SmLine.h>
#include <SmLoop.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmOffsetExecutive.h>
#include <SmOffsetGeometryCreation.h>
#include <SmOffsetSurface.h>
#include <SmPlane.h>
#include <SmRegion.h>
#include <SmShell.h>
#include <SmSurfOfExtrusion.h>
#include <SmSSIData.h>
#include <SmVertex.h>
#include <SmVertexuse.h>
#include <nurbs.h>
#include <algorithm>
#include <cmath>
#include <memory>
#include <new>
#include <vector>

namespace {
constexpr int64_t max_values = 1048576;
constexpr int64_t max_items = 4096;
constexpr int64_t max_points = 65536;
constexpr int64_t max_degree = 17; // NL_MAXDEG, not an arbitrary evaluation degree.
struct Argument {};
struct Rejected { SmStatus status; };

// A second bounds check here makes malformed direct ABI calls fail as arguments,
// not read beyond the caller's buffer. Geometric invariants are checked in Rust.
struct Reader {
    const double* data;
    int64_t size;
    int64_t pos = 0;
    double number() {
        if (pos >= size) throw Argument{};
        double v = data[pos++];
        if (!std::isfinite(v)) throw Argument{};
        return v;
    }
    int64_t index(int64_t limit = max_items) {
        double v = number();
        if (v < 0 || v > (double)limit || std::floor(v) != v) throw Argument{};
        return (int64_t)v;
    }
    bool flag() { return index(1) != 0; }
    SmPoint3d point() {
        double x = number(), y = number(), z = number();
        return SmPoint3d(x, y, z);
    }
    SmPoint2d uv() {
        double x = number(), y = number();
        return SmPoint2d(x, y);
    }
    SmExtent1d interval() {
        double a = number(), b = number();
        if (a > b || !std::isfinite(b-a)) throw Argument{};
        return SmExtent1d(a, b);
    }
    SmExtent2d domain() {
        SmPoint2d a = uv(), b = uv();
        if (!(a.x < b.x && a.y < b.y)) throw Argument{};
        return SmExtent2d(a, b);
    }
    void done() const { if (pos != size) throw Argument{}; }
};

void constructed(SmStatus status) {
    if (status != SM_SUCCESS) throw Rejected{status};
}
void put_point(std::vector<double>& out, const SmPoint3d& p) {
    out.insert(out.end(), {p.x, p.y, p.z});
}
void put_domain(std::vector<double>& out, const SmExtent2d& d) {
    out.insert(out.end(), {d.GetMin().x, d.GetMin().y, d.GetMax().x, d.GetMax().y});
}
template<class T> double id(const SmTArray<T*>& list, const T* p) {
    if (!p) return -1;
    for (ULONG i=0; i<list.GetSize(); ++i) if (list[i] == p) return (double)i;
    return -2;
}
void put_ids(std::vector<double>& out, const SmTArray<SmEdgeuse*>& values,
             const SmTArray<SmEdgeuse*>& traversal) {
    out.push_back(values.GetSize());
    for (ULONG i=0; i<values.GetSize(); ++i) out.push_back(id(traversal, values[i]));
}
std::unique_ptr<SmBrep> brep(SmContext& ctx) {
    return std::unique_ptr<SmBrep>(new (ctx) SmBrep(SM_USE_DEFAULT, SM_USE_DEFAULT, &ctx));
}

// Config-only geometry-subclass fixture. Preserve base initialization and the
// inherited GetTangencyTolDegrees calculation; only the protected scalar changes.
class ConfigGeometry : public SmOffsetGeometryCreation {
public:
    ConfigGeometry(double distance, double tol, double tangency_tol_radians)
        : SmOffsetGeometryCreation(distance, tol) {
        m_dTangencyTolRadians = tangency_tol_radians;
    }
};

// Only fixture injection/observation: all behavior under test remains native.
class Executive : public SmOffsetExecutive {
public:
    Executive(SmContext& ctx, SmOffsetOperationType op, SmOffsetGeometryCreation& geometry,
              SmBrep* original) : SmOffsetExecutive(ctx, op, geometry, original) {}
    void install(std::unique_ptr<SmBrep> offset) { m_pOffsetBrep = offset.release(); }
    SmStatus relate(SmEdgeuse* a, SmEdgeuse* b) { return m_vEUToEU.RelatePair(a, b); }
    void seconds(SmEdgeuse* a, SmTArray<SmEdgeuse*>& b) { m_vEUToEU.GetSeconds(a, b); }
    SmStatus relate_face(SmFaceuse* a, SmFace* b) { return m_vFUToF.RelatePair(a, b); }
    bool merge() const { return m_bMergeResults != FALSE; }
    bool solid() const { return m_bCreateOffsetSolid != FALSE; }
    SmSurface* cached(SmSurface* base) { return m_vSurfExtSurf.At(base); }
    void forget(SmSurface* base) { m_vSurfExtSurf.Remove(base); }
    void clear() { m_vSurfExtSurf.RemoveAll(); }
    ULONG cache_count() const { return m_vSurfExtSurf.Count(); }
    void faces_of(SmFaceuse* use, SmTArray<SmFace*>& faces) { m_vFUToF.GetSeconds(use, faces); }
};

std::unique_ptr<SmCurve> read_curve(Reader& rd, SmContext& ctx) {
    int64_t kind = rd.index(1);
    if (kind == 0) {
        SmPoint3d start = rd.point(), end = rd.point();
        SmExtent1d domain = rd.interval();
        SmLine* line = nullptr;
        SmStatus status = SmLine::CreateLineSegment(ctx, 3, start, end, line, &domain);
        std::unique_ptr<SmCurve> result(line);
        constructed(status);
        if (!result) throw std::bad_alloc();
        return result;
    }
    auto degree = (gw_DEGREE)rd.index(max_degree);
    auto npoints = (gw_INDEX)rd.index(max_points);
    if (degree < 1 || npoints <= degree) throw Argument{};
    gw_INDEX n = npoints - 1, m = npoints + degree;
    std::unique_ptr<gw_CURVE, decltype(&sm_FreeNurbCurve)> raw(
        sm_AllocateNurbCurve(n, degree, m), &sm_FreeNurbCurve);
    gw_CURVE* curve = raw.get();
    if (!curve) throw std::bad_alloc();
    // The native constructor tunes knots, so initialize storage BEFORE invoking it.
    for (gw_INDEX i=0; i<npoints; ++i) {
        curve->pol->Pw[i].x = rd.number(); curve->pol->Pw[i].y = rd.number();
        curve->pol->Pw[i].z = rd.number(); curve->pol->Pw[i].w = rd.number();
    }
    for (gw_INDEX i=0; i<=m; ++i) curve->knt->U[i] = rd.number();
    std::unique_ptr<SmCurve> result(new (ctx) SmBSplineCurve(curve, 3, FALSE, &ctx));
    raw.release();
    return result;
}

SmEdge* make_wire(SmContext& ctx, SmBrep* target, std::unique_ptr<SmCurve> curve,
                  const SmExtent1d& interval) {
    SmPoint3d a(0,0,0), b(0,0,0);
    constructed(curve->EvaluatePoint(interval.GetMin(), a));
    constructed(curve->EvaluatePoint(interval.GetMax(), b));
    SmShell* shell = nullptr;
    SmVertex *v1 = nullptr, *v2 = nullptr;
    constructed(target->MakeShellVertex(nullptr, a, shell, v1));
    constructed(target->MakeShellVertex(nullptr, b, shell, v2));
    SmEdge* edge = nullptr;
    // MakeWireEdge can fail after assigning the curve to a newly made edge.
    // The guard must not delete a curve already owned by that topology.
    struct Pending {
        SmCurve* p;
        ~Pending() { if (p && !p->GetOwner()) delete p; }
    } pending{curve.release()};
    constructed(target->MakeWireEdge(nullptr, v1, v2, pending.p, interval, SM_OT_SAME, edge));
    if (!edge || !edge->GetPrimaryEdgeuse()) throw Argument{};
    (void)ctx;
    return edge;
}

SmEdge* unit_wire(SmContext& ctx, SmBrep* target, double y) {
    SmLine* curve = nullptr;
    SmExtent1d interval(0,1);
    constructed(SmLine::CreateLineSegment(ctx, 3, SmPoint3d(0,y,0), SmPoint3d(1,y,0),
                                          curve, &interval));
    return make_wire(ctx, target, std::unique_ptr<SmCurve>(curve), interval);
}

SmFace* plane_face(SmContext& ctx, SmBrep* target, double z) {
    SmExtent2d domain(SmPoint2d(0,0), SmPoint2d(1,1));
    SmPlane* plane = new (ctx) SmPlane(SmPoint3d(0,0,z), SmVector3d(1,0,0),
                                     SmVector3d(0,1,0), SmVector2d(1,1), domain, &ctx);
    struct Pending {
        SmSurface* p;
        ~Pending() { if (p && !p->GetOwner()) delete p; }
    } pending{plane};
    SmFace* face = nullptr;
    constructed(target->CreateFaceFromSurface(plane, domain, face));
    if (!face) throw Argument{};
    return face;
}

std::unique_ptr<SmSurface> read_surface(Reader& rd, SmContext& ctx) {
    int64_t kind = rd.index(1);
    if (kind == 0) {
        SmPoint3d origin = rd.point();
        SmVector3d x = rd.point(), y = rd.point();
        SmVector2d scale = rd.uv();
        SmExtent2d domain = rd.domain();
        return std::unique_ptr<SmSurface>(new (ctx) SmPlane(origin, x, y, scale, domain, &ctx));
    }
    auto p = (gw_DEGREE)rd.index(max_degree), q = (gw_DEGREE)rd.index(max_degree);
    auto nu = (gw_INDEX)rd.index(max_points), nv = (gw_INDEX)rd.index(max_points);
    if (p < 1 || q < 1 || nu <= p || nv <= q || (int64_t)nu*nv > max_points)
        throw Argument{};
    gw_INDEX r = nu+p, s = nv+q;
    auto free_surface = [](gw_SURFACE* p) { smos_Free(p); };
    std::unique_ptr<gw_SURFACE, decltype(free_surface)> raw(
        sm_AllocateNurbSurface(nu-1, nv-1, p, q, r, s), free_surface);
    gw_SURFACE* surface = raw.get();
    if (!surface) throw std::bad_alloc();
    for (gw_INDEX i=0; i<nu; ++i) for (gw_INDEX j=0; j<nv; ++j) {
        surface->net->Pw[i][j].x = rd.number(); surface->net->Pw[i][j].y = rd.number();
        surface->net->Pw[i][j].z = rd.number(); surface->net->Pw[i][j].w = rd.number();
    }
    for (gw_INDEX i=0; i<=r; ++i) surface->knu->U[i] = rd.number();
    for (gw_INDEX i=0; i<=s; ++i) surface->knv->U[i] = rd.number();
    std::unique_ptr<SmBSplineSurface> result(new (ctx) SmBSplineSurface(surface, FALSE, &ctx));
    raw.release();
    result->SetBSplineSurfaceForm(SM_SF_UNSPECIFIED);
    return std::unique_ptr<SmSurface>(result.release());
}

void state(std::vector<double>& out, Executive& executive, SmOffsetGeometryCreation& geometry) {
    SmTArray<SmFace*> faces;
    executive.GetShellFaces(faces);
    out.insert(out.end(), {
        geometry.GetOffsetDistance(), geometry.GetThisApproxTol3d(),
        (double)executive.GetOffsetOperation(), (double)executive.GetInset(),
        (double)executive.GetExtendConvexEdges(), (double)executive.merge(),
        (double)executive.solid(), executive.GetTangencyTolDegrees(),
        executive.GetOffsetBrep() ? 1.0 : 0.0, (double)faces.GetSize()
    });
}

void config(Reader& rd, std::vector<double>& out) {
    double distance = rd.number(), tol = rd.number(), tangency_tol_radians = rd.number();
    auto operation = (SmOffsetOperationType)rd.index(4);
    bool inset = rd.flag(), extend = rd.flag(), merge = rd.flag(), solid = rd.flag();
    bool install = rd.flag();
    int64_t norig = rd.index(), noffset = rd.index(), nfaces = rd.index();
    SmContext ctx;
    auto original = brep(ctx), offset = brep(ctx), face_owner = brep(ctx);
    for (int64_t i=0; i<norig; ++i) unit_wire(ctx, original.get(), (double)i);
    for (int64_t i=0; i<noffset; ++i) unit_wire(ctx, offset.get(), (double)i);
    for (int64_t i=0; i<nfaces; ++i) plane_face(ctx, face_owner.get(), (double)i);
    SmTArray<SmEdgeuse*> originals, offsets;
    SmTArray<SmFace*> faces;
    original->GetEdgeuses(originals); offset->GetEdgeuses(offsets); face_owner->GetFaces(faces);
    ConfigGeometry geometry(distance, tol, tangency_tol_radians);
    Executive executive(ctx, operation, geometry, original.get());
    out.push_back(0);
    state(out, executive, geometry);
    executive.SetInset(inset ? TRUE : FALSE);
    executive.SetExtendConvexEdges(extend ? TRUE : FALSE);
    executive.SetMergeResults(merge ? TRUE : FALSE);
    executive.SetCreateOffsetSolid(solid ? TRUE : FALSE);
    if (install) executive.install(std::move(offset)); // only executive deletes this B-rep.
    std::vector<double> shell_records;
    int64_t nsets = rd.index();
    shell_records.push_back((double)nsets);
    for (int64_t i=0; i<nsets; ++i) {
        SmTArray<SmFace*> selectors, result;
        int64_t n = rd.index();
        for (int64_t j=0; j<n; ++j) {
            int64_t index = rd.index();
            if (index >= faces.GetSize()) throw Argument{};
            selectors.Add(faces[(ULONG)index]);
        }
        executive.SetShellFaces(selectors);
        // Mutating the caller's array checks SetShellFaces' copy, and prefilling
        // the destination checks that GetShellFaces resets, rather than appends.
        selectors.ReSet();
        if (faces.GetSize()) result.Add(faces[0]);
        executive.GetShellFaces(result);
        shell_records.push_back(result.GetSize());
        for (ULONG j=0; j<result.GetSize(); ++j) shell_records.push_back(id(faces, result[j]));
    }
    state(out, executive, geometry);
    out.insert(out.end(), shell_records.begin(), shell_records.end());
    int64_t npairs = rd.index();
    std::vector<double> statuses;
    for (int64_t i=0; i<npairs; ++i) {
        int64_t a = rd.index(max_items*2), b = rd.index(max_items*2);
        if (a >= originals.GetSize() || b >= offsets.GetSize()) throw Argument{};
        statuses.push_back(executive.relate(originals[(ULONG)a], offsets[(ULONG)b]));
    }
    rd.done();
    out.push_back(originals.GetSize());
    for (ULONG i=0; i<originals.GetSize(); ++i) {
        SmTArray<SmEdgeuse*> seconds;
        out.push_back(id(offsets, executive.GetOffsetEdgeuse(originals[i])));
        executive.seconds(originals[i], seconds);
        put_ids(out, seconds, offsets);
    }
    out.push_back((double)statuses.size());
    out.insert(out.end(), statuses.begin(), statuses.end());
}

// A closing segment requires distinct endpoints (SmBSplineCurve.cpp's SAME_PNT_ER).
// For exactly coincident endpoints, use a nonzero closed cubic instead; only this
// unmeasured support changes kind. Its fixed unit weights and clamped [0,1] knots
// are valid native constructor data, not a replacement for any measured geometry.
std::unique_ptr<SmCurve> closing_curve(SmContext& ctx, const SmPoint3d& start,
                                       const SmPoint3d& end) {
    if (start.x == end.x && start.y == end.y && start.z == end.z) {
        std::unique_ptr<gw_CURVE, decltype(&sm_FreeNurbCurve)> raw(
            sm_AllocateNurbCurve(3, 3, 7), &sm_FreeNurbCurve);
        if (!raw) throw std::bad_alloc();
        const SmPoint3d points[] = {start, SmPoint3d(1,0,0), SmPoint3d(0,1,0), end};
        for (gw_INDEX i=0; i<4; ++i) {
            raw->pol->Pw[i].x = points[i].x; raw->pol->Pw[i].y = points[i].y;
            raw->pol->Pw[i].z = points[i].z; raw->pol->Pw[i].w = 1;
        }
        for (gw_INDEX i=0; i<8; ++i) raw->knt->U[i] = i < 4 ? 0 : 1;
        std::unique_ptr<SmCurve> result(new (ctx) SmBSplineCurve(raw.get(), 3, FALSE, &ctx));
        raw.release();
        return result;
    }
    SmBSplineCurve* closing = nullptr;
    SmStatus status = SmBSplineCurve::CreateLineSegment(ctx, 3, start, end, closing);
    std::unique_ptr<SmCurve> result(closing);
    constructed(status);
    if (!result) throw std::bad_alloc();
    return result;
}

// Give each mapped wire a genuine two-edge loop. The second edge closes the
// loop and is fixture scaffolding only. No UV trims are requested: synchronization
// concerns the 3D edge ranges and must also accept nonplanar B-spline curves.
SmEdge* loop_fixture(SmContext& ctx, SmBrep* target, std::unique_ptr<SmCurve> curve,
                     const SmExtent1d& interval, std::vector<SmEdge*>& supports) {
    SmEdge* edge = make_wire(ctx, target, std::move(curve), interval);
    SmVertex* a = edge->GetStartVertex();
    SmVertex* b = edge->GetOtherVertex(a);
    auto closing_owner = closing_curve(ctx, b->GetPoint(), a->GetPoint());
    SmExtent1d unit = closing_owner->GetNaturalInterval();
    struct PendingCurve {
        SmCurve* p;
        ~PendingCurve() { if (p && !p->GetOwner()) delete p; }
    } pending_curve{closing_owner.release()};
    SmEdge* support = nullptr;
    constructed(target->MakeWireEdge(nullptr, b, a, pending_curve.p,
                                      unit, SM_OT_SAME, support));
    if (!support) throw Argument{};
    supports.push_back(support);
    SmTArray<ULONG> loop_sizes;
    SmTArray<SmEdgeuse*> uses;
    SmTArray<SmVertex*> vertices;
    loop_sizes.Add(2);
    uses.Add(edge->GetPrimaryEdgeuse()); uses.Add(support->GetPrimaryEdgeuse());
    SmExtent2d domain(SmPoint2d(0,0), SmPoint2d(1,1));
    struct PendingSurface {
        SmSurface* p;
        ~PendingSurface() { if (p && !p->GetOwner()) delete p; }
    } plane{new (ctx) SmPlane(SmPoint3d(0,0,0), SmVector3d(1,0,0), SmVector3d(0,1,0),
                              SmVector2d(1,1), domain, &ctx)};
    SmRegion* region = nullptr;
    SmShell* shell = nullptr;
    SmFace* face = nullptr;
    constructed(target->MakeFaceTopology(nullptr, plane.p, SM_OT_SAME, loop_sizes, uses, vertices,
                                          region, shell, face, FALSE, FALSE));
    SmEdgeuse* use = edge->GetPrimaryEdgeuse();
    if (!use || !use->GetCCWEdgeuse() || !use->GetCWEdgeuse()) throw Argument{};
    return edge;
}

// Native traversal restricted to the measured edges: auxiliary closing edges are
// not part of the unit's inputs or output topology. Split descendants remain here.
void measured_traversal(SmBrep* target, const std::vector<SmEdge*>& supports,
                         SmTArray<SmEdge*>& edges, SmTArray<SmEdgeuse*>& uses) {
    edges.ReSet(); uses.ReSet();
    SmTArray<SmEdge*> all;
    target->GetEdges(all);
    for (ULONG i=0; i<all.GetSize(); ++i) {
        if (std::find(supports.begin(), supports.end(), all[i]) != supports.end()) continue;
        edges.Add(all[i]);
        SmTArray<SmEdgeuse*> pair;
        all[i]->GetEdgeuses(pair);
        uses.Append(pair);
    }
}

void put_eu_records(std::vector<double>& out, const SmTArray<SmEdge*>& edges,
                    const SmTArray<SmEdgeuse*>& uses) {
    out.push_back(uses.GetSize());
    for (ULONG i=0; i<uses.GetSize(); ++i) {
        SmEdge* edge = uses[i]->GetEdge();
        const SmExtent1d& interval = edge->GetInterval();
        SmCurve* curve = edge->GetCurve();
        out.insert(out.end(), {(double)i, id(edges, edge), interval.GetMin(), interval.GetMax(),
                                curve->ApproximateLength(interval, 10)});
        SmPoint3d point(0,0,0);
        out.push_back(curve->EvaluatePoint(interval.GetMin(), point));
        put_point(out, point);
        point.Set(0,0,0);
        out.push_back(curve->EvaluatePoint(interval.GetMax(), point));
        put_point(out, point);
    }
}

void synchronize(Reader& rd, std::vector<double>& out) {
    bool null_original = rd.flag(), null_other = rd.flag();
    SmContext ctx;
    auto original = brep(ctx), offset = brep(ctx);
    SmEdgeuse* a = unit_wire(ctx, original.get(), 0)->GetPrimaryEdgeuse();
    SmEdgeuse* b = unit_wire(ctx, original.get(), 1)->GetPrimaryEdgeuse();
    SmTArray<SmEdgeuse*> first, second;
    std::vector<SmEdge*> supports;
    for (int side=0; side<2; ++side) {
        int64_t n = rd.index();
        for (int64_t i=0; i<n; ++i) {
            auto curve = read_curve(rd, ctx);
            SmExtent1d interval = rd.interval();
            SmEdge* edge = loop_fixture(ctx, offset.get(), std::move(curve), interval, supports);
            (side == 0 ? first : second).Add(edge->GetPrimaryEdgeuse());
        }
    }
    rd.done();
    SmOffsetGeometryCreation geometry(1, 1e-6);
    Executive executive(ctx, SM_OO_SOLID_OFFSET, geometry, original.get());
    SmBrep* target = offset.get();
    executive.install(std::move(offset));
    for (ULONG i=0; i<first.GetSize(); ++i) {
        SmStatus status = executive.relate(a, first[i]);
        if (status != SM_SUCCESS) throw Argument{};
    }
    if (!null_other) for (ULONG i=0; i<second.GetSize(); ++i) {
        SmStatus status = executive.relate(b, second[i]);
        if (status != SM_SUCCESS) throw Argument{};
    }
    if (null_original) a = nullptr;
    if (null_other) b = nullptr;
    SmTArray<SmEdgeuse*> before, related_a, related_b;
    SmTArray<SmEdge*> measured_edges;
    measured_traversal(target, supports, measured_edges, before);
    double lookup_a = id(before, executive.GetOffsetEdgeuse(a));
    double lookup_b = id(before, executive.GetOffsetEdgeuse(b));
    executive.seconds(a, related_a); executive.seconds(b, related_b);
    std::vector<double> relation_records;
    put_ids(relation_records, related_a, before); put_ids(relation_records, related_b, before);
    SmTArray<SmEdgeuse*> matched_a, matched_b, gap_a, gap_b;
    SmStatus status = executive.SyncronizeOffsetEdgeuses(a, b, matched_a, matched_b, gap_a, gap_b);
    out.insert(out.end(), {0, (double)status, lookup_a, lookup_b});
    SmTArray<SmEdgeuse*> after, fixtures;
    measured_traversal(target, supports, measured_edges, after);
    fixtures.Append(first); fixtures.Append(second);
    put_ids(out, fixtures, after);
    out.insert(out.end(), relation_records.begin(), relation_records.end());
    put_eu_records(out, measured_edges, after);
    put_ids(out, matched_a, after); put_ids(out, matched_b, after);
    put_ids(out, gap_a, after); put_ids(out, gap_b, after);
}

// NL_CrvAdv.cpp's G1 extension retries a zero end tangent using Pw[2] or
// Pw[n-2], even for a two-point curve. Check the native isocurves sampled by
// NL_SrfAdv.cpp first: otherwise an accepted degenerate bilinear fixture can
// read outside its control polygon. This is a safety check, not an extension
// implementation, and does not reject short but normalizable tangents.
void guard_two_point_isocurves(SmSurface* base) {
    SmBSplineSurface* spline = SM_CAST_PTR(SmBSplineSurface, base);
    if (!spline) return;
    gw_SURFACE* surface = spline->GetOrCreateGwNurbPointer();
    if (!surface) throw Argument{};
    for (int direction=0; direction<2; ++direction) {
        if ((direction == 0 ? surface->net->n : surface->net->m) != 1) continue;
        gw_KNOTVECTOR* cross_knots = direction == 0 ? surface->knv : surface->knu;
        for (double fraction : {0.07, 0.19, 0.45, 0.73, 0.92}) {
            double parameter = (1.0-fraction)*cross_knots->U[0]
                             + fraction*cross_knots->U[cross_knots->m];
            std::unique_ptr<gw_CURVE, decltype(&sm_FreeNurbCurve)> curve(
                sm_AllocateNurbCurve(1, 1, 3), &sm_FreeNurbCurve);
            if (!curve) throw std::bad_alloc();
            if (N_SrfExtractIsoCrv(surface, parameter, direction == 0 ? NL_UDIR : NL_VDIR,
                                   curve.get(), nullptr) != 0) continue;
            NL_POINT a, b;
            NL_VECTOR tangent, unit;
            NL_REAL magnitude = 0;
            N_CPtToPtEuclid(curve->pol->Pw[0], &a);
            N_CPtToPtEuclid(curve->pol->Pw[1], &b);
            N_VectorDir(a, b, &tangent);
            if (N_VectorNormalize(tangent, &unit, &magnitude) != 0) throw Argument{};
        }
    }
}

void extend(Reader& rd, std::vector<double>& out) {
    double distance = rd.number(), tol = rd.number();
    if (distance < 0.0) throw Argument{};
    SmContext ctx;
    std::vector<std::unique_ptr<SmSurface>> bases, extended;
    int64_t n = rd.index();
    for (int64_t i=0; i<n; ++i) bases.push_back(read_surface(rd, ctx));
    std::vector<SmPoint2d> samples;
    int64_t nsamples = rd.index();
    for (int64_t i=0; i<nsamples; ++i) samples.push_back(rd.uv());
    auto original = brep(ctx);
    SmOffsetGeometryCreation geometry(distance, tol);
    Executive executive(ctx, SM_OO_SOLID_OFFSET, geometry, original.get());
    int64_t nactions = rd.index();
    out.insert(out.end(), {0, (double)nactions});
    for (int64_t i=0; i<nactions; ++i) {
        int64_t action = rd.index(2);
        if (action == 2) {
            out.insert(out.end(), {2, (double)executive.cache_count(), 0});
            executive.clear();
            continue;
        }
        double raw_key = rd.number();
        if (raw_key < -1 || std::floor(raw_key) != raw_key || raw_key >= (double)bases.size()
            || (action == 1 && raw_key < 0)) throw Argument{};
        SmSurface* base = raw_key == -1 ? nullptr : bases.at((size_t)raw_key).get();
        bool hit = executive.cached(base) != nullptr;
        if (action == 1) {
            executive.forget(base);
            out.insert(out.end(), {1, hit ? 1.0 : 0.0, (double)executive.cache_count()});
            continue;
        }
        if (base && !hit) guard_two_point_isocurves(base);
        SmExtent2d domain;
        SmSurface* result = nullptr;
        SmStatus status = executive.GetCreateExtendedSurface(base, domain, result);
        // SmOffsetExecutive deletes its map, NOT the extended surfaces. Keep each
        // distinct returned allocation alive through eviction/clear and clean it
        // after the executive has destroyed the map. Never wrap a base alias twice.
        double result_id = -1;
        if (result) {
            for (size_t j=0; j<extended.size(); ++j)
                if (extended[j].get() == result) result_id = (double)j;
            if (result_id == -1) {
                if (std::any_of(bases.begin(), bases.end(),
                                [result](const std::unique_ptr<SmSurface>& p){ return p.get() == result; }))
                    throw Argument{}; // unsupported ownership contract, not a fixture rejection.
                result_id = (double)extended.size();
                extended.emplace_back(result);
            }
        }
        out.insert(out.end(), {0, (double)status, hit ? 1.0 : 0.0, result_id, result ? 1.0 : 0.0});
        if (status != SM_SUCCESS) continue;
        if (!result) throw Argument{};
        out.push_back(result->IsKindOf(SmPlane_TYPE) ? 1 :
                      result->IsKindOf(SmBSplineSurface_TYPE) ? 2 : 3);
        put_domain(out, domain);
        out.push_back((double)samples.size());
        for (const SmPoint2d& fraction : samples) {
            SmPoint2d uv((1.0-fraction.x)*domain.GetMin().x + fraction.x*domain.GetMax().x,
                         (1.0-fraction.y)*domain.GetMin().y + fraction.y*domain.GetMax().y);
            SmPoint3d point(0,0,0);
            SmVector3d normal(0,0,0);
            out.push_back(result->EvaluatePoint(uv, point)); put_point(out, point);
            out.push_back(result->EvaluateNormal(uv, TRUE, TRUE, normal)); put_point(out, normal);
        }
    }
    rd.done();
}

// All new geometry remains locally owned even if construction/evaluation throws.
// This guard is created before BuildOffsetOfVertex so partial native outputs are
// cleaned on operation failure as well as on snapshot-allocation exceptions.
struct VertexResult {
    SmTArray<SmCurve*> curves;
    SmTArray<SmOrientType> orientations;
    SmSurface* surface = nullptr;
    ~VertexResult() {
        for (ULONG i=0; i<curves.GetSize(); ++i) delete curves[i];
        delete surface;
    }
};

SmFace* face_on_edges(SmBrep* target, std::unique_ptr<SmSurface> surface,
                      const SmTArray<SmEdgeuse*>& uses) {
    struct Pending {
        SmSurface* p;
        ~Pending() { if (p && !p->GetOwner()) delete p; }
    } pending{surface.release()};
    SmTArray<ULONG> loops;
    loops.Add(uses.GetSize());
    SmTArray<SmVertex*> vertices;
    SmRegion* region = nullptr;
    SmShell* shell = nullptr;
    SmFace* face = nullptr;
    constructed(target->MakeFace(nullptr, loops, uses, vertices, pending.p,
                                pending.p->GetNaturalUVDomain(), SM_OT_SAME,
                                region, shell, face, FALSE, FALSE));
    if (!face) throw Argument{};
    return face;
}

// A bounded plane, an interior vertex-loop, and a genuine new edge joining it
// to the outer boundary. The interior endpoint has exactly one incident edge
// and IsStrut is true. No source/result cap is preseeded.
SmVertex* source_strut(SmContext& ctx, SmBrep* target, const SmPoint3d& point,
                       SmFaceuse*& source_use) {
    SmExtent2d domain(SmPoint2d(-2,-2), SmPoint2d(2,2));
    struct Pending {
        SmSurface* p;
        ~Pending() { if (p && !p->GetOwner()) delete p; }
    } pending{new (ctx) SmPlane(point, SmVector3d(1,0,0), SmVector3d(0,1,0),
                               SmVector2d(1,1), domain, &ctx)};
    SmFace* face = nullptr;
    constructed(target->CreateFaceFromSurface(pending.p, domain, face));
    if (!face) throw Argument{};
    SmTArray<SmVertex*> boundary;
    face->GetVertices(boundary);
    if (!boundary.GetSize()) throw Argument{};
    SmVertex* start = boundary[0];
    if (!start) throw Argument{};
    SmVertex* vertex = nullptr;
    SmLoop* loop = nullptr;
    target->m_bEditingEnabled = TRUE;
    constructed(target->MakeVertexLoop(face, point, loop, vertex));
    if (!vertex) throw Argument{};
    SmBSplineCurve* curve = nullptr;
    SmStatus curve_status = SmBSplineCurve::CreateLineSegment(ctx, 3, start->GetPoint(), point, curve);
    struct PendingCurve {
        SmCurve* p;
        ~PendingCurve() { if (p && !p->GetOwner()) delete p; }
    } pending_curve{curve};
    constructed(curve_status);
    if (!curve) throw Argument{};
    SmEdge* edge = nullptr;
    SmFace* split_face = nullptr;
    constructed(target->MakeEdgeInFace(face, start, vertex, curve, nullptr,
                                      curve->GetNaturalInterval(), SM_OT_SAME, 0.0,
                                      edge, loop, split_face));
    if (!edge || !edge->GetPrimaryEdgeuse()) throw Argument{};
    SmEdgeuse* use = edge->GetPrimaryEdgeuse();
    if (!use->GetVertexuse()) throw Argument{};
    if (use->GetVertexuse()->GetVertex() != vertex) use = use->GetRadial();
    // Safety invariant of this constructor, not an arithmetic parity assertion.
    if (!use || !use->IsStrut() || !use->GetFaceuse()) throw Argument{};
    source_use = use->GetFaceuse();
    return vertex;
}

SmFace* circular_disk(SmContext& ctx, SmBrep* target, const SmPoint3d& center,
                      double radius) {
    SmAxis2Placement placement;
    placement.Translate(center);
    SmBSplineCurve* circle = nullptr;
    SmStatus status = SmBSplineCurve::CreateCircleSegment(ctx, 3, placement, radius,
                                                        0.0, 360.0, SM_CO_QUADRATIC, circle);
    std::unique_ptr<SmCurve> circle_owner(circle);
    constructed(status);
    if (!circle) throw Argument{};
    SmPoint3d point(0,0,0);
    constructed(circle->EvaluatePoint(circle->GetNaturalInterval().GetMin(), point));
    SmShell* shell = nullptr;
    SmVertex* vertex = nullptr;
    constructed(target->MakeShellVertex(nullptr, point, shell, vertex));
    struct Pending {
        SmCurve* p;
        ~Pending() { if (p && !p->GetOwner()) delete p; }
    } pending{circle_owner.release()};
    SmEdge* edge = nullptr;
    constructed(target->MakeWireEdge(nullptr, vertex, vertex, pending.p,
                                    circle->GetNaturalInterval(), SM_OT_SAME, edge));
    if (!edge || !edge->GetPrimaryEdgeuse()) throw Argument{};
    SmTArray<SmEdgeuse*> uses;
    uses.Add(edge->GetPrimaryEdgeuse());
    SmExtent2d domain(SmPoint2d(-radius,-radius), SmPoint2d(radius,radius));
    std::unique_ptr<SmSurface> plane(new (ctx) SmPlane(center, SmVector3d(1,0,0),
                                                     SmVector3d(0,1,0), SmVector2d(1,1),
                                                     domain, &ctx));
    return face_on_edges(target, std::move(plane), uses);
}

void put_vertex_surface(std::vector<double>& out, SmSurface* surface,
                        const std::vector<SmPoint2d>& samples) {
    out.push_back(surface->GetType());
    SmExtent2d domain = surface->GetNaturalUVDomain();
    put_domain(out, domain);
    // GetType is compared separately: never turn an analytic Sphere into a
    // B-spline merely to make the samples agree.
    SmBSplineSurface* spline = SM_CAST_PTR(SmBSplineSurface, surface);
    gw_SURFACE* nurb = spline ? spline->GetGwNurbPointer() : nullptr;
    out.push_back(nurb ? 1.0 : 0.0);
    if (nurb) {
        gw_INDEX n=nurb->net->n, m=nurb->net->m, r=nurb->knu->m, s=nurb->knv->m;
        out.insert(out.end(), {(double)n, (double)m, (double)nurb->p,
                               (double)nurb->q, (double)r, (double)s});
        for (gw_INDEX i=0; i<=n; ++i) for (gw_INDEX j=0; j<=m; ++j) {
            const auto& point = nurb->net->Pw[i][j];
            out.insert(out.end(), {point.x,point.y,point.z,point.w});
        }
        for (gw_INDEX i=0; i<=r; ++i) out.push_back(nurb->knu->U[i]);
        for (gw_INDEX i=0; i<=s; ++i) out.push_back(nurb->knv->U[i]);
    }
    out.push_back(samples.size());
    for (const auto& sample : samples) {
        SmPoint2d uv((1.0-sample.x)*domain.GetMin().x + sample.x*domain.GetMax().x,
                     (1.0-sample.y)*domain.GetMin().y + sample.y*domain.GetMax().y);
        SmPoint3d point(0,0,0);
        SmVector3d du(0,0,0), dv(0,0,0), duv(0,0,0), duu(0,0,0), dvv(0,0,0);
        SmStatus status = surface->Evaluate2ndDerivatives(uv, TRUE, TRUE,
                                                        point, du, dv, duv, duu, dvv);
        out.push_back(status);
        if (status != SM_SUCCESS) {
            out.insert(out.end(), 18, 0.0);
        } else {
            put_point(out, point); put_point(out, du); put_point(out, dv);
            put_point(out, duv); put_point(out, duu); put_point(out, dvv);
        }
    }
}

void vertex(Reader& rd, std::vector<double>& out) {
    SmPoint3d point = rd.point();
    double distance=rd.number(), approx_tol=rd.number();
    double original_tol=rd.number(), result_tol=rd.number();
    bool has_strut=rd.flag();
    double radius=0.0, height=0.0;
    bool map_mate=false;
    if (has_strut) {
        radius=rd.number(); height=rd.number(); map_mate=rd.flag();
        for (double value : {point.x-2.0, point.x+2.0, point.y-2.0, point.y+2.0,
                             point.z+height, point.x+radius, point.x-radius,
                             point.y+radius, point.y-radius})
            if (!std::isfinite(value)) throw Argument{};
    }
    std::vector<SmPoint2d> samples;
    int64_t count=rd.index();
    for (int64_t i=0; i<count; ++i) {
        SmPoint2d sample=rd.uv();
        if (sample.x<0 || sample.x>1 || sample.y<0 || sample.y>1) throw Argument{};
        samples.push_back(sample);
    }
    rd.done();
    SmContext ctx;
    auto original=brep(ctx), offset=brep(ctx);
    original->SetTolerance(original_tol, FALSE, FALSE);
    offset->SetTolerance(result_tol, FALSE, FALSE);
    SmVertex* source=nullptr;
    SmFaceuse* source_use=nullptr;
    SmFace* mapped_face=nullptr;
    if (has_strut) {
        source=source_strut(ctx, original.get(), point, source_use);
        mapped_face=circular_disk(ctx, offset.get(), SmPoint3d(point.x,point.y,point.z+height), radius);
    } else {
        SmShell* shell=nullptr;
        constructed(original->MakeShellVertex(nullptr, point, shell, source));
    }
    SmOffsetGeometryCreation geometry(distance, approx_tol);
    Executive executive(ctx, SM_OO_SOLID_OFFSET, geometry, original.get());
    executive.install(std::move(offset));
    if (has_strut) {
        if (map_mate) source_use=source_use->GetMate();
        if (executive.relate_face(source_use, mapped_face) != SM_SUCCESS) throw Argument{};
    }
    VertexResult result;
    SmStatus status=executive.BuildOffsetOfVertex(source, result.curves, result.orientations,
                                                 result.surface);
    out.insert(out.end(), {0, (double)status, original->GetTolerance(),
                           executive.GetOffsetBrep()->GetTolerance(),
                           (double)result.curves.GetSize(), (double)result.orientations.GetSize(),
                           result.surface ? 1.0 : 0.0});
    for (ULONG i=0; i<result.orientations.GetSize(); ++i) out.push_back(result.orientations[i]);
    for (ULONG i=0; i<result.curves.GetSize(); ++i) {
        SmCurve* curve=result.curves[i];
        if (!curve) throw Argument{};
        SmExtent1d interval=curve->GetNaturalInterval();
        out.insert(out.end(), {(double)curve->GetType(), interval.GetMin(), interval.GetMax(), 4});
        for (double fraction : {0.113,0.371,0.729,0.913}) {
            double parameter=(1.0-fraction)*interval.GetMin()+fraction*interval.GetMax();
            SmPoint3d point(0,0,0);
            SmStatus status=curve->EvaluatePoint(parameter,point);
            out.push_back(status);
            if (status==SM_SUCCESS) put_point(out,point);
            else out.insert(out.end(),3,0.0);
        }
    }
    if (result.surface) put_vertex_surface(out, result.surface, samples);
}

void probe(Reader& rd, std::vector<double>& out) {
    int64_t operation = rd.index(12);
    double tol = rd.number();
    rd.done();
    SmContext ctx;
    auto original = brep(ctx), offset = brep(ctx);
    SmOffsetGeometryCreation geometry(0.25, tol);
    Executive executive(ctx, SM_OO_SOLID_OFFSET, geometry, original.get());
    executive.install(std::move(offset));
    SmTArray<SmCurve*> curves;
    SmTArray<SmOrientType> orientations;
    SmSurface* surface = nullptr;
    SmBrep* result = nullptr;
    SmStatus status = SM_SUCCESS;
    SmTArray<SmEdgeuse*> originals, offsets;
    switch (operation) {
    case 0: status = SmOffsetExecutive::ReplaceImplicitOffsets(original.get(), tol); break;
    case 1: status = executive.RemoveExcessFaces(executive.GetOffsetBrep()); break;
    case 2:
        executive.SetMergeResults(TRUE);
        status = executive.RemoveExcessFaces(executive.GetOffsetBrep()); break;
    case 3: status = executive.BuildSideWallOfEdgeuse(nullptr); break;
    case 4: status = executive.BuildSideWallOfEdgeuse(unit_wire(ctx, original.get(), 0)->GetPrimaryEdgeuse()); break;
    case 5: {
        SmTArray<SmEdgeuse*> gaps;
        SmTArray<SmFace*> faces;
        status = executive.FillGapEdgeuses(nullptr, gaps, faces); break;
    }
    case 6: status = executive.BuildOffsetOfVertex(nullptr, curves, orientations, surface); break;
    case 7: {
        SmShell* shell = nullptr;
        SmVertex* vertex = nullptr;
        constructed(original->MakeShellVertex(nullptr, SmPoint3d(0,0,0), shell, vertex));
        status = executive.BuildOffsetOfVertex(vertex, curves, orientations, surface); break;
    }
    case 8: case 9: {
        SmSSIData intersections;
        SmFaceuse* use = nullptr;
        if (operation == 9) use = plane_face(ctx, original.get(), 0)->GetUpwardFaceuse();
        status = executive.BuildOffsetOfFaceuse(use, surface, intersections); break;
    }
    case 10: status = executive.PiecewiseMerge(result); break;
    case 11: {
        SmFace* face = plane_face(ctx, original.get(), 0);
        status = executive.MapEdgeuses(face->GetUpwardFaceuse(), face);
        original->GetEdgeuses(originals); offsets.Append(originals);
        break;
    }
    case 12: {
        SmFace* face = plane_face(ctx, original.get(), 0);
        face->GetUpwardFaceuse()->GetEdgeuses(originals);
        if (!originals.GetSize()) throw Argument{};
        SmEdgeuse* use = originals[0];
        if (!use->GetRadial()) throw Argument{};
        status = executive.BuildOffsetOfEdgeuses(use, use->GetRadial(), SM_MT_MARK);
        originals.ReSet();
        break;
    }
    }
    // PiecewiseMerge transfers ownership out of m_pOffsetBrep before stitching.
    std::unique_ptr<SmBrep> result_owner(result);
    std::unique_ptr<SmSurface> surface_owner(surface);
    out.insert(out.end(), {0, (double)status, (double)curves.GetSize(), (double)orientations.GetSize(),
                            surface ? 1.0 : 0.0, result ? 1.0 : 0.0,
                            executive.GetOffsetBrep() ? 1.0 : 0.0});
    out.push_back(originals.GetSize());
    for (ULONG i=0; i<originals.GetSize(); ++i)
        out.push_back(id(offsets, executive.GetOffsetEdgeuse(originals[i])));
    for (ULONG i=0; i<curves.GetSize(); ++i) delete curves[i];
}
// Geometry handed to native constructors that may or may not adopt it: anything
// still unowned when the scope ends is the fixture's to delete.
struct Unowned {
    std::vector<SmCurve*> curves;
    SmSurface* surface = nullptr;
    Unowned() = default;
    Unowned(const Unowned&) = delete;
    Unowned& operator=(const Unowned&) = delete;
    ~Unowned() {
        for (SmCurve* c : curves) if (c && !c->GetOwner()) delete c;
        if (surface && !surface->GetOwner()) delete surface;
    }
};

SmPoint3d vertex_point(SmEdgeuse* use) {
    if (!use || !use->GetVertexuse() || !use->GetVertexuse()->GetVertex()) throw Argument{};
    return use->GetVertexuse()->GetVertex()->GetPoint();
}

// A plane face made by MakeFaceWithCurves from explicit 3D and UV lines
// (CreateFaceFromSurface's natural trim curves have no Rust fixture twin): the
// straight-edged loop `points` at height z on the plane with axes X, Y (or Y, X
// when `flip`, so its normal points down and UV is (y, x)), the plane's domain =
// the loop's UV bounding box.
SmFace* curve_face(SmContext& ctx, SmBrep* target, const std::vector<SmPoint2d>& points, double z,
                   bool flip) {
    const size_t n = points.size();
    if (n < 3) throw Argument{};
    double x0 = points[0].x, y0 = points[0].y, x1 = x0, y1 = y0;
    for (const SmPoint2d& p : points) {
        x0 = std::min(x0, p.x); y0 = std::min(y0, p.y); x1 = std::max(x1, p.x); y1 = std::max(y1, p.y);
    }
    Unowned owned;
    SmTArray<SmCurve*> curves3d;
    SmTArray<SmBSplineCurve*> uv;
    // MakeFaceWithCurves deletes the UV curves it is given (SmObjsDelete,
    // SmBrep.cpp:15743), so they leave this guard when the call is made.
    struct UvGuard {
        SmTArray<SmBSplineCurve*>* list;
        ~UvGuard() { if (list) for (ULONG i=0; i<list->GetSize(); ++i) delete (*list)[i]; }
    } uv_guard{&uv};
    SmTArray<SmOrientType> orients;
    SmTArray<SmPoint3d> vertex_points; // MakeFaceWithCurves loop points (none)
    SmTArray<ULONG> loops;
    SmExtent1d unit(0,1);
    auto uv_of = [flip](double x, double y) { return flip ? SmPoint3d(y, x, 0) : SmPoint3d(x, y, 0); };
    for (size_t i=0; i<n; ++i) {
        const SmPoint2d& pa = points[i];
        const SmPoint2d& pb = points[(i+1) % n];
        SmPoint3d a(pa.x, pa.y, z), b(pb.x, pb.y, z);
        SmLine* line = nullptr;
        SmStatus status = SmLine::CreateLineSegment(ctx, 3, a, b, line, &unit);
        owned.curves.push_back(line);
        constructed(status);
        SmBSplineCurve* trim = nullptr;
        status = SmBSplineCurve::CreateLineSegment(ctx, 2, uv_of(a.x, a.y), uv_of(b.x, b.y), trim);
        uv.Add(trim); // deleted with the array until handed to MakeFaceWithCurves
        constructed(status);
        if (!line || !trim) throw std::bad_alloc();
        curves3d.Add(line); orients.Add(SM_OT_SAME);
    }
    loops.Add((ULONG)n);
    SmExtent2d domain = flip ? SmExtent2d(SmPoint2d(y0, x0), SmPoint2d(y1, x1))
                             : SmExtent2d(SmPoint2d(x0, y0), SmPoint2d(x1, y1));
    owned.surface = new (ctx) SmPlane(SmPoint3d(0,0,z), flip ? SmVector3d(0,1,0) : SmVector3d(1,0,0),
                                      flip ? SmVector3d(1,0,0) : SmVector3d(0,1,0),
                                      SmVector2d(1,1), domain, &ctx);
    SmRegion* region = nullptr;
    SmShell* shell = nullptr;
    SmFace* face = nullptr;
    uv_guard.list = nullptr;
    constructed(target->MakeFaceWithCurves(target->GetInfiniteRegion(), loops, &curves3d, &uv,
                                           orients, vertex_points, owned.surface, domain, SM_OT_SAME,
                                           region, shell, face));
    if (!face || !face->GetUpwardFaceuse() || !face->GetUpwardFaceuse()->GetMate())
        throw Argument{};
    return face;
}
// The loop of a rectangle, counter-clockwise from its lower-left corner, with the
// bottom edge split at `split` when given.
std::vector<SmPoint2d> rectangle_loop(const SmExtent2d& rect, const double* split) {
    const double x0 = rect.GetMin().x, y0 = rect.GetMin().y, x1 = rect.GetMax().x, y1 = rect.GetMax().y;
    std::vector<SmPoint2d> points{SmPoint2d(x0, y0)};
    if (split) points.push_back(SmPoint2d(*split, y0));
    points.insert(points.end(), {SmPoint2d(x1, y0), SmPoint2d(x1, y1), SmPoint2d(x0, y1)});
    return points;
}
SmFace* curve_face(SmContext& ctx, SmBrep* target) {
    return curve_face(ctx, target, rectangle_loop(SmExtent2d(SmPoint2d(0,0), SmPoint2d(1,1)), nullptr),
                      0.0, false);
}

// Per original edge use of `use`, sorted by the use's own start point (GetEdgeuses
// starts a loop at a construction-dependent use): start xyz, then -1 when
// GetOffsetEdgeuse is null, else 1 and the start points of the offset use and of
// its mate.
void put_mapped_uses(std::vector<double>& out, Executive& executive, SmFaceuse* use) {
    SmTArray<SmEdgeuse*> uses;
    use->GetEdgeuses(uses);
    std::vector<std::vector<double>> records;
    for (ULONG i=0; i<uses.GetSize(); ++i) {
        std::vector<double> record;
        put_point(record, vertex_point(uses[i]));
        SmEdgeuse* mapped = executive.GetOffsetEdgeuse(uses[i]);
        if (!mapped) { record.push_back(-1); }
        else {
            record.push_back(1);
            put_point(record, vertex_point(mapped));
            put_point(record, vertex_point(mapped->GetMate()));
        }
        records.push_back(record);
    }
    std::sort(records.begin(), records.end());
    out.push_back((double)records.size());
    for (auto& r : records) out.insert(out.end(), r.begin(), r.end());
}

// BuildOffsetOfFaceuse with a real offset plane z = distance over the given domain.
// Input: distance, approx_tol, domain, local, extend, merge, mate, then a selector
// 0 / 1 / 2 and that many UV segments of one self-intersection: the 3D line at
// z = distance runs along the first segment; the second UV curve is the same line
// (selector 1) or the second segment (selector 2). Output: status, the related
// faces (edge count and UV domain each, sorted by domain), offset B-rep
// face/edge/vertex counts, its sorted vertex points, then per original edge use
// (sorted by its start point) GetOffsetEdgeuse's end points.
void face_tail(Reader& rd, std::vector<double>& out) {
    double distance = rd.number(), tol = rd.number();
    SmExtent2d domain = rd.domain();
    bool local = rd.flag(), extend = rd.flag(), merge = rd.flag(), mate = rd.flag();
    int64_t segments = rd.index(2);
    bool has_ssi = segments > 0;
    SmPoint2d s0(0,0), s1(0,0), t0(0,0), t1(0,0);
    if (has_ssi) { s0 = rd.uv(); s1 = rd.uv(); t0 = s0; t1 = s1; }
    if (segments == 2) { t0 = rd.uv(); t1 = rd.uv(); }
    rd.done();
    SmContext ctx;
    auto original = brep(ctx), offset = brep(ctx);
    SmFace* face = curve_face(ctx, original.get());
    SmFaceuse* use = face->GetUpwardFaceuse();
    if (mate) use = use->GetMate();
    SmOffsetGeometryCreation geometry(distance, tol);
    Executive executive(ctx, local ? SM_OO_LOCAL_OPERATION : SM_OO_SOLID_OFFSET, geometry,
                        original.get());
    executive.install(std::move(offset));
    executive.SetExtendConvexEdges(extend ? TRUE : FALSE);
    executive.SetMergeResults(merge ? TRUE : FALSE);
    // Declared after the executive: checks ownership before the offset B-rep dies.
    Unowned owned;
    SmSSIData intersections;
    if (has_ssi) {
        SmExtent1d unit(0,1);
        SmLine* line = nullptr;
        SmStatus status = SmLine::CreateLineSegment(ctx, 3, SmPoint3d(s0.x, s0.y, distance),
                                                    SmPoint3d(s1.x, s1.y, distance), line, &unit);
        owned.curves.push_back(line);
        constructed(status);
        intersections.m_v3DCurves.Add(line);
        for (int i=0; i<2; ++i) {
            SmBSplineCurve* trim = nullptr;
            const SmPoint2d& a = i ? t0 : s0;
            const SmPoint2d& b = i ? t1 : s1;
            status = SmBSplineCurve::CreateLineSegment(ctx, 2, SmPoint3d(a.x, a.y, 0),
                                                       SmPoint3d(b.x, b.y, 0), trim);
            owned.curves.push_back(trim);
            constructed(status);
            (i ? intersections.m_vUVCurves2 : intersections.m_vUVCurves1).Add(trim);
        }
    }
    owned.surface = new (ctx) SmPlane(SmPoint3d(0,0,distance), SmVector3d(1,0,0),
                                      SmVector3d(0,1,0), SmVector2d(1,1), domain, &ctx);
    SmSurface* surface = owned.surface;
    SmStatus status = executive.BuildOffsetOfFaceuse(use, surface, intersections);
    out.insert(out.end(), {0, (double)status});
    SmTArray<SmFace*> related;
    executive.faces_of(use, related);
    std::vector<std::vector<double>> summaries;
    for (ULONG i=0; i<related.GetSize(); ++i) {
        SmTArray<SmEdge*> edges;
        related[i]->GetEdges(edges);
        SmExtent2d d = related[i]->GetUVDomain();
        summaries.push_back({d.GetMin().x, d.GetMin().y, d.GetMax().x, d.GetMax().y,
                             (double)edges.GetSize()});
    }
    std::sort(summaries.begin(), summaries.end());
    out.push_back((double)summaries.size());
    for (auto& v : summaries) out.insert(out.end(), v.begin(), v.end());
    SmBrep* result = executive.GetOffsetBrep();
    if (!result) throw Argument{};
    SmTArray<SmFace*> faces;
    SmTArray<SmEdge*> edges;
    SmTArray<SmVertex*> vertices;
    result->GetFaces(faces); result->GetEdges(edges); result->GetVertices(vertices);
    out.insert(out.end(), {(double)faces.GetSize(), (double)edges.GetSize(),
                           (double)vertices.GetSize()});
    std::vector<std::vector<double>> points;
    for (ULONG i=0; i<vertices.GetSize(); ++i) {
        SmPoint3d p = vertices[i]->GetPoint();
        points.push_back({p.x, p.y, p.z});
    }
    std::sort(points.begin(), points.end());
    for (auto& p : points) out.insert(out.end(), p.begin(), p.end());
    put_mapped_uses(out, executive, use);
}

// MapEdgeuses alone, on two MakeFaceWithCurves faces: the original rectangle at
// z = 0 and an offset rectangle at z = distance (its plane flipped when asked),
// matching the upward or downward original use. Input: distance, original rect,
// offset rect, flip, mate, then a flag and optionally the x at which the offset
// rectangle's bottom edge is split into two collinear edges. Output: status, then
// per original edge use (sorted by its start point) GetOffsetEdgeuse's end points
// as in face_tail.
void map_twin(Reader& rd, std::vector<double>& out) {
    double distance = rd.number();
    SmExtent2d original_rect = rd.domain(), offset_rect = rd.domain();
    bool flip = rd.flag(), mate = rd.flag(), has_split = rd.flag();
    double split = 0;
    if (has_split) {
        split = rd.number();
        if (!(offset_rect.GetMin().x < split && split < offset_rect.GetMax().x)) throw Argument{};
    }
    rd.done();
    SmContext ctx;
    auto original = brep(ctx), offset = brep(ctx);
    SmFace* face = curve_face(ctx, original.get(), rectangle_loop(original_rect, nullptr), 0.0, false);
    SmFace* twin = curve_face(ctx, offset.get(), rectangle_loop(offset_rect, has_split ? &split : nullptr),
                              distance, flip);
    SmFaceuse* use = face->GetUpwardFaceuse();
    if (mate) use = use->GetMate();
    SmOffsetGeometryCreation geometry(distance, 1e-6);
    Executive executive(ctx, SM_OO_SOLID_OFFSET, geometry, original.get());
    executive.install(std::move(offset));
    SmStatus status = executive.MapEdgeuses(use, twin);
    out.insert(out.end(), {0, (double)status});
    put_mapped_uses(out, executive, use);
}
void replace(Reader& rd, std::vector<double>& out) {
    SmContext ctx;
    int64_t base_kind = rd.index(1);
    std::unique_ptr<SmSurface> base;
    if (base_kind == 0) {
        base = read_surface(rd, ctx);
    } else {
        std::unique_ptr<SmCurve> curve = read_curve(rd, ctx);
        SmBSplineCurve* generator = SM_CAST_PTR(SmBSplineCurve, curve.get());
        if (!generator) throw Argument{};
        SmPoint3d origin = rd.point();
        SmVector3d direction = rd.point();
        SmExtent2d domain = rd.domain();
        bool swap = rd.flag();
        // The extrusion owns its generator curve.
        base.reset(new (ctx) SmSurfOfExtrusion(generator, origin, direction, domain,
                                               swap ? TRUE : FALSE));
        curve.release();
    }
    double distance = rd.number(), tol = rd.number();
    int64_t nsamples = rd.index(64);
    std::vector<SmPoint2d> samples;
    for (int64_t i=0; i<nsamples; ++i) {
        SmPoint2d sample = rd.uv();
        if (sample.x<0 || sample.x>1 || sample.y<0 || sample.y>1) throw Argument{};
        samples.push_back(sample);
    }
    rd.done();
    auto original = brep(ctx);
    struct Pending {
        SmSurface* p;
        ~Pending() { if (p && !p->GetOwner()) delete p; }
    } pending{new (ctx) SmOffsetSurface(distance, *base, TRUE)};
    base.release();
    SmExtent2d natural = pending.p->GetNaturalUVDomain();
    SmFace* face = nullptr;
    constructed(original->CreateFaceFromSurface(pending.p, natural, face));
    if (!face) throw Argument{};
    SmStatus status = SmOffsetExecutive::ReplaceImplicitOffsets(original.get(), tol);
    SmTArray<SmFace*> faces;
    original->GetFaces(faces);
    out.insert(out.end(), {0, (double)status, (double)faces.GetSize()});
    for (ULONG i=0; i<faces.GetSize(); ++i) {
        SmSurface* surface = faces[i]->GetSurface();
        out.push_back(surface ? (double)surface->GetType() : -1.0);
        put_domain(out, faces[i]->GetUVDomain());
        if (!surface) continue;
        SmExtent2d domain = surface->GetNaturalUVDomain();
        put_domain(out, domain);
        out.push_back((double)samples.size());
        for (const SmPoint2d& f : samples) {
            SmPoint2d uv((1.0-f.x)*domain.GetMin().x + f.x*domain.GetMax().x,
                         (1.0-f.y)*domain.GetMin().y + f.y*domain.GetMax().y);
            SmPoint3d point(0,0,0);
            SmStatus eval = surface->EvaluatePoint(uv, point);
            out.push_back(eval);
            if (eval == SM_SUCCESS) put_point(out, point);
            else out.insert(out.end(), 3, 0.0);
        }
    }
}
} // namespace

// Plain-C ABI, mode 0 config / 1 synchronize / 2 extend / 3 probe / 4 face tail /
// 5 map twin / 6 vertex / 7 replace.
// Caller owns every buffer; no native allocation or object address crosses it.
SC_EXPORT int32_t sc_offset_offset_executive_p1_eval(
    int32_t mode, const double* input, int64_t input_len,
    double* out, int64_t cap, int64_t* out_len) {
    if (!out_len || cap < 0 || (cap && !out) || !input || input_len < 0 || input_len > max_values)
        return SC_ERR_ARG;
    try {
        SC_NATIVE_LOCK;
        Reader rd{input, input_len};
        std::vector<double> result;
        try {
            switch (mode) {
            case 0: config(rd, result); break;
            case 1: synchronize(rd, result); break;
            case 2: extend(rd, result); break;
            case 3: probe(rd, result); break;
            case 6: vertex(rd, result); break;
            case 4: face_tail(rd, result); break;
            case 5: map_twin(rd, result); break;
            case 7: replace(rd, result); break;
            default: throw Argument{};
            }
        } catch (const Rejected& rejection) {
            result = {-1, (double)rejection.status};
        }
        return sc_copy_out(result.data(), (int64_t)result.size(), out, cap, out_len);
    } catch (const Argument&) {
        return SC_ERR_ARG;
    } catch (...) {
        return SC_ERR_EXCEPTION;
    }
}
