//! Profiles cut at the seams of a wall and the planes tangent to it (#5358, Task 3), in wall
//! coordinates: each seam is the line x = c, and the strip between seams is a zone (zone i lies
//! between seam lines i - 1 and i, seams in increasing x). A profile loop is cut into pieces, one
//! per zone it passes through between crossings; [`trace`] closes the runs inside one zone into
//! loops along the seam lines, as the classic clip of a region by a strip.
use solidcore_record::{Loop2, Seg2, Vec2};
use solidcraft_geom::prelude::*;

use super::{crossings, fail};
use crate::Result;

/// Where a loop crosses a seam line: the line, the height there, and its number in the emboss
/// (the vertex the pieces either side share).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Crossing {
    pub line: usize,
    pub z: f64,
    pub id: usize,
}

/// One piece of a profile segment, inside one zone, and the crossing it ends on, if it does.
#[derive(Clone, Debug)]
pub(super) struct Cut {
    pub seg: Seg2,
    /// The segment of the loop it is a piece of.
    pub of: usize,
    pub zone: usize,
    pub ends: Option<Crossing>,
}

/// Loop `lp` cut at the seam lines `seams` (increasing x). A profile that touches a seam
/// without crossing it cleanly (a corner within `touch` of the line, a segment along it, a
/// crossing tangent to it) is refused: its footprint would meet the seam at a point.
pub(super) fn cut_loop(lp: &Loop2, seams: &[f64], touch: f64, next_id: &mut usize) -> Result<Vec<Cut>> {
    let refuse = || fail("a profile touches the seam between the cylinder and a tangent plane: move it clear or across");
    let zone = |x: f64| seams.iter().filter(|c| **c < x).count();
    let mut out = Vec::new();
    for (of, seg) in lp.segs.iter().enumerate() {
        if seams.iter().any(|c| (seg.start().x - c).abs() <= touch || (seg.end().x - c).abs() <= touch) {
            return Err(refuse());
        }
        let mut ts: Vec<(f64, usize)> = Vec::new();
        for (line, c) in seams.iter().enumerate() {
            for t in crossings(seg, *c) {
                let d = seg.derivative(t);
                if d.x.abs() <= 1e-6 * d.len() {
                    return Err(refuse());
                }
                ts.push((t, line));
            }
        }
        ts.sort_by(|a, b| a.0.total_cmp(&b.0));
        if ts.windows(2).any(|w| w[1].0 - w[0].0 < 1e-9) {
            return Err(refuse());
        }
        let mut t0 = 0.0;
        for (t, line) in ts.iter().copied().chain(std::iter::once((1.0, usize::MAX))) {
            let piece = if t0 == 0.0 && t == 1.0 { *seg } else { seg.part(t0, t) };
            let ends = (line != usize::MAX).then(|| {
                let id = *next_id;
                *next_id += 1;
                Crossing { line, z: seg.point_at(t).y, id }
            });
            out.push(Cut { zone: zone(piece.point_at(0.5).x), seg: piece, of, ends });
            t0 = t;
        }
    }
    Ok(out)
}

/// A run of a loop inside one zone, from the crossing it enters by to the one it leaves by.
#[derive(Clone, Debug)]
pub(super) struct Run<T> {
    pub items: Vec<T>,
    pub from: Crossing,
    pub to: Crossing,
}

/// The pieces of cut loop `cut` in zone `zone`: its runs there (piece indices into `cut`), or
/// the whole loop when it crosses no seam and lies in the zone.
pub(super) fn runs_in(cut: &[Cut], zone: usize) -> (Vec<Run<usize>>, Option<Vec<usize>>) {
    let n = cut.len();
    let Some(last) = (0..n).rev().find(|i| cut.get(*i).is_some_and(|c| c.ends.is_some())) else {
        let whole = cut.iter().all(|c| c.zone == zone) && n > 0;
        return (Vec::new(), whole.then(|| (0..n).collect()));
    };
    let mut runs = Vec::new();
    let mut from = cut.get(last).and_then(|c| c.ends);
    let mut items = Vec::new();
    for k in 1..=n {
        let i = (last + k) % n;
        let Some(c) = cut.get(i) else { continue };
        items.push(i);
        if let Some(to) = c.ends {
            if let Some(f) = from
                && c.zone == zone
            {
                runs.push(Run { items: std::mem::take(&mut items), from: f, to });
            }
            items.clear();
            from = Some(to);
        }
    }
    (runs, None)
}

