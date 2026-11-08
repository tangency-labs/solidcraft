//! Issue #2954: shell-tangent gap rail reconstruction on curved seams and inner cavity walls.
//!
//! 1. `curved_shell_tangent_quarter_torus_seam_tracks_boundary`: a quarter-cylinder (`x >= 0`,
//!    `y >= 0`, `r = 10`, `h = 20`) whose top circular rim is filleted (`R = 3`), shelled `1 mm`
//!    inward with the outer cylindrical wall (`r = 10`) open. Across the horizontal quarter-circle
//!    seam at `z = 17`, the open `r = 10` cylinder and the `R = 3` toroidal fillet meet
//!    tangentially (`G1`), and the flat radial walls at `x = 0` and `y = 0` offset inward to
//!    `x = 1` and `y = 1`, trimming the shared vertices along both curved rails from
//!    `(10, 0, 17)..(0, 10, 17)` to `(sqrt(99), 1, 17)..(1, sqrt(99), 17)` on `r = 10` and
//!    `(sqrt(80), 1, 17)..(1, sqrt(80), 17)` on `r = 9` so `reconcile_shell_tangent_rails`
//!    exercises curved rail trimming, extended-surface `Surface::drop_curve` UV projection, and
//!    shared-vertex incident-edge consistency (`AssertValid` Edge tests 16–17).
//! 2. `shelled_box_open_inner_wall_shared_vertex_consistency`: the `#2954` doubly-shelled rounded
//!    box (`70 x 50 x 30`, vertical `R = 6` rounds, shelled `2 mm` open on top, then shelled
//!    `0.5 mm` open at a flat inner cavity wall), checking manifoldness, closed-form volume, and
//!    every shared vertex's distance to all incident edge endpoints against `tau_e + tau_v`.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, volume};
use solidcore::blend::fillet_executive::api::fillet_edges;
use solidcore::curve::curve::CurveKind;
use solidcore::math::Point2;
use solidcore::offset::offset_executive::p2::{shell_brep_simple, OffsetOptions, ShellOptions};
use solidcore::primitive::api_primitives::{create_box, create_partial_cylinder};
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::topology::brep::p1;
use solidcore::topology::edge as tedge;
use solidcore::{Brep, FaceId, Kernel, Point3};

fn shell_solid(
    k: &mut Kernel,
    part: &Brep,
    distance: f64,
    open: &[FaceId],
) -> solidcore::Result<Brep> {
    let mut r = shell_brep_simple(
        k,
        part,
        ShellOptions {
            offset: OffsetOptions {
                distance,
                extend_convex_edges: true,
                merge_results: true,
            },
            create_offset_solid: true,
            merge_sheet_side_walls: true,
        },
        open,
    )?;
    r.refresh_cached_topology();
    Ok(r)
}

fn assert_all_edge_vertex_endpoints_within_tolerance(
    k: &Kernel,
    b: &Brep,
) -> solidcore::Result<()> {
    for e in p1::edges(b, None)? {
        let edge = b.edge(e)?;
        let Some(ref curve) = edge.curve else {
            continue;
        };
        let ivl = edge.interval;
        let edge_tol = edge.tolerance;
        if let Some(v_start) = tedge::start_vertex(b, e)? {
            let v_rec = b.vertex(v_start)?;
            let p_start = curve.evaluate_point(k, ivl.min())?;
            let d = v_rec.point.distance(p_start);
            assert!(
                d <= edge_tol + v_rec.tolerance,
                "edge {e:?} start gap {d:.3e} exceeds tau_e + tau_v ({:.3e})",
                edge_tol + v_rec.tolerance
            );
            if let Some(v_end) = tedge::other_vertex(b, e, Some(v_start))? {
                let v_end_rec = b.vertex(v_end)?;
                let p_end = curve.evaluate_point(k, ivl.max())?;
                let d_end = v_end_rec.point.distance(p_end);
                assert!(
                    d_end <= edge_tol + v_end_rec.tolerance,
                    "edge {e:?} end gap {d_end:.3e} exceeds tau_e + tau_v ({:.3e})",
                    edge_tol + v_end_rec.tolerance
                );
            }
        }
    }
    Ok(())
}

