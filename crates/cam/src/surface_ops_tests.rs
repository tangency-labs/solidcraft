//! Tests of the 3-axis surface operations, against closed forms wherever there is one.

use std::f64::consts::PI;

use solidcore_record::{Loop2, Region2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::foundation_tests::*;
use crate::*;

fn ball(d: f64) -> Tool {
    Tool::new(1, ToolKind::BallEndmill, d, 20.0)
}

fn flat(d: f64) -> Tool {
    Tool::new(2, ToolKind::FlatEndmill, d, 20.0)
}

fn bull(d: f64, rc: f64) -> Tool {
    let mut t = Tool::new(3, ToolKind::BullNose, d, 20.0);
    t.corner_radius = Some(rc);
    t
}

fn feeds() -> Feeds {
    Feeds { cut: 800.0, plunge: 200.0 }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Boundary {
    Boundary { rect: Some([Vec2::new(x0, y0), Vec2::new(x1, y1)]), region: None }
}

fn par(heights: Heights, boundary: Boundary) -> ParallelParams {
    ParallelParams {
        heights,
        feeds: feeds(),
        stepover: 1.5,
        angle: 0.0,
        boundary,
        stock_to_leave: 0.0,
        step: 1.0,
        tolerance: 0.01,
        zigzag: true,
        descent_limit: DEFAULT_DESCENT_LIMIT,
        descent_feed: None,
    }
}

/// The targets of every move at feed.
fn cuts(path: &Toolpath) -> Vec<Vec3> {
    path.moves.iter().filter_map(|m| if let Move::Feed { to, .. } = *m { Some(to) } else { None }).collect()
}

/// Every move at feed, from where the tool was.
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

/// Where the cutter rests, stock to leave included.
fn rest(s: &Surface, c: &Cutter, leave: f64, p: Vec3) -> Option<f64> {
    drop_z(s, &c.grown(leave).unwrap(), p.x, p.y).map(|z| z + leave)
}

#[test]
fn parallel_over_a_sphere_follows_the_ball_on_sphere_height() {
    let (rs, r) = (20.0, 3.0);
    let s = sphere(Vec3::new(0.0, 0.0, 0.0), rs, 64);
    let chord = rs * (1.0 - (PI / 64.0).cos()) * 2.0;
    let h = Heights { clearance: 40.0, retract: 25.0, top: 20.0, bottom: -50.0 };
    let mut p = par(h, rect(-15.0, -15.0, 15.0, 15.0));
    p.angle = 0.3;
    let tool = ball(2.0 * r);
    let path = parallel(&s, &tool, &p).unwrap();
    verify(&path, &h).unwrap();
    let pts = cuts(&path);
    assert!(pts.len() > 500);
    for q in &pts {
        let rho = q.x.hypot(q.y);
        let want = ((rs + r) * (rs + r) - rho * rho).sqrt() - r;
        assert!(q.z <= want + 1e-9 && q.z >= want - chord, "{q:?}: {} vs {want}", q.z);
    }
    // Between its points the path is never more than the tolerance into the surface.
    let c = tool.cutter().unwrap();
    for (a, b) in feed_moves(&path) {
        let m = a.lerp(b, 0.5);
        if let Some(z) = rest(&s, &c, 0.0, m) {
            assert!(m.z >= z - p.tolerance - 1e-9, "{a:?} → {b:?} cuts into the surface");
        }
    }
    // The same parameters give the same path.
    assert_eq!(parallel(&s, &tool, &p).unwrap(), path);
}

#[test]
fn parallel_over_a_slope_matches_the_closed_forms() {
    let th: f64 = 0.5;
    let s = slope(100.0, th);
    let h = Heights { clearance: 40.0, retract: 30.0, top: 20.0, bottom: -50.0 };
    let p = par(h, rect(-20.0, -10.0, 20.0, 10.0));
    let (r, rc, a) = (4.0, 1.0, 3.0);
    let flat_path = parallel(&s, &flat(2.0 * r), &p).unwrap();
    verify(&flat_path, &h).unwrap();
    for q in cuts(&flat_path) {
        close(q.z, (q.x + r) * th.tan(), 1e-7);
    }
    let bull_path = parallel(&s, &bull(2.0 * r, rc), &p).unwrap();
    verify(&bull_path, &h).unwrap();
    for q in cuts(&bull_path) {
        close(q.z, (rc + th.sin() * (q.x + a)) / th.cos() - rc, 1e-7);
    }
    // A plane needs no refinement: a pass is its base samples.
    let n = cuts(&flat_path).len();
    assert!(n < 2 * 41 * 15, "{n} points");
}

#[test]
fn parallel_passes_are_a_stepover_apart() {
    let s = plane(50.0, 0.0);
    let h = Heights { clearance: 10.0, retract: 5.0, top: 0.0, bottom: -1.0 };
    let mut p = par(h, rect(0.0, 0.0, 10.0, 7.0));
    p.stepover = 2.0;
    let path = parallel(&s, &flat(6.0), &p).unwrap();
    verify(&path, &h).unwrap();
    let mut ys: Vec<f64> = cuts(&path).iter().map(|q| q.y).collect();
    ys.sort_by(f64::total_cmp);
    ys.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    assert_eq!(ys.len(), 5, "{ys:?}");
    for w in ys.windows(2) {
        close(w[1] - w[0], 1.75, 1e-9);
    }
    close(ys[0], 0.0, 1e-9);
    close(ys[4], 7.0, 1e-9);
    // Zigzag: the passes go back and forth, linked at feed along the edge; one way: all
    // the same way, linked over the top.
    let fwd = |path: &Toolpath| feed_moves(path).iter().filter(|(a, b)| (a.y - b.y).abs() < 1e-9 && b.x > a.x + 1e-9).count();
    let back = |path: &Toolpath| feed_moves(path).iter().filter(|(a, b)| (a.y - b.y).abs() < 1e-9 && b.x < a.x - 1e-9).count();
    assert!(fwd(&path) > 0 && back(&path) > 0);
    assert_eq!(path.moves.iter().filter(|m| matches!(m, Move::Rapid { .. })).count(), 3, "zigzag stays down on a plane");
    p.zigzag = false;
    let one_way = parallel(&s, &flat(6.0), &p).unwrap();
    verify(&one_way, &h).unwrap();
    assert_eq!(back(&one_way), 0);
    // Across the passes, at 90°.
    p.angle = PI / 2.0;
    let across = parallel(&s, &flat(6.0), &p).unwrap();
    let mut xs: Vec<f64> = cuts(&across).iter().map(|q| q.x).collect();
    xs.sort_by(f64::total_cmp);
    xs.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    assert_eq!(xs.len(), 6, "{xs:?}");
}

#[test]
fn stock_to_leave_raises_the_path() {
    let s = plane(50.0, 3.0);
    let h = Heights { clearance: 20.0, retract: 10.0, top: 3.0, bottom: -1.0 };
    let mut p = par(h, rect(-20.0, -20.0, 20.0, 20.0));
    p.stock_to_leave = 0.5;
    for tool in [ball(6.0), flat(6.0), bull(6.0, 1.0)] {
        let path = parallel(&s, &tool, &p).unwrap();
        verify(&path, &h).unwrap();
        for q in cuts(&path) {
            close(q.z, 3.5, 1e-9);
        }
    }
    // The floor holds the tool up where the surface is lower.
    p.stock_to_leave = 0.0;
    p.heights.bottom = 4.0;
    p.heights.top = 4.0;
    let floored = parallel(&s, &ball(6.0), &p).unwrap();
    assert!(cuts(&floored).iter().all(|q| (q.z - 4.0).abs() < 1e-9));
}

#[test]
fn a_region_confines_the_cutter_centre() {
    let s = sphere(Vec3::new(0.0, 0.0, 0.0), 20.0, 48);
    let h = Heights { clearance: 40.0, retract: 25.0, top: 20.0, bottom: -50.0 };
    let circle = Region2 { outer: Loop2::circle(Vec2::new(2.0, 1.0), 8.0), holes: vec![] };
    let p = par(h, Boundary { rect: None, region: Some(circle) });
    let path = parallel(&s, &ball(4.0), &p).unwrap();
    verify(&path, &h).unwrap();
    let pts = cuts(&path);
    assert!(!pts.is_empty());
    for q in pts {
        assert!(Vec2::new(q.x, q.y).dist(Vec2::new(2.0, 1.0)) <= 8.0 + 0.01, "{q:?}");
    }
}

#[test]
fn parallel_works_in_a_tilted_frame() {
    // A sphere seen from a tilted frame is still a sphere about the frame's image of its centre.
    let f = tilted(Vec3::new(5.0, -3.0, 2.0), 0.6, -0.3);
    let s = sphere(Vec3::new(5.0, -3.0, 2.0), 15.0, 48).in_frame(&f).unwrap();
    let h = Heights { clearance: 30.0, retract: 20.0, top: 15.0, bottom: -40.0 };
    let path = parallel(&s, &ball(6.0), &par(h, rect(-8.0, -8.0, 8.0, 8.0))).unwrap();
    verify(&path, &h).unwrap();
    let chord = 15.0 * (1.0 - (PI / 48.0).cos()) * 2.0;
    for q in cuts(&path) {
        let want = (18.0f64 * 18.0 - q.x * q.x - q.y * q.y).sqrt() - 3.0;
        assert!(q.z <= want + 1e-9 && q.z >= want - chord);
    }
}

#[test]
fn hostile_parallel_params_are_refused() {
    let s = plane(50.0, 0.0);
    let h = Heights { clearance: 10.0, retract: 5.0, top: 0.0, bottom: -1.0 };
    let good = par(h, rect(-10.0, -10.0, 10.0, 10.0));
    let tool = ball(6.0);
    parallel(&s, &tool, &good).unwrap();
    let bad: Vec<Box<dyn Fn(&mut ParallelParams)>> = vec![
        Box::new(|p| p.stepover = f64::NAN),
        Box::new(|p| p.stepover = 0.0),
        Box::new(|p| p.stepover = 7.0),
        Box::new(|p| p.tolerance = f64::NAN),
        Box::new(|p| p.tolerance = 0.0),
        Box::new(|p| p.step = -1.0),
        Box::new(|p| p.step = f64::INFINITY),
        Box::new(|p| p.angle = f64::NAN),
        Box::new(|p| p.stock_to_leave = -0.1),
        Box::new(|p| p.feeds.cut = 0.0),
        Box::new(|p| p.heights.retract = -5.0),
        Box::new(|p| p.heights.clearance = f64::NAN),
        Box::new(|p| p.boundary.rect = Some([Vec2::new(f64::NAN, 0.0), Vec2::new(1.0, 1.0)])),
        Box::new(|p| p.boundary.rect = Some([Vec2::new(5.0, 0.0), Vec2::new(1.0, 1.0)])),
    ];
    for (i, f) in bad.iter().enumerate() {
        let mut p = good.clone();
        f(&mut p);
        assert!(matches!(parallel(&s, &tool, &p), Err(CamError::Invalid(_))), "case {i}");
    }
    // A retract height below the top of the surface would rapid through it.
    let mut low = good.clone();
    low.heights = Heights { clearance: 10.0, retract: 5.0, top: 0.0, bottom: -1.0 };
    assert!(parallel(&plane(50.0, 6.0), &tool, &low).is_err());
    // Sizes that would run for hours are refused before any work.
    let mut huge = good.clone();
    huge.boundary = rect(-1e5, -1e5, 1e5, 1e5);
    huge.stepover = 0.01;
    assert!(matches!(parallel(&s, &tool, &huge), Err(CamError::TooLarge(_))));
    let mut fine = good.clone();
    fine.boundary = rect(-100.0, -100.0, 100.0, 100.0);
    fine.step = 1e-4;
    assert!(matches!(parallel(&s, &tool, &fine), Err(CamError::TooLarge(_))));
    assert!(parallel(&s, &Tool::new(4, ToolKind::Drill, 6.0, 30.0), &good).is_err());
    // An empty surface needs a rectangle, and then there is nothing to cut.
    let empty = Surface::new(vec![]).unwrap();
    assert!(parallel(&empty, &tool, &par(h, Boundary::default())).is_err());
    assert!(parallel(&empty, &tool, &good).unwrap().moves.is_empty());
}

fn rough(heights: Heights, boundary: Boundary) -> RoughingParams {
    RoughingParams {
        heights,
        feeds: feeds(),
        stepdown: 1.5,
        stepover: 2.5,
        angle: 0.0,
        boundary,
        stock_to_leave: 0.5,
        step: 1.0,
        tolerance: 0.01,
        zigzag: true,
    }
}

/// Checks no point of the path at feed is under where the cutter rests (and none of its moves
/// is more than `tol` into it).
fn never_below(path: &Toolpath, s: &Surface, c: &Cutter, leave: f64, tol: f64) {
    for (a, b) in feed_moves(path) {
        for (q, slack) in [(b, 1e-9), (a.lerp(b, 0.5), tol + 1e-9)] {
            if let Some(z) = rest(s, c, leave, q) {
                assert!(q.z >= z - slack, "{q:?} on {a:?} → {b:?} is below the part ({z})");
            }
        }
    }
}

#[test]
fn roughing_a_block_clears_the_stock_in_levels() {
    let s = block(Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 5.0));
    let h = Heights { clearance: 20.0, retract: 10.0, top: 8.0, bottom: 0.0 };
    let p = rough(h, rect(-20.0, -20.0, 20.0, 20.0));
    let tool = flat(6.0);
    let path = roughing(&s, &tool, &p).unwrap();
    verify(&path, &h).unwrap();
    let c = tool.cutter().unwrap();
    never_below(&path, &s, &c, 0.5, p.tolerance);
    let levels: Vec<f64> = (1..=6).map(|k| 8.0 - 8.0 * k as f64 / 6.0).collect();
    // Points are at the levels or on the part, but for the few where the tool steps over
    // the edge of the part's reach; and every level is cut.
    let mut seen = vec![false; levels.len()];
    let pts = cuts(&path);
    let mut other = 0;
    for q in &pts {
        match levels.iter().position(|l| (q.z - l).abs() < 1e-9) {
            Some(k) => seen[k] = true,
            None if rest(&s, &c, 0.5, *q).is_some_and(|z| (q.z - z).abs() < 1e-6) => {}
            None => other += 1,
        }
    }
    assert!(other * 100 < pts.len(), "{other} of {} points", pts.len());
    assert!(seen.iter().all(|s| *s), "{seen:?}");
    close(path.stats().lowest_z.unwrap(), 0.0, 1e-9);
    // Over the block the tool cuts the level above it, then the next level is below its top
    // (with the stock to leave) and cuts there, and the lower levels skip what is already cut.
    let mut over: Vec<f64> = cuts(&path).iter().filter(|q| q.x.abs() <= 4.0 && q.y.abs() <= 4.0).map(|q| q.z).collect();
    let passes_over = over.len();
    over.sort_by(f64::total_cmp);
    over.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    assert_eq!(over.len(), 2, "{over:?}");
    close(over[0], 5.5, 1e-9);
    close(over[1], levels[0], 1e-9);
    // Each of the two levels runs the same passes over the block.
    let at_top = cuts(&path).iter().filter(|q| q.x.abs() <= 4.0 && q.y.abs() <= 4.0 && (q.z - 5.5).abs() < 1e-9).count();
    assert_eq!(2 * at_top, passes_over);
}

#[test]
fn roughing_a_sphere_never_cuts_into_it() {
    let s = sphere(Vec3::new(0.0, 0.0, 0.0), 10.0, 24);
    let h = Heights { clearance: 25.0, retract: 15.0, top: 11.0, bottom: -1.0 };
    for (tool, angle) in [(ball(6.0), 0.0), (bull(6.0, 1.0), 0.7), (flat(6.0), PI / 2.0)] {
        let mut p = rough(h, rect(-15.0, -15.0, 15.0, 15.0));
        p.angle = angle;
        p.zigzag = angle == 0.0;
        let path = roughing(&s, &tool, &p).unwrap();
        verify(&path, &h).unwrap();
        never_below(&path, &s, &tool.cutter().unwrap(), 0.5, p.tolerance);
        // The last level reaches the floor beside the sphere.
        close(path.stats().lowest_z.unwrap(), -1.0, 1e-9);
    }
    let mut bad = rough(h, rect(-15.0, -15.0, 15.0, 15.0));
    bad.stepdown = 0.0;
    assert!(roughing(&s, &ball(6.0), &bad).is_err());
    bad.stepdown = 1e-4;
    bad.heights.bottom = -1e5;
    assert!(matches!(roughing(&s, &ball(6.0), &bad), Err(CamError::TooLarge(_))));
}

fn wl(heights: Heights, boundary: Boundary) -> WaterlineParams {
    WaterlineParams { heights, feeds: feeds(), stepdown: 5.0, boundary, stock_to_leave: 0.0, step: 0.5, tolerance: 0.005, climb: true }
}

/// The runs cut at feed, each from its plunge to its retract.
fn runs(path: &Toolpath, cut: f64) -> Vec<Vec<Vec3>> {
    let mut out = Vec::new();
    let mut cur: Vec<Vec3> = Vec::new();
    for m in &path.moves {
        match *m {
            Move::Feed { to, feed } if feed == cut => cur.push(to),
            Move::Feed { to, .. } => {
                if cur.len() > 1 {
                    out.push(std::mem::take(&mut cur));
                }
                cur = vec![to];
            }
            _ => {
                if cur.len() > 1 {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
            }
        }
    }
    out
}

#[test]
fn waterline_around_a_sphere() {
    let (rs, r) = (20.0, 3.0);
    let s = sphere(Vec3::new(0.0, 0.0, 0.0), rs, 48);
    let h = Heights { clearance: 40.0, retract: 25.0, top: 15.0, bottom: 0.0 };
    let mut p = wl(h, rect(-30.0, -30.0, 30.0, 30.0));
    p.step = 1.0;
    let path = waterline(&s, &ball(2.0 * r), &p).unwrap();
    verify(&path, &h).unwrap();
    let loops = runs(&path, p.feeds.cut);
    assert_eq!(loops.len(), 4, "one loop per level");
    for (lp, level) in loops.iter().zip([15.0, 10.0, 5.0, 0.0]) {
        close3(lp[0], *lp.last().unwrap(), 1e-12);
        assert!(lp.len() > 50);
        let want = ((rs + r) * (rs + r) - (level + r) * (level + r)).sqrt();
        let mut area = 0.0;
        for (a, b) in lp.iter().zip(lp.iter().skip(1)) {
            close(b.z, level, 1e-12);
            let rho = b.x.hypot(b.y);
            // Inside by at most the facets' chord error over the wall's slope.
            assert!(rho >= want - 0.1 && rho <= want + p.tolerance + 1e-9, "level {level}: ρ {rho} vs {want}");
            area += a.x * b.y - b.x * a.y;
        }
        // Climb: the material on the right, so round the sphere clockwise.
        assert!(area < 0.0);
    }
    let mut conv = p.clone();
    conv.climb = false;
    conv.heights.top = 5.0;
    let back = waterline(&s, &ball(2.0 * r), &conv).unwrap();
    let back = runs(&back, p.feeds.cut);
    assert_eq!(back.len(), 2);
    for lp in back {
        let area: f64 = lp.iter().zip(lp.iter().skip(1)).map(|(a, b)| a.x * b.y - b.x * a.y).sum();
        assert!(area > 0.0);
    }
}

#[test]
fn waterline_around_two_spheres_and_a_saddle() {
    // Two spheres whose waterlines at the lower level merge into one loop, through cells
    // where the corners disagree diagonally.
    let mut tris: Vec<[Vec3; 3]> = sphere(Vec3::new(-9.0, 0.0, 0.0), 10.0, 48).triangles().to_vec();
    tris.extend_from_slice(sphere(Vec3::new(9.0, 0.0, 0.0), 10.0, 48).triangles());
    let s = Surface::new(tris).unwrap();
    let h = Heights { clearance: 30.0, retract: 15.0, top: 8.0, bottom: 2.0 };
    let mut p = wl(h, rect(-25.0, -15.0, 25.0, 15.0));
    p.step = 0.75;
    p.stepdown = 6.0;
    let path = waterline(&s, &ball(4.0), &p).unwrap();
    verify(&path, &h).unwrap();
    let loops = runs(&path, p.feeds.cut);
    assert_eq!(loops.len(), 3, "two loops at the top level, one round both lower down");
    for lp in &loops {
        close3(lp[0], *lp.last().unwrap(), 1e-12);
    }
    let c = ball(4.0).cutter().unwrap();
    for q in cuts(&path) {
        if let Some(z) = rest(&s, &c, 0.0, q) {
            assert!(q.z >= z - 1e-9, "{q:?} is in the part");
        }
    }
    // A region clips the loops into open runs.
    p.boundary.region = Some(Region2 { outer: Loop2::circle(Vec2::new(-9.0, 0.0), 8.0), holes: vec![] });
    p.heights.bottom = 8.0;
    let clipped = waterline(&s, &ball(4.0), &p).unwrap();
    verify(&clipped, &p.heights).unwrap();
    let parts = runs(&clipped, p.feeds.cut);
    assert!(!parts.is_empty());
    for q in parts.iter().flatten() {
        assert!(Vec2::new(q.x, q.y).dist(Vec2::new(-9.0, 0.0)) <= 8.0 + 1e-3);
    }
    // Grids too fine are refused.
    p.step = 1e-3;
    assert!(matches!(waterline(&s, &ball(4.0), &p), Err(CamError::TooLarge(_))));
    p.step = f64::NAN;
    assert!(waterline(&s, &ball(4.0), &p).is_err());
}

#[test]
fn surface_params_round_trip_through_json() {
    let h = Heights { clearance: 10.0, retract: 5.0, top: 0.0, bottom: -1.0 };
    let mut p = par(h, rect(0.0, 0.0, 1.0, 1.0));
    p.boundary.region = Some(Region2 { outer: Loop2::circle(Vec2::new(0.0, 0.0), 1.0), holes: vec![] });
    let json = serde_json::to_string(&p).unwrap();
    assert_eq!(serde_json::from_str::<ParallelParams>(&json).unwrap(), p);
    let r = rough(h, Boundary::default());
    assert_eq!(serde_json::from_str::<RoughingParams>(&serde_json::to_string(&r).unwrap()).unwrap(), r);
    let w = wl(h, Boundary::default());
    assert_eq!(serde_json::from_str::<WaterlineParams>(&serde_json::to_string(&w).unwrap()).unwrap(), w);
}

/// A profile of `(u, z)` points swept from -20 to 20 along the other axis: `u` is X when
/// `along_x`, Y otherwise. Vertical steps are walls.
fn extruded(profile: &[(f64, f64)], along_x: bool) -> Surface {
    let p = |u: f64, v: f64, z: f64| if along_x { Vec3::new(u, v, z) } else { Vec3::new(v, u, z) };
    let mut tris = Vec::new();
    for w in profile.windows(2) {
        let ((u0, z0), (u1, z1)) = (w[0], w[1]);
        let (a, b, c, d) = (p(u0, -20.0, z0), p(u1, -20.0, z1), p(u1, 20.0, z1), p(u0, 20.0, z0));
        tris.push([a, b, c]);
        tris.push([a, c, d]);
    }
    Surface::new(tris).unwrap()
}

/// A vertical pin of radius `r` about the Z axis from 0 to `top`, `n` facets round, capped.
fn pin(r: f64, top: f64, n: usize) -> Surface {
    let at = |j: usize, z: f64| {
        let th = 2.0 * PI * (j % n) as f64 / n as f64;
        Vec3::new(r * th.cos(), r * th.sin(), z)
    };
    let mut tris = Vec::new();
    for j in 0..n {
        let (a, b, c, d) = (at(j, 0.0), at(j + 1, 0.0), at(j + 1, top), at(j, top));
        tris.push([a, b, c]);
        tris.push([a, c, d]);
        tris.push([Vec3::new(0.0, 0.0, top), at(j, top), at(j + 1, top)]);
    }
    Surface::new(tris).unwrap()
}

/// The deepest any move at feed goes below where the cutter rests, sampled every 0.01 mm.
fn worst_gouge(path: &Toolpath, s: &Surface, c: &Cutter, leave: f64) -> f64 {
    let mut worst = f64::NEG_INFINITY;
    for (a, b) in feed_moves(path) {
        let n = ((a.dist(b) / 0.01).ceil() as usize).clamp(1, 5000);
        for k in 0..=n {
            let q = a.lerp(b, k as f64 / n as f64);
            if let Some(z) = rest(s, c, leave, q) {
                worst = worst.max(z - q.z);
            }
        }
    }
    worst
}

#[test]
fn a_link_between_passes_never_cuts_past_an_edge() {
    // A block top at z 5 ending in a wall at y 0, the floor beyond. The pass at y 2.7 rides the
    // block's top (the flat cutter reaches its edge until y 3) and the next at y 4.2 is on the
    // floor: the link between them must not feed straight down through the edge.
    let s = extruded(&[(-20.0, 5.0), (0.0, 5.0), (0.0, 0.0), (20.0, 0.0)], false);
    let h = Heights { clearance: 20.0, retract: 10.0, top: 5.0, bottom: -1.0 };
    let p = par(h, rect(-10.0, -0.3, 10.0, 5.7));
    let tool = flat(6.0);
    let path = parallel(&s, &tool, &p).unwrap();
    verify(&path, &h).unwrap();
    let gouge = worst_gouge(&path, &s, &tool.cutter().unwrap(), 0.0);
    assert!(gouge <= p.tolerance + 1e-9, "a feed is {gouge} mm into the part");
}

#[test]
fn parallel_stays_within_the_tolerance_over_a_chamfer_and_a_thin_rib() {
    // A 45° chamfer from z 0 up to 0.5. With these samples the dropped height half way along
    // the stretch over it is exactly on the chord between its ends.
    let chamfer = extruded(&[(-20.0, 0.0), (0.0, 0.0), (0.5, 0.5), (20.0, 0.5)], true);
    let h = Heights { clearance: 20.0, retract: 10.0, top: 3.0, bottom: -1.0 };
    let p = par(h, rect(-10.25, -1.0, 9.75, 1.0));
    let tool = flat(6.0);
    let path = parallel(&chamfer, &tool, &p).unwrap();
    verify(&path, &h).unwrap();
    let gouge = worst_gouge(&path, &chamfer, &tool.cutter().unwrap(), 0.0);
    assert!(gouge <= p.tolerance + 1e-9, "flat: a feed is {gouge} mm into the chamfer");
    let mut p = par(h, rect(-10.0, -10.0, 10.0, 10.0));
    p.angle = 0.3;
    let tool = bull(6.0, 0.5);
    let path = parallel(&chamfer, &tool, &p).unwrap();
    let gouge = worst_gouge(&path, &chamfer, &tool.cutter().unwrap(), 0.0);
    assert!(gouge <= p.tolerance + 1e-9, "bull nose: a feed is {gouge} mm into the chamfer");
    // A rib 0.5 mm wide and 3 mm tall, between samples 10 mm apart: a step that long is not
    // taken past the cutter's radius, so the pass still rises over the rib.
    let rib = extruded(&[(-20.0, 0.0), (2.25, 0.0), (2.25, 3.0), (2.75, 3.0), (2.75, 0.0), (20.0, 0.0)], true);
    let mut p = par(h, rect(0.0, -1.0, 10.0, 1.0));
    p.step = 10.0;
    let tool = flat(2.0);
    let path = parallel(&rib, &tool, &p).unwrap();
    verify(&path, &h).unwrap();
    let gouge = worst_gouge(&path, &rib, &tool.cutter().unwrap(), 0.0);
    assert!(gouge <= p.tolerance + 1e-9, "a feed is {gouge} mm into the rib");
}

#[test]
fn roughing_goes_down_at_the_plunge_feed() {
    // Where the tool steps off the part's reach it goes straight down into stock no level has
    // cut yet: that is a plunge, not a cut.
    let s = block(Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 5.0));
    let h = Heights { clearance: 20.0, retract: 10.0, top: 8.0, bottom: 0.0 };
    let p = rough(h, rect(-20.0, -20.0, 20.0, 20.0));
    let path = roughing(&s, &flat(6.0), &p).unwrap();
    let mut pos: Option<Vec3> = None;
    let mut steep = 0;
    for m in &path.moves {
        if let (Some(q), Move::Feed { to, feed }) = (pos, *m) {
            let drop = q.z - to.z;
            let across = (to.x - q.x).hypot(to.y - q.y);
            if drop > 1e-9 && drop > across {
                steep += 1;
                // It goes down no faster than the plunge feed, and straight down at exactly it.
                assert!(feed * drop / q.dist(to) <= p.feeds.plunge * (1.0 + 1e-9), "{q:?} → {to:?} goes down at {feed}");
                if across <= 1e-12 {
                    assert_eq!(feed, p.feeds.plunge, "{q:?} → {to:?} plunges at {feed}");
                }
            }
        }
        pos = m.target().or(pos);
    }
    assert!(steep > 20, "{steep} moves down");
}

#[test]
fn waterline_round_a_pin_never_cuts_into_it() {
    // The clear contour round a pin of radius 1 is a circle 1 + the cutter radius from its
    // axis; straight feeds between points a grid step apart would cut inside it.
    let s = pin(1.0, 10.0, 256);
    let h = Heights { clearance: 30.0, retract: 15.0, top: 8.0, bottom: 2.0 };
    let mut p = wl(h, rect(-6.0, -6.0, 6.0, 6.0));
    p.step = 1.0;
    p.stepdown = 3.0;
    let inner = 2.0 + (PI / 256.0).cos();
    for tool in [ball(4.0), flat(4.0)] {
        let path = waterline(&s, &tool, &p).unwrap();
        verify(&path, &h).unwrap();
        assert_eq!(runs(&path, p.feeds.cut).len(), 3, "one loop per level");
        for (a, b) in feed_moves(&path) {
            for k in 0..=50 {
                let q = a.lerp(b, k as f64 / 50.0);
                let rho = q.x.hypot(q.y);
                assert!(rho >= inner - p.tolerance - 1e-9, "{q:?} on {a:?} → {b:?} is {rho} from the axis");
            }
        }
    }
}

/// The distance seen from above from `q` to the square `[-s, s]²`.
fn off_square(q: Vec3, s: f64) -> f64 {
    (q.x.abs() - s).max(0.0).hypot((q.y.abs() - s).max(0.0))
}

#[test]
fn waterline_at_the_floor_traces_the_foot_of_the_wall() {
    // A block standing on a floor, the lowest level at the floor: the loop round the foot of
    // the wall is cut there, not dropped because the floor is exactly at the level.
    let mut tris = block(Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 10.0)).triangles().to_vec();
    tris.extend_from_slice(plane(30.0, 0.0).triangles());
    let s = Surface::new(tris).unwrap();
    let h = Heights { clearance: 30.0, retract: 15.0, top: 9.0, bottom: 0.0 };
    let mut p = wl(h, rect(-20.0, -20.0, 20.0, 20.0));
    p.stepdown = 3.0;
    let path = waterline(&s, &ball(6.0), &p).unwrap();
    verify(&path, &h).unwrap();
    let loops = runs(&path, p.feeds.cut);
    let at_floor: Vec<&Vec<Vec3>> = loops.iter().filter(|lp| lp.iter().all(|q| q.z.abs() < 1e-12)).collect();
    assert_eq!(at_floor.len(), 1, "{} loops", loops.len());
    for q in at_floor.iter().copied().flatten() {
        let d = off_square(*q, 10.0);
        assert!(d >= 3.0 - p.tolerance - 1e-9 && d <= 3.0 + 0.5, "{q:?} is {d} from the wall");
    }
}

