//! Edge detail for the view: a body's edges drawn with fewer line segments where it is small on
//! the screen. An edge is a polyline of the display mesh, made at the chosen display quality's
//! chord whatever the body's size on screen, so a far or small part (a screw's threads, a fillet
//! on a small part) can draw hundreds of segments inside a few pixels. Every one is a quad the GPU
//! rasterizes and blends; a large assembly drew more edge segments than the view has pixels.
//!
//! Each body gets a tolerance in millimetres ([`tolerance`]): the most its simplified edges may
//! move, kept below [`TOLERANCE_PX`] of a pixel at the nearest the body can come to the eye while
//! the view orbits its target, so orbiting never brings a body close enough to show the
//! difference. The edges keep their ends (edges stay joined, vertices stay where they are).
//! Display only: picking, measuring, exports and the B-rep use the reference mesh.

use solidcore_record::Vec3;

/// The most a simplified edge may move on the screen (physical pixels). The edges are drawn about
/// 1.3 px wide, anti-aliased.
pub(crate) const TOLERANCE_PX: f64 = 0.25;

/// The tolerance step (a power of two, in millimetres: `2^step`) for a body drawn at most
/// `px_per_mm` pixels per millimetre, or `None` where it cannot be known (no scale).
pub(crate) fn step_for(px_per_mm: f64) -> Option<i8> {
    if !(px_per_mm.is_finite() && px_per_mm > 0.0) {
        return None;
    }
    let step = (TOLERANCE_PX / px_per_mm).log2().floor();
    step.is_finite().then(|| step.clamp(-60.0, 60.0) as i8)
}

/// The step to draw at, from the one drawn now (`current`) and the one the view asks for
/// (`wanted`): a finer one at once (the edges would show their corners), a coarser one only once it
/// is two steps coarser, so a small zoom out does not build the edges again.
pub(crate) fn hysteresis(current: Option<i8>, wanted: i8) -> i8 {
    match current {
        Some(c) if c <= wanted && c >= wanted.saturating_sub(1) => c,
        _ => wanted,
    }
}

/// The tolerance of a step, in millimetres.
pub(crate) fn tolerance(step: i8) -> f64 {
    2f64.powi(i32::from(step))
}

