// SPDX-License-Identifier: Apache-2.0
// Real native adapter for offset.offset_executive_p2 (draft PR #201).
// Upstream pinned c9c5979: source/SMLib/{inc,src}/SmOffsetExecutive.{h,cpp},
// source/SM_API/src/SmApiBrep.cpp. Calls all six functions owned by units.json.
// Protocol/DTOs: crates/solidcore-ffi/src/units/offset_offset_executive_p2.rs.
// Deviations: bounded analytic fixtures, traversal IDs and sampled observations;
// no native implementation/helper is included or reimplemented. SC_OK is zero,
// native SM_SUCCESS is 1000. [-1,status] is ONLY a fixture-construction rejection.
//
// Ownership: observe actual per-case B-rep construction/destruction notifications.
// Each distinct live allocation is reclaimed once, including private native
// working B-reps abandoned on early errors. The executive is destroyed FIRST;
// its m_pOffsetBrep remains exclusively executive-owned until then. The static
// shell entrypoint uses the explicit transfer-reporting overload. Source lifetime
// is independently observed, so retained inputs, aliases and consumed inputs
// cannot produce duplicate deletes or stale source snapshots. No address escapes.
// The upstream global API context is kept alive with its normal configuration;
// our scoped callback and doing-Boolean guard are restored before returning.
// The shared native-call guard excludes every other shim until that restoration
// and all native destruction complete (including exceptions and buffer retries).
#include "sc_shim.h"
#include "StdAfx.h"
#include <SmApiBrep.h>
#include <SmApiGeneral.h>
#include <SmApiPrimitives.h>
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
#include <SmPlane.h>
#include <SmRegion.h>
#include <SmShell.h>
#include <SmVertex.h>
#include <nurbs.h>
#include <array>
#include <cmath>
#include <memory>
#include <stdexcept>
#include <vector>

