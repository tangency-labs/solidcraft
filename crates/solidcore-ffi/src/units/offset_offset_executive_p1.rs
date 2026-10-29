//! Native fixtures for `offset.offset_executive_p1` (PR #175).
//!
//! Upstream: `source/SMLib/inc/SmOffsetExecutive.h`,
//! `source/SMLib/src/SmOffsetExecutive.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`config`] | constructor/destructor, `Get/SetInset`, `Get/SetExtendConvexEdges`, `SetMergeResults`, `SetCreateOffsetSolid`, `GetTangencyTolDegrees`, `GetOffsetBrep`, `Get/SetShellFaces`, `GetOffsetEdgeuse` |
//! | [`synchronize`] | `SyncronizeOffsetEdgeuses`, indirectly `sm_RemoveSmallStuff`, `sm_InsertBreaks`, `sm_SortEdgeuses`, `sm_ExtractGaps` |
//! | [`extend`] | `GetCreateExtendedSurface` |
//! | [`vertex`] | `BuildOffsetOfVertex`, including both real `VertexOffset` callsites |
//! | [`face_tail`] | `BuildOffsetOfFaceuse` (contained-domain branch), `MapEdgeuses`, `MakeFaceWithCurves` (fixture) |
//! | [`map_twin`] | `MapEdgeuses` alone, between two `MakeFaceWithCurves` rectangle faces |
//! | [`replace`] | `ReplaceImplicitOffsets` on one face whose surface is an owning `SmOffsetSurface` of a real `SmPlane`, `SmBSplineSurface` or `SmSurfOfExtrusion` |
//! | [`probe`] | `ReplaceImplicitOffsets`, `RemoveExcessFaces`, `BuildSideWallOfEdgeuse`, `BuildOffsetOfEdgeuses`, `FillGapEdgeuses`, `BuildOffsetOfVertex`, `BuildOffsetOfFaceuse`, `PiecewiseMerge`, `MapEdgeuses`; `sm_NotifyEdgeAndVertices` indirectly via `MapEdgeuses` |
//!
//! Deviations: caller-owned flat snapshots instead of pointers/out-parameters;
//! bounded fixture sizes, and validated NURBS storage before entering C++.
//! Extension rejects negative internal distance magnitudes in both the safe packer
//! and the direct native ABI before constructing geometry (signed zero is allowed).
//! A native preflight also rejects two-control-point extension isocurves whose
//! tangent cannot be normalized: upstream's fallback would access a third point
//! outside the polygon (`NL_CrvAdv.cpp`). This returns a shim argument error,
//! never a fixture rejection or a fabricated operation status.
//! No private static helper is copied. Split cases execute native topology splitting;
//! the runner must defer them if its Rust kernel cannot yet split edges.
//!
//! # Geometry constructor views
//!
//! `config` accepts [`Config::tangency_tol_radians`] after `offset_distance` and
//! `approx_tol`, before `operation` in its packed input. Pass `2.0 * PI / 180.0`
//! for the default geometry-constructor view, or any finite radians value for a
//! custom geometry-subclass view. A fixture-only subclass first runs the native
//! default geometry constructor, then assigns its protected tangency scalar;
//! `GetTangencyTolDegrees` remains the actual inherited native getter.
//! Both `default_state` and `final_state` below observe that supplied geometry:
//! "default" refers to executive flags before setters, not a reset of tangency.
//! Other modes keep the unmodified native default geometry constructor.
//!
//! # Offset distance contract
//!
//! Public offset/shell distances are signed, but `GetCreateExtendedSurface` uses
//! an internal **nonnegative magnitude**, multiplied by five. The entrypoints in
//! `source/SMLib/src/SmOffsetExecutive.cpp:114-120` and `:206-223` construct
//! `SmOffsetGeometryCreation` with `fabs(distance)` and carry the sign separately
//! through `SetInset`. The local-operation caller in
//! `source/SMLib/src/SmBrep.cpp:12004` supplies bounding-sphere radius / 2 instead.
//! Accordingly, [`Extension::offset_distance`] rejects only values `< 0.0` in
//! addition to the existing finite-value checks (including the product by five):
//! both signed zeros, positive subnormals and large finite magnitudes remain valid.
//! Control point coordinates are bounded by [`MAX_COORDINATE`]: beyond it the native
//! extension of a B-spline surface does not return (fuzz case `fuzz00331`).
//! [`Config`] only observes constructor/executive state and never extends geometry;
//! its finite scalar values, including negative distances/tolerances, are preserved.
//!
//! # Snapshot protocol (all entries are `f64`)
//!
//! Every snapshot starts with `0` (fixture built), or consists of `[-1, status]`
//! **only** when a fixture constructor rejected it. Operation failures are native
//! status values inside a built-fixture snapshot, never fixture rejections.
//! All native statuses are **unmodified**: [`NATIVE_SUCCESS`] is 1000, not 0;
//! null-pointer errors are [`NATIVE_NULL_POINTER`] (1006). The leading 0 is an
//! adapter fixture tag, not a native operation status.
//! IDs are zero-based indices in native `GetEdgeuses` / `GetEdges` / `GetFaces`
//! traversal, not addresses. `-1` is null, `-2` is a non-null pointer absent from
//! that traversal. A counted list is `length, entries...`.
//!
//! * `config`: `0, default_state[10], final_state[10], number_shell_sets`, then
//!   one counted face-ID list per shell setter, then `number_original_edgeuses`;
//!   for each original EU: `GetOffsetEdgeuse_id, counted_related_EU_IDs`;
//!   finally a counted list of `RelatePair` statuses (duplicates are passed through).
//!   State order: `offset_distance, approx_tol, operation, inset, extend_convex,
//!   merge_results, create_solid, tangency_degrees, has_offset_brep, shell_count`.
//! * `synchronize`: `0, operation_status, lookup_first, lookup_second`, counted
//!   fixture-primary-EU IDs (first fixtures then second), counted pre-operation
//!   relation IDs for first and second, `number_final_edgeuses`, one record per
//!   final EU, then four counted EU-ID lists: matched first, matched second,
//!   first gaps, second gaps. Lookups/relation IDs use the **pre-operation**
//!   traversal; fixture IDs and final lists use the **post-operation** traversal.
//!   Synchronization fixtures have real two-edge loops so native splitting has
//!   valid CW/CCW neighbors. Auxiliary closing edges are excluded from measured
//!   edge/EU traversals; all measured split descendants remain in the snapshot.
//!   No UV trims are requested, so arbitrary 3D B-splines remain admissible.
//!   Exactly coincident measured endpoints need a nonzero closed cubic support
//!   instead of `SmBSplineCurve::CreateLineSegment` (which rejects such endpoints).
//!   Its native constructor takes four unit-weight points: end, (1,0,0), (0,1,0),
//!   start, with knots [0,0,0,0,1,1,1,1]. This changes only unmeasured scaffolding;
//!   zero trimming intervals and constant measured B-splines are not substituted.
//!   Coincident analytic `SmLine::CreateLineSegment` inputs still reach the real
//!   constructor rejection, recorded separately from parity.
//!   Each final EU record is `eu_id, edge_id, min, max, ApproximateLength(10),
//!   start_status, start_xyz[3], end_status, end_xyz[3]` (13 entries).
//! * `extend`: `0, action_count`; Get records are `0, status, was_cached,
//!   result_id, has_result`. On success append `surface_kind, domain[4],
//!   sample_count` and samples `point_status, xyz[3], normal_status, normal[3]`.
//!   Domain order is `umin, vmin, umax, vmax`; kind is 1 Plane, 2 B-spline,
//!   3 other. Result IDs are assigned on first successful encounter, never reused
//!   after eviction. Forget records: `1, was_cached, remaining_cache_count`;
//!   Clear records: `2, previous_cache_count, 0`.
//! * `vertex` (ABI mode 6; modes 4/5 are `face_tail`/`map_twin`): input `point[3], distance, approx_tol, original_tol,
//!   result_tol, has_strut`, optional `radius, height, map_mate`, then counted UV
//!   fractions. A strut is made with `MakeVertexLoop`/`MakeEdgeInFace` on a real
//!   planar face; its mapped result face is a planar disk with one closed rational
//!   B-spline circular edge (never a cached/prebuilt cap). Output: `0, status`,
//!   effective original/result tolerances, `curve_count, orientation_count,
//!   has_surface`, orientations, then each trim's `type, interval[2]`, counted
//!   samples `point_status, xyz[3]`. If present the cap is `type, domain[4],
//!   has_nurb`; a NURB adds `n,m,p,q,r,s`, homogeneous net in U-major/V-minor
//!   order, all U knots and all V knots. Finally counted cap samples contain
//!   `Evaluate2ndDerivatives_status, [P,Du,Dv,Duv,Duu,Dvv][18]` (TRUE/TRUE sides).
//!   All type codes/statuses are native, all arithmetic uses the runner's normal
//!   1e-9 relative comparator with no absolute floors. The unchanged `probe`
//!   isolated-vertex fixture remains a counts-only smoke test.
//! * `probe`: `0, status, curve_count, orientation_count, has_surface,
//!   has_result_brep, has_offset_brep, counted_original_to_offset_EU_lookups`.
//! * `face_tail`: `0, status, related_face_count`, per related face
//!   `uv_domain[4], edge_count` (sorted by domain), then the offset B-rep's
//!   `face_count, edge_count, vertex_count`, its vertex points `xyz[3]` sorted
//!   lexicographically, and `number_original_edgeuses` (the measured face use's
//!   `GetEdgeuses`) with per use, sorted by the use's own `start_xyz[3]` (first in
//!   the record), `-1` when `GetOffsetEdgeuse` is null, else
//!   `1, start_xyz[3], mate_start_xyz[3]` of the offset use.
//! * `map_twin`: `0, status, number_original_edgeuses` and the same per-use
//!   records as `face_tail`.
//! * `replace`: `0, status, face_count`, per face `surface_type, face_uv_domain[4]`
//!   and, with a surface, its `natural_uv_domain[4]` and counted samples (fractions
//!   of that domain) `point_status, xyz[3]`.
//!
//! **Relation ordering:** this checkout uses pointer-hashed `unordered_set`
//! values in `SmRelation`. We preserve actual `GetSeconds` order; equal-minimum
//! sort ties and summation order can therefore be upstream-nondeterministic.
//! Do not mistake this for insertion order. Any equal-minimum canonicalization
//! belongs to comparison in the runner; this raw C++ protocol never reorders arrays.

