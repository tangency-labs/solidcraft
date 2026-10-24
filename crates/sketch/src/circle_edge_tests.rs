//! Distances to the edge of a circle or arc from a point, and between the edges of two circles or
//! arcs (#4064): solved numerically and by construction, measured, drawn between the edge points
//! that face each other, saved, and refused on other kinds of curve.

use solidcraft_geom::Vec2;

use crate::tests::v;
use crate::*;

/// A circle of radius `r` at `centre`, its radius set by a dimension; its centre fixed when
/// `fixed`. The circle and its centre point.
fn circle(sk: &mut Sketch, centre: Vec2, r: f64, fixed: bool) -> (usize, usize) {
    let c = sk.add_circle(centre, r, None, None).unwrap();
    let CurveKind::Circle { c: ci, .. } = sk.curves[c].kind else { panic!("not a circle") };
    sk.add_constraint(ConstraintKind::Radius { c, value: r }, None).unwrap();
    if fixed {
        sk.add_constraint(ConstraintKind::Fix { p: ci }, None).unwrap();
    }
    (c, ci)
}

/// A fixed circle of radius 5 at (20, 15) and a point at `at`, kept level with the centre (so
/// a distance from the centre places it). The circle, its centre and the point.
fn circle_and_point(at: Vec2) -> (Sketch, usize, usize, usize) {
    let mut sk = Sketch::new();
    let (c, ci) = circle(&mut sk, v(20.0, 15.0), 5.0, true);
    let p = sk.add_point(at, None).unwrap();
    sk.add_constraint(ConstraintKind::HorizontalPoints { p: ci, q: p }, None).unwrap();
    (sk, c, ci, p)
}

/// A fixed circle of radius `ra` at the origin and a free circle of radius `rb` at (`x`, 0),
/// level with it. The sketch, both circles and the free circle's centre.
fn two_circles(ra: f64, x: f64, rb: f64) -> (Sketch, usize, usize, usize) {
    let mut sk = Sketch::new();
    let (a, ca) = circle(&mut sk, v(0.0, 0.0), ra, true);
    let (b, cb) = circle(&mut sk, v(x, 0.0), rb, false);
    sk.add_constraint(ConstraintKind::HorizontalPoints { p: ca, q: cb }, None).unwrap();
    (sk, a, b, cb)
}

fn measured(sk: &Sketch, k: usize) -> f64 {
    solver::measure_dimension(sk, &sk.constraints[k].kind.clone()).unwrap()
}

#[test]
fn a_point_to_circle_distance_places_the_point_its_gap_from_the_edge() {
    let (mut sk, c, _, p) = circle_and_point(v(32.0, 15.0));
    let k = sk.constraints.len();
    sk.add_constraint(ConstraintKind::PointCircleDistance { p, c, value: 7.0, inside: false }, None).unwrap();
    // As drawn the point is 12 from the centre, 12 − 5 = 7 from the edge.
    assert!((measured(&sk, k) - 7.0).abs() < 1e-9);
    assert!(solve(&mut sk).ok());
    // A gap of 3 puts the point 5 + 3 from the centre, on the side it was drawn on.
    sk.constraints[k].kind.set_value(3.0);
    let rep = solve(&mut sk);
    assert!(rep.ok() && rep.dof == 0, "{rep:?}");
    assert!(sk.points[p].pos.dist(v(28.0, 15.0)) < 1e-7, "{:?}", sk.points[p].pos);
    assert!((measured(&sk, k) - 3.0).abs() < 1e-7);
    // Zero puts it on the circle.
    sk.constraints[k].kind.set_value(0.0);
    assert!(solve(&mut sk).ok());
    assert!(sk.points[p].pos.dist(v(25.0, 15.0)) < 1e-7, "{:?}", sk.points[p].pos);
}

#[test]
fn a_point_inside_the_circle_measures_how_far_within_the_edge_it_lies() {
    // 2 from the centre, radius 5: 3 within the edge.
    let (mut sk, c, _, p) = circle_and_point(v(22.0, 15.0));
    let k = sk.constraints.len();
    sk.add_constraint(ConstraintKind::PointCircleDistance { p, c, value: 3.0, inside: true }, None).unwrap();
    assert!((measured(&sk, k) - 3.0).abs() < 1e-9);
    assert!(solve(&mut sk).ok());
    // 1 within the edge is 4 from the centre: the point stays inside.
    sk.constraints[k].kind.set_value(1.0);
    let rep = solve(&mut sk);
    assert!(rep.ok(), "{rep:?}");
    assert!(sk.points[p].pos.dist(v(24.0, 15.0)) < 1e-7, "{:?}", sk.points[p].pos);
}

