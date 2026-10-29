//! Infinite lines (#1359): a line type through two points, construction geometry that reaches
//! across the whole view, a boundary along its whole line, never trimmed, filleted or in a
//! profile.

use serde_json::{Value, json};
use solidcraft_geom::Vec2;
use solidcraft_sketch::{CurveKind, Sketch};

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

fn curve<'a>(sk: &'a Sketch, id: &str) -> &'a solidcraft_sketch::Curve {
    sk.curves.get(sk.curve_index(id).unwrap()).unwrap()
}

fn new_sketch() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    s
}

fn profiles(s: &Session) -> usize {
    s.model.state().sketch(s.active_sketch.unwrap()).unwrap().profiles.len()
}

#[test]
fn construction_line_methods_make_infinite_lines_with_direction_constraints() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [3, 4]], "ids": ["reference"], "infer": false}));
    for (mode, expected) in [
        ("horizontal", Vec2::new(1.0, 0.0)),
        ("vertical", Vec2::new(0.0, 1.0)),
        ("angle", Vec2::new(0.5, 3.0_f64.sqrt() / 2.0)),
        ("parallel", Vec2::new(0.6, 0.8)),
        ("perpendicular", Vec2::new(-0.8, 0.6)),
    ] {
        let out = run(&mut s, "sketch.construction_line", json!({"mode": mode, "at": [10, 10], "curve": "reference", "angle": 60}));
        let sk = sketch(&s);
        let id = out["curves"][0].as_str().unwrap();
        let c = curve(&sk, id);
        assert!(c.infinite && c.construction);
        let CurveKind::Line { a, b } = c.kind else { panic!("expected a line") };
        let direction = (sk.point(b).unwrap() - sk.point(a).unwrap()).normalized().unwrap();
        assert!(direction.cross(expected).abs() < 1e-6, "{mode}: {direction:?}");
        assert_eq!(out["constraints"].as_array().unwrap().len(), 1);
    }
    assert!(s.execute("sketch.construction_line", &json!({"mode": "parallel", "at": [0, 0], "curve": "missing"})).is_err());
}

#[test]
fn construction_angle_keeps_its_parameter_expression() {
    let mut s = new_sketch();
    run(&mut s, "parameters.add", json!({"name":"inclination","expression":"30 deg"}));
    let r = run(&mut s, "sketch.construction_line.angle", json!({"at":[5,5],"angle":"inclination"}));
    let id = r["curves"][0].as_str().unwrap();
    let angle = |s: &Session| {
        let sk = sketch(s);
        let CurveKind::Line { a, b } = curve(&sk, id).kind else { panic!("line") };
        (sk.point(b).unwrap() - sk.point(a).unwrap()).angle().to_degrees()
    };
    assert!((angle(&s) - 30.0).abs() < 1e-6);
    run(&mut s, "parameters.change", json!({"name":"inclination","expression":"60 deg"}));
    assert!((angle(&s) - 60.0).abs() < 1e-6);
}

#[test]
fn construction_common_tangent_exposes_all_four_solutions() {
    let mut s = new_sketch();
    run(&mut s, "sketch.circle.center", json!({"center":[0,0],"radius":5,"id":"a"}));
    run(&mut s, "sketch.circle.center", json!({"center":[20,0],"radius":3,"id":"b"}));
    let p = json!({"at":[10,10],"curve":"a","curve2":"b"});
    let lines = super::super::sketch_construction::construction_line_solutions(&sketch(&s), &p, "tangent_two").unwrap();
    assert_eq!(lines.len(), 4);
    for (i, expected) in lines.iter().enumerate() {
        let mut p = p.clone();
        p["solution"] = json!(i);
        let r = run(&mut s, "sketch.construction_line.tangent_two", p);
        let sk = sketch(&s);
        let CurveKind::Line { a, b } = curve(&sk, r["curves"][0].as_str().unwrap()).kind else { panic!("line") };
        let start = sk.point(a).unwrap();
        let direction = (sk.point(b).unwrap() - start).normalized().unwrap();
        assert!(direction.cross(expected.direction).abs() < 1e-6);
        assert!((start - expected.start).cross(direction).abs() < 1e-6);
        for id in ["a", "b"] {
            let c = sk.curve_index(id).unwrap();
            assert!(((sk.center(c).unwrap() - start).cross(direction).abs() - sk.radius(c).unwrap()).abs() < 1e-6);
        }
    }
}

