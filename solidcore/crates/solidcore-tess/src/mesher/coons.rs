//! Structured meshing of a four-cornered face whose sides are not parameter lines (a fillet
//! strip with mitred ends, the wall of a cross hole): a transfinite (Coons) blend of the four
//! sides gives an interior grid in (u, v), joined to the shared edge nodes by zipped strips.
//! A blend that folds (a strongly non-convex boundary) is refused, and the face goes to the
//! constrained Delaunay path.

use solidcore::Vec3;
use crate::vec::VecExt as _;

/// Smallest turn (radians) of the boundary at a corner, in the tolerance metric.
const CORNER_TURN: f64 = 0.6;

/// One side: boundary vertex indices from one corner to the next, with each node's position
/// along the side (0 to 1, by length in 3D).
struct Side {
    nodes: Vec<u32>,
    at: Vec<f64>,
    /// Its length in 3D (mm).
    len: f64,
}

/// The four corners of a single boundary loop, by how sharply the loop turns there in (u, v)
/// scaled by `scale` (one unit per tolerance cell); `None` unless there are exactly four.
fn corners(uv: &[[f64; 2]], scale: [f64; 2]) -> Option<[usize; 4]> {
    let [a, b, c, d] = candidates(uv, scale)?[..] else { return None };
    Some([a, b, c, d])
}

/// Every node where the loop turns more than [`CORNER_TURN`] (or meets a pole copy), in loop
/// order.
fn candidates(uv: &[[f64; 2]], scale: [f64; 2]) -> Option<Vec<usize>> {
    let n = uv.len();
    if n < 4 {
        return None;
    }
    let p = |i: usize| uv.get(i % n).map(|q| [q[0] * scale[0], q[1] * scale[1]]);
    let mut found = Vec::new();
    for i in 0..n {
        let (a, b, c) = (p(i + n - 1)?, p(i)?, p(i + 1)?);
        let (d0, d1) = ([b[0] - a[0], b[1] - a[1]], [c[0] - b[0], c[1] - b[1]]);
        let (l0, l1) = (d0[0].hypot(d0[1]), d1[0].hypot(d1[1]));
        // Two copies of a pole meet with no length between them: that is a corner.
        if l0 == 0.0 || l1 == 0.0 {
            found.push(i);
            continue;
        }
        let turn = (d0[0] * d1[1] - d0[1] * d1[0]).atan2(d0[0] * d1[0] + d0[1] * d1[1]).abs();
        if turn > CORNER_TURN {
            found.push(i);
        }
    }
    Some(found)
}

/// Most segments a patch's own level gives one direction.
const MAX_LEVEL: usize = 4096;

/// Most corner candidates [`coons_best`] chooses four from (8 choose 4 = 70 blends at most).
const MAX_CANDIDATES: usize = 8;

/// Like [`coons`] for a loop with more than four corners (a patch of a trimmed face with a
/// corner cut off, the wing of a butterfly around a hole): every choice of four of its corners
/// is blended, and the one whose smallest triangle angle (in the metric `scale`) is largest
/// wins. Its other corners lie on sides. `None` if no choice gives a blend that does not fold.
pub(crate) fn coons_best(uv: &[[f64; 2]], fixed: &[Vec3], scale: [f64; 2], max_points: usize, h: f64) -> Option<(Vec<[f64; 2]>, Vec<[u32; 3]>)> {
    let c = candidates(uv, scale)?;
    if c.len() < 4 || c.len() > MAX_CANDIDATES {
        return None;
    }
    let mut best: Option<(f64, (Vec<[f64; 2]>, Vec<[u32; 3]>))> = None;
    let k = c.len();
    for a in 0..k {
        for b in a + 1..k {
            for d in b + 1..k {
                for e in d + 1..k {
                    let pick = [*c.get(a)?, *c.get(b)?, *c.get(d)?, *c.get(e)?];
                    let Some(r) = coons_at(uv, fixed, pick, max_points, h, None) else { continue };
                    let q = min_angle(uv, &r, scale);
                    if best.as_ref().is_none_or(|(bq, _)| q > *bq) {
                        best = Some((q, r));
                    }
                }
            }
        }
    }
    best.map(|(_, r)| r)
}

