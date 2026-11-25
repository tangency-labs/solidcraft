//! A pcurve made by following the edge across its spline face, where the kernel's trims are far
//! off it.
//!
//! The kernel makes a spline face's trims by dropping each edge onto the surface. Where a cut
//! runs along a plane that stands (all but) tangent to the surface, as the flat end of a coil
//! spring wire that is cut where the wire's tube meets the plane of the turn above or below it,
//! the drop settles on another part of the surface for stretches of the edge, and the pcurve it
//! returns runs several millimetres off its edge (#20575). The edge is on the surface all the
//! same, and a nearest point found from where the edge was a moment before follows it: each point
//! of the edge is projected from the parameters the pcurve so far predicts, which stays on the
//! part of the surface the edge is on even where another part of it passes within a few
//! hundredths of a millimetre.
//!
//! Only a use whose pcurve lies [`FAR`] off its edge is made again, and only when the followed
//! pcurve lies nearer it. It is followed from whichever of the kernel's ends lies on the edge's
//! part of the surface, and its other end is put on its neighbour's; the loop's uses after it are
//! then moved by whole periods to run on from it ([`align`]). Each piece is a cubic Hermite spline through the followed parameters and the slopes the
//! surface's derivatives give (the edge's velocity taken into parameters by least squares),
//! halved until its middle lies no farther from the edge than the nearest point of the surface
//! does, within [`FIT`] (or than its ends do: the kernel's ends may lie a little off the edge).

use crate::kernel::{CurveEval, SurfaceEval};
use crate::vec::Vec3Geom as _;
use solidcore_ops::pcurve;
use solidcore_record::Vec3;
use solidcore_record::brep::{Curve2, Surface as BSurface};

use super::{EDGE_GAP_SAMPLES, EdgeB, MAX_SPLINE_POINTS, R};

/// How far (mm) a pcurve's surface lies off its edge before it is made again.
const FAR: f64 = 0.1;
/// How much farther (mm) than the surface's nearest point to the edge a piece's middle may lie.
const FIT: f64 = 1e-3;
/// The pieces the edge is first followed in.
const FIRST_PIECES: usize = 64;
/// The most nodes a followed pcurve has, and the most times a piece is halved.
const MAX_NODES: usize = 1024;
const MAX_DEPTH: u32 = 10;
/// The most times a step along the edge is halved.
const MAX_ADVANCE_DEPTH: u32 = 8;
/// The farthest (mm) the followed pcurve's end may lie from the kernel's.
const END_AGREE: f64 = 0.1;
/// How far (mm) a followed point may lie from where the pcurve so far predicts it: this much of
/// the distance from the last point of the edge, and this much more.
const REACH_SHARE: f64 = 0.5;
const REACH_MORE: f64 = 0.05;
/// The most steps a nearest point takes.
const NEWTON_STEPS: usize = 40;
/// The longest step (in the surface's parameters) a nearest point takes.
const LONGEST_STEP: f64 = 0.5;
/// The farthest (mm) a followed point may lie from the edge: the edge is on the surface to within
/// far less, so a nearest point farther off has settled on another part of it.
const NODE_OFF: f64 = 0.02;
/// How far (of the surface's width) the followed end may lie from the neighbour's in parameters.
const FAR_SHARE: f64 = 0.02;

/// A point of the followed pcurve: the edge's parameter, its parameters on the surface, the
/// slope of those in the edge's parameter, and how far (mm) the surface at them lies off the edge.
#[derive(Clone, Copy)]
struct Node {
    t: f64,
    q: [f64; 2],
    m: [f64; 2],
    off: f64,
}

/// `q` put back into the surface's box along each periodic direction, for evaluating. The followed
/// parameters themselves are not wrapped: they run on across a seam as one continuous pcurve.
fn fold(on: &SurfaceEval, q: [f64; 2]) -> [f64; 2] {
    let domain = on.domain();
    let mut out = q;
    for ((x, range), period) in out.iter_mut().zip(domain.range).zip(domain.period) {
        if let (Some((lo, _)), Some(p)) = (range, period)
            && p > 0.0
        {
            *x = lo + (*x - lo).rem_euclid(p);
        }
    }
    out
}

