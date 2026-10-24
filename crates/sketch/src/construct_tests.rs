//! Harness for the constructive seed: branch preservation on dimension edits, no freezing of
//! points that keep a degree of freedom, drag continuity, and hostile input. Deterministic
//! (fixed LCG seeds), no timing.

use solidcraft_geom::Vec2;

use crate::construct::{Placement, seed};
use crate::decompose::Budget;
use crate::solver::points_on_curves;
use crate::tests::{Lcg, given, seed_of, seeded, v};
use crate::*;

fn orientation(a: Vec2, b: Vec2, c: Vec2) -> f64 {
    (b - a).cross(c - a).signum()
}

/// Fan-triangulated k-gon (diagonals from vertex 0), vertices 0 and 1 fixed, every other
/// dimension scaled by a factor in `lo..hi`; only edits where every fan triangle still exists
/// are kept, so an orientation-preserving solution always exists. Returns (failed, mirrored)
/// over 500 edits.
fn fan(rnd: &mut Lcg, k: usize, lo: f64, hi: f64, on: bool) -> (usize, usize) {
    let (mut failed, mut mirrored, mut kept) = (0, 0, 0);
    while kept < 500 {
        let mut sk = Sketch::new();
        let ids: Vec<PointIdx> = (0..k)
            .map(|i| {
                let t = i as f64 / k as f64 * std::f64::consts::TAU;
                let r = 30.0 + rnd.next() * 10.0;
                sk.add_point(v(r * t.cos(), r * t.sin()), None).unwrap()
            })
            .collect();
        let pos = |sk: &Sketch, i: usize| sk.points[ids[i]].pos;
        let orient = |sk: &Sketch| (1..k - 1).map(|i| orientation(pos(sk, 0), pos(sk, i), pos(sk, i + 1))).collect::<Vec<_>>();
        let before = orient(&sk);
        let mut dist = std::collections::HashMap::new();
        for i in 1..k {
            let j = (i + 1) % k;
            dist.insert((i, j), pos(&sk, i).dist(pos(&sk, j)) * (lo + rnd.next() * (hi - lo)));
        }
        dist.insert((0, 1), pos(&sk, 0).dist(pos(&sk, 1)));
        for i in 2..k {
            dist.insert((0, i), pos(&sk, 0).dist(pos(&sk, i)) * (lo + rnd.next() * (hi - lo)));
        }
        dist.insert((0, k - 1), dist[&(k - 1, 0)]);
        let feasible = (1..k - 1).all(|i| {
            let (a, b, c) = (dist[&(0, i)], dist[&(i, i + 1)], dist[&(0, i + 1)]);
            a + b > c * 1.02 && a + c > b * 1.02 && b + c > a * 1.02
        });
        if !feasible {
            continue;
        }
        kept += 1;
        for &p in &ids[..2] {
            sk.add_constraint(ConstraintKind::Fix { p }, None).unwrap();
        }
        for i in 1..k {
            let j = (i + 1) % k;
            sk.add_constraint(ConstraintKind::Distance { p: ids[i], q: ids[j], value: dist[&(i, j)] }, None).unwrap();
        }
        for i in 2..k - 1 {
            sk.add_constraint(ConstraintKind::Distance { p: ids[0], q: ids[i], value: dist[&(0, i)] }, None).unwrap();
        }
        let rep = seeded(on, || solve(&mut sk));
        if !rep.ok() {
            failed += 1;
        } else if orient(&sk) != before {
            mirrored += 1;
        }
    }
    (failed, mirrored)
}

fn fan_table(on: bool) -> Vec<(f64, f64, usize, usize, usize)> {
    let mut rnd = Lcg(99);
    let mut out = Vec::new();
    for (lo, hi) in [(0.9, 1.15), (0.7, 1.6), (0.5, 2.5)] {
        for k in [4usize, 5, 6, 8, 12] {
            let (failed, mirrored) = fan(&mut rnd, k, lo, hi, on);
            out.push((lo, hi, k, failed, mirrored));
        }
    }
    out
}

#[test]
fn fan_polygons_keep_every_triangle_on_dimension_edits() {
    let table = fan_table(true);
    for (lo, hi, k, failed, mirrored) in &table {
        eprintln!("seed on  {lo}–{hi}× {k:>2}-gon: {failed} failed, {mirrored}/500 mirrored");
    }
    assert!(table.iter().all(|row| row.3 == 0 && row.4 == 0), "{table:?}");
}

/// The measurement the seed is judged against: without it, LM mirrors fan triangles.
#[test]
fn fan_polygons_mirror_without_the_seed() {
    let table = fan_table(false);
    for (lo, hi, k, failed, mirrored) in &table {
        eprintln!("seed off {lo}–{hi}× {k:>2}-gon: {failed} failed, {mirrored}/500 mirrored");
    }
    let wide_12 = table.iter().find(|r| r.0 == 0.5 && r.2 == 12).map(|r| r.4).unwrap();
    assert!(wide_12 > 100, "{table:?}");
}

/// Base fixed, apex near the base, both sides edited; also far from the origin.
fn triangle_edits(on: bool) -> Vec<(f64, usize, usize)> {
    let mut rnd = Lcg(99);
    let mut out = Vec::new();
    for off in [0.0, 1e4, 1e6] {
        let (mut flips, mut fails) = (0, 0);
        for _ in 0..2000 {
            let o = v(off, off);
            let mut sk = Sketch::new();
            let a = sk.add_point(o, None).unwrap();
            let b = sk.add_point(o + v(40.0, 0.0), None).unwrap();
            let c = sk.add_point(o + v(5.0 + rnd.next() * 30.0, 0.05 + rnd.next() * 3.0), None).unwrap();
            for p in [a, b] {
                sk.add_constraint(ConstraintKind::Fix { p }, None).unwrap();
            }
            let pc = sk.points[c].pos;
            sk.add_constraint(ConstraintKind::Distance { p: a, q: c, value: pc.dist(o) }, None).unwrap();
            sk.add_constraint(ConstraintKind::Distance { p: b, q: c, value: pc.dist(o + v(40.0, 0.0)) }, None).unwrap();
            let (r1, r2) = loop {
                let (r1, r2) = (5.0 + rnd.next() * 60.0, 5.0 + rnd.next() * 60.0);
                if r1 + r2 > 41.0 && (r1 - r2).abs() < 39.0 {
                    break (r1, r2);
                }
            };
            sk.constraints[2].kind.set_value(r1);
            sk.constraints[3].kind.set_value(r2);
            if !seeded(on, || solve(&mut sk)).ok() {
                fails += 1;
            } else if sk.points[c].pos.y < off {
                flips += 1;
            }
        }
        out.push((off, flips, fails));
    }
    out
}

#[test]
fn triangle_edits_keep_the_apex_side_far_from_the_origin() {
    for on in [false, true] {
        for (off, flips, fails) in triangle_edits(on) {
            eprintln!("seed {} triangle at offset {off:e}: {flips}/2000 mirrored, {fails} failed", if on { "on " } else { "off" });
            if on {
                assert_eq!((flips, fails), (0, 0), "offset {off:e}");
            }
        }
    }
}

