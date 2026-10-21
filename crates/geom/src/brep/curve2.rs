//! Pcurves: an edge in its face's surface parameters, (u, v). 2D B-splines only, as SMLib keeps
//! its UV trim curves (`SmBSplineCurve`); 2D lines and arcs are exact as degree-1 and rational
//! quadratic splines.
use serde::{Deserialize, Serialize};

use super::{Budget, Result, knots, numbers, require};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Curve2 {
    Spline {
        knots: Vec<f64>,
        points: Vec<[f64; 2]>,
    },
    /// Homogeneous `[u*w, v*w, w]` coefficients.
    RationalSpline {
        knots: Vec<f64>,
        homogeneous: Vec<[f64; 3]>,
    },
}

impl Curve2 {
    /// The active parameter interval `[knots[degree], knots[count]]`.
    pub fn interval(&self) -> Option<[f64; 2]> {
        let (k, count) = match self {
            Curve2::Spline { knots, points } => (knots, points.len()),
            Curve2::RationalSpline { knots, homogeneous } => (knots, homogeneous.len()),
        };
        let degree = k.len().checked_sub(count.checked_add(1)?)?;
        Some([*k.get(degree)?, *k.get(count)?])
    }

    /// The point at `t` (`None` outside the interval or for a malformed record).
    pub fn eval(&self, t: f64) -> Option<[f64; 2]> {
        let (knots, h): (&[f64], Vec<[f64; 3]>) = match self {
            Curve2::Spline { knots, points } => (knots, points.iter().map(|&[u, v]| [u, v, 1.0]).collect()),
            Curve2::RationalSpline { knots, homogeneous } => (knots, homogeneous.clone()),
        };
        let p = knots.len().checked_sub(h.len() + 1)?;
        let span = crate::bspline::find_span(knots, p, h.len(), t)?;
        let n = basis(knots, p, span, t)?;
        let mut s = [0.0; 3];
        for (j, nj) in n.iter().enumerate() {
            let q = h.get(span + j - p)?;
            for (a, x) in s.iter_mut().zip(q) {
                *a += nj * x;
            }
        }
        (s[2].abs() > 0.0).then(|| [s[0] / s[2], s[1] / s[2]])
    }
}

/// The `p + 1` basis functions non-zero on knot span `span` at `t`, of any degree (NURBS Book
/// A2.2).
fn basis(knots: &[f64], p: usize, span: usize, t: f64) -> Option<Vec<f64>> {
    if span < p || span + p >= knots.len() {
        return None;
    }
    let k = |i: usize| knots.get(i).copied().unwrap_or(0.0);
    let mut n = vec![0.0; p + 1];
    let (mut left, mut right) = (vec![0.0; p + 1], vec![0.0; p + 1]);
    *n.first_mut()? = 1.0;
    for j in 1..=p {
        *left.get_mut(j)? = t - k(span + 1 - j);
        *right.get_mut(j)? = k(span + j) - t;
        let mut saved = 0.0;
        for r in 0..j {
            let (rr, lj) = (*right.get(r + 1)?, *left.get(j - r)?);
            let den = rr + lj;
            let tmp = if den != 0.0 { n.get(r)? / den } else { 0.0 };
            *n.get_mut(r)? = saved + rr * tmp;
            saved = lj * tmp;
        }
        *n.get_mut(j)? = saved;
    }
    Some(n)
}

pub(super) fn curve2(c: &Curve2, budget: &mut Budget) -> Result<()> {
    budget.add(1)?;
    match c {
        Curve2::Spline { knots: k, points } => {
            knots(k, points.len(), budget)?;
            budget.add(points.len())?;
            numbers(points.iter().flatten().copied())
        }
        Curve2::RationalSpline { knots: k, homogeneous } => {
            knots(k, homogeneous.len(), budget)?;
            budget.add(homogeneous.len())?;
            numbers(homogeneous.iter().flatten().copied())?;
            require(homogeneous.iter().all(|p| p[2] >= 1e-12), "non-positive rational weight")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcurves_validate_and_round_trip() {
        let line = Curve2::Spline { knots: vec![0., 0., 1., 1.], points: vec![[0., 0.], [1., 2.]] };
        curve2(&line, &mut Budget(0)).unwrap();
        assert_eq!(line.interval(), Some([0., 1.]));
        assert_eq!(serde_json::from_str::<Curve2>(&serde_json::to_string(&line).unwrap()).unwrap(), line);
        let bad = Curve2::RationalSpline { knots: vec![0., 0., 1., 1.], homogeneous: vec![[0., 0., 1.], [1., 2., 0.]] };
        assert!(curve2(&bad, &mut Budget(0)).is_err());
        let nan = Curve2::Spline { knots: vec![0., 0., 1., 1.], points: vec![[0., f64::NAN], [1., 2.]] };
        assert!(curve2(&nan, &mut Budget(0)).is_err());
    }
}
