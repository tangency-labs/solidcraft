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
//! | [`CancelToken`], [`Kernel::cancel`], [`Kernel::set_cancel_token`], [`Kernel::check_cancelled`] | (none: cooperative cancellation for worker-thread jobs) |
//! | [`WorkCounters`], [`Kernel::work_counters`] | the SolidCraft fork's `SmWorkCounters` (process-wide there) |
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

use core::cell::{Cell, RefCell};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use crate::error::SolidError;
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
/// Clearing must never change results, only speed.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct Caches {
    /// Not upstream (#2568): the results of `SmBSplineCurve::AssertValid`'s global
    /// property analyses by the exact content of their inputs, so a curve checked again
    /// is not solved again. A [`RefCell`] so the check can fill it through the `&Kernel`
    /// it takes (the kernel stays `Send`; it is not shared between threads).
    pub(crate) curve_analysis: RefCell<crate::nurbs::b_spline_curve::analysis::AnalysisMemo>,
}

impl Caches {
    /// Drops all cached data.
    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// Counts of the work done by the steps of a boolean, a blend or a tight box whose cost once
/// grew with the square of the body's size, so a test can check how that work grows: the same
/// on every machine, where a ratio of times is not (the SolidCraft fork's `SmWorkCounters`,
/// tangency-labs/smlib@2e9e6b2 and @edf7414).
///
/// Each count is added where its work is done; counting changes no result. The fork's
/// counters are process-wide atomics; these belong to their [`Kernel`] (no global state)
/// and are [`Cell`]s, so operations that take `&Kernel` can add to them and the kernel
/// stays `Send`. Only [`WorkCounters::reset`] zeroes them.
#[derive(Debug, Default)]
pub struct WorkCounters {
    face_pairs: Cell<u64>,
    loops_classified: Cell<u64>,
    wall_body_copies: Cell<u64>,
    proto_edge_intersections: Cell<u64>,
    face_extreme_control_points: Cell<u64>,
    feet_tm_coefficients: Cell<u64>,
    mass_iso_evaluations: Cell<u64>,
    coincident_curves_classified: Cell<u64>,
    containment_edge_drops: Cell<u64>,
    loop_walks: Cell<u64>,
    target_record_seedings: Cell<u64>,
    classification_snapshots: Cell<u64>,
    orient_boxes: Cell<u64>,
}

impl WorkCounters {
    /// Pairs of faces of the two B-reps examined for intersection
    /// (`SmWorkCounters::eFacePairs`): each pair the face loop of
    /// `TopologyIntersector::intersect_topology` visits.
    #[must_use]
    pub fn face_pairs(&self) -> u64 {
        self.face_pairs.get()
    }

    /// Inner loops of a face classified against the new face that a closed loop
    /// imprinted in it makes (`SmWorkCounters::eLoopsClassified`): each inner loop
    /// `make_edge_in_face_with_vu` goes on to classify after the box test of
    /// [`UpstreamFixes::boolean_skips_faces_apart`] (every inner loop with that flag off).
    #[must_use]
    pub fn loops_classified(&self) -> u64 {
        self.loops_classified.get()
    }

    /// Copies keep features makes of the body a feature's walls lie in (the sheet of
    /// rounds for a boss, the target for a cut) to read the walls' boxes and creases from.
    /// Not the fork's.
    #[must_use]
    pub fn wall_body_copies(&self) -> u64 {
        self.wall_body_copies.get()
    }

    /// Curve intersections a boolean ran to split its proto edges where they cross
    /// (`SplitIntersectingProtoEdges`): pairs of lines whose boxes are apart are not
    /// counted, as no intersection runs for them (#14329). Not the fork's.
    #[must_use]
    pub fn proto_edge_intersections(&self) -> u64 {
        self.proto_edge_intersections.get()
    }

    /// Control points of the B-spline nets the trimmed-face tight box's directed solves
    /// ran on (`face_directed_max`, WR-588), summed over its solves: a solve's control-net
    /// guesses read every control point of the surface it is given. Each cell of a
    /// B-spline face is solved on its own subpatch, so the count grows with the cells'
    /// spans, not with the cells times the whole net. Not the fork's.
    #[must_use]
    pub fn face_extreme_control_points(&self) -> u64 {
        self.face_extreme_control_points.get()
    }

    /// Series coefficients of G the G^n certificate's feet Taylor models compute
    /// (`blend::fillet_section::feet_tm::gn_enclose_tm`): each position `(sigma^i, eps^k)`
    /// of the system's series that an evaluation computes, its order-by-order solve's and
    /// its last. Not the fork's.
    #[must_use]
    pub fn feet_tm_coefficients(&self) -> u64 {
        self.feet_tm_coefficients.get()
    }

    /// Integrand evaluations of the precise mass properties' iso-curve integrals
    /// (`OdeIsoCurveLength::evaluate`): each point of a surface line the face integration
    /// samples. A face's bands between its v knots are integrated each on its own, so the
    /// count grows with the knot spans a face crosses, not with their square. Not the
    /// fork's.
    #[must_use]
    pub fn mass_iso_evaluations(&self) -> u64 {
        self.mass_iso_evaluations.get()
    }