#[test]
fn waterline_with_the_default_boundary_runs_round_a_standalone_part() {
    // The contour round a part's outer walls is a cutter radius outside the part: the default
    // boundary takes that in.
    let s = block(Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 10.0));
    let h = Heights { clearance: 30.0, retract: 15.0, top: 7.0, bottom: 1.0 };
    let mut p = wl(h, Boundary::default());
    p.stepdown = 3.0;
    let path = waterline(&s, &ball(6.0), &p).unwrap();
    verify(&path, &h).unwrap();
    let loops = runs(&path, p.feeds.cut);
    assert_eq!(loops.len(), 3, "one loop per level");
    for lp in &loops {
        close3(lp[0], *lp.last().unwrap(), 1e-12);
        for q in lp {
            let d = off_square(*q, 10.0);
            assert!(d >= 3.0 - p.tolerance - 1e-9 && d <= 3.0 + 0.5, "{q:?} is {d} from the wall");
        }
    }
}

#[test]
fn levels_times_samples_are_bounded() {
    // A small sphere in a large grid with very many levels: each level would visit every cell.
    let s = sphere(Vec3::new(0.0, 0.0, 0.0), 5.0, 24);
    let h = Heights { clearance: 30.0, retract: 15.0, top: 4.9, bottom: -0.9 };
    let mut p = wl(h, rect(-400.0, -400.0, 400.0, 400.0));
    p.step = 0.8;
    p.stepdown = 0.005;
    assert!(matches!(waterline(&s, &ball(2.0), &p), Err(CamError::TooLarge(_))));
    // Roughing with very many levels over very many passes: each level would look at every pass.
    let high = plane(2000.0, 10.0);
    let h = Heights { clearance: 30.0, retract: 15.0, top: 0.0, bottom: -9999.0 };
    let mut r = rough(h, rect(0.0, 0.0, 0.001, 1000.0));
    r.stepover = 0.005;
    r.stepdown = 1.0;
    assert!(matches!(roughing(&high, &flat(6.0), &r), Err(CamError::TooLarge(_))));
    // A small region of a large rectangle: the passes that miss it cost nothing at each level.
    let deep = plane(1.0, -5000.0);
    let h = Heights { clearance: 30.0, retract: 15.0, top: 0.0, bottom: -1100.0 };
    let mut r = rough(h, rect(-500.0, -500.0, 500.0, 500.0));
    r.boundary.region = Some(Region2 { outer: Loop2::circle(Vec2::new(0.0, 0.0), 2.0), holes: vec![] });
    r.stepover = 1.0;
    r.stepdown = 1.0;
    let path = roughing(&deep, &flat(6.0), &r).unwrap();
    verify(&path, &h).unwrap();
    assert!(cuts(&path).iter().all(|q| q.x.hypot(q.y) <= 2.0 + 0.01));
    // Levels nothing can be cut at cost nothing: the part is above all of them.
    let high = plane(60.0, 0.0);
    let h = Heights { clearance: 30.0, retract: 15.0, top: -1.0, bottom: -900.0 };
    let mut r = rough(h, rect(-50.0, -50.0, 50.0, 50.0));
    r.stepdown = 1.0;
    r.step = 0.1;
    assert!(roughing(&high, &flat(6.0), &r).unwrap().moves.is_empty());
}

