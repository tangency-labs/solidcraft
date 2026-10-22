//! Profile segments as exact schema 4 curves: each sketch segment on its plane becomes a 3D edge
//! curve and, on the plane's own surface (`origin + u x + v y`, (u, v) the sketch coordinates), its
//! pcurve, both on t in [0, 1] with the same knots and weights, so the pcurve lifts onto the edge
//! exactly. Lines are lines; arcs rational quadratic B-splines (one span per quarter turn or less);
//! cubic and higher Béziers B-splines on their own control points; conics rational quadratic
//! Béziers (exact ellipses, parabolas and hyperbolas).

use solidcraft_geom::brep::{Curve, Curve2, Surface};
use solidcraft_geom::{Plane, Seg2, Vec2, Vec3};

use crate::{KernelError, Result};

/// The interval every curve [`segment`] makes runs on.
pub(crate) const INTERVAL: [f64; 2] = [0.0, 1.0];

/// The plane of a sketch as a record surface whose (u, v) are the sketch's coordinates.
pub(crate) fn plane_surface(plane: &Plane) -> Surface {
    let a = |v: Vec3| [v.x, v.y, v.z];
    Surface::Plane { origin: a(plane.origin), u_axis: a(plane.x), v_axis: a(plane.y) }
}

/// A clamped Bézier knot vector of `degree`.
fn bezier_knots(degree: usize) -> Vec<f64> {
    std::iter::repeat_n(0.0, degree + 1).chain(std::iter::repeat_n(1.0, degree + 1)).collect()
}

/// The 2D rational control net of `s` (points, weights, knots), on t in [0, 1].
fn net(s: &Seg2) -> Result<(Vec<Vec2>, Vec<f64>, Vec<f64>)> {
    Ok(match *s {
        Seg2::Line { a, b } => (vec![a, b], vec![1.0; 2], bezier_knots(1)),
        Seg2::Arc { center, radius, start, sweep } => {
            if !(radius.is_finite() && radius > 0.0 && sweep.is_finite() && sweep != 0.0 && sweep.abs() <= std::f64::consts::TAU * (1.0 + 1e-9)) {
                return Err(KernelError::Invalid("an arc with no radius or turn, or turning more than once".into()));
            }
            let sweep = sweep.clamp(-std::f64::consts::TAU, std::f64::consts::TAU);
            let m = (sweep.abs() / std::f64::consts::FRAC_PI_2 - 1e-9).ceil().max(1.0) as usize;
            let d = sweep / m as f64;
            let at = |a: f64, r: f64| center + Vec2::new(a.cos(), a.sin()) * r;
            let w = (d / 2.0).cos();
            let (mut pts, mut weights, mut knots) = (vec![at(start, radius)], vec![1.0], vec![0.0; 3]);
            for j in 0..m {
                let a0 = start + d * j as f64;
                pts.push(at(a0 + d / 2.0, radius / w));
                weights.push(w);
                pts.push(at(a0 + d, radius));
                weights.push(1.0);
                let u = (j + 1) as f64 / m as f64;
                knots.extend(if j + 1 < m { [u, u].as_slice() } else { [1.0, 1.0, 1.0].as_slice() });
            }
            (pts, weights, knots)
        }
        Seg2::Cubic { p0, p1, p2, p3 } => (vec![p0, p1, p2, p3], vec![1.0; 4], bezier_knots(3)),
        Seg2::Conic { a, apex, b, w } => {
            if !(w.is_finite() && w > 0.0) {
                return Err(KernelError::Invalid("a conic's weight must be positive".into()));
            }
            (vec![a, apex, b], vec![1.0, w, 1.0], bezier_knots(2))
        }
        Seg2::Bezier { n, ref p } => {
            let n = usize::from(n);
            let pts = p.get(..=n).filter(|_| (1..=solidcraft_geom::MAX_BEZIER_DEGREE).contains(&n));
            let pts = pts.ok_or_else(|| KernelError::Invalid("Bézier degree".into()))?;
            (pts.to_vec(), vec![1.0; n + 1], bezier_knots(n))
        }
    })
}

/// The segment `s` on `plane` as an edge curve and its pcurve on [`plane_surface`], both on
/// [`INTERVAL`].
pub(crate) fn segment(plane: &Plane, s: &Seg2) -> Result<(Curve, Curve2)> {
    let (pts, weights, knots) = net(s)?;
    if pts.iter().any(|q| !q.is_finite()) {
        return Err(KernelError::Invalid("a profile curve is not finite".into()));
    }
    let w = |q: Vec2| {
        let p = plane.to_world(q);
        [p.x, p.y, p.z]
    };
    if let Seg2::Line { a, b } = *s {
        if a.dist(b) <= 0.0 {
            return Err(KernelError::Invalid("a profile line has no length".into()));
        }
        let pc = Curve2::Spline { knots, points: vec![[a.x, a.y], [b.x, b.y]] };
        return Ok((Curve::Line { start: w(a), end: w(b) }, pc));
    }
    if weights.iter().all(|x| *x == 1.0) {
        let curve = Curve::Spline { knots: knots.clone(), points: pts.iter().map(|q| w(*q)).collect() };
        return Ok((curve, Curve2::Spline { knots, points: pts.iter().map(|q| [q.x, q.y]).collect() }));
    }
    let homogeneous = pts
        .iter()
        .zip(&weights)
        .map(|(q, h)| {
            let [x, y, z] = w(*q);
            [x * h, y * h, z * h, *h]
        })
        .collect();
    let pcurve = Curve2::RationalSpline { knots: knots.clone(), homogeneous: pts.iter().zip(&weights).map(|(q, h)| [q.x * h, q.y * h, *h]).collect() };
    Ok((Curve::RationalSpline { knots, homogeneous }, pcurve))
}
