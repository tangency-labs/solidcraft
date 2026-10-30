use super::*;
use crate::Vec3;
use crate::drawing_view::{NamedView, PRESETS};
use crate::profile::Loop2;

const EPS: f64 = 1e-9;

fn near(a: Vec2, b: Vec2) -> bool {
    (a - b).len() < EPS
}

fn scale(paper: f64, model: f64) -> Scale {
    Scale::new(paper, model).unwrap()
}

/// A front view at 1:2, its plane's origin at (100, 150) on paper.
fn parent() -> ViewFrame {
    ViewFrame::new(NamedView::Front.basis(), scale(1.0, 2.0), Vec2::ZERO, Vec2::new(100.0, 150.0)).unwrap()
}

/// A circle of radius 5 about (40, 10) of the parent's plane, at 2:1 with its centre at (300, 100).
fn circle_detail() -> Detail {
    let boundary = Boundary::circle(Vec2::new(40.0, 10.0), 5.0).unwrap();
    Detail::new(&parent(), boundary, scale(2.0, 1.0), Vec2::new(300.0, 100.0)).unwrap()
}

fn circle_of(shape: Shape) -> (Vec2, f64) {
    match shape {
        Shape::Circle { centre, radius } => (centre, radius),
        Shape::Rectangle(_) => panic!("expected a circle, got {shape:?}"),
    }
}

fn rectangle_of(shape: Shape) -> Aabb2 {
    match shape {
        Shape::Rectangle(b) => b,
        Shape::Circle { .. } => panic!("expected a rectangle, got {shape:?}"),
    }
}

/// Worked by hand: the parent halves, so the outline is about (100 + 40/2, 150 + 10/2) with radius
/// 2.5; the detail doubles about its own centre, so the window is about (300, 100) with radius 10.
#[test]
fn a_circular_detail_lands_where_worked_by_hand() {
    let detail = circle_detail();
    let (centre, radius) = circle_of(detail.outline_shape());
    assert!(near(centre, Vec2::new(120.0, 155.0)), "{centre:?}");
    assert!((radius - 2.5).abs() < EPS);
    let (centre, radius) = circle_of(detail.window());
    assert!(near(centre, Vec2::new(300.0, 100.0)), "{centre:?}");
    assert!((radius - 10.0).abs() < EPS);
    assert_eq!(detail.scale(), scale(2.0, 1.0));
    assert_eq!(detail.frame().basis(), parent().basis());

    // A model point 5 mm along the plane's x from the boundary's centre: 10 mm right of the
    // window's centre on the detail, 2.5 mm right of the outline's centre on the parent.
    let basis = parent().basis();
    let model = basis.origin + basis.x * 45.0 + basis.y * 10.0;
    assert!(near(detail.frame().to_paper(model), Vec2::new(310.0, 100.0)));
    assert!(near(parent().to_paper(model), Vec2::new(122.5, 155.0)));
    let map = detail.parent_to_detail(&parent()).unwrap();
    assert!((map.scale() - 4.0).abs() < EPS);
    assert!(near(map.apply(Vec2::new(122.5, 155.0)), Vec2::new(310.0, 100.0)));
}

/// A rectangle given by its corners in either order is the same boundary, and maps corner for
/// corner: the parent's 1:2 halves it about (100, 150), the detail's 5:1 grows it about (50, 60).
#[test]
fn a_rectangular_detail_maps_its_corners() {
    let a = Vec2::new(-10.0, 4.0);
    let b = Vec2::new(30.0, -8.0);
    let boundary = Boundary::rectangle(a, b).unwrap();
    assert_eq!(boundary, Boundary::rectangle(b, a).unwrap());
    assert_eq!(boundary, Boundary::rectangle(Vec2::new(a.x, b.y), Vec2::new(b.x, a.y)).unwrap());
    assert!(near(boundary.centre(), Vec2::new(10.0, -2.0)));
    let detail = Detail::new(&parent(), boundary, scale(5.0, 1.0), Vec2::new(50.0, 60.0)).unwrap();
    let outline = rectangle_of(detail.outline_shape());
    assert!(near(outline.min, Vec2::new(95.0, 146.0)) && near(outline.max, Vec2::new(115.0, 152.0)), "{outline:?}");
    let window = rectangle_of(detail.window());
    assert!(near(window.min, Vec2::new(-50.0, 30.0)) && near(window.max, Vec2::new(150.0, 90.0)), "{window:?}");
    assert!(near(window.center(), Vec2::new(50.0, 60.0)));
}

