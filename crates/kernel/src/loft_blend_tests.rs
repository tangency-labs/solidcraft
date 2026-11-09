//! Tests of the multi-section blend: analytic cross-sections given as frames, and the surface
//! measured back off the face it built (a grid of points, five-point differences along the run).

use super::*;
use crate::bridge::{bridge, sample_grid};
use solidcraft_geom::brep::eval::SurfaceEval;

/// The blend's surface: it is one face.
fn surface_of(body: &Body) -> SurfaceEval {
    SurfaceEval::new(&body.record().faces[0].surface).unwrap()
}

/// Position and the run direction's first three derivatives at `(t, v)` of the face, read with the
/// five-point differences `bridge_tests` reads a cross-section with. The samples run inwards from
/// the section (so nothing is asked of the face outside its own domain); `backwards` reads with
/// the parameter falling and turns the odd derivatives round, so every value is a derivative in
/// the run direction `u`.
fn run_derivs(body: &Body, t: f64, v: f64, backwards: bool, h: f64) -> (Vec3, Vec3, Vec3, Vec3) {
    let surf = surface_of(body);
    let dir = if backwards { -1.0 } else { 1.0 };
    let q: Vec<Vec3> = (0..5).map(|k| surf.point([t + dir * k as f64 * h, v]).unwrap()).collect();
    let d1 = (q[0] * -25.0 + q[1] * 48.0 + q[2] * -36.0 + q[3] * 16.0 + q[4] * -3.0) * (dir / (12.0 * h));
    let d2 = (q[0] * 35.0 + q[1] * -104.0 + q[2] * 114.0 + q[3] * -56.0 + q[4] * 11.0) * (1.0 / (12.0 * h * h));
    let d3 = (q[0] * -5.0 + q[1] * 18.0 + q[2] * -24.0 + q[3] * 14.0 + q[4] * -3.0) * (dir / (2.0 * h * h * h));
    (q[0], d1, d2, d3)
}

/// The geometric shape of a curve from its first three derivatives: the unit tangent, the
/// curvature vector and the arclength derivative of that curvature vector (`bridge`'s own
/// reading, kept local to the test).
fn curve_shape(d1: Vec3, d2: Vec3, d3: Vec3) -> (Vec3, Vec3, Vec3) {
    let sigma = d1.len();
    let t = d1 * (1.0 / sigma);
    let tp = d2 * (1.0 / sigma) - d1 * (d1.dot(d2) / (sigma * sigma * sigma));
    let nn = d2 - t * d2.dot(t);
    let np = d3 - t * d3.dot(t) - tp * d2.dot(t) - t * d2.dot(tp);
    let kappa = np * (sigma * sigma) - nn * (2.0 * d1.dot(d2));
    (t, nn * (1.0 / (sigma * sigma)), kappa * (1.0 / sigma.powi(5)))
}

/// The angle between two vectors, in degrees.
fn angle_deg(a: Vec3, b: Vec3) -> f64 {
    (a.normalized().unwrap().dot(b.normalized().unwrap())).clamp(-1.0, 1.0).acos().to_degrees()
}

/// A circle of radius `r` at height `z` about the run (the z axis), sampled at `n` stations. Each
/// frame leaves along `leave`; its `bend` is given per station as a function of the station's
/// outward radial direction (a support surface curving in the leave direction), and its change of
/// curvature is `change`.
fn circle_section(r: f64, z: f64, n: usize, leave: Vec3, bend: impl Fn(Vec3) -> Vec3, change: Vec3) -> Vec<Frame> {
    (0..n)
        .map(|s| {
            let a = std::f64::consts::TAU * s as f64 / n as f64;
            let p = Vec3::new(r * a.cos(), r * a.sin(), z);
            Frame { p, leave, bend: bend(Vec3::new(p.x / r, p.y / r, 0.0)), bend_change: change }
        })
        .collect()
}

/// `count` straight cross-sections along x, `10` apart in z and rising in order: the run is +z.
/// Every frame leaves along +z except the last one, which leaves back into the blend along −z as
/// a bridge's far boundary does. `bend` and `change` are the same at every station, and `bend` is
/// perpendicular to the run (the only shape a support surface can have along a direction it does
/// not bend around).
fn line_sections(count: usize, n: usize, bend: Vec3, change: Vec3) -> Vec<Vec<Frame>> {
    (0..count)
        .map(|i| {
            let leave = if i + 1 == count { Vec3::new(0.0, 0.0, -1.0) } else { Vec3::new(0.0, 0.0, 1.0) };
            (0..n).map(|s| Frame { p: Vec3::new(10.0 * s as f64 / (n - 1) as f64, 0.0, 10.0 * i as f64), leave, bend, bend_change: change }).collect()
        })
        .collect()
}

