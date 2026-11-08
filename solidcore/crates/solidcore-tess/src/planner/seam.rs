//! Faces on a closed surface whose boundary winds around it: a cylinder face bounded by two
//! circles and no seam edge, or one trimmed by a helix that turns several times (a modelled
//! thread, as other CAD systems write them to STEP). In the surface's unwrapped parameters such
//! a face repeats every period, and its loops do not close. It is cut along a parameter line
//! (the seam it never had) into one period: every loop, and every copy of it a whole number of
//! periods along, is clipped to the strip between the line and its next copy, and the parts of
//! the two lines inside the face join the pieces into closed loops. Both lines are one curve
//! on the surface, so their nodes are made once and shared: the face stays closed across it.

use solidcore::eval::EvalSurface;
use solidcore_convert as kernel;
use solidcore_record::Vec3;

use super::NoValue;
use super::size::Rules;

/// Why a face could not be cut into one period.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Cut {
    /// The loops cannot be cut cleanly: the face would be unbounded along the line, or the two
    /// lines disagree.
    Unclean,
    /// The surface does not evaluate at a node of the cut line.
    NoValue(NoValue),
}

/// One loop of a face: parameters (unwrapped along the loop) and the 3D node each stands for.
#[derive(Clone, Debug, Default)]
pub(crate) struct Loop {
    pub uv: Vec<[f64; 2]>,
    pub pts: Vec<Vec3>,
}

/// Most loop copies one face may be clipped into (a helix of this many turns).
const MAX_COPIES: i64 = 2048;
/// Most nodes a cut line may get between two crossings.
const MAX_SEAM_NODES: usize = 4096;

/// The copies `a..=b` of a loop (whole numbers, as `f64`), as integers: `None` where they are
/// not finite, run backwards, or number more than [`MAX_COPIES`]. Compared in `f64` before the
/// casts, so no cast saturates and no subtraction of the integers overflows.
fn copies(a: f64, b: f64) -> Option<(i64, i64)> {
    // Well inside i64, and exact in f64: every whole number up to 2^53 is.
    const LIMIT: f64 = 9_007_199_254_740_992.0;
    let fits = |x: f64| x.abs() <= LIMIT;
    // Proven: both are whole numbers of magnitude at most 2^53 (checked, NaN failing every
    // comparison), so the casts are exact.
    (fits(a) && fits(b) && a <= b && b - a <= MAX_COPIES as f64).then_some((a as i64, b as i64))
}

/// A run along a cut line joining two pieces: the piece it leaves, its nodes' parameters and
/// points, and the piece it enters.
type Link = (usize, Vec<[f64; 2]>, Vec<Vec3>, usize);

/// How far `l` drifts along `d` going once around it (a whole number of periods for a closed
/// loop: zero unless it winds around the surface).
pub(crate) fn drift(l: &Loop, d: usize, per: f64) -> f64 {
    let (Some(&first), Some(&last)) = (
        l.uv.first().and_then(|q| q.get(d)),
        l.uv.last().and_then(|q| q.get(d)),
    ) else {
        return 0.0;
    };
    let close = solidcore::eval::uv::unwrap(first, last, Some(per)) - last;
    last - first + close
}

/// Whether any loop winds around the closed direction `d`.
pub(crate) fn winds(loops: &[Loop], d: usize, per: f64) -> bool {
    loops.iter().any(|l| drift(l, d, per).abs() > per * 0.5)
}

/// A point where the boundary crosses a cut line: its place along the line, its 3D point, and
/// whether the face lies above it (towards larger parameters across the line) or below.
#[derive(Clone, Copy, Debug)]
struct Crossing {
    y: f64,
    p: Vec3,
    above: bool,
}

/// A boundary run across the strip: it enters through one cut line and leaves through one.
struct Piece {
    uv: Vec<[f64; 2]>,
    pts: Vec<Vec3>,
}

