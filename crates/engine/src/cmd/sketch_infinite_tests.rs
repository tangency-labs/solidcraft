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