/// With no descent limit and the plunge feed as the descent feed, no cutting move goes down
/// faster than the plunge feed: a move that descends is slowed in proportion, so a ramp just
/// under 45° is not taken at the full cut feed (and the feed does not jump between the two on
/// near-45° slivers). Roughing always works so.
#[test]
fn descents_never_go_down_faster_than_the_plunge_feed() {
    let s = sphere(Vec3::new(0.0, 0.0, 0.0), 10.0, 32);
    let h = Heights { clearance: 25.0, retract: 15.0, top: 11.0, bottom: -1.0 };
    let f = feeds();
    let check = |path: &Toolpath| {
        let mut pos: Option<Vec3> = None;
        let mut descents = 0;
        for m in &path.moves {
            if let (Move::Feed { to, feed }, Some(q)) = (*m, pos) {
                let (len, drop) = (q.dist(to), q.z - to.z);
                if drop > 1e-9 {
                    descents += 1;
                    assert!(feed * drop / len <= f.plunge * (1.0 + 1e-9), "down at {} mm/min (feed {feed}, {drop} in {len})", feed * drop / len);
                    assert!(feed >= f.plunge - 1e-9);
                }
            }
            if let Some(t) = m.target() {
                pos = Some(t);
            }
        }
        assert!(descents > 0);
    };
    for tool in [flat(6.0), ball(6.0)] {
        check(&roughing(&s, &tool, &rough(h, rect(-15.0, -15.0, 15.0, 15.0))).unwrap());
        let mut p = par(h, rect(-15.0, -15.0, 15.0, 15.0));
        p.descent_limit = 0.0;
        p.descent_feed = Some(f.plunge);
        check(&parallel(&s, &tool, &p).unwrap());
    }
}

