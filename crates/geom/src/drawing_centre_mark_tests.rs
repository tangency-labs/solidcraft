use super::*;

#[test]
fn diameter_twenty_and_paper_overshoot_at_both_scales() {
    let centre = Vec2::new(30.0, 40.0);
    for (radius, half) in [(10.0, 12.0), (5.0, 7.0)] {
        let lines = strokes(centre, radius, 2.0).unwrap();
        assert_eq!(lines[0], Seg2::Line { a: Vec2::new(30.0 - half, 40.0), b: Vec2::new(30.0 + half, 40.0) });
        assert_eq!(lines[1], Seg2::Line { a: Vec2::new(30.0, 40.0 - half), b: Vec2::new(30.0, 40.0 + half) });
        assert_eq!(lines[0].length(), 2.0 * half);
        assert_eq!(lines[1].length(), 2.0 * half);
    }
}

#[test]
fn invalid_sizes_and_coordinates_are_refused() {
    for radius in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::MAX] {
        assert!(strokes(Vec2::ZERO, radius, 2.0).is_err());
    }
    for overshoot in [-1.0, f64::NAN, f64::INFINITY, MAX_SIDE_MM + 1.0] {
        assert!(strokes(Vec2::ZERO, 10.0, overshoot).is_err());
    }
    for centre in [Vec2::new(f64::NAN, 0.0), Vec2::new(0.0, f64::INFINITY), Vec2::new(MAX_COORDINATE_MM, 0.0)] {
        assert!(strokes(centre, 10.0, 2.0).is_err());
    }
    assert!(strokes(Vec2::ZERO, 1.0, 0.0).is_ok());
    assert!(strokes(Vec2::ZERO, EPS / 2.0, 0.0).is_err());
}

#[test]
fn geometry_is_valid_paper_scene_input_and_moves_with_its_centre() {
    use crate::paper::{Group, Item, ObjectKind, Scene};

    let first = strokes(Vec2::new(25.0, 50.0), 5.0, 2.0).unwrap();
    let moved = strokes(Vec2::new(35.0, 70.0), 5.0, 2.0).unwrap();
    for (a, b) in first.iter().zip(moved) {
        assert_eq!(a.start() + Vec2::new(10.0, 20.0), b.start());
        assert_eq!(a.end() + Vec2::new(10.0, 20.0), b.end());
    }
    let items = first.into_iter().map(|line| Item::Stroke { path: vec![line], closed: false, width_mm: 0.18, pattern: None }).collect();
    let scene = Scene::new(Vec2::new(100.0, 100.0), vec![Group { object: 1, kind: ObjectKind::CentreMark, items }]);
    assert!(scene.validate().is_ok());
}

#[test]
fn a_centre_line_extends_both_ends_of_projected_points() {
    let line = centre_line(Vec2::new(0.0, 0.0), Vec2::new(3.0, 4.0), 5.0).unwrap();
    assert_eq!(line, Seg2::Line { a: Vec2::new(-3.0, -4.0), b: Vec2::new(6.0, 8.0) });
    assert_eq!(line.length(), 15.0);
    assert!(centre_line(Vec2::ZERO, Vec2::ZERO, 2.0).is_err());
    assert!(centre_line(Vec2::ZERO, Vec2::X, -1.0).is_err());
    assert!(centre_line(Vec2::ZERO, Vec2::new(f64::NAN, 0.0), 0.0).is_err());
    assert!(centre_line(Vec2::ZERO, Vec2::new(MAX_COORDINATE_MM, 0.0), 1.0).is_err());
}
