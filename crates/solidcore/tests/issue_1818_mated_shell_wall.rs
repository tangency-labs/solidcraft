//! tangency-labs/solidcore#1818 under `Kernel::fixes.offset_extended_edges_mated` (WR-344):
//! a ruled loft from a 40 x 40 square at z = 0 to a circle r 15 at z = 60 (four curves a
//! section, `create_skin_primitive`), shelled 1 inward open at its top with extended convex
//! edges, merged results and the offset solid made (as SolidCraft's `smlib_shell` calls
//! `SmApiShellBrepFull`).
//!
//! Upstream (default fixes) refuses the shell: the merge fails with `SM_ERR` (1001) in
//! `make_faceuse` (pinned C++ agrees, `offset.offset_executive_p2` case
//! `issue1818_square_to_circle_loft_api_shell_1_top`). With the flag alone the merge no
//! longer intersects the side faces' mated offsets, gets through, and the difference
//! returned `Ok` with ten faces, not a manifold solid, volume 0. The fork's commit
//! (tangency-labs/smlib@85bbfaf, applied alone to the pinned C++) returns the same body;
//! the fork's C API refuses it (`finish`, `SMLIB_ERR_NOT_SOLID`). The flag now refuses a
//! shell of a solid that is no closed manifold solid (`SM_ERR`). The circle-to-circle loft
//! (a cone frustum r 15) is the control: it shells to the closed form under the flag.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, volume, Notifications};
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::error::{SolidError, SM_ERR};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::kernel::UpstreamFixes;
use solidcore::math::Vec3;
use solidcore::offset::offset_executive::p1::OffsetExecutive;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions, ShellOutcome};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::sweep::api_surfaces::create_skin_primitive;
use solidcore::{Brep, Kernel};

/// The issue's loft: a 40 x 40 square (or, for the control, a circle r 15) at z = 0 to a
/// circle r 15 at z = 60, each section four curves, the circle's quarter arcs starting at
/// -135 degrees (so the side faces' edges run from the square's corners).
fn loft(k: &mut Kernel, square: bool) -> Brep {
    let mut curves = Vec::new();
    let xy = [[-20.0, -20.0], [20.0, -20.0], [20.0, 20.0], [-20.0, 20.0]];
    for section in 0..2 {
        for (i, point) in xy.iter().enumerate() {
            let c = if square && section == 0 {
                Curve::from(
                    create_line_segment(
                        Vec3::new(point[0], point[1], 0.0),
                        Vec3::new(xy[(i + 1) % 4][0], xy[(i + 1) % 4][1], 0.0),
                    )
                    .unwrap(),
                )
            } else {
                let start = (-135.0 + f64::from(u32::try_from(i).unwrap()) * 90.0)
                    * std::f64::consts::PI
                    / 180.0;
                let frame = Axis2Placement::new(
                    Vec3::new(0.0, 0.0, f64::from(section) * 60.0),
                    Vec3::new(start.cos(), start.sin(), 0.0),
                    Vec3::new(-start.sin(), start.cos(), 0.0),
                );
                Curve::from(
                    Ellipse::create_canonical_circle(
                        k,
                        &frame,
                        15.0,
                        Some(&Extent1d::from_min_max(0.0, 90.0)),
                        None,
                    )
                    .unwrap(),
                )
            };
            curves.push(c);
        }
    }
    let refs: Vec<_> = curves.iter().map(Some).collect();
    let mut part = create_skin_primitive(k, &[4, 4], &refs, 1, true)
        .unwrap()
        .brep;
    part.refresh_cached_topology();
    part
}

/// The loft shelled 1 inward open at its top (`SmApiShellBrepFull(b, -1, TRUE, TRUE,
/// TRUE, [top])`), under `fixes`.
fn shell(fixes: UpstreamFixes, square: bool) -> (Kernel, ShellOutcome) {
    let mut k = Kernel::default();
    k.fixes.clone_from(&fixes);
    let mut part = loft(&mut k, square);
    assert!(solidcore::topology::integrity::check(&part).is_ok());
    assert!(brep_is_manifold_solid(&part).unwrap());
    let top = planar_faces_at(&k, &part, 2, 60.0);
    assert_eq!(top.len(), 1);
    let mut notes = Notifications::new(&mut part);
    let result = OffsetExecutive::shell_brep(
        &mut k,
        part,
        ShellOptions {
            offset: OffsetOptions {
                distance: -1.0,
                extend_convex_edges: true,
                merge_results: true,
            },
            create_offset_solid: true,
            merge_sheet_side_walls: false,
        },
        &top,
        &mut notes.context(),
    );
    (k, result)
}

fn mated() -> UpstreamFixes {
    let mut f = UpstreamFixes::default();
    f.offset_extended_edges_mated = true;
    f
}

fn assert_sm_err(result: solidcore::Result<Brep>, operation: &str, label: &str) {
    match result {
        Err(SolidError::Upstream {
            code, operation: o, ..
        }) => {
            assert_eq!(code, SM_ERR, "{label}");
            assert_eq!(o, operation, "{label}");
        }
        Err(other) => panic!("{label}: SM_ERR expected, got {other}"),
        Ok(mut wall) => {
            wall.refresh_cached_topology();
            panic!(
                "{label}: SM_ERR expected, got Ok: {} faces, manifold {:?}",
                wall.face_list().len(),
                brep_is_manifold_solid(&wall)
            );
        }
    }
}

/// Upstream's refusal (the default path, at parity with the pinned C++).
#[test]
fn default_fixes_refuse_the_square_to_circle_shell() {
    let (_, result) = shell(UpstreamFixes::default(), true);
    // The early merge refusal happens before shell_difference takes ownership.
    assert!(result.retained_input.is_some());
    assert!(!result.input_ownership_transferred());
    assert_sm_err(
        result.result,
        "boolean::merge::p2::make_faceuse",
        "default fixes",
    );
}

