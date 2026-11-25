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
//! | [`Rng`], [`Kernel::seed`], [`Kernel::seeded_rng`] | C `rand()` / `srand(time(NULL))` (e.g. `SmShell.cpp`) |
//! | [`Caches`]                             | `SmCacheMgr`, `SmCacheMgrCrv/Srf/TSrf/Brep` |
//! | [`Kernel::upstream_ub_passed`], [`Kernel::note_upstream_ub`] | (none: upstream undefined behaviour the port went past) |
//! | [`CancelToken`], [`Kernel::cancel`], [`Kernel::set_cancel_token`], [`Kernel::check_cancelled`] | (none: cooperative cancellation for worker-thread jobs) |
//! | [`WorkCounters`], [`Kernel::work_counters`] | the SolidCraft fork's `SmWorkCounters` (process-wide there) |
//!
//! Upstream seeds `rand()` from the clock in a few places; the port replaces that with an
//! [`Rng`] each such call makes for itself from [`KernelOptions::seed`]
//! ([`Kernel::seeded_rng`]), so a call's draws are the same whatever ran before it. Results
//! that depend on random draws therefore cannot match the C++ bit for bit; difftests of such
//! code must compare outcomes that are invariant to the draw (and say so in
//! `plan/findings.md`).
//!
//! Upstream also keeps a few process-global function statics whose first use fixes
//! their value for the rest of the process. The kernel keeps no such state: a value
//! upstream fixes on first use is chosen here from the call's own input (the
//! finite-difference step of `SmFilletSolver::ComputePointValues`, `sdStaticStep`, is
//! chosen for each point), so a result never depends on the calls before it.

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
#[derive(Debug)]
#[non_exhaustive]
pub struct Caches {
    /// Not upstream (#2568): the results of `SmBSplineCurve::AssertValid`'s global
    /// property analyses by the exact content of their inputs, so a curve checked again
    /// is not solved again. A [`RefCell`] so the check can fill it through the `&Kernel`
    /// it takes (the kernel stays `Send`; it is not shared between threads).
    pub(crate) curve_analysis: RefCell<crate::nurbs::b_spline_curve::analysis::AnalysisMemo>,
    /// Not upstream (#18166): whether a lazily built UV trim raises its edge's tolerance by
    /// the trim's proved distance from the edge (`topology::edgeuse::p3::trim_raise_distance`),
    /// as it does by default, or by upstream's sampled distance, as a file's translation asks
    /// through [`Kernel::with_sampled_trim_raise`]. Crate-private: this is not a kernel
    /// option, only the import's narrow seam. (tangency-labs/solidcore#3368 makes the proved
    /// raise fast enough for import and removes the sampled path.)
    pub(crate) prove_lazy_trim_raise: bool,
    /// Not upstream (#21105): whether a UV trim built now leaves its edge's tolerance as
    /// it is, as `SetUVTrimCurve` does on a B-rep marked `m_bValidateOnly`. Set only for
    /// the duration of a validate-only tolerance pass ([`Kernel::with_held_trim_raise`]),
    /// which so takes `bValidateOnly` as its argument instead of setting and restoring the
    /// flag on the body it checks. Crate-private: not a kernel option.
    pub(crate) hold_trim_raise: bool,
}

impl Default for Caches {
    fn default() -> Self {
        Self {
            curve_analysis: RefCell::default(),
            prove_lazy_trim_raise: true,
            hold_trim_raise: false,
        }
    }
}

