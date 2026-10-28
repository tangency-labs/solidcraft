//! tangency-labs/solidcore#398: SolidCraft rig case shell/1525 (`tangency-labs/solidcraft`,
//! branch `kernel-smlib`, `tests/kernel-cases/shell/1525-shell-box-with-rounds-chamfer-and-fillet`):
//! a 50 x 50 x 30 box, its four vertical edges rounded r10, a 5 mm chamfer round the base,
//! a 4 mm fillet where the chamfer meets the wall, shelled 2 mm inward (open at the top, and
//! closed).
//!
//! The cause is WR-421 (#420): `CalcTorusFilletGeom` revolves only a circular section, so the
//! chamfer round the r10 arcs is traced as an offset intersection instead of made as a cone.
//! The r4 fillet on that traced chamfer then shells into a body that is not a manifold solid
//! and has zero volume, although the shell reports success. The pinned C++ reference gives the
//! same body (72 faces, 307 edges, 236 vertices, for both shells), so by default the port
//! keeps it. With `fixes.fillet_chamfer_cone_on_arc` the chamfer is a cone, and both shells
//! are manifold solids of the closed-form volume (the SMLib fork, which has the fix, gives
//! 13701.73 for the open shell).
//!
//! The recipe follows the rig's commands as the fork's C API runs them: rounds as
//! `SmApiFilletEdgesPerEdge` (circular, constant radius), the chamfer as a linear
//! constant-radius section at offset 5 (equal setbacks across a right angle) with every
//! corner a bevel, then stitching; edges and faces are picked by the rig's points. Adapted
//! from tdamsma's reproduction, commit
//! <https://github.com/tdamsma/solidcore/commit/3a8d137bfddf857c42d1a4440f8bb2256ffa3bf2>.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use offset_fixture::{planar_faces_at, report, shell, volume};
use solidcore::blend::fillet_executive::session::FilletSession;
use solidcore::blend::fillet_solver::p1::FilletSurfaceGeneratorType;
use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_box;
use solidcore::query::api_queries::brep_is_manifold_solid;
use solidcore::topology::brep::p5::{stitch_and_orient, StitchAndOrientOptions};
use solidcore::{Brep, EdgeId, Kernel};

/// The closed-form volume of the part (`cavity_top: None`) or of its shell with the cavity
/// up to `cavity_top`. Every face is one profile in the (inset `d` from the wall, `z`) plane
/// swept round the rounded square of side 50 and corner radius 10, whose plane section at inset
/// `d` has area `A(d) = (50 - 2d)^2 - (4 - pi)(10 - d)^2`. The part's profile is the chamfer
/// `d = 5 - z`, the r4 fillet arc centred at `(4, 1 + 4 sqrt 2)` and the wall `d = 0` up to
/// 30. The cavity's is that profile offset 2 into the material: floor `z = 2`, chamfer
/// `d = 5 + 2 sqrt 2 - z`, the concentric r2 arc and the wall `d = 2`. Each piece of
/// `int A(d(z)) dz` is integrated exactly.
fn closed_form(cavity_top: Option<f64>) -> f64 {
    use std::f64::consts::{PI, SQRT_2};
    let c = 4.0 - PI;
    let (a0, a1, a2) = (2500.0 - 100.0 * c, -200.0 + 20.0 * c, 4.0 - c);
    let area = |d: f64| a0 + a1 * d + a2 * d * d;
    let antiderivative = |d: f64| a0 * d + a1 * d * d / 2.0 + a2 * d * d * d / 3.0;
    let zc = 1.0 + 4.0 * SQRT_2;
    // the fillet arc d = 4 - r cos t, z = zc + r sin t, t from -pi/4 to 0
    let arc = |r: f64| {
        let b = a0 + 4.0 * a1 + 16.0 * a2;
        r * (b * SQRT_2 / 2.0 - (a1 + 8.0 * a2) * r * (PI / 8.0 + 0.25)
            + a2 * r * r * 5.0 * SQRT_2 / 12.0)
    };
    // the chamfer d = k - z from z0 up to the arc of radius r, the arc, the wall d_wall to top
    let solid = |k: f64, z0: f64, r: f64, d_wall: f64, top: f64| {
        let z1 = zc - r / SQRT_2;
        antiderivative(k - z0) - antiderivative(k - z1) + arc(r) + area(d_wall) * (top - zc)
    };
    let part = solid(5.0, 0.0, 4.0, 0.0, 30.0);
    cavity_top.map_or(part, |top| {
        part - solid(5.0 + 2.0 * SQRT_2, 2.0, 2.0, 2.0, top)
    })
}

