//! Cluster decomposition: the canonical bracket (no propagation order from the fixed points),
//! its random family under large dimension edits, fans, a triconnected linkage left to the
//! numerical solver, and hostile input.
//!
//! `gap_counter` (ignored; run with `--ignored --nocapture`) measures how often the numerical
//! solver alone mirrors a rigid sub-triangle on each family, and what the decomposition does.
use super::*;
use crate::construct::switch;
use crate::model::{Constraint, ConstraintKind, CurveKind, Sketch};
use crate::solver::{SolveReport, solve};

use crate::tests::Lcg;

fn known(sk: &Sketch) -> Vec<Option<Vec2>> {
    sk.locked_points().iter().zip(&sk.points).map(|(l, p)| l.then_some(p.pos)).collect()
}

fn current(sk: &Sketch) -> Vec<Vec2> {
    sk.points.iter().map(|p| p.pos).collect()
}

/// The decomposition of `sk` relative to `known`, `reference` covering the points it has.
fn decompose_from(sk: &Sketch, known: &[Option<Vec2>], reference: &[Vec2]) -> Placed {
    let reading = crate::loci::read(
        sk,
        &|i| reference.get(i).or(sk.points.get(i).map(|p| &p.pos)).copied(),
        &crate::solver::points_on_curves(sk),
        &crate::loci::radii(sk),
    );
    let mut d = Decomposition::new(&reading, known, true, MAX_WORK);
    Placed { pos: (0..sk.points.len()).map(|i| d.pos(i)).collect() }
}

/// Every point's position in the ground cluster.
#[derive(Debug)]
struct Placed {
    pos: Vec<Option<Vec2>>,
}

/// A constraint pushed without validation (hostile and oversized inputs).
fn raw(kind: ConstraintKind) -> Constraint {
    Constraint { id: "k".into(), kind, param: None, driven: false, text: None }
}

/// A point pushed past the sketch's own size cap.
fn push_point(sk: &mut Sketch, pos: Vec2) {
    let id = format!("p{}", sk.points.len());
    sk.points.push(crate::model::SPoint { id, pos, fixed: false, link: None, exprs: None });
}

/// Every point of `ids` placed at its `want` position.
fn assert_placed(d: &Placed, ids: &[usize], want: &[Vec2]) {
    for (id, w) in ids.iter().zip(want) {
        let at = d.pos[*id].unwrap_or_else(|| panic!("point {id} unplaced"));
        assert!(at.dist(*w) < 1e-9, "point {id}: {at:?} vs {w:?}");
    }
}

/// The sketch with the decomposed positions written in.
fn seeded(sk: &Sketch, d: &Placed) -> Sketch {
    let mut out = sk.clone();
    for (p, pos) in out.points.iter_mut().zip(&d.pos) {
        if let Some(v) = pos {
            p.pos = *v;
        }
    }
    out
}

/// An orientation the edit must keep: the turn of three points, or which side of `p` a point
/// `q` is on along an axis.
#[derive(Clone, Copy, Debug)]
enum Check {
    Turn(usize, usize, usize),
    Side(usize, usize, Axis),
}

fn signature(checks: &[Check], pos: &[Vec2]) -> Vec<bool> {
    checks
        .iter()
        .map(|c| match *c {
            Check::Turn(a, b, c) => (pos[b] - pos[a]).cross(pos[c] - pos[a]) > 0.0,
            Check::Side(p, q, Axis::X) => pos[q].x > pos[p].x,
            Check::Side(p, q, Axis::Y) => pos[q].y > pos[p].y,
        })
        .collect()
}

/// Every check well away from degenerate (so "mirrored" is unambiguous).
fn clear(checks: &[Check], pos: &[Vec2], size: f64) -> bool {
    checks.iter().all(|c| match *c {
        Check::Turn(a, b, c) => {
            let (u, v, w) = (pos[b] - pos[a], pos[c] - pos[a], pos[c] - pos[b]);
            let edge = u.len2().max(v.len2()).max(w.len2());
            u.cross(v).abs() > 0.05 * edge && u.len().min(v.len()).min(w.len()) > 0.05 * size
        }
        Check::Side(p, q, Axis::X) => (pos[q].x - pos[p].x).abs() > 0.05 * size,
        Check::Side(p, q, Axis::Y) => (pos[q].y - pos[p].y).abs() > 0.05 * size,
    })
}

struct Case {
    sk: Sketch,
    /// The solution the edit asks for (keeps every check's orientation).
    target: Option<Vec<Vec2>>,
    checks: Vec<Check>,
    size: f64,
}

/// How the bracket's datum (O1, O2) is held.
#[derive(Clone, Copy, Debug)]
enum Datum {
    /// O1 and O2 fixed.
    Fixed,
    /// O1 is the sketch origin; O2 on a horizontal line of given length from it.
    Horizontal,
    /// O1 fixed; O2 at a DistanceX and a DistanceY from it.
    Offsets,
    /// O1 fixed, no O2; Q horizontal with O1 (the rigid bracket turns about O1).
    Hinge,
}

const DATUMS: [Datum; 4] = [Datum::Fixed, Datum::Horizontal, Datum::Offsets, Datum::Hinge];

