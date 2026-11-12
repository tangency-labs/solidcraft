//! Tests of the leads, entries, linking, tabs and finishing passes of the 2D operations: every
//! one stays off the part, never plunges into material at the cutting feed, and refuses what
//! would cut the part or cannot be made, never panicking.

use std::f64::consts::PI;

use solidcore_record::{Loop2, Region2, Vec2, Vec3};
use solidcraft_geom::prelude::*;

use super::*;
use crate::offset::Shape;

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn polygon(pts: &[(f64, f64)]) -> Region2 {
    Region2 { outer: Loop2::polygon(&pts.iter().map(|p| v(p.0, p.1)).collect::<Vec<_>>()), holes: vec![] }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Region2 {
    polygon(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)])
}

fn tool(d: f64) -> Tool {
    Tool::new(1, ToolKind::FlatEndmill, d, 20.0)
}

const CUT_FEED: f64 = 600.0;
const PLUNGE_FEED: f64 = 200.0;

fn cut(top: f64, bottom: f64, stepdown: f64) -> Cut {
    Cut { heights: Heights { clearance: 10.0, retract: 2.0, top, bottom }, feeds: Feeds { cut: CUT_FEED, plunge: PLUNGE_FEED }, stepdown }
}

fn outside(climb: bool) -> ContourParams {
    ContourParams { side: Side::Outside, climb, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 }
}

fn lanes(stepover: f64) -> PocketParams {
    PocketParams { stepover, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 }
}

fn never(_: Vec2, _: Vec2, _: f64) -> bool {
    false
}

fn always(_: Vec2, _: Vec2, _: f64) -> bool {
    true
}

fn cutting_points(path: &Toolpath, below: f64) -> Vec<Vec3> {
    path.points(0.005).into_iter().filter(|(p, cutting)| *cutting && p.z < below).map(|(p, _)| p).collect()
}

/// Distance in XY from `q` to the nearest cut made entirely below `below`.
fn dist_to_cuts(path: &Toolpath, below: f64, q: Vec2) -> f64 {
    let flat = |p: Vec3| Vec3::new(p.x, p.y, 0.0);
    let pts = path.points(0.005);
    pts.windows(2)
        .filter(|w| w[1].1 && w[0].0.z < below && w[1].0.z < below)
        .map(|w| q.extend(0.0).dist_to_segment(flat(w[0].0), flat(w[1].0)))
        .fold(f64::INFINITY, f64::min)
}

/// Distance from `p` to the rectangle `[x0, x1] × [y0, y1]` (0 inside it).
fn outside_rect(p: Vec2, x0: f64, y0: f64, x1: f64, y1: f64) -> f64 {
    (p.x.clamp(x0, x1) - p.x).hypot(p.y.clamp(y0, y1) - p.y)
}

/// The steepest descent of a cutting move below `top` (radians), and whether one goes straight
/// down; a feed straight down from where a rapid stopped (through the air a level before cut,
/// to that level) is not counted.
fn steepest(path: &Toolpath, top: f64) -> (f64, bool) {
    let pts = path.points(0.002);
    let (mut angle, mut plunges) = (0.0f64, false);
    for w in pts.windows(2) {
        let (a, b) = (w[0].0, w[1].0);
        if !w[1].1 || b.z >= a.z - 1e-9 || a.z > top + 1e-9 || (!w[0].1 && (b.x - a.x).hypot(b.y - a.y) < 1e-9) {
            continue;
        }
        let run = (b.x - a.x).hypot(b.y - a.y);
        plunges |= run < 1e-9;
        angle = angle.max((a.z - b.z).atan2(run));
    }
    (angle, plunges)
}

/// Every straight-down feed that ends below `top`: its feed.
fn vertical_feeds(path: &Toolpath, top: f64) -> Vec<f64> {
    let mut pos: Option<Vec3> = None;
    let mut out = Vec::new();
    for m in &path.moves {
        if let (Move::Feed { to, feed }, Some(p)) = (*m, pos)
            && (to.x - p.x).hypot(to.y - p.y) < 1e-9
            && to.z < p.z - 1e-9
            && to.z < top - 1e-9
        {
            out.push(feed);
        }
        if let Some(to) = m.target() {
            pos = Some(to);
        }
    }
    out
}

/// Rapids that move sideways, with their height.
fn sideways_rapids(path: &Toolpath) -> Vec<f64> {
    let mut pos: Option<Vec3> = None;
    let mut out = Vec::new();
    for m in &path.moves {
        if let (Move::Rapid { to }, Some(p)) = (*m, pos)
            && (to.x - p.x).hypot(to.y - p.y) > 1e-9
        {
            out.push(to.z);
        }
        if let Some(to) = m.target() {
            pos = Some(to);
        }
    }
    out
}

/// The direction of travel at the end of each move and at the start of the next, where both
/// are cuts in XY: how far they turn (radians), largest first.
fn direction_changes(path: &Toolpath) -> Vec<(f64, Vec3)> {
    let dir = |from: Vec3, m: &Move, at_end: bool| -> Option<Vec2> {
        match *m {
            Move::Feed { to, .. } => (v(to.x, to.y) - v(from.x, from.y)).normalized(),
            Move::Arc { to, center, ccw, .. } => {
                let p = if at_end { v(to.x, to.y) } else { v(from.x, from.y) };
                let t = (p - center).perp().normalized()?;
                Some(if ccw { t } else { -t })
            }
            _ => None,
        }
    };
    let mut out = Vec::new();
    let mut pos: Option<Vec3> = None;
    let mut prev: Option<(Vec3, Move)> = None;
    for m in &path.moves {
        if let (Some(p), Some((from, last))) = (pos, prev)
            && let (Some(a), Some(b)) = (dir(from, &last, true), dir(p, m, false))
        {
            out.push((a.cross(b).atan2(a.dot(b)).abs(), p));
        }
        prev = pos.map(|p| (p, *m));
        if let Some(to) = m.target() {
            pos = Some(to);
        }
    }
    out.sort_by(|a, b| b.0.total_cmp(&a.0));
    out
}

