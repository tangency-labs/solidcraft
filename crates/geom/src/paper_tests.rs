use std::f64::consts::{FRAC_PI_2, PI, TAU};

use serde_json::json;

use super::*;
use crate::{Loop2, MAX_BEZIER_CTRL};

fn p(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn line(a: Vec2, b: Vec2) -> Seg2 {
    Seg2::Line { a, b }
}

fn stroke(path: Vec<Seg2>, closed: bool, width_mm: f64) -> Item {
    Item::Stroke { path, closed, width_mm, pattern: None }
}

fn fill(outer: Loop2, holes: Vec<Loop2>) -> Item {
    Item::Fill { region: Region2 { outer, holes } }
}

/// Anticlockwise from `min`.
fn rectangle(min: Vec2, max: Vec2) -> Loop2 {
    Loop2::polygon(&[min, p(max.x, min.y), max, p(min.x, max.y)])
}

/// An A3 landscape sheet with one object's items.
fn sheet(items: Vec<Item>) -> Scene {
    Scene { size_mm: p(420.0, 297.0), groups: vec![Group { object: 1, kind: ObjectKind::DrawingBorder, items }] }
}

fn open(path: Vec<Seg2>) -> Scene {
    sheet(vec![stroke(path, false, 0.35)])
}

fn closed(path: Vec<Seg2>) -> Scene {
    sheet(vec![stroke(path, true, 0.35)])
}

/// ISO 5457's drawing border on an A3 landscape sheet, and the two trimming rectangles at its
/// lower-left corner, by hand.
fn a3() -> Scene {
    sheet(vec![
        stroke(rectangle(p(20.0, 10.0), p(410.0, 287.0)).segs, true, 0.7),
        fill(rectangle(p(0.0, 0.0), p(10.0, 5.0)), vec![]),
        fill(rectangle(p(0.0, 0.0), p(5.0, 10.0)), vec![]),
    ])
}

#[test]
fn an_a3_drawing_border_and_its_trimming_marks_validate_and_round_trip() {
    let scene = a3();
    scene.validate().unwrap();
    let value = serde_json::to_value(&scene).unwrap();
    assert_eq!(value["size_mm"], json!([420.0, 297.0]));
    let group = &value["groups"][0];
    assert_eq!(group["object"], json!(1));
    assert_eq!(group["kind"], "drawing_border");
    let border = &group["items"][0];
    assert_eq!(border["kind"], "stroke");
    assert_eq!(border["closed"], true);
    assert_eq!(border["width_mm"], 0.7);
    assert_eq!(border["path"][0], json!({"type": "line", "a": [20.0, 10.0], "b": [410.0, 10.0]}));
    assert_eq!(border["path"][2], json!({"type": "line", "a": [410.0, 287.0], "b": [20.0, 287.0]}));
    assert_eq!(border["path"][3]["b"], json!([20.0, 10.0]));
    let trim = &group["items"][2];
    assert_eq!(trim["kind"], "fill");
    assert_eq!(trim["region"]["outer"]["segs"][1], json!({"type": "line", "a": [5.0, 0.0], "b": [5.0, 10.0]}));
    assert_eq!(trim["region"]["holes"], json!([]));
    let restored: Scene = serde_json::from_value(value).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored, scene);
}

#[test]
fn each_kind_names_its_layer_in_the_capitals_of_its_id() {
    use ObjectKind::*;
    for (kind, layer) in [
        (SheetBorder, "SHEET_BORDER"),
        (DrawingBorder, "DRAWING_BORDER"),
        (TitleBlock, "TITLE_BLOCK"),
        (PartsList, "PARTS_LIST"),
        (RevisionTable, "REVISION_TABLE"),
        (View, "VIEW"),
    ] {
        assert_eq!(kind.layer(), layer);
        let id = layer.to_ascii_lowercase();
        assert_eq!(serde_json::to_value(kind).unwrap(), json!(id));
        assert_eq!(serde_json::from_value::<ObjectKind>(json!(id)).unwrap(), kind);
    }
}