/// Two triangles joined by three links: no point beyond the first triangle has two loci on
/// placed points, so the seed must leave them to LM (projecting each onto its one locus would
/// freeze a point that still has a degree of freedom).
fn linkage() -> (Sketch, Vec<PointIdx>) {
    const TRUE: [(f64, f64); 6] = [(0.0, 0.0), (40.0, 0.0), (18.0, 30.0), (8.0, 55.0), (52.0, 48.0), (30.0, 80.0)];
    const PAIRS: [(usize, usize); 9] = [(0, 1), (1, 2), (2, 0), (3, 4), (4, 5), (5, 3), (0, 3), (1, 4), (2, 5)];
    let mut sk = Sketch::new();
    let ids: Vec<PointIdx> = TRUE
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let at = if i == 0 { *p } else { (p.0 + 1.5 * ((i * 7 % 5) as f64 - 2.0), p.1 - 1.2 * ((i * 3 % 4) as f64 - 1.5)) };
            sk.add_point(v(at.0, at.1), None).unwrap()
        })
        .collect();
    sk.add_constraint(ConstraintKind::Fix { p: ids[0] }, None).unwrap();
    let l = sk.add_line_pts(ids[0], ids[1], None).unwrap();
    sk.add_constraint(ConstraintKind::Horizontal { l }, None).unwrap();
    for (a, b) in PAIRS {
        let value = v(TRUE[a].0, TRUE[a].1).dist(v(TRUE[b].0, TRUE[b].1));
        sk.add_constraint(ConstraintKind::Distance { p: ids[a], q: ids[b], value }, None).unwrap();
    }
    (sk, ids)
}

/// Two triangles joined by three links (as [`linkage`]), every distance scaled by 0.5–2.5×.
fn edited_linkage(rnd: &mut Lcg) -> Sketch {
    let (mut sk, _) = linkage();
    for c in sk.constraints.iter_mut() {
        if let Some(v) = c.kind.value() {
            c.kind.set_value(v * (0.5 + rnd.next() * 2.0));
        }
    }
    sk
}

/// Pinning the base triangle exactly can leave LM stuck on the rest where the plain start
/// solves; the solve then falls back to the plain start, so the seed never loses a solution.
#[test]
fn edited_linkages_never_fail_where_the_plain_start_solves() {
    let mut rnd = Lcg(7);
    let (mut lost, mut gained) = (Vec::new(), Vec::new());
    for i in 0..500 {
        let sk = edited_linkage(&mut rnd);
        let (mut a, mut b) = (sk.clone(), sk.clone());
        let on = seeded(true, || solve(&mut a)).ok();
        let off = seeded(false, || solve(&mut b)).ok();
        if off && !on {
            lost.push(i);
        }
        if on && !off {
            gained.push(i);
        }
    }
    eprintln!("edited linkages: seeded solves {} the plain start does not ({gained:?}), loses {lost:?}", gained.len());
    assert!(lost.is_empty(), "{lost:?}");
}

/// An edited linkage (case 28 of the family above) where LM from the seeded start alone fails
/// and the plain start solves.
#[test]
fn edited_linkage_the_seeded_start_alone_cannot_solve() {
    let (mut sk, _) = linkage();
    let values = [
        46.108603805595436,
        66.03877723361462,
        44.29474034815241,
        49.872556594908566,
        70.62035270311223,
        42.30949365900061,
        95.95247718421143,
        62.63491114263336,
        39.69273507189351,
    ];
    for (c, v) in sk.constraints.iter_mut().filter(|c| c.kind.value().is_some()).zip(values) {
        c.kind.set_value(v);
    }
    let rep = solve(&mut sk);
    assert!(rep.ok(), "{rep:?}");
}

#[test]
fn triconnected_linkage_still_solves_exactly() {
    let (mut sk, ids) = linkage();
    let s = seed_of(&sk);
    assert_eq!(s.placed_by[ids[0].0], Some(Placement::Given));
    assert!(matches!(s.placed_by[ids[1].0], Some(Placement::TwoLoci { transversal: true, .. })));
    assert!(matches!(s.placed_by[ids[2].0], Some(Placement::TwoLoci { transversal: true, .. })));
    for &p in &ids[3..] {
        assert_eq!(s.placed_by[p.0], None, "point {p} has one locus only");
    }
    let rep = solve(&mut sk);
    assert!(rep.ok(), "{rep:?}");
    assert_eq!(rep.dof, 0);
    let worst = sk
        .constraints
        .iter()
        .filter_map(|c| match c.kind {
            ConstraintKind::Distance { p, q, value } => Some((sk.points[p].pos.dist(sk.points[q].pos) - value).abs()),
            _ => None,
        })
        .fold(0.0, f64::max);
    eprintln!("linkage: {} iterations, worst distance error {worst:e}", rep.iterations);
    assert!(worst < 1e-9, "{worst}");
}

/// n×n grid, origin fixed, every row and column edge horizontal/vertical with a distance,
/// started ±1 mm off.
fn grid(n: usize) -> Sketch {
    let mut sk = Sketch::new();
    let mut rnd = Lcg(12345);
    let mut id = vec![vec![PointIdx::ORIGIN; n]; n];
    for (r, row) in id.iter_mut().enumerate() {
        for (c, slot) in row.iter_mut().enumerate() {
            let j = if r == 0 && c == 0 { v(0.0, 0.0) } else { v(rnd.next() * 2.0 - 1.0, rnd.next() * 2.0 - 1.0) };
            *slot = sk.add_point(v(c as f64 * 10.0, r as f64 * 10.0) + j, None).unwrap();
        }
    }
    sk.add_constraint(ConstraintKind::Fix { p: id[0][0] }, None).unwrap();
    let rows = id.iter().flat_map(|row| row.windows(2).map(|w| (w[0], w[1], true)));
    let cols = (0..n).flat_map(|c| id.windows(2).map(move |w| (w[0][c], w[1][c], false)));
    for (p, q, horizontal) in rows.chain(cols).collect::<Vec<_>>() {
        let l = sk.add_line_pts(p, q, None).unwrap();
        sk.add_constraint(if horizontal { ConstraintKind::Horizontal { l } } else { ConstraintKind::Vertical { l } }, None).unwrap();
        sk.add_constraint(ConstraintKind::Distance { p, q, value: 10.0 }, None).unwrap();
    }
    sk
}

#[test]
fn grid_from_a_perturbed_start_is_constructed_whole() {
    let mut sk = grid(20);
    let s = seed_of(&sk);
    // Point 0 is the sketch origin, point 1 the fixed grid corner.
    assert_eq!(s.placed_by[1], Some(Placement::Given));
    assert!(s.placed_by.iter().skip(2).all(|p| matches!(p, Some(Placement::TwoLoci { transversal: true, .. }))));
    let rep = solve(&mut sk);
    assert!(rep.ok(), "{rep:?}");
    assert_eq!(rep.dof, 0);
    eprintln!("20×20 grid ±1 mm: {} iterations", rep.iterations);
    for (i, p) in sk.points.iter().enumerate().skip(1) {
        let (r, c) = ((i - 1) / 20, (i - 1) % 20);
        assert!(p.pos.dist(v(c as f64 * 10.0, r as f64 * 10.0)) < 1e-9, "{i}: {:?}", p.pos);
    }
}

/// Every grid point is constructed transversally from two loci and its rows hold there: LM
/// gets no variable, and the rank analysis eliminates them all.
#[test]
fn constructed_grid_leaves_nothing_to_lm() {
    let sk = grid(20);
    let mut solved = sk.clone();
    crate::solver::lm_vars::take();
    let rep = seeded(true, || solve(&mut solved));
    assert_eq!(crate::solver::lm_vars::take(), 0);
    assert!(rep.ok() && rep.dof == 0 && rep.point_determined.iter().all(|d| *d), "{rep:?}");
    // Without the seed LM solves all of them, to the same outcome.
    let mut plain = sk.clone();
    let without = seeded(false, || solve(&mut plain));
    assert_eq!(crate::solver::lm_vars::take(), 2 * (20 * 20 - 1));
    assert_eq!((without.status, without.dof, &without.point_determined), (rep.status, rep.dof, &rep.point_determined));
    for (a, b) in solved.points.iter().zip(&plain.points) {
        assert!(a.pos.dist(b.pos) < 1e-6, "{:?} {:?}", a.pos, b.pos);
    }
}

