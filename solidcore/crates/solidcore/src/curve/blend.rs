//! Blend curves: the curve from the end of one curve to the end of another that meets each in
//! position (G0), in tangent direction too (G1), or in curvature too (G2).
//!
//! Not upstream. The blend is one Bézier curve (a clamped B-spline with no inner knot) on
//! `[0, 1]`, of the least degree whose control points carry both ends' conditions. An end met at
//! G`k` fixes the `k + 1` control points nearest it, so the degree `n` is the sum of the two
//! orders plus one: 1 from G0 to G0, 3 from G1 to G1, 5 from G2 to G2. At a curve's end `p`,
//! with `c'` and `c''` its derivatives there taken leaving the curve (`c'` turned round at its
//! start), and `h` the end's shape factor times the chord between the two ends:
//!
//! ```text
//! P0 = p,   P1 = P0 + h c' / |c'|,   P2 = λ² c'' / (n (n - 1)) + 2 P1 - P0,   λ = n h / |c'|
//! ```
//!
//! The blend's first and second derivatives there are then `λ c'` and `λ² c''`, the curve's own
//! derivatives under the change of parameter `t = t0 ± λ u`. A change of parameter keeps the
//! tangent, the curvature and the osculating plane, so the end is met at G1 and G2 (the Bézier
//! end derivatives, `n (P1 - P0)` and `n (n - 1) (P2 - 2 P1 + P0)`; Farin, *Curves and Surfaces
//! for CAGD*, on geometric continuity). The second curve's end is met in the same way, its
//! control points counted from the blend's last. From G1 to G1 the blend is the cubic Hermite
//! curve ([`crate::curve::hermite_curve::HermiteCurve`]) with end derivatives `3 h c' / |c'|`.
//!
//! The result is checked, not trusted: each end's continuity is measured on the curve built
//! ([`evaluate_curve_continuity_with`], its geometric G2 test, at the kernel's angle tolerance
//! [`ANG_TOL_DEG`]), and a blend that does not meet an end at the contact asked is refused.
//!
//! Deviations: none (not a port). Bounded: at most degree 5, six control points, no loop.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests

use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::geom_utility::p2::{
    evaluate_curve_continuity_with, ContinuityOptions, ContinuityType, CurveEval,
};
use crate::geom::tol::{ANG_TOL_DEG, ZONE_TOL_3D};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, EFF_ZERO};
use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};

const OP: &str = "curve::blend::blend_curve";

/// Which end of a curve a blend meets: the start or the end of its natural interval.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// The start of the curve's natural interval.
    Start,
    /// The end of the curve's natural interval.
    End,
}

/// How a blend meets a curve's end.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Contact {
    /// In position.
    G0,
    /// In position and tangent direction.
    G1,
    /// In position, tangent direction and curvature (its value and its plane).
    G2,
}

impl Contact {
    /// The contact's order: 0, 1 or 2.
    #[must_use]
    pub const fn order(self) -> usize {
        match self {
            Contact::G0 => 0,
            Contact::G1 => 1,
            Contact::G2 => 2,
        }
    }
}

/// One end of a blend: the curve, which of its ends, the contact asked there and the shape
/// factor, the distance of the blend's first inner control point from the end as a fraction of
/// the chord between the two ends (not read at G0).
#[derive(Clone, Copy, Debug)]
pub struct BlendEnd<'a> {
    /// The curve.
    pub curve: &'a Curve,
    /// Which of its ends.
    pub end: End,
    /// The contact asked there.
    pub contact: Contact,
    /// The shape factor, finite and positive at G1 and G2.
    pub shape: f64,
}

/// What [`blend_curve`] made.
#[derive(Debug)]
pub struct BlendCurve {
    /// The blend, a Bézier curve on `[0, 1]` from the first end to the second.
    pub curve: Curve,
    /// Its degree.
    pub degree: usize,
    /// Its control points, `degree + 1`.
    pub control_points: Vec<Point3>,
    /// The continuity measured at each end (the first, then the second), each at least the
    /// contact asked there.
    pub measured: [ContinuityType; 2],
}