/// The canonical bracket with random sizes: O1, O2 held; rigid triangle P Q R (|PQ| |QR| |PR|);
/// |O1P|, |O1R|, |O2Q|. No point has two placed neighbours. The sketch starts at a random
/// layout and its dimensions come from a second one with the same orientations, every
/// dimension changed by 0.5–2.5×.
fn bracket(rng: &mut Lcg, datum: Datum, scale: f64) -> Option<Case> {
    let size = 60.0 * scale;
    let o1 = match datum {
        Datum::Horizontal => Vec2::ZERO,
        _ => Vec2::new(rng.range(-200.0, 200.0), rng.range(-200.0, 200.0)) * scale,
    };
    let rnd = |rng: &mut Lcg| o1 + Vec2::new(rng.range(-size, size), rng.range(-size, size));
    // [O1, O2, P, Q, R]
    let mut l0 = [o1, rnd(rng), rnd(rng), rnd(rng), rnd(rng)];
    let mut l1 = [o1, l0[1], rnd(rng), rnd(rng), rnd(rng)];
    match datum {
        Datum::Fixed => {}
        Datum::Horizontal => {
            l0[1].y = o1.y;
            l1[1] = o1 + Vec2::new((l0[1].x - o1.x).signum() * rng.range(0.1, 1.0) * size, 0.0);
        }
        Datum::Offsets => {
            let d = l0[1] - o1;
            l1[1] = o1 + Vec2::new(d.x.signum() * rng.range(0.1, 1.0) * size, d.y.signum() * rng.range(0.1, 1.0) * size);
        }
        Datum::Hinge => {
            l0[3].y = o1.y;
            l1[3].y = o1.y;
        }
    }
    let (o1i, o2i, p, q, r) = (0, 1, 2, 3, 4);
    let mut pairs = vec![(p, q), (q, r), (p, r), (o1i, p), (o1i, r)];
    let mut checks = vec![Check::Turn(p, q, r), Check::Turn(o1i, p, r)];
    match datum {
        Datum::Hinge => checks.push(Check::Side(o1i, q, Axis::X)),
        _ => {
            pairs.push((o2i, q));
            checks.push(Check::Turn(o1i, o2i, q));
        }
    }
    if matches!(datum, Datum::Horizontal) {
        pairs.push((o1i, o2i));
    }
    let ratio_ok = |a: f64, b: f64| b > 0.5 * a && b < 2.5 * a;
    let mut ok = pairs.iter().all(|&(a, b)| ratio_ok(l0[a].dist(l0[b]), l1[a].dist(l1[b])))
        && clear(&checks, &l0, size)
        && clear(&checks, &l1, size)
        && signature(&checks, &l0) == signature(&checks, &l1);
    if matches!(datum, Datum::Offsets) {
        let (d0, d1) = (l0[1] - o1, l1[1] - o1);
        ok &= ratio_ok(d0.x.abs(), d1.x.abs()) && ratio_ok(d0.y.abs(), d1.y.abs());
    }
    for a in 0..5 {
        for b in a + 1..5 {
            ok &= l0[a].dist(l0[b]) > 0.05 * size && l1[a].dist(l1[b]) > 0.05 * size;
        }
    }
    if !ok {
        return None;
    }

    let mut sk = Sketch::new();
    let mut ids = [0usize; 5];
    for (i, pos) in l0.iter().enumerate() {
        ids[i] = match (i, datum) {
            (0, Datum::Horizontal) => 0,
            (1, Datum::Hinge) => usize::MAX,
            _ => sk.add_point(*pos, None).ok()?,
        };
    }
    let fix = |sk: &mut Sketch, p: usize| sk.add_constraint(ConstraintKind::Fix { p }, None).map(|_| ());
    match datum {
        Datum::Fixed => {
            fix(&mut sk, ids[0]).ok()?;
            fix(&mut sk, ids[1]).ok()?;
        }
        Datum::Horizontal => {
            let l = sk.add_line_pts(ids[0], ids[1], None).ok()?;
            sk.add_constraint(ConstraintKind::Horizontal { l }, None).ok()?;
            sk.add_constraint(ConstraintKind::Length { l, value: l1[0].dist(l1[1]) }, None).ok()?;
        }
        Datum::Offsets => {
            fix(&mut sk, ids[0]).ok()?;
            let d = l1[1] - l1[0];
            sk.add_constraint(ConstraintKind::DistanceX { p: ids[0], q: ids[1], value: d.x.abs() }, None).ok()?;
            sk.add_constraint(ConstraintKind::DistanceY { p: ids[0], q: ids[1], value: d.y.abs() }, None).ok()?;
        }
        Datum::Hinge => {
            fix(&mut sk, ids[0]).ok()?;
            sk.add_constraint(ConstraintKind::HorizontalPoints { p: ids[0], q: ids[3] }, None).ok()?;
        }
    }
    for &(a, b) in pairs.iter().filter(|(a, b)| (*a, *b) != (o1i, o2i)) {
        sk.add_constraint(ConstraintKind::Distance { p: ids[a], q: ids[b], value: l1[a].dist(l1[b]) }, None).ok()?;
    }
    let mut target = current(&sk);
    for (i, id) in ids.iter().enumerate() {
        if let Some(t) = target.get_mut(*id) {
            *t = l1[i];
        }
    }
    let at = |i: usize| ids[i];
    let checks = checks
        .into_iter()
        .map(|c| match c {
            Check::Turn(a, b, c) => Check::Turn(at(a), at(b), at(c)),
            Check::Side(a, b, axis) => Check::Side(at(a), at(b), axis),
        })
        .collect();
    Some(Case { sk, target: Some(target), checks, size })
}

/// Fan-triangulated k-gon, v0 v1 fixed (dimensioned instead when not `anchored`: the polygon
/// floats), every other dimension scaled by `lo..hi`; only cases where every fan triangle stays
/// feasible.
fn fan(rng: &mut Lcg, k: usize, lo: f64, hi: f64, anchored: bool) -> Option<Case> {
    let pts: Vec<Vec2> = (0..k)
        .map(|i| {
            let t = i as f64 / k as f64 * std::f64::consts::TAU;
            let r = rng.range(30.0, 40.0);
            Vec2::new(r * t.cos(), r * t.sin())
        })
        .collect();
    let mut dist = std::collections::HashMap::new();
    for i in 1..k {
        dist.insert((i, (i + 1) % k), pts[i].dist(pts[(i + 1) % k]) * rng.range(lo, hi));
    }
    for i in 2..k - 1 {
        dist.insert((0, i), pts[0].dist(pts[i]) * rng.range(lo, hi));
    }
    dist.insert((0, 1), pts[0].dist(pts[1]));
    dist.insert((0, k - 1), dist[&(k - 1, 0)]);
    let feasible = (1..k - 1).all(|i| {
        let (a, b, c) = (dist[&(0, i)], dist[&(i, i + 1)], dist[&(0, i + 1)]);
        a + b > c * 1.02 && a + c > b * 1.02 && b + c > a * 1.02
    });
    if !feasible {
        return None;
    }
    // The sketch is built only for feasible draws (most are rejected).
    let mut sk = Sketch::new();
    let ids: Vec<usize> = pts.iter().map(|p| sk.add_point(*p, None)).collect::<Result<_, _>>().ok()?;
    if anchored {
        for &p in &ids[..2] {
            sk.add_constraint(ConstraintKind::Fix { p }, None).ok()?;
        }
    } else {
        sk.add_constraint(ConstraintKind::Distance { p: ids[0], q: ids[1], value: dist[&(0, 1)] }, None).ok()?;
    }
    for i in 1..k {
        let j = (i + 1) % k;
        sk.add_constraint(ConstraintKind::Distance { p: ids[i], q: ids[j], value: dist[&(i, j)] }, None).ok()?;
    }
    for i in 2..k - 1 {
        sk.add_constraint(ConstraintKind::Distance { p: ids[0], q: ids[i], value: dist[&(0, i)] }, None).ok()?;
    }
    let checks = (1..k - 1).map(|i| Check::Turn(ids[0], ids[i], ids[i + 1])).collect();
    Some(Case { sk, target: None, checks, size: 40.0 })
}

/// Two triangles joined by three links (triconnected, Owen §3.3.1 step 4): base 0 fixed,
/// 0–1 horizontal; every other dimension scaled by 0.8–1.25×.
fn linkage(rng: &mut Lcg) -> Option<Case> {
    const TRUE: [(f64, f64); 6] = [(0.0, 0.0), (40.0, 0.0), (18.0, 30.0), (8.0, 55.0), (52.0, 48.0), (30.0, 80.0)];
    const PAIRS: [(usize, usize); 9] = [(0, 1), (1, 2), (2, 0), (3, 4), (4, 5), (5, 3), (0, 3), (1, 4), (2, 5)];
    let mut sk = Sketch::new();
    let ids: Vec<usize> = TRUE.iter().map(|p| sk.add_point(Vec2::new(p.0, p.1), None)).collect::<Result<_, _>>().ok()?;
    sk.add_constraint(ConstraintKind::Fix { p: ids[0] }, None).ok()?;
    let l = sk.add_line_pts(ids[0], ids[1], None).ok()?;
    sk.add_constraint(ConstraintKind::Horizontal { l }, None).ok()?;
    for (a, b) in PAIRS {
        let d = sk.points[ids[a]].pos.dist(sk.points[ids[b]].pos) * rng.range(0.8, 1.25);
        sk.add_constraint(ConstraintKind::Distance { p: ids[a], q: ids[b], value: d }, None).ok()?;
    }
    let checks = vec![Check::Turn(ids[0], ids[1], ids[2]), Check::Turn(ids[3], ids[4], ids[5])];
    Some(Case { sk, target: None, checks, size: 40.0 })
}