use crate::{call_with_buffer, FfiError};

/// Fixture resource bound on edges, faces, surfaces, actions and relation pairs.
/// These are adapter budgets, not limits asserted about the kernel API.
pub const MAX_ITEMS: usize = 4096;
/// Maximum number of homogeneous control points in one fixture.
pub const MAX_CONTROL_POINTS: usize = 65_536;
/// Largest control point coordinate magnitude (also after dividing by the weight).
/// HANG-37: native `CreateExtendedSurface` on a B-spline surface with one control
/// point of magnitude `1e157` or more gives no result within 25 s, on x, y and z
/// alike (`1e154`, `1e155` and `1e156` return in about 30 ms each; `1e157`, `1e158`,
/// `1e160`, `1e200`, `1e300` do not). The bound sits 1000x below the first slow
/// value; each coordinate's own square also still stays finite there (a 3-term norm
/// would not, so that is not the argument).
pub const MAX_COORDINATE: f64 = 1e154;
/// Native `NL_MAXDEG` (`NL_Nurbsdef.h`); larger degrees overrun basis work arrays.
pub const MAX_DEGREE: usize = 17;
/// Maximum packed input length, bounding aggregate native fixture allocations.
pub const MAX_PACKED_VALUES: usize = 1_048_576;
/// Aggregate synchronization-edge budget, bounding the quadratic split expansion.
pub const MAX_SYNC_EDGES: usize = 128;
/// Unmodified upstream `SM_SUCCESS` (distinct from the shim ABI's `SC_OK = 0`).
pub const NATIVE_SUCCESS: i32 = 1000;
/// Unmodified upstream `SM_ERR_NULL_POINTER` returned by native NER checks.
pub const NATIVE_NULL_POINTER: i32 = 1006;

/// Constructor settings and protected state exercised by [`config`].
// Independent native setters are boolean toggles, not states of one machine.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    /// Raw finite geometry-constructor scalar, preserved even when negative.
    /// This state-only fixture does not extend geometry; real offset/shell callers
    /// pass `fabs(signed_distance)` and carry the sign in [`Self::inset`] instead.
    /// See the module-level offset distance contract and upstream callsites.
    pub offset_distance: f64,
    /// Finite approximation tolerance, including the native default sentinel.
    pub approx_tol: f64,
    /// Geometry-subclass tangency scalar in radians; any finite value is allowed,
    /// including signed zero, negative, subnormal and large values. Use
    /// `2.0 * PI / 180.0` for the native default constructor's value.
    pub tangency_tol_radians: f64,
    /// Native operation enum, 0 solid, 1 NMT, 2 shell, 3 local, 4 machine.
    pub operation: u32,
    /// Value passed to `SetInset`, separately carrying public offset direction.
    pub inset: bool,
    /// Value passed to `SetExtendConvexEdges`.
    pub extend_convex: bool,
    /// Value passed to `SetMergeResults`.
    pub merge_results: bool,
    /// Value passed to `SetCreateOffsetSolid`.
    pub create_solid: bool,
    /// Install the fixture offset B-rep into the executive, transferring ownership.
    pub install_offset_brep: bool,
    /// Number of independent original wire edges (two edgeuses each).
    pub original_edges: usize,
    /// Number of independent offset wire edges (two edgeuses each).
    pub offset_edges: usize,
    /// Number of independent planar selector faces in a separate fixture B-rep.
    pub face_count: usize,
    /// Successive calls to `SetShellFaces`, using native face traversal IDs.
    /// Empty outer list leaves the constructor's null shell list untouched.
    pub shell_face_sets: Vec<Vec<usize>>,
    /// Original-EU/offset-EU traversal IDs; duplicates reach real `RelatePair`.
    pub relations: Vec<[usize; 2]>,
}

/// Geometry owned by an independent wire-edge fixture.
#[derive(Debug, Clone, PartialEq)]
pub enum CurveFixture {
    /// A real analytic `SmLine`, constructed with `SmLine::CreateLineSegment`.
    Line {
        /// Point at the lower domain bound.
        start: [f64; 3],
        /// Point at the upper domain bound; coincident endpoints reach native validation.
        end: [f64; 3],
        /// Strictly increasing natural parameter interval.
        domain: [f64; 2],
    },
    /// A real `SmBSplineCurve`, not a line stand-in.
    BSpline {
        /// Polynomial degree, 1 through [`MAX_DEGREE`].
        degree: usize,
        /// Homogeneous `(x*w, y*w, z*w, w)` control points, positive weights.
        control_points: Vec<[f64; 4]>,
        /// Nondecreasing knots, length `control_points.len() + degree + 1`.
        knots: Vec<f64>,
    },
}

