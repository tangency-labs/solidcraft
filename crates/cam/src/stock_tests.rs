//! Tests of the stock and of facing: closed forms for the stock's intrusion, and facing passes
//! that cover the whole area, enter beside it and refuse hostile input.

use solidcore_record::{Vec2, Vec3};
use solidcraft_geom::Aabb3;
use solidcraft_geom::prelude::*;

use crate::foundation_tests::{close, cylinder_x};
use crate::stock::{FACE_LEAD, FaceParams, Stock, default_face_overhang, default_face_stepover, face, flat_diameter};
use crate::*;

fn face_mill(diameter: f64) -> Tool {
    Tool::new(1, ToolKind::FlatEndmill, diameter, 5.0)
}

fn params(tool: &Tool, angle: f64, zigzag: bool) -> FaceParams {
    FaceParams {
        area: [Vec2::new(-25.0, -25.0), Vec2::new(25.0, 25.0)],
        heights: Heights { clearance: 15.0, retract: 3.0, top: 0.0, bottom: -1.5 },
        feeds: Feeds { cut: 1200.0, plunge: 300.0 },
        stepdown: 0.6,
        stepover: default_face_stepover(tool),
        angle,
        overhang: default_face_overhang(tool),
        zigzag,
        stock: None,
        lead_radius: None,
    }
}

/// The cutting moves of `path` at `z`, as segments seen from above.
fn cuts_at(path: &Toolpath, z: f64) -> Vec<(Vec2, Vec2)> {
    let mut out = Vec::new();
    let mut pos: Option<Vec3> = None;
    for m in &path.moves {
        if let (Move::Feed { to, .. }, Some(from)) = (m, pos)
            && (from.z - z).abs() < 1e-9
            && (to.z - z).abs() < 1e-9
        {
            out.push((Vec2::new(from.x, from.y), Vec2::new(to.x, to.y)));
        }
        pos = m.target().or(pos);
    }
    out
}

fn dist_to_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    let t = if d.len2() > 0.0 { ((p - a).dot(d) / d.len2()).clamp(0.0, 1.0) } else { 0.0 };
    p.dist(a + d * t)
}

/// The distance from `p` to the square `[-h, h]²` (0 inside).
fn dist_to_square(p: Vec2, h: f64) -> f64 {
    let d = Vec2::new((p.x.abs() - h).max(0.0), (p.y.abs() - h).max(0.0));
    d.len()
}

/// Facing a 50 × 50 top with a 32 mm face mill: every point of the area (and the overhang
/// beyond its edges) is under the flat of the cutter on some pass at the final depth, every
/// level, and every descent and every pass start is beside the stock, the whole cutter clear.
#[test]
fn facing_a_square_leaves_no_strip_and_enters_beside_it() {
    let tool = face_mill(32.0);
    for (angle, zigzag) in [(0.0, true), (0.0, false), (30f64.to_radians(), true), (90f64.to_radians(), false)] {
        let p = params(&tool, angle, zigzag);
        let path = face(&tool, &p).unwrap();
        let r = tool.radius();
        // 1.5 mm in levels of at most 0.6: three of 0.5.
        for z in [-0.5, -1.0, -1.5] {
            let cuts = cuts_at(&path, z);
            assert!(!cuts.is_empty(), "angle {angle}: no cuts at {z}");
            // A grid finer than any strip that matters, the edges included.
            for i in 0..=200 {
                for j in 0..=200 {
                    let q = Vec2::new(-25.0 + 0.25 * i as f64, -25.0 + 0.25 * j as f64);
                    let d = cuts.iter().map(|&(a, b)| dist_to_segment(q, a, b)).fold(f64::INFINITY, f64::min);
                    assert!(d <= r - 1e-6, "angle {angle} zigzag {zigzag}: {q:?} is {d} from the passes at {z}");
                }
            }
        }
        // Every move that goes down, and every rapid, is beside the stock with the cutter clear.
        let mut pos: Option<Vec3> = None;
        for m in &path.moves {
            let to = m.target().unwrap();
            let descends = pos.is_some_and(|q| to.z < q.z - 1e-9);
            if descends || matches!(m, Move::Rapid { .. }) && to.z < 0.0 {
                let gap = dist_to_square(Vec2::new(to.x, to.y), 25.0);
                assert!(gap >= r + FACE_LEAD - 1e-6, "angle {angle}: {m:?} goes down {gap} from the stock");
            }
            if let Move::Rapid { to } = m {
                assert!(to.z >= p.heights.retract - 1e-9, "a rapid below the retract height: {m:?}");
            }
            pos = Some(to);
        }
        // Nothing below the bottom; the path ends at the clearance height.
        assert!(path.moves.iter().filter_map(Move::target).all(|q| q.z >= -1.5 - 1e-9));
        close(path.moves.last().and_then(Move::target).unwrap().z, 15.0, 1e-12);
    }
}