#[test]
fn construction_tangent_at_an_open_curve_endpoint_reuses_the_point() {
    for (command, params, expected) in [
        ("sketch.spline.control_point", json!({"points":[[0,0],[0,10],[20,10],[20,0]]}), Vec2::Y),
        ("sketch.conic", json!({"start":[0,0],"end":[20,0],"apex":[10,20],"rho":0.5}), Vec2::new(1.0, 2.0)),
    ] {
        let mut s = new_sketch();
        let source = run(&mut s, command, params)["curves"][0].as_str().unwrap().to_string();
        let start = format!("{source}.start");
        let r = run(&mut s, "sketch.construction_line.tangent_at", json!({"curve":source,"at":start}));
        let sk = sketch(&s);
        let CurveKind::Line { a, b } = curve(&sk, r["curves"][0].as_str().unwrap()).kind else { panic!("line") };
        assert_eq!(a, sk.resolve_point(&start).unwrap());
        assert!(sk.point(a).unwrap().dist(Vec2::ZERO) < 1e-7);
        let direction = (sk.point(b).unwrap() - sk.point(a).unwrap()).normalized().unwrap();
        assert!(direction.cross(expected.normalized().unwrap()).abs() < 1e-7, "{command}: {direction:?}");
        assert_eq!(r["constraints"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn construction_line_tangents_touch_the_circle() {
    let mut s = new_sketch();
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 5, "id": "circle"}));
    for (mode, at, alternate) in [("tangent_external", [13, 0], false), ("tangent_external", [13, 0], true), ("tangent_at", [5, 0], false)] {
        let out = run(&mut s, "sketch.construction_line", json!({"mode": mode, "at": at, "curve": "circle", "alternate": alternate}));
        let sk = sketch(&s);
        let id = out["curves"][0].as_str().unwrap();
        let CurveKind::Line { a, b } = curve(&sk, id).kind else { panic!("expected a line") };
        let p = sk.point(a).unwrap();
        let d = (sk.point(b).unwrap() - p).normalized().unwrap();
        let ci = sk.curve_index("circle").unwrap();
        let center = sk.center(ci).unwrap();
        let radius = sk.radius(ci).unwrap();
        assert!(((center - p).cross(d).abs() - radius).abs() < 1e-5);
    }
    assert!(s.execute("sketch.construction_line", &json!({"mode":"tangent_external", "at":[0,0], "curve":"circle"})).is_err());
}

#[test]
fn construction_common_tangent_and_bisector() {
    let mut s = new_sketch();
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 5, "id": "c1"}));
    run(&mut s, "sketch.circle.center", json!({"center": [20, 0], "radius": 3, "id": "c2"}));
    let out = run(&mut s, "sketch.construction_line", json!({"mode":"tangent_two", "curve":"c1", "curve2":"c2", "at":[10, 8]}));
    let sk = sketch(&s);
    let CurveKind::Line { a, b } = curve(&sk, out["curves"][0].as_str().unwrap()).kind else { panic!("expected a line") };
    let start = sk.point(a).unwrap();
    let direction = (sk.point(b).unwrap() - start).normalized().unwrap();
    for id in ["c1", "c2"] {
        let c = sk.curve_index(id).unwrap();
        assert!(((sk.center(c).unwrap() - start).cross(direction).abs() - sk.radius(c).unwrap()).abs() < 1e-5);
    }
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[0,0],[10,0]],"ids":["a"],"infer":false}));
    run(&mut s, "sketch.line", json!({"points":[[0,0],[0,10]],"ids":["b"],"infer":false,"connect":false}));
    let out = run(&mut s, "sketch.construction_line", json!({"mode":"bisector", "curve":"a", "curve2":"b", "at":[5,5]}));
    let sk = sketch(&s);
    let CurveKind::Line { a, b } = curve(&sk, out["curves"][0].as_str().unwrap()).kind else { panic!("expected a line") };
    let direction = (sk.point(b).unwrap() - sk.point(a).unwrap()).normalized().unwrap();
    assert!((direction.x.abs() - direction.y.abs()).abs() < 1e-5);
}