#[test]
fn two_circles_apart_are_placed_their_gap_from_each_other() {
    // Centres 20 apart, radii 5 and 3: a gap of 12.
    let (mut sk, a, b, cb) = two_circles(5.0, 20.0, 3.0);
    let k = sk.constraints.len();
    sk.add_constraint(ConstraintKind::CircleCircleDistance { a, b, value: 12.0, circles: TwoCircles::Apart }, None).unwrap();
    assert!((measured(&sk, k) - 12.0).abs() < 1e-9);
    assert!(solve(&mut sk).ok());
    sk.constraints[k].kind.set_value(4.0);
    let rep = solve(&mut sk);
    assert!(rep.ok() && rep.dof == 0, "{rep:?}");
    assert!(sk.points[cb].pos.dist(v(12.0, 0.0)) < 1e-7, "{:?}", sk.points[cb].pos);
    // Tangent: a gap of zero.
    sk.constraints[k].kind.set_value(0.0);
    assert!(solve(&mut sk).ok());
    assert!(sk.points[cb].pos.dist(v(8.0, 0.0)) < 1e-7, "{:?}", sk.points[cb].pos);
}

#[test]
fn crossing_circles_measure_how_far_they_overlap() {
    // Centres 6 apart, radii 5 and 3: they overlap by 5 + 3 − 6 = 2.
    let (mut sk, a, b, cb) = two_circles(5.0, 6.0, 3.0);
    let k = sk.constraints.len();
    sk.add_constraint(ConstraintKind::CircleCircleDistance { a, b, value: 2.0, circles: TwoCircles::Crossing }, None).unwrap();
    assert!((measured(&sk, k) - 2.0).abs() < 1e-9);
    sk.constraints[k].kind.set_value(1.0);
    let rep = solve(&mut sk);
    assert!(rep.ok(), "{rep:?}");
    assert!(sk.points[cb].pos.dist(v(7.0, 0.0)) < 1e-7, "{:?}", sk.points[cb].pos);
}

#[test]
fn nested_circles_measure_the_narrowest_gap_between_them() {
    // A radius 3 circle 4 off the centre of a radius 10 one: the wall is 10 − 3 − 4 = 3 at its
    // narrowest, whichever circle is named first.
    for swap in [false, true] {
        let (mut sk, a, b, cb) = two_circles(10.0, 4.0, 3.0);
        let (a, b) = if swap { (b, a) } else { (a, b) };
        let k = sk.constraints.len();
        sk.add_constraint(ConstraintKind::CircleCircleDistance { a, b, value: 3.0, circles: TwoCircles::Nested }, None).unwrap();
        assert!((measured(&sk, k) - 3.0).abs() < 1e-9);
        sk.constraints[k].kind.set_value(1.0);
        let rep = solve(&mut sk);
        assert!(rep.ok(), "{rep:?}");
        assert!(sk.points[cb].pos.dist(v(6.0, 0.0)) < 1e-7, "{swap}: {:?}", sk.points[cb].pos);
    }
}

#[test]
fn how_two_circles_lie_and_their_gap() {
    use TwoCircles::*;
    for (d, ra, rb, lie, gap) in [
        (20.0, 5.0, 3.0, Apart, 12.0),
        (8.0, 5.0, 3.0, Apart, 0.0),
        (6.0, 5.0, 3.0, Crossing, 2.0),
        (4.0, 10.0, 3.0, Nested, 3.0),
        (0.0, 3.0, 10.0, Nested, 7.0),
    ] {
        assert_eq!(TwoCircles::of(d, ra, rb), lie, "{d} {ra} {rb}");
        assert!((lie.gap(d, ra, rb) - gap).abs() < 1e-12, "{d} {ra} {rb}");
        assert!((lie.centres_apart(gap, ra, rb) - d).abs() < 1e-12, "{d} {ra} {rb}");
    }
}