/// The outline is one closed counter-clockwise loop whose area is the outline's.
#[test]
fn outlines_are_closed_counter_clockwise_loops() {
    let rect = Detail::new(&parent(), Boundary::rectangle(Vec2::new(0.0, 0.0), Vec2::new(8.0, 6.0)).unwrap(), scale(1.0, 1.0), Vec2::ZERO).unwrap();
    for (detail, area) in [(circle_detail(), std::f64::consts::PI * 2.5 * 2.5), (rect, 4.0 * 3.0)] {
        let segs = detail.outline();
        assert!(segs.len() == 2 || segs.len() == 4);
        for (s, next) in segs.iter().zip(segs.iter().cycle().skip(1)) {
            assert!(near(s.end(), next.start()), "{s:?} then {next:?}");
        }
        let lp = Loop2 { segs };
        assert!(lp.is_ccw());
        assert!((lp.signed_area() - area).abs() < 1e-9, "{} against {area}", lp.signed_area());
        // Every point of the outline is on the outline's shape: inside it, and not inside a
        // slightly smaller copy.
        for s in &lp.segs {
            for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
                assert!(detail.outline_shape().bounds().inflated(EPS).contains(s.point_at(t)));
            }
        }
    }
}

/// The parent's curves go onto the detail through the detail's frame: an arc about the boundary's
/// centre lands about the window's centre, its radius scaled by the detail's scale alone.
#[test]
fn parent_curves_move_onto_the_detail() {
    let detail = circle_detail();
    let arc = Seg2::Arc { center: Vec2::new(40.0, 10.0), radius: 3.0, start: 0.3, sweep: 1.2 };
    match detail.frame().seg_to_paper(&arc) {
        Seg2::Arc { center, radius, start, sweep } => {
            assert!(near(center, Vec2::new(300.0, 100.0)));
            assert!((radius - 6.0).abs() < EPS);
            assert!((start - 0.3).abs() < EPS && (sweep - 1.2).abs() < EPS);
        }
        other => panic!("an arc stays an arc, got {other:?}"),
    }
    let line = Seg2::Line { a: Vec2::new(35.0, 10.0), b: Vec2::new(45.0, 10.0) };
    let on_detail = detail.frame().seg_to_paper(&line);
    assert!(near(on_detail.start(), Vec2::new(290.0, 100.0)) && near(on_detail.end(), Vec2::new(310.0, 100.0)));
    // The same line through the parent's paper and the parent-to-detail map lands in the same place.
    let map = detail.parent_to_detail(&parent()).unwrap();
    let via_parent = parent().seg_to_paper(&line).similar(&map);
    assert!(near(via_parent.start(), on_detail.start()) && near(via_parent.end(), on_detail.end()));
}