/// Finishing runs down the surface at the cut feed: with the plunge feed as the descent feed,
/// only moves at least as steep as the descent limit (70° by default) and entries from above
/// are slowed to the plunge feed's rate of descent. On a sphere a 45° flank used to run at
/// about a fifth of the cut feed.
#[test]
fn finishing_runs_down_the_surface_at_the_cut_feed() {
    let s = sphere(Vec3::new(0.0, 0.0, 0.0), 10.0, 64);
    let h = Heights { clearance: 25.0, retract: 15.0, top: 11.0, bottom: -1.0 };
    let f = feeds();
    for limit in [DEFAULT_DESCENT_LIMIT, 45f64.to_radians()] {
        let mut p = par(h, rect(-12.0, -12.0, 12.0, 12.0));
        p.descent_limit = limit;
        p.descent_feed = Some(f.plunge);
        let path = parallel(&s, &ball(4.0), &p).unwrap();
        verify(&path, &h).unwrap();
        let (mut shallow, mut steep) = (0, 0);
        let mut pos: Option<Vec3> = None;
        for m in &path.moves {
            if let (Move::Feed { to, feed }, Some(q)) = (*m, pos) {
                let (len, drop) = (q.dist(to), q.z - to.z);
                if drop > 1e-9 && drop < limit.sin() * len * (1.0 - 1e-9) {
                    shallow += 1;
                    assert_eq!(feed, f.cut, "{q:?} → {to:?} down the surface at {feed}");
                } else if drop > 1e-9 {
                    steep += 1;
                    assert!(feed * drop / len <= f.plunge * (1.0 + 1e-9), "{q:?} → {to:?} goes down at {}", feed * drop / len);
                }
            }
            pos = m.target().or(pos);
        }
        assert!(shallow > 100 && steep > 0, "{shallow} down the surface, {steep} steep");
    }
    // A descent limit beyond a right angle, or not a number, is refused.
    for bad in [-0.1, 1.6, f64::NAN] {
        let mut p = par(h, rect(-12.0, -12.0, 12.0, 12.0));
        p.descent_limit = bad;
        assert!(parallel(&s, &ball(4.0), &p).is_err());
    }
    // Left out of the JSON, the default.
    let mut v = serde_json::to_value(par(h, rect(0.0, 0.0, 1.0, 1.0))).unwrap();
    v.as_object_mut().unwrap().remove("descent_limit");
    assert_eq!(serde_json::from_value::<ParallelParams>(v).unwrap().descent_limit, DEFAULT_DESCENT_LIMIT);
}

