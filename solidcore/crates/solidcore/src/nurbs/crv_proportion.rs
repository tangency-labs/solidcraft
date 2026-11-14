//! A curve made longer keeping its control points' proportions: the same knots and count, the
//! control polygon stretched along itself from the far end.
//!
//! Not upstream. To lengthen a curve at one end by `dist`, the control polygon is read from the
//! other end (pinned) by its cumulative chord `s_i` (`s_0 = 0`, `S` its length), and continued
//! past the moving end along that end's last leg. The polygon is stretched to `S + d`: control
//! point `i` goes to the point at `s_i (S + d) / S` along the continued polygon, so each keeps
//! its share of the polygon from the pinned end, the pinned end stays, and the moving end goes
//! out along its leg by `d`. The weights stay. `d` is found so the curve's arc length grows by
//! `dist`: secant steps on the measured length ([`Curve::arc_length`]), at most [`MAX_STEPS`],
//! from `d = dist`; the reached length is returned beside the curve.
//!
//! What is kept: the knots, the degree, the count and weights of the control points, and each
//! control point's share of the polygon. What is not: the curve over its old domain (the points
//! slide along the polygon, so the shape near the pinned end moves a little as the ones near the
//! moving end move more).
//!
//! Refused: a length not finite and above 0, a curve of fewer than two control points, a
//! polygon of no length, or an end leg of no length behind the moving end (every leg from it
//! back of no length).

use crate::error::{Result, SolidError};
use crate::math::Vec3;
use crate::nurbs::crv_adv::p1::CurveEnd;
use crate::nurbs::crv_basic::Curve;
use crate::nurbs::crv_geom::ToleranceKind;
use crate::nurbs::math::CPoint;

const OP: &str = "nurbs::crv_proportion::extend_keeping_proportion";

/// The most secant steps on the stretch.
pub const MAX_STEPS: usize = 12;

/// The relative tolerance the arc lengths are measured to.
const LENGTH_TOL: f64 = 1e-9;

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

/// The control polygon from the pinned end, its cumulative chord and the moving end's unit leg
/// direction.
struct Polygon {
    points: Vec<Vec3>,
    chord: Vec<f64>,
    out: Vec3,
}

impl Polygon {
    fn of(points: Vec<Vec3>) -> Result<Polygon> {
        let mut chord = Vec::new();
        let mut s = 0.0;
        chord.push(0.0);
        for w in points.windows(2) {
            if let [a, b] = w {
                s += (*b - *a).length();
            }
            chord.push(s);
        }
        if !(s > 0.0 && s.is_finite()) {
            return Err(invalid("a control polygon of no length"));
        }
        // the last leg of some length, from the moving end back
        let out = points
            .windows(2)
            .rev()
            .find_map(|w| match w {
                [a, b] => (*b - *a).unitize().ok(),
                _ => None,
            })
            .ok_or_else(|| invalid("no leg behind the moving end"))?;
        Ok(Polygon { points, chord, out })
    }

    fn length(&self) -> f64 {
        self.chord.last().copied().unwrap_or(0.0)
    }

    /// The point at chord length `t` along the polygon, continued past its moving end.
    fn at(&self, t: f64) -> Vec3 {
        let (Some(&last), Some(&end)) = (self.points.last(), self.chord.last()) else {
            return Vec3::ZERO;
        };
        if t >= end {
            return last + self.out * (t - end);
        }
        for (k, w) in self.chord.windows(2).enumerate() {
            if let ([s0, s1], Some(&a), Some(&b)) = (w, self.points.get(k), self.points.get(k + 1))
            {
                if t <= *s1 {
                    let span = s1 - s0;
                    return if span > 0.0 {
                        a + (b - a) * ((t - s0) / span)
                    } else {
                        a
                    };
                }
            }
        }
        last
    }

    /// Each point moved to its share of the polygon stretched by `d`.
    fn stretched(&self, d: f64) -> Vec<Vec3> {
        let scale = (self.length() + d) / self.length();
        self.chord.iter().map(|s| self.at(s * scale)).collect()
    }
}