#[test]
fn default_options_are_the_plain_operations() {
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let c = cut(0.0, -6.0, 3.0);
    for ramp in [0.0, 3f64.to_radians()] {
        let params = ContourParams { ramp_angle: ramp, ..outside(true) };
        assert_eq!(
            contour_with(&regions, &tool(6.0), &params, &ContourOptions::default(), &c, &always).unwrap(),
            contour(&regions, &tool(6.0), &params, &c).unwrap()
        );
        let params = PocketParams { ramp_angle: ramp, ..lanes(2.5) };
        assert_eq!(
            pocket_with(&regions, &tool(6.0), &params, &PocketOptions::default(), &c, &always).unwrap(),
            pocket(&regions, &tool(6.0), &params, &c).unwrap()
        );
    }
}

#[test]
fn leads_are_tangent_arcs_away_from_the_part() {
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let c = cut(0.0, -6.0, 3.0);
    let lead = Lead { radius: 2.0, sweep: PI / 2.0, line: 1.0 };
    for climb in [true, false] {
        let opts = ContourOptions { lead: Some(lead), ..ContourOptions::default() };
        let path = contour_with(&regions, &tool(6.0), &outside(climb), &opts, &c, &never).unwrap();
        // Never closer to the part than the tool radius; the leads lie further out.
        let pts = cutting_points(&path, 0.0);
        assert!(pts.iter().all(|p| outside_rect(v(p.x, p.y), 0.0, 0.0, 40.0, 30.0) >= 3.0 - 1e-6));
        assert!(pts.iter().any(|p| outside_rect(v(p.x, p.y), 0.0, 0.0, 40.0, 30.0) > 4.5), "the leads come from outside");
        // The lead arcs: two per level, each of radius 2.
        let arcs = path.moves.iter().filter(|m| matches!(m, Move::Arc { center, .. } if outside_rect(*center, 0.0, 0.0, 40.0, 30.0) > 4.9)).count();
        assert_eq!(arcs, 4, "{:?}", path.moves);
        // The tool never changes direction in a cut: every lead is tangent to what it joins,
        // and so is each line to its arc (the loop's own corners are arcs too).
        let turns: Vec<_> = direction_changes(&path).into_iter().filter(|(_, p)| p.z < 0.0).collect();
        assert!(turns.first().is_none_or(|t| t.0 < 1e-6), "{turns:?}");
        // The loop starts in the middle of a long side.
        let start = path.moves.iter().find_map(|m| match m {
            Move::Arc { to, center, .. } if outside_rect(*center, 0.0, 0.0, 40.0, 30.0) > 4.9 => Some(*to),
            _ => None,
        });
        let start = start.unwrap();
        assert!((start.x - 20.0).abs() < 1e-6 && ((start.y + 3.0).abs() < 1e-6 || (start.y - 33.0).abs() < 1e-6), "{start:?}");
    }
    // Inside an opening the lead comes from the opening's middle, and one too large for it is refused.
    let inside = ContourParams { side: Side::Inside, ..outside(true) };
    let opts = ContourOptions { lead: Some(lead), ..ContourOptions::default() };
    let path = contour_with(&regions, &tool(6.0), &inside, &opts, &c, &never).unwrap();
    let shape = Shape::from_regions(&regions, 0.01).unwrap();
    assert!(cutting_points(&path, 0.0).iter().all(|p| shape.clear(v(p.x, p.y), 3.0 - 1e-6)));
    let big = ContourOptions { lead: Some(Lead { radius: 13.0, sweep: PI, line: 0.0 }), ..ContourOptions::default() };
    let e = contour_with(&regions, &tool(6.0), &inside, &big, &c, &never).unwrap_err();
    assert!(e.to_string().contains("lead would cut into the part"), "{e}");
    // A lead's line running into a second part is refused too.
    let two = [rect(0.0, 0.0, 40.0, 30.0), rect(0.0, -14.0, 40.0, -8.0)];
    let long = ContourOptions { lead: Some(Lead { radius: 1.0, sweep: PI / 2.0, line: 30.0 }), ..ContourOptions::default() };
    assert!(contour_with(&two[..1], &tool(6.0), &outside(true), &long, &c, &never).is_ok());
    let e = contour_with(&two, &tool(4.0), &outside(true), &long, &cut(0.0, -2.0, 2.0), &never).unwrap_err();
    assert!(e.to_string().contains("lead"), "{e}");
}

#[test]
fn entries_never_plunge_into_material_at_the_cutting_feed() {
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let c = cut(0.0, -5.0, 2.5);
    let angle = 2f64.to_radians();
    let inside_walls = |path: &Toolpath| {
        for p in cutting_points(path, 0.0) {
            assert!(p.x >= 3.0 - 1e-6 && p.x <= 37.0 + 1e-6 && p.y >= 3.0 - 1e-6 && p.y <= 27.0 + 1e-6, "gouge at {p:?}");
        }
    };
    // A helix into the middle of the pocket: no steeper than its angle, never straight down.
    for linking in [Linking::Clearance, Linking::StayDown] {
        let opts = PocketOptions { entry: Some(Entry::Helix { radius: 2.7, angle }), linking };
        let path = pocket_with(&regions, &tool(6.0), &lanes(2.5), &opts, &c, &always).unwrap();
        let (steep, plunges) = steepest(&path, 0.0);
        assert!(!plunges && steep <= angle + 1e-6, "{steep} {plunges}");
        assert!(path.moves.iter().any(|m| matches!(m, Move::Arc { .. })), "helical arcs");
        assert_eq!(path.stats().lowest_z, Some(-5.0));
        inside_walls(&path);
        for i in 0..=34 {
            for j in 0..=24 {
                let q = v(3.0 + i as f64, 3.0 + j as f64);
                assert!(dist_to_cuts(&path, -5.0 + 1e-6, q) <= 3.0 + 0.01, "{q:?}");
            }
        }
    }
    // Too narrow a pocket for any helix: the ramp, else the plunge at the plunge feed.
    let slot = [rect(0.0, 0.0, 6.4, 30.0)];
    let opts = PocketOptions { entry: Some(Entry::Helix { radius: 3.0, angle }), ..PocketOptions::default() };
    let path = pocket_with(&slot, &tool(6.0), &lanes(2.5), &opts, &c, &never).unwrap();
    assert_eq!(path.stats().lowest_z, Some(-5.0));
    assert!(vertical_feeds(&path, 0.0).iter().all(|f| *f == PLUNGE_FEED));
    let tight = [rect(0.0, 0.0, 6.004, 6.004)];
    let path = pocket_with(&tight, &tool(6.0), &lanes(2.5), &opts, &c, &never).unwrap();
    let feeds = vertical_feeds(&path, 0.0);
    assert!(!feeds.is_empty() && feeds.iter().all(|f| *f == PLUNGE_FEED), "{feeds:?}");

    // A contour: a helix beside the part, a ramp along the lead, a ramp before the first tab.
    let cc = cut(0.0, -6.0, 3.0);
    let lead = Some(Lead { radius: 2.5, sweep: PI / 2.0, line: 4.0 });
    let tabs = Some(Tabs { layout: TabLayout::Count(2), width: 4.0, height: 1.5, ramp_angle: PI / 4.0 });
    for (entry, lead, tabs) in [
        (Entry::Helix { radius: 3.0, angle }, None, None),
        (Entry::Helix { radius: 3.0, angle }, lead, None),
        (Entry::Ramp { angle }, lead, None),
        (Entry::Ramp { angle }, None, tabs),
        (Entry::Helix { radius: 3.0, angle }, None, tabs),
    ] {
        let opts = ContourOptions { entry: Some(entry), lead, tabs, ..ContourOptions::default() };
        let path = contour_with(&regions, &tool(6.0), &outside(true), &opts, &cc, &never).unwrap();
        let (steep, plunges) = steepest(&path, 0.0);
        // The tabs' own ramps are steeper.
        let limit = if tabs.is_some() { PI / 4.0 } else { angle };
        assert!(!plunges && steep <= limit + 1e-6, "{entry:?} {lead:?}: {steep} {plunges}");
        assert!(cutting_points(&path, 0.0).iter().all(|p| outside_rect(v(p.x, p.y), 0.0, 0.0, 40.0, 30.0) >= 3.0 - 1e-6), "{entry:?}");
        assert_eq!(path.stats().lowest_z, Some(-6.0));
    }
    // A helix is never wider than the tool: it would leave a core standing.
    let wide = PocketOptions { entry: Some(Entry::Helix { radius: 3.5, angle }), ..PocketOptions::default() };
    assert!(pocket_with(&regions, &tool(6.0), &lanes(2.5), &wide, &c, &never).unwrap_err().to_string().contains("core"));
    // An entry and a ramp angle are one choice, not two.
    let both = PocketParams { ramp_angle: angle, ..lanes(2.5) };
    let opts = PocketOptions { entry: Some(Entry::Plunge), ..PocketOptions::default() };
    assert!(pocket_with(&regions, &tool(6.0), &both, &opts, &c, &never).is_err());
}

