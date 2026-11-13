//! Closed-form iso lengths, evaluated in the side point's frame.
//!
//! Not upstream. Translation is linear in homogeneous coordinates, as in `Local::natural`.
//! Fused multiply-add forms H - O w with ONE rounding of the local result, rather than
//! separately rounding O w at the part's position. No recursion.
//!
//! # Certificate model
//!
//! The stored binary controls and the chosen anchor are exact inputs. Translating
//! them changes neither the rational map nor its parameterisation. One FMA per
//! coordinate gives `|delta H| <= gamma_1 |RN(H-Ow)|`; positive rational basis
//! functions then give a Euclidean perturbation <= `sqrt(3) max|delta H|/w_min`.
//! `certified_partials` supplies the independent de Boor/projection contribution.
//! Two such point balls give a chord ball by the triangle inequality and the
//! Euclidean norm's 1-Lipschitz property. No cancellation condition number divides
//! this absolute bound. A linear span's difference is evaluated as a scaled FULL
//! chord, rather than subtracting two rounded, almost equal interpolated points.
//!
//! Radius and chord INTERVALS are passed through monotone asin, retaining its
//! unbounded conditioning at a diameter. An uncertified orientation encloses both
//! arcs. Construction error is included in these input intervals, not put in a
//! stop allowance. `local_iso` separately bounds the actual world-restoration
//! residual, so its already-stored quantisation is not discarded or replaced by
//! a claim that local evaluation makes construction exact.
//!
//! Arithmetic uses Higham, Accuracy and Stability of Numerical Algorithms,
//! 2nd ed., §§3.1–3.4, with `u=2^-53`, `gamma_n=nu/(1-nu)`, no overflow or
//! underflow. Nonfinite bounds are refused by exact acceptance. As for the
//! kernel's other closed forms, sin/asin/cos are modelled with one-ulp error;
//! this is a separate elementary-function assumption, not a theorem of Higham.

use super::{not_exact, op_err};
use crate::error::Result;
use crate::eval::collapse::certified_partials;
use crate::eval::spline::EvalSurface;
use crate::geom::vector2d::Point2;
use crate::geom::vector3d::Vec3;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::nurbs::math::{gamma, NOW};
use crate::surface::side::{Across, Bracket};
use std::f64::consts::PI;

/// The point at `uv`, relative to the exactly chosen floating-point origin `o`, and
/// its Euclidean rounding bound. The origin need not approximate any particular point.
pub(super) fn point(s: &BSplineSurface, uv: Point2, o: Vec3) -> Result<(Vec3, f64)> {
    let net = s.gw_nurb().ok_or_else(op_err)?;
    let cp = net.cpts();
    let nu = cp.len();
    let nv = cp.first().map_or(0, Vec::len);
    let mut ctrl = Vec::new();
    let (mut mag, mut wmin) = (0.0_f64, f64::INFINITY);
    for j in 0..nv {
        for row in &cp {
            let c = row.get(j).ok_or_else(op_err)?;
            let w = if c.w == NOW { 1.0 } else { c.w };
            if !(w > 0.0 && w.is_finite()) {
                return Err(not_exact(None));
            }
            let h = [
                (-o.x).mul_add(w, c.x),
                (-o.y).mul_add(w, c.y),
                (-o.z).mul_add(w, c.z),
                w,
            ];
            mag = mag.max(h[0].abs()).max(h[1].abs()).max(h[2].abs());
            wmin = wmin.min(w);
            ctrl.push(h);
        }
    }
    let eval = EvalSurface::nurbs_closed(
        [net.p(), net.q()],
        [net.u_knots().to_vec(), net.v_knots().to_vec()],
        [nu, nv],
        ctrl,
        [false; 2],
    );
    let (d, e) = certified_partials(&eval, [uv.x, uv.y]).map_err(|_| not_exact(None))?;
    let p = d.first().and_then(|r| r.first()).ok_or_else(op_err)?;
    let err = *e.first().and_then(|r| r.first()).ok_or_else(op_err)?;
    // Higham, Accuracy and Stability of Numerical Algorithms, 2nd ed., §§3.1–3.4:
    // `gamma_1` encloses each FMA's error from its rounded LOCAL result. Division by
    // w_min and sqrt(3) turn the coordinate bound into a Euclidean one. `gamma_8`
    // widens these products, division, addition and the bound's own evaluation.
    // The evaluator includes its de Boor gamma_(12p+4) per direction and projection.
    let err = (err + 3f64.sqrt() * gamma(1) * mag / wmin) * (1.0 + gamma(8));
    if !err.is_finite() {
        return Err(not_exact(None));
    }
    Ok((Vec3::new(p.x, p.y, p.z), err))
}