/// Smallest corner angle (radians) of the triangles, in the metric.
fn min_angle(uv: &[[f64; 2]], r: &(Vec<[f64; 2]>, Vec<[u32; 3]>), scale: [f64; 2]) -> f64 {
    let at = |i: u32| {
        let i = i as usize;
        let q = if i < uv.len() { uv.get(i) } else { r.0.get(i - uv.len()) };
        q.map(|q| [q[0] * scale[0], q[1] * scale[1]])
    };
    let mut worst = std::f64::consts::PI;
    for t in &r.1 {
        let (Some(a), Some(b), Some(c)) = (at(t[0]), at(t[1]), at(t[2])) else { continue };
        for (p, q, s) in [(a, b, c), (b, c, a), (c, a, b)] {
            let (u, v) = ([q[0] - p[0], q[1] - p[1]], [s[0] - p[0], s[1] - p[1]]);
            let ang = (u[0] * v[1] - u[1] * v[0]).abs().atan2(u[0] * v[0] + u[1] * v[1]);
            worst = worst.min(ang);
        }
    }
    worst
}

fn side(fixed: &[Vec3], n: usize, from: usize, to: usize) -> Option<Side> {
    let len = (to + n - from) % n;
    if len == 0 {
        return None;
    }
    let nodes: Vec<u32> = (0..=len).map(|k| ((from + k) % n) as u32).collect();
    let mut at = vec![0.0];
    let mut total = 0.0;
    for w in nodes.windows(2) {
        total += fixed.get(w[0] as usize)?.dist(*fixed.get(w[1] as usize)?);
        at.push(total);
    }
    if !(total > 0.0) {
        // A pole side: evenly along it.
        let m = (nodes.len() - 1).max(1) as f64;
        return Some(Side { at: (0..nodes.len()).map(|k| k as f64 / m).collect(), nodes, len: 0.0 });
    }
    Some(Side { nodes, at: at.into_iter().map(|x| x / total).collect(), len: total })
}

/// The point of a side at position `s` (0 to 1), linear between its nodes.
fn along(side: &Side, uv: &[[f64; 2]], s: f64) -> Option<[f64; 2]> {
    let k = side.at.partition_point(|x| *x < s).clamp(1, side.at.len().saturating_sub(1).max(1));
    let (a0, a1) = (*side.at.get(k - 1)?, *side.at.get(k)?);
    let (p, q) = (uv.get(*side.nodes.get(k - 1)? as usize)?, uv.get(*side.nodes.get(k)? as usize)?);
    let f = if a1 > a0 { ((s - a0) / (a1 - a0)).clamp(0.0, 1.0) } else { 0.0 };
    Some([p[0] + (q[0] - p[0]) * f, p[1] + (q[1] - p[1]) * f])
}

/// Interior line positions for a pair of opposite sides: the sides' own when they have as many
/// nodes as each other (so the grid meets both node for node); else evenly at the inner level a
/// GPU tessellator takes, the larger of the two sides' (outer) levels, and never coarser than the
/// size rules ask (the sides' mean length over the target side `h`). The denser side then meets
/// the grid node for node and the sparser one steps down to it through its ring.
fn lines(a: &Side, b: &Side, h: f64) -> Vec<f64> {
    let inner = |s: &Side| s.at.get(1..s.at.len().saturating_sub(1)).map(<[f64]>::to_vec).unwrap_or_default();
    if a.at.len() == b.at.len() {
        let l: Vec<f64> = inner(a).iter().zip(inner(b)).map(|(x, y)| 0.5 * (x + y)).collect();
        if !l.is_empty() {
            return l;
        }
    }
    let size = if h > 0.0 { (0.5 * (a.len + b.len) / h).ceil() } else { 1.0 };
    let size = if size.is_finite() { size as usize } else { 1 };
    let outer = a.at.len().max(b.at.len()).saturating_sub(1);
    let n = outer.max(size).clamp(2, MAX_LEVEL);
    (1..n).map(|i| i as f64 / n as f64).collect()
}