#[test]
fn pocket_links_stay_where_the_passes_have_cut() {
    // Two 20 × 20 rooms joined by an 8 mm passage: their middle rings are separate, and the
    // straight line between them runs through both rooms' uncut material.
    let rooms = [polygon(&[
        (0.0, 0.0),
        (20.0, 0.0),
        (20.0, 6.0),
        (30.0, 6.0),
        (30.0, 0.0),
        (50.0, 0.0),
        (50.0, 20.0),
        (30.0, 20.0),
        (30.0, 14.0),
        (20.0, 14.0),
        (20.0, 20.0),
        (0.0, 20.0),
    ])];
    let c = cut(0.0, -4.0, 2.0);
    let shape = Shape::from_regions(&rooms, 0.01).unwrap();
    let path = pocket(&rooms, &tool(4.0), &lanes(1.5), &c).unwrap();
    // Each room is entered on its own at every level, never linked to at depth across the other.
    // (The first move, to the clearance height, is not counted.)
    assert!(sideways_rapids(&path).len() >= 2 * 2 - 1, "{:?}", sideways_rapids(&path));
    for p in cutting_points(&path, 0.0) {
        assert!(shape.clear(v(p.x, p.y), 2.0 - 1e-6), "gouge at {p:?}");
    }
    // Staying down, the tool goes between the rooms at the retract height, and from one level to
    // the next through the level it has cleared.
    let opts = PocketOptions { entry: Some(Entry::Helix { radius: 1.8, angle: 0.05 }), linking: Linking::StayDown };
    let path = pocket_with(&rooms, &tool(4.0), &lanes(1.5), &opts, &c, &always).unwrap();
    let rapids = sideways_rapids(&path);
    assert!(rapids.len() >= 2 && rapids.iter().all(|z| (*z - 2.0).abs() < 1e-9), "{rapids:?}");
    for p in cutting_points(&path, 0.0) {
        assert!(shape.clear(v(p.x, p.y), 2.0 - 1e-6), "gouge at {p:?}");
    }
    // Where the part stands above the retract height in between, it goes over at the clearance height.
    let path = pocket_with(&rooms, &tool(4.0), &lanes(1.5), &opts, &c, &never).unwrap();
    assert!(sideways_rapids(&path).iter().all(|z| *z >= 10.0 - 1e-9), "{:?}", sideways_rapids(&path));
    // The same floor is cleared whichever way it links.
    for i in 0..=100 {
        for j in 0..=40 {
            let q = v(i as f64 * 0.5, j as f64 * 0.5);
            if shape.clear(q, 2.0 + 1e-3) {
                assert!(dist_to_cuts(&path, -4.0 + 1e-6, q) <= 2.0 + 0.01, "{q:?}");
            }
        }
    }
}

