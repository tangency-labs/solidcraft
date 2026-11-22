//! Points of curves in their own parameters, their tangents and curvature vectors. A
//! `surface_intersection` is the point of both surfaces in the plane through its leader's
//! point normal to the leader there, found by Newton's method on the three planes.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests

use super::geometry::Curve;
use super::normal_section::normal_section;
use super::spline::{EvalCurve, MAX_SPLINE_DEGREE};
use super::vec::{v3, VecExt};
use super::{Axes, EvalError, SurfaceEval};
use crate::math::Vec3;

/// Newton steps of one intersection point.
const MAX_NEWTON: usize = 64;

fn unsupported(what: impl std::fmt::Display) -> EvalError {
    EvalError::Unsupported(what.to_string())
}

/// A curve ready to evaluate.
#[derive(Clone, Debug, PartialEq)]
pub enum CurveEval {
    /// `start + t (end - start)`.
    Line {
        /// The point at t = 0.
        start: Vec3,
        /// The point at t = 1.
        end: Vec3,
    },
    /// A circle or an ellipse.
    Conic {
        /// The frame.
        axes: Axes,
        /// The radii along x and y.
        radii: [f64; 2],
    },
    /// Straight segments, segment i over t in [i, i + 1].
    Polyline(Vec<Vec3>),
    /// A B-spline or NURBS.
    Spline(EvalCurve),
    /// The intersection of two surfaces.
    Intersection(Box<Intersection>),
}

/// A curve's differential geometry at one parameter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurveJet {
    /// The point.
    pub point: Vec3,
    /// The unit tangent, along the curve's parameter.
    pub tangent: Vec3,
    /// The curvature vector `κ N`, 1/mm: towards the centre of curvature, its length the
    /// curvature; zero where the curve is straight.
    pub curvature: Vec3,
}

impl CurveJet {
    fn is_finite(&self) -> bool {
        self.point.is_finite() && self.tangent.is_finite() && self.curvature.is_finite()
    }
}

/// A `surface_intersection` ready to evaluate.
#[derive(Clone, Debug, PartialEq)]
pub struct Intersection {
    surfaces: [SurfaceEval; 2],
    leader: CurveEval,
}

impl CurveEval {
    /// The evaluator of `c`.
    pub fn new(c: &Curve) -> Result<Self, EvalError> {
        Ok(match c {
            Curve::Line { start, end } => CurveEval::Line {
                start: v3(*start),
                end: v3(*end),
            },
            Curve::Circle { frame, radius } => CurveEval::Conic {
                axes: Axes::new(frame),
                radii: [*radius; 2],
            },
            Curve::Ellipse { frame, radii } => CurveEval::Conic {
                axes: Axes::new(frame),
                radii: *radii,
            },
            Curve::Polyline { points } => {
                CurveEval::Polyline(points.iter().copied().map(v3).collect())
            }
            Curve::Spline { knots, points } => spline(
                knots,
                points.iter().map(|p| [p[0], p[1], p[2], 1.0]).collect(),
            )?,
            Curve::RationalSpline { knots, homogeneous } => spline(knots, homogeneous.clone())?,
            Curve::SurfaceIntersection {
                surfaces, leader, ..
            } => {
                // The leader's kind is decided on the record, before descending into it: an
                // intersection nested in a leader is refused here, however deep the nest.
                if !matches!(
                    **leader,
                    Curve::Polyline { .. } | Curve::Spline { .. } | Curve::RationalSpline { .. }
                ) {
                    return Err(EvalError::Invalid("intersection leader".into()));
                }
                let leader = CurveEval::new(leader)?;
                CurveEval::Intersection(Box::new(Intersection {
                    surfaces: [
                        SurfaceEval::new(&surfaces[0])?,
                        SurfaceEval::new(&surfaces[1])?,
                    ],
                    leader,
                }))
            }
        })
    }

    /// The point at `t`.
    pub fn point(&self, t: f64) -> Option<Vec3> {
        if !t.is_finite() {
            return None;
        }
        match self {
            CurveEval::Line { start, end } => Some(*start + (*end - *start) * t),
            CurveEval::Conic { axes: a, radii } => {
                let (s, c) = t.sin_cos();
                Some(a.o + a.x * (radii[0] * c) + a.y * (radii[1] * s))
            }
            CurveEval::Polyline(_) | CurveEval::Spline(_) => self.leader(t).map(|d| d.0),
            CurveEval::Intersection(x) => x.point(t),
        }
    }