#[test]
fn each_side_of_the_sheet_is_above_zero_and_at_most_5000_mm() {
    const REFUSED: Invalid = Invalid("each side of the sheet is above 0 and at most 5000 mm");
    // Marks may lie off a small sheet: the scene doesn't clip them.
    for side in [5000.0, 1e-9, f64::MIN_POSITIVE] {
        for size_mm in [p(side, 297.0), p(420.0, side)] {
            Scene { size_mm, ..a3() }.validate().unwrap();
        }
    }
    for side in [0.0, -0.0, -1.0, 5000.001, 5000f64.next_up(), f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for size_mm in [p(side, 297.0), p(420.0, side)] {
            assert_eq!(Scene { size_mm, ..a3() }.validate(), Err(REFUSED), "{size_mm:?}");
        }
    }
}

#[test]
fn a_line_is_above_zero_and_at_most_10_mm_wide() {
    let border = |width| sheet(vec![stroke(rectangle(p(20.0, 10.0), p(410.0, 287.0)).segs, true, width)]);
    for width in [1e-9, 0.7, 10.0] {
        border(width).validate().unwrap();
    }
    for width in [0.0, -0.0, -0.7, 10.000001, 10f64.next_up(), f64::NAN, f64::INFINITY] {
        assert_eq!(border(width).validate(), Err(Invalid("a line is above 0 and at most 10 mm wide")), "{width}");
    }
}

#[test]
fn coordinates_and_radii_are_finite_and_at_most_1e5_mm() {
    const REFUSED: Invalid = Invalid("non-finite or excessive coordinate");
    for v in [1e5, -1e5] {
        open(vec![line(p(0.0, 0.0), p(v, 0.0))]).validate().unwrap();
        open(vec![line(p(0.0, v), p(1.0, 0.0))]).validate().unwrap();
    }
    for v in [1e5 + 1e-6, -1e5 - 1e-6, 1e5f64.next_up(), f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(open(vec![line(p(0.0, 0.0), p(v, 0.0))]).validate(), Err(REFUSED), "{v}");
        assert_eq!(open(vec![line(p(0.0, v), p(1.0, 0.0))]).validate(), Err(REFUSED), "{v}");
        assert_eq!(sheet(vec![fill(rectangle(p(0.0, 0.0), p(v, 1.0)), vec![])]).validate(), Err(REFUSED), "{v}");
    }
    // A radius has the same bound, so a circle's points stay within ±2e5 mm each way.
    let circle = |center, radius| vec![Seg2::Arc { center, radius, start: 0.0, sweep: TAU }];
    closed(circle(p(1e5, -1e5), 1e5)).validate().unwrap();
    for radius in [1e5 + 1e-6, 0.0, -1.0, 1e-13, f64::NAN, f64::INFINITY] {
        assert_eq!(closed(circle(p(0.0, 0.0), radius)).validate(), Err(Invalid("arc radius")), "{radius}");
    }
    assert_eq!(closed(circle(p(1e5 + 1e-6, 0.0), 1.0)).validate(), Err(REFUSED));
}

#[test]
fn every_piece_goes_through_the_section_checks_at_the_papers_bound() {
    let arc = |radius, start, sweep| Seg2::Arc { center: p(50.0, 50.0), radius, start, sweep };
    let conic = |w| Seg2::Conic { a: p(1.0, 0.0), apex: p(1.0, 1.0), b: p(0.0, 1.0), w };
    let one = p(1.0, 1.0);
    for (piece, message) in [
        (arc(0.0, 0.0, PI), "arc radius"),
        (arc(10.0, 1e6 + 1.0, PI), "arc start angle"),
        (arc(10.0, 0.0, 0.0), "arc sweep"),
        (arc(10.0, 0.0, -TAU - 1e-8), "arc sweep"),
        (conic(0.0), "conic weight"),
        (conic(1e12 + 1.0), "conic weight"),
        (line(one, one), "collapsed curve"),
        (Seg2::Cubic { p0: one, p1: one, p2: one, p3: one }, "collapsed curve"),
    ] {
        assert_eq!(open(vec![piece]).validate(), Err(Invalid(message)), "{piece:?}");
    }
    // A conic's weight is a ratio, not a length: the paper's bound doesn't apply to it.
    open(vec![conic(1e12)]).validate().unwrap();
    // A fill's loops go through the same checks: a zero-radius arc in a hole.
    let hole = Loop2 { segs: vec![arc(0.0, 0.0, TAU)] };
    assert_eq!(sheet(vec![fill(rectangle(p(0.0, 0.0), p(100.0, 100.0)), vec![hole])]).validate(), Err(Invalid("arc radius")));
    // A Bézier's unused slots are checked at the paper's bound too, as they are serialized.
    let mut controls = [Vec2::ZERO; MAX_BEZIER_CTRL];
    for (i, q) in controls.iter_mut().enumerate().take(5) {
        *q = p(i as f64, (i % 2) as f64);
    }
    open(vec![Seg2::Bezier { n: 4, p: controls }]).validate().unwrap();
    for value in [f64::NAN, f64::INFINITY, 1e5 + 1e-6] {
        let mut hidden = controls;
        hidden[MAX_BEZIER_CTRL - 1] = p(value, 0.0);
        assert_eq!(open(vec![Seg2::Bezier { n: 4, p: hidden }]).validate(), Err(Invalid("non-finite or excessive coordinate")), "{value}");
    }
    for n in [3, 10] {
        assert_eq!(open(vec![Seg2::Bezier { n, p: controls }]).validate(), Err(Invalid("Bezier degree")));
    }
}

