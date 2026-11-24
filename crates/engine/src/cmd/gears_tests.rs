//! `gear.insert`: the gear's size, volume and build time.

use std::f64::consts::PI;

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &mut Session) -> f64 {
    let m = run(s, "inspect.measure", json!({}));
    m["bodies"].as_array().map(|b| b.iter().filter_map(|x| x["volume_mm3"].as_f64()).sum()).unwrap_or(0.0)
}

#[test]
fn a_gear_is_solid_between_its_root_and_tip_circles() {
    let mut s = Session::default();
    let r = run(&mut s, "gear.insert", json!({"module": 2, "teeth": 20, "face_width": 10}));
    assert!((r["pitch_diameter"].as_f64().unwrap_or(0.0) - 40.0).abs() < 1e-9);
    let v = volume(&mut s);
    let (root, tip) = (17.5f64, 22.0f64);
    assert!(v > PI * root * root * 10.0 && v < PI * tip * tip * 10.0, "{v}");
    // The pitch circle cuts a tooth and a gap equally: the area is about that of the pitch circle.
    assert!((v / 10.0 - PI * 20.0 * 20.0).abs() < 0.06 * PI * 400.0, "{v}");
}

#[test]
fn hostile_gear_parameters_are_refused_and_leave_the_design_alone() {
    let mut s = Session::default();
    for p in [
        json!({"module": 0, "teeth": 20}),
        json!({"module": -2, "teeth": 20}),
        json!({"module": 2, "teeth": 3}),
        json!({"module": 2, "teeth": 100000}),
        json!({"module": 2, "teeth": 20, "pressure_angle": 90}),
        json!({"module": 2, "teeth": 20, "face_width": 0}),
        json!({"module": 2, "teeth": 20, "flank_points": 100000}),
        json!({"module": 2, "teeth": 20, "profile_shift": 1e9}),
        json!({"module": 1e300, "teeth": 20}),
    ] {
        assert!(s.execute("gear.insert", &p).is_err(), "{p}");
    }
    assert_eq!(volume(&mut s), 0.0);
}

#[test]
fn a_gear_with_many_teeth_builds_with_the_right_size() {
    // Four edges a tooth keep the extrude cheap (#6796): a 120-tooth gear is still solid at its pitch circle.
    let mut s = Session::default();
    run(&mut s, "gear.insert", json!({"module": 1, "teeth": 120, "face_width": 5}));
    let v = volume(&mut s);
    assert!((v / 5.0 - PI * 60.0 * 60.0).abs() < 0.03 * PI * 3600.0, "{v}");
}

fn tip_diameter(r: &Value) -> f64 {
    r["tip_diameter"].as_f64().unwrap_or(0.0)
}

#[test]
fn editing_a_gear_rebuilds_its_teeth_in_place() {
    let mut s = Session::default();
    let g = run(&mut s, "gear.insert", json!({"module": 2, "teeth": 20, "face_width": 10}));
    let comp = g["component"].clone();
    let before = volume(&mut s);
    let e = run(&mut s, "gear.edit", json!({"component": comp, "teeth": 30}));
    // The same component and occurrence, a bigger gear: 30 teeth of module 2 is Ø60 pitch.
    assert_eq!((&e["component"], &e["occurrence"]), (&g["component"], &g["occurrence"]));
    assert!((e["pitch_diameter"].as_f64().unwrap_or(0.0) - 60.0).abs() < 1e-9);
    assert!(tip_diameter(&e) > tip_diameter(&g));
    let after = volume(&mut s);
    assert!((after / 10.0 - PI * 30.0 * 30.0).abs() < 0.06 * PI * 900.0, "{before} {after}");
    // The default name follows the values.
    assert_eq!(e["name"], "Spur Gear M2 Z30");
    // One undo takes the edit back.
    run(&mut s, "edit.undo", json!({}));
    assert!((volume(&mut s) - before).abs() < 1e-6 * before);
    assert_eq!(s.doc.gears[&comp.as_u64().unwrap_or(0)].teeth, 20);
}

