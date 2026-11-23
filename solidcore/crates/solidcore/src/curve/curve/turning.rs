//! Not upstream (#17685): the parameters where one coordinate of a polynomial or rational
//! curve may stop changing (`C'(t) . e = 0` for an axis `e`). Each is isolated by Bezier
//! convex-hull subdivision of the derivative's numerator, for the tight box.
//!
//! Upstream: none. `CalculateTightBoundingBox` (`source/SMLib/src/SmCurve.cpp`) finds
//! these points with the perpendicular property analyses only. Their objective refuses a
//! parameter where `|C'|^2 < SM_EFF_ZERO`, so a coordinate's extreme where the curve
//! (nearly) stops is missed. See [`Curve::add_turning_points`].
//!
//! | Rust                          | Upstream |
//! |-------------------------------|----------|
//! | [`Curve::add_turning_points`] | none     |
//! | `isolate` (private)           | none     |
//! | `bspline_pieces` (private)    | none     |
//! | `bezier_points` (private)     | none     |
//! | `hermite_pieces` (private)    | none     |
//!
//! Deviations: not a port. The subdivision is a loop over an explicit stack. It is
//! bounded by [`MAX_DEPTH`] levels per piece and [`MAX_NODES`] subdivisions per curve
//! and axis; past either it refuses with `InvalidInput`. Cancellation is polled every
//! [`CANCEL_POLL`] subdivisions. The data's rounding (Bezier extraction, the numerator,
//! the halving) and the acceptance tests' own arithmetic are counted with [`gamma`],
//! ignoring underflow. Not counted: the rounding of the map from a sub-interval's `s` to
//! the curve's `t`, which moves a kept range's ends by an ulp or two of `t`.

use crate::curve::curve::p2::{Curve, CurveKind};
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::vector3d::Point3;
use crate::kernel::Kernel;
use crate::nurbs::crv_basic::Curve as NlCurve;
use crate::nurbs::knots::Side;
use crate::nurbs::math::{gamma, CPoint, NOW, NOZ};

const OP: &str = "curve::curve::turning::add_turning_points";

/// The deepest a piece is halved. Each level at least halves the bound on the
/// coordinate's change over a sub-interval, so a piece needs about
/// `52 + log2(2 q (wmax / wmin)^2)` levels to bring that bound under the coordinate's
/// rounding (see `isolate`), `q` the numerator's degree. That is under 200 for any
/// degree a curve evaluation holds (below `nurbs_crv::MAX_DEGREE`, 32, so `q <= 61`;
/// the NURB data limit `knots::MAX_DEGREE` is 17) and a weight ratio up to `2^70`.
pub(crate) const MAX_DEPTH: usize = 200;

/// Subdivisions between two polls of the kernel's cancel token.
const CANCEL_POLL: usize = 1024;

/// The most subdivisions for one curve and axis. Outside the flat stretches below, a
/// degree `q` numerator has at most `q` roots in a piece and each root costs at most
/// [`MAX_DEPTH`] subdivisions. A stretch where the numerator is zero to within its
/// rounding (the coordinate is constant or nearly so) is accepted by its hull instead
/// (see `isolate`), after about `log2` of its weight ratio levels.
pub(crate) const MAX_NODES: usize = 1 << 20;

/// Values in Bernstein form on `s` in `[0, 1]`, each with a bound on its absolute error.
#[derive(Clone)]
struct Bern {
    v: Vec<f64>,
    e: Vec<f64>,
}

impl Bern {
    /// One de Casteljau step at `s = 1/2`: the values on `[0, 1/2]` and `[1/2, 1]`.
    ///
    /// Each step computes `v = 0.5 (a + b)`, one rounding (the halving is exact), so
    /// `|v - exact| <= 0.5 (e_a + e_b) + gamma_1 |v|`. The bound is itself three operations,
    /// covered by the factor `1 + gamma_3`.
    fn halve(&self) -> (Self, Self) {
        let mut v = self.v.clone();
        let mut e = self.e.clone();
        let q = v.len().min(e.len());
        let mut left = Self {
            v: Vec::new(),
            e: Vec::new(),
        };
        let mut right = left.clone();
        for r in 0..q {
            if let (Some(&a), Some(&ea)) = (v.first(), e.first()) {
                left.v.push(a);
                left.e.push(ea);
            }
            let live = q - r;
            if let (Some(&b), Some(&eb)) = (v.get(live - 1), e.get(live - 1)) {
                right.v.push(b);
                right.e.push(eb);
            }
            for i in 0..live.saturating_sub(1) {
                if let (Some(&a), Some(&b), Some(&ea), Some(&eb)) =
                    (v.get(i), v.get(i + 1), e.get(i), e.get(i + 1))
                {
                    let m = 0.5 * (a + b);
                    let em = (0.5 * (ea + eb) + gamma(1) * m.abs()) * (1.0 + gamma(3));
                    if let (Some(sv), Some(se)) = (v.get_mut(i), e.get_mut(i)) {
                        *sv = m;
                        *se = em;
                    }
                }
            }
        }
        right.v.reverse();
        right.e.reverse();
        (left, right)
    }

    /// Whether every value is finite and every bound finite and not negative.
    fn is_finite(&self) -> bool {
        self.v.len() == self.e.len()
            && self.v.iter().all(|v| v.is_finite())
            && self.e.iter().all(|e| e.is_finite() && *e >= 0.0)
    }
}

/// The numerator `N` of one coordinate's derivative over a parameter range, in Bernstein
/// form on `s` in `[0, 1]`. `t = t0 + s (t1 - t0)`. The coordinate's derivative in `s`
/// is `N / W^2` with `W >= w_min`; `scale` bounds the coordinate's magnitude over the
/// range. `x` and `w` are the range's homogeneous control points (the coordinate times
/// the weight, and the weight; weight 1 for a polynomial), whose quotients bound the
/// coordinate (the convex hull property, for positive weights).
struct Piece {
    t0: f64,
    t1: f64,
    num: Bern,
    w_min: f64,
    scale: f64,
    x: Bern,
    w: Bern,
}

/// One sub-interval `[s0, s1]` of a piece on the search's stack.
struct Node {
    s0: f64,
    s1: f64,
    num: Bern,
    x: Bern,
    w: Bern,
    depth: usize,
}

/// Whether the coordinate over a sub-interval is certified to stay within its rounding:
/// with `y_i = x_i / w_i`, `spread = max y - min y` and `p` the largest bound on
/// `|y_i - exact y_i|`, whether `spread <= 2 p + EPSILON * scale`.
///
/// Then every exact `y_i` is in `[min y - p, max y + p]`, so is the coordinate over the
/// sub-interval (its convex hull, for positive weights), and the coordinate at either end
/// is within `spread + 2 p <= 4 p + EPSILON * scale` of its extreme there.
///
/// `p`: with `|x - X| <= e_x`, `|w - W| <= e_w` and `w > e_w`,
/// `|x / w - X / W| <= (e_x + |x / w| e_w) / (w - e_w)`, plus `gamma_1 |y|` for the
/// division. The factor `1 + gamma_8` covers `|x / w|` against `y` and the bound's own
/// six operations. The factor `1 + gamma_3` on the spread covers its own subtraction,
/// that factor's product and the sum on the right (`2 p` and `EPSILON * scale` are
/// exact). `false` when a weight is not above its error bound, or a value is NaN.
///
/// `p` is local: `x` and `w` carry the bounds of their own segment's extraction
/// ([`bezier_points`]) and halvings, so it is a few tens of ulps of the segment's own
/// `max |x| (w_max / w_min)`, whatever the weights elsewhere on the curve.
fn hull_is_flat(x: &Bern, w: &Bern, scale: f64) -> bool {
    let (mut lo, mut hi, mut perr) = (f64::INFINITY, f64::NEG_INFINITY, 0.0_f64);
    for (((&xi, &exi), &wi), &ewi) in x.v.iter().zip(&x.e).zip(&w.v).zip(&w.e) {
        if wi.is_nan() || ewi.is_nan() || wi <= ewi {
            return false;
        }
        let y = xi / wi;
        let p = ((exi + y.abs() * ewi) / (wi - ewi) + gamma(1) * y.abs()) * (1.0 + gamma(8));
        if !(y.is_finite() && p.is_finite()) {
            return false;
        }
        lo = lo.min(y);
        hi = hi.max(y);
        perr = perr.max(p);
    }
    let spread = (hi - lo) * (1.0 + gamma(3));
    spread.is_finite() && spread <= 2.0 * perr + f64::EPSILON * scale
}

