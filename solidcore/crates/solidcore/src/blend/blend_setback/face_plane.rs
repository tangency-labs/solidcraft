//! A corner face's rational parameter plane: the plane a Setback's spring curve is built in
//! ([`super::spring_curve`]), and the plane the corner's piece of the face is decided in
//! (solidcore#2343 (d)).
//!
//! The plane: a plane's own coordinates along two unit directions in it; a cylinder's or
//! cone's `(t, h)`, `t = tan(theta / 2)` about the axis from the radial direction `e1` halfway
//! between P's and Q's, h along the axis; a sphere's stereographic plane `(a, b)` from the
//! point opposite the middle of P's and Q's directions. On the face each coordinate is
//! linear-fractional in the point's: `t = u2 / (rho(u3) + u1)` with `rho(h) = R + slope h` the
//! radius at h (half the angle's tangent, `sin / (1 + cos)`), and `(a, b) = (u1, u2) / (R +
//! u3)`, u the point's coordinates from the frame's origin along `(e1, e2, e3)`. A rational
//! curve on the face maps to a rational curve in the plane ([`FacePlane::net`]): of its own
//! degree on a plane or sphere, of twice it on a cylinder or cone (t's denominator and h's
//! differ).
//!
//! | Rust                  | Fork (`SmBlendSetback`, 37c9e1d)          |
//! |-----------------------|-------------------------------------------|
//! | [`FacePlane::new`]    | `SpringCurve`'s frame                     |
//! | [`FacePlane::to_plane`] | `SpringCurve`'s closure `toPlane`       |
//! | [`FacePlane::partials`] | `SpringCurve`'s closure `tangents`      |
//! | [`FacePlane::net`], [`FacePlane::point`] | none (solidcore#2343 (d)) |
//!
//! Deviations: moved out of `spring.rs` unchanged (the frame, `to_plane` and `partials` are
//! the same operations in the same order); the frame is a value of its own. No recursion.

use super::{bern_elevate, bern_mul, el, sm_err, Face, FaceKind, Hom};
use crate::error::Result;
use crate::geom::vector3d::sqrt;
use crate::math::{Point3, Vec3, EFF_ZERO, PI};

/// A point in homogeneous plane coordinates `(x w, y w, w)`.
pub(crate) type Hom2 = [f64; 3];

/// A corner face's rational parameter plane (module docs): its kind and size, and the frame
/// `(o, e1, e2, e3)` it is taken in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FacePlane {
    /// What the face is.
    pub(crate) kind: FaceKind,
    /// A quadric's radius at `o`.
    pub(crate) radius: f64,
    /// A cone's change of radius along its axis.
    pub(crate) slope: f64,
    /// The frame's origin: the face's point (a quadric's axis point or centre).
    pub(crate) o: Vec3,
    /// The frame's first direction: a plane's from P to Q, a quadric's radial halfway
    /// between P's and Q's (a sphere's along Q's direction less P's).
    pub(crate) e1: Vec3,
    /// `e3 x e1`.
    pub(crate) e2: Vec3,
    /// A plane's normal, a cylinder's or cone's axis, a sphere's middle of P's and Q's
    /// directions.
    pub(crate) e3: Vec3,
}

impl FacePlane {
    /// The plane of `face` for a spring curve from `p` to `q` (`SpringCurve`'s frame).
    ///
    /// # Errors
    /// `SM_ERR` where a direction of the frame is zero.
    pub(crate) fn new(face: &Face, p: Point3, q: Point3) -> Result<Self> {
        let o = face.point;
        // the frame: e3 the axis (a plane's normal), e1 radial halfway between P and Q (a
        // plane's: P to Q)
        let mut e3 = if face.kind == FaceKind::Plane {
            face.normal
        } else {
            face.axis
        };
        e3 = e3.unitize()?;
        let mut e1 = match face.kind {
            FaceKind::Plane => q - p,
            FaceKind::Sphere => {
                let up = (p - o).unitize()?;
                let uq = (q - o).unitize()?;
                e3 = (up + uq).unitize()?; // m: the middle of P's and Q's directions
                uq - up
            }
            FaceKind::Cylinder | FaceKind::Cone => {
                let mut rp = p - o;
                let mut rq = q - o;
                rp = rp - e3 * rp.dot(e3);
                rq = rq - e3 * rq.dot(e3);
                rp.unitize()? + rq.unitize()?
            }
        };
        e1 = e1 - e3 * e1.dot(e3);
        e1 = e1.unitize()?;
        let e2 = e3.cross(e1);
        Ok(Self {
            kind: face.kind,
            radius: face.radius,
            slope: face.slope,
            o,
            e1,
            e2,
            e3,
        })
    }

