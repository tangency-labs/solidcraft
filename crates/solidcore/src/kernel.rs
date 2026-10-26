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
    /// `CalcRailIntEdgeuse`'s retry rail search holds a variable round's radius only where its first search does (at a corner calculated as constant radius) (WR-300, #1023).
    pub fillet_retry_radius_as_first: bool,
    /// `TestRollOver` joins a rail's neighbouring intervals on its face or the face's boundary before the rollover, so a rail along its face's rim does not fail "Unable to Process Rollover" (WR-301, #485).
    pub fillet_rail_rim_on_face: bool,
    /// The fillet executive refuses a blend whose rails leave the faces they roll on (`RailsFitTheirFaces`, no room) (WR-302, #1024/#841).
    pub fillet_refuse_rails_leaving_faces: bool,
    /// `create_fillet_brep` refuses blends that take the whole of a face they roll on (`FacesOutlastTheirBlends`, no room) (WR-320, #879).
    pub fillet_refuse_face_consuming_blends: bool,
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
    /// `SweepProfilesAlongPath` asks for caps only when the profile closes (WR-400, #837).
    pub planar_path_uncapped_open_profiles: bool,
    /// A full-turn rotational sweep of a multi-curve profile makes lines and arcs their analytic surfaces (WR-401, #545).
    pub revolve_analytic_profile_faces: bool,
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
    /// An inward shell mates the walls across rounds thinner than it (collapsed faces) (#397).
    pub offset_collapsed_rounds_mated: bool,
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
    /// The Boolean's topology intersector treats a self-loop proto edge whose ends a vertex definition on a vertex combined (`CanCombineVertexDefinitions`) as degenerate, instead of `SmProtoEdge::Resolve` splitting it onto that one vertex, after which `SmMerge::MakeFaceuse` returns `SM_ERR` (WR-214, #480; `plan/upstream-issues/WR-214--smprotoedge-resolve-splits-a-self-loop-proto-edge-onto-one.md`).
    pub proto_edge_self_loop_on_vertex_degenerate: bool,
    /// `SM_BO_SLICE` keeps a coincident pair whose primary face bounds material on one side only when that material lies on the side the sheet's `SM_OT_SAME` faceuse bounds (step 7.4d's rule), instead of falling through to the difference rule, which keeps a face lying in the slicing sheet in both parts (WR-254, #528; `plan/upstream-issues/WR-254--manifoldboolean-keeps-a-face-lying-in-the-slicing-sheet-in.md`).
    pub slice_coincident_faces_sheet_side: bool,
    /// After step 8, a Union, Difference or Intersection whose result is a manifold solid with more than two regions (the infinite region first) runs `SetRegionIsVoidFlagsForNestedSolids`, which puts each region's enclosing shell first (the fork's tangency-labs/smlib `abastran/void-shell-check`). It also rewrites the void flags by nesting from the infinite region, so where step 7 left flags that disagree with the nesting the result's void flags change too, not only its shell order (WR-265, #388; `plan/upstream-issues/WR-265--manifold-boolean-void-shell-first.md` and its 2026-10-26 addendum).
    pub boolean_outer_shell_first: bool,
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
    /// `SmProtoEdge::Resolve` treats a proto edge from a proto vertex back to itself whose ends find one vertex definition as degenerate below twice the proto vertex's containment radius, and `ImprintIntersections` fits that vertex to its edges (WR-306, #1058).
    pub degenerate_proto_edge_in_one_vertex: bool,
    /// A torus rebuilt from its NURB measures its generating interval signed (WR-408, #1112).
    pub torus_signed_gen_curve_interval: bool,
    /// A sheet sweep's open-profile wires are glued where the curves meet (WR-409, #1113).
    pub sheet_sweep_joins_profile_wires: bool,
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
    pub notify_records: std::collections::BTreeMap<
        crate::geom::attribute::ObjectKey,
        crate::topology::topology::Topology,
    >,
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
            notify_records: std::collections::BTreeMap::new(),
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
