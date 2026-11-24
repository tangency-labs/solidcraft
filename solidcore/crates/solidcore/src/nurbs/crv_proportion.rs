//! A curve made longer keeping its control points' proportions: the same knots and count, the
//! control polygon stretched along itself from the far end.
//!
//! Not upstream. To lengthen a curve at one end by `dist`, the control polygon is read from the
//! other end (pinned) by its cumulative chord `s_i` (`s_0 = 0`, `S` its length), and continued
//! past the moving end along that end's last leg. The polygon is stretched to `S + d`: control
//! point `i` goes to the point at `s_i (S + d) / S` along the continued polygon, so each keeps
//! its share of the polygon from the pinned end, the pinned end stays, and the moving end goes
//! out along its leg by `d`. The weights stay.
//!
//! `d` is found so the curve's arc length grows by `dist`. The arc length is not monotone in
//! `d`: where the polygon turns back (a hook at the moving end), points slide round the turn
//! and the curve can get shorter as `d` grows. It is continuous in `d` (each point moves
//! continuously along the polygon), it is the curve's own length at `d = 0`, and it grows
//! without bound (every point but the pinned one runs out along the end leg, so the moving end
//! leaves the pinned one). So a `d` whose length is the one asked exists past any bracket
//! `[d_lo, d_hi]` with the measured length short at `d_lo` and not short at `d_hi`: `d_hi` is
//! found by doubling from `dist` ([`MAX_DOUBLINGS`]), then the root by regula falsi with the
//! Illinois step (at most [`MAX_STEPS`]).
//!
//! Lengths are measured by certified enclosures (`length_enclosure`: per Bézier piece, the
//! chord below and the control polygon above, for a polynomial piece also a third-order bound
//! from its speed, for a rational piece also a bound from its derivative, rounding counted,
//! pieces halved until the enclosure is narrow), to a quarter of the acceptance margin; the search
//! steers on their middles. The result is accepted only when the enclosures of the curve and
//! of the result differ by within [`ACCEPT_TOL`] `dist` of `dist`. Otherwise the extension is
//! refused
//! ([`ExtendRefusal::LengthNotMet`]); it never returns a curve short of, or past, the length.
//!
//! What is kept: the knots, the degree, the count and weights of the control points, and each
//! control point's share of the polygon. What is not: the curve over its old domain (the points
//! slide along the polygon, so the shape near the pinned end moves a little as the ones near the
//! moving end move more).
//!
//! Refused: a length not finite and above 0, a curve of fewer than two control points, a
//! polygon of no length, or an end leg of no length behind the moving end (every leg from it
//! back of no length), as invalid input; a length not met or not certified as above.
//!
//! Deviations: none (not upstream); no recursion (the enclosure's pieces are refined in rounds
//! over a vector, at most [`MAX_PIECES`] pieces, each halved at most [`MAX_DEPTH`] times).

use crate::error::{Result, SolidError};
use crate::math::Vec3;
use crate::nurbs::crv_adv::p1::CurveEnd;
use crate::nurbs::crv_basic::Curve;
use crate::nurbs::math::{gamma, CPoint, NOW, NOZ};
use crate::surface::extend::ExtendRefusal;

const OP: &str = "nurbs::crv_proportion::extend_keeping_proportion";

/// The most regula falsi steps on the stretch, once it is bracketed.
pub const MAX_STEPS: usize = 60;

/// The most doublings of the stretch to bracket the length asked.
pub const MAX_DOUBLINGS: usize = 64;

/// How close, relative to `dist`, the certified growth in arc length must be to `dist` for the
/// extension to be accepted.
pub const ACCEPT_TOL: f64 = 1e-7;

/// The most Bézier pieces an arc-length enclosure refines to.
pub const MAX_PIECES: usize = 1 << 15;

/// The most times one piece of an enclosure is halved.
pub const MAX_DEPTH: u32 = 40;

/// The most refinement rounds of an enclosure.
const MAX_ROUNDS: usize = 64;

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

