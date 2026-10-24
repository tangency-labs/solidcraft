//! Dimension continuation (`solve_after_value_edit`): the triconnected linkage family (two
//! triangles joined by three links, left to the iteration by the construction) under random
//! dimension edits from its previous solved state, with every failure that remains proven
//! infeasible; and the solve without continuation when nothing it could help with changed.
use solidcraft_geom::Vec2;

use crate::construct::switch;
use crate::model::{ConstraintKind, PointIdx, Sketch};
use crate::solver::{DrivingValues, solve, solve_after_value_edit};

struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + self.next() * (hi - lo)
    }
}

/// The linkage's solved layout: base triangle 0 1 2 (0 fixed, 0–1 horizontal), top triangle
/// 3 4 5, links 0–3, 1–4, 2–5.
const LAYOUT: [(f64, f64); 6] = [(0.0, 0.0), (40.0, 0.0), (18.0, 30.0), (8.0, 55.0), (52.0, 48.0), (30.0, 80.0)];
const PAIRS: [(usize, usize); 9] = [(0, 1), (1, 2), (2, 0), (3, 4), (4, 5), (5, 3), (0, 3), (1, 4), (2, 5)];

/// The linkage in its solved layout with every distance edited by `lo..hi`×, and the values
/// the layout was solved for (per constraint). With `lines`, each triangle side is a line of
/// its own (end points coincident with the corners) with a length dimension.
fn linkage(rng: &mut Lcg, lo: f64, hi: f64, lines: bool) -> (Sketch, DrivingValues) {
    let mut sk = Sketch::new();
    let ids: Vec<PointIdx> = LAYOUT.iter().map(|p| sk.add_point(Vec2::new(p.0, p.1), None).unwrap()).collect();
    sk.add_constraint(ConstraintKind::Fix { p: ids[0] }, None).unwrap();
    let l = sk.add_line_pts(ids[0], ids[1], None).unwrap();
    sk.add_constraint(ConstraintKind::Horizontal { l }, None).unwrap();
    for (k, (a, b)) in PAIRS.into_iter().enumerate() {
        let (pa, pb) = (sk.points[ids[a]].pos, sk.points[ids[b]].pos);
        if lines && k < 6 {
            let l = sk.add_line(pa, pb, None, None, None).unwrap();
            let (s, e) = sk.curves[l].kind.ends().unwrap();
            sk.add_constraint(ConstraintKind::Coincident { p: s, q: ids[a] }, None).unwrap();
            sk.add_constraint(ConstraintKind::Coincident { p: e, q: ids[b] }, None).unwrap();
            sk.add_constraint(ConstraintKind::Length { l, value: pa.dist(pb) }, None).unwrap();
        } else {
            sk.add_constraint(ConstraintKind::Distance { p: ids[a], q: ids[b], value: pa.dist(pb) }, None).unwrap();
        }
    }
    let old = sk.driving_values();
    for c in &mut sk.constraints {
        if let Some(v) = c.kind.value() {
            c.kind.set_value(v * rng.range(lo, hi));
        }
    }
    (sk, old)
}

/// The nine distances of a linkage sketch, in `PAIRS` order.
fn distances(sk: &Sketch) -> Vec<f64> {
    sk.constraints.iter().filter_map(|c| c.kind.value()).collect()
}

/// Turn of both triangles (layout point `i` is sketch point `i + 1`, after the origin).
fn turns(sk: &Sketch) -> [bool; 2] {
    let p = |i: usize| sk.points[PointIdx(i + 1)].pos;
    [(p(1) - p(0)).cross(p(2) - p(0)) > 0.0, (p(4) - p(3)).cross(p(5) - p(3)) > 0.0]
}

/// Both intersections of two circles; `None` when they miss by more than rounding.
fn circles(c0: Vec2, r0: f64, c1: Vec2, r1: f64) -> Option<[Vec2; 2]> {
    let d = c0.dist(c1);
    let a = (r0 * r0 - r1 * r1 + d * d) / (2.0 * d);
    let h2 = r0 * r0 - a * a;
    if h2 < -1e-9 * r0 * r0 {
        return None;
    }
    let u = (c1 - c0) / d;
    let m = c0 + u * a;
    let h = h2.max(0.0).sqrt();
    Some([m + u.perp() * h, m - u.perp() * h])
}