/// Across every preset scale for both parent and detail, and boundaries off the plane's origin,
/// the outline's centre goes to the window's centre and its size grows by the ratio of the scales.
#[test]
fn window_and_outline_agree_at_every_preset_scale() {
    let boundaries =
        [Boundary::circle(Vec2::new(3.0, -7.0), 0.75).unwrap(), Boundary::rectangle(Vec2::new(-2.0, -1.0), Vec2::new(1.5, 0.5)).unwrap()];
    let mut checked = 0;
    for parent_scale in PRESETS {
        let parent = ViewFrame::new(NamedView::Top.basis(), *parent_scale, Vec2::new(1.0, 2.0), Vec2::new(210.0, 148.5)).unwrap();
        for detail_scale in PRESETS {
            for boundary in boundaries {
                let Ok(detail) = Detail::new(&parent, boundary, *detail_scale, Vec2::new(-40.0, 75.0)) else {
                    // Only a window under 0.01 mm across is refused here: 1.5 mm at 1:200 and below.
                    let across = 1.5 * detail_scale.factor();
                    assert!(across < MIN_WINDOW_MM, "{boundary:?} at {detail_scale} refused, {across} mm across");
                    continue;
                };
                let map = detail.parent_to_detail(&parent).unwrap();
                let ratio = detail_scale.factor() / parent_scale.factor();
                assert!((map.scale() - ratio).abs() <= 1e-12 * ratio);
                let (outline, window) = (detail.outline_shape().bounds(), detail.window().bounds());
                let tol = 1e-9 * (1.0 + window.size().len());
                assert!((map.apply(outline.min) - window.min).len() < tol, "{outline:?} {window:?}");
                assert!((map.apply(outline.max) - window.max).len() < tol, "{outline:?} {window:?}");
                assert!(near(detail.window().centre(), Vec2::new(-40.0, 75.0)));
                checked += 1;
            }
        }
    }
    assert_eq!(checked, 18 * 18 * 2 - 6 * 18 * 2);
}

#[test]
fn contains_takes_the_edge_and_nothing_outside() {
    let circle = Boundary::circle(Vec2::new(1.0, 1.0), 2.0).unwrap();
    assert!(circle.contains(Vec2::new(1.0, 1.0)) && circle.contains(Vec2::new(3.0, 1.0)) && circle.contains(Vec2::new(1.0, -1.0)));
    assert!(!circle.contains(Vec2::new(3.0, 3.0)) && !circle.contains(Vec2::new(3.000001, 1.0)));
    assert!(!circle.contains(Vec2::new(f64::NAN, 1.0)));
    let rect = Boundary::rectangle(Vec2::new(0.0, 0.0), Vec2::new(4.0, 2.0)).unwrap();
    assert!(rect.contains(Vec2::new(0.0, 0.0)) && rect.contains(Vec2::new(4.0, 2.0)) && rect.contains(Vec2::new(2.0, 1.0)));
    assert!(!rect.contains(Vec2::new(4.1, 1.0)) && !rect.contains(Vec2::new(2.0, -0.1)) && !rect.contains(Vec2::new(f64::NAN, 1.0)));
    assert_eq!(circle.shape().bounds(), Aabb2 { min: Vec2::new(-1.0, -1.0), max: Vec2::new(3.0, 3.0) });
}

#[test]
fn hostile_boundaries_are_refused() {
    let bad = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 2e12];
    for v in bad {
        assert!(Boundary::circle(Vec2::new(v, 0.0), 1.0).is_err(), "{v}");
        assert!(Boundary::circle(Vec2::new(0.0, v), 1.0).is_err(), "{v}");
        assert!(Boundary::circle(Vec2::ZERO, v).is_err(), "{v}");
        assert!(Boundary::rectangle(Vec2::new(v, 0.0), Vec2::new(1.0, 1.0)).is_err(), "{v}");
        assert!(Boundary::rectangle(Vec2::ZERO, Vec2::new(1.0, v)).is_err(), "{v}");
    }
    for r in [0.0, -0.0, -1.0, -f64::MIN_POSITIVE] {
        assert!(Boundary::circle(Vec2::ZERO, r).is_err(), "{r}");
    }
    assert!(Boundary::rectangle(Vec2::new(1.0, 0.0), Vec2::new(1.0, 5.0)).is_err());
    assert!(Boundary::rectangle(Vec2::new(0.0, 2.0), Vec2::new(5.0, 2.0)).is_err());
    assert!(Boundary::rectangle(Vec2::ZERO, Vec2::ZERO).is_err());
    assert!(Boundary::circle(Vec2::new(-1e12, 1e12), 1e12).is_ok());
}

