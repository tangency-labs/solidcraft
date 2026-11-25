use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute(id, &p).unwrap_or_else(|e| panic!("{id} {p}: {e}"))
}

fn fails(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} {p} should fail, gave {v}"),
        Err(e) => e.to_string(),
    }
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-link-parameters-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A 10 × 20 plate `h` high (a user parameter, 5 mm), saved at `path`.
fn save_plate(path: &Path) {
    let mut part = Session::default();
    run(&mut part, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
    run(&mut part, "solid.box", json!({"length": 10, "width": 20, "height": "h", "body_name": "Plate"}));
    run(&mut part, "file.save", json!({"path": path}));
}

/// An assembly with `lift` (12 mm) that links the plate at `part` with `h = lift`.
fn driven_assembly(part: &Path) -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "lift", "expression": "12 mm"}));
    run(&mut s, "component.insert_design", json!({"path": part, "name": "Plate"}));
    s
}

#[test]
fn a_link_takes_the_values_its_expressions_give_and_follows_them() {
    let dir = folder("follows");
    let part = dir.join("plate.sc");
    save_plate(&part);
    let mut s = driven_assembly(&part);
    assert!((volume(&mut s) - 1000.0).abs() < 1e-6, "the design's own h = 5 mm");

    let set = run(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": "lift"}}));
    assert_eq!(set["parameters"]["h"], json!({"expression": "lift", "value": 12.0}));
    assert_eq!(set["geometry_changed"], true);
    assert!((volume(&mut s) - 2400.0).abs() < 1e-6);

    // A value edit drives the link in the same step, and Undo takes both back.
    let changed = run(&mut s, "parameters.change", json!({"name": "lift", "expression": "7 mm"}));
    assert_eq!(changed["linked"], json!(["Plate"]));
    assert!((volume(&mut s) - 1400.0).abs() < 1e-6);
    run(&mut s, "edit.undo", json!({}));
    assert!((volume(&mut s) - 2400.0).abs() < 1e-6);

    // An edit that doesn't change what the expression gives leaves the link alone.
    run(&mut s, "parameters.add", json!({"name": "other", "expression": "3 mm"}));
    let other = run(&mut s, "parameters.change", json!({"name": "other", "expression": "4 mm"}));
    assert!(other.get("linked").is_none(), "{other}");

    // The design keeps the link's parameters and their values; component.links lists them.
    let listed = run(&mut s, "component.links", json!({}));
    assert_eq!(listed["links"][0]["parameters"]["h"]["value"], 12.0);
    let link = s.doc.links.values().next().unwrap();
    assert_eq!(link.parameter_values.get("h").map(String::as_str), Some("12 mm"));

    // null gives the parameter back to the design.
    run(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": null}}));
    assert!((volume(&mut s) - 1000.0).abs() < 1e-6);
    assert!(s.doc.links.values().next().unwrap().parameters.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_link_refuses_what_it_cannot_set() {
    let dir = folder("refuses");
    let part = dir.join("plate.sc");
    save_plate(&part);
    let mut s = driven_assembly(&part);
    let unknown = fails(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"depth": "lift"}}));
    assert!(unknown.contains("no user parameter `depth`"), "{unknown}");
    let angle = fails(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": "30 deg"}}));
    assert!(angle.contains("`h` = `30 deg`"), "{angle}");
    let missing = fails(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": "nothing_here"}}));
    assert!(missing.contains("unknown parameter `nothing_here`"), "{missing}");
    assert!(s.doc.links.values().next().unwrap().parameters.is_empty(), "a refused set changes nothing");

    // A parameter a link uses can't go: the link's expression would no longer work out.
    run(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": "lift"}}));
    let deleted = fails(&mut s, "parameters.change", json!({"delete": "lift"}));
    assert!(deleted.contains("Plate"), "{deleted}");
    assert!((volume(&mut s) - 2400.0).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_value_edit_never_takes_in_a_changed_file() {
    let dir = folder("changed-file");
    let part = dir.join("plate.sc");
    save_plate(&part);
    let mut s = driven_assembly(&part);
    run(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": "lift"}}));
    // The plate becomes 40 wide on disk.
    let mut edited = Session::default();
    run(&mut edited, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
    run(&mut edited, "solid.box", json!({"length": 10, "width": 40, "height": "h", "body_name": "Plate"}));
    run(&mut edited, "file.save", json!({"path": part}));

    let changed = run(&mut s, "parameters.change", json!({"name": "lift", "expression": "7 mm"}));
    assert!(changed.get("linked").is_none(), "{changed}");
    assert!(changed["link_warnings"][0].as_str().unwrap_or("").contains("Update Links"), "{changed}");
    assert!((volume(&mut s) - 2400.0).abs() < 1e-6, "the link keeps its bodies");
    let refused = fails(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": "lift + 1 mm"}}));
    assert!(refused.contains("Update Links"), "{refused}");

    // Update Links takes in the file with the link's values: 10 × 40 × 7.
    run(&mut s, "component.update_links", json!({}));
    assert!((volume(&mut s) - 2800.0).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_linked_design_drives_the_designs_it_links_in_turn() {
    let dir = folder("nested");
    let plate = dir.join("plate.sc");
    save_plate(&plate);
    // The middle design has its own `mid_h` and links the plate with `h = mid_h`.
    let middle = dir.join("middle.sc");
    let mut mid = Session::default();
    run(&mut mid, "parameters.add", json!({"name": "mid_h", "expression": "5 mm"}));
    run(&mut mid, "component.insert_design", json!({"path": plate, "name": "Plate"}));
    run(&mut mid, "component.link_parameters", json!({"component": "Plate", "set": {"h": "mid_h"}}));
    run(&mut mid, "file.save", json!({"path": middle}));

    let mut top = Session::default();
    run(&mut top, "parameters.add", json!({"name": "top_h", "expression": "9 mm"}));
    run(&mut top, "component.insert_design", json!({"path": middle, "name": "Middle"}));
    assert!((volume(&mut top) - 1000.0).abs() < 1e-6);
    run(&mut top, "component.link_parameters", json!({"component": "Middle", "set": {"mid_h": "top_h"}}));
    assert!((volume(&mut top) - 1800.0).abs() < 1e-6);
    let changed = run(&mut top, "parameters.change", json!({"name": "top_h", "expression": "3 mm"}));
    assert_eq!(changed["linked"], json!(["Middle"]));
    assert!((volume(&mut top) - 600.0).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_driven_link_saves_and_opens_with_its_bodies() {
    let dir = folder("saved");
    let part = dir.join("plate.sc");
    save_plate(&part);
    let assembly = dir.join("assembly.sc");
    let mut s = driven_assembly(&part);
    run(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": "lift * 2"}}));
    run(&mut s, "file.save", json!({"path": assembly}));
    // It opens with the bodies it keeps, even without the linked file.
    std::fs::remove_file(&part).unwrap();
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path": assembly}));
    assert!((volume(&mut opened) - 4800.0).abs() < 1e-6);
    let link = opened.doc.links.values().next().unwrap();
    assert_eq!(link.parameters.get("h").map(String::as_str), Some("lift * 2"));
    // Without the file a value edit keeps the bodies and says why.
    let changed = run(&mut opened, "parameters.change", json!({"name": "lift", "expression": "1 mm"}));
    assert!(changed["link_warnings"][0].as_str().unwrap_or("").contains("Plate keeps its bodies"), "{changed}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_link_by_name_passes_this_designs_parameters_down() {
    let dir = folder("by-name");
    let part = dir.join("plate.sc");
    save_plate(&part);
    // The assembly has its own `h`, 8 mm, and nothing else the plate uses.
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "h", "expression": "8 mm"}));
    run(&mut s, "component.insert_design", json!({"path": part, "name": "Plate"}));
    let set = run(&mut s, "component.link_parameters", json!({"component": "Plate", "by_name": true}));
    assert_eq!(set["by_name"], true);
    assert_eq!(set["parameters"]["h"], json!({"expression": "h", "value": 8.0, "by_name": true}));
    assert!((volume(&mut s) - 1600.0).abs() < 1e-6);
    let changed = run(&mut s, "parameters.change", json!({"name": "h", "expression": "6 mm"}));
    assert_eq!(changed["linked"], json!(["Plate"]));
    assert!((volume(&mut s) - 1200.0).abs() < 1e-6);
    // An expression of the link's own wins over the name.
    run(&mut s, "parameters.add", json!({"name": "lift", "expression": "2 mm"}));
    run(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": "lift"}}));
    assert!((volume(&mut s) - 400.0).abs() < 1e-6);
    // Deleting this design's `h` gives the plate its own value back instead of failing.
    run(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": null}}));
    assert!((volume(&mut s) - 1200.0).abs() < 1e-6);
    run(&mut s, "parameters.change", json!({"delete": "h"}));
    assert!((volume(&mut s) - 1000.0).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(dir);
}

/// A linked shaft's cosmetic thread runs its whole length: driven longer, the thread in this
/// design is as long as the shaft's own with that value, through a new read and through records,
/// and Undo takes it back with the shaft (review of PR #1).
#[test]
fn a_driven_links_cosmetic_threads_follow_the_values_it_passes() {
    let dir = folder("threads");
    let part = dir.join("shaft.sc");
    let mut shaft = Session::default();
    run(&mut shaft, "parameters.add", json!({"name": "h", "expression": "20 mm"}));
    run(&mut shaft, "solid.cylinder", json!({"radius": 3, "height": "h", "body_name": "Shaft"}));
    run(&mut shaft, "solid.thread", json!({"face": [3, 0, 5], "designation": "M6", "from": [3, 0, 0]}));
    run(&mut shaft, "file.save", json!({"path": part}));

    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "len", "expression": "20 mm"}));
    run(&mut s, "component.insert_design", json!({"path": part, "name": "Shaft", "parameters": {"h": "len"}}));
    assert!(same_fingerprint(&thread_extents(&s), &thread_extents(&shaft)), "{} vs {}", thread_extents(&s), thread_extents(&shaft));
    let mut records = 0;
    for value in ["35 mm", "40 mm", "45 mm"] {
        let before = super::RECORD_READS.with(std::cell::Cell::get);
        run(&mut s, "parameters.change", json!({"name": "len", "expression": value}));
        records += super::RECORD_READS.with(std::cell::Cell::get) - before;
        run(&mut shaft, "parameters.change", json!({"name": "h", "expression": value}));
        let (here, own) = (thread_extents(&s), thread_extents(&shaft));
        assert!(same_fingerprint(&here, &own), "h = {value}: this design {here}, the shaft itself {own}");
    }
    assert!(records > 0, "a drive handed the bodies over as records");
    run(&mut s, "edit.undo", json!({}));
    run(&mut shaft, "parameters.change", json!({"name": "h", "expression": "40 mm"}));
    assert!(same_fingerprint(&thread_extents(&s), &thread_extents(&shaft)), "undone: {}", thread_extents(&s));
    let _ = std::fs::remove_dir_all(dir);
}

/// A dimension's parameter that only a link's expression uses stays when the unused ones are
/// dropped, so the link goes on working its value out (review of #20542).
#[test]
fn a_dimension_parameter_a_link_passes_is_kept() {
    let dir = folder("link-uses-model-parameter");
    let part = dir.join("plate.sc");
    save_plate(&part);
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "other", "expression": "1 mm"}));
    let dimension =
        |name: &str| solidcraft_doc::Param { name: name.into(), expr: "8 mm".into(), unit: "mm".into(), comment: String::new(), model: true };
    s.doc_mut().params.extend([dimension("d901"), dimension("d902")]);
    run(&mut s, "component.insert_design", json!({"path": part, "name": "Plate", "parameters": {"h": "d901"}}));
    assert!((volume(&mut s) - 1600.0).abs() < 1e-6, "{}", volume(&mut s));

    s.doc_mut().prune_model_params();
    assert!(s.doc.param("d901").is_some(), "the link's expression uses it");
    assert!(s.doc.param("d902").is_none(), "nothing uses this one");
    run(&mut s, "parameters.change", json!({"name": "other", "expression": "2 mm"}));
    assert!((volume(&mut s) - 1600.0).abs() < 1e-6, "{}", volume(&mut s));
    let _ = std::fs::remove_dir_all(dir);
}

/// A number here does not set a length there by name: `h = 8` without a unit leaves the plate's
/// `h = 5 mm` alone, with a warning, and is not offered as the value this design would give. An
/// expression still reads a unit-less result in the parameter's unit (review of #20542).
#[test]
fn a_number_taken_by_name_does_not_set_a_length() {
    let dir = folder("by-name-number");
    let part = dir.join("plate.sc");
    save_plate(&part);
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "h", "expression": "8", "unit": ""}));
    let listed = run(&mut s, "component.design_parameters", json!({"path": part}));
    let row = listed["parameters"].as_array().into_iter().flatten().find(|r| r["name"] == "h").cloned().unwrap_or_default();
    assert!(row.get("here").is_none_or(Value::is_null), "nothing to take by name: {listed}");

    let inserted = run(&mut s, "component.insert_design", json!({"path": part, "name": "Plate", "by_name": true}));
    assert!(
        inserted["warnings"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|w| w.as_str().is_some_and(|w| w.contains("`h`") && w.contains("keeps its own value"))),
        "insert: {inserted}"
    );
    assert!((volume(&mut s) - 1000.0).abs() < 1e-6, "the plate keeps its own 5 mm: {}", volume(&mut s));

    run(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": "h"}}));
    assert!((volume(&mut s) - 1600.0).abs() < 1e-6, "an expression reads the number in mm: {}", volume(&mut s));
    let _ = std::fs::remove_dir_all(dir);
}

/// A parameter that only shares a name, an angle here and a length there, keeps the linked
/// design's own value, and the command that set the link going says so: Insert Design, Link
/// Parameters and Update Links each in their own reply (review of PR #1).
#[test]
fn a_by_name_unit_mismatch_is_said_by_the_command_that_meets_it() {
    let dir = folder("by-name-units");
    let part = dir.join("plate.sc");
    save_plate(&part);
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "h", "expression": "30 deg"}));
    let said = |reply: &Value| {
        reply["warnings"].as_array().into_iter().flatten().any(|w| w.as_str().is_some_and(|w| w.contains("`h`") && w.contains("keeps its own value")))
    };

    let inserted = run(&mut s, "component.insert_design", json!({"path": part, "name": "Plate", "by_name": true}));
    assert!(said(&inserted), "insert: {inserted}");
    assert!((volume(&mut s) - 1000.0).abs() < 1e-6, "the plate keeps its own 5 mm");

    let mut other = Session::default();
    run(&mut other, "parameters.add", json!({"name": "h", "expression": "30 deg"}));
    let quiet = run(&mut other, "component.insert_design", json!({"path": part, "name": "Plate"}));
    assert!(!said(&quiet), "a link that takes nothing by name has nothing to say: {quiet}");
    let set = run(&mut other, "component.link_parameters", json!({"component": "Plate", "by_name": true}));
    assert!(said(&set), "link parameters: {set}");
    assert!((volume(&mut other) - 1000.0).abs() < 1e-6);

    let updated = run(&mut other, "component.update_links", json!({}));
    assert!(said(&updated), "update links: {updated}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_link_driven_again_and_again_saves_what_a_new_read_of_its_bodies_gives() {
    // After its first read, a link driven again takes the design's live bodies in that read's
    // place: what it saves must open as the same bodies, with the same names and face names.
    let dir = folder("live-bodies");
    let part = dir.join("plate.sc");
    let mut plate = Session::default();
    run(&mut plate, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
    run(&mut plate, "solid.box", json!({"length": 10, "width": 20, "height": "h", "body_name": "Plate"}));
    run(&mut plate, "solid.cylinder", json!({"radius": 3, "height": "h * 2", "center": [30, 0, 0], "body_name": "Post"}));
    run(&mut plate, "file.save", json!({"path": part}));
    let mut s = driven_assembly(&part);
    run(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": "lift"}}));
    let before = super::RECORD_READS.with(std::cell::Cell::get);
    for lift in ["7 mm", "9 mm", "4 mm"] {
        let changed = run(&mut s, "parameters.change", json!({"name": "lift", "expression": lift}));
        assert_eq!(changed["linked"], json!(["Plate"]), "{changed}");
    }
    assert_eq!(super::RECORD_READS.with(std::cell::Cell::get) - before, 3, "each drive after the first read takes the live bodies");
    let shape = |s: &mut Session| {
        let measured = run(s, "inspect.measure", json!({}));
        let bodies: Vec<(String, f64)> = measured["bodies"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|b| (b["name"].as_str().unwrap_or_default().to_string(), b["volume_mm3"].as_f64().unwrap_or(f64::NAN)))
            .collect();
        let faces: Vec<Value> = bodies.iter().map(|(name, _)| run(s, "model.faces", json!({"body": name}))["faces"].clone()).collect();
        (bodies, faces)
    };
    let (live, live_faces) = shape(&mut s);
    let r: f64 = 3.0;
    assert!((live.iter().map(|(_, v)| v).sum::<f64>() - (10.0 * 20.0 * 4.0 + std::f64::consts::PI * r * r * 8.0)).abs() < 1e-3, "{live:?}");
    let saved = dir.join("assembly.sc");
    run(&mut s, "file.save", json!({"path": saved}));
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path": saved}));
    let (read, read_faces) = shape(&mut opened);
    assert_eq!(live.iter().map(|(n, _)| n).collect::<Vec<_>>(), read.iter().map(|(n, _)| n).collect::<Vec<_>>());
    for ((_, a), (name, b)) in live.iter().zip(&read) {
        assert!((a - b).abs() < 1e-6, "{name}: {a} live, {b} opened");
    }
    let names = |faces: &[Value]| -> Vec<Vec<String>> {
        faces.iter().map(|f| f.as_array().into_iter().flatten().map(|x| x["name"].as_str().unwrap_or_default().to_string()).collect()).collect()
    };
    assert_eq!(names(&live_faces), names(&read_faces));
    let _ = std::fs::remove_dir_all(dir);
}

/// A 10 × `w` × `h` block (user parameters `h` 5 mm and `w` 20 mm), saved at `path`.
fn save_block(path: &Path) {
    let mut part = Session::default();
    run(&mut part, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
    run(&mut part, "parameters.add", json!({"name": "w", "expression": "20 mm"}));
    run(&mut part, "solid.box", json!({"length": 10, "width": "w", "height": "h", "body_name": "Block"}));
    run(&mut part, "file.save", json!({"path": path}));
}

#[test]
fn a_kept_parameter_keeps_the_linked_designs_own_value() {
    let dir = folder("kept");
    let part = dir.join("block.sc");
    save_block(&part);
    // The assembly has its own `h` (8 mm) and `w` (30 mm).
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "h", "expression": "8 mm"}));
    run(&mut s, "parameters.add", json!({"name": "w", "expression": "30 mm"}));
    run(&mut s, "component.insert_design", json!({"path": part, "name": "Block"}));
    run(&mut s, "component.link_parameters", json!({"component": "Block", "by_name": true}));
    assert!((volume(&mut s) - 10.0 * 30.0 * 8.0).abs() < 1e-6);

    // Kept, `w` goes back to the block's own 20 mm; `h` still comes by name.
    let kept = run(&mut s, "component.link_parameters", json!({"component": "Block", "keep": ["w"]}));
    assert_eq!(kept["keep"], json!(["w"]));
    assert!(kept["parameters"].get("w").is_none(), "{kept}");
    assert_eq!(kept["parameters"]["h"]["value"], 8.0);
    assert!((volume(&mut s) - 10.0 * 20.0 * 8.0).abs() < 1e-6);
    // A change of the kept name drives nothing; a change of `h` still does.
    let w = run(&mut s, "parameters.change", json!({"name": "w", "expression": "40 mm"}));
    assert!(w.get("linked").is_none(), "{w}");
    let h = run(&mut s, "parameters.change", json!({"name": "h", "expression": "6 mm"}));
    assert_eq!(h["linked"], json!(["Block"]));
    assert!((volume(&mut s) - 10.0 * 20.0 * 6.0).abs() < 1e-6);
    let listed = run(&mut s, "component.links", json!({}));
    assert_eq!(listed["links"][0]["parameters_kept"], json!(["w"]));

    // What it can't keep is refused and changes nothing.
    let unknown = fails(&mut s, "component.link_parameters", json!({"component": "Block", "keep": ["depth"]}));
    assert!(unknown.contains("no number user parameter `depth` to keep"), "{unknown}");
    let both = fails(&mut s, "component.link_parameters", json!({"component": "Block", "set": {"w": "h"}}));
    assert!(both.contains("`w` is both kept and given an expression"), "{both}");
    let blank = fails(&mut s, "component.link_parameters", json!({"component": "Block", "keep": [""]}));
    assert!(blank.contains("`keep`"), "{blank}");
    assert_eq!(s.doc.links.values().next().unwrap().parameters_kept.iter().collect::<Vec<_>>(), ["w"]);

    // The design keeps the list; `[]` keeps none again.
    let saved = dir.join("assembly.sc");
    run(&mut s, "file.save", json!({"path": saved}));
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path": saved}));
    assert_eq!(opened.doc.links.values().next().unwrap().parameters_kept.iter().collect::<Vec<_>>(), ["w"]);
    run(&mut opened, "component.link_parameters", json!({"component": "Block", "keep": []}));
    assert!((volume(&mut opened) - 10.0 * 40.0 * 6.0).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn insert_design_sets_the_values_its_link_passes() {
    let dir = folder("insert");
    let part = dir.join("block.sc");
    save_block(&part);
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "lift", "expression": "12 mm"}));
    run(&mut s, "parameters.add", json!({"name": "w", "expression": "30 mm"}));

    // Refused settings make nothing.
    let null = fails(&mut s, "component.insert_design", json!({"path": part, "parameters": {"h": null}}));
    assert!(null.contains("`parameters.h` must be an expression"), "{null}");
    let both = fails(&mut s, "component.insert_design", json!({"path": part, "parameters": {"w": "lift"}, "keep": ["w"]}));
    assert!(both.contains("`w` is both kept"), "{both}");
    let unknown = fails(&mut s, "component.insert_design", json!({"path": part, "parameters": {"depth": "lift"}}));
    assert!(unknown.contains("no user parameter `depth`"), "{unknown}");
    assert!(s.doc.links.is_empty() && s.doc.components.iter().all(|c| c.name != "Block"));

    // By expression and by name from the start: h = lift, w by its name.
    let made = run(&mut s, "component.insert_design", json!({"path": part, "name": "Block", "parameters": {"h": "lift"}, "by_name": true}));
    assert_eq!(made["parameters"]["h"], json!({"expression": "lift", "value": 12.0}));
    assert_eq!(made["parameters"]["w"], json!({"expression": "w", "value": 30.0, "by_name": true}));
    assert!((volume(&mut s) - 10.0 * 30.0 * 12.0).abs() < 1e-6);
    let changed = run(&mut s, "parameters.change", json!({"name": "lift", "expression": "7 mm"}));
    assert_eq!(changed["linked"], json!(["Block"]));
    assert!((volume(&mut s) - 10.0 * 30.0 * 7.0).abs() < 1e-6);

    // Reuse takes only a link that passes the same; another one gets a link of its own.
    let same = run(&mut s, "component.insert_design", json!({"path": part, "parameters": {"h": "lift"}, "by_name": true, "reuse": true}));
    assert!(same.get("occurrence").is_some() && same.get("feature").is_none(), "{same}");
    let other = run(&mut s, "component.insert_design", json!({"path": part, "name": "Own", "by_name": true, "keep": ["w"], "reuse": true}));
    assert!(other.get("feature").is_some(), "{other}");
    assert!(other.get("parameters").is_none(), "keeps `w` and the assembly has no `h`: {other}");
    assert_eq!(s.doc.links.len(), 2);
    assert!((volume(&mut s) - 2.0 * 10.0 * 30.0 * 7.0 - 10.0 * 20.0 * 5.0).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(dir);
}

/// A plain value needs no parameter in the assembly (#20803): one design inserted twice into a
/// design with no user parameters, each time at its own size, makes two links that each keep the
/// value they were given.
#[test]
fn a_plain_value_needs_no_parameter_here_and_each_link_keeps_its_own() {
    let dir = folder("plain");
    let part = dir.join("block.sc");
    save_block(&part);
    let mut s = Session::default();
    let short = run(&mut s, "component.insert_design", json!({"path": part, "name": "Short", "parameters": {"h": "270 mm"}}));
    assert_eq!(short["parameters"]["h"], json!({"expression": "270 mm", "value": 270.0}));
    let long = run(&mut s, "component.insert_design", json!({"path": part, "name": "Long", "parameters": {"h": "510 mm"}, "reuse": false}));
    assert_eq!(long["parameters"]["h"], json!({"expression": "510 mm", "value": 510.0}));
    assert!(s.doc.params.iter().all(|p| p.model), "the assembly still has no user parameter");
    assert_eq!(s.doc.links.len(), 2);
    let passed: Vec<&str> = s.doc.links.values().map(|l| l.parameter_values["h"].as_str()).collect();
    assert_eq!(passed, ["270 mm", "510 mm"]);
    assert!((volume(&mut s) - 10.0 * 20.0 * (270.0 + 510.0)).abs() < 1e-6);
    // One link's value changes; the other keeps its own.
    run(&mut s, "component.link_parameters", json!({"component": "Short", "set": {"h": "350 mm"}}));
    assert!((volume(&mut s) - 10.0 * 20.0 * (350.0 + 510.0)).abs() < 1e-6);
    // Saved and opened again without the file, both are as they were.
    let assembly = dir.join("assembly.sc");
    run(&mut s, "file.save", json!({"path": assembly}));
    std::fs::remove_file(&part).unwrap();
    let mut again = Session::default();
    run(&mut again, "doc.open", json!({"path": assembly}));
    assert!((volume(&mut again) - 10.0 * 20.0 * (350.0 + 510.0)).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(dir);
}

/// A linked design's mesh bodies (#20050) come in with a link that passes values too, once, and
/// stay through each edit of what it passes.
#[test]
fn a_link_that_passes_values_keeps_the_designs_mesh_bodies() {
    let dir = folder("meshes");
    let (part, stl) = (dir.join("board.sc"), dir.join("Chip.stl"));
    std::fs::write(&stl, super::super::tests::tetra_stl()).unwrap();
    let mut board = Session::default();
    run(&mut board, "parameters.add", json!({"name": "t", "expression": "2 mm"}));
    run(&mut board, "solid.box", json!({"length": 10, "width": 20, "height": "t", "body_name": "Board"}));
    run(&mut board, "file.insert_mesh", json!({"path": stl}));
    run(&mut board, "file.save", json!({"path": part}));
    let meshes = |s: &Session| s.model.state().bodies.iter().filter(|b| b.body.is_mesh()).count();
    let solid = |s: &mut Session| {
        let measured = run(s, "inspect.measure", json!({}));
        let board = measured["bodies"].as_array().and_then(|b| b.iter().find(|b| b["name"] == "Board").cloned());
        board.and_then(|b| b["volume_mm3"].as_f64()).unwrap_or(f64::NAN)
    };
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "thick", "expression": "3 mm"}));
    let made = run(&mut s, "component.insert_design", json!({"path": part, "name": "Board", "parameters": {"t": "thick"}}));
    let comp = made["component"].as_u64().unwrap();
    assert_eq!(meshes(&s), 1, "{made}");
    assert!((solid(&mut s) - 10.0 * 20.0 * 3.0).abs() < 1e-6);
    let st = s.model.state();
    assert!(st.bodies.iter().all(|b| s.doc.model_body_component(b) == comp), "the mesh body is in the linked component");
    run(&mut s, "parameters.change", json!({"name": "thick", "expression": "5 mm"}));
    assert_eq!(meshes(&s), 1);
    assert!((solid(&mut s) - 10.0 * 20.0 * 5.0).abs() < 1e-6);
    run(&mut s, "component.link_parameters", json!({"component": "Board", "set": {"t": "4 mm"}}));
    assert_eq!(meshes(&s), 1);
    assert!((solid(&mut s) - 10.0 * 20.0 * 4.0).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(dir);
}

/// A design inserted with values brings its images where the design shows them with those
/// values: a decal on the plate's top comes in at the height the link passes, not the saved one.
#[test]
fn insert_design_with_values_brings_the_designs_images_where_those_values_put_them() {
    let dir = folder("insert-images");
    let part = dir.join("plate.sc");
    let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    png.extend(20u32.to_be_bytes());
    png.extend(10u32.to_be_bytes());
    png.extend([8, 6, 0, 0, 0, 0, 0, 0, 0]);
    let mut source = Session::default();
    run(&mut source, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
    run(&mut source, "solid.box", json!({"length": 10, "width": 20, "height": "h", "corner": [0, 0, 0], "body_name": "Plate"}));
    run(&mut source, "canvas.decal", json!({"path": crate::tests::image_file(&png), "face": [5, 10, 5], "width": 6, "name": "Logo"}));
    run(&mut source, "file.save", json!({"path": part}));
    let corners = |s: &Session| {
        let (vals, _) = s.doc.param_values();
        let c = s.doc.canvases.iter().find(|c| c.name == "Logo").expect("the decal");
        s.doc.canvas_corners(&vals, c, &s.model.state()).unwrap()
    };
    let saved = corners(&source);
    assert!(saved.iter().all(|c| (c.z - 5.0).abs() < 1e-6), "on the top at h = 5 mm: {saved:?}");

    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "lift", "expression": "12 mm"}));
    let made = run(&mut s, "component.insert_design", json!({"path": part, "name": "Plate", "parameters": {"h": "lift"}}));
    assert!((volume(&mut s) - 2400.0).abs() < 1e-6);
    let component = made["component"].as_u64().unwrap();
    assert_eq!(s.doc.canvases.iter().filter(|c| c.component == component).count(), 1, "the decal comes with the design");
    let got = corners(&s);
    for (g, e) in got.iter().zip(saved) {
        assert!((g.x - e.x).abs() < 1e-6 && (g.y - e.y).abs() < 1e-6 && (g.z - 12.0).abs() < 1e-6, "on the top at h = 12 mm: {got:?}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn design_parameters_lists_what_a_link_could_set_and_what_this_design_would_give() {
    let dir = folder("design-parameters");
    let part = dir.join("block.sc");
    save_block(&part);
    // This design has an `h` that would pass (a length) and a `w` that wouldn't (an angle).
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "h", "expression": "8 mm"}));
    run(&mut s, "parameters.add", json!({"name": "w", "expression": "30 deg"}));
    let listed = run(&mut s, "component.design_parameters", json!({"path": part}));
    let names: Vec<&str> = listed["parameters"].as_array().unwrap().iter().filter_map(|p| p["name"].as_str()).collect();
    assert_eq!(names, ["h", "w"]);
    assert_eq!(listed["parameters"][0]["value"], 5.0);
    assert_eq!(listed["parameters"][0]["here"], 8.0);
    assert!(listed["parameters"][1].get("here").is_none(), "an angle can't pass to a length: {listed}");
    assert_eq!((listed["by_name"].clone(), listed["keep"].clone()), (json!(false), json!([])));
    assert!(s.doc.links.is_empty(), "reading a design changes nothing");

    // For a linked component, also what the link passes.
    run(&mut s, "component.insert_design", json!({"path": part, "name": "Block", "parameters": {"w": "40 mm"}, "by_name": true, "keep": ["h"]}));
    let linked = run(&mut s, "component.design_parameters", json!({"component": "Block"}));
    assert_eq!(linked["set"], json!({"w": "40 mm"}));
    assert_eq!((linked["by_name"].clone(), linked["keep"].clone()), (json!(true), json!(["h"])));

    let both = fails(&mut s, "component.design_parameters", json!({"path": part, "component": "Block"}));
    assert!(both.contains("not both"), "{both}");
    let neither = fails(&mut s, "component.design_parameters", json!({}));
    assert!(neither.contains("give `path`"), "{neither}");
    let _ = std::fs::remove_dir_all(dir);
}

/// A link taking parameters by name starts driving one this design gains later, and one it lost
/// and gains again (review of PR #1).
#[test]
fn a_link_by_name_takes_a_parameter_this_design_gains_later() {
    let dir = folder("new-match");
    let part = dir.join("plate.sc");
    save_plate(&part);
    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path": part, "name": "Plate", "by_name": true}));
    assert!((volume(&mut s) - 1000.0).abs() < 1e-6, "no `h` here yet: the plate's own 5 mm");
    run(&mut s, "parameters.add", json!({"name": "h", "expression": "8 mm"}));
    assert!((volume(&mut s) - 1600.0).abs() < 1e-6, "the new `h` is passed at once");
    let changed = run(&mut s, "parameters.change", json!({"name": "h", "expression": "9 mm"}));
    assert_eq!(changed["linked"], json!(["Plate"]));
    assert!((volume(&mut s) - 1800.0).abs() < 1e-6);
    // Deleted, the plate goes back to its own value; added again, it is passed again.
    run(&mut s, "parameters.change", json!({"delete": "h"}));
    assert!((volume(&mut s) - 1000.0).abs() < 1e-6);
    run(&mut s, "parameters.add", json!({"name": "h", "expression": "6 mm"}));
    assert!((volume(&mut s) - 1200.0).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(dir);
}

/// Renaming a parameter a link's expression uses renames it there too, and a parameter a link
/// takes by name goes on being passed under its new name; a parameter an expression uses can't
/// be deleted (review of PR #1).
#[test]
fn renaming_a_parameter_carries_the_links_that_use_it() {
    let dir = folder("rename");
    let part = dir.join("plate.sc");
    save_plate(&part);
    let mut s = driven_assembly(&part);
    run(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": "lift"}}));
    run(&mut s, "parameters.rename", json!({"name": "lift", "new_name": "height"}));
    assert_eq!(s.doc.links.values().next().unwrap().parameters.get("h").map(String::as_str), Some("height"));
    let changed = run(&mut s, "parameters.change", json!({"name": "height", "expression": "7 mm"}));
    assert_eq!(changed["linked"], json!(["Plate"]));
    assert!((volume(&mut s) - 1400.0).abs() < 1e-6);
    // The rename form of parameters.change too.
    run(&mut s, "parameters.change", json!({"name": "height", "new_name": "rise"}));
    assert_eq!(s.doc.links.values().next().unwrap().parameters.get("h").map(String::as_str), Some("rise"));
    let users = run(&mut s, "parameters.users", json!({"name": "rise"}));
    assert!(users.to_string().contains("\"parameter\":\"h\""), "{users}");
    let refused = fails(&mut s, "parameters.change", json!({"delete": "rise"}));
    assert!(refused.contains("linked component Plate (h)"), "{refused}");

    // By name: the plate's `h` comes from this design's `h`; renamed, it still does.
    let mut named = Session::default();
    run(&mut named, "parameters.add", json!({"name": "h", "expression": "11 mm"}));
    run(&mut named, "component.insert_design", json!({"path": part, "name": "Plate", "by_name": true}));
    assert!((volume(&mut named) - 2200.0).abs() < 1e-6);
    run(&mut named, "parameters.rename", json!({"name": "h", "new_name": "plate_h"}));
    assert_eq!(named.doc.links.values().next().unwrap().parameters.get("h").map(String::as_str), Some("plate_h"));
    run(&mut named, "parameters.change", json!({"name": "plate_h", "expression": "4 mm"}));
    assert!((volume(&mut named) - 800.0).abs() < 1e-6);
    run(&mut named, "edit.undo", json!({}));
    run(&mut named, "edit.undo", json!({}));
    assert!(named.doc.links.values().next().unwrap().parameters.is_empty(), "Undo takes the rename back from the link too");
    let _ = std::fs::remove_dir_all(dir);
}

/// Renaming a parameter to a name the linked design has too changes no linked part: the link
/// goes on passing the old one under its new name, and the linked design's parameter of the
/// new name keeps its own value, as the link's Keep its own (review of #20542).
#[test]
fn renaming_a_parameter_to_a_name_the_linked_design_has_changes_no_linked_part() {
    let dir = folder("rename-onto");
    let path = dir.join("plate.sc");
    let mut part = Session::default();
    run(&mut part, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
    run(&mut part, "parameters.add", json!({"name": "w", "expression": "20 mm"}));
    run(&mut part, "solid.box", json!({"length": 10, "width": "w", "height": "h", "body_name": "Plate"}));
    run(&mut part, "file.save", json!({"path": path}));

    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "h", "expression": "11 mm"}));
    run(&mut s, "component.insert_design", json!({"path": path, "name": "Plate", "by_name": true}));
    assert!((volume(&mut s) - 2200.0).abs() < 1e-6, "{}", volume(&mut s));
    run(&mut s, "parameters.rename", json!({"name": "h", "new_name": "w"}));
    let link = s.doc.links.values().next().unwrap();
    assert_eq!(link.parameters.get("h").map(String::as_str), Some("w"));
    assert!(link.parameters_kept.contains("w"), "the plate's own `w` is kept");
    assert!((volume(&mut s) - 2200.0).abs() < 1e-6, "renamed: {}", volume(&mut s));
    run(&mut s, "parameters.change", json!({"name": "w", "expression": "12 mm"}));
    assert!((volume(&mut s) - 2400.0).abs() < 1e-6, "the plate's `h` follows, its `w` stays 20 mm: {}", volume(&mut s));
    run(&mut s, "edit.undo", json!({}));
    run(&mut s, "edit.undo", json!({}));
    let link = s.doc.links.values().next().unwrap();
    assert!(link.parameters.is_empty() && link.parameters_kept.is_empty(), "Undo takes the rename back from the link too");
    let _ = std::fs::remove_dir_all(dir);
}

/// A linked design's own value edit doesn't take what the outer edit had noted: a link that
/// kept its bodies still says so when a later link is driven (review of PR #1).
#[test]
fn every_link_an_edit_drives_or_leaves_is_reported() {
    let dir = folder("report");
    let missing = dir.join("missing.sc");
    let healthy = dir.join("healthy.sc");
    save_plate(&missing);
    save_plate(&healthy);
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "lift", "expression": "12 mm"}));
    run(&mut s, "component.insert_design", json!({"path": missing, "name": "Missing", "parameters": {"h": "lift"}}));
    run(&mut s, "component.insert_design", json!({"path": healthy, "name": "Healthy", "parameters": {"h": "lift + 1 mm"}}));
    std::fs::remove_file(&missing).unwrap();
    let reply = run(&mut s, "parameters.change", json!({"name": "lift", "expression": "7 mm"}));
    assert_eq!(reply["linked"], json!(["Healthy"]), "{reply}");
    let warnings = reply["link_warnings"].to_string();
    assert!(warnings.contains("Missing keeps its bodies"), "{reply}");

    // Two links driven: both are listed.
    let mut both = Session::default();
    run(&mut both, "parameters.add", json!({"name": "lift", "expression": "12 mm"}));
    run(&mut both, "component.insert_design", json!({"path": healthy, "name": "A", "parameters": {"h": "lift"}}));
    run(&mut both, "component.insert_design", json!({"path": healthy, "name": "B", "parameters": {"h": "lift + 1 mm"}}));
    let reply = run(&mut both, "parameters.change", json!({"name": "lift", "expression": "7 mm"}));
    assert_eq!(reply["linked"], json!(["A", "B"]), "{reply}");
    let _ = std::fs::remove_dir_all(dir);
}

