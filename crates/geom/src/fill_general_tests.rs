use super::*;
use std::f64::consts::{PI, TAU};

const TOL: f64 = 1e-5;
const ANGLE: f64 = 0.05 * PI / 180.0;

/// The surface at fractions `(x, y)` of its domain.
fn at(s: &NurbsSurface, x: f64, y: f64) -> Vec3 {
    let [du, dv] = s.domain();
    s.eval((du[0] + (du[1] - du[0]) * x).min(du[1]), (dv[0] + (dv[1] - dv[0]) * y).min(dv[1])).unwrap()
}

fn spec(points: &[Vec3]) -> FillSpec<'_> {
    FillSpec { tol: TOL, angle_tol: ANGLE, crown: 0.0, points }
}

/// The radial and tangential unit vectors of a cylinder about z at angle `a`.
fn radial(a: f64) -> Vec3 {
    Vec3::new(a.cos(), a.sin(), 0.0)
}
fn around(a: f64) -> Vec3 {
    Vec3::new(-a.sin(), a.cos(), 0.0)
}

/// The open end of a tube about z (radius `r`, its wall below z = 0), one side: G1 (or G2) to
/// the wall, which leaves straight up.
fn tube_end(r: f64, g2: bool) -> impl FnMut(usize, &[f64]) -> Result<Vec<Station>, String> {
    move |_, ts| {
        Ok(ts
            .iter()
            .map(|t| {
                let a = t * TAU;
                let across = Across { leave: Vec3::Z, normal: radial(a), bend: g2.then_some(Vec3::ZERO), jet: None };
                Station { p: radial(a) * r, up: around(a).cross(Vec3::Z), across: Some(across), corner: false, clash: false }
            })
            .collect())
    }
}

/// A window in the wall of a cylinder about z (radius `r`): `a` either side of the x axis, `h`
/// high, its four sides G1 to the wall around it (the same cylinder).
fn window(r: f64, a: f64, h: f64) -> impl FnMut(usize, &[f64]) -> Result<Vec<Station>, String> {
    move |k, ts| {
        Ok(ts
            .iter()
            .map(|&t| {
                let (angle, z, tangent, leave) = match k {
                    0 => (-a + 2.0 * a * t, 0.0, around(-a + 2.0 * a * t), Vec3::Z),
                    1 => (a, h * t, Vec3::Z, around(a) * -1.0),
                    2 => (a - 2.0 * a * t, h, around(a - 2.0 * a * t) * -1.0, Vec3::Z * -1.0),
                    _ => (-a, h - h * t, Vec3::Z * -1.0, around(-a)),
                };
                let p = radial(angle) * r + Vec3::Z * z;
                let up = tangent.cross(leave);
                Station { p, up, across: Some(Across { leave, normal: radial(angle), bend: None, jet: None }), corner: t == 0.0, clash: false }
            })
            .collect())
    }
}

#[test]
fn a_tube_end_capped_g1_meets_the_wall_and_rises_over_it() {
    for g2 in [false, true] {
        let f = fill_general(1, &mut tube_end(10.0, g2), &spec(&[])).unwrap();
        eprintln!(
            "tube end (g2 {g2}): {:?} spans, {} stations, off the rim {:e}, normals within {:.4}°, curvature mismatch {:e}",
            f.spans,
            f.stations,
            f.deviation,
            f.normal_deviation.to_degrees(),
            f.curvature_mismatch
        );
        assert!(f.deviation <= TOL && f.normal_deviation <= ANGLE);
        // The middle rises over the rim (the cap leaves the wall upwards).
        let mid = at(&f.surface, 0.5, 0.5);
        eprintln!("the cap's middle: {mid:?}");
        assert!(mid.z > 2.0 && mid.x.hypot(mid.y) < 1.0, "{mid:?}");
    }
}

#[test]
fn a_window_in_a_cylinder_wrapping_past_half_way_fills_close_to_the_cylinder() {
    // 240° around: no plane sees it as a height field.
    let (r, a, h) = (10.0, 2.0 * PI / 3.0, 10.0);
    let f = fill_general(4, &mut window(r, a, h), &spec(&[])).unwrap();
    eprintln!("window: {:?} spans, off {:e}, normals within {:.4}°", f.spans, f.deviation, f.normal_deviation.to_degrees());
    assert!(f.deviation <= TOL && f.normal_deviation <= ANGLE);
    // Inside, the fill stays near the cylinder it was cut from (it is the least-bending surface
    // with the cylinder's boundary data, not the cylinder itself).
    let mut worst = 0.0f64;
    for k in 1..20 {
        for j in 1..20 {
            let (ang, z) = (-a + 2.0 * a * k as f64 / 20.0, h * j as f64 / 20.0);
            // The fill's point nearest this cylinder point, by a coarse search.
            let target = radial(ang) * r + Vec3::Z * z;
            let mut best = f64::INFINITY;
            for u in 0..=120 {
                for v in 0..=120 {
                    best = best.min(at(&f.surface, u as f64 / 120.0, v as f64 / 120.0).dist(target));
                }
            }
            worst = worst.max(best);
        }
    }
    eprintln!("window: the cylinder is within {worst} of the fill");
    assert!(worst < 0.25, "{worst}");
}