#[test]
fn arcs_are_measured_to_their_circles() {
    let mut sk = Sketch::new();
    let p = sk.add_point(v(20.0, 0.0), None).unwrap();
    // Centre (20, 20), radius 6.
    let arc = sk.add_arc(v(20.0, 20.0), v(26.0, 20.0), v(14.0, 20.0), [None; 3], None).unwrap();
    let c = sk.add_circle(v(40.0, 20.0), 4.0, None, None).unwrap();
    let k = sk.constraints.len();
    sk.add_constraint(ConstraintKind::PointCircleDistance { p, c: arc, value: 14.0, inside: false }, None).unwrap();
    sk.add_constraint(ConstraintKind::CircleCircleDistance { a: arc, b: c, value: 10.0, circles: TwoCircles::Apart }, None).unwrap();
    assert!((measured(&sk, k) - 14.0).abs() < 1e-9);
    // Centres 20 apart, radii 6 and 4.
    assert!((measured(&sk, k + 1) - 10.0).abs() < 1e-9);
}

#[test]
fn the_dimensions_need_circles_or_arcs() {
    let mut sk = Sketch::new();
    let l = sk.add_line(v(0.0, 0.0), v(40.0, 0.0), None, None, None).unwrap();
    let c = sk.add_circle(v(20.0, 20.0), 5.0, None, None).unwrap();
    let (ec, em) = (sk.add_point(v(60.0, 20.0), None).unwrap(), sk.add_point(v(70.0, 20.0), None).unwrap());
    let e = sk.add_curve(CurveKind::Ellipse { c: ec, m: em, r: 4.0 }, None).unwrap();
    let p = sk.add_point(v(0.0, 30.0), None).unwrap();
    for c in [l, e] {
        assert!(sk.add_constraint(ConstraintKind::PointCircleDistance { p, c, value: 1.0, inside: false }, None).is_err(), "{c}");
    }
    for (a, b) in [(c, c), (c, l), (l, c), (c, e)] {
        let k = ConstraintKind::CircleCircleDistance { a, b, value: 1.0, circles: TwoCircles::Apart };
        assert!(sk.add_constraint(k, None).is_err(), "{a} {b}");
    }
    for value in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(sk.add_constraint(ConstraintKind::PointCircleDistance { p, c, value, inside: false }, None).is_err(), "{value}");
    }
}

#[test]
fn the_dimensions_survive_a_round_trip_and_a_renumbering() {
    let (mut sk, a, b, _) = two_circles(10.0, 4.0, 3.0);
    let p = sk.add_point(v(30.0, 0.0), None).unwrap();
    sk.add_constraint(ConstraintKind::PointCircleDistance { p, c: a, value: 1.0, inside: true }, None).unwrap();
    sk.add_constraint(ConstraintKind::CircleCircleDistance { a, b, value: 3.0, circles: TwoCircles::Nested }, None).unwrap();
    let json = serde_json::to_value(&sk).unwrap();
    let saved = json["constraints"].as_array().unwrap();
    let (point, circles) = (&saved[saved.len() - 2], &saved[saved.len() - 1]);
    assert_eq!(point["type"], "point_circle_distance", "{point}");
    assert_eq!(point["inside"], true, "{point}");
    assert_eq!(circles["type"], "circle_circle_distance", "{circles}");
    assert_eq!(circles["circles"], "nested", "{circles}");
    let back: Sketch = serde_json::from_value(json).unwrap();
    assert_eq!(back.constraints, sk.constraints);
    // The defaults (a point outside, circles apart) are left out.
    let outside = ConstraintKind::PointCircleDistance { p, c: a, value: 1.0, inside: false };
    assert!(serde_json::to_value(&outside).unwrap().get("inside").is_none());
    let apart = ConstraintKind::CircleCircleDistance { a, b, value: 1.0, circles: TwoCircles::Apart };
    assert!(serde_json::to_value(&apart).unwrap().get("circles").is_none());
    let mut k = outside;
    assert!(k.remap(&|q| Some(q + 1), &|i| Some(i + 10)));
    assert_eq!(k.indices(), (vec![p + 1], vec![a + 10]));
    assert!(!k.remap(&|_| None, &|i| Some(i)));
    let mut k = apart;
    assert!(k.remap(&|q| Some(q), &|i| Some(i + 10)));
    assert_eq!(k.indices(), (vec![], vec![a + 10, b + 10]));
    assert!(!k.remap(&|q| Some(q), &|_| None));
}

