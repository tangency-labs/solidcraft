//! The solidcore backend: one kernel for the process (SMLib's context is process-global too, and
//! some of its state, such as the fillet solver's latched step, lives for the whole process),
//! every call under its lock and a panic guard.
//!
//! Upstream fixes ([`fixes`]) are solidcore's opt-in corrections of SMLib defects that the C++
//! fork fixed in its own sources; each one turned on here says which fork commit it matches.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Mutex, PoisonError};

use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::{Kernel, KernelOptions, UpstreamFixes};
use solidcore::mass::integrator::properties::{PropertyAttributes, brep_volume};
use solidcore::primitive::api_primitives as primitives;

use crate::{Error, Result};

pub(crate) mod blend;
#[cfg(test)]
mod blend_tests;
pub(crate) mod boolean;
#[cfg(test)]
mod boolean_tests;
mod convert;
#[cfg(test)]
mod convert_tests;
pub(crate) mod history;
mod measure;
#[cfg(test)]
mod measure_tests;
pub(crate) mod merge;
#[cfg(test)]
mod merge_tests;
pub(crate) mod offset;
#[cfg(test)]
mod offset_tests;
pub(crate) mod split;
#[cfg(test)]
mod split_tests;
pub(crate) mod sweep;
#[cfg(test)]
mod sweep_tests;
pub(crate) mod transform;
#[cfg(test)]
mod transform_tests;
pub(crate) mod trim;
#[cfg(test)]
mod trim_tests;

pub(crate) use convert::{from_arrays, to_arrays};
pub(crate) use measure::{area, bounds, check, classify_point, edge_face_distance, is_manifold_solid, mass_properties, solid_count};

/// A solidcore body. Its own lock: solidcore's operations take `&mut Brep`, even to measure.
pub(crate) struct Body(pub(crate) Mutex<solidcore::topology::Brep>);

impl Body {
    pub(crate) fn new(b: solidcore::topology::Brep) -> Body {
        Body(Mutex::new(b))
    }
}

