//! Rational B-spline patches, in the layout Form Studio's kernel emits, and
//! their rational Bézier pieces.
//!
//! A patch has `u_count × v_count` Cartesian poles, pole (i, j) at
//! `poles[i * v_count + j]` with i along u, optional weights (all 1 when
//! absent) and full knot vectors. Its domain is `[u_knots[p], u_knots[n + 1]]`
//! by the same in v, clamped or not.

use crate::surface3::{NurbsSurf, Surf};

use crate::piece::{Piece, H, MAX_DEGREE};
use crate::Error;

/// A tensor-product rational B-spline patch.
#[derive(Clone, Debug)]
pub struct Patch {
    /// The degree in u.
    pub u_degree: usize,
    /// The degree in v.
    pub v_degree: usize,
    /// The number of poles along u.
    pub u_count: usize,
    /// The number of poles along v.
    pub v_count: usize,
    /// The poles, (i, j) at i * `v_count` + j.
    pub poles: Vec<[f64; 3]>,
    /// The weights, laid out as the poles; all 1 when absent.
    pub weights: Option<Vec<f64>>,
    /// The full knot vector in u (`u_count` + `u_degree` + 1 knots).
    pub u_knots: Vec<f64>,
    /// The full knot vector in v.
    pub v_knots: Vec<f64>,
    /// The analytic surface the patch lies on, when known (a plane, cylinder,
    /// cone, sphere or torus): its implicit equation then bounds, and proves,
    /// how other patches meet it (see `field`).
    pub analytic: Option<Surf>,
}

/// The span index of `t`: knots[k] <= t < knots[k + 1], within [p, n]; the
/// domain's end goes to its last span. None when the knots stop short of n + 2.
fn span_of(knots: &[f64], p: usize, n: usize, t: f64) -> Option<usize> {
    let knot = |k: usize| knots.get(k).copied();
    if t >= knot(n + 1)? {
        let mut k = n;
        while k > p && !(knot(k)? < knot(k + 1)?) {
            k -= 1;
        }
        return Some(k);
    }
    let mut k = p;
    while k < n && knot(k + 1)? <= t {
        k += 1;
    }
    Some(k)
}

/// The p + 1 nonzero basis functions at `t` in span `k` (The NURBS Book, A2.2);
/// None when the knots around span k are missing.
fn basis_functions(knots: &[f64], p: usize, k: usize, t: f64) -> Option<Vec<f64>> {
    let mut n = vec![0.0; p + 1];
    let mut left = vec![0.0; p + 1];
    let mut right = vec![0.0; p + 1];
    *n.first_mut()? = 1.0;
    for j in 1..=p {
        *left.get_mut(j)? = t - knots.get((k + 1).checked_sub(j)?)?;
        *right.get_mut(j)? = knots.get(k + j)? - t;
        let mut saved = 0.0;
        for r in 0..j {
            let (right_r, left_jr) = (*right.get(r + 1)?, *left.get(j - r)?);
            let nr = n.get_mut(r)?;
            let temp = *nr / (right_r + left_jr);
            *nr = saved + right_r * temp;
            saved = left_jr * temp;
        }
        *n.get_mut(j)? = saved;
    }
    Some(n)
}

impl Patch {
    /// Weight `index`: 1 without weights, None past the weights' end.
    fn weight(&self, index: usize) -> Option<f64> {
        self.weights
            .as_ref()
            .map_or(Some(1.0), |w| w.get(index).copied())
    }

    /// The patch at (u, v) in its own knot domain; None when its counts and
    /// knots do not agree.
    pub fn evaluate(&self, u: f64, v: f64) -> Option<[f64; 3]> {
        let (p, q) = (self.u_degree, self.v_degree);
        let ku = span_of(&self.u_knots, p, self.u_count.checked_sub(1)?, u)?;
        let kv = span_of(&self.v_knots, q, self.v_count.checked_sub(1)?, v)?;
        let nu = basis_functions(&self.u_knots, p, ku, u)?;
        let nv = basis_functions(&self.v_knots, q, kv, v)?;
        let mut h = [0.0; 4];
        for (a, nua) in nu.iter().enumerate() {
            let i = (ku + a).checked_sub(p)?;
            for (b, nvb) in nv.iter().enumerate() {
                let index = i * self.v_count + (kv + b).checked_sub(q)?;
                let weight = nua * nvb * self.weight(index)?;
                let pole = self.poles.get(index)?;
                h[0] += weight * pole[0];
                h[1] += weight * pole[1];
                h[2] += weight * pole[2];
                h[3] += weight;
            }
        }
        Some([h[0] / h[3], h[1] / h[3], h[2] / h[3]])
    }