#[test]
fn details_that_do_not_fit_on_paper_are_refused() {
    let parent = parent();
    let small = Boundary::circle(Vec2::new(40.0, 10.0), 5.0).unwrap();
    let one = scale(1.0, 1.0);
    // The detail's own position.
    for p in [Vec2::new(5000.1, 0.0), Vec2::new(0.0, -5000.1), Vec2::new(f64::NAN, 0.0), Vec2::new(0.0, f64::INFINITY)] {
        assert!(Detail::new(&parent, small, one, p).is_err(), "{p:?}");
    }
    // A window reaching past 5000 mm: 4000 mm of radius doubled.
    let big = Boundary::circle(Vec2::ZERO, 4000.0).unwrap();
    assert!(Detail::new(&parent, big, scale(2.0, 1.0), Vec2::ZERO).is_err());
    assert!(Detail::new(&parent, big, scale(1.0, 1.0), Vec2::ZERO).is_ok());
    // A window under 0.01 mm across, and one just at it.
    assert!(Detail::new(&parent, Boundary::circle(Vec2::ZERO, 0.004).unwrap(), one, Vec2::ZERO).is_err());
    assert!(Detail::new(&parent, Boundary::circle(Vec2::ZERO, 0.005).unwrap(), one, Vec2::ZERO).is_ok());
    assert!(Detail::new(&parent, Boundary::rectangle(Vec2::ZERO, Vec2::new(1.0, 0.001)).unwrap(), one, Vec2::ZERO).is_err());
    // An outline off the parent's paper: 1e9 mm from the plane's origin at 1:2.
    let far = Boundary::circle(Vec2::new(1e9, 0.0), 1.0).unwrap();
    assert!(Detail::new(&parent, far, one, Vec2::ZERO).is_err());
    // Huge but finite numbers never panic, whichever scale.
    for s in [PRESETS[0], *PRESETS.last().unwrap(), scale(1000.0, 1.0), scale(1.0, 100000.0)] {
        for b in
            [Boundary::circle(Vec2::new(1e12, -1e12), 1e12).unwrap(), Boundary::rectangle(Vec2::new(-1e12, -1e12), Vec2::new(1e12, 1e12)).unwrap()]
        {
            assert!(Detail::new(&parent, b, s, Vec2::ZERO).is_err());
        }
    }
}

/// The anchor sits `gap` outside the outline, along the asked direction from its centre.
#[test]
fn label_anchors_sit_outside_the_outline() {
    let detail = circle_detail();
    // Outline: about (120, 155), radius 2.5.
    assert!(near(detail.label_anchor(0.0, 2.0).unwrap(), Vec2::new(124.5, 155.0)));
    assert!(near(detail.label_anchor(std::f64::consts::FRAC_PI_2, 0.0).unwrap(), Vec2::new(120.0, 157.5)));
    assert!(near(detail.label_anchor(std::f64::consts::PI, 1.0).unwrap(), Vec2::new(116.5, 155.0)));
    // A 20 x 6 outline about (105, 149): straight up meets the top side, 45° the top too (3 < 10).
    let rect =
        Detail::new(&parent(), Boundary::rectangle(Vec2::new(-10.0, -8.0), Vec2::new(30.0, 4.0)).unwrap(), scale(1.0, 1.0), Vec2::ZERO).unwrap();
    assert!(near(rectangle_of(rect.outline_shape()).center(), Vec2::new(105.0, 149.0)));
    assert!(near(rect.label_anchor(std::f64::consts::FRAC_PI_2, 1.0).unwrap(), Vec2::new(105.0, 153.0)));
    assert!(near(rect.label_anchor(0.0, 1.0).unwrap(), Vec2::new(116.0, 149.0)));
    let diagonal = rect.label_anchor(std::f64::consts::FRAC_PI_4, 0.0).unwrap();
    assert!(near(diagonal, Vec2::new(108.0, 152.0)), "{diagonal:?}");
    for (direction, gap) in [(f64::NAN, 1.0), (f64::INFINITY, 1.0), (0.0, -0.1), (0.0, f64::NAN), (0.0, 5000.1), (0.0, f64::INFINITY)] {
        assert!(detail.label_anchor(direction, gap).is_err(), "{direction} {gap}");
    }
    // A gap that would leave the paper.
    assert!(detail.label_anchor(0.0, 4900.0).is_err());
    assert!(detail.label_anchor(0.0, 4800.0).is_ok());
}