/// The flag alone: the difference's broken wall is refused, not returned.
#[test]
fn mated_offsets_alone_refuse_the_broken_wall() {
    let (_, result) = shell(mated(), true);
    // The final-wall refusal must not return an input already consumed by the difference.
    assert!(result.retained_input.is_none());
    assert!(result.input_ownership_transferred());
    assert_sm_err(
        result.result,
        "offset::offset_executive::p2::shell_brep",
        "offset_extended_edges_mated",
    );
}

/// With WR-578's loop-gap closing too, the offset is refused before the difference.
#[test]
fn mated_offsets_with_loop_gaps_refuse_earlier() {
    let mut fixes = mated();
    fixes.offset_closes_loop_gaps = true;
    let (_, result) = shell(fixes, true);
    assert_sm_err(
        result.result,
        "offset::offset_executive::step_gaps",
        "offset_extended_edges_mated + offset_closes_loop_gaps",
    );
}

/// SolidCraft's fixes (`issue_1476_swept_bar_shell.rs`, which include the flag and WR-454's
/// `shell_empty_cavity_fails`, not WR-578's loop gaps) reached the same broken wall.
#[test]
fn solidcraft_fixes_refuse_the_broken_wall() {
    let (_, result) = shell(solidcraft_fixes(), true);
    assert_sm_err(
        result.result,
        "offset::offset_executive::p2::shell_brep",
        "SolidCraft fixes",
    );
}

/// The control: the circle-to-circle loft (a cone frustum, here a cylinder r 15) shells
/// under the flag to the closed form, as on the default path.
#[test]
fn mated_offsets_shell_the_circle_control() {
    let analytic = std::f64::consts::PI * (15.0 * 15.0 * 60.0 - 14.0 * 14.0 * 59.0);
    for fixes in [UpstreamFixes::default(), mated()] {
        let (k, result) = shell(fixes, false);
        let mut wall = result.result.unwrap();
        wall.refresh_cached_topology();
        assert!(solidcore::topology::integrity::check(&wall).is_ok());
        assert!(brep_is_manifold_solid(&wall).unwrap());
        let v = volume(&k, &mut wall).unwrap();
        assert!((v - analytic).abs() < 1e-6 * analytic, "{v} vs {analytic}");
    }
}

/// The difftest operand (`offset.offset_executive_p2`, `issue1818.json`).
const OUT: &str = "fixtures/offset.offset_executive_p2/data/issue1818_square_to_circle_loft.smb";

fn named(e: solidcore::EntityRef, base: u64) -> solidcore::geom::object::ObjectRef {
    use solidcore::EntityRef;
    let (type_code, kind, index) = match e {
        EntityRef::Brep(x) => (16011, 0u64, x.index()),
        EntityRef::Region(x) => (16012, 1, x.index()),
        EntityRef::Shell(x) => (16013, 2, x.index()),
        EntityRef::Faceuse(x) => (16022, 3, x.index()),
        EntityRef::Face(x) => (16002, 4, x.index()),
        EntityRef::Loopuse(x) => (16004, 5, x.index()),
        EntityRef::Loop(x) => (16003, 6, x.index()),
        EntityRef::Edgeuse(x) => (16005, 7, x.index()),
        EntityRef::Edge(x) => (16016, 8, x.index()),
        EntityRef::Vertexuse(x) => (16006, 9, x.index()),
        EntityRef::Vertex(x) => (16021, 10, x.index()),
    };
    solidcore::geom::object::ObjectRef {
        type_code,
        key: solidcore::geom::attribute::ObjectKey(base + 1 + (kind << 32) + u64::from(index)),
    }
}

#[test]
#[ignore = "generator: writes the #1818 fixture file"]
fn write_issue_1818_operand() {
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::topology::api_files::{read_brep_from_file, write_brep_to_file};
    use solidcore::topology::brep::p10::TopologyEnv;
    use solidcore::topology::brep_data_write::FromBrepEnv;
    use solidcore::topology::topology::NotifyRecords;
    use solidcore::topology::{EdgeId, EdgeuseId, FaceId};
    let mut k = Kernel::default();
    let b = loft(&mut k, true);
    let store = AttributeStore::new();
    let recs = NotifyRecords::new();
    let key_of = |e| named(e, 0);
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &key_of,
        records: &recs,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    let bytes = write_brep_to_file(&k, Some(&b), &env, true).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join(OUT);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    let mut store2 = AttributeStore::new();
    let mut recs2 = NotifyRecords::new();
    let read_key = |e| named(e, 1 << 40);
    let mut tenv = TopologyEnv {
        all_attributes: &[],
        attributes: &mut store2,
        key_of: &read_key,
        records: &mut recs2,
        next_key: 1 << 50,
    };
    let mut read =
        read_brep_from_file(&mut k, Some(bytes.clone()), true, false, &mut tenv).unwrap();
    read.refresh_cached_topology();
    let open = planar_faces_at(&k, &read, 2, 60.0);
    assert_eq!(open.len(), 1);
    let index = read.face_list().iter().position(|&f| f == open[0]).unwrap();
    eprintln!(
        "ISSUE 1818 wrote {OUT} ({} bytes): faces {} edges {} volume {:?}; open face index {index}",
        bytes.len(),
        read.faces().len(),
        read.edges().len(),
        volume(&k, &mut read)
    );
}

/// SolidCraft's `core::fixes()` as `issue_1476_swept_bar_shell.rs` lists them.
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