/// A point at arclength `s` along the perimeter of the rounded rectangle of half-width `a`,
/// half-height `b` and corner radius `r` (in the z = 0 plane, counter-clockwise from the right
/// edge's lower end).
fn rounded_rect_at(a: f64, b: f64, r: f64, s: f64) -> Vec3 {
    let (sa, sb) = ((a - r).max(0.0), (b - r).max(0.0));
    let quarter = std::f64::consts::FRAC_PI_2;
    let segs = [2.0 * sb, quarter * r, 2.0 * sa, quarter * r, 2.0 * sb, quarter * r, 2.0 * sa, quarter * r];
    let mut left = s;
    let mut k = 0;
    while k + 1 < segs.len() && left > segs[k] {
        left -= segs[k];
        k += 1;
    }
    let f = if segs[k] > 0.0 { left / segs[k] } else { 0.0 };
    let arc = |centre: Vec3, from: f64| {
        let ang = from + quarter * f;
        Vec3::new(centre.x + r * ang.cos(), centre.y + r * ang.sin(), 0.0)
    };
    match k {
        0 => Vec3::new(a, -sb + 2.0 * sb * f, 0.0),
        1 => arc(Vec3::new(sa, sb, 0.0), 0.0),
        2 => Vec3::new(sa - 2.0 * sa * f, b, 0.0),
        3 => arc(Vec3::new(-sa, sb, 0.0), quarter),
        4 => Vec3::new(-a, sb - 2.0 * sb * f, 0.0),
        5 => arc(Vec3::new(-sa, -sb, 0.0), std::f64::consts::PI),
        6 => Vec3::new(-sa + 2.0 * sa * f, -b, 0.0),
        _ => arc(Vec3::new(sa, -sb, 0.0), 3.0 * quarter),
    }
}

/// A rounded rectangle at height `z`, sampled at `n` even arclength steps around it: a section
/// that is not a scaled copy of the circles it can sit between, with a plan support (no bend).
fn rounded_rect_section(a: f64, b: f64, r: f64, z: f64, n: usize) -> Vec<Frame> {
    let total = 4.0 * ((a - r).max(0.0) + (b - r).max(0.0)) + 2.0 * std::f64::consts::PI * r;
    (0..n)
        .map(|s| {
            let p = rounded_rect_at(a, b, r, total * s as f64 / n as f64);
            Frame { p: Vec3::new(p.x, p.y, z), leave: Vec3::new(0.0, 0.0, 1.0), bend: Vec3::ZERO, bend_change: Vec3::ZERO }
        })
        .collect()
}

/// Two parallel circular cylinders of radius `r` 20 apart in y, leaving towards each other along
/// ∓y, each bending up towards its own axis and with a curvature that does not change as it goes
/// round (`bridge_tests`'s own case, for the two-section comparison).
fn valley_between_cylinders(count: usize, r: f64) -> (Vec<Frame>, Vec<Frame>) {
    let mut a = Vec::new();
    let mut b = Vec::new();
    for i in 0..count {
        let y = 10.0 * i as f64 / (count - 1) as f64;
        a.push(Frame {
            p: Vec3::new(0.0, y, 0.0),
            leave: Vec3::new(0.0, 1.0, 0.0),
            bend: Vec3::new(0.0, 0.0, 1.0 / r),
            bend_change: Vec3::new(0.0, -1.0 / (r * r), 0.0),
        });
        b.push(Frame {
            p: Vec3::new(0.0, y + 20.0, 0.0),
            leave: Vec3::new(0.0, -1.0, 0.0),
            bend: Vec3::new(0.0, 0.0, 1.0 / r),
            bend_change: Vec3::new(0.0, 1.0 / (r * r), 0.0),
        });
    }
    (a, b)
}

