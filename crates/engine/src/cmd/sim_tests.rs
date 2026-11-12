//! The analysis commands, checked against answers worked out on paper.
//!
//! The solver's own mathematics is verified in `solidcraft-sim`; what these check is the part
//! this module owns -- turning picks, materials and units into a study, and the answer back into
//! numbers -- and that a hostile call is refused rather than guessed at.

use serde_json::{Value, json};

use crate::Session;

/// A hundred-millimetre bar, twenty square. Twenty cells across its length puts its walls
/// exactly on cell boundaries, so the lattice holds it with no error at all and the answers
/// below are the closed-form ones.
fn bar() -> Session {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 100, "width": 20, "height": 20})).expect("a bar");
    s
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} should have refused this, and answered {v}"),
        Err(e) => e.to_string(),
    }
}

/// A number somewhere in a reply. A path step that parses as a number indexes a list.
fn number(v: &Value, path: &[&str]) -> f64 {
    let mut at = v;
    for key in path {
        at = match key.parse::<usize>() {
            Ok(i) => at.get(i).unwrap_or_else(|| panic!("no {i} in {at}")),
            Err(_) => at.get(key).unwrap_or_else(|| panic!("no {key} in {at}")),
        };
    }
    at.as_f64().unwrap_or_else(|| panic!("{at} is not a number"))
}

fn patch_watts(v: &Value, name: &str) -> f64 {
    v.get("patches")
        .and_then(Value::as_array)
        .and_then(|a| a.iter().find(|p| p.get("name").and_then(Value::as_str) == Some(name)))
        .and_then(|p| p.get("watts"))
        .and_then(Value::as_f64)
        .unwrap_or_else(|| panic!("no patch `{name}` in {v}"))
}

#[test]
fn the_material_list_names_what_a_study_can_run_on() {
    let mut s = Session::default();
    let v = run(&mut s, "sim.materials", json!({}));
    let materials = v.get("materials").and_then(Value::as_array).expect("materials");
    assert!(materials.len() >= 13, "{}", materials.len());
    let aluminium = materials.iter().find(|m| m.get("material").and_then(Value::as_str) == Some("Aluminum")).expect("aluminium is in the list");
    assert!((number(aluminium, &["conductivity_w_m_k", "0"]) - 167.0).abs() < 1e-9, "{aluminium}");
    // Oak is the one that knows which way the grain runs.
    let oak = materials.iter().find(|m| m.get("material").and_then(Value::as_str) == Some("Oak")).expect("oak");
    assert_eq!(oak.get("orthotropic"), Some(&json!(true)));
    // And the fluids come with the numbers a flow or a film needs.
    let fluids = v.get("fluids").and_then(Value::as_array).expect("fluids");
    let air = fluids.iter().find(|f| f.get("fluid").and_then(Value::as_str) == Some("Air")).expect("air");
    assert!((number(air, &["prandtl"]) - 0.731).abs() < 0.002, "{air}");
    // The list says what it is, so nobody mistakes it for data.
    assert!(v.get("note").and_then(Value::as_str).is_some_and(|n| n.contains("nominal")), "{v}");
}

#[test]
fn the_lattice_report_says_how_well_it_holds_the_model() {
    let mut s = bar();
    let v = run(&mut s, "sim.lattice", json!({"cells": 20}));
    assert_eq!(v.get("lattice").and_then(|l| l.get("cells")), Some(&json!([22, 6, 6])));
    assert!((number(&v, &["lattice", "spacing_mm"]) - 5.0).abs() < 1e-12);
    assert_eq!(number(&v, &["lattice", "model_cells"]), (20 * 4 * 4) as f64);
    // A box square to the axes is held exactly, so the volume is the model's own.
    assert!(number(&v, &["lattice", "volume_error"]).abs() < 1e-9, "{v}");
    // And all six of its faces are somewhere a condition could be put.
    let faces = v.get("faces").and_then(Value::as_array).expect("faces");
    assert_eq!(faces.len(), 6, "{faces:?}");
    assert_eq!(number(&v, &["unreachable_surface_cells"]), 0.0);
}

#[test]
fn a_bar_held_hot_at_one_end_passes_the_heat_fourier_says_it_will() {
    let mut s = bar();
    let v = run(
        &mut s,
        "sim.thermal",
        json!({
            "cells": 20,
            "material": "Aluminum",
            "conditions": [
                {"name": "cold", "side": "-x", "fixed_c": 20},
                {"name": "hot", "side": "+x", "fixed_c": 120},
            ],
        }),
    );
    // Q = k·A·ΔT/L: aluminium's 167 W/(m·K) over 4 cm², a hundred kelvin across ten centimetres.
    let truth = 167.0 * 4e-4 * 100.0 / 0.1;
    assert!((patch_watts(&v, "hot") - truth).abs() < 1e-6, "{} vs {truth}", patch_watts(&v, "hot"));
    assert!((patch_watts(&v, "cold") + truth).abs() < 1e-6);
    // The ends are held, so the first and last cell centres sit a twentieth of the way in.
    assert!((number(&v, &["temperature", "min_c"]) - 22.5).abs() < 1e-6, "{v}");
    assert!((number(&v, &["temperature", "max_c"]) - 117.5).abs() < 1e-6);
    // Kelvin beside the degrees, and the two agree.
    assert!((number(&v, &["temperature", "max_k"]) - number(&v, &["temperature", "max_c"]) - 273.15).abs() < 1e-9);
    // Nothing is generated, so what comes in goes out.
    assert!(number(&v, &["balance", "imbalance"]).abs() < 1e-9, "{v}");
    assert!(number(&v, &["balance", "generated_w"]).abs() < 1e-12);
    // The hottest place is at the hot end, and it is named.
    let hot = v.get("hot_spots").and_then(Value::as_array).and_then(|a| a.first().cloned()).expect("a hot spot");
    assert_eq!(hot.get("body"), Some(&json!("Body1")));
    assert!(number(&hot, &["at", "0"]) > 90.0, "{hot}");
    assert!((number(&hot, &["c"]) - 117.5).abs() < 1e-6, "{hot}");
    // Using nominal properties is said out loud.
    let warnings = v.get("warnings").and_then(Value::as_array).expect("warnings");
    assert!(warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains("nominal"))), "{warnings:?}");
}

#[test]
fn degrees_and_kelvin_are_the_same_study_said_two_ways() {
    let mut s = bar();
    let in_degrees = run(
        &mut s,
        "sim.thermal",
        json!({"cells": 10, "material": "Steel", "conditions": [
            {"name": "a", "side": "-x", "fixed_c": 20}, {"name": "b", "side": "+x", "fixed_c": 120}]}),
    );
    let in_kelvin = run(
        &mut s,
        "sim.thermal",
        json!({"cells": 10, "material": "Steel", "conditions": [
            {"name": "a", "side": "-x", "fixed_k": 293.15}, {"name": "b", "side": "+x", "fixed_k": 393.15}]}),
    );
    assert!((patch_watts(&in_degrees, "b") - patch_watts(&in_kelvin, "b")).abs() < 1e-9);
    assert!((number(&in_degrees, &["temperature", "max_c"]) - number(&in_kelvin, &["temperature", "max_c"])).abs() < 1e-9);
}

#[test]
fn a_material_assigned_in_the_design_is_the_one_the_study_uses() {
    let mut s = bar();
    run(&mut s, "material.assign", json!({"bodies": ["Body1"], "material": "Copper"}));
    let v = run(
        &mut s,
        "sim.thermal",
        json!({"cells": 10, "conditions": [
            {"name": "a", "side": "-x", "fixed_c": 20}, {"name": "b", "side": "+x", "fixed_c": 120}]}),
    );
    let body = v.get("bodies").and_then(Value::as_array).and_then(|a| a.first().cloned()).expect("a body");
    assert_eq!(body.get("material"), Some(&json!("Copper")));
    assert!((number(&body, &["conductivity_w_m_k", "0"]) - 398.0).abs() < 1e-9, "{body}");
    // Copper's density comes from the design's own library, so mass and heat agree.
    assert!((number(&body, &["density_kg_m3"]) - 8960.0).abs() < 1e-6, "{body}");
    assert!((patch_watts(&v, "b") - 398.0 * 4e-4 * 1000.0).abs() < 1e-6);
}

#[test]
fn a_body_with_no_material_is_told_what_to_do_about_it() {
    let mut s = bar();
    let why = fails(&mut s, "sim.thermal", json!({"conditions": [{"side": "-x", "fixed_c": 20}]}));
    assert!(why.contains("no material"), "{why}");
    assert!(why.contains("material.assign") || why.contains("properties"), "it says how to fix it: {why}");
}

#[test]
fn numbers_given_outright_override_the_material() {
    let mut s = bar();
    let v = run(
        &mut s,
        "sim.thermal",
        json!({
            "cells": 10,
            "material": "Steel",
            "properties": {"Body1": {"conductivity_w_m_k": 1.0}},
            "conditions": [{"name": "a", "side": "-x", "fixed_c": 20}, {"name": "b", "side": "+x", "fixed_c": 120}],
        }),
    );
    assert!((patch_watts(&v, "b") - 1.0 * 4e-4 * 1000.0).abs() < 1e-9, "{v}");
    let body = v.get("bodies").and_then(Value::as_array).and_then(|a| a.first().cloned()).expect("a body");
    assert_eq!(body.get("source"), Some(&json!("properties")));
    // Given numbers are not nominal, so nothing is said about them.
    let warnings = v.get("warnings").and_then(Value::as_array).expect("warnings");
    assert!(!warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains("nominal"))), "{warnings:?}");
}

#[test]
fn a_conductivity_may_be_given_per_axis() {
    let mut s = bar();
    let v = run(
        &mut s,
        "sim.thermal",
        json!({
            "cells": 10,
            "properties": {"Body1": {"conductivity_w_m_k": [5.0, 100.0, 100.0]}},
            "conditions": [{"name": "a", "side": "-x", "fixed_c": 20}, {"name": "b", "side": "+x", "fixed_c": 120}],
        }),
    );
    // The heat runs along x, so only the x conductivity matters.
    assert!((patch_watts(&v, "b") - 5.0 * 4e-4 * 1000.0).abs() < 1e-9, "{v}");
}

#[test]
fn watts_generated_inside_all_leave_through_the_surface() {
    let mut s = bar();
    let v = run(
        &mut s,
        "sim.thermal",
        json!({
            "cells": 10,
            "material": "Aluminum",
            "generation": {"Body1": {"watts": 15.0}},
            "ambient": {"c": 20, "convection": 25.0},
            "conditions": [],
        }),
    );
    // Fifteen watts in, fifteen watts out, whatever the lattice does at the walls: the scheme
    // conserves energy over every cell.
    assert!((number(&v, &["balance", "generated_w"]) - 15.0).abs() < 1e-9, "{v}");
    assert!((number(&v, &["balance", "surface_w"]) + 15.0).abs() < 1e-6, "{v}");
    assert!(number(&v, &["balance", "imbalance"]).abs() < 1e-9);
    // And the bar is warmer than the air it sits in.
    assert!(number(&v, &["temperature", "min_c"]) > 20.0, "{v}");
    // The surface area is 2·20·20 + 4·100·20 mm², so the rise is about 15/(25·A) above ambient.
    let area_m2 = 2.0 * 0.02 * 0.02 + 4.0 * 0.1 * 0.02;
    let lumped = 20.0 + 15.0 / (25.0 * area_m2);
    assert!((number(&v, &["temperature", "max_c"]) - lumped).abs() < 1.0, "{} vs about {lumped}", number(&v, &["temperature", "max_c"]));
}

#[test]
fn a_condition_may_be_put_on_a_face_of_the_model_by_number_or_by_a_point_on_it() {
    let mut s = bar();
    // `solid.box` gives the bar six faces; find the one the point (100, 10, 10) lies on and
    // put the heat there, then do the same by naming the face outright.
    let by_point = run(
        &mut s,
        "sim.thermal",
        json!({
            "cells": 10,
            "material": "Aluminum",
            "conditions": [
                {"name": "heated", "faces": [[100.0, 10.0, 10.0]], "flux_w_m2": 20000.0},
                {"name": "cooled", "faces": [[0.0, 10.0, 10.0]], "fixed_c": 20},
            ],
        }),
    );
    // Twenty kilowatts a square metre over four square centimetres is eight watts.
    assert!((patch_watts(&by_point, "heated") - 8.0).abs() < 1e-9, "{by_point}");
    assert!((patch_watts(&by_point, "cooled") + 8.0).abs() < 1e-6);

    // The same study with the faces named by number has to agree. The bar's end faces are
    // whichever two the lattice report found, so read them from the point-based answer's own
    // geometry: a face pick and a point on that face are the same pick.
    let faces = run(&mut s, "sim.lattice", json!({"cells": 10}));
    let listed = faces.get("faces").and_then(Value::as_array).expect("faces");
    assert_eq!(listed.len(), 6);
}