fn not_met() -> SolidError {
    SolidError::ExtendRefused {
        operation: OP,
        refusal: ExtendRefusal::LengthNotMet,
        distance: None,
    }
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
    /// This curve made `dist` longer at `end`, its control polygon stretched along itself from
    /// the other end (module notes), and the middle of its certified arc length. The growth in
    /// arc length is certified within [`ACCEPT_TOL`] `dist` of `dist`.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for the refusals the module names; [`SolidError::ExtendRefused`] with
    /// [`ExtendRefusal::LengthNotMet`] where the length is not bracketed, not found, or not
    /// certified; those of the Bézier decomposition.
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
        // accepted on the certified growth only, within `ACCEPT_TOL dist` of `dist`: relative to
        // the growth asked, not the whole length, so no growth short of `dist (1 - ACCEPT_TOL)`
        // passes however long the curve (a margin relative to the length would accept none at
        // all for `dist` under `ACCEPT_TOL` times it). The lengths are enclosed to a quarter of
        // the margin; a curve that cannot be (a growth too small for its length to resolve)
        // refuses. With the search stopped within a tenth of the margin of the middles, the
        // certified growth is within `0.1 + 0.25` of the margin of `dist`.
        let margin = ACCEPT_TOL * dist;
        let width = 0.25 * margin;
        let enclose = |c: &Curve| -> Result<(f64, f64)> {
            match length_enclosure(c, width)? {
                Some(e) => Ok(e),
                None => Err(not_met()),
            }
        };
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
        let old = enclose(self)?;
        let target = 0.5 * (old.0 + old.1) + dist;
        if !target.is_finite() {
            return Err(not_met());
        }
        // the shortfall from the length asked, measured at the enclosure's middle, with the
        // enclosure it was measured on
        let shortfall = |d: f64| -> Result<(f64, (f64, f64))> {
            let e = enclose(&make(d))?;
            Ok((0.5 * (e.0 + e.1) - target, e))
        };
        // the bracket: short at d_lo (the curve itself), not short at d_hi
        let (mut d_lo, mut g_lo) = (0.0_f64, -dist);
        let mut d_hi = dist;
        let (mut g_hi, mut e_hi) = shortfall(d_hi)?;
        let mut doublings = 0;
        // only a measured shortfall moves the low end; any length not short closes the bracket
        // (a NaN length is not finite, and refuses)
        loop {
            if !g_hi.is_finite() {
                return Err(not_met());
            }
            if g_hi >= 0.0 {
                break;
            }
            if doublings >= MAX_DOUBLINGS {
                return Err(not_met());
            }
            (d_lo, g_lo) = (d_hi, g_hi);
            d_hi *= 2.0;
            if !d_hi.is_finite() {
                return Err(not_met());
            }
            (g_hi, e_hi) = shortfall(d_hi)?;
            doublings += 1;
        }
        // regula falsi with the Illinois step, keeping the bracket, to a tenth of the margin
        let stop = 0.1 * margin;
        let (mut best_d, mut best_g, mut best_e) = if g_hi.abs() < g_lo.abs() {
            (d_hi, g_hi, Some(e_hi))
        } else {
            (d_lo, g_lo, None)
        };
        let mut side = 0_i8;
        for _ in 0..MAX_STEPS {
            if best_g.abs() <= stop {
                break;
            }
            let d = (d_lo * g_hi - d_hi * g_lo) / (g_hi - g_lo);
            let d = if d.is_finite() && d > d_lo && d < d_hi {
                d
            } else {
                0.5 * (d_lo + d_hi)
            };
            if !(d > d_lo && d < d_hi) {
                break;
            }
            let (g, e) = shortfall(d)?;
            if !g.is_finite() {
                return Err(not_met());
            }
            if g.abs() < best_g.abs() {
                (best_d, best_g, best_e) = (d, g, Some(e));
            }
            if g < 0.0 {
                (d_lo, g_lo) = (d, g);
                if side == -1 {
                    g_hi *= 0.5;
                }
                side = -1;
            } else {
                (d_hi, g_hi) = (d, g);
                if side == 1 {
                    g_lo *= 0.5;
                }
                side = 1;
            }
        }
        // the curve itself (no growth) is never accepted
        let Some(new) = best_e else {
            return Err(not_met());
        };
        let grew = (new.0 - old.1, new.1 - old.0);
        // the differences' rounding, outward
        let pad = gamma(2) * (new.1 + old.1);
        if grew.0 - pad >= dist - margin && grew.1 + pad <= dist + margin {
            Ok((make(best_d), target + best_g))
        } else {
            Err(not_met())
        }
    }
}

/// A Bézier piece of a curve: its homogeneous control points (`[w x, w y, w z, w]`), the bound
/// on each coefficient's rounding, how many times it was halved, and its enclosure.
///
/// A rational piece carries its rounding `err` from its ancestors and is bounded with it. A
/// polynomial piece is bounded as the exact polynomial of its own float coefficients (`err`
/// 0); the rounding that made it, against the exact polynomial it stands for, is counted
/// once, as `moved`, a bound on the change in length (see [`moved_length`]), which the
/// enclosure adds to the whole. So each halving's rounding is counted in that halving's two
/// pieces only, not again in every piece halved from them.
#[derive(Debug, Clone)]
struct Piece {
    h: Vec<[f64; 4]>,
    err: f64,
    depth: u32,
    polynomial: bool,
    lo: f64,
    hi: f64,
    moved: f64,
}

/// The largest absolute coefficient of `h`.
fn largest(h: &[[f64; 4]]) -> f64 {
    h.iter().flatten().fold(0.0_f64, |m, v| m.max(v.abs()))
}

/// The largest absolute coordinate of `h`, its weights left out (a polynomial piece's, all 1,
/// are averaged exactly).
fn largest_xyz(h: &[[f64; 4]]) -> f64 {
    h.iter()
        .flat_map(|c| [c[0], c[1], c[2]])
        .fold(0.0_f64, |m, v| m.max(v.abs()))
}