/// Whether the linkage with distances `d` (`PAIRS` order) has a real solution, decided by
/// branch and bound. With the base triangle placed (one of its four mirror images) and the
/// top triangle's shape fixed (one of two), a solution is an angle φ of link 0–3 and a turn θ
/// of the top triangle where the links 1–4 and 2–5 have their lengths. Over a cell of
/// half-width w in (φ, θ) the points move at most (d03 + top radius)·w, so a cell whose centre
/// misses both lengths by more than that holds no solution: when every cell is discarded so,
/// none exists. A cell that cannot be discarded down to w ≈ 1e-10 holds one within rounding.
/// `keep`: only solutions with both triangles turning so. `near`: only solutions with every
/// point within the distance of the given layout (discarded the same way).
fn feasible(d: &[f64], keep: Option<[bool; 2]>, near: Option<(&[Vec2], f64)>) -> bool {
    let [d01, d12, d20, d34, d45, d53, d03, d14, d25] = d.try_into().unwrap();
    let Some(tops) = circles(Vec2::ZERO, d53, Vec2::new(d34, 0.0), d45) else { return false };
    let q4 = Vec2::new(d34, 0.0);
    let lip = d03 + d34.max(d53);
    let far = |i: usize, at: Vec2, slack: f64| near.is_some_and(|(layout, r)| layout[i].dist(at) > r + slack);
    for s1 in [1.0, -1.0] {
        let p1 = Vec2::new(s1 * d01, 0.0);
        let Some(p2s) = circles(Vec2::ZERO, d20, p1, d12) else { return false };
        for p2 in p2s.into_iter().filter(|p2| keep.is_none_or(|k| (p1.cross(*p2) > 0.0) == k[0])) {
            if far(1, p1, 0.0) || far(2, p2, 0.0) {
                continue;
            }
            for q5 in tops.into_iter().filter(|q5| keep.is_none_or(|k| (q4.cross(*q5) > 0.0) == k[1])) {
                let miss = |phi: f64, theta: f64, w: f64| {
                    let rot = |q: Vec2| Vec2::new(q.x * theta.cos() - q.y * theta.sin(), q.x * theta.sin() + q.y * theta.cos());
                    let p3 = Vec2::from_angle(phi) * d03;
                    let (p4, p5) = (p3 + rot(q4), p3 + rot(q5));
                    if far(3, p3, lip * w) || far(4, p4, lip * w) || far(5, p5, lip * w) {
                        return f64::INFINITY;
                    }
                    (p1.dist(p4) - d14).abs().max((p2.dist(p5) - d25).abs())
                };
                let n = 64;
                let w0 = std::f64::consts::PI / n as f64;
                let mut stack: Vec<(f64, f64, f64)> = Vec::new();
                for i in 0..n {
                    for j in 0..n {
                        stack.push(((2 * i + 1) as f64 * w0, (2 * j + 1) as f64 * w0, w0));
                    }
                }
                while let Some((phi, theta, w)) = stack.pop() {
                    if miss(phi, theta, w) > lip * w * (1.0 + 1e-9) + 1e-12 {
                        continue;
                    }
                    if w < 1e-10 {
                        return true;
                    }
                    let h = w / 2.0;
                    for (a, b) in [(-h, -h), (-h, h), (h, -h), (h, h)] {
                        stack.push((phi + a, theta + b, h));
                    }
                }
            }
        }
    }
    false
}

/// The edit's distances `t` of the way from old to new.
fn along(old: &[f64], new: &[f64], t: f64) -> Vec<f64> {
    old.iter().zip(new).map(|(o, n)| o + t * (n - o)).collect()
}

/// Outcome counts over the family.
#[derive(Debug, Default)]
struct Count {
    one_jump_failed: usize,
    one_jump_folded: usize,
    /// Turned a triangle over (named in the report, for a value edit to refuse).
    refused: usize,
    /// Failed otherwise.
    failed: usize,
    /// Of the refused and failed: the new values have no solution.
    infeasible: usize,
    /// The new values are solved only with a triangle mirrored.
    only_mirrored: usize,
    /// The sketch's own solution ends on the straight value path: followed in 1/4096 steps
    /// it fails or folds at some t < 1, and no orientation-keeping solution for the values at
    /// t lies within 1 mm of where it was one step before (a fold of the path, or no solution).
    branch_ends: usize,
    /// Refused or failed cases none of the above explains: old and new distances.
    unexplained: Vec<(Vec<f64>, Vec<f64>)>,
}