/// A curve's end and its first two derivatives there, taken leaving the curve: at its start
/// the first derivative turned round.
struct Jet {
    point: Point3,
    first: Vec3,
    second: Vec3,
}

fn finite(v: Vec3) -> bool {
    v.x.is_finite() && v.y.is_finite() && v.z.is_finite()
}

fn jet(k: &Kernel, curve: &Curve, end: End) -> Result<Jet> {
    let span = curve.natural_interval();
    let t = match end {
        End::Start => span.min(),
        End::End => span.max(),
    };
    if !t.is_finite() {
        return Err(SolidError::invalid_input(
            OP,
            "a curve end that is not at a finite parameter",
        ));
    }
    let d = curve.evaluate(k, t, 2, end == End::End, false)?;
    let (Some(&point), Some(&first), Some(&second)) = (d.first(), d.get(1), d.get(2)) else {
        return Err(SolidError::invalid_input(
            OP,
            "a curve without a second derivative at its end",
        ));
    };
    if !(finite(point) && finite(first) && finite(second)) {
        return Err(SolidError::invalid_input(
            OP,
            "a curve whose end or derivatives there are not finite",
        ));
    }
    let first = match end {
        End::Start => -first,
        End::End => first,
    };
    Ok(Jet {
        point,
        first,
        second,
    })
}

/// The control points an end fixes, from the end inward: the end itself, at G1 the point
/// `h` along the unit tangent, at G2 the point that gives the blend the curve's second
/// derivative under the change of parameter (the module's formula), `n` the degree.
fn inner(jet: &Jet, contact: Contact, shape: f64, chord: f64, n: usize) -> Result<Vec<Point3>> {
    let mut points = vec![jet.point];
    if contact == Contact::G0 {
        return Ok(points);
    }
    let speed = jet.first.length();
    if !(speed > EFF_ZERO) {
        return Err(SolidError::invalid_input(
            OP,
            "a zero-length tangent at an end met at G1 or G2",
        ));
    }
    let h = shape * chord;
    let p1 = jet.point + jet.first * (h / speed);
    points.push(p1);
    if contact == Contact::G2 {
        let nf = n as f64;
        let lambda = nf * h / speed;
        let p2 = jet.second * (lambda * lambda / (nf * (nf - 1.0))) + p1 * 2.0 - jet.point;
        points.push(p2);
    }
    Ok(points)
}

/// The order a measured continuity reaches: `None` for a gap.
fn reached(c: ContinuityType) -> Option<usize> {
    match c {
        ContinuityType::Undefined | ContinuityType::Discontinuous => None,
        ContinuityType::C0 => Some(0),
        ContinuityType::G1 | ContinuityType::G1R | ContinuityType::C1 => Some(1),
        ContinuityType::G1G2
        | ContinuityType::G1G2G3
        | ContinuityType::C1G2
        | ContinuityType::C1G2G3
        | ContinuityType::C1C2
        | ContinuityType::C1C2G3
        | ContinuityType::C1C2C3
        | ContinuityType::CInfinity => Some(2),
    }
}

/// The continuity where the curve of `jet` runs into the blend: the curve arriving at its end
/// (along the leaving derivative), the blend leaving from `d` (point and derivatives, the first
/// already pointing away from the end).
fn measure(jet: &Jet, d: [Vec3; 3]) -> Result<ContinuityType> {
    let [position, first, second] = d;
    evaluate_curve_continuity_with(
        &CurveEval {
            position: jet.point,
            first: jet.first,
            second: jet.second,
            third: None,
        },
        &CurveEval {
            position,
            first,
            second,
            third: None,
        },
        ANG_TOL_DEG,
        &ContinuityOptions { geometric_g2: true },
    )
}