/// One independent offset edge and its actual native trimming interval.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeFixture {
    /// Native curve to construct.
    pub curve: CurveFixture,
    /// Ordered subinterval of the curve's natural interval; zero width is allowed.
    pub interval: [f64; 2],
}

/// Arguments for [`synchronize`].
#[derive(Debug, Clone, PartialEq)]
pub struct Synchronize {
    /// Mapped offsets of the first original EU, in fixture construction order.
    pub first: Vec<EdgeFixture>,
    /// Mapped offsets of the other original EU.
    pub second: Vec<EdgeFixture>,
    /// Pass null as the first original EU: native NER returns its real error code.
    pub null_original: bool,
    /// Pass null as the other original EU; its otherwise supplied fixtures remain
    /// unmapped (the first list can still be nonempty).
    pub null_other: bool,
}

/// Surface geometry constructed independently of the Rust kernel's surface types.
#[derive(Debug, Clone, PartialEq)]
pub enum SurfaceFixture {
    /// Finite analytic plane with a real B-spline backing representation.
    Plane {
        /// Parametric origin.
        origin: [f64; 3],
        /// Native plane X axis (unitized by the constructor).
        x_axis: [f64; 3],
        /// Native plane Y axis (unitized by the constructor).
        y_axis: [f64; 3],
        /// Parameter scales; finite, zero and negative scales reach native code.
        scale: [f64; 2],
        /// `umin, vmin, umax, vmax` (positive extent in both directions).
        domain: [f64; 4],
    },
    /// Tensor-product B-spline surface in U-major control-net order.
    BSpline {
        /// U degree.
        u_degree: usize,
        /// V degree.
        v_degree: usize,
        /// Number of U control points, not the highest index.
        u_count: usize,
        /// Number of V control points, not the highest index.
        v_count: usize,
        /// Homogeneous points at `(u, v)`, indexed `u * v_count + v`.
        control_points: Vec<[f64; 4]>,
        /// U knots, length `u_count + u_degree + 1`.
        u_knots: Vec<f64>,
        /// V knots, length `v_count + v_degree + 1`.
        v_knots: Vec<f64>,
    },
}

/// Actions on the executive's actual extended-surface map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheAction {
    /// Call `GetCreateExtendedSurface`; `None` exercises the valid native NER error.
    Get(Option<usize>),
    /// Remove one map key without deleting any returned surface.
    Forget(usize),
    /// Clear map entries without deleting returned surfaces.
    Clear,
}

/// Arguments for [`extend`].
#[derive(Debug, Clone, PartialEq)]
pub struct Extension {
    /// Base surfaces, indexed by [`CacheAction`] keys.
    pub surfaces: Vec<SurfaceFixture>,
    /// Internal nonnegative magnitude, not a public signed offset distance.
    /// Both signed zeros are allowed; this value and its product by five must be
    /// finite. The native method extends by that product, with no sign conversion.
    /// See the module-level offset distance contract and upstream callsites.
    pub offset_distance: f64,
    /// Stored approximation tolerance (not used by this extension operation).
    pub approx_tol: f64,
    /// Repeated gets, eviction and clear operations, executed in order.
    pub actions: Vec<CacheAction>,
    /// Sample fractions in `[0, 1]^2` of each returned extended natural domain.
    pub samples: Vec<[f64; 2]>,
}

/// A real source strut and mapped circular result face for [`vertex`].
#[derive(Debug, Clone, PartialEq)]
pub struct VertexStrut {
    /// Radius of the closed B-spline edge on the result disk.
    pub radius: f64,
    /// Result disk center's Z displacement from the original vertex.
    pub height: f64,
    /// Map the source faceuse's mate, exercising the executive's fallback lookup.
    pub map_mate: bool,
}

/// Geometry-valued `BuildOffsetOfVertex` fixture on two independently toleranced B-reps.
#[derive(Debug, Clone, PartialEq)]
pub struct VertexFixture {
    /// Location of the original shell vertex or interior strut endpoint.
    pub point: [f64; 3],
    /// Geometry creator's offset distance, forwarded unchanged.
    pub offset_distance: f64,
    /// Geometry creator's approximation tolerance, forwarded unchanged.
    pub approx_tol: f64,
    /// Original B-rep tolerance (the creator reads this, not the result tolerance).
    pub original_tolerance: f64,
    /// Result B-rep tolerance (used by the executive's edge-selection predicates).
    pub result_tolerance: f64,
    /// Absent for an isolated original shell vertex, present for a real mapped strut.
    pub strut: Option<VertexStrut>,
    /// Normalized samples of the returned cap's natural domain, in `[0, 1]^2`.
    pub samples: Vec<[f64; 2]>,
}

/// Independent native topology paths, never implicitly mixed into arithmetic cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Probe {
    /// `ReplaceImplicitOffsets` on an empty B-rep.
    ReplaceEmpty = 0,
    /// `RemoveExcessFaces` on an empty B-rep, merging disabled.
    RemoveNoMerge = 1,
    /// `RemoveExcessFaces` on an empty B-rep, merging enabled.
    RemoveEmpty = 2,
    /// `BuildSideWallOfEdgeuse(NULL)`.
    SideWallNull = 3,
    /// `BuildSideWallOfEdgeuse` with a real, unmapped wire EU.
    SideWallUnmapped = 4,
    /// `FillGapEdgeuses(NULL, empty, empty)`.
    FillEmpty = 5,
    /// `BuildOffsetOfVertex(NULL, ...)` (native NER).
    VertexNull = 6,
    /// `BuildOffsetOfVertex` with a real isolated shell vertex.
    VertexIsolated = 7,
    /// `BuildOffsetOfFaceuse(NULL, ...)` (native NER).
    FaceuseNull = 8,
    /// Real planar faceuse, null offset surface (native early success).
    FaceuseNoSurface = 9,
    /// `PiecewiseMerge` on an empty offset B-rep, merge disabled.
    PiecewiseEmpty = 10,
    /// `MapEdgeuses` from a real planar face's upward use to the same face.
    MapPlane = 11,
    /// `BuildOffsetOfEdgeuses` on a real planar face edge and its radial mate,
    /// with no mapped offsets (no geometry-creator branch is entered).
    EdgeusesUnmapped = 12,
}

/// A [`face_tail`] fixture: the unit-square plane face `z = 0` (made by
/// `MakeFaceWithCurves` from 3D and UV lines) offset onto the plane
/// `z = offset_distance` over `domain`.
// Independent native setters are boolean toggles, not states of one machine.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, PartialEq)]
pub struct FaceTail {
    /// Geometry-creation distance; also the offset plane's height.
    pub offset_distance: f64,
    /// Geometry-creation approximation tolerance.
    pub approx_tol: f64,
    /// The offset plane's natural domain `umin, vmin, umax, vmax` (strictly increasing).
    pub domain: [f64; 4],
    /// `SM_OO_LOCAL_OPERATION` instead of `SM_OO_SOLID_OFFSET`.
    pub local: bool,
    /// `SetExtendConvexEdges`.
    pub extend_convex: bool,
    /// `SetMergeResults`.
    pub merge_results: bool,
    /// Offset the downward face use instead of the upward one.
    pub mate: bool,
    /// One self-intersection segment `u0, v0, u1, v1` on the offset plane: its 3D
    /// line at `z = offset_distance` and, unless [`Self::second_uv`] is given, both
    /// UV curves.
    pub self_intersection: Option<[f64; 4]>,
    /// The second UV curve of that self-intersection, `u0, v0, u1, v1`, when it
    /// differs from the first (requires [`Self::self_intersection`]).
    pub second_uv: Option<[f64; 4]>,
}