/// Links inside links follow further than a few levels: ten designs, each linking the one
/// below by name, all take the top's value.
#[test]
fn a_chain_of_ten_linked_designs_follows_the_top() {
    let dir = folder("chain");
    let mut below = dir.join("level0.sc");
    save_plate(&below);
    for level in 1..10 {
        let path = dir.join(format!("level{level}.sc"));
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
        run(&mut s, "component.insert_design", json!({"path": below, "name": format!("L{}", level - 1), "by_name": true}));
        run(&mut s, "file.save", json!({"path": path}));
        below = path;
    }
    let mut top = Session::default();
    run(&mut top, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
    run(&mut top, "component.insert_design", json!({"path": below, "name": "L9", "by_name": true}));
    let changed = run(&mut top, "parameters.change", json!({"name": "h", "expression": "7 mm"}));
    assert_eq!(changed["linked"], json!(["L9"]), "{changed}");
    assert!(changed.get("link_warnings").is_none(), "{changed}");
    assert!((volume(&mut top) - 1400.0).abs() < 1e-6, "the plate ten levels down is 7 mm high");
    let _ = std::fs::remove_dir_all(dir);
}

/// Linked designs drive the designs they link to 32 levels down, and no further: a design
/// linked 33 levels down is refused, the edit's design and bodies stay as they were, and a
/// drive afterwards goes 32 levels down again (the nesting count unwound). Run on a thread with
/// a large stack: each level is a design evaluated inside the one above.
#[test]
fn links_follow_32_levels_down_and_no_further() {
    let body = || {
        let dir = folder("depth");
        let mut levels = vec![dir.join("level0.sc")];
        save_plate(&levels[0]);
        for level in 1..=32 {
            let path = dir.join(format!("level{level}.sc"));
            let mut s = Session::default();
            run(&mut s, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
            run(&mut s, "component.insert_design", json!({"path": levels[level - 1], "name": format!("L{}", level - 1), "by_name": true}));
            run(&mut s, "file.save", json!({"path": path}));
            levels.push(path);
        }
        let top = |below: &Path| {
            let mut s = Session::default();
            run(&mut s, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
            run(&mut s, "component.insert_design", json!({"path": below, "name": "Below", "by_name": true}));
            s
        };
        // 32 designs below the edit's: the plate is driven 32 levels down.
        let mut deep = top(&levels[31]);
        let changed = run(&mut deep, "parameters.change", json!({"name": "h", "expression": "7 mm"}));
        assert_eq!(changed["linked"], json!(["Below"]), "{changed}");
        assert!((volume(&mut deep) - 1400.0).abs() < 1e-6, "{changed}");
        // 33: refused.
        let mut deeper = top(&levels[32]);
        let before = (deeper.doc.to_json(), volume(&mut deeper));
        let refused = deeper.execute("parameters.change", &json!({"name": "h", "expression": "7 mm"})).unwrap_err().to_string();
        assert!(refused.contains("more than 32 deep"), "{refused}");
        assert_eq!((deeper.doc.to_json(), volume(&mut deeper)), before, "the refused edit changed nothing");
        // And again 32 levels down, after the refusal.
        assert_eq!(super::DEPTH.with(std::cell::Cell::get), 0, "the nesting count unwound");
        let changed = run(&mut deep, "parameters.change", json!({"name": "h", "expression": "9 mm"}));
        assert_eq!(changed["linked"], json!(["Below"]), "{changed}");
        assert!((volume(&mut deep) - 1800.0).abs() < 1e-6, "{changed}");
        let _ = std::fs::remove_dir_all(dir);
    };
    std::thread::Builder::new().stack_size(512 << 20).spawn(body).unwrap().join().unwrap();
}

/// A design that links one that links it back is a loop: the link that closes it keeps its
/// bodies and says so, and the edit goes on. The bodies of the design around it would not have
/// the values either, so its link keeps its own too, and says so by the path to the loop.
#[test]
fn a_link_back_to_a_design_passing_it_values_is_a_loop() {
    let dir = folder("loop");
    let a = dir.join("a.sc");
    let b = dir.join("b.sc");
    save_plate(&a);
    // b links a, passing it its `w` as `h`.
    let mut sb = Session::default();
    run(&mut sb, "parameters.add", json!({"name": "w", "expression": "5 mm"}));
    run(&mut sb, "component.insert_design", json!({"path": a, "name": "A", "parameters": {"h": "w"}}));
    run(&mut sb, "file.save", json!({"path": b}));
    // a, saved where it was, links b, passing it its `h` as `w`.
    let mut sa = Session::default();
    run(&mut sa, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
    run(&mut sa, "solid.box", json!({"length": 10, "width": 20, "height": "h", "body_name": "Plate"}));
    run(&mut sa, "file.save", json!({"path": a}));
    run(&mut sa, "component.insert_design", json!({"path": b, "name": "B", "parameters": {"w": "h"}}));
    run(&mut sa, "file.save", json!({"path": a}));
    let before = sa.doc.links.values().next().unwrap().parameter_values.clone();
    let changed = run(&mut sa, "parameters.change", json!({"name": "h", "expression": "7 mm"}));
    assert!(changed.get("linked").is_none(), "{changed}");
    assert_eq!(sa.doc.links.values().next().unwrap().parameter_values, before, "B keeps the values its bodies have");
    let warnings = changed["link_warnings"].to_string();
    assert!(warnings.contains("B keeps its bodies: in ") && warnings.contains("A keeps its bodies"), "{changed}");
    assert!(warnings.contains("links back") && warnings.contains("a.sc → b.sc → a.sc"), "{changed}");
    let _ = std::fs::remove_dir_all(dir);
}

/// A link to a configuration starts from that configuration's values: the dialog's data shows
/// them, and Keep its own keeps them (review of PR #1).
#[test]
fn a_link_to_a_configuration_shows_and_keeps_its_values() {
    let dir = folder("configured");
    let part = dir.join("configured.sc");
    let mut source = Session::default();
    run(&mut source, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
    run(&mut source, "solid.box", json!({"length": 10, "width": 20, "height": "h", "body_name": "Plate"}));
    run(&mut source, "config.configure", json!({"params": ["h"]}));
    run(&mut source, "config.row", json!({"name": "Tall", "values": {"h": "30 mm"}}));
    run(&mut source, "file.save", json!({"path": part}));

    let mut s = Session::default();
    run(&mut s, "component.insert_design", json!({"path": part, "name": "Plate", "configuration": "Tall", "parameters": {"h": "12 mm"}}));
    assert!((volume(&mut s) - 2400.0).abs() < 1e-6);
    let listed = run(&mut s, "component.design_parameters", json!({"component": "Plate"}));
    let h = listed["parameters"].as_array().unwrap().iter().find(|p| p["name"] == "h").unwrap().clone();
    assert_eq!(h["display_value"], json!(30.0), "the configuration's value, not the saved 5 mm: {h}");
    run(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": null}, "keep": ["h"]}));
    assert!((volume(&mut s) - 6000.0).abs() < 1e-6, "kept at the configuration's 30 mm");
    let _ = std::fs::remove_dir_all(dir);
}

/// What a design's bodies are, for comparing two evaluations of it: each body's name, measures,
/// topology counts and face names, and the cosmetic threads.
fn fingerprint(s: &Session) -> Value {
    let world = s.world_state().unwrap();
    let bodies: Vec<Value> = world
        .bodies
        .iter()
        .map(|b| {
            let m = solidcraft_kernel::measure(&b.body).unwrap();
            json!({
                "name": b.name,
                "volume": m.volume,
                "area": m.area,
                "bbox": [m.bbox.min.x, m.bbox.min.y, m.bbox.min.z, m.bbox.max.x, m.bbox.max.y, m.bbox.max.z],
                "topology": [m.faces, m.edges, m.vertices, m.shells],
                "face_names": *solidcraft_doc::naming::face_names(b),
            })
        })
        .collect();
    // What each body shows: its colours and its faces' (review of PR #1: the records once lost them).
    let shown = looks(s);
    json!({"bodies": bodies, "threads": thread_extents(s), "looks": shown})
}

/// A design's cosmetic threads: each one's body, size and where it starts and ends along its axis.
fn thread_extents(s: &Session) -> Value {
    let world = s.world_state().unwrap();
    world
        .threads
        .iter()
        .map(|t| json!({"body": t.body, "designation": t.designation, "major": t.major_diameter, "start": t.start, "end": t.end}))
        .collect()
}

/// The colours a design shows its bodies with: each body's own and its faces' (with opacity).
fn looks(s: &Session) -> Value {
    let world = s.world_state().unwrap();
    world
        .bodies
        .iter()
        .map(|b| {
            let faces: Vec<Value> =
                s.doc.face_colors(b).into_iter().map(|(face, look)| json!([face, look.color, (look.opacity * 1000.0).round() / 1000.0])).collect();
            json!({"name": b.name, "color": s.doc.body_color(b), "faces": faces})
        })
        .collect()
}

/// Whether two fingerprints agree: the same text and counts, numbers within 1e-6 of each other.
fn same_fingerprint(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap_or(f64::NAN), y.as_f64().unwrap_or(f64::NAN));
            (x - y).abs() <= 1e-6 * x.abs().max(y.abs()).max(1.0)
        }
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(x, y)| same_fingerprint(x, y)),
        (Value::Object(x), Value::Object(y)) => x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| same_fingerprint(v, w))),
        _ => a == b,
    }
}

