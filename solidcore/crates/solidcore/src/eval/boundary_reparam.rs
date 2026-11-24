//! A pcurve along a patch's boundary for an edge that lies on the boundary but runs at a pace of
//! its own: the straight line `(u₀, g(t))` with `g` a monotone piecewise-linear map from the
//! edge's parameter to the boundary's, and a certified bound of how far the edge is from where
//! that pcurve puts it.
//!
//! The map is found by projecting points of the edge onto the boundary (a search: nothing here
//! trusts it). The bound does not depend on how `g` was found. With `g` linear on `[τ_k, τ_{k+1}]`
//! and `g(τ_k) = γ_k`, the lift of the pcurve there is `B(g(t))`, the boundary `B` on
//! `[γ_k, γ_{k+1}]` at an affine pace. A polynomial's restriction to an interval is a polynomial
//! of the same degree whose Bézier points do not depend on the pace, so on a span of `B`
//! containing the interval they are the blossom of the span's Bézier points at the interval's
//! ends. The pieces over `k` joined at their shared ends are a spline `R` of degree `p` (knots
//! `τ_k` of multiplicity `p`) that is *exactly* the lift, and the edge is compared with `R` at
//! equal parameter: [`certified_distance`] and the extraction's rounding.
//!
//! Every piece must lie in one span of `B`, so the breakpoints include the parameter of each
//! interior knot of `B` that `g` passes (a piece over a knot would have a knot at an unknown
//! parameter of `t`); where that cannot be arranged there is no proof.
//!
//! Rounding (docs/definitions-and-certificates.md §4.4). `M` is the largest coordinate of the
//! boundary's and the edge's control points.
//!
//! 1. The boundary's Bézier decomposition: as [`certified_distance`] counts a refinement, `I`
//!    insertions carry `γ_(12 (I + 1))` of `2 M` per coordinate.
//! 2. A local parameter `λ = fl(fl(γ − κ_j) / fl(κ_{j+1} − κ_j))` is off its exact value by at
//!    most `δ = γ₂` (`λ` in `[0, 1]`, two roundings). The sub-extraction at the computed `λ` is
//!    then the exact sub-extraction at a parameter within `δ` of the intended one, which moves
//!    each point of the piece by at most `δ` times the largest derivative of the span's Bézier
//!    curve, `p` times its largest control difference `D`: `δ p D`.
//! 3. The blossom at the computed parameters: each of the `p` levels forms `fl((1 − λ) x + λ y)`
//!    with `fl(1 − λ)` one rounding (`λ` and `fl(1 − λ)` sum to within `u` of 1), an error of at
//!    most `γ₃` of the larger magnitude, which is at most `M` (a convex combination does not grow
//!    it); the levels add, and each earlier error is passed on whole: `p γ_(3 p + 3) M ≤
//!    γ_(8 p² + 8) M` per coordinate, with room.
//!
//! The control points of `R` are then within `e = (1) + (3)` per coordinate, which moves a point
//! by at most `2 e` and so the spline by at most `2 e` (every point of `R` is a convex
//! combination of its control points). The bound is `(certified + 2 e + δ p D) (1 + γ₈)`.
//!
//! Non-rational 3D curves of degree 1 to [`MAX_DEGREE`], the boundary clamped, only.

use super::vec::VecExt;
use crate::math::Vec3;
use crate::nurbs::crv_basic::Curve as NlCurve;
use crate::nurbs::crv_deviation::{certified_distance, norm_up};
use crate::nurbs::knots::{Side, MAX_DEGREE};
use crate::nurbs::math::{gamma, CPoint, NOW};

/// The most points of the pcurve: a spline of the portable B-rep record has at most 4096
/// (`solidcore_record::brep`), the breakpoints at the boundary's knots included.
const MOST_POINTS: usize = 4096;
/// The most pieces of the map before the knots of the boundary are added to its breakpoints. The
/// straight pieces leave an error that falls with the square of their length and grows with the
/// boundary's speed (mm per unit of parameter), so a long boundary at an uneven pace needs
/// thousands: a 400 mm one with an edge at a rough pace takes about 3300 to bring the bound to
/// 2e-4 mm.
const MOST_PIECES: usize = 3900;
/// Pieces of the first, even, try.
const FIRST_PIECES: usize = 8;
/// Samples of the boundary per span for the first guess of a projection.
const PER_SPAN: usize = 24;
/// How many caps the edge may miss the boundary by at a sampled point before it is no boundary's.
const MISS: f64 = 4.0;
/// How far (in samples) either way a projection looks from where the map says it should be.
const WINDOW: usize = 3 * PER_SPAN;