/// Triangulate the loop `uv` (with its fixed 3D nodes) as a Coons patch: boundary vertices
/// keep their indices, the interior grid follows. Triangles counter-clockwise in (u, v).
///
/// `fine(a, b)`: whether the surface between two parameter points needs no point between them.
/// Given for a whole face, whose sides are its edges' nodes: where its grid converges (towards a
/// pole) the interior can need more lines than the sides carry, and the lines are split there.
/// A motorcycle patch passes `None`: its sides are cuts seeded at the face's global seed.
pub(crate) fn coons(
    uv: &[[f64; 2]],
    fixed: &[Vec3],
    scale: [f64; 2],
    max_points: usize,
    h: f64,
    fine: Option<&dyn Fn([f64; 2], [f64; 2]) -> bool>,
) -> Option<(Vec<[f64; 2]>, Vec<[u32; 3]>)> {
    let pick = corners(uv, scale)?;
    coons_at(uv, fixed, pick, max_points, h, fine)
}

/// [`coons`] with the four corners given (loop indices, in loop order).
pub(crate) fn coons_at(
    uv: &[[f64; 2]],
    fixed: &[Vec3],
    pick: [usize; 4],
    max_points: usize,
    h: f64,
    fine: Option<&dyn Fn([f64; 2], [f64; 2]) -> bool>,
) -> Option<(Vec<[f64; 2]>, Vec<[u32; 3]>)> {
    let n = uv.len();
    let [c0, c1, c2, c3] = pick;
    // Sides in loop order; the bottom (0) and right (1) run with the loop, the top (2) and
    // left (3) against it, so s and t both run from corner c0.
    let bottom = side(fixed, n, c0, c1)?;
    let right = side(fixed, n, c1, c2)?;
    let top = reversed(side(fixed, n, c2, c3)?);
    let left = reversed(side(fixed, n, c3, c0)?);
    let corner = |i: usize| uv.get(i).copied();
    let (p00, p10, p11, p01) = (corner(c0)?, corner(c1)?, corner(c2)?, corner(c3)?);
    // The blend at (s, t): the sides' points, less the corners counted twice.
    let at = |s: f64, t: f64| -> Option<[f64; 2]> {
        let (b, tp, l, r) = (along(&bottom, uv, s)?, along(&top, uv, s)?, along(&left, uv, t)?, along(&right, uv, t)?);
        let blend = |d: usize| {
            (1.0 - t) * b[d] + t * tp[d] + (1.0 - s) * l[d] + s * r[d]
                - ((1.0 - s) * (1.0 - t) * p00[d] + s * (1.0 - t) * p10[d] + (1.0 - s) * t * p01[d] + s * t * p11[d])
        };
        Some([blend(0), blend(1)])
    };
    // The lines come from the sides' seeds (see [`lines`]): every side is seeded at the face's
    // global seed, which the size rules set for the whole surface, so the interior needs no finer
    // spacing than its sides.
    let (ss, ts) = (lines(&bottom, &top, h), lines(&left, &right, h));
    let (ss, ts) = match fine {
        Some(fine) => {
            (refine(ss, max_points, &|a, b, x| Some(fine(at(a, x)?, at(b, x)?))), refine(ts, max_points, &|a, b, x| Some(fine(at(x, a)?, at(x, b)?))))
        }
        None => (ss, ts),
    };
    if ss.len().checked_mul(ts.len())? > max_points {
        return None;
    }
    let mut out: Vec<[f64; 2]> = uv.to_vec();
    let base = u32::try_from(n).ok()?;
    for &t in &ts {
        for &s in &ss {
            out.push(at(s, t)?);
        }
    }
    let (ns, nt) = (ss.len(), ts.len());
    let g = |i: usize, j: usize| base + (j * ns + i) as u32;
    let mut tris = Vec::new();
    for j in 0..nt.saturating_sub(1) {
        for i in 0..ns.saturating_sub(1) {
            tris.push([g(i, j), g(i + 1, j), g(i + 1, j + 1)]);
            tris.push([g(i, j), g(i + 1, j + 1), g(i, j + 1)]);
        }
    }
    // Strips: each side against its grid row or column, by position along the side. In (s, t)
    // the bottom and right strips come out counter-clockwise, the top and left clockwise.
    let row = |j: usize| (0..ns).map(|i| g(i, j)).collect::<Vec<u32>>();
    let col = |i: usize| (0..nt).map(|j| g(i, j)).collect::<Vec<u32>>();
    // Where a side's segments and its grid row's differ, a transition row steps between them
    // halfway to the first grid line (its segments the two counts' geometric mean), each zipped
    // to it, with one triangle at each end closing it against the corner.
    let (s_first, s_last) = (*ss.first()?, *ss.last()?);
    let (t_first, t_last) = (*ts.first()?, *ts.last()?);
    // Each side: its grid row, the row's positions, the orientation, where the transition row
    // runs, and the point at a position along it. The row lies halfway between the side and the
    // grid row; its ends lie halfway between the diagonal from the corner to the grid row's end
    // and that end, so they are strictly inside the side's own ring.
    #[allow(clippy::type_complexity)]
    let sides: [(&Side, Vec<u32>, &[f64], bool, (f64, f64), &dyn Fn(f64) -> [f64; 2]); 4] = [
        (&bottom, row(0), &ss, false, (0.75 * s_first, 1.0 - 0.75 * (1.0 - s_last)), &|x| [x, 0.5 * t_first]),
        (&right, col(ns - 1), &ts, false, (0.75 * t_first, 1.0 - 0.75 * (1.0 - t_last)), &|x| [0.5 * (1.0 + s_last), x]),
        (&top, row(nt - 1), &ss, true, (0.75 * s_first, 1.0 - 0.75 * (1.0 - s_last)), &|x| [x, 0.5 * (1.0 + t_last)]),
        (&left, col(0), &ts, true, (0.75 * t_first, 1.0 - 0.75 * (1.0 - t_last)), &|x| [0.5 * s_first, x]),
    ];
    for (side, inner, inner_at, flip, (x0, x1), place) in sides {
        let (outer_segs, inner_segs) = (side.nodes.len().saturating_sub(1), inner.len() + 1);
        let mean = ((outer_segs * inner_segs) as f64).sqrt().round() as usize;
        if mean == outer_segs || mean == inner_segs || mean < 2 {
            zip(side, &inner, inner_at, flip, &mut tris)?;
            continue;
        }
        let k = mean - 1;
        let at_t: Vec<f64> = (0..k).map(|j| if k > 1 { x0 + (x1 - x0) * j as f64 / (k - 1) as f64 } else { 0.5 * (x0 + x1) }).collect();
        let mut mid: Vec<u32> = Vec::with_capacity(k);
        for &x in &at_t {
            let [sx, tx] = place(x);
            mid.push(u32::try_from(out.len()).ok()?);
            out.push(at(sx, tx)?);
        }
        zip(side, &mid, &at_t, flip, &mut tris)?;
        zip(&Side { nodes: mid.clone(), at: at_t.clone(), len: 0.0 }, &inner, inner_at, flip, &mut tris)?;
        // The two end triangles: corner, transition end, grid end, turned like the zips' (in
        // (s, t) the first runs with them and the last against). Never turned to fit the blend:
        // a blend that folds there leaves one inverted, and the patch is refused below.
        for t in [[*side.nodes.first()?, *mid.first()?, *inner.first()?], [*side.nodes.last()?, *inner.last()?, *mid.last()?]] {
            tris.push(if flip { [t[0], t[2], t[1]] } else { t });
        }
    }
    // The (s, t) square maps to (u, v) keeping or reversing orientation, with the loop: all
    // triangles must share one sign, which is then made counter-clockwise.
    let area = |t: &[u32; 3]| -> Option<f64> {
        let (a, b, c) = (out.get(t[0] as usize)?, out.get(t[1] as usize)?, out.get(t[2] as usize)?);
        Some((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]))
    };
    let first = area(tris.first()?)?;
    let sign = if first > 0.0 { 1.0 } else { -1.0 };
    for t in &tris {
        if !(area(t)? * sign > 0.0) {
            return None;
        }
    }
    if sign < 0.0 {
        for t in &mut tris {
            t.swap(1, 2);
        }
    }
    let extra = out.split_off(n);
    Some((extra, tris))
}