/// A case from `make`, retried until feasible.
fn sample(rng: &mut Lcg, make: &mut dyn FnMut(&mut Lcg) -> Option<Case>) -> Case {
    for _ in 0..2_000_000 {
        if let Some(c) = make(rng) {
            return c;
        }
    }
    panic!("no feasible case in 2000000 draws");
}

/// Points reachable by point-by-point construction from the locked points (two loci on placed
/// points: circle, horizontal or vertical line), as the propagation seed does it.
fn propagation_places_all(sk: &Sketch) -> bool {
    let mut placed = sk.locked_points();
    let ends = |l: usize| match sk.curves.get(l).map(|c| &c.kind) {
        Some(CurveKind::Line { a, b }) => Some((*a, *b)),
        _ => None,
    };
    loop {
        let mut loci = vec![(0usize, false, false); sk.points.len()];
        let mut changed = false;
        for c in &sk.constraints {
            use ConstraintKind::*;
            let (p, q, kind) = match c.kind {
                Distance { p, q, .. } => (p, q, 0),
                Length { l, .. } => match ends(l) {
                    Some((a, b)) => (a, b, 0),
                    None => continue,
                },
                DistanceX { p, q, .. } | VerticalPoints { p, q } => (p, q, 1),
                DistanceY { p, q, .. } | HorizontalPoints { p, q } => (p, q, 2),
                Vertical { l } => match ends(l) {
                    Some((a, b)) => (a, b, 1),
                    None => continue,
                },
                Horizontal { l } => match ends(l) {
                    Some((a, b)) => (a, b, 2),
                    None => continue,
                },
                Coincident { p, q } => (p, q, 3),
                _ => continue,
            };
            for (free, base) in [(p, q), (q, p)] {
                if placed[base] && !placed[free] {
                    let l = &mut loci[free];
                    match kind {
                        0 => l.0 += 1,
                        1 => l.1 = true,
                        2 => l.2 = true,
                        _ => l.0 += 2,
                    }
                }
            }
        }
        for (i, (c, x, y)) in loci.into_iter().enumerate() {
            if !placed[i] && (c >= 2 || (c >= 1 && (x || y)) || (x && y)) {
                placed[i] = true;
                changed = true;
            }
        }
        if !changed {
            return placed.iter().all(|p| *p);
        }
    }
}

/// Same status, degrees of freedom and determined flags.
fn same_analysis(a: &SolveReport, b: &SolveReport) -> bool {
    a.status == b.status && a.dof == b.dof && a.point_determined == b.point_determined && a.curve_determined == b.curve_determined
}

#[test]
fn canonical_bracket_is_placed_exactly() {
    // O1 O2 fixed; rigid P Q R; |O1P| |O1R| |O2Q|.
    let truth = [Vec2::new(0.0, 0.0), Vec2::new(60.0, 0.0), Vec2::new(10.0, 30.0), Vec2::new(45.0, 35.0), Vec2::new(25.0, 55.0)];
    let checks = [Check::Turn(2, 3, 4), Check::Turn(0, 2, 4), Check::Turn(0, 1, 3)];
    // At the solution, and with the free points 6 mm away as after an edit. The shifted start
    // reverses O1→P→R (nearly collinear), so its orientation-keeping solution is a different
    // one of the eight; it must still be exact.
    for shift in [Vec2::ZERO, Vec2::new(6.0, -6.0)] {
        let mut sk = Sketch::new();
        let ids: Vec<usize> = truth.iter().enumerate().map(|(i, p)| sk.add_point(if i < 2 { *p } else { *p + shift }, None).unwrap()).collect();
        for &p in &ids[..2] {
            sk.add_constraint(ConstraintKind::Fix { p }, None).unwrap();
        }
        for (a, b) in [(2, 3), (3, 4), (2, 4), (0, 2), (0, 4), (1, 3)] {
            sk.add_constraint(ConstraintKind::Distance { p: ids[a], q: ids[b], value: truth[a].dist(truth[b]) }, None).unwrap();
        }
        assert!(!propagation_places_all(&sk), "the canonical case has no propagation order");
        let d = decompose_from(&sk, &known(&sk), &current(&sk));
        let placed: Vec<Vec2> = ids.iter().map(|id| d.pos[*id].unwrap()).collect();
        let start: Vec<Vec2> = ids.iter().map(|id| sk.points[*id].pos).collect();
        assert_eq!(signature(&checks, &placed), signature(&checks, &start), "every triangle keeps its orientation");
        if shift == Vec2::ZERO {
            assert!(placed.iter().zip(&truth).all(|(p, t)| p.dist(*t) < 1e-12), "{placed:?}");
        }
        // Placed by the cluster solve: every point not known on entry (P Q R).
        let given = known(&sk);
        assert!(ids.iter().all(|id| d.pos[*id].is_some()) && ids[2..].iter().all(|id| given[*id].is_none()));
        let mut s = seeded(&sk, &d);
        let rep = solve(&mut s);
        assert!(rep.ok() && rep.dof == 0, "{rep:?}");
        assert_eq!(rep.iterations, 0, "decomposed positions satisfy every constraint already");
    }
}

/// The ground grows by propagation, never by the single-point merge: merging clusters into it
/// leaves it no nodes to try.
#[test]
fn the_ground_has_no_nodes_to_grow() {
    let truth = [Vec2::new(0.0, 0.0), Vec2::new(60.0, 0.0), Vec2::new(10.0, 30.0), Vec2::new(45.0, 35.0), Vec2::new(25.0, 55.0)];
    let mut sk = Sketch::new();
    let ids: Vec<usize> = truth.iter().map(|p| sk.add_point(*p, None).unwrap()).collect();
    for &p in &ids[..2] {
        sk.add_constraint(ConstraintKind::Fix { p }, None).unwrap();
    }
    for (a, b) in [(2, 3), (3, 4), (2, 4), (0, 2), (0, 4), (1, 3)] {
        sk.add_constraint(ConstraintKind::Distance { p: ids[a], q: ids[b], value: truth[a].dist(truth[b]) }, None).unwrap();
    }
    let reading = crate::loci::read(&sk, &|i| sk.points.get(i).map(|p| p.pos), &crate::solver::points_on_curves(&sk), &crate::loci::radii(&sk));
    let given = known(&sk);
    let mut g = Graph::build(&reading, &|i| given.get(i).copied().flatten(), MAX_WORK).unwrap();
    g.reduce();
    let ground = g.cluster(g.ground.unwrap()).unwrap();
    assert!(ids.iter().all(|id| ground.pts.contains_key(id)), "the bracket reached the ground");
    assert!(ground.fresh.is_empty(), "{:?}", ground.fresh);
}