impl Curve {
    /// This curve made about `dist` longer at `end`, its control polygon stretched along itself
    /// from the other end (module notes), and the curve's arc length reached.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for the refusals the module names; those of [`Curve::arc_length`].
    pub fn extend_keeping_proportion(&self, dist: f64, end: CurveEnd) -> Result<(Curve, f64)> {
        if !(dist.is_finite() && dist > 0.0) {
            return Err(invalid("a length not finite and above 0"));
        }
        let given: Vec<(Vec3, f64)> = self.cpts().iter().map(|c| c.to_euclid_and_w()).collect();
        if given.len() < 2 {
            return Err(invalid("fewer than two control points"));
        }
        let weights: Vec<f64> = given.iter().map(|(_, w)| *w).collect();
        let mut points: Vec<Vec3> = given.iter().map(|(p, _)| *p).collect();
        if end == CurveEnd::Start {
            points.reverse();
        }
        let polygon = Polygon::of(points)?;
        let (Some(&lo), Some(&hi)) = (self.knots().first(), self.knots().last()) else {
            return Err(invalid("a curve without knots"));
        };
        let length = |c: &Curve| c.arc_length(lo, hi, LENGTH_TOL, ToleranceKind::Relative);
        let make = |d: f64| -> Curve {
            let mut moved = polygon.stretched(d);
            if end == CurveEnd::Start {
                moved.reverse();
            }
            let mut c = self.clone();
            for ((slot, p), w) in c.cpts_mut().iter_mut().zip(moved).zip(&weights) {
                *slot = CPoint::weighted(p, *w);
            }
            c
        };
        let target = length(self)? + dist;
        let (mut d0, mut l0) = (0.0, target - dist);
        let mut d1 = dist;
        let mut best = make(d1);
        let mut l1 = length(&best)?;
        for _ in 0..MAX_STEPS {
            if (l1 - target).abs() <= LENGTH_TOL * target.max(1.0)
                || (l1 - l0).abs() <= f64::EPSILON * target.max(1.0)
            {
                break;
            }
            let d2 = d1 + (target - l1) * (d1 - d0) / (l1 - l0);
            if !(d2.is_finite() && d2 > -polygon.length()) {
                break;
            }
            let next = make(d2);
            let l2 = length(&next)?;
            (d0, l0, d1, l1) = (d1, l1, d2, l2);
            best = next;
        }
        Ok((best, l1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve(points: &[[f64; 3]], degree: usize) -> Curve {
        let pw: Vec<CPoint> = points
            .iter()
            .map(|p| CPoint::from_point(Vec3::new(p[0], p[1], p[2])))
            .collect();
        let n = pw.len();
        let mut knots = vec![0.0; degree + 1];
        for i in 1..n - degree {
            knots.push(i as f64 / (n - degree) as f64);
        }
        knots.extend(vec![1.0; degree + 1]);
        Curve::new(pw, degree, knots).unwrap()
    }

    fn cvs(c: &Curve) -> Vec<Vec3> {
        c.cpts().iter().map(|p| p.to_euclid()).collect()
    }

    /// A straight line 10 long made 5 longer at its end: the end at 15, the start where it was.
    #[test]
    fn a_line_grows_along_itself_from_its_pinned_start() {
        let line = curve(&[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]], 1);
        let (out, reached) = line.extend_keeping_proportion(5.0, CurveEnd::End).unwrap();
        assert!((reached - 15.0).abs() < 1e-6, "{reached}");
        let p = cvs(&out);
        assert!(
            (p[0] - Vec3::ZERO).length() < 1e-12
                && (p[1] - Vec3::new(15.0, 0.0, 0.0)).length() < 1e-6,
            "{p:?}"
        );
    }

    /// A straight cubic with CVs at x = 0, 1, 3 and 6 made 3 longer at its end: the CVs at
    /// 0, 1.5, 4.5 and 9, each at its share of the polygon as before, the knots kept.
    #[test]
    fn a_straight_cubic_keeps_each_cvs_share_of_its_polygon() {
        let c = curve(
            &[
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [3.0, 0.0, 0.0],
                [6.0, 0.0, 0.0],
            ],
            3,
        );
        let (out, reached) = c.extend_keeping_proportion(3.0, CurveEnd::End).unwrap();
        assert!((reached - 9.0).abs() < 1e-6, "{reached}");
        assert_eq!(out.knots(), c.knots());
        let p = cvs(&out);
        for (got, x) in p.iter().zip([0.0, 1.5, 4.5, 9.0]) {
            assert!((*got - Vec3::new(x, 0.0, 0.0)).length() < 1e-6, "{p:?}");
        }
    }

    /// A curved cubic made 3 longer: the same count and knots, the start CV fixed, the arc
    /// length 3 more.
    #[test]
    fn a_curved_cubic_grows_by_the_length_asked() {
        let c = curve(
            &[
                [0.0, 0.0, 0.0],
                [2.0, 3.0, 0.0],
                [6.0, 3.0, 0.0],
                [9.0, 0.0, 0.0],
            ],
            3,
        );
        let before = c
            .arc_length(0.0, 1.0, 1e-9, ToleranceKind::Relative)
            .unwrap();
        let (out, reached) = c.extend_keeping_proportion(3.0, CurveEnd::End).unwrap();
        assert!((reached - before - 3.0).abs() < 1e-6, "{before} {reached}");
        assert_eq!(out.knots(), c.knots());
        let (old, new) = (cvs(&c), cvs(&out));
        assert_eq!(old.len(), new.len());
        assert!((old[0] - new[0]).length() < 1e-12, "the start stays");
    }

    /// Grown at its start, the end stays.
    #[test]
    fn grown_at_its_start_the_end_stays() {
        let c = curve(
            &[
                [0.0, 0.0, 0.0],
                [2.0, 3.0, 0.0],
                [6.0, 3.0, 0.0],
                [9.0, 0.0, 0.0],
            ],
            3,
        );
        let (out, _) = c.extend_keeping_proportion(2.0, CurveEnd::Start).unwrap();
        let (old, new) = (cvs(&c), cvs(&out));
        assert!((old[3] - new[3]).length() < 1e-12, "the end stays");
        assert!((new[0] - old[0]).length() > 1.0, "the start moves out");
    }

    #[test]
    fn bad_lengths_and_polygons_are_refused() {
        let c = curve(&[[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]], 1);
        assert!(c.extend_keeping_proportion(0.0, CurveEnd::End).is_err());
        assert!(c
            .extend_keeping_proportion(f64::NAN, CurveEnd::End)
            .is_err());
        let point = curve(&[[1.0, 1.0, 1.0], [1.0, 1.0, 1.0]], 1);
        assert!(point.extend_keeping_proportion(1.0, CurveEnd::End).is_err());
    }
}
