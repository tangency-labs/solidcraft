use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

/// The other sketch: a line on the XZ plane that crosses the XY plane at x = 5.
fn other_sketch(s: &mut Session) -> u64 {
    let id = run(s, "sketch.create", json!({"plane": "XZ"}))["sketch"].as_u64().unwrap();
    run(s, "sketch.line", json!({"points": [[5, -5], [5, 5]], "ids": ["l1"]}));
    run(s, "sketch.finish", json!({}));
    id
}

/// The point of the sketch being edited, by id.
fn point(si: &Value, id: &str) -> Value {
    si["points"].as_array().unwrap().iter().find(|p| p["id"] == id).unwrap().clone()
}

/// A Pierce constraint holds a point of this sketch where a curve of another sketch crosses
/// this sketch's plane (#3599): the point of the XY sketch sits on the XZ sketch's line.
#[test]
#[ignore = "the engine command sketch.constraint.pierce is not implemented yet"]
fn a_pierce_constraint_holds_a_point_where_another_sketches_curve_crosses_this_plane() {
    let mut s = Session::default();
    let other = other_sketch(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.point", json!({"point": [20, 20], "id": "p1"}));

    let r = run(&mut s, "sketch.constraint.pierce", json!({"point": "p1", "curve": {"sketch": other, "curve": "l1"}}));
    assert!(r["constraint"].is_string(), "{r}");

    // The point of this sketch now sits where the other sketch's line crosses this plane.
    let si = run(&mut s, "sketch.inspect", json!({}));
    let p = point(&si, "p1");
    assert!((p["at"][0].as_f64().unwrap() - 5.0).abs() < 1e-9, "{p}");
    assert!(p["at"][1].as_f64().unwrap().abs() < 1e-9, "{p}");
    assert!((p["world"][2].as_f64().unwrap()).abs() < 1e-9, "{p}");

    // The constraint is a Pierce one, named as such.
    let c = si["constraints"].as_array().unwrap().iter().find(|c| c["name"] == "Pierce").unwrap().clone();
    assert_eq!(c["type"], "pierce", "{c}");
    assert_eq!(c["sketch"], other, "{c}");
    assert_eq!(c["curve"], "l1", "{c}");

    // The point is held there, not free to be dragged off the curve.
    assert_eq!(p["fixed"], true, "{p}");
}

/// The point follows the other sketch: the curve moves, the pierced point moves with it.
#[test]
#[ignore = "the engine command sketch.constraint.pierce is not implemented yet"]
fn a_pierced_point_follows_the_curve_of_the_other_sketch() {
    let mut s = Session::default();
    let other = other_sketch(&mut s);
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.point", json!({"point": [20, 20], "id": "p1"}));
    run(&mut s, "sketch.constraint.pierce", json!({"point": "p1", "curve": {"sketch": other, "curve": "l1"}}));

    // The other sketch's line moves across this plane: the pierced point follows it.
    run(&mut s, "sketch.edit", json!({"sketch": other}));
    run(&mut s, "sketch.line", json!({"points": [[7, -5], [7, 5]], "ids": ["l2"]}));
    run(&mut s, "sketch.finish", json!({}));

    let si = run(&mut s, "sketch.inspect", json!({"sketch": "XY"}));
    let p = point(&si, "p1");
    assert!((p["at"][0].as_f64().unwrap() - 7.0).abs() < 1e-9, "{p}");
}

/// A Pierce constraint needs a curve of another sketch: anything else is refused.
#[test]
#[ignore = "the engine command sketch.constraint.pierce is not implemented yet"]
fn a_pierce_constraint_needs_a_curve_of_another_sketch() {
    let mut s = Session::default();
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.point", json!({"point": [20, 20], "id": "p1"}));
    let e = s.execute("sketch.constraint.pierce", &json!({"point": "p1", "curve": {"sketch": "p1"}})).unwrap_err().to_string();
    assert!(e.contains("curve"), "{e}");
}