    /// The parameter-plane point and tangent of a point `x` with a tangent `t` on the face
    /// (the closure `toPlane`). `SM_ERR` where the point is on a quadric's axis or pole, or
    /// more than a half turn about a cylinder's or cone's axis.
    pub(crate) fn to_plane(self, x: Point3, t: Vec3) -> Result<([f64; 2], [f64; 2])> {
        let (o, e1, e2, e3) = (self.o, self.e1, self.e2, self.e3);
        let r = self.radius;
        let u = x - o;
        let (u1, u2, u3) = (u.dot(e1), u.dot(e2), u.dot(e3));
        let (d1, d2, d3) = (t.dot(e1), t.dot(e2), t.dot(e3));
        match self.kind {
            FaceKind::Plane => Ok(([u1, u2], [d1, d2])),
            FaceKind::Sphere => {
                let den = r + u3;
                if !(den > EFF_ZERO * r) {
                    return Err(sm_err());
                }
                Ok((
                    [u1 / den, u2 / den],
                    [
                        (d1 * den - u1 * d3) / (den * den),
                        (d2 * den - u2 * d3) / (den * den),
                    ],
                ))
            }
            FaceKind::Cylinder | FaceKind::Cone => {
                let rho = sqrt(u1 * u1 + u2 * u2);
                if !(rho > EFF_ZERO) {
                    return Err(sm_err());
                }
                let theta = u2.atan2(u1);
                if theta.abs() >= 0.5 * PI {
                    return Err(sm_err()); // more than a half turn between P and Q
                }
                let tt = (0.5 * theta).tan();
                let dtheta = (-theta.sin() * d1 + theta.cos() * d2) / rho;
                Ok(([tt, u3], [0.5 * (1.0 + tt * tt) * dtheta, d3]))
            }
        }
    }

    /// The face's parametric partials along the parameter-plane parabola `(cu, cv)` (the
    /// closure `tangents`).
    pub(crate) fn partials(&self, cu: &[f64], cv: &[f64]) -> Result<[Vec<Hom>; 2]> {
        let (o, e1, e2, e3) = (self.o, self.e1, self.e2, self.e3);
        let r = self.radius;
        let mut xyz: [Vec<Vec3>; 2] = [Vec::new(), Vec::new()];
        let mut w: [Vec<f64>; 2] = [Vec::new(), Vec::new()];
        match self.kind {
            FaceKind::Plane => {
                xyz = [vec![e1], vec![e2]];
                w = [vec![0.0], vec![0.0]];
            }
            FaceKind::Sphere => {
                // S(a, b) = O (1 + a^2 + b^2) + R (2 a e1 + 2 b e2 + (1 - a^2 - b^2) m),
                // w = 1 + a^2 + b^2: S_a = (2 a O + R (2 e1 - 2 a m), 2 a), S_b likewise; affine
                // in (a, b), quadratic along it
                for (k, (xk, wk)) in xyz.iter_mut().zip(w.iter_mut()).enumerate() {
                    for i in 0..3 {
                        let da = if k == 0 { el(cu, i)? } else { el(cv, i)? };
                        let ee = if k == 0 { e1 } else { e2 };
                        xk.push(o * (2.0 * da) + (ee * 2.0 - e3 * (2.0 * da)) * r);
                        wk.push(2.0 * da);
                    }
                }
            }
            FaceKind::Cylinder | FaceKind::Cone => {
                // the circumferential direction -2 t e1 + (1 - t^2) e2 (degree 4), and the
                // cylinder's axis or the cone's generator kappa (1 - t^2) e1 + 2 kappa t e2 +
                // (1 + t^2) e3 (degree 4)
                let tt = bern_mul(cu, cu)?;
                let t4 = bern_elevate(cu, 4)?;
                let [x0, x1] = &mut xyz;
                let [w0, w1] = &mut w;
                for i in 0..5 {
                    x0.push(e1 * (-2.0 * el(&t4, i)?) + e2 * (1.0 - el(&tt, i)?));
                    w0.push(0.0);
                }
                if self.kind == FaceKind::Cylinder {
                    *x1 = vec![e3];
                    *w1 = vec![0.0];
                } else {
                    let kk = self.slope;
                    for i in 0..5 {
                        x1.push(
                            e1 * (kk * (1.0 - el(&tt, i)?))
                                + e2 * (2.0 * kk * el(&t4, i)?)
                                + e3 * (1.0 + el(&tt, i)?),
                        );
                        w1.push(0.0);
                    }
                }
            }
        }
        let mut out: [Vec<Hom>; 2] = [Vec::new(), Vec::new()];
        for ((ok, xk), wk) in out.iter_mut().zip(&xyz).zip(&w) {
            for (x, &wi) in xk.iter().zip(wk) {
                ok.push([x.x, x.y, x.z, wi]);
            }
        }
        Ok(out)
    }

