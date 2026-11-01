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

/// Emits the [`UpstreamFixes`] definition it wraps unchanged, plus
/// [`UpstreamFixes::NAMES`], [`UpstreamFixes::flags`] and [`UpstreamFixes::flag_mut`]
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
/// [`UpstreamFixes::flag_mut`] finds a field by its name. `upstream_fixes!` generates
/// them from this definition, so a new field needs no other edit to be listed.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
#[non_exhaustive]
pub struct UpstreamFixes {
    /// `SmPlane::IsNurbSurfacePlane` places a recognised surface's origin after
    /// orthogonalising its axes (WR-652, upstream issue #1056).
    pub plane_recognition_origin_after_axes: bool,
    /// `SmGapFunction::BuildSampleSet` samples the B-spline spans inside the requested
    /// curve interval or surface domain, normalizing against that interval and keeping
    /// its exact endpoints. Upstream starts at the first knot and normalizes against
    /// the whole knot range before mapping into the sample interval, so trimmed
    /// geometry can miss a gap peak (WR-89). Off by default for pinned C++ parity.
    pub gap_samples_use_trimmed_spans: bool,
    /// `SmGapFunction::BuildSampleSet` normalizes the interior V parameter samples of
    /// non-B-spline surfaces by the V sample index `(jj - cu) / (cv - 1)`, keeping them
    /// within `[0.0, 1.0]`. Upstream divides the raw array index `jj / (cv - 1)`, which
    /// starts at `cu + 1` and extrapolates interior V parameters well beyond `[0.0, 1.0]`
    /// (WR-88). Off by default for pinned C++ parity.
    pub gap_samples_non_bspline_v_stride: bool,
    /// `SmGapArray::Append` updates the longest-gap index (`m_lMaxGap3d`) when the
    /// appended array contains a strictly longer gap (or when appending into an empty
    /// array with a longest gap). Upstream never updates the longest gap on `Append`,
    /// so the combined array misses any longer gap from the appended array (WR-90).
    /// Off by default for pinned C++ parity.
    pub gap_array_append_updates_longest: bool,
    /// `SmTransform::EvaluateBoundingBoxSimple`'s aligned-basis fast path maps parameter
    /// intervals by multiplying by each axis column's length (`|x|`, `|y|`, `|z|`) and
    /// translating by the origin's component along each transformed basis vector
    /// (`o_u * |x|`, `o_v * |y|`, `o_w * |z|` where `(o_u, o_v, o_w) = T_lin^-1(o)`),
    /// permutes parameter axis `k`'s interval into basis slot `|alAxisMap[k]| - 1`
    /// (negated when `alAxisMap[k] < 0`), circumscribes an axis-aligned projected
    /// `PseudoBox` into `pOptProjBox` by basis (`WR-5`), and falls back to the general
    /// corner/unbounded branch when the transform's linear part is singular, instead of
    /// scaling by `1 / |axis|`, translating by `o · axis`, and indexing
    /// `sIvls[|alAxisMap[k]| - 1]` (WR-8, #2148; see
    /// `plan/upstream-issues/WR-8--addendum-2026-10-31-aligned-bbox-image.md`). Off by
    /// default for pinned C++ parity.
    pub transform_aligned_bbox_image: bool,
    /// `SmCurveLoops::GetLoopStartEnd` starts a later loop after the previous loop
    /// ends, keeping adjacency within that loop (WR-96). Off by default for C++ parity.
    pub curve_loop_start_after_previous_end: bool,
    /// `SmBSplineCurve::CreateSimpleOffset` passes `false` for `ApproximateCurve`'s
    /// `bOptCreateAnalytics` and forwards `bOptMatchParameterization` in its own slot
    /// instead of passing `(bOptMatchParameterization, FALSE)` (WR-37, #2141; see
    /// `plan/upstream-issues/WR-37--addendum-2026-10-31-simple-offset-match-param.md`).
    /// Off by default for pinned C++ parity.
    pub bspline_simple_offset_matches_parameterization: bool,
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
    /// `CalcRailIntEdgeuse`'s retry rail search holds a variable round's radius only where its first search does (at a corner calculated as constant radius), so the rail misses the arc it would meet only at that radius and the corner cuts the round off against the body; needs `fillet_nx1_end_rib_global_merge` there (WR-300, #1023). `RailRailIntersect` holds the radii at their end values only there too (its callers `CalcRailIntRail` and `CalcRailRailInterpolation`'s cubic rail interpolant): held at any other corner, two variable rounds meeting at a box's corner met where the laws are smaller and the trace from there failed with 1001 (the fork's tangency-labs/smlib@465b62b, the rule of 1d06159).
    pub fillet_retry_radius_as_first: bool,
    /// `TestRollOver` joins a rail's neighbouring intervals on its face or the face's boundary before the rollover, so a rail along its face's rim does not fail "Unable to Process Rollover" (WR-301, #485).
    pub fillet_rail_rim_on_face: bool,
    /// The fillet executive refuses a blend whose rails leave the faces they roll on (`RailsFitTheirFaces`, no room) (WR-302, #1024/#841).
    pub fillet_refuse_rails_leaving_faces: bool,
    /// `create_fillet_brep` refuses blends that take the whole of a face they roll on (`FacesOutlastTheirBlends`, no room) (WR-320, #879).
    pub fillet_refuse_face_consuming_blends: bool,
    /// `create_fillet_brep` refuses blends whose bands on a face they share overlap away from any corner that trims them (a rail of one inside the band between the other's filleted edge and its rail: both vertical edges of a box end rounded past half its width), with `FilletErrorType::NoRoom`, where upstream builds a body that is not closed (WR-542; the fork's tangency-labs/smlib@c0d74ee, `BandsClearOtherRails`). A rail is sampled only between its end vertices, where the corners have placed them: the rails are not trimmed yet and run on past their corners' mitres, so a run-on crossing another blend's band refused a valid round (WR-649, #1984; the fork's tangency-labs/smlib@d548305).
    pub fillet_refuse_overlapping_bands: bool,
    /// `DoFillet` refuses a filleted target that is not a closed manifold solid (an edge that is not `SmEdge::IsManifold`) with `SM_FILERR_BAD_MERGE`, where upstream returns success (WR-212, #422); the public wrappers (`fillet_edges`, `fillet_edges_per_edge`, `variable_radius_fillet`) then put the caller's body back as it was before the call.
    pub fillet_refuse_non_manifold_result: bool,
    /// The fillet executive refuses two rounds on edges of one face whose rails cross on it (no room between them), before the target is edited, where upstream builds an open body (WR-460, #1073). A closest-point query on an edge with no solution finds no crossing there and is not a failure (#2050).
    pub fillet_refuse_crossing_rails: bool,
    /// The vertex overload of `SmFilletErrorInfo::NoteError` records the vertex, its location, the fillet status, the error code and the comment, as the edge overload does, and `CreateFilletCorners` notes a vertex no corner type handles as `SM_FILERR_CORNER_UNSUPPORTED` with "Corner case not implemented" (WR-513, #1469; tangency-labs/smlib@a73cf5e). The pinned overload records nothing, so the failure reached the caller as the generic 1001 with no reason. Off by default (pinned behaviour). Diagnostics only: the pending vertex entry and the operation's result are unchanged, so this does not make an unsupported corner succeed. The whole-operation wrappers (e.g. `fillet_edges`) drop their session and return only the later failure; the retained note is readable through [`crate::blend::fillet_executive::session::FilletSession::error_info`]. The C API's `SMLIB_ERR_CORNER_UNSUPPORTED` (-1111) is not mapped here.
    pub fillet_vertex_error_diagnostics: bool,
    /// The fillet solver tries the corner ball's patch after the torus when a round's centre line collapses to a point (WR-420, #416).
    pub fillet_corner_ball_patch: bool,
    /// With `fillet_corner_ball_patch`, `CalcSphereFilletGeom` centres the corner ball where the two cylinders' axes meet (their nearest points, within the tolerance of each other), instead of requiring every trace point's centre within tolerance of the first: at the seam's end both offset cylinders collapse onto their axes and the last point's solve stops a step short of the crossing, so the ball was refused and the seam round failed (WR-541; the fork's tangency-labs/smlib@bc6ddf9).
    pub fillet_corner_ball_center_from_axes: bool,
    /// `SmFilletCorner::MakeVariableRadiusFillet` refuses to turn a four-sided corner into a variable-radius fillet when the new fillet's rail along the opposite fillet leaves that fillet's surface (more than ten times the corner's tolerance from its natural domain): the rail replaces the opposite edge, which bounds the opposite fillet's face, so that face would end off its own edge. The corner is then the patch on its exact edges. At a drafted pocket's top corners the rail ran 0.42 off the corner round at R3, 10 degrees (WR-645, #968).
    pub fillet_corner_variable_radius_rail_on_opposite_fillet: bool,
    /// `CalcTorusFilletGeom` takes the round's span as the turn from its start section to its end one, without taking `IsArc`'s start angle off it: an `SmCircle`'s start angle is measured from the circle's own frame (a boolean's circle framed on a cylinder's seam starts at 60 or 115 degrees), so the torus came out short by it and the end rib found no fillet surface at the far end (1001 at an Nx1 corner) (WR-568; the fork's tangency-labs/smlib@985faae).
    pub fillet_torus_span_from_start_section: bool,
    /// `CalcTorusFilletGeom` revolves a linear section into a cone (WR-421, #420).
    pub fillet_chamfer_cone_on_arc: bool,
    /// With `fillet_chamfer_cone_on_arc`, `CalcTorusFilletGeom` stores a linear section's revolution as the `SmCone` it is: the revolution's NURBS form is copied (`SmBSplineSurface::Copy`) and passed through `CopyAndAddAnalytics`, and when that gives a cone it is the fillet surface and the rails are made on it (as a straight edge's round is stored as its cylinder); anything else stays the surface of revolution. Without it a chamfer round a hole's rim is a surface of revolution, which reads back as a rational B-spline, not the cone it is (WR-572, #1503; the fork's tangency-labs/smlib@b5743ec).
    pub fillet_chamfer_cone_stored_as_cone: bool,
    /// `CalcTorusFilletGeom` also revolves a blend-curve (G1/G2/G3) section when the fillet's centre line is a point on the arc's axis (a blend as wide as the face it uses up, such as an R10 cylinder's rim at 10): upstream takes only circular sections there, so such a blend is marched along a centre line of no length and `TrimRails` fails (`SM_ERR`), leaving the body as it was; the revolved section is the blend curve `CreateSurfaceFromCurves` builds for each section of a marched blend, and every other blend is marched as upstream (WR-581, #919).
    pub fillet_blend_curve_revolved_on_arc: bool,
    /// `CreateCornerBlend`'s four-sided (not bilinear) corner patch is made tangent to the surfaces along its sides: a bicubic Coons patch meets its cross-boundary derivatives only where their twists agree at the corners, and where two rounds of different curvature meet `sm_CreateCoonsSrf`'s mean twist left both rounds at an angle along whole sides (15.6 degrees off a bar's round at r 2, rig fillets/1207). A copy is refined to 8 up to 64 spans each way and the control points next to each side are moved into that side's tangent planes until every side is within `SM_CONTINUITY_ANGLE` (WR-547; the fork's tangency-labs/smlib@cae8618). The patch's normal at each measured station is made unit by its own length, not by `Unitize`, whose absolute floor (1e-12) refused a small patch's normal near its collapsed side, so a variable-radius corner on a box of 0.01 failed (tangency-labs/smlib@701da4b).
    pub corner_blend_coons_patch_tangent_to_sides: bool,
    /// `SmFilletCorner::CreateCornerPatch` marks the patch sides that lie on a face of the body (a fillet edge with an original face) and `sm_CreateCoonsSrf` gives a corner between such a side and a blend's side the blend side's twist instead of the mean of the two: a bicubic Coons patch meets a side's cross-boundary derivative only where the corner twists are the other sides' own, so the mean tilted the patch off the face's tangent plane beside that side and it crossed the face (0.037 out of a bar's side face where the bar's top-edge round meets the concave round at a cylinder it runs tangent into, r 2). Corners between two face sides or two blend sides keep the mean (WR-613, #1836; the fork's tangency-labs/smlib@7fd9fb0 (commit not in fork history at 26cc451), from the issue's diff).
    pub corner_patch_face_side_twist: bool,
    /// A torus blend whose section centre is on the arc's axis turns rail points into the edge ends' half-planes (WR-422, #417).
    pub fillet_torus_center_on_axis: bool,
    /// `CalcTorusFilletGeom` moves a section's rail point that lies within the solver's tolerance (`GetThisApproxTol3d`) of the edge arc's axis onto the axis: it turns about the axis in a circle smaller than the tolerance, so the rail collapses to the axis's point and the surface of revolution is exactly singular there, where upstream left the point off the axis by the model's residues (1.2e-11 after a sketch's 1e-10) and the surface nearly singular, which `smlib_brep_check` refused (`SmSurface` #5, Bad Surf Pole Approximation; a round along an earlier round's end arc of the ball's own radius) (WR-592; the fork's tangency-labs/smlib@994f129). Whether the result then passes the check depends on the input's residues: on the bar `tests/wr592_torus_rail_on_axis.rs` rebuilds, the pinned SMLib with this hunk alone and the port both leave `SmSurfOfRevolution` #7 failing by roundoff, where the fork's own input passes (`plan/findings/blend.fillet_solver_p2--wr592-revolution-polar-start-angle.md`).
    pub fillet_torus_rail_on_axis: bool,
    /// The fillet solver decides whether a constant round is exact by one classification of its spine (the curve the ball's centre runs on) from its faces' kinds and placement, read as the natural quadrics they lie on (plane, sphere, circular cylinder or cone, an offset of one): a line (two planes; a plane or cylinder parallel to a cylinder's axis) is built as the cylinder, a circle about a common axis (coaxial quadrics, a plane square to the axis, a sphere's centre on it) as the section turned about that axis, a point (two cylinders as wide as the ball, axes meeting) as the ball's piece; every other round is traced. This replaces upstream's try-and-fall-through chain (`CanMakeAnalytic`, planes and cones only, then cylinder, torus and with `fillet_corner_ball_patch` the sphere, each deciding from the edge's shape) at `CalcFilletGeom` and at `ReCalcFilletGeom` (where a line round is rebuilt as its cylinder, not the cone), and a builder's failure is then an error, not a trace. So a round on a sphere is exact, and a circular round's frame comes from the faces' axis, not the edge's `IsArc` (WR-669; the fork's tangency-labs/smlib@b5f2867 and 7ac037e). The torus and ball builders' own flags still apply inside them: the fork's behaviour is this with `fillet_torus_span_from_start_section`, `fillet_chamfer_cone_on_arc`, `fillet_chamfer_cone_stored_as_cone`, `fillet_torus_center_on_axis`, `fillet_torus_rail_on_axis` and `fillet_corner_ball_center_from_axes` on (a chamfer round a circle is traced without `fillet_chamfer_cone_on_arc`). A conic spine (a plane with a cylinder or cone at any other angle; two cylinders or cones touching a common ball the same way round) is built as the exact rational surface of the ball's arcs, checked against the ball before it is kept (the fork's tangency-labs/smlib@0061c41 and 1b5f1e7); the fork's nodal-quartic rounds are classified but still traced here.
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
    /// At a set-back corner whose rounds ease (`SmFilletCorner::m_dEaseDist`, the fork's `e74bbd3`), `SmFilletCorner::MakeFaceBrep` bounds each face of the N-sided patch on one of the corner's face curves (`SM_FE_RAIL_RAIL_INTERPOLATION`) by that curve's own half, the curve trimmed to half its interval (one quintic span, curvature-continuous with its contact line and with the other half), instead of the patch surface's iso-curve, which `N_FillNSidedHole` fits to the half only to tolerance in its surfaces' degree and knots (a quintic face curve's half came back degree 6 over 7 spans) (WR-659; the fork's tangency-labs/smlib@4702889). Corners without an ease are unchanged; not reachable in the pinned C++, which has no ease.
    pub fillet_corner_face_curve_halves: bool,
    /// The extended offset (`SmOffsetExecutive::DoExtendedOffset`, which shells and offsets with extended convex edges) caps each vertex of the original with one edge (a cone's apex, the tip of a drilled hole) with the sphere `BuildOffsetOfVertex` builds for it, a face of the whole sphere zone the merge joins to the cone's offset; upstream builds no vertex offsets there, so the cone's offset ends open on the circle the apex's normals sweep out and the shell fails (WR-531; the fork's tangency-labs/smlib@d5dadaa).
    pub offset_point_vertex_caps: bool,
    /// `BuildOffsetOfFaceuse` clips a face to its offset domain from its counterclockwise loops (an inset is not turned inside out) (WR-342, #974).
    pub offset_clip_right_side_out: bool,
    /// `SmOffsetExecutive::MapEdgeuses` raises each offset edge it matches to an original edge to that edge's tolerance, and the offset edge's vertices to the original edge's vertices' largest. Upstream builds the offset faces' edges and vertices afresh at the B-rep's zone tolerance, so the offset of a sheet whose edges and vertices carry their own, larger tolerances (a trimmed B-spline patch: B-rep 2.08e-5, edges 4.15e-5, vertices 8.31e-5) is checked against half the B-rep's tolerance at its vertices, and its offset by 0.75 fails `check_brep` with edge/vertex Bad Gaps of 2.4e-5 to 3.9e-5, inside the original's own tolerances (WR-671, #2111).
    pub offset_edges_keep_source_tolerance: bool,
    /// `SmSurfOfRevolution::CreateOffsetSurface` cuts the offset generator at the axis on either side of the frame origin (WR-343, #973).
    pub revolution_offset_axis_both_sides: bool,
    /// With extended convex edges, `piecewise_merge` relates the mated offsets of a manifold edge instead of intersecting them again (WR-344, #469; the fork's tangency-labs/smlib@9c21c9a (was 85bbfaf)), except an edge whose mate failed in `EdgeuseOffset`, which is intersected even where it is a tangent sector (#1795; the fork's tangency-labs/smlib@9d8ace9). A shell of a solid whose result is then no closed manifold solid is refused (`SM_ERR`) instead of returned: the merge no longer refuses some broken offsets, and the difference hands back a wall that bounds nothing (#1818; the fork's `ShellBrep` returns it, its C API refuses it as `SMLIB_ERR_NOT_SOLID`). Needs no other flag.
    pub offset_extended_edges_mated: bool,
    /// The extended offset (`SmOffsetExecutive::GetCreateExtendedSurface`) extends a cone or cylinder as itself, along its rulings and around its axis with `SM_CT_CINFINITY` (around by no more than half the turn it leaves open on each side), where upstream extends every surface by a tangent extension (`SM_CT_G1R`) that leaves a cylinder at once; a face that reaches a hair into that extension is later fitted back into a cylinder off its radius (WR-532; the fork's tangency-labs/smlib@b283ea2).
    pub offset_extends_cones_as_themselves: bool,
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
    /// A volumetric Boolean removes the zero-area loops a contact curve inside a face leaves, and splits shells joined only through them (WR-360, #746).
    pub remove_slit_loops_where_solids_touch: bool,
    /// Regularize non-merged, in-place whole-body translational sweeps (#1937):
    /// squeeze newly swept faces degenerate at one quarter of the body tolerance,
    /// and remove isolated open-edge inner slit loops. Existing Euler operations
    /// preserve the remaining boundaries; mappings to deleted results become null.
    /// Disabled by default to retain the pinned native result.
    pub translation_sweep_regularize: bool,
    /// A face a Boolean takes over from its second operand keeps that operand's exact UV trims (WR-361, #488).
    pub keep_operand_uv_trims: bool,
    /// `smgu_EvaluateCurveContinuity`'s G2 test compares the two sides' Frenet curvatures and normals (WR-380, #933/#838).
    pub geometric_g2_continuity: bool,
    /// `smgu_CircleFindSilhouettePoints` in perspective rotates the unit direction from the
    /// circle centre toward the projected eye point (`proj_eye - center`) as a pure vector
    /// about the circle normal by `±acos(radius / dist)` and offsets the scaled radius
    /// vectors by `circle_center`, returning `None` when the circle normal is degenerate.
    /// Pinned v1.0.1 negates `to_eye` (`center - proj_eye`), rotates a frame at
    /// `circle_center` about the global origin, transforms `to_eye` as a point and scales
    /// by `radius` without adding `circle_center` (WR-11, #2138;
    /// `plan/upstream-issues/WR-11--addendum-2026-10-31-perspective-silhouette-center.md`).
    /// Off by default for pinned C++ parity.
    pub circle_silhouette_perspective_center: bool,
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
    /// `CreateOrientedTrimBoundary` takes a UV gap of whole periods (up to 1% of one) in a closed direction between consecutive curves for a seam crossing and moves the curves after it by the period, instead of filling it with a line across the whole domain that doubles the loop back on itself (WR-491). A loop that does not close that way keeps the line, and so does a gap along a singular side of the surface (a sphere's pole, a cone's apex; #1437).
    pub trim_boundary_seam_wrap: bool,
    /// `SmBSplineCurve::AssertValid` reads the minimum radius of curvature before tests 13/14 compare it (WR-382, #475).
    pub bspline_check_min_radius: bool,
    /// `SmEllipse::AdjustSTEPInterval` rejects inverted STEP edits and collapsed or non-finite converted NURBS intervals before mutation, and revolution edits install their domain only after their generator accepts it (WR-211, #428).
    pub ellipse_step_interval_checks: bool,
    /// `SmEllipse::ConvertTo2D` solves the extremal projected-radius quadratic with
    /// coefficients in `SolveQuadraticEqn`'s ascending `[C, B, A]` order, evaluates the
    /// second semi-axis at the orthogonal angle `dThetaYRad` (`theta_x_rad + PI / 2`
    /// when linear) instead of reusing `dThetaXRad`, and aligns a full-turn STEP domain
    /// with the projected NURB seam before rebuilding the polar converter (WR-31,
    /// #2140; `plan/upstream-issues/WR-31--addendum-2026-10-31-convert-to-2d-second-axis.md`).
    /// Off by default for pinned C++ parity.
    pub ellipse_convert_to_2d_uses_both_axes: bool,
    /// `SmCircle::CreateSimpleOffset` distinguishes antiparallel offset plane normals
    /// from parallel ones, subtracting offset distance rather than adding it (WR-32,
    /// 2026-10-31 addendum). Off by default for pinned C++ parity.
    pub circle_offset_antiparallel_normal: bool,
    /// `SmBSplineCurve::AssertValid` measures a UV trim on its face's surface: the radius of curvature test (13, live with `bspline_check_min_radius`) and the coincident interior control point test (15) run on the trim mapped by the surface's first fundamental form at its middle (the Cholesky factor: exact on a plane), with the mapped trim's tolerance and spacing for test 13 (WR-560, #1524; tangency-labs/smlib b4f8c57, 54ca3e0).
    pub uv_trim_checks_on_surface: bool,
    /// `SmBSplineCurve::AssertValid` takes a zero-speed tangent end of an edge's curve or UV trim (its end control points coincide, its second derivative does not vanish, the end is a vertex of the edge, and the edge's faces have parallel normals there within `SM_CONTINUITY_ANGLE`): test 3 accepts the coincident first control points, and tests 13/14 (live with `bspline_check_min_radius`) skip a radius below the tolerance within the stretch of the end where its growth `2 |C''|^3 / |C'' x C'''| |t - t_end|` puts it below. A G2 blend's end on a face tangent to its own leaves the vertex as a B-spline in `t ~ sqrt(s)`, with zero speed there (WR-668; tangency-labs/smlib ecf1963).
    pub curve_check_zero_speed_tangent_end: bool,
    /// `SmFace::ComputeLoopOrientation`'s 3D path holds a loop's dimensionless area to a floor relative to the loop's own bounding box instead of the face's whole domain, so a loop far smaller than its face (a 0.002 hole in a 3 m plate) is oriented (WR-561, #1523; tangency-labs/smlib 54ca3e0).
    pub loop_orientation_floor_from_loop_box: bool,
    /// The local solves take no value from a failed evaluation (WR-253; the fork's tangency-labs/smlib@d2757e1). The one-dimensional ones ignore a failed guess in `SolveOscillating`'s sign-change search (#481), and `SmLocalSolve1d::SolveIt` without an interval returns a failed first evaluation's status instead of iterating from it (no caller solves without an interval). The N-dimensional one (`SmLocalSolveNd::SolveIt`) sums no equations of a failed clamped or scaled evaluation, takes the clamped-and-scaled step only when it evaluated, and then with that evaluation's Jacobian and found flag (upstream kept the unscaled point's), stops the line search when the clamped step failed and no scaled one was taken, and finds no solution when the best guess cannot be re-evaluated (#1945), nor when the guess's first evaluation fails, where upstream reports the unevaluated guess as found (WR-20; the fork's tangency-labs/smlib@d5c5cfd).
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
    /// `SmSurface::DropCurve` tests the drop domain's own V length for `bDegenSurfV`, not the U length again, so a surface whose natural domain is degenerate only in V reports `SM_ERR_DEGENERATE_SURFACE` (`SM_CTC_DEGENSURFACE_FAIL`) and its failure record sets `bDegenSurfV` from V; upstream copies the U test, misses the early exit and goes on to drop (WR-100, 2026-10-31 addendum). Off by default for pinned C++ parity.
    pub drop_curve_v_domain_degeneracy: bool,
    /// `SmTorus::AssertValid` tests its V (minor) interval, not the U interval twice, and reports its U and V overlap tests as 3 and 4 (and `SmSurfOfRevolution`'s U test as 13), the ids their labels give them, where upstream reports them as 2 and 3 (and 2) (WR-538, #482; the fork's tangency-labs/smlib@0cd9d08).
    pub torus_check_reads_v_interval: bool,
    /// `SmTorus::AssertValid` reports a spindle torus whose V interval runs through an apex (`V = ±(90 + asin(R/r))`) as test 5, "Self Intersection", where upstream clears its result with no report and tests the U interval against `90 + acos(R/r)`, failing every spindle torus with a whole turn of U (WR-539; the fork's tangency-labs/smlib@daba678).
    pub torus_check_spindle_apex: bool,
    /// Volume line-domain checks using `SmExtent3d::ContainsLineSeg3d` require
    /// both endpoints inside the natural parameter box instead of either (WR-3).
    pub extent3d_contains_both_segment_endpoints: bool,
    /// `SmPeriodicExtent1d::AddInterval` on an empty extent sets `[from, to]` rather
    /// than assuming the full period `[0, period]` (WR-6, 2026-10-31 addendum).
    /// Off by default for pinned C++ parity.
    pub periodic_extent_add_empty_interval: bool,
    /// `SmExtent3d::Circumscribe` reads `SmPseudoBox::IsAxisAligned`'s map by basis:
    /// basis `ii`'s interval lands on axis `|map[ii]| - 1`, negated when the basis is
    /// reversed, so a basis that is a non-involutive permutation of the axes (e.g.
    /// `y, z, x`) keeps its intervals on their own axes. Upstream indexes the basis
    /// with the axis map entry of axis `ii`, which is right for swaps and sign flips
    /// but cycles a 3-cycle (WR-5). Off by default for pinned C++ parity.
    pub circumscribe_axis_map_by_basis: bool,
    /// Perspective culling projects each sphere's own radius offsets about its center (WR-608, #525).
    pub perspective_sphere_offsets: bool,
    /// `SmCPolyFace::RemovePolyFace` checks whether the face is a member of the
    /// composite before removing it, so removing a non-member face does nothing
    /// instead of removing the first member (WR-81). Off by default for pinned
    /// C++ parity.
    pub cpoly_remove_poly_face_check_member: bool,
    /// `SmTrackTopologyChanges::StopTracking` clears the context's system notify
    /// callback and records post-edit dependencies only when the context's currently
    /// installed callback is this tracker's own (`IsTracking`), instead of whenever
    /// `m_pLastNotifyCallback == NULL` — which also fires when a tracker was
    /// constructed with `bStartNow = FALSE` and never started, or when another caller
    /// replaced this tracker's callback before `StopTracking` ran (WR-17, #2149; see
    /// `plan/upstream-issues/WR-17--addendum-2026-10-31-stop-tracking-own-callback.md`).
    /// Off by default for pinned C++ parity.
    pub stop_tracking_checks_own_callback: bool,
    /// `SmCone::RebuildSTEPFromNURBParameters` gives a cylinder the mean of near-equal radii (semi-angle 0) where upstream's own copy within `SM_EFF_ZERO` does not apply, as the fork's 5df5b27 (WR-385, #713).
    pub cylinder_rebuild_mean_radius: bool,
    /// `SmCylinder::IsNurbSurfaceCylinder` gives an inside-out NURB (its iso circles turning against the line through their centres) the frame `IsNurbSurfaceCone` gives one: Z against the NURB's linear direction, origin at its top-knot circle; upstream keeps the bottom-knot circle and the NURB's direction, so the cylinder's Z is reversed and its STEP evaluation is mirrored in height (WR-496, #1463).
    pub nurb_cylinder_inside_out_frame: bool,
    /// `SmPlane::Reparameterize` moves the placement origin to `sOriginMax` when the UV domain
    /// is reparameterized, maintaining consistency between analytic plane and NURB evaluations
    /// across shifted domains; upstream computes `sOriginMax` but only references it under
    /// `SM_DEBUG_CODE` (WR-41). Off by default for pinned C++ parity.
    pub plane_reparameterize_moves_origin: bool,
    /// `SweepProfilesAlongPath` asks for caps only when the profile closes (WR-400, #837).
    pub planar_path_uncapped_open_profiles: bool,
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
    /// `PositionProfileCurves` places profiles in the planar path's own XY frame (one
    /// frame for all its pieces), then maps them back, instead of using global X and Z
    /// to orient them (WR-660). This keeps the profile's roll and first-point anchor
    /// under rigid motions, including vertical planes and reversed plane normals.
    /// Paths in or parallel to XY with either normal retain upstream arithmetic;
    /// straight paths retain upstream placement. A path of several pieces also needs
    /// `sweep_frame_on_planar_path_normal`: without it the sweep's own pose dependence can
    /// still cut an arc-then-line path to volume 0. Off by default for C++ parity.
    pub planar_path_profile_in_path_frame: bool,
    /// A rotational sweep (whole or partial turn) makes each line and circular arc of its profile its cylinder, cone, sphere or torus, not only a one-curve profile's (WR-448, #545; SolidCraft change, tangency-labs/smlib 0b143f7 (commit not in fork history at 26cc451)).
    pub revolve_analytic_profile_faces: bool,
    /// With `revolve_analytic_profile_faces`, a revolved arc whose centre is within the B-rep tolerance of the axis is the sphere centred on the axis through the arc's ends, and an end within it of the axis is its pole, not a sphere (or torus) built on the arc's own off-axis circle with a pole row of radius about sqrt(2 tol R) (WR-471, #1123; tangency-labs/smlib, "a revolved arc centred within tolerance of the axis is a sphere through its ends").
    pub revolve_arc_centre_near_axis_is_sphere: bool,
    /// With `revolve_analytic_profile_faces`, a revolved arc whose circle reaches the axis to within the sweep's tolerance (its centre's distance from the axis within that tolerance of its radius) sweeps the horn torus, its major radius snapped to its minor one, as the sweep takes the arc's end there to be on the axis; swept as it lay, a hair inside or outside, the spindle or ring torus's minor circles cross or miss the axis where the topology collapses and the face's loop there comes out turned over. The snapped torus is kept only when it passes the analytic sweep's exact layout test (else the B-spline surface of revolution stays) (WR-552; the fork's tangency-labs/smlib@c4e378d).
    pub revolve_arc_touching_axis_is_horn_torus: bool,
    /// A taper extrude's corner whose links meet at both ends pivots on the joint nearest its offset curve (WR-402, #479).
    pub taper_pivot_nearest_corner: bool,
    /// A taper extrude with linear-extension (tangent) corners of a profile of lines and circular arcs with a sharp corner continues each wall as its tangent plane to meet its neighbour along their exact seam (a line's wall is its own plane, so two lines meet at a corner edge), and refuses a corner it cannot close (`create_taper_extrude_with_corner` reports why and where), instead of ruling separate corner faces from offset extension lines; with fillet corners, or a profile with another curve kind, nothing changes (WR-521, #1449; SolidCraft change, tangency-labs/smlib e8f23e0).
    pub taper_tangent_corner_planes: bool,
    /// The curvature-keeping and tangent corner rules of `create_taper_extrude` (`sm_CreateCurvatureKeepingWalls`) follow the section up the height: a wall whose section shrinks to nothing below the far end ends at that apex and its neighbours meet from there up (two lines brought face to face close together along their common line, and the walls beyond them meet from its other end), where the fork's first version refused the draft (`SM_TCR_WALL_CLOSES`) though the solid exists. A wall closing at the far end, the whole section closing, and an arc shrinking to its centre are still refused (WR-571; the fork's tangency-labs/smlib@183ea65).
    pub taper_wall_closes_at_apex: bool,
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
    /// A curve sweep caps from the side faces at the sweep's start and end (WR-404, #411).
    pub curve_sweep_caps_at_sweep_ends: bool,
    /// `SmPolarBox::AddVector3d` (`PolarBoxState::Region`), when the x span collapses
    /// below `SM_EFF_ZERO_SQRT` and it rotates `(basis[1], basis[2])` to `(basis[2],
    /// -basis[1])` and relabels the box as `SM_PS_ARC`, rotates the polar domain to
    /// `[lo.y, hi.y] × [-hi.x, -lo.x]` so the arc's angular span lies on the x axis
    /// that `SM_PS_ARC` operations read and every previously added vector's polar
    /// coordinates `(y, -x)` in the rotated basis stay inside the domain (WR-9, #2133;
    /// see `plan/upstream-issues/WR-9--addendum-2026-10-31-region-to-arc-domain.md`).
    /// Upstream calls `SetMinMax(UMin, VMin, UMax, VMax)`, leaving the arc's span in y
    /// and a collapsed x span. Off by default for pinned C++ parity.
    pub polar_box_region_to_arc_transposes_domain: bool,
    /// `CreateLinearSweep` makes a circular arc short of a whole turn that is swept square to its plane an `SmCylinder` bounded 0..its sweep angle by 0..the distance, its frame turned to the arc's start (for a sweep against the arc's normal, to its end and then flipped), where upstream makes a cylinder only of a whole circle and an `SmSurfOfExtrusion` of any other arc; a sweep that is not square stays a surface of extrusion (WR-565, #1477; SolidCraft change, tangency-labs/smlib 12918e9).
    pub linear_sweep_arc_is_cylinder: bool,
    /// Planar faces from curves join a loop touching another loop at a vertex into that loop (WR-405, #892).
    pub join_touching_loops: bool,
    /// The pointer check judges a UV trim's end turn on its face's surface (first fundamental form), not in raw (u, v) (WR-406, #891).
    pub uv_trim_reversal_on_surface: bool,
    /// `sm_CheckEdgeuseGeometry` evaluates the clockwise neighbour in UV space with
    /// `SmEdgeuse::NormalizedEvaluate(.., TRUE, ..)`, whose `GetOrCreateUVTrimCurve`
    /// stores a new UV trim curve on that neighbour: a `const` validation
    /// (`SmBrep::ValidatePointers`, e.g. from `SmBrepConstructor::EndShell(TRUE)`) then
    /// changes the B-rep it is asked to check (WR-446;
    /// `plan/upstream-issues/WR-446--brepconstructor-endshell-validatepointers-makes-uv-trim-curves.md`).
    /// With this set the neighbour is evaluated with `ok_to_make_uv_trim_curve = FALSE`:
    /// the 3D point is still evaluated and dropped onto the existing face, but no trim
    /// is made; the self evaluation, the existing trims and the statuses are unchanged.
    /// Off by default for pinned C++ parity. This is not the WR-406 trim-reversal fix
    /// (`uv_trim_reversal_on_surface`): validation still repairs existing trims as before.
    pub validation_does_not_make_uv_trims: bool,
    /// Before the piecewise merge, `do_extended_offset` rebuilds every offset face whose loops cross from the stretches of its loops that bound it (WR-345, #854).
    pub offset_resolve_crossing_loops: bool,
    /// `SmPointSet3d::SetUpCentroid` sums every full 50-point block when the point count
    /// is a positive multiple of 50 (`extras == 0`) instead of starting with a 0-point
    /// block that drops the last 50 points (returning `(0, 0, 0)` for 50 points), and
    /// marks `Cache::CENTROID` (`m_bCentroidSet = TRUE`) after computing it so repeated
    /// centroid queries reuse the cached value and do not depend on whether `GetNormal`
    /// ran first (WR-15, #2134; see
    /// `plan/upstream-issues/WR-15--addendum-2026-10-31-centroid-multiples-of-50.md`).
    /// Off by default for pinned C++ parity.
    pub point_set_centroid_all_blocks: bool,
    /// `SmLocalSolve1d::SolveIt` clamps the Newton step `delta_t` to
    /// `0.08 * m_vInterval.GetLength() + SM_EFF_ZERO` (scaled by `0.99`) only when
    /// `m_bUseInterval` is `true`, instead of evaluating `m_vInterval.GetLength()` on the
    /// uninitialised `[1e20, -1e20]` interval (`length = -2e20`, `step_limit = -1.6e19`)
    /// when solving without an interval — which made `|delta_t| > step_limit` always true
    /// and replaced every step with `±1.584e19` in the opposite direction of Newton
    /// (WR-19, #2150; see
    /// `plan/upstream-issues/WR-19--addendum-2026-10-31-unbounded-newton-step.md`). Off by
    /// default for pinned C++ parity.
    pub local_solve1d_unbounded_newton_step: bool,
    /// `SmBrep::LocalOperation` sizes the offset executive's distance (by which it extends the neighbouring surfaces to meet the moved ones) from a box that also holds the replacement surfaces, not from the original body alone, so a face moved further than about the body's size away is still reached (WR-534; the fork's tangency-labs/smlib@a1ee273).
    pub local_operation_box_holds_new_surfaces: bool,
    /// With `offset_resolve_crossing_loops` (it has no effect without it), loops of an offset face that touch without crossing (run along one another, meet tangentially, or come up to one another and go back on the same side) are refused, before any crossing is resolved, with `SM_ERR_OFFSET_WALLS_TOUCH` (1042) instead of `SM_ERR_BAD_INTERSECTIONS` (1027): the offset walls meet along a line (WR-345 addendum, #1442; tangency-labs/smlib a759622).
    pub offset_walls_touch_refused: bool,
    /// An inset's `do_extended_offset` makes manifold only the finite regions of the merged offset that are cavity, not every finite region (upstream's own TODO there: "we will need to select regions to keep"): a region is cavity where one of its faces has the body's face the wall's thickness behind it and none has the body's face that far ahead. Where a floor is no thicker than the wall its two faces' offsets cross and the merge closes the region between them; kept, the cavity was no manifold solid and the shell failed with `SM_ERR` (1001) (WR-667; the fork's tangency-labs/smlib@37af643).
    pub offset_keep_cavity_regions: bool,
    /// `SmBrep::LocalOperation` returns the failed extended offset's own status (e.g. `SM_ERR_OFFSET_WALLS_TOUCH`, 1042, or `SM_ERR_BAD_INTERSECTIONS`, 1027) instead of a generic `SM_ERR` (WR-345 addendum, #1442; tangency-labs/smlib a759622).
    pub local_operation_offset_status: bool,
    /// In a local operation (`SmBrep::LocalOperation`), `SmOffsetGeometryCreation::ExtendTrimEdges` fails a tangent or lamina edge whose lift onto one face's new surface drops onto the other's farther than ten times `max(the offset edge's tolerance, the approximation tolerance)`, and `EdgeuseOffset` passes that failure on, so the operation is refused; upstream ignores it ("will be fixed later", which no later step of a local operation does) and returns a body with an edge as loose as the gap between a moved face and its unmoved tangent neighbour (WR-530, #402; the fork's tangency-labs/smlib@23c5b0a). Offsets and shells are unchanged.
    pub local_operation_refuses_loose_tangent_edges: bool,
    /// A local operation that moves one of two tangent faces along its normal builds the step: `EdgeuseOffset` finds the two faces parallel but apart across the edge (`IsStepAcrossEdge`) and builds the ruled wall between their edges (`CreateStepWall`, rails of uniform weight made non-rational), kept out of the face extensions; `CloseLoopGaps` closes the gaps the wall leaves at its ends and the edges running past its corners are split; a vertex moved past an edge's other end is `SM_ERR_OFFSET_EDGE_VANISHES` (1043) (WR-530 addendum; tangency-labs/smlib 5d428ae, 90aabff, 34318f4).
    pub local_operation_step_walls: bool,
    /// Every extended offset closes the loop gaps it leaves where two consecutive edges' offsets run parallel and never meet (`CloseLoopGaps`), before the merge; a vertex moved past an edge's other end is `SM_ERR_OFFSET_EDGE_VANISHES` (1043), and a shell whose offset of a solid is still no closed solid is refused (`SM_ERR`) instead of returned (WR-578; tangency-labs/smlib 7ff0a94, 34318f4, rig case shell/1494). Limit: tangency-labs/solidcore#1528's plate shells on a clean operand but is refused after a prior mass-properties query on it, which writes UV trims and gap caches the offset reads (upstream behaviour, WR-583).
    pub offset_closes_loop_gaps: bool,
    /// With `offset_closes_loop_gaps` (it has no effect without it): a convex corner of four plane faces whose offset planes no longer meet in one point (two chamfers meeting at a box's corner) becomes the edge between the two faces across it that the offset makes meet, where upstream ends the corner's edges at the same pair of the planes' meeting points whichever way the corner splits, and so leaves an inset, or a shell open at one of the four faces, with loop gaps and no volume (or the merge refuses it, 1001). A corner one of whose edges would vanish is left as it is. An offset (`offset_brep`) or a shell of a manifold solid that is no closed manifold solid is refused (`SM_ERR`) instead of returned (WR-657).
    pub offset_splits_corner_vertices: bool,
    /// With `local_operation_step_walls`: an edge whose vertex a local operation moves onto its other end, where its other vertex is, has closed to a point and is squeezed out of its offset face (`CloseLoopGap`); once every gap is closed the offset faces that closed to nothing (`SmFace::IsDegenerate`, a step's wall whose low top moved onto the high top's plane) are deleted and forgotten, and the merge joins the faces either side. A closed edge on no such face or across from one is still `SM_ERR_OFFSET_EDGE_VANISHES` (WR-566; tangency-labs/smlib ccbf77a). In the face move (`offset::offset_faces`), the edges then left between a moved face and a face that stayed, on one surface, which `IsTopological` finds removable, are removed, so the tops either side of the closed wall are one face (WR-566 addendum; tangency-labs/smlib 9504880).
    pub local_operation_closes_walls_to_nothing: bool,
    /// An inward shell mates the walls across rounds thinner than it (collapsed faces) (#397).
    pub offset_collapsed_rounds_mated: bool,
    /// `FaceuseOffset` fails on a face whose offset surface is missing without collapsing (`SmSurface::OffsetCollapses`: a cone, cylinder, sphere or torus offset onto its spine), where upstream reads any missing surface as a collapsed round (WR-464, #1072).
    pub offset_missing_surface_fails: bool,
    /// `ApproximateSurface` raises the starter's inner knots to full multiplicity (exact knot insertion) before each round of cubic Bezier patches, which needs every inner knot of full multiplicity; and a patch that runs past the control points is `SM_ERR`. Upstream uses a starter copied from a root B-spline with simple inner knots (the offset of a fitted bicubic patch) as it is, its patches run past its control points, and it returns success with no surface, so exporting an offset B-spline sheet as B-splines fails (1001) (WR-573, #1501; the fork's tangency-labs/smlib@eae96cb).
    pub approximate_surface_bezier_starter: bool,
    /// A surface of revolution whose generatrix is one circular arc (a fillet's round along a circular edge, a torus in all but its type) collapses when an offset runs towards the arc's centre by its radius or more: `Surface::offset_collapses` says so, as a torus's does, and `create_offset_surface` builds no surface, where upstream says it never collapses and builds the generatrix's offset past the arc's centre as the arc turned inside out, so an inward shell thicker than the round fails with 1001 or leaves a wrong volume (WR-567; the fork's tangency-labs/smlib@8b2090f, smlib#64; SolidCraft `shell_of_a_disc_rounded_smaller_than_the_wall`, solidcraft#319). The faces either side are mated across it with `offset_collapsed_rounds_mated`.
    pub revolution_round_offset_collapses: bool,
    /// At an Nx1 corner whose end rib cannot be built because a rail missed a side edge ending within the fillet's width, the rib is deleted and the global merge cuts the round off (WR-303, #841).
    pub fillet_nx1_end_rib_global_merge: bool,
    /// `SmCompositeCurve::CalculateBoundingBox` stacks each segment's composite span end
    /// to end starting at the first segment's `GetNaturalInterval().GetMin()` (matching
    /// [`CompositeCurve::natural_interval`](crate::curve::composite_curve::CompositeCurve::natural_interval)
    /// and `MakeCompositeNurb` / `CreateByJoining`) instead of translating each segment's
    /// natural interval from `0`, and maps each non-empty overlap with the requested
    /// composite `interval` back onto the segment's natural interval (reversing within
    /// `[min, max]` when `same_sense` is `false`) before querying the segment's boxes
    /// (WR-34, #2135; `plan/upstream-issues/WR-34--addendum-2026-10-31-bbox-segment-interval.md`).
    /// Segment box evaluation and unioning (including `PolarBox` tangent sense on
    /// reversed segments) are otherwise unchanged. Off by default for pinned SMLib
    /// compatibility.
    pub composite_curve_bbox_segment_intervals: bool,
    /// `sm_FixUVTrimCurves` moves only the end control points of the two UV trims meeting at a corner onto the vertex's UV point, without dropping the vertex onto them, trimming there and stretching them back over the edge interval (WR-255, #959/#570; the fork's tangency-labs/smlib@e5606fb (was 5af7070)).
    pub uv_trim_corner_end_points_only: bool,
    /// `SmEdgeuse::CreateUVTrimCurve` takes a drop that came back in pieces as the longest run of pieces that meet end to end (the same edge parameter and `(u, v)`), joined exactly on the edge parameters it was dropped from, where upstream reparameterizes a piece covering 90% of the edge onto the whole edge interval, which runs the trim at the wrong pace (up to 5 mm off its edge) (WR-543, #1383; the fork's tangency-labs/smlib@e36feb1).
    pub uv_trim_joins_drop_pieces: bool,
    /// `BSplineCurve::trim` removes short end spans only for model-space curves
    /// (dimension 3), preserving exact UV trims in surface parameter units
    /// (WR-441, #1114). Off by default for pinned SMLib compatibility.
    pub bspline_trim_preserves_uv_spans: bool,
    /// `BSplineCurve::trim` keeps a short end span of a rational curve (its control points carry weights, `N_IsCrvRat`: a conic's NURB) instead of removing it with `N_CrvRemoveDegenSegs`, which stretches the remaining spans over its length and so moves a conic off its circle or ellipse (a trimmed half circle of radius 16 by about 1e-6, enough for its edge and vertex gaps to pass the tolerance); non-rational curves are cleaned as upstream (WR-575, #1529). An end span whose control points all lie within check test #15's pole tolerance (`SM_ZONE_TOL_3D / 10`) is not kept: the trim ends at its interior knot instead, on the conic and within that tolerance of the requested end, so the curve passes the check (#2090). Off by default for pinned SMLib compatibility.
    pub bspline_trim_keeps_rational_end_spans: bool,
    /// `SmCone::IntersectWithCone` tests the line of two parallel cylinders against both face domains at its middle, not its bottom end (WR-259, #711).
    pub cone_parallel_line_midpoint: bool,
    /// `SmCone::IntersectWithCone` clips parallel-cylinder lines to both requested face domains, including overlaps away from the full line's middle (WR-259, #711). Independent of `cone_parallel_line_midpoint`; off by default for pinned SMLib compatibility.
    pub cone_parallel_line_domains: bool,
    /// `SmCone::IntersectWithPlane` takes a plane parallel to a cylinder's axis as tangent also when it lies within the zone tolerance of a bounded face's wall along a line (`|d| > R - zone_tol`, the larger of the cone's and the plane's face tolerances, 0 for a surface with no face), besides upstream's two-crossing-lines test, so a plane turned a hair off a face's wall it touches does not cut it in two lines microns apart; bare surfaces are unchanged (WR-510, #1525; tangency-labs/smlib#55, merged as b22bfbc (was bd20f6e)).
    pub cylinder_plane_tangent_within_tolerance: bool,
    /// The Boolean's topology intersector treats a self-loop proto edge whose ends a vertex definition on a vertex combined (`CanCombineVertexDefinitions`) as degenerate, instead of `SmProtoEdge::Resolve` splitting it onto that one vertex, after which `SmMerge::MakeFaceuse` returns `SM_ERR` (WR-214, #480; `plan/upstream-issues/WR-214--smprotoedge-resolve-splits-a-self-loop-proto-edge-onto-one.md`).
    pub proto_edge_self_loop_on_vertex_degenerate: bool,
    /// `SM_BO_SLICE` keeps a coincident pair whose primary face bounds material on one side only when that material lies on the side the sheet's `SM_OT_SAME` faceuse bounds (step 7.4d's rule), instead of falling through to the difference rule, which keeps a face lying in the slicing sheet in both parts (WR-254, #528; `plan/upstream-issues/WR-254--manifoldboolean-keeps-a-face-lying-in-the-slicing-sheet-in.md`).
    pub slice_coincident_faces_sheet_side: bool,
    /// After step 8, a Union, Difference or Intersection whose result is a manifold solid with more than two regions (the infinite region first) runs `SetRegionIsVoidFlagsForNestedSolids`, which puts each region's enclosing shell first (the fork's tangency-labs/smlib `abastran/void-shell-check`). It also rewrites the void flags by nesting from the infinite region, so where step 7 left flags that disagree with the nesting the result's void flags change too, not only its shell order (WR-265, #388; `plan/upstream-issues/WR-265--manifold-boolean-void-shell-first.md` and its 2026-10-26 addendum).
    pub boolean_outer_shell_first: bool,
    /// The Boolean's temporary origin shift returns failed bounding-box and operand-transform statuses instead of continuing with differently placed bodies or reporting success after a failed restoration (WR-586, #483). If shifting the second operand fails, the first is restored; cleanup attempts every surviving operand and returns its first error. Off by default for pinned SMLib compatibility.
    pub boolean_temporary_transform_statuses: bool,
    /// Pivot selection and path extension return failed curve evaluations or extensions
    /// before consuming their outputs (#483). Off by default for pinned compatibility.
    pub primitive_path_curve_statuses: bool,
    /// Planar path sweeps propagate mandatory operation failures and delete their
    /// owned temporary B-reps on failure (#483); the mitre's `BrepIntersect` and
    /// `GlobalCurveSolve` searches are returned too, as the fork does (tangency-labs/smlib
    /// c8f6a31; changed by #2009 group C, WR-662). Off by default for pinned compatibility.
    pub planar_path_sweep_statuses: bool,
    /// Fillet corners fail softly where a status upstream ignores would leave an unset
    /// value, as the fork does (tangency-labs/smlib 66b02de; #483, changed by #2009):
    /// `sm_TestG1Continuity` finds no tangency when an end tangent cannot be evaluated,
    /// and `CheckAndFixDegeneracy` refines a vertex's UV point only with a computed one.
    /// The binormal evaluations, the G1 angle and the Nx1 degeneracy test keep upstream's
    /// handling, as the fork does. Off by default.
    pub fillet_corner_statuses: bool,
    /// `sm_CheckEdgeCurveDomains` returns no trimmed curve (the edge's own curve is
    /// used) when the copy cannot be made or trimmed, as the fork does
    /// (tangency-labs/smlib 66b02de; #483, changed by #2009), instead of returning an
    /// untrimmed copy as the requested domain. Off by default.
    pub fillet_edge_curve_domain_statuses: bool,
    /// Offset tangent-gap checks skip a sample they cannot evaluate instead of
    /// measuring an unset point, as the fork does (`HaveTangentGaps`, tangency-labs/smlib
    /// 4803d6d; #483, changed by #2009); `ClosestPoint`'s status stays dropped and a
    /// successful query without a solution keeps its existing meaning.
    pub offset_tangent_gap_statuses: bool,
    /// Surface-intersection singularity alignment aligns nothing when the direction
    /// angle cannot be computed, as the fork does (`ALIGN_DIR_WITH_PREVIOUS_POINT`,
    /// tangency-labs/smlib e7ebc8d; #483, changed by #2009) instead of comparing an angle
    /// read as zero. Captured when the intersector is built.
    pub ssi_alignment_statuses: bool,
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
    /// c8f6a31). Complements `primitive_path_curve_statuses`, `planar_path_sweep_statuses`
    /// and `planar_path_profile_placement_statuses`. Off by default for pinned parity.
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
    /// The fillet executive, solvers, corners and geometry pass on the statuses SMLib
    /// drops: a failed solver load, orientation, list edit, copy, trim, interval edit,
    /// evaluation, knot edit or split-face update returns its error instead of the fillet
    /// carrying on and reporting success (WR-661, #2009 group B; tangency-labs/smlib
    /// 873c7c9, 66b02de, 9b7cd48). Off by default for pinned parity.
    pub fillet_statuses_pass_on: bool,
    /// `SmShell::IsInnerShellOf` passes on the statuses it ignores (fork 4038552): `UNSURE` when the outer shell's closure, the B-rep's box, the sample points or a crossing face's point or normal cannot be computed; a ray that cannot be solved or classified decides nothing and the next ray is tried (WR-497, #1473).
    pub shell_inner_shell_of_statuses: bool,
    /// `SmEdge::ReverseOrientation` returns a failed curve reversal's status before installing the interval, flipping the edge uses or deleting their UV trims, and an edge without curve keeps its interval (WR-260, #521; WR-260 addendum, tangency-labs/smlib 47507b6).
    pub edge_reversal_keeps_failed_edge: bool,
    /// `SmBSplineSurface::CreateIsoParametricCurve` with a domain of no length along the curve (a sliver cell at a double knot) returns a degenerate curve at that point, where upstream's `Trim` refuses the interval and the whole untrimmed iso curve is returned (WR-562; tangency-labs/smlib 8088f5e).
    pub iso_curve_sliver_domain_is_degenerate: bool,
    /// `SmIsoCurve::CreateUVIsoLine` constructs the UV line for both directions: a
    /// constant-`u` curve gets `UV(t) = (iso, t, 0)` over the domain's v interval and a
    /// constant-`v` curve `UV(t) = (t, iso, 0)` over its u interval, with the varying
    /// coordinate's origin at zero. Pinned v1.0.1 tests `SM_SP_U` in both branches, so a
    /// constant-`u` line gets the u interval and a domain-minimum origin (the line is
    /// `P + s*V`, so that minimum is also added to every returned point) and constant
    /// `v` is `SM_ERR_INVALID_INPUT` (WR-30;
    /// `plan/upstream-issues/WR-30--addendum-2026-10-31-uv-iso-line-both-params.md`).
    /// Off by default for pinned C++ parity.
    pub iso_curve_uv_line_both_params: bool,
    /// `GetUVCornerBoundaries`, `GetStartPointUVLoopGap` and `sm_CalcDistAndDev` answer only from what they computed instead of reading unwritten or earlier outputs (WR-262, WR-263, WR-264, #521), as the fork does (changed by #2009 group D to tangency-labs/smlib 7a35f33 and 666242b): in the corner walk an edge use whose trim curve cannot be made has its 3D ends dropped and one whose ends cannot be evaluated adds no boundary (failed drops stay ignored, as upstream); a loop gap that cannot be measured (a failed evaluation or drop) is `SM_BIG_DOUBLE`; a distance without the surface's derivatives or normal is none (`-1`).
    pub topology_failed_evaluations_propagate: bool,
    /// `SetProps_Stage3` returns a failed `ClassifyLoops` status before reading its outputs, and a failed stage drops its in-progress marker so a retry reruns it (WR-268, WR-266, #521).
    pub heal_stage3_failures_propagate: bool,
    /// `CreateCurveSweep` starts a planar path's sweep frame with its z on the plane's normal, not on `MakeUnitOrthoVectors` of the start tangent, so a flat cap stays flat to the rounding (WR-407, #434; SolidCraft change, tangency-labs/smlib e960ca1 (was 1b6f5fa)).
    pub sweep_frame_on_planar_path_normal: bool,
    /// `CreateLinearSweep` tests a profile curve for an arc to the zero an `SmCylinder`'s own check holds its generating curve to (`SM_EFF_ZERO * 1000 * (1 + the largest coordinate of the curve's ends)`), not to the B-rep's tolerance, so only a curve that is an arc becomes a cylinder and any other is swept as itself (`SmSurfOfExtrusion`), not as a cylinder its own edge is off (WR-576, #1507; SolidCraft change, tangency-labs/smlib fd59111).
    pub linear_sweep_arc_to_cylinder_zero: bool,
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
    /// `SmFace::CurveOnClassify` classifies an interval whose two ends lie on one face edge (or its vertices) and whose midpoint is within the intersection tolerance of it as on that edge (the fork's `ClassifyEdgeInterval`, WR-305, #1057).
    pub classify_edge_interval: bool,
    /// `SmPointClassification::ComputePointParameters` takes the nearest of a face's (or surface's) point-solve solutions when they all evaluate to the same point within the source zone tolerance: a point on the seam of a face that closes on itself has its parameters at both ends of the period, and upstream, finding no single solution, drops the classification to `SM_PC_UNKNOWN` (a boolean of a hole and a pin a hair off its axis then fails in `Homogenize`) (WR-537; the fork's tangency-labs/smlib@577f7be).
    pub seam_point_solutions_are_one: bool,
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
    /// The `SmTorus` constructor sets `m_bInsideOut` from its `bInsideOut` (upstream only orients the generating circle with it), and a torus made, rebuilt or read with this fix keeps the flag in its copies (upstream's copy constructor resets it), so the surface and its generator agree (`SmSurfOfRevolution` Test #11 `"InsideOut GenCurve"`) (WR-500, #1490).
    pub torus_keeps_inside_out: bool,
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
    /// `SmSurfOfExtrusion::EvaluateSTEP` and `EvaluateSTEPPoint` evaluate the clamped
    /// copy of the STEP parameters ("clamp input point to surface domain"), not the
    /// caller's unclamped `crUV`: with out-of-bounds evaluation off, a parameter
    /// outside `m_vAnalUVDomain` no longer extrapolates along the extrusion or past
    /// the generator's own domain (WR-61; see
    /// `plan/upstream-issues/WR-61--addendum-2026-10-31-step-evaluators-use-clamped-uv.md`).
    /// Off by default for pinned C++ parity. The guard is `ContainsPoint2d` with its
    /// default `tol = 0`, so a NaN coordinate (never "outside" by that comparison)
    /// stays unclamped, and both bodies clamp in the analytic domain, as upstream does
    /// (neither reads `SwapUV`).
    pub extrusion_step_uses_clamped_uv: bool,
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
    /// A sheet sweep's open-profile wires are glued where the curves meet (WR-409, #1113).
    pub sheet_sweep_joins_profile_wires: bool,
    /// `FindPolygonGuess` keeps a suspicious control-net triangle guess as a candidate against the nearest control point's, and returns the nearer one on the surface, so a rational surface whose control points are farther apart than its neighbouring parts does not lose a good guess (WR-444, #1164).
    pub point_solve_keeps_triangle_guess: bool,
    /// `SmFilletVertex::CalcSetBackVert` measures a corner setback from the vertex `GetStartVertex()` marks over the edge's own interval, not from `GetVertex()` over the curve's natural interval, so an edge whose primary edge use runs against its curve is not measured from its far end (WR-445, #1169).
    pub fillet_setback_from_start_vertex: bool,
    /// `CreateSphereFromArcs` also makes the sphere through four arcs of one centre and radius that close a loop (a corner where four equal rounds meet, such as a pyramid's apex), where upstream's "only works for 3 now" refuses and the corner falls back to an approximating blend (WR-502; SolidCraft oracle case 37).
    pub fillet_corner_sphere_from_four_arcs: bool,
    /// `SmFilletCorner::MakeAnalyticCornerSurface`'s `SER` / `NER` exits return their failing status as an `SmBoolean`, which `SmFilletCorner::CreateCornerPatch` reads as `TRUE` (an analytic surface was made, though none was added) and so skips the Coons patch. With this fix those exits read as `FALSE` and the caller builds the patch. Only the exits that return an upstream status (`SM_ERR` 1001 / `SM_ERR_NULL_POINTER` 1006) change; the port's `Unported`, `InvalidInput` and `PortInvariant` still propagate (WR-254, `plan/upstream-issues/WR-254--smfilletcorner-makeanalyticcornersurface-reports-its-ser-ner--dup2.md`; off by default for pinned C++ parity).
    pub analytic_corner_failure_is_false: bool,
    /// `CreateSphereFromArcs` refuses an arc whose centre is farther than the 3D tolerance from the first arc's, after the existing radius test, instead of reproducing upstream's centre check that compares `sRefFrame.GetOriginRef()` with a reference to that same origin: the check never fires, non-concentric arcs are accepted, and the sphere is built about the last examined arc's centre (WR-103; `plan/upstream-issues/WR-103--addendum-2026-10-31-sphere-from-arcs-common-centre.md`). Off by default for pinned C++ parity.
    pub sphere_from_arcs_common_centre: bool,
    /// `SmEdgeDefinition::Imprint` first moves each end of an intersection edge's interval to the curve's point nearest its proto vertex when that point lies inside the interval, so the edge does not run past its vertex along a face rim (screw-thread groove booleans: "Face has partially Coincident Edges", `SmFace` test #34; WR-452, #435, #410; tangency-labs/smlib ae9167f (was b43876f)). The end goes to its proto vertex's position only when that proto vertex is one vertex in each B-rep, else to its own final vertex (WR-498, #1480; tangency-labs/smlib 0b1b945 (was ffaf4e8)).
    pub imprint_trims_edge_to_proto_vertex: bool,
    /// `SmEdgeDefinition::Imprint` continues an intersection edge's 3d and UV curves, each by the polynomial of its own end span, to a proto vertex that lies a few tolerances past the curve's end, so the edge reaches its vertex (internal thread cuts; WR-453, #410; tangency-labs/smlib bf5d3ae (was a8caccd)). The end goes to its proto vertex's position only when that proto vertex is one vertex in each B-rep, else to its own final vertex (WR-498, #1480; tangency-labs/smlib 0b1b945 (was ffaf4e8)).
    pub imprint_extends_edge_to_proto_vertex: bool,
    /// With `imprint_extends_edge_to_proto_vertex`, a line, circle or ellipse is continued to its proto vertex over its NURB and stays that kind: a circle or ellipse keeps its frame, radii and inside-out sense, its angular domain carried on to the new end, and a line runs between the continued ends (the bar-top/boss arc of a union, an exact `SmCircle` from `SmCone::IntersectWithPlane`, stopped 5.4e-10 short of its tangent point). Without it those kinds are not continued (the fork before this change made them plain B-splines). The new curve must evaluate as the continued NURB does, or the curve is not continued (WR-601; the fork's tangency-labs/smlib@bec8a0b).
    pub imprint_extension_keeps_analytic_curve: bool,
    /// `SmEdgeDefinition::Imprint` takes an edge Brep2's face already has between the definition's two vertices along its curve (`SmTools::FindCoincidentEdge` within the larger of the face's and the definition's tolerance) as the definition's edge there, where upstream makes a second edge beside it: the face then has coincident edges and the merge copies the boundary edge into Brep1 beside the curve's (`SmFace` tests 33 and 34; two equal tubes meeting end to end) (WR-580, #1519; the fork's tangency-labs/smlib@81cd61a).
    pub imprint_reuses_coincident_face_edge: bool,
    /// `SmFilletSolver::ComputePointValues` chooses its finite-difference step for each point (1e-3 at a tangent point, 1e-5 otherwise), not once per process as the function-local static `sdStaticStep`, so a blend does not depend on the blends before it ([`Kernel::fillet_point_step`] is then neither read nor set, and an existing latch is kept as it is) (ND-3, #1173; the fork's tangency-labs/smlib@9e17f64 (was 0a05860)).
    pub fillet_point_step_per_point: bool,
    /// `SmFilletSolver::CalcFilletGeom` starts a trace whose first through point (after the reversal for a tangent last point) is a tangent point toward the next through point, the chord between their points on the first offset surface (when longer than the tolerance), not along the filleted edge's end tangent: the offsets touch at a tangent point, and that tangent can be the line along which they touch to second order, so the first step lay on both offsets within tolerance, the trace missed the next point and ran round a cylinder (a bar's top edge rounded into a cylinder tangent to the bar's side, refused as fillet error 8) (WR-603; the fork's tangency-labs/smlib@68fb916).
    pub fillet_trace_from_tangent_point_toward_next: bool,
    /// `SmBSplineSurface::CopyAndAddAnalytics` keeps an exact copy where the recognised analytic surface does not give the same points at the same parameters (7 x 7 grid), so a face split by `MakeEdgeInFaceWithVU` keeps trims that still lie on its edges (WR-450, #387; the fork's tangency-labs/smlib@7af1987 (was 276b509)).
    pub analytic_copy_keeps_parameterization: bool,
    /// `SmFilletSolver::ComputePointValues`' bigger-step retry of the forward finite-difference step solves with the bigger step (`dBigStep`), not with the small `dStep` again (WR-456, #1181).
    pub fillet_point_bigger_step_retry: bool,
    /// `SmFilletSolver::ComputePointValues` sets a tangent point's curve tangent from the forward finite-difference step alone (along the trace, the previous point's tangent), and from the backward step only when the forward one finds nothing, not from their central difference: the intersection curve ends or branches at a tangent point, and where the offset surfaces touch only to within the model's accuracy (a sketch's 1e-10 residues) the backward step lands on the near-tangent crossing beyond it, 18 degrees off the curve, so every step after the point failed the angle test and the trace crept on in steps down to 8e-10 (repeated rail control points, an internal pole) (WR-595; the fork's tangency-labs/smlib@f8fd83e). Use with `fillet_trace_from_tangent_point_toward_next`, as the fork has both: alone, the forward step from a trace's tangent first point runs along the edge's end tangent, and `Fillet/nurb_cone.smb` edge 8 at r 0.5, which upstream builds, fails.
    pub fillet_tangent_point_one_sided_step: bool,
    /// A fillet's trace also runs through the filleted edge's end where a rail ends on an extended edge, so the surface reaches that end (#1022, WR-468).
    pub fillet_through_extended_rail_end: bool,
    /// `SmFace::ComputePreciseProperties` scales the volume and moment error bounds by the body size (ten times the root of the estimated area, or the origin's distance when larger), not 1.0.1's tenth of it, so the integration converges at tight accuracies instead of returning `SM_ERR` (WR-472; the SolidCraft fork's tangency-labs/smlib@073d4740).
    pub precise_properties_body_length_scale: bool,
    /// `SmFace::ComputePreciseProperties` integrates each UV trim curve piece by piece between its knots and its crossings of the surface's u knot lines, and each iso-u integral of `SmODEAreaIFEO` piece by piece between the surface's v knots, so no Runge-Kutta step crosses a knot of a B-spline surface (a NURB cylinder's, sphere's or torus's rational-circle double knots) where the integrand's derivatives jump and the step's error estimate does not bound its error; the requested accuracy then holds at 1e-8 (WR-494, #1435; the SolidCraft fork's tangency-labs/smlib@08d9a14).
    pub precise_properties_split_at_surface_knots: bool,
    /// With `precise_properties_split_at_surface_knots`, each iso-u piece of `SmODEAreaIFEO` evaluates the surface at its end from the knot span below it (`bVFromLeft = FALSE`), not the span above (upstream's `TRUE`, kept by the fork's tangency-labs/smlib@08d9a14): where the piece stops at a v knot with a derivative jump (a G2-not-C2 loft merged into one face across its 19 spans, #2091), the integrand's last point came from the next span, the Runge-Kutta steps shrank toward every knot, and each piece took about 5x the evaluations it needs: the #2091 model's precise volume took 26.7 s, 5.8 s with this fix (WR-670, #2091). No effect without `precise_properties_split_at_surface_knots`.
    pub precise_properties_span_end_from_below: bool,
    /// `SmFace::ComputePreciseProperties` starts each iso-u integral of `SmODEAreaIFEO` at the lowest v of the face's UV trim curves (the union of their bounding boxes, kept no lower than the face's UV domain), not at the UV domain's lowest v: on a face whose domain is the surface's unbounded one (`±INFINITE_PARAMETER` in v) the integrals ran about 1.2e6 parameter units below the face and the area came out as a small difference of large terms, off by up to 190% (WR-563, #1556).
    pub precise_properties_trim_base_v: bool,
    /// `SmBrep::ComputePreciseProperties` adds a volume-separating faceuse's volume and mass only where it bounds a non void region and its area only where the area behaviour selects it, where upstream adds both whenever either behaviour selects the faceuse: a face between two solid regions (a merge of overlapping bodies) was counted in the area once per side (WR-535; the fork's tangency-labs/smlib@d2d9586).
    pub brep_area_between_solid_and_void: bool,
    /// An offset surface evaluated beyond its base's domain extends the base itself on every side that can grow at once, each by the domain size times the least power of two that reaches the point and never less than the extension it replaces, so two copies of one offset give the same points whatever was evaluated first (WR-458, #419; the fork's tangency-labs/smlib@9f54d05 (was 2d7c7ed)).
    pub offset_extension_from_base: bool,
    /// With `offset_extension_from_base` (which must be on too; this flag does nothing without it), an offset surface whose base is a cylinder, cone, sphere or torus extends that base past its domain as itself, where upstream's tangent ruled extension (G1R) leaves the surface at once: a cylinder R25 continued 0.44 past its face's end was 0.0036 off the circle, and a round traced there was built that far off all along (WR-633, #960; the SolidCraft fork's tangency-labs/smlib@de0a6b8, branch `replay/1.0.1`). The extension is built from the analytic surface (the fork's @93db777): the base's spans are kept and new ones joined at its ends with a knot of full multiplicity, every row round the axis continues in rational quadratic arc spans, a sphere's or torus's generator in quarter turns, and a cone's line straight. Round the axis it reaches no further than closing the turn, half the gap past either end, and once it reaches the closure that direction wraps like a periodic one (WR-650, #1986; the fork's @b298cc2): a 294 deg arc was extended to 882 deg and more. A surface of revolution whose generator is an arc of one circle (SMLib's concave round of a circular edge) continues it as its own circle, as a torus does (the fork's @43fd6b6).
    pub offset_extension_as_itself: bool,
    /// `N_FillNSidedHole` (through `SmBSplineSurface::CreateCornerBlend`) shortens the cross-boundary derivative across an inner curve where its cubic blend from the opposite boundary would leave that boundary's tangent plane, so a corner patch (a fillet setback corner) stays inside planar faces it meets, with G1 kept (WR-461, #1170). Curved neighbours are not covered.
    pub corner_blend_stays_inside_neighbours: bool,
    /// `DoFilleting`'s `BooleanPostProcess` removes none of the target's own edges, nor an edge between two of its faces (the fork's `m_sPostProcessKeptEdges`/`Faces`), so the patches of a sphere imported in six faces are not merged into a face no patch's parameterization can carry (WR-459, #423; the fork's tangency-labs/smlib@b7d7b96 (was 9ddf4a8)).
    pub fillet_cleanup_keeps_target_faces: bool,
    /// With `fillet_cleanup_keeps_target_faces` on, `DoFilleting`'s `BooleanPostProcess` still checks a kept edge (one of the target's, or one between two of its faces) that has one face on both sides: removing that slit merges no faces, and the test of an edge between two kept faces needs two faces. Upstream (with the fork's keep) leaves the slit where a round took the face on one side of a target edge away (WR-602; the fork's tangency-labs/smlib@cac11ea).
    pub fillet_cleanup_removes_input_slits: bool,
    /// `SmFilletNx1Corner::MakeCornerTopology` skips a side edge whose faces meet tangentially at the corner's vertex (a flat top running smoothly onto a round) round the vertex to the edge past it, so the end face is the one beyond the tangent seam (the slot wall), not the round the fillet never meets, and `SmFilletEdge::CalcEdgeOnExtendedEdge` intersects the side face with the end face's extension when the side face is not one of the edge's own faces (WR-457, #418; the fork's tangency-labs/smlib@ba7f68e (was d5ebcb3)).
    pub fillet_nx1_tangent_side_edge: bool,
    /// `SmFilletNx1Corner::MakeCornerTopology`'s tangent side-edge skip (`fillet_nx1_tangent_side_edge`, which must be on too) also passes, once, the filleted edge's G1 continuation across the vertex of the opposite convexity (past any tangent edges): the overhang's rim continuing the base circle of a cylinder standing over a block's edge. The edge past it, of the filleted edge's convexity, is the side edge extended and the face beyond it the end face, so the round ends there instead of failing with status 1001 (WR-499, #1474; the fork's tangency-labs/smlib@420e47d).
    pub fillet_nx1_opposite_continuation_side_edge: bool,
    /// `SmCircularCrossSectionFSG::CreateSurfaceFromCurves` moves both rails' end control points to their midpoint at an end where the rails' ends lie within the approximation tolerance of each other (a degenerate corner whose cross section is a point), before the surface is skinned: each rail is lifted from its own surface, so at a vertex their ends differ by the model's accuracy (6.5e-12 mm on a sketch with solver residues), and the skinned surface kept a near pole there instead of an exact one, which `SmSurface` check test 5 refuses as close to singular (WR-594; the fork's tangency-labs/smlib@5fdefb9).
    pub fillet_rails_meet_at_one_end_point: bool,
    /// `SmFilletNx1Corner::MakeCornerTopology`'s tangent side-edge skip (`fillet_nx1_tangent_side_edge`, which must be on too) takes the edge reached past the tangent side edge as the corner's edge also when that edge's own faces meet tangentially at the vertex (a stadium bar's flat side running onto its round end, below the cylinder wall the bar's top meets): such an edge has no convexity, and `IsConvexRadialSector` on it reads only the sign of rounding residues in the geometry, so one end of the round's arc could read "opposite", leave no corner edge and fail with status 1001 (fillet error 3, corner type 8) (WR-593; the fork's tangency-labs/smlib@1ff7353).
    pub fillet_nx1_tangent_corner_edge: bool,
    /// `SmFilletNx1Corner::MakeCornerTopology` takes a side edge whose faces meet tangentially at the corner's vertex (a stadium bar's flat side running onto the cylinder its round end lies on) as of the filleted edge's convexity: `same_convexity_edges` on such a G1 edge reads only the sign of rounding residues, so with a sketch's 1e-10 residues the two ends of one concave arc read differently, the front corner was not built as the mixed-convexity one the back end is, and the round failed with status 1001, "Corner Edge Computation Failure" (WR-600; the fork's tangency-labs/smlib@7992ec7). Such a side edge that is a tangent seam the rail crosses (its faces' normal curvatures across it differ: a bar's flat side running into the cylinder of its round end) is the rail's tangent end: the rail end is a rail/edge crossing, the end face the face across the seam continued as itself, the end rib solved from the tangent end outward (`blend::fillet_geom::tangent_end_rib`; for an odd contact order past 1, a G2 blend's, fitted in `t = sqrt(s)` with zero speed at the tangent end, the fork's tangency-labs/smlib@cd47efd) and the cap edge the seam's own curve (the fork's tangency-labs/smlib@856b54e; a chamfer ending there failed with status 1001). Its `RailEdgeuseIntersect` run-on past the corner's vertex along the seam acts only with `fillet_run_on_past_far_end_only` on too. On a cylinder it also needs `fillet_run_on_bspline_trims` (WR-654), because the seam's B-spline UV trim clamps: without it a concave blend whose rail crosses the seam past the vertex fails with status 1001.
    pub fillet_nx1_tangent_side_edge_same_convexity: bool,
    /// `SmFillet3x2MixedConvexityCorner::MakeCornerTopology` gives the roles of the corner's two filleted edges by the unfilleted edge's convexity: where it is concave (a boss standing over a bar's side face, its line on that face running down from the vertex) the convex fillet's rail crosses it and the concave fillet's rail crosses its continuation past the vertex, instead of the other way round as in an L-shaped box, so the corner no longer fails with status 1001 "Corner Geometry Computation Failure" (WR-524, #1475; the fork's tangency-labs/smlib@1255993).
    pub fillet_3x2_mixed_concave_side_edge: bool,
    /// With `fillet_3x2_mixed_concave_side_edge` (which must be on too), `SmFillet3x2MixedConvexityCorner::MakeCornerTopology` takes the two fillets' roles at an unfilleted edge whose faces meet tangentially at the vertex (a block's side running tangent into a cylinder whose wall the block's top meets) from the faces' corners, not from `IsConvexRadialSector`, which on a G1 edge reads only the sign of rounding: the convex fillet's rail crosses that edge unless its face shared with the edge turns more than half a turn between them at the vertex (`sm_CheckFaceComplimentaryCornerConcavity`), so the concave round's rail is no longer sent to cross an edge it never meets and the corner no longer fails with status 1001 (fillet error 3, corner type 5) (WR-597; the fork's tangency-labs/smlib@c17b745).
    pub fillet_3x2_tangent_side_edge_roles: bool,
    /// `SmFilletNx2Corner::MakeCornerTopology` mitres two blends meeting at a vertex of three edges where the common face's corner between them is concave (mixed convexity: a pocket's rim at its sharp corner, a boss's root at its sharp corner, the side edge not filleted), as at a corner of uniform convexity: the rails meet in the common face (`SM_FV_RAIL_X_RAIL`) and on the side edge, the blends' surfaces meet along their intersection, and the corner is a bevel (where the rails meet the side edge at two places, `AdjustTopology` joins them along the side face). Upstream sweeps a 3-sided face round the side edge between ribs straight across from where the rails meet it. Edges meeting at 5 degrees or less still blend (WR-642; the fork's tangency-labs/smlib@4f06e72, the owner's ruling of 2026-10-30).
    pub fillet_mitre_concave_face_corners: bool,
    /// `SmFilletNx2Corner::AdjustTopology`, at a bevel (mitre) of two blends meeting at a vertex of three edges where the common face's corner between them is concave and whose rails meet the side edge at two places (walls leaning differently, a sloped basin's rim), builds the topology `AdjustConcaveCornerTopology` builds at a swept corner of mixed convexity instead of splitting the side vertex as for a convex bevel: each rail ends where it meets the side edge, the larger round's cut and the smaller rail's extension run on the side face's extended surface to where the mitre's curve meets them, and `CreateCornerPatch` fills only that gap with a patch on the side face's extended surface (`FilletCorner::bevel_gap_patch`). Upstream cut the larger round by the side face's own surface, which ends at the side edge (`CalcFilletIntSideFace` fails), or the smaller rail ran past its face (`TestRollOver` fails). Mitres at such corners come from `fillet_mitre_concave_face_corners` or a corner set to bevel (WR-644; the fork's tangency-labs/smlib@2223108).
    pub fillet_mitre_gap_patch: bool,
    /// Each fan of faces at a vertex where the body touches itself is a corner of its own: `SmFilletExecutive::SetFilletParameters` splits each filleted edge's vertex whose edge uses form several fans sharing no edge (two cubes sharing a corner; the up and down halves of squares drawn edge to edge, touching at the origin) into one vertex per fan at its point (`SmBrep::SplitVertexByFans`, `crate::topology::brep::vertex_fans`), `DoFilleting` joins the target's vertices still at a split point after the blends, whether they built or not (`SmBrep::JoinVertexFans`), and `SmFilletCorner::Create` builds no two-edge corner where an edge not blended there is a knife edge (its faces tangent and folded back on each other, `SmFilletSolver::IsKnifeEdge`). Upstream `SmFilletCorner::Create` walked one fan's faces, found the blended edges of both fans and built an Nx2 corner of edges sharing no face, failing with status 1001 ("unable to make topology") (WR-665; the fork's tangency-labs/smlib@4ecfbab and @e418750).
    pub fillet_corner_per_vertex_fan: bool,
    /// `TrimExtendedEdge` takes, on a closed curve, the arc that runs where its base edge does, not always the interval between its two trim points: a hemisphere's rim, two arcs of one circle, no longer becomes one arc twice (WR-463, #403; the fork's tangency-labs/smlib@34eaa6b (was fa7ff4a)).
    pub offset_closed_curve_trim_arc: bool,
    /// The offset builds a vertex where more than three faces meet (a floor of four facets falling 1 degree to a drain, and its corners where two walls meet two facets) as the two vertices and the new edge the four offset planes give, where upstream fails the shell (1001): `SmOffsetGeometryCreation`'s tangency tolerance is half a degree, not two (the facets, 1.2 degrees apart, are intersected, not left to meet by extending their trims); `TrimExtendedEdge` keeps a trim point within 5 degrees of tangent where the curve crosses the surface (only a touch is dropped); and, where two faces' offsets cut the edge at a vertex all of whose edges are convex (or all concave), the edge ends at the cut it reaches first from its other end, not at the one nearest the vertex. It has no effect without `offset_closes_loop_gaps`, which closes the loop gaps the split leaves on the faces round the vertex (alone, the split returns a solid with those loops cut short, of the wrong volume) (WR-666, #2052; the fork's tangency-labs/smlib@2e1bb8c).
    pub offset_four_face_vertex_split: bool,
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
    /// `create_fillet_brep` refuses a blend whose rolling ball does not fit the faces it rolls on (a face curves tighter than the ball at a section of the edge: `BallFitsItsFaces`) before the corners, with `FilletErrorType::NoRoom` (error info code 8, not an upstream code), where the corners fail with the generic status looking for a rail end that is not there (WR-462, #1172; SolidCraft change, diff `2c5baae..e5696ca` in the issue, in the fork as tangency-labs/smlib@a15b5a2).
    pub fillet_refuse_ball_not_fitting: bool,
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
    /// `SmMerge::BooleanLists` copies its surface operands with the virtual `Copy` (the derived surface, its natural domain and its kind), where upstream's base copy constructor slices every surface to a geometry-less `SmSurface`, so a surface list always fails with `SM_ERR` and the planar 2D path is never taken (WR-196; `plan/upstream-issues/WR-196--booleanlists-slices-derived-surfaces-into-base-smsurface.md`).
    pub boolean_lists_virtual_surface_copies: bool,
    /// `SmSphere::IntersectWithPlane` and `IntersectWithSphere` return a single touching point only when the intersection circle's own radius (`sqrt(R^2 - d^2)`, or the two-sphere circle's height, each formed from the gaps to tangency) is within the tolerance, not whenever the gap to tangency is, which collapsed circles of radius up to about `sqrt(2 R tol)` (1e-3 for `R = 1`, `tol = 1e-6`) into one point (WR-79 part 1, #396; the fork's tangency-labs/smlib@1f5c0f7 (was 30d25bf)).
    pub sphere_small_circle_not_touching: bool,
    /// `SmAdvSurfaceIntersector::FindInteriorCurves` seeds a Newton search for isolated tangential contact points (zeros of `S1 - S2` with parallel normals, 4x4 system with the exact Jacobian) from the leaf pairs it cannot trace (normals within 5 or 2 degrees, no local intersection), plus the poles, and adds each contact with a numerically zero gap and normals within 1e-4 rad that is not on a curve already found as a degenerate `TOUCHING` curve (WR-74, #394; the fork's tangency-labs/smlib@9e8bf10 (was c26edbc)). Higher-order contacts (equal curvature) are not found.
    pub ssi_tangent_contact_points: bool,
    /// `SmAdvSurfaceIntersector::FindInteriorCurves` traces a start point whose surface normals are within 5 degrees (guess) or 2 degrees (solution) on probation instead of skipping it, keeping the trace only when it closes into a crossing loop whose box diagonal exceeds twice the tolerance; starts below `2 tol / model size` radians are still skipped and rejected traces (and their start points) are remembered so neighbouring starts are not retraced. A small loop between nearly tangent surfaces (a plane just above a NURBS paraboloid's apex) was otherwise never found (WR-79 part 2, #396; the fork's tangency-labs/smlib@1f5c0f7 (was 30d25bf)).
    pub ssi_low_angle_start_on_probation: bool,
    /// `SmSurfaceIntersector` traces an open tangent curve once: `AddStartPoint` keeps one start point per cluster of tangent boundary start points (`IsSameTangentCluster`), a finished trace removes the clusters at its ends (`DoIntersection`), `TraceCurve` runs its "already traced" test on a first step that snapped onto another start point, and a near-tangent trace stops at the free end of a curve already found instead of running over it (WR-76, #392; the SolidCraft fork's tangency-labs/smlib@920da20 (was 0ce09f2)).
    pub ssi_trace_tangent_curves_once: bool,
    /// `SmSurfaceIntersector::DoIntersection` and `FindInteriorCurves` trace a curve back the other way only when the trace produced points: `TraceCurve` returns an empty curve when its start point already lies on a found curve, and upstream then reverses the empty list and fails the whole intersection with `SM_ERR_NULL_POINTER` (WR-80; the fork's tangency-labs/smlib@0dbe681).
    pub ssi_no_reverse_of_empty_trace: bool,
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
    /// `DoIntersection` traces the tangential intersection curves the marcher cannot follow (the surfaces touch along them, `n1 x n2 = 0`), and the crossing branches through the contact points where crossing curves meet, which it reports as touching points (WR-75; tangency-labs/smlib ef4ab25, e9fde8f, d8c61db, 1693c9d). `FindInteriorCurves` keeps the node pairs it skips for parallel normals as seeds.
    pub ssi_tangent_curve_tracer: bool,
    /// `SmSurfaceIntersector::DoIntersection` splits each traced crossing (or near-tangent) B-spline curve, with its two UV curves, where it passes through a singular point of the intersection (the normals parallel while two branches cross): `|n1 x n2|` is sampled 8 times per knot span, each sharp minimum refined by golden section, and the curve split where the sine is below 1e-6 and rises above 1e-3 again on both sides, more than 10 approximation tolerances from its ends; the tails are appended. Upstream's marcher steps straight through such a node, so the curves cross there without ending; two cylinders touching inside at one point then union to a body that loses one of them, fails 1001 in `MakeFaceuse`, or fails the check's seam tests (WR-77, #393, #2110; the fork's tangency-labs/smlib@3c9d1d3, `sm_SplitCurvesAtSingularPoints`).
    pub ssi_split_curves_at_singular_points: bool,
    /// `MakeDupin` uses `1/sqrt(abs(k1))` for the bottom hyperbola's second axis, matching the top conjugate branch at unequal-curvature saddle points (WR-69).
    pub dupin_bottom_hyperbola_axis: bool,
    /// `SmFilletIntersector::ComputeNextPoint` rejects a next point that is more than half a step short of where the step aimed, when the step was not clipped: where `StepSolve`'s plane-adjusting retry lands on another root of the sphere equations (a variable round whose law is evaluated past the end of its edge), the span passes the Hermite test along a straight trace, but its rail parameter advances by the whole step over a fraction of it, and the rail doubles back (WR-503).
    pub fillet_trace_rejects_short_step: bool,
    /// `SmFilletIntersector::ComputeNextPoint`, where the solver's offsets change along the trace (`OffsetRadiiCanChange`, a variable round), solves the fillet at the span's five test shares (a step of that share from its start) and rejects the span where a solution's contact lies farther than the approximation tolerance from its Hermite rail. Upstream tests only that the rails lie on the base surfaces, which on a plane every uv curve does, so the step grows to the angle tolerance and the rails cut across the radius law between the traced points (a cubic-law round on planar faces removes the wrong volume, its surface off the law's circle) (WR-646, #1989; the fork's tangency-labs/smlib@4438859).
    pub fillet_variable_span_follows_fillet: bool,
    /// `SmCurve::CalculateTightBoundingBox` adds only the B-spline knots strictly inside the queried interval (the interval's ends are already in the box). Upstream adds every tangent-discontinuous knot and always the last knot whatever the interval, so the tight box of an edge on part of a NURBS curve (an imported sphere cap's seam, 30 deg of a quarter-circle NURBS) reaches points the edge never does (WR-474, #1054; the fork's \"a curve's tight box holds the knots inside its interval only\").
    pub curve_tight_box_interval_knots: bool,
    /// `SmFace::CalculateTightBoundingBox` searches a trimmed face's own extreme inside its trim (subdividing the UV domain from the vertex and edge extremes, bounded at depth 16 / 4096 cells, an undecided cell divided at the surface's knot inside it nearest its middle, else at its middle), not only the surface's global extreme, so a face whose surface peaks outside the trim is not boxed too small (WR-588, #1799; tangency-labs/smlib@beb6061 (was 9d44c1b)), nor one whose rational surface has knots off the domain's dyadic points too large (WR-651; tangency-labs/smlib@326de73); only points classified inside the face (`SM_PC_FACE`), not on its edges or vertices, count as the face's, so a rounded box corner's sphere octant is not boxed past its arcs (1.0000243 for 1; tangency-labs/smlib@95bf434).
    pub face_tight_box_searches_trim: bool,
    /// `SmTopologyIntersector::IntersectTopology` skips a face pair whose face boxes lie farther apart than the intersection tolerance (a box that cannot be computed, or holds nothing, rules nothing out), and `SmBrep::MakeEdgeInFaceWithVU`, on a plane, leaves in place without classifying an inner loop whose test point lies outside the box of the new face's outer loop (grown by the edges' tolerances, at the retry tolerance). Upstream pairs every face of one sheet with every face of the other through whole-surface checks and classifies every inner loop against each imprinted face, so a 2D boolean of a sheet with n holes takes time in n^2 (WR-527, #1526/#1527; the fork's tangency-labs/smlib@60b1c6a).
    pub boolean_skips_faces_apart: bool,
    /// `SmBrep::DeleteTopologicalVertex` joins an edge whose curve is a curve on a B-spline-kind surface along a UV line of constant u or v (the seam an extrusion leaves on its wall) as that surface's iso curve over the edge, the same curve exactly, so the seam of a full-turn face that a union runs on through a covered face is one edge (WR-598; the fork's tangency-labs/smlib@829b982). Upstream casts the curve's copy to `SmBSplineCurve` and dereferences the null; without the flag the port leaves the vertex in place (the routine's silent `SM_ERR`).
    pub delete_vertex_joins_seam_iso_curve: bool,
    /// `SmBrep::MakeEdgeTopology`, when a new edge joins a wire shell (its start vertex's) to a faceuse shell (its end vertex's), moves the wire shell's wires into the faceuse shell, puts the faceuse shell in the wire shell's place in the region's shell list and deletes the wire shell. Upstream moves every face use of the faceuse shell into the wire shell (and `SmShell::GetWireEdges` passes over every edge of the B-rep), the size of the body for each such edge a boolean makes. The shell left holds the same face uses in the same order and the same wires in the same place, but it is the faceuse shell's object, not the wire shell's (WR-585; the fork's tangency-labs/smlib@be8ac73).
    pub boolean_wire_edge_keeps_faceuse_shell: bool,
    /// `SmBrep::TestClosureSplitRegion` asks whether a new face closes a region with `ClosureTraversalFromBothSides`, two breadth-first searches through edge-use radials from the face use and from its mate that stop when they meet or one runs out, instead of `ClosureTraversal`'s depth-first walk from one side that stops only at the mate. The answer is the same, and no mark is claimed or set; upstream's walk can cross most of the shell for each face a boolean makes, so a union costs the new faces times the shell's size (WR-584; the fork's tangency-labs/smlib@69c5198).
    pub boolean_new_face_closure_both_sides: bool,
    /// `SmStitch::StitchIntoSolid`, when its stitch leaves no lamina edge and more than one region, makes the B-rep a nested solid model (`FindAndSetInfiniteRegion(NULL, SM_VS_NESTEDSOLID)`, with editing disabled): material inside the outer shell, a void inside a shell within it, and so on, each region's outer shell first. Upstream's stitch only finds the infinite region (`SM_VS_INFINITEREGION_SWAP`), so the region inside a closed shell nested in another stays solid and the shells stay where the gluing put them: a holed profile swept round a closed path measures as if it had no hole, and twisted a half turn fails the check (Test #12, an inner shell not inside its region's outer shell) (WR-569; the fork's tangency-labs/smlib@351c57c).
    pub stitch_solid_nests_cavities: bool,
    /// `SmBrep::GlueVertices` joins the pair of shells of every region both vertices are in (each other pair's children transferred with its wire edges, the emptied shell removed), not only the last pair its search finds: a solid's vertex is in a shell of the solid's region and one of the infinite region's, so where a face that alone held two parts of a body together was deleted, gluing the parts again at a vertex left the other region's pair apart, and the check reported an edge use and its radial in two shells (WR-607; SolidCraft change, tangency-labs/smlib 661f79c).
    pub glue_vertices_joins_every_region: bool,
    /// `SmPrimitiveCreation::CreateBox` returns the status of each face it makes (a plane that cannot be made, a face whose making fails even after the face was made, such as a box far smaller than the tolerance whose top face closes up on its bottom one) and of its `StitchAndOrient`, and makes no face on a side of zero extent. Upstream drops every one of those statuses and returns success with whatever faces were made (WR-596; the fork's tangency-labs/smlib@0df62c8, 35008b1 and 449c81c, `AddBoxFace`).
    pub box_face_failures_propagate: bool,
    /// `SmEllipse::AdjustSTEPInterval` converts a new interval end outside the current arc
    /// itself: past a full-turn arc it is taken whole turns back into the arc and its NURBS
    /// parameter moved by the circle's NURBS length per turn (the other way for an
    /// inside-out curve); past a partial arc the arc's NURBS interval continues in
    /// proportion to the angle. Upstream's polar converter clamps such an angle to the arc's
    /// ends, so a full circle moved to `[360, 540]` (a torus face whose tube range starts a
    /// turn on) collapses to one NURBS parameter and the surface built on it gets NaN knots
    /// and fails with `SM_ERR` (WR-211, #428; the fork's tangency-labs/smlib@5c0b5ed).
    pub ellipse_adjust_step_interval_keeps_turns: bool,
    /// `SmBrep::DeleteFilletFace` removes a ring fillet (a round all the way round a boss's foot or rim: two closed tangent rails, one vertex each, and at most one seam) with the fork's `SmRingFilletRemoval`: the sharp edge is the intersection of the two tangent faces' surfaces within their natural domains (an exact circle for a boss), the tangent face's seam runs on to it along its line, and the rails become it; a removal that would need a surface or a curved edge continued past its domain is refused before the B-rep changes. Upstream takes four-sided fillets only, and fails on a ring (WR-605; SolidCraft change, tangency-labs/smlib 52bfef0, with 661f79c under `glue_vertices_joins_every_region`).
    pub fillet_removal_takes_ring_fillets: bool,
    /// `SmFeatureExecutive::DoDefeature` refuses a feature whose base patch could not be built (`SmFeature::BuildPatches` leaves `m_bBaseRebuilt` FALSE), with `SM_ERR` (the loop edge of the first unfilled loop in the error), and puts the base B-rep back as it was before the call; the executive is left with no features. Upstream applies no patch, returns success and hands back the base with the feature's faces cut out and the hole open: any face of a box with `SM_RB_EXTEND_SURFACE` gives 5 faces, volume 0 (WR-656; own design, not in the fork). A rebuild type of `SM_RB_NONE`, which asks for the hole, is not refused.
    pub defeature_refuses_open_base: bool,
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
    /// tangency-labs/smlib@c2e3d9d).
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
    /// `DoIntersection`'s start-point loop fails with [`crate::SolidError::InvalidInput`]
    /// once its traces have taken [`MAX_START_LOOP_TRACE_STEPS`] `ComputeNextPoint` calls
    /// together. Upstream's loop ignores a failed trace and goes on to the next start point,
    /// so a trace creeping toward a surface boundary repeats until every start point has run
    /// out of its 5000 points: defeaturing a fillet face of an ordinary box takes about 16
    /// minutes in release here and 11 in C++ (HANG-52). It does finish, so this is a time
    /// limit and not a hang fix. Off by default (pinned behaviour).
    ///
    /// [`MAX_START_LOOP_TRACE_STEPS`]: crate::intersect::surface_intersector::p4::MAX_START_LOOP_TRACE_STEPS
    pub ssi_start_loop_trace_budget: bool,
    /// `SmEdgeDefinition::Imprint` makes no edge for a definition whose two ends are one proto vertex and land on one vertex in each B-rep (`GetFinalVertex`), when its curve is a speck on that vertex (9 samples, ends included, all within each B-rep's vertex tolerance and within the definition's own tolerance, which its edge would have) and another definition of the same proto edge is imprinted between two distinct vertices in each B-rep. Upstream makes a closed edge from that vertex back to itself along the speck; a coplanar face at it is then classified against a flat radial sector, and a union of two cubes within 5e-6 of each other across a 1e-4 strip deletes its bottom sliver piece and comes back with no solid region (WR-619, #1794; own design, not in the fork).
    pub imprint_skips_collapsed_definition: bool,
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