#[test]
fn bracket_family_keeps_every_triangle_under_large_edits() {
    let mut rng = Lcg(7);
    for datum in DATUMS {
        for scale in [0.2, 1.0, 10.0] {
            for _ in 0..40 {
                let case = sample(&mut rng, &mut |r| bracket(r, datum, scale));
                assert!(!propagation_places_all(&case.sk), "{datum:?}: propagation alone would place it");
                let d = decompose_from(&case.sk, &known(&case.sk), &current(&case.sk));
                let target = case.target.as_ref().unwrap();
                for (i, t) in target.iter().enumerate() {
                    let p = d.pos[i].unwrap_or_else(|| panic!("{datum:?} scale {scale}: point {i} unplaced"));
                    assert!(p.dist(*t) < 1e-9 * case.size.max(1.0) * 10.0, "{datum:?} scale {scale}: point {i} {p:?} vs {t:?}");
                }
                let mut s = seeded(&case.sk, &d);
                let rep = solve(&mut s);
                assert!(rep.ok() && rep.dof == 0, "{datum:?}: {rep:?}");
                assert_eq!(rep.iterations, 0, "{datum:?} scale {scale}");
                assert_eq!(signature(&case.checks, &current(&s)), signature(&case.checks, &current(&case.sk)));
                // The seed changes where the solver starts, never what it reports about the sketch.
                let lm = lm_alone(&mut case.sk.clone());
                if lm.ok() {
                    assert!(same_analysis(&lm, &rep), "{datum:?}: {lm:?} vs {rep:?}");
                }
            }
        }
    }
}

/// The numerical solver alone, from the sketch's positions (no constructive seed).
fn lm_alone(sk: &mut Sketch) -> SolveReport {
    switch::without_seed(|| solve(sk))
}

#[test]
fn bracket_family_is_solved_exactly_by_solve() {
    // Through `solve`, no hand-written seed: the decomposition inside the seed places every
    // point exactly, so LM has nothing to do and no sub-triangle mirrors.
    let mut rng = Lcg(11);
    for datum in DATUMS {
        for scale in [0.2, 1.0, 10.0] {
            for _ in 0..40 {
                let mut case = sample(&mut rng, &mut |r| bracket(r, datum, scale));
                let before = signature(&case.checks, &current(&case.sk));
                let rep = solve(&mut case.sk);
                assert!(rep.ok() && rep.dof == 0, "{datum:?} scale {scale}: {rep:?}");
                assert_eq!(rep.iterations, 0, "{datum:?} scale {scale}");
                assert_eq!(signature(&case.checks, &current(&case.sk)), before, "{datum:?} scale {scale}: mirrored");
                let target = case.target.as_ref().unwrap();
                for (i, t) in target.iter().enumerate() {
                    let p = case.sk.points[i].pos;
                    assert!(p.dist(*t) < 1e-8 * case.size.max(1.0), "{datum:?} scale {scale}: point {i} {p:?} vs {t:?}");
                }
            }
        }
    }
}

#[test]
fn fan_family_is_solved_exactly_by_solve() {
    let mut rng = Lcg(5);
    for k in [6, 8, 12] {
        for _ in 0..60 {
            let mut case = sample(&mut rng, &mut |r| fan(r, k, 0.5, 2.5, true));
            let before = signature(&case.checks, &current(&case.sk));
            let rep = solve(&mut case.sk);
            assert!(rep.ok() && rep.iterations == 0, "{k}-gon: {rep:?}");
            assert_eq!(signature(&case.checks, &current(&case.sk)), before, "{k}-gon mirrored");
        }
    }
}

#[test]
fn fan_family_keeps_every_triangle() {
    let mut rng = Lcg(99);
    for k in [4, 6, 8, 12] {
        for _ in 0..60 {
            let case = sample(&mut rng, &mut |r| fan(r, k, 0.5, 2.5, true));
            let d = decompose_from(&case.sk, &known(&case.sk), &current(&case.sk));
            assert!(d.pos.iter().all(Option::is_some), "{k}-gon: every point placed");
            let mut s = seeded(&case.sk, &d);
            let rep = solve(&mut s);
            assert!(rep.ok() && rep.iterations == 0, "{k}-gon: {rep:?}");
            assert_eq!(signature(&case.checks, &current(&s)), signature(&case.checks, &current(&case.sk)), "{k}-gon mirrored");
        }
    }
}

/// Mean position of the points.
fn centroid(pos: &[Vec2]) -> Vec2 {
    pos.iter().fold(Vec2::ZERO, |s, p| s + *p) / pos.len() as f64
}

/// A fan with no fixed point is one rigid cluster that never reaches the ground: it is placed
/// by its reference pose (the least-squares rotation and translation onto where it was), so no
/// triangle mirrors, the solver has nothing left to do, the rigid motion stays free (3 degrees
/// of freedom) and the polygon moves no more than the solver alone would move it.
#[test]
fn floating_fan_family_keeps_every_triangle() {
    let mut rng = Lcg(31);
    let mut lm_mirrored = 0;
    for k in [4, 6, 8, 12] {
        for _ in 0..60 {
            let case = sample(&mut rng, &mut |r| fan(r, k, 0.5, 2.5, false));
            let start = current(&case.sk);
            let before = signature(&case.checks, &start);
            let mut lm = case.sk.clone();
            let lm_rep = lm_alone(&mut lm);
            lm_mirrored += usize::from(lm_rep.ok() && signature(&case.checks, &current(&lm)) != before);
            let mut s = case.sk.clone();
            let rep = solve(&mut s);
            assert!(rep.ok() && rep.dof == 3 && rep.iterations == 0, "{k}-gon: {rep:?}");
            assert_eq!(signature(&case.checks, &current(&s)), before, "{k}-gon mirrored");
            if lm_rep.ok() {
                let (shift, lm_shift) = (centroid(&current(&s)).dist(centroid(&start)), centroid(&current(&lm)).dist(centroid(&start)));
                assert!(shift <= lm_shift + 1e-6, "{k}-gon: centroid moved {shift}, the solver alone {lm_shift}");
            }
        }
    }
    eprintln!("floating fans, solver alone: {lm_mirrored}/240 mirrored");
}

