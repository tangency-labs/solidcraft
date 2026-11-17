use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn volume(s: &Session) -> f64 {
    let st = s.model.state();
    st.bodies.iter().map(|b| solidcraft_kernel::measure(&b.body).unwrap().volume).sum()
}

/// A 120 x 80 x 3 plate, `Wall`, with a sketch on its top face of the band x 40 to 80.
fn wall() -> Session {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 120, "width": 80, "height": 3, "body_name": "Wall"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 3, "name": "HingeArea"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [40, 0], "p1": [80, 80]}));
    run(&mut s, "sketch.finish", json!({}));
    s
}

fn hinge(s: &mut Session, extra: Value) -> Result<Value, crate::EngineError> {
    let mut p = json!({"sketch": "HingeArea", "slot_length": 20, "bridge": 3, "row_spacing": 1.5, "angle": "90 deg", "name": "CornerHinge"});
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        p[k] = v;
    }
    s.execute("solid.living_hinge", &p)
}

#[test]
fn a_living_hinge_cuts_slots_out_of_the_plate() {
    let mut s = wall();
    let before = volume(&s);
    let reply = hinge(&mut s, json!({"targets": ["Wall"]})).unwrap();
    assert_eq!(reply["name"], "CornerHinge", "{reply}");
    let after = volume(&s);
    assert!(after < before - 1.0, "{before} -> {after}");
    let id = reply["feature"].as_u64().unwrap();
    let st = s.model.state();
    let report = st.living_hinge_report(id).expect("a report");
    assert!(report.slots > 0 && report.rows > 0, "{report:?}");
    // The volume removed is the slots' open area through the 3 mm plate.
    assert!((before - after - 3.0 * report.open_area).abs() < 1e-3, "{before} {after} {report:?}");
    // The reply carries the same numbers.
    assert_eq!(reply["slots"].as_u64(), Some(report.slots as u64), "{reply}");
    assert_eq!(reply["open_area_mm2"].as_f64(), Some(report.open_area), "{reply}");
}

#[test]
fn a_living_hinge_refuses_what_cannot_be_cut() {
    let mut s = wall();
    for (extra, words) in [
        (json!({"slot_length": 0.1}), "slot_length"),
        (json!({"row_spacing": 0.2}), "row_spacing"),
        (json!({"bridge": 0}), "bridge"),
        (json!({"slot_width": 0}), "slot_width"),
        (json!({"margin": -1}), "margin"),
        (json!({"clip": "around"}), "clip"),
    ] {
        let e = hinge(&mut s, extra.clone()).expect_err(&extra.to_string()).to_string();
        assert!(e.contains(words), "{extra}: {e}");
    }
}

#[test]
fn a_living_hinge_follows_its_parameters() {
    let mut s = wall();
    run(&mut s, "parameters.add", json!({"name": "slot", "expression": "20 mm"}));
    hinge(&mut s, json!({"slot_length": "slot"})).unwrap();
    let first = volume(&s);
    run(&mut s, "parameters.change", json!({"name": "slot", "expression": "10 mm"}));
    assert!((volume(&s) - first).abs() > 1e-3, "the volume did not change");
}

#[test]
fn a_living_hinge_finds_its_plate_and_stores_it() {
    let mut s = wall();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [200, 0, 0], "operation": "new_body", "body_name": "Far"}));
    let reply = hinge(&mut s, json!({})).unwrap();
    assert_eq!(reply["changed_bodies"], json!(["Wall"]), "{reply}");
    let id = reply["feature"].as_u64().unwrap();
    let f = s.doc.features.iter().find(|f| f.id == id).unwrap();
    let solidcraft_doc::FeatureKind::LivingHinge { targets, .. } = &f.kind else { panic!("{f:?}") };
    assert_eq!(targets.iter().map(|t| t.0.as_str()).collect::<Vec<_>>(), ["Wall"]);
}

#[test]
fn a_living_hinge_refuses_no_plate_or_several() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 120, "width": 80, "height": 3, "body_name": "Wall"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 10, "name": "HingeArea"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [40, 0], "p1": [80, 80]}));
    run(&mut s, "sketch.finish", json!({}));
    let e = hinge(&mut s, json!({})).unwrap_err().to_string();
    assert!(e.contains("no plate has a flat face in the sketch's plane"), "{e}");

    let mut s = wall();
    run(&mut s, "solid.box", json!({"length": 120, "width": 80, "height": 3, "corner": [0, 80, 0], "operation": "new_body", "body_name": "Other"}));
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 3, "name": "Both"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [40, 10], "p1": [80, 150]}));
    run(&mut s, "sketch.finish", json!({}));
    let e = hinge(&mut s, json!({"sketch": "Both"})).unwrap_err().to_string();
    assert!(e.contains("the area lies over") && e.contains("Wall") && e.contains("Other"), "{e}");
}

#[test]
fn a_living_hinge_warns_when_its_slots_are_no_longer_than_its_bridges() {
    let mut s = wall();
    let reply = hinge(&mut s, json!({"bridge": 20})).unwrap();
    assert!(reply["warning"].as_str().is_some_and(|w| w.contains("no longer than the bridges")), "{reply}");
    let mut s = wall();
    let reply = hinge(&mut s, json!({})).unwrap();
    assert!(!reply["warning"].as_str().is_some_and(|w| w.contains("no longer than the bridges")), "{reply}");
}

/// The agent run of design 3.6, exactly.
#[test]
fn the_agent_example_cuts_what_the_design_says() {
    let mut s = wall();
    let reply = hinge(&mut s, json!({})).unwrap();
    assert_eq!(reply["bodies"], json!(["Wall"]), "{reply}");
    for (k, v) in [("slots", 95), ("rows", 27), ("whole", 67), ("shortened", 0), ("run_out", 28), ("left_out", 26)] {
        assert_eq!(reply[k].as_u64(), Some(v), "{k}: {reply}");
    }
    assert!((reply["band_width_mm"].as_f64().unwrap() - 40.0).abs() < 1e-9, "{reply}");
    assert!((reply["open_area_mm2"].as_f64().unwrap() - 354.104690).abs() < 1e-5, "{reply}");
    assert!((volume(&s) - 27737.685930).abs() < 1e-5, "{}", volume(&s));
}