    /// Point and derivative of a polyline (truck's: the segment from ⌊t⌋, the last one at the
    /// end) or a spline.
    /// The point and first derivative at `t`; `None` for a traced intersection (its tangent
    /// is not held).
    pub fn derivs(&self, t: f64) -> Option<(Vec3, Vec3)> {
        if !t.is_finite() {
            return None;
        }
        match self {
            CurveEval::Line { start, end } => Some((*start + (*end - *start) * t, *end - *start)),
            CurveEval::Conic { axes: a, radii } => {
                let (s, c) = t.sin_cos();
                Some((
                    a.o + a.x * (radii[0] * c) + a.y * (radii[1] * s),
                    a.y * (radii[1] * c) - a.x * (radii[0] * s),
                ))
            }
            CurveEval::Polyline(_) | CurveEval::Spline(_) => self.leader(t),
            CurveEval::Intersection(_) => None,
        }
    }

    /// The point, unit tangent and curvature vector at `t` ([`CurveJet`]), exactly: from the
    /// record's first two derivatives on a line, conic or spline (in closed form, or the basis
    /// functions' own, weights divided out), straight on each segment of a polyline; on a traced
    /// intersection from both surfaces' normal curvatures along its tangent
    /// ([`Intersection::jet`]). `None` where the tangent vanishes, the two surfaces are tangent,
    /// or a value is not finite.
    pub fn jet(&self, t: f64) -> Option<CurveJet> {
        if !t.is_finite() {
            return None;
        }
        let (point, d1, d2) = match self {
            CurveEval::Line { start, end } => {
                (*start + (*end - *start) * t, *end - *start, Vec3::ZERO)
            }
            CurveEval::Conic { axes: a, radii } => {
                let (s, c) = t.sin_cos();
                let (x, y) = (a.x * (radii[0] * c), a.y * (radii[1] * s));
                (
                    a.o + x + y,
                    a.y * (radii[1] * c) - a.x * (radii[0] * s),
                    -(x + y),
                )
            }
            CurveEval::Polyline(_) => {
                let (p, d) = self.leader(t)?;
                (p, d, Vec3::ZERO)
            }
            CurveEval::Spline(c) => match c.derivatives(t, 2)?.as_slice() {
                [p, d1, d2] => (*p, *d1, *d2),
                _ => return None,
            },
            CurveEval::Intersection(x) => return x.jet(t),
        };
        let speed2 = d1.len2();
        if !(speed2 > 0.0) {
            return None;
        }
        let tangent = d1 * (1.0 / speed2.sqrt());
        // κ N = (r'' − (r''·T) T) / |r'|²: the part of the acceleration across the curve.
        let curvature = (d2 - tangent * d2.dot(tangent)) * (1.0 / speed2);
        let jet = CurveJet {
            point,
            tangent,
            curvature,
        };
        jet.is_finite().then_some(jet)
    }

    fn leader(&self, t: f64) -> Option<(Vec3, Vec3)> {
        match self {
            CurveEval::Polyline(p) => {
                let n = p.len();
                let last = *p.last()?;
                if t <= 0.0 {
                    return Some((*p.first()?, *p.get(1)? - *p.first()?));
                }
                if t + 1.0 >= n as f64 {
                    return Some((last, last - *p.get(n.checked_sub(2)?)?));
                }
                let i = t as usize;
                let (a, b) = (*p.get(i)?, *p.get(i + 1)?);
                Some((a + (b - a) * (t - i as f64), b - a))
            }
            CurveEval::Spline(c) => c.eval(t),
            _ => None,
        }
    }
}

fn spline(knots: &[f64], ctrl: Vec<[f64; 4]>) -> Result<CurveEval, EvalError> {
    let degree = knots
        .len()
        .checked_sub(ctrl.len() + 1)
        .ok_or_else(|| EvalError::Invalid("spline degree".into()))?;
    if !(1..=MAX_SPLINE_DEGREE).contains(&degree) {
        return Err(unsupported(format!("a spline curve of degree {degree}")));
    }
    Ok(CurveEval::Spline(EvalCurve::new(
        degree,
        knots.to_vec(),
        ctrl,
    )))
}