/// Three circles of different sizes at three heights, the middle one smaller: a shape that
/// changes as the face runs up it.
fn three_circles(n: usize) -> Vec<Vec<Frame>> {
    vec![
        // The first section is the equator of a sphere of its own radius: the support bends
        // inwards by 1/r along the run, and its curvature does not change as it goes round.
        circle_section(10.0, 0.0, n, Vec3::new(0.0, 0.0, 1.0), |radial| radial * -0.1, Vec3::new(0.0, 0.0, -0.01)),
        // The middle section's own shape data are ignored: the blend reads it from its neighbours.
        circle_section(8.0, 10.0, n, Vec3::new(0.0, 0.0, 1.0), |_| Vec3::ZERO, Vec3::ZERO),
        // The last section's frames leave back into the blend, along −z.
        circle_section(12.0, 20.0, n, Vec3::new(0.0, 0.0, -1.0), |_| Vec3::ZERO, Vec3::ZERO),
    ]
}

/// Every station of every section is on the face, within the tolerance a blend promises.
fn assert_runs_through(body: &Body, sections: &[Vec<Frame>], n: usize) {
    let grid = sample_grid(body, sections.len(), n).unwrap();
    for (k, section) in sections.iter().enumerate() {
        for (s, f) in section.iter().enumerate() {
            assert!(grid[k][s].dist(f.p) < 1e-9, "section {k} station {s}: {:?} vs {:?}", grid[k][s], f.p);
        }
    }
}

#[test]
fn three_sections_run_through_every_station() {
    let n = 9;
    let sections = three_circles(n);
    let body = loft_blend(&sections, [Continuity::G2, Continuity::G2], [0.0, 0.0], n).unwrap();
    assert_runs_through(&body, &sections, n);

    // The ends hold what their frames ask for: the leave direction is the tangent, and the
    // support's own curvature (1/10 inwards on the sphere, nothing on the plane) is the face's.
    for s in [0usize, 4, 8] {
        let v = s as f64 / (n - 1) as f64;
        // The stations run round the circle at even angles.
        let angle = std::f64::consts::TAU * s as f64 / n as f64;
        let radial = Vec3::new(angle.cos(), angle.sin(), 0.0);
        let (_, d1, d2, d3) = run_derivs(&body, 0.0, v, false, 1e-3);
        let (t, bend, _) = curve_shape(d1, d2, d3);
        assert!(angle_deg(t, Vec3::new(0.0, 0.0, 1.0)) < 0.01, "the face leaves along +z: {t:?}");
        assert!(bend.dist(radial * -0.1) < 1e-4, "the face leaves the sphere with its curvature: {bend:?}");
        // The last section's frames leave back into the blend, so read the shape that way round.
        let (_, d1, d2, d3) = run_derivs(&body, 1.0, v, true, 1e-3);
        let (t, bend, _) = curve_shape(-d1, d2, -d3);
        assert!(angle_deg(t, Vec3::new(0.0, 0.0, -1.0)) < 0.01, "and arrives along the far leave: {t:?}");
        assert!(bend.len() < 1e-3, "the last section's support is flat: {bend:?}");
    }

    // Three sections: the one interior joint has no fourth station to take a third difference
    // over, so it holds position, tangent and curvature — C² — and both sides agree.
    for s in [0usize, 4, 8] {
        let v = s as f64 / (n - 1) as f64;
        let (from_left, left1, left2, left3) = run_derivs(&body, 0.5, v, false, 1e-4);
        let (from_right, right1, right2, right3) = run_derivs(&body, 0.5, v, true, 1e-4);
        assert!(from_left.dist(from_right) < 1e-9, "the joint is on the middle section");
        assert!((left1 - right1).len() < 1e-7 * (1.0 + left1.len()), "the tangent is continuous: {left1:?} vs {right1:?}");
        assert!((left2 - right2).len() < 1e-4 * (1.0 + left2.len()), "the curvature is continuous: {left2:?} vs {right2:?}");
        // But the change of curvature is not: a three-point stencil carries no third difference,
        // so the joint is C² and not C³, and the difference stays whatever the step size is.
        assert!((left3 - right3).len() > 0.05 * (1.0 + left3.len()), "a C² joint is not C³: {left3:?} vs {right3:?}");
        // What it holds is the sections' own finite differences along the run: the secant of the
        // two neighbours for the tangent, their second difference for the curvature.
        let p = |k: usize| sections[k][s].p;
        let secant = p(2) - p(0);
        let second = (p(2) - p(1) * 2.0 + p(0)) * 4.0;
        assert!((left1 - secant).len() < 1e-7 * (1.0 + secant.len()), "the tangent is the neighbours' secant: {left1:?} vs {secant:?}");
        assert!((left2 - second).len() < 1e-4 * (1.0 + second.len()), "the curvature is the neighbours' second difference: {left2:?} vs {second:?}");
    }
}