/// A bound on how far the arc length of a polynomial Bézier piece of `legs` legs moves when
/// each coordinate of each control point moves by at most `e`: with `C' = n sum_i B^{n-1}_i
/// dP_i` and `int n B^{n-1}_i = 1`, the two lengths differ by at most `int |C' - C~'| <= sum_i
/// |dP_i - dP~_i| <= legs 2 sqrt(3) e`, taken as `4 legs e`, its own rounding outward.
fn moved_length(legs: usize, e: f64) -> f64 {
    #[allow(clippy::cast_precision_loss)] // a piece has a few points
    let legs_f = legs as f64;
    4.0 * legs_f * e * (1.0 + gamma(2))
}

/// A polynomial piece moved so its first point is the origin, and the bound on its coefficients'
/// rounding against the exact piece so moved. A length does not change under a translation;
/// each coefficient is the difference `x - q` rounded once, within `gamma(1)` of its own size
/// more. The weights are set to 1, the exact weight of a curve that is not rational (whatever a
/// knot insertion's rounding left in them). Its later rounding is then relative to the piece's
/// own size, not to where it stands.
fn recentred(mut h: Vec<[f64; 4]>, err: f64) -> (Vec<[f64; 4]>, f64) {
    let Some(&q) = h.first() else {
        return (h, err);
    };
    for c in &mut h {
        *c = [c[0] - q[0], c[1] - q[1], c[2] - q[2], 1.0];
    }
    let err = err + gamma(1) * largest_xyz(&h);
    (h, err)
}

impl Piece {
    /// The piece of coefficients `h`, each within `err` of the exact piece it stands for.
    fn new(h: Vec<[f64; 4]>, err: f64, depth: u32, polynomial: bool) -> Option<Piece> {
        if polynomial {
            let (h, err) = recentred(h, err);
            let moved = moved_length(h.len().saturating_sub(1), err);
            let (lo, hi) = bounds(&h, 0.0, true)?;
            moved.is_finite().then_some(Piece {
                h,
                err: 0.0,
                depth,
                polynomial,
                lo,
                hi,
                moved,
            })
        } else {
            let (lo, hi) = bounds(&h, err, false)?;
            Some(Piece {
                h,
                err,
                depth,
                polynomial,
                lo,
                hi,
                moved: 0.0,
            })
        }
    }

    /// Its halves, by de Casteljau at the middle: each level a convex combination, so each
    /// coefficient within `gamma(2 n + 2)` of the largest more (as the rail certificate of the
    /// tangent rollover counts a subdivision), against the exact halves of this piece's own
    /// coefficients.
    fn halves(&self) -> Option<(Piece, Piece)> {
        let n = self.h.len();
        let mut work = self.h.clone();
        let mut left = Vec::new();
        let mut right = vec![[0.0; 4]; n];
        for level in 0..n {
            left.push(*work.first()?);
            *right.get_mut(n - 1 - level)? = *work.get(n - 1 - level)?;
            for i in 0..n - 1 - level {
                let (a, b) = (*work.get(i)?, *work.get(i + 1)?);
                *work.get_mut(i)? = [
                    0.5 * (a[0] + b[0]),
                    0.5 * (a[1] + b[1]),
                    0.5 * (a[2] + b[2]),
                    0.5 * (a[3] + b[3]),
                ];
            }
        }
        let size = if self.polynomial {
            largest_xyz(&self.h)
        } else {
            largest(&self.h)
        };
        // a polynomial piece's own coefficients are exact for its halves (`err` 0)
        let err = self.err + gamma(2 * n + 2) * size;
        Some((
            Piece::new(left, err, self.depth + 1, self.polynomial)?,
            Piece::new(right, err, self.depth + 1, self.polynomial)?,
        ))
    }
}