/// A rigid triangle pinned at one vertex is placed by its reference pose turned about the pin,
/// never reflected, and the turn stays free: a thin triangle (sides 3.9, 2, 2) whose vertex is
/// held and moved 5–40 mm in one jump, seeded on the positions before the move.
#[test]
fn a_cluster_pinned_at_one_point_turns_about_it() {
    let mut rng = Lcg(41);
    let mut lm_mirrored = 0;
    for _ in 0..200 {
        let mut sk = Sketch::new();
        let pin = Vec2::new(rng.range(-50.0, 50.0), rng.range(-50.0, 50.0));
        let turn = rng.range(0.0, std::f64::consts::TAU);
        let u = Vec2::from_angle(turn);
        let n = u.perp() * rng.sign();
        let pts = [pin, pin + u * 3.9, pin + u * 1.95 + n * (4.0f64 - 1.95 * 1.95).sqrt()];
        let ids: Vec<usize> = pts.iter().map(|p| sk.add_point(*p, None).unwrap()).collect();
        for (p, q, value) in [(0, 1, 3.9), (1, 2, 2.0), (2, 0, 2.0)] {
            sk.add_constraint(ConstraintKind::Distance { p: ids[p], q: ids[q], value }, None).unwrap();
        }
        let checks = [Check::Turn(ids[0], ids[1], ids[2])];
        let reference = current(&sk);
        let before = signature(&checks, &reference);
        let jump = rng.range(0.0, std::f64::consts::TAU);
        let to = pin + Vec2::from_angle(jump) * rng.range(5.0, 40.0);
        sk.points[ids[0]].pos = to;
        let hold = crate::solver::Hold { points: vec![ids[0]], radii: vec![] };
        let mut lm = sk.clone();
        let lm_rep = switch::without_seed(|| crate::solve_with(&mut lm, &hold, Some(&reference), crate::Analysis::Full, None));
        lm_mirrored += usize::from(lm_rep.ok() && signature(&checks, &current(&lm)) != before);
        let rep = crate::solve_with(&mut sk, &hold, Some(&reference), crate::Analysis::Full, None);
        assert!(rep.ok() && rep.dof == 1 && rep.iterations == 0, "{rep:?}");
        assert_eq!(signature(&checks, &current(&sk)), before, "mirrored");
        assert_eq!(sk.points[ids[0]].pos, to);
        // Moved with the pin, not turned: the reference pose is kept about it.
        for i in 1..3 {
            let (was, now) = (reference[ids[i]] - reference[ids[0]], sk.points[ids[i]].pos - to);
            assert!(was.dist(now) < 1e-9, "point {i} turned: {was:?} -> {now:?}");
        }
    }
    eprintln!("thin triangle pinned at a held vertex, one jump, solver alone: {lm_mirrored}/200 mirrored");
}

/// A rigid cluster whose reference pose breaks a relation to the placed points (the top of the
/// triconnected linkage, held by three links) is not floated: it is left to the solver.
#[test]
fn a_floated_pose_that_breaks_a_relation_is_left_to_the_solver() {
    let mut rng = Lcg(3);
    for _ in 0..20 {
        let case = sample(&mut rng, &mut linkage);
        let out = crate::tests::seed_of(&case.sk);
        assert!(out.pos[4..7].iter().all(Option::is_none), "{out:?}");
    }
}

#[test]
fn triconnected_linkage_is_left_to_the_solver() {
    let mut rng = Lcg(3);
    for _ in 0..20 {
        let case = sample(&mut rng, &mut linkage);
        let d = decompose_from(&case.sk, &known(&case.sk), &current(&case.sk));
        // The base triangle hangs off the fixed point and the horizontal line; the top
        // triangle and its three links are triconnected.
        assert!(d.pos[1..4].iter().all(Option::is_some));
        assert!(d.pos[4..7].iter().all(Option::is_none));
        let mut s = seeded(&case.sk, &d);
        let (rep, lm) = (solve(&mut s), lm_alone(&mut case.sk.clone()));
        if lm.ok() {
            assert!(rep.ok() && same_analysis(&lm, &rep), "{lm:?} vs {rep:?}");
        }
    }
}

#[test]
fn branch_follows_the_reference_not_the_current_geometry() {
    // Base fixed, apex currently above it; the reference (before a drag) has it below.
    let mut sk = Sketch::new();
    let a = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
    let b = sk.add_point(Vec2::new(40.0, 0.0), None).unwrap();
    let c = sk.add_point(Vec2::new(20.0, 15.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: a }, None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: b }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: a, q: c, value: 25.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: b, q: c, value: 25.0 }, None).unwrap();
    let mut reference = current(&sk);
    reference[c] = Vec2::new(20.0, -15.0);
    let d = decompose_from(&sk, &known(&sk), &reference);
    assert!(d.pos[c].unwrap().dist(Vec2::new(20.0, -15.0)) < 1e-12);
    let d = decompose_from(&sk, &known(&sk), &current(&sk));
    assert!(d.pos[c].unwrap().dist(Vec2::new(20.0, 15.0)) < 1e-12);
}

#[test]
fn rigid_cluster_with_a_horizontal_edge_is_oriented() {
    // P fixed; triangle P Q R dimensioned; Q–R horizontal. Neither Q nor R has two loci on
    // placed points; the triangle turns about nothing until Q–R is level, then translates
    // onto P. Q is left of R in the reference, R below P.
    let mut sk = Sketch::new();
    let p = sk.add_point(Vec2::new(10.0, 10.0), None).unwrap();
    let q = sk.add_point(Vec2::new(0.0, -21.0), None).unwrap();
    let r = sk.add_point(Vec2::new(31.0, -19.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p }, None).unwrap();
    let truth = [Vec2::new(10.0, 10.0), Vec2::new(-2.0, -20.0), Vec2::new(30.0, -20.0)];
    for (a, b) in [(0, 1), (1, 2), (0, 2)] {
        let ids = [p, q, r];
        sk.add_constraint(ConstraintKind::Distance { p: ids[a], q: ids[b], value: truth[a].dist(truth[b]) }, None).unwrap();
    }
    let l = sk.add_line_pts(q, r, None).unwrap();
    sk.add_constraint(ConstraintKind::Horizontal { l }, None).unwrap();
    assert!(!propagation_places_all(&sk));
    let d = decompose_from(&sk, &known(&sk), &current(&sk));
    assert!(d.pos[q].unwrap().dist(truth[1]) < 1e-12 && d.pos[r].unwrap().dist(truth[2]) < 1e-12, "{d:?}");
    let mut s = seeded(&sk, &d);
    let rep = solve(&mut s);
    assert!(rep.ok() && rep.iterations == 0, "{rep:?}");
}

#[test]
fn a_busy_ground_point_does_not_exhaust_the_budget() {
    // 1000 points hang off the fixed hub by one distance each (they never merge); a strip
    // of triangles grows from the hub and a second fixed point. The strip is placed.
    let mut sk = Sketch::new();
    let hub = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
    let second = sk.add_point(Vec2::new(10.0, 0.0), None).unwrap();
    for p in [hub, second] {
        sk.add_constraint(ConstraintKind::Fix { p }, None).unwrap();
    }
    for i in 0..1000 {
        let t = i as f64 * 0.0063;
        let p = sk.add_point(Vec2::new(-30.0 * t.cos(), -30.0 - 30.0 * t.sin()), None).unwrap();
        sk.add_constraint(ConstraintKind::Distance { p: hub, q: p, value: 30.0 }, None).unwrap();
    }
    let mut strip = vec![hub, second];
    let mut truth = vec![Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0)];
    for j in 2..200 {
        let t = Vec2::new(j as f64 * 5.0 + 7.0, (j % 2) as f64 * 8.0 + 4.0);
        let p = sk.add_point(t + Vec2::new(0.3, -0.2), None).unwrap();
        for back in [1, 2] {
            let (q, tq) = (strip[j - back], truth[j - back]);
            sk.add_constraint(ConstraintKind::Distance { p: q, q: p, value: tq.dist(t) }, None).unwrap();
        }
        strip.push(p);
        truth.push(t);
    }
    let d = decompose_from(&sk, &known(&sk), &current(&sk));
    assert_placed(&d, &strip, &truth);
}