/// A parallel finish over a block's vertical wall: the plunge feed is for entries from above,
/// and the moves inside a pass that run down the wall go at the descent feed, the cut feed by
/// default (they used to run at the plunge feed: on a domed block with 20 mm walls that was 11.8
/// of its 18.8 minutes). A descent limit of a right angle never slows them.
#[test]
fn moves_down_a_wall_run_at_the_descent_feed_and_entries_at_the_plunge_feed() {
    // A block 10 mm high from X -10 to 10, standing on a floor at Z 0.
    let s = extruded(&[(-20.0, 0.0), (-10.0, 0.0), (-10.0, 10.0), (10.0, 10.0), (10.0, 0.0), (20.0, 0.0)], true);
    let h = Heights { clearance: 25.0, retract: 15.0, top: 11.0, bottom: 0.0 };
    let f = feeds();
    // (descent feed, descent limit, the feed down the wall)
    for (given, limit, want) in [(None, DEFAULT_DESCENT_LIMIT, f.cut), (Some(300.0), DEFAULT_DESCENT_LIMIT, 300.0), (Some(300.0), PI / 2.0, f.cut)] {
        let mut p = par(h, rect(-18.0, -5.0, 18.0, 5.0));
        p.descent_feed = given;
        p.descent_limit = limit;
        let path = parallel(&s, &flat(4.0), &p).unwrap();
        verify(&path, &h).unwrap();
        let (mut walls, mut entries) = (0, 0);
        let mut pos: Option<Vec3> = None;
        for m in &path.moves {
            if let (Move::Feed { to, feed }, Some(q)) = (*m, pos)
                && q.z - to.z > 5.0
                && (q.x - to.x).hypot(q.y - to.y) < 1e-6
            {
                if q.z >= h.retract - 1e-9 {
                    entries += 1;
                    assert_eq!(feed, f.plunge, "an entry from {q:?}");
                } else {
                    walls += 1;
                    assert_eq!(feed, want, "down the wall from {q:?} with {given:?}");
                }
            }
            pos = m.target().or(pos);
        }
        assert!(walls > 3 && entries >= 1, "{walls} down the wall, {entries} entries");
    }
    // A descent feed that is not a feed is refused.
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let mut p = par(h, rect(-18.0, -5.0, 18.0, 5.0));
        p.descent_feed = Some(bad);
        assert!(parallel(&s, &flat(4.0), &p).is_err(), "{bad}");
    }
}

