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

/// A name that is no body is refused, as inspect.clearance refuses it. Dropped, a misspelled or
/// wrongly cased body was never checked and two overlapping cubes read as no interference (#9740).
#[test]
fn a_body_name_that_is_no_body_is_refused() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "A"}));
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [9, 0, 0], "operation": "new_body", "body_name": "B"}));
    let i = run(&mut s, "inspect.interference", json!({"bodies": ["A", "B"]}));
    assert_eq!((&i["count"], &i["checked"]), (&json!(1), &json!(2)), "{i}");
    for (names, says) in
        [(json!(["A", "Typo"]), "no body `Typo`"), (json!(["A", "b"]), "no body `b`; did you mean `B`?"), (json!(["Typo"]), "no body `Typo`")]
    {
        let e = s.execute("inspect.interference", &json!({"bodies": names})).map(|r| r.to_string()).unwrap_or_else(|e| e.to_string());
        assert!(e.contains(says), "{names}: {e}");
    }
    assert!(
        !s.execute("inspect.interference", &json!({"bodies": ["Typo"]})).unwrap_err().to_string().contains("did you mean"),
        "nothing is near `Typo`"
    );
    // A long name a rename gives (100 CJK characters, 300 bytes) still gets its suggestion.
    let long = "中".repeat(100);
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [50, 0, 0], "operation": "new_body", "body_name": long}));
    let typo = format!("{}国", "中".repeat(99));
    let e = s.execute("inspect.interference", &json!({"bodies": ["A", typo]})).unwrap_err().to_string();
    assert!(e.contains(&format!("did you mean `{long}`?")), "{e}");
}

/// `exclude_pairs` leaves a pair out by body or by occurrence, and each pair reported says where
/// its bodies sit (#5963).
#[test]
fn excluded_pairs_are_left_out_and_pairs_name_their_place() {
    let mut s = Session::default();
    for (name, x) in [("Pin", 0), ("Hole", 5)] {
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "component.create", json!({"name": name}));
        run(&mut s, "solid.cylinder", json!({"radius": 5, "height": 10, "base": [x, 0, 0], "body_name": name}));
    }
    let i = run(&mut s, "inspect.interference", json!({}));
    assert_eq!((i["count"].as_u64(), i["pairs_excluded"].as_u64()), (Some(1), Some(0)), "{i}");
    let hit = &i["interferences"][0];
    for side in ["a", "b"] {
        let part = &hit[format!("{side}_part")];
        let component = part["component"].as_str().unwrap_or_default();
        assert!(["Pin", "Hole"].contains(&component), "{hit}");
        assert_eq!(part["path"], json!([format!("{component}:1")]), "{hit}");
    }
    for pair in [json!(["Pin", "Hole"]), json!(["Hole:1", "Pin:1"]), json!([hit["a"], hit["b"]])] {
        let i = run(&mut s, "inspect.interference", json!({"exclude_pairs": [pair]}));
        assert_eq!((i["count"].as_u64(), i["pairs_excluded"].as_u64()), (Some(0), Some(1)), "{pair}: {i}");
    }
    for bad in [json!("Pin"), json!([["Pin"]]), json!([["Pin", "Nope"]]), json!([["root", "Pin"]])] {
        assert!(s.execute("inspect.interference", &json!({"exclude_pairs": bad})).is_err(), "{bad}");
    }
}