/// The parameter ranges, each clamped to `[lo, hi]` and in increasing order, that may
/// hold a root of `piece`'s numerator.
///
/// A sub-interval is dropped when every coefficient exceeds its error bound in the same
/// sign: the numerator then has that sign over the whole sub-interval (the convex hull
/// property of the Bernstein form). It is kept as a range once
///
/// - the coordinate's change over it is at most
///   `width * max (|c| + e) / w_min^2 <= EPSILON * scale`, under its rounding, or
/// - every coefficient is within its error bound (the numerator is zero to within its
///   rounding, as where the coordinate is constant), and its hull is flat
///   ([`hull_is_flat`]): the coordinate's ends are then within `4 p + EPSILON * scale`
///   of its extreme over it, `p` the rounding of its control points, or
/// - no double lies strictly between its ends `ta` and `tb` (their computed midpoint
///   rounds to one of them), so the ends are every parameter in it that can be evaluated.
///   A width test against ulps of `t` alone would accept a range with a double inside,
///   whose interior extreme the ends miss (knots near `2^52`, spans a few ulps wide).
///
/// Without the second rule a stretch where the numerator is zero to within rounding is
/// never dropped and the first takes a width under `1 / (C p^2 (w_max / w_min)^2)`, so it
/// would be tiled into that many ranges.
///
/// The first rule's `change` is four roundings (`s1 - s0` is exact, both being dyadic),
/// covered by the factor `1 + gamma_6`; `EPSILON * scale` is exact. It divides by `w_min`
/// twice rather than by `w_min^2`, which overflows for weights past about `1e154` and
/// would make `change` zero.
///
/// Every node's coefficients, points and bounds are checked finite (a halving can
/// overflow past about `9e307`), so the `max` folds never see a NaN; a node that is not
/// finite refuses with `InvalidInput`. The kernel's cancel token is polled every
/// [`CANCEL_POLL`] subdivisions.
fn isolate(
    k: &Kernel,
    piece: &Piece,
    lo: f64,
    hi: f64,
    nodes: &mut usize,
    out: &mut Vec<(f64, f64)>,
) -> Result<()> {
    if !(piece.scale.is_finite() && piece.w_min > 0.0 && piece.w_min.is_finite()) {
        return Err(SolidError::invalid_input(
            OP,
            "a coordinate's derivative has a non-finite coefficient or weight bound",
        ));
    }
    let span = piece.t1 - piece.t0;
    let at = |s: f64| {
        if s >= 1.0 {
            piece.t1
        } else {
            piece.t0 + s * span
        }
    };
    let mut stack = vec![Node {
        s0: 0.0,
        s1: 1.0,
        num: piece.num.clone(),
        x: piece.x.clone(),
        w: piece.w.clone(),
        depth: 0,
    }];
    while let Some(node) = stack.pop() {
        let (ta, tb) = (at(node.s0), at(node.s1));
        if tb < lo || ta > hi {
            continue;
        }
        let c = &node.num;
        if !(c.is_finite() && node.x.is_finite() && node.w.is_finite()) {
            return Err(SolidError::invalid_input(
                OP,
                "a coordinate's derivative has a non-finite coefficient or weight bound",
            ));
        }
        let above = c.v.iter().zip(&c.e).all(|(&v, &e)| v > e);
        let below = c.v.iter().zip(&c.e).all(|(&v, &e)| v < -e);
        if above || below {
            continue;
        }
        let big = c.v.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
        let err = c.e.iter().fold(0.0_f64, |m, e| m.max(*e));
        let change =
            (node.s1 - node.s0) * ((big + err) / piece.w_min) / piece.w_min * (1.0 + gamma(6));
        let mid_t = 0.5 * ta + 0.5 * tb;
        let zero = c.v.iter().zip(&c.e).all(|(&v, &e)| v.abs() <= e);
        if change <= f64::EPSILON * piece.scale
            || (zero && hull_is_flat(&node.x, &node.w, piece.scale))
            || mid_t <= ta
            || mid_t >= tb
        {
            out.push((ta.max(lo).min(hi), tb.max(lo).min(hi)));
            continue;
        }
        if node.depth >= MAX_DEPTH {
            return Err(SolidError::invalid_input(
                OP,
                "a coordinate's turning point was not isolated within the subdivision depth",
            ));
        }
        *nodes += 1;
        if *nodes % CANCEL_POLL == 0 {
            k.check_cancelled(OP)?;
        }
        if *nodes > MAX_NODES {
            return Err(SolidError::invalid_input(
                OP,
                "a coordinate's turning points need more subdivisions than allowed",
            ));
        }
        let (nl, nr) = node.num.halve();
        let (xl, xr) = node.x.halve();
        let (wl, wr) = node.w.halve();
        let mid = 0.5 * (node.s0 + node.s1);
        let depth = node.depth + 1;
        // Right first, so the left half is taken next: ranges come out in order.
        stack.push(Node {
            s0: mid,
            s1: node.s1,
            num: nr,
            x: xr,
            w: wr,
            depth,
        });
        stack.push(Node {
            s0: node.s0,
            s1: mid,
            num: nl,
            x: xl,
            w: wl,
            depth,
        });
    }
    Ok(())
}

/// `n choose k`, exact in `u128` for `n <= 64` and rounded to `f64` (exact to `2^53`).
fn binom(n: usize, k: usize) -> f64 {
    if k > n {
        return 0.0;
    }
    let k = k.min(n - k);
    let mut c: u128 = 1;
    for i in 0..k {
        // c * (n - i) / (i + 1) stays an integer and below 2^127 for n <= 64.
        c = c.saturating_mul((n - i) as u128) / (i as u128 + 1);
    }
    c as f64
}

/// The coordinate `axis` (0, 1, 2) and weight of a control point, `None` for a 2D point's
/// z. A non-rational point has weight 1 and holds its coordinate unweighted.
fn homogeneous(c: CPoint, axis: usize) -> Option<(f64, f64)> {
    let x = match axis {
        0 => c.x,
        1 => c.y,
        _ => {
            if c.z == NOZ {
                return None;
            }
            c.z
        }
    };
    Some(if c.w == NOW { (x, 1.0) } else { (x, c.w) })
}

/// The Bezier control values, each with a bound on its absolute error, of one
/// component of the B-spline over its non-empty span `[a, b] = [U_j, U_{j+1}]`.
/// `vals` are the component's `p + 1` stored values `V_{j-p} ..= V_j` (exact: the
/// stored curve is the reference), `u` the `2 p` knots `U_{j-p+1} ..= U_{j+p}`.
///
/// Bezier point `i` is the blossom `b(a^{p-i}, b^i)`, by de Boor's triangle: at level
/// `r` (argument `t_r`), `d_l = (1 - al) d_{l-1} + al d_l` for `l = p` down to `r`, with
/// `al = (t_r - U_{j-p+l}) / (U_{j+1+l-r} - U_{j-p+l})`. Since
/// `U_{j-p+l} <= a <= t_r <= b <= U_{j+1+l-r}`, `al` is in `[0, 1]` (and so is its
/// rounding, by monotonicity) and the denominator is at least `b - a > 0`: every step
/// is a convex combination of values of this span's own support.
///
/// Rounding of one step, with operands `P`, `Q` off by at most `e_P`, `e_Q`: the
/// computed `al` is within `gamma_3 al` (two subtractions and a division of exact
/// knots), `1 - al` within `gamma_4`, and the two products and the sum add `gamma_2
/// (|P| + |Q|)`, so the result is within `max(e_P, e_Q) + gamma_7 (|P| + |Q|)`, times
/// `1 + gamma_5` for the bound's own arithmetic. A step whose `al` is exactly 0 or 1
/// (`t_r` equal to a knot) copies its operand, with no rounding: a span whose knots
/// already have full multiplicity is extracted exactly.
///
/// So after the `p` levels each bound is at most about `14 p u A`, `A` the largest
/// `|V|` of the span's support: local to this span, whatever the curve's values and
/// weights elsewhere. `None` when `vals` or `u` is short.
///
/// (#17987) `[a, b]` may also lie past the span, for the span's polynomial extended past
/// the curve's knots (see [`bspline_pieces`]). A step whose `al` is outside `[0, 1]` is
/// then an affine combination, not a convex one: with `s = |1 - al| + |al|` its result is
/// within `s max(e_P, e_Q) + gamma_7 s (|P| + |Q|)`. `s` is taken from the computed `al`,
/// within `gamma_8` of the exact one (the `gamma_3` of `al` and `gamma_4` of `1 - al`), and
/// `1 + gamma_16` covers that and the bound's own arithmetic. The denominator is a knot
/// difference across the span, at least its width, whatever `a` and `b` are. Steps inside
/// `[0, 1]` keep the bound above, so a span's own extraction is unchanged.
fn bezier_points(vals: &[f64], u: &[f64], a: f64, b: f64) -> Option<Vec<(f64, f64)>> {
    let p = vals.len().checked_sub(1)?;
    if u.len() != 2 * p {
        return None;
    }
    let mut out = Vec::new();
    for i in 0..=p {
        let mut d: Vec<(f64, f64)> = vals.iter().map(|&v| (v, 0.0)).collect();
        for r in 1..=p {
            let t = if r <= p - i { a } else { b };
            for l in (r..=p).rev() {
                let (&(pv, pe), &(qv, qe)) = (d.get(l - 1)?, d.get(l)?);
                let (&lo, &hi) = (u.get(l - 1)?, u.get(l + p - r)?);
                let next = if t == lo {
                    (pv, pe)
                } else if t == hi {
                    (qv, qe)
                } else {
                    let al = (t - lo) / (hi - lo);
                    let v = (1.0 - al) * pv + al * qv;
                    let e = if (0.0..=1.0).contains(&al) {
                        (pe.max(qe) + gamma(7) * (pv.abs() + qv.abs())) * (1.0 + gamma(5))
                    } else {
                        let s = (1.0 - al).abs() + al.abs();
                        (s * pe.max(qe) + gamma(7) * s * (pv.abs() + qv.abs())) * (1.0 + gamma(16))
                    };
                    (v, e)
                };
                *d.get_mut(l)? = next;
            }
        }
        out.push(*d.get(p)?);
    }
    Some(out)
}