/// The map `g` and the bound of the pcurve it gives.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Reparam {
    /// The edge's parameters `τ_k`, increasing, from the interval's start to its end.
    pub(super) params: Vec<f64>,
    /// The boundary's parameters `γ_k = g(τ_k)`, monotone (increasing, or decreasing where the
    /// edge runs against the boundary), from one end of the boundary to the other.
    pub(super) iso: Vec<f64>,
    /// A bound of the edge's distance from the lift of the pcurve at every parameter.
    pub(super) gap: f64,
}

thread_local! {
    /// The last few answers, by every number of their question: the importer asks the same one of
    /// an edge more than once (to raise its tolerance, to take the line, to check it).
    static ANSWERS: std::cell::RefCell<Vec<(Vec<u64>, Option<Reparam>)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// How many answers are kept.
const KEPT: usize = 16;

/// [`compute`], remembering the answer to this question: exactly the same numbers give exactly
/// the same answer, so a hit is no weaker than a new run (the key is every bit of the question,
/// not a hash of it).
pub(super) fn reparametrised(
    edge: &NlCurve,
    iso: &NlCurve,
    reversed: bool,
    interval: [f64; 2],
    cap: f64,
) -> Option<Reparam> {
    let mut key: Vec<u64> = vec![
        u64::from(reversed),
        cap.to_bits(),
        interval[0].to_bits(),
        interval[1].to_bits(),
    ];
    for c in [edge, iso] {
        key.push(c.degree() as u64);
        key.extend(c.knots().iter().map(|k| k.to_bits()));
        key.push(u64::MAX);
        key.extend(
            c.cpts()
                .iter()
                .flat_map(|p| [p.x, p.y, p.z, p.w])
                .map(f64::to_bits),
        );
        key.push(u64::MAX);
    }
    let hit = ANSWERS.with(|a| {
        a.borrow()
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.clone())
    });
    if let Some(answer) = hit {
        return answer;
    }
    let answer = compute(edge, iso, reversed, interval, cap);
    ANSWERS.with(|a| {
        let mut a = a.borrow_mut();
        if a.len() >= KEPT {
            a.clear();
        }
        a.push((key, answer.clone()));
    });
    answer
}

/// The proved map and gap for `edge` against the boundary curve `iso` over `[t0, t1]`, the
/// bound within `cap`, or `None`. `reversed` means the edge runs from the boundary's end to its
/// start. Both curves polynomial, the edge's knots exactly on `[t0, t1]`.
///
/// The map is refined where the straight piece between two of its points misses the projection of
/// the edge's midpoint by more than a target, which is made a tenth of the cap, then smaller,
/// while the bound keeps falling: it is the edge's own distance from the boundary, plus what the
/// straight pieces leave, which falls with the square of their length. A bound well past the cap
/// after the first try is the edge's own distance, and no refinement lowers it.
fn compute(
    edge: &NlCurve,
    iso: &NlCurve,
    reversed: bool,
    [t0, t1]: [f64; 2],
    cap: f64,
) -> Option<Reparam> {
    if !(cap.is_finite() && cap > 0.0 && t0.is_finite() && t1.is_finite() && t0 < t1) {
        return None;
    }
    if edge.cpts().iter().chain(iso.cpts()).any(|p| p.w != NOW) {
        return None;
    }
    let p = iso.degree();
    if p == 0 || p > MAX_DEGREE || edge.degree() == 0 || edge.degree() > MAX_DEGREE {
        return None;
    }
    let spans = Spans::of(iso)?;
    let samples = spans.samples(iso, reversed)?;
    let k = iso.knots();
    let (first, last) = (*k.first()?, *k.last()?);
    let (start, end) = if reversed {
        (last, first)
    } else {
        (first, last)
    };
    let mut params = vec![t0];
    let mut values = vec![start];
    let mut from = 0;
    for i in 1..FIRST_PIECES {
        let t = t0 + (t1 - t0) * (i as f64 / FIRST_PIECES as f64);
        if !(t > *params.last()? && t < t1) {
            continue;
        }
        let on_edge = point(edge, t)?;
        let (s, at) = nearest(iso, &samples, from..samples.len(), on_edge)?;
        from = at;
        // The edge's own distance from the boundary: no map and no refinement lowers it, so a
        // point of the edge well past the cap (with room for a search that settles short) is
        // no boundary's.
        if point(iso, s)?.dist(on_edge) > MISS * cap {
            return None;
        }
        let prev = *values.last()?;
        params.push(t);
        values.push(if reversed {
            s.min(prev).max(first)
        } else {
            s.max(prev).min(last)
        });
    }
    params.push(t1);
    values.push(end);
    let mut best: Option<Reparam> = None;
    for target in [cap / 10.0, cap / 100.0, cap / 1000.0, cap / 10_000.0] {
        refine(
            edge,
            iso,
            &samples,
            [first, last],
            [target, MISS * cap],
            !reversed,
            &mut params,
            &mut values,
        )?;
        let (mut ps, mut vs) = (params.clone(), values.clone());
        let found = cross_knots(iso, reversed, &mut ps, &mut vs)
            .filter(|()| ps.len() <= MOST_POINTS)
            .and_then(|()| certify(edge, iso, &spans, &ps, &vs, [t0, t1]));
        let Some(gap) = found else {
            return best.filter(|b| b.gap <= cap);
        };
        if best.is_none() && gap > 2.0 * cap {
            return None;
        }
        let improved = best.as_ref().is_none_or(|b| gap < 0.9 * b.gap);
        if improved {
            best = Some(Reparam {
                params: ps,
                iso: vs,
                gap,
            });
        } else {
            break;
        }
        if params.len() >= MOST_PIECES {
            break;
        }
    }
    best.filter(|b| b.gap <= cap)
}

