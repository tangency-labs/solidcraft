//! tangency-labs/solidcore#1476: shelling a swept bar failed with 1001. SolidCraft rig
//! case shell/566: an 8 x 5 rectangle swept (by the C++ sweep) along a 30 mm line, a
//! quarter arc of r 30 and a 30 mm line (14 faces: 12 B-spline or extrusion walls, two
//! planar caps), shelled 0.5 inward open at its start cap (z = 0). The operand is the
//! rig's working copy right before the shell (`data/issue_1476_swept_bar.smd`).
//!
//! Two causes, both fixed unconditionally:
//! - `OffsetExecutive::replace_implicit_offsets` took only a plain B-spline base;
//!   upstream's `SM_CAST_PTR(SmBSplineSurface, pBase)` takes every subclass, here a
//!   straight wall's `SmSurfOfExtrusion`. Both tests fail with 1001 without it.
//! - With SolidCraft's opt-in fixes on (the issue's configuration), the bend walls'
//!   offset curves (125 and 127 cubic segments) need 37 625 and 38 227 samples at
//!   1e-5, above the former fixed HANG-44 cap of 16 384; the bound is now
//!   input-scaled (`nurbs::crv_offset::offset_sample_bound`). The second test fails
//!   with 1001 without it. With the defaults the shell takes another path and never
//!   samples that many.
//!
//! Expected: one solid, V = 40 L - 28 (L - 0.5) = 1299.48668 with L = 60 + 15 pi. The
//! port gives 1299.48231 with SolidCraft's fixes (3.4e-6 below; SolidCraft measured
//! 1299.48275 on an older main), pinned to 1e-9; the operand's own volume is 1.2e-2 below
//! its closed form. With the kernel's own defaults the volume is checked against the
//! closed form only.
//!
//! With `shell_edges_on_analytics_exact` (WR-545) on as well, as the SolidCraft rig runs
//! it, `make_edges_on_analytics_exact` intersected the walls' face-owned surfaces without
//! their faces and the shell failed with `Unported(topology.face_p2)`; it now goes through
//! the result's face services. Two marched edges between the inner planar walls become
//! lines; the volumes stay as above.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, volume, Notifications};
use solidcore::curve::curve::p2::{Curve, CurveKind};
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::ObjectRef;
use solidcore::kernel::UpstreamFixes;
use solidcore::offset::offset_executive::p1::OffsetExecutive;
use solidcore::offset::offset_executive::p2::{OffsetOptions, ShellOptions};
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::topology::brep::p10::TopologyEnv;
use solidcore::topology::brep::p7::read_from_file;
use solidcore::topology::database_io::FileType;
use solidcore::topology::topology::NotifyRecords;
use solidcore::{Brep, EntityRef, Kernel};

/// `SmBrep::ReadFromFile` of the rig's operand.
fn operand(k: &mut Kernel) -> Brep {
    let bytes = include_bytes!("data/issue_1476_swept_bar.smd").to_vec();
    let mut b = Brep::new_with_infinite_region().unwrap();
    let mut records = NotifyRecords::new();
    let mut attributes = AttributeStore::default();
    let key = |_: EntityRef| ObjectRef {
        key: ObjectKey(0),
        type_code: 0,
    };
    let mut env = TopologyEnv {
        all_attributes: &[],
        attributes: &mut attributes,
        key_of: &key,
        records: &mut records,
        next_key: 1 << 40,
    };
    read_from_file(k, &mut b, bytes, FileType::Ascii, false, &mut env).unwrap();
    b.refresh_cached_topology();
    b
}

/// SolidCraft's `core::fixes()`: every opt-in `UpstreamFixes` flag it runs with (the
/// issue's configuration).
fn solidcraft_fixes() -> UpstreamFixes {
    let mut f = UpstreamFixes::default();
    f.fillet_refuse_face_consuming_blends = true;
    f.fillet_corner_ball_patch = true;
    f.fillet_chamfer_cone_on_arc = true;
    f.offset_excess_faces_after_merge = true;
    f.offset_clip_right_side_out = true;
    f.offset_extended_edges_mated = true;
    f.keep_operand_uv_trims = true;
    f.geometric_g2_continuity = true;
    f.proto_vertex_miss_is_false = true;
    f.check_loop_vertex_tolerance = true;
    f.check_loop_projected_seam_ends = true;
    f.oscillating_skips_failed_guesses = true;
    f.refine_cc_range_ends = true;
    f.flat_corners_test_bits = true;
    f.cylinder_rebuild_mean_radius = true;
    f.revolve_analytic_profile_faces = true;
    f.revolve_nested_shells_as_cavities = true;
    f.join_touching_loops = true;
    f.offset_resolve_crossing_loops = true;
    f.offset_walls_touch_refused = true;
    f.offset_analytic_mate_edges = true;
    f.nurb_cylinder_inside_out_frame = true;
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
    f.offset_stitch_at_body_tolerance = true;
    f.curve_contacts_at_curve_ends = true;
    f.shell_empty_cavity_fails = true;
    f.ssi_tangent_contact_points = true;
    f.ssi_trace_tangent_curves_once = true;
    f.found_torus_sphere_keep_tested_nurb = true;
    f
}