/// The fixes this backend runs with: every one of solidcore's opt-in fixes of upstream SMLib
/// defects (`UpstreamFixes`, each naming its record and issue). They are all fixes; the C++ fork
/// carries most of them in its own sources. A test checks none is left off but those in
/// `HELD_BACK`, so moving the solidcore pin to a rev with a new flag fails until it is added here.
pub(crate) fn fixes() -> UpstreamFixes {
    let mut f = UpstreamFixes::default();
    f.fillet_retry_radius_as_first = true;
    f.fillet_rail_rim_on_face = true;
    f.fillet_refuse_rails_leaving_faces = true;
    f.fillet_refuse_face_consuming_blends = true;
    f.fillet_refuse_non_manifold_result = true;
    f.fillet_refuse_crossing_rails = true;
    f.fillet_vertex_error_diagnostics = true;
    f.fillet_corner_ball_patch = true;
    f.fillet_chamfer_cone_on_arc = true;
    f.fillet_torus_center_on_axis = true;
    f.offset_excess_faces_after_merge = true;
    f.offset_clip_right_side_out = true;
    f.revolution_offset_axis_both_sides = true;
    f.offset_extended_edges_mated = true;
    f.remove_slit_loops_where_solids_touch = true;
    f.keep_operand_uv_trims = true;
    f.proto_vertex_miss_is_false = true;
    f.check_loop_vertex_tolerance = true;
    f.check_loop_projected_seam_ends = true;
    f.trim_boundary_seam_wrap = true;
    f.bspline_check_min_radius = true;
    f.oscillating_skips_failed_guesses = true;
    f.refine_cc_range_ends = true;
    f.flat_corners_test_bits = true;
    f.cylinder_rebuild_mean_radius = true;
    f.planar_path_uncapped_open_profiles = true;
    f.revolve_analytic_profile_faces = true;
    f.revolve_arc_centre_near_axis_is_sphere = true;
    f.taper_pivot_nearest_corner = true;
    f.taper_tangent_corner_planes = true;
    f.revolve_nested_shells_as_cavities = true;
    f.revolve_holed_profile_void = true;
    f.curve_sweep_caps_at_sweep_ends = true;
    f.join_touching_loops = true;
    f.uv_trim_reversal_on_surface = true;
    f.offset_resolve_crossing_loops = true;
    f.offset_walls_touch_refused = true;
    f.offset_analytic_mate_edges = true;
    f.precise_properties_split_at_surface_knots = true;
    f.nurb_cylinder_inside_out_frame = true;
    f.shell_inner_shell_of_statuses = true;
    f.local_operation_offset_status = true;
    f.local_operation_refuses_loose_tangent_edges = true;
    f.torus_major_radius_from_origin = true;
    f.fillet_corner_sphere_from_four_arcs = true;
    f.fillet_nx1_opposite_continuation_side_edge = true;
    f.fillet_trace_rejects_short_step = true;
    f.torus_iso_circle_axis_from_midpoint = true;
    f.torus_keeps_inside_out = true;
    f.internal_pole_row_through_knot = true;
    f.extrusion_origin_scaled_zero = true;
    f.transform_scales_each_tolerance = true;
    f.transform_model_size_from_body_box = true;
    f.offset_collapsed_rounds_mated = true;
    f.offset_missing_surface_fails = true;
    f.fillet_nx1_end_rib_global_merge = true;
    f.uv_trim_corner_end_points_only = true;
    f.bspline_trim_preserves_uv_spans = true;
    f.cone_parallel_line_midpoint = true;
    f.cone_parallel_line_domains = true;
    f.proto_edge_self_loop_on_vertex_degenerate = true;
    f.slice_coincident_faces_sheet_side = true;
    f.boolean_outer_shell_first = true;
    f.edge_reversal_keeps_failed_edge = true;
    f.topology_failed_evaluations_propagate = true;
    f.heal_stage3_failures_propagate = true;
    f.sweep_frame_on_planar_path_normal = true;
    f.cover_refuses_folded_continuation = true;
    f.boolean_vertex_by_best_crossing = true;
    f.boolean_cleanup_removes_slits = true;
    f.classify_edge_interval = true;
    f.ellipse_trim_seam_lines_span_cone_height = true;
    f.degenerate_proto_edge_in_one_vertex = true;
    f.new_edge_vertex_tolerance_covers_gap = true;
    f.torus_signed_gen_curve_interval = true;
    f.sheet_sweep_joins_profile_wires = true;
    f.point_solve_keeps_triangle_guess = true;
    f.fillet_setback_from_start_vertex = true;
    f.imprint_trims_edge_to_proto_vertex = true;
    f.imprint_extends_edge_to_proto_vertex = true;
    f.fillet_point_step_per_point = true;
    f.analytic_copy_keeps_parameterization = true;
    f.fillet_point_bigger_step_retry = true;
    f.fillet_through_extended_rail_end = true;
    f.precise_properties_body_length_scale = true;
    f.offset_extension_from_base = true;
    f.corner_blend_stays_inside_neighbours = true;
    f.fillet_cleanup_keeps_target_faces = true;
    f.fillet_nx1_tangent_side_edge = true;
    f.fillet_3x2_mixed_concave_side_edge = true;
    f.offset_closed_curve_trim_arc = true;
    f.offset_stitch_at_body_tolerance = true;
    f.curve_contacts_at_curve_ends = true;
    f.shell_empty_cavity_fails = true;
    f.fillet_refuse_ball_not_fitting = true;
    f.boolean_lists_virtual_surface_copies = true;
    f.sphere_small_circle_not_touching = true;
    f.ssi_tangent_contact_points = true;
    f.ssi_trace_tangent_curves_once = true;
    f.ssi_singular_start_inside_direction = true;
    f.found_torus_sphere_keep_tested_nurb = true;
    f.boolean_skips_faces_apart = true;
    f.uv_trim_checks_on_surface = true;
    f.loop_orientation_floor_from_loop_box = true;
    f.offset_extends_cones_as_themselves = true;
    f.local_operation_box_holds_new_surfaces = true;
    f.uv_trim_joins_drop_pieces = true;
    f.shell_edges_on_analytics_exact = true;
    f.ssi_no_reverse_of_empty_trace = true;
    f.ssi_pole_point_keeps_its_tangent = true;
    f.iso_curve_sliver_domain_is_degenerate = true;
    f.corner_blend_coons_patch_tangent_to_sides = true;
    f.fillet_patch_keeps_fillet_edges = true;
    f.fillet_corner_ball_center_from_axes = true;
    f.seam_point_solutions_are_one = true;
    f.boolean2d_plane_covers_far_faces = true;
    f.brep_area_between_solid_and_void = true;
    f.boolean_splits_definition_across_split = true;
    f.curve_tight_box_interval_knots = true;
    f.fillet_refuse_overlapping_bands = true;
    f.ssi_tangent_curve_tracer = true;
    f.torus_check_reads_v_interval = true;
    f.torus_check_spindle_apex = true;
    f.local_operation_step_walls = true;
    f.edge_check_orders_tangent_faces = true;
    f.revolve_arc_touching_axis_is_horn_torus = true;
    f.thicken_planar_side_walls = true;
    f.fillet_torus_span_from_start_section = true;
    f.fillet_knife_edge_rounds = true;
    f.taper_wall_closes_at_apex = true;
    f.revolution_round_offset_collapses = true;
    f.fillet_chamfer_cone_stored_as_cone = true;
    f.offset_point_vertex_caps = true;
    f.approximate_surface_bezier_starter = true;
    f.proto_edge_splits_at_captured_vertices = true;
    f.stitch_solid_nests_cavities = true;
    f.local_operation_closes_walls_to_nothing = true;
    f.linear_sweep_arc_to_cylinder_zero = true;
    f.imprint_reuses_coincident_face_edge = true;
    f.boolean_new_face_closure_both_sides = true;
    f.linear_sweep_arc_is_cylinder = true;
    f.offset_closes_loop_gaps = true;
    f.cylinder_plane_tangent_within_tolerance = true;
    f.boolean_wire_edge_keeps_faceuse_shell = true;
    f.fillet_nx1_tangent_corner_edge = true;
    f.fillet_torus_rail_on_axis = true;
    f.loops_go_on_round_the_vertex = true;
    f.revolve_splits_curves_on_axis = true;
    f.cover_wraps_across_closed_seam = true;
    f.imprint_extension_keeps_analytic_curve = true;
    f.fillet_trace_from_tangent_point_toward_next = true;
    f.fillet_tangent_point_one_sided_step = true;
    f.fillet_rails_meet_at_one_end_point = true;
    f.fillet_trace_reaches_rails = true;
    f.delete_vertex_joins_seam_iso_curve = true;
    f.box_face_failures_propagate = true;
    f.ellipse_adjust_step_interval_keeps_turns = true;
    f.fillet_cleanup_removes_input_slits = true;
    f.fillet_rail_end_on_continued_surface = true;
    f.glue_vertices_joins_every_region = true;
    f.fillet_nx1_tangent_side_edge_same_convexity = true;
    f.fillet_removal_takes_ring_fillets = true;
    // The fork's commits that reached `SMLIB_REV` at eaa998a (tangency-labs/smlib#68, #69, #70).
    f.fillet_trim_inside_by_best_dot = true;
    f.fillet_half_degree_crease = true;
    f.fillet_split_edge_keeps_near_half = true;
    f.curve_loop_normal_from_area = true;
    // The fork's commits in `SMLIB_REV` 2fc375f, ported to solidcore since.
    f.fillet_cliff_overflow = true;
    f.fillet_3x2_tangent_side_edge_roles = true;
    f.offset_extension_as_itself = true;
    f.fillet_mitre_concave_face_corners = true;
    f.fillet_mitre_gap_patch = true;
    f.fillet_run_on_past_far_end_only = true;
    f.good_drop_result_distances_in_order = true;
    f.face_tight_box_searches_trim = true;
    f.ssi_low_angle_start_on_probation = true;
    f.fillet_variable_span_follows_fillet = true;
    f.simple_coincidence_crossing_is_point = true;
    f.boolean_temporary_transform_statuses = true;
    f.primitive_path_curve_statuses = true;
    f.planar_path_sweep_statuses = true;
    f.planar_path_profile_placement_statuses = true;
    f.fillet_trim_status = true;
    f.perspective_sphere_offsets = true;
    f.plane_recognition_origin_after_axes = true;
    f
}

