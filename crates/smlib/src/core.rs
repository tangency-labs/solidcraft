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
use solidcore::kernel::{CancelToken, Kernel, KernelOptions, Rng, UpstreamFixes};
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
pub(crate) mod definition;
pub(crate) mod dump;
pub(crate) mod file;
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
pub(crate) mod picked;
pub(crate) mod split;
#[cfg(test)]
mod split_tests;
pub(crate) mod ssi;
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
pub(crate) use measure::{
    area, bounds, check, check_with, classify_point, classify_points, edge_face_distance, is_manifold_solid, mass_properties, solid_count,
};

/// A solidcore body. Its own lock: solidcore's operations take `&mut Brep`, even to measure.
pub(crate) struct Body(pub(crate) Mutex<solidcore::topology::Brep>);

impl Body {
    pub(crate) fn new(b: solidcore::topology::Brep) -> Body {
        Body(Mutex::new(b))
    }
}

/// The fixes this backend runs with: every one of solidcore's opt-in fixes of upstream SMLib
/// defects (`UpstreamFixes`, each naming its record and issue), whether or not the C++ fork has it,
/// unless it appears broken. A test checks none is left off but those in `HELD_BACK`, so moving the
/// solidcore pin to a rev with a new flag fails until it is added here.
pub(crate) fn fixes() -> UpstreamFixes {
    let mut f = UpstreamFixes::default();
    f.fillet_refuse_rails_leaving_faces = true;
    f.fillet_refuse_face_consuming_blends = true;
    f.fillet_refuse_non_manifold_result = true;
    f.fillet_vertex_error_diagnostics = true;
    f.fillet_corner_ball_patch = true;
    f.fillet_chamfer_cone_on_arc = true;
    f.offset_excess_faces_after_merge = true;
    f.offset_clip_right_side_out = true;
    f.offset_extended_edges_mated = true;
    f.keep_operand_uv_trims = true;
    f.proto_vertex_miss_is_false = true;
    f.check_loop_vertex_tolerance = true;
    f.check_loop_projected_seam_ends = true;
    f.oscillating_skips_failed_guesses = true;
    f.refine_cc_range_ends = true;
    f.flat_corners_test_bits = true;
    f.cylinder_rebuild_mean_radius = true;
    f.revolve_analytic_profile_faces = true;
    f.taper_tangent_corner_planes = true;
    f.revolve_nested_shells_as_cavities = true;
    f.revolve_holed_profile_void = true;
    f.join_touching_loops = true;
    f.offset_resolve_crossing_loops = true;
    f.offset_walls_touch_refused = true;
    f.offset_analytic_mate_edges = true;
    f.nurb_cylinder_inside_out_frame = true;
    f.local_operation_refuses_loose_tangent_edges = true;
    f.torus_major_radius_from_origin = true;
    f.fillet_nx1_opposite_continuation_side_edge = true;
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
    f.proto_edge_self_loop_on_vertex_degenerate = true;
    f.slice_coincident_faces_sheet_side = true;
    f.boolean_outer_shell_first = true;
    f.cover_refuses_folded_continuation = true;
    f.boolean_vertex_by_best_crossing = true;
    f.boolean_cleanup_removes_slits = true;
    f.classify_edge_interval = true;
    f.ellipse_trim_seam_lines_span_cone_height = true;
    f.degenerate_proto_edge_in_one_vertex = true;
    f.new_edge_vertex_tolerance_covers_gap = true;
    f.torus_signed_gen_curve_interval = true;
    f.point_solve_keeps_triangle_guess = true;
    f.imprint_extends_edge_to_proto_vertex = true;
    f.fillet_point_step_per_point = true;
    f.analytic_copy_keeps_parameterization = true;
    f.offset_extension_from_base = true;
    f.fillet_cleanup_keeps_target_faces = true;
    f.fillet_nx1_tangent_side_edge = true;
    f.fillet_3x2_mixed_concave_side_edge = true;
    f.offset_stitch_at_body_tolerance = true;
    f.curve_contacts_at_curve_ends = true;
    f.shell_empty_cavity_fails = true;
    f.ssi_tangent_contact_points = true;
    f.ssi_trace_tangent_curves_once = true;
    f.ssi_singular_start_inside_direction = true;
    f.found_torus_sphere_keep_tested_nurb = true;
    f.boolean_skips_faces_apart = true;
    f.local_operation_box_holds_new_surfaces = true;
    f.shell_edges_on_analytics_exact = true;
    f.ssi_pole_point_keeps_its_tangent = true;
    f.fillet_patch_keeps_fillet_edges = true;
    f.fillet_corner_ball_center_from_axes = true;
    f.boolean2d_plane_covers_far_faces = true;
    f.boolean_splits_definition_across_split = true;
    f.fillet_refuse_overlapping_bands = true;
    f.ssi_tangent_curve_tracer = true;
    f.torus_check_spindle_apex = true;
    f.local_operation_step_walls = true;
    f.edge_check_orders_tangent_faces = true;
    f.thicken_planar_side_walls = true;
    f.fillet_knife_edge_rounds = true;
    f.revolution_round_offset_collapses = true;
    f.fillet_chamfer_cone_stored_as_cone = true;
    f.offset_point_vertex_caps = true;
    f.proto_edge_splits_at_captured_vertices = true;
    f.local_operation_closes_walls_to_nothing = true;
    f.imprint_reuses_coincident_face_edge = true;
    f.boolean_new_face_closure_both_sides = true;
    f.linear_sweep_arc_is_cylinder = true;
    f.fillet_nx1_tangent_corner_edge = true;
    f.loops_go_on_round_the_vertex = true;
    f.revolve_splits_curves_on_axis = true;
    f.cover_wraps_across_closed_seam = true;
    f.fillet_rails_meet_at_one_end_point = true;
    f.fillet_trace_reaches_rails = true;
    f.delete_vertex_joins_seam_iso_curve = true;
    f.box_face_failures_propagate = true;
    f.fillet_rail_end_on_continued_surface = true;
    f.glue_vertices_joins_every_region = true;
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
    f.planar_path_sweep_statuses = true;
    f.planar_path_profile_placement_statuses = true;
    f.fillet_trim_status = true;
    f.plane_recognition_origin_after_axes = true;
    f.brep_edit_statuses_pass_on = true;
    f.fillet_run_on_bspline_trims = true;
    f.query_statuses_pass_on = true;
    f.sweep_statuses_pass_on = true;
    f.ssi_proto_topology_statuses_pass_on = true;
    f.analytic_inversion_answers_apart = true;
    f.fillet_statuses_pass_on = true;
    f.fillet_corner_per_vertex_fan = true;
    f.fillet_rail_trim_at_near_knot = true;
    f.offset_keep_cavity_regions = true;
    f.temporary_radius_setup_errors = true;
    f.ellipse_step_interval_checks = true;
    f.corner_patch_face_side_twist = true;
    f.boolean_union_refuses_solid_leak = true;
    f.curve_loop_start_after_previous_end = true;
    f.loop_use_reports_open_gaps = true;
    f.face_points_span_domain = true;
    f.drop_curve_v_domain_degeneracy = true;
    f.mirror_curve_keeps_euclidean_control_points = true;
    f.fillet_corner_variable_radius_rail_on_opposite_fillet = true;
    f.dupin_bottom_hyperbola_axis = true;
    f.extent3d_contains_both_segment_endpoints = true;
    f.twist_simple_duw_sign = true;
    f.offset_swap_uv_extended_distance = true;
    f.validation_does_not_make_uv_trims = true;
    f.analytic_corner_failure_is_false = true;
    f.proto_edge_split_upper_keeps_the_old_max = true;
    f.point_on_start_point_squared_tolerance = true;
    f.bspline_trim_keeps_rational_end_spans = true;
    f.fillet_exact_round_classes = true;
    f.check_seam_crossing_unclosed = true;
    f.ascii_doubles_round_trip = true;
    f.offset_edges_keep_source_tolerance = true;
    f.surface_continuity_checks_last_v_knot = true;
    f.volume_lift_third_derivative_terms = true;
    f.plane_reparameterize_moves_origin = true;
    f.curve_bounded_surface_read_uv_type_and_implicit_outer = true;
    f.volume_representational_end_continuities = true;
    f.offset_open_edge_second_trim_point = true;
    f.close_loop_relates_only_coincident_edges = true;
    f.ssi_split_curves_at_singular_points = true;
    f.surface_parallel_to_vector_checks = true;
    f.fillet_round_ends_at_tangent_continuation = true;
    f.geometric_g2_continuity = true;
    f.degen_param_tiebreak_uses_actual_param = true;
    f.local_surface_intersect_guards_max_boundary_retry = true;
    f.curve_sweep_closed_path_joins_ends = true;
    f.validate_tolerances_fixes_reported_gaps = true;
    f.edgeuse_convexity_propagates_step_off_failure = true;
    f.database_io_byte_swaps_read_boolean_and_u16 = true;
    f.cone_create_canonical_preserves_inside_out_and_checks_height = true;
    f.point_class_gap_3d_propagates_no_drop_error = true;
    f.offset_updates_result_tolerances = true;
    f.cone_iso_circle_at_axial_height = true;
    f.fillet_overflow_trim_at_creases = true;
    f.fillet_section_end_trim_at_creases = true;
    f.fillet_refuse_mixed_corner_crossing_rails = true;
    // Held back: see `HELD_BACK`.
    f.fillet_refuse_rounds_breaking_through = false;
    f.cone_unbounded_parameterized_as_nurb = true;
    f.fillet_round_as_definition = true;
    f.fillet_overflow_crease_along_edge = true;
    f.keep_exact_pcurves = true;
    f.planar_section_face_tolerance = true;
    f.face_swap_uv_transposes_trims = true;
    f.closed_smooth_pole_tangent_limit = true;
    // Held back: see `HELD_BACK`.
    f.fillet_round_ends_on_outline = false;
    f.fillet_setback_beside_curved_faces = true;
    f.fillet_chamfer_corner = true;
    f.brep_ray_classifies_current_trims = true;
    f
}

