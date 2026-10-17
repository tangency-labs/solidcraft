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
// Deviations: bounded plain-C fixture protocol, traversal IDs instead of pointers;
// extension-only nonnegative-distance validation. Native array ordering is unchanged.
// No implementation file is included; no native numerical/helper code is copied.
#include "sc_shim.h"
#include "StdAfx.h"
#include <SmBSplineCurve.h>
#include <SmBSplineSurface.h>
#include <SmBrep.h>
#include <SmContext.h>
#include <SmEdge.h>
#include <SmEdgeuse.h>
#include <SmFace.h>
#include <SmFaceuse.h>
#include <SmLine.h>
#include <SmNurbsCrv.h>
#include <SmNurbsSrf.h>
#include <SmOffsetExecutive.h>
#include <SmOffsetGeometryCreation.h>
#include <SmPlane.h>
#include <SmSSIData.h>
#include <SmVertex.h>
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
    bool merge() const { return m_bMergeResults != FALSE; }
    bool solid() const { return m_bCreateOffsetSolid != FALSE; }
    SmSurface* cached(SmSurface* base) { return m_vSurfExtSurf.At(base); }
    void forget(SmSurface* base) { m_vSurfExtSurf.Remove(base); }
    void clear() { m_vSurfExtSurf.RemoveAll(); }
    ULONG cache_count() const { return m_vSurfExtSurf.Count(); }
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
} // namespace

// Plain-C ABI, mode 0 config / 1 synchronize / 2 extend / 3 probe.
// Caller owns every buffer; no native allocation or object address crosses it.
SC_EXPORT int32_t sc_offset_offset_executive_p1_eval(
    int32_t mode, const double* input, int64_t input_len,
    double* out, int64_t cap, int64_t* out_len) {
    if (!out_len || cap < 0 || (cap && !out) || !input || input_len < 0 || input_len > max_values)
        return SC_ERR_ARG;
    try {
        Reader rd{input, input_len};
        std::vector<double> result;
        try {
            switch (mode) {
            case 0: config(rd, result); break;
            case 1: synchronize(rd, result); break;
            case 2: extend(rd, result); break;
            case 3: probe(rd, result); break;
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