#[test]
fn tabs_stand_on_the_straight_sides_and_the_tool_rises_over_them() {
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let c = cut(0.0, -6.0, 2.0);
    let (width, height) = (4.0, 2.0);
    let tabs = Tabs { layout: TabLayout::Count(4), width, height, ramp_angle: PI / 4.0 };
    for climb in [true, false] {
        let opts = ContourOptions { tabs: Some(tabs), linking: Linking::StayDown, ..ContourOptions::default() };
        let path = contour_with(&regions, &tool(6.0), &outside(climb), &opts, &c, &always).unwrap();
        assert_eq!(path.stats().lowest_z, Some(-6.0));
        // Four climbs onto the tabs' height, all at the last level (the only one below it).
        let mut pos: Option<Vec3> = None;
        let mut tops = Vec::new();
        for m in &path.moves {
            if let (Some(p), Some(to)) = (pos, m.target())
                && p.z < -4.0 - 1e-9
                && (to.z + 4.0).abs() < 1e-9
            {
                tops.push(to);
            }
            pos = m.target().or(pos);
        }
        assert_eq!(tops.len(), 4, "{tops:?}");
        // Each tab is beside a straight side, not at a corner.
        for t in &tops {
            let side = ((t.y + 3.0).abs() < 1e-6 || (t.y - 33.0).abs() < 1e-6) && (0.0..=40.0).contains(&t.x)
                || ((t.x + 3.0).abs() < 1e-6 || (t.x - 43.0).abs() < 1e-6) && (0.0..=30.0).contains(&t.y);
            assert!(side, "{t:?}");
        }
        // Below the tabs' height the tool never comes within its radius of a tab's material: the
        // wall beside each top stretch, half the tab width either side of its middle.
        let all = path.points(0.005);
        let mut runs: Vec<(Vec3, Vec3)> = Vec::new();
        let mut open: Option<(Vec3, Vec3)> = None;
        for (p, cutting) in &all {
            if *cutting && (p.z + 4.0).abs() < 1e-9 {
                open = Some(open.map_or((*p, *p), |(a, _)| (a, *p)));
            } else if let Some(r) = open.take() {
                runs.push(r);
            }
        }
        let tab_runs: Vec<(Vec3, Vec3)> = runs.into_iter().filter(|(a, b)| a.dist(*b) > 9.0).collect();
        assert_eq!(tab_runs.len(), 4, "{tab_runs:?}");
        let below = cutting_points(&path, -4.0 - 1e-6);
        for (a, b) in &tab_runs {
            // The top stretch is the tab's width plus the tool's diameter long.
            assert!((a.dist(*b) - (width + 6.0)).abs() < 1e-6, "{a:?} {b:?}");
            let mid = v((a.x + b.x) * 0.5, (a.y + b.y) * 0.5);
            let wall = v(mid.x.clamp(0.0, 40.0), mid.y.clamp(0.0, 30.0));
            let along = (v(b.x, b.y) - v(a.x, a.y)).normalized().unwrap();
            for k in 0..=8 {
                let w = wall + along * (width * (k as f64 / 8.0 - 0.5));
                assert!(below.iter().all(|p| v(p.x, p.y).dist(w) >= 3.0 - 1e-6), "the tab at {w:?} is cut");
            }
        }
        assert!(below.iter().all(|p| outside_rect(v(p.x, p.y), 0.0, 0.0, 40.0, 30.0) >= 3.0 - 1e-6));
        // No descent steeper than the ramps.
        // Off the tabs no steeper than their ramps (the plunges into each level aside).
        let pts = path.points(0.002);
        for w in pts.windows(2) {
            let (a, b) = (w[0].0, w[1].0);
            let run = (b.x - a.x).hypot(b.y - a.y);
            if w[1].1 && b.z < a.z - 1e-9 && run > 1e-9 {
                assert!((a.z - b.z).atan2(run) <= PI / 4.0 + 1e-6, "{a:?} {b:?}");
            }
        }
    }
    // Tabs need an outside contour, a height below the depth, and room.
    let inside = ContourParams { side: Side::Inside, ..outside(true) };
    let opts = ContourOptions { tabs: Some(tabs), ..ContourOptions::default() };
    assert!(contour_with(&regions, &tool(6.0), &inside, &opts, &c, &never).is_err());
    let tall = ContourOptions { tabs: Some(Tabs { height: 6.0, ..tabs }), ..ContourOptions::default() };
    assert!(contour_with(&regions, &tool(6.0), &outside(true), &tall, &c, &never).is_err());
    let crowded = ContourOptions { tabs: Some(Tabs { layout: TabLayout::Count(40), ..tabs }), ..ContourOptions::default() };
    assert!(contour_with(&regions, &tool(6.0), &outside(true), &crowded, &c, &never).unwrap_err().to_string().contains("no room"));
    // By spacing: about one every 40 mm round the 159 mm loop.
    let spaced = ContourOptions { tabs: Some(Tabs { layout: TabLayout::Spacing(40.0), ..tabs }), ..ContourOptions::default() };
    assert!(contour_with(&regions, &tool(6.0), &outside(true), &spaced, &c, &never).is_ok());
}

/// Whether tabs have room round a contour is answered as the contour itself places them: for
/// every shape, tool, count and height, `tabs_fit` agrees with planning the contour.
#[test]
fn tabs_fit_agrees_with_the_contour() {
    let shapes = [
        rect(-15.0, -5.0, 15.0, 5.0),
        rect(-30.0, -20.0, 30.0, 20.0),
        rect(-7.0, -3.0, 7.0, 3.0),
        polygon(&[(0.0, 0.0), (40.0, 0.0), (40.0, 30.0), (20.0, 10.0), (0.0, 30.0)]),
    ];
    let mut seen = [false; 2];
    for region in &shapes {
        for d in [4.0, 6.0, 10.0] {
            for n in 1..=5 {
                for height in [1.5, 2.5, 6.0] {
                    for climb in [true, false] {
                        let tabs = Tabs { layout: TabLayout::Count(n), width: 4.0, height, ramp_angle: PI / 6.0 };
                        let c = Cut { heights: Heights { clearance: 20.0, retract: 12.0, top: 10.0, bottom: 0.0 }, ..cut(1.0, 0.0, 3.0) };
                        let params = outside(climb);
                        let opts =
                            ContourOptions { tabs: Some(tabs), entry: Some(Entry::Ramp { angle: 2f64.to_radians() }), ..ContourOptions::default() };
                        let planned = contour_with(std::slice::from_ref(region), &tool(d), &params, &opts, &c, &never).is_ok();
                        let fit = tabs_fit(std::slice::from_ref(region), &tool(d), &params, 0.0, &tabs).is_ok();
                        assert_eq!(fit, planned, "D{d}, {n} tabs {height} high, climb {climb}, round {:?}", region.outer);
                        seen[usize::from(fit)] = true;
                    }
                }
            }
        }
    }
    assert_eq!(seen, [true, true], "both answers come up");
    // Never a panic on hostile values.
    let bad = Tabs { layout: TabLayout::Count(3), width: f64::NAN, height: 1.0, ramp_angle: PI / 6.0 };
    assert!(tabs_fit(&[rect(0.0, 0.0, 10.0, 10.0)], &tool(4.0), &outside(true), 0.0, &bad).is_err());
    assert!(tabs_fit(&[], &tool(4.0), &outside(true), 0.0, &Tabs { width: 4.0, ..bad }).is_err());
    assert!(tabs_fit(&[rect(0.0, 0.0, 10.0, 10.0)], &tool(4.0), &outside(true), f64::INFINITY, &Tabs { width: 4.0, ..bad }).is_err());
}