/// The point of the surface at `q`, folded ([`fold`]).
fn at(on: &SurfaceEval, q: [f64; 2]) -> Option<Vec3> {
    on.point(fold(on, q))
}

/// How far the surface at `pc` lies off the edge, over the samples the record check takes.
fn gap(on: &SurfaceEval, curve: &CurveEval, pc: &Curve2, [t0, t1]: [f64; 2]) -> f64 {
    let mut worst = 0.0f64;
    for k in 0..=EDGE_GAP_SAMPLES {
        let t = if k == EDGE_GAP_SAMPLES { t1 } else { t0 + (t1 - t0) * (k as f64 / EDGE_GAP_SAMPLES as f64) };
        if let (Some(q), Some(p)) = (pcurve::eval(pc, t), curve.point(t))
            && let Some(s) = at(on, q)
        {
            worst = worst.max(s.dist(p));
        }
    }
    worst
}

/// The nearest point of the surface to `p` found from the parameters `from` by Gauss-Newton steps
/// (each at most [`LONGEST_STEP`] long, kept in the surface's box), and how far it lies from `p`:
/// the one nearest `from`, not the nearest of the whole surface.
fn near(on: &SurfaceEval, p: Vec3, from: [f64; 2]) -> Option<([f64; 2], f64)> {
    let domain = on.domain();
    let mut q = from;
    for _ in 0..NEWTON_STEPS {
        let f = fold(on, q);
        let (s, su, sv) = on.derivs(f[0], f[1])?;
        let r = p - s;
        let (a, b, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
        let det = a * c - b * b;
        if !(det > 1e-12 * a * c) {
            return None;
        }
        let (g0, g1) = (su.dot(r), sv.dot(r));
        let mut step = [(c * g0 - b * g1) / det, (a * g1 - b * g0) / det];
        let length = step[0].hypot(step[1]);
        if !length.is_finite() {
            return None;
        }
        if length > LONGEST_STEP {
            step = [step[0] * LONGEST_STEP / length, step[1] * LONGEST_STEP / length];
        }
        let mut next = [q[0] + step[0], q[1] + step[1]];
        // A periodic direction runs on across its seam (folded for evaluating); the others stay in
        // their box.
        for ((x, range), period) in next.iter_mut().zip(domain.range).zip(domain.period) {
            if let (Some((lo, hi)), None) = (range, period) {
                *x = x.clamp(lo, hi);
            }
        }
        let moved = (next[0] - q[0]).hypot(next[1] - q[1]);
        q = next;
        if moved < 1e-12 {
            break;
        }
    }
    let s = at(on, q)?;
    Some((q, s.dist(p)))
}

/// The slope in the edge's parameter, at `t`, of the parameters `q` of its points.
fn slope(on: &SurfaceEval, curve: &CurveEval, t: f64, q: [f64; 2]) -> Option<[f64; 2]> {
    let (_, d) = curve.derivs(t)?;
    let f = fold(on, q);
    let (_, su, sv) = on.derivs(f[0], f[1])?;
    let (a, b, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
    let det = a * c - b * b;
    if !(det > 1e-12 * a * c) {
        return None;
    }
    let (g0, g1) = (su.dot(d), sv.dot(d));
    let m = [(c * g0 - b * g1) / det, (a * g1 - b * g0) / det];
    (m[0].is_finite() && m[1].is_finite()).then_some(m)
}

/// The cubic Hermite value at `s` in `[0, 1]` from values `p` and slopes `m` (per unit of `s`).
fn hermite(p: [f64; 2], m: [f64; 2], s: f64) -> f64 {
    let (s2, s3) = (s * s, s * s * s);
    (2.0 * s3 - 3.0 * s2 + 1.0) * p[0] + (s3 - 2.0 * s2 + s) * m[0] + (-2.0 * s3 + 3.0 * s2) * p[1] + (s3 - s2) * m[1]
}

/// The cubic Hermite parameters at `s` between nodes `a` and `b`.
fn between(a: &Node, b: &Node, s: f64) -> [f64; 2] {
    let h = b.t - a.t;
    [hermite([a.q[0], b.q[0]], [h * a.m[0], h * b.m[0]], s), hermite([a.q[1], b.q[1]], [h * a.m[1], h * b.m[1]], s)]
}

/// The surface and the edge the pcurve is followed along.
struct Walk<'s> {
    on: &'s SurfaceEval,
    curve: &'s CurveEval,
}

impl Walk<'_> {
    /// The node at `t`, found from the parameters `hint`, which its point must lie within `reach`
    /// mm of on the surface.
    fn node(&self, t: f64, hint: [f64; 2], reach: f64) -> Option<Node> {
        let p = self.curve.point(t)?;
        let (q, off) = near(self.on, p, hint)?;
        if off > NODE_OFF || at(self.on, q)?.dist(at(self.on, hint)?) > reach {
            return None;
        }
        let m = slope(self.on, self.curve, t, q)?;
        Some(Node { t, q, m, off })
    }

    /// The nodes from `prev` on to the edge's parameter `t` (at least the one at `t`): taken in one
    /// step from where the slope predicts, or in halves where that lies too far off the part of
    /// the surface the edge is on.
    fn advance(&self, prev: Node, t: f64, depth: u32) -> Option<Vec<Node>> {
        let h = t - prev.t;
        let hint = [prev.q[0] + h * prev.m[0], prev.q[1] + h * prev.m[1]];
        let reach = REACH_SHARE * self.curve.point(prev.t)?.dist(self.curve.point(t)?) + REACH_MORE;
        if let Some(node) = self.node(t, hint, reach) {
            return Some(vec![node]);
        }
        if depth >= MAX_ADVANCE_DEPTH {
            return None;
        }
        let mid = prev.t + 0.5 * h;
        let mut first = self.advance(prev, mid, depth + 1)?;
        let from = *first.last()?;
        first.extend(self.advance(from, t, depth + 1)?);
        Some(first)
    }

    /// The nodes after `a` up to and with `b`, halving until each piece fits.
    fn pieces(&self, a: Node, b: Node, depth: u32, out: &mut Vec<Node>) -> Option<()> {
        let h = b.t - a.t;
        let mut fits = true;
        for s in [0.25, 0.5, 0.75] {
            let t = a.t + s * h;
            let hq = between(&a, &b, s);
            let p = self.curve.point(t)?;
            let on = at(self.on, hq)?.dist(p);
            let nearest = near(self.on, p, hq).map_or(on, |(_, d)| d);
            if on > nearest.max(a.off).max(b.off) + FIT {
                fits = false;
                break;
            }
        }
        if fits {
            out.push(b);
            return Some(());
        }
        if depth >= MAX_DEPTH || out.len() >= MAX_NODES {
            return None;
        }
        let mid = a.t + 0.5 * h;
        let reach = REACH_SHARE * self.curve.point(a.t)?.dist(self.curve.point(b.t)?) + REACH_MORE;
        let node = self.node(mid, between(&a, &b, 0.5), reach)?;
        self.pieces(a, node, depth + 1, out)?;
        self.pieces(node, b, depth + 1, out)
    }
}