/// The child half of [`a_driven_design_reopens_in_a_fresh_process_with_the_bodies_it_had`]: opens
/// the design named by `SOLIDCRAFT_LINK_FRESH_OPEN` without its saved geometry (every feature,
/// the linked bodies' Import too, made again) and prints its fingerprint.
#[test]
#[ignore = "run in a child process by a_driven_design_reopens_in_a_fresh_process_with_the_bodies_it_had"]
fn fresh_process_open() {
    let Ok(path) = std::env::var("SOLIDCRAFT_LINK_FRESH_OPEN") else { return };
    let bytes = std::fs::read(&path).unwrap();
    let (doc, saved) = super::read_document(Path::new(&path), &bytes, false).unwrap();
    assert!(saved.is_none(), "the saved geometry stays unread");
    let s = Session::opened_at_validating(doc, None, Some(&path), Session::default().validation());
    assert!(s.model.results.iter().all(|r| r.error.is_none()), "every feature evaluates");
    println!("FINGERPRINT {}", fingerprint(&s));
}

/// A design whose link was driven through records and through new reads saves bodies that a
/// fresh process, with no caches and none of the geometry the design saved, makes again exactly:
/// the same bodies, measures, topology, face names, threads, and a feature of this design built on
/// a face of the linked part (review of PR #1).
#[test]
fn a_driven_design_reopens_in_a_fresh_process_with_the_bodies_it_had() {
    let dir = folder("fresh");
    let part = dir.join("plate.sc");
    let mut plate = Session::default();
    run(&mut plate, "parameters.add", json!({"name": "h", "expression": "10 mm"}));
    run(&mut plate, "parameters.add", json!({"name": "n", "expression": "2", "unit": ""}));
    run(&mut plate, "solid.box", json!({"length": 40, "width": 20, "height": "h", "body_name": "Plate"}));
    run(&mut plate, "solid.fillet", json!({"edges": [[0, 0, 5], [40, 0, 5], [0, 20, 5], [40, 20, 5]], "radius": 2, "name": "Corners"}));
    run(
        &mut plate,
        "solid.hole",
        json!({"position": [6, 10, 0], "direction": [0, 0, 1], "thread": "M6", "depth": 8, "thread_depth": 4, "name": "Hole"}),
    );
    run(&mut plate, "solid.pattern.rectangular", json!({"features": ["Hole"], "dir1": [1, 0, 0], "count1": "n", "spacing1": 12, "name": "Holes"}));
    run(&mut plate, "file.save", json!({"path": part}));
    // The cosmetic threads the plate itself has with three holes, 14 mm high.
    run(&mut plate, "parameters.change", json!({"changes": {"h": "14 mm", "n": "3"}}));
    let own_threads = plate.world_state().unwrap().threads.len();
    assert!(own_threads > 0);

    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "lift", "expression": "10 mm"}));
    run(&mut s, "parameters.add", json!({"name": "count", "expression": "2", "unit": ""}));
    run(&mut s, "component.insert_design", json!({"path": part, "name": "Plate", "parameters": {"h": "lift", "n": "count"}}));
    // A feature of this design on the linked plate's top face, which must follow it.
    run(&mut s, "sketch.create", json!({"plane": {"face": [20, 10, 10]}, "name": "OnTop"}));
    run(&mut s, "sketch.circle.center", json!({"center": [0, 0], "radius": 2}));
    run(&mut s, "sketch.finish", json!({}));
    let extruded = run(&mut s, "solid.extrude", json!({"distance": 3, "operation": "new", "name": "Boss"}));
    let boss_name = extruded["created_bodies"][0].as_str().unwrap_or_else(|| panic!("{extruded}")).to_string();

    // Another hole: a new read. Then the same holes higher: records. (The first drive after the
    // linked design was opened reads too, and so may the first after the new hole: a read can list
    // a body's faces in another order than the rebuild did, and only a read that matches the live
    // body face for face stands for the next ones.)
    let mut records = Vec::new();
    for (name, value) in [("lift", "12 mm"), ("count", "3"), ("lift", "13 mm"), ("lift", "13.5 mm"), ("lift", "14 mm")] {
        let before = super::RECORD_READS.with(std::cell::Cell::get);
        let changed = run(&mut s, "parameters.change", json!({"name": name, "expression": value}));
        assert_eq!(changed["linked"], json!(["Plate"]), "{changed}");
        records.push(super::RECORD_READS.with(std::cell::Cell::get) - before);
    }
    assert_eq!(records[1], 0, "a new hole reads STEP text: {records:?}");
    assert!(records[2..].iter().sum::<usize>() >= 2 && records[4] == 1, "the drives that keep the topology hand over records: {records:?}");
    let live = fingerprint(&s);
    let boss = live["bodies"].as_array().unwrap().iter().find(|b| b["name"] == boss_name.as_str()).expect("the boss").clone();
    assert!((boss["bbox"][2].as_f64().unwrap() - 14.0).abs() < 1e-6, "the boss follows the plate's top face: {boss}");
    assert_eq!(live["threads"].as_array().map(Vec::len), Some(own_threads), "the plate's own threads: {live}");

    // The threads are the plate's own with these values, not the ones it was inserted with: the
    // live and reopened designs agreeing proves nothing when both keep the same stale ones.
    assert!(same_fingerprint(&live["threads"], &thread_extents(&plate)), "parent: {}\nplate itself: {}", live["threads"], thread_extents(&plate));

    let saved = dir.join("assembly.sc");
    run(&mut s, "file.save", json!({"path": saved}));
    let fresh = fresh_fingerprint(&saved);
    assert!(same_fingerprint(&live, &fresh), "live:\n{live}\nfresh process:\n{fresh}");
    let _ = std::fs::remove_dir_all(dir);
}

