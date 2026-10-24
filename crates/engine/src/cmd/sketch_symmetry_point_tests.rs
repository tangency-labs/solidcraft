//! Symmetry about a point (#1075): the origin (or any sketch point) picked as the third entity,
//! or given as `point`. Each pair is joined by a construction line the point is the midpoint of.

use serde_json::{Value, json};
use solidcraft_geom::Vec2;
use solidcraft_sketch::{ConstraintKind, PointIdx, Sketch};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn sketch(s: &Session) -> Sketch {
    s.doc.sketch(s.active_sketch.unwrap()).unwrap().clone()
}

fn pt(sk: &Sketch, r: &str) -> Vec2 {
    sk.point(sk.resolve_point(r).unwrap()).unwrap()
}

fn new_sketch() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    s
}

#[test]
fn two_points_symmetric_about_the_origin() {
    for params in [json!({"a": "p", "b": "q", "line": "origin"}), json!({"a": "p", "b": "q", "point": "origin"})] {
        let mut s = new_sketch();
        run(&mut s, "sketch.point", json!({"point": [10, 5], "id": "p"}));
        run(&mut s, "sketch.point", json!({"point": [-12, -3], "id": "q"}));
        let r = run(&mut s, "sketch.constraint.symmetry", params.clone());
        let sk = sketch(&s);
        // The first point stays; the second mirrors onto it through the origin.
        assert!(pt(&sk, "p").dist(Vec2::new(10.0, 5.0)) < 1e-6, "{params}");
        assert!(pt(&sk, "q").dist(Vec2::new(-10.0, -5.0)) < 1e-6, "{params}: {:?}", pt(&sk, "q"));
        // One construction line, from p to q, with the origin at its midpoint.
        let created = r["created"]["curves"].as_array().unwrap();
        assert_eq!(created.len(), 1, "{r}");
        let l = sk.curve_index(created[0].as_str().unwrap()).unwrap();
        assert!(sk.curves[l].construction);
        assert!(sk.constraints.iter().any(|k| k.kind == ConstraintKind::Midpoint { p: PointIdx::ORIGIN, l }));
        // It holds: moving p moves q opposite.
        run(&mut s, "sketch.drag", json!({"entity": "p", "from": [10, 5], "to": [20, 8]}));
        let sk = sketch(&s);
        assert!((pt(&sk, "p") + pt(&sk, "q")).len() < 1e-6, "{:?} {:?}", pt(&sk, "p"), pt(&sk, "q"));
    }
}

#[test]
fn circles_and_lines_symmetric_about_a_point() {
    let mut s = new_sketch();
    run(&mut s, "sketch.point", json!({"point": [5, 5], "id": "c"}));
    run(&mut s, "sketch.circle.center", json!({"center": [15, 5], "radius": 2, "id": "k1"}));
    run(&mut s, "sketch.circle.center", json!({"center": [-4, 6], "radius": 3, "id": "k2"}));
    run(&mut s, "sketch.constraint.symmetry", json!({"a": "k1", "b": "k2", "point": "c"}));
    let sk = sketch(&s);
    let (c1, c2) = (sk.center(sk.curve_index("k1").unwrap()).unwrap(), sk.center(sk.curve_index("k2").unwrap()).unwrap());
    assert!(((c1 + c2) * 0.5).dist(Vec2::new(5.0, 5.0)) < 1e-6, "{c1:?} {c2:?}");
    assert!((sk.radius(sk.curve_index("k1").unwrap()).unwrap() - sk.radius(sk.curve_index("k2").unwrap()).unwrap()).abs() < 1e-6);

    // Two lines: each end pairs with the other line's end nearest its mirror image.
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[2, 1], [8, 4]], "ids": ["a"]}));
    run(&mut s, "sketch.line", json!({"points": [[-7, -5], [-1, -1]], "ids": ["b"]}));
    let r = run(&mut s, "sketch.constraint.symmetry", json!({"a": "a", "b": "b", "line": "origin"}));
    assert_eq!(r["created"]["curves"].as_array().unwrap().len(), 2, "{r}");
    let sk = sketch(&s);
    assert!((pt(&sk, "a.start") + pt(&sk, "b.end")).len() < 1e-6);
    assert!((pt(&sk, "a.end") + pt(&sk, "b.start")).len() < 1e-6);
}

#[test]
fn a_point_cannot_be_its_own_mirror() {
    let mut s = new_sketch();
    run(&mut s, "sketch.point", json!({"point": [10, 5], "id": "p"}));
    let before = s.doc.to_json();
    assert!(s.execute("sketch.constraint.symmetry", &json!({"a": "p", "b": "origin", "point": "origin"})).is_err());
    assert!(s.execute("sketch.constraint.symmetry", &json!({"a": "p", "b": "p", "line": "origin"})).is_err());
    assert!(s.execute("sketch.constraint.symmetry", &json!({"a": "p", "b": "origin", "point": "nope"})).is_err());
    assert_eq!(s.doc.to_json(), before);
}
