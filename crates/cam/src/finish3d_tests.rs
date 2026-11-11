//! Tests of the 3D finishing strategies: scallop heights and coverage measured independently
//! of the strategies (the envelope of the cutter over the path against the best the tool can
//! do), on a dome, a bowl with a concave fillet and a stepped part; the crease of a fillet
//! joint; rest material in a corner; and hostile parameters.

use std::f64::consts::{PI, TAU};

use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::foundation_tests::*;
use crate::*;

fn ball(d: f64) -> Tool {
    Tool::new(1, ToolKind::BallEndmill, d, 20.0)
}

fn flat(d: f64) -> Tool {
    Tool::new(2, ToolKind::FlatEndmill, d, 20.0)
}

fn feeds() -> Feeds {
    Feeds { cut: 800.0, plunge: 200.0 }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Boundary {
    Boundary { rect: Some([Vec2::new(x0, y0), Vec2::new(x1, y1)]), region: None }
}

fn joined(parts: &[Surface]) -> Surface {
    Surface::new(parts.iter().flat_map(|s| s.triangles().to_vec()).collect()).unwrap()
}

/// A 40 mm square plate at Z 0 with a dome of radius 12 rising 7 mm out of it.
fn dome() -> Surface {
    joined(&[plane(20.0, 0.0), sphere(Vec3::new(0.0, 0.0, -5.0), 12.0, 72)])
}

/// The profile `(x, z)` swept along Y from `y0` to `y1`.
fn swept(profile: &[(f64, f64)], y0: f64, y1: f64) -> Surface {
    let mut tris = Vec::new();
    for w in profile.windows(2) {
        let (a, b) = (w[0], w[1]);
        let p = |x: f64, y: f64, z: f64| Vec3::new(x, y, z);
        tris.push([p(a.0, y0, a.1), p(b.0, y0, b.1), p(b.0, y1, b.1)]);
        tris.push([p(a.0, y0, a.1), p(b.0, y1, b.1), p(a.0, y1, a.1)]);
    }
    Surface::new(tris).unwrap()
}

/// The profile `(r, z)` turned about the Z axis in `n` steps.
fn turned(profile: &[(f64, f64)], n: usize) -> Surface {
    let mut tris = Vec::new();
    let at = |r: f64, z: f64, k: usize| {
        let th = TAU * k as f64 / n as f64;
        Vec3::new(r * th.cos(), r * th.sin(), z)
    };
    for w in profile.windows(2) {
        let (a, b) = (w[0], w[1]);
        for k in 0..n {
            tris.push([at(a.0, a.1, k), at(b.0, b.1, k), at(b.0, b.1, k + 1)]);
            tris.push([at(a.0, a.1, k), at(b.0, b.1, k + 1), at(a.0, a.1, k + 1)]);
        }
    }
    Surface::new(tris).unwrap()
}

/// Points of a quarter circle about `c` of radius `r` from angle `a0` to `a1`.
fn arc(c: (f64, f64), r: f64, a0: f64, a1: f64, n: usize) -> Vec<(f64, f64)> {
    (0..=n).map(|k| a0 + (a1 - a0) * k as f64 / n as f64).map(|a| (c.0 + r * a.cos(), c.1 + r * a.sin())).collect()
}

/// A bowl: a floor of radius 6 at Z 0, a concave fillet of radius 4 up to a wall at 60°, a
/// rim at Z 10 out to radius 22.
fn bowl() -> Surface {
    let wall = 60f64.to_radians();
    let rf = 4.0;
    // The fillet's centre is above the floor's edge; it meets the wall where its normal is
    // square to the wall.
    let mut p = vec![(0.0, 0.0), (6.0, 0.0)];
    p.extend(arc((6.0, rf), rf, -PI / 2.0, -PI / 2.0 + wall, 24).into_iter().skip(1));
    let &(x, z) = p.last().unwrap();
    let rise = 10.0 - z;
    p.push((x + rise / wall.tan(), 10.0));
    p.push((22.0, 10.0));
    turned(&p, 96)
}

/// A floor at Z 0 for X below 0, a wall at X 0 up to Z 6 with a concave fillet of radius
/// `fillet` at its foot (0: a sharp corner), and a top at Z 6.
fn fillet_joint(fillet: f64) -> Surface {
    let mut p = vec![(-20.0, 0.0)];
    if fillet > 0.0 {
        p.extend(arc((-fillet, fillet), fillet, -PI / 2.0, 0.0, 16));
    } else {
        p.push((0.0, 0.0));
    }
    p.push((0.0, 6.0));
    p.push((20.0, 6.0));
    swept(&p, -12.0, 12.0)
}

/// Three treads 4 mm up from one another, risers at X -5 and 5.
fn stepped() -> Surface {
    swept(&[(-20.0, 0.0), (-5.0, 0.0), (-5.0, 4.0), (5.0, 4.0), (5.0, 8.0), (20.0, 8.0)], -12.0, 12.0)
}

fn heights(top: f64) -> Heights {
    Heights { clearance: top + 20.0, retract: top + 5.0, top, bottom: -1.0 }
}

fn scallop_params(h: Heights, b: Boundary, spacing: Spacing) -> ScallopParams {
    ScallopParams {
        heights: h,
        feeds: feeds(),
        descent_feed: None,
        spacing,
        direction: OffsetDirection::Inward,
        boundary: b,
        stock_to_leave: 0.0,
        step: 0.25,
        tolerance: 0.005,
        climb: true,
    }
}

/// The tip at every move at feed, from where the tool was.
fn feed_moves(path: &Toolpath) -> Vec<(Vec3, Vec3)> {
    let mut out = Vec::new();
    let mut pos: Option<Vec3> = None;
    for m in &path.moves {
        if let (Some(p), Move::Feed { to, .. }) = (pos, *m) {
            out.push((p, to));
        }
        pos = m.target().or(pos);
    }
    out
}

/// No move at feed goes into the surface by more than the tolerance between its ends, and
/// every move down at least as steeply as the default descent limit (70°) is no faster down
/// than `f.cut` (the descent feed is the cut feed by default; give the descent feed there to
/// check a slower one).
fn assert_safe(s: &Surface, tool: &Tool, path: &Toolpath, tol: f64, f: Feeds) {
    let c = tool.cutter().unwrap();
    let mut pos: Option<Vec3> = None;
    for m in &path.moves {
        if let (Some(a), Move::Feed { to: b, feed }) = (pos, *m) {
            for u in [0.0, 0.25, 0.5, 0.75, 1.0] {
                let q = a.lerp(b, u);
                if let Some(z) = drop_z(s, &c, q.x, q.y) {
                    assert!(q.z >= z - tol - 1e-9, "{a:?} → {b:?} cuts {} into the surface", z - q.z);
                }
            }
            let (len, down) = (a.dist(b), a.z - b.z);
            if down > 1e-9 && len > 1e-9 && down >= DEFAULT_DESCENT_LIMIT.sin() * len - 1e-12 {
                assert!(feed * down / len <= f.cut + 1e-6, "{a:?} → {b:?} goes down at {}", feed * down / len);
            }
        }
        pos = m.target().or(pos);
    }
}

/// Heights over a grid of points, for measuring what a path leaves.
struct Eval {
    lo: Vec2,
    h: f64,
    n: usize,
}

impl Eval {
    fn new(lo: Vec2, hi: Vec2, h: f64) -> Eval {
        let n = (((hi.x - lo.x).max(hi.y - lo.y)) / h).ceil() as usize + 1;
        Eval { lo, h, n }
    }

    fn point(&self, k: usize) -> Vec2 {
        Vec2::new(self.lo.x + self.h * (k % self.n) as f64, self.lo.y + self.h * (k / self.n) as f64)
    }

    /// The lowest the cutter's surface reaches over each point with its tip at `tips`.
    fn envelope(&self, c: &Cutter, tips: &[Vec3]) -> Vec<f64> {
        let mut z = vec![f64::INFINITY; self.n * self.n];
        let reach = (c.radius / self.h).ceil() as isize + 1;
        for t in tips {
            let (ci, cj) = (((t.x - self.lo.x) / self.h).round() as isize, ((t.y - self.lo.y) / self.h).round() as isize);
            for j in (cj - reach).max(0)..=(cj + reach).min(self.n as isize - 1) {
                for i in (ci - reach).max(0)..=(ci + reach).min(self.n as isize - 1) {
                    let k = j as usize * self.n + i as usize;
                    let p = self.point(k);
                    if let Some(h) = c.height(p.dist(Vec2::new(t.x, t.y))) {
                        z[k] = z[k].min(t.z + h);
                    }
                }
            }
        }
        z
    }
}

/// Tips along every move at feed, at most `spacing` apart.
fn path_tips(path: &Toolpath, spacing: f64) -> Vec<Vec3> {
    let mut out = Vec::new();
    for (a, b) in feed_moves(path) {
        let n = (a.dist(b) / spacing).ceil().max(1.0) as usize;
        out.extend((0..=n).map(|k| a.lerp(b, k as f64 / n as f64)));
    }
    out
}

/// Where the cutter rests over a grid `spacing` apart covering `lo..hi`: every place it can
/// reach, the best any path with it can do.
fn reach_tips(s: &Surface, c: &Cutter, lo: Vec2, hi: Vec2, spacing: f64) -> Vec<Vec3> {
    let (nx, ny) = (((hi.x - lo.x) / spacing).ceil() as usize, ((hi.y - lo.y) / spacing).ceil() as usize);
    let mut out = Vec::new();
    for j in 0..=ny {
        for i in 0..=nx {
            let (x, y) = (lo.x + spacing * i as f64, lo.y + spacing * j as f64);
            if let Some(z) = drop_z(s, c, x, y) {
                out.push(Vec3::new(x, y, z));
            }
        }
    }
    out
}

/// What a path leaves above the best the tool can do, square to the surface, at the points
/// `pick` keeps: (largest, 95th percentile, mean, share at most `limit`).
#[derive(Debug)]
#[allow(dead_code)] // `mean` and `worst` are read in the reports, through `Debug`.
struct Left {
    max: f64,
    p95: f64,
    mean: f64,
    within: f64,
    points: usize,
    /// Where the most is left.
    worst: Vec2,
}

fn left(s: &Surface, tool: &Tool, paths: &[&Toolpath], ev: &Eval, pick: &dyn Fn(Vec2) -> bool, limit: f64) -> Left {
    let c = tool.cutter().unwrap();
    let hi = ev.point(ev.n * ev.n - 1);
    let grow = Vec2::new(c.radius + 1.0, c.radius + 1.0);
    let best = ev.envelope(&c, &reach_tips(s, &c, ev.lo - grow, hi + grow, 0.05));
    let tips: Vec<Vec3> = paths.iter().flat_map(|p| path_tips(p, 0.05)).collect();
    let got = ev.envelope(&c, &tips);
    let mut vals = Vec::new();
    let mut worst = (0.0, Vec2::new(0.0, 0.0));
    for k in 0..ev.n * ev.n {
        let p = ev.point(k);
        if !pick(p) || !best[k].is_finite() {
            continue;
        }
        // Square to the surface under the point.
        let nz = s.raycast(p.extend(1000.0), Vec3::new(0.0, 0.0, -1.0)).and_then(|(_, i)| s.facet_normal(i)).map_or(1.0, |n| n.z.abs());
        let v = ((got[k] - best[k]) * nz).max(0.0);
        if v > worst.0 {
            worst = (v, p);
        }
        vals.push(v);
    }
    if std::env::var("FINISH3D_MAP").is_ok() {
        let mut out = String::new();
        for j in (0..ev.n).rev().step_by(3) {
            for i in (0..ev.n).step_by(3) {
                let k = j * ev.n + i;
                let p = ev.point(k);
                let v = if pick(p) && best[k].is_finite() { (got[k] - best[k]).max(0.0) } else { -1.0 };
                out.push(if v < 0.0 {
                    ' '
                } else if v <= limit {
                    '.'
                } else if v <= 2.0 * limit {
                    'o'
                } else if v <= 10.0 * limit {
                    'O'
                } else {
                    '#'
                });
            }
            out.push('\n');
        }
        eprintln!("{out}");
    }
    vals.sort_by(f64::total_cmp);
    let n = vals.len().max(1);
    Left {
        max: vals.last().copied().unwrap_or(0.0),
        p95: vals.get(n * 95 / 100).copied().unwrap_or(0.0),
        mean: vals.iter().sum::<f64>() / n as f64,
        within: vals.iter().filter(|v| **v <= limit).count() as f64 / n as f64,
        points: vals.len(),
        worst: worst.1,
    }
}

#[test]
fn scallop_height_holds_across_a_dome() {
    let s = dome();
    let tool = ball(6.0);
    let target = 0.01;
    let h = heights(8.0);
    let p = scallop_params(h, rect(-14.0, -14.0, 14.0, 14.0), Spacing::ScallopHeight(target));
    let path = scallop(&s, &tool, &p).unwrap();
    verify(&path, &h).unwrap();
    assert_safe(&s, &tool, &path, p.tolerance, p.feeds);
    // The same parameters give the same path.
    assert_eq!(scallop(&s, &tool, &p).unwrap(), path);
    // With the plunge feed as the descent feed, no move down the surface at least as steep as
    // the descent limit goes down faster than it (the check's bound is its `cut`).
    let slow = ScallopParams { descent_feed: Some(p.feeds.plunge), ..p.clone() };
    assert_safe(&s, &tool, &scallop(&s, &tool, &slow).unwrap(), p.tolerance, Feeds { cut: p.feeds.plunge, ..p.feeds });
    let ev = Eval::new(Vec2::new(-12.0, -12.0), Vec2::new(12.0, 12.0), 0.1);
    // The dome, away from its foot (a crease no path of this tool reaches into).
    let on_dome = |q: Vec2| q.x.hypot(q.y) < 9.5;
    let got = left(&s, &tool, &[&path], &ev, &on_dome, target * 1.25);
    // A raster at the stepover the scallop height gives on a plane, for comparison.
    let d = 2.0 * (2.0 * 3.0 * target - target * target).sqrt();
    let par = ParallelParams {
        heights: h,
        feeds: feeds(),
        descent_feed: None,
        stepover: d,
        angle: 0.0,
        boundary: rect(-14.0, -14.0, 14.0, 14.0),
        stock_to_leave: 0.0,
        step: 0.25,
        tolerance: 0.005,
        zigzag: true,
        descent_limit: DEFAULT_DESCENT_LIMIT,
    };
    let raster = parallel(&s, &tool, &par).unwrap();
    let base = left(&s, &tool, &[&raster], &ev, &on_dome, target * 1.25);
    let (len, rlen) = (path.stats().cut_length, raster.stats().cut_length);
    eprintln!("dome scallop {target}: {got:?}, {len:.0} mm; raster at {d:.3}: {base:?}, {rlen:.0} mm");
    assert!(got.points > 20_000);
    assert!(got.max <= target * 2.0, "{got:?}");
    assert!(got.p95 <= target * 1.1, "{got:?}");
    assert!(got.within >= 0.98, "{got:?}");
    assert!(base.points == got.points);
}

#[test]
fn scallop_covers_a_bowl_with_a_concave_fillet() {
    let s = bowl();
    let tool = ball(6.0);
    let target = 0.01;
    let h = heights(10.0);
    let p = scallop_params(h, rect(-18.0, -18.0, 18.0, 18.0), Spacing::ScallopHeight(target));
    let path = scallop(&s, &tool, &p).unwrap();
    verify(&path, &h).unwrap();
    assert_safe(&s, &tool, &path, p.tolerance, p.feeds);
    let ev = Eval::new(Vec2::new(-17.0, -17.0), Vec2::new(17.0, 17.0), 0.1);
    let got = left(&s, &tool, &[&path], &ev, &|q| q.x.hypot(q.y) < 16.0, target * 1.25);
    eprintln!("bowl scallop {target}: {got:?}, {:.0} mm", path.stats().cut_length);
    assert!(got.p95 <= target * 1.3, "{got:?}");
    assert!(got.within >= 0.93, "{got:?}");
    // Outward: the same passes, the middle first.
    let mut out = p.clone();
    out.direction = OffsetDirection::Outward;
    let back = scallop(&s, &tool, &out).unwrap();
    verify(&back, &h).unwrap();
    let first = feed_moves(&back)[0].1;
    let last = feed_moves(&path)[0].1;
    assert!(first.x.hypot(first.y) < last.x.hypot(last.y), "{first:?} {last:?}");
}

#[test]
fn scallop_and_steep_shallow_on_a_stepped_part() {
    let s = stepped();
    let tool = ball(6.0);
    let target = 0.01;
    let h = heights(8.0);
    let b = rect(-15.0, -8.0, 15.0, 8.0);
    let path = scallop(&s, &tool, &scallop_params(h, b.clone(), Spacing::ScallopHeight(target))).unwrap();
    verify(&path, &h).unwrap();
    assert_safe(&s, &tool, &path, 0.005, feeds());
    let ev = Eval::new(Vec2::new(-14.0, -7.0), Vec2::new(14.0, 7.0), 0.1);
    // The treads, away from the boundary's ends; not the risers' own line, a wall seen edge on.
    let treads = |q: Vec2| q.y.abs() < 6.0 && q.x.abs() < 13.0 && (q.x.abs() - 5.0).abs() > 0.05;
    let flat_only = left(&s, &tool, &[&path], &ev, &treads, target * 1.25);
    let sp = SteepShallowParams {
        heights: h,
        feeds: feeds(),
        descent_feed: None,
        spacing: Spacing::ScallopHeight(target),
        threshold: 45f64.to_radians(),
        overlap: 0.5,
        stepdown: None,
        shallow: ShallowPattern::Scallop,
        boundary: b,
        stock_to_leave: 0.0,
        step: 0.25,
        tolerance: 0.005,
        climb: true,
    };
    let both = steep_shallow(&s, &tool, &sp).unwrap();
    verify(&both, &h).unwrap();
    assert_safe(&s, &tool, &both, 0.005, feeds());
    let mixed = left(&s, &tool, &[&both], &ev, &treads, target * 1.25);
    eprintln!("stepped: scallop {flat_only:?} {:.0} mm; steep/shallow {mixed:?} {:.0} mm", path.stats().cut_length, both.stats().cut_length);
    for got in [&flat_only, &mixed] {
        assert!(got.p95 <= target * 1.1 && got.within >= 0.99, "{got:?}");
        assert!(got.max <= target * 3.0, "{got:?}");
    }
    // The risers are steep: the z-levels run along them at constant heights between the treads.
    let levels: Vec<f64> = feed_moves(&both)
        .iter()
        .filter(|(a, b)| (a.z - b.z).abs() < 1e-9 && (a.y - b.y).abs() > 1e-3 && (a.x - b.x).abs() < 1e-6 && a.z > 0.5 && a.z < 7.5)
        .map(|(a, _)| a.z)
        .collect();
    assert!(levels.len() > 10, "{levels:?}");
}

#[test]
fn steep_shallow_splits_a_bowl_by_slope() {
    let s = bowl();
    let tool = ball(6.0);
    let target = 0.01;
    let h = heights(10.0);
    let mut sp = SteepShallowParams {
        heights: h,
        feeds: feeds(),
        descent_feed: None,
        spacing: Spacing::ScallopHeight(target),
        threshold: 40f64.to_radians(),
        overlap: 0.5,
        stepdown: None,
        shallow: ShallowPattern::Scallop,
        boundary: rect(-18.0, -18.0, 18.0, 18.0),
        stock_to_leave: 0.0,
        step: 0.25,
        tolerance: 0.005,
        climb: true,
    };
    let ev = Eval::new(Vec2::new(-17.0, -17.0), Vec2::new(17.0, 17.0), 0.1);
    let pick = |q: Vec2| q.x.hypot(q.y) < 16.0;
    for pattern in [ShallowPattern::Scallop, ShallowPattern::Parallel { angle: 0.3 }] {
        sp.shallow = pattern;
        let side = crate::finish3d::DRAPED_SIDE_BY_SIDE.with(std::cell::Cell::get);
        let path = steep_shallow(&s, &tool, &sp).unwrap();
        // The passes are draped side by side, and give the path draped one after the other.
        assert!(crate::finish3d::DRAPED_SIDE_BY_SIDE.with(std::cell::Cell::get) > side, "{pattern:?}");
        assert_eq!(path, crate::par::serially(|| steep_shallow(&s, &tool, &sp)).unwrap(), "{pattern:?}");
        verify(&path, &h).unwrap();
        assert_safe(&s, &tool, &path, sp.tolerance, sp.feeds);
        let got = left(&s, &tool, &[&path], &ev, &pick, target * 1.25);
        eprintln!("bowl steep/shallow {pattern:?}: {got:?}, {:.0} mm", path.stats().cut_length);
        assert!(got.p95 <= target * 1.4, "{pattern:?} {got:?}");
        // Level moves on the wall (its steep part is from radius 8.5 out to 14).
        let on_wall = feed_moves(&path)
            .iter()
            .filter(|(a, b)| (a.z - b.z).abs() < 1e-9 && a.z > 2.0 && a.z < 9.0 && a.dist(*b) > 1e-3 && a.x.hypot(a.y) > 8.0)
            .count();
        assert!(on_wall > 100, "{on_wall}");
    }
}

#[test]
fn steep_shallow_drapes_each_point_of_a_pass_once() {
    let s = bowl();
    let tool = ball(6.0);
    let sp = SteepShallowParams {
        heights: heights(10.0),
        feeds: feeds(),
        descent_feed: None,
        spacing: Spacing::ScallopHeight(0.01),
        threshold: 40f64.to_radians(),
        overlap: 0.5,
        stepdown: None,
        shallow: ShallowPattern::Scallop,
        boundary: rect(-18.0, -18.0, 18.0, 18.0),
        stock_to_leave: 0.0,
        step: 0.25,
        tolerance: 0.005,
        climb: true,
    };
    let count = || (crate::finish3d::DRAPE_DROPS.with(std::cell::Cell::get), crate::finish3d::DRAPE_REDROPS.with(std::cell::Cell::get));
    let before = count();
    crate::par::serially(|| steep_shallow(&s, &tool, &sp)).unwrap();
    let after = count();
    // A pass's stretches share their ends: each end is sampled once, not by both stretches.
    assert!(after.0 > before.0 + 1000, "{before:?} {after:?}");
    assert_eq!(after.1, before.1, "{before:?} {after:?}");
}

#[test]
fn pencil_finds_the_crease_of_a_fillet_joint() {
    let tool = ball(6.0);
    let h = heights(6.0);
    let mut p = PencilParams {
        heights: h,
        feeds: feeds(),
        descent_feed: None,
        min_angle: 30f64.to_radians(),
        passes: 0,
        stepover: 0.5,
        min_length: 3.0,
        boundary: rect(-10.0, -8.0, 8.0, 8.0),
        stock_to_leave: 0.0,
        step: 0.2,
        tolerance: 0.005,
        climb: true,
    };
    // A fillet tighter than the ball: the ball touches the floor and the wall at once, its tip
    // at X -3 on the floor.
    let s = fillet_joint(1.0);
    let path = pencil(&s, &tool, &p).unwrap();
    verify(&path, &h).unwrap();
    assert_safe(&s, &tool, &path, p.tolerance, p.feeds);
    let cuts = feed_moves(&path);
    assert!(!cuts.is_empty());
    let mut ys: Vec<f64> = Vec::new();
    for (_, b) in &cuts {
        assert!((b.x + 3.0).abs() < 0.02 && b.z.abs() < 0.02, "{b:?}");
        ys.push(b.y);
    }
    let (y0, y1) = (ys.iter().copied().fold(f64::INFINITY, f64::min), ys.iter().copied().fold(f64::NEG_INFINITY, f64::max));
    assert!(y0 < -7.5 && y1 > 7.5, "{y0} {y1}");
    // A sharp corner is a crease at the same place.
    let sharp = pencil(&fillet_joint(0.0), &tool, &p).unwrap();
    assert!(feed_moves(&sharp).iter().all(|(_, b)| (b.x + 3.0).abs() < 0.02), "{sharp:?}");
    // Offset passes run beside it on the floor, a stepover apart.
    p.passes = 2;
    let wide = pencil(&s, &tool, &p).unwrap();
    verify(&wide, &h).unwrap();
    assert_safe(&s, &tool, &wide, p.tolerance, p.feeds);
    let xs: Vec<f64> = feed_moves(&wide).iter().filter(|(_, b)| b.y.abs() < 5.0 && b.z.abs() < 0.02).map(|(_, b)| b.x).collect();
    for want in [-3.5, -4.0] {
        assert!(xs.iter().any(|x| (x - want).abs() < 0.08), "no pass at {want}: {xs:?}");
    }
    // A plane has no crease, nor has a fillet far wider than the ball.
    assert!(pencil(&plane(20.0, 0.0), &tool, &p).unwrap().moves.is_empty());
    p.passes = 0;
    let gentle = pencil(&fillet_joint(5.0), &tool, &p).unwrap();
    assert!(gentle.moves.is_empty(), "{} moves: {:?}", gentle.moves.len(), &gentle.moves[..gentle.moves.len().min(12)]);
}

#[test]
fn rest_finishing_cuts_only_the_corner() {
    let s = fillet_joint(0.0);
    let (big, small) = (ball(10.0), ball(4.0));
    let h = heights(6.0);
    let b = rect(-15.0, -8.0, 8.0, 8.0);
    let p = RestParams {
        heights: h,
        feeds: feeds(),
        descent_feed: None,
        previous: big.clone(),
        threshold: 0.01,
        spacing: Spacing::ScallopHeight(0.01),
        overlap: 0.5,
        boundary: b.clone(),
        stock_to_leave: 0.0,
        step: 0.2,
        tolerance: 0.005,
        climb: true,
    };
    let path = rest_finish(&s, &small, &p).unwrap();
    verify(&path, &h).unwrap();
    assert_safe(&s, &small, &path, p.tolerance, p.feeds);
    let cuts = feed_moves(&path);
    assert!(cuts.len() > 50);
    // Only in the corner the big ball could not reach: its tip stopped 5 mm short of the wall,
    // and nothing is left on the top, nor on the floor further out.
    for (_, q) in &cuts {
        assert!(q.x > -5.0 - 0.5 - 0.3 && q.x < 0.3, "{q:?}");
        assert!(q.z < 6.0 - 1e-6 || q.x < 0.0, "{q:?}");
        // The overlap runs on over the floor, not up the wall onto its top edge: closer to the
        // wall than its own radius the small ball can only ride that edge, where the big one
        // left nothing.
        assert!(q.x < -2.0 + 0.05, "{q:?} rides the wall's top edge");
    }
    let xs =
        cuts.iter().filter(|(_, q)| q.z.abs() < 0.01).map(|(_, q)| q.x).fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(x), b.max(x)));
    assert!((xs.1 + 2.0).abs() < 0.1, "the small ball's corner position is cut: {xs:?}");
    // What is left in the corner after both tools is the small tool's own best.
    let first = parallel(
        &s,
        &big,
        &ParallelParams {
            heights: h,
            feeds: feeds(),
            descent_feed: None,
            stepover: 0.5,
            angle: PI / 2.0,
            boundary: b.clone(),
            stock_to_leave: 0.0,
            step: 0.25,
            tolerance: 0.005,
            zigzag: true,
            descent_limit: DEFAULT_DESCENT_LIMIT,
        },
    )
    .unwrap();
    let ev = Eval::new(Vec2::new(-7.0, -6.0), Vec2::new(0.0, 6.0), 0.05);
    let corner = |q: Vec2| q.x > -6.0 && q.x < -0.1 && q.y.abs() < 5.0;
    let before = left(&s, &small, &[&first], &ev, &corner, 0.02);
    let after = left(&s, &small, &[&first, &path], &ev, &corner, 0.02);
    eprintln!("rest corner: big tool alone {before:?}; with rest {after:?}, {:.0} mm", path.stats().cut_length);
    assert!(before.max > 1.0, "{before:?}");
    assert!(after.max < 0.05 && after.within > 0.95, "{after:?}");
    // A flat floor leaves nothing for it.
    assert!(rest_finish(&plane(20.0, 0.0), &small, &p).unwrap().moves.is_empty());
}