/// The pixels per millimetre a body in `bounds` can come to at most while the view orbits its
/// target: at the least depth it can reach, the camera's distance less the body's farthest reach
/// from the target. Orthographic views have one scale. `None` where the body can come to the eye.
pub(crate) fn orbit_px_per_mm(cam: &solidcraft_viewport::Camera, bounds_min: Vec3, bounds_max: Vec3, scale: f64) -> Option<f64> {
    if cam.fov <= 0.0 {
        return Some(scale);
    }
    let centre = (bounds_min + bounds_max) * 0.5;
    let radius = length(bounds_max - bounds_min) * 0.5;
    let depth = cam.distance - length(centre - cam.target) - radius;
    // A body that can come within a hundredth of the distance is left at full detail.
    (depth > cam.distance * 1e-2).then(|| scale / depth)
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn length(a: Vec3) -> f64 {
    dot(a, a).sqrt()
}

/// The distance from `p` to the segment `a`–`b` (to `a` where they are the same point).
fn to_segment(p: Vec3, a: Vec3, b: Vec3) -> f64 {
    let ab = b - a;
    let len2 = dot(ab, ab);
    let t = if len2 > 0.0 { (dot(p - a, ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
    length(p - (a + ab * t))
}

/// `points` with the points dropped that lie within `tol` of the simplified line (Douglas and
/// Peucker): the ends are kept, and no dropped point is farther than `tol` from the result. A
/// closed loop (first point equals the last) keeps its point farthest from the start, so it stays a
/// loop. A point whose distance is not a number is kept. Iterative: no recursion on long edges.
pub(crate) fn simplify(points: &[Vec3], tol: f64) -> Vec<Vec3> {
    let n = points.len();
    if n <= 2 || !(tol > 0.0) {
        return points.to_vec();
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut stack = vec![(0, n - 1)];
    // A closed loop's ends are one point: split it at its point farthest from them first.
    if points[0] == points[n - 1] {
        let start = points[0];
        let far = (1..n - 1).max_by(|&i, &j| length(points[i] - start).total_cmp(&length(points[j] - start)));
        if let Some(far) = far {
            keep[far] = true;
            stack = vec![(0, far), (far, n - 1)];
        }
    }
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let (pa, pb) = (points[a], points[b]);
        let mut far = (a + 1, -1.0);
        for (i, p) in points.iter().enumerate().take(b).skip(a + 1) {
            let d = to_segment(*p, pa, pb);
            // A distance that is not a number keeps its point.
            let d = if d.is_nan() { f64::INFINITY } else { d };
            if d > far.1 {
                far = (i, d);
            }
        }
        if far.1 > tol {
            keep[far.0] = true;
            stack.push((a, far.0));
            stack.push((far.0, b));
        }
    }
    points.iter().zip(keep).filter_map(|(p, k)| k.then_some(*p)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    /// A circle of `r` in `n` segments (closed: the last point is the first).
    fn circle(r: f64, n: usize) -> Vec<Vec3> {
        (0..=n).map(|i| std::f64::consts::TAU * (i % n) as f64 / n as f64).map(|a| v(r * a.cos(), r * a.sin(), 0.0)).collect()
    }

    #[test]
    fn a_fine_circle_keeps_only_the_points_its_tolerance_needs_and_no_point_moves_further() {
        let fine = circle(10.0, 720);
        let tol = 0.05;
        let coarse = simplify(&fine, tol);
        assert_eq!((coarse.first(), coarse.last()), (fine.first(), fine.last()), "the ends stay");
        // A chord of sagitta 0.05 on r 10 spans 2·acos(1 − 0.005) ≈ 11.5°: about 32 segments.
        assert!((20..=64).contains(&(coarse.len() - 1)), "{} segments", coarse.len() - 1);
        let worst = fine.iter().map(|p| coarse.windows(2).map(|w| to_segment(*p, w[0], w[1])).fold(f64::INFINITY, f64::min)).fold(0.0, f64::max);
        assert!(worst <= tol, "every dropped point lies within the tolerance: {worst}");
    }

    #[test]
    fn straight_runs_and_short_edges_are_left_as_they_are_where_nothing_can_go() {
        let line: Vec<Vec3> = (0..10).map(|i| v(f64::from(i), 0.0, 0.0)).collect();
        assert_eq!(simplify(&line, 0.01), vec![line[0], line[9]], "a straight run is its two ends");
        let corner = vec![v(0.0, 0.0, 0.0), v(1.0, 0.0, 0.0), v(1.0, 1.0, 0.0)];
        assert_eq!(simplify(&corner, 0.1), corner, "a corner larger than the tolerance stays");
        assert_eq!(simplify(&corner[..2], 5.0), corner[..2], "a single segment stays");
        assert_eq!(simplify(&corner, 0.0), corner, "no tolerance, no change");
        assert_eq!(simplify(&corner, f64::NAN), corner, "a tolerance that is not a number changes nothing");
    }

    #[test]
    fn a_closed_loop_stays_a_loop_and_points_that_are_not_numbers_are_kept() {
        let tiny = simplify(&circle(1.0, 64), 10.0);
        assert!(tiny.len() >= 3 && tiny.first() == tiny.last(), "a loop smaller than the tolerance keeps its far point: {tiny:?}");
        let odd = vec![v(0.0, 0.0, 0.0), v(f64::NAN, 0.0, 0.0), v(2.0, 0.0, 0.0)];
        assert_eq!(simplify(&odd, 1.0).len(), 3, "a point at no known distance is kept");
    }

    #[test]
    fn the_step_keeps_the_tolerance_under_the_pixel_limit_and_changes_only_when_it_must() {
        for ppm in [0.01, 0.37, 1.0, 25.0, 4000.0] {
            let step = step_for(ppm).expect("a scale gives a step");
            let px = tolerance(step) * ppm;
            assert!(px <= TOLERANCE_PX && px > TOLERANCE_PX / 2.0, "{ppm} px/mm: {px} px");
        }
        assert_eq!([step_for(0.0), step_for(f64::NAN), step_for(f64::INFINITY)], [None, None, None]);
        // Coarser by one step: kept. Coarser by two: taken. Finer: taken at once.
        assert_eq!((hysteresis(Some(-3), -2), hysteresis(Some(-4), -2), hysteresis(Some(-1), -2), hysteresis(None, -2)), (-3, -2, -2, -2));
    }

    #[test]
    fn the_orbit_scale_uses_the_nearest_depth_the_body_can_reach() {
        let cam = solidcraft_viewport::Camera { target: v(0.0, 0.0, 0.0), distance: 100.0, fov: 0.6, ..Default::default() };
        // A unit box 30 mm from the target: it can come to 100 − 30 − √3/2 from the eye.
        let ppm = orbit_px_per_mm(&cam, v(29.5, -0.5, -0.5), v(30.5, 0.5, 0.5), 1000.0).expect("in front");
        assert!((ppm - 1000.0 / (70.0 - 3f64.sqrt() / 2.0)).abs() < 1e-9, "{ppm}");
        assert_eq!(orbit_px_per_mm(&cam, v(-200.0, -1.0, -1.0), v(200.0, 1.0, 1.0), 1000.0), None, "a body around the eye stays whole");
        let ortho = solidcraft_viewport::Camera { fov: 0.0, ..cam };
        assert_eq!(orbit_px_per_mm(&ortho, v(29.5, -0.5, -0.5), v(30.5, 0.5, 0.5), 7.0), Some(7.0));
    }
}
