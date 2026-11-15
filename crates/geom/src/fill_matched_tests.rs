use super::*;
use crate::fill_general::Across;
use std::f64::consts::FRAC_PI_2;

/// The face tolerance of the 60 x 40 x 20 box the corner is cut from (10⁻⁶ of its size).
const TOL: f64 = 7.5e-5;
const ANGLE: f64 = 0.05 * std::f64::consts::PI / 180.0;
/// Half a percent of the 1/5 per mm a rolling ball of radius 5 jumps by across its seams.
const CURVATURE: f64 = 1e-3;

fn spec() -> MatchedSpec {
    MatchedSpec { tol: TOL, angle_tol: ANGLE, curvature_tol: CURVATURE }
}

/// The hole a setback leaves at the corner of three rounds of radius `r` set back `s` (> r)
/// along each edge of a corner at the origin, the solid below it in every axis: three sections
/// of the rounds (quarter circles on the cylinders about the three edges) and three curves on the
/// faces between their ends, each a quarter circle about the point `s` back along both edges
/// (radius `s − r`), meeting the rounds' contact lines tangentially. Each side is G2 to its face:
/// a round leaves its section along its axis, where it is straight, a face anywhere; both have no
/// curvature across the hole.
fn setback_hole(r: f64, s: f64, g2: bool) -> impl FnMut(usize, &[f64]) -> Result<Vec<Station>, String> {
    let rho = s - r;
    move |k, ts| {
        ts.iter()
            .map(|t| {
                let a = t * FRAC_PI_2;
                let (sin, cos) = a.sin_cos();
                let (p, normal, leave) = match k {
                    0 => (Vec3::new(-s, -r + r * sin, -r + r * cos), Vec3::new(0.0, sin, cos), Vec3::X),
                    1 => (Vec3::new(-s + rho * sin, 0.0, -s + rho * cos), Vec3::Y, Vec3::new(sin, 0.0, cos)),
                    2 => (Vec3::new(-r + r * sin, -r + r * cos, -s), Vec3::new(sin, cos, 0.0), Vec3::Z),
                    3 => (Vec3::new(0.0, -s + rho * cos, -s + rho * sin), Vec3::X, Vec3::new(0.0, cos, sin)),
                    4 => (Vec3::new(-r + r * cos, -s, -r + r * sin), Vec3::new(cos, 0.0, sin), Vec3::Y),
                    5 => (Vec3::new(-s + rho * cos, -s + rho * sin, 0.0), Vec3::Z, Vec3::new(cos, sin, 0.0)),
                    _ => return Err(format!("no side {k}")),
                };
                let up = Vec3::new(1.0, 1.0, 1.0).normalized().unwrap_or(Vec3::Z);
                Ok(Station {
                    p,
                    up,
                    across: Some(Across { leave, normal, bend: g2.then_some(Vec3::ZERO), jet: None }),
                    corner: *t == 0.0,
                    clash: false,
                })
            })
            .collect()
    }
}

#[test]
fn a_setback_corner_hole_is_filled_g2_to_its_rounds_and_faces() {
    let f = fill_matched(6, &mut setback_hole(5.0, 8.0, true), &spec()).unwrap();
    eprintln!(
        "setback hole: spans {:?}, {} stations, deviation {:e}, normal {:.5}° (all {:.5}°), curvature {:e} (all {:e})",
        f.spans,
        f.stations,
        f.fill.deviation,
        f.normal_deviation.to_degrees(),
        f.normal_deviation_all.to_degrees(),
        f.curvature_mismatch,
        f.curvature_mismatch_all
    );
    assert!(f.fill.deviation <= TOL && f.normal_deviation <= ANGLE && f.curvature_mismatch <= CURVATURE);
    // The plane is the corner's diagonal one, by symmetry.
    let d = Vec3::new(1.0, 1.0, 1.0).normalized().unwrap();
    assert!(f.fill.normal.dot(d) > 1.0 - 1e-9, "{:?}", f.fill.normal);
    // The patch is the corner's cap: inside the corner cube's octant and within the hull of the
    // three rounds' tangent planes at 45° (the box's corner cut back by the rounds).
    let [du, dv] = f.fill.surface.domain();
    let mut highest = f64::NEG_INFINITY;
    for i in 0..=40 {
        for j in 0..=40 {
            let u = (du[0] + (du[1] - du[0]) * i as f64 / 40.0).min(du[1]);
            let v = (dv[0] + (dv[1] - dv[0]) * j as f64 / 40.0).min(dv[1]);
            let p = f.fill.surface.eval(u, v).unwrap();
            // Over the loop only: the box's corners outside the hole are not the patch.
            if p.x > -8.0 && p.y > -8.0 && p.z > -8.0 && p.x < 0.0 && p.y < 0.0 && p.z < 0.0 {
                highest = highest.max(p.dot(d));
            }
        }
    }
    eprintln!("setback hole: highest point along the diagonal {highest:.6}");
    assert!(highest < 0.0);
}

#[test]
fn a_setback_corner_hole_is_filled_g1_on_a_coarser_grid() {
    let f = fill_matched(6, &mut setback_hole(5.0, 8.0, false), &spec()).unwrap();
    eprintln!("setback hole G1: spans {:?}, deviation {:e}, normal {:.5}°", f.spans, f.fill.deviation, f.normal_deviation.to_degrees());
    assert!(f.fill.deviation <= TOL && f.normal_deviation <= ANGLE);
    // Flat to the second order along a curved plane's curve is what makes G2 the harder fill.
    let g2 = fill_matched(6, &mut setback_hole(5.0, 8.0, true), &spec()).unwrap();
    assert!(f.spans[0] < g2.spans[0], "{:?} {:?}", f.spans, g2.spans);
}