#[test]
fn the_ends_hold_the_shape_their_frames_ask_for() {
    let stations = 7;
    let bend = Vec3::new(0.0, 0.2, 0.0);
    let change = Vec3::new(0.0, 0.5, 0.0);
    let sections = line_sections(3, stations, bend, change);
    let z = |s: f64| s * 10.0;

    // G1 holds the tangent and nothing more.
    let g1 = loft_blend(&sections, [Continuity::G1, Continuity::G1], [0.0, 0.0], stations).unwrap();
    for (v, x) in [(0.0, 0.0), (0.5, 5.0), (1.0, 10.0)] {
        let (p, d1, d2, d3) = run_derivs(&g1, 0.0, v, false, 1e-3);
        let (t, _, _) = curve_shape(d1, d2, d3);
        assert!(p.dist(Vec3::new(x, 0.0, 0.0)) < 1e-9, "the face starts on the first section: {p:?}");
        assert!(angle_deg(t, Vec3::new(0.0, 0.0, 1.0)) < 0.01, "a G1 blend leaves along leave: {t:?}");
    }

    // G2 holds the tangent and the support's curvature, at both ends.
    let g2 = loft_blend(&sections, [Continuity::G2, Continuity::G2], [0.0, 0.0], stations).unwrap();
    for (v, x) in [(0.0, 0.0), (0.5, 5.0), (1.0, 10.0)] {
        let (p, d1, d2, d3) = run_derivs(&g2, 0.0, v, false, 1e-3);
        let (t, k, _) = curve_shape(d1, d2, d3);
        assert!(p.dist(Vec3::new(x, 0.0, 0.0)) < 1e-9, "the face starts on the first section: {p:?}");
        assert!(angle_deg(t, Vec3::new(0.0, 0.0, 1.0)) < 0.01, "a G2 blend leaves along leave: {t:?}");
        assert!(k.dist(bend) / bend.len() < 1e-3, "a G2 blend holds the support's curvature: {k:?} vs {bend:?}");
        let (p, d1, d2, d3) = run_derivs(&g2, 1.0, v, true, 1e-3);
        let (t, k, c) = curve_shape(-d1, d2, -d3);
        assert!(p.dist(Vec3::new(x, 0.0, z(2.0))) < 1e-9, "the face ends on the last section: {p:?}");
        assert!(angle_deg(t, Vec3::new(0.0, 0.0, -1.0)) < 0.01, "reading back, it arrives along the far leave: {t:?}");
        assert!(k.dist(bend) / bend.len() < 1e-3, "the far end holds the curvature too: {k:?}");
        assert!(c.len() < 1e3);
    }
    // A G2 blend leaves the change of curvature free: moving the frames' own value does not move
    // the face at the first section, while a G3 blend follows it.
    let (_, d1, d2, d3) = run_derivs(&g2, 0.0, 0.5, false, 1e-3);
    let free = curve_shape(d1, d2, d3).2;
    let moved = line_sections(3, stations, bend, change + Vec3::new(0.0, 1.0, 0.0));
    let g2_moved = loft_blend(&moved, [Continuity::G2, Continuity::G2], [0.0, 0.0], stations).unwrap();
    let (_, d1, d2, d3) = run_derivs(&g2_moved, 0.0, 0.5, false, 1e-3);
    assert!(curve_shape(d1, d2, d3).2.dist(free) < 1e-9, "a G2 blend does not read the far frame's change");

    // G3 holds the change of curvature as well: its normal part is what a surface match can hold,
    // and here the frame's own change is normal (along its curvature vector).
    let g3 = loft_blend(&sections, [Continuity::G3, Continuity::G3], [0.0, 0.0], stations).unwrap();
    for (v, x) in [(0.0, 0.0), (0.5, 5.0), (1.0, 10.0)] {
        let (_, d1, d2, d3) = run_derivs(&g3, 0.0, v, false, 1e-3);
        let (t, k, c) = curve_shape(d1, d2, d3);
        assert!(angle_deg(t, Vec3::new(0.0, 0.0, 1.0)) < 0.01);
        assert!(k.dist(bend) / bend.len() < 1e-3, "a G3 blend still holds the curvature: {k:?}");
        assert!((c.y - change.y).abs() < 1e-3, "a G3 blend holds the change of curvature: {c:?} vs {change:?}");
        let (p, d1, d2, d3) = run_derivs(&g3, 1.0, v, true, 1e-3);
        let (t, k, c) = curve_shape(-d1, d2, -d3);
        assert!(angle_deg(t, Vec3::new(0.0, 0.0, -1.0)) < 0.01);
        assert!(k.dist(bend) / bend.len() < 1e-3);
        assert!((c.y - change.y).abs() < 1e-3, "and the far end holds it too: {c:?} vs {change:?}");
        assert!(p.dist(Vec3::new(x, 0.0, z(2.0))) < 1e-9);
    }
    // With G3, moving the frames' own value does move the face there.
    let g3_moved = loft_blend(&moved, [Continuity::G3, Continuity::G3], [0.0, 0.0], stations).unwrap();
    let (_, d1, d2, d3) = run_derivs(&g3_moved, 0.0, 0.5, false, 1e-3);
    assert!((curve_shape(d1, d2, d3).2.y - free.y).abs() > 0.5, "a G3 blend follows the frame's change");
}