/// The certified enclosure `(lo, hi)` of a Bézier piece's arc length, from its homogeneous
/// control points `h`, each coefficient within `err` of exact, `polynomial` for a piece of a
/// curve that is not rational (every weight exactly 1, the NURB's own marker). `None` where
/// the weights are not proved positive or a value is not finite.
///
/// The Euclidean points `P_i` are within `widen` of exact in each coordinate (for a polynomial
/// piece the coefficients themselves, every weight exactly 1, so `widen = err`; for a rational
/// one the quotient of coefficients within `err` by a weight at least `w_min - err`, and its
/// own rounding), so a
/// computed distance between two of them is within `4 widen` of exact (`2 sqrt 3 < 4`), and
/// each distance carries `gamma(5)` relative (a difference, a square, a sum of three, a root),
/// a sum of `n` of them `gamma(n + 4)`, counted per piece from its degree.
/// - Below: the chord `|P_n - P_0|`, which no curve between the two ends is shorter than.
/// - Above: the control polygon's length. De Casteljau's step on the homogeneous points is,
///   for positive weights, a convex combination of the Euclidean points with positive
///   coefficients (`w_i / (w_i + w_{i+1})` and its complement), so it cuts corners of the
///   Euclidean polygon: each subdivision's polygons are no longer than the polygon they come
///   from, and they converge to the curve, so the curve is no longer than any of them. This
///   holds for rational and polynomial pieces alike, and converges at second order.
/// - Above, rational, also: with `W = sum w_i B_i`, `C' = (n / W) sum_i B^{n-1}_i (w_{i+1}
///   dP_i + dw_i (P_i - C))` (`dP_i = P_{i+1} - P_i`, `dw_i = w_{i+1} - w_i`), and `C` in the
///   hull of the `P_i`, so `|P_i - C| <= D`, the diagonal of their box. With `int n B^{n-1}_i
///   = 1`, the length is at most `(w_max / w_min) sum |dP_i| + D sum |dw_i| / w_min`. The
///   smaller of the two bounds is taken.
fn bounds(h: &[[f64; 4]], err: f64, polynomial: bool) -> Option<(f64, f64)> {
    if !err.is_finite() || !h.iter().flatten().all(|v| v.is_finite()) {
        return None;
    }
    let w_min = h.iter().map(|c| c[3]).fold(f64::INFINITY, f64::min);
    let w_max = h.iter().map(|c| c[3]).fold(f64::NEG_INFINITY, f64::max);
    if !polynomial && !crate::geom::certified::above(w_min - err, 0.0) {
        return None;
    }
    let (points, widen): (Vec<Vec3>, f64) = if polynomial {
        // every weight exactly 1: the coefficients are the points, within `err`
        (h.iter().map(|c| Vec3::new(c[0], c[1], c[2])).collect(), err)
    } else {
        let points: Vec<Vec3> = h
            .iter()
            .map(|c| Vec3::new(c[0] / c[3], c[1] / c[3], c[2] / c[3]))
            .collect();
        let size = points
            .iter()
            .flat_map(|p| [p.x.abs(), p.y.abs(), p.z.abs()])
            .fold(0.0_f64, f64::max);
        let widen = err * (1.0 + size) / (w_min - err) + gamma(2) * size;
        (points, widen)
    };
    let slack = 4.0 * widen;
    let (Some(&first), Some(&last)) = (points.first(), points.last()) else {
        return None;
    };
    // one distance: a difference, a square, a sum of three and a root, `gamma(5)`
    let chord = (last - first).length();
    let lo = (chord * (1.0 - gamma(5)) - slack).max(0.0);
    let legs = points.len().saturating_sub(1);
    #[allow(clippy::cast_precision_loss)] // a piece has a few points
    let legs_f = legs as f64;
    // `legs` distances, each `gamma(5)`, summed in order, `gamma(legs - 1)` more
    let polygon: f64 = points
        .windows(2)
        .map(|w| match w {
            [a, b] => (*b - *a).length(),
            _ => 0.0,
        })
        .sum();
    let polygon_hi = (polygon * (1.0 + gamma(legs + 4)) + legs_f * slack) * (1.0 + gamma(2));
    let hi = if polynomial {
        polygon_hi
    } else {
        let (mut bmin, mut bmax) = (
            Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        );
        for p in &points {
            bmin = Vec3::new(bmin.x.min(p.x), bmin.y.min(p.y), bmin.z.min(p.z));
            bmax = Vec3::new(bmax.x.max(p.x), bmax.y.max(p.y), bmax.z.max(p.z));
        }
        let diagonal = ((bmax - bmin).length() * (1.0 + gamma(5)) + slack) * (1.0 + gamma(1));
        let dw: f64 = h
            .windows(2)
            .map(|w| match w {
                [a, b] => (b[3] - a[3]).abs() + 2.0 * err,
                _ => 0.0,
            })
            .sum();
        // `dw`: each term two roundings, summed in order, `gamma(legs + 1)`; then the bound's
        // eight operations (`low_w` one, each product and quotient one, the sum one)
        let low_w = w_min - err;
        let derivative =
            ((w_max + err) / low_w * polygon_hi + diagonal * dw / low_w) * (1.0 + gamma(legs + 9));
        // the polygon bound holds for positive weights too (rational de Casteljau cuts corners
        // of the Euclidean polygon); the derivative bound can be the smaller on a long piece
        polygon_hi.min(derivative)
    };
    // a polynomial piece's coefficients are its exact points (`err` 0 for those): the speed's
    // bound is third order, and the tighter of the two is taken at each end
    let (lo, hi) = match speed_bounds(&points) {
        Some((s_lo, s_hi)) if polynomial && err == 0.0 => (lo.max(s_lo), hi.min(s_hi)),
        _ => (lo, hi),
    };
    (lo.is_finite() && hi.is_finite()).then_some((lo, hi))
}