/// A drag frame's decomposition stops at the frame budget (about 16 ms) where the full one goes
/// on (about 0.2 s here); the frame's solve still solves, from LM.
#[test]
fn a_drag_frame_decomposes_within_its_budget() {
    let mut sk = Sketch::new();
    let hub = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
    let second = sk.add_point(Vec2::new(10.0, 0.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: hub }, None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: second }, None).unwrap();
    let mut strip = vec![(hub, Vec2::new(0.0, 0.0)), (second, Vec2::new(10.0, 0.0))];
    for i in 0..1000 {
        let t = i as f64 * 0.0063;
        let p = sk.add_point(Vec2::new(-30.0 * t.cos(), -30.0 - 30.0 * t.sin()), None).unwrap();
        sk.add_constraint(ConstraintKind::Distance { p: hub, q: p, value: 30.0 }, None).unwrap();
        let j = i + 2;
        let at = Vec2::new(j as f64 * 5.0 + 7.0, (j % 2) as f64 * 8.0 + 4.0);
        let p = sk.add_point(at + Vec2::new(0.3, -0.2), None).unwrap();
        for back in [1, 2] {
            let (q, tq) = strip[strip.len() - back];
            sk.add_constraint(ConstraintKind::Distance { p: q, q: p, value: tq.dist(at) }, None).unwrap();
        }
        strip.push((p, at));
    }
    let reading = crate::loci::read(&sk, &|i| sk.points.get(i).map(|p| p.pos), &crate::solver::points_on_curves(&sk), &crate::loci::radii(&sk));
    let work = |limit: usize| Decomposition::new(&reading, &known(&sk), true, limit).work().0;
    let (frame, full) = (work(FRAME_WORK), work(MAX_WORK));
    assert!(frame > FRAME_WORK / 2 && frame <= FRAME_WORK + FRAME_WORK / 100, "frame work {frame}");
    assert!(full > 10 * FRAME_WORK, "full work {full}");
    let hold = crate::Hold::default();
    assert!(
        switch::without_differential(|| crate::solve_with(&mut sk.clone(), &hold, None, crate::Analysis::StatusOnly, Some(&mut Budget::frame())))
            .ok()
    );
}

/// Wherever the budget runs out, the merge search stops within one point's scan of it: a
/// rigid triangle grows from a hub with thousands of relations, each one a candidate.
#[test]
fn growth_stops_at_the_budget() {
    let mut sk = Sketch::new();
    let h = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
    let a = sk.add_point(Vec2::new(10.0, 0.0), None).unwrap();
    let b = sk.add_point(Vec2::new(3.0, 8.0), None).unwrap();
    for (p, q) in [(h, a), (a, b), (b, h)] {
        let d = sk.points[p].pos.dist(sk.points[q].pos);
        sk.add_constraint(ConstraintKind::Distance { p, q, value: d }, None).unwrap();
    }
    let n = 1000;
    for i in 0..n {
        let s = sk.add_point(Vec2::from_angle(i as f64 * 0.002) * 20.0, None).unwrap();
        sk.add_constraint(ConstraintKind::Distance { p: h, q: s, value: 20.0 }, None).unwrap();
    }
    let reading = crate::loci::read(&sk, &|i| sk.points.get(i).map(|p| p.pos), &crate::solver::points_on_curves(&sk), &crate::loci::radii(&sk));
    let none = |_: usize| None;
    let mut full = Graph::build(&reading, &none, MAX_WORK).unwrap();
    full.reduce();
    let total = full.work;
    assert!(total > 2 * n, "work {total}");
    // One point's scan: the hub's relations, and what its candidates cost.
    let scan = n + 16;
    let worst = (1..total).step_by((total / 60).max(1)).map(|limit| {
        let mut g = Graph::build(&reading, &none, limit).unwrap();
        g.reduce();
        g.work.saturating_sub(limit)
    });
    let worst = worst.max().unwrap_or(0);
    assert!(worst <= scan, "overshoot {worst} past the budget");
}

#[test]
fn work_is_bounded_on_a_star_of_oriented_pairs() {
    // 20,000 points at a DistanceX and a DistanceY from one fixed point (the cluster cap).
    let mut sk = Sketch::new();
    let n = MAX_CLUSTERS;
    for i in 0..n {
        let pos = Vec2::new((i % 200) as f64 + 1.0, (i / 200) as f64 + 1.0);
        push_point(&mut sk, pos);
        sk.constraints.push(raw(ConstraintKind::DistanceX { p: 0, q: i + 1, value: pos.x }));
        sk.constraints.push(raw(ConstraintKind::DistanceY { p: 0, q: i + 1, value: pos.y }));
    }
    let given = known(&sk);
    let reading = crate::loci::read(&sk, &|i| sk.points.get(i).map(|p| p.pos), &crate::solver::points_on_curves(&sk), &crate::loci::radii(&sk));
    let mut g = Graph::build(&reading, &|i| given.get(i).copied().flatten(), MAX_WORK).unwrap();
    g.reduce();
    assert!(g.work < MAX_WORK / 10, "work {}", g.work);
    let ground = g.cluster(g.ground.unwrap()).unwrap();
    assert_eq!(ground.pts.len(), n + 1);
    assert!(ground.pts.iter().all(|(i, p)| p.dist(sk.points[*i].pos) < 1e-12));
}

#[test]
fn orient_tries_every_bond() {
    // P fixed; the rigid body P S Q R T: R coincides with Q in it (same distances to P and
    // S, same side), so the horizontal relation Q–R can't turn it; Q–T horizontal does.
    let truth = [Vec2::new(0.0, 0.0), Vec2::new(12.0, 0.0), Vec2::new(6.0, 8.0), Vec2::new(6.0, 8.0), Vec2::new(20.0, 8.0)];
    let turned = |v: Vec2| Vec2::new(v.x * 0.98 - v.y * 0.17, v.x * 0.17 + v.y * 0.98);
    let mut sk = Sketch::new();
    let ids: Vec<usize> = truth
        .iter()
        .enumerate()
        .map(|(i, t)| sk.add_point(turned(*t) + if i == 3 { Vec2::new(0.2, -0.1) } else { Vec2::ZERO }, None).unwrap())
        .collect();
    // [P, S, Q, R, T]
    sk.add_constraint(ConstraintKind::Fix { p: ids[0] }, None).unwrap();
    for (a, b) in [(0, 1), (0, 2), (1, 2), (0, 3), (1, 3), (0, 4), (1, 4)] {
        sk.add_constraint(ConstraintKind::Distance { p: ids[a], q: ids[b], value: truth[a].dist(truth[b]) }, None).unwrap();
    }
    sk.add_constraint(ConstraintKind::HorizontalPoints { p: ids[2], q: ids[3] }, None).unwrap();
    sk.add_constraint(ConstraintKind::HorizontalPoints { p: ids[2], q: ids[4] }, None).unwrap();
    let d = decompose_from(&sk, &known(&sk), &current(&sk));
    assert_placed(&d, &ids, &truth);
}