/// The fixes held off, each with the case and the numbers that show it broken: a volume or topology
/// wrong against the analytic value, a check failure, a crash, sound geometry refused that builds
/// without the flag, or a large slowdown. Differing from the C++ kernel is no such evidence: a fix
/// the fork doesn't have stays on, and where solidcore builds what SMLib refuses, a result right
/// against its closed form is a win.
///
/// `fillet_refuse_rounds_breaking_through` (WR-707, solidcore#2311): sound geometry refused. At
/// solidcore 9856ffee5, with it on, the fillet rig's 1668-fillet-both-sides-of-thin-wall-r7 is
/// refused as breaking through the wall into the hollow behind it ("the largest radius that fits is
/// 6.828 mm"), where with it off it builds the closed form, 39509.938 (at 8ddce9016 it refused 11
/// rig cases that pass with it off; at this pin the other 270 fillet rig cases come out as with it
/// off).
///
/// `fillet_round_ends_on_outline` (WR-730, solidcore#2541): volumes wrong against their closed
/// forms, and rounds too big for their faces built instead of refused. With it on, each of these
/// passes with it off: `fillet_over_a_cliff` measures 721.832 against the closed form's 693.205;
/// `a_round_wider_than_its_faces_overflows_onto_its_tangent_sides` 6125.719 against 6107.529;
/// `taper_follows_the_room_along_the_edge` 4210.926 against 4195.579; the round larger than the
/// cube in `hostile_inputs_are_errors` builds (4773.168 against the 1094.853 its refusal leaves),
/// as do the fillets the CLI's `run_strict` and `json_out` tests need to fail; and
/// `a_corner_round_wider_than_its_faces_overflows_and_takes_the_corners_other_edges`,
/// `overflow_goes_first`, `corners_refuse_what_they_cannot_build` and
/// `fillet_edge_removed_by_a_larger_overflowing_round_is_noted` are refused (too large, no room,
/// status 1001), where with it off they build. Measured again at solidcore 9856ffee5: the same.
#[cfg(test)]
const HELD_BACK: &[&str] = &["fillet_refuse_rounds_breaking_through", "fillet_round_ends_on_outline"];

