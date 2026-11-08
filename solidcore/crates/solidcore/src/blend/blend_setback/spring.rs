//! The spring curve on a face at a setback corner: the curve from one round's rail end to the
//! next one's, exactly on the face and tangent to both rails, built in the face's rational
//! parameter plane (solidcore#2343, built in solidcore from the SolidCraft fork's work in
//! progress on smlib#85 part C, `SmBlendSetback::SpringCurve` at tangency-labs/smlib
//! 37c9e1d; no fork commit).
//!
//! The parameter plane: a plane's own coordinates along two unit directions in it; a
//! cylinder's or cone's `(t, h)`, `t = tan(theta / 2)` about the axis from the radial
//! direction `e1` halfway between P's and Q's, h along the axis; a sphere's stereographic
//! plane `(a, b)` from the point opposite the middle of P's and Q's directions. A tangent T
//! at a point maps by the parametrization's differential: `dtheta = T . e_theta / rho`,
//! `dt = (1 + t^2) dtheta / 2`, `dh = T . axis`; on the sphere `(a, b) = (u1, u2) / (R + u3)`,
//! u the point's coordinates from the centre along `(e1, e2, m)`, so
//! `da = (du1 (R + u3) - u1 du3) / (R + u3)^2`. The parabola `(P^, X^, Q^)`, `X^` where
//! `P^ + a dP^` meets `Q^ - b dQ^` (`a, b > 0`), mapped up (Hoschek 1992: a quadric's
//! quadratic rational parametrization maps a parameter-plane curve of degree n to a rational
//! curve of degree 2n on it; Dietz, Hoschek and Juttler 1993):
//! - cylinder: `W = 1 + t^2`, `(X, Y) = R (1 - t^2, 2 t)`, the axial `h W` of degree 6
//!   replaced by the quartic with its end values and derivatives (its middle coefficient its
//!   neighbours' mean): the curve stays on the cylinder (`X^2 + Y^2 = R^2 W^2` whatever the
//!   axial coordinate) and keeps its end tangents, degree 4;
//! - cone: `W = 1 + t^2`, `(X, Y) = rho(h) (1 - t^2, 2 t)`, the axial `h W`, all of degree 6;
//! - sphere: `W = 1 + a^2 + b^2`, `R (2 a, 2 b, 1 - a^2 - b^2)` about the centre, degree 4;
//! - plane: the parabola itself, degree 2.
//!
//! | Rust                 | Fork (`SmBlendSetback`, 37c9e1d)  |
//! |----------------------|-----------------------------------|
//! | [`spring_curve`]     | `SmBlendSetback::SpringCurve`     |
//! | [`SpringCurve`], [`SpringHalf`] | its out-params `rFromP`, `rFromQ`, `pTanP`, `pTanQ` |
//!
//! Deviations: the halves and their partials are returned as homogeneous control points, not
//! as flat arrays; the reason of a refusal is the [`Refusal`]'s. No recursion.

use super::{axpy, bern_elevate, bern_mul, el, el_mut, sm_err, Bern, Hom, Refusal, HOM_ZERO};
use super::{Face, FaceKind};
use crate::error::Result;
use crate::geom::vector3d::sqrt;
use crate::math::{Point3, Vec3, EFF_ZERO, PI};

/// One half of a spring curve, from its rail end (`curve[0]`) to the base point (its last
/// control point).
#[derive(Debug, Clone, PartialEq)]
pub struct SpringHalf {
    /// Its homogeneous Bernstein coefficients `(x w, y w, z w, w)`: of degree 2 on a plane, 4
    /// on a cylinder or sphere, 6 on a cone.
    pub curve: Vec<[f64; 4]>,
    /// Two homogeneous polynomials along it (Bernstein, `(x, y, z, w)`) that with the half
    /// span the face's tangent plane there projectively: the face's parametric partials along
    /// the parameter-plane parabola (a plane's two directions, degree 0; a cylinder's
    /// circumferential direction, 4, and its axis, 0; a cone's circumferential direction and
    /// generator, 4 and 4; a sphere's stereographic partials, 2 and 2). A column next to the
    /// half is G1 to the face exactly where it differs from the half by
    /// `alpha T1 + beta T2 + gamma H0` (Hahn's connecting functions).
    pub partials: [Vec<[f64; 4]>; 2],
}