    /// Edge curves of two coincident faces that `FaceIntersect` classifies on both: those
    /// of each face within reach of the other face's box. Not upstream (the fork's counters
    /// have none).
    #[must_use]
    pub fn coincident_curves_classified(&self) -> u64 {
        self.coincident_curves_classified.get()
    }

    /// Edge curves the loop containment test (`query::loop::contains_uv_point`) drops the
    /// test point onto: those whose box, grown by the edge's tolerance, reaches the point.
    /// Not upstream (upstream drops it onto every edge of the loop it walks).
    #[must_use]
    pub fn containment_edge_drops(&self) -> u64 {
        self.containment_edge_drops.get()
    }

    /// Walks of a loop's UV trim curves around a point to classify it against that loop
    /// (`SmLoop::ContainsUVPoint`, [`crate::query::r#loop::contains_uv_point`]): each one
    /// drops the point's surface point on every edge curve of the loop and steps along
    /// every trim. Not the fork's.
    #[must_use]
    pub fn loop_walks(&self) -> u64 {
        self.loop_walks.get()
    }

    /// Seedings of a fillet session's notification records over every entity of its target
    /// (`seed_records` on the target, in the merge's tail): each one walks the whole target.
    /// Not the fork's.
    #[must_use]
    pub fn target_record_seedings(&self) -> u64 {
        self.target_record_seedings.get()
    }

    /// Copies of a whole B-rep that curve-on-face classifications run on
    /// ([`crate::query::face::p2::curve_on_classify`]'s snapshot): one per classification
    /// that starts without a shared snapshot. Each copy walks every entity of the B-rep.
    /// Not the fork's.
    #[must_use]
    pub fn classification_snapshots(&self) -> u64 {
        self.classification_snapshots.get()
    }

    /// Bounding boxes of a whole B-rep the orientation of its faces computes for its ray
    /// votes (`orient_trimmed_surfaces`): each one walks every vertex of the B-rep, and one
    /// pass needs one, not one per face. Not the fork's.
    #[must_use]
    pub fn orient_boxes(&self) -> u64 {
        self.orient_boxes.get()
    }

    /// Zeroes every count (`SmWorkCounters::Reset`).
    pub fn reset(&self) {
        self.face_pairs.set(0);
        self.loops_classified.set(0);
        self.wall_body_copies.set(0);
        self.proto_edge_intersections.set(0);
        self.face_extreme_control_points.set(0);
        self.feet_tm_coefficients.set(0);
        self.mass_iso_evaluations.set(0);
        self.coincident_curves_classified.set(0);
        self.containment_edge_drops.set(0);
        self.loop_walks.set(0);
        self.target_record_seedings.set(0);
        self.classification_snapshots.set(0);
        self.orient_boxes.set(0);
    }

    /// Counts one face pair examined for intersection.
    pub(crate) fn add_face_pair(&self) {
        self.face_pairs.set(self.face_pairs.get().saturating_add(1));
    }

    /// Counts one inner loop classified against a new face.
    pub(crate) fn add_loop_classified(&self) {
        self.loops_classified
            .set(self.loops_classified.get().saturating_add(1));
    }

    /// Counts one copy of a body keep features reads feature walls from.
    pub(crate) fn add_wall_body_copy(&self) {
        self.wall_body_copies
            .set(self.wall_body_copies.get().saturating_add(1));
    }

    /// Counts one curve intersection of two proto edges.
    pub(crate) fn add_proto_edge_intersection(&self) {
        self.proto_edge_intersections
            .set(self.proto_edge_intersections.get().saturating_add(1));
    }

    /// Counts the control points of the net one trimmed-face extreme solve ran on.
    pub(crate) fn add_face_extreme_control_points(&self, n: u64) {
        self.face_extreme_control_points
            .set(self.face_extreme_control_points.get().saturating_add(n));
    }

    /// Counts one evaluation of a mass-property iso-curve integrand.
    pub(crate) fn add_mass_iso_evaluation(&self) {
        self.mass_iso_evaluations
            .set(self.mass_iso_evaluations.get().saturating_add(1));
    }

    /// Counts `n` series coefficients a feet Taylor model computed.
    pub(crate) fn add_feet_tm_coefficients(&self, n: u64) {
        self.feet_tm_coefficients
            .set(self.feet_tm_coefficients.get().saturating_add(n));
    }

    /// Counts one edge curve of coincident faces classified on both.
    pub(crate) fn add_coincident_curve_classified(&self) {
        self.coincident_curves_classified
            .set(self.coincident_curves_classified.get().saturating_add(1));
    }

    /// Counts one drop of a containment test's point onto an edge curve.
    pub(crate) fn add_containment_edge_drop(&self) {
        self.containment_edge_drops
            .set(self.containment_edge_drops.get().saturating_add(1));
    }

    /// Counts one walk of a loop around a point.
    pub(crate) fn add_loop_walk(&self) {
        self.loop_walks.set(self.loop_walks.get().saturating_add(1));
    }