/// A point the construction cannot place (one locus only) is all LM iterates on: the
/// constructed grid it hangs off stays where the seed put it, as constants.
#[test]
fn lm_iterates_on_what_the_construction_leaves() {
    let mut sk = grid(8);
    let corner = PointIdx(sk.points.len() - 1);
    let q = sk.add_point(sk.points[corner].pos + v(7.0, 0.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: corner, q, value: 5.0 }, None).unwrap();
    crate::solver::lm_vars::take();
    let rep = seeded(true, || solve(&mut sk.clone()));
    assert_eq!(crate::solver::lm_vars::take(), 2, "only the hanging point");
    assert!(rep.ok() && rep.dof == 1, "{rep:?}");
    assert!(!rep.point_determined[q.0] && rep.point_determined[corner.0]);
    // The outcome is the one of the unseeded solve, which iterates on every point.
    solve(&mut sk);
}

/// A pinned solve that fails is followed by one whole-component solve, from where the
/// component would have started without the seed: never again from the seeded start it failed
/// from. Here the hanging point is held at two distances from one corner.
#[test]
fn a_failed_pinned_solve_is_retried_once() {
    let mut sk = grid(4);
    let corner = PointIdx(sk.points.len() - 1);
    let q = sk.add_point(sk.points[corner].pos + v(7.0, 0.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: corner, q, value: 5.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: corner, q, value: 9.0 }, None).unwrap();
    crate::solver::lm_vars::runs();
    let rep = seeded(true, || solve(&mut sk.clone()));
    assert!(!rep.ok(), "{rep:?}");
    assert_eq!(crate::solver::lm_vars::runs(), 2, "the pinned solve and the unseeded retry");
}

/// Move point `q` to `to` holding it, as a drag does; `reference` from before the move.
fn drag(sk: &mut Sketch, q: PointIdx, to: Vec2, with_reference: bool) -> bool {
    let before: Vec<Vec2> = sk.points.iter().map(|p| p.pos).collect();
    let mut t = sk.clone();
    t.points[q].pos = to;
    let hold = Hold { points: vec![q], radii: vec![] };
    let ok = solve_with(&mut t, &hold, with_reference.then_some(before.as_slice()), Analysis::Full, None).ok();
    if ok {
        *sk = t;
    }
    ok
}

#[test]
fn dragging_a_base_across_and_back_returns_to_the_start() {
    let mut sk = Sketch::new();
    let a = sk.add_point(v(0.0, 0.0), None).unwrap();
    let b = sk.add_point(v(40.0, 0.0), None).unwrap();
    let c = sk.add_point(v(20.0, 15.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: a }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: a, q: c, value: 25.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: b, q: c, value: 25.0 }, None).unwrap();
    let start = sk.points[c].pos;
    let side = |sk: &Sketch| orientation(sk.points[a].pos, sk.points[b].pos, sk.points[c].pos);
    for with_reference in [true, false] {
        let mut t = sk.clone();
        assert!(drag(&mut t, b, v(-40.0, 0.0), with_reference));
        // Measured on the moved base, the apex has already changed side: only the reference
        // knows which side it was on.
        if with_reference {
            assert_eq!(side(&t), side(&sk), "the apex kept its side of the base");
        }
        assert!(drag(&mut t, b, v(40.0, 0.0), with_reference));
        assert!(t.points[c].pos.dist(start) < 1e-7, "{:?}", t.points[c].pos);
    }
}

/// Two-link arm: shoulder fixed, hand held at the cursor, one-jump drags along paths clear of
/// the arm's singular poses. The links are dimensioned, or (`equal`) the upper arm is and the
/// forearm is held equal to it. Returns elbow flips over 1000 drags.
fn elbow_flips(with_reference: bool, on: bool, equal: bool) -> usize {
    let mut rnd = Lcg(11);
    let (a, b) = if equal { (25.0, 25.0) } else { (30.0, 20.0) };
    let (lo, hi) = ((a - b) + 3.0, a + b - 3.0);
    let (mut flips, mut kept) = (0, 0);
    while kept < 1000 {
        let th1 = rnd.next() * std::f64::consts::TAU;
        let bend = (0.4 + rnd.next() * 2.2) * if rnd.next() < 0.5 { 1.0 } else { -1.0 };
        let v2 = Vec2::from_angle(th1) * a;
        let v3 = v2 + Vec2::from_angle(th1 + bend) * b;
        let tr = lo + rnd.next() * (hi - lo);
        let to = Vec2::from_angle(rnd.next() * std::f64::consts::TAU) * tr;
        if !(0..=100).all(|i| (lo..=hi).contains(&v3.lerp(to, i as f64 / 100.0).len())) {
            continue;
        }
        kept += 1;
        let mut sk = Sketch::new();
        let p1 = sk.add_point(Vec2::ZERO, None).unwrap();
        let p2 = sk.add_point(v2, None).unwrap();
        let p3 = sk.add_point(v3, None).unwrap();
        sk.add_constraint(ConstraintKind::Fix { p: p1 }, None).unwrap();
        let upper = sk.add_line_pts(p1, p2, None).unwrap();
        let fore = sk.add_line_pts(p2, p3, None).unwrap();
        if equal {
            sk.add_constraint(ConstraintKind::Length { l: upper, value: a }, None).unwrap();
            sk.add_constraint(ConstraintKind::Equal { a: upper, b: fore }, None).unwrap();
        } else {
            sk.add_constraint(ConstraintKind::Distance { p: p1, q: p2, value: a }, None).unwrap();
            sk.add_constraint(ConstraintKind::Distance { p: p2, q: p3, value: b }, None).unwrap();
        }
        let side = |sk: &Sketch| orientation(sk.points[p1].pos, sk.points[p3].pos, sk.points[p2].pos);
        let before = side(&sk);
        seeded(on, || drag(&mut sk, p3, to, with_reference));
        if side(&sk) != before {
            flips += 1;
        }
    }
    flips
}

#[test]
fn one_jump_drag_keeps_the_elbow_with_the_reference() {
    let off = elbow_flips(false, false, false);
    let current = elbow_flips(false, true, false);
    let reference = elbow_flips(true, true, false);
    eprintln!("two-link arm, one jump: {off}/1000 flips without seed, {current} seeded on current positions, {reference} seeded on the reference");
    assert_eq!(reference, 0);
}

/// The forearm's length comes from an Equal to the dimensioned upper arm: the seed reads it,
/// so a one-jump drag keeps the elbow too.
#[test]
fn one_jump_drag_keeps_an_equal_linked_elbow() {
    let off = elbow_flips(true, false, true);
    let reference = elbow_flips(true, true, true);
    eprintln!("equal-linked arm, one jump: {off}/1000 flips without seed, {reference} seeded on the reference");
    assert_eq!(reference, 0);
}