/// The fingerprint of the design saved at `saved`, opened in a fresh process with no caches and
/// none of its saved geometry ([`fresh_process_open`]).
fn fresh_fingerprint(saved: &Path) -> Value {
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "cmd::component_link::parameters::tests::fresh_process_open", "--ignored", "--nocapture", "--test-threads=1"])
        .env("SOLIDCRAFT_LINK_FRESH_OPEN", saved)
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&child.stdout);
    let printed = out
        .lines()
        .find_map(|l| l.split_once("FINGERPRINT ").map(|(_, f)| f))
        .unwrap_or_else(|| panic!("the child printed no fingerprint: {out}\n{}", String::from_utf8_lossy(&child.stderr)));
    serde_json::from_str(printed).unwrap()
}

/// A linked design whose bodies came in painted (a STEP part with its own face colours and
/// opacity) shows them still after a drive hands its bodies over as records, and after the
/// design reopens in a fresh process (review of PR #1).
#[test]
fn a_driven_link_keeps_the_face_colours_its_bodies_came_with() {
    use solidcore_record::Vec3;
    use solidcraft_doc::appearance::{FaceLook, Look};
    let dir = folder("paint");
    // A cube with a red, part-clear top, as a STEP file.
    let step = dir.join("cube.step");
    let mut cube = Session::default();
    run(&mut cube, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [0, 0, 0], "body_name": "Cube"}));
    cube.doc_mut().appearances.faces.push(FaceLook {
        body: "Cube".into(),
        point: Vec3::new(5.0, 5.0, 10.0),
        look: Look::custom([190, 20, 30], 0.65),
        name: None,
        strict_name: false,
        certificate: None,
    });
    run(&mut cube, "file.export", json!({"path": step.to_string_lossy()}));
    // A design with that cube imported, and a plate `h` high beside it.
    let part = dir.join("part.sc");
    let mut source = Session::default();
    run(&mut source, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
    run(&mut source, "file.insert_step", json!({"path": step.to_string_lossy()}));
    run(&mut source, "solid.box", json!({"length": 10, "width": 20, "height": "h", "corner": [20, 0, 0], "body_name": "Plate"}));
    run(&mut source, "file.save", json!({"path": part}));

    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "lift", "expression": "10 mm"}));
    run(&mut s, "component.insert_design", json!({"path": part, "name": "Part", "parameters": {"h": "lift"}}));
    let top = |s: &Session| {
        let state = s.model.state();
        let body = state.bodies.iter().find(|b| b.name.contains("Cube")).unwrap_or_else(|| panic!("no cube: {}", looks(s)));
        let face = solidcraft_doc::appearance::face_index_at(body, Vec3::new(5.0, 5.0, 10.0)).unwrap();
        s.doc.face_colors(body).into_iter().find(|(f, _)| *f == face).map(|(_, look)| (look.color, (look.opacity * 100.0).round()))
    };
    assert_eq!(top(&s), Some(([190, 20, 30], 65.0)), "inserted");
    let mut records = 0;
    for value in ["12 mm", "13 mm", "14 mm"] {
        let before = super::RECORD_READS.with(std::cell::Cell::get);
        let changed = run(&mut s, "parameters.change", json!({"name": "lift", "expression": value}));
        assert_eq!(changed["linked"], json!(["Part"]), "{changed}");
        records += super::RECORD_READS.with(std::cell::Cell::get) - before;
        assert_eq!(top(&s), Some(([190, 20, 30], 65.0)), "after lift = {value}: {}", looks(&s));
    }
    assert!(records > 0, "a drive handed the bodies over as records");
    let live = fingerprint(&s);
    let saved = dir.join("assembly.sc");
    run(&mut s, "file.save", json!({"path": saved}));
    let fresh = fresh_fingerprint(&saved);
    assert!(same_fingerprint(&live, &fresh), "live:\n{live}\nfresh process:\n{fresh}");
    let _ = std::fs::remove_dir_all(dir);
}