/// Opt-in fixes left off because the C++ fork (`SMLIB_REV`) does not behave as they do, so turning
/// them on would part this backend from the C++ one. Each was read in the fork's source at 2fc375f.
///
/// Upstream's defect is still there: `precise_properties_trim_base_v` (WR-563: `SmODEAreaIFEO` starts
/// at the domain's lowest v), `fillet_blend_curve_revolved_on_arc` (WR-581: `CalcTorusFilletGeom` takes
/// circular and linear sections only), `ellipse_step_interval_checks` (WR-211: `AdjustSTEPInterval`
/// tests no inverted or collapsed interval), `boolean_union_refuses_solid_leak` (WR-619: STEP 6 deletes
/// without testing regions), `curve_loop_start_after_previous_end` (WR-96), `loop_use_reports_open_gaps`
/// (WR-93), `face_points_span_domain` (WR-86), `drop_curve_v_domain_degeneracy` (WR-100),
/// `mirror_curve_keeps_euclidean_control_points` (WR-98), `geometric_g2_continuity` (WR-380: a normal
/// compared with itself), `fillet_corner_variable_radius_rail_on_opposite_fillet` (WR-645: no test of
/// the rail against the opposite fillet), `sphere_from_arcs_common_centre` (WR-103: `rOrigin` refers to
/// the origin it is compared with), `dupin_bottom_hyperbola_axis` (WR-69: k2 for both axes),
/// `extent3d_contains_both_segment_endpoints` (WR-3: `ContainsLineSeg3d` ors its two points),
/// `gap_samples_use_trimmed_spans` (WR-89: `BuildSampleSet` spans from the first knot),
/// `twist_simple_duw_sign` (WR-46: `EvaluateSimple`'s Duw y is -per cos) and
/// `translation_sweep_regularize` (solidcore#1937: nothing regularizes after the sweep).
///
/// The fork deals with it another way: `bspline_trim_keeps_rational_end_spans` (WR-575: the fork keeps
/// a short span of a UV curve, and still removes a 3D conic's) and `corner_patch_face_side_twist`
/// (WR-613: the fork refines the patch, `corner_blend_coons_patch_tangent_to_sides`).
///
/// The fork carries on softly where these refuse: `temporary_radius_setup_errors` (#458: a scope guard
/// cannot report), `fillet_corner_statuses` (`sm_TestG1Continuity`: no tangent is no tangency),
/// `fillet_edge_curve_domain_statuses` (`sm_CheckEdgeCurveDomains`: a failed trim keeps the edge's
/// curve), `offset_tangent_gap_statuses` (`HaveTangentGaps`: a failed sample shows no gap) and
/// `ssi_alignment_statuses` (`ALIGN_DIR_WITH_PREVIOUS_POINT`: no angle, no alignment).
///
/// On since `SMLIB_REV` eaa998a: `fillet_trim_inside_by_best_dot` (fork de4dae6),
/// `fillet_half_degree_crease` (2e8758b), `fillet_split_edge_keeps_near_half` (ceb3272) and
/// `curve_loop_normal_from_area` (be45e8d).
///
/// `geometric_g2_continuity`: no fork commit; the C++ still has the defect at SmGeomUtility.cpp:5822
/// (crashes/2992: 183 faces against 831).
///
/// Upstream's defect is still in the fork's source at `SMLIB_REV` 2fc375f:
/// `fillet_corner_variable_radius_rail_on_opposite_fillet` (WR-645: `MakeVariableRadiusFillet` has
/// only the concave corner's rail-rail test before resetting the edges, no test of the rail against
/// the opposite fillet), `sphere_from_arcs_common_centre` (WR-103: `CreateSphereFromArcs` compares
/// `sRefFrame`'s origin with `rOrigin`, a reference to it), `dupin_bottom_hyperbola_axis` (WR-69:
/// `MakeDupin`'s bottom hyperbola takes k2 for both axes) and
/// `extent3d_contains_both_segment_endpoints` (WR-3: `ContainsLineSeg3d` ors its two points).
///
/// The fork handles a failed status softly where these flags refuse (solidcore#1775, #483), so the
/// flags would fail where the C++ kernel carries on: `fillet_corner_statuses` (`sm_TestG1Continuity`:
/// no tangent is no tangency), `fillet_edge_curve_domain_statuses` (`sm_CheckEdgeCurveDomains`: a
/// failed trim keeps the edge's curve), `offset_tangent_gap_statuses` (`HaveTangentGaps`: a failed
/// sample shows no gap) and `ssi_alignment_statuses` (`ALIGN_DIR_WITH_PREVIOUS_POINT`: no angle, no
/// alignment). `translation_sweep_regularize` (solidcore#1937) has no fork commit, and the fork keeps
/// upstream's `gap_samples_use_trimmed_spans` (WR-89: `BuildSampleSet` spans from the first knot) and
/// `twist_simple_duw_sign` (WR-46: `EvaluateSimple`'s Duw y is -per cos).
#[cfg(test)]
const HELD_BACK: &[&str] = &[
    "precise_properties_trim_base_v",
    "bspline_trim_keeps_rational_end_spans",
    "fillet_blend_curve_revolved_on_arc",
    "temporary_radius_setup_errors",
    "ellipse_step_interval_checks",
    "corner_patch_face_side_twist",
    "boolean_union_refuses_solid_leak",
    "curve_loop_start_after_previous_end",
    "loop_use_reports_open_gaps",
    "face_points_span_domain",
    "drop_curve_v_domain_degeneracy",
    "mirror_curve_keeps_euclidean_control_points",
    "geometric_g2_continuity",
    "fillet_corner_variable_radius_rail_on_opposite_fillet",
    "sphere_from_arcs_common_centre",
    "dupin_bottom_hyperbola_axis",
    "extent3d_contains_both_segment_endpoints",
    "fillet_corner_statuses",
    "fillet_edge_curve_domain_statuses",
    "offset_tangent_gap_statuses",
    "ssi_alignment_statuses",
    "translation_sweep_regularize",
    "gap_samples_use_trimmed_spans",
    "twist_simple_duw_sign",
];

