//! A dimension from a link keeps its side when the link moves: the solve after the links are
//! refreshed reads sides and turns where the points were stored ([`Sketch::positions_in`]),
//! not beside a line that has already moved past them.

use super::*;
use crate::{ConstraintKind, solve, solve_after_edit};

fn source() -> LinkSource {
    LinkSource::SketchCurve { sketch: 1, curve: "rail".into() }
}

fn rail(height: f64) -> Vec<LinkGeom> {
    vec![LinkGeom::Line(Vec2::new(0.0, height), Vec2::new(80.0, height))]
}

/// What the evaluation does with a stored sketch whose link's source moved: the link goes
/// where the source is, then the sketch is solved from its stored state. `kept`: the solve is
/// given the stored positions.
fn moved(stored: &Sketch, link: &str, geom: &[LinkGeom], kept: bool) -> Sketch {
    let mut sk = stored.clone();
    assert!(!sk.update_link(link, geom).unwrap(), "the link keeps its points");
    let before = if kept { sk.positions_in(stored) } else { None };
    assert!(solve_after_edit(&mut sk, &stored.driving_values(), before.as_deref()).keeps_orientation());
    sk
}

fn near(at: Vec2, x: f64, y: f64) -> bool {
    at.dist(Vec2::new(x, y)) < 1e-9
}

#[test]
fn a_point_dimensioned_from_a_linked_line_keeps_its_side_when_the_line_moves_past() {
    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Project, source(), &rail(50.0)).unwrap();
    let l = sk.link_curves(&link)[0];
    let p = sk.add_point(Vec2::new(40.0, 45.0), None).unwrap();
    sk.add_constraint(ConstraintKind::DistanceX { p: PointIdx::ORIGIN, q: p, value: 40.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::PointLineDistance { p, l, value: 5.0 }, None).unwrap();
    assert!(solve(&mut sk).ok());
    // Past the point (20, 6, 44), away from it (400) and not as far as it (48).
    for height in [20.0, 6.0, 44.0, 400.0, 48.0] {
        let at = moved(&sk, &link, &rail(height), true).points[p].pos;
        assert!(near(at, 40.0, height - 5.0), "{height}: {at:?}");
    }
    // Read where the line is now, the stored point is above it: the solution on that side.
    let at = moved(&sk, &link, &rail(20.0), false).points[p].pos;
    assert!(near(at, 40.0, 25.0), "{at:?}");
}

#[test]
fn a_dimension_along_an_axis_keeps_its_side_of_a_linked_point_that_moves_past() {
    for upright in [true, false] {
        let end = |h: f64| vec![LinkGeom::Point(if upright { Vec2::new(80.0, h) } else { Vec2::new(h, 80.0) })];
        let mut sk = Sketch::new();
        let link = sk.add_link(LinkKind::Project, LinkSource::SketchPoint { sketch: 1, point: "end".into() }, &end(50.0)).unwrap();
        let q = sk.link_points(&link)[0];
        let p = sk.add_point(if upright { Vec2::new(40.0, 45.0) } else { Vec2::new(45.0, 40.0) }, None).unwrap();
        if upright {
            sk.add_constraint(ConstraintKind::DistanceX { p: PointIdx::ORIGIN, q: p, value: 40.0 }, None).unwrap();
            sk.add_constraint(ConstraintKind::DistanceY { p: q, q: p, value: 5.0 }, None).unwrap();
        } else {
            sk.add_constraint(ConstraintKind::DistanceY { p: PointIdx::ORIGIN, q: p, value: 40.0 }, None).unwrap();
            sk.add_constraint(ConstraintKind::DistanceX { p: q, q: p, value: 5.0 }, None).unwrap();
        }
        assert!(solve(&mut sk).ok());
        for height in [20.0, 6.0, 400.0] {
            let at = moved(&sk, &link, &end(height), true).points[p].pos;
            let (along, across) = if upright { (at.y, at.x) } else { (at.x, at.y) };
            assert!((along - (height - 5.0)).abs() < 1e-9 && (across - 40.0).abs() < 1e-9, "upright {upright}, {height}: {at:?}");
        }
        let at = moved(&sk, &link, &end(20.0), false).points[p].pos;
        assert!(((if upright { at.y } else { at.x }) - 25.0).abs() < 1e-9, "upright {upright}: {at:?}");
    }
}

/// A corner 50 from both ends of a linked line: below the line, or its mirror image above.
#[test]
fn a_corner_dimensioned_from_two_linked_points_keeps_its_turn_when_they_move_past() {
    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Project, source(), &rail(50.0)).unwrap();
    let ends = sk.link_points(&link);
    let p = sk.add_point(Vec2::new(40.0, 20.0), None).unwrap();
    for end in ends {
        sk.add_constraint(ConstraintKind::Distance { p: end, q: p, value: 50.0 }, None).unwrap();
    }
    assert!(solve(&mut sk).ok());
    assert!(near(sk.points[p].pos, 40.0, 20.0));
    for height in [10.0, -200.0, 300.0] {
        let at = moved(&sk, &link, &rail(height), true).points[p].pos;
        assert!(near(at, 40.0, height - 30.0), "{height}: {at:?}");
    }
    let at = moved(&sk, &link, &rail(10.0), false).points[p].pos;
    assert!(near(at, 40.0, 40.0), "{at:?}");
}

#[test]
fn stored_positions_are_matched_by_point_id() {
    let mut stored = Sketch::new();
    let link = stored.add_link(LinkKind::Project, source(), &rail(50.0)).unwrap();
    let p = stored.add_point(Vec2::new(40.0, 45.0), Some("p")).unwrap();
    assert_eq!(stored.positions_in(&stored), None, "nothing moved");

    let mut sk = stored.clone();
    sk.update_link(&link, &rail(20.0)).unwrap();
    let was: Vec<Vec2> = stored.points.iter().map(|q| q.pos).collect();
    assert_eq!(sk.positions_in(&stored), Some(was));

    // A link of another shape is built anew: its points are new, and the points after it
    // change places in the list.
    let mut sk = stored.clone();
    let bent = [LinkGeom::Line(Vec2::new(0.0, 20.0), Vec2::new(40.0, 30.0)), LinkGeom::Line(Vec2::new(40.0, 30.0), Vec2::new(80.0, 20.0))];
    assert!(sk.update_link(&link, &bent).unwrap());
    let now = sk.point_index("p").unwrap();
    assert_ne!(now, p);
    assert_eq!(sk.positions_in(&stored), None, "the new points have no stored place and `p` has not moved");
    sk.points[now].pos = Vec2::new(60.0, 45.0);
    let was = sk.positions_in(&stored).unwrap();
    assert_eq!(was[now.0], Vec2::new(40.0, 45.0));
    for i in sk.link_points(&link) {
        assert_eq!(was[i.0], sk.points[i].pos);
    }
}