#[test]
fn finishing_passes_close_in_on_the_wall_and_the_spring_pass_repeats_the_last() {
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let c = cut(0.0, -6.0, 3.0);
    let lap = |d: f64| 140.0 + 2.0 * PI * d;
    let opts = ContourOptions { finish_passes: 2, finish_stepover: 0.25, spring_pass: true, ..ContourOptions::default() };
    let path = contour_with(&regions, &tool(6.0), &outside(true), &opts, &c, &never).unwrap();
    // Two depth passes 3.5 out, then 3.25, 3 and 3 again at the bottom, linked in the cut: the
    // only sideways rapid is the first (not counted).
    assert!(sideways_rapids(&path).is_empty(), "{:?}", sideways_rapids(&path));
    let at_bottom: Vec<f64> = cutting_points(&path, -6.0 + 1e-9).iter().map(|p| outside_rect(v(p.x, p.y), 0.0, 0.0, 40.0, 30.0)).collect();
    for d in [3.5, 3.25, 3.0] {
        assert!(at_bottom.iter().any(|x| (x - d).abs() < 1e-6), "no pass {d} out");
    }
    assert!(at_bottom.iter().all(|x| *x >= 3.0 - 1e-6 && *x <= 3.5 + 1e-6));
    // 7 mm plunged (from 1 mm above the top, where the rapid stops, → −3 → −6), the laps, and the
    // two short links between the passes.
    let len = path.stats().cut_length;
    let laps = 2.0 * lap(3.5) + lap(3.25) + 2.0 * lap(3.0) + 7.0;
    assert!(len >= laps - 1e-6 && len <= laps + 2.0, "{len} {laps}");
    // With leads every pass goes in and out by its lead, linked above the part.
    let lead = ContourOptions { lead: Some(Lead { radius: 2.0, sweep: PI / 2.0, line: 0.0 }), linking: Linking::StayDown, ..opts };
    let path = contour_with(&regions, &tool(6.0), &outside(true), &lead, &c, &always).unwrap();
    assert!(cutting_points(&path, 0.0).iter().all(|p| outside_rect(v(p.x, p.y), 0.0, 0.0, 40.0, 30.0) >= 3.0 - 1e-6));
    let rapids = sideways_rapids(&path);
    assert!(rapids.len() >= 4 && rapids.iter().skip(1).all(|z| (*z - 2.0).abs() < 1e-9), "{rapids:?}");
    assert!(vertical_feeds(&path, 0.0).iter().all(|f| *f == PLUNGE_FEED));
    // An inside contour closes in on its opening's walls the same way.
    let inside = ContourParams { side: Side::Inside, ..outside(true) };
    let path = contour_with(&regions, &tool(6.0), &inside, &opts, &c, &never).unwrap();
    let shape = Shape::from_regions(&regions, 0.01).unwrap();
    assert!(cutting_points(&path, 0.0).iter().all(|p| shape.clear(v(p.x, p.y), 3.0 - 1e-6)));
    // A spring pass alone repeats the bottom pass.
    let spring = ContourOptions { spring_pass: true, ..ContourOptions::default() };
    let path = contour_with(&regions, &tool(6.0), &outside(true), &spring, &c, &never).unwrap();
    assert!((path.stats().cut_length - (3.0 * lap(3.0) + 7.0)).abs() < 1e-6);
    // Refused: finishing passes on the profile itself, or further apart than half the tool's radius.
    let on = ContourParams { side: Side::On, ..outside(true) };
    assert!(contour_with(&regions, &tool(6.0), &on, &opts, &c, &never).is_err());
    let wide = ContourOptions { finish_stepover: 2.0, ..opts };
    assert!(contour_with(&regions, &tool(6.0), &outside(true), &wide, &c, &never).is_err());
    let many = ContourOptions { finish_passes: MAX_FINISH_PASSES + 1, ..opts };
    assert!(contour_with(&regions, &tool(6.0), &outside(true), &many, &c, &never).is_err());
}

#[test]
fn hostile_options_are_errors_never_panics() {
    let bad = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0, 1e300, -1e300, 1e-300, 1e-7, 3.0, 1e4];
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let circle = [Region2 { outer: Loop2::circle(v(0.0, 0.0), 8.0), holes: vec![] }];
    let c = cut(0.0, -6.0, 3.0);
    for &b in &bad {
        let entries = [Entry::Plunge, Entry::Ramp { angle: b }, Entry::Helix { radius: b, angle: 0.05 }, Entry::Helix { radius: 2.0, angle: b }];
        let leads =
            [Lead { radius: b, sweep: PI / 2.0, line: 0.0 }, Lead { radius: 2.0, sweep: b, line: 0.0 }, Lead { radius: 2.0, sweep: 1.0, line: b }];
        let layouts = [TabLayout::Count(0), TabLayout::Count(usize::MAX), TabLayout::Spacing(b)];
        let tabs = [
            Tabs { layout: TabLayout::Count(3), width: b, height: 1.0, ramp_angle: 0.5 },
            Tabs { layout: TabLayout::Count(3), width: 2.0, height: b, ramp_angle: 0.5 },
            Tabs { layout: TabLayout::Count(3), width: 2.0, height: 1.0, ramp_angle: b },
        ];
        for side in [Side::Outside, Side::Inside, Side::On] {
            let params = ContourParams { side, ..outside(true) };
            for regions in [&regions[..], &circle[..]] {
                for entry in entries {
                    for lead in leads {
                        let opts = ContourOptions { entry: Some(entry), lead: Some(lead), ..ContourOptions::default() };
                        let _ = contour_with(regions, &tool(6.0), &params, &opts, &c, &always);
                    }
                    let opts = PocketOptions { entry: Some(entry), linking: Linking::StayDown };
                    let _ = pocket_with(regions, &tool(6.0), &lanes(2.5), &opts, &c, &always);
                }
                for t in tabs.iter().copied().chain(layouts.iter().map(|&layout| Tabs { layout, width: 2.0, height: 1.0, ramp_angle: 0.5 })) {
                    let opts =
                        ContourOptions { tabs: Some(t), finish_passes: 2, finish_stepover: 0.2, spring_pass: true, ..ContourOptions::default() };
                    let _ = contour_with(regions, &tool(6.0), &params, &opts, &c, &never);
                }
                let opts = ContourOptions { finish_passes: 3, finish_stepover: b, ..ContourOptions::default() };
                let _ = contour_with(regions, &tool(6.0), &params, &opts, &c, &never);
            }
        }
    }
    // A circle: leads, a helix, tabs and finishing passes on arcs only.
    let opts = ContourOptions {
        lead: Some(Lead { radius: 2.0, sweep: PI / 2.0, line: 1.0 }),
        entry: Some(Entry::Helix { radius: 2.5, angle: 0.05 }),
        tabs: Some(Tabs { layout: TabLayout::Count(3), width: 3.0, height: 1.0, ramp_angle: 0.6 }),
        finish_passes: 2,
        finish_stepover: 0.3,
        spring_pass: true,
        linking: Linking::StayDown,
        vertical_lead: None,
    };
    let path = contour_with(&circle, &tool(6.0), &outside(true), &opts, &c, &always).unwrap();
    assert!(cutting_points(&path, 0.0).iter().all(|p| v(p.x, p.y).len() >= 11.0 - 1e-6));
    assert!(verify(&path, &c.heights).is_ok());
}

