//! Regression test for tangency-labs/solidcore#1945 on SolidCraft's own input: the B-rep SolidCraft
//! hands its fillet session at step 4 of `fillets/1207` (`data/issue_1945_blend_1207_step4.smb`),
//! with SolidCraft's flag set, rounded r2 along the bar's top front edge and the first round's
//! section curve. Repro, flag set and root cause by tdamsma on #1945: `SmLocalSolveNd::SolveIt`
//! kept the unscaled point's Jacobian for a clamped-and-scaled step (fixed in the fork's
//! tangency-labs/smlib@d2757e1, ported under WR-253's `fixes.oscillating_skips_failed_guesses`), so
//! a near-tangent SSI's end landed 1.6e-8 off and the rail's UV trim kept a 3.1e-8 span. The fork's
//! C++ (68fb9162) builds 11 faces / 25 edges / 16 vertices, V 49712.42808748303, check clean. Its
//! rounds' sections are upstream's approximated arcs; here they are the ball's exact circles (WR-708,
//! as the fork's own from 523bc25 on), which keep 8.1e-5 more material: V 49712.42816838713.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    missing_docs
)]
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
use solidcore::topology::api_files::read_brep_from_file;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p3::topology_ref;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{edge, Brep, EdgeId};
use solidcore::{Kernel, Point3};

/// The body's volume with its rounds' sections the ball's exact circles (see the header).
const EXACT_CIRCLES_VOLUME: f64 = 49712.42816838713;

fn sc_fixes(k: &mut Kernel) {
    k.fixes.fillet_trim_status = true;
    k.fixes.curve_loop_normal_from_area = true;
    k.fixes.fillet_refuse_rails_leaving_faces = true;
    k.fixes.fillet_refuse_face_consuming_blends = true;
    k.fixes.fillet_refuse_overlapping_bands = true;
    k.fixes.fillet_vertex_error_diagnostics = true;
    k.fixes.fillet_chamfer_cone_on_arc = true;
    k.fixes.fillet_chamfer_cone_stored_as_cone = true;
    k.fixes.corner_patch_face_side_twist = true;
    k.fixes.fillet_patch_keeps_fillet_edges = true;
    k.fixes.fillet_trim_inside_by_best_dot = true;
    k.fixes.fillet_knife_edge_rounds = true;
    k.fixes.fillet_half_degree_crease = true;
    k.fixes.offset_excess_faces_after_merge = true;
    k.fixes.offset_point_vertex_caps = true;
    k.fixes.offset_clip_right_side_out = true;
    k.fixes.offset_extended_edges_mated = true;
    k.fixes.thicken_planar_side_walls = true;
    k.fixes.keep_operand_uv_trims = true;
    k.fixes.geometric_g2_continuity = true;
    k.fixes.proto_vertex_miss_is_false = true;
    k.fixes.check_loop_vertex_tolerance = true;
    k.fixes.check_loop_projected_seam_ends = true;
    k.fixes.edge_check_orders_tangent_faces = true;
    k.fixes.ellipse_step_interval_checks = true;
    k.fixes.oscillating_skips_failed_guesses = true;
    k.fixes.refine_cc_range_ends = true;
    k.fixes.flat_corners_test_bits = true;
    k.fixes.good_drop_result_distances_in_order = true;
    k.fixes.torus_check_spindle_apex = true;
    k.fixes.cylinder_rebuild_mean_radius = true;
    k.fixes.nurb_cylinder_inside_out_frame = true;
    k.fixes.revolve_analytic_profile_faces = true;
    k.fixes.taper_tangent_corner_planes = true;
    k.fixes.revolve_nested_shells_as_cavities = true;
    k.fixes.revolve_holed_profile_void = true;
    k.fixes.revolve_splits_curves_on_axis = true;
    k.fixes.loops_go_on_round_the_vertex = true;
    k.fixes.linear_sweep_arc_is_cylinder = true;
    k.fixes.join_touching_loops = true;
    k.fixes.offset_resolve_crossing_loops = true;
    k.fixes.local_operation_box_holds_new_surfaces = true;
    k.fixes.local_operation_refuses_loose_tangent_edges = true;
    k.fixes.local_operation_step_walls = true;
    k.fixes.local_operation_closes_walls_to_nothing = true;
    k.fixes.offset_collapsed_rounds_mated = true;
    k.fixes.revolution_round_offset_collapses = true;
    k.fixes.uv_trim_corner_end_points_only = true;
    k.fixes.bspline_trim_preserves_uv_spans = true;
    k.fixes.proto_edge_self_loop_on_vertex_degenerate = true;
    k.fixes.slice_coincident_faces_sheet_side = true;
    k.fixes.boolean_outer_shell_first = true;
    k.fixes.cover_refuses_folded_continuation = true;
    k.fixes.cover_wraps_across_closed_seam = true;
    k.fixes.boolean2d_plane_covers_far_faces = true;
    k.fixes.boolean_vertex_by_best_crossing = true;
    k.fixes.boolean_splits_definition_across_split = true;
    k.fixes.proto_edge_splits_at_captured_vertices = true;
    k.fixes.boolean_cleanup_removes_slits = true;
    k.fixes.classify_edge_interval = true;
    k.fixes.ellipse_trim_seam_lines_span_cone_height = true;
    k.fixes.degenerate_proto_edge_in_one_vertex = true;
    k.fixes.new_edge_vertex_tolerance_covers_gap = true;
    k.fixes.transform_scales_each_tolerance = true;
    k.fixes.transform_model_size_from_body_box = true;
    k.fixes.torus_signed_gen_curve_interval = true;
    k.fixes.torus_major_radius_from_origin = true;
    k.fixes.torus_iso_circle_axis_from_midpoint = true;
    k.fixes.found_torus_sphere_keep_tested_nurb = true;
    k.fixes.internal_pole_row_through_knot = true;
    k.fixes.extrusion_origin_scaled_zero = true;
    k.fixes.point_solve_keeps_triangle_guess = true;
    k.fixes.imprint_extends_edge_to_proto_vertex = true;
    k.fixes.imprint_reuses_coincident_face_edge = true;
    k.fixes.fillet_point_step_per_point = true;
    k.fixes.analytic_copy_keeps_parameterization = true;
    k.fixes.offset_extension_from_base = true;
    k.fixes.fillet_cleanup_keeps_target_faces = true;
    k.fixes.fillet_nx1_tangent_side_edge = true;
    k.fixes.fillet_nx1_opposite_continuation_side_edge = true;
    k.fixes.fillet_rails_meet_at_one_end_point = true;
    k.fixes.fillet_3x2_mixed_concave_side_edge = true;
    k.fixes.offset_analytic_mate_edges = true;
    k.fixes.shell_edges_on_analytics_exact = true;
    k.fixes.offset_stitch_at_body_tolerance = true;
    k.fixes.curve_contacts_at_curve_ends = true;
    k.fixes.shell_empty_cavity_fails = true;
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.fillet_rail_end_on_continued_surface = true;
    k.fixes.ssi_tangent_contact_points = true;
    k.fixes.ssi_trace_tangent_curves_once = true;
    k.fixes.ssi_singular_start_inside_direction = true;
    k.fixes.ssi_pole_point_keeps_its_tangent = true;
    k.fixes.ssi_tangent_curve_tracer = true;
    k.fixes.boolean_skips_faces_apart = true;
    k.fixes.delete_vertex_joins_seam_iso_curve = true;
    k.fixes.boolean_new_face_closure_both_sides = true;
    k.fixes.box_face_failures_propagate = true;
    k.fixes.fillet_removal_takes_ring_fillets = true;
}

