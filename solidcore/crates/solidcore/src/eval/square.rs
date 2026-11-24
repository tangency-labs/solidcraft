//! Squaring the axes of a frame or a plane a record holds a little off square, as the record is
//! read. The readers take a frame's axes only unit and square to 8 ulps, and a plane's axes only
//! square to the rounding of their dot product (`gamma(3)`, `from_parameters`), because the
//! kernel's placement cannot hold a skew one. Records written before the writer squared them
//! (#21509) carry axes off by about 1e-14, so they did not open. These functions give the axes the
//! readers take, keeping the first axis (a frame's x, a plane's u) exact:
//!
//! - **Plane:** the same plane, reparameterised. Its u axis is kept to the bit; its v axis loses
//!   its part along u (Gram-Schmidt) and is the part across. A point `o + a u + b v` of the
//!   record's is `o + (a + s b) u + b v'` with `s` the part of v along u, so the face's pcurves
//!   and box move by `u' = u + s v` ([`shear_uv`]) and every point of the face is where it was,
//!   to rounding, however far off square the axes were.
//! - **Frame:** x made unit, y made unit across it. z is the same direction, so a cylinder, cone,
//!   sphere or torus is the same surface and a circle lies in the same plane; only the angle is
//!   measured on square axes. A point at angle t moves by at most the radius times the frame's
//!   deviation from orthonormal ([`frame_deviation`]).
//!
//! [`SQUARE_TOLERANCE`] is how far off square axes may be and be squared without a word: 1e-9.
//! A frame's point moves by at most its distance from the origin times the deviation, so at 1e-9
//! a part a metre across moves by at most 1e-6 mm, the record's own default linear tolerance; a
//! plane moves nothing. Axes further off are squared all the same (a design is never refused for
//! them), and the caller warns, naming the face or edge ([`square`](crate::eval::square)'s users).
//!
//! Not upstream: SolidCraft's reading of older records. No recursion.

use crate::geom::vector3d::Vec3;
use crate::nurbs::math::gamma;

/// How far off square a record's axes may be and be squared on read without a warning (see the
/// module's derivation).
pub const SQUARE_TOLERANCE: f64 = 1e-9;