#[test]
fn editing_a_gear_to_the_values_it_has_changes_nothing() {
    let mut s = Session::default();
    let g = run(&mut s, "gear.insert", json!({"module": 2, "teeth": 20, "face_width": 10, "helix_angle": 15, "bore_diameter": 6}));
    let comp = g["component"].as_u64().unwrap_or(0);
    let (undo, features, revision) = (s.undo.len(), s.doc.features.len(), s.revision);
    let ids: Vec<u64> = s.doc.features.iter().map(|f| f.id).collect();
    // The form sends every value; sending the saved ones, or none, is no edit.
    for p in
        [json!({"component": comp}), json!({"component": comp, "module": 2, "teeth": 20, "face_width": 10, "helix_angle": 15, "bore_diameter": 6})]
    {
        let e = run(&mut s, "gear.edit", p);
        assert_eq!(e["component"].as_u64(), Some(comp));
        assert!((e["pitch_diameter"].as_f64().unwrap_or(0.0) - 40.0).abs() < 1e-9);
    }
    assert_eq!(s.undo.len(), undo, "no undo step");
    assert_eq!(s.doc.features.len(), features);
    assert_eq!(s.doc.features.iter().map(|f| f.id).collect::<Vec<_>>(), ids, "the features are not rebuilt");
    assert_eq!(s.revision, revision, "the document is untouched");
    // A real change still edits.
    run(&mut s, "gear.edit", json!({"component": comp, "teeth": 21}));
    assert_eq!(s.undo.len(), undo + 1);
}

#[test]
fn the_form_diameters_are_the_ones_the_command_reports() {
    let mut s = Session::default();
    for (internal, shift) in [(false, 0.3), (true, 0.0)] {
        let r = run(&mut s, "gear.insert", json!({"module": 2.5, "teeth": 24, "pressure_angle": 22, "profile_shift": shift, "internal": internal}));
        let d = crate::cmd::gear_diameters(2.5, 24, 22.0, shift, 0.25, internal).expect("diameters");
        for (got, key) in d.iter().zip(["pitch_diameter", "base_diameter", "tip_diameter", "root_diameter"]) {
            assert!((got - r[key].as_f64().unwrap_or(f64::NAN)).abs() < 1e-9, "{internal} {key}");
        }
    }
    assert!(crate::cmd::gear_diameters(f64::NAN, 20, 20.0, 0.0, 0.25, false).is_none());
}

#[test]
fn a_gear_keeps_its_values_through_save_and_reopen() {
    let mut s = Session::default();
    let g = run(&mut s, "gear.insert", json!({"module": 1.5, "teeth": 24, "pressure_angle": 25, "face_width": 8, "backlash": 0.05}));
    let comp = g["component"].as_u64().unwrap_or(0);
    let mut reopened = Session::new(solidcraft_doc::Document::from_json(&s.doc.to_json().to_string()).unwrap());
    assert_eq!(reopened.doc.gears.get(&comp), s.doc.gears.get(&comp));
    // The reopened design edits the same gear.
    let e = run(&mut reopened, "gear.edit", json!({"component": comp, "face_width": 12}));
    assert_eq!(e["component"].as_u64(), Some(comp));
    assert_eq!(reopened.doc.gears[&comp].teeth, 24);
    assert_eq!(reopened.doc.gears[&comp].backlash, 0.05);
}

#[test]
fn a_bad_gear_edit_is_refused_and_leaves_the_gear_alone() {
    let mut s = Session::default();
    let g = run(&mut s, "gear.insert", json!({"module": 2, "teeth": 20}));
    let comp = g["component"].clone();
    let before = volume(&mut s);
    for p in [json!({"teeth": 2}), json!({"module": -1}), json!({"pressure_angle": 90}), json!({"face_width": 0}), json!({"profile_shift": 1e9})] {
        let mut p = p;
        p["component"] = comp.clone();
        assert!(s.execute("gear.edit", &p).is_err(), "{p}");
    }
    assert!(s.execute("gear.edit", &json!({"component": 999, "teeth": 30})).is_err());
    assert!(s.execute("gear.edit", &json!({"component": "nothing"})).is_err());
    assert_eq!(volume(&mut s), before);
    assert_eq!(s.doc.gears[&comp.as_u64().unwrap_or(0)].teeth, 20);
}

#[test]
fn deleting_a_gear_forgets_its_values() {
    let mut s = Session::default();
    let g = run(&mut s, "gear.insert", json!({"module": 2, "teeth": 20}));
    run(&mut s, "component.delete", json!({"component": g["component"]}));
    assert!(s.doc.gears.is_empty());
}