    /// The homogeneous Bernstein coefficients in this plane of the rational Bezier curve on
    /// the face with homogeneous coefficients `net` (module docs): `(U1, U2, W)` on a plane,
    /// `(U1, U2, R W + U3)` on a sphere, `(U2 W, U3 D, D W)` on a cylinder or cone with `D = R
    /// W + slope U3 + U1`, U the coefficients' coordinates from the frame's origin (`x w - o
    /// w`) and W their weights. Exact (but for rounding) for a curve on the face, where
    /// `rho(u3) = sqrt(u1^2 + u2^2)`. `None` where a weight of the result is not positive or a
    /// coordinate not finite (a curve through a sphere's pole or a quadric's opposite line, or
    /// one whose hull reaches past it): such a curve has no hull in the plane.
    pub(crate) fn net(&self, net: &[Hom]) -> Option<Vec<Hom2>> {
        let mut u1 = Vec::new();
        let mut u2 = Vec::new();
        let mut u3 = Vec::new();
        let mut w = Vec::new();
        for h in net {
            let x = Vec3::new(h[0], h[1], h[2]) - self.o * h[3];
            u1.push(x.dot(self.e1));
            u2.push(x.dot(self.e2));
            u3.push(x.dot(self.e3));
            w.push(h[3]);
        }
        let (x, y, ww): (Vec<f64>, Vec<f64>, Vec<f64>) = match self.kind {
            FaceKind::Plane => (u1, u2, w),
            FaceKind::Sphere => {
                let den = w
                    .iter()
                    .zip(&u3)
                    .map(|(&wi, &zi)| self.radius * wi + zi)
                    .collect();
                (u1, u2, den)
            }
            FaceKind::Cylinder | FaceKind::Cone => {
                let d: Vec<f64> = w
                    .iter()
                    .zip(&u3)
                    .zip(&u1)
                    .map(|((&wi, &zi), &xi)| self.radius * wi + self.slope * zi + xi)
                    .collect();
                (
                    bern_mul(&u2, &w).ok()?,
                    bern_mul(&u3, &d).ok()?,
                    bern_mul(&d, &w).ok()?,
                )
            }
        };
        let mut out = Vec::new();
        for ((&xi, &yi), &wi) in x.iter().zip(&y).zip(&ww) {
            if !(wi > 0.0) || !xi.is_finite() || !yi.is_finite() || !wi.is_finite() {
                return None;
            }
            out.push([xi, yi, wi]);
        }
        Some(out)
    }

    /// The point in this plane of the point `x` on the face ([`FacePlane::net`] of it);
    /// `None` on a sphere's pole or a quadric's opposite line, or where it is not finite.
    pub(crate) fn point(&self, x: Point3) -> Option<[f64; 2]> {
        let h = self.net(&[[x.x, x.y, x.z, 1.0]])?;
        let [px, py, pw] = *h.first()?;
        let out = [px / pw, py / pw];
        out.iter().all(|c| c.is_finite()).then_some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A point on each kind of face maps by [`FacePlane::point`] to [`FacePlane::to_plane`]'s
    /// point, and a rational curve on it to the curve of its points.
    #[test]
    fn the_plane_of_a_net_is_the_plane_of_its_points() {
        let cylinder = Face {
            kind: FaceKind::Cylinder,
            axis: Vec3::new(0.0, 0.0, 1.0),
            radius: 10.0,
            ..Face::default()
        };
        let cone = Face {
            kind: FaceKind::Cone,
            axis: Vec3::new(0.0, 0.0, 1.0),
            radius: 2.0,
            slope: 0.5,
            ..Face::default()
        };
        let sphere = Face {
            kind: FaceKind::Sphere,
            radius: 10.0,
            ..Face::default()
        };
        let plane = Face::plane(Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, 1.0));
        let on = |f: &Face, a: f64, h: f64| -> Point3 {
            match f.kind {
                FaceKind::Plane => Vec3::new(a, h, 1.0),
                FaceKind::Sphere => Vec3::new(
                    10.0 * h.cos() * a.cos(),
                    10.0 * h.cos() * a.sin(),
                    10.0 * h.sin(),
                ),
                FaceKind::Cylinder | FaceKind::Cone => {
                    let rho = f.radius + f.slope * h;
                    Vec3::new(rho * a.cos(), rho * a.sin(), h)
                }
            }
        };
        for f in [cylinder, cone, sphere, plane] {
            let (p, q) = (on(&f, 0.3, 0.4), on(&f, 0.9, 0.6));
            let fp = FacePlane::new(&f, p, q).unwrap();
            for (a, h) in [(0.3, 0.4), (0.5, 0.45), (0.9, 0.6)] {
                let x = on(&f, a, h);
                let (want, _) = fp.to_plane(x, Vec3::new(1.0, 0.0, 0.0)).unwrap();
                let got = fp.point(x).unwrap();
                assert!(
                    (got[0] - want[0]).abs() <= 1e-13 && (got[1] - want[1]).abs() <= 1e-13,
                    "{:?}: {got:?} against {want:?}",
                    f.kind
                );
            }
            // the segment between two points, as a weighted net: its plane net's points are
            // the points' planes
            let (x0, x1) = (on(&f, 0.4, 0.5), on(&f, 0.6, 0.5));
            let seg = [
                [x0.x, x0.y, x0.z, 1.0],
                [x1.x * 2.0, x1.y * 2.0, x1.z * 2.0, 2.0],
            ];
            let pn = fp.net(&seg).unwrap();
            let first = pn.first().unwrap();
            let last = pn.last().unwrap();
            let (w0, w1) = (fp.point(x0).unwrap(), fp.point(x1).unwrap());
            assert!((first[0] / first[2] - w0[0]).abs() <= 1e-13);
            assert!((last[1] / last[2] - w1[1]).abs() <= 1e-13);
        }
    }
}
