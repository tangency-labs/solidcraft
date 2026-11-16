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
//! | `hermite_pieces` (private)    | none     |
//!
//! Deviations: not a port. The subdivision is a loop over an explicit stack. It is
//! bounded by [`MAX_DEPTH`] levels per piece and [`MAX_NODES`] subdivisions per curve
//! and axis; past either it refuses with `InvalidInput`. Rounding is counted with
//! [`gamma`] throughout (operation counts at each bound), ignoring underflow.

use crate::curve::curve::p2::{Curve, CurveKind};
use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::vector3d::Point3;
use crate::kernel::Kernel;
use crate::nurbs::crv_basic::Curve as NlCurve;
use crate::nurbs::iges::clamp_curve;
use crate::nurbs::math::{gamma, CPoint, NOW, NOZ};
use crate::nurbs::nurbs_crv::{create_bez_segments, is_clamped};

const OP: &str = "curve::curve::turning::add_turning_points";

/// The deepest a piece is halved. Each level at least halves the bound on the
/// coordinate's change over a sub-interval, so a piece needs about
/// `52 + log2(2 p (wmax / wmin)^2)` levels to bring that bound under the coordinate's
/// rounding (see `isolate`). That is under 200 for any degree up to the NURB limit (32)
/// and a weight ratio up to `2^70`.
pub(crate) const MAX_DEPTH: usize = 200;

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
            v: Vec::with_capacity(q),
            e: Vec::with_capacity(q),
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
/// six operations, and `1 + gamma_1` the rounding of the spread. `false` when a weight
/// is not above its error bound.
fn hull_is_flat(x: &Bern, w: &Bern, scale: f64) -> bool {
    let (mut lo, mut hi, mut perr) = (f64::INFINITY, f64::NEG_INFINITY, 0.0_f64);
    for (((&xi, &exi), &wi), &ewi) in x.v.iter().zip(&x.e).zip(&w.v).zip(&w.e) {
        if wi <= ewi {
            return false;
        }
        let y = xi / wi;
        let p = ((exi + y.abs() * ewi) / (wi - ewi) + gamma(1) * y.abs()) * (1.0 + gamma(8));
        lo = lo.min(y);
        hi = hi.max(y);
        perr = perr.max(p);
    }
    let spread = (hi - lo) * (1.0 + gamma(1));
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
/// - it is no wider than two ulps of its parameter.
///
/// Without the second rule a stretch where the numerator is zero to within rounding is
/// never dropped and the first takes a width under `1 / (C p^2 (w_max / w_min)^2)`, so it
/// would be tiled into that many ranges.
fn isolate(
    piece: &Piece,
    lo: f64,
    hi: f64,
    nodes: &mut usize,
    out: &mut Vec<(f64, f64)>,
) -> Result<()> {
    if !(piece.num.is_finite()
        && piece.x.is_finite()
        && piece.w.is_finite()
        && piece.scale.is_finite()
        && piece.w_min > 0.0)
    {
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
        let above = c.v.iter().zip(&c.e).all(|(&v, &e)| v > e);
        let below = c.v.iter().zip(&c.e).all(|(&v, &e)| v < -e);
        if above || below {
            continue;
        }
        let big = c.v.iter().fold(0.0_f64, |m, v| m.max(v.abs()));
        let err = c.e.iter().fold(0.0_f64, |m, e| m.max(*e));
        let change = (node.s1 - node.s0) * (big + err) / (piece.w_min * piece.w_min);
        let ulp2 = 2.0 * f64::EPSILON * ta.abs().max(tb.abs());
        let zero = c.v.iter().zip(&c.e).all(|(&v, &e)| v.abs() <= e);
        if change <= f64::EPSILON * piece.scale
            || (zero && hull_is_flat(&node.x, &node.w, piece.scale))
            || tb - ta <= ulp2
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

/// Whether some interior knot of the clamped `knots` has multiplicity under `p`, so
/// Bezier extraction inserts knots (and rounds).
fn extraction_inserts(knots: &[f64], p: usize) -> bool {
    let inner = knots
        .get(p + 1..knots.len().saturating_sub(p + 1))
        .unwrap_or(&[]);
    let mut run = 0_usize;
    for (i, &u) in inner.iter().enumerate() {
        run += 1;
        if inner.get(i + 1) != Some(&u) {
            if run < p {
                return true;
            }
            run = 0;
        }
    }
    false
}

/// The derivative numerators of coordinate `axis` of the B-spline `nl`, one piece per
/// Bezier segment. `None` when the coordinate does not exist (2D), is the same at every
/// control point (the coordinate is then constant, so its ends hold it), or the degree is
/// 0 (`C' = 0` on every span). A curve with unclamped knots is first clamped to its domain
/// `[U_p, U_{n+1}]` by knot insertion ([`clamp_curve`]), which leaves the curve over it
/// the same.
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
/// Rounding: the segment's points are those of the stored curve up to the rounding of
/// clamping and extraction. Each is at most `2p` knot-insertion steps per pass
/// (`p` at each end of its span), each a convex combination, so as in
/// `nurbs_crv::piece_slack` every homogeneous component is within
/// `gamma_{16 L}` of `w_max M` (`X`) or `w_max` (`w`), `L` the steps (0 when neither
/// pass inserts a knot), `M` and `w_max` the stored curve's largest `|x|` and weight. `c_m`
/// is then `p + 11` roundings of terms bounded by its magnitude sum (`D`: two products and
/// a difference; the difference of two `D`; the binomial ratio, three roundings and two
/// operations; the product; the sum of at most `p` terms; the factor `p`), plus the effect
/// of the points' error on each `D`, `e_X (w_i + w_j) + e_w (|X_i| + |X_j|) + 2 e_X e_w`.
/// A non-rational `c_k` has two roundings and `2 p e_X` from the points.
fn bspline_pieces(nl: &NlCurve, axis: usize) -> Result<Option<Vec<Piece>>> {
    let pts = nl.cpts();
    let mut first = None;
    let mut varies = false;
    let (mut big_x, mut w_max) = (0.0_f64, 0.0_f64);
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
        big_x = big_x.max(x.abs());
        w_max = w_max.max(h.1);
        match first {
            None => first = Some(x),
            Some(f) => varies |= x != f,
        }
    }
    let p = nl.degree();
    if !varies || p == 0 {
        return Ok(None);
    }
    let mut steps = 0_usize;
    let mut clamped;
    let mut nl = nl;
    if !is_clamped(nl.knots(), p) {
        let (Some(&a), Some(&b)) = (nl.knots().get(p), nl.knots().get(nl.n() + 1)) else {
            return Err(SolidError::invalid_input(
                OP,
                "a B-spline's knots do not cover its control points",
            ));
        };
        clamped = nl.clone();
        clamp_curve(&mut clamped, [a, b])?;
        nl = &clamped;
        steps += 2 * p;
    }
    if extraction_inserts(nl.knots(), p) {
        steps += 2 * p;
    }
    let e_x = gamma(16 * steps) * w_max * big_x;
    let e_w = if steps == 0 {
        0.0
    } else {
        gamma(16 * steps) * w_max
    };
    let rational = pts.iter().any(|c| c.w != NOW);
    let pf = p as f64;
    let mut pieces = Vec::new();
    for seg in create_bez_segments(nl)? {
        let (Some(&t0), Some(&t1)) = (seg.knots().first(), seg.knots().last()) else {
            return Err(SolidError::invalid_input(
                OP,
                "a Bezier segment has no knots",
            ));
        };
        let hs: Vec<(f64, f64)> = seg
            .cpts()
            .iter()
            .filter_map(|&c| homogeneous(c, axis))
            .collect();
        if seg.degree() != p || hs.len() != p + 1 {
            return Err(SolidError::invalid_input(
                OP,
                "a Bezier segment has not the curve's degree and dimension",
            ));
        }
        let scale = hs.iter().fold(0.0_f64, |m, &(x, w)| m.max((x / w).abs()));
        let w_min = hs.iter().fold(f64::INFINITY, |m, &(_, w)| m.min(w - e_w));
        let num = if rational {
            let d = |i: usize, j: usize| -> (f64, f64, f64) {
                match (hs.get(i), hs.get(j)) {
                    (Some(&(xi, wi)), Some(&(xj, wj))) => (
                        xi * wj - xj * wi,
                        (xi * wj).abs() + (xj * wi).abs(),
                        e_x * (wi + wj) + e_w * (xi.abs() + xj.abs()) + 2.0 * e_x * e_w,
                    ),
                    _ => (0.0, 0.0, 0.0),
                }
            };
            let q = 2 * p - 1;
            let g = gamma(p + 11);
            let mut num = Bern {
                v: Vec::with_capacity(q + 1),
                e: Vec::with_capacity(q + 1),
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
                v: Vec::with_capacity(p),
                e: Vec::with_capacity(p),
            };
            for (a, b) in hs.iter().skip(1).zip(&hs) {
                let c = pf * (a.0 - b.0);
                num.v.push(c);
                num.e
                    .push(gamma(2) * c.abs() + (1.0 + gamma(2)) * 2.0 * pf * e_x);
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
                e: vec![e_x; p + 1],
            },
            w: Bern {
                v: hs.iter().map(|h| h.1).collect(),
                e: vec![e_w; p + 1],
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
    /// bounds (which count clamping and extraction), excludes zero. This misses no root
    /// of the stored curve's numerator. Every kept range is one where the coordinate
    /// changes by at most its rounding, one where the numerator is zero to within its
    /// rounding and the coordinate's hull is that narrow (a constant or nearly constant
    /// stretch), or one no wider than two ulps of the parameter. The curve's points at
    /// both ends of each range are added.
    ///
    /// Kinds: the B-spline kinds ([`Curve::b_spline_base`], a curve with its NURB) and
    /// the cubic Hermite curve, over any `interval` (a Hermite curve's cubic is
    /// extrapolated past its natural domain, as it evaluates). The other kinds
    /// (procedural, offset, on-surface, composite, iso, projected) add nothing; their box
    /// is upstream's. A B-spline is searched over its knots' range only: a part of
    /// `interval` past them (with out-of-bounds evaluation enabled) is not searched.
    ///
    /// # Errors
    ///
    /// `InvalidInput` for a non-finite interval, a non-finite control point or weight, a
    /// weight that is not positive, or when isolation needs more than [`MAX_DEPTH`]
    /// levels or [`MAX_NODES`] subdivisions; those of clamping, extraction and
    /// evaluation. An inverted (empty) interval adds nothing.
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
                _ => match self.b_spline_base().and_then(|b| b.nurb.as_ref()) {
                    Some(nl) => bspline_pieces(nl, axis)?,
                    None => None,
                },
            };
            let mut nodes = 0;
            for piece in pieces.iter().flatten() {
                isolate(piece, lo, hi, &mut nodes, &mut ranges)?;
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
        isolate(&piece, 0.0, 1.0, &mut nodes, &mut out).unwrap();
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
        let r = isolate(&piece, 0.0, 1.0, &mut nodes, &mut out);
        assert!(matches!(r, Err(SolidError::InvalidInput { .. })));
    }
}