#[test]
fn hostile_finishing_params_are_refused() {
    let s = dome();
    let tool = ball(6.0);
    let h = heights(8.0);
    let good = scallop_params(h, rect(-10.0, -10.0, 10.0, 10.0), Spacing::Stepover(1.0));
    scallop(&s, &tool, &good).unwrap();
    let bad: Vec<Box<dyn Fn(&mut ScallopParams)>> = vec![
        Box::new(|p| p.spacing = Spacing::Stepover(f64::NAN)),
        Box::new(|p| p.spacing = Spacing::Stepover(0.0)),
        Box::new(|p| p.spacing = Spacing::Stepover(7.0)),
        Box::new(|p| p.spacing = Spacing::ScallopHeight(-0.1)),
        Box::new(|p| p.spacing = Spacing::ScallopHeight(f64::INFINITY)),
        Box::new(|p| p.spacing = Spacing::ScallopHeight(3.5)),
        Box::new(|p| p.tolerance = 0.0),
        Box::new(|p| p.step = f64::NAN),
        Box::new(|p| p.step = -1.0),
        Box::new(|p| p.stock_to_leave = -1.0),
        Box::new(|p| p.feeds.plunge = 0.0),
        Box::new(|p| p.heights.retract = 1.0),
        Box::new(|p| p.boundary.rect = Some([Vec2::new(0.0, 0.0), Vec2::new(0.0, 5.0)])),
        Box::new(|p| p.boundary.rect = Some([Vec2::new(f64::NAN, 0.0), Vec2::new(1.0, 5.0)])),
    ];
    for (i, f) in bad.iter().enumerate() {
        let mut p = good.clone();
        f(&mut p);
        assert!(matches!(scallop(&s, &tool, &p), Err(CamError::Invalid(_))), "case {i}");
    }
    // A flat end mill leaves no scallop to size by; a drill cannot mill.
    let mut p = good.clone();
    p.spacing = Spacing::ScallopHeight(0.01);
    assert!(scallop(&s, &flat(6.0), &p).is_err());
    assert!(scallop(&s, &Tool::new(4, ToolKind::Drill, 6.0, 30.0), &good).is_err());
    // Sizes that would run for hours are refused before the work.
    let mut huge = good.clone();
    huge.boundary = rect(-1e5, -1e5, 1e5, 1e5);
    huge.heights = Heights { clearance: 40.0, retract: 30.0, top: 8.0, bottom: -1.0 };
    assert!(matches!(scallop(&s, &tool, &huge), Err(CamError::TooLarge(_))));
    let mut deep = good.clone();
    deep.step = 1e-4;
    assert!(matches!(scallop(&s, &tool, &deep), Err(CamError::TooLarge(_))));
    // An empty surface: nothing to cut in a given rectangle.
    let empty = Surface::new(vec![]).unwrap();
    assert!(scallop(&empty, &tool, &good).unwrap().moves.is_empty());
    assert!(scallop(&empty, &tool, &scallop_params(h, Boundary::default(), Spacing::Stepover(1.0))).is_err());

    let sp = SteepShallowParams {
        heights: h,
        feeds: feeds(),
        descent_feed: None,
        spacing: Spacing::Stepover(1.0),
        threshold: 0.7,
        overlap: 0.5,
        stepdown: None,
        shallow: ShallowPattern::Parallel { angle: 0.0 },
        boundary: rect(-10.0, -10.0, 10.0, 10.0),
        stock_to_leave: 0.0,
        step: 0.5,
        tolerance: 0.01,
        climb: false,
    };
    steep_shallow(&s, &tool, &sp).unwrap();
    let bad: Vec<Box<dyn Fn(&mut SteepShallowParams)>> = vec![
        Box::new(|p| p.threshold = 0.0),
        Box::new(|p| p.threshold = PI / 2.0),
        Box::new(|p| p.threshold = f64::NAN),
        Box::new(|p| p.overlap = -1.0),
        Box::new(|p| p.overlap = f64::NAN),
        Box::new(|p| p.stepdown = Some(0.0)),
        Box::new(|p| p.stepdown = Some(f64::NAN)),
        Box::new(|p| p.shallow = ShallowPattern::Parallel { angle: f64::INFINITY }),
    ];
    for (i, f) in bad.iter().enumerate() {
        let mut p = sp.clone();
        f(&mut p);
        assert!(matches!(steep_shallow(&s, &tool, &p), Err(CamError::Invalid(_))), "case {i}");
    }
    let mut many = sp.clone();
    many.stepdown = Some(1e-4);
    assert!(matches!(steep_shallow(&s, &tool, &many), Err(CamError::TooLarge(_))));

    let pp = PencilParams {
        heights: h,
        feeds: feeds(),
        descent_feed: None,
        min_angle: 0.3,
        passes: 1,
        stepover: 0.5,
        min_length: 1.0,
        boundary: rect(-10.0, -10.0, 10.0, 10.0),
        stock_to_leave: 0.0,
        step: 0.25,
        tolerance: 0.01,
        climb: true,
    };
    pencil(&s, &tool, &pp).unwrap();
    let bad: Vec<Box<dyn Fn(&mut PencilParams)>> = vec![
        Box::new(|p| p.min_angle = 0.0),
        Box::new(|p| p.min_angle = 4.0),
        Box::new(|p| p.min_angle = f64::NAN),
        Box::new(|p| p.passes = 1000),
        Box::new(|p| p.stepover = 0.0),
        Box::new(|p| p.stepover = 100.0),
        Box::new(|p| p.min_length = -1.0),
        Box::new(|p| p.min_length = f64::NAN),
    ];
    for (i, f) in bad.iter().enumerate() {
        let mut p = pp.clone();
        f(&mut p);
        assert!(matches!(pencil(&s, &tool, &p), Err(CamError::Invalid(_))), "case {i}");
    }

    let rp = RestParams {
        heights: h,
        feeds: feeds(),
        descent_feed: None,
        previous: ball(10.0),
        threshold: 0.01,
        spacing: Spacing::Stepover(0.5),
        overlap: 0.5,
        boundary: rect(-10.0, -10.0, 10.0, 10.0),
        stock_to_leave: 0.0,
        step: 0.25,
        tolerance: 0.01,
        climb: true,
    };
    rest_finish(&s, &tool, &rp).unwrap();
    let bad: Vec<Box<dyn Fn(&mut RestParams)>> = vec![
        Box::new(|p| p.previous = ball(6.0)),
        Box::new(|p| p.previous = ball(f64::NAN)),
        Box::new(|p| p.previous = Tool::new(4, ToolKind::Drill, 10.0, 30.0)),
        Box::new(|p| p.threshold = 0.0),
        Box::new(|p| p.threshold = f64::NAN),
        Box::new(|p| p.overlap = -0.5),
        Box::new(|p| p.overlap = f64::INFINITY),
    ];
    for (i, f) in bad.iter().enumerate() {
        let mut p = rp.clone();
        f(&mut p);
        assert!(matches!(rest_finish(&s, &tool, &p), Err(CamError::Invalid(_))), "case {i}");
    }
}