/// The loops a zone's runs close into: on each seam line the ends met there, in height order,
/// pair off as the intervals of the line inside the region (a run arriving at one end, one
/// leaving at the other), joined by `chord(line, from, to)`. Fails when the runs do not pair
/// that way (a region not lying on one side of its boundary).
pub(super) fn trace<T: Clone>(runs: &[Run<T>], mut chord: impl FnMut(Crossing, Crossing) -> T) -> Result<Vec<Vec<T>>> {
    let bad = || fail("profile pieces at a seam do not pair");
    let mut ends: Vec<(Crossing, usize, bool)> = Vec::with_capacity(runs.len() * 2);
    for (i, r) in runs.iter().enumerate() {
        ends.push((r.to, i, true));
        ends.push((r.from, i, false));
    }
    ends.sort_by(|a, b| a.0.line.cmp(&b.0.line).then(a.0.z.total_cmp(&b.0.z)));
    let mut next: Vec<Option<(usize, Crossing, Crossing)>> = vec![None; runs.len()];
    let mut k = 0;
    while k < ends.len() {
        let (Some(a), Some(b)) = (ends.get(k), ends.get(k + 1)) else { return Err(bad()) };
        if a.0.line != b.0.line || a.2 == b.2 {
            return Err(bad());
        }
        let (arrive, leave) = if a.2 { (a, b) } else { (b, a) };
        let slot = next.get_mut(arrive.1).ok_or_else(bad)?;
        *slot = Some((leave.1, arrive.0, leave.0));
        k += 2;
    }
    let mut seen = vec![false; runs.len()];
    let mut loops = Vec::new();
    for start in 0..runs.len() {
        if seen.get(start).copied().unwrap_or(true) {
            continue;
        }
        let mut lp = Vec::new();
        let mut i = start;
        for _ in 0..=runs.len() {
            let s = seen.get_mut(i).ok_or_else(bad)?;
            if *s {
                break;
            }
            *s = true;
            lp.extend(runs.get(i).ok_or_else(bad)?.items.iter().cloned());
            let (j, from, to) = next.get(i).copied().flatten().ok_or_else(bad)?;
            lp.push(chord(from, to));
            i = j;
        }
        if i != start {
            return Err(bad());
        }
        loops.push(lp);
    }
    Ok(loops)
}