/// Splits each piece of the map whose midpoint's projection is more than `target` mm (by the
/// boundary's speed) from the straight piece, until none is or there are [`MOST_PIECES`].
/// `None` where a projection fails.
#[allow(clippy::too_many_arguments)]
fn refine(
    edge: &NlCurve,
    iso: &NlCurve,
    samples: &[(f64, Vec3)],
    [first, last]: [f64; 2],
    [target, limit]: [f64; 2],
    increasing: bool,
    params: &mut Vec<f64>,
    values: &mut Vec<f64>,
) -> Option<()> {
    loop {
        let mut split: Vec<(usize, f64, f64)> = Vec::new();
        for i in 0..params.len().checked_sub(1)? {
            let (ta, tb) = (*params.get(i)?, *params.get(i + 1)?);
            let (ga, gb) = (*values.get(i)?, *values.get(i + 1)?);
            let tm = 0.5 * (ta + tb);
            if !(tm > ta && tm < tb) {
                continue;
            }
            let lin = 0.5 * (ga + gb);
            // The samples run monotonically in the boundary's parameter, either way.
            let hint = if increasing {
                samples.partition_point(|x| x.0 < lin)
            } else {
                samples.partition_point(|x| x.0 > lin)
            }
            .min(samples.len().saturating_sub(1));
            let window = hint.saturating_sub(WINDOW)..(hint + WINDOW).min(samples.len());
            let on_edge = point(edge, tm)?;
            let (s, _) = nearest(iso, samples, window, on_edge)?;
            let s = s.max(first).min(last);
            if point(iso, s)?.dist(on_edge) > limit {
                return None;
            }
            let speed = norm_up_of(derivs(iso, s)?[1])?;
            if (s - lin).abs() * speed > target {
                // Inside the piece's range, so it stays monotone.
                let s = s.max(ga.min(gb)).min(ga.max(gb));
                split.push((i, tm, s));
            }
        }
        if split.is_empty() || params.len() + split.len() > MOST_PIECES {
            return Some(());
        }
        let (mut new_params, mut new_values) = (Vec::new(), Vec::new());
        let mut pending = split.into_iter().peekable();
        for (i, (t, g)) in params.iter().zip(values.iter()).enumerate() {
            new_params.push(*t);
            new_values.push(*g);
            if let Some((_, tm, s)) = pending.next_if(|(at, _, _)| *at == i) {
                new_params.push(tm);
                new_values.push(s);
            }
        }
        *params = new_params;
        *values = new_values;
    }
}

fn norm_up_of(v: Vec3) -> Option<f64> {
    norm_up(v.x, v.y, v.z)
}

/// The spans of the boundary: its Bézier decomposition, and the size of its net.
struct Spans {
    /// Each span: its parameter range and its Bézier points.
    pieces: Vec<([f64; 2], Vec<Vec3>)>,
    /// The largest coordinate of the boundary's net.
    size: f64,
    /// The rounding of the decomposition, per coordinate (module docs, 1).
    decomposition: f64,
    /// The largest difference of two consecutive Bézier points of any span (`D`).
    largest_step: f64,
}