#[test]
fn four_sections_join_at_c3() {
    let n = 9;
    let sections: Vec<Vec<Frame>> = [(10.0, 0.0), (9.0, 10.0), (8.0, 20.0), (11.0, 30.0)]
        .iter()
        .enumerate()
        .map(|(i, &(r, z))| {
            let leave = if i == 3 { Vec3::new(0.0, 0.0, -1.0) } else { Vec3::new(0.0, 0.0, 1.0) };
            circle_section(r, z, n, leave, |_| Vec3::ZERO, Vec3::ZERO)
        })
        .collect();
    let body = loft_blend(&sections, [Continuity::G3, Continuity::G3], [0.0, 0.0], n).unwrap();
    assert_runs_through(&body, &sections, n);

    // Every joint now has a fourth station for the third difference, so the two sides agree in
    // the tangent, the curvature and its change: the joints are C³.
    let h = 1.0 / 3.0;
    for (i, s) in [(1usize, 0usize), (1, 4), (1, 8), (2, 0), (2, 4), (2, 8)] {
        let t = i as f64 / 3.0;
        let v = s as f64 / (n - 1) as f64;
        let (_, left1, left2, left3) = run_derivs(&body, t, v, true, 1e-4);
        let (_, right1, right2, right3) = run_derivs(&body, t, v, false, 1e-4);
        assert!((left1 - right1).len() < 1e-7 * (1.0 + left1.len()), "joint {i} station {s}: the tangent agrees");
        assert!((left2 - right2).len() < 1e-4 * (1.0 + left2.len()), "joint {i} station {s}: the curvature agrees");
        // The change of curvature agrees too, as far as finite differences can tell: its
        // mismatch falls away with the step size, so the joint is C³, not just C².
        assert!(
            (left3 - right3).len() < 2e-2 * (1.0 + left3.len()),
            "joint {i} station {s}: the change of curvature agrees: {left3:?} vs {right3:?}"
        );
        // What they hold are the sections' own differences along the run.
        let p = |k: usize| sections[k][s].p;
        let d1 = (p(i + 1) - p(i - 1)) * (1.0 / (2.0 * h));
        let d2 = (p(i + 1) - p(i) * 2.0 + p(i - 1)) * (1.0 / (h * h));
        let d3 = if i == 1 {
            (p(0) * -1.0 + p(1) * 3.0 - p(2) * 3.0 + p(3)) * (1.0 / (h * h * h))
        } else {
            (p(0) - p(1) * 3.0 + p(2) * 3.0 - p(3)) * (1.0 / (h * h * h))
        };
        assert!((left1 - d1).len() < 1e-7 * (1.0 + d1.len()), "the tangent is the secant: {left1:?} vs {d1:?}");
        assert!((left2 - d2).len() < 1e-4 * (1.0 + d2.len()), "the curvature is the second difference: {left2:?} vs {d2:?}");
        assert!((left3 - d3).len() < 2e-2 * (1.0 + d3.len()), "the change of curvature is the third difference: {left3:?} vs {d3:?}");
    }
}