/// The blend from `from`'s end to `to`'s end ([`BlendCurve`]): one Bézier curve of the least
/// degree that carries both contacts (the module's construction), its continuity at each end
/// measured and at least the contact asked.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a shape factor not finite and positive at G1 or G2, a
/// curve end that is not finite, ends no farther apart than [`ZONE_TOL_3D`], a zero-length
/// tangent at an end met at G1 or G2, or control points that are not finite;
/// [`SolidError::PortInvariant`] for a blend whose measured continuity at an end is less than
/// the contact asked; [`SolidError::Cancelled`]; those of the curves' evaluations and of
/// [`BSplineCurve::create_canonical`].
pub fn blend_curve(k: &Kernel, from: &BlendEnd<'_>, to: &BlendEnd<'_>) -> Result<BlendCurve> {
    k.check_cancelled(OP)?;
    for e in [from, to] {
        if e.contact != Contact::G0 && !(e.shape.is_finite() && e.shape > 0.0) {
            return Err(SolidError::invalid_input(
                OP,
                "a shape factor that is not finite and positive",
            ));
        }
    }
    let a = jet(k, from.curve, from.end)?;
    let b = jet(k, to.curve, to.end)?;
    let chord = a.point.distance(b.point);
    if !(chord > ZONE_TOL_3D) {
        return Err(SolidError::invalid_input(
            OP,
            "the ends coincide: no curve joins them",
        ));
    }
    let n = from.contact.order() + to.contact.order() + 1;
    let mut points = inner(&a, from.contact, from.shape, chord, n)?;
    let mut back = inner(&b, to.contact, to.shape, chord, n)?;
    back.reverse();
    points.extend(back);
    if points.len() != n + 1 || !points.iter().all(|&p| finite(p)) {
        return Err(SolidError::invalid_input(
            OP,
            "control points that are not finite",
        ));
    }
    let curve = Curve::from(BSplineCurve::create_canonical(&CanonicalData {
        dim: 3,
        degree: n,
        control_points: &points,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &[n + 1, n + 1],
        knots: &[0.0, 1.0],
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })?);
    k.check_cancelled(OP)?;
    let three = |d: Vec<Vec3>| -> Result<[Vec3; 3]> {
        match d.as_slice() {
            [p, d1, d2, ..] => Ok([*p, *d1, *d2]),
            _ => Err(SolidError::invalid_input(
                OP,
                "a blend without a second derivative at its end",
            )),
        }
    };
    let [p0, d0, dd0] = three(curve.evaluate(k, 0.0, 2, false, false)?)?;
    let [p1, d1, dd1] = three(curve.evaluate(k, 1.0, 2, true, false)?)?;
    // the blend leaves the first end along its first derivative, the second end along the
    // first derivative turned round
    let measured = [measure(&a, [p0, d0, dd0])?, measure(&b, [p1, -d1, dd1])?];
    for (m, e) in measured.iter().zip([from, to]) {
        if reached(*m) < Some(e.contact.order()) {
            return Err(SolidError::PortInvariant {
                operation: OP,
                reason: "the blend built does not meet an end at the contact asked",
            });
        }
    }
    Ok(BlendCurve {
        curve,
        degree: n,
        control_points: points,
        measured,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(p: [f64; 3]) -> Vec3 {
        Vec3::new(p[0], p[1], p[2])
    }

    fn line(p: [f64; 3], q: [f64; 3]) -> Curve {
        Curve::from(BSplineCurve::create_line_segment(3, v(p), v(q)).unwrap())
    }

    /// A Bézier curve on [0, 1] through `points`, rational with `weights`.
    fn bezier(points: &[[f64; 3]], weights: Option<&[f64]>) -> Curve {
        let control: Vec<Vec3> = points.iter().map(|&p| v(p)).collect();
        let n = control.len() - 1;
        Curve::from(
            BSplineCurve::create_canonical(&CanonicalData {
                dim: 3,
                degree: n,
                control_points: &control,
                form: BSplineCurveForm::Unspecified,
                knot_multiplicities: &[n + 1, n + 1],
                knots: &[0.0, 1.0],
                knot_type: KnotType::Unspecified,
                weights,
                trim: None,
            })
            .unwrap(),
        )
    }

    fn end(curve: &Curve, end: End, contact: Contact, shape: f64) -> BlendEnd<'_> {
        BlendEnd {
            curve,
            end,
            contact,
            shape,
        }
    }

    /// The curvature `|c' x c''| / |c'|^3` of `curve` at `t`.
    fn curvature(k: &Kernel, curve: &Curve, t: f64, from_left: bool) -> f64 {
        let d = curve.evaluate(k, t, 2, from_left, false).unwrap();
        let s = d[1].length();
        d[1].cross(d[2]).length() / (s * s * s)
    }

    fn close(a: Vec3, b: Vec3) -> bool {
        a.distance(b) < 1e-12
    }

    /// Two lines, the first ending at (10, 0, 0) along +x, the second starting at (20, 5, 0)
    /// along +x: the blend at G0, G1 and G2 is of degree 1, 3 and 5, its ends on the lines' and
    /// its continuity measured at least the contact asked at both ends.
    #[test]
    fn two_lines_at_g0_g1_and_g2() {
        let k = Kernel::default();
        let a = line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
        let b = line([20.0, 5.0, 0.0], [30.0, 5.0, 0.0]);
        let chord = v([10.0, 5.0, 0.0]).length();
        for contact in [Contact::G0, Contact::G1, Contact::G2] {
            let r = blend_curve(
                &k,
                &end(&a, End::End, contact, 0.3),
                &end(&b, End::Start, contact, 0.3),
            )
            .unwrap();
            let order = contact.order();
            assert_eq!(r.degree, 2 * order + 1, "{contact:?}");
            assert_eq!(r.control_points.len(), r.degree + 1);
            for m in r.measured {
                assert!(reached(m) >= Some(order), "{contact:?}: {m:?}");
            }
            let first = r.control_points[0];
            let last = r.control_points[r.degree];
            assert!(close(first, v([10.0, 0.0, 0.0])) && close(last, v([20.0, 5.0, 0.0])));
            if order >= 1 {
                // the inner points on each line's tangent, 0.3 of the chord from the end
                let p1 = r.control_points[1];
                let q1 = r.control_points[r.degree - 1];
                assert!(close(p1, v([10.0 + 0.3 * chord, 0.0, 0.0])), "{p1:?}");
                assert!(close(q1, v([20.0 - 0.3 * chord, 5.0, 0.0])), "{q1:?}");
            }
            if order == 2 {
                // a line has no curvature: the second points stay on the tangents
                assert!(r.control_points[2].y.abs() < 1e-12);
                assert!((r.control_points[r.degree - 2].y - 5.0).abs() < 1e-12);
                assert!(curvature(&k, &r.curve, 0.0, false) < 1e-12);
                assert!(curvature(&k, &r.curve, 1.0, true) < 1e-12);
            }
        }
    }

    /// A quarter circle of radius 10 (curvature 1/10) ending at (0, 10, 0) along -x, to a line
    /// starting at (-10, 14, 0) along -x: met at G2 the blend's curvature at its start is the
    /// circle's, its curvature vector pointing the same way (towards the centre).
    #[test]
    fn a_circle_arc_to_a_line_at_g2() {
        let k = Kernel::default();
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let arc = bezier(
            &[[10.0, 0.0, 0.0], [10.0, 10.0, 0.0], [0.0, 10.0, 0.0]],
            Some(&[1.0, w, 1.0]),
        );
        assert!((curvature(&k, &arc, 1.0, true) - 0.1).abs() < 1e-12);
        let b = line([-10.0, 14.0, 0.0], [-20.0, 14.0, 0.0]);
        let r = blend_curve(
            &k,
            &end(&arc, End::End, Contact::G2, 0.4),
            &end(&b, End::Start, Contact::G1, 0.4),
        )
        .unwrap();
        assert_eq!(r.degree, 4);
        assert!(reached(r.measured[0]) >= Some(2), "{:?}", r.measured);
        assert!(reached(r.measured[1]) >= Some(1), "{:?}", r.measured);
        let kappa = curvature(&k, &r.curve, 0.0, false);
        assert!((kappa - 0.1).abs() < 1e-9, "{kappa}");
        // the curvature vector c' x c'' x c' points down (-y), towards the circle's centre
        let d = r.curve.evaluate(&k, 0.0, 2, false, false).unwrap();
        assert!(d[1].cross(d[2]).cross(d[1]).y < 0.0);
        assert!(close(r.control_points[0], v([0.0, 10.0, 0.0])));
    }

    /// A larger shape factor sets the inner control points farther along the tangents.
    #[test]
    fn a_larger_shape_factor_pushes_the_inner_points_out() {
        let k = Kernel::default();
        let a = line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
        let b = line([20.0, 5.0, 0.0], [20.0, 15.0, 0.0]);
        for contact in [Contact::G1, Contact::G2] {
            let mut last: Option<(f64, f64, f64)> = None;
            for shape in [0.1, 0.25, 0.5, 1.0] {
                let r = blend_curve(
                    &k,
                    &end(&a, End::End, contact, shape),
                    &end(&b, End::Start, contact, shape),
                )
                .unwrap();
                let pts = &r.control_points;
                let from_a = pts[1].distance(pts[0]);
                let from_b = pts[r.degree - 1].distance(pts[r.degree]);
                // at G2 the second point too, 2 h along the first line (no curvature)
                let second = pts[2].distance(pts[0]);
                if let Some((fa, fb, s2)) = last {
                    let further = contact == Contact::G1 || second > s2;
                    assert!(from_a > fa && from_b > fb && further, "{contact:?} {shape}");
                }
                last = Some((from_a, from_b, second));
            }
        }
    }

    /// A zero-length tangent at an end met at G1 is refused (at G0 the end is enough), and so
    /// are coincident ends, a shape factor that is not positive and a start curve with no end.
    #[test]
    fn degenerate_asks_are_refused() {
        let k = Kernel::default();
        // a quadratic whose first two control points coincide: no tangent at its start
        let stub = bezier(&[[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [10.0, 0.0, 0.0]], None);
        let b = line([20.0, 5.0, 0.0], [30.0, 5.0, 0.0]);
        let to = end(&b, End::Start, Contact::G1, 0.3);
        let refused = |r: Result<BlendCurve>| matches!(r, Err(SolidError::InvalidInput { .. }));
        assert!(refused(blend_curve(
            &k,
            &end(&stub, End::Start, Contact::G1, 0.3),
            &to
        )));
        assert!(refused(blend_curve(
            &k,
            &end(&stub, End::Start, Contact::G2, 0.3),
            &to
        )));
        assert!(blend_curve(&k, &end(&stub, End::Start, Contact::G0, 0.3), &to).is_ok());
        // the second line starts where the first ends
        let a = line([0.0, 0.0, 0.0], [10.0, 0.0, 0.0]);
        let touching = line([10.0, 0.0, 0.0], [20.0, 5.0, 0.0]);
        assert!(refused(blend_curve(
            &k,
            &end(&a, End::End, Contact::G1, 0.3),
            &end(&touching, End::Start, Contact::G1, 0.3)
        )));
        for shape in [0.0, -1.0, f64::NAN, f64::INFINITY] {
            assert!(refused(blend_curve(
                &k,
                &end(&a, End::End, Contact::G1, shape),
                &to
            )));
        }
    }
}