/// The turn of the feed moves about the Z axis through `c`, at radii from `r0` to `r1`: the
/// share of their length going counter-clockwise.
fn counter_clockwise_share(path: &Toolpath, c: Vec2, r0: f64, r1: f64) -> f64 {
    let (mut ccw, mut all) = (0.0, 0.0);
    for (a, b) in feed_moves(path) {
        let (pa, pb) = (Vec2::new(a.x, a.y) - c, Vec2::new(b.x, b.y) - c);
        let r = pa.x.hypot(pa.y);
        let len = pa.dist(pb);
        if r < r0 || r > r1 || len < 1e-6 {
            continue;
        }
        all += len;
        if pa.x * pb.y - pa.y * pb.x > 0.0 {
            ccw += len;
        }
    }
    assert!(all > 10.0, "{all} mm of moves between radius {r0} and {r1}");
    ccw / all
}

#[test]
fn offset_passes_climb_whichever_way_they_run() {
    // Climb milling keeps what is still to cut on the right of the travel (the spindle turning
    // clockwise seen from above). Inward, what is left is the middle: the loops round the dome
    // go clockwise. Outward, it is the outside: they go counter-clockwise.
    let s = dome();
    let tool = ball(6.0);
    let h = heights(8.0);
    let mut p = scallop_params(h, rect(-14.0, -14.0, 14.0, 14.0), Spacing::Stepover(1.0));
    p.step = 0.5;
    for (direction, climb, ccw) in [
        (OffsetDirection::Inward, true, false),
        (OffsetDirection::Inward, false, true),
        (OffsetDirection::Outward, true, true),
        (OffsetDirection::Outward, false, false),
    ] {
        p.direction = direction;
        p.climb = climb;
        let path = scallop(&s, &tool, &p).unwrap();
        verify(&path, &h).unwrap();
        let share = counter_clockwise_share(&path, Vec2::new(0.0, 0.0), 2.0, 11.0);
        let want = if ccw { share } else { 1.0 - share };
        assert!(want > 0.9, "{direction:?} climb {climb}: {share} of the moves counter-clockwise");
    }
}