/// Cut a face whose loops wind around direction `d` (period `per`) into one period. The face
/// lies left of its loops in (u, v). [`Cut::Unclean`] when the loops cannot be cut cleanly (the
/// face would be unbounded along the line, or the two lines disagree); [`Cut::NoValue`] when the
/// surface does not evaluate at a node of the cut line (the evaluator refuses the face).
pub(crate) fn cut(
    s: &EvalSurface,
    loops: &[Loop],
    d: usize,
    per: f64,
    rules: &Rules,
    h: f64,
) -> Result<Vec<Loop>, Cut> {
    if !(per > 0.0 && per.is_finite()) || loops.is_empty() {
        return Err(Cut::Unclean);
    }
    // Work in (x, y) with x along the closed direction; swapping (u, v) mirrors the plane, so the
    // loops are reversed to keep the face on their left.
    let swap = d == 1;
    let local: Vec<Loop> = loops
        .iter()
        .map(|l| {
            let mut uv: Vec<[f64; 2]> =
                l.uv.iter()
                    .map(|q| if swap { [q[1], q[0]] } else { *q })
                    .collect();
            let mut pts = l.pts.clone();
            if swap {
                uv.reverse();
                pts.reverse();
            }
            Loop { uv, pts }
        })
        .collect();
    let x0 = cut_position(&local, per).ok_or(Cut::Unclean)?;
    let x1 = x0 + per;
    let mut closed: Vec<Loop> = Vec::new();
    let mut pieces: Vec<Piece> = Vec::new();
    for l in &local {
        let n = l.uv.len();
        if n < 2 || l.pts.len() != n {
            return Err(Cut::Unclean);
        }
        let w = drift(l, 0, per);
        let (lo, hi) =
            l.uv.iter()
                .fold((f64::MAX, f64::MIN), |(a, b), q| (a.min(q[0]), b.max(q[0])));
        if !(lo.is_finite() && hi.is_finite()) {
            return Err(Cut::Unclean);
        }
        if w.abs() <= per * 0.5 {
            // A closed loop: each copy along x that reaches into the strip.
            let (k0, k1) =
                copies(((x0 - hi) / per).floor(), ((x1 - lo) / per).ceil()).ok_or(Cut::Unclean)?;
            for k in k0..=k1 {
                let shift = per * k as f64;
                let uv: Vec<[f64; 2]> = l.uv.iter().map(|q| [q[0] + shift, q[1]]).collect();
                if uv.iter().all(|q| q[0] > x0 && q[0] < x1) {
                    closed.push(Loop {
                        uv,
                        pts: l.pts.clone(),
                    });
                } else if uv.iter().any(|q| q[0] > x0 && q[0] < x1) || crosses(&uv, x0, x1) {
                    // Start at a node outside the strip, so every run inside is whole.
                    let start = uv
                        .iter()
                        .position(|q| !(q[0] > x0 && q[0] < x1))
                        .ok_or(Cut::Unclean)?;
                    // Proven: n = l.uv.len() >= 2, checked above
                    let idx: Vec<usize> = (0..=n).map(|i| (start + i) % n).collect();
                    let path: Vec<[f64; 2]> =
                        idx.iter().filter_map(|&i| uv.get(i).copied()).collect();
                    let pts: Vec<Vec3> =
                        idx.iter().filter_map(|&i| l.pts.get(i).copied()).collect();
                    clip(&path, &pts, x0, x1, &mut pieces);
                }
            }
        } else {
            // A loop winding around: the endless path it repeats along, over enough turns to
            // start and end outside the strip.
            let turns = (w / per).round();
            if turns.abs() != 1.0 {
                return Err(Cut::Unclean);
            }
            // Compared in f64, before any cast: a NaN or a reach past the bound is unclean.
            let span = ((hi - lo) / per).ceil() + 2.0;
            if span.is_nan() || span > MAX_COPIES as f64 {
                return Err(Cut::Unclean);
            }
            // Copies j with lo + j·w ≤ x1 and hi + j·w ≥ x0, plus one each side.
            let (ja, jb) = if w > 0.0 {
                (((x0 - hi) / w).floor() - 1.0, ((x1 - lo) / w).ceil() + 1.0)
            } else {
                (((x1 - lo) / w).floor() - 1.0, ((x0 - hi) / w).ceil() + 1.0)
            };
            let (ja, jb) = copies(ja, jb).ok_or(Cut::Unclean)?;
            let mut path = Vec::new();
            let mut pts = Vec::new();
            for j in ja..=jb {
                let shift = w * j as f64;
                path.extend(l.uv.iter().map(|q| [q[0] + shift, q[1]]));
                pts.extend(l.pts.iter().copied());
            }
            let (Some(a), Some(b)) = (path.first(), path.last()) else {
                return Err(Cut::Unclean);
            };
            if (a[0] > x0 && a[0] < x1) || (b[0] > x0 && b[0] < x1) {
                return Err(Cut::Unclean);
            }
            clip(&path, &pts, x0, x1, &mut pieces);
        }
    }
    // The crossings on each line, sorted along it; both lines must see the same ones.
    let mut left: Vec<(Crossing, usize, bool)> = Vec::new();
    let mut right: Vec<(Crossing, usize, bool)> = Vec::new();
    for (i, p) in pieces.iter().enumerate() {
        let (Some(a), Some(b), Some(pa), Some(pb)) =
            (p.uv.first(), p.uv.last(), p.pts.first(), p.pts.last())
        else {
            return Err(Cut::Unclean);
        };
        // Entering through the left line (moving +x): the face is above; through the right
        // line (moving −x): below. Leaving through the left (−x): below; the right (+x): above.
        let enter = Crossing {
            y: a[1],
            p: *pa,
            above: a[0] == x0,
        };
        let leave = Crossing {
            y: b[1],
            p: *pb,
            above: b[0] == x1,
        };
        if a[0] == x0 {
            left.push((enter, i, true));
        } else {
            right.push((enter, i, true));
        }
        if b[0] == x0 {
            left.push((leave, i, false));
        } else {
            right.push((leave, i, false));
        }
    }
    left.sort_by(|a, b| a.0.y.total_cmp(&b.0.y));
    right.sort_by(|a, b| a.0.y.total_cmp(&b.0.y));
    if left.len() != right.len() || !left.len().is_multiple_of(2) {
        return Err(Cut::Unclean);
    }
    let scale = loops
        .iter()
        .flat_map(|l| l.uv.iter())
        .fold(1.0f64, |m, q| m.max(q[0].abs()).max(q[1].abs()));
    for (a, b) in left.iter().zip(&right) {
        if a.0.above != b.0.above || (a.0.y - b.0.y).abs() > 1e-7 * scale {
            return Err(Cut::Unclean);
        }
    }
    // The same crossing on both lines is one point.
    for (a, b) in left.clone().iter().zip(right.iter_mut()) {
        b.0.y = a.0.y;
        b.0.p = a.0.p;
    }
    for (c, i, start) in left.iter().chain(&right) {
        let p = pieces.get_mut(*i).ok_or(Cut::Unclean)?;
        let k = if *start {
            0
        } else {
            p.uv.len().checked_sub(1).ok_or(Cut::Unclean)?
        };
        if let (Some(q), Some(pt)) = (p.uv.get_mut(k), p.pts.get_mut(k)) {
            q[1] = c.y;
            *pt = c.p;
        }
    }
    // Inside the face along each line: from a crossing with the face above to the next one
    // up (which must have it below).
    let mut links: Vec<Link> = Vec::new();
    // Proven: both chunk sizes are the constant 2, not zero.
    for pair in left.chunks(2).zip(right.chunks(2)) {
        let (lp, rp) = pair;
        let (Some(la), Some(lb), Some(ra), Some(rb)) =
            (lp.first(), lp.get(1), rp.first(), rp.get(1))
        else {
            return Err(Cut::Unclean);
        };
        if !la.0.above || lb.0.above {
            return Err(Cut::Unclean);
        }
        // Nodes strictly between, made once for both lines.
        let inner = seam_nodes(s, swap, x0, la.0.y, lb.0.y, rules, h).map_err(Cut::NoValue)?;
        // Left line, downwards: from the piece leaving at the top to the one entering at the bottom.
        let (top_leave, bottom_enter) = (lb.1, la.1);
        let down_uv: Vec<[f64; 2]> = inner.iter().rev().map(|(y, _)| [x0, *y]).collect();
        let down_pts: Vec<Vec3> = inner.iter().rev().map(|(_, p)| *p).collect();
        if lb.2 || !la.2 {
            return Err(Cut::Unclean);
        }
        links.push((top_leave, down_uv, down_pts, bottom_enter));
        // Right line, upwards: from the piece leaving at the bottom to the one entering at the top.
        if ra.2 || !rb.2 {
            return Err(Cut::Unclean);
        }
        let up_uv: Vec<[f64; 2]> = inner.iter().map(|(y, _)| [x1, *y]).collect();
        let up_pts: Vec<Vec3> = inner.iter().map(|(_, p)| *p).collect();
        links.push((ra.1, up_uv, up_pts, rb.1));
    }
    // Chain pieces through the links into closed loops.
    let mut next: Vec<Option<usize>> = vec![None; pieces.len()];
    let mut via: Vec<Option<usize>> = vec![None; pieces.len()];
    for (li, (from, _, _, to)) in links.iter().enumerate() {
        if next.get(*from).copied().flatten().is_some() {
            return Err(Cut::Unclean);
        }
        *next.get_mut(*from).ok_or(Cut::Unclean)? = Some(*to);
        *via.get_mut(*from).ok_or(Cut::Unclean)? = Some(li);
    }
    let mut used = vec![false; pieces.len()];
    for start in 0..pieces.len() {
        if used.get(start).copied().unwrap_or(true) {
            continue;
        }
        let mut out = Loop::default();
        let mut at = start;
        for _ in 0..=pieces.len() {
            if *used.get(at).ok_or(Cut::Unclean)? {
                break;
            }
            *used.get_mut(at).ok_or(Cut::Unclean)? = true;
            let p = pieces.get(at).ok_or(Cut::Unclean)?;
            out.uv.extend(&p.uv);
            out.pts.extend(&p.pts);
            let (_, luv, lpts, _) = via
                .get(at)
                .copied()
                .flatten()
                .and_then(|li| links.get(li))
                .ok_or(Cut::Unclean)?;
            out.uv.extend(luv);
            out.pts.extend(lpts);
            at = next.get(at).copied().flatten().ok_or(Cut::Unclean)?;
        }
        if at != start {
            return Err(Cut::Unclean);
        }
        closed.push(out);
    }
    // Back to (u, v).
    let out: Vec<Loop> = closed
        .into_iter()
        .map(|mut l| {
            if swap {
                l.uv = l.uv.iter().rev().map(|q| [q[1], q[0]]).collect();
                l.pts.reverse();
            }
            l
        })
        .collect();
    // The face must come out with positive area (left of its loops).
    let area: f64 = out
        .iter()
        .map(|l| {
            // Each node with the next one round the loop.
            l.uv.iter()
                .zip(l.uv.iter().cycle().skip(1))
                .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
                .sum::<f64>()
        })
        .sum();
    if area > 0.0 {
        Ok(out)
    } else {
        Err(Cut::Unclean)
    }
}

