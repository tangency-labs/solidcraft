//! The free-boundary motorcycle graph of a trimmed face (Lyon, Campen, Bommes & Kobbelt,
//! "Parametrization Quantization with Free Boundaries for Trimmed Quad Meshing", ACM TOG 38(4),
//! 2019, §4; after Eppstein, Goodrich, Kim & Tamstorf, "Motorcycle Graphs: Canonical Quad Mesh
//! Partitioning", SGP 2008).
//!
//! Peaks (Def. 4.1) are boundary nodes where u or v is a local extremum; a node in a flat run
//! counts only where the value differs on one side. A peak is concave where the face lies
//! beyond the extremum. Only concave peaks spawn motorcycles (Def. 4.2): a u-peak one along u,
//! a v-peak one along v, into the face. All move at once at the same speed (in the surface's
//! metric) along their iso-line; each stops where it reaches the boundary, a trace another
//! motorcycle laid earlier, or another motorcycle head-on (both stop where they meet). The
//! traces cut the face into patches that are rectangles with corners cut by free boundary.

/// A trace: the iso-line `axis` = `at` (0: u = at, running along v; 1: v = at, running along u)
/// from `from` to `to` in the other parameter (`from < to`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Trace {
    pub axis: usize,
    pub at: f64,
    pub from: f64,
    pub to: f64,
    /// The boundary node (peak) it starts from.
    pub start: usize,
    /// Its start and end points in (u, v).
    pub a: [f64; 2],
    pub b: [f64; 2],
    /// Where it stopped.
    pub end: End,
}

/// Where a motorcycle stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum End {
    /// On the boundary segment between these two boundary nodes.
    Boundary(usize, usize),
    /// On the trace with this index (a T-node).
    Trace(usize),
    /// Head-on with the trace with this index (both end at one point).
    HeadOn(usize),
}

/// Most motorcycles one face spawns (the pairwise events grow with their square).
const MAX_MOTORCYCLES: usize = 512;

struct Moto {
    /// Origin (metric).
    o: [f64; 2],
    /// Direction of travel: the parameter (0: u, 1: v) and its sign.
    d: usize,
    sign: f64,
    /// Boundary node it starts on (its own segments are not a crash).
    node: usize,
    /// Distance to the boundary along its way, and the segment it reaches there.
    reach: f64,
    hit: (usize, usize),
}