/// The outermost passes reach the overhang past the edges, square to the passes; passes are
/// no more than the stepover apart.
#[test]
fn facing_passes_reach_the_overhang_and_keep_the_stepover() {
    let tool = face_mill(10.0);
    let mut p = params(&tool, 0.0, true);
    p.overhang = 2.0;
    p.stepover = 6.0;
    let path = face(&tool, &p).unwrap();
    let mut ys: Vec<f64> =
        cuts_at(&path, -1.5).iter().filter(|(a, b)| (a.y - b.y).abs() < 1e-9 && (a.x - b.x).abs() > 50.0).map(|(a, _)| a.y).collect();
    ys.sort_by(f64::total_cmp);
    ys.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    let (first, last) = (ys.first().copied().unwrap(), ys.last().copied().unwrap());
    close(first - 5.0, -25.0 - 2.0, 1e-9);
    close(last + 5.0, 25.0 + 2.0, 1e-9);
    assert!(ys.windows(2).all(|w| w[1] - w[0] <= 6.0 + 1e-9), "{ys:?}");
    // A bull nose's flat is what the stepover is held to.
    let mut bull = Tool::new(2, ToolKind::BullNose, 20.0, 5.0);
    bull.corner_radius = Some(2.0);
    close(flat_diameter(&bull), 16.0, 1e-12);
    close(default_face_stepover(&bull), 11.2, 1e-12);
    let mut q = params(&bull, 0.0, true);
    q.stepover = 16.5;
    assert!(face(&bull, &q).is_err(), "a stepover wider than the flat leaves ridges");
}