/// The derivative numerators of coordinate `axis` of the B-spline `nl`, one piece per
/// non-empty knot span of its domain `[U_p, U_{n+1}]` (clamped or not). `None` when the
/// coordinate does not exist (2D), is the same at every control point (the coordinate
/// is then constant, so its ends hold it), or the degree is 0 (`C' = 0` on every span).
///
/// Each span's Bezier points (homogeneous coordinate `X_i` and weight `w_i`) come from
/// its own `p + 1` stored points by [`bezier_points`], each with its own error bound
/// `e_X,i`, `e_w,i` (a non-rational curve's weights are exactly 1).
///
/// A segment with homogeneous points `X_i` and weights `w_i` (degree `p`) has
/// `x = X / W` and `x' = N / W^2` with
/// `N = X' W - X W' = sum_{i,j} (X_i w_j - X_j w_i) B'_i B_j`.
/// With `B'_i = p (B^{p-1}_{i-1} - B^{p-1}_i)` and
/// `B^{p-1}_k B^p_j = C(p-1,k) C(p,j) / C(2p-1,k+j) B^{2p-1}_{k+j}`, its degree `2p - 1`
/// Bernstein coefficients are
/// `c_m = p sum_{k+j=m} C(p-1,k) C(p,j) / C(2p-1,m) (D_{k+1,j} - D_{k,j})`, where
/// `D_{i,j} = X_i w_j - X_j w_i`. For a non-rational segment, `W = 1` and the hodograph
/// `c_k = p (x_{k+1} - x_k)` (degree `p - 1`) is used.
///
/// Rounding: `c_m` is `p + 11` roundings of terms bounded by its magnitude sum (`D`: two
/// products and a difference; the difference of two `D`; the binomial ratio, three
/// roundings and two operations; the product; the sum of at most `p` terms; the factor
/// `p`), plus the effect of the points' error on each `D`,
/// `e_X,i (w_j + e_w,j) + e_w,j |X_i| + e_X,j (w_i + e_w,i) + e_w,i |X_j|`. A
/// non-rational `c_k` has two roundings and `p (e_k + e_{k+1})` from the points. The
/// weight bound `w_min` is `min (w_i - e_w,i)`, two roundings, times `1 - gamma_3`.
///
/// (#17987) `past`, for a curve that evaluates past its knots: where `[lo, hi]` runs
/// below the domain's start or above its end, one more piece covers that part, the
/// polynomial of the span the evaluation takes there extended (`KnotVector::find_span`:
/// the first or last span at least `1e-12` wide), extracted by [`bezier_points`] over
/// the part. A rational extension's Bezier weights may then be zero or negative even
/// where its weight polynomial is positive: while both end weights (the weight at the
/// part's ends) are certified positive, such a part is halved until each half's weights
/// are (`w_min > 0`), at most [`MAX_DEPTH`] levels and [`MAX_NODES`] halvings. A part
/// whose weight is not positive at an end (past a pole, or at one) is not halved; it, or
/// a half that never gets there (a zero of the weight inside, a pole), keeps
/// `w_min <= 0`, and `isolate` refuses it.
///
/// # Errors
///
/// `InvalidInput` for a non-finite or non-positive weight, a non-finite point or knot,
/// decreasing knots, too few knots for the control points, or a degree at or above
/// `nurbs_crv::MAX_DEGREE` (as Bezier extraction refuses), or a rational extension that
/// needs more than [`MAX_NODES`] halvings; those of `find_span`.
fn bspline_pieces(
    nl: &NlCurve,
    axis: usize,
    past: Option<(f64, f64)>,
) -> Result<Option<Vec<Piece>>> {
    let pts = nl.cpts();
    let mut first = None;
    let mut varies = false;
    let mut hs_all = Vec::new();
    for c in pts {
        let Some(h) = homogeneous(*c, axis) else {
            return Ok(None);
        };
        if !(h.0.is_finite() && h.1.is_finite() && h.1 > 0.0) {
            return Err(SolidError::invalid_input(
                OP,
                "a control point or weight is not finite, or a weight is not positive",
            ));
        }
        let x = h.0 / h.1;
        match first {
            None => first = Some(x),
            Some(f) => varies |= x != f,
        }
        hs_all.push(h);
    }
    let p = nl.degree();
    if !varies || p == 0 {
        return Ok(None);
    }
    if p >= crate::nurbs::nurbs_crv::MAX_DEGREE {
        return Err(SolidError::invalid_input(
            OP,
            "degree at or above the largest degree a curve evaluation holds",
        ));
    }
    let knots = nl.knots();
    if !knots.iter().all(|u| u.is_finite()) {
        return Err(SolidError::invalid_input(OP, "a knot is not finite"));
    }
    let rational = pts.iter().any(|c| c.w != NOW);
    let xs_all: Vec<f64> = hs_all.iter().map(|h| h.0).collect();
    let ws_all: Vec<f64> = hs_all.iter().map(|h| h.1).collect();
    let pf = p as f64;
    let short =
        || SolidError::invalid_input(OP, "a B-spline's knots do not cover its control points");
    // (span j, t0, t1, halvings left): the non-empty spans, then the parts of `past` beyond
    // the domain, each on the span the curve's evaluation extends there.
    let mut spans = Vec::new();
    for j in p..hs_all.len() {
        let (&t0, &t1) = (
            knots.get(j).ok_or_else(short)?,
            knots.get(j + 1).ok_or_else(short)?,
        );
        if t1 < t0 {
            return Err(SolidError::invalid_input(OP, "a B-spline's knots decrease"));
        }
        if t1 > t0 {
            spans.push((j, t0, t1, 0));
        }
    }
    if let Some((lo, hi)) = past {
        let (&d0, &d1) = (
            knots.get(p).ok_or_else(short)?,
            knots.get(hs_all.len()).ok_or_else(short)?,
        );
        let kv = nl.knot_vector()?;
        if lo < d0 {
            spans.push((kv.find_span(p, lo, Side::Left)?, lo, d0, MAX_DEPTH));
        }
        if hi > d1 {
            spans.push((kv.find_span(p, hi, Side::Left)?, d1, hi, MAX_DEPTH));
        }
    }
    // Taken from the end: reversed, so the pieces keep the spans' order.
    spans.reverse();
    let mut pieces = Vec::new();
    let mut splits = 0;
    while let Some((j, t0, t1, halvings)) = spans.pop() {
        let u = knots.get(j + 1 - p..=j + p).ok_or_else(short)?;
        let support = j - p..=j;
        let bez = |vals: &[f64]| {
            vals.get(support.clone())
                .and_then(|v| bezier_points(v, u, t0, t1))
                .ok_or_else(short)
        };
        let xs = bez(&xs_all)?;
        let ws = if rational {
            bez(&ws_all)?
        } else {
            vec![(1.0, 0.0); p + 1]
        };
        // (X_i, w_i, e_X,i, e_w,i)
        let hs: Vec<(f64, f64, f64, f64)> = xs
            .iter()
            .zip(&ws)
            .map(|(&(x, ex), &(w, ew))| (x, w, ex, ew))
            .collect();
        let scale = hs
            .iter()
            .fold(0.0_f64, |m, &(x, w, _, _)| m.max((x / w).abs()));
        let w_min = hs.iter().fold(f64::INFINITY, |m, &(_, w, _, ew)| {
            m.min((w - ew) * (1.0 - gamma(3)))
        });
        // The end weights are the weight at `t0` and `t1`: where one is not certified
        // positive, halving cannot help.
        let end_positive =
            |h: Option<&(f64, f64, f64, f64)>| h.is_some_and(|&(_, w, _, ew)| w - ew > 0.0);
        let mid = 0.5 * t0 + 0.5 * t1;
        if rational
            && halvings > 0
            && w_min <= 0.0
            && end_positive(hs.first())
            && end_positive(hs.last())
            && t0 < mid
            && mid < t1
        {
            splits += 1;
            if splits > MAX_NODES {
                return Err(SolidError::invalid_input(
                    OP,
                    "a rational extension's weights need more halvings than allowed",
                ));
            }
            spans.push((j, mid, t1, halvings - 1));
            spans.push((j, t0, mid, halvings - 1));
            continue;
        }
        let num = if rational {
            let d = |i: usize, j: usize| -> (f64, f64, f64) {
                match (hs.get(i), hs.get(j)) {
                    (Some(&(xi, wi, exi, ewi)), Some(&(xj, wj, exj, ewj))) => (
                        xi * wj - xj * wi,
                        (xi * wj).abs() + (xj * wi).abs(),
                        exi * (wj + ewj) + ewj * xi.abs() + exj * (wi + ewi) + ewi * xj.abs(),
                    ),
                    _ => (0.0, 0.0, 0.0),
                }
            };
            let q = 2 * p - 1;
            let g = gamma(p + 11);
            let mut num = Bern {
                v: Vec::new(),
                e: Vec::new(),
            };
            for m in 0..=q {
                let (mut sum, mut mag, mut ext) = (0.0_f64, 0.0_f64, 0.0_f64);
                for k in 0..p {
                    let Some(j) = m.checked_sub(k) else { break };
                    if j > p {
                        continue;
                    }
                    let f = binom(p - 1, k) * binom(p, j) / binom(q, m);
                    let (a, ma, ea) = d(k + 1, j);
                    let (b, mb, eb) = d(k, j);
                    sum += f * (a - b);
                    mag += f * (ma + mb);
                    ext += f * (ea + eb);
                }
                num.v.push(pf * sum);
                num.e.push(g * pf * mag + (1.0 + g) * pf * ext);
            }
            num
        } else {
            let mut num = Bern {
                v: Vec::new(),
                e: Vec::new(),
            };
            for (a, b) in hs.iter().skip(1).zip(&hs) {
                let c = pf * (a.0 - b.0);
                num.v.push(c);
                num.e
                    .push(gamma(2) * c.abs() + (1.0 + gamma(2)) * pf * (a.2 + b.2));
            }
            num
        };
        pieces.push(Piece {
            t0,
            t1,
            num,
            w_min: if rational { w_min } else { 1.0 },
            scale,
            x: Bern {
                v: hs.iter().map(|h| h.0).collect(),
                e: hs.iter().map(|h| h.2).collect(),
            },
            w: Bern {
                v: hs.iter().map(|h| h.1).collect(),
                e: hs.iter().map(|h| h.3).collect(),
            },
        });
    }
    Ok(Some(pieces))
}

