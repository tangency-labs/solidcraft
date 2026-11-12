//! Tests of adaptive clearing: the engagement stays within the limit (measured independently),
//! the region is cleared to the walls, the 3D roughing stays off the part, and hostile input
//! is refused without hanging.

use std::f64::consts::{PI, TAU};

use solidcore_record::{Loop2, Region2, Seg2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use crate::adaptive::{
    AdaptiveParams, AdaptivePath, AdaptiveRoughingParams, HeightMap, MoveRole, StockModel, adaptive, adaptive_in, adaptive_roughing,
    adaptive_roughing_in,
};
use crate::dropcutter::{OPENED, drop_z, drop_z_blocks};
use crate::engagement::{Material, engagement};
use crate::ops::{Cut, Feeds, Heights, PocketParams, pocket};
use crate::path::{Move, Toolpath};
use crate::surface::Surface;
use crate::surface_ops::Boundary;
use crate::tool::{Tool, ToolKind};
use crate::{CamError, Cutter, offset_regions, verify};

const DEG: f64 = PI / 180.0;
/// How far above the limit the independent measurement may find the engagement (degrees): the
/// planner works on a raster a sixteenth of the radius fine.
const ENGAGEMENT_TOL: f64 = 3.0;

fn flat(d: f64) -> Tool {
    Tool { flutes: 3, ..Tool::new(1, ToolKind::FlatEndmill, d, 20.0) }
}

fn poly(pts: &[(f64, f64)]) -> Loop2 {
    let p: Vec<Vec2> = pts.iter().map(|&(x, y)| Vec2::new(x, y)).collect();
    Loop2 { segs: (0..p.len()).map(|i| Seg2::Line { a: p[i], b: p[(i + 1) % p.len()] }).collect() }
}

fn region(outer: &[(f64, f64)], holes: &[&[(f64, f64)]]) -> Region2 {
    Region2 { outer: poly(outer), holes: holes.iter().map(|h| poly(h)).collect() }
}

fn square(s: f64) -> Region2 {
    region(&[(0.0, 0.0), (s, 0.0), (s, s), (0.0, s)], &[])
}

fn ell() -> Region2 {
    region(&[(0.0, 0.0), (40.0, 0.0), (40.0, 16.0), (16.0, 16.0), (16.0, 36.0), (0.0, 36.0)], &[])
}

fn with_island() -> Region2 {
    region(&[(0.0, 0.0), (40.0, 0.0), (40.0, 30.0), (0.0, 30.0)], &[&[(14.0, 10.0), (14.0, 20.0), (26.0, 20.0), (26.0, 10.0)]])
}

fn params() -> AdaptiveParams {
    AdaptiveParams {
        optimal_load: 0.6,
        min_engagement: 10.0 * DEG,
        max_engagement: 60.0 * DEG,
        stock_to_leave: 0.0,
        tolerance: 0.01,
        helix_diameter: 4.8,
        ramp_angle: 3.0 * DEG,
        climb: true,
    }
}

fn cut(top: f64, bottom: f64) -> Cut {
    Cut { heights: Heights { clearance: 20.0, retract: 2.0, top, bottom }, feeds: Feeds { cut: 1000.0, plunge: 200.0 }, stepdown: 3.0 }
}

/// Every point of the region within reach of where the centre may go (less `slack`) is within
/// the radius of a cutting move at the bottom: a raster of its own, exact distances.
fn assert_cleared(regions: &[Region2], path: &Toolpath, r: f64, leave: f64, bottom: f64, slack: f64) {
    let (checked, missed) = uncut(regions, path, r, leave, bottom, slack);
    assert!(checked > 100, "{checked}");
    assert!(missed.is_empty(), "{} of {checked} points left uncut, first {:?}", missed.len(), missed.first());
}

/// The points of a raster over the region within reach of where the centre may go (less
/// `slack`) that no cutting move at the bottom comes within the radius of, and how many points
/// were looked at.
fn uncut(regions: &[Region2], path: &Toolpath, r: f64, leave: f64, bottom: f64, slack: f64) -> (usize, Vec<Vec2>) {
    let centre = offset_regions(regions, r + leave, 1e-3).unwrap();
    let rings: Vec<Vec<Vec2>> = centre.iter().map(|l| l.segs.iter().flat_map(|s| s.polyline(1e-3)).collect()).collect();
    let inside = |q: Vec2| {
        let mut odd = false;
        for ring in &rings {
            for (a, b) in ring.iter().zip(ring.iter().cycle().skip(1)) {
                if (a.y > q.y) != (b.y > q.y) && q.x < a.x + (q.y - a.y) / (b.y - a.y) * (b.x - a.x) {
                    odd = !odd;
                }
            }
        }
        odd
    };
    let seg = |p: Vec2, a: Vec2, b: Vec2| {
        let d = b - a;
        let t = if d.len2() > 0.0 { ((p - a).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
        p.dist(a + d * t)
    };
    let to_c = |q: Vec2| {
        rings.iter().flat_map(|ring| ring.iter().zip(ring.iter().cycle().skip(1))).map(|(a, b)| seg(q, *a, *b)).fold(f64::INFINITY, f64::min)
    };
    let pts = path.points(1e-3);
    let mut cuts: Vec<(Vec2, Vec2)> = Vec::new();
    for w in pts.windows(2) {
        let ((a, _), (b, cutting)) = (w[0], w[1]);
        if cutting && (a.z - bottom).abs() < 1e-6 && (b.z - bottom).abs() < 1e-6 {
            cuts.push((Vec2::new(a.x, a.y), Vec2::new(b.x, b.y)));
        }
    }
    let (mut lo, mut hi) = (Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
    for r in regions {
        for s in &r.outer.segs {
            for p in s.polyline(1e-3) {
                lo = Vec2::new(lo.x.min(p.x), lo.y.min(p.y));
                hi = Vec2::new(hi.x.max(p.x), hi.y.max(p.y));
            }
        }
    }
    let step = 0.25;
    let (mut checked, mut missed) = (0, Vec::new());
    let mut y = lo.y + 0.13;
    while y < hi.y {
        let mut x = lo.x + 0.07;
        while x < hi.x {
            let q = Vec2::new(x, y);
            if inside(q) || to_c(q) <= r - slack {
                checked += 1;
                if !cuts.iter().any(|(a, b)| seg(q, *a, *b) <= r + 1e-6) {
                    missed.push(q);
                }
            }
            x += step;
        }
        y += step;
    }
    (checked, missed)
}

fn measure(regions: &[Region2], a: &AdaptivePath, r: f64, top: f64) -> crate::engagement::Engagement {
    engagement(&a.path, Some(&a.roles), r, &Material { regions: regions.to_vec(), top, within: None }, 0.1).unwrap()
}

fn check_pocket(regions: &[Region2], p: &AdaptiveParams) -> AdaptivePath {
    let tool = flat(6.0);
    let c = cut(0.0, -3.0);
    let a = adaptive(regions, &tool, p, &c).unwrap();
    verify(&a.path, &c.heights).unwrap();
    assert_eq!(a.roles.len(), a.path.moves.len());
    assert!(a.helices >= 1, "{a:?}");
    let e = measure(regions, &a, 3.0, 0.0);
    let limit = p.max_engagement / DEG;
    assert!(
        e.max_deg <= limit + ENGAGEMENT_TOL,
        "engagement {} above {limit}: {e:?} {:?} {:?}",
        e.max_deg,
        e.worst_move.map(|i| (a.roles[i], a.path.moves[i - 1], a.path.moves[i])),
        &a.path.moves[e.worst_move.unwrap_or(0).saturating_sub(6)..e.worst_move.unwrap_or(0) + 1]
    );
    assert_eq!(e.full_slot_share, 0.0, "{e:?}");
    assert!(e.mean_deg > 10.0, "{e:?}");
    assert_cleared(regions, &a.path, 3.0, p.stock_to_leave, -3.0, 0.4);
    a
}

#[test]
fn a_square_pocket_stays_within_the_engagement() {
    let a = check_pocket(&[square(30.0)], &params());
    // Entries are helices: arcs that go down.
    assert!(a.path.moves.iter().zip(&a.roles).any(|(m, r)| *r == MoveRole::Entry && matches!(m, Move::Arc { .. })));
}

#[test]
fn refreshing_the_distances_works_out_only_the_cells_it_keeps() {
    // The distances are brought up to date round what a walk cleared from the material in a
    // window twice as wide: along every row of the window, but down only the columns it keeps
    // (worked out down all of them, about two distances per cell of the window).
    let (_, kept, windows) = crate::adaptive::distances_in(|| check_pocket(&[square(30.0)], &params()));
    assert!(windows > 0);
    assert!((kept as f64) < 1.6 * windows as f64, "{kept} distances worked out for windows of {windows} cells");
}

#[test]
fn a_distance_window_cut_down_is_the_whole_window_cut_down() {
    // Sites scattered over the raster, some rows and columns without any.
    let site = |c: usize| (c * 2_654_435_761) % 97 < 3 && !c.is_multiple_of(7);
    for (win, inner) in [
        ((0, 41, 0, 33), (0, 41, 0, 33)),
        ((0, 41, 0, 33), (10, 30, 5, 20)),
        ((3, 40, 2, 30), (3, 4, 29, 30)),
        ((5, 25, 6, 26), (5, 25, 6, 7)),
        ((5, 25, 6, 26), (24, 25, 6, 26)),
    ] {
        assert!(crate::adaptive::window_matches_whole(40, 32, &site, win, inner).unwrap(), "{win:?} {inner:?}");
    }
}

#[test]
fn an_l_shaped_pocket_stays_within_the_engagement() {
    check_pocket(&[ell()], &params());
}

#[test]
fn a_pocket_with_an_island_stays_within_the_engagement() {
    let a = check_pocket(&[with_island()], &params());
    // Nothing comes within the radius of the island.
    for (p, cutting) in a.path.points(1e-3) {
        if cutting {
            let (dx, dy) = ((14.0 - p.x).max(p.x - 26.0).max(0.0), (10.0 - p.y).max(p.y - 20.0).max(0.0));
            assert!(dx.hypot(dy) >= 3.0 - 1e-6, "{p:?}");
        }
    }
}

#[test]
fn conventional_milling_and_stock_to_leave_still_clear_the_pocket() {
    let p = AdaptiveParams { climb: false, stock_to_leave: 0.5, ..params() };
    check_pocket(&[square(24.0)], &p);
}

/// Plain offset pocketing slots its first ring; the adaptive path never does.
#[test]
fn adaptive_against_offset_pocketing() {
    let regions = [square(30.0)];
    let tool = flat(6.0);
    let c = cut(0.0, -3.0);
    let a = adaptive(&regions, &tool, &params(), &c).unwrap();
    let pp = PocketParams { stepover: 2.4, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    let plain = pocket(&regions, &tool, &pp, &c).unwrap();
    let material = Material { regions: regions.to_vec(), top: 0.0, within: None };
    let ea = engagement(&a.path, Some(&a.roles), 3.0, &material, 0.1).unwrap();
    let ep = engagement(&plain, None, 3.0, &material, 0.1).unwrap();
    assert!(ep.max_deg >= 170.0 && ep.full_slot_share > 0.0, "{ep:?}");
    assert_eq!(ea.full_slot_share, 0.0, "{ea:?}");
    assert!(ea.max_deg < ep.max_deg, "{ea:?} {ep:?}");
    // A lighter cut: a longer path.
    assert!(a.path.stats().cut_length > plain.stats().cut_length, "{} {}", a.path.stats().cut_length, plain.stats().cut_length);
}

/// Climb milling keeps the material on the right of the travel (spindle clockwise): the helices
/// go counter-clockwise and the walks round the cleared area turn counter-clockwise overall;
/// conventional milling the other way round, and the engagement holds either way.
#[test]
fn climb_and_conventional_turn_opposite_ways() {
    for climb in [true, false] {
        let p = AdaptiveParams { climb, ..params() };
        let a = adaptive(&[square(24.0)], &flat(6.0), &p, &cut(0.0, -1.0)).unwrap();
        let mut arcs = 0;
        for (m, role) in a.path.moves.iter().zip(&a.roles) {
            if let (Move::Arc { ccw, .. }, MoveRole::Entry) = (m, role) {
                assert_eq!(*ccw, climb, "a helix the wrong way round, climb {climb}");
                arcs += 1;
            }
        }
        assert!(arcs >= 2, "{arcs}");
        // The signed turn between consecutive cutting moves, summed.
        let mut turn = 0.0;
        let mut prev: Option<(Vec3, Option<Vec2>)> = None;
        for (m, role) in a.path.moves.iter().zip(&a.roles) {
            let Some(to) = m.target() else { continue };
            let dir = match (prev, role) {
                (Some((from, _)), MoveRole::Cut) => (Vec2::new(to.x, to.y) - Vec2::new(from.x, from.y)).normalized(),
                _ => None,
            };
            if let (Some((_, Some(d0))), Some(d1)) = (prev, dir) {
                turn += d0.cross(d1).atan2(d0.dot(d1));
            }
            prev = Some((to, dir));
        }
        let turns = turn / (2.0 * PI);
        assert!(if climb { turns > 1.0 } else { turns < -1.0 }, "climb {climb}: {turns} turns");
        let e = measure(&[square(24.0)], &a, 3.0, 0.0);
        assert!(e.max_deg <= 60.0 + ENGAGEMENT_TOL && e.full_slot_share == 0.0, "climb {climb}: {e:?}");
    }
}

/// A limit well below the load's engagement (a heavy load capped at 30°): the walk's steps and
/// raster follow the width the limit allows, not the load, so the pocket is cleared from its
/// helix within the limit.
#[test]
fn a_low_limit_under_a_heavy_load_still_clears_the_pocket() {
    let regions = [square(14.0)];
    let p = AdaptiveParams { optimal_load: 1.8, min_engagement: 3.0 * DEG, max_engagement: 30.0 * DEG, ..params() };
    let a = adaptive(&regions, &flat(6.0), &p, &cut(0.0, -1.0)).unwrap();
    assert_eq!(a.helices, 1, "{a:?}");
    let e = measure(&regions, &a, 3.0, 0.0);
    assert!(e.max_deg <= 30.0 + ENGAGEMENT_TOL, "{e:?}");
    assert_eq!(e.full_slot_share, 0.0, "{e:?}");
    assert_cleared(&regions, &a.path, 3.0, 0.0, -1.0, 0.4);
}

#[test]
fn the_helix_is_refused_without_room() {
    // 7 mm wide: the 6 mm tool's centre has a 1 mm strip, no room for a 4.8 mm helix.
    let slot = region(&[(0.0, 0.0), (40.0, 0.0), (40.0, 7.0), (0.0, 7.0)], &[]);
    let e = adaptive(std::slice::from_ref(&slot), &flat(6.0), &params(), &cut(0.0, -3.0)).unwrap_err();
    assert!(matches!(&e, CamError::Invalid(m) if m.contains("no room for the entry helix")), "{e}");
    // Not even the smallest helix the tool takes: the refusal names it, not a room it then refuses.
    assert!(matches!(&e, CamError::Invalid(m) if m.contains("nor for one of 1.500 mm")), "{e}");
    // 10 mm wide: no room for the 4.8 mm helix asked for. The largest that fits the 4 mm strip
    // the centre has (less a cell of the raster) is used instead, and said.
    let wide = region(&[(0.0, 0.0), (40.0, 0.0), (40.0, 10.0), (0.0, 10.0)], &[]);
    let a = adaptive(std::slice::from_ref(&wide), &flat(6.0), &params(), &cut(0.0, -1.0)).unwrap();
    assert!(a.helices >= 1, "{a:?}");
    let said = a.warnings.iter().find(|w| w.contains("entry helix of diameter 4.800 mm had no room")).unwrap_or_else(|| panic!("{:?}", a.warnings));
    let used: f64 = said.split("helices down to ").nth(1).and_then(|r| r.split(' ').next()).and_then(|d| d.parse().ok()).unwrap();
    assert!((3.0..=4.0).contains(&used), "{said}");
    // Every helix is within the room: the path's own check, and the engagement holds.
    let e = measure(std::slice::from_ref(&wide), &a, 3.0, 0.0);
    assert!(e.max_deg <= 60.0 + ENGAGEMENT_TOL, "{e:?}");
    // The helix it says it used plans when asked for.
    let p = AdaptiveParams { helix_diameter: used, ..params() };
    let again = adaptive(std::slice::from_ref(&wide), &flat(6.0), &p, &cut(0.0, -1.0)).unwrap();
    assert!(!again.warnings.iter().any(|w| w.contains("had no room")), "{:?}", again.warnings);
    let p = AdaptiveParams { helix_diameter: 2.0, ..params() };
    let a = adaptive(std::slice::from_ref(&wide), &flat(6.0), &p, &cut(0.0, -1.0)).unwrap();
    assert!(a.helices >= 1);
    let e = measure(&[wide], &a, 3.0, 0.0);
    assert!(e.max_deg <= 60.0 + ENGAGEMENT_TOL, "{e:?}");
    // A pocket the tool does not fit at all.
    assert!(adaptive(&[square(5.0)], &flat(6.0), &params(), &cut(0.0, -1.0)).is_err());
}

/// Where the centre may stand and the cells inside the regions are rasterised from one exact
/// distance per stretch of a row the boundary is far from, with the exact tests' answers at
/// every centre: on a pocket with round corners and a round island, under half the centres are
/// measured (each was before), those near the walls.
#[test]
fn the_raster_of_a_pocket_measures_few_centres_and_matches_the_exact_tests() {
    let v = Vec2::new;
    let (r, s) = (5.0, 60.0);
    let arc = |cx: f64, cy: f64, start: f64| Seg2::Arc { center: v(cx, cy), radius: r, start, sweep: PI / 2.0 };
    let outer = Loop2 {
        segs: vec![
            Seg2::Line { a: v(r, 0.0), b: v(s - r, 0.0) },
            arc(s - r, r, -PI / 2.0),
            Seg2::Line { a: v(s, r), b: v(s, s - r) },
            arc(s - r, s - r, 0.0),
            Seg2::Line { a: v(s - r, s), b: v(r, s) },
            arc(r, s - r, PI / 2.0),
            Seg2::Line { a: v(0.0, s - r), b: v(0.0, r) },
            arc(r, r, PI),
        ],
    };
    let island = Loop2 { segs: vec![Seg2::Arc { center: v(30.0, 30.0), radius: 8.0, start: 0.0, sweep: -TAU }] };
    let regions = [Region2 { outer, holes: vec![island] }];
    for (h, wall) in [(0.1, 2.2), (0.37, 3.1), (0.05, 0.6)] {
        let (cells, measured, wrong) = crate::adaptive::shape_raster_against_exact(&regions, h, wall).expect("rasterised");
        assert_eq!(wrong, 0, "cell {h}, wall {wall}: {wrong} centres differ from the exact tests");
        assert!(measured * 2 < cells, "cell {h}, wall {wall}: {measured} of {cells} centres measured");
    }
}

#[test]
fn hostile_parameters_are_refused() {
    let tool = flat(6.0);
    let regions = [square(20.0)];
    let c = cut(0.0, -1.0);
    let bad = [
        AdaptiveParams { optimal_load: 0.0, ..params() },
        AdaptiveParams { optimal_load: f64::NAN, ..params() },
        AdaptiveParams { optimal_load: 7.0, ..params() },
        AdaptiveParams { max_engagement: 0.0, ..params() },
        AdaptiveParams { max_engagement: 4.0, ..params() },
        AdaptiveParams { max_engagement: f64::NAN, ..params() },
        AdaptiveParams { min_engagement: 70.0 * DEG, ..params() },
        AdaptiveParams { min_engagement: -1.0, ..params() },
        AdaptiveParams { stock_to_leave: -1.0, ..params() },
        AdaptiveParams { stock_to_leave: f64::INFINITY, ..params() },
        AdaptiveParams { tolerance: 0.0, ..params() },
        AdaptiveParams { helix_diameter: 0.0, ..params() },
        AdaptiveParams { helix_diameter: 6.5, ..params() },
        AdaptiveParams { ramp_angle: 0.0, ..params() },
        AdaptiveParams { ramp_angle: 1.0, ..params() },
        AdaptiveParams { ramp_angle: f64::NAN, ..params() },
    ];
    for p in &bad {
        assert!(adaptive(&regions, &tool, p, &c).is_err(), "{p:?}");
    }
    let drill = Tool { kind: ToolKind::Drill, ..flat(6.0) };
    assert!(adaptive(&regions, &drill, &params(), &c).is_err());
    assert!(adaptive(&regions, &flat(f64::NAN), &params(), &c).is_err());
    assert!(adaptive(&[], &tool, &params(), &c).is_err());
    let nan = region(&[(0.0, 0.0), (f64::NAN, 0.0), (10.0, 10.0)], &[]);
    assert!(adaptive(&[nan], &tool, &params(), &c).is_err());
    let flat_region = region(&[(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)], &[]);
    assert!(adaptive(&[flat_region], &tool, &params(), &c).is_err());
    for h in [
        Heights { clearance: 1.0, retract: 2.0, top: 0.0, bottom: -1.0 },
        Heights { clearance: 20.0, retract: 2.0, top: 0.0, bottom: f64::NAN },
        Heights { clearance: 20.0, retract: 2.0, top: 0.0, bottom: -1e9 },
    ] {
        assert!(adaptive(&regions, &tool, &params(), &Cut { heights: h, ..c }).is_err(), "{h:?}");
    }
    assert!(adaptive(&regions, &tool, &params(), &Cut { stepdown: 0.0, ..c }).is_err());
    // A pass deeper than the flutes reach (20 mm) is refused; a stepdown larger than the whole
    // depth is not.
    let deep = Cut { heights: Heights { bottom: -25.0, ..c.heights }, stepdown: 25.0, ..c };
    let e = adaptive(&regions, &tool, &params(), &deep).unwrap_err();
    assert!(e.to_string().contains("flute length"), "{e}");
    assert!(adaptive(&regions, &tool, &params(), &Cut { stepdown: 50.0, ..c }).is_ok());
    assert!(adaptive(&regions, &tool, &params(), &Cut { feeds: Feeds { cut: 0.0, plunge: 1.0 }, ..c }).is_err());
}

/// The cell visits and drop-cutter samples `f` took, its work split over threads kept on this
/// one so that it is all counted: the tests' measure of a clearing's work, the same on any
/// machine under any load.
fn work<R>(f: impl FnOnce() -> R) -> (R, f64, usize) {
    crate::par::serially(|| {
        let ((r, drops), visits) = crate::adaptive::visits_in(|| crate::surface_ops::drops_in(f));
        (r, visits, drops)
    })
}

/// Huge regions are refused before any work, not left running.
#[test]
fn huge_regions_are_refused_quickly() {
    let (_, visits, drops) = work(|| {
        let e = adaptive(&[square(100_000.0)], &flat(6.0), &params(), &cut(0.0, -1.0)).unwrap_err();
        assert!(matches!(e, CamError::TooLarge(_)), "{e}");
        let e = adaptive(&[square(5.0e6)], &flat(6.0), &params(), &cut(0.0, -1.0)).unwrap_err();
        assert!(matches!(e, CamError::Invalid(_) | CamError::TooLarge(_)), "{e}");
        let surface = Surface::new(plate_and_boss()).unwrap();
        let mut p = roughing_params();
        p.boundary.rect = Some([Vec2::new(-50_000.0, -50_000.0), Vec2::new(50_000.0, 50_000.0)]);
        assert!(matches!(adaptive_roughing(&surface, &flat(6.0), &p).unwrap_err(), CamError::TooLarge(_)));
    });
    assert!(visits <= 0.0 && drops == 0, "{visits} cell visits, {drops} drops before refusing");
}

/// The triangles of a box, wound outward.
fn box_tris(lo: Vec3, hi: Vec3, out: &mut Vec<[Vec3; 3]>) {
    let c = |i: usize| Vec3::new(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z });
    let quads = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    for q in quads {
        out.push([c(q[0]), c(q[1]), c(q[2])]);
        out.push([c(q[0]), c(q[2]), c(q[3])]);
    }
}

/// A 40 mm square plate, its top at Z 0, with a 12 mm square boss 6 mm high in the middle.
fn plate_and_boss() -> Vec<[Vec3; 3]> {
    let mut t = Vec::new();
    box_tris(Vec3::new(-20.0, -20.0, -10.0), Vec3::new(20.0, 20.0, 0.0), &mut t);
    box_tris(Vec3::new(-6.0, -6.0, 0.0), Vec3::new(6.0, 6.0, 6.0), &mut t);
    t
}

fn roughing_params() -> AdaptiveRoughingParams {
    AdaptiveRoughingParams {
        heights: Heights { clearance: 30.0, retract: 8.0, top: 6.0, bottom: 0.25 },
        feeds: Feeds { cut: 1000.0, plunge: 200.0 },
        stepdown: 2.5,
        boundary: Boundary::default(),
        flats: true,
        adaptive: AdaptiveParams { stock_to_leave: 0.25, ..params() },
        rest: None,
        step_up: None,
    }
}

/// How much stock a roughing path leaves above the surface along Y 0 (the most, mm), measured
/// from the cutting moves' segments a flat end of radius `r` sweeps.
fn most_left_along_x(a: &AdaptivePath, surface: &Surface, r: f64, top: f64, leave: f64) -> f64 {
    let pts = a.path.points(1e-3);
    let segs: Vec<(Vec3, Vec3)> = pts.windows(2).filter(|w| w[1].1).map(|w| (w[0].0, w[1].0)).collect();
    let tip = Cutter::flat(1e-3).unwrap();
    let mut most = 0.0f64;
    for i in 0..=30 {
        let q = Vec2::new(-15.0 + i as f64, 0.0);
        let mut stock = top;
        for (a, b) in &segs {
            let (a2, b2) = (Vec2::new(a.x, a.y), Vec2::new(b.x, b.y));
            let d = b2 - a2;
            let t = if d.len2() > 0.0 { ((q - a2).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
            if q.dist(a2 + d * t) <= r - 0.05 {
                stock = stock.min(a.z.max(b.z));
            }
        }
        let floor = drop_z(surface, &tip, q.x, q.y).unwrap_or(top);
        most = most.max(stock - floor - leave);
    }
    most
}

/// A plate whose top slopes gently (about 9 degrees) from Z 0 at X -20 to Z 6 at X 20, on a base.
fn ramp() -> Surface {
    let mut t = Vec::new();
    box_tris(Vec3::new(-20.0, -20.0, -10.0), Vec3::new(20.0, 20.0, 0.0), &mut t);
    let (a, b, c, d) = (Vec3::new(-20.0, -20.0, 0.0), Vec3::new(20.0, -20.0, 6.0), Vec3::new(20.0, 20.0, 6.0), Vec3::new(-20.0, 20.0, 0.0));
    t.push([a, b, c]);
    t.push([a, c, d]);
    Surface::new(t).unwrap()
}

/// Regular levels a full stepdown apart leave terraces as high as the stepdown on shallow
/// ground; step-up levels between them cut those down to at most the step-up (and what the
/// slope rises under the tool's radius), so a ball finish meets no tall steps.
#[test]
fn step_up_levels_cut_the_terraces_on_shallow_ground() {
    let surface = ramp();
    let tool = flat(6.0);
    let base = AdaptiveRoughingParams {
        heights: Heights { clearance: 30.0, retract: 8.0, top: 6.0, bottom: 0.25 },
        stepdown: 6.0,
        flats: false,
        boundary: Boundary { rect: Some([Vec2::new(-20.0, -20.0), Vec2::new(20.0, 20.0)]), region: None },
        ..roughing_params()
    };
    let plain = adaptive_roughing(&surface, &tool, &base).unwrap();
    let left = most_left_along_x(&plain, &surface, 3.0, 6.0, 0.25);
    assert!(left > 4.0, "without step-up levels a tall terrace is left: {left}");
    let p = AdaptiveRoughingParams { step_up: Some(1.0), ..base };
    let a = adaptive_roughing(&surface, &tool, &p).unwrap();
    verify(&a.path, &p.heights).unwrap();
    assert!(a.levels > plain.levels, "{} vs {}", a.levels, plain.levels);
    let left = most_left_along_x(&a, &surface, 3.0, 6.0, 0.25);
    // The step-up, and the slope's rise under the tool's radius (0.45 mm).
    assert!(left <= 1.0 + 0.45 + 0.1, "{left}");
    let rest = Cutter::flat(3.0).unwrap().grown(0.25).unwrap();
    for (q, cutting) in a.path.points(1e-3) {
        if cutting && let Some(z) = drop_z(&surface, &rest, q.x, q.y) {
            assert!(q.z >= z + 0.25 - 1e-6, "{q:?}");
        }
    }
    // Hostile step-ups are refused.
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(adaptive_roughing(&surface, &tool, &AdaptiveRoughingParams { step_up: Some(bad), ..p.clone() }).is_err(), "{bad}");
    }
}

/// A step-up level enters only from the area the levels so far cleared: a recess whose floor lies
/// between two levels, enclosed by walls and too narrow for any helix the tool takes, is left with
/// a note, not a failure of the whole roughing (a domed knob's roughing failed so and its
/// finishes were dropped with it).
#[test]
fn a_step_up_level_that_cannot_enter_a_recess_leaves_it_with_a_note() {
    let mut t = Vec::new();
    box_tris(Vec3::new(-20.0, -20.0, -10.0), Vec3::new(20.0, 20.0, 0.0), &mut t);
    // A block 6 mm high with a recess 7.5 mm wide, its floor at Z 2: the 6 mm tool's centre has a
    // strip of about half a millimetre in it, no room for even the smallest helix (1.5 mm).
    for (lo, hi) in [((-10.0, -10.0), (-3.75, 10.0)), ((3.75, -10.0), (10.0, 10.0)), ((-3.75, -10.0), (3.75, -8.0)), ((-3.75, 8.0), (3.75, 10.0))] {
        box_tris(Vec3::new(lo.0, lo.1, 0.0), Vec3::new(hi.0, hi.1, 6.0), &mut t);
    }
    box_tris(Vec3::new(-3.75, -8.0, 0.0), Vec3::new(3.75, 8.0, 2.0), &mut t);
    let surface = Surface::new(t).unwrap();
    let p = AdaptiveRoughingParams {
        heights: Heights { clearance: 30.0, retract: 8.0, top: 6.0, bottom: 0.25 },
        stepdown: 6.0,
        flats: false,
        boundary: Boundary { rect: Some([Vec2::new(-20.0, -20.0), Vec2::new(20.0, 20.0)]), region: None },
        step_up: Some(1.0),
        ..roughing_params()
    };
    let a = adaptive_roughing(&surface, &flat(6.0), &p).unwrap();
    verify(&a.path, &p.heights).unwrap();
    // The regular level enters from open air, the step-up levels from what it cleared: no helix.
    assert_eq!(a.helices, 0, "{a:?}");
    // The block's walls are upright: the recess is all the step-up levels find, and none cuts.
    assert_eq!(a.levels, 1, "{a:?}");
    assert!(a.warnings.iter().any(|w| w.contains("could not enter from the area already cleared")), "{:?}", a.warnings);
    let rest = Cutter::flat(3.0).unwrap().grown(0.25).unwrap();
    for (q, cutting) in a.path.points(1e-3) {
        if cutting && let Some(z) = drop_z(&surface, &rest, q.x, q.y) {
            assert!(q.z >= z + 0.25 - 1e-6, "{q:?}");
        }
    }
}

/// A stock an earlier setup left, known point by point (`solid`): where that setup cut a slot
/// under other material (none between the second level and just above the first past X 0), the
/// second level finds only air there and does not cut it; the levels with material between them
/// and the level above still do.
#[test]
fn adaptive_roughing_does_not_cut_the_air_an_earlier_setup_left() {
    let surface = Surface::new(plate_and_boss()).unwrap();
    let tool = flat(6.0);
    let p = AdaptiveRoughingParams {
        boundary: Boundary { rect: Some([Vec2::new(-20.0, -20.0), Vec2::new(20.0, 20.0)]), region: None },
        flats: false,
        ..roughing_params()
    };
    // Cutting points of the level at `z` past the boss and the tool's radius on the slot's side.
    let past = |a: &AdaptivePath, z: f64| a.path.points(1e-3).iter().filter(|(q, cutting)| *cutting && (q.z - z).abs() < 1e-6 && q.x > 9.5).count();
    let whole = adaptive_roughing(&surface, &tool, &p).unwrap();
    let mut levels: Vec<f64> = Vec::new();
    for (q, cutting) in whole.path.points(1e-3) {
        if cutting && !levels.iter().any(|z| (z - q.z).abs() < 1e-6) {
            levels.push(q.z);
        }
    }
    levels.sort_by(|a, b| b.total_cmp(a));
    let [first, second, third] = levels[..] else { panic!("three levels: {levels:?}") };
    assert!(past(&whole, second) > 0, "the whole stock is cut at the second level past X 9.5");
    let slot = |q: Vec3| !(q.x > 0.0 && q.z > second && q.z < first + 0.05);
    let a = adaptive_roughing_in(&surface, &tool, &p, Some(&slot)).unwrap();
    verify(&a.path, &p.heights).unwrap();
    assert_eq!(past(&a, second), 0, "the slot's air is cut at Z {second}");
    assert!(past(&a, first) > 0, "the material above the slot is left at Z {first}");
    assert!(past(&a, third) > 0, "the material below the slot is left at Z {third}");
    // Material everywhere: the same path as the whole stock's.
    let full = adaptive_roughing_in(&surface, &tool, &p, Some(&|_: Vec3| true)).unwrap();
    assert_eq!(full.path.points(1e-3).len(), whole.path.points(1e-3).len());
}

#[test]
fn adaptive_roughing_stays_off_the_part_and_within_the_engagement() {
    let surface = Surface::new(plate_and_boss()).unwrap();
    let tool = flat(6.0);
    let p = roughing_params();
    let a = adaptive_roughing(&surface, &tool, &p).unwrap();
    verify(&a.path, &p.heights).unwrap();
    assert!(a.levels >= 2, "{a:?}");
    // Entered from the open air round the stock: no helix needed.
    assert_eq!(a.helices, 0);
    // The stock was taken from the part, which a larger blank would make wrong: said so.
    assert!(a.warnings.iter().any(|w| w.contains("the part's extent")), "{:?}", a.warnings);
    let rest = Cutter::flat(3.0).unwrap().grown(0.25).unwrap();
    let mut lowest = f64::INFINITY;
    for (q, cutting) in a.path.points(1e-3) {
        if cutting {
            lowest = lowest.min(q.z);
            if let Some(z) = drop_z(&surface, &rest, q.x, q.y) {
                assert!(q.z >= z + 0.25 - 1e-6, "{q:?} below {}", z + 0.25);
            }
        }
    }
    assert!((lowest - 0.25).abs() < 1e-6, "{lowest}");
    let stock = Material { regions: vec![region(&[(-20.0, -20.0), (20.0, -20.0), (20.0, 20.0), (-20.0, 20.0)], &[])], top: 6.0, within: None };
    let e = engagement(&a.path, Some(&a.roles), 3.0, &stock, 0.1).unwrap();
    assert!(e.max_deg <= 60.0 + ENGAGEMENT_TOL, "{e:?}");
    assert_eq!(e.full_slot_share, 0.0, "{e:?}");
    // The plate's top is cleared down to the stock to leave out to the boss.
    let bottom: Vec<(Vec2, Vec2)> = a
        .path
        .points(1e-3)
        .windows(2)
        .filter(|w| w[1].1 && (w[0].0.z - 0.25).abs() < 1e-6 && (w[1].0.z - 0.25).abs() < 1e-6)
        .map(|w| (Vec2::new(w[0].0.x, w[0].0.y), Vec2::new(w[1].0.x, w[1].0.y)))
        .collect();
    for q in [Vec2::new(-15.0, -15.0), Vec2::new(15.0, 0.0), Vec2::new(0.0, 12.0), Vec2::new(-19.5, 19.5), Vec2::new(-9.6, 0.0)] {
        let near = bottom.iter().any(|(a, b)| {
            let d = *b - *a;
            let t = if d.len2() > 0.0 { ((q - *a).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
            q.dist(*a + d * t) <= 3.0
        });
        assert!(near, "{q:?} not cleared");
    }
}

/// The regular levels of an adaptive roughing are planned side by side over the cores, the
/// step-up levels after them in order and each helix's start from the levels above: the same
/// roughing as planned in one thread, helices and step-up levels included.
#[test]
fn adaptive_roughing_over_the_cores_is_the_one_planned_in_one_thread() {
    let mut t = Vec::new();
    box_tris(Vec3::new(-20.0, -20.0, -5.0), Vec3::new(20.0, 20.0, 0.0), &mut t);
    for (lo, hi) in [((-20.0, -20.0), (20.0, -12.0)), ((-20.0, 12.0), (20.0, 20.0)), ((-20.0, -12.0), (-12.0, 12.0)), ((12.0, -12.0), (20.0, 12.0))] {
        box_tris(Vec3::new(lo.0, lo.1, 0.0), Vec3::new(hi.0, hi.1, 6.0), &mut t);
    }
    let cavity = Surface::new(t).unwrap();
    let in_cavity = AdaptiveRoughingParams {
        boundary: Boundary { rect: Some([Vec2::new(-12.0, -12.0), Vec2::new(12.0, 12.0)]), region: None },
        stepdown: 1.5,
        ..roughing_params()
    };
    let on_ramp = AdaptiveRoughingParams {
        heights: Heights { clearance: 30.0, retract: 8.0, top: 6.0, bottom: 0.25 },
        stepdown: 2.0,
        flats: false,
        step_up: Some(0.5),
        boundary: Boundary { rect: Some([Vec2::new(-20.0, -20.0), Vec2::new(20.0, 20.0)]), region: None },
        ..roughing_params()
    };
    for (surface, p) in [(&cavity, &in_cavity), (&ramp(), &on_ramp)] {
        let side_by_side = adaptive_roughing(surface, &flat(6.0), p).unwrap();
        let one = crate::par::serially(|| adaptive_roughing(surface, &flat(6.0), p)).unwrap();
        assert!(side_by_side.levels >= 3, "{side_by_side:?}");
        assert_eq!(side_by_side, one);
    }
    // Helices below the first level start where the level above left material over them.
    let a = adaptive_roughing(&cavity, &flat(6.0), &in_cavity).unwrap();
    assert!(a.helices >= 2, "{a:?}");
}

/// A closed cavity in the part: the material is entered with a helix, and the cavity's walls
/// are respected.
#[test]
fn adaptive_roughing_enters_an_enclosed_cavity_with_a_helix() {
    let mut t = Vec::new();
    // A frame round a 24 mm square cavity, its floor at Z 0.
    box_tris(Vec3::new(-20.0, -20.0, -5.0), Vec3::new(20.0, 20.0, 0.0), &mut t);
    for (lo, hi) in [((-20.0, -20.0), (20.0, -12.0)), ((-20.0, 12.0), (20.0, 20.0)), ((-20.0, -12.0), (-12.0, 12.0)), ((12.0, -12.0), (20.0, 12.0))] {
        box_tris(Vec3::new(lo.0, lo.1, 0.0), Vec3::new(hi.0, hi.1, 6.0), &mut t);
    }
    let surface = Surface::new(t).unwrap();
    let p = AdaptiveRoughingParams {
        boundary: Boundary { rect: Some([Vec2::new(-12.0, -12.0), Vec2::new(12.0, 12.0)]), region: None },
        ..roughing_params()
    };
    let a = adaptive_roughing(&surface, &flat(6.0), &p).unwrap();
    assert!(a.helices >= 1, "{a:?}");
    // The stock was given: no warning about it.
    assert!(!a.warnings.iter().any(|w| w.contains("the part's extent")), "{:?}", a.warnings);
    let rest = Cutter::flat(3.0).unwrap().grown(0.25).unwrap();
    for (q, cutting) in a.path.points(1e-3) {
        if cutting && let Some(z) = drop_z(&surface, &rest, q.x, q.y) {
            assert!(q.z >= z + 0.25 - 1e-6, "{q:?}");
        }
    }
}

#[test]
fn adaptive_roughing_refuses_hostile_input() {
    let surface = Surface::new(plate_and_boss()).unwrap();
    let tool = flat(6.0);
    let mut p = roughing_params();
    p.heights.retract = 4.0;
    assert!(adaptive_roughing(&surface, &tool, &p).is_err(), "retract below the part");
    let mut p = roughing_params();
    p.boundary.rect = Some([Vec2::new(5.0, 5.0), Vec2::new(-5.0, -5.0)]);
    assert!(adaptive_roughing(&surface, &tool, &p).is_err());
    p.boundary.rect = Some([Vec2::new(f64::NAN, 5.0), Vec2::new(5.0, 6.0)]);
    assert!(adaptive_roughing(&surface, &tool, &p).is_err());
    let p = AdaptiveRoughingParams { stepdown: 0.0, ..roughing_params() };
    assert!(adaptive_roughing(&surface, &tool, &p).is_err());
    let p = AdaptiveRoughingParams { adaptive: AdaptiveParams { optimal_load: -1.0, ..params() }, ..roughing_params() };
    assert!(adaptive_roughing(&surface, &tool, &p).is_err());
    // Levels 1.9 mm apart with flutes 1.5 mm long.
    let short = Tool { flute_length: 1.5, ..tool.clone() };
    let e = adaptive_roughing(&surface, &short, &roughing_params()).unwrap_err();
    assert!(e.to_string().contains("flute length"), "{e}");
    let empty = Surface::new(vec![]).unwrap();
    assert!(adaptive_roughing(&empty, &tool, &roughing_params()).is_err());
}

/// Adaptive clearing takes the shared milling-tool rule: it enters along a helix, so a face mill
/// may clear, but a chamfer mill, a drill and a thread mill are refused.
#[test]
fn adaptive_clearing_refuses_tools_that_cannot_clear() {
    let mut face = Tool::new(1, ToolKind::FaceMill, 6.0, 20.0);
    face.corner_radius = Some(0.5);
    assert!(adaptive(&[square(24.0)], &face, &params(), &cut(0.0, -1.0)).is_ok());
    let mut chamfer = Tool::new(7, ToolKind::Chamfer, 6.0, 10.0);
    chamfer.angle_deg = Some(90.0);
    let refused = [
        (chamfer, "a chamfer mill's side is tapered"),
        (Tool::new(3, ToolKind::Drill, 6.0, 30.0), "a drill cannot mill"),
        (Tool::new(5, ToolKind::ThreadMill, 6.0, 12.0), "a thread mill cuts threads only"),
    ];
    for (t, why) in refused {
        let e = adaptive(&[square(24.0)], &t, &params(), &cut(0.0, -1.0)).unwrap_err();
        assert!(e.to_string().contains(why), "{:?}: {e}", t.kind);
    }
}

/// The work an adaptive roughing may take is bounded level by level, each level's bound scaled
/// to its raster and to the walks its area and load call for, not to how many levels there are,
/// so a larger stepdown (fewer levels) is never refused where a smaller one is planned. A job
/// that would take minutes is refused at once, with the numbers and what to change.
#[test]
fn adaptive_roughing_work_is_bounded_per_level_and_refused_at_once_when_too_large() {
    use crate::adaptive::{MAX_ROUGHING_WORK, TYPICAL_LEVEL_WORK_PER_CELL, check_roughing_work, level_work_limit};
    let cells = 400_000;
    let (steps, step_work) = (20_000.0, 4000.0);
    let hint = || "a hint".to_string();
    // Monotonic in the number of levels: once refused, refused for every larger number.
    let first = (1..=200).find(|&n| check_roughing_work(n, cells, steps, step_work, &hint).is_err()).unwrap();
    assert!((first..=200).all(|n| check_roughing_work(n, cells, steps, step_work, &hint).is_err()));
    assert!((1..first).all(|n| check_roughing_work(n, cells, steps, step_work, &hint).is_ok()));
    assert!(first as f64 * (cells as f64 * TYPICAL_LEVEL_WORK_PER_CELL + steps * step_work) > MAX_ROUGHING_WORK);
    // A 2 mm stepdown down 20 mm of that raster is planned.
    assert!(check_roughing_work(11, cells, steps, step_work, &hint).is_ok());
    // The level's own bound does not shrink with more levels; it grows with the raster and with
    // the steps its walks are expected to take.
    assert!(level_work_limit(2 * cells, steps, step_work) > level_work_limit(cells, steps, step_work));
    assert!(level_work_limit(cells, 2.0 * steps, step_work) > level_work_limit(cells, steps, step_work));
    // Refused at once, naming the fix: before any level's work or drop.
    let surface = Surface::new(plate_and_boss()).unwrap();
    let mut p = roughing_params();
    p.heights.top = 600.0;
    p.heights.retract = 610.0;
    p.heights.clearance = 620.0;
    p.stepdown = 0.5;
    p.boundary.rect = Some([Vec2::new(-60.0, -60.0), Vec2::new(60.0, 60.0)]);
    let (e, visits, drops) = work(|| adaptive_roughing(&surface, &Tool { flute_length: 40.0, ..flat(6.0) }, &p).unwrap_err());
    assert!(matches!(e, CamError::TooLarge(_)) && e.to_string().contains("larger stepdown"), "{e}");
    assert!(visits <= 0.0 && drops == 0, "{visits} cell visits, {drops} drops before refusing");
    // A plate round a boss at a 1 mm stepdown, more levels than the old bound on the whole
    // operation let through, is planned.
    let mut p = roughing_params();
    p.stepdown = 1.0;
    let a = adaptive_roughing(&surface, &flat(6.0), &p).unwrap();
    assert!(a.levels >= 6, "{a:?}");
}

/// A 6 mm end mill's adaptive parameters at `load` mm, the engagement limits following the load
/// (the largest one and a half times the load's, the smallest a third of it).
fn light(load: f64) -> AdaptiveParams {
    let opt = (1.0 - load / 3.0f64).acos();
    let most = (opt * 1.5).max(opt + 15.0 * DEG).min(PI);
    AdaptiveParams { optimal_load: load, min_engagement: (opt / 3.0).min(most * 0.5), max_engagement: most, ..params() }
}

/// A 60 x 50 mm plate 5 mm thick under a 30 x 20 mm boss 5 mm high, its top at Z 0.
fn plate_under_boss(half: Vec2) -> Surface {
    let mut t = Vec::new();
    box_tris(Vec3::new(-half.x, -half.y, -10.0), Vec3::new(half.x, half.y, -5.0), &mut t);
    box_tris(Vec3::new(-15.0, -10.0, -5.0), Vec3::new(15.0, 10.0, 0.0), &mut t);
    Surface::new(t).unwrap()
}

/// The light loads a small mill's tool library sets (5% of the diameter, and 2% for the larger
/// tools) are planned, not refused: the planner's raster follows the tool, not the load, and
/// its budget scales with the area and the load. Before, a 6 mm tool at 0.3 mm round a boss on
/// a 60 x 50 mm stock was refused after half a minute, and 0.45 mm on 100 x 60 mm took 26 s.
#[test]
fn light_loads_plan_round_a_boss() {
    let tool = flat(6.0);
    // With the work each takes: cell visits and drop-cutter samples, half as much again as they
    // take now (1.4e8 and 1.9e5; 3.6e9 and 8.2e5). The walks of a load the planner could not
    // follow ran into the level's budget before they were refused, many times this.
    for (half, load, most_visits, most_drops) in [(Vec2::new(30.0, 25.0), 0.3, 2.0e8, 280_000), (Vec2::new(50.0, 30.0), 0.12, 5.4e9, 1_250_000)] {
        let surface = plate_under_boss(half);
        let p = AdaptiveRoughingParams {
            heights: Heights { clearance: 10.0, retract: 2.0, top: 0.0, bottom: -5.0 },
            feeds: Feeds { cut: 1000.0, plunge: 200.0 },
            stepdown: 5.0,
            boundary: Boundary { rect: Some([-half, half]), region: None },
            flats: true,
            adaptive: AdaptiveParams { stock_to_leave: 0.0, ..light(load) },
            rest: None,
            step_up: None,
        };
        let (a, visits, drops) = work(|| adaptive_roughing(&surface, &tool, &p));
        let a = a.unwrap();
        let area = 4.0 * half.x * half.y - 600.0;
        let ratio = a.path.stats().cut_length / (area / load);
        eprintln!("{half:?} at {load} mm: {visits} cell visits, {drops} drops, path {:.0} mm, {ratio:.2} x area/load", a.path.stats().cut_length);
        assert!(ratio <= 1.5, "{ratio}");
        assert!(visits <= most_visits, "{visits} cell visits");
        assert!(drops <= most_drops, "{drops} drops");
        assert_eq!(a.helices, 0);
    }
}

/// Where the walks would take more steps than a clearing may, the refusal comes at once and
/// names the smallest load that plans.
#[test]
fn a_refusal_names_the_smallest_load_that_plans() {
    let tool = flat(6.0);
    let big = square(400.0);
    let (e, visits, drops) = work(|| adaptive(std::slice::from_ref(&big), &tool, &light(0.05), &cut(0.0, -1.0)).unwrap_err());
    let msg = e.to_string();
    assert!(matches!(e, CamError::TooLarge(_)) && msg.contains("optimal load of at least"), "{msg}");
    // Only the region's rasterising (1.5e7 cell visits), not a walk.
    assert!(visits <= 2.2e7 && drops == 0, "{visits} cell visits, {drops} drops before refusing");
    // The load named is one the estimate lets through.
    let at: f64 = msg.split("at least ").nth(1).and_then(|t| t.split(' ').next()).and_then(|t| t.parse().ok()).unwrap();
    assert!(at > 0.05 && at <= 6.0, "{at}");
}

/// The cut length of `a` against the area it clears over the load (one level).
fn length_ratio(a: &AdaptivePath, area: f64, load: f64) -> f64 {
    a.path.stats().cut_length / (area / load)
}

/// A 60 x 50 mm ring round a 30 x 20 mm boss whose outside is the stock's edge.
fn ring() -> Region2 {
    region(&[(-30.0, -25.0), (30.0, -25.0), (30.0, 25.0), (-30.0, 25.0)], &[&[(-15.0, -10.0), (-15.0, 10.0), (15.0, 10.0), (15.0, -10.0)]])
}

fn ring_stock() -> StockModel {
    StockModel { rect: Some([Vec2::new(-30.0, -25.0), Vec2::new(30.0, 25.0)]), top: Some(0.0), ..Default::default() }
}

/// Given the stock, a 2D clearing goes down in the open air beside it and cuts in from there,
/// as an adaptive roughing does: no helix, and a path about as long as the area over the load.
/// Without it (the regions taken as full of material, entered with a helix) the same ring took
/// 30.5 m at 0.6 mm, 7.6 times the area over the load, and was refused at 0.3 mm.
#[test]
fn a_2d_clearing_given_the_stock_enters_from_open_air() {
    let tool = flat(6.0);
    for load in [0.6, 0.3] {
        let c = Cut {
            heights: Heights { clearance: 10.0, retract: 2.0, top: 0.0, bottom: -5.0 },
            feeds: Feeds { cut: 1000.0, plunge: 200.0 },
            stepdown: 5.0,
        };
        let p = light(load);
        let a = adaptive_in(&[ring()], &tool, &p, &c, &ring_stock()).unwrap();
        verify(&a.path, &c.heights).unwrap();
        assert_eq!(a.helices, 0, "{a:?}");
        let ratio = length_ratio(&a, 2400.0, load);
        eprintln!("ring at {load} mm: {:.0} mm, {ratio:.2} x area/load", a.path.stats().cut_length);
        assert!(ratio <= 1.5, "{ratio}");
        // Every move that cuts below the top keeps the tool off the boss.
        for (q, cutting) in a.path.points(1e-3) {
            if cutting && q.z < 0.0 {
                let (dx, dy) = ((-15.0 - q.x).max(q.x - 15.0).max(0.0), (-10.0 - q.y).max(q.y - 10.0).max(0.0));
                assert!(dx.hypot(dy) >= 3.0 - 1e-6, "{q:?}");
            }
        }
        let stock = Material { regions: vec![region(&[(-30.0, -25.0), (30.0, -25.0), (30.0, 25.0), (-30.0, 25.0)], &[])], top: 0.0, within: None };
        let e = engagement(&a.path, Some(&a.roles), 3.0, &stock, 0.1).unwrap();
        assert!(e.max_deg <= p.max_engagement / DEG + ENGAGEMENT_TOL, "{e:?}");
        assert_eq!(e.full_slot_share, 0.0, "{e:?}");
        assert_cleared(&[ring()], &a.path, 3.0, 0.0, -5.0, 0.4);
    }
}

/// Where a region reaches past the stock, the part outside it is air: the clearing does not cut
/// there, so its path is as long as the stock's part of the region needs. A level at or above
/// the stock's top is left out.
#[test]
fn a_2d_clearing_does_not_cut_air_past_the_stock() {
    let tool = flat(6.0);
    let wide = region(&[(-40.0, -25.0), (40.0, -25.0), (40.0, 25.0), (-40.0, 25.0)], &[]);
    let stock = StockModel { rect: Some([Vec2::new(-20.0, -25.0), Vec2::new(20.0, 25.0)]), top: Some(-1.0), ..Default::default() };
    let c = Cut {
        heights: Heights { clearance: 10.0, retract: 2.0, top: 0.0, bottom: -3.0 },
        feeds: Feeds { cut: 1000.0, plunge: 200.0 },
        stepdown: 1.0,
    };
    let a = adaptive_in(&[wide], &tool, &params(), &c, &stock).unwrap();
    // Levels at -1 (the stock's top: nothing to cut), -2 and -3.
    assert_eq!(a.levels, 2, "{a:?}");
    assert_eq!(a.helices, 0);
    let ratio = a.path.stats().cut_length / (2.0 * 40.0 * 50.0 / 0.6);
    assert!(ratio <= 1.5, "{ratio}");
    // Nothing cuts past the stock but to come in from the air.
    for (q, cutting) in a.path.points(1e-3) {
        if cutting && q.z < -1.0 {
            assert!(q.x.abs() <= 20.0 + 3.0 + 0.5 * 6.0, "{q:?}");
        }
    }
}

/// A pocket closed all round is entered with a helix and cleared out to its corners. The walk
/// ends where it runs out of material instead of wandering through what it cleared, and the next
/// starts where cleared space meets material again: a 40 mm square at 0.6 mm took 3.3 times the
/// area over the load, and takes under 2.1 (the corners need a way back to each new cut when
/// every cut climbs).
#[test]
fn a_closed_pocket_path_is_not_much_longer_than_its_area_over_the_load() {
    let a = adaptive(&[square(40.0)], &flat(6.0), &light(0.6), &cut(0.0, -1.0)).unwrap();
    let ratio = length_ratio(&a, 1600.0, 0.6);
    eprintln!("closed square: {:.0} mm, {ratio:.2} x area/load", a.path.stats().cut_length);
    assert!(ratio <= 2.1, "{ratio}");
    assert_cleared(&[square(40.0)], &a.path, 3.0, 0.0, -1.0, 0.4);
}

/// Each level's helix ramps down from the level above it (cleared by then), not from the top of
/// the stock: a deep pocket's helices would otherwise grow with every level, through air.
#[test]
fn each_levels_helix_starts_at_the_level_above() {
    let closed = StockModel { rect: Some([Vec2::new(-10.0, -10.0), Vec2::new(40.0, 40.0)]), top: Some(0.0), ..Default::default() };
    for stock in [StockModel::default(), closed] {
        let c = cut(0.0, -9.0);
        let a = adaptive_in(&[square(30.0)], &flat(6.0), &params(), &c, &stock).unwrap();
        assert_eq!(a.levels, 3, "{a:?}");
        let mut helices = 0;
        let (mut pos, mut start): (Option<Vec3>, Option<f64>) = (None, None);
        for (m, role) in a.path.moves.iter().zip(&a.roles) {
            let Some(to) = m.target() else { continue };
            match (role, start) {
                (MoveRole::Entry, None) => start = pos.map(|p| p.z),
                (MoveRole::Entry, Some(_)) => {}
                (_, Some(s)) => {
                    let end = pos.map_or(s, |p| p.z);
                    assert!(s - end <= c.stepdown + 1e-6, "a helix from {s} down to {end}");
                    helices += 1;
                    start = None;
                }
                _ => {}
            }
            pos = Some(to);
        }
        assert!(helices >= 3, "{helices}");
    }
}

/// A height map over `lo..hi` in cells of `cell` mm, its tops from `top(x, y)`.
fn height_map(lo: Vec2, hi: Vec2, cell: f64, top: &dyn Fn(f64, f64) -> f64) -> HeightMap {
    let (nx, ny) = (((hi.x - lo.x) / cell).ceil() as usize, ((hi.y - lo.y) / cell).ceil() as usize);
    let heights = (0..nx * ny).map(|k| top(lo.x + ((k % nx) as f64 + 0.5) * cell, lo.y + ((k / nx) as f64 + 0.5) * cell)).collect();
    HeightMap { x0: lo.x, y0: lo.y, dx: cell, dy: cell, nx, ny, heights }
}

/// The length of the cutting moves (not links or entries) whose ends are both where `inside`.
fn cut_length_where(a: &AdaptivePath, inside: &dyn Fn(Vec3) -> bool) -> f64 {
    let mut len = 0.0;
    let mut prev: Option<Vec3> = None;
    for (m, role) in a.path.moves.iter().zip(&a.roles) {
        if let (Some(p), Some(t), MoveRole::Cut) = (prev, m.target(), role)
            && inside(p)
            && inside(t)
        {
            len += p.dist(t);
        }
        prev = m.target().or(prev);
    }
    len
}

/// Rest machining: with a height map of what earlier cuts left, the levels clear only what is
/// still above them. The plate's half at X < 0 already cut down to the stock to leave, a 3D
/// roughing cuts the other half; with everything cut it cuts nothing. A 2D clearing does the
/// same with what the map leaves inside its profiles.
#[test]
fn rest_machining_clears_only_what_earlier_cuts_left() {
    let surface = Surface::new(plate_and_boss()).unwrap();
    let tool = flat(6.0);
    let full = adaptive_roughing(&surface, &tool, &roughing_params()).unwrap();
    let (lo, hi) = (Vec2::new(-25.0, -25.0), Vec2::new(25.0, 25.0));
    let half = height_map(lo, hi, 0.5, &|x, _| if x < 0.0 { 0.25 } else { 6.0 });
    let p = AdaptiveRoughingParams { rest: Some(half), ..roughing_params() };
    let rest = adaptive_roughing(&surface, &tool, &p).unwrap();
    let (lf, lr) = (full.path.stats().cut_length, rest.path.stats().cut_length);
    eprintln!("whole stock {lf:.0} mm, half of it left {lr:.0} mm");
    assert!(lr < 0.65 * lf, "{lr} of {lf}");
    // Nothing is cut well inside the half already cut (the tool's radius and a cell off).
    let cut_left = cut_length_where(&rest, &|q| q.x < -3.0 - 1.0 - 0.5 * 6.0 && q.z < 6.0);
    assert!(cut_left < 0.02 * lr, "{cut_left} of {lr}");
    // All cut: nothing to do.
    let none = height_map(lo, hi, 0.5, &|_, _| 0.25);
    let p = AdaptiveRoughingParams { rest: Some(none), ..roughing_params() };
    let a = adaptive_roughing(&surface, &tool, &p).unwrap();
    assert_eq!(a.levels, 0, "{a:?}");
    // Hostile maps are refused.
    let bad = HeightMap { heights: vec![0.0; 3], ..height_map(lo, hi, 0.5, &|_, _| 0.0) };
    assert!(adaptive_roughing(&surface, &tool, &AdaptiveRoughingParams { rest: Some(bad), ..roughing_params() }).is_err());
    let nan = height_map(lo, hi, 0.5, &|x, _| if x < 0.0 { f64::NAN } else { 0.0 });
    assert!(adaptive_roughing(&surface, &tool, &AdaptiveRoughingParams { rest: Some(nan), ..roughing_params() }).is_err());
    // 2D: a square pocket whose half at X < 15 is already cut to its floor.
    let map = height_map(Vec2::new(-5.0, -5.0), Vec2::new(35.0, 35.0), 0.25, &|x, _| if x < 15.0 { -3.0 } else { 0.0 });
    let stock = StockModel { heights: Some(map), rest_tolerance: 0.02, ..Default::default() };
    let whole = adaptive(&[square(30.0)], &tool, &params(), &cut(0.0, -3.0)).unwrap();
    let a = adaptive_in(&[square(30.0)], &tool, &params(), &cut(0.0, -3.0), &stock).unwrap();
    let (lw, la) = (whole.path.stats().cut_length, a.path.stats().cut_length);
    eprintln!("2D: whole pocket {lw:.0} mm, half left {la:.0} mm");
    assert!(la < 0.65 * lw, "{la} of {lw}");
    assert!(cut_length_where(&a, &|q| q.x < 15.0 - 3.0 - 1.5 && q.z < 0.0) < 0.02 * la);
}

/// A stop asked for from another thread ends adaptive clearing, adaptive roughing and the
/// drop-cutter's 3D roughing with `Stopped` (nothing half planned); without one, or with the flag
/// down, the same calls plan as before.
#[test]
fn a_stop_ends_long_planning_with_stopped() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    let surface = Surface::new(plate_and_boss()).unwrap();
    let raised = Some(Arc::new(AtomicBool::new(true)));
    let e = crate::stop::scoped(raised.clone(), || adaptive(&[square(30.0)], &flat(6.0), &params(), &cut(0.0, -2.0))).unwrap_err();
    assert_eq!(e, CamError::Stopped);
    let e = crate::stop::scoped(raised.clone(), || adaptive_roughing(&surface, &flat(6.0), &roughing_params())).unwrap_err();
    assert_eq!(e, CamError::Stopped);
    let rough = crate::surface_ops::RoughingParams {
        heights: Heights { clearance: 30.0, retract: 8.0, top: 6.0, bottom: 0.25 },
        feeds: Feeds { cut: 800.0, plunge: 300.0 },
        stepdown: 2.0,
        stepover: 3.0,
        angle: 0.0,
        boundary: Boundary::default(),
        stock_to_leave: 0.2,
        step: 1.0,
        tolerance: 0.05,
        zigzag: true,
    };
    let e = crate::stop::scoped(raised, || crate::surface_ops::roughing(&surface, &flat(6.0), &rough)).unwrap_err();
    assert_eq!(e, CamError::Stopped);
    // The flag is this scope's only: outside it, and with it down, the work is done.
    assert!(!crate::stop::stopped());
    let down = Some(Arc::new(AtomicBool::new(false)));
    assert!(crate::stop::scoped(down, || adaptive(&[square(30.0)], &flat(6.0), &params(), &cut(0.0, -2.0))).is_ok());
    assert!(adaptive_roughing(&surface, &flat(6.0), &roughing_params()).is_ok());
}

/// A rectangle from `lo` to `hi` with its corners rounded to `r`, anticlockwise.
fn rounded(lo: (f64, f64), hi: (f64, f64), r: f64) -> Region2 {
    let (x0, y0, x1, y1) = (lo.0, lo.1, hi.0, hi.1);
    let v = Vec2::new;
    let arc = |cx: f64, cy: f64, start: f64| Seg2::Arc { center: v(cx, cy), radius: r, start, sweep: PI / 2.0 };
    let segs = vec![
        Seg2::Line { a: v(x0 + r, y0), b: v(x1 - r, y0) },
        arc(x1 - r, y0 + r, -PI / 2.0),
        Seg2::Line { a: v(x1, y0 + r), b: v(x1, y1 - r) },
        arc(x1 - r, y1 - r, 0.0),
        Seg2::Line { a: v(x1 - r, y1), b: v(x0 + r, y1) },
        arc(x0 + r, y1 - r, PI / 2.0),
        Seg2::Line { a: v(x0, y1 - r), b: v(x0, y0 + r) },
        arc(x0 + r, y0 + r, PI),
    ];
    Region2 { outer: Loop2 { segs }, holes: vec![] }
}

/// Parameters for a tool of radius `r` at `load`, the engagement limits following the load as in
/// [`light`].
fn light_for(r: f64, load: f64) -> AdaptiveParams {
    let opt = (1.0 - load / r).acos();
    let most = (opt * 1.5).max(opt + 15.0 * DEG).min(PI);
    AdaptiveParams { optimal_load: load, min_engagement: (opt / 3.0).min(most * 0.5), max_engagement: most, helix_diameter: 1.6 * r, ..params() }
}

/// A 10 mm and a 12 mm tool at a tool library's adaptive load (0.2 mm, 2% and 1.7% of the
/// diameter) clear a 36 x 24 mm pocket whose 4 mm corners are tighter than their radius: the
/// walks are not refused for the work the narrow pocket takes, and the corners they cannot reach
/// are left and reported as rest material for a smaller tool. Both were refused ("the walks keep
/// missing the material").
#[test]
fn large_tools_clear_a_pocket_with_tighter_corners_at_a_light_load() {
    let pocket = rounded((0.0, 0.0), (36.0, 24.0), 4.0);
    for d in [10.0, 12.0] {
        let p = light_for(d / 2.0, 0.2);
        let c = Cut { stepdown: 8.0, ..cut(0.0, -8.0) };
        let (a, planned, _) = work(|| adaptive(std::slice::from_ref(&pocket), &flat(d), &p, &c));
        let a = a.unwrap();
        verify(&a.path, &c.heights).unwrap();
        let ratio = length_ratio(&a, 864.0, 0.2);
        eprintln!("D{d} at 0.2 mm: {:.0} mm, {ratio:.2} x area/load, corners {:.2} mm², {:?}", a.path.stats().cut_length, a.corner_area, planned);
        assert!(a.corner_area > 0.5 && a.corner_area < 4.0 * 4.0 * 4.0, "{}", a.corner_area);
        assert!(a.warnings.iter().any(|w| w.contains("corners tighter than the tool")), "{:?}", a.warnings);
        let e = measure(std::slice::from_ref(&pocket), &a, d / 2.0, 0.0);
        assert!(e.max_deg <= p.max_engagement / DEG + ENGAGEMENT_TOL, "{e:?}");
        assert_eq!(e.full_slot_share, 0.0, "{e:?}");
        // What the walks leave within reach is in the corners too, and reported.
        let (checked, missed) = uncut(std::slice::from_ref(&pocket), &a.path, d / 2.0, 0.0, -8.0, 0.4);
        let corners = [Vec2::new(0.0, 0.0), Vec2::new(36.0, 0.0), Vec2::new(36.0, 24.0), Vec2::new(0.0, 24.0)];
        assert!(checked > 1000 && missed.iter().all(|q| corners.iter().any(|c| q.dist(*c) <= d)), "{missed:?}");
        assert!(missed.is_empty() || a.left_area > 0.0, "{}", a.left_area);
        // Planning takes 6.2e8 cell visits for the 10 mm tool and 7.5e8 for the 12 mm one.
        assert!(planned <= 1.1e9, "{planned} cell visits");
    }
}

/// A clearing refused for the work its walks take names the smallest optimal load that does
/// plan, found by planning (not estimated), and that load plans.
#[test]
fn a_refusal_for_the_walks_work_names_a_load_that_plans() {
    let pocket = rounded((0.0, 0.0), (36.0, 24.0), 4.0);
    let c = Cut { stepdown: 8.0, ..cut(0.0, -8.0) };
    let (tool, p) = (flat(10.0), light_for(5.0, 0.2));
    // A tenth of the usual budget, so the light load is refused.
    let e = crate::adaptive::adaptive_budgeted(std::slice::from_ref(&pocket), &tool, &p, &c, &StockModel::default(), 0.1).unwrap_err();
    let msg = e.to_string();
    assert!(matches!(e, CamError::TooLarge(_)) && msg.contains("optimal load of at least"), "{msg}");
    let at: f64 = msg.split("at least ").nth(1).and_then(|t| t.split(' ').next()).and_then(|t| t.parse().ok()).unwrap();
    assert!(at > 0.2 && at <= 10.0, "{at}");
    let raised = light_for(5.0, at);
    crate::adaptive::adaptive_budgeted(std::slice::from_ref(&pocket), &tool, &raised, &c, &StockModel::default(), 0.1).unwrap();
}

/// The work floor that lets a light load plan a narrow pocket is the clearing's own: one level
/// planned and repeated gets all of it, levels planned each on their own (rest machining) share
/// it, so a refusal at twenty levels does not take twenty times the floor's time (and as many
/// again for each load tried to name one that plans).
#[test]
fn the_work_floor_of_a_clearing_is_shared_among_the_levels_it_plans() {
    let one = crate::adaptive::level_floor_2d(1);
    assert!(one >= 1.0e9, "{one}");
    assert_eq!(crate::adaptive::level_floor_2d(0), one);
    let twenty = crate::adaptive::level_floor_2d(20);
    assert!((twenty * 20.0 - one).abs() <= 1e-6 * one, "{twenty}");
}

/// A dome of radius 20 on a 50 mm square plate (top at Z 0), finely tessellated.
fn dome() -> Vec<[Vec3; 3]> {
    let mut t = Vec::new();
    box_tris(Vec3::new(-25.0, -25.0, -5.0), Vec3::new(25.0, 25.0, 0.0), &mut t);
    let (n, m) = (48usize, 16usize);
    let p = |i: usize, j: usize| {
        let (a, b) = (TAU * i as f64 / n as f64, 0.5 * PI * j as f64 / m as f64);
        Vec3::new(20.0 * b.cos() * a.cos(), 20.0 * b.cos() * a.sin(), 20.0 * b.sin())
    };
    for i in 0..n {
        for j in 0..m {
            t.push([p(i, j), p(i + 1, j), p(i + 1, j + 1)]);
            t.push([p(i, j), p(i + 1, j + 1), p(i, j + 1)]);
        }
    }
    t
}

#[test]
fn a_level_check_answers_as_the_drop_and_opens_fewer_triangles() {
    // Whether a cutter rests above a level is what the full drop says, at every sample, for
    // flat, bull and ball cutters; above the dome's middle the level check opens far fewer
    // triangles (the roughing's walk asks it at every step of every level).
    let surface = Surface::new(dome()).unwrap();
    let opened = || OPENED.with(|n| n.replace(0));
    let (mut full, mut level) = (0usize, 0usize);
    for cutter in [Cutter::flat(5.25).unwrap(), Cutter::bull(5.25, 1.0).unwrap(), Cutter::ball(3.0).unwrap()] {
        for z in [2.0, 9.0, 15.0, 18.5] {
            for i in 0..=30 {
                for j in 0..=30 {
                    let (x, y) = (-30.0 + 2.0 * i as f64, -30.0 + 2.0 * j as f64);
                    let blocks = |zg: f64| zg + 0.25 > z + 1e-9;
                    opened();
                    let want = drop_z(&surface, &cutter, x, y).is_some_and(blocks);
                    full += opened();
                    assert_eq!(drop_z_blocks(&surface, &cutter, x, y, z - 0.25 - 1e-6, blocks), want, "at ({x}, {y}) level {z}");
                    level += opened();
                }
            }
        }
    }
    assert!(level * 3 < full, "the level check opened {level} triangles, the full drop {full}");
}

/// A clearing whose walks reach their work limit with only crumbs of the material left (some
/// 0.5% of it, along the walls) is planned, the crumbs named in a warning for the finish or a
/// rest pass to take, not refused: refusing it sent the search for a load that plans through
/// more clearings, each run to its limit (40 s of a flat on a steel shaft's 51 s). One clearing
/// is planned, no load searched for.
#[test]
fn walks_that_miss_only_crumbs_plan_with_a_warning_instead_of_a_refusal() {
    let pocket = rounded((0.0, 0.0), (36.0, 24.0), 4.0);
    let c = Cut { stepdown: 8.0, ..cut(0.0, -8.0) };
    let (tool, p) = (flat(10.0), light_for(5.0, 0.2));
    let (planned, stops) = (crate::adaptive::clearings_planned(), crate::adaptive::crumb_stops());
    // Four tenths of the usual budget: the walks run out of work with crumbs left.
    let a = crate::adaptive::adaptive_budgeted(std::slice::from_ref(&pocket), &tool, &p, &c, &StockModel::default(), 0.4).unwrap();
    assert_eq!(crate::adaptive::clearings_planned() - planned, 1);
    assert_eq!(crate::adaptive::crumb_stops() - stops, 1, "{:?}", a.warnings);
    verify(&a.path, &c.heights).unwrap();
    let crumbs = a.warnings.iter().find(|w| w.contains("kept missing the last of the material")).unwrap();
    assert!(crumbs.contains(&format!("{:.2} mm²", a.left_area)), "{crumbs}");
    assert!(a.left_area > 0.0 && a.left_area <= 0.005 * 36.0 * 24.0, "{}", a.left_area);
    let e = measure(std::slice::from_ref(&pocket), &a, 5.0, 0.0);
    assert!(e.max_deg <= p.max_engagement / DEG + ENGAGEMENT_TOL, "{e:?}");
}

/// A level's walks share their fans' engagements and the distances' windows out over a crew
/// of threads: the same clearing, and the same work counted, as on one thread.
#[test]
fn a_crew_plans_what_one_thread_plans() {
    let plan = || {
        let ((path, kept, windows), visits) = crate::adaptive::visits_in(|| {
            crate::adaptive::distances_in(|| adaptive(&[square(30.0)], &flat(6.0), &params(), &cut(0.0, -3.0)).unwrap())
        });
        (path, visits, kept, windows)
    };
    let before = crate::adaptive::crew_engaged();
    let shared = plan();
    let one = crate::par::serially(plan);
    assert_eq!(shared, one);
    if std::thread::available_parallelism().map_or(1, |n| n.get()) >= 8 {
        assert!(crate::adaptive::crew_engaged() > before, "no engagement was worked out by a crew");
    }
}
