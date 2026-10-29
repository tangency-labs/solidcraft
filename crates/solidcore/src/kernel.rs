//! The [`Kernel`] value: tolerances, deterministic RNG and caches, passed explicitly to
//! operations in place of upstream's process-global context.
//!
//! Upstream: `source/SMLib/inc/SmContext.h` (`SmContext`), `SmTol.h` (`SmTol`,
//! `SmTol3d`), the tolerance defaults of `SmMath.h`, the `SmCacheMgr*` classes, and
//! `SM_API`'s `SmApiGetOrCreateContext()`.
//!
//! | Rust                                   | Upstream                                  |
//! |----------------------------------------|-------------------------------------------|
//! | [`Kernel`]                             | `SmContext`                               |
//! | [`Kernel::context`]                    | `SmContext` marks, flags and own tolerance (see [`crate::geom::context`]) |
//! | [`Tolerances::zone_tol_3d`]            | `SmContext::m_sThisZoneTol3d`, `SM_ZONE_TOL_3D` |
//! | [`Tolerances::xsect_tol_3d`]           | `SM_XSECT_TOL_3D`                         |
//! | [`Tolerances::approx_tol_3d`]          | `SM_APPROX_TOL_3D`                        |
//! | [`Tolerances::ang_tol_deg`]            | `SM_ANG_TOL_DEG`                          |
//! | [`Tolerances::tol_gap_gain`]           | `SM_TOL_GAP_GAIN`                         |
//! | [`Tolerances::model_size_estimate`]    | `SmContext::m_dThisModelSizeEstimate`, `SM_MODEL_SIZE_ESTIMATE` |
//! | [`Tolerances::large_small_size_ratio`] | `SM_LARGE_SMALL_SIZE_RATIO`               |
//! | [`Rng`]                                | C `rand()` / `srand(time(NULL))` (e.g. `SmShell.cpp`) |
//! | [`Caches`]                             | `SmCacheMgr`, `SmCacheMgrCrv/Srf/TSrf/Brep` |
//! | [`Kernel::fillet_point_step`], [`Kernel::latch_fillet_point_step`] | the function-static `sdStaticStep` of `SmFilletSolver::ComputePointValues` |
//! | [`Kernel::upstream_ub_passed`], [`Kernel::note_upstream_ub`] | (none: upstream undefined behaviour the port went past) |
//!
//! Upstream seeds `rand()` from the clock in a few places; the port replaces that with
//! [`Rng`], seeded from [`KernelOptions::seed`], so runs are reproducible. Results that
//! depend on random draws therefore cannot match the C++ bit for bit; difftests of such
//! code must compare outcomes that are invariant to the draw (and say so in
//! `plan/findings.md`).
//!
//! Upstream also keeps a few process-global function statics whose first use fixes
//! their value for the rest of the process; those that change results live on the
//! [`Kernel`] as latches (e.g. [`Kernel::fillet_point_step`]), so each kernel starts
//! unlatched and a difftest runner can seed the latch with the value the C++ process
//! already holds.

use core::cell::Cell;

use crate::geom::context::Context;

/// Geometric tolerances in model units (upstream defaults from `SmMath.h`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerances {
    /// Local-neighbourhood size: points closer than this are coincident
    /// (`SM_ZONE_TOL_3D` = `1e-5`).
    pub zone_tol_3d: f64,
    /// Default gap / intersection tolerance (`SM_XSECT_TOL_3D` = `2 * zone`).
    pub xsect_tol_3d: f64,
    /// Default approximation tolerance (`SM_APPROX_TOL_3D` = `zone / 2`).
    pub approx_tol_3d: f64,
    /// Maximum angle between parallel vectors, degrees (`SM_ANG_TOL_DEG` = `0.5`).
    pub ang_tol_deg: f64,
    /// Over-size factor applied above an object's max gap (`SM_TOL_GAP_GAIN` = `1.1`).
    pub tol_gap_gain: f64,
    /// Expected model size used to pick tolerances
    /// (`SM_MODEL_SIZE_ESTIMATE` = `50.0`).
    pub model_size_estimate: f64,
    /// Ratio of largest to smallest feature size
    /// (`SM_LARGE_SMALL_SIZE_RATIO` = `1000`).
    pub large_small_size_ratio: f64,
}

impl Default for Tolerances {
    fn default() -> Self {
        let zone = 1.0e-5;
        Self {
            zone_tol_3d: zone,
            xsect_tol_3d: 2.0 * zone,
            approx_tol_3d: zone / 2.0,
            ang_tol_deg: 0.5,
            tol_gap_gain: 1.1,
            model_size_estimate: 50.0,
            large_small_size_ratio: 1000.0,
        }
    }
}

impl Tolerances {
    /// [`Tolerances::ang_tol_deg`] in radians (`SM_ANG_TOL_RAD`).
    #[must_use]
    pub fn ang_tol_rad(&self) -> f64 {
        self.ang_tol_deg * crate::math::PI / 180.0
    }
}

/// Deterministic pseudo-random generator: `SplitMix64` (Steele, Lea & Flood,
/// "Fast splittable pseudorandom number generators", OOPSLA 2014).
///
/// Chosen because it is tiny, has no dependencies, passes `BigCrush`, behaves the same
/// on every target (including wasm32) and is `Send`. It is not cryptographic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rng {
    state: u64,
}

impl Rng {
    /// Default seed used by [`KernelOptions::default`].
    pub const DEFAULT_SEED: u64 = 0x5EED_5EED_5EED_5EED;