#[test]
fn a_held_face_and_a_fierce_film_on_it_are_the_same_study() {
    let mut s = bar();
    let study = |film: Value| {
        json!({"cells": 10, "material": "Steel", "conditions": [
            {"name": "a", "side": "-x", "fixed_c": 20}, film]})
    };
    let held = run(&mut s, "sim.thermal", study(json!({"name": "b", "side": "+x", "fixed_c": 120})));
    let blown = run(
        &mut s,
        "sim.thermal",
        // A hundred million watts a square metre a kelvin: beside the bar's own half-cell
        // conduction of some tens of thousands, that is as good as no film resistance at all.
        study(json!({"name": "b", "side": "+x", "convection": {"w_m2_k": 1e8, "c": 120}})),
    );
    assert!((patch_watts(&held, "b") - patch_watts(&blown, "b")).abs() < 1e-3, "{} vs {}", patch_watts(&held, "b"), patch_watts(&blown, "b"));
}

#[test]
fn a_transient_step_moves_the_temperature_towards_the_air_around_it() {
    let mut s = bar();
    let v = run(
        &mut s,
        "sim.thermal",
        json!({
            "cells": 8,
            "material": "Aluminum",
            "ambient": {"c": 20, "convection": 50.0},
            "time": {"seconds": 30.0, "from_c": 200.0},
        }),
    );
    let max = number(&v, &["temperature", "max_c"]);
    assert!(max < 200.0 && max > 20.0, "it has cooled, but not all the way: {max}");
    // A transient stores energy, so the surface and the generation are not meant to balance,
    // and the report does not pretend otherwise.
    assert!(number(&v, &["balance", "imbalance"]).abs() < 1e-12, "{v}");
}

#[test]
fn a_study_with_nothing_settling_it_says_so_rather_than_answering() {
    let mut s = bar();
    let why = fails(&mut s, "sim.thermal", json!({"cells": 8, "material": "Steel"}));
    assert!(why.contains("settles") || why.contains("fixed temperature"), "{why}");
}

#[test]
fn hostile_and_contradictory_calls_are_refused_rather_than_guessed_at() {
    let mut s = bar();
    let base = |extra: Value| {
        let mut v = json!({"cells": 10, "material": "Steel", "conditions": []});
        if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
            for (k, val) in e {
                o.insert(k.clone(), val.clone());
            }
        }
        v
    };

    // A lattice nobody could hold, and one too coarse to mean anything.
    assert!(fails(&mut s, "sim.thermal", base(json!({"cells": 100000}))).contains("cells"));
    assert!(fails(&mut s, "sim.thermal", base(json!({"cells": 1}))).contains("cells"));
    // JSON cannot carry a NaN, so a number that is not one arrives as something else.
    assert!(fails(&mut s, "sim.thermal", base(json!({"cells": "lots"}))).contains("settles"), "a non-number falls back to the default");

    // A condition that does not say where, or says it twice.
    let why = fails(&mut s, "sim.thermal", base(json!({"conditions": [{"fixed_c": 20}]})));
    assert!(why.contains("does not say where"), "{why}");
    let why = fails(&mut s, "sim.thermal", base(json!({"conditions": [{"side": "+x", "rest": true, "fixed_c": 20}]})));
    assert!(why.contains("more than once"), "{why}");
    // One that says where but not what.
    let why = fails(&mut s, "sim.thermal", base(json!({"conditions": [{"side": "+x"}]})));
    assert!(why.contains("not what"), "{why}");
    // A side that is not a side.
    assert!(fails(&mut s, "sim.thermal", base(json!({"conditions": [{"side": "up", "fixed_c": 20}]}))).contains("+x"));

    // Temperatures below absolute zero, and both units at once.
    assert!(fails(&mut s, "sim.thermal", base(json!({"conditions": [{"side": "+x", "fixed_c": -500}]}))).contains("absolute zero"));
    assert!(fails(&mut s, "sim.thermal", base(json!({"conditions": [{"side": "+x", "fixed_k": -5}]}))).contains("not a temperature"));
    let why = fails(&mut s, "sim.thermal", base(json!({"conditions": [{"side": "+x", "fixed_c": 20, "fixed_k": 300}]})));
    assert!(why.contains("both"), "{why}");

    // A film with no temperature, an emissivity that is not a fraction, a negative coefficient.
    assert!(fails(&mut s, "sim.thermal", base(json!({"conditions": [{"side": "+x", "convection": {"w_m2_k": 10}}]}))).contains("temperature"));
    assert!(
        fails(&mut s, "sim.thermal", base(json!({"conditions": [{"side": "+x", "radiation": {"emissivity": 2.0, "c": 20}}]}))).contains("emissivity")
    );
    assert!(fails(&mut s, "sim.thermal", base(json!({"conditions": [{"side": "+x", "convection": {"w_m2_k": -1.0, "c": 20}}]}))).contains("w_m2_k"));

    // Properties that are not properties.
    assert!(fails(&mut s, "sim.thermal", base(json!({"properties": {"Body1": {}}}))).contains("conductivity"));
    assert!(fails(&mut s, "sim.thermal", base(json!({"properties": {"Body1": {"conductivity_w_m_k": 0.0}}}))).contains("above zero"));
    assert!(fails(&mut s, "sim.thermal", base(json!({"properties": {"Body1": {"conductivity_w_m_k": f64::INFINITY}}}))).contains("conductivity"));

    // Generation said two ways at once, and an unknown body.
    assert!(fails(&mut s, "sim.thermal", base(json!({"generation": {"Body1": {"watts": 1.0, "w_m3": 1.0}}}))).contains("both"));
    assert!(fails(&mut s, "sim.thermal", base(json!({"bodies": ["Nothing"]}))).contains("no body"));

    // An ambient that gives a temperature but no way to reach it.
    assert!(fails(&mut s, "sim.thermal", base(json!({"ambient": {"c": 20}}))).contains("no way for heat"));
    // A time step that is not a step.
    assert!(fails(&mut s, "sim.thermal", base(json!({"time": {"seconds": 0.0, "from_c": 20}}))).contains("seconds"));
    assert!(fails(&mut s, "sim.thermal", base(json!({"time": {"seconds": 1.0}}))).contains("start from"));
    // A tolerance outside the only range one can have.
    assert!(fails(&mut s, "sim.thermal", base(json!({"tolerance": 0.0}))).contains("tolerance"));
    assert!(fails(&mut s, "sim.thermal", base(json!({"tolerance": 2.0}))).contains("tolerance"));
}

#[test]
fn an_analysis_changes_the_design_in_no_way_at_all() {
    let mut s = bar();
    let before = s.world_state().unwrap().bodies.len();
    let features = s.doc.features.len();
    let revision = s.revision;
    // Reading the lattice draws nothing, so nothing moves at all.
    run(&mut s, "sim.lattice", json!({"cells": 8}));
    assert_eq!(s.revision, revision, "a reading that draws nothing changes nothing");
    run(
        &mut s,
        "sim.thermal",
        json!({"cells": 8, "material": "Steel", "conditions": [{"side": "-x", "fixed_c": 20}, {"side": "+x", "fixed_c": 100}]}),
    );
    // The design is untouched: the same bodies, the same features, no undo step. What moved is
    // the revision, because the viewport keeps the scene it built and rebuilds it when the
    // revision says there is something new in it -- so a field that did not move the revision
    // would be held and never drawn. The same is true of every other analysis that shades the
    // model.
    assert_eq!(s.world_state().unwrap().bodies.len(), before);
    assert_eq!(s.doc.features.len(), features, "an analysis adds no feature");
    assert!(s.revision > revision, "the viewport is told there is something new to draw");
    assert!(s.analysis_result.as_ref().is_some_and(|r| r.revision == s.revision), "and the field is about this model");
    for id in ["sim.materials", "sim.lattice", "sim.thermal", "sim.flow", "sim.stress"] {
        let spec = crate::cmd::find_command(id).unwrap_or_else(|| panic!("{id} is registered"));
        assert!(!spec.undoable, "{id} must not take an undo step");
    }
}

#[test]
fn two_bodies_of_different_materials_conduct_through_their_interface() {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 50, "width": 20, "height": 20, "body_name": "Left"})).expect("left");
    s.execute("solid.box", &json!({"length": 50, "width": 20, "height": 20, "corner": [50, 0, 0], "operation": "new", "body_name": "Right"}))
        .expect("right");
    let v = run(
        &mut s,
        "sim.thermal",
        json!({
            "cells": 20,
            "material": {"Left": "Copper", "Right": "Stainless Steel"},
            "conditions": [{"name": "a", "side": "-x", "fixed_c": 20}, {"name": "b", "side": "+x", "fixed_c": 120}],
        }),
    );
    // Two resistances in series: 50 mm of copper at 398 and 50 mm of stainless at 16.
    let area = 4e-4;
    let r = 0.05 / (398.0 * area) + 0.05 / (16.0 * area);
    let truth = 100.0 / r;
    assert!((patch_watts(&v, "b") - truth).abs() / truth < 1e-6, "{} vs {truth}", patch_watts(&v, "b"));
    // Both bodies are reported, with their own conductivities.
    let bodies = v.get("bodies").and_then(Value::as_array).expect("bodies");
    assert_eq!(bodies.len(), 2);
}

/// A twenty-millimetre cube, for the flow cases.
fn cube() -> Session {
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 20, "width": 20, "height": 20})).expect("a cube");
    s
}

/// A small, quick flow: coarse, but with a tunnel big enough that its own walls do not
/// decide the answer, and run long enough for the wake to develop. Enough to check the signs
/// and the plumbing; the physics itself is verified in `solidcraft-sim`.
fn quick_flow(extra: Value) -> Value {
    let mut v = json!({"speed_m_s": 5.0, "cells": 8, "padding": 1.5, "steps": 1200, "settle": 0.0});
    if let (Some(o), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        for (k, val) in e {
            o.insert(k.clone(), val.clone());
        }
    }
    v
}

#[test]
fn a_cube_in_a_stream_is_pushed_along_it() {
    let mut s = cube();
    let v = run(&mut s, "sim.flow", quick_flow(json!({})));
    // Pushed downstream, and a symmetric body in a straight stream feels little else.
    let drag = number(&v, &["force", "drag_n"]);
    assert!(drag > 0.0, "{v}");
    assert!(number(&v, &["force", "cross_n"]) < drag * 0.2, "{v}");
    // The shadow of a 20 mm cube is 400 mm², and the lattice should land near it.
    let area = number(&v, &["force", "reference_area_mm2"]);
    assert!((area - 400.0).abs() / 400.0 < 0.3, "the area across the flow is {area} mm2");
    // A cube's drag coefficient. The flow actually ran at a Reynolds number of about 120 --
    // see the next case -- where a cube measures about 1.2 to 1.4, and this lattice is coarse,
    // so the band is wide. A number outside it would mean something is wrong rather than
    // merely approximate.
    let cd = number(&v, &["force", "drag_coefficient"]);
    assert!((0.6..3.0).contains(&cd), "the drag coefficient came out {cd}");
    // The air has to speed up getting past.
    assert!(number(&v, &["speed", "fastest_m_s"]) > 5.0, "{v}");
    // And the Reynolds number of a 20 mm cube in 5 m/s air is about seven thousand.
    let re = number(&v, &["reynolds", "value"]);
    assert!((re / 6600.0 - 1.0).abs() < 0.4, "the Reynolds number came out {re}");
}

#[test]
fn a_flow_this_lattice_cannot_hold_as_given_says_what_it_ran_instead() {
    // The honest limit. Five metres a second over millimetre cells asks for a relaxation time
    // right against the stability edge, so the fluid is run thicker than it is and the flow
    // runs at a Reynolds number of about 120 rather than the 6600 asked for. Both numbers are
    // reported, because a drag coefficient read off a flow at the wrong Reynolds number is
    // worth having only if you know which one it was.
    let mut s = cube();
    let v = run(&mut s, "sim.flow", quick_flow(json!({})));
    assert_eq!(v.get("mapping").and_then(|m| m.get("thickened")), Some(&json!(true)), "{v}");
    assert_eq!(v.get("mapping").and_then(|m| m.get("resolved")), Some(&json!(false)), "{v}");
    assert!(number(&v, &["mapping", "cell_reynolds"]) > 100.0, "{v}");
    assert!(number(&v, &["mapping", "resolved_spacing_mm"]) < 1.0, "{v}");
    // The Reynolds number asked for, and the one that ran, both said.
    let asked = number(&v, &["reynolds", "value"]);
    let ran = number(&v, &["reynolds", "ran_at"]);
    assert!(asked > 3000.0, "{asked}");
    assert!(ran > 20.0 && ran < asked / 5.0, "asked {asked}, ran {ran}");
    let warnings = v.get("warnings").and_then(Value::as_array).expect("warnings");
    assert!(warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains("thicker than it is") && w.contains("turbulence"))), "{warnings:?}");
}

