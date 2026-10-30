//! Edge nodes: every edge of a body sampled once, so the faces on either side share the very
//! same boundary points (watertight by construction).

use solidcraft_geom::Vec3;
use solidcraft_geom::brep::eval::CurveEval;
use solidcraft_geom::brep::{self, Curve};

use super::size::Rules;
use crate::{KernelError, Result};

/// Most nodes on one edge.
pub(crate) const MAX_EDGE_NODES: usize = 20_000;
/// Deepest bisection of one edge segment.
const MAX_DEPTH: u32 = 24;
/// The most of its curve's turn one segment may span (a quarter turn, with rounding room): the
/// boundary of every face stays a polygon with the face's area, as a closed curve's first four
/// segments keep it.
pub(crate) const QUARTER_TURN: f64 = std::f64::consts::FRAC_PI_2 * (1.0 + 1e-9);

/// An edge's nodes, from its start vertex (interval min) to its end vertex (interval max).
pub(crate) struct Nodes {
    pub points: Vec<Vec3>,
    /// The curve's parameter at each node (the edge's interval, ends included): where each
    /// face's pcurve of the edge gives the node's (u, v).
    pub params: Vec<f64>,
    /// The parameters of the nodes that are another edge's node, where the two edges touch
    /// within the chord tolerance (see [`snap`]): in a face that runs both edges such a node is
    /// the other's point of the face, a pinch, not a crossing.
    pub snapped: Vec<f64>,
}

/// Nodes of edge `ei` of `b` from its start to its end: every chord within the tolerance of
/// the curve (checked at the quarter points, so an S-bend in one segment is not missed), at
/// most `h` long, within the rules' facet angle and never spanning more than a quarter turn of
/// the curve. The end nodes are the vertices' own points, so edges meeting at a vertex meet
/// exactly. A curve that does not evaluate somewhere on its interval is an error naming the
/// edge.
pub(crate) fn sample_edge(b: &brep::Brep, ei: usize, rules: &Rules, h: f64) -> Result<Nodes> {
    let fail = |why: String| KernelError::Failed(format!("tessellation: edge {ei}: {why}"));
    let edge = b.edges.get(ei).ok_or_else(|| fail("no such edge".into()))?;
    let vertex = |k: usize| {
        let v = edge.vertices.get(k).and_then(|&v| b.vertices.get(v as usize)).ok_or_else(|| fail("no such vertex".into()))?;
        Ok::<Vec3, KernelError>(Vec3::new(v.point[0], v.point[1], v.point[2]))
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
        let n = if h > 0.0 { (front.dist(back) / h).ceil().clamp(1.0, (MAX_EDGE_NODES - 1) as f64) as usize } else { 1 };
        let mut pts: Vec<Vec3> = (0..=n).map(|i| front + (back - front) * (i as f64 / n as f64)).collect();
        let mut params: Vec<f64> = (0..=n).map(|i| t0 + (t1 - t0) * (i as f64 / n as f64)).collect();
        if let (Some(p), Some(t)) = (pts.last_mut(), params.last_mut()) {
            *p = back;
            *t = t1;
        }
        return Ok(Nodes { points: pts, params, snapped: Vec::new() });
    }
    let curve = CurveEval::new(edge.parameters()).map_err(|e| fail(e.to_string()))?;
    let at =
        |t: f64| -> Result<Vec3> { curve.point(t).filter(|p| p.is_finite()).ok_or_else(|| fail(format!("the curve does not evaluate at t = {t}"))) };
    // A closed curve's chord between its ends is empty: start it from a few segments. An open
    // one starts whole, so an arc the tolerance leaves flat stays one segment.
    let initial: u32 = if edge.vertices[0] == edge.vertices[1] { 4 } else { 1 };
    let t_at = |i: u32| t0 + (t1 - t0) * f64::from(i) / f64::from(initial);
    let ends: Vec<Vec3> = (0..=initial).map(|i| at(t_at(i))).collect::<Result<_>>()?;
    // Depth-first bisection, segment by segment, keeping the output ordered. Each segment
    // carries its end points and middle, so every parameter is evaluated once (an
    // intersection curve's point is a Newton solve).
    struct Seg {
        a: f64,
        b: f64,
        pa: Vec3,
        pb: Vec3,
        mid: Option<Vec3>,
        depth: u32,
    }
    let mut out: Vec<(f64, Vec3)> = vec![(t0, front)];
    let mut stack: Vec<Seg> = Vec::with_capacity(64);
    for (i, w) in ends.windows(2).enumerate().rev() {
        let (Some(&pa), Some(&pb)) = (w.first(), w.get(1)) else { continue };
        let i = i as u32;
        stack.push(Seg { a: t_at(i), b: t_at(i + 1), pa, pb, mid: None, depth: 0 });
    }
    while let Some(sg) = stack.pop() {
        let (a, b, pa, pb) = (sg.a, sg.b, sg.pa, sg.pb);
        let m = 0.5 * (a + b);
        let pm = match sg.mid {
            Some(p) => p,
            None => at(m)?,
        };
        let (q1, q3) = (at(a + 0.25 * (b - a))?, at(a + 0.75 * (b - a))?);
        let done = sg.depth >= MAX_DEPTH || out.len() + stack.len() >= MAX_EDGE_NODES;
        let fine = || {
            let sag = [q1, pm, q3].iter().map(|q| q.dist_to_segment(pa, pb)).fold(0.0, f64::max);
            let short = !(pa.dist(pb) > h * (1.0 + 1e-9));
            // The two half chords of an arc turning θ differ by θ/2.
            let bend = 2.0 * turn(pm - pa, pb - pm);
            let straight = rules.turn.is_none_or(|t| !t.exceeds(bend, sag));
            // Whatever the tolerance, no segment spans more than a quarter turn of its curve: a
            // small circle split into two half arcs would otherwise be two chords on one line,
            // a hole of no area (the 1 mm hole of a 990 mm plate at its 2 mm chord).
            let quarter = bend <= QUARTER_TURN;
            short && straight && quarter && sag <= tol
        };
        if !done && !fine() {
            stack.push(Seg { a: m, b, pa: pm, pb, mid: Some(q3), depth: sg.depth + 1 });
            stack.push(Seg { a, b: m, pa, pb: pm, mid: Some(q1), depth: sg.depth + 1 });
        } else {
            out.push((b, pb));
        }
    }
    if let Some(p) = out.last_mut() {
        *p = (t1, back);
    }
    Ok(Nodes { points: out.iter().map(|p| p.1).collect(), params: out.iter().map(|p| p.0).collect(), snapped: Vec::new() })
}