#[test]
fn a_strokes_pieces_meet_within_1e_9_mm_and_a_closed_one_ends_where_it_starts() {
    const APART: Invalid = Invalid("a stroke's pieces meet end to start");
    const OPEN_ENDS: Invalid = Invalid("a closed stroke ends where it starts");
    // The joins lie at the origin, where a gap of 1e-9 mm is exact.
    let corner = |gap| vec![line(p(-10.0, 0.0), p(0.0, 0.0)), line(p(gap, 0.0), p(0.0, 10.0))];
    open(corner(1e-9)).validate().unwrap();
    for gap in [2e-9, 1e-9f64.next_up(), -2e-9] {
        assert_eq!(open(corner(gap)).validate(), Err(APART), "{gap}");
    }
    let square = |gap| vec![line(p(0.0, 0.0), p(10.0, 0.0)), line(p(10.0, 0.0), p(10.0, 10.0)), line(p(10.0, 10.0), p(0.0, gap))];
    closed(square(1e-9)).validate().unwrap();
    for gap in [2e-9, 1e-9f64.next_up(), 10.0] {
        assert_eq!(closed(square(gap)).validate(), Err(OPEN_ENDS), "{gap}");
        open(square(gap)).validate().unwrap();
    }
}

#[test]
fn stroke_shapes_one_piece_reversed_duplicated_and_doubled_back() {
    let a = line(p(0.0, 0.0), p(10.0, 0.0));
    let border = rectangle(p(20.0, 10.0), p(410.0, 287.0));
    open(vec![a]).validate().unwrap();
    assert_eq!(closed(vec![a]).validate(), Err(Invalid("a closed stroke ends where it starts")));
    assert_eq!(open(vec![]).validate(), Err(Invalid("a stroke has at least one piece")));
    assert_eq!(closed(vec![]).validate(), Err(Invalid("a stroke has at least one piece")));
    // A whole circle as one piece, or as two halves.
    closed(vec![Seg2::Arc { center: p(100.0, 100.0), radius: 5.0, start: 0.3, sweep: -TAU }]).validate().unwrap();
    closed(Loop2::circle(p(100.0, 100.0), 5.0).segs).validate().unwrap();
    closed(border.reversed().segs).validate().unwrap();
    // A piece twice over doesn't join; out of order neither.
    assert_eq!(open(vec![a, a]).validate(), Err(Invalid("a stroke's pieces meet end to start")));
    let mut swapped = border.segs.clone();
    swapped.swap(1, 2);
    assert_eq!(closed(swapped).validate(), Err(Invalid("a stroke's pieces meet end to start")));
    // Doubling back joins, so it is accepted: the scene doesn't certify a path.
    open(vec![a, a.reversed()]).validate().unwrap();
    closed(vec![a, a.reversed()]).validate().unwrap();
    // Nothing to draw is a valid scene, and so is an object that drew nothing.
    Scene { groups: vec![], ..a3() }.validate().unwrap();
    sheet(vec![]).validate().unwrap();
}