#[test]
fn signs_come_from_the_constrained_point_not_its_coincident_group() {
    // p fixed; q coincident with p but drawn 20 mm right of it; r 5 mm from q along x and
    // level with it, drawn left of q (right of p). r goes to q's left.
    let mut sk = Sketch::new();
    let p = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
    let q = sk.add_point(Vec2::new(20.0, 0.0), None).unwrap();
    let r = sk.add_point(Vec2::new(10.0, 1.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p }, None).unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p, q }, None).unwrap();
    sk.add_constraint(ConstraintKind::DistanceX { p: q, q: r, value: 5.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::HorizontalPoints { p: q, q: r }, None).unwrap();
    let d = decompose_from(&sk, &known(&sk), &current(&sk));
    assert_eq!(d.pos[q], Some(Vec2::ZERO));
    assert_eq!(d.pos[r], Some(Vec2::new(-5.0, 0.0)));
}

#[test]
fn triangle_turn_comes_from_the_constrained_point() {
    // C (drawn below the base) is coincident with D (drawn above); the distances are written
    // on D, so the apex goes above.
    let mut sk = Sketch::new();
    let a = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
    let b = sk.add_point(Vec2::new(40.0, 0.0), None).unwrap();
    let c = sk.add_point(Vec2::new(20.0, -15.0), None).unwrap();
    let d = sk.add_point(Vec2::new(20.0, 15.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: a }, None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: b }, None).unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: c, q: d }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: a, q: d, value: 25.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: b, q: d, value: 25.0 }, None).unwrap();
    let out = decompose_from(&sk, &known(&sk), &current(&sk));
    assert_placed(&out, &[c, d], &[Vec2::new(20.0, 15.0); 2]);
}

#[test]
fn hinge_side_comes_from_the_constrained_pivot() {
    // F and E fixed; G coincident with F but drawn at (−40, 0); the rigid triangle G P Q is
    // written on G and turns about F until Q is level with E. Q is drawn right of G (left
    // of F), so it lands right of the pin.
    let mut sk = Sketch::new();
    let f = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
    let e = sk.add_point(Vec2::new(5.0, 0.0), None).unwrap();
    let g = sk.add_point(Vec2::new(-40.0, 0.0), None).unwrap();
    let p = sk.add_point(Vec2::new(-30.0, 10.0), None).unwrap();
    let q = sk.add_point(Vec2::new(-20.0, 3.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: f }, None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: e }, None).unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: f, q: g }, None).unwrap();
    for (a, b) in [(g, p), (g, q), (p, q)] {
        let value = sk.points[a].pos.dist(sk.points[b].pos);
        sk.add_constraint(ConstraintKind::Distance { p: a, q: b, value }, None).unwrap();
    }
    sk.add_constraint(ConstraintKind::HorizontalPoints { p: e, q }, None).unwrap();
    let out = decompose_from(&sk, &known(&sk), &current(&sk));
    let radius = sk.points[g].pos.dist(sk.points[q].pos);
    assert_placed(&out, &[q], &[Vec2::new(radius, 0.0)]);
}

#[test]
fn zero_distance_joins_points() {
    // C and C2 at distance zero carry one side each of the triangle on the fixed base.
    let mut sk = Sketch::new();
    let a = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
    let b = sk.add_point(Vec2::new(40.0, 0.0), None).unwrap();
    let c = sk.add_point(Vec2::new(20.0, 10.0), None).unwrap();
    let c2 = sk.add_point(Vec2::new(21.0, 11.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: a }, None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: b }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: c, q: c2, value: 0.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: a, q: c, value: 25.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: b, q: c2, value: 25.0 }, None).unwrap();
    let d = decompose_from(&sk, &known(&sk), &current(&sk));
    assert_eq!(d.pos[c], d.pos[c2]);
    assert!(d.pos[c].is_some_and(|v| v.dist(Vec2::new(20.0, 15.0)) < 1e-12), "{d:?}");
}

#[test]
fn coincident_points_and_unreachable_triangles() {
    // C is coincident with C2, which carries the second distance; the triangle closes.
    let mut sk = Sketch::new();
    let a = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
    let b = sk.add_point(Vec2::new(40.0, 0.0), None).unwrap();
    let c = sk.add_point(Vec2::new(20.0, 10.0), None).unwrap();
    let c2 = sk.add_point(Vec2::new(21.0, 11.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: a }, None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: b }, None).unwrap();
    sk.add_constraint(ConstraintKind::Coincident { p: c, q: c2 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Distance { p: a, q: c, value: 25.0 }, None).unwrap();
    let id = sk.add_constraint(ConstraintKind::Distance { p: b, q: c2, value: 25.0 }, None).unwrap();
    let d = decompose_from(&sk, &known(&sk), &current(&sk));
    assert_eq!(d.pos[c], d.pos[c2]);
    assert!(d.pos[c].unwrap().dist(Vec2::new(20.0, 15.0)) < 1e-12);
    // Sides that can't meet: nothing placed, the solver reports the conflict.
    sk.constraints.iter_mut().find(|k| k.id == id).unwrap().kind.set_value(5.0);
    let d = decompose_from(&sk, &known(&sk), &current(&sk));
    assert!(d.pos[c].is_none() && d.pos[c2].is_none());
}

#[test]
fn hostile_input_never_panics() {
    let mut sk = Sketch::new();
    let p = sk.add_point(Vec2::new(1.0, 1.0), None).unwrap();
    let q = sk.add_point(Vec2::new(5.0, 1.0), None).unwrap();
    let r = sk.add_point(Vec2::new(3.0, 4.0), None).unwrap();
    for kind in [
        ConstraintKind::Distance { p, q: 999, value: 1.0 },
        ConstraintKind::Distance { p, q, value: f64::NAN },
        ConstraintKind::Distance { p, q: r, value: f64::INFINITY },
        ConstraintKind::Distance { p: q, q: r, value: -3.0 },
        ConstraintKind::Distance { p, q: p, value: 2.0 },
        ConstraintKind::Length { l: 77, value: 1.0 },
        ConstraintKind::Horizontal { l: 3 },
        ConstraintKind::DistanceX { p, q, value: 1e300 },
        ConstraintKind::DistanceY { p, q, value: 1e300 },
        ConstraintKind::Coincident { p: 999, q: p },
        ConstraintKind::Coincident { p, q },
        ConstraintKind::Coincident { p: q, q: p },
        ConstraintKind::Fix { p: 12345 },
    ] {
        sk.constraints.push(raw(kind));
    }
    let n = sk.points.len();
    let nan = Vec2::new(f64::NAN, f64::INFINITY);
    for known in [vec![], vec![Some(nan); n], vec![Some(Vec2::ZERO); n + 5], vec![None, Some(Vec2::new(1e300, -1e300))]] {
        for reference in [vec![], vec![nan; n], vec![Vec2::ZERO; 1]] {
            let d = decompose_from(&sk, &known, &reference);
            assert_eq!(d.pos.len(), n);
            assert!(d.pos.iter().flatten().all(|v| v.is_finite()));
        }
    }
    // Every point coincident with its neighbour, all on the origin: one node, nothing to do.
    let mut chain = Sketch::new();
    for i in 0..200 {
        chain.add_point(Vec2::new(i as f64, 0.0), None).unwrap();
    }
    for i in 1..chain.points.len() {
        chain.constraints.push(raw(ConstraintKind::Coincident { p: i, q: i - 1 }));
    }
    let d = decompose_from(&chain, &known(&chain), &[]);
    assert!(d.pos.iter().all(|p| *p == Some(Vec2::ZERO)));
}