    /// The patch's rational Bézier pieces, each tagged with `index` and its knot rectangle.
    pub fn bezier_pieces(&self, index: usize) -> Result<Vec<Piece>, Error> {
        let (p, q) = (self.u_degree, self.v_degree);
        if p == 0 || q == 0 || p > MAX_DEGREE || q > MAX_DEGREE {
            return Err(Error::BadPatch(format!(
                "patch {index} has degrees ({p}, {q}); 1 to {MAX_DEGREE} are taken"
            )));
        }
        if self.u_count <= p
            || self.v_count <= q
            || self.poles.len() != self.u_count * self.v_count
            || self.u_knots.len() != self.u_count + p + 1
            || self.v_knots.len() != self.v_count + q + 1
            || self
                .weights
                .as_ref()
                .is_some_and(|w| w.len() != self.poles.len())
        {
            return Err(Error::BadPatch(format!(
                "patch {index}: pole, weight and knot counts do not agree"
            )));
        }
        let mut homogeneous = Vec::with_capacity(self.poles.len());
        for (k, pole) in self.poles.iter().enumerate() {
            let w = self.weight(k).unwrap_or(f64::NAN);
            if !(w > 0.0) {
                return Err(Error::BadPatch(format!(
                    "patch {index} has a weight that is not positive: {w}"
                )));
            }
            homogeneous.push([w * pole[0], w * pole[1], w * pole[2], w]);
        }
        // Along v for every u row, then along u for every column of each v span.
        let mut rows = Vec::with_capacity(self.u_count);
        for row in homogeneous.chunks_exact(self.v_count) {
            rows.push(decompose_curve(&self.v_knots, q, row.to_vec())?);
        }
        let closed = self.closed();
        let inconsistent =
            || Error::BadPatch(format!("patch {index}: its Bézier spans do not line up"));
        // Point k of span s of a decomposed row or column.
        let point = |spans: &[Span], s: usize, k: usize| {
            spans
                .get(s)
                .and_then(|span| span.points.get(k))
                .copied()
                .ok_or_else(inconsistent)
        };
        let mut pieces = Vec::new();
        let v_spans = rows.first().map_or(0, Vec::len);
        for s in 0..v_spans {
            let mut columns = Vec::with_capacity(q + 1);
            for j in 0..=q {
                let column = rows
                    .iter()
                    .map(|row| point(row, s, j))
                    .collect::<Result<Vec<H>, _>>()?;
                columns.push(decompose_curve(&self.u_knots, p, column)?);
            }
            let v_span = rows
                .first()
                .and_then(|row| row.get(s))
                .ok_or_else(inconsistent)?;
            let u_spans = columns.first().map_or(0, Vec::len);
            for r in 0..u_spans {
                let mut c = Vec::with_capacity((p + 1) * (q + 1));
                for i in 0..=p {
                    for column in &columns {
                        c.push(point(column, r, i)?);
                    }
                }
                let u_span = columns
                    .first()
                    .and_then(|column| column.get(r))
                    .ok_or_else(inconsistent)?;
                let above = [r + 1 < u_spans || closed[0], s + 1 < v_spans || closed[1]];
                pieces.push(Piece::new(
                    index, u_span.t0, u_span.t1, v_span.t0, v_span.t1, p, q, c, above,
                ));
            }
        }
        Ok(pieces)
    }

    /// Whether one of the patch's four boundary edges is collapsed to a point
    /// (a pole, a cone's apex): all its poles one point.
    pub fn has_collapsed_edge(&self) -> bool {
        let (m, n) = (self.u_count, self.v_count);
        let same = |ids: &mut dyn Iterator<Item = usize>| {
            let pts: Option<Vec<[f64; 3]>> = ids.map(|k| self.poles.get(k).copied()).collect();
            pts.is_some_and(|pts| {
                pts.windows(2).all(|w| match w {
                    [a, b] => a
                        .iter()
                        .zip(b)
                        .all(|(x, y)| (x - y).abs() <= 1e-12 * (1.0 + x.abs())),
                    _ => true,
                })
            })
        };
        same(&mut (0..n))
            || same(&mut (m.saturating_sub(1) * n..m * n))
            || same(&mut (0..m).map(|i| i * n))
            || same(&mut (0..m).map(|i| i * n + n - 1))
    }

    /// The patch's knot domain: [`u_start`, `u_end`, `v_start`, `v_end`]; None
    /// when the knot vectors are too short for the counts.
    pub fn domain(&self) -> Option<[f64; 4]> {
        let (p, q) = (self.u_degree, self.v_degree);
        Some([
            *self.u_knots.get(p)?,
            *self.u_knots.get(self.u_count)?,
            *self.v_knots.get(q)?,
            *self.v_knots.get(self.v_count)?,
        ])
    }