/// The chord from `a` to `b`. A straight, single-span direction is measured over its
/// full span then scaled by the parameter DIFFERENCE: its tiny sub-chord has tiny error.
pub(super) fn chord(s: &BSplineSurface, a: Point2, b: Point2) -> Result<(f64, f64)> {
    let net = s.gw_nurb().ok_or_else(op_err)?;
    let along_u = a.y == b.y;
    let (degree, knots, count) = if along_u {
        (net.p(), net.u_knots(), net.cpts().len())
    } else {
        (
            net.q(),
            net.v_knots(),
            net.cpts().first().map_or(0, Vec::len),
        )
    };
    // Equal weights along a degree-one span make its Euclidean parametrisation linear.
    let cp = net.cpts();
    let equal = if along_u {
        match (cp.first(), cp.last()) {
            (Some(x), Some(y)) => x.iter().zip(y).all(|(x, y)| x.w == y.w),
            _ => false,
        }
    } else {
        cp.iter()
            .all(|r| matches!((r.first(), r.last()), (Some(x), Some(y)) if x.w == y.w))
    };
    if degree == 1 && count == 2 && equal {
        let (&lo, &hi) = (
            knots.first().ok_or_else(op_err)?,
            knots.last().ok_or_else(op_err)?,
        );
        let (x, y, from, to) = if along_u {
            (Point2::new(lo, a.y), Point2::new(hi, a.y), a.x, b.x)
        } else {
            (Point2::new(a.x, lo), Point2::new(a.x, hi), a.y, b.y)
        };
        let (length, err) = endpoint_chord(s, x, y)?;
        let scale = ((to - from) / (hi - lo)).abs();
        let value = length * scale;
        // `gamma_4`: two parameter differences, division, multiplication (Higham §3).
        return Ok((value, (err * scale + gamma(4) * value) * (1.0 + gamma(4))));
    }
    endpoint_chord(s, a, b)
}

fn endpoint_chord(s: &BSplineSurface, a: Point2, b: Point2) -> Result<(f64, f64)> {
    let origin = s.evaluate_point(a)?;
    let (x, ex) = point(s, a, origin)?;
    let (y, ey) = point(s, b, origin)?;
    let chord = (y - x).length();
    // `gamma_8`: coordinate differences, three squares, two sums and sqrt; adding the
    // two evaluation errors uses the triangle inequality, not independence (Higham §3).
    let err = (ex + ey + gamma(8) * (x.length() + y.length() + chord)) * (1.0 + gamma(8));
    Ok((chord, err))
}

