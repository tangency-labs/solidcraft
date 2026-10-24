use std::f64::consts::PI;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn err(s: &mut Session, id: &str, p: Value) -> String {
    match s.execute(id, &p) {
        Ok(v) => panic!("{id} {p} should fail, gave {v}"),
        Err(e) => e.to_string(),
    }
}

fn volume(s: &mut Session) -> f64 {
    run(s, "inspect.measure", json!({}))["total"]["volume_mm3"].as_f64().unwrap_or(f64::NAN)
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-6 * b.abs().max(1.0)
}

/// A measured volume against an analytic one: the measure is within 0.1% of a cylinder's volume,
/// while the changes these tests make move it by 5% or more.
fn near(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-3 * b.abs()
}

fn row(s: &mut Session, name: &str) -> Value {
    let l = run(s, "parameters.list", json!({}));
    l["parameters"].as_array().and_then(|a| a.iter().find(|p| p["name"] == name).cloned()).unwrap_or(Value::Null)
}

fn state(s: &mut Session) -> Value {
    run(s, "parameters.links", json!({}))["designs"][0].clone()
}

/// A fresh folder for one test, with `std/` and `parts/` in it.
fn folder(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("solidcraft-param-link-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("std")).unwrap();
    std::fs::create_dir_all(dir.join("parts")).unwrap();
    dir
}

/// The linked designs as the design file at `file` keeps them.
fn saved_paths(file: &Path) -> Vec<String> {
    let doc = solidcraft_io::read_design(&std::fs::read(file).unwrap()).unwrap();
    doc.linked_designs.into_iter().map(|d| d.path).collect()
}

/// Rows of a standards design's parameter table (CSV as `parameters.import` reads it): plain
/// values, expressions of other rows and an angle.
const STANDARD: &str = "name,unit,expression,comment
Clr_Slide,mm,0.20 mm,\"Sliding fit per side (keys, cradles)\"
Key_W,mm,4.0 mm,R/X divot width
Key_Ang,deg,45 deg,Ramp angle on key ends
KeyM_W,mm,Key_W - 2 * Clr_Slide,R/X male key width
Q_InsHole,mm,8.2 mm,1/4-20 insert hole diameter
Q_InsLen,mm,9.5 mm,1/4-20 insert length
Q_InsDepth,mm,Q_InsLen + 1.4 mm,1/4-20 insert hole depth
";

/// The standards design, saved at `path`, still open for editing.
fn standard(path: &Path) -> Session {
    let mut s = Session::default();
    run(&mut s, "parameters.import", json!({"text": STANDARD}));
    run(&mut s, "file.save", json!({"path": path}));
    s
}

/// A design linking all of the standard at `std_file`, with an insert hole's worth of material:
/// a cylinder Q_InsHole across and Q_InsDepth tall.
fn linked_part(std_file: &Path) -> Session {
    let mut part = Session::default();
    run(&mut part, "parameters.link_design", json!({"path": std_file}));
    run(&mut part, "solid.cylinder", json!({"diameter": "Q_InsHole", "height": "Q_InsDepth"}));
    part
}

fn insert_volume(hole: f64, depth: f64) -> f64 {
    PI * (hole / 2.0).powi(2) * depth
}

#[test]
fn a_design_uses_the_standards_parameters_and_follows_them_on_update() {
    let dir = folder("follow");
    let std_file = dir.join("std").join("CC-STD-Interface.solidcraft");
    let part_file = dir.join("parts").join("core.solidcraft");
    let mut std = standard(&std_file);
    // Given a design that links the standard (all its user parameters, by default).
    let mut part = Session::default();
    run(&mut part, "file.save", json!({"path": part_file}));
    let linked = run(&mut part, "parameters.link_design", json!({"path": std_file}));
    assert_eq!((linked["design"].as_str(), linked["scope"].as_str()), (Some("../std/CC-STD-Interface.solidcraft"), Some("all")), "{linked}");
    assert_eq!(linked["parameters"].as_array().map(Vec::len), Some(7), "{linked}");
    run(&mut part, "solid.cylinder", json!({"diameter": "Q_InsHole", "height": "Q_InsDepth"}));
    assert!(near(volume(&mut part), insert_volume(8.2, 10.9)), "{}", volume(&mut part));
    // The linked rows carry their values in their own units, worked out in the standard.
    let r = row(&mut part, "KeyM_W");
    assert_eq!((r["source"].as_str(), r["expression"].as_str()), (Some("linked"), Some("3.6 mm")), "{r}");
    let r = row(&mut part, "Key_Ang");
    assert!(r["expression"] == "45 deg" && close(r["value"].as_f64().unwrap_or(f64::NAN), 45.0), "{r}");
    assert_eq!(state(&mut part)["state"], "up_to_date");
    // When the standard's insert hole grows, the design says so and takes it on update.
    run(&mut std, "parameters.change", json!({"name": "Q_InsHole", "expression": "8.5 mm"}));
    run(&mut std, "file.save", json!({"path": std_file}));
    let st = state(&mut part);
    assert_eq!((st["state"].as_str(), &st["changed"]), (Some("out_of_date"), &json!(["Q_InsHole"])), "{st}");
    assert!(near(volume(&mut part), insert_volume(8.2, 10.9)), "the values stay until the design is updated");
    let up = run(&mut part, "parameters.update_links", json!({}));
    assert_eq!(up["updated"][0]["changed"], json!([{"name": "Q_InsHole", "from": "8.2 mm", "to": "8.5 mm"}]), "{up}");
    assert!(near(volume(&mut part), insert_volume(8.5, 10.9)), "{}", volume(&mut part));
    assert_eq!(state(&mut part)["state"], "up_to_date");
    assert_eq!(run(&mut part, "parameters.update_links", json!({}))["unchanged"], json!(["../std/CC-STD-Interface.solidcraft"]));
    // An expression change upstream arrives as its new value; undo takes it back.
    run(&mut std, "parameters.change", json!({"name": "Q_InsLen", "expression": "8 mm"}));
    run(&mut std, "file.save", json!({"path": std_file}));
    run(&mut part, "parameters.update_links", json!({"design": "CC-STD-Interface"}));
    assert!(near(volume(&mut part), insert_volume(8.5, 9.4)), "{}", volume(&mut part));
    run(&mut part, "edit.undo", json!({}));
    assert!(near(volume(&mut part), insert_volume(8.5, 10.9)), "{}", volume(&mut part));
    // Linked parameters are listed as their own kind, not as model parameters.
    let model = run(&mut part, "parameters.list", json!({"filter": "model"}));
    assert!(model["parameters"].as_array().unwrap().iter().all(|r| r["source"] != "linked"), "{model}");
    assert_eq!(run(&mut part, "parameters.list", json!({"filter": "linked"}))["count"], 7);
    assert_eq!(run(&mut part, "parameters.users", json!({"name": "Q_InsHole"}))["users"].as_array().map(Vec::len), Some(1));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every command that edits a parameter refuses a linked one: refused, the linked design's file
/// named, the design unchanged and no undo step.
#[test]
fn every_editing_command_refuses_a_linked_parameter() {
    let dir = folder("read-only");
    let std_file = dir.join("std").join("standard.solidcraft");
    standard(&std_file);
    let mut part = linked_part(&std_file);
    run(&mut part, "parameters.add", json!({"name": "Local", "expression": "Q_InsHole * 2"}));
    // A feature whose input is named after a linked parameter, as `timeline.add` would bring it.
    let mut feature = serde_json::to_value(&part.doc.features[0]).unwrap();
    feature["id"] = json!(0);
    feature["name"] = json!("Copy");
    feature["param_names"] = json!(["Q_InsHole", "Fresh"]);
    let cases = [
        ("parameters.change", json!({"name": "Q_InsHole", "expression": "9 mm"})),
        ("parameters.change", json!({"changes": {"Q_InsHole": "9 mm"}})),
        ("parameters.change", json!({"name": "Q_InsHole", "new_name": "Hole"})),
        ("parameters.change", json!({"delete": "Q_InsHole"})),
        ("parameters.add", json!({"name": "Q_InsHole", "expression": "9 mm"})),
        ("parameters.rename", json!({"name": "Q_InsHole", "new_name": "Hole"})),
        ("parameters.delete", json!({"name": "Q_InsHole"})),
        ("parameters.comment", json!({"name": "Q_InsHole", "comment": "mine"})),
        ("parameters.move", json!({"name": "Q_InsHole", "before": "Local"})),
        ("parameters.move", json!({"name": "Local", "after": "Q_InsHole"})),
        ("parameters.import", json!({"text": "name,expression\nQ_InsHole,9 mm\n"})),
        ("config.configure", json!({"params": ["Q_InsHole"]})),
    ];
    for (id, p) in cases {
        let (before, undo) = (part.doc.clone(), part.undo.len());
        let e = err(&mut part, id, p.clone());
        assert!(e.contains("linked design") && e.contains("standard.solidcraft"), "{id} {p}: {e}");
        assert!(part.doc == before && part.undo.len() == undo, "{id} {p} changed the design");
    }
    // Taking a linked name for something else is refused too, as any name in use.
    for (id, p) in [
        ("parameters.rename", json!({"name": "Local", "new_name": "Key_W"})),
        ("timeline.add", json!({"feature": feature})),
        ("timeline.add", json!({"feature": {"kind": {"type": "box"}}, "params": [{"name": "Key_W", "expression": "1 mm", "unit": "mm"}]})),
    ] {
        let (before, undo) = (part.doc.clone(), part.undo.len());
        assert!(part.execute(id, &p).is_err(), "{id} {p}");
        assert!(part.doc == before && part.undo.len() == undo, "{id} {p} changed the design");
    }
    // A linked parameter can still be a favourite, and local parameters use it.
    run(&mut part, "parameters.favorite", json!({"name": "Q_InsHole"}));
    assert!(close(row(&mut part, "Local")["value"].as_f64().unwrap_or(f64::NAN), 16.4));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn favourites_come_in_alone_when_asked_for() {
    let dir = folder("favourites");
    let std_file = dir.join("std").join("standard.solidcraft");
    let mut std = standard(&std_file);
    let mut part = Session::default();
    let e = err(&mut part, "parameters.link_design", json!({"path": std_file, "scope": "favorites"}));
    assert!(e.contains("favourite") && e.contains("all"), "{e}");
    for name in ["Q_InsHole", "Q_InsDepth"] {
        run(&mut std, "parameters.favorite", json!({"name": name}));
    }
    run(&mut std, "file.save", json!({"path": std_file}));
    let linked = run(&mut part, "parameters.link_design", json!({"path": std_file, "scope": "favorites"}));
    assert_eq!(linked["parameters"], json!(["Q_InsHole", "Q_InsDepth"]), "{linked}");
    // Its value is worked out in the standard, from a parameter that doesn't come in.
    assert_eq!(row(&mut part, "Q_InsDepth")["expression"], "10.9 mm");
    assert_eq!(row(&mut part, "Q_InsLen"), Value::Null);
    run(&mut std, "parameters.favorite", json!({"name": "Key_W"}));
    run(&mut std, "file.save", json!({"path": std_file}));
    assert_eq!(run(&mut part, "parameters.update_links", json!({}))["updated"][0]["added"], json!(["Key_W"]));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_name_this_design_already_uses_refuses_the_link_and_the_update() {
    let dir = folder("clash");
    let std_file = dir.join("std").join("standard.solidcraft");
    let mut std = standard(&std_file);
    let mut part = Session::default();
    run(&mut part, "parameters.add", json!({"name": "Q_InsHole", "expression": "7 mm"}));
    run(&mut part, "parameters.add", json!({"name": "Wall", "expression": "2 mm"}));
    let before = part.doc.clone();
    let e = err(&mut part, "parameters.link_design", json!({"path": std_file}));
    assert!(e.contains("Q_InsHole") && !e.contains("Key_W"), "{e}");
    assert_eq!(part.doc, before);
    // Once linked, a new parameter there with a name used here keeps the design as it was.
    run(&mut part, "parameters.delete", json!({"name": "Q_InsHole"}));
    run(&mut part, "parameters.link_design", json!({"path": std_file}));
    run(&mut std, "parameters.add", json!({"name": "Wall", "expression": "3 mm"}));
    run(&mut std, "file.save", json!({"path": std_file}));
    let before = part.doc.clone();
    let e = err(&mut part, "parameters.update_links", json!({}));
    assert!(e.contains("Wall"), "{e}");
    assert_eq!(part.doc, before);
    // A second linked design may not bring a name the first one has.
    let other = dir.join("std").join("other.solidcraft");
    standard(&other);
    let e = err(&mut part, "parameters.link_design", json!({"path": other}));
    assert!(e.contains("Q_InsHole"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_parameter_in_use_cant_vanish_on_update() {
    let dir = folder("vanish");
    let std_file = dir.join("std").join("standard.solidcraft");
    let mut std = standard(&std_file);
    let mut part = linked_part(&std_file);
    run(&mut std, "parameters.rename", json!({"name": "Q_InsHole", "new_name": "Q_Hole"}));
    run(&mut std, "file.save", json!({"path": std_file}));
    let before = part.doc.clone();
    let e = err(&mut part, "parameters.update_links", json!({}));
    assert!(e.contains("Q_InsHole") && e.contains("no longer") && e.contains("renamed"), "{e}");
    assert_eq!(part.doc, before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_design_file_keeps_relative_paths_and_opens_without_its_linked_design() {
    let dir = folder("missing");
    let std_file = dir.join("std").join("standard.solidcraft");
    let part_file = dir.join("parts").join("core.solidcraft");
    standard(&std_file);
    // Linked before the design has a file: the file keeps the path relative to where it's saved.
    let mut part = linked_part(&std_file);
    run(&mut part, "file.save", json!({"path": part_file}));
    assert_eq!(saved_paths(&part_file), ["../std/standard.solidcraft"]);
    // Reopened, the design is as it was; opening checks only that the file is there.
    let mut back = Session::default();
    let opened = run(&mut back, "doc.open", json!({"path": part_file}));
    assert!(opened.get("warnings").is_none(), "{opened}");
    assert!(near(volume(&mut back), insert_volume(8.2, 10.9)));
    assert_eq!(state(&mut back)["state"], "up_to_date");
    // A copy of the whole folder elsewhere (another computer's synced folder) finds its standard.
    let moved = std::env::temp_dir().join(format!("solidcraft-param-link-moved-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&moved);
    for sub in ["std", "parts"] {
        std::fs::create_dir_all(moved.join(sub)).unwrap();
    }
    std::fs::copy(&std_file, moved.join("std").join("standard.solidcraft")).unwrap();
    std::fs::copy(&part_file, moved.join("parts").join("core.solidcraft")).unwrap();
    let mut copy = Session::default();
    run(&mut copy, "doc.open", json!({"path": moved.join("parts").join("core.solidcraft")}));
    let st = state(&mut copy);
    assert_eq!((st["state"].as_str(), st["file"].as_str()), (Some("up_to_date"), moved.join("std").join("standard.solidcraft").to_str()), "{st}");
    // Without the standard it still opens on the values last read, and says the file is missing.
    std::fs::remove_file(&std_file).unwrap();
    let mut gone = Session::default();
    let opened = run(&mut gone, "doc.open", json!({"path": part_file}));
    assert!(opened["warnings"][0].as_str().is_some_and(|w| w.contains("standard.solidcraft")), "{opened}");
    assert!(near(volume(&mut gone), insert_volume(8.2, 10.9)));
    assert_eq!(state(&mut gone)["state"], "missing");
    let before = gone.doc.clone();
    let e = err(&mut gone, "parameters.update_links", json!({}));
    assert!(e.contains("standard.solidcraft"), "{e}");
    assert_eq!(gone.doc, before);
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&moved);
}

/// The desktop app saves on a copy and adopts it; Save As into another folder, then undo past it,
/// still saves the linked design's path right (#5354 review).
#[test]
fn save_as_on_a_copy_and_undo_keep_the_link() {
    let dir = folder("save-as");
    let std_file = dir.join("std").join("standard.solidcraft");
    standard(&std_file);
    let first = dir.join("parts").join("core.solidcraft");
    let mut part = linked_part(&std_file);
    run(&mut part, "file.save", json!({"path": first}));
    run(&mut part, "parameters.add", json!({"name": "Local", "expression": "2 mm"}));
    // Save As into a deeper folder, the way the desktop app does it.
    std::fs::create_dir_all(dir.join("parts").join("sub")).unwrap();
    let second = dir.join("parts").join("sub").join("core.solidcraft");
    let mut copy = part.scratch();
    copy.path.clone_from(&part.path);
    run(&mut copy, "file.save", json!({"path": second}));
    part.adopt_save(&copy);
    assert_eq!(saved_paths(&second), ["../../std/standard.solidcraft"]);
    // Undo past the Save As brings back the earlier design; saving it still finds the standard.
    run(&mut part, "edit.undo", json!({}));
    run(&mut part, "file.save", json!({"path": second}));
    assert_eq!(saved_paths(&second), ["../../std/standard.solidcraft"]);
    let mut back = Session::default();
    run(&mut back, "doc.open", json!({"path": second}));
    assert_eq!(state(&mut back)["state"], "up_to_date");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A merged file holding a user parameter and a linked one of the same name opens: the model
/// uses the design's own value, as an edit would change it, and opening says so.
#[test]
fn a_file_holding_a_clash_opens_and_says_so() {
    let dir = folder("clash-file");
    let std_file = dir.join("std").join("standard.solidcraft");
    let part_file = dir.join("parts").join("core.solidcraft");
    standard(&std_file);
    let mut part = linked_part(&std_file);
    run(&mut part, "file.save", json!({"path": part_file}));
    // As a merge could leave it: Q_InsHole is this design's own (6 mm) and linked (8.2 mm).
    let mut doc = solidcraft_io::read_design(&std::fs::read(&part_file).unwrap()).unwrap();
    doc.params.push(solidcraft_doc::Param { name: "Q_InsHole".into(), expr: "6 mm".into(), unit: "mm".into(), comment: String::new(), model: false });
    std::fs::write(&part_file, solidcraft_io::write_design(&doc)).unwrap();
    let mut back = Session::default();
    let opened = run(&mut back, "doc.open", json!({"path": part_file}));
    let warnings = opened["warnings"].to_string();
    assert!(warnings.contains("Q_InsHole") && warnings.contains("this design's own is used"), "{opened}");
    assert!(near(volume(&mut back), insert_volume(6.0, 10.9)), "{}", volume(&mut back));
    assert_eq!(row(&mut back, "Q_InsHole")["source"], "user");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Extract to File (#5113) takes a component that uses linked parameters into its own design:
/// the new design links the same standard, relative to its own folder, and builds the same body.
#[test]
fn an_extracted_component_keeps_its_linked_parameters() {
    let dir = folder("extract");
    let std_file = dir.join("std").join("standard.solidcraft");
    standard(&std_file);
    let mut asm = Session::default();
    run(&mut asm, "file.save", json!({"path": dir.join("asm.solidcraft")}));
    run(&mut asm, "parameters.link_design", json!({"path": std_file}));
    run(&mut asm, "component.create", json!({"name": "Insert"}));
    run(&mut asm, "solid.cylinder", json!({"diameter": "Q_InsHole", "height": "Q_InsDepth", "body_name": "Insert"}));
    run(&mut asm, "component.activate", json!({"component": 0}));
    let before = volume(&mut asm);
    let file = dir.join("parts").join("insert.solidcraft");
    run(&mut asm, "component.extract", json!({"component": "Insert", "path": file}));
    assert_eq!(saved_paths(&file), ["../std/standard.solidcraft"]);
    let mut part = Session::default();
    let opened = run(&mut part, "doc.open", json!({"path": file}));
    assert!(opened.get("warnings").is_none(), "{opened}");
    assert_eq!(state(&mut part)["state"], "up_to_date");
    assert!(near(volume(&mut part), insert_volume(8.2, 10.9)));
    assert!(near(volume(&mut asm), before));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn relink_points_a_linked_design_at_its_new_file() {
    let dir = folder("relink");
    let std_file = dir.join("std").join("standard.solidcraft");
    let mut std = standard(&std_file);
    let mut part = linked_part(&std_file);
    // The standard moves and changes: the old file is missing, then Relink reads the new one.
    let moved = dir.join("std").join("standard-v2.solidcraft");
    run(&mut std, "parameters.change", json!({"name": "Q_InsHole", "expression": "8.5 mm"}));
    run(&mut std, "file.save", json!({"path": moved}));
    std::fs::remove_file(&std_file).unwrap();
    assert_eq!(state(&mut part)["state"], "missing");
    for bad in [json!({"path": dir.join("std").join("notes.txt")}), json!({"path": dir.join("std").join("none.solidcraft")}), json!({})] {
        let before = part.doc.clone();
        assert!(part.execute("parameters.relink", &bad).is_err(), "{bad}");
        assert_eq!(part.doc, before);
    }
    let r = run(&mut part, "parameters.relink", json!({"path": moved}));
    assert_eq!(r["changed"], json!([{"name": "Q_InsHole", "from": "8.2 mm", "to": "8.5 mm"}]), "{r}");
    assert!(near(volume(&mut part), insert_volume(8.5, 10.9)));
    assert_eq!(state(&mut part)["state"], "up_to_date");
    // A design that lacks a parameter in use here can't take its place.
    let bare = dir.join("std").join("bare.solidcraft");
    let mut b = Session::default();
    run(&mut b, "parameters.add", json!({"name": "Other", "expression": "1 mm"}));
    run(&mut b, "file.save", json!({"path": bare}));
    let e = err(&mut part, "parameters.relink", json!({"path": bare}));
    assert!(e.contains("Q_InsHole"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_design_cant_link_itself_or_close_a_loop() {
    let dir = folder("loop");
    let a_file = dir.join("std").join("a.solidcraft");
    let b_file = dir.join("parts").join("b.solidcraft");
    let mut a = standard(&a_file);
    let e = err(&mut a, "parameters.link_design", json!({"path": a_file}));
    assert!(e.contains("own"), "{e}");
    // b links a; a may not then link b, directly or through c.
    let mut b = Session::default();
    run(&mut b, "parameters.add", json!({"name": "B_only", "expression": "1 mm"}));
    run(&mut b, "file.save", json!({"path": b_file}));
    run(&mut b, "parameters.link_design", json!({"path": a_file}));
    run(&mut b, "file.save", json!({"path": b_file}));
    let e = err(&mut a, "parameters.link_design", json!({"path": b_file}));
    assert!(e.contains("loop"), "{e}");
    let c_file = dir.join("parts").join("c.solidcraft");
    let mut c = Session::default();
    run(&mut c, "parameters.add", json!({"name": "C_only", "expression": "1 mm"}));
    run(&mut c, "file.save", json!({"path": c_file}));
    run(&mut c, "parameters.link_design", json!({"path": b_file}));
    run(&mut c, "file.save", json!({"path": c_file}));
    let e = err(&mut a, "parameters.link_design", json!({"path": c_file}));
    assert!(e.contains("loop"), "{e}");
    assert!(a.doc.linked_designs.is_empty());
    let e = err(&mut b, "parameters.link_design", json!({"path": a_file}));
    assert!(e.contains("already"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn outside_an_agents_folders_is_a_state_not_a_failure() {
    let dir = folder("scope");
    let std_file = dir.join("std").join("standard.solidcraft");
    standard(&std_file);
    let mut part = linked_part(&std_file);
    let scope = solidcraft_io::vfs::PathScope::new([dir.join("parts")]);
    solidcraft_io::vfs::scoped(Some(scope), || {
        assert_eq!(state(&mut part)["state"], "not_allowed");
        assert!(part.execute("parameters.update_links", &json!({})).is_err());
    });
    assert_eq!(state(&mut part)["state"], "up_to_date");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn break_link_keeps_the_values_as_user_parameters() {
    let dir = folder("break");
    let std_file = dir.join("std").join("standard.solidcraft");
    standard(&std_file);
    let mut part = linked_part(&std_file);
    let broken = run(&mut part, "parameters.break_link", json!({}));
    assert_eq!(broken["parameters"].as_array().map(Vec::len), Some(7), "{broken}");
    assert!(part.doc.linked_designs.is_empty());
    let r = row(&mut part, "Q_InsDepth");
    assert_eq!(
        (r["source"].as_str(), r["expression"].as_str(), r["comment"].as_str()),
        (Some("user"), Some("10.9 mm"), Some("1/4-20 insert hole depth"))
    );
    assert!(near(volume(&mut part), insert_volume(8.2, 10.9)));
    run(&mut part, "parameters.change", json!({"name": "Q_InsHole", "expression": "6 mm"}));
    assert!(near(volume(&mut part), insert_volume(6.0, 10.9)));
    assert!(err(&mut part, "parameters.break_link", json!({})).contains("links no parameters"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_exported_script_links_the_same_design_first() {
    let dir = folder("script");
    let std_file = dir.join("std").join("standard.solidcraft");
    standard(&std_file);
    let mut part = linked_part(&std_file);
    run(&mut part, "parameters.add", json!({"name": "Twice", "expression": "Q_InsHole * 2"}));
    let script = run(&mut part, "timeline.export_script", json!({}));
    let steps = script["script"]["commands"].as_array().cloned().unwrap_or_default();
    assert_eq!(steps.first().map(|s| &s["command"]), Some(&json!("parameters.link_design")), "{script}");
    // Replayed into a new design, it builds the same thing.
    let mut replay = Session::default();
    for step in &steps {
        run(&mut replay, step["command"].as_str().unwrap_or_default(), step["params"].clone());
    }
    assert!(close(volume(&mut replay), volume(&mut part)));
    assert_eq!(row(&mut replay, "Twice")["value"], json!(16.4));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn hostile_link_params_never_panic() {
    let mut s = Session::default();
    for (id, p) in [
        ("parameters.link_design", json!({})),
        ("parameters.link_design", json!({"path": 7})),
        ("parameters.link_design", json!({"path": ""})),
        ("parameters.link_design", json!({"path": "x".repeat(5000)})),
        ("parameters.link_design", json!({"path": "/no/such/design.solidcraft"})),
        ("parameters.link_design", json!({"path": "/no/such/design.step"})),
        ("parameters.link_design", json!({"path": "/no/such/design.solidcraft", "scope": "some"})),
        ("parameters.link_design", json!({"path": format!("{}x.solidcraft", "../".repeat(64))})),
        ("parameters.update_links", json!({})),
        ("parameters.update_links", json!({"design": 5})),
        ("parameters.update_links", json!({"design": ""})),
        ("parameters.relink", json!({"path": "/no/such/design.solidcraft"})),
        ("parameters.break_link", json!({"design": "nothing"})),
        ("parameters.break_link", json!({"design": null})),
    ] {
        assert!(s.execute(id, &p).is_err(), "{id} {p}");
    }
    assert_eq!(run(&mut s, "parameters.links", json!({})), json!({"designs": []}));
    assert!(s.doc.linked_designs.is_empty() && s.doc.params.is_empty());
}