impl Spans {
    fn of(iso: &NlCurve) -> Option<Self> {
        let p = iso.degree();
        let size = iso.cpts().iter().fold(0.0_f64, |m, c| {
            m.max(c.x.abs()).max(c.y.abs()).max(c.z.abs())
        });
        if !size.is_finite() {
            return None;
        }
        let mut pieces = Vec::new();
        let mut largest_step = 0.0_f64;
        for piece in iso.decompose_bez().ok()? {
            let k = piece.knots();
            let range = [*k.first()?, *k.last()?];
            #[allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing
            if !(range[0] < range[1]) {
                continue;
            }
            let pts: Vec<Vec3> = piece
                .cpts()
                .iter()
                .map(|c| Vec3::new(c.x, c.y, c.z))
                .collect();
            if pts.len() != p + 1 {
                return None;
            }
            for pair in pts.windows(2) {
                let (a, b) = (pair.first()?, pair.get(1)?);
                largest_step = largest_step.max(norm_up(b.x - a.x, b.y - a.y, b.z - a.z)?);
            }
            pieces.push((range, pts));
        }
        if pieces.is_empty() {
            return None;
        }
        // Knots inserted: those of the pieces' joined knot vector past the boundary's.
        let joined = pieces
            .len()
            .saturating_sub(1)
            .saturating_mul(p)
            .saturating_add(2 * (p + 1));
        let inserted = joined.saturating_sub(iso.knots().len());
        let decomposition = gamma(inserted.saturating_add(1).saturating_mul(12)) * 2.0 * size;
        Some(Self {
            pieces,
            size,
            decomposition,
            largest_step,
        })
    }

    /// Points of the boundary by parameter, in the order of travel, for first guesses.
    fn samples(&self, iso: &NlCurve, reversed: bool) -> Option<Vec<(f64, Vec3)>> {
        let mut out = Vec::new();
        for ([a, b], _) in &self.pieces {
            for i in 0..PER_SPAN {
                let s = a + (b - a) * (i as f64 / PER_SPAN as f64);
                out.push((s, point(iso, s)?));
            }
        }
        let (last, _) = self.pieces.last()?;
        out.push((last[1], point(iso, last[1])?));
        if reversed {
            out.reverse();
        }
        Some(out)
    }
}

fn point(c: &NlCurve, u: f64) -> Option<Vec3> {
    c.derivs(u, Side::Left, 0).ok()?.first().copied()
}

/// The point and the first two derivatives of a curve at `u`.
fn derivs(c: &NlCurve, u: f64) -> Option<[Vec3; 3]> {
    let d = c.derivs(u, Side::Left, 2).ok()?;
    Some([*d.first()?, *d.get(1)?, *d.get(2)?])
}

fn dot(a: Vec3, b: Vec3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn minus(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x - b.x, a.y - b.y, a.z - b.z)
}

/// The boundary's parameter nearest to `target` among the samples of `window` (the map is
/// monotone), refined by Newton's method inside the neighbouring samples, and the sample reached.
fn nearest(
    iso: &NlCurve,
    samples: &[(f64, Vec3)],
    window: std::ops::Range<usize>,
    target: Vec3,
) -> Option<(f64, usize)> {
    let (mut best, mut at) = (f64::INFINITY, window.start);
    for (i, (_, q)) in samples
        .iter()
        .enumerate()
        .take(window.end)
        .skip(window.start)
    {
        let d = dot(minus(*q, target), minus(*q, target));
        if d < best {
            (best, at) = (d, i);
        }
    }
    let here = samples.get(at)?.0;
    let lo = samples.get(at.saturating_sub(1))?.0;
    let hi = samples.get(at + 1).map_or(here, |s| s.0);
    let (lo, hi) = (lo.min(hi).min(here), lo.max(hi).max(here));
    let mut s = here;
    for _ in 0..6 {
        let [b, d1, d2] = derivs(iso, s)?;
        let r = minus(b, target);
        let (f, df) = (dot(r, d1), dot(d1, d1) + dot(r, d2));
        if !(df > 0.0 && f.is_finite()) {
            break;
        }
        let next = (s - f / df).max(lo).min(hi);
        let done = (next - s).abs() <= 1e-15 * (1.0 + s.abs());
        s = next;
        if done {
            break;
        }
    }
    Some((s, at))
}