#[test]
fn stock_intrusion_closed_forms() {
    let block = Stock::Box { min: Vec3::new(-25.0, -25.0, -20.0), max: Vec3::new(25.0, 25.0, 0.0) };
    block.validate().unwrap();
    close(block.top(), 0.0, 0.0);
    let down = |x: f64, y: f64, z: f64| Pose::vertical(Vec3::new(x, y, z));
    // Above the top: clear; below it over the box: in by the depth; beside it: clear.
    assert_eq!(block.intrusion(&down(0.0, 0.0, 0.5), 3.0, 0.01), None);
    close(block.intrusion(&down(0.0, 0.0, -2.0), 3.0, 0.01).unwrap(), 1.99, 1e-9);
    assert_eq!(block.intrusion(&down(30.0, 0.0, -2.0), 3.0, 0.01), None);
    // Beside it by less than the radius: in sideways.
    close(block.intrusion(&down(27.0, 0.0, -2.0), 3.0, 0.01).unwrap(), 3.0 - 2.01, 1e-9);
    // Along +Y (an indexed side), the cutter endless towards +Y.
    let side = Pose::new(Vec3::new(0.0, 26.0, -10.0), Vec3::Y);
    assert_eq!(block.intrusion(&side, 3.0, 0.01), None);
    let side = Pose::new(Vec3::new(0.0, 24.0, -10.0), Vec3::Y);
    close(block.intrusion(&side, 3.0, 0.01).unwrap(), 0.99, 1e-9);
    // A tilted cutter well above is clear, and one through the box is in.
    let tilted = Pose::new(Vec3::new(0.0, 0.0, 10.0), Vec3::new(0.0, 0.6, 0.8));
    assert_eq!(block.intrusion(&tilted, 3.0, 0.01), None);
    let tilted = Pose::new(Vec3::new(0.0, 0.0, -10.0), Vec3::new(0.0, 0.6, 0.8));
    assert!(block.intrusion(&tilted, 3.0, 0.01).is_some());

    // A bar of radius 12 about X: a radial tool above it, or beyond its ends, is clear.
    let bar = Stock::Cylinder { radius: 12.0, x_min: -30.0, x_max: 30.0 };
    bar.validate().unwrap();
    close(bar.top(), 12.0, 0.0);
    assert_eq!(bar.intrusion(&down(0.0, 0.0, 13.0), 3.0, 0.01), None);
    close(bar.intrusion(&down(0.0, 0.0, 10.0), 3.0, 0.01).unwrap(), 1.99, 1e-9);
    assert_eq!(bar.intrusion(&down(34.0, 0.0, 0.0), 3.0, 0.01), None);
    // Beside the bar at its axis height, the cutter rising past it: clear when it is outside.
    assert_eq!(bar.intrusion(&down(0.0, 16.0, 0.0), 3.0, 0.01), None);
    close(bar.intrusion(&down(0.0, 14.0, 0.0), 3.0, 0.01).unwrap(), 11.99 - 11.0, 1e-9);
    // A radial tool at A = 90°, along +Y.
    assert_eq!(bar.intrusion(&Pose::new(Vec3::new(0.0, 12.5, 0.0), Vec3::Y), 3.0, 0.01), None);
    close(bar.radius_about_x(Vec2::new(0.0, 0.0)), 12.0, 0.0);
    close(block.radius_about_x(Vec2::new(0.0, 0.0)), 25.0f64.hypot(20.0), 1e-12);

    // Round a part, grown by the margin.
    let part: Aabb3 = [Vec3::new(-10.0, -5.0, -8.0), Vec3::new(10.0, 5.0, 0.0)].into_iter().collect();
    let s = Stock::around(&part, Vec3::new(1.0, 2.0, 0.5)).unwrap();
    assert_eq!(s, Stock::Box { min: Vec3::new(-11.0, -7.0, -8.5), max: Vec3::new(11.0, 7.0, 0.5) });
    assert!(s.holds(&part, 0.0));
    assert!(!bar.holds(&[Vec3::new(-10.0, -10.0, -10.0), Vec3::new(10.0, 10.0, 10.0)].into_iter().collect(), 0.01));
    // A round part in a bar of its own radius: its box's corners stand outside the bar, the
    // part itself does not.
    let round = cylinder_x(12.0, -30.0, 30.0, 48);
    assert!(bar.holds_surface(&round, 0.01));
    assert!(!bar.holds_surface(&cylinder_x(12.5, -30.0, 30.0, 48), 0.01));
    assert!(!bar.holds_surface(&cylinder_x(12.0, -30.0, 31.0, 48), 0.01));
    assert!(s.holds_surface(&crate::foundation_tests::block(Vec3::new(-10.0, -5.0, -8.0), Vec3::new(10.0, 5.0, 0.0)), 0.0));
}