/// A hole in a cylinder of radius 5 about the x axis (its top line through the origin), the rim
/// over the circle of radius 2 about the z axis, G2 all round: the loop has no corner, and the
/// fill repeats the cylinder there.
#[test]
fn a_hole_in_a_cylinder_is_filled_with_the_cylinder_to_the_second_order() {
    let r = 5.0;
    let mut rim = move |_: usize, ts: &[f64]| -> Result<Vec<Station>, String> {
        ts.iter()
            .map(|t| {
                let a = t * std::f64::consts::TAU;
                let (x, y) = (2.0 * a.cos(), 2.0 * a.sin());
                let z = (r * r - y * y).sqrt() - r;
                let p = Vec3::new(x, y, z);
                let normal = Vec3::new(0.0, y, z + r).normalized().ok_or("normal")?;
                let tangent = Vec3::new(-a.sin(), a.cos(), -a.cos() * y / (z + r)).normalized().ok_or("tangent")?;
                // Into the hole, square to the rim in the cylinder.
                let leave = tangent.cross(normal).normalized().ok_or("leave")?;
                let leave = if leave.dot(Vec3::new(-x, -y, 0.0)) > 0.0 { leave } else { leave * -1.0 };
                // The cylinder's normal curvature along `leave`: its part across the axis, squared,
                // over the radius, bending away from the normal.
                let across = leave - Vec3::X * leave.x;
                let bend = normal * (-across.len2() / r);
                Ok(Station { p, up: normal, across: Some(Across { leave, normal, bend: Some(bend), jet: None }), corner: false, clash: false })
            })
            .collect()
    };
    let f = fill_matched(1, &mut rim, &spec()).unwrap();
    eprintln!("cylinder hole: spans {:?}, deviation {:e}, curvature {:e}", f.spans, f.fill.deviation, f.curvature_mismatch_all);
    assert!(f.fill.deviation <= TOL && f.normal_deviation_all <= ANGLE && f.curvature_mismatch_all <= CURVATURE);
}

#[test]
fn hostile_matched_fills_are_refused() {
    let bad = MatchedSpec { tol: 0.0, ..spec() };
    assert!(fill_matched(6, &mut setback_hole(5.0, 8.0, true), &bad).is_err());
    assert!(fill_matched(0, &mut setback_hole(5.0, 8.0, true), &spec()).is_err());
    // A face beside the loop standing along the fill's normal.
    let mut wall = |_: usize, ts: &[f64]| -> Result<Vec<Station>, String> {
        Ok(ts
            .iter()
            .map(|t| {
                let a = t * std::f64::consts::TAU / 3.0;
                let p = Vec3::new(a.cos(), a.sin(), 0.0);
                Station { p, up: Vec3::Z, across: Some(Across { leave: Vec3::Z, normal: p, bend: None, jet: None }), corner: false, clash: false }
            })
            .collect())
    };
    assert!(matches!(fill_matched(3, &mut wall, &spec()), Err(SplineError::Unsupported(_))));
}

/// How far the fill of `setback_hole(r, s)` rises past the planes it is G2 to, over the loop
/// (the corner is convex: all of it should lie under them), sampled.
fn over_the_planes(f: &MatchedFill, r: f64, s: f64) -> f64 {
    let mut st = setback_hole(r, s, true);
    let ts: Vec<f64> = (0..256).map(|i| i as f64 / 256.0).collect();
    let poly: Vec<[f64; 2]> = (0..6)
        .flat_map(|k| st(k, &ts).unwrap())
        .map(|q| {
            let d = q.p - f.fill.origin;
            [d.dot(f.fill.axes[0]), d.dot(f.fill.axes[1])]
        })
        .collect();
    let [du, dv] = f.fill.surface.domain();
    let n = 200;
    let mut over = f64::NEG_INFINITY;
    for i in 0..=n {
        for j in 0..=n {
            let u = (du[0] + (du[1] - du[0]) * i as f64 / n as f64).min(du[1]);
            let v = (dv[0] + (dv[1] - dv[0]) * j as f64 / n as f64).min(dv[1]);
            if crate::fill::inside_loop([u, v], &poly) {
                let p = f.fill.surface.eval(u, v).unwrap();
                over = over.max(p.x).max(p.y).max(p.z);
            }
        }
    }
    over
}

/// Set back from 1.5 to 3 times the radius the G2 fill stays under the planes it meets, but by
/// under a thousandth where it passes from a round's curvature to the plane's at the hole's
/// corners (SMLib's own setback patch rises past them beyond about twice the radius, by 0.002 at
/// 2.2 r and 0.25 at 3 r).
#[test]
fn a_setback_g2_fill_stays_under_its_planes() {
    for s in [7.5, 8.0, 10.0, 12.0, 15.0] {
        let f = fill_matched(6, &mut setback_hole(5.0, s, true), &spec()).unwrap();
        let over = over_the_planes(&f, 5.0, s);
        eprintln!("set back {s}: spans {:?}, curvature {:.2e}, over the planes {over:.2e}", f.spans, f.curvature_mismatch);
        assert!(over < 1e-3, "set back {s}: {over}");
    }
}