/// A new kernel, with this backend's fixes.
fn fresh() -> Kernel {
    let mut k = Kernel::new(&KernelOptions::default());
    k.fixes = fixes();
    k
}

/// The kernels calls run on. The lock is only held to take or put back a kernel, never during a
/// call: a call that runs long (or never returns) holds up nothing but its own body, so one
/// design's long operation never delays another's. Calls one after another all get the same
/// kernel; one that starts while another runs gets one of its own (solidcore keeps no global
/// state, and a body's identities are valid only in the kernel that issued them, so any kernel
/// takes any body).
///
/// A call stuck for good keeps its kernel and the thread it runs on, and costs nothing more: at
/// most `most_idle` kernels wait between calls, however many calls once ran side by side. A job
/// that starts again and sticks again holds one more each time, so at `most_running` calls a new
/// call fails, saying so, rather than grow without end or wait for calls that may never return.
pub(crate) struct Pool<K> {
    pub(crate) idle: Vec<K>,
    pub(crate) running: usize,
    pub(crate) most_idle: usize,
    pub(crate) most_running: usize,
}

impl<K> Pool<K> {
    /// A kernel for a call, the last used first; `None` when too many are running.
    pub(crate) fn take(&mut self, make: impl FnOnce() -> K) -> Option<K> {
        if self.running >= self.most_running {
            return None;
        }
        self.running += 1;
        Some(self.idle.pop().unwrap_or_else(make))
    }