/// The seam line's segment from crossing `from` to `to` (wall coordinates).
pub(super) fn chord_seg(seams: &[f64], from: Crossing, to: Crossing) -> Seg2 {
    let c = seams.get(from.line).copied().unwrap_or(f64::NAN);
    Seg2::Line { a: Vec2::new(c, from.z), b: Vec2::new(c, to.z) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore_record::Region2;

    /// Region `reg` clipped to zone `zone` by seams `seams`, as loops of segments.
    fn clip(reg: &Region2, seams: &[f64], zone: usize) -> Vec<Loop2> {
        let mut id = 0;
        let mut runs = Vec::new();
        let mut loops = Vec::new();
        for lp in reg.loops() {
            let cut = cut_loop(lp, seams, 1e-9, &mut id).unwrap();
            let (r, whole) = runs_in(&cut, zone);
            let segs = |items: &[usize]| items.iter().map(|i| cut[*i].seg).collect::<Vec<_>>();
            runs.extend(r.into_iter().map(|r| Run { items: segs(&r.items), from: r.from, to: r.to }));
            if let Some(w) = whole {
                loops.push(Loop2 { segs: segs(&w) });
            }
        }
        for segs in trace(&runs, |a, b| chord_seg(seams, a, b)).unwrap() {
            loops.push(Loop2 { segs });
        }
        loops
    }

    fn area(loops: &[Loop2]) -> f64 {
        loops.iter().map(Loop2::signed_area).sum()
    }

    #[test]
    fn a_region_cut_at_seams_closes_in_each_zone() {
        // A square ring (a hole in it) across the seam x = 1, and a circle across both x = 1 and
        // x = 3: each zone's loops close, every one of them, and their areas add up.
        let ring = Region2 {
            outer: Loop2::polygon(&[Vec2::new(-2., 0.), Vec2::new(4., 0.), Vec2::new(4., 6.), Vec2::new(-2., 6.)]),
            holes: vec![Loop2::polygon(&[Vec2::new(-1., 1.), Vec2::new(-1., 5.), Vec2::new(3., 5.), Vec2::new(3., 1.)])],
        };
        let left = clip(&ring, &[1.], 0);
        let right = clip(&ring, &[1.], 1);
        // Left: a C of 3 × 6 less 2 × 4; right: 3 × 6 less 2 × 4, closed in one loop each.
        assert_eq!((left.len(), right.len()), (1, 1));
        assert!((area(&left) - 10.).abs() < 1e-12 && (area(&right) - 10.).abs() < 1e-12, "{} {}", area(&left), area(&right));
        assert!(left.iter().chain(&right).all(|l| l.is_closed(1e-12)));
        let disc = Region2 { outer: Loop2::circle(Vec2::new(2., 0.), 1.5), holes: vec![] };
        let seams = [1., 3.];
        let zones: Vec<Vec<Loop2>> = (0..3).map(|z| clip(&disc, &seams, z)).collect();
        let total: f64 = zones.iter().map(|z| area(z)).sum();
        assert!((total - std::f64::consts::PI * 2.25).abs() < 1e-9, "{total}");
        // Symmetric about x = 2: the two caps match.
        assert!((area(&zones[0]) - area(&zones[2])).abs() < 1e-9);
        assert!(zones.iter().flatten().all(|l| l.is_closed(1e-9)));
        // A corner on the seam, or a side along it, is refused.
        let mut id = 0;
        let on = Loop2::polygon(&[Vec2::new(1., 0.), Vec2::new(2., 0.), Vec2::new(2., 1.), Vec2::new(1., 1.)]);
        assert!(cut_loop(&on, &[1.], 1e-9, &mut id).is_err());
        // A remnant: the zone's outline with the seam side left open, less the ring: the ring's
        // loops reversed, run with the outline's own run.
        let mut id = 10;
        let mut runs = Vec::new();
        let mut loops = Vec::new();
        for lp in ring.loops() {
            let cut = cut_loop(&lp.reversed(), &[1.], 1e-9, &mut id).unwrap();
            let (r, whole) = runs_in(&cut, 0);
            runs.extend(r.into_iter().map(|r| Run { items: r.items.iter().map(|i| cut[*i].seg).collect(), from: r.from, to: r.to }));
            loops.extend(whole.map(|w| Loop2 { segs: w.iter().map(|i| cut[*i].seg).collect() }));
        }
        let outline = Loop2::polygon(&[Vec2::new(1., 8.), Vec2::new(-5., 8.), Vec2::new(-5., -2.), Vec2::new(1., -2.)]);
        let mut segs = outline.segs.clone();
        segs.pop();
        runs.push(Run { items: segs, from: Crossing { line: 0, z: 8., id: 0 }, to: Crossing { line: 0, z: -2., id: 1 } });
        loops.extend(trace(&runs, |a, b| chord_seg(&[1.], a, b)).unwrap().into_iter().map(|segs| Loop2 { segs }));
        // The outline less the C, and the island inside the ring's hole: 6 × 10 − 10 = 50.
        assert_eq!(loops.len(), 2);
        assert!((area(&loops) - 50.).abs() < 1e-12, "{}", area(&loops));
    }
}