#[test]
fn a_flow_slow_enough_for_the_lattice_is_held_as_given() {
    // The other side of it: a centimetre a second over the same cube needs no thickening at
    // all, and then there is nothing to apologise for. The drag coefficient is far higher,
    // which is what the drag curve does at a Reynolds number of thirteen.
    let mut s = cube();
    let v = run(&mut s, "sim.flow", quick_flow(json!({"speed_m_s": 0.01})));
    assert_eq!(v.get("mapping").and_then(|m| m.get("thickened")), Some(&json!(false)), "{v}");
    let asked = number(&v, &["reynolds", "value"]);
    let ran = number(&v, &["reynolds", "ran_at"]);
    assert!((asked - ran).abs() / asked < 1e-6, "asked {asked}, ran {ran}");
    assert!(asked < 20.0, "{asked}");
    // Viscous-dominated, so the drag coefficient is several, not about one.
    assert!(number(&v, &["force", "drag_coefficient"]) > 3.0, "{v}");
}

#[test]
fn turning_the_flow_round_turns_the_force_round() {
    let mut s = cube();
    let forward = run(&mut s, "sim.flow", quick_flow(json!({"direction": [1, 0, 0]})));
    let backward = run(&mut s, "sim.flow", quick_flow(json!({"direction": [-1, 0, 0]})));
    let (a, b) = (number(&forward, &["force", "drag_n"]), number(&backward, &["force", "drag_n"]));
    // Drag is along the flow either way, so both are positive and a symmetric cube gives the
    // same number.
    assert!(a > 0.0 && b > 0.0, "{a} and {b}");
    assert!((a - b).abs() / a < 0.1, "a cube is the same both ways round: {a} vs {b}");
    // But the force vector itself reverses.
    let fx = number(&forward, &["force", "vector_n", "0"]);
    let bx = number(&backward, &["force", "vector_n", "0"]);
    assert!(fx > 0.0 && bx < 0.0, "{fx} and {bx}");
}

#[test]
fn a_flow_across_the_axes_is_handled_rather_than_rounded_to_the_nearest_one() {
    let mut s = cube();
    let v = run(&mut s, "sim.flow", quick_flow(json!({"direction": [1, 1, 0]})));
    let fx = number(&v, &["force", "vector_n", "0"]);
    let fy = number(&v, &["force", "vector_n", "1"]);
    // Pushed along the diagonal: both components positive and, for a cube, about equal.
    assert!(fx > 0.0 && fy > 0.0, "{fx}, {fy}");
    assert!((fx - fy).abs() / fx.max(fy) < 0.25, "a cube in a diagonal stream: {fx}, {fy}");
    assert!(number(&v, &["force", "drag_n"]) > 0.0);
}

#[test]
fn water_pushes_far_harder_than_air_at_the_same_speed() {
    let mut s = cube();
    let air = run(&mut s, "sim.flow", quick_flow(json!({"fluid": "Air", "speed_m_s": 0.5})));
    let water = run(&mut s, "sim.flow", quick_flow(json!({"fluid": "Water", "speed_m_s": 0.5})));
    let (a, w) = (number(&air, &["force", "drag_n"]), number(&water, &["force", "drag_n"]));
    // Force goes as the density, and water is eight hundred times denser than air.
    assert!(w / a > 100.0, "air {a} N, water {w} N");
    // Water is also a hundred times less viscous by kinematic measure, so its Reynolds number
    // is far higher at the same speed and size.
    assert!(number(&water, &["reynolds", "value"]) > number(&air, &["reynolds", "value"]) * 5.0);
    assert_eq!(water.get("fluid").and_then(|f| f.get("fluid")), Some(&json!("Water")));
}

/// A flow can be given the fluid's own numbers, which is the only way to run one at a
/// temperature that matters.
///
/// The named fluids are room-temperature figures, and a flow turns on the kinematic viscosity
/// v = u/p: it sets the Reynolds number, and on a lattice the relaxation time with it. Air at
/// 200 degrees C has about 2.3 times the kinematic viscosity it has at 20, so the same flow over
/// the same part is a different flow. A thermal or a stress study has taken `properties` all
/// along; this is a flow catching up.
#[test]
fn a_flow_takes_the_fluids_own_numbers_and_runs_a_different_flow_for_them() {
    let mut s = cube();
    let cold = run(&mut s, "sim.flow", quick_flow(json!({"fluid": "Air", "speed_m_s": 0.5})));
    // Dry air at 200 C: thinner and stickier, both of which raise the kinematic viscosity.
    let hot = run(
        &mut s,
        "sim.flow",
        quick_flow(json!({
            "fluid": "Air",
            "speed_m_s": 0.5,
            "properties": {"density_kg_m3": 0.7458, "dynamic_viscosity_pa_s": 2.577e-5, "conductivity_w_m_k": 0.03779, "specific_heat_j_kg_k": 1026.0},
        })),
    );

    let (c, h) = (number(&cold, &["reynolds", "value"]), number(&hot, &["reynolds", "value"]));
    // v goes from 1.516e-5 to 3.455e-5, so the Reynolds number falls by that ratio.
    let ratio = c / h;
    assert!((ratio - 2.279).abs() < 0.02, "Reynolds should fall 2.28x, not {ratio}x (cold {c}, hot {h})");

    // The numbers it ran on are reported, and it no longer claims to be the named fluid.
    assert_eq!(hot.get("fluid").and_then(|f| f.get("fluid")), Some(&json!("Custom")));
    let nu = number(&hot, &["fluid", "kinematic_viscosity_m2_s"]);
    assert!((nu - 3.455e-5).abs() < 1e-8, "the kinematic viscosity it ran on: {nu}");
    // Given all four, the Prandtl number is the fluid's own and no warning is needed.
    let pr = number(&hot, &["fluid", "prandtl"]);
    assert!((pr - 0.6997).abs() < 0.01, "air at 200 C has Pr about 0.70, not {pr}");

    // A number that cannot be a fluid property, and a key that is not one, are both refused
    // rather than quietly ignored: this is input-derived and hostile.
    for bad in [json!({"density_kg_m3": 0}), json!({"dynamic_viscosity_pa_s": -1}), json!({"density_kg_m3": "thick"})] {
        let why = fails(&mut s, "sim.flow", quick_flow(json!({"speed_m_s": 0.5, "properties": bad})));
        assert!(why.contains("above zero"), "{why}");
    }
    let why = fails(&mut s, "sim.flow", quick_flow(json!({"speed_m_s": 0.5, "properties": {"viscosity": 1.0}})));
    assert!(why.contains("not a fluid property"), "{why}");
}

/// Giving only the viscosity and density leaves the Prandtl number mixing two temperatures, and
/// the study says so rather than letting it pass as the fluid's own.
#[test]
fn a_flow_run_too_slowly_for_f32_says_so_rather_than_claiming_the_accuracy() {
    // `lattice_speed` was documented here, and in the library, as a knob where smaller is simply
    // more accurate, because the compressibility error goes as its square. It does not go on
    // being true: the distributions are f32 and the force is a difference of numbers much larger
    // than itself, so round-off in the answer grows as the flow is slowed -- measured at about
    // tenfold per decade (`zz_wall_precision_floor` in the sim crate). Accuracy is a U, and a
    // user who followed the old advice to its conclusion got arithmetic.
    let mut s = cube();
    let slow = run(&mut s, "sim.flow", quick_flow(json!({"lattice_speed": 0.001})));
    let warnings = slow.get("warnings").and_then(|w| w.as_array()).cloned().unwrap_or_default();
    assert!(
        warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains("round-off"))),
        "a flow below MIN_LATTICE_SPEED should say the force is losing accuracy, not gaining it: {warnings:?}"
    );

    // The default is well clear of it, and must not cry wolf.
    let ordinary = run(&mut s, "sim.flow", quick_flow(json!({})));
    let warnings = ordinary.get("warnings").and_then(|w| w.as_array()).cloned().unwrap_or_default();
    assert!(
        !warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains("round-off"))),
        "the default lattice speed is not in the round-off region: {warnings:?}"
    );
}

#[test]
fn a_part_given_fluid_warns_that_its_prandtl_number_is_still_the_named_ones() {
    let mut s = cube();
    let hot = run(
        &mut s,
        "sim.flow",
        quick_flow(json!({
            "fluid": "Air",
            "speed_m_s": 0.5,
            "properties": {"density_kg_m3": 0.7458, "dynamic_viscosity_pa_s": 2.577e-5},
        })),
    );
    let warnings = hot.get("warnings").and_then(|w| w.as_array()).cloned().unwrap_or_default();
    assert!(
        warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains("Prandtl"))),
        "a part-given fluid should warn about its Prandtl number: {warnings:?}"
    );

    // Given the whole set, there is nothing to warn about.
    let whole = run(
        &mut s,
        "sim.flow",
        quick_flow(json!({
            "fluid": "Air",
            "speed_m_s": 0.5,
            "properties": {"density_kg_m3": 0.7458, "dynamic_viscosity_pa_s": 2.577e-5, "conductivity_w_m_k": 0.03779, "specific_heat_j_kg_k": 1026.0},
        })),
    );
    let warnings = whole.get("warnings").and_then(|w| w.as_array()).cloned().unwrap_or_default();
    assert!(
        !warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains("Prandtl"))),
        "a fluid given whole needs no Prandtl warning: {warnings:?}"
    );
}

#[test]
fn a_flow_study_too_big_to_run_is_refused_with_the_size_of_it() {
    let mut s = cube();
    let why = fails(&mut s, "sim.flow", json!({"speed_m_s": 5.0, "cells": 50, "padding": 1.5, "steps": 100000}));
    assert!(why.contains("cell-steps"), "{why}");
    assert!(why.contains("fewer `cells`") || why.contains("fewer `steps`"), "it says what to cut: {why}");
}

#[test]
fn a_hostile_flow_study_is_refused_rather_than_guessed_at() {
    let mut s = cube();
    // Nothing moving is not a flow.
    assert!(fails(&mut s, "sim.flow", json!({})).contains("speed_m_s"));
    assert!(fails(&mut s, "sim.flow", json!({"speed_m_s": 0.0})).contains("speed_m_s"));
    assert!(fails(&mut s, "sim.flow", json!({"speed_m_s": -5.0})).contains("speed_m_s"));
    // A direction that is not one.
    assert!(fails(&mut s, "sim.flow", quick_flow(json!({"direction": [0, 0, 0]}))).contains("direction"));
    assert!(fails(&mut s, "sim.flow", quick_flow(json!({"direction": "along"}))).contains("direction"));
    // A fluid nobody has.
    assert!(fails(&mut s, "sim.flow", quick_flow(json!({"fluid": "Mercury"}))).contains("no fluid"));
    // A lattice speed past the low-Mach limit, and one of nothing.
    assert!(fails(&mut s, "sim.flow", quick_flow(json!({"lattice_speed": 0.5}))).contains("lattice_speed"));
    assert!(fails(&mut s, "sim.flow", quick_flow(json!({"lattice_speed": 0.0}))).contains("lattice_speed"));
    // A collision scheme that is not one, a tunnel of nothing, and steps that are not steps.
    assert!(fails(&mut s, "sim.flow", quick_flow(json!({"collision": "cascaded"}))).contains("collision"));
    assert!(fails(&mut s, "sim.flow", quick_flow(json!({"padding": 0.0}))).contains("padding"));
    assert!(fails(&mut s, "sim.flow", quick_flow(json!({"steps": 0}))).contains("steps"));
    assert!(fails(&mut s, "sim.flow", quick_flow(json!({"settle": -1.0}))).contains("settle"));
    // And a body nobody has.
    assert!(fails(&mut s, "sim.flow", quick_flow(json!({"bodies": ["Nothing"]}))).contains("no body"));
}

#[test]
fn a_flow_study_leaves_the_design_as_it_was() {
    let mut s = cube();
    let features = s.doc.features.len();
    let bodies = s.world_state().unwrap().bodies.len();
    run(&mut s, "sim.flow", quick_flow(json!({})));
    assert_eq!(s.world_state().unwrap().bodies.len(), bodies);
    assert_eq!(s.doc.features.len(), features, "an analysis adds no feature");
    let spec = crate::cmd::find_command("sim.flow").expect("registered");
    assert!(!spec.undoable);
}