    /// A call ended, handing back its kernel if it is fit to use again.
    pub(crate) fn put(&mut self, kernel: Option<K>) {
        self.running = self.running.saturating_sub(1);
        if let Some(kernel) = kernel
            && self.idle.len() < self.most_idle
        {
            self.idle.push(kernel);
        }
    }
}

/// Most calls at once. Far above what any work needs: an operation runs on at most 16 threads.
const MOST_RUNNING: usize = 256;

static POOL: Mutex<Pool<Kernel>> = Mutex::new(Pool { idle: Vec::new(), running: 0, most_idle: 8, most_running: MOST_RUNNING });

fn take_kernel() -> Option<Kernel> {
    POOL.lock().unwrap_or_else(PoisonError::into_inner).take(fresh)
}

fn put_kernel(k: Option<Kernel>) {
    POOL.lock().unwrap_or_else(PoisonError::into_inner).put(k);
}

/// What a call must not find from the calls before it: the same operation on the same bodies
/// answers the same on any kernel, however much that kernel did first. (Its draws of random rays,
/// the stations it remembers; marks, identities and what is only kept to reuse memory change no
/// answer. Tolerances and fixes are the same on every kernel.)
fn start_clean(k: &mut Kernel) {
    k.rng = Rng::new(KernelOptions::default().seed);
    *k.blend_station_memo.borrow_mut() = Default::default();
    k.fillet_point_step.set(None);
    k.upstream_ub_passed.clear();
}

/// A kernel kept out of use until dropped ([`crate::hold_a_kernel`]).
pub struct HeldKernel(Option<Kernel>);

impl Drop for HeldKernel {
    fn drop(&mut self) {
        put_kernel(self.0.take());
    }
}

pub(crate) fn hold_a_kernel() -> HeldKernel {
    HeldKernel(take_kernel())
}

/// Runs `f` on a kernel of its own. A panic in solidcore is caught: the kernel, which it may have
/// left half-updated, is dropped and the call fails with [`Error::Internal`]. A token over this
/// thread's work ([`crate::stop`]) stops it.
pub(crate) fn run<T: dump::Answer>(call: &'static str, f: impl FnOnce(&mut Kernel) -> std::result::Result<T, solidcore::SolidError>) -> Result<T> {
    run_on(call, &[], f)
}