    /// Counts one seeding of a fillet session's records over its whole target.
    pub(crate) fn add_target_record_seeding(&self) {
        self.target_record_seedings
            .set(self.target_record_seedings.get().saturating_add(1));
    }

    /// Counts one copy of a B-rep made for a curve-on-face classification.
    pub(crate) fn add_classification_snapshot(&self) {
        self.classification_snapshots
            .set(self.classification_snapshots.get().saturating_add(1));
    }

    /// Counts one whole-B-rep box computed for orienting its faces.
    pub(crate) fn add_orient_box(&self) {
        self.orient_boxes
            .set(self.orient_boxes.get().saturating_add(1));
    }
}

/// Emits the [`UpstreamFixes`] definition it wraps unchanged, plus
/// [`UpstreamFixes::NAMES`], [`UpstreamFixes::flags`], [`UpstreamFixes::all`] and
/// [`UpstreamFixes::flag_mut`]
/// read from that same field list, so a new field is listed by name with no second
/// place to edit (#2014). It
/// accepts only `pub <name>: bool` fields (each with its doc comment), so a field of
/// another type fails to compile instead of being left out of the list.
macro_rules! upstream_fixes {
    (
        $(#[$meta:meta])*
        pub struct $name:ident {
            $(
                $(#[$field_meta:meta])*
                pub $field:ident: bool
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        pub struct $name {
            $(
                $(#[$field_meta])*
                pub $field: bool,
            )*
        }

        impl $name {
            /// The name of every flag (its field name), in declaration order.
            pub const NAMES: &'static [&'static str] = &[$(stringify!($field)),*];

            /// Every flag as `(name, on)`, in declaration order: the names of
            /// [`Self::NAMES`], each with its field's value.
            pub fn flags(&self) -> impl Iterator<Item = (&'static str, bool)> {
                Self::NAMES.iter().copied().zip([$(self.$field),*])
            }

            /// Every flag on: all the opt-in fixes, none of upstream's behaviour where
            /// a fix exists. Unlike [`Default`] (every flag off, the pinned C++), this
            /// includes fixes that are known not to combine cleanly or to be
            /// incomplete; each field's doc and its `plan/upstream-issues/` record say
            /// which.
            pub fn all() -> Self {
                Self {
                    $($field: true,)*
                }
            }

            /// The flag whose field name is `name` (one of [`Self::NAMES`]), to set
            /// or read by name; `None` for any other name.
            pub fn flag_mut(&mut self, name: &str) -> Option<&mut bool> {
                match name {
                    $(stringify!($field) => Some(&mut self.$field),)*
                    _ => None,
                }
            }
        }
    };
}

upstream_fixes! {
/// Opt-in fixes of upstream SMLib defects (`plan/upstream-issues/`), all off by default
/// (upstream behaviour). Set them on [`Kernel::fixes`] before running operations; each
/// field names its record (`WR-…`) and the `tangency-labs/solidcore` issue.
///
/// [`UpstreamFixes::flags`] lists every field with its value and
/// [`UpstreamFixes::NAMES`] every field name, both in declaration order;
/// [`UpstreamFixes::flag_mut`] finds a field by its name; [`UpstreamFixes::all`] turns
/// every field on. `upstream_fixes!` generates
/// them from this definition, so a new field needs no other edit to be listed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
#[non_exhaustive]
pub struct UpstreamFixes {
    /// `SmCurve::ComputeCrvsNormal` uses the sampled polygon's area vector, so a
    /// concave planar loop's normal follows its orientation and its taper drafts
    /// in the requested direction (WR-587, #1776). An open chain closes by its chord.
    /// As the fork's change (tangency-labs/smlib@be45e8d, unchanged to eaa998a), a
    /// failed point evaluation is the call's status and a zero area is
    /// `SM_ERR_INVALID_INPUT`, where upstream's turns give `(0, 0, 1)`.
    pub curve_loop_normal_from_area: bool,
    /// `sm_RefineCCSolutions` judges a range end's shift by its distance from 1 (WR-383, #527).
    pub refine_cc_range_ends: bool,
    /// `SmFace::CurveOnClassify` classifies an interval whose two ends lie on one face edge (or its vertices) and whose midpoint is within the intersection tolerance of it as on that edge (the fork's `ClassifyEdgeInterval`, WR-305, #1057).
    pub classify_edge_interval: bool,
    /// `SmBSplineSurface::CopyAndAddAnalytics` keeps an exact copy where the recognised analytic surface does not give the same points at the same parameters (7 x 7 grid), so a face split by `MakeEdgeInFaceWithVU` keeps trims that still lie on its edges (WR-450, #387; the fork's tangency-labs/smlib@7af1987 (was 276b509)).
    pub analytic_copy_keeps_parameterization: bool,
    /// `SmCurve::GlobalCurveIntersect` moves a point solution to an end of either curve when that end lies within tolerance of the other curve and the curves stay within tolerance of each other over the whole stretch between (`sm_MoveContactsToCurveEnds`), so a tangential contact is not placed inside the curve (WR-455, #855; the fork's tangency-labs/smlib@51eefa5 (was 601d804)).
    pub curve_contacts_at_curve_ends: bool,
    /// `SmFace::CalculateTightBoundingBox` searches a trimmed face's own extreme inside its trim (subdividing the UV domain from the vertex and edge extremes, bounded at depth 16 / 4096 cells, an undecided cell divided at the surface's knot inside it nearest its middle, else at its middle), not only the surface's global extreme, so a face whose surface peaks outside the trim is not boxed too small (WR-588, #1799; tangency-labs/smlib@beb6061 (was 9d44c1b)), nor one whose rational surface has knots off the domain's dyadic points too large (WR-651; tangency-labs/smlib@326de73); only points classified inside the face (`SM_PC_FACE`), not on its edges or vertices, count as the face's, so a rounded box corner's sphere octant is not boxed past its arcs (1.0000243 for 1; tangency-labs/smlib@95bf434).
    pub face_tight_box_searches_trim: bool,
    /// `SmCurve::SimpleCoincidenceChecker` returns a candidate coincidence range whose curves'
    /// separation points to opposite sides at its two ends (both separations longer than
    /// `SM_EFF_ZERO` scaled by the ends' size) as the one crossing `LocalCurveIntersect`
    /// finds inside it, started where the separation taken as linear between the ends
    /// vanishes (`SM_ERR` when none is found). Upstream returns the range: two equal circles
    /// a little apart graze within tolerance over a stretch set by the subdivision, and the
    /// range becomes a tiny edge, so a ring unioned with a half-annulus prism whose r 5 walls
    /// are 1e-4 apart is non-manifold and at 1e-3 not a solid (WR-647, #1979; the fork's
    /// tangency-labs/smlib@c2e3d9d). A range whose separation keeps one side without
    /// vanishing (two curves running close within tolerance without touching, or a
    /// tangency) is the one point where the curves come nearest (`LocalCurveSolve`
    /// minimum from the range's middle); only curves whose separation vanishes coincide
    /// over a range (the fork's 04b42b4).
    pub simple_coincidence_crossing_is_point: bool,
    /// A convex blend whose section between its rails would break through the part (leave its material through a face and come back into it, where one of the faces it crosses there shares no edge with the blended faces: a hollow behind a wall, such as a round on a wall thinner than it reaches into the hollow behind, or a blind pocket or cavity under a blended face, which the pinned construction builds into a body that intersects itself or has the wrong volume) is refused with `FilletErrorType::NoRoom` (`NoRoomReason::BreaksThrough`), naming the largest size whose sections do not break through, and that size less the fit margin is the refusal's fit (`SolidError::FilletRefused`). A section that runs off its face (past a cliff, into a hole or an opening: the part beyond it reaches a rail) or through a hole in a blended face does not break through. The same 32 sections as `BandCutsThePart` are judged; the faces at the blended edge's ends, which the corners trim, are not ([`crate::blend::fillet_executive::session::band_cuts::band_breaks_through_the_part`]). Not the fork's: fork 5e8c9c1 segfaults in `DoFilleting` on #2299's case (WR-707, #2299; addendum `plan/upstream-issues/WR-707--addendum-2026-11-02-rounds-running-off-their-faces.md`).
    pub fillet_refuse_rounds_breaking_through: bool,
    /// With `fillet_cliff_overflow` (and `fillet_refuse_rails_leaving_faces` and `fillet_exact_round_classes`, which it builds on), a lone constant round bigger than the wall it rolls on ends on the body's outline instead of being built on the wall continued past its rim and cut from the body. Where the round's rail runs past a rim of its face (a convex, not tangent edge of the face that runs the filleted edge's whole length) and the ball rolling on the continued face passes inside that rim by more than the body's tolerance (so the round would take material beyond the face), the ball touches the rim instead: it stays tangent to the other face and its section passes through the rim. Where that ball would land on the other face past that face's own rim, it touches both rims, its section the arc through them. The solver rolls the ball on the surface tangent to it along each rim it touches, in place of the face: the face's plane turned about a straight rim, or the wall's cylinder leaned into the cone through a circular rim (both keep the face's parameterization along the rim), so the round is built as a round as tall as its wall, exactly (a cylinder or a torus). Decided in the round's section ([`crate::blend::fillet_outline::outline_supports`]) for a round along a straight edge between two planes, or along a circular edge between a plane square to its axis and a coaxial cylinder; any other round is left to the cliff overflow. A 100 x 100 x 10 plate rounded r 90 on a top edge keeps 86 577.030 mm³, and the round never takes the whole plate: past r = 505 its section runs from the bottom edge to the far top edge. Off: the round on the continued wall, as before (WR-730, #2541; solidcore-native, no fork commit).
    pub fillet_round_ends_on_outline: bool,
    /// A B-rep point classification (`query::brep::ray`, `SmBrep::ClassifyPoint` and
    /// its ray casts) while the body is being edited builds the face's trimmed-surface
    /// tree with the face's current trims implanted, where upstream's
    /// `SmTrimSrfCache::BuildTree` skips the trims while editing and answers against
    /// the untrimmed surface, right only when it reuses a tree built before the edit
    /// (a face trimmed mid-session is classified as its whole surface). Only a face
    /// whose loops close is implanted (`query::brep::ray`'s `loops_close`, reading
    /// the edges' ends through their uses; vertices a stitch's glue is still merging
    /// meet within their tolerances). One whose trims then fail to implant, and one
    /// whose loops are open mid-edit (#2624), is
    /// [`crate::SolidError::FaceTrimsUnavailable`] naming the face, not an answer
    /// against the untrimmed surface. Classifications outside editing are unchanged
    /// (WR-732, #2503; solidcore-native, no fork commit).
    pub brep_ray_classifies_current_trims: bool,
}
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

#[cold]
#[inline(never)]
const fn cancelled_error(operation: &'static str) -> SolidError {
    SolidError::cancelled(operation)
}

/// Shared cooperative cancellation flag for cancelling a [`Kernel`] operation from
/// another thread.
///
/// Cloning a `CancelToken` shares the underlying `AtomicBool`: calling [`Self::cancel`]
/// on any clone signals every [`Kernel`] configured with [`Kernel::set_cancel_token`], so
/// one token can also cancel a group of jobs. [`Self::either`] makes a token that also fires
/// with another one, for work started under two (a dialog's job of a design that may close).
///
/// Not upstream: a token made by [`Self::after_polls`] counts the times it is polled
/// ([`Self::is_cancelled`], [`Self::check`], and so [`Kernel::is_cancelled`] and
/// [`Kernel::check_cancelled`]) and turns cancelled at a given poll. That stops an operation
/// at a point fixed by its own work, not by a clock: for deterministic tests that an
/// operation stops where it is cancelled, and for an application's progress or abort UI
/// (SolidCraft's), which can read how far an operation got ([`Self::polls`]) or stop it after
/// a budget of work. A token made by [`Self::new`] counts nothing and pays nothing for it.
#[derive(Clone, Debug, Default)]
pub struct CancelToken {
    flag: Arc<AtomicBool>,
    /// Not upstream: the token this one also fires with ([`Self::either`]).
    also: Option<Arc<CancelToken>>,
    /// Not upstream: the polls counted, and the one it turns cancelled at
    /// ([`Self::after_polls`]); `None` for a token that counts nothing.
    polls: Option<Arc<PollCount>>,
}

/// The polls a [`CancelToken::after_polls`] token counts.
#[derive(Debug)]
struct PollCount {
    /// The poll the token is cancelled from (1: the first).
    from: u64,
    /// The polls made so far.
    made: AtomicU64,
}

impl PollCount {
    /// Counts one poll: whether the token is cancelled from it on.
    fn poll(&self) -> bool {
        let made = self.made.fetch_add(1, Ordering::Relaxed).saturating_add(1);
        made >= self.from
    }
}

impl CancelToken {
    /// Creates an uncancelled token (`false`).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Signals cancellation (`true`) to every clone of this token.
    pub fn cancel(&self) {
        self.flag.store(true, Ordering::Relaxed);
    }

    /// Clears the cancellation signal (`false`) so the token can be reused. A token made by
    /// [`Self::after_polls`] starts counting its polls again from 0.
    pub fn reset(&self) {
        self.flag.store(false, Ordering::Relaxed);
        if let Some(p) = &self.polls {
            p.made.store(0, Ordering::Relaxed);
        }
    }

    /// Not upstream: an uncancelled token that counts its polls and is cancelled from its
    /// `n`-th on (`n` = 0 or 1: from the first). [`Self::cancel`] cancels it at once as any
    /// token's; `after_polls(u64::MAX)` only counts. Clones share the count.
    #[must_use]
    pub fn after_polls(n: u64) -> Self {
        Self {
            polls: Some(Arc::new(PollCount {
                from: n,
                made: AtomicU64::new(0),
            })),
            ..Self::default()
        }
    }

    /// Not upstream: how many times this token has been polled since it was made or last
    /// [`Self::reset`], for a token made by [`Self::after_polls`]; `None` for one made by
    /// [`Self::new`], which counts nothing. A token made by [`Self::either`] counts as the
    /// token it was made from (`self`): a poll of it is a poll of `self` and of the token it
    /// was joined with, and is counted by each of them that counts.
    #[must_use]
    pub fn polls(&self) -> Option<u64> {
        self.polls
            .as_deref()
            .map(|p| p.made.load(Ordering::Relaxed))
    }

    /// Not upstream: a token that is cancelled when `self` or `other` is. Cancelling it
    /// cancels `self` and not `other`.
    #[must_use]
    pub fn either(&self, other: &CancelToken) -> CancelToken {
        CancelToken {
            flag: Arc::clone(&self.flag),
            also: Some(Arc::new(match &self.also {
                // keep the chain flat in the token's own flag order: self's others, then other
                Some(more) => more.either(other),
                None => other.clone(),
            })),
            // a poll of the joined token is a poll of self: counted, and cancelling, as self's
            polls: self.polls.clone(),
        }
    }

    /// Whether [`Self::cancel`] has been called since construction or the last
    /// [`Self::reset`] (on this token, or on a token it was joined with by [`Self::either`]),
    /// or, for a token made by [`Self::after_polls`], this is its `n`-th poll or a later one.
    /// Every call is one poll of each counting token in the chain: none is skipped because
    /// an earlier one is already cancelled.
    #[must_use]
    #[inline]
    pub fn is_cancelled(&self) -> bool {
        let counted = self.polls.as_deref().is_some_and(PollCount::poll);
        let also = self.also.as_deref().is_some_and(CancelToken::is_cancelled);
        counted || also || self.flag.load(Ordering::Relaxed)
    }

    /// Returns `Err(SolidError::Cancelled { operation })` when [`Self::is_cancelled`] is
    /// `true`, else `Ok(())`.
    ///
    /// # Errors
    ///
    /// [`SolidError::Cancelled`] when [`Self::cancel`] has been signalled.
    #[inline]
    pub fn check(&self, operation: &'static str) -> crate::Result<()> {
        if self.is_cancelled() {
            return Err(cancelled_error(operation));
        }
        Ok(())
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
    /// the surface value (`true` unless the `curve-memo-off` feature is on), and whether
    /// the check's other result-invariant shortcuts are taken (the B-spline curve
    /// analyses' bounds and memo, #2568; face test 37's winding hulls). The memos and
    /// shortcuts never change a result, only the time; `false` builds and runs
    /// everything in full, to check that.
    pub memoize_curve_caches: bool,
    /// Opt-in fixes of upstream defects (all off: upstream behaviour).
    pub fixes: UpstreamFixes,
    /// Optional cooperative cancellation token polled at outer solver, trace, cache,
    /// stitch, heal, integration, fillet, and boolean loops (`None` by default).
    ///
    /// When cancellation fires, operations unwind through normal `Result` paths with
    /// [`SolidError::Cancelled`]; never a partial or altered `Ok` result. Values borrowed
    /// mutably by the cancelled call (such as mutable `Brep` operands or `Kernel` caches
    /// and RNG state) may have been partly modified, as on any other error return.
    pub cancel: Option<CancelToken>,
    /// Not upstream: the default-record map of the last `kernel_named_edit`
    /// (`topology::brep::p3`), kept only to reuse its allocation; every edit resets it
    /// to the records of a fresh seeding, so it never carries state between edits.
    #[doc(hidden)]
    pub notify_records: crate::topology::topology::NotifyRecords,
    /// Not upstream: the tolerance the innermost
    /// [`crate::surface::procedural_blend::NetScope`] asks for (the fork's
    /// `SmBlendNetScope` depth, a `thread_local` there, with the caller's tolerance,
    /// #2510): a procedural blend whose net's certified bound is at most it evaluates as
    /// its net. 0 outside every scope (every blend is its definition), infinite in a scope
    /// that takes every net. A [`Cell`]: the scope is entered and read on paths that hold
    /// `&Kernel` (every surface evaluation).
    pub blend_net_tolerance: Cell<f64>,
    /// Not upstream: the fork's process-wide `SmBlendCounts` (diagnostics only, no
    /// result reads them). A [`Cell`]: the definition's evaluators count through `&Kernel`.
    pub blend_counts: Cell<crate::surface::procedural_blend::BlendCounts>,
    /// Not upstream: the fork's `thread_local` station memo (`t_asMemo`), the stations
    /// the definitions of procedural blends solved last
    /// ([`crate::blend::fillet_section::definition::StationMemo`]). A [`RefCell`]: the
    /// definition is evaluated through `&Kernel`; it is never borrowed across a solve, and
    /// a hit returns what a fresh solve from the same seeds gives.
    pub blend_station_memo: RefCell<crate::blend::fillet_section::definition::StationMemo>,
    /// Not upstream: the arena-identity record map of the last stitch glue or split
    /// (`heal::stitch::p1`) and the B-rep value it was kept for (its progress tag, held
    /// weakly), kept only to reuse its allocation. An edit of another B-rep value starts
    /// on an empty map instead, and every edit resets it to a default record for every
    /// object of its B-rep, so it never carries state between edits.
    #[doc(hidden)]
    pub arena_notify_records: (
        std::sync::Weak<()>,
        crate::topology::topology::NotifyRecords,
    ),
    /// Not upstream: the fork's `thread_local` memo of equal area thumbweights
    /// (`t_sWMemo`, b8ec238, smlib#109;
    /// [`crate::blend::fillet_section::section::ThumbweightMemo`]). A [`RefCell`]: the
    /// stations and the definition solve through `&Kernel`; it is never borrowed across a
    /// solve, and a hit returns what a fresh solve of the same order and u gives.
    pub blend_w_memo: RefCell<crate::blend::fillet_section::section::ThumbweightMemo>,
    /// Not upstream: the fork's `thread_local` count of open blend operation scopes
    /// (`t_lOperationScopes`; [`crate::blend::fillet_section::section::OperationScope`]).
    /// Shared, so that a scope can outlive a borrow of the kernel (a fillet session holds
    /// one while it takes `&mut Kernel`); an atomic keeps the kernel `Send`. Its updates are
    /// `Relaxed`: a kernel runs on one thread at a time (it is `Send`, not shared), so the
    /// count orders nothing between threads; a copy of the kernel (`cache_kernel`) takes its
    /// own count, never this one.
    pub blend_operation_scopes: Arc<core::sync::atomic::AtomicU32>,
    /// Not upstream (the SolidCraft fork's `SmWorkCounters`): counts of the work done by
    /// the steps of a boolean or a blend whose cost once grew with the square of the body's
    /// size, for tests and profiling. They change no result.
    pub work_counters: WorkCounters,
    /// Not upstream (tangency-labs/solidcore#2489): while `Some`, each fillet session that
    /// finishes filleting appends the faces it made in its target, each with what made it
    /// ([`crate::blend::definition_check`]), for the check that every blend face is a
    /// definition. `None` (the default) marks and keeps nothing; it changes no result.
    pub blend_face_log: Option<crate::blend::definition_check::BlendFaceLog>,
    /// Not upstream: the profiler of the operations run on this kernel (feature
    /// `profile`; [`crate::profile`]). It only observes: no result reads it. Shared with
    /// the kernel copies an operation makes for its caches, which record into the same
    /// tree.
    #[cfg(feature = "profile")]
    pub profiler: crate::profile::Profiler,
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
            cancel: None,
            notify_records: crate::topology::topology::NotifyRecords::new(),
            blend_net_tolerance: Cell::new(0.0),
            blend_counts: Cell::new(crate::surface::procedural_blend::BlendCounts::default()),
            blend_station_memo: RefCell::new(
                crate::blend::fillet_section::definition::StationMemo::default(),
            ),
            arena_notify_records: (
                std::sync::Weak::new(),
                crate::topology::topology::NotifyRecords::new(),
            ),
            blend_w_memo: RefCell::default(),
            blend_operation_scopes: Arc::default(),
            work_counters: WorkCounters::default(),
            blend_face_log: None,
            #[cfg(feature = "profile")]
            profiler: crate::profile::Profiler::default(),
        }
    }

    /// What the profiler recorded since the last call (feature `profile`;
    /// [`crate::profile::Profiler::take`]); the records then start afresh.
    #[cfg(feature = "profile")]
    #[must_use]
    pub fn take_profile(&self) -> crate::profile::Profile {
        self.profiler.take()
    }

    /// Installs `token` as this kernel's cooperative cancellation token.
    ///
    /// Inputs borrowed mutably by an operation when `token.cancel()` is observed may be
    /// left partially modified, as on any other error return.
    pub fn set_cancel_token(&mut self, token: CancelToken) {
        self.cancel = Some(token);
    }

    /// Removes any configured cooperative cancellation token (`None`).
    pub fn clear_cancel_token(&mut self) {
        self.cancel = None;
    }

    /// The configured [`CancelToken`], if any.
    #[must_use]
    #[inline]
    pub const fn cancel_token(&self) -> Option<&CancelToken> {
        self.cancel.as_ref()
    }

    /// Polls [`Self::cancel`] (`false` when unset).
    #[must_use]
    #[inline]
    pub fn is_cancelled(&self) -> bool {
        match &self.cancel {
            Some(token) => token.is_cancelled(),
            None => false,
        }
    }

    /// Polls [`Self::cancel`] and returns `Err(SolidError::Cancelled { operation })` when
    /// cancellation has been signalled.
    ///
    /// # Errors
    ///
    /// [`SolidError::Cancelled`] when [`Self::cancel`] is `Some` and cancelled.
    #[inline]
    pub fn check_cancelled(&self, operation: &'static str) -> crate::Result<()> {
        if let Some(token) = &self.cancel {
            return token.check(operation);
        }
        Ok(())
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
    fn all_turns_every_flag_on_and_default_none() {
        assert!(UpstreamFixes::all().flags().all(|(_, on)| on));
        assert!(UpstreamFixes::default().flags().all(|(_, on)| !on));
        assert_eq!(
            UpstreamFixes::all().flags().count(),
            UpstreamFixes::NAMES.len()
        );
    }

    #[test]
    fn kernel_is_send() {
        fn assert_send<T: Send + core::fmt::Debug>() {}
        fn assert_send_sync<T: Send + Sync + Clone + core::fmt::Debug + Default>() {}
        assert_send::<Kernel>();
        assert_send_sync::<CancelToken>();
    }

    #[test]
    fn either_token_fires_with_each_and_cancels_only_its_own() {
        let (a, b, c) = (CancelToken::new(), CancelToken::new(), CancelToken::new());
        let ab = a.either(&b);
        let abc = ab.either(&c);
        assert!(!ab.is_cancelled() && !abc.is_cancelled());
        c.cancel();
        assert!(abc.is_cancelled() && !ab.is_cancelled());
        c.reset();
        b.cancel();
        assert!(ab.is_cancelled() && abc.is_cancelled() && !a.is_cancelled());
        b.reset();
        ab.cancel();
        assert!(a.is_cancelled() && !b.is_cancelled() && abc.is_cancelled());
    }

    #[test]
    fn after_polls_cancels_from_its_nth_poll_and_counts_every_poll() {
        assert_eq!(CancelToken::new().polls(), None);
        let t = CancelToken::after_polls(3);
        assert_eq!(t.polls(), Some(0));
        assert!(!t.is_cancelled());
        assert_eq!(t.check("kernel::test"), Ok(()));
        assert_eq!(
            t.check("kernel::test"),
            Err(SolidError::cancelled("kernel::test"))
        );
        // cancelled from the third poll on, and every poll counted
        assert!(t.is_cancelled());
        assert_eq!(t.polls(), Some(4));
        // a clone shares the count; a kernel's polls are the token's
        let mut k = Kernel::default();
        k.set_cancel_token(t.clone());
        t.reset();
        assert_eq!(t.polls(), Some(0));
        assert!(!k.is_cancelled());
        assert_eq!(k.check_cancelled("kernel::test"), Ok(()));
        assert!(k.is_cancelled());
        assert_eq!(t.polls(), Some(3));
        // from the first poll for 0 and 1; cancel() at once
        assert!(CancelToken::after_polls(0).is_cancelled());
        assert!(CancelToken::after_polls(1).is_cancelled());
        let c = CancelToken::after_polls(u64::MAX);
        assert!(!c.is_cancelled());
        c.cancel();
        assert!(c.is_cancelled());
        assert_eq!(c.polls(), Some(2));
    }

    #[test]
    fn either_counts_a_poll_on_each_counting_token_of_its_chain() {
        let (a, b) = (
            CancelToken::after_polls(u64::MAX),
            CancelToken::after_polls(2),
        );
        let ab = a.either(&b);
        assert!(!ab.is_cancelled());
        // the joined token counts as a, and its poll is one of a and one of b
        assert_eq!(
            (ab.polls(), a.polls(), b.polls()),
            (Some(1), Some(1), Some(1))
        );
        // b's second poll cancels b and the joined token, not a
        assert!(ab.is_cancelled());
        assert!(b.is_cancelled() && !a.is_cancelled());
        assert_eq!((a.polls(), b.polls()), (Some(3), Some(3)));
        // a token that counts nothing, joined with b: b counts the poll, the joined token none
        let plain = CancelToken::new().either(&b);
        assert!(plain.is_cancelled());
        assert_eq!((plain.polls(), b.polls()), (None, Some(4)));
        // a's countdown is the joined token's too, and an earlier cancel skips no count
        let (c, d) = (
            CancelToken::after_polls(1),
            CancelToken::after_polls(u64::MAX),
        );
        let cd = c.either(&d);
        assert!(cd.is_cancelled() && cd.is_cancelled());
        assert_eq!((c.polls(), d.polls()), (Some(2), Some(2)));
    }

    #[test]
    fn cancel_token_round_trip() {
        let mut k = Kernel::default();
        assert!(k.cancel.is_none());
        assert!(k.cancel_token().is_none());
        assert!(!k.is_cancelled());
        assert_eq!(k.check_cancelled("kernel::test"), Ok(()));

        let token = CancelToken::new();
        k.set_cancel_token(token.clone());
        assert!(k.cancel_token().is_some());
        assert!(!k.is_cancelled());
        assert!(format!("{token:?}").contains("false"));
        assert_eq!(k.check_cancelled("kernel::test"), Ok(()));

        token.cancel();
        assert!(k.is_cancelled());
        assert!(format!("{token:?}").contains("true"));
        assert_eq!(
            k.check_cancelled("kernel::test"),
            Err(SolidError::cancelled("kernel::test"))
        );

        token.reset();
        assert!(!k.is_cancelled());
        assert_eq!(k.check_cancelled("kernel::test"), Ok(()));

        k.clear_cancel_token();
        assert!(k.cancel.is_none());
        assert_eq!(k.check_cancelled("kernel::test"), Ok(()));
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
    fn work_counters_count_until_reset() {
        let k = Kernel::default();
        let w = &k.work_counters;
        let counts = |w: &WorkCounters| {
            (
                w.face_pairs(),
                w.loops_classified(),
                w.wall_body_copies(),
                w.proto_edge_intersections(),
                w.loop_walks(),
            )
        };
        assert_eq!(counts(w), (0, 0, 0, 0, 0));
        w.add_face_pair();
        w.add_face_pair();
        w.add_loop_classified();
        w.add_wall_body_copy();
        w.add_proto_edge_intersection();
        w.add_loop_walk();
        w.add_loop_walk();
        w.add_loop_walk();
        assert_eq!(counts(w), (2, 1, 1, 1, 3));
        // each kernel counts its own work
        assert_eq!(Kernel::default().work_counters.face_pairs(), 0);
        w.reset();
        assert_eq!(counts(w), (0, 0, 0, 0, 0));
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