#[test]
fn construction_tools_keep_profiles_and_undo_and_save_as_one_edit() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0":[0,0],"p1":[20,10]}));
    let before = s.doc.to_json();
    let depth = s.undo.len();
    let r = run(&mut s, "sketch.construction_line", json!({"at":[-5,5],"through":[30,5]}));
    assert_eq!(s.undo.len(), depth + 1);
    assert_eq!(profiles(&s), 1);
    let doc = solidcraft_doc::Document::from_json(&s.doc.to_json()).unwrap();
    let id = r["curves"][0].as_str().unwrap();
    let c = curve(doc.sketch(s.active_sketch.unwrap()).unwrap(), id);
    assert!(c.construction && c.infinite);
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(s.doc.to_json(), before);
}

#[test]
fn construction_tangents_respect_arc_sweeps_and_invalid_inputs_are_atomic() {
    let mut s = new_sketch();
    run(&mut s, "sketch.arc.center_point", json!({"center":[0,0],"start":[5,0],"end":[0,5],"id":"arc"}));
    let sk = sketch(&s);
    let p = json!({"at":[13,0],"curve":"arc"});
    let lines = super::super::sketch_construction::construction_line_solutions(&sk, &p, "tangent_external").unwrap();
    assert_eq!(lines.len(), 1, "the lower-circle tangent is outside the quarter arc");
    assert!(lines[0].direction.y > 0.0);
    let r = run(&mut s, "sketch.construction_line.tangent_external", p);
    let sk = sketch(&s);
    let CurveKind::Line { a, b } = curve(&sk, r["curves"][0].as_str().unwrap()).kind else { panic!("line") };
    let actual = (sk.point(b).unwrap() - sk.point(a).unwrap()).normalized().unwrap();
    assert!(actual.cross(lines[0].direction).abs() < 1e-7);
    for p in [
        json!({}),
        json!({"at":[0,0],"curve":"arc","mode":"tangent_external"}),
        json!({"at":[10,10],"mode":"angle","angle":"bad"}),
        json!({"at":[10,10],"mode":"horizontal","solution":99}),
        json!({"at":[10,10],"mode":"horizontal","alternate":"false"}),
        json!({"at":[1,1],"through":[1,1]}),
    ] {
        let before = s.doc.to_json();
        assert!(s.execute("sketch.construction_line", &p).is_err(), "{p}");
        assert_eq!(s.doc.to_json(), before, "{p}");
    }
}

#[test]
fn construction_relations_follow_source_edits_without_an_auxiliary_dimension() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points":[[0,0],[20,0]],"ids":["a"],"infer":false}));
    run(&mut s, "sketch.line", json!({"points":[[0,0],[0,20]],"ids":["b"],"infer":false}));
    let parallel = run(&mut s, "sketch.construction_line.parallel", json!({"curve":"a","at":[5,5]}));
    let bisector = run(&mut s, "sketch.construction_line.bisector", json!({"curve":"a","curve2":"b","at":[10,10]}));
    assert!(sketch(&s).constraints.iter().all(|c| !matches!(c.kind, solidcraft_sketch::ConstraintKind::Distance { .. })));
    run(&mut s, "sketch.drag", json!({"entity":"a.end","from":[20,0],"to":[20,10]}));
    let sk = sketch(&s);
    let direction = |id: &str| {
        let CurveKind::Line { a, b } = curve(&sk, id).kind else { panic!("line") };
        (sk.point(b).unwrap() - sk.point(a).unwrap()).normalized().unwrap()
    };
    let a = direction("a");
    let b = direction("b");
    let p = direction(parallel["curves"][0].as_str().unwrap());
    let d = direction(bisector["curves"][0].as_str().unwrap());
    assert!(a.cross(p).abs() < 1e-5);
    assert!((a.dot(d).abs() - b.dot(d).abs()).abs() < 1e-5);
}