/// Interior lines (strictly between 0 and 1) with every interval, ends included, split while
/// `fine(a, b, x)` fails at some position `x` across (or the lines reach `max`).
fn refine(lines: Vec<f64>, max: usize, fine: &dyn Fn(f64, f64, f64) -> Option<bool>) -> Vec<f64> {
    const ACROSS: [f64; 5] = [0.0, 0.25, 0.5, 0.75, 1.0];
    let all: Vec<f64> = std::iter::once(0.0).chain(lines).chain(std::iter::once(1.0)).collect();
    let mut out = vec![0.0];
    let mut stack: Vec<(f64, f64, u32)> = all.windows(2).rev().map(|w| (w[0], w[1], 0)).collect();
    while let Some((a, b, depth)) = stack.pop() {
        let ok = depth >= 16 || out.len() + stack.len() >= max || ACROSS.iter().all(|x| fine(a, b, *x).unwrap_or(true));
        if ok {
            out.push(b);
        } else {
            let m = 0.5 * (a + b);
            stack.push((m, b, depth + 1));
            stack.push((a, m, depth + 1));
        }
    }
    out.into_iter().filter(|x| *x > 0.0 && *x < 1.0).collect()
}

fn reversed(s: Side) -> Side {
    Side { nodes: s.nodes.into_iter().rev().collect(), at: s.at.into_iter().rev().map(|x| 1.0 - x).collect(), len: s.len }
}