fn fresh() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = fixes();
    k
}

static KERNEL: Mutex<Option<Kernel>> = Mutex::new(None);

/// Runs `f` on the process's kernel. A panic in solidcore is caught: the kernel, which it may have
/// left half-updated, is replaced by a fresh one and the call fails with [`Error::Internal`].
pub(crate) fn run<T>(call: &'static str, f: impl FnOnce(&mut Kernel) -> std::result::Result<T, solidcore::SolidError>) -> Result<T> {
    let mut guard = KERNEL.lock().unwrap_or_else(PoisonError::into_inner);
    let k = guard.get_or_insert_with(fresh);
    match catch_unwind(AssertUnwindSafe(|| f(k))) {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(Error::Solidcore(format!("{call}: {e}"))),
        Err(p) => {
            *guard = Some(fresh());
            let why = p.downcast_ref::<&str>().map(|s| (*s).to_owned()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default();
            log::warn!(target: "smlib", "solidcore panicked in {call}: {why}");
            Err(Error::Internal(format!("{call}: {why}")))
        }
    }
}

/// A call the capi checks for `SM_SUCCESS`, treating any other status as no answer: an error
/// that stands for an SMLib status exactly ([`SolidError::sm_status`](solidcore::SolidError::sm_status),
/// solidcore#1452) is `None`. Every other solidcore error
/// (unported code, invalid input to an adapter, a missing record...) is not a status the C++
/// path returns, so it fails the call instead of hiding as a skipped entity.
pub(crate) fn answer<T>(r: std::result::Result<T, solidcore::SolidError>) -> std::result::Result<Option<T>, solidcore::SolidError> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.sm_status().is_some() => Ok(None),
        Err(e) => Err(e),
    }
}