#[test]
fn hostile_stock_and_facing_are_refused() {
    let bad = [
        Stock::Box { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(0.0, 1.0, 1.0) },
        Stock::Box { min: Vec3::new(f64::NAN, 0.0, 0.0), max: Vec3::new(1.0, 1.0, 1.0) },
        Stock::Box { min: Vec3::new(-1e300, 0.0, 0.0), max: Vec3::new(1.0, 1.0, 1.0) },
        Stock::Cylinder { radius: 0.0, x_min: 0.0, x_max: 1.0 },
        Stock::Cylinder { radius: -3.0, x_min: 0.0, x_max: 1.0 },
        Stock::Cylinder { radius: f64::INFINITY, x_min: 0.0, x_max: 1.0 },
        Stock::Cylinder { radius: 3.0, x_min: 1.0, x_max: 1.0 },
    ];
    for s in bad {
        assert!(s.validate().is_err(), "{s:?}");
    }
    let part: Aabb3 = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 1.0)].into_iter().collect();
    assert!(Stock::around(&part, Vec3::new(-1.0, 0.0, 0.0)).is_err());
    assert!(Stock::around(&part, Vec3::new(f64::NAN, 0.0, 0.0)).is_err());
    assert!(Stock::around(&Aabb3::EMPTY, Vec3::new(1.0, 1.0, 1.0)).is_err());
    let flat: Aabb3 = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0)].into_iter().collect();
    assert!(Stock::around(&flat, Vec3::ZERO).is_err());

    let tool = face_mill(32.0);
    let ok = params(&tool, 0.0, true);
    assert!(face(&tool, &ok).is_ok());
    let cases: Vec<Box<dyn Fn(&mut FaceParams)>> = vec![
        Box::new(|p| p.stepover = 0.0),
        Box::new(|p| p.stepover = 40.0),
        Box::new(|p| p.stepover = f64::NAN),
        Box::new(|p| p.stepdown = 0.0),
        Box::new(|p| p.stepdown = 1e-300),
        Box::new(|p| p.stepdown = f64::INFINITY),
        Box::new(|p| p.overhang = -1.0),
        Box::new(|p| p.overhang = 1e9),
        Box::new(|p| p.angle = f64::NAN),
        Box::new(|p| p.angle = 1e300),
        Box::new(|p| p.heights.bottom = 0.0),
        Box::new(|p| p.heights.retract = -5.0),
        Box::new(|p| p.heights.clearance = f64::NAN),
        Box::new(|p| p.area = [Vec2::new(5.0, 5.0), Vec2::new(-5.0, -5.0)]),
        Box::new(|p| p.area = [Vec2::new(-1e300, 0.0), Vec2::new(5.0, 5.0)]),
        Box::new(|p| {
            p.area = [Vec2::new(-1e5, -1e5), Vec2::new(1e5, 1e5)];
            p.stepover = 1e-3;
        }),
        Box::new(|p| p.feeds.cut = 0.0),
    ];
    for (i, f) in cases.iter().enumerate() {
        let mut p = ok;
        f(&mut p);
        assert!(face(&tool, &p).is_err(), "case {i}: {p:?}");
    }
    assert!(face(&Tool::new(1, ToolKind::BallEndmill, 6.0, 5.0), &ok).is_err(), "a ball leaves scallops");
    assert!(face(&Tool::new(1, ToolKind::Drill, 6.0, 5.0), &ok).is_err());
    assert!(face(&Tool::new(1, ToolKind::FlatEndmill, -6.0, 5.0), &ok).is_err());
}

/// A tilted cutter whose axis passes right through a thin plate far from its tip, between where
/// a sampled axis would look, is in it; so is one whose axis crosses a bar's axis far along X.
#[test]
fn a_tilted_cutter_far_through_thin_stock_is_found() {
    let plate = Stock::Box { min: Vec3::new(0.0, 0.0, 0.0), max: Vec3::new(100.0, 100.0, 1.0) };
    let axis = Vec3::new(0.0, 1.0, 1.0).normalized().unwrap();
    // Up the axis from (50, -450, -500): through the plate at about (50, 50, 0..1).
    let through = Pose::new(Vec3::new(50.0, -450.0, -500.0), axis);
    close(plate.intrusion(&through, 3.0, 0.01).unwrap(), 3.0, 1e-6);
    // The same axis 10 mm to the side of the plate: clear; 2 mm: in by 1.
    let beside = Pose::new(Vec3::new(110.0, -450.0, -500.0), axis);
    assert_eq!(plate.intrusion(&beside, 3.0, 0.0), None);
    let near = Pose::new(Vec3::new(102.0, -450.0, -500.0), axis);
    close(plate.intrusion(&near, 3.0, 0.0).unwrap(), 1.0, 1e-6);
    // Through a thin bar's axis 400 mm along X from the tip.
    let bar = Stock::Cylinder { radius: 5.0, x_min: -50.0, x_max: 50.0 };
    let tilted = Pose::new(Vec3::new(-400.0, 0.0, -1600.0 / 3.0), Vec3::new(0.6, 0.0, 0.8));
    close(bar.intrusion(&tilted, 3.0, 0.0).unwrap(), 5.0, 1e-6);
    let clear = Pose::new(Vec3::new(-400.0, 9.0, -1600.0 / 3.0), Vec3::new(0.6, 0.0, 0.8));
    assert_eq!(bar.intrusion(&clear, 3.0, 0.0), None);
}