#[test]
fn an_internal_gear_is_a_ring_with_its_teeth_inside() {
    let mut s = Session::default();
    let r = run(&mut s, "gear.insert", json!({"module": 2, "teeth": 40, "face_width": 8, "internal": true}));
    assert_eq!(r["name"], "Internal Gear M2 Z40");
    // Tips of the ring's teeth are inside the pitch circle, the roots outside it.
    assert!((r["tip_diameter"].as_f64().unwrap_or(0.0) - 76.0).abs() < 1e-9);
    assert!((r["root_diameter"].as_f64().unwrap_or(0.0) - 85.0).abs() < 1e-9);
    // The default ring is two modules thick outside the roots: outer radius 42.5 + 4. The pitch circle cuts a tooth and a gap equally.
    let v = volume(&mut s);
    assert!((v / 8.0 - PI * (46.5 * 46.5 - 40.0 * 40.0)).abs() < 0.06 * PI * 46.5 * 46.5, "{v}");
}

#[test]
fn an_external_gear_takes_an_axle_hole_and_edit_can_turn_it_into_a_ring() {
    let mut s = Session::default();
    run(&mut s, "gear.insert", json!({"module": 2, "teeth": 20, "face_width": 10}));
    let solid = volume(&mut s);
    run(&mut s, "gear.edit", json!({"component": "Spur Gear M2 Z20", "bore_diameter": 10}));
    // The axle hole takes a cylinder 10 mm across and 10 mm long out of the gear.
    assert!((solid - volume(&mut s) - PI * 5.0 * 5.0 * 10.0).abs() < 1.0, "{solid}");
    run(&mut s, "gear.edit", json!({"component": "Spur Gear M2 Z20", "bore_diameter": 0}));
    assert!((volume(&mut s) - solid).abs() < 1e-6);
    // Inside out: the saved axle hole is dropped and the name follows.
    let r = run(&mut s, "gear.edit", json!({"component": "Spur Gear M2 Z20", "internal": true, "teeth": 40}));
    assert_eq!(r["name"], "Internal Gear M2 Z40");
    // One ring: its area is the pitch-circle annulus out to the default rim (outer radius 46.5), not zero and not a disc.
    let bodies = run(&mut s, "inspect.measure", json!({}))["bodies"].as_array().map_or(0, Vec::len);
    assert_eq!(bodies, 1);
    let v = volume(&mut s);
    assert!((v / 10.0 - PI * (46.5 * 46.5 - 40.0 * 40.0)).abs() < 0.06 * PI * 46.5 * 46.5, "{v}");
}

#[test]
fn turning_a_shifted_gear_into_a_ring_drops_the_shift_and_a_fraction_of_flank_points_is_refused() {
    let mut s = Session::default();
    run(&mut s, "gear.insert", json!({"module": 2, "teeth": 20, "profile_shift": 0.3}));
    run(&mut s, "gear.edit", json!({"component": "Spur Gear M2 Z20", "internal": true, "teeth": 40}));
    assert!(s.doc.gears.values().all(|g| g.internal && g.profile_shift == 0.0));
    let before = s.doc.gears.clone();
    for bad in [json!(2.5), json!(-3), json!("8")] {
        assert!(s.execute("gear.insert", &json!({"module": 2, "teeth": 20, "flank_points": bad})).is_err());
    }
    assert_eq!(s.doc.gears, before);
}

#[test]
fn edit_gear_refuses_when_the_component_holds_features_that_are_not_the_gears() {
    let mut s = Session::default();
    let g = run(&mut s, "gear.insert", json!({"module": 2, "teeth": 20, "face_width": 10}));
    run(&mut s, "component.activate", json!({"component": g["component"]}));
    run(&mut s, "sketch.create", json!({"plane": "XY"}));
    run(&mut s, "sketch.finish", json!({}));
    let features = s.doc.features.len();
    assert!(s.execute("gear.edit", &json!({"component": g["component"], "teeth": 24})).is_err());
    assert_eq!(s.doc.features.len(), features);
    assert_eq!(s.doc.gears[&g["component"].as_u64().unwrap_or(0)].teeth, 20);
}