/// A [`map_twin`] fixture: `MapEdgeuses` between the rectangle plane face
/// `original` at `z = 0` and the rectangle face `offset` at `z = offset_distance`,
/// both made by `MakeFaceWithCurves` from explicit 3D and UV lines.
#[derive(Debug, Clone)]
pub struct MapTwin {
    /// The offset face's height (also the geometry-creation distance).
    pub offset_distance: f64,
    /// The original rectangle `xmin, ymin, xmax, ymax` (strictly increasing).
    pub original: [f64; 4],
    /// The offset rectangle `xmin, ymin, xmax, ymax` (strictly increasing).
    pub offset: [f64; 4],
    /// Build the offset plane with axes `Y, X` (normal down, UV = `(y, x)`).
    pub flip: bool,
    /// Map the downward original face use instead of the upward one.
    pub mate: bool,
    /// Split the offset rectangle's bottom edge at this `x` (strictly inside the
    /// rectangle) into two collinear edges, so the loop has five edges.
    pub split_bottom: Option<f64>,
}

/// The base of the offset surface of a [`replace`] fixture.
#[derive(Debug, Clone, PartialEq)]
pub enum OffsetBase {
    /// A real `SmPlane` or `SmBSplineSurface`.
    Surface(SurfaceFixture),
    /// `SmSurfOfExtrusion(generator, origin, direction, domain, swap_uv)`, without
    /// NURB (the executive's `GetOrCreateGwNurbPointer` makes it).
    Extrusion {
        /// The generator (an `SmBSplineCurve` kind).
        generator: CurveFixture,
        /// A point on the generator.
        origin: [f64; 3],
        /// The extrusion vector.
        direction: [f64; 3],
        /// The analytic domain `umin, vmin, umax, vmax` (U = the generator's domain).
        domain: [f64; 4],
        /// The generator is a constant-U iso-curve of the NURB.
        swap_uv: bool,
    },
}

/// A [`replace`] fixture: one `CreateFaceFromSurface` face (natural domain) on an
/// owning `SmOffsetSurface(offset_distance, base)`, then
/// `ReplaceImplicitOffsets(brep, approx_tol)`.
#[derive(Debug, Clone, PartialEq)]
pub struct Replace {
    /// The offset surface's base.
    pub base: OffsetBase,
    /// The offset distance.
    pub offset_distance: f64,
    /// The approximation tolerance passed to `ReplaceImplicitOffsets`.
    pub approx_tol: f64,
    /// Sample fractions in `[0, 1]^2` of each result face surface's natural domain.
    pub samples: Vec<[f64; 2]>,
}

mod sys {
    extern "C" {
        pub fn sc_offset_offset_executive_p1_eval(
            mode: i32,
            input: *const f64,
            input_len: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

fn bad(message: &str) -> FfiError {
    FfiError::InvalidArgument(format!("offset.offset_executive_p1: {message}"))
}

fn finite(values: &[f64]) -> Result<(), FfiError> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(bad("non-finite fixture value"))
    }
}

fn count(n: usize) -> Result<(), FfiError> {
    if n <= MAX_ITEMS {
        Ok(())
    } else {
        Err(bad("fixture count exceeds MAX_ITEMS"))
    }
}

fn interval([a, b]: [f64; 2], positive: bool) -> Result<(), FfiError> {
    finite(&[a, b, b - a])?;
    if a > b || (positive && a == b) {
        return Err(bad("invalid parameter interval"));
    }
    Ok(())
}

fn knots(degree: usize, n: usize, values: &[f64]) -> Result<[f64; 2], FfiError> {
    if !(1..=MAX_DEGREE).contains(&degree)
        || n <= degree
        || n > MAX_CONTROL_POINTS
        || n.checked_add(degree).and_then(|v| v.checked_add(1)) != Some(values.len())
    {
        return Err(bad("NURBS degree/count/knot length mismatch"));
    }
    finite(values)?;
    if values.windows(2).any(|v| matches!(v, [a, b] if a > b)) {
        return Err(bad("decreasing knots"));
    }
    // Also prevents a zero-width support interval / excessive knot multiplicity.
    for (a, b) in values.iter().zip(values.iter().skip(degree + 1)) {
        if a >= b || !(b - a).is_finite() {
            return Err(bad("zero-width NURBS basis support"));
        }
    }
    let domain = [
        *values
            .get(degree)
            .ok_or_else(|| bad("missing start knot"))?,
        *values.get(n).ok_or_else(|| bad("missing end knot"))?,
    ];
    interval(domain, true)?;
    Ok(domain)
}

fn points(values: &[[f64; 4]]) -> Result<(), FfiError> {
    if values.len() > MAX_CONTROL_POINTS {
        return Err(bad("control net exceeds MAX_CONTROL_POINTS"));
    }
    for &[x, y, z, w] in values {
        finite(&[x, y, z, w, x / w, y / w, z / w])?;
        if w <= 0.0 {
            return Err(bad("fixture homogeneous weights must be positive"));
        }
        if [x, y, z, x / w, y / w, z / w]
            .iter()
            .any(|v| v.abs() > MAX_COORDINATE)
        {
            return Err(bad("control point magnitude exceeds MAX_COORDINATE"));
        }
    }
    Ok(())
}

// Every append checks the aggregate budget before allocating. Counts are derived
// from validated slice lengths, never from a caller-supplied packed header.
#[derive(Default)]
struct Pack(Vec<f64>);

impl Pack {
    fn add(&mut self, values: &[f64]) -> Result<(), FfiError> {
        if self
            .0
            .len()
            .checked_add(values.len())
            .is_none_or(|n| n > MAX_PACKED_VALUES)
        {
            return Err(bad("aggregate fixture exceeds MAX_PACKED_VALUES"));
        }
        self.0.extend_from_slice(values);
        Ok(())
    }

    fn size(&mut self, n: usize) -> Result<(), FfiError> {
        self.add(&[n as f64])
    }

    fn control_points(&mut self, values: &[[f64; 4]]) -> Result<(), FfiError> {
        points(values)?;
        for point in values {
            self.add(point)?;
        }
        Ok(())
    }

    fn curve(&mut self, curve: &CurveFixture) -> Result<[f64; 2], FfiError> {
        match curve {
            CurveFixture::Line { start, end, domain } => {
                finite(start)?;
                finite(end)?;
                interval(*domain, true)?;
                for (a, b) in start.iter().zip(end) {
                    finite(&[b - a])?;
                }
                self.add(&[0.0])?;
                self.add(start)?;
                self.add(end)?;
                self.add(domain)?;
                Ok(*domain)
            }
            CurveFixture::BSpline {
                degree,
                control_points,
                knots: k,
            } => {
                let domain = knots(*degree, control_points.len(), k)?;
                self.add(&[1.0, *degree as f64, control_points.len() as f64])?;
                self.control_points(control_points)?;
                self.add(k)?;
                Ok(domain)
            }
        }
    }