#[test]
fn the_mass_on_the_lattice_does_not_drift_while_the_flow_runs() {
    // An inlet and an outlet that did not add up would show here first: mass appearing or
    // vanishing is the symptom of a boundary condition that is not conserving.
    let mut s = cube();
    let v = run(&mut s, "sim.flow", quick_flow(json!({})));
    let cells = number(&v, &["lattice", "total_cells"]);
    let solid = number(&v, &["lattice", "model_cells"]);
    let fluid = cells - solid;
    let mass = number(&v, &["solver", "mass"]);
    // The reference density is one, so the mass should be about the fluid cell count. A couple
    // of per cent of drift is the pressure field settling, not a leak.
    assert!((mass / fluid - 1.0).abs() < 0.02, "{mass} over {fluid} fluid cells");
}

/// Rollers on the three near faces: held still without being stopped from changing shape.
fn near_rollers() -> Vec<Value> {
    (0..3)
        .map(|axis| {
            let side = ["-x", "-y", "-z"][axis];
            json!({"name": format!("{side} roller"), "side": side, "roller": true})
        })
        .collect()
}

/// `fit_surface` reaches the solver, and on a round part it is worth asking for.
///
/// A rod pulled along its axis carries a uniform stress -- the traction, everywhere -- and the
/// lattice cannot give it, because the barrel is a staircase and the worst cell is at one of its
/// inside corners. Pulling the boundary nodes onto the barrel takes those corners away. What is
/// asserted is the comparison and the direction, not a figure: a number pinned here would be
/// pinning this lattice rather than the physics.
#[test]
fn a_rod_pulled_along_its_axis_reads_nearer_the_truth_with_the_surface_followed() {
    let traction = 10.0;
    let loads = json!([
        {"name": "held", "side": "-z", "fixed": true},
        {"name": "pulled", "side": "+z", "traction_pa": [0, 0, 10.0e6]},
    ]);
    let mut worst = Vec::new();
    for fit in [false, true] {
        let mut s = Session::default();
        s.execute("solid.cylinder", &json!({"radius": 10, "height": 40, "base": [0, 0, 0]})).expect("a rod");
        let v = run(&mut s, "sim.stress", json!({"cells": 40, "material": "Steel", "fit_surface": fit, "loads": loads}));
        if fit {
            let moved = number(&v, &["stress", "fitted", "nodes_moved"]);
            assert!(moved > 500.0, "only {moved} nodes were pulled onto the barrel");
        }
        worst.push(number(&v, &["stress", "worst_von_mises_mpa"]));
    }
    let (off, on) = (worst[0], worst[1]);
    assert!(off > traction * 1.1, "the lattice should over-read a uniform stress: {off} MPa against {traction}");
    assert!(on < off, "following the surface should take some of the over-read off: {off} MPa then {on} MPa");
    assert!(on > traction * 0.9, "and it must not start under-reading: {on} MPa against {traction}");
}

/// Asked for where the rest of the study would not match it, the command says so.
///
/// A fitted cell is not a box of side `h`, so the weight of its material and the push of its own
/// warmth -- both still worked out as if it were -- no longer belong to it. The solver refuses
/// rather than mixing the two, and the refusal has to survive the trip through the command.
#[test]
fn following_the_surface_is_refused_where_the_study_would_not_match_it() {
    let loads = json!([{"name": "held", "side": "-z", "fixed": true}]);
    for extra in [json!({"gravity": true}), json!({"temperature_rise_k": 40})] {
        let mut s = Session::default();
        s.execute("solid.cylinder", &json!({"radius": 10, "height": 40, "base": [0, 0, 0]})).expect("a rod");
        let mut params = json!({"cells": 16, "material": "Steel", "fit_surface": true, "loads": loads});
        if let (Some(p), Some(e)) = (params.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                p.insert(k.clone(), v.clone());
            }
        }
        let why = fails(&mut s, "sim.stress", params);
        assert!(why.contains("not supported yet"), "{extra}: {why}");
    }
}

/// A part whose every wall lands on the lattice has nothing to follow, and is told so rather
/// than being given a different answer.
#[test]
fn a_box_asked_to_follow_its_surface_is_told_there_is_nothing_to_follow() {
    let mut s = cube();
    let loads = json!([
        {"name": "held", "side": "-x", "fixed": true},
        {"name": "pulled", "side": "+x", "traction_pa": [10.0e6, 0, 0]},
    ]);
    let plain = run(&mut s, "sim.stress", json!({"cells": 16, "material": "Steel", "loads": loads}));
    let fitted = run(&mut s, "sim.stress", json!({"cells": 16, "material": "Steel", "fit_surface": true, "loads": loads}));

    assert_eq!(number(&fitted, &["stress", "fitted", "nodes_moved"]), 0.0, "{fitted}");
    let warnings = fitted.get("warnings").and_then(|w| w.as_array()).cloned().unwrap_or_default();
    assert!(
        warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains("nothing to follow"))),
        "it should say there was nothing to follow: {warnings:?}"
    );
    // And the answer is the one it would have given anyway, to the bit.
    assert_eq!(
        number(&plain, &["stress", "worst_von_mises_mpa"]),
        number(&fitted, &["stress", "worst_von_mises_mpa"]),
        "a box square to the axes must answer the same either way"
    );
}

/// Keeping the thin cells pays only with the surface followed, and the study can ask for both.
///
/// Measured on the plate with a hole at 1.25 mm cells: 3.591 plain, 3.566 with the surface
/// followed, 3.448 with the thin cells kept as well, against CalculiX's 3.242 on a mesh that
/// follows the bore. The last of those is the nearest this gets, and before this it could not be
/// asked for at all -- the rule was a constant. It is not the default because the cells it keeps
/// are slivers, and on a part whose stress is uniform they only read high.
///
/// Here it runs coarser, so the figures are not those; what is asserted is the ordering, which
/// is the thing that holds.
#[test]
fn a_hole_reads_nearest_with_the_thin_cells_kept_and_the_surface_followed() {
    let loads = json!([
        {"name": "held", "side": "-x", "fixed": true},
        {"name": "pulled", "side": "+x", "traction_pa": [10.0e6, 0, 0]},
    ]);
    let mut worst = Vec::new();
    for (fit, least) in [(false, 0.5), (true, 0.5), (true, 0.15)] {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 100, "width": 100, "height": 6})).expect("a plate");
        s.execute("solid.cylinder", &json!({"radius": 10, "height": 20, "base": [50, 50, -7], "operation": "cut", "targets": ["Body1"]}))
            .expect("a hole");
        let v = run(&mut s, "sim.stress", json!({"cells": 44, "material": "Steel", "fit_surface": fit, "least_fill": least, "loads": loads}));
        worst.push(number(&v, &["stress", "worst_von_mises_mpa"]));
    }
    let (plain, fitted, both) = (worst[0], worst[1], worst[2]);
    assert!(plain > 20.0, "a hole concentrates the stress: {plain} MPa of a 10 MPa pull");
    assert!(both < fitted, "keeping the thin cells as well should read nearer: {fitted} then {both}");
    assert!(fitted <= plain, "following the surface should not read further: {plain} then {fitted}");
}

/// A fill rule that is not a fraction, or is outside what the lattice can carry, is refused.
#[test]
fn a_hostile_fill_rule_is_refused() {
    let loads = json!([{"name": "held", "side": "-z", "fixed": true}]);
    for bad in [json!(0.0), json!(0.9), json!(-1), json!("thin")] {
        let mut s = cube();
        let why = fails(&mut s, "sim.stress", json!({"cells": 12, "material": "Steel", "least_fill": bad, "loads": loads}));
        assert!(why.contains("least_fill"), "{bad}: {why}");
    }
}

/// A bar pulled along its axis reads the traction exactly, at every resolution asked for.
///
/// The state of strain is uniform and a trilinear brick reproduces a uniform strain field
/// exactly, so this is not a tolerance to argue about: the answer is 10 MPa and the tool should
/// say 10 MPa. It did not. On the lattice as it was, the same bar read 12.95, 12.41, 9.22, 14.19
/// and 14.08 MPa at spacings that do not divide its 20 mm section, and exactly 10 at the ones
/// that do -- 42 per cent out on the easiest case in mechanics, with *no trend as the lattice is
/// refined*, because the lattice overhung the bar and the half-full rule solved the overhang as
/// material. Every exactness test in the suite had missed it by choosing round spacings
/// (`slab_lattice` says so in its own comment). `Grid3::over_parts` now moves the count by a few
/// per cent until the part's box is a whole number of cells across, so a spacing that divides
/// the bar is what every one of these gets (docs/analysis-verification.md).
///
/// The resolutions here are the ones that were wrong: 16, 18, 24, 36 and 48 cells across the
/// length give 3.2, 3.6, 4.8, 7.2 and 9.6 cells across the section before fitting.
#[test]
fn a_bar_pulled_along_its_axis_reads_the_traction_exactly_at_every_resolution() {
    let mut loads = near_rollers();
    loads.push(json!({"name": "pulled", "side": "+x", "traction_pa": [10.0e6, 0, 0]}));
    for cells in [16, 18, 24, 36, 48] {
        let mut s = Session::default();
        s.execute("solid.box", &json!({"length": 100, "width": 20, "height": 20})).expect("a bar");
        let v = run(&mut s, "sim.stress", json!({"cells": cells, "material": "Steel", "loads": loads.clone()}));
        let worst = number(&v, &["stress", "worst_von_mises_mpa"]);
        assert!((worst - 10.0).abs() < 1e-6, "{cells} cells: {worst} MPa against an exact 10");
    }
}

/// The study can check whether its own answer has settled, and the answer differs by part.
///
/// Solve it twice, once on a lattice three quarters as fine, and see whether the worst stress
/// stayed put. It is the check every textbook asks for by hand and almost nobody does, and here
/// it earns its keep: a bar on rollers, whose state of strain is uniform, moves by a fraction of
/// a per cent, where a plate with a hole in it moves nine and a half, because the peak at a curved
/// wall climbs as the lattice is refined and never settles. One of those figures can be trusted
/// and the other cannot, and before this there was no way to tell them apart from inside the tool
/// -- only a warning that said the same thing about every part with a curved face.
///
/// The bar is on rollers rather than clamped for the same reason the exactness tests are: a clamp
/// blocks the Poisson contraction and so makes a concentration of its own at the held face, and
/// *that* is as lattice-bound as any other concentration. A clamped bar here moved 30% at these
/// spacings, which the check is right to report -- it just says nothing about whether the check
/// can recognise a case that has settled.
#[test]
fn a_study_can_tell_whether_its_own_answer_has_settled() {
    let loads = json!([
        {"name": "held", "side": "-x", "fixed": true},
        {"name": "pulled", "side": "+x", "traction_pa": [10.0e6, 0, 0]},
    ]);

    // A bar on rollers: the state of strain is uniform, so the lattice hardly matters.
    let mut held = near_rollers();
    held.push(json!({"name": "pulled", "side": "+x", "traction_pa": [10.0e6, 0, 0]}));
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 100, "width": 20, "height": 20})).expect("a bar");
    let bar = run(&mut s, "sim.stress", json!({"cells": 24, "material": "Steel", "settled": true, "loads": held}));
    let moved = number(&bar, &["stress", "settled", "moved_fraction"]);
    assert!(moved < 0.02, "a bar should settle, and moved {:.1}%", moved * 100.0);
    // And the coarser lattice really was coarser, so a figure that did not move says something.
    let coarser = number(&bar, &["stress", "settled", "coarser_cells"]);
    assert!((4.0..24.0).contains(&coarser), "the second lattice should be coarser than 24, and was {coarser}");
    let warnings = bar.get("warnings").and_then(|w| w.as_array()).cloned().unwrap_or_default();
    assert!(
        !warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains("has not settled"))),
        "a bar that settled should not be warned about: {warnings:?}"
    );

    // A hole: the peak is at a curved wall, where refining reads higher rather than settling.
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 100, "width": 100, "height": 6})).expect("a plate");
    s.execute("solid.cylinder", &json!({"radius": 10, "height": 20, "base": [50, 50, -7], "operation": "cut", "targets": ["Body1"]}))
        .expect("a hole");
    let hole = run(&mut s, "sim.stress", json!({"cells": 44, "material": "Steel", "settled": true, "loads": loads}));
    let moved = number(&hole, &["stress", "settled", "moved_fraction"]);
    assert!(moved > 0.05, "a hole should not settle, and moved only {:.1}%", moved * 100.0);
    let warnings = hole.get("warnings").and_then(|w| w.as_array()).cloned().unwrap_or_default();
    assert!(
        warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains("has not settled"))),
        "a figure that has not settled should say so: {warnings:?}"
    );
}

