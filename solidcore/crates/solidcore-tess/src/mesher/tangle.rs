//! Where a face's sampled boundary crosses itself: chords of its loops, in the face's
//! parameters, that cross another chord of the face although the curves they stand for do not
//! (two curves running closer together than their chords sag).

use solidcore::Vec3;
use super::comp;
use super::seam;
use super::uv;

/// Two chords cross only where each one's ends lie strictly either side of the other, by more
/// than this fraction of the product of their lengths (a chord ending on another, as at a
/// shared vertex or along a seam's two uses a period apart, is not a crossing).
const STRADDLE: f64 = 1e-10;

/// One chord of a loop, or a copy of it whole periods along.
struct Chord {
    /// The chord's place in the face: its loop and its first node.
    at: (usize, usize),
    a: [f64; 2],
    b: [f64; 2],
    /// The chord's end nodes in space, where the loop has them.
    ends: [Option<Vec3>; 2],
}

impl Chord {
    fn lo(&self, d: usize) -> f64 {
        comp(&self.a, d).min(comp(&self.b, d))
    }
    fn hi(&self, d: usize) -> f64 {
        comp(&self.a, d).max(comp(&self.b, d))
    }
}

/// The chords (loop, first node) of `loops` that cross another chord of the face, in its loop or
/// another, each once and in order. On a closed surface the boundary crosses itself where any
/// copies of two chords, whole periods apart, cross; chords ending on a pole, where the
/// parameter along the pole is free, are left out. Adjacent chords of a loop share a node and
/// never cross. `reach`: how near a pole a node is taken as one (see [`super::poles::Face::pole`]).
pub(crate) fn crossing(
    s: &super::poles::Face,
    reach: f64,
    loops: &[seam::Loop],
) -> Vec<(usize, usize)> {
    let mut out: Vec<_> = splits(s, reach, loops)
        .into_iter()
        .map(|(l, k, _)| (l, k))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Crossing events, with the fraction along each chord where it meets the other chord.
/// Refinement must put a curve node there: halving can keep missing a crossing near an end.
pub(crate) fn splits(
    s: &super::poles::Face,
    reach: f64,
    loops: &[seam::Loop],
) -> Vec<(usize, usize, f64)> {
    let mut out: Vec<_> = events(s, reach, loops)
        .into_iter()
        .map(|e| (e.at.0, e.at.1, e.fraction))
        .collect();
    out.sort_by(|a, b| (a.0, a.1).cmp(&(b.0, b.1)).then(a.2.total_cmp(&b.2)));
    out.dedup();
    out
}

/// A directed crossing, retaining the other chord's identity. `end` identifies a touching
/// endpoint of the other chord; proper crossings have no endpoint.
pub(crate) struct Event {
    pub at: (usize, usize),
    pub other: (usize, usize),
    pub end: Option<usize>,
    pub fraction: f64,
}

pub(crate) fn events(s: &super::poles::Face, reach: f64, loops: &[seam::Loop]) -> Vec<Event> {
    let dom = uv::domain(s);
    let pole = |q: [f64; 2]| (0..2).any(|d| s.pole(q, d, reach));
    let mut chords: Vec<Chord> = Vec::new();
    for (li, l) in loops.iter().enumerate() {
        let n = l.uv.len();
        if n < 3 {
            continue;
        }
        // Each node is on a pole or not whichever two chords it ends: asked once (ported from
        // #13559).
        let mut on_pole: Vec<Option<bool>> = vec![None; n];
        let mut node_pole = |k: usize, q: [f64; 2]| -> bool {
            if let Some(p) = on_pole.get(k).copied().flatten() {
                return p;
            }
            let p = pole(q);
            if let Some(slot) = on_pole.get_mut(k) {
                *slot = Some(p);
            }
            p
        };
        for k in 0..n {
            let next = (k + 1) % n;
            let (Some(&a), Some(&b)) = (l.uv.get(k), l.uv.get(next)) else {
                continue;
            };
            if !(a.iter().chain(&b).all(|x| x.is_finite())) || node_pole(k, a) || node_pole(next, b)
            {
                continue;
            }
            // The loop's closing chord of a loop that winds round the surface ends at the copy of
            // its first node nearest its last; every chord then starts in one period.
            let (mut a, mut b) = (a, b);
            for (d, per) in dom.period.into_iter().enumerate() {
                let Some(per) = per else { continue };
                let (Some(ad), Some(bd)) = (a.get_mut(d), b.get_mut(d)) else {
                    continue;
                };
                *bd = uv::unwrap(*bd, *ad, Some(per));
                let back = (ad.min(*bd) / per).floor() * per;
                *ad -= back;
                *bd -= back;
            }
            let ends = [l.pts.get(k).copied(), l.pts.get(next).copied()];
            chords.push(Chord {
                at: (li, k),
                a,
                b,
                ends,
            });
        }
    }
    // With every chord starting in one period, two chords' copies can only cross where one of
    // them is moved a period along at most: each chord also as its copies a period along.
    for (d, per) in dom.period.into_iter().enumerate() {
        let Some(per) = per else { continue };
        let along: Vec<Chord> = chords
            .iter()
            .map(|c| {
                let (mut a, mut b) = (c.a, c.b);
                if let (Some(ad), Some(bd)) = (a.get_mut(d), b.get_mut(d)) {
                    *ad += per;
                    *bd += per;
                }
                Chord {
                    at: c.at,
                    a,
                    b,
                    ends: c.ends,
                }
            })
            .collect();
        chords.extend(along);
    }
    // Two chords that end on one node, the same point in space (edges meeting at a vertex end on
    // its point to the last bit), meet there and nowhere else, as adjacent chords do: two segments
    // from one point cross only along one line, which `along` leaves out anyway. Their (u, v)
    // ends may differ by a rounding (a seam's two copies, a period apart), which is no crossing
    // and no node lying on the other chord: a split there would put a node a rounding from the
    // vertex, the next round the same again, and the mesh would weld them into one.
    let meet = |x: &Chord, y: &Chord| {
        x.ends
            .iter()
            .flatten()
            .any(|p| y.ends.iter().flatten().any(|q| p == q))
    };
    let adjacent = |p: (usize, usize), q: (usize, usize)| {
        let n = loops.get(p.0).map_or(0, |l| l.uv.len());
        p == q || (p.0 == q.0 && ((p.1 + 1) % n == q.1 || (q.1 + 1) % n == p.1))
    };
    // Sweep along u: only chords whose u ranges overlap can cross.
    chords.sort_by(|x, y| x.lo(0).total_cmp(&y.lo(0)));
    let mut out: Vec<Event> = Vec::new();
    for (i, x) in chords.iter().enumerate() {
        for y in chords.iter().skip(i + 1) {
            if y.lo(0) > x.hi(0) {
                break;
            }
            if y.lo(1) > x.hi(1)
                || x.lo(1) > y.hi(1)
                || adjacent(x.at, y.at)
                || meet(x, y)
            {
                continue;
            }
            if cross(x.a, x.b, y.a, y.b) {
                let r = [x.b[0] - x.a[0], x.b[1] - x.a[1]];
                let s = [y.b[0] - y.a[0], y.b[1] - y.a[1]];
                let q = [y.a[0] - x.a[0], y.a[1] - x.a[1]];
                let det = |a: [f64; 2], b: [f64; 2]| a[0] * b[1] - a[1] * b[0];
                let den = det(r, s);
                // Parallel chords have no single crossing point; a fraction is kept only where it
                // names a point strictly inside its chord.
                let inside = |f: f64| f.is_finite() && f > 0.0 && f < 1.0;
                let (fx, fy) = (det(q, s) / den, det(q, r) / den);
                if inside(fx) {
                    out.push(Event {
                        at: x.at,
                        other: y.at,
                        end: None,
                        fraction: fx,
                    });
                }
                if inside(fy) {
                    out.push(Event {
                        at: y.at,
                        other: x.at,
                        end: None,
                        fraction: fy,
                    });
                }
            }
            // A node of one chord lying on the other (inside it, not at its ends) is no crossing,
            // but no triangulation can keep both as constraints either: where two curves leave a
            // vertex they touch at along one tangent (a circle touching another from inside), the
            // inner one's node can fall on the outer one's chord (with the outer chord twice the
            // inner, exactly so but for the next order). Split that chord at the node's projection.
            // (Not where the two chords run along one line: a seam's two uses, a period apart,
            // lie on each other as copies.)
            for (a, b) in [(x, y), (y, x)] {
                if along(a.a, a.b, b.a, b.b) {
                    continue;
                }
                for (end, q) in [b.a, b.b].into_iter().enumerate() {
                    if on(a.a, a.b, q) {
                        let dx = a.b[0] - a.a[0];
                        let dy = a.b[1] - a.a[1];
                        let f = ((q[0] - a.a[0]) * dx + (q[1] - a.a[1]) * dy) / (dx * dx + dy * dy);
                        out.push(Event {
                            at: a.at,
                            other: b.at,
                            end: Some(end),
                            fraction: f,
                        });
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| {
        (a.at, a.other, a.end)
            .cmp(&(b.at, b.other, b.end))
            .then(a.fraction.total_cmp(&b.fraction))
    });
    out.dedup_by(|a, b| a.at == b.at && a.other == b.other && a.end == b.end);
    out
}

/// Whether `q` lies on the segment `ab` within [`STRADDLE`] (of the segment's squared length,
/// off its line) strictly between its ends.
fn on(a: [f64; 2], b: [f64; 2], q: [f64; 2]) -> bool {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    if !(len2 > 0.0) {
        return false;
    }
    let (qx, qy) = (q[0] - a[0], q[1] - a[1]);
    let off = (dx * qy - dy * qx).abs();
    let along = (dx * qx + dy * qy) / len2;
    off <= 1e3 * STRADDLE * len2 && along > 0.0 && along < 1.0
}

/// Whether `c` and `d` both lie on the line of `ab` (within [`on`]'s measure).
fn along(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let off = |q: [f64; 2]| (dx * (q[1] - a[1]) - dy * (q[0] - a[0])).abs();
    len2 > 0.0 && off(c) <= 1e3 * STRADDLE * len2 && off(d) <= 1e3 * STRADDLE * len2
}

/// Whether the segments `ab` and `cd` cross: each one's ends strictly either side of the other.
fn cross(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let orient = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| {
        (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0])
    };
    let len = |p: [f64; 2], q: [f64; 2]| (q[0] - p[0]).hypot(q[1] - p[1]);
    let eps = STRADDLE * len(a, b) * len(c, d);
    let side = |o: f64| {
        if o > eps {
            1
        } else if o < -eps {
            -1
        } else {
            0
        }
    };
    let (s1, s2, s3, s4) = (
        side(orient(a, b, c)),
        side(orient(a, b, d)),
        side(orient(c, d, a)),
        side(orient(c, d, b)),
    );
    s1 * s2 < 0 && s3 * s4 < 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossing_chords_are_found_and_touching_ones_are_not() {
        assert!(cross([0.0, 0.0], [2.0, 2.0], [0.0, 2.0], [2.0, 0.0]));
        // Ending on the other, sharing an end, collinear: no crossing.
        assert!(!cross([0.0, 0.0], [2.0, 0.0], [1.0, 0.0], [1.0, 1.0]));
        assert!(!cross([0.0, 0.0], [2.0, 0.0], [2.0, 0.0], [3.0, 1.0]));
        assert!(!cross([0.0, 0.0], [2.0, 0.0], [1.0, 0.0], [3.0, 0.0]));
    }
}