/// The pcurve of the edge on `curve` over `interval`, followed along the surface from the start
/// of `seed` (the kernel's), or `None` where the edge cannot be followed or ends elsewhere than
/// `seed` does.
fn followed(on: &SurfaceEval, curve: &CurveEval, interval: [f64; 2], seed: &Curve2, meets: [Option<[f64; 2]>; 2], backward: bool) -> Option<Curve2> {
    let [t0, t1] = interval;
    if !(t1 > t0) {
        return None;
    }
    let walk = Walk { on, curve };
    // The edge is followed from the end of the kernel's pcurve that is on the right part of the
    // surface: tried from its start, then from its end (the start may lie on the wrong one of two
    // sheets a few hundredths of a millimetre apart).
    let (anchor_t, far_t, sign) = if backward { (t1, t0, -1.0) } else { (t0, t1, 1.0) };
    let q0 = pcurve::eval(seed, anchor_t)?;
    let first = Node { t: anchor_t, q: q0, m: slope(on, curve, anchor_t, q0)?, off: at(on, q0)?.dist(curve.point(anchor_t)?) };
    let mut coarse = vec![first];
    let mut prev = first;
    for i in 1..=FIRST_PIECES {
        let t = if i == FIRST_PIECES { far_t } else { anchor_t + sign * (t1 - t0) * (i as f64 / FIRST_PIECES as f64) };
        let more = walk.advance(prev, t, 0)?;
        for n in more {
            prev = n;
            coarse.push(n);
        }
    }
    // The far end meets the pcurve of its neighbour in the loop (the kernel's own, where the loop
    // has no other use): on the same sheet and the same turn (a followed end a period away would
    // leave a jump across a seam that the loop has no edge for). The kernel's own end may be the
    // wrong one: it is where the trim starts on another sheet.
    let far = match meets[usize::from(!backward)] {
        Some(q) => q,
        None => pcurve::eval(seed, far_t)?,
    };
    let mut end = *coarse.last()?;
    // On a closed surface the neighbour's end may be the same point a whole period away from the
    // followed one (the followed pcurve may run round the other way from the kernel's): it is
    // moved by whole periods onto the followed end, and the loop's uses are then moved by whole
    // periods to run on from one another ([`align`]).
    let domain = on.domain();
    let mut far = far;
    for ((x, y), (p, range)) in far.iter_mut().zip(end.q).zip(domain.period.into_iter().zip(domain.range)) {
        if let Some(p) = p.filter(|p| *p > 0.0) {
            *x += ((y - *x) / p).round() * p;
        }
        // On the same sheet: the parameters, not only the points, lie close.
        if let Some((lo, hi)) = range
            && (*x - y).abs() > FAR_SHARE * (hi - lo)
        {
            return None;
        }
    }
    if at(on, far)?.dist(at(on, end.q)?) > END_AGREE {
        return None;
    }
    end.q = far;
    end.m = slope(on, curve, far_t, far)?;
    end.off = at(on, far)?.dist(curve.point(far_t)?);
    if let Some(slot) = coarse.last_mut() {
        *slot = end;
    }
    if backward {
        coarse.reverse();
    }
    let first = *coarse.first()?;
    let mut nodes = vec![first];
    for pair in coarse.windows(2) {
        let (a, b) = (*pair.first()?, *pair.get(1)?);
        walk.pieces(a, b, 0, &mut nodes)?;
    }
    // A cubic Hermite piece from each pair of nodes, as a spline through them.
    let mut knots = vec![t0; 4];
    let mut points = vec![first.q];
    for (k, pair) in nodes.windows(2).enumerate() {
        let (p, q) = (*pair.first()?, *pair.get(1)?);
        let h = q.t - p.t;
        points.push([p.q[0] + h * p.m[0] / 3.0, p.q[1] + h * p.m[1] / 3.0]);
        points.push([q.q[0] - h * q.m[0] / 3.0, q.q[1] - h * q.m[1] / 3.0]);
        if k + 2 < nodes.len() {
            knots.extend([q.t; 2]);
        } else {
            points.push(q.q);
        }
    }
    knots.extend([t1; 4]);
    if points.len() > MAX_SPLINE_POINTS || points.iter().flatten().any(|x| !x.is_finite()) {
        return None;
    }
    Some(Curve2::Spline { knots, points })
}