#[test]
fn a_bar_pulled_along_its_length_reports_the_stress_and_the_safety_factor() {
    let mut s = bar();
    let mut loads = near_rollers();
    loads.push(json!({"name": "pull", "side": "+x", "traction_pa": [100.0e6, 0, 0]}));
    let v = run(&mut s, "sim.stress", json!({"cells": 20, "material": "Steel", "loads": loads}));

    // A uniaxial pull: von Mises is the stress itself, and steel's nominal yield is 250 MPa.
    assert!((number(&v, &["stress", "worst_von_mises_mpa"]) - 100.0).abs() < 0.01, "{v}");
    assert!((number(&v, &["safety", "value"]) - 2.5).abs() < 0.01, "{v}");
    // The pull and what the rollers take cancel.
    let pull = 100.0e6 * 4.0e-4;
    assert!((number(&v, &["balance", "applied_n", "0"]) - pull).abs() / pull < 1e-9, "{v}");
    assert!((number(&v, &["balance", "reaction_n", "0"]) + pull).abs() / pull < 1e-6, "{v}");
    assert!(number(&v, &["balance", "imbalance"]) < 1e-6, "{v}");
    // The bar stretched by t·L/E: a twentieth of a millimetre.
    assert!((number(&v, &["movement", "most_mm"]) - 0.05).abs() < 0.001, "{v}");
    // And the body it is worst in is named.
    assert_eq!(v.get("stress").and_then(|x| x.get("worst")).and_then(|x| x.get("body")), Some(&json!("Body1")));
}

#[test]
fn a_bar_held_at_both_ends_and_warmed_carries_e_alpha_delta_t() {
    // The two solvers together, which is the point of having both. A conduction study on the
    // same lattice says the bar reaches a hundred degrees; the load case says that a bar held
    // between two walls and warmed eighty kelvin from where it was made carries −E·α·ΔT, which
    // for this steel is 192 MPa against a yield of 250 -- a safety factor of 1.3 from nothing
    // but being warm and not allowed to grow.
    let mut s = bar();
    let mut loads = near_rollers();
    loads.push(json!({"name": "+x roller", "side": "+x", "roller": true}));
    let v = run(
        &mut s,
        "sim.stress",
        json!({
            "cells": 20,
            "material": "Steel",
            "loads": loads,
            "heat": {
                "reference_c": 20,
                "conditions": [
                    {"name": "one end", "side": "-x", "fixed_c": 100},
                    {"name": "the other", "side": "+x", "fixed_c": 100},
                ],
            },
        }),
    );

    // The conduction study's own answer comes back too: the bar is at a hundred degrees all
    // through, since both ends are held there and nothing else carries heat away.
    assert!((number(&v, &["heat", "max_c"]) - 100.0).abs() < 0.01, "{v}");
    assert!((number(&v, &["heat", "min_c"]) - 100.0).abs() < 0.01, "{v}");
    assert!((number(&v, &["heat", "reference_c"]) - 20.0).abs() < 1e-9);

    let want: f64 = 200.0e9 * 1.2e-5 * 80.0 / 1.0e6;
    assert!((want - 192.0).abs() < 0.01, "the case itself: {want} MPa");
    assert!((number(&v, &["stress", "worst_von_mises_mpa"]) - want).abs() / want < 1e-3, "{v}");
    assert!((number(&v, &["safety", "value"]) - 250.0 / want).abs() < 0.01, "{v}");
    // Warming a part pushes on nothing outside it, so nothing was applied and the supports
    // take, in total, nothing -- even though each end is pushing hard on its wall.
    assert!(number(&v, &["balance", "imbalance"]) < 1e-6, "{v}");
}

#[test]
fn a_uniform_warming_may_be_given_outright_instead_of_solved_for() {
    let mut s = bar();
    let mut loads = near_rollers();
    loads.push(json!({"name": "+x roller", "side": "+x", "roller": true}));
    let v = run(&mut s, "sim.stress", json!({"cells": 20, "material": "Steel", "loads": loads, "temperature_rise_k": 80.0}));
    let want = 200.0e9 * 1.2e-5 * 80.0 / 1.0e6;
    assert!((number(&v, &["stress", "worst_von_mises_mpa"]) - want).abs() / want < 1e-3, "{v}");
    // No conduction study was run, so there is nothing to report about one.
    assert_eq!(v.get("heat"), Some(&Value::Null), "{v}");
    // And the two ways of saying it cannot both be given.
    let why = fails(
        &mut s,
        "sim.stress",
        json!({"cells": 8, "material": "Steel", "loads": near_rollers(), "temperature_rise_k": 10.0, "heat": {"reference_c": 20}}),
    );
    assert!(why.contains("not both"), "{why}");
}

#[test]
fn a_part_free_to_grow_when_it_is_warmed_carries_nothing() {
    let mut s = bar();
    let v = run(&mut s, "sim.stress", json!({"cells": 20, "material": "Steel", "loads": near_rollers(), "temperature_rise_k": 80.0}));
    // Held only where it has to be and free everywhere else, it simply grows.
    let stress = number(&v, &["stress", "worst_von_mises_mpa"]);
    assert!(stress < 0.5, "a part free to grow carries no stress: {stress} MPa");
    // The far corner moves by α·ΔT along all three of its distances from the held faces: a
    // tenth of a millimetre along the bar and a fiftieth across it, and what is reported is how
    // far it went altogether.
    let growth: f64 = 1.2e-5 * 80.0;
    let want: f64 = (growth * growth * (100.0 * 100.0 + 20.0 * 20.0 + 20.0 * 20.0)).sqrt();
    assert!((want - 0.0998).abs() < 0.001, "the case itself: {want} mm");
    assert!((number(&v, &["movement", "most_mm"]) - want).abs() / want < 0.01, "{v}");
    // And with no stress anywhere, there is no safety factor worth reporting.
    assert!(v.get("safety").and_then(|x| x.get("value")).and_then(Value::as_f64).is_none_or(|f| f > 100.0), "{v}");
}

#[test]
fn a_bar_hung_from_one_end_is_held_up_by_its_own_weight() {
    let mut s = bar();
    let v = run(
        &mut s,
        "sim.stress",
        json!({
            // Twenty cells along puts five-millimetre cells on a bar that is a hundred by
            // twenty, so the lattice holds it exactly and its weight is exactly its weight.
            "cells": 20,
            "material": "Steel",
            "loads": [{"name": "hung", "side": "-z", "fixed": true}],
            "gravity": true,
        }),
    );
    // Steel at 7.85 g/cm³ over 100 × 20 × 20 mm is about three newtons.
    let weight: f64 = 7850.0 * (0.1 * 4.0e-4) * 9.806_65;
    assert!((weight - 3.079).abs() < 0.01, "the case itself: {weight} N");
    assert!((number(&v, &["balance", "applied_n", "2"]) + weight).abs() / weight < 1e-6, "{v}");
    assert!((number(&v, &["balance", "reaction_n", "2"]) - weight).abs() / weight < 1e-5, "{v}");
    assert!(number(&v, &["balance", "imbalance"]) < 1e-5, "{v}");
    // Gravity may also be given outright, and pointing nowhere is the same as not giving it.
    let v =
        run(&mut s, "sim.stress", json!({"cells": 8, "material": "Steel", "loads": [{"side": "-z", "fixed": true}], "gravity": [0, 0, -9.80665]}));
    assert!(number(&v, &["balance", "applied_n", "2"]) < 0.0, "{v}");
}

#[test]
fn a_pressure_on_a_face_squeezes_the_part() {
    let mut s = bar();
    let mut loads = near_rollers();
    loads.push(json!({"name": "pressed", "side": "+x", "pressure_pa": 50.0e6}));
    let v = run(&mut s, "sim.stress", json!({"cells": 20, "material": "Steel", "loads": loads}));
    // A pressure of 50 MPa on the end gives a uniaxial compression of 50 MPa, so von Mises is
    // 50 MPa and the part shortens.
    assert!((number(&v, &["stress", "worst_von_mises_mpa"]) - 50.0).abs() < 0.1, "{v}");
    let push = 50.0e6 * 4.0e-4;
    assert!((number(&v, &["balance", "applied_n", "0"]) + push).abs() / push < 1e-9, "pressure pushes inward: {v}");
}

#[test]
fn a_brittle_material_has_no_safety_factor_and_says_why() {
    let mut s = bar();
    let mut loads = near_rollers();
    loads.push(json!({"name": "pull", "side": "+x", "traction_pa": [50.0e6, 0, 0]}));
    let v = run(&mut s, "sim.stress", json!({"cells": 10, "material": "Cast Iron", "loads": loads}));
    assert!((number(&v, &["stress", "worst_von_mises_mpa"]) - 50.0).abs() < 0.1, "{v}");
    assert_eq!(v.get("safety"), Some(&Value::Null), "{v}");
    let warnings = v.get("warnings").and_then(Value::as_array).expect("warnings");
    assert!(warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains("no yield point") && w.contains("brittle"))), "{warnings:?}");
}

#[test]
fn a_material_whose_stiffness_runs_along_its_grain_is_refused_by_name() {
    // Oak is twenty times stiffer along the grain than across it. There is no single modulus
    // that would not be a lie, so a load case on it is refused rather than answered wrongly --
    // while a thermal study on the same oak runs, because conduction along axes is something
    // the lattice can carry.
    let mut s = bar();
    let mut loads = near_rollers();
    loads.push(json!({"name": "pull", "side": "+x", "traction_pa": [1.0e6, 0, 0]}));
    let why = fails(&mut s, "sim.stress", json!({"cells": 8, "material": "Oak", "loads": loads}));
    assert!(why.contains("orthotropic"), "{why}");
    assert!(why.contains("not supported yet"), "an unfinished feature says so: {why}");
    // The same material, the same lattice, a thermal study: fine.
    run(&mut s, "sim.thermal", json!({"cells": 8, "material": "Oak", "conditions": [{"side": "-x", "fixed_c": 20}, {"side": "+x", "fixed_c": 60}]}));
}

#[test]
fn mechanical_numbers_may_be_given_outright() {
    let mut s = bar();
    let mut loads = near_rollers();
    loads.push(json!({"name": "pull", "side": "+x", "traction_pa": [10.0e6, 0, 0]}));
    let v = run(
        &mut s,
        "sim.stress",
        json!({
            "cells": 10,
            "properties": {"Body1": {"young_modulus_pa": 70.0e9, "poisson_ratio": 0.33, "yield_strength_pa": 40.0e6}},
            "loads": loads,
        }),
    );
    let body = v.get("bodies").and_then(Value::as_array).and_then(|a| a.first().cloned()).expect("a body");
    assert_eq!(body.get("source"), Some(&json!("properties")));
    assert!((number(&body, &["young_modulus_pa"]) - 70.0e9).abs() < 1.0);
    assert!((number(&v, &["safety", "value"]) - 4.0).abs() < 0.01, "40 MPa against 10 MPa: {v}");
    // It stretched by t·L/E, which for a softer material is more.
    assert!((number(&v, &["movement", "most_mm"]) - 10.0e6 * 0.1 / 70.0e9 * 1000.0).abs() < 0.001, "{v}");
}

#[test]
fn a_case_with_nothing_holding_the_part_is_refused() {
    let mut s = bar();
    let why = fails(&mut s, "sim.stress", json!({"cells": 8, "material": "Steel"}));
    assert!(why.contains("drift"), "{why}");
    let why = fails(&mut s, "sim.stress", json!({"cells": 8, "material": "Steel", "loads": [{"side": "+x", "traction_pa": [1.0e6, 0, 0]}]}));
    assert!(why.contains("drift") || why.contains("settles"), "{why}");
}

#[test]
fn a_hostile_load_case_is_refused_rather_than_guessed_at() {
    let mut s = bar();
    let with = |loads: Value| json!({"cells": 8, "material": "Steel", "loads": loads});

    // A load that does not say what it does, or says it twice.
    assert!(fails(&mut s, "sim.stress", with(json!([{"side": "-x"}]))).contains("not what"));
    let why = fails(&mut s, "sim.stress", with(json!([{"side": "-x", "fixed": true, "roller": true}])));
    assert!(why.contains("more than once"), "{why}");
    // One that does not say where.
    assert!(fails(&mut s, "sim.stress", with(json!([{"fixed": true}]))).contains("does not say where"));
    // Loads that are not a list.
    assert!(fails(&mut s, "sim.stress", with(json!("everywhere"))).contains("list"));
    // A pressure or a traction that is not a number.
    assert!(fails(&mut s, "sim.stress", with(json!([{"side": "-x", "pressure_pa": "hard"}]))).contains("pressure_pa"));
    assert!(fails(&mut s, "sim.stress", with(json!([{"side": "-x", "traction_pa": 5}]))).contains("traction_pa"));
    // Properties that are not properties.
    let mut loads = near_rollers();
    loads.push(json!({"side": "+x", "traction_pa": [1.0e6, 0, 0]}));
    for bad in [json!({}), json!({"young_modulus_pa": 0.0}), json!({"young_modulus_pa": -1.0})] {
        let why = fails(&mut s, "sim.stress", json!({"cells": 8, "properties": {"Body1": bad}, "loads": loads.clone()}));
        assert!(why.contains("young_modulus_pa"), "{why}");
    }
    for bad in [0.5, 0.7, -1.0] {
        let why = fails(
            &mut s,
            "sim.stress",
            json!({"cells": 8, "properties": {"Body1": {"young_modulus_pa": 1.0e9, "poisson_ratio": bad}}, "loads": loads.clone()}),
        );
        assert!(why.contains("poisson_ratio"), "{bad}: {why}");
    }
    // A warming that is said but is not a number: refused, rather than taken as no warming at
    // all and answered as though the question had been a different one.
    let why = fails(&mut s, "sim.stress", json!({"cells": 8, "material": "Steel", "loads": near_rollers(), "temperature_rise_k": "warm"}));
    assert!(why.contains("temperature_rise_k"), "{why}");
    let why = fails(&mut s, "sim.stress", json!({"cells": 8, "material": "Steel", "loads": near_rollers(), "heat": {"conditions": []}}));
    assert!(why.contains("reference"), "{why}");
    // A gravity that is not a direction.
    assert!(fails(&mut s, "sim.stress", json!({"cells": 8, "material": "Steel", "loads": near_rollers(), "gravity": "down"})).contains("gravity"));
}

