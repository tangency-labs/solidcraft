//! Edge nodes: every edge of a body sampled once, so the faces on either side share the very
//! same boundary points (watertight by construction).

use std::cmp::Ordering;

use crate::vec::VecExt as _;
use solidcore::eval::CurveEval;
use solidcore_convert as kernel;
use solidcore_record::Vec3;
use solidcore_record::brep::{self, Curve};

use super::size::Rules;
use crate::{Result, TessError};

/// Most nodes on one edge.
pub(crate) const MAX_EDGE_NODES: usize = 20_000;
/// Deepest bisection of one edge segment.
const MAX_DEPTH: u32 = 24;
/// The most of its curve's turn one segment may span (a quarter turn, with rounding room): the
/// boundary of every face stays a polygon with the face's area, as a closed curve's first four
/// segments keep it.
pub(crate) const QUARTER_TURN: f64 = std::f64::consts::FRAC_PI_2 * (1.0 + 1e-9);

/// An edge's nodes, from its start vertex (interval min) to its end vertex (interval max).
/// None (the default) for an edge that could not be sampled: every face it bounds is refused.
#[derive(Default)]
pub(crate) struct Nodes {
    pub points: Vec<Vec3>,
    /// The curve's parameter at each node (the edge's interval, ends included): where each
    /// face's pcurve of the edge gives the node's (u, v).
    pub params: Vec<f64>,
    /// The parameters of the nodes that are another edge's node, where the two edges touch
    /// within the chord tolerance (see [`snap`]): in a face that runs both edges such a node is
    /// the other's point of the face, a pinch, not a crossing.
    pub snapped: Vec<f64>,
    /// Where [`sample_edge`] stopped at a bound with a segment still wanting a split: which
    /// bound ([`crate::EdgeCap::Nodes`] where both were met), the farthest any of its chords lies
    /// from the curve at their quarter points and middles (mm; a sampled lower bound, exact
    /// where a chord's farthest point is its middle, as on a circle), and the most any chord
    /// exceeds its rules (the largest of its sag over the tolerance, its length over the side
    /// asked, its turn over a quarter turn and over the facet angle; 1 at the rule).
    pub capped: Option<(crate::EdgeCap, f64, f64)>,
}

/// A segment of [`sample_edge`]'s bisection, measured: its parameters, end points, quarter
/// points and middle, depth, whether it is fine, its chord's sag, and by how much it exceeds the
/// rules it fails (0 when fine). Each segment carries its points, so every parameter is
/// evaluated once (an intersection curve's point is a Newton solve).
struct Seg {
    a: f64,
    b: f64,
    pa: Vec3,
    pb: Vec3,
    q1: Vec3,
    pm: Vec3,
    q3: Vec3,
    depth: u32,
    ok: bool,
    sag: f64,
    excess: f64,
}

impl Seg {
    /// Which of two segments is halved first: the one exceeding its rules more, then the one
    /// earlier along the curve.
    fn order(&self, other: &Seg) -> Ordering {
        self.excess
            .total_cmp(&other.excess)
            .then_with(|| other.a.total_cmp(&self.a))
    }
}

impl PartialEq for Seg {
    fn eq(&self, other: &Self) -> bool {
        self.order(other) == Ordering::Equal
    }
}

impl Eq for Seg {}