namespace {
constexpr int64_t max_selections = 1024;
constexpr int64_t max_input = 4096;
constexpr size_t max_output = 262144;
constexpr size_t no_allocation = static_cast<size_t>(-1);
struct Argument {};
struct Rejected { SmStatus status; };

void finite(double v) { if (!std::isfinite(v)) throw Argument{}; }
struct Reader {
    const double* data;
    int64_t size;
    int64_t pos = 0;
    double number() {
        if (pos >= size) throw Argument{};
        double v = data[pos++];
        finite(v);
        return v;
    }
    int64_t integer(int64_t lo, int64_t hi) {
        double v = number();
        if (v < (double)lo || v > (double)hi || std::floor(v) != v) throw Argument{};
        return (int64_t)v;
    }
    SmBoolean flag() { return integer(0, 1) ? TRUE : FALSE; }
};

void interval(double a, double b, bool strict) {
    if (a > b || (strict && a == b)) throw Argument{};
    finite(b-a);
}

struct Spec {
    int kind = 0;
    std::array<double, 15> v{};
    int64_t faces() const { return kind == 3 ? 1 : kind == 4 ? 6 : 0; }
};
Spec read_spec(Reader& rd) {
    Spec s;
    s.kind = (int)rd.integer(0, 4);
    int n = s.kind == 2 ? 10 : s.kind == 3 ? 15 : s.kind == 4 ? 6 : 0;
    for (int i=0; i<n; ++i) s.v[i] = rd.number();
    const auto& v = s.v;
    if (s.kind == 2) {
        interval(v[6], v[7], true);
        interval(v[8], v[9], false);
        if (v[8] < v[6] || v[9] > v[7]) throw Argument{};
        double squared = 0;
        for (int i=0; i<3; ++i) squared += (v[i+3]-v[i])*(v[i+3]-v[i]);
        finite(squared);
    } else if (s.kind == 3) {
        interval(v[11], v[13], true);
        interval(v[12], v[14], true);
        for (int begin : {3, 6}) {
            double squared = 0;
            for (int i=begin; i<begin+3; ++i) squared += v[i]*v[i];
            finite(squared);
        }
        finite(v[9]*v[11]); finite(v[9]*v[13]);
        finite(v[10]*v[12]); finite(v[10]*v[14]);
    } else if (s.kind == 4) {
        for (int i=0; i<3; ++i) { finite(v[i]+v[i+3]); finite(v[i+3]*v[i+3]); }
    }
    return s;
}

struct Selection { int origin; int64_t index; };
struct Program {
    int operation;
    double tolerance, distance, member_distance, member_tol;
    SmOffsetOperationType member_operation;
    SmBoolean inset, extend, merge, solid, skip;
    Spec source, foreign;
    std::vector<Selection> selections;
    bool member() const { return operation == 2 || operation == 3; }
    bool can_extend() const {
        // Called only after the whole DTO is parsed and every index validated.
        // Member NER and both API null checks precede extension arithmetic;
        // SmApiShellBrepFull also rejects foreign/null selectors before copying.
        // Use DTO origins, never traversal-index equality, to identify that path.
        if (source.kind == 0) return false;
        if (operation == 4)
            for (const auto& selection : selections) if (selection.origin != 0) return false;
        // Keep the conservative guard for every other non-null fixture,
        // including empty/wire B-reps; no additional unreachability is assumed.
        return operation == 3 || extend != FALSE;
    }
};
Program read_program(Reader& rd) {
    Program p;
    p.operation = (int)rd.integer(0, 5);
    p.tolerance = rd.number();
    p.distance = rd.number();
    p.member_distance = rd.number();
    p.member_tol = rd.number();
    p.member_operation = (SmOffsetOperationType)rd.integer(0, 4);
    p.inset = rd.flag(); p.extend = rd.flag(); p.merge = rd.flag();
    p.solid = rd.flag(); p.skip = rd.flag();
    if (p.member() && p.member_distance < 0.0) throw Argument{};
    p.source = read_spec(rd);
    p.foreign = read_spec(rd);
    if (p.source.kind == 0 && (p.operation == 0 || p.operation == 1)) throw Argument{};
    int64_t count = rd.integer(0, max_selections);
    // Parse everything before allocating native geometry. No input-sized reserve.
    for (int64_t i=0; i<count; ++i) {
        int origin = (int)rd.integer(0, 2);
        int64_t index = rd.integer(-1, 6);
        if (origin == 2) { if (index != -1) throw Argument{}; }
        else if (index < 0 || index >= (origin == 0 ? p.source.faces() : p.foreign.faces()))
            throw Argument{};
        p.selections.push_back({origin, index});
    }
    if (rd.pos != rd.size) throw Argument{};
    // All scalars remain finite and all selectors remain bounds-checked, even
    // for proven early returns. Only the unused fivefold product is exempted.
    if (p.can_extend()) finite((p.member() ? p.member_distance : std::fabs(p.distance))*5.0);
    return p;
}

// SmBrep emits construction only after its topology lists are initialized, and
// destruction before deleting those lists (SmBrep.cpp:576,682,707). This observer
// never changes topology. It catches otherwise inaccessible working allocations
// on native early returns as well as the public result. It is scoped to this
// single synchronous call; system callbacks and the existing user callback stay
// intact. Tokens, not pointer equality, distinguish address reuse after deletion.
class Allocations : public SmNotifyCallback {
    struct Entry { SmBrep* ptr; bool live; };
    SmContext& context;
    SmNotifyCallback* previous;
    std::vector<Entry> entries;
public:
    explicit Allocations(SmContext& ctx) : context(ctx), previous(ctx.GetUserNotifyCallback()) {
        entries.reserve(64); // fixed adapter bookkeeping, not an input-declared size
        context.SetUserNotifyCallback(this);
    }
    void finish() {
        // All executive instances must already have been destroyed. Continue
        // reclaiming distinct allocations even if a native destructor throws;
        // normal-path cleanup reports that as SC_ERR_EXCEPTION at the boundary.
        bool failed = false;
        for (size_t i=entries.size(); i>0; --i) {
            if (entries[i-1].live) {
                SmBrep* ptr = entries[i-1].ptr;
                entries[i-1].live = false;
                try { delete ptr; } catch (...) { failed = true; }
            }
        }
        context.SetUserNotifyCallback(previous);
        if (failed) throw std::runtime_error("native B-rep destruction threw");
    }
    ~Allocations() override {
        // Never throw a second exception while unwinding a native operation.
        try { finish(); } catch (...) { context.SetUserNotifyCallback(previous); }
    }
    SmStatus Execute(SmNotifyOperation action, SmObject* object, SmObject* a,
                     SmObject* b, SmObject* c) override {
        if (action == SM_NO_CONSTRUCTION && object && object->IsKindOf(SmBrep_TYPE))
            entries.push_back({static_cast<SmBrep*>(object), true});
        else if (action == SM_NO_DESTRUCTION) {
            for (auto& entry : entries) if (entry.live && entry.ptr == object) entry.live = false;
        }
        return previous ? previous->Execute(action, object, a, b, c) : SM_SUCCESS;
    }
    size_t token(const SmBrep* ptr) const {
        if (!ptr) return no_allocation;
        for (size_t i=entries.size(); i>0; --i)
            if (entries[i-1].live && entries[i-1].ptr == ptr) return i-1;
        throw std::runtime_error("unobserved native B-rep allocation");
    }
    bool live(size_t token) const { return token != no_allocation && entries.at(token).live; }
};

struct BooleanRestore {
    SmContext& context;
    SmBoolean previous;
    explicit BooleanRestore(SmContext& ctx) : context(ctx), previous(ctx.GetDoingBoolean()) {}
    ~BooleanRestore() { context.GetDoingBooleanRef() = previous; }
};

void constructed(SmStatus status) {
    if (status != SM_SUCCESS) throw Rejected{status};
}
template<class T> struct PendingGeometry {
    T* ptr;
    ~PendingGeometry() { if (ptr && !ptr->GetOwner()) delete ptr; }
};
SmPoint3d point(const std::array<double, 15>& v, size_t i) {
    return SmPoint3d(v[i], v[i+1], v[i+2]);
}
SmBrep* build(const Spec& s, SmContext& ctx, double tolerance) {
    if (!s.kind) return nullptr;
    const auto& v = s.v;
    SmBrep* result = nullptr;
    if (s.kind == 4) {
        constructed(SmApiCreateBox(point(v, 0), v[3], v[4], v[5], result));
        if (!result) throw std::runtime_error("successful box constructor returned null");
        // CreateBox ignores CreateFaceFromSurface statuses (SmPrimitiveCreation.cpp:
        // 294-344), so SM_SUCCESS alone does not establish a constructed box.
        SmTArray<SmFace*> faces;
        SmTArray<SmEdge*> edges;
        SmTArray<SmVertex*> vertices;
        result->GetFaces(faces);
        result->GetEdges(edges);
        result->GetVertices(vertices);
        // Tiny / far-translated boxes come back as an EMPTY B-rep (0 faces): a
        // defined native result, not a crash, so it is built and compared as is.
        result->SetTolerance(tolerance, FALSE);
        return result;
    }
    result = new (ctx) SmBrep();
    result->SetTolerance(tolerance, FALSE);
    if (s.kind == 2) {
        SmExtent1d domain(v[6], v[7]), trim(v[8], v[9]);
        SmLine* line = nullptr;
        SmStatus status = SmLine::CreateLineSegment(ctx, 3, point(v, 0), point(v, 3), line, &domain);
        PendingGeometry<SmCurve> pending{line};
        constructed(status);
        if (!line) throw std::runtime_error("successful line constructor returned null");
        SmPoint3d a(0,0,0), b(0,0,0);
        constructed(line->EvaluatePoint(trim.GetMin(), a));
        constructed(line->EvaluatePoint(trim.GetMax(), b));
        SmShell* shell = nullptr;
        SmVertex *first = nullptr, *second = nullptr;
        constructed(result->MakeShellVertex(nullptr, a, shell, first));
        constructed(result->MakeShellVertex(nullptr, b, shell, second));
        SmEdge* edge = nullptr;
        // MakeWireEdge may attach the curve before returning an error. The
        // pending guard only deletes it if it still has no native topology owner.
        constructed(result->MakeWireEdge(nullptr, first, second, line, trim, SM_OT_SAME, edge));
        if (!edge) throw std::runtime_error("successful wire constructor returned null");
    } else if (s.kind == 3) {
        SmExtent2d domain(SmPoint2d(v[11],v[12]), SmPoint2d(v[13],v[14]));
        PendingGeometry<SmSurface> pending{
            new (ctx) SmPlane(point(v,0), point(v,3), point(v,6), SmVector2d(v[9],v[10]), domain, &ctx)
        };
        SmFace* face = nullptr;
        constructed(result->CreateFaceFromSurface(pending.ptr, domain, face));
        // CreateFaceFromSurface may succeed after tolerance-driven boundary
        // merges/removal, leaving a degenerate face; that is a defined native
        // result and is built and compared as is.
        if (!face) throw std::runtime_error("successful sheet constructor returned null");
    }
    return result;
}

// Same safety preflight as p1: public native extraction/normalization checks,
// not a replacement for extension. NL_CrvAdv's zero-tangent fallback reads a
// third control point even when its isocurve has only two. SmPlane derives from
// SmBSplineSurface, so real analytic sheets and box faces require this check too.
void guard_two_point_isocurves(SmSurface* base) {
    SmBSplineSurface* spline = SM_CAST_PTR(SmBSplineSurface, base);
    if (!spline) return;
    gw_SURFACE* surface = spline->GetOrCreateGwNurbPointer();
    if (!surface) throw Argument{};
    for (int direction=0; direction<2; ++direction) {
        if ((direction == 0 ? surface->net->n : surface->net->m) != 1) continue;
        gw_KNOTVECTOR* knots = direction == 0 ? surface->knv : surface->knu;
        for (double fraction : {0.07, 0.19, 0.45, 0.73, 0.92}) {
            double parameter = (1.0-fraction)*knots->U[0] + fraction*knots->U[knots->m];
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

// Only access to protected flags for observation. No behavior override.
class Executive : public SmOffsetExecutive {
public:
    Executive(SmContext& ctx, SmOffsetOperationType op, SmOffsetGeometryCreation& geometry,
              SmBrep* source) : SmOffsetExecutive(ctx, op, geometry, source) {}
    bool merge() const { return m_bMergeResults != FALSE; }
    bool solid() const { return m_bCreateOffsetSolid != FALSE; }
};

template<class T> double id(const SmTArray<T*>& traversal, const T* ptr) {
    if (!ptr) return -1;
    for (ULONG i=0; i<traversal.GetSize(); ++i) if (traversal[i] == ptr) return (double)i;
    return -2;
}
void put_point(std::vector<double>& out, const SmPoint3d& p) {
    out.insert(out.end(), {p.x,p.y,p.z});
}
void put_domain(std::vector<double>& out, const SmExtent2d& d) {
    out.insert(out.end(), {d.GetMin().x,d.GetMin().y,d.GetMax().x,d.GetMax().y});
}
int surface_kind(SmSurface* surface) {
    return !surface ? 0 : surface->IsKindOf(SmPlane_TYPE) ? 1
        : surface->IsKindOf(SmBSplineSurface_TYPE) ? 2 : 3;
}
int curve_kind(SmCurve* curve) {
    return !curve ? 0 : curve->IsKindOf(SmLine_TYPE) ? 1
        : curve->IsKindOf(SmBSplineCurve_TYPE) ? 2 : 3;
}
struct Block {
    bool present = false;
    std::vector<double> integers, values;
};
template<class T> void put_ids(std::vector<double>& out, const SmTArray<T*>& list,
                              const SmTArray<T*>& traversal) {
    out.push_back((double)list.GetSize());
    for (ULONG i=0; i<list.GetSize(); ++i) out.push_back(id(traversal, list[i]));
}
Block snapshot(SmBrep* brep) {
    Block block;
    if (!brep) return block;
    block.present = true;
    auto& ints = block.integers;
    auto& vals = block.values;
    SmTArray<SmRegion*> regions;
    SmTArray<SmShell*> shells;
    SmTArray<SmFace*> faces;
    SmTArray<SmLoop*> loops;
    SmTArray<SmEdge*> edges;
    SmTArray<SmEdgeuse*> edgeuses;
    SmTArray<SmVertex*> vertices;
    brep->GetRegions(regions); brep->GetShells(shells); brep->GetFaces(faces);
    brep->GetLoops(loops); brep->GetEdges(edges); brep->GetEdgeuses(edgeuses);
    brep->GetVertices(vertices);
    size_t total = (size_t)regions.GetSize() + shells.GetSize() + faces.GetSize()
                 + loops.GetSize() + edges.GetSize() + edgeuses.GetSize() + vertices.GetSize();
    if (total > max_output/64) throw std::length_error("native snapshot topology budget");
    ints.insert(ints.end(), {
        (double)regions.GetSize(), (double)shells.GetSize(), (double)faces.GetSize(),
        (double)loops.GetSize(), (double)edges.GetSize(), (double)edgeuses.GetSize(),
        (double)vertices.GetSize(), id(regions,brep->GetInfiniteRegion()),
        brep->m_bEditingEnabled ? 1.0 : 0.0, brep->m_bValidateOnly ? 1.0 : 0.0
    });
    vals.push_back((double)brep->GetTolerance());
    for (ULONG i=0; i<regions.GetSize(); ++i)
        ints.insert(ints.end(), {(double)i, regions[i]->IsVoid() ? 1.0 : 0.0});
    for (ULONG i=0; i<shells.GetSize(); ++i)
        ints.insert(ints.end(), {(double)i, id(regions,shells[i]->GetRegion())});
    for (ULONG i=0; i<faces.GetSize(); ++i) {
        SmFace* face = faces[i];
        SmSurface* surface = face->GetSurface();
        SmFaceuse* up = face->GetUpwardFaceuse();
        SmFaceuse* mate = up ? up->GetMate() : nullptr;
        ints.insert(ints.end(), {(double)i, (double)surface_kind(surface),
            id(shells,up ? up->GetShell() : nullptr), id(shells,mate ? mate->GetShell() : nullptr),
            up ? (double)up->GetOrientation() : -1.0, mate ? (double)mate->GetOrientation() : -1.0});
        SmTArray<SmLoop*> face_loops;
        face->GetLoops(face_loops);
        put_ids(ints,face_loops,loops);
        vals.push_back((double)face->GetTolerance());
        SmExtent2d domain = face->GetUVDomain();
        put_domain(vals,domain);
        if (surface) put_domain(vals,surface->GetNaturalUVDomain());
        else vals.insert(vals.end(),4,0.0);
        SmPoint2d low = domain.GetMin(), high = domain.GetMax();
        for (double fraction : {0.0,0.5,1.0}) {
            SmPoint2d uv((1.0-fraction)*low.x + fraction*high.x,
                         (1.0-fraction)*low.y + fraction*high.y);
            SmPoint3d p(0,0,0);
            SmVector3d normal(0,0,0);
            // Nonfinite returned domains are observable, but must not become
            // unchecked NLib span indices. -1 means sampling was not attempted.
            SmStatus ps = (SmStatus)-1, ns = (SmStatus)-1;
            if (surface && std::isfinite(uv.x) && std::isfinite(uv.y)) {
                ps = surface->EvaluatePoint(uv,p);
                ns = surface->EvaluateNormal(uv,TRUE,TRUE,normal);
            }
            ints.insert(ints.end(), {(double)ps,(double)ns});
            put_point(vals,p); put_point(vals,normal);
        }
    }
    for (ULONG i=0; i<loops.GetSize(); ++i) {
        ints.insert(ints.end(), {(double)i,id(faces,loops[i]->GetFace())});
        SmTArray<SmEdgeuse*> loop_edgeuses;
        loops[i]->GetEdgeuses(loop_edgeuses);
        put_ids(ints,loop_edgeuses,edgeuses);
    }
    for (ULONG i=0; i<edges.GetSize(); ++i) {
        SmEdge* edge = edges[i];
        SmCurve* curve = edge->GetCurve();
        ints.insert(ints.end(), {(double)i,id(vertices,edge->GetStartVertex()),
            id(vertices,edge->GetEndVertex()),(double)curve_kind(curve)});
        SmTArray<SmEdgeuse*> uses;
        edge->GetEdgeuses(uses);
        put_ids(ints,uses,edgeuses);
        SmExtent1d trim = edge->GetInterval();
        vals.insert(vals.end(), {(double)edge->GetTolerance(),trim.GetMin(),trim.GetMax()});
        if (curve) {
            SmExtent1d domain = curve->GetNaturalInterval();
            vals.insert(vals.end(), {domain.GetMin(),domain.GetMax()});
        } else vals.insert(vals.end(),2,0.0);
        for (double t : {trim.GetMin(),trim.GetMax()}) {
            SmPoint3d p(0,0,0);
            SmStatus status = (SmStatus)-1;
            if (curve && std::isfinite(t)) status = curve->EvaluatePoint(t,p);
            ints.push_back((double)status);
            put_point(vals,p);
        }
    }
    for (ULONG i=0; i<edgeuses.GetSize(); ++i) {
        SmEdgeuse* eu = edgeuses[i];
        ints.insert(ints.end(), {(double)i,id(edges,eu->GetEdge()),id(faces,eu->GetFace()),
            id(shells,eu->GetShell()),id(edgeuses,eu->GetMate()),id(edgeuses,eu->GetRadial()),
            (double)eu->GetOrientation()});
    }
    for (ULONG i=0; i<vertices.GetSize(); ++i) {
        ints.push_back((double)i);
        put_point(vals,vertices[i]->GetPoint());
        vals.push_back((double)vertices[i]->GetTolerance());
    }
    return block;
}
void put_block(std::vector<double>& out, const Block& block) {
    out.push_back(block.present ? 1.0 : 0.0);
    if (!block.present) return;
    if (block.integers.size() + block.values.size() + out.size() + 2 > max_output)
        throw std::length_error("native snapshot output budget");
    out.push_back((double)block.integers.size());
    out.insert(out.end(),block.integers.begin(),block.integers.end());
    out.push_back((double)block.values.size());
    out.insert(out.end(),block.values.begin(),block.values.end());
}

std::vector<double> observe_selectors(const SmTArray<SmFace*>& selectors, SmBrep* source,
                                     SmBrep* foreign, const std::vector<double>* before = nullptr) {
    SmTArray<SmFace*> sources, foreigners;
    if (source) source->GetFaces(sources);
    if (foreign) foreign->GetFaces(foreigners);
    std::vector<double> result;
    for (ULONG i=0; i<selectors.GetSize(); ++i) {
        if (before && !source && before->at((size_t)i*2) == 0.0) {
            result.insert(result.end(), {3.0,before->at((size_t)i*2+1)});
            continue; // Do not even compare the consumed source face pointer.
        }
        SmFace* face = selectors[i];
        double src = id(sources,face), other = id(foreigners,face);
        if (!face) result.insert(result.end(), {2.0,-1.0});
        else if (src >= 0) result.insert(result.end(), {0.0,src});
        else if (other >= 0) result.insert(result.end(), {1.0,other});
        else result.insert(result.end(), {4.0,-1.0});
    }
    return result;
}

void run(const Program& p, std::vector<double>& out) {
    SmContext& ctx = *SmApiGetOrCreateContext();
    BooleanRestore restore(ctx); // outlives all per-case B-rep destruction
    Allocations allocations(ctx);
    SmBrep* source = build(p.source,ctx,p.tolerance);
    SmBrep* foreign = build(p.foreign,ctx,p.tolerance);
    size_t source_token = allocations.token(source), foreign_token = allocations.token(foreign);
    SmTArray<SmFace*> source_faces, foreign_faces, selectors;
    SmTArray<const SmFace*> api_selectors;
    if (source) source->GetFaces(source_faces);
    if (foreign) foreign->GetFaces(foreign_faces);
    for (const auto& selection : p.selections) {
        SmFace* face = nullptr;
        if (selection.origin != 2) {
            const auto& list = selection.origin == 0 ? source_faces : foreign_faces;
            // Expected construction counts are not permission to index an
            // unexpectedly different native topology. This is a layout error,
            // never a fabricated constructor rejection or native operation status.
            // A constructor can legitimately leave fewer faces than the DTO
            // expects (an empty tiny box): there is then no face to select and
            // nothing to run on either side, which is a fixture rejection.
            if (selection.index < 0 || (ULONG)selection.index >= list.GetSize())
                throw Rejected{SM_ERR};
            face = list[(ULONG)selection.index];
        }
        selectors.Add(face);
        api_selectors.Add(face);
    }
    // Do not cross API selection validation merely to perform a geometry safety
    // preflight: invalid API selectors must reach the actual early API return.
    if (source && p.can_extend()) {
        for (ULONG i=0; i<source_faces.GetSize(); ++i)
            guard_two_point_isocurves(source_faces[i]->GetSurface());
    }
    Block source_before = snapshot(source), foreign_before = snapshot(foreign);
    auto selected_before = observe_selectors(selectors,source,foreign);
    SmOffsetGeometryCreation geometry(p.member_distance,p.member_tol);
    std::unique_ptr<Executive> executive;
    if (p.member()) {
        executive.reset(new Executive(ctx,p.member_operation,geometry,source));
        executive->SetInset(p.inset);
        executive->SetExtendConvexEdges(p.extend);
        executive->SetMergeResults(p.merge);
        executive->SetCreateOffsetSolid(p.solid);
        executive->SetShellFaces(selectors);
    }
    SmBrep* result = nullptr;
    SmBoolean transferred = FALSE;
    SmBoolean boolean_before = ctx.GetDoingBoolean();
    SmStatus status = SM_ERR;
    switch (p.operation) {
    case 0:
        status = SmOffsetExecutive::OffsetBrep(ctx,source,p.distance,p.extend,p.merge,result);
        break;
    case 1:
        status = SmOffsetExecutive::ShellBrep(ctx,source,p.distance,p.extend,p.merge,p.solid,
                                             selectors,result,transferred);
        break;
    case 2: status = executive->DoSolidOffset(result,p.skip); break;
    case 3: status = executive->DoExtendedOffset(result); break;
    case 4: status = SmApiShellBrepFull(source,p.distance,p.extend,p.merge,p.solid,api_selectors,result); break;
    case 5: status = SmApiOffsetBrepFull(source,p.distance,p.extend,p.merge,result); break;
    }
    SmBoolean boolean_after = ctx.GetDoingBoolean();
    bool source_live = allocations.live(source_token), foreign_live = allocations.live(foreign_token);
    // Verify a non-null result belongs to a live observed allocation BEFORE any
    // dereference. Native errors remain built-case statuses even with a result.
    allocations.token(result);
    SmBrep* offset = executive ? executive->GetOffsetBrep() : nullptr;
    allocations.token(offset);
    int lifetime = !source ? 0 : !source_live ? 3 : result == source ? 2 : 1;
    int origin = !result ? 0 : source_live && result == source ? 2 : result == offset ? 3 : 1;
    // The explicit transfer report controls ownership interpretation, while the
    // observer covers actual deletion and also cleans allocations on exceptions.
    if (p.operation == 1 && transferred && source_live && result != source)
        throw std::runtime_error("native shell ownership report contradicts source lifetime");
    out.insert(out.end(), {0,1,(double)p.operation,(double)status,transferred ? 1.0 : 0.0,
        (double)lifetime,(double)origin,boolean_before ? 1.0 : 0.0,boolean_after ? 1.0 : 0.0,
        executive ? 1.0 : 0.0});
    if (executive) {
        out.insert(out.end(), {(double)executive->GetOffsetOperation(),
            executive->GetInset() ? 1.0 : 0.0,executive->GetExtendConvexEdges() ? 1.0 : 0.0,
            executive->merge() ? 1.0 : 0.0,executive->solid() ? 1.0 : 0.0,
            geometry.GetOffsetDistance(),geometry.GetThisApproxTol3d(),executive->GetTangencyTolDegrees()});
    }
    out.push_back((double)selectors.GetSize());
    out.insert(out.end(),selected_before.begin(),selected_before.end());
    auto selected_after = observe_selectors(selectors,source_live ? source : nullptr,
                                            foreign_live ? foreign : nullptr,&selected_before);
    out.insert(out.end(),selected_after.begin(),selected_after.end());
    put_block(out,source_before);
    put_block(out,snapshot(source_live ? source : nullptr));
    put_block(out,foreign_before);
    put_block(out,snapshot(foreign_live ? foreign : nullptr));
    put_block(out,snapshot(result));
    put_block(out,snapshot(offset));
    // Executive destruction owns offset; then allocation scope reclaims every
    // remaining distinct B-rep (source/foreign/result/unreturned native working data).
    executive.reset();
    allocations.finish();
}
} // namespace

SC_EXPORT int32_t sc_offset_offset_executive_p2_eval(
    const double* input, int64_t input_len, double* out, int64_t cap, int64_t* out_len) {
    if (!input || input_len < 0 || input_len > max_input || !out_len || cap < 0 || (cap && !out))
        return SC_ERR_ARG;
    try {
        SC_NATIVE_LOCK;
        Reader rd{input,input_len};
        Program p = read_program(rd);
        std::vector<double> result;
        try { run(p,result); }
        catch (const Rejected& rejected) { result = {-1,(double)rejected.status}; }
        if (result.size() > max_output) throw std::length_error("native output budget");
        return sc_copy_out(result.data(),(int64_t)result.size(),out,cap,out_len);
    } catch (const Argument&) {
        return SC_ERR_ARG;
    } catch (...) {
        return SC_ERR_EXCEPTION;
    }
}
