use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, FRAC_PI_6, PI};

use super::*;
use crate::paper::{Group, ObjectKind, Scene};

fn p(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

/// Each piece's ends, rounded to the nanometre so hand values compare exactly.
fn ends(segs: &[Seg2]) -> Vec<[i64; 4]> {
    let nm = |v: f64| (v * 1e6).round() as i64;
    segs.iter().map(|s| [nm(s.start().x), nm(s.start().y), nm(s.end().x), nm(s.end().y)]).collect()
}

fn piece(x0: f64, y0: f64, x1: f64, y1: f64) -> [i64; 4] {
    ends(&[Seg2::Line { a: p(x0, y0), b: p(x1, y1) }])[0]
}

/// A stroke's pieces, closed or not, and width; a fill's outline with no width.
fn drawn(item: &Item) -> (Vec<[i64; 4]>, bool, Option<f64>) {
    match item {
        Item::Stroke { path, closed, width_mm } => (ends(path), *closed, Some(*width_mm)),
        Item::Fill { region } => (ends(&region.outer.segs), true, None),
    }
}

/// The items on an A3 sheet pass the paper scene's own checks.
fn valid(items: Vec<Item>) {
    Scene { size_mm: p(420.0, 297.0), groups: vec![Group { object: 1, kind: ObjectKind::DrawingBorder, items }] }.validate().unwrap();
}

fn arrow(shape: ArrowShape, length_mm: f64, angle: f64) -> Terminator {
    Terminator::Arrow { shape, length_mm, angle }
}

/// A 3 mm arrowhead at 30° on a line along +x to (10, 0): its base at x = 7, 2 × 3 × tan 15° =
/// 1.608 mm wide, its corners 0.804 either side; filled and closed ones stop the line at the base,
/// an open one at the tip.
#[test]
fn an_arrowhead_by_hand_and_where_it_stops_its_line() {
    let w = 3.0 * (PI / 12.0).tan();
    let (at, from) = (p(10.0, 0.0), p(0.0, 0.0));
    let (filled, stop) = arrow(ArrowShape::Filled, 3.0, FRAC_PI_6).place(at, from, 0.35).unwrap();
    assert_eq!(stop, p(7.0, 0.0));
    assert!((2.0 * w - 1.607695).abs() < 1e-6);
    let triangle = vec![piece(10.0, 0.0, 7.0, w), piece(7.0, w, 7.0, -w), piece(7.0, -w, 10.0, 0.0)];
    assert_eq!(filled.iter().map(drawn).collect::<Vec<_>>(), [(triangle.clone(), true, None)]);
    let (closed, stop) = arrow(ArrowShape::Closed, 3.0, FRAC_PI_6).place(at, from, 0.35).unwrap();
    assert_eq!((closed.iter().map(drawn).collect::<Vec<_>>(), stop), (vec![(triangle, true, Some(0.35))], p(7.0, 0.0)));
    let (open, stop) = arrow(ArrowShape::Open, 3.0, FRAC_PI_6).place(at, from, 0.35).unwrap();
    assert_eq!(open.iter().map(drawn).collect::<Vec<_>>(), [(vec![piece(7.0, w, 10.0, 0.0), piece(10.0, 0.0, 7.0, -w)], false, Some(0.35))]);
    assert_eq!(stop, at);
    // The filled triangle runs anticlockwise, like every loop the scene is given.
    let Item::Fill { region } = &filled[0] else { panic!("a fill") };
    assert!(region.outer.signed_area() > 0.0);
    valid([filled, closed, open].concat());
}

/// Along a slanted line the arrowhead turns with it: a 90° one on a line from (0, 0) to (4, 3) is
/// 5 mm along it, its base at (0, 0) and its corners 5 mm either side.
#[test]
fn an_arrowhead_follows_its_lines_direction() {
    let (items, stop) = arrow(ArrowShape::Filled, 5.0, FRAC_PI_2).place(p(4.0, 3.0), p(-4.0, -3.0), 0.5).unwrap();
    assert_eq!(stop, p(0.0, 0.0));
    let (left, right) = (p(-3.0, 4.0), p(3.0, -4.0));
    let expected = vec![piece(4.0, 3.0, left.x, left.y), piece(left.x, left.y, right.x, right.y), piece(right.x, right.y, 4.0, 3.0)];
    assert_eq!(drawn(&items[0]), (expected, true, None));
}

/// An oblique stroke is centred on its point, 45° anticlockwise from the line, whichever way the
/// line arrives: both ends of a dimension line get the same stroke. A dot is a filled circle; none
/// draws nothing and stops the line at its point.
#[test]
fn oblique_strokes_dots_and_none() {
    let h = 2f64.sqrt() / 2.0;
    for from in [p(-10.0, 0.0), p(10.0, 0.0)] {
        let (items, stop) = Terminator::Oblique { length_mm: 2.0 }.place(p(0.0, 0.0), from, 0.25).unwrap();
        let (segs, closed, width) = drawn(&items[0]);
        let [[x0, y0, x1, y1]] = segs[..] else { panic!("one piece") };
        // Either end may come first; compare them left to right.
        let ends = if x0 < x1 { [x0, y0, x1, y1] } else { [x1, y1, x0, y0] };
        assert_eq!((ends, closed, width, stop), (piece(-h, -h, h, h), false, Some(0.25), p(0.0, 0.0)), "{from:?}");
    }
    let dot = DOT_PER_LINE_WIDTH * 0.25;
    let (items, stop) = Terminator::Dot { diameter_mm: dot }.place(p(5.0, 5.0), p(0.0, 0.0), 0.25).unwrap();
    let Item::Fill { region } = &items[0] else { panic!("a fill") };
    assert_eq!((dot, stop), (1.25, p(5.0, 5.0)));
    assert!((region.outer.signed_area() - PI * 0.625 * 0.625).abs() < 1e-9);
    assert!(region.outer.segs.iter().all(|s| matches!(s, Seg2::Arc { center, radius, .. } if *center == p(5.0, 5.0) && *radius == 0.625)));
    assert_eq!(Terminator::None.place(p(1.0, 1.0), p(0.0, 0.0), 0.25).unwrap(), (vec![], p(1.0, 1.0)));
    valid(items);
}

fn leader(points: &[Vec2], landing_mm: Option<f64>, terminator: Terminator) -> Leader {
    Leader { points: points.to_vec(), landing_mm, terminator, width_mm: 0.25 }
}

/// A leader is its line, through every point, then a landing in the reading direction, then its
/// terminator over the line: to the right unless the leader arrives going left, to the right when
/// it arrives straight up, none without a landing; a filled arrowhead stops the line at its base.
#[test]
fn a_leader_is_its_line_landing_and_terminator() {
    let landing = LANDING_PER_LINE_WIDTH * 0.25;
    let dot = Terminator::Dot { diameter_mm: 1.25 };
    let items = leader(&[p(0.0, 0.0), p(10.0, 10.0), p(20.0, 10.0)], Some(landing), dot).items().unwrap();
    assert_eq!(landing, 5.0);
    let line = vec![piece(0.0, 0.0, 10.0, 10.0), piece(10.0, 10.0, 20.0, 10.0), piece(20.0, 10.0, 25.0, 10.0)];
    assert_eq!(drawn(&items[0]), (line, false, Some(0.25)));
    assert!(matches!(&items[1], Item::Fill { .. }) && items.len() == 2);
    valid(items);
    let left = leader(&[p(50.0, 0.0), p(40.0, 10.0)], Some(5.0), Terminator::None).items().unwrap();
    assert_eq!(drawn(&left[0]).0, [piece(50.0, 0.0, 40.0, 10.0), piece(40.0, 10.0, 35.0, 10.0)]);
    let up = leader(&[p(0.0, 0.0), p(0.0, 20.0)], Some(5.0), arrow(ArrowShape::Filled, 3.0, FRAC_PI_6)).items().unwrap();
    assert_eq!(drawn(&up[0]).0, [piece(0.0, 3.0, 0.0, 20.0), piece(0.0, 20.0, 5.0, 20.0)]);
    let bare = leader(&[p(0.0, 0.0), p(10.0, 5.0)], None, Terminator::None).items().unwrap();
    assert_eq!((drawn(&bare[0]).0, bare.len()), (vec![piece(0.0, 0.0, 10.0, 5.0)], 1));
    // An open arrowhead leaves the line running to its tip, and its strokes take the leader's width.
    let open = leader(&[p(0.0, 0.0), p(10.0, 0.0)], None, arrow(ArrowShape::Open, 3.0, FRAC_PI_6)).items().unwrap();
    assert_eq!((drawn(&open[0]).0, drawn(&open[1]).2), (vec![piece(0.0, 0.0, 10.0, 0.0)], Some(0.25)));
    valid([up, bare, left, open].concat());
}

/// Every check at its bounds, both ways: sizes above 0 and at most 5000 mm, the arrowhead's angle
/// 15° to 90°, 2 to 256 points within the paper's coordinates with no two in a row the same, a
/// direction for the terminator, and a first piece longer than a closed or filled arrowhead.
#[test]
fn every_check_holds_at_its_bounds() {
    let (at, from) = (p(0.0, 0.0), p(-10.0, 0.0));
    let refused = |t: Terminator| t.place(at, from, 0.25).unwrap_err().0;
    for ok in [1e-9, 5000.0] {
        arrow(ArrowShape::Open, ok, FRAC_PI_4).place(at, from, 0.25).unwrap();
        Terminator::Oblique { length_mm: ok }.place(at, from, 0.25).unwrap();
        Terminator::Dot { diameter_mm: ok }.place(at, from, 0.25).unwrap();
    }
    for bad in [0.0, -0.0, -1.0, 5000f64.next_up(), f64::NAN, f64::INFINITY] {
        assert_eq!(refused(arrow(ArrowShape::Open, bad, FRAC_PI_4)), "an arrowhead is above 0 and at most 5000 mm long", "{bad}");
        assert_eq!(refused(Terminator::Oblique { length_mm: bad }), "an oblique stroke is above 0 and at most 5000 mm long", "{bad}");
        assert_eq!(refused(Terminator::Dot { diameter_mm: bad }), "a dot is above 0 and at most 5000 mm across", "{bad}");
        let landing = leader(&[at, p(10.0, 0.0)], Some(bad), Terminator::None).items().unwrap_err().0;
        assert_eq!(landing, "a landing is above 0 and at most 5000 mm long", "{bad}");
    }
    for ok in [PI / 12.0, FRAC_PI_2] {
        arrow(ArrowShape::Filled, 1.0, ok).place(at, from, 0.25).unwrap();
    }
    for bad in [(PI / 12.0).next_down(), FRAC_PI_2.next_up(), 0.0, PI, f64::NAN] {
        assert_eq!(refused(arrow(ArrowShape::Filled, 1.0, bad)), "an arrowhead's included angle is 15° to 90°", "{bad}");
    }
    assert_eq!(Terminator::None.place(at, at, 0.25).unwrap_err().0, "a terminator's line has a direction");
    let run = |n: usize| (0..n).map(|i| p(i as f64, (i % 2) as f64)).collect::<Vec<_>>();
    leader(&run(2), None, Terminator::None).items().unwrap();
    leader(&run(MAX_LEADER_POINTS), None, Terminator::None).items().unwrap();
    for points in [run(0), run(1), run(MAX_LEADER_POINTS + 1)] {
        assert_eq!(leader(&points, None, Terminator::None).items().unwrap_err().0, "a leader has 2 to 256 points", "{}", points.len());
    }
    let twice = leader(&[at, p(5.0, 0.0), p(5.0, 0.0), p(9.0, 3.0)], None, Terminator::None).items().unwrap_err().0;
    assert_eq!(twice, "no two points in a row of a leader are the same");
    // The same point again, but not in a row: a leader may come back on itself.
    leader(&[at, p(5.0, 0.0), p(5.0, 5.0), p(5.0, 0.0)], None, Terminator::None).items().unwrap();
    leader(&[p(1e5, -1e5), at], None, Terminator::None).items().unwrap();
    for bad in [1e5f64.next_up(), f64::NAN, f64::INFINITY] {
        let far = leader(&[p(bad, 0.0), at], None, Terminator::None).items().unwrap_err().0;
        assert_eq!(far, "a leader's points lie within the paper's coordinates", "{bad}");
    }
    let short = |x: f64| leader(&[at, p(x, 0.0)], None, arrow(ArrowShape::Filled, 3.0, FRAC_PI_6)).items();
    assert_eq!(short(3.0).unwrap_err().0, "a leader's first piece is longer than its arrowhead");
    valid(short(3.0001).unwrap());
}
