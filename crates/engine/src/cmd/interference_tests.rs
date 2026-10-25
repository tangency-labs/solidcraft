//! `inspect.interference`: the mesh pre-filter, the bodies it must not clear, and the budget.

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}

/// Curved bodies whose boxes overlap but which stay apart are answered from their meshes: no
/// boolean runs. Two cylinders on a diagonal (centres 12.7 mm apart, radii 5), and a torus
/// beside a sphere that sits in the corner of its box.
#[test]
fn separated_curved_bodies_are_cleared_without_a_boolean() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 10, "base": [0, 0, 0], "body_name": "C1"}));
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 10, "base": [9, 9, 0], "operation": "new", "body_name": "C2"}));
    run(&mut s, "solid.torus", json!({"major": 20, "minor": 3, "center": [60, 0, 0], "body_name": "Torus"}));
    // In the torus box's corner (the box reaches 23 out on each axis), 1.66 mm clear of the tube.
    run(&mut s, "solid.sphere", json!({"radius": 3, "center": [80, 19, 1], "operation": "new", "body_name": "Ball"}));
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!(i["count"], 0, "{i}");
    assert_eq!(i["pairs_candidate"], 2, "{i}");
    assert_eq!(i["pairs_cleared"], 2, "{i}");
    assert_eq!(i["pairs_intersected"], 0, "{i}");
    assert_eq!(f(&i["boolean_ms"]), 0.0, "{i}");
    assert_eq!(i["unchecked"].as_array().map(Vec::len), Some(0), "{i}");
    // Moved into each other, the same pairs are intersected and overlap.
    run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 10, "base": [7, 0, 0], "operation": "new", "body_name": "C3"}));
    let i = run(&mut s, "inspect.interference", json!({"bodies": ["C1", "C3"]}));
    assert_eq!(i["count"], 1, "{i}");
    assert_eq!(i["pairs_intersected"], 1, "{i}");
    // The lens of two r5 circles 7 apart, 10 high.
    let (r, d): (f64, f64) = (5.0, 7.0);
    let lens = 2.0 * r * r * (d / (2.0 * r)).acos() - d / 2.0 * (4.0 * r * r - d * d).sqrt();
    let got = f(&i["interferences"][0]["volume_mm3"]);
    assert!((got - 10.0 * lens).abs() < 1e-3 * 10.0 * lens, "{got} against {}: {i}", 10.0 * lens);
}

/// A body wholly inside another has meshes that never meet: it is still intersected, and its
/// whole volume is the overlap.
#[test]
fn a_body_inside_another_is_still_found() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 40, "height": 40, "corner": [-20, -20, -20], "body_name": "Block"}));
    run(&mut s, "solid.sphere", json!({"radius": 5, "center": [3, -2, 1], "operation": "new", "body_name": "Ball"}));
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!(i["count"], 1, "{i}");
    assert_eq!(i["pairs_cleared"], 0, "{i}");
    let want = 4.0 / 3.0 * std::f64::consts::PI * 125.0;
    let got = f(&i["interferences"][0]["volume_mm3"]);
    assert!((got - want).abs() < 1e-3 * want, "{got} against {want}: {i}");
}

/// A body in two pieces, one of them wholly inside a block and the other outside it: the boxes
/// are not nested and the meshes never meet, but a point of the inner piece's faces is inside
/// the block, so the pair is intersected. The whole bodies' boolean comes out empty there, so
/// the pieces are intersected one by one.
#[test]
fn a_piece_of_a_body_inside_another_is_still_found() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [0, 0, 0], "body_name": "Block"}));
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [5, 5, 5], "operation": "new", "body_name": "Pieces"}));
    run(&mut s, "solid.box", json!({"length": 2, "width": 2, "height": 2, "corner": [30, 5, 5], "operation": "join", "targets": ["Pieces"]}));
    let st = s.world_state().unwrap();
    let pieces = st.bodies.iter().find(|b| b.name == "Pieces").map(|b| b.body.shell_count());
    assert_eq!(pieces, Some(2), "the two cubes are one body in two pieces");
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!(i["count"], 1, "{i}");
    assert_eq!(i["pairs_cleared"], 0, "{i}");
    assert!((f(&i["interferences"][0]["volume_mm3"]) - 8.0).abs() < 1e-6, "{i}");
}