/// The derivative of coordinate `axis` of a cubic Hermite curve
/// `x(u) = a + b u + c u^2 + d u^3` over `[lo, hi]`, as one piece: with `u = lo + s h`,
/// `dx/ds = h (b + 2 c u + 3 d u^2)`, whose degree 2 Bernstein coefficients follow from
/// its power coefficients `e0 + e1 s + e2 s^2` as `e0`, `e0 + e1 / 2` and `e0 + e1 + e2`.
///
/// Rounding: each coefficient is a chain of at most 10 operations (`h` and `e0` 8,
/// the sums 2 more) of terms bounded by `big`; `gamma_12` also covers the bound's own
/// rounding. The cubic's control points follow from the hodograph,
/// `P_0 = x(lo)` (Horner, six operations of terms bounded by `scale`) and
/// `P_{k+1} = P_k + c_k / 3` (two roundings each).
fn hermite_pieces(coef: [f64; 4], lo: f64, hi: f64) -> Option<Vec<Piece>> {
    let [a, b, c, d] = coef;
    if b == 0.0 && c == 0.0 && d == 0.0 {
        return None;
    }
    let h = hi - lo;
    let e0 = h * (b + lo * (2.0 * c + 3.0 * d * lo));
    let e1 = h * h * (2.0 * c + 6.0 * d * lo);
    let e2 = h * h * h * (3.0 * d);
    let coefs = vec![e0, e0 + 0.5 * e1, e0 + e1 + e2];
    let u = lo.abs().max(hi.abs());
    let mag = |hh: f64| hh * (b.abs() + u * (2.0 * c.abs() + 3.0 * d.abs() * u));
    let big = mag(h.abs())
        + h.abs() * h.abs() * (2.0 * c.abs() + 6.0 * d.abs() * u)
        + h.abs().powi(3) * 3.0 * d.abs();
    let scale = a.abs() + u * (b.abs() + u * (c.abs() + u * d.abs()));
    let err = gamma(12) * big;
    let mut x = Bern {
        v: vec![a + lo * (b + lo * (c + lo * d))],
        e: vec![gamma(8) * scale],
    };
    for &ck in &coefs {
        let (Some(&pk), Some(&ek)) = (x.v.last(), x.e.last()) else {
            return None;
        };
        let next = pk + ck / 3.0;
        x.v.push(next);
        x.e.push((ek + err / 3.0 + gamma(2) * (pk.abs() + ck.abs() / 3.0)) * (1.0 + gamma(4)));
    }
    Some(vec![Piece {
        t0: lo,
        t1: hi,
        num: Bern {
            e: vec![err; coefs.len()],
            v: coefs,
        },
        w_min: 1.0,
        scale,
        x,
        w: Bern {
            v: vec![1.0; 4],
            e: vec![0.0; 4],
        },
    }])
}