#[test]
fn every_loop_of_a_fill_is_closed_within_1e_9_mm() {
    const OPEN: Invalid = Invalid("each loop of a fill is closed");
    let outer = rectangle(p(0.0, 0.0), p(100.0, 100.0));
    // A triangle whose last piece ends `gap` from its start, at the origin.
    let triangle = |gap| Loop2 { segs: vec![line(p(0.0, 0.0), p(10.0, 0.0)), line(p(10.0, 0.0), p(0.0, 10.0)), line(p(0.0, 10.0), p(0.0, gap))] };
    sheet(vec![fill(triangle(1e-9), vec![])]).validate().unwrap();
    let hole = |gap: f64| triangle(gap).reversed();
    sheet(vec![fill(outer.clone(), vec![hole(1e-9)])]).validate().unwrap();
    for gap in [2e-9, 1e-9f64.next_up(), 5.0] {
        assert_eq!(sheet(vec![fill(triangle(gap), vec![])]).validate(), Err(OPEN), "{gap}");
        assert_eq!(sheet(vec![fill(outer.clone(), vec![hole(gap)])]).validate(), Err(OPEN), "{gap}");
    }
    assert_eq!(sheet(vec![fill(Loop2::default(), vec![])]).validate(), Err(OPEN));
    assert_eq!(sheet(vec![fill(outer.clone(), vec![Loop2::default()])]).validate(), Err(OPEN));
    // Even-odd: a hole that runs the same way as its outer loop, and a round hole, are accepted.
    sheet(vec![fill(outer.clone(), vec![rectangle(p(10.0, 10.0), p(20.0, 20.0))])]).validate().unwrap();
    sheet(vec![fill(outer, vec![Loop2::circle(p(50.0, 50.0), 5.0).reversed()])]).validate().unwrap();
}

#[test]
fn at_most_100_000_records_are_counted_before_any_geometry() {
    const REFUSED: Invalid = Invalid("more than 100,000 groups, items, loops and pieces");
    let group = |items| Group { object: 7, kind: ObjectKind::PartsList, items };
    let scene = |groups| Scene { size_mm: p(420.0, 297.0), groups };
    // Groups alone.
    scene(vec![group(vec![]); MAX_RECORDS]).validate().unwrap();
    assert_eq!(scene(vec![group(vec![]); MAX_RECORDS + 1]).validate(), Err(REFUSED));
    // One group and one stroke, then its pieces.
    let steps = |n: usize| -> Vec<Seg2> { (0..n).map(|i| line(p(i as f64 * 1e-3, 0.0), p((i + 1) as f64 * 1e-3, 0.0))).collect() };
    scene(vec![group(vec![stroke(steps(MAX_RECORDS - 2), false, 0.35)])]).validate().unwrap();
    assert_eq!(group(vec![stroke(steps(MAX_RECORDS - 2), false, 0.35)]).records(), MAX_RECORDS);
    assert_eq!(scene(vec![group(vec![stroke(steps(MAX_RECORDS - 1), false, 0.35)])]).validate(), Err(REFUSED));
    // A fill counts each loop as well as its pieces: 2 + (1 + 5) + 49,996 × (1 + 1) = 100,000.
    let holes = vec![Loop2 { segs: vec![Seg2::Arc { center: p(200.0, 150.0), radius: 1.0, start: 0.0, sweep: TAU }] }; 49_996];
    let polygon = |n: usize| {
        let corners: Vec<Vec2> = (0..n).map(|i| p(200.0, 150.0) + Vec2::from_angle(TAU * i as f64 / n as f64) * 100.0).collect();
        Loop2::polygon(&corners)
    };
    scene(vec![group(vec![fill(polygon(5), holes.clone())])]).validate().unwrap();
    assert_eq!(group(vec![fill(polygon(5), holes.clone())]).records(), MAX_RECORDS);
    assert_eq!(scene(vec![group(vec![fill(polygon(6), holes)])]).validate(), Err(REFUSED));
    // Counted first: an over-budget scene with a non-finite piece reports the count.
    let mut bad = steps(MAX_RECORDS - 1);
    bad[0] = line(p(f64::NAN, 0.0), p(1.0, 0.0));
    assert_eq!(Scene { size_mm: p(f64::NAN, 0.0), ..scene(vec![group(vec![stroke(bad, false, f64::NAN)])]) }.validate(), Err(REFUSED));
    let mut count = Budget(MAX_RECORDS);
    assert_eq!(count.add(usize::MAX), Err(REFUSED));
}