/// A component's envelope is left out unless named, never meets a body inside it, and with
/// `envelopes` is intersected instead of the bodies inside it: a 10 mm housing and a 4 mm insert
/// sharing 8 mm³ in a component, and a lid that overlaps the envelope but neither part.
#[test]
fn an_envelope_stands_in_for_its_components_bodies_when_asked() {
    let mut s = Session::default();
    let cube = |s: &mut Session, name: &str, size: f64, corner: [f64; 3]| {
        run(s, "solid.box", json!({"length": size, "width": size, "height": size, "corner": corner, "operation": "new", "body_name": name}));
    };
    run(&mut s, "component.create", json!({"name": "Bought"}));
    cube(&mut s, "Housing", 10.0, [0.0, 0.0, 0.0]);
    run(&mut s, "component.create", json!({"name": "Insert part"}));
    cube(&mut s, "Insert", 4.0, [8.0, 3.0, 3.0]);
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "solid.bounding_solid", json!({"component": "Bought", "body_name": "Envelope"}));
    // Over the insert's end (x 10..12), above the housing: inside the envelope only.
    cube(&mut s, "Lid", 1.0, [10.5, 0.0, 0.0]);
    let names = |i: &Value| -> Vec<[String; 2]> {
        i["interferences"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|h| {
                let mut p = [h["a"].as_str().unwrap_or("").to_string(), h["b"].as_str().unwrap_or("").to_string()];
                p.sort();
                p
            })
            .collect()
    };
    let all = run(&mut s, "inspect.interference", json!({}));
    assert_eq!(names(&all), vec![["Housing".to_string(), "Insert".to_string()]], "{all}");
    assert!((f(&all["interferences"][0]["volume_mm3"]) - 2.0 * 4.0 * 4.0).abs() < 1e-6, "{all}");
    assert_eq!(all["checked"], 3, "{all}");
    let env = run(&mut s, "inspect.interference", json!({"envelopes": true}));
    assert_eq!(names(&env), vec![["Envelope".to_string(), "Lid".to_string()]], "{env}");
    assert!((f(&env["interferences"][0]["volume_mm3"]) - 1.0).abs() < 1e-6, "{env}");
    assert_eq!(env["checked"], 2, "{env}");
    // A pair naming a body inside the envelope excludes the envelope's pair.
    let excluded = run(&mut s, "inspect.interference", json!({"envelopes": true, "exclude_pairs": [["Housing", "Lid"]]}));
    assert_eq!((excluded["count"].as_u64(), excluded["pairs_excluded"].as_u64()), (Some(0), Some(1)), "{excluded}");
    // Named, the envelope is checked, but not against the housing it encloses.
    let named = run(&mut s, "inspect.interference", json!({"bodies": ["Envelope", "Housing", "Lid"]}));
    assert_eq!(names(&named), vec![["Envelope".to_string(), "Lid".to_string()]], "{named}");
    assert_eq!(named["pairs_excluded"], 1, "{named}");
}

/// A body added to an enveloped component after its envelope, outside the box, is checked on its
/// own with `envelopes`: the envelope doesn't stand in for what it doesn't hold.
#[test]
fn a_body_outside_its_components_envelope_is_checked_on_its_own() {
    let mut s = Session::default();
    let cube = |s: &mut Session, name: &str, size: f64, corner: [f64; 3]| {
        run(s, "solid.box", json!({"length": size, "width": size, "height": size, "corner": corner, "operation": "new", "body_name": name}));
    };
    run(&mut s, "component.create", json!({"name": "Bought"}));
    cube(&mut s, "Housing", 10.0, [0.0, 0.0, 0.0]);
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "solid.bounding_solid", json!({"component": "Bought", "body_name": "Envelope"}));
    run(&mut s, "component.activate", json!({"component": "Bought"}));
    cube(&mut s, "Late", 10.0, [100.0, 0.0, 0.0]);
    run(&mut s, "component.activate", json!({"component": "root"}));
    cube(&mut s, "Other", 10.0, [105.0, 0.0, 0.0]);
    let env = run(&mut s, "inspect.interference", json!({"envelopes": true}));
    let hits: Vec<_> = env["interferences"].as_array().unwrap().iter().map(|h| (h["a"].as_str().unwrap(), h["b"].as_str().unwrap())).collect();
    assert!(hits.iter().any(|&(a, b)| [a, b].contains(&"Late") && [a, b].contains(&"Other")), "{env}");
}