#[test]
fn an_internal_or_bored_gear_keeps_its_values_through_save_and_reopen() {
    let mut s = Session::default();
    run(&mut s, "gear.insert", json!({"module": 2, "teeth": 30, "internal": true, "rim_thickness": 5}));
    run(&mut s, "gear.insert", json!({"module": 1, "teeth": 24, "bore_diameter": 6}));
    let saved = s.doc.gears.clone();
    let text = serde_json::to_string(&saved).unwrap_or_default();
    let back: std::collections::BTreeMap<u64, solidcraft_doc::SpurGear> = serde_json::from_str(&text).unwrap_or_default();
    assert_eq!(back, saved);
    assert!(saved.values().any(|g| g.internal && g.rim_thickness == 5.0));
    assert!(saved.values().any(|g| !g.internal && g.bore_diameter == 6.0));
}

#[test]
fn hostile_internal_and_bore_values_are_refused_and_leave_the_design_alone() {
    let mut s = Session::default();
    for p in [
        json!({"module": 2, "teeth": 40, "internal": true, "profile_shift": 0.5}),
        json!({"module": 2, "teeth": 40, "internal": true, "bore_diameter": 5}),
        json!({"module": 2, "teeth": 40, "internal": true, "rim_thickness": -1}),
        json!({"module": 2, "teeth": 40, "rim_thickness": 3}),
        json!({"module": 2, "teeth": 40, "bore_diameter": -1}),
        json!({"module": 2, "teeth": 40, "bore_diameter": 1e9}),
        json!({"module": 2, "teeth": 40, "bore_diameter": 80}),
        json!({"module": 2, "teeth": 40, "internal": "yes"}),
        json!({"module": 2, "teeth": 40, "backlash": -1}),
    ] {
        assert!(s.execute("gear.insert", &p).is_err(), "{p}");
    }
    assert_eq!(volume(&mut s), 0.0);
}

fn bodies(s: &mut Session) -> Vec<Value> {
    run(s, "inspect.measure", json!({}))["bodies"].as_array().cloned().unwrap_or_default()
}

#[test]
fn a_helical_gear_has_the_volume_of_its_straight_twin_and_the_same_height() {
    let mut s = Session::default();
    run(&mut s, "gear.insert", json!({"module": 2, "teeth": 20, "face_width": 10, "name": "Straight"}));
    let straight = volume(&mut s);
    run(&mut s, "gear.edit", json!({"component": "Straight", "helix_angle": 20}));
    let b = bodies(&mut s);
    assert_eq!(b.len(), 1);
    // Turning the outline as it goes up keeps every slice's area.
    assert!((volume(&mut s) - straight).abs() < 0.005 * straight, "{straight}");
    assert!((b[0]["bbox"]["max"][2].as_f64().unwrap_or(0.0) - 10.0).abs() < 1e-6);
    // And back to straight teeth.
    run(&mut s, "gear.edit", json!({"component": "Straight", "helix_angle": 0}));
    assert!((volume(&mut s) - straight).abs() < 1e-6);
}

#[test]
fn a_double_helical_gear_is_one_body_of_two_mirrored_halves() {
    let mut s = Session::default();
    run(&mut s, "gear.insert", json!({"module": 2, "teeth": 20, "face_width": 10, "name": "Straight"}));
    let straight = volume(&mut s);
    run(&mut s, "gear.edit", json!({"component": "Straight", "helix_angle": 25, "double_helical": true}));
    let b = bodies(&mut s);
    assert_eq!(b.len(), 1, "{b:?}");
    assert!((volume(&mut s) - straight).abs() < 0.005 * straight);
    assert!((b[0]["bbox"]["max"][2].as_f64().unwrap_or(0.0) - 10.0).abs() < 1e-6);
    assert!(b[0]["bbox"]["min"][2].as_f64().unwrap_or(1.0).abs() < 1e-6);
}