/// Where use `j` of a loop starts in the loop's direction, or ends (`end`).
fn use_end(l: &[(u32, bool)], pcs: &[Curve2], j: usize, end: bool) -> Option<[f64; 2]> {
    let &(_, forward) = l.get(j)?;
    let c = pcs.get(j)?;
    let [a, b] = c.interval()?;
    pcurve::eval(c, if forward == end { b } else { a })
}

/// `c` moved by `by` along the surface's parameter `d`.
fn shift(c: &mut Curve2, d: usize, by: f64) {
    match c {
        Curve2::Spline { points, .. } => {
            for x in points.iter_mut().filter_map(|p| p.get_mut(d)) {
                *x += by;
            }
        }
        Curve2::RationalSpline { homogeneous, .. } => {
            for h in homogeneous {
                let w = h[2];
                if let Some(x) = h.get_mut(d) {
                    *x += by * w;
                }
            }
        }
    }
}

/// The uses of a loop from `k` (made again) on, each moved by whole periods to start where the use
/// before it ends, up to the first after `k` that already does. A followed pcurve may run round a
/// closed surface the other way from the kernel's (#20575: the kernel's trim started on another
/// sheet and swept a quarter turn one way, the edge sweeps three quarters the other), so it ends
/// on the kernel's point a period away from where the kernel's ended, and the uses after it (the
/// seam edge's, made to run on from the kernel's trim) are a period off it. The uses before `k`,
/// and the loop of any face whose trims were not made again, are left as they are.
///
/// # Errors
/// A use whose pcurve cannot be evaluated at its ends.
fn align(on: &SurfaceEval, l: &[(u32, bool)], pcs: &mut [Curve2], k: usize) -> R<()> {
    let n = l.len();
    let periods = on.domain().period;
    for i in 0..n.saturating_sub(1) {
        let j = (k + i) % n;
        let before = use_end(l, pcs, (j + n - 1) % n, true).ok_or("pcurve end")?;
        let start = use_end(l, pcs, j, false).ok_or("pcurve start")?;
        let mut moved = false;
        for (d, ((b, s), period)) in before.into_iter().zip(start).zip(periods).enumerate() {
            if let Some(p) = period.filter(|p| *p > 0.0)
                && (b - s).abs() > 0.5 * p
            {
                shift(pcs.get_mut(j).ok_or("pcurve")?, d, ((b - s) / p).round() * p);
                moved = true;
            }
        }
        if i > 0 && !moved {
            break;
        }
    }
    Ok(())
}