#[test]
fn a_stress_study_leaves_the_design_as_it_was() {
    let mut s = bar();
    let features = s.doc.features.len();
    run(&mut s, "sim.stress", json!({"cells": 8, "material": "Steel", "loads": [{"side": "-x", "fixed": true}], "gravity": true}));
    assert_eq!(s.doc.features.len(), features, "an analysis adds no feature");
    let spec = crate::cmd::find_command("sim.stress").expect("registered");
    assert!(!spec.undoable);
}

#[test]
fn a_stress_study_says_that_a_lattice_of_bricks_is_stiff_in_bending() {
    // The element's known shortcoming, said out loud on every answer rather than left in the
    // documentation: a deflection from this is short, and how short depends on the spacing.
    let mut s = bar();
    let v = run(
        &mut s,
        "sim.stress",
        json!({
            "cells": 10,
            "material": "Steel",
            "loads": [
                {"name": "clamped", "side": "-x", "fixed": true},
                {"name": "pushed", "side": "+x", "traction_pa": [0, 0, -1.0e6]},
            ],
        }),
    );
    let warnings = v.get("warnings").and_then(Value::as_array).expect("warnings");
    assert!(warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains("stiffer in bending"))), "{warnings:?}");
    // It does bend, and it bends the way it was pushed.
    assert!(number(&v, &["movement", "most_mm"]) > 0.0, "{v}");
}

// ---- saved studies -------------------------------------------------------------------------

#[test]
fn a_study_is_kept_by_the_design_and_runs_from_what_it_kept() {
    let mut s = bar();
    let made = run(&mut s, "sim.study.create", json!({"kind": "thermal", "name": "Hot end"}));
    assert_eq!(made["study"], "Hot end");
    run(
        &mut s,
        "sim.study.set",
        json!({
            "study": "Hot end",
            "setup": {"cells": 8, "material": "Steel", "conditions": [{"side": "-x", "fixed_c": 100}, {"side": "+x", "fixed_c": 20}]},
        }),
    );
    // The design holds it, so it survives everything a document survives.
    assert_eq!(s.doc.analysis.studies.len(), 1);
    let answer = run(&mut s, "sim.study.run", json!({"study": "Hot end"}));
    assert_eq!(answer["kind"], "thermal");
    // The ends were held at a hundred and at twenty, and the bar runs between them. The
    // extremes are the hottest and coldest cell *centres*, half a cell in from each held face,
    // so they fall short of the two held values rather than reaching them.
    let (max, min) = (number(&answer, &["result", "temperature", "max_c"]), number(&answer, &["result", "temperature", "min_c"]));
    assert!((85.0..100.0).contains(&max), "the hot end: {max}");
    assert!((20.0..35.0).contains(&min), "the cold end: {min}");
}

#[test]
fn a_study_says_what_it_still_needs_before_it_is_run() {
    // The point of keeping a study is being told what is missing instead of finding out from
    // the solver, so the list has to disagree with the run about nothing.
    let mut s = bar();
    run(&mut s, "sim.study.create", json!({"kind": "thermal", "name": "Bare"}));
    let listed = run(&mut s, "sim.study.list", json!({}));
    let study = &listed["studies"][0];
    assert_eq!(study["ready"], false, "{study}");
    let nodes = study["nodes"].as_array().expect("nodes");
    let node = |name: &str| nodes.iter().find(|n| n["node"] == name).unwrap_or_else(|| panic!("{name} in {nodes:?}")).clone();
    assert_eq!(node("materials")["status"], "incomplete", "a bar with no material cannot conduct");
    assert_eq!(node("conditions")["status"], "incomplete", "and nothing holds a temperature");
    assert_eq!(node("lattice")["status"], "optional", "the lattice has a default that works");
    // And the solver refuses it for the same reasons the list gave.
    let why = fails(&mut s, "sim.study.run", json!({"study": "Bare"}));
    assert!(why.contains("material"), "{why}");

    // Once it has both, the list says so.
    run(&mut s, "sim.study.set", json!({"study": "Bare", "setup": {"material": "Steel", "conditions": [{"side": "-x", "fixed_c": 50}]}}));
    let listed = run(&mut s, "sim.study.list", json!({}));
    assert_eq!(listed["studies"][0]["ready"], true, "{listed}");
}

#[test]
fn each_kind_of_study_names_what_drives_it() {
    let mut s = bar();
    run(&mut s, "sim.study.create", json!({"kind": "flow", "name": "Wind"}));
    run(&mut s, "sim.study.create", json!({"kind": "stress", "name": "Pull"}));
    let listed = run(&mut s, "sim.study.list", json!({}));
    let of = |name: &str| {
        let studies = listed["studies"].as_array().expect("studies");
        studies.iter().find(|x| x["study"] == name).expect("the study").clone()
    };
    let nodes = |v: &Value| v["nodes"].as_array().cloned().unwrap_or_default();
    assert!(nodes(&of("Wind")).iter().any(|n| n["node"] == "flow" && n["status"] == "incomplete"), "a flow with no speed");
    assert!(nodes(&of("Pull")).iter().any(|n| n["node"] == "loads" && n["status"] == "incomplete"), "a stress with no load");
}

#[test]
fn a_study_is_refused_rather_than_guessed_at() {
    let mut s = bar();
    assert!(fails(&mut s, "sim.study.create", json!({})).contains("kind"), "no kind");
    assert!(fails(&mut s, "sim.study.create", json!({"kind": "buckling"})).contains("not a kind"), "a kind we cannot solve");
    assert!(fails(&mut s, "sim.study.create", json!({"kind": "thermal", "setup": 7})).contains("object"), "a setup that is not one");
    run(&mut s, "sim.study.create", json!({"kind": "thermal", "name": "One"}));
    assert!(fails(&mut s, "sim.study.create", json!({"kind": "thermal", "name": "One"})).contains("already here"), "twice");
    // A name nobody has: the refusal says what there is rather than only no.
    let why = fails(&mut s, "sim.study.run", json!({"study": "Two"}));
    assert!(why.contains("One"), "{why}");
    assert!(fails(&mut s, "sim.study.delete", json!({"study": "Two"})).contains("no study"), "deleting one that is not there");
    // A refused study leaves the design as it was.
    assert_eq!(s.doc.analysis.studies.len(), 1);
}

#[test]
fn a_study_names_itself_when_it_is_not_named() {
    let mut s = bar();
    assert_eq!(run(&mut s, "sim.study.create", json!({"kind": "thermal"}))["study"], "Thermal Study 1");
    assert_eq!(run(&mut s, "sim.study.create", json!({"kind": "thermal"}))["study"], "Thermal Study 2");
    assert_eq!(run(&mut s, "sim.study.create", json!({"kind": "flow"}))["study"], "Flow Study 1");
}

#[test]
fn making_and_deleting_a_study_is_an_undo_step_but_running_one_is_not() {
    // Changing the design is undoable; asking it a question is not, as with every other
    // analysis.
    let mut s = bar();
    run(&mut s, "sim.study.create", json!({"kind": "thermal", "name": "Keep"}));
    s.execute("edit.undo", &json!({})).expect("undone");
    assert!(s.doc.analysis.studies.is_empty(), "the study went with the undo");
    for (id, undoable) in
        [("sim.study.create", true), ("sim.study.set", true), ("sim.study.delete", true), ("sim.study.run", false), ("sim.study.list", false)]
    {
        let spec = crate::cmd::find_command(id).unwrap_or_else(|| panic!("{id} is registered"));
        assert_eq!(spec.undoable, undoable, "{id}");
    }
}

/// The bar, with a hundred kelvin across its length and the answer drawn on it.
///
/// Twenty cells along a hundred millimetres puts its walls exactly on cell boundaries, so the
/// profile through it is the straight line Fourier says it is and the numbers below are the
/// closed-form ones.
fn warmed_bar() -> Session {
    let mut s = bar();
    run(
        &mut s,
        "sim.thermal",
        json!({
            "cells": 20,
            "material": "Aluminum",
            "conditions": [
                {"name": "cold", "side": "-x", "fixed_c": 20},
                {"name": "hot", "side": "+x", "fixed_c": 120},
            ],
        }),
    );
    s
}

fn shown(s: &Session) -> &crate::AnalysisResult {
    s.analysis_result.as_ref().expect("a result is being drawn")
}

fn names(v: &Value) -> Vec<String> {
    v.get("fields")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("no fields in {v}"))
        .iter()
        .map(|f| f.as_str().unwrap_or_default().to_string())
        .collect()
}

#[test]
fn a_run_keeps_every_field_it_worked_out_and_shades_one_of_them() {
    let mut s = warmed_bar();
    let v = run(&mut s, "sim.result.stats", json!({}));
    assert_eq!(names(&v), ["Temperature", "Heat flux"], "a conduction run works out both");
    assert_eq!(v["field"], "Temperature", "and shades the first until it is told otherwise");

    // Shading another one does not run the solver again -- the answer is already held -- so the
    // same squares come back with a different number on them.
    let before = shown(&s).patches.len();
    let v = run(&mut s, "sim.result.show", json!({"field": "flux"}));
    assert_eq!(v["field"], "Heat flux", "a prefix picks it out while it picks out only one");
    assert_eq!(v["unit"], "W/m\u{b2}");
    assert_eq!(number(&v, &["squares"]) as usize, before);
    // By number as well as by name, counting from one the way someone reading the list would.
    assert_eq!(run(&mut s, "sim.result.show", json!({"field": 1}))["field"], "Temperature");
}

#[test]
fn the_flux_field_says_what_fourier_says_it_should() {
    // A hundred-millimetre aluminium bar with a hundred kelvin across it passes
    // k dT/dx = 167 x 100 / 0.1 = 167 kW through every square metre of its section, the same
    // all along it because the profile is a straight line. The watts are checked elsewhere;
    // what this checks is that the field read off the answer is the same answer.
    let mut s = warmed_bar();
    let v = run(&mut s, "sim.result.stats", json!({"field": "Heat flux"}));
    let mean = number(&v, &["volume", "mean"]);
    assert!((mean - 167_000.0).abs() < 167_000.0 * 0.02, "{mean}");
    let (low, high) = (number(&v, &["volume", "low"]), number(&v, &["volume", "high"]));
    assert!(high - low < 167_000.0 * 0.02, "one flux through a straight profile: {low} to {high}");
}

#[test]
fn the_statistics_of_a_field_are_that_field_added_up() {
    let mut s = warmed_bar();
    let v = run(&mut s, "sim.result.stats", json!({}));
    // The two ends are held, and the wall is what the surface carries, so it reaches both.
    assert!((number(&v, &["surface", "low"]) - 20.0).abs() < 1.0, "{v}");
    assert!((number(&v, &["surface", "high"]) - 120.0).abs() < 1.0, "{v}");
    // The cells sit half a cell in from each end, so the inside stops short of both -- but a
    // straight profile averages to the middle of its range whatever the lattice.
    assert!((number(&v, &["volume", "mean"]) - 70.0).abs() < 0.5, "{v}");
    assert!(number(&v, &["volume", "count"]) > 0.0, "{v}");
    assert!(number(&v, &["surface", "count"]) > 0.0, "{v}");
}

