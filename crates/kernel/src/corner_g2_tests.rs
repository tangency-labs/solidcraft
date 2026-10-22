//! What the G2 corner patch has to prove on a corner whose answer is known analytically: the box.

use super::*;
use std::f64::consts::FRAC_PI_2;

/// One seam of a box corner: a quarter circle about `centre` from direction `u` to direction `w`,
/// with `leave` the way the patch leaves the blend's surface. A fillet is flat along its own axis,
/// so its curvature across the seam is nothing.
fn seam(centre: Vec3, u: Vec3, w: Vec3, leave: Vec3, r: f64, n: usize) -> Vec<Frame> {
    (0..n)
        .map(|i| {
            let theta = FRAC_PI_2 * i as f64 / (n - 1) as f64;
            Frame { p: centre + (u * theta.cos() + w * theta.sin()) * r, leave, bend: Vec3::ZERO, bend_change: Vec3::ZERO }
        })
        .collect()
}

/// A box corner at the origin (material x, y, z ≥ 0) with three fillets of radius `r`: the three
/// section curves where the fillets stop, meeting at (0,r,r), (r,0,r) and (r,r,0), and the middle of
/// the ball's octant a rolling ball would leave there.
fn box_corner(r: f64, n: usize) -> (Vec<Vec<Frame>>, Vec3, Vec3) {
    let c = Vec3::new(r, r, r);
    let (a, b, d) = (Vec3::new(0.0, r, r), Vec3::new(r, 0.0, r), Vec3::new(r, r, 0.0));
    let dir = |p: Vec3| (p - c).normalized().unwrap_or(Vec3::X);
    let sides = vec![
        // A → B on the z edge's cylinder (flat along z), B → C on the x edge's, C → A on the y
        // edge's; each patch leaves the blend towards the material the corner caps.
        seam(c, dir(a), dir(b), Vec3::new(0.0, 0.0, -1.0), r, n),
        seam(c, dir(b), dir(d), Vec3::new(-1.0, 0.0, 0.0), r, n),
        seam(c, dir(d), dir(a), Vec3::new(0.0, -1.0, 0.0), r, n),
    ];
    let apex = c - Vec3::new(1.0, 1.0, 1.0).normalized().unwrap_or(Vec3::Z) * r;
    (sides, apex, c)
}

/// The rolling-ball corner the kernel builds today: the sphere through the same three seams, made
/// as a sphere record about the ball's centre, its axis along the corner's diagonal.
fn rolling_ball(r: f64, c: Vec3) -> b::Surface {
    let axis = Vec3::new(1.0, 1.0, 1.0).normalized().unwrap_or(Vec3::Z);
    let side = axis.any_perp().normalized().unwrap();
    let y = axis.cross(side);
    b::Surface::Sphere { frame: b::Frame { origin: [c.x, c.y, c.z], x_axis: [side.x, side.y, side.z], y_axis: [y.x, y.y, y.z] }, radius: r }
}

/// The stations clear of the corners. Two blends meet at a corner with their seams 90° apart and
/// ask for different curvature at that one point, which no single patch can hold, and the corner's
/// own spline rounds the seam there; both effects fade within a few stations, so the measurement
/// starts clear of them.
fn interior(sides: &[Vec<Frame>]) -> Vec<Vec<Frame>> {
    sides.iter().map(|s| s.iter().skip(4).take(s.len().saturating_sub(8)).copied().collect()).collect()
}

#[test]
fn a_box_corner_is_curvature_continuous_with_every_blend() {
    let r = 2.0;
    let (sides, apex, centre) = box_corner(r, 25);
    let surface = corner_g2_patch(&sides, apex).unwrap().surface;
    let inner = interior(&sides);
    let (gap, angle, curvature) = seam_error(&surface, &inner, 1.0 / r).unwrap();
    println!("G2 corner: gap {gap:e}, normal angle {angle:e} rad, curvature {curvature:e} (relative to 1/r)");
    assert!(gap <= r * 1e-5, "the patch is {gap} from the seam");
    // The stated tolerances: the normal angle within 0.01° and the curvature across the seam within
    // 1% of the blend's own (here the fillet is flat across the seam, so that is 1% of 1/r).
    assert!(angle < 0.01f64.to_radians(), "normal angle {angle} rad");
    assert!(curvature < 1e-2, "curvature across the seam is {curvature} of the blend's");

    // The same measurement on the rolling-ball corner the kernel builds today: G1 (angle nought)
    // but the ball's own 1/r across each seam, which is the whole of the blend's curvature.
    let ball = rolling_ball(r, centre);
    let (ball_gap, ball_angle, ball_curvature) = seam_error(&ball, &inner, 1.0 / r).unwrap();
    println!("rolling ball: gap {ball_gap:e}, normal angle {ball_angle:e} rad, curvature {ball_curvature:e}");
    assert!(ball_angle < 0.01f64.to_radians(), "the ball is tangent to each blend");
    assert!(ball_curvature > 0.5, "the ball keeps its own curvature across the seam: {ball_curvature}");
    assert!(curvature * 20.0 < ball_curvature, "the G2 patch has to be far closer than the ball");
}

