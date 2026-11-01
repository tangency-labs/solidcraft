//! Regression test for tangency-labs/solidcore#1950: WR-545's
//! `shell_edges_on_analytics_exact` (the fork's tangency-labs/smlib@cd85d34) on
//! face-owned surfaces. `MakeEdgesOnAnalyticsExact`
//! intersects the surfaces of the two faces of each marched edge. Upstream those are the
//! faces' own surfaces, whose caches resolve through their faces. Before #1963 (tdamsma)
//! the port intersected them without the B-rep's face services and stopped on `Unported`
//! (a trimmed cache of a face-owned surface, `TrimSrfCache::build_tree_with_face`, with
//! SolidCraft's fixes; the face handle of a surface's cache with the defaults). #1963
//! tests shell/566 and shell/1525; this is the third case of #1950. SolidCraft's plate: the profile
//! (0, 0)-(42.5, 0)-(42.5, 20)-(0, 20) closed by a half circle r 10 on the x < 0 side,
//! extruded 27.5, less a blind hole r 5 along x through the arc side, shelled 2 inward
//! open at x = 42.5. SolidCraft's rig gives V 10400.2131 without the flag; with it the
//! fork's C++ fails its plate with 1001 (#1950), which this reconstruction (the
//! `issue_1795` operand) does not reproduce: the port shells it, within 1e-7 of its
//! volume without the flag.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, shell, volume};
use solidcore::boolean::api_brep as boolean;
use solidcore::curve::curve::Curve;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::UpstreamFixes;
use solidcore::primitive::api_curves as curves;
use solidcore::primitive::api_primitives as primitives;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::sweep::api_brep as sweeps;
use solidcore::topology::proto_topology::p1::api_transform::{api_rotate, TransformObject};
use solidcore::{Brep, Kernel};

/// SolidCraft's volume of the shelled plate (without the flag).
const RIG_VOLUME: f64 = 10400.2131;

/// The plate (as `issue_1795_shell_hole_through_arc.rs` builds the #1528 operand).
fn drilled(k: &mut Kernel) -> Brep {
    let p = |x: f64, y: f64| Vec3::new(x, y, 0.0);
    let cs: Vec<Curve> = vec![
        Curve::from(curves::create_line_segment(p(0.0, 0.0), p(42.5, 0.0)).unwrap()),
        Curve::from(curves::create_line_segment(p(42.5, 0.0), p(42.5, 20.0)).unwrap()),
        Curve::from(curves::create_line_segment(p(42.5, 20.0), p(0.0, 20.0)).unwrap()),
        Curve::from(curves::create_arc(p(0.0, 10.0), 10.0, 90.0, 270.0).unwrap()),
    ];
    let refs: Vec<Option<&Curve>> = cs.iter().map(Some).collect();
    let body = sweeps::create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 27.5, true).unwrap();
    // a cylinder r 5 along z from (-10, 12.5, 12.5), turned onto +x about its base
    let mut drill =
        primitives::create_cylinder(k, Point3::new(-10.0, 12.5, 12.5), 5.0, 17.5).unwrap();
    api_rotate(
        k,
        Some(TransformObject::Brep(&mut drill)),
        Point3::new(-10.0, 12.5, 12.5),
        Vec3::new(0.0, 1.0, 0.0),
        90.0,
    )
    .unwrap();
    let mut b = boolean::boolean_difference(k, body, drill).unwrap();
    b.refresh_cached_topology();
    b
}

/// SolidCraft's `core::fixes()` (as `issue_1476_swept_bar_shell.rs` lists them).
fn solidcraft_fixes() -> UpstreamFixes {
    let mut f = UpstreamFixes::default();
    f.fillet_retry_radius_as_first = true;
    f.fillet_rail_rim_on_face = true;
    f.fillet_refuse_rails_leaving_faces = true;
    f.fillet_refuse_face_consuming_blends = true;
    f.fillet_refuse_non_manifold_result = true;
    f.fillet_refuse_crossing_rails = true;
    f.fillet_corner_ball_patch = true;
    f.fillet_chamfer_cone_on_arc = true;
    f.fillet_torus_center_on_axis = true;
    f.offset_excess_faces_after_merge = true;
    f.offset_clip_right_side_out = true;
    f.revolution_offset_axis_both_sides = true;
    f.offset_extended_edges_mated = true;
    f.remove_slit_loops_where_solids_touch = true;
    f.keep_operand_uv_trims = true;
    f.geometric_g2_continuity = true;
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
    f.revolve_nested_shells_as_cavities = true;
    f.curve_sweep_caps_at_sweep_ends = true;
    f.join_touching_loops = true;
    f.uv_trim_reversal_on_surface = true;
    f.offset_resolve_crossing_loops = true;
    f.offset_walls_touch_refused = true;
    f.offset_analytic_mate_edges = true;
    f.precise_properties_split_at_surface_knots = true;
    f.nurb_cylinder_inside_out_frame = true;
    f.shell_inner_shell_of_statuses = true;
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
    f.offset_closed_curve_trim_arc = true;
    f.offset_stitch_at_body_tolerance = true;
    f.curve_contacts_at_curve_ends = true;
    f.shell_empty_cavity_fails = true;
    f.fillet_refuse_ball_not_fitting = true;
    f.boolean_lists_virtual_surface_copies = true;
    f.sphere_small_circle_not_touching = true;
    f.ssi_tangent_contact_points = true;
    f.ssi_trace_tangent_curves_once = true;
    f.found_torus_sphere_keep_tested_nurb = true;
    f
}

/// The plate shelled 2 inward, open at x = 42.5, under `fixes`: a manifold solid and its
/// volume.
fn shelled_volume(fixes: &UpstreamFixes) -> f64 {
    let mut k = Kernel {
        fixes: *fixes,
        ..Kernel::default()
    };
    let b = drilled(&mut k);
    let open = planar_faces_at(&k, &b, 0, 42.5);
    assert_eq!(open.len(), 1, "the x = 42.5 face");
    let mut r = shell(&mut k, b, -2.0, &open).unwrap();
    r.refresh_cached_topology();
    assert_eq!(r.face_list().len(), 16);
    assert!(brep_is_manifold_solid(&r).unwrap());
    volume(&k, &mut r).unwrap()
}

fn check(v: f64, want: f64, without_flag: f64) {
    assert!(
        (v - RIG_VOLUME).abs() <= 1e-6 * RIG_VOLUME,
        "{v} vs the rig's"
    );
    assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
    // the exact edges move the volume by under 1e-7 relative
    assert!(
        (v - without_flag).abs() <= 1e-7 * without_flag,
        "{v} vs {without_flag}"
    );
}

/// With SolidCraft's fixes (the reported configuration; the trimmed cache stub before).
#[test]
fn shelled_plate_with_solidcraft_fixes() {
    let mut fixes = solidcraft_fixes();
    let without_flag = shelled_volume(&fixes);
    assert!((without_flag - 10400.212787697543).abs() <= 1e-9 * without_flag);
    fixes.shell_edges_on_analytics_exact = true;
    check(shelled_volume(&fixes), 10400.212342134768, without_flag);
}

/// With only the flag on.
#[test]
fn shelled_plate_with_the_flag_alone() {
    let mut fixes = UpstreamFixes::default();
    let without_flag = shelled_volume(&fixes);
    assert!((without_flag - 10400.212793536877).abs() <= 1e-9 * without_flag);
    fixes.shell_edges_on_analytics_exact = true;
    check(shelled_volume(&fixes), 10400.21294715712, without_flag);
}