/// With a radius known and the other coordinate set, the seed places the point or the centre
/// exactly, before the numerical solver runs.
#[test]
fn the_seed_places_them_by_construction() {
    // The point 5 right of the centre and 8 from the edge: 13 from the centre, so 12 above it.
    let (mut sk, c, ci, p) = circle_and_point(v(24.0, 15.0));
    sk.constraints.retain(|k| !matches!(k.kind, ConstraintKind::HorizontalPoints { .. }));
    sk.points[p].pos = v(24.0, 26.0);
    sk.add_constraint(ConstraintKind::DistanceX { p: ci, q: p, value: 5.0 }, None).unwrap();
    sk.add_constraint(ConstraintKind::PointCircleDistance { p, c, value: 8.0, inside: false }, None).unwrap();
    let out = crate::tests::seed_of(&sk);
    assert!(out.placed_by[p].is_some_and(crate::construct::Placement::anchors), "{out:?}");
    assert!(out.pos[p].unwrap().dist(v(25.0, 27.0)) < 1e-9, "{out:?}");
    let rep = solve(&mut sk);
    assert!(rep.ok() && rep.dof == 0, "{rep:?}");
    assert!(sk.points[p].pos.dist(v(25.0, 27.0)) < 1e-9, "{:?}", sk.points[p].pos);
    // Circles apart by 4, radii 5 and 3: the free centre 12 along the level line.
    let (mut sk, a, b, cb) = two_circles(5.0, 20.0, 3.0);
    sk.add_constraint(ConstraintKind::CircleCircleDistance { a, b, value: 4.0, circles: TwoCircles::Apart }, None).unwrap();
    let out = crate::tests::seed_of(&sk);
    assert!(out.placed_by[cb].is_some_and(crate::construct::Placement::anchors), "{out:?}");
    assert!(out.pos[cb].unwrap().dist(v(12.0, 0.0)) < 1e-9, "{out:?}");
}

/// The dimension line runs between the edge points that face each other: from the edge to the
/// point along the radius, and along the line through two centres.
#[test]
fn the_dimensions_are_drawn_between_the_facing_edge_points() {
    let linear = |sk: &Sketch, k: &ConstraintKind| match dim_frame(sk, k) {
        Some(DimFrame::Linear { p0, p1, dir }) => (p0, p1, dir),
        f => panic!("{f:?}"),
    };
    let (sk, c, _, p) = circle_and_point(v(32.0, 15.0));
    let (p0, p1, dir) = linear(&sk, &ConstraintKind::PointCircleDistance { p, c, value: 7.0, inside: false });
    assert!(p0.dist(v(25.0, 15.0)) < 1e-12 && p1.dist(v(32.0, 15.0)) < 1e-12 && dir.dist(Vec2::X) < 1e-12, "{p0:?} {p1:?} {dir:?}");
    // Apart: from a's edge towards b to b's edge towards a.
    let (sk, a, b, _) = two_circles(5.0, 20.0, 3.0);
    let (p0, p1, _) = linear(&sk, &ConstraintKind::CircleCircleDistance { a, b, value: 12.0, circles: TwoCircles::Apart });
    assert!(p0.dist(v(5.0, 0.0)) < 1e-12 && p1.dist(v(17.0, 0.0)) < 1e-12, "{p0:?} {p1:?}");
    // Nested, either way round: across the narrowest part of the wall, from 7 to 10.
    let (sk, a, b, _) = two_circles(10.0, 4.0, 3.0);
    for (a, b) in [(a, b), (b, a)] {
        let (p0, p1, _) = linear(&sk, &ConstraintKind::CircleCircleDistance { a, b, value: 3.0, circles: TwoCircles::Nested });
        let (lo, hi) = if p0.x < p1.x { (p0, p1) } else { (p1, p0) };
        assert!(lo.dist(v(7.0, 0.0)) < 1e-12 && hi.dist(v(10.0, 0.0)) < 1e-12, "{p0:?} {p1:?}");
    }
}