#[test]
fn a_corner_meets_three_blends_of_different_shape_apart() {
    // Blends that differ along their seams: a fillet of radius 2 next to rounder walls, each asking
    // for its own curvature across the seam, at its own direction.
    let r = 2.0;
    let (mut sides, apex, _) = box_corner(r, 33);
    let bends = [0.25, 0.4, 0.6];
    for (i, bend) in bends.iter().enumerate() {
        let Some(side) = sides.get_mut(i) else { continue };
        for f in side.iter_mut() {
            // Across the seam, perpendicular to both the leave direction and the seam.
            f.bend = f.leave.any_perp() * *bend;
        }
    }
    let surface = corner_g2_patch(&sides, apex).unwrap().surface;
    let inner = interior(&sides);
    let (gap, angle, curvature) = seam_error(&surface, &inner, bends.iter().copied().fold(0.0, f64::max)).unwrap();
    println!("three different blends: gap {gap:e}, angle {angle:e} rad, curvature {curvature:e}");
    assert!(gap <= r * 1e-6, "{gap}");
    assert!(angle < 0.01f64.to_radians(), "{angle}");
    assert!(curvature < 1e-2, "each blend's own curvature across the seam: {curvature}");
}

#[test]
fn a_g2_corner_refuses_what_it_cannot_hold() {
    let r = 2.0;
    let (sides, apex, _) = box_corner(r, 9);
    assert!(corner_g2_patch(&sides, apex).is_ok());

    // Two blends (the ball's patch still fills that corner), or one.
    assert!(corner_g2_patch(&sides[..2], apex).is_err());
    assert!(corner_g2_patch(&sides[..1], apex).is_err());
    assert!(corner_g2_patch(&[], apex).is_err());

    // Too few stations to interpolate a cubic through, and too many to work with.
    let thin: Vec<Vec<Frame>> = sides.iter().map(|s| s[..3].to_vec()).collect();
    assert!(corner_g2_patch(&thin, apex).is_err());
    let fat: Vec<Vec<Frame>> = sides.iter().map(|s| s[..1].iter().cycle().take(MAX_STATIONS + 1).copied().collect()).collect();
    assert!(corner_g2_patch(&fat, apex).is_err());

    // A vertex the construction cannot hold: the section curves do not meet at their corners.
    let mut apart = sides.clone();
    if let Some(first) = apart.first_mut().and_then(|s| s.first_mut()) {
        first.p += Vec3::new(0.5, 0.0, 0.0);
    }
    assert!(corner_g2_patch(&apart, apex).is_err());

    // A section curve that runs along the seam, so the patch has no direction to leave it in.
    let along: Vec<Vec<Frame>> = sides
        .iter()
        .map(|s| {
            s.windows(2)
                .map(|w| {
                    let d = w.first().zip(w.get(1)).map(|(a, b)| (b.p - a.p).normalized().unwrap_or(Vec3::X)).unwrap_or(Vec3::X);
                    Frame { leave: d, ..w.first().copied().unwrap_or(s[0]) }
                })
                .chain(s.last().copied())
                .collect()
        })
        .collect();
    assert!(corner_g2_patch(&along, apex).is_err());

    // Hostile numbers never panic.
    let mut nan = sides.clone();
    if let Some(f) = nan.first_mut().and_then(|s| s.get_mut(1)) {
        f.p = Vec3::new(f64::NAN, 0.0, 0.0);
    }
    assert!(corner_g2_patch(&nan, apex).is_err());
    assert!(corner_g2_patch(&sides, Vec3::new(f64::INFINITY, 0.0, 0.0)).is_err());
    // A corner with no room: every station on the apex.
    let on_apex: Vec<Vec<Frame>> = sides.iter().map(|s| s.iter().map(|f| Frame { p: apex, ..*f }).collect()).collect();
    assert!(corner_g2_patch(&on_apex, apex).is_err());
}