/// Both readers of the constraints (propagation and decomposition) share one validation:
/// a dimension beyond the sketch's coordinate range is read by neither.
#[test]
fn oversized_dimensions_are_read_by_neither() {
    let mut sk = Sketch::new();
    let a = sk.add_point(Vec2::new(0.0, 0.0), None).unwrap();
    let b = sk.add_point(Vec2::new(5.0, 1.0), None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: a }, None).unwrap();
    sk.constraints.push(raw(ConstraintKind::Distance { p: a, q: b, value: 1e15 }));
    let reading = crate::loci::read(&sk, &|i| sk.points.get(i).map(|p| p.pos), &crate::solver::points_on_curves(&sk), &crate::loci::radii(&sk));
    assert_eq!(reading.rels.len(), 0);
    let given = known(&sk);
    let g = Graph::build(&reading, &|i| given.get(i).copied().flatten(), MAX_WORK).unwrap();
    assert_eq!(g.clusters.len(), 1, "the ground only");
}

#[test]
fn large_inputs_are_capped() {
    // More starting clusters than the cap: left to the solver untouched.
    let mut sk = Sketch::new();
    let n = 210;
    for i in 0..n {
        push_point(&mut sk, Vec2::new(i as f64, (i * i % 17) as f64));
    }
    for i in 0..n {
        for j in i + 1..n {
            sk.constraints.push(raw(ConstraintKind::Distance { p: i, q: j, value: 1.0 + (i + j) as f64 }));
        }
    }
    assert!(sk.constraints.len() > MAX_CLUSTERS);
    let d = decompose_from(&sk, &known(&sk), &current(&sk));
    assert!(d.pos.iter().all(Option::is_none), "no graph, nothing placed");

    // A dense complete graph under the cap finishes (the merge loop is bounded).
    let mut sk = Sketch::new();
    let pts: Vec<Vec2> = (0..60).map(|i| Vec2::new((i as f64 * 1.7).sin() * 50.0, (i as f64 * 2.3).cos() * 50.0)).collect();
    let ids: Vec<usize> = pts.iter().map(|p| sk.add_point(*p, None).unwrap()).collect();
    sk.add_constraint(ConstraintKind::Fix { p: ids[0] }, None).unwrap();
    sk.add_constraint(ConstraintKind::Fix { p: ids[1] }, None).unwrap();
    for i in 0..ids.len() {
        for j in i + 1..ids.len() {
            sk.add_constraint(ConstraintKind::Distance { p: ids[i], q: ids[j], value: pts[i].dist(pts[j]) }, None).unwrap();
        }
    }
    let d = decompose_from(&sk, &known(&sk), &current(&sk));
    for (i, id) in ids.iter().enumerate() {
        assert!(d.pos[*id].unwrap().dist(pts[i]) < 1e-6);
    }
}

/// Measurement, not a gate: per family, through `solve`, how often a sub-triangle mirrors and
/// how often the solve fails, with the numerical solver alone, with the propagation seed (the
/// solver before the decomposition was part of the seed) and with the full seed. `gap`: cases
/// propagation cannot place (see [`propagation_places_all`]) that the solve mirrors.
#[test]
#[ignore]
fn gap_counter() {
    let mut rng = Lcg(2026);
    let n = 500;
    let mut families: Vec<(String, Box<dyn FnMut(&mut Lcg) -> Option<Case>>)> = Vec::new();
    for datum in DATUMS {
        families.push((
            format!("bracket {datum:?}"),
            Box::new(move |r: &mut Lcg| {
                let scale = if r.next() < 0.5 { 1.0 } else { 10.0 };
                bracket(r, datum, scale)
            }),
        ));
    }
    for k in [6, 8, 12] {
        families.push((format!("fan {k}-gon 0.5–2.5×"), Box::new(move |r: &mut Lcg| fan(r, k, 0.5, 2.5, true))));
        families.push((format!("floating fan {k}-gon 0.5–2.5×"), Box::new(move |r: &mut Lcg| fan(r, k, 0.5, 2.5, false))));
    }
    families.push(("triconnected linkage 0.8–1.25×".into(), Box::new(linkage)));
    println!(
        "family | cases | not propagatable | LM alone: failed / mirrored | propagation seed: failed / mirrored / gap | full seed: failed / mirrored / gap / iterations>0 | full vs LM alone: worse / better"
    );
    for (name, make) in &mut families {
        let (mut unreach, mut iterated, mut worse, mut better) = (0, 0, 0, 0);
        let mut tally = [Tally::default(); 3];
        for _ in 0..n {
            let case = sample(&mut rng, &mut **make);
            let before = signature(&case.checks, &current(&case.sk));
            let reachable = propagation_places_all(&case.sk);
            unreach += usize::from(!reachable);
            let solved = Mode::ALL.map(|mode| {
                let mut s = case.sk.clone();
                let rep = match mode {
                    Mode::LmAlone => switch::without_seed(|| solve(&mut s)),
                    Mode::Propagation => switch::without_decomposition(|| solve(&mut s)),
                    Mode::Full => solve(&mut s),
                };
                let mirrored = rep.ok() && signature(&case.checks, &current(&s)) != before;
                (rep, mirrored)
            });
            for (t, (rep, mirrored)) in tally.iter_mut().zip(&solved) {
                t.failed += usize::from(!rep.ok());
                t.mirrored += usize::from(*mirrored);
                t.gap += usize::from(*mirrored && !reachable);
            }
            let [(lm, _), _, (full, _)] = &solved;
            iterated += usize::from(full.iterations > 0);
            worse += usize::from(lm.ok() && !full.ok());
            better += usize::from(!lm.ok() && full.ok());
        }
        let [a, p, f] = tally;
        println!(
            "{name} | {n} | {unreach} | {} / {} | {} / {} / {} | {} / {} / {} / {iterated} | {worse} / {better}",
            a.failed, a.mirrored, p.failed, p.mirrored, p.gap, f.failed, f.mirrored, f.gap
        );
    }
}

/// How `gap_counter` solves: LM from the sketch's positions, the propagation seed only, the
/// full seed.
#[derive(Clone, Copy)]
enum Mode {
    LmAlone,
    Propagation,
    Full,
}

impl Mode {
    const ALL: [Mode; 3] = [Mode::LmAlone, Mode::Propagation, Mode::Full];
}

#[derive(Clone, Copy, Default)]
struct Tally {
    failed: usize,
    mirrored: usize,
    gap: usize,
}