#[test]
fn unknown_fields_kinds_and_ids_are_refused_when_read() {
    let value = serde_json::to_value(a3()).unwrap();
    let read = |v: serde_json::Value| serde_json::from_value::<Scene>(v);
    read(value.clone()).unwrap();
    for at in ["", "/groups/0", "/groups/0/items/0", "/groups/0/items/1"] {
        let mut v = value.clone();
        v.pointer_mut(at).unwrap()["layer"] = json!("A");
        assert!(read(v).is_err(), "an unknown field at {at:?}");
    }
    let mut v = value.clone();
    v["groups"][0]["items"][0]["kind"] = json!("text");
    assert!(read(v).is_err());
    for kind in ["frame", "Title Block", "TITLE_BLOCK", "titleblock", ""] {
        let mut v = value.clone();
        v["groups"][0]["kind"] = json!(kind);
        assert!(read(v).is_err(), "{kind:?}");
    }
    // The id is any u64 and nothing else.
    for object in [json!(-1), json!(1.5), json!("1"), json!(null)] {
        let mut v = value.clone();
        v["groups"][0]["object"] = object;
        assert!(read(v).is_err());
    }
    let mut v = value.clone();
    v["groups"][0]["object"] = json!("ID");
    let with = |id: &str| serde_json::from_str::<Scene>(&serde_json::to_string(&v).unwrap().replace("\"ID\"", id));
    with("1").unwrap();
    assert!(with("18446744073709551616").is_err());
    v["groups"][0].as_object_mut().unwrap().remove("object");
    assert!(read(v).is_err());
    let mut v = value;
    v["groups"][0]["object"] = json!(u64::MAX);
    read(v).unwrap().validate().unwrap();
}

/// A grid counts whole steps: half a step and less rounds to the nearer, -0 is 0, and a number
/// too large or not a number is refused. A grid has 1 to 1e9 steps a millimetre.
#[test]
fn a_grid_counts_whole_steps_and_refuses_what_it_cannot_hold() {
    let nm = Grid::NANOMETRE;
    assert_eq!((nm.steps(-0.0).unwrap(), nm.steps(4e-7).unwrap(), nm.steps(6e-7).unwrap()), (0, 0, 1));
    assert_eq!(nm.steps(9e9).unwrap(), 9_000_000_000_000_000);
    for v in [f64::NAN, f64::INFINITY, 1e10, -1e10] {
        assert!(nm.steps(v).is_err(), "{v}");
    }
    for per_mm in [1.0, 2048.0, 1e9] {
        assert_eq!(Grid::new(per_mm).map(Grid::per_mm), Some(per_mm));
    }
    for per_mm in [0.0, 0.999, 1e9 + 1.0, -2048.0, f64::NAN, f64::INFINITY] {
        assert!(Grid::new(per_mm).is_none(), "{per_mm}");
    }
}

/// The A3 border on the nanometre grid: y up as in the scene, the closing line left to the
/// writer's close, each fill loop closed, nothing snapped or merged.
#[test]
fn a_scene_on_the_grid_keeps_y_up_and_leaves_the_closing_line_to_the_writer() {
    let on = a3().prepare().unwrap().on_grid(Grid::NANOMETRE).unwrap();
    let g = |x: i64, y: i64| GridPoint { x: x * 1_000_000, y: y * 1_000_000 };
    assert_eq!(on.size, g(420, 297));
    let group = &on.groups[0];
    assert_eq!((group.object, group.kind, group.items.len()), (1, ObjectKind::DrawingBorder, 3));
    let GridItem::Stroke { width, path, dashes: None } = &group.items[0] else { panic!("{:?}", group.items[0]) };
    assert_eq!(*width, 700_000);
    let lines = |points: &[GridPoint]| points.iter().map(|&to| Piece::Line { to }).collect::<Vec<_>>();
    assert_eq!(*path, GridPath { start: g(20, 10), pieces: lines(&[g(410, 10), g(410, 287), g(20, 287)]), closed: true });
    let GridItem::Fill { loops } = &group.items[1] else { panic!("{:?}", group.items[1]) };
    assert_eq!(*loops, [GridPath { start: g(0, 0), pieces: lines(&[g(10, 0), g(10, 5), g(0, 5)]), closed: true }]);
    assert_eq!(on.report, GridReport::default());
}