/// The child half of [`placements_of_a_design_whose_link_is_missing_keep_their_own_bodies`]:
/// opens the design named by `SOLIDCRAFT_LINK_FRESH_EDIT` in this fresh process, sets `h` to
/// 15 mm, saves it, opens it again, and prints what each linked placement has at each step.
#[test]
#[ignore = "run in a child process by placements_of_a_design_whose_link_is_missing_keep_their_own_bodies"]
fn fresh_process_edit() {
    let Ok(path) = std::env::var("SOLIDCRAFT_LINK_FRESH_EDIT") else { return };
    let mut s = Session::default();
    run(&mut s, "doc.open", json!({"path": path}));
    let opened = placements(&s);
    let changed = run(&mut s, "parameters.change", json!({"name": "h", "expression": "15 mm"}));
    let edited = placements(&s);
    run(&mut s, "file.save", json!({}));
    let mut again = Session::default();
    run(&mut again, "doc.open", json!({"path": path}));
    let reopened = placements(&again);
    println!(
        "PLACEMENTS {}",
        json!({"opened": opened, "edited": edited, "reopened": reopened, "linked": changed["linked"], "link_warnings": changed["link_warnings"]})
    );
}

/// Each linked placement: the volume of its bodies and the values its link last passed.
fn placements(s: &Session) -> Value {
    let world = s.world_state().unwrap();
    s.doc
        .links
        .iter()
        .map(|(comp, link)| {
            let name = s.doc.components.iter().find(|c| c.id == *comp).unwrap().name.clone();
            let volume: f64 =
                world.bodies.iter().filter(|b| b.feature == link.feature).map(|b| solidcraft_kernel::measure(&b.body).unwrap().volume).sum();
            (name, json!({"volume": (volume * 1000.0).round() / 1000.0, "values": link.parameter_values}))
        })
        .collect::<serde_json::Map<_, _>>()
        .into()
}

/// A master places one intermediate design twice, each placement passing it its own value, and
/// the intermediate design links a plate. With the plate's file gone, the master still opens with
/// the bodies it saved; a value edit then can't make new ones, so each placement keeps its own
/// bodies and the values it was made with, says so by its own path, and saves them so
/// (review of PR #1: both became the intermediate file's own 5 mm plate). With both placements
/// asking the same value, each still says so.
#[test]
fn placements_of_a_design_whose_link_is_missing_keep_their_own_bodies() {
    let dir = folder("nested-missing");
    let leaf = dir.join("leaf.sc");
    save_plate(&leaf);
    let middle = dir.join("middle.sc");
    let mut mid = Session::default();
    run(&mut mid, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
    run(&mut mid, "component.insert_design", json!({"path": leaf, "name": "Leaf", "parameters": {"h": "h"}}));
    run(&mut mid, "file.save", json!({"path": middle}));
    let master = |name: &str, second: &str| {
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name": "h", "expression": "10 mm"}));
        run(&mut s, "component.insert_design", json!({"path": middle, "name": "First", "parameters": {"h": "h"}}));
        run(&mut s, "component.insert_design", json!({"path": middle, "name": "Second", "parameters": {"h": second}}));
        let path = dir.join(name);
        run(&mut s, "file.save", json!({"path": path}));
        (path, placements(&s))
    };
    let (unequal, saved) = master("unequal.sc", "h + 2 mm");
    assert_eq!(saved["First"]["volume"], json!(2000.0), "{saved}");
    assert_eq!(saved["Second"]["volume"], json!(2400.0), "{saved}");
    let (equal, _) = master("equal.sc", "h");
    std::fs::rename(&leaf, dir.join("leaf-moved.sc")).unwrap();

    let edit = |path: &Path| -> Value {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "cmd::component_link::parameters::tests::fresh_process_edit", "--ignored", "--nocapture", "--test-threads=1"])
            .env("SOLIDCRAFT_LINK_FRESH_EDIT", path)
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&child.stdout);
        let printed = out
            .lines()
            .find_map(|l| l.split_once("PLACEMENTS ").map(|(_, f)| f))
            .unwrap_or_else(|| panic!("the child printed nothing: {out}\n{}", String::from_utf8_lossy(&child.stderr)));
        serde_json::from_str(printed).unwrap()
    };
    let r = edit(&unequal);
    for step in ["opened", "edited", "reopened"] {
        assert_eq!(r[step], saved, "{step}: each placement keeps its own bodies and values: {r}");
    }
    assert!(r["linked"].as_array().is_none_or(Vec::is_empty), "nothing took new bodies: {r}");
    let warnings: Vec<&str> = r["link_warnings"].as_array().unwrap().iter().filter_map(Value::as_str).collect();
    for name in ["First", "Second"] {
        assert_eq!(
            warnings.iter().filter(|w| w.starts_with(&format!("{name} keeps its bodies: in ")) && w.contains("Leaf keeps its bodies")).count(),
            1,
            "{name}: {r}"
        );
    }

    let r = edit(&equal);
    assert_eq!(r["edited"]["First"], r["opened"]["First"], "{r}");
    assert_eq!(r["edited"]["Second"], r["opened"]["Second"], "{r}");
    assert_eq!(r["opened"]["First"]["volume"], json!(2000.0), "{r}");
    let warnings = r["link_warnings"].to_string();
    assert!(warnings.contains("First keeps its bodies") && warnings.contains("Second keeps its bodies"), "each placement says so: {r}");
    let _ = std::fs::remove_dir_all(dir);
}