#[test]
fn an_internal_gear_can_be_helical_and_an_axle_hole_can_sit_off_centre() {
    let mut s = Session::default();
    let flat = run(&mut s, "gear.insert", json!({"module": 2, "teeth": 40, "face_width": 8, "internal": true, "name": "Ring"}));
    assert!(flat["component"].is_u64());
    let ring = volume(&mut s);
    run(&mut s, "gear.edit", json!({"component": "Ring", "helix_angle": 15}));
    assert!((volume(&mut s) - ring).abs() < 0.01 * ring, "{ring}");

    let mut s = Session::default();
    run(&mut s, "gear.insert", json!({"module": 2, "teeth": 20, "face_width": 10}));
    let solid = volume(&mut s);
    run(&mut s, "gear.edit", json!({"component": "Spur Gear M2 Z20", "bore_diameter": 6, "bore_offset": 6}));
    assert!((solid - volume(&mut s) - PI * 3.0 * 3.0 * 10.0).abs() < 1.0);
    // The hole is on +X, so what is left is heavier on -X.
    let b = bodies(&mut s);
    assert!(b[0]["center_of_mass"][0].as_f64().unwrap_or(0.0) < -0.1, "{b:?}");
}

#[test]
fn helical_and_offset_values_are_kept_through_save_and_refused_when_hostile() {
    let mut s = Session::default();
    run(&mut s, "gear.insert", json!({"module": 2, "teeth": 20, "helix_angle": -15, "double_helical": true, "bore_diameter": 5, "bore_offset": 2}));
    let saved = s.doc.gears.clone();
    let text = serde_json::to_string(&saved).unwrap_or_default();
    let back: std::collections::BTreeMap<u64, solidcraft_doc::SpurGear> = serde_json::from_str(&text).unwrap_or_default();
    assert_eq!(back, saved);
    assert!(saved.values().any(|g| g.helix_angle == -15.0 && g.double_helical && g.bore_offset == 2.0));

    let mut s = Session::default();
    for p in [
        json!({"module": 2, "teeth": 20, "helix_angle": 60}),
        json!({"module": 2, "teeth": 20, "helix_angle": "x"}),
        json!({"module": 2, "teeth": 20, "double_helical": true}),
        json!({"module": 2, "teeth": 20, "double_helical": 1, "helix_angle": 10}),
        json!({"module": 2, "teeth": 40, "internal": true, "helix_angle": 10, "double_helical": true}),
        json!({"module": 2, "teeth": 20, "bore_offset": 3}),
        json!({"module": 2, "teeth": 20, "bore_diameter": 4, "bore_offset": 20}),
        json!({"module": 2, "teeth": 20, "face_width": 1e6, "helix_angle": 45}),
    ] {
        assert!(s.execute("gear.insert", &p).is_err(), "{p}");
    }
    assert_eq!(volume(&mut s), 0.0);
}

fn root_diameter(r: &Value) -> f64 {
    r["root_diameter"].as_f64().unwrap_or(0.0)
}

#[test]
fn clearance_sets_how_deep_the_roots_lie_and_defaults_to_the_standard() {
    let mut s = Session::default();
    let std = run(&mut s, "gear.insert", json!({"module": 2, "teeth": 20, "face_width": 10}));
    let deep = run(&mut s, "gear.insert", json!({"module": 2, "teeth": 20, "face_width": 10, "clearance": 0.4}));
    // Root radius is pitch radius minus module times (1 + clearance): 20 - 2.5 and 20 - 2.8.
    assert!((root_diameter(&std) - 35.0).abs() < 1e-9, "{}", root_diameter(&std));
    assert!((root_diameter(&deep) - 34.4).abs() < 1e-9, "{}", root_diameter(&deep));
    let comp = deep["component"].as_u64().unwrap_or(0);
    assert_eq!(s.doc.gears[&comp].clearance, 0.4);
    // An edit can change it back, and a gear saved before clearance existed reads as 0.25.
    let back = run(&mut s, "gear.edit", json!({"component": comp, "clearance": 0.25}));
    assert!((root_diameter(&back) - 35.0).abs() < 1e-9);
    let old: solidcraft_doc::SpurGear =
        serde_json::from_value(json!({"module": 2.0, "teeth": 20, "pressure_angle": 20.0, "face_width": 10.0, "flank_points": 8}))
            .expect("an older gear reads");
    assert_eq!((old.clearance, old.root_fillet, old.tip_round, old.backlash_thicker), (0.25, 0.0, 0.0, false));
    let saved = serde_json::to_value(&s.doc.gears[&comp]).expect("saves");
    assert!(saved.get("clearance").is_none() && saved.get("root_fillet").is_none(), "defaults are not written: {saved}");
}