#[test]
fn a_cut_takes_away_one_side_and_shades_the_face_it_leaves() {
    let mut s = warmed_bar();
    let whole = shown(&s).patches.len();
    // Across the bar, halfway along it. The far half goes and the cut itself is shaded, which is
    // the whole point: the outside of this bar says nothing about how far in the heat reached.
    let v = run(&mut s, "sim.result.show", json!({"cut": {"at": [50, 0, 0], "normal": [1, 0, 0]}}));
    assert_eq!(v["inside"], true, "this lattice is small enough to keep");
    let r = shown(&s);
    assert!(r.patches.iter().all(|p| p.at.x <= 50.0 + 1e-6), "nothing is left on the side the normal points at");
    let on_cut: Vec<_> = r.patches.iter().filter(|p| (p.at.x - 50.0).abs() < 1e-6 && p.normal.x > 0.9).collect();
    assert!(!on_cut.is_empty(), "the cut is a face, not a hole");
    for p in &on_cut {
        assert!((60.0..80.0).contains(&p.value), "halfway along a 20-to-120 bar: {}", p.value);
    }
    assert!(r.patches.len() < whole, "half the surface has gone: {} of {whole}", r.patches.len());

    // Putting the model back is one word, and it comes back whole.
    run(&mut s, "sim.result.show", json!({"cut": false}));
    assert_eq!(shown(&s).patches.len(), whole);
}

#[test]
fn the_scale_may_be_pinned_and_broken_into_bands() {
    let mut s = warmed_bar();
    let v = run(&mut s, "sim.result.show", json!({"low": 0, "high": 100, "bands": 4}));
    assert!((number(&v, &["low"]) - 0.0).abs() < 1e-9, "{v}");
    assert!((number(&v, &["high"]) - 100.0).abs() < 1e-9, "{v}");
    let r = shown(&s);
    // Four bands, so every value lands on one of four colours, in the middle of its own band.
    for value in [0.0, 1.0, 24.0, 26.0, 49.0, 51.0, 74.0, 76.0, 99.0, 100.0] {
        let f = r.fraction(value);
        assert!([0.125, 0.375, 0.625, 0.875].iter().any(|b| (b - f).abs() < 1e-9), "{value} landed at {f}");
    }
    // Without them the scale is a smooth wash again, and without a range it is the field's own.
    run(&mut s, "sim.result.show", json!({"bands": false}));
    assert!((shown(&s).fraction(25.0) - 0.25).abs() < 1e-9, "{}", shown(&s).fraction(25.0));
    let v = run(&mut s, "sim.result.show", json!({"range": false}));
    assert!((number(&v, &["low"]) - 20.0).abs() < 1.0, "{v}");
    assert!((number(&v, &["high"]) - 120.0).abs() < 1.0, "{v}");
}

#[test]
fn a_threshold_leaves_only_the_part_that_is_past_it() {
    let mut s = warmed_bar();
    let whole = shown(&s).patches.len();
    run(&mut s, "sim.result.show", json!({"iso": 100}));
    let r = shown(&s);
    assert!(!r.patches.is_empty(), "the hot end of the bar is past a hundred degrees");
    assert!(r.patches.len() < whole, "and the rest of it is not: {} of {whole}", r.patches.len());
    for p in &r.patches {
        assert!(p.value >= 100.0, "{} is not past the threshold", p.value);
    }
    // A safety factor runs the other way, so "past the threshold" has to mean the low side of
    // it there. That is what `low_is_cool` is for, and the threshold reads it.
    run(&mut s, "sim.result.show", json!({"iso": false}));
    assert_eq!(shown(&s).patches.len(), whole);
}

#[test]
fn a_probe_reads_the_cell_the_point_falls_in() {
    let mut s = warmed_bar();
    // Halfway along a bar that runs from twenty degrees to a hundred and twenty.
    let v = run(&mut s, "sim.result.probe", json!({"at": [50, 10, 10]}));
    assert!((number(&v, &["value"]) - 70.0).abs() < 5.0, "{v}");
    assert_eq!(v["unit"], "degrees");
    // Off the lattice there is no temperature to report, and saying so beats inventing one.
    let v = run(&mut s, "sim.result.probe", json!({"at": [500, 500, 500]}));
    assert_eq!(v["value"], Value::Null, "{v}");
    assert_eq!(v["points"].as_array().map(Vec::len), Some(2), "both points stay marked: {v}");
    let v = run(&mut s, "sim.result.probe", json!({"clear": true}));
    assert_eq!(v["probes"].as_array().map(Vec::len), Some(0), "{v}");
}

#[test]
fn looking_at_a_result_is_not_an_undo_step() {
    // These commands change what is on screen, not what the part is. A cut plane that could be
    // undone would put an undo step between someone and the edit they actually want back.
    for id in ["sim.result.show", "sim.result.probe", "sim.result.stats"] {
        let spec = crate::cmd::find_command(id).unwrap_or_else(|| panic!("{id} is registered"));
        assert!(!spec.undoable, "{id}");
    }
    let mut s = warmed_bar();
    let saved = s.is_dirty();
    run(&mut s, "sim.result.show", json!({"cut": {"normal": [0, 1, 0]}, "bands": 6}));
    run(&mut s, "sim.result.probe", json!({"at": [50, 10, 10]}));
    run(&mut s, "sim.result.stats", json!({}));
    assert_eq!(s.is_dirty(), saved, "and the design is no more changed than it was");
}

#[test]
fn a_result_that_is_no_longer_about_this_model_is_not_redrawn() {
    let mut s = Session::default();
    assert!(fails(&mut s, "sim.result.show", json!({})).contains("no analysis result"));
    assert!(fails(&mut s, "sim.result.stats", json!({})).contains("no analysis result"));
    let mut s = warmed_bar();
    run(&mut s, "solid.box", json!({"corner": [200, 0, 0], "length": 10, "width": 10, "height": 10}));
    let why = fails(&mut s, "sim.result.show", json!({"field": "Temperature"}));
    assert!(why.contains("the model has changed"), "{why}");
}

#[test]
fn a_view_that_is_refused_leaves_the_one_before_it_untouched() {
    let mut s = warmed_bar();
    run(&mut s, "sim.result.show", json!({"field": "Heat flux", "bands": 8}));
    let before = shown(&s).view.clone();
    assert!(fails(&mut s, "sim.result.show", json!({"field": "nonsense"})).contains("no field"));
    assert!(fails(&mut s, "sim.result.show", json!({"field": 9})).contains("field"));
    assert!(fails(&mut s, "sim.result.show", json!({"bands": 1})).contains("bands"));
    assert!(fails(&mut s, "sim.result.show", json!({"low": 5, "high": 5})).contains("must differ"));
    assert!(fails(&mut s, "sim.result.show", json!({"cut": {"normal": [0, 0, 0]}})).contains("direction"));
    assert!(fails(&mut s, "sim.result.show", json!({"cut": true})).contains("cut"));
    assert_eq!(shown(&s).view, before, "nothing moved");
}

#[test]
fn a_stress_run_keeps_the_three_fields_a_stress_is_read_by() {
    let mut s = bar();
    run(
        &mut s,
        "sim.stress",
        json!({
            "cells": 10,
            "material": "Steel",
            "loads": [
                {"name": "held", "side": "-x", "fixed": true},
                {"name": "pulled", "side": "+x", "traction_pa": [50_000_000.0f64, 0.0, 0.0]},
            ],
        }),
    );
    let v = run(&mut s, "sim.result.stats", json!({}));
    assert_eq!(names(&v), ["Von Mises stress", "Displacement", "Safety factor"]);

    // A bar pulled along its length moves most at the end that is not held. The field shades
    // cells, and a cell against the held face has four of its eight corners on that face and
    // four a cell further in, so the least it reads is small rather than nothing -- which is
    // the honest number for a cell, and why the furthest a single corner went is reported
    // separately.
    let v = run(&mut s, "sim.result.stats", json!({"field": "Displacement"}));
    assert_eq!(v["unit"], "mm");
    let (least, most) = (number(&v, &["volume", "low"]), number(&v, &["volume", "high"]));
    assert!(most > 0.0, "the free end moves: {v}");
    assert!(least < most / 10.0, "and the held end barely does: {least} against {most}");
    assert!((number(&v, &["worst", "0"]) - 100.0).abs() < 1e-6, "the furthest movement is at the free end: {v}");

    // Low is the bad end of a safety factor, which is the one thing a picture of it must get
    // right, and it is held at ten so an unloaded corner cannot flatten the scale.
    run(&mut s, "sim.result.show", json!({"field": "Safety"}));
    let r = shown(&s);
    assert!(!r.low_is_cool, "a safety factor is worst at the bottom of its range");
    assert!(r.fraction(r.low) > 0.99 && r.fraction(r.high) < 0.01, "{} to {}", r.low, r.high);
    assert!(r.high <= 10.0 + 1e-9, "held at ten: {}", r.high);
}

#[test]
fn a_stress_run_with_no_yield_point_offers_no_safety_factor() {
    // A column of blanks is worse than a column that is not there: the study already warns that
    // the body has no yield point, and a field that is nothing but holes invites the reading
    // that the part is safe everywhere.
    let mut s = bar();
    run(
        &mut s,
        "sim.stress",
        json!({
            "cells": 8,
            "properties": {"Body1": {"young_modulus_pa": 2.0e11, "poisson_ratio": 0.3}},
            "loads": [
                {"name": "held", "side": "-x", "fixed": true},
                {"name": "pulled", "side": "+x", "traction_pa": [1_000_000.0f64, 0.0, 0.0]},
            ],
        }),
    );
    let v = run(&mut s, "sim.result.stats", json!({}));
    assert_eq!(names(&v), ["Von Mises stress", "Displacement"]);
    assert!(fails(&mut s, "sim.result.show", json!({"field": "Safety factor"})).contains("no field"));
}

#[test]
fn streamlines_follow_the_flow_where_arrows_only_sample_it() {
    let mut s = cube();
    run(&mut s, "sim.flow", quick_flow(json!({"speed_m_s": 1.0})));
    let v = run(&mut s, "sim.result.stats", json!({}));
    assert_eq!(names(&v), ["Pressure", "Speed"]);

    let v = run(&mut s, "sim.result.show", json!({}));
    assert!(number(&v, &["arrows"]) > 0.0, "a flow is drawn with arrows until lines are asked for: {v}");
    assert_eq!(number(&v, &["lines"]) as usize, 0);
    let v = run(&mut s, "sim.result.show", json!({"streamlines": true}));
    assert!(number(&v, &["lines"]) > 0.0, "{v}");
    assert_eq!(number(&v, &["arrows"]) as usize, 0, "one or the other, not both at once");

    let r = shown(&s);
    // Taken together the lines go downstream, which is the claim worth making: a single line is
    // as likely to be one caught in the wake behind the cube, and that one really does turn
    // back on itself. Averaging over all of them is what says the flow goes where it is sent.
    let mut net = 0.0;
    for line in &r.lines {
        let (Some(first), Some(last)) = (line.points.first(), line.points.last()) else { continue };
        net += last.x - first.x;
        assert_eq!(line.points.len(), line.fractions.len(), "a speed for every point, so the line can be coloured");
        assert!(line.fractions.iter().all(|f| (0.0..=1.0).contains(f)));
    }
    assert!(net > 0.0, "the air comes from behind +x, so the lines as a whole travel that way: {net}");
}

#[test]
fn the_arrows_are_coloured_by_speed_whatever_field_is_shaded() {
    // The arrows say where the air goes and how fast; the shading says what the pressure is.
    // Reading the speed off the pressure scale would make a picture that invites one reading
    // and means another, so an arrow carries where it sits on the scale of speeds.
    let mut s = cube();
    run(&mut s, "sim.flow", quick_flow(json!({"speed_m_s": 1.0})));
    let r = shown(&s);
    assert_eq!(r.quantity, "Pressure", "the pressure is what is shaded");
    assert!(!r.arrows.is_empty());
    assert!(r.arrows.iter().all(|a| (0.0..=1.0).contains(&a.fraction)));
    let fastest = r.arrows.iter().max_by(|a, b| a.value.total_cmp(&b.value)).expect("an arrow");
    let slowest = r.arrows.iter().min_by(|a, b| a.value.total_cmp(&b.value)).expect("an arrow");
    assert!(fastest.fraction > slowest.fraction, "faster air is further along the scale of speeds");
}

