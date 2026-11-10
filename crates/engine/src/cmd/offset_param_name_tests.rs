#![allow(clippy::unwrap_used, clippy::panic)]
use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, command: &str, params: Value) -> Value {
    s.execute(command, &params).unwrap_or_else(|error| panic!("{command}: {error}"))
}

fn rectangle() -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name":"wall","expression":"1 mm"}));
    run(&mut s, "sketch.create", json!({"plane":"XY","name":"S"}));
    run(&mut s, "sketch.line", json!({"points":[[0,0],[20,0],[20,10],[0,10]],"closed":true}));
    s
}

/// The lowest and highest y of the offset curves' end points, rounded to 1e-6.
fn inset(s: &Session, curves: &[String]) -> (f64, f64) {
    let sketch = s.model.state().sketch(s.active_sketch.unwrap()).unwrap().sketch.clone();
    let ys: Vec<f64> = curves
        .iter()
        .flat_map(|c| [format!("{c}.start"), format!("{c}.end")])
        .map(|id| sketch.point(sketch.resolve_point(&id).unwrap()).unwrap().y)
        .collect();
    let round = |v: f64| (v * 1e6).round() / 1e6;
    (round(ys.iter().cloned().fold(f64::INFINITY, f64::min)), round(ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max)))
}

/// #12295: `param` names the offset's driving dimension.
#[test]
fn param_names_the_offset_dimension() {
    let mut s = rectangle();
    let r = run(&mut s, "sketch.offset", json!({"curves":["l1"],"distance":"wall","side":[10,5],"param":"w_in"}));
    assert_eq!(r["param"], "w_in");
    assert_eq!(r["sides"][0]["param"], "w_in");
    assert_eq!(s.doc.param("w_in").unwrap().expr, "wall");
    assert!(s.doc.param("d1").is_none());
    // The named dimension drives the offset: the inner rectangle's edges sit `wall` in from the
    // outer one, and follow `wall`, then `w_in` itself once it is changed.
    let curves: Vec<String> = r["curves"].as_array().unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect();
    assert_eq!(inset(&s, &curves), (1.0, 9.0));
    run(&mut s, "parameters.change", json!({"name":"wall","expression":"2 mm"}));
    assert_eq!(inset(&s, &curves), (2.0, 8.0));
    run(&mut s, "parameters.change", json!({"name":"w_in","expression":"3 mm"}));
    assert_eq!(inset(&s, &curves), (3.0, 7.0));
    assert_eq!(s.doc.param("w_in").unwrap().expr, "3 mm");
}

/// `param2` names the dimension of the second side.
#[test]
fn param2_names_the_second_side() {
    let mut s = rectangle();
    let r = run(&mut s, "sketch.offset", json!({"curves":["l1"],"distance":1,"distance2":2,"param":"inner","param2":"outer"}));
    assert_eq!(r["param"], "inner");
    assert_eq!(r["param2"], "outer");
    assert_eq!(s.doc.param("inner").unwrap().expr, "1 mm");
    assert_eq!(s.doc.param("outer").unwrap().expr, "2 mm");
}

/// A bad or taken name is refused and changes nothing.
#[test]
fn a_bad_param_name_is_refused() {
    for (params, says) in [
        (json!({"param":"wall"}), "already a parameter"),
        (json!({"param":"2x"}), "not a valid parameter name"),
        (json!({"param":"a","distance2":2,"param2":"a"}), "different"),
        (json!({"param2":"b"}), "distance2"),
        (json!({"param":5}), "`param` must be a parameter name"),
        (json!({"param2":["b"],"distance2":2}), "`param2` must be a parameter name"),
    ] {
        let mut s = rectangle();
        let before = s.doc.to_json();
        let mut p = json!({"curves":["l1"],"distance":1,"side":[10,5]});
        p.as_object_mut().unwrap().extend(params.as_object().unwrap().clone());
        let error = s.execute("sketch.offset", &p).expect_err(&p.to_string()).to_string();
        assert!(error.contains(says), "{p}: {error}");
        assert_eq!(s.doc.to_json(), before, "{p}");
    }
}

/// A chosen `param2` of `d1` is free in a fresh design: the unnamed first side takes the next
/// generated name instead of `d1`.
#[test]
fn param2_may_take_the_first_generated_name() {
    let mut s = rectangle();
    let r = run(&mut s, "sketch.offset", json!({"curves":["l1"],"distance":1,"distance2":2,"param2":"d1"}));
    assert_eq!(r["param2"], "d1");
    assert_eq!(s.doc.param("d1").unwrap().expr, "2 mm");
    let first = r["param"].as_str().unwrap();
    assert_ne!(first, "d1");
    assert_eq!(s.doc.param(first).unwrap().expr, "1 mm");
}
