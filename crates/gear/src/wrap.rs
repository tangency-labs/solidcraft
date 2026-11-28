//! Wrapping drives (chains and belts) between two wheels: the open belt's path, its length, and
//! a roller chain's length in links and the centre distance that a whole number of links gives.

use std::f64::consts::PI;

use crate::geom::{Outline, Seg, V2};

/// The angle the straight runs make with the line of centres, for pitch radii `r1` (at the
/// origin) and `r2` (at (`a`, 0)).
pub fn run_angle(r1: f64, r2: f64, a: f64) -> f64 {
    ((r2 - r1) / a).clamp(-1.0, 1.0).asin()
}

/// The angle the belt or chain wraps round the smaller wheel (radians).
pub fn wrap_angle(r1: f64, r2: f64, a: f64) -> f64 {
    PI - 2.0 * run_angle(r1, r2, a).abs()
}

/// The pitch line length of an open belt round two wheels.
pub fn belt_length(r1: f64, r2: f64, a: f64) -> f64 {
    let b = run_angle(r1, r2, a);
    2.0 * a * b.cos() + r1 * (PI - 2.0 * b) + r2 * (PI + 2.0 * b)
}

/// The closed path of an open belt (its pitch line moved outwards by `off`), counter-clockwise:
/// round wheel 2 on the right, along the top, round wheel 1 on the left, back along the bottom.
/// None when the wheels overlap.
pub fn belt_path(r1: f64, r2: f64, a: f64, off: f64) -> Option<Outline> {
    if a <= (r1 - r2).abs() || r1 + off <= 0.0 || r2 + off <= 0.0 {
        return None;
    }
    let b = run_angle(r1, r2, a);
    let up = V2::new(-b.sin(), b.cos());
    let down = up.flip_y();
    let c2 = V2::new(a, 0.0);
    let (q1, q2) = (r1 + off, r2 + off);
    let p2d = c2 + down * q2;
    let p2u = c2 + up * q2;
    let p1u = up * q1;
    let p1d = down * q1;
    Some(Outline {
        segs: vec![
            Seg::Arc { c: c2, a: p2d, b: p2u, ccw: true },
            Seg::Line(p2u, p1u),
            Seg::Arc { c: V2::ZERO, a: p1u, b: p1d, ccw: true },
            Seg::Line(p1d, p2d),
        ],
    })
}

/// A roller chain's length in pitches for sprockets of `z1` and `z2` teeth at centre distance
/// `a` (ISO 10823's approximation, not rounded).
pub fn chain_links(z1: f64, z2: f64, a: f64, pitch: f64) -> f64 {
    let k = (z2 - z1) / (2.0 * PI);
    2.0 * a / pitch + (z1 + z2) / 2.0 + pitch / a * k * k
}

/// The centre distance at which a chain of `links` pitches is taut (ISO 10823), or None when the
/// chain is too short to go round.
pub fn chain_center(z1: f64, z2: f64, links: f64, pitch: f64) -> Option<f64> {
    let k = (z2 - z1) / (2.0 * PI);
    let m = links - (z1 + z2) / 2.0;
    let disc = m * m - 8.0 * k * k;
    (m > 0.0 && disc >= 0.0).then(|| pitch / 4.0 * (m + disc.sqrt()))
}