impl Caches {
    /// Drops all cached data. The trim raise's modes stay: clearing a kernel's caches must
    /// change no result.
    pub fn clear(&mut self) {
        *self = Self {
            prove_lazy_trim_raise: self.prove_lazy_trim_raise,
            hold_trim_raise: self.hold_trim_raise,
            ..Self::default()
        };
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
    shell_traversal_faces: Cell<u64>,
    proto_vertex_point_solves: Cell<u64>,
    merge_record_visits: Cell<u64>,
    infinite_region_searches: Cell<u64>,
    face_extreme_control_points: Cell<u64>,
    feet_tm_coefficients: Cell<u64>,
    mass_iso_evaluations: Cell<u64>,
    mass_closure_evaluations: Cell<u64>,
    coincident_curves_classified: Cell<u64>,
    containment_edge_drops: Cell<u64>,
    loop_walks: Cell<u64>,
    target_record_seedings: Cell<u64>,
    classification_snapshots: Cell<u64>,
    orient_boxes: Cell<u64>,
    ball_certified_roots: Cell<u64>,
    curved_edge_runs: Cell<u64>,
    blends_recertified: Cell<u64>,
    vertex_drops: Cell<u64>,
    seam_tests: Cell<u64>,
    contact_stretch_certifications: Cell<u64>,
    uv_trim_builds: Cell<u64>,
    trim_deviation_bounds: Cell<u64>,
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
    /// `make_edge_in_face_with_vu` goes on to classify after its box test (WR-527).
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

    /// Faces marked by shell traversals (`SmTopologyTraverser::ShellTraversal`): each
    /// face a walk over what a shell joins to a vertex reaches. Deleting a face starts
    /// one to learn whether the face's vertices are still joined to the shell. Not the
    /// fork's.
    #[must_use]
    pub fn shell_traversal_faces(&self) -> u64 {
        self.shell_traversal_faces.get()
    }

    /// Point solves a boolean ran to learn whether a proto vertex lies on a proto edge
    /// (`RemoveCoincidentNoEdgeProtoVertex`): each solve of a proto vertex's position
    /// against a proto edge's curve. A proto vertex found on a proto edge is removed
    /// only when it has no proto edges of its own. Not the fork's.
    #[must_use]
    pub fn proto_vertex_point_solves(&self) -> u64 {
        self.proto_vertex_point_solves.get()
    }

    /// The work done on the notification records of the B-reps merged into one list operand
    /// (`BooleanLists`' `MergeBrep` of each B-rep after the first,
    /// `boolean::merge::p3::combine_brep`): one for each arena slot, key or record a pass
    /// touches in the reserved-key check, the seeding of default records and the reset of a
    /// kept store, for the operand and for the B-rep merged into it
    /// (`NotifyRecords::note_visits` says what a pass is and what is outside the count). Not
    /// the fork's.
    #[must_use]
    pub fn merge_record_visits(&self) -> u64 {
        self.merge_record_visits.get()
    }

    /// Searches for a B-rep's infinite region (`FindInfiniteRegion`): each time a B-rep
    /// of more than one region goes on to choose a ray and classify it. Not the fork's.
    #[must_use]
    pub fn infinite_region_searches(&self) -> u64 {
        self.infinite_region_searches.get()
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

    /// Integrand evaluations of the ruled strips the precise mass properties close a face
    /// onto its edges' curves with (`gap_closure`): each point of a strip's trim the
    /// strip's integration samples. Not the fork's.
    #[must_use]
    pub fn mass_closure_evaluations(&self) -> u64 {
        self.mass_closure_evaluations.get()
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

    /// Curved intersection pieces classified as running along a face edge
    /// (`CurveClassification::classify_edge_interval`, WR-305): each one certified within the
    /// intersection tolerance of it as a whole (`curve_runs_along`, or the worst-first bound of
    /// #18065). Not the fork's (a test's witness that a body reaches the
    /// certificate).
    #[must_use]
    pub fn curved_edge_runs(&self) -> u64 {
        self.curved_edge_runs.get()
    }

    /// Procedural blends recertified as a record's faces got their definitions back
    /// (`surface::procedural_blend::convert::definition_surface`): each certificate that
    /// ran, not each blend [`Kernel::recertify_memo`] answered. Not the fork's (a test's
    /// witness that an unchanged blend is certified once, #17640).
    #[must_use]
    pub fn blends_recertified(&self) -> u64 {
        self.blends_recertified.get()
    }

    /// Face vertices a curve being classified on a face is solved against (the drops of
    /// `CurveClassification::intersect_vertices_in`): each vertex not passed over as out of
    /// the curve's reach. Not the fork's.
    #[must_use]
    pub fn vertex_drops(&self) -> u64 {
        self.vertex_drops.get()
    }

    /// Edges and vertices of a face asked whether they are a seam or a pole of it while a
    /// face intersection classifies its curves on the face (docs/booleans.md, "Seams and
    /// poles"): each edge [`crate::topology::edge::face_on_both_sides`] is asked about and
    /// each vertex [`crate::topology::vertex::inside_face`] is asked about, for a curve's
    /// boundary edges and vertices, its reach and the face's own seam. A face without a
    /// seam has no pole either, so once its edges are found to hold none, its curves skip
    /// both questions until the B-rep changes. Not the fork's (#18705).
    #[must_use]
    pub fn seam_tests(&self) -> u64 {
        self.seam_tests.get()
    }

    /// Stretches a curve intersection certified to keep its two curves within the
    /// tolerance, from a point solution to an end of either curve, to learn whether the
    /// solution moves to that end (`move_contacts_to_curve_ends`): each certificate
    /// subdivides the stretch. The count grows with the point solutions near an end, so
    /// two curves that coincide within the tolerance along many turns, crossing at
    /// thousands of points, show here. Not the fork's.
    #[must_use]
    pub fn contact_stretch_certifications(&self) -> u64 {
        self.contact_stretch_certifications.get()
    }

    /// UV trim curves built for an edge use
    /// (`topology::edgeuse::p3::create_uv_trim_curve`): each build, whether it
    /// attaches its curve or fails. Not the fork's (#21105).
    #[must_use]
    pub fn uv_trim_builds(&self) -> u64 {
        self.uv_trim_builds.get()
    }

    /// Certified bounds on a new UV trim's distance from its edge
    /// (`topology::edgeuse::p3::trim_deviation_bound`, and its same-parameter form
    /// `trim_same_parameter_deviation_bound` that gates a boolean's carried trims): each
    /// bisection the bound starts, the costly part of building a trim. Not the fork's (#21105).
    #[must_use]
    pub fn trim_deviation_bounds(&self) -> u64 {
        self.trim_deviation_bounds.get()
    }

    /// Zeroes every count (`SmWorkCounters::Reset`).
    pub fn reset(&self) {
        self.face_pairs.set(0);
        self.loops_classified.set(0);
        self.wall_body_copies.set(0);
        self.proto_edge_intersections.set(0);
        self.shell_traversal_faces.set(0);
        self.proto_vertex_point_solves.set(0);
        self.merge_record_visits.set(0);
        self.infinite_region_searches.set(0);
        self.face_extreme_control_points.set(0);
        self.feet_tm_coefficients.set(0);
        self.mass_iso_evaluations.set(0);
        self.mass_closure_evaluations.set(0);
        self.coincident_curves_classified.set(0);
        self.containment_edge_drops.set(0);
        self.loop_walks.set(0);
        self.target_record_seedings.set(0);
        self.classification_snapshots.set(0);
        self.orient_boxes.set(0);
        self.ball_certified_roots.set(0);
        self.curved_edge_runs.set(0);
        self.blends_recertified.set(0);
        self.vertex_drops.set(0);
        self.seam_tests.set(0);
        self.contact_stretch_certifications.set(0);
        self.uv_trim_builds.set(0);
        self.trim_deviation_bounds.set(0);
    }

    /// Counts one UV trim curve built.
    pub(crate) fn add_uv_trim_build(&self) {
        self.uv_trim_builds
            .set(self.uv_trim_builds.get().saturating_add(1));
    }

    /// Counts one certified bound on a trim's distance from its edge.
    pub(crate) fn add_trim_deviation_bound(&self) {
        self.trim_deviation_bounds
            .set(self.trim_deviation_bounds.get().saturating_add(1));
    }

    /// Counts one edge or vertex asked whether it is a seam or a pole of a face.
    pub(crate) fn add_seam_test(&self) {
        self.seam_tests.set(self.seam_tests.get().saturating_add(1));
    }

    /// Counts one stretch certified for a contact's move to a curve end.
    pub(crate) fn add_contact_stretch_certification(&self) {
        self.contact_stretch_certifications
            .set(self.contact_stretch_certifications.get().saturating_add(1));
    }

    /// Counts one vertex a classified curve is solved against.
    pub(crate) fn add_vertex_drop(&self) {
        self.vertex_drops
            .set(self.vertex_drops.get().saturating_add(1));
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

    /// Counts one face marked by a shell traversal.
    pub(crate) fn add_shell_traversal_face(&self) {
        self.shell_traversal_faces
            .set(self.shell_traversal_faces.get().saturating_add(1));
    }

    /// Counts one point solve of a proto vertex against a proto edge.
    pub(crate) fn add_proto_vertex_point_solve(&self) {
        self.proto_vertex_point_solves
            .set(self.proto_vertex_point_solves.get().saturating_add(1));
    }

    /// Counts `visits` slots and keys visited for a list operand merge's records.
    pub(crate) fn add_merge_record_visits(&self, visits: u64) {
        self.merge_record_visits
            .set(self.merge_record_visits.get().saturating_add(visits));
    }

    /// Counts one search for a B-rep's infinite region.
    pub(crate) fn add_infinite_region_search(&self) {
        self.infinite_region_searches
            .set(self.infinite_region_searches.get().saturating_add(1));
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

    /// Counts one evaluation of a mass-property closure strip's integrand.
    pub(crate) fn add_mass_closure_evaluation(&self) {
        self.mass_closure_evaluations
            .set(self.mass_closure_evaluations.get().saturating_add(1));
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

    /// Counts one curved piece certified to run along a face edge.
    pub(crate) fn add_curved_edge_run(&self) {
        self.curved_edge_runs
            .set(self.curved_edge_runs.get().saturating_add(1));
    }

    /// Counts one procedural blend recertified as its record's face got it back.
    pub(crate) fn add_blend_recertified(&self) {
        self.blends_recertified
            .set(self.blends_recertified.get().saturating_add(1));
    }
}

/// Emits the [`UpstreamFixes`] definition it wraps unchanged, plus
/// [`UpstreamFixes::NAMES`], [`UpstreamFixes::flags`], [`UpstreamFixes::all`] and
/// [`UpstreamFixes::flag_mut`]
/// read from that same field list, so a field is listed by name with no second
/// place to edit (#2014). It
/// accepts only `pub <name>: bool` fields (each with its doc comment), so a field of
/// another type fails to compile instead of being left out of the list.
///
/// The field list itself is generated: `build.rs` reads `src/kernel/fixes/`, one file per
/// field (`<field>.md`, holding the field's doc), and writes `upstream_fix_fields!`, which
/// passes the struct's attributes and name given to it here with every field, sorted by
/// name. So adding or retiring a flag adds or deletes its own file, and no line that
/// another flag's change touches.
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
            /// The name of every flag (its field name), sorted by name (the fields'
            /// declaration order).
            pub const NAMES: &'static [&'static str] = &[$(stringify!($field)),*];

            /// Every flag as `(name, on)`, sorted by name: the names of
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

// `upstream_fix_fields!`: the field list, one file per flag in `src/kernel/fixes/`
// (generated by build.rs; see `upstream_fixes!`).
include!(concat!(env!("OUT_DIR"), "/upstream_fixes.rs"));

upstream_fix_fields! {
/// Opt-in fixes of upstream SMLib defects (`plan/upstream-issues/`), all off by default
/// (upstream behaviour). Set them on [`Kernel::fixes`] before running operations; each
/// field names its record (`WR-…`) and the `tangency-labs/solidcore` issue.
///
/// [`UpstreamFixes::flags`] lists every field with its value and
/// [`UpstreamFixes::NAMES`] every field name, both sorted by name;
/// [`UpstreamFixes::flag_mut`] finds a field by its name; [`UpstreamFixes::all`] turns
/// every field on. `upstream_fixes!` generates
/// them from this definition. Each field is its own file, `src/kernel/fixes/<field>.md`
/// holding its doc, so adding or retiring a flag adds or deletes that file and edits no
/// shared list.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
#[non_exhaustive]
pub struct UpstreamFixes
}

/// Options for [`Kernel::new`].
#[derive(Debug, Clone, PartialEq)]
pub struct KernelOptions {
    /// Tolerances to use.
    pub tolerances: Tolerances,
    /// Seed of the generators the kernel's sampling calls make ([`Kernel::seeded_rng`]).
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
    /// The seed of every generator a call makes for its random draws
    /// ([`Kernel::seeded_rng`]; from [`KernelOptions::seed`]). Configuration only: no call
    /// changes it, so the kernel holds no position in a random sequence.
    pub seed: u64,
    /// Memoisation caches.
    pub caches: Caches,
    /// The session's modeling context (marks, Boolean flag, callbacks, its own
    /// model size estimate); created with the defaults, as `SmApiGetOrCreateContext`.
    pub context: Context,
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
    /// Not upstream: the memo of the loop area signs this kernel certified
    /// ([`crate::mass::loop_area::LoopAreaMemo`]), on with [`Kernel::memoize_curve_caches`].
    /// A [`RefCell`]: the certificate runs through `&Kernel`; it is never borrowed across a
    /// certificate, and a hit returns what certifying the loop again returns.
    pub loop_area_memo: RefCell<crate::mass::loop_area::LoopAreaMemo>,
    /// Not upstream: the memo of the procedural blends this kernel recertified as it gave a
    /// record's faces their definitions back
    /// ([`crate::surface::procedural_blend::convert::RecertifyMemo`]), on with
    /// [`Kernel::memoize_curve_caches`]. A [`RefCell`]: it is never borrowed across a
    /// certificate, and a hit returns what recertifying the same blend again returns.
    pub recertify_memo: RefCell<crate::surface::procedural_blend::convert::RecertifyMemo>,
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
    /// Not upstream; diagnostic, tests only: while `Some`, each Coons corner patch made
    /// tangent to its sides appends the record of its refinement (every candidate, its
    /// stations and side angles, and the one kept), for tests that replay the choice.
    /// No library code reads it. `None` (the default)
    /// keeps nothing; it changes no result. A [`RefCell`] so the corner blend can append
    /// through the `&Kernel` it takes (the kernel stays `Send`).
    pub coons_tangency_log:
        RefCell<Option<Vec<crate::nurbs::b_spline_surface_n_lib::p2::CoonsTangencyLog>>>,
    /// Not upstream: the profiler of the operations run on this kernel (feature
    /// `profile`; [`crate::profile`]). It only observes: no result reads it. Shared with
    /// the kernel copies an operation makes for its caches, which record into the same
    /// tree.
    #[cfg(feature = "profile")]
    pub profiler: crate::profile::Profiler,
    /// Test-only seam of the curved coincident strip's drop ([`StripProbe`]); shared with
    /// the kernel copies an operation makes.
    #[cfg(test)]
    pub(crate) strip_probe: Arc<std::sync::Mutex<StripProbe>>,
    /// Not upstream, test only (feature `wr57-replay`): the seam of the goldens' WR-57
    /// substitution replay ([`crate::surface::surface::replay`]). No hook by default, and
    /// then it changes nothing. Shared with the kernel copies an operation makes for its
    /// caches, which call through the same seam.
    #[cfg(feature = "wr57-replay")]
    pub replay: crate::surface::surface::replay::ReplaySeam,
}

/// Test-only seam of `boolean::topology_intersector::curved_strip`
/// (tangency-labs/solidcraft#18166): where an operation's topology intersector drops the
/// curves over curved coincident strips, each run in `runs` is made first on a copy of the
/// operation's real intersection list, and the number of intervals it drops appended to
/// `dropped`. `active` is the run being made; its perturbations are read by the drop's
/// checks.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct StripProbe {
    /// The runs still to make (taken by the first drop reached).
    pub(crate) runs: Vec<StripProbeRun>,
    /// The intervals each run dropped, in order.
    pub(crate) dropped: Vec<usize>,
    /// The run being made.
    pub(crate) active: Option<StripProbeRun>,
}

/// One run of [`StripProbe`]: the drop at tolerance `tol`, with `extra` added to the
/// strip's enclosure, the dropped curve's gap and N's piece's gap (in that order), and,
/// with `event_on_n`, a synthetic interval end on N at the middle of its piece.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct StripProbeRun {
    /// The tolerance the drop is called with.
    pub(crate) tol: f64,
    /// Added to the strip's bound, the curve's gap and N's piece's gap.
    pub(crate) extra: [f64; 3],
    /// A synthetic interval end on N inside its piece.
    pub(crate) event_on_n: bool,
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
            seed: options.seed,
            caches: Caches::default(),
            context: Context::default(),
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
            loop_area_memo: RefCell::default(),
            recertify_memo: RefCell::default(),
            blend_operation_scopes: Arc::default(),
            work_counters: WorkCounters::default(),
            blend_face_log: None,
            coons_tangency_log: RefCell::new(None),
            #[cfg(feature = "profile")]
            profiler: crate::profile::Profiler::default(),
            #[cfg(test)]
            strip_probe: Arc::default(),
            #[cfg(feature = "wr57-replay")]
            replay: crate::surface::surface::replay::ReplaySeam::default(),
        }
    }

    /// Internal to file import, not a kernel option: runs `f` with lazily built UV trims
    /// raised by upstream's sampled distance instead of #18166's proved one, then restores
    /// the proved raise, whether `f` returns a value or an error.
    ///
    /// A file's translation builds every trim of every face it reads, and proving each one
    /// cost many times the translation (a 561-part assembly opened in 126 s instead of
    /// 20 s). Its trims are then raised as before #18166. A panic instead drops the kernel
    /// (solidcraft-smlib's `call_on` never reuses a kernel a panic interrupted), so no
    /// kernel keeps sampled trims past the import.
    ///
    /// Do not call this outside file import: https://github.com/tangency-labs/solidcore/issues/3368
    /// makes the proved raise fast enough for import and then removes this path, so one
    /// implementation remains.
    #[doc(hidden)]
    pub fn with_sampled_trim_raise<T>(&mut self, f: impl FnOnce(&mut Kernel) -> T) -> T {
        let proved = self.caches.prove_lazy_trim_raise;
        self.caches.prove_lazy_trim_raise = false;
        let out = f(self);
        self.caches.prove_lazy_trim_raise = proved;
        out
    }

    /// Runs `f` with every UV trim it builds leaving its edge's tolerance as it is (see
    /// [`Caches::hold_trim_raise`]), then restores the previous mode, whether `f` returns a
    /// value or an error: the validate-only tolerance pass's `bValidateOnly`, carried for the
    /// pass instead of written to the B-rep it checks (#21105).
    pub(crate) fn with_held_trim_raise<T>(&mut self, f: impl FnOnce(&mut Kernel) -> T) -> T {
        let held = self.caches.hold_trim_raise;
        self.caches.hold_trim_raise = true;
        let out = f(self);
        self.caches.hold_trim_raise = held;
        out
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

    /// A generator at the start of the sequence of the kernel's [seed](Self::seed).
    ///
    /// A call that draws random numbers makes one for itself and draws from it, so what it
    /// draws is a function of the seed and of its own inputs, never of what ran on the
    /// kernel before: the same call on the same input draws the same numbers.
    #[must_use]
    pub const fn seeded_rng(&self) -> Rng {
        Rng::new(self.seed)
    }

    /// Records that an operation went past upstream undefined behaviour `id` (see
    /// [`Kernel::upstream_ub_passed`]).
    pub fn note_upstream_ub(&mut self, id: &'static str) {
        if !self.upstream_ub_passed.contains(&id) {
            self.upstream_ub_passed.push(id);
        }
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
                w.shell_traversal_faces(),
                w.proto_vertex_point_solves(),
                w.merge_record_visits(),
                w.infinite_region_searches(),
                w.curved_edge_runs(),
            )
        };
        assert_eq!(counts(w), (0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0));
        w.add_face_pair();
        w.add_face_pair();
        w.add_loop_classified();
        w.add_wall_body_copy();
        w.add_proto_edge_intersection();
        w.add_loop_walk();
        w.add_loop_walk();
        w.add_loop_walk();
        w.add_ball_certified_root();
        w.add_shell_traversal_face();
        w.add_proto_vertex_point_solve();
        w.add_merge_record_visits(3);
        w.add_infinite_region_search();
        w.add_infinite_region_search();
        w.add_curved_edge_run();
        assert_eq!(counts(w), (2, 1, 1, 1, 3, 1, 1, 1, 3, 2, 1));
        // each kernel counts its own work
        assert_eq!(Kernel::default().work_counters.face_pairs(), 0);
        w.reset();
        assert_eq!(counts(w), (0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0));
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
        let k2 = Kernel::new(&KernelOptions::default());
        let (mut r1, mut r2) = (k1.seeded_rng(), k2.seeded_rng());
        for _ in 0..1000 {
            let a = r1.next_f64();
            assert_eq!(a, r2.next_f64());
            assert!((0.0..1.0).contains(&a));
            let s = r1.next_signed_unit();
            assert_eq!(s, r2.next_signed_unit());
            assert!((-1.0..1.0).contains(&s));
        }
        k1.caches.clear();
    }

    /// Every generator a kernel hands out starts at its seed, however many were handed out
    /// and drawn from before; another seed gives another sequence.
    #[test]
    fn every_seeded_rng_starts_at_the_kernels_seed() {
        let k = Kernel::new(&KernelOptions {
            seed: 12_345,
            ..KernelOptions::default()
        });
        assert_eq!(k.seed, 12_345);
        let mut first = k.seeded_rng();
        let drawn: Vec<u64> = (0..5).map(|_| first.next_u64()).collect();
        let mut second = k.seeded_rng();
        assert_eq!(second, Rng::new(12_345));
        assert_eq!((0..5).map(|_| second.next_u64()).collect::<Vec<_>>(), drawn);
        assert_eq!(Kernel::default().seed, Rng::DEFAULT_SEED);
        assert_ne!(Kernel::default().seeded_rng().next_u64(), drawn[0]);
    }
}