/// Breakpoints at the parameters where `g` passes an interior knot of the boundary, so that no
/// piece holds one: the point `(τ, κ)` on the segment of `g` that crosses `κ`. `None` where one
/// cannot be put strictly between the segment's ends.
fn cross_knots(
    iso: &NlCurve,
    reversed: bool,
    params: &mut Vec<f64>,
    values: &mut Vec<f64>,
) -> Option<()> {
    let mut knots: Vec<f64> = iso.knots().to_vec();
    knots.dedup();
    let (first, last) = (*knots.first()?, *knots.last()?);
    let interior: Vec<f64> = knots
        .into_iter()
        .filter(|k| *k > first && *k < last)
        .collect();
    let (mut new_params, mut new_values) = (Vec::new(), Vec::new());
    for (tw, gw) in params.windows(2).zip(values.windows(2)) {
        let (ta, tb) = (*tw.first()?, *tw.get(1)?);
        let (ga, gb) = (*gw.first()?, *gw.get(1)?);
        new_params.push(ta);
        new_values.push(ga);
        let (lo, hi) = (ga.min(gb), ga.max(gb));
        // The knots strictly inside the segment's range, in the order the segment meets them.
        let mut inside: Vec<f64> = interior
            .iter()
            .copied()
            .filter(|k| *k > lo && *k < hi)
            .collect();
        if reversed {
            inside.reverse();
        }
        let mut before = ta;
        for kappa in inside {
            let t = ta + (kappa - ga) / (gb - ga) * (tb - ta);
            if !(t > before && t < tb) {
                return None;
            }
            new_params.push(t);
            new_values.push(kappa);
            before = t;
        }
    }
    new_params.push(*params.last()?);
    new_values.push(*values.last()?);
    *params = new_params;
    *values = new_values;
    Some(())
}

/// The spline `R` of the pieces, and the bound (module docs).
fn certify(
    edge: &NlCurve,
    iso: &NlCurve,
    spans: &Spans,
    params: &[f64],
    values: &[f64],
    [t0, t1]: [f64; 2],
) -> Option<f64> {
    let p = iso.degree();
    let size = spans.size.max(edge.cpts().iter().fold(0.0_f64, |m, c| {
        m.max(c.x.abs()).max(c.y.abs()).max(c.z.abs())
    }));
    let delta = gamma(2);
    let mut net: Vec<CPoint> = Vec::new();
    let mut knots: Vec<f64> = vec![t0; p + 1];
    let count = params.len().checked_sub(1)?;
    for (k, (t, g)) in params.windows(2).zip(values.windows(2)).enumerate() {
        let (ga, gb) = (*g.first()?, *g.get(1)?);
        let (lo, hi) = (ga.min(gb), ga.max(gb));
        let (range, bez) = spans
            .pieces
            .iter()
            .find(|([a, b], _)| *a <= lo && hi <= *b)?;
        let width = range[1] - range[0];
        let local = |x: f64| ((x - range[0]) / width).clamp(0.0, 1.0);
        let (la, lb) = (local(ga), local(gb));
        let points = sub_range(bez, la, lb)?;
        if !points
            .iter()
            .all(|q| q.x.is_finite() && q.y.is_finite() && q.z.is_finite())
        {
            return None;
        }
        // The first point of a piece is the last of the one before.
        let skip = usize::from(!net.is_empty());
        for q in points.into_iter().skip(skip) {
            net.push(CPoint::new(q.x, q.y, q.z, NOW));
        }
        if k + 1 < count {
            knots.extend(std::iter::repeat_n(*t.get(1)?, p));
        }
    }
    knots.extend(std::iter::repeat_n(t1, p + 1));
    let r = NlCurve::new(net, p, knots).ok()?;
    let certified = certified_distance(edge, &r).ok()?;
    let sub = gamma(8 * p * p + 8) * size;
    let e = spans.decomposition + sub;
    let shift = delta * (p as f64) * spans.largest_step;
    let bound = (certified + 2.0 * e + shift) * (1.0 + gamma(8));
    bound.is_finite().then_some(bound)
}

/// The Bézier points of a Bézier curve's restriction to `[la, lb]` (local parameters, either
/// order: the points then run from `la` to `lb`), by the blossom at the ends.
fn sub_range(bez: &[Vec3], la: f64, lb: f64) -> Option<Vec<Vec3>> {
    let p = bez.len().checked_sub(1)?;
    let mut out = Vec::new();
    for i in 0..=p {
        let mut d: Vec<Vec3> = bez.to_vec();
        for r in 1..=p {
            // The first `p - i` levels at `la`, the last `i` at `lb`.
            let u = if r <= p - i { la } else { lb };
            for j in 0..=(p - r) {
                let (x, y) = (*d.get(j)?, *d.get(j + 1)?);
                let v = 1.0 - u;
                *d.get_mut(j)? = Vec3::new(v * x.x + u * y.x, v * x.y + u * y.y, v * x.z + u * y.z);
            }
        }
        out.push(*d.first()?);
    }
    Some(out)
}