/// Slivers are measured in the grid's own steps: a 5 mm piece goes straight on to the next on a
/// 1 mm grid, and stays on the nanometre grid. An arc's quarters keep its radius and turn, in
/// steps; a width under half a step is one step. A curve the walk can't take yet is refused.
#[test]
fn slivers_and_radii_are_in_the_grids_own_steps() {
    let millimetre = Grid::new(1.0).unwrap();
    let walk = |scene: &Scene, grid: Grid| scene.prepare().unwrap().on_grid(grid).unwrap();
    let path = |on: &OnGrid| match &on.groups[0].items[0] {
        GridItem::Stroke { width, path, .. } => (*width, path.clone()),
        other => panic!("{other:?}"),
    };
    let three = open(vec![line(p(0.0, 0.0), p(100.0, 0.0)), line(p(100.0, 0.0), p(105.0, 0.0)), line(p(105.0, 0.0), p(200.0, 0.0))]);
    let coarse = walk(&three, millimetre);
    let to = |x: i64| Piece::Line { to: GridPoint { x, y: 0 } };
    assert_eq!(path(&coarse).1.pieces, [to(100), to(200)]);
    assert_eq!(coarse.report, GridReport { widths_snapped: 1, pieces_merged: 1, dashes_snapped: 0 }, "0.35 mm is under half a 1 mm step");
    assert_eq!(path(&coarse).0, 1);
    // The boundary both ways: 10 steps along x merge, 11 don't; a (6, 8) step merges, (7, 8) doesn't.
    for (dx, dy, merged) in [(10.0, 0.0, 1), (11.0, 0.0, 0), (6.0, 8.0, 1), (7.0, 8.0, 0)] {
        let (a, b) = (p(100.0, 0.0), p(100.0 + dx, dy));
        let scene = open(vec![line(p(0.0, 0.0), a), line(a, b), line(b, p(200.0, 50.0))]);
        assert_eq!(walk(&scene, millimetre).report.pieces_merged, merged, "({dx}, {dy})");
    }
    let fine = walk(&three, Grid::NANOMETRE);
    assert_eq!((path(&fine).1.pieces.len(), fine.report), (3, GridReport::default()));
    // A clockwise half turn of radius 10 round (50, 50) from the top: two quarters.
    let half = open(vec![Seg2::Arc { center: p(50.0, 50.0), radius: 10.0, start: FRAC_PI_2, sweep: -PI }]);
    let arc = |x, y| Piece::Arc { radius: 10, clockwise: true, to: GridPoint { x, y } };
    assert_eq!(
        path(&walk(&half, millimetre)).1,
        GridPath { start: GridPoint { x: 50, y: 60 }, pieces: vec![arc(60, 50), arc(50, 40)], closed: false }
    );
    let ellipse = open(vec![Seg2::Conic { a: p(0.0, 0.0), apex: p(10.0, 10.0), b: p(20.0, 0.0), w: 0.5 }]);
    let refused = ellipse.prepare().unwrap().on_grid(Grid::NANOMETRE).unwrap_err();
    assert!(refused.0.contains("not supported yet"), "{refused}");
}

fn dashed(path: Vec<Seg2>, closed: bool, lengths_mm: &[f64], phase_mm: f64) -> Scene {
    sheet(vec![Item::Stroke { path, closed, width_mm: 0.35, pattern: Some(Dashes { lengths_mm: lengths_mm.to_vec(), phase_mm }) }])
}

/// A pattern has 2 to 12 lengths, an even count, each above 0 and at most 1000 mm; its phase is
/// from 0 up to, not including, the pattern's length.
#[test]
fn a_dash_pattern_is_an_even_2_to_12_lengths_with_its_phase_inside_it() {
    let pattern = |lengths: &[f64], phase| dashed(vec![line(p(0.0, 0.0), p(100.0, 0.0))], false, lengths, phase).validate();
    for n in [2, 4, 12] {
        pattern(&vec![1.0; n], 0.0).unwrap();
    }
    for n in [0, 1, 3, 11, 13, 14] {
        assert_eq!(pattern(&vec![1.0; n], 0.0), Err(Invalid("a dash pattern has an even count of 2 to 12 lengths")), "{n}");
    }
    for length in [1e-9, 1000.0] {
        pattern(&[length, 1.0], 0.0).unwrap();
        pattern(&[1.0, length], 0.0).unwrap();
    }
    for length in [0.0, -0.0, -1.0, 1000f64.next_up(), f64::NAN, f64::INFINITY] {
        assert_eq!(pattern(&[1.0, length], 0.0), Err(Invalid("each dash and gap is above 0 and at most 1000 mm")), "{length}");
    }
    for phase in [0.0, -0.0, 1.5, 3f64.next_down()] {
        pattern(&[1.0, 2.0], phase).unwrap();
    }
    for phase in [3.0, (-0.0f64).next_down(), -1.0, 4.0, f64::NAN, f64::INFINITY] {
        assert_eq!(pattern(&[1.0, 2.0], phase), Err(Invalid("a dash pattern's phase is from 0 up to its length")), "{phase}");
    }
}