/// The traces of the face bounded by `loops` (ranges of `boundary`, even–odd), in (u, v),
/// measured in the metric `scale`. Two motorcycles coming at each other on lines less than
/// `snap` apart (in the metric) meet head-on: a polygon's extreme node is only near the curve's
/// true extreme, so the two extremes facing each other across a gap (one hole above another)
/// are rarely on exactly one line. Empty when there is no concave peak.
pub(crate) fn traces(boundary: &[[f64; 2]], loops: &[std::ops::Range<usize>], scale: [f64; 2], snap: f64) -> Vec<Trace> {
    if !scale.iter().all(|s| s.is_finite() && *s > 0.0) {
        return Vec::new();
    }
    let p: Vec<[f64; 2]> = boundary.iter().map(|q| [q[0] * scale[0], q[1] * scale[1]]).collect();
    let mut lo = [f64::MAX; 2];
    let mut hi = [f64::MIN; 2];
    for q in &p {
        for d in 0..2 {
            lo[d] = lo[d].min(q[d]);
            hi[d] = hi[d].max(q[d]);
        }
    }
    let extent = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    if !(extent.is_finite() && extent > 0.0) {
        return Vec::new();
    }
    // Values this close are one (a node on an iso-line edge carries projection rounding).
    let eps = extent * 1e-9;
    let segs: Vec<(usize, usize)> = loops
        .iter()
        .flat_map(|r| {
            let n = r.len();
            (0..n).map(move |k| (r.start + k, r.start + (k + 1) % n))
        })
        .collect();
    let inside = |q: [f64; 2]| {
        let mut odd = false;
        for &(a, b) in &segs {
            let (Some(a), Some(b)) = (p.get(a), p.get(b)) else { continue };
            if (a[1] > q[1]) != (b[1] > q[1]) && q[0] < a[0] + (q[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
                odd = !odd;
            }
        }
        odd
    };
    // Peaks and their motorcycles.
    let mut motos: Vec<Moto> = Vec::new();
    for r in loops {
        let n = r.len();
        if n < 3 {
            continue;
        }
        for k in 0..n {
            let (Some(&a), Some(&b), Some(&c)) = (p.get(r.start + (k + n - 1) % n), p.get(r.start + k), p.get(r.start + (k + 1) % n)) else {
                continue;
            };
            for d in 0..2 {
                let (before, after) = (a[d] - b[d], c[d] - b[d]);
                let flat = |x: f64| x.abs() <= eps;
                // A (non-strict) minimum higher on one side, or maximum lower on one side.
                let min = (before > eps || flat(before)) && (after > eps || flat(after)) && !(flat(before) && flat(after));
                let max = (before < -eps || flat(before)) && (after < -eps || flat(after)) && !(flat(before) && flat(after));
                let sign = match (min, max) {
                    (true, false) => -1.0,
                    (false, true) => 1.0,
                    _ => continue,
                };
                // Concave: the face lies beyond the extremum.
                let mut probe = b;
                probe[d] += sign * extent * 1e-6;
                if !inside(probe) {
                    continue;
                }
                if motos.len() >= MAX_MOTORCYCLES {
                    return Vec::new();
                }
                motos.push(Moto { o: b, d, sign, node: r.start + k, reach: f64::INFINITY, hit: (0, 0) });
            }
        }
    }
    if motos.is_empty() {
        return Vec::new();
    }
    // Where each would reach the boundary.
    for m in &mut motos {
        let e = 1 - m.d;
        for &(ia, ib) in &segs {
            if ia == m.node || ib == m.node {
                continue;
            }
            let (Some(a), Some(b)) = (p.get(ia), p.get(ib)) else { continue };
            // The segment crosses the line other = o[e] at parameter s along d.
            let (lo_e, hi_e) = (a[e].min(b[e]), a[e].max(b[e]));
            if m.o[e] < lo_e || m.o[e] > hi_e {
                continue;
            }
            // A segment along the line itself is reached at its nearer end ahead.
            let ahead = |x: f64| (x - m.o[m.d]) * m.sign;
            let t = if (b[e] - a[e]).abs() <= eps {
                [ahead(a[m.d]), ahead(b[m.d])].into_iter().filter(|t| *t > eps).fold(f64::INFINITY, f64::min)
            } else {
                ahead(a[m.d] + (m.o[e] - a[e]) / (b[e] - a[e]) * (b[m.d] - a[m.d]))
            };
            if t > eps && t < m.reach {
                m.reach = t;
                m.hit = (ia, ib);
            }
        }
    }
    // Events in time order: (time, the motorcycle that stops, where, the one whose trace stops
    // it and when it passed there).
    struct Event {
        t: f64,
        who: usize,
        at: f64,
        cause: Option<(usize, f64)>,
        partner: Option<usize>,
    }
    let mut events: Vec<Event> = Vec::new();
    for (i, m) in motos.iter().enumerate() {
        if m.reach.is_finite() {
            events.push(Event { t: m.reach, who: i, at: m.reach, cause: None, partner: None });
        }
        for (j, n) in motos.iter().enumerate().skip(i + 1) {
            if m.d != n.d {
                // Crossing: m's line is other = m.o[1 - m.d]; n travels along n.d = 1 - m.d.
                let x_on_m = (n.o[m.d] - m.o[m.d]) * m.sign;
                let x_on_n = (m.o[n.d] - n.o[n.d]) * n.sign;
                if x_on_m > eps && x_on_m < m.reach && x_on_n > eps && x_on_n < n.reach {
                    // The later one stops on the earlier one's trace (a tie stops both).
                    if x_on_m >= x_on_n {
                        events.push(Event { t: x_on_m, who: i, at: x_on_m, cause: Some((j, x_on_n)), partner: None });
                    }
                    if x_on_n >= x_on_m {
                        events.push(Event { t: x_on_n, who: j, at: x_on_n, cause: Some((i, x_on_m)), partner: None });
                    }
                }
            } else if m.sign != n.sign && (m.o[1 - m.d] - n.o[1 - n.d]).abs() <= eps.max(snap) {
                // Head-on on one line: they meet halfway if each would get there.
                let gap = (n.o[m.d] - m.o[m.d]) * m.sign;
                let half = 0.5 * gap;
                if gap > eps && half < m.reach && half < n.reach {
                    events.push(Event { t: half, who: i, at: half, cause: None, partner: Some(j) });
                }
            }
        }
    }
    events.sort_by(|a, b| a.t.total_cmp(&b.t));
    let mut stop: Vec<Option<(f64, Stop)>> = vec![None; motos.len()];
    for ev in &events {
        if stop.get(ev.who).is_none_or(|s| s.is_some()) {
            continue;
        }
        if let Some((c, passed)) = ev.cause {
            // The trace is there only if its motorcycle got that far.
            if stop.get(c).copied().flatten().is_some_and(|(s, _)| s < passed) {
                continue;
            }
        }
        let how = match (ev.cause, ev.partner) {
            (Some((c, _)), _) => Stop::Trace(c),
            (None, Some(j)) => Stop::HeadOn(j),
            (None, None) => Stop::Boundary,
        };
        if let Some(j) = ev.partner {
            if stop.get(j).is_none_or(|s| s.is_some()) {
                continue;
            }
            if let Some(s) = stop.get_mut(j) {
                *s = Some((ev.at, Stop::HeadOn(ev.who)));
            }
        }
        if let Some(s) = stop.get_mut(ev.who) {
            *s = Some((ev.at, how));
        }
    }
    // Output indices of the motorcycles that stopped (all do: each reaches the boundary).
    let mut out_index = vec![usize::MAX; motos.len()];
    let mut k = 0;
    for (i, s) in stop.iter().enumerate() {
        if s.is_some() {
            if let Some(slot) = out_index.get_mut(i) {
                *slot = k;
            }
            k += 1;
        }
    }
    let uv = |q: [f64; 2]| [q[0] / scale[0], q[1] / scale[1]];
    motos
        .iter()
        .zip(&stop)
        .filter_map(|(m, s)| {
            let (len, how) = (*s)?;
            let (a, b) = (m.o[m.d], m.o[m.d] + m.sign * len);
            let (from, to) = (a.min(b) / scale[m.d], a.max(b) / scale[m.d]);
            // The trace runs along d, so it lies on the iso-line of the other parameter.
            let axis = 1 - m.d;
            let mut endp = m.o;
            endp[m.d] = b;
            let end = match how {
                Stop::Boundary => End::Boundary(m.hit.0, m.hit.1),
                Stop::Trace(c) => End::Trace(*out_index.get(c)?),
                Stop::HeadOn(j) => End::HeadOn(*out_index.get(j)?),
            };
            Some(Trace { axis, at: m.o[axis] / scale[axis], from, to, start: m.node, a: uv(m.o), b: uv(endp), end })
        })
        .collect()
}

#[derive(Clone, Copy, Debug)]
enum Stop {
    Boundary,
    Trace(usize),
    HeadOn(usize),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle(c: [f64; 2], r: f64, n: usize, ccw: bool) -> Vec<[f64; 2]> {
        (0..n)
            .map(|k| {
                let a = std::f64::consts::TAU * k as f64 / n as f64 * if ccw { 1.0 } else { -1.0 };
                [c[0] + r * a.cos(), c[1] + r * a.sin()]
            })
            .collect()
    }

    #[test]
    fn a_convex_face_has_no_traces() {
        let b = circle([0.0, 0.0], 1.0, 32, true);
        assert!(traces(&b, &[std::ops::Range { start: 0, end: 32 }], [1.0, 1.0], 0.0).is_empty());
        let sq = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        assert!(traces(&sq, &[std::ops::Range { start: 0, end: 4 }], [1.0, 1.0], 0.0).is_empty());
    }

    /// A centred round hole: its four extremes are concave peaks, and each motorcycle runs
    /// straight to the outer square (none meets another).
    #[test]
    fn a_hole_sends_four_traces_to_the_outside() {
        let mut b = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        b.extend(circle([2.0, 2.0], 1.0, 32, false));
        let t = traces(&b, &[0..4, 4..36], [1.0, 1.0], 0.0);
        assert_eq!(t.len(), 4, "{t:?}");
        let on = |axis: usize, at: f64, from: f64, to: f64| {
            t.iter().any(|x| x.axis == axis && (x.at - at).abs() < 1e-12 && (x.from - from).abs() < 1e-9 && (x.to - to).abs() < 1e-9)
        };
        assert!(on(1, 2.0, 0.0, 1.0), "left: v = 2 from u = 0 to the hole");
        assert!(on(1, 2.0, 3.0, 4.0), "right");
        assert!(on(0, 2.0, 0.0, 1.0), "below");
        assert!(on(0, 2.0, 3.0, 4.0), "above");
    }

    /// An L: the reflex corner is a concave peak in both u and v, and its two motorcycles
    /// cut the L into three rectangles.
    #[test]
    fn an_l_is_cut_at_its_reflex_corner() {
        let b = [[0.0, 0.0], [2.0, 0.0], [2.0, 1.0], [1.0, 1.0], [1.0, 2.0], [0.0, 2.0]];
        let t = traces(&b, &[std::ops::Range { start: 0, end: 6 }], [1.0, 1.0], 0.0);
        assert_eq!(t.len(), 2, "{t:?}");
        let has = |axis: usize| {
            t.iter().any(|x| x.axis == axis && x.at == 1.0 && x.from == 0.0 && x.to == 1.0 && x.start == 3 && matches!(x.end, End::Boundary(..)))
        };
        assert!(has(0) && has(1), "{t:?}");
    }

    /// Two holes side by side: the inner motorcycles meet head-on halfway, and a later
    /// motorcycle stops on an earlier one's trace (no crossings).
    #[test]
    fn traces_meet_head_on_and_never_cross() {
        let mut b = vec![[0.0, 0.0], [8.0, 0.0], [8.0, 4.0], [0.0, 4.0]];
        b.extend(circle([2.0, 2.0], 1.0, 32, false));
        b.extend(circle([6.0, 2.5], 1.0, 32, false));
        let t = traces(&b, &[0..4, 4..36, 36..68], [1.0, 1.0], 0.0);
        // Hole 1's right extreme (3, 2) and hole 2's left extreme (5, 2.5) are on different
        // lines; each runs on until it meets the other hole or a trace.
        for (i, a) in t.iter().enumerate() {
            for c in t.iter().skip(i + 1) {
                if a.axis != c.axis {
                    // a on axis line a.at; c crosses it if c.at is strictly within a's span
                    // and a.at strictly within c's.
                    let cross = c.at > a.from + 1e-9 && c.at < a.to - 1e-9 && a.at > c.from + 1e-9 && a.at < c.to - 1e-9;
                    assert!(!cross, "{a:?} crosses {c:?}");
                }
            }
        }
        // Same-line pair: two holes at the same height meet halfway.
        let mut b = vec![[0.0, 0.0], [8.0, 0.0], [8.0, 4.0], [0.0, 4.0]];
        b.extend(circle([2.0, 2.0], 1.0, 32, false));
        b.extend(circle([6.0, 2.0], 1.0, 32, false));
        let t = traces(&b, &[0..4, 4..36, 36..68], [1.0, 1.0], 0.0);
        assert!(t.iter().any(|x| x.axis == 1 && (x.at - 2.0).abs() < 1e-12 && (x.from - 3.0).abs() < 1e-9 && (x.to - 4.0).abs() < 1e-9), "{t:?}");
        assert!(t.iter().any(|x| x.axis == 1 && (x.at - 2.0).abs() < 1e-12 && (x.from - 4.0).abs() < 1e-9 && (x.to - 5.0).abs() < 1e-9), "{t:?}");
        // Those two ended head-on with each other.
        let i = t.iter().position(|x| x.axis == 1 && (x.from - 3.0).abs() < 1e-9).unwrap_or(usize::MAX);
        let j = t.iter().position(|x| x.axis == 1 && (x.from - 4.0).abs() < 1e-9).unwrap_or(usize::MAX);
        assert_eq!(t.get(i).map(|x| x.end), Some(End::HeadOn(j)), "{t:?}");
        assert_eq!(t.get(j).map(|x| x.end), Some(End::HeadOn(i)), "{t:?}");
    }
}