/// [`spring_curve`]'s two halves, meeting at its middle (the base point).
#[derive(Debug, Clone, PartialEq)]
pub struct SpringCurve {
    /// The half from P (`rFromP`, `pTanP`).
    pub from_p: SpringHalf,
    /// The half from Q (`rFromQ`, `pTanQ`).
    pub from_q: SpringHalf,
}

/// The parameter-plane point and tangent of a point `x` with a tangent `t` on `face`, in the
/// frame `(o, e1, e2, e3)` (the closure `toPlane`). `SM_ERR` where the point is on a
/// quadric's axis or pole, or more than a half turn about a cylinder's or cone's axis.
fn to_plane(
    face: &Face,
    frame: (Vec3, Vec3, Vec3, Vec3),
    x: Point3,
    t: Vec3,
) -> Result<([f64; 2], [f64; 2])> {
    let (o, e1, e2, e3) = frame;
    let r = face.radius;
    let u = x - o;
    let (u1, u2, u3) = (u.dot(e1), u.dot(e2), u.dot(e3));
    let (d1, d2, d3) = (t.dot(e1), t.dot(e2), t.dot(e3));
    match face.kind {
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

/// The face's parametric partials along the parameter-plane parabola `(cu, cv)` (the closure
/// `tangents`).
fn partials(
    face: &Face,
    frame: (Vec3, Vec3, Vec3, Vec3),
    cu: &[f64],
    cv: &[f64],
) -> Result<[Vec<Hom>; 2]> {
    let (o, e1, e2, e3) = frame;
    let r = face.radius;
    let mut xyz: [Vec<Vec3>; 2] = [Vec::new(), Vec::new()];
    let mut w: [Vec<f64>; 2] = [Vec::new(), Vec::new()];
    match face.kind {
        FaceKind::Plane => {
            xyz = [vec![e1], vec![e2]];
            w = [vec![0.0], vec![0.0]];
        }
        FaceKind::Sphere => {
            // S(a, b) = O (1 + a^2 + b^2) + R (2 a e1 + 2 b e2 + (1 - a^2 - b^2) m),
            // w = 1 + a^2 + b^2: S_a = (2 a O + R (2 e1 - 2 a m), 2 a), S_b likewise; affine in
            // (a, b), quadratic along it
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
            if face.kind == FaceKind::Cylinder {
                *x1 = vec![e3];
                *w1 = vec![0.0];
            } else {
                let kk = face.slope;
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

/// `SmBlendSetback::SpringCurve`: the spring curve on `face` from `p` to `q`, tangent there
/// to the unit directions `tp` (leaving P) and `tq` (arriving at Q), in the face's rational
/// parameter plane (module docs), returned as its two halves meeting at its middle (the base
/// point), each from its rail end, with the face's partials along it.
///
/// # Errors
/// [`Refusal`] with `SM_ERR_INVALID_INPUT` and its reason where the tangent lines' images
/// run parallel or cross behind P or Q (no corner region between the rails), or a weight
/// would not stay positive; `SM_ERR` (no reason) where the frame cannot be made, or a point
/// lies on a quadric's axis or pole or more than a half turn about its axis from the other.
pub fn spring_curve(
    face: &Face,
    p: Point3,
    tp: Vec3,
    q: Point3,
    tq: Vec3,
) -> core::result::Result<SpringCurve, Refusal> {
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
    let r = face.radius;
    let frame = (o, e1, e2, e3);

    let (pp, dp) = to_plane(face, frame, p, tp)?;
    let (qq, dq) = to_plane(face, frame, q, tq)?;
    // X^: P^ + a dP^ = Q^ - b dQ^, i.e. a dP^ + b dQ^ = Q^ - P^
    let det = dp[0] * dq[1] - dp[1] * dq[0];
    if !(det.abs() > EFF_ZERO) {
        return Err(Refusal::invalid(
            "SmBlendSetback: two rails on a face run parallel (no corner region between them)",
        ));
    }
    let r0 = qq[0] - pp[0];
    let r1 = qq[1] - pp[1];
    let a = (r0 * dq[1] - r1 * dq[0]) / det;
    let b = (dp[0] * r1 - dp[1] * r0) / det;
    if !(a > 0.0) || !(b > 0.0) {
        return Err(Refusal::invalid(
            "SmBlendSetback: two rounds' rails on a face cross behind their ends (no corner region between them)",
        ));
    }
    let su: Bern = vec![pp[0], pp[0] + a * dp[0], qq[0]];
    let sv: Bern = vec![pp[1], pp[1] + a * dp[1], qq[1]];

    // the homogeneous curve's components about the frame (c1, c2, c3 along e1, e2, e3,
    // then w)
    let (c1, c2, c3, w): (Bern, Bern, Bern, Bern) = match face.kind {
        FaceKind::Plane => (su.clone(), sv.clone(), vec![0.0; 3], vec![1.0; 3]),
        FaceKind::Sphere => {
            let uu = bern_mul(&su, &su)?;
            let vv = bern_mul(&sv, &sv)?;
            let u4 = bern_elevate(&su, 4)?;
            let v4 = bern_elevate(&sv, 4)?;
            let uu4 = bern_elevate(&uu, 4)?;
            let vv4 = bern_elevate(&vv, 4)?;
            let mut w = vec![0.0; 5];
            let mut c1 = vec![0.0; 5];
            let mut c2 = vec![0.0; 5];
            let mut c3 = vec![0.0; 5];
            for i in 0..5 {
                *el_mut(&mut w, i)? = 1.0 + el(&uu4, i)? + el(&vv4, i)?;
                *el_mut(&mut c1, i)? = r * 2.0 * el(&u4, i)?;
                *el_mut(&mut c2, i)? = r * 2.0 * el(&v4, i)?;
                *el_mut(&mut c3, i)? = r * (1.0 - el(&uu4, i)? - el(&vv4, i)?);
            }
            (c1, c2, c3, w)
        }
        FaceKind::Cylinder | FaceKind::Cone => {
            // t quadratic: t^2, W = 1 + t^2 and 1 - t^2 quartic, 2 t raised to 4
            let tt = bern_mul(&su, &su)?;
            let mut sw = vec![0.0; 5];
            let mut one_minus = vec![0.0; 5];
            for i in 0..5 {
                *el_mut(&mut sw, i)? = 1.0 + el(&tt, i)?;
                *el_mut(&mut one_minus, i)? = 1.0 - el(&tt, i)?;
            }
            let two_t = bern_elevate(
                &[2.0 * el(&su, 0)?, 2.0 * el(&su, 1)?, 2.0 * el(&su, 2)?],
                4,
            )?;
            if face.kind == FaceKind::Cylinder {
                let c1: Bern = one_minus.iter().map(|&x| x * r).collect();
                let c2: Bern = two_t.iter().map(|&x| x * r).collect();
                // the axial coordinate: the quartic with h W's end values and derivatives
                // (h W is of degree 6), its middle coefficient its neighbours' mean
                let hw = bern_mul(&sv, &sw)?;
                let mut c3 = vec![0.0; 5];
                *el_mut(&mut c3, 0)? = el(&hw, 0)?;
                *el_mut(&mut c3, 4)? = el(&hw, 6)?;
                *el_mut(&mut c3, 1)? = el(&hw, 0)? + (el(&hw, 1)? - el(&hw, 0)?) * 6.0 / 4.0;
                *el_mut(&mut c3, 3)? = el(&hw, 6)? - (el(&hw, 6)? - el(&hw, 5)?) * 6.0 / 4.0;
                *el_mut(&mut c3, 2)? = 0.5 * (el(&c3, 1)? + el(&c3, 3)?);
                (c1, c2, c3, sw)
            } else {
                let rho: Bern = vec![
                    r + face.slope * el(&sv, 0)?,
                    r + face.slope * el(&sv, 1)?,
                    r + face.slope * el(&sv, 2)?,
                ];
                (
                    bern_mul(&rho, &one_minus)?,
                    bern_mul(&rho, &two_t)?,
                    bern_mul(&sv, &sw)?,
                    bern_elevate(&sw, 6)?,
                )
            }
        }
    };
    let d = w.len().checked_sub(1).ok_or_else(sm_err)?;
    // homogeneous world coordinates
    let mut curve = vec![HOM_ZERO; d + 1];
    for (i, h) in curve.iter_mut().enumerate() {
        let wi = el(&w, i)?;
        let x = o * wi + e1 * el(&c1, i)? + e2 * el(&c2, i)? + e3 * el(&c3, i)?;
        if !(wi > 0.0) {
            return Err(Refusal::invalid(
                "SmBlendSetback: a spring curve's weight would not stay positive",
            ));
        }
        *h = [x.x, x.y, x.z, wi];
    }
    // the halves at s = 1/2 (de Casteljau), the second reversed (from Q)
    let mut left = vec![HOM_ZERO; d + 1];
    let mut right = vec![HOM_ZERO; d + 1];
    let mut work = curve;
    for k in 0..=d {
        *el_mut(&mut left, k)? = el(&work, 0)?;
        *el_mut(&mut right, k)? = el(&work, d - k)?; // from Q (k = 0) to the middle (k = d)
        for i in 0..d - k {
            let wi = el(&work, i)?;
            let wn = el(&work, i + 1)?;
            *el_mut(&mut work, i)? = axpy(&axpy(&HOM_ZERO, 0.5, &wi), 0.5, &wn);
        }
    }

    // the tangent directions along each half: the parameter-plane parabola split at its
    // middle (from P, and from Q), the face's partials there
    let (u0, u1, u2) = (el(&su, 0)?, el(&su, 1)?, el(&su, 2)?);
    let (v0, v1, v2) = (el(&sv, 0)?, el(&sv, 1)?, el(&sv, 2)?);
    let ul = [u0, 0.5 * (u0 + u1), 0.25 * (u0 + 2.0 * u1 + u2)];
    let vl = [v0, 0.5 * (v0 + v1), 0.25 * (v0 + 2.0 * v1 + v2)];
    let ur = [u2, 0.5 * (u1 + u2), ul[2]];
    let vr = [v2, 0.5 * (v1 + v2), vl[2]];
    Ok(SpringCurve {
        from_p: SpringHalf {
            curve: left,
            partials: partials(face, frame, &ul, &vl)?,
        },
        from_q: SpringHalf {
            curve: right,
            partials: partials(face, frame, &ur, &vr)?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A homogeneous Bezier curve at `t` (de Casteljau), as a Euclidean point.
    fn at(h: &[[f64; 4]], t: f64) -> Point3 {
        let mut c = h.to_vec();
        for r in 1..c.len() {
            for i in 0..c.len() - r {
                let next = c[i + 1];
                for (a, b) in c[i].iter_mut().zip(next) {
                    *a = (1.0 - t) * *a + t * b;
                }
            }
        }
        Vec3::new(c[0][0] / c[0][3], c[0][1] / c[0][3], c[0][2] / c[0][3])
    }

    fn euclid_of(h: &[f64; 4]) -> Point3 {
        Vec3::new(h[0] / h[3], h[1] / h[3], h[2] / h[3])
    }

    /// The spring curve from `p` (leaving along `tp`) to `q` (arriving along `tq`) on `face`
    /// lies on it to 1e-12 of `size` (33 stations a half), starts at P and Q, leaves them
    /// along their tangents, and its halves meet. The tangents to 1e-10 rad: each passes
    /// through the face's parameter-plane map and back (tan of half the angle about the
    /// axis), which leaves a rounding of order 1e-11 at these sizes.
    fn on_face(face: &Face, p: Point3, tp: Vec3, q: Point3, tq: Vec3, size: f64) {
        let sc = spring_curve(face, p, tp, q, tq).unwrap();
        for half in [&sc.from_p, &sc.from_q] {
            for i in 0..=32 {
                let x = at(&half.curve, f64::from(i) / 32.0);
                let d = face.distance(x).0.abs();
                assert!(d <= 1e-12 * size, "{:?}: {d} off at {i}", face.kind);
            }
        }
        let (fp, fq) = (&sc.from_p.curve, &sc.from_q.curve);
        assert!(euclid_of(&fp[0]).distance(p) <= 1e-12 * size);
        assert!(euclid_of(&fq[0]).distance(q) <= 1e-12 * size);
        let (ep, eq) = (euclid_of(fp.last().unwrap()), euclid_of(fq.last().unwrap()));
        assert!(
            ep.distance(eq) <= 1e-12 * size,
            "halves apart by {}",
            ep.distance(eq)
        );
        let angle = |a: Vec3, b: Vec3| a.cross(b).length().atan2(a.dot(b));
        let leave_p = euclid_of(&fp[1]) - euclid_of(&fp[0]);
        let leave_q = euclid_of(&fq[1]) - euclid_of(&fq[0]);
        assert!(angle(leave_p, tp) <= 1e-10, "{}", angle(leave_p, tp));
        assert!(angle(leave_q, -tq) <= 1e-10, "{}", angle(leave_q, -tq));
    }

    /// A point and the unit tangent of the curve `(theta, h)(s)` on a surface of revolution
    /// about z of radius `rho(h)`, at s (central differences of the closed form, 1e-6).
    fn on_revolution(
        rho: impl Fn(f64) -> f64,
        theta: impl Fn(f64) -> f64,
        h: impl Fn(f64) -> f64,
        s: f64,
    ) -> (Point3, Vec3) {
        let x = |s: f64| {
            let (t, z) = (theta(s), h(s));
            Vec3::new(rho(z) * t.cos(), rho(z) * t.sin(), z)
        };
        let d = 1e-6;
        (x(s), (x(s + d) - x(s - d)).unitize().unwrap())
    }

    #[test]
    fn spring_curves_lie_on_a_cylinder_a_cone_and_a_sphere() {
        let cylinder = Face {
            point: Vec3::new(0.0, 0.0, 0.0),
            kind: FaceKind::Cylinder,
            axis: Vec3::new(0.0, 0.0, 1.0),
            radius: 10.0,
            ..Face::default()
        };
        let theta = |s: f64| 0.3 + 0.6 * s;
        let h = |s: f64| 2.0 + s + 0.5 * s * (1.0 - s);
        let (p, tp) = on_revolution(|_| 10.0, theta, h, 0.0);
        let (q, tq) = on_revolution(|_| 10.0, theta, h, 1.0);
        on_face(&cylinder, p, tp, q, tq, 10.0);

        // a cone of apex the origin, radius h / 2 at h along z
        let cone = Face {
            point: Vec3::new(0.0, 0.0, 0.0),
            kind: FaceKind::Cone,
            axis: Vec3::new(0.0, 0.0, 1.0),
            radius: 0.0,
            slope: 0.5,
            ..Face::default()
        };
        let h = |s: f64| 6.0 + 2.0 * s + 0.5 * s * (1.0 - s);
        let (p, tp) = on_revolution(|z| 0.5 * z, theta, h, 0.0);
        let (q, tq) = on_revolution(|z| 0.5 * z, theta, h, 1.0);
        on_face(&cone, p, tp, q, tq, 8.0);

        // a sphere of radius 10: latitude phi, longitude theta
        let sphere = Face {
            point: Vec3::new(0.0, 0.0, 0.0),
            kind: FaceKind::Sphere,
            radius: 10.0,
            ..Face::default()
        };
        let x = |s: f64| {
            let (t, f) = (0.3 + 0.6 * s, 0.2 + 0.3 * s + 0.2 * s * (1.0 - s));
            Vec3::new(
                10.0 * f.cos() * t.cos(),
                10.0 * f.cos() * t.sin(),
                10.0 * f.sin(),
            )
        };
        let d = 1e-6;
        let tan = |s: f64| (x(s + d) - x(s - d)).unitize().unwrap();
        on_face(&sphere, x(0.0), tan(0.0), x(1.0), tan(1.0), 10.0);

        // a plane
        let plane = Face::plane(Vec3::new(0.0, 0.0, 1.0), Vec3::new(0.0, 0.0, 1.0));
        on_face(
            &plane,
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.2, 0.0).unitize().unwrap(),
            Vec3::new(3.0, 2.0, 1.0),
            Vec3::new(0.2, 1.0, 0.0).unitize().unwrap(),
            3.0,
        );
    }
}
