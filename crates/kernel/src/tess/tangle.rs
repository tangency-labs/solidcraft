//! Where a face's sampled boundary crosses itself: chords of its loops, in the face's
//! parameters, that cross another chord of the face although the curves they stand for do not
//! (two curves running closer together than their chords sag).

use solidcraft_geom::tess::EvalSurface;

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
}

impl Chord {
    fn lo(&self, d: usize) -> f64 {
        self.a[d].min(self.b[d])
    }
    fn hi(&self, d: usize) -> f64 {
        self.a[d].max(self.b[d])
    }
}

/// The chords (loop, first node) of `loops` that cross another chord of the face, in its loop or
/// another, each once and in order. On a closed surface the boundary crosses itself where any
/// copies of two chords, whole periods apart, cross; chords ending on a pole, where the
/// parameter along the pole is free, are left out. Adjacent chords of a loop share a node and
/// never cross. `reach`: how near a pole a node is taken as one (see [`uv::degenerate_within`]).
pub(crate) fn crossing(s: &EvalSurface, reach: f64, loops: &[seam::Loop]) -> Vec<(usize, usize)> {
    let dom = uv::domain(s);
    let pole = |q: [f64; 2]| (0..2).any(|d| uv::degenerate_within(s, &dom, q, d, reach));
    let mut chords: Vec<Chord> = Vec::new();
    for (li, l) in loops.iter().enumerate() {
        let n = l.uv.len();
        if n < 3 {
            continue;
        }
        for k in 0..n {
            let (Some(&a), Some(&b)) = (l.uv.get(k), l.uv.get((k + 1) % n)) else { continue };
            if !(a.iter().chain(&b).all(|x| x.is_finite())) || pole(a) || pole(b) {
                continue;
            }
            // The loop's closing chord of a loop that winds round the surface ends at the copy of
            // its first node nearest its last; every chord then starts in one period.
            let (mut a, mut b) = (a, b);
            for d in 0..2 {
                let Some(per) = dom.period[d] else { continue };
                b[d] = uv::unwrap(b[d], a[d], Some(per));
                let back = (a[d].min(b[d]) / per).floor() * per;
                a[d] -= back;
                b[d] -= back;
            }
            chords.push(Chord { at: (li, k), a, b });
        }
    }
    // With every chord starting in one period, two chords' copies can only cross where one of
    // them is moved a period along at most: each chord also as its copies a period along.
    for d in 0..2 {
        let Some(per) = dom.period[d] else { continue };
        let along: Vec<Chord> = chords
            .iter()
            .map(|c| {
                let (mut a, mut b) = (c.a, c.b);
                a[d] += per;
                b[d] += per;
                Chord { at: c.at, a, b }
            })
            .collect();
        chords.extend(along);
    }
    let adjacent = |p: (usize, usize), q: (usize, usize)| {
        let n = loops.get(p.0).map_or(0, |l| l.uv.len());
        p == q || (p.0 == q.0 && ((p.1 + 1) % n == q.1 || (q.1 + 1) % n == p.1))
    };
    // Sweep along u: only chords whose u ranges overlap can cross.
    chords.sort_by(|x, y| x.lo(0).total_cmp(&y.lo(0)));
    let mut out: Vec<(usize, usize)> = Vec::new();
    for (i, x) in chords.iter().enumerate() {
        for y in chords.iter().skip(i + 1) {
            if y.lo(0) > x.hi(0) {
                break;
            }
            if y.lo(1) > x.hi(1) || x.lo(1) > y.hi(1) || adjacent(x.at, y.at) {
                continue;
            }
            if cross(x.a, x.b, y.a, y.b) {
                out.push(x.at);
                out.push(y.at);
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

/// Whether the segments `ab` and `cd` cross: each one's ends strictly either side of the other.
fn cross(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let orient = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
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
    let (s1, s2, s3, s4) = (side(orient(a, b, c)), side(orient(a, b, d)), side(orient(c, d, a)), side(orient(c, d, b)));
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