/// The shell of the operand (`SmApiShellBrepFull(b, -0.5, TRUE, TRUE, TRUE, [start
/// cap])`), with its run time.
fn shell(fixes: &UpstreamFixes) -> (Kernel, solidcore::Result<Brep>) {
    let mut k = Kernel::default();
    k.fixes.clone_from(fixes);
    let mut part = operand(&mut k);
    assert_eq!(part.face_list().len(), 14);
    let start = planar_faces_at(&k, &part, 2, 0.0);
    assert_eq!(start.len(), 1);
    let mut notifications = Notifications::new(&mut part);
    let clock = std::time::Instant::now();
    let result = OffsetExecutive::shell_brep(
        &mut k,
        part,
        ShellOptions {
            offset: OffsetOptions {
                distance: -0.5,
                extend_convex_edges: true,
                merge_results: true,
            },
            create_offset_solid: true,
            merge_sheet_side_walls: true,
        },
        &start,
        &mut notifications.context(),
    )
    .result;
    eprintln!("issue_1476 shell: {:.2?}", clock.elapsed());
    (k, result)
}

/// Checks the shell and returns its number of line edges.
fn check(fixes: &UpstreamFixes, want: Option<f64>) -> usize {
    let (k, result) = shell(fixes);
    let mut b = result.unwrap();
    b.refresh_cached_topology();
    assert!(brep_is_manifold_solid(&b).unwrap());
    let v = volume(&k, &mut b).unwrap();
    let kinds: Vec<&str> = b
        .edge_list()
        .iter()
        .map(
            |&e| match b.edge(e).unwrap().curve.as_ref().map(Curve::kind) {
                Some(CurveKind::Line(_)) => "line",
                Some(CurveKind::Ellipse(_)) => "conic",
                Some(_) => "other",
                None => "none",
            },
        )
        .collect();
    eprintln!(
        "issue_1476 shell: {} faces, volume {v}, edges: {} lines, {} conics, {} other",
        b.face_list().len(),
        kinds.iter().filter(|&&k| k == "line").count(),
        kinds.iter().filter(|&&k| k == "conic").count(),
        kinds.iter().filter(|&&k| k == "other").count(),
    );
    let analytic =
        40.0 * (60.0 + 15.0 * std::f64::consts::PI) - 28.0 * (59.5 + 15.0 * std::f64::consts::PI);
    assert!((v - analytic).abs() <= 1e-5 * analytic, "{v} vs {analytic}");
    if let Some(want) = want {
        assert!((v - want).abs() <= 1e-9 * want, "{v} vs {want}");
    }
    kinds.iter().filter(|&&k| k == "line").count()
}

#[test]
fn issue_1476_swept_bar_shells() {
    assert_eq!(check(&UpstreamFixes::default(), None), 16);
}

/// About 6 s at opt-level 3 but over 4 min in a debug build (the two 38 000-sample
/// offsets' knot removal): run with `--ignored` (`--profile difftest`). The default
/// test above keeps the extrusion fix live; `nurbs::crv_offset`'s unit tests keep the
/// bound live.
#[test]
#[ignore = "slow in debug builds (two 38 000-sample curve offsets)"]
fn issue_1476_swept_bar_shells_with_solidcraft_fixes() {
    assert_eq!(check(&solidcraft_fixes(), Some(1299.482314966473)), 16);
}

/// `shell_edges_on_analytics_exact` (WR-545) alone: `make_edges_on_analytics_exact`
/// intersects the result's face-owned surfaces (the extrusion walls) with their faces'
/// trimmed caches, as the fork's `GlobalSurfaceIntersect` reaches them through
/// `SmSurface::GetFace`; without the face context it failed with
/// `Unported(topology.face_p2)` (SolidCraft rig case shell/566). Two marched edges
/// between the inner planar walls become lines; the volume does not change. The two
/// inner straight walls are certified nets (WR-718), bicubic and not recognised as
/// extrusions when merged, so their edges with the end cap stay marched (`N_SrfOffset`'s
/// fit was recognised, and those two became lines as well). The volume moves by 1.2e-10
/// relative, within the pin.
#[test]
fn issue_1476_swept_bar_shells_with_edges_on_analytics_exact() {
    let mut fixes = UpstreamFixes::default();
    fixes.shell_edges_on_analytics_exact = true;
    assert_eq!(check(&fixes, None), 18);
}

/// SolidCraft's configuration with `shell_edges_on_analytics_exact` on as well (the
/// rig's run of shell/566, which failed with `Unported(topology.face_p2)`): two edges
/// become lines. The fork (tangency-labs/smlib 68fb9162) gives V = 1299.4823. Slow in
/// debug builds, as above.
#[test]
#[ignore = "slow in debug builds (two 38 000-sample curve offsets)"]
fn issue_1476_swept_bar_shells_with_solidcraft_fixes_and_edges_on_analytics_exact() {
    let mut fixes = solidcraft_fixes();
    fixes.shell_edges_on_analytics_exact = true;
    assert_eq!(check(&fixes, Some(1299.482314966473)), 18);
}