/// The fastest any cut goes down (mm/min of Z): a feed's or an arc's rate times the share of
/// its length that is descent.
fn fastest_descent(path: &Toolpath) -> f64 {
    let mut pos: Option<Vec3> = None;
    let mut fastest = 0.0f64;
    for m in &path.moves {
        if let Some(p) = pos {
            match *m {
                Move::Feed { to, feed } if to.z < p.z - 1e-9 => fastest = fastest.max(feed * (p.z - to.z) / p.dist(to)),
                Move::Arc { to, center, ccw, feed } if to.z < p.z - 1e-9 => {
                    let run = v(p.x, p.y).dist(center) * arc_sweep(p, to, center, ccw);
                    fastest = fastest.max(feed * (p.z - to.z) / run.hypot(p.z - to.z));
                }
                // A vertical arc goes down fastest where it is steepest: straight down at the
                // start of a quarter arc into a cut.
                Move::PlaneArc { to, center, plane, ccw, feed } => {
                    let arc = crate::path::PlaneArc { from: p, to, center, plane, ccw };
                    for k in 0..=64 {
                        let down = arc.tangent(f64::from(k) / 64.0).map_or(0.0, |t| -t.z);
                        fastest = fastest.max(feed * down);
                    }
                }
                _ => {}
            }
        }
        pos = m.target().or(pos);
    }
    fastest
}

#[test]
fn no_cut_goes_down_faster_than_the_plunge_feed() {
    // The feeds of a 3 mm cutter in aluminium on a small mill: 2880 across, 100 down. A tab's
    // 30° ramp at the cutting feed would go down at 1440 mm/min, a 2° helix at 100.5.
    let feeds = Feeds { cut: 2880.0, plunge: 100.0 };
    let c = Cut { feeds, ..cut(0.0, -6.0, 2.0) };
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let angle = 2f64.to_radians();
    let tabs = Some(Tabs { layout: TabLayout::Count(4), width: 4.0, height: 2.0, ramp_angle: PI / 6.0 });
    let lead = Some(Lead { radius: 2.0, sweep: PI / 2.0, line: 2.0 });
    for climb in [true, false] {
        for (entry, lead, tabs) in [
            (Entry::Plunge, None, tabs),
            (Entry::Ramp { angle }, None, tabs),
            (Entry::Ramp { angle }, lead, None),
            (Entry::Helix { radius: 1.4, angle }, None, None),
            (Entry::Helix { radius: 1.4, angle }, lead, tabs),
        ] {
            let opts = ContourOptions { entry: Some(entry), lead, tabs, finish_passes: 1, finish_stepover: 0.2, ..ContourOptions::default() };
            let path = contour_with(&regions, &tool(3.0), &outside(climb), &opts, &c, &never).unwrap();
            let fastest = fastest_descent(&path);
            assert!(fastest <= feeds.plunge * (1.0 + 1e-9), "{entry:?} {lead:?} {tabs:?}: down at {fastest} mm/min");
            // Only the descents are slowed: the passes themselves go at the cutting feed.
            assert!(path.moves.iter().any(|m| matches!(m, Move::Feed { feed, .. } if *feed == feeds.cut)));
        }
        // A vertical lead's quarter arc starts straight down.
        for vertical in [1.0, 2.5] {
            let opts = ContourOptions { vertical_lead: Some(vertical), finish_passes: 1, finish_stepover: 0.2, ..ContourOptions::default() };
            let path = contour_with(&regions, &tool(3.0), &outside(climb), &opts, &c, &never).unwrap();
            assert!(path.moves.iter().any(|m| matches!(m, Move::PlaneArc { .. })));
            let fastest = fastest_descent(&path);
            assert!(fastest <= feeds.plunge * (1.0 + 1e-6), "vertical lead {vertical}: down at {fastest} mm/min");
        }
        let params = PocketParams { climb, ..lanes(1.2) };
        for entry in [Entry::Helix { radius: 1.4, angle }, Entry::Ramp { angle }] {
            for linking in [Linking::Clearance, Linking::StayDown] {
                let opts = PocketOptions { entry: Some(entry), linking };
                let path = pocket_with(&regions, &tool(3.0), &params, &opts, &c, &always).unwrap();
                let fastest = fastest_descent(&path);
                assert!(fastest <= feeds.plunge * (1.0 + 1e-9), "{entry:?} {linking:?}: down at {fastest} mm/min");
            }
        }
    }
}

#[test]
fn a_contour_on_the_profile_takes_no_lead_or_helix() {
    // On the profile the tool cuts both sides of the line: a lead or a helix beside it would
    // cut into the face the line is engraved in.
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let c = cut(0.0, -1.0, 1.0);
    let on = ContourParams { side: Side::On, ..outside(true) };
    let lead = ContourOptions { lead: Some(Lead { radius: 2.0, sweep: PI / 2.0, line: 0.0 }), ..ContourOptions::default() };
    let e = contour_with(&regions, &tool(3.0), &on, &lead, &c, &never).unwrap_err();
    assert!(e.to_string().contains("on the profile"), "{e}");
    let helix = ContourOptions { entry: Some(Entry::Helix { radius: 1.0, angle: 0.05 }), ..ContourOptions::default() };
    let e = contour_with(&regions, &tool(3.0), &on, &helix, &c, &never).unwrap_err();
    assert!(e.to_string().contains("on the profile"), "{e}");
    // A ramp or a plunge stays on the line.
    for entry in [Entry::Ramp { angle: 0.05 }, Entry::Plunge] {
        let opts = ContourOptions { entry: Some(entry), ..ContourOptions::default() };
        let path = contour_with(&regions, &tool(3.0), &on, &opts, &c, &never).unwrap();
        let shape = Shape::from_regions(&regions, 0.01).unwrap();
        assert!(cutting_points(&path, 0.0).iter().all(|p| shape.dist(v(p.x, p.y)) < 1e-6), "{entry:?}");
    }
}