    fn edge(&mut self, edge: &EdgeFixture) -> Result<(), FfiError> {
        let [a, b] = self.curve(&edge.curve)?;
        interval(edge.interval, false)?;
        let [min, max] = edge.interval;
        if min < a || max > b {
            return Err(bad("edge interval is outside the curve domain"));
        }
        self.add(&edge.interval)
    }

    fn surface(&mut self, surface: &SurfaceFixture) -> Result<(), FfiError> {
        match surface {
            SurfaceFixture::Plane {
                origin,
                x_axis,
                y_axis,
                scale,
                domain,
            } => {
                finite(origin)?;
                finite(x_axis)?;
                finite(y_axis)?;
                finite(scale)?;
                let [a, b, c, d] = *domain;
                interval([a, c], true)?;
                interval([b, d], true)?;
                // Zero axes are not rejected: native Unitize has a defined fallback.
                self.add(&[0.0])?;
                self.add(origin)?;
                self.add(x_axis)?;
                self.add(y_axis)?;
                self.add(scale)?;
                self.add(domain)
            }
            SurfaceFixture::BSpline {
                u_degree,
                v_degree,
                u_count,
                v_count,
                control_points,
                u_knots,
                v_knots,
            } => {
                if u_count.checked_mul(*v_count) != Some(control_points.len()) {
                    return Err(bad("surface control net length mismatch/overflow"));
                }
                knots(*u_degree, *u_count, u_knots)?;
                knots(*v_degree, *v_count, v_knots)?;
                self.add(&[
                    1.0,
                    *u_degree as f64,
                    *v_degree as f64,
                    *u_count as f64,
                    *v_count as f64,
                ])?;
                self.control_points(control_points)?;
                self.add(u_knots)?;
                self.add(v_knots)
            }
        }
    }
}

fn config_input(input: &Config) -> Result<Pack, FfiError> {
    finite(&[
        input.offset_distance,
        input.approx_tol,
        input.tangency_tol_radians,
    ])?;
    if input.operation > 4 {
        return Err(bad("invalid offset operation"));
    }
    for n in [
        input.original_edges,
        input.offset_edges,
        input.face_count,
        input.shell_face_sets.len(),
        input.relations.len(),
    ] {
        count(n)?;
    }
    let mut pack = Pack::default();
    pack.add(&[
        input.offset_distance,
        input.approx_tol,
        input.tangency_tol_radians,
        f64::from(input.operation),
        f64::from(input.inset),
        f64::from(input.extend_convex),
        f64::from(input.merge_results),
        f64::from(input.create_solid),
        f64::from(input.install_offset_brep),
        input.original_edges as f64,
        input.offset_edges as f64,
        input.face_count as f64,
    ])?;
    pack.size(input.shell_face_sets.len())?;
    for selectors in &input.shell_face_sets {
        count(selectors.len())?;
        pack.size(selectors.len())?;
        for &id in selectors {
            if id >= input.face_count {
                return Err(bad("shell face selector out of range"));
            }
            pack.size(id)?;
        }
    }
    pack.size(input.relations.len())?;
    for &[a, b] in &input.relations {
        if a >= input.original_edges * 2 || b >= input.offset_edges * 2 {
            return Err(bad("relation EU selector out of range"));
        }
        pack.add(&[a as f64, b as f64])?;
    }
    Ok(pack)
}

fn sync_input(input: &Synchronize) -> Result<Pack, FfiError> {
    count(input.first.len())?;
    count(input.second.len())?;
    if input
        .first
        .len()
        .checked_add(input.second.len())
        .is_none_or(|n| n > MAX_SYNC_EDGES)
    {
        return Err(bad("synchronization exceeds MAX_SYNC_EDGES"));
    }
    let mut pack = Pack::default();
    pack.add(&[f64::from(input.null_original), f64::from(input.null_other)])?;
    for edges in [&input.first, &input.second] {
        pack.size(edges.len())?;
        for edge in edges {
            pack.edge(edge)?;
        }
    }
    Ok(pack)
}

fn extension_input(input: &Extension) -> Result<Pack, FfiError> {
    finite(&[
        input.offset_distance,
        input.offset_distance * 5.0,
        input.approx_tol,
    ])?;
    if input.offset_distance < 0.0 {
        return Err(bad("extension offset distance must be nonnegative"));
    }
    for n in [
        input.surfaces.len(),
        input.actions.len(),
        input.samples.len(),
    ] {
        count(n)?;
    }
    let gets = input
        .actions
        .iter()
        .filter(|a| matches!(a, CacheAction::Get(_)))
        .count();
    let output_bound = input
        .samples
        .len()
        .checked_mul(8)
        .and_then(|n| n.checked_add(11))
        .and_then(|n| n.checked_mul(gets))
        .and_then(|n| n.checked_add(input.actions.len() * 3 + 2));
    if output_bound.is_none_or(|n| n > MAX_PACKED_VALUES) {
        return Err(bad("extension snapshot exceeds MAX_PACKED_VALUES"));
    }
    let mut pack = Pack::default();
    pack.add(&[input.offset_distance, input.approx_tol])?;
    pack.size(input.surfaces.len())?;
    for surface in &input.surfaces {
        pack.surface(surface)?;
    }
    pack.size(input.samples.len())?;
    for sample in &input.samples {
        finite(sample)?;
        if sample.iter().any(|v| !(0.0..=1.0).contains(v)) {
            return Err(bad("surface sample fractions must be in [0, 1]"));
        }
        pack.add(sample)?;
    }
    pack.size(input.actions.len())?;
    for action in &input.actions {
        match *action {
            CacheAction::Get(key) => {
                if let Some(key) = key {
                    if key >= input.surfaces.len() {
                        return Err(bad("surface key out of range"));
                    }
                }
                pack.add(&[0.0, key.map_or(-1.0, |k| k as f64)])?;
            }
            CacheAction::Forget(key) => {
                if key >= input.surfaces.len() {
                    return Err(bad("surface key out of range"));
                }
                pack.add(&[1.0, key as f64])?;
            }
            CacheAction::Clear => pack.add(&[2.0])?,
        }
    }
    Ok(pack)
}

fn vertex_input(input: &VertexFixture) -> Result<Pack, FfiError> {
    finite(&input.point)?;
    finite(&[
        input.offset_distance,
        input.approx_tol,
        input.original_tolerance,
        input.result_tolerance,
    ])?;
    count(input.samples.len())?;
    let mut pack = Pack::default();
    pack.add(&input.point)?;
    pack.add(&[
        input.offset_distance,
        input.approx_tol,
        input.original_tolerance,
        input.result_tolerance,
        f64::from(input.strut.is_some()),
    ])?;
    if let Some(strut) = &input.strut {
        // These are arithmetic in the fixture constructors; avoid overflowing
        // before native geometry validation can report a constructor rejection.
        finite(&[
            strut.radius,
            strut.height,
            input.point[0] - 2.0,
            input.point[0] + 2.0,
            input.point[1] - 2.0,
            input.point[1] + 2.0,
            input.point[2] + strut.height,
            input.point[0] + strut.radius,
            input.point[0] - strut.radius,
            input.point[1] + strut.radius,
            input.point[1] - strut.radius,
        ])?;
        pack.add(&[strut.radius, strut.height, f64::from(strut.map_mate)])?;
    }
    pack.size(input.samples.len())?;
    for sample in &input.samples {
        finite(sample)?;
        if sample.iter().any(|value| !(0.0..=1.0).contains(value)) {
            return Err(bad("vertex sample fractions must be in [0, 1]"));
        }
        pack.add(sample)?;
    }
    Ok(pack)
}

fn run(mode: i32, pack: &Pack) -> Result<Vec<f64>, FfiError> {
    call_with_buffer(1024, |out, len| {
        // SAFETY: only the typed, validated packers call this private function.
        // All counts, knot layouts, selectors and scalar finiteness were checked;
        // input/output slices and len remain live and C++ honors out's capacity.
        unsafe {
            sys::sc_offset_offset_executive_p1_eval(
                mode,
                pack.0.as_ptr(),
                pack.0.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Constructs real native topology and snapshots executive state and relationships.
/// See the module-level protocol for the returned flat record.
pub fn config(input: &Config) -> Result<Vec<f64>, FfiError> {
    run(0, &config_input(input)?)
}

/// Synchronizes actual line/B-spline edgeuses through the native public method.
/// Native split-path statuses/results are retained even when the Rust side defers.
pub fn synchronize(input: &Synchronize) -> Result<Vec<f64>, FfiError> {
    run(1, &sync_input(input)?)
}

/// Exercises real surface extension, cache hits/eviction and evaluated samples.
/// Negative internal distance magnitudes are rejected before calling C++;
/// signed zero is allowed. Native operation/evaluation errors remain in the snapshot.
pub fn extend(input: &Extension) -> Result<Vec<f64>, FfiError> {
    run(2, &extension_input(input)?)
}

/// Calls native `BuildOffsetOfVertex` on real topology and snapshots the resulting cap.
/// Native geometry-constructor rejections remain separate from operation statuses.
pub fn vertex(input: &VertexFixture) -> Result<Vec<f64>, FfiError> {
    run(6, &vertex_input(input)?)
}

/// Runs one independent topology probe. The tolerance is forwarded unchanged;
/// only non-finite values are rejected before calling native code.
pub fn probe(operation: Probe, approx_tol: f64) -> Result<Vec<f64>, FfiError> {
    finite(&[approx_tol])?;
    run(3, &Pack(vec![f64::from(operation as i32), approx_tol]))
}

fn face_tail_input(input: &FaceTail) -> Result<Pack, FfiError> {
    let [u0, v0, u1, v1] = input.domain;
    finite(&[input.offset_distance, input.approx_tol])?;
    finite(&input.domain)?;
    if !(u0 < u1 && v0 < v1) {
        return Err(bad("offset domain must be strictly increasing"));
    }
    let mut pack = Pack::default();
    pack.add(&[input.offset_distance, input.approx_tol])?;
    pack.add(&input.domain)?;
    for flag in [
        input.local,
        input.extend_convex,
        input.merge_results,
        input.mate,
    ] {
        pack.add(&[f64::from(u8::from(flag))])?;
    }
    let segments = match (input.self_intersection, input.second_uv) {
        (None, None) => 0.0,
        (None, Some(_)) => return Err(bad("second UV curve without a self-intersection")),
        (Some(_), None) => 1.0,
        (Some(_), Some(_)) => 2.0,
    };
    pack.add(&[segments])?;
    for segment in [input.self_intersection, input.second_uv]
        .into_iter()
        .flatten()
    {
        finite(&segment)?;
        pack.add(&segment)?;
    }
    Ok(pack)
}

fn rectangle([x0, y0, x1, y1]: [f64; 4]) -> Result<(), FfiError> {
    finite(&[x0, y0, x1, y1])?;
    if !(x0 < x1 && y0 < y1) {
        return Err(bad("rectangle must be strictly increasing"));
    }
    Ok(())
}

fn map_twin_input(input: &MapTwin) -> Result<Pack, FfiError> {
    finite(&[input.offset_distance])?;
    rectangle(input.original)?;
    rectangle(input.offset)?;
    if let Some(x) = input.split_bottom {
        finite(&[x])?;
        let [x0, _, x1, _] = input.offset;
        if !(x0 < x && x < x1) {
            return Err(bad(
                "bottom split must lie strictly inside the offset rectangle",
            ));
        }
    }
    let mut pack = Pack::default();
    pack.add(&[input.offset_distance])?;
    pack.add(&input.original)?;
    pack.add(&input.offset)?;
    pack.add(&[
        f64::from(u8::from(input.flip)),
        f64::from(u8::from(input.mate)),
        f64::from(u8::from(input.split_bottom.is_some())),
    ])?;
    if let Some(x) = input.split_bottom {
        pack.add(&[x])?;
    }
    Ok(pack)
}

/// Runs the native `MapEdgeuses` between two real rectangle plane faces. See the
/// module-level protocol for the returned record.
pub fn map_twin(input: &MapTwin) -> Result<Vec<f64>, FfiError> {
    run(5, &map_twin_input(input)?)
}

fn replace_input(input: &Replace) -> Result<Pack, FfiError> {
    finite(&[input.offset_distance, input.approx_tol])?;
    if input.samples.len() > 64 {
        return Err(bad("at most 64 replace samples"));
    }
    let mut pack = Pack::default();
    match &input.base {
        OffsetBase::Surface(surface) => {
            pack.add(&[0.0])?;
            pack.surface(surface)?;
        }
        OffsetBase::Extrusion {
            generator,
            origin,
            direction,
            domain,
            swap_uv,
        } => {
            finite(origin)?;
            finite(direction)?;
            let [a, b, c, d] = *domain;
            interval([a, c], true)?;
            interval([b, d], true)?;
            pack.add(&[1.0])?;
            pack.curve(generator)?;
            pack.add(origin)?;
            pack.add(direction)?;
            pack.add(domain)?;
            pack.add(&[f64::from(u8::from(*swap_uv))])?;
        }
    }
    pack.add(&[input.offset_distance, input.approx_tol])?;
    pack.size(input.samples.len())?;
    for sample in &input.samples {
        finite(sample)?;
        if sample.iter().any(|v| !(0.0..=1.0).contains(v)) {
            return Err(bad("surface sample fractions must be in [0, 1]"));
        }
        pack.add(sample)?;
    }
    Ok(pack)
}

/// Runs the native `ReplaceImplicitOffsets` on one real offset-surface face. See the
/// module-level protocol for the returned record.
pub fn replace(input: &Replace) -> Result<Vec<f64>, FfiError> {
    run(7, &replace_input(input)?)
}

// The packers' input checks without the native call: the difftest runner's Rust
// side refuses exactly what the FFI refuses by running the same code (counts,
// finiteness, selector ranges, fixture geometry validation and the packed-size
// budgets). Only the shim's own native-safety checks (its `Reader` bounds and
// `guard_two_point_isocurves`) are beyond these.

/// [`config`]'s input checks alone.
pub fn validate_config(input: &Config) -> Result<(), FfiError> {
    config_input(input).map(|_| ())
}
/// [`synchronize`]'s input checks alone.
pub fn validate_synchronize(input: &Synchronize) -> Result<(), FfiError> {
    sync_input(input).map(|_| ())
}
/// [`extend`]'s input checks alone.
pub fn validate_extend(input: &Extension) -> Result<(), FfiError> {
    extension_input(input).map(|_| ())
}
/// [`face_tail`]'s input checks alone.
pub fn validate_face_tail(input: &FaceTail) -> Result<(), FfiError> {
    face_tail_input(input).map(|_| ())
}
/// [`replace`]'s input checks alone.
pub fn validate_replace(input: &Replace) -> Result<(), FfiError> {
    replace_input(input).map(|_| ())
}
/// [`map_twin`]'s input checks alone.
pub fn validate_map_twin(input: &MapTwin) -> Result<(), FfiError> {
    map_twin_input(input).map(|_| ())
}

/// Offsets a real plane face use onto a real offset plane through the native
/// `BuildOffsetOfFaceuse`. See the module-level protocol for the returned record.
pub fn face_tail(input: &FaceTail) -> Result<Vec<f64>, FfiError> {
    run(4, &face_tail_input(input)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vertex_fixture() -> VertexFixture {
        VertexFixture {
            point: [1.25, -2.5, 3.75],
            offset_distance: 1.0,
            approx_tol: 1e-6,
            original_tolerance: 2e-7,
            result_tolerance: 5e-6,
            strut: Some(VertexStrut {
                radius: 0.6,
                height: 0.8,
                map_mate: true,
            }),
            samples: vec![[0.137, 0.219], [0.371, 0.683]],
        }
    }

    #[test]
    fn vertex_protocol_preserves_original_and_result_tolerances_and_real_mapping() {
        let input = vertex_fixture();
        let packed = vertex_input(&input).unwrap();
        assert_eq!(
            packed.0,
            [
                1.25, -2.5, 3.75, 1.0, 1e-6, 2e-7, 5e-6, 1.0, 0.6, 0.8, 1.0, 2.0, 0.137, 0.219,
                0.371, 0.683
            ]
        );
        let mut isolated = input;
        isolated.strut = None;
        assert_eq!(vertex_input(&isolated).unwrap().0.len(), 13);
        // Defined finite constructor/operation errors must reach native code,
        // not disappear as guard rejections of a successful comparison domain.
        isolated.offset_distance = -1.0;
        isolated.approx_tol = -1.0;
        isolated.original_tolerance = 0.0;
        isolated.result_tolerance = -0.0;
        assert!(vertex_input(&isolated).is_ok());
        let mut input = vertex_fixture();
        input.strut.as_mut().unwrap().radius = 0.0;
        assert!(vertex_input(&input).is_ok()); // actual native circle-constructor rejection
    }

    #[test]
    fn vertex_preflight_bounds_samples_and_refuses_nonfinite_constructor_arithmetic() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut input = vertex_fixture();
            input.point[1] = value;
            assert!(vertex_input(&input).is_err());
            input = vertex_fixture();
            input.result_tolerance = value;
            assert!(vertex_input(&input).is_err());
            input = vertex_fixture();
            input.strut.as_mut().unwrap().height = value;
            assert!(vertex_input(&input).is_err());
        }
        for sample in [[-0.01, 0.5], [0.5, 1.01], [f64::NAN, 0.5]] {
            let mut input = vertex_fixture();
            input.samples = vec![sample];
            assert!(vertex_input(&input).is_err());
        }
        let mut input = vertex_fixture();
        input.samples = vec![[0.3, 0.7]; MAX_ITEMS + 1];
        assert!(vertex_input(&input).is_err());
        input = vertex_fixture();
        input.point[2] = f64::MAX;
        input.strut.as_mut().unwrap().height = f64::MAX;
        assert!(vertex_input(&input).is_err());
    }

    fn line(interval: [f64; 2]) -> EdgeFixture {
        EdgeFixture {
            curve: CurveFixture::Line {
                start: [0.0, 0.0, 0.0],
                end: [1.0, 0.0, 0.0],
                domain: [0.0, 1.0],
            },
            interval,
        }
    }

    #[test]
    fn sync_accepts_gaps_zero_width_and_nulls() {
        assert!(sync_input(&Synchronize {
            first: vec![line([0.8, 1.0]), line([0.0, 0.0]), line([0.0, 0.2])],
            second: vec![],
            null_original: true,
            null_other: true,
        })
        .is_ok());
    }

    #[test]
    fn sync_rejects_bad_ranges_and_nonfinite() {
        for range in [[0.5, 0.4], [-0.1, 0.5], [0.0, 1.1], [0.0, f64::NAN]] {
            assert!(sync_input(&Synchronize {
                first: vec![line(range)],
                second: vec![],
                null_original: false,
                null_other: false,
            })
            .is_err());
        }
    }

    #[test]
    fn validates_native_nurbs_storage() {
        assert!(knots(1, 2, &[0.0, 0.0, 1.0, 1.0]).is_ok());
        assert!(knots(MAX_DEGREE + 1, 32, &[]).is_err());
        assert!(knots(1, 2, &[0.0, 0.0, 0.0, 1.0]).is_err());
        assert!(knots(2, 3, &[0.0, 0.0, 0.0, 1.0, 0.5, 1.0]).is_err());
        assert!(points(&[[0.0, 0.0, 0.0, 0.0]]).is_err());
        assert!(points(&[[f64::INFINITY, 0.0, 0.0, 1.0]]).is_err());
        assert!(points(&[[2.0, 0.0, 0.0, 2.0]]).is_ok());
    }

    #[test]
    fn extension_checks_net_overflow_keys_and_samples() {
        let surface = SurfaceFixture::BSpline {
            u_degree: 1,
            v_degree: 1,
            u_count: usize::MAX,
            v_count: 2,
            control_points: vec![],
            u_knots: vec![],
            v_knots: vec![],
        };
        assert!(Pack::default().surface(&surface).is_err());
        let mut ext = Extension {
            surfaces: vec![],
            offset_distance: 0.5,
            approx_tol: 1.0e-6,
            actions: vec![CacheAction::Get(None), CacheAction::Clear],
            samples: vec![],
        };
        assert!(extension_input(&ext).is_ok());
        ext.actions.push(CacheAction::Get(Some(0)));
        assert!(extension_input(&ext).is_err());
        ext.actions.clear();
        ext.samples.push([1.01, 0.0]);
        assert!(extension_input(&ext).is_err());
    }

    fn extension(offset_distance: f64) -> Extension {
        Extension {
            surfaces: vec![
                SurfaceFixture::Plane {
                    origin: [0.0, 0.0, 0.0],
                    x_axis: [1.0, 0.0, 0.0],
                    y_axis: [0.0, 1.0, 0.0],
                    scale: [2.0, -3.0],
                    domain: [0.0, 0.0, 1.0, 1.0],
                },
                SurfaceFixture::BSpline {
                    u_degree: 1,
                    v_degree: 1,
                    u_count: 2,
                    v_count: 2,
                    control_points: vec![
                        [0.0, 0.0, 0.0, 1.0],
                        [0.0, 1.0, 0.0, 1.0],
                        [1.0, 0.0, 0.0, 1.0],
                        [1.0, 1.0, 0.0, 1.0],
                    ],
                    u_knots: vec![0.0, 0.0, 1.0, 1.0],
                    v_knots: vec![0.0, 0.0, 1.0, 1.0],
                },
            ],
            offset_distance,
            approx_tol: 1.0e-6,
            actions: vec![CacheAction::Get(Some(0)), CacheAction::Get(Some(1))],
            samples: vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]],
        }
    }

    #[test]
    fn extension_rejects_negative_distances_before_native_call() {
        for distance in [
            -f64::from_bits(1),
            -f64::MIN_POSITIVE,
            -1.0e-12,
            -0.25,
            -f64::MAX / 10.0,
        ] {
            // Exercise the pure packer used by extend before it can call run/C++.
            assert!(matches!(
                extension_input(&extension(distance)),
                Err(FfiError::InvalidArgument(message))
                    if message.ends_with("extension offset distance must be nonnegative")
            ));
        }
    }

    #[test]
    fn extension_accepts_nonnegative_magnitudes_without_changing_bits() {
        for distance in [
            -0.0,
            0.0,
            f64::from_bits(1),
            f64::MIN_POSITIVE,
            1.0e-12,
            0.25,
            f64::MAX / 10.0,
        ] {
            let packed = extension_input(&extension(distance)).unwrap();
            assert_eq!(
                packed.0.first().map(|v| v.to_bits()),
                Some(distance.to_bits())
            );
        }
    }

    #[test]
    fn extension_still_rejects_nonfinite_values_and_fivefold_overflow() {
        for distance in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
            assert!(matches!(
                extension_input(&extension(distance)),
                Err(FfiError::InvalidArgument(_))
            ));
        }
    }

    #[cfg(feature = "cpp")]
    #[test]
    fn native_extension_rejects_negative_distances_as_arguments() {
        for distance in [-f64::from_bits(1), -0.25, -f64::MAX / 10.0] {
            // Complete empty fixture: without the guard it would build successfully.
            let input = [distance, 1.0e-6, 0.0, 0.0, 0.0];
            let mut out = [123.0; 2];
            let mut out_len = -1;
            // SAFETY: all buffers remain live and have the declared lengths. The
            // direct ABI must reject the negative magnitude before building geometry.
            let status = unsafe {
                sys::sc_offset_offset_executive_p1_eval(
                    2,
                    input.as_ptr(),
                    input.len() as i64,
                    out.as_mut_ptr(),
                    out.len() as i64,
                    &raw mut out_len,
                )
            };
            assert_eq!(status, crate::SC_ERR_ARG);
            assert_eq!(out, [123.0; 2]);
            assert_eq!(out_len, -1);
        }
    }

    #[test]
    fn config_accepts_negative_constructor_scalars_without_extension_bounds() {
        let mut cfg = Config {
            offset_distance: -0.25,
            approx_tol: -1.0e-6,
            tangency_tol_radians: -0.5,
            operation: 0,
            inset: true,
            extend_convex: false,
            merge_results: false,
            create_solid: false,
            install_offset_brep: false,
            original_edges: 0,
            offset_edges: 0,
            face_count: 0,
            shell_face_sets: vec![],
            relations: vec![],
        };
        for distance in [-f64::from_bits(1), -0.25, f64::MIN] {
            cfg.offset_distance = distance;
            let packed = config_input(&cfg).unwrap();
            assert_eq!(packed.0.first(), Some(&distance));
            assert_eq!(packed.0.get(1), Some(&cfg.approx_tol));
            assert_eq!(packed.0.get(2), Some(&cfg.tangency_tol_radians));
        }
    }

    #[test]
    fn config_checks_selectors_not_duplicate_relations() {
        let mut cfg = Config {
            offset_distance: -0.2,
            approx_tol: 0.0,
            tangency_tol_radians: 2.0 * std::f64::consts::PI / 180.0,
            operation: 4,
            inset: false,
            extend_convex: false,
            merge_results: false,
            create_solid: false,
            install_offset_brep: true,
            original_edges: 1,
            offset_edges: 1,
            face_count: 1,
            shell_face_sets: vec![vec![0, 0], vec![]],
            relations: vec![[0, 1], [0, 1]],
        };
        assert!(config_input(&cfg).is_ok());
        cfg.relations.push([2, 0]);
        assert!(config_input(&cfg).is_err());
        cfg.relations.clear();
        cfg.shell_face_sets.push(vec![1]);
        assert!(config_input(&cfg).is_err());
    }

    #[test]
    fn config_validates_tangency_finiteness_only_and_packs_before_operation() {
        let mut cfg = Config {
            offset_distance: 0.25,
            approx_tol: 1.0e-6,
            tangency_tol_radians: 2.0 * std::f64::consts::PI / 180.0,
            operation: 4,
            inset: false,
            extend_convex: false,
            merge_results: false,
            create_solid: false,
            install_offset_brep: false,
            original_edges: 0,
            offset_edges: 0,
            face_count: 0,
            shell_face_sets: vec![],
            relations: vec![],
        };
        for tangency in [
            cfg.tangency_tol_radians,
            0.0,
            -0.0,
            -0.25,
            f64::from_bits(1),
            f64::MIN_POSITIVE,
            f64::MIN,
            f64::MAX,
        ] {
            cfg.tangency_tol_radians = tangency;
            let packed = config_input(&cfg).unwrap();
            assert_eq!(packed.0.len(), 14);
            assert_eq!(packed.0.first(), Some(&cfg.offset_distance));
            assert_eq!(packed.0.get(1), Some(&cfg.approx_tol));
            assert_eq!(
                packed.0.get(2).map(|v| v.to_bits()),
                Some(tangency.to_bits())
            );
            assert_eq!(packed.0.get(3), Some(&f64::from(cfg.operation)));
        }
        for tangency in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            cfg.tangency_tol_radians = tangency;
            assert!(matches!(
                config_input(&cfg),
                Err(FfiError::InvalidArgument(_))
            ));
        }
    }

    #[test]
    fn synchronization_and_snapshot_growth_are_bounded() {
        assert!(sync_input(&Synchronize {
            first: vec![line([0.0, 1.0]); MAX_SYNC_EDGES + 1],
            second: vec![],
            null_original: false,
            null_other: false,
        })
        .is_err());
        assert!(extension_input(&Extension {
            surfaces: vec![],
            offset_distance: 1.0,
            approx_tol: 0.0,
            actions: vec![CacheAction::Get(None); MAX_ITEMS],
            samples: vec![[0.5, 0.5]; MAX_ITEMS],
        })
        .is_err());
    }

    #[test]
    fn accepts_real_bspline_and_plane_layouts() {
        let curve = CurveFixture::BSpline {
            degree: 2,
            control_points: vec![
                [0.0, 0.0, 0.0, 1.0],
                [0.5, 1.0, 0.0, 1.0],
                [1.0, 0.0, 0.0, 1.0],
            ],
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        };
        assert!(sync_input(&Synchronize {
            first: vec![EdgeFixture {
                curve,
                interval: [0.2, 0.8]
            }],
            second: vec![],
            null_original: false,
            null_other: false,
        })
        .is_ok());
        assert!(extension_input(&Extension {
            surfaces: vec![SurfaceFixture::Plane {
                origin: [0.0, 0.0, 0.0],
                x_axis: [1.0, 0.0, 0.0],
                y_axis: [0.0, 1.0, 0.0],
                scale: [2.0, -3.0],
                domain: [0.0, 0.0, 1.0, 1.0],
            }],
            offset_distance: 0.25,
            approx_tol: 1.0e-6,
            actions: vec![
                CacheAction::Get(Some(0)),
                CacheAction::Forget(0),
                CacheAction::Clear
            ],
            samples: vec![[0.0, 0.0], [0.5, 0.5], [1.0, 1.0]],
        })
        .is_ok());
    }

    #[test]
    fn aggregate_budget_is_checked_before_append() {
        let mut pack = Pack(vec![0.0; MAX_PACKED_VALUES]);
        assert!(pack.add(&[1.0]).is_err());
        assert_eq!(pack.0.len(), MAX_PACKED_VALUES);
    }
}