/// Edge `ei`'s nodes with each of its segments `segs` (counted from its start) halved at the
/// middle of its curve parameters, the new node on the curve (on a line, halfway between the two,
/// as [`sample_edge`] spaces a line's nodes). A segment too short to halve in floating point keeps
/// its two nodes. More than [`MAX_EDGE_NODES`] nodes, or a curve that does not evaluate there, is
/// an error naming the edge.
pub(crate) fn split_segments(b: &brep::Brep, ei: usize, nodes: &mut Nodes, segs: &[u32]) -> Result<()> {
    let fail = |why: String| KernelError::Failed(format!("tessellation: edge {ei}: {why}"));
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
        _ => Some(CurveEval::new(edge.parameters()).map_err(|e| fail(e.to_string()))?),
    };
    let (mut points, mut params) = (Vec::with_capacity(n + segs.len()), Vec::with_capacity(n + segs.len()));
    let mut next = segs.iter().copied().peekable();
    for k in 0..n {
        let (Some(&p), Some(&t)) = (nodes.points.get(k), nodes.params.get(k)) else { continue };
        points.push(p);
        params.push(t);
        if next.next_if_eq(&(k as u32)).is_none() {
            continue;
        }
        let (Some(&q), Some(&u)) = (nodes.points.get(k + 1), nodes.params.get(k + 1)) else { continue };
        let m = 0.5 * (t + u);
        if !(m > t.min(u) && m < t.max(u)) {
            continue;
        }
        let pm = match &curve {
            None => (p + q) * 0.5,
            Some(c) => c.point(m).filter(|p| p.is_finite()).ok_or_else(|| fail(format!("the curve does not evaluate at t = {m}")))?,
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
    let mut add: Vec<(f64, Vec3)> = add.iter().copied().filter(|(t, p)| t.is_finite() && p.is_finite()).collect();
    add.sort_by(|a, b| a.0.total_cmp(&b.0));
    add.dedup_by(|a, b| a.0 == b.0);
    if nodes.points.len() + add.len() > MAX_EDGE_NODES {
        return Err(KernelError::Failed(format!("tessellation: edge {ei}: more than {MAX_EDGE_NODES} nodes")));
    }
    let n = nodes.points.len().min(nodes.params.len());
    let (mut points, mut params) = (Vec::with_capacity(n + add.len()), Vec::with_capacity(n + add.len()));
    let mut next = add.iter().peekable();
    for k in 0..n {
        let (Some(&p), Some(&t)) = (nodes.points.get(k), nodes.params.get(k)) else { continue };
        points.push(p);
        params.push(t);
        let Some(&u) = nodes.params.get(k + 1) else { continue };
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
    (a.dot(b) / (la * lb)).clamp(-1.0, 1.0).acos()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(b: crate::Result<crate::Body>) -> brep::Brep {
        match b.and_then(|b| crate::brep_record(&b)) {
            Ok(r) => r,
            Err(e) => panic!("{e}"),
        }
    }

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

    #[test]
    fn circle_edges_are_within_tolerance_and_end_on_vertices() {
        let cyl = record(crate::cylinder(Vec3::ZERO, Vec3::Z, 10.0, 5.0));
        for tol in [0.1, 0.01, 0.001] {
            for (ei, e) in cyl.edges.iter().enumerate() {
                let pts = sample(&cyl, ei, &Rules { tol, max_len: f64::INFINITY, turn: None }, f64::INFINITY).points;
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
        let cyl = record(crate::cylinder(Vec3::ZERO, Vec3::Z, 10.0, 5.0));
        for (ei, e) in cyl.edges.iter().enumerate() {
            let pts = sample(&cyl, ei, &Rules { tol: 1.0, max_len: f64::INFINITY, turn: None }, 1.2).points;
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
        let thirty =
            Rules { tol: 5.0, max_len: f64::INFINITY, turn: Some(crate::tess::size::Turn { angle: std::f64::consts::PI / 6.0, floor: 0.0 }) };
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
        let pin = crate::cylinder(Vec3::new(5.0, 5.0, -1.0), Vec3::Z, 2.0, 6.0);
        let cross = crate::cylinder(Vec3::new(5.0, -1.0, 2.0), Vec3::Y, 1.0, 12.0);
        let body = record(pin.and_then(|pin| {
            let cross = cross?;
            crate::boolean(&pin, &cross, crate::BoolOp::Cut)?.ok_or_else(|| crate::KernelError::Failed("cut: nothing left".into()))
        }));
        let rules = Rules { tol: 0.01, max_len: f64::INFINITY, turn: None };
        let curved = |f: u32| !matches!(body.faces[f as usize].surface, brep::Surface::Plane { .. } | brep::Surface::PlaneThrough { .. });
        let (mut edges, mut checked) = (0, 0);
        for (ei, e) in body.edges.iter().enumerate() {
            if e.uses.len() != 2 || !e.uses.iter().all(|u| curved(u.face)) {
                continue;
            }
            let n = sample(&body, ei, &rules, f64::INFINITY);
            edges += 1;
            for u in &e.uses {
                let face = &body.faces[u.face as usize];
                let brep::Loop::Edges { edges: loop_edges } = &face.loops[u.r#loop as usize] else { panic!("an edge loop") };
                let oe = &loop_edges[u.position as usize];
                let Ok((s, map)) = crate::tess::surface::eval_surface(face.parameters(), face.domain) else { panic!("evaluates") };
                for (t, p) in n.params.iter().zip(&n.points) {
                    let Some(q) = oe.pcurve.eval(*t) else { panic!("pcurve at {t}") };
                    let q = map.to_eval(q);
                    let Some((at, _, _)) = s.derivs(q[0], q[1]) else { panic!("evaluates") };
                    assert!(at.dist(*p) < 1e-6 + e.tolerance, "{at:?} vs {p:?}");
                    checked += 1;
                }
            }
        }
        assert!(edges > 0 && checked > 20, "{edges} edges, {checked} nodes");
    }
}
