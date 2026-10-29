//! Sketch Dimension to the edge of a circle or arc (#4064): `sketch.dimension` with a point and a
//! circle or arc measures the distance from the point to its edge, with two circles or arcs the
//! gap between their edges (how far they overlap when they cross, the narrowest gap when one is
//! inside the other); it drives that distance, follows its parameter, and survives undo and
//! reopening.

use serde_json::{Value, json};
use solidcraft_geom::Vec2;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn sketch(s: &Session) -> solidcraft_sketch::Sketch {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().sketch.clone()
}

fn point(s: &Session, id: &str) -> Vec2 {
    let sk = sketch(s);
    sk.resolve_point(id).and_then(|p| sk.point(p)).unwrap()
}

/// A new sketch with a circle of radius `r` at `centre` (its diameter dimensioned), its centre
/// fixed when `fixed`.
fn add_circle(s: &mut Session, centre: [f64; 2], r: f64, fixed: bool) -> String {
    let id = run(s, "sketch.circle.center", json!({"center": centre, "radius": r}))["created"]["curves"][0].as_str().unwrap().to_string();
    if fixed {
        run(s, "sketch.constraint.fix", json!({"entities": [format!("{id}.center")]}));
    }
    run(s, "sketch.dimension", json!({"entities": [id], "type": "diameter", "value": 2.0 * r}));
    id
}

/// A circle of radius 5 with its centre fixed at (20, 15) (`c1`) and a point `pt` at `at`, kept
/// level with the centre.
fn circle_and_point(at: [f64; 2]) -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    add_circle(&mut s, [20.0, 15.0], 5.0, true);
    run(&mut s, "sketch.point", json!({"point": at, "id": "pt"}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"points": ["c1.center", "pt"], "mode": "horizontal"}));
    s
}

/// A circle of radius `ra` with its centre fixed at (10, 10) (`c1`) and a circle of radius `rb`
/// (`c2`) `x` to its right, kept level with it.
fn two_circles(ra: f64, x: f64, rb: f64) -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    add_circle(&mut s, [10.0, 10.0], ra, true);
    add_circle(&mut s, [10.0 + x, 10.0], rb, false);
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"points": ["c1.center", "c2.center"], "mode": "horizontal"}));
    s
}

fn near(s: &Session, id: &str, want: [f64; 2]) {
    let at = point(s, id);
    assert!(at.dist(Vec2::new(want[0], want[1])) < 1e-7, "{id} at {at:?}, want {want:?}");
}

#[test]
fn a_point_and_a_circle_dimension_to_its_edge() {
    let mut s = circle_and_point([32.0, 15.0]);
    // 12 from the centre, radius 5: 7 from the edge.
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["pt", "c1"]}));
    assert_eq!(r["type"], "LinearDimension", "{r}");
    assert_eq!(r["measured"], 7.0, "{r}");
    assert_eq!(r["expression"], "7 mm", "{r}");
    let param = r["param"].as_str().unwrap().to_string();
    // The parameter drives the gap: 3 puts the point 5 + 3 from the centre.
    run(&mut s, "parameters.change", json!({"name": param, "expression": "3 mm"}));
    near(&s, "pt", [28.0, 15.0]);
    // The circle and the point in the other order, with a value.
    let mut s = circle_and_point([32.0, 15.0]);
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["c1", "pt"], "value": 4}));
    assert_eq!(r["measured"], 4.0, "{r}");
    near(&s, "pt", [29.0, 15.0]);
}

#[test]
fn a_point_inside_the_circle_dimensions_how_far_within_the_edge_it_lies() {
    // 2 from the centre, radius 5: 3 within the edge.
    let mut s = circle_and_point([22.0, 15.0]);
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["pt", "c1"]}));
    assert_eq!(r["measured"], 3.0, "{r}");
    let param = r["param"].as_str().unwrap().to_string();
    run(&mut s, "parameters.change", json!({"name": param, "expression": "1"}));
    // 1 within the edge is 4 from the centre: the point stays inside.
    near(&s, "pt", [24.0, 15.0]);
}

