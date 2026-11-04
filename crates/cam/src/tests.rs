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

fn area(loops: &[Loop2]) -> f64 {
    loops.iter().map(Loop2::signed_area).sum()
}

fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "{a} is not within {tol} of {b}");
}

/// Every point of the offset loops is `d` from the boundary, and every loop is closed.
fn check_offset(regions: &[Region2], d: f64, loops: &[Loop2]) {
    let shape = Shape::from_regions(regions, 0.01).unwrap();
    for lp in loops {
        assert!(lp.is_closed(1e-3), "open loop");
        for p in lp.polyline(0.001) {
            close(shape.dist(p), d.abs(), 1e-3);
        }
    }
}

fn tool(d: f64) -> Tool {
    Tool::new(1, ToolKind::FlatEndmill, d, 20.0)
}

fn cut(top: f64, bottom: f64, stepdown: f64) -> Cut {
    Cut { heights: Heights { clearance: 10.0, retract: 2.0, top, bottom }, feeds: Feeds { cut: 600.0, plunge: 200.0 }, stepdown }
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

#[test]
fn square_offsets_to_a_smaller_square_and_to_a_rounded_larger_one() {
    let r = [rect(0.0, 0.0, 40.0, 30.0)];
    let inner = offset_regions(&r, 5.0, 0.01).unwrap();
    assert_eq!(inner.len(), 1);
    close(area(&inner), 30.0 * 20.0, 1e-6);
    check_offset(&r, 5.0, &inner);
    // Grown by d: area + perimeter × d + π d² (four quarter circles at the corners).
    let outer = offset_regions(&r, -5.0, 0.01).unwrap();
    assert_eq!(outer.len(), 1);
    close(area(&outer), 1200.0 + 140.0 * 5.0 + PI * 25.0, 1e-6);
    check_offset(&r, 5.0, &outer);
}

#[test]
fn inside_corner_of_an_l_shape_is_rounded() {
    // Two 30×10 arms that share a 10×10 square, plus the part of the 5×5 square at the inside
    // corner that is at least 5 from the corner.
    let l = [polygon(&[(0.0, 0.0), (40.0, 0.0), (40.0, 20.0), (20.0, 20.0), (20.0, 40.0), (0.0, 40.0)])];
    let inner = offset_regions(&l, 5.0, 0.01).unwrap();
    assert_eq!(inner.len(), 1);
    close(area(&inner), 500.0 + 25.0 - 25.0 * PI / 4.0, 1e-6);
    check_offset(&l, 5.0, &inner);
}

#[test]
fn circles_and_holes_offset_exactly() {
    let disc = [Region2 { outer: Loop2::circle(v(3.0, -2.0), 10.0), holes: vec![] }];
    close(area(&offset_regions(&disc, 3.0, 0.01).unwrap()), 49.0 * PI, 1e-6);
    close(area(&offset_regions(&disc, -3.0, 0.01).unwrap()), 169.0 * PI, 1e-6);
    // A hole grows as its region shrinks.
    let plate = [Region2 { outer: rect(0.0, 0.0, 40.0, 40.0).outer, holes: vec![Loop2::circle(v(20.0, 20.0), 5.0).reversed()] }];
    let inner = offset_regions(&plate, 3.0, 0.01).unwrap();
    assert_eq!(inner.len(), 2);
    close(area(&inner), 34.0 * 34.0 - 64.0 * PI, 1e-6);
    check_offset(&plate, 3.0, &inner);
    // Hole loops given counter-clockwise are turned round.
    let ccw_hole = [Region2 { outer: rect(0.0, 0.0, 40.0, 40.0).outer, holes: vec![Loop2::circle(v(20.0, 20.0), 5.0)] }];
    close(area(&offset_regions(&ccw_hole, 3.0, 0.01).unwrap()), 34.0 * 34.0 - 64.0 * PI, 1e-6);
}

#[test]
fn narrow_neck_splits_the_offset_and_a_small_region_vanishes() {
    let dumbbell = [polygon(&[
        (0.0, 0.0),
        (20.0, 0.0),
        (20.0, 8.0),
        (30.0, 8.0),
        (30.0, 0.0),
        (50.0, 0.0),
        (50.0, 20.0),
        (30.0, 20.0),
        (30.0, 12.0),
        (20.0, 12.0),
        (20.0, 20.0),
        (0.0, 20.0),
    ])];
    let inner = offset_regions(&dumbbell, 3.0, 0.01).unwrap();
    assert_eq!(inner.len(), 2, "the 4 mm neck is too narrow for a 3 mm offset");
    check_offset(&dumbbell, 3.0, &inner);
    for lp in &inner {
        // A 14×14 square plus a small bulge towards the neck.
        let a = lp.signed_area();
        assert!(a > 196.0 && a < 200.0, "{a}");
    }
    // At 1.5 the neck stays open.
    assert_eq!(offset_regions(&dumbbell, 1.5, 0.01).unwrap().len(), 1);
    assert!(offset_regions(&[rect(0.0, 0.0, 10.0, 10.0)], 6.0, 0.01).unwrap().is_empty());
}

#[test]
fn polygon_with_many_shallow_corners_offsets_to_one_loop() {
    // A regular 720-gon: neighbouring sides turn by half a degree.
    let (n, radius) = (720usize, 25.0);
    let pts: Vec<(f64, f64)> =
        (0..n).map(|i| (radius * (i as f64 * 2.0 * PI / n as f64).cos(), radius * (i as f64 * 2.0 * PI / n as f64).sin())).collect();
    let gon = [polygon(&pts)];
    let half = PI / n as f64;
    let (apothem, perimeter) = (radius * half.cos(), 2.0 * n as f64 * radius * half.sin());
    let inner = offset_regions(&gon, 4.0, 0.01).unwrap();
    assert_eq!(inner.len(), 1);
    close(area(&inner), n as f64 * (apothem - 4.0).powi(2) * half.tan(), 1e-3);
    let outer = offset_regions(&gon, -4.0, 0.01).unwrap();
    assert_eq!(outer.len(), 1);
    close(area(&outer), n as f64 * apothem.powi(2) * half.tan() + perimeter * 4.0 + PI * 16.0, 1e-3);
}

#[test]
fn pocket_stays_inside_the_walls_and_clears_everything_the_tool_can_reach() {
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let params = PocketParams { stepover: 2.5, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    let path = pocket(&regions, &tool(6.0), &params, &cut(0.0, -5.0, 2.0)).unwrap();
    let stats = path.stats();
    assert_eq!(stats.lowest_z, Some(-5.0));
    // Three passes of 5/3 mm.
    let mut levels: Vec<i64> = cutting_points(&path, 0.0).iter().map(|p| (p.z * 1e6).round() as i64).collect();
    levels.sort();
    levels.dedup();
    assert!(levels.len() >= 3 && levels.contains(&-5_000_000) && levels.contains(&-1_666_667), "{levels:?}");
    for p in cutting_points(&path, 0.0) {
        assert!(p.x >= 3.0 - 1e-6 && p.x <= 37.0 + 1e-6 && p.y >= 3.0 - 1e-6 && p.y <= 27.0 + 1e-6, "gouge at {p:?}");
    }
    for i in 0..=34 {
        for j in 0..=24 {
            let q = v(3.0 + i as f64, 3.0 + j as f64);
            let d = dist_to_cuts(&path, -5.0 + 1e-6, q);
            assert!(d <= 3.0 + 0.01, "{q:?} is {d} from the nearest pass");
        }
    }
}

#[test]
fn pocket_goes_round_an_island() {
    let island = Loop2::circle(v(30.0, 20.0), 5.0);
    let regions = [Region2 { outer: rect(0.0, 0.0, 60.0, 40.0).outer, holes: vec![island] }];
    let params = PocketParams { stepover: 3.0, climb: true, stock_to_leave: 0.5, tolerance: 0.01, ramp_angle: 0.0 };
    let path = pocket(&regions, &tool(6.0), &params, &cut(0.0, -2.0, 2.0)).unwrap();
    let shape = Shape::from_regions(&regions, 0.01).unwrap();
    let pts = cutting_points(&path, 0.0);
    for p in &pts {
        // Tool radius 3 plus 0.5 left on the walls.
        assert!(shape.clear(v(p.x, p.y), 3.5 - 1e-4), "gouge at {p:?}");
    }
    for i in 0..=60 {
        for j in 0..=40 {
            let q = v(i as f64, j as f64);
            if shape.clear(q, 3.5 + 1e-3) {
                let d = dist_to_cuts(&path, 0.0, q);
                assert!(d <= 3.0 + 0.01, "{q:?} is {d} from the nearest pass");
            }
        }
    }
}

#[test]
fn contour_runs_round_the_part_with_the_wall_on_the_chosen_side() {
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let c = cut(0.0, -6.0, 3.0);
    let run = |climb: bool| {
        let params = ContourParams { side: Side::Outside, climb, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
        contour(&regions, &tool(6.0), &params, &c).unwrap()
    };
    let climb = run(true);
    // Two passes round the rounded rectangle 3 mm outside the part (exact lines and arcs),
    // plus the two descents from the clearance height at plunge feed (10 → −3, 10 → −6).
    let ring = 140.0 + 6.0 * PI;
    close(climb.stats().cut_length, 2.0 * ring + 29.0, 1e-6);
    close(climb.stats().cut_minutes, 2.0 * ring / 600.0 + 29.0 / 200.0, 1e-9);
    let turns = |path: &Toolpath| {
        let pts = cutting_points(path, -5.9);
        pts.iter().zip(pts.iter().cycle().skip(1)).map(|(a, b)| a.x * b.y - a.y * b.x).sum::<f64>()
    };
    // Climb round the outside of a part is clockwise (wall on the right of travel).
    assert!(turns(&climb) < 0.0);
    assert!(turns(&run(false)) > 0.0);
    for p in cutting_points(&climb, 0.0) {
        let outside = (p.x.clamp(0.0, 40.0) - p.x).hypot(p.y.clamp(0.0, 30.0) - p.y);
        close(outside, 3.0, 1e-6);
    }
    // Inside an opening the tool must fit.
    let inside = ContourParams { side: Side::Inside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    assert!(contour(&[rect(0.0, 0.0, 5.0, 5.0)], &tool(6.0), &inside, &c).is_err());
    let inner = contour(&regions, &tool(6.0), &inside, &c).unwrap();
    // The inset rectangle has sharp (mitered) corners: two exact laps plus the descents.
    close(inner.stats().cut_length, 2.0 * 2.0 * (34.0 + 24.0) + 29.0, 1e-6);
    // Climb inside an opening is counter-clockwise.
    assert!(turns(&inner) > 0.0);
}

/// The steepest descent of a cutting move (radians), and whether any goes straight down below `top`.
fn steepest(path: &Toolpath, top: f64) -> (f64, bool) {
    let pts = path.points(0.002);
    let (mut angle, mut plunges) = (0.0f64, false);
    for w in pts.windows(2) {
        let (a, b) = (w[0].0, w[1].0);
        if !w[1].1 || b.z >= a.z - 1e-9 || a.z > top + 1e-9 {
            continue;
        }
        let run = (b.x - a.x).hypot(b.y - a.y);
        plunges |= run < 1e-9;
        angle = angle.max((a.z - b.z).atan2(run));
    }
    (angle, plunges)
}

#[test]
fn ramps_replace_plunges_into_material() {
    let ramp = 3.0f64.to_radians();
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    // Each depth pass enters by ramping along the wall (the engine's ramp
    // entry walks the loop, so the descent is never steeper than asked),
    // then cuts it flat; the pass's own re-descent at the loop start lands
    // in the channel the ramp just cut.
    let c = cut(0.0, -6.0, 3.0);
    let params = ContourParams { side: Side::Outside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: ramp };
    let path = contour(&regions, &tool(6.0), &params, &c).unwrap();
    let (angle, _verticals) = steepest(&path, 0.0);
    assert!(angle <= ramp + 1e-6 && angle > 0.01, "{angle}");
    // Every straight-down descent sits at one corner of the offset loop:
    // the loop start, where the ramp ended and the channel is clear.
    let down: Vec<Vec3> = path
        .points(0.002)
        .windows(2)
        .filter(|w| w[1].1 && w[1].0.z < w[0].0.z - 1e-9 && (w[0].0.x - w[1].0.x).hypot(w[0].0.y - w[1].0.y) < 1e-9)
        .map(|w| w[1].0)
        .collect();
    assert!(!down.is_empty());
    assert!(down.iter().all(|p| Vec2::new(p.x, p.y).dist(Vec2::new(down[0].x, down[0].y)) < 1e-6), "{down:?}");
    assert_eq!(path.stats().lowest_z, Some(-6.0));

    // In a pocket the first ring is short, so the ramp goes round it several times.
    let params = PocketParams { stepover: 2.5, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: ramp };
    let path = pocket(&regions, &tool(6.0), &params, &cut(0.0, -5.0, 2.5)).unwrap();
    // Descents into uncut material never exceed the ramp angle. The
    // planner's SafeZ link hops land half a millimetre above the floor and
    // re-enter with one move down into the channel they just cut — cut
    // area, not a ramp into material — so re-entries of that depth are
    // exempt.
    let pts = path.points(0.002);
    let mut angle = 0.0f64;
    let mut plunges = false;
    for w in pts.windows(2) {
        let (a, b) = (w[0].0, w[1].0);
        if !w[1].1 || b.z >= a.z - 1e-9 || a.z > 0.0 + 1e-9 {
            continue;
        }
        let run = (b.x - a.x).hypot(b.y - a.y);
        if run < 1e-9 {
            plunges = true;
        } else if a.z - b.z > 0.5 + 1e-9 {
            angle = angle.max((a.z - b.z).atan2(run));
        }
    }
    assert!(!plunges && angle <= ramp + 1e-6 && angle > 0.01, "{angle} {plunges}");
    assert_eq!(path.stats().lowest_z, Some(-5.0));
    for p in cutting_points(&path, 0.0) {
        assert!(p.x >= 3.0 - 1e-6 && p.x <= 37.0 + 1e-6 && p.y >= 3.0 - 1e-6 && p.y <= 27.0 + 1e-6, "gouge at {p:?}");
    }
    // The floor is still cleared: the flat pass follows the ramp.
    for i in 0..=34 {
        for j in 0..=24 {
            let q = v(3.0 + i as f64, 3.0 + j as f64);
            assert!(dist_to_cuts(&path, -5.0 + 1e-6, q) <= 3.0 + 0.01, "{q:?}");
        }
    }
    // A pocket no wider than the tool has no room to ramp or helix into and
    // no wide region for the planner to clear: the operation refuses (the
    // engine plans slot passes for narrow passages between wide regions
    // only — an all-narrow pocket has nothing it can enter; upstream gap).
    let tight = pocket(&[rect(0.0, 0.0, 6.004, 6.004)], &tool(6.0), &params, &cut(0.0, -5.0, 2.5));
    assert!(tight.is_err(), "a tool-width pocket must be refused");
    assert!(pocket(&regions, &tool(6.0), &PocketParams { ramp_angle: 2.0, ..params }, &c).is_err());
}

#[test]
fn pocket_with_passes_up_to_a_diameter_apart_still_clears_the_middle() {
    // A 40 × 30 pocket, a 6 mm tool: gouge-free everywhere, and the floor
    // left uncleared only within the leave-behind the engine was told to
    // tolerate (area_tolerance = 1 mm²). The measured residue grows with
    // the stepover: the crescent between the entry seed and the first
    // spiral turn — at a full-diameter stepover it exceeds the tolerance
    // (raygeo issue #99, planned as a plan-layer slot-style cleanup).
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    const CELL: f64 = 0.25;
    let deep_uncut = |path: &Toolpath| {
        let mut uncut = 0.0;
        for i in 0..=136 {
            for j in 0..=96 {
                let q = v(3.125 + i as f64 * CELL, 3.125 + j as f64 * CELL);
                if dist_to_cuts(path, -2.0 + 1e-6, q) > 3.05 {
                    uncut += CELL * CELL;
                }
            }
        }
        uncut
    };
    for (stepover, name, max_uncut) in [(5.0, "wide", 6.0), (6.0, "a diameter", 12.0), (4.0, "moderate", 2.0)] {
        let params = PocketParams { stepover, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
        let path = pocket(&regions, &tool(6.0), &params, &cut(0.0, -2.0, 2.0)).unwrap();
        for p in cutting_points(&path, 0.0) {
            assert!(p.x >= 3.0 - 1e-6 && p.x <= 37.0 + 1e-6 && p.y >= 3.0 - 1e-6 && p.y <= 27.0 + 1e-6, "{name}: gouge at {p:?}");
        }
        let uncut = deep_uncut(&path);
        assert!(uncut <= max_uncut, "{name}: {uncut:.2} mm² left deeper than a radius-0.05 sweep film");
    }
    // Two rooms joined by a wide door, round an island: gouge-free everywhere.
    // Floor coverage is NOT asserted here yet: the engine leaves ~25% of the
    // machinable floor uncut at this stepover (long-standing — identical on
    // raygeo 1.69.1 — tracked upstream as raygeo issue #99; tighten when its
    // fix lands).
    let rooms = Region2 {
        outer: polygon(&[(0.0, 0.0), (30.0, 0.0), (30.0, 8.0), (60.0, 8.0), (60.0, 26.0), (30.0, 26.0), (30.0, 40.0), (0.0, 40.0)]).outer,
        holes: vec![Loop2::circle(v(15.0, 20.0), 4.0)],
    };
    let params = PocketParams { stepover: 5.5, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    let path = pocket(std::slice::from_ref(&rooms), &tool(6.0), &params, &cut(0.0, -2.0, 2.0)).unwrap();
    let shape = Shape::from_regions(std::slice::from_ref(&rooms), 0.01).unwrap();
    for p in cutting_points(&path, 0.0) {
        assert!(shape.clear(v(p.x, p.y), 3.0 - 1e-4), "gouge at {p:?}");
    }
    let too_wide = PocketParams { stepover: 6.5, ..params };
    assert!(pocket(&regions, &tool(6.0), &too_wide, &cut(0.0, -2.0, 2.0)).is_err(), "passes more than a diameter apart");
}

/// Two holes plunged at 200 mm/min with a quarter-second dwell, written out move by move.
fn drill_program() -> Program {
    let t = Tool::new(3, ToolKind::Drill, 5.0, 40.0);
    let at = |x: f64, z: f64| Vec3::new(x, 10.0, z);
    let mut moves = Vec::new();
    for (i, x) in [10.0, 30.5].into_iter().enumerate() {
        if i > 0 {
            moves.push(Move::Rapid { to: at(10.0, 10.0) });
        }
        moves.extend([
            Move::Rapid { to: at(x, 10.0) },
            Move::Rapid { to: at(x, 2.0) },
            Move::Feed { to: at(x, -10.0), feed: 200.0 },
            Move::Dwell { seconds: 0.25 },
        ]);
    }
    moves.push(Move::Rapid { to: at(30.5, 10.0) });
    let path = Toolpath { moves };
    verify(&path, &cut(0.0, -10.0, 1.0).heights).unwrap();
    Program {
        work_offset: None,
        name: "Bracket (rev 2)".into(),
        operations: vec![Operation { name: "Drill".into(), tool: t, spindle_rpm: 3000.0, coolant: true, path }],
    }
}

/// A post for a controller that wants a decimal point on every number, two-digit motion codes,
/// dwells as `G04 X` in seconds and helices as lines.
fn padded() -> Post {
    Post {
        name: "padded".into(),
        trailing_decimal: true,
        two_digit_codes: true,
        helical_arcs: false,
        dwell: "G04 X{seconds}".into(),
        ..Post::rs274()
    }
}

#[test]
fn post_switches_change_how_numbers_and_codes_are_written() {
    let text = emit(&drill_program(), &padded()).unwrap();
    for line in ["G00 X10. Y10.", "G00 Z2.", "G01 Z-10. F200.", "G04 X0.25", "G00 X30.5"] {
        assert!(text.lines().any(|l| l == line), "no `{line}` in\n{text}");
    }
    let back = Toolpath { moves: parse(&text).unwrap() };
    assert_eq!(back.stats(), drill_program().operations[0].path.stats());
}

#[test]
fn rs274_program_text() {
    let text = emit(&drill_program(), &Post::rs274()).unwrap();
    let expected = "\
(Bracket rev 2)
G17 G21 G40 G49 G54 G80 G90 G94
(Drill: T3 D5 drill)
M5
T3 M6
G43 H3
S3000 M3
M8
G0 X10 Y10
G0 Z10
G0 Z2
G1 Z-10 F200
G4 P0.25
G0 Z10
G0 X30.5
G0 Z2
G1 Z-10
G4 P0.25
G0 Z10
M9
M5
M30
";
    assert_eq!(text, expected);
}

#[test]
fn written_program_reads_back_as_the_same_path() {
    // A contour: its roll corners and the circular island wall are true arcs, so the
    // post's arc handling is exercised both ways.
    let island = Loop2::circle(v(30.0, 20.0), 5.0);
    let regions = [Region2 { outer: rect(0.0, 0.0, 60.0, 40.0).outer, holes: vec![island] }];
    let params = ContourParams { side: Side::Outside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    let path = contour(&regions, &tool(6.0), &params, &cut(0.0, -4.0, 2.0)).unwrap();
    let program = Program {
        work_offset: None,
        name: "p".into(),
        operations: vec![Operation { name: "Contour".into(), tool: tool(6.0), spindle_rpm: 12000.0, coolant: false, path: path.clone() }],
    };
    for arcs in [true, false] {
        let mut post = Post::rs274();
        post.arcs = arcs;
        let text = emit(&program, &post).unwrap();
        assert_eq!(text.contains("G2 ") || text.contains("G3 "), arcs);
        let back = Toolpath { moves: parse(&text).unwrap() };
        verify(&back, &cut(0.0, -4.0, 2.0).heights).unwrap();
        let (a, b) = (path.stats(), back.stats());
        // Coordinates are written to 0.0001 mm.
        close(a.cut_length, b.cut_length, 0.2);
        close(a.rapid_length, b.rapid_length, 0.2);
        assert_eq!(a.lowest_z, b.lowest_z);
        // Every point of the path as read is on the path as planned.
        let planned: Vec<Vec3> = path.points(0.002).into_iter().map(|(p, _)| p).collect();
        for (p, _) in back.points(0.002) {
            let d = planned.windows(2).map(|w| p.dist_to_segment(w[0], w[1])).fold(f64::INFINITY, f64::min);
            assert!(d < 0.02, "{p:?} is {d} off the planned path");
        }
    }
}

#[test]
fn every_post_writes_within_its_own_precision() {
    let island = Loop2::circle(v(30.0, 20.0), 5.0);
    let regions = [Region2 { outer: rect(0.0, 0.0, 60.0, 40.0).outer, holes: vec![island] }];
    let params = PocketParams { stepover: 3.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 3.0f64.to_radians() };
    let path = pocket(&regions, &tool(6.0), &params, &cut(0.0, -4.0, 2.0)).unwrap();
    let mut program = Program {
        work_offset: None,
        name: "p".into(),
        operations: vec![Operation { name: "Pocket".into(), tool: tool(6.0), spindle_rpm: 12000.0, coolant: false, path: path.clone() }],
    };
    let mut inch = Post::rs274();
    inch.units = Units::Inch;
    let mut chords = Post::rs274();
    chords.arcs = false;
    for post in [Post::rs274(), inch, chords, padded()] {
        let text = emit(&program, &post).unwrap();
        let r = readback(&text, &program, &post).unwrap();
        assert!(r.within(), "{}: {r:?}", post.name);
        close(r.cut_length, r.planned_cut_length, 0.05 * r.planned_cut_length / 100.0 + 0.5);
    }
    // The precision a post allows: four decimals in millimetres, 0.005 mm chords.
    close(allowed_deviation(&Post::rs274()), 3.0 * 0.00005 * 3f64.sqrt() + 0.01 + 1e-6, 1e-12);

    // A post that rounds to whole millimetres keeps to its own precision, but that precision
    // is coarse: the program strays tenths of a millimetre, which the read-back measures.
    let mut coarse = Post::rs274();
    coarse.decimals = 0;
    let r = readback(&emit(&program, &coarse).unwrap(), &program, &coarse).unwrap();
    assert!(r.within() && r.max_deviation > 0.3 && r.allowed > 2.0, "{r:?}");

    // A program that is not what was written: every move 0.5 mm higher.
    let text = emit(&program, &Post::rs274()).unwrap();
    for m in &mut program.operations[0].path.moves {
        if let Move::Rapid { to } | Move::Feed { to, .. } | Move::Arc { to, .. } = m {
            to.z += 0.5;
        }
    }
    let r = readback(&text, &program, &Post::rs274()).unwrap();
    assert!(!r.within(), "{r:?}");
    close(r.max_deviation, 0.5, 0.01);
    // A program with no cut against one with cuts: as far as the comparison looks.
    let empty = Program {
        work_offset: None,
        name: "e".into(),
        operations: vec![Operation {
            path: Toolpath { moves: vec![Move::Rapid { to: Vec3::new(0.0, 0.0, 10.0) }] },
            ..program.operations[0].clone()
        }],
    };
    let r = readback(&text, &empty, &Post::rs274()).unwrap();
    assert_eq!(r.max_deviation, r.reach);
}

#[test]
fn arc_too_short_to_write_is_never_a_full_circle() {
    // 0.00002 mm of arc: its end is written the same as its start.
    let moves = vec![
        Move::Rapid { to: Vec3::new(10.0, 0.0, 5.0) },
        Move::Feed { to: Vec3::new(10.0, 0.0, -1.0), feed: 100.0 },
        Move::Arc { to: Vec3::new(10.0, 0.00002, -1.0), center: v(0.0, 0.0), ccw: true, feed: 100.0 },
    ];
    let program = Program {
        work_offset: None,
        name: "p".into(),
        operations: vec![Operation { name: "o".into(), tool: tool(6.0), spindle_rpm: 1000.0, coolant: false, path: Toolpath { moves } }],
    };
    let text = emit(&program, &Post::rs274()).unwrap();
    assert!(!text.contains("G2 ") && !text.contains("G3 "), "{text}");
}

#[test]
fn verify_refuses_unsafe_paths() {
    let h = cut(0.0, -5.0, 1.0).heights;
    let at = |x: f64, z: f64| Vec3::new(x, 0.0, z);
    let unsafe_paths = [
        // Starts below the clearance height.
        vec![Move::Rapid { to: at(0.0, 1.0) }],
        // Starts with a cut.
        vec![Move::Feed { to: at(0.0, 10.0), feed: 100.0 }],
        // Rapid down into uncut material.
        vec![Move::Rapid { to: at(0.0, 10.0) }, Move::Rapid { to: at(0.0, -1.0) }],
        // Sideways rapid below the retract height.
        vec![Move::Rapid { to: at(0.0, 10.0) }, Move::Feed { to: at(0.0, -1.0), feed: 100.0 }, Move::Rapid { to: at(5.0, -1.0) }],
        // Below the bottom.
        vec![Move::Rapid { to: at(0.0, 10.0) }, Move::Feed { to: at(0.0, -5.1), feed: 100.0 }],
        // No feed.
        vec![Move::Rapid { to: at(0.0, 10.0) }, Move::Feed { to: at(0.0, -1.0), feed: 0.0 }],
        // Not a number.
        vec![Move::Rapid { to: at(f64::NAN, 10.0) }],
        // An arc whose ends are at different radii.
        vec![Move::Rapid { to: at(5.0, 10.0) }, Move::Arc { to: at(-7.0, 10.0), center: v(0.0, 0.0), ccw: true, feed: 100.0 }],
    ];
    for moves in unsafe_paths {
        assert!(matches!(verify(&Toolpath { moves: moves.clone() }, &h), Err(CamError::Unsafe(_))), "{moves:?}");
    }
    // A drill returning into its own hole is fine.
    let ok = vec![
        Move::Rapid { to: at(0.0, 10.0) },
        Move::Feed { to: at(0.0, -3.0), feed: 100.0 },
        Move::Rapid { to: at(0.0, 2.0) },
        Move::Rapid { to: at(0.0, -2.5) },
        Move::Feed { to: at(0.0, -5.0), feed: 100.0 },
    ];
    verify(&Toolpath { moves: ok }, &h).unwrap();
}

#[test]
fn hostile_values_are_errors_never_panics() {
    let bad = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 0.0, -1.0, 1e300, -1e300, 1e-300, f64::MIN_POSITIVE];
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let good_cut = cut(0.0, -5.0, 2.0);
    for &b in &bad {
        let _ = offset_regions(&regions, b, 0.01);
        let _ = offset_regions(&regions, 1.0, b);
        let _ = offset_regions(&[rect(0.0, 0.0, b, b)], 1.0, 0.01);
        let _ = offset_regions(&[Region2 { outer: Loop2::circle(v(b, b), b), holes: vec![Loop2::circle(v(0.0, b), b)] }], b, 0.01);
        let t = Tool::new(1, ToolKind::FlatEndmill, b, b);
        let pocket_params = PocketParams { stepover: b, climb: true, stock_to_leave: b, tolerance: b, ramp_angle: b };
        let contour_params = ContourParams { side: Side::Outside, climb: true, stock_to_leave: b, tolerance: b, ramp_angle: b };
        let bad_cut = Cut { heights: Heights { clearance: b, retract: b, top: b, bottom: -b }, feeds: Feeds { cut: b, plunge: b }, stepdown: b };
        for c in [good_cut, bad_cut, Cut { stepdown: b, ..good_cut }] {
            for tl in [&t, &tool(6.0)] {
                let _ = pocket(&regions, tl, &pocket_params, &c);
                let _ = pocket(&regions, tl, &PocketParams { stepover: 2.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 }, &c);
                let _ = contour(&regions, tl, &contour_params, &c);
            }
        }
        let moves = vec![
            Move::Rapid { to: Vec3::new(b, b, b) },
            Move::Arc { to: Vec3::new(b, 0.0, b), center: v(b, b), ccw: true, feed: b },
            Move::Dwell { seconds: b },
        ];
        let path = Toolpath { moves };
        let _ = path.stats();
        let _ = path.points(b);
        let _ = verify(&path, &good_cut.heights);
        let program = Program {
            work_offset: None,
            name: "x".into(),
            operations: vec![Operation { name: "o".into(), tool: tool(6.0), spindle_rpm: b, coolant: false, path }],
        };
        let _ = emit(&program, &Post::rs274());
    }
    // Things that must be refused outright.
    assert!(
        pocket(&regions, &tool(6.0), &PocketParams { stepover: 1e-9, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 }, &good_cut)
            .is_err()
    );
    assert!(
        pocket(&regions, &tool(60.0), &PocketParams { stepover: 2.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 }, &good_cut)
            .is_err()
    );
    assert!(
        pocket(&[], &tool(6.0), &PocketParams { stepover: 2.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 }, &good_cut)
            .is_err()
    );
    // A drill cuts only at its point: no contour or pocket with one.
    let drill = Tool::new(4, ToolKind::Drill, 6.0, 30.0);
    let square = [rect(0.0, 0.0, 20.0, 20.0)];
    let side = ContourParams { side: Side::Outside, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    assert!(contour(&square, &drill, &side, &good_cut).unwrap_err().to_string().contains("a drill cannot mill"));
    let lanes = PocketParams { stepover: 2.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    assert!(pocket(&square, &drill, &lanes, &good_cut).is_err());
    let open = Region2 { outer: Loop2 { segs: rect(0.0, 0.0, 10.0, 10.0).outer.segs.into_iter().take(3).collect() }, holes: vec![] };
    assert!(offset_regions(&[open], 1.0, 0.01).is_err());
    assert!(emit(&Program::default(), &Post::rs274()).is_err());
}

#[test]
fn posts_are_data_and_bad_ones_are_refused() {
    let mut own = Post::rs274();
    (own.name, own.units, own.line_numbers) = ("own".into(), Units::Inch, Some(LineNumbers { start: 10, step: 5 }));
    let json = serde_json::to_string(&own).unwrap();
    let post: Post = serde_json::from_str(&json).unwrap();
    assert_eq!(post, own);
    assert_eq!(Post::builtin("LinuxCNC"), Some(Post::rs274()));
    assert!(Post::BUILTIN.iter().all(|name| Post::builtin(name).is_some_and(|p| p.validate().is_ok())));
    let broken = |change: fn(&mut Post)| {
        let mut p = Post::rs274();
        change(&mut p);
        p.validate().is_err() && emit(&drill_program(), &p).is_err()
    };
    assert!(broken(|p| p.header = vec!["G21 {nonsense}".into()]));
    assert!(broken(|p| p.footer = vec!["M30 {tool".into()]));
    assert!(broken(|p| p.tool_change = vec!["M6\nG0 Z-100".into()]));
    assert!(broken(|p| p.decimals = 9));
    assert!(broken(|p| p.arc_tolerance = f64::NAN));
    assert!(broken(|p| p.line_numbers = Some(LineNumbers { start: 1, step: 0 })));
    assert!(broken(|p| p.extension = "../x".into()));
    assert!(broken(|p| p.spindle_on = vec!["M3".into(); 1000]));
    assert!(broken(|p| p.dwell = "G4 P{minutes}".into()));
    // A post saved before these switches existed reads with what it meant.
    let mut old = serde_json::to_value(Post::rs274()).unwrap();
    for k in ["helical_arcs", "trailing_decimal", "two_digit_codes", "dwell"] {
        old.as_object_mut().unwrap().remove(k);
    }
    assert_eq!(serde_json::from_value::<Post>(old).unwrap(), Post::rs274());
}

#[test]
fn reader_reports_what_it_cannot_read() {
    for text in ["G1 X1 Y1 Z1", "X1 Y1 Z1", "G0 X1 Y1 Z1\nG2 X2 Y2", "G91", "G0 X1..2", "G0 X", "G0 @", "G2 X1 Y1 Z1 I1 F10"] {
        assert!(matches!(parse(text), Err(CamError::Parse(..))), "{text}");
    }
    assert_eq!(parse("%\n(a comment) ; another\nN10 g0 x1 y2 (inline) z3\n").unwrap(), vec![Move::Rapid { to: Vec3::new(1.0, 2.0, 3.0) }]);
    assert_eq!(parse("G20 G0 X1 Y0 Z0\nG1 X2 F10").unwrap().last(), Some(&Move::Feed { to: Vec3::new(50.8, 0.0, 0.0), feed: 254.0 }));
}

#[test]
fn work_offsets_go_where_the_post_writes_them() {
    let mut program = drill_program();
    program.work_offset = Some(3);
    let text = emit(&program, &Post::rs274()).unwrap();
    assert!(text.contains("\nG17 G21 G40 G49 G56 G80 G90 G94\n"), "{text}");
    // A post that leaves the offset to the operator can only stand for the first one.
    let mut manual = Post::rs274();
    manual.header = vec!["G17 {units} G40 G49 G80 G90 G94".into()];
    let refused = emit(&program, &manual).unwrap_err().to_string();
    assert!(refused.contains("writes no work offset") && refused.contains("G56"), "{refused}");
    program.work_offset = Some(1);
    assert!(emit(&program, &manual).is_ok());
    for n in [0, 7, 255] {
        program.work_offset = Some(n);
        assert!(emit(&program, &Post::rs274()).is_err());
    }
    assert_eq!(work_offset_code(6).unwrap(), "G59");
}

#[test]
fn adaptive_stays_inside_the_walls_and_clears_everything_the_tool_can_reach() {
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let params = AdaptiveParams { stepover: 2.5, climb: true, stock_to_leave: 0.0, tolerance: 0.01 };
    let path = adaptive(&regions, &tool(6.0), &params, &cut(0.0, -5.0, 2.0)).unwrap();
    let stats = path.stats();
    assert_eq!(stats.lowest_z, Some(-5.0));
    // The depth passes are the same as the pocket's.
    let mut levels: Vec<i64> = cutting_points(&path, 0.0).iter().map(|p| (p.z * 1e6).round() as i64).collect();
    levels.sort();
    levels.dedup();
    assert!(levels.len() >= 3 && levels.contains(&-5_000_000) && levels.contains(&-1_666_667), "{levels:?}");
    for p in cutting_points(&path, 0.0) {
        assert!(p.x >= 3.0 - 1e-6 && p.x <= 37.0 + 1e-6 && p.y >= 3.0 - 1e-6 && p.y <= 27.0 + 1e-6, "gouge at {p:?}");
    }
    // Everything a 3 mm-radius tool can reach is cut at the bottom.
    for i in 0..=34 {
        for j in 0..=24 {
            let q = v(3.0 + i as f64, 3.0 + j as f64);
            let d = dist_to_cuts(&path, -5.0 + 1e-6, q);
            assert!(d <= 3.0 + 0.01, "{q:?} is {d} from the nearest pass");
        }
    }
}

#[test]
fn adaptive_keeps_the_stock_to_leave_on_the_walls() {
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let params = AdaptiveParams { stepover: 2.5, climb: true, stock_to_leave: 0.5, tolerance: 0.01 };
    let path = adaptive(&regions, &tool(6.0), &params, &cut(0.0, -2.0, 2.0)).unwrap();
    for p in cutting_points(&path, 0.0) {
        assert!(p.x >= 3.5 - 0.01 && p.x <= 36.5 + 0.01 && p.y >= 3.5 - 0.01 && p.y <= 26.5 + 0.01, "cut into the stock at {p:?}");
    }
}

#[test]
fn adaptive_goes_round_an_island() {
    let regions = [Region2 {
        outer: Loop2::polygon(&[v(0.0, 0.0), v(40.0, 0.0), v(40.0, 30.0), v(0.0, 30.0)]),
        holes: vec![Loop2::polygon(&[v(15.0, 10.0), v(25.0, 10.0), v(25.0, 20.0), v(15.0, 20.0)])],
    }];
    let params = AdaptiveParams { stepover: 2.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01 };
    let path = adaptive(&regions, &tool(6.0), &params, &cut(0.0, -4.0, 4.0)).unwrap();
    // The walls and the island are kept a tool radius away at the bottom. On
    // the way down, the planner's entries are placed against its region
    // approximation and may come about half a millimetre closer.
    let island_dist = |p: Vec3| {
        let (dx, dy) = ((15.0 - p.x).max(0.0).max(p.x - 25.0), (10.0 - p.y).max(0.0).max(p.y - 20.0));
        (dx * dx + dy * dy).sqrt()
    };
    // The clamp keeps every cutting point a tool radius from the walls and
    // the island at every depth.
    for p in cutting_points(&path, 0.0) {
        assert!(island_dist(p) >= 3.0 - 0.01, "gouge into the island at {p:?}");
        assert!(p.x >= 3.0 - 0.01 && p.x <= 37.0 + 0.01 && p.y >= 3.0 - 0.01 && p.y <= 27.0 + 0.01, "gouge at {p:?}");
    }
}

#[test]
fn adaptive_refuses_what_it_cannot_cut() {
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    // A drill cannot mill.
    let drill = Tool::new(1, ToolKind::Drill, 6.0, 20.0);
    let params = AdaptiveParams { stepover: 2.5, climb: true, stock_to_leave: 0.0, tolerance: 0.01 };
    assert!(adaptive(&regions, &drill, &params, &cut(0.0, -5.0, 2.0)).is_err());
    // A stepover above the tool diameter.
    let wide = AdaptiveParams { stepover: 7.0, ..params };
    assert!(adaptive(&regions, &tool(6.0), &wide, &cut(0.0, -5.0, 2.0)).is_err());
    // A pocket the tool does not fit in.
    let tight = [rect(0.0, 0.0, 4.0, 4.0)];
    assert!(adaptive(&tight, &tool(6.0), &params, &cut(0.0, -5.0, 2.0)).is_err());
    // No regions at all.
    assert!(adaptive(&[], &tool(6.0), &params, &cut(0.0, -5.0, 2.0)).is_err());
}

#[test]
fn adaptive_splits_a_pocket_at_a_narrow_passage() {
    // Two wide areas joined by a 5 mm-wide neck: with a 6 mm tool, the left
    // area is cleared and the neck is left alone — the tool does not fit into
    // it, and the engine crosses (never clears) a passage. The in-tree pocket
    // operation is the one for pockets a tool only partially fits.
    let regions = [polygon(&[(0.0, 0.0), (30.0, 0.0), (30.0, 12.5), (50.0, 12.5), (50.0, 17.5), (30.0, 17.5), (30.0, 30.0), (0.0, 30.0)])];
    let params = AdaptiveParams { stepover: 2.0, climb: true, stock_to_leave: 0.0, tolerance: 0.01 };
    let path = adaptive(&regions, &tool(6.0), &params, &cut(0.0, -3.0, 3.0)).unwrap();
    for p in cutting_points(&path, 0.0) {
        assert!(p.y >= 3.0 - 0.01 || (p.x >= 27.0 && p.x <= 53.0), "gouge at {p:?}");
        assert!(p.y <= 27.0 + 0.01 || (p.x >= 27.0 && p.x <= 53.0), "gouge at {p:?}");
        assert!(p.x >= 3.0 - 0.01 && p.x <= 47.0 + 0.01, "gouge at {p:?}");
    }
    // The room is cleared to its middle.
    let room = v(8.0, 15.0);
    let d = dist_to_cuts(&path, -3.0 + 1e-6, room);
    assert!(d <= 3.0 + 0.01, "{room:?} is {d} from the nearest pass");
    // The neck is never entered: its middle is farther from every cut than
    // the tool radius, and so is the wall beside it.
    for spot in [v(35.0, 15.0), v(40.0, 10.0)] {
        let d = dist_to_cuts(&path, -3.0 + 1e-6, spot);
        assert!(d > 3.0, "{spot:?} is {d} from the nearest pass");
    }
}

#[test]
fn clearing_runs_in_the_climb_direction_only_until_the_planner_walks_both() {
    // The clearing engine holds a wrong-side safehold in conventional
    // direction (a planner defect, not a milling rule): the operations
    // refuse the request instead of handing the planner a crash.
    let regions = [rect(0.0, 0.0, 40.0, 30.0)];
    let pocket_params = PocketParams { stepover: 2.5, climb: false, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
    let adaptive_params = AdaptiveParams { stepover: 2.5, climb: false, stock_to_leave: 0.0, tolerance: 0.01 };
    let c = cut(0.0, -5.0, 2.0);
    let pocket_refused = pocket(&regions, &tool(6.0), &pocket_params, &c).unwrap_err().to_string();
    assert!(pocket_refused.contains("not supported yet"), "{pocket_refused}");
    let adaptive_refused = adaptive(&regions, &tool(6.0), &adaptive_params, &c).unwrap_err().to_string();
    assert!(adaptive_refused.contains("not supported yet"), "{adaptive_refused}");
    // The same operations run in climb direction.
    let climb = PocketParams { climb: true, ..pocket_params };
    assert!(pocket(&regions, &tool(6.0), &climb, &c).is_ok());
    let climb = AdaptiveParams { climb: true, ..adaptive_params };
    assert!(adaptive(&regions, &tool(6.0), &climb, &c).is_ok());
}

#[test]
fn a_circle_wall_comes_out_as_exact_arcs() {
    // The planner's exact offsetting degenerates on a full-circle arc (its
    // chord is zero), so the geometry never hands it one: every arc past
    // half a turn is written as two halves, and the wall stays true arcs.
    let disc = [Region2 { outer: Loop2::circle(v(0.0, 0.0), 10.0), holes: vec![] }];
    for side in [Side::Outside, Side::Inside] {
        let params = ContourParams { side, climb: true, stock_to_leave: 0.0, tolerance: 0.01, ramp_angle: 0.0 };
        let path = contour(&disc, &tool(4.0), &params, &cut(0.0, -2.0, 2.0)).unwrap();
        let wall_radius = if side == Side::Outside { 12.0 } else { 8.0 };
        let mut arcs = 0;
        for m in &path.moves {
            if let Move::Arc { to, center, .. } = *m {
                arcs += 1;
                close(v(to.x, to.y).dist(center), wall_radius, 1e-6);
                close(v(center.x, center.y).dist(v(0.0, 0.0)), 0.0, 1e-9);
            }
        }
        assert!(arcs >= 4, "{side:?}: {arcs} arcs for two depth passes");
    }
}