/// Where to cut: the middle of the widest gap between the loops' nodes around the period, as
/// far from any node as can be.
fn cut_position(loops: &[Loop], per: f64) -> Option<f64> {
    let mut xs: Vec<f64> = loops
        .iter()
        .flat_map(|l| l.uv.iter().map(|q| q[0].rem_euclid(per)))
        .collect();
    xs.sort_by(f64::total_cmp);
    let &first = xs.first()?;
    let mut best = (0.0, f64::NEG_INFINITY);
    for (i, x) in xs.iter().enumerate() {
        let next = xs.get(i + 1).copied().unwrap_or(first + per);
        if next - x > best.1 {
            best = (0.5 * (x + next), next - x);
        }
    }
    (best.1 > 0.0).then_some(best.0)
}

/// Whether a path jumps over the strip in one segment.
fn crosses(uv: &[[f64; 2]], x0: f64, x1: f64) -> bool {
    uv.iter()
        .zip(uv.iter().skip(1))
        .any(|(a, b)| (a[0] <= x0 && b[0] >= x1) || (b[0] <= x0 && a[0] >= x1))
}

/// The runs of an open path (starting and ending outside the strip) inside the strip, each
/// from the point where it enters to the one where it leaves. The crossing points lie on the
/// boundary's own straight 3D segments, so the neighbouring face's edge passes through them.
fn clip(path: &[[f64; 2]], pts: &[Vec3], x0: f64, x1: f64, out: &mut Vec<Piece>) {
    let inside = |q: &[f64; 2]| q[0] > x0 && q[0] < x1;
    let mut cur: Option<Piece> = None;
    for ((a, b), (pa, pb)) in path
        .iter()
        .zip(path.iter().skip(1))
        .zip(pts.iter().zip(pts.iter().skip(1)))
    {
        let at = |x: f64| {
            // Proven: the clamp bounds are the constants 0.0 <= 1.0, neither NaN.
            let t = ((x - a[0]) / (b[0] - a[0])).clamp(0.0, 1.0);
            ([x, a[1] + (b[1] - a[1]) * t], *pa + (*pb - *pa) * t)
        };
        // Lines crossed by this segment, in order along it.
        let mut hits: Vec<f64> = [x0, x1]
            .into_iter()
            .filter(|x| (a[0] - x) * (b[0] - x) < 0.0)
            .collect();
        if b[0] < a[0] {
            hits.reverse();
        }
        for x in hits {
            let (q, p) = at(x);
            match cur.take() {
                Some(mut piece) => {
                    piece.uv.push(q);
                    piece.pts.push(p);
                    out.push(piece);
                }
                None => {
                    cur = Some(Piece {
                        uv: vec![q],
                        pts: vec![p],
                    });
                }
            }
        }
        if inside(b)
            && let Some(piece) = cur.as_mut()
        {
            piece.uv.push(*b);
            piece.pts.push(*pb);
        }
    }
}