/// Facing with the job's stock: an area that stops short of the stock along the passes would
/// take the tool down into it, and a bottom below the stock's would cut what holds it; both are
/// refused. An area reaching past the stock, or one narrower across the passes, is fine.
#[test]
fn facing_into_the_stock_or_below_it_is_refused() {
    let tool = face_mill(10.0);
    let stock = Stock::Box { min: Vec3::new(-25.0, -25.0, -20.0), max: Vec3::new(25.0, 25.0, 0.0) };
    let mut p = params(&tool, 0.0, true);
    p.stock = Some(stock);
    assert!(face(&tool, &p).is_ok(), "the whole stock, entered beside it");
    // An area 10 mm inside the stock along X: every descent is in it.
    for zigzag in [true, false] {
        let mut inside = p;
        inside.zigzag = zigzag;
        inside.area = [Vec2::new(-15.0, -25.0), Vec2::new(15.0, 25.0)];
        let e = face(&tool, &inside).unwrap_err().to_string();
        assert!(e.contains("down into the stock"), "{e}");
    }
    // A strip across the passes, reaching past the stock along them: fine.
    let mut strip = p;
    strip.area = [Vec2::new(-25.0, -5.0), Vec2::new(25.0, 5.0)];
    assert!(face(&tool, &strip).is_ok());
    // The same strip at 90°: the passes run along Y, the descents land on the stock.
    strip.angle = 90f64.to_radians();
    assert!(face(&tool, &strip).is_err());
    // A bottom below the stock's.
    let mut deep = p;
    deep.heights.bottom = -21.0;
    deep.stepdown = 5.0;
    assert!(face(&tool, &deep).unwrap_err().to_string().contains("below the stock's bottom"));
    deep.heights.bottom = -20.0;
    assert!(face(&tool, &deep).is_ok());
    // A round bar faced along X: down beside its ends, clear; an area short of its ends is not.
    let bar = Stock::Cylinder { radius: 12.0, x_min: -30.0, x_max: 30.0 };
    let mut round = p;
    round.stock = Some(bar);
    round.area = bar.footprint();
    round.heights = Heights { clearance: 30.0, retract: 14.0, top: 12.0, bottom: 10.0 };
    assert!(face(&tool, &round).is_ok());
    round.area = [Vec2::new(-20.0, -12.0), Vec2::new(20.0, 12.0)];
    assert!(face(&tool, &round).is_err());
    // A hostile stock is refused, not followed.
    let mut bad = p;
    bad.stock = Some(Stock::Box { min: Vec3::new(f64::NAN, 0.0, 0.0), max: Vec3::new(1.0, 1.0, 1.0) });
    assert!(face(&tool, &bad).is_err());
}

/// A face mill faces: its passes go down beside the stock, never into it, and its stepover is
/// held to the flat between its inserts' corners. A chamfer mill and a thread mill do not face.
#[test]
fn a_face_mill_faces_and_its_inserts_corners_set_the_flat() {
    let mut fm = Tool::new(3, ToolKind::FaceMill, 20.0, 5.0);
    fm.corner_radius = Some(0.8);
    close(flat_diameter(&fm), 18.4, 1e-12);
    let path = face(&fm, &params(&fm, 0.0, true)).unwrap();
    assert!(!cuts_at(&path, -1.5).is_empty());
    let square = Tool::new(3, ToolKind::FaceMill, 20.0, 5.0);
    close(flat_diameter(&square), 20.0, 1e-12);
    assert!(face(&square, &params(&square, 0.0, false)).is_ok());
    let mut chamfer = Tool::new(4, ToolKind::Chamfer, 20.0, 5.0);
    chamfer.angle_deg = Some(90.0);
    let thread = Tool::new(5, ToolKind::ThreadMill, 10.0, 5.0);
    for t in [chamfer, thread] {
        let e = face(&t, &params(&t, 0.0, true)).unwrap_err();
        assert!(e.to_string().contains("facing needs a face mill"), "{e}");
    }
}