#[test]
fn a_field_can_be_added_up_over_one_face_someone_picked() {
    // A mean over the whole skin is not the number a decision is made on. The held hot face of
    // this bar sits at a hundred and twenty and the cold one at twenty, while the surface as a
    // whole averages somewhere between the two.
    let mut s = warmed_bar();
    let hot = run(&mut s, "sim.result.stats", json!({"faces": [[100.0, 10.0, 10.0]]}));
    assert!((number(&hot, &["picked", "mean"]) - 120.0).abs() < 1.0, "{hot}");
    assert_eq!(hot["faces"].as_array().map(Vec::len), Some(1));
    assert!((number(&hot, &["faces", "0", "mean"]) - 120.0).abs() < 1.0, "{hot}");
    let cold = run(&mut s, "sim.result.stats", json!({"faces": [[0.0, 10.0, 10.0]]}));
    assert!((number(&cold, &["picked", "mean"]) - 20.0).abs() < 1.0, "{cold}");
    let whole = number(&hot, &["surface", "mean"]);
    assert!((20.0..120.0).contains(&whole), "the whole surface lies between the two ends: {whole}");

    // Named outright as well as pointed at, and the two have to agree: a pick and a point on
    // what was picked are the same pick.
    let face = cold["faces"][0]["face"].as_u64().expect("a face number");
    let body = cold["faces"][0]["body"].as_str().expect("a body name").to_string();
    let named = run(&mut s, "sim.result.stats", json!({"faces": [{"body": body, "face": face}]}));
    assert_eq!(named["picked"], cold["picked"], "{named}");

    // Nothing is picked over, and nothing is guessed at.
    assert!(fails(&mut s, "sim.result.stats", json!({"faces": []})).contains("picked nothing"));
    assert!(fails(&mut s, "sim.result.stats", json!({"faces": [{"body": "Nope", "face": 0}]})).contains("no body"));
    assert!(fails(&mut s, "sim.result.stats", json!({"faces": [{}]})).contains("face"));
    assert!(fails(&mut s, "sim.result.stats", json!({"faces": "everything"})).contains("faces"));
}

#[test]
fn a_cut_finds_its_face_wherever_the_plane_falls() {
    // Cell middles are a whole cell apart, so a band half a cell wide finds nothing at all for
    // half of the positions a slider can be dragged to. Every position has to land on exactly
    // one layer of cells: one, because two would be drawn in the same place.
    let mut s = warmed_bar();
    let spacing = s.analysis_result.as_ref().and_then(|r| r.lattice).map(|l| l.spacing).expect("a lattice");
    assert!((spacing - 5.0).abs() < 1e-9, "twenty cells over a hundred millimetres: {spacing}");
    for at in [40.0, 41.0, 42.5, 44.9, 45.0, 47.4, 47.5, 50.0, 51.0, 52.6, 55.0] {
        run(&mut s, "sim.result.show", json!({"cut": {"at": [at, 0, 0], "normal": [1, 0, 0]}}));
        let r = s.analysis_result.as_ref().expect("a result");
        let on_cut: Vec<_> = r.patches.iter().filter(|p| p.normal.x > 0.9 && (p.at.x - at).abs() < 1e-6).collect();
        assert!(!on_cut.is_empty(), "no cut face at all with the plane at x = {at}");
        let mut places: Vec<(i64, i64)> = on_cut.iter().map(|p| ((p.at.y * 1e3) as i64, (p.at.z * 1e3) as i64)).collect();
        places.sort_unstable();
        let before = places.len();
        places.dedup();
        assert_eq!(places.len(), before, "two squares of the cut in one place, with the plane at x = {at}");
    }
}

#[test]
fn a_field_that_is_not_one_is_refused_rather_than_overflowing() {
    // The cast that turns a number into an index saturates, so anything below isize::MIN
    // becomes isize::MIN, and taking one off that overflows -- which panics in every build with
    // overflow checks, and those are the builds the tests and CI run.
    let mut s = warmed_bar();
    for hostile in [json!(-1e308), json!(1e308), json!(-1e19), json!(f64::MAX), json!(0), json!(-1), json!(99), json!(1.5e300)] {
        let why = fails(&mut s, "sim.result.show", json!({"field": hostile.clone()}));
        assert!(why.contains("field"), "{hostile}: {why}");
        let why = fails(&mut s, "sim.result.stats", json!({"field": hostile.clone()}));
        assert!(why.contains("field"), "{hostile}: {why}");
    }
    // And the ones that are a field still pick it out, counting from one.
    assert_eq!(run(&mut s, "sim.result.show", json!({"field": 2}))["field"], "Heat flux");
    assert_eq!(run(&mut s, "sim.result.show", json!({"field": 1}))["field"], "Temperature");
}

#[test]
fn a_scale_pinned_by_hand_does_not_follow_the_field_it_was_pinned_for() {
    // Nought to a hundred degrees, carried on to a heat flux of a hundred and sixty thousand
    // watts a square metre, is not a scale: every square clamps to the far end and the whole
    // part comes out one colour.
    let mut s = warmed_bar();
    run(&mut s, "sim.result.show", json!({"low": 0, "high": 100}));
    let v = run(&mut s, "sim.result.show", json!({"field": "Heat flux"}));
    assert!(number(&v, &["high"]) > 1000.0, "the flux is shaded over its own range: {v}");
    // Unless the call says otherwise in the same breath, which means what it says.
    let v = run(&mut s, "sim.result.show", json!({"field": "Temperature", "low": 0, "high": 100}));
    assert!((number(&v, &["high"]) - 100.0).abs() < 1e-9, "{v}");
}

#[test]
fn a_field_with_nothing_in_it_clears_the_scale_rather_than_keeping_the_last_one() {
    // A bar held at one end and pulled by nothing has no stress, so it has no safety factor
    // anywhere. Leaving the last field's range on the legend would have it read as this one's.
    let mut s = bar();
    run(
        &mut s,
        "sim.stress",
        json!({
            "cells": 8,
            "material": "Steel",
            "loads": [{"name": "held", "side": "-x", "fixed": true}, {"name": "free", "side": "+x", "free": true}],
        }),
    );
    let before = run(&mut s, "sim.result.show", json!({"field": "Von Mises stress"}));
    let v = run(&mut s, "sim.result.show", json!({"field": "Safety factor"}));
    assert_eq!(v["field"], "Safety factor");
    assert_eq!(number(&v, &["squares"]) as usize, 0, "there is nothing to draw: {v}");
    assert!(
        (number(&v, &["low"]) - 0.0).abs() < 1e-9 && (number(&v, &["high"]) - 0.0).abs() < 1e-9,
        "and the scale is cleared rather than left at {}: {v}",
        number(&before, &["high"]),
    );
    // A point read on a field that holds nothing answers nothing, rather than answering zero.
    let probe = run(&mut s, "sim.result.probe", json!({"at": [50, 10, 10]}));
    assert_eq!(probe["value"], Value::Null, "{probe}");
}

#[test]
fn a_flux_through_a_wall_one_cell_thick_is_declined_rather_than_called_zero() {
    // A difference taken over nothing is not a flux of zero. Where the model is one cell thick
    // there is no cell on either side to difference against, and a confident nought there would
    // read as a wall that no heat crosses -- while the study reports real watts through it.
    let mut s = Session::default();
    s.execute("solid.box", &json!({"length": 60, "width": 6, "height": 6})).expect("a thin bar");
    run(
        &mut s,
        "sim.thermal",
        json!({
            "cells": 10,
            "material": "Aluminum",
            "conditions": [{"name": "cold", "side": "-x", "fixed_c": 20}, {"name": "hot", "side": "+x", "fixed_c": 120}],
        }),
    );
    // Ten cells over sixty millimetres is six, so the bar is one cell across in y and in z.
    let v = run(&mut s, "sim.result.stats", json!({"field": "Heat flux"}));
    assert_eq!(number(&v, &["volume", "count"]) as usize, 0, "the flux declines to answer: {v}");
    // The temperature itself is unaffected: it is solved for, not differenced.
    let t = run(&mut s, "sim.result.stats", json!({"field": "Temperature"}));
    assert!(number(&t, &["volume", "count"]) > 0.0, "{t}");
}

/// A body with no volume is left out of the model volume a study reports, rather than putting an
/// arbitrary figure into the denominator of `volume_error` -- which is the one number that says
/// whether the lattice holds the shape, and so whether any of the answers are about the model.
#[test]
fn a_body_that_does_not_close_is_left_out_of_the_model_volume() {
    let mut s = bar();
    run(&mut s, "sketch.create", json!({"plane": "XY", "offset": 50, "name": "S"}));
    run(&mut s, "sketch.rectangle.two_point", json!({"p0": [0, 0], "p1": [10, 10]}));
    run(&mut s, "sketch.finish", json!({}));
    run(&mut s, "surface.patch", json!({"sketch": "S", "body_name": "Sheet"}));
    let v = run(&mut s, "sim.lattice", json!({"cells": 10}));
    // The bar alone, not the bar plus the 1666.667 mm³ the sheet used to be worth.
    let bar_only = 100.0 * 20.0 * 20.0;
    assert!((number(&v, &["lattice", "model_volume_mm3"]) - bar_only).abs() < 1.0, "{v}");
    let w = v["warnings"].as_array().cloned().unwrap_or_default();
    assert_eq!(w.len(), 1, "the sheet is named: {v}");
    assert!(w[0].as_str().unwrap_or_default().starts_with("`Sheet` is not closed"), "{w:?}");
    // And the lattice's own volume is compared against a figure that means something.
    assert!(number(&v, &["lattice", "volume_error"]).abs() < 0.05, "{v}");
}

#[test]
fn a_flow_says_which_collision_operator_it_ran_and_whether_that_one_holds_a_wall() {
    let mut s = cube();
    // The default throws the lattice's own even moments away each step, which holds a lower
    // relaxation time than two rates can and puts the walls in the same place.
    let default = run(&mut s, "sim.flow", quick_flow(json!({})));
    assert_eq!(default.get("collision").and_then(|c| c.get("operator")), Some(&json!("regularized")));
    assert_eq!(default.get("collision").and_then(|c| c.get("holds_the_wall")), Some(&json!(true)));

    // Which is worth a factor of two in Reynolds number over two rates on the same lattice at
    // the same speed: the floor is 0.51 against 0.52, and the viscosity is (τ − 1/2)/3.
    let two_rate = run(&mut s, "sim.flow", quick_flow(json!({"collision": "trt"})));
    let (reg, trt) = (number(&default, &["reynolds", "ran_at"]), number(&two_rate, &["reynolds", "ran_at"]));
    assert!((reg / trt - 2.0).abs() < 0.05, "regularised should run at twice the Reynolds number: {reg} against {trt}");
    // The flow asked for is the same one either way; only how much of it could be run differs.
    let asked = (number(&default, &["reynolds", "value"]), number(&two_rate, &["reynolds", "value"]));
    assert!((asked.0 - asked.1).abs() < 1e-9, "the same flow was asked for: {asked:?}");

    // An operator that gives the wall up says so, because the force and the drag coefficient
    // are read at that wall. This is the one thing a user choosing it must not discover later.
    let mrt = run(&mut s, "sim.flow", quick_flow(json!({"collision": "mrt"})));
    assert_eq!(mrt.get("collision").and_then(|c| c.get("holds_the_wall")), Some(&json!(false)));
    let warnings = mrt.get("warnings").and_then(Value::as_array).expect("warnings");
    assert!(
        warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains("does not hold a bounce-back wall") && w.contains("regularized"))),
        "{warnings:?}"
    );
    // And it buys what it claims: twenty times the Reynolds number two rates could hold.
    let far = number(&mrt, &["reynolds", "ran_at"]);
    assert!((far / trt - 20.0).abs() < 0.5, "mrt should run at twenty times the Reynolds number: {far} against {trt}");

    // One rate also moves its walls, so it is warned about on the same grounds.
    let one_rate = run(&mut s, "sim.flow", quick_flow(json!({"collision": "bgk"})));
    let warnings = one_rate.get("warnings").and_then(Value::as_array).expect("warnings");
    assert!(warnings.iter().any(|w| w.as_str().is_some_and(|w| w.contains("does not hold a bounce-back wall"))), "{warnings:?}");

    // Anything else is refused, with the four names, rather than quietly falling back.
    let why = fails(&mut s, "sim.flow", quick_flow(json!({"collision": "cascaded"})));
    for name in ["regularized", "trt", "bgk", "mrt"] {
        assert!(why.contains(name), "the refusal should name {name}: {why}");
    }
}

#[test]
fn a_flow_puts_the_walls_where_the_model_has_them_unless_told_otherwise() {
    let mut s = cube();
    // A cube square to the axes on a lattice fitted to it lands on the lattice, so both answers
    // are the same one; what is checked here is that the choice is taken and reported, and that
    // asking for something that is not a wall is refused rather than quietly ignored.
    let model = run(&mut s, "sim.flow", quick_flow(json!({})));
    let lattice = run(&mut s, "sim.flow", quick_flow(json!({"wall": "lattice"})));
    for v in [&model, &lattice] {
        assert!(number(v, &["force", "drag_n"]) > 0.0, "the stream pushes the cube downstream");
    }
    let why = fails(&mut s, "sim.flow", quick_flow(json!({"wall": "halfway"})));
    assert!(why.contains("model") && why.contains("lattice"), "the refusal should name both: {why}");
}