#[test]
fn the_circles_own_centre_is_refused() {
    let mut s = circle_and_point([32.0, 15.0]);
    for entities in [["c1.center", "c1"], ["c1", "c1.center"]] {
        let err = s.execute("sketch.dimension", &json!({"entities": entities})).unwrap_err().to_string();
        assert!(err.contains("own centre"), "{err}");
    }
}

#[test]
fn two_circles_dimension_the_gap_between_their_edges() {
    // Centres 20 apart, radii 5 and 3: a gap of 12.
    let mut s = two_circles(5.0, 20.0, 3.0);
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["c1", "c2"]}));
    assert_eq!(r["type"], "LinearDimension", "{r}");
    assert_eq!(r["measured"], 12.0, "{r}");
    let param = r["param"].as_str().unwrap().to_string();
    run(&mut s, "parameters.change", json!({"name": param, "expression": "4 mm"}));
    near(&s, "c2.center", [22.0, 10.0]);
}

#[test]
fn crossing_and_nested_circles_dimension_their_overlap_and_their_narrowest_gap() {
    // Centres 6 apart, radii 5 and 3: they overlap by 2; overlapping by 1 puts them 7 apart.
    let mut s = two_circles(5.0, 6.0, 3.0);
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["c2", "c1"]}));
    assert_eq!(r["measured"], 2.0, "{r}");
    run(&mut s, "parameters.change", json!({"name": r["param"].as_str().unwrap(), "expression": "1"}));
    near(&s, "c2.center", [17.0, 10.0]);
    // A radius 3 circle 4 off the centre of a radius 10 one: 3 at the narrowest; 1 there puts
    // it 6 off.
    let mut s = two_circles(10.0, 4.0, 3.0);
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["c1", "c2"]}));
    assert_eq!(r["measured"], 3.0, "{r}");
    run(&mut s, "parameters.change", json!({"name": r["param"].as_str().unwrap(), "expression": "1"}));
    near(&s, "c2.center", [16.0, 10.0]);
}

/// #6764: enlarging the bore beyond the old outside radius must enlarge the outside too,
/// keeping the selected annulus through recompute, undo/redo and reopening.
#[test]
fn a_tube_keeps_its_bore_when_its_inner_diameter_changes() {
    fn check(s: &mut Session, inner: f64) {
        {
            let state = s.model.state();
            let ss = state.sketch(1).unwrap();
            let radius = |id| ss.sketch.radius(ss.sketch.curve_index(id).unwrap()).unwrap();
            assert!((radius("outside") - inner - 10.0).abs() < 1e-6);
            assert!((radius("bore") - inner).abs() < 1e-6);
            let annulus = ss.profiles.iter().find(|p| p.outer_curves == ["outside"]).unwrap();
            assert_eq!(annulus.hole_curves, [vec!["bore"]]);
        }
        let area = std::f64::consts::PI * ((inner + 10.0).powi(2) - inner.powi(2));
        let m = run(s, "inspect.measure", json!({}));
        assert_eq!(m["body_count"], 1, "{m}");
        let volume = m["total"]["volume_mm3"].as_f64().unwrap();
        assert!((volume / (100.0 * area) - 1.0).abs() < 0.001, "{m}");
        let body = &s.model.state().bodies[0].body;
        assert!(!body.contains(solidcraft_geom::Vec3::new(0.0, 0.0, 50.0)), "the bore stays empty");
        assert!(body.contains(solidcraft_geom::Vec3::new(inner + 5.0, 0.0, 50.0)), "the wall stays solid");
    }
    for dimension_first in [false, true] {
        let mut s = Session::default();
        run(&mut s, "sketch.create", json!({"plane": "XY"}));
        run(&mut s, "sketch.circle.center", json!({"center": "origin", "radius": 20, "id": "outside"}));
        run(&mut s, "sketch.circle.center", json!({"center": "origin", "radius": 10, "id": "bore"}));
        let diameter = json!({"entities": ["bore"], "type": "diameter", "value": 20});
        let wall = json!({"entities": ["bore", "outside"], "value": 10});
        let params = if dimension_first { [diameter, wall] } else { [wall, diameter] };
        let dimensions = params.map(|p| run(&mut s, "sketch.dimension", p));
        let name = dimensions[usize::from(!dimension_first)]["param"].as_str().unwrap();
        run(&mut s, "sketch.finish", json!({}));
        run(&mut s, "solid.extrude", json!({"distance": 100, "profiles": [{"curves": ["outside"]}]}));
        check(&mut s, 10.0);
        run(&mut s, "parameters.change", json!({"name": name, "expression": "50 mm"}));
        check(&mut s, 25.0);
        run(&mut s, "edit.undo", json!({}));
        check(&mut s, 10.0);
        run(&mut s, "edit.redo", json!({}));
        check(&mut s, 25.0);
        let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap());
        check(&mut reopened, 25.0);
        run(&mut reopened, "parameters.change", json!({"name": name, "expression": "6 mm"}));
        check(&mut reopened, 3.0);
    }
}