    /// Creates a generator from a seed. Equal seeds give equal sequences.
    #[must_use]
    pub const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// Next 64 random bits.
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform `f64` in `[0, 1)` built from the top 53 bits.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Uniform `f64` in `[-1, 1)`; the port of upstream's
    /// `2.0*((double)rand()/RAND_MAX) - 1.0` idiom.
    pub fn next_signed_unit(&mut self) -> f64 {
        2.0 * self.next_f64() - 1.0
    }
}

/// Memoisation caches owned by a [`Kernel`] (upstream `SmCacheMgr*`).
///
/// Empty until the cache units are ported; clearing must never change results, only
/// speed.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct Caches {}

impl Caches {
    /// Drops all cached data.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// Opt-in fixes of upstream SMLib defects (`plan/upstream-issues/`), all off by default
/// (upstream behaviour). Set them on [`Kernel::fixes`] before running operations; each
/// field names its record (`WR-…`) and the `tangency-labs/solidcore` issue.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
#[non_exhaustive]
pub struct UpstreamFixes {
    /// `CalcRailIntEdgeuse`'s retry rail search holds a variable round's radius only where its first search does (at a corner calculated as constant radius), so the rail misses the arc it would meet only at that radius and the corner cuts the round off against the body; needs `fillet_nx1_end_rib_global_merge` there (WR-300, #1023).
    pub fillet_retry_radius_as_first: bool,
    /// `TestRollOver` joins a rail's neighbouring intervals on its face or the face's boundary before the rollover, so a rail along its face's rim does not fail "Unable to Process Rollover" (WR-301, #485).
    pub fillet_rail_rim_on_face: bool,
    /// The fillet executive refuses a blend whose rails leave the faces they roll on (`RailsFitTheirFaces`, no room) (WR-302, #1024/#841).
    pub fillet_refuse_rails_leaving_faces: bool,
    /// `create_fillet_brep` refuses blends that take the whole of a face they roll on (`FacesOutlastTheirBlends`, no room) (WR-320, #879).
    pub fillet_refuse_face_consuming_blends: bool,
    /// `DoFillet` refuses a filleted target that is not a closed manifold solid (an edge that is not `SmEdge::IsManifold`) with `SM_FILERR_BAD_MERGE`, where upstream returns success (WR-212, #422).
    pub fillet_refuse_non_manifold_result: bool,
    /// The fillet executive refuses two rounds on edges of one face whose rails cross on it (no room between them), before the target is edited, where upstream builds an open body (WR-460, #1073).
    pub fillet_refuse_crossing_rails: bool,
    /// The fillet solver tries the corner ball's patch after the torus when a round's centre line collapses to a point (WR-420, #416).
    pub fillet_corner_ball_patch: bool,
    /// `CalcTorusFilletGeom` revolves a linear section into a cone (WR-421, #420).
    pub fillet_chamfer_cone_on_arc: bool,
    /// A torus blend whose section centre is on the arc's axis turns rail points into the edge ends' half-planes (WR-422, #417).
    pub fillet_torus_center_on_axis: bool,
    /// `DoExtendedOffset` finds the excess offset faces after the merge only, not also on the untrimmed faces before it (WR-341, #977).
    pub offset_excess_faces_after_merge: bool,
    /// `BuildOffsetOfFaceuse` clips a face to its offset domain from its counterclockwise loops (an inset is not turned inside out) (WR-342, #974).
    pub offset_clip_right_side_out: bool,
    /// `SmSurfOfRevolution::CreateOffsetSurface` cuts the offset generator at the axis on either side of the frame origin (WR-343, #973).
    pub revolution_offset_axis_both_sides: bool,
    /// With extended convex edges, `piecewise_merge` relates the mated offsets of a manifold edge instead of intersecting them again (WR-344, #469).
    pub offset_extended_edges_mated: bool,
    /// A volumetric Boolean removes the zero-area loops a contact curve inside a face leaves, and splits shells joined only through them (WR-360, #746).
    pub remove_slit_loops_where_solids_touch: bool,
    /// A face a Boolean takes over from its second operand keeps that operand's exact UV trims (WR-361, #488).
    pub keep_operand_uv_trims: bool,
    /// `smgu_EvaluateCurveContinuity`'s G2 test compares the two sides' Frenet curvatures and normals (WR-380, #933/#838).
    pub geometric_g2_continuity: bool,
    /// `RemoveCoincidentNoEdgeProtoVertex` reports a miss as `false` (WR-73, #848).
    pub proto_vertex_miss_is_false: bool,
    /// `CheckLoop` measures a gap against at least its vertex's zone tolerance (WR-381, #476).
    pub check_loop_vertex_tolerance: bool,
    /// `CheckLoop` compares an end projected from its vertex (no UV trim) with its neighbour on the nearest natural period of a closed direction, so a one-period seam jump is not an `SM_ERR` (WR-440, #530).
    pub check_loop_projected_seam_ends: bool,
    /// `CreateOrientedTrimBoundary` takes a UV gap of whole periods (up to 1% of one) in a closed direction between consecutive curves for a seam crossing and moves the curves after it by the period, instead of filling it with a line across the whole domain that doubles the loop back on itself (WR-491). A loop that does not close that way keeps the line, and so does a gap along a singular side of the surface (a sphere's pole, a cone's apex; #1437).
    pub trim_boundary_seam_wrap: bool,
    /// `SmBSplineCurve::AssertValid` reads the minimum radius of curvature before tests 13/14 compare it (WR-382, #475).
    pub bspline_check_min_radius: bool,
    /// The curve property solves ignore a failed guess in `SolveOscillating`'s sign-change search (WR-253, #481).
    pub oscillating_skips_failed_guesses: bool,
    /// `sm_RefineCCSolutions` judges a range end's shift by its distance from 1 (WR-383, #527).
    pub refine_cc_range_ends: bool,
    /// `SmSurface::GetFlatCorners` tests each corner bit instead of clearing the others (WR-384, #525).
    pub flat_corners_test_bits: bool,
    /// `SmCone::RebuildSTEPFromNURBParameters` gives a cylinder the mean of near-equal radii (semi-angle 0) (WR-385, #713).
    pub cylinder_rebuild_mean_radius: bool,
    /// `SmCylinder::IsNurbSurfaceCylinder` gives an inside-out NURB (its iso circles turning against the line through their centres) the frame `IsNurbSurfaceCone` gives one: Z against the NURB's linear direction, origin at its top-knot circle; upstream keeps the bottom-knot circle and the NURB's direction, so the cylinder's Z is reversed and its STEP evaluation is mirrored in height (WR-496, #1463).
    pub nurb_cylinder_inside_out_frame: bool,
    /// `SweepProfilesAlongPath` asks for caps only when the profile closes (WR-400, #837).
    pub planar_path_uncapped_open_profiles: bool,
    /// A rotational sweep (whole or partial turn) makes each line and circular arc of its profile its cylinder, cone, sphere or torus, not only a one-curve profile's (WR-448, #545; SolidCraft change, tangency-labs/smlib 0b143f7).
    pub revolve_analytic_profile_faces: bool,
    /// With `revolve_analytic_profile_faces`, a revolved arc whose centre is within the B-rep tolerance of the axis is the sphere centred on the axis through the arc's ends, and an end within it of the axis is its pole, not a sphere (or torus) built on the arc's own off-axis circle with a pole row of radius about sqrt(2 tol R) (WR-471, #1123; tangency-labs/smlib, "a revolved arc centred within tolerance of the axis is a sphere through its ends").
    pub revolve_arc_centre_near_axis_is_sphere: bool,
    /// A taper extrude's corner whose links meet at both ends pivots on the joint nearest its offset curve (WR-402, #479).
    pub taper_pivot_nearest_corner: bool,
    /// A full-turn rotational sweep makes a closed shell inside another solid its cavity (WR-403, #478).
    pub revolve_nested_shells_as_cavities: bool,
    /// A curve sweep caps from the side faces at the sweep's start and end (WR-404, #411).
    pub curve_sweep_caps_at_sweep_ends: bool,
    /// Planar faces from curves join a loop touching another loop at a vertex into that loop (WR-405, #892).
    pub join_touching_loops: bool,
    /// The pointer check judges a UV trim's end turn on its face's surface (first fundamental form), not in raw (u, v) (WR-406, #891).
    pub uv_trim_reversal_on_surface: bool,
    /// Before the piecewise merge, `do_extended_offset` rebuilds every offset face whose loops cross from the stretches of its loops that bound it (WR-345, #854).
    pub offset_resolve_crossing_loops: bool,
    /// With `offset_resolve_crossing_loops` (it has no effect without it), loops of an offset face that touch without crossing (run along one another, meet tangentially, or come up to one another and go back on the same side) are refused, before any crossing is resolved, with `SM_ERR_OFFSET_WALLS_TOUCH` (1042) instead of `SM_ERR_BAD_INTERSECTIONS` (1027): the offset walls meet along a line (WR-345 addendum, #1442; tangency-labs/smlib a759622).
    pub offset_walls_touch_refused: bool,
    /// `SmBrep::LocalOperation` returns the failed extended offset's own status (e.g. `SM_ERR_OFFSET_WALLS_TOUCH`, 1042, or `SM_ERR_BAD_INTERSECTIONS`, 1027) instead of a generic `SM_ERR` (WR-345 addendum, #1442; tangency-labs/smlib a759622).
    pub local_operation_offset_status: bool,
    /// An inward shell mates the walls across rounds thinner than it (collapsed faces) (#397).
    pub offset_collapsed_rounds_mated: bool,
    /// `FaceuseOffset` fails on a face whose offset surface is missing without collapsing (`SmSurface::OffsetCollapses`: a cone, cylinder, sphere or torus offset onto its spine), where upstream reads any missing surface as a collapsed round (WR-464, #1072).
    pub offset_missing_surface_fails: bool,
    /// At an Nx1 corner whose end rib cannot be built because a rail missed a side edge ending within the fillet's width, the rib is deleted and the global merge cuts the round off (WR-303, #841).
    pub fillet_nx1_end_rib_global_merge: bool,
    /// `sm_FixUVTrimCurves` moves only the end control points of the two UV trims meeting at a corner onto the vertex's UV point, without dropping the vertex onto them, trimming there and stretching them back over the edge interval (WR-255, #959/#570; the fork's tangency-labs/smlib@5af7070).
    pub uv_trim_corner_end_points_only: bool,
    /// `BSplineCurve::trim` removes short end spans only for model-space curves
    /// (dimension 3), preserving exact UV trims in surface parameter units
    /// (WR-441, #1114). Off by default for pinned SMLib compatibility.
    pub bspline_trim_preserves_uv_spans: bool,
    /// `SmCone::IntersectWithCone` tests the line of two parallel cylinders against both face domains at its middle, not its bottom end (WR-259, #711).
    pub cone_parallel_line_midpoint: bool,
    /// `SmCone::IntersectWithCone` clips parallel-cylinder lines to both requested face domains, including overlaps away from the full line's middle (WR-259, #711). Independent of `cone_parallel_line_midpoint`; off by default for pinned SMLib compatibility.
    pub cone_parallel_line_domains: bool,
    /// The Boolean's topology intersector treats a self-loop proto edge whose ends a vertex definition on a vertex combined (`CanCombineVertexDefinitions`) as degenerate, instead of `SmProtoEdge::Resolve` splitting it onto that one vertex, after which `SmMerge::MakeFaceuse` returns `SM_ERR` (WR-214, #480; `plan/upstream-issues/WR-214--smprotoedge-resolve-splits-a-self-loop-proto-edge-onto-one.md`).
    pub proto_edge_self_loop_on_vertex_degenerate: bool,
    /// `SM_BO_SLICE` keeps a coincident pair whose primary face bounds material on one side only when that material lies on the side the sheet's `SM_OT_SAME` faceuse bounds (step 7.4d's rule), instead of falling through to the difference rule, which keeps a face lying in the slicing sheet in both parts (WR-254, #528; `plan/upstream-issues/WR-254--manifoldboolean-keeps-a-face-lying-in-the-slicing-sheet-in.md`).
    pub slice_coincident_faces_sheet_side: bool,
    /// After step 8, a Union, Difference or Intersection whose result is a manifold solid with more than two regions (the infinite region first) runs `SetRegionIsVoidFlagsForNestedSolids`, which puts each region's enclosing shell first (the fork's tangency-labs/smlib `abastran/void-shell-check`). It also rewrites the void flags by nesting from the infinite region, so where step 7 left flags that disagree with the nesting the result's void flags change too, not only its shell order (WR-265, #388; `plan/upstream-issues/WR-265--manifold-boolean-void-shell-first.md` and its 2026-10-26 addendum).
    pub boolean_outer_shell_first: bool,
    /// `SmShell::IsInnerShellOf` passes on the statuses it ignores (fork 4038552): `UNSURE` when the outer shell's closure, the B-rep's box, the sample points or a crossing face's point or normal cannot be computed; a ray that cannot be solved or classified decides nothing and the next ray is tried (WR-497, #1473).
    pub shell_inner_shell_of_statuses: bool,
    /// `SmEdge::ReverseOrientation` returns a failed curve reversal's status before installing the interval, flipping the edge uses or deleting their UV trims (WR-260, #521).
    pub edge_reversal_keeps_failed_edge: bool,
    /// `GetUVCornerBoundaries`, `GetStartPointUVLoopGap` and `sm_CalcDistAndDev` return failed curve/surface evaluations and point drops instead of reading unwritten or earlier outputs (WR-262, WR-263, WR-264, #521).
    pub topology_failed_evaluations_propagate: bool,
    /// `SetProps_Stage3` returns a failed `ClassifyLoops` status before reading its outputs, and a failed stage drops its in-progress marker so a retry reruns it (WR-268, WR-266, #521).
    pub heal_stage3_failures_propagate: bool,
    /// `CreateCurveSweep` starts a planar path's sweep frame with its z on the plane's normal, not on `MakeUnitOrthoVectors` of the start tangent, so a flat cap stays flat to the rounding (WR-407, #434; SolidCraft change, tangency-labs/smlib 1b6f5fa).
    pub sweep_frame_on_planar_path_normal: bool,
    /// `CoverCoincidentSurface` refuses a cover that extends a B-spline-based surface onto a continuation whose normal vanishes or turns back (a fold) over the domain the merge would use (WR-407, #434; SolidCraft change, tangency-labs/smlib 1b6f5fa).
    pub cover_refuses_folded_continuation: bool,
    /// A boolean vertex takes its position from the intersection curve that crosses its edge most steeply (WR-410, #1059).
    pub boolean_vertex_by_best_crossing: bool,
    /// `SmMerge::BooleanPostProcess` also checks the merged face's edges that a face merge left with that face on both sides (slits), not only the intersection edges (WR-304, #1120).
    pub boolean_cleanup_removes_slits: bool,
    /// `SmFace::CurveOnClassify` classifies an interval whose two ends lie on one face edge (or its vertices) and whose midpoint is within the intersection tolerance of it as on that edge (the fork's `ClassifyEdgeInterval`, WR-305, #1057).
    pub classify_edge_interval: bool,
    /// `sm_GetEllipseTrimIntervals` evaluates the two seam lines on the cone's whole height (only their sweep from the given domain), as the end circles and the classification are, so a face trimmed along its axis keeps an ellipse whose only seam crossing lies in its trimmed-off part (WR-490, #871; the SolidCraft fork's fix; `plan/upstream-issues/WR-490--ellipse-trim-seam-lines-on-trimmed-domain.md`).
    pub ellipse_trim_seam_lines_span_cone_height: bool,
    /// `SmProtoEdge::Resolve` treats a proto edge from a proto vertex back to itself whose ends find one vertex definition as degenerate below twice the proto vertex's containment radius, and `ImprintIntersections` fits that vertex to its edges (WR-306, #1058).
    pub degenerate_proto_edge_in_one_vertex: bool,
    /// `MakeEdgeInFaceWithVU` grows each end vertex's tolerance to twice its gap to the new edge's end (only if larger), after `RefreshTolerance` keeps it as it was, so `AssertValid` does not refuse the edge (WR-469, #870; the SolidCraft fork's fix; `plan/upstream-issues/WR-469--make-edge-in-face-vertex-tolerance-gap.md`).
    pub new_edge_vertex_tolerance_covers_gap: bool,
    /// `SmBrep::Transform` changes no tolerance under a scale of 1 (a rotation or reflection), and under a scale s sets each vertex, edge and face tolerance to the larger of its own times s and the new body tolerance, not every one to the body tolerance (WR-507, #1520; tangency-labs/smlib#49).
    pub transform_scales_each_tolerance: bool,
    /// `SmBrep::Transform` takes a scaled body's model size from the body's bounding box (vertices, edges and faces, not grown by tolerances), not from its vertices alone, so a body with no vertices (a lone torus) does not get a zero tolerance (WR-508, #1521; tangency-labs/smlib#50).
    pub transform_model_size_from_body_box: bool,
    /// A torus rebuilt from its NURB measures its generating interval signed (WR-408, #1112).
    pub torus_signed_gen_curve_interval: bool,
    /// The `SmTorus` constructor sets `m_bInsideOut` from its `bInsideOut` (upstream only orients the generating circle with it), and a torus made, rebuilt or read with this fix keeps the flag in its copies (upstream's copy constructor resets it), so the surface and its generator agree (`SmSurfOfRevolution` Test #11 `"InsideOut GenCurve"`) (WR-500, #1490).
    pub torus_keeps_inside_out: bool,
    /// `SmTorus::IsNurbSurfaceTorus` measures the major radius from the torus origin (the minor circle's centre projected onto the common line), not from the first sweep circle's centre, which lies off the equator when the NURB's tube does not start there (WR-501, #1470; tangency-labs/smlib@6136a04).
    pub torus_major_radius_from_origin: bool,
    /// `SmTorus::CreateIsoParametricCurve` takes a minor circle's direction from its midpoint when its start lies on the axis, not from the start point again, so a horn torus's minor iso circles get a direction and an analytic domain (WR-504, #1514; tangency-labs/smlib#40).
    pub torus_iso_circle_axis_from_midpoint: bool,
    /// `SmTorus::IsNurbSurfaceTorus` and `SmSphere::IsNurbSurfaceSphere` keep the NURB they tested (rebuilding the analytic data from it, as `SetFromGwNurb` does), not the one made from the fitted circles (~1e-11 off), and the sphere test takes a half meridian within `180 + 360 * SM_EFF_ZERO` degrees, not `180 + SM_EFF_ZERO` (WR-493, #1433; tangency-labs/smlib@7ea6a96).
    pub found_torus_sphere_keep_tested_nurb: bool,
    /// `SmBSplineSurface::HasInternalPole` tests the row through each interior knot (`knots - 1 - degree`), not one row further for each repeated copy of the knot, so a horn torus's corner rows on its axis are not taken for an internal pole (WR-505, #1515; tangency-labs/smlib#41).
    pub internal_pole_row_through_knot: bool,
    /// A sheet sweep's open-profile wires are glued where the curves meet (WR-409, #1113).
    pub sheet_sweep_joins_profile_wires: bool,
    /// `FindPolygonGuess` keeps a suspicious control-net triangle guess as a candidate against the nearest control point's, and returns the nearer one on the surface, so a rational surface whose control points are farther apart than its neighbouring parts does not lose a good guess (WR-444, #1164).
    pub point_solve_keeps_triangle_guess: bool,
    /// `SmFilletVertex::CalcSetBackVert` measures a corner setback from the vertex `GetStartVertex()` marks over the edge's own interval, not from `GetVertex()` over the curve's natural interval, so an edge whose primary edge use runs against its curve is not measured from its far end (WR-445, #1169).
    pub fillet_setback_from_start_vertex: bool,
    /// `CreateSphereFromArcs` also makes the sphere through four arcs of one centre and radius that close a loop (a corner where four equal rounds meet, such as a pyramid's apex), where upstream's "only works for 3 now" refuses and the corner falls back to an approximating blend (WR-502; SolidCraft oracle case 37).
    pub fillet_corner_sphere_from_four_arcs: bool,
    /// `SmEdgeDefinition::Imprint` first moves each end of an intersection edge's interval to the curve's point nearest its proto vertex when that point lies inside the interval, so the edge does not run past its vertex along a face rim (screw-thread groove booleans: "Face has partially Coincident Edges", `SmFace` test #34; WR-452, #435, #410; tangency-labs/smlib b43876f). The end goes to its proto vertex's position only when that proto vertex is one vertex in each B-rep, else to its own final vertex (WR-498, #1480; tangency-labs/smlib ffaf4e8).
    pub imprint_trims_edge_to_proto_vertex: bool,
    /// `SmEdgeDefinition::Imprint` continues an intersection edge's 3d and UV curves, each by the polynomial of its own end span, to a proto vertex that lies a few tolerances past the curve's end, so the edge reaches its vertex (internal thread cuts; WR-453, #410; tangency-labs/smlib a8caccd). The end goes to its proto vertex's position only when that proto vertex is one vertex in each B-rep, else to its own final vertex (WR-498, #1480; tangency-labs/smlib ffaf4e8).
    pub imprint_extends_edge_to_proto_vertex: bool,
    /// `SmFilletSolver::ComputePointValues` chooses its finite-difference step for each point (1e-3 at a tangent point, 1e-5 otherwise), not once per process as the function-local static `sdStaticStep`, so a blend does not depend on the blends before it ([`Kernel::fillet_point_step`] is then neither read nor set, and an existing latch is kept as it is) (ND-3, #1173; the fork's tangency-labs/smlib@0a05860).
    pub fillet_point_step_per_point: bool,
    /// `SmBSplineSurface::CopyAndAddAnalytics` keeps an exact copy where the recognised analytic surface does not give the same points at the same parameters (7 x 7 grid), so a face split by `MakeEdgeInFaceWithVU` keeps trims that still lie on its edges (WR-450, #387; the fork's tangency-labs/smlib@276b509).
    pub analytic_copy_keeps_parameterization: bool,
    /// `SmFilletSolver::ComputePointValues`' bigger-step retry of the forward finite-difference step solves with the bigger step (`dBigStep`), not with the small `dStep` again (WR-456, #1181).
    pub fillet_point_bigger_step_retry: bool,
    /// A fillet's trace also runs through the filleted edge's end where a rail ends on an extended edge, so the surface reaches that end (#1022, WR-468).
    pub fillet_through_extended_rail_end: bool,
    /// `SmFace::ComputePreciseProperties` scales the volume and moment error bounds by the body size (ten times the root of the estimated area, or the origin's distance when larger), not 1.0.1's tenth of it, so the integration converges at tight accuracies instead of returning `SM_ERR` (WR-472; the SolidCraft fork's tangency-labs/smlib@073d4740).
    pub precise_properties_body_length_scale: bool,
    /// `SmFace::ComputePreciseProperties` integrates each UV trim curve piece by piece between its knots and its crossings of the surface's u knot lines, and each iso-u integral of `SmODEAreaIFEO` piece by piece between the surface's v knots, so no Runge-Kutta step crosses a knot of a B-spline surface (a NURB cylinder's, sphere's or torus's rational-circle double knots) where the integrand's derivatives jump and the step's error estimate does not bound its error; the requested accuracy then holds at 1e-8 (WR-494, #1435; the SolidCraft fork's tangency-labs/smlib@08d9a14).
    pub precise_properties_split_at_surface_knots: bool,
    /// An offset surface evaluated beyond its base's domain extends the base itself on every side that can grow at once, each by the domain size times the least power of two that reaches the point and never less than the extension it replaces, so two copies of one offset give the same points whatever was evaluated first (WR-458, #419; the fork's tangency-labs/smlib@2d7c7ed).
    pub offset_extension_from_base: bool,
    /// `N_FillNSidedHole` (through `SmBSplineSurface::CreateCornerBlend`) shortens the cross-boundary derivative across an inner curve where its cubic blend from the opposite boundary would leave that boundary's tangent plane, so a corner patch (a fillet setback corner) stays inside planar faces it meets, with G1 kept (WR-461, #1170). Curved neighbours are not covered.
    pub corner_blend_stays_inside_neighbours: bool,
    /// `DoFilleting`'s `BooleanPostProcess` removes none of the target's own edges, nor an edge between two of its faces (the fork's `m_sPostProcessKeptEdges`/`Faces`), so the patches of a sphere imported in six faces are not merged into a face no patch's parameterization can carry (WR-459, #423; the fork's tangency-labs/smlib@9ddf4a8).
    pub fillet_cleanup_keeps_target_faces: bool,
    /// `SmFilletNx1Corner::MakeCornerTopology` skips a side edge whose faces meet tangentially at the corner's vertex (a flat top running smoothly onto a round) round the vertex to the edge past it, so the end face is the one beyond the tangent seam (the slot wall), not the round the fillet never meets, and `SmFilletEdge::CalcEdgeOnExtendedEdge` intersects the side face with the end face's extension when the side face is not one of the edge's own faces (WR-457, #418; the fork's tangency-labs/smlib@d5ebcb3).
    pub fillet_nx1_tangent_side_edge: bool,
    /// `SmFilletNx1Corner::MakeCornerTopology`'s tangent side-edge skip (`fillet_nx1_tangent_side_edge`, which must be on too) also passes, once, the filleted edge's G1 continuation across the vertex of the opposite convexity (past any tangent edges): the overhang's rim continuing the base circle of a cylinder standing over a block's edge. The edge past it, of the filleted edge's convexity, is the side edge extended and the face beyond it the end face, so the round ends there instead of failing with status 1001 (WR-499, #1474; the fork's tangency-labs/smlib@420e47d).
    pub fillet_nx1_opposite_continuation_side_edge: bool,
    /// `TrimExtendedEdge` takes, on a closed curve, the arc that runs where its base edge does, not always the interval between its two trim points: a hemisphere's rim, two arcs of one circle, no longer becomes one arc twice (WR-463, #403; the fork's tangency-labs/smlib@fa7ff4a).
    pub offset_closed_curve_trim_arc: bool,
    /// `MateEdges` also intersects analytic copies of the two faces' surfaces (plane, cylinder, cone, sphere, torus; extended only along a plane or a cylinder's or cone's rulings, where the tangent (G1R) extensions still are the surface) and replaces each marched piece that lies on an exact line or conic between the same ends, and drops to one UV curve on each extended surface within the body's tolerance, by that exact curve, so a shelled cylinder's floor circle, seam line and rim stay exact (WR-495, #1453/#1130; the fork's tangency-labs/smlib@fddc366 and @98ab1b0).
    pub offset_analytic_mate_edges: bool,
    /// `DoExtendedOffset`'s protective stitch makes one pass at the merged body's tolerance instead of `StitchIntoSolid`'s doubling up to a 200th of the body's size, which welds the rims of cut-off pieces `MakeManifold` removes next into one edge of a far larger tolerance (WR-455, #855; the fork's tangency-labs/smlib@601d804).
    pub offset_stitch_at_body_tolerance: bool,
    /// `SmCurve::GlobalCurveIntersect` moves a point solution to an end of either curve when that end lies within tolerance of the other curve and the curves stay within tolerance of each other over the whole stretch between (`sm_MoveContactsToCurveEnds`), so a tangential contact is not placed inside the curve (WR-455, #855; the fork's tangency-labs/smlib@601d804).
    pub curve_contacts_at_curve_ends: bool,
    /// `SmOffsetExecutive::ShellBrep` of a solid inward fails when the inset leaves no cavity (an empty offset, `SM_ERR_INVALID_INPUT`) or the offset is not a closed solid (`SM_ERR`), instead of handing the input back unhollowed as a success (WR-454, #380; the fork's tangency-labs/smlib@fbe43bb).
    pub shell_empty_cavity_fails: bool,
    /// `create_fillet_brep` refuses a blend whose rolling ball does not fit the faces it rolls on (a face curves tighter than the ball at a section of the edge: `BallFitsItsFaces`) before the corners, with `FilletErrorType::NoRoom` (error info code 8, not an upstream code), where the corners fail with the generic status looking for a rail end that is not there (WR-462, #1172; SolidCraft change, diff `2c5baae..e5696ca` in the issue).
    pub fillet_refuse_ball_not_fitting: bool,
    /// `SmMerge::BooleanLists` copies its surface operands with the virtual `Copy` (the derived surface, its natural domain and its kind), where upstream's base copy constructor slices every surface to a geometry-less `SmSurface`, so a surface list always fails with `SM_ERR` and the planar 2D path is never taken (WR-196; `plan/upstream-issues/WR-196--booleanlists-slices-derived-surfaces-into-base-smsurface.md`).
    pub boolean_lists_virtual_surface_copies: bool,
    /// `SmSphere::IntersectWithPlane` and `IntersectWithSphere` return a single touching point only when the intersection circle's own radius (`sqrt(R^2 - d^2)`, or the two-sphere circle's height, each formed from the gaps to tangency) is within the tolerance, not whenever the gap to tangency is, which collapsed circles of radius up to about `sqrt(2 R tol)` (1e-3 for `R = 1`, `tol = 1e-6`) into one point (WR-79 part 1, #396; the fork's tangency-labs/smlib@30d25bf).
    pub sphere_small_circle_not_touching: bool,
    /// `SmAdvSurfaceIntersector::FindInteriorCurves` seeds a Newton search for isolated tangential contact points (zeros of `S1 - S2` with parallel normals, 4x4 system with the exact Jacobian) from the leaf pairs it cannot trace (normals within 5 or 2 degrees, no local intersection), plus the poles, and adds each contact with a numerically zero gap and normals within 1e-4 rad that is not on a curve already found as a degenerate `TOUCHING` curve (WR-74, #394; the fork's tangency-labs/smlib@c26edbc). Higher-order contacts (equal curvature) are not found.
    pub ssi_tangent_contact_points: bool,
    /// `SmSurfaceIntersector` traces an open tangent curve once: `AddStartPoint` keeps one start point per cluster of tangent boundary start points (`IsSameTangentCluster`), a finished trace removes the clusters at its ends (`DoIntersection`), `TraceCurve` runs its "already traced" test on a first step that snapped onto another start point, and a near-tangent trace stops at the free end of a curve already found instead of running over it (WR-76, #392; the SolidCraft fork's tangency-labs/smlib@0ce09f2).
    pub ssi_trace_tangent_curves_once: bool,
    /// `SmFilletIntersector::ComputeNextPoint` rejects a next point that is more than half a step short of where the step aimed, when the step was not clipped: where `StepSolve`'s plane-adjusting retry lands on another root of the sphere equations (a variable round whose law is evaluated past the end of its edge), the span passes the Hermite test along a straight trace, but its rail parameter advances by the whole step over a fraction of it, and the rail doubles back (WR-503).
    pub fillet_trace_rejects_short_step: bool,
}

/// Options for [`Kernel::new`].
#[derive(Debug, Clone, PartialEq)]
pub struct KernelOptions {
    /// Tolerances to use.
    pub tolerances: Tolerances,
    /// Seed for the kernel's [`Rng`].
    pub seed: u64,
}

impl Default for KernelOptions {
    fn default() -> Self {
        Self {
            tolerances: Tolerances::default(),
            seed: Rng::DEFAULT_SEED,
        }
    }
}

/// Per-session kernel state passed to operations (upstream `SmContext`).
///
/// A `Kernel` is `Send` (it can move between threads) and holds no global state, so
/// independent kernels never interfere.
#[derive(Debug)]
pub struct Kernel {
    /// Geometric tolerances.
    pub tolerances: Tolerances,
    /// Deterministic random-number generator.
    pub rng: Rng,
    /// Memoisation caches.
    pub caches: Caches,
    /// The session's modeling context (marks, Boolean flag, callbacks, its own
    /// model size estimate); created with the defaults, as `SmApiGetOrCreateContext`.
    pub context: Context,
    /// The function-static `sdStaticStep` of `SmFilletSolver::ComputePointValues`
    /// (`SmFilletSolver.cpp`, unit `blend.fillet_solver_p4`): `None` until the first
    /// finite-difference step of a fillet point evaluation latches it (to `1e-3` when
    /// that first point is a tangent point, else `1e-5`); every later evaluation on
    /// this kernel reuses the latched value, as upstream does for the whole process.
    /// A [`Cell`] so the latch can be set through the `&Kernel` the solvers take
    /// (the kernel stays `Send`). Difftest runners seed it with the value the C++
    /// process has latched; see [`Kernel::latch_fillet_point_step`].
    pub fillet_point_step: Cell<Option<f64>>,
    /// The upstream undefined-behaviour conditions (their `plan/upstream-issues.md`
    /// ids, e.g. `"UB-175"`) that operations on this kernel met and went past with the
    /// port's defined semantics, each listed once in the order first met. Upstream's
    /// result for such an input is undefined, so a comparison against the C++ (the
    /// difftest) reads this to classify the case; no operation reads it.
    pub upstream_ub_passed: Vec<&'static str>,
    /// Not upstream: whether `make_curve_cache` keeps a memo of each curve cache in the
    /// curve value, and `CoincidenceCheck` a memo of each surface's boundary curves in
    /// the surface value (`true` unless the `curve-memo-off` feature is on). The memos
    /// never change a result, only the time; `false` builds everything fresh, to check
    /// that.
    pub memoize_curve_caches: bool,
    /// Opt-in fixes of upstream defects (all off: upstream behaviour).
    pub fixes: UpstreamFixes,
    /// Not upstream: the default-record map of the last `kernel_named_edit`
    /// (`topology::brep::p3`), kept only to reuse its allocation; every edit resets it
    /// to the records of a fresh seeding, so it never carries state between edits.
    #[doc(hidden)]
    pub notify_records: crate::topology::topology::NotifyRecords,
}

impl Default for Kernel {
    fn default() -> Self {
        Self::new(&KernelOptions::default())
    }
}

impl Kernel {
    /// Creates a kernel from options.
    #[must_use]
    pub fn new(options: &KernelOptions) -> Self {
        Self {
            tolerances: options.tolerances,
            rng: Rng::new(options.seed),
            caches: Caches::default(),
            context: Context::default(),
            fillet_point_step: Cell::new(None),
            upstream_ub_passed: Vec::new(),
            memoize_curve_caches: !cfg!(feature = "curve-memo-off"),
            fixes: UpstreamFixes::default(),
            notify_records: crate::topology::topology::NotifyRecords::new(),
        }
    }