#[test]
fn a_shape_that_is_not_a_scaled_copy_still_runs_through() {
    let n = 17;
    let sections = vec![
        circle_section(10.0, 0.0, n, Vec3::new(0.0, 0.0, 1.0), |_| Vec3::ZERO, Vec3::ZERO),
        rounded_rect_section(9.0, 7.0, 2.5, 10.0, n),
        circle_section(8.0, 20.0, n, Vec3::new(0.0, 0.0, -1.0), |_| Vec3::ZERO, Vec3::ZERO),
    ];
    let body = loft_blend(&sections, [Continuity::G1, Continuity::G1], [0.0, 0.0], n).unwrap();
    assert_runs_through(&body, &sections, n);

    // The interior joint is still smooth to C² and holds the neighbours' own estimates, whatever
    // the sections' shapes are.
    for s in [0usize, 4, 8, 12, 16] {
        let v = s as f64 / (n - 1) as f64;
        let (_, left1, left2, _) = run_derivs(&body, 0.5, v, false, 2e-4);
        let (_, right1, right2, _) = run_derivs(&body, 0.5, v, true, 2e-4);
        assert!((left1 - right1).len() < 1e-6 * (1.0 + left1.len()), "station {s}: the tangent is continuous");
        assert!((left2 - right2).len() < 1e-6 * (1.0 + left2.len()), "station {s}: the curvature is continuous");
        let p = |k: usize| sections[k][s].p;
        let secant = p(2) - p(0);
        let second = (p(2) - p(1) * 2.0 + p(0)) * 4.0;
        assert!((left1 - secant).len() < 1e-6 * (1.0 + secant.len()), "station {s}: the tangent is the secant: {left1:?} vs {secant:?}");
        assert!((left2 - second).len() < 1e-6 * (1.0 + second.len()), "station {s}: the curvature is the second difference: {left2:?} vs {second:?}");
    }
}

#[test]
fn a_two_section_blend_is_the_bridge() {
    let (a, b) = valley_between_cylinders(9, 10.0);
    let made = loft_blend(&[a.clone(), b.clone()], [Continuity::G2, Continuity::G2], [0.0, 0.0], 9).unwrap();
    let other = bridge(&a, &b, [Continuity::G2, Continuity::G2], [0.0, 0.0]).unwrap();
    // The blend runs with the sections, the bridge crosses from one boundary to the other: the
    // same face, its two parameters the other way round.
    for (run, station) in [(0.0, 0.0), (0.25, 0.3), (0.5, 0.7), (0.75, 0.1), (1.0, 1.0)] {
        let x = surface_of(&made).point([run, station]).unwrap();
        let y = surface_of(&other).point([station, run]).unwrap();
        assert!(x.dist(y) < 1e-9, "run {run} station {station}: {x:?} vs {y:?}");
    }
    let grid = sample_grid(&other, 9, 7).unwrap();
    for (row, frame) in grid.iter().zip(&a) {
        assert!(row[0].dist(frame.p) < 1e-9);
    }
}

#[test]
fn crown_shapes_the_middle_without_moving_the_joints() {
    let n = 9;
    let sections = three_circles(n);
    let mut heights = Vec::new();
    let mut joints = Vec::new();
    for crown in [-0.5, 0.0, 1.0, 2.0] {
        let body = loft_blend(&sections, [Continuity::G2, Continuity::G2], [crown, crown], n).unwrap();
        // The interior joint holds the sections' own estimates whatever the crown does: those do
        // not depend on it.
        let (p, d1, d2, _) = run_derivs(&body, 0.5, 0.5, false, 2e-4);
        assert!(p.dist(sections[1][4].p) < 1e-9, "the crown left the joint where the sections put it");
        joints.push((d1, d2));
        // Halfway up the first span is where a fuller crown reaches further.
        heights.push(surface_of(&body).point([0.25, 0.5]).unwrap().z);
        assert_runs_through(&body, &sections, n);
    }
    for (d1, d2) in &joints {
        let (base1, base2) = joints[0];
        assert!((*d1 - base1).len() < 1e-4 * (1.0 + base1.len()), "the crown left the joint's tangent alone: {d1:?}");
        assert!((*d2 - base2).len() < 1e-4 * (1.0 + base2.len()), "the crown left the joint's curvature alone: {d2:?}");
    }
    let steps: Vec<f64> = heights.windows(2).map(|w| w[1] - w[0]).collect();
    assert!(steps.iter().all(|d| d.abs() > 1e-6 && d.signum() == steps[0].signum()), "a fuller crown moves the middle one way: {heights:?}");
}