fn read(k: &mut Kernel) -> Brep {
    let bytes = include_bytes!("data/issue_1945_blend_1207_step4.smb").to_vec();
    let mut attributes = AttributeStore::new();
    let mut records = NotifyRecords::new();
    let read_key = |e| topology_ref(1, e);
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &read_key,
        records: &mut records,
        next_key: 1 << 50,
    };
    let mut b = read_brep_from_file(k, Some(bytes), false, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

/// The edge whose points at 0, 1/4 .. 1 of its interval are `pts` (either way round).
fn edge_by(k: &Kernel, b: &Brep, pts: &[[f64; 3]; 5]) -> EdgeId {
    let mut best = (f64::INFINITY, None);
    for e in b.edges().ids() {
        let c = edge::curve(b, e).unwrap().unwrap();
        let ivl = edge::interval(b, e).unwrap();
        let s: Vec<Point3> = [0.0, 0.25, 0.5, 0.75, 1.0]
            .iter()
            .map(|&u| c.evaluate_point(k, ivl.evaluate(u)).unwrap())
            .collect();
        let fwd = (0..5)
            .map(|i| s[i].distance(Point3::new(pts[i][0], pts[i][1], pts[i][2])))
            .fold(0.0, f64::max);
        let bwd = (0..5)
            .map(|i| s[4 - i].distance(Point3::new(pts[i][0], pts[i][1], pts[i][2])))
            .fold(0.0, f64::max);
        let d = fwd.min(bwd);
        if d < best.0 {
            best = (d, Some(e));
        }
    }
    assert!(best.0 < 1e-9, "no edge matches: {}", best.0);
    best.1.unwrap()
}

fn run(nd_fix: bool) -> (usize, usize, usize, f64, Vec<String>) {
    run_memo(nd_fix, None)
}

/// [`run`] with `Kernel::memoize_curve_caches` set to `memo` (left at its default for
/// `None`).
fn run_memo(nd_fix: bool, memo: Option<bool>) -> (usize, usize, usize, f64, Vec<String>) {
    let mut k = Kernel::new(&solidcore::kernel::KernelOptions::default());
    if let Some(memo) = memo {
        k.memoize_curve_caches = memo;
    }
    sc_fixes(&mut k);
    k.fixes.fillet_trace_reaches_rails = true;
    k.fixes.oscillating_skips_failed_guesses = nd_fix;
    let mut b = read(&mut k);
    let top = edge_by(
        &k,
        &b,
        &[
            [16.09347693943108, -15.0, 10.0],
            [37.07010770457331, -15.0, 10.0],
            [58.04673846971554, -15.0, 10.0],
            [79.02336923485778, -15.0, 10.0],
            [100.0, -15.0, 10.0],
        ],
    );
    let section = edge_by(
        &k,
        &b,
        &[
            [13.228756555322962, -15.0, 11.999999999999996],
            [13.54215880044533, -14.999999999999892, 11.1105555177324],
            [14.259567152304003, -14.999999999999995, 10.483331405453916],
            [15.144481419353674, -14.999999999999982, 10.120740307933572],
            [16.093476939431085, -15.0, 10.0],
        ],
    );
    {
        let mut s = FilletSession::new(&mut k, &mut b).unwrap();
        s.prepare_per_edge_constant_radius(
            &mut k,
            &[top, section],
            &[2.0, 2.0],
            &[FilletSurfaceGeneratorType::Circular],
            1,
            1.0,
        )
        .unwrap();
        s.create_corners(&k, None).unwrap();
        s.set_mark_corner_patches(false);
        s.do_filleting(&mut k, None).unwrap();
    }
    stitch_and_orient(&mut k, &mut b, &StitchAndOrientOptions::default()).unwrap();
    b.refresh_cached_topology();
    let lines = check_brep(&mut k, &mut b, CheckLevel::Level2)
        .unwrap()
        .lines();
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    let v = brep_volume(&k, &mut b, 1e-8, &attrs).unwrap();
    (
        b.face_list().len(),
        b.edge_list().len(),
        b.vertex_list().len(),
        v,
        lines,
    )
}

/// With the fix: the fork's topology, the volume of its body with exact circles (to 1e-9
/// relative; see the header) and a clean check.
#[test]
fn step4_round_matches_the_fork_and_checks_clean() {
    let (f, e, v, vol, lines) = run(true);
    assert_eq!((f, e, v), (11, 25, 16));
    let want = EXACT_CIRCLES_VOLUME;
    assert!((vol - want).abs() <= 1e-9 * want, "V {vol} vs {want}");
    assert!(lines.is_empty(), "{lines:#?}");
}

/// Without the N-D opt-in this case builds the same body: since
/// tangency-labs/solidcore#2574 the one-dimensional solve takes no value from a failed
/// evaluation by default, and `oscillating_skips_failed_guesses` no longer changes the
/// result here — the fork's topology and volume, bit for bit what `run(true)` builds, and
/// a clean check. Before #2574 the rail's UV trim kept its short span (test #15), and
/// with SolidCraft's tangent end (WR-600's flag, the fork's tangency-labs/smlib@856b54e)
/// the upstream solve left a false near cusp in a straight 2D trim instead (test #13), so
/// the body was refused.
#[test]
fn step4_round_without_the_nd_opt_in_builds_the_same_body() {
    let (f, e, v, vol, lines) = run(false);
    assert_eq!((f, e, v), (11, 25, 16));
    let want = EXACT_CIRCLES_VOLUME;
    assert!((vol - want).abs() <= 1e-9 * want, "V {vol} vs {want}");
    assert!(lines.is_empty(), "{lines:#?}");
    let (_, _, _, with_opt_in, _) = run(true);
    assert_eq!(
        vol.to_bits(),
        with_opt_in.to_bits(),
        "V {vol} vs {with_opt_in}"
    );
}

/// #2568: the check's curve analysis shortcuts change no report. Without the fix the
/// body has a straight UV trim on which the radius search returns a radius of 0 (its
/// objective fails to evaluate there); the shortcut once skipped that search and lost
/// the report.
#[test]
fn step4_checks_the_same_with_and_without_the_curve_analysis_shortcuts() {
    for nd_fix in [false, true] {
        let full = run_memo(nd_fix, Some(false));
        let fast = run_memo(nd_fix, Some(true));
        assert_eq!(fast.4, full.4, "nd_fix {nd_fix}");
        assert_eq!((fast.0, fast.1, fast.2), (full.0, full.1, full.2));
        assert_eq!(fast.3.to_bits(), full.3.to_bits());
    }
}