#[test]
fn pencil_offset_passes_climb_towards_the_crease() {
    // The offset passes on the floor beside the crease at X -3 leave what is still to cut on
    // their +X side, towards the crease: climb milling runs them along +Y, conventional along
    // -Y.
    let s = fillet_joint(1.0);
    let tool = ball(6.0);
    let h = heights(6.0);
    for climb in [true, false] {
        let p = PencilParams {
            heights: h,
            feeds: feeds(),
            descent_feed: None,
            min_angle: 30f64.to_radians(),
            passes: 2,
            stepover: 0.5,
            min_length: 3.0,
            boundary: rect(-10.0, -8.0, 8.0, 8.0),
            stock_to_leave: 0.0,
            step: 0.2,
            tolerance: 0.005,
            climb,
        };
        let path = pencil(&s, &tool, &p).unwrap();
        verify(&path, &h).unwrap();
        let along: Vec<f64> = feed_moves(&path)
            .iter()
            .filter(|(a, b)| b.z.abs() < 0.02 && [-3.5, -4.0].iter().any(|x| (b.x - x).abs() < 0.1) && (b.y - a.y).abs() > 1e-3)
            .map(|(a, b)| b.y - a.y)
            .collect();
        assert!(along.len() > 20, "{along:?}");
        let forward = along.iter().filter(|dy| **dy > 0.0).count() as f64 / along.len() as f64;
        assert!(if climb { forward > 0.95 } else { forward < 0.05 }, "climb {climb}: {forward} of the offset moves along +Y");
    }
}