#[test]
fn the_blend_refuses_what_it_cannot_hold() {
    let n = 5;
    let orders = [Continuity::G1, Continuity::G1];
    let good = three_circles(n);
    assert!(loft_blend(&good, orders, [0.0, 0.0], n).is_ok());

    // Fewer than two sections, and more than the cap.
    assert!(loft_blend(&good[..1], orders, [0.0, 0.0], n).is_err(), "one section is no blend");
    let many: Vec<Vec<Frame>> =
        (0..=MAX_BLEND_SECTIONS).map(|i| circle_section(10.0, i as f64, 3, Vec3::new(0.0, 0.0, 1.0), |_| Vec3::ZERO, Vec3::ZERO)).collect();
    assert!(loft_blend(&many, orders, [0.0, 0.0], 3).is_err(), "more than {MAX_BLEND_SECTIONS} sections");

    // Station counts that do not match each other, or the count asked for.
    let mut unequal = good.clone();
    unequal[1] = unequal[1][..n - 1].to_vec();
    assert!(loft_blend(&unequal, orders, [0.0, 0.0], n).is_err(), "unequal station counts");
    assert!(loft_blend(&good, orders, [0.0, 0.0], n + 1).is_err(), "the samples must match the sections");
    for samples in [0usize, 1, MAX_BLEND_STATIONS + 1] {
        assert!(loft_blend(&good, orders, [0.0, 0.0], samples).is_err(), "samples {samples}");
    }

    // A crown outside its range, and one that is not a number.
    for crown in [f64::NAN, 5.0, -1.0, f64::INFINITY] {
        assert!(loft_blend(&good, orders, [crown, crown], n).is_err(), "crown {crown}");
    }
    assert!(loft_blend(&good, orders, [0.0, f64::NAN], n).is_err());

    // A frame that is not a number, wherever it is not, and a leave direction with no direction.
    for pick in 0..4 {
        let mut bad = good.clone();
        let frame = bad[1][2];
        bad[1][2] = match pick {
            0 => Frame { p: Vec3::new(f64::NAN, 0.0, 0.0), ..frame },
            1 => Frame { leave: Vec3::new(f64::INFINITY, 0.0, 0.0), ..frame },
            2 => Frame { bend: Vec3::new(0.0, f64::NAN, 0.0), ..frame },
            _ => Frame { bend_change: Vec3::new(0.0, 0.0, f64::NAN), ..frame },
        };
        assert!(loft_blend(&bad, orders, [0.0, 0.0], n).is_err(), "a non-finite frame {pick}");
    }
    let mut no_leave = good.clone();
    no_leave[1][2].leave = Vec3::ZERO;
    assert!(loft_blend(&no_leave, orders, [0.0, 0.0], n).is_err(), "a leave direction with no direction");

    // Two sections that meet at a station: no room for a blend.
    let mut meeting = good.clone();
    meeting[1] = meeting[0].clone();
    assert!(loft_blend(&meeting, orders, [0.0, 0.0], n).is_err(), "sections that meet");
    let mut touching = good.clone();
    touching[1][2].p = touching[0][2].p;
    assert!(loft_blend(&touching, orders, [0.0, 0.0], n).is_err(), "a station where the sections meet");

    // Every order from G0 to G3 builds, alone and together, and so does the largest blend.
    for (near, far) in [
        (Continuity::G0, Continuity::G0),
        (Continuity::G1, Continuity::G0),
        (Continuity::G0, Continuity::G3),
        (Continuity::G3, Continuity::G3),
        (Continuity::G2, Continuity::G3),
    ] {
        let body = loft_blend(&good, [near, far], [0.0, 0.0], n).unwrap();
        assert_runs_through(&body, &good, n);
    }
    let full: Vec<Vec<Frame>> =
        (0..MAX_BLEND_SECTIONS).map(|i| circle_section(10.0, i as f64, 5, Vec3::new(0.0, 0.0, 1.0), |_| Vec3::ZERO, Vec3::ZERO)).collect();
    let body = loft_blend(&full, [Continuity::G3, Continuity::G3], [0.0, 0.0], 5).unwrap();
    assert_runs_through(&body, &full, 5);
}