/// A fillet in a corner: L1 horizontal from a fixed end, arc tangent to L1 and L2, radius 5.
fn fillet() -> (Sketch, PointIdx, PointIdx, PointIdx) {
    let mut sk = Sketch::new();
    let l1 = sk.add_line(v(40.0, 0.0), v(5.0, 0.0), None, None, None).unwrap();
    let (s1, e1) = sk.curves[l1].kind.ends().unwrap();
    let l2 = sk.add_line(v(0.0, 5.0), v(0.0, 80.0), None, None, None).unwrap();
    let (s2, e2) = sk.curves[l2].kind.ends().unwrap();
    let arc = sk.add_arc(v(5.0, 5.0), v(0.0, 5.0), v(5.0, 0.0), [None, Some(s2), Some(e1)], None).unwrap();
    let CurveKind::Arc { c, .. } = sk.curves[arc].kind else { panic!() };
    sk.add_constraint(ConstraintKind::Fix { p: s1 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Horizontal { l: l1 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Radius { c: arc, value: 5.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Tangent { a: l1, b: arc }, None).unwrap();
    sk.add_constraint(ConstraintKind::Tangent { a: l2, b: arc }, None).unwrap();
    (sk, c, e2, s2)
}

#[test]
fn solved_fillet_stays_put_and_stays_in_its_corner_while_dragged() {
    let (mut sk, c, e2, s2) = fillet();
    let before = sk.points.clone();
    assert!(solve(&mut sk).ok());
    for (p, q) in sk.points.iter().zip(&before) {
        assert!(p.pos.dist(q.pos) < 1e-9, "{} moved to {:?}", p.id, p.pos);
    }
    let inside = |sk: &Sketch| {
        let (cc, s, e) = (sk.points[c].pos, sk.points[s2].pos, sk.points[e2].pos);
        cc.y > 0.0 && (e - s).cross(cc - s) < 0.0
    };
    for to in [v(-30.0, 75.0), v(20.0, 75.0), v(-60.0, 40.0), v(0.0, 80.0)] {
        for with_reference in [false, true] {
            let mut t = sk.clone();
            for k in 1..=10 {
                let at = t.points[e2].pos.lerp(to, k as f64 / 10.0);
                drag(&mut t, e2, at, with_reference);
                assert!(inside(&t), "centre left the corner dragging to {to:?}: {:?}", t.points[c].pos);
            }
        }
    }
}

#[test]
fn hostile_input_neither_panics_nor_changes_the_report() {
    let base = || {
        let mut sk = Sketch::new();
        let a = sk.add_point(v(10.0, 0.0), None).unwrap();
        let b = sk.add_point(v(5.0, 8.0), None).unwrap();
        let l = sk.add_line_pts(PointIdx::ORIGIN, a, None).unwrap();
        sk.add_constraint(ConstraintKind::Horizontal { l }, None).unwrap();
        sk.add_constraint(ConstraintKind::Distance { p: PointIdx::ORIGIN, q: a, value: 10.0 }, None).unwrap();
        sk.add_constraint(ConstraintKind::Distance { p: PointIdx::ORIGIN, q: b, value: 9.0 }, None).unwrap();
        sk.add_constraint(ConstraintKind::Distance { p: a, q: b, value: 9.0 }, None).unwrap();
        (sk, a, b, l)
    };
    let push = |sk: &mut Sketch, kind: ConstraintKind| {
        sk.constraints.push(Constraint { id: format!("h{}", sk.constraints.len()), kind, param: None, driven: false, text: None })
    };
    let cases: Vec<Box<dyn Fn(&mut Sketch, PointIdx, PointIdx, CurveIdx)>> = vec![
        Box::new(|sk, _, _, _| sk.constraints[1].kind.set_value(f64::NAN)),
        Box::new(|sk, _, _, _| sk.constraints[2].kind.set_value(f64::INFINITY)),
        Box::new(|sk, _, _, _| sk.constraints[3].kind.set_value(1e300)),
        Box::new(|sk, _, _, _| sk.constraints[2].kind.set_value(-4.0)),
        Box::new(|sk, _, _, _| sk.constraints[3].kind.set_value(0.0)),
        Box::new(|sk, _, b, _| sk.points[b].pos = v(f64::NAN, 1.0)),
        Box::new(|sk, a, _, _| sk.points[a].pos = v(f64::INFINITY, 0.0)),
        Box::new(|sk, _, b, _| sk.points[b].pos = v(1e300, -1e300)),
        Box::new(move |sk, _, b, _| push(sk, ConstraintKind::Distance { p: b, q: PointIdx(999), value: 3.0 })),
        Box::new(move |sk, a, _, _| push(sk, ConstraintKind::DistanceX { p: a, q: PointIdx(usize::MAX), value: 3.0 })),
        Box::new(move |sk, _, _, _| push(sk, ConstraintKind::Horizontal { l: CurveIdx(77) })),
        Box::new(move |sk, _, b, _| push(sk, ConstraintKind::Coincident { p: b, q: b })),
        Box::new(move |sk, _, _, l| push(sk, ConstraintKind::Length { l, value: f64::NAN })),
        Box::new(move |sk, a, b, _| push(sk, ConstraintKind::DistanceY { p: a, q: b, value: f64::NEG_INFINITY })),
    ];
    // Every solve here runs through the crate-wide seed-on/seed-off comparison (`solve_from`).
    for (i, case) in cases.iter().enumerate() {
        let (mut sk, a, b, l) = base();
        case(&mut sk, a, b, l);
        let mut mask = given(&sk);
        mask.push(true);
        let s = seed(&sk, &mask, Some(&[v(f64::NAN, 0.0)]), false, &points_on_curves(&sk), &mut Budget::full());
        assert!(s.pos.iter().flatten().all(|p| p.is_finite()), "case {i}: {s:?}");
        let n = sk.points.len();
        for reference in [None, Some(vec![Vec2::ZERO; 1]), Some(vec![v(f64::NAN, f64::INFINITY); n]), Some(vec![Vec2::ZERO; n + 3])] {
            let hold = Hold { points: vec![PointIdx(usize::MAX)], radii: vec![CurveIdx(usize::MAX)] };
            let mut t = sk.clone();
            match &reference {
                Some(r) => solve_with(&mut t, &hold, Some(r), Analysis::Full, None),
                None => solve_with(&mut t, &hold, None, Analysis::Full, None),
            };
        }
    }
}

/// A point whose second anchor is placed after it was first examined (it comes earlier in the
/// point list than that anchor) is examined again and placed.
#[test]
fn points_out_of_dependency_order_are_seeded() {
    let mut sk = Sketch::new();
    let a = sk.add_point(v(0.0, 0.0), None).unwrap();
    let d = sk.add_point(v(-10.0, 25.0), None).unwrap();
    let b = sk.add_point(v(40.0, 0.0), None).unwrap();
    let c = sk.add_point(v(20.0, 15.0), None).unwrap();
    for p in [a, b] {
        sk.add_constraint(ConstraintKind::Fix { p }, None).unwrap();
    }
    sk.add_constraint(ConstraintKind::Distance { p: a, q: c, value: 26.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: b, q: c, value: 26.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: a, q: d, value: 28.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: c, q: d, value: 30.0 }, None).unwrap();
    let s = seed_of(&sk);
    assert!(matches!(s.placed_by[c.0], Some(Placement::TwoLoci { transversal: true, .. })), "{s:?}");
    assert_eq!(s.placed_by[d.0], Some(Placement::TwoLoci { a: a.0, b: c.0, transversal: true }), "{s:?}");
}

/// Tangency and concentricity are judged on the loci's own size, not on how far from the
/// origin they are.
#[test]
fn far_from_the_origin_a_flat_triangle_still_has_two_branches() {
    // Apex 0.05 mm above a 40 mm base, 1e5 mm from the origin.
    let o = v(1e5, 1e5);
    let mut sk = Sketch::new();
    let a = sk.add_point(o, None).unwrap();
    let b = sk.add_point(o + v(40.0, 0.0), None).unwrap();
    let c = sk.add_point(o + v(20.0, 0.05), None).unwrap();
    for p in [a, b] {
        sk.add_constraint(ConstraintKind::Fix { p }, None).unwrap();
    }
    let r = v(20.0, 0.05).len();
    sk.add_constraint(ConstraintKind::Distance { p: a, q: c, value: r }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: b, q: c, value: r }, None).unwrap();
    let s = seed_of(&sk);
    assert_eq!(s.placed_by[c.0], Some(Placement::TwoLoci { a: a.0, b: b.0, transversal: true }));
    assert!(s.pos[c.0].unwrap().dist(o + v(20.0, 0.05)) < 1e-6, "{:?}", s.pos[c.0]);

    // Circles about centres 1 mm apart, 1e6 mm from the origin, are not concentric.
    let o = v(1e6, 1e6);
    let mut sk = Sketch::new();
    let a = sk.add_point(o, None).unwrap();
    let b = sk.add_point(o + v(1.0, 0.0), None).unwrap();
    let c = sk.add_point(o + v(0.4, -9.0), None).unwrap();
    for p in [a, b] {
        sk.add_constraint(ConstraintKind::Fix { p }, None).unwrap();
    }
    sk.add_constraint(ConstraintKind::Distance { p: a, q: c, value: 10.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: b, q: c, value: 10.0 }, None).unwrap();
    let s = seed_of(&sk);
    assert_eq!(s.placed_by[c.0], Some(Placement::TwoLoci { a: a.0, b: b.0, transversal: true }));
    assert!(s.pos[c.0].unwrap().dist(o + v(0.5, -(100.0f64 - 0.25).sqrt())) < 1e-6, "{:?}", s.pos[c.0]);
}

/// Loci that only touch leave the point where it was: the solve is exactly the unseeded one.
#[test]
fn touching_loci_are_left_to_lm() {
    let mut sk = Sketch::new();
    let a = sk.add_point(v(0.0, 0.0), None).unwrap();
    let b = sk.add_point(v(40.0, 0.0), None).unwrap();
    let c = sk.add_point(v(18.0, 3.0), None).unwrap();
    for p in [a, b] {
        sk.add_constraint(ConstraintKind::Fix { p }, None).unwrap();
    }
    sk.add_constraint(ConstraintKind::Distance { p: a, q: c, value: 15.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: b, q: c, value: 25.0 }, None).unwrap();
    let s = seed_of(&sk);
    assert_eq!(s.placed_by[c.0], Some(Placement::TwoLoci { a: a.0, b: b.0, transversal: false }));
    let (mut on, mut off) = (sk.clone(), sk.clone());
    let with = seeded(true, || solve(&mut on));
    let without = seeded(false, || solve(&mut off));
    assert_eq!(on.points, off.points);
    assert_eq!((with.status, with.iterations), (without.status, without.iterations));
}

/// The canonical bracket (Owen's articulation-pair case) behind a row–column point, with a
/// point on top: O2 is placed by propagation (level with O1, under F), P Q R only by the
/// decomposition (none has two placed neighbours), and S by propagation again (level with P,
/// under Q: relations the decomposition does not combine). Returns the sketch and
/// [O1, F, O2, P, Q, R, S].
fn bracket_chain() -> (Sketch, [PointIdx; 7]) {
    let truth = [v(0.0, 0.0), v(60.0, 40.0), v(60.0, 0.0), v(10.0, 30.0), v(45.0, 35.0), v(25.0, 55.0), v(45.0, 30.0)];
    let mut sk = Sketch::new();
    let mut ids = [PointIdx::ORIGIN; 7];
    for (i, t) in truth.iter().enumerate() {
        // The free points start a few mm off, as after an edit.
        let off = if i < 2 { v(0.0, 0.0) } else { v(1.5 * (i as f64 - 4.0), 1.0 - 0.5 * i as f64) };
        ids[i] = sk.add_point(*t + off, None).unwrap();
    }
    let [o1, f, o2, p, q, r, s] = ids;
    for x in [o1, f] {
        sk.add_constraint(ConstraintKind::Fix { p: x }, None).unwrap();
    }
    sk.add_constraint(ConstraintKind::HorizontalPoints { p: o1, q: o2 }, None).unwrap();
    sk.add_constraint(ConstraintKind::VerticalPoints { p: f, q: o2 }, None).unwrap();
    let at = |x: PointIdx| truth[ids.iter().position(|i| *i == x).unwrap()];
    sk.add_constraint(ConstraintKind::HorizontalPoints { p, q: s }, None).unwrap();
    sk.add_constraint(ConstraintKind::VerticalPoints { p: q, q: s }, None).unwrap();
    for (a, b) in [(p, q), (q, r), (p, r), (o1, p), (o1, r), (o2, q)] {
        sk.add_constraint(ConstraintKind::Distance { p: a, q: b, value: at(a).dist(at(b)) }, None).unwrap();
    }
    (sk, ids)
}

#[test]
fn propagation_and_decomposition_interleave() {
    let (mut sk, [o1, f, o2, p, q, r, s]) = bracket_chain();
    let out = seed_of(&sk);
    for x in [o1, f] {
        assert_eq!(out.placed_by[x.0], Some(Placement::Given));
    }
    assert!(matches!(out.placed_by[o2.0], Some(Placement::TwoLoci { transversal: true, .. })), "{out:?}");
    for x in [p, q, r] {
        assert_eq!(out.placed_by[x.0], Some(Placement::Cluster), "point {x}: {out:?}");
    }
    assert_eq!(out.placed_by[s.0], Some(Placement::TwoLoci { a: p.0, b: q.0, transversal: true }), "{out:?}");
    let rep = solve(&mut sk);
    assert!(rep.ok() && rep.dof == 0, "{rep:?}");
    assert_eq!(rep.iterations, 0, "every point constructed exactly");
}

/// A cluster that only fits at a touching intersection is not merged: its points are left to
/// LM, as a touching pair of loci is.
#[test]
fn touching_cluster_merges_are_left_to_lm() {
    // P fixed; rigid triangle P Q R; Q–R at a DistanceX. Equal to |QR|, it holds only with
    // Q–R level: turning the triangle onto it is a tangency.
    let build = |dx: f64| {
        let mut sk = Sketch::new();
        let p = sk.add_point(v(10.0, 10.0), None).unwrap();
        let q = sk.add_point(v(0.0, -21.0), None).unwrap();
        let r = sk.add_point(v(31.0, -17.0), None).unwrap();
        sk.add_constraint(ConstraintKind::Fix { p }, None).unwrap();
        let truth = [v(10.0, 10.0), v(-2.0, -20.0), v(30.0, -20.0)];
        let ids = [p, q, r];
        for (a, b) in [(0, 1), (1, 2), (0, 2)] {
            sk.add_constraint(ConstraintKind::Distance { p: ids[a], q: ids[b], value: truth[a].dist(truth[b]) }, None).unwrap();
        }
        sk.add_constraint(ConstraintKind::DistanceX { p: q, q: r, value: dx }, None).unwrap();
        (sk, q, r)
    };
    let (sk, q, r) = build(32.0);
    let out = seed_of(&sk);
    assert_eq!((out.placed_by[q.0], out.placed_by[r.0]), (None, None), "{out:?}");
    let (mut on, mut off) = (sk.clone(), sk.clone());
    let with = seeded(true, || solve(&mut on));
    let without = seeded(false, || solve(&mut off));
    assert_eq!(on.points, off.points);
    assert_eq!((with.status, with.iterations), (without.status, without.iterations));
    // Shorter: the triangle turns onto it at a crossing and is placed.
    let (sk, q, r) = build(31.0);
    let out = seed_of(&sk);
    assert_eq!((out.placed_by[q.0], out.placed_by[r.0]), (Some(Placement::Cluster), Some(Placement::Cluster)), "{out:?}");
}

/// The seed does not depend on the order of the constraints.
#[test]
fn seed_is_independent_of_constraint_order() {
    for mut sk in [grid(6), linkage().0, bracket_chain().0] {
        let forward = seed_of(&sk);
        sk.constraints.reverse();
        assert_eq!(seed_of(&sk), forward);
        let n = sk.constraints.len();
        sk.constraints.rotate_left(n / 3);
        assert_eq!(seed_of(&sk), forward);
    }
}

/// A reference taken before points were appended covers the points it has; one longer than
/// the point list is ignored.
#[test]
fn short_reference_covers_its_points_and_long_one_is_ignored() {
    let arm = || {
        let mut sk = Sketch::new();
        let p1 = sk.add_point(v(0.0, 0.0), None).unwrap();
        let p2 = sk.add_point(v(0.0, 30.0), None).unwrap();
        let p3 = sk.add_point(v(20.0, 30.0), None).unwrap();
        sk.add_constraint(ConstraintKind::Fix { p: p1 }, None).unwrap();
        sk.add_constraint(ConstraintKind::Distance { p: p1, q: p2, value: 30.0 }, None).unwrap();
        sk.add_constraint(ConstraintKind::Distance { p: p2, q: p3, value: 20.0 }, None).unwrap();
        (sk, p1, p2, p3)
    };
    let side = |sk: &Sketch, p1: PointIdx, p2: PointIdx, p3: PointIdx| orientation(sk.points[p1].pos, sk.points[p3].pos, sk.points[p2].pos);
    let hand_moved = |extra: usize| {
        let (mut sk, p1, p2, p3) = arm();
        let before: Vec<Vec2> = sk.points.iter().map(|p| p.pos).collect();
        let start = side(&sk, p1, p2, p3);
        sk.add_point(v(5.0, 5.0), None).unwrap();
        sk.points[p3].pos = v(-20.0, 30.0);
        let reference: Vec<Vec2> = before.iter().copied().chain(std::iter::repeat_n(Vec2::ZERO, extra)).collect();
        assert!(solve_with(&mut sk, &Hold { points: vec![p3], radii: vec![] }, Some(&reference), Analysis::Full, None).ok());
        (start, side(&sk, p1, p2, p3))
    };
    // Shorter than the sketch (a point appended since): the elbow keeps its side.
    let (start, after) = hand_moved(0);
    assert_eq!(after, start);
    // Longer than the sketch: ignored, the orientation comes from the moved hand.
    let (start, after) = hand_moved(5);
    assert_ne!(after, start);
}

/// [`bracket_chain`] with a second bracket on S (placed by propagation after the first
/// decomposition) and Q: P' Q' R' rigid, |S P'|, |S R'|, |Q Q'|. Returns the sketch and
/// [P', Q', R'].
fn two_stage_bracket() -> (Sketch, [PointIdx; 3]) {
    let (mut sk, [.., q, _, s]) = bracket_chain();
    let truth = [v(70.0, 20.0), v(85.0, 40.0), v(65.0, 50.0)];
    let ids = truth.map(|t| sk.add_point(t + v(1.0, -1.5), None).unwrap());
    let (sq, qq) = (v(45.0, 30.0), v(45.0, 35.0));
    for (a, b, value) in [
        (ids[0], ids[1], truth[0].dist(truth[1])),
        (ids[1], ids[2], truth[1].dist(truth[2])),
        (ids[0], ids[2], truth[0].dist(truth[2])),
        (s, ids[0], sq.dist(truth[0])),
        (s, ids[2], sq.dist(truth[2])),
        (q, ids[1], qq.dist(truth[1])),
    ] {
        sk.add_constraint(ConstraintKind::Distance { p: a, q: b, value }, None).unwrap();
    }
    (sk, ids)
}

/// Propagation placing a point the second bracket hangs on extends the graph: one build.
#[test]
fn the_decomposition_graph_is_reused_across_rounds() {
    let (mut sk, ids) = two_stage_bracket();
    let builds = crate::decompose::builds();
    let out = seed_of(&sk);
    assert_eq!(crate::decompose::builds() - builds, 1, "one graph for both rounds");
    for x in ids {
        assert_eq!(out.placed_by[x.0], Some(Placement::Cluster), "point {x}: {out:?}");
    }
    let rep = solve(&mut sk);
    assert!(rep.ok() && rep.dof == 0 && rep.iterations == 0, "{rep:?}");
}

/// A long chain of dimensioned links hanging off a fixed point: every free point keeps a degree
/// of freedom and no cluster of three points is rigid, so no graph is built.
#[test]
fn no_graph_is_built_where_the_decomposition_can_place_nothing() {
    let mut sk = Sketch::new();
    let mut prev = sk.add_point(v(0.0, 0.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: prev }, None).unwrap();
    for i in 1..300 {
        let p = sk.add_point(v(i as f64 * 10.0, (i % 3) as f64), None).unwrap();
        sk.add_constraint(ConstraintKind::Distance { p: prev, q: p, value: 10.0 }, None).unwrap();
        prev = p;
    }
    let builds = crate::decompose::builds();
    let out = seed_of(&sk);
    assert_eq!(crate::decompose::builds() - builds, 0);
    // The sketch's origin and the fixed end are given.
    assert!(out.placed_by.iter().skip(2).all(Option::is_none), "{:?}", out.placed_by);
}

/// Dimensioned rectangles that float (no fixed point) are oriented clusters. Solved, they hold
/// at their reference and no graph is built; after a width edit the edited one is floated by
/// its reference pose (it keeps its centre) and the others are left alone.
#[test]
fn floating_rectangles_are_placed_by_their_reference_pose() {
    let mut sk = Sketch::new();
    let mut widths = Vec::new();
    for k in 0..50 {
        let o = v(k as f64 * 30.0, 100.0);
        let ps = [v(0.0, 0.0), v(20.0, 0.0), v(20.0, 10.0), v(0.0, 10.0)].map(|d| sk.add_point(o + d, None).unwrap());
        for i in 0..4 {
            let l = sk.add_line_pts(ps[i], ps[(i + 1) % 4], None).unwrap();
            let kind = if i % 2 == 0 { ConstraintKind::Horizontal { l } } else { ConstraintKind::Vertical { l } };
            sk.add_constraint(kind, None).unwrap();
            sk.add_constraint(ConstraintKind::Length { l, value: if i % 2 == 0 { 20.0 } else { 10.0 } }, None).unwrap();
            if i % 2 == 0 {
                widths.push(sk.constraints.len() - 1);
            }
        }
    }
    let builds = crate::decompose::builds();
    let out = seed_of(&sk);
    assert_eq!(crate::decompose::builds() - builds, 0);
    assert!(out.placed_by.iter().skip(1).all(Option::is_none), "{:?}", out.placed_by);
    // The first rectangle made 26 wide (bottom and top).
    for c in &widths[..2] {
        if let ConstraintKind::Length { value, .. } = &mut sk.constraints[*c].kind {
            *value = 26.0;
        }
    }
    let out = seed_of(&sk);
    assert_eq!(crate::decompose::builds() - builds, 1);
    // Past the sketch's origin: the first rectangle 3 wider on each side; the others, already
    // at their reference pose, are left alone.
    for (i, p) in sk.points.iter().enumerate().skip(1) {
        if i <= 4 {
            assert_eq!(out.placed_by[i], Some(Placement::Float), "point {i}");
            let want = p.pos + v(if p.pos.x > 10.0 { 3.0 } else { -3.0 }, 0.0);
            assert!(out.pos[i].unwrap().dist(want) < 1e-9, "point {i}: {:?} vs {want:?}", out.pos[i]);
        } else {
            assert_eq!(out.placed_by[i], None, "point {i}");
        }
    }
    let rep = solve(&mut sk);
    assert!(rep.ok() && rep.dof == 100 && rep.iterations == 0, "{rep:?}");
}

/// Readings of one pair that disagree are left to LM, whatever the order of the constraints.
#[test]
fn conflicting_readings_of_one_pair_do_not_depend_on_order() {
    let (mut sk, [.., p, q, _, _]) = bracket_chain();
    sk.add_constraint(ConstraintKind::Distance { p, q, value: 25.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p, q, value: 30.0 }, None).unwrap();
    let forward = seed_of(&sk);
    sk.constraints.reverse();
    assert_eq!(seed_of(&sk), forward);
    // Neither end is seeded by a cluster, nor floated with one (P with the triangle O1 P R).
    for x in [p, q] {
        assert!(!matches!(forward.placed_by[x.0], Some(Placement::Cluster | Placement::Float)), "point {x}: {forward:?}");
    }
}

/// A cluster placement must hold every relation of the point, as a propagated one does.
#[test]
fn inconsistent_offsets_are_not_placed() {
    // B: 30 right of A, 40 above it, and 60 from it (50 would hold). C hangs off A and B.
    let mut sk = Sketch::new();
    let a = sk.add_point(v(0.0, 0.0), None).unwrap();
    let b = sk.add_point(v(31.0, 39.0), None).unwrap();
    let c = sk.add_point(v(60.0, 0.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: a }, None).unwrap();
    sk.add_constraint(ConstraintKind::DistanceX { p: a, q: b, value: 30.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::DistanceY { p: a, q: b, value: 40.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: a, q: b, value: 60.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: a, q: c, value: 60.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: b, q: c, value: 50.0 }, None).unwrap();
    let out = seed_of(&sk);
    assert_eq!((out.placed_by[b.0], out.placed_by[c.0]), (None, None), "{out:?}");
}

/// A given point is a constant for the solve: never placed, even when out of range.
#[test]
fn given_points_are_never_placed() {
    let mut sk = Sketch::new();
    let a = sk.add_point(v(0.0, 0.0), None).unwrap();
    let b = sk.add_point(v(40.0, 50.0), None).unwrap();
    for p in [a, b] {
        sk.add_constraint(ConstraintKind::Fix { p }, None).unwrap();
    }
    let g = PointIdx(sk.points.len());
    sk.points.push(SPoint { id: "far".into(), pos: v(2e9, 0.0), fixed: true, link: None, exprs: None });
    let d = sk.add_point(v(20.0, 30.0), None).unwrap();
    let push = |sk: &mut Sketch, kind: ConstraintKind| {
        sk.constraints.push(Constraint { id: format!("h{}", sk.constraints.len()), kind, param: None, driven: false, text: None })
    };
    // Level with A, under B: the row and the column cross at (40, 0), no branch to pick.
    push(&mut sk, ConstraintKind::HorizontalPoints { p: a, q: g });
    push(&mut sk, ConstraintKind::VerticalPoints { p: b, q: g });
    push(&mut sk, ConstraintKind::Distance { p: g, q: d, value: 30.0 });
    push(&mut sk, ConstraintKind::Distance { p: a, q: d, value: 36.0 });
    let mask = given(&sk);
    assert!(mask[g.0]);
    let out = seed(&sk, &mask, None, true, &points_on_curves(&sk), &mut Budget::full());
    assert_eq!((out.pos[g.0], out.placed_by[g.0]), (None, None), "{out:?}");
    assert_eq!(out.placed_by[d.0], None, "nothing is built on it: {out:?}");
}

/// A component the seed did not move is solved once, also when it fails: the plain-start
/// retry only runs where the seed moved the start.
#[test]
fn an_unseeded_failing_component_is_not_solved_twice() {
    // A triangle the seed places, and two free points held 10 and 20 apart.
    let mut sk = Sketch::new();
    let a = sk.add_point(v(0.0, 0.0), None).unwrap();
    let b = sk.add_point(v(40.0, 0.0), None).unwrap();
    let c = sk.add_point(v(21.0, 14.0), None).unwrap();
    for p in [a, b] {
        sk.add_constraint(ConstraintKind::Fix { p }, None).unwrap();
    }
    sk.add_constraint(ConstraintKind::Distance { p: a, q: c, value: 25.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: b, q: c, value: 25.0 }, None).unwrap();
    let pair = |sk: &mut Sketch| {
        let p = sk.add_point(v(100.0, 0.0), None).unwrap();
        let q = sk.add_point(v(130.0, 7.0), None).unwrap();
        sk.add_constraint(ConstraintKind::Distance { p, q, value: 10.0 }, None).unwrap();
        sk.add_constraint(ConstraintKind::Distance { p, q, value: 20.0 }, None).unwrap();
    };
    pair(&mut sk);
    let mut alone = Sketch::new();
    pair(&mut alone);
    let pair_alone = seeded(false, || solve(&mut alone));
    let with = seeded(true, || solve(&mut sk));
    assert!(!with.ok() && !pair_alone.ok());
    assert!(pair_alone.iterations > 0);
    assert_eq!(with.iterations, pair_alone.iterations, "the triangle takes none, the pair is solved once");
}

// ---------------------------------------------------------------------------------------------
// Floating clusters: rigid clusters that are free or pinned at a single placed point.

/// Distances between pairs of points, each its current length times `scale`.
fn dimension(sk: &mut Sketch, pairs: &[(PointIdx, PointIdx, f64)]) {
    for &(p, q, scale) in pairs {
        let value = sk.points[p].pos.dist(sk.points[q].pos) * scale;
        sk.add_constraint(ConstraintKind::Distance { p, q, value }, None).unwrap();
    }
}

fn points(sk: &mut Sketch, at: &[Vec2]) -> Vec<PointIdx> {
    at.iter().map(|p| sk.add_point(*p, None).unwrap()).collect()
}

/// A floated pose is a guess at free motion: it seeds the solver but nothing is built on it.
/// Two triangles pinned at fixed P and Q, both edited, joined by two links (no freedom left):
/// B is not built on A's guess (and then rejected against it), it is floated too.
#[test]
fn a_floated_pose_anchors_nothing() {
    let mut sk = Sketch::new();
    let a = points(&mut sk, &[v(0.0, 0.0), v(10.0, 5.0), v(5.0, 12.0)]);
    let b = points(&mut sk, &[v(40.0, 0.0), v(30.0, 5.0), v(35.0, 12.0)]);
    for p in [a[0], b[0]] {
        sk.add_constraint(ConstraintKind::Fix { p }, None).unwrap();
    }
    for t in [&a, &b] {
        dimension(&mut sk, &[(t[0], t[1], 1.0), (t[1], t[2], 1.1), (t[2], t[0], 1.0)]);
    }
    dimension(&mut sk, &[(a[1], b[1], 1.0), (a[2], b[2], 1.0)]);
    let out = seed_of(&sk);
    for x in [a[1], a[2], b[1], b[2]] {
        assert_eq!(out.placed_by[x.0], Some(Placement::Float), "point {x}: {out:?}");
    }
    let rep = solve(&mut sk);
    assert!(rep.ok() && rep.dof == 0, "{rep:?}");
}

/// A float is pinned only at a point the seed accepted: the bracket's P is constructed by the
/// decomposition but breaks a midpoint the decomposition does not combine with it, so it is not
/// accepted, and the edited triangle hanging off P is not floated about the rejected position.
#[test]
fn floats_are_pinned_only_at_accepted_points() {
    let truth = [v(0.0, 0.0), v(60.0, 0.0), v(10.0, 30.0), v(45.0, 35.0), v(25.0, 55.0)];
    let mut sk = Sketch::new();
    let ids = points(&mut sk, &truth);
    let [o1, o2, p, q, r] = [ids[0], ids[1], ids[2], ids[3], ids[4]];
    for x in [o1, o2] {
        sk.add_constraint(ConstraintKind::Fix { p: x }, None).unwrap();
    }
    dimension(&mut sk, &[(p, q, 1.0), (q, r, 1.0), (p, r, 1.0), (o1, p, 1.0), (o1, r, 1.0), (o2, q, 1.0)]);
    let qr = sk.add_line_pts(q, r, None).unwrap();
    sk.add_constraint(ConstraintKind::Midpoint { p, l: qr }, None).unwrap();
    let t = points(&mut sk, &[v(5.0, 40.0), v(0.0, 33.0)]);
    dimension(&mut sk, &[(p, t[0], 1.0), (t[0], t[1], 1.2), (t[1], p, 1.0)]);
    let out = seed_of(&sk);
    assert_eq!(out.placed_by[p.0], None, "{out:?}");
    for x in t {
        assert_eq!(out.placed_by[x.0], None, "point {x}: {out:?}");
    }
}

/// An edit to a floating triangle read only through a tie (a point's distance from a line)
/// still floats it.
#[test]
fn a_tie_that_misses_floats_its_cluster() {
    let mut sk = Sketch::new();
    let [a, b, c] = [v(0.0, 0.0), v(20.0, 0.0), v(10.0, 8.0)].map(|p| sk.add_point(p, None).unwrap());
    let l = sk.add_line_pts(a, b, None).unwrap();
    dimension(&mut sk, &[(a, b, 1.0), (b, c, 1.0)]);
    sk.add_constraint(ConstraintKind::PointLineDistance { p: c, l, value: 10.0 }, None).unwrap();
    let out = seed_of(&sk);
    for x in [a, b, c] {
        assert_eq!(out.placed_by[x.0], Some(Placement::Float), "point {x}: {out:?}");
    }
    let rep = solve(&mut sk);
    assert!(rep.ok() && rep.dof == 3 && rep.iterations == 0, "{rep:?}");
}

/// A drag step moves only held points: a floating part it does not reach (here one that can't
/// be solved) builds no graph on any frame.
#[test]
fn a_drag_builds_no_graph_for_a_floating_part_it_does_not_reach() {
    let mut sk = Sketch::new();
    let t = points(&mut sk, &[v(100.0, 0.0), v(101.0, 0.0), v(100.5, 0.5)]);
    for (p, q, value) in [(t[0], t[1], 1.0), (t[1], t[2], 1.0), (t[2], t[0], 5.0)] {
        sk.add_constraint(ConstraintKind::Distance { p, q, value }, None).unwrap();
    }
    let o = sk.add_point(v(0.0, 0.0), None).unwrap();
    let d = sk.add_point(v(10.0, 0.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: o }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: o, q: d, value: 10.0 }, None).unwrap();
    let builds = crate::decompose::builds();
    for k in 1..=8 {
        drag(&mut sk, d, Vec2::from_angle(k as f64 * 0.1) * 10.0, true);
    }
    assert_eq!(crate::decompose::builds() - builds, 0);
}

/// Solved floating parts are where their reference pose puts them: an edit elsewhere floats
/// none of them, and they cost the decomposition little more than its graph.
#[test]
fn solved_floating_parts_are_not_floated() {
    let mut sk = Sketch::new();
    let rect = |sk: &mut Sketch, o: Vec2, width: f64| {
        let ps = [v(0.0, 0.0), v(20.0, 0.0), v(20.0, 10.0), v(0.0, 10.0)].map(|d| sk.add_point(o + d, None).unwrap());
        for i in 0..4 {
            let l = sk.add_line_pts(ps[i], ps[(i + 1) % 4], None).unwrap();
            let kind = if i % 2 == 0 { ConstraintKind::Horizontal { l } } else { ConstraintKind::Vertical { l } };
            sk.add_constraint(kind, None).unwrap();
            sk.add_constraint(ConstraintKind::Length { l, value: if i % 2 == 0 { width } else { 10.0 } }, None).unwrap();
        }
        ps
    };
    for k in 0..1000 {
        rect(&mut sk, v((k % 40) as f64 * 30.0, 100.0 + (k / 40) as f64 * 20.0), 20.0);
    }
    let floating = sk.points.len();
    // The canonical bracket (O1 O2 fixed, rigid P Q R; no propagation order), edited.
    let o = v(0.0, -100.0);
    let ids = points(&mut sk, &[v(0.0, 0.0), v(60.0, 0.0), v(10.0, 30.0), v(45.0, 35.0), v(25.0, 55.0)].map(|p| o + p));
    for x in &ids[..2] {
        sk.add_constraint(ConstraintKind::Fix { p: *x }, None).unwrap();
    }
    dimension(
        &mut sk,
        &[(ids[2], ids[3], 1.1), (ids[3], ids[4], 1.0), (ids[2], ids[4], 1.0), (ids[0], ids[2], 1.0), (ids[0], ids[4], 1.0), (ids[1], ids[3], 1.0)],
    );
    let out = seed_of(&sk);
    let work = crate::decompose::last_work();
    for x in &ids[2..] {
        assert_eq!(out.placed_by[x.0], Some(Placement::Cluster), "point {x}");
    }
    assert!(out.placed_by[1..floating].iter().all(Option::is_none));
    assert!(work < 1_000_000, "{work}");
}

#[test]
fn an_arc_follows_its_constructed_centre() {
    // An arc whose centre is put on the origin follows it: the seed places the centre, and the
    // ends, which nothing else constrains, start carried along instead of left behind (from
    // there LM would turn them; reference test 20-slot-constrained).
    let mut sk = Sketch::new();
    let arc = sk.add_arc(v(1.0, 1.0), v(1.0, 7.0), v(1.0, -5.0), [None, None, None], None).unwrap();
    let CurveKind::Arc { c, a, b } = sk.curves[arc].kind else { panic!() };
    sk.add_constraint(ConstraintKind::Coincident { p: c, q: PointIdx::ORIGIN }, None).unwrap();
    assert!(solve_holding(&mut sk, &Hold { points: vec![PointIdx::ORIGIN], radii: vec![] }).ok());
    for (p, want) in [(c, v(0.0, 0.0)), (a, v(0.0, 6.0)), (b, v(0.0, -6.0))] {
        assert!(sk.points[p].pos.dist(want) < 1e-9, "{:?} vs {want:?}", sk.points[p].pos);
    }
}
