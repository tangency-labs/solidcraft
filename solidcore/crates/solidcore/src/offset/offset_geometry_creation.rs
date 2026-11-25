//! Offset geometry creation (`SmOffsetGeometryCreation`): the offset surfaces of
//! faces, the cap surfaces of edges and vertices of an offset, and the mating,
//! extension and trimming of the offset edges, used by the offset executive.
//!
//! Upstream: `source/SMLib/src/SmOffsetGeometryCreation.cpp`,
//! `source/SMLib/inc/SmOffsetGeometryCreation.h`.
//!
//! | Rust                                                        | Upstream                                              |
//! |-------------------------------------------------------------|-------------------------------------------------------|
//! | [`OffsetGeometryCreation`], [`OffsetGeometryCreation::new`] | `SmOffsetGeometryCreation`, its constructor           |
//! | `Drop` (implicit)                                           | `~SmOffsetGeometryCreation`                           |
//! | [`OffsetGeometryCreation::this_approx_tol_3d`]              | `GetThisApproxTol3d`                                  |
//! | [`OffsetGeometryCreation::tangency_tol_radians`]            | `GetTangencyTolRadians`                               |
//! | [`OffsetGeometryCreation::tangency_tol_degrees`]            | `GetTangencyTolDegrees`                               |
//! | [`OffsetGeometryCreation::offset_distance`]                 | `GetOffsetDistance`                                   |
//! | [`OffsetGeometryCreation::set_make_analytics`]              | `SetMakeAnalytics`                                    |
//! | [`OffsetGeometryCreation::faceuse_offset`]                  | `SmOffsetGeometryCreation::FaceuseOffset`             |
//! | [`LocalOperationGeometryCreation`], [`LocalOperationGeometryCreation::faceuse_offset`] | `SmLocalOperationGeometryCreation`, its `FaceuseOffset` |
//! | [`OffsetGeometryCreation::vertex_offset`]                   | `VertexOffset`                                                |
//! | [`OffsetGeometryCreation::update_face_surface_domain`]      | `UpdateFaceSurfaceDomain`                             |
//! | [`OffsetGeometryCreation::replace_edge_curves`]             | `ReplaceEdgeCurves`                                   |
//! | [`OffsetGeometryCreation::trim_extended_edge`]              | `TrimExtendedEdge`                                    |
//! | [`OffsetGeometryCreation::extend_trim_edges`]               | `ExtendTrimEdges`                                     |
//! | [`OffsetGeometryCreation::mate_edges`]                      | `MateEdges`                                           |
//! | [`OffsetGeometryCreation::have_tangent_gaps`]               | `HaveTangentGaps`                                     |
//! | [`OffsetGeometryCreation::edgeuse_offset`]                  | `EdgeuseOffset`                                       |
//! | [`OffsetGeometryCreation::is_step_across_edge`]             | the fork's `IsStepAcrossEdge` (tangency-labs/smlib 5d428ae; opt-in) |
//! | [`OffsetGeometryCreation::create_step_wall`]                | the fork's `CreateStepWall` (tangency-labs/smlib 5d428ae, 90aabff; opt-in) |
//! | `make_non_rational` (private)                               | the fork's uniform-weight rail made polynomial (90aabff) |
//! | [`SsiData`]                                                 | `SmSSIData` (`SmSSIData.h`, CO-PORTED: no unit owns it) |
//! | [`OffsetExecutive`]                                         | what this unit uses of `SmOffsetExecutive` (ported in `offset.offset_executive_p1`), over handles |
//! | [`OffsetBrep`]                                              | what the unit reads from and writes to the two B-reps |
//! | `VertexOffsetBrep` (private)                                | the vertex creator's topology/owned-geometry subset |
//! | `FaceOffsetBrep` (private)                                   | the face creator's topology/owned-geometry subset |
//! | `create_analytic_extension` (private)                       | the SolidCraft fork's `sm_CreateAnalyticExtension` (tangency-labs/smlib@fddc366, @98ab1b0; opt-in) |
//! | `exact_piece` (private)                                     | the fork's `sm_ExactPiece` (tangency-labs/smlib@fddc366; opt-in) |
//! | [`make_edges_on_analytics_exact`]                          | the fork's `MakeEdgesOnAnalyticsExact` (tangency-labs/smlib@cd85d34) |
//! | [`OffsetGeometryCreation::default_tangency_tol_radians`]    | the constructor's `m_dTangencyTolRadians` (half a degree in the fork's tangency-labs/smlib@2e1bb8c; opt-in) |
//! | `first_cut_from_far_end` (private)                          | the fork's cut-first-from-the-far-end choice in `TrimExtendedEdge` (tangency-labs/smlib@2e1bb8c; opt-in) |
//! | `offset_vertex_image` (private)                             | not upstream: a vertex's image on its face's offset (WR-736; opt-in) |
//! | `passages_on_one_turn`, `middle_passage` (private)          | not upstream: a vertex image the continued curve passes on several turns, taken on the edge's (WR-736) |
//!
//! Deviations:
//! - **Disappearing periodic seams.** Genuine support roots bound each SSI
//!   component's active interval. A seam outside that interval emits a boundary
//!   event; its branch is identified by an already mated opposite boundary,
//!   rather than proximity to the old edge. Seam walks use explicit visited sets.
//! - **Curved tangent crossings.** A local operation may create two intersection
//!   branches beside an originally tangent edge. A plane/curved pair keeps the curved
//!   face's side. Two curved faces require a proved unique physical component
//!   ([`unique_cone_meeting`]); otherwise they are refused explicitly before SSI.
//!   Midpoint distance cannot identify a retained curved branch.
//! - **Four faces at a vertex (WR-666, the only path).** The fork's
//!   tangency-labs/smlib@2e1bb8c: the tangency
//!   tolerance is half a degree, `trim_extended_edge` keeps a near-tangent trim point where
//!   the curve passes through the surface by more than the tolerance, and at an all-convex or all-concave vertex takes the cut
//!   reached first from the edge's other end. That choice also skips, on an open edge's
//!   second end, a cut at the first trim point again (the fork's `bFirstAgain`, used there
//!   only; the main candidate search is upstream's, WR-463's opt-in covers closed curves).
//! - **A lamina edge's corner at its vertex image (WR-736, #2611, #2769).**
//!   `trim_extended_edge` ends a lamina edge's offset, at a vertex whose next edge is
//!   lamina or a tangent sector, where the offset curve passes nearest the vertex's image
//!   `V + d n(V)` (`offset_vertex_image`), not at the crossing of the two edges' UV trim
//!   curves (a parameter of the UV curve, which the lifted 3D curve does not keep: corners
//!   landed up to 4.3e-5 apart) nor at the neighbour's offset vertex. Upstream's reading is
//!   gone (no flag). Where the lifted curve passes the image more than once (an arc's UV
//!   curve continued ten lengths each way runs round its circle several times), an open
//!   edge takes it on one turn at both its ends: of the parameters of that one point, the
//!   one nearest where the edge's middle drops onto the curve (`passages_on_one_turn`;
//!   of that drop's own passages the one farthest from the curve's ends, `middle_passage`),
//!   not whichever rounding leaves nearest the base vertex. Solidcore-native, no fork
//!   commit.
//! - **An edge between coplanar planes, at its vertex's image.** `trim_extended_edge` takes
//!   the vertex's image as an end candidate, as for a lamina edge, for an edge between two
//!   planar faces on one plane whose tangent neighbours at the vertex run between the same
//!   two faces, not the crossings of their extended UV curves: those continue one another,
//!   and on one circle (a capped footprint split where it touches its face's rim) they
//!   coincide and crossed at thousands of points within the tolerance. Solidcore-native, no
//!   fork commit.
//! - **A tangent edge's ends and its other UV curve (#17514).** `trim_extended_edge` takes a
//!   grazing cut by a neighbour's offset as a crossing where the end has no tangent
//!   neighbour and the curve meets the offset at more than the tangency tolerance (the
//!   angle that classed no neighbour tangent there), and `extend_trim_edges` gives an edge whose two uses are on one surface (a
//!   seam), with UV curves a period apart, the extended UV curve moved onto the other use's
//!   side instead of a drop (`same_surface_twin`). Solidcore-native, no fork commit.
//! - **Object handles.** Every `SmObject*` (topology, curves, surfaces) is a caller
//!   handle `O` ([`ClassObject`] + [`SolutionObject`]), as in
//!   [`proto_topology`](crate::topology::proto_topology) and the topology intersector.
//!   The B-reps are reached through [`OffsetBrep`] (each method names the ported
//!   function it stands for), geometry through [`ClassGeometry`] /
//!   [`ProtoGeometry`]; new curves and surfaces go to the owner's stores
//!   (`insert_curve` / `insert_surface`) and their handles are returned where upstream
//!   returns owned pointers. The offset executive is reached through
//!   [`OffsetExecutive`] (its stored flags, shell faces, edge-use map and
//!   extended-surface cache), which its owner implements over the ported
//!   [`crate::offset::offset_executive::p1::OffsetExecutive`]. The executive's face and
//!   vertex adapters use narrow internal backends; public full-backend methods delegate
//!   to the same algorithms, preserving lookup/storage error translation.
//! - **Context.** `crContext` is the [`Kernel`] passed to each method.
//! - **Out-params** become return values ([`VertexOffsetGeometry`], [`EdgeuseOffset`],
//!   [`ReplaceEdgeCurves`], the trim interval of `TrimExtendedEdge` as an `Option`,
//!   the surface and self-intersections of `FaceuseOffset`). The `NotUsed`
//!   parameters are kept.
//! - **Deleted objects.** Geometry retained by handle stays in the owner's store
//!   when upstream deletes it ([`ReplaceEdgeCurves`] reports the `rbDelete*` flags).
//!   Trimmed UV and approximated offset-rail temporaries instead stay locally owned.
//! - **Callees not ported yet** are private stubs returning [`SolidError::Unported`],
//!   each tagged `UNPORTED(<unit>)`:
//!   `SmCircularCrossSectionFSG::CreateSurfaceFromCurves` is
//!   `blend.fillet_standard_solver_p1`'s, on the centre line's `SmBSplineCurve` part.
//!   The code after a stub is ported in full;
//!   `SmEdgeuse::NormalizedEvaluate` / `CreateUVTrimCurve` (`topology.edgeuse_p3`,
//!   through [`OffsetBrep`]), `SmBrep::MateLaminaEdgeToSurface` / `ReplaceSurface`
//!   (`topology.brep_p2`, through [`OffsetBrep`]), `SmSurface::GlobalCurveIntersect` (`surface.surface_p8`) and
//!   `SmSurface::CreateOffsetSurface` / the `SmOffsetSurface` constructor
//!   (`surface.offset_surface`) are real calls. `EdgeuseOffset` approximates its
//!   missing rails through [`crate::curve::crv_on_surf::CrvOnSurf`] on the actual
//!   offset surface, retaining its concrete evaluation and supplied knot sequence.
//! - `SM_CAST_PTR(SmOffsetSurface, ..)` on an analytic copy: an offset surface gets
//!   `SetAllowExtension(FALSE)`, as upstream.
//! - **Undefined behaviour upstream** (null objects dereferenced, a curve that is
//!   not an `SmBSplineCurve` cast as one and used, a null curve extension used) is
//!   [`SolidError::InvalidInput`] with [`UNDEFINED_UPSTREAM`] as operation, which tells
//!   it apart from a callee's real `SM_ERR_INVALID_INPUT`; the statuses upstream
//!   ignores are ignored whatever they are, except these and [`SolidError::Unported`].
//! - **Loose tangent edges (WR-530, #402, not upstream).** In a local operation ([`OffsetExecutive::is_local_operation`]) `ExtendTrimEdges`
//!   fails when the lifted edge drops onto the other extended surface farther than ten
//!   times `max(edge tolerance, approximation tolerance)`
//!   ([`OffsetFailure::LooseTangentEdge`]), and `EdgeuseOffset` passes that refusal on
//!   (the fork's tangency-labs/smlib@23c5b0a); any other failure of `ExtendTrimEdges` is
//!   only reported, as upstream's (the fork's passed every failure on, refusing a lone
//!   disc's lamina edge).
//! - **UV curve off the extended domain (#21743, not upstream).** `ExtendTrimEdges`
//!   lifts the piece of the extended UV curve that `FindUVCurveIvlInDomain` keeps (the
//!   one holding the original curve's midpoint) only when that midpoint is inside the
//!   extended surface's domain (within the UV point tolerance `EFF_ZERO_SQRT * (1 + |uv|)`,
//!   which a seam trim's fitting noise stays within), and only when the whole trimmed
//!   piece is (`uv_piece_in_domain`: Bezier control-point hulls certify it, a curve point
//!   outside refuses; at most 40 halvings and 4096 segments, past which it is lifted as
//!   before). The piece runs between boundary
//!   crossings, so it is wholly inside or wholly outside; outside (an edge use whose UV
//!   curve is on another surface), upstream lifted it through the surface clamped at its
//!   boundary, a 3D curve along that boundary and not over the edge. The port refuses
//!   with [`OffsetFailure::ExtendedCurveOffDomain`] instead, which `EdgeuseOffset`
//!   only reports, as any non-loose failure: the edge keeps its curve.
//! - **Step walls (WR-530 addendum).** A
//!   local operation's tangent edge whose two faces stay parallel but apart (one moved
//!   along its normal) gets the ruled step wall between the two offset edges instead of
//!   the extension (the fork's 5d428ae, 90aabff); `EdgeuseOffset::closing_wall` marks it.
//!   `IsStepAcrossEdge` samples 11 points along the edge (a fixed count).
//! - **`ExtendTrimEdges`' empty seam drop.** When a candidate's `GlobalPointSolve`
//!   (`SM_SR_SINGLE`, minimize) finds no solution, upstream reads `sSolutions2[0]`
//!   anyway (cpp:1610): defined behaviour, the array's backing `sData2[0]` (a default
//!   `SmSolution`, value `0.0`, or an earlier candidate's result). A single minimizing
//!   solve always has a solution, so this is unreachable; the port refuses it with the
//!   same [`UNDEFINED_UPSTREAM`] error instead of reproducing the stale value.
//! - **Virtual calls.** `pSur->CreateOffsetSurface` dispatches on the surface kind
//!   (the plane, surface-of-revolution and B-spline overrides, else the base); the
//!   curves cast `SM_CAST_PTR(SmBSplineCurve, ..)` keep their kind (a line or a circle
//!   is not sliced to its B-spline part), so `IsArc`, `IsLine`, `IsClosed`,
//!   `CalculateBoundingBox`, `Trim`, `Copy`, `DropPoint` and the evaluations take the
//!   kind's override; only `new SmBSplineCurve(*p)` and the static B-spline routines
//!   (`CreateTrimmedPlaneFromRuled`'s reading of an arc, `MakeCurvesCompatible`, the
//!   knots and extension of a UV curve) see the B-spline part.
//! - **Code behind stubs** is ported as upstream runs it, unchecked statuses included:
//!   `HaveTangentGaps` ignores `ClosestPoint`'s status (a failed closest point counts as
//!   `bSuccess == FALSE`; a sample it cannot evaluate is skipped, as the fork does, see
//!   below), `TrimExtendedEdge` ignores the `Evaluate` / `EvaluateNormal` of a solution
//!   the same way, and `ExtendTrimEdges` keeps
//!   `pOtherUVCurve` as the edge use's or radial's own UV trim curve when there is no
//!   other extended surface (`cpp:1452`, `1458`), passes it to `TrimExtendedEdge` and trims
//!   it in place (`cpp:1694`). The unchecked `Copy` calls of `ExtendTrimEdges` and
//!   `MateEdges` copy B-spline or line curves, which cannot fail in the port (upstream's
//!   only failure is an allocation), so their `?` never fires.
//!   `SmBSplineSurface::CreateTrimmedPlaneFromRuled` fills its trimming arrays in place
//!   before its last step (the bilinear plane from the plane's own corners) and keeps
//!   them when that step fails; the port returns them only on success, which differs
//!   only if that bilinear surface fails.
//! - **Trim intervals.** A `Trim(SmExtent1d &)` may snap its interval to knots; where
//!   upstream trims several curves to one interval, the snapped interval reaches the
//!   next trim as it does there.
//! - `MSG` / `SM_ASSERT` only report; the `SM_DEBUG_CODE` and `#if 0` blocks and the
//!   empty `bShelling` branch are not ported. `SM_VALIDATE_INTERSECTORS` is not
//!   defined in the reference build.
//! - **Centre line of the circular FSG.** `CreateSurfaceFromCurves` receives the centre
//!   line as the [`Curve`] itself, so `GetDegree` / `IsLine` / `IsArc` / `Evaluate`
//!   dispatch on an `SmLine` (or other analytic) centre line as upstream does
//!   (`SmFilletStandardSolver.cpp:3353-3384`).
//! - **Opt-in fork fix.** With `Kernel::fixes.offset_analytic_mate_edges` (WR-495),
//!   `mate_edges` runs the SolidCraft fork's exact-piece swap (tangency-labs/smlib@fddc366
//!   with @98ab1b0) between the marched intersection and its trimming; off, it is
//!   upstream's. `exact_piece` counts the whole turns it moves a conic angle by and
//!   returns [`SolidError::NotConverged`] past `MAX_EXACT_PIECE_TURNS` (65536), where
//!   the fork's loop would not end for a non-finite or huge angle.
//! - **Fork fix.** The shell's result always runs [`make_edges_on_analytics_exact`]
//!   (WR-545; the fork's tangency-labs/smlib@cd85d34, which upstream lacks). Upstream's
//!   `CreateTrimmedNURBSCurve` gives `NULL` where the port returns an error; any error
//!   but [`SolidError::Unported`] skips the edge as `NULL` does, as does a failing
//!   `GlobalSurfaceIntersect`. `SmBrep::ReplaceCurve` is
//!   [`crate::topology::brep::p2::replace_curve`]. The faces'
//!   surfaces are intersected through a [`BrepFaceServices`] of the result (the face
//!   context upstream's `SmSurface::GetFace` gives each surface's trimmed cache), made
//!   for each edge: the face caches are not kept from one edge to the next, where
//!   upstream's stay on the surfaces between curve replacements.
//! - **Recursion.** None.
//! - **Failures (#15034, #15368).** Every failure keeps upstream's status and is named:
//!   the caller's arguments (no vertex, no edge use to mate, a face not listed for a local
//!   operation) and body as supplied (an edge with no curve, a face with no surface) are
//!   invalid input; a curve kind the routine does not read, a UV trim curve the body does not
//!   carry, a vertex whose bounding curves are not arcs or whose arcs miss their neighbours'
//!   planes, an edge whose extended curve or intersection is cut at neither end, the gaps
//!   at a concave tangent edge, rails over different intervals, an undecided crossing and a
//!   shell's tangent gap that cannot be certified are each an [`OffsetFailure`] of their own;
//!   a step that fails through the kernel's fault is an [`OffsetFailure::Defect`] whose
//!   reason (one of the named `const`s) says why. A caught error is kept as the cause.
//!   `edgeuse_offset` leaves an edge without its offset edge uses unmated without asking
//!   `MateEdges` (which refuses it), and passes a defect of the mating on.
//! - **Periodic seam topology (#18803).** An SSI component that does not meet a
//!   same-face seam's chart boundary in its active support interval ends on the
//!   genuine boundary beyond that seam. The supports must give one agreeing root. The selected mate reports
//!   these ends so the executive can rebuild the periodic quotient boundary.
//!
//! As the fork does (#483; fork 4803d6d, since #2009), `HaveTangentGaps` skips a sample
//! it cannot evaluate (upstream measured an unset point); `ClosestPoint`'s status stays
//! dropped, its success flag decides. A successful query without a solution retains its
//! existing meaning.
//!

use crate::blend::fillet_section::interval::Ivl;
use crate::blend::fillet_standard_solver::p1::CircularCrossSectionFsg;
use crate::curve::curve::p1::GlobalPointSolveOptions;
use crate::curve::curve::p2::{Curve, CurveKind};
use crate::curve::curve::p4::is_closed_base;
use crate::curve::curve::p8::CurveIntersectOptions;
use crate::curve::ellipse::Ellipse;
use crate::curve::line::Line;
use crate::error::offset::OffsetFailure;
use crate::error::{EntityRef, Failure, Reason, Result, SolidError, SM_ERR, SM_ERR_NULL_POINTER};
use crate::geom::assert_array::ObjectKey;
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::context::MarkType;
use crate::geom::expansion as exact;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::geom_utility::p1::point_project_to_plane;
use crate::geom::geom_utility::p2::{circle_plane_intersect, ContinuityType, CurveIntersection};
use crate::geom::global_solver::{ObjectZoneTol, SolutionRequested, SolverOperation};
use crate::geom::solution_array::{SolutionArray, SolutionObject, SolutionType};
use crate::geom::tol::{approx_tol3d, are_same, zone_tol_3d_of_curve};
use crate::intersect::point_class::{ClassGeometry, ClassObject};
use crate::kernel::Kernel;
use crate::math::{
    max2, min2, Point2, Point3, Vec3, BIG_DOUBLE, EFF_ZERO, EFF_ZERO_PARAM, EFF_ZERO_SQRT, PI,
    UNDEF_DOUBLE,
};
use crate::nurbs::b_spline_curve::p1::NurbCircleParam;
use crate::nurbs::b_spline_curve::p2::{ArcData, BoundingBoxRequest, ControlPointForm};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::b_spline_surface_n_lib::p2::{EvaluateFlags, OrientType};
use crate::nurbs::knots::Side;
use crate::nurbs::math::{NOW, NOZ};
use crate::query::brep::BrepFaceServices;
use crate::query::edge::ClosestPoint;
use crate::surface::implicit::{AnalyticSupport, Implicit};
use crate::surface::offset_surface::{OffsetSurface, SS_UNKNOWN as OFFSET_SS_UNKNOWN};
use crate::surface::surface::p1::{SurfaceIntersectContext, SurfaceSolveContext};
use crate::surface::surface::p2::{
    SurfaceIntersection, SurfaceKind, TsectCurveType, CONE_TYPE, CYLINDER_TYPE, PLANE_TYPE,
    SPHERE_TYPE, TORUS_TYPE,
};
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::Surface;
use crate::surface::surface_cache::face_context::FaceQueries;
use crate::surface::surface_cache::{SurfaceCache, SurfaceCacheState};
use crate::topology::brep::p1 as bp1;
use crate::topology::brep::p2 as brep_p2;
use crate::topology::brep_cache::CacheObject;
use crate::topology::edge::{self as topo_edge, NotificationStores};
use crate::topology::face::p3::create_uv_trim_curves;
use crate::topology::proto_topology::ProtoGeometry;
use crate::topology::{Brep, EdgeId, FaceId, VertexId};

const OP_LOCAL: &str =
    "offset::offset_geometry_creation::LocalOperationGeometryCreation::faceuse_offset";
const OP_FACEUSE: &str = "offset::offset_geometry_creation::faceuse_offset";
const OP_VERTEX: &str = "offset::offset_geometry_creation::vertex_offset";
const OP_UPDATE: &str = "offset::offset_geometry_creation::update_face_surface_domain";
const OP_EXTEND: &str = "offset::offset_geometry_creation::extend_trim_edges";
const OP_MATE: &str = "offset::offset_geometry_creation::mate_edges";
const OP_TRIM: &str = "offset::offset_geometry_creation::trim_extended_edge";
const OP_EDGEUSE: &str = "offset::offset_geometry_creation::edgeuse_offset";
const OP_EXACT_PIECE: &str = "offset::offset_geometry_creation::exact_piece";
const OP_ANALYTICS_EXACT: &str = "offset::offset_geometry_creation::make_edges_on_analytics_exact";
/// Not upstream (WR-346): the most sides a collapsed round's loop is walked for its other
/// rail; a loop longer than this is no round of two supports and has none.
const MAX_COLLAPSED_ROUND_SIDES: usize = 64;

/// (not upstream, solidcraft#16983) An extended edge's trim interval on its curve, and
/// whether its offset is inverted: the trim point of the base edge use's start vertex
/// lies past the one of its end vertex, as the edge use runs. The two faces' offsets
/// beside the edge have crossed over it (a wall thicker than half a rib between them
/// moves each side's offset past the other's), so the edge's offset runs from its start
/// vertex's image to its end vertex's image against the base edge. Its curve is set that
/// way ([`OffsetGeometryCreation::replace_edge_curves`] with `inverted`), so that the
/// edge still ends where its neighbours' offsets do; the merge resolves the faces that
/// turned inside out with it.
#[derive(Debug, Clone)]
struct TrimmedEdge<O> {
    /// The trim interval (increasing).
    interval: Extent1d,
    /// The trim points run against the base edge use.
    inverted: bool,
    /// Material boundaries reached through a chart seam that disappears.
    periodic_ends: Vec<PeriodicBoundaryEnd<O>>,
}

/// An intersection end whose old chart seam no longer meets the intersection.
/// The end instead meets the genuine boundary beyond that seam. All handles name
/// the original B-rep; `point` is on the displaced supports.
#[derive(Debug, Clone, Copy)]
pub struct PeriodicBoundaryEnd<O> {
    /// The genuine original intersection edge use being rebuilt.
    pub edgeuse: O,
    /// Its original endpoint.
    pub vertex: O,
    /// The original same-face seam edge use to remove from this boundary.
    pub seam: O,
    /// The genuine original boundary edge use beyond the seam, on its face.
    pub across: O,
    /// The new intersection endpoint on that boundary's displaced support.
    pub point: Point3,
}

enum BoundaryRoot {
    NoCut,
    /// The root's curve parameter, its point, and its (u, v) on the boundary's displaced,
    /// extended support.
    One(f64, Point3, Point2),
    Unresolved,
}

enum PeriodicEnd<O> {
    Ordinary,
    Root(f64, Vec<PeriodicBoundaryEnd<O>>),
    Unresolved,
}

/// The most whole turns `exact_piece` moves a conic's angle by (a guard: the angles
/// are within a few turns of each other).
const MAX_EXACT_PIECE_TURNS: u32 = 1 << 16;

/// The `operation` of the [`SolidError::InvalidInput`] this module raises where upstream
/// dereferences a null pointer or a failed cast (undefined behaviour), so that a runner
/// tells it apart from a real `SM_ERR_INVALID_INPUT` status of a callee.
pub const UNDEFINED_UPSTREAM: &str =
    "offset::offset_geometry_creation (undefined behaviour upstream)";

/// On a common exact unit axis, positive-nappe cones have radial laws
/// `rho = c_i + t_i z`. Unequal signed slopes admit at most one meeting height,
/// hence one physical circle. The reach certificate supplies a regular meeting;
/// SSI pieces can only be fragments of that circle, never competing branches.
/// Failed exact axis proofs retain the refusal. This is used only on crossings
/// certified by reach, and checks the new supports' analytic classes as well.
#[allow(clippy::float_cmp)] // equality of stored coefficients is the exact hypothesis
fn unique_cone_meeting(k: &Kernel, first: &Surface, second: &Surface) -> Result<bool> {
    let mut cones = Vec::new();
    for s in [first, second] {
        let Some(label) = AnalyticSupport::of(s) else {
            return Ok(false);
        };
        let Implicit::Cone {
            origin,
            axis,
            radius,
            tan_angle,
        } = label.implicit()
        else {
            return Ok(false);
        };
        if !radius.is_finite() || !tan_angle.is_finite() {
            return Ok(false);
        }
        match AnalyticSupport::check(k, s) {
            Ok(_) => {}
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => return Ok(false),
        }
        cones.push((origin, axis, tan_angle));
    }
    let [(o1, a1, t1), (o2, a2, t2)] = cones.as_slice() else {
        return Ok(false);
    };
    let sign = if a1 == a2 {
        1.0
    } else if *a1 == -*a2 {
        -1.0
    } else {
        return Ok(false);
    };
    if *t1 == sign * *t2 {
        return Ok(false);
    }
    let Some(norm) = exact::dot(*a1, *a1) else {
        return Ok(false);
    };
    if !exact::is_zero(&exact::sub(&norm, &[1.0])) {
        return Ok(false);
    }
    let (Some(c1), Some(c2)) = (exact::cross(*o1, *a1), exact::cross(*o2, *a1)) else {
        return Ok(false);
    };
    Ok(c1
        .iter()
        .zip(&c2)
        .all(|(x, y)| exact::is_zero(&exact::sub(x, y))))
}

/// When [`OffsetGeometryCreation::replace_edge_curves`]'s shortcut keeps an edge whose
/// curve's ends match the new curve's.
#[derive(Clone, Copy)]
enum KeepOld {
    /// Always (upstream).
    OnMatchingEnds,
    /// As decided by the caller (not upstream: `MateEdges`, whose new curve is the surfaces'
    /// intersection, keeps an offset edge only where it is proved on both surfaces; see
    /// [`OffsetGeometryCreation::replace_edge_curves_in`]).
    Decided(bool),
}

/// A dereference of a null pointer or a cast that fails upstream (undefined
/// behaviour): `InvalidInput` with [`UNDEFINED_UPSTREAM`] as operation.
const fn dangling(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: UNDEFINED_UPSTREAM,
        reason,
    }
}

/// The failure of `operation` for `cause`, with the cause's status (that of the site it
/// replaced: `SM_ERR`, or `SM_ERR_NULL_POINTER` for an `NER`).
#[track_caller]
fn failed(operation: &'static str, cause: OffsetFailure) -> SolidError {
    Failure::new(operation, cause).into_error()
}

/// A defect of `operation` ([`OffsetFailure::Defect`]) with `status`, the status of the site
/// it replaced (`SM_ERR`, or `SM_ERR_NULL_POINTER` for an `NER`).
#[track_caller]
fn defect(operation: &'static str, why: &'static str, status: i32) -> SolidError {
    Failure::new(operation, OffsetFailure::Defect { why })
        .with_status(status)
        .into_error()
}

/// The edge of the operation's input behind the handle `h`, where it names one (error
/// context only).
fn input_edge<O>(geometry: &impl ClassGeometry<O>, h: O) -> Option<EdgeId> {
    match geometry.topology_entity(h) {
        Some((_, EntityRef::Edge(e))) => Some(e),
        _ => None,
    }
}

/// The face of the caller's body behind the handle `h` has no surface (`edgeuse_offset`,
/// `SM_ERR_NULL_POINTER`): [`OffsetFailure::FaceHasNoSurface`] at the face, where the handle
/// names one ([`OffsetFailure::UnnamedFaceHasNoSurface`] where it does not).
#[track_caller]
fn face_without_surface<O>(geometry: &impl ClassGeometry<O>, h: O) -> SolidError {
    let cause = match geometry.topology_entity(h) {
        Some((_, EntityRef::Face(face))) => OffsetFailure::FaceHasNoSurface { face },
        _ => OffsetFailure::UnnamedFaceHasNoSurface,
    };
    failed(OP_EDGEUSE, cause)
}

// Why each step that fails here is the kernel's fault (`OffsetFailure::Defect`).

/// Why a curve evaluated with its first derivative has none
/// ([`make_edges_on_analytics_exact`]).
const NO_TANGENT: &str = "the evaluation of the exact piece and of the edge's curve with one \
     derivative succeeded but returned no tangent, which an evaluation that succeeds returns";
/// Why the jets of `passes_through` are short.
const SHORT_JET: &str = "the evaluation of the curve to its second derivative, or of the surface \
     to its second-order jet, succeeded but returned fewer derivatives than asked for, which an \
     evaluation that succeeds returns";
/// Why the offset surface is missing ([`OffsetGeometryCreation::faceuse_offset`]).
const OFFSET_SURFACE_COLLAPSED_LATE: &str = "the offset surface's construction found the offset \
     collapses, though `offset_collapses`, asked just before with the same distance and \
     tolerance, said it does not";
/// Why a bounding edge of a vertex has no curve ([`OffsetGeometryCreation::vertex_offset`]).
const BOUNDING_EDGE_WITHOUT_CURVE: &str = "an offset edge bounding the vertex has no curve, \
     though the offset made each such edge with its curve";
/// Why an intersection's UV curve is of no B-spline kind ([`OffsetGeometryCreation::mate_edges`]).
const SSI_UV_CURVE_KIND: &str = "a UV curve of an intersection of the extended surfaces is of no \
     B-spline curve kind, though the intersection gives its UV curves as B-spline curves \
     (marched, or dropped from an exact piece)";
/// Why a matched shell-tangent rail does not span its edge.
const MATCHED_RAIL_OFF_EDGE: &str = "a rail made from the offset edge's own curve trimmed to the \
     edge's interval does not span that interval, though the edge's ends were matched to its \
     settled vertices";

/// A status upstream ignores (whatever it is, `SM_ERR_INVALID_INPUT` included), except
/// the errors that cannot be ignored here: unported callees and undefined behaviour
/// upstream ([`dangling`]).
fn ignore_soft<T>(r: Result<T>) -> Result<Option<T>> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(
            e @ (SolidError::Unported { .. }
            | SolidError::Cancelled { .. }
            | SolidError::InvalidInput {
                operation: UNDEFINED_UPSTREAM,
                ..
            }),
        ) => Err(e),
        Err(_) => Ok(None),
    }
}

/// A trim miss can leave an edge for the later merge. A failed proof of which
/// branch to retain cannot: preserve its reason before the unmated-edge path. Nor can a
/// defect (the kernel's fault, [`SolidError::is_defect`]): it is passed on with its reason,
/// never read as an edge that does not mate.
fn mating_result<T>(r: Result<T>) -> Result<Option<T>> {
    match r {
        Err(e)
            if matches!(
                e.failure().map(|f| &f.reason),
                Some(Reason::Offset(
                    OffsetFailure::ExactCurveUncut
                        | OffsetFailure::TangentBranchUncertified
                        | OffsetFailure::PeriodicBranchAmbiguous
                ))
            ) || is_tangent_end_off_curve(&e)
                || e.is_defect() =>
        {
            Err(e)
        }
        r => ignore_soft(r),
    }
}

/// Whether `e` is the refusal of an end beside a crossing tangent edge
/// ([`OffsetFailure::TangentEndOffCurve`]).
fn is_tangent_end_off_curve(e: &SolidError) -> bool {
    matches!(
        e.failure().map(|f| &f.reason),
        Some(Reason::Offset(OffsetFailure::TangentEndOffCurve))
    )
}

/// Whether `e` is the refusal of a tangent edge whose new faces no longer meet along it
/// ([`OffsetFailure::LooseTangentEdge`]).
fn is_loose_tangent_edge(e: &SolidError) -> bool {
    matches!(
        e.failure().map(|f| &f.reason),
        Some(Reason::Offset(OffsetFailure::LooseTangentEdge { .. }))
    )
}

/// `SmTArray::FindElement`.
fn contains<O: PartialEq>(list: &[O], item: &O) -> bool {
    list.iter().any(|x| x == item)
}

// ---- the data types -----------------------------------------------------------------------

/// The surf/surf intersection curves of a surface (`SmSSIData`): parallel arrays
/// of 3D curves, their UV curves on each surface, kinds and deviations.
#[derive(Debug, Clone, PartialEq)]
pub struct SsiData<O> {
    /// `m_v3DCurves`.
    pub curves_3d: Vec<O>,
    /// `m_vUVCurves1`.
    pub uv_curves1: Vec<Option<O>>,
    /// `m_vUVCurves2`.
    pub uv_curves2: Vec<Option<O>>,
    /// `m_vCurveTypes`.
    pub curve_types: Vec<TsectCurveType>,
    /// `m_vDeviations`.
    pub deviations: Vec<f64>,
}

impl<O> Default for SsiData<O> {
    fn default() -> Self {
        Self {
            curves_3d: Vec::new(),
            uv_curves1: Vec::new(),
            uv_curves2: Vec::new(),
            curve_types: Vec::new(),
            deviations: Vec::new(),
        }
    }
}

impl<O> SsiData<O> {
    /// Empties every array (the `ReSet` calls at the start of the offsets).
    pub fn reset(&mut self) {
        self.curves_3d.clear();
        self.uv_curves1.clear();
        self.uv_curves2.clear();
        self.curve_types.clear();
        self.deviations.clear();
    }
}

/// What [`OffsetGeometryCreation::vertex_offset`] returns.
#[derive(Debug, Clone, PartialEq)]
pub struct VertexOffsetGeometry<O> {
    /// `r3DTrimmingCurves`.
    pub trimming_curves: Vec<O>,
    /// `rOrients`.
    pub orients: Vec<OrientType>,
    /// `rpNewSurface`.
    pub surface: Option<O>,
}

/// What [`OffsetGeometryCreation::edgeuse_offset`] returns.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeuseOffset<O> {
    /// `rpNewSurface`.
    pub surface: Option<O>,
    /// `r3DTrimmingCurves`.
    pub trimming_curves: Vec<O>,
    /// `rTrimOrientations`.
    pub trim_orientations: Vec<OrientType>,
    /// `rSelfIntersections` (never filled: the self-intersection block is `#if 0`).
    pub self_intersections: SsiData<O>,
    /// Not upstream (`*pbOptClosingWall`, the WR-530 addendum's step walls,
    /// fork tangency-labs/smlib 5d428ae): `surface` is a step wall, which spans the gap
    /// between two edges and is never extended (as a lamina side wall).
    pub closing_wall: bool,
}

/// What [`OffsetGeometryCreation::replace_edge_curves`] returns: whether the caller
/// should delete the candidate 3D curve and UV curve (`rbDelete3DCurve`,
/// `rbDeleteUVCurve`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplaceEdgeCurves {
    /// `rbDelete3DCurve`.
    pub delete_3d_curve: bool,
    /// `rbDeleteUVCurve`.
    pub delete_uv_curve: bool,
}

/// Sample the two geometry services in their original order; a no-solution closest
/// point is a successful query and does not establish a gap.
fn tangent_gap_samples<B>(
    k: &Kernel,
    brep: &mut B,
    num_samples: u64,
    tol: f64,
    mut evaluate: impl FnMut(&Kernel, &mut B, f64) -> Result<Vec3>,
    mut closest: impl FnMut(&Kernel, &mut B, Vec3) -> Result<Option<f64>>,
) -> Result<bool> {
    for i in 0..=num_samples {
        let param = (i as f64 * 1.0) / (num_samples as f64 * 1.0);
        // fork 4803d6d: a sample that cannot be evaluated shows no gap
        let Some(pnt) = ignore_soft(evaluate(k, brep, param))? else {
            continue;
        };
        // `ClosestPoint`'s status is dropped (the fork keeps it so: its `bSuccess`
        // says whether a closest point was found)
        let cp = ignore_soft(closest(k, brep, pnt))?;
        if let Some(Some(distance)) = cp {
            if distance > tol {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

// ---- what the unit needs from the executive and the B-reps --------------------------------

/// What this unit uses of the offset executive (`SmOffsetExecutive`, ported as
/// [`crate::offset::offset_executive::p1::OffsetExecutive`]): its stored flags, the
/// faces to shell, the relation of original edge uses to offset edge uses and its
/// extended-surface cache, over this unit's object handles `O`. The executive's owner
/// implements it on top of the ported struct, mapping handles to its typed ids. The
/// tangency tolerance is not here: upstream's executive calls back into the geometry
/// creation (`m_rGeometryCreation.GetTangencyTolDegrees()`), so this module reads its own.
///
/// # Implementing this trait
///
/// As for [`OffsetBrep`], the only implementation today is the difftest runner's (`Exec`,
/// wrapping the ported struct). An adapter keeps upstream's identities: the extension
/// cache is keyed by the base surface's identity (upstream's pointer; one key per
/// surface object, never per copy), a lookup that hits returns the cached surface's
/// handle (the runner seeds the cache with each surface mapped to itself, as the C++
/// shim seeds `m_vSurfExtSurf`, so a hit returns the base handle), and an extension made
/// on a miss is stored once and its handle returned again on later hits.
/// [`OffsetExecutive::offset_edgeuse`] follows `GetOffsetEdgeuse`'s exactly-one rule.
pub trait OffsetExecutive<O> {
    /// A genuine boundary whose paired curves were successfully installed.
    fn has_mated_edgeuse(&self, original: O) -> bool {
        let _ = original;
        false
    }
    /// Records completion, rather than a traversal mark, of an edge mitre.
    fn note_mated_edgeuse(&mut self, original: O) -> Result<()> {
        let _ = original;
        Ok(())
    }
    /// Records a selected intersection end that passes an obsolete periodic seam.
    /// Candidate curves never notify this: only the installed intersection does.
    fn note_periodic_boundary_end(&mut self, event: PeriodicBoundaryEnd<O>) -> Result<()> {
        let _ = event;
        Err(SolidError::Unsupported {
            operation: OP_TRIM,
            reason: "the offset adapter cannot reconstruct a periodic boundary event",
        })
    }
    /// `GetInset`.
    fn inset(&self) -> bool;
    /// `GetExtendConvexEdges`.
    fn extend_convex_edges(&self) -> bool;
    /// `GetShellFaces`: the faces to shell rather than offset (empty without a list).
    fn shell_faces(&self) -> Vec<O>;
    /// `GetOffsetOperation() == SM_OO_LOCAL_OPERATION`: whether the executive runs a
    /// local operation (`SmBrep::LocalOperation`); `false` unless the adapter says so.
    fn is_local_operation(&self) -> bool {
        false
    }
    /// Not upstream: whether original edge use `original`'s edge keeps its geometry in a
    /// local operation (its faces and every face at its vertices keep their surfaces):
    /// its offsets keep their faces' own curves, not mated again; `false` unless the
    /// adapter says so.
    fn keeps_edge(&self, original: O) -> bool {
        let _ = original;
        false
    }
    /// Not upstream: whether the new surfaces of a local operation cross along original
    /// edge use `original`'s edge (certified on every normal section,
    /// `reach::LocalReach::crossing`), though its faces may have been tangent in the
    /// input; `false` unless the adapter says so.
    fn crosses_at(&self, original: O) -> bool {
        let _ = original;
        false
    }
    /// Not upstream (solidcraft#16983): notes offset face `offset_face`, which has an
    /// edge whose offset is inverted (see `TrimmedEdge`); nothing unless the adapter
    /// keeps it.
    fn note_inverted_face(&mut self, offset_face: O) {
        let _ = offset_face;
    }
    /// Not upstream (WR-346): whether any inset face's offset collapsed (a convex round
    /// thinner than the wall).
    fn has_collapsed_faces(&self) -> bool {
        false
    }
    /// Not upstream (WR-346, `IsCollapsedFace`): whether original face `face` is an
    /// inset face whose offset collapsed.
    fn is_collapsed_face(&self, face: O) -> bool {
        let _ = face;
        false
    }
    /// `GetOffsetEdgeuse`: the offset edge use related to `original`, only when exactly
    /// one is (`m_vEUToEU` is a many-to-many `SmRelation`; none or several give `None`).
    fn offset_edgeuse(&self, original: O) -> Option<O>;
    /// Not upstream (WR-344 refinement, tangency-labs/solidcore#1795): `EdgeuseOffset`
    /// could not mate the offsets of `original`'s edge (`MateEdges` failed, which upstream
    /// only reports), so the merge must still intersect them. The default ignores it.
    fn note_unmated_edgeuse(&mut self, original: O) {
        let _ = original;
    }
    /// `GetCreateExtendedSurface(pBase, rUVDomainOfOffset, rpExtSurface)`: the extension
    /// of `base` cached by the executive (`m_vSurfExtSurf`), created on a miss by
    /// extending `base` exactly ruled by its face's offset reach (not upstream: upstream
    /// extends by `m_rGeometryCreation.GetOffsetDistance() * 5.0`) with
    /// [`crate::offset::offset_executive::p1::OffsetExecutive::get_create_extended_surface`],
    /// and its natural UV domain. A newly made extension is stored where `brep` resolves
    /// the returned handle; a hit returns the cached handle again.
    ///
    /// # Errors
    ///
    /// The extension's errors, and [`SolidError::InvalidInput`] for a `base` that does not
    /// resolve to a surface (upstream `NER(pBase)`).
    fn get_create_extended_surface<B: OffsetBrep<O>>(
        &mut self,
        k: &Kernel,
        brep: &mut B,
        base: O,
    ) -> Result<(O, Extent2d)>;
    /// The margin the analytic copy of `base` is extended by for the exact mate
    /// (`Kernel::fixes.offset_analytic_mate_edges`): the one its extension
    /// ([`Self::get_create_extended_surface`]) is made with, so the copy holds every piece
    /// marched on that extension. The default is `upstream`, the fork's five offset
    /// distances (an implementation compared against it keeps it); the executive's
    /// adapter gives its face's recorded reach (not upstream).
    fn analytic_extension_margin(&self, base: O, upstream: f64) -> f64 {
        let _ = base;
        upstream
    }
}

/// What the unit reads from and writes to the original and offset B-reps,
/// implemented by their owner on top of [`ClassGeometry`] / [`ProtoGeometry`] with
/// the ported topology functions each method names. A method returns `Ok(None)` or
/// [`SolidError::InvalidInput`] where upstream would dereference null.
///
/// # Implementing this trait
///
/// The only implementation today is the `offset.offset_geometry_creation` difftest
/// runner's (`Two` in `crates/difftest/src/units/offset_offset_geometry_creation.rs`),
/// which is compared against upstream; a library-side adapter (for
/// [`crate::offset::offset_executive::p1::OffsetExecutive`]'s `EdgeuseOffset` /
/// `VertexOffset` calls) must make the same choices where a method's upstream call has
/// side effects:
/// - For [`OffsetBrep::edge_set_curve`], a copy of the curve through
///   `topology::edge::set_curve` with `SetCurveOptions { delete_old_curve: true,
///   dbg_warn_leaks: true, call_notify: true }` and the B-rep's notification environment
///   (`topology::edge::NotifyEnv`: its topology records, attribute store and object
///   keys), then `Curve::set_owner` on the edge's new curve with the edge's key.
/// - For [`OffsetBrep::set_edge_tolerance`], `topology::edge::set_tolerance(.., true, true)`
///   (`bUpdateOnlyIfLarger`, `bCascadeToBndries`: upstream's defaults).
/// - For [`OffsetBrep::edgeuse_set_uv_trim_curve`], `topology::edgeuse::p1::set_uv_trim_curve`
///   with a new curve (or null), the given distance and `true` (delete the existing UV
///   trim curve), no drop-curve record.
/// - For [`OffsetBrep::set_vertex_point`], `topology::vertex::set_point` with the same
///   notification environment (vertex uses keyed as the B-rep's topology records).
/// - For [`OffsetBrep::insert_curve`] / [`OffsetBrep::insert_surface`], new objects get
///   handles of their own; they are never attached to topology by the store.
/// - The surfaces of faces keep their face as owner (`Surface::set_owner`), so that a
///   trimmed surface cache built on them reaches the face as upstream's does.
#[allow(clippy::too_many_arguments)]
pub trait OffsetBrep<O>: ProtoGeometry<O> {
    /// Prepares the surface cache when a numerical solve actually requests it.
    /// Backends with owning topology override this to build the real trimmed
    /// cache; the default retains the existing opaque-owner deferral.
    fn prepare_surface_solve_cache(
        &mut self,
        k: &Kernel,
        surface: &Surface,
        handle: Option<O>,
    ) -> Result<SurfaceCache<O>>
    where
        O: Copy + PartialEq,
    {
        let mut cache = crate::surface::surface::p1::get_surface_cache(k, surface, handle)?;
        // the solve's GetTree: a reused cache whose tree is not built builds it now
        if cache.tree().is_none() {
            cache.get_tree(k)?;
        }
        Ok(cache)
    }

    /// The state bits of the surface cache as `smsurf_GetSurfaceCache` hands it to a
    /// reader of them only, without the solve's `GetTree` (`GlobalPointSolve`'s
    /// fast-drop face test). Backends that keep face caches override this; the default
    /// is the solve's cache's.
    fn prepare_surface_state_cache(
        &mut self,
        k: &Kernel,
        surface: &Surface,
        handle: Option<O>,
    ) -> Result<SurfaceCacheState<O>>
    where
        O: Copy + PartialEq,
    {
        self.prepare_surface_solve_cache(k, surface, handle)?
            .state()
    }

    /// Read-only face queries after cache preparation. One service resolves all
    /// owning B-reps in this operation's object space.
    fn prepared_face_queries(&self) -> Option<&dyn FaceQueries<O>> {
        None
    }

    /// Stores a new curve and returns its handle; `None` when the store is full.
    fn insert_curve(&mut self, curve: Curve) -> Option<O>;
    /// Stores a new surface and returns its handle; `None` when the store is full.
    fn insert_surface(&mut self, surface: Surface) -> Option<O>;
    /// Mutable access to the surface behind a handle.
    fn surface_mut(&mut self, o: O) -> Option<&mut Surface>;
    /// `GetBrep()->GetTolerance()` of a topology object (`Brep::tolerance`).
    fn brep_tolerance(&self, topo: O) -> Result<f64>;
    /// `SmFaceuse::GetFace` (`topology::edgeuse::p1::faceuse_face`).
    fn faceuse_face(&self, fu: O) -> Result<Option<O>>;
    /// `SmFaceuse::GetOrientation` (`topology::edgeuse::p1::faceuse_orientation`).
    fn faceuse_orientation(&self, fu: O) -> Result<OrientType>;
    /// `SmFace::GetSurface` (`topology::face::p2::surface`).
    fn face_surface(&self, face: O) -> Result<Option<O>>;
    /// `SmFace::GetUVDomain` (`topology::face::p2::uv_domain`).
    fn face_uv_domain(&self, face: O) -> Result<Extent2d>;
    /// `SmFace::SetUVDomain` (`topology::face::p2::set_uv_domain`).
    fn set_face_uv_domain(&mut self, face: O, domain: Extent2d) -> Result<()>;
    /// `SmFace::CalculateUVDomainFromUVTrimCurves(rDomain)` with upstream's defaults
    /// (`bLimitDomain = TRUE`, no loop gaps;
    /// `topology::face::p1::calculate_uv_domain_from_uv_trim_curves`). It makes the face's
    /// UV trim curves.
    fn face_uv_domain_from_uv_trim_curves(&mut self, k: &Kernel, face: O) -> Result<Extent2d>;
    /// `SmEdge::GetCurve` (`topology::edge::curve`).
    fn edge_curve(&self, edge: O) -> Result<Option<O>>;
    /// `SmEdge::GetInterval` (`topology::edge::interval`).
    fn edge_interval(&self, edge: O) -> Result<Extent1d>;
    /// `SmEdge::SetInterval` (`topology::edge::set_interval`).
    fn set_edge_interval(&mut self, edge: O, interval: Extent1d) -> Result<()>;
    /// `SmEdge::GetTolerance` (`topology::edge::tolerance`).
    fn edge_tolerance(&self, edge: O) -> Result<f64>;
    /// `SmEdge::SetTolerance(tol)` with upstream's defaults (`topology::edge::set_tolerance`).
    fn set_edge_tolerance(&mut self, k: &Kernel, edge: O, tolerance: f64) -> Result<()>;
    /// `SmEdge::SetCurve(curve, TRUE)`, `curve->SetOwner(edge)` (`topology::edge::set_curve`,
    /// `Curve::set_owner`): the edge takes the curve (its old curve and the edge
    /// uses' UV trim curves are dropped).
    fn edge_set_curve(&mut self, k: &Kernel, edge: O, curve: O) -> Result<()>;
    /// `SmBrep::MateLaminaEdgeToSurface(pEdge, crSurface, dSurfaceExtensionFactor,
    /// dUVCurveExtensionFactor)` of the edge's B-rep (`topology::brep::p2`, with the
    /// owner's arenas and notification state).
    fn brep_mate_lamina_edge_to_surface(
        &mut self,
        k: &Kernel,
        edge: O,
        surface: O,
        surface_extension_factor: f64,
        uv_curve_extension_factor: f64,
    ) -> Result<()>;
    /// `SmBrep::ReplaceSurface(pFace, pNewSurface, bAddToDeletedList, NULL,
    /// bCreateUVTrimCurves)` of the face's B-rep (`topology::brep::p2`): the face takes
    /// the surface behind `surface`.
    fn brep_replace_surface(
        &mut self,
        k: &Kernel,
        face: O,
        surface: O,
        add_to_deleted_list: bool,
        create_uv_trim_curves: bool,
    ) -> Result<()>;
    /// `SmEdge::GetFaces` (`topology::edge::faces`).
    fn edge_faces(&self, edge: O) -> Result<Vec<O>>;
    /// `SmEdge::GetEdgeuses` (`topology::edge::edgeuses`).
    fn edge_edgeuses(&self, edge: O) -> Result<Vec<O>>;
    /// `SmEdge::IsClosed` (`topology::edge::is_closed`).
    fn edge_is_closed(&self, edge: O) -> Result<bool>;
    /// `SmVertex::GetEdges` (`topology::vertex::edges`, no face filter).
    fn vertex_edges(&self, vertex: O) -> Result<Vec<O>>;
    /// `SmFaceuse::GetShell` (`topology::edgeuse::p1::faceuse_shell`).
    fn faceuse_shell(&self, fu: O) -> Result<Option<O>>;
    /// `SmEdge::IsLamina` (`topology::edge::is_lamina`).
    fn edge_is_lamina(&self, edge: O) -> Result<bool>;
    /// `SmEdge::IsSeam(*face->GetSurface())` (`topology::edge::is_seam_of_surface`).
    fn edge_is_seam_of_face_surface(&mut self, k: &Kernel, edge: O, face: O) -> Result<bool>;
    /// `SmEdge::GetVertex` (`topology::edge::vertex`).
    fn edge_vertex(&self, edge: O) -> Result<Option<O>>;
    /// `SmEdge::GetOtherVertex` (`topology::edge::other_vertex`).
    fn edge_other_vertex(&self, edge: O, vertex: O) -> Result<Option<O>>;
    /// `SmEdge::ClosestPoint` (`query::edge::closest_point`): `None` without a
    /// solution (`rbSuccess == FALSE`).
    fn edge_closest_point(
        &self,
        k: &Kernel,
        edge: O,
        point: Point3,
    ) -> Result<Option<ClosestPoint>>;
    /// `SmEdge::CreateTrimmedNURBSCurve` (`topology::edge::create_trimmed_nurbs_curve`):
    /// the handle of the new curve, `None` when the edge's curve is not a B-spline.
    fn edge_create_trimmed_nurbs_curve(&mut self, k: &Kernel, edge: O) -> Result<Option<O>>;
    /// `SmVertex::GetPoint` (`topology::vertex::point`).
    fn vertex_point(&self, vertex: O) -> Result<Point3>;
    /// `SmVertex::GetTolerance` (`topology::vertex::tolerance`).
    fn vertex_tolerance(&self, vertex: O) -> Result<f64>;
    /// `SmVertex::SetPoint` (`topology::vertex::set_point`).
    fn set_vertex_point(&mut self, k: &Kernel, vertex: O, point: Point3) -> Result<()>;
    /// `SmVertex::IsLaminaVertex` (`topology::vertex::is_lamina_vertex`).
    fn vertex_is_lamina(&self, vertex: O) -> Result<bool>;
    /// `SmVertex::AreEdgesTangent(e1, e2, angTolDeg)` (`topology::vertex::are_edges_tangent`).
    fn vertex_are_edges_tangent(&self, vertex: O, e1: O, e2: O, ang_tol_deg: f64) -> Result<bool>;
    /// `SmEdgeuse::GetEdge` (`topology::edgeuse::p1::edge`).
    fn edgeuse_edge(&self, eu: O) -> Result<Option<O>>;
    /// `SmEdgeuse::GetMate` (`topology::edgeuse::p1::mate`).
    fn edgeuse_mate(&self, eu: O) -> Result<Option<O>>;
    /// `SmEdgeuse::GetRadial` (`topology::edgeuse::p1::radial`).
    fn edgeuse_radial(&self, eu: O) -> Result<Option<O>>;
    /// `SmEdgeuse::GetCWEdgeuse` (`topology::edgeuse::p1::cw_edgeuse`).
    fn edgeuse_cw(&self, eu: O) -> Result<Option<O>>;
    /// `SmEdgeuse::GetCCWEdgeuse` (`topology::edgeuse::p1::ccw_edgeuse`).
    fn edgeuse_ccw(&self, eu: O) -> Result<Option<O>>;
    /// `SmEdgeuse::GetVertexuse()->GetVertex()` (`topology::edgeuse::p1::vertexuse`,
    /// `Vertexuse::vertex`).
    fn edgeuse_vertex(&self, eu: O) -> Result<Option<O>>;
    /// `SmEdgeuse::GetFace` (`topology::edgeuse::p1::face`).
    fn edgeuse_face(&self, eu: O) -> Result<Option<O>>;
    /// `SmEdgeuse::GetFaceuse` (`topology::edgeuse::p1::faceuse`).
    fn edgeuse_faceuse(&self, eu: O) -> Result<Option<O>>;
    /// `SmEdgeuse::GetOrientation` (`topology::edgeuse::p1::orientation`), read only by
    /// the step walls (WR-530 addendum); an
    /// implementation that does not provide it fails there.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] unless implemented.
    fn edgeuse_orientation(&self, eu: O) -> Result<OrientType> {
        let _ = eu;
        Err(dangling("edge use orientation not provided"))
    }
    /// `SmEdgeuse::GetUVTrimCurve` (`topology::edgeuse::p1::uv_trim_curve`): the handle
    /// of the UV trim curve, if any.
    fn edgeuse_uv_trim_curve(&self, eu: O) -> Result<Option<O>>;
    /// `SmEdgeuse::SetUVTrimCurve(curve, maxDist, TRUE)`
    /// (`topology::edgeuse::p1::set_uv_trim_curve`, deleting the old curve). Not upstream
    /// (#18009): where `max_dist` raises the tolerance (it is below `2 * max_dist`) and `B`,
    /// the new trim's proved distance from the edge curve, is enclosed, the edge tolerance is
    /// raised to `B` where `B` is past `2 * max_dist` (the bound being the whole bisection's
    /// wherever it is above half the tolerance), and for `B` where it is below `max_dist` only
    /// if every other UV trim of the edge is enclosed within the lowered tolerance
    /// `max(tolerance, 2 * B)`; otherwise for `max_dist`, as upstream
    /// (`topology::edgeuse::p3::trim_raise_distance`).
    fn edgeuse_set_uv_trim_curve(
        &mut self,
        k: &Kernel,
        eu: O,
        curve: Option<O>,
        max_dist: f64,
    ) -> Result<()>;
    /// `SmEdgeuse::IsTangentSector(tolDeg)` with upstream's other defaults
    /// (`topology::edgeuse::p2::is_tangent_sector`).
    fn edgeuse_is_tangent_sector(&mut self, k: &Kernel, eu: O, tol_deg: f64) -> Result<bool>;
    /// `SmEdgeuse::IsConvexRadialSector(samples)` with upstream's other defaults
    /// (`topology::edgeuse::p2::is_convex_radial_sector`).
    fn edgeuse_is_convex_radial_sector(
        &mut self,
        k: &Kernel,
        eu: O,
        num_samples: u64,
    ) -> Result<bool>;
    /// `SmEdgeuse::IsMarked(eMarkType)` (`topology::topology::Topology::is_marked`).
    fn edgeuse_is_marked(&self, k: &Kernel, eu: O, mark: MarkType) -> Result<bool>;
    /// Not upstream: `SmEdgeuse::EvaluateBinormal` at the middle of the edge's interval
    /// (`topology::edgeuse::p3::evaluate_binormal`): the unit vector across the edge,
    /// perpendicular to the face use normal, pointing into the edge use's face.
    fn edgeuse_middle_binormal(&mut self, k: &Kernel, eu: O) -> Result<Vec3>;
    /// `SmEdgeuse::NormalizedEvaluate(t, bParameterSpaceEval, rPoint, pOptFirstDeriv,
    /// bOKToMakeUVTrimCurve)` (`topology::edgeuse::p3::normalized_evaluate`): the point
    /// (`want_first_deriv` is `pOptFirstDeriv != NULL`; the derivative is not returned).
    fn edgeuse_normalized_evaluate(
        &mut self,
        k: &Kernel,
        eu: O,
        t: f64,
        parameter_space_eval: bool,
        want_first_deriv: bool,
        ok_to_make_uv_trim_curve: bool,
    ) -> Result<Point3>;
    /// `SmEdgeuse::CreateUVTrimCurve(rdMaxDist, rpUVCurve, pdMaxDeviation,
    /// pOptDropCurveFail)` (`topology::edgeuse::p3::create_uv_trim_curve`): `rdMaxDist`
    /// and the new, unattached UV curve (`want_max_deviation` / `want_drop_curve_fail`:
    /// the optional out-parameters are non-null; their values are not returned).
    fn edgeuse_create_uv_trim_curve(
        &mut self,
        k: &Kernel,
        eu: O,
        want_max_deviation: bool,
        want_drop_curve_fail: bool,
    ) -> Result<(f64, Curve)>;
}

/// The topology and owned-geometry subset used by `VertexOffset`. Keeping this
/// interface separate lets the executive borrow the real source and result arenas
/// without implementing unrelated SSI, classification or editing operations.
pub(crate) trait VertexOffsetBrep<O> {
    /// The source vertex's B-rep tolerance, not the result B-rep's tolerance.
    fn brep_tolerance(&self, vertex: O) -> Result<f64>;
    /// `SmVertex::GetPoint` in the source arena.
    fn vertex_point(&self, vertex: O) -> Result<Point3>;
    /// `SmVertex::IsLaminaVertex` in the source arena.
    fn vertex_is_lamina(&self, vertex: O) -> Result<bool>;
    /// The result edge's incident faces in native traversal order.
    fn edge_faces(&self, edge: O) -> Result<Vec<O>>;
    /// `SmEdge::IsSeam(*face->GetSurface())` in the result arena.
    fn edge_is_seam_of_face_surface(&mut self, k: &Kernel, edge: O, face: O) -> Result<bool>;
    /// The result edge's curve handle.
    fn edge_curve(&self, edge: O) -> Result<Option<O>>;
    /// The result edge's parameter interval.
    fn edge_interval(&self, edge: O) -> Result<Extent1d>;
    /// Resolves a curve without replacing its concrete kind by its B-spline part.
    fn curve(&self, curve: O) -> Result<&Curve>;
    /// Stores an owned trimming curve.
    fn insert_curve(&mut self, curve: Curve) -> Result<O>;
    /// Stores the owned cap surface.
    fn insert_surface(&mut self, surface: Surface) -> Result<O>;
}

/// Adapts the existing full backend so public callers keep using the same path.
struct OffsetVertexBrep<'a, B>(&'a mut B);

impl<O: ClassObject, B: OffsetBrep<O>> VertexOffsetBrep<O> for OffsetVertexBrep<'_, B> {
    fn brep_tolerance(&self, vertex: O) -> Result<f64> {
        self.0.brep_tolerance(vertex)
    }

    fn vertex_point(&self, vertex: O) -> Result<Point3> {
        self.0.vertex_point(vertex)
    }

    fn vertex_is_lamina(&self, vertex: O) -> Result<bool> {
        self.0.vertex_is_lamina(vertex)
    }

    fn edge_faces(&self, edge: O) -> Result<Vec<O>> {
        self.0.edge_faces(edge)
    }

    fn edge_is_seam_of_face_surface(&mut self, k: &Kernel, edge: O, face: O) -> Result<bool> {
        self.0.edge_is_seam_of_face_surface(k, edge, face)
    }

    fn edge_curve(&self, edge: O) -> Result<Option<O>> {
        self.0.edge_curve(edge)
    }

    fn edge_interval(&self, edge: O) -> Result<Extent1d> {
        self.0.edge_interval(edge)
    }

    fn curve(&self, curve: O) -> Result<&Curve> {
        curve_of(self.0, curve)
    }

    fn insert_curve(&mut self, curve: Curve) -> Result<O> {
        store_curve(self.0, curve)
    }

    fn insert_surface(&mut self, surface: Surface) -> Result<O> {
        store_surface(self.0, surface)
    }
}

/// The narrow source/owned-geometry interface needed by `FaceuseOffset`.
pub(crate) trait FaceOffsetBrep<O> {
    /// `SmFaceuse::GetOrientation` in the source arena.
    fn faceuse_orientation(&self, faceuse: O) -> Result<OrientType>;
    /// `SmFaceuse::GetFace` in the source arena.
    fn faceuse_face(&self, faceuse: O) -> Result<Option<O>>;
    /// The source face's surface, preserving its concrete kind and owner.
    fn face_surface(&self, face: O) -> Result<Option<O>>;
    /// Resolves either source geometry or newly owned offset geometry.
    fn surface(&self, surface: O) -> Result<&Surface>;
    /// Stores a newly created surface without attaching it to topology.
    fn insert_surface(&mut self, surface: Surface) -> Result<O>;
    /// Stores a newly created intersection curve without changing its kind.
    fn insert_curve(&mut self, curve: Curve) -> Result<O>;
}

struct OffsetFaceBrep<'a, B>(&'a mut B);

impl<O: ClassObject, B: OffsetBrep<O>> FaceOffsetBrep<O> for OffsetFaceBrep<'_, B> {
    fn faceuse_orientation(&self, faceuse: O) -> Result<OrientType> {
        self.0.faceuse_orientation(faceuse)
    }

    fn faceuse_face(&self, faceuse: O) -> Result<Option<O>> {
        self.0.faceuse_face(faceuse)
    }

    fn face_surface(&self, face: O) -> Result<Option<O>> {
        self.0.face_surface(face)
    }

    fn surface(&self, surface: O) -> Result<&Surface> {
        surface_of(self.0, surface)
    }

    fn insert_surface(&mut self, surface: Surface) -> Result<O> {
        store_surface(self.0, surface)
    }

    fn insert_curve(&mut self, curve: Curve) -> Result<O> {
        store_curve(self.0, curve)
    }
}

// ---- callees through the B-rep, and unported callees ----------------------------------------
//
// Each helper carries every argument of the upstream call (literals and header defaults
// included, as named parameters) plus the kernel and the B-rep / executive it is called
// on, so wiring a stub changes no call site.

/// `pEdgeuse->NormalizedEvaluate(t, bParameterSpaceEval, rPoint, pOptFirstDeriv = NULL,
/// bOKToMakeUVTrimCurve = TRUE)`: the point at the normalised parameter (every call
/// here passes `FALSE`, a 3D evaluation, and no derivative).
fn edgeuse_normalized_evaluate<O>(
    k: &Kernel,
    brep: &mut impl OffsetBrep<O>,
    eu: O,
    t: f64,
    parameter_space_eval: bool,
    want_first_deriv: bool,
    ok_to_make_uv_trim_curve: bool,
) -> Result<Point3> {
    brep.edgeuse_normalized_evaluate(
        k,
        eu,
        t,
        parameter_space_eval,
        want_first_deriv,
        ok_to_make_uv_trim_curve,
    )
}

/// `pEdgeuse->CreateUVTrimCurve(rdMaxDist, rpUVCurve, pdMaxDeviation = NULL,
/// pOptDropCurveFail = NULL)`: the largest distance and the new UV curve (stored, not
/// attached to the edge use: upstream's caller owns it).
fn edgeuse_create_uv_trim_curve<O: ClassObject>(
    k: &Kernel,
    brep: &mut impl OffsetBrep<O>,
    eu: O,
    want_max_deviation: bool,
    want_drop_curve_fail: bool,
) -> Result<(f64, O)> {
    let (max_dist, curve) =
        brep.edgeuse_create_uv_trim_curve(k, eu, want_max_deviation, want_drop_curve_fail)?;
    Ok((max_dist, store_curve(brep, curve)?))
}

/// `pEdge->GetBrep()->MateLaminaEdgeToSurface(pEdge, crSurface, dTolMult = 1.5,
/// dMaxTolMult = 10.0)` (both values passed explicitly upstream).
fn brep_mate_lamina_edge_to_surface<O>(
    k: &Kernel,
    brep: &mut impl OffsetBrep<O>,
    edge: O,
    surface: O,
    tol_mult: f64,
    max_tol_mult: f64,
) -> Result<()> {
    brep.brep_mate_lamina_edge_to_surface(k, edge, surface, tol_mult, max_tol_mult)
}

/// `pFace->GetBrep()->ReplaceSurface(pFace, pNewSurface, bAddToDeleteSurfaceList = FALSE,
/// rOptModifiedFaces = NULL, bOptCreateUVTrimCurves = TRUE)`.
fn brep_replace_surface<O>(
    k: &Kernel,
    brep: &mut impl OffsetBrep<O>,
    face: O,
    surface: O,
    add_to_delete_surface_list: bool,
    want_modified_faces: bool,
    create_uv_trim_curves: bool,
) -> Result<()> {
    // rOptModifiedFaces = NULL: the modified faces are not wanted
    let _ = want_modified_faces;
    brep.brep_replace_surface(
        k,
        face,
        surface,
        add_to_delete_surface_list,
        create_uv_trim_curves,
    )
}

/// `SmSurface::GlobalCurveIntersect(domain, curve, interval, tol, solutions)` of the
/// surface and curve behind the handles (unit `surface.surface_p8`).
struct OffsetSurfaceSolveContext<'a, B>(&'a mut B);

impl<O: ClassObject, B: OffsetBrep<O>> SurfaceSolveContext<O> for OffsetSurfaceSolveContext<'_, B> {
    fn build_surface_cache(
        &mut self,
        k: &Kernel,
        surface: &Surface,
        handle: Option<O>,
    ) -> Result<SurfaceCache<O>> {
        self.0.prepare_surface_solve_cache(k, surface, handle)
    }

    fn surface_cache_without_get_tree(
        &mut self,
        k: &Kernel,
        surface: &Surface,
        handle: Option<O>,
    ) -> Result<SurfaceCacheState<O>> {
        self.0.prepare_surface_state_cache(k, surface, handle)
    }

    fn face_queries(&self) -> Option<&dyn FaceQueries<O>> {
        self.0.prepared_face_queries()
    }
}

fn surface_global_curve_intersect<O: ClassObject + SolutionObject>(
    k: &Kernel,
    geometry: &mut impl OffsetBrep<O>,
    surface: O,
    domain: &Extent2d,
    curve: O,
    interval: &Extent1d,
    tol: f64,
) -> Result<SolutionArray<O>> {
    let c = curve_of(geometry, curve)?.clone_for_evaluation()?;
    let s = surface_of(geometry, surface)?.clone_for_evaluation()?;
    // `GlobalCurveIntersect` reaches the surface's cache through
    // `smsurf_GetSurfaceCache` where it needs it: its degenerate-curve point solve and
    // coincidence check read the cache before its `GetTree` (the fast-drop face test
    // sees an unbuilt cache's state bits), so each step asks the context
    s.global_curve_intersect_with_context(
        k,
        [Some(curve), Some(surface)],
        None,
        &mut OffsetSurfaceSolveContext(geometry),
        domain,
        &c,
        interval,
        tol,
    )
}

/// Approximates a trimmed UV curve on a temporary offset surface, as
/// `SmCrvOnSurf::ApproximateCurve` in `EdgeuseOffset`. The opposite rail supplies
/// the breaks when present; otherwise the UV curve supplies them. Both temporary
/// curves stay owned here rather than being inserted into the caller's store.
fn offset_surface_rail<O: ClassObject>(
    k: &Kernel,
    geometry: &impl ClassGeometry<O>,
    surface: O,
    uv_curve: Curve,
    signed_distance: f64,
    owns_surface: bool,
    knots_source: Option<&BSplineCurve>,
    tol: f64,
    approximation: ApproximateCurveFlags,
) -> Result<BSplineCurve> {
    let base = surface_of(geometry, surface)?.clone_for_evaluation()?;
    let offset = OffsetSurface::new(k, signed_distance, base, owns_surface, OFFSET_SS_UNKNOWN)?;
    let mut rail = crate::curve::crv_on_surf::CrvOnSurf::new(
        k,
        uv_curve,
        Surface::new(SurfaceKind::OffsetSurface(offset)),
        None,
        0,
    )?;
    let knots = match knots_source {
        Some(curve) => curve.knots(None)?.0,
        None => rail.uv_curve().knots(k, None)?.0,
    };
    // SmCrvOnSurf::ApproximateCurve explicitly ignores these two flags upstream.
    let _ = (
        approximation.create_analytics,
        approximation.just_copy_b_splines,
    );
    rail.approximate_curve(k, &knots, tol, approximation.match_parameterization)
        .map(|(curve, _)| curve)
}

/// The three trailing flags of `SmCurve::ApproximateCurve(.., bOptCreateAnalytics,
/// bOptMatchParameterization, bJustCopyBSplines)`.
#[derive(Debug, Clone, Copy)]
struct ApproximateCurveFlags {
    /// `bOptCreateAnalytics`.
    create_analytics: bool,
    /// `bOptMatchParameterization`.
    match_parameterization: bool,
    /// `bJustCopyBSplines`.
    just_copy_b_splines: bool,
}

/// `SmCircularCrossSectionFSG sFSG(bApproximateArcs = FALSE, dTolPercentRadius = 0.05,
/// bMirror = FALSE, bComplement = FALSE)` then `sFSG.CreateSurfaceFromCurves(ctx,
/// *pCenterLine, *pRail1, *pRail2, tol, bNeedG1Continuity = FALSE, NULL, NULL, NULL, NULL,
/// pNewSurface)`; the four NULLs are the optional surfaces and UV curves of the rails
/// (`blend.fillet_standard_solver_p1`'s generator takes none). The curves are non-const
/// upstream, so they are `&mut`; the centre line is passed as the curve itself, so its
/// virtuals dispatch on its kind (`rCenterLine` binds the `SmBSplineCurve` kinds).
fn circular_cross_section_surface(
    k: &Kernel,
    fsg: CircularCrossSectionFsg,
    center_line: &mut Curve,
    rail1: &mut BSplineCurve,
    rail2: &mut BSplineCurve,
    tol: f64,
    need_g1_continuity: bool,
) -> Result<BSplineSurface> {
    fsg.create_surface_from_curves(k, center_line, rail1, rail2, tol, need_g1_continuity)
}

// ---- helpers over handles -------------------------------------------------------------------

/// `SM_CAST_PTR(SmBSplineCurve, curve)` on the curve behind `h`: `None` when the cast
/// fails (the curve is not an `SmBSplineCurve` kind); otherwise the curve itself, whose
/// methods dispatch on its real kind (a line, a conic or a B-spline), as the virtual
/// calls through the `SmBSplineCurve *` do upstream.
fn b_spline_kind_of<O: ClassObject>(
    geometry: &impl ClassGeometry<O>,
    h: O,
) -> Result<Option<&Curve>> {
    let c = curve_of(geometry, h)?;
    Ok(is_b_spline_kind(c).then_some(c))
}

/// The same cast through the vertex-only backend; concrete curve kinds stay intact.
fn vertex_b_spline_kind_of<O>(geometry: &impl VertexOffsetBrep<O>, h: O) -> Result<Option<&Curve>> {
    let c = geometry.curve(h)?;
    Ok(is_b_spline_kind(c).then_some(c))
}

/// Whether `SM_CAST_PTR(SmBSplineCurve, c)` succeeds: `c` is an `SmBSplineCurve` or one
/// of its derived kinds (line, ellipse, hyperbola, parabola).
fn is_b_spline_kind(c: &Curve) -> bool {
    matches!(
        c.kind(),
        CurveKind::BSpline(_)
            | CurveKind::Line(_)
            | CurveKind::Ellipse(_)
            | CurveKind::Hyperbola(_)
            | CurveKind::Parabola(_)
    )
}

/// The B-spline data of the curve behind `h` as a plain B-spline curve
/// ([`b_spline_part`]): `None` when it is not an `SmBSplineCurve` kind.
fn b_spline_of<O: ClassObject>(
    geometry: &impl ClassGeometry<O>,
    h: O,
) -> Result<Option<BSplineCurve>> {
    b_spline_part(curve_of(geometry, h)?)
}

/// `SmBSplineCurve::Copy(crContext, SmBSplineCurve*&)`, the non-virtual overload a
/// call on an `SmBSplineCurve*` binds: `new SmBSplineCurve(*this)`, a plain B-spline
/// copy of the curve's B-spline part whatever its kind (an `SmLine` or conic loses
/// its analytic data; a NURB that is not made yet stays unmade, as the copy
/// constructor copies only an existing one). A curve that is not an `SmBSplineCurve`
/// cannot be behind such a pointer.
fn b_spline_overload_copy(c: &Curve) -> Result<Curve> {
    Ok(Curve::from(match c.kind() {
        CurveKind::BSpline(b) => b.clone(),
        CurveKind::Line(l) => l.base.clone(),
        CurveKind::Ellipse(e) => e.base().clone(),
        CurveKind::Hyperbola(h) => h.base().clone(),
        CurveKind::Parabola(p) => p.base().clone(),
        _ => return Err(dangling("UV trim curve is not a B-spline curve")),
    }))
}

/// Not upstream (#17514): `extended`, an extended UV curve of an edge use on `surface`,
/// moved onto the side of the edge's other use on the same surface, `this` and `other`
/// being the two uses' own UV trim curves: by the step from the middle of `this` to the
/// middle of `other`. Where the two run over one edge, their middles are one point of it:
/// across a seam the step is the surface's period, and along the edge it is nought. The
/// control points move as points, a 2D curve staying 2D (`CPoint::translate`), so the
/// result keeps `extended`'s parameters.
///
/// `None` where the step does not carry `this` onto the same points of `surface` (at its
/// two ends and its middle, within `tol`): the two curves then do not run over one edge
/// (a pole's row and a meridian of a sphere), the moved curve would lie elsewhere, and
/// the caller drops the lifted curve as upstream does.
///
/// # Errors
///
/// Those of the evaluations; `SM_ERR` without a NURB.
fn same_surface_twin(
    extended: &BSplineCurve,
    this: &Curve,
    other: &Curve,
    surface: &Surface,
    tol: f64,
    k: &Kernel,
) -> Result<Option<Curve>> {
    let this_ivl = this.natural_interval();
    let this_mid = this.evaluate_point(k, this_ivl.evaluate(0.5))?;
    let other_mid = other.evaluate_point(k, other.natural_interval().evaluate(0.5))?;
    let step = other_mid - this_mid;
    for t in [this_ivl.min(), this_ivl.evaluate(0.5), this_ivl.max()] {
        let p = this.evaluate_point(k, t)?;
        let here = surface.evaluate_point(k, Point2::new(p.x, p.y))?;
        let moved = p + step;
        let there = surface.evaluate_point(k, Point2::new(moved.x, moved.y))?;
        if (there - here).length() > tol {
            return Ok(None);
        }
    }
    let mut twin = extended.copy();
    for pw in twin.get_or_create_gw_nurb()?.cpts_mut() {
        *pw = pw.translate(step);
    }
    Ok(Some(Curve::from(twin)))
}

/// `new SmBSplineCurve(*pCurve)`, or the `SmBSplineCurve` data a static B-spline
/// routine reads: a plain B-spline copy of the NURB part of an `SmBSplineCurve` kind
/// (a line's NURB is made when it has none); `None` for the other kinds. Only for the
/// sites where upstream copies or reads the base object, never for the virtual calls.
fn b_spline_part(c: &Curve) -> Result<Option<BSplineCurve>> {
    Ok(match c.kind() {
        CurveKind::BSpline(b) => Some(b.clone()),
        CurveKind::Line(l) => Some(l.base_with_nurb()?.into_owned()),
        CurveKind::Ellipse(e) => Some(e.base().clone()),
        CurveKind::Hyperbola(h) => Some(h.base().clone()),
        CurveKind::Parabola(p) => Some(p.base().clone()),
        _ => None,
    })
}

/// Like [`b_spline_part`], materialising the rational NURB on a clone when an analytic
/// conic (`Ellipse`, `Hyperbola`, `Parabola`) has cleared its lazy NURB (e.g. after
/// [`Curve::trim`]).
fn b_spline_with_nurb(k: &Kernel, c: &Curve) -> Result<Option<BSplineCurve>> {
    Ok(match c.kind() {
        CurveKind::BSpline(b) => Some(b.clone()),
        CurveKind::Line(l) => Some(l.base_with_nurb()?.into_owned()),
        CurveKind::Ellipse(e) if e.base().gw_nurb().is_none() => {
            let mut made = e.clone();
            made.make_nurb(k)?;
            Some(made.base().clone())
        }
        CurveKind::Ellipse(e) => Some(e.base().clone()),
        CurveKind::Hyperbola(h) if h.base().gw_nurb().is_none() => {
            let mut made = h.clone();
            made.make_nurb(None)?;
            Some(made.base().clone())
        }
        CurveKind::Hyperbola(h) => Some(h.base().clone()),
        CurveKind::Parabola(p) if p.base().gw_nurb().is_none() => {
            let mut made = p.clone();
            made.make_nurb(None)?;
            Some(made.base().clone())
        }
        CurveKind::Parabola(p) => Some(p.base().clone()),
        _ => None,
    })
}

/// The NURB part of an `SmBSplineCurve` kind, in place: the static
/// `SmBSplineCurve::MakeCurvesCompatible` edits the NURB of whatever kind it is given
/// (a line's NURB is made first when it has none); `None` for the other kinds.
fn b_spline_part_mut(c: &mut Curve) -> Result<Option<&mut BSplineCurve>> {
    Ok(match c.kind_mut() {
        CurveKind::BSpline(b) => Some(b),
        CurveKind::Line(l) => {
            let made = match l.base_with_nurb()? {
                std::borrow::Cow::Owned(b) => Some(b),
                std::borrow::Cow::Borrowed(_) => None,
            };
            if let Some(b) = made {
                *l.base_mut() = b;
            }
            Some(l.base_mut())
        }
        CurveKind::Ellipse(e) => Some(e.base_mut()),
        CurveKind::Hyperbola(h) => Some(h.base_mut()),
        CurveKind::Parabola(p) => Some(p.base_mut()),
        _ => None,
    })
}

/// The curve behind `h` (a dereference upstream).
fn curve_of<O: ClassObject>(geometry: &impl ClassGeometry<O>, h: O) -> Result<&Curve> {
    geometry
        .curve(h)
        .ok_or(dangling("curve handle cannot be resolved"))
}

/// The other parameter of the point at `t` on `curve` when the curve is closed
/// ([`Curve::is_closed`] within `tol`) and the point is its seam (within `tol` of the end of
/// the curve's interval nearer `t`): the other end. `None` for an open curve or a point off
/// the seam, which have one parameter only.
///
/// # Errors
///
/// Those of [`Curve::is_closed`] and of the two evaluations.
fn seam_twin(k: &Kernel, curve: &Curve, t: f64, tol: f64) -> Result<Option<f64>> {
    let ivl = curve.natural_interval();
    if !curve.is_closed(k, &ivl, tol)? {
        return Ok(None);
    }
    let (near, far) = if t - ivl.min() <= ivl.max() - t {
        (ivl.min(), ivl.max())
    } else {
        (ivl.max(), ivl.min())
    };
    let on_seam = distance(curve.evaluate_point(k, t)?, curve.evaluate_point(k, near)?) <= tol;
    Ok(on_seam.then_some(far))
}

/// Not upstream: the candidates left of `passages` (parameters of `curve`: the solutions
/// of the vertex image `image` solved onto it) once the image counts once. The curve
/// passes the image where a solution lies within `same_tol` of it. A curve that is an
/// arc continued ten of its lengths each way (`ExtendTrimEdges`) runs round its circle
/// more than once, five times for a quarter circle, so it passes the image on every
/// turn. Those passages lie equally near `base_point`, the base vertex, but for
/// rounding, which then decided the one the search took; and an end taken on another
/// turn than the edge's other end gives the edge the rest of the circle, or the circle
/// several times over. Where the solution nearest the base vertex (the search's choice)
/// is a passage of the image, only the passage nearest `middle` along the curve stays:
/// `middle` is where the edge's middle drops onto the curve ([`middle_passage`]), and an
/// open edge's end is nearer its own middle than the same end a turn on. Both ends are
/// then taken on the middle's turn. Solutions that are no passage of the image (the
/// nearest points of a curve that misses it) stay as they are, and so do all of them
/// where the nearest the base vertex is one of those.
///
/// Returns the indices kept, in order; none for no solution.
///
/// # Errors
///
/// Those of the evaluations.
fn passages_on_one_turn(
    k: &Kernel,
    curve: &Curve,
    passages: &[f64],
    image: Point3,
    base_point: Point3,
    middle: f64,
    same_tol: f64,
) -> Result<Vec<usize>> {
    // which solutions are passages of the image, the one nearest the base vertex, and of
    // the passages the one nearest the edge's middle along the curve
    let mut at_image = Vec::new();
    let mut nearest: Option<(f64, bool)> = None;
    let mut on_turn: Option<(f64, usize)> = None;
    for (index, &t) in passages.iter().enumerate() {
        let point = curve.evaluate_point(k, t)?;
        let passes = distance(point, image) <= same_tol;
        at_image.push(passes);
        let off = distance(point, base_point);
        if nearest.is_none_or(|(least, _)| off < least) {
            nearest = Some((off, passes));
        }
        let away = (t - middle).abs();
        if passes && on_turn.is_none_or(|(least, _)| away < least) {
            on_turn = Some((away, index));
        }
    }
    let taken_is_passage = nearest.is_some_and(|(_, passes)| passes);
    let mut kept = Vec::new();
    for (index, &passes) in at_image.iter().enumerate() {
        if !(taken_is_passage && passes) || on_turn.is_some_and(|(_, at)| at == index) {
            kept.push(index);
        }
    }
    Ok(kept)
}

/// Not upstream: of `drops` (parameters of `curve`: the solutions of the point `middle`
/// dropped onto it), the parameter that stands for the edge's middle in
/// [`passages_on_one_turn`]. The nearest solution is the drop; a curve that runs round
/// more than once passes the drop's point on every turn (the solutions within
/// `same_tol` of it), and the one taken is the passage farthest from both ends of the
/// curve, nearest the middle of its interval: a turn cut short by an end of the curve
/// would lack one of the edge's ends. `ExtendTrimEdges` continues an edge as far each
/// way, so that passage is on the turn the edge itself is on. `None` without a solution.
///
/// # Errors
///
/// Those of the evaluations.
fn middle_passage(
    k: &Kernel,
    curve: &Curve,
    drops: &[f64],
    middle: Point3,
    same_tol: f64,
) -> Result<Option<f64>> {
    let mut points = Vec::new();
    let mut nearest: Option<(f64, Point3)> = None;
    for &t in drops {
        let point = curve.evaluate_point(k, t)?;
        points.push(point);
        let off = distance(point, middle);
        if nearest.is_none_or(|(least, _)| off < least) {
            nearest = Some((off, point));
        }
    }
    let Some((_, drop)) = nearest else {
        return Ok(None);
    };
    let centre = curve.natural_interval().evaluate(0.5);
    let mut taken: Option<(f64, f64)> = None;
    for (&t, &point) in drops.iter().zip(&points) {
        let away = (t - centre).abs();
        if distance(point, drop) <= same_tol && taken.is_none_or(|(least, _)| away < least) {
            taken = Some((away, t));
        }
    }
    Ok(taken.map(|(_, t)| t))
}

/// Not upstream (WR-673, #2174): whether the point of `curve` at `t` is the first trim
/// point (at `t_first`) again, reached round a closed curve: the curve is closed over
/// its natural interval, and over the span from `t_first` to `t` its ends lie within
/// `tol` of each other while its middle does not (`SmCurve::IsClosed` on that span),
/// so the span runs round the curve back to its start. A short open edge whose second
/// point lies within `tol` of its first is no such span (its middle is near both ends,
/// or the curve is not closed): that point stays a candidate.
///
/// Such a span leaves out only a short arc across the curve's seam, between its ends.
/// Where the edge's own two ends lie within `tol` of each other (`ends_apart` false),
/// that short arc is where the edge runs, the mirror of a short span on one side of the
/// seam, and the point at `t` is a real second point, however near the first: only the
/// first point's own other parameter is passed over, the span then the whole natural
/// interval (the arc across the seam has no parameter length, [`are_same`], as a
/// candidate at `t_first` itself has none). Where the edge's ends are apart, its two
/// trim points cannot be one point, and every such span is the first point again.
///
/// # Errors
///
/// Those of [`Curve::is_closed`] and [`is_closed_base`].
pub fn is_first_point_again(
    k: &Kernel,
    curve: &Curve,
    t_first: f64,
    t: f64,
    tol: f64,
    ends_apart: bool,
) -> Result<bool> {
    let natural = curve.natural_interval();
    if !curve.is_closed(k, &natural, tol)? {
        return Ok(false);
    }
    let span = Extent1d::from_min_max(smos_min(t_first, t), smos_max(t_first, t));
    if !ends_apart && !are_same(span.length(), natural.length()) {
        return Ok(false);
    }
    is_closed_base(k, curve, &span, tol)
}

/// Not upstream (WR-673): whether an open edge's two vertices lie more than `tol` apart
/// (see [`is_first_point_again`]).
///
/// # Errors
///
/// Those of the topology queries.
fn open_edge_ends_apart<O: ClassObject>(
    brep: &impl OffsetBrep<O>,
    edge: O,
    tol: f64,
) -> Result<bool> {
    let v0 = brep
        .edge_vertex(edge)?
        .ok_or(dangling("edge without vertex"))?;
    let v1 = brep
        .edge_other_vertex(edge, v0)?
        .ok_or(dangling("edge without other vertex"))?;
    Ok(distance(brep.vertex_point(v0)?, brep.vertex_point(v1)?) > tol)
}

/// Not upstream (WR-530): whether a tangent edge's new curve, dropped onto the other
/// face's new surface, lands `max_drop_to_surf` from it, past `meet_tol`: the faces no
/// longer meet along the edge. A drop (or limit) that is not a number is no evidence that
/// they meet, so it is loose too: only a drop within the limit passes.
fn drop_is_loose(max_drop_to_surf: f64, meet_tol: f64) -> bool {
    // `!(max_drop_to_surf <= meet_tol)`, spelled without a negated partial comparison
    !matches!(
        max_drop_to_surf.partial_cmp(&meet_tol),
        Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
    )
}

/// The surface behind `h` (a dereference upstream).
fn surface_of<O: ClassObject>(geometry: &impl ClassGeometry<O>, h: O) -> Result<&Surface> {
    geometry
        .surface(h)
        .ok_or(dangling("surface handle cannot be resolved"))
}

/// `SmBSplineCurve::MakeNonRational` (fork use, `CreateStepWall`): every weighted control
/// point divided through by its weight and made non-rational.
fn make_non_rational(curve: &mut BSplineCurve) -> Result<()> {
    let nurb = curve.nurb.as_mut().ok_or(dangling("curve without NURB"))?;
    for cp in nurb.cpts_mut() {
        if cp.w != NOW {
            let w = cp.w;
            cp.x /= w;
            cp.y /= w;
            if cp.z != NOZ {
                cp.z /= w;
            }
            cp.w = NOW;
        }
    }
    Ok(())
}

/// Stores `curve` and returns its handle.
fn store_curve<O: ClassObject>(brep: &mut impl OffsetBrep<O>, curve: Curve) -> Result<O> {
    brep.insert_curve(curve)
        .ok_or(dangling("curve store is full"))
}

/// Stores `surface` and returns its handle.
fn store_surface<O: ClassObject>(brep: &mut impl OffsetBrep<O>, surface: Surface) -> Result<O> {
    brep.insert_surface(surface)
        .ok_or(dangling("surface store is full"))
}

/// `SmPoint3d::DistanceBetween`.
fn distance(a: Point3, b: Point3) -> f64 {
    a.distance(b)
}

/// `smos_Max`: `a > b ? a : b`.
fn smos_max(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `smos_Min`: `a < b ? a : b`.
fn smos_min(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// The fork's `sm_CreateAnalyticExtension` (tangency-labs/smlib@fddc366 with
/// @98ab1b0, behind [`crate::kernel::UpstreamFixes::offset_analytic_mate_edges`]): a
/// plane, cylinder, cone, sphere or torus copied and extended as itself where the
/// offset's extended surfaces (`GetCreateExtendedSurface`, tangent (G1R) B-spline
/// extensions) still are that surface: a plane all round, a cylinder or cone along its
/// rulings (a cone up to its apex), each side by `dist` (3D) unless it is a
/// singularity. Not round a turn, where the tangent extension leaves the surface.
/// `None` for any other surface.
fn create_analytic_extension(k: &Kernel, surface: &Surface, dist: f64) -> Result<Option<Surface>> {
    let ty = surface.type_code();
    let plane = ty == PLANE_TYPE;
    if !plane && ty != CYLINDER_TYPE && ty != CONE_TYPE && ty != SPHERE_TYPE && ty != TORUS_TYPE {
        return Ok(None);
    }

    let domain = surface.step_uv_domain();
    let max = if plane {
        domain
    } else {
        surface.max_analytic_domain(k)?
    };
    let (mut new_min, mut new_max) = (domain.min(), domain.max());
    for dir in 0..2 {
        // 0 = u, 1 = v
        if !plane && (dir == 0 || ty == SPHERE_TYPE || ty == TORUS_TYPE) {
            continue; // a turn: the faces' extensions are not the surface there
        }
        let lo = if dir == 0 {
            domain.min().x
        } else {
            domain.min().y
        };
        let hi = if dir == 0 {
            domain.max().x
        } else {
            domain.max().y
        };
        let mid = if dir == 0 {
            0.5 * (domain.min().y + domain.max().y)
        } else {
            0.5 * (domain.min().x + domain.max().x)
        };
        let mut grow = [0.0_f64, 0.0];
        for (side, g) in grow.iter_mut().enumerate() {
            let at = if side == 0 { lo } else { hi };
            let uv = if dir == 0 {
                Point2::new(at, mid)
            } else {
                Point2::new(mid, at)
            };
            let from = if side == 0 { Side::Left } else { Side::Right };
            let ders = surface.evaluate_step(
                k,
                uv,
                1,
                1,
                EvaluateFlags {
                    u_side: from,
                    v_side: from,
                    only_upper_half: false,
                    non_zero_tangents: true,
                },
            )?;
            // upstream's [D, Dv, Du, Duv]: Du is ders[1][0], Dv is ders[0][1]
            let der = if dir == 0 {
                ders.get(1).and_then(|r| r.first())
            } else {
                ders.first().and_then(|r| r.get(1))
            };
            let speed = der.ok_or(dangling("surface derivative"))?.length();
            if speed > EFF_ZERO {
                // no extension through a singularity
                *g = dist / speed;
            }
        }
        let [grow_lo, grow_hi] = grow;
        let mut new_lo = lo - grow_lo;
        let mut new_hi = hi + grow_hi;
        if !plane {
            // a cylinder's or cone's rulings, as far as the cone reaches (its apex)
            let (max_lo, max_hi) = (max.min().y, max.max().y);
            new_lo = smos_max(new_lo, smos_min(lo, max_lo));
            new_hi = smos_min(new_hi, smos_max(hi, max_hi));
        }
        if dir == 0 {
            new_min.x = new_lo;
            new_max.x = new_hi;
        } else {
            new_min.y = new_lo;
            new_max.y = new_hi;
        }
    }

    let mut copy = surface.copy(k)?;
    copy.adjust_step_uv_domain(k, &Extent2d::from_points(new_min, new_max))?;
    Ok(Some(copy))
}

/// One end of a piece of a surface/surface intersection ([`join_through_tangent_points`]):
/// the piece, which end (`true` its last), the point, the outward unit tangent and the UV
/// points on both surfaces.
#[derive(Clone, Copy)]
struct PieceEnd {
    piece: usize,
    last: bool,
    point: Point3,
    outward: Vec3,
    uv: [Point2; 2],
}

/// Whether the B-spline `c` is clamped at both ends (each end knot of multiplicity
/// `p + 1`): its end points are its end control points.
fn clamped(c: &BSplineCurve) -> Result<bool> {
    let order = c.degree()? + 1;
    let (_, multiplicities) = c.knots(None)?;
    Ok(multiplicities.first() == Some(&order) && multiplicities.last() == Some(&order))
}

/// The ends of piece `piece` of `ssi` whose three curves are B-splines clamped at both
/// ends ([`clamped`]; `None` for any other piece, which is left as it is).
fn piece_ends(
    k: &Kernel,
    ssi: &SurfaceIntersection,
    piece: usize,
) -> Result<Option<[PieceEnd; 2]>> {
    let (Some(c3), Some(Some(uv1)), Some(Some(uv2))) = (
        ssi.curves_3d.get(piece),
        ssi.surface1_uv_curves.get(piece),
        ssi.surface2_uv_curves.get(piece),
    ) else {
        return Ok(None);
    };
    for c in [c3, uv1, uv2] {
        match c.kind() {
            CurveKind::BSpline(b) if clamped(b)? => {}
            _ => return Ok(None),
        }
    }
    if c3.is_degenerate(k, EFF_ZERO, None)? {
        return Ok(None);
    }
    let ivl = c3.natural_interval();
    let mut ends = Vec::new();
    for last in [false, true] {
        let t = if last { ivl.max() } else { ivl.min() };
        let d = c3.evaluate(k, t, 1, last, true)?;
        let (Some(&point), Some(&tangent)) = (d.first(), d.get(1)) else {
            return Ok(None);
        };
        let Ok(unit) = tangent.unitize() else {
            return Ok(None);
        };
        let uv = |c: &Curve| -> Result<Point2> {
            let p = c.evaluate_point(k, t)?;
            Ok(Point2::new(p.x, p.y))
        };
        ends.push(PieceEnd {
            piece,
            last,
            point,
            outward: if last { unit } else { -unit },
            uv: [uv(uv1)?, uv(uv2)?],
        });
    }
    Ok(ends.try_into().ok())
}

/// Whether the ends `a` and `b` of two intersection pieces are one point of both surfaces
/// where the surfaces are tangent and the pieces continue each other: the points within
/// the intersection's tolerance `tol`, each surface's UV points within it at first order
/// (`|Δu| |S_u| + |Δv| |S_v|`, so a piece across a seam is not taken), the normals and the
/// pieces' outward tangents opposite, each within `sine` (the tangency tolerance's).
/// These decide the join only; the joined curve's deviation is proved where it is made
/// ([`join_through_tangent_points`]).
fn continues_through_tangent_point(
    k: &Kernel,
    surfaces: [&Surface; 2],
    a: &PieceEnd,
    b: &PieceEnd,
    tol: f64,
    sine: f64,
) -> Result<bool> {
    let gap = a.point.distance(b.point);
    if gap > tol || gap.is_nan() || a.outward.cross(-b.outward).length() > sine {
        return Ok(false);
    }
    if a.outward.dot(b.outward) >= 0.0 {
        return Ok(false);
    }
    let mut normals = [Vec3::ZERO; 2];
    for ((surface, normal), (ua, ub)) in surfaces
        .iter()
        .zip(normals.iter_mut())
        .zip(a.uv.iter().zip(b.uv.iter()))
    {
        let d = surface.evaluate(k, *ua, 1, 1, EvaluateFlags::default())?;
        let (Some(su), Some(sv)) = (
            d.get(1).and_then(|r| r.first()),
            d.first().and_then(|r| r.get(1)),
        ) else {
            return Ok(false);
        };
        let moved = (ua.x - ub.x).abs() * su.length() + (ua.y - ub.y).abs() * sv.length();
        if moved > tol || moved.is_nan() {
            return Ok(false);
        }
        *normal = surface.evaluate_normal(k, *ua, Side::Right, Side::Right)?;
    }
    let [n1, n2] = normals;
    Ok(n1.cross(n2).length() <= sine)
}

/// Not upstream (#17210): the pieces of the intersection of two surfaces that meet end to
/// end at a point where the surfaces are tangent, and continue each other there, joined
/// into one curve, with their UV curves. Marching stops at a tangent point of the
/// surfaces, where two branches of their intersection cross (two oblique cylinders
/// offset from one circle meet on that circle's offset and on a curve through it), so
/// one curve comes out as two pieces; an edge whose end lies past the tangent point
/// (its offset vertex on the far piece) found no trim on either. An end is joined only
/// to the one other end that continues it ([`continues_through_tangent_point`]); an
/// end with none, or more than one, is left. The three curves of the two pieces are
/// joined alike ([`BSplineCurve::create_by_joining`]: the first keeps its parameters,
/// the second follows on), so the joined 3D and UV curves keep one parameter. Before the
/// join, each of the second piece's three curves has its end control point at the joint
/// set to the first piece's there (both clamped, [`clamped`]: those are their end points),
/// so the two ends are one point, in 3D and in both UV domains, and the join drops nothing.
/// Moving one control point by `δ` (its weight kept) moves a curve by at most `δ`, its
/// basis being a partition of unity, so the joined 3D curve's deviation is the first
/// piece's, or the second's raised by that distance (enclosed in rounded interval
/// arithmetic), whichever is larger: a proved bound, with nothing estimated. Each joined
/// curve then loses the copies of its joint knot whose removal is exact to rounding in
/// all three curves ([`remove_exact_joint_knot_together`]); the 3D curve's deviation grows by that removal's
/// certified bound. The deviation is the 3D curve's only: the UV curves' move (their
/// joint knot's removal, like their end points set above) is not recorded.
///
/// # Errors
///
/// Those of the evaluations and of the joins.
fn join_through_tangent_points(
    k: &Kernel,
    ssi: &mut SurfaceIntersection,
    surfaces: [&Surface; 2],
    tol: f64,
    tangency_tol_radians: f64,
) -> Result<()> {
    let sine = tangency_tol_radians.abs().min(PI / 2.0).sin();
    // each join removes a piece: at most one fewer than the pieces
    for _ in 1..ssi.curves_3d.len() {
        let mut ends = Vec::new();
        for piece in 0..ssi.curves_3d.len() {
            if let Some(both) = piece_ends(k, ssi, piece)? {
                ends.extend(both);
            }
        }
        let mut join = None;
        'ends: for a in &ends {
            let mut found = None;
            for b in ends.iter().filter(|b| b.piece != a.piece) {
                if continues_through_tangent_point(k, surfaces, a, b, tol, sine)? {
                    if found.is_some() {
                        continue 'ends;
                    }
                    found = Some(*b);
                }
            }
            let Some(b) = found else {
                continue;
            };
            // and `a` is the only end that continues `b`
            for c in ends
                .iter()
                .filter(|c| c.piece != b.piece && (c.piece, c.last) != (a.piece, a.last))
            {
                if continues_through_tangent_point(k, surfaces, &b, c, tol, sine)? {
                    continue 'ends;
                }
            }
            join = Some((*a, b));
            break;
        }
        let Some((a, b)) = join else {
            return Ok(());
        };
        // the piece whose last end joins goes first; the other runs on from its joined end
        let (first, second) = if a.last { (a, b) } else { (b, a) };
        let senses = [true, !second.last];
        let mut joined: Vec<(BSplineCurve, f64)> = Vec::new();
        // how far the second piece's 3D curve moves, proved
        let mut moved = 0.0_f64;
        for (kind, curves) in [
            &ssi.curves_3d.iter().map(Some).collect::<Vec<_>>(),
            &ssi.surface1_uv_curves.iter().map(Option::as_ref).collect(),
            &ssi.surface2_uv_curves.iter().map(Option::as_ref).collect(),
        ]
        .into_iter()
        .enumerate()
        {
            let b_spline = |i: usize| match curves.get(i).copied().flatten().map(Curve::kind) {
                Some(CurveKind::BSpline(c)) => Ok(c.clone()),
                _ => Err(SolidError::port_invariant(
                    OP_MATE,
                    "a piece of the intersection to join is not a B-spline",
                )),
            };
            let mut segments = [b_spline(first.piece)?, b_spline(second.piece)?];
            let [head, tail] = &mut segments;
            let end_of = |c: &BSplineCurve, last: bool| -> Result<usize> {
                Ok(if last {
                    c.number_control_points()?.saturating_sub(1)
                } else {
                    0
                })
            };
            let at = (end_of(&*head, first.last)?, end_of(&*tail, second.last)?);
            let (joint, _) = head.control_point(ControlPointForm::EuclideanRational, at.0)?;
            let (old, weight) = tail.control_point(ControlPointForm::EuclideanRational, at.1)?;
            if tail.is_rational()? {
                tail.set_control_point(ControlPointForm::EuclideanRational, at.1, joint, weight)?;
            } else {
                tail.set_control_point(ControlPointForm::NonRational, at.1, joint, 1.0)?;
            }
            if kind == 0 {
                let d = |x: f64, y: f64| Ivl::from(x) - Ivl::from(y);
                let (dx, dy, dz) = (d(joint.x, old.x), d(joint.y, old.y), d(joint.z, old.z));
                moved = (dx * dx + dy * dy + dz * dz).sqrt().hi;
            }
            // with no last end at the joint, both first ends: the first runs reversed
            let senses = if first.last { senses } else { [false, true] };
            let (curve, breaks) =
                BSplineCurve::create_by_joining_with_breaks(&mut segments, Some(&senses))?;
            let at_joint = breaks.get(1).copied().ok_or_else(|| {
                SolidError::port_invariant(OP_MATE, "a join of two pieces has no joint")
            })?;
            joined.push((curve, at_joint));
        }
        let mut joined: [(BSplineCurve, f64); 3] = joined
            .try_into()
            .map_err(|_| SolidError::port_invariant(OP_MATE, "three joined curves"))?;
        // how far removing the joint knot moves the joined 3D curve, bounded
        let smoothed = remove_exact_joint_knot_together(&mut joined)?;
        let [c3, uv1, uv2] = joined.map(|(c, _)| Curve::from(c));
        let (keep, drop) = (first.piece.min(second.piece), first.piece.max(second.piece));
        if let Some(c) = ssi.curves_3d.get_mut(keep) {
            *c = c3;
        }
        if let Some(c) = ssi.surface1_uv_curves.get_mut(keep) {
            *c = Some(uv1);
        }
        if let Some(c) = ssi.surface2_uv_curves.get_mut(keep) {
            *c = Some(uv2);
        }
        let deviation = |i: usize| ssi.deviations.get(i).copied().unwrap_or(0.0);
        let worst = max2(deviation(first.piece), deviation(second.piece) + moved);
        // the joint knot's removal moves the whole joined curve by at most its bound
        let worst = if smoothed > 0.0 {
            (Ivl::from(worst) + Ivl::from(smoothed)).hi
        } else {
            worst
        };
        if let Some(d) = ssi.deviations.get_mut(keep) {
            *d = worst;
        }
        drop_index(&mut ssi.surface1_uv_curves, drop);
        drop_index(&mut ssi.surface2_uv_curves, drop);
        drop_index(&mut ssi.curves_3d, drop);
        drop_index(&mut ssi.curve_types, drop);
        drop_index(&mut ssi.deviations, drop);
    }
    Ok(())
}

/// Not upstream: the joined curve `curve` without the copies of its joint knot at `at`
/// (at most `most` of them) whose removal moves the curve by no more than its own scaled zero,
/// `SM_EFF_ZERO (1 + extent)`, where the extent is the largest spread of the curve's
/// control points along a coordinate axis ([`joint_knot_tolerance`]).
///
/// Where the two pieces were cut from one curve (`intersect::surface_intersector::
/// singular_splits` splits a crossing curve at the tangent node it passes through, WR-77),
/// joining them gives that curve back with the joint knot raised to full multiplicity.
/// The copies the join added are removable exactly; rounding is all the removal sees. Where
/// the cut fell on a knot the curve already had, the copies it had stay: removing one of
/// them would move the curve, so the removal stops there and keeps the ones before
/// ([`BSplineCurve::remove_knot_certified`] keeps a partial removal). Left in, the joint
/// knot can sit a rounding-sized distance inside where an edge's trim ends at the node's
/// offset vertex. The trim then drops that tiny span without reparametrising
/// (`remove_degen_segs`), which leaves a break in parametric speed at the next knot, and
/// the UV trims dropped from that curve fit less closely there (on the thickened
/// `HoodCorner` sheet of `tests/issue_1502_thicken_rim_halves.rs`, a drop span fits to
/// 1.7e-5 with the knot and to 7e-8 without it).
///
/// The test is the curve's shape at the joint, not where the pieces came from or where
/// they lie: the tolerance scales with the curve's size, not its distance from the origin,
/// so a translated joint is decided the same way (or, far out, where the coordinates'
/// rounding passes that tolerance, keeps its knot). Pieces traced apart that meet at a
/// true break (a corner, a turn of the tangent) keep their knot. Pieces traced apart whose
/// join happens to be smooth to rounding lose it as cut pieces do: two collinear line
/// pieces through a saddle's node (#17210) join into one line. Either way the bound below
/// covers the move.
///
/// A 3D curve and its UV curves are not decided here one by one: they keep one knot
/// vector (`docs/ssi.md` section 8, invariant 5), so [`remove_exact_joint_knot_together`]
/// removes from each only the copies all three lose.
///
/// Returns the copies removed and the bound on how far the curve moved: the copies removed times the
/// tolerance, rounded up. [`BSplineCurve::remove_knot_certified`] keeps a removal only
/// when a certified bound (the knot reinserted in interval arithmetic, control points
/// compared) is within that; `0` when no copy is removed. Rational curves are left
/// alone (no copy removed): a control-point bound in homogeneous space does not bound
/// the Euclidean move without a bound on the weights, and the traced intersection
/// curves joined here are non-rational fits.
///
/// # Errors
///
/// Those of the knot and control-point queries and of the removal.
fn remove_joint_copies(curve: &mut BSplineCurve, at: f64, most: usize) -> Result<(usize, f64)> {
    let (knots, multiplicities) = curve.knots(None)?;
    let Some(times) = knots
        .iter()
        .zip(&multiplicities)
        .find_map(|(&u, &m)| (u == at).then_some(m.min(most)))
    else {
        return Ok((0, 0.0));
    };
    let tol = joint_knot_tolerance(curve)?;
    if times == 0 || !tol.is_finite() {
        return Ok((0, 0.0));
    }
    let removed = curve.remove_knot_certified(at, times, tol)?;
    if removed == 0 {
        return Ok((0, 0.0));
    }
    Ok((removed, (Ivl::from(removed as f64) * Ivl::from(tol)).hi))
}

/// Not upstream: the joined 3D curve and its two UV curves, each with its joint knot,
/// without the copies of that knot that all three lose exactly
/// ([`remove_joint_copies`]), so they keep one knot vector (`docs/ssi.md` section 8,
/// invariant 5). Each curve is tried on its own (on a copy); the fewest copies any of
/// them loses, `n`, are then removed from each (again on copies), and the three results
/// are kept only when every one lost exactly `n`; otherwise, or when the three joints
/// are not at one parameter, all three keep their knot as joined. A joint where one
/// curve turns keeps its knot in all three: at the `HoodCorner` node of
/// `tests/issue_1502_thicken_rim_halves.rs` the 3D curve is smooth but the first UV
/// curve turns (one copy would come out only within `1e-8`, a second not within `1e-4`).
///
/// Returns the bound on how far the 3D curve (the first) moved; `0` when nothing is
/// removed.
///
/// # Errors
///
/// Those of [`remove_joint_copies`].
fn remove_exact_joint_knot_together(joined: &mut [(BSplineCurve, f64); 3]) -> Result<f64> {
    let [(_, at), (_, at_uv1), (_, at_uv2)] = &*joined;
    let at = *at;
    if at != *at_uv1 || at != *at_uv2 {
        return Ok(0.0);
    }
    let mut fewest = usize::MAX;
    for (curve, _) in joined.iter() {
        let mut trial = curve.clone_exact();
        fewest = fewest.min(remove_joint_copies(&mut trial, at, usize::MAX)?.0);
    }
    if fewest == 0 {
        return Ok(0.0);
    }
    let mut removed = Vec::new();
    for (curve, _) in joined.iter() {
        let mut candidate = curve.clone_exact();
        let (count, bound) = remove_joint_copies(&mut candidate, at, fewest)?;
        if count != fewest {
            return Ok(0.0);
        }
        removed.push((candidate, bound));
    }
    let mut bound_3d = 0.0;
    for (i, ((curve, _), (candidate, bound))) in joined.iter_mut().zip(removed).enumerate() {
        *curve = candidate;
        if i == 0 {
            bound_3d = bound;
        }
    }
    Ok(bound_3d)
}

/// Not upstream: the tolerance of [`remove_joint_copies`] for `curve`,
/// `SM_EFF_ZERO (1 + extent)` rounded up, where the extent is the largest spread
/// `max - min` of the curve's control points along one coordinate axis (a 2D curve's
/// `z` is `0`): the curve's size, which a translation leaves alone.
///
/// # Errors
///
/// Those of the control-point queries.
fn joint_knot_tolerance(curve: &BSplineCurve) -> Result<f64> {
    let mut low = [f64::INFINITY; 3];
    let mut high = [f64::NEG_INFINITY; 3];
    for i in 0..curve.number_control_points()? {
        let (p, _) = curve.control_point(ControlPointForm::EuclideanRational, i)?;
        for ((lo, hi), x) in low.iter_mut().zip(high.iter_mut()).zip([p.x, p.y, p.z]) {
            *lo = lo.min(x);
            *hi = hi.max(x);
        }
    }
    let extent = low.iter().zip(&high).fold(0.0_f64, |m, (&lo, &hi)| {
        m.max((Ivl::from(hi) - Ivl::from(lo)).hi)
    });
    Ok((Ivl::from(EFF_ZERO) * (Ivl::from(1.0) + Ivl::from(extent))).hi)
}

/// Removes the element at `index` when it exists; an out-of-range index leaves the list as is.
fn drop_index<T>(list: &mut Vec<T>, index: usize) {
    let mut at = 0usize;
    list.retain(|_| {
        let keep = at != index;
        at += 1;
        keep
    });
}

/// Not upstream (tangency-labs/solidcraft#16727): whether `old` on `old_ivl` lies on `new` on
/// `new_ivl`: its points a quarter, a half and three quarters of the way along its interval each
/// drop onto `new` within `tol` (a global drop, so the two curves' parameterisations need not
/// agree). Ends that agree prove nothing about the curve between them: a face turned about a hinge
/// through both ends of one of its edges keeps those ends while the edge between them moves. With
/// the two ends, these are five points: for a line or a conic (the exact class of a plane
/// face's edges), five points on another of the same plane determine it, so the old curve lies on
/// the new one exactly when they agree; for a freeform edge they are a check at the edge's
/// tolerance, not a proof.
fn curve_lies_on(
    k: &Kernel,
    old: &Curve,
    old_ivl: &Extent1d,
    new: &Curve,
    new_ivl: &Extent1d,
    tol: f64,
) -> Result<bool> {
    for f in [0.25, 0.5, 0.75] {
        let p = old.evaluate_point(k, old_ivl.evaluate(f))?;
        let d = new.drop_point(k, new_ivl, p, None, tol, None, SolverOperation::Minimize)?;
        if !(d.success && d.distance <= tol) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// The fork's `sm_ExactPiece` (tangency-labs/smlib@fddc366, behind
/// [`crate::kernel::UpstreamFixes::offset_analytic_mate_edges`]): the exact line or
/// conic of `exact` that `marched` (a marched intersection piece) approximates, as a
/// copy over the same span: from `marched`'s start point to its end point, the way
/// round through its middle (all of a closed conic for a closed piece). `None` when no
/// line or conic of `exact` runs through all three points within `tol`.
fn exact_piece(
    k: &Kernel,
    marched: &Curve,
    exact: &[Curve],
    tol: f64,
) -> Result<Option<ExactPiece>> {
    let ivl = marched.natural_interval();
    // start, middle, end
    let p = [
        marched.evaluate_point(k, ivl.min())?,
        marched.evaluate_point(k, ivl.evaluate(0.5))?,
        marched.evaluate_point(k, ivl.max())?,
    ];
    for candidate in exact {
        // (a line or a conic: `IsKindOf(SmLine_TYPE)` or `IsKindOf(SmEllipse_TYPE)`, which
        // only those kinds answer)
        let (line, of_kind) = match candidate.kind() {
            CurveKind::Line(l) => (true, ExactKind::Line(l)),
            CurveKind::Ellipse(e) => (false, ExactKind::Conic(e)),
            _ => continue,
        };
        let mut t = [0.0_f64; 3];
        let mut on = true;
        for (tk, pk) in t.iter_mut().zip(p.iter()) {
            if !on {
                break;
            }
            *tk = candidate.step_inversion(k, *pk)?;
            let on_curve = candidate.evaluate_step_point(k, *tk)?;
            on = distance(on_curve, *pk) <= tol;
        }
        if !on {
            continue;
        }
        let [t0, t1, t2] = t;
        let [p0, _, p2] = p;
        let span = if line {
            Extent1d::from_min_max(smos_min(t0, t2), smos_max(t0, t2))
        } else {
            // a conic: its STEP parameter is its angle in degrees
            let turn = 360.0;
            // `angle` moved into (from, from + turn]; the turns are counted so that a
            // parameter far outside a turn cannot loop without bound (the angles are
            // a conic's STEP parameters, within a few turns)
            let after = |mut angle: f64, from: f64| -> Result<f64> {
                let mut turns = 0_u32;
                while angle <= from {
                    angle += turn;
                    turns += 1;
                    if turns > MAX_EXACT_PIECE_TURNS {
                        return Err(SolidError::NotConverged {
                            operation: OP_EXACT_PIECE,
                            iterations: turns,
                        });
                    }
                }
                while angle > from + turn {
                    angle -= turn;
                    turns += 1;
                    if turns > MAX_EXACT_PIECE_TURNS {
                        return Err(SolidError::NotConverged {
                            operation: OP_EXACT_PIECE,
                            iterations: turns,
                        });
                    }
                }
                Ok(angle)
            };
            if distance(p0, p2) <= tol {
                Extent1d::from_min_max(t0, t0 + turn)
            } else if after(t1, t0)? < after(t2, t0)? {
                Extent1d::from_min_max(t0, after(t2, t0)?)
            } else {
                Extent1d::from_min_max(t2, after(t0, t2)?)
            }
        };
        // (the candidate's copy, as `Curve::copy` makes it, over the span)
        return Ok(Some(match of_kind {
            ExactKind::Line(l) => {
                let mut copy = l.copy();
                copy.adjust_step_interval(&span)?;
                ExactPiece::Line(copy)
            }
            ExactKind::Conic(e) => {
                let mut copy = e.copy(k)?;
                copy.adjust_step_interval(k, &span, None)?;
                ExactPiece::Conic(copy)
            }
        }));
    }
    Ok(None)
}

/// The kind of a candidate of [`exact_piece`].
enum ExactKind<'a> {
    Line(&'a Line),
    Conic(&'a Ellipse),
}

/// An exact piece ([`exact_piece`]): a line or a conic.
#[derive(Debug, Clone)]
enum ExactPiece {
    Line(Line),
    Conic(Ellipse),
}

impl ExactPiece {
    /// `ReverseParameterization(ivl)` of the line or conic, as
    /// [`Curve::reverse_parameterization`] does it for a curve of its own (no owner).
    fn reverse_parameterization(&mut self, k: &Kernel, ivl: &Extent1d) -> Result<()> {
        match self {
            Self::Line(l) => {
                let _ = l.reverse_parameterization(ivl);
            }
            Self::Conic(e) => {
                e.reverse_parameterization(k, ivl, None)?;
            }
        }
        Ok(())
    }

    /// `AssertValid(NULL, SM_LEVEL_2, SM_NO_WALK)` of the line or conic.
    fn assert_valid(&mut self, k: &Kernel) -> Result<bool> {
        match self {
            Self::Line(l) => l.assert_valid(k, None, false, ObjectKey(0)),
            Self::Conic(c) => c.assert_valid(k, None, false, ObjectKey(0), ObjectKey(0)),
        }
    }
}

impl From<ExactPiece> for Curve {
    fn from(piece: ExactPiece) -> Self {
        match piece {
            ExactPiece::Line(l) => Self::from(l),
            ExactPiece::Conic(e) => Self::from(e),
        }
    }
}

/// A cone or cylinder copy grown along its axis by `margin` at each end of its STEP v
/// domain (its axial height), no farther than the cone reaches (its apex): for
/// [`make_edges_on_analytics_exact`]'s intersection of the faces' surfaces (#16587).
/// Other surfaces are left as they are. The growth and the snap of
/// [`crate::surface::cone::Cone`]'s plane intersection onto its domain's ends are both in
/// STEP v, which for a cone or cylinder is axial length (a point of the axis is
/// `origin + v·axis`), so `margin` compares directly with the snap distance.
fn grow_along_axis(k: &Kernel, s: &mut Surface, margin: f64) -> Result<()> {
    let ty = s.type_code();
    if ty != CYLINDER_TYPE && ty != CONE_TYPE {
        return Ok(());
    }
    let domain = s.step_uv_domain();
    let max = s.max_analytic_domain(k)?;
    let (lo, hi) = (domain.min().y, domain.max().y);
    let new_lo = smos_max(lo - margin, smos_min(lo, max.min().y));
    let new_hi = smos_min(hi + margin, smos_max(hi, max.max().y));
    if new_lo == lo && new_hi == hi {
        return Ok(());
    }
    s.adjust_step_uv_domain(
        k,
        &Extent2d::from_bounds(domain.min().x, new_lo, domain.max().x, new_hi),
    )
}

/// An edge [`make_edges_on_analytics_exact`] would give an exact curve.
struct ExactEdge {
    edge: EdgeId,
    piece: Curve,
    /// The piece's points at its start and end (its natural interval's ends).
    ends: [Point3; 2],
    faces: [FaceId; 2],
    /// Its start and end vertices.
    vertices: [Option<VertexId>; 2],
}

/// The ends at `v` of edge `e` as it will be: the exact piece's ends for a kept
/// [`ExactEdge`], else the ends of its curve over its interval (none without a curve).
fn edge_ends_at(
    k: &Kernel,
    b: &Brep,
    e: EdgeId,
    v: VertexId,
    exact: &[ExactEdge],
    kept: &[bool],
) -> Result<Vec<Point3>> {
    let mut out = Vec::new();
    if let Some(x) = exact
        .iter()
        .zip(kept)
        .find_map(|(x, &keep)| (keep && x.edge == e).then_some(x))
    {
        for (end, vertex) in x.ends.iter().zip(x.vertices) {
            if vertex == Some(v) {
                out.push(*end);
            }
        }
        return Ok(out);
    }
    let Some(curve) = topo_edge::curve(b, e)? else {
        return Ok(out);
    };
    let ivl = topo_edge::interval(b, e)?;
    let (start, end) = topo_edge::start_end_vertices(b, e)?;
    if start == Some(v) {
        out.push(curve.evaluate_point(k, ivl.min())?);
    }
    if end == Some(v) {
        out.push(curve.evaluate_point(k, ivl.max())?);
    }
    Ok(out)
}

/// Whether vertex `v` may move to `target`: by no more than the smallest of its own
/// tolerance and those of the edges it bounds, and every end at `v` of each of those
/// edges, as it will be ([`edge_ends_at`]), within that distance of `target`. A NaN
/// tolerance or distance does not fit.
fn vertex_move_fits(
    k: &Kernel,
    b: &Brep,
    v: VertexId,
    target: Point3,
    exact: &[ExactEdge],
    kept: &[bool],
) -> Result<bool> {
    let edges = crate::topology::vertex::edges(b, v, None)?;
    let mut tolerances = vec![crate::topology::vertex::tolerance(b, v)?];
    for &e in &edges {
        tolerances.push(topo_edge::tolerance(b, e)?);
    }
    let Some(allowed) = smallest_tolerance(&tolerances) else {
        return Ok(false);
    };
    if !move_within(distance(b.vertex(v)?.point, target), allowed) {
        return Ok(false);
    }
    for &e in &edges {
        for end in edge_ends_at(k, b, e, v, exact, kept)? {
            if !move_within(distance(end, target), allowed) {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// The smallest of `tolerances`, or `None` when any is NaN (`f64::min` would skip it) or
/// there are none.
fn smallest_tolerance(tolerances: &[f64]) -> Option<f64> {
    let mut smallest: Option<f64> = None;
    for &t in tolerances {
        if t.is_nan() {
            return None;
        }
        smallest = Some(smallest.map_or(t, |s| s.min(t)));
    }
    smallest
}

/// Whether a move of `distance` stays within `allowed`: written so that a NaN on either
/// side refuses.
fn move_within(distance: f64, allowed: f64) -> bool {
    distance <= allowed
}

/// The fork's `SmOffsetGeometryCreation::MakeEdgesOnAnalyticsExact`
/// (tangency-labs/smlib@cd85d34; WR-545), run at the end of every shell: each edge
/// of `b` whose curve is not analytic (an intersection marched and approximated)
/// between two faces on analytic surfaces takes the exact line or conic the surfaces
/// meet on through its ends and middle (`exact_piece`), running as the edge did,
/// unless that curve fails its own validity check. The faces of the changed edges get
/// their UV trim curves made again. Notifications go through `stores`.
///
/// Not upstream (#16587):
/// * the faces' cone and cylinder copies are intersected grown along their axes by the
///   edges' tolerance plus twice the intersection's approximation tolerance
///   ([`grow_along_axis`]). A face's surface was shrunk to its UV trims, which end at the
///   marched intersection, within the edge tolerance of the exact one; so a plane square
///   to the axis cuts the grown copy more than the approximation tolerance from its
///   ends, where the cone's plane intersection gives the circle at the exact height
///   rather than the one at its domain's end;
/// * an edge's start and end vertices move to its exact curve's start and end, each
///   vertex once (to the first changed edge's end, in edge order), and only if the move
///   is within the smallest of its tolerance and those of its edges and every edge it
///   bounds ends within that of the new point ([`vertex_move_fits`]); otherwise every
///   changed edge at that vertex keeps its marched curve, and the check is made again
///   over the remaining ones;
/// * an edge whose exact curve cannot be found or checked keeps its marched curve rather
///   than fail the shell: a grown copy refused, the intersection failing, the edge's
///   trimmed curve not built, no line or conic through its three points, or the piece
///   failing its validity check ([`exact_edge`]).
///
/// # Errors
///
/// Those of [`exact_edge`]; those of the vertex checks and moves, of the curve
/// replacement and of `CreateUVTrimCurves`.
pub fn make_edges_on_analytics_exact(
    k: &mut Kernel,
    b: &mut Brep,
    stores: &mut NotificationStores<'_>,
) -> Result<()> {
    let tol = b.tolerance();
    let ssi_tol = tol / 4.0;
    let deg20_rad = 20.0 * PI / 180.0;
    let margin = tol + 2.0 * approx_tol3d(k, ssi_tol);
    let mut exact_edges: Vec<ExactEdge> = Vec::new();
    for e in bp1::edges(b, None)? {
        if let Some(x) = exact_edge(k, b, e, tol, ssi_tol, deg20_rad, margin)? {
            exact_edges.push(x);
        }
    }

    // Not upstream (#16587): the vertices of the changed edges move onto their exact
    // ends where that fits every edge they bound; a changed edge with a vertex that cannot
    // move keeps its marched curve. Each pass drops at least one edge or ends.
    let mut kept = vec![true; exact_edges.len()];
    let moves = loop {
        let mut moves: Vec<(VertexId, Point3)> = Vec::new();
        for (x, _) in exact_edges.iter().zip(&kept).filter(|(_, &keep)| keep) {
            for (end, vertex) in x.ends.iter().zip(x.vertices) {
                if let Some(v) = vertex {
                    if !moves.iter().any(|(m, _)| *m == v) {
                        moves.push((v, *end));
                    }
                }
            }
        }
        let mut dropped = false;
        for &(v, target) in &moves {
            if !vertex_move_fits(k, b, v, target, &exact_edges, &kept)? {
                for (x, keep) in exact_edges.iter().zip(kept.iter_mut()) {
                    if *keep && x.vertices.contains(&Some(v)) {
                        *keep = false;
                        dropped = true;
                    }
                }
            }
        }
        if !dropped {
            break moves;
        }
    };

    let mut changed: Vec<FaceId> = Vec::new();
    for (x, keep) in exact_edges.into_iter().zip(kept) {
        if !keep {
            continue;
        }
        brep_p2::replace_curve(k, b, x.edge, x.piece, stores)?;
        for f in x.faces {
            if !changed.contains(&f) {
                changed.push(f);
            }
        }
    }
    for (v, target) in moves {
        let key_of = stores.key_of;
        crate::topology::vertex::set_point(
            b,
            v,
            target,
            &mut *stores.records,
            &mut *stores.attributes,
            None,
            key_of(EntityRef::Vertex(v)),
            &|vu| key_of(EntityRef::Vertexuse(vu)),
        )?;
    }
    for f in changed {
        create_uv_trim_curves(k, b, f, true, None)?;
    }
    Ok(())
}

/// The exact curve [`make_edges_on_analytics_exact`] gives the edge `e`, or `None` where it
/// keeps its marched curve: the edge is outside the pass, or its exact curve cannot be
/// found or checked (a grown copy refused, the intersection or the trimmed curve failing,
/// no line or conic through its three points, the piece failing its validity check).
///
/// # Errors
///
/// [`SolidError::Unported`] and [`SolidError::Cancelled`] from the growth, the
/// intersection and the trimmed curve; those of the topology reads, the surface copies
/// and domains and the evaluations; a defect where an evaluation gives no tangent.
/// `exact_piece`'s [`SolidError::NotConverged`] is its turn-count guard, not a failed
/// match: the STEP parameters it counts come from the conic's own inversion, within a
/// turn, so only a broken conic reaches it, and that propagates.
fn exact_edge(
    k: &mut Kernel,
    b: &mut Brep,
    e: EdgeId,
    tol: f64,
    ssi_tol: f64,
    deg20_rad: f64,
    margin: f64,
) -> Result<Option<ExactEdge>> {
    let use_surface_edges = [false, false];
    let Some(curve) = topo_edge::curve(b, e)? else {
        return Ok(None);
    };
    if curve.is_analytic() {
        return Ok(None);
    }
    let faces = topo_edge::faces(b, e, None)?;
    let &[f0, f1] = faces.as_slice() else {
        return Ok(None);
    };
    if f0 == f1 {
        return Ok(None);
    }
    let (Some(s1), Some(s2)) = (b.face(f0)?.surface.as_ref(), b.face(f1)?.surface.as_ref()) else {
        return Ok(None);
    };
    if !s1.is_analytic()? || !s2.is_analytic()? {
        return Ok(None);
    }
    // The faces' own surfaces: the SSI reaches each one's cache through its face
    // (`SmSurface::GetFace`, a trimmed surface cache), so the evaluation copies
    // (same owner and cache slot) are intersected through the B-rep's face
    // services, the handles naming the two faces.
    let mut s1 = s1.clone_for_evaluation()?;
    let mut s2 = s2.clone_for_evaluation()?;
    // A copy that cannot be grown (its domain refused, e.g. rounding past the apex)
    // leaves the edge marched rather than fail the shell.
    match grow_along_axis(k, &mut s1, margin).and_then(|()| grow_along_axis(k, &mut s2, margin)) {
        Ok(()) => {}
        Err(err @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => return Err(err),
        Err(_) => return Ok(None),
    }
    let domain1 = s1.natural_uv_domain_with_nurb(k)?;
    let domain2 = s2.natural_uv_domain_with_nurb(k)?;
    // The surfaces' own intersection: none found (tangent surfaces) leaves the edge.
    let exact = match s1.global_surface_intersect_with_context(
        k,
        [
            Some(CacheObject::Surface(f0)),
            Some(CacheObject::Surface(f1)),
        ],
        &mut BrepFaceServices::new(&mut *b),
        &domain1,
        &s2,
        &domain2,
        use_surface_edges,
        Some(ssi_tol),
        Some(deg20_rad),
    ) {
        Ok(exact) => exact,
        Err(err @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => return Err(err),
        Err(_) => return Ok(None),
    };
    let trimmed = match topo_edge::create_trimmed_nurbs_curve(k, b, e) {
        Ok(Some(trimmed)) => trimmed,
        Err(err @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => return Err(err),
        Ok(None) | Err(_) => return Ok(None),
    };
    let Some(mut found) = exact_piece(k, &trimmed, &exact.curves_3d, tol)? else {
        return Ok(None);
    };
    // The piece runs as the edge does (exact_piece may give it end to start).
    let old = trimmed.evaluate(k, trimmed.natural_interval().evaluate(0.5), 1, true, true)?;
    let (new, found_ivl) = {
        let probe = Curve::from(found.clone());
        let ivl = probe.natural_interval();
        (probe.evaluate(k, ivl.evaluate(0.5), 1, true, true)?, ivl)
    };
    let (Some(old_tangent), Some(new_tangent)) = (old.get(1), new.get(1)) else {
        return Err(defect(OP_ANALYTICS_EXACT, NO_TANGENT, SM_ERR));
    };
    if old_tangent.dot(*new_tangent) < 0.0 {
        found.reverse_parameterization(k, &found_ivl)?;
    }
    // A conic whose own parameters SMLib cannot hold consistently (its polar
    // conversion off by more than its own zero, far from the origin) fails its
    // validity check: the edge keeps its marched curve rather than take it.
    if !found.assert_valid(k)? {
        return Ok(None);
    }
    let piece = Curve::from(found);
    // The edge's curve becomes the piece over its natural interval (Replace3DCurve).
    let ivl = piece.natural_interval();
    let ends = [
        piece.evaluate_point(k, ivl.min())?,
        piece.evaluate_point(k, ivl.max())?,
    ];
    let (start, end) = topo_edge::start_end_vertices(b, e)?;
    Ok(Some(ExactEdge {
        edge: e,
        piece,
        ends,
        faces: [f0, f1],
        vertices: [start, end],
    }))
}

/// `pSur->CreateOffsetSurface(ctx, signed_distance, approx_tol_3d, rNewSurface)`: the
/// virtual call ([`Surface::create_offset_surface`] dispatches on the kind: the plane,
/// surface of revolution and offset surface overrides, else `SmSurface`'s, which
/// `SmBSplineSurface`'s calls: an `SmOffsetSurface` owning a copy). `None` is
/// upstream's success with `rNewSurface == NULL` (the `SmTorus` override, whose minor
/// radius vanishes, not reached: no [`SurfaceKind`] is a torus yet; the `SmCone`
/// override when both offset radii collapse).
fn create_offset_surface(
    k: &Kernel,
    s: &Surface,
    signed_distance: f64,
    approx_tol_3d: f64,
) -> Result<Option<Surface>> {
    s.create_offset_surface(k, signed_distance, approx_tol_3d)
}

/// `IsArc(samples, tol, ..)` of a plain B-spline curve (a rail or an arc this module
/// built).
fn is_arc(k: &Kernel, c: &BSplineCurve, num_samples: u64, tol: f64) -> Result<Option<ArcData>> {
    Curve::from(c.clone()).is_arc(k, num_samples, tol)
}

// ---- the geometry creation --------------------------------------------------------------------

/// Whether `curve` at `t` passes through `surface` at `uv` (where they meet) by more than
/// `tol`. To second order in arc length s the curve's height over the surface is
/// `g1 s + g2 s^2 / 2`, `g1` the sine of the angle between them and `g2` the curve's
/// curvature towards the surface's normal less the surface's normal curvature along the
/// curve: it is back on the surface at `-2 g1 / g2` and between the two it reaches
/// `g1^2 / (2 |g2|)` from the surface. Within `tol` that is a touch: a curve exact to `tol`
/// that touches the surface meets it twice, close together, wherever the error leaves it.
///
/// # Errors
///
/// [`OffsetFailure::CrossingUndecided`] where the curve stops or the surface is singular (no
/// tangent plane); those of the evaluations.
fn passes_through(
    k: &Kernel,
    curve: &Curve,
    t: f64,
    surface: &Surface,
    uv: Point2,
    tol: f64,
) -> Result<bool> {
    const OP: &str = "offset_geometry_creation::passes_through";
    let short = || defect(OP, SHORT_JET, SM_ERR);
    let undecided = || failed(OP, OffsetFailure::CrossingUndecided);
    let undecided_by = |e: SolidError| {
        Failure::new(OP, OffsetFailure::CrossingUndecided)
            .caused_by(e)
            .into_error()
    };
    let pv = curve.evaluate(k, t, 2, true, true)?;
    let (d1, d2) = (*pv.get(1).ok_or_else(short)?, *pv.get(2).ok_or_else(short)?);
    // Only the total-order-two jet is used below. The full rectangle would also
    // request D(2, 2), beyond an implicit offset's derivative limit.
    let s = surface.evaluate(
        k,
        uv,
        2,
        2,
        EvaluateFlags {
            only_upper_half: true,
            ..EvaluateFlags::default()
        },
    )?;
    let at = |i: usize, j: usize| s.get(i).and_then(|r| r.get(j)).copied().ok_or_else(short);
    let (du, dv, duu, duv, dvv) = (at(1, 0)?, at(0, 1)?, at(2, 0)?, at(1, 1)?, at(0, 2)?);
    // (a zero normal or tangent: the surface has no tangent plane, the curve stops)
    let n = du.cross(dv).unitize().map_err(undecided_by)?;
    let speed2 = d1.length_squared();
    let tangent = d1.unitize().map_err(undecided_by)?;
    // the tangent's components along du and dv
    let (e, f, g) = (du.dot(du), du.dot(dv), dv.dot(dv));
    let det = e * g - f * f;
    if det <= EFF_ZERO * e * g {
        return Err(undecided());
    }
    let (tu, tv) = (tangent.dot(du), tangent.dot(dv));
    let (a, b) = ((g * tu - f * tv) / det, (e * tv - f * tu) / det);
    let normal_curvature = n.dot(duu) * a * a + 2.0 * n.dot(duv) * a * b + n.dot(dvv) * b * b;
    let g1 = n.dot(tangent);
    let g2 = n.dot(d2 - tangent * d2.dot(tangent)) / speed2 - normal_curvature;
    Ok(g1 * g1 > 2.0 * g2.abs() * tol)
}

/// The offset geometry creator (`SmOffsetGeometryCreation`).
#[derive(Debug, Clone, PartialEq)]
pub struct OffsetGeometryCreation {
    /// `m_dOffsetDistance`.
    offset_distance: f64,
    /// `m_dThisApproxTol3d`.
    this_approx_tol_3d: f64,
    /// `m_dTangencyTolRadians`.
    tangency_tol_radians: f64,
    /// `m_bMakeAnalytics`.
    make_analytics: bool,
}

impl OffsetGeometryCreation {
    /// `SmOffsetGeometryCreation(dOffsetDistance, dThisApproxTol3d)`: tangency
    /// tolerance 2 degrees, analytics made.
    #[must_use]
    pub fn new(offset_distance: f64, this_approx_tol_3d: f64) -> Self {
        Self {
            offset_distance,
            this_approx_tol_3d,
            tangency_tol_radians: 2.0 * PI / 180.0,
            make_analytics: true,
        }
    }

    /// The constructor's angular tolerance (`m_dTangencyTolRadians`): half a degree,
    /// `0.5 * PI / 180.0` (WR-666, the fork's tangency-labs/smlib@2e1bb8c), not upstream's
    /// two degrees, on the default path since #2981.
    #[must_use]
    pub fn default_tangency_tol_radians() -> f64 {
        0.5 * PI / 180.0
    }

    /// The same creator with the angular tolerance `tangency_tol_radians`, which an
    /// upstream creator subclass may override (`m_dTangencyTolRadians`).
    #[must_use]
    pub const fn with_tangency_tol_radians(mut self, tangency_tol_radians: f64) -> Self {
        self.tangency_tol_radians = tangency_tol_radians;
        self
    }

    /// `GetThisApproxTol3d` (`SM_ASSERT_TOL` only reports).
    #[must_use]
    pub const fn this_approx_tol_3d(&self) -> f64 {
        self.this_approx_tol_3d
    }

    /// `GetTangencyTolRadians`.
    #[must_use]
    pub const fn tangency_tol_radians(&self) -> f64 {
        self.tangency_tol_radians
    }

    /// `GetTangencyTolDegrees`.
    #[must_use]
    pub fn tangency_tol_degrees(&self) -> f64 {
        self.tangency_tol_radians * 180.0 / PI
    }

    /// `GetOffsetDistance`.
    #[must_use]
    pub const fn offset_distance(&self) -> f64 {
        self.offset_distance
    }

    /// `m_bMakeAnalytics`.
    #[must_use]
    pub const fn make_analytics(&self) -> bool {
        self.make_analytics
    }

    /// `SetMakeAnalytics`.
    pub fn set_make_analytics(&mut self, make_analytics: bool) {
        self.make_analytics = make_analytics;
    }

    /// `SmOffsetGeometryCreation::FaceuseOffset`: the offset surface of the face use's
    /// face (offset against the surface normal for an opposite face use), made
    /// analytic when asked and possible, and, unless `skip_self_intersection`, its
    /// self-intersections. `None` when no offset surface was made.
    ///
    /// # Errors
    ///
    /// [`SolidError::Unported`] for the offset surface (`surface.offset_surface`);
    /// `SER` of the analytic copy and the self-intersection;
    /// [`SolidError::InvalidInput`] for a face use without face or surface.
    pub fn faceuse_offset<O: ClassObject + SolutionObject>(
        &self,
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        original_faceuse: O,
        skip_self_intersection: bool,
    ) -> Result<(Option<O>, SsiData<O>)> {
        self.faceuse_offset_with_backend(
            k,
            &mut OffsetFaceBrep(brep),
            original_faceuse,
            skip_self_intersection,
        )
    }

    /// Shared face-use algorithm for the full creator and owned executive adapter.
    pub(crate) fn faceuse_offset_with_backend<O: Copy>(
        &self,
        k: &Kernel,
        brep: &mut impl FaceOffsetBrep<O>,
        original_faceuse: O,
        skip_self_intersection: bool,
    ) -> Result<(Option<O>, SsiData<O>)> {
        // init output
        let mut self_intersections = SsiData::default();

        // locals
        let sign = if brep.faceuse_orientation(original_faceuse)? == OrientType::Opposite {
            -1.0
        } else {
            1.0
        };
        let face = brep
            .faceuse_face(original_faceuse)?
            .ok_or(dangling("face use without face"))?;
        let sur = brep
            .face_surface(face)?
            .ok_or(dangling("face without surface"))?;

        // WR-464 (#1072; the fork's 758829c): an
        // offset that shrinks the surface onto its spine or past it is no surface, as the
        // surface says, asked before the offset is made (an exact offset that refuses to pass
        // its centre, a ball patch's, is never asked for); any other miss has failed.
        if brep.surface(sur)?.offset_collapses(
            k,
            sign * self.offset_distance,
            self.this_approx_tol_3d / 10.0,
        )? {
            return Ok((None, self_intersections));
        }

        // construct an offset surface from this surface
        // (SER_DELETE: the surface is deleted on failure; here its handle is simply not returned)
        let Some(offset) = create_offset_surface(
            k,
            brep.surface(sur)?,
            sign * self.offset_distance,
            self.this_approx_tol_3d / 10.0,
        )?
        else {
            return Err(defect(OP_FACEUSE, OFFSET_SURFACE_COLLAPSED_LATE, SM_ERR));
        };
        let offset_h = brep.insert_surface(offset)?;
        let mut new_surface: Option<O> = Some(offset_h);

        // when working with analytics - replace all current appropriate BSplineSurface
        // with analytic copies when the surface is properly represented
        if self.make_analytics {
            // skip surfaces which are already analytics
            if !brep.surface(offset_h)?.is_analytic()? {
                // see if this surface is secretly an analytic - Copy pS, when possible as an analytic surface
                let mut analytic = brep.surface(offset_h)?.copy_and_add_analytics(k)?;
                if let SurfaceKind::OffsetSurface(offset) = analytic.kind_mut() {
                    // SM_CAST_PTR(SmOffsetSurface, pAnalyticS)
                    offset.set_allow_extension(false);
                }
                // store the copy
                new_surface = Some(brep.insert_surface(analytic)?);
            }
        } // end m_bMakeAnalytics check

        // when only one offset surface was made and checking for self-intersections
        if let (Some(off), false) = (new_surface, skip_self_intersection) {
            // self-intersection locals
            let angle_tol = 20.0 * PI / 180.0;
            let use_surface_edges = [false, false];
            let uv_domain = brep.surface(off)?.natural_uv_domain_with_nurb(k)?;

            // find self intersections
            let ssi = {
                let s = brep.surface(off)?;
                s.global_surface_intersect(
                    k,
                    &uv_domain,
                    s,
                    &uv_domain,
                    use_surface_edges,
                    Some(self.this_approx_tol_3d),
                    Some(angle_tol),
                )?
            };
            for c in ssi.curves_3d {
                self_intersections.curves_3d.push(brep.insert_curve(c)?);
            }
            for c in ssi.surface1_uv_curves {
                let h = match c {
                    Some(c) => Some(brep.insert_curve(c)?),
                    None => None,
                };
                self_intersections.uv_curves1.push(h);
            }
            for c in ssi.surface2_uv_curves {
                let h = match c {
                    Some(c) => Some(brep.insert_curve(c)?),
                    None => None,
                };
                self_intersections.uv_curves2.push(h);
            }
            self_intersections.curve_types = ssi.curve_types;
            self_intersections.deviations = ssi.deviations;
        } // end self-intersection check

        // all done
        Ok((new_surface, self_intersections))
    }

    /// `VertexOffset(ctx, pOriginalVertex, rBoundingEdges, ..)`: the cap surface of a
    /// vertex of the offset from the offset edges bounding it. One bounding edge that
    /// is a seam, or two coincident bounding edges, need no cap (`None`); a vertex
    /// without exactly three bounding edges, or a lamina vertex, gets a sphere (of
    /// revolution of a half circle, or of the arc through a closed bounding arc);
    /// three bounding arcs get the sphere through them (`CreateSphereFromArcs`, with
    /// arcs that miss their neighbours' planes re-made between the circle/plane
    /// intersections), or, when shelling (an arc and two lines), the trimmed plane
    /// ruled between the arc and its centre.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` for a null vertex; `SM_ERR` when a bounding curve of a
    /// three-edge vertex is not an arc, when an arc/plane intersection is degenerate
    /// or too far, or when the sphere cannot be made; `SER` of the geometry calls;
    /// [`SolidError::InvalidInput`] for a bounding edge without curve or a non-B-spline
    /// curve cast as one.
    #[allow(clippy::too_many_lines)]
    pub fn vertex_offset<O: ClassObject + SolutionObject>(
        &self,
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        original_vertex: Option<O>,
        bounding_edges: &[O],
    ) -> Result<VertexOffsetGeometry<O>> {
        self.vertex_offset_with_backend(
            k,
            &mut OffsetVertexBrep(brep),
            original_vertex,
            bounding_edges,
        )
    }

    /// The shared vertex algorithm, also used by the executive's cross-arena adapter.
    #[allow(clippy::too_many_lines)]
    pub(crate) fn vertex_offset_with_backend<O: Copy>(
        &self,
        k: &Kernel,
        brep: &mut impl VertexOffsetBrep<O>,
        original_vertex: Option<O>,
        bounding_edges: &[O],
    ) -> Result<VertexOffsetGeometry<O>> {
        // init output
        let original_vertex =
            original_vertex.ok_or_else(|| failed(OP_VERTEX, OffsetFailure::VertexNotGiven))?;
        let mut out = VertexOffsetGeometry {
            trimming_curves: Vec::new(),
            orients: Vec::new(),
            surface: None,
        };

        // locals
        let tolerance = brep.brep_tolerance(original_vertex)?;

        // If we have no bounding edges or don't have 3, just create a spherical surface
        // and hope that the merge will take care of the rest.
        if bounding_edges.len() == 1 {
            // Check for closed case
            let e = bounding_edges
                .first()
                .copied()
                .ok_or(dangling("edge list"))?;
            let faces = brep.edge_faces(e)?;
            let f = faces
                .first()
                .copied()
                .ok_or(dangling("edge without faces"))?;
            if brep.edge_is_seam_of_face_surface(k, e, f)? {
                // no need for a vertex cap Surface
                return Ok(out);
            }
        } // end just 1 BoundingEdge check

        // Check for case where bounding edges are coincident.
        if bounding_edges.len() == 2 {
            let (e0, e1) = (
                bounding_edges
                    .first()
                    .copied()
                    .ok_or(dangling("edge list"))?,
                bounding_edges
                    .get(1)
                    .copied()
                    .ok_or(dangling("edge list"))?,
            );
            let c1 = brep.edge_curve(e0)?.ok_or(dangling("edge without curve"))?;
            let c2 = brep.edge_curve(e1)?.ok_or(dangling("edge without curve"))?;
            let ivl0 = brep.edge_interval(e0)?;
            let ivl1 = brep.edge_interval(e1)?;
            let param2_start = ivl1.min();
            let param2_end = ivl1.max();
            let max_dist = brep
                .curve(c1)?
                .curve_max_distance_between(
                    k,
                    &ivl0,
                    brep.curve(c2)?,
                    param2_start,
                    param2_end,
                    10,
                    Some(tolerance),
                )?
                .distance;
            if max_dist < tolerance {
                // Don't need to do anything here just quit because they are the same geometric edges.
                return Ok(out);
            }

            // check to see if the curves have opposite parameterization
            let max_dist = brep
                .curve(c1)?
                .curve_max_distance_between(
                    k,
                    &ivl0,
                    brep.curve(c2)?,
                    param2_end,
                    param2_start,
                    10,
                    Some(tolerance),
                )?
                .distance;
            if max_dist < tolerance {
                // Don't need to do anything here just quit because they are the same geometric edges.
                return Ok(out);
            }
        } // end 2 coincident bounding curve checks

        // when vertex does not have 3 bounding edges or is part of a lamina edge
        if bounding_edges.len() != 3 || brep.vertex_is_lamina(original_vertex)? {
            let mut arc: Option<BSplineCurve> = None;
            let mut placement = Axis2Placement::IDENTITY;
            let vertex_point = brep.vertex_point(original_vertex)?;
            placement.translate(vertex_point);

            // when there is just 1 bounding edge
            if bounding_edges.len() == 1 {
                let e = bounding_edges
                    .first()
                    .copied()
                    .ok_or(dangling("edge list"))?;
                let c = brep.edge_curve(e)?.ok_or(dangling("edge without curve"))?;
                // SM_CAST_PTR(SmBSplineCurve, curve): the failed cast is dereferenced
                let blend = vertex_b_spline_kind_of(brep, c)?
                    .ok_or(dangling("bounding curve is not a B-spline curve"))?;
                let ivl = brep.edge_interval(e)?;
                if blend.is_closed(k, &ivl, 0.0)? {
                    let mut ref_frame = Axis2Placement::IDENTITY;
                    if let Some((frame, _arc_rad, _start_ang, _end_ang)) =
                        blend.is_arc(k, 10, tolerance)?
                    {
                        ref_frame = frame;
                        let mut z_axis = ref_frame.z_axis();
                        let center = vertex_point;
                        let vec_to_edge = ref_frame.origin() - center;
                        if vec_to_edge.dot(z_axis) < 0.0 {
                            z_axis = -z_axis;
                        }
                        let point1 = blend.evaluate_point(k, blend.natural_interval().min())?;
                        let point2 = center + self.offset_distance * z_axis;
                        arc = Some(BSplineCurve::create_arc_from_points(
                            center,
                            point1,
                            point2,
                            NurbCircleParam::Quadratic,
                        )?);
                    }
                    // SetCanonical's status is ignored upstream (fork 4803d6d: `SER`)
                    placement.set_canonical(
                        placement.origin(),
                        ref_frame.z_axis(),
                        ref_frame.x_axis(),
                    )?;
                }
            } // end 1 bounding edge check

            // create the vertex endCap surface
            let sphere = match arc {
                None => {
                    let arc = BSplineCurve::create_circle_segment(
                        3,
                        &placement,
                        self.offset_distance,
                        0.0,
                        180.0,
                        NurbCircleParam::Quadratic,
                    )?;
                    BSplineSurface::create_surf_of_revolution(
                        k,
                        &arc,
                        placement.origin(),
                        placement.x_axis(),
                        180.0,
                        false,
                        None,
                    )?
                }
                Some(arc) => BSplineSurface::create_surf_of_revolution(
                    k,
                    &arc,
                    placement.origin(),
                    placement.x_axis(),
                    360.0,
                    false,
                    None,
                )?,
            };
            out.surface = Some(brep.insert_surface(Surface::from(sphere))?);
        } else {
            // 3 boundary curves none of which are lamina
            // In this simple case we will assume that we are doing a constant radius
            // offset and that all of the edges are arcs of the same radius
            // (SM_CAST_PTR(SmBSplineCurve, curve), NER: a missing curve or a failed cast is
            // SM_ERR_NULL_POINTER; the curves keep their kinds for the virtual calls below)
            let mut curves: Vec<Curve> = Vec::new();
            for &e in bounding_edges {
                let c = brep.edge_curve(e)?.ok_or_else(|| {
                    defect(OP_VERTEX, BOUNDING_EDGE_WITHOUT_CURVE, SM_ERR_NULL_POINTER)
                })?;
                // (an offset edge, named by no entity of the input)
                let bsc = vertex_b_spline_kind_of(brep, c)?.ok_or_else(|| {
                    let curve_type = brep.curve(c).map_or(0, Curve::type_code);
                    failed(
                        OP_VERTEX,
                        OffsetFailure::CurveKindUnsupported {
                            edge: None,
                            curve_type,
                        },
                    )
                })?;
                curves.push(bsc.clone());
            }

            // Please note that some of the arcs may not be complete - Check them
            // against the corresponding 3 planes and see if one of them needs to be extended.
            if curves.len() == 3 {
                // Check for shelling case
                {
                    let mut shell_arc: Option<usize> = None;
                    let mut line_count = 0usize;
                    let mut frame = Axis2Placement::IDENTITY;
                    for (jj, line) in curves.iter().enumerate() {
                        if let Some((f, _radius, _start, _end)) = line.is_arc(k, 10, tolerance)? {
                            frame = f;
                            shell_arc = Some(jj);
                        }
                        if line.is_line(k, 10, tolerance)?.is_some() {
                            line_count += 1;
                        }
                    }
                    if let (Some(arc_idx), 2) = (shell_arc, line_count) {
                        let point_curve = BSplineCurve::create_degenerate_curve(3, frame.origin())?;
                        // Now that we have the curves create the geometry for the surface.
                        // (`*pShellArc` as a `const SmBSplineCurve &`: the callee reads the
                        // ends and the NURB of an arc, which no kind overrides)
                        let shell_arc = curves.get(arc_idx).ok_or(dangling("arc index"))?;
                        let shell_arc = b_spline_part(shell_arc)?.ok_or(dangling("arc kind"))?;
                        let trimmed = BSplineSurface::create_trimmed_plane_from_ruled(
                            k,
                            &shell_arc,
                            &point_curve,
                        )?;
                        for c in trimmed.trimming_curves {
                            out.trimming_curves.push(brep.insert_curve(c)?);
                        }
                        out.orients = trimmed.trim_orientations;
                        out.surface = Some(brep.insert_surface(Surface::from(trimmed.plane))?);
                        return Ok(out);
                    }
                }

                // Create planes
                let tol = self.this_approx_tol_3d();
                let mut plane_points = [Point3::new(0.0, 0.0, 0.0); 3];
                let mut plane_normals = [Vec3::new(0.0, 0.0, 0.0); 3];
                for j in 0..curves.len() {
                    let arc = curves.get(j).ok_or(dangling("curve index"))?;
                    // If we have a line then we probably are shelling
                    let Some((frame, _radius, _start, _end)) = arc.is_arc(k, 10, tol)? else {
                        return Err(failed(OP_VERTEX, OffsetFailure::VertexCurveNotArc));
                    };
                    let next = (j + 1) % curves.len();
                    let other = curves.get(next).ok_or(dangling("curve index"))?;
                    let mid = other.evaluate_point(k, other.natural_interval().evaluate(0.5))?;
                    let pp = frame.origin();
                    let mut pn = frame.z_axis();
                    let vec = pp - mid;
                    if vec.dot(pn) < 0.0 {
                        pn = -pn;
                    }
                    if let (Some(p), Some(n)) = (plane_points.get_mut(j), plane_normals.get_mut(j))
                    {
                        *p = pp;
                        *n = pn;
                    }
                }

                // (bShelling is never set: its branch is empty upstream)

                // See if all arc points lie on 2 planes
                for kidx in 0..curves.len() {
                    // (upstream keeps pArc and its ends from before the kk loop: a replacement
                    // of sCurves[k] inside the loop does not change what the next kk reads)
                    let orig_arc = curves.get(kidx).ok_or(dangling("curve index"))?.clone();
                    let (start, end) = {
                        let ends = orig_arc.ends(k)?;
                        (ends.start, ends.end)
                    };
                    for kk in 0..curves.len() {
                        if kk == kidx {
                            continue;
                        }
                        let (pp, pn) = (
                            *plane_points.get(kk).ok_or(dangling("plane index"))?,
                            *plane_normals.get(kk).ok_or(dangling("plane index"))?,
                        );
                        let proj_point = point_project_to_plane(start, pp, pn);
                        let dist1 = distance(proj_point, start);
                        let proj_point2 = point_project_to_plane(end, pp, pn);
                        let dist2 = distance(proj_point2, end);
                        if dist1 > tol && dist2 > tol {
                            // Have a bad arc because at least one point needs to be on the plane.
                            let mut other = (kk + 1) % 3;
                            if other == kidx {
                                other = (other + 1) % 3;
                            }

                            let arc = &orig_arc;
                            // the status of IsArc is not checked here; a non-arc leaves the
                            // frame and radius uninitialised upstream
                            let Some((frame, radius, _, _)) = arc.is_arc(k, 10, tol)? else {
                                return Err(dangling("arc data of a non-arc read"));
                            };
                            let mid =
                                arc.evaluate_point(k, arc.natural_interval().evaluate(0.5))?;

                            let closest_pt1 =
                                Self::closest_circle_plane_point(tol, radius, &frame, pp, pn, mid)?;
                            let (op, on) = (
                                *plane_points.get(other).ok_or(dangling("plane index"))?,
                                *plane_normals.get(other).ok_or(dangling("plane index"))?,
                            );
                            let closest_pt2 =
                                Self::closest_circle_plane_point(tol, radius, &frame, op, on, mid)?;

                            // Make a new arc using the intersection points.
                            let p0 = *plane_points.first().ok_or(dangling("plane index"))?;
                            let new_arc = BSplineCurve::create_arc_from_points(
                                p0,
                                closest_pt1,
                                closest_pt2,
                                NurbCircleParam::Quadratic,
                            )?;
                            if let Some(slot) = curves.get_mut(kidx) {
                                *slot = Curve::from(new_arc);
                            }
                        }
                    }
                }
            } // end exactly 3 curves check

            // (any failure is SM_ERR, keeping the sphere's error as its cause, which decides
            // its class: the planes checked above do not prove the arcs of one sphere; an
            // unported callee stays unported)
            let sphere = match BSplineSurface::create_sphere_from_arcs(k, &curves, tolerance) {
                Ok(sphere) => sphere,
                Err(e @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => {
                    return Err(e)
                }
                Err(e) => {
                    return Err(
                        Failure::new(OP_VERTEX, OffsetFailure::VertexArcsNotOneSphere)
                            .caused_by(e)
                            .into_error(),
                    )
                }
            };
            let trim_curves: Vec<Curve> = sphere
                .ccw_arcs
                .iter()
                .filter_map(|&i| curves.get(i).cloned())
                .collect();
            out.orients = sphere.orients;
            out.surface = Some(brep.insert_surface(Surface::from(sphere.sphere))?);
            for orig in trim_curves {
                // SM_CAST_PTR(SmBSplineCurve, curve), new SmBSplineCurve(*pOrig): a plain
                // B-spline copy of the curve's NURB part
                let copy = b_spline_part(&orig)?.ok_or(dangling("trim curve kind"))?;
                out.trimming_curves
                    .push(brep.insert_curve(Curve::from(copy))?);
            }
        } // end given 3 boundary curves branch

        // all done
        Ok(out)
    }

    /// The "find the closest point to the plane" blocks of `VertexOffset`: of the
    /// intersections of the arc's circle with a plane, the one closest to the arc's
    /// midpoint; `SM_ERR` for three intersections (impossible) or when the closest is
    /// farther than ten radii.
    fn closest_circle_plane_point(
        tol: f64,
        radius: f64,
        frame: &Axis2Placement,
        plane_point: Point3,
        plane_normal: Vec3,
        mid: Point3,
    ) -> Result<Point3> {
        let mut closest_dist = BIG_DOUBLE;
        let mut closest = Point3::new(0.0, 0.0, 0.0);
        let points = match circle_plane_intersect(
            tol,
            radius,
            frame.origin(),
            frame.z_axis(),
            plane_point,
            plane_normal,
        )? {
            CurveIntersection::Points(p) => p,
            CurveIntersection::Coincident => {
                return Err(failed(OP_VERTEX, OffsetFailure::VertexArcInPlane))
            }
        };
        for &p in points.iter().take(2) {
            let dist = distance(p, mid);
            if dist < closest_dist {
                closest_dist = dist;
                closest = p;
            }
        }
        if closest_dist > radius * 10.0 {
            return Err(failed(
                OP_VERTEX,
                OffsetFailure::VertexArcMissesPlane {
                    distance: closest_dist,
                    radius,
                },
            ));
        }
        Ok(closest)
    }

    /// `UpdateFaceSurfaceDomain(pFace, pExtendedSurface)`: sets the face's UV domain to
    /// that of its UV trim curves (snapped to the surface domain's edges within
    /// `SM_EFF_ZERO`), and replaces the face's surface by a copy of the extended one
    /// when the new domain is not inside the surface's.
    ///
    /// # Errors
    ///
    /// `SM_ERR` when the trim curve domain cannot be computed;
    /// [`SolidError::Unported`] from `SmFace::CalculateUVDomainFromUVTrimCurves`' callees
    /// (`CreateUVTrimCurves`, `topology.face_p3`); those of `SmBrep::ReplaceSurface`
    /// ([`OffsetBrep::brep_replace_surface`]).
    pub fn update_face_surface_domain<O: ClassObject + SolutionObject>(
        &self,
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        face: O,
        extended_surface: O,
    ) -> Result<()> {
        let domain_unresolved = |e: SolidError| {
            Failure::new(OP_UPDATE, OffsetFailure::FaceDomainUnresolved)
                .caused_by(e)
                .into_error()
        };
        let face_domain = match brep.face_uv_domain_from_uv_trim_curves(k, face) {
            Ok(d) => d,
            // `SmExtent2d::Intersect`'s `SM_ERR_INVALID_INPUT` (disjoint domains) is a real
            // status upstream, which `!= SM_SUCCESS` turns into `SM_ERR`; every other
            // `InvalidInput` is a null dereference upstream.
            // (each `SM_ERR`, keeping the error of the step that failed as its cause)
            Err(
                e @ SolidError::InvalidInput {
                    operation: "geom::extent2d::Extent2d::intersect",
                    ..
                },
            ) => return Err(domain_unresolved(e)),
            Err(
                e @ (SolidError::Unported { .. }
                | SolidError::Cancelled { .. }
                | SolidError::InvalidInput { .. }
                | SolidError::PortInvariant { .. }),
            ) => return Err(e),
            Err(e) => return Err(domain_unresolved(e)),
        };

        let sur = brep
            .face_surface(face)?
            .ok_or(dangling("face without surface"))?;
        let surf_domain = surface_of(brep, sur)?.natural_uv_domain_with_nurb(k)?;
        let mut min = face_domain.min();
        let mut max = face_domain.max();
        if (min.x - surf_domain.min().x).abs() < EFF_ZERO {
            min.x = surf_domain.min().x;
        }
        if (min.y - surf_domain.min().y).abs() < EFF_ZERO {
            min.y = surf_domain.min().y;
        }
        if (max.x - surf_domain.max().x).abs() < EFF_ZERO {
            max.x = surf_domain.max().x;
        }
        if (max.y - surf_domain.max().y).abs() < EFF_ZERO {
            max.y = surf_domain.max().y;
        }
        let new_domain = Extent2d::from_points(min, max);
        brep.set_face_uv_domain(face, new_domain)?;

        if new_domain.is_contained_by(&surf_domain, EFF_ZERO) {
            return Ok(()); // no extension surface needed
        }

        // Need to replace it with the extended surface
        let ext = surface_of(brep, extended_surface)?.copy(k)?;
        let ext_h = store_surface(brep, ext)?;
        brep_replace_surface(k, brep, face, ext_h, false, false, true)?;

        Ok(())
    }

    /// `ReplaceEdgeCurves(pEdgeuse, rp3DCurve, rpUVCurve, cpOptOrigEdgeuse, ..)`: when
    /// `curve_3d` is given and its ends differ from the edge curve's (in either order,
    /// beyond the edge tolerance), the curve (reversed when it runs against the edge
    /// use's, or the original edge use's, curve) becomes the edge's curve and interval;
    /// the UV curve becomes the edge use's trim curve in every case; with a new 3D curve
    /// the start and end vertices move to its ends. Returns whether the caller should
    /// delete the candidate curves (both, when the ends matched).
    ///
    /// # Errors
    ///
    /// `SER` of the evaluations (`SmEdgeuse::NormalizedEvaluate` included) and the
    /// vertex moves;
    /// [`SolidError::InvalidInput`] for an edge use without edge, an edge without
    /// curve, or a null UV curve where upstream dereferences it.
    pub fn replace_edge_curves<O: ClassObject + SolutionObject>(
        &self,
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        edgeuse: O,
        curve_3d: Option<O>,
        uv_curve: Option<O>,
        opt_orig_edgeuse: Option<O>,
    ) -> Result<ReplaceEdgeCurves> {
        Self::replace_edge_curves_oriented(
            k,
            brep,
            edgeuse,
            curve_3d,
            uv_curve,
            opt_orig_edgeuse,
            false,
        )
    }

    /// [`Self::replace_edge_curves`] for an edge whose offset may be inverted (not
    /// upstream, solidcraft#16983; see [`TrimmedEdge`]): an inverted edge's new curve runs
    /// against the edge use's (or the original edge use's) curve, from the image of the
    /// edge's start vertex to the image of its end vertex, and is set even where its ends
    /// are the edge curve's ends in reverse order. A curve whose ends match is kept as
    /// upstream keeps it ([`KeepOld::OnMatchingEnds`]).
    ///
    /// # Errors
    ///
    /// As [`Self::replace_edge_curves`].
    #[allow(clippy::too_many_arguments)]
    fn replace_edge_curves_oriented<O: ClassObject + SolutionObject>(
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        edgeuse: O,
        curve_3d: Option<O>,
        uv_curve: Option<O>,
        opt_orig_edgeuse: Option<O>,
        inverted: bool,
    ) -> Result<ReplaceEdgeCurves> {
        Self::replace_edge_curves_in(
            k,
            brep,
            edgeuse,
            curve_3d,
            uv_curve,
            opt_orig_edgeuse,
            KeepOld::OnMatchingEnds,
            inverted,
        )
    }

    /// Whether [`Self::replace_edge_curves_in`]'s shortcut would keep `edgeuse`'s edge
    /// against the candidate `curve_3d` in `MateEdges` (not upstream): its curve's ends match
    /// the candidate's within the edge tolerance (either way round) and it is proved within
    /// that tolerance of both `surfaces`
    /// ([`crate::offset::curve_on_surfaces::curve_on_surfaces`]).
    fn keeps_old_edge<O: ClassObject + SolutionObject>(
        k: &Kernel,
        brep: &impl OffsetBrep<O>,
        edgeuse: O,
        curve_3d: O,
        surfaces: [&Surface; 2],
    ) -> Result<bool> {
        let edge = brep
            .edgeuse_edge(edgeuse)?
            .ok_or(dangling("edge use without edge"))?;
        let eu_curve = brep
            .edge_curve(edge)?
            .ok_or(dangling("edge without curve"))?;
        let edge_ivl = brep.edge_interval(edge)?;
        let edge_tol = brep.edge_tolerance(edge)?;
        let old = curve_of(brep, eu_curve)?;
        let (s, e) = (
            old.evaluate_point(k, edge_ivl.min())?,
            old.evaluate_point(k, edge_ivl.max())?,
        );
        let new = curve_of(brep, curve_3d)?;
        let ivl = new.natural_interval();
        let (cs, ce) = (
            new.evaluate_point(k, ivl.min())?,
            new.evaluate_point(k, ivl.max())?,
        );
        let ends = (distance(s, cs) < edge_tol && distance(e, ce) < edge_tol)
            || (distance(s, ce) < edge_tol && distance(e, cs) < edge_tol);
        Ok(ends
            && crate::offset::curve_on_surfaces::curve_on_surfaces(
                k, old, edge_ivl, surfaces, edge_tol,
            )?)
    }

    /// [`Self::replace_edge_curves`], with `keep` choosing when a curve whose ends match the
    /// edge curve's leaves the edge as it is. Upstream always keeps it: that is sound where
    /// the candidate is the edge's own curve extended or trimmed (`ExtendTrimEdges`, the
    /// shell-tangent rails), as two such curves with the same ends are the same curve. Not
    /// upstream: `MateEdges`'s candidate is a different curve, the two surfaces'
    /// intersection, and matching ends say nothing about the old edge's interior: where the
    /// faces are tangent at both vertices and meet at an angle between them (a loft's ruling
    /// edge), each face's own offset edge ends at the intersection's ends but leaves it in
    /// between (by up to 0.032 for the hood corner sheet of
    /// `tests/issue_1502_thicken_rim_halves.rs` thickened by 2), and keeping them leaves the
    /// two offset faces apart along the edge. So `MateEdges` keeps each offset edge of the
    /// pair only where [`Self::keeps_old_edge`] holds for it (proved within its tolerance of
    /// both extended surfaces by [`crate::offset::curve_on_surfaces::curve_on_surfaces`]:
    /// closed form on planes and cones, interval bounds on B-spline nets and their
    /// offsets), and otherwise installs the intersection, upstream's own path for ends that
    /// do not match. Each use decides alone, as upstream, so one use of a pair can keep its
    /// old curve while the other installs the intersection. A kept edge is proved within
    /// its tolerance of both surfaces; an installed intersection is upstream's surface
    /// intersection and is not certified here. `inverted` is as for
    /// [`Self::replace_edge_curves_oriented`]: an inverted edge never takes the shortcut.
    #[allow(clippy::too_many_arguments)] // `ReplaceEdgeCurves`, the choice and the orientation
    fn replace_edge_curves_in<O: ClassObject + SolutionObject>(
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        edgeuse: O,
        curve_3d: Option<O>,
        uv_curve: Option<O>,
        opt_orig_edgeuse: Option<O>,
        keep: KeepOld,
        inverted: bool,
    ) -> Result<ReplaceEdgeCurves> {
        // locals
        let edge = brep
            .edgeuse_edge(edgeuse)?
            .ok_or(dangling("edge use without edge"))?;
        let eu_curve = brep.edge_curve(edge)?; // dereferenced only with a 3D curve

        let mut out = ReplaceEdgeCurves {
            delete_3d_curve: false,
            delete_uv_curve: false,
        };

        // when given a 3dCurve with a new shape - Update pEdge->Curve = rp3DCurve
        if let Some(c3) = curve_3d {
            // SM_ASSERT_MSG(rp3DCurve != pEUCurve) only reports
            let eu_curve = eu_curve.ok_or(dangling("edge without curve"))?;
            let edge_ivl = brep.edge_interval(edge)?;
            let edge_tol = brep.edge_tolerance(edge)?;

            // Edgesuse->Curve Start and End pts
            let edgeuse_start = curve_of(brep, eu_curve)?.evaluate_point(k, edge_ivl.min())?;
            let edgeuse_end = curve_of(brep, eu_curve)?.evaluate_point(k, edge_ivl.max())?;

            // input 3DCurve- Start and End pts
            let c3_ivl = curve_of(brep, c3)?.natural_interval();
            let curve_start = curve_of(brep, c3)?.evaluate_point(k, c3_ivl.min())?;
            let curve_end = curve_of(brep, c3)?.evaluate_point(k, c3_ivl.max())?;

            // Not upstream: ends that agree say "no change" only where the old curve lies on the
            // new one between them ([`curve_lies_on`]).
            let lies_on = |brep: &_| {
                curve_lies_on(
                    k,
                    curve_of(brep, eu_curve)?,
                    &edge_ivl,
                    curve_of(brep, c3)?,
                    &c3_ivl,
                    edge_tol,
                )
            };

            // (not upstream: where asked, the pair's joint decision, see `KeepOld`)
            let keep_old = |_: &mut _| -> Result<bool> {
                Ok(match keep {
                    KeepOld::OnMatchingEnds => true,
                    KeepOld::Decided(keep) => keep,
                })
            };

            // low work - Edgesuse->Curve and 3DCurve start and end Points are within tol
            if !inverted
                && distance(edgeuse_start, curve_start) < edge_tol
                && distance(edgeuse_end, curve_end) < edge_tol
                && lies_on(brep)?
                && keep_old(brep)?
            {
                // No change in ends of curve - don't bother using the inputs - delete them and get out.
                out.delete_3d_curve = true;
                out.delete_uv_curve = true;
                return Ok(out);
            }

            // low work - Edgeuse->Curve and 3DCurve start and end Points are within tol with reverse parameterization.
            if !inverted
                && distance(edgeuse_start, curve_end) < edge_tol
                && distance(edgeuse_end, curve_start) < edge_tol
                && lies_on(brep)?
                && keep_old(brep)?
            {
                // No change in ends of curve - just delete the curves and get out
                out.delete_3d_curve = true;
                out.delete_uv_curve = true;
                return Ok(out);
            }

            // Edgeuse MidPoint (use cpOptOrigEdgeuse if given)
            let pv = match opt_orig_edgeuse {
                None => {
                    let param = edge_ivl.evaluate(0.5);
                    curve_of(brep, eu_curve)?.evaluate(k, param, 1, true, true)?
                }
                Some(orig) => {
                    let orig_edge = brep
                        .edgeuse_edge(orig)?
                        .ok_or(dangling("edge use without edge"))?;
                    let orig_curve = brep
                        .edge_curve(orig_edge)?
                        .ok_or(dangling("edge without curve"))?;
                    let param = brep.edge_interval(orig_edge)?.evaluate(0.5);
                    curve_of(brep, orig_curve)?.evaluate(k, param, 1, true, true)?
                }
            };

            // input 3DCurve midPoint
            let pv2 = curve_of(brep, c3)?.evaluate(k, c3_ivl.evaluate(0.5), 1, true, true)?;

            // when input and existing edge curves run in opposite directions - reverse 3DCurve and UVCurve
            let t1 = pv.get(1).copied().ok_or(dangling("tangent missing"))?;
            let t2 = pv2.get(1).copied().ok_or(dangling("tangent missing"))?;
            // (not upstream) an inverted edge's curve runs the other way
            if (t1.dot(t2) < 0.0) != inverted {
                // Opposite orientation - reverse 3DCurve and UVCurve (statuses ignored
                // upstream; passed on here, WR-658)
                let ivl = curve_of(brep, c3)?.natural_interval();
                let c = brep
                    .curve_mut(c3)
                    .ok_or(dangling("curve handle cannot be resolved"))?;
                c.reverse_parameterization(k, &ivl)?;
                let uv = uv_curve.ok_or(dangling("null UV curve reversed"))?;
                let ivl = curve_of(brep, uv)?.natural_interval();
                let c = brep
                    .curve_mut(uv)
                    .ok_or(dangling("curve handle cannot be resolved"))?;
                c.reverse_parameterization(k, &ivl)?;
            } // end Edge and Input curve running in opposite directions check

            // set input curves as Edge geometry (deleting the pre-existing curve and the
            // UV trim curves) and the edge interval
            brep.edge_set_curve(k, edge, c3)?;
            let natural = curve_of(brep, c3)?.natural_interval();
            brep.set_edge_interval(edge, natural)?;
        } // end rp3DCurve existence check

        // Update Edgeuse UVTrimCurve = rpUVCurve (TRUE = delete existing UVTrimCurve)
        let edge_tol = brep.edge_tolerance(edge)?;
        brep.edgeuse_set_uv_trim_curve(k, edgeuse, uv_curve, edge_tol)?;

        // when given a 3DCurve - Update pEdgeuse Start and End Vertex->Positions = rp3DCurve->Evals
        if curve_3d.is_some() {
            // Update the geometry of the start vertex.
            let v = brep
                .edgeuse_vertex(edgeuse)?
                .ok_or(dangling("edge use without vertex"))?;
            let pnt = edgeuse_normalized_evaluate(k, brep, edgeuse, 0.0, false, false, true)?; // FALSE = 3d Eval
            brep.set_vertex_point(k, v, pnt)?;

            // Update the geometry of the other vertex
            let mate = brep
                .edgeuse_mate(edgeuse)?
                .ok_or(dangling("edge use without mate"))?;
            let v = brep
                .edgeuse_vertex(mate)?
                .ok_or(dangling("edge use without vertex"))?;
            let pnt = edgeuse_normalized_evaluate(k, brep, edgeuse, 1.0, false, false, true)?;
            brep.set_vertex_point(k, v, pnt)?;
        } // end given 3DCurve check

        // all done
        Ok(out)
    }

    /// `TrimExtendedEdge(ctx, exec, pBaseEdgeuse, pOtherBaseEdgeuse, pOffsetEdgeuse,
    /// pOtherOffsetEdgeuse, p3DCurve, pUVCurveBase, pUVCurveOtherBase, ..)`: the trim
    /// interval of the extended intersection curve, found end by end (the start vertex
    /// of the base edge use first, then its end vertex) from candidate trim points: the
    /// offset vertices of a neighbouring tangent or lamina edge use that was already
    /// mated, else the intersection of the base UV curve with the neighbour's UV trim
    /// curve extended ten lengths each way; the intersections of the 3D curve with the
    /// extended surfaces of the offsets of non-tangent neighbours (non-grazing ones
    /// only; not upstream, #17514: a grazing one too at an end without a tangent
    /// neighbour, where it meets the curve at more than the tangency tolerance); or, without candidates, the drop of the base vertex onto the curve. The
    /// candidate closest to the base vertex (and not the start already chosen) wins.
    /// Not upstream: beside a neighbour already offset (marked) whose faces were tangent in
    /// the input and whose new surfaces cross in a local operation
    /// (`OffsetExecutive::crosses_at`), that neighbour's vertices on the curve are the only
    /// candidates (a lamina base edge still takes its vertex's image).
    /// `None` (`bGoodEdge == FALSE`) when an end has no candidate or the interval is
    /// zero length. `offset_edgeuse` and `other_offset_edgeuse` are `NotUsed`.
    ///
    /// # Errors
    ///
    /// [`OffsetFailure::TangentEndOffCurve`] where such a neighbour has no vertex on the
    /// curve; `SER` of the point solves, intersections and evaluations;
    /// [`SolidError::InvalidInput`] for null objects upstream dereferences. `SER` of
    /// `SmEdgeuse::CreateUVTrimCurve` is included.
    #[allow(clippy::too_many_arguments)]
    pub fn trim_extended_edge<O: ClassObject + SolutionObject>(
        &self,
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        executive: &mut impl OffsetExecutive<O>,
        base_edgeuse: O,
        other_base_edgeuse: Option<O>,
        offset_edgeuse: Option<O>,
        other_offset_edgeuse: Option<O>,
        curve_3d: O,
        uv_curve_base: Option<O>,
        uv_curve_other_base: Option<O>,
        mark: MarkType,
    ) -> Result<Option<Extent1d>> {
        let _ = (offset_edgeuse, other_offset_edgeuse);
        Ok(self
            .trim_extended_edge_ends(
                k,
                brep,
                executive,
                base_edgeuse,
                other_base_edgeuse,
                curve_3d,
                uv_curve_base,
                uv_curve_other_base,
                mark,
            )?
            .map(|t| t.interval))
    }

    /// Whether an SSI component reaches a periodic seam's chart boundary. The line
    /// covers the complete extended chart, rather than the old seam's short trim.
    fn intersects_periodic_chart<O: ClassObject + SolutionObject>(
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        executive: &mut impl OffsetExecutive<O>,
        seam: O,
        uv: O,
        active: &Extent1d,
    ) -> Result<bool> {
        let radial = brep
            .edgeuse_radial(seam)?
            .ok_or(dangling("seam without radial"))?;
        let a = edgeuse_normalized_evaluate(k, brep, seam, 0.5, true, false, true)?;
        let z = edgeuse_normalized_evaluate(k, brep, radial, 0.5, true, false, true)?;
        let face = brep
            .edgeuse_face(seam)?
            .ok_or(dangling("seam without face"))?;
        let surface = brep
            .face_surface(face)?
            .ok_or(dangling("face without surface"))?;
        let natural = surface_of(brep, surface)?.natural_uv_domain_with_nurb(k)?;
        let offset = executive
            .offset_edgeuse(seam)
            .ok_or(dangling("seam without offset"))?;
        let offset_face = brep
            .edgeuse_face(offset)?
            .ok_or(dangling("seam offset without face"))?;
        let offset_surface = brep
            .face_surface(offset_face)?
            .ok_or(dangling("face without surface"))?;
        let (_, domain) = executive.get_create_extended_surface(k, brep, offset_surface)?;
        let edge = brep
            .edgeuse_edge(seam)?
            .ok_or(dangling("seam without edge"))?;
        let tolerance = brep.edge_tolerance(edge)?.max(brep.brep_tolerance(edge)?);
        let uv_tolerance = crate::surface::surface::p1::map_to_2d(
            k,
            tolerance,
            Point2::new(a.x, a.y),
            surface_of(brep, offset_surface)?,
        )?;
        if !uv_tolerance.is_finite() || uv_tolerance < 0.0 {
            return Err(SolidError::invalid_input(
                OP_TRIM,
                "invalid chart tolerance",
            ));
        }
        // The two uses of a seam differ by its chart period. This identifies the
        // periodic coordinate without an angular or distance threshold.
        let (lo, hi) = if (a.x - z.x).abs() > (a.y - z.y).abs() {
            let x = if (a.x - natural.min().x).abs() <= (a.x - natural.max().x).abs() {
                natural.min().x
            } else {
                natural.max().x
            };
            (
                Point3::new(x, domain.min().y, 0.0),
                Point3::new(x, domain.max().y, 0.0),
            )
        } else {
            let y = if (a.y - natural.min().y).abs() <= (a.y - natural.max().y).abs() {
                natural.min().y
            } else {
                natural.max().y
            };
            (
                Point3::new(domain.min().x, y, 0.0),
                Point3::new(domain.max().x, y, 0.0),
            )
        };
        let chart_curve = Curve::from(BSplineCurve::create_line_segment(3, lo, hi)?);
        let candidate = curve_of(brep, uv)?;
        let roots: SolutionArray<O> = candidate.global_curve_intersect(
            k,
            active,
            &chart_curve,
            &chart_curve.natural_interval(),
            uv_tolerance,
            &CurveIntersectOptions {
                skip_global_coincidence_check: false,
                handles: [Some(uv), None],
                analytic_handles: [None, None],
            },
        )?;
        Ok(!roots.is_empty())
    }

    /// Genuine boundary uses reached at the far ends of connected same-face seams.
    /// The explicit visited list bounds the walk by the source topology's edge count.
    fn boundaries_beyond_seam<O: ClassObject + SolutionObject>(
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        seam: O,
        vertex: O,
    ) -> Result<Vec<O>> {
        let face = brep
            .edgeuse_face(seam)?
            .ok_or(dangling("seam without face"))?;
        let faceuse = brep
            .edgeuse_faceuse(seam)?
            .ok_or(dangling("seam without face use"))?;
        let mut pending = vec![(seam, vertex)];
        let mut visited = Vec::new();
        let mut boundaries = Vec::new();
        while let Some((current, near)) = pending.pop() {
            k.check_cancelled(OP_TRIM)?;
            let edge = brep
                .edgeuse_edge(current)?
                .ok_or(dangling("seam without edge"))?;
            if visited.contains(&edge) {
                continue;
            }
            visited.push(edge);
            let Some(far) = brep.edge_other_vertex(edge, near)? else {
                continue;
            };
            for next in brep.vertex_edges(far)? {
                if next == edge {
                    continue;
                }
                for use_ in brep.edge_edgeuses(next)? {
                    if brep.edgeuse_faceuse(use_)? != Some(faceuse) {
                        continue;
                    }
                    let radial = brep
                        .edgeuse_radial(use_)?
                        .ok_or(dangling("edge use without radial"))?;
                    if brep.edgeuse_face(radial)? == Some(face)
                        && brep.edge_is_seam_of_face_surface(k, next, face)?
                    {
                        if !visited.contains(&next) {
                            pending.push((use_, far));
                        }
                    } else if !boundaries.contains(&use_) {
                        boundaries.push(use_);
                    }
                }
            }
        }
        Ok(boundaries)
    }

    /// The genuine third supports at the other end of an original intersection.
    /// Same-pair edges and periodic seams carry no additional material constraint.
    fn opposite_end_boundaries<O: ClassObject + SolutionObject>(
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        edgeuse: O,
        vertex: O,
    ) -> Result<Vec<O>> {
        let edge = brep
            .edgeuse_edge(edgeuse)?
            .ok_or(dangling("edge use without edge"))?;
        let Some(far) = brep.edge_other_vertex(edge, vertex)? else {
            return Ok(Vec::new());
        };
        let radial = brep
            .edgeuse_radial(edgeuse)?
            .ok_or(dangling("edge use without radial"))?;
        let faces = [brep.edgeuse_face(edgeuse)?, brep.edgeuse_face(radial)?];
        let faceuses = [
            brep.edgeuse_faceuse(edgeuse)?,
            brep.edgeuse_faceuse(radial)?,
        ];
        let mut boundaries = Vec::new();
        for other_edge in brep.vertex_edges(far)? {
            if other_edge == edge {
                continue;
            }
            for use_ in brep.edge_edgeuses(other_edge)? {
                if !faceuses.contains(&brep.edgeuse_faceuse(use_)?) {
                    continue;
                }
                let across = brep
                    .edgeuse_radial(use_)?
                    .ok_or(dangling("edge use without radial"))?;
                if faces.contains(&brep.edgeuse_face(across)?) {
                    continue;
                }
                let face = brep
                    .edgeuse_face(use_)?
                    .ok_or(dangling("edge use without face"))?;
                if !brep.edge_is_seam_of_face_surface(k, other_edge, face)?
                    && !boundaries.contains(&use_)
                {
                    boundaries.push(use_);
                }
            }
        }
        Ok(boundaries)
    }

    /// One reported root of a genuine boundary's displaced support on this SSI
    /// component. Distinct curve parameters do not establish endpoint ownership.
    fn unique_boundary_root<O: ClassObject + SolutionObject>(
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        executive: &mut impl OffsetExecutive<O>,
        curve: O,
        across: O,
        tolerance: f64,
    ) -> Result<BoundaryRoot> {
        let radial = brep
            .edgeuse_radial(across)?
            .ok_or(dangling("boundary without radial"))?;
        let offset = executive
            .offset_edgeuse(radial)
            .ok_or(dangling("boundary without offset"))?;
        let face = brep
            .edgeuse_face(offset)?
            .ok_or(dangling("offset boundary without face"))?;
        let surface = brep
            .face_surface(face)?
            .ok_or(dangling("face without surface"))?;
        let (extended, domain) = executive.get_create_extended_surface(k, brep, surface)?;
        let interval = curve_of(brep, curve)?.natural_interval();
        let roots = surface_global_curve_intersect(
            k, brep, extended, &domain, curve, &interval, tolerance,
        )?;
        let mut root: Option<(f64, Point3, Point2)> = None;
        for solution in roots.as_slice() {
            if solution.solution_type == SolutionType::RangeOfValues {
                return Ok(BoundaryRoot::Unresolved);
            }
            let parameter = solution.start.param(0)?;
            let point = curve_of(brep, curve)?.evaluate_point(k, parameter)?;
            if !(parameter.is_finite()
                && point.x.is_finite()
                && point.y.is_finite()
                && point.z.is_finite())
            {
                return Ok(BoundaryRoot::Unresolved);
            }
            if let Some((previous_parameter, _, _)) = root {
                // The solver preserves reports across chart seams and closed
                // curve ends. Only identical parameters identify the same point
                // of this curve; spatial proximity cannot merge material roots.
                if parameter != previous_parameter {
                    return Ok(BoundaryRoot::Unresolved);
                }
            } else {
                let uv = Point2::new(solution.start.param(1)?, solution.start.param(2)?);
                root = Some((parameter, point, uv));
            }
        }
        Ok(match root {
            Some((t, point, uv)) => BoundaryRoot::One(t, point, uv),
            None => BoundaryRoot::NoCut,
        })
    }

    /// Whether an already constructed opposite boundary contains the support
    /// root. This identifies the source branch by incidence, without ranking
    /// unrelated SSI branches by their midpoints. None means it is not mated yet.
    #[allow(clippy::too_many_arguments)]
    fn root_on_mated_boundary<O: ClassObject + SolutionObject>(
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        executive: &impl OffsetExecutive<O>,
        across: O,
        point: Point3,
        tolerance: f64,
    ) -> Result<Option<bool>> {
        let edge = brep
            .edgeuse_edge(across)?
            .ok_or(dangling("boundary without edge"))?;
        let mut mated = false;
        for use_ in brep.edge_edgeuses(edge)? {
            mated |= executive.has_mated_edgeuse(use_);
        }
        if !mated {
            return Ok(None);
        }
        let offset = executive
            .offset_edgeuse(across)
            .ok_or(dangling("boundary without offset"))?;
        let offset_edge = brep
            .edgeuse_edge(offset)?
            .ok_or(dangling("offset boundary without edge"))?;
        let handle = brep
            .edge_curve(offset_edge)?
            .ok_or(dangling("boundary without curve"))?;
        let interval = brep.edge_interval(offset_edge)?;
        let curve = curve_of(brep, handle)?;
        let solutions: SolutionArray<O> = curve.global_point_solve(
            k,
            &interval,
            SolverOperation::Minimize,
            point,
            tolerance,
            SolutionRequested::Single,
            &GlobalPointSolveOptions::default(),
        )?;
        let Some(solution) = solutions.get(0) else {
            return Ok(Some(false));
        };
        let gap = curve
            .evaluate_point(k, solution.start.param(0)?)?
            .distance(point);
        Ok(Some(gap.is_finite() && gap <= tolerance))
    }

    /// Whether the candidate interval of `curve`, from its root `far_parameter` on the far
    /// support beyond `across` toward its rim root `parameter`, leaves that support to the
    /// side the original edge of `edgeuse` leaves it at its far vertex (the end that is not
    /// `vertex`). The edge's side is the sign of its tangent there, directed toward `vertex`,
    /// along the third face's surface normal; the candidate's is the sign of its tangent at
    /// the far root, directed toward the rim root, along the normal of that face's
    /// displaced, extended surface at the root's `far_uv`. A regular offset keeps its
    /// base's parameters and orientation, so the two normals have one sense. `None` where
    /// either tangent lies in its support's tangent plane or a value is not finite: no side
    /// is established.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for missing topology; those of the evaluations.
    #[allow(clippy::too_many_arguments)]
    fn leaves_far_support_as_edge<O: ClassObject + SolutionObject>(
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        executive: &mut impl OffsetExecutive<O>,
        edgeuse: O,
        vertex: O,
        curve: O,
        across: O,
        far_parameter: f64,
        parameter: f64,
        far_uv: Point2,
    ) -> Result<Option<bool>> {
        let base = brep
            .edgeuse_edge(edgeuse)?
            .ok_or(dangling("edge use without edge"))?;
        let base_curve = brep
            .edge_curve(base)?
            .ok_or(dangling("edge without curve"))?;
        let interval = brep.edge_interval(base)?;
        // A use along its edge starts at the interval's lower end.
        let at_start = brep.edgeuse_vertex(edgeuse)? == Some(vertex);
        let along = brep.edgeuse_orientation(edgeuse)? != OrientType::Opposite;
        let (far_at, toward_vertex) = if at_start == along {
            (interval.max(), -1.0)
        } else {
            (interval.min(), 1.0)
        };
        let edge_tangent = curve_of(brep, base_curve)?
            .evaluate(k, far_at, 1, true, true)?
            .get(1)
            .copied()
            .ok_or(dangling("tangent missing"))?
            * toward_vertex;
        let radial = brep
            .edgeuse_radial(across)?
            .ok_or(dangling("boundary without radial"))?;
        let third = brep
            .edgeuse_face(radial)?
            .ok_or(dangling("boundary without face"))?;
        let third_surface = brep
            .face_surface(third)?
            .ok_or(dangling("face without surface"))?;
        let far = brep
            .edge_other_vertex(base, vertex)?
            .ok_or(dangling("edge without other vertex"))?;
        let radial_end = if brep.edgeuse_vertex(radial)? == Some(far) {
            0.0
        } else {
            1.0
        };
        let uv = edgeuse_normalized_evaluate(k, brep, radial, radial_end, true, false, true)?;
        let before = edge_tangent.dot(surface_of(brep, third_surface)?.evaluate_normal(
            k,
            Point2::new(uv.x, uv.y),
            Side::Left,
            Side::Left,
        )?);
        let offset = executive
            .offset_edgeuse(radial)
            .ok_or(dangling("boundary without offset"))?;
        let face = brep
            .edgeuse_face(offset)?
            .ok_or(dangling("offset boundary without face"))?;
        let surface = brep
            .face_surface(face)?
            .ok_or(dangling("face without surface"))?;
        let (extended, _) = executive.get_create_extended_surface(k, brep, surface)?;
        let normal =
            surface_of(brep, extended)?.evaluate_normal(k, far_uv, Side::Left, Side::Left)?;
        let run = if parameter > far_parameter { 1.0 } else { -1.0 };
        let after = curve_of(brep, curve)?
            .evaluate(k, far_parameter, 1, true, true)?
            .get(1)
            .copied()
            .ok_or(dangling("tangent missing"))?
            .dot(normal)
            * run;
        if !(before.is_finite() && after.is_finite()) || before == 0.0 || after == 0.0 {
            return Ok(None);
        }
        Ok(Some((before > 0.0) == (after > 0.0)))
    }

    /// A disappearing chart seam is not a material constraint. The genuine
    /// supports at the opposite end and beyond the seam bound the active SSI
    /// interval. Chart crossings outside this interval do not retain the seam.
    /// The active interval must leave each far support as the edge does; a rim root
    /// behind the far end keeps the ordinary trim. Without genuine supports on both
    /// sides, a chart miss does not establish seam removal: ordinary endpoint trimming
    /// still supplies the constraint.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn periodic_boundary_end<O: ClassObject + SolutionObject>(
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        executive: &mut impl OffsetExecutive<O>,
        edgeuse: O,
        vertex: O,
        curve: O,
        neighbours: &[O],
        uvs: &[Option<O>],
    ) -> Result<PeriodicEnd<O>> {
        let base = brep
            .edgeuse_edge(edgeuse)?
            .ok_or(dangling("edge use without edge"))?;
        // A closed edge's sole endpoint is its chart cut, not a material end to
        // replace by a third support. Its periodic trimming is unchanged.
        if brep.edge_is_closed(base)? {
            return Ok(PeriodicEnd::Ordinary);
        }
        let tolerance = brep.brep_tolerance(base)?;
        let mut events = Vec::new();
        let mut root: Option<(f64, Point3)> = None;
        let mut ordinary_end = false;
        'seams: for (index, &seam) in neighbours.iter().enumerate() {
            let e = brep
                .edgeuse_edge(seam)?
                .ok_or(dangling("edge use without edge"))?;
            let face = brep
                .edgeuse_face(seam)?
                .ok_or(dangling("edge use without face"))?;
            let radial = brep
                .edgeuse_radial(seam)?
                .ok_or(dangling("edge use without radial"))?;
            if brep.edgeuse_face(radial)? != Some(face)
                || !brep.edge_is_seam_of_face_surface(k, e, face)?
            {
                continue;
            }
            let boundaries = Self::boundaries_beyond_seam(k, brep, seam, vertex)?;
            let opposite = Self::opposite_end_boundaries(k, brep, edgeuse, vertex)?;
            if boundaries.is_empty() || opposite.is_empty() {
                // This seam-removal construction requires a support-bounded
                // interval. Its absence leaves the ordinary trim applicable,
                // including when the candidate misses this chart line. As for
                // a retained seam, it cannot also emit a replacement root.
                ordinary_end = true;
                continue;
            }
            let uv = uvs
                .get(index)
                .copied()
                .flatten()
                .ok_or(dangling("SSI without UV curve"))?;
            let mut seam_root: Option<(f64, Point3)> = None;
            for &across in &boundaries {
                let candidate =
                    match Self::unique_boundary_root(k, brep, executive, curve, across, tolerance)?
                    {
                        BoundaryRoot::One(t, p, _) => (t, p),
                        BoundaryRoot::NoCut => {
                            // This component never meets the rim support. It cannot
                            // undergo the seam/rim incidence change considered here;
                            // its chart crossing remains the trimming constraint.
                            let interval = curve_of(brep, curve)?.natural_interval();
                            if Self::intersects_periodic_chart(
                                k, brep, executive, seam, uv, &interval,
                            )? {
                                ordinary_end = true;
                                continue 'seams;
                            }
                            return Ok(PeriodicEnd::Unresolved);
                        }
                        BoundaryRoot::Unresolved => return Ok(PeriodicEnd::Unresolved),
                    };
                if let Some((previous_parameter, _)) = seam_root {
                    if candidate.0 != previous_parameter {
                        return Ok(PeriodicEnd::Unresolved);
                    }
                } else {
                    seam_root = Some(candidate);
                }
            }
            let Some((parameter, point)) = seam_root else {
                return Ok(PeriodicEnd::Unresolved);
            };
            let mut far_root: Option<(f64, Point3)> = None;
            let mut far_boundary_roots = Vec::new();
            for across in opposite {
                let BoundaryRoot::One(t, p, far_uv) =
                    Self::unique_boundary_root(k, brep, executive, curve, across, tolerance)?
                else {
                    return Ok(PeriodicEnd::Unresolved);
                };
                let candidate = (t, p);
                far_boundary_roots.push((across, p, far_uv));
                if let Some((previous_parameter, _)) = far_root {
                    if candidate.0 != previous_parameter {
                        return Ok(PeriodicEnd::Unresolved);
                    }
                } else {
                    far_root = Some(candidate);
                }
            }
            let Some((far_parameter, _)) = far_root else {
                return Ok(PeriodicEnd::Unresolved);
            };
            // The interval from the far root to the rim root is the edge's image only if it
            // leaves every far support to the side the edge does. A rim root behind the far
            // end bounds no part of this edge: the seam is not removed by it, and the
            // ordinary trim supplies the constraint (#19714).
            let mut leaves_as_edge = true;
            for &(across, _, far_uv) in &far_boundary_roots {
                match Self::leaves_far_support_as_edge(
                    k,
                    brep,
                    executive,
                    edgeuse,
                    vertex,
                    curve,
                    across,
                    far_parameter,
                    parameter,
                    far_uv,
                )? {
                    Some(true) => {}
                    Some(false) => leaves_as_edge = false,
                    None => return Ok(PeriodicEnd::Unresolved),
                }
            }
            if !leaves_as_edge {
                ordinary_end = true;
                continue;
            }
            let active = Extent1d::from_min_max(parameter, far_parameter);
            if Self::intersects_periodic_chart(k, brep, executive, seam, uv, &active)? {
                ordinary_end = true;
                continue;
            }
            let mut owned_branch = false;
            for (across, point, _) in far_boundary_roots {
                match Self::root_on_mated_boundary(k, brep, executive, across, point, tolerance)? {
                    Some(true) => owned_branch = true,
                    Some(false) => return Ok(PeriodicEnd::Unresolved),
                    None => {}
                }
            }
            if !owned_branch {
                return Ok(PeriodicEnd::Unresolved);
            }
            if let Some((previous_parameter, _)) = root {
                if parameter != previous_parameter {
                    return Ok(PeriodicEnd::Unresolved);
                }
            } else {
                root = Some((parameter, point));
            }
            for across in boundaries {
                events.push(PeriodicBoundaryEnd {
                    edgeuse,
                    vertex,
                    seam,
                    across,
                    point,
                });
            }
        }
        if ordinary_end && root.is_some() {
            return Ok(PeriodicEnd::Unresolved);
        }
        match root {
            Some((parameter, _)) => Ok(PeriodicEnd::Root(parameter, events)),
            None => Ok(PeriodicEnd::Ordinary),
        }
    }

    /// [`Self::trim_extended_edge`] with the order of its trim points: the trim interval,
    /// and whether the edge's offset is inverted (see [`TrimmedEdge`]).
    ///
    /// # Errors
    ///
    /// As [`Self::trim_extended_edge`]; those of the two tangent evaluations
    /// ([`Self::trim_points_inverted`]).
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    fn trim_extended_edge_ends<O: ClassObject + SolutionObject>(
        &self,
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        executive: &mut impl OffsetExecutive<O>,
        base_edgeuse: O,
        other_base_edgeuse: Option<O>,
        curve_3d: O,
        uv_curve_base: Option<O>,
        uv_curve_other_base: Option<O>,
        mark: MarkType,
    ) -> Result<Option<TrimmedEdge<O>>> {
        // locals
        let mut trim_ivl = Extent1d::new();
        let tan_tol_deg = self.tangency_tol_degrees();
        let base_edge = brep
            .edgeuse_edge(base_edgeuse)?
            .ok_or(dangling("edge use without edge"))?;

        // (not upstream) the trim point of the base edge use's start vertex, then its end's
        let mut ends = [0.0_f64; 2];
        // (not upstream) whether each trim point is a cut by a neighbour's offset surface
        let mut cuts = [false; 2];
        let mut periodic_ends = Vec::new();
        // (not upstream) where the base edge's middle drops onto the curve, once it is asked
        // for: one parameter for both ends
        let mut edge_middle: Option<Option<f64>> = None;
        // for two iterations - find a one 3DCurve trim point
        for i in 0..2 {
            let mut solutions: SolutionArray<O> = SolutionArray::with_capacity(16);

            // Let's first try to take care of start vertex of pBaseEdgeuse
            let mut off_adj_eus: Vec<O> = Vec::new(); // list of
            let mut tan_eus: Vec<O> = Vec::new(); // list of NextEdgeuses between tangent faces
            let mut tan_uvs: Vec<Option<O>> = Vec::new(); // list of BaseCurve UVTrimCurves when NextEdgeuse is a tangent sector

            // the base vertex and the neighbours of each iteration
            let base_vertex: O;
            if i == 0 {
                // start at BaseEdgeuse->StartVertex
                base_vertex = brep
                    .edgeuse_vertex(base_edgeuse)?
                    .ok_or(dangling("edge use without vertex"))?;
                let bcw = brep
                    .edgeuse_cw(base_edgeuse)?
                    .ok_or(dangling("edge use without CW"))?;

                // See if base edgeuse is tangent and save it along with the curve on the
                // correct surface to be used during intersection.
                if bcw != base_edgeuse {
                    self.collect_neighbour(
                        k,
                        brep,
                        executive,
                        base_vertex,
                        base_vertex,
                        base_edge,
                        bcw,
                        uv_curve_base,
                        tan_tol_deg,
                        &mut tan_eus,
                        &mut tan_uvs,
                        &mut off_adj_eus,
                    )?;
                }

                // See if otherBase edgeuse is tangent and save it along with the curve on the
                // correct surface to be used during intersection.
                if let Some(other_base) = other_base_edgeuse {
                    let obccw = brep
                        .edgeuse_ccw(other_base)?
                        .ok_or(dangling("edge use without CCW"))?;
                    if obccw != other_base {
                        let other_base_vertex = brep
                            .edge_other_vertex(base_edge, base_vertex)?
                            .ok_or(dangling("edge without other vertex"))?;
                        let other_base_edge = brep
                            .edgeuse_edge(other_base)?
                            .ok_or(dangling("edge use without edge"))?;
                        self.collect_neighbour(
                            k,
                            brep,
                            executive,
                            base_vertex,
                            other_base_vertex,
                            other_base_edge,
                            obccw,
                            uv_curve_other_base,
                            tan_tol_deg,
                            &mut tan_eus,
                            &mut tan_uvs,
                            &mut off_adj_eus,
                        )?;
                    }
                }
            } else {
                // 2nd iteration: start at BaseEdgeuse->EndVertex
                let mate = brep
                    .edgeuse_mate(base_edgeuse)?
                    .ok_or(dangling("edge use without mate"))?;
                base_vertex = brep
                    .edgeuse_vertex(mate)?
                    .ok_or(dangling("edge use without vertex"))?;
                let bccw = brep
                    .edgeuse_ccw(base_edgeuse)?
                    .ok_or(dangling("edge use without CCW"))?;

                // See if base edgeuse is tangent and save it along with the curve on the
                // correct surface to be used during intersection.
                if bccw != base_edgeuse {
                    self.collect_neighbour(
                        k,
                        brep,
                        executive,
                        base_vertex,
                        base_vertex,
                        base_edge,
                        bccw,
                        uv_curve_base,
                        tan_tol_deg,
                        &mut tan_eus,
                        &mut tan_uvs,
                        &mut off_adj_eus,
                    )?;
                }

                // See if otherBase edgeuse is tangent and save it along with the curve on the
                // correct surface to be used during intersection.
                if let Some(other_base) = other_base_edgeuse {
                    let obcw = brep
                        .edgeuse_cw(other_base)?
                        .ok_or(dangling("edge use without CW"))?;
                    if obcw != other_base {
                        let other_base_vertex = brep
                            .edge_other_vertex(base_edge, base_vertex)?
                            .ok_or(dangling("edge without other vertex"))?;
                        let other_base_edge = brep
                            .edgeuse_edge(other_base)?
                            .ok_or(dangling("edge use without edge"))?;
                        self.collect_neighbour(
                            k,
                            brep,
                            executive,
                            base_vertex,
                            other_base_vertex,
                            other_base_edge,
                            obcw,
                            uv_curve_other_base,
                            tan_tol_deg,
                            &mut tan_eus,
                            &mut tan_uvs,
                            &mut off_adj_eus,
                        )?;
                    }
                }
            } // end 2nd iteration branch

            let genuine_pair = match other_base_edgeuse {
                Some(other) => brep.edgeuse_face(base_edgeuse)? != brep.edgeuse_face(other)?,
                None => false,
            };
            if genuine_pair {
                match Self::periodic_boundary_end(
                    k,
                    brep,
                    executive,
                    base_edgeuse,
                    base_vertex,
                    curve_3d,
                    &tan_eus,
                    &tan_uvs,
                )? {
                    PeriodicEnd::Ordinary => {}
                    PeriodicEnd::Unresolved => return Ok(None),
                    PeriodicEnd::Root(parameter, events) => {
                        trim_ivl.add_value(parameter);
                        if let Some(end) = ends.get_mut(i) {
                            *end = parameter;
                        }
                        if let Some(cut) = cuts.get_mut(i) {
                            *cut = true;
                        }
                        periodic_ends.extend(events);
                        continue;
                    }
                }
            }

            // (WR-736, #2611, #2769) a lamina edge's offset (the lifted curve, on its own face's offset) ends, at a vertex
            // whose next edge is lamina or a tangent sector, at the vertex's own image
            // `V + d n(V)`, where it passes nearest that image (a well-conditioned solve: the
            // curve runs through the image to the lift's tolerance). Neither the neighbour's
            // offset vertex, snapped to within ten tolerances, nor the crossing of the two
            // edges' UV trim curves is taken: the crossing's parameter is the UV curve's,
            // which the lifted curve does not keep, and between tangent edges the crossing
            // is ill-conditioned. A curve that passes the image more than once (an arc
            // continued round its circle) has it on every turn: an open edge takes it on
            // the turn its middle drops onto, the same for both ends
            // ([`passages_on_one_turn`]). A closed edge's one vertex is both its ends, and
            // its passages all stay candidates for the search below.
            // Not upstream: the same end candidate, for the same reason, where the base edge
            // runs between two planar faces on one plane (a face capped on another's plane)
            // and each tangent neighbour runs on between the same two faces. Both faces
            // offset onto one plane, which carries the vertex's image, and the edges' offsets
            // meet there; a neighbouring face's offset that cuts the curve still gives its
            // cuts below, and the nearest candidate is taken as ever. The crossing of the
            // extended UV curves has no point to give: the curves continue one another, and
            // where they lie on one circle (a capped footprint's circle, split where it
            // touches its face's rim) their extensions, each wound round the circle many
            // times, coincide and cross within the tolerance at thousands of points, each
            // then weighed for moving to a curve end.
            let vertex_image = !tan_eus.is_empty()
                && (brep.edge_is_lamina(base_edge)?
                    || Self::runs_on_between_planes(
                        brep,
                        base_edge,
                        base_edgeuse,
                        other_base_edgeuse,
                        &tan_eus,
                    )?);
            if vertex_image {
                let tol_3d = brep.brep_tolerance(base_edge)?;
                let image = self.offset_vertex_image(k, brep, base_edgeuse, base_vertex)?;
                let on_curve = Self::point_solve(
                    k,
                    brep,
                    curve_3d,
                    SolverOperation::Minimize,
                    image,
                    tol_3d,
                    None,
                    SolutionRequested::All,
                )?;
                let same_tol = 10.0 * self.this_approx_tol_3d;
                let middle = if on_curve.len() > 1 && !brep.edge_is_closed(base_edge)? {
                    if let Some(middle) = edge_middle {
                        middle
                    } else {
                        let middle =
                            Self::edge_middle_passage(k, brep, base_edgeuse, curve_3d, same_tol)?;
                        edge_middle = Some(middle);
                        middle
                    }
                } else {
                    None
                };
                match middle {
                    Some(middle) => {
                        let passages = on_curve
                            .as_slice()
                            .iter()
                            .map(|passage| passage.start.param(0))
                            .collect::<Result<Vec<f64>>>()?;
                        let kept = passages_on_one_turn(
                            k,
                            curve_of(brep, curve_3d)?,
                            &passages,
                            image,
                            brep.vertex_point(base_vertex)?,
                            middle,
                            same_tol,
                        )?;
                        for index in kept {
                            if let Some(passage) = on_curve.get(index) {
                                solutions.add(passage);
                            }
                        }
                    }
                    None => {
                        solutions.append(&on_curve);
                    }
                }
            }
            // (not upstream) the points of the mated neighbours whose new surfaces cross
            let mut crossing_ends: Option<SolutionArray<O>> = None;
            // Tangent sector handling - extend parameter space and intersect it for one or two curves.
            for (itry, &tan_eu) in tan_eus.iter().enumerate() {
                if vertex_image {
                    break;
                }
                let tan_uv = tan_uvs.get(itry).copied().flatten();
                let mut uv_curve = brep.edgeuse_uv_trim_curve(tan_eu)?;

                // If this has already been mated then we should just grab the point of the vertex
                let mut found_3d_vertex = false;
                // (the mate is dereferenced only when the edge use is marked)
                if brep.edgeuse_is_marked(k, tan_eu, mark)? && {
                    let tan_mate = brep
                        .edgeuse_mate(tan_eu)?
                        .ok_or(dangling("edge use without mate"))?;
                    brep.edgeuse_is_marked(k, tan_mate, mark)?
                } {
                    let tan_edge = brep
                        .edgeuse_edge(tan_eu)?
                        .ok_or(dangling("edge use without edge"))?;
                    let crossing = executive.crosses_at(tan_eu);
                    if crossing && crossing_ends.is_none() {
                        crossing_ends = Some(SolutionArray::with_capacity(2));
                    }
                    let all_eus = brep.edge_edgeuses(tan_edge)?;
                    for eu in all_eus {
                        // skip
                        let Some(off_eu) = executive.offset_edgeuse(eu) else {
                            continue;
                        };
                        let mut v = brep
                            .edgeuse_vertex(off_eu)?
                            .ok_or(dangling("edge use without vertex"))?;

                        // get point on 3DCurve closest to pOffEu->StartVertex
                        let tol_3d = self.this_approx_tol_3d * 10.0;
                        let point = brep.vertex_point(v)?;
                        let sols2 = Self::point_solve(
                            k,
                            brep,
                            curve_3d,
                            SolverOperation::Minimize,
                            point,
                            tol_3d,
                            Some(tol_3d),
                            SolutionRequested::All,
                        )?;

                        // save solutions
                        if !sols2.is_empty() {
                            solutions.append(&sols2);
                            found_3d_vertex = true;
                            if let Some(ends) = crossing_ends.as_mut().filter(|_| crossing) {
                                ends.append(&sols2);
                            }
                        } // end found a solution check

                        // not get point on 3DCurve closest to pOffEd0->EndVertex
                        let off_edge = brep
                            .edgeuse_edge(off_eu)?
                            .ok_or(dangling("edge use without edge"))?;
                        let Some(other_v) = brep.edge_other_vertex(off_edge, v)? else {
                            continue; // Closed Curve Case
                        };
                        v = other_v;
                        let point = brep.vertex_point(v)?;
                        let sols2 = Self::point_solve(
                            k,
                            brep,
                            curve_3d,
                            SolverOperation::Minimize,
                            point,
                            tol_3d,
                            Some(tol_3d),
                            SolutionRequested::All,
                        )?;

                        // save solutions
                        if !sols2.is_empty() {
                            solutions.append(&sols2);
                            found_3d_vertex = true;
                            if let Some(ends) = crossing_ends.as_mut().filter(|_| crossing) {
                                ends.append(&sols2);
                            }
                        }
                    }
                } // end pTanEu and its mate are marked checks

                // If we didn't find a 3D vertex from the offset close enough then we need to do
                // UV curve extension and intersections to solve for this tangency case.
                if let (false, Some(tan_uv)) = (found_3d_vertex, tan_uv) {
                    // get a temporary copy of the pTanEU->UVTrimCurve
                    let uv_copy: Curve = match uv_curve.take() {
                        None => {
                            let (_max_dist, h) =
                                edgeuse_create_uv_trim_curve(k, brep, tan_eu, false, false)?;
                            curve_of(brep, h)?.clone()
                        }
                        // (`pUVCurve->Copy(crContext, pUVCurve)` on an `SmBSplineCurve*`
                        // binds the non-virtual `SmBSplineCurve*&` overload: a plain
                        // B-spline copy, a line's analytic data is dropped)
                        Some(h) => b_spline_overload_copy(curve_of(brep, h)?)?,
                    };
                    // (SmBSplineCurve *pUVCurve = pTanEU->GetUVTrimCurve(): the NURB data of
                    // a B-spline kind, which CreateExtendedCurve reads)
                    let uv_bs = b_spline_part(&uv_copy)?
                        .ok_or(dangling("UV trim curve is not a B-spline curve"))?;

                    // set pUVCurve extension amounts
                    let curve_extension_factor = 10.0; // Curve Extension Factor
                    let ivl = uv_bs.natural_interval();
                    let extension_parameter = ivl.min() - curve_extension_factor * ivl.length();
                    let extension_parameter2 = ivl.max() + curve_extension_factor * ivl.length();

                    // extend both ends of pUVCurve (a null extension, of a zero-length
                    // interval, is dereferenced upstream)
                    let precise_extension = true; // Otherwise it adds another length of sIvl.
                    let curve_extended = uv_bs
                        .create_extended_curve(
                            extension_parameter,
                            ContinuityType::G1G2,
                            precise_extension,
                        )?
                        .ok_or(dangling("null extended curve"))?;
                    let mut curve_extended2 = curve_extended
                        .create_extended_curve(
                            extension_parameter2,
                            ContinuityType::G1G2,
                            precise_extension,
                        )?
                        .ok_or(dangling("null extended curve"))?;
                    curve_extended2.remove_extra_knots(EFF_ZERO_SQRT / 100.0)?;

                    // intersect pTanUV/Extended pTanEU->UVTrimCurve (the extended curve
                    // goes to the store: the solutions name it as their object, which
                    // the solver's comparisons dereference for its parameter interval)
                    let ext_h = store_curve(brep, Curve::from(curve_extended2))?;
                    let ext_curve = curve_of(brep, ext_h)?;
                    let ext_ivl = ext_curve.natural_interval();
                    let tan_curve = curve_of(brep, tan_uv)?;
                    let sols2: SolutionArray<O> = tan_curve.global_curve_intersect(
                        k,
                        &tan_curve.natural_interval(),
                        ext_curve,
                        &ext_ivl,
                        EFF_ZERO_SQRT,
                        &CurveIntersectOptions {
                            skip_global_coincidence_check: false,
                            handles: [Some(tan_uv), Some(ext_h)],
                            analytic_handles: [None, None],
                        },
                    )?;

                    // save solutions as possible trim Points
                    solutions.append(&sols2);
                }
            } // end iter every sTanEU/sTanUV pair of members

            // Now handle intersecting faces if there are any

            // for every sOffAdjEU - try letting trimPoint = OffAdjEu->ExtendedSurface/3DCurve intersection
            let mut last_face: Option<O> = None;
            // (WR-666) each adjacent face's candidates in `solutions`
            let mut face_solutions: Vec<(usize, usize)> = Vec::new();
            for &off_adj_eu in &off_adj_eus {
                let f = brep
                    .edgeuse_face(off_adj_eu)?
                    .ok_or(dangling("edge use without face"))?;

                // skip LastFace
                if Some(f) == last_face {
                    continue;
                }

                // Extend pF->Surface
                last_face = Some(f);
                let tol_3d = brep.brep_tolerance(f)?;
                let f_surface = brep
                    .face_surface(f)?
                    .ok_or(dangling("face without surface"))?;
                let (ext_surface, ext_domain) =
                    executive.get_create_extended_surface(k, brep, f_surface)?;

                // intersect pF->ExtendedSurface with 3DCurve
                let crv_ivl = curve_of(brep, curve_3d)?.natural_interval();
                let Some(sols2) = ignore_soft(surface_global_curve_intersect(
                    k,
                    brep,
                    ext_surface,
                    &ext_domain,
                    curve_3d,
                    &crv_ivl,
                    tol_3d,
                ))?
                else {
                    continue;
                };

                // Note that the adjacent surface intersection overrides tangent intersections
                // Test the solutions to see if they are not nearly tangent.
                face_solutions.push((solutions.len(), solutions.len()));
                for sol in sols2.as_slice() {
                    // (neither status is checked upstream: a failed evaluation leaves its
                    // output at SmPoint3d's SM_UNDEF_DOUBLE default; passed on here, WR-658)
                    let undef = Vec3::new(UNDEF_DOUBLE, UNDEF_DOUBLE, UNDEF_DOUBLE);
                    let pv = curve_of(brep, curve_3d)?.evaluate(
                        k,
                        sol.start.param(0)?,
                        1,
                        true,
                        true,
                    )?;
                    let uv = Point2::new(sol.start.param(1)?, sol.start.param(2)?);
                    let normal = surface_of(brep, ext_surface)?.evaluate_normal(
                        k,
                        uv,
                        Side::Left,
                        Side::Left,
                    )?;
                    let tangent = pv.get(1).copied().unwrap_or(undef).unitize_or_z();
                    let angle_rad = tangent.angle_between(normal)?;
                    let angle_deg = angle_rad * 180.0 / PI;
                    // Four-face vertex splitting (WR-666, the fork's
                    // tangency-labs/smlib@2e1bb8c): a curve that crosses the surface at a
                    // shallow angle crosses it all the same (a trough's floor facets falling
                    // 1 degree, whose offsets cut the offset edges along the walls 0.6
                    // degrees off them): only a touch, the curve meeting the surface without
                    // passing through it, is no trim point. A curve exact to the tolerance
                    // that touches the surface (the drafted end of a round, cut where the
                    // round meets the top face tangentially) crosses it twice within the
                    // tolerance, so it crosses only where it passes through by more than
                    // the tolerance ([`passes_through`]).
                    // Not upstream (#17514): at an end with no tangent neighbour, a curve
                    // that meets the surface at more than the operation's tangency tolerance
                    // (`tan_tol_deg`, the angle by which `collect_neighbour` found no
                    // neighbour tangent here) is not tangent to it, so it passes through
                    // however shallow the angle, even where it comes back within the
                    // tolerance (a plane cutting a torus 2.09 degrees from its bottom circle:
                    // the offset seam meets the floor's offset 2.96 degrees off it and dips
                    // under it by less than the tolerance, which [`passes_through`] reads as
                    // a touch; the end then fell back to the old vertex's offset and the mate
                    // moved the vertex along the edge). A curve that meets the surface within
                    // that angle is weighed by [`passes_through`] as before: an offset edge
                    // tangent to a neighbour's offset away from the vertex is no cut.
                    let transversal = tan_eus.is_empty() && (angle_deg - 90.0).abs() > tan_tol_deg;
                    let crosses = !(angle_deg < 85.0 || angle_deg > 95.0)
                        && (transversal
                            || passes_through(
                                k,
                                curve_of(brep, curve_3d)?,
                                sol.start.param(0)?,
                                surface_of(brep, ext_surface)?,
                                uv,
                                tol_3d,
                            )?);
                    if angle_deg < 85.0 || angle_deg > 95.0 || crosses {
                        // save solution as possible trimPoint
                        solutions.add(sol);
                        if let Some(range) = face_solutions.last_mut() {
                            range.1 = solutions.len();
                        }
                    }
                }
            } // end iter every sOffAdjEU

            // Not upstream: beside an edge tangent in the input whose new surfaces cross
            // (`OffsetExecutive::crosses_at`), already mated, the end is that edge's new
            // vertex where it lies on the curve. The cuts of the neighbours' extended
            // surfaces are not weighed against it: the plane beside a round cuts the new
            // round's curve on both sides of the old vertex, about as far on each (`√(2 r
            // δ)`), and the cut on the round's continuation gives an arc across the curve.
            // A mated vertex off the curve is refused, not replaced by a nearest cut.
            if let Some(ends) = crossing_ends {
                if ends.is_empty() {
                    return Err(
                        Failure::new(OP_TRIM, OffsetFailure::TangentEndOffCurve).into_error()
                    );
                }
                solutions = ends;
                face_solutions.clear();
            }

            let mut found = false;
            let mut found_dist = BIG_DOUBLE;
            let mut found_param = 0.0;

            // If we have no candidate TrimPoints found by intersecting the 3DCurve with other
            // geometry, get a candidate TrimPoint by dropping the Vertex->Point onto the 3DCurve
            if solutions.is_empty() {
                let tol_3d = brep.brep_tolerance(base_edge)?;
                let point = brep.vertex_point(base_vertex)?;
                solutions = Self::point_solve(
                    k,
                    brep,
                    curve_3d,
                    SolverOperation::Minimize,
                    point,
                    tol_3d,
                    None,
                    SolutionRequested::All,
                )?;
            }

            // search every solution for the one closest to the BaseVertex to use as the trimPoint
            let base_point = brep.vertex_point(base_vertex)?;
            // Not upstream (WR-673, #2174; the fork's
            // tangency-labs/smlib@7ee0876): an open edge's second trim point is where its
            // other vertex goes, never the first trim point again reached round a closed 3D
            // curve. On a closed 3D curve (the whole offset circle lifted for half a sphere
            // cap's rim) the first point's twin across the curve's seam is another parameter
            // but the same point: taken, it gave the half edge the whole circle and the loop
            // a gap. Only that twin is passed over ([`is_first_point_again`]): an open edge
            // that is short now keeps its real second point, however near the first, on
            // either side of the seam.
            let open_second = i == 1 && !brep.edge_is_closed(base_edge)?;
            let same_tol = 10.0 * self.this_approx_tol_3d;
            let ends_apart = open_second && open_edge_ends_apart(brep, base_edge, same_tol)?;
            let first_again = |t: f64| -> Result<bool> {
                if !open_second {
                    return Ok(false);
                }
                is_first_point_again(
                    k,
                    curve_of(brep, curve_3d)?,
                    trim_ivl.min(),
                    t,
                    same_tol,
                    ends_apart,
                )
            };
            // (not upstream, #14062) a closed edge's second trim point is its one vertex
            // again: where that point is the seam of a closed 3D curve (a loft's floor circle
            // offset, its seam at the vertex) it has the curve's two ends as parameters, and
            // the edge runs all the way round from one to the other. The solutions may give
            // only one of them, the first trim point, which `are_same` below excludes: its
            // twin is the second, taken within the tolerance that decides "the same point"
            // for an open edge (`first_again`).
            let closed_second = i == 1 && brep.edge_is_closed(base_edge)?;
            let mut found_at = None;
            let mut found_twin = false;
            for (ix, sol) in solutions.as_slice().iter().enumerate() {
                let t = sol.start.param(0)?;
                let twin = if closed_second {
                    seam_twin(k, curve_of(brep, curve_3d)?, t, same_tol)?
                } else {
                    None
                };
                for (t, is_twin) in std::iter::once((t, false)).chain(twin.map(|t| (t, true))) {
                    let pnt = curve_of(brep, curve_3d)?.evaluate_point(k, t)?;

                    // search for the best solution
                    let dist_moved = distance(pnt, base_point);
                    if dist_moved < found_dist && !are_same(t, trim_ivl.min()) && !first_again(t)? {
                        found_param = t;
                        found_dist = dist_moved;
                        found = true;
                        found_at = Some(ix);
                        found_twin = is_twin;
                    }
                }

                // If solution is a range, e.g. coincident curves, check m_vEnd
                if sol.solution_type == SolutionType::RangeOfValues {
                    let t = sol.end.param(0)?;
                    let pnt = curve_of(brep, curve_3d)?.evaluate_point(k, t)?;
                    let dist_moved = distance(pnt, base_point);

                    if dist_moved < found_dist && !are_same(t, trim_ivl.min()) && !first_again(t)? {
                        found_param = t;
                        found_dist = dist_moved;
                        found = true;
                        found_at = Some(ix);
                        found_twin = false;
                    }
                }
            }
            let mut cut = found_at
                .is_some_and(|ix| face_solutions.iter().any(|&(lo, hi)| lo <= ix && ix < hi));

            // Four-face vertex splitting (WR-666, the fork's
            // tangency-labs/smlib@2e1bb8c): where more than three faces meet at the vertex,
            // the faces on either side of the edge's ends there are different faces, and
            // their offsets cut the edge's offset at two points: four offset planes do not
            // meet in one point. The offset vertex is two vertices joined by a new edge,
            // and where every edge at the vertex is convex (or every one concave) the edge
            // ends at the one of the two cuts its offset reaches first coming from its
            // other end. With a vertex of mixed edges the nearest point stays.
            if found && face_solutions.len() == 2 {
                if let Some(param) = self.first_cut_from_far_end(
                    k,
                    brep,
                    base_edgeuse,
                    base_edge,
                    base_vertex,
                    curve_3d,
                    i,
                    &trim_ivl,
                    &solutions,
                    &face_solutions,
                    tan_tol_deg,
                )? {
                    found_param = param;
                    found_twin = false;
                    cut = true;
                }
            }

            // BadEdge when no trim point was found for this iteration
            if !found {
                return Ok(None);
            }

            // else set Interval
            trim_ivl.add_value(found_param);
            if let Some(end) = ends.get_mut(i) {
                *end = found_param;
            }
            if let Some(c) = cuts.get_mut(i) {
                *c = cut;
            }
            // (#14062) the second trim point is the first one's twin: the edge is the whole
            // curve, end to end, not from a first point the solutions give within their
            // tolerance of the seam, which would leave that bit of the turn out
            if found_twin {
                trim_ivl = curve_of(brep, curve_3d)?.natural_interval();
            }
        } // end iter twice

        // (not upstream) whether the trim points come in the base edge use's order: an
        // open edge only (a closed curve's two arcs are chosen below), whose two ends are
        // both cuts by its neighbours' offsets (they crossed over it); an end dropped from
        // its vertex or taken from a mated neighbour's vertex says nothing of a crossing.
        // Only in an inward shell with faces opened (a rib thinner than two walls), at an
        // edge between two faces, where the faces that turn inside out with it are
        // resolved before the merge (`resolve_crossing_loops`, which drops their inverted
        // regions); elsewhere (a solid offset, a sheet's thickening, a lamina edge) an edge
        // whose vertex moves past its other end is refused as vanishing, as before.
        let inverted = executive.inset()
            && !executive.shell_faces().is_empty()
            && other_base_edgeuse.is_some()
            && cuts == [true, true]
            && !brep.edge_is_closed(base_edge)?
            && Self::trim_points_inverted(k, brep, base_edgeuse, curve_3d, &trim_ivl, ends)?;

        // (WR-463, #403): on a closed curve the two trim points bound two arcs and the
        // interval between them is only one of them. The edge takes the arc that runs where
        // the base edge does; when a trim point lies on the curve's seam the other arc is an
        // interval too, else the edge is not trimmed. A trim point at the seam has two
        // parameters at one point, so which of them the search above found is a matter of
        // rounding: without this the edge can take the whole other arc. A closed base edge
        // runs all the way round, so it has no arc to choose.
        if !brep.edge_is_closed(base_edge)? {
            let nat_ivl = curve_of(brep, curve_3d)?.natural_interval();
            let seam_tol = EFF_ZERO * (1.0 + nat_ivl.max_dimension());
            let tol = self.this_approx_tol_3d;
            if trim_ivl.length() > seam_tol
                && curve_of(brep, curve_3d)?.is_closed(k, &nat_ivl, tol)?
            {
                let base_mid =
                    edgeuse_normalized_evaluate(k, brep, base_edgeuse, 0.5, false, false, true)?;
                let curve = curve_of(brep, curve_3d)?;
                let in_mid = curve.evaluate_point(k, trim_ivl.evaluate(0.5))?;
                let mut out_param = trim_ivl.max() + 0.5 * (nat_ivl.length() - trim_ivl.length());
                if out_param > nat_ivl.max() {
                    out_param -= nat_ivl.length();
                }
                let out_mid = curve.evaluate_point(k, out_param)?;
                let lo = curve.evaluate_point(k, trim_ivl.min())?;
                let hi = curve.evaluate_point(k, trim_ivl.max())?;
                // two trim points at one place with the whole curve between them bound no
                // other arc: its middle is at its ends. Unless the base edge's own ends are
                // at one place too (within the "same point" tolerance of the search above,
                // the follow-up to #17806): then the trim points are its two ends, the
                // other arc, short as it is, is an arc as long as it has a parameter
                // length, and which arc the edge runs is left to its middle below. The
                // test is on the base edge, so it holds at any offset distance.
                let other_len = nat_ivl.length() - trim_ivl.length();
                let other_arc = distance(out_mid, hi) > tol
                    || (other_len > seam_tol
                        && !open_edge_ends_apart(brep, base_edge, 10.0 * tol)?);
                if other_arc && distance(out_mid, base_mid) < distance(in_mid, base_mid) {
                    let seam = curve.evaluate_point(k, nat_ivl.min())?;
                    if distance(lo, seam) <= tol {
                        trim_ivl = Extent1d::from_raw(trim_ivl.max(), nat_ivl.max());
                    } else if distance(hi, seam) <= tol {
                        trim_ivl = Extent1d::from_raw(nat_ivl.min(), trim_ivl.min());
                    } else {
                        // the arc runs across the seam: no interval of this curve gives it
                        return Ok(None);
                    }
                }
            }
        }

        // cull zero length curves
        let scaled_zero = EFF_ZERO * (1.0 + trim_ivl.max_dimension());
        if trim_ivl.length() > scaled_zero {
            Ok(Some(TrimmedEdge {
                interval: trim_ivl,
                inverted,
                periodic_ends,
            }))
        } else {
            Ok(None)
        }
    }

    /// (not upstream, solidcraft#16983) Whether the trim points `ends` (on `curve_3d`, of
    /// the base edge use's start vertex and of its end vertex) run against the base edge
    /// use: going from the first to the second, the curve runs the other way to the edge
    /// use. Its direction along the edge use is the sign of the product of the two
    /// tangents at their middles (the curve's at the middle of `trim_ivl`), the test
    /// [`Self::replace_edge_curves`] orients the curve by; tangents square to each other
    /// give no direction, and the trim points are taken in order.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for a base edge use without edge or curve; those of
    /// the two evaluations.
    fn trim_points_inverted<O: ClassObject + SolutionObject>(
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        base_edgeuse: O,
        curve_3d: O,
        trim_ivl: &Extent1d,
        ends: [f64; 2],
    ) -> Result<bool> {
        let [start, end] = ends;
        if start == end {
            return Ok(false);
        }
        let base_edge = brep
            .edgeuse_edge(base_edgeuse)?
            .ok_or(dangling("edge use without edge"))?;
        let base_curve = brep
            .edge_curve(base_edge)?
            .ok_or(dangling("edge without curve"))?;
        let base_mid = brep.edge_interval(base_edge)?.evaluate(0.5);
        let base = curve_of(brep, base_curve)?.evaluate(k, base_mid, 1, true, true)?;
        let curve = curve_of(brep, curve_3d)?.evaluate(k, trim_ivl.evaluate(0.5), 1, true, true)?;
        let base_tangent = base.get(1).copied().ok_or(dangling("tangent missing"))?;
        let curve_tangent = curve.get(1).copied().ok_or(dangling("tangent missing"))?;
        let mut along = curve_tangent.dot(base_tangent);
        if brep.edgeuse_orientation(base_edgeuse)? == OrientType::Opposite {
            along = -along;
        }
        Ok((end - start) * along < 0.0)
    }

    /// `TrimExtendedEdge`'s choice at a vertex of more than three faces under WR-666
    /// (the fork's tangency-labs/smlib@2e1bb8c; the split is the only path since
    /// #2981): when every edge at `base_vertex` is convex (or every
    /// one concave; a lamina, tangent or shell-less edge counts as neither) and each of
    /// the two adjacent faces (`face_solutions`, ranges of `solutions`) has a cut nearest
    /// the vertex (not the start already chosen, nor, on an open edge's second end, the
    /// first trim point again round a closed curve, [`is_first_point_again`]), and the two
    /// cuts are apart, the one nearer along the
    /// curve to where the base edge's middle drops onto it. `None` keeps the nearest.
    #[allow(clippy::too_many_arguments)]
    fn first_cut_from_far_end<O: ClassObject + SolutionObject>(
        &self,
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        base_edgeuse: O,
        base_edge: O,
        base_vertex: O,
        curve_3d: O,
        i: usize,
        trim_ivl: &Extent1d,
        solutions: &SolutionArray<O>,
        face_solutions: &[(usize, usize)],
        tan_tol_deg: f64,
    ) -> Result<Option<f64>> {
        // an open edge's second trim point is never the first one again reached round a
        // closed 3D curve (WR-673, [`is_first_point_again`], as in `trim_extended_edge_ends`)
        let open_second = i == 1 && !brep.edge_is_closed(base_edge)?;
        let same_tol = 10.0 * self.this_approx_tol_3d;
        let ends_apart = open_second && open_edge_ends_apart(brep, base_edge, same_tol)?;

        let mut convex = true;
        let mut concave = true;
        let v_edges = brep.vertex_edges(base_vertex)?;
        let base_fu = brep
            .edgeuse_faceuse(base_edgeuse)?
            .ok_or(dangling("edge use without face use"))?;
        let shell = brep.faceuse_shell(base_fu)?;
        for &e in &v_edges {
            if !(convex || concave) {
                break;
            }
            let mut side: Option<O> = None;
            for eu in brep.edge_edgeuses(e)? {
                if side.is_some() {
                    break;
                }
                if let Some(fu) = brep.edgeuse_faceuse(eu)? {
                    if brep.faceuse_shell(fu)? == shell {
                        side = Some(eu);
                    }
                }
            }
            let Some(side) = side else {
                convex = false;
                concave = false;
                continue;
            };
            if brep.edgeuse_radial(side)? == Some(side)
                || brep.edgeuse_is_tangent_sector(k, side, tan_tol_deg)?
            {
                convex = false;
                concave = false;
                continue;
            }
            if brep.edgeuse_is_convex_radial_sector(k, side, 10)? {
                concave = false;
            } else {
                convex = false;
            }
        }

        // Each face's cut nearest the vertex, and which comes first from the edge's other
        // end: the cut nearer, along the curve, to the base edge's middle.
        let mut face_param = [0.0, 0.0];
        let mut face_found = [false, false];
        let base_point = brep.vertex_point(base_vertex)?;
        for ff in 0..2 {
            if !(convex || concave) {
                break;
            }
            let &(lo, hi) = face_solutions
                .get(ff)
                .ok_or(dangling("adjacent face candidates"))?;
            let mut best = BIG_DOUBLE;
            for sol in solutions.as_slice().get(lo..hi).unwrap_or(&[]) {
                let t = sol.start.param(0)?;
                let pnt = curve_of(brep, curve_3d)?.evaluate_point(k, t)?;
                let dist = distance(pnt, base_point);
                if dist < best
                    && !are_same(t, trim_ivl.min())
                    && !(open_second
                        && is_first_point_again(
                            k,
                            curve_of(brep, curve_3d)?,
                            trim_ivl.min(),
                            t,
                            same_tol,
                            ends_apart,
                        )?)
                {
                    best = dist;
                    if let (Some(p), Some(f)) = (face_param.get_mut(ff), face_found.get_mut(ff)) {
                        *p = t;
                        *f = true;
                    }
                }
            }
        }
        let [p0, p1] = face_param;
        let [found0, found1] = face_found;
        if !(found0 && found1) {
            return Ok(None);
        }
        let curve = curve_of(brep, curve_3d)?;
        let cut0 = curve.evaluate_point(k, p0)?;
        let cut1 = curve.evaluate_point(k, p1)?;
        if distance(cut0, cut1) <= same_tol {
            return Ok(None);
        }
        let mid = edgeuse_normalized_evaluate(k, brep, base_edgeuse, 0.5, false, false, true)?; // FALSE = 3d Eval
        let tol = brep.brep_tolerance(base_edge)?;
        let m_solutions = Self::point_solve(
            k,
            brep,
            curve_3d,
            SolverOperation::Minimize,
            mid,
            tol,
            None,
            SolutionRequested::Single,
        )?;
        let Some(first_sol) = m_solutions.as_slice().first() else {
            return Ok(None);
        };
        let d_mid = first_sol.start.param(0)?;
        Ok(Some(if (p0 - d_mid).abs() <= (p1 - d_mid).abs() {
            p0
        } else {
            p1
        }))
    }

    /// Not upstream: where the middle of `base_edgeuse`'s edge drops onto `curve_3d`, for a
    /// curve that may pass that point on more than one turn ([`middle_passage`]): every
    /// solution of the edge's point half way along it is asked for.
    ///
    /// # Errors
    ///
    /// Those of the evaluations and the point solve; [`SolidError::InvalidInput`] for an
    /// edge use without an edge.
    fn edge_middle_passage<O: ClassObject + SolutionObject>(
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        base_edgeuse: O,
        curve_3d: O,
        same_tol: f64,
    ) -> Result<Option<f64>> {
        let base_edge = brep
            .edgeuse_edge(base_edgeuse)?
            .ok_or(dangling("edge use without edge"))?;
        let mid = edgeuse_normalized_evaluate(k, brep, base_edgeuse, 0.5, false, false, true)?; // FALSE = 3d Eval
        let tol = brep.brep_tolerance(base_edge)?;
        let drops = Self::point_solve(
            k,
            brep,
            curve_3d,
            SolverOperation::Minimize,
            mid,
            tol,
            None,
            SolutionRequested::All,
        )?
        .as_slice()
        .iter()
        .map(|drop| drop.start.param(0))
        .collect::<Result<Vec<f64>>>()?;
        middle_passage(k, curve_of(brep, curve_3d)?, &drops, mid, same_tol)
    }

    /// Not upstream: whether `edge` (its edge use `edgeuse`, and `other` on its other face)
    /// runs between two faces that both lie on planes, and each of the edge uses `next`
    /// (its tangent neighbours at a vertex) lies on an edge between the same two faces.
    /// `false` without `other` (a lamina edge) or without neighbours.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for dangling topology.
    fn runs_on_between_planes<O: ClassObject + SolutionObject>(
        brep: &impl OffsetBrep<O>,
        edge: O,
        edgeuse: O,
        other: Option<O>,
        next: &[O],
    ) -> Result<bool> {
        let Some(other) = other else {
            return Ok(false);
        };
        let faces = brep.edge_faces(edge)?;
        if next.is_empty() || faces.len() != 2 {
            return Ok(false);
        }
        for &eu in next {
            let next_edge = brep
                .edgeuse_edge(eu)?
                .ok_or(dangling("edge use without edge"))?;
            let next_faces = brep.edge_faces(next_edge)?;
            if next_faces.len() != 2 || !next_faces.iter().all(|f| faces.contains(f)) {
                return Ok(false);
            }
        }
        for eu in [edgeuse, other] {
            let face = brep
                .edgeuse_face(eu)?
                .ok_or(dangling("edge use without face"))?;
            let sur = brep
                .face_surface(face)?
                .ok_or(dangling("face without surface"))?;
            if !matches!(surface_of(brep, sur)?.kind(), SurfaceKind::Plane(_)) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Not upstream (WR-736, #2611, #2769): the
    /// image of `vertex` on the offset of `edgeuse`'s face, `V + sign d n(V)`: the unit
    /// normal of the face's surface at the vertex, `sign` that of the face use's
    /// orientation, as [`Self::faceuse_offset`] offsets the surface. The vertex lies on
    /// the surface, so its parameters are a well-conditioned point solve.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for dangling topology or no solution; those of the
    /// point solve and the normal evaluation.
    fn offset_vertex_image<O: ClassObject + SolutionObject>(
        &self,
        k: &Kernel,
        brep: &impl OffsetBrep<O>,
        edgeuse: O,
        vertex: O,
    ) -> Result<Point3> {
        let faceuse = brep
            .edgeuse_faceuse(edgeuse)?
            .ok_or(dangling("edge use without face use"))?;
        let sign = if brep.faceuse_orientation(faceuse)? == OrientType::Opposite {
            -1.0
        } else {
            1.0
        };
        let face = brep
            .edgeuse_face(edgeuse)?
            .ok_or(dangling("edge use without face"))?;
        let sur = brep
            .face_surface(face)?
            .ok_or(dangling("face without surface"))?;
        let mut surface = surface_of(brep, sur)?.clone_for_evaluation()?;
        surface.set_owner(None);
        let point = brep.vertex_point(vertex)?;
        let domain = surface.natural_uv_domain_with_nurb(k)?;
        // (a copy free of its face: the vertex, on the face's boundary, needs no test
        // against the face's trims)
        let sols: SolutionArray<O> = surface.global_point_solve(
            k,
            None,
            &domain,
            SolverOperation::Minimize,
            point,
            brep.brep_tolerance(vertex)?,
            None,
            SolutionRequested::Single,
        )?;
        let sol = sols
            .get(0)
            .ok_or(dangling("vertex off its face's surface"))?;
        let uv = Point2::new(sol.start.param(0)?, sol.start.param(1)?);
        let normal = surface.evaluate_normal(k, uv, Side::Left, Side::Left)?;
        Ok(point + normal * (sign * self.offset_distance))
    }

    /// The neighbour classification of `TrimExtendedEdge` (repeated for each end of
    /// each base edge use): a lamina or tangent neighbour goes to the tangent lists
    /// with the base UV curve; otherwise, when the base edge and the neighbour's edge
    /// are not tangent at the vertex, the offset of the neighbour's radial edge use
    /// goes to the adjacent list. (Not upstream, WR-346: a neighbour whose radial lies
    /// on a collapsed face gives the offset of the edge use across that face, seen from
    /// `base_vertex`, instead.)
    #[allow(clippy::too_many_arguments)]
    fn collect_neighbour<O: ClassObject + SolutionObject>(
        &self,
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        executive: &mut impl OffsetExecutive<O>,
        base_vertex: O,
        vertex: O,
        base_edge: O,
        neighbour: O,
        base_uv: Option<O>,
        tan_tol_deg: f64,
        tan_eus: &mut Vec<O>,
        tan_uvs: &mut Vec<Option<O>>,
        off_adj_eus: &mut Vec<O>,
    ) -> Result<()> {
        // Not upstream (WR-346): across a collapsed face, trim to the extended surface
        // of the face beyond it.
        if executive.has_collapsed_faces() {
            let radial = brep.edgeuse_radial(neighbour)?;
            if let Some(across) =
                self.edgeuse_across_collapsed_face(k, brep, &*executive, radial, Some(base_vertex))?
            {
                if let Some(off_across) = executive.offset_edgeuse(across) {
                    off_adj_eus.push(off_across);
                }
                return Ok(());
            }
        }
        let n_edge = brep
            .edgeuse_edge(neighbour)?
            .ok_or(dangling("edge use without edge"))?;
        // When next edgeuse is either a tangentSector or lamina
        if brep.edge_is_lamina(n_edge)?
            || brep.edgeuse_is_tangent_sector(k, neighbour, tan_tol_deg)?
        {
            // save it and this baseEdgeuse in the Tan lists
            tan_eus.push(neighbour);
            tan_uvs.push(base_uv);
        } else {
            // Look for intersecting surface
            if !brep.vertex_are_edges_tangent(vertex, base_edge, n_edge, 1.0)? {
                let adj_eu = brep
                    .edgeuse_radial(neighbour)?
                    .ok_or(dangling("edge use without radial"))?;
                if let Some(off_adj_eu) = executive.offset_edgeuse(adj_eu) {
                    off_adj_eus.push(off_adj_eu);
                }
            }
        }
        Ok(())
    }

    /// Not upstream (WR-346, `SmOffsetExecutive::GetEdgeuseAcrossCollapsedFace`): the
    /// original edge use beyond a collapsed face, seen from its edge use `edgeuse`.
    /// Across a rail (a tangent edge of a four-sided round) it is the radial of the
    /// opposite rail; across an end edge, seen from `from` (one of that edge's
    /// vertices), the radial of the rail at the edge's other vertex. A round of more or
    /// fewer sides (one that meets another round at a corner, or ends on a sliver of a
    /// third face) has its rails told from its ends by what they bound: across a rail it
    /// is the radial of the loop's one other tangent edge onto a face that keeps an
    /// offset. `None` when the edge use's face did not collapse, the face across collapsed
    /// too, or the face has another shape. The four-sided test walks the loop's CCW edge
    /// uses; the end edge's two vertices are `GetVertex` / `GetOtherVertex`.
    ///
    /// # Errors
    ///
    /// [`SolidError::InvalidInput`] for dangling topology; those of the tangency test.
    pub fn edgeuse_across_collapsed_face<O: ClassObject + SolutionObject>(
        &self,
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        executive: &impl OffsetExecutive<O>,
        edgeuse: Option<O>,
        from: Option<O>,
    ) -> Result<Option<O>> {
        let Some(edgeuse) = edgeuse else {
            return Ok(None);
        };
        match brep.edgeuse_face(edgeuse)? {
            Some(face) if executive.is_collapsed_face(face) => {}
            _ => return Ok(None),
        }
        let tan_tol_deg = self.tangency_tol_degrees();
        let next = if brep.edgeuse_is_tangent_sector(k, edgeuse, tan_tol_deg)? {
            // a four-sided loop (bounded walk: a fifth step means more sides)
            let mut sides = 1;
            let mut cur = brep
                .edgeuse_ccw(edgeuse)?
                .ok_or(dangling("edge use without CCW"))?;
            while cur != edgeuse && sides <= 4 {
                sides += 1;
                cur = brep
                    .edgeuse_ccw(cur)?
                    .ok_or(dangling("edge use without CCW"))?;
            }
            if sides == 4 {
                let ccw = brep
                    .edgeuse_ccw(edgeuse)?
                    .ok_or(dangling("edge use without CCW"))?;
                brep.edgeuse_ccw(ccw)?
            } else {
                // a round of more or fewer sides (one that meets another round at a
                // corner, or ends on a sliver of a third face): its other rail is the
                // loop's one other tangent edge onto a face that keeps an offset (bounded
                // walk of the loop)
                let mut rails = Vec::new();
                let mut cur = brep
                    .edgeuse_ccw(edgeuse)?
                    .ok_or(dangling("edge use without CCW"))?;
                let mut steps = 0;
                while cur != edgeuse {
                    steps += 1;
                    if steps > MAX_COLLAPSED_ROUND_SIDES {
                        return Ok(None);
                    }
                    if brep.edgeuse_is_tangent_sector(k, cur, tan_tol_deg)? {
                        let beyond = match brep.edgeuse_radial(cur)? {
                            Some(r) if r != cur => brep.edgeuse_face(r)?,
                            _ => None,
                        };
                        if beyond.is_some_and(|f| !executive.is_collapsed_face(f)) {
                            rails.push(cur);
                        }
                    }
                    cur = brep
                        .edgeuse_ccw(cur)?
                        .ok_or(dangling("edge use without CCW"))?;
                }
                match rails.as_slice() {
                    [rail] => Some(*rail),
                    _ => None,
                }
            }
        } else {
            let Some(from) = from else {
                return Ok(None);
            };
            // the end edge's vertex away from `from` (by position: `from` may be a
            // vertex of the rail beside it)
            let end_edge = brep
                .edgeuse_edge(edgeuse)?
                .ok_or(dangling("edge use without edge"))?;
            let v0 = brep
                .edge_vertex(end_edge)?
                .ok_or(dangling("edge without vertex"))?;
            let v1 = brep
                .edge_other_vertex(end_edge, v0)?
                .ok_or(dangling("edge without other vertex"))?;
            let from_point = brep.vertex_point(from)?;
            let far = if brep.vertex_point(v0)?.distance(from_point)
                > brep.vertex_point(v1)?.distance(from_point)
            {
                v0
            } else {
                v1
            };
            let sides = [
                brep.edgeuse_cw(edgeuse)?
                    .ok_or(dangling("edge use without CW"))?,
                brep.edgeuse_ccw(edgeuse)?
                    .ok_or(dangling("edge use without CCW"))?,
            ];
            let mut found = None;
            for side in sides {
                if side == edgeuse {
                    continue;
                }
                let side_edge = brep
                    .edgeuse_edge(side)?
                    .ok_or(dangling("edge use without edge"))?;
                let s0 = brep.edge_vertex(side_edge)?;
                let s1 = match s0 {
                    Some(s0) => brep.edge_other_vertex(side_edge, s0)?,
                    None => None,
                };
                if s0 == Some(far) || s1 == Some(far) {
                    found = Some(side);
                    break;
                }
            }
            found
        };
        let Some(next) = next else {
            return Ok(None);
        };
        let Some(across) = brep.edgeuse_radial(next)? else {
            return Ok(None);
        };
        if across == next {
            return Ok(None);
        }
        match brep.edgeuse_face(across)? {
            Some(face) if executive.is_collapsed_face(face) => Ok(None),
            _ => Ok(Some(across)),
        }
    }

    /// `GlobalPointSolve(interval, op, point, tol, target, NULL, requested, solutions)`
    /// of the curve behind `curve` over its natural interval.
    fn point_solve<O: ClassObject + SolutionObject>(
        k: &Kernel,
        brep: &impl OffsetBrep<O>,
        curve: O,
        op: SolverOperation,
        point: Point3,
        dist_tol: f64,
        target_distance: Option<f64>,
        requested: SolutionRequested,
    ) -> Result<SolutionArray<O>> {
        let c = curve_of(brep, curve)?;
        c.global_point_solve(
            k,
            &c.natural_interval(),
            op,
            point,
            dist_tol,
            requested,
            &GlobalPointSolveOptions {
                target_distance,
                opt_vectors: None,
                handle: Some(curve),
                analytic_handle: None,
                zone_tol_3d: None,
            },
        )
    }

    /// `ExtendTrimEdges(ctx, exec, pEdgeuse, pOtherEdgeuse, pOffsetEdgeuse,
    /// pOtherOffsetEdgeuse, eMarkType)`: extends or trims a lamina or tangent edge of
    /// the offset. The more iso-parametric of the edge use's and its radial's UV trim
    /// curves (the radial's only with an other offset edge use) is extended ten lengths
    /// each way, trimmed to the extended offset surface's domain and lifted to 3D; on a
    /// non-lamina edge that curve is dropped onto the other extended surface for the
    /// second UV curve (the piece closest to the other UV curve's midpoint, or a copy
    /// of the original UV curve when the drop fails; not upstream, #17514: with both edge
    /// uses on one surface and their UV curves a period apart, the extended UV curve moved
    /// onto the other use's side, `same_surface_twin`). `TrimExtendedEdge` finds the trim
    /// interval; the three curves are trimmed to it and become the offset edge uses'
    /// geometry (`ReplaceEdgeCurves`).
    ///
    /// # Errors
    ///
    /// `SM_ERR` when no trim interval is found or the second UV curve is missing;
    /// [`OffsetFailure::ExtendedCurveOffDomain`] (`SM_ERR`, not upstream, #21743) when
    /// the kept piece of the extended UV curve lies outside the extended surface's domain;
    /// `SER` of the geometry calls (`GetCreateExtendedSurface` included);
    /// [`SolidError::Unported`] for the callees of `TrimExtendedEdge` /
    /// `ReplaceEdgeCurves`;
    /// [`SolidError::InvalidInput`] for null objects upstream dereferences.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub fn extend_trim_edges<O: ClassObject + SolutionObject>(
        &self,
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        executive: &mut impl OffsetExecutive<O>,
        edgeuse: O,
        other_edgeuse: Option<O>,
        offset_edgeuse: Option<O>,
        other_offset_edgeuse: Option<O>,
        mark: MarkType,
    ) -> Result<()> {
        // locals
        let mut uv_curve = brep
            .edgeuse_uv_trim_curve(edgeuse)?
            .ok_or(dangling("edge use without UV trim curve"))?;
        let mut other_uv_curve: Option<O> = None;
        let mut use_rad_uv = false;
        let mut other_mid = Point3::new(0.0, 0.0, 0.0);
        let edge = brep
            .edgeuse_edge(edgeuse)?
            .ok_or(dangling("edge use without edge"))?;

        // Let's first decide if edgeuse or radial UVTrimCurve is optimal TrimCurve to take.
        // We like ISO curves the best.

        // when Edgeuse is not lamina - pick the best UVTrimCurve on Edgeuse and RadialEdgeuse
        if !brep.edge_is_lamina(edge)? {
            let radial = brep
                .edgeuse_radial(edgeuse)?
                .ok_or(dangling("edge use without radial"))?;
            let uv_rad = brep
                .edgeuse_uv_trim_curve(radial)?
                .ok_or(dangling("radial edge use without UV trim curve"))?;

            // Edgeuse and RadialEdgeuse UVCurve bounding boxes and sizes (virtual calls:
            // a UV line's box is its segment's, not clamped to the natural interval)
            let uv = curve_of(brep, uv_curve)?;
            let uv_ivl = uv.natural_interval();
            let req = BoundingBoxRequest {
                normal: true,
                ..BoundingBoxRequest::default()
            };
            let uv_bbox = uv
                .calculate_bounding_box(k, &uv_ivl, req, true)?
                .normal
                .ok_or(dangling("no box"))?;
            // (upstream passes the edge use curve's interval to the radial's box too)
            let uv_rad_bbox = curve_of(brep, uv_rad)?
                .calculate_bounding_box(k, &uv_ivl, req, true)?
                .normal
                .ok_or(dangling("no box"))?;
            let uv_size = uv_bbox.size();
            let uv_rad_size = uv_rad_bbox.size();

            let min_uv = min2(uv_size.x, uv_size.y);
            let min_rad_uv = min2(uv_rad_size.x, uv_rad_size.y);

            // let pUVCurve be the Edgeuse or RadialEdgeuse curve that's the most isoparameter like.
            // But: if we use the radial uv curve, then we have to have pOtherOffsetEdgeuse,
            // otherwise pExtSurf will not get created (see below). [B473]
            if min_rad_uv < min_uv - EFF_ZERO && other_offset_edgeuse.is_some() {
                other_uv_curve = Some(uv_curve);
                uv_curve = uv_rad;
                use_rad_uv = true;
            } else {
                other_uv_curve = Some(uv_rad);
            }
            // refresh locals
            let other = other_uv_curve.ok_or(dangling("other UV curve"))?;
            let orig_ivl = curve_of(brep, other)?.natural_interval();
            other_mid = curve_of(brep, other)?.evaluate_point(k, orig_ivl.evaluate(0.5))?;
        } // end edgeuse is not lamina check

        // Create a greatly extended curve and trim it to the bounding box of the extended surface.
        let uv_bs = b_spline_of(brep, uv_curve)?
            .ok_or(dangling("UV trim curve is not a B-spline curve"))?;
        let ivl = uv_bs.natural_interval();
        let new_min = ivl.min() - 10.0 * ivl.length();
        let new_max = ivl.max() + 10.0 * ivl.length();
        let precise_extension = true; // Otherwise it adds another length of sIvl.

        // pCurveExtended = Extend the UVCurve to the NewMin parameter value (a null
        // extension, of a zero-length interval, is dereferenced upstream)
        let curve_extended = uv_bs
            .create_extended_curve(new_min, ContinuityType::G1G2, precise_extension)?
            .ok_or(dangling("null extended curve"))?;

        // pUVCurveExtended = ReExtend the Extended Curve to the NewMax parameter value
        let mut uv_curve_extended = curve_extended
            .create_extended_curve(new_max, ContinuityType::G1G2, precise_extension)?
            .ok_or(dangling("null extended curve"))?;

        // Clean up extended curve pUVCurveExtended
        let mut ext_ivl = uv_curve_extended.natural_interval();
        uv_curve_extended.remove_extra_knots(EFF_ZERO_SQRT / 100.0)?;

        // locals for surface extension
        let mut ext_surf: Option<(O, Extent2d)> = None;
        let mut other_ext_surf: Option<(O, Extent2d)> = None;

        // when using the Edgeuse UVTrimCurve (else branch) or the RadialEdgeuse UVTrimCurve
        if use_rad_uv {
            // when using the RadialEdgeuse UVTrimCurve
            if let Some(off) = offset_edgeuse {
                let off_face = brep
                    .edgeuse_face(off)?
                    .ok_or(dangling("edge use without face"))?;
                let surf = brep
                    .face_surface(off_face)?
                    .ok_or(dangling("face without surface"))?;
                other_ext_surf = Some(executive.get_create_extended_surface(k, brep, surf)?);
            }
            if let Some(other_off) = other_offset_edgeuse {
                let face2 = brep
                    .edgeuse_face(other_off)?
                    .ok_or(dangling("edge use without face"))?;
                let surf2 = brep
                    .face_surface(face2)?
                    .ok_or(dangling("face without surface"))?;
                ext_surf = Some(executive.get_create_extended_surface(k, brep, surf2)?);
            }
        } else {
            // extend the surface (pOffsetEdgeuse is dereferenced: null is undefined)
            let off_face = brep
                .edgeuse_face(offset_edgeuse.ok_or(dangling("null offset edge use"))?)?
                .ok_or(dangling("edge use without face"))?;
            let surf = brep
                .face_surface(off_face)?
                .ok_or(dangling("face without surface"))?;
            ext_surf = Some(executive.get_create_extended_surface(k, brep, surf)?);

            if !brep.edge_is_lamina(edge)? {
                if let Some(other_off) = other_offset_edgeuse {
                    let face2 = brep
                        .edgeuse_face(other_off)?
                        .ok_or(dangling("edge use without face"))?;
                    let surf2 = brep
                        .face_surface(face2)?
                        .ok_or(dangling("face without surface"))?;
                    other_ext_surf = Some(executive.get_create_extended_surface(k, brep, surf2)?);
                }
            }
        }

        // Trim extended UVTrimCurve to pExtDomain
        let (ext_surf_h, ext_domain) = ext_surf.ok_or(dangling("no extended surface"))?;
        let ext_curve = Curve::from(uv_curve_extended);
        let trim_extended = crate::surface::surface::p1::find_uv_curve_ivl_in_domain::<O>(
            k,
            &ext_curve,
            &ext_ivl,
            ivl.evaluate(0.5),
            &ext_domain,
        )?;
        // Not upstream (#21743): the piece holds the original curve's midpoint and runs
        // between two crossings of the domain's boundary (or to the curve's ends), so it lies
        // wholly inside the domain or wholly outside it, as that midpoint does (a piece just
        // outside, its midpoint within the tolerance below, is still wholly outside: the
        // whole kept piece is checked after the trim, `uv_piece_in_domain`). Outside, the
        // UV curve is not in the extended surface's parameters (an edge use whose curve is
        // on another surface): the surface clamps there, and the lift would run along its
        // boundary, not over the curve. No piece of the curve is on the extended surface, so
        // there is nothing to extend the edge along. "Inside" allows the kernel's UV point
        // tolerance (`EFF_ZERO_SQRT * (1 + |uv|)`, `MapEdgeuses`' UV tolerance): it covers the
        // knot removal above (`EFF_ZERO_SQRT / 100`) and the fitting noise of a trim that runs
        // along the domain's boundary (a seam's), where the clamp moves the lift by no more.
        let mid_uv = ext_curve.evaluate_point(k, ivl.evaluate(0.5))?;
        let (inside, outside) = ext_domain.contains_point_dist(
            Point2::new(mid_uv.x, mid_uv.y),
            EFF_ZERO_SQRT * (1.0 + mid_uv.max_dimension()),
        );
        if !inside {
            let edge = match brep.topology_entity(edge) {
                Some((_, EntityRef::Edge(e))) => Some(e),
                _ => None,
            };
            return Err(failed(
                OP_EXTEND,
                OffsetFailure::ExtendedCurveOffDomain { edge, outside },
            ));
        }
        let CurveKind::BSpline(mut uv_curve_extended) = ext_curve.into_kind() else {
            return Err(dangling("extended curve kind"));
        };
        let mut trim_extended = trim_extended;
        uv_curve_extended.trim(k, &mut trim_extended, TrimMode::Notify)?; // may snap sIvl by tol to existing knots

        // Not upstream (#21743): the midpoint test above settles a piece that is clearly
        // outside, but a midpoint within the tolerance does not bound the rest of the
        // piece: one that stays just outside the boundary, never crossing it, has no
        // crossing to cut it, and its ends can lie far outside. The kept piece, as it will
        // be lifted, must lie inside the domain at every point, within the same
        // tolerance ([`uv_piece_in_domain`]). One that does not is refused as above; an
        // undecided one (within its bounds) is lifted, as before.
        if let PieceInDomain::Outside(outside) =
            uv_piece_in_domain(&uv_curve_extended, &ext_domain)?
        {
            let edge = match brep.topology_entity(edge) {
                Some((_, EntityRef::Edge(e))) => Some(e),
                _ => None,
            };
            return Err(failed(
                OP_EXTEND,
                OffsetFailure::ExtendedCurveOffDomain { edge, outside },
            ));
        }

        // refresh sExtIvl to trimmed Ivl
        ext_ivl = uv_curve_extended.natural_interval();

        // Set pLiftedCurve = pUVCurveExtended projected through pExtSurf
        let edge_tol = brep.edge_tolerance(edge)?;
        let ext_surface = surface_of(brep, ext_surf_h)?.clone_for_evaluation()?;
        let lifted = ext_surface
            .lift_curve(
                k,
                &ext_domain,
                &Curve::from(uv_curve_extended.clone()),
                &ext_ivl,
                edge_tol / 3.0,
                false,
            )?
            .curve;
        let lifted_curve = lifted;

        // When the Edgeuse is not lamina - build the 2nd UVTrimCurve on the other Surface
        let mut uv_curve_extended_h: Option<O> = None;
        // pOtherUVCurve: the edge use's or radial's own UV trim curve until the drop below
        // replaces it (cpp:1452/1458); without an other extended surface it reaches
        // TrimExtendedEdge and is trimmed in place (cpp:1694)
        let mut other_uv_h: Option<O> = other_uv_curve;
        if let Some((other_ext_h, other_ext_domain)) = other_ext_surf {
            // Not upstream (#17514): both edge uses on one surface (a seam, the other side
            // of a closed face's period) take the lifted curve back on that surface. Where
            // the step between the two edge uses' own UV curves (a whole period across a
            // seam) carries the first onto the same points of the surface, within the
            // edge's tolerance (`same_surface_twin`), the second UV curve is the extended
            // UV curve itself moved by that step: the same parameters, exactly. Dropping
            // the lifted curve instead gives a piece on each side of the seam, and a piece
            // cut short (a tube's extension turning past a whole turn) left the trim
            // interval outside it. Two UV curves that do not run over one edge (a step
            // that is not a period) are dropped as upstream does.
            let twin = if other_ext_h == ext_surf_h {
                let other = other_uv_curve.ok_or(dangling("null other UV curve"))?;
                same_surface_twin(
                    &uv_curve_extended,
                    curve_of(brep, uv_curve)?,
                    curve_of(brep, other)?,
                    &ext_surface,
                    edge_tol,
                    k,
                )?
            } else {
                None
            };
            let candidate_h = if let Some(twin) = twin {
                store_curve(brep, twin)?
            } else {
                // (pOffsetEdgeuse->GetEdge()->GetTolerance(): null is undefined)
                let off_edge = brep
                    .edgeuse_edge(offset_edgeuse.ok_or(dangling("null offset edge use"))?)?
                    .ok_or(dangling("edge use without edge"))?;
                let off_tol = brep.edge_tolerance(off_edge)?;

                // Drop Lifted Curve onto other extended surface to make 2nd UVTrimCurve
                // (the drop's trace gets the surface's cache, `smsurf_GetSurfaceCache`: the
                // face's trimmed cache when a face owns the extended surface)
                let other_ext = surface_of(brep, other_ext_h)?.clone_for_evaluation()?;
                let mut face_cache = if other_ext.face()?.is_some() {
                    Some(brep.prepare_surface_solve_cache(k, &other_ext, Some(other_ext_h))?)
                } else {
                    None
                };
                let dropped = other_ext.drop_and_trim_curve_in::<O, O>(
                    k,
                    face_cache.as_mut(),
                    &other_ext_domain,
                    &lifted_curve,
                    &lifted_curve.natural_interval(),
                    off_tol / 3.0,
                    true,
                    true,
                    None,
                )?;
                // Not upstream (WR-530, #402; the fork's tangency-labs/smlib@23c5b0a): the edge is
                // lifted onto one face's new surface and dropped onto the other's. Faces offset
                // alike stay tangent, so the drop lands on the edge; when only one of them got a
                // new surface (a local operation moving a face beside a blend, or one piece of a
                // split face), the drop crosses the gap between them and the faces no longer
                // meet there
                if executive.is_local_operation() {
                    let meet_tol = 10.0 * smos_max(off_tol, self.this_approx_tol_3d());
                    let max_drop_to_surf = dropped.max_drop_to_surf;
                    if drop_is_loose(max_drop_to_surf, meet_tol) {
                        // the edge of the input, named for the refusal where the handle names
                        // one (error context only)
                        let edge = match brep.topology_entity(edge) {
                            Some((_, EntityRef::Edge(e))) => Some(e),
                            _ => None,
                        };
                        let reason = OffsetFailure::LooseTangentEdge {
                            edge,
                            max_drop_to_surf,
                            meet_tol,
                        };
                        return Err(Failure::new(OP_EXTEND, reason).into_error());
                    }
                }
                let uv_curves = dropped.uv_curves;
                let mut closest_idx = 0usize;

                // the candidate for the 2nd UVTrimCurve
                if uv_curves.is_empty() {
                    // LiftedCurve failed to Drop to OtherExtSurf - Unable to do dropping - just use
                    // original UV and copy it (the status of the copy is ignored)
                    let other = other_uv_curve.ok_or(dangling("null UV curve copied"))?;
                    // (`pOtherUVCurve->Copy(crContext, pOtherUVCurve)` on an
                    // `SmBSplineCurve*` binds the non-virtual `SmBSplineCurve*&` overload:
                    // a plain B-spline copy, a line's analytic data is dropped)
                    let copy = b_spline_overload_copy(curve_of(brep, other)?)?;
                    store_curve(brep, copy)?
                } else {
                    // When Lifted Curve dropped to OtherExtSurf

                    // When LiftedCurve dropped in more than 1 piece to OtherExtSurf
                    if uv_curves.len() > 1 {
                        // Find the UVTrimCurve closest to the original - assume curve dropped to a seam
                        let mut min_dist = BIG_DOUBLE;
                        for (j, test) in uv_curves.iter().enumerate() {
                            let tc = test.clone();
                            let sols2: SolutionArray<O> = tc.global_point_solve(
                                k,
                                &tc.natural_interval(),
                                SolverOperation::Minimize,
                                other_mid,
                                EFF_ZERO_SQRT,
                                SolutionRequested::Single,
                                &GlobalPointSolveOptions::default(),
                            )?;
                            let sol2 = sols2
                                .get(0)
                                .ok_or(dangling("point solve without solution"))?;

                            // save the UVTrimCurve with the shortest drop
                            if sol2.start.solution_value < min_dist {
                                closest_idx = j;
                                min_dist = sol2.start.solution_value;
                            }
                        } // end iter every DropCurve candidate - looking for best to use
                    } // end LiftedCurve dropped in more than 1 piece check

                    // Set pOtherUVCurve to best LiftedCurve dropped to OtherExtSurf UVTrimCurve
                    let best = uv_curves
                        .get(closest_idx)
                        .ok_or(dangling("drop curve index"))?
                        .clone();
                    store_curve(brep, best)?
                } // end LiftedCurve failed to drop to OtherExtSurf branch
            };

            // When using the RadialEdgeuse - swap the extended UVTrimCurves
            if use_rad_uv {
                // Swap them back
                other_uv_h = Some(store_curve(brep, Curve::from(uv_curve_extended.clone()))?);
                uv_curve_extended_h = Some(candidate_h);
            } else {
                other_uv_h = Some(candidate_h);
            }
        } // end pOtherExtSurf existence check - working with a nonLamina edge (assumes manifold edge)

        // the extended UV curve as a handle when it was not swapped in above
        let uv_curve_extended_h = match uv_curve_extended_h {
            Some(h) => h,
            None => store_curve(brep, Curve::from(uv_curve_extended.clone()))?,
        };

        // Now that we have created one or two UV curves and a 3D curve find trimming
        // interval and if we find a good one replace the geometry.
        let lifted_h = store_curve(brep, lifted_curve.clone())?;
        let trimmed = self.trim_extended_edge_ends(
            k,
            brep,
            executive,
            edgeuse,
            other_edgeuse,
            lifted_h,
            Some(uv_curve_extended_h),
            other_uv_h,
            mark,
        )?;

        // when TrimExtendedEdge() call worked
        let Some(TrimmedEdge {
            interval: trim_ivl,
            inverted,
            periodic_ends,
        }) = trimmed
        else {
            // TrimExtendedEdge() call did not work: a search that found no cut
            return Err(failed(OP_EXTEND, OffsetFailure::ExtendedEdgeUntrimmed));
        };

        // Trim the curves to the common interval so the 3D curve and its UV curves end together
        let c_lifted = curve_of(brep, lifted_h)?;
        let c_uv = curve_of(brep, uv_curve_extended_h)?;
        let mut curves_to_trim = vec![c_lifted, c_uv];
        if let Some(other) = other_uv_h {
            curves_to_trim.push(curve_of(brep, other)?);
        }
        let ti = crate::surface::surface::exact_pieces::common_trim_interval(
            k,
            &curves_to_trim,
            trim_ivl,
            OP_EXTEND,
        )?;
        let mut handles = vec![lifted_h, uv_curve_extended_h];
        if let Some(other) = other_uv_h {
            handles.push(other);
        }
        for h in handles {
            let c = brep
                .curve_mut(h)
                .ok_or(dangling("curve handle cannot be resolved"))?;
            crate::surface::surface::exact_pieces::trim_unsnapped(k, c, ti, OP_EXTEND)?;
        }

        let lifted_copy = curve_of(brep, lifted_h)?.copy(k)?;
        let lifted_copy_h = store_curve(brep, lifted_copy)?;

        // change the pOffsetEdgeuse geometry (ReplaceEdgeCurves dereferences it: null is
        // undefined)
        let r = Self::replace_edge_curves_oriented(
            k,
            brep,
            offset_edgeuse.ok_or(dangling("null offset edge use"))?,
            Some(lifted_h),
            Some(uv_curve_extended_h),
            None,
            inverted,
        );
        // (the rbDelete* flags only free the candidates upstream)
        r?;
        if inverted {
            for off in [offset_edgeuse, other_offset_edgeuse].into_iter().flatten() {
                if let Some(face) = brep.edgeuse_face(off)? {
                    executive.note_inverted_face(face);
                }
            }
        }

        // when appropriate - change the pOtherOffsetEdgeuse geometry
        if let Some(other_off) = other_offset_edgeuse {
            // Not Lamina
            // (only a lamina edge, which has no other side, has no second UV curve)
            let Some(other) = other_uv_h else {
                return Err(failed(OP_EXTEND, OffsetFailure::LaminaEdgeOtherSide));
            };
            // GWC:BUG?? These two Edgeuses point to the same edge - these two calls should
            //           have the same 3DCurve
            Self::replace_edge_curves_oriented(
                k,
                brep,
                other_off,
                Some(lifted_copy_h),
                Some(other),
                None,
                inverted,
            )?;
        } // end pOtherOffsetEdgeuse existence check

        executive.note_mated_edgeuse(edgeuse)?;
        if let Some(other) = other_edgeuse {
            executive.note_mated_edgeuse(other)?;
        }
        for event in periodic_ends {
            executive.note_periodic_boundary_end(event)?;
        }

        // all done
        Ok(())
    }

    /// `MateEdges(ctx, exec, pEdgeuse, pOtherEdgeuse, pOffsetEdgeuse,
    /// pOtherOffsetEdgeuse, eMarkType)`: mates two offset edge uses by extending their
    /// faces' surfaces and intersecting them (at a quarter of the B-rep tolerance, 20
    /// degrees, using the surface edges); of the non-degenerate intersection curves
    /// that `TrimExtendedEdge` can trim, the one whose trimmed midpoint is closest to
    /// the original edge use's midpoint (the only one, when alone) is trimmed with its
    /// UV curves and becomes the geometry of both offset edge uses
    /// (`ReplaceEdgeCurves`, oriented by the original edge uses). Not upstream: at an edge
    /// whose faces were tangent in the input and whose new surfaces, a plane and a curved
    /// surface, cross in a local operation (`OffsetExecutive::crosses_at`), only the curves
    /// on the curved face's side of the edge are candidates.
    /// A disappearing periodic seam instead requires the unique branch incident
    /// to its already mated opposite boundary; midpoint distance does not select it.
    ///
    /// With [`crate::kernel::UpstreamFixes::offset_analytic_mate_edges`] (the SolidCraft
    /// fork's tangency-labs/smlib@fddc366 and @98ab1b0), each marched intersection
    /// piece that lies on an exact line or conic of analytic copies of the two face
    /// surfaces is first replaced by that curve, with UV curves dropped from it.
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` for a null edge use or a missing UV curve; `SM_ERR`
    /// [`OffsetFailure::TangentBranchUncertified`] where tangent curved neighbours now cross
    /// without a proof of the retained intersection branch or a unique physical circle;
    /// [`OffsetFailure::PeriodicBranchAmbiguous`] where multiple SSI branches claim
    /// the same periodic boundary event;
    /// when no intersection curve can be trimmed ([`OffsetFailure::TangentEndOffCurve`]
    /// where a curve was refused for it); `SER` of the geometry calls
    /// (`GetCreateExtendedSurface` included); [`SolidError::Unported`] for the callees
    /// of `TrimExtendedEdge` / `ReplaceEdgeCurves`.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub fn mate_edges<O: ClassObject + SolutionObject>(
        &self,
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        executive: &mut impl OffsetExecutive<O>,
        edgeuse: Option<O>,
        other_edgeuse: Option<O>,
        offset_edgeuse: Option<O>,
        other_offset_edgeuse: Option<O>,
        mark: MarkType,
    ) -> Result<()> {
        // These can't be Null (the caller's arguments: `edgeuse_offset` mates no edge without
        // them, and the executive passes all four):
        let missing = || failed(OP_MATE, OffsetFailure::EdgeuseNotGiven);
        let edgeuse = edgeuse.ok_or_else(missing)?;
        let other_edgeuse = other_edgeuse.ok_or_else(missing)?;
        let offset_edgeuse = offset_edgeuse.ok_or_else(missing)?;
        let other_offset_edgeuse = other_offset_edgeuse.ok_or_else(missing)?;

        // Extend Surfaces of faces connected to current edge.
        let face1 = brep
            .edgeuse_face(offset_edgeuse)?
            .ok_or(dangling("edge use without face"))?;
        let surf1 = brep
            .face_surface(face1)?
            .ok_or(dangling("face without surface"))?;
        let face2 = brep
            .edgeuse_face(other_offset_edgeuse)?
            .ok_or(dangling("edge use without face"))?;
        let surf2 = brep
            .face_surface(face2)?
            .ok_or(dangling("face without surface"))?;
        // Not upstream: where faces tangent in the input cross in a local operation
        // (`OffsetExecutive::crosses_at`), the plane cuts the curved surface on both sides of
        // the old edge, about as far on each (`√(2 r δ)` for a round of radius `r` moved
        // `δ`), so the nearest midpoint decides nothing. The edge is the cut on the curved
        // face's own side, where the plane is extended past its edge; the other lies on the
        // curved surface's continuation past its old tangent line.
        let curved_side = if executive.crosses_at(edgeuse)
            && brep.edgeuse_is_tangent_sector(k, edgeuse, self.tangency_tol_degrees())?
        {
            let plane = |h: O| -> Result<bool> {
                Ok(matches!(surface_of(brep, h)?.kind(), SurfaceKind::Plane(_)))
            };
            let curved = match (plane(surf1)?, plane(surf2)?) {
                (true, false) => Some(other_edgeuse),
                (false, true) => Some(edgeuse),
                (false, false)
                    if unique_cone_meeting(
                        k,
                        surface_of(brep, surf1)?,
                        surface_of(brep, surf2)?,
                    )? =>
                {
                    None
                }
                (false, false) => {
                    // The nearest circle can lie on a curved face's continuation past
                    // the old edge. Unlike a plane/curved pair, neither face supplies
                    // the existing one-sided construction; distance proves no branch.
                    return Err(
                        Failure::new(OP_MATE, OffsetFailure::TangentBranchUncertified).into_error(),
                    );
                }
                (true, true) => None,
            };
            match curved {
                Some(curved) => Some((
                    edgeuse_normalized_evaluate(k, brep, edgeuse, 0.5, false, false, true)?,
                    brep.edgeuse_middle_binormal(k, curved)?,
                )),
                None => None,
            }
        } else {
            None
        };

        let (ext_surface1, domain1) = executive.get_create_extended_surface(k, brep, surf1)?;
        let (ext_surface2, domain2) = executive.get_create_extended_surface(k, brep, surf2)?;

        // locals for surf/surf intersection
        let edge = brep
            .edgeuse_edge(edgeuse)?
            .ok_or(dangling("edge use without edge"))?;
        let ssi_tol = brep.brep_tolerance(edge)? / 4.0;
        let deg20_rad = 20.0_f64.to_radians();
        let use_surface_edges = [true, true];

        // Intersect extended surfaces. The evaluation copies cannot be passed for
        // both when the two faces share one surface (`SmSurfaceIntersector` records
        // `doing_self_intersection` from pointer identity, and the C++ call passes
        // the same surface pointer twice).
        let s1 = surface_of(brep, ext_surface1)?.clone_for_evaluation()?;
        let s2 = surface_of(brep, ext_surface2)?.clone_for_evaluation()?;
        let other = if ext_surface1 == ext_surface2 {
            &s1
        } else {
            &s2
        };
        let mut ssi = s1.global_surface_intersect_with_context(
            k,
            [Some(ext_surface1), Some(ext_surface2)],
            &mut OffsetSurfaceSolveContext(brep),
            &domain1,
            other,
            &domain2,
            use_surface_edges,
            Some(ssi_tol),
            Some(deg20_rad),
        )?;
        // Not upstream: an extension kept exactly keeps its analytic kind, so the
        // intersection may be an analytic special case's exact curve, over the whole
        // analytic surfaces and without UV curves, where upstream's fitted extensions give
        // marched pieces bounded by the domains, with their UV curves. Each such curve is
        // its pieces inside both extended domains, with their UV curves, proved on both.
        // Every failure of the cut is typed and passed on (never read as "unmated").
        // The drops run under the intersection's face-aware context: an extended surface
        // that is a face's own surface traces on its face's cache.
        ssi.clip_exact_curves_with_context(
            k,
            Some(SurfaceIntersectContext {
                solver: &mut OffsetSurfaceSolveContext(brep),
                handles: [Some(ext_surface1), Some(ext_surface2)],
            }),
            &s1,
            &domain1,
            other,
            &domain2,
            ssi_tol,
        )
        .map_err(|e| {
            if e.is_unported_or_cancelled() {
                e
            } else {
                Failure::new(OP_MATE, OffsetFailure::ExactCurveUncut)
                    .caused_by(e)
                    .into_error()
            }
        })?;

        // `Kernel::fixes.offset_analytic_mate_edges` (tangency-labs/smlib@fddc366 and
        // @98ab1b0, #1453/#1130): two analytic surfaces meet on lines and conics. The
        // marched intersection of their B-spline extensions gives the pieces (bounded by
        // the extended domains and seams) and their UV curves; each piece that lies on
        // both extended surfaces as the exact curve between the same ends (within the
        // analytic surfaces, not on a tangent extension) is replaced by that curve, its
        // UV curves dropped from it.
        if k.fixes.offset_analytic_mate_edges {
            let upstream = self.offset_distance().abs() * 5.0;
            let margin1 = executive.analytic_extension_margin(surf1, upstream);
            let margin2 = executive.analytic_extension_margin(surf2, upstream);
            let analytic1 = create_analytic_extension(k, surface_of(brep, surf1)?, margin1)?;
            let analytic2 = create_analytic_extension(k, surface_of(brep, surf2)?, margin2)?;
            if let (Some(analytic1), Some(analytic2)) = (&analytic1, &analytic2) {
                if !ssi.curves_3d.is_empty() {
                    let exact = analytic1.global_surface_intersect(
                        k,
                        &analytic1.natural_uv_domain_with_nurb(k)?,
                        analytic2,
                        &analytic2.natural_uv_domain_with_nurb(k)?,
                        use_surface_edges,
                        Some(ssi_tol),
                        Some(deg20_rad),
                    )?;
                    let tol = brep.brep_tolerance(edge)?;
                    for ii in 0..ssi.curves_3d.len() {
                        let marched = ssi.curves_3d.get(ii).ok_or(dangling("curve index"))?;
                        if marched.is_degenerate(k, EFF_ZERO, None)? {
                            continue;
                        }
                        let Some(piece) = exact_piece(k, marched, &exact.curves_3d, tol)? else {
                            continue;
                        };
                        let piece = Curve::from(piece);
                        let mut uv: [Option<Curve>; 2] = [None, None];
                        for (i_surf, slot) in uv.iter_mut().enumerate() {
                            let (ext, dom) = if i_surf == 0 {
                                (&s1, &domain1)
                            } else {
                                (other, &domain2)
                            };
                            let dropped = ext.drop_curve::<NoObject>(
                                k,
                                dom,
                                &piece,
                                &piece.natural_interval(),
                                ssi_tol,
                                true,
                                None,
                                true,
                            )?;
                            if dropped.uv_curves.len() == 1 && dropped.max_drop_to_surf <= tol {
                                *slot = dropped.uv_curves.into_iter().next();
                            }
                        }
                        let [Some(uv1), Some(uv2)] = uv else {
                            // The piece leaves an extended surface where it is a tangent
                            // (G1R) extension, not the analytic surface: there the marched
                            // piece is these surfaces' intersection.
                            continue;
                        };
                        if let Some(c) = ssi.curves_3d.get_mut(ii) {
                            *c = piece;
                        }
                        if let Some(c) = ssi.surface1_uv_curves.get_mut(ii) {
                            *c = Some(uv1);
                        }
                        if let Some(c) = ssi.surface2_uv_curves.get_mut(ii) {
                            *c = Some(uv2);
                        }
                    }
                }
            }
        }

        // (not upstream, #17210) one curve through a tangent point of the surfaces
        join_through_tangent_points(
            k,
            &mut ssi,
            [&s1, other],
            brep.brep_tolerance(edge)?,
            self.tangency_tol_radians(),
        )?;

        let mut curves_3d: Vec<O> = Vec::new();
        let mut surface1_uv_curves: Vec<Option<O>> = Vec::new();
        let mut surface2_uv_curves: Vec<Option<O>> = Vec::new();
        for c in ssi.curves_3d {
            curves_3d.push(store_curve(brep, c)?);
        }
        for c in ssi.surface1_uv_curves {
            surface1_uv_curves.push(match c {
                Some(c) => Some(store_curve(brep, c)?),
                None => None,
            });
        }
        for c in ssi.surface2_uv_curves {
            surface2_uv_curves.push(match c {
                Some(c) => Some(store_curve(brep, c)?),
                None => None,
            });
        }

        // Now see if we have an intersection curve that has proper trimming
        let mut off_curve: Option<SolidError> = None;
        let mut min_dist = BIG_DOUBLE;
        // (the chosen curve with its two UV curves, which every candidate has)
        let mut min_curve: Option<(O, O, O)> = None;
        let mut trim_ivl = Extent1d::new();
        let mut inverted = false;
        let mut periodic_ends = Vec::new();
        let mut periodic_curve = None;

        // for every intersection solution
        for ii in 0..curves_3d.len() {
            let c3 = *curves_3d.get(ii).ok_or(dangling("curve index"))?;

            // skip degenerate curve solutions
            if curve_of(brep, c3)?.is_degenerate(k, EFF_ZERO, None)? {
                continue;
            }

            // get iith XSect 3D and UVTrim Curves (null UVTrimCurves are an error; not
            // upstream: a port invariant, every curve the intersection gives here has
            // both, so it is passed on, never taken for an edge that cannot be mated)
            let missing_uv = || {
                SolidError::port_invariant(
                    OP_MATE,
                    "an intersection curve of the extended surfaces has no UV curve",
                )
            };
            let uv1 = surface1_uv_curves
                .get(ii)
                .copied()
                .flatten()
                .ok_or_else(missing_uv)?;
            let uv2 = surface2_uv_curves
                .get(ii)
                .copied()
                .flatten()
                .ok_or_else(missing_uv)?;
            if !is_b_spline_kind(curve_of(brep, uv1)?) || !is_b_spline_kind(curve_of(brep, uv2)?) {
                return Err(defect(OP_MATE, SSI_UV_CURVE_KIND, SM_ERR_NULL_POINTER));
            }

            // Trim the extended Surf/Surf intersection (not upstream: a curve that does not
            // end at a crossing tangent neighbour's vertex is no candidate, and is the
            // refusal when no curve is one)
            let my_trim = match self.trim_extended_edge_ends(
                k,
                brep,
                executive,
                edgeuse,
                Some(other_edgeuse),
                c3,
                Some(uv1),
                Some(uv2),
                mark,
            ) {
                Err(e) if is_tangent_end_off_curve(&e) => {
                    off_curve = Some(e);
                    None
                }
                r => r?,
            };

            // (not upstream) a cut on the curved surface's continuation is no candidate
            let my_trim = match (my_trim, curved_side) {
                (Some(my_trim), Some((orig_mid, into_curved))) => {
                    let mid_pnt =
                        curve_of(brep, c3)?.evaluate_point(k, my_trim.interval.evaluate(0.5))?;
                    ((mid_pnt - orig_mid).dot(into_curved) > 0.0).then_some(my_trim)
                }
                (my_trim, _) => my_trim,
            };

            // when intersection was successfully trimmed
            if let Some(TrimmedEdge {
                interval: my_trim,
                inverted: my_inverted,
                periodic_ends: my_periodic_ends,
            }) = my_trim
            {
                if !my_periodic_ends.is_empty() {
                    if periodic_curve.is_some() {
                        return Err(SolidError::offset(
                            OP_MATE,
                            OffsetFailure::PeriodicBranchAmbiguous,
                        ));
                    }
                    periodic_curve = Some(ii);
                    min_curve = Some((c3, uv1, uv2));
                    trim_ivl = my_trim;
                    inverted = my_inverted;
                    periodic_ends = my_periodic_ends;
                    continue;
                }
                if periodic_curve.is_some() {
                    continue;
                }
                // when there is more than 1 intersection curve to check
                if curves_3d.len() > 1 {
                    // save the trimmed XSectCurve that has the smallest change from the original 3DCurve.
                    let mid_pnt = curve_of(brep, c3)?.evaluate_point(k, my_trim.evaluate(0.5))?;
                    let orig_mid =
                        edgeuse_normalized_evaluate(k, brep, edgeuse, 0.5, false, false, true)?; // FALSE = 3d Eval

                    let dist = distance(mid_pnt, orig_mid);

                    if dist < min_dist {
                        min_curve = Some((c3, uv1, uv2));
                        min_dist = dist;
                        trim_ivl = my_trim;
                        inverted = my_inverted;
                        periodic_ends = my_periodic_ends;
                    }
                } else {
                    // only 1 curve to check branch - use this curve
                    trim_ivl = my_trim;
                    inverted = my_inverted;
                    min_curve = Some((c3, uv1, uv2));
                    periodic_ends = my_periodic_ends;
                }
            }
        } // end iter every extended Surf/Surf intersection

        // Error Exit - no XSectCurve found: no curve was cut at the edge's ends
        let Some((curve_3d_found, uv_curve1_found, uv_curve2_found)) = min_curve else {
            return Err(
                off_curve.unwrap_or_else(|| failed(OP_MATE, OffsetFailure::ExtendedEdgeUntrimmed))
            );
        };

        // arrive here after finding the XSectCurve result whose midpoint that is closest
        // to the pEdgeuse->Curve->midPoint (its UV curves are the candidate's, which the loop
        // above required)

        // (the found curves are removed from the arrays upstream so that they are not deleted)

        // Trim bound best XSectCurves to sTrimIvl (may snap sIvl by tol to existing knots).
        // Not upstream: upstream trims them in turn, the interval snapped by each trim
        // given to the next, so a curve trimmed before a later snap ends at the unsnapped
        // parameter: the edge's interval (its 3D curve's, `replace_edge_curves`) is then
        // not its UV curves'. Upstream's marched curves share their knots and snap alike;
        // an exact curve's piece (`clip_exact_curves`, a conic snapping onto its NURB's
        // knots) and its dropped UV curves do not. All three are trimmed to the one
        // interval none of their trims snaps.
        let ti = crate::surface::surface::exact_pieces::common_trim_interval(
            k,
            &[
                curve_of(brep, curve_3d_found)?,
                curve_of(brep, uv_curve1_found)?,
                curve_of(brep, uv_curve2_found)?,
            ],
            trim_ivl,
            OP_MATE,
        )?;
        for h in [curve_3d_found, uv_curve1_found, uv_curve2_found] {
            let c = brep
                .curve_mut(h)
                .ok_or(dangling("curve handle cannot be resolved"))?;
            crate::surface::surface::exact_pieces::trim_unsnapped(k, c, ti, OP_MATE)?;
        }

        // Replace Curve of edge and set its new domain.
        let copy_3d = curve_of(brep, curve_3d_found)?.copy(k)?;
        let copy_3d_h = store_curve(brep, copy_3d)?;

        // when appropriate - change the pOffsetEdgeuse geometry (not upstream: an offset
        // edge whose ends match is kept only when proved on both extended surfaces, see
        // `replace_edge_curves_in`; an inverted edge never keeps its curve, so it is not
        // checked)
        let keep = KeepOld::Decided(
            !inverted && Self::keeps_old_edge(k, brep, offset_edgeuse, curve_3d_found, [&s1, &s2])?,
        );
        Self::replace_edge_curves_in(
            k,
            brep,
            offset_edgeuse,
            Some(curve_3d_found),
            Some(uv_curve1_found),
            Some(edgeuse),
            keep,
            inverted,
        )?;

        // when appropriate - change the pOtherOffsetEdgeuse geometry
        // GWC:BUG?? These two Edgeuses point to the same edge - these two calls should
        //           have the same 3DCurve. It's ok: At this point they do not (yet) point to the same edge.
        let keep = KeepOld::Decided(
            !inverted
                && Self::keeps_old_edge(k, brep, other_offset_edgeuse, copy_3d_h, [&s1, &s2])?,
        );
        Self::replace_edge_curves_in(
            k,
            brep,
            other_offset_edgeuse,
            Some(copy_3d_h),
            Some(uv_curve2_found),
            Some(other_edgeuse),
            keep,
            inverted,
        )?;
        if inverted {
            for off in [offset_edgeuse, other_offset_edgeuse] {
                if let Some(face) = brep.edgeuse_face(off)? {
                    executive.note_inverted_face(face);
                }
            }
        }

        executive.note_mated_edgeuse(edgeuse)?;
        executive.note_mated_edgeuse(other_edgeuse)?;
        for event in periodic_ends {
            executive.note_periodic_boundary_end(event)?;
        }

        // all done
        Ok(())
    }

    /// `HaveTangentGaps(lNumSamples, cpOffsetEdgeuse, cpOtherOffsetEdgeuse)`: whether
    /// any of `num_samples + 1` points along the other offset edge use is farther from
    /// the offset edge use's edge than its tolerance (a closest point was found).
    ///
    /// # Errors
    ///
    /// Evaluation and closest-point statuses are ignored (`Unported` aside); a sample
    /// that cannot be evaluated is skipped (fork 4803d6d). A successful closest-point
    /// query without a solution does not establish a gap.
    /// [`SolidError::InvalidInput`] for an edge use without edge.
    pub fn have_tangent_gaps<O: ClassObject + SolutionObject>(
        &self,
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        num_samples: u64,
        offset_edgeuse: O,
        other_offset_edgeuse: O,
    ) -> Result<bool> {
        let edge = brep
            .edgeuse_edge(offset_edgeuse)?
            .ok_or(dangling("edge use without edge"))?;
        let tol = brep.edge_tolerance(edge)?;

        tangent_gap_samples(
            k,
            brep,
            num_samples,
            tol,
            |k, b, t| {
                edgeuse_normalized_evaluate(k, b, other_offset_edgeuse, t, false, false, true)
            },
            |k, b, p| {
                b.edge_closest_point(k, edge, p)
                    .map(|cp| cp.map(|cp| cp.distance))
            },
        )
    }

    /// Not upstream (`IsStepAcrossEdge`, fork tangency-labs/smlib 5d428ae, the WR-530
    /// addendum's step walls): whether the faces of two offset edge
    /// uses of one tangent edge stayed parallel across it but apart. At each of 11 samples
    /// along the first edge use the nearest point of the other's edge must lie farther
    /// than the edges meet within (`10 * max(edge tolerances, this_approx_tol_3d)`) and the
    /// gap must run along both faces' normals within that distance. A face that turned
    /// instead (a draft) leaves its gap off its normal and is no step.
    ///
    /// # Errors
    ///
    /// Those of the evaluations, the closest point and the normals (`SER`);
    /// [`SolidError::InvalidInput`] for missing topology or geometry (`NER`).
    pub fn is_step_across_edge<O: ClassObject + SolutionObject>(
        &self,
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        offset_edgeuse: O,
        other_offset_edgeuse: O,
    ) -> Result<bool> {
        // The fork's sample count: 11 points along the edge use.
        const NUM_SAMPLES: u32 = 10;
        let edge = brep
            .edgeuse_edge(offset_edgeuse)?
            .ok_or(dangling("edge use without edge"))?;
        let other_edge = brep
            .edgeuse_edge(other_offset_edgeuse)?
            .ok_or(dangling("edge use without edge"))?;
        let face = brep
            .edgeuse_face(offset_edgeuse)?
            .ok_or(dangling("edge use without face"))?;
        let other_face = brep
            .edgeuse_face(other_offset_edgeuse)?
            .ok_or(dangling("edge use without face"))?;
        let surf = brep
            .face_surface(face)?
            .ok_or(dangling("face without surface"))?;
        let other_surf = brep
            .face_surface(other_face)?
            .ok_or(dangling("face without surface"))?;
        if edge == other_edge {
            return Ok(false);
        }

        // The distance the two edges meet within (as ExtendTrimEdges judges a drop).
        let meet_tol = 10.0
            * max2(
                max2(brep.edge_tolerance(edge)?, brep.edge_tolerance(other_edge)?),
                self.this_approx_tol_3d,
            );
        let other_ivl = brep.edge_interval(other_edge)?;
        let other_len = other_ivl.length();
        let positive = other_len > 0.0;
        if !positive {
            return Ok(false);
        }
        let other_opposite =
            brep.edgeuse_orientation(other_offset_edgeuse)? == OrientType::Opposite;

        for ii in 0..=NUM_SAMPLES {
            let t = f64::from(ii) / f64::from(NUM_SAMPLES);
            let pnt = edgeuse_normalized_evaluate(k, brep, offset_edgeuse, t, false, false, true)?;
            let uv = edgeuse_normalized_evaluate(k, brep, offset_edgeuse, t, true, false, true)?;

            let Some(cp) = brep.edge_closest_point(k, other_edge, pnt)? else {
                return Ok(false);
            };
            let apart = cp.distance > meet_tol;
            if !apart {
                return Ok(false);
            }
            let mut other_t = (cp.parameter - other_ivl.min()) / other_len;
            if other_opposite {
                other_t = 1.0 - other_t;
            }
            other_t = min2(1.0, max2(0.0, other_t));
            let other_pnt = edgeuse_normalized_evaluate(
                k,
                brep,
                other_offset_edgeuse,
                other_t,
                false,
                false,
                true,
            )?;
            let other_uv = edgeuse_normalized_evaluate(
                k,
                brep,
                other_offset_edgeuse,
                other_t,
                true,
                false,
                true,
            )?;

            let normal = surface_of(brep, surf)?.evaluate_normal(
                k,
                Point2::new(uv.x, uv.y),
                Side::Right,
                Side::Right,
            )?;
            let other_normal = surface_of(brep, other_surf)?.evaluate_normal(
                k,
                Point2::new(other_uv.x, other_uv.y),
                Side::Right,
                Side::Right,
            )?;

            // The gap runs along both unit normals: what lies across each is within the
            // meet distance.
            let gap = other_pnt - pnt;
            let along = gap.project_to_plane(normal).length() <= meet_tol
                && gap.project_to_plane(other_normal).length() <= meet_tol;
            if !along {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Not upstream (`CreateStepWall`, fork tangency-labs/smlib 5d428ae and 90aabff,
    /// the WR-530 addendum's step walls): the ruled wall between two
    /// offset edge uses whose faces lie apart ([`Self::is_step_across_edge`]), stored and
    /// returned as a handle. The rails are the two edges' trimmed NURBS curves; a rail
    /// whose weights are all one value (to `SM_EFF_ZERO` relative) is made non-rational
    /// first (`MakeNonRational`), so two lines stored with different weights rule a
    /// bilinear wall. The second rail is reversed when its points at a quarter, half and
    /// three quarters along lie nearer the first's reversed (interior points: a closed
    /// rail starts where it ends).
    ///
    /// # Errors
    ///
    /// Those of the curve copies, evaluations, reversal and `CreateRuledSurface` (`SER`);
    /// [`SolidError::InvalidInput`] for missing curves (`NER`).
    pub fn create_step_wall<O: ClassObject + SolutionObject>(
        &self,
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        offset_edgeuse: O,
        other_offset_edgeuse: O,
    ) -> Result<O> {
        let mut rails = Vec::with_capacity(2);
        for eu in [offset_edgeuse, other_offset_edgeuse] {
            let edge = brep
                .edgeuse_edge(eu)?
                .ok_or(dangling("edge use without edge"))?;
            let h = brep
                .edge_create_trimmed_nurbs_curve(k, edge)?
                .ok_or(dangling("edge without NURBS curve"))?;
            let rail = match curve_of(brep, h)?.kind() {
                CurveKind::BSpline(b) => b.clone(),
                _ => return Err(dangling("trimmed NURBS curve is not a B-spline")),
            };
            rails.push(rail);
        }
        let [mut rail1, mut rail2]: [BSplineCurve; 2] = rails
            .try_into()
            .map_err(|_| dangling("two rails expected"))?;
        for rail in [&mut rail1, &mut rail2] {
            if !rail.is_rational()? {
                continue;
            }
            let (_, first) = rail.control_point(ControlPointForm::EuclideanRational, 0)?;
            let mut equal = true;
            let count = rail.number_control_points()?;
            let mut kk = 1;
            while kk < count && equal {
                let (_, w) = rail.control_point(ControlPointForm::EuclideanRational, kk)?;
                equal = (w - first).abs() <= EFF_ZERO * first.abs();
                kk += 1;
            }
            if equal {
                make_non_rational(rail)?;
            }
        }

        // The second rail runs the way the first does when its interior points lie nearer
        // the first's than the reversed ones do.
        let ivl1 = rail1.natural_interval();
        let ivl2 = rail2.natural_interval();
        let mut same = 0.0;
        let mut reversed = 0.0;
        for kk in 1..=3 {
            let t = 0.25 * f64::from(kk);
            let p1 = rail1.evaluate_point(ivl1.evaluate(t))?;
            let p2 = rail2.evaluate_point(ivl2.evaluate(t))?;
            let p2_reversed = rail2.evaluate_point(ivl2.evaluate(1.0 - t))?;
            same += distance(p1, p2);
            reversed += distance(p1, p2_reversed);
        }
        if reversed < same {
            rail2.reverse_parameterization(&ivl2)?;
        }

        let wall = BSplineSurface::create_ruled_surface(&rail1, &rail2, SurfParamType::U)?;
        store_surface(brep, Surface::from(wall))
    }

    /// `EdgeuseOffset(ctx, exec, cpOriginalEdgeuse, cpOtherOriginalEdgeuse,
    /// cpOffsetEdgeuse, cpOtherOffsetEdgeuse, ..)`: the cap surface of an edge of the
    /// offset. A tangent edge between an offset face and a shell face whose offset edge
    /// uses leave gaps is always bridged by a cap below. Otherwise, when the executive
    /// extends convex edges, lamina and tangent edges are
    /// extended/trimmed (`ExtendTrimEdges`) and the others mated (`MateEdges`),
    /// problems only reported (not upstream: a tangent edge whose new surfaces cross in a
    /// local operation is mated, and the mate's [`OffsetFailure::TangentEndOffCurve`] is
    /// passed on). Otherwise a non-convex radial sector gets nothing
    /// (`SM_ERR` for a tangent sector with gaps between the offsets), a lamina edge
    /// nothing, an inset on a shell face mates the offset edges to the other face's
    /// surface; else the rails (the offset edges' trimmed curves, or the approximated
    /// offsets of the original UV trim curves) and the original curve (trimmed to the
    /// rails' ends) make the cap: a trimmed plane or ruled surface for a shell face, a
    /// torus for arcs, a cylinder/cone for lines, else a circular cross-section surface;
    /// rails within ten tolerances of each other only raise the offset edges' tolerances.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for a tangent sector with gaps, or rails whose intervals do not line up;
    /// `SM_ERR_NULL_POINTER` for missing curves; `SER` of the geometry calls;
    /// [`OffsetFailure::PeriodicBranchAmbiguous`] from mating is propagated;
    /// [`SolidError::Unported`] for the callees listed in the module header;
    /// [`SolidError::InvalidInput`] for null objects upstream dereferences.
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub fn edgeuse_offset<O: ClassObject + SolutionObject>(
        &self,
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        executive: &mut impl OffsetExecutive<O>,
        original_edgeuse: O,
        other_original_edgeuse: Option<O>,
        offset_edgeuse: Option<O>,
        other_offset_edgeuse: Option<O>,
        mark: MarkType,
    ) -> Result<EdgeuseOffset<O>> {
        // init output
        let mut out = EdgeuseOffset {
            surface: None,
            trimming_curves: Vec::new(),
            trim_orientations: Vec::new(),
            self_intersections: SsiData::default(),
            closing_wall: false,
        };
        let orig_edge = brep
            .edgeuse_edge(original_edgeuse)?
            .ok_or(dangling("edge use without edge"))?;

        // A tangent edge between an offset face and a shell face can't be closed by
        // extension: bridge it below.
        let shell_faces = executive.shell_faces();
        let gap_edgeuses = (other_original_edgeuse, offset_edgeuse, other_offset_edgeuse);
        let shell_tangent_gap = match gap_edgeuses {
            (Some(other_orig), Some(off), Some(other_off)) => {
                let on_shell = |face: Option<O>| face.is_some_and(|f| contains(&shell_faces, &f));
                on_shell(brep.edgeuse_face(original_edgeuse)?)
                    != on_shell(brep.edgeuse_face(other_orig)?)
                    && brep.edgeuse_is_tangent_sector(
                        k,
                        original_edgeuse,
                        self.tangency_tol_degrees(),
                    )?
                    && self.have_tangent_gaps(k, brep, 10, off, other_off)?
            }
            _ => false,
        };

        // If we are extending convex edges, extend both offset surfaces to hit each other.
        if executive.extend_convex_edges() && !shell_tangent_gap {
            // Not upstream: in a local operation, an edge whose faces and vertices' faces
            // all keep their surfaces keeps its geometry: the offset edges keep the faces'
            // own curves (owner's decision; their surfaces are not intersected again)
            if executive.keeps_edge(original_edgeuse) {
                return Ok(out);
            }
            let lamina = brep.edge_is_lamina(orig_edge)?;

            // No, we have to call ExtendTrimEdges() for Lamina Edges.  [cbi B01; B256 ok]
            let tan_tol_deg = self.tangency_tol_degrees();
            // Not upstream: in a local operation, faces tangent in the input whose new
            // surfaces cross (certified in every normal section by the reach) meet at an
            // angle: their edge is mated (on the curved face's side, `mate_edges`), not
            // extended and trimmed along its old line.
            let tan_sec = !executive.crosses_at(original_edgeuse)
                && brep.edgeuse_is_tangent_sector(k, original_edgeuse, tan_tol_deg)?;

            // Not upstream (WR-530 addendum, fork
            // tangency-labs/smlib 5d428ae): a local operation that moved only one of two
            // tangent faces along its normal leaves them parallel, one step apart: a wall
            // between their edges closes the body.
            if tan_sec && !lamina && executive.is_local_operation() {
                if let (Some(off), Some(other_off)) = (offset_edgeuse, other_offset_edgeuse) {
                    if self.is_step_across_edge(k, brep, off, other_off)? {
                        out.surface = Some(self.create_step_wall(k, brep, off, other_off)?);
                        out.closing_wall = true;
                        return Ok(out);
                    }
                }
            }

            if tan_sec || lamina {
                // Extend or Trim Lamina or Tangent Edges (a problem is only reported)
                let extended = self.extend_trim_edges(
                    k,
                    brep,
                    executive,
                    original_edgeuse,
                    other_original_edgeuse,
                    offset_edgeuse,
                    other_offset_edgeuse,
                    mark,
                );
                // Not upstream (WR-530): in a local operation, an edge whose faces no
                // longer meet along it ([`OffsetFailure::LooseTangentEdge`]) has no later
                // step that would close the gap: it fails. Any other problem is only
                // reported, as upstream's
                match extended {
                    Err(e) if is_loose_tangent_edge(&e) => return Err(e),
                    r => {
                        let _ = ignore_soft(r)?;
                    }
                }
                return Ok(out);
            }

            // Mate a couple of edges by extension and intersection (a problem is only
            // reported; not upstream: a defect, an exact intersection curve that cannot be
            // cut into its pieces and an uncertified or ambiguous retained intersection
            // branch are passed on, `mating_result`). Not upstream: an edge without both
            // offset edge uses and the other original one is not mated, as `MateEdges`
            // refused it, without asking it (its null check, an invalid argument there).
            let mated = match (other_original_edgeuse, offset_edgeuse, other_offset_edgeuse) {
                (Some(other), Some(off), Some(other_off)) => mating_result(self.mate_edges(
                    k,
                    brep,
                    executive,
                    Some(original_edgeuse),
                    Some(other),
                    Some(off),
                    Some(other_off),
                    mark,
                ))?,
                _ => None,
            };
            // Not upstream (WR-344 refinement, #1795): the merge must still intersect the
            // offsets of an edge that could not be mated
            if mated.is_none() {
                executive.note_unmated_edgeuse(original_edgeuse);
            }
            return Ok(out);
        } // end when extending convex edges

        // Make sure sector being offset is convex otherwise just skip it
        if !shell_tangent_gap
            && brep.edgeuse_radial(original_edgeuse)? == other_original_edgeuse
            && !brep.edgeuse_is_convex_radial_sector(k, original_edgeuse, 10)?
        {
            if brep.edgeuse_is_tangent_sector(k, original_edgeuse, 3.0)? {
                if let (Some(off), Some(other_off)) = (offset_edgeuse, other_offset_edgeuse) {
                    if self.have_tangent_gaps(k, brep, 10, off, other_off)? {
                        // (Figure out how to recover from this outside)
                        return Err(failed(OP_EDGEUSE, OffsetFailure::TangentGapsAtConcaveEdge));
                    }
                }
            }
            return Ok(out);
        }

        // Now take care of case where we are insetting and hit the shell face
        // we need to extend and intersect the side face.
        // Note: if cpOtherOriginalEdgeuse was passed in NULL, then there's nothing
        // else we can do in this method.  That will happen for Lamina edges.
        let Some(other_original_edgeuse) = other_original_edgeuse else {
            return Ok(out);
        };

        let orig_face = brep
            .edgeuse_face(original_edgeuse)?
            .ok_or(dangling("edge use without face"))?;
        let other_orig_face = brep
            .edgeuse_face(other_original_edgeuse)?
            .ok_or(dangling("edge use without face"))?;

        // when Orig or OtherOrig face is a ShellFace (not offset - just copied) and we are insetting
        if !shell_tangent_gap
            && (contains(&shell_faces, &orig_face) || contains(&shell_faces, &other_orig_face))
            && executive.inset()
        {
            // when OrigFace is not a shell face and we have an OffsetEdgeuse
            if let (false, Some(off)) = (contains(&shell_faces, &orig_face), offset_edgeuse) {
                let offset_edge = brep
                    .edgeuse_edge(off)?
                    .ok_or(dangling("edge use without edge"))?;
                let other_surf = brep
                    .face_surface(other_orig_face)?
                    .ok_or(dangling("face without surface"))?;
                // a problem is only reported
                let _ = ignore_soft(brep_mate_lamina_edge_to_surface(
                    k,
                    brep,
                    offset_edge,
                    other_surf,
                    1.5,
                    10.0,
                ))?;
            }

            // when OtherFace is not a shell face and we have an OtherOffsetEdgeuse
            if let (false, Some(other_off)) = (
                contains(&shell_faces, &other_orig_face),
                other_offset_edgeuse,
            ) {
                let other_offset_edge = brep
                    .edgeuse_edge(other_off)?
                    .ok_or(dangling("edge use without edge"))?;
                let surf = brep
                    .face_surface(orig_face)?
                    .ok_or(dangling("face without surface"))?;
                let _ = ignore_soft(brep_mate_lamina_edge_to_surface(
                    k,
                    brep,
                    other_offset_edge,
                    surf,
                    1.5,
                    10.0,
                ))?;
            }

            // all done - Don't need to build one if it is an inset
            return Ok(out);
        } // end inset on shell face check

        // let pCenterLine = temporary copy of OrigEU->Edge->Curve
        // (SM_CAST_PTR(SmBSplineCurve, ..), NER, then Copy: the copy keeps the curve's kind)
        // (the edge of the caller's body: a missing curve is the body as supplied, a curve
        // of another kind one the cap does not handle)
        let e_curve = brep.edge_curve(orig_edge)?.ok_or_else(|| {
            failed(
                OP_EDGEUSE,
                OffsetFailure::EdgeHasNoCurve {
                    edge: input_edge(brep, orig_edge),
                },
            )
        })?;
        let mut center_line = b_spline_kind_of(brep, e_curve)?
            .ok_or_else(|| {
                let curve_type = curve_of(brep, e_curve).map_or(0, Curve::type_code);
                failed(
                    OP_EDGEUSE,
                    OffsetFailure::CurveKindUnsupported {
                        edge: input_edge(brep, orig_edge),
                        curve_type,
                    },
                )
            })?
            .copy(k)?;

        // get OrigEU->Edge interval (snapped by the trims below)
        let mut edge_ivl = brep.edge_interval(orig_edge)?;

        // Let RailCurves = 3dCurve Copies of offsetEdge->Curves - trimmed to edge intervals
        let mut rail1: Option<BSplineCurve> = None;
        let mut rail2: Option<BSplineCurve> = None;
        if let Some(off) = offset_edgeuse {
            let off_edge = brep
                .edgeuse_edge(off)?
                .ok_or(dangling("edge use without edge"))?;
            if let Some(h) = brep.edge_create_trimmed_nurbs_curve(k, off_edge)? {
                rail1 = b_spline_of(brep, h)?;
            }
        }
        if let Some(other_off) = other_offset_edgeuse {
            let off_edge = brep
                .edgeuse_edge(other_off)?
                .ok_or(dangling("edge use without edge"))?;
            if let Some(h) = brep.edge_create_trimmed_nurbs_curve(k, off_edge)? {
                rail2 = b_spline_of(brep, h)?;
            }
        }
        if shell_tangent_gap {
            if let (Some(off), Some(other_off)) = (offset_edgeuse, other_offset_edgeuse) {
                let cl_pv = center_line.evaluate(k, edge_ivl.evaluate(0.5), 1, true, true)?;
                let cl_tan = cl_pv
                    .get(1)
                    .copied()
                    .ok_or(dangling("centre line tangent"))?;
                self.reconcile_shell_tangent_rails(
                    k,
                    brep,
                    executive,
                    [
                        ShellTangentInput {
                            orig_eu: original_edgeuse,
                            off_eu: off,
                            rail: &mut rail1,
                        },
                        ShellTangentInput {
                            orig_eu: other_original_edgeuse,
                            off_eu: other_off,
                            rail: &mut rail2,
                        },
                    ],
                    cl_tan,
                )?;
            }
        }
        if rail1.is_none() {
            // make temporary copy of the OrigEU->UVTrimCurve trimmed to sEdgeIvl
            // (UV trim curves are optional on a valid body, and nothing made this one)
            let uv = brep
                .edgeuse_uv_trim_curve(original_edgeuse)?
                .ok_or_else(|| {
                    failed(
                        OP_EDGEUSE,
                        OffsetFailure::EdgeuseHasNoUvTrim {
                            edge: input_edge(brep, orig_edge),
                        },
                    )
                })?;
            let mut uv_copy = curve_of(brep, uv)?.copy(k)?;
            uv_copy.trim(k, &mut edge_ivl, TrimMode::Notify)?; // may snap sIvl by tol to existing knots

            // make an offset surface from origEU->Face->Surface
            let surface = brep
                .face_surface(orig_face)?
                .ok_or_else(|| face_without_surface(brep, orig_face))?;
            let fu = brep
                .edgeuse_faceuse(original_edgeuse)?
                .ok_or(dangling("edge use without face use"))?;
            let sign = if brep.faceuse_orientation(fu)? == OrientType::Same {
                1.0
            } else {
                -1.0
            };

            // let pRail1Curve = approximation of the 3dCurve on the offset surface
            rail1 = Some(offset_surface_rail(
                k,
                brep,
                surface,
                uv_copy,
                sign * self.offset_distance,
                false,
                rail2.as_ref(),
                self.this_approx_tol_3d,
                ApproximateCurveFlags {
                    create_analytics: true,
                    match_parameterization: false,
                    just_copy_b_splines: false,
                },
            )?);
        } // end not given an OffsetEdgeuse check

        // When not given an OtherOffsetEdgeuse - make a Rail2Curve by approximating OtherOrigEU->UVTrimCurve Offset
        if rail2.is_none() {
            let uv = brep
                .edgeuse_uv_trim_curve(other_original_edgeuse)?
                .ok_or_else(|| {
                    failed(
                        OP_EDGEUSE,
                        OffsetFailure::EdgeuseHasNoUvTrim {
                            edge: input_edge(brep, orig_edge),
                        },
                    )
                })?;
            let mut uv_copy = curve_of(brep, uv)?.copy(k)?;
            uv_copy.trim(k, &mut edge_ivl, TrimMode::Notify)?;

            let surface = brep
                .face_surface(other_orig_face)?
                .ok_or_else(|| face_without_surface(brep, other_orig_face))?;
            let fu = brep
                .edgeuse_faceuse(other_original_edgeuse)?
                .ok_or(dangling("edge use without face use"))?;
            let sign = if brep.faceuse_orientation(fu)? == OrientType::Same {
                1.0
            } else {
                -1.0
            };

            rail2 = Some(offset_surface_rail(
                k,
                brep,
                surface,
                uv_copy,
                sign * self.offset_distance,
                false,
                Some(rail1.as_ref().ok_or(dangling("rail 1"))?),
                self.this_approx_tol_3d,
                ApproximateCurveFlags {
                    create_analytics: true,
                    match_parameterization: false,
                    just_copy_b_splines: false,
                },
            )?);
        } // end not given an OtherOffsetEdgeuse check

        // Make sure intervals of rail curves are the same
        let mut rail1 = rail1.ok_or(dangling("rail 1"))?;
        let mut rail2 = rail2.ok_or(dangling("rail 2"))?;
        let ivl1 = rail1.natural_interval();
        let ivl2 = rail2.natural_interval();
        if !ivl1.is_value_on_boundary(ivl2.min(), EFF_ZERO).0
            || !ivl1.is_value_on_boundary(ivl2.max(), EFF_ZERO).0
        {
            // Non syncronized edgeuses
            return Err(failed(
                OP_EDGEUSE,
                OffsetFailure::RailIntervalsDiffer {
                    first: [ivl1.min(), ivl1.max()],
                    second: [ivl2.min(), ivl2.max()],
                },
            ));
        }

        // get Rail1Curve/Rail2Curve max distance
        let max_dist = Curve::from(rail1.clone())
            .curve_max_distance_between(
                k,
                &rail1.natural_interval(),
                &Curve::from(rail2.clone()),
                rail2.natural_interval().min(),
                rail2.natural_interval().max(),
                30,
                None,
            )?
            .distance;

        // When railCurves are within 10*Tolerance of each other
        let e_tol = brep.edge_tolerance(orig_edge)?;
        if max_dist < e_tol * 10.0 {
            // don't build a capSurface - just increase railCurve tolerances and return
            if let Some(off) = offset_edgeuse {
                let e = brep
                    .edgeuse_edge(off)?
                    .ok_or(dangling("edge use without edge"))?;
                brep.set_edge_tolerance(k, e, max_dist * 2.0)?;
            }
            if let Some(other_off) = other_offset_edgeuse {
                let e = brep
                    .edgeuse_edge(other_off)?
                    .ok_or(dangling("edge use without edge"))?;
                brep.set_edge_tolerance(k, e, max_dist * 2.0)?;
            }
            return Ok(out);
        }

        // regularize the railCurve knot vectors by inserting knots into large spans
        if rail1.degree()? == 3 {
            rail1.dampen_knot_spacing(2.0, 3)?;
        }
        if rail2.degree()? == 3 {
            rail2.dampen_knot_spacing(2.0, 3)?;
        }

        // The offset edgeuses must be syncronized but may only be a subset of
        // the original edge.  Therefore we do a trim just to make sure.

        // get Rail1Curve StartPoint
        let rail1_ivl = rail1.natural_interval();
        let mut pnt1 = rail1.evaluate_point(rail1_ivl.min())?;

        // get CenterLine StartPoint
        let mut center_ivl = center_line.natural_interval();
        let mut pnt2 = center_line.evaluate_point(k, center_ivl.min())?;

        // When StartPoints Gap != OffsetDistance to within tolerance
        if (distance(pnt1, pnt2) - self.offset_distance).abs() > self.this_approx_tol_3d {
            // drop Rail1Curve StartPoint to CenterLine (SmBSplineCurve::Evaluate: no
            // non-zero tangent substitution)
            let pv = rail1.evaluate(rail1_ivl.min(), 1, true, false)?;
            let p0 = pv.first().copied().ok_or(dangling("evaluation"))?;
            let p1 = pv.get(1).copied().ok_or(dangling("evaluation"))?;
            let dropped = center_line.drop_point(
                k,
                &center_ivl,
                p0,
                Some(p1),
                1.1 * self.offset_distance,
                None,
                SolverOperation::Intersect,
            )?;
            // when Rail1Curve StartPoint dropped to CenterLine
            if dropped.success {
                // Trim CenterLine to Start dropPoint (both statuses ignored upstream and
                // passed on here, WR-658; the trim may snap sCenterIvl by tol to existing
                // knots)
                center_ivl.set_min_max(dropped.parameter, center_ivl.max())?;
                center_line.trim(k, &mut center_ivl, TrimMode::Notify)?;
            }
        } // end Rail1/CenterLine StartPoint gapSize check

        // get Rail1 and CenterLine EndPoints
        pnt1 = rail1.evaluate_point(rail1_ivl.max())?;
        pnt2 = center_line.evaluate_point(k, center_ivl.max())?;

        // When EndPoints Gap != OffsetDistance to within tolerance
        if (distance(pnt1, pnt2) - self.offset_distance).abs() > self.this_approx_tol_3d {
            // drop Rail1Curve EndPoint to CenterLine
            let pv = rail1.evaluate(rail1_ivl.max(), 1, true, false)?;
            let p0 = pv.first().copied().ok_or(dangling("evaluation"))?;
            let p1 = -pv.get(1).copied().ok_or(dangling("evaluation"))?; // "inward-pointing vector".

            // Call DropPoint() with INTERSECT, and pass a maximum allowable distance.
            let max_drop_dist = 1.1 * self.offset_distance;

            let dropped = center_line.drop_point(
                k,
                &center_ivl,
                p0,
                Some(p1),
                max_drop_dist,
                None,
                SolverOperation::Intersect,
            )?;
            // when Rail1Curve EndPoint dropped to CenterLine
            if dropped.success {
                // Trim CenterLine to End dropPoint
                center_ivl.set_min_max(center_ivl.min(), dropped.parameter)?;
                center_line.trim(k, &mut center_ivl, TrimMode::Notify)?;
            }
        } // end Rail1/CenterLine EndPoint gapSize check

        // when either railCurve is degree 3 or higher
        if rail1.degree()? > 2 || rail2.degree()? > 2 {
            // make rail1/rail2/centerLine curves compatible i.e. share a common knotVector
            // (the static routine edits the NURB part of the centre line, whatever its kind)
            if shell_tangent_gap {
                if let Some(mut made_cl) = b_spline_with_nurb(k, &center_line)? {
                    made_cl.edit_parameterization(&rail1.natural_interval())?;
                    if let Some(cl_slot) = b_spline_part_mut(&mut center_line)? {
                        *cl_slot = made_cl;
                    }
                }
            }
            let cl_part =
                b_spline_part_mut(&mut center_line)?.ok_or(dangling("centre line kind"))?;
            let mut curves = [rail1, rail2, cl_part.clone()];
            BSplineCurve::make_curves_compatible(&mut curves, EFF_ZERO_PARAM)?;
            let [r1, r2, cl] = curves;
            rail1 = r1;
            rail2 = r2;
            *cl_part = cl;
        }

        // railCurve's have been manicured - make a surface between them
        let mut new_surface: Option<BSplineSurface> = None;

        // First handle case of one face being shelling face and just
        // create a linear blend between the rails or extend the surface.
        if contains(&shell_faces, &orig_face) || contains(&shell_faces, &other_orig_face) {
            // Now that we have the curves create the geometry for the surface.
            match BSplineSurface::create_trimmed_plane_from_ruled(k, &rail1, &rail2) {
                Ok(trimmed) => {
                    for c in trimmed.trimming_curves {
                        out.trimming_curves.push(store_curve(brep, c)?);
                    }
                    out.trim_orientations = trimmed.trim_orientations;
                    new_surface = Some(trimmed.plane);
                }
                Err(e @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => {
                    return Err(e)
                }
                // any other failure is upstream's `!= SM_SUCCESS` (e.g. the `SM_ERR` of
                // `Unitize` on the zero cross product of two collinear-derivative rails):
                // the ruled surface between the rails instead
                Err(_) => {
                    new_surface = Some(BSplineSurface::create_ruled_surface(
                        &rail1,
                        &rail2,
                        SurfParamType::U,
                    )?);
                    if shell_tangent_gap {
                        out.closing_wall = true;
                    }
                }
            }
        } // end one face is a shell face check

        // First do a quick check for the self intersecting torus situation.
        if new_surface.is_none() {
            if let Some((ref_frame, major_radius, start_angle, end_angle)) =
                center_line.is_arc(k, 10, self.this_approx_tol_3d)?
            {
                // (`&&`: the second IsArc runs only when the first succeeds)
                let rails = match is_arc(k, &rail1, 10, self.this_approx_tol_3d)? {
                    Some((_, radius_r1, _, _)) => is_arc(k, &rail2, 10, self.this_approx_tol_3d)?
                        .map(|(_, radius_r2, _, _)| (radius_r1, radius_r2)),
                    None => None,
                };
                if let Some((radius_r1, radius_r2)) = rails {
                    // Have a torus situation
                    let cl_point = center_line
                        .evaluate_point(k, center_line.natural_interval().evaluate(0.0))?;
                    let mut r1_point =
                        rail1.evaluate_point(rail1.natural_interval().evaluate(0.0))?;
                    let mut r2_point =
                        rail2.evaluate_point(rail2.natural_interval().evaluate(0.0))?;
                    let rad1 = distance(cl_point, r1_point);
                    let rad2 = distance(cl_point, r2_point);
                    if (rad1 - rad2).abs() < EFF_ZERO * (1.0 + r1_point.max_dimension()) {
                        let minor_radius = (rad1 + rad2) / 2.0;

                        // Swap and start arc so that it corresponds to the curve which is
                        // the same radius as the center line.
                        if (major_radius - radius_r2).abs() < (major_radius - radius_r1).abs() {
                            core::mem::swap(&mut r1_point, &mut r2_point);
                        }
                        let mut arc = BSplineCurve::create_arc_from_points(
                            cl_point,
                            r1_point,
                            r2_point,
                            NurbCircleParam::Quadratic,
                        )?;

                        // Make a ray going in the right direction
                        let ray_vector = ref_frame.z_axis();
                        let ray_point =
                            ref_frame.origin() - (minor_radius + major_radius) * ray_vector;

                        if major_radius - minor_radius < -EFF_ZERO {
                            // Self intersecting torus case - create a new point and a new circle
                            // (a copy of the arc goes to the store so that the solutions
                            // name an object, as upstream's name pArc)
                            let mut arc_ivl = arc.natural_interval();
                            let arc_h = store_curve(brep, Curve::from(arc.clone()))?;
                            let rays = [ray_vector];
                            // SmTol::GetZoneTol3d(pArc): the arc has no owner, so its
                            // curve branch gives the system zone tolerance
                            let zone_tol = |o: O| zone_tol_3d_of_curve(k, curve_of(brep, o)?);
                            let sols: SolutionArray<O> = curve_of(brep, arc_h)?
                                .global_point_solve(
                                    k,
                                    &arc_ivl,
                                    SolverOperation::Rayfire,
                                    ray_point,
                                    self.this_approx_tol_3d,
                                    SolutionRequested::Single,
                                    &GlobalPointSolveOptions {
                                        target_distance: None,
                                        opt_vectors: Some(&rays),
                                        handle: Some(arc_h),
                                        analytic_handle: None,
                                        zone_tol_3d: Some(ObjectZoneTol(&zone_tol)),
                                    },
                                )?;
                            if let Some(sol) = sols.get(0) {
                                let t = sol.start.param(0)?;
                                if (t - arc_ivl.min()).abs() > EFF_ZERO_SQRT {
                                    arc_ivl.set_min_max(arc_ivl.min(), t)?;
                                    arc.trim(k, &mut arc_ivl, TrimMode::Notify)?;
                                }
                            }
                        }
                        // Now Create torus
                        new_surface = Some(BSplineSurface::create_surf_of_revolution(
                            k,
                            &arc,
                            ref_frame.origin(),
                            ref_frame.z_axis(),
                            end_angle - start_angle,
                            false,
                            None,
                        )?);
                    }
                }
            }
        } // end quick check for self intersecting torus case

        // Check for case of cone or cylinder
        if new_surface.is_none() {
            if let Some((_line_pt, line_vec)) =
                center_line.is_line(k, 10, self.this_approx_tol_3d)?
            {
                if rail1.is_line(10, self.this_approx_tol_3d)?.is_some()
                    && rail2.is_line(10, self.this_approx_tol_3d)?.is_some()
                {
                    let cl_point = center_line
                        .evaluate_point(k, center_line.natural_interval().evaluate(0.0))?;
                    let r1_point = rail1.evaluate_point(rail1.natural_interval().evaluate(0.0))?;
                    let r2_point = rail2.evaluate_point(rail2.natural_interval().evaluate(0.0))?;
                    let rad1 = distance(cl_point, r1_point);
                    let rad2 = distance(cl_point, r2_point);
                    if (rad1 - rad2).abs() < EFF_ZERO * (1.0 + r1_point.max_dimension()) {
                        let mut arc = BSplineCurve::create_arc_from_points(
                            cl_point,
                            r2_point,
                            r1_point,
                            NurbCircleParam::Quadratic,
                        )?;
                        if is_arc(k, &arc, 10, self.this_approx_tol_3d)?.is_some() {
                            new_surface =
                                Some(BSplineSurface::create_linear_sweep(&mut arc, line_vec)?);
                        }
                    }
                }
            }
        } // end cone or cylinder case

        // when case is not already handled - build a surface to fill the curves with circular cross section
        let new_surface = match new_surface {
            Some(s) => s,
            None => circular_cross_section_surface(
                k,
                // SmCircularCrossSectionFSG sFSG(FALSE): the other arguments' defaults
                CircularCrossSectionFsg {
                    approximate_arcs: false,
                    ..CircularCrossSectionFsg::default()
                },
                &mut center_line,
                &mut rail1,
                &mut rail2,
                self.this_approx_tol_3d,
                false,
            )?,
        };

        // when working with analytics - turn NewSurface into Analytic when possible
        let final_surface = if self.make_analytics {
            // Copy pNewSurface, when possible as an analytic surface
            Surface::from(new_surface).copy_and_add_analytics(k)?
        } else {
            Surface::from(new_surface)
        };
        out.surface = Some(store_surface(brep, final_surface)?);

        // (the self-intersection block is #if 0)

        // all done
        Ok(out)
    }

    /// Inspects one offset side of a deferred shell-tangent gap, returning `Ok(None)` when the
    /// edge has unsettled vertices or degenerates within its zone/edge tolerance (which acts
    /// as a no-op in `reconcile_shell_tangent_rails`, leaving the original `edgeuse_offset`
    /// curve and step-wall rail intact), or its endpoint match status and certified endpoint
    /// margins (`tau_e + tau_v` floored by the pre-`Fix_TolSizes` meeting tolerance).
    fn prepare_shell_tangent_side<O: ClassObject + SolutionObject>(
        &self,
        k: &Kernel,
        brep: &impl OffsetBrep<O>,
        orig_eu: O,
        off_eu: O,
    ) -> Result<Option<ShellTangentPrepared<O>>> {
        let edge = brep.edgeuse_edge(off_eu)?.ok_or(dangling("off edge"))?;
        let v1 = brep.edgeuse_vertex(off_eu)?;
        let mate = brep.edgeuse_mate(off_eu)?;
        let v2 = mate.map(|m| brep.edgeuse_vertex(m)).transpose()?.flatten();
        let p1 = v1.map(|v| brep.vertex_point(v)).transpose()?;
        let p2 = v2.map(|v| brep.vertex_point(v)).transpose()?;
        let (Some(p1), Some(p2)) = (p1, p2) else {
            return Ok(None);
        };
        // `docs/validity-and-tolerances.md` §4 (Edge test 7) & §5.1: an edge is at least
        // its tube radius `tau_e` and two points within the body's zone tolerance are one
        // point.
        let tol = brep.edge_tolerance(edge)?;
        let min_len = tol.max(brep.brep_tolerance(edge)?);
        if (p1 - p2).length() <= min_len {
            return Ok(None);
        }
        let tv1 = v1
            .map(|v| brep.vertex_tolerance(v))
            .transpose()?
            .unwrap_or(0.0)
            .max(tol);
        let tv2 = v2
            .map(|v| brep.vertex_tolerance(v))
            .transpose()?
            .unwrap_or(0.0)
            .max(tol);
        // `docs/validity-and-tolerances.md` §4 (Edge tests 16, 17) & §5.1:
        // `|C(t_end) - P_v| <= tau_e + tau_v`, floored by the pre-`Fix_TolSizes`
        // offset edge meeting tolerance `10 * max(tau_e, this_approx_tol_3d)`
        // (`extend_trim_edges` / `is_step_across_edge`).
        let meet_tol = 10.0 * max2(tol, self.this_approx_tol_3d);
        let margin1 = (tol + tv1).max(meet_tol);
        let margin2 = (tol + tv2).max(meet_tol);
        let matched = if let Some(ch) = brep.edge_curve(edge)? {
            let cv = curve_of(brep, ch)?;
            let ivl = brep.edge_interval(edge)?;
            let e0 = cv.evaluate_point(k, ivl.min())?;
            let e1 = cv.evaluate_point(k, ivl.max())?;
            ((p1 - e0).length() <= margin1 && (p2 - e1).length() <= margin2)
                || ((p1 - e1).length() <= margin1 && (p2 - e0).length() <= margin2)
        } else {
            false
        };
        Ok(Some(ShellTangentPrepared {
            orig_eu,
            off_eu,
            edge,
            v1,
            v2,
            p1,
            p2,
            tol,
            matched,
            margin1,
            margin2,
        }))
    }

    /// Validates a pre-existing matched rail's interval against its edge's stored interval,
    /// or materializes the rail when the edge's curve is analytic without a pre-built NURBS
    /// slot.
    fn ensure_matched_shell_tangent_rail<O: ClassObject>(
        k: &Kernel,
        brep: &impl OffsetBrep<O>,
        side: &ShellTangentPrepared<O>,
        rail: &mut Option<BSplineCurve>,
    ) -> Result<()> {
        if let Some(r) = rail.as_ref() {
            let e_ivl = brep.edge_interval(side.edge)?;
            let r_ivl = r.natural_interval();
            if r_ivl.is_init()
                || r_ivl.length() <= EFF_ZERO_PARAM
                || !r_ivl.is_value_on_boundary(e_ivl.min(), EFF_ZERO).0
                || !r_ivl.is_value_on_boundary(e_ivl.max(), EFF_ZERO).0
            {
                return Err(defect(OP_EDGEUSE, MATCHED_RAIL_OFF_EDGE, SM_ERR));
            }
        } else {
            let (_, r, _, _) = Self::derive_shell_tangent_curve_and_rail(
                k,
                brep,
                side.edge,
                side.p1,
                side.p2,
                side.margin1,
                side.margin2,
            )?;
            *rail = Some(r);
        }
        Ok(())
    }

    /// Rebuilds the 3D curve, certified UV trim, and step-wall rail for an unmatched side of
    /// a shell-tangent gap.
    fn rebuild_shell_tangent_side<O: ClassObject + SolutionObject>(
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        executive: &mut impl OffsetExecutive<O>,
        side: &ShellTangentPrepared<O>,
    ) -> Result<ShellTangentRebuilt> {
        let (curve, rail, start, end) = Self::derive_shell_tangent_curve_and_rail(
            k,
            brep,
            side.edge,
            side.p1,
            side.p2,
            side.margin1,
            side.margin2,
        )?;
        let uv =
            Self::drop_curve_onto_extended_face(k, brep, executive, side.off_eu, &curve, side.tol)?;
        Ok(ShellTangentRebuilt {
            curve,
            uv,
            rail,
            start,
            end,
        })
    }

    /// Normalizes both step-wall rails onto `[0, 1]` oriented along `cl_tan`.
    fn normalize_shell_tangent_rails(
        rail1: &mut Option<BSplineCurve>,
        rail2: &mut Option<BSplineCurve>,
        cl_tan: Vec3,
    ) -> Result<()> {
        if let (Some(r1), Some(r2)) = (rail1.as_mut(), rail2.as_mut()) {
            for r in [r1, r2] {
                r.edit_parameterization(&Extent1d::from_min_max(0.0, 1.0))?;
                let r_pv = r.evaluate(0.5, 1, true, true)?;
                let r_tan = r_pv.get(1).copied().ok_or(dangling("rail tangent"))?;
                if r_tan.dot(cl_tan) < 0.0 {
                    let nat = r.natural_interval();
                    r.reverse_parameterization(&nat)?;
                    r.edit_parameterization(&Extent1d::from_min_max(0.0, 1.0))?;
                }
            }
        }
        Ok(())
    }

    /// Reconciles the boundary curves, UV trims, and step-wall rails across a deferred
    /// shell-tangent gap (`shell_tangent_gap`), replacing only an offset edge whose
    /// curve does not already span its settled vertices within the certified endpoint
    /// bound `tau_e + tau_v` (`docs/validity-and-tolerances.md` §4 Edge 16–17, §5.1).
    fn reconcile_shell_tangent_rails<O: ClassObject + SolutionObject>(
        &self,
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        executive: &mut impl OffsetExecutive<O>,
        [in1, in2]: [ShellTangentInput<'_, O>; 2],
        cl_tan: Vec3,
    ) -> Result<()> {
        let Some(s1) = self.prepare_shell_tangent_side(k, brep, in1.orig_eu, in1.off_eu)? else {
            return Ok(());
        };
        let Some(s2) = self.prepare_shell_tangent_side(k, brep, in2.orig_eu, in2.off_eu)? else {
            return Ok(());
        };
        if s1.matched && s2.matched {
            return Ok(());
        }
        if s1.matched {
            Self::ensure_matched_shell_tangent_rail(k, brep, &s1, in1.rail)?;
        }
        if s2.matched {
            Self::ensure_matched_shell_tangent_rail(k, brep, &s2, in2.rail)?;
        }

        let work1 = if s1.matched {
            None
        } else {
            Some(Self::rebuild_shell_tangent_side(k, brep, executive, &s1)?)
        };
        let work2 = if s2.matched {
            None
        } else {
            Some(Self::rebuild_shell_tangent_side(k, brep, executive, &s2)?)
        };

        // Before `replace_edge_curves` updates any shared vertex position, verify that
        // every other incident edge at each moved vertex remains within `tau_inc + tau_v`.
        let partner_for_1 = work2.as_ref().map(|w| s2.as_partner(w));
        let partner_for_2 = work1.as_ref().map(|w| s1.as_partner(w));
        for (side, work, partner) in [
            (&s1, work1.as_ref(), partner_for_1),
            (&s2, work2.as_ref(), partner_for_2),
        ] {
            if let Some(w) = work {
                for (v, pt) in [(side.v1, w.start), (side.v2, w.end)] {
                    Self::verify_vertex_move_on_incident_edges(
                        k, brep, v, pt, side.edge, side.tol, partner,
                    )?;
                }
            }
        }

        for (side, work, rail_out) in [(&s1, work1, &mut *in1.rail), (&s2, work2, &mut *in2.rail)] {
            if let Some(w) = work {
                let h_c = store_curve(brep, w.curve)?;
                let h_uv = store_curve(brep, w.uv)?;
                self.replace_edge_curves(
                    k,
                    brep,
                    side.off_eu,
                    Some(h_c),
                    Some(h_uv),
                    Some(side.orig_eu),
                )?;
                *rail_out = Some(w.rail);
            }
        }

        Self::normalize_shell_tangent_rails(in1.rail, in2.rail, cl_tan)
    }

    /// Derives a trimmed 3D curve and `[0, 1]` B-spline rail between `q1` and `q2` from
    /// `e`'s boundary curve, certifying endpoint and interior deviations against
    /// `(margin1, margin2)` directly from control nets without point sampling.
    fn derive_shell_tangent_curve_and_rail<O: ClassObject>(
        k: &Kernel,
        brep: &impl OffsetBrep<O>,
        e: O,
        q1: Point3,
        q2: Point3,
        margin1: f64,
        margin2: f64,
    ) -> Result<(Curve, BSplineCurve, Point3, Point3)> {
        let Some(ch) = brep.edge_curve(e)? else {
            let line = BSplineCurve::create_line_segment(3, q1, q2)?;
            let mut r = line.clone();
            r.edit_parameterization(&Extent1d::from_min_max(0.0, 1.0))?;
            return Ok((Curve::from(line), r, q1, q2));
        };
        let cv = curve_of(brep, ch)?;
        let uncertified = || failed(OP_EDGEUSE, OffsetFailure::ShellTangentCurveUncertified);
        if let Some((pt, dir, eps_line)) = exact_collinear_line(cv) {
            let dir_u = dir.unitize_or_z();
            let s1 = pt + (q1 - pt).dot(dir_u) * dir_u;
            let s2 = pt + (q2 - pt).dot(dir_u) * dir_u;
            let d1 = (q1 - s1).length();
            let d2 = (q2 - s2).length();
            if d1 + eps_line > margin1 || d2 + eps_line > margin2 || (s1 - s2).length() <= EFF_ZERO
            {
                return Err(uncertified());
            }
            let line = BSplineCurve::create_line_segment(3, s1, s2)?;
            let mut r = line.clone();
            r.edit_parameterization(&Extent1d::from_min_max(0.0, 1.0))?;
            return Ok((Curve::from(line), r, s1, s2));
        }

        let nat = cv.natural_interval();
        let dp1 = cv.drop_point(k, &nat, q1, None, margin1, None, SolverOperation::Minimize)?;
        let dp2 = cv.drop_point(k, &nat, q2, None, margin2, None, SolverOperation::Minimize)?;
        if !dp1.success
            || !dp2.success
            || dp1.distance > margin1
            || dp2.distance > margin2
            || (dp1.parameter - dp2.parameter).abs() <= EFF_ZERO_PARAM
        {
            return Err(uncertified());
        }
        let t1 = dp1.parameter;
        let t2 = dp2.parameter;
        let mut trimmed_cv = cv.copy(k)?;
        let mut ivl = Extent1d::from_min_max(t1.min(t2), t1.max(t2));
        trimmed_cv.trim(k, &mut ivl, TrimMode::Notify)?;
        if t1 > t2 {
            trimmed_cv.reverse_parameterization(k, &ivl)?;
        }
        match trimmed_cv.kind_mut() {
            CurveKind::Ellipse(e) if e.base().gw_nurb().is_none() => e.make_nurb(k)?,
            CurveKind::Hyperbola(h) if h.base().gw_nurb().is_none() => h.make_nurb(None)?,
            CurveKind::Parabola(p) if p.base().gw_nurb().is_none() => p.make_nurb(None)?,
            _ => {}
        }
        let trimmed_ivl = trimmed_cv.natural_interval();
        let c_start = trimmed_cv.evaluate_point(k, trimmed_ivl.min())?;
        let c_end = trimmed_cv.evaluate_point(k, trimmed_ivl.max())?;
        if (c_start - q1).length() > margin1 || (c_end - q2).length() > margin2 {
            return Err(uncertified());
        }
        // (an offset edge's curve, of a kind with no B-spline form)
        let src_bs = b_spline_with_nurb(k, &trimmed_cv)?.ok_or_else(|| {
            Failure::new(
                OP_EDGEUSE,
                OffsetFailure::CurveKindUnsupported {
                    edge: None,
                    curve_type: trimmed_cv.type_code(),
                },
            )
            .with_status(SM_ERR)
            .into_error()
        })?;
        let mut b_spline = src_bs.clone();
        b_spline.edit_parameterization(&Extent1d::from_min_max(0.0, 1.0))?;
        if !is_exact_unit_reparameterization(&src_bs, &b_spline) {
            return Err(uncertified());
        }
        Ok((trimmed_cv, b_spline, c_start, c_end))
    }

    /// Projects `c3` onto the extended surface of `eu`'s owning face within `tol`
    /// (`executive.get_create_extended_surface` + `Surface::drop_curve`), requiring a
    /// certified 3D bound `<= tol` on `||ext_s(uvc(t)) - c3(t)||` over `c3`'s full interval
    /// ([`crate::surface::surface::exact_pieces::certified_lift`]).
    fn drop_curve_onto_extended_face<O: ClassObject + SolutionObject>(
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        executive: &mut impl OffsetExecutive<O>,
        eu: O,
        c3: &Curve,
        tol: f64,
    ) -> Result<Curve> {
        if !(tol.is_finite() && tol > 0.0) {
            return Err(SolidError::Unsupported {
                operation: OP_EDGEUSE,
                reason: "shell-tangent UV lift requires a positive finite tolerance",
            });
        }
        let face = brep.edgeuse_face(eu)?.ok_or(dangling("face"))?;
        let surf_h = brep.face_surface(face)?.ok_or(dangling("surf"))?;
        let (ext_surf_h, ext_dom) = executive.get_create_extended_surface(k, brep, surf_h)?;
        let raw_ext_s = surface_of(brep, ext_surf_h)?;
        if crate::surface::surface::exact_pieces::surface_uses_approximate_net(raw_ext_s) {
            return Err(SolidError::Unsupported {
                operation: OP_EDGEUSE,
                reason: crate::surface::surface::exact_pieces::APPROXIMATE_NET_REASON,
            });
        }
        let ext_s = raw_ext_s.clone_for_evaluation()?;
        let c3_ivl = c3.natural_interval();
        let dropped =
            ext_s.drop_curve::<NoObject>(k, &ext_dom, c3, &c3_ivl, tol, false, None, true)?;
        let drop_dev = dropped.max_drop_to_surf.hypot(dropped.max_approx_dev);
        if dropped.uv_curves.is_empty() || !drop_dev.is_finite() || drop_dev > tol {
            return Err(SolidError::Unsupported {
                operation: OP_EDGEUSE,
                reason: "a UV curve's lift is farther than the tolerance from its 3D curve",
            });
        }
        let unmatched = || failed(OP_EDGEUSE, OffsetFailure::ShellTangentUvUnmatched);
        let uvc = match <[Curve; 1]>::try_from(dropped.uv_curves) {
            Ok([candidate]) => {
                let uv_ivl = candidate.natural_interval();
                if !uv_ivl.is_value_on_boundary(c3_ivl.min(), EFF_ZERO).0
                    || !uv_ivl.is_value_on_boundary(c3_ivl.max(), EFF_ZERO).0
                {
                    return Err(unmatched());
                }
                candidate
            }
            Err(uv_curves) => {
                // When extending a bounded surface (e.g. a half-cylinder) closes it into a
                // periodic surface, an edge along that periodic seam drops to two
                // full-interval UV seam branches (`u_min` and `u_max`, or `v_min` and
                // `v_max`). Select the full-interval branch on `eu`'s side of the face using
                // its existing UV trim curve (`extend_trim_edges`,
                // `extended_sides_keeping_parameters`).
                let edge = brep.edgeuse_edge(eu)?.ok_or(dangling("edge"))?;
                // (an offset edge use, whose edge names none of the input's)
                let orig_uv_h = brep.edgeuse_uv_trim_curve(eu)?.ok_or_else(|| {
                    Failure::new(OP_EDGEUSE, OffsetFailure::EdgeuseHasNoUvTrim { edge: None })
                        .with_status(SM_ERR)
                        .into_error()
                })?;
                let orig_mid = curve_of(brep, orig_uv_h)?
                    .evaluate_point(k, brep.edge_interval(edge)?.evaluate(0.5))?;
                let mut best: Option<Curve> = None;
                let mut min_dist = BIG_DOUBLE;
                for cand in uv_curves {
                    let uv_ivl = cand.natural_interval();
                    if !uv_ivl.is_value_on_boundary(c3_ivl.min(), EFF_ZERO).0
                        || !uv_ivl.is_value_on_boundary(c3_ivl.max(), EFF_ZERO).0
                    {
                        continue;
                    }
                    let sols: SolutionArray<NoObject> = cand.global_point_solve(
                        k,
                        &uv_ivl,
                        SolverOperation::Minimize,
                        orig_mid,
                        EFF_ZERO_SQRT,
                        SolutionRequested::Single,
                        &GlobalPointSolveOptions::default(),
                    )?;
                    let nearer = sols
                        .get(0)
                        .map(|sol| sol.start.solution_value)
                        .filter(|&value| value < min_dist);
                    if let Some(value) = nearer {
                        min_dist = value;
                        best = Some(cand);
                    }
                }
                best.ok_or_else(unmatched)?
            }
        };
        crate::surface::surface::exact_pieces::certified_lift(k, &ext_s, &uvc, c3, c3_ivl, tol)?;
        Ok(uvc)
    }

    /// Verifies that moving `opt_v` to `moved_pt` keeps every other incident edge's
    /// endpoint at `opt_v` within `tau_inc + tau_v` (`AssertValid` Edge tests 16–17,
    /// with `tau_v >= max(tau_self, tau_inc)` after `Fix_TolSizes`,
    /// `docs/validity-and-tolerances.md` §5.1).
    pub(crate) fn verify_vertex_move_on_incident_edges<O: ClassObject>(
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        opt_v: Option<O>,
        moved_pt: Point3,
        self_edge: O,
        self_tol: f64,
        partner_rebuilt: Option<RebuiltPartnerEdge<O>>,
    ) -> Result<()> {
        let Some(v) = opt_v else {
            return Ok(());
        };
        let too_far = |moved, bound| {
            failed(
                OP_EDGEUSE,
                OffsetFailure::ShellTangentVertexMoveTooFar { moved, bound },
            )
        };
        let cur_pt = brep.vertex_point(v)?;
        let raw_v_tol = brep.vertex_tolerance(v)?;
        for inc_e in brep.vertex_edges(v)? {
            if inc_e == self_edge {
                continue;
            }
            if let Some(p) = partner_rebuilt {
                if inc_e == p.edge {
                    let eff_v_tol = raw_v_tol.max(self_tol).max(p.tol);
                    let bound = p.tol + eff_v_tol;
                    for (end_vertex, end) in [(p.v1, p.start), (p.v2, p.end)] {
                        let moved = (moved_pt - end).length();
                        if end_vertex == Some(v) && moved > bound {
                            return Err(too_far(moved, bound));
                        }
                    }
                    continue;
                }
            }
            if (moved_pt - cur_pt).length() <= EFF_ZERO || brep.edge_curve(inc_e)?.is_none() {
                continue;
            }
            let inc_tol = brep.edge_tolerance(inc_e)?;
            let eff_v_tol = raw_v_tol.max(self_tol).max(inc_tol);
            let bound = inc_tol + eff_v_tol;
            for inc_eu in brep.edge_edgeuses(inc_e)? {
                if brep.edgeuse_vertex(inc_eu)? == Some(v) {
                    let end_pt =
                        edgeuse_normalized_evaluate(k, brep, inc_eu, 0.0, false, false, false)?;
                    let moved = (moved_pt - end_pt).length();
                    if (cur_pt - end_pt).length() <= bound && moved > bound {
                        return Err(too_far(moved, bound));
                    }
                }
            }
        }
        Ok(())
    }
}

/// Input edgeuses and rail slot for one side of `reconcile_shell_tangent_rails`.
struct ShellTangentInput<'a, O> {
    orig_eu: O,
    off_eu: O,
    rail: &'a mut Option<BSplineCurve>,
}

/// Settled endpoint data and endpoint-match margins for one side of a shell-tangent gap.
struct ShellTangentPrepared<O> {
    orig_eu: O,
    off_eu: O,
    edge: O,
    v1: Option<O>,
    v2: Option<O>,
    p1: Point3,
    p2: Point3,
    tol: f64,
    matched: bool,
    margin1: f64,
    margin2: f64,
}

impl<O: Copy> ShellTangentPrepared<O> {
    fn as_partner(&self, rebuilt: &ShellTangentRebuilt) -> RebuiltPartnerEdge<O> {
        RebuiltPartnerEdge {
            edge: self.edge,
            tol: self.tol,
            v1: self.v1,
            start: rebuilt.start,
            v2: self.v2,
            end: rebuilt.end,
        }
    }
}

/// Rebuilt 3D curve, certified UV trim, and step-wall rail for one side of a shell-tangent gap.
struct ShellTangentRebuilt {
    curve: Curve,
    uv: Curve,
    rail: BSplineCurve,
    start: Point3,
    end: Point3,
}

/// Candidate rebuilt partner rail endpoints checked before mutating shared vertices.
#[derive(Clone, Copy)]
pub(crate) struct RebuiltPartnerEdge<O> {
    edge: O,
    tol: f64,
    v1: Option<O>,
    start: Point3,
    v2: Option<O>,
    end: Point3,
}

/// Returns `(origin, direction, eps_line)` when `cv` is an exact line (`eps_line = 0`) or a
/// 3D positive-weight B-spline whose control polygon is collinear to machine precision,
/// bounding `sup_t dist(cv(t), line(origin, direction)) <= eps_line` in outward-rounded [`Ivl`]
/// arithmetic via NURBS convex-hull containment and convexity of Euclidean point-to-line
/// distance. Curved conics and non-collinear B-splines return `None`.
fn exact_collinear_line(cv: &Curve) -> Option<(Point3, Vec3, f64)> {
    use crate::blend::fillet_section::series::{v_sub, V3};

    if cv.dim() != 3 {
        return None;
    }
    match cv.kind() {
        CurveKind::Line(l) => {
            let (pt, dir) = l.is_line();
            (dir.length() > EFF_ZERO).then_some((pt, dir, 0.0))
        }
        CurveKind::BSpline(bs) => {
            let nurb = bs.gw_nurb()?;
            let cpts = nurb.cpts();
            if cpts.len() < 2 {
                return None;
            }
            let rational = nurb.is_rational();
            let to_ivl_pt = |cp: &crate::nurbs::math::CPoint| -> Option<V3<Ivl>> {
                if rational {
                    if !(cp.w.is_finite() && cp.w > 0.0) {
                        return None;
                    }
                    let inv_w = Ivl::from(1.0) / Ivl::from(cp.w);
                    Some(V3 {
                        x: Ivl::from(cp.x) * inv_w,
                        y: Ivl::from(cp.y) * inv_w,
                        z: Ivl::from(cp.z) * inv_w,
                    })
                } else {
                    Some(V3 {
                        x: Ivl::from(cp.x),
                        y: Ivl::from(cp.y),
                        z: Ivl::from(cp.z),
                    })
                }
            };
            let p0 = to_ivl_pt(cpts.first()?)?;
            let pn = to_ivl_pt(cpts.last()?)?;
            let d_ivl = v_sub(pn, p0);
            let d_len = (d_ivl.x * d_ivl.x + d_ivl.y * d_ivl.y + d_ivl.z * d_ivl.z).sqrt();
            if !(d_len.lo.is_finite() && d_len.lo > EFF_ZERO) {
                return None;
            }
            let scale = 1.0 + p0.x.mag().max(p0.y.mag()).max(p0.z.mag()).max(d_len.hi);
            let mut eps_line = 0.0_f64;
            for cp in cpts.iter().skip(1).take(cpts.len().saturating_sub(2)) {
                let pi = to_ivl_pt(cp)?;
                let rel = v_sub(pi, p0);
                let cr = V3 {
                    x: rel.y * d_ivl.z - rel.z * d_ivl.y,
                    y: rel.z * d_ivl.x - rel.x * d_ivl.z,
                    z: rel.x * d_ivl.y - rel.y * d_ivl.x,
                };
                let cr_norm_hi = (cr.x * cr.x + cr.y * cr.y + cr.z * cr.z).sqrt().hi;
                let dist_hi = (Ivl::from(cr_norm_hi) / d_len).hi;
                if !dist_hi.is_finite() {
                    return None;
                }
                eps_line = eps_line.max(dist_hi);
            }
            if eps_line > EFF_ZERO * scale {
                return None;
            }
            let pt0 = cpts.first()?.to_euclid();
            let ptn = cpts.last()?.to_euclid();
            let dir = ptn - pt0;
            if dir.length() <= EFF_ZERO {
                return None;
            }
            Some((pt0, dir, eps_line + 2.0 * f64::EPSILON * scale))
        }
        _ => None,
    }
}

/// Verifies directly on the NURBS nets that `unit` on `[0, 1]` is an affine knot
/// reparameterization of `src` with identical degree, rationality, and homogeneous control
/// points (so both curves trace the exact same 3D locus by affine invariance of B-spline basis
/// functions, without point sampling).
fn is_exact_unit_reparameterization(src: &BSplineCurve, unit: &BSplineCurve) -> bool {
    use crate::blend::fillet_section::interval::Ivl;

    let (Some(n1), Some(n2)) = (src.gw_nurb(), unit.gw_nurb()) else {
        return false;
    };
    if n1.degree() != n2.degree()
        || n1.is_rational() != n2.is_rational()
        || n1.cpts().len() != n2.cpts().len()
    {
        return false;
    }
    for (c1, c2) in n1.cpts().iter().zip(n2.cpts().iter()) {
        if c1.x.to_bits() != c2.x.to_bits()
            || c1.y.to_bits() != c2.y.to_bits()
            || c1.z.to_bits() != c2.z.to_bits()
            || c1.w.to_bits() != c2.w.to_bits()
        {
            return false;
        }
    }
    let (u1, u2) = (n1.knots(), n2.knots());
    let Some(m) = u1.len().checked_sub(1) else {
        return false;
    };
    let p = n1.degree();
    if u1.len() != u2.len() || 2_usize.saturating_mul(p).saturating_add(1) > u1.len() {
        return false;
    }
    if u2.windows(2).any(|w| match (w.first(), w.get(1)) {
        (Some(&a), Some(&b)) => !(a.is_finite() && b.is_finite() && a <= b),
        _ => true,
    }) {
        return false;
    }
    let (Some(&first), Some(&last), Some(&u2_first), Some(&u2_last)) =
        (u1.first(), u1.last(), u2.first(), u2.last())
    else {
        return false;
    };
    if u1
        .iter()
        .zip(u2.iter())
        .all(|(k1, k2)| k1.to_bits() == k2.to_bits())
    {
        return u2_first.abs() <= EFF_ZERO && (u2_last - 1.0).abs() <= EFF_ZERO;
    }
    let span = Ivl::from(last) - Ivl::from(first);
    if !(span.lo.is_finite() && span.lo > EFF_ZERO_PARAM) {
        return false;
    }
    for (idx, (&k1, &k2)) in u1.iter().zip(u2.iter()).enumerate() {
        if idx <= p {
            if k2 != 0.0 {
                return false;
            }
        } else if idx >= m.saturating_sub(p) {
            if k2 != 1.0 {
                return false;
            }
        } else {
            let ideal = (Ivl::from(k1) - Ivl::from(first)) / span;
            let pad = 4.0 * f64::EPSILON;
            if k2 < ideal.lo - pad || k2 > ideal.hi + pad {
                return false;
            }
        }
    }
    true
}

// ---- the local-operation variant --------------------------------------------------------------

/// The geometry creator of local operations (`SmLocalOperationGeometryCreation`):
/// the offset surfaces are given per face.
#[derive(Debug, Clone, PartialEq)]
pub struct LocalOperationGeometryCreation<O> {
    /// The base object (its `m_bMakeAnalytics` is `FALSE`).
    pub base: OffsetGeometryCreation,
    /// `m_vFaces`.
    pub faces: Vec<O>,
    /// `m_vSurfaces`: the surface replacing each face's.
    pub surfaces: Vec<O>,
}

impl<O: Copy + PartialEq> LocalOperationGeometryCreation<O> {
    /// The creator over the owned-geometry face adapter ([`FaceOffsetBrep`]) of the
    /// executive: [`LocalOperationGeometryCreation::new`] without the handle bounds.
    pub(crate) fn from_handles(
        offset_distance: f64,
        this_approx_tol_3d: f64,
        faces: Vec<O>,
        surfaces: Vec<O>,
    ) -> Self {
        let mut base = OffsetGeometryCreation::new(offset_distance, this_approx_tol_3d);
        base.make_analytics = false;
        Self {
            base,
            faces,
            surfaces,
        }
    }

    /// [`LocalOperationGeometryCreation::faceuse_offset`] over the face adapter: the
    /// surface listed for the face use's face, as it is (`m_bMakeAnalytics` is `FALSE`
    /// from this constructor on, and its only caller, the executive's `build_local`, asks
    /// at once, so the reparameterisation and analytic copy are never reached).
    pub(crate) fn faceuse_offset_with_backend(
        &self,
        brep: &mut impl FaceOffsetBrep<O>,
        original_faceuse: O,
    ) -> Result<(Option<O>, SsiData<O>)> {
        let face = brep
            .faceuse_face(original_faceuse)?
            .ok_or(dangling("face use without face"))?;
        let Some(index) = self.faces.iter().position(|f| *f == face) else {
            return Err(failed(OP_LOCAL, OffsetFailure::LocalFaceNotListed));
        };
        let new_surface = self
            .surfaces
            .get(index)
            .copied()
            .ok_or(dangling("surface list shorter than the face list"))?;
        Ok((Some(new_surface), SsiData::default()))
    }
}

impl<O: ClassObject + SolutionObject> LocalOperationGeometryCreation<O> {
    /// `SmLocalOperationGeometryCreation(dOffsetDistance, dThisApproxTol3d, rFaces,
    /// rSurfaces)`.
    #[must_use]
    pub fn new(offset_distance: f64, this_approx_tol_3d: f64, faces: &[O], surfaces: &[O]) -> Self {
        let mut base = OffsetGeometryCreation::new(offset_distance, this_approx_tol_3d);
        base.make_analytics = false;
        Self {
            base,
            faces: faces.to_vec(),
            surfaces: surfaces.to_vec(),
        }
    }

    /// `SmLocalOperationGeometryCreation::FaceuseOffset`: the surface listed for the
    /// face use's face (reparameterised to the face surface's natural domain and
    /// copied as an analytic surface when `m_bMakeAnalytics`, which the constructor
    /// clears). `skip_self_intersection` is `NotUsed`; the self-intersections are
    /// always empty.
    ///
    /// # Errors
    ///
    /// `SM_ERR` when the face is not listed; `SER` of the analytic copy;
    /// [`SolidError::InvalidInput`] for a face use without face, a face without
    /// surface or a listed face without surface (upstream dereferences them).
    pub fn faceuse_offset(
        &self,
        k: &Kernel,
        brep: &mut impl OffsetBrep<O>,
        original_faceuse: O,
        skip_self_intersection: bool,
    ) -> Result<(Option<O>, SsiData<O>)> {
        let _ = skip_self_intersection;
        // init output
        let self_intersections = SsiData::default();

        // locals
        let face = brep
            .faceuse_face(original_faceuse)?
            .ok_or(dangling("face use without face"))?;

        let Some(index) = self.faces.iter().position(|f| *f == face) else {
            return Err(failed(OP_LOCAL, OffsetFailure::LocalFaceNotListed));
        };

        let mut new_surface = self
            .surfaces
            .get(index)
            .copied()
            .ok_or(dangling("surface list shorter than the face list"))?;

        if self.base.make_analytics {
            // (pFace->GetSurface() is dereferenced only here)
            let sur = brep
                .face_surface(face)?
                .ok_or(dangling("face without surface"))?;
            let old_domain = surface_of(brep, sur)?.natural_uv_domain_with_nurb(k)?;
            // Reparameterize's status is ignored upstream; passed on here (WR-658)
            {
                let s = brep
                    .surface_mut(new_surface)
                    .ok_or(dangling("surface handle cannot be resolved"))?;
                s.reparameterize(k, &old_domain)?;
            }
            // Copy pS, when possible as an analytic surface (the listed surface is deleted)
            let mut analytic = surface_of(brep, new_surface)?.copy_and_add_analytics(k)?;
            if let SurfaceKind::OffsetSurface(offset) = analytic.kind_mut() {
                // SM_CAST_PTR(SmOffsetSurface, pAnalyticS)
                offset.set_allow_extension(false);
            }
            new_surface = store_surface(brep, analytic)?;
        }

        Ok((Some(new_surface), self_intersections))
    }
}

/// Where a UV curve piece lies against a domain, each point within the kernel's UV
/// point tolerance `EFF_ZERO_SQRT * (1 + |uv|)` ([`uv_piece_in_domain`]).
#[derive(Debug, Clone, Copy, PartialEq)]
enum PieceInDomain {
    /// Every point of the piece is inside, certified by control-point hulls.
    Inside,
    /// A point of the piece is outside, at this distance from the domain.
    Outside(f64),
    /// Neither was settled within [`PIECE_SPLIT_DEPTH`] and [`PIECE_SEGMENT_BUDGET`].
    Undecided,
}

/// The deepest halving of one Bezier segment by [`uv_piece_in_domain`].
const PIECE_SPLIT_DEPTH: u32 = 40;
/// The most Bezier segments [`uv_piece_in_domain`] looks at.
const PIECE_SEGMENT_BUDGET: usize = 4096;

/// Whether every point of `piece` is inside `domain`, each within the UV point
/// tolerance `EFF_ZERO_SQRT * (1 + |uv|)` (#21743). The check is not upstream.
///
/// Each Bezier segment of the curve is checked in turn:
/// - **Witness.** Its end points are points of the curve. One that is outside its
///   tolerance proves the piece leaves the domain.
/// - **Certificate.** With positive weights, the box of its Euclidean control points
///   holds the segment. Every point `q` of that box has `|q| >= m`, where `m` is the
///   box's least max-norm, so its tolerance is at least `t = EFF_ZERO_SQRT * (1 + m)`.
///   A box inside the domain grown by `t` therefore certifies the whole segment.
/// - **Otherwise** the segment is halved by de Casteljau and both halves are checked.
///
/// The work is a loop over an explicit stack, left to right, bounded by
/// [`PIECE_SPLIT_DEPTH`] and [`PIECE_SEGMENT_BUDGET`].
fn uv_piece_in_domain(piece: &BSplineCurve, domain: &Extent2d) -> Result<PieceInDomain> {
    let nurb = piece.nurb_ref()?;
    let p = nurb.degree();
    let mut stack: Vec<(Vec<crate::nurbs::math::CPoint>, u32)> = Vec::new();
    for seg in nurb.decompose_bez()?.into_iter().rev() {
        stack.push((seg.cpts().to_vec(), 0));
    }
    let point_tol = |q: Vec3| EFF_ZERO_SQRT * (1.0 + q.max_dimension());
    // the least |x| over [a, b]
    let nearest = |a: f64, b: f64| {
        if a > 0.0 {
            a
        } else if b < 0.0 {
            -b
        } else {
            0.0
        }
    };
    let (lo_d, hi_d) = (domain.min(), domain.max());
    let mut visited = 0usize;
    let mut undecided = false;
    while let Some((pw, depth)) = stack.pop() {
        visited += 1;
        for q in [pw.first(), pw.last()].into_iter().flatten() {
            let q = q.to_euclid();
            let (inside, outside) = domain.contains_point_dist(Point2::new(q.x, q.y), point_tol(q));
            if !inside {
                return Ok(PieceInDomain::Outside(outside));
            }
        }
        if pw.iter().any(|c| c.w != NOW && c.w <= 0.0) {
            // (no hull property without positive weights)
            undecided = true;
            continue;
        }
        let (mut lo, mut hi) = (
            Point2::new(f64::INFINITY, f64::INFINITY),
            Point2::new(f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for c in &pw {
            let e = c.to_euclid();
            lo = Point2::new(lo.x.min(e.x), lo.y.min(e.y));
            hi = Point2::new(hi.x.max(e.x), hi.y.max(e.y));
        }
        let t = EFF_ZERO_SQRT * (1.0 + nearest(lo.x, hi.x).max(nearest(lo.y, hi.y)));
        if lo.x >= lo_d.x - t && hi.x <= hi_d.x + t && lo.y >= lo_d.y - t && hi.y <= hi_d.y + t {
            continue;
        }
        if depth >= PIECE_SPLIT_DEPTH || visited >= PIECE_SEGMENT_BUDGET {
            undecided = true;
            continue;
        }
        let (left, right) = crate::nurbs::bezier::split(&pw, p, 0.5)?;
        stack.push((right, depth + 1));
        stack.push((left, depth + 1));
    }
    Ok(if undecided {
        PieceInDomain::Undecided
    } else {
        PieceInDomain::Inside
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::math::CPoint;

    /// The quadratic `UV(t) = (t, -d - (t - 1/2)^2)` on `[0, 1]`, as a Bezier curve.
    fn dipping_uv_piece(d: f64) -> BSplineCurve {
        let pw = [
            CPoint::new(0.0, -d - 0.25, NOZ, NOW),
            CPoint::new(0.5, -d + 0.25, NOZ, NOW),
            CPoint::new(1.0, -d - 0.25, NOZ, NOW),
        ];
        BSplineCurve::from_data(2, 2, 5, 2, &pw, &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0])
            .expect("a quadratic")
    }

    /// #21744 review (CodeRabbit): the piece `(t, -d - (t - 1/2)^2)`, `d = EFF_ZERO_SQRT /
    /// 4`, stays below the domain `[0, 1]^2` and never crosses its boundary, so no crossing
    /// cuts it, and its midpoint `(1/2, -d)` is inside within the UV point tolerance. Its
    /// ends `(0, -d - 1/4)` and `(1, -d - 1/4)` are `d + 1/4` outside, and the whole-piece
    /// test finds one of them. Without it the midpoint test alone kept the piece.
    #[test]
    fn a_piece_outside_with_its_midpoint_within_tolerance_is_refused() {
        let d = EFF_ZERO_SQRT / 4.0;
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let piece = dipping_uv_piece(d);
        let mid = piece.evaluate_point(0.5).expect("the midpoint");
        let (inside, _) = domain.contains_point_dist(
            Point2::new(mid.x, mid.y),
            EFF_ZERO_SQRT * (1.0 + mid.max_dimension()),
        );
        assert!(inside, "the midpoint {mid:?} passes the midpoint test");
        assert_eq!(
            uv_piece_in_domain(&piece, &domain).expect("checked"),
            PieceInDomain::Outside(d + 0.25)
        );
    }

    /// The same parabola raised wholly into the domain (`d = -1/2`: `v` in `[1/4, 1/2]`)
    /// is certified inside. So is one whose control polygon leaves the domain while the
    /// curve does not: `d = -1/4 - 1e-3` puts the curve's `v` in `[1e-3, 1/4 + 1e-3]` and
    /// its middle control point at `v = 1/2 + 1e-3`, above a domain topped at `0.3`; one
    /// halving brings each half's control points to `v <= 1/4 + 1e-3`.
    #[test]
    fn a_piece_inside_is_certified_inside() {
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        assert_eq!(
            uv_piece_in_domain(&dipping_uv_piece(-0.5), &domain).expect("checked"),
            PieceInDomain::Inside
        );
        let low_top = Extent2d::from_bounds(0.0, 0.0, 1.0, 0.3);
        assert_eq!(
            uv_piece_in_domain(&dipping_uv_piece(-0.25 - 1e-3), &low_top).expect("checked"),
            PieceInDomain::Inside
        );
    }

    /// [`remove_joint_copies`] with no cap: the bound alone.
    fn remove_exact_joint_knot(curve: &mut BSplineCurve, at: f64) -> Result<f64> {
        Ok(remove_joint_copies(curve, at, usize::MAX)?.1)
    }

    #[test]
    fn periodic_branch_ambiguity_is_not_an_unmated_edge() {
        let error = mating_result::<()>(Err(SolidError::offset(
            OP_MATE,
            OffsetFailure::PeriodicBranchAmbiguous,
        )))
        .unwrap_err();
        let failure = error.failure().unwrap();
        assert_eq!(failure.operation, OP_MATE);
        assert_eq!(failure.kind(), crate::error::ErrorKind::Unsupported);
        assert!(!error.is_defect());
        assert_eq!(
            failure.reason,
            Reason::Offset(OffsetFailure::PeriodicBranchAmbiguous)
        );
        assert_eq!(
            mating_result::<()>(Err(SolidError::Unsupported {
                operation: OP_MATE,
                reason: "ordinary trim miss",
            }))
            .unwrap(),
            None
        );
        assert_eq!(
            mating_result::<()>(Err(failed(OP_MATE, OffsetFailure::ExtendedEdgeUntrimmed)))
                .unwrap(),
            None
        );
        // a defect is passed on, never read as an edge that does not mate
        let defect =
            mating_result::<()>(Err(defect(OP_MATE, SSI_UV_CURVE_KIND, SM_ERR))).unwrap_err();
        assert!(defect.is_defect());
        assert_eq!(defect.upstream_status(), Some(SM_ERR));
        assert_eq!(mating_result(Ok(())).unwrap(), Some(()));
    }

    /// The cubic of the joint-knot tests (simple knot `1`, ends `0` and `2.5`), moved by
    /// `offset` along every axis, cut at `cut` and joined back: the joint knot `cut` at
    /// full multiplicity. With `kink`, the joint's control point (the one the curve passes
    /// through there) is moved by `kink` along `y`, which makes the joint a corner.
    fn rejoined_cubic(offset: f64, cut: f64, kink: f64) -> (BSplineCurve, f64) {
        let k = Kernel::default();
        let o = offset;
        let pw = [
            CPoint::new(o, o, o, NOW),
            CPoint::new(1.0 + o, 2.0 + o, 0.5 + o, NOW),
            CPoint::new(3.0 + o, 3.0 + o, -1.0 + o, NOW),
            CPoint::new(4.0 + o, 1.0 + o, 2.0 + o, NOW),
            CPoint::new(6.0 + o, o, 1.0 + o, NOW),
        ];
        let u = [0.0, 0.0, 0.0, 0.0, 1.0, 2.5, 2.5, 2.5, 2.5];
        let whole = BSplineCurve::from_data(3, 4, 8, 3, &pw, &u).unwrap();
        let mut head = Curve::from(whole.clone_exact());
        let tail = head.create_by_split_curve(&k, cut).unwrap();
        let piece = |c: &Curve| match c.kind() {
            CurveKind::BSpline(b) => b.clone_exact(),
            _ => panic!("not a B-spline"),
        };
        let mut segments = [piece(&head), piece(&tail)];
        let (mut joined, breaks) =
            BSplineCurve::create_by_joining_with_breaks(&mut segments, Some(&[true, true]))
                .unwrap();
        let at = breaks[1];
        assert_eq!(at, cut);
        if kink != 0.0 {
            let on = joined.evaluate_point(at).unwrap();
            let i = (0..joined.number_control_points().unwrap())
                .find(|&i| {
                    let (p, _) = joined
                        .control_point(ControlPointForm::EuclideanRational, i)
                        .unwrap();
                    p.distance(on) <= 1e-9 * (1.0 + offset.abs())
                })
                .expect("the curve passes through a control point at its joint");
            let (p, _) = joined
                .control_point(ControlPointForm::EuclideanRational, i)
                .unwrap();
            let moved = Point3::new(p.x, p.y + kink, p.z);
            joined
                .set_control_point(ControlPointForm::NonRational, i, moved, 1.0)
                .unwrap();
        }
        (joined, at)
    }

    /// A 3D curve and its two UV curves keep one knot vector at the joint
    /// (`docs/ssi.md` section 8, invariant 5): when one of them turns there (kink 1e-6),
    /// all three keep every copy of the knot, though the other two alone would lose them;
    /// when all three are smooth, all three lose the copies, and the 3D curve's bound is
    /// returned.
    #[test]
    fn a_joint_knot_is_kept_or_dropped_in_all_three_curves_together() {
        let kept = rejoined_cubic(0.0, 1.7, 0.0).0.knots(None).unwrap();
        assert_eq!(kept, (vec![0.0, 1.0, 1.7, 2.5], vec![4, 1, 3, 4]));
        let (smooth, at) = rejoined_cubic(0.0, 1.7, 0.0);
        let (kinked, at_kinked) = rejoined_cubic(0.0, 1.7, 1e-6);
        assert_eq!(at, at_kinked);
        for turning in 0..3 {
            let mut joined: [(BSplineCurve, f64); 3] = core::array::from_fn(|i| {
                if i == turning {
                    (kinked.clone_exact(), at)
                } else {
                    (smooth.clone_exact(), at)
                }
            });
            assert_eq!(remove_exact_joint_knot_together(&mut joined).unwrap(), 0.0);
            for (curve, _) in &joined {
                assert_eq!(curve.knots(None).unwrap(), kept, "{turning}");
            }
        }
        let mut joined: [(BSplineCurve, f64); 3] =
            core::array::from_fn(|_| (smooth.clone_exact(), at));
        let bound = remove_exact_joint_knot_together(&mut joined).unwrap();
        assert!(bound > 0.0, "{bound}");
        for (curve, _) in &joined {
            assert_eq!(
                curve.knots(None).unwrap(),
                (vec![0.0, 1.0, 2.5], vec![4, 1, 4])
            );
        }
    }

    /// When one of the three curves can lose only some copies of the joint knot, all three
    /// lose exactly that many: a cubic that already had a simple knot at the cut loses 2
    /// of its 3 copies, the rejoined cubic without one would lose all 3, and all three end
    /// with one copy left. The 3D curve's bound is its two copies' tolerance, rounded up.
    #[test]
    fn a_joint_knot_is_reduced_in_all_three_curves_to_the_fewest_removable() {
        let k = Kernel::default();
        let pw = [
            CPoint::new(0.0, 0.0, 0.0, NOW),
            CPoint::new(1.0, 2.0, 0.5, NOW),
            CPoint::new(2.0, 3.0, -0.5, NOW),
            CPoint::new(3.0, 3.0, -1.0, NOW),
            CPoint::new(4.0, 1.0, 2.0, NOW),
            CPoint::new(6.0, 0.0, 1.0, NOW),
        ];
        let u = [0.0, 0.0, 0.0, 0.0, 1.0, 1.7, 2.5, 2.5, 2.5, 2.5];
        let whole = BSplineCurve::from_data(3, 5, 9, 3, &pw, &u).unwrap();
        let mut head = Curve::from(whole.clone_exact());
        let tail = head.create_by_split_curve(&k, 1.7).unwrap();
        let piece = |c: &Curve| match c.kind() {
            CurveKind::BSpline(b) => b.clone_exact(),
            _ => panic!("not a B-spline"),
        };
        let mut segments = [piece(&head), piece(&tail)];
        let (had_knot, breaks) =
            BSplineCurve::create_by_joining_with_breaks(&mut segments, Some(&[true, true]))
                .unwrap();
        assert_eq!(breaks[1], 1.7);
        let joined_at = (vec![0.0, 1.0, 1.7, 2.5], vec![4, 1, 3, 4]);
        assert_eq!(had_knot.knots(None).unwrap(), joined_at);
        let mut alone = had_knot.clone_exact();
        assert_eq!(
            remove_joint_copies(&mut alone, 1.7, usize::MAX).unwrap().0,
            2
        );
        let (smooth, at) = rejoined_cubic(0.0, 1.7, 0.0);
        assert_eq!(smooth.knots(None).unwrap(), joined_at);
        let tol = joint_knot_tolerance(&had_knot).unwrap();
        for first in 0..3 {
            let mut joined: [(BSplineCurve, f64); 3] = core::array::from_fn(|i| {
                if i == first {
                    (had_knot.clone_exact(), at)
                } else {
                    (smooth.clone_exact(), at)
                }
            });
            let bound = remove_exact_joint_knot_together(&mut joined).unwrap();
            if first == 0 {
                assert_eq!(bound, (Ivl::from(2.0) * Ivl::from(tol)).hi);
            }
            for (curve, _) in &joined {
                assert_eq!(
                    curve.knots(None).unwrap(),
                    (vec![0.0, 1.0, 1.7, 2.5], vec![4, 1, 1, 4]),
                    "{first}"
                );
            }
        }
    }

    /// The joint-knot decision depends on the curve's shape, not on where it lies. The
    /// cubic rejoined at 1.7, moved 1000 out along every axis, loses its knot as at the
    /// origin, within the same bound. With a kink of 1e-10 at the joint it keeps its knot,
    /// at the origin and moved out alike, though a tolerance scaled by the largest
    /// coordinate (1e-9 out there) would take the kink's knot out.
    #[test]
    fn a_joint_far_from_the_origin_is_decided_as_at_the_origin() {
        // the curve's extent is 6 wherever it lies: tolerance EFF_ZERO (1 + 6)
        let tol = 7.0 * EFF_ZERO * (1.0 + 1e-12);
        for offset in [0.0, 1000.0] {
            let (mut smooth, at) = rejoined_cubic(offset, 1.7, 0.0);
            let whole = rejoined_cubic(offset, 1.7, 0.0).0;
            let bound = remove_exact_joint_knot(&mut smooth, at).unwrap();
            assert!(bound > 0.0 && bound <= 3.0 * tol, "{offset}: {bound}");
            assert_eq!(
                smooth.knots(None).unwrap(),
                (vec![0.0, 1.0, 2.5], vec![4, 1, 4])
            );
            for j in 0..=25 {
                let t = 0.1 * f64::from(j);
                let a = whole.evaluate_point(t).unwrap();
                let b = smooth.evaluate_point(t).unwrap();
                assert!(a.distance(b) <= bound, "{offset} {t}: {a:?} {b:?}");
            }

            let (mut kinked, at) = rejoined_cubic(offset, 1.7, 1e-10);
            let before = kinked.knots(None).unwrap();
            assert_eq!(
                remove_exact_joint_knot(&mut kinked, at).unwrap(),
                0.0,
                "{offset}"
            );
            assert_eq!(kinked.knots(None).unwrap(), before);
        }
        // a tolerance from the largest coordinate, 1e-12 (1 + 1002) out there, takes the
        // kink's knot out
        let (mut kinked, at) = rejoined_cubic(1000.0, 1.7, 1e-10);
        let by_coordinate = (Ivl::from(EFF_ZERO) * (Ivl::from(1.0) + Ivl::from(1002.0))).hi;
        assert!(kinked.remove_knot_certified(at, 3, by_coordinate).unwrap() > 0);
    }

    /// A vertex move with a NaN distance or a NaN tolerance does not fit.
    #[test]
    fn a_vertex_move_with_a_nan_does_not_fit() {
        assert!(move_within(1e-6, 1e-5));
        assert!(move_within(1e-5, 1e-5));
        assert!(!move_within(2e-5, 1e-5));
        assert!(!move_within(f64::NAN, 1e-5));
        assert!(!move_within(1e-6, f64::NAN));
        assert!(!move_within(f64::NAN, f64::NAN));
        assert_eq!(smallest_tolerance(&[1e-5, 2e-6, 1e-4]), Some(2e-6));
        assert_eq!(smallest_tolerance(&[f64::NAN, 1e-5]), None);
        assert_eq!(smallest_tolerance(&[1e-5, f64::NAN]), None);
        assert_eq!(smallest_tolerance(&[]), None);
    }

    /// A quarter circle of radius 18 continued ten of its lengths each way, as
    /// `ExtendTrimEdges` continues an edge's UV trim curve, runs round its circle several
    /// times: its two ends and its middle are each passed more than once, every passage of
    /// one of them the same point. Whichever passage of the middle the edge's middle drops
    /// onto, the two ends are taken on that turn, however their passages are listed: the
    /// curve between them is the quarter (it stays in the quarter's quadrant and passes
    /// the middle once), not the other three quarters nor more than a turn. A solution at
    /// another point stays a candidate, and nothing is dropped where the nearest the base
    /// vertex is such a point or the curve misses the image.
    #[test]
    fn a_continued_arcs_ends_are_taken_on_one_turn() {
        let k = Kernel::default();
        let quarter = BSplineCurve::create_circle_segment(
            3,
            &Axis2Placement::default(),
            18.0,
            0.0,
            90.0,
            NurbCircleParam::Quadratic,
        )
        .unwrap();
        let own = quarter.natural_interval();
        let reach = 10.0 * own.length();
        let continued = quarter
            .create_extended_curve(own.min() - reach, ContinuityType::G1G2, true)
            .unwrap()
            .unwrap()
            .create_extended_curve(own.max() + reach, ContinuityType::G1G2, true)
            .unwrap()
            .unwrap();
        let curve = Curve::from(continued);
        let whole = curve.natural_interval();
        assert!(whole.min() < own.min() - 9.0 * own.length());
        assert!(whole.max() > own.max() + 9.0 * own.length());
        let at = |t: f64| curve.evaluate_point(&k, t).unwrap();
        // the curve's passages of a point: where its distance to it, sampled 64 times a
        // quarter's length, has a minimum, narrowed to the point itself
        let passages_of = |point: Point3| {
            let off = |t: f64| distance(at(t), point);
            let step = own.length() / 64.0;
            let mut passages = Vec::new();
            let mut t = whole.min() + step;
            while t + step <= whole.max() {
                if off(t) < off(t - step) && off(t) <= off(t + step) {
                    let (mut low, mut high) = (t - step, t + step);
                    for _ in 0..80 {
                        let third = (high - low) / 3.0;
                        if off(low + third) < off(high - third) {
                            high -= third;
                        } else {
                            low += third;
                        }
                    }
                    let t = 0.5 * (low + high);
                    if off(t) < 1e-6 {
                        passages.push(t);
                    }
                }
                t += step;
            }
            passages
        };
        let (start, end) = (Point3::new(18.0, 0.0, 0.0), Point3::new(0.0, 18.0, 0.0));
        let half = 18.0 * std::f64::consts::FRAC_1_SQRT_2;
        let middle = Point3::new(half, half, 0.0);
        let (starts, ends, middles) = (passages_of(start), passages_of(end), passages_of(middle));
        assert!(starts.len() > 1 && ends.len() > 1 && middles.len() > 1);
        // the base vertices: the ends moved 2 off the curve's plane, as an offset leaves them
        let lift = Vec3::new(0.0, 0.0, 2.0);
        let tol = 1e-5;
        for &on in &middles {
            let mut trims = [0.0; 2];
            for (trim, (point, passages)) in trims.iter_mut().zip([(start, &starts), (end, &ends)])
            {
                let kept = passages_on_one_turn(&k, &curve, passages, point, point + lift, on, tol)
                    .unwrap();
                assert_eq!(kept.len(), 1, "{passages:?}");
                *trim = passages[kept[0]];
                // listed the other way round, the same passage
                let reversed: Vec<f64> = passages.iter().rev().copied().collect();
                let kept =
                    passages_on_one_turn(&k, &curve, &reversed, point, point + lift, on, tol)
                        .unwrap();
                assert_eq!(kept.len(), 1);
                assert_eq!(reversed[kept[0]], *trim);
            }
            let [from, to] = trims;
            assert!(distance(at(from), start) < 1e-6 && distance(at(to), end) < 1e-6);
            assert!(from < on && on < to, "{from} {on} {to}");
            let between = middles.iter().filter(|&&m| from < m && m < to).count();
            assert_eq!(between, 1, "{from} to {to} passes the middle once");
            for i in 0..=64 {
                let point = at(from + (to - from) * f64::from(i) / 64.0);
                assert!(point.x > -1e-6 && point.y > -1e-6, "{point:?}");
            }
        }
        // the edge's middle stands for the passage farthest from the curve's ends, the one
        // on the quarter itself (the curve reaches as far either way), however they are
        // listed; its base point is the middle moved 2 off the plane
        let own_middle = middle_passage(&k, &curve, &middles, middle + lift, tol)
            .unwrap()
            .unwrap();
        assert!(
            own.min() < own_middle && own_middle < own.max(),
            "{own_middle}"
        );
        assert!(distance(at(own_middle), middle) < 1e-6);
        let reversed: Vec<f64> = middles.iter().rev().copied().collect();
        assert_eq!(
            middle_passage(&k, &curve, &reversed, middle + lift, tol).unwrap(),
            Some(own_middle)
        );
        // a drop elsewhere is no passage of the nearest one's point
        let mut drops = vec![own.evaluate(0.9)];
        drops.extend(&middles);
        assert_eq!(
            middle_passage(&k, &curve, &drops, middle + lift, tol).unwrap(),
            Some(own_middle)
        );
        assert_eq!(middle_passage(&k, &curve, &[], middle, tol).unwrap(), None);
        // a solution at another point (a third of the way along the quarter) stays, beside
        // the one passage of the image left
        let mut mixed = starts.clone();
        mixed.push(own.evaluate(1.0 / 3.0));
        let other = mixed.len() - 1;
        let kept =
            passages_on_one_turn(&k, &curve, &mixed, start, start + lift, middles[0], tol).unwrap();
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[1], other);
        // where the nearest the base vertex is that other point, the search takes it
        // whatever is kept: everything stays
        let kept =
            passages_on_one_turn(&k, &curve, &mixed, start, at(mixed[other]) + lift, 0.0, tol)
                .unwrap();
        assert_eq!(kept.len(), mixed.len());
        // a curve that misses the image passes it nowhere: its nearest points all stay
        let off_curve = start + Vec3::new(0.0, 0.0, 1.0);
        let kept = passages_on_one_turn(&k, &curve, &starts, off_curve, off_curve + lift, 0.0, tol)
            .unwrap();
        assert_eq!(kept.len(), starts.len());
        // nothing to take from no solution
        assert!(
            passages_on_one_turn(&k, &curve, &[], start, start + lift, 0.0, tol)
                .unwrap()
                .is_empty()
        );
    }

    /// WR-673: an open edge's second trim point is passed over only where it is the first
    /// trim point again, reached round a closed 3D curve (its twin across the seam). An
    /// open edge whose real second point lies within the "same point" tolerance of the
    /// first keeps it, on an open curve and on a closed one alike: a test of the distance
    /// alone (the guard as first retired) passed both over and left the edge with no
    /// second point.
    #[test]
    fn a_short_open_edge_keeps_its_second_trim_point() {
        let k = Kernel::default();
        let tol = 1e-5;
        // an open line: its second point 4e-6 along, within the tolerance of the first
        let line = Curve::from(
            BSplineCurve::create_line_segment(
                3,
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        );
        let ivl = line.natural_interval();
        let (t0, t) = (ivl.min(), ivl.evaluate(4e-6));
        let near = distance(
            line.evaluate_point(&k, t0).unwrap(),
            line.evaluate_point(&k, t).unwrap(),
        );
        assert!(near > 0.0 && near <= tol, "{near}");
        assert!(!is_first_point_again(&k, &line, t0, t, tol, false).unwrap());

        // a closed circle of radius 10: the seam twin of its start is the first point
        // again; a point 5e-6 along the circle from the start is a real second point
        let circle = Curve::from(
            Ellipse::new_circle(
                &k,
                Point3::new(1.0, 2.0, 3.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Extent1d::from_min_max(0.0, 360.0),
                10.0,
                &crate::curve::ellipse::EllipseOptions::default(),
            )
            .unwrap(),
        );
        let ivl = circle.natural_interval();
        let (t0, twin) = (ivl.min(), ivl.max());
        let same = distance(
            circle.evaluate_point(&k, t0).unwrap(),
            circle.evaluate_point(&k, twin).unwrap(),
        );
        assert!(same <= tol, "{same}");
        for ends_apart in [false, true] {
            assert!(is_first_point_again(&k, &circle, t0, twin, tol, ends_apart).unwrap());
            assert!(is_first_point_again(&k, &circle, twin, t0, tol, ends_apart).unwrap());
        }
        let step = 5e-6 / (10.0 * 2.0 * PI) * (ivl.max() - ivl.min());
        let short = t0 + step;
        let near = distance(
            circle.evaluate_point(&k, t0).unwrap(),
            circle.evaluate_point(&k, short).unwrap(),
        );
        assert!(near > 0.0 && near <= tol, "{near}");
        assert!(!is_first_point_again(&k, &circle, t0, short, tol, false).unwrap());
        // reversed: the first point at the natural end and the second 5e-6 across the
        // seam, the edge's ends as near as above. The span from the second point to the
        // first runs round the curve, its ends within the tolerance, but the edge is the
        // short arc across the seam: a real second point, kept as on the other side.
        assert!(!is_first_point_again(&k, &circle, twin, short, tol, false).unwrap());
        let before = twin - step;
        assert!(!is_first_point_again(&k, &circle, t0, before, tol, false).unwrap());
        // where the edge's own ends are apart, a span round the curve is its first
        // point again (WR-673 as merged)
        assert!(is_first_point_again(&k, &circle, twin, short, tol, true).unwrap());
        // a point half way round is not the first point at all
        for ends_apart in [false, true] {
            let half = ivl.evaluate(0.5);
            assert!(!is_first_point_again(&k, &circle, t0, half, tol, ends_apart).unwrap());
        }

        // an open half circle whose ends are apart: never the first point again
        let arc = Curve::from(
            Ellipse::new_circle(
                &k,
                Point3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
                Extent1d::from_min_max(0.0, 180.0),
                10.0,
                &crate::curve::ellipse::EllipseOptions::default(),
            )
            .unwrap(),
        );
        let ivl = arc.natural_interval();
        let t = ivl.min() + step / 2.0;
        assert!(!is_first_point_again(&k, &arc, ivl.min(), t, tol, true).unwrap());
    }

    /// WR-530: a tangent edge's drop passes only within its limit; a drop or limit that
    /// is not a number refuses the edge as loose rather than passing it.
    #[test]
    fn a_drop_that_is_not_a_number_is_loose() {
        assert!(!drop_is_loose(0.0, 1e-4));
        assert!(!drop_is_loose(1e-4, 1e-4));
        assert!(drop_is_loose(1.000_000_1e-4, 1e-4));
        assert!(drop_is_loose(f64::NAN, 1e-4));
        assert!(drop_is_loose(0.0, f64::NAN));
        assert!(drop_is_loose(f64::INFINITY, 1e-4));
    }

    /// The pieces of one cubic split apart and joined back again: the joint knot (full
    /// multiplicity) comes out, the curve is the original to rounding at every sample, and
    /// the bound covers how far it moved. Two lines meeting at a corner keep their knot.
    #[test]
    fn a_joint_inside_a_smooth_curve_loses_its_knot_and_a_corner_keeps_it() {
        let k = Kernel::default();
        let pw = [
            CPoint::new(0.0, 0.0, 0.0, NOW),
            CPoint::new(1.0, 2.0, 0.5, NOW),
            CPoint::new(3.0, 3.0, -1.0, NOW),
            CPoint::new(4.0, 1.0, 2.0, NOW),
            CPoint::new(6.0, 0.0, 1.0, NOW),
        ];
        let u = [0.0, 0.0, 0.0, 0.0, 1.0, 2.5, 2.5, 2.5, 2.5];
        let whole = BSplineCurve::from_data(3, 4, 8, 3, &pw, &u).unwrap();
        let mut head = Curve::from(whole.clone_exact());
        let tail = head.create_by_split_curve(&k, 1.7).unwrap();
        let piece = |c: &Curve| match c.kind() {
            CurveKind::BSpline(b) => b.clone_exact(),
            _ => panic!("not a B-spline"),
        };
        let mut segments = [piece(&head), piece(&tail)];
        let (mut joined, breaks) =
            BSplineCurve::create_by_joining_with_breaks(&mut segments, Some(&[true, true]))
                .unwrap();
        let at = breaks[1];
        assert_eq!(at, 1.7);
        let (knots, mults) = joined.knots(None).unwrap();
        assert!(knots.iter().zip(&mults).any(|(&u, &m)| u == at && m == 3));
        let bound = remove_exact_joint_knot(&mut joined, at).unwrap();
        assert!(bound > 0.0 && bound < 1e-10, "{bound}");
        let (knots, mults) = joined.knots(None).unwrap();
        assert_eq!(knots, vec![0.0, 1.0, 2.5]);
        assert_eq!(mults, vec![4, 1, 4]);
        for j in 0..=25 {
            let t = 0.1 * f64::from(j);
            let a = whole.evaluate_point(t).unwrap();
            let b = joined.evaluate_point(t).unwrap();
            assert!(a.distance(b) <= bound, "{t}: {a:?} {b:?}");
        }

        let mut corner = [
            BSplineCurve::create_line_segment(
                3,
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
            BSplineCurve::create_line_segment(
                3,
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
            )
            .unwrap(),
        ];
        let (mut kinked, breaks) =
            BSplineCurve::create_by_joining_with_breaks(&mut corner, Some(&[true, true])).unwrap();
        let before = kinked.knots(None).unwrap();
        assert_eq!(
            remove_exact_joint_knot(&mut kinked, breaks[1]).unwrap(),
            0.0
        );
        assert_eq!(kinked.knots(None).unwrap(), before);
    }

    /// The cubic of the test above split at its knot `1` (multiplicity 1) and joined back:
    /// the joint has the knot at full multiplicity 3, of which the join added 2. Those two
    /// come out exactly, the third would move the curve, so it stays: the original knots
    /// and curve come back, within the bound of the 2 copies removed.
    #[test]
    fn a_cut_on_an_existing_knot_loses_only_the_copies_the_join_added() {
        let k = Kernel::default();
        let pw = [
            CPoint::new(0.0, 0.0, 0.0, NOW),
            CPoint::new(1.0, 2.0, 0.5, NOW),
            CPoint::new(3.0, 3.0, -1.0, NOW),
            CPoint::new(4.0, 1.0, 2.0, NOW),
            CPoint::new(6.0, 0.0, 1.0, NOW),
        ];
        let u = [0.0, 0.0, 0.0, 0.0, 1.0, 2.5, 2.5, 2.5, 2.5];
        let whole = BSplineCurve::from_data(3, 4, 8, 3, &pw, &u).unwrap();
        let mut head = Curve::from(whole.clone_exact());
        let tail = head.create_by_split_curve(&k, 1.0).unwrap();
        let piece = |c: &Curve| match c.kind() {
            CurveKind::BSpline(b) => b.clone_exact(),
            _ => panic!("not a B-spline"),
        };
        let mut segments = [piece(&head), piece(&tail)];
        let (mut joined, breaks) =
            BSplineCurve::create_by_joining_with_breaks(&mut segments, Some(&[true, true]))
                .unwrap();
        let at = breaks[1];
        assert_eq!(at, 1.0);
        let (knots, mults) = joined.knots(None).unwrap();
        assert_eq!(knots, vec![0.0, 1.0, 2.5]);
        assert_eq!(mults, vec![4, 3, 4]);
        let bound = remove_exact_joint_knot(&mut joined, at).unwrap();
        // two copies, each at most EFF_ZERO (1 + 6)
        assert!(
            bound > 0.0 && bound <= 2.0 * 7.0 * EFF_ZERO * (1.0 + 1e-12),
            "{bound}"
        );
        let (knots, mults) = joined.knots(None).unwrap();
        assert_eq!(knots, vec![0.0, 1.0, 2.5]);
        assert_eq!(mults, vec![4, 1, 4]);
        for j in 0..=25 {
            let t = 0.1 * f64::from(j);
            let a = whole.evaluate_point(t).unwrap();
            let b = joined.evaluate_point(t).unwrap();
            assert!(a.distance(b) <= bound, "{t}: {a:?} {b:?}");
        }
    }

    /// The certified removal keeps a removal only within its bound. A knot that is not
    /// removable exactly (the cubic's simple knot `1`), removed with a loose tolerance:
    /// the curve moves by what the certificate measures, which covers the curve's move at
    /// every sample and is within the removal's budget. The same knot with a tolerance
    /// below that move stays, and a rational curve is never changed.
    #[test]
    fn a_knot_removal_is_kept_only_within_its_certified_bound() {
        let pw = [
            CPoint::new(0.0, 0.0, 0.0, NOW),
            CPoint::new(1.0, 2.0, 0.5, NOW),
            CPoint::new(3.0, 3.0, -1.0, NOW),
            CPoint::new(4.0, 1.0, 2.0, NOW),
            CPoint::new(6.0, 0.0, 1.0, NOW),
        ];
        let u = [0.0, 0.0, 0.0, 0.0, 1.0, 2.5, 2.5, 2.5, 2.5];
        let whole = BSplineCurve::from_data(3, 4, 8, 3, &pw, &u).unwrap();
        let mut loose = whole.clone_exact();
        assert_eq!(loose.remove_knot_certified(1.0, 1, 10.0).unwrap(), 1);
        assert_eq!(loose.knots(None).unwrap().0, vec![0.0, 2.5]);
        let mut moved = 0.0_f64;
        for j in 0..=250 {
            let t = 0.01 * f64::from(j);
            let a = whole.evaluate_point(t).unwrap();
            moved = moved.max(a.distance(loose.evaluate_point(t).unwrap()));
        }
        assert!(moved > 0.1 && moved <= 10.0, "{moved}");
        // below the sampled move, no removal can be certified
        let mut tight = whole.clone_exact();
        assert_eq!(tight.remove_knot_certified(1.0, 1, 0.5 * moved).unwrap(), 0);
        assert_eq!(tight.knots(None).unwrap(), whole.knots(None).unwrap());
        // a rational curve keeps its knots whatever the tolerance
        let rpw = [
            CPoint::new(0.0, 0.0, 0.0, 1.0),
            CPoint::new(2.0, 4.0, 1.0, 2.0),
            CPoint::new(3.0, 3.0, -1.0, 1.0),
            CPoint::new(4.0, 1.0, 2.0, 1.0),
            CPoint::new(6.0, 0.0, 1.0, 1.0),
        ];
        let mut rational = BSplineCurve::from_data(3, 4, 8, 3, &rpw, &u).unwrap();
        assert_eq!(rational.remove_knot_certified(1.0, 1, 1e9).unwrap(), 0);
        assert_eq!(rational.knots(None).unwrap().0, vec![0.0, 1.0, 2.5]);
    }

    /// #17210: a saddle `z = x y` and the plane `z = 0` are tangent at the origin, where the
    /// two lines of their intersection, `x = 0` and `y = 0`, cross. Marched, each line comes
    /// out as two pieces that end there. The pieces of each line are joined into one curve
    /// (one of them given backwards), whose 3D and UV curves keep one parameter, and the two
    /// lines, which cross there, are not joined to each other. Pieces that meet where the
    /// surfaces cross are not joined.
    #[test]
    fn intersection_pieces_through_a_tangent_point_are_one_curve() {
        let k = Kernel::default();
        let p = Point3::new;
        // u, v over [0, 1]: x = 2u − 1, y = 2v − 1
        let saddle = Surface::from(
            BSplineSurface::create_bilinear_surface(
                p(-1.0, -1.0, 1.0),
                p(1.0, -1.0, -1.0),
                p(-1.0, 1.0, -1.0),
                p(1.0, 1.0, 1.0),
            )
            .unwrap(),
        );
        let plane = Surface::from(
            BSplineSurface::create_bilinear_surface(
                p(-1.0, -1.0, 0.0),
                p(1.0, -1.0, 0.0),
                p(-1.0, 1.0, 0.0),
                p(1.0, 1.0, 0.0),
            )
            .unwrap(),
        );
        let line = |dim: u64, a: Point3, b: Point3| {
            Curve::from(BSplineCurve::create_line_segment(dim, a, b).unwrap())
        };
        let uv = |x: f64, y: f64| p(0.5 * (x + 1.0), 0.5 * (y + 1.0), 0.0);
        let mut ssi = SurfaceIntersection::default();
        // y = 0 from x = −1 in, from x = 1 in (backwards), x = 0 from y = −1 in and out to 1
        for (a, b) in [
            ((-1.0, 0.0), (0.0, 0.0)),
            ((1.0, 0.0), (0.0, 0.0)),
            ((0.0, -1.0), (0.0, 0.0)),
            ((0.0, 0.0), (0.0, 1.0)),
        ] {
            ssi.curves_3d
                .push(line(3, p(a.0, a.1, 0.0), p(b.0, b.1, 0.0)));
            for list in [&mut ssi.surface1_uv_curves, &mut ssi.surface2_uv_curves] {
                list.push(Some(line(2, uv(a.0, a.1), uv(b.0, b.1))));
            }
            ssi.curve_types.push(TsectCurveType::default());
            ssi.deviations.push(0.0);
        }
        let tol = 1e-6;
        join_through_tangent_points(&k, &mut ssi, [&saddle, &plane], tol, 0.5_f64.to_radians())
            .unwrap();
        assert_eq!(ssi.curves_3d.len(), 2);
        let mut lines = Vec::new();
        for (i, c3) in ssi.curves_3d.iter().enumerate() {
            let ivl = c3.natural_interval();
            let ends = [ivl.min(), ivl.max()].map(|t| c3.evaluate_point(&k, t).unwrap());
            for j in 0..=8 {
                let t = ivl.evaluate(f64::from(j) / 8.0);
                let at = c3.evaluate_point(&k, t).unwrap();
                // on both surfaces, through each UV curve at the same parameter
                for (surface, uvs) in [
                    (&saddle, &ssi.surface1_uv_curves),
                    (&plane, &ssi.surface2_uv_curves),
                ] {
                    let w = uvs[i].as_ref().unwrap().evaluate_point(&k, t).unwrap();
                    let lifted = surface.evaluate_point(&k, Point2::new(w.x, w.y)).unwrap();
                    assert!(lifted.distance(at) < tol, "{lifted:?} {at:?}");
                }
            }
            lines.push(ends);
            // two collinear pieces join smoothly: the joint knot comes out of the 3D and
            // both UV curves (a single span is left), and the deviation, 0 for the
            // pieces, is the removal's bound, a few rounding units
            for c in [
                c3,
                ssi.surface1_uv_curves[i].as_ref().unwrap(),
                ssi.surface2_uv_curves[i].as_ref().unwrap(),
            ] {
                let CurveKind::BSpline(b) = c.kind() else {
                    panic!("not a B-spline");
                };
                assert_eq!(b.knots(None).unwrap().0.len(), 2, "{:?}", b.knots(None));
            }
            let d = ssi.deviations[i];
            assert!(
                d > 0.0 && d <= 3.0 * 2.0 * EFF_ZERO * (1.0 + 1e-12),
                "{d:e}"
            );
        }
        // each a whole line through the origin: y = 0 from end to end, and x = 0
        for ends in &lines {
            let [a, b] = ends;
            assert!((a.distance(*b) - 2.0).abs() < tol, "{ends:?}");
            assert!((*a + *b).length() < tol, "{ends:?}");
        }
        // the same pieces where the surfaces cross (the plane z = 0 against z = x): left
        let tilted = Surface::from(
            BSplineSurface::create_bilinear_surface(
                p(-1.0, -1.0, -1.0),
                p(1.0, -1.0, 1.0),
                p(-1.0, 1.0, -1.0),
                p(1.0, 1.0, 1.0),
            )
            .unwrap(),
        );
        let mut crossing = SurfaceIntersection::default();
        for (a, b) in [((0.0, -1.0), (0.0, 0.0)), ((0.0, 0.0), (0.0, 1.0))] {
            crossing
                .curves_3d
                .push(line(3, p(a.0, a.1, 0.0), p(b.0, b.1, 0.0)));
            for list in [
                &mut crossing.surface1_uv_curves,
                &mut crossing.surface2_uv_curves,
            ] {
                list.push(Some(line(2, uv(a.0, a.1), uv(b.0, b.1))));
            }
            crossing.curve_types.push(TsectCurveType::default());
            crossing.deviations.push(0.0);
        }
        join_through_tangent_points(
            &k,
            &mut crossing,
            [&tilted, &plane],
            tol,
            0.5_f64.to_radians(),
        )
        .unwrap();
        assert_eq!(crossing.curves_3d.len(), 2);
    }

    #[test]
    fn coaxial_cones_have_only_one_physical_meeting_circle() {
        use crate::primitive::api_primitives as ap;
        let k = Kernel::default();
        let cylinder =
            Surface::from(ap::create_cylinder_surface(&k, Point3::ZERO, 3.0, 4.0).unwrap());
        let cone = Surface::from(
            ap::create_cone_surface(&k, Point3::new(0.0, 0.0, 2.0), 3.0, 2.8, 2.0).unwrap(),
        );
        assert!(unique_cone_meeting(&k, &cylinder, &cone).unwrap());
        assert!(unique_cone_meeting(&k, &cone, &cylinder).unwrap());
        assert!(!unique_cone_meeting(&k, &cone, &cone).unwrap());
        // A tiny transverse shift is not exact coaxiality, even below body tolerance.
        let shifted = Surface::from(
            ap::create_cone_surface(&k, Point3::new(1e-12, 0.0, 2.0), 3.0, 2.8, 2.0).unwrap(),
        );
        assert!(!unique_cone_meeting(&k, &cylinder, &shifted).unwrap());
    }

    #[test]
    fn audit_483_tangent_gaps_preserve_errors_and_no_solution() {
        let k = Kernel::default();
        let error = SolidError::sm_err("injected tangent-gap service");
        for fail_evaluation in [false, true] {
            let mut calls = Vec::new();
            let result = tangent_gap_samples(
                &k,
                &mut calls,
                2,
                1e-6,
                |_, calls, _| {
                    calls.push("evaluate");
                    if fail_evaluation {
                        Err(error.clone())
                    } else {
                        Ok(Vec3::ZERO)
                    }
                },
                |_, calls, _| {
                    calls.push("closest");
                    Err(error.clone())
                },
            );
            // fork 4803d6d: a failed sample is skipped; ClosestPoint's status stays
            // dropped
            assert!(!result.unwrap());
            assert_eq!(
                calls,
                if fail_evaluation {
                    vec!["evaluate"; 3]
                } else {
                    ["evaluate", "closest"].repeat(3)
                }
            );
        }
        let mut samples = Vec::new();
        assert!(!tangent_gap_samples(
            &k,
            &mut samples,
            2,
            1e-6,
            |_, calls, t| {
                calls.push(t);
                Ok(Vec3::X * t)
            },
            |_, _, _| Ok(None)
        )
        .unwrap());
        assert_eq!(samples, vec![0.0, 0.5, 1.0]);
        assert!(tangent_gap_samples(
            &k,
            &mut (),
            2,
            1e-6,
            |_, (), _| Ok(Vec3::X),
            |_, (), _| Ok(Some(2e-6))
        )
        .unwrap());
    }

    /// fork 4803d6d (`SmOffsetGeometryCreation::HaveTangentGaps`), and the default
    /// kernel: a sample that cannot be evaluated shows no gap and is not measured.
    /// Upstream measured the unset point instead, and a closest point far from it read
    /// as a gap.
    #[test]
    fn tangent_gaps_skip_a_sample_that_cannot_be_evaluated() {
        let k = Kernel::default();
        let mut measured = Vec::new();
        let gap = tangent_gap_samples(
            &k,
            &mut measured,
            2,
            1e-6,
            |_, _, t| {
                if t == 0.5 {
                    Err(SolidError::sm_err("injected evaluation failure"))
                } else {
                    Ok(Vec3::X * t)
                }
            },
            |_, measured, p| {
                measured.push(p.x);
                Ok(Some(if p.x == UNDEF_DOUBLE { 1.0 } else { 0.0 }))
            },
        )
        .unwrap();
        assert!(!gap);
        assert_eq!(measured, vec![0.0, 1.0]);
    }

    /// A quadratic in the plane y = 0 over the plane z = 0, from x = -1 to 1 (t = 0 to 1):
    /// `z = e - k x^2 / 2` when `curved`, else the line `z = e x`.
    fn over_the_plane(e: f64, k: f64, curved: bool) -> Curve {
        use crate::nurbs::crv_basic::Curve as NlCurve;
        use crate::nurbs::math::CPoint;
        let (z0, z1, z2) = if curved {
            (e - k / 2.0, e + k / 2.0, e - k / 2.0)
        } else {
            (-e, 0.0, e)
        };
        let pw = vec![
            CPoint::new(-1.0, 0.0, z0, NOW),
            CPoint::new(0.0, 0.0, z1, NOW),
            CPoint::new(1.0, 0.0, z2, NOW),
        ];
        let nl = NlCurve::new(pw, 2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap();
        Curve::from(BSplineCurve::from_nurb(nl, 3, false))
    }

    /// A curve that touches a surface, exact to the tolerance, meets it where its error
    /// lifts it: the curve of radius 3 lifted 1e-6 over the plane meets it 2.4e-3 from the
    /// touch, 0.05 degrees off it, and does not pass through it by more than the 1e-5
    /// tolerance. Lifted 1e-3, or a line 0.6 degrees off the plane, it does.
    #[test]
    fn a_curve_passes_through_a_surface_only_by_more_than_the_tolerance() {
        use crate::surface::plane::Plane;
        let k = Kernel::default();
        let plane = Surface::from(Plane::from_normal(Vec3::ZERO, Vec3::Z));
        let uv = Point2::new(0.0, 0.0);
        let tol = 1e-5;
        let at = |e: f64| (1.0 + (6.0 * e).sqrt()) / 2.0;
        let touching = over_the_plane(1e-6, 1.0 / 3.0, true);
        assert!(touching.evaluate_point(&k, at(1e-6)).unwrap().z.abs() < 1e-15);
        assert!(!passes_through(&k, &touching, at(1e-6), &plane, uv, tol).unwrap());
        let lifted = over_the_plane(1e-3, 1.0 / 3.0, true);
        assert!(passes_through(&k, &lifted, at(1e-3), &plane, uv, tol).unwrap());
        let shallow = over_the_plane(0.6_f64.to_radians().tan(), 0.0, false);
        assert!(passes_through(&k, &shallow, 0.5, &plane, uv, tol).unwrap());
    }

    /// The crossing test uses only the second-order jet. An implicit offset keeps
    /// its three-total-derivative limit, yet must classify the same plane's touch
    /// and shallow crossing (the unsupported request prevented #1818's rim mating).
    #[test]
    fn a_curve_passes_through_an_implicit_offset_using_only_second_order() {
        use crate::surface::offset_surface::OffsetSurface;
        use crate::surface::plane::Plane;

        let k = Kernel::default();
        let base = Surface::from(Plane::from_normal(Vec3::new(0.0, 0.0, -0.5), Vec3::Z));
        let surface = Surface::from(OffsetSurface::new(&k, 0.5, base, true, 0).unwrap());
        let uv = Point2::new(0.0, 0.0);
        assert!(surface.evaluate_point(&k, uv).unwrap().distance(Vec3::ZERO) < 1e-15);
        let e = surface
            .evaluate(&k, uv, 2, 2, EvaluateFlags::default())
            .unwrap_err();
        assert_eq!(e.upstream_status(), Some(SM_ERR));
        assert_eq!(
            e.operation(),
            "surface::offset_surface::OffsetSurface::evaluate"
        );

        let tol = 1e-5;
        let at = |e: f64| (1.0 + (6.0 * e).sqrt()) / 2.0;
        let touching = over_the_plane(1e-6, 1.0 / 3.0, true);
        assert!(!passes_through(&k, &touching, at(1e-6), &surface, uv, tol).unwrap());
        let lifted = over_the_plane(1e-3, 1.0 / 3.0, true);
        assert!(passes_through(&k, &lifted, at(1e-3), &surface, uv, tol).unwrap());
        let shallow = over_the_plane(0.6_f64.to_radians().tan(), 0.0, false);
        assert!(passes_through(&k, &shallow, 0.5, &surface, uv, tol).unwrap());
    }

    #[test]
    fn creation_defaults() {
        let g = OffsetGeometryCreation::new(0.25, 1e-5);
        assert_eq!(g.offset_distance(), 0.25);
        assert_eq!(g.this_approx_tol_3d(), 1e-5);
        assert!((g.tangency_tol_degrees() - 2.0).abs() < 1e-12);
        assert!((g.tangency_tol_radians() - 2.0 * PI / 180.0).abs() < 1e-18);
        assert!(g.make_analytics());
        let mut g = g;
        g.set_make_analytics(false);
        assert!(!g.make_analytics());
    }

    #[test]
    fn local_operation_clears_analytics() {
        let l: LocalOperationGeometryCreation<H> =
            LocalOperationGeometryCreation::new(1.0, 1e-4, &[H(1), H(2)], &[H(10), H(20)]);
        assert!(!l.base.make_analytics());
        assert_eq!(l.faces, vec![H(1), H(2)]);
        assert_eq!(l.surfaces, vec![H(10), H(20)]);
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct H(u32);

    impl ClassObject for H {
        fn class_kind(&self) -> crate::intersect::point_class::ClassObjectKind {
            crate::intersect::point_class::ClassObjectKind::Other
        }
    }

    impl SolutionObject for H {
        fn kind(&self) -> crate::geom::solution_array::ObjectKind {
            crate::geom::solution_array::ObjectKind::Other
        }
    }

    #[test]
    fn ssi_data_reset() {
        let mut d: SsiData<H> = SsiData::default();
        d.curves_3d.push(H(1));
        d.deviations.push(0.5);
        d.reset();
        assert!(d.curves_3d.is_empty() && d.deviations.is_empty());
    }

    #[test]
    fn exact_collinear_line_and_unit_reparameterization_use_control_nets_not_samples() {
        use crate::curve::line::Line;
        use crate::nurbs::crv_basic::Curve as NlCurve;
        use crate::nurbs::math::CPoint;

        // 1. Exact `Line` and collinear 3D B-spline pass `exact_collinear_line`.
        let seg = Curve::from(Line::from_points(
            Point3::new(1.0, 2.0, 3.0),
            Point3::new(5.0, -2.0, 7.0),
            3,
        ));
        let (pt, dir, eps) = exact_collinear_line(&seg).expect("exact line");
        assert_eq!(eps, 0.0);
        assert!((pt - Point3::new(1.0, 2.0, 3.0)).length() < 1e-15);
        assert!(dir.length() > 1.0);

        let collinear_bs = Curve::from(BSplineCurve::from_nurb(
            NlCurve::new(
                vec![
                    CPoint::new(0.0, 0.0, 0.0, 1.0),
                    CPoint::new(2.0, 4.0, 6.0, 2.0), // Euclidean `(1, 2, 3)`
                    CPoint::new(3.0, 6.0, 9.0, 1.0), // Euclidean `(3, 6, 9)`
                ],
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            )
            .unwrap(),
            3,
            true,
        ));
        let (_, _, eps_bs) = exact_collinear_line(&collinear_bs).expect("collinear rational net");
        assert!(eps_bs <= 1e-11);

        // 2. A shallow quadratic arc of sagitta `5e-5` (strictly below `10 * tau_e = 1e-4`
        //    where `cv.is_line(k, 10, line_tol)` previously collapsed it into a straight chord!)
        //    is rejected by `exact_collinear_line` so its curved geometry is preserved, and its
        //    `[0, 1]` knot reparameterization passes `is_exact_unit_reparameterization`.
        let shallow_arc_bs = BSplineCurve::from_nurb(
            NlCurve::new(
                vec![
                    CPoint::new(0.0, 0.0, 0.0, NOW),
                    CPoint::new(2.5, 5e-5, 0.0, NOW),
                    CPoint::new(7.5, 5e-5, 0.0, NOW),
                    CPoint::new(10.0, 0.0, 0.0, NOW),
                ],
                2,
                vec![2.0, 2.0, 2.0, 5.5, 9.0, 9.0, 9.0],
            )
            .unwrap(),
            3,
            false,
        );
        assert!(exact_collinear_line(&Curve::from(shallow_arc_bs.clone())).is_none());
        let mut unit_bs = shallow_arc_bs.clone();
        unit_bs
            .edit_parameterization(&Extent1d::from_min_max(0.0, 1.0))
            .unwrap();
        assert!(is_exact_unit_reparameterization(&shallow_arc_bs, &unit_bs));

        // Perturbing an interior knot or control point invalidates the affine equivalence.
        let perturbed = BSplineCurve::from_nurb(
            NlCurve::new(
                vec![
                    CPoint::new(0.0, 0.0, 0.0, NOW),
                    CPoint::new(2.5, 5e-5, 0.0, NOW),
                    CPoint::new(7.5, 5e-5, 0.0, NOW),
                    CPoint::new(10.0, 0.0, 0.0, NOW),
                ],
                2,
                vec![0.0, 0.0, 0.0, 0.6, 1.0, 1.0, 1.0],
            )
            .unwrap(),
            3,
            false,
        );
        assert!(!is_exact_unit_reparameterization(
            &shallow_arc_bs,
            &perturbed
        ));
    }

    /// Three arcs on the coordinate planes about the origin as a vertex's bounding edges,
    /// for `VertexOffset`'s three-edge sphere cap.
    struct ArcCorner {
        curves: Vec<Curve>,
        surfaces: Vec<Surface>,
    }

    impl VertexOffsetBrep<usize> for ArcCorner {
        fn brep_tolerance(&self, _vertex: usize) -> Result<f64> {
            Ok(1e-6)
        }
        fn vertex_point(&self, _vertex: usize) -> Result<Point3> {
            Ok(Point3::new(0.0, 0.0, 0.0))
        }
        fn vertex_is_lamina(&self, _vertex: usize) -> Result<bool> {
            Ok(false)
        }
        fn edge_faces(&self, _edge: usize) -> Result<Vec<usize>> {
            Ok(Vec::new())
        }
        fn edge_is_seam_of_face_surface(
            &mut self,
            _k: &Kernel,
            _edge: usize,
            _face: usize,
        ) -> Result<bool> {
            Ok(false)
        }
        fn edge_curve(&self, edge: usize) -> Result<Option<usize>> {
            Ok(Some(edge))
        }
        fn edge_interval(&self, edge: usize) -> Result<Extent1d> {
            Ok(self.curve(edge)?.natural_interval())
        }
        fn curve(&self, curve: usize) -> Result<&Curve> {
            self.curves
                .get(curve)
                .ok_or(SolidError::invalid_input("test", "no such curve"))
        }
        fn insert_curve(&mut self, curve: Curve) -> Result<usize> {
            self.curves.push(curve);
            Ok(self.curves.len() - 1)
        }
        fn insert_surface(&mut self, surface: Surface) -> Result<usize> {
            self.surfaces.push(surface);
            Ok(self.surfaces.len() - 1)
        }
    }

    #[test]
    fn vertex_offset_keeps_the_sphere_error_as_its_cause() {
        let k = Kernel::default();
        let o = Point3::new(0.0, 0.0, 0.0);
        let arc = |a: [f64; 3], b: [f64; 3]| {
            Curve::from(
                BSplineCurve::create_arc_from_points(
                    o,
                    Point3::new(a[0], a[1], a[2]),
                    Point3::new(b[0], b[1], b[2]),
                    NurbCircleParam::Quadratic,
                )
                .unwrap(),
            )
        };
        // each arc's ends lie on the other arcs' planes, but the third has radius 2: no
        // sphere is made from them
        let curves = vec![
            arc([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
            arc([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            arc([0.0, 0.0, 2.0], [2.0, 0.0, 0.0]),
        ];
        let built = BSplineSurface::create_sphere_from_arcs(&k, &curves, 1e-6)
            .map(|_| ())
            .unwrap_err();
        let mut brep = ArcCorner {
            curves,
            surfaces: Vec::new(),
        };
        let e = OffsetGeometryCreation::new(1.0, 1e-6)
            .vertex_offset_with_backend(&k, &mut brep, Some(0), &[0, 1, 2])
            .map(|_| ())
            .unwrap_err();
        assert_eq!(
            e.offset_failure(),
            Some(&OffsetFailure::VertexArcsNotOneSphere)
        );
        // the sphere's error decides the class, never a defect of its own
        assert_eq!(e.is_defect(), built.is_defect());
        assert_eq!(e.is_refusal(), built.is_refusal());
        assert_eq!(e.upstream_status(), Some(SM_ERR));
        assert_eq!(e.cause(), Some(&built));
    }

    /// No vertex is the caller's argument (`SM_ERR_NULL_POINTER`, as upstream's `NER`); a
    /// vertex whose three bounding curves are not all arcs (two arcs and a line: no shell
    /// either) gets no sphere (`SM_ERR`).
    #[test]
    fn vertex_offset_names_no_vertex_and_a_bounding_curve_that_is_no_arc() {
        use crate::curve::line::Line;
        let k = Kernel::default();
        let o = Point3::new(0.0, 0.0, 0.0);
        let arc = |a: Point3, b: Point3| {
            Curve::from(
                BSplineCurve::create_arc_from_points(o, a, b, NurbCircleParam::Quadratic).unwrap(),
            )
        };
        let (x, y, z) = (
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.0, 0.0, 1.0),
        );
        let mut brep = ArcCorner {
            curves: vec![
                arc(x, y),
                arc(y, z),
                Curve::from(Line::from_points(z, x, 3)),
            ],
            surfaces: Vec::new(),
        };
        let creator = OffsetGeometryCreation::new(1.0, 1e-6);
        let e = creator
            .vertex_offset_with_backend(&k, &mut brep, None, &[0, 1, 2])
            .map(|_| ())
            .unwrap_err();
        assert_eq!(e.offset_failure(), Some(&OffsetFailure::VertexNotGiven));
        assert_eq!(e.upstream_status(), Some(SM_ERR_NULL_POINTER));
        assert_eq!(e.kind(), crate::error::ErrorKind::InvalidInput);
        let e = creator
            .vertex_offset_with_backend(&k, &mut brep, Some(0), &[0, 1, 2])
            .map(|_| ())
            .unwrap_err();
        assert_eq!(e.offset_failure(), Some(&OffsetFailure::VertexCurveNotArc));
        assert_eq!(e.upstream_status(), Some(SM_ERR));
        assert_eq!(e.operation(), OP_VERTEX);
    }

    /// A curve that stops (no tangent) where it meets a surface leaves the crossing
    /// undecided, with the unit vector's error as its cause (`SM_ERR`).
    #[test]
    fn a_crossing_of_a_curve_that_stops_is_undecided() {
        use crate::surface::plane::Plane;
        let k = Kernel::default();
        let plane = Surface::from(Plane::from_normal(Vec3::ZERO, Vec3::Z));
        let point = Curve::from(BSplineCurve::create_degenerate_curve(3, Point3::ZERO).unwrap());
        let e = passes_through(&k, &point, 0.5, &plane, Point2::new(0.0, 0.0), 1e-5).unwrap_err();
        assert_eq!(e.offset_failure(), Some(&OffsetFailure::CrossingUndecided));
        assert_eq!(e.upstream_status(), Some(SM_ERR));
        assert!(e.cause().is_some(), "{e:?}");
    }

    /// The faces and surfaces of a local operation, over handles: the face use `fu` is on
    /// face `f`.
    struct LocalFaces {
        surfaces: Vec<Surface>,
    }

    impl FaceOffsetBrep<H> for LocalFaces {
        fn faceuse_orientation(&self, _faceuse: H) -> Result<OrientType> {
            Ok(OrientType::Same)
        }
        fn faceuse_face(&self, faceuse: H) -> Result<Option<H>> {
            Ok(Some(H(faceuse.0 + 100)))
        }
        fn face_surface(&self, _face: H) -> Result<Option<H>> {
            Ok(None)
        }
        fn surface(&self, surface: H) -> Result<&Surface> {
            self.surfaces
                .get(surface.0 as usize)
                .ok_or(SolidError::invalid_input("test", "no such surface"))
        }
        fn insert_surface(&mut self, surface: Surface) -> Result<H> {
            self.surfaces.push(surface);
            Ok(H(self.surfaces.len() as u32 - 1))
        }
        fn insert_curve(&mut self, _curve: Curve) -> Result<H> {
            Ok(H(0))
        }
    }

    /// A local operation's creator asked for the surface of a face it was not given: the
    /// caller's argument (`SM_ERR`); a listed face gets its listed surface.
    #[test]
    fn a_local_operation_refuses_a_face_it_was_not_given() {
        let mut brep = LocalFaces {
            surfaces: Vec::new(),
        };
        let creator =
            LocalOperationGeometryCreation::from_handles(1.0, 1e-6, vec![H(101)], vec![H(7)]);
        let (listed, _) = creator
            .faceuse_offset_with_backend(&mut brep, H(1))
            .unwrap();
        assert_eq!(listed, Some(H(7)));
        let e = creator
            .faceuse_offset_with_backend(&mut brep, H(2))
            .unwrap_err();
        assert_eq!(e.offset_failure(), Some(&OffsetFailure::LocalFaceNotListed));
        assert_eq!(e.upstream_status(), Some(SM_ERR));
        assert_eq!(e.kind(), crate::error::ErrorKind::InvalidInput);
    }
}