/// Join a side (its nodes at positions `side.at`) to a grid row at positions `inner_at`.
fn zip(side: &Side, inner: &[u32], inner_at: &[f64], flip: bool, tris: &mut Vec<[u32; 3]>) -> Option<()> {
    let mut push = |a: u32, b: u32, c: u32| tris.push(if flip { [a, c, b] } else { [a, b, c] });
    let (outer, oat) = (&side.nodes, &side.at);
    let (mut a, mut b) = (0usize, 0usize);
    while a + 1 < outer.len() || b + 1 < inner.len() {
        let advance_outer = if a + 1 >= outer.len() {
            false
        } else if b + 1 >= inner.len() {
            true
        } else {
            oat.get(a + 1)? <= inner_at.get(b + 1)?
        };
        if advance_outer {
            push(*outer.get(a)?, *outer.get(a + 1)?, *inner.get(b)?);
            a += 1;
        } else {
            push(*outer.get(a)?, *inner.get(b + 1)?, *inner.get(b)?);
            b += 1;
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(uv: &[[f64; 2]], tris: &[[u32; 3]]) -> f64 {
        tris.iter()
            .map(|t| {
                let (a, b, c) = (uv[t[0] as usize], uv[t[1] as usize], uv[t[2] as usize]);
                0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]))
            })
            .sum()
    }

    /// A trapezoid (a strip with mitred ends), counter-clockwise, `k` nodes per side.
    fn trapezoid(k: usize) -> Vec<[f64; 2]> {
        let corners = [[0.0, 0.0], [10.0, 0.0], [8.0, 2.0], [2.0, 2.0]];
        let mut b = Vec::new();
        for w in 0..4 {
            let (p, q) = (corners[w], corners[(w + 1) % 4]);
            for i in 0..k {
                let f = i as f64 / k as f64;
                b.push([p[0] + (q[0] - p[0]) * f, p[1] + (q[1] - p[1]) * f]);
            }
        }
        b
    }

    #[test]
    fn trapezoid_is_tiled_exactly() {
        for k in [2, 5] {
            for reverse in [false, true] {
                let mut b = trapezoid(k);
                if reverse {
                    b.reverse();
                }
                let fixed: Vec<Vec3> = b.iter().map(|p| Vec3::new(p[0], p[1], 0.0)).collect();
                let Some((extra, tris)) = coons(&b, &fixed, [1.0, 1.0], 10_000, 1.0, None) else { panic!("meshes (k {k})") };
                let mut uv = b.clone();
                uv.extend(extra);
                assert!((area(&uv, &tris) - 16.0).abs() < 1e-9, "k {k} reverse {reverse}: {}", area(&uv, &tris));
            }
        }
    }

    /// Opposite sides with different node counts (20 against 4): the interior grid takes the
    /// larger side's level (a tessellator's inner level), so the dense side meets it node for node
    /// and the sparse side steps down to it through a transition row. Covered exactly, nothing
    /// missing or doubled.
    #[test]
    fn the_interior_level_is_the_larger_sides_and_the_sparser_side_steps_down() {
        // A 4 × 4 square: bottom in 20 segments, the other sides in 4.
        let mut b: Vec<[f64; 2]> = (0..20).map(|k| [k as f64 * 0.2, 0.0]).collect();
        b.extend((0..4).map(|k| [4.0, k as f64]));
        b.extend((0..4).map(|k| [4.0 - k as f64, 4.0]));
        b.extend((0..4).map(|k| [0.0, 4.0 - k as f64]));
        let fixed: Vec<Vec3> = b.iter().map(|p| Vec3::new(p[0], p[1], 0.0)).collect();
        let Some((extra, tris)) = coons(&b, &fixed, [1.0, 1.0], 10_000, 1.0, None) else { panic!("meshes") };
        let mut uv = b.clone();
        uv.extend(extra.iter().copied());
        assert!((area(&uv, &tris) - 16.0).abs() < 1e-9);
        // Level 20 along s (the bottom's), 4 along t: 19 × 3 interior points; plus the transition
        // row by the 4-segment top (√(4 · 20) ≈ 9 segments: 8 points).
        assert_eq!(extra.len(), 19 * 3 + 8, "{}", extra.len());
        // Every inner side shared by two triangles, every boundary side used once: nothing
        // missing or doubled at the transition's ends.
        let mut sides: std::collections::HashMap<(u32, u32), i32> = std::collections::HashMap::new();
        for t in &tris {
            for e in 0..3 {
                *sides.entry((t[e], t[(e + 1) % 3])).or_default() += 1;
            }
        }
        let n = b.len() as u32;
        for (&(a, c), &k) in &sides {
            assert_eq!(k, 1, "side {a}-{c} once each way");
            let boundary = a < n && c < n && (c == (a + 1) % n);
            assert!(boundary || sides.contains_key(&(c, a)), "inner side {a}-{c} shared");
        }
    }

    #[test]
    fn five_corners_is_not_a_coons_patch() {
        let b = [[0.0, 0.0], [1.0, 0.0], [1.0, 0.5], [0.5, 1.0], [0.0, 1.0]];
        let fixed: Vec<Vec3> = b.iter().map(|p| Vec3::new(p[0], p[1], 0.0)).collect();
        assert!(coons(&b, &fixed, [1.0, 1.0], 100, 1.0, None).is_none());
    }

    /// A planar strip with a fillet at one end and a chamfer at the other, in mm (a face of
    /// robustness seed 33): its corners make the one-segment chamfer a side facing the long one,
    /// and at some target sides the blend folds beside the chamfer. The transition row's end
    /// triangles are turned like its zips, never to fit the blend, so a fold leaves a triangle
    /// inverted and the blend is refused; any blend given covers the loop exactly once.
    #[test]
    #[allow(clippy::approx_constant)] // measured coordinates, one of them near ln 10
    fn a_blend_folding_beside_a_short_side_is_refused_not_overlapped() {
        let b: Vec<[f64; 2]> = vec![
            [24.0000, 3.0150],
            [23.0000, 4.4292],
            [16.6667, 4.4292],
            [10.3333, 4.4292],
            [4.0000, 4.4292],
            [4.0000, 3.0150],
            [4.0002, 2.9836],
            [4.0010, 2.9520],
            [4.0023, 2.9201],
            [4.0040, 2.8880],
            [4.0064, 2.8556],
            [4.0092, 2.8231],
            [4.0127, 2.7905],
            [4.0167, 2.7577],
            [4.0213, 2.7248],
            [4.0265, 2.6919],
            [4.0322, 2.6589],
            [4.0386, 2.6259],
            [4.0456, 2.5929],
            [4.0532, 2.5600],
            [4.0614, 2.5272],
            [4.0702, 2.4945],
            [4.0797, 2.4619],
            [4.0897, 2.4295],
            [4.1004, 2.3974],
            [4.1117, 2.3655],
            [4.1236, 2.3339],
            [4.1362, 2.3026],
            [4.1493, 2.2717],
            [4.1630, 2.2412],
            [4.1773, 2.2111],
            [4.1922, 2.1814],
            [4.2076, 2.1523],
            [4.2236, 2.1237],
            [4.2402, 2.0956],
            [4.2573, 2.0681],
            [4.2748, 2.0412],
            [4.2929, 2.0150],
            [4.3114, 1.9895],
            [4.3304, 1.9646],
            [4.3499, 1.9405],
            [4.3697, 1.9171],
            [4.3899, 1.8945],
            [4.4105, 1.8726],
            [4.4315, 1.8516],
            [4.4528, 1.8313],
            [4.4744, 1.8119],
            [4.4962, 1.7934],
            [4.5184, 1.7757],
            [4.5407, 1.7588],
            [4.5633, 1.7428],
            [4.5860, 1.7277],
            [4.6089, 1.7135],
            [4.6319, 1.7001],
            [4.6550, 1.6876],
            [4.6783, 1.6760],
            [4.7015, 1.6653],
            [4.7248, 1.6554],
            [4.7482, 1.6464],
            [4.7715, 1.6382],
            [4.7948, 1.6309],
            [4.8180, 1.6244],
            [4.8412, 1.6188],
            [4.8643, 1.6139],
            [4.8873, 1.6098],
            [4.9101, 1.6065],
            [4.9329, 1.6040],
            [4.9554, 1.6022],
            [4.9778, 1.6012],
            [5.0000, 1.6008],
            [11.3333, 1.6008],
            [17.6667, 1.6008],
            [24.0000, 1.6008],
        ];
        let fixed: Vec<Vec3> = b.iter().map(|p| Vec3::new(p[0], p[1], 0.0)).collect();
        let n = b.len();
        let loop_area: f64 = (0..n).map(|k| 0.5 * (b[k][0] * b[(k + 1) % n][1] - b[(k + 1) % n][0] * b[k][1])).sum();
        for h in [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0] {
            let Some((extra, tris)) = coons(&b, &fixed, [1.0, 1.0], 100_000, h, None) else { continue };
            let mut uv = b.clone();
            uv.extend(extra);
            assert!((area(&uv, &tris) - loop_area).abs() < 1e-9 * loop_area, "h {h}: {} for {loop_area}", area(&uv, &tris));
            let mut sides: std::collections::HashMap<(u32, u32), usize> = std::collections::HashMap::new();
            for t in &tris {
                for k in 0..3 {
                    *sides.entry((t[k], t[(k + 1) % 3])).or_default() += 1;
                }
            }
            assert!(sides.values().all(|&k| k == 1), "h {h}: a side used twice the same way");
        }
    }
}