fn v(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

fn arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// A frame's axes as the readers take them: both unit and square to 8 ulps (the test of
/// `surface::from_parameters` and `curve::from_parameters`).
#[must_use]
pub fn frame_is_square(x: [f64; 3], y: [f64; 3]) -> bool {
    let ulps = 8.0 * f64::EPSILON;
    let (x, y) = (v(x), v(y));
    let unit = |v: Vec3| (v.length() - 1.0).abs() <= ulps;
    unit(x) && unit(y) && x.dot(y).abs() <= ulps
}

/// How far a frame's axes are from orthonormal: the largest of their lengths' distances from 1
/// and the cosine of the angle between them. Infinite for an axis of no length or not finite.
#[must_use]
pub fn frame_deviation(x: [f64; 3], y: [f64; 3]) -> f64 {
    let (x, y) = (v(x), v(y));
    let (lx, ly) = (x.length(), y.length());
    if !(lx > 0.0 && ly > 0.0 && lx.is_finite() && ly.is_finite()) {
        return f64::INFINITY;
    }
    (lx - 1.0)
        .abs()
        .max((ly - 1.0).abs())
        .max(x.dot(y).abs() / (lx * ly))
}

/// The frame's axes squared: x made unit, y with its part along x taken out and made unit, so
/// that [`frame_is_square`] holds. A frame that already holds it is given back to the bit.
/// `None` for an axis of no length or not finite, or y along x.
#[must_use]
pub fn square_frame(x: [f64; 3], y: [f64; 3]) -> Option<([f64; 3], [f64; 3])> {
    if frame_is_square(x, y) {
        return Some((x, y));
    }
    let (_, ux) = v(x).length_and_unit()?;
    let mut w = v(y);
    // two passes take out what rounding the first leaves along x
    for _ in 0..3 {
        let (_, uw) = w.length_and_unit()?;
        w = uw;
        if frame_is_square(arr(ux), arr(w)) {
            break;
        }
        w = w - ux * w.dot(ux);
    }
    let out = (arr(ux), arr(w));
    (frame_is_square(out.0, out.1) && [ux.x, ux.y, ux.z, w.x, w.y, w.z].iter().all(|c| c.is_finite()))
        .then_some(out)
}

/// A plane's axes as the reader takes them: square to the rounding of their dot product,
/// `|du . dv| <= gamma(3) |du| |dv|`.
#[must_use]
pub fn plane_is_square(du: [f64; 3], dv: [f64; 3]) -> bool {
    let (du, dv) = (v(du), v(dv));
    du.dot(dv).abs() <= gamma(3) * du.length() * dv.length()
}

/// The cosine of the angle between a plane's axes: how far off square they are. Infinite for
/// an axis of no length or not finite.
#[must_use]
pub fn plane_skew(du: [f64; 3], dv: [f64; 3]) -> f64 {
    let (du, dv) = (v(du), v(dv));
    let (a, b) = (du.length(), dv.length());
    if !(a > 0.0 && b > 0.0 && a.is_finite() && b.is_finite()) {
        return f64::INFINITY;
    }
    du.dot(dv).abs() / (a * b)
}

/// A plane's v axis square to its u axis `du`: `(v', s)` with `dv = s du + v'` and
/// [`plane_is_square`]`(du, v')`, so the record's `(a, b)` is `(a + s b, b)` on `du`, `v'`
/// ([`shear_uv`]). `du` is kept as it is. `None` for an axis of no length or not finite, or
/// `dv` along `du`.
#[must_use]
pub fn square_plane(du: [f64; 3], dv: [f64; 3]) -> Option<([f64; 3], f64)> {
    let (u, mut w) = (v(du), v(dv));
    let uu = u.dot(u);
    if !(uu > 0.0 && uu.is_finite() && w.length().is_finite()) {
        return None;
    }
    let mut s = 0.0;
    // two passes take out what rounding the first leaves along u
    for _ in 0..3 {
        if plane_is_square(du, arr(w)) {
            break;
        }
        let part = w.dot(u) / uu;
        w = w - u * part;
        s += part;
    }
    let lw = w.length();
    (plane_is_square(du, arr(w)) && lw > 0.0 && lw.is_finite() && s.is_finite())
        .then_some((arr(w), s))
}

/// A point `(a, b)` in the record's plane parameters in those of the squared plane
/// ([`square_plane`]'s shear `s`): `(a + s b, b)`. For a homogeneous point `(a w, b w, w)` the
/// same map holds, as it is linear.
#[must_use]
pub fn shear_uv(s: f64, a: f64, b: f64) -> (f64, f64) {
    (a + s * b, b)
}

/// A plane face's box `[[u0, u1], [v0, v1]]` in the squared plane's parameters: the box round
/// the sheared box's corners ([`shear_uv`]), so it holds every point of the face.
#[must_use]
pub fn shear_box(s: f64, domain: [[f64; 2]; 2]) -> [[f64; 2]; 2] {
    let [[u0, u1], [v0, v1]] = domain;
    let (lo, hi) = ((s * v0).min(s * v1), (s * v0).max(s * v1));
    [[u0 + lo, u1 + hi], [v0, v1]]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(a: [f64; 3], c: f64, s: f64) -> [f64; 3] {
        // about z
        [c * a[0] - s * a[1], s * a[0] + c * a[1], a[2]]
    }

    #[test]
    fn a_square_frame_is_kept_to_the_bit() {
        let (c, s) = 0.3f64.sin_cos();
        let (x, y) = (turn([1.0, 0.0, 0.0], c, s), turn([0.0, 1.0, 0.0], c, s));
        assert!(frame_is_square(x, y));
        assert_eq!(square_frame(x, y), Some((x, y)));
    }

    #[test]
    fn a_skew_frame_is_squared_keeping_x_and_z() {
        for eps in [1e-12, 1e-6, 1e-2] {
            let x = [0.6, 0.8, 0.0];
            let y = [-0.8 + eps, 0.6, eps];
            assert!(!frame_is_square(x, y));
            let (sx, sy) = square_frame(x, y).unwrap();
            assert!(frame_is_square(sx, sy), "{eps}");
            // x keeps its direction
            let (vx, vsx) = (v(x), v(sx));
            assert!(vx.cross(vsx).length() <= 4.0 * f64::EPSILON, "{eps}");
            // z keeps its direction: y moved in the plane of x and y
            let (n, sn) = (vx.cross(v(y)), vsx.cross(v(sy)));
            let n = n / n.length();
            assert!(n.cross(sn).length() <= 8.0 * f64::EPSILON, "{eps}");
            assert!(frame_deviation(x, y) >= eps * 0.5);
        }
    }

    #[test]
    fn a_skew_plane_keeps_its_points() {
        let du = [3.0, 0.0, 4.0];
        for eps in [1e-12, 1e-6, 0.3] {
            let dv = [eps * 3.0, 2.0, eps * 4.0];
            assert!(!plane_is_square(du, dv));
            let (w, s) = square_plane(du, dv).unwrap();
            assert!(plane_is_square(du, w));
            for (a, b) in [(0.0, 0.0), (1.5, -2.0), (-7.0, 13.0)] {
                let p = v(du) * a + v(dv) * b;
                let (a2, b2) = shear_uv(s, a, b);
                let q = v(du) * a2 + v(w) * b2;
                assert!((p - q).length() <= 16.0 * f64::EPSILON * (p.length() + 1.0), "{eps} {a} {b}");
            }
            let bx = shear_box(s, [[-1.0, 2.0], [-3.0, 5.0]]);
            for (a, b) in [(-1.0, -3.0), (2.0, -3.0), (-1.0, 5.0), (2.0, 5.0)] {
                let (a2, _) = shear_uv(s, a, b);
                assert!(bx[0][0] <= a2 && a2 <= bx[0][1]);
            }
        }
    }

    #[test]
    fn a_square_plane_is_not_sheared() {
        let (du, dv) = ([2.0, 0.0, 0.0], [0.0, 0.5, 0.0]);
        assert_eq!(square_plane(du, dv), Some((dv, 0.0)));
    }

    #[test]
    fn axes_that_cannot_be_squared_are_none() {
        assert_eq!(square_frame([1.0, 0.0, 0.0], [2.0, 0.0, 0.0]), None);
        assert_eq!(square_frame([0.0; 3], [0.0, 1.0, 0.0]), None);
        assert_eq!(square_frame([f64::NAN, 0.0, 0.0], [0.0, 1.0, 0.0]), None);
        assert_eq!(square_plane([1.0, 0.0, 0.0], [-3.0, 0.0, 0.0]), None);
        assert_eq!(square_plane([0.0; 3], [0.0, 1.0, 0.0]), None);
        assert_eq!(square_plane([1.0, 0.0, 0.0], [0.0, f64::INFINITY, 0.0]), None);
    }
}