#[test]
fn finishing_passes_never_reach_where_the_depth_passes_did_not_fit() {
    // A part with a notch 10 mm wide, 20 deep. A 6 mm tool's depth passes 7.5 out do not fit in
    // it (they would need 15 mm); finishing passes 4.5 and 3 out do, and the first of them would
    // cut the notch as a full slot at full depth.
    let notched = [polygon(&[(0.0, 0.0), (40.0, 0.0), (40.0, 30.0), (25.0, 30.0), (25.0, 10.0), (15.0, 10.0), (15.0, 30.0), (0.0, 30.0)])];
    let c = cut(0.0, -6.0, 3.0);
    let opts = ContourOptions { finish_passes: 3, finish_stepover: 1.5, ..ContourOptions::default() };
    let e = contour_with(&notched, &tool(6.0), &outside(true), &opts, &c, &never).unwrap_err();
    assert!(e.to_string().contains("full slot"), "{e}");
    // Two parts 10 mm apart: the depth passes go round both, the finishing passes between them.
    let pair = [rect(0.0, 0.0, 40.0, 30.0), rect(0.0, 40.0, 40.0, 70.0)];
    let e = contour_with(&pair, &tool(6.0), &outside(true), &opts, &c, &never).unwrap_err();
    assert!(e.to_string().contains("full slot"), "{e}");
    // An opening whose neck the depth passes cannot pass but the finishing passes can.
    let inside = ContourParams { side: Side::Inside, ..outside(true) };
    let dumbbell = [polygon(&[
        (0.0, 0.0),
        (20.0, 0.0),
        (20.0, 10.0),
        (30.0, 10.0),
        (30.0, 0.0),
        (50.0, 0.0),
        (50.0, 30.0),
        (30.0, 30.0),
        (30.0, 20.0),
        (20.0, 20.0),
        (20.0, 30.0),
        (0.0, 30.0),
    ])];
    let e = contour_with(&dumbbell, &tool(6.0), &inside, &opts, &c, &never).unwrap_err();
    assert!(e.to_string().contains("full slot"), "{e}");
    // Where the depth passes fit too, or the finishing passes take less, the passes are planned,
    // sharp corners included.
    let light = ContourOptions { finish_passes: 1, finish_stepover: 0.3, ..ContourOptions::default() };
    for (regions, params) in [(&notched[..], outside(true)), (&pair[..], outside(true)), (&dumbbell[..], inside)] {
        assert!(contour_with(regions, &tool(6.0), &params, &light, &c, &never).is_ok());
    }
    let wide = [polygon(&[(0.0, 0.0), (40.0, 0.0), (40.0, 30.0), (30.0, 30.0), (30.0, 10.0), (10.0, 10.0), (10.0, 30.0), (0.0, 30.0)])];
    assert!(contour_with(&wide, &tool(6.0), &outside(true), &opts, &c, &never).is_ok());
    let triangle = [polygon(&[(0.0, 0.0), (60.0, 0.0), (30.0, 40.0)])];
    for params in [outside(true), inside] {
        assert!(contour_with(&triangle, &tool(6.0), &params, &opts, &c, &never).is_ok());
    }
}

/// The milling-tool rule of the face and chamfer mills holds for the entry a pocket or contour
/// actually takes: a face mill goes in along a ramp but never plunges, and a chamfer mill, a
/// drill and a thread mill do not pocket at all.
#[test]
fn the_entry_decides_whether_a_face_mill_may_pocket_or_contour() {
    let mut face = Tool::new(1, ToolKind::FaceMill, 10.0, 6.0);
    face.corner_radius = Some(0.8);
    let region = [rect(0.0, 0.0, 40.0, 30.0)];
    let c = cut(0.0, -1.0, 1.0);
    let ramp = Some(Entry::Ramp { angle: 3f64.to_radians() });
    let plunge = PocketOptions { entry: Some(Entry::Plunge), ..PocketOptions::default() };
    let e = pocket_with(&region, &face, &lanes(4.0), &plunge, &c, &never).unwrap_err();
    assert!(e.to_string().contains("a face mill cannot plunge"), "{e}");
    let e = pocket_with(&region, &face, &lanes(4.0), &PocketOptions::default(), &c, &never).unwrap_err();
    assert!(e.to_string().contains("a face mill cannot plunge"), "no entry and no ramp angle is a plunge: {e}");
    let ramped = PocketOptions { entry: ramp, ..PocketOptions::default() };
    assert!(pocket_with(&region, &face, &lanes(4.0), &ramped, &c, &never).is_ok());
    let e = contour_with(&region, &face, &outside(true), &ContourOptions::default(), &c, &never).unwrap_err();
    assert!(e.to_string().contains("a face mill cannot plunge"), "{e}");
    let ramped = ContourOptions { entry: ramp, ..ContourOptions::default() };
    assert!(contour_with(&region, &face, &outside(true), &ramped, &c, &never).is_ok());
    let mut chamfer = Tool::new(7, ToolKind::Chamfer, 10.0, 10.0);
    chamfer.angle_deg = Some(90.0);
    let others = [
        (chamfer, "a chamfer mill's side is tapered"),
        (Tool::new(3, ToolKind::Drill, 6.0, 30.0), "a drill cannot mill"),
        (Tool::new(5, ToolKind::ThreadMill, 6.0, 12.0), "a thread mill cuts threads only"),
    ];
    for (t, why) in others {
        let ramped = PocketOptions { entry: ramp, ..PocketOptions::default() };
        let e = pocket_with(&region, &t, &lanes(4.0), &ramped, &c, &never).unwrap_err();
        assert!(e.to_string().contains(why), "{:?}: {e}", t.kind);
        let ramped = ContourOptions { entry: ramp, ..ContourOptions::default() };
        let e = contour_with(&region, &t, &outside(true), &ramped, &c, &never).unwrap_err();
        assert!(e.to_string().contains(why), "{:?}: {e}", t.kind);
    }
}

