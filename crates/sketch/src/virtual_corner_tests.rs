#![allow(clippy::unwrap_used, clippy::panic)]

use crate::{Constraint, ConstraintKind, CurveKind, Sketch};
use solidcraft_geom::Vec2;

fn point(sk: &mut Sketch, x: f64, y: f64) -> usize {
    sk.add_point(Vec2::new(x, y), None).unwrap()
}

fn on(sk: &mut Sketch, p: usize, c: usize) {
    sk.add_constraint(ConstraintKind::PointOnCurve { p, c }, None).unwrap();
}

#[test]
fn virtual_corner_extensions_keep_constraint_order_and_distinct_curves() {
    let mut sk = Sketch::new();
    let p = point(&mut sk, 10., 10.);
    let horizontal = sk.add_line(Vec2::new(0., 10.), Vec2::new(8., 10.), None, None, None).unwrap();
    let vertical = sk.add_line(Vec2::new(10., 0.), Vec2::new(10., 7.), None, None, None).unwrap();
    on(&mut sk, p, vertical);
    on(&mut sk, p, vertical);
    assert!(sk.virtual_corners().is_empty(), "one curve is not a sharp corner even with duplicate constraints");
    on(&mut sk, p, horizontal);
    assert_eq!(sk.virtual_corners(), vec![(p, vec![(Vec2::new(10., 10.), Vec2::new(10., 7.)), (Vec2::new(10., 10.), Vec2::new(8., 10.))])]);
    let copy = serde_json::from_str::<Sketch>(&serde_json::to_string(&sk).unwrap()).unwrap();
    assert_eq!(copy.virtual_corners(), sk.virtual_corners());
}

#[test]
fn virtual_corner_inside_lines_stays_visible_without_extensions_and_origin_is_excluded() {
    let mut sk = Sketch::new();
    let p = point(&mut sk, 1., 1.);
    let a = sk.add_line(Vec2::new(0., 1.), Vec2::new(2., 1.), None, None, None).unwrap();
    let b = sk.add_line(Vec2::new(1., 0.), Vec2::new(1., 2.), None, None, None).unwrap();
    for c in [a, b] {
        on(&mut sk, p, c);
        on(&mut sk, 0, c);
    }
    assert_eq!(sk.virtual_corners(), vec![(p, vec![])]);
}

#[test]
fn curve_centres_and_spline_interior_points_are_used_and_invalid_incidence_is_safe() {
    let mut sk = Sketch::new();
    let centre = point(&mut sk, 10., 10.);
    let interior = point(&mut sk, 20., 20.);
    let a = sk.add_line(Vec2::new(0., 10.), Vec2::new(8., 10.), None, None, None).unwrap();
    let b = sk.add_line(Vec2::new(10., 0.), Vec2::new(10., 8.), None, None, None).unwrap();
    sk.add_arc(Vec2::new(10., 10.), Vec2::new(11., 10.), Vec2::new(10., 11.), [Some(centre), None, None], None).unwrap();
    let first = point(&mut sk, 19., 19.);
    let last = point(&mut sk, 21., 21.);
    let mut spline = sk.curves[a].clone();
    spline.id = "interior_spline".into();
    spline.kind = CurveKind::Spline { pts: vec![first, interior, last], control: false, degree: 3, periodic: false, handles: Vec::new() };
    sk.curves.push(spline);
    for p in [centre, interior] {
        for c in [a, b] {
            on(&mut sk, p, c);
        }
    }
    for (p, c) in [(usize::MAX, a), (centre, usize::MAX)] {
        sk.constraints.push(Constraint {
            id: format!("damaged{p}{c}"),
            kind: ConstraintKind::PointOnCurve { p, c },
            param: None,
            driven: false,
            text: None,
        });
    }
    assert!(sk.virtual_corners().is_empty());
}

#[test]
fn many_virtual_corners_are_indexed_in_point_order() {
    let mut sk = Sketch::new();
    let mut expected = Vec::new();
    for i in 0..3500 {
        let x = i as f64 * 3. + 1.;
        let p = point(&mut sk, x, 1.);
        let a = sk.add_line(Vec2::new(x - 1., 1.), Vec2::new(x - 0.5, 1.), None, None, None).unwrap();
        let b = sk.add_line(Vec2::new(x, 0.), Vec2::new(x, 0.5), None, None, None).unwrap();
        on(&mut sk, p, a);
        on(&mut sk, p, b);
        expected.push((p, vec![(Vec2::new(x, 1.), Vec2::new(x - 0.5, 1.)), (Vec2::new(x, 1.), Vec2::new(x, 0.5))]));
    }
    assert_eq!(sk.virtual_corners(), expected);
}

#[test]
fn oversized_reference_input_is_refused_without_scanning_or_mutating_it() {
    let mut sk = Sketch::new();
    let a = sk.add_line(Vec2::new(0., 1.), Vec2::new(2., 1.), None, None, None).unwrap();
    let before = sk.points.clone();
    sk.curves[a].kind = CurveKind::Spline { pts: vec![usize::MAX; 1_000_001], control: false, degree: 3, periodic: false, handles: Vec::new() };
    assert!(sk.virtual_corners().is_empty());
    assert_eq!(sk.points, before);
    assert_eq!(sk.curves[a].kind.point_ids().len(), 1_000_001);
}