/// The edge of `b` nearest each pick (sampled along each edge's curve).
fn edges_at(k: &Kernel, b: &Brep, picks: &[[f64; 3]]) -> Vec<EdgeId> {
    picks
        .iter()
        .map(|&[x, y, z]| {
            let p = Point3::new(x, y, z);
            let (d, id) = b
                .edges()
                .iter()
                .filter_map(|(id, e)| {
                    let c = e.curve.as_ref()?;
                    let (t0, t1) = (e.interval.min(), e.interval.max());
                    let d = (0..=256)
                        .map(|i| {
                            let q = c
                                .evaluate_point(k, t0 + (t1 - t0) * f64::from(i) / 256.0)
                                .unwrap();
                            (q - p).length()
                        })
                        .fold(f64::INFINITY, f64::min);
                    Some((d, id))
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .unwrap();
            assert!(d < 1e-2, "no edge at {p:?} (nearest {d})");
            id
        })
        .collect()
}

fn round(k: &mut Kernel, b: &mut Brep, picks: &[[f64; 3]], radius: f64) {
    let edges = edges_at(k, b, picks);
    {
        let mut session = FilletSession::new(k, b).unwrap();
        session
            .prepare_per_edge_constant_radius(
                k,
                &edges,
                &vec![radius; edges.len()],
                &vec![FilletSurfaceGeneratorType::Circular; edges.len()],
                1,
                1.0,
            )
            .unwrap();
        session.do_fillet(k).unwrap();
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
}

fn chamfer(k: &mut Kernel, b: &mut Brep, picks: &[[f64; 3]], distance: f64) {
    let edges = edges_at(k, b, picks);
    {
        let mut session = FilletSession::new(k, b).unwrap();
        session
            .prepare_per_edge_constant_radius(
                k,
                &edges,
                &vec![distance; edges.len()],
                &vec![FilletSurfaceGeneratorType::Linear; edges.len()],
                1,
                1.0,
            )
            .unwrap();
        session.create_corners(k, None).unwrap();
        for corner in session.fillet_corners().unwrap() {
            session.set_corner_bevel(corner, true).unwrap();
        }
        session.do_filleting(k, None).unwrap();
    }
    stitch_and_orient(k, b, &StitchAndOrientOptions::default()).unwrap();
}

fn part(k: &mut Kernel) -> Brep {
    let mut b = create_box(k, Point3::new(0.0, 0.0, 0.0), 50.0, 50.0, 30.0).unwrap();
    round(
        k,
        &mut b,
        &[
            [0.0, 0.0, 15.0],
            [50.0, 0.0, 15.0],
            [0.0, 50.0, 15.0],
            [50.0, 50.0, 15.0],
        ],
        10.0,
    );
    let base = |z: f64| {
        [
            [25.0, 0.0, z],
            [50.0, 25.0, z],
            [25.0, 50.0, z],
            [0.0, 25.0, z],
            [2.929, 2.929, z],
            [47.071, 2.929, z],
            [47.071, 47.071, z],
            [2.929, 47.071, z],
        ]
    };
    chamfer(k, &mut b, &base(0.0), 5.0);
    round(k, &mut b, &base(5.0), 4.0);
    b.refresh_cached_topology();
    b
}

/// The part, with WR-421's fix on or off.
fn part_with(cone_on_arc: bool) -> (Kernel, Brep) {
    let mut k = Kernel::default();
    k.fixes.fillet_chamfer_cone_on_arc = cone_on_arc;
    let b = part(&mut k);
    (k, b)
}

/// The shell, open at the top or closed.
fn shell_of(k: &mut Kernel, b: Brep, open: bool) -> solidcore::Result<Brep> {
    let top = if open {
        planar_faces_at(k, &b, 2, 30.0)
    } else {
        Vec::new()
    };
    if open {
        assert_eq!(top.len(), 1, "the top face");
    }
    shell(k, b, -2.0, &top)
}

#[test]
fn issue_398_part_input() {
    let (k, mut b) = part_with(true);
    let got = volume(&k, &mut b).unwrap();
    eprintln!(
        "PROBE issue_398 input: faces={} volume={got}",
        b.face_list().len()
    );
    assert!(report(
        &k,
        "issue_398 input",
        Ok(b),
        Some(closed_form(None)),
        1e-6
    ));
}

#[test]
fn issue_398_open_top_shell() {
    let (mut k, b) = part_with(true);
    let result = shell_of(&mut k, b, true);
    assert!(report(
        &k,
        "issue_398 open top",
        result,
        Some(closed_form(Some(30.0))),
        1e-6
    ));
}

#[test]
fn issue_398_closed_shell() {
    let (mut k, b) = part_with(true);
    let result = shell_of(&mut k, b, false);
    assert!(report(
        &k,
        "issue_398 closed",
        result,
        Some(closed_form(Some(28.0))),
        1e-6
    ));
}

/// Without the fix the port keeps the pinned C++ reference's result: both shells report
/// success with the same body, which is not a manifold solid and has zero volume
/// (`plan/findings/offset.offset_executive_p2--shell-succeeds-with-non-manifold-body.md`).
#[test]
fn issue_398_reference_without_fix() {
    for open in [true, false] {
        let (mut k, b) = part_with(false);
        let mut got = shell_of(&mut k, b, open).unwrap();
        let counts = (
            got.face_list().len(),
            got.edge_list().len(),
            got.vertex_list().len(),
        );
        assert_eq!(counts, (72, 307, 236), "open={open}: the reference body");
        assert!(
            !brep_is_manifold_solid(&got).unwrap(),
            "open={open}: not a manifold solid"
        );
        assert_eq!(volume(&k, &mut got).unwrap(), 0.0, "open={open}");
    }
}

/// Narrows the open-top failure: the shell after each step of the recipe, open at the
/// top and closed (reported only).
#[test]
#[ignore = "diagnostic probe, run on demand"]
fn issue_398_by_step() {
    for steps in 1..=3 {
        for open in [true, false] {
            let mut k = Kernel::default();
            let mut b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 50.0, 50.0, 30.0).unwrap();
            round(
                &mut k,
                &mut b,
                &[
                    [0.0, 0.0, 15.0],
                    [50.0, 0.0, 15.0],
                    [0.0, 50.0, 15.0],
                    [50.0, 50.0, 15.0],
                ],
                10.0,
            );
            let base = |z: f64| {
                [
                    [25.0, 0.0, z],
                    [50.0, 25.0, z],
                    [25.0, 50.0, z],
                    [0.0, 25.0, z],
                    [2.929, 2.929, z],
                    [47.071, 2.929, z],
                    [47.071, 47.071, z],
                    [2.929, 47.071, z],
                ]
            };
            if steps >= 2 {
                chamfer(&mut k, &mut b, &base(0.0), 5.0);
            }
            if steps >= 3 {
                round(&mut k, &mut b, &base(5.0), 4.0);
            }
            b.refresh_cached_topology();
            let top = if open {
                planar_faces_at(&k, &b, 2, 30.0)
            } else {
                Vec::new()
            };
            let result = shell(&mut k, b, -2.0, &top);
            let _ = report(
                &k,
                &format!("issue_398 steps={steps} open={open}"),
                result,
                None,
                0.0,
            );
        }
    }
}