/// A budget spent before any pair could start lists every pair as not checked (and the call
/// returns); bad budgets are refused.
#[test]
fn pairs_left_when_the_budget_runs_out_are_listed_not_checked() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "A"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [5, 0, 0], "operation": "new", "body_name": "B"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [12, 0, 0], "operation": "new", "body_name": "C"}));
    let i = run(&mut s, "inspect.interference", json!({"budget_s": 1e-12}));
    assert_eq!(i["count"], 0, "{i}");
    assert_eq!(i["budget_exceeded"], true, "{i}");
    assert_eq!(i["pairs_not_checked"], 2, "{i}");
    let unchecked = i["unchecked"].as_array().cloned().unwrap_or_default();
    assert_eq!(unchecked.len(), 2, "{i}");
    assert!(unchecked.iter().all(|u| u["reason"].as_str().is_some_and(|r| r.starts_with("not checked") && r.contains("time budget"))), "{i}");
    // The default budget checks both pairs: 500 and 300 mm³.
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!(i["budget_exceeded"], false, "{i}");
    assert_eq!(i["budget_s"], 240.0, "{i}");
    assert_eq!(i["count"], 2, "{i}");
    let mut v: Vec<f64> = i["interferences"].as_array().into_iter().flatten().map(|h| f(&h["volume_mm3"])).collect();
    v.sort_by(f64::total_cmp);
    assert!((v[0] - 300.0).abs() < 1e-6 && (v[1] - 500.0).abs() < 1e-6, "{i}");
    // Answers come in pair order whatever order they finished in.
    assert_eq!((i["interferences"][0]["a"].as_str(), i["interferences"][0]["b"].as_str()), (Some("A"), Some("B")), "{i}");
    for bad in [json!(0), json!(-1), json!("soon"), json!(f64::MAX), json!(1e6)] {
        assert!(s.execute("inspect.interference", &json!({"budget_s": bad})).is_err(), "{bad}");
    }
}

/// Two STL copies of a cube inserted into a design. Returns the names of the two mesh bodies.
pub(super) fn insert_two_mesh_cubes(s: &mut Session, tag: &str) -> Vec<String> {
    let dir = std::env::temp_dir().join(format!("solidcraft-mesh-unchecked-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let stl = dir.join("cube.stl");
    let mut cube = Session::default();
    run(&mut cube, "solid.box", json!({"length": 20, "width": 20, "height": 20, "corner": [0, 0, 0], "body_name": "Cube"}));
    run(&mut cube, "file.export", json!({"path": stl.to_string_lossy()}));
    for _ in 0..2 {
        run(s, "file.insert_mesh", json!({"path": stl.to_string_lossy()}));
    }
    let _ = std::fs::remove_dir_all(&dir);
    s.world_state().unwrap().bodies.iter().filter(|b| b.body.is_mesh()).map(|b| b.name.clone()).collect()
}

/// #3249: mesh bodies (STL, 3MF, OBJ imports) are not intersected, but they used to vanish from
/// the answer: `count: 0, unchecked: []` read as "no collision". Each is now listed in `unchecked`
/// with a reason, and the solid bodies are checked as before.
#[test]
fn mesh_bodies_are_listed_as_unchecked_not_dropped() {
    let mut s = Session::default();
    let meshes = insert_two_mesh_cubes(&mut s, "interference");
    assert_eq!(meshes.len(), 2, "{meshes:?}");
    // Only meshes: nothing is checked, and both are said to be left out.
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!((i["count"].as_u64(), i["checked"].as_u64()), (Some(0), Some(0)), "{i}");
    let unchecked = i["unchecked"].as_array().cloned().unwrap_or_default();
    assert_eq!(unchecked.len(), 2, "{i}");
    for (u, name) in unchecked.iter().zip(&meshes) {
        assert_eq!(u["a"].as_str(), Some(name.as_str()), "{i}");
        assert!(u["b"].is_null(), "{i}");
        assert_eq!(u["reason"], "mesh body: not checked", "{i}");
    }
    // With solid bodies beside them: those are checked, the meshes are still listed.
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [100, 0, 0], "operation": "new", "body_name": "A"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [105, 0, 0], "operation": "new", "body_name": "B"}));
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!((i["count"].as_u64(), i["checked"].as_u64()), (Some(1), Some(2)), "{i}");
    assert_eq!(i["unchecked"].as_array().map(Vec::len), Some(2), "{i}");
    // Naming only solid bodies lists no mesh; naming a mesh body lists just that one.
    let i = run(&mut s, "inspect.interference", json!({"bodies": ["A", "B"]}));
    assert_eq!(i["unchecked"].as_array().map(Vec::len), Some(0), "{i}");
    let i = run(&mut s, "inspect.interference", json!({"bodies": ["A", meshes[0]]}));
    assert_eq!(i["checked"], 1, "{i}");
    assert_eq!(i["unchecked"][0]["a"].as_str(), Some(meshes[0].as_str()), "{i}");
}