impl PartialOrd for Seg {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Seg {
    fn cmp(&self, other: &Self) -> Ordering {
        self.order(other)
    }
}

/// Nodes of edge `ei` of `b` from its start to its end: every chord within the tolerance of
/// the curve (checked at the quarter points, so an S-bend in one segment is not missed), at
/// most `h` long, within the rules' facet angle and never spanning more than a quarter turn of
/// the curve. The end nodes are the vertices' own points, so edges meeting at a vertex meet
/// exactly. A curve that does not evaluate somewhere on its interval is an error naming the
/// edge.
pub(crate) fn sample_edge(b: &brep::Brep, ei: usize, rules: &Rules, h: f64) -> Result<Nodes> {
    let fail = |why: String| TessError::Failed(format!("tessellation: edge {ei}: {why}"));
    let edge = b.edges.get(ei).ok_or_else(|| fail("no such edge".into()))?;
    let vertex = |k: usize| {
        let v = edge
            .vertices
            .get(k)
            .and_then(|&v| b.vertices.get(v as usize))
            .ok_or_else(|| fail("no such vertex".into()))?;
        Ok::<Vec3, TessError>(Vec3::new(v.point[0], v.point[1], v.point[2]))
    };
    let (front, back) = (vertex(0)?, vertex(1)?);
    let tol = rules.tol;
    let [t0, t1] = edge.interval;
    if !(t0.is_finite() && t1.is_finite() && t1 > t0) {
        return Err(fail(format!("interval [{t0}, {t1}]")));
    }
    if matches!(edge.curve, Curve::Line { .. }) {
        // Evenly, as few segments as keep each at most `h`. The ends are the vertices' own
        // points: `front + (back − front) · 1` is not `back` to the last bit, and the face that
        // runs the edge backwards starts its loop with that node, where the face running it
        // forwards takes the next edge's first node (the vertex) — a crack.
        // A vertex not a number (or the two too far apart for a length) has no nodes between:
        // the count below would round it to none.
        let length = front.dist(back);
        if !length.is_finite() {
            return Err(fail(format!(
                "its vertices {front:?} and {back:?} are {length} apart"
            )));
        }
        let wanted = if h > 0.0 { (length / h).ceil() } else { 1.0 };
        // Proven: clamp bounds are constants 1.0 <= 19 999.0, neither NaN
        let n = wanted.clamp(1.0, (MAX_EDGE_NODES - 1) as f64) as usize;
        // More segments than an edge may have: its sides are longer than asked (a line's chords
        // lie on it, so none sags).
        let capped = (wanted > (MAX_EDGE_NODES - 1) as f64)
            .then(|| (crate::EdgeCap::Nodes, 0.0, length / n as f64 / h));
        let mut pts: Vec<Vec3> = (0..=n)
            .map(|i| front + (back - front) * (i as f64 / n as f64))
            .collect();
        let mut params: Vec<f64> = (0..=n)
            .map(|i| t0 + (t1 - t0) * (i as f64 / n as f64))
            .collect();
        if let (Some(p), Some(t)) = (pts.last_mut(), params.last_mut()) {
            *p = back;
            *t = t1;
        }
        return Ok(Nodes {
            points: pts,
            params,
            snapped: Vec::new(),
            capped,
        });
    }
    let curve = CurveEval::new(&kernel::curve(edge.parameters()))
        .map_err(|e| fail(crate::eval_message(e)))?;
    let at = |t: f64| -> Result<Vec3> {
        curve
            .point(t)
            .map(kernel::record_vec3)
            .filter(|p| p.is_finite())
            .ok_or_else(|| fail(format!("the curve does not evaluate at t = {t}")))
    };
    // A closed curve's chord between its ends is empty: start it from a few segments. An open
    // one starts whole, so an arc the tolerance leaves flat stays one segment.
    let initial: u32 = if edge.vertices[0] == edge.vertices[1] {
        4
    } else {
        1
    };
    let t_at = |i: u32| t0 + (t1 - t0) * f64::from(i) / f64::from(initial);
    let ends: Vec<Vec3> = (0..=initial).map(|i| at(t_at(i))).collect::<Result<_>>()?;
    // The segment from `a` to `b` (points `pa`, `pb`, its middle `pm` where known), measured.
    let measure = |a: f64, b: f64, pa: Vec3, pb: Vec3, pm: Option<Vec3>, depth: u32| {
        let pm = match pm {
            Some(p) => p,
            None => at(0.5 * (a + b))?,
        };
        let (q1, q3) = (at(a + 0.25 * (b - a))?, at(a + 0.75 * (b - a))?);
        let sag = [q1, pm, q3]
            .iter()
            .map(|q| q.dist_to_segment(pa, pb))
            .fold(0.0, f64::max);
        let len = pa.dist(pb);
        let short = len.partial_cmp(&(h * (1.0 + 1e-9))) != Some(Ordering::Greater);
        // The two half chords of an arc turning θ differ by θ/2.
        let bend = 2.0 * turn(pm - pa, pb - pm);
        let straight = rules.turn.is_none_or(|t| !t.exceeds(bend, sag));
        // Whatever the tolerance, no segment spans more than a quarter turn of its curve: a
        // small circle split into two half arcs would otherwise be two chords on one line, a
        // hole of no area (the 1 mm hole of a 990 mm plate at its 2 mm chord).
        let quarter = bend <= QUARTER_TURN;
        let ok = short && straight && quarter && sag <= tol;
        // How far past its rules a segment that fails them is: the most of each rule's ratio.
        let excess = if ok {
            0.0
        } else {
            let mut x = (sag / tol).max(bend / QUARTER_TURN);
            if h > 0.0 {
                x = x.max(len / h);
            }
            if let Some(t) = rules.turn.filter(|t| t.exceeds(bend, sag)) {
                x = x.max(bend / t.angle);
            }
            x
        };
        Ok::<Seg, TessError>(Seg {
            a,
            b,
            pa,
            pb,
            q1,
            pm,
            q3,
            depth,
            ok,
            sag,
            excess,
        })
    };
    // Bisection, the segment exceeding its rules most first (then the earliest): a segment is
    // halved where it fails the rules, whatever the order, so the nodes are the same as any
    // order's; where MAX_EDGE_NODES stops it the segments kept whole are those that exceed their
    // rules least, and the error is spread along the edge. (MAX_DEPTH stops a segment on its own,
    // usually the one exceeding its rules most: a corner no halving reaches.)
    let mut heap: std::collections::BinaryHeap<Seg> = std::collections::BinaryHeap::new();
    // Proven: the window size is the constant 2, not zero.
    for (i, w) in ends.windows(2).enumerate() {
        let (Some(&pa), Some(&pb)) = (w.first(), w.get(1)) else {
            continue;
        };
        // Proven: i < initial (at most 4).
        let i = i as u32;
        heap.push(measure(t_at(i), t_at(i + 1), pa, pb, None, 0)?);
    }
    // The segments kept: their end parameter and point; the most any sags, and exceeds its rules.
    let mut kept: Vec<(f64, Vec3)> = Vec::new();
    let (mut most_sag, mut most_excess) = (0.0f64, 0.0f64);
    let mut capped: Option<crate::EdgeCap> = None;
    while let Some(sg) = heap.pop() {
        let deep = sg.depth >= MAX_DEPTH;
        // The edge's nodes are its start and the ends of the segments kept, pending and this
        // one (2 + kept + pending): halving this one adds a node, past the bound once they
        // number MAX_EDGE_NODES.
        let full = 2 + kept.len() + heap.len() >= MAX_EDGE_NODES;
        if !deep && !full && !sg.ok {
            let m = 0.5 * (sg.a + sg.b);
            heap.push(measure(sg.a, m, sg.pa, sg.pm, Some(sg.q1), sg.depth + 1)?);
            heap.push(measure(m, sg.b, sg.pm, sg.pb, Some(sg.q3), sg.depth + 1)?);
        } else {
            // A segment a bound keeps whole although it wants a split: the edge is named
            // ([`crate::PlanStats::edges_capped`]).
            if !sg.ok && capped != Some(crate::EdgeCap::Nodes) {
                capped = Some(if full {
                    crate::EdgeCap::Nodes
                } else {
                    crate::EdgeCap::Depth
                });
            }
            most_sag = most_sag.max(sg.sag);
            most_excess = most_excess.max(sg.excess);
            kept.push((sg.b, sg.pb));
        }
    }
    kept.sort_by(|x, y| x.0.total_cmp(&y.0));
    let capped = capped.map(|cap| (cap, most_sag, most_excess));
    let mut out: Vec<(f64, Vec3)> = std::iter::once((t0, front)).chain(kept).collect();
    if let Some(p) = out.last_mut() {
        *p = (t1, back);
    }
    Ok(Nodes {
        points: out.iter().map(|p| p.1).collect(),
        params: out.iter().map(|p| p.0).collect(),
        snapped: Vec::new(),
        capped,
    })
}

/// Edge `ei`'s nodes with each of its segments `segs` (counted from its start) halved at the
/// middle of its curve parameters, the new node on the curve (on a line, halfway between the two,
/// as [`sample_edge`] spaces a line's nodes). A segment too short to halve in floating point keeps
/// its two nodes. More than [`MAX_EDGE_NODES`] nodes, or a curve that does not evaluate there, is
/// an error naming the edge.
pub(crate) fn split_segments(
    b: &brep::Brep,
    ei: usize,
    nodes: &mut Nodes,
    segs: &[u32],
) -> Result<()> {
    let fail = |why: String| TessError::Failed(format!("tessellation: edge {ei}: {why}"));
    let edge = b.edges.get(ei).ok_or_else(|| fail("no such edge".into()))?;
    let mut segs = segs.to_vec();
    segs.sort_unstable();
    segs.dedup();
    let n = nodes.points.len().min(nodes.params.len());
    if n + segs.len() > MAX_EDGE_NODES {
        return Err(fail(format!("more than {MAX_EDGE_NODES} nodes")));
    }
    let curve = match edge.curve {
        Curve::Line { .. } => None,
        _ => Some(
            CurveEval::new(&kernel::curve(edge.parameters()))
                .map_err(|e| fail(crate::eval_message(e)))?,
        ),
    };
    // Proven: n + segs.len() <= MAX_EDGE_NODES = 20 000, checked above
    let (mut points, mut params) = (
        Vec::with_capacity(n + segs.len()),
        Vec::with_capacity(n + segs.len()),
    );
    let mut next = segs.iter().copied().peekable();
    for k in 0..n {
        let (Some(&p), Some(&t)) = (nodes.points.get(k), nodes.params.get(k)) else {
            continue;
        };
        points.push(p);
        params.push(t);
        // Proven: k < n <= MAX_EDGE_NODES (20 000): it fits a u32.
        if next.next_if_eq(&(k as u32)).is_none() {
            continue;
        }
        let (Some(&q), Some(&u)) = (nodes.points.get(k + 1), nodes.params.get(k + 1)) else {
            continue;
        };
        let m = 0.5 * (t + u);
        if !(m > t.min(u) && m < t.max(u)) {
            continue;
        }
        let pm = match &curve {
            None => (p + q) * 0.5,
            Some(c) => c
                .point(m)
                .map(kernel::record_vec3)
                .filter(|p| p.is_finite())
                .ok_or_else(|| fail(format!("the curve does not evaluate at t = {m}")))?,
        };
        points.push(pm);
        params.push(m);
    }
    nodes.points = points;
    nodes.params = params;
    Ok(())
}

/// Edge `ei`'s nodes with `add` (each a parameter strictly inside a segment and the point there)
/// put in, in order, each marked as snapped: the point is another edge's node within the chord
/// tolerance of this edge's chord there (the edges touch), so the two edges share it. A parameter
/// that is not strictly between its segment's two (a node already there) is left out. More than
/// [`MAX_EDGE_NODES`] nodes is an error naming the edge.
pub(crate) fn snap(ei: usize, nodes: &mut Nodes, add: &[(f64, Vec3)]) -> Result<()> {
    let mut add: Vec<(f64, Vec3)> = add
        .iter()
        .copied()
        .filter(|(t, p)| t.is_finite() && p.is_finite())
        .collect();
    add.sort_by(|a, b| a.0.total_cmp(&b.0));
    add.dedup_by(|a, b| a.0 == b.0);
    if nodes.points.len() + add.len() > MAX_EDGE_NODES {
        return Err(TessError::Failed(format!(
            "tessellation: edge {ei}: more than {MAX_EDGE_NODES} nodes"
        )));
    }
    let n = nodes.points.len().min(nodes.params.len());
    let (mut points, mut params) = (
        // Proven: n + add.len() <= MAX_EDGE_NODES = 20 000, checked above
        Vec::with_capacity(n + add.len()),
        Vec::with_capacity(n + add.len()),
    );
    let mut next = add.iter().peekable();
    for k in 0..n {
        let (Some(&p), Some(&t)) = (nodes.points.get(k), nodes.params.get(k)) else {
            continue;
        };
        points.push(p);
        params.push(t);
        let Some(&u) = nodes.params.get(k + 1) else {
            continue;
        };
        while let Some(&&(m, q)) = next.peek() {
            if m >= u {
                break;
            }
            next.next();
            if m > t {
                points.push(q);
                params.push(m);
                nodes.snapped.push(m);
            }
        }
    }
    nodes.points = points;
    nodes.params = params;
    Ok(())
}

/// Angle (radians) between two directions; zero when either is empty.
pub(crate) fn turn(a: Vec3, b: Vec3) -> f64 {
    let (la, lb) = (a.len(), b.len());
    if !(la > 0.0 && lb > 0.0) {
        return 0.0;
    }
    // Proven: the clamp bounds are the constants -1.0 <= 1.0, neither NaN.
    (a.dot(b) / (la * lb)).clamp(-1.0, 1.0).acos()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::planner::tests::fixture;

    fn sample(b: &brep::Brep, ei: usize, rules: &Rules, h: f64) -> Nodes {
        match sample_edge(b, ei, rules, h) {
            Ok(n) => n,
            Err(e) => panic!("{e}"),
        }
    }

    fn ends(b: &brep::Brep, ei: usize) -> [Vec3; 2] {
        b.edges[ei].vertices.map(|v| {
            let p = b.vertices[v as usize].point;
            Vec3::new(p[0], p[1], p[2])
        })
    }

    /// A line edge whose vertex is not a number is refused, naming the edge: its length is not a
    /// number, and the node count it gave was none (a polyline of one point).
    #[test]
    fn a_line_edge_with_a_vertex_not_a_number_is_refused() {
        let mut cyl = fixture("cylinder_r10_h5");
        let Some(line) = cyl
            .edges
            .iter()
            .position(|e| matches!(e.curve, Curve::Line { .. }))
        else {
            panic!("a line edge")
        };
        let v = cyl.edges[line].vertices[0] as usize;
        cyl.vertices[v].point[0] = f64::NAN;
        let rules = Rules {
            tol: 0.01,
            max_len: f64::INFINITY,
            turn: None,
        };
        match sample_edge(&cyl, line, &rules, 1.0) {
            Err(e) => assert!(
                e.to_string()
                    .contains(&format!("edge {line}: its vertices")),
                "{e}"
            ),
            Ok(n) => panic!("{} nodes", n.points.len()),
        }
    }

    /// A circle of radius 10 sampled to sides of at most 1 µm wants about 63 000 segments, more
    /// than an edge may have: it stops at `MAX_EDGE_NODES` and says so. The longest segments are
    /// halved first, so when the bound stops it every side kept is at least half the longest
    /// (each was halved from one at least as long as any kept whole): its N = 19 999 sides are at
    /// most twice the even split's 2πr/N, and as a chord's sag goes with its square, none sags
    /// more than 4 r (1 − cos(π/N)), about 4.9e-7 mm (the even split's own: 1.2e-7). The sag
    /// recorded is the polyline's own largest. Halving depth first, the bound filled along the
    /// start of the circle and kept the rest whole: a quarter turn, 2.9 mm off.
    #[test]
    fn an_edge_that_stops_at_its_node_bound_says_so() {
        let cyl = fixture("cylinder_r10_h5");
        let Some(ei) = cyl
            .edges
            .iter()
            .position(|e| !matches!(e.curve, Curve::Line { .. }))
        else {
            panic!("a circle")
        };
        let rules = Rules {
            tol: 0.01,
            max_len: f64::INFINITY,
            turn: None,
        };
        let n = sample(&cyl, ei, &rules, 1e-3);
        // Exactly the bound: N = 19 999 sides.
        assert_eq!(n.points.len(), MAX_EDGE_NODES);
        let Some((crate::EdgeCap::Nodes, sag, excess)) = n.capped else {
            panic!("{:?}", n.capped)
        };
        // Each chord's sag on the circle of radius 10, from its length.
        let r = 10.0;
        let most = n
            .points
            .windows(2)
            .map(|w| {
                let c = w[0].dist(w[1]);
                r - (r * r - 0.25 * c * c).sqrt()
            })
            .fold(0.0, f64::max);
        let sides = (MAX_EDGE_NODES - 1) as f64;
        let bound = 4.0 * r * (1.0 - (std::f64::consts::PI / sides).cos());
        assert!(most <= bound * (1.0 + 1e-6), "{most} against {bound}");
        assert!(
            (sag - most).abs() <= 1e-6 * bound,
            "recorded {sag}, the polyline's {most}"
        );
        // Its sides are longer than the 1 µm asked: about 2πr / N / 1e-3, π.
        assert!(excess > 2.0 && excess < 7.0, "{excess}");
        // A line asked for more sides than that too, its chords on it.
        let Some(line) = cyl
            .edges
            .iter()
            .position(|e| matches!(e.curve, Curve::Line { .. }))
        else {
            panic!("a line")
        };
        let n = sample(&cyl, line, &rules, 1e-6);
        assert_eq!(n.points.len(), MAX_EDGE_NODES);
        // 5 mm in 19 999 sides, each 0.25 µm against the 1e-6 mm asked.
        let side = 5.0 / (MAX_EDGE_NODES - 1) as f64;
        let Some((crate::EdgeCap::Nodes, sag, excess)) = n.capped else {
            panic!("{:?}", n.capped)
        };
        assert_eq!(sag, 0.0);
        assert!((excess - side / 1e-6).abs() < 1e-9 * excess, "{excess}");
        // At sides it can have, neither says so.
        assert_eq!(sample(&cyl, ei, &rules, 1.0).capped, None);
        assert_eq!(sample(&cyl, line, &rules, 1.0).capped, None);
    }

    /// A polyline with a corner at a third of its parameter (never a node of the halving): the
    /// segment across the corner sags at every depth, so its halving stops at `MAX_DEPTH`, and the
    /// edge says so, with the sag left there. That was silent.
    #[test]
    fn an_edge_that_stops_at_its_depth_bound_says_so() {
        let mut b = fixture("box_10x6x4");
        // Edge 2 runs (10, 0, 4) to (10, 6, 4): bent up to (10, 2, 6) at t = 1/3.
        b.edges[2].curve = Curve::Spline {
            knots: vec![0.0, 0.0, 1.0 / 3.0, 1.0, 1.0],
            points: vec![[10.0, 0.0, 4.0], [10.0, 2.0, 6.0], [10.0, 6.0, 4.0]],
        };
        b.edges[2].nurbs = None;
        let rules = Rules {
            tol: 1e-12,
            max_len: f64::INFINITY,
            turn: None,
        };
        let n = sample(&b, 2, &rules, f64::INFINITY);
        assert!(n.points.len() < 1000, "{}", n.points.len());
        let Some((crate::EdgeCap::Depth, sag, _)) = n.capped else {
            panic!("{:?}", n.capped)
        };
        assert!(sag > 1e-12, "{sag}");
    }

    #[test]
    fn circle_edges_are_within_tolerance_and_end_on_vertices() {
        // A cylinder of radius 10 and length 5 up z from the origin.
        let cyl = fixture("cylinder_r10_h5");
        for tol in [0.1, 0.01, 0.001] {
            for (ei, e) in cyl.edges.iter().enumerate() {
                let pts = sample(
                    &cyl,
                    ei,
                    &Rules {
                        tol,
                        max_len: f64::INFINITY,
                        turn: None,
                    },
                    f64::INFINITY,
                )
                .points;
                assert!(pts.len() >= 2);
                let [front, back] = ends(&cyl, ei);
                assert_eq!(pts.first().copied(), Some(front));
                assert_eq!(pts.last().copied(), Some(back));
                if matches!(e.curve, Curve::Line { .. }) {
                    assert_eq!(pts.len(), 2);
                    continue;
                }
                // Circle of radius 10: every chord's middle sags at most `tol` (plus rounding).
                for w in pts.windows(2) {
                    let m = (w[0] + w[1]) * 0.5;
                    let r = (m.x * m.x + m.y * m.y).sqrt();
                    assert!(10.0 - r <= tol * 1.0001, "sag {} at tol {tol}", 10.0 - r);
                }
            }
        }
    }

    #[test]
    fn edges_are_split_to_the_side_length_and_turn() {
        // A cylinder of radius 10 and length 5 up z from the origin.
        let cyl = fixture("cylinder_r10_h5");
        for (ei, e) in cyl.edges.iter().enumerate() {
            let pts = sample(
                &cyl,
                ei,
                &Rules {
                    tol: 1.0,
                    max_len: f64::INFINITY,
                    turn: None,
                },
                1.2,
            )
            .points;
            for w in pts.windows(2) {
                assert!(w[0].dist(w[1]) <= 1.2 + 1e-9, "side {}", w[0].dist(w[1]));
            }
            if matches!(e.curve, Curve::Line { .. }) {
                // 5 mm in sides of at most 1.2: five equal sides.
                assert_eq!(pts.len(), 6);
            }
            // The ends are the vertices' own points, to the last bit.
            let [front, back] = ends(&cyl, ei);
            assert_eq!(pts.first().copied(), Some(front));
            assert_eq!(pts.last().copied(), Some(back));
        }
        // A coarse tolerance alone would leave a few long chords; a 30° facet angle keeps every
        // side within 30° of arc: at least 12 around each of the two circles.
        let thirty = Rules {
            tol: 5.0,
            max_len: f64::INFINITY,
            turn: Some(crate::planner::size::Turn {
                angle: std::f64::consts::PI / 6.0,
                floor: 0.0,
            }),
        };
        let sides: usize = (0..cyl.edges.len())
            .filter(|&ei| !matches!(cyl.edges[ei].curve, Curve::Line { .. }))
            .map(|ei| sample(&cyl, ei, &thirty, f64::INFINITY).points.len() - 1)
            .sum();
        assert!(sides >= 24, "{sides}");
    }

    /// The nodes of every edge where two curved faces meet carry their curve parameters: each
    /// face's pcurve of the edge puts each node on that face's surface, at the node.
    #[test]
    fn edge_nodes_carry_their_parameters_on_both_faces() {
        // A cylinder of radius 2 up z through (5, 5, -1), 6 long, less a cylinder of radius 1
        // along y through (5, -1, 2), 12 long.
        let body = fixture("cylinder_with_cross_hole");
        let rules = Rules {
            tol: 0.01,
            max_len: f64::INFINITY,
            turn: None,
        };
        let curved = |f: u32| {
            !matches!(
                body.faces[f as usize].surface,
                brep::Surface::Plane { .. } | brep::Surface::PlaneThrough { .. }
            )
        };
        let (mut edges, mut checked) = (0, 0);
        for (ei, e) in body.edges.iter().enumerate() {
            if e.uses.len() != 2 || !e.uses.iter().all(|u| curved(u.face)) {
                continue;
            }
            let n = sample(&body, ei, &rules, f64::INFINITY);
            edges += 1;
            for u in &e.uses {
                let face = &body.faces[u.face as usize];
                let brep::Loop::Edges { edges: loop_edges } = &face.loops[u.r#loop as usize] else {
                    panic!("an edge loop")
                };
                let oe = &loop_edges[u.position as usize];
                let Ok(s) = crate::planner::surface::eval_surface(face.parameters()) else {
                    panic!("evaluates")
                };
                let pcurve = kernel::curve2(&oe.pcurve);
                for (t, p) in n.params.iter().zip(&n.points) {
                    let Some(q) = pcurve.eval(*t) else {
                        panic!("pcurve at {t}")
                    };
                    let Some((at, _, _)) = s.derivs(q[0], q[1]).map(kernel::record_triple) else {
                        panic!("evaluates")
                    };
                    assert!(at.dist(*p) < 1e-6 + e.tolerance, "{at:?} vs {p:?}");
                    checked += 1;
                }
            }
        }
        assert!(edges > 0 && checked > 20, "{edges} edges, {checked} nodes");
    }
}