/// The binomial coefficient `C(n, k)`, exact: each step's product `C(n, i) (n - i)` and quotient
/// `C(n, i + 1)` are integers below `2^53` for every one used here (at most `C(24, 12)`), so
/// neither rounds.
fn binomial(n: usize, k: usize) -> f64 {
    if k > n {
        return 0.0;
    }
    let mut c = 1.0_f64;
    for i in 0..k.min(n - k) {
        #[allow(clippy::cast_precision_loss)] // small counts
        let (top, step) = ((n - i) as f64, (i + 1) as f64);
        c = c * top / step;
    }
    c
}

/// The highest degree [`speed_bounds`] takes: its binomials, to `C(4 (p - 1), 2 (p - 1))`,
/// stay exact.
const SPEED_MAX_DEGREE: usize = 7;

/// A certified enclosure of the arc length of the polynomial Bézier piece with exactly these
/// control points `P_0..P_n` over `[0, 1]`, of third order in the piece's size, from its
/// squared speed `q = |C'|^2`, a polynomial of degree `m = 2 (n - 1)`.
///
/// With `qb` the mean of `q` over `[0, 1]`, Taylor's theorem with the Lagrange remainder gives
/// `sqrt(q) = sqrt(qb) + (q - qb) / (2 sqrt(qb)) - (q - qb)^2 / (8 eta^(3/2))`, `eta` between
/// `qb` and `q`. The middle term integrates to 0, so with `V = int (q - qb)^2` and `q_lo <= eta
/// <= q_hi` on the piece, `sqrt(qb) - V / (8 q_lo^(3/2)) <= L <= sqrt(qb) - V / (8 q_hi^(3/2))`.
/// `q`'s Bernstein coefficients are `q_k = sum_{i+j=k} C(n-1,i) C(n-1,j) / C(m,k) D_i . D_j`
/// (`D_i = n (P_{i+1} - P_i)`), so `qb = sum q_k / (m + 1)`, `q` lies in their hull, and `V =
/// sum_{k,l} r_k r_l C(m,k) C(m,l) / (C(2m,k+l) (2m+1))`, `r_k = q_k - qb`. The gap is
/// `V (q_lo^(-3/2) - q_hi^(-3/2)) / 8`, of the order of the cube of `q`'s relative spread,
/// which halving a piece halves.
///
/// Rounding is counted: each computed `q_k` is within `delta_q` of exact (the differences and
/// the factor `n` two roundings, each dot three, each coefficient and product one, the sum
/// `n` terms: `gamma(n + 13)` of the coefficients' absolute sum, doubled for the rounding of
/// that bound itself), the mean within `delta_bar`, each `r_k` within `delta_r`, `V` within
/// `2 R delta_r + delta_r^2` and its own sum's `gamma((m + 1)^2 + 3) R^2` (`R` the largest
/// `|r_k|`; the weights sum to 1), and the last operations outward.
///
/// `None` (the caller keeps its other bounds) past [`SPEED_MAX_DEGREE`], where `q` is not
/// proved positive on the piece (a cusp or a stop), or a value is not finite.
fn speed_bounds(points: &[Vec3]) -> Option<(f64, f64)> {
    let n = points.len().checked_sub(1)?;
    if n == 0 || n > SPEED_MAX_DEGREE {
        return None;
    }
    let a = n - 1;
    let m = 2 * a;
    #[allow(clippy::cast_precision_loss)] // a degree
    let (nf, mf) = (n as f64, m as f64);
    let d: Vec<Vec3> = points
        .windows(2)
        .map(|w| match w {
            [p, q] => (*q - *p) * nf,
            _ => Vec3::ZERO,
        })
        .collect();
    let mut q = vec![0.0_f64; m + 1];
    let mut q_abs = vec![0.0_f64; m + 1];
    for (i, di) in d.iter().enumerate() {
        for (j, dj) in d.iter().enumerate() {
            let c = binomial(a, i) * binomial(a, j) / binomial(m, i + j);
            let abs = di.x.abs() * dj.x.abs() + di.y.abs() * dj.y.abs() + di.z.abs() * dj.z.abs();
            *q.get_mut(i + j)? += c * di.dot(*dj);
            *q_abs.get_mut(i + j)? += c * abs;
        }
    }
    let delta_q = 2.0 * gamma(n + 13) * q_abs.iter().fold(0.0_f64, |s, v| s.max(*v));
    let q_bar = q.iter().sum::<f64>() / (mf + 1.0);
    let q_big = q.iter().fold(0.0_f64, |s, v| s.max(v.abs()));
    let delta_bar = delta_q + gamma(m + 2) * q_big;
    let r: Vec<f64> = q.iter().map(|v| v - q_bar).collect();
    let r_big = r.iter().fold(0.0_f64, |s, v| s.max(v.abs()));
    let delta_r = delta_q + delta_bar + gamma(1) * r_big;
    let mut v = 0.0_f64;
    let denominator = 2.0 * mf + 1.0;
    for (k, rk) in r.iter().enumerate() {
        for (l, rl) in r.iter().enumerate() {
            let w = binomial(m, k) * binomial(m, l) / (binomial(2 * m, k + l) * denominator);
            v += w * rk * rl;
        }
    }
    let dv =
        (gamma((m + 1) * (m + 1) + 3) * r_big * r_big + 2.0 * r_big * delta_r + delta_r * delta_r)
            * (1.0 + gamma(4));
    let (v_lo, v_hi) = ((v - dv).max(0.0), v + dv);
    let shift = delta_q.max(delta_bar);
    let q_min = q.iter().fold(f64::INFINITY, |s, v| s.min(*v));
    let q_max = q.iter().fold(f64::NEG_INFINITY, |s, v| s.max(*v));
    let q_lo = q_min.min(q_bar) - shift;
    let q_hi = q_max.max(q_bar) + shift;
    let (bar_lo, bar_hi) = (q_bar - delta_bar, q_bar + delta_bar);
    if !(crate::geom::certified::above(q_lo, 0.0) && bar_lo > 0.0) {
        return None;
    }
    let t_lo = q_lo * q_lo.sqrt() * (1.0 - gamma(3));
    let t_hi = q_hi * q_hi.sqrt() * (1.0 + gamma(3));
    let lo = bar_lo.sqrt() * (1.0 - gamma(1)) - v_hi / (8.0 * t_lo) * (1.0 + gamma(3));
    let lo = lo - lo.abs() * gamma(1);
    let hi = bar_hi.sqrt() * (1.0 + gamma(1)) - v_lo / (8.0 * t_hi) * (1.0 - gamma(3));
    let hi = hi + hi.abs() * gamma(1);
    (lo.is_finite() && hi.is_finite()).then_some((lo.max(0.0), hi))
}