/// The linkage's distances set to `d`.
fn set_distances(sk: &mut Sketch, d: &[f64]) {
    for (c, v) in sk.constraints.iter_mut().filter(|c| c.kind.value().is_some()).zip(d) {
        c.kind.set_value(*v);
    }
}

/// The value path from `old` to `new` in 1/4096 steps, each solved by the iteration alone
/// from the previous solution: where it fails or folds a triangle (t and the layout one step
/// before), or `None` when it reaches the new values.
fn fine_path(sk: &Sketch, old: &[f64], new: &[f64]) -> Option<(f64, Vec<Vec2>)> {
    let mut s = sk.clone();
    for (p, xy) in s.points.iter_mut().skip(1).zip(LAYOUT) {
        p.pos = Vec2::new(xy.0, xy.1);
    }
    let before = turns(&s);
    let n = 4096;
    for k in 1..=n {
        let t = k as f64 / n as f64;
        let last: Vec<Vec2> = s.points.iter().skip(1).map(|p| p.pos).collect();
        set_distances(&mut s, &along(old, new, t));
        let rep = switch::without_differential(|| switch::without_seed(|| solve(&mut s)));
        if !rep.ok() || turns(&s) != before {
            return Some((t, last));
        }
    }
    None
}

/// `n` edits of the family: the one-jump solve (LM from the layout) against
/// `solve_after_value_edit`, which must solve keeping both triangles' turn or refuse with the
/// layout kept; every refusal and failure is explained (see [`Count`]).
fn run_family(seed: u64, n: usize, lo: f64, hi: f64, lines: bool) -> Count {
    let mut rng = Lcg(seed);
    let mut count = Count::default();
    for _ in 0..n {
        let (sk, old) = linkage(&mut rng, lo, hi, lines);
        let before = turns(&sk);
        let (mut jump, mut cont) = (sk.clone(), sk.clone());
        let (a, b) = switch::without_differential(|| (solve(&mut jump), solve_after_value_edit(&mut cont, &old)));
        count.one_jump_failed += usize::from(!a.ok());
        count.one_jump_folded += usize::from(a.ok() && turns(&jump) != before);
        if a.ok() && turns(&jump) == before {
            assert_eq!(jump.points, cont.points, "a one-jump solve that keeps its triangles is the result");
        }
        if b.keeps_orientation() {
            assert_eq!(turns(&cont), before, "an accepted edit keeps every triangle's turn");
            continue;
        }
        if b.folded.is_empty() {
            count.failed += 1;
            assert_eq!(cont.points, sk.points, "a failed solve keeps the layout");
        } else {
            // Solved, turned over: kept as the best result, named for a command to refuse.
            count.refused += 1;
            assert!(b.ok() && turns(&cont) != before, "{b:?}");
            let dims: Vec<&str> = sk.constraints.iter().filter(|c| c.kind.value().is_some()).map(|c| c.id.as_str()).collect();
            assert!(b.folded.iter().all(|id| dims.contains(&id.as_str()) && b.failing.contains(id)), "{b:?}");
        }
        let new = distances(&sk);
        let old: Vec<f64> = sk.constraints.iter().filter_map(|c| old.get(&c.id).copied()).collect();
        if !feasible(&new, None, None) {
            count.infeasible += 1;
        } else if !feasible(&new, Some(before), None) {
            count.only_mirrored += 1;
        } else if let Some((t, last)) = fine_path(&sk, &old, &new)
            && !feasible(&along(&old, &new, t), Some(before), Some((&last, 1.0)))
        {
            count.branch_ends += 1;
        } else {
            count.unexplained.push((old, new));
        }
    }
    count
}

fn report(name: &str, c: &Count) {
    eprintln!(
        "{name}: one jump failed {} folded {}; continuation refused {} failed {} (infeasible {}, only mirrored {}, branch ends {}, unexplained {})",
        c.one_jump_failed,
        c.one_jump_folded,
        c.refused,
        c.failed,
        c.infeasible,
        c.only_mirrored,
        c.branch_ends,
        c.unexplained.len()
    );
    for u in c.unexplained.iter().take(3) {
        eprintln!("  {u:?}");
    }
}