#[test]
fn the_line_type_switches_on_and_off_and_stays_out_of_profiles() {
    let mut s = new_sketch();
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [40, 20]}));
    run(&mut s, "sketch.line", json!({"points": [[20, -10], [20, 30]], "ids": ["axis"]}));
    assert_eq!(profiles(&s), 2, "a normal line cuts the rectangle in two");
    run(&mut s, "sketch.infinite", json!({"curves": ["axis"]}));
    let sk = sketch(&s);
    assert!(curve(&sk, "axis").infinite && curve(&sk, "axis").construction && !curve(&sk, "axis").centerline);
    assert_eq!(profiles(&s), 1, "an infinite line is construction: no profile");
    // Back to a normal line, by toggling or by Normal/Construction.
    run(&mut s, "sketch.infinite", json!({"curves": ["axis"]}));
    let sk = sketch(&s);
    assert!(!curve(&sk, "axis").infinite && !curve(&sk, "axis").construction);
    run(&mut s, "sketch.infinite", json!({"curves": ["axis"], "value": true}));
    run(&mut s, "sketch.construction", json!({"curves": ["axis"], "value": false}));
    assert!(!curve(&sketch(&s), "axis").infinite, "a normal line is a finite one");
    // A centerline is not an infinite line.
    run(&mut s, "sketch.infinite", json!({"curves": ["axis"], "value": true}));
    run(&mut s, "sketch.centerline", json!({"curves": ["axis"], "value": true}));
    let sk = sketch(&s);
    assert!(curve(&sk, "axis").centerline && !curve(&sk, "axis").infinite);
    // Only lines.
    run(&mut s, "sketch.circle.center", json!({"center": [60, 0], "radius": 5, "id": "c"}));
    assert!(s.execute("sketch.infinite", &json!({"curves": ["c"]})).is_err());
}

#[test]
fn it_is_saved_and_a_line_without_it_reads_finite() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 5]], "ids": ["a"]}));
    run(&mut s, "sketch.line", json!({"points": [[0, 10], [10, 15]], "ids": ["b"]}));
    run(&mut s, "sketch.infinite", json!({"curves": ["a"]}));
    let text = s.doc.to_json();
    let back = solidcraft_doc::Document::from_json(&text).unwrap();
    let sk = back.sketch(s.active_sketch.unwrap()).unwrap();
    assert!(curve(sk, "a").infinite);
    assert!(!curve(sk, "b").infinite);
    assert_eq!(text.matches("\"infinite\"").count(), 1, "saved only where it is set");
}