/// Runs `f` on the kernel and `body`, locking the kernel first.
pub(crate) fn with_body<T>(
    call: &'static str,
    body: &Body,
    f: impl FnOnce(&mut Kernel, &mut solidcore::topology::Brep) -> std::result::Result<T, solidcore::SolidError>,
) -> Result<T> {
    // A body a panic interrupted may be half-edited: it is never used again.
    if body.0.is_poisoned() {
        return Err(Error::Internal(format!("{call}: the body was left unfinished by an earlier panic")));
    }
    run(call, |k| {
        let mut b = body.0.lock().unwrap_or_else(PoisonError::into_inner);
        f(k, &mut b)
    })
}

/// An axis-aligned box from its lower corner (`SmApiCreateBox`, as `smlib_make_box` checks it:
/// every edge length positive).
pub(crate) fn make_box(origin: [f64; 3], size: [f64; 3]) -> Result<Body> {
    if !origin.iter().chain(&size).all(|x| x.is_finite()) || !size.iter().all(|&s| s > 0.0) {
        return Err(Error::Argument);
    }
    let [x, y, z] = size;
    run("create_box", |k| primitives::create_box(k, Vec3::new(origin[0], origin[1], origin[2]), x, y, z)).map(Body::new)
}

fn point(p: [f64; 3]) -> Result<Vec3> {
    if !p.iter().all(|x| x.is_finite()) {
        return Err(Error::Argument);
    }
    Ok(Vec3::new(p[0], p[1], p[2]))
}