#[test]
fn feasibility_check_agrees_with_the_layout() {
    let d: Vec<f64> = PAIRS.iter().map(|&(a, b)| Vec2::new(LAYOUT[a].0, LAYOUT[a].1).dist(Vec2::new(LAYOUT[b].0, LAYOUT[b].1))).collect();
    assert!(feasible(&d, None, None));
    let mut long = d.clone();
    long[7] = 500.0;
    assert!(!feasible(&long, None, None));
    let mut flat = d;
    flat[0] = flat[1] + flat[2] + 1.0;
    assert!(!feasible(&flat, None, None));
}

#[test]
fn linkage_edits_are_solved_by_continuation_or_refused_with_reason() {
    // The first 40 edits of the two families of `linkage_family_counter`, with every refusal
    // and failure explained.
    let c = run_family(2026, 40, 0.8, 1.25, false);
    report("linkage 0.8–1.25× (40)", &c);
    assert!(c.unexplained.is_empty() && c.branch_ends > 0 && c.failed + c.refused < c.one_jump_failed + c.one_jump_folded, "{c:?}");
    let c = run_family(7, 40, 0.5, 2.5, true);
    report("linkage 0.5–2.5× lines (40)", &c);
    assert!(c.unexplained.is_empty() && c.refused > 0, "{c:?}");
}

/// Measurement, not a gate: the two families, 500 edits each, every refusal and failure
/// explained (run with `--ignored --nocapture`).
#[test]
#[ignore]
fn linkage_family_counter() {
    for (seed, lo, hi, lines) in [(2026, 0.8, 1.25, false), (7, 0.5, 2.5, true)] {
        let c = run_family(seed, 500, lo, hi, lines);
        report(&format!("linkage {lo}–{hi}× lines {lines} (500)"), &c);
        assert!(c.unexplained.is_empty(), "{c:?}");
    }
}

#[test]
fn edits_from_an_unsolved_state_are_not_continued() {
    // Old values the layout is not solved for: no solved state to step from, nor orientations
    // to keep; the one-jump result stands.
    let mut rng = Lcg(2026);
    let mut tried = 0;
    for _ in 0..200 {
        let (sk, old) = linkage(&mut rng, 0.8, 1.25, false);
        let wrong: DrivingValues = old.iter().map(|(k, v)| (k.clone(), v * 1.1)).collect();
        let (mut a, mut b) = (sk.clone(), sk.clone());
        let (ra, rb) = (solve(&mut a), solve_after_value_edit(&mut b, &wrong));
        if ra.ok() {
            continue;
        }
        tried += 1;
        assert_eq!(rb.continuations, 0, "{rb:?}");
        assert!(rb.folded.is_empty());
        assert_eq!(a.points, b.points);
    }
    assert!(tried > 0);
}

#[test]
fn unchanged_values_solve_as_without_old_values() {
    let mut rng = Lcg(1);
    for _ in 0..20 {
        let (sk, _) = linkage(&mut rng, 0.8, 1.25, true);
        let current = sk.driving_values();
        let (mut a, mut b) = (sk.clone(), sk.clone());
        let (ra, rb) = (solve(&mut a), solve_after_value_edit(&mut b, &current));
        assert_eq!((ra.status, ra.iterations, ra.dof), (rb.status, rb.iterations, rb.dof));
        assert_eq!(rb.continuations, 0);
        assert_eq!(a.points, b.points);
    }
}

#[test]
fn hostile_old_values_never_panic() {
    let mut rng = Lcg(3);
    let (sk, old) = linkage(&mut rng, 0.5, 2.5, false);
    let all = |v: f64| -> DrivingValues { old.keys().map(|k| (k.clone(), v)).collect() };
    let unknown: DrivingValues = (0..20).map(|i| (format!("nope{i}"), 1.0)).collect();
    for bad in [DrivingValues::new(), all(f64::NAN), all(f64::INFINITY), all(-1e300), all(0.0), unknown, old.clone()] {
        let mut s = sk.clone();
        let rep = solve_after_value_edit(&mut s, &bad);
        assert!(rep.max_residual.is_finite());
        assert!(s.points.iter().all(|p| p.pos.x.is_finite() && p.pos.y.is_finite()));
    }
}