fn check_quarter_torus_seam_case(mut k: Kernel) -> solidcore::Result<()> {
    let mut quarter_part =
        create_partial_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 0.0, 90.0)?;
    let top_rim: Vec<_> = quarter_part
        .edges()
        .iter()
        .filter(|(_, e)| {
            let Some(c) = e.curve.as_ref() else {
                return false;
            };
            if matches!(c.kind(), CurveKind::Line(_))
                || c.is_line(&k, 10, 1e-4).ok().flatten().is_some()
            {
                return false;
            }
            let p = c.evaluate_point(&k, e.interval.evaluate(0.5)).unwrap();
            (p.z - 20.0).abs() < 1e-6
        })
        .map(|(id, _)| id)
        .collect();
    assert_eq!(top_rim.len(), 1, "quarter-cylinder top circular edge");
    fillet_edges(&mut k, &mut quarter_part, &top_rim, 3.0, 1, 1, 1.0)?;
    quarter_part.refresh_cached_topology();

    let cyl_open: Vec<_> = quarter_part
        .faces()
        .iter()
        .filter(|(_, f)| {
            let Some(s) = f.surface.as_ref() else {
                return false;
            };
            let dom = s.natural_uv_domain_with_nurb(&k).unwrap();
            let mid = Point2::new(
                0.5 * (dom.min().x + dom.max().x),
                0.5 * (dom.min().y + dom.max().y),
            );
            let Ok(p) = s.evaluate_point(&k, mid) else {
                return false;
            };
            (p.x.hypot(p.y) - 10.0).abs() < 0.2 && p.z > 2.0 && p.z < 15.0
        })
        .map(|(id, _)| id)
        .collect();
    assert_eq!(cyl_open.len(), 1, "quarter-cylinder outer wall at r = 10");

    // 1. Unmerged offset sheet (`merge_results: false, create_offset_solid: false`):
    //    exercises `do_extended_offset` + `reconcile_shell_tangent_rails` directly before
    //    boolean merge, proving the `z = 17` annular step-wall rails are trimmed to the
    //    settled vertices `(sqrt(99), 1, 17)..(1, sqrt(99), 17)` and
    //    `(sqrt(80), 1, 17)..(1, sqrt(80), 17)` with zero open loop gaps.
    let mut unmerged = shell_brep_simple(
        &mut k,
        &quarter_part,
        ShellOptions {
            offset: OffsetOptions {
                distance: -1.0,
                extend_convex_edges: true,
                merge_results: false,
            },
            create_offset_solid: false,
            merge_sheet_side_walls: false,
        },
        &cyl_open,
    )?;
    unmerged.refresh_cached_topology();
    solidcore::topology::integrity::check(&unmerged)?;
    assert_all_edge_vertex_endpoints_within_tolerance(&k, &unmerged)?;

    // 2. Merged solid (`merge_results: true, create_offset_solid: true`):
    let shelled = shell_solid(&mut k, &quarter_part, -1.0, &cyl_open)?;
    solidcore::topology::integrity::check(&shelled)?;
    assert!(
        brep_is_manifold_solid(&shelled)?,
        "shelled quarter-filleted cylinder must be a manifold solid"
    );
    assert_all_edge_vertex_endpoints_within_tolerance(&k, &shelled)?;

    // Both rails of the `1 mm` horizontal annular step wall at `z = 17` (`r = 10` on
    // the open cylinder and `r = 9` on the offset torus, trimmed to `x >= 1, y >= 1`)
    // must be genuinely curved (non-line), end at `(sqrt(r^2 - 1), 1, 17)` and
    // `(1, sqrt(r^2 - 1), 17)`, and stay within `edge.tolerance` of `r` and `z = 17`
    // across interior samples (straight chords would dip to `r = 7.74` and `r = 7.03`).
    let mut found_outer_rail = false;
    let mut found_inner_rail = false;
    for e in p1::edges(&shelled, None)? {
        let edge = shelled.edge(e)?;
        let Some(ref curve) = edge.curve else {
            continue;
        };
        if matches!(curve.kind(), CurveKind::Line(_)) || curve.is_line(&k, 10, 1e-4)?.is_some() {
            continue;
        }
        let ivl = edge.interval;
        let p_mid = curve.evaluate_point(&k, ivl.evaluate(0.5))?;
        if (p_mid.z - 17.0).abs() >= 1e-2 || p_mid.x < 2.0 || p_mid.y < 2.0 {
            continue;
        }
        let r_mid = p_mid.x.hypot(p_mid.y);
        let expected_r = if (r_mid - 10.0).abs() < 0.2 {
            found_outer_rail = true;
            10.0_f64
        } else if (r_mid - 9.0).abs() < 0.2 {
            found_inner_rail = true;
            9.0_f64
        } else {
            continue;
        };
        let trim_coord = (expected_r * expected_r - 1.0).sqrt();
        let end_a = Point3::new(trim_coord, 1.0, 17.0);
        let end_b = Point3::new(1.0, trim_coord, 17.0);
        let p0 = curve.evaluate_point(&k, ivl.min())?;
        let p1_end = curve.evaluate_point(&k, ivl.max())?;
        let ends_match = (p0.distance(end_a) <= 2.0 * edge.tolerance
            && p1_end.distance(end_b) <= 2.0 * edge.tolerance)
            || (p0.distance(end_b) <= 2.0 * edge.tolerance
                && p1_end.distance(end_a) <= 2.0 * edge.tolerance);
        assert!(
            ends_match,
            "rail r={expected_r} ends ({p0:?}, {p1_end:?}) do not match trimmed ({end_a:?}, {end_b:?}) within 2*tol ({:.3e})",
            2.0 * edge.tolerance
        );
        for frac in [0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0] {
            let p = curve.evaluate_point(&k, ivl.evaluate(frac))?;
            let r = p.x.hypot(p.y);
            assert!(
                (r - expected_r).abs() <= edge.tolerance,
                "sample {frac} radius {r} deviates from {expected_r} by {:.3e} > edge tol {:.3e}",
                (r - expected_r).abs(),
                edge.tolerance
            );
            assert!(
                (p.z - 17.0).abs() <= edge.tolerance,
                "sample {frac} z {} deviates from 17.0 by {:.3e} > edge tol {:.3e}",
                p.z,
                (p.z - 17.0).abs(),
                edge.tolerance
            );
        }
    }
    assert!(
        found_outer_rail && found_inner_rail,
        "expected both curved step-wall rails at z = 17 (r = 10: {found_outer_rail}, r = 9: {found_inner_rail})"
    );
    Ok(())
}