/// A link between passes is never further into the part than half the tolerance: it follows the
/// surface where the straight line would cut into it. Over a dome and the rounded edge of the
/// block it stands on (where a link once cut 0.016 mm in at a tolerance of 0.01), sampled every
/// 0.01 mm.
#[test]
fn links_between_passes_follow_the_surface() {
    let mut tris = sphere(Vec3::new(0.0, 0.0, -6.0), 12.0, 96).triangles().to_vec();
    tris.extend_from_slice(cylinder_x(2.0, -30.0, 30.0, 96).triangles());
    let s = Surface::new(tris).unwrap();
    let h = Heights { clearance: 30.0, retract: 15.0, top: 7.0, bottom: -4.0 };
    let mut p = par(h, rect(-14.0, -6.0, 14.0, 6.0));
    p.stepover = 0.4;
    p.step = 1.0;
    let tool = ball(4.0);
    let path = parallel(&s, &tool, &p).unwrap();
    verify(&path, &h).unwrap();
    let c = tool.cutter().unwrap();
    let mut links = 0;
    let mut worst: f64 = 0.0;
    for (a, b) in feed_moves(&path) {
        if (a.y - b.y).abs() < 1e-9 {
            continue;
        }
        links += 1;
        let n = ((a.dist(b) / 0.01).ceil() as usize).clamp(1, 5000);
        for k in 0..=n {
            let q = a.lerp(b, k as f64 / n as f64);
            if let Some(z) = rest(&s, &c, 0.0, q) {
                worst = worst.max(z - q.z);
            }
        }
    }
    assert!(links > 20, "{links} links");
    assert!(worst <= 0.5 * p.tolerance + 1e-9, "a link is {worst} mm into the part");
}