#[test]
fn trim_and_extend_use_the_whole_line_but_never_trim_it() {
    // An infinite vertical line at x = 25 whose two points lie far above the other line.
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[25, 50], [25, 60]], "ids": ["inf"]}));
    run(&mut s, "sketch.infinite", json!({"curves": ["inf"]}));
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0]], "ids": ["l"]}));
    // Trimming the line's right part stops at the infinite line's crossing, x = 25.
    run(&mut s, "sketch.trim", json!({"curve": "l", "at": [35, 0]}));
    let sk = sketch(&s);
    let CurveKind::Line { a, b } = curve(&sk, "l").kind else { panic!("a line") };
    let xs = [sk.point(a).unwrap().x, sk.point(b).unwrap().x];
    assert!(xs.iter().any(|x| (x - 25.0).abs() < 1e-6) && xs.iter().all(|x| *x <= 25.0 + 1e-6), "{xs:?}");
    // The infinite line itself is not trimmed.
    let e = s.execute("sketch.trim", &json!({"curve": "inf", "at": [25, 55]})).unwrap_err().to_string();
    assert!(e.contains("infinite"), "{e}");
    // A short line extends to it, beyond its two points.
    run(&mut s, "sketch.line", json!({"points": [[0, 10], [10, 10]], "ids": ["short"]}));
    run(&mut s, "sketch.extend", json!({"curve": "short", "at": [10, 10]}));
    let sk = sketch(&s);
    let CurveKind::Line { b, .. } = curve(&sk, "short").kind else { panic!("a line") };
    assert!(sk.point(b).unwrap().dist(Vec2::new(25.0, 10.0)) < 1e-6, "{:?}", sk.point(b));
}

#[test]
fn extend_refuses_it_instead_of_moving_it() {
    // An infinite line has no end to grow from: moving one of its two points would move the whole
    // line, so the command says why and leaves it alone.
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[25, 50], [25, 60]], "ids": ["inf"]}));
    run(&mut s, "sketch.infinite", json!({"curves": ["inf"]}));
    let before = s.doc.to_json();
    let e = s.execute("sketch.extend", &json!({"curve": "inf", "at": [25, 50]})).unwrap_err().to_string();
    assert!(e.contains("infinite"), "{e}");
    assert_eq!(s.doc.to_json(), before);
    let sk = sketch(&s);
    let ci = sk.curve_index("inf").expect("the infinite line");
    assert!(crate::cmd::extend_preview(&sk, ci, Vec2::new(25.0, 50.0)).is_none(), "no preview to offer");
}

#[test]
fn fillets_and_chamfers_refuse_it_and_mirrors_copy_it() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [20, 0], [20, 20]], "ids": ["a", "b"]}));
    run(&mut s, "sketch.infinite", json!({"curves": ["a"]}));
    let before = s.doc.to_json();
    for (cmd, p) in [
        ("sketch.fillet", json!({"point": "a.end", "radius": 2})),
        ("sketch.fillet", json!({"a": "a", "b": "b", "radius": 2})),
        ("sketch.chamfer.equal_distance", json!({"point": "a.end", "distance": 2})),
    ] {
        let e = s.execute(cmd, &p).unwrap_err().to_string();
        assert!(e.contains("infinite"), "{cmd}: {e}");
    }
    assert_eq!(s.doc.to_json(), before);
    run(&mut s, "sketch.line", json!({"points": [[-10, -5], [-10, 5]], "ids": ["m"]}));
    let r = run(&mut s, "sketch.mirror", json!({"entities": ["a"], "line": "m"}));
    let made = r["curves"].as_array().unwrap().iter().filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>();
    let sk = sketch(&s);
    assert!(made.iter().any(|id| curve(&sk, id).infinite), "{r}");
}

#[test]
fn an_offset_of_it_is_infinite_too() {
    let mut s = new_sketch();
    run(&mut s, "sketch.line", json!({"points": [[0, 0], [10, 0]], "ids": ["inf"]}));
    run(&mut s, "sketch.infinite", json!({"curves": ["inf"]}));
    let r = run(&mut s, "sketch.offset", json!({"curves": ["inf"], "distance": 5, "side": [5, 5]}));
    let made = r["curves"].as_array().unwrap().iter().filter_map(Value::as_str).map(str::to_string).collect::<Vec<_>>();
    let sk = sketch(&s);
    assert!(!made.is_empty() && made.iter().all(|id| curve(&sk, id).infinite), "{r}");
    // It stays infinite when the offset follows a new distance.
    let param = r["param"].as_str().unwrap().to_string();
    run(&mut s, "parameters.change", json!({"name": param, "expression": "8 mm"}));
    let sk = sketch(&s);
    assert!(made.iter().all(|id| curve(&sk, id).infinite));
}