#[test]
fn a_root_fillet_adds_material_and_a_tip_rounding_takes_it_away() {
    let mut s = Session::default();
    let base = json!({"module": 3, "teeth": 24, "face_width": 10, "flank_points": 16});
    let with = |extra: Value| {
        let mut p = base.clone();
        if let (Some(p), Some(e)) = (p.as_object_mut(), extra.as_object()) {
            p.extend(e.clone());
        }
        p
    };
    let sharp = run(&mut s, "gear.insert", base.clone());
    let sharp_v = volume(&mut s);
    run(&mut s, "edit.undo", json!({}));
    let _ = sharp;
    run(&mut s, "gear.insert", with(json!({"root_fillet": 0.8})));
    let filleted = volume(&mut s);
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "gear.insert", with(json!({"tip_round": 0.5})));
    let rounded = volume(&mut s);
    assert!(filleted > sharp_v + 1.0 && filleted < sharp_v * 1.02, "{sharp_v} {filleted}");
    assert!(rounded < sharp_v - 1.0 && rounded > sharp_v * 0.98, "{sharp_v} {rounded}");
}

#[test]
fn an_internal_gear_takes_a_root_fillet_and_a_tip_rounding_too() {
    let mut s = Session::default();
    let base = json!({"module": 2, "teeth": 40, "face_width": 8, "internal": true, "flank_points": 16});
    run(&mut s, "gear.insert", base.clone());
    let sharp = volume(&mut s);
    run(&mut s, "edit.undo", json!({}));
    let mut p = base;
    p["root_fillet"] = json!(0.5);
    p["tip_round"] = json!(0.3);
    run(&mut s, "gear.insert", p);
    let rounded = volume(&mut s);
    // The ring's tooth roots lie outside its space: a fillet there is material removed from the ring,
    // and rounding the tips is material removed too.
    assert!(rounded != sharp && (rounded - sharp).abs() < sharp * 0.03, "{sharp} {rounded}");
}

#[test]
fn the_backlash_direction_thins_or_thickens_the_teeth() {
    let mut s = Session::default();
    let v = |s: &mut Session, extra: Value| {
        let mut p = json!({"module": 2, "teeth": 24, "face_width": 10});
        if let (Some(p), Some(e)) = (p.as_object_mut(), extra.as_object()) {
            p.extend(e.clone());
        }
        run(s, "gear.insert", p);
        let x = volume(s);
        run(s, "edit.undo", json!({}));
        x
    };
    let none = v(&mut s, json!({}));
    let thin = v(&mut s, json!({"backlash": 0.2}));
    let explicit = v(&mut s, json!({"backlash": 0.2, "backlash_direction": "thinner"}));
    let thick = v(&mut s, json!({"backlash": 0.2, "backlash_direction": "thicker"}));
    assert!(thin < none && none < thick, "{thin} {none} {thick}");
    assert!((thin - explicit).abs() < 1e-9);
    // Equal and opposite on the pitch circle: the two are as far from the plain gear as each other.
    assert!(((none - thin) - (thick - none)).abs() < 0.05 * (none - thin), "{thin} {none} {thick}");
}

#[test]
fn hostile_clearance_fillet_and_direction_values_are_refused() {
    let mut s = Session::default();
    let ok = json!({"module": 2, "teeth": 20, "face_width": 10});
    for extra in [
        json!({"clearance": -0.1}),
        json!({"clearance": 0.7}),
        json!({"clearance": 1e300}),
        json!({"root_fillet": -1}),
        json!({"root_fillet": 2.5}),
        json!({"tip_round": 2.5}),
        json!({"root_fillet": 2.0, "flank_points": 2}),
        json!({"backlash_direction": "sideways"}),
        json!({"backlash_direction": 3}),
        json!({"backlash": 3.1, "backlash_direction": "thicker"}),
    ] {
        let mut p = ok.clone();
        if let (Some(p), Some(e)) = (p.as_object_mut(), extra.as_object()) {
            p.extend(e.clone());
        }
        assert!(s.execute("gear.insert", &p).is_err(), "{extra}");
    }
    assert!(s.doc.gears.is_empty(), "a refused gear leaves nothing behind");
}