impl Curve {
    /// (Not upstream, #17685) Adds to `add` the points of this curve over `interval`
    /// where a coordinate may stop changing, for [`Curve::calculate_tight_bounding_box`].
    ///
    /// A coordinate of the curve is greatest or least at an end, at a tangent
    /// discontinuous knot, or where `C' . e = 0`. The perpendicular analyses find the
    /// third kind only where `|C'|^2 >= SM_EFF_ZERO`; where the curve stops or nearly
    /// stops (the u = 0 side of a patch whose first row of control points all lie on one
    /// axis rises and turns back), they refuse to evaluate. So for each axis `e`, the
    /// roots of `C'_e` are isolated directly, with no normalisation by `|C'|` and no
    /// speed minimisation. The numerator of `C'_e` (that of the rational derivative for
    /// a rational curve) is taken in Bernstein form per Bezier segment, and sub-intervals
    /// are dropped where the convex hull of its coefficients, widened by their rounding
    /// bounds (which count each segment's own extraction), excludes zero. This misses no
    /// root of the stored curve's numerator. The curve's points at both ends of each kept
    /// range are added. On a kept range the coordinate's exact value at either end is
    /// within the following of its extreme over the range (before the map from `s` to
    /// `t`, and the evaluation, round):
    ///
    /// - `EPSILON * scale` where its change is bounded by that;
    /// - `4 p + EPSILON * scale` where the numerator is zero to within its rounding and
    ///   the hull is that narrow (a constant or nearly constant stretch). `p` bounds the
    ///   rounding of the range's control points. Their homogeneous values are off by at
    ///   most `14 deg u A` from their segment's extraction ([`bezier_points`]; `A` the
    ///   largest `|X|`, or weight, of the segment's `deg + 1` stored points) plus
    ///   `d u A` from `d` halvings, to first order. With `M` the largest `|x|` of those
    ///   stored points and `rho` the ratio of their largest to smallest weight (every
    ///   value is a convex combination of theirs, so `|X| <= M w_max` and
    ///   `w >= w_min`), `p <= 2 (14 deg + d) u M rho + u M` to first order, so this bound is at
    ///   most `8 (14 deg + d) u M rho + 3 EPSILON M`, `d <=` [`MAX_DEPTH`]. Both factors
    ///   are the segment's own: weights and values elsewhere on the curve do not enter;
    /// - `|C'|` times one ulp of `t`, where no double lies strictly inside the range.
    ///
    /// Kinds: the B-spline kinds ([`Curve::b_spline_base`], a curve with its NURB) and
    /// the cubic Hermite curve, over any `interval` (a Hermite curve's cubic is
    /// extrapolated past its natural domain, as it evaluates). The other kinds
    /// (procedural, offset, on-surface, composite, iso, projected) add nothing; their box
    /// is upstream's. A B-spline with out-of-bounds evaluation enabled is also searched
    /// over the part of `interval` past its knots, where its evaluation extends its first
    /// or last span's polynomial (#17987); without it, evaluation clamps past `[U_0, U_m]`.
    /// The ends of an unclamped curve's `[U_0, U_m]` outside its domain `[U_p, U_{n+1}]`
    /// are not searched.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a non-finite interval, a non-finite control point or weight, a
    /// weight that is not positive (also a weight of a rational B-spline's extension past
    /// its knots that is not certified positive: it may run through a pole), or when
    /// isolation needs more than [`MAX_DEPTH`] levels or [`MAX_NODES`] subdivisions;
    /// those of `find_span`, clamping, extraction and evaluation. An inverted (empty)
    /// interval adds nothing.
    pub(crate) fn add_turning_points(
        &self,
        k: &Kernel,
        interval: &Extent1d,
        pt: &mut Point3,
        add: &mut dyn FnMut(Point3),
    ) -> Result<()> {
        let (lo, hi) = (interval.min(), interval.max());
        if !(lo.is_finite() && hi.is_finite()) {
            return Err(SolidError::invalid_input(OP, "the interval is not finite"));
        }
        if lo > hi {
            return Ok(());
        }
        let mut ranges = Vec::new();
        for axis in 0..3 {
            let pieces = match self.kind() {
                CurveKind::Hermite(h) => {
                    let (c, d) = h.power_basis();
                    let (a, b) = (h.start_point(), h.start_derivative());
                    let pick = |v: crate::geom::vector3d::Vec3| match axis {
                        0 => v.x,
                        1 => v.y,
                        _ => v.z,
                    };
                    hermite_pieces([pick(a), pick(b), pick(c), pick(d)], lo, hi)
                }
                _ => match self.b_spline_base() {
                    Some(b) => match b.nurb.as_ref() {
                        Some(nl) => {
                            let past = b.is_out_of_bounds_enabled().then_some((lo, hi));
                            bspline_pieces(nl, axis, past)?
                        }
                        None => None,
                    },
                    None => None,
                },
            };
            let mut nodes = 0;
            for piece in pieces.iter().flatten() {
                isolate(k, piece, lo, hi, &mut nodes, &mut ranges)?;
            }
        }
        for (a, b) in ranges {
            for t in [a, b] {
                *pt = self.evaluate_point(k, t)?;
                add(*pt);
            }
        }
        Ok(())
    }
}