/// A certified enclosure `(lo, hi)` of `curve`'s arc length no wider than `width`, from its
/// Bézier pieces, the pieces whose enclosure is at least half the average width halved in
/// rounds;
/// `None` where it is not reached within [`MAX_PIECES`] pieces, [`MAX_DEPTH`] halvings or
/// `MAX_ROUNDS` rounds, or a piece is not bounded (weights not proved positive, a value not
/// finite, a 2D net). The decomposition's knot insertions leave each coefficient within
/// `gamma(16 (p + 1))` of the curve's largest (as the rail certificate of the tangent rollover
/// counts them); the sums of the pieces carry `gamma(count)` relative. A polynomial piece's
/// rounding is counted as a length once per piece made (`Piece::moved`) and added to the whole.
///
/// # Errors
/// Those of the decomposition.
fn length_enclosure(curve: &Curve, width: f64) -> Result<Option<(f64, f64)>> {
    if !(width.is_finite() && width > 0.0) {
        return Ok(None);
    }
    let homogeneous = |c: &CPoint| -> Option<[f64; 4]> {
        if c.z == NOZ {
            return None;
        }
        if c.w == NOW {
            Some([c.x, c.y, c.z, 1.0])
        } else {
            Some([c.x, c.y, c.z, c.w])
        }
    };
    let all: Option<Vec<[f64; 4]>> = curve.cpts().iter().map(homogeneous).collect();
    let Some(all) = all else {
        return Ok(None);
    };
    let polynomial = !curve.is_rational();
    let decomposed = curve.knots().len() > 2 * (curve.degree() + 1);
    let err0 = if decomposed {
        gamma(16 * (curve.degree() + 1)) * largest(&all)
    } else {
        0.0
    };
    let mut pieces = Vec::new();
    // the polynomial pieces' rounding, as length (`Piece::moved`), over every piece made
    let (mut carried, mut made) = (0.0_f64, 0_usize);
    for bezier in curve.decompose_bez()? {
        let h: Option<Vec<[f64; 4]>> = bezier.cpts().iter().map(homogeneous).collect();
        let Some(piece) = h.and_then(|h| Piece::new(h, err0, 0, polynomial)) else {
            return Ok(None);
        };
        carried += piece.moved;
        made += 1;
        pieces.push(piece);
    }
    for _ in 0..MAX_ROUNDS {
        #[allow(clippy::cast_precision_loss)] // the count is at most MAX_PIECES
        let count = pieces.len() as f64;
        let sum = |f: &dyn Fn(&Piece) -> f64| pieces.iter().map(f).sum::<f64>();
        let rounding = gamma(pieces.len());
        let moved = carried * (1.0 + gamma(made));
        // each difference and sum rounded once more, outward
        let lo = sum(&|p| p.lo) * (1.0 - rounding) - moved;
        let lo = lo - lo.abs() * gamma(1);
        let hi = sum(&|p| p.hi) * (1.0 + rounding) + moved;
        let hi = hi + hi.abs() * gamma(1);
        if !(lo.is_finite() && hi.is_finite()) {
            return Ok(None);
        }
        if hi - lo <= width {
            return Ok(Some((lo, hi)));
        }
        let average = (hi - lo) / count;
        let mut next = Vec::new();
        for piece in pieces {
            if piece.hi - piece.lo >= 0.5 * average {
                if piece.depth >= MAX_DEPTH {
                    return Ok(None);
                }
                let Some((a, b)) = piece.halves() else {
                    return Ok(None);
                };
                carried += a.moved + b.moved;
                made += 2;
                next.push(a);
                next.push(b);
            } else {
                next.push(piece);
            }
            if next.len() > MAX_PIECES {
                return Ok(None);
            }
        }
        pieces = next;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nurbs::crv_geom::ToleranceKind;

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

    fn length_of(c: &Curve) -> f64 {
        let (lo, hi) = (c.knots()[0], *c.knots().last().unwrap());
        c.arc_length(lo, hi, 1e-10, ToleranceKind::Relative)
            .unwrap()
    }

    /// The outcome of growing `c` by `dist` at `end`: an extension whose growth in arc length is
    /// the length asked, by a certified enclosure of both curves' lengths within the acceptance
    /// tolerance and by the measured lengths, or the named refusal. Panics on anything else.
    fn check(c: &Curve, dist: f64, end: CurveEnd, what: &str) -> bool {
        match c.extend_keeping_proportion(dist, end) {
            Ok((out, reached)) => {
                let before = length_of(c);
                let target = before + dist;
                // the certified margin, relative to the growth asked
                let tol = ACCEPT_TOL * dist;
                // the measured lengths, each to the quadrature's 1e-10 of itself
                let measured = tol + 1e-10 * (before + target);
                let after = length_of(&out);
                assert!(
                    (after - target).abs() <= measured,
                    "{what}: measured {after}, asked {target} (reported {reached})"
                );
                assert!(
                    (reached - target).abs() <= measured,
                    "{what}: reported {reached}"
                );
                let width = 0.25 * tol;
                let old = length_enclosure(c, width).unwrap().unwrap();
                let new = length_enclosure(&out, width).unwrap().unwrap();
                assert!(
                    new.0 - old.1 >= dist - tol && new.1 - old.0 <= dist + tol,
                    "{what}: certified growth [{}, {}] for {dist}",
                    new.0 - old.1,
                    new.1 - old.0
                );
                assert_eq!(out.knots(), c.knots(), "{what}");
                true
            }
            Err(SolidError::ExtendRefused {
                refusal: ExtendRefusal::LengthNotMet,
                ..
            }) => false,
            Err(e) => panic!("{what}: {e:?}"),
        }
    }

    /// #16635: a hooked cubic (the polygon turns back at its end) grown by 0.5 and by 3. The
    /// stretch slides the points round the hook, so the length first falls as the stretch grows;
    /// it was returned as Ok at 8.77 and 7.53, short of the 10.35 and 12.85 asked (even shorter
    /// than the curve). Now each reaches the length asked, certified.
    #[test]
    fn a_hooked_cubic_grows_by_the_length_asked() {
        let c = curve(
            &[
                [0.0, 0.0, 0.0],
                [10.0, 0.0, 0.0],
                [10.0, 0.1, 0.0],
                [9.0, 0.1, 0.0],
            ],
            3,
        );
        assert!((length_of(&c) - 9.848_846_342_765_029).abs() < 1e-9);
        for dist in [0.5, 3.0] {
            assert!(
                check(&c, dist, CurveEnd::End, &format!("hook {dist}")),
                "hook {dist}: refused"
            );
        }
    }

    /// A deterministic sweep (fixed seed): straight, curved and nearly degenerate polygons of
    /// degree 1 to 3, grown at either end by lengths from a hundredth of the curve to three
    /// times it. Every Ok has the length asked (certified and measured); every other outcome is
    /// the named refusal. On main a quarter of the Ok results were short or long.
    #[test]
    fn every_extension_reaches_the_length_asked_or_is_refused() {
        let mut state: u64 = 0x0005_eed1_6635_u64;
        let mut next = || {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((state >> 11) as f64) / ((1u64 << 53) as f64)
        };
        let mut polygons: Vec<(Vec<[f64; 3]>, usize, &str)> = vec![
            (
                vec![[0.0, 0.0, 0.0], [5.0, 0.0, 0.0], [7.0, 0.0, 0.0]],
                2,
                "straight",
            ),
            (
                vec![
                    [0.0, 0.0, 0.0],
                    [2.0, 3.0, 0.0],
                    [6.0, 3.0, 1.0],
                    [9.0, 0.0, 0.0],
                ],
                3,
                "curved",
            ),
            (
                vec![
                    [0.0, 0.0, 0.0],
                    [1.0, 1e-9, 0.0],
                    [2.0, 0.0, 0.0],
                    [3.0, 1e-9, 0.0],
                ],
                3,
                "nearly straight",
            ),
            (
                vec![
                    [0.0, 0.0, 0.0],
                    [1e-6, 0.0, 0.0],
                    [1e-6, 1e-6, 0.0],
                    [5.0, 0.0, 0.0],
                ],
                3,
                "a tiny first leg",
            ),
        ];
        for k in 0..40 {
            let degree = 1 + k % 3;
            let n = degree + 1 + (k % 4);
            let pts: Vec<[f64; 3]> = (0..n)
                .map(|_| {
                    [
                        20.0 * next() - 10.0,
                        20.0 * next() - 10.0,
                        4.0 * next() - 2.0,
                    ]
                })
                .collect();
            polygons.push((pts, degree, "random"));
        }
        let (mut built, mut refused) = (0, 0);
        for (i, (pts, degree, kind)) in polygons.iter().enumerate() {
            let c = curve(pts, *degree);
            let l = length_of(&c);
            for share in [0.01, 0.3, 3.0] {
                for end in [CurveEnd::Start, CurveEnd::End] {
                    let what = format!("{kind} {i} share {share} {end:?}");
                    if check(&c, share * l, end, &what) {
                        built += 1;
                    } else {
                        eprintln!("refused: {what}");
                        refused += 1;
                    }
                }
            }
        }
        eprintln!("sweep: built {built}, refused {refused}");
        // every case is decided; most build
        assert!(built > 10 * refused, "built {built}, refused {refused}");
    }

    /// A rational quarter circle grown by 1, at either end: certified and the length asked (the
    /// control polygon bounds a rational piece too, so its enclosure converges at second order).
    #[test]
    fn a_rational_arc_reaches_the_length_asked() {
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let pw = vec![
            CPoint::weighted(Vec3::new(1.0, 0.0, 0.0), 1.0),
            CPoint::weighted(Vec3::new(1.0, 1.0, 0.0), h),
            CPoint::weighted(Vec3::new(0.0, 1.0, 0.0), 1.0),
        ];
        let arc = Curve::new(pw, 2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap();
        for end in [CurveEnd::Start, CurveEnd::End] {
            assert!(check(&arc, 1.0, end, "arc"), "arc {end:?}: refused");
        }
    }

    /// A growth of 1e-6 on a curve about 1100 long (the curved cubic scaled by 100). The margin
    /// is `1e-7` of the growth, `1e-13`, below the rounding of a length near 1100 (`gamma(2)`
    /// of it, about `5e-13`), so no extension can be certified and it is refused. A margin
    /// relative to the whole length (`1e-4` here) would accept the curve unchanged.
    #[test]
    fn a_tiny_growth_on_a_long_curve_is_refused_not_skipped() {
        let c = curve(
            &[
                [0.0, 0.0, 0.0],
                [200.0, 300.0, 0.0],
                [600.0, 300.0, 100.0],
                [900.0, 0.0, 0.0],
            ],
            3,
        );
        assert!(length_of(&c) > 1000.0);
        for end in [CurveEnd::Start, CurveEnd::End] {
            assert!(!check(&c, 1e-6, end, "tiny"), "tiny {end:?}: built");
        }
        // a growth the lengths do resolve is built
        assert!(check(&c, 100.0, CurveEnd::End, "long"), "long: refused");
    }

    /// The enclosure is the closed form: a straight cubic of length 6 (CVs at x 0, 1, 3, 6),
    /// a quarter circle of radius 1 and a parabola's arc lie inside their enclosures.
    #[test]
    fn the_length_enclosure_holds_closed_forms() {
        let c = curve(
            &[
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [3.0, 0.0, 0.0],
                [6.0, 0.0, 0.0],
            ],
            3,
        );
        let (lo, hi) = length_enclosure(&c, 1e-9).unwrap().unwrap();
        assert!(lo <= 6.0 && 6.0 <= hi && hi - lo <= 1e-9, "{lo} {hi}");
        let h = std::f64::consts::FRAC_1_SQRT_2;
        let pw = vec![
            CPoint::weighted(Vec3::new(1.0, 0.0, 0.0), 1.0),
            CPoint::weighted(Vec3::new(1.0, 1.0, 0.0), h),
            CPoint::weighted(Vec3::new(0.0, 1.0, 0.0), 1.0),
        ];
        let arc = Curve::new(pw, 2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap();
        let quarter = std::f64::consts::FRAC_PI_2;
        let (lo, hi) = length_enclosure(&arc, 1e-8).unwrap().unwrap();
        assert!(
            lo <= quarter && quarter <= hi && hi - lo <= 1e-8,
            "{lo} {hi}"
        );
        // the parabola y = x^2 over [0, 1], its speed varying: (2 sqrt 5 + asinh 2) / 4
        let parabola = curve(&[[0.0, 0.0, 0.0], [0.5, 0.0, 0.0], [1.0, 1.0, 0.0]], 2);
        let exact = (2.0 * 5.0_f64.sqrt() + 2.0_f64.asinh()) / 4.0;
        let (lo, hi) = length_enclosure(&parabola, 1e-10).unwrap().unwrap();
        assert!(
            lo <= exact && exact <= hi && hi - lo <= 1e-10,
            "{lo} {exact} {hi}"
        );
    }
}
