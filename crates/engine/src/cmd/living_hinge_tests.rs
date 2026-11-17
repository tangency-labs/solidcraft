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