/// The depths of the cuts that run level on the wall (`d` out from the rectangle), highest first.
fn levels_on_wall(path: &Toolpath, d: f64) -> Vec<f64> {
    let mut pos: Option<Vec3> = None;
    let mut zs: Vec<f64> = Vec::new();
    for m in &path.moves {
        if let (Some(a), Move::Feed { to, .. } | Move::Arc { to, .. }) = (pos, *m)
            && (a.z - to.z).abs() < 1e-9
            && (outside_rect(v(to.x, to.y), 0.0, 0.0, 40.0, 30.0) - d).abs() < 1e-6
            && !zs.iter().any(|z| (z - to.z).abs() < 1e-9)
        {
            zs.push(to.z);
        }
        pos = m.target().or(pos);
    }
    zs.sort_by(|a, b| b.total_cmp(a));
    zs
}

/// A finishing pass takes its stepover off the whole wall below it: on a wall deeper than the
/// tool's flutes it goes down in steps no deeper than the flutes, the last at the bottom (the
/// wall above each step already finished at its offset); within the flutes, once at the
/// bottom. A shank wider than the cutter would rub the finished wall above a step: refused.
#[test]
fn finishing_passes_deeper_than_the_flutes_step_down_within_them() {
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let c = cut(0.0, -30.0, 3.0);
    let opts = ContourOptions { finish_passes: 1, finish_stepover: 0.25, spring_pass: true, ..ContourOptions::default() };
    let short = Tool::new(1, ToolKind::FlatEndmill, 6.0, 12.0);
    let path = contour_with(&regions, &short, &outside(true), &opts, &c, &never).unwrap();
    // The finishing pass runs on the wall, 3 mm out (the spring pass again at the bottom).
    assert_eq!(levels_on_wall(&path, 3.0), [-10.0, -20.0, -30.0]);
    let (steep, _) = steepest(&path, 0.0);
    assert!(steep <= PI / 2.0 + 1e-9);
    // With flutes that reach the bottom, once at the bottom.
    let long = Tool::new(1, ToolKind::FlatEndmill, 6.0, 35.0);
    let path = contour_with(&regions, &long, &outside(true), &opts, &c, &never).unwrap();
    assert_eq!(levels_on_wall(&path, 3.0), [-30.0]);
    // A shank wider than the cutter.
    let wide = Tool { shank_diameter: Some(8.0), stickout: Some(40.0), ..short.clone() };
    let e = contour_with(&regions, &wide, &outside(true), &opts, &c, &never).unwrap_err();
    assert!(e.to_string().contains("flutes") && e.to_string().contains("shank"), "{e}");
    // The depth passes alone are not stepped this way (each is a stepdown deep).
    let rough = ContourOptions::default();
    assert!(contour_with(&regions, &short, &outside(true), &rough, &c, &never).is_ok());
}

/// A vertical lead comes down a quarter arc in the vertical plane of the pass's start into it,
/// and goes up one out of its end: `G18`/`G19` where that runs along X or Y, lines otherwise.
#[test]
fn a_vertical_lead_arcs_down_into_the_pass_and_up_out_of_it() {
    let c = cut(0.0, -4.0, 2.0);
    let square = [rect(-10.0, -10.0, 10.0, 10.0)];
    let opts = ContourOptions { vertical_lead: Some(1.5), ..ContourOptions::default() };
    let path = contour_with(&square, &tool(6.0), &outside(true), &opts, &c, &never).unwrap();
    assert!(verify(&path, &c.heights).is_ok());
    let arcs: Vec<&Move> = path.moves.iter().filter(|m| matches!(m, Move::PlaneArc { .. })).collect();
    // Into and out of each of the two levels; the square's sides run along X and Y.
    assert_eq!(arcs.len(), 4, "{arcs:?}");
    // Nothing is cut inside the square, the leads included.
    assert!(cutting_points(&path, 0.0).iter().all(|p| outside_rect(v(p.x, p.y), -10.0, -10.0, 10.0, 10.0) >= 3.0 - 1e-6));
    // With a horizontal lead, the vertical one comes down into the lead's start.
    let both = ContourOptions { lead: Some(Lead { radius: 2.0, sweep: PI / 2.0, line: 0.0 }), vertical_lead: Some(1.0), ..ContourOptions::default() };
    let path = contour_with(&square, &tool(6.0), &outside(true), &both, &c, &never).unwrap();
    assert!(verify(&path, &c.heights).is_ok());
    assert!(path.moves.iter().any(|m| matches!(m, Move::PlaneArc { .. })));
    // A circle starts along neither axis: its vertical leads are lines.
    let circle = [Region2 {
        outer: Loop2 { segs: vec![solidcore_record::Seg2::Arc { center: v(0.0, 0.0), radius: 10.0, start: 0.3, sweep: 2.0 * PI }] },
        holes: vec![],
    }];
    let path = contour_with(&circle, &tool(6.0), &outside(true), &opts, &c, &never).unwrap();
    assert!(verify(&path, &c.heights).is_ok());
    // A ramp or helix with it, a bad radius, or a lead into the part are refused.
    let ramp = ContourOptions { entry: Some(Entry::Ramp { angle: 0.1 }), ..opts };
    assert!(contour_with(&square, &tool(6.0), &outside(true), &ramp, &c, &never).is_err());
    for bad in [0.0, -1.0, f64::NAN, 1e9] {
        let o = ContourOptions { vertical_lead: Some(bad), ..ContourOptions::default() };
        assert!(contour_with(&square, &tool(6.0), &outside(true), &o, &c, &never).is_err(), "{bad}");
    }
    let inside = ContourParams { side: Side::Inside, ..outside(true) };
    let big = ContourOptions { vertical_lead: Some(30.0), ..ContourOptions::default() };
    assert!(contour_with(&square, &tool(6.0), &inside, &big, &c, &never).is_err());
    // A finishing pass whose vertical lead starts where the depth passes did not cut would
    // plunge into the material down to the arc's start, as a plain plunge would: refused,
    // unless the arc starts above the top.
    // Round a circle the lead's start, along the tangent, is well away from the depth passes.
    let deep = cut(0.0, -20.0, 10.0);
    let finish =
        |vertical: f64| ContourOptions { vertical_lead: Some(vertical), finish_passes: 1, finish_stepover: 0.25, ..ContourOptions::default() };
    let e = contour_with(&circle, &tool(6.0), &outside(true), &finish(12.0), &deep, &never).unwrap_err();
    assert!(e.to_string().contains("did not cut"), "{e}");
    let path = contour_with(&circle, &tool(6.0), &outside(true), &finish(20.0), &deep, &never).unwrap();
    assert!(verify(&path, &deep.heights).is_ok());
}
