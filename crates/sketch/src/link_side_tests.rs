//! A dimension from a link keeps its side when the link moves: the solve after the links are
//! refreshed reads sides and turns where the points were stored ([`Sketch::positions_in`]),
//! not beside a line that has already moved past them.

use super::*;
use crate::{Analysis, ConstraintKind, flips_from_stored, solve, solve_after_edit, solve_after_link_move};

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
    sk.add_constraint(ConstraintKind::DistanceX { p: 0, q: p, value: 40.0 }, None).unwrap();
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
            sk.add_constraint(ConstraintKind::DistanceX { p: 0, q: p, value: 40.0 }, None).unwrap();
            sk.add_constraint(ConstraintKind::DistanceY { p: q, q: p, value: 5.0 }, None).unwrap();
        } else {
            sk.add_constraint(ConstraintKind::DistanceY { p: 0, q: p, value: 40.0 }, None).unwrap();
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
    assert_eq!(was[now], Vec2::new(40.0, 45.0));
    for i in sk.link_points(&link) {
        assert_eq!(was[i], sk.points[i].pos);
    }
}

/// A 40 mm horizontal line tangent to a linked circle (r 3) above it, its left end 20 mm left
/// of the centre; the stored sketch with the circle where it was (`y`).
fn tangent_on_circle(y: f64) -> (Sketch, String, usize) {
    let circle = |y: f64| vec![LinkGeom::Circle(Vec2::new(0.0, y), 3.0)];
    let mut sk = Sketch::new();
    let link = sk.add_link(LinkKind::Project, LinkSource::SketchCurve { sketch: 1, curve: "bend".into() }, &circle(y)).unwrap();
    let c = sk.link_curves(&link)[0];
    let centre = sk.link_points(&link)[0];
    let l = sk.add_line(Vec2::new(-20.0, y + 3.0), Vec2::new(20.0, y + 3.0), None, None, None).unwrap();
    let (a, _) = sk.curves[l].kind.ends().unwrap();
    sk.add_constraint(ConstraintKind::Horizontal { l }, None).unwrap();
    sk.add_constraint(ConstraintKind::Tangent { a: l, b: c }, None).unwrap();
    sk.add_constraint(ConstraintKind::DistanceX { p: a, q: centre, value: 20.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::Length { l, value: 40.0 }, None).unwrap();
    assert!(solve(&mut sk).ok());
    (sk, link, l)
}

#[test]
fn a_line_tangent_to_a_linked_circle_keeps_its_side_when_the_circle_moves_far() {
    let (stored, link, l) = tangent_on_circle(0.0);
    let height = |sk: &Sketch| match sk.curves[l].kind {
        CurveKind::Line { a, .. } => sk.points[a].pos.y,
        _ => f64::NAN,
    };
    assert!((height(&stored) - 3.0).abs() < 1e-9);
    let circle = |y: f64| vec![LinkGeom::Circle(Vec2::new(0.0, y), 3.0)];
    // Moved 100 mm up, the circle passes the line. Its other tangent (y 97, a diameter below)
    // solves every constraint too: only the side it left tells it from the right one.
    let mut mirrored = stored.clone();
    mirrored.update_link(&link, &circle(100.0)).unwrap();
    for p in &mut mirrored.points {
        if p.link.is_none() && p.id != "origin" {
            p.pos.y += 94.0;
        }
    }
    assert!((height(&mirrored) - 97.0).abs() < 1e-9);
    assert!(solve(&mut mirrored).ok() && (height(&mirrored) - 97.0).abs() < 1e-9);
    assert!(!flips_from_stored(&mirrored, &stored).is_empty());
    // Walked there, the line stays above the circle, in either direction and far.
    for y in [100.0, -100.0, 2000.0] {
        let mut sk = stored.clone();
        sk.update_link(&link, &circle(y)).unwrap();
        let report = solve_after_link_move(&mut sk, &stored, &stored.driving_values(), Analysis::Full);
        assert!(report.is_some_and(|r| r.keeps_orientation()), "{y}");
        assert!((height(&sk) - (y + 3.0)).abs() < 1e-9 && flips_from_stored(&sk, &stored).is_empty(), "{y}: {}", height(&sk));
    }
    // Nothing moved: nothing to walk.
    let mut sk = stored.clone();
    assert!(solve_after_link_move(&mut sk, &stored, &stored.driving_values(), Analysis::Full).is_none());
}

/// A point held on a linked rail 3 mm from another point on it (a distance between the two):
/// the same distance back past the other point solves every constraint too, and only the side
/// it was drawn on tells them apart. The other point is held on the rail itself, or lies on it
/// as the mirror image of a point on it across a line square to it. The mirror image counts as
/// a flip, and the rail walked far (steeper, higher, the other way up) keeps the point on its side.
#[test]
fn a_point_a_distance_along_a_linked_line_keeps_its_side() {
    let slant = |y0: f64, y1: f64| vec![LinkGeom::Line(Vec2::new(0.0, y0), Vec2::new(80.0, y1))];
    for mirrored_on in [false, true] {
        let mut sk = Sketch::new();
        let link = sk.add_link(LinkKind::Project, source(), &slant(10.0, 50.0)).unwrap();
        let rail = sk.link_curves(&link)[0];
        let a = sk.add_point(Vec2::new(20.0, 20.0), None).unwrap();
        let h = sk.add_point(Vec2::new(23.0, 21.5), None).unwrap();
        if mirrored_on {
            // `a` mirrors `b` (on the rail, x 0..) across a line square to the rail at x 10.
            let b = sk.add_point(Vec2::new(0.5, 10.0), None).unwrap();
            let mh = sk.add_line(Vec2::new(10.0, 15.0), Vec2::new(5.5, 24.0), None, None, None).unwrap();
            let (m, _) = sk.curves[mh].kind.ends().unwrap();
            sk.add_constraint(ConstraintKind::PointOnCurve { p: b, c: rail }, None).unwrap();
            sk.add_constraint(ConstraintKind::PointOnCurve { p: m, c: rail }, None).unwrap();
            sk.add_constraint(ConstraintKind::Perpendicular { a: mh, b: rail }, None).unwrap();
            sk.add_constraint(ConstraintKind::Length { l: mh, value: 10.0 }, None).unwrap();
            sk.add_constraint(ConstraintKind::DistanceX { p: 0, q: m, value: 10.0 }, None).unwrap();
            sk.add_constraint(ConstraintKind::DistanceX { p: 0, q: b, value: 0.5 }, None).unwrap();
            sk.add_constraint(ConstraintKind::Symmetric { p: b, q: a, l: mh }, None).unwrap();
        } else {
            sk.add_constraint(ConstraintKind::PointOnCurve { p: a, c: rail }, None).unwrap();
            sk.add_constraint(ConstraintKind::DistanceX { p: 0, q: a, value: 20.0 }, None).unwrap();
        }
        sk.add_constraint(ConstraintKind::PointOnCurve { p: h, c: rail }, None).unwrap();
        let span = sk.add_constraint(ConstraintKind::Distance { p: a, q: h, value: 3.0 }, None).unwrap();
        assert!(solve(&mut sk).ok(), "{mirrored_on}");
        let ahead = |sk: &Sketch| match sk.curves[rail].kind {
            CurveKind::Line { a: s, b: e } => (sk.points[e].pos - sk.points[s].pos).dot(sk.points[h].pos - sk.points[a].pos),
            _ => f64::NAN,
        };
        assert!(ahead(&sk) > 0.0 && (sk.points[h].pos.dist(sk.points[a].pos) - 3.0).abs() < 1e-9, "{mirrored_on}");
        let stored = sk.clone();
        // The same 3 mm back past `a`: solved as it is, and told apart only by the side.
        let mut back = stored.clone();
        back.points[h].pos = back.points[a].pos * 2.0 - back.points[h].pos;
        assert!(solve(&mut back).ok() && ahead(&back) < 0.0, "{mirrored_on}");
        assert!(flips_from_stored(&back, &stored).contains(&span), "{mirrored_on}: {:?}", flips_from_stored(&back, &stored));
        // Walked there, `h` stays ahead of `a` along the rail.
        for (y0, y1) in [(300.0, 260.0), (-40.0, 400.0), (2000.0, 1990.0)] {
            let mut sk = stored.clone();
            sk.update_link(&link, &slant(y0, y1)).unwrap();
            let report = solve_after_link_move(&mut sk, &stored, &stored.driving_values(), Analysis::Full);
            assert!(report.is_some_and(|r| r.keeps_orientation()), "{mirrored_on} {y0} {y1}");
            assert!(ahead(&sk) > 0.0 && flips_from_stored(&sk, &stored).is_empty(), "{mirrored_on} {y0} {y1}: {}", ahead(&sk));
        }
    }
}