#[test]
fn arcs_dimension_to_their_circles() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.arc.center_point", json!({"center": [20, 20], "start": [26, 20], "end": [14, 20]}));
    run(&mut s, "sketch.circle.center", json!({"center": [40, 20], "radius": 4}));
    run(&mut s, "sketch.point", json!({"point": [20, 0], "id": "pt"}));
    // Centres 20 apart, radii 6 and 4.
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["a1", "c1"]}));
    assert_eq!(r["measured"], 10.0, "{r}");
    // 20 below the arc's centre, radius 6.
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["pt", "a1"]}));
    assert_eq!(r["measured"], 14.0, "{r}");
}

#[test]
fn the_dimensions_undo_and_reopen() {
    let mut s = two_circles(5.0, 20.0, 3.0);
    run(&mut s, "sketch.point", json!({"point": [10, 30], "id": "pt"}));
    run(&mut s, "sketch.constraint.horizontal_vertical", json!({"points": ["c1.center", "pt"], "mode": "vertical"}));
    let before = sketch(&s).constraints.len();
    run(&mut s, "sketch.dimension", json!({"entities": ["c1", "c2"], "value": 6}));
    run(&mut s, "sketch.dimension", json!({"entities": ["pt", "c1"], "value": 9}));
    let saved = s.doc.to_json();
    assert!(saved.contains("\"point_circle_distance\"") && saved.contains("\"circle_circle_distance\""), "not saved");
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&saved).unwrap());
    run(&mut reopened, "sketch.edit", json!({"sketch": "Sketch1"}));
    near(&reopened, "c2.center", [24.0, 10.0]);
    near(&reopened, "pt", [10.0, 24.0]);
    let kinds = |s: &Session| sketch(s).constraints.iter().map(|c| c.kind.clone()).collect::<Vec<_>>();
    assert_eq!(kinds(&reopened), kinds(&s));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(sketch(&s).constraints.len(), before);
}

#[test]
fn a_gap_the_sketch_already_has_is_refused_or_driven() {
    // Tangent circles already have a gap of zero.
    let mut s = two_circles(5.0, 8.0, 3.0);
    run(&mut s, "sketch.constraint.tangent", json!({"a": "c1", "b": "c2"}));
    let err = s.execute("sketch.dimension", &json!({"entities": ["c1", "c2"]})).unwrap_err().to_string();
    assert!(err.contains("already determined"), "{err}");
    let r = run(&mut s, "sketch.dimension", json!({"entities": ["c1", "c2"], "if_redundant": "driven"}));
    assert_eq!(r["driven"], true, "{r}");
    assert!(r["measured"].as_f64().unwrap().abs() < 1e-7, "{r}");
}

#[test]
fn the_same_circle_twice_and_an_ellipse_are_refused() {
    let mut s = circle_and_point([32.0, 15.0]);
    let err = s.execute("sketch.dimension", &json!({"entities": ["c1", "c1"]})).unwrap_err().to_string();
    assert!(err.contains("two different"), "{err}");
    run(&mut s, "sketch.ellipse", json!({"center": [60, 20], "major": [70, 20], "minor_radius": 4}));
    let err = s.execute("sketch.dimension", &json!({"entities": ["pt", "e1"]})).unwrap_err().to_string();
    assert!(err.contains("cannot dimension that combination"), "{err}");
}