/// A cylinder about +Z from its base centre (`SmApiCreateCylinder`, as `smlib_make_cylinder`
/// checks it: radius and height positive).
pub(crate) fn make_cylinder(origin: [f64; 3], radius: f64, height: f64) -> Result<Body> {
    let o = point(origin)?;
    if !(radius > 0.0 && height > 0.0 && radius.is_finite() && height.is_finite()) {
        return Err(Error::Argument);
    }
    run("create_cylinder", |k| primitives::create_cylinder(k, o, radius, height)).map(Body::new)
}

/// A sphere (`SmApiCreateSphere`; radius positive).
pub(crate) fn make_sphere(center: [f64; 3], radius: f64) -> Result<Body> {
    let o = point(center)?;
    if !(radius > 0.0 && radius.is_finite()) {
        return Err(Error::Argument);
    }
    run("create_sphere", |k| primitives::create_sphere(k, o, radius)).map(Body::new)
}

/// A cone or frustum about +Z (`SmApiCreateCone`; `smlib_make_cone`'s checks: base radius and
/// height positive, top radius at least 0 and not the base radius).
pub(crate) fn make_cone(origin: [f64; 3], base_radius: f64, top_radius: f64, height: f64) -> Result<Body> {
    let o = point(origin)?;
    let ok = base_radius > 0.0 && top_radius >= 0.0 && height > 0.0 && top_radius != base_radius;
    if !(ok && base_radius.is_finite() && top_radius.is_finite() && height.is_finite()) {
        return Err(Error::Argument);
    }
    run("create_cone", |k| primitives::create_cone(k, o, base_radius, top_radius, height)).map(Body::new)
}

/// A ring torus about +Z (`SmApiCreateTorus`; major radius > minor radius > 0).
pub(crate) fn make_torus(center: [f64; 3], major_radius: f64, minor_radius: f64) -> Result<Body> {
    let o = point(center)?;
    if !(minor_radius > 0.0 && minor_radius < major_radius && major_radius.is_finite()) {
        return Err(Error::Argument);
    }
    run("create_torus", |k| primitives::create_torus(k, o, major_radius, minor_radius)).map(Body::new)
}

/// The enclosed volume (`SmApiBrepComputeVolume`).
pub(crate) fn volume(body: &Body, relative_accuracy: f64) -> Result<f64> {
    if !relative_accuracy.is_finite() {
        return Err(Error::Argument);
    }
    with_body("brep_volume", body, |k, b| {
        let store = AttributeStore::default();
        let attrs = PropertyAttributes { store: &store, key_of: &|_| ObjectKey(0) };
        brep_volume(k, b, relative_accuracy, &attrs)
    })
}

/// Face, edge and vertex counts.
pub(crate) fn counts(body: &Body) -> Result<[usize; 3]> {
    with_body("counts", body, |_, b| Ok([b.face_list().len(), b.edge_list().len(), b.vertex_list().len()]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_upstream_fix_is_on() {
        let f = format!("{:?}", fixes());
        let off: Vec<&str> =
            f.split([',', '{', '}']).filter_map(|kv| kv.trim().strip_suffix(": false")).filter(|name| !HELD_BACK.contains(name)).collect();
        assert!(off.is_empty(), "UpstreamFixes flags are off: {off:?}");
        for name in HELD_BACK {
            assert!(f.contains(&format!("{name}: false")), "{name} is on or gone: drop it from HELD_BACK");
        }
    }

    #[test]
    fn a_box_measures_exactly() {
        let b = make_box([1.0, -2.0, 3.0], [10.0, 20.0, 30.0]).unwrap();
        assert_eq!(counts(&b).unwrap(), [6, 12, 8]);
        let v = volume(&b, 1e-8).unwrap();
        assert!((v - 6000.0).abs() < 6000.0 * 1e-9, "{v}");
    }

    #[test]
    fn hostile_numbers_are_refused() {
        assert!(make_box([f64::NAN, 0.0, 0.0], [1.0; 3]).is_err());
        assert!(make_box([0.0; 3], [1.0, -1.0, 1.0]).is_err());
        let b = make_box([0.0; 3], [1.0; 3]).unwrap();
        assert!(volume(&b, f64::INFINITY).is_err());
    }

    #[test]
    fn a_panic_is_an_error_and_the_kernel_is_fresh_after() {
        let r: Result<()> = run("test", |_| panic!("boom"));
        assert!(matches!(r, Err(Error::Internal(ref m)) if m.contains("boom")), "{r:?}");
        assert!(make_box([0.0; 3], [1.0; 3]).is_ok());
    }
}