/// Each roughing level comes down at rapid through the air the levels before it cleared, to
/// [`ENTRY_GAP`] above the material left (the level before, or the stock's top), and feeds in
/// from there; it used to feed all the way from the retract height.
#[test]
fn roughing_levels_come_down_through_the_air_they_cleared() {
    let s = block(Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 5.0));
    let h = Heights { clearance: 30.0, retract: 20.0, top: 8.0, bottom: 0.0 };
    let p = rough(h, rect(-20.0, -20.0, 20.0, 20.0));
    let path = roughing(&s, &flat(6.0), &p).unwrap();
    verify(&path, &h).unwrap();
    let levels: Vec<f64> = (0..=6).map(|k| 8.0 - 8.0 * k as f64 / 6.0).collect();
    let mut pos: Option<Vec3> = None;
    let mut entries = 0;
    let mut last_rapid = false;
    for m in &path.moves {
        if let (Move::Feed { to, feed }, Some(q)) = (*m, pos)
            && last_rapid
        {
            entries += 1;
            assert_eq!(feed, p.feeds.plunge);
            // Straight down, from just above the level before this one's.
            assert!((q.x - to.x).hypot(q.y - to.y) < 1e-12, "{q:?} → {to:?}");
            let before = levels.iter().copied().filter(|l| *l > to.z + 1e-9).fold(f64::INFINITY, f64::min);
            assert!(q.z <= before + ENTRY_GAP + 1e-9, "{q:?} → {to:?}: fed from {} above the level before", q.z - before);
        }
        last_rapid = matches!(m, Move::Rapid { .. });
        pos = m.target().or(pos);
    }
    assert!(entries >= 6, "{entries} entries");
}

/// The points of `path` at feed, each reached from the one before.
fn feed_points(path: &Toolpath) -> Vec<Vec3> {
    feed_moves(path).into_iter().flat_map(|(a, b)| [a, b]).collect()
}

/// The distance from `q` to the nearest move of `path` at feed, arcs followed closely.
fn off_path(path: &Toolpath, q: Vec3) -> f64 {
    let pts = path.points(1e-5);
    pts.windows(2).filter(|w| w[1].1).map(|w| q.dist_to_segment(w[0].0, w[1].0)).fold(f64::INFINITY, f64::min)
}

/// Merging: a roughing level's straight passes become one move each, a waterline's loop round
/// a pin becomes arcs, and every point merged away is within the tolerance of what replaces it.
#[test]
fn compacting_merges_lines_and_fits_arcs_within_the_tolerance() {
    let s = block(Vec3::new(-10.0, -10.0, 0.0), Vec3::new(10.0, 10.0, 5.0));
    let h = Heights { clearance: 20.0, retract: 10.0, top: 8.0, bottom: 0.0 };
    let rough_path = roughing(&s, &flat(6.0), &rough(h, rect(-20.0, -20.0, 20.0, 20.0))).unwrap();
    let how = Compaction { tolerance: 0.001, arcs: true, helical: true, bow: 0.001 };
    let merged = compact(&rough_path, &how).unwrap();
    verify(&merged, &h).unwrap();
    assert!(merged.moves.len() * 4 < rough_path.moves.len(), "{} of {} moves", merged.moves.len(), rough_path.moves.len());
    for q in feed_points(&rough_path) {
        assert!(off_path(&merged, q) <= how.tolerance + 1e-9, "{q:?} is {} off", off_path(&merged, q));
    }
    close(merged.stats().cut_length, rough_path.stats().cut_length, 1e-2);
    // Straight passes give no arcs.
    assert!(!merged.moves.iter().any(|m| matches!(m, Move::Arc { .. })));

    let pin = pin(1.0, 10.0, 512);
    let hw = Heights { clearance: 30.0, retract: 15.0, top: 8.0, bottom: 2.0 };
    let mut p = wl(hw, rect(-6.0, -6.0, 6.0, 6.0));
    p.step = 0.5;
    p.stepdown = 3.0;
    p.tolerance = 0.0005;
    let loops = waterline(&pin, &flat(4.0), &p).unwrap();
    let how = Compaction { tolerance: 0.002, arcs: true, helical: false, bow: p.tolerance };
    let merged = compact(&loops, &how).unwrap();
    verify(&merged, &hw).unwrap();
    let arcs = merged.moves.iter().filter(|m| matches!(m, Move::Arc { .. })).count();
    assert!(arcs >= 6 && merged.moves.len() * 5 < loops.moves.len(), "{arcs} arcs, {} of {} moves", merged.moves.len(), loops.moves.len());
    for q in feed_points(&loops) {
        assert!(off_path(&merged, q) <= how.tolerance + 1e-9, "{q:?} is {} off", off_path(&merged, q));
    }
    // Without arcs only lines are merged; a level loop is no helix.
    let lines_only = compact(&loops, &Compaction { arcs: false, ..how }).unwrap();
    assert!(!lines_only.moves.iter().any(|m| matches!(m, Move::Arc { .. })));
    // A hostile tolerance is refused.
    for bad in [-1.0, f64::NAN, 1e9] {
        assert!(compact(&loops, &Compaction { tolerance: bad, ..how }).is_err());
        assert!(compact(&loops, &Compaction { bow: bad, ..how }).is_err());
    }
}

/// A merged move stays near the path it replaces all along it, not only at the points it
/// replaces: a line within the tolerance, an arc within the tolerance and the bow asked for.
/// The corners of an octagon lie on a circle, but an arc through them bows out of the
/// octagon's sides by 7.6% of its radius between them (it was fitted, and a wall the octagon
/// runs along would have been cut 0.76 mm deep).
#[test]
fn compacting_never_moves_the_path_far_between_its_points() {
    let corner = |k: usize| {
        let a = k as f64 * PI / 4.0;
        Vec3::new(10.0 * a.cos(), 10.0 * a.sin(), 0.0)
    };
    let mut moves = vec![Move::Rapid { to: corner(0) }];
    moves.extend((1..=8).map(|k| Move::Feed { to: corner(k), feed: 500.0 }));
    let octagon = Toolpath { moves };
    // The same with each side cut into steps: merging gives the sides back as lines.
    let mut moves = vec![Move::Rapid { to: corner(0) }];
    for k in 0..8 {
        moves.extend((1..=10).map(|s| Move::Feed { to: corner(k).lerp(corner(k + 1), s as f64 / 10.0), feed: 500.0 }));
    }
    let stepped = Toolpath { moves };
    let pin = pin(1.0, 10.0, 512);
    let hw = Heights { clearance: 30.0, retract: 15.0, top: 8.0, bottom: 2.0 };
    let mut p = wl(hw, rect(-6.0, -6.0, 6.0, 6.0));
    p.step = 0.5;
    p.stepdown = 3.0;
    p.tolerance = 0.0005;
    let loops = waterline(&pin, &flat(4.0), &p).unwrap();
    for (path, tol, bow) in [(&octagon, 0.01, 0.01), (&stepped, 0.01, 0.01), (&loops, 0.002, p.tolerance)] {
        let how = Compaction { tolerance: tol, arcs: true, helical: true, bow };
        let merged = compact(path, &how).unwrap();
        let pts = merged.points(1e-5);
        for w in pts.windows(2).filter(|w| w[1].1) {
            for k in 0..=8 {
                let q = w[0].0.lerp(w[1].0, k as f64 / 8.0);
                assert!(off_path(path, q) <= tol + bow + 1e-9, "{q:?} is {} off the path it replaces", off_path(path, q));
            }
        }
        for q in feed_points(path) {
            assert!(off_path(&merged, q) <= tol + 1e-9, "{q:?} is {} off", off_path(&merged, q));
        }
    }
    // The waterline's loops round the pin are still written as arcs.
    let merged = compact(&loops, &Compaction { tolerance: 0.002, arcs: true, helical: false, bow: p.tolerance }).unwrap();
    assert!(merged.moves.iter().filter(|m| matches!(m, Move::Arc { .. })).count() >= 6);
    // The octagon stays an octagon.
    let merged = compact(&octagon, &Compaction { tolerance: 0.01, arcs: true, helical: true, bow: 0.01 }).unwrap();
    assert!(!merged.moves.iter().any(|m| matches!(m, Move::Arc { .. })), "{merged:?}");
}