impl crate::eval::Curve2 {
    /// Knot boundaries and the ends of the isolated U and V turning ranges, in parameter order.
    /// Between these stations each coordinate is monotone up to the rounding of its extrema.
    /// The isolator is the same one that supplies spline extrema to the tight bounding box.
    ///
    /// # Errors
    /// Invalid spline data or a turning range that exceeds the isolation budget.
    pub fn monotone_breaks(&self, k: &Kernel) -> Result<Vec<f64>> {
        let invalid = || SolidError::invalid_input(OP, "invalid trim curve");
        let nl = crate::eval::spline::nl_curve2(self).ok_or_else(invalid)?;
        let [lo, hi] = self.interval().ok_or_else(invalid)?;
        let mut breaks: Vec<_> = nl
            .knots()
            .iter()
            .copied()
            .filter(|t| *t >= lo && *t <= hi)
            .collect();
        if nl.degree() > 1 {
            let mut ranges = Vec::new();
            for axis in 0..2 {
                let mut nodes = 0;
                for piece in bspline_pieces(&nl, axis, None)?.iter().flatten() {
                    isolate(k, piece, lo, hi, &mut nodes, &mut ranges)?;
                }
            }
            breaks.extend(ranges.into_iter().flat_map(|(a, b)| [a, b]));
        }
        breaks.sort_by(f64::total_cmp);
        breaks.dedup();
        Ok(breaks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A piece over [0, 1] with numerator `coefs` (each within `err`), whose coordinate
    /// runs from 0 to 1 (so its hull is never flat).
    fn test_piece(coefs: Vec<f64>, err: f64) -> Piece {
        Piece {
            t0: 0.0,
            t1: 1.0,
            num: Bern {
                e: vec![err; coefs.len()],
                v: coefs,
            },
            w_min: 1.0,
            scale: 1.0,
            x: Bern {
                v: vec![0.0, 1.0],
                e: vec![0.0; 2],
            },
            w: Bern {
                v: vec![1.0; 2],
                e: vec![0.0; 2],
            },
        }
    }

    #[test]
    fn halving_reproduces_the_polynomial() {
        // 1 - 3s + s^2 in degree 2 Bernstein form: 1, -0.5, -1.
        let c = [1.0, -0.5, -1.0];
        let (l, r) = Bern {
            v: c.to_vec(),
            e: vec![0.0; 3],
        }
        .halve();
        let (l, r) = (l.v, r.v);
        let f = |s: f64| 1.0 - 3.0 * s + s * s;
        assert_eq!(l.first().copied(), Some(f(0.0)));
        assert_eq!(l.last().copied(), Some(f(0.5)));
        assert_eq!(r.first().copied(), Some(f(0.5)));
        assert_eq!(r.last().copied(), Some(f(1.0)));
    }

    #[test]
    fn isolation_brackets_every_root_of_a_cubic() {
        // (s - 0.2)(s - 0.5)(s - 0.9) in Bernstein form.
        let pw = |s: f64| (s - 0.2) * (s - 0.5) * (s - 0.9);
        // Power coefficients: s^3 - 1.6 s^2 + 0.73 s - 0.09.
        let (a0, a1, a2, a3) = (-0.09, 0.73, -1.6, 1.0);
        let coefs = vec![
            a0,
            a0 + a1 / 3.0,
            a0 + 2.0 * a1 / 3.0 + a2 / 3.0,
            a0 + a1 + a2 + a3,
        ];
        let piece = test_piece(coefs, 1e-15);
        let mut out = Vec::new();
        let mut nodes = 0;
        isolate(&Kernel::default(), &piece, 0.0, 1.0, &mut nodes, &mut out).unwrap();
        for root in [0.2, 0.5, 0.9] {
            assert!(
                out.iter().any(|&(a, b)| a <= root && root <= b),
                "{root} not bracketed in {out:?}"
            );
        }
        for &(a, b) in &out {
            assert!(pw(a).abs() < 1e-12 || pw(b).abs() < 1e-12 || pw(a) * pw(b) <= 0.0);
        }
    }

    /// The tight box of a quadratic over `[0, 1]` with control points `pts` and weights
    /// `w` (`None`: non-rational).
    fn quadratic_box(pts: [[f64; 3]; 3], w: Option<[f64; 3]>) -> crate::geom::extent3d::Extent3d {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        let pw = pts
            .iter()
            .enumerate()
            .map(|(i, p)| match w {
                Some(w) => CPoint::new(p[0] * w[i], p[1] * w[i], p[2] * w[i], w[i]),
                None => CPoint::new(p[0], p[1], p[2], NOW),
            })
            .collect();
        let nl = NlCurve::new(pw, 2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]).unwrap();
        let curve = Curve::from(BSplineCurve::from_nurb(nl, 3, false));
        curve
            .calculate_tight_bounding_box(&Kernel::default(), &curve.natural_interval())
            .unwrap()
    }

    /// (#17685) A curve that crawls along x at speed 1e-7 while z rises and falls: at the
    /// peak `|C'|^2 = 1e-14`, under `SM_EFF_ZERO`, so the perpendicular analyses refuse
    /// it, and the curve never stops. The box reaches the peak, 0.5, to 4 ulp.
    #[test]
    fn tight_box_reaches_a_peak_where_the_curve_nearly_stops() {
        let bx = quadratic_box(
            [[0.0, 0.0, 0.0], [0.5e-7, 0.0, 1.0], [1e-7, 0.0, 0.0]],
            None,
        );
        let top = bx.max().z;
        assert!(
            (0.5 - 4.0 * f64::EPSILON * 0.5..=0.5 + 4.0 * f64::EPSILON).contains(&top),
            "{top}"
        );
    }

    /// The same through the rational derivative's numerator: weights 1, 2, 1 put the
    /// peak at t = 1/2, z = 2/3.
    #[test]
    fn tight_box_reaches_a_rational_curves_peak_where_it_nearly_stops() {
        let bx = quadratic_box(
            [[0.0, 0.0, 0.0], [0.5e-7, 0.0, 1.0], [1e-7, 0.0, 0.0]],
            Some([1.0, 2.0, 1.0]),
        );
        let top = bx.max().z;
        let peak = 2.0 / 3.0;
        assert!(
            (peak - 4.0 * f64::EPSILON..=peak + 4.0 * f64::EPSILON).contains(&top),
            "{top}"
        );
    }

    /// (#17894) A Hermite curve's box over an interval past its natural domain [0, 1]
    /// reaches the extremes of the extrapolated cubic (goldens `curve.curve_p9` gen2299
    /// and gen1409). Each expected value is the coordinate at the root of the quadratic
    /// `C'_e` in closed form, evaluated to 80 digits from the stored power coefficients
    /// and rounded to the nearest f64 (-21.550081498956349662, -10.263109697210416546 and
    /// -1.7094077137164749130).
    #[test]
    fn hermite_box_past_the_natural_domain_reaches_the_extrapolated_extremes() {
        use crate::curve::hermite_curve::HermiteCurve;
        use crate::geom::vector3d::Vec3;
        let close = |got: f64, want: f64| (got - want).abs() <= 4.0 * f64::EPSILON * want.abs();
        let k = Kernel::default();
        // gen2299 over [0.3454.., 1.0990..]: y and z are least at t = 1.01265.. and 1.04940..
        let h = Curve::from(HermiteCurve::new(
            Vec3::new(-24.764475207720782, 33.914804157735276, 40.20453211452791),
            Vec3::new(17.302293683225344, 20.451093437799685, 37.49545861097444),
            Vec3::new(14.080432946000627, -21.521127178924747, -9.855079971447076),
            Vec3::new(30.981215111180788, -4.556387745894654, -16.22411528431006),
            3,
        ));
        let ivl = Extent1d::from_min_max(0.3454394395402768, 1.0990214530465379);
        let bx = h.calculate_tight_bounding_box(&k, &ivl).unwrap();
        assert!(close(bx.min().y, -21.55008149895635), "{}", bx.min().y);
        assert!(close(bx.min().z, -10.263109697210416), "{}", bx.min().z);
        // gen1409 over [-0.0941.., 0.6556..]: z is least at t = -0.0055861..
        let h = Curve::from(HermiteCurve::new(
            Vec3::new(-2.190577544349888, 2.06733903830132, -1.7090144666134555),
            Vec3::new(1.7004875847166918, -1.5359313661696896, 0.14054078307885254),
            Vec3::new(-2.8870201748942197, 0.10026274459280593, 2.8302137285376148),
            Vec3::new(2.9667880221770266, 0.6553888862005781, 0.8251558720527767),
            3,
        ));
        let ivl = Extent1d::from_min_max(-0.0941559708344709, 0.6556683361925206);
        let bx = h.calculate_tight_bounding_box(&k, &ivl).unwrap();
        assert!(close(bx.min().z, -1.709407713716475), "{}", bx.min().z);
    }

    /// A degree 2 B-spline with `knots`, control points `pts` and weights `w` (`None`:
    /// non-rational), evaluating past its knots.
    fn out_of_bounds_quadratic(pts: &[[f64; 3]], w: Option<&[f64]>, knots: Vec<f64>) -> Curve {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        let pw = pts
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let wi = w.and_then(|w| w.get(i).copied());
                match wi {
                    Some(wi) => CPoint::new(p[0] * wi, p[1] * wi, p[2] * wi, wi),
                    None => CPoint::new(p[0], p[1], p[2], NOW),
                }
            })
            .collect();
        let nl = NlCurve::new(pw, 2, knots).unwrap();
        let mut b = BSplineCurve::from_nurb(nl, 3, false);
        b.set_out_of_bounds_enabled(true);
        Curve::from(b)
    }

    /// (#17987) With out-of-bounds evaluation enabled, a B-spline over an interval past
    /// its knots is its first or last polynomial piece extended, and its box reaches that
    /// extension's extremes, each given in closed form:
    ///
    /// - the clamped quadratic (0,0,0), (1,0,1), (2,0,1.2) on [0, 1] has `z = 2t - 0.8t^2`;
    ///   over [0, 2] z is greatest, 1.25, at t = 1.25, past the last knot (the box held
    ///   only z(1) = 1.2 before);
    /// - the same points reversed peak at t = -0.25, before the first knot, over [-1, 1];
    /// - two spans (knots 0, 0, 0, 1, 2, 2, 2) with `x = t`: on [0, 1] `z = 1 - t - t^2`,
    ///   greatest, 1.25, at t = -0.5; on [1, 2] `z = -1 - 3s + s^2` (`s = t - 1`), least,
    ///   -3.25, at t = 2.5. Over [-1, 3] each extension takes its own span's polynomial.
    #[test]
    fn b_spline_box_past_its_knots_reaches_the_extrapolated_extremes() {
        let k = Kernel::default();
        let close = |got: f64, want: f64| (got - want).abs() <= 8.0 * f64::EPSILON * want.abs();
        let clamped = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
        let c = out_of_bounds_quadratic(
            &[[0.0, 0.0, 0.0], [1.0, 0.0, 1.0], [2.0, 0.0, 1.2]],
            None,
            clamped.clone(),
        );
        let bx = c
            .calculate_tight_bounding_box(&k, &Extent1d::from_min_max(0.0, 2.0))
            .unwrap();
        assert!(close(bx.max().z, 1.25), "{}", bx.max().z);
        let c = out_of_bounds_quadratic(
            &[[2.0, 0.0, 1.2], [1.0, 0.0, 1.0], [0.0, 0.0, 0.0]],
            None,
            clamped,
        );
        let bx = c
            .calculate_tight_bounding_box(&k, &Extent1d::from_min_max(-1.0, 1.0))
            .unwrap();
        assert!(close(bx.max().z, 1.25), "{}", bx.max().z);
        let c = out_of_bounds_quadratic(
            &[
                [0.0, 0.0, 1.0],
                [0.5, 0.0, 0.5],
                [1.5, 0.0, -2.5],
                [2.0, 0.0, -3.0],
            ],
            None,
            vec![0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0],
        );
        let bx = c
            .calculate_tight_bounding_box(&k, &Extent1d::from_min_max(-1.0, 3.0))
            .unwrap();
        assert!(close(bx.max().z, 1.25), "{}", bx.max().z);
        assert!(close(bx.min().z, -3.25), "{}", bx.min().z);
        // Over its knots only, the box is the one without out-of-bounds evaluation.
        let pts = [[0.0, 0.0, 0.0], [1.0, 0.0, 1.0], [2.0, 0.0, 1.2]];
        let c = out_of_bounds_quadratic(&pts, None, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
        let bx = c
            .calculate_tight_bounding_box(&k, &Extent1d::from_min_max(0.0, 1.0))
            .unwrap();
        let inside = quadratic_box(pts, None);
        assert!(bx.min() == inside.min() && bx.max() == inside.max());
    }

    /// (#17987) A rational extension with positive weights: the 45 degree arc of the unit
    /// circle from (1, 0) (weights 1, cos 22.5 degrees, 1) continues round the circle past
    /// its last knot, to about 188 degrees at t = 20. Its box over [0, 20] reaches
    /// y = 1 (90 degrees) and x = -1 (180 degrees), both past the knots.
    #[test]
    fn a_circle_arcs_extension_reaches_the_circles_extremes() {
        let h = std::f64::consts::FRAC_PI_8;
        let c = out_of_bounds_quadratic(
            &[
                [1.0, 0.0, 0.0],
                [1.0, h.tan(), 0.0],
                [
                    std::f64::consts::FRAC_1_SQRT_2,
                    std::f64::consts::FRAC_1_SQRT_2,
                    0.0,
                ],
            ],
            Some(&[1.0, h.cos(), 1.0]),
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        );
        let bx = c
            .calculate_tight_bounding_box(&Kernel::default(), &Extent1d::from_min_max(0.0, 20.0))
            .unwrap();
        assert!(
            (bx.max().y - 1.0).abs() <= 16.0 * f64::EPSILON,
            "{}",
            bx.max().y
        );
        assert!(
            (bx.min().x + 1.0).abs() <= 16.0 * f64::EPSILON,
            "{}",
            bx.min().x
        );
    }

    /// (#17987) Bezier weights that are not all positive where the weight is: weights 1,
    /// 0.5, 0.3 give `w = 1 - t + 0.3t^2 > 0`, but over [1, 4] the extension's Bezier
    /// weights are 0.3, -0.3, 1.8. The part is halved until each half's are positive, and
    /// the box over [0, 4] reaches z's least value past the knots: with points (0, 0, 0),
    /// (1, 0, 1), (2, 0, 0), `z = (t - t^2) / w`, least where `1 - 2t + 0.7t^2 = 0`, at
    /// t = (2 + sqrt 1.2) / 1.4 = 2.2110..., z = -10.477225575051661... (to 50 digits,
    /// rounded to the nearest f64).
    #[test]
    fn a_rational_extension_with_a_negative_bezier_weight_is_halved() {
        let c = out_of_bounds_quadratic(
            &[[0.0, 0.0, 0.0], [1.0, 0.0, 1.0], [2.0, 0.0, 0.0]],
            Some(&[1.0, 0.5, 0.3]),
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        );
        let bx = c
            .calculate_tight_bounding_box(&Kernel::default(), &Extent1d::from_min_max(0.0, 4.0))
            .unwrap();
        let want = -10.47722557505166;
        assert!(
            (bx.min().z - want).abs() <= 16.0 * f64::EPSILON * want.abs(),
            "{}",
            bx.min().z
        );
    }

    /// (#17987) A rational extension through a pole is refused, as a stored weight that is
    /// not positive is: the quadratic with weights 1, 3, 1 on [0, 1] has
    /// `w = 1 + 4t - 4t^2`, zero at t = (1 + sqrt 2) / 2 = 1.207..., so over [0, 2] it has a
    /// pole. Over [0, 1] and [0, 1.2] there is none, and the search succeeds.
    #[test]
    fn a_rational_extension_through_a_pole_is_refused() {
        let c = out_of_bounds_quadratic(
            &[[0.0, 0.0, 0.0], [1.0, 0.0, 1.0], [2.0, 0.0, 0.0]],
            Some(&[1.0, 3.0, 1.0]),
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        );
        let search = |hi: f64| {
            let mut pt = crate::geom::vector3d::Vec3::UNDEF;
            c.add_turning_points(
                &Kernel::default(),
                &Extent1d::from_min_max(0.0, hi),
                &mut pt,
                &mut |_| {},
            )
        };
        let r = search(2.0);
        assert!(matches!(r, Err(SolidError::InvalidInput { .. })), "{r:?}");
        assert!(search(1.0).is_ok() && search(1.2).is_ok());
    }

    /// A rational quadratic in two Bezier segments (knots 0, 0, 0, 1, 1, 2, 2, 2). Over
    /// [0, 1] z is 5 at both ends and `5 + dz` at the middle control point, with weights
    /// `wa`, `wb`, `wa`; over [1, 2] it falls to 1 with weights `wa`. x and y vary on both.
    fn two_segment(wa: f64, wb: f64, dz: f64) -> Curve {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        let pts = [
            [0.0, 0.0, 5.0],
            [0.5, 0.1, 5.0 + dz],
            [1.0, 0.0, 5.0],
            [1.5, 0.1, 3.0],
            [2.0, 0.0, 1.0],
        ];
        let w = [wa, wb, wa, wa, wa];
        let pw = pts
            .iter()
            .zip(w)
            .map(|(p, w)| CPoint::new(p[0] * w, p[1] * w, p[2] * w, w))
            .collect();
        let knots = vec![0.0, 0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 2.0];
        let nl = NlCurve::new(pw, 2, knots).unwrap();
        Curve::from(BSplineCurve::from_nurb(nl, 3, false))
    }

    /// The number of points [`Curve::add_turning_points`] adds over the natural interval.
    fn turning_calls(c: &Curve) -> Result<usize> {
        let mut calls = 0;
        let mut pt = crate::geom::vector3d::Vec3::UNDEF;
        c.add_turning_points(
            &Kernel::default(),
            &c.natural_interval(),
            &mut pt,
            &mut |_| calls += 1,
        )?;
        Ok(calls)
    }

    /// (#17961 review) A segment whose z is constant, or constant to 1e-13 or 1e-10, under
    /// weight ratios 1e2, 1e3 and 1e5: the derivative's numerator is zero to within its
    /// rounding there, so its sign test never drops a piece. The search must still end
    /// after a few pieces, not tile the segment (2^19 ranges at ratio 1e2) or run out of
    /// subdivisions (`InvalidInput` from ratio 1e3), and the box reaches z's greatest value,
    /// `(X_0 + X_1) / (w_a + w_b)` at t = 1/2 (both 5 when `dz` is 0), to 4 ulp.
    #[test]
    fn a_flat_segment_under_heavy_weights_ends_in_a_few_pieces() {
        for (wa, wb) in [(1.0, 100.0), (1.0, 1000.0), (1e-3, 1.0), (1e-3, 100.0)] {
            for dz in [0.0, 1e-13, 1e-10] {
                let c = two_segment(wa, wb, dz);
                let calls = turning_calls(&c);
                assert!(
                    matches!(calls, Ok(n) if n <= 256),
                    "weights {wa} {wb} dz {dz}: {calls:?}"
                );
                let bx = c
                    .calculate_tight_bounding_box(&Kernel::default(), &c.natural_interval())
                    .unwrap();
                let peak = ((5.0 * wa) + (5.0 + dz) * wb) / (wa + wb);
                let top = bx.max().z;
                assert!(
                    (top - peak).abs() <= 4.0 * f64::EPSILON * peak,
                    "weights {wa} {wb} dz {dz}: {top} against {peak}"
                );
            }
        }
    }

    /// (#17961 review) An unclamped B-spline (uniform knots 0..5, degree 2, domain [2, 3])
    /// crawling along x as the first test's: its turning points reach z's peak 3/4 at
    /// t = 5/2 (`z = 1/2 + s - s^2`, `s = t - 2`) to 4 ulp, rather than refusing. (The whole
    /// box is not checked: `BSplineCurve` takes `[U_0, U_m]` as the natural interval, as
    /// upstream, so its own evaluation does not support unclamped knots.)
    #[test]
    fn an_unclamped_curves_turning_points_reach_its_peak() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        let pw = vec![
            CPoint::new(0.0, 0.0, 0.0, NOW),
            CPoint::new(0.5e-7, 0.0, 1.0, NOW),
            CPoint::new(1e-7, 0.0, 0.0, NOW),
        ];
        let nl = NlCurve::new(pw, 2, vec![0.0, 1.0, 2.0, 3.0, 4.0, 5.0]).unwrap();
        let c = Curve::from(BSplineCurve::from_nurb(nl, 3, false));
        let ivl = Extent1d::from_min_max(2.0, 3.0);
        let mut top = f64::NEG_INFINITY;
        let mut pt = crate::geom::vector3d::Vec3::UNDEF;
        c.add_turning_points(&Kernel::default(), &ivl, &mut pt, &mut |p| {
            top = top.max(p.z);
        })
        .unwrap();
        assert!((top - 0.75).abs() <= 4.0 * f64::EPSILON * 0.75, "{top}");
    }

    /// (#17961 review r2) Simple interior knots (0, 0, 0, 1, 2.3, 3.1, 4, 4, 4), so
    /// extraction inserts knots, under end weights 100 against `wl` = 1 or 1e-3 (weight
    /// ratios 1e2 and 1e5). z has a bump of `dz` (1e-10 or 1e-7) over [1, 2.3], whose
    /// three stored points all have weight `wl`, while x and y crawl at 1e-7, so the
    /// perpendicular analyses refuse it and the box's z max comes from the turning points.
    ///
    /// Over [1, 2.3] the weights are equal, so z is the quadratic of the stored `X / wl`.
    /// Its peak, from the stored f64 values in exact rational arithmetic (vertex
    /// `(c0 c2 - c1^2) / (c0 - 2 c1 + c2)` of its Bezier values), is, by `(wl, dz)`:
    /// 5.0000000000769230255 (1, 1e-10), 5.0000000000769229690 (1e-3, 1e-10),
    /// 5.0000000769230772023 (1, 1e-7) and 5.0000000769230773073 (1e-3, 1e-7); `peak` is
    /// the nearest f64. Every other span's hull stays under `5 + 0.61 dz`.
    /// The box must reach it to within the documented rule (b) bound for that span
    /// (`M = 5 + dz`, `rho = 1`, degree 2, `d <= MAX_DEPTH`; constants doubled to
    /// cover the second-order terms) plus the evaluation's rounding (two de Boor levels
    /// and a division) and `peak`'s, under `gamma_16 M`. While extraction's rounding was
    /// sized from the end weights, (1e-3, 1e-7) missed by 6.4e-10, over 300 times that.
    #[test]
    fn heavy_end_weights_do_not_widen_a_bumps_turning_point() {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        let k = Kernel::default();
        for (wl, dz, peak) in [
            (1.0, 1e-10, 5.000000000076923),
            (1e-3, 1e-10, 5.000000000076923),
            (1.0, 1e-7, 5.000000076923077),
            (1e-3, 1e-7, 5.000000076923078),
        ] {
            let pts = [
                [0.0, 0.0, 5.0],
                [0.5e-7, 0.1e-7, 5.0],
                [1.0e-7, 0.0, 5.0 + dz],
                [1.5e-7, 0.1e-7, 5.0 + 0.37 * dz],
                [2.0e-7, 0.0, 5.0],
                [2.5e-7, 0.1e-7, 1.0],
            ];
            let w = [100.0, wl, wl, wl, wl, 100.0];
            let pw = pts
                .iter()
                .zip(w)
                .map(|(p, w)| CPoint::new(p[0] * w, p[1] * w, p[2] * w, w))
                .collect();
            let knots = vec![0.0, 0.0, 0.0, 1.0, 2.3, 3.1, 4.0, 4.0, 4.0];
            let nl = NlCurve::new(pw, 2, knots).unwrap();
            let c = Curve::from(BSplineCurve::from_nurb(nl, 3, false));
            let m = 5.0 + dz;
            let u = f64::EPSILON / 2.0;
            let rule_b =
                8.0 * (2.0 * 14.0 * 2.0 + 2.0 * MAX_DEPTH as f64) * u * m + 4.0 * f64::EPSILON * m;
            let eval = gamma(16) * m;
            let top = c
                .calculate_tight_bounding_box(&k, &c.natural_interval())
                .unwrap()
                .max()
                .z;
            assert!(
                top >= peak - rule_b - eval && top <= peak + eval,
                "wl {wl} dz {dz}: {top} against {peak}, short by {}",
                peak - top
            );
        }
    }

    /// A cancelled kernel stops the search at its next poll: a numerator that is zero to
    /// within a large bound over a hull that is not flat is tiled, past `CANCEL_POLL`.
    #[test]
    fn a_cancelled_search_stops_at_its_poll() {
        let mut k = Kernel::default();
        let token = crate::kernel::CancelToken::new();
        token.cancel();
        k.set_cancel_token(token);
        let piece = test_piece(vec![0.0, 0.0], 1.0);
        let (mut out, mut nodes) = (Vec::new(), 0);
        let r = isolate(&k, &piece, 0.0, 1.0, &mut nodes, &mut out);
        assert!(r.as_ref().is_err_and(SolidError::is_cancelled), "{r:?}");
        assert_eq!(nodes, CANCEL_POLL);
    }

    /// A node that overflows mid-search (coefficients near `f64::MAX`, halved into
    /// infinity) refuses instead of folding a NaN.
    #[test]
    fn an_overflowing_node_is_refused() {
        let piece = test_piece(vec![f64::MAX, f64::MAX, -f64::MAX], 0.0);
        let (mut out, mut nodes) = (Vec::new(), 0);
        let r = isolate(&Kernel::default(), &piece, 0.0, 1.0, &mut nodes, &mut out);
        assert!(matches!(r, Err(SolidError::InvalidInput { .. })), "{r:?}");
        // The per-node finite check refuses within the first halvings; without it the
        // search would run on to the MAX_NODES refusal instead.
        assert!(format!("{r:?}").contains("non-finite coefficient"), "{r:?}");
        assert!(nodes <= 2, "{nodes}");
    }

    /// The greatest z among the points `add_turning_points` adds for the quadratic with
    /// control points `pts`, all of weight `w`, over the clamped knots `t0, t0, t0, t1,
    /// t1, t1`. The knots are used as given (a borrowed NURB, whose knots are not fixed
    /// up), so a span narrower than the fix-up's tolerance stays.
    fn turning_top_z(pts: [[f64; 3]; 3], w: f64, t0: f64, t1: f64) -> f64 {
        use crate::nurbs::b_spline_curve::BSplineCurve;
        let pw = pts
            .iter()
            .map(|p| CPoint::new(p[0] * w, p[1] * w, p[2] * w, w))
            .collect();
        let nl = NlCurve::new(pw, 2, vec![t0, t0, t0, t1, t1, t1]).unwrap();
        let c = Curve::from(BSplineCurve::from_nurb(nl, 3, true));
        let mut top = f64::NEG_INFINITY;
        let mut pt = crate::geom::vector3d::Vec3::UNDEF;
        c.add_turning_points(
            &Kernel::default(),
            &Extent1d::from_raw(t0, t1),
            &mut pt,
            &mut |p| top = top.max(p.z),
        )
        .unwrap();
        top
    }

    /// (#17961 review) Equal weights past `1e154` keep the change bound finite:
    /// `w_min^2` overflowed to infinity, `change` came out 0 and the whole span was kept
    /// as one range, whose ends (z = 0) miss the peak z = 5e-5 at t = 1/2. With weights 1
    /// and 1e155 the same curve's peak is found, to 4 ulp. (Coordinates stay under
    /// `1e-4`, so the numerator, about `|x| w^2`, stays finite.)
    #[test]
    fn huge_equal_weights_keep_the_peak() {
        let pts = [[0.0, 0.0, 0.0], [5e-5, 0.0, 1e-4], [1e-4, 0.0, 0.0]];
        for w in [1.0, 1e155] {
            let top = turning_top_z(pts, w, 0.0, 1.0);
            assert!(
                (top - 5e-5).abs() <= 4.0 * f64::EPSILON * 5e-5,
                "w {w}: {top}"
            );
        }
    }

    /// (#17961 review) Knots `2^52` and `2^52 + 2`: the span is two ulps of `t`
    /// wide, with the double `2^52 + 1` inside, where z peaks at 1/2. A two-ulp width test
    /// kept the whole span and added only its ends (z = 0); the range is now split until
    /// no double lies inside it, so the peak is added exactly.
    #[test]
    fn a_span_a_few_ulps_wide_keeps_its_interior_peak() {
        let pts = [[0.0, 0.0, 0.0], [0.5, 0.0, 1.0], [1.0, 0.0, 0.0]];
        let t0 = 2f64.powi(52);
        assert_eq!(turning_top_z(pts, 1.0, 0.0, 2.0), 0.5);
        assert_eq!(turning_top_z(pts, 1.0, t0, t0 + 2.0), 0.5);
    }

    /// Extraction of a span with full-multiplicity knots copies the stored values with no
    /// rounding; a span with simple knots carries a bound of its own values' size.
    #[test]
    fn extraction_bounds_are_local() {
        // Degree 2, span [1, 2] of knots .., 0, 1, 2, 3, ..: u = U_{j-1} ..= U_{j+2}.
        let got = bezier_points(&[1e-3, 2e-3, 3e-3], &[0.0, 1.0, 2.0, 3.0], 1.0, 2.0).unwrap();
        let want = [1.5e-3, 2e-3, 2.5e-3];
        for (&(v, e), w) in got.iter().zip(want) {
            assert!(
                (v - w).abs() <= e && e <= 40.0 * f64::EPSILON * 3e-3,
                "{v} {e}"
            );
        }
        // Full multiplicity: knots 1, 1, 2, 2 around the span [1, 2].
        let got = bezier_points(&[7.0, 8.0, 9.0], &[1.0, 1.0, 2.0, 2.0], 1.0, 2.0).unwrap();
        assert_eq!(got, vec![(7.0, 0.0), (8.0, 0.0), (9.0, 0.0)]);
    }

    /// An inverted interval is empty: nothing to add, and no panic.
    #[test]
    fn an_inverted_interval_adds_nothing() {
        let c = two_segment(1.0, 2.0, 1.0);
        let mut calls = 0;
        let mut pt = crate::geom::vector3d::Vec3::UNDEF;
        let r = c.add_turning_points(
            &Kernel::default(),
            &Extent1d::from_raw(2.0, 0.0),
            &mut pt,
            &mut |_| calls += 1,
        );
        assert!(r.is_ok() && calls == 0, "{r:?} {calls}");
    }

    #[test]
    fn non_finite_coefficients_are_refused() {
        let piece = test_piece(vec![f64::NAN, 1.0], 0.0);
        let mut out = Vec::new();
        let mut nodes = 0;
        let r = isolate(&Kernel::default(), &piece, 0.0, 1.0, &mut nodes, &mut out);
        assert!(matches!(r, Err(SolidError::InvalidInput { .. })));
    }
}