    /// The patch as a `surface3` B-spline surface, for its certified closest point.
    pub fn nurbs(&self) -> Result<NurbsSurf, Error> {
        let n = self.v_count;
        if n == 0 || self.poles.len() != self.u_count * n {
            return Err(Error::BadPatch(format!(
                "{} poles do not make {} rows of {n}",
                self.poles.len(),
                self.u_count
            )));
        }
        let ctrl: Vec<Vec<_>> = self
            .poles
            .chunks_exact(n)
            .map(|row| {
                row.iter()
                    .map(|p| crate::geom3::V3::new(p[0], p[1], p[2]))
                    .collect()
            })
            .collect();
        let weights: Option<Vec<Vec<f64>>> = self
            .weights
            .as_ref()
            .map(|w| w.chunks_exact(n).map(<[f64]>::to_vec).collect());
        NurbsSurf::new(
            self.u_degree,
            self.v_degree,
            &ctrl,
            weights.as_ref(),
            &self.u_knots,
            &self.v_knots,
        )
        .map_err(Error::BadPatch)
    }

    /// Whether the patch closes on itself in u and in v: its first and last
    /// rows of poles (and weights) coincide, so its two edges there are one seam.
    pub fn closed(&self) -> [bool; 2] {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        for pole in &self.poles {
            for ((l, h), &x) in lo.iter_mut().zip(&mut hi).zip(pole) {
                *l = l.min(x);
                *h = h.max(x);
            }
        }
        let tol = 1e-12 * hi.iter().zip(&lo).map(|(h, l)| h - l).fold(0.0, f64::max);
        let same = |i: usize, j: usize| match (
            self.poles.get(i),
            self.poles.get(j),
            self.weight(i),
            self.weight(j),
        ) {
            (Some(a), Some(b), Some(wi), Some(wj)) => {
                a.iter().zip(b).all(|(x, y)| (x - y).abs() <= tol) && (wi - wj).abs() <= 1e-12 * wi
            }
            _ => false,
        };
        let (m, n) = (self.u_count, self.v_count);
        [
            (0..n).all(|j| same(j, m.saturating_sub(1) * n + j)),
            (0..m).all(|i| same(i * n, i * n + n - 1)),
        ]
    }
}

/// One Boehm insertion of `t` into a curve of homogeneous points (The NURBS
/// Book, A5.1); None when the knots do not cover the points at degree p.
fn insert_knot(knots: &[f64], p: usize, points: &[H], t: f64) -> Option<(Vec<f64>, Vec<H>)> {
    let n = points.len().checked_sub(1)?;
    let k = span_of(knots, p, n, t)?;
    let mut out = Vec::with_capacity(n + 2);
    out.extend_from_slice(points.get(..=k.checked_sub(p)?)?);
    for i in k - p + 1..=k {
        let (ki, kip) = (*knots.get(i)?, *knots.get(i + p)?);
        let alpha = (t - ki) / (kip - ki);
        let (a, b) = (points.get(i - 1)?, points.get(i)?);
        let mut h = [0.0; 4];
        for ((hc, x), y) in h.iter_mut().zip(a).zip(b) {
            *hc = (1.0 - alpha) * x + alpha * y;
        }
        out.push(h);
    }
    out.extend_from_slice(points.get(k..)?);
    let mut grown = knots.get(..=k)?.to_vec();
    grown.push(t);
    grown.extend_from_slice(knots.get(k + 1..)?);
    Some((grown, out))
}

struct Span {
    t0: f64,
    t1: f64,
    points: Vec<H>,
}

/// A curve's Bézier spans over its domain [knots[p], knots[n + 1]]: every knot
/// value there raised to multiplicity p, clamped or not, so span k's p + 1
/// control points are points[k - p ..= k].
fn decompose_curve(knots_in: &[f64], p: usize, points_in: Vec<H>) -> Result<Vec<Span>, Error> {
    let bad = || Error::BadPatch("the knots do not cover the poles".into());
    let mut knots = knots_in.to_vec();
    let mut points = points_in;
    let n0 = points.len().checked_sub(1).ok_or_else(bad)?;
    let (start, end) = (
        *knots.get(p).ok_or_else(bad)?,
        *knots.get(n0 + 1).ok_or_else(bad)?,
    );
    if !(start < end) {
        return Err(Error::BadPatch(format!(
            "an empty knot domain [{start}, {end}]"
        )));
    }
    let mut values: Vec<f64> = knots
        .iter()
        .copied()
        .filter(|&t| t >= start && t <= end)
        .collect();
    values.sort_by(f64::total_cmp);
    values.dedup();
    for t in values {
        let mut multiplicity = knots.iter().filter(|&&knot| knot == t).count();
        while multiplicity < p {
            (knots, points) = insert_knot(&knots, p, &points, t).ok_or_else(bad)?;
            multiplicity += 1;
        }
    }
    let mut spans = Vec::new();
    for (k, pair) in knots.windows(2).enumerate().take(points.len()).skip(p) {
        if let &[t0, t1] = pair {
            if t0 < t1 && t0 >= start && t1 <= end {
                let points = points.get(k - p..=k).ok_or_else(bad)?.to_vec();
                spans.push(Span { t0, t1, points });
            }
        }
    }
    Ok(spans)
}