/// The pcurves of a spline face's uses (`pcs`, in the order of `edge_loops`) that lie [`FAR`] off
/// their edges, made again where following the edge brings them nearer it: how many were.
///
/// # Errors
/// An edge or surface that cannot be evaluated.
pub(super) fn retrack(surface: &BSurface, edges: &[EdgeB], edge_loops: &[Vec<(u32, bool)>], pcs: &mut [Vec<Curve2>]) -> R<usize> {
    let on = SurfaceEval::new(surface).map_err(|m| m.to_string())?;
    let mut made = 0;
    for (l, loop_pcs) in edge_loops.iter().zip(pcs.iter_mut()) {
        let n = l.len();
        for k in 0..n {
            let (&(e, forward), Some(pc)) = (l.get(k).ok_or("edge use")?, loop_pcs.get(k).cloned()) else {
                continue;
            };
            let edge = edges.get(e as usize).ok_or("edge reference")?;
            let (curve, interval) = match &edge.form {
                Some((c, i)) => (c, *i),
                None => (&edge.curve, edge.interval),
            };
            let eval = CurveEval::new(curve).map_err(|m| m.to_string())?;
            let off = gap(&on, &eval, &pc, interval);
            if !(off > FAR) {
                continue;
            }
            // Where the uses next to this one end and start, as ends of this pcurve.
            let meets = if n < 2 {
                [None, None]
            } else {
                let (before, after) = (use_end(l, loop_pcs, (k + n - 1) % n, true), use_end(l, loop_pcs, (k + 1) % n, false));
                if forward { [before, after] } else { [after, before] }
            };
            let mut best: Option<(f64, Curve2)> = None;
            for backward in [false, true] {
                if let Some(again) = followed(&on, &eval, interval, &pc, meets, backward) {
                    let g = gap(&on, &eval, &again, interval);
                    if g < off && best.as_ref().is_none_or(|(b, _)| g < *b) {
                        best = Some((g, again));
                    }
                }
            }
            if let Some((_, again)) = best
                && let Some(slot) = loop_pcs.get_mut(k)
            {
                *slot = again;
                made += 1;
                align(&on, l, loop_pcs, k)?;
            }
        }
    }
    Ok(made)
}