/// A small overlap with a large envelope is a finding, whichever order the pair comes in: the
/// shared volume is measured against the smaller body, not the envelope's span.
#[test]
fn a_small_overlap_with_a_large_envelope_is_found() {
    let mut s = Session::default();
    run(&mut s, "component.create", json!({"name": "Big"}));
    run(&mut s, "solid.box", json!({"length": 2000, "width": 2000, "height": 2000, "corner": [0, 0, 0], "body_name": "Frame"}));
    run(&mut s, "component.activate", json!({"component": "root"}));
    run(&mut s, "solid.bounding_solid", json!({"component": "Big", "body_name": "Envelope"}));
    // 2 x 4 x 4 = 32 mm³ inside the envelope, under the 41.6 mm³ a 2 m box's span allows.
    run(&mut s, "solid.box", json!({"length": 4, "width": 4, "height": 4, "corner": [1998, 0, 0], "operation": "new", "body_name": "Small"}));
    let env = run(&mut s, "inspect.interference", json!({"envelopes": true}));
    assert_eq!(env["count"], 1, "{env}");
    assert!((f(&env["interferences"][0]["volume_mm3"]) - 32.0).abs() < 1e-6, "{env}");
}

/// Two large envelopes sharing a sliver too small to count at their size are not passed as clear:
/// the parts inside can be far smaller than the envelopes, so the pair is listed as not checked.
#[test]
fn a_sliver_shared_by_two_large_envelopes_is_listed_not_cleared() {
    let mut s = Session::default();
    for (name, corner) in [("A", [0.0, 0.0, 0.0]), ("B", [1999.5, 1999.5, 1999.5])] {
        run(&mut s, "component.create", json!({"name": name}));
        run(
            &mut s,
            "solid.box",
            json!({"length": 2000, "width": 2000, "height": 2000, "corner": corner, "operation": "new", "body_name": format!("{name} Frame")}),
        );
        run(&mut s, "component.activate", json!({"component": "root"}));
        run(&mut s, "solid.bounding_solid", json!({"component": name, "body_name": format!("{name} Envelope")}));
    }
    // 0.5 mm cubed: 0.125 mm³, under the about 41.6 mm³ a 2 m box's span allows (1e-9 times
    // the cube of its 3464 mm diagonal).
    let env = run(&mut s, "inspect.interference", json!({"envelopes": true}));
    assert_eq!(env["count"], 0, "{env}");
    let unchecked = env["unchecked"].as_array().unwrap();
    assert!(
        unchecked.iter().any(|u| {
            let pair = [u["a"].as_str().unwrap_or(""), u["b"].as_str().unwrap_or("")];
            pair.contains(&"A Envelope") && pair.contains(&"B Envelope")
        }),
        "{env}"
    );
}

/// A shared part bigger than the smaller body, or reaching outside where the two bodies' boxes
/// overlap, can't be theirs (#21553): a plate of 200 x 100 x 30 mm and a pin of 8 x 20 mm seated
/// in it, whose intersection came back as the whole plate. A real overlap inside the pin passes.
#[test]
fn a_shared_part_bigger_than_the_smaller_body_is_not_an_overlap() {
    use solidcore_record::Vec3;
    use solidcraft_geom::Aabb3;
    let bounds = |min: [f64; 3], max: [f64; 3]| Aabb3 { min: Vec3::new(min[0], min[1], min[2]), max: Vec3::new(max[0], max[1], max[2]) };
    let plate_box = bounds([0.0, 0.0, 0.0], [200.0, 100.0, 30.0]);
    let pin_box = bounds([96.0, 46.0, 10.0], [104.0, 54.0, 30.0]);
    let (plate, pin) = ((599_000.0, &plate_box), (1005.3, &pin_box));
    let whole = super::overlap_out_of_bounds(599_000.0, &plate_box, plate, pin).unwrap_or_default();
    assert!(whole.contains("more than the smaller body"), "{whole}");
    let outside = super::overlap_out_of_bounds(500.0, &bounds([90.0, 46.0, 10.0], [104.0, 54.0, 30.0]), plate, pin).unwrap_or_default();
    assert!(outside.contains("reaches outside"), "{outside}");
    assert_eq!(super::overlap_out_of_bounds(200.0, &bounds([96.0, 46.0, 25.0], [104.0, 54.0, 30.0]), plate, pin), None);
    // The pin entirely inside the plate shares all of itself: that is the smaller body, not more.
    assert_eq!(super::overlap_out_of_bounds(1005.3, &pin_box, plate, pin), None);
}
