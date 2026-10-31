//! `sketch.trim.path`: dragging across curves trims each piece the path crosses, as clicking it
//! with `sketch.trim` would, in one undo step.

use serde_json::{Value, json};
use solidcraft_geom::Vec2;
use solidcraft_sketch::Shape;

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn ids(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn sketch(s: &Session) -> solidcraft_sketch::Sketch {
    let id = s.active_sketch.unwrap();
    s.model.state().sketch(id).unwrap().sketch.clone()
}

fn new_sketch() -> Session {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    s
}

fn line(s: &mut Session, points: Value) -> Vec<String> {
    ids(&run(s, "sketch.line", json!({"points": points}))["curves"])
}

/// A 40 x 40 square with a step cut across its top right corner (as in the single trim's
/// dimension test), the right side dimensioned.
fn square_with_step() -> (Session, String, String) {
    let mut s = new_sketch();
    let sq = ids(&run(&mut s, "sketch.line", json!({"points": [[0, 0], [40, 0], [40, 40], [0, 40]], "closed": true, "infer": true}))["curves"]);
    let (right, top) = (sq[1].clone(), sq[2].clone());
    run(&mut s, "sketch.dimension", json!({"entities": [right]}));
    let step = ids(&run(&mut s, "sketch.line", json!({"points": [[23, 40], [23, 23], [40, 23]], "infer": true}))["curves"]);
    run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{}.start", step[0]), "b": top}));
    run(&mut s, "sketch.constraint.coincident", json!({"a": format!("{}.end", step[1]), "b": right}));
    (s, right, top)
}

#[test]
fn a_path_trims_what_clicking_each_crossed_piece_would() {
    // Clicks: the right side above the step, then the top side right of it.
    let (mut clicked, right, top) = square_with_step();
    run(&mut clicked, "sketch.trim", json!({"curve": right, "at": [40, 30]}));
    run(&mut clicked, "sketch.trim", json!({"curve": top, "at": [30, 40]}));

    // One drag from outside the right side, across the corner, out through the top.
    let (mut dragged, ..) = square_with_step();
    let before = sketch(&dragged);
    let undo_before = dragged.history.len();
    let out = run(&mut dragged, "sketch.trim.path", json!({"points": [[45, 30], [30, 30], [30, 45]]}));
    assert_eq!(out["trimmed"].as_array().map(Vec::len), Some(2), "{out}");
    assert_eq!(out["removed"].as_array().map(Vec::len), Some(2), "{out}");
    assert_eq!(sketch(&dragged), sketch(&clicked), "the drag and the two clicks leave the same sketch");

    // One undo step brings everything back.
    assert_eq!(dragged.history.len(), undo_before + 1);
    run(&mut dragged, "edit.undo", json!({}));
    assert_eq!(sketch(&dragged), before);
}

#[test]
fn a_piece_crossed_twice_is_trimmed_once() {
    let mut s = new_sketch();
    line(&mut s, json!([[0, 0], [20, 0]]));
    line(&mut s, json!([[0, 10], [20, 10]]));
    let v = line(&mut s, json!([[10, -5], [10, 15]]))[0].clone();
    // A zigzag across the middle piece of the vertical line, three times.
    let out = run(&mut s, "sketch.trim.path", json!({"points": [[5, 3], [15, 4], [5, 5], [15, 6]]}));
    assert_eq!(out["trimmed"].as_array().map(Vec::len), Some(1), "{out}");
    let removed = out["removed"][0].as_array().unwrap();
    let ends: Vec<Vec2> =
        [removed.first().unwrap(), removed.last().unwrap()].iter().map(|q| Vec2::new(q[0].as_f64().unwrap(), q[1].as_f64().unwrap())).collect();
    assert!(ends.iter().any(|q| q.dist(Vec2::new(10.0, 0.0)) < 1e-6) && ends.iter().any(|q| q.dist(Vec2::new(10.0, 10.0)) < 1e-6), "{ends:?}");
    // The two outer pieces of the vertical line stay, the original id on the lower one.
    let sk = sketch(&s);
    assert_eq!(sk.curves.len(), 4);
    let lower = sk.shape(sk.curve_index(&v).unwrap()).unwrap();
    assert!(matches!(lower, Shape::Line { a, b } if a.dist(Vec2::new(10.0, -5.0)) < 1e-6 && b.dist(Vec2::new(10.0, 0.0)) < 1e-6), "{lower:?}");
}

/// A scene built twice: trimmed once along `path` and once by clicking `clicks` (curve index
/// in the scene's curves, where) in turn; both must leave the same sketch.
fn same_as_clicks(build: fn(&mut Session) -> Vec<String>, path: Value, clicks: &[(usize, [f64; 2])]) {
    let mut clicked = new_sketch();
    let curves = build(&mut clicked);
    for (k, at) in clicks {
        run(&mut clicked, "sketch.trim", json!({"curve": curves[*k], "at": at}));
    }
    let mut dragged = new_sketch();
    build(&mut dragged);
    let out = run(&mut dragged, "sketch.trim.path", json!({"points": path}));
    assert_eq!(out["trimmed"].as_array().map(Vec::len), Some(clicks.len()), "{out}");
    assert!(out["removed"].as_array().unwrap().iter().all(|r| r.as_array().is_some_and(|r| r.len() >= 2)), "{out}");
    assert_eq!(sketch(&dragged), sketch(&clicked), "{path}");
}

#[test]
fn circles_arcs_and_splines_trim_along_a_path() {
    // A circle across a line: the lower half goes, the rest is an arc.
    same_as_clicks(
        |s| {
            let c = ids(&run(s, "sketch.circle.center", json!({"center": [0, 0], "radius": 10}))["curves"]);
            [c, line(s, json!([[-20, 0], [20, 0]]))].concat()
        },
        json!([[0, -15], [0, -5]]),
        &[(0, [0.0, -10.0])],
    );
    // An arc cut by a line through its middle: the left part goes.
    same_as_clicks(
        |s| {
            let a = ids(&run(s, "sketch.arc.three_point", json!({"start": [-10, 0], "through": [0, 10], "end": [10, 0]}))["curves"]);
            [a, line(s, json!([[0, -5], [0, 15]]))].concat()
        },
        json!([[-10, 8], [-3, 8]]),
        &[(0, [-6.0, 8.0])],
    );
    // A spline over a line: its hump goes, then the line's piece under it.
    same_as_clicks(
        |s| {
            let sp =
                ids(&run(s, "sketch.spline.fit_point", json!({"points": [[-20, -20], [-10, 5], [10, 5], [20, -20]], "connect": false}))["curves"]);
            [sp, line(s, json!([[-30, 0], [30, 0]]))].concat()
        },
        json!([[0, 10], [0, -1]]),
        &[(0, [0.0, 10.0]), (1, [0.0, 0.0])],
    );
}

#[test]
fn a_dry_run_reports_the_pieces_without_changing_the_design() {
    let (mut s, ..) = square_with_step();
    let before = sketch(&s);
    let (undo, revision) = (s.history.len(), s.revision);
    let path = json!([[45, 30], [30, 30], [30, 45]]);
    let dry = run(&mut s, "sketch.trim.path", json!({"points": path, "dry_run": true}));
    assert_eq!(dry["dry_run"], true);
    assert_eq!(sketch(&s), before);
    assert_eq!((s.history.len(), s.revision), (undo, revision), "a dry run is no step");
    let real = run(&mut s, "sketch.trim.path", json!({"points": path}));
    assert_eq!(dry["removed"], real["removed"]);
    assert_eq!(dry["trimmed"], real["trimmed"]);
}

#[test]
fn curves_trim_cannot_change_are_passed_over() {
    let mut s = new_sketch();
    let inf = line(&mut s, json!([[0, -10], [0, 10]]))[0].clone();
    run(&mut s, "sketch.infinite", json!({"curves": [inf]}));
    let h = line(&mut s, json!([[-20, 0], [20, 0]]))[0].clone();
    // Crossing the infinite line leaves it alone; the horizontal line's right part goes.
    let out = run(&mut s, "sketch.trim.path", json!({"points": [[-5, 5], [10, 5], [10, -5]]}));
    assert_eq!(out["trimmed"].as_array().map(Vec::len), Some(1), "{out}");
    let sk = sketch(&s);
    assert!(sk.curves[sk.curve_index(&inf).unwrap()].infinite);
    let rest = sk.shape(sk.curve_index(&h).unwrap()).unwrap();
    assert!(matches!(rest, Shape::Line { a, b } if a.dist(Vec2::new(-20.0, 0.0)) < 1e-6 && b.dist(Vec2::new(0.0, 0.0)) < 1e-6), "{rest:?}");
}

#[test]
fn hostile_paths_fail_cleanly() {
    let mut s = new_sketch();
    line(&mut s, json!([[0, 0], [10, 0]]));
    let before = sketch(&s);
    let long: Vec<[f64; 2]> = (0..2_001).map(|i| [i as f64, 1.0]).collect();
    for p in [
        json!({}),
        json!({"points": []}),
        json!({"points": [[0, 1]]}),
        json!({"points": "x"}),
        json!({"points": [[0, 1], ["a", 2]]}),
        json!({"points": long}),
        json!({"points": [[50, 50], [60, 60]]}),
        json!({"points": [[1e308, -1e308], [-1e308, 1e308]]}),
        json!({"points": [[5, 1], [5, 1]]}),
    ] {
        assert!(s.execute("sketch.trim.path", &p).is_err(), "{p}");
        assert_eq!(sketch(&s), before, "{p}");
    }
    // Outside a sketch.
    let mut none = Session::default();
    assert!(none.execute("sketch.trim.path", &json!({"points": [[0, -1], [0, 1]]})).is_err());
}