/// A circle's short arc, or its complement for `long`, from a chord enclosure. Propagate
/// the whole enclosure through asin: its conditioning is unbounded at a diameter, so a
/// fixed multiple of the chord's error would not be a bound there.
pub(super) fn arc(chord: (f64, f64), radius: (f64, f64), long: bool) -> Result<(f64, f64)> {
    let (c, ec) = chord;
    let (r, er) = radius;
    if !(r > 0.0 && er >= 0.0 && ec >= 0.0) {
        return Err(not_exact(None));
    }
    let value = |c: f64, r: f64| {
        let half = (c / (2.0 * r)).clamp(0.0, 1.0).asin();
        2.0 * r * if long { PI - half } else { half }
    };
    let length = value(c, r);
    if er >= r {
        // A poorly conditioned bracket probe can still supply a nominal value;
        // its infinite enclosure cannot pass the unchanged acceptance test.
        return Ok((length, f64::INFINITY));
    }
    // `gamma_8` covers radius/chord endpoint arithmetic, division, asin's rounding
    // (one ulp), multiplication, and the complement's subtraction. As in the kernel's
    // other closed forms, elementary functions are assumed accurate to one ulp.
    // Higham §§3.1–3.4 apply to the arithmetic; monotonicity encloses asin's conditioning.
    let g = gamma(8);
    let clo = (c - ec).max(0.0) * (1.0 - g);
    let chi = (c + ec) * (1.0 + g);
    let rlo = (r - er) * (1.0 - g);
    let rhi = (r + er) * (1.0 + g);
    // Independent interval factors deliberately over-enclose their correlation.
    let alo = (clo / (2.0 * rhi)).clamp(0.0, 1.0).asin();
    let ahi = (chi / (2.0 * rlo)).clamp(0.0, 1.0).asin();
    let (lo, hi) = if long {
        (2.0 * rlo * (PI - ahi), 2.0 * rhi * (PI - alo))
    } else {
        (2.0 * rlo * alo, 2.0 * rhi * ahi)
    };
    let rounding = ((length - lo).max(hi - length) + g * hi) * (1.0 + g);
    if !(length.is_finite() && rounding.is_finite()) {
        return Err(not_exact(None));
    }
    Ok((length, rounding))
}

/// A sign used to select an arc is certified separately from its length. `e`
/// bounds each radial vector, `en` the unit normal. A 3x3 scalar triple product
/// has six products, three differences and a dot product (`gamma_32` including
/// bound arithmetic). Multilinearity encloses the input perturbations.
pub(super) fn turn_sign(a: Vec3, b: Vec3, n: Vec3, e: f64, en: f64) -> (bool, bool) {
    let (x, y) = (a.length(), b.length());
    let value = a.cross(b).dot(n);
    let error =
        ((x + y) * e + e * e + en * (x + e) * (y + e) + gamma(32) * x * y) * (1.0 + gamma(16));
    (value < 0.0, value.abs() > error)
}

/// Uncertain orientation must enclose BOTH arcs, not silently pick the short one.
/// The nominal value remains available to bracket a root; infinite/large bounds
/// cannot pass the existing exactness test.
pub(super) fn oriented_arc(
    chord: (f64, f64),
    radius: (f64, f64),
    sense: (bool, bool),
) -> Result<(f64, f64)> {
    let (length, error) = arc(chord, radius, sense.0)?;
    let error = if sense.1 {
        error
    } else {
        error + 2.0 * PI * (radius.0 + radius.1) * (1.0 + gamma(8))
    };
    Ok((length, error))
}

/// The side-centred circle offset has chord 2 r sin(angle/2). This evaluates
/// the difference directly, without subtracting nearly equal radial points.
/// Angle and radius uncertainties are propagated BEFORE asin. Higham §§3.1–3.4:
/// `gamma_8` covers halving, sine (one ulp), the two products and bound arithmetic;
/// |sin'| <= 1 supplies the absolute input-angle contribution.
pub(super) fn circle_from_angle(
    (r, er): (f64, f64),
    (angle, ea): (f64, f64),
) -> Result<(f64, f64)> {
    let sine = (0.5 * angle).sin().abs();
    let chord = 2.0 * r * sine;
    let ec = (2.0 * er * sine + (r + er) * ea + gamma(8) * chord) * (1.0 + gamma(8));
    let (length, error) = arc((chord, ec), (r, er), angle > PI)?;
    // If the angle interval straddles PI, the two choices of arc differ by
    // <= 2 r ea. Include that instead of assuming a reliable branch at a diameter.
    Ok((length, (error + 2.0 * (r + er) * ea) * (1.0 + gamma(4))))
}

pub(super) fn across((length, rounding): (f64, f64)) -> Across {
    Across {
        enclosure: Bracket {
            lower: (length - rounding).max(0.0),
            upper: length + rounding,
        },
        truncation: 0.0,
        rounding,
        converged: true,
        resolution: 0.0,
    }
}