impl Intersection {
    /// The point in the leader's normal plane at `t`, found from the leader's point: a function
    /// of `t` alone, whatever was evaluated before (and so the same on every thread).
    fn point(&self, t: f64) -> Option<Vec3> {
        let (lead, along) = self.leader.leader(t)?;
        if !(along.len2() > 0.0) {
            return None;
        }
        self.solve(lead, along)
    }

    /// The curve's [`CurveJet`] at `t`. The tangent is square to both surfaces' normals (along the
    /// leader); the curvature vector lies in the plane of those normals, and its component along
    /// each is that surface's normal curvature in the tangent's direction (Meusnier), from the
    /// surface's exact partial derivatives ([`normal_section`]): two equations for its two
    /// components. `None` where the surfaces are tangent (their normals do not span a plane).
    fn jet(&self, t: f64) -> Option<CurveJet> {
        let (lead, along) = self.leader.leader(t)?;
        if !(along.len2() > 0.0) {
            return None;
        }
        let point = self.solve(lead, along)?;
        let mut feet = [(Vec3::ZERO, None); 2];
        for (foot, s) in feet.iter_mut().zip(&self.surfaces) {
            let q = s.project(point, None, f64::INFINITY)?;
            let d = s.partials(q[0], q[1])?;
            *foot = (d[1][0].cross(d[0][1]).normalized()?, Some(d));
        }
        let [(n0, Some(d0)), (n1, Some(d1))] = feet else {
            return None;
        };
        let mut tangent = n0.cross(n1).normalized()?;
        if tangent.dot(along) < 0.0 {
            tangent = -tangent;
        }
        let k0 = normal_section(&d0, n0, tangent)?.curvature;
        let k1 = normal_section(&d1, n1, tangent)?.curvature;
        // κN = a n0 + b n1 with κN·n0 = k0 and κN·n1 = k1.
        let c = n0.dot(n1);
        let det = 1.0 - c * c;
        if !(det > 1e-12) {
            return None;
        }
        let (a, b) = ((k0 - c * k1) / det, (k1 - c * k0) / det);
        let jet = CurveJet {
            point,
            tangent,
            curvature: n0 * a + n1 * b,
        };
        jet.is_finite().then_some(jet)
    }

    /// Newton on the planes: each surface's tangent plane at the foot of the current point, and
    /// the leader's normal plane, until the point stops moving.
    fn solve(&self, lead: Vec3, along: Vec3) -> Option<Vec3> {
        let mut x = lead;
        let mut uv: Option<[[f64; 2]; 2]> = None;
        for _ in 0..MAX_NEWTON {
            let mut planes = [(Vec3::ZERO, 0.0); 2];
            let mut feet = [[0.0; 2]; 2];
            for (i, s) in self.surfaces.iter().enumerate() {
                let q = s.project(x, uv.and_then(|h| h.get(i).copied()), f64::INFINITY)?;
                let (f, su, sv) = s.derivs(q[0], q[1])?;
                let n = su.cross(sv).normalized()?;
                *planes.get_mut(i)? = (n, n.dot(f));
                *feet.get_mut(i)? = q;
            }
            uv = Some(feet);
            let [(n0, d0), (n1, d1)] = planes;
            let y = solve3([n0, n1, along], [d0, d1, along.dot(lead)])?;
            let step = y.dist(x);
            x = y;
            if step <= 1e-12 * (1.0 + x.len()) {
                return Some(x);
            }
        }
        None
    }
}

/// The point on the three planes `rows[i] · x = rhs[i]` (Cramer's rule), `None` when they meet
/// in no single point (tangent surfaces).
fn solve3(rows: [Vec3; 3], rhs: [f64; 3]) -> Option<Vec3> {
    let [a, b, c] = rows;
    let (bc, ca, ab) = (b.cross(c), c.cross(a), a.cross(b));
    let det = a.dot(bc);
    if !(det.abs() > 1e-12 * a.len() * b.len() * c.len()) {
        return None;
    }
    let x = (bc * rhs[0] + ca * rhs[1] + ab * rhs[2]) * (1.0 / det);
    x.is_finite().then_some(x)
}