/// A link between two points where nothing is under the cutter (the ends of two passes that
/// ran off the part's reach) still looks between them: here the straight line passes 0.2 mm
/// under where the ball rests on the corner of a block, and used to be taken as it was (a link
/// over the corner of the dome's block cut 0.0097 mm into it at a tolerance of 0.01).
#[test]
fn a_link_between_two_clear_points_rises_over_a_corner_between_them() {
    let s = block(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 10.0, 5.0));
    let c = Cutter::ball(2.0).unwrap();
    let tol = 0.01;
    let field = crate::surface_ops::Field::new(&s, &c, 0.0, None, tol).unwrap();
    // Both ends 2.05 from the corner's edge, the chord between them 1 long: its middle is 1.988
    // from the edge, where the ball rests on the corner at 3.22.
    let at = |deg: f64| Vec3::new(2.05 * deg.to_radians().cos(), 2.05 * deg.to_radians().sin(), 3.0);
    let half = (0.5f64 / 2.05).asin().to_degrees();
    let (a, b) = (at(225.0 - half), at(225.0 + half));
    assert!(rest(&s, &c, 0.0, a).is_none() && rest(&s, &c, 0.0, b).is_none());
    assert!(rest(&s, &c, 0.0, a.lerp(b, 0.5)).is_some_and(|z| z > 3.2), "the corner is under the middle");
    let way = crate::surface_ops::link_path(&field, a, b, 10.0, 1.0, tol).unwrap().unwrap();
    assert!(way.len() > 2, "{way:?}");
    let mut from = a;
    let mut worst = f64::NEG_INFINITY;
    for p in way {
        for k in 0..=200 {
            let q = from.lerp(p, k as f64 / 200.0);
            if let Some(z) = rest(&s, &c, 0.0, q) {
                worst = worst.max(z - q.z);
            }
        }
        from = p;
    }
    assert!(worst <= 0.5 * tol + 1e-9, "the link is {worst} mm into the corner");
}

/// A parallel or waterline finish's `heights.top` may be below the part (a parallel finish does
/// not use it): a rapid that stops above that top but inside the part is refused by the check
/// against where the cutter rests on the part, which the plain check (at or above `top`) lets
/// through. A finish over a dome with such a top is planned and passes it.
#[test]
fn a_finishing_rapid_is_held_to_the_part_not_to_the_top_height() {
    // A dome whose top is at Z 6.
    let s = sphere(Vec3::new(0.0, 0.0, -6.0), 12.0, 96);
    let h = Heights { clearance: 30.0, retract: 15.0, top: -3.0, bottom: -4.0 };
    let tool = ball(4.0);
    let field = crate::surface_ops::Field::new(&s, &tool.cutter().unwrap(), 0.0, None, 0.01).unwrap();
    let rest = |p: Vec2| field.rest(p);
    let down_to = |z: f64| Toolpath {
        moves: vec![
            Move::Rapid { to: Vec3::new(0.0, 0.0, 30.0) },
            Move::Rapid { to: Vec3::new(0.0, 0.0, 15.0) },
            Move::Rapid { to: Vec3::new(0.0, 0.0, z) },
            Move::Feed { to: Vec3::new(0.0, 0.0, z - 0.5), feed: 100.0 },
            Move::Rapid { to: Vec3::new(0.0, 0.0, 30.0) },
        ],
    };
    // Down at rapid to Z 2, 4 mm into the dome.
    verify(&down_to(2.0), &h).unwrap();
    let e = crate::check::verify_over(&down_to(2.0), &h, &rest).unwrap_err();
    assert!(e.to_string().contains("material"), "{e}");
    // Down to 1 mm above where the ball rests on its top.
    crate::check::verify_over(&down_to(7.0), &h, &rest).unwrap();
    // Parallel and waterline finishes with that top.
    let path = parallel(&s, &tool, &ParallelParams { stepover: 1.0, ..par(h, rect(-8.0, -8.0, 8.0, 8.0)) }).unwrap();
    crate::check::verify_over(&path, &h, &rest).unwrap();
    let w = WaterlineParams {
        heights: Heights { top: 5.0, ..h },
        feeds: feeds(),
        stepdown: 1.0,
        boundary: rect(-14.0, -14.0, 14.0, 14.0),
        stock_to_leave: 0.0,
        step: 0.5,
        tolerance: 0.01,
        climb: true,
    };
    let path = waterline(&s, &tool, &w).unwrap();
    crate::check::verify_over(&path, &w.heights, &rest).unwrap();
}

/// A stop asked for from another thread ends an operation with [`CamError::Stopped`] and no
/// path; without one, or with the flag down, the same operation plans as before.
#[test]
fn a_stop_ends_planning_with_nothing_planned() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    let s = sphere(Vec3::new(0.0, 0.0, 0.0), 20.0, 64);
    let h = Heights { clearance: 40.0, retract: 25.0, top: 20.0, bottom: -50.0 };
    let p = par(h, rect(-15.0, -15.0, 15.0, 15.0));
    let tool = ball(6.0);
    let planned = parallel(&s, &tool, &p).unwrap();
    let up = Arc::new(AtomicBool::new(false));
    assert_eq!(stop::scoped(Some(Arc::clone(&up)), || parallel(&s, &tool, &p)).unwrap(), planned);
    let down = Arc::new(AtomicBool::new(true));
    assert_eq!(stop::scoped(Some(down), || parallel(&s, &tool, &p)), Err(CamError::Stopped));
    assert_eq!(
        stop::scoped(Some(Arc::new(AtomicBool::new(true))), || roughing(&s, &flat(6.0), &rough(h, rect(-15.0, -15.0, 15.0, 15.0)))),
        Err(CamError::Stopped)
    );
    // The flag is this thread's only inside the scope.
    assert!(!stop::stopped());
    assert_eq!(parallel(&s, &tool, &p).unwrap(), planned);
}