/// A dashed line needs at most a million dashes, counted as pairs along its length. A line of
/// exactly a million patterns of 2⁻¹³ mm is the bound; an arc counts its own length, not its
/// chord's.
#[test]
fn a_dashed_line_needs_at_most_a_million_dashes() {
    const TOO_MANY: Invalid = Invalid("a dashed line needs at most 1,000,000 dashes");
    let half = 2f64.powi(-14);
    let along = |x: f64, lengths: &[f64]| dashed(vec![line(p(0.0, 0.0), p(x, 0.0))], false, lengths, 0.0).validate();
    along(122.0703125, &[half, half]).unwrap();
    assert_eq!(along(122.0703125f64.next_up(), &[half, half]), Err(TOO_MANY));
    // Two dashes a pattern: the same length is twice the count.
    assert_eq!(along(122.0703125, &[half / 2.0; 4]), Err(TOO_MANY));
    along(61.03515625, &[half / 2.0; 4]).unwrap();
    // A quarter turn of radius 100 is 157 mm long; its chord is 141 mm.
    let quarter = vec![Seg2::Arc { center: p(0.0, 0.0), radius: 100.0, start: 0.0, sweep: FRAC_PI_2 }];
    assert_eq!(dashed(quarter, false, &[7.5e-5, 7.5e-5], 0.0).validate(), Err(TOO_MANY));
    along(141.5, &[7.5e-5, 7.5e-5]).unwrap();
}

/// A pattern is written only when a stroke has one, so a solid stroke reads and writes as before;
/// unknown fields in it are refused.
#[test]
fn a_pattern_round_trips_and_a_solid_stroke_has_none() {
    let scene = dashed(rectangle(p(20.0, 10.0), p(410.0, 287.0)).segs, true, &[4.2, 1.05], 0.5);
    scene.validate().unwrap();
    let value = serde_json::to_value(&scene).unwrap();
    assert_eq!(value["groups"][0]["items"][0]["pattern"], json!({"lengths_mm": [4.2, 1.05], "phase_mm": 0.5}));
    assert_eq!(serde_json::from_value::<Scene>(value.clone()).unwrap(), scene);
    let mut v = value.clone();
    v["groups"][0]["items"][0]["pattern"]["unit"] = json!("mm");
    assert!(serde_json::from_value::<Scene>(v).is_err());
    let solid = serde_json::to_value(a3()).unwrap();
    assert!(solid["groups"][0]["items"][0].get("pattern").is_none());
}

/// On the grid a pattern is in whole steps: a length under half a step is one step, counted, and
/// the phase, rounded on its own, is taken within the rounded pattern.
#[test]
fn a_pattern_on_the_grid_is_in_whole_steps() {
    let stroke = |lengths: &[f64], phase, grid| {
        let on = dashed(vec![line(p(0.0, 0.0), p(100.0, 0.0))], false, lengths, phase).prepare().unwrap().on_grid(grid).unwrap();
        let GridItem::Stroke { dashes: Some(dashes), .. } = &on.groups[0].items[0] else { panic!("{:?}", on.groups[0].items[0]) };
        (dashes.clone(), on.report.dashes_snapped)
    };
    let millimetre = Grid::new(1.0).unwrap();
    let grid_dashes = |lengths: &[i64], phase| GridDashes { lengths: lengths.to_vec(), phase };
    assert_eq!(stroke(&[4.2, 1.05], 0.5, Grid::NANOMETRE), (grid_dashes(&[4_200_000, 1_050_000], 500_000), 0));
    assert_eq!(stroke(&[4.2, 1.05], 0.5, millimetre), (grid_dashes(&[4, 1], 1), 0));
    assert_eq!(stroke(&[0.4, 0.3, 2.0, 0.5], 0.0, millimetre), (grid_dashes(&[1, 1, 2, 1], 0), 2));
    // 2.7 rounds to 3, past the rounded pattern's 2: a step into it.
    assert_eq!(stroke(&[1.4, 1.4], 2.7, millimetre), (grid_dashes(&[1, 1], 1), 0));
}