#[test]
fn curved_shell_tangent_quarter_torus_seam_tracks_boundary() -> solidcore::Result<()> {
    // `offset_closes_loop_gaps` (WR-578) runs `CloseLoopGaps` — including
    // `reconcile_shell_tangent_rails` — before `do_extended_offset` merges the shell.
    // 1. Default surface representations (`SmBSplineSurface` revolved wall and
    //    `SmOffsetSurface` over the B-spline top-rim round).
    let mut k_bspline = Kernel::default();
    k_bspline.fixes.offset_closes_loop_gaps = true;
    check_quarter_torus_seam_case(k_bspline)?;

    // 2. Analytic wall and round (`revolve_analytic_profile_faces` produces `SmCone`
    //    for the cylindrical wall, and `fillet_exact_round_classes` produces `SmTorus`
    //    for the circular top-rim round).
    let mut k_analytic = Kernel::default();
    k_analytic.fixes.offset_closes_loop_gaps = true;
    k_analytic.fixes.revolve_analytic_profile_faces = true;
    k_analytic.fixes.fillet_exact_round_classes = true;
    check_quarter_torus_seam_case(k_analytic)
}

#[test]
fn shelled_box_open_inner_wall_shared_vertex_consistency() -> solidcore::Result<()> {
    // Only `offset_closes_loop_gaps` (WR-578) is enabled so `do_extended_offset` runs
    // `CloseLoopGaps` / `reconcile_shell_tangent_rails` across the open inner cavity
    // wall's tangent corners before merging.
    let mut k = Kernel::default();
    k.fixes.offset_closes_loop_gaps = true;

    // Issue #2954 geometry: `70 x 50 x 30` box (`[-35, 35] x [-25, 25] x [0, 30]`) with
    // vertical `R = 6` corner rounds, shelled `2 mm` inward open on top (`z = 30`), then
    // shelled `0.5 mm` inward open at the flat inner cavity wall `y = -23`.
    let mut b = create_box(&mut k, Point3::new(-35.0, -25.0, 0.0), 70.0, 50.0, 30.0)?;
    let vertical_edges: Vec<_> = b
        .edges()
        .iter()
        .filter(|(_, e)| {
            let c = e.curve.as_ref().unwrap();
            let p = c.evaluate_point(&k, e.interval.min()).unwrap();
            let q = c.evaluate_point(&k, e.interval.max()).unwrap();
            (p.x - q.x).abs() < 1e-9 && (p.y - q.y).abs() < 1e-9
        })
        .map(|(id, _)| id)
        .collect();
    assert_eq!(vertical_edges.len(), 4);
    fillet_edges(&mut k, &mut b, &vertical_edges, 6.0, 1, 1, 1.0)?;
    b.refresh_cached_topology();

    let top = planar_faces_at(&k, &b, 2, 30.0);
    assert_eq!(top.len(), 1);
    let first_shell = shell_solid(&mut k, &b, -2.0, &top)?;

    let inner_wall = planar_faces_at(&k, &first_shell, 1, -23.0);
    assert_eq!(inner_wall.len(), 1, "flat inner cavity wall at y = -23");

    let mut second_shell = shell_solid(&mut k, &first_shell, -0.5, &inner_wall)?;
    solidcore::topology::integrity::check(&second_shell)?;
    assert!(brep_is_manifold_solid(&second_shell)?);
    assert_all_edge_vertex_endpoints_within_tolerance(&k, &second_shell)?;

    let vol = volume(&k, &mut second_shell)?;
    assert!(
        (9017.6..=9038.3).contains(&vol),
        "second shell volume {vol} outside closed-form enclosure [9017.6, 9038.3]"
    );
    Ok(())
}