/// Nodes along the cut line between two crossings (`y0` < `y1`, ends excluded): within the
/// tolerance, the side length and the facet angle of the surface along the line. The parameters
/// of a node the surface does not evaluate at are the error.
fn seam_nodes(
    s: &EvalSurface,
    swap: bool,
    x: f64,
    y0: f64,
    y1: f64,
    rules: &Rules,
    h: f64,
) -> Result<Vec<(f64, Vec3)>, NoValue> {
    let uv = |y: f64| if swap { [y, x] } else { [x, y] };
    let mut ys = vec![y0, y1];
    let mut i = 0;
    while let (Some(&a), Some(&b)) = (ys.get(i), ys.get(i + 1)) {
        if ys.len() < MAX_SEAM_NODES
            && !super::uv::fine_between(s, rules, h, uv(a), uv(b))?
            && b - a > 1e-12 * (1.0 + a.abs())
        {
            // Proven: i + 1 < ys.len(), as `ys.get(i + 1)` is Some
            ys.insert(i + 1, 0.5 * (a + b));
        } else {
            i += 1;
        }
    }
    // `ys` starts as [y0, y1] and only grows, so its inner nodes are all but its first and last.
    let inner = ys.iter().skip(1).take(ys.len().saturating_sub(2));
    inner
        .map(|&y| {
            s.derivs(uv(y)[0], uv(y)[1])
                .map(|d| (y, kernel::record_vec3(d.0)))
                .ok_or_else(|| NoValue(uv(y)))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A seamless face on a revolution whose surface has no point on the circle u = 0.5 (see
    /// [`super::super::refusals::cylinder_without`]): its two loops, the circles
    /// u = 0 and u = 1 a whole turn round, wind round it and are cut along a line v = const from
    /// u = 0 to u = 1. That line's node at u = 0.5 does not evaluate: the cut names it, not drops
    /// it from the line.
    #[test]
    fn the_cut_names_a_node_the_surface_does_not_evaluate_at() {
        let s = super::super::refusals::cylinder_without(0.5);
        let tau = std::f64::consts::TAU;
        let n = 16;
        let circle = |u: f64, up: bool| {
            let uv: Vec<[f64; 2]> = (0..n)
                .map(|k| {
                    [
                        u,
                        tau * f64::from(if up { k } else { n - k }) / f64::from(n),
                    ]
                })
                .collect();
            let pts: Vec<Vec3> = uv
                .iter()
                .filter_map(|q| s.derivs(q[0], q[1]).map(|d| kernel::record_vec3(d.0)))
                .collect();
            assert_eq!(pts.len(), uv.len());
            Loop { uv, pts }
        };
        // The face lies left of its loops: up the circle at u = 1, down the one at u = 0.
        let loops = [circle(1.0, true), circle(0.0, false)];
        assert!(winds(&loops, 1, tau));
        let rules = Rules::new(0.01, 40.0, None);
        match cut(&s, &loops, 1, tau, &rules, 1.0) {
            Err(Cut::NoValue(NoValue(q))) => assert_eq!(q[0], 0.5, "{q:?}"),
            Err(Cut::Unclean) => panic!("refused as unclean"),
            Ok(l) => panic!("cut into {} loops", l.len()),
        }
    }

    /// A loop whose copies along the closed direction run past what an `i64` holds (a loop
    /// spanning ±1e25 periods, rounding to a closed one): its first and last copies saturated in
    /// the casts and their difference overflowed. Compared in `f64` first, the cut refuses it as
    /// unclean.
    #[test]
    fn a_loop_whose_copies_overflow_an_integer_is_unclean() {
        let s = super::super::refusals::cylinder_without(0.5);
        let far = 1e25;
        let uv = vec![[0.25, -far], [0.25, far], [0.75, far], [0.75, -far]];
        let pts = vec![Vec3::new(0.0, 0.0, 0.0); uv.len()];
        let loops = [Loop { uv, pts }];
        let tau = std::f64::consts::TAU;
        assert!(!winds(&loops, 1, tau));
        let rules = Rules::new(0.01, 40.0, None);
        assert!(matches!(
            cut(&s, &loops, 1, tau, &rules, 1.0),
            Err(Cut::Unclean)
        ));
    }
}