/// The same in one process, with the intermediate design still open from the last drive: the
/// second placement's drive left it with the plate at 12 mm, so the first placement asking 12 mm
/// would have taken those bodies without meeting the plate's file. Each drive starts from the
/// intermediate file's own design, so with the plate's file gone, or changed since the
/// intermediate design read it, the first placement keeps its own bodies and values and says so
/// (review of PR #1, loop 2).
#[test]
fn a_placement_never_takes_what_the_open_design_holds_from_another_placement() {
    for case in ["gone", "changed"] {
        let dir = folder(&format!("warm-nested-{case}"));
        let leaf = dir.join("leaf.sc");
        save_plate(&leaf);
        let middle = dir.join("middle.sc");
        let mut mid = Session::default();
        run(&mut mid, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
        run(&mut mid, "component.insert_design", json!({"path": leaf, "name": "Leaf", "parameters": {"h": "h"}}));
        run(&mut mid, "file.save", json!({"path": middle}));
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name": "a", "expression": "10 mm"}));
        run(&mut s, "parameters.add", json!({"name": "b", "expression": "12 mm"}));
        run(&mut s, "component.insert_design", json!({"path": middle, "name": "First", "parameters": {"h": "a"}}));
        run(&mut s, "component.insert_design", json!({"path": middle, "name": "Second", "parameters": {"h": "b"}}));
        let before = placements(&s);
        assert_eq!((before["First"]["volume"].clone(), before["Second"]["volume"].clone()), (json!(2000.0), json!(2400.0)), "{before}");
        if case == "gone" {
            std::fs::rename(&leaf, dir.join("leaf-moved.sc")).unwrap();
        } else {
            let mut wider = Session::default();
            run(&mut wider, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
            run(&mut wider, "solid.box", json!({"length": 10, "width": 40, "height": "h", "body_name": "Plate"}));
            run(&mut wider, "file.save", json!({"path": leaf}));
        }
        let changed = run(&mut s, "parameters.change", json!({"name": "a", "expression": "12 mm"}));
        assert_eq!(placements(&s), before, "{case}: each placement keeps its own bodies and values: {changed}");
        let warnings = changed["link_warnings"].to_string();
        assert!(warnings.contains("First keeps its bodies") && warnings.contains("Leaf keeps its bodies"), "{case}: {changed}");
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// The same for placements of a configuration of the intermediate design: activating it drives
/// the plate there, so a placement asking the configuration's own value must still meet the
/// plate's file as a new read activating it would, not the plate the open copy was activated
/// with (review of PR #1, loop 2, round 2).
#[test]
fn a_configured_placement_never_takes_what_the_open_design_was_activated_with() {
    for case in ["gone", "changed"] {
        let dir = folder(&format!("warm-configured-{case}"));
        let leaf = dir.join("leaf.sc");
        save_plate(&leaf);
        let middle = dir.join("middle.sc");
        let mut mid = Session::default();
        run(&mut mid, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
        run(&mut mid, "component.insert_design", json!({"path": leaf, "name": "Leaf", "parameters": {"h": "h"}}));
        run(&mut mid, "config.configure", json!({"params": ["h"]}));
        run(&mut mid, "config.row", json!({"name": "Eight", "values": {"h": "8 mm"}}));
        run(&mut mid, "file.save", json!({"path": middle}));
        let mut s = Session::default();
        run(&mut s, "parameters.add", json!({"name": "a", "expression": "10 mm"}));
        run(&mut s, "parameters.add", json!({"name": "b", "expression": "12 mm"}));
        run(&mut s, "component.insert_design", json!({"path": middle, "name": "First", "configuration": "Eight", "parameters": {"h": "a"}}));
        run(&mut s, "component.insert_design", json!({"path": middle, "name": "Second", "configuration": "Eight", "parameters": {"h": "b"}}));
        let before = placements(&s);
        assert_eq!((before["First"]["volume"].clone(), before["Second"]["volume"].clone()), (json!(2000.0), json!(2400.0)), "{before}");
        if case == "gone" {
            std::fs::rename(&leaf, dir.join("leaf-moved.sc")).unwrap();
        } else {
            let mut wider = Session::default();
            run(&mut wider, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
            run(&mut wider, "solid.box", json!({"length": 10, "width": 40, "height": "h", "body_name": "Plate"}));
            run(&mut wider, "file.save", json!({"path": leaf}));
        }
        // The configuration's own 8 mm.
        let changed = run(&mut s, "parameters.change", json!({"name": "a", "expression": "8 mm"}));
        assert_eq!(placements(&s), before, "{case}: each placement keeps its own bodies and values: {changed}");
        let warnings = changed["link_warnings"].to_string();
        assert!(warnings.contains("First keeps its bodies") && warnings.contains("Leaf keeps its bodies"), "{case}: {changed}");
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// `keep` is checked against the expressions the link has once `set` is applied: removing an
/// expression and keeping the name is one edit, and only a remaining expression for a kept
/// name is refused (review of #3).
#[test]
fn an_expression_moves_to_keep_its_own_in_one_edit() {
    let dir = folder("set-null-keep");
    let part = dir.join("plate.sc");
    save_plate(&part);
    let mut s = driven_assembly(&part);
    run(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": "lift"}}));
    assert!((volume(&mut s) - 2400.0).abs() < 1e-6);
    let refused = fails(&mut s, "component.link_parameters", json!({"component": "Plate", "keep": ["h"]}));
    assert!(refused.contains("both kept and given an expression"), "{refused}");
    let moved = run(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": null}, "keep": ["h"]}));
    assert_eq!((moved["keep"].clone(), moved["parameters"].clone()), (json!(["h"]), json!({})), "{moved}");
    assert!((volume(&mut s) - 1000.0).abs() < 1e-6, "the plate's own 5 mm");
    let _ = std::fs::remove_dir_all(dir);
}

/// An intermediate design that already has the value asked for, through a configuration or
/// because it was saved after an edit its link could not follow, still has a plate made with
/// another value when the plate's file is gone: inserting it with that value is refused and
/// names the plate's link, instead of reporting the value applied over the old plate (review of
/// PR #1, and #3's acceptance matrix).
#[test]
fn inserting_a_design_whose_link_inside_kept_its_bodies_is_refused() {
    let dir = folder("insert-stale");
    let leaf = dir.join("leaf.sc");
    save_plate(&leaf);
    let middle = dir.join("middle.sc");
    let mut mid = Session::default();
    run(&mut mid, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
    run(&mut mid, "component.insert_design", json!({"path": leaf, "name": "Leaf", "parameters": {"h": "h"}}));
    run(&mut mid, "config.configure", json!({"params": ["h"]}));
    run(&mut mid, "config.row", json!({"name": "Tall", "values": {"h": "10 mm"}}));
    run(&mut mid, "file.save", json!({"path": middle}));
    let insert = |path: &Path, configuration: Option<&str>| {
        let mut s = Session::default();
        let mut p = json!({"path": path, "name": "Middle", "parameters": {"h": "10 mm"}});
        if let Some(c) = configuration {
            p["configuration"] = json!(c);
        }
        let r = s.execute("component.insert_design", &p).map_err(|e| e.to_string());
        (r, volume(&mut s))
    };
    // With the plate there, the configured insert has a 10 mm plate.
    let control = dir.join("control.sc");
    std::fs::copy(&middle, &control).unwrap();
    let (r, v) = insert(&control, Some("Tall"));
    assert!(r.is_ok() && (v - 2000.0).abs() < 1e-6, "{r:?} {v}");

    std::fs::rename(&leaf, dir.join("leaf-moved.sc")).unwrap();
    // The configuration already sets 10 mm: the plate inside still is 5 mm.
    let (r, _) = insert(&middle, Some("Tall"));
    let refused = r.expect_err("the configured insert is refused");
    assert!(refused.contains("Leaf keeps its bodies") && refused.contains("leaf.sc"), "{refused}");

    // Saved after an edit the plate's link could not follow: 10 mm, with the 5 mm plate.
    let changed = run(&mut mid, "parameters.change", json!({"name": "h", "expression": "10 mm"}));
    assert!(changed["link_warnings"].to_string().contains("Leaf keeps its bodies"), "{changed}");
    let saved = dir.join("saved-stale.sc");
    run(&mut mid, "file.save", json!({"path": saved}));
    let (r, _) = insert(&saved, None);
    let refused = r.expect_err("the insert of the saved design is refused");
    assert!(refused.contains("Leaf keeps its bodies") && refused.contains("leaf.sc"), "{refused}");
    // Its own values, not driven, insert as the design was saved.
    let mut plain = Session::default();
    run(&mut plain, "component.insert_design", json!({"path": saved, "name": "Middle"}));

    // The plate's file back: the same insert now drives the plate's link inside and has a 10 mm
    // plate, though nothing asks the intermediate design for another value (review of PR #1:
    // it stayed refused once the plate was back).
    std::fs::rename(dir.join("leaf-moved.sc"), &leaf).unwrap();
    let (r, v) = insert(&saved, None);
    assert!(r.is_ok() && (v - 2000.0).abs() < 1e-6, "{r:?} {v}");
    let _ = std::fs::remove_dir_all(dir);
}

/// A part with a one-way rule (the hole goes when `Enable` is 0, and no rule brings it back): a
/// placement given 1 has its hole whatever another placement, or this one earlier, was given,
/// as a new read at 1 has (review of PR #1: a linked design kept open across drives kept the
/// hole switched off).
#[test]
fn a_rule_that_switches_a_feature_one_way_gives_each_drive_what_a_new_read_gives() {
    let dir = folder("one-way-rule");
    let part = dir.join("drilled.sc");
    let mut p = Session::default();
    run(&mut p, "parameters.add", json!({"name": "Enable", "expression": "1", "unit": ""}));
    run(&mut p, "parameters.add", json!({"name": "L", "expression": "10 mm"}));
    run(&mut p, "solid.box", json!({"length": "L", "width": 10, "height": 10, "body_name": "Cube"}));
    run(&mut p, "solid.hole", json!({"position": [5, 5, 10], "diameter": 4, "name": "Hole"}));
    let hole = p.doc.features.last().map_or(0, |f| f.id);
    run(&mut p, "config.configure", json!({"params": ["Enable"], "features": ["Hole"]}));
    run(
        &mut p,
        "config.rule",
        json!({"rule": {"id": "off", "name": "off", "enabled": true,
            "condition": {"type": "parameter", "parameter": "Enable", "operator": "eq", "expression": "0"},
            "effect": {"type": "drive_suppression", "feature": hole, "value": true}}}),
    );
    run(&mut p, "file.save", json!({"path": part}));
    let full = 1000.0;
    let drilled = full - std::f64::consts::PI * 4.0 * 10.0;
    let near = |a: f64, b: f64| (a - b).abs() < 0.5;
    for order in [["0", "1"], ["1", "0"]] {
        let mut s = Session::default();
        run(&mut s, "component.insert_design", json!({"path": part, "name": "A", "parameters": {"Enable": order[0]}}));
        run(&mut s, "component.insert_design", json!({"path": part, "name": "B", "parameters": {"Enable": order[1]}}));
        let placed = placements(&s);
        let want = |e: &str| if e == "1" { drilled } else { full };
        assert!(near(placed["A"]["volume"].as_f64().unwrap(), want(order[0])), "{order:?}: {placed}");
        assert!(near(placed["B"]["volume"].as_f64().unwrap(), want(order[1])), "{order:?}: {placed}");
    }
    // One placement driven to 0 and back to 1 has its hole again.
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "on", "expression": "1", "unit": ""}));
    run(&mut s, "component.insert_design", json!({"path": part, "name": "A", "parameters": {"Enable": "on"}}));
    run(&mut s, "parameters.change", json!({"name": "on", "expression": "0"}));
    assert!(near(placements(&s)["A"]["volume"].as_f64().unwrap(), full), "{}", placements(&s));
    run(&mut s, "parameters.change", json!({"name": "on", "expression": "1"}));
    assert!(near(placements(&s)["A"]["volume"].as_f64().unwrap(), drilled), "{}", placements(&s));
    // The design stays open between drives (review of PR #1, loop 2: it was read from its file
    // for each one): a drive that keeps its bodies and faces hands them over as records, and
    // the rule still sees each drive as a new read of the file would.
    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "on", "expression": "1", "unit": ""}));
    run(&mut s, "parameters.add", json!({"name": "len", "expression": "10 mm"}));
    run(&mut s, "component.insert_design", json!({"path": part, "name": "A", "parameters": {"Enable": "on", "L": "len"}}));
    let before = super::RECORD_READS.with(std::cell::Cell::get);
    for len in [11.0, 12.0] {
        run(&mut s, "parameters.change", json!({"name": "len", "expression": format!("{len} mm")}));
        let want = len * 100.0 - std::f64::consts::PI * 4.0 * 10.0;
        assert!(near(placements(&s)["A"]["volume"].as_f64().unwrap(), want), "{len}: {}", placements(&s));
    }
    assert_eq!(super::RECORD_READS.with(std::cell::Cell::get) - before, 2, "each drive after the first takes the open design's bodies");
    run(&mut s, "parameters.change", json!({"name": "on", "expression": "0"}));
    assert!(near(placements(&s)["A"]["volume"].as_f64().unwrap(), 1200.0), "{}", placements(&s));
    run(&mut s, "parameters.change", json!({"name": "len", "expression": "13 mm"}));
    assert!(near(placements(&s)["A"]["volume"].as_f64().unwrap(), 1300.0), "the rule switched the hole off for this drive too: {}", placements(&s));
    run(&mut s, "parameters.change", json!({"name": "on", "expression": "1"}));
    let drilled = 1300.0 - std::f64::consts::PI * 4.0 * 10.0;
    assert!(near(placements(&s)["A"]["volume"].as_f64().unwrap(), drilled), "{}", placements(&s));
    let _ = std::fs::remove_dir_all(dir);
}

/// A link's expression that reads a feature input follows a feature edit, and a by-name link
/// follows a rename that gives this design a parameter of the matching name: both in the
/// command's own undo step (review of PR #1: only value edits drove the links).
#[test]
fn a_feature_edit_or_a_rename_that_changes_what_a_link_passes_drives_it() {
    let dir = folder("feature-edit");
    let part = dir.join("plate.sc");
    save_plate(&part);
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 12, "corner": [50, 0, 0], "body_name": "Post"}));
    let post = s.doc.features[0].id;
    let height = s.doc.features[0].param_names[2].clone();
    run(&mut s, "component.insert_design", json!({"path": part, "name": "Plate", "parameters": {"h": height}}));
    assert_eq!(placements(&s)["Plate"]["volume"], json!(2400.0));
    run(&mut s, "timeline.edit", json!({"feature": post, "set": {"height": 7}}));
    assert_eq!(placements(&s)["Plate"]["volume"], json!(1400.0), "the plate follows the post's height");
    assert_eq!(s.doc.links.values().next().unwrap().parameter_values.get("h").map(String::as_str), Some("7 mm"));
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(placements(&s)["Plate"]["volume"], json!(2400.0), "one undo step takes both back");

    let mut named = Session::default();
    run(&mut named, "parameters.add", json!({"name": "rise", "expression": "8 mm"}));
    run(&mut named, "component.insert_design", json!({"path": part, "name": "Plate", "by_name": true}));
    assert_eq!(placements(&named)["Plate"]["volume"], json!(1000.0), "no `h` here: the plate's own 5 mm");
    run(&mut named, "parameters.rename", json!({"name": "rise", "new_name": "h"}));
    assert_eq!(placements(&named)["Plate"]["volume"], json!(1600.0), "renamed to `h`, it is passed at once");
    run(&mut named, "edit.undo", json!({}));
    assert_eq!(placements(&named)["Plate"]["volume"], json!(1000.0));
    assert!(named.doc.links.values().next().unwrap().parameter_values.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

/// The plate's `h` renamed `height` on disk: Update Links can't take the file in with the link's
/// `h`, and Link Parameters refuses a changed file unasked; with `update` it takes the file and
/// the new setting in one step, keeping the component, its placement and its Import feature
/// (review of PR #1: nothing could repair such a link in place).
#[test]
fn a_parameter_renamed_in_the_linked_file_is_repaired_with_update() {
    let dir = folder("source-rename");
    let part = dir.join("plate.sc");
    save_plate(&part);
    let mut s = driven_assembly(&part);
    run(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": "lift"}}));
    let (comp, feature) = s.doc.links.iter().next().map(|(c, l)| (*c, l.feature)).unwrap();
    let occurrence = s.doc.occurrences.iter().find(|o| o.component == comp).unwrap().id;
    let mut edited = Session::default();
    run(&mut edited, "doc.open", json!({"path": part}));
    run(&mut edited, "parameters.rename", json!({"name": "h", "new_name": "height"}));
    run(&mut edited, "file.save", json!({"path": part}));

    let update = fails(&mut s, "component.update_links", json!({"component": "Plate"}));
    assert!(update.contains("`h`"), "{update}");
    let unasked = fails(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": null, "height": "lift"}}));
    assert!(unasked.contains("changed since this design last read it") && unasked.contains("`update`"), "{unasked}");
    let repaired = run(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": null, "height": "lift"}, "update": true}));
    assert_eq!(repaired["parameters"], json!({"height": {"expression": "lift", "value": 12.0}}), "{repaired}");
    let link = s.doc.links.get(&comp).expect("the same component").clone();
    assert_eq!(link.feature, feature);
    assert!(s.doc.occurrences.iter().any(|o| o.id == occurrence && o.component == comp));
    assert_eq!(link.file_sha256.as_deref(), Some(solidcraft_doc::DesignLink::file_digest(&std::fs::read(&part).unwrap()).as_str()));
    assert!((volume(&mut s) - 2400.0).abs() < 1e-6);
    let changed = run(&mut s, "parameters.change", json!({"name": "lift", "expression": "7 mm"}));
    assert_eq!(changed["linked"], json!(["Plate"]), "{changed}");
    assert!((volume(&mut s) - 1400.0).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(dir);
}

/// A link made before links kept their file's digest: setting its parameters records the bytes
/// its bodies came from, so a later change to the file is not taken in by a value edit (review
/// of PR #1).
#[test]
fn setting_a_legacy_links_parameters_records_the_file_it_read() {
    let dir = folder("legacy-digest");
    let part = dir.join("plate.sc");
    save_plate(&part);
    let mut s = driven_assembly(&part);
    for link in s.doc_mut().links.values_mut() {
        link.file_sha256 = None;
    }
    run(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": "lift"}}));
    let read = solidcraft_doc::DesignLink::file_digest(&std::fs::read(&part).unwrap());
    assert_eq!(s.doc.links.values().next().unwrap().file_sha256.as_deref(), Some(read.as_str()));
    let mut edited = Session::default();
    run(&mut edited, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
    run(&mut edited, "solid.box", json!({"length": 10, "width": 40, "height": "h", "body_name": "Plate"}));
    run(&mut edited, "file.save", json!({"path": part}));
    let changed = run(&mut s, "parameters.change", json!({"name": "lift", "expression": "7 mm"}));
    assert!(changed.get("linked").is_none(), "{changed}");
    assert!(changed["link_warnings"].to_string().contains("Update Links"), "{changed}");
    assert!((volume(&mut s) - 2400.0).abs() < 1e-6, "the 20 wide plate it had");
    let _ = std::fs::remove_dir_all(dir);
}

/// A link with no record of the file it read (saved before links kept a digest, or written by
/// hand) is not known to hold the file's bytes: a value edit keeps its bodies and says why,
/// Update Links records the file, and the next edit drives it (upstream review of #20542).
#[test]
fn a_value_edit_never_drives_a_link_with_no_record_of_its_file() {
    let dir = folder("no-digest");
    let part = dir.join("plate.sc");
    save_plate(&part);
    let mut s = driven_assembly(&part);
    run(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": "lift"}}));
    assert!((volume(&mut s) - 2400.0).abs() < 1e-6);
    for link in s.doc_mut().links.values_mut() {
        link.file_sha256 = None;
    }
    let kept = run(&mut s, "parameters.change", json!({"name": "lift", "expression": "7 mm"}));
    assert!(kept.get("linked").is_none(), "{kept}");
    let why = kept["link_warnings"][0].as_str().unwrap_or("");
    assert!(
        why.contains("Plate keeps its bodies") && why.contains("has no record of what this design last read from it; Update Links reads it"),
        "{kept}"
    );
    assert!((volume(&mut s) - 2400.0).abs() < 1e-6, "the link keeps its bodies");
    assert!(s.doc.links.values().all(|l| l.file_sha256.is_none()), "a value edit records nothing");

    // Update Links reads the file with the link's values (10 × 20 × 7) and records it.
    run(&mut s, "component.update_links", json!({}));
    let read = solidcraft_doc::DesignLink::file_digest(&std::fs::read(&part).unwrap());
    assert_eq!(s.doc.links.values().next().unwrap().file_sha256.as_deref(), Some(read.as_str()));
    assert!((volume(&mut s) - 1400.0).abs() < 1e-6);
    let driven = run(&mut s, "parameters.change", json!({"name": "lift", "expression": "9 mm"}));
    assert_eq!(driven["linked"], json!(["Plate"]), "{driven}");
    assert!((volume(&mut s) - 1800.0).abs() < 1e-6);
    let _ = std::fs::remove_dir_all(dir);
}

/// Saving without linked bodies (`linked_bodies: false`) keeps the bodies of a link that passes
/// values, and says so: reading the file again as it is would give the design's own sizes.
#[test]
fn a_save_without_linked_bodies_keeps_the_bodies_of_a_driven_link() {
    let dir = folder("driven-left-out");
    let part = dir.join("plate.sc");
    save_plate(&part);
    let mut s = driven_assembly(&part);
    run(&mut s, "component.link_parameters", json!({"component": "Plate", "set": {"h": "lift"}}));
    let file = dir.join("assembly.sc");
    let saved = run(&mut s, "file.save", json!({"path": file, "linked_bodies": false}));
    assert!(saved.to_string().contains("passes it parameter values"), "{saved}");
    let mut opened = Session::default();
    run(&mut opened, "doc.open", json!({"path": file}));
    assert!((volume(&mut opened) - 2400.0).abs() < 1e-6, "the plate at the 12 mm its link passes, not its own 5 mm");
    let _ = std::fs::remove_dir_all(dir);
}

/// A value that brings in a body the design had switched off: the body comes with its own
/// material, as it does when the design is inserted with that value (review of PR #1: a value
/// edit's drive left it on the default material).
#[test]
fn a_body_a_value_brings_in_comes_with_its_material() {
    let dir = folder("material");
    let part = dir.join("two.sc");
    let mut p = Session::default();
    run(&mut p, "parameters.add", json!({"name": "Enable", "expression": "1", "unit": ""}));
    run(&mut p, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "Base"}));
    run(&mut p, "solid.box", json!({"length": 10, "width": 10, "height": 10, "corner": [30, 0, 0], "body_name": "Brick", "name": "BrickBox"}));
    run(&mut p, "material.assign", json!({"bodies": ["Brick"], "material": "Brass"}));
    let brick = p.doc.features.last().map_or(0, |f| f.id);
    run(&mut p, "config.configure", json!({"params": ["Enable"], "features": ["BrickBox"]}));
    for (id, when, off) in [("off", "0", true), ("on", "1", false)] {
        run(
            &mut p,
            "config.rule",
            json!({"rule": {"id": id, "name": id, "enabled": true,
                "condition": {"type": "parameter", "parameter": "Enable", "operator": "eq", "expression": when},
                "effect": {"type": "drive_suppression", "feature": brick, "value": off}}}),
        );
    }
    run(&mut p, "parameters.change", json!({"name": "Enable", "expression": "0"}));
    run(&mut p, "file.save", json!({"path": part}));
    let mass = |s: &mut Session| {
        let comp = *s.doc.links.keys().next().unwrap();
        run(s, "component.properties", json!({"component": comp}))["total"]["mass_g"].as_f64().unwrap()
    };
    let mut fresh = Session::default();
    run(&mut fresh, "component.insert_design", json!({"path": part, "name": "Two", "parameters": {"Enable": "1"}}));
    let both = mass(&mut fresh);

    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "on", "expression": "0", "unit": ""}));
    run(&mut s, "component.insert_design", json!({"path": part, "name": "Two", "parameters": {"Enable": "on"}}));
    let one = mass(&mut s);
    assert!(both > one * 2.0, "brass weighs more than the default: {both} {one}");
    run(&mut s, "parameters.change", json!({"name": "on", "expression": "1"}));
    assert!((mass(&mut s) - both).abs() < 1e-6, "{} {both}", mass(&mut s));
    let _ = std::fs::remove_dir_all(dir);
}

/// One of two placements of a driven plate relinked to another plate: it goes on taking the
/// values its link passed, and a plate that can't take them is refused with the placement left
/// as it was (review of PR #1: it took the new plate's own values and stopped following).
#[test]
fn relinking_one_placement_keeps_the_values_its_link_passes() {
    let dir = folder("relink-one");
    let part = dir.join("plate.sc");
    save_plate(&part);
    let wide = dir.join("wide.sc");
    let mut w = Session::default();
    run(&mut w, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
    run(&mut w, "solid.box", json!({"length": 10, "width": 40, "height": "h", "body_name": "Plate"}));
    run(&mut w, "file.save", json!({"path": wide}));
    let plain = dir.join("plain.sc");
    let mut b = Session::default();
    run(&mut b, "solid.box", json!({"length": 10, "width": 40, "height": 5, "body_name": "Plate"}));
    run(&mut b, "file.save", json!({"path": plain}));

    let mut s = Session::default();
    run(&mut s, "parameters.add", json!({"name": "lift", "expression": "12 mm"}));
    let first = run(&mut s, "component.insert_design", json!({"path": part, "name": "Plate", "parameters": {"h": "lift"}}));
    let second = run(&mut s, "component.insert_design", json!({"path": part, "reuse": true, "parameters": {"h": "lift"}}));
    assert_eq!(first["component"], second["component"], "the same link, placed twice");
    let refused = fails(&mut s, "component.relink", json!({"component": "Plate", "occurrence": second["occurrence"], "path": plain}));
    assert!(refused.contains("`h`"), "{refused}");
    assert_eq!(s.doc.links.len(), 1, "nothing changed");
    run(&mut s, "component.relink", json!({"component": "Plate", "occurrence": second["occurrence"], "path": wide}));
    assert!((volume(&mut s) - (2400.0 + 4800.0)).abs() < 1e-6, "{}", volume(&mut s));
    let changed = run(&mut s, "parameters.change", json!({"name": "lift", "expression": "7 mm"}));
    assert_eq!(changed["linked"].as_array().map(Vec::len), Some(2), "{changed}");
    assert!((volume(&mut s) - (1400.0 + 2800.0)).abs() < 1e-6, "{}", volume(&mut s));
    let _ = std::fs::remove_dir_all(dir);
}

/// An intermediate design whose by-name link to the plate couldn't take a new `h` (the plate's
/// file was gone when `h` was added), saved, then the plate's file back: inserting it with the
/// `h` it already has drives the plate's link inside and gives the 10 mm plate (review of PR
/// #1, round 2: a by-name match the link had never passed went unseen, and the insert kept the
/// 5 mm plate while reporting 10 mm).
#[test]
fn a_by_name_match_a_link_inside_never_passed_is_driven_when_its_file_is_back() {
    let dir = folder("nested-by-name");
    let leaf = dir.join("leaf.sc");
    save_plate(&leaf);
    let middle = dir.join("middle.sc");
    let mut mid = Session::default();
    run(&mut mid, "component.insert_design", json!({"path": leaf, "name": "Leaf", "by_name": true}));
    std::fs::rename(&leaf, dir.join("leaf-moved.sc")).unwrap();
    run(&mut mid, "parameters.add", json!({"name": "h", "expression": "10 mm"}));
    run(&mut mid, "file.save", json!({"path": middle}));
    std::fs::rename(dir.join("leaf-moved.sc"), &leaf).unwrap();

    let mut s = Session::default();
    let inserted = run(&mut s, "component.insert_design", json!({"path": middle, "name": "Middle", "parameters": {"h": "10 mm"}}));
    assert_eq!(inserted["parameters"]["h"]["value"], json!(10.0), "{inserted}");
    assert!((volume(&mut s) - 2000.0).abs() < 1e-6, "the plate at 10 mm, as reported: {}", volume(&mut s));
    let _ = std::fs::remove_dir_all(dir);
}

/// A command made of several (a plugin's, say) whose value edit came before a feature edit that
/// changes what a link is passed: the links follow the final state when the command ends, and
/// what they did is in its reply; a state they already followed drives nothing (review of PR #1,
/// round 2: an earlier drive in the same command stood for the whole command).
#[test]
fn the_links_follow_the_state_a_command_of_several_steps_ends_with() {
    let dir = folder("compound");
    let part = dir.join("plate.sc");
    save_plate(&part);
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 12, "corner": [50, 0, 0], "body_name": "Post"}));
    let post = s.doc.features[0].id;
    let height = s.doc.features[0].param_names[2].clone();
    run(&mut s, "component.insert_design", json!({"path": part, "name": "Plate", "parameters": {"h": height}}));
    assert_eq!(placements(&s)["Plate"]["volume"], json!(2400.0));
    {
        // The enclosing command is the outermost one, so its steps run as nested commands do.
        let (_inside, outermost) = crate::frames::Nested::enter("plugin.run");
        assert!(outermost);
        run(&mut s, "parameters.add", json!({"name": "unrelated", "expression": "3 mm"}));
        run(&mut s, "timeline.edit", json!({"feature": post, "set": {"height": 7}}));
    }
    assert_eq!(placements(&s)["Plate"]["volume"], json!(2400.0), "inside the command, nothing followed the feature edit yet");
    let (linked, warnings) = super::follow_command(&mut s).unwrap();
    assert_eq!((linked, warnings), (vec!["Plate".to_string()], Vec::<String>::new()));
    s.refresh();
    assert_eq!(placements(&s)["Plate"]["volume"], json!(1400.0), "the command ends with the plate at the post's height");
    assert_eq!(super::follow_command(&mut s).unwrap(), (Vec::new(), Vec::new()), "followed: nothing more to drive");
    let _ = std::fs::remove_dir_all(dir);
}

/// A feature edit that changes what a link is passed when the link can't follow (its file gone,
/// or changed since this design read it): the edit is made, the link keeps its bodies, and the
/// edit's reply says so as a value edit's does (review of PR #1, round 2: only `parameters.change`
/// said it).
#[test]
fn a_feature_edit_a_link_cannot_follow_says_so_in_its_reply() {
    let dir = folder("feature-edit-kept");
    let part = dir.join("plate.sc");
    save_plate(&part);
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 12, "corner": [50, 0, 0], "body_name": "Post"}));
    let post = s.doc.features[0].id;
    let height = s.doc.features[0].param_names[2].clone();
    run(&mut s, "component.insert_design", json!({"path": part, "name": "Plate", "parameters": {"h": height}}));
    std::fs::rename(&part, dir.join("plate-moved.sc")).unwrap();
    let edited = run(&mut s, "timeline.edit", json!({"feature": post, "set": {"height": 7}}));
    assert!(edited["link_warnings"].to_string().contains("Plate keeps its bodies"), "{edited}");
    assert_eq!(placements(&s)["Plate"]["volume"], json!(2400.0));

    // Back, but changed since this design read it: the same, with Update Links named.
    std::fs::rename(dir.join("plate-moved.sc"), &part).unwrap();
    let mut wider = Session::default();
    run(&mut wider, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
    run(&mut wider, "solid.box", json!({"length": 10, "width": 40, "height": "h", "body_name": "Plate"}));
    run(&mut wider, "file.save", json!({"path": part}));
    let edited = run(&mut s, "timeline.edit", json!({"feature": post, "set": {"height": 8}}));
    assert!(edited["link_warnings"].to_string().contains("Update Links"), "{edited}");
    assert_eq!(placements(&s)["Plate"]["volume"], json!(2400.0));
    let _ = std::fs::remove_dir_all(dir);
}

/// The intermediate design whose by-name link couldn't take its new `h` (the plate's file gone
/// when `h` was added), inserted with that `h` in a fresh process (no design or parameter read
/// kept from this one) while the plate's file is still gone, and while it is back but changed:
/// each insert is refused and names the plate's link, instead of reporting 10 mm over the 5 mm
/// plate (review of PR #1, round 3: with nothing read yet, an unreadable file counted as no
/// match). The link records the plate's parameters (`parameter_units`), so the match is known.
#[test]
fn a_by_name_match_inside_is_known_without_the_linked_file() {
    let dir = folder("nested-by-name-gone");
    let leaf = dir.join("leaf.sc");
    save_plate(&leaf);
    let middle = dir.join("middle.sc");
    let mut mid = Session::default();
    run(&mut mid, "component.insert_design", json!({"path": leaf, "name": "Leaf", "by_name": true}));
    let recorded = mid.doc.links.values().next().unwrap().parameter_units.clone();
    assert_eq!(recorded, std::collections::BTreeMap::from([("h".to_string(), "mm".to_string())]), "the plate's parameters, as read");
    std::fs::rename(&leaf, dir.join("leaf-moved.sc")).unwrap();
    let added = run(&mut mid, "parameters.add", json!({"name": "h", "expression": "10 mm"}));
    assert!(added["link_warnings"].to_string().contains("Leaf keeps its bodies"), "the match is found and said: {added}");
    run(&mut mid, "file.save", json!({"path": middle}));

    let insert = || -> Value {
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "cmd::component_link::parameters::tests::fresh_process_insert", "--ignored", "--nocapture", "--test-threads=1"])
            .env("SOLIDCRAFT_LINK_FRESH_INSERT", &middle)
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&child.stdout);
        let printed = out
            .lines()
            .find_map(|l| l.split_once("INSERT ").map(|(_, f)| f))
            .unwrap_or_else(|| panic!("the child printed nothing: {out}\n{}", String::from_utf8_lossy(&child.stderr)));
        serde_json::from_str(printed).unwrap()
    };
    let gone = insert();
    assert!(gone["error"].as_str().is_some_and(|e| e.contains("Leaf keeps its bodies") && e.contains("leaf.sc")), "{gone}");

    let mut wider = Session::default();
    run(&mut wider, "parameters.add", json!({"name": "h", "expression": "5 mm"}));
    run(&mut wider, "solid.box", json!({"length": 10, "width": 40, "height": "h", "body_name": "Plate"}));
    run(&mut wider, "file.save", json!({"path": leaf}));
    let changed = insert();
    assert!(changed["error"].as_str().is_some_and(|e| e.contains("Leaf keeps its bodies") && e.contains("Update Links")), "{changed}");
    let _ = std::fs::remove_dir_all(dir);
}

/// The child half of `a_by_name_match_inside_is_known_without_the_linked_file`: inserts the
/// design at `SOLIDCRAFT_LINK_FRESH_INSERT` with `h = 10 mm` and prints the outcome.
#[test]
#[ignore = "run in a child process by a_by_name_match_inside_is_known_without_the_linked_file"]
fn fresh_process_insert() {
    let Ok(path) = std::env::var("SOLIDCRAFT_LINK_FRESH_INSERT") else { return };
    let mut s = Session::default();
    let outcome = match s.execute("component.insert_design", &json!({"path": path, "name": "Middle", "parameters": {"h": "10 mm"}})) {
        Ok(v) => json!({"inserted": v, "volume": volume(&mut s)}),
        Err(e) => json!({"error": e.to_string()}),
    };
    println!("INSERT {outcome}");
}