#[test]
fn a_planar_polygon_is_its_plane_and_a_point_lifts_it() {
    // An L-shaped hexagon (one reflex corner) in z = 0, its neighbours standing up around it.
    let c = [[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [10.0, 10.0], [10.0, 20.0], [0.0, 20.0]];
    let mut stations = |k: usize, ts: &[f64]| -> Result<Vec<Station>, String> {
        let (Some(a), Some(b)) = (c.get(k), c.get((k + 1) % 6)) else { return Err("no side".into()) };
        let tangent = Vec3::new(b[0] - a[0], b[1] - a[1], 0.0).normalized().ok_or("no tangent")?;
        let inward = Vec3::Z.cross(tangent);
        Ok(ts
            .iter()
            .map(|t| {
                let p = Vec3::new(a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, 0.0);
                Station {
                    p,
                    up: Vec3::Z,
                    across: Some(Across { leave: inward, normal: Vec3::Z, bend: None, jet: None }),
                    corner: *t == 0.0,
                    clash: false,
                }
            })
            .collect())
    };
    let f = fill_general(6, &mut stations, &spec(&[])).unwrap();
    let mut off = 0.0f64;
    for i in 0..=20 {
        for j in 0..=20 {
            off = off.max(at(&f.surface, i as f64 / 20.0, j as f64 / 20.0).z.abs());
        }
    }
    eprintln!("L: {:?} spans, the plane within {off:e}", f.spans);
    assert!(off < 1e-8, "{off}");
    // G0 with a point 3 above the inside of the L: the surface passes through it.
    let mut g0 = |k: usize, ts: &[f64]| stations(k, ts).map(|v| v.into_iter().map(|s| Station { across: None, ..s }).collect());
    let point = [Vec3::new(5.0, 5.0, 3.0)];
    let f = fill_general(6, &mut g0, &spec(&point)).unwrap();
    eprintln!("L through a point: {:?} spans, off the loop {:e}, off the point {:e}", f.spans, f.deviation, f.point_deviation);
    assert!(f.deviation <= TOL && f.point_deviation <= TOL);
}

#[test]
fn hostile_loops_and_specs_are_refused() {
    let ok = |s: &FillSpec| fill_general(1, &mut tube_end(10.0, false), s);
    for (tol, angle, crown) in
        [(0.0, ANGLE, 0.0), (f64::NAN, ANGLE, 0.0), (TOL, -1.0, 0.0), (TOL, ANGLE, f64::NAN), (TOL, ANGLE, -0.95), (TOL, ANGLE, 1e300)]
    {
        assert!(ok(&FillSpec { tol, angle_tol: angle, crown, points: &[] }).is_err(), "{tol} {angle} {crown}");
    }
    assert!(ok(&spec(&[Vec3::new(f64::NAN, 0.0, 0.0)])).is_err());
    assert!(ok(&spec(&vec![Vec3::ZERO; MAX_FILL_POINTS + 1])).is_err());
    // A point not over the inside of the loop.
    assert!(ok(&spec(&[Vec3::new(30.0, 0.0, 0.0)])).is_err());
    assert!(fill_general(0, &mut tube_end(10.0, false), &spec(&[])).is_err());
    assert!(fill_general(10_000, &mut tube_end(10.0, false), &spec(&[])).is_err());
    assert!(fill_general(1, &mut |_, _| Err("no".into()), &spec(&[])).is_err());
    assert!(
        fill_general(
            1,
            &mut |_, ts| Ok(vec![Station { p: Vec3::ZERO, up: Vec3::Z, across: None, corner: false, clash: false }; ts.len()]),
            &spec(&[])
        )
        .is_err()
    );
    // A loop that runs out and straight back.
    let mut spike = |_: usize, ts: &[f64]| -> Result<Vec<Station>, String> {
        Ok(ts
            .iter()
            .map(|t| Station { p: Vec3::X * (1.0 - (2.0 * t - 1.0f64).abs()) * 10.0, up: Vec3::Z, across: None, corner: false, clash: false })
            .collect())
    };
    assert!(fill_general(1, &mut spike, &spec(&[])).is_err());
    // A figure eight: its development crosses itself.
    let mut eight = |_: usize, ts: &[f64]| -> Result<Vec<Station>, String> {
        Ok(ts
            .iter()
            .map(|t| {
                let a = t * TAU;
                Station { p: Vec3::new(10.0 * a.sin(), 5.0 * (2.0 * a).sin(), 0.0), up: Vec3::Z, across: None, corner: false, clash: false }
            })
            .collect())
    };
    assert!(fill_general(1, &mut eight, &spec(&[])).is_err());
    // Huge and tiny loops do not panic.
    for scale in [1e300, 1e-300] {
        let mut s = |_: usize, ts: &[f64]| -> Result<Vec<Station>, String> {
            Ok(ts.iter().map(|t| Station { p: radial(t * TAU) * scale, up: Vec3::Z, across: None, corner: false, clash: false }).collect())
        };
        let _ = fill_general(1, &mut s, &spec(&[]));
    }
}