/// Vertical leads: each level enters down a quarter arc in the vertical plane of the passes
/// from beside the stock, the passes of a level link round half circles outside the area, and
/// the level ends up a quarter arc; still nothing left standing and nothing below the bottom.
#[test]
fn facing_with_vertical_leads_arcs_down_into_and_up_out_of_the_passes() {
    let tool = face_mill(10.0);
    for (angle, plane) in [(0.0, Some(ArcPlane::Zx)), (90f64.to_radians(), Some(ArcPlane::Yz)), (30f64.to_radians(), None)] {
        let p = FaceParams { lead_radius: Some(0.3), ..params(&tool, angle, true) };
        let path = face(&tool, &p).unwrap();
        assert!(crate::check::verify(&path, &p.heights).is_ok());
        let arcs: Vec<(Vec3, ArcPlane)> =
            path.moves.iter().filter_map(|m| if let Move::PlaneArc { to, plane, .. } = m { Some((*to, *plane)) } else { None }).collect();
        match plane {
            // One in and one out a level: three levels.
            Some(pl) => {
                assert_eq!(arcs.len(), 6, "angle {angle}: {arcs:?}");
                assert!(arcs.iter().all(|(_, p)| *p == pl));
            }
            None => assert!(arcs.is_empty()),
        }
        let half_circles = path.moves.iter().filter(|m| matches!(m, Move::Arc { .. })).count();
        let r = tool.radius();
        for z in [-0.5, -1.0, -1.5] {
            let cuts = cuts_at(&path, z);
            for i in 0..=100 {
                for j in 0..=100 {
                    let q = Vec2::new(-25.0 + 0.5 * i as f64, -25.0 + 0.5 * j as f64);
                    let d = cuts.iter().map(|&(a, b)| dist_to_segment(q, a, b)).fold(f64::INFINITY, f64::min);
                    assert!(d <= r - 1e-6, "angle {angle}: {q:?} is {d} from the passes at {z}");
                }
            }
        }
        assert!(half_circles > 0, "the passes link round half circles");
        // Every descent and every link is outside the area with the cutter clear of it.
        let mut pos: Option<Vec3> = None;
        for m in &path.moves {
            let to = m.target().unwrap();
            if pos.is_some_and(|q| to.z < q.z - 1e-9) || matches!(m, Move::Arc { .. }) {
                let gap = dist_to_square(Vec2::new(to.x, to.y), 25.0);
                assert!(gap >= r + FACE_LEAD - 1e-6, "angle {angle}: {m:?} is {gap} from the stock");
            }
            pos = Some(to);
        }
        assert!(path.moves.iter().filter_map(Move::target).all(|q| q.z >= -1.5 - 1e-9));
    }
    // The first descent ends the lead's radius above the first level, the lead's radius behind
    // the pass's start; the arc ends at the start, at depth.
    let p = FaceParams { lead_radius: Some(0.3), ..params(&tool, 0.0, true) };
    let path = face(&tool, &p).unwrap();
    let k = path.moves.iter().position(|m| matches!(m, Move::PlaneArc { .. })).unwrap();
    let before = path.moves[k - 1].target().unwrap();
    let Move::PlaneArc { to, center, .. } = path.moves[k] else { panic!() };
    close(before.z, -0.5 + 0.3, 1e-12);
    close(to.z, -0.5, 1e-12);
    close(to.x - before.x, 0.3, 1e-12);
    close(center.z, -0.2, 1e-12);
    // Hostile radii are refused.
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY, 1e9] {
        assert!(face(&tool, &FaceParams { lead_radius: Some(bad), ..params(&tool, 0.0, true) }).is_err(), "{bad}");
    }
    // A lead that starts above the clearance height: the tool goes up to its start, and every
    // arc starts where the tool is, its radius from its centre.
    for zigzag in [true, false] {
        let p = FaceParams { lead_radius: Some(20.0), ..params(&tool, 0.0, zigzag) };
        let path = face(&tool, &p).unwrap();
        assert!(crate::check::verify(&path, &p.heights).is_ok());
        let mut pos: Option<Vec3> = None;
        for m in &path.moves {
            if let (Some(from), Move::PlaneArc { to, center, plane, ccw, .. }) = (pos, *m) {
                close(crate::path::PlaneArc { from, to, center, plane, ccw }.radius(), 20.0, 1e-9);
            }
            pos = m.target().or(pos);
        }
    }
}