#[test]
fn labels_are_one_to_three_capital_letters() {
    for ok in ["A", "Q", "ZZZ", "AB"] {
        assert_eq!(Label::new(ok).unwrap().as_str(), ok);
        assert_eq!(Label::new(ok).unwrap().to_string(), ok);
    }
    for bad in ["", "a", "ABCD", "A1", "É", " A", "A ", "-", "AÉ"] {
        assert!(Label::new(bad).is_err(), "{bad:?}");
    }
}

#[test]
fn next_label_skips_i_o_and_q_and_fills_gaps() {
    let labels = |names: &[&str]| names.iter().map(|n| Label::new(n).unwrap()).collect::<Vec<_>>();
    assert_eq!(next_label(&[]).unwrap().as_str(), "A");
    assert_eq!(next_label(&labels(&["A", "B"])).unwrap().as_str(), "C");
    assert_eq!(next_label(&labels(&["A", "B", "C", "D", "E", "F", "G", "H"])).unwrap().as_str(), "J");
    assert_eq!(next_label(&labels(&["B", "I", "O"])).unwrap().as_str(), "A");
    let first: Vec<Label> = (0..23).map(|n| Label(nth_label(n).unwrap())).collect();
    assert_eq!(first.last().unwrap().as_str(), "Z");
    assert_eq!(next_label(&first).unwrap().as_str(), "AA");
    assert_eq!(nth_label(24).as_deref(), Some("AB"));
    assert_eq!(nth_label(23 + 23).as_deref(), Some("BA"));
}

/// The whole sequence: 23 + 23² + 23³ labels, each valid, each once, none with I, O or Q, ending
/// at ZZZ; with all of them in use, there is no next one.
#[test]
fn the_label_sequence_ends_at_zzz() {
    let all: Vec<String> = (0..).map_while(nth_label).collect();
    assert_eq!(all.len(), 23 + 23 * 23 + 23 * 23 * 23);
    assert_eq!(all.last().map(String::as_str), Some("ZZZ"));
    assert_eq!(all.iter().collect::<HashSet<_>>().len(), all.len());
    assert!(all.iter().all(|l| Label::new(l).is_ok() && !l.contains(['I', 'O', 'Q'])));
    assert_eq!(nth_label(usize::MAX), None);
    let mut used: Vec<Label> = all.into_iter().map(Label).collect();
    assert!(next_label(&used).is_err());
    // Freeing one in the middle makes it the next.
    used.retain(|l| l.as_str() != "MN");
    assert_eq!(next_label(&used).unwrap().as_str(), "MN");
    // Letters outside the sequence (I, O, Q) used by hand don't stop it.
    assert_eq!(next_label(&[Label::new("I").unwrap(), Label::new("OQ").unwrap()]).unwrap().as_str(), "A");
}

/// A model point's place on the detail is its parent place through the parent-to-detail map, for
/// a view that is not principal.
#[test]
fn an_isometric_parent_gives_the_same_mapping() {
    let parent = ViewFrame::new(NamedView::IsoFrontRight.basis(), scale(1.0, 5.0), Vec2::new(-3.0, 4.0), Vec2::new(200.0, 120.0)).unwrap();
    let boundary = Boundary::rectangle(Vec2::new(10.0, 10.0), Vec2::new(30.0, 25.0)).unwrap();
    let detail = Detail::new(&parent, boundary, scale(2.0, 1.0), Vec2::new(350.0, 60.0)).unwrap();
    let map = detail.parent_to_detail(&parent).unwrap();
    for p in [Vec3::new(1.0, 2.0, 3.0), Vec3::new(-50.0, 20.0, 7.5), Vec3::new(0.0, 0.0, 0.0)] {
        let via_parent = map.apply(parent.to_paper(p));
        assert!((via_parent - detail.frame().to_paper(p)).len() < 1e-9, "{p:?}");
    }
    assert!(near(detail.frame().paper_map().apply(boundary.centre()), Vec2::new(350.0, 60.0)));
}