thread_local! {
    /// How many solidcore calls this thread is inside: a panic now is caught ([`run_on`]).
    static IN_CALL: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Is this thread inside a solidcore call, whose panic [`run_on`] catches? (Crash reports mark such
/// a panic recovered.)
pub(crate) fn in_call() -> bool {
    IN_CALL.with(std::cell::Cell::get) > 0
}

/// [`run`] for a call that reads the bodies `inputs`: should it fail while a dump folder is set,
/// they are left there ([`dump::failed`]).
pub(crate) fn run_on<T: dump::Answer>(
    call: &'static str,
    inputs: dump::Inputs<'_>,
    f: impl FnOnce(&mut Kernel) -> std::result::Result<T, solidcore::SolidError>,
) -> Result<T> {
    let stop = crate::stop::token();
    if stop.as_ref().is_some_and(CancelToken::is_cancelled) {
        return Err(Error::Cancelled(call));
    }
    call_on(call, inputs, stop, f)
}

/// [`run_on`], stopped by `stop`.
fn call_on<T: dump::Answer>(
    call: &'static str,
    inputs: dump::Inputs<'_>,
    stop: Option<CancelToken>,
    f: impl FnOnce(&mut Kernel) -> std::result::Result<T, solidcore::SolidError>,
) -> Result<T> {
    let Some(mut k) = take_kernel() else {
        log::warn!(target: "smlib", "{call}: refused, {MOST_RUNNING} kernel calls are still running");
        return Err(Error::Solidcore(format!("{call}: {MOST_RUNNING} kernel calls are still running")));
    };
    start_clean(&mut k);
    k.cancel = stop.clone();
    // A profiler span per call: where the kernel's time goes.
    let _span = solidcraft_profile::span("solidcore", call);
    let in_flight = dump::in_flight(call);
    struct Depth;
    impl Drop for Depth {
        fn drop(&mut self) {
            IN_CALL.with(|d| d.set(d.get().saturating_sub(1)));
        }
    }
    IN_CALL.with(|d| d.set(d.get() + 1));
    let depth = Depth;
    let caught = catch_unwind(AssertUnwindSafe(|| f(&mut k)));
    drop(depth);
    let r = match caught {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(Error::Solidcore(format!("{call}: {e}"))),
        Err(p) => {
            k = fresh();
            let why = p.downcast_ref::<&str>().map(|s| (*s).to_owned()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default();
            log::warn!(target: "smlib", "solidcore panicked in {call}: {why}");
            Err(Error::Internal(format!("{call}: {why}")))
        }
    };
    drop(in_flight);
    // Stopped, it is no failure to look into, and solidcore may have left the kernel part way.
    // No result either, even one solidcore finished first: the caller never has to ask whether
    // it came in time (as solidcore-ops' session answers).
    if stop.is_some_and(|s| s.is_cancelled()) {
        put_kernel(None);
        return match r {
            Err(Error::Internal(e)) => Err(Error::Internal(e)),
            _ => Err(Error::Cancelled(call)),
        };
    }
    let why = match &r {
        Ok(v) => v.refusal(),
        Err(e) => Some(e.to_string()),
    };
    if let Some(why) = why {
        // The kernel the call ran on, or the fresh one that replaced it after a panic.
        dump::failed(&k, call, &why, inputs);
    }
    k.cancel = None;
    put_kernel(Some(k));
    r
}

/// A call the capi checks for `SM_SUCCESS`, treating any other status as no answer: an error
/// that stands for an SMLib status exactly ([`SolidError::sm_status`](solidcore::SolidError::sm_status),
/// solidcore#1452) is `None`. Every other solidcore error
/// (unported code, invalid input to an adapter, a missing record...) is not a status the C++
/// path returns, so it fails the call instead of hiding as a skipped entity. So does a status
/// error whose cause chain holds a solidcore-side failure or a cancellation
/// ([`SolidError::is_status_answer`](solidcore::SolidError::is_status_answer)), the rule of
/// solidcore-ops' `session::answer`.
pub(crate) fn answer<T>(r: std::result::Result<T, solidcore::SolidError>) -> std::result::Result<Option<T>, solidcore::SolidError> {
    match r {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.is_status_answer() => Ok(None),
        Err(e) => Err(e),
    }
}

/// Runs `f` on a kernel and `body`, its lock held for the call.
pub(crate) fn with_body<T: dump::Answer>(
    call: &'static str,
    body: &Body,
    f: impl FnOnce(&mut Kernel, &mut solidcore::topology::Brep) -> std::result::Result<T, solidcore::SolidError>,
) -> Result<T> {
    // A body a panic interrupted may be half-edited: it is never used again.
    if body.0.is_poisoned() {
        return Err(Error::Internal(format!("{call}: the body was left unfinished by an earlier panic")));
    }
    // No token stops it: stopped part way, solidcore may leave the body it borrows half-changed.
    call_on(call, &[("brep", body)], None, |k| f(k, &mut body.0.lock().unwrap_or_else(PoisonError::into_inner)))
}

/// [`with_body`] on a body only this call reads (one the caller consumes): the token over this
/// thread's work stops it, since what a stopped call leaves half-changed is never read again.
pub(crate) fn with_own_body<T: dump::Answer>(
    call: &'static str,
    body: &Body,
    f: impl FnOnce(&mut Kernel, &mut solidcore::topology::Brep) -> std::result::Result<T, solidcore::SolidError>,
) -> Result<T> {
    if body.0.is_poisoned() {
        return Err(Error::Internal(format!("{call}: the body was left unfinished by an earlier panic")));
    }
    let stop = crate::stop::token();
    if stop.as_ref().is_some_and(CancelToken::is_cancelled) {
        return Err(Error::Cancelled(call));
    }
    call_on(call, &[("brep", body)], stop, |k| f(k, &mut body.0.lock().unwrap_or_else(PoisonError::into_inner)))
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

/// The enclosed volume (`SmApiBrepComputeVolume`), its accuracy clamped as [`mass_properties`]
/// clamps it.
pub(crate) fn volume(body: &Body, relative_accuracy: f64) -> Result<f64> {
    let relative_accuracy = measure::accuracy(relative_accuracy)?;
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
    fn every_upstream_fix_is_on_unless_shown_broken() {
        let f = fixes();
        let flags: Vec<(&str, bool)> = f.flags().collect();
        let off: Vec<&str> = flags.iter().filter(|(name, on)| !on && !HELD_BACK.contains(name)).map(|(name, _)| *name).collect();
        assert!(off.is_empty(), "UpstreamFixes flags are off: {off:?}");
        for name in HELD_BACK {
            assert!(flags.contains(&(*name, false)), "{name} is on or gone: drop it from HELD_BACK");
        }
    }

    #[test]
    fn a_box_measures_exactly() {
        let b = make_box([1.0, -2.0, 3.0], [10.0, 20.0, 30.0]).unwrap();
        assert_eq!(counts(&b).unwrap(), [6, 12, 8]);
        let v = volume(&b, 1e-8).unwrap();
        assert!((v - 6000.0).abs() < 6000.0 * 1e-9, "{v}");
    }

    /// An accuracy finer than 1e-8 or not positive is 1e-8, as `mass_properties` takes it: a
    /// sphere's volume at 0, -1 and 1e-15 is its volume at 1e-8, on its closed form.
    #[test]
    fn a_volume_accuracy_out_of_range_is_clamped() {
        let ball = make_sphere([1.0, 2.0, 3.0], 5.0).unwrap();
        let at = volume(&ball, 1e-8).unwrap();
        assert!((at - 4.0 / 3.0 * std::f64::consts::PI * 125.0).abs() < 1e-6, "{at}");
        for acc in [0.0, -1.0, 1e-15] {
            assert_eq!(volume(&ball, acc).ok(), Some(at), "accuracy {acc}");
        }
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
