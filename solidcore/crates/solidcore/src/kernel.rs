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
    ball_certified_roots: Cell<u64>,
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

    /// Ball corners whose centre a certified root proves
    /// (`blend::blend_corner::ball`'s `ball_certified_root`, `sm_BallCertifiedRoot`): a corner
    /// of three equal rounds with fewer than two planes among its faces, whose centre has no
    /// closed form. Not the fork's (a test's witness that a body reaches the solve, #15140).
    #[must_use]
    pub fn ball_certified_roots(&self) -> u64 {
        self.ball_certified_roots.get()
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
        self.ball_certified_roots.set(0);
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

    /// Counts one ball corner's certified root.
    pub(crate) fn add_ball_certified_root(&self) {
        self.ball_certified_roots
            .set(self.ball_certified_roots.get().saturating_add(1));
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
    /// `SmPlane::IsNurbSurfacePlane` places a recognised surface's origin after
    /// orthogonalising its axes (WR-652, upstream issue #1056).
    pub plane_recognition_origin_after_axes: bool,
    /// `SmCurveLoops::GetLoopStartEnd` starts a later loop after the previous loop
    /// ends, keeping adjacency within that loop (WR-96). Off by default for C++ parity.
    pub curve_loop_start_after_previous_end: bool,
    /// The generic temporary-radius constructor reports setup failures before filleting (#458, UB-252 follow-up).
    pub temporary_radius_setup_errors: bool,
    /// `SmFilletGeom::Trim` returns the first curve/surface trim or interval-intersection error instead of dropping it and continuing with an unset interval (WR-604, #483). Earlier successful edits remain; off by default for pinned C++ parity.
    pub fillet_trim_status: bool,
    /// `SmCurve::ComputeCrvsNormal` uses the sampled polygon's area vector, so a
    /// concave planar loop's normal follows its orientation and its taper drafts
    /// in the requested direction (WR-587, #1776). An open chain closes by its chord.
    /// As the fork's change (tangency-labs/smlib@be45e8d, unchanged to eaa998a), a
    /// failed point evaluation is the call's status and a zero area is
    /// `SM_ERR_INVALID_INPUT`, where upstream's turns give `(0, 0, 1)`.
    pub curve_loop_normal_from_area: bool,
    /// The fillet executive refuses a blend whose rails leave the faces they roll on (`RailsFitTheirFaces`, no room) (WR-302, #1024/#841).
    pub fillet_refuse_rails_leaving_faces: bool,
    /// `create_fillet_brep` refuses blends that take the whole of a face they roll on (`FacesOutlastTheirBlends`, no room) (WR-320, #879).
    pub fillet_refuse_face_consuming_blends: bool,
    /// `create_fillet_brep` refuses blends whose bands on a face they share overlap away from any corner that trims them (a rail of one inside the band between the other's filleted edge and its rail: both vertical edges of a box end rounded past half its width), with `FilletErrorType::NoRoom`, where upstream builds a body that is not closed (WR-542; the fork's tangency-labs/smlib@c0d74ee, `BandsClearOtherRails`). A rail is sampled only between its end vertices, where the corners have placed them: the rails are not trimmed yet and run on past their corners' mitres, so a run-on crossing another blend's band refused a valid round (WR-649, #1984; the fork's tangency-labs/smlib@d548305).
    pub fillet_refuse_overlapping_bands: bool,
    /// The vertex overload of `SmFilletErrorInfo::NoteError` records the vertex, its location, the fillet status, the error code and the comment, as the edge overload does, and `CreateFilletCorners` notes a vertex no corner type handles as `SM_FILERR_CORNER_UNSUPPORTED` with "Corner case not implemented" (WR-513, #1469; tangency-labs/smlib@a73cf5e). The pinned overload records nothing, so the failure reached the caller as the generic 1001 with no reason. Off by default (pinned behaviour). Diagnostics only: the pending vertex entry and the operation's result are unchanged, so this does not make an unsupported corner succeed. The whole-operation wrappers (e.g. `fillet_edges`) drop their session and return only the later failure; the retained note is readable through [`crate::blend::fillet_executive::session::FilletSession::error_info`]. The C API's `SMLIB_ERR_CORNER_UNSUPPORTED` (-1111) is not mapped here.
    pub fillet_vertex_error_diagnostics: bool,
    /// The fillet solver tries the corner ball's patch after the torus when a round's centre line collapses to a point (WR-420, #416).
    pub fillet_corner_ball_patch: bool,
    /// With `fillet_corner_ball_patch`, `CalcSphereFilletGeom` centres the corner ball where the two cylinders' axes meet (their nearest points, within the tolerance of each other), instead of requiring every trace point's centre within tolerance of the first: at the seam's end both offset cylinders collapse onto their axes and the last point's solve stops a step short of the crossing, so the ball was refused and the seam round failed (WR-541; the fork's tangency-labs/smlib@bc6ddf9).
    pub fillet_corner_ball_center_from_axes: bool,
    /// `SmFilletCorner::MakeVariableRadiusFillet` refuses to turn a four-sided corner into a variable-radius fillet when the new fillet's rail along the opposite fillet leaves that fillet's surface (more than ten times the corner's tolerance from its natural domain): the rail replaces the opposite edge, which bounds the opposite fillet's face, so that face would end off its own edge. The corner is then the patch on its exact edges. At a drafted pocket's top corners the rail ran 0.42 off the corner round at R3, 10 degrees (WR-645, #968).
    pub fillet_corner_variable_radius_rail_on_opposite_fillet: bool,
    /// `CalcTorusFilletGeom` revolves a linear section into a cone (WR-421, #420).
    pub fillet_chamfer_cone_on_arc: bool,
    /// With `fillet_chamfer_cone_on_arc`, `CalcTorusFilletGeom` stores a linear section's revolution as the `SmCone` it is: the revolution's NURBS form is copied (`SmBSplineSurface::Copy`) and passed through `CopyAndAddAnalytics`, and when that gives a cone it is the fillet surface and the rails are made on it (as a straight edge's round is stored as its cylinder); anything else stays the surface of revolution. Without it a chamfer round a hole's rim is a surface of revolution, which reads back as a rational B-spline, not the cone it is (WR-572, #1503; the fork's tangency-labs/smlib@b5743ec).
    pub fillet_chamfer_cone_stored_as_cone: bool,
    /// `SmFilletCorner::CreateCornerPatch` marks the patch sides that lie on a face of the body (a fillet edge with an original face) and `sm_CreateCoonsSrf` gives a corner between such a side and a blend's side the blend side's twist instead of the mean of the two: a bicubic Coons patch meets a side's cross-boundary derivative only where the corner twists are the other sides' own, so the mean tilted the patch off the face's tangent plane beside that side and it crossed the face (0.037 out of a bar's side face where the bar's top-edge round meets the concave round at a cylinder it runs tangent into, r 2). Corners between two face sides or two blend sides keep the mean (WR-613, #1836; the fork's tangency-labs/smlib@7fd9fb0 (commit not in fork history at 26cc451), from the issue's diff).
    pub corner_patch_face_side_twist: bool,
    /// The fillet solver decides whether a constant round is exact by one classification of its spine (the curve the ball's centre runs on) from its faces' kinds and placement, read as the natural quadrics they lie on (plane, sphere, circular cylinder or cone, an offset of one): a line (two planes; a plane or cylinder parallel to a cylinder's axis) is built as the cylinder, a circle about a common axis (coaxial quadrics, a plane square to the axis, a sphere's centre on it) as the section turned about that axis, a point (two cylinders as wide as the ball, axes meeting) as the ball's piece; every other round is traced. This replaces upstream's try-and-fall-through chain (`CanMakeAnalytic`, planes and cones only, then cylinder, torus and with `fillet_corner_ball_patch` the sphere, each deciding from the edge's shape) at `CalcFilletGeom` and at `ReCalcFilletGeom` (where a line round is rebuilt as its cylinder, not the cone), and a builder's failure is then an error, not a trace. So a round on a sphere is exact, and a circular round's frame comes from the faces' axis, not the edge's `IsArc` (WR-669; the fork's tangency-labs/smlib@b5f2867 and 7ac037e). The torus and ball builders' own flags still apply inside them: the fork's behaviour is this with `fillet_chamfer_cone_on_arc`, `fillet_chamfer_cone_stored_as_cone` and `fillet_corner_ball_center_from_axes` on (a chamfer round a circle is traced without `fillet_chamfer_cone_on_arc`). A conic spine (a plane with a cylinder or cone at any other angle; two cylinders or cones touching a common ball the same way round) is built as the exact rational surface of the ball's arcs, checked against the ball before it is kept (the fork's tangency-labs/smlib@0061c41 and 1b5f1e7); the fork's nodal-quartic rounds are classified but still traced here. A line round's cylinder is built on its exact spine, the line its offsets meet in, its rails on the faces' feet of it, and keeps that frame instead of the stations' centres and a frame refitted from its net (#2405, solidcore-native).
    pub fillet_exact_round_classes: bool,
    /// `SmFilletExecutive::CreateFilletBrep` stitches the fillet body keeping a fillet's own edge where an edge of corner-patch faces only meets an edge of other fillet faces only (`SmFilletPatchStitchCallback`): an N-sided corner patch is bounded by its surfaces' isoparametric curves, the approximations it was fitted to, and the default choice between the two near-equal distances left one half of a set-back section the exact arc and the other the polynomial approximation, off the round (WR-546, #1377; the fork's tangency-labs/smlib@1072d1b).
    pub fillet_patch_keeps_fillet_edges: bool,
    /// `SmFilletGeom::TrimOriginalSurfaces` takes the piece of an original face 'inside' a fillet's rail (the one to delete) as the piece whose binormal along the rail makes the largest positive dot with the fillet's (above `SM_EFF_ZERO_SQRT`), not any piece above a fixed 0.2: the pieces' binormals are opposite, and 0.2 refused any fillet meeting its face at more than 78 degrees. A knife edge's chamfer meets the flat face at 81 degrees at d 0.5 (dot 0.15): nothing was deleted and the chamfer face was left apart from the body (WR-611, #1809; the fork's tangency-labs/smlib@de4dae6).
    pub fillet_trim_inside_by_best_dot: bool,
    /// A knife edge (a cusp: faces tangent along the edge that fold back on each other,
    /// their binormals pointing the same way) is rounded with the ball touching both faces
    /// back from the edge: `SmEdge::GetBlendEdgeuse` no longer takes it for a smooth edge,
    /// the constant radius, constant distance and variable radius solvers offset each face
    /// to the side the other lies on, from the difference of the faces' normal curvatures
    /// across the edge where the faces are tangent (`sm_SectorSides`), and the Nx1
    /// corner's degeneracy test takes anti-parallel side normals for an ordinary corner.
    /// Upstream refuses the edge as tangent (WR-570; the fork's tangency-labs/smlib@b7ec55b).
    pub fillet_knife_edge_rounds: bool,
    /// `SmEdge::GetBlendEdgeuse` takes two faces for tangent (an edge with nothing to round) only within half a degree of each other, not 2 degrees: a crease of 0.5 to 2 degrees (a draft meeting a flat, a roof crease) is an edge the model means and is rounded like any other. Upstream refuses it (`SM_ERR_INVALID_INPUT` from the fillet) (WR-621, #1808; the fork's tangency-labs/smlib@2e8758b).
    pub fillet_half_degree_crease: bool,
    /// `DoExtendedOffset` finds the excess offset faces after the merge only, not also on the untrimmed faces before it (WR-341, #977).
    pub offset_excess_faces_after_merge: bool,
    /// `SmFilletEdge::FindFilletBrepEdge`, looking for both edges of a fillet edge split in two (a corner's face curve split at the side's middle), keeps the second half when the first half's midpoint matched the curve to scaled zero and the other's only to the edges' tolerance, provided its ends lie on the curve too. Upstream kept such a near edge only for when nothing matched at all, so the second half was never merged into its face and the corner's sheet came out with two free edges (`SMLIB_ERR_NOT_SOLID` for some set-back, eased G2 corners) (WR-612, #1819; the fork's tangency-labs/smlib@ceb3272).
    pub fillet_split_edge_keeps_near_half: bool,
    /// The extended offset (`SmOffsetExecutive::DoExtendedOffset`, which shells and offsets with extended convex edges) caps each vertex of the original with one edge (a cone's apex, the tip of a drilled hole) with the sphere `BuildOffsetOfVertex` builds for it, a face of the whole sphere zone the merge joins to the cone's offset; upstream builds no vertex offsets there, so the cone's offset ends open on the circle the apex's normals sweep out and the shell fails (WR-531; the fork's tangency-labs/smlib@d5dadaa).
    pub offset_point_vertex_caps: bool,
    /// `BuildOffsetOfFaceuse` clips a face to its offset domain from its counterclockwise loops (an inset is not turned inside out) (WR-342, #974).
    pub offset_clip_right_side_out: bool,
    /// `SmOffsetExecutive::MapEdgeuses` raises each offset edge it matches to an original edge to that edge's tolerance, and the offset edge's vertices to the original edge's vertices' largest. Upstream builds the offset faces' edges and vertices afresh at the B-rep's zone tolerance, so the offset of a sheet whose edges and vertices carry their own, larger tolerances (a trimmed B-spline patch: B-rep 2.08e-5, edges 4.15e-5, vertices 8.31e-5) is checked against half the B-rep's tolerance at its vertices, and its offset by 0.75 fails `check_brep` with edge/vertex Bad Gaps of 2.4e-5 to 3.9e-5, inside the original's own tolerances (WR-671, #2111).
    pub offset_edges_keep_source_tolerance: bool,
    /// With extended convex edges, `piecewise_merge` relates the mated offsets of a manifold edge instead of intersecting them again (WR-344, #469; the fork's tangency-labs/smlib@9c21c9a (was 85bbfaf)), except an edge whose mate failed in `EdgeuseOffset`, which is intersected even where it is a tangent sector (#1795; the fork's tangency-labs/smlib@9d8ace9). The merge then no longer refuses some broken offsets, and the difference hands back a wall that bounds nothing (#1818; the fork's `ShellBrep` returns it, its C API refuses it as `SMLIB_ERR_NOT_SOLID`); the shell's refusal of a result that is no closed manifold solid is unconditional, whatever this flag says. Needs no other flag.
    pub offset_extended_edges_mated: bool,
    /// `SmOffsetSurface::SwapUV` negates the distance once for the extended surface and
    /// again for the base, so an owning offset that has a stored extension comes back
    /// with its distance unchanged and lies on the wrong side of its base; with this
    /// fix the base's successful swap keeps the single negation the extended surface's
    /// swap made, and `Offset(uv)` after the swap is the pre-swap `Offset(vu)` (WR-164).
    /// An offset without an extension still negates once. Off by default for pinned C++
    /// parity.
    pub offset_swap_uv_extended_distance: bool,
    /// Thickening a sheet (`ShellBrep` with `bCreateOffsetSolid` on a non-manifold input) puts every planar side wall onto its plane (the fork's `sm_PutPlanarFaceOnItsPlane`) and deletes the old surface's seam it leaves as a slit, where upstream keeps each wall as the ruled surface between the lamina edge and its offset: a tube's end rings are `SmPlane`s with two loops, not `SmSurfOfRevolution`s (WR-564, #1599; the fork's tangency-labs/smlib#62).
    pub thicken_planar_side_walls: bool,
    /// A face a Boolean takes over from its second operand keeps that operand's exact UV trims (WR-361, #488).
    pub keep_operand_uv_trims: bool,
    /// `smgu_EvaluateCurveContinuity`'s G2 test compares the two sides' Frenet curvatures and normals (WR-380, #933/#838).
    pub geometric_g2_continuity: bool,
    /// `SmSurface::FindDegenParamForDirection` compares the actual singular parameter
    /// mapped into `sSIvl` (`s_ivl.evaluate(this_param)` and
    /// `s_ivl.evaluate(near_best_param)`) with the caller's singular parameter `dS` in
    /// the "nearly as good and closer to the input" tie-break, so the nearest candidate
    /// starting guess is chosen on degenerate boundaries whose singular domain is not
    /// `[0, 1]`; pinned v1.0.1 compares the `[0, 1]` sample fractions `dThisParam` and
    /// `dNearBestParam` directly against `dS` (WR-57, #2209;
    /// `plan/upstream-issues/WR-57--addendum-2026-11-01-degen-param-tiebreak.md`). Off
    /// by default for pinned C++ parity.
    pub degen_param_tiebreak_uses_actual_param: bool,
    /// `RemoveCoincidentNoEdgeProtoVertex` reports a miss as `false` (WR-73, #848).
    pub proto_vertex_miss_is_false: bool,
    /// `CheckLoop` measures a gap against at least its vertex's zone tolerance (WR-381, #476).
    pub check_loop_vertex_tolerance: bool,
    /// `CheckLoop` compares an end projected from its vertex (no UV trim) with its neighbour on the nearest natural period of a closed direction, so a one-period seam jump is not an `SM_ERR` (WR-440, #530).
    pub check_loop_projected_seam_ends: bool,
    /// `SmLoopuse::IsClosed3d` reports a loop open when an end-to-end gap exceeds its
    /// cross-section tolerance: the over-tolerance value is `gap - tolerance`, where the
    /// pinned C++ computes `tolerance - gap` (never above its zero initial maximum, so
    /// it always reports closed). The reported gap is the actual gap, with its edge use
    /// and tolerance; `bCheckAllGaps == FALSE` still stops at the first over-tolerance
    /// gap (WR-93). Off by default for pinned C++ parity; callers inherit the kernel's
    /// setting.
    pub loop_use_reports_open_gaps: bool,
    /// `SmEdge::AssertValid` test 9 orders the faces round an edge by the direction to each face's point 20 edge tolerances into it (square to the edge) where two of them leave the edge within `SM_ANG_TOL_DEG` of each other, instead of by the binormals alone, which tie for faces tangent along the edge (a cylinder resting on a plane, kept by a boolean as a four-face edge) and fail the valid order (WR-540, #383; the fork's tangency-labs/smlib@5533bbb).
    pub edge_check_orders_tangent_faces: bool,
    /// `SmEllipse::AdjustSTEPInterval` rejects inverted STEP edits and collapsed or non-finite converted NURBS intervals before mutation, and revolution edits install their domain only after their generator accepts it (WR-211, #428).
    pub ellipse_step_interval_checks: bool,
    /// The N-dimensional local solve takes no value from a failed evaluation (WR-253; the fork's tangency-labs/smlib@d2757e1). The one-dimensional solver does this by default since #2574, so this flag no longer reaches it. `SmLocalSolveNd::SolveIt` sums no equations of a failed clamped or scaled evaluation, takes the clamped-and-scaled step only when it evaluated, and then with that evaluation's Jacobian and found flag (upstream kept the unscaled point's), stops the line search when the clamped step failed and no scaled one was taken, and finds no solution when the best guess cannot be re-evaluated (#1945), nor when the guess's first evaluation fails, where upstream reports the unevaluated guess as found (WR-20; the fork's tangency-labs/smlib@d5c5cfd).
    pub oscillating_skips_failed_guesses: bool,
    /// `sm_RefineCCSolutions` judges a range end's shift by its distance from 1 (WR-383, #527).
    pub refine_cc_range_ends: bool,
    /// `SmSurface::GetFlatCorners` tests each corner bit instead of clearing the others (WR-384, #525).
    pub flat_corners_test_bits: bool,
    /// `SmSurfaceCache::GetSurfaceContinuity` checks the last internal V knot (`iv + 1 < rVConts.GetSize()`) rather than skipping it (`iv + 1 < rVConts.GetSize() - 1`) (WR-42).
    pub surface_continuity_checks_last_v_knot: bool,
    /// `sm_GoodDropResult` (`SmSurface::DropPoint` given a guess) refuses a local solve's result when the result is farther from the point than its guess, as its comment says; upstream measures its two distances the wrong way round (`dDsqGuess` to the result, `dDsqResult` to the guess), so it refused a result nearer than its guess, sending the drop to the global solve, and took one farther. The same fork change also takes a local result as the foot only when the gap's components along the unit tangents are within the local solve's tight tolerance, `SM_EFF_ZERO (1 + max|P|)` (their sum under it; an interior result each under it), where upstream takes any foot 1e-4 off (an interior one 1e-3 off in each direction); a stalled solve goes to the global solve (WR-56, #1852; the fork's tangency-labs/smlib@eaa998a (was 0b4de8c) / eaa998a, fork PR #70).
    pub good_drop_result_distances_in_order: bool,
    /// `SmTwistVolume::EvaluateSimple` uses positive `per * cos` for the y
    /// component of `Duw` (WR-46). Propagated through volume evaluation options.
    pub twist_simple_duw_sign: bool,
    /// `smvol_LiftThirdDerivative` (surface and volume forms) uses `Dxzz` instead of
    /// `Dxxz` for the `x z^2` outer-derivative term in `Duuv`, `Duvv`, `Duuw`, `Duww`,
    /// `Dvvw`, and `Dvww`, and builds the volume form's `Duww` and `Dvww` from
    /// `Fuw`/`Fw` and `Fvw`/`Fw` instead of `Fuv`/`Fv` and `Fvv`/`Fv` (WR-45, #2165;
    /// see `plan/upstream-issues/WR-45--addendum-2026-10-31-lift-third-derivative-terms.md`).
    /// Propagated through `EvalOptions::fixes` across compound volume and
    /// `SrfInVolume` / `CrvInVolume` evaluations. Off by default for pinned C++ parity.
    pub volume_lift_third_derivative_terms: bool,
    /// `SmSurface::DropCurve` tests the drop domain's own V length for `bDegenSurfV`, not the U length again, so a surface whose natural domain is degenerate only in V reports `SM_ERR_DEGENERATE_SURFACE` (`SM_CTC_DEGENSURFACE_FAIL`) and its failure record sets `bDegenSurfV` from V; upstream copies the U test, misses the early exit and goes on to drop (WR-100, 2026-10-31 addendum). Off by default for pinned C++ parity.
    pub drop_curve_v_domain_degeneracy: bool,
    /// `SmTorus::AssertValid` reports a spindle torus whose V interval runs through an apex (`V = ±(90 + asin(R/r))`) as test 5, "Self Intersection", where upstream clears its result with no report and tests the U interval against `90 + acos(R/r)`, failing every spindle torus with a whole turn of U (WR-539; the fork's tangency-labs/smlib@daba678).
    pub torus_check_spindle_apex: bool,
    /// Volume line-domain checks using `SmExtent3d::ContainsLineSeg3d` require
    /// both endpoints inside the natural parameter box instead of either (WR-3).
    pub extent3d_contains_both_segment_endpoints: bool,
    /// `SmSurface::LocalSurfaceIntersect`'s out-of-bounds boundary retries guard both
    /// the minimum and maximum boundary tests of the second through fourth parameter
    /// branches (`v` of `this`, `u` of `other`, `v` of `other`) with `!bFoundSolution`,
    /// matching step 1, instead of binding `!bFoundSolution` only to the minimum test
    /// via `!bFoundSolution && min || max` and re-running `sm_IntersectSrfBndryCrv` at
    /// a maximum boundary where a failed solve overwrites an already-found boundary
    /// solution (WR-99, #2210;
    /// `plan/upstream-issues/WR-99--addendum-2026-11-01-guard-max-boundary-retry.md`).
    /// Off by default for pinned C++ parity.
    pub local_surface_intersect_guards_max_boundary_retry: bool,
    /// `SmCone::RebuildSTEPFromNURBParameters` gives a cylinder the mean of near-equal radii (semi-angle 0) where upstream's own copy within `SM_EFF_ZERO` does not apply, as the fork's 5df5b27 (WR-385, #713).
    pub cylinder_rebuild_mean_radius: bool,
    /// `SmCylinder::IsNurbSurfaceCylinder` gives an inside-out NURB (its iso circles turning against the line through their centres) the frame `IsNurbSurfaceCone` gives one: Z against the NURB's linear direction, origin at its top-knot circle; upstream keeps the bottom-knot circle and the NURB's direction, so the cylinder's Z is reversed and its STEP evaluation is mirrored in height (WR-496, #1463).
    pub nurb_cylinder_inside_out_frame: bool,
    /// `SmPlane::Reparameterize` moves the placement origin to `sOriginMax` when the UV domain
    /// is reparameterized, maintaining consistency between analytic plane and NURB evaluations
    /// across shifted domains; upstream computes `sOriginMax` but only references it under
    /// `SM_DEBUG_CODE` (WR-41). Off by default for pinned C++ parity.
    pub plane_reparameterize_moves_origin: bool,
    /// `SmCurveBoundedSurface::ReadFromDB` dispatches each UV boundary curve on its own
    /// stream header type (`lUVType`) instead of reusing the last 3D boundary's type
    /// (`l3DType`, `0` when there are no 3D boundaries), and passes the read
    /// `bImplicitOuter` flag to the constructed `SmCurveBoundedSurface` instead of
    /// dropping it and defaulting to `false` (WR-43, #2164; see
    /// `plan/upstream-issues/WR-43--addendum-2026-10-31-read-uv-type-and-implicit-outer.md`).
    /// Off by default for pinned C++ parity.
    pub curve_bounded_surface_read_uv_type_and_implicit_outer: bool,
    /// `PositionProfileCurves` returns the first failing status of its two
    /// `SmAxis2Placement::SetCanonical` calls, its three profile `Transform` calls and
    /// `MakeUnitOrthoVectors`, instead of ignoring them and positioning the profiles
    /// with the partial frame: a path start tangent whose squared length overflows
    /// (a `1e300` component) unitizes to zero, `MakeUnitOrthoVectors` fails `SM_ERR`,
    /// and upstream then transforms the profiles by an invalid partial frame (WR-632,
    /// #483). The edits made before the first failure stay (no rollback); the identity
    /// `SetCanonical` on the profile-normal frame (always successful upstream) is
    /// elided. Off by default for pinned C++ parity.
    pub planar_path_profile_placement_statuses: bool,
    /// A rotational sweep (whole or partial turn) makes each line and circular arc of its profile its cylinder, cone, sphere or torus, not only a one-curve profile's (WR-448, #545; SolidCraft change, tangency-labs/smlib 0b143f7 (commit not in fork history at 26cc451)).
    pub revolve_analytic_profile_faces: bool,
    /// A taper extrude with linear-extension (tangent) corners of a profile of lines and circular arcs with a sharp corner continues each wall as its tangent plane to meet its neighbour along their exact seam (a line's wall is its own plane, so two lines meet at a corner edge), and refuses a corner it cannot close (`create_taper_extrude_with_corner` reports why and where), instead of ruling separate corner faces from offset extension lines; with fillet corners, or a profile with another curve kind, nothing changes (WR-521, #1449; SolidCraft change, tangency-labs/smlib e8f23e0).
    pub taper_tangent_corner_planes: bool,
    /// A full-turn rotational sweep makes a closed shell inside another solid its cavity (WR-403, #478).
    pub revolve_nested_shells_as_cavities: bool,
    /// With `revolve_nested_shells_as_cavities`, a full-turn rotational sweep whose stitch left one shell in the infinite region but nested the others itself (a profile with a hole: the hole's ring lies inside the ring around it, in that ring's region) sets every region's void flag from the nesting (`SetRegionIsVoidFlagsForNestedSolids`) when every shell is a closed face-use shell, so the hole's ring is a void, not material (WR-529; the fork's tangency-labs/smlib@38b4212).
    pub revolve_holed_profile_void: bool,
    /// A partial-turn rotational sweep first splits each profile curve where it meets the axis inside itself (`IntersectWithLine` on the axis at the B-rep tolerance, as `CheckCurveSweep` finds them), joining a closed curve's last piece back onto its first; upstream's `CheckCurveSweep` refuses such a curve (`SM_SC_ROT_ON_CURVE`) and the sweep fails with `SM_ERR`, so a whole circle tangent to the axis (a horn torus) turned part way fails unless its seam lies on the axis (WR-590; the fork's tangency-labs/smlib@e4f1377).
    pub revolve_splits_curves_on_axis: bool,
    /// `SmTrimmingTools::OrderCurvesIntoLoops`, at a vertex where several curve ends meet (a hole whose corner touches the outline), goes on from the end (of a curve it has not used, or its first curve's start) whose leaving tangent comes next round the vertex from the arriving curve's, counterclockwise about one normal for the whole list, from the curves' area vectors and turned against the loops' area, so loops touching from outside each other close apart (ends leaving tangent to each other in the order they bend; tangency-labs/smlib@b0ee3f9). Upstream takes the first such end by index in the input, so the input order decides whether the loop closes, ends on a curve it already used (`SM_ERR`) or runs round the hole the wrong way, crossing itself and counting the hole as material (WR-591; the fork's tangency-labs/smlib@9209e76).
    pub loops_go_on_round_the_vertex: bool,
    /// `SmTrimmingTools::MirrorCurve` keeps the Euclidean control points `GetCanonical`
    /// returns instead of dividing each one by its weight a second time, so a rational UV
    /// curve is reflected rather than moved and distorted (WR-98; see
    /// `plan/upstream-issues/WR-98--addendum-2026-10-31-euclidean-mirror.md`). Off by
    /// default: the pinned C++ divides twice.
    pub mirror_curve_keeps_euclidean_control_points: bool,
    /// A curve sweep along a closed path whose end sections lie on its start sections
    /// (within the B-rep's tolerance) leaves both caps off, so the stitch joins the side
    /// faces' end edges to their start edges: one closed tube, where upstream caps both
    /// ends anyway and leaves two coincident caps, a second region, and a body booleans
    /// cannot use (tangency-labs/solidcore#2192; SolidCraft #2005).
    pub curve_sweep_closed_path_joins_ends: bool,
    /// `CreateLinearSweep` makes a circular arc short of a whole turn that is swept square to its plane an `SmCylinder` bounded 0..its sweep angle by 0..the distance, its frame turned to the arc's start (for a sweep against the arc's normal, to its end and then flipped), where upstream makes a cylinder only of a whole circle and an `SmSurfOfExtrusion` of any other arc; a sweep that is not square stays a surface of extrusion (WR-565, #1477; SolidCraft change, tangency-labs/smlib 12918e9).
    pub linear_sweep_arc_is_cylinder: bool,
    /// Planar faces from curves join a loop touching another loop at a vertex into that loop (WR-405, #892).
    pub join_touching_loops: bool,
    /// `sm_CheckEdgeuseGeometry` evaluates the clockwise neighbour in UV space with
    /// `SmEdgeuse::NormalizedEvaluate(.., TRUE, ..)`, whose `GetOrCreateUVTrimCurve`
    /// stores a new UV trim curve on that neighbour: a `const` validation
    /// (`SmBrep::ValidatePointers`, e.g. from `SmBrepConstructor::EndShell(TRUE)`) then
    /// changes the B-rep it is asked to check (WR-446;
    /// `plan/upstream-issues/WR-446--brepconstructor-endshell-validatepointers-makes-uv-trim-curves.md`).
    /// With this set the neighbour is evaluated with `ok_to_make_uv_trim_curve = FALSE`:
    /// the 3D point is still evaluated and dropped onto the existing face, but no trim
    /// is made; the self evaluation, the existing trims and the statuses are unchanged.
    /// Off by default for pinned C++ parity. This is not the WR-406 trim-reversal check:
    /// validation still repairs existing trims as before.
    pub validation_does_not_make_uv_trims: bool,
    /// `SmBrep::ValidateAndUpdateTolerances` grows a vertex's tolerance when one of its
    /// edge use's vertex/edge or vertex/face gaps is over half the tolerance, the test its
    /// report applies. Upstream grows it only past the whole tolerance, so the update
    /// mode reports a gap between the two as bad and never fixes it (WR-679, #2111; the
    /// fork's tangency-labs/smlib@8c4bda5 by bherbruck).
    pub validate_tolerances_fixes_reported_gaps: bool,
    /// `SmApiOffsetBrepFull` and `SmApiShellBrepFull` (offset, shell and thicken) run
    /// `SmBrep::ValidateAndUpdateTolerances(FALSE)` on their result before returning it,
    /// so its vertex and edge tolerances cover the gaps its construction left and it
    /// passes the check without the caller running the update. Upstream returns the
    /// result with the tolerances its faces were built at (a patch sheet's offset by 0.5
    /// fails `check_brep` with 4 vertex/edge Bad Gaps even with
    /// `offset_edges_keep_source_tolerance`). Neither upstream nor the fork has this
    /// call; the update grows a vertex for a gap between half its tolerance and all of
    /// it only with `validate_tolerances_fixes_reported_gaps` (WR-705, #2111).
    pub offset_updates_result_tolerances: bool,
    /// Before the piecewise merge, `do_extended_offset` rebuilds every offset face whose loops cross from the stretches of its loops that bound it (WR-345, #854).
    pub offset_resolve_crossing_loops: bool,
    /// [`crate::topology::edgeuse::p2::is_convex_radial_sector`]
    /// (`SmEdgeuse::IsConvexRadialSector`) propagates a failing
    /// [`crate::topology::edgeuse::p2::evaluate_binormal_step_off`] status on either this
    /// edge use or its radial partner as `Err` instead of returning `Ok(true)` (pinned
    /// v1.0.1's `SER(...)` inside an `SmBoolean` function returns the non-zero `SmStatus` as
    /// `TRUE`, so a step-off that leaves a face's UV domain or a failed binormal evaluation
    /// falsely classifies even a concave sector as convex; `WR-84`,
    /// `plan/upstream-issues/WR-84--addendum-2026-11-01-propagate-step-off-failure.md`).
    /// Off by default for pinned C++ parity.
    pub edgeuse_convexity_propagates_step_off_failure: bool,
    /// `SmBrep::LocalOperation` sizes the offset executive's distance (by which it extends the neighbouring surfaces to meet the moved ones) from a box that also holds the replacement surfaces, not from the original body alone, so a face moved further than about the body's size away is still reached (WR-534; the fork's tangency-labs/smlib@a1ee273).
    pub local_operation_box_holds_new_surfaces: bool,
    /// With `offset_resolve_crossing_loops` (it has no effect without it), loops of an offset face that touch without crossing (run along one another, meet tangentially, or come up to one another and go back on the same side) are refused, before any crossing is resolved, with `SM_ERR_OFFSET_WALLS_TOUCH` (1042) instead of `SM_ERR_BAD_INTERSECTIONS` (1027): the offset walls meet along a line (WR-345 addendum, #1442; tangency-labs/smlib a759622).
    pub offset_walls_touch_refused: bool,
    /// An inset's `do_extended_offset` makes manifold only the finite regions of the merged offset that are cavity, not every finite region (upstream's own TODO there: "we will need to select regions to keep"): a region is cavity where one of its faces has the body's face the wall's thickness behind it and none has the body's face that far ahead. Where a floor is no thicker than the wall its two faces' offsets cross and the merge closes the region between them; kept, the cavity was no manifold solid and the shell failed with `SM_ERR` (1001) (WR-667; the fork's tangency-labs/smlib@37af643).
    pub offset_keep_cavity_regions: bool,
    /// In a local operation (`SmBrep::LocalOperation`), `SmOffsetGeometryCreation::ExtendTrimEdges` fails a tangent or lamina edge whose lift onto one face's new surface drops onto the other's farther than ten times `max(the offset edge's tolerance, the approximation tolerance)`, and `EdgeuseOffset` passes that failure on, so the operation is refused; upstream ignores it ("will be fixed later", which no later step of a local operation does) and returns a body with an edge as loose as the gap between a moved face and its unmoved tangent neighbour (WR-530, #402; the fork's tangency-labs/smlib@23c5b0a). Offsets and shells are unchanged.
    pub local_operation_refuses_loose_tangent_edges: bool,
    /// A local operation that moves one of two tangent faces along its normal builds the step: `EdgeuseOffset` finds the two faces parallel but apart across the edge (`IsStepAcrossEdge`) and builds the ruled wall between their edges (`CreateStepWall`, rails of uniform weight made non-rational), kept out of the face extensions; `CloseLoopGaps` closes the gaps the wall leaves at its ends and the edges running past its corners are split; a vertex moved past an edge's other end is `SM_ERR_OFFSET_EDGE_VANISHES` (1043) (WR-530 addendum; tangency-labs/smlib 5d428ae, 90aabff, 34318f4).
    pub local_operation_step_walls: bool,
    /// With `local_operation_step_walls`: an edge whose vertex a local operation moves onto its other end, where its other vertex is, has closed to a point and is squeezed out of its offset face (`CloseLoopGap`); once every gap is closed the offset faces that closed to nothing (`SmFace::IsDegenerate`, a step's wall whose low top moved onto the high top's plane) are deleted and forgotten, and the merge joins the faces either side. A closed edge on no such face or across from one is still `SM_ERR_OFFSET_EDGE_VANISHES` (WR-566; tangency-labs/smlib ccbf77a). In the face move (`offset::offset_faces`), the edges then left between a moved face and a face that stayed, on one surface, which `IsTopological` finds removable, are removed, so the tops either side of the closed wall are one face (WR-566 addendum; tangency-labs/smlib 9504880).
    pub local_operation_closes_walls_to_nothing: bool,
    /// An inward shell mates the walls across rounds thinner than it (collapsed faces) (#397).
    pub offset_collapsed_rounds_mated: bool,
    /// `FaceuseOffset` fails on a face whose offset surface is missing without collapsing (`SmSurface::OffsetCollapses`: a cone, cylinder, sphere or torus offset onto its spine), where upstream reads any missing surface as a collapsed round (WR-464, #1072).
    pub offset_missing_surface_fails: bool,
    /// A surface of revolution whose generatrix is one circular arc (a fillet's round along a circular edge, a torus in all but its type) collapses when an offset runs towards the arc's centre by its radius or more: `Surface::offset_collapses` says so, as a torus's does, and `create_offset_surface` builds no surface, where upstream says it never collapses and builds the generatrix's offset past the arc's centre as the arc turned inside out, so an inward shell thicker than the round fails with 1001 or leaves a wrong volume (WR-567; the fork's tangency-labs/smlib@8b2090f, smlib#64; SolidCraft `shell_of_a_disc_rounded_smaller_than_the_wall`, solidcraft#319). The faces either side are mated across it with `offset_collapsed_rounds_mated`.
    pub revolution_round_offset_collapses: bool,
    /// At an Nx1 corner whose end rib cannot be built because a rail missed a side edge ending within the fillet's width, the rib is deleted and the global merge cuts the round off (WR-303, #841).
    pub fillet_nx1_end_rib_global_merge: bool,
    /// `sm_FixUVTrimCurves` moves only the end control points of the two UV trims meeting at a corner onto the vertex's UV point, without dropping the vertex onto them, trimming there and stretching them back over the edge interval (WR-255, #959/#570; the fork's tangency-labs/smlib@e5606fb (was 5af7070)).
    pub uv_trim_corner_end_points_only: bool,
    /// `BSplineCurve::trim` removes short end spans only for model-space curves
    /// (dimension 3), preserving exact UV trims in surface parameter units
    /// (WR-441, #1114). A short UV end span whose control points all lie within check
    /// test #15's pole tolerance (`SM_ZONE_TOL_3D / 10`) is still removed as upstream
    /// removes it, so the trim passes the check (#2129). Off by default for pinned SMLib
    /// compatibility.
    pub bspline_trim_preserves_uv_spans: bool,
    /// `BSplineCurve::trim` keeps a short end span of a rational curve (its control points carry weights, `N_IsCrvRat`: a conic's NURB) instead of removing it with `N_CrvRemoveDegenSegs`, which stretches the remaining spans over its length and so moves a conic off its circle or ellipse (a trimmed half circle of radius 16 by about 1e-6, enough for its edge and vertex gaps to pass the tolerance); non-rational curves are cleaned as upstream (WR-575, #1529). An end span whose control points all lie within check test #15's pole tolerance (`SM_ZONE_TOL_3D / 10`) is not kept: the trim ends at its interior knot instead, on the conic and within that tolerance of the requested end, so the curve passes the check (#2090). Off by default for pinned SMLib compatibility.
    pub bspline_trim_keeps_rational_end_spans: bool,
    /// The Boolean's topology intersector treats a self-loop proto edge whose ends a vertex definition on a vertex combined (`CanCombineVertexDefinitions`) as degenerate, instead of `SmProtoEdge::Resolve` splitting it onto that one vertex, after which `SmMerge::MakeFaceuse` returns `SM_ERR` (WR-214, #480; `plan/upstream-issues/WR-214--smprotoedge-resolve-splits-a-self-loop-proto-edge-onto-one.md`).
    pub proto_edge_self_loop_on_vertex_degenerate: bool,
    /// `SM_BO_SLICE` keeps a coincident pair whose primary face bounds material on one side only when that material lies on the side the sheet's `SM_OT_SAME` faceuse bounds (step 7.4d's rule), instead of falling through to the difference rule, which keeps a face lying in the slicing sheet in both parts (WR-254, #528; `plan/upstream-issues/WR-254--manifoldboolean-keeps-a-face-lying-in-the-slicing-sheet-in.md`).
    pub slice_coincident_faces_sheet_side: bool,
    /// After step 8, a Union, Difference or Intersection whose result is a manifold solid with more than two regions (the infinite region first) runs `SetRegionIsVoidFlagsForNestedSolids`, which puts each region's enclosing shell first (the fork's tangency-labs/smlib `abastran/void-shell-check`). It also rewrites the void flags by nesting from the infinite region, so where step 7 left flags that disagree with the nesting the result's void flags change too, not only its shell order (WR-265, #388; `plan/upstream-issues/WR-265--manifold-boolean-void-shell-first.md` and its 2026-10-26 addendum).
    pub boolean_outer_shell_first: bool,
    /// Planar path sweeps propagate mandatory operation failures and delete their
    /// owned temporary B-reps on failure (#483); the mitre's `BrepIntersect` and
    /// `GlobalCurveSolve` searches are returned too, as the fork does (tangency-labs/smlib
    /// c8f6a31; changed by #2009 group C, WR-662). Off by default for pinned compatibility.
    pub planar_path_sweep_statuses: bool,
    /// The B-rep's own edits, its `StitchAndOrient`, the offset and shell stitches and the
    /// Boolean merge, stitch and topology-intersection passes pass on the statuses SMLib
    /// drops: a failed list edit, evaluation, trim, stitch or orientation returns its error
    /// instead of the operation carrying on and reporting success (WR-658, #2009 group A;
    /// tangency-labs/smlib 60a8a51, 4803d6d, e7ebc8d). Off by default for pinned parity.
    pub brep_edit_statuses_pass_on: bool,
    /// The loop, vertex, edge use, face, curve and surface queries pass on the statuses
    /// SMLib drops where the call returns a status, and the classifying queries answer
    /// only from what they computed: a value that could not be evaluated is no evidence
    /// (`SM_POC_UNKNOWN`, no G1 meeting, no parallel pair, no dog-leg, not fresh) instead
    /// of one read unset (WR-663, #2009 group D; tangency-labs/smlib 7a35f33, 6997962,
    /// 666242b, 8843967, 8311f8e, 21dcf69). Off by default for pinned parity.
    pub query_statuses_pass_on: bool,
    /// Extrusions, revolutions, sweeps, lofts and the station sweeps (`SmPrimitiveCreation`,
    /// `SmTopologySweep`, the translational and rotational sweep geometry and `SM_API`'s
    /// primitive wrappers) pass on the statuses SMLib drops: a failed evaluation, copy,
    /// transform, trim, reparameterization, knot or degree edit, face or edge construction,
    /// stitch or orientation returns its error instead of the sweep carrying on with an
    /// unset value and reporting success (WR-662, #2009 group C; tangency-labs/smlib
    /// c8f6a31). Complements the path-curve statuses pivot selection and path extension
    /// always return (#483), `planar_path_sweep_statuses` and
    /// `planar_path_profile_placement_statuses`. Off by default for pinned parity.
    pub sweep_statuses_pass_on: bool,
    /// Surface/surface intersection and proto-topology pass on the statuses SMLib drops;
    /// where a helper answers a question it answers "no" when a point it compares cannot
    /// be evaluated, instead of comparing unset values (`IsPointOnCurve`, `AddStartPoint`'s
    /// tangent test, `sm_ComputeCurveType`, `sm_AreSameCurve`, `IsInteriorPoint`,
    /// `sm_IsInterior`, `sm_AreOverlapping`, `sm_WhichPVToCombine`) (WR-662, #2009 group
    /// C; tangency-labs/smlib bcc6f01). Off by default for pinned parity.
    pub ssi_proto_topology_statuses_pass_on: bool,
    /// Analytic `STEPInversion` (sphere, surface of revolution, so cone and cylinder) tells
    /// its answers (off the surface, outside the sweep) apart from real failures, and the
    /// cone and sphere intersection callers that read the location after a failed
    /// inversion pass the failure on instead of reading "exterior" and dropping a point
    /// or curve (WR-662, #2009 group C; tangency-labs/smlib dfc7cb6). Off by default for
    /// pinned parity.
    pub analytic_inversion_answers_apart: bool,
    /// `SmCone::CreateIsoCircleFromPoint` as the SolidCraft fork has it (WR-704;
    /// tangency-labs/smlib 1279539, with 672a437's `SER` on the NURB conversion): the
    /// circle's STEP height is the reference point's projection onto the axis clamped to
    /// the cone's height interval, instead of the height an inversion of the point moved
    /// onto the cone finds, and a failed NURB conversion is returned instead of read.
    /// Past the apex end of a cone whose apex is an end (within the tolerance the call
    /// accepts), that point is on the far nappe: the inversion mirrors it through the
    /// apex and comes back with a height outside the cone, and the circle fails with
    /// `SM_ERR`, failing `SmSphere::IntersectWithCone` for a sphere crossing the cone's
    /// line just past its apex. Other points within the tolerance past an end also get
    /// that end's circle. On an unbounded cone of negative semi-angle the fork's circle
    /// is at the point's axial height over the cosine of the semi-angle, not at that
    /// height; the WR record lists the cases this flag changes. Off by default for
    /// pinned parity.
    pub cone_iso_circle_at_axial_height: bool,
    /// Unbounded `SmCone`s are parameterised as their NURB is (WR-713, #2313). The
    /// unbounded constructor rotates the generator by a negative semi-angle too, not only
    /// a positive one, and a cone of negative semi-angle runs from `-SM_INFINITE_PARAMETER`
    /// up to its apex (above the origin) instead of from the apex up, so it is the cone
    /// through its base circle, not a cylinder of the base radius cut off above the apex.
    /// `ConvertUVFromSTEPToNURBS` and `ConvertUVFromNURBSToSTEP` map the height of an
    /// inside-out unbounded cone onto its NURB's reversed height interval, as for a
    /// bounded cone, instead of negating it: the NURB runs over the STEP heights
    /// reversed, not over their negatives. Upstream's iso circle through a point at
    /// height 1 on an inside-out unbounded cone is then near `SM_INFINITE_PARAMETER`,
    /// its NURB-parameter points disagree with its NURB, and a negative-angle unbounded
    /// cone holds none of its points below the apex. Off by default for pinned parity.
    pub cone_unbounded_parameterized_as_nurb: bool,
    /// The fillet executive, solvers, corners and geometry pass on the statuses SMLib
    /// drops: a failed solver load, orientation, list edit, copy, trim, interval edit,
    /// evaluation, knot edit or split-face update returns its error instead of the fillet
    /// carrying on and reporting success (WR-661, #2009 group B; tangency-labs/smlib
    /// 873c7c9, 66b02de, 9b7cd48). Off by default for pinned parity.
    pub fillet_statuses_pass_on: bool,
    /// `SmDatabaseIO::ReadBoolean` ([`crate::topology::database_io::DatabaseReader::read_boolean`]
    /// on a reader configured via [`crate::topology::database_io::DatabaseReader::with_fixes`] or
    /// [`crate::topology::database_io::DatabaseReader::set_fixes`]) byte-swaps a 4-byte binary
    /// `SmBoolean` when `m_bByteSwap` is active (matching `SmDatabaseIO::ReadBooleans` at
    /// `SmDatabaseIO.cpp:547`), and `SmDatabaseIO::ByteSwap32(unsigned short)`
    /// ([`crate::topology::database_io::byte_swap_32_u16_with_fixes`]) swaps the two bytes of the
    /// `u16` (`SmDatabaseIO::ByteSwap16`) instead of shifting both bytes into bits `16..31` of a
    /// 32-bit intermediate and truncating back to `0` (`WR-47`;
    /// `plan/upstream-issues/WR-47--addendum-2026-11-01-database-io-byte-swap.md`). Off by
    /// default for pinned C++ parity.
    pub database_io_byte_swaps_read_boolean_and_u16: bool,
    /// `CoverCoincidentSurface` refuses a cover that extends a B-spline-based surface onto a continuation whose normal vanishes or turns back (a fold) over the domain the merge would use (WR-407, #434; SolidCraft change, tangency-labs/smlib e960ca1 (was 1b6f5fa)).
    pub cover_refuses_folded_continuation: bool,
    /// `SmSurface::CoverCoincidentSurface` covers a face straddling this surface's seam in a direction the surface already wraps (`IsClosed` over its natural domain: a cylinder's full turn): a step of its boundary walk across that seam is no failure, and the domain in that direction stays the period it spans; a crossing in an open direction, or at a pole, still refuses. `SmMerge::BooleanPostProcess` then completes each merged face as a periodic face has it (`HasSeamProblem`, then `SplitAtSeam` when a seam is crossed or missing), so the seam runs on through the covered part. Upstream refuses every crossing and the two walls stay apart (WR-599; the fork's tangency-labs/smlib@16bf1cb). `sm_CoverOtherCoincidentFace` lets only its trim-derived attempt cross such a seam (`bAcrossWrappedSeams`): the stored-domain attempt stays within one period, so faces whose stored domains are their whole surfaces do not merge to those (360 x 208 for half-turn faces spanning 180 x 3; tangency-labs/smlib@b518bbe).
    pub cover_wraps_across_closed_seam: bool,
    /// `SmSurface::CoverCoincidentSurface` on a plane grows its search domain to the projection of the other surface's tight box (a plane is unbounded, its STEP parameterization affine), where upstream searches only `GetMaxAnalyticDomain` (the domain grown by 100 widths); and `SmPrimitiveCreation::Boolean2D` refuses with `SM_ERR_INVALID_INPUT` when a cover fails, where upstream ignores it and rebuilds a face on a plane that does not hold it, with edges far off their surface (WR-536; the fork's tangency-labs/smlib@67b8a1e).
    pub boolean2d_plane_covers_far_faces: bool,
    /// A boolean vertex takes its position from the intersection curve that crosses its edge most steeply (WR-410, #1059).
    pub boolean_vertex_by_best_crossing: bool,
    /// `SmProtoTopologyManager::UpdateEdges` splits a coincident edge definition on a split edge that runs across the split point (away from both its ends, on its curve) there, with its proto edge (`SmProtoEdge::Split`), and classifies each part to the piece it lies on; upstream re-points the whole definition to the piece under its midpoint, whose vertices are not its ends, and the imprint finds no vertex for them (`SM_ERR_NULL_POINTER`, 1006) (WR-533; the fork's tangency-labs/smlib@f11d0ac).
    pub boolean_splits_definition_across_split: bool,
    /// `SmProtoEdge::Resolve` splits a proto edge from a proto vertex back to itself whose curve ends find two of its vertex definitions at each other definition of that proto vertex lying on the curve (`DropPoint` within the larger of the two tolerances, farther than twice it from both ends), then resolves the proto vertex; upstream keeps the edge whole past that third vertex, a vertex of its own face, and the merge fails (`SM_ERR`; two equal tubes a hair apart end to end) (WR-579, #1517; the fork's tangency-labs/smlib@9eec159).
    pub proto_edge_splits_at_captured_vertices: bool,
    /// `SmProtoEdge::Split` gives the new upper child proto edge the split parameter to this
    /// edge's old maximum, `[dT, old_max]`, where pinned v1.0.1 reads this edge's maximum
    /// after clamping it and the upper child gets the empty interval `[dT, dT]` (WR-101;
    /// `plan/upstream-issues/WR-101--addendum-2026-10-31-opt-in-upper-interval.md`). The
    /// lower child is `[old_min, dT]` either way. Off by default for pinned C++ parity.
    pub proto_edge_split_upper_keeps_the_old_max: bool,
    /// `SmMerge::BooleanPostProcess` also checks the merged face's edges that a face merge left with that face on both sides (slits), not only the intersection edges (WR-304, #1120).
    pub boolean_cleanup_removes_slits: bool,
    /// `SmFace::CurveOnClassify` classifies an interval whose two ends lie on one face edge (or its vertices) and whose midpoint is within the intersection tolerance of it as on that edge (the fork's `ClassifyEdgeInterval`, WR-305, #1057). Curved pieces only: a straight interval along a straight edge is always classified, by a certificate on its two ends (the distance is affine along it), with or without this flag.
    pub classify_edge_interval: bool,
    /// `sm_GetEllipseTrimIntervals` evaluates the two seam lines on the cone's whole height (only their sweep from the given domain), as the end circles and the classification are, so a face trimmed along its axis keeps an ellipse whose only seam crossing lies in its trimmed-off part (WR-490, #871; a solidcore-only fix, not in the SolidCraft fork at tangency-labs/smlib@26cc451, whose `sm_GetEllipseTrimIntervals` still takes the seam lines on the domain's v range; `plan/upstream-issues/WR-490--ellipse-trim-seam-lines-on-trimmed-domain.md`).
    pub ellipse_trim_seam_lines_span_cone_height: bool,
    /// `SmProtoEdge::Resolve` treats a proto edge from a proto vertex back to itself whose ends find one vertex definition as degenerate below twice the proto vertex's containment radius, and `ImprintIntersections` fits that vertex to its edges (WR-306, #1058).
    pub degenerate_proto_edge_in_one_vertex: bool,
    /// `MakeEdgeInFaceWithVU` grows each end vertex's tolerance to twice its gap to the new edge's end (only if larger), after `RefreshTolerance` keeps it as it was, so `AssertValid` does not refuse the edge (WR-469, #870; a solidcore-only fix, not in the SolidCraft fork at tangency-labs/smlib@26cc451, whose `MakeEdgeInFaceWithVU` ends with `RefreshTolerance` only; `plan/upstream-issues/WR-469--make-edge-in-face-vertex-tolerance-gap.md`).
    pub new_edge_vertex_tolerance_covers_gap: bool,
    /// `SmBrep::Transform` changes no tolerance under a scale of 1 (a rotation or reflection), and under a scale s sets each vertex, edge and face tolerance to the larger of its own times s and the new body tolerance, not every one to the body tolerance (WR-507, #1520; tangency-labs/smlib#49).
    pub transform_scales_each_tolerance: bool,
    /// `SmBrep::Transform` takes a scaled body's model size from the body's bounding box (vertices, edges and faces, not grown by tolerances), not from its vertices alone, so a body with no vertices (a lone torus) does not get a zero tolerance (WR-508, #1521; tangency-labs/smlib#50).
    pub transform_model_size_from_body_box: bool,
    /// A torus rebuilt from its NURB measures its generating interval signed (WR-408, #1112).
    pub torus_signed_gen_curve_interval: bool,
    /// `SmTorus::IsNurbSurfaceTorus` measures the major radius from the torus origin (the minor circle's centre projected onto the common line), not from the first sweep circle's centre, which lies off the equator when the NURB's tube does not start there (WR-501, #1470; tangency-labs/smlib@6136a04).
    pub torus_major_radius_from_origin: bool,
    /// `SmTorus::CreateIsoParametricCurve` takes a minor circle's direction from its midpoint when its start lies on the axis, not from the start point again, so a horn torus's minor iso circles get a direction and an analytic domain (WR-504, #1514; tangency-labs/smlib#40).
    pub torus_iso_circle_axis_from_midpoint: bool,
    /// `SmTorus::IsNurbSurfaceTorus` and `SmSphere::IsNurbSurfaceSphere` keep the NURB they tested (rebuilding the analytic data from it, as `SetFromGwNurb` does), not the one made from the fitted circles (~1e-11 off), and the sphere test takes a half meridian within `180 + 360 * SM_EFF_ZERO` degrees, not `180 + SM_EFF_ZERO` (WR-493, #1433; tangency-labs/smlib@7ea6a96). The sphere's sweep interval is its polar curve's, so a small corner sphere passes `SmSurfOfRevolution` tests #6 and #7 (#2073; not in the fork).
    pub found_torus_sphere_keep_tested_nurb: bool,
    /// `SmBSplineSurface::HasInternalPole` tests the row through each interior knot (`knots - 1 - degree`), not one row further for each repeated copy of the knot, so a horn torus's corner rows on its axis are not taken for an internal pole (WR-505, #1515; tangency-labs/smlib#41).
    pub internal_pole_row_through_knot: bool,
    /// `SmSurfOfExtrusion::AssertValid` test 8 point-solves the origin on the generating curve with an unscaled `SM_EFF_ZERO`, so an origin evaluated on the curve at coordinates of about 10 (a text outline's quadratic Bezier) resolves to a few 1e-12 and is reported missing; with this fix that solve uses the issue's coordinate-aware zero `SM_EFF_ZERO * (1.0 + origin.max_dimension())`, its own factor rather than the factor 100 of the neighbouring point tests (WR-511, #1508; tangency-labs/smlib#31).
    pub extrusion_origin_scaled_zero: bool,
    /// `SmBSplineVolume::CalculateContinuitiesSimple` with `bCalcGeometric == FALSE`
    /// sets both end knots (`0` and `knot_cnt - 1`) to `SM_CT_DISCONTINUOUS` (matching
    /// its `SmBSplineVolume.h` contract, the `bCalcGeometric == TRUE` branch, and
    /// `SmSurface::CalculateContinuities`) and evaluates representational knot
    /// continuity (`degree - mult`) and `reMinContinuity` over interior knots
    /// `1..knot_cnt - 1` only (leaving `reMinContinuity = SM_CT_CINFINITY` on a
    /// single-span volume), instead of filling `0..knot_cnt - 1` into a zero-cleared
    /// (`SM_CT_UNDEFINED`) array — which left the last knot `SM_CT_UNDEFINED`, set
    /// knot `0` to `SM_CT_C0` on every clamped volume, lowered `reMinContinuity` to
    /// `SM_CT_C0`, and made `HasDiscontinuitiesSimple(..., FALSE)` report
    /// discontinuities on smooth single-span and `C1`/`C2` volumes (WR-49, #2151; see
    /// `plan/upstream-issues/WR-49--addendum-2026-10-31-representational-end-continuities.md`).
    /// Off by default for pinned C++ parity.
    pub volume_representational_end_continuities: bool,
    /// `FindPolygonGuess` keeps a suspicious control-net triangle guess as a candidate against the nearest control point's, and returns the nearer one on the surface, so a rational surface whose control points are farther apart than its neighbouring parts does not lose a good guess (WR-444, #1164).
    pub point_solve_keeps_triangle_guess: bool,
    /// `SmFilletCorner::MakeAnalyticCornerSurface`'s `SER` / `NER` exits return their failing status as an `SmBoolean`, which `SmFilletCorner::CreateCornerPatch` reads as `TRUE` (an analytic surface was made, though none was added) and so skips the Coons patch. With this fix those exits read as `FALSE` and the caller builds the patch. Only the exits that return an upstream status (`SM_ERR` 1001 / `SM_ERR_NULL_POINTER` 1006) change; the port's `Unported`, `InvalidInput` and `PortInvariant` still propagate (WR-254, `plan/upstream-issues/WR-254--smfilletcorner-makeanalyticcornersurface-reports-its-ser-ner--dup2.md`; off by default for pinned C++ parity).
    pub analytic_corner_failure_is_false: bool,
    /// `SmEdgeDefinition::Imprint` continues an intersection edge's 3d and UV curves, each by the polynomial of its own end span, to a proto vertex that lies a few tolerances past the curve's end, so the edge reaches its vertex (internal thread cuts; WR-453, #410; tangency-labs/smlib bf5d3ae (was a8caccd)). The end goes to its proto vertex's position only when that proto vertex is one vertex in each B-rep, else to its own final vertex (WR-498, #1480; tangency-labs/smlib 0b1b945 (was ffaf4e8)).
    pub imprint_extends_edge_to_proto_vertex: bool,
    /// `SmEdgeDefinition::Imprint` takes an edge Brep2's face already has between the definition's two vertices along its curve (`SmTools::FindCoincidentEdge` within the larger of the face's and the definition's tolerance) as the definition's edge there, where upstream makes a second edge beside it: the face then has coincident edges and the merge copies the boundary edge into Brep1 beside the curve's (`SmFace` tests 33 and 34; two equal tubes meeting end to end) (WR-580, #1519; the fork's tangency-labs/smlib@81cd61a).
    pub imprint_reuses_coincident_face_edge: bool,
    /// `SmFilletSolver::ComputePointValues` chooses its finite-difference step for each point (1e-3 at a tangent point, 1e-5 otherwise), not once per process as the function-local static `sdStaticStep`, so a blend does not depend on the blends before it ([`Kernel::fillet_point_step`] is then neither read nor set, and an existing latch is kept as it is) (ND-3, #1173; the fork's tangency-labs/smlib@9e17f64 (was 0a05860)).
    pub fillet_point_step_per_point: bool,
    /// `SmBSplineSurface::CopyAndAddAnalytics` keeps an exact copy where the recognised analytic surface does not give the same points at the same parameters (7 x 7 grid), so a face split by `MakeEdgeInFaceWithVU` keeps trims that still lie on its edges (WR-450, #387; the fork's tangency-labs/smlib@7af1987 (was 276b509)).
    pub analytic_copy_keeps_parameterization: bool,
    /// An offset surface evaluated beyond its base's domain extends the base itself on every side that can grow at once, each by the domain size times the least power of two that reaches the point and never less than the extension it replaces, so two copies of one offset give the same points whatever was evaluated first (WR-458, #419; the fork's tangency-labs/smlib@9f54d05 (was 2d7c7ed)).
    pub offset_extension_from_base: bool,
    /// With `offset_extension_from_base` (which must be on too; this flag does nothing without it), an offset surface whose base is a cylinder, cone, sphere or torus extends that base past its domain as itself, where upstream's tangent ruled extension (G1R) leaves the surface at once: a cylinder R25 continued 0.44 past its face's end was 0.0036 off the circle, and a round traced there was built that far off all along (WR-633, #960; the SolidCraft fork's tangency-labs/smlib@de0a6b8, branch `replay/1.0.1`). The extension is built from the analytic surface (the fork's @93db777): the base's spans are kept and new ones joined at its ends with a knot of full multiplicity, every row round the axis continues in rational quadratic arc spans, a sphere's or torus's generator in quarter turns, and a cone's line straight. Round the axis it reaches no further than closing the turn, half the gap past either end, and once it reaches the closure that direction wraps like a periodic one (WR-650, #1986; the fork's @b298cc2): a 294 deg arc was extended to 882 deg and more. A surface of revolution whose generator is an arc of one circle (SMLib's concave round of a circular edge) continues it as its own circle, as a torus does (the fork's @43fd6b6).
    pub offset_extension_as_itself: bool,
    /// `DoFilleting`'s `BooleanPostProcess` removes none of the target's own edges, nor an edge between two of its faces (the fork's `m_sPostProcessKeptEdges`/`Faces`), so the patches of a sphere imported in six faces are not merged into a face no patch's parameterization can carry (WR-459, #423; the fork's tangency-labs/smlib@b7d7b96 (was 9ddf4a8)).
    pub fillet_cleanup_keeps_target_faces: bool,
    /// `SmFilletNx1Corner::MakeCornerTopology` skips a side edge whose faces meet tangentially at the corner's vertex (a flat top running smoothly onto a round) round the vertex to the edge past it, so the end face is the one beyond the tangent seam (the slot wall), not the round the fillet never meets, and `SmFilletEdge::CalcEdgeOnExtendedEdge` intersects the side face with the end face's extension when the side face is not one of the edge's own faces (WR-457, #418; the fork's tangency-labs/smlib@ba7f68e (was d5ebcb3)).
    pub fillet_nx1_tangent_side_edge: bool,
    /// `SmFilletNx1Corner::MakeCornerTopology`'s tangent side-edge skip (`fillet_nx1_tangent_side_edge`, which must be on too) also passes, once, the filleted edge's G1 continuation across the vertex of the opposite convexity (past any tangent edges): the overhang's rim continuing the base circle of a cylinder standing over a block's edge. The edge past it, of the filleted edge's convexity, is the side edge extended and the face beyond it the end face, so the round ends there instead of failing with status 1001 (WR-499, #1474; the fork's tangency-labs/smlib@420e47d).
    pub fillet_nx1_opposite_continuation_side_edge: bool,
    /// `SmCircularCrossSectionFSG::CreateSurfaceFromCurves` moves both rails' end control points to their midpoint at an end where the rails' ends lie within the approximation tolerance of each other (a degenerate corner whose cross section is a point), before the surface is skinned: each rail is lifted from its own surface, so at a vertex their ends differ by the model's accuracy (6.5e-12 mm on a sketch with solver residues), and the skinned surface kept a near pole there instead of an exact one, which `SmSurface` check test 5 refuses as close to singular (WR-594; the fork's tangency-labs/smlib@5fdefb9).
    pub fillet_rails_meet_at_one_end_point: bool,
    /// `SmFilletNx1Corner::MakeCornerTopology`'s tangent side-edge skip (`fillet_nx1_tangent_side_edge`, which must be on too) takes the edge reached past the tangent side edge as the corner's edge also when that edge's own faces meet tangentially at the vertex (a stadium bar's flat side running onto its round end, below the cylinder wall the bar's top meets): such an edge has no convexity, and `IsConvexRadialSector` on it reads only the sign of rounding residues in the geometry, so one end of the round's arc could read "opposite", leave no corner edge and fail with status 1001 (fillet error 3, corner type 8) (WR-593; the fork's tangency-labs/smlib@1ff7353).
    pub fillet_nx1_tangent_corner_edge: bool,
    /// `SmFillet3x2MixedConvexityCorner::MakeCornerTopology` gives the roles of the corner's two filleted edges by the unfilleted edge's convexity: where it is concave (a boss standing over a bar's side face, its line on that face running down from the vertex) the convex fillet's rail crosses it and the concave fillet's rail crosses its continuation past the vertex, instead of the other way round as in an L-shaped box, so the corner no longer fails with status 1001 "Corner Geometry Computation Failure" (WR-524, #1475; the fork's tangency-labs/smlib@1255993).
    pub fillet_3x2_mixed_concave_side_edge: bool,
    /// With `fillet_3x2_mixed_concave_side_edge` (which must be on too), `SmFillet3x2MixedConvexityCorner::MakeCornerTopology` takes the two fillets' roles at an unfilleted edge whose faces meet tangentially at the vertex (a block's side running tangent into a cylinder whose wall the block's top meets) from the faces' corners, not from `IsConvexRadialSector`, which on a G1 edge reads only the sign of rounding: the convex fillet's rail crosses that edge unless its face shared with the edge turns more than half a turn between them at the vertex (`sm_CheckFaceComplimentaryCornerConcavity`), so the concave round's rail is no longer sent to cross an edge it never meets and the corner no longer fails with status 1001 (fillet error 3, corner type 5) (WR-597; the fork's tangency-labs/smlib@c17b745).
    pub fillet_3x2_tangent_side_edge_roles: bool,
    /// Not upstream (WR-710, #2256): `SmFilletCorner::Create` refuses a 3x2 mixed-convexity corner (`SmFillet3x2MixedConvexityCorner`: three edges at the vertex, two filleted, one convex and one concave) whose two filleted edges leave the vertex along one tangent in the same sense (a bar's front top edge and the concave arc where its top meets a cylinder that its front face runs tangent into). The common face closes to a cusp there, so the rails, each set the blend's size into it, cross each other before the corner, and the corner patch built across them stands out of the faces (6.5e-3 out of the front face in #2256) though `check_brep` passes. The fillet returns [`SolidError::Unsupported`] with the reason `blend::fillet_corner::p3::MIXED_CORNER_RAILS_CROSS` instead. A refusal only: no corner is built for the case. A corner that is the blend corner's Mixed one (smlib#82, #2360) is built instead and not refused; the refusal stands for the others.
    pub fillet_refuse_mixed_corner_crossing_rails: bool,
    /// `SmFilletNx2Corner::MakeCornerTopology` mitres two blends meeting at a vertex of three edges where the common face's corner between them is concave (mixed convexity: a pocket's rim at its sharp corner, a boss's root at its sharp corner, the side edge not filleted), as at a corner of uniform convexity: the rails meet in the common face (`SM_FV_RAIL_X_RAIL`) and on the side edge, the blends' surfaces meet along their intersection, and the corner is a bevel (where the rails meet the side edge at two places, `AdjustTopology` joins them along the side face). Upstream sweeps a 3-sided face round the side edge between ribs straight across from where the rails meet it. Edges meeting at 5 degrees or less still blend (WR-642; the fork's tangency-labs/smlib@4f06e72, the owner's ruling of 2026-10-30).
    pub fillet_mitre_concave_face_corners: bool,
    /// `SmFilletNx2Corner::AdjustTopology`, at a bevel (mitre) of two blends meeting at a vertex of three edges where the common face's corner between them is concave and whose rails meet the side edge at two places (walls leaning differently, a sloped basin's rim), builds the topology `AdjustConcaveCornerTopology` builds at a swept corner of mixed convexity instead of splitting the side vertex as for a convex bevel: each rail ends where it meets the side edge, the larger round's cut and the smaller rail's extension run on the side face's extended surface to where the mitre's curve meets them, and `CreateCornerPatch` fills only that gap with a patch on the side face's extended surface (`FilletCorner::bevel_gap_patch`). Upstream cut the larger round by the side face's own surface, which ends at the side edge (`CalcFilletIntSideFace` fails), or the smaller rail ran past its face (`TestRollOver` fails). Mitres at such corners come from `fillet_mitre_concave_face_corners` or a corner set to bevel (WR-644; the fork's tangency-labs/smlib@2223108).
    pub fillet_mitre_gap_patch: bool,
    /// Each fan of faces at a vertex where the body touches itself is a corner of its own: `SmFilletExecutive::SetFilletParameters` splits each filleted edge's vertex whose edge uses form several fans sharing no edge (two cubes sharing a corner; the up and down halves of squares drawn edge to edge, touching at the origin) into one vertex per fan at its point (`SmBrep::SplitVertexByFans`, `crate::topology::brep::vertex_fans`), `DoFilleting` joins the target's vertices still at a split point after the blends, whether they built or not (`SmBrep::JoinVertexFans`), and `SmFilletCorner::Create` builds no two-edge corner where an edge not blended there is a knife edge (its faces tangent and folded back on each other, `SmFilletSolver::IsKnifeEdge`). Upstream `SmFilletCorner::Create` walked one fan's faces, found the blended edges of both fans and built an Nx2 corner of edges sharing no face, failing with status 1001 ("unable to make topology") (WR-665; the fork's tangency-labs/smlib@4ecfbab and @e418750).
    pub fillet_corner_per_vertex_fan: bool,
    /// `TrimExtendedEdge` never takes an open edge's first trim point again as its second, by point as well as by parameter: on the closed curve a rim half of a sphere cap lifts onto (the whole offset circle), the first point's twin across the curve's seam (another parameter, the same point, often the far end of the coincident range the other rim half's offset gives) is refused, so the half no longer gets the whole circle (or nothing) and its offset loop no gap. Thickening the cap above z = 5 of a sphere whose rim is two half circles placed against its seam (the sphere cut by a turned box) failed with `SM_ERR_OFFSET_EDGE_VANISHES` (1043) with loop-gap closure, and gave a non-manifold body when closure could still be disabled (WR-673, #2174; the fork's tangency-labs/smlib@7ee0876, for #1502).
    pub offset_open_edge_second_trim_point: bool,
    /// `SmCone::CreateCanonical` (`Cone::create_canonical_with_semi_angle` and `Cone::create_canonical`) passes `inside_out` as the bounded cone constructor's `bInsideOut` and `false` as `bMakeNurbGenCurve` when `height` is `Some` (where upstream passes `FALSE` for `bInsideOut` and the caller's `bInsideOut` for `bMakeNurbGenCurve`, dropping an inside-out request), and rejects `height <= 0.0` in the two-radius overload with `SolidError::InvalidInput` (`SM_ERR_INVALID_INPUT`) instead of forwarding `top_radius` as the semi-angle in degrees of an unbounded cone (WR-63, #2207; see `plan/upstream-issues/WR-63--addendum-2026-11-01-cone-create-canonical-args.md`).
    pub cone_create_canonical_preserves_inside_out_and_checks_height: bool,
    /// `MateEdges` also intersects analytic copies of the two faces' surfaces (plane, cylinder, cone, sphere, torus; extended only along a plane or a cylinder's or cone's rulings, where the tangent (G1R) extensions still are the surface) and replaces each marched piece that lies on an exact line or conic between the same ends, and drops to one UV curve on each extended surface within the body's tolerance, by that exact curve, so a shelled cylinder's floor circle, seam line and rim stay exact (WR-495, #1453/#1130; the fork's tangency-labs/smlib@fddc366 and @98ab1b0).
    pub offset_analytic_mate_edges: bool,
    /// `ShellBrep` ends with `MakeEdgesOnAnalyticsExact` on its result (not on the non-manifold offset it returns early): each edge whose curve is a marched intersection between two faces on analytic surfaces (planes, cylinders, cones, spheres, tori) takes the exact line or conic those surfaces meet on through its ends and middle, unless that conic fails its own validity check, and its faces' UV trims are made again, so a shelled rounded box's rim is arcs, not cubics 2.4e-6 off them (WR-545; the fork's tangency-labs/smlib@cd85d34).
    pub shell_edges_on_analytics_exact: bool,
    /// `DoExtendedOffset`'s protective stitch makes one pass at the merged body's tolerance instead of `StitchIntoSolid`'s doubling up to a 200th of the body's size, which welds the rims of cut-off pieces `MakeManifold` removes next into one edge of a far larger tolerance (WR-455, #855; the fork's tangency-labs/smlib@51eefa5 (was 601d804)).
    pub offset_stitch_at_body_tolerance: bool,
    /// `SmCurve::GlobalCurveIntersect` moves a point solution to an end of either curve when that end lies within tolerance of the other curve and the curves stay within tolerance of each other over the whole stretch between (`sm_MoveContactsToCurveEnds`), so a tangential contact is not placed inside the curve (WR-455, #855; the fork's tangency-labs/smlib@51eefa5 (was 601d804)).
    pub curve_contacts_at_curve_ends: bool,
    /// `SmOffsetExecutive::ShellBrep` of a solid inward fails when the inset leaves no cavity (an empty offset, `SM_ERR_INVALID_INPUT`) or the offset is not a closed solid (`SM_ERR`), instead of handing the input back unhollowed as a success (WR-454, #380; the fork's tangency-labs/smlib@1fc0d8f (was fbe43bb)).
    pub shell_empty_cavity_fails: bool,
    /// `CreateFilletBrep` sets each solver's trace reach before the corners (`sm_ExtendTraceToItsRails`): where the base surfaces' domains extended by the solver's factor (half their size) do not reach as far as a rail can lie from the edge (`r / tan(t/2)`, `t` the smallest angle between the faces' planes along it, `r` the largest radius over the trace, a variable round's law included), they reach twice that and `r` more, in the parameter directions that run across the edge (`GetDomainExtension`), so a blend wider than its faces is traced to its rails instead of being clamped at the extended domain's border; and `RailEdgeuseIntersect` runs a straight side edge on a plane on along its line as far, so a rail meeting the side edge's line past the edge's end is found (WR-609, WR-610, #960; the fork's tangency-labs/smlib@a9277fd and @6e77631). It runs such an edge on along its line on a cylinder or cone too, a ruling of it, so a disc's rim rounded past its wall meets the seam's line below the wall at its closed corner (the fork's @779cb9b).
    pub fillet_trace_reaches_rails: bool,
    /// `SmFilletVertex::CalcRailIntEdgeuse` places a rail's end found on a side edge at the rail's base surface evaluated with out-of-bounds evaluation on (the surface continued, as `RailEdgeuseIntersect`'s solve over the extended domain found it), not clamped to the surface's own domain, where a rail meeting a side edge's line just past the edge's end (a cliff overflow, with `fillet_trace_reaches_rails`) was put on the edge's end vertex, off the rail by the square of that distance over twice the radius, and taken as meeting the side edge (WR-630, #960/#1467; the fork's tangency-labs/smlib@3f22e97).
    pub fillet_rail_end_on_continued_surface: bool,
    /// SolidCraft's cliff overflow (WR-631, #960; the fork's tangency-labs/smlib@8ba4667 and later, rev 68fb9162): with `fillet_refuse_rails_leaving_faces` (which must be on too; this flag does nothing without it), a convex blend whose rails leave the faces they roll on across cliffs (an opening's rim, the body's outline) is built on the faces' continuations and the material it removes is subtracted from the body, instead of being refused. A concave overflow, a round running on into the body's material, an overflow with another blend concave, with a rollover, or with the global merge on, are refused with `FilletErrorType::NoRoom` and their own messages. An accepted overflow (`FilletExecutive::cliff_overflow`) leaves the Nx1 corners' ends its rails run past uncut (`FilletCorner::drop_end_rib`, the open ends), and `DoFilleting` subtracts it instead of merging (`SubtractRemovalRegion`, `crate::blend::fillet_cliff_overflow`): each variable round on planes must hold its radius law; each open end is cut by the end face's plane; the fillet sheet is closed into the region the blends remove by faces on the hosts' surfaces (a plane continued exactly), stitched, oriented, and subtracted from the target by the Boolean. A construction that fails keeps its own error (WR-638, tangency-labs/smlib@ae2a6de): `NoRoom` where it finds no room, `NotExact` for a continuation that cannot be exact (a non-planar surface past its domain, a curved side edge past its end), `BadMerge` for a region that does not close or a failed subtraction. The closing also takes a corner's end face on a wall's surface as that wall, a chain ending at a filleted edges' vertex, and a planar surface of any kind as its plane (WR-635, tangency-labs/smlib@eddf0ed), and closes a chain round a loop of blended edges on a planar face (WR-636, @bba9914; both branch `replay/1.0.1`). Use with `fillet_trace_reaches_rails` and `fillet_rail_end_on_continued_surface`, as the fork has them.
    pub fillet_cliff_overflow: bool,
    /// With `fillet_trace_reaches_rails`, `RailEdgeuseIntersect` runs a straight side edge on along its line past its far end only, never behind the corner: past the end at the filleted edge's vertex it lies on the far side of the filleted edge, outside the sector the blend fills, where no rail of it can be, and where (with the faces continued as themselves) a mirror image of the ball can be solved for (WR-643, #960; the SolidCraft fork's tangency-labs/smlib@7224896, branch `replay/1.0.1`). A side edge with both or neither end on the filleted edge still runs on past both.
    pub fillet_run_on_past_far_end_only: bool,
    /// With `fillet_trace_reaches_rails`, `RailEdgeuseIntersect` evaluates a side edge's linear B-spline UV trim with out-of-bounds evaluation on over the run-on range (on its own copy of the trim), so it continues along its line past its ends as a line trim does. An `SmBSplineCurve` clamps the parameter to its natural interval (`SmBSplineCurve::Evaluate`, `SmBSplineCurve.cpp:3052`), so the run-on did nothing on the exact degree-1 B-spline trims `SwapUV` mirrors onto some of a box's faces: at a side edge with a line trim on one wall and a B-spline on the other, one rail's end was found past the edge's end and the other's not at all, and a chamfer split its corner's vertex between two points and failed with error 3, corner edge computation (WR-654, #1981; a round on a 5 x 5 box's edge failed with error 4).
    pub fillet_run_on_bspline_trims: bool,
    /// `SmFilletGeom::TrimRailCurves` trims a rail at its own knot next to the parameter its end vertex drops to, inside the trim, when the rail between the two is no longer than the target B-rep's zone tolerance (`sm_SnapRailEndToKnot`): a trim there left an end span shorter than the tolerance with coincident control points, an internal point singularity of the rail and of the face curves trimmed with it, which the body check refuses. It happens where the rail stands still at a face's boundary past its end and where the section's other rail crosses its side edge there (WR-664; the fork's tangency-labs/smlib@7d79866).
    pub fillet_rail_trim_at_near_knot: bool,
    /// `SmTopologyIntersector::CloseIntersectionLoop` relates a target edge with the sole other edge between the mated vertices only when the target's midpoint drops onto it within `max(2 * dMaxDist, the two edge tolerances)`, not whenever the closest-point drop succeeds (which it always does): two different edges between the same vertices, such as a cylinder rim's long arc and a crescent prism's r 6 arc, were related as one, and the wall was built on the wrong curve (WR-528; the fork's tangency-labs/smlib@e881cdb).
    pub close_loop_relates_only_coincident_edges: bool,
    /// `SmAdvSurfaceIntersector::FindInteriorCurves` seeds a Newton search for isolated tangential contact points (zeros of `S1 - S2` with parallel normals, 4x4 system with the exact Jacobian) from the leaf pairs it cannot trace (normals within 5 or 2 degrees, no local intersection), plus the poles, and adds each contact with a numerically zero gap and normals within 1e-4 rad that is not on a curve already found as a degenerate `TOUCHING` curve (WR-74, #394; the fork's tangency-labs/smlib@9e8bf10 (was c26edbc)). Higher-order contacts (equal curvature) are not found.
    pub ssi_tangent_contact_points: bool,
    /// `SmAdvSurfaceIntersector::FindInteriorCurves` traces a start point whose surface normals are within 5 degrees (guess) or 2 degrees (solution) on probation instead of skipping it, keeping the trace only when it closes into a crossing loop whose box diagonal exceeds twice the tolerance; starts below `2 tol / model size` radians are still skipped and rejected traces (and their start points) are remembered so neighbouring starts are not retraced. A small loop between nearly tangent surfaces (a plane just above a NURBS paraboloid's apex) was otherwise never found (WR-79 part 2, #396; the fork's tangency-labs/smlib@1f5c0f7 (was 30d25bf)).
    pub ssi_low_angle_start_on_probation: bool,
    /// `SmSurfaceIntersector` traces an open tangent curve once: `AddStartPoint` keeps one start point per cluster of tangent boundary start points (`IsSameTangentCluster`), a finished trace removes the clusters at its ends (`DoIntersection`), `TraceCurve` runs its "already traced" test on a first step that snapped onto another start point, and a near-tangent trace stops at the free end of a curve already found instead of running over it (WR-76, #392; the SolidCraft fork's tangency-labs/smlib@920da20 (was 0ce09f2)).
    pub ssi_trace_tangent_curves_once: bool,
    /// `SmAdvSurfaceIntersector::ComputeSingularityPoint` starts a curve at a singularity from the first singular direction that points into both domains, whose angle is the search's `dAngleMin`, before the later inside directions are searched for the one closest to the previous direction. Upstream leaves the output at `aSingDirections[0]`, so where the first inside direction is the nearest (or the only one) the curve starts along a direction that need not point into either domain (WR-512, #1522; the fork's tangency-labs/smlib#52).
    pub ssi_singular_start_inside_direction: bool,
    /// `SmSurfaceIntersector::IsPointOnStartPoint` compares the squared distance of each
    /// surface point with the squared tolerance instead of with the tolerance itself
    /// (WR-51). Upstream compares `distance_squared < tol`, whose effective radius is
    /// `sqrt(tol)`, so a small tolerance accepts points far farther apart than intended.
    /// Off by default for pinned C++ parity.
    pub point_on_start_point_squared_tolerance: bool,
    /// `SmSurfaceIntersector::StepBackFromSingularity` leaves each point its own curve tangent: the tangent at a surface pole comes from the pole's limit normal and is exact, and copying the previous point's over it bent the Hermite span into the pole (a line projected through a sphere's pole, r 20, came out 2.02e-4 off the sphere). Only the surface derivatives come from the stepped-back point (WR-550; the fork's tangency-labs/smlib@8d95600).
    pub ssi_pole_point_keeps_its_tangent: bool,
    /// `DoIntersection` traces the tangential intersection curves the marcher cannot follow (the surfaces touch along them, `n1 x n2 = 0`), and the crossing branches through the contact points where crossing curves meet, which it reports as touching points (WR-75; tangency-labs/smlib ef4ab25, e9fde8f, d8c61db, 1693c9d). `FindInteriorCurves` keeps the node pairs it skips for parallel normals as seeds. A crossing branch the marcher's curve covers except inside the band where the surfaces are within tolerance of each other is dropped, not traced on along a tangency past that curve's end (#2089).
    pub ssi_tangent_curve_tracer: bool,
    /// `SmSurfaceIntersector::DoIntersection` splits each traced crossing (or near-tangent) B-spline curve, with its two UV curves, where it passes through a singular point of the intersection (the normals parallel while two branches cross): `|n1 x n2|` is sampled 8 times per knot span, each sharp minimum refined by golden section, and the curve split where the sine is below 1e-6 and rises above 1e-3 again on both sides, more than 10 approximation tolerances from its ends; the tails are appended. Upstream's marcher steps straight through such a node, so the curves cross there without ending; two cylinders touching inside at one point then union to a body that loses one of them, fails 1001 in `MakeFaceuse`, or fails the check's seam tests (WR-77, #393, #2110; the fork's tangency-labs/smlib@3c9d1d3, `sm_SplitCurvesAtSingularPoints`).
    pub ssi_split_curves_at_singular_points: bool,
    /// `MakeDupin` uses `1/sqrt(abs(k1))` for the bottom hyperbola's second axis, matching the top conjugate branch at unequal-curvature saddle points (WR-69).
    pub dupin_bottom_hyperbola_axis: bool,
    /// `SmPointClassification::GetGap3d(pOptSurface)` returns `Err(SM_ERR)` (and propagates
    /// object-point, surface-drop, and surface-evaluation failures) when the object point
    /// has no single drop point on `pOptSurface` even at `100 * sObjZoneTol3d` (WR-58).
    /// Upstream writes `return SM_ERR;` inside a `double`-returning function (`SmPointClass.cpp:464`)
    /// and ignores failing `SmStatus` returns along the surface-drop path, so `1001.0` (or
    /// undefined evaluation geometry) masquerades as a large 3D in-surface gap to callers
    /// such as `SmCurveClassification::HasNearMisses`. Off by default for pinned C++ parity.
    pub point_class_gap_3d_propagates_no_drop_error: bool,
    /// `SmFilletIntersector::ComputeNextPoint`, where the solver's offsets change along the trace (`OffsetRadiiCanChange`, a variable round), solves the fillet at the span's five test shares (a step of that share from its start) and rejects the span where a solution's contact lies farther than the approximation tolerance from its Hermite rail. Upstream tests only that the rails lie on the base surfaces, which on a plane every uv curve does, so the step grows to the angle tolerance and the rails cut across the radius law between the traced points (a cubic-law round on planar faces removes the wrong volume, its surface off the law's circle) (WR-646, #1989; the fork's tangency-labs/smlib@4438859).
    pub fillet_variable_span_follows_fillet: bool,
    /// `SmFace::CalculateTightBoundingBox` searches a trimmed face's own extreme inside its trim (subdividing the UV domain from the vertex and edge extremes, bounded at depth 16 / 4096 cells, an undecided cell divided at the surface's knot inside it nearest its middle, else at its middle), not only the surface's global extreme, so a face whose surface peaks outside the trim is not boxed too small (WR-588, #1799; tangency-labs/smlib@beb6061 (was 9d44c1b)), nor one whose rational surface has knots off the domain's dyadic points too large (WR-651; tangency-labs/smlib@326de73); only points classified inside the face (`SM_PC_FACE`), not on its edges or vertices, count as the face's, so a rounded box corner's sphere octant is not boxed past its arcs (1.0000243 for 1; tangency-labs/smlib@95bf434).
    pub face_tight_box_searches_trim: bool,
    /// `SmTopologyIntersector::IntersectTopology` skips a face pair whose face boxes lie farther apart than the intersection tolerance (a box that cannot be computed, or holds nothing, rules nothing out), and `SmBrep::MakeEdgeInFaceWithVU`, on a plane, leaves in place without classifying an inner loop whose test point lies outside the box of the new face's outer loop (grown by the edges' tolerances, at the retry tolerance). Upstream pairs every face of one sheet with every face of the other through whole-surface checks and classifies every inner loop against each imprinted face, so a 2D boolean of a sheet with n holes takes time in n^2 (WR-527, #1526/#1527; the fork's tangency-labs/smlib@60b1c6a).
    pub boolean_skips_faces_apart: bool,
    /// `SmBrep::DeleteTopologicalVertex` joins an edge whose curve is a curve on a B-spline-kind surface along a UV line of constant u or v (the seam an extrusion leaves on its wall) as that surface's iso curve over the edge, the same curve exactly, so the seam of a full-turn face that a union runs on through a covered face is one edge (WR-598; the fork's tangency-labs/smlib@829b982). Upstream casts the curve's copy to `SmBSplineCurve` and dereferences the null; without the flag the port leaves the vertex in place (the routine's silent `SM_ERR`).
    pub delete_vertex_joins_seam_iso_curve: bool,
    /// `SmBrep::TestClosureSplitRegion` asks whether a new face closes a region with `ClosureTraversalFromBothSides`, two breadth-first searches through edge-use radials from the face use and from its mate that stop when they meet or one runs out, instead of `ClosureTraversal`'s depth-first walk from one side that stops only at the mate. The answer is the same, and no mark is claimed or set; upstream's walk can cross most of the shell for each face a boolean makes, so a union costs the new faces times the shell's size (WR-584; the fork's tangency-labs/smlib@69c5198).
    pub boolean_new_face_closure_both_sides: bool,
    /// `SmBrep::GlueVertices` joins the pair of shells of every region both vertices are in (each other pair's children transferred with its wire edges, the emptied shell removed), not only the last pair its search finds: a solid's vertex is in a shell of the solid's region and one of the infinite region's, so where a face that alone held two parts of a body together was deleted, gluing the parts again at a vertex left the other region's pair apart, and the check reported an edge use and its radial in two shells (WR-607; SolidCraft change, tangency-labs/smlib 661f79c).
    pub glue_vertices_joins_every_region: bool,
    /// `SmPrimitiveCreation::CreateBox` returns the status of each face it makes (a plane that cannot be made, a face whose making fails even after the face was made, such as a box far smaller than the tolerance whose top face closes up on its bottom one) and of its `StitchAndOrient`, and makes no face on a side of zero extent. Upstream drops every one of those statuses and returns success with whatever faces were made (WR-596; the fork's tangency-labs/smlib@0df62c8, 35008b1 and 449c81c, `AddBoxFace`).
    pub box_face_failures_propagate: bool,
    /// `SmBrep::DeleteFilletFace` removes a ring fillet (a round all the way round a boss's foot or rim: two closed tangent rails, one vertex each, and at most one seam) with the fork's `SmRingFilletRemoval`: the sharp edge is the intersection of the two tangent faces' surfaces within their natural domains (an exact circle for a boss), the tangent face's seam runs on to it along its line, and the rails become it; a removal that would need a surface or a curved edge continued past its domain is refused before the B-rep changes. Upstream takes four-sided fillets only, and fails on a ring (WR-605; SolidCraft change, tangency-labs/smlib 52bfef0, with 661f79c under `glue_vertices_joins_every_region`).
    pub fillet_removal_takes_ring_fillets: bool,
    /// `SmMerge::ManifoldBoolean` refuses a union (`SM_ERR`, the face in the error) when a face its STEP 6 would delete has the infinite region on one side and a solid region on the other: deleting it would merge that solid into the outside. Upstream deletes it: where two operands' faces stay within tolerance of each other across a strip without being coincident (two 4 mm cubes, one turned 5.7 deg about z, within 5e-6 across a 1e-4 strip), the union deletes two such faces of the first operand, both of its solid regions merge into the infinite region, and the union comes back with no solid region and volume 0 (WR-619, #1794; own design, not in the fork).
    pub boolean_union_refuses_solid_leak: bool,
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
    /// `SmFace::GetPointsInFace` samples a rectangularly trimmed face from its UV domain's
    /// minimum: each column starts at the domain's minimum u plus the column increment and
    /// each row at the domain's minimum v plus the row increment, and the column increment
    /// is the domain's u length over the number of columns plus one. Upstream samples
    /// `(k * dUInc, j * dVInc + dither)` as fractions of the domain's lengths from zero, so
    /// a face whose domain does not start at the origin gets points outside it, and its
    /// `dUInc` kept the divisor of the `min(2 * count, 30) + 1` search count after the
    /// rectangular branch reset the column count to `min(count, 30)`, so the columns did
    /// not span the domain (WR-86, #1909). This corrects the origin and the column spacing,
    /// not uniform coverage of the domain for every count: counts above 30 still plan only
    /// `min(count, 30)` columns and stop at the requested count, so when those rows do not
    /// divide the requested count the last sampled column is truncated and later columns
    /// are not sampled (31 points fill 16 of 30 columns), and with more than one point the
    /// V dither still comes from `kernel.rng`. Off by default (pinned behaviour).
    pub face_points_span_domain: bool,
    /// `sm_CheckSeamCrossing` returns `false` (no seam crossed) for a surface closed
    /// in neither direction (`!closed_u && !closed_v`). Upstream returns `SM_SUCCESS`
    /// (1000), which reads as `true` from a function documented to return whether a
    /// shift happened (WR-87; `plan/upstream-issues/WR-87--sm-checkseamcrossing-returns-sm-success-as-its-smboolean.md`
    /// and its 2026-10-31 addendum). Off by default for pinned C++ parity.
    pub check_seam_crossing_unclosed: bool,
    /// `SmBSplineSurface::IsParallelToVector` (and `SmPlane`'s inherited branch) returns
    /// `false` when `vector` cannot be unitised, and tests subsequent control points of
    /// each row or column against the chord direction `(line_end - line_pnt).unitize()`
    /// rather than `line_end`. Upstream returns `SER(sTestVec.Unitize())`'s
    /// `SM_ERR_INVALID_INPUT` (1004) as `SmBoolean` (`true`) for a zero vector and passes
    /// `sLineEnd` as the line direction to `smgu_LinePointDistance(sLinePnt, sLineEnd, ..)`
    /// (WR-39, #2145;
    /// `plan/upstream-issues/WR-39--addendum-2026-10-31-parallel-to-vector.md`). Off by
    /// default for pinned C++ parity.
    pub surface_parallel_to_vector_checks: bool,
    /// The ASCII database writer (`SmDatabaseIO`, `SmBrep::WriteToFile(.., SM_ASCII,
    /// ..)`) writes each double in the shortest form that reads back as the same
    /// double, in exponent notation (`3.0000000000000004e-1`), where upstream's stream
    /// is `fixed` with precision 16 (`%.16f`): that keeps 16 digits after the point,
    /// fewer than 17 significant digits below 1, so `0.3` plus 1 ULP reads back as
    /// `0.3` and a sphere's read-back volume drifts by 1.3e-5 relative (WR-655, the
    /// loss seen in #1485). Both kinds of file read back with every reader (`>>`
    /// accepts the exponent); binary files are lossless either way. Off by default
    /// (pinned behaviour: the same bytes as the C++ writer).
    pub ascii_doubles_round_trip: bool,
    /// A round whose filleted edge runs on tangentially at a vertex into an edge of its own convexity that its rail on that face runs alongside and never meets (an R2 corner arc of a plate's outline between straight sides, picked alone) ends at its section through the vertex: the round is cut there by the plane normal to the filleted edge, and the region between the round, its faces and the end sections is subtracted from the body, a flat end face (the section's corner) left on each such end. With `fillet_nx1_end_rib_global_merge` the end rib was deleted for the global merge, which cannot cut a round lying along the continued edge, and `DoFilleting` left an open body (`fillet_edges` refused it as not closed); pinned SMLib fails the corner (`SM_FILERR_VERTEX_PROBLEM`, 1001). Only a single round whose both ends are such ends is built; any other combination is refused with `FilletErrorType::NoRoom` (WR-674, #2176; own design, not in the fork, whose `CalcCornerEdgeGeom` takes the global merge as WR-303 does).
    pub fillet_round_ends_at_tangent_continuation: bool,
    /// Constant and variable-radius rounds between NURBS faces are built as their definition, a [`crate::surface::procedural_blend::ProceduralBlend`] (the fork's `SmProceduralBlend`): the two faces the ball rolls on, its radius or radius law, the section's order and thumbweight rule and the leader, which evaluate exactly; its net, the certified B-spline approximation within the certificate's bound (the radius law in the certificate), is what the surface cache and tessellation use (`SmBlendNetScope`), the cache's node boxes grown by the net's bound. With it the file reader reads a definition (`SmProceduralBlend_TYPE`, `SURF_BASE_TYPE + 41`); without it that type is refused as the pinned reader refuses it (WR-698, #2273; the fork's tangency-labs/smlib@014f66f to 729f893, smlib#79 step 7, merged at 5e8c9c1).
    ///
    /// Not the fork's (#2486): a chamfer (the linear section) of a constant-radius solver's two offsets between NURBS faces, or of a constant-distance solver's chord between planes, is built as its definition too, the section generator's order 0: the straight chord between the feet of the point at the two offsets off the faces, certified as any section; between planes it is the exact plane through the feet, a `Plane` at a bound of 0 carrying its definition ([`crate::surface::procedural_blend::exact::ExactClass::Chord`], reached through [`crate::surface::surface::Surface::procedural_blend`]); either way keeping the chamfer's sizes (two distances, a distance and an angle, or a chord, with the wedge angle they were converted at; [`crate::surface::procedural_blend::chamfer`], [`crate::blend::fillet_executive::session::FilletSession::prepare_chamfers`]). The record and the database carry the sizes.
    pub fillet_round_as_definition: bool,
    /// With `fillet_cliff_overflow` (and its prerequisites), a blend past its faces is trimmed at its creases where it is built instead of being closed into the region it removes and subtracted (the fork's `SmFilletExecutive::TrimAtCreases`, `SmFilletOverflow.cpp`, tangency-labs/smlib#79 step 6 at 5e8c9c1, which deletes `SubtractRemovalRegion`): the open ends are cut by their end faces' planes as before; each sheet face is sectioned by every body face near it that holds none of its boundary, exactly by a plane, and split along each crease where it lies inside both faces; a piece is kept only when its inner point lies inside the body; a planar support face whose kept rail runs past its boundary into the material is carried on along its plane to the rail; the kept sheet's boundary is imprinted on the body's faces, the faces it encloses round the blended edges are deleted, the sheet is merged in and stitched, the body oriented, and the edges between faces sewn in on one surface removed. The blend keeps its radius and the sharp edge moves. Refused with `FilletErrorType::NoRoom`: a crease on a face that runs on tangentially from a face the blend rolls on (`NoRoomReason::CreaseAcrossTangentEdge`), a blend whose corners take its whole edge (`CornersTakeWholeEdge`), a blend that keeps no length of its edge where no corner takes any of it (`OverflowKeepsNothing`, with a knife edge's chamfer between a plane and a tangent cylinder sized to where its chord takes the whole body), and a rail into the material that the support face cannot be carried on to (`RailRunsPastFaceIntoMaterial`); `BadMerge` for a sheet that does not sew into a manifold solid. Off: the subtraction, as before (WR-695, #2273).
    pub fillet_overflow_trim_at_creases: bool,
    /// With both `fillet_overflow_trim_at_creases` and `fillet_round_ends_at_tangent_continuation`, a round with a section end (WR-674: its filleted edge runs on tangentially at the vertex) is trimmed at its creases and sewn into the body as a cliff overflow is, instead of being closed into the region it removes and subtracted: each section end is cut by its plane through the vertex normal to the filleted edge and given its flat end face (the section's corner, from the section's ends to the vertex), and the sheet is trimmed and sewn in as `fillet_overflow_trim_at_creases` does it; a side of an end face that lies along an edge of the body (the seam where a corner's cylinder runs on tangentially into a flat side) is sewn to that edge's piece instead of being imprinted. So a round with a section end is built together with other rounds (an R25 rim and an R2 corner arc of a tabbed plate rounded together, which the subtraction refuses with `FilletErrorType::NoRoom`: it builds only a lone round both of whose ends are section ends). Off: the subtraction, as before (WR-706, #2223; own design, not in the fork, which at 5e8c9c1 leaves such a corner arc unrounded).
    pub fillet_section_end_trim_at_creases: bool,
    /// With `fillet_overflow_trim_at_creases`, a crease of a blend past its faces that runs along an edge of the face it crosses, within the trim's tolerance, is taken as that edge: the sheet is split along it, its piece past the face is deleted, and the body's edge stands as the imprint where the material taken off stops and the sheet is sewn (as an extension's edges along the body do). The fork's `TrimAtCreases` (tangency-labs/smlib#79 step 6 at 5e8c9c1) keeps only the crease's runs inside the face (`sm_PiecesInFace`), so such a crease is dropped, the sheet keeps its piece past the face, the face is taken off with the rest, and the sewing is refused (`SM_FILERR_BAD_MERGE`): a disc 13 in radius and 2 high rounded 2.0001 to 2.01 on its top edge (the crease runs (r - 2)^2/4 inside the bottom edge) fails where r 2 and r 2.02 build. Not the fork's (WR-717, #2385).
    pub fillet_overflow_crease_along_edge: bool,
    /// A convex blend whose section between its rails would break through the part (leave its material through a face and come back into it, where one of the faces it crosses there shares no edge with the blended faces: a hollow behind a wall, such as a round on a wall thinner than it reaches into the hollow behind, or a blind pocket or cavity under a blended face, which the pinned construction builds into a body that intersects itself or has the wrong volume) is refused with `FilletErrorType::NoRoom` (`NoRoomReason::BreaksThrough`), naming the largest size whose sections do not break through, and that size less the fit margin is the refusal's fit (`SolidError::FilletRefused`). A section that runs off its face (past a cliff, into a hole or an opening: the part beyond it reaches a rail) or through a hole in a blended face does not break through. The same 32 sections as `BandCutsThePart` are judged; the faces at the blended edge's ends, which the corners trim, are not ([`crate::blend::fillet_executive::session::band_cuts::band_breaks_through_the_part`]). Not the fork's: fork 5e8c9c1 segfaults in `DoFilleting` on #2299's case (WR-707, #2299; addendum `plan/upstream-issues/WR-707--addendum-2026-11-02-rounds-running-off-their-faces.md`).
    pub fillet_refuse_rounds_breaking_through: bool,
    /// A face made from exact pcurves keeps them as its edge uses' trims where upstream drops them and projects each edge's curve again later, a fit: the draft extrusion's curvature-keeping and tangent-plane walls ([`crate::primitive::primitive_creation::p2::PrimitiveCreationAccess::face_with_curves`]) pass theirs to `SmBrep::MakeFaceWithCurves` with `pbOptCrvOnSurf` set, and `SmBrep::CopyFaces` without the merge passes a face's own trims (`CreateCurvesFromFace`) on to its copy. And `MakeFaceWithCurves`'s keep step (`CreateUVTrimCurves(&uv, &orients)`) flips the orientations for a face made with its surface's orientation opposite: they are given along the loop, and `sm_FindBestUVCurve` matches them against the upward face use's edge uses, which run the other way there, so upstream's tangent test refuses every exact curve and each is projected again. A draft half disc's cone wall then keeps its seam pcurves on their edges to 1.2e-13, where upstream's are 2.4e-6 off. And an arc's draft wall on its cone covers just its face (from its seams' least angle to their greatest, exact), not the seams' padded reach, which `SmBrep::ShrinkGeometry` trimmed to the face where the pad passed a thousandth of the face's size, removing the exact trims to project the edges again; so a seam cut at a span end of the cone keeps its quartics (#2497) (WR-722, #2405; solidcore-native, no fork commit).
    pub keep_exact_pcurves: bool,
    /// A face's planar section with no approximation tolerance given (`SmBrep::CreatePlanarSectionCurves` and `SmSurface::CreatePlanarSectionCurves` through [`crate::topology::brep::planar_section::face_planar_section_curves`], `SmApiIntersectBrepWithPlane`) is traced to the face's tolerance, as the curves are to lie within it of the face, where upstream passes the null on and the surface intersector traces to its default, 1/1000 of the surface's size: a G^n round's section on a 40 x 30 x 20 box left the round by 5.2e-3 at a face tolerance of 1.6e-4 (WR-723, #2451; solidcore-native, no fork commit).
    pub planar_section_face_tolerance: bool,
    /// `SmFace::SwapUV` transposes each UV trim by swapping its control points' `u` and `v` ([`crate::nurbs::b_spline_curve::BSplineCurve::create_transposed_curve`]), where upstream mirrors it about the plane through the origin with x axis `(1, 1, 0)` unitised: the rounded unit vector makes `N_CrvTransform`'s matrix `I - 2nn^T` `-2.2e-16` on the diagonal and `1 + 2.2e-16` off it, so every coordinate of a swapped face's trims moves by up to an ulp and a trim's end on a domain bound lands past it (a unit box's three faces `StitchAndOrient` swaps end at `v = -2.2e-16` and `1 - 1.1e-16` on `[0, 1]`). Every operation that orients or reverses faces passes its trims through it, so their ends then lie in the face's domain to rounding as they are made (WR-729, #2479; solidcore-native, no fork commit).
    pub face_swap_uv_transposes_trims: bool,
    /// `N_SrfIsClosedSmooth` ([`crate::nurbs::srf_basic::Surface::is_closed_smooth_with_fixes`]) takes the G1 test across the seam at a station where the derivative across the seam vanishes (a collapsed row, a pole on the seam: shorter than `0.001 * NL_MTOL`, the pole test of `N_SrfEvalPtNormalAtPole`) by its limit, the mixed derivative `S_uv` (`S_uv` for the v seam), negated at the end of the domain, where upstream takes the angle of the two vanishing vectors: their rounding then decides between `NL_GEO_ERR` (two exact zeros), not smooth (two noise vectors at any angle) and smooth, so a sphere is not reported closed and smooth in u (WR-725, #2521; solidcore-native, no fork commit).
    pub closed_smooth_pole_tangent_limit: bool,
    /// With `fillet_cliff_overflow` (and `fillet_refuse_rails_leaving_faces` and `fillet_exact_round_classes`, which it builds on), a lone constant round bigger than the wall it rolls on ends on the body's outline instead of being built on the wall continued past its rim and cut from the body. Where the round's rail runs past a rim of its face (a convex, not tangent edge of the face that runs the filleted edge's whole length) and the ball rolling on the continued face passes inside that rim by more than the body's tolerance (so the round would take material beyond the face), the ball touches the rim instead: it stays tangent to the other face and its section passes through the rim. Where that ball would land on the other face past that face's own rim, it touches both rims, its section the arc through them. The solver rolls the ball on the surface tangent to it along each rim it touches, in place of the face: the face's plane turned about a straight rim, or the wall's cylinder leaned into the cone through a circular rim (both keep the face's parameterization along the rim), so the round is built as a round as tall as its wall, exactly (a cylinder or a torus). Decided in the round's section ([`crate::blend::fillet_outline::outline_supports`]) for a round along a straight edge between two planes, or along a circular edge between a plane square to its axis and a coaxial cylinder; any other round is left to the cliff overflow. A 100 x 100 x 10 plate rounded r 90 on a top edge keeps 86 577.030 mm³, and the round never takes the whole plate: past r = 505 its section runs from the bottom edge to the far top edge. Off: the round on the continued wall, as before (WR-730, #2541; solidcore-native, no fork commit).
    pub fillet_round_ends_on_outline: bool,
    /// Chamfers meeting at a vertex where every edge is chamfered, each chamfer a plane
    /// between two planes, are closed by the chamfer planes themselves, exactly: the blend
    /// corner's chamfer event ([`crate::blend::blend_corner::chamfer`]), in place of the
    /// pinned 3x3 corner's chamfer branch (`SmFilletConvexNxNCorner` with `SetChamfer`,
    /// convex and concave alike). The corner type is the session's
    /// (`FilletSession::set_chamfer_corner`): the Mitre (the default), where each chamfer
    /// runs on to the others and three of them meet at the point M of their 3x3 solve,
    /// pairwise along the exact lines from the rails' crossings to M, with no corner face;
    /// or the Facet, one plane through the rails' crossings whose edges are the chamfers'
    /// end sections, exact lines in their planes. A mitre point outside the corner (a
    /// chamfer much wider than the others) is refused with `FilletErrorType::NoRoom`,
    /// naming the largest size of that chamfer that keeps it inside, less the fit margin,
    /// as the refusal's fit; the facet is refused there too, its plane leaving the
    /// chamfers. Four or more chamfers whose crossings lie in one plane build the facet
    /// (their mitre is refused, naming why); where their crossings are not in one plane the
    /// corner is a patch, whichever type was asked: the chamfers stop at their crossings and
    /// n bilinear quadrilaterals round the crossings' centroid close the skew polygon of
    /// their end sections, G0 to each other and to each chamfer (at four, the one bilinear
    /// patch of the crossings), within the crossings' convex hull and so in the corner.
    /// Chamfer planes so nearly coplanar that the points where they
    /// meet are not determined to the part's rounding are refused with
    /// `FilletErrorType::CornerUnsupported`, naming that. The pinned corner marches the
    /// mitre's edges as surface intersections, refuses a mitre of four or more chamfers with
    /// no reason, and builds its corner face as a Coons patch where the chamfers are unequal
    /// (WR-716, #2345; own design, smlib#89's brief, the owner's answers on #2345). For the
    /// port in progress only: at parity it becomes the default and the pinned corner's
    /// chamfer branch is deleted (WR-716's retirement plan).
    pub fillet_chamfer_corner: bool,
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
                w.ball_certified_roots(),
            )
        };
        assert_eq!(counts(w), (0, 0, 0, 0, 0, 0));
        w.add_face_pair();
        w.add_face_pair();
        w.add_loop_classified();
        w.add_wall_body_copy();
        w.add_proto_edge_intersection();
        w.add_loop_walk();
        w.add_loop_walk();
        w.add_loop_walk();
        w.add_ball_certified_root();
        assert_eq!(counts(w), (2, 1, 1, 1, 3, 1));
        // each kernel counts its own work
        assert_eq!(Kernel::default().work_counters.face_pairs(), 0);
        w.reset();
        assert_eq!(counts(w), (0, 0, 0, 0, 0, 0));
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