    /// Records that an operation went past upstream undefined behaviour `id` (see
    /// [`Kernel::upstream_ub_passed`]).
    pub fn note_upstream_ub(&mut self, id: &'static str) {
        if !self.upstream_ub_passed.contains(&id) {
            self.upstream_ub_passed.push(id);
        }
    }

    /// `static double sdStaticStep = first;`: the latched value of
    /// [`Kernel::fillet_point_step`], latching `first` when nothing is latched yet.
    #[must_use]
    pub fn latch_fillet_point_step(&self, first: f64) -> f64 {
        if let Some(step) = self.fillet_point_step.get() {
            return step;
        }
        self.fillet_point_step.set(Some(first));
        first
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Kernel>();
    }

    #[test]
    fn fillet_point_step_latches_once() {
        let k = Kernel::default();
        assert_eq!(k.fillet_point_step.get(), None);
        assert_eq!(k.latch_fillet_point_step(1.0e-5), 1.0e-5);
        assert_eq!(k.latch_fillet_point_step(1.0e-3), 1.0e-5);
        k.fillet_point_step.set(Some(1.0e-3));
        assert_eq!(k.latch_fillet_point_step(1.0e-5), 1.0e-3);
    }

    #[test]
    fn default_tolerances_match_smmath() {
        let t = Tolerances::default();
        assert_eq!(t.zone_tol_3d, 1.0e-5);
        assert_eq!(t.xsect_tol_3d, 2.0e-5);
        assert_eq!(t.approx_tol_3d, 5.0e-6);
        assert_eq!(t.ang_tol_rad(), 0.5 * crate::math::PI / 180.0);
    }

    #[test]
    fn splitmix64_reference_sequence() {
        // Reference values of SplitMix64 seeded with 0 (Vigna's splitmix64.c).
        let mut r = Rng::new(0);
        assert_eq!(r.next_u64(), 0xE220_A839_7B1D_CDAF);
        assert_eq!(r.next_u64(), 0x6E78_9E6A_A1B9_65F4);
        assert_eq!(r.next_u64(), 0x06C4_5D18_8009_454F);
    }

    #[test]
    fn rng_is_deterministic_and_in_range() {
        let mut k1 = Kernel::default();
        let mut k2 = Kernel::new(&KernelOptions::default());
        for _ in 0..1000 {
            let a = k1.rng.next_f64();
            assert_eq!(a, k2.rng.next_f64());
            assert!((0.0..1.0).contains(&a));
            let s = k1.rng.next_signed_unit();
            assert_eq!(s, k2.rng.next_signed_unit());
            assert!((-1.0..1.0).contains(&s));
        }
        k1.caches.clear();
    }
}