#[test]
fn shallow_rasters_too_dense_to_cut_are_refused_before_they_are_built() {
    // A stepover of half a micron over a 20 mm square makes over 50 000 passes of 200 points
    // each: refused while planning, not after building ten million points.
    let s = dome();
    let tool = ball(6.0);
    let sp = SteepShallowParams {
        heights: heights(8.0),
        feeds: feeds(),
        descent_feed: None,
        spacing: Spacing::Stepover(5e-4),
        threshold: 0.7,
        overlap: 0.5,
        stepdown: Some(1.0),
        shallow: ShallowPattern::Parallel { angle: 0.0 },
        boundary: rect(-10.0, -10.0, 10.0, 10.0),
        stock_to_leave: 0.0,
        step: 0.1,
        tolerance: 0.01,
        climb: true,
    };
    match steep_shallow(&s, &tool, &sp) {
        Err(CamError::TooLarge(why)) => assert!(why.contains("shallow passes"), "{why}"),
        other => panic!("{:?}", other.map(|p| p.moves.len())),
    }
}

/// Scallop rings 2 mm apart over a dome: a straight link from one ring to the next cuts across
/// the dome's curve (a 2 mm chord on a 12 mm sphere sags 0.04 mm, far past the tolerance), so
/// the links follow the dome (raised where the cutter resting on it is higher), as a parallel
/// finish's do, instead of lifting to the retract height and plunging again between rings.
#[test]
fn scallop_links_follow_the_surface_between_rings() {
    let s = dome();
    let tool = ball(6.0);
    let h = heights(8.0);
    let p = scallop_params(h, rect(-14.0, -14.0, 14.0, 14.0), Spacing::Stepover(2.0));
    let path = scallop(&s, &tool, &p).unwrap();
    verify(&path, &h).unwrap();
    assert_safe(&s, &tool, &path, p.tolerance, p.feeds);
    // Lifts: rapids up to the retract height or above.
    let mut pos: Option<Vec3> = None;
    let mut lifts = 0;
    for m in &path.moves {
        if let (Some(a), Move::Rapid { to }) = (pos, *m)
            && to.z > a.z + 1e-9
        {
            lifts += 1;
        }
        pos = m.target().or(pos);
    }
    // Links that rise over the dome: a feed move up and on to the next ring.
    let rising = feed_moves(&path).iter().filter(|(a, b)| b.z > a.z + 0.01 && (b.x - a.x).hypot(b.y - a.y) > 1e-6).count();
    assert!(rising > 0, "no link follows the dome");
    // Straight links, taken only where they stay out of the dome, lift 16 times here.
    assert!(lifts <= 10, "{lifts} lifts between rings");
}
